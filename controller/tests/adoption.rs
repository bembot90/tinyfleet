//! A CLAIM IS TAKEN ONCE, ON A LIVE PANE, AND HOLDS NOTHING PAST ITS END.
//!
//! A test binary of its own, because these arms drive the loop in this process
//! and it reads the PROCESS's environment for the machine directory: an arm
//! setting it beside arms that do not would be setting it for every thread in
//! the binary. Inside this one they are serialised on the lock below, which
//! each rig holds for its whole life.
//!
//! What it measures is the thing neither `effect::adopt` nor `decide` can say
//! alone: that adoption claims a session the table names on the first poll that
//! sees it LIVE — its pane alive on the host and its row, by the pane's pid, on
//! the listing — once per session and never again; and that the claim holds
//! nothing once the pane is dead. A dead pane is an END (ruling 3): the session
//! a host keeps cannot hibernate or be re-hosted under it, so the hold a claim
//! used to buy a pid-less row (lessons claude-code A3, A10, both retired) has
//! nothing left to hold, and a claimed seat and an unclaimed one are decided
//! alike on the poll their panes read dead.
//!
//! And the end itself: the first poll that reads a pane dead writes ONE
//! `session.ended`, dated by that poll when the controller saw the pane alive
//! an interval before, and by the transcript when the pane was dead before
//! the controller was looking.
//!
//! And what is never claimed at all: a session Claude Code's background daemon
//! still hosts in a seat's worktree. The upgrade adopts nothing (ruling 10) —
//! the controller refuses to start over it, names it, and touches nothing.

use fleet_controller::adapter::claude_code::{parse_hosted, DaemonListing, Hosted};
use fleet_controller::clock::Clock;
use fleet_controller::events;
use fleet_controller::host::{session_for, Host};
use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options, Seams, StopHandler};
use fleet_controller::test_support::{Answers, FakeClock, FakeHost, StubAgent, FIRST_PANE_PID};
use fleet_core::seat::identity::SeatId;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod common;

/// The environment is the process's, so one rig runs at a time. A poisoned lock
/// is taken anyway: the panic that poisoned it already failed its own arm, and
/// refusing it here would fail every other arm for it.
static ENV: Mutex<()> = Mutex::new(());

/// The seat whose session an arm claims, or finds claimed.
const OWNED_ID: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const OWNED: &str = "agent-e8a04b17";
/// The seat beside it whose session the table names and no claim covers. It is
/// the control: whatever the claimed seat gets on a dead pane, this one gets
/// too, because the claim is not what decides a dead pane.
const UNOWNED_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const UNOWNED: &str = "agent-93b9739a";
const PROJECT: &str = "a-project";
const OWNED_SESSION: &str = "a-session";
const UNOWNED_SESSION: &str = "b-session";

/// The poll interval the rigs' policy file carries, and the fake time one nap
/// spends against the clock that ends a run of ticks.
const POLL_SECONDS: u64 = 1;

/// One main-chain assistant turn carrying a window well under the rest
/// threshold, which is what makes a dead pane's verdict a revive rather than a
/// spawn: the listing carries no token field, so the transcript is the only
/// surface that reading comes from.
const A_LIGHT_TRANSCRIPT: &str =
    "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":1000}}}\n";

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the wall clock is past the epoch")
        .as_millis() as u64
}

