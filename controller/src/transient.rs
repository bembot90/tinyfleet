//! The transient-seat primitives.
//!
//! Three verbs — `fleet seat spawn`, `fleet seat feed`, `fleet seat retire` —
//! as three functions over the machine directory, the adapter, the policy, the
//! seat list and the session table. They are the primitives a pack's `dispatch`
//! calls: the order note, the brief's content and the assignment belong to the
//! caller, and NOTHING HERE TOUCHES THE WORK GRAPH.
//!
//! Everything a verb cannot work out for itself is gathered by the caller into
//! [`Machine`], the way `effect.rs` gathers a seat into its `Target`: the
//! project and its two directories come from the project's own policy file,
//! which this crate cannot read because it takes nothing from core but its
//! bounded runner (`fleet_core::process`), the release it supports
//! (`fleet_core::supported`) and a seat's identity
//! (`fleet_core::seat::identity`: the id, the fleet.toml roster and the
//! resolver). The stream and the session table are opened here from the
//! machine directory the caller named, because a verb is one process and there
//! is no loop holding them across a poll.
//!
//! ## The belt's two environment overrides
//!
//! `FLEET_LOAD_AVERAGE` and `FLEET_CPUS` are read by [`Readings::taken`] and by
//! nothing else in this crate. They exist so a suite can force both readings —
//! the way the drive suite forces the agent binary with `FLEET_CLAUDE_BIN` —
//! because an arm that read the real load average is one whose answer changes
//! with whatever else the machine is running. A BINARY is what calls `taken`:
//! an in-process caller hands the two numbers in on [`Machine`] instead,
//! because setting a variable in a process that is forking children races the
//! fork.

use crate::adapter::{self, dir_key, Activity, Agent, Permissions};
use crate::config::{self, Seat};
use crate::effect::{self, Outcome, Target, Typed};
use crate::events::{self, ActorRef, EventLog};
use crate::host::{self, HostRead, PaneState};
use crate::platform;
use crate::policy::Policy;
use crate::projection::SeatView;
use crate::sessions::{self, Table};
use fleet_core::seat::identity::{resolve, SeatId, SeatRef};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The rows of the exit table a REFUSAL can carry.
/// Done is not among them: a verb that answers `Ok` carries a value and not a
/// status, and the cli turns it into 0.
pub const REFUSED: u8 = 1;
pub const COULD_NOT_TELL: u8 = 3;
pub const NO_SESSION: u8 = 4;
/// The row is a named seat where a transient one was required. Named seats are
/// rung and rested; transient ones are fed and retired.
pub const NOT_TRANSIENT: u8 = 6;

/// A verb that stopped, with the status a script reads and the sentence a
/// person does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: u8,
    pub message: String,
}

impl Refusal {
    fn at(code: u8, message: impl Into<String>) -> Refusal {
        Refusal {
            code,
            message: message.into(),
        }
    }

    fn refused(message: impl Into<String>) -> Refusal {
        Refusal::at(REFUSED, message)
    }

    /// An instrument the answer needed would not answer. Never rounded into a
    /// clean verdict: a session nobody could ask is a question, not an absence.
    fn could_not_tell(message: impl Into<String>) -> Refusal {
        Refusal::at(COULD_NOT_TELL, message)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Everything the three verbs are told rather than read for themselves.
pub struct Machine<'a> {
    pub machine_dir: &'a Path,
    pub agent: &'a dyn Agent,
    /// The host a spawned seat's session runs on (`crate::host`). A start
    /// that fails kills its session there, so a spawn's rollback finds
    /// nothing of it left on the host.
    pub host: &'a dyn crate::host::Host,
    pub policy: &'a Policy,
    /// The project a transient seat's worktree is keyed under in the seat list.
    pub project: &'a str,
    /// The checkout every `git worktree` call is run from.
    pub primary: &'a Path,
    /// Where a transient seat's worktree is made.
    pub worktrees_dir: &'a Path,
    /// The belt's two machine readings, TAKEN BY THE CALLER like everything
    /// else here. [`Readings::taken`] is what a binary passes; an in-process
    /// caller hands the numbers in, which is what keeps the two overrides —
    /// the process's own environment, shared by every thread — out of a suite
    /// that runs arms in parallel and forks children while they run.
    pub readings: Readings,
}

impl Machine<'_> {
    fn config_path(&self) -> PathBuf {
        self.machine_dir.join("config.json")
    }

    fn stream_path(&self) -> PathBuf {
        self.machine_dir.join("events.jsonl")
    }

    fn table_path(&self) -> PathBuf {
        sessions::path_in(self.machine_dir)
    }

    /// The configuration directory a spawned seat comes up under: one per
    /// seat, under the machine directory and named by its machine name, so
    /// nothing from the person's home directory reaches a flight.
    ///
    /// The LOCATION is the machine directory's and no policy key names it: a
    /// second spelling of it is a second place a stale value can live.
    fn config_dir_for(&self, seat: &Seat) -> PathBuf {
        self.config_dir_named(&seat.machine_name())
    }

    /// The same directory for a seat known so far only by the machine name a
    /// claim took — which is what a rollback holds before any row reads back.
    fn config_dir_named(&self, machine_name: &str) -> PathBuf {
        self.machine_dir.join(CONFIG_DIRS).join(machine_name)
    }

    /// The directory a seat's own session row names, where it names one. The
    /// table is keyed on the seat's id.
    fn recorded_config_dir(&self, seat: &str) -> Option<String> {
        sessions::read(&self.table_path())
            .0?
            .newest_for(seat)?
            .config_dir
            .clone()
    }

    /// The seat list as the verbs read it. An unreadable one is could-not-tell:
    /// a verb that treated it as an empty fleet would call every named seat
    /// transient.
    fn seats(&self) -> Result<Vec<Seat>, Refusal> {
        match config::read(&self.config_path()) {
            Ok(machine) => Ok(machine.seats),
            Err(why) => Err(Refusal::could_not_tell(format!(
                "the seat list could not be read, so this verb cannot tell a transient row \
                 from a named one: {why}"
            ))),
        }
    }

    /// The table this controller wrote, off the disk and under no lock.
    ///
    /// A TABLE THAT WOULD NOT PARSE IS A REFUSAL AND NEVER AN EMPTY ONE.
    /// `sessions::read`'s `None` means nothing came off the disk, which covers
    /// a file that is absent as squarely as one that is corrupt or of a schema
    /// this build refuses — and rounding the second to an empty table renames it
    /// over the real one, taking every other seat's row, the halt latch and the
    /// cursor with it. The two are told apart by the cause beside it: an absent
    /// file carries none and is the empty table it already is.
    ///
    /// Never rebuilt from the stream here: a rebuild is the loop's act at
    /// startup, and a verb that rebuilt would write a table the running
    /// controller did not agree to.
    ///
    /// A COPY READ HERE IS ONLY EVER A READING, never the basis of a write: the
    /// file can move before the write lands, so what goes back to disk is built
    /// on [`Machine::table_under_lock`]'s copy and nothing else.
    fn table_now(&self) -> Result<Table, Refusal> {
        match sessions::read(&self.table_path()) {
            (Some(table), _) => Ok(table),
            (None, None) => Ok(Table::default()),
            (None, Some(cause)) => Err(Refusal::could_not_tell(format!(
                "the session table could not be read, and a table nobody could read is not an \
                 empty fleet — nothing was written: {cause}"
            ))),
        }
    }

    /// The same table read UNDER THE LOCK that guards a read-modify-write of
    /// it, with the lock held for as long as the returned handle lives.
    ///
    /// A rename is atomic and a read-modify-write is not, which is the hazard
    /// the seat list's own lock exists for: two verbs that each read this table
    /// and then rename their own version over it leave one of the two edits.
    ///
    /// THE READ IS INSIDE THE LOCK and is never hoisted out of it. A verb that
    /// edits a copy it read before taking the lock writes back every other
    /// seat's row as that copy carried it, which loses whatever landed in
    /// between — the same lost edit the lock exists to stop, moved one step.
    fn table_under_lock(&self) -> Result<(std::fs::File, Table), Refusal> {
        let path = self.table_path();
        let held = platform::lock_beside(&path).map_err(|e| {
            Refusal::could_not_tell(format!(
                "the session table's lock at {} could not be taken: {e}",
                path.display()
            ))
        })?;
        self.table_now().map(|table| (held, table))
    }

