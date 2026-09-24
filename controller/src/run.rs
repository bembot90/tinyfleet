//! `fleet observe`: the poll loop. It observes, decides, acts and publishes.

use crate::adapter::claude_code::ClaudeCode;
use crate::adapter::{dir_key, Agent};
use crate::clock::{self, Clock, SystemClock};
use crate::config::{self, MachineConfig, Seat};
use crate::decide::{self, FleetShape, SeatInput, Verdict};
use crate::effect::{self, Outcome, Target};
use crate::events::{self, EventLog, CONTROLLER};
use crate::observe::{self, RosterState, SeatObservation};
use crate::platform;
use crate::policy::{self, Policy};
use crate::projection::{self, InFlight, PolicyView, Projection, SeatRow, SeatView};
use crate::routines;
use crate::sessions::{self, SeatState, Table};
use fleet_core::seat::identity::SeatId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Startup could not read what it needs. There is no last-good before the first
/// read, so the only honest answer is to refuse and name the path.
pub const EXIT_NO_POLICY: u8 = 3;

pub struct Options {
    /// One poll, then exit: what a check runs.
    pub once: bool,
}

/// What one seat's events this tick asked for.
#[derive(Default, Clone)]
struct Pending {
    /// An unconsumed `seat.resting`.
    rest: bool,
    /// An unconsumed `seat.resting` or `seat.exited` — the deliberate end the
    /// roster cannot report (lessons claude-code A3).
    deliberate_end: bool,
    /// An unconsumed `seat.clear_halt` — a person's request to lift the blind
    /// guard, consumed on this tick exactly as a rest is.
    clear_halt: bool,
    /// The sequence of the OLDEST unconsumed `seat.resting`. A tick that did not
    /// collect the rest holds the cursor below this line, so the next tick reads
    /// the same event and tries again — which is what "the rest stays pending"
    /// means on a file the cursor walks forward through.
    rest_seq: Option<u64>,
}

pub fn observe(options: &Options) -> u8 {
    observe_with(
        options,
        platform::Grant::new(platform::directory_listing(), platform::GRANT_PROBE_TIMEOUT),
    )
}

/// The loop with the file-access gate handed in and no run seam.
///
/// The gate's seam exists because the thing it guards cannot be raised from
/// inside a suite: the dialog needs the service loaded in a desktop session and
/// a reset aimed at the identifier of the service currently running this fleet
/// (lessons claude-code D4). Above it is [`observe`], which hands in the
/// platform's own listing and the D4 band.
pub fn observe_with(options: &Options, grant: platform::Grant) -> u8 {
    observe_runs(options, grant, None)
}

/// The loop with the RUN seam handed in beside the gate.
///
/// The seam exists because this crate takes nothing from core but its bounded
/// runner and the release it supports — no store, no packs, no policy reader —
/// and the three acts a run's advance needs are wired in the binary. `None` is
/// a loop that knows nothing of runs and polls exactly as it did before they
/// existed.
pub fn observe_runs(
    options: &Options,
    grant: platform::Grant,
    runs: Option<&dyn crate::runs::Runs>,
) -> u8 {
    observe_clocked(options, grant, runs, &SystemClock)
}

/// The loop with the clock it spends its POLL INTERVAL against handed in too.
///
/// The clock reaches exactly one site — [`nap`], the wait between ticks — which
/// is the only wait on this path whose subject is a DURATION. Every other wait
/// the tick can reach bounds an external fact: a child's exit, a pipe's drain, a
/// permission dialog's answer. No clock hurries one of those.
///
/// It resolves [`Wiring`] — one resolution, taken before the first tick — and
/// hands its seams to [`observe_seamed`].
pub fn observe_clocked(
    options: &Options,
    grant: platform::Grant,
    runs: Option<&dyn crate::runs::Runs>,
    clock: &dyn Clock,
) -> u8 {
    let wiring = Wiring::resolve();
    observe_seamed(
        options,
        grant,
        runs,
        wiring.seams(clock, StopHandler::Armed),
    )
}

/// What the loop acts through that somebody has to OWN: the agent this fleet
/// runs, the `PATH` a routine's children carry, and why no effect may be issued.
///
/// [`Seams`] borrows all three, so a caller driving ticks itself holds one of
/// these for as long as its [`Observer`] lives.
pub struct Wiring {
    agent: ClaudeCode,
    child_path: String,
    effects_off: Option<String>,
}

impl Wiring {
    /// The agent this fleet runs, built from the environment, with the binary
    /// its effects exec resolved.
    pub fn resolve() -> Wiring {
        let machine_dir = platform::machine_dir();
        let mut agent = ClaudeCode::new(&platform::home_dir(), &machine_dir);
        // The binary an EFFECT execs, resolved once and never by bare name.
        // Unresolvable is not fatal: the loop observes and publishes with
        // effects off and the projection carries the cause, which is the shape
        // the grant gate has too.
        //
        // ONE RESOLUTION, TWO USERS. The value below is both the gate the loop
        // reads and the binary the adapter execs, because a controller that
        // gated on one file and acted through another would issue effects nobody
        // checked — which is what a service-launched process, whose own `PATH`
        // is not the operator's shell, would do on every start.
        //
        // The cause travels to the loop rather than being said here, so the line
        // it prints keeps its place in the order a reader meets the startup's
        // lines in.
        let effect_bin = ClaudeCode::resolve_effect_bin(
            crate::adapter::claude_code::configured_bin().as_deref(),
            &agent.child_path,
        );
        if let Ok(bin) = &effect_bin {
            agent = agent.with_effect_bin(bin.clone());
        }
        let child_path = agent.child_path.clone();
        Wiring {
            agent,
            child_path,
            effects_off: effect_bin.err(),
        }
    }

    /// This wiring plus the clock the poll interval is spent against and
    /// whether the loop arms the process's stop handlers.
    ///
    /// The handler choice is an ARGUMENT and never a default: the value decides
    /// what a SIGTERM does to the whole process, and a caller that did not state
    /// it would be deciding that by omission.
    pub fn seams<'a>(&'a self, clock: &'a dyn Clock, stop_handler: StopHandler) -> Seams<'a> {
        Seams {
            clock,
            agent: &self.agent,
            child_path: &self.child_path,
            effects_off: self.effects_off.clone(),
            stop_handler,
        }
    }
}

/// Whether the loop arms this PROCESS's SIGINT and SIGTERM handlers.
///
/// A handler set here outlives the loop and belongs to the whole process, so a
/// suite driving the loop in-process asks for [`StopHandler::Unarmed`]: an
/// armed test binary answers the harness's SIGTERM by setting a flag and living
/// on, and is then killed at the grace period instead of at the signal. Such a
/// suite ends its own run with `platform::request_stop`, which needs no
/// signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopHandler {
    Armed,
    Unarmed,
}

/// Everything the loop ACTS THROUGH, gathered by the caller.
///
/// The search path is its own field rather than a verb on [`Agent`]: it is the
/// environment a child of an ORDER carries, and the agent is not the thing that
/// runs those.
pub struct Seams<'a> {
    pub clock: &'a dyn Clock,
    pub agent: &'a dyn Agent,
    /// The `PATH` every child a routine's action starts carries.
    pub child_path: &'a str,
    /// Why no effect may be issued, or `None` for a fleet that can issue them.
    /// The binary those effects exec is resolved by the caller, so the gate this
    /// loop reads and the file the adapter execs are one value.
    pub effects_off: Option<String>,
    /// Whether [`Observer::start`] arms the process's stop handlers.
    pub stop_handler: StopHandler,
}

