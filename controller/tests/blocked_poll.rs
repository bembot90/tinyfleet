//! A RUN'S STEPS RUN ON THE POLLING THREAD, and the poll that follows one must
//! not read the gap as a fleet that went away.
//!
//! `runs::tick` is called from inside the loop's own tick and executes the run
//! through the seam, so a land step that takes twenty minutes is twenty minutes
//! in which not one row is read. The poll that finally reads them holds a live
//! sighting twenty minutes old for every seat on the fleet — and the rows it
//! reads went pid-less somewhere inside that gap. A transit window measured from
//! the sighting is closed on exactly the poll it exists for, which is what put
//! three `/wake` turns on three live idle sessions six seconds after a run
//! failed at its land step, twice in one sitting.
//!
//! A test binary of its own, for the reason `rehost.rs` is one: these arms drive
//! the loop in this process and it reads the PROCESS's environment for the
//! machine directory and the agent binary.
//!
//! THE FAKE CLOCK IS THE SUBJECT HERE. The land step is fake time spent on the
//! same clock the naps are spent on, and the loop's poll stamp comes through
//! [`Clock::now_ms`], so the gap this rig drives is a gap the windows can
//! measure. Under a real-clock stamp both arms below pass against either rule,
//! which is the shape that let the defect ship.

use fleet_controller::adapter::claude_code::parse_roster;
use fleet_controller::adapter::{DaemonRead, RosterRead};
use fleet_controller::clock::Clock;
use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options, Seams, StopHandler};
use fleet_controller::runs::Runs;
use fleet_controller::test_support::{Answers, FakeClock, StubAgent};
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

/// The fleet: three seats, every one of them standing on a live session when
/// the run starts. Three is the smallest roster the upgrade shape can read —
/// its floor is two pid-less seats AND half the fleet — so a poll that reads all
/// three pid-less is the whole-fleet reading the incident's log line carried.
/// Each row is keyed by its id, and the loop names each seat by the machine
/// name its id gives, in the same order.
const SEAT_IDS: [&str; 3] = [
    "01a0d1f1-0aec-765f-9abe-5c21e8a04b17",
    "01a0d1f1-0aec-765f-9abe-d4f993b9739a",
    "01a0d1f1-0aec-765f-9abe-00007e3fa2c0",
];
const SEATS: [&str; 3] = ["agent-e8a04b17", "agent-93b9739a", "agent-7e3fa2c0"];
const SESSIONS: [&str; 3] = ["a-session", "b-session", "c-session"];
const ADDRESSES: [&str; 3] = ["ab12", "cd34", "ef56"];
const PROJECT: &str = "a-project";

/// The run whose land step occupies the polling thread.
const RUN: &str = "a-run";

const POLL_SECONDS: u64 = 1;
/// The arrival window, from the rig's own policy file: small enough that the
/// control arm can close it in a handful of ticks.
const WINDOW_SECONDS: u64 = 5;

/// The land step, as fake time spent on the polling thread. Of the same order
/// as the real one: the run this arm is written from spent 24m 09s between its
/// `run.started` and its `run.failed`, and no roster was read in any of it.
const LAND_STEP: Duration = Duration::from_secs(20 * 60);

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