/// A machine directory naming two seats and a session table naming a sighted
/// session for each — one of them claimed where the arm asks for it — plus the
/// host their sessions run on and the agent whose listing names them.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    owned: PathBuf,
    unowned: PathBuf,
    host: FakeHost,
    stub: StubAgent,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("fleet-adoption-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            machine: root.join("machine"),
            owned: root.join("wt").join(OWNED),
            unowned: root.join("wt").join(UNOWNED),
            host: FakeHost::new(),
            // The agent builds no resume: a revive here is DECIDED and never
            // carried out, so a dead pane stays where the next poll reads it
            // — which is what these arms are about. What a revive does is the
            // effects suite's.
            stub: StubAgent::answering(Answers {
                session_log: Some(A_LIGHT_TRANSCRIPT.to_string()),
                last_write: Some(now_ms()),
                resume: Err("this rig's agent resumes nothing".to_string()),
                ..Answers::default()
            }),
            root,
            _held: held,
        };
        std::fs::create_dir_all(&rig.owned).expect("the owned seat's worktree is made");
        std::fs::create_dir_all(&rig.unowned).expect("the control seat's worktree is made");

        write(
            &rig.root.join("fleet.toml"),
            &format!("[controller]\npoll_seconds = {POLL_SECONDS}\n"),
        );
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": [\
                 {{\"id\": \"{OWNED_ID}\", \"worktrees\": {{\"{PROJECT}\": \"{}\"}}}}, \
                 {{\"id\": \"{UNOWNED_ID}\", \"worktrees\": {{\"{PROJECT}\": \"{}\"}}}}]}}\n",
                rig.root.join("fleet.toml").display(),
                rig.owned.display(),
                rig.unowned.display()
            ),
        );
        rig.write_the_table(false);
        common::hermetic::export(common::hermetic::in_process_vars(
            &rig.root,
            &rig.machine,
            None,
        ));
        platform::clear_stop();
        rig
    }

    /// The table as a poll before this one left it: each seat's row carries
    /// the session id a sighting wrote onto it, and the owned seat's is
    /// `adopted` where `claimed` — the state a poll after the one that adopted
    /// it reads back off disk.
    ///
    /// Both dispatches are far enough back that the arrival window is closed,
    /// so the hold that keeps a fresh dispatch from being re-issued says
    /// nothing here and the verdict is reached on the seat itself.
    fn write_the_table(&self, claimed: bool) {
        let claim = match claimed {
            true => format!(", \"adopted\": \"{OWNED_SESSION}\""),
            false => String::new(),
        };
        let row = |seat: &str, name: &str, worktree: &Path, session: &str, extra: &str| {
            format!(
                "{{\"seat\": \"{seat}\", \"project\": \"{PROJECT}\", \"worktree\": \"{}\", \
                 \"name\": \"{name}\", \"model\": \"claude-opus-5\", \"posture\": \"auto\", \
                 \"first_turn\": \"/wake {name}\", \"transient\": false, \
                 \"dispatch_id\": \"dispatch-{name}\", \"dispatched_at\": 1000, \
                 \"session_id\": \"{session}\"{extra}}}",
                worktree.display()
            )
        };
        write(
            &self.machine.join("sessions.json"),
            &format!(
                "{{\"schema\": 2, \"sessions\": [{}, {}]}}\n",
                row(OWNED_ID, OWNED, &self.owned, OWNED_SESSION, &claim),
                row(UNOWNED_ID, UNOWNED, &self.unowned, UNOWNED_SESSION, "")
            ),
        );
    }

    fn with_the_claim_taken(self) -> Rig {
        self.write_the_table(true);
        self
    }

    /// The table with the control seat's row recorded under a configuration
    /// directory of its own, as a spawned seat's is.
    fn with_the_control_under(self, dir: &str) -> Rig {
        let row = |seat: &str, name: &str, worktree: &Path, session: &str, extra: &str| {
            format!(
                "{{\"seat\": \"{seat}\", \"project\": \"{PROJECT}\", \"worktree\": \"{}\", \
                 \"name\": \"{name}\", \"model\": \"claude-opus-5\", \"posture\": \"auto\", \
                 \"first_turn\": \"/wake {name}\", \"transient\": false, \
                 \"dispatch_id\": \"dispatch-{name}\", \"dispatched_at\": 1000, \
                 \"session_id\": \"{session}\"{extra}}}",
                worktree.display()
            )
        };
        write(
            &self.machine.join("sessions.json"),
            &format!(
                "{{\"schema\": 2, \"sessions\": [{}, {}]}}\n",
                row(OWNED_ID, OWNED, &self.owned, OWNED_SESSION, ""),
                row(
                    UNOWNED_ID,
                    UNOWNED,
                    &self.unowned,
                    UNOWNED_SESSION,
                    &format!(", \"config_dir\": \"{dir}\"")
                )
            ),
        );
        self
    }

    /// The seats and the table the start reads, off the rig's own files.
    fn check(&self, daemon: &dyn DaemonListing) -> run::DaemonCheck {
        let config = fleet_controller::config::read(&self.machine.join("config.json"))
            .expect("the seat list reads");
        let (table, _) = fleet_controller::sessions::read(&self.machine.join("sessions.json"));
        run::daemon_check(
            &config.seats,
            &table.expect("the session table reads"),
            daemon,
        )
    }

    /// One start of the loop, reading `daemon` for what the Claude Code daemon
    /// hosts, and the status it ended with. Effects are off: what is measured
    /// is whether the loop starts, and a poll that went on to start the absent
    /// seats would spend each start's watch finding out nothing about it.
    fn start_reading(&self, daemon: &dyn DaemonListing) -> u8 {
        let clock = FakeClock::new();
        let mut seams = self.seams_reading(&clock, Some(daemon));
        seams.effects_off = Some("this arm measures the start alone".to_string());
        run::observe_seamed(&Options { once: true }, self.grant(), None, seams)
    }

    fn started(&self) -> bool {
        self.stream()
            .iter()
            .any(|event| event["type"] == events::CONTROLLER_STARTED)
    }

    /// A live session for `seat` on the host, as a start leaves one, and the
    /// pid its pane was given.
    fn start(&self, seat: &str, worktree: &Path) -> u32 {
        let before = self.host.calls_of(FakeHost::NEW_SESSION).len() as u32;
        self.host
            .new_session(
                &session_of(seat),
                worktree,
                &["/nowhere/agent".to_string()],
                &[],
            )
            .expect("the fake host starts the session");
        FIRST_PANE_PID + before
    }

    /// The listing naming each `(session, pid, worktree)` as the recorded
    /// interactive row reads (lessons claude-code B10): a pid, a status, no
    /// address.
    fn list(&self, rows: &[(&str, u32, &Path)]) {
        let body = rows
            .iter()
            .map(|(session, pid, worktree)| {
                format!(
                    "{{\"sessionId\":\"{session}\",\"cwd\":\"{}\",\"kind\":\"interactive\",\
                     \"pid\":{pid},\"status\":\"idle\",\"startedAt\":1000}}",
                    worktree.display()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let listing = format!("[{body}]");
        self.stub.set(|answers| answers.listing = Ok(listing));
    }

    /// One poll, from a loop that starts again knowing only what the session
    /// table on disk carries — which is what a restarted controller is.
    fn poll_once(&self) {
        let clock = FakeClock::new();
        assert_eq!(
            run::observe_seamed(
                &Options { once: true },
                self.grant(),
                None,
                self.seams(&clock)
            ),
            0
        );
    }

    fn seams<'a>(&'a self, clock: &'a dyn Clock) -> Seams<'a> {
        self.seams_reading(clock, None)
    }

    /// The seams, with what the start reads for sessions the Claude Code
    /// daemon still hosts.
    fn seams_reading<'a>(
        &'a self,
        clock: &'a dyn Clock,
        daemon: Option<&'a dyn DaemonListing>,
    ) -> Seams<'a> {
        Seams {
            clock,
            adapter: "stub",
            agent: &self.stub,
            host: &self.host,
            daemon,
            child_path: "",
            effects_off: None,
            stop_handler: StopHandler::Unarmed,
        }
    }

    fn grant(&self) -> Grant {
        Grant::new(platform::directory_listing(), Duration::from_secs(5))
    }

    fn stream(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Every session line the run wrote, as `<type> <actor id>` pairs — the actor
    /// beside the kind, because the claim is about WHICH seat got which line.
    fn session_lines(&self) -> Vec<String> {
        self.stream()
            .into_iter()
            .filter_map(|event| {
                let kind = event["type"].as_str()?.to_string();
                let actor = event["actor"]["id"].as_str().unwrap_or("-").to_string();
                kind.starts_with("session.")
                    .then(|| format!("{kind} {actor}"))
            })
            .collect()
    }

    fn lines_reading(&self, line: &str) -> usize {
        self.session_lines()
            .into_iter()
            .filter(|seen| seen == line)
            .count()
    }

    /// The `session.ended` lines for one seat, whole.
    fn ends_of(&self, seat: &str) -> Vec<serde_json::Value> {
        self.stream()
            .into_iter()
            .filter(|event| event["type"] == events::SESSION_ENDED && event["actor"]["id"] == seat)
            .collect()
    }

    fn projection(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine.join("projection.json"))
            .expect("a projection is published");
        serde_json::from_str(&body).expect("the projection parses")
    }

    fn row(&self, seat: &str) -> serde_json::Value {
        let document = self.projection();
        document["seats"]
            .as_array()
            .expect("the projection carries seats")
            .iter()
            .find(|row| row["seat"]["id"] == seat)
            .unwrap_or_else(|| panic!("{seat} is published: {document}"))
            .clone()
    }

    fn decision(&self, seat: &str) -> String {
        self.row(seat)["decision"]
            .as_str()
            .unwrap_or_else(|| panic!("{seat}'s decision is published"))
            .to_string()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        platform::clear_stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn session_of(seat: &str) -> String {
    session_for(&SeatId::parse(seat).expect("a hand-written seat id parses"))
}

/// The clock a run of ticks is driven on: fake time, and a stop asked for once
/// `after` has been napped away.
///
/// A nap that ENDS with the flag raised returns false and the loop leaves, so
/// the run is one tick per nap that finished quietly plus the tick before the
/// nap that raised it. Nothing here waits on the wall clock.
///
/// THE NAP IS ALSO THE TICK BOUNDARY, which is the only place a seamed run can
/// be told something new: `at_first_nap` runs once, between the first tick and
/// the second.
struct NapThenStop<'a> {
    inner: FakeClock,
    after: Duration,
    at_first_nap: Option<Box<dyn Fn() + 'a>>,
    napped: AtomicBool,
}

impl<'a> NapThenStop<'a> {
    fn new(after: Duration) -> NapThenStop<'a> {
        NapThenStop {
            inner: FakeClock::new(),
            after,
            at_first_nap: None,
            napped: AtomicBool::new(false),
        }
    }

    fn then(mut self, change: impl Fn() + 'a) -> NapThenStop<'a> {
        self.at_first_nap = Some(Box::new(change));
        self
    }
}

impl Clock for NapThenStop<'_> {
    fn now(&self) -> Instant {
        self.inner.now()
    }

    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    fn sleep(&self, d: Duration) {
        if !self.napped.swap(true, Ordering::SeqCst) {
            if let Some(change) = &self.at_first_nap {
                change();
            }
        }
        self.inner.sleep(d);
        if self.inner.spent() >= self.after {
            platform::request_stop();
        }
    }
}