    /// The table written back under a lock the caller is holding. The handle is
    /// taken by reference so a caller cannot drop the lock and still write.
    fn write_table(&self, _held: &std::fs::File, table: &Table) -> Result<(), Refusal> {
        sessions::write(&self.table_path(), table).map_err(|e| {
            Refusal::could_not_tell(format!(
                "the session table at {} could not be written: {e}",
                self.table_path().display()
            ))
        })
    }

    fn log(&self) -> EventLog {
        EventLog::open(&self.stream_path())
    }

    /// One line on the stream, which is the controller's ONE ledger.
    ///
    /// A failed append is reported and never swallowed: the act it journals has
    /// already happened — the marker moved, the seat was reclaimed — so a
    /// silent failure leaves the machine changed and the record saying nothing.
    /// It is could-not-tell rather than a refusal, which is the same shape a
    /// probe that cannot answer takes: the act stands, and the message says
    /// which line did not land.
    ///
    /// Every line these verbs write is about a seat, and names it as the actor
    /// by its id.
    fn journal(
        &self,
        log: &mut EventLog,
        kind: &str,
        seat: &str,
        payload: serde_json::Value,
    ) -> Result<(), Refusal> {
        log.append(kind, &ActorRef::seat(seat), payload)
            .map_err(|e| {
                Refusal::could_not_tell(format!(
                    "{kind} for `{seat}` could not be appended to {}, so the act stands and the \
                 ledger does not carry it: {e}",
                    self.stream_path().display()
                ))
            })
    }

    /// The row of the seat list this argument names, through the one resolver
    /// every seat argument takes, with the refusals a verb over a transient
    /// seat owes: an argument that names no one row, and a named row where a
    /// transient one was required.
    fn transient_row(&self, seats: &[Seat], arg: &str) -> Result<Seat, Refusal> {
        let refs: Vec<SeatRef> = seats.iter().map(Seat::as_ref).collect();
        let row = match resolve(&refs, arg) {
            Ok(index) => &seats[index],
            Err(unresolved) => return Err(Refusal::at(unresolved.code(), unresolved.to_string())),
        };
        if !row.transient {
            return Err(Refusal::at(
                NOT_TRANSIENT,
                format!(
                    "{} is a named seat — named seats are rung and rested, and only a \
                     transient row is fed and retired",
                    row.machine_name()
                ),
            ));
        }
        Ok(row.clone())
    }

    /// This seat's worktree for this project, falling back to its first as the
    /// ring does: a row may hold worktrees for several projects, and the one
    /// this verb is about is the one to act in.
    fn worktree_of(&self, row: &Seat) -> Result<String, Refusal> {
        row.worktrees
            .iter()
            .find(|(project, _)| project == self.project)
            .or_else(|| row.worktrees.first())
            .map(|(_, path)| path.clone())
            .ok_or_else(|| {
                Refusal::refused(format!(
                    "`{}` carries no worktree to act in",
                    row.machine_name()
                ))
            })
    }
}

// ---- the load belt ----------------------------------------------------------

/// The variable a suite sets to force the five-minute load average the belt
/// reads (`platform::LOAD_SAMPLE`).
pub const LOAD_OVERRIDE: &str = "FLEET_LOAD_AVERAGE";
/// The variable a suite sets to force the processor count.
pub const CPUS_OVERRIDE: &str = "FLEET_CPUS";

/// The two numbers the load leg is computed from, as the caller took them.
///
/// `None` on either is a reading nobody has, which makes the load leg
/// could-not-tell — never a machine under no load.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Readings {
    pub load: Option<f64>,
    pub cpus: Option<u32>,
}

impl Readings {
    /// The two overrides first, then the platform. This is the ONE function in
    /// this crate that reads them, and a binary is its only caller: an
    /// in-process caller builds a `Readings` instead, because setting a
    /// variable in a process that is forking children races the fork.
    pub fn taken() -> Readings {
        Readings {
            load: override_f64(LOAD_OVERRIDE).or_else(platform::load_average_5m),
            cpus: override_u32(CPUS_OVERRIDE).or_else(platform::cpus),
        }
    }
}

/// Both readings the belt took, whichever of them refuses.
///
/// A person reading a refusal is deciding whether to wait, and they need the
/// other number to decide with — so the two legs are reported together on every
/// path out, including the one that lets the spawn through.
#[derive(Debug, Clone, PartialEq)]
pub struct Belt {
    /// The average, the ceiling it was judged against, and the cpu count that
    /// ceiling was computed from. `None` is a leg nobody could read.
    pub load: Option<(f64, f64, u32)>,
    /// Transient seats mid-turn and the cap. `None` is the cap leg
    /// could-not-tell, which refuses nothing.
    pub busy: Option<(u32, u32)>,
    /// Why the cap leg could not be judged, where it could not.
    pub busy_unreadable: Option<String>,
}

