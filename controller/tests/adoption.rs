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

use fleet_controller::adapter::claude_code::parse_roster;
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
            stub: StubAgent::answering(Answers {
                transcript: Some(A_LIGHT_TRANSCRIPT.to_string()),
                ended_at: Some(now_ms()),
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
        let listing = parse_roster(&format!("[{body}]"));
        self.stub.set(|answers| answers.status = listing);
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
        Seams {
            clock,
            agent: &self.stub,
            host: &self.host,
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

    // THREE TICKS RAN, read from the agent rather than assumed: both seats are
    // decided against the fleet's own listing, so a tick is one listing read.
    assert_eq!(
        rig.stub.calls_of(StubAgent::STATUS).len(),
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
/// behind it — a session fleet does not host, so not claimed, and the seat is
/// left unread rather than started beside it.
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

    // The control: listed live, named by the table, and never the seat's.
    assert_eq!(rig.row(UNOWNED_ID)["roster_state"], "unknown");
    assert_eq!(rig.decision(UNOWNED_ID), "leave-alone");
    assert_eq!(
        rig.lines_reading(&format!("session.spawned {UNOWNED_ID}")),
        0,
        "no second session beside one the listing names: {lines:?}"
    );
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