/// A REAL-TIME BACKSTOP on the run of ticks: every way the nap can break is a
/// run that never asks for a stop, and an arm that hangs proves nothing.
struct Watchdog {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    fn armed() -> Watchdog {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let thread = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            platform::request_stop();
        });
        Watchdog {
            cancelled,
            thread: Some(thread),
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A RESTART CLAIMS THE LIVE SESSION ONCE, AND THE DEAD PANE IT LEAVES IS
/// REVIVED ON THE NEXT POLL, CLAIM AND ALL.
///
/// TWO POLLS, each from a loop that starts again knowing only the session table
/// on disk, and the host MOVES between them: the first sees the owned seat's
/// pane alive with its row on the listing by the pane's pid and claims it; the
/// second finds the pane dead and the row gone (lessons claude-code B10). The
/// claim is on the table by then and holds nothing: the seat is revived, and
/// its end is written once, dated by the transcript, because the controller
/// that read it dead had not seen it alive.
#[test]
fn a_restart_claims_a_live_pane_once_and_revives_it_once_the_pane_is_dead() {
    let rig = Rig::new("claimed-across-a-restart");
    let pid = rig.start(OWNED_ID, &rig.owned);
    // The control stays live and listed the whole way, so nothing is started
    // for it and what moves is the owned seat alone.
    let other = rig.start(UNOWNED_ID, &rig.unowned);
    rig.list(&[
        (OWNED_SESSION, pid, &rig.owned),
        (UNOWNED_SESSION, other, &rig.unowned),
    ]);

    rig.poll_once();
    let after_one = rig.session_lines();
    assert!(
        after_one.contains(&format!("session.adopted {OWNED_ID}")),
        "the first poll claimed the live session the table names: {after_one:?}"
    );
    assert_eq!(rig.decision(OWNED_ID), "leave-alone");
    assert_eq!(rig.row(OWNED_ID)["roster_state"], "present");

    // The session ends: the pane stays dead with its status, and the row
    // leaves the listing with its process.
    rig.host.end(&session_of(OWNED_ID), Some(0));
    rig.list(&[(UNOWNED_SESSION, other, &rig.unowned)]);

    rig.poll_once();
    let lines = rig.session_lines();
    assert_eq!(rig.row(UNOWNED_ID)["roster_state"], "present");
    assert!(rig.ends_of(UNOWNED_ID).is_empty(), "a live pane has no end");
    assert_eq!(rig.row(OWNED_ID)["roster_state"], "stopped");
    assert_eq!(rig.row(OWNED_ID)["exit_status"], 0);
    assert_eq!(
        rig.decision(OWNED_ID),
        "revive",
        "a claim holds nothing past the pane's end: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        1,
        "and the claim was taken once, not once per poll: {lines:?}"
    );
    let ends = rig.ends_of(OWNED_ID);
    assert_eq!(ends.len(), 1, "one end for one dead pane: {lines:?}");
    assert_eq!(ends[0]["payload"]["session"], OWNED_SESSION);
    assert_eq!(ends[0]["payload"]["status"], 0);
    assert_eq!(
        ends[0]["payload"]["source"],
        events::ENDED_FROM_TRANSCRIPT,
        "a pane this controller never saw alive is dated by its transcript"
    );
    assert!(ends[0]["payload"]["at"].is_string(), "{}", ends[0]);
    assert_eq!(
        rig.row(OWNED_ID)["ended_at"],
        ends[0]["payload"]["at"],
        "and the stopped row publishes the end the line carries"
    );
}

/// ONE CONTROLLER, POLLING ON: A PANE DYING BETWEEN TWO TICKS WRITES ONE
/// `session.ended`.
///
/// Three ticks of ONE loop. The first sees the pane alive and claims it; the
/// pane dies in the nap after it; the second reads it dead and writes the end,
/// dated by that poll because this controller saw it alive one interval
/// before; the third reads the same dead pane and writes nothing, because the
/// end is latched on the table.
#[test]
fn a_pane_dying_between_two_ticks_writes_one_session_ended() {
    let rig = Rig::new("ended-between-ticks");
    let pid = rig.start(OWNED_ID, &rig.owned);
    let other = rig.start(UNOWNED_ID, &rig.unowned);
    rig.list(&[
        (OWNED_SESSION, pid, &rig.owned),
        (UNOWNED_SESSION, other, &rig.unowned),
    ]);
    let clock = NapThenStop::new(Duration::from_secs(POLL_SECONDS * 3)).then(|| {
        rig.host.end(&session_of(OWNED_ID), Some(1));
        rig.list(&[(UNOWNED_SESSION, other, &rig.unowned)]);
    });
    let watchdog = Watchdog::armed();
    let status = run::observe_seamed(
        &Options { once: false },
        rig.grant(),
        None,
        rig.seams(&clock),
    );
    drop(watchdog);
    assert_eq!(status, 0, "the loop ended on the stop it was asked for");

    // THREE TICKS RAN, read from the agent rather than assumed: every live
    // pane is asked about in one read, so a tick is one read of the agent.
    assert_eq!(
        rig.stub.calls_of(StubAgent::READ).len(),
        3,
        "the loop polled three times: {:?}",
        rig.stub.verbs()
    );
    assert_eq!(
        rig.host.calls_of(FakeHost::LIST).len(),
        3,
        "and read the host once per poll"
    );

    let lines = rig.session_lines();
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        1,
        "the claim was taken on the first tick and not retaken: {lines:?}"
    );
    let ends = rig.ends_of(OWNED_ID);
    assert_eq!(
        ends.len(),
        1,
        "one end across two polls of a dead pane: {lines:?}"
    );
    assert_eq!(ends[0]["payload"]["status"], 1);
    assert_eq!(
        ends[0]["payload"]["source"],
        events::ENDED_OBSERVED,
        "a pane seen alive an interval before is dated by the poll that saw it dead"
    );
    assert_eq!(rig.decision(OWNED_ID), "revive");

    // The control on the latch: the table carries it, so the rebuild the next
    // lost table would make carries it too.
    let rebuilt = fleet_controller::sessions::rebuild(&rig.machine.join("events.jsonl"));
    let row = rebuilt
        .newest_for(OWNED_ID)
        .expect("the rebuild opens the owned seat's row");
    assert_eq!(
        row.ended.as_ref().map(|ended| ended.source.as_str()),
        Some(events::ENDED_OBSERVED)
    );
}