/// A machine directory naming three seats, and NO session table at all: these
/// are another manager's sessions, exactly as the porter's are to a foreground
/// controller started beside them.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    worktrees: Vec<PathBuf>,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("fleet-blocked-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let worktrees: Vec<PathBuf> = SEATS
            .iter()
            .map(|seat| root.join("wt").join(seat))
            .collect();
        let rig = Rig {
            machine: root.join("machine"),
            worktrees,
            root,
            _held: held,
        };
        for worktree in &rig.worktrees {
            std::fs::create_dir_all(worktree).expect("a seat's worktree is made");
        }

        write(
            &rig.root.join("fleet.toml"),
            &format!(
                "[controller]\npoll_seconds = {POLL_SECONDS}\n\
                 arrival_window_seconds = {WINDOW_SECONDS}\n"
            ),
        );
        let children: Vec<String> = SEAT_IDS
            .iter()
            .zip(&rig.worktrees)
            .map(|(id, worktree)| {
                format!(
                    "{{\"id\": \"{id}\", \"worktrees\": {{\"{PROJECT}\": \"{}\"}}}}",
                    worktree.display()
                )
            })
            .collect();
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": [{}]}}\n",
                rig.root.join("fleet.toml").display(),
                children.join(", ")
            ),
        );
        // THE RUN, WAITING, and a line written after the position it recorded:
        // that pair is what the runs pass reads as a run somebody else has moved
        // since it parked, and it is the one shape that makes the pass execute
        // it. Seeded here rather than left to the loop's own first line, so the
        // arm does not depend on which event the start happens to write.
        append(
            &rig.stream_path(),
            "run.waiting",
            "a-seat",
            serde_json::json!({"run": RUN, "seq": 0}),
        );
        append(
            &rig.stream_path(),
            "hold.cleared",
            "a-seat",
            serde_json::json!({"item": RUN, "letter": "A"}),
        );

        for (key, value) in common::hermetic::vars(&rig.root, &rig.machine, None) {
            std::env::set_var(key, value);
        }
        platform::clear_stop();
        rig
    }

    fn stream_path(&self) -> PathBuf {
        self.machine.join("events.jsonl")
    }

    /// The listing for one tick: every seat's row carries a pid or none of them
    /// does, which is the whole-fleet reading the shape rule fires on. Both
    /// rows started long enough ago to be past the newborn grace, and neither is
    /// marked ended.
    fn roster(&self, live: bool) -> RosterRead {
        let pid = match live {
            true => ",\"pid\":4242",
            false => "",
        };
        let rows: Vec<String> = (0..SEATS.len())
            .map(|i| {
                format!(
                    "{{\"id\":\"{}\",\"sessionId\":\"{}\",\"cwd\":\"{}\",\"kind\":\"background\",\
                     \"status\":\"idle\",\"startedAt\":1000{pid}}}",
                    ADDRESSES[i],
                    SESSIONS[i],
                    self.worktrees[i].display()
                )
            })
            .collect();
        parse_roster(&format!("[{}]", rows.join(",")))
    }

    fn grant(&self) -> Grant {
        Grant::new(platform::directory_listing(), Duration::from_secs(5))
    }

    fn stream(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.stream_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Every line the run wrote, as `<type> <actor>` pairs — the actor beside
    /// the kind, because what each arm asserts is WHICH seat got which line.
    fn lines(&self) -> Vec<String> {
        self.stream()
            .into_iter()
            .filter_map(|event| {
                let kind = event["type"].as_str()?.to_string();
                let actor = event["actor"].as_str().unwrap_or("-").to_string();
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
            .find(|row| row["seat_dir"] == seat)
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

/// Append one line to the stream at the sequence the FILE has room for, which is
/// how the loop's own log appends: the run's back half is another writer, and a
/// sequence held in memory would hand it a number a controller line has taken.
fn append(path: &Path, kind: &str, actor: &str, payload: serde_json::Value) {
    let next = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| value.get("seq").and_then(serde_json::Value::as_u64))
        .max()
        .unwrap_or(0)
        + 1;
    let line = serde_json::json!({
        "id": format!("fixture-{next:08}"),
        "seq": next,
        "ts": "2026-09-20T19:01:36Z",
        "type": kind,
        "actor": actor,
        "payload": payload,
    });
    let mut body = std::fs::read_to_string(path).unwrap_or_default();
    body.push_str(&line.to_string());
    body.push('\n');
    write(path, &body);
}

/// The run as the loop acts on it: one execution that occupies the polling
/// thread for the land step and then fails, exactly as core's `run` child does.
///
/// The hold and the retire answer refusals and are asserted never to have been
/// called: this run spawned no seat, so the cleanup that follows its failure
/// walks an empty set and its `run.cleaned` carries a count of zero — which is
/// what the live stream carried on all three of the incident's runs.
struct ALandStepOnThePollingThread<'a> {
    clock: &'a Ticker<'a>,
    stream: PathBuf,
    calls: Mutex<Vec<String>>,
}

impl Runs for ALandStepOnThePollingThread<'_> {
    fn rerun(&self, run: &str) -> Result<(), String> {
        self.calls
            .lock()
            .expect("the run stub's own lock")
            .push(format!("rerun {run}"));
        // The step itself: no roster is read while it runs, because the thread
        // that reads them is this one.
        self.clock.spend(LAND_STEP);
        append(
            &self.stream,
            "run.failed",
            "controller",
            serde_json::json!({
                "run": run,
                "reason": {"code": "refused", "verb": "land", "why": "the suite exited 1"},
            }),
        );
        Ok(())
    }

    fn hold(&self, run: &str, _reason: &str) -> Result<String, String> {
        self.calls
            .lock()
            .expect("the run stub's own lock")
            .push(format!("hold {run}"));
        Err("this rig raises no hold".to_string())
    }

    fn retire(&self, seat: &str, run: &str) -> Result<(), String> {
        self.calls
            .lock()
            .expect("the run stub's own lock")
            .push(format!("retire {seat} {run}"));
        Err("this run spawned no seat".to_string())
    }
}

/// The clock the ticks are driven on: fake time, a listing swapped in as each
/// whole poll interval passes, and a stop asked for once the arm's ticks have
/// run.
///
/// THE SWAP AND THE STOP ARE BOTH COUNTED IN POLLS THE AGENT ANSWERED, which is
/// the difference from `rehost.rs`'s ticker: a nap sleeps in slices, so a rig
/// keyed to the call would move the roster ten times between two polls, and the
/// land step spends twenty fake minutes with no poll in it, so a rig keyed to
/// elapsed fake time would end the run on the first nap after it. One poll is
/// one listing read, and the stub counts those for itself.
struct Ticker<'a> {
    inner: FakeClock,
    stub: &'a StubAgent,
    rosters: Mutex<VecDeque<RosterRead>>,
    swapped: Mutex<usize>,
    stop_after_polls: usize,
}