/// The loop with the clock, the agent and the search path a routine's children
/// carry ALL handed in: one tick or many, against no process and no wall clock
/// but the ones the caller supplies.
pub fn observe_seamed(
    options: &Options,
    grant: platform::Grant,
    runs: Option<&dyn crate::runs::Runs>,
    seams: Seams<'_>,
) -> u8 {
    let mut observer = match Observer::start(grant, runs, seams) {
        Ok(observer) => observer,
        Err(code) => return code,
    };
    loop {
        observer.tick();
        if options.once || platform::stop_requested() {
            break;
        }
        if !observer.nap() {
            break;
        }
    }
    observer.finish()
}

/// The loop's own state: what one poll leaves for the next.
///
/// A value here OUTLIVES a poll — the policy in force, the mtimes a re-read is
/// gated on, the once-per-change latches, the session table — and a value a
/// poll computes for itself is a local inside [`Observer::tick`]. The line
/// between the two is what makes the once-per-change clauses true: a latch left
/// inside the tick is re-armed every poll and says its line again, and a
/// per-poll reading lifted up here is the last poll's answer standing in for
/// this one's.
pub struct Observer<'a> {
    seams: Seams<'a>,
    runs: Option<&'a dyn crate::runs::Runs>,
    /// The file-access gate, whose own probe state lives across polls: one that
    /// outran its bound is read again rather than started again.
    grant: platform::Grant,
    machine_dir: PathBuf,
    config_path: PathBuf,
    table_path: PathBuf,
    /// The seat list's mtime as of the last read of it, and the document that
    /// read produced.
    config_seen: Option<std::time::SystemTime>,
    raw_config: MachineConfig,
    /// The policy file in force and its mtime: the path moves when the seat
    /// list re-points it, and an mtime is comparable only against the same path.
    policy_path: PathBuf,
    policy_mtime: Option<std::time::SystemTime>,
    file_policy: Policy,
    unknown_override_said: BTreeSet<String>,
    policy: Policy,
    config: MachineConfig,
    /// The mtime the "did the policy file move?" test compares against, and the
    /// last-good policy's parse error while one stands.
    policy_seen: Option<std::time::SystemTime>,
    policy_error: Option<String>,
    events_log: EventLog,
    /// The spread announced and not yet closed. Only a version that WAS READ
    /// and agrees with the pin clears it.
    announced_move: Option<(Option<String>, Option<String>)>,
    table: Table,
    /// The table as the FILE last had it, which is what tells this loop's own
    /// moves from another writer's: a row that differs from this is one this
    /// loop moved, and every other row belongs to whoever wrote the file.
    table_base: Table,
    /// The cause the last unreadable table gave. Once per cause, never once per
    /// poll.
    table_said: Option<String>,
    adopted: bool,
    /// The seats standing in a hold, whichever of the two holds it is: the line
    /// is written once per transition into one and not once per poll.
    held: BTreeSet<SeatId>,
    /// When this controller last read a LIVE row for each seat, in epoch
    /// milliseconds.
    ///
    /// THIS PROCESS'S OWN MEMORY and not the table's: the table records what
    /// this fleet DISPATCHED, and the session whose host is replaced under a
    /// controller is typically one another manager started, which no row names
    /// and `Table::sight` therefore records nothing for. A restart starts this
    /// empty, which is the seat's own pre-hold verdict and never a hold nobody
    /// can account for.
    live_seen: BTreeMap<SeatId, u64>,
    /// When this controller read the FIRST pid-less poll of the run of them each
    /// seat is currently standing in, in epoch milliseconds.
    ///
    /// Written at the poll a seat's row goes pid-less and REMOVED at the poll it
    /// is live again, so the value is always the start of an unbroken run and
    /// never the start of an older one. It is the clock the re-host hold's
    /// window is measured on, because the interval between two polls is not the
    /// poll interval: a run's steps execute on this thread, and the poll after a
    /// twenty-minute land step holds a live sighting twenty minutes old for
    /// every seat on the fleet — the reading the poll after a run's land step turns on.
    pidless_since: BTreeMap<SeatId, u64>,
    upgrade_shape: Option<FleetShape>,
    rest_failed_said: BTreeSet<String>,
    routines_state_said: BTreeSet<String>,
    grant_said: Option<String>,
}

impl<'a> Observer<'a> {
    /// Read what the loop needs before its first poll, or refuse and name the
    /// path.
    ///
    /// `Err` carries the exit status the caller returns; there is no last-good
    /// before the first read, so a startup that cannot read is not a tick that
    /// went quiet.
    pub fn start(
        grant: platform::Grant,
        runs: Option<&'a dyn crate::runs::Runs>,
        seams: Seams<'a>,
    ) -> Result<Observer<'a>, u8> {
        let machine_dir = platform::machine_dir();
        let config_path = machine_dir.join("config.json");
        // Stat BEFORE the read, both here and in the loop: an edit that lands in
        // the window between them is recorded as already seen and never re-read.
        let config_seen = config::mtime(&config_path);
        let raw_config = match config::read(&config_path) {
            Ok(config) => config,
            Err(why) => {
                eprintln!("fleet observe: cannot read the seat list at {why}");
                return Err(EXIT_NO_POLICY);
            }
        };
        let policy_path = raw_config.fleet_toml.clone();
        let policy_mtime = policy::mtime(&policy_path);
        // The FILE's policy and the EFFECTIVE one are two values: `config.json` is
        // local and beats policy per key, so the overlay is recomputed
        // wherever either document is re-read, and the file's own value is what the
        // "did policy move?" test compares.
        let file_policy = match policy::load(&policy_path) {
            Ok(policy) => policy,
            Err(why) => {
                eprintln!("fleet observe: cannot read policy at {why}");
                return Err(EXIT_NO_POLICY);
            }
        };
        // Once per key, never once per poll.
        let mut unknown_override_said: BTreeSet<String> = BTreeSet::new();
        let policy = overlaid(&file_policy, &raw_config, &mut unknown_override_said);
        let config = admitted(&raw_config, &policy);
        report_skipped(&config);

        let policy_seen = policy_mtime;
        let policy_error: Option<String> = None;

        let mut events_log = EventLog::open(&machine_dir.join("events.jsonl"));
        let announced_move: Option<(Option<String>, Option<String>)> = None;

        if let Some(why) = &seams.effects_off {
            eprintln!("fleet observe: effects are off — {why}");
        }