impl Belt {
    /// Both legs, read once.
    ///
    /// A cap leg nobody could read is could-not-tell and REFUSES NOTHING — an
    /// agent nobody can ask must not wedge every spawn in the fleet — while
    /// the load leg keeps its teeth.
    pub fn read(machine: &Machine, seats: &[Seat]) -> Belt {
        let Readings {
            load: reading,
            cpus,
        } = machine.readings;
        let load = match (reading, cpus) {
            (Some(load), Some(cpus)) if cpus > 0 => Some((
                load,
                cpus as f64 * machine.policy.load_ceiling_per_cpu,
                cpus,
            )),
            _ => None,
        };

        // EVERY TRANSIENT SEAT WHOSE PANE THE HOST HOLDS ALIVE, asked about in
        // ONE read, each by the session its row last sighted and by its pane,
        // under the directory its session came up under: a spawned seat is
        // known to its agent under that directory and no other, so a cap
        // counted off the fleet's would be zero however many were mid-turn — a
        // ceiling that refuses nothing, which is what this leg exists to
        // prevent. A seat is counted by what the agent says its OWN session is
        // doing, and never by a session standing in its worktree (CORRECTIONS
        // AT REVIEW, 2026-09-25).
        //
        // ONE READING NOBODY COULD MAKE makes the whole leg could-not-tell, as
        // one unreadable listing did: a count short by an unknown number is not
        // a count, and an agent nobody can read must not wedge every spawn.
        //
        // The table is read ONCE, not once per seat: the directories all come
        // out of the same file and a second read could answer differently.
        let recorded = sessions::read(&machine.table_path()).0;
        let (mid_turn, busy_unreadable) = match machine.host.list() {
            HostRead::Unreadable { cause } => {
                (0, Some(format!("the host could not be read: {cause}")))
            }
            HostRead::Readable(panes) => {
                let asked: Vec<adapter::SeatRef> = seats
                    .iter()
                    .filter(|seat| seat.transient)
                    .filter_map(|seat| {
                        let session = host::session_for(&seat.id);
                        let pane = panes.iter().find(|pane| {
                            pane.session == session && pane.state == PaneState::Alive
                        })?;
                        let row = recorded
                            .as_ref()
                            .and_then(|table| table.newest_for(&seat.id.to_string()));
                        Some(adapter::SeatRef {
                            seat: seat.id,
                            session_id: row.and_then(|row| row.session_id.clone()),
                            pid: pane.pid,
                            config_dir: row.and_then(|row| row.config_dir.clone()),
                            worktree: pane.path.clone(),
                            screen: None,
                        })
                    })
                    .collect();
                let read = adapter::readings(machine.agent, &asked);
                let unreadable = read
                    .iter()
                    .find(|reading| {
                        reading.activity == Activity::Unknown && reading.session_id.is_none()
                    })
                    .map(|reading| reading.cause.clone().unwrap_or_default());
                let busy = read
                    .iter()
                    .filter(|reading| reading.activity == Activity::Busy)
                    .count() as u32;
                (busy, unreadable)
            }
        };
        let busy = match &busy_unreadable {
            Some(_) => None,
            None => Some((mid_turn, machine.policy.max_transient_busy)),
        };

        Belt {
            load,
            busy,
            busy_unreadable,
        }
    }
    /// The legs that are over their ceiling, in the order they are read.
    pub fn over(&self) -> Vec<&'static str> {
        let mut over = Vec::new();
        if let Some((load, ceiling, _)) = self.load {
            if load > ceiling {
                over.push("load average");
            }
        }
        if let Some((mid_turn, cap)) = self.busy {
            if mid_turn > cap {
                over.push("transient seats mid-turn");
            }
        }
        over
    }

    /// Both readings, one per line, in the shape a refusal prints them.
    pub fn lines(&self) -> String {
        let load = match self.load {
            Some((load, ceiling, cpus)) => format!(
                "{load:.2} (ceiling {ceiling:.2} = {cpus} cpu x {:.2})",
                ceiling / cpus as f64
            ),
            None => "COULD NOT TELL — no load average or cpu count on this host".to_string(),
        };
        let busy = match (&self.busy, &self.busy_unreadable) {
            (Some((mid_turn, cap)), _) => format!("{mid_turn} (cap {cap})"),
            (None, Some(cause)) => {
                format!("COULD NOT TELL — the roster could not be read, which is not zero: {cause}")
            }
            (None, None) => "COULD NOT TELL".to_string(),
        };
        format!("  load average (5m)       : {load}\n  transient seats mid-turn: {busy}")
    }

    /// Both readings as the stream carries them: the numbers this start was let
    /// through on, rather than the sentence a person reads.
    ///
    /// A LEG NOBODY COULD READ IS `null` AND NEVER A ZERO — a count that is not
    /// a count would fold as a quiet machine, which is the rounding this leg
    /// exists to refuse.
    pub fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "load": self.load.map(|(load, _, _)| load),
            "load_ceiling": self.load.map(|(_, ceiling, _)| ceiling),
            "cpus": self.load.map(|(_, _, cpus)| cpus),
            "mid_turn": self.busy.map(|(mid_turn, _)| mid_turn),
            "mid_turn_cap": self.busy.map(|(_, cap)| cap),
        })
    }

    fn refusal(&self) -> Option<Refusal> {
        let over = self.over();
        if over.is_empty() {
            return None;
        }
        Some(Refusal::refused(format!(
            "the machine cannot take another transient seat: {} over its ceiling.\n{}",
            over.join(" and "),
            self.lines()
        )))
    }
}

fn override_f64(key: &str) -> Option<f64> {
    std::env::var(key).ok()?.trim().parse().ok()
}

fn override_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok()?.trim().parse().ok()
}

// ---- spawn ------------------------------------------------------------------

/// What a spawn was asked for. The first turn is the file's TEXT: reading the
/// file is the caller's, because the caller is the one that knows whether a
/// missing file is a usage error or a brief that failed to render.
pub struct Spawn<'a> {
    pub first_turn: &'a str,
    pub model: Option<&'a str>,
    /// What the seat may run without asking, in fleet's own words: the
    /// project's command words and the builder's checks.
    ///
    /// A spawned session comes up under a posture that refuses every writing
    /// call it holds no rule for, so the launch hands these to the agent's
    /// adapter, which renders them into its agent's own format inside the
    /// seat's worktree before the first turn (reviewer call 2026-09-25, E8).
    /// Nothing here knows that format or where the agent reads it.
    pub permissions: Permissions,
    /// The work item this spawn is being made for, where the caller is giving
    /// one. It is carried only to be RECORDED — on the row and on the stream —
    /// so a report about this seat can name the work it was holding; nothing
    /// here reads the work graph.
    pub item: Option<&'a str>,
    /// The commit this seat's worktree is cut from, where the caller names one.
    /// `None` cuts from [`TRUNK`], which is what every spawn did before a
    /// reviewer had to read a delivery and a returned builder had to resume
    /// from one.
    ///
    /// A base the primary cannot resolve REFUSES BEFORE ANYTHING IS MADE: the
    /// `worktree add` would otherwise leave a name claimed and a rollback to
    /// take, for a fact that was readable one call earlier.
    pub base: Option<&'a str>,
    /// The per-provider overlay files that belong in a configuration space,
    /// as `(relative path, content)`.
    ///
    /// They are copied into the per-row directory before the start. The
    /// resolved overlay carries none today — the guards reach a session through
    /// the plugin root and the permissions through the worktree's own settings
    /// file — so the directory's whole content is its emptiness, and this is the
    /// seam that stops being true without a second edit here.
    pub config_files: &'a [(String, String)],
}

/// Where the per-row configuration directories live, under the machine
/// directory: one per spawned seat, named by the seat.
pub const CONFIG_DIRS: &str = "config";

/// What a spawn left behind. The seat's machine name, `agent-<short>`, is the
/// verb's one answer on stdout, because a person reads it; its id is what
/// `dispatch` assigns the item to, because the record is keyed by the id.
#[derive(Debug)]
pub struct Spawned {
    pub id: SeatId,
    pub seat: String,
    pub worktree: PathBuf,
    pub belt: Belt,
    /// The commit the worktree was cut at, read from ITS OWN HEAD rather than
    /// from the trunk ref the add named: the ref can move between the cut and
    /// the read, and the tree is the thing this fact describes. `None` where the
    /// read would not answer — the spawn stands either way, and a caller that
    /// records this writes it absent rather than guessing the trunk.
    pub base: Option<String>,
}