/// A LIVE IDLE SESSION IS CLAIMED AT ITS FIRST SIGHTING, BY THE PANE'S PID.
///
/// The row carries no address and no state word (lessons claude-code B10), so
/// nothing but the pid can say it is the seat's: the pane the host holds for
/// the seat is the agent's own process (E2). The control is a seat whose table
/// row names a session the listing carries in its worktree with NO pane
/// behind it — a session fleet does not host, so not claimed; and since no
/// seat is found by its working directory (CORRECTIONS AT REVIEW, 2026-09-25),
/// the seat the host holds nothing for reads absent.
#[test]
fn a_live_idle_session_is_claimed_at_its_first_sighting() {
    let rig = Rig::new("live-idle-adopted");
    let pid = rig.start(OWNED_ID, &rig.owned);
    rig.list(&[
        (OWNED_SESSION, pid, &rig.owned),
        (UNOWNED_SESSION, 4242, &rig.unowned),
    ]);

    rig.poll_once();

    let lines = rig.session_lines();
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        1,
        "the live row was claimed: {lines:?}"
    );
    assert_eq!(rig.decision(OWNED_ID), "leave-alone");
    assert_eq!(
        rig.lines_reading(&format!("session.revived {OWNED_ID}")),
        0,
        "and nothing was attached to a session that is already up: {lines:?}"
    );

    // The control: listed live, named by the table, and never the seat's —
    // no claim without a pane, and presence is the host's alone.
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {UNOWNED_ID}")),
        0,
        "a listed session with no pane behind it is not claimed: {lines:?}"
    );
    assert_eq!(rig.row(UNOWNED_ID)["roster_state"], "absent");
}