        let table_path = sessions::path_in(&machine_dir);
        let (on_disk, table_error) = sessions::read(&table_path);
        // A table that is missing, unparseable or of a schema this build refuses is
        // REBUILT from the stream rather than started from empty: every row
        // and every counter it carries was written to the stream by the layer that
        // did the thing, so the table is derivable and never a second source of
        // truth. An empty one here would forget a standing halt.
        //
        // The trigger is the ABSENCE OF A TABLE and not the presence of a fault: a
        // deleted file is a read that found nothing rather than one that failed, and
        // a controller that rebuilt only on the fault would start empty on exactly
        // the loss the hold exists to survive. A first-ever start rebuilds too, from
        // a stream that is not there either, which is the same empty table it would
        // otherwise have made.
        let table = match on_disk {
            Some(table) => table,
            None => {
                let rebuilt = sessions::rebuild(&machine_dir.join("events.jsonl"));
                eprintln!(
                    "fleet observe: the session table was rebuilt from the event stream — \
                     {} row(s), cursor {}; {}",
                    rebuilt.sessions.len(),
                    rebuilt.consumed_seq,
                    table_error.unwrap_or_else(|| format!("{} is not there", table_path.display()))
                );
                rebuilt
            }
        };
        // Every session the table names that the roster still holds live is claimed
        // on the FIRST poll and never again — a restart re-hosts nothing.
        let adopted = false;
        // Once per transition into a hold, and once per transition into the
        // upgrade shape: both are observations, not decisions, and a line per
        // poll would drown the one a person came to read.
        let held: BTreeSet<SeatId> = BTreeSet::new();
        // No seat has been seen live before the first poll, so the first poll
        // decides every seat exactly as it did before this memory existed.
        let live_seen: BTreeMap<SeatId, u64> = BTreeMap::new();
        // And no seat is standing in a run of pid-less polls until one is read.
        let pidless_since: BTreeMap<SeatId, u64> = BTreeMap::new();
        let upgrade_shape: Option<FleetShape> = None;
        // For a rest whose stop failed: the retry is silent and the first
        // failure is loud.
        let rest_failed_said: BTreeSet<String> = BTreeSet::new();
        // And for a routines state that will not read: once per cause, never once
        // per poll.
        let routines_state_said: BTreeSet<String> = BTreeSet::new();

        // The gate's state lives across polls: a probe that outran its bound is read
        // again rather than started again. It is built by the caller, BEFORE the
        // first poll, so the dialog it raises is answered in the minute the service
        // loads (lessons claude-code D4).
        //
        // Once per transition into pending, never once per poll.
        let grant_said: Option<String> = None;