/// Create a transient seat: the belt, the worktree, the row, the start, the
/// read-back.
///
/// Each step is read back before the next, and the ROLLBACK WINDOW OPENS AT THE
/// WORKTREE: everything refused above it leaves nothing behind, and a start
/// that fails below it takes the worktree and the row with it — never a branch.
pub fn spawn(machine: &Machine, ask: &Spawn, now_ms: u64) -> Result<Spawned, Refusal> {
    let seats = machine.seats()?;

    // (a) The belt, before anything is created.
    let belt = Belt::read(machine, &seats);
    if let Some(refusal) = belt.refusal() {
        return Err(refusal);
    }

    // (a2) The base, resolved in the primary BEFORE the name is claimed: a cut
    // from a commit that is not there refuses here with nothing made.
    let cut_from = match ask.base {
        Some(base) => resolved_base(machine.primary, base)?,
        None => TRUNK.to_string(),
    };

    // (a3) What the agent declares, before anything is made: the model a
    // spawn that names none starts on is the fleet's, else the agent's.
    let capabilities = machine.agent.capabilities().map_err(|why| {
        Refusal::could_not_tell(format!(
            "the agent's capabilities could not be read, so no model can be named and nothing \
             was made: {why}"
        ))
    })?;

    // (b) and (c) under ONE lock: a freshly minted seat id, the worktree cut
    // from the commit above, and the row. The rollback window opens with the
    // `worktree add` inside it, and a claim that answers `Made` is one that got
    // that far.
    let model = machine.policy.model_for(ask.model, &capabilities);
    let claimed = config::claim_transient_seat(
        &machine.config_path(),
        &config::TransientSeat {
            project: machine.project,
            model: &model,
            worktrees_dir: machine.worktrees_dir,
        },
        |_name, worktree| {
            git(
                machine.primary,
                "worktree add",
                &[
                    "worktree",
                    "add",
                    "--detach",
                    &worktree.display().to_string(),
                    &cut_from,
                ],
            )
            .map(|_| ())
        },
    );
    let claimed = match claimed {
        Ok(claimed) => claimed,
        Err(config::ClaimError::Nothing(why)) => return Err(Refusal::refused(why)),
        Err(config::ClaimError::Made { worktree, why }) => {
            return Err(rolled_back(machine, REFUSED, None, &worktree, &why))
        }
    };
    let name = claimed.name.clone();
    let worktree = claimed.worktree.clone();
    let worktree_arg = worktree.display().to_string();
    // READ HERE, right after the add and inside the rollback window: this is the
    // one moment the tree exists and nothing of the seat's has touched it, so
    // the commit read is the one the seat starts from. A read that will not
    // answer is not a refusal — the seat is real and the base is a fact about
    // it, not a precondition of it.
    let base = base_of(&worktree);

    // The read-back finds the row by the id the claim minted, which is the
    // one key the row is written under.
    let row = match machine.seats() {
        Ok(seats) => match seats
            .into_iter()
            .find(|seat| seat.id == claimed.id && seat.transient)
        {
            Some(row) => row,
            None => {
                return Err(rolled_back(
                    machine,
                    COULD_NOT_TELL,
                    Some(&claimed),
                    &worktree,
                    &format!("the seat list does not read back a transient row for {name}"),
                ))
            }
        },
        Err(refusal) => {
            return Err(rolled_back(
                machine,
                refusal.code,
                Some(&claimed),
                &worktree,
                &refusal.message,
            ))
        }
    };

    // (c3) The seat's OWN configuration directory, empty but for whatever the
    // overlay puts in a configuration space. Made here, inside the rollback
    // window and before the start, because the start is what the directory is
    // for and a session that came up under the person's own directory is the
    // isolation failure this whole slice is against.
    let config_dir = machine.config_dir_for(&row);
    if let Err(why) = make_config_dir(&config_dir, ask.config_files) {
        return Err(rolled_back(
            machine,
            COULD_NOT_TELL,
            Some(&claimed),
            &worktree,
            &why,
        ));
    }

    // (d) The start, through the same effect the loop's own spawn takes. The
    // table is READ here, before the start, so an unreadable one refuses before
    // a session is brought up that nothing could record — and it is read WITHOUT
    // the lock, because the start below spends a whole watch window and a lock
    // held across it blocks every other verb on this machine for that long.
    if let Err(refusal) = machine.table_now() {
        return Err(rolled_back(
            machine,
            refusal.code,
            Some(&claimed),
            &worktree,
            &refusal.message,
        ));
    }
    let mut log = machine.log();
    let before = log.seq();
    // A seat nobody has started yet has no row to have recorded a session name
    // on, so its session is named by its machine name.
    let target = Target {
        seat: claimed.id,
        session_name: name.clone(),
        project: machine.project,
        worktree: &worktree_arg,
        model,
        posture: machine.policy.posture_for(true).to_string(),
        first_turn: ask.first_turn.to_string(),
        transient: true,
        config_dir: Some(config_dir.display().to_string()),
        item: ask.item.map(str::to_string),
        // Rendered by the agent's adapter into the seat's worktree as the
        // launch is built — inside the rollback window and BEFORE the start: a
        // session that came up without them is one that cannot write.
        permissions: ask.permissions.clone(),
        belt: Some(belt.payload()),
        // THE SPAWNING PROCESS'S OWN RUN, read here and nowhere else. A
        // workflow's child carries `FLEET_RUN_ID`, and `fleet seat spawn` under
        // one inherits it — so a seat a run started is tagged with it and a seat
        // started from a shell is not.
        run: crate::runs::of_this_process(),
        session_id: None,
        context_tokens: None,
    };
    // INTO A TABLE OF ITS OWN: what this call produces is the ROW, and the file
    // it belongs in can be written by a feed or a retire while the start runs.
    // The row is folded below into a copy read under the lock, so the table this
    // spawn hands over carries whatever else landed inside its window.
    let mut opened = Table::default();
    let started = effect::spawn_woken(
        machine.agent,
        machine.host,
        machine.policy,
        &target,
        &mut log,
        &mut opened,
        now_ms,
    );
    if started != Outcome::Spawned {
        // The session died, never listed, or was never started, and the start
        // has already killed whatever it made on the host. The cause and the
        // capture are read from the line the start itself wrote, so the
        // refusal names what the crash event carries.
        let (cause, output) =
            crashed_output(&machine.stream_path(), before, &claimed.id.to_string());
        return Err(rolled_back(
            machine,
            REFUSED,
            Some(&claimed),
            &worktree,
            &format!(
                "the start for {name} failed inside its {}s watch window ({}); its output is at \
                 {}",
                machine.policy.start_watch_seconds,
                cause.as_deref().unwrap_or("the stream names no cause"),
                output
                    .as_deref()
                    .unwrap_or("(the stream names no output file)")
            ),
        ));
    }
    // (d2) The row onto the file, under the lock and over a table read inside
    // it. A failure here is not rolled back: the session is up, and a refusal
    // that also deleted its worktree would take a live seat's tree with it.
    let (held, mut table) = machine.table_under_lock()?;
    table.sessions.extend(opened.sessions);
    machine.write_table(&held, &table)?;
    drop(held);

    // (e) The read-back: the row off the seat list, and the session-table row
    // off its own file rather than off the object this process just built.
    let written = sessions::read(&machine.table_path()).0.ok_or_else(|| {
        Refusal::could_not_tell(format!(
            "the session table at {} does not read back after the start",
            machine.table_path().display()
        ))
    })?;
    if written.newest_for(&claimed.id.to_string()).is_none() {
        return Err(Refusal::could_not_tell(format!(
            "the session table carries no row for {name} after its start"
        )));
    }
    Ok(Spawned {
        id: claimed.id,
        seat: name,
        worktree,
        belt,
        base,
    })
}

/// The trunk ref a transient worktree is cut from where a spawn names no base,
/// as the local ref names it.
const TRUNK: &str = "origin/main";

/// One named base as the primary resolves it, or a refusal naming it.
///
/// `rev-parse --verify <base>^{commit}` and not a bare `rev-parse`: the bare
/// form validates SYNTAX and echoes any 40-hex string back at exit 0, so a
/// commit that is not in this repository would reach the `worktree add` and
/// fail there — inside the rollback window, with a name already claimed.
fn resolved_base(primary: &Path, base: &str) -> Result<String, Refusal> {
    git(
        primary,
        "rev-parse",
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .map(|read| read.trim().to_string())
    .map_err(|why| {
        Refusal::refused(format!(
            "`{base}` does not resolve to a commit in {} — nothing was made: {why}",
            primary.display()
        ))
    })
    .and_then(|sha| {
        if sha.is_empty() {
            return Err(Refusal::refused(format!(
                "`{base}` does not resolve to a commit in {} — nothing was made",
                primary.display()
            )));
        }
        Ok(sha)
    })
}

/// The commit a fresh worktree's HEAD names, or nothing.
fn base_of(worktree: &Path) -> Option<String> {
    let read = git(worktree, "rev-parse HEAD", &["rev-parse", "HEAD"]).ok()?;
    let sha = read.trim().to_string();
    (sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())).then_some(sha)
}

/// The seat's own configuration directory, made and read back.
///
/// A directory that already stands is EMPTIED first — a fresh id makes that a
/// defect, never a reuse.
///
/// The read-back is the same check every other step here takes, and it is the
/// LISTING and not the existence: a directory the process could not write into
/// is one the agent will populate from the person's own instead.
fn make_config_dir(dir: &Path, files: &[(String, String)]) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .map_err(|e| format!("{} could not be emptied: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    for (relative, content) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .map_err(|e| format!("{} was not written: {e}", path.display()))?;
    }
    let back = std::fs::read_dir(dir)
        .map_err(|e| format!("{} does not read back: {e}", dir.display()))?
        .count();
    if back != files.len() {
        return Err(format!(
            "{} reads back {back} entries and {} were written",
            dir.display(),
            files.len()
        ));
    }
    Ok(())
}