/// A CLAIMED SESSION WHOSE PANE IS DEAD IS REVIVED, EXACTLY AS AN UNCLAIMED ONE.
///
/// Both seats' panes died before this controller started, the owned seat's
/// session claimed by an earlier poll and the control's never. The claim used
/// to hold a pid-less row the daemon still listed; a dead pane is no such row,
/// and the two seats take the same verdict, each writing its one end.
#[test]
fn a_claimed_session_whose_pane_is_dead_is_revived_exactly_as_an_unclaimed_one() {
    let rig = Rig::new("claimed-then-dead").with_the_claim_taken();
    rig.start(OWNED_ID, &rig.owned);
    rig.start(UNOWNED_ID, &rig.unowned);
    rig.host.end(&session_of(OWNED_ID), Some(0));
    rig.host.end(&session_of(UNOWNED_ID), Some(0));
    rig.list(&[]);

    rig.poll_once();

    let lines = rig.session_lines();
    assert_eq!(
        rig.decision(OWNED_ID),
        "revive",
        "the claim does not hold a dead pane: {lines:?}"
    );
    assert_eq!(
        rig.decision(UNOWNED_ID),
        "revive",
        "and the control takes the same arm: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        0,
        "a dead pane is not claimed again: {lines:?}"
    );
    assert_eq!(rig.ends_of(OWNED_ID).len(), 1, "{lines:?}");
    assert_eq!(rig.ends_of(UNOWNED_ID).len(), 1, "{lines:?}");
}