        if seams.stop_handler == StopHandler::Armed {
            platform::install_stop_handler();
        }
        log_event(
            &mut events_log,
            events::CONTROLLER_STARTED,
            CONTROLLER,
            serde_json::json!({
                "controller_version": env!("CARGO_PKG_VERSION"),
                "machine_dir": machine_dir.display().to_string(),
                "policy": policy_path.display().to_string(),
                "seats": config.seats.len(),
                "effects": seams.effects_off.is_none(),
                "plugin_dir": policy.plugin_dir.as_ref().map(|dir| dir.display().to_string()),
            }),
        );
        Ok(Observer {
            seams,
            runs,
            grant,
            machine_dir,
            config_path,
            table_path,
            config_seen,
            raw_config,
            policy_path,
            policy_mtime,
            file_policy,
            unknown_override_said,
            policy,
            config,
            policy_seen,
            policy_error,
            events_log,
            announced_move,
            table_base: table.clone(),
            table_said: None,
            table,
            adopted,
            held,
            live_seen,
            pidless_since,
            upgrade_shape,
            rest_failed_said,
            routines_state_said,
            grant_said,
        })
    }

    /// One poll: observe, decide, act and publish once, against the state this
    /// [`Observer`] carries.
    ///
    /// A caller drives it on its own thread, editing what the loop reads between
    /// calls. Nothing here waits on the clock — the interval between ticks is
    /// [`Observer::nap`]'s.
    pub fn tick(&mut self) {
        let agent = self.seams.agent;
        // config.json is re-read whenever its mtime moves. A re-read that cannot
        // parse leaves the seat list standing: the empty fleet a broken file
        // parses to is a configuration nobody asked for.
        let config_mtime = config::mtime(&self.config_path);
        let mut re_gate = false;
        // Set when the seat list re-points `fleet_toml`, and read by the policy
        // block below: the new file is read whatever its mtime happens to be,
        // since an mtime is only comparable against the same path. PER POLL and
        // not a field, because the block that reads it is entered on every path
        // that sets it, so no tick ever starts with it true.
        let mut policy_repointed = false;
        if config_mtime != self.config_seen {
            self.config_seen = config_mtime;
            match config::read(&self.config_path) {
                Ok(fresh) => {
                    if fresh.fleet_toml != self.policy_path {
                        eprintln!(
                            "fleet observe: the seat list re-points policy to {}",
                            fresh.fleet_toml.display()
                        );
                        self.policy_path = fresh.fleet_toml.clone();
                        policy_repointed = true;
                    }
                    self.raw_config = fresh;
                    re_gate = true;
                }
                Err(why) => eprintln!("fleet observe: the seat list stands; {why}"),
            }
        }

        let file_mtime = policy::mtime(&self.policy_path);
        if policy_repointed || file_mtime != self.policy_seen {
            self.policy_seen = file_mtime;
            match policy::load(&self.policy_path) {
                Ok(fresh) => {
                    if fresh != self.file_policy {
                        eprintln!(
                            "fleet observe: policy re-read from {}",
                            self.policy_path.display()
                        );
                    }
                    self.file_policy = fresh;
                    self.policy_mtime = file_mtime;
                    self.policy_error = None;
                    // The gate is the policy's list against the row's model, so
                    // a policy that moved re-decides which rows stand.
                    re_gate = true;
                }
                Err(why) => {
                    // Once per change, never once per poll.
                    eprintln!("fleet observe: running on last-good policy; {why}");
                    self.policy_error = Some(why);
                }
            }
        }
        if re_gate {
            self.policy = overlaid(
                &self.file_policy,
                &self.raw_config,
                &mut self.unknown_override_said,
            );
            self.config = admitted(&self.raw_config, &self.policy);
            report_skipped(&self.config);
        }

        // ONE LISTING PER DISTINCT CONFIGURATION DIRECTORY, and the fleet's own
        // beside them. A seat started under its own directory is held by its
        // own daemon and named by no other listing, so a poll that read once
        // would publish every spawned seat absent.
        //
        // The directories come off the session table, which is what the
        // controller remembers about what it started: the seat list carries no
        // such field, and a directory derived from the seat's name here would be
        // a second spelling that a spawn could already have decided otherwise.
        let recorded_dirs: BTreeMap<SeatId, String> = self
            .config
            .seats
            .iter()
            .filter_map(|seat| {
                self.table
                    .newest_for(&seat.id.to_string())
                    .and_then(|row| row.config_dir.clone())
                    .map(|dir| (seat.id, dir))
            })
            .collect();
        let rosters = observe::Rosters::gather(
            &self.config.seats,
            &|seat: &SeatId| recorded_dirs.get(seat).cloned(),
            &|dir: Option<&Path>| agent.status(dir),
        );
        let agent_version = agent.version();
        // Through the clock seam, so the windows a rig drives the loop across
        // age with the fake time its naps spend.
        let now_ms = self.seams.clock.now_ms();
        let mut table_moved = false;

        // The daemon, read ONCE for the whole poll exactly as the roster is: two
        // seats must not decide against different beliefs about whether the
        // fleet is mid-replacement.
        //
        // A pid this read could not take does not overwrite the one the last
        // poll recorded: forgetting it would make the next good read compare
        // against nothing and miss the replacement it is there to catch.
        let daemon = agent.daemon();
        let daemon_pid_changed = match (self.table.daemon_pid, daemon.pid()) {
            (Some(was), Some(now)) => was != now,
            // Neither "not running" nor "could not read" is a change.
            _ => false,
        };
        if let Some(pid) = daemon.pid() {
            if self.table.daemon_pid != Some(pid) {
                self.table.daemon_pid = Some(pid);
                table_moved = true;
            }
        }

        // Adoption, on the first poll and against the roster this poll read.
        // Not gated on effects: it issues none, and a controller that could not
        // exec the agent still knows which sessions it owns.
        //
        // ONCE PER PROCESS is the WRITE and never the reading: the claim goes on
        // the row, and the verdicts below ask the table for it every poll. A
        // term filled from what this call claimed would be false on every poll
        // after the first, which is the whole of the defect it exists to close.
        if !self.adopted {
            self.adopted = true;
            // EVERY listing's rows, folded: a session under a per-row directory
            // is in that directory's listing alone, and a controller that
            // restarted has to be able to claim it like any other.
            {
                let rows = rosters.all_rows();
                let claimed = effect::adopt(&rows, &mut self.table, &mut self.events_log, now_ms);
                if !claimed.is_empty() {
                    table_moved = true;
                    eprintln!(
                        "fleet observe: adopted {} session(s) the table names: {}",
                        claimed.len(),
                        claimed.join(", ")
                    );
                }
            }
        }

        // The stopped-row window, from policy, with the end each row is judged
        // on resolved through the same adapter and the same worktree spelling
        // the context read below uses.

        // The stream, from the line after the cursor. Read BEFORE deciding, so
        // what a seat asked for between polls is in hand when its verdict is
        // reached.
        let known: BTreeSet<SeatId> = self.config.seats.iter().map(|s| s.id).collect();
        let transient: BTreeSet<SeatId> = self
            .config
            .seats
            .iter()
            .filter(|s| s.transient)
            .map(|s| s.id)
            .collect();
        let stream = events::read_after(
            &self.machine_dir.join("events.jsonl"),
            self.table.consumed_seq,
        );
        let read_to = stream.iter().map(|record| record.seq).max();
        let pending = fold(&stream, &known, &transient);

        let mut observations: Vec<(usize, SeatObservation, Option<u64>)> = Vec::new();
        let mut seats = Vec::with_capacity(self.config.seats.len());
        let mut logged_out: Vec<(SeatView, String, Option<String>)> = Vec::new();
        for (index, seat) in self.config.seats.iter().enumerate() {
            let machine_name = seat.machine_name();
            // What the session table and the projection key this seat on.
            let key = seat.id.to_string();
            // The seat's own directory, threaded through every read about it:
            // the listing that can see it, the end its stopped-row window is
            // judged on, and the transcript its context is read from.
            let under: Option<PathBuf> = recorded_dirs.get(&seat.id).map(PathBuf::from);
            // The stopped-row window, from policy, with the end each row is
            // judged on resolved through the same adapter and the same worktree
            // spelling the context read below uses.
            let recency = observe::Recency {
                window_ms: self.policy.stopped_recency_hours * 60 * 60 * 1000,
                ended_at: &|worktree: &str, session_id: &str| {
                    agent.ended_at(under.as_deref(), dir_key(worktree), session_id)
                },
            };
            let observation =
                observe::observe_seat(rosters.for_seat(&seat.id), seat, now_ms, &recency);
            // Context comes from the transcript and never from the listing,
            // which carries no token field (lessons claude-code B2). An ended
            // row still answers, because the transcript outlives the process.
            //
            // The worktree is the CONFIGURED spelling, so it goes through the
            // same normalisation the seat match uses: the agent keys its
            // transcript directory on the directory itself, and a configured
            // trailing separator encodes to one the agent never wrote.
            let body = match &observation.session_id {
                Some(session_id) if observation.state.has_context_reading() => {
                    observation.worktree.as_deref().and_then(|worktree| {
                        agent.transcript(under.as_deref(), dir_key(worktree), session_id)
                    })
                }
                _ => None,
            };
            let context_tokens = body.as_deref().and_then(observe::context_tokens_in);
            // A seat that came up LOGGED OUT. The roster cannot say so — a
            // logged-out session is live and idle, with a pid, exactly like one
            // waiting for work — so the transcript is the only surface that
            // carries the reading, and it is read from the same body the context
            // came from rather than from a second read that could disagree.
            //
            // Once per ROW, at its first sighting: the reading stands for as long
            // as the transcript does, and a line per poll is the noise the stream
            // rule is against.
            let sighted = self
                .table
                .newest_for(&key)
                .is_none_or(|row| row.first_seen_at.is_some());
            if observe::logged_out_dispatch(
                seat.transient,
                observation.state,
                sighted,
                body.as_deref(),
            ) {
                logged_out.push((
                    SeatView::from(&seat.as_ref()),
                    machine_name.clone(),
                    self.table.newest_for(&key).and_then(|row| row.item.clone()),
                ));
            }
            // A sighting answers a dispatch's arrival window, and it is the
            // ROSTER's answer rather than the start's own return (A7). Only a
            // LIVE row is one: a pid-less row is a session that has ended or has
            // not started, and neither is a seat that arrived.
            if matches!(
                observation.state,
                RosterState::Present | RosterState::PromptBlocked
            ) {
                // The same reading the sighting below is taken on, kept for
                // every seat rather than for the rows this controller opened:
                // the session whose host is replaced under a poll is usually
                // one another manager started, and `sight` records nothing for
                // a session no row names. It is written BEFORE the verdicts and
                // only on a live row, so the seat it decides about — a pid-less
                // one — reads the stamp an earlier poll left.
                self.live_seen.insert(seat.id, now_ms);
                if let (Some(session_id), Some(worktree)) =
                    (&observation.session_id, &observation.worktree)
                {
                    table_moved |= self.table.sight(
                        &key,
                        dir_key(worktree),
                        session_id,
                        observation.short_id.as_deref(),
                        now_ms,
                    );
                }
            }
            // The run of pid-less polls this seat stands in, opened at the poll
            // its row goes pid-less and closed at the poll the row reads
            // anything else. The hold's window is measured from the open, so a
            // row that came back and went again is a new run rather than the old
            // one's tail — and a seat the loop did not look at for twenty
            // minutes opens its run at the poll that finally looked.
            match observation.state {
                RosterState::Stopped | RosterState::Absent => {
                    self.pidless_since.entry(seat.id).or_insert(now_ms);
                }
                _ => {
                    self.pidless_since.remove(&seat.id);
                }
            }
            // The row names the seat as its object: the id, the seat's own name
            // where it has one, and its kind.
            seats.push(SeatRow::from_observation(
                seat,
                &observation,
                context_tokens,
            ));
            observations.push((index, observation, context_tokens));
        }

        // The logged-out dispatches, one line each. WRITTEN AND NOTHING
        // ELSE: holding the item and retiring the seat are a workflow's, and a
        // controller that acted here would be deciding a run's business from
        // inside the poll.
        for (seat, machine_name, item) in &logged_out {
            if let Err(e) = self.events_log.append(
                events::DISPATCH_FAILED,
                &seat.id,
                events::dispatch_failed_payload(
                    seat,
                    item.as_deref(),
                    observe::AUTHENTICATION_FAILED,
                ),
            ) {
                eprintln!(
                    "fleet observe: {machine_name} came up logged out and the line could not be \
                     appended: {e}"
                );
            }
        }

        // One verdict per seat, from terms this poll reads for itself — nothing
        // here is a restored belief, so the answer survives a restart. The halt
        // latch and the blind count are slice 4's, so they are passed as the
        // constants they are here rather than left unstated.
        // The upgrade shape: one line rather than N independent absences, once
        // per transition into it. Reported like an observation and not
        // like a decision, and threaded into no seat's verdict.
        let shape = decide::fleet_shape(
            &observations
                .iter()
                .map(|(_, observation, _)| observation.state)
                .collect::<Vec<_>>(),
        );
        if shape != self.upgrade_shape {
            if let Some(shape) = &shape {
                eprintln!("fleet observe: {}", shape.describe());
            }
            self.upgrade_shape = shape;
        }

        // The clear-halt requests, consumed BEFORE the verdicts, so a halt
        // a person cleared does not survive one more poll and dispatch nothing
        // for another interval.
        for seat in &self.config.seats {
            if !pending
                .get(&seat.id)
                .map(|asked| asked.clear_halt)
                .unwrap_or(false)
            {
                continue;
            }
            let machine_name = seat.machine_name();
            let key = seat.id.to_string();
            let was = self.table.seat_state(&key);
            if was.blind == 0 && !was.halted {
                continue;
            }
            self.table.set_seat_state(&key, SeatState::default());
            table_moved = true;
            eprintln!(
                "fleet observe: {machine_name}'s blind counter reset from {} and its halt lifted \
                 by request",
                was.blind
            );
        }

        let mut verdicts: Vec<Verdict> = Vec::with_capacity(observations.len());
        for (index, observation, context_tokens) in &observations {
            let seat = &self.config.seats[*index];
            let machine_name = seat.machine_name();
            let key = seat.id.to_string();
            let asked = pending.get(&seat.id).cloned().unwrap_or_default();
            let newest = self.table.newest_for(&key);
            let carried = self.table.seat_state(&key);
            let input = SeatInput {
                seat_dir: &machine_name,
                state: observation.state,
                unknown_cause: observation.unknown_cause.as_deref(),
                transient: seat.transient,
                pending_rest: asked.rest,
                pending_deliberate_end: asked.deliberate_end,
                context_tokens: *context_tokens,
                rest_threshold_tokens: self.policy.rest_threshold_tokens,
                session_id: observation.session_id.as_deref(),
                already_nudged: observation
                    .session_id
                    .as_deref()
                    .map(|id| self.table.is_nudged(&key, id))
                    .unwrap_or(false),
                dispatch_age_ms: newest.map(|row| now_ms.saturating_sub(row.dispatched_at)),
                sighted: newest.map(|row| row.session_id.is_some()).unwrap_or(false),
                // The claim from the table, which carries it across polls and
                // across restarts, and the listing from THIS poll's own row:
                // a session whose row NAMES an end is not owned any more,
                // however long ago it was claimed. A pid-less row reading
                // `done` is not such a row — it is hibernation, a stop from
                // idle and a kill in one reading — so the claim holds it and
                // the controller's own record ends it.
                adopted_and_listed: !observation.state_names_an_end
                    && observation
                        .session_id
                        .as_deref()
                        .map(|id| self.table.is_adopted(id))
                        .unwrap_or(false),
                arrival_window_ms: self.policy.arrival_window_seconds * 1000,
                halted: carried.halted,
                blind: carried.blind,
                pidless_row: observation.pidless_row,
                daemon_pid_changed,
                daemon_uptime_ms: daemon.uptime_ms(),
                // This controller's own earlier sighting, which is what tells a
                // session being re-hosted from one that is gone while the
                // daemon holding it stands unchanged — and the age of the
                // pid-less run itself, which is what the window is spent on.
                seen_live: self.live_seen.contains_key(&seat.id),
                since_pidless_ms: self
                    .pidless_since
                    .get(&seat.id)
                    .map(|at| now_ms.saturating_sub(*at)),
            };
            let verdict = decide::decide(&input);
            // The hold's own line, from the same function the arm reaches its
            // verdict through, once per transition into it.
            match decide::hold(&input) {
                Some(why) if self.held.insert(seat.id) => {
                    eprintln!("fleet observe: {why}")
                }
                None => {
                    self.held.remove(&seat.id);
                }
                _ => {}
            }
            seats[*index].decision = verdict.as_str().to_string();
            seats[*index].blind = carried.blind;
            seats[*index].halted = carried.halted;
            verdicts.push(verdict);
        }

        // The gate, read every poll while it is pending. Every configured
        // worktree of every seat, in one listing each.
        let probed: Vec<PathBuf> = self
            .config
            .seats
            .iter()
            .flat_map(|seat| seat.worktrees.iter().map(|(_, path)| PathBuf::from(path)))
            .collect();
        let grant_read = self.grant.poll(&probed);
        if self.grant_said.as_deref() != grant_read.detail.as_deref() {
            if let Some(detail) = &grant_read.detail {
                eprintln!(
                    "fleet observe: the file-access grant is pending, so no effect is issued — \
                     {detail}"
                );
            }
            self.grant_said = grant_read.detail.clone();
        }

        let effects = projection::effects_of(
            grant_read.detail.as_deref(),
            self.seams.effects_off.as_deref(),
        );
        // A pending grant holds every effect, so the blind counter does not move
        // either: a dispatch nobody issued is not a dispatch nobody answered.
        let acting = effects.acting;

        let mut document = Projection {
            version: projection::VERSION,
            generated_at: clock::now_stamp(),
            controller_version: env!("CARGO_PKG_VERSION").to_string(),
            agent_version: agent_version.clone(),
            // The expectation, which a fleet that pins nothing still has: the
            // release fleet supports. `fleet.claude_code` below stays the
            // file's own word, so a reader tells the two apart.
            agent_version_expected: Some(self.policy.claude_code_expected()),
            fleet: PolicyView {
                path: self.policy_path.display().to_string(),
                mtime: self.policy_mtime.and_then(clock::stamp_of),
                poll_seconds: self.policy.poll_seconds,
                claude_code: self.policy.claude_code_pin.clone(),
                plugin_dir: self
                    .policy
                    .plugin_dir
                    .as_ref()
                    .map(|dir| dir.display().to_string()),
            },
            fleet_parse_error: self.policy_error.clone(),
            in_flight: None,
            effects: effects.view,
            grant: grant_read.state.to_string(),
            grant_detail: grant_read.detail.clone(),
            seats,
            orders: Vec::new(),
        };

        // The routines, re-read every tick: a directory listing and a parse each,
        // so a file dropped into a routines directory is live on the next
        // evaluation with no restart.
        let routines_now = routines::now_secs();
        // The seats a routine may name: the rows this machine runs for a
        // nudge, and every seat the policy lists besides for an item's
        // assignee — read off the policy file as it stands this tick.
        let seat_directory = config::directory(
            &self.config.seats,
            &fleet_core::item::table_at(&self.policy_path),
            &self.machine_dir,
        );
        // The directory holding the policy file in force, which is this
        // machine's fleet root. A daemon's own working directory is the service
        // manager's and names nothing, so the walk-up the CLI does is not a
        // reading here.
        let routines_root = self.policy_path.parent().map(Path::to_path_buf);
        let registry = match &routines_root {
            Some(root) => routines::load::load(
                &routines::load::roots(root, &self.machine_dir, &projects_of(root)),
                &seat_directory,
            ),
            None => routines::load::Registry::default(),
        };
        let (mut routines_state, routines_state_error) = routines::state::read(&self.machine_dir);
        if let Some(why) = routines_state_error {
            if self.routines_state_said.insert(why.clone()) {
                eprintln!("fleet observe: the routines state stands empty; {why}");
            }
        }
        document.orders = routines::rows(&registry, &routines_state, routines_now);

        // The line the cursor may not pass this tick: the oldest `seat.resting`
        // whose collection did not happen. A rest that was asked for and not
        // collected is the alarm, and an alarm the cursor walked past
        // would be a request the fleet silently forgot.
        let mut hold_at: Option<u64> = None;
        if acting {
            for ((index, observation, context_tokens), verdict) in
                observations.iter().zip(verdicts.iter())
            {
                let seat = &self.config.seats[*index];
                let outcome = act(
                    agent,
                    &self.policy,
                    seat,
                    observation,
                    *context_tokens,
                    *verdict,
                    &mut document,
                    &self.machine_dir,
                    &mut self.events_log,
                    &mut self.table,
                    now_ms,
                    &mut self.rest_failed_said,
                );
                if outcome != Outcome::None {
                    table_moved = true;
                }
                document.seats[*index].outcome = outcome.as_str().to_string();
                // The counter, moved once per poll on the state this poll SAW
                // and the verdict it reached. It is computed only where effects
                // are on: a poll that dispatched nothing has no blind dispatch
                // to count, and counting one would halt a seat this controller
                // never acted for.
                let key = seat.id.to_string();
                let carried = self.table.seat_state(&key);
                let blind = decide::blind_after(carried.blind, observation.state, *verdict);
                if blind != carried.blind || carried.halted != (blind >= decide::BLIND_LIMIT) {
                    let halted = carried.halted || blind >= decide::BLIND_LIMIT;
                    if blind > carried.blind {
                        effect::blind_dispatch(
                            &seat.id,
                            blind,
                            verdict.as_str(),
                            &mut self.events_log,
                        );
                    }
                    // Once per transition INTO the halt, never once per poll.
                    if halted && !carried.halted {
                        effect::halted(&seat.id, &seat.machine_name(), blind, &mut self.events_log);
                    }
                    self.table.set_seat_state(&key, SeatState { blind, halted });
                    document.seats[*index].blind = blind;
                    document.seats[*index].halted = halted;
                    table_moved = true;
                }
                // A rest is consumed by the collection that answered it, or by
                // the successor a stopped row got instead. Anything else leaves
                // it standing for the next tick.
                let collected = matches!(outcome, Outcome::Rested | Outcome::Spawned);
                if let Some(seq) = pending
                    .get(&seat.id)
                    .filter(|asked| asked.rest && !collected)
                    .and_then(|asked| asked.rest_seq)
                {
                    hold_at = Some(hold_at.unwrap_or(seq).min(seq));
                }
            }
        } else {
            // No effect was issued, so nothing was collected and every rest read
            // this tick is still standing.
            hold_at = pending.values().filter_map(|asked| asked.rest_seq).min();
        }

        // The cursor moves once the tick's verdicts are computed and its effects
        // are taken, so a controller that dies mid-tick re-reads the same events
        // and acts once against a roster that has already moved.
        if let Some(seq) = read_to {
            let advanced = match hold_at {
                Some(held) => seq.min(held.saturating_sub(1)),
                None => seq,
            };
            if advanced > self.table.consumed_seq {
                self.table.consumed_seq = advanced;
                table_moved = true;
            }
        }
        self.commit_table(table_moved);
        write_projection(&self.machine_dir, &document);

        // The routines' own pass, after the document this tick publishes and
        // before the substrate read. The array above carries the state as it
        // stood at the top of this tick, so a firing here is published by the
        // next one — the document says what was OBSERVED and never what is
        // happening inside it.
        {
            let mut pass = routines::Pass {
                machine: routines::action::Machine {
                    machine_dir: &self.machine_dir,
                    child_path: self.seams.child_path,
                    policy: &self.policy,
                    seats: &seat_views(
                        &self.config.seats,
                        &observations,
                        &recorded_dirs,
                        &self.table,
                    ),
                    // THE SAME GATE the per-seat effects take. A routine's ring
                    // is an effect — it starts a turn in a seat's worktree —
                    // and a pending grant means no effect is issued, so a pass
                    // that read only the binary would fire one while the
                    // projection said effects were off with the grant as cause.
                    agent: acting.then_some(agent),
                    effects_off: document.effects.cause.clone(),
                },
                events: &mut self.events_log,
                state: &mut routines_state,
            };
            routines::tick(&registry, &mut pass, routines_now);
        }

        // The runs' own pass, after the routines'. The DECISION is this crate's
        // — a fold of the stream this loop already owns — and only the three
        // acts a run's advance needs are the seam's, so the pass is a call here
        // and the seam is what it acts through. A refusal is logged and the
        // loop goes on: the poll's other work is what every seat depends on.
        if let Some(runs) = self.runs {
            let stream = self.machine_dir.join("events.jsonl");
            let mut pass = crate::runs::Pass {
                runs,
                events: &mut self.events_log,
                stream: &stream,
                max_crashes: self.policy.run_max_crashes,
            };
            if let Err(why) = crate::runs::tick(&mut pass) {
                eprintln!("{}: {why}", crate::runs::REFUSED);
            }
        }

        // A live version that differs from the pin is a flag to re-measure,
        // never a failure — and one event per move, not one per poll. The
        // pin is the fleet's own where its file writes one, and the release
        // fleet supports where it does not.
        //
        // Only a version that WAS READ and agrees with the pin closes an
        // announcement. A poll whose version read failed knows nothing about the
        // spread, and clearing on it re-announces the same move on the next
        // healthy poll.
        let pair = (
            agent_version.clone(),
            Some(self.policy.claude_code_expected()),
        );
        match (&pair.0, &pair.1) {
            (Some(live), Some(pinned)) if live != pinned => {
                if self.announced_move.as_ref() != Some(&pair) {
                    log_event(
                        &mut self.events_log,
                        events::SUBSTRATE_MOVED,
                        CONTROLLER,
                        serde_json::json!({
                            "agent": "claude_code",
                            "observed": pair.0,
                            "expected": pair.1,
                        }),
                    );
                    self.announced_move = Some(pair);
                }
            }
            (Some(_), Some(_)) => self.announced_move = None,
            _ => {}
        }
    }

    /// The session table put back as a READ-MODIFY-WRITE under the lock every
    /// other writer of it takes (`sessions::read_under_lock`), rather than as a
    /// rename of the copy this loop carries.
    ///
    /// The loop reads the table once at startup and holds it across polls, so a
    /// row another writer pushed, edited or dropped under the lock — a flight's
    /// spawn, `fleet seat` spawn, feed or retire — is not in that copy, and a
    /// rename of it would take the row with it — and the seat that row names is
    /// then polled against the fleet's own daemon rather than its own
    /// configuration directory, which reads absent while it works. The fresh
    /// read is merged against the copy this tick started from: this loop's moves
    /// are this loop's, every other row is the file's.
    ///
    /// RUN ON EVERY POLL AND WRITTEN ONLY ON A MOVE. The merged table is what
    /// the next poll reads its configuration directories off, so a poll that
    /// changed nothing still has to pick up the rows that arrived under the
    /// lock; a poll that changed nothing has nothing to put back.
    ///
    /// A FILE THAT DOES NOT READ IS WRITTEN OVER RATHER THAN MERGED WITH, and
    /// this is the one writer of it that may: there is nothing to merge onto,
    /// and the copy this loop carries is the stream's own fold plus the moves
    /// this run has made — so the table a person deleted or a schema this build
    /// refuses is healed by the next poll that has something to put back, which
    /// is what the startup rebuild is for. A verb takes the opposite reading of
    /// the same file, because its copy is one edit and not a fold.
    ///
    /// A lock that cannot be taken is the one case that writes nothing: without
    /// it a write is the rename this whole method exists to stop.
    fn commit_table(&mut self, table_moved: bool) {
        let (held, fresh, fault) = match sessions::read_under_lock(&self.table_path) {
            Ok(read) => read,
            Err(cause) => {
                if self.table_said.as_deref() != Some(cause.as_str()) {
                    eprintln!("fleet observe: the session table stands; {cause}");
                    self.table_said = Some(cause);
                }
                return;
            }
        };
        let merged = match fresh {
            Some(fresh) => {
                self.table_said = None;
                sessions::merge(&self.table_base, &self.table, fresh)
            }
            None => {
                match fault {
                    // Once per cause, never once per poll.
                    Some(cause) if self.table_said.as_deref() != Some(cause.as_str()) => {
                        eprintln!(
                            "fleet observe: the session table does not read, so this poll writes \
                             the copy the loop folded from the stream over it; {cause}"
                        );
                        self.table_said = Some(cause);
                    }
                    Some(_) => {}
                    None => self.table_said = None,
                }
                self.table.clone()
            }
        };
        if table_moved {
            if let Err(e) = sessions::write_under_lock(&held, &self.table_path, &merged) {
                eprintln!("fleet observe: could not write the session table: {e}");
                return;
            }
        }
        drop(held);
        self.table_base = merged.clone();
        self.table = merged;
    }

    /// The interval between ticks, spent against the clock the caller handed in
    /// and re-read from the policy in force every time. `false` means stop.
    fn nap(&self) -> bool {
        nap(
            self.seams.clock,
            Duration::from_secs(self.policy.poll_seconds),
        )
    }

    /// The line a deliberate stop owes the stream, and the loop's exit status.
    fn finish(&mut self) -> u8 {
        if platform::stop_requested() {
            log_event(
                &mut self.events_log,
                events::CONTROLLER_STOPPED,
                CONTROLLER,
                serde_json::json!({ "controller_version": env!("CARGO_PKG_VERSION") }),
            );
        }
        0
    }
}

