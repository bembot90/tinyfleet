//! A SESSION WHOSE HOST IS REPLACED MID-RUN IS HELD, NEVER REVIVED.
//!
//! A controller polling beside another manager's seats reads one of their live
//! sessions pid-less for the poll its hosted process is replaced on, with the
//! daemon itself unchanged. Attaching there spends a dispatch — a whole `/wake`
//! turn, on a seat whose first turn is one — on a session that is already there
//! and is listed again on its own within about a minute.
//!
//! A test binary of its own, because these arms drive the loop in this process
//! and it reads the PROCESS's environment for the machine directory and the
//! agent binary: an arm setting those beside arms that do not would be setting
//! them for every thread in the binary.
//!
//! THE ROSTER MOVES UNDER THE LOOP, which is the whole subject — one seat live,
//! then pid-less, then live again — so the rig drives three ticks and swaps the
//! listing the stub answers with between them.
//!
//! NEITHER SESSION IS IN THE TABLE, and that is the fixture rather than an
//! omission: the session a controller finds re-hosting is one another manager
//! started, which no row of this controller's table names, so the claim that
//! spares an ADOPTED row cannot reach it and the row arrives at the revive arm
//! exactly as the recorded one did.

use fleet_controller::adapter::claude_code::parse_roster;
use fleet_controller::adapter::{DaemonRead, RosterRead};
use fleet_controller::clock::Clock;
use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options, Seams, StopHandler};
use fleet_controller::test_support::{Answers, FakeClock, FakeHost, StubAgent};
use std::collections::VecDeque;
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

/// The seat whose session is live on the first tick and re-hosting on the
/// second — the subject. Its row is keyed by the id, and everything the loop
/// writes about it names it by the machine name that id gives.
const HOSTED_ID: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const HOSTED: &str = "agent-e8a04b17";
/// The seat standing on a pid-less row from the first tick on, which this
/// controller has therefore never seen live. It is the control: no sighting
/// covers it, so the revive it takes is the one the subject was spared.
const NEVER_LIVE_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const NEVER_LIVE: &str = "agent-93b9739a";
const PROJECT: &str = "a-project";
const HOSTED_SESSION: &str = "a-session";
const HOSTED_ADDRESS: &str = "ab12";
const NEVER_LIVE_SESSION: &str = "b-session";
const NEVER_LIVE_ADDRESS: &str = "cd34";

/// The poll interval the rig's policy file carries, and the fake time one nap
/// spends against the clock that drives the ticks.
const POLL_SECONDS: u64 = 1;

/// How many ticks the arm drives: the poll that sees the session live, the poll
/// that reads it pid-less, and the poll that sees it listed again.
const TICKS: u64 = 3;

/// One main-chain assistant turn carrying a window well under the rest
/// threshold, which is what makes a pid-less row's verdict a revive rather than
/// a spawn: the listing carries no token field, so the transcript is the only
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