/// A seat's own shell carries the agent's config directory, set by the
/// controller on every child it spawns, and a rig that leaves it standing
/// reads its transcripts out of the operator's real one. The bare witness: the
/// arms above answer the same only when the rig has shadowed it, and this arm
/// reds bare when the shadow is lost.
#[test]
fn the_rig_shadows_the_config_directory_a_seats_shell_carries() {
    let decoy = std::env::temp_dir().join("adoption-decoy-config-dir");
    std::env::set_var(common::hermetic::CONFIG_DIR, &decoy);
    let rig = Rig::new("shadows-the-config-dir");
    assert_eq!(
        std::env::var_os(common::hermetic::CONFIG_DIR),
        Some(rig.root.join(".claude").into_os_string()),
        "the rig's environment block leaves the agent's config directory where the shell had it"
    );
}

/// A background row in the listing's own shape: a short id, and a pid while
/// the daemon keeps it running (lessons claude-code A3, A6).
fn background_row(session: &str, short: &str, worktree: &Path, pid: Option<u32>) -> String {
    let pid = pid.map(|pid| format!(",\"pid\":{pid}")).unwrap_or_default();
    format!(
        "{{\"id\":\"{short}\",\"sessionId\":\"{session}\",\"cwd\":\"{}\",\"kind\":\"background\",\
         \"state\":\"done\",\"startedAt\":1000{pid}}}",
        worktree.display()
    )
}