/// Carry one verdict out, with `in_flight` published around the blocking call.
///
/// The field is written BEFORE the call and cleared after it, which is the whole
/// of its value: a poll that is inside a slow effect and a controller that has
/// died are indistinguishable from freshness alone.
#[allow(clippy::too_many_arguments)]
fn act(
    agent: &dyn Agent,
    policy: &Policy,
    seat: &Seat,
    observation: &SeatObservation,
    context_tokens: Option<u64>,
    verdict: Verdict,
    document: &mut Projection,
    machine_dir: &Path,
    events_log: &mut EventLog,
    table: &mut Table,
    now_ms: u64,
    rest_failed_said: &mut BTreeSet<String>,
) -> Outcome {
    match verdict {
        Verdict::LeaveAlone => Outcome::None,
        // The guard's whole effect is that nothing is dispatched. The outcome is
        // published so a reader can tell a held seat from a quiet one; the line
        // and the event were written once, at the transition.
        Verdict::Halt => Outcome::Halted,
        Verdict::Revive | Verdict::SpawnWoken | Verdict::Rest | Verdict::SuggestRest => {
            let machine_name = seat.machine_name();
            let recorded = Recorded::of(table, seat);
            let Some(target) = target_for(policy, seat, observation, context_tokens, recorded)
            else {
                eprintln!(
                    "fleet observe: {machine_name} is due {} and names no one worktree, so there \
                     is nowhere to start it; nothing is done",
                    verdict.as_str()
                );
                return Outcome::None;
            };
            document.in_flight = Some(InFlight {
                seat: SeatView::from(&seat.as_ref()),
                effect: verdict.as_str().to_string(),
            });
            write_projection(machine_dir, document);
            let outcome = match verdict {
                Verdict::SpawnWoken => {
                    effect::spawn_woken(agent, policy, &target, events_log, table, now_ms)
                }
                Verdict::Revive => effect::revive(agent, &target, events_log, table, now_ms),
                Verdict::Rest => {
                    match effect::rest(agent, policy, &target, events_log, table, now_ms) {
                        effect::Rested::Collected => Outcome::Rested,
                        effect::Rested::StopFailed(cause) => {
                            if rest_failed_said.insert(format!(
                                "{}:{}",
                                machine_name,
                                observation.session_id.as_deref().unwrap_or("-")
                            )) {
                                eprintln!(
                                    "fleet observe: {}'s rest is not collected and stays pending; \
                                 nothing was started and nothing was removed — {cause}",
                                    machine_name
                                );
                            }
                            Outcome::Failed
                        }
                        effect::Rested::StartFailed(cause) => {
                            if rest_failed_said.insert(format!(
                                "{}:{}",
                                machine_name,
                                observation.session_id.as_deref().unwrap_or("-")
                            )) {
                                eprintln!(
                                    "fleet observe: {}'s predecessor was stopped and its successor \
                                 did not start, so the rest is not collected and stays pending; \
                                 nothing was removed — {cause}",
                                    machine_name
                                );
                            }
                            Outcome::Failed
                        }
                        effect::Rested::NoAddress => {
                            eprintln!(
                                "fleet observe: {} asked to rest and its row carries no short id, \
                             which is the address a stop takes; nothing is done",
                                machine_name
                            );
                            Outcome::Failed
                        }
                    }
                }
                _ => effect::nudge(agent, policy, &target, events_log, table),
            };
            document.in_flight = None;
            outcome
        }
    }
}