/// Undo everything the window covers, and say what the undo did. `None` for
/// the claim is one that made the worktree and never wrote the row, where there
/// is nothing to drop and the line says so.
///
/// NEVER A BRANCH. `git worktree remove` leaves refs alone, so a branch made
/// inside the worktree survives — which is the point: it is exactly what a
/// re-dispatch resumes from.
///
/// `code` is the status the cause CARRIED. A rollback does not change what kind
/// of answer this is: a table nobody could read is still could-not-tell after
/// the worktree has been taken back, and rounding it to a refusal would tell the
/// caller the machine said no when it said it could not say.
fn rolled_back(
    machine: &Machine,
    code: u8,
    claimed: Option<&config::Claimed>,
    worktree: &Path,
    why: &str,
) -> Refusal {
    let removed = match git(
        machine.primary,
        "worktree remove",
        &[
            "worktree",
            "remove",
            "--force",
            &worktree.display().to_string(),
        ],
    ) {
        Ok(_) => format!("the worktree at {} was removed", worktree.display()),
        Err(cause) => format!("THE WORKTREE AT {} SURVIVES: {cause}", worktree.display()),
    };
    let held_config = claimed.map(|claimed| machine.config_dir_named(&claimed.name));
    let unconfigured = match held_config.filter(|dir| dir.exists()) {
        None => "no configuration directory was left to remove".to_string(),
        Some(held_config) => match std::fs::remove_dir_all(&held_config) {
            Ok(()) => format!(
                "the configuration directory at {} was removed",
                held_config.display()
            ),
            Err(e) => format!(
                "THE CONFIGURATION DIRECTORY AT {} SURVIVES: {e}",
                held_config.display()
            ),
        },
    };
    // No claim is one that made the worktree and never reached the row, so
    // there is nothing to look for and the line says that rather than naming a
    // seat that was never written.
    let dropped = match claimed {
        None => "no seat-list row was written to drop".to_string(),
        Some(claimed) => {
            let name = &claimed.name;
            match config::drop_seat(&machine.config_path(), &claimed.id) {
                Ok(true) => format!("the seat-list row for {name} was dropped"),
                Ok(false) => format!("the seat list carried no row for {name} to drop"),
                Err(cause) => format!("THE SEAT-LIST ROW FOR {name} SURVIVES: {cause}"),
            }
        }
    };
    Refusal::at(
        code,
        format!(
            "{why}\n  rolled back: {removed}; {dropped}; {unconfigured}; no branch was deleted"
        ),
    )
}

/// The cause and the output file the `session.crashed` line this start wrote
/// names, each `None` where the line carries none. `seat` is the seat's id, and
/// the line's actor is that seat.
fn crashed_output(stream: &Path, after: u64, seat: &str) -> (Option<String>, Option<String>) {
    let Some(record) = events::read_after(stream, after)
        .into_iter()
        .rev()
        .find(|record| {
            record.kind == events::SESSION_CRASHED && record.actor.seat_id() == Some(seat)
        })
    else {
        return (None, None);
    };
    let field = |name: &str| {
        record
            .payload
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };
    (field("cause"), field("output"))
}

// ---- feed -------------------------------------------------------------------

/// What a feed left behind: the turn that was in the seat and the one that is
/// now, so a caller can say what moved.
#[derive(Debug)]
pub struct Fed {
    pub seat: String,
    pub prior: String,
    pub next: String,
}

/// Hand a live transient seat its next first turn, moving the occupant marker
/// with a put-back on a delivery that was not witnessed.
///
/// THE MARKER IS THE SESSION-TABLE ROW'S `first_turn`, which the spawn set and
/// this verb moves; the move is journaled on the event stream, which is the
/// controller's one ledger, and never in a file beside it.
///
/// The turn is TYPED into the seat's own session ([`effect::type_turn`]), so
/// the session that holds the seat receives it, and it is believed only when
/// the listing turns busy.
pub fn feed(machine: &Machine, seat: &str, first_turn: &str) -> Result<Fed, Refusal> {
    let seats = machine.seats()?;
    let row = machine.transient_row(&seats, seat)?;
    // From here the session table and the stream are asked by the seat's id,
    // and every sentence names it by its machine name.
    let name = row.machine_name();
    let seat = name.as_str();
    let seat_id = row.id.to_string();
    let worktree = machine.worktree_of(&row)?;

    // Under the seat's own configuration directory, for the same reason the
    // retire reads there: the fleet's listing does not name a spawned seat's
    // session, and a feed that read it would refuse every live seat as having
    // none. The row is the one carrying the seat's pane's pid.
    let config_dir = machine.recorded_config_dir(&seat_id);
    let under = config_dir.as_deref().map(Path::new);
    let target = effect::TurnTarget {
        seat: &row.id,
        config_dir: under,
    };
    let live = match effect::seat_row(machine.agent, machine.host, &target) {
        Ok(live) => live,
        Err(Typed::Absent) => {
            return Err(Refusal::at(
                NO_SESSION,
                format!("`{seat}` has no live session in {worktree}, so there is nothing to feed"),
            ))
        }
        Err(other) => {
            return Err(Refusal::could_not_tell(format!(
                "no session can be named for `{seat}` — {}",
                other.recorded()
            )))
        }
    };
    // The primitive knows only what the AGENT says. Whether the seat is holding
    // a work item is the caller's half and is not asked here.
    if live.reading.activity == Activity::Busy {
        return Err(Refusal::refused(format!(
            "`{seat}` is still holding a turn — the agent reports its session {}",
            adapter::word(Activity::Busy)
        )));
    }
    if let Some(cause) = adapter::waiting_on(&live.reading) {
        return Err(Refusal::refused(format!(
            "`{seat}` is stopped in front of a person — blocked on {cause} — and nothing is \
             typed at a dialog"
        )));
    }

    // THE NEWEST ROW FOR THE SEAT, which is the one this seat is sitting in: a
    // seat is carried through as many rows as the loop opened, and the first in
    // file order is a dispatch two successors ago. The lock is held across the
    // read, the move and the write-back, and DELIBERATELY ON across the
    // delivery to the put-back below: a second feed landing between the move
    // and its put-back would have its own move undone by this one.
    let (held, mut table) = machine.table_under_lock()?;
    let Some(marker) = table.newest_for_mut(&seat_id) else {
        return Err(Refusal::refused(format!(
            "the session table carries no row for `{seat}`, so there is no occupant marker to \
             move"
        )));
    };
    let prior = marker.first_turn.clone();
    marker.first_turn = first_turn.to_string();
    machine.write_table(&held, &table)?;

    let mut log = machine.log();
    let typed = effect::type_turn(
        machine.agent,
        machine.host,
        &target,
        first_turn,
        Duration::from_secs(machine.policy.nudge_timeout_seconds),
    );
    // ONLY A WITNESSED TURN IS A FEED. A turn queued behind another, refused
    // at a dialog or never taken leaves the seat on the turn it had.
    let delivered = match &typed {
        Typed::Delivered => Ok(()),
        other => Err(other.recorded()),
    };
    let outcome = match &delivered {
        Ok(()) => "delivered".to_string(),
        Err(recorded) => recorded.clone(),
    };
    // The journal BEFORE the put-back, so a line that cannot land is met with
    // the marker still where this verb moved it and the message saying so.
    let journalled = machine.journal(
        &mut log,
        events::SESSION_NUDGED,
        &seat_id,
        serde_json::json!({
            "worktree": worktree,
            "prior_first_turn": first_line(&prior),
            "first_turn": first_line(first_turn),
            "outcome": outcome,
            "put_back": false,
        }),
    );

    if let Err(cause) = delivered {
        // The row goes back to the turn that was in the seat, and the put-back
        // is a SECOND line rather than a rewrite of the first: the stream is
        // append-only and the attempt is part of what happened.
        if let Some(marker) = table.newest_for_mut(&seat_id) {
            marker.first_turn = prior.clone();
        }
        machine.write_table(&held, &table)?;
        journalled?;
        machine.journal(
            &mut log,
            events::SESSION_NUDGED,
            &seat_id,
            serde_json::json!({
                "worktree": worktree,
                "prior_first_turn": first_line(first_turn),
                "first_turn": first_line(&prior),
                "outcome": format!("put back: {cause}"),
                "put_back": true,
            }),
        )?;
        return Err(Refusal::refused(format!(
            "`{seat}` was not fed — {cause}; the occupant marker was put back"
        )));
    }
    journalled?;

    Ok(Fed {
        seat: seat.to_string(),
        prior,
        next: first_turn.to_string(),
    })
}