/// An interactive row as a seat's own session reads (lessons claude-code B10):
/// a pid and a status, and no short id.
fn interactive_row(session: &str, worktree: &Path, pid: u32) -> String {
    format!(
        "{{\"sessionId\":\"{session}\",\"cwd\":\"{}\",\"kind\":\"interactive\",\
         \"pid\":{pid},\"status\":\"idle\",\"startedAt\":1000}}",
        worktree.display()
    )
}

/// THE UPGRADE ADOPTS NOTHING (ruling 10): A SEAT THE CLAUDE CODE DAEMON STILL
/// HOSTS STOPS THE START, NAMED, BEFORE ANYTHING IS TOUCHED.
///
/// The listing carries a live background row in the owned seat's worktree —
/// the shape a seat started before fleet ran its sessions itself reads. The
/// loop exits 1, reads no seat's listing and makes no host call, and writes no
/// `controller.started`; the lines it refuses with name the seat, the session,
/// its short id and the command that stops it.
#[test]
fn a_seat_the_daemon_still_hosts_stops_the_start_and_is_named() {
    let rig = Rig::new("daemon-hosted");
    let listing = format!(
        "[{}]",
        background_row(OWNED_SESSION, "ab12", &rig.owned, Some(4242))
    );
    let asked = Mutex::new(Vec::new());
    let daemon = |dir: Option<&Path>| -> Result<Vec<Hosted>, String> {
        asked.lock().unwrap().push(dir.map(Path::to_path_buf));
        parse_hosted(&listing)
    };

    assert_eq!(rig.start_reading(&daemon), run::EXIT_DAEMON_HOSTED);
    assert_eq!(run::EXIT_DAEMON_HOSTED, 1);
    assert!(
        rig.host.calls().is_empty(),
        "no host call before the refusal: {:?}",
        rig.host.verbs()
    );
    assert!(
        rig.stub
            .verbs()
            .iter()
            .all(|verb| *verb == StubAgent::CAPABILITIES),
        "and no read of the agent — only its declaration, taken before the \
         policy is gated: {:?}",
        rig.stub.verbs()
    );
    assert!(!rig.started(), "no controller.started: {:?}", rig.stream());
    assert_eq!(
        *asked.lock().unwrap(),
        vec![None],
        "the fleet's own listing, and no directory the table does not record"
    );

    assert_eq!(
        rig.check(&daemon),
        run::DaemonCheck::Hosted(vec![
            format!(
                "{OWNED} is hosted by the Claude Code daemon: session {OWNED_SESSION}, \
                 short id ab12, in {}",
                rig.owned.display()
            ),
            run::DAEMON_REMEDY.to_string(),
            "  claude stop ab12".to_string(),
        ])
    );
}