/// The projects routines are read from.
///
/// This slice has one, the embedded project, and it is the fleet root itself —
/// handed to the loader as a list of one so a registry of many is one more
/// element and not a second reader.
fn projects_of(fleet_root: &Path) -> Vec<(String, PathBuf)> {
    let name = fleet_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    vec![(name, fleet_root.to_path_buf())]
}

/// The seat rows a routine's ring reads: where each seat works, the name its
/// session answers to, and what the roster said this tick.
///
/// The worktree is the CONFIGURED one — the first project's, in the alphabetical
/// order the seat list is parsed into — because a routine names a seat and not a
/// session, and a seat standing nowhere still has one place to be started in.
fn seat_views(
    seats: &[Seat],
    observations: &[(usize, SeatObservation, Option<u64>)],
    recorded_dirs: &BTreeMap<SeatId, String>,
    table: &Table,
) -> Vec<routines::SeatView> {
    observations
        .iter()
        .filter_map(|(index, observation, _)| {
            let seat = seats.get(*index)?;
            let (_, worktree) = seat.worktrees.first()?;
            Some(routines::SeatView {
                id: seat.id,
                session_name: table.session_name(&seat.as_ref()),
                worktree: dir_key(worktree).to_string(),
                state: observation.state,
                config_dir: recorded_dirs.get(&seat.id).cloned(),
            })
        })
        .collect()
}