/// A turn's first line, which is what the journal carries: the whole text is
/// the table's, and a stream line that repeated it would be a second copy that
/// could disagree with the first.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

// ---- retire -----------------------------------------------------------------

/// What a retire reclaimed, as numbers a person reads.
#[derive(Debug)]
pub struct Reclaimed {
    pub seat: String,
    pub worktree: String,
    /// The worktree's bytes, measured BEFORE the removal. `None` is a directory
    /// this verb could not walk, which is stated rather than reported as zero.
    pub bytes: Option<u64>,
    /// The pid of the seat's live pane before the stop — the agent itself —
    /// and `None` where the host held no live pane for the seat.
    pub pid: Option<u32>,
    pub dead: bool,
    /// What the stop did on the host: `killed <session>` where the host held a
    /// session for the seat, live or dead, and `None` where it held none.
    pub removal: Option<String>,
    /// The branch the seat's worktree stood on, read BEFORE the removal took
    /// the worktree with it. `None` is a detached HEAD or a tree git would not
    /// answer about — never a branch called nothing.
    ///
    /// Nothing here acts on it. It is the second key a caller's release of that
    /// branch turns on: the landing's note says which branch it classified SAFE,
    /// and this says which one the seat that is going actually held.
    pub branch: Option<String>,
    /// What the caller's [`Withdrawal`] answered: the record's hold on this
    /// seat, released before the name was freed. Empty is a seat that held
    /// nothing ordered AND a caller that withdraws nothing — the two are one
    /// answer here, because this verb asks no work graph of its own.
    pub withdrawn: Vec<String>,
}

/// The record's half of a retire, filled by the caller.
///
/// A SEAM AND NOT A CALL, because this crate reaches no work graph: the store
/// is core's, and the cli is the one crate holding both. It is handed the seat
/// going and answers what it released, or the refusal the retire stops on.
///
/// The retire runs it at the LAST moment it can still stop — after the session
/// is stopped and verified gone, before the seat-list row comes off — so a
/// withdrawal that cannot be written leaves the name held rather than freed for
/// the next spawn to take with somebody else's order still on it.
pub type Withdrawal<'a> = &'a dyn Fn(&str) -> Result<Vec<String>, Refusal>;

/// The caller that withdraws nothing, which is what every retire did before the
/// seam existed: a controller with no work graph in reach passes it.
pub fn withdraws_nothing(_seat: &str) -> Result<Vec<String>, Refusal> {
    Ok(Vec::new())
}

/// End a transient seat and verify from OUTSIDE that nothing of it holds RAM or
/// disk.
///
/// The host is read once, whole, at the top: an unreadable one is
/// could-not-tell, because a session nobody could ask about is a question and
/// not an absence. The session is stopped on the host by the seat's own name,
/// the worktree is fleet's own to remove, and every probe at the end is its own
/// reading and none of them is this verb's own earlier report.
pub fn retire(machine: &Machine, seat: &str, dead: bool) -> Result<Reclaimed, Refusal> {
    retire_with(machine, seat, dead, &withdraws_nothing)
}

/// The same retire, with the record's half of it filled (see [`Withdrawal`]).
pub fn retire_with(
    machine: &Machine,
    seat: &str,
    dead: bool,
    withdrawal: Withdrawal,
) -> Result<Reclaimed, Refusal> {
    let seats = machine.seats()?;
    let row = machine.transient_row(&seats, seat)?;
    // From here the session table and the stream are asked by the seat's id,
    // and the record's assignee and every sentence by its machine name.
    let name = row.machine_name();
    let seat = name.as_str();
    let seat_id = row.id.to_string();
    let worktree = machine.worktree_of(&row)?;
    let key = dir_key(&worktree).to_string();

    // THE DIRECTORY THIS SEAT'S SESSION IS HELD UNDER, read off its own row.
    // The agent is asked about it under that directory: a spawned seat is
    // known to its agent under its own and no other, so a read under the
    // fleet's would find nothing whatever was still running.
    let config_dir = machine.recorded_config_dir(&seat_id);

    // THE SEAT'S SESSION, BY THE SEAT: its name on the host is its id
    // (reviewer call 2026-09-25, E1), so nothing the agent issued addresses it.
    // Read once, whole, here: a host nobody could read is could-not-tell,
    // because a session nobody could ask about is a question and not an
    // absence.
    let session = host::session_for(&row.id);
    let pane = match machine.host.list() {
        HostRead::Readable(panes) => panes.into_iter().find(|pane| pane.session == session),
        HostRead::Unreadable { cause } => {
            return Err(Refusal::could_not_tell(format!(
                "the host could not be read, so whether `{seat}`'s session {session} is still \
                 running cannot be told and nothing was removed: {cause}"
            )))
        }
    };
    let live = pane.as_ref().filter(|pane| pane.state == PaneState::Alive);

    // Measured before anything is removed, because every reading is gone the
    // instant the stop or the removal lands. The pid is the LIVE pane's — the
    // agent itself, since nothing sits between (E2) — and never a dead one's,
    // which names a process that has already ended and whose number the
    // system may since have handed to another.
    let bytes = bytes_under(Path::new(&worktree));
    let branch = branch_of(Path::new(&worktree));
    let pid = live.and_then(|pane| pane.pid);

    // The agent asked, ONCE and before anything is touched, about the live
    // pane's process under the seat's own directory — a reading nobody could
    // make is could-not-tell here rather than after the stop — for the session
    // it names: a session is the seat's by that pane and never by the directory
    // it stands in (lessons claude-code B5).
    let listed = match live.zip(pid) {
        None => None,
        Some((pane, pid)) => {
            let reading = adapter::readings(
                machine.agent,
                &[adapter::SeatRef {
                    seat: row.id,
                    session_id: None,
                    pid: Some(pid),
                    config_dir: config_dir.clone(),
                    worktree: pane.path.clone(),
                    screen: None,
                }],
            )
            .remove(0);
            if reading.activity == Activity::Unknown && reading.session_id.is_none() {
                return Err(Refusal::could_not_tell(format!(
                    "the agent could not be read, so no session can be named: {}",
                    reading.cause.unwrap_or_default()
                )));
            }
            reading.session_id
        }
    };

    // READ WITHOUT THE LOCK, and an unreadable table refuses here, before the
    // stop: the row comes off it at the end, and a table found unreadable only
    // then would leave the session stopped and every row standing. What the
    // lock guards is the read-modify-write at the end; held from here it would
    // span the stop's grace and two git calls, and every other verb on this
    // machine would wait out a whole retire.
    machine.table_now()?;

    // `--dead` licenses the retire by a COMPLETED host read that holds no live
    // pane for the seat — never by silence, which is what the could-not-tell
    // above keeps out of this branch.
    if let (Some(live), true) = (live, dead) {
        return Err(Refusal::refused(format!(
            "`{seat}` is not dead — the host holds its session {session} live{}{}, so --dead \
             was the wrong flag; retire it without --dead to stop it first",
            match live.pid {
                Some(pid) => format!(" as pid {pid}"),
                None => String::new(),
            },
            match &listed {
                Some(session) => format!(", listed as {session}"),
                None => String::new(),
            }
        )));
    }

    // THE STOP IS THE HOST'S, AND SO IS ITS WITNESS: `stop_session` answers Ok
    // only once a listing read after the kill names no session for the seat.
    // A dead pane is killed the same way — it is no session, and it holds the
    // seat's name. `None` is a seat the host holds nothing for.
    let removal = match &pane {
        None => None,
        Some(_) => {
            effect::stop_session(machine.host, &session).map_err(|cause| {
                Refusal::refused(format!(
                    "`{seat}` could not be stopped, so nothing was removed — the worktree, the \
                     seat-list row and the session table all stand: {cause}"
                ))
            })?;
            Some(format!("killed {session}"))
        }
    };

    if Path::new(&worktree).exists() {
        git(
            machine.primary,
            "worktree remove",
            &["worktree", "remove", "--force", &worktree],
        )
        .map_err(|cause| {
            // WHAT THIS REFUSAL SAYS IS GONE IS WHAT THE STOP ABOVE DID: it
            // ran wherever the host held a session for the seat, and that
            // session is gone by the time a worktree can be found stuck.
            let stopped = match &removal {
                Some(answer) => format!("its session is already gone ({answer})"),
                None => "there was no session on the host to stop".to_string(),
            };
            Refusal::refused(format!(
                "{worktree} could not be removed: {cause}\n  {stopped}; the seat-list row and \
                 the session table both stand — so re-running this after clearing the cause is \
                 safe"
            ))
        })?;
    }
    let _ = git(machine.primary, "worktree prune", &["worktree", "prune"]);

    // THE RECORD BEFORE THE ROW. The line below is the moment this seat stops
    // existing, and an order still standing against it would be one nobody
    // delivers and nothing dispatches again. Everything above has already
    // happened and a refusal here says so: the session is stopped and gone,
    // and the row is the one thing left.
    let withdrawn = withdrawal(seat).map_err(|refusal| Refusal {
        code: refusal.code,
        message: format!(
            "{}\n  the session and the worktree for `{seat}` ARE ALREADY GONE and the seat-list \
             row STANDS: clear the item and re-run this",
            refusal.message
        ),
    })?;

    config::drop_seat(&machine.config_path(), &row.id).map_err(|cause| {
        Refusal::could_not_tell(format!(
            "the seat-list row for `{seat}` could not be dropped: {cause}"
        ))
    })?;
    // The row comes off under the lock, over a table read inside it: every
    // other seat's row on the copy this edits is the one on disk now, and not
    // the one this verb read before the stop.
    let (held, mut table) = machine.table_under_lock()?;
    table.sessions.retain(|row| row.seat != seat_id);
    machine.write_table(&held, &table)?;
    drop(held);

    // FROM HERE THE TWO ROWS ARE ALREADY GONE, so every refusal below says so:
    // a re-run would be refused at the seat list with "names no seat", which
    // tells the person nothing about what is still standing.
    let verified = verify_from_outside(machine, seat, &session, &worktree, &key, pid);
    verified.map_err(|refusal| Refusal {
        code: refusal.code,
        message: format!(
            "{}\n  the seat-list row and the session-table row for `{seat}` ARE ALREADY DROPPED, \
             so a re-run will refuse at the seat list: finish by hand from {worktree}{}",
            refusal.message,
            match pid {
                Some(pid) => format!(" and pid {pid}"),
                None => String::new(),
            }
        ),
    })?;

    // LAST, and only once the probes above have answered: the seat's
    // configuration directory, which holds that one session's whole
    // configuration space — its transcript and its listing's scope included.
    // A refused verification leaves the directory standing, which is right:
    // something may still be running under it.
    let held_config = machine.config_dir_for(&row);
    if held_config.exists() {
        std::fs::remove_dir_all(&held_config).map_err(|e| {
            Refusal::could_not_tell(format!(
                "the configuration directory at {} could not be removed, and the seat is \
                 otherwise fully retired: {e}",
                held_config.display()
            ))
        })?;
    }

    // Retire NEVER deletes a branch: refs are the landing verb's, and a branch a
    // dead seat left is exactly what a re-dispatch resumes from. A caller
    // holding the landing that classified this one releases it through
    // [`delete_branch`], off that note and never off this verb's own reading.
    let mut log = machine.log();
    machine.journal(
        &mut log,
        events::SESSION_STOPPED,
        &seat_id,
        serde_json::json!({
            "worktree": worktree,
            "bytes": bytes,
            "pid": pid,
            "dead": dead,
            "removal": removal,
        }),
    )?;

    Ok(Reclaimed {
        seat: seat.to_string(),
        worktree,
        bytes,
        pid,
        dead,
        removal,
        branch,
        withdrawn,
    })
}