/// A SEAT'S RECORDED DIRECTORY IS READ TOO, AND ITS STOP NAMES IT.
///
/// A spawned seat's session is listed under its own configuration directory
/// and no other (lessons claude-code A11, B10), so the check reads every
/// directory the table records for a seat, and the command it hands the person
/// runs under that same directory.
#[test]
fn a_seat_hosted_under_its_own_directory_is_named_with_that_directory() {
    let rig = Rig::new("daemon-hosted-dir").with_the_control_under("/cfg/control");
    let listing = format!(
        "[{}]",
        background_row(UNOWNED_SESSION, "cd34", &rig.unowned, Some(4343))
    );
    let daemon = |dir: Option<&Path>| -> Result<Vec<Hosted>, String> {
        match dir {
            Some(dir) if dir == Path::new("/cfg/control") => parse_hosted(&listing),
            _ => parse_hosted("[]"),
        }
    };

    assert_eq!(rig.start_reading(&daemon), run::EXIT_DAEMON_HOSTED);
    assert_eq!(
        rig.check(&daemon),
        run::DaemonCheck::Hosted(vec![
            format!(
                "{UNOWNED} is hosted by the Claude Code daemon: session {UNOWNED_SESSION}, \
                 short id cd34, in {}",
                rig.unowned.display()
            ),
            run::DAEMON_REMEDY.to_string(),
            "  CLAUDE_CONFIG_DIR=/cfg/control claude stop cd34".to_string(),
        ])
    );
}

/// A LISTING OF THE SEATS' OWN SESSIONS LETS THE START GO ON.
///
/// An interactive row carries no short id (B10), so it is nobody's daemon's;
/// and a background row the daemon no longer runs — pid-less, `done`, which is
/// what the refusal's own stop leaves listed (A3) — does not hold the start
/// either, or a person who ran the command the refusal named would meet it
/// again. A row outside every seat's worktree is not a seat's.
#[test]
fn a_listing_of_interactive_rows_lets_the_start_go_on() {
    let rig = Rig::new("daemon-clear");
    let elsewhere = rig.root.join("elsewhere");
    let listing = format!(
        "[{},{},{}]",
        interactive_row(OWNED_SESSION, &rig.owned, 4242),
        background_row(UNOWNED_SESSION, "cd34", &rig.unowned, None),
        background_row("a-person-s-session", "ef56", &elsewhere, Some(4444)),
    );
    let daemon = |_: Option<&Path>| -> Result<Vec<Hosted>, String> { parse_hosted(&listing) };

    assert_eq!(rig.check(&daemon), run::DaemonCheck::Clear);
    assert_eq!(rig.start_reading(&daemon), 0);
    assert!(rig.started(), "the controller started: {:?}", rig.stream());
}

/// A LISTING THAT CANNOT BE READ IS SAID ONCE, AND THE START GOES ON
/// (reviewer call 2026-09-25, 2): refusing would leave a fleet whose listing
/// breaks unable to start at all, and the poll reads a seat it cannot see as
/// unknown, which nothing is started over.
#[test]
fn an_unreadable_listing_lets_the_start_go_on() {
    let rig = Rig::new("daemon-unreadable");
    let daemon = |_: Option<&Path>| -> Result<Vec<Hosted>, String> { parse_hosted("") };

    assert_eq!(
        rig.check(&daemon),
        run::DaemonCheck::Unreadable(
            "the fleet's listing: the listing answered with zero bytes and a success status"
                .to_string()
        )
    );
    assert_eq!(rig.start_reading(&daemon), 0);
    assert!(rig.started(), "the controller started: {:?}", rig.stream());
}