/// Everything an effect needs about one seat, or `None` when the seat names no
/// one directory to act in.
fn target_for<'a>(
    policy: &Policy,
    seat: &'a Seat,
    observation: &'a SeatObservation,
    context_tokens: Option<u64>,
    recorded: Recorded,
) -> Option<Target<'a>> {
    // The directory the seat was SEEN in when it was seen in one, and its single
    // configured worktree otherwise. A seat registered on several projects and
    // standing in none of them has no one answer, and picking arbitrarily is how
    // a session comes up in the wrong checkout.
    let (project, worktree) = match (&observation.project, &observation.worktree) {
        (Some(project), Some(worktree)) => (project.as_str(), worktree.as_str()),
        _ => match seat.worktrees.as_slice() {
            [(project, path)] => (project.as_str(), path.as_str()),
            _ => return None,
        },
    };
    let model = policy.model_for(seat.model.as_deref());
    Some(Target {
        seat: seat.id,
        project,
        worktree: dir_key(worktree),
        posture: policy.posture_for(seat.transient).to_string(),
        first_turn: policy.first_turn_for(&recorded.session_name),
        session_name: recorded.session_name,
        model,
        transient: seat.transient,
        config_dir: recorded.config_dir,
        item: recorded.item,
        settings: None,
        // The loop's own starts run no load belt: they wake seats the config
        // already carries rather than creating any, so a reading here would be
        // one this call never took.
        belt: None,
        // The loop's own starts are outside every run: this process is a
        // service and the seats it wakes are the seat list's, not a workflow's.
        run: None,
        session_id: observation.session_id.as_deref(),
        short_id: observation.short_id.as_deref(),
        context_tokens,
    })
}