/// A machine directory naming two seats, and NO session table at all: nothing
/// here is a session this controller started.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    hosted: PathBuf,
    never_live: PathBuf,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("fleet-rehost-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            machine: root.join("machine"),
            hosted: root.join("wt").join(HOSTED),
            never_live: root.join("wt").join(NEVER_LIVE),
            root,
            _held: held,
        };
        std::fs::create_dir_all(&rig.hosted).expect("the hosted seat's worktree is made");
        std::fs::create_dir_all(&rig.never_live).expect("the control seat's worktree is made");

        write(
            &rig.root.join("fleet.toml"),
            &format!("[controller]\npoll_seconds = {POLL_SECONDS}\n"),
        );
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": [\
                 {{\"id\": \"{HOSTED_ID}\", \"worktrees\": {{\"{PROJECT}\": \"{}\"}}}}, \
                 {{\"id\": \"{NEVER_LIVE_ID}\", \"worktrees\": {{\"{PROJECT}\": \"{}\"}}}}]}}\n",
                rig.root.join("fleet.toml").display(),
                rig.hosted.display(),
                rig.never_live.display()
            ),
        );
        common::hermetic::export(common::hermetic::vars(&rig.root, &rig.machine, None));
        platform::clear_stop();
        rig
    }

    /// The listing for one tick. The hosted seat's row carries a pid or does
    /// not; the control's never does. Both rows started long enough ago to be
    /// past the newborn grace, and neither is marked ended.
    fn roster(&self, hosted_is_live: bool) -> RosterRead {
        let pid = match hosted_is_live {
            true => ",\"pid\":4242",
            false => "",
        };
        parse_roster(&format!(
            "[{{\"id\":\"{HOSTED_ADDRESS}\",\"sessionId\":\"{HOSTED_SESSION}\",\"cwd\":\"{}\",\
             \"kind\":\"background\",\"status\":\"idle\",\"startedAt\":1000{pid}}},\
             {{\"id\":\"{NEVER_LIVE_ADDRESS}\",\"sessionId\":\"{NEVER_LIVE_SESSION}\",\
             \"cwd\":\"{}\",\"kind\":\"background\",\"status\":\"idle\",\"startedAt\":1000}}]",
            self.hosted.display(),
            self.never_live.display()
        ))
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

    /// Every line the run wrote, as `<type> <actor id>` pairs — the actor beside
    /// the kind, because what each arm asserts is WHICH seat got which line.
    fn lines(&self) -> Vec<String> {
        self.stream()
            .into_iter()
            .filter_map(|event| {
                let kind = event["type"].as_str()?.to_string();
                let actor = event["actor"]["id"].as_str().unwrap_or("-").to_string();
                Some(format!("{kind} {actor}"))
            })
            .collect()
    }

    fn lines_reading(&self, line: &str) -> usize {
        self.lines().into_iter().filter(|seen| seen == line).count()
    }

    fn decision(&self, seat: &str) -> String {
        let body = std::fs::read_to_string(self.machine.join("projection.json"))
            .expect("a projection is published");
        let document: serde_json::Value =
            serde_json::from_str(&body).expect("the projection parses");
        document["seats"]
            .as_array()
            .expect("the projection carries seats")
            .iter()
            .find(|row| row["seat"]["id"] == seat)
            .unwrap_or_else(|| panic!("{seat} is published: {document}"))["decision"]
            .as_str()
            .unwrap_or_else(|| panic!("{seat}'s decision is published: {document}"))
            .to_string()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        platform::clear_stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The clock the ticks are driven on: fake time, a listing swapped in as each
/// whole poll interval passes, and a stop asked for once [`TICKS`] of them have.
///
/// The swap is keyed to the INTERVAL and not to the call, because a nap sleeps
/// in slices: a rig that moved the roster on every `sleep` would move it ten
/// times between two polls.
struct Ticker<'a> {
    inner: FakeClock,
    stub: &'a StubAgent,
    rosters: Mutex<VecDeque<RosterRead>>,
    poll: Duration,
    stop_after: Duration,
}

impl<'a> Ticker<'a> {
    fn new(stub: &'a StubAgent, rosters: VecDeque<RosterRead>) -> Ticker<'a> {
        Ticker {
            inner: FakeClock::new(),
            stub,
            rosters: Mutex::new(rosters),
            poll: Duration::from_secs(POLL_SECONDS),
            stop_after: Duration::from_secs(POLL_SECONDS * TICKS),
        }
    }
}

impl Clock for Ticker<'_> {
    fn now(&self) -> Instant {
        self.inner.now()
    }

    /// The poll's wall-clock stamp, from the same fake the naps are spent
    /// against: the windows these ticks drive are differences between two of
    /// these, so a stamp off the real clock would leave every one of them at
    /// microseconds however much fake time the ticks spent.
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    fn sleep(&self, d: Duration) {
        let before = self.inner.spent().as_millis() / self.poll.as_millis();
        self.inner.sleep(d);
        let after = self.inner.spent();
        if after.as_millis() / self.poll.as_millis() > before {
            if let Some(next) = self
                .rosters
                .lock()
                .expect("the ticker's own lock")
                .pop_front()
            {
                self.stub.set(|answers| answers.status = next);
            }
        }
        if after >= self.stop_after {
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

/// LIVE, PID-LESS, LISTED AGAIN: the poll in the middle dispatches nothing.
///
/// The three ticks are the whole of a re-host as a controller sees it, and the
/// arm reads what the middle one did from the stream rather than from a
/// decision: a revive writes its line, moves the blind counter and reaches the
/// agent, so the absence of all three is what says the row was held.
///
/// The control seat differs in ONE term — this controller never saw it live —
/// and IS revived, which is what makes the subject's silence a reading of the
/// sighting rather than a fixture that never reached the revive arm.
#[test]
fn a_session_that_is_re_hosted_between_polls_is_held_and_the_seat_never_seen_live_is_revived() {
    let rig = Rig::new("held-across-a-re-host");
    let stub = StubAgent::answering(Answers {
        status: rig.roster(true),
        // No daemon pid and no uptime: the replacement window reads nothing, so
        // the hold this arm measures is the one keyed to the SESSION's host.
        daemon: DaemonRead::Readable(None),
        transcript: Some(A_LIGHT_TRANSCRIPT.to_string()),
        ended_at: Some(now_ms()),
        ..Answers::default()
    });
    // Tick 2 reads the hosted session pid-less; tick 3 reads it listed again.
    let clock = Ticker::new(
        &stub,
        VecDeque::from(vec![rig.roster(false), rig.roster(true)]),
    );
    let watchdog = Watchdog::armed();

    let host = FakeHost::new();

    let status = run::observe_seamed(
        &Options { once: false },
        rig.grant(),
        None,
        Seams {
            clock: &clock,
            agent: &stub,
            host: &host,
            child_path: "",
            effects_off: None,
            stop_handler: StopHandler::Unarmed,
        },
    );
    drop(watchdog);
    assert_eq!(status, 0, "the loop ended on the stop it was asked for");

    // THREE TICKS RAN, read from the agent rather than assumed: both seats are
    // decided against the fleet's own listing, so a tick is one roster read.
    assert_eq!(
        stub.calls_of(StubAgent::STATUS).len() as u64,
        TICKS,
        "the loop polled three times: {:?}",
        stub.verbs()
    );

    let lines = rig.lines();
    assert_eq!(
        rig.lines_reading(&format!("session.revived {HOSTED_ID}")),
        0,
        "no tick attached to the re-hosting session: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.spawned {HOSTED_ID}")),
        0,
        "nor started a second session beside it: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("dispatch.blind {HOSTED_ID}")),
        0,
        "and the counter did not move, because nothing was dispatched: {lines:?}"
    );
    assert_eq!(
        rig.decision(HOSTED_ID),
        "leave-alone",
        "the tick that read it listed again left it alone: {lines:?}"
    );

    // The control, and with it the only attach the whole run issued.
    assert_eq!(
        rig.lines_reading(&format!("session.revived {NEVER_LIVE_ID}")),
        1,
        "the seat no sighting covers took the revive, exactly once: {lines:?}"
    );
    let attached: Vec<String> = stub
        .calls_of(StubAgent::REVIVE)
        .into_iter()
        .map(|call| call.about)
        .collect();
    assert_eq!(
        attached,
        vec![NEVER_LIVE_ADDRESS.to_string()],
        "one attach, and it is the control's"
    );
}