/// One work branch deleted in the primary, for a caller that has read a landing
/// saying so.
///
/// THE MUZZLE IS GUARDED AND NOT ONLY THE TRIGGER. This is the one call in this
/// crate that destroys a ref, so a name git would read as an option or as a
/// second argument is refused here whatever decided to pass it — and `--` is
/// passed for the same reason. What a name MEANS is the caller's to judge: this
/// knows nothing of trunks or of which branch was classified.
///
/// A delete that fails is a `String` and not a panic: the retire it follows has
/// already landed, and a ref that outlives it is a line to print.
pub fn delete_branch(machine: &Machine, branch: &str) -> Result<(), String> {
    if branch.is_empty() || branch.starts_with('-') || branch.split_whitespace().count() != 1 {
        return Err(format!(
            "`{branch}` is not a name this deletes — it is empty, reads as an option, or is not \
             one word"
        ));
    }
    git(
        machine.primary,
        "branch -D",
        &["branch", "-D", "--", branch],
    )
    .map(|_| ())
}

// ---- the priced retire ------------------------------------------------------

/// What one seat cost, and where its work got to.
///
/// EVERY FIELD IS A READING and every one is an `Option`. A transcript that
/// would not open prices no seat and a worktree git cannot answer about names no
/// branch; a zero here would read as free and an empty string as a branch called
/// nothing, and both are the lie this shape exists to refuse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cost {
    /// The main-chain context the session's last turn carried.
    pub context_tokens: Option<u64>,
    /// Main-chain assistant entries carrying a usage block.
    pub turns: Option<u64>,
    /// The clock less the row's `dispatched_at`.
    pub wall_ms: Option<u64>,
    /// The branch the worktree was on, read before the removal.
    pub branch: Option<String>,
    /// The commit its HEAD named, read in the same act.
    pub commit: Option<String>,
}

/// What a priced retire left behind: the reclaim and the cost beside it.
#[derive(Debug)]
pub struct Priced {
    pub reclaimed: Reclaimed,
    pub cost: Cost,
}

/// Retire a seat a flight spawned, reading what it cost before it goes.
///
/// THE READINGS COME FIRST, ALL OF THEM, because each is gone the instant the
/// removal lands: the transcript goes with the session row, the worktree with
/// the `worktree remove`, and the session table's `dispatched_at` with the row.
///
/// A READING NOBODY CAN TAKE DOES NOT STOP THE RETIRE. A seat nobody can price
/// is still a seat to reclaim, so every failure above is a `None` and the event
/// says so; only the retire itself refuses.
///
/// `--dead` IS NOT PASSED, and that is a reading rather than an omission: the
/// flag REFUSES when the host holds the seat's session live and changes nothing
/// when it does not, so a flagless call reclaims a seat whose session is already
/// gone exactly as `--dead` would and stops one that is still up. A flight
/// retiring a crashed seat and a flight retiring a delivered one therefore take
/// the same call.
pub fn priced(machine: &Machine, seat: &str, item: &str, now_ms: u64) -> Result<Priced, Refusal> {
    priced_with(machine, seat, item, now_ms, &withdraws_nothing)
}

