//! The poll loop driven IN THIS PROCESS: no child, no wall clock, no binary.
//!
//! `run::observe_seamed` takes the clock a poll interval is spent against and
//! the agent every call goes through, so an arm here runs the whole tick —
//! read, decide, act, publish — against `FakeClock` and `StubAgent` and reads
//! back both what the loop published and what the agent was asked for.
//!
//! A test binary of its own, because the loop reads the PROCESS's environment
//! for its machine directory: an arm setting that beside arms that do not would
//! be setting it for every thread in the binary. Inside this one the rigs are
//! serialised on the lock each holds for its whole life.

use fleet_controller::adapter::RosterRead;
use fleet_controller::clock::Clock;
use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options, Seams, StopHandler};
use fleet_controller::test_support::{self, Answers, FakeClock, FakeHost, StubAgent};
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

/// The one seat the rig configures, spelled once so the fixture and every
/// assertion about it read the same value: the id its row is keyed by, and the
/// machine name everything the loop writes about it carries.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const SEAT: &str = "agent-e8a04b17";
const PROJECT: &str = "a-project";

/// The poll interval the rig's policy file carries. Ten minutes, so the fake
/// time a nap spends and the wall clock it costs are not the same order of
/// magnitude by accident.
const POLL_SECONDS: u64 = 600;

/// What a whole run of ticks may cost in REAL time. Generous on purpose: this
/// box is shared, and the claim is that the interval is not waited on at all,
/// not that a tick is fast.
const WALL_CEILING: Duration = Duration::from_millis(1_500);

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

/// A machine directory naming one seat, in a worktree that exists.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    worktree: PathBuf,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("fleet-in-process-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            machine: root.join("machine"),
            worktree: root.join("wt").join(SEAT),
            root,
            _held: held,
        };
        std::fs::create_dir_all(&rig.worktree).expect("the worktree is made");
        write(
            &rig.root.join("fleet.toml"),
            &format!("[controller]\npoll_seconds = {POLL_SECONDS}\n"),
        );
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": [\
                 {{\"id\": \"{SEAT_ID}\", \"worktrees\": {{\"{PROJECT}\": \"{}\"}}}}]}}\n",
                rig.root.join("fleet.toml").display(),
                rig.worktree.display()
            ),
        );
        common::hermetic::export(common::hermetic::vars(&rig.root, &rig.machine, None));
        // The stop flag is process-wide and the arm below raises it, so every
        // rig starts from a fleet nobody has asked to stop.
        platform::clear_stop();
        rig
    }

    /// The gate, over the worktree the seat names — which is there, so the read
    /// is granted and the loop's effects are on.
    fn grant(&self) -> Grant {
        Grant::new(platform::directory_listing(), Duration::from_secs(5))
    }

    fn projection(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine.join("projection.json"))
            .expect("a projection is published");
        serde_json::from_str(&body).expect("the projection parses")
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        platform::clear_stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The clock a run of ticks is driven on: fake time, and a stop asked for once
/// the loop has napped a whole interval.
///
/// The loop leaves on the stop flag alone, so the clock is what ends it — which
/// keeps the run bounded without a signal, and bounds it at the exact wait the
/// arm is about.
struct NapThenStop {
    inner: FakeClock,
    after: Duration,
}

impl NapThenStop {
    fn new(after: Duration) -> NapThenStop {
        NapThenStop {
            inner: FakeClock::new(),
            after,
        }
    }

    fn spent(&self) -> Duration {
        self.inner.spent()
    }
}

impl Clock for NapThenStop {
    fn now(&self) -> Instant {
        self.inner.now()
    }

    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        if self.inner.spent() >= self.after {
            platform::request_stop();
        }
    }
}