impl<'a> Ticker<'a> {
    fn new(
        stub: &'a StubAgent,
        rosters: VecDeque<RosterRead>,
        stop_after_polls: usize,
    ) -> Ticker<'a> {
        Ticker {
            inner: FakeClock::new(),
            stub,
            rosters: Mutex::new(rosters),
            swapped: Mutex::new(0),
            stop_after_polls,
        }
    }

    /// Fake time spent by something that is not a nap — the run's own step.
    fn spend(&self, d: Duration) {
        self.inner.advance(d);
    }
}

impl Clock for Ticker<'_> {
    fn now(&self) -> Instant {
        self.inner.now()
    }

    /// The poll's wall-clock stamp, from the same fake the naps and the land
    /// step are spent against. A stamp off the real clock leaves every window
    /// this rig drives at microseconds, whatever the ticks spent.
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        let polls = self.stub.calls_of(StubAgent::STATUS).len();
        let mut swapped = self.swapped.lock().expect("the ticker's own lock");
        if polls > *swapped {
            *swapped = polls;
            if let Some(next) = self
                .rosters
                .lock()
                .expect("the ticker's own lock")
                .pop_front()
            {
                self.stub.set(|answers| answers.status = next);
            }
        }
        if polls >= self.stop_after_polls {
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
            let until = Instant::now() + Duration::from_secs(10);
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

fn answers(rig: &Rig) -> Answers {
    Answers {
        status: rig.roster(true),
        // No daemon pid and no uptime: the replacement window reads nothing, so
        // the only hold that can answer here is the one keyed to the rows.
        daemon: DaemonRead::Readable(None),
        transcript: Some(A_LIGHT_TRANSCRIPT.to_string()),
        ended_at: Some(now_ms()),
        ..Answers::default()
    }
}

/// The whole arm, driven once: the ticks, the stub's own record of what it was
/// asked to do, and the rig's stream.
fn fly(rig: &Rig, stub: &StubAgent, clock: &Ticker, runs: &dyn Runs) {
    let watchdog = Watchdog::armed();
    let status = run::observe_seamed(
        &Options { once: false },
        rig.grant(),
        Some(runs),
        Seams {
            clock,
            agent: stub,
            child_path: "",
            effects_off: None,
            stop_handler: StopHandler::Unarmed,
        },
    );
    drop(watchdog);
    assert_eq!(status, 0, "the loop ended on the stop it was asked for");
}

/// THE FLEET GOES PID-LESS ACROSS A RUN'S LAND STEP AND IS BACK ON THE NEXT
/// POLL: the poll in the middle dispatches nothing.
///
/// Three ticks. The first reads every seat live and then spends twenty fake
/// minutes inside the run, which fails; the second is the first reading since —
/// every row pid-less, the whole-fleet shape — and the third reads them listed
/// again. What the middle tick did is read from the stream and not from a
/// decision: a revive writes its line, moves the blind counter and reaches the
/// agent, so the absence of all three is what says the fleet was held.
#[test]
fn a_fleet_that_went_pid_less_inside_a_runs_land_step_is_held_on_the_poll_after_it() {
    let rig = Rig::new("held-across-a-land-step");
    let stub = StubAgent::answering(answers(&rig));
    // Tick 2 reads every row pid-less; tick 3 reads them listed again.
    let clock = Ticker::new(
        &stub,
        VecDeque::from(vec![rig.roster(false), rig.roster(true)]),
        3,
    );
    let runs = ALandStepOnThePollingThread {
        clock: &clock,
        stream: rig.stream_path(),
        calls: Mutex::new(Vec::new()),
    };

    fly(&rig, &stub, &clock, &runs);

    // THREE TICKS RAN, read from the agent rather than assumed: every seat is
    // decided against the fleet's own listing, so a tick is one roster read.
    assert_eq!(
        stub.calls_of(StubAgent::STATUS).len(),
        3,
        "the loop polled three times: {:?}",
        stub.verbs()
    );
    // And the run was executed, once, on the polling thread.
    let lines = rig.lines();
    assert_eq!(
        *runs.calls.lock().expect("the run stub's own lock"),
        vec![format!("rerun {RUN}")],
        "the run was executed once and nothing else was asked of it: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading("run.failed controller"),
        1,
        "the run failed at its land step: {lines:?}"
    );

    // THE CLEANUP THAT FOLLOWS THE FAILURE TOUCHES NO SESSION: it walks the
    // seats the run SPAWNED, and this run spawned none, so its line carries a
    // count of zero — the reading all three of the incident's runs carried.
    assert_eq!(
        rig.lines_reading("run.cleaned controller"),
        1,
        "the failure was cleaned up once: {lines:?}"
    );
    let cleaned = rig
        .stream()
        .into_iter()
        .find(|event| event["type"] == "run.cleaned")
        .expect("the cleanup's own line");
    assert_eq!(
        cleaned["payload"]["count"], 0,
        "and it retired nothing: {cleaned}"
    );

    for seat in SEAT_IDS {
        assert_eq!(
            rig.lines_reading(&format!("session.revived {seat}")),
            0,
            "no tick attached to {seat}'s session: {lines:?}"
        );
        assert_eq!(
            rig.lines_reading(&format!("session.spawned {seat}")),
            0,
            "nor started a second session beside it: {lines:?}"
        );
        assert_eq!(
            rig.lines_reading(&format!("dispatch.blind {seat}")),
            0,
            "and {seat}'s counter did not move, because nothing was dispatched: {lines:?}"
        );
        assert_eq!(
            rig.decision(seat),
            "leave-alone",
            "the tick that read {seat} listed again left it alone: {lines:?}"
        );
    }
    assert!(
        stub.calls_of(StubAgent::REVIVE).is_empty(),
        "and the agent was asked to attach to nothing: {:?}",
        stub.verbs()
    );
}

/// THE CONTROL: the same roster pid-less through the whole window, and every
/// seat is revived — once each, at the window's close.
///
/// It differs from the arm above in ONE term — the rows never come back — so
/// the silence up there is a reading of the transit and not a fixture that never
/// reached the revive arm. The revive is what a fleet that really went away is
/// owed, and holding it forever would be the defect on the other side.
#[test]
fn a_fleet_that_stays_pid_less_through_the_window_is_revived_once_each_at_its_close() {
    let rig = Rig::new("revived-at-the-windows-close");
    let stub = StubAgent::answering(answers(&rig));
    // Every tick after the first reads the same pid-less roster. The window is
    // WINDOW_SECONDS and a nap is POLL_SECONDS, so it closes inside these ticks
    // and the polls after the revive are what prove it fires only once.
    let after = (WINDOW_SECONDS / POLL_SECONDS + 3) as usize;
    let rosters: VecDeque<RosterRead> = (0..after).map(|_| rig.roster(false)).collect();
    let clock = Ticker::new(&stub, rosters, after + 1);
    let runs = ALandStepOnThePollingThread {
        clock: &clock,
        stream: rig.stream_path(),
        calls: Mutex::new(Vec::new()),
    };

    fly(&rig, &stub, &clock, &runs);

    let lines = rig.lines();
    for seat in SEAT_IDS {
        assert_eq!(
            rig.lines_reading(&format!("session.revived {seat}")),
            1,
            "{seat} was revived exactly once: {lines:?}"
        );
        assert_eq!(
            rig.lines_reading(&format!("dispatch.blind {seat}")),
            1,
            "and its counter moved exactly once with it: {lines:?}"
        );
    }
    // One attach per seat and no more, read from the agent: the polls after the
    // revive are held by the arrival window the dispatch itself opened.
    let mut attached: Vec<String> = stub
        .calls_of(StubAgent::REVIVE)
        .into_iter()
        .map(|call| call.about)
        .collect();
    attached.sort();
    assert_eq!(
        attached,
        ADDRESSES.map(str::to_string).to_vec(),
        "every seat's address, once: {:?}",
        stub.verbs()
    );
}