/// The same priced retire, with the record's half of it filled (see
/// [`Withdrawal`]).
///
/// THE SEAM IS THE ONE THE HAND VERB RUNS. A run's cleanup retires a seat whose
/// item can still be open — parked, or never delivered — and the name it frees
/// is the one the next spawn takes, so it owes the record exactly what
/// `fleet seat retire` owes it: the order withdrawn before the name goes back
/// on the pile. The withdrawal runs inside the reclaim below, at the last
/// moment that can still stop it.
pub fn priced_with(
    machine: &Machine,
    seat: &str,
    item: &str,
    now_ms: u64,
    withdrawal: Withdrawal,
) -> Result<Priced, Refusal> {
    let seats = machine.seats()?;
    let row = machine.transient_row(&seats, seat)?;
    // From here the session table and the stream are asked by the seat's id,
    // and the line's payload names the seat as its `{id, name?, kind}` object.
    let seat = SeatView::from(&row.as_ref());
    let seat_id = row.id.to_string();
    let worktree = machine.worktree_of(&row)?;

    // The session row, for the id the transcript is keyed by and the stamp the
    // wall time is measured from.
    let (session_id, dispatched_at) = {
        let path = machine.table_path();
        let (table, _) = sessions::read(&path);
        match table.as_ref().and_then(|table| table.newest_for(&seat_id)) {
            Some(row) => (row.session_id.clone(), Some(row.dispatched_at)),
            None => (None, None),
        }
    };

    // UNDER THE SEAT'S OWN CONFIGURATION DIRECTORY, which is where the agent
    // keeps a spawned seat's session: a read under the default resolves
    // nothing at all, which would price every seat this fleet spawned as
    // unread. Asked of an agent that declares `context`, and of no other.
    let config_dir = machine.recorded_config_dir(&seat_id);
    let declared = machine
        .agent
        .capabilities()
        .is_ok_and(|capabilities| capabilities.context);
    let context = session_id.as_ref().and_then(|session| {
        adapter::contexts(
            machine.agent,
            declared,
            &[adapter::SeatRef {
                seat: row.id,
                session_id: Some(session.clone()),
                pid: None,
                config_dir: config_dir.clone(),
                worktree: dir_key(&worktree).to_string(),
                screen: None,
            }],
        )
        .remove(&row.id)
    });
    // Whether the agent could price the session at all: a context that states
    // neither a window nor a turn is a session nobody read.
    let read = context
        .as_ref()
        .is_some_and(|context| context.tokens.is_some() || context.turns.is_some());
    let commit = head_of(Path::new(&worktree));

    let reclaimed = retire_with(machine, &seat_id, false, withdrawal)?;

    // The branch is the RETIRE's own reading, taken before the same removal
    // every reading above is taken before, and read off what it answered rather
    // than a second time here: two readings of one fact are two things that can
    // disagree.
    let cost = Cost {
        context_tokens: context.as_ref().and_then(|context| context.tokens),
        // A session that took no turn is a MEASURED ZERO, which is the agent's
        // `turns: 0`, and one nobody could read is no count at all: they are
        // different facts, and the agent answers them differently.
        turns: context.as_ref().and_then(|context| context.turns),
        wall_ms: dispatched_at.map(|from| now_ms.saturating_sub(from)),
        branch: reclaimed.branch.clone(),
        commit,
    };
    let mut log = machine.log();
    machine.journal(
        &mut log,
        events::SESSION_RETIRED,
        &seat_id,
        serde_json::json!({
            "seat": seat,
            "item": item,
            "worktree": reclaimed.worktree,
            "context_tokens": cost.context_tokens,
            "turns": cost.turns,
            "wall_ms": cost.wall_ms,
            "branch": cost.branch,
            "commit": cost.commit,
            "transcript": read,
            "bytes": reclaimed.bytes,
            "pid": reclaimed.pid,
            "dead": reclaimed.dead,
            "removal": reclaimed.removal,
            // THIS PATH HAS NO TERMINAL. The hand verb prints a line per item
            // it took off the seat; a run's cleanup runs inside a service, so
            // the stream is the only place the withdrawal can be read back.
            "withdrawn": reclaimed.withdrawn,
        }),
    )?;

    Ok(Priced { reclaimed, cost })
}

/// The branch a worktree is on, or `None` for a detached HEAD or a directory
/// git will not answer about.
fn branch_of(worktree: &Path) -> Option<String> {
    let name = git(
        worktree,
        "rev-parse --abbrev-ref HEAD",
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )
    .ok()?;
    let name = name.trim().to_string();
    (!name.is_empty() && name != "HEAD").then_some(name)
}

/// The commit that worktree's HEAD names. A tree with no commit on it answers
/// `None` rather than an empty string.
fn head_of(worktree: &Path) -> Option<String> {
    let sha = git(worktree, "rev-parse HEAD", &["rev-parse", "HEAD"]).ok()?;
    let sha = sha.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// The three probes — the host, the worktree, the pane's process — each its
/// own reading and none of them this verb's own earlier report.
///
/// No listing is among them. The session was the host's and the row is only
/// the agent's account of its process, so a row left behind by a process the
/// pid probe reads as gone is an account and not a session; and one whose
/// process is alive is the pid probe's to refuse.
///
/// A probe that CANNOT ANSWER is could-not-tell and refuses at 3, never rounded
/// to clean: the whole point of verifying from outside is that a retire which
/// says "done" has looked.
fn verify_from_outside(
    machine: &Machine,
    seat: &str,
    session: &str,
    worktree: &str,
    key: &str,
    pid: Option<u32>,
) -> Result<(), Refusal> {
    // THE HOST, which is where the seat's session ran and the one reading of
    // whether it still does (ruling 3).
    match machine.host.list() {
        HostRead::Readable(panes) => {
            if panes.iter().any(|pane| pane.session == session) {
                return Err(Refusal::refused(format!(
                    "the host still holds `{seat}`'s session {session} after the retire"
                )));
            }
        }
        HostRead::Unreadable { cause } => {
            return Err(Refusal::could_not_tell(format!(
                "the host could not be read, so whether `{seat}`'s session {session} is gone is \
                 unknown: {cause}"
            )))
        }
    }

    if Path::new(worktree).exists() {
        return Err(Refusal::refused(format!(
            "{worktree} still exists after the removal"
        )));
    }
    let listed = git(
        machine.primary,
        "worktree list --porcelain",
        &["worktree", "list", "--porcelain"],
    )
    .map_err(|cause| {
        Refusal::could_not_tell(format!(
            "`git worktree list` could not be read, so whether {worktree} is still registered \
             is unknown: {cause}"
        ))
    })?;
    if listed
        .lines()
        .any(|line| line.strip_prefix("worktree ").map(dir_key) == Some(key))
    {
        return Err(Refusal::refused(format!(
            "`git worktree list` in {} still names {worktree}",
            machine.primary.display()
        )));
    }

    if let Some(pid) = pid {
        match platform::process_alive(pid) {
            Some(false) => {}
            Some(true) => {
                return Err(Refusal::refused(format!(
                    "pid {pid}, which the host gave for `{seat}`'s pane before the stop, is \
                     still a live process"
                )))
            }
            None => {
                return Err(Refusal::could_not_tell(format!(
                    "whether pid {pid} is still alive could not be read, so this retire is not \
                     verified — everything else is done"
                )))
            }
        }
    }
    Ok(())
}

/// The bytes under a directory, following no symlink. `None` is a directory
/// this verb could not walk, which is reported as unknown rather than as zero.
///
/// Regular-file lengths and not disk blocks: the number answers "how much did
/// this seat's checkout hold", which is what a person reclaiming a machine is
/// asking, and it is the same figure on both filesystems this fleet runs on.
fn bytes_under(dir: &Path) -> Option<u64> {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(here) = stack.pop() {
        for entry in std::fs::read_dir(&here).ok()? {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Some(total)
}

// ---- git --------------------------------------------------------------------

/// One git call in the primary, the binary through the standard library with
/// the terminal prompt disabled.
///
/// A non-zero exit is a refusal naming the step and what git said, never a
/// value rounded to a default: a verb that read a failed `worktree add` as a
/// worktree would start a session in a directory that is not there.
fn git(primary: &Path, step: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(primary)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("`git {step}` could not be run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`git {step}` {}: {}",
            match out.status.code() {
                Some(code) => format!("exited {code}"),
                None => String::from("was killed by a signal"),
            },
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