/// What the seat's own session row remembers about the dispatch that opened it:
/// the configuration directory every act about the session is made under, the
/// item the order index named, and the name the session was started under.
///
/// Read off the table and CLONED rather than borrowed, because the same table is
/// written inside the calls that take this.
#[derive(Clone, Debug)]
struct Recorded {
    config_dir: Option<String>,
    item: Option<String>,
    /// `sessions::Table::session_name`: the row's, else the machine name.
    session_name: String,
}

impl Recorded {
    fn of(table: &Table, seat: &Seat) -> Recorded {
        let row = table.newest_for(&seat.id.to_string());
        Recorded {
            config_dir: row.and_then(|row| row.config_dir.clone()),
            item: row.and_then(|row| row.item.clone()),
            session_name: table.session_name(&seat.as_ref()),
        }
    }
}

/// Fold this tick's stream into what each seat asked for.
///
/// A seat event whose actor names no configured row is DROPPED with a line, and
/// so is a `seat.resting` for a transient row: only named seats rest, and
/// `fleet seat retire` is the verb for the other kind. `seat.woke` and
/// `seat.handed_off` are recorded as lifecycle and nothing more — neither asks
/// for anything.
///
/// The actor a line carries is the seat's id, and `known` is the ids of the
/// seat list's rows. An actor that is no id at all — a line an older build
/// wrote under the machine name — names no row either.
fn fold(
    stream: &[events::Record],
    known: &BTreeSet<SeatId>,
    transient: &BTreeSet<SeatId>,
) -> BTreeMap<SeatId, Pending> {
    let mut pending: BTreeMap<SeatId, Pending> = BTreeMap::new();
    for record in stream {
        if !events::SEAT_TYPES.contains(&record.kind.as_str()) {
            continue;
        }
        let Some(id) = SeatId::parse(&record.actor)
            .ok()
            .filter(|id| known.contains(id))
        else {
            eprintln!(
                "fleet observe: dropping a {} whose actor `{}` names no seat row",
                record.kind, record.actor
            );
            continue;
        };
        if record.kind == events::SEAT_RESTING && transient.contains(&id) {
            eprintln!(
                "fleet observe: dropping a {} for `{}`, which is a transient row; only named \
                 seats rest",
                record.kind, record.actor
            );
            continue;
        }
        let entry = pending.entry(id).or_default();
        if record.kind == events::SEAT_RESTING {
            entry.rest = true;
            entry.rest_seq = Some(entry.rest_seq.unwrap_or(record.seq).min(record.seq));
        }
        if events::is_deliberate_end(&record.kind) {
            entry.deliberate_end = true;
        }
        if record.kind == events::SEAT_CLEAR_HALT {
            entry.clear_halt = true;
        }
        // A seat that woke or handed off after asking to rest has answered the
        // ask itself: the successor is up, or the seat ended on its own terms,
        // and acting on the older event now would stop a session that just
        // started.
        if record.kind == events::SEAT_WOKE {
            entry.rest = false;
            entry.deliberate_end = false;
            entry.rest_seq = None;
        }
    }
    pending
}

/// The policy in force on this machine: the file's, with `config.json`'s own
/// answers over it key by key.
///
/// A key the object carries that names no `[controller]` key is ignored and
/// said once — a machine-local key nobody reads is worth a line and is not worth
/// refusing the seat list over.
fn overlaid(file: &Policy, raw: &MachineConfig, said: &mut BTreeSet<String>) -> Policy {
    let over = policy::overrides_in(raw.controller.as_ref());
    for why in over.ignored() {
        if said.insert(why.clone()) {
            eprintln!("fleet observe: the seat list's {why}, so it is ignored");
        }
    }
    file.overlaid(&over)
}

/// The seat list with the model-gated rows taken out (lessons claude-code D3).
///
/// A row whose start would ask for a posture its model was not measured to
/// honour comes up in the default mode and says so ONLY ON SCREEN — nothing an
/// instrument reads reports the downgrade, and the seat then stops at the first
/// approval dialog with nobody there to answer. So the row is dropped here,
/// before any start is attempted, and the drop is loud.
fn admitted(raw: &MachineConfig, policy: &Policy) -> MachineConfig {
    let mut seats = Vec::with_capacity(raw.seats.len());
    let mut skipped = raw.skipped.clone();
    for seat in &raw.seats {
        let model = policy.model_for(seat.model.as_deref());
        if policy.posture_is_ungranted(seat.transient, &model) {
            skipped.push(format!(
                "{} would start under posture `{}` on model `{model}`, which matches none of \
                 the models measured to honour it ({})",
                seat.machine_name(),
                policy.posture_for(seat.transient),
                policy.auto_capable_models.join(", ")
            ));
            continue;
        }
        seats.push(seat.clone());
    }
    MachineConfig {
        fleet_toml: raw.fleet_toml.clone(),
        seats,
        skipped,
        controller: raw.controller.clone(),
    }
}

/// Sleep the poll interval in slices so a stop is answered inside one of them
/// rather than at the end of the wait. `false` means stop.
///
/// The slices are the clock's, not the thread's, so the stop is read once per
/// slice of whatever time that clock keeps.
fn nap(clock: &dyn Clock, total: Duration) -> bool {
    let slice = Duration::from_millis(100);
    let mut slept = Duration::ZERO;
    while slept < total {
        if platform::stop_requested() {
            return false;
        }
        clock.sleep(slice.min(total - slept));
        slept += slice;
    }
    !platform::stop_requested()
}

fn write_projection(machine_dir: &Path, document: &Projection) {
    let path: PathBuf = machine_dir.join("projection.json");
    match projection::render(document) {
        Ok(body) => {
            if let Err(e) = platform::write_atomic(&path, body.as_bytes()) {
                eprintln!("fleet observe: could not publish {}: {e}", path.display());
            }
        }
        Err(e) => eprintln!("fleet observe: could not render the projection: {e}"),
    }
}

fn log_event(events_log: &mut EventLog, kind: &str, actor: &str, payload: serde_json::Value) {
    if let Err(e) = events_log.append(kind, actor, payload) {
        eprintln!("fleet observe: could not append {kind} to the event stream: {e}");
    }
}

/// A skipped row is reported every time the file is read: a seat that silently
/// vanishes from supervision is the failure this exists to make loud.
fn report_skipped(config: &MachineConfig) {
    for why in &config.skipped {
        eprintln!("fleet observe: skipping a seat row — {why}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeClock;
    use std::time::Instant;

    /// The whole point of the seam, measured: the poll interval driven against a
    /// fake clock spends the full interval of FAKE time and no wall clock. Ten
    /// minutes of naps, and the arm is bounded at 10 ms of real time.
    #[test]
    fn the_poll_nap_under_a_fake_clock_costs_fake_time_and_not_wall_clock() {
        let clock = FakeClock::new();
        let interval = Duration::from_secs(600);

        let started = Instant::now();
        assert!(nap(&clock, interval), "no stop was requested");
        let wall = started.elapsed();

        assert_eq!(
            clock.spent(),
            interval,
            "the nap spends the interval it was given"
        );
        assert!(
            wall < Duration::from_millis(10),
            "{interval:?} of fake time cost {wall:?} of wall clock"
        );
    }
}