/// A REAL-TIME BACKSTOP on a run of ticks.
///
/// The loop's only exit is the stop flag, and every way the nap can break — a
/// wait that leaks onto the thread, a clock that is never spent — is a run that
/// never asks for one. Without this such a break hangs the arm instead of
/// failing it, and an arm that hangs proves nothing. The deadline is far above
/// [`WALL_CEILING`], so on a healthy loop this thread is cancelled having done
/// nothing.
struct Watchdog {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

const WATCHDOG: Duration = Duration::from_secs(5);

impl Watchdog {
    fn armed() -> Watchdog {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let thread = std::thread::spawn(move || {
            let until = Instant::now() + WATCHDOG;
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

/// Every verb one tick of the loop must reach the agent for. The roster read
/// and the version are the observation; the start is the effect the absent
/// seat's verdict asks for.
const A_TICKS_VERBS: [&str; 3] = [StubAgent::STATUS, StubAgent::VERSION_CALL, StubAgent::START];

/// A stub whose listing shows the one session a start on a fresh [`FakeHost`]
/// brings up, by its pane's pid, so the start is believed at its first read
/// and its watch spends no wall clock. The row stands in no seat's worktree,
/// so the seat still reads absent on the tick that starts it.
fn listing_its_arrival() -> StubAgent {
    StubAgent::answering(Answers {
        status: RosterRead::Readable(test_support::arrivals(1)),
        ..Answers::default()
    })
}

fn assert_the_tick_ran(stub: &StubAgent) {
    let verbs = stub.verbs();
    for verb in A_TICKS_VERBS {
        assert!(
            verbs.contains(&verb),
            "the tick reached the agent for `{verb}`; it called {verbs:?}"
        );
    }
}

/// THE WHOLE POINT OF THE SEAM, at the loop and not at the wait.
///
/// The loop is run with no `once`, so it reaches the nap between ticks; the
/// clock spends the interval in fake time and asks for the stop the loop leaves
/// on. What the arm bounds is the pair: a lower bound on the FAKE time the poll
/// interval consumed, and an upper bound on the REAL time the whole run cost.
/// An interval waited on for real fails the second; a loop that skipped the nap
/// fails the first.
#[test]
fn a_run_of_ticks_spends_its_poll_interval_in_fake_time_and_costs_no_wall_clock() {
    let rig = Rig::new("nap");
    let stub = listing_its_arrival();
    let host = FakeHost::new();
    let clock = NapThenStop::new(Duration::from_secs(POLL_SECONDS));
    let watchdog = Watchdog::armed();

    let started = Instant::now();
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
    let wall = started.elapsed();
    drop(watchdog);
    assert_eq!(status, 0, "the loop ended on the stop it was asked for");

    // The interval the loop actually ran on, read from the document the loop
    // published rather than from a second copy of the fixture's own number.
    let document = rig.projection();
    let interval = Duration::from_secs(
        document["fleet"]["poll_seconds"]
            .as_u64()
            .unwrap_or_else(|| panic!("the projection carries the poll interval: {document}")),
    );

    assert!(
        clock.spent() >= interval,
        "the nap spent {interval:?} of fake time; it spent {:?}",
        clock.spent()
    );
    assert!(
        wall < WALL_CEILING,
        "{interval:?} of poll interval cost {wall:?} of wall clock, over the {WALL_CEILING:?} bound"
    );
    assert_the_tick_ran(&stub);
}

/// And the tick itself, with no process behind any of it.
///
/// One poll, driven through the same entry: the arm reads what the agent was
/// ASKED for — the roster, the version, the daemon, and the start the absent
/// seat's verdict calls for — and what the start was given, so a loop that
/// returned before reaching its effect fails here rather than passing quietly.
#[test]
fn one_tick_reaches_the_agent_for_every_call_the_poll_makes_and_publishes_its_outcome() {
    let rig = Rig::new("tick");
    let stub = listing_its_arrival();
    let host = FakeHost::new();
    let clock = FakeClock::new();

    let status = run::observe_seamed(
        &Options { once: true },
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
    assert_eq!(status, 0);

    assert_the_tick_ran(&stub);
    assert_eq!(
        clock.spent(),
        Duration::ZERO,
        "a single poll naps not at all"
    );

    let starts = stub.starts();
    assert_eq!(starts.len(), 1, "one seat, one start: {:?}", stub.verbs());
    assert_eq!(starts[0].seat, SEAT_ID, "the start carries the seat's id");
    assert_eq!(
        starts[0].name, SEAT,
        "and its session is named by the machine name"
    );
    assert_eq!(
        Path::new(&starts[0].worktree),
        rig.worktree,
        "the start was issued in the worktree the seat list names"
    );

    let document = rig.projection();
    assert_eq!(document["seats"][0]["seat"]["id"], SEAT_ID, "{document}");
    assert_eq!(document["seats"][0]["outcome"], "spawned", "{document}");
    assert_eq!(
        document["agent_version"],
        StubAgent::VERSION,
        "the published version is the one the agent answered: {document}"
    );
}

/// The stub's answers are the arm's, and they reach the loop.
///
/// The same tick with one field changed: a start the agent refuses is published
/// as a failed outcome and opens no session row. Without this, an arm could not
/// tell a stub the loop consults from one whose answers it ignores.
#[test]
fn a_start_the_agent_refuses_is_published_as_a_failed_outcome() {
    let rig = Rig::new("refused");
    let stub = StubAgent::answering(Answers {
        launch: Err("the arm refused this start".to_string()),
        ..Answers::default()
    });
    let host = FakeHost::new();
    let clock = FakeClock::new();

    let status = run::observe_seamed(
        &Options { once: true },
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
    assert_eq!(status, 0);

    assert_eq!(stub.calls_of(StubAgent::START).len(), 1);
    let document = rig.projection();
    assert_eq!(document["seats"][0]["outcome"], "failed", "{document}");
}
