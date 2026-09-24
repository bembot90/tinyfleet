//! A CLAIMED SESSION IS NEVER ATTACHED TO, ON ANY POLL.
//!
//! A test binary of its own, because these arms drive the loop in this process
//! and it reads the PROCESS's environment for the machine directory and the
//! agent binary: an arm setting those beside arms that do not would be setting
//! them for every thread in the binary. Inside this one they are serialised on
//! the lock below, which each rig holds for its whole life.
//!
//! What it measures is the thing neither `effect::adopt` nor `decide` can say
//! alone: that the claim adoption takes ONCE is in hand on every later poll's
//! verdicts, so a seat standing on a session the fleet already owns is not
//! attached to as though it were gone — not on the poll that claimed it, and
//! not on the poll after that.
//!
//! TWO SHAPES FOR THE CARRY, because the claim travels two ways. The first arm
//! drives two separate polls, where the only carrier between them is the
//! session table on disk. The second drives two ticks of ONE loop, where
//! adoption runs on the first tick and never again — which is the shape the
//! hourly revives were recorded in.
//!
//! THEN ONE ARM PER ROW SHAPE, because the claim is taken and released on what
//! the row reads: a live idle session is claimed at its first sighting, the
//! claim holds it through the pid-less stretch that follows, an end the roster
//! can NAME releases it, and a pid-less row no claim covers is revived as it
//! always was.

use fleet_controller::adapter::claude_code::parse_roster;
use fleet_controller::adapter::{dir_key, transcript_path, DaemonRead};
use fleet_controller::clock::Clock;
use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options, Seams, StopHandler};
use fleet_controller::test_support::{Answers, FakeClock, StubAgent};
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

/// The seat whose session the table names — the one an adoption can claim. Its
/// row is keyed by the id, and the table and the stream name it by the machine
/// name that id gives.
const OWNED_ID: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const OWNED: &str = "agent-e8a04b17";
/// The seat standing on the same shaped row with nothing in the table naming its
/// session. It is the control: no claim can cover it, so the revive it gets is
/// what the other seat would have got.
const UNOWNED_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const UNOWNED: &str = "agent-93b9739a";
const PROJECT: &str = "a-project";
const OWNED_SESSION: &str = "a-session";
const OWNED_ADDRESS: &str = "ab12";
const UNOWNED_SESSION: &str = "b-session";
const UNOWNED_ADDRESS: &str = "cd34";

/// What the owned seat's row reads this poll, in the shapes the live daemon
/// produces (lessons claude-code A3, re-read on 2.1.261).
///
/// `done` spans the first two: a live idle session carries it with a pid and
/// `status: idle` beside it, and a session that has hibernated, been stopped
/// from idle or been killed from outside carries it pid-less with no status —
/// which is why the word answers no question about whether a session is over.
/// `stopped` is reached only from a non-idle prior state, and is one of the two
/// ends the roster can name on its own.
#[derive(Clone, Copy)]
enum Owned {
    LiveIdle,
    PidlessDone,
    Stopped,
}

impl Owned {
    /// The fields this shape adds to the row, in the daemon's own spelling. A
    /// pid-less row carries NO status: the field is present only while a pid
    /// is.
    fn fields(self) -> &'static str {
        match self {
            Owned::LiveIdle => ",\"pid\":4242,\"state\":\"done\",\"status\":\"idle\"",
            Owned::PidlessDone => ",\"state\":\"done\"",
            Owned::Stopped => ",\"state\":\"stopped\"",
        }
    }
}

/// The poll interval the rigs' policy file carries, and the fake time one nap
/// spends against the clock that ends the two-tick run.
const POLL_SECONDS: u64 = 1;

/// How many ticks the second arm drives. Two is the whole of its subject: the
/// tick adoption runs on, and the one after it that has no claim of its own.
const TICKS: u64 = 2;

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

fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("the stub is executable");
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the wall clock is past the epoch")
        .as_millis() as u64
}

/// A machine directory naming two seats, a listing carrying a pid-less row in
/// each of their worktrees, and a session table that names one of the two
/// sessions and not the other.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    owned: PathBuf,
    unowned: PathBuf,
    argv: PathBuf,
    /// Whether the owned seat's row starts out ALREADY claimed — the state a
    /// poll after the one that adopted it reads back off disk.
    owned_claimed: bool,
    /// The shape the owned seat's row reads on the listing.
    owned_row: Owned,
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
            argv: root.join("agent-argv"),
            owned_claimed: false,
            owned_row: Owned::PidlessDone,
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

        rig.write_the_table();
        rig
    }

    /// The table as a poll before this one left it: the owned seat's row carries
    /// the session id a live sighting wrote onto it. `adopted` is present only
    /// where the rig was asked for a claim an earlier poll took; absent is the
    /// state of every row no adoption has reached yet.
    ///
    /// Its dispatch is far enough back that the arrival window is closed, so the
    /// hold that keeps a fresh dispatch from being re-issued says nothing here
    /// and the verdict is reached on the row itself.
    fn write_the_table(&self) {
        let claim = match self.owned_claimed {
            true => format!(", \"adopted\": \"{OWNED_SESSION}\""),
            false => String::new(),
        };
        write(
            &self.machine.join("sessions.json"),
            &format!(
                "{{\"schema\": 2, \"sessions\": [{{\
                 \"seat\": \"{OWNED_ID}\", \"project\": \"{PROJECT}\", \"worktree\": \"{}\", \
                 \"name\": \"A Seat\", \"model\": \"claude-opus-5\", \"posture\": \"auto\", \
                 \"first_turn\": \"/wake {OWNED}\", \"transient\": false, \
                 \"dispatch_id\": \"an-earlier-dispatch\", \"dispatched_at\": 1000, \
                 \"session_id\": \"{OWNED_SESSION}\", \"short_id\": \"{OWNED_ADDRESS}\"{claim}\
                 }}]}}\n",
                self.owned.display()
            ),
        );
    }

    /// The rig where the claim was taken by an earlier poll: the table names
    /// the session as this fleet's, which is what a controller reads back off
    /// disk after a restart.
    fn with_the_claim_taken(mut self) -> Rig {
        self.owned_claimed = true;
        self.write_the_table();
        self
    }

    /// The rig whose owned row reads `shape` on the listing.
    fn with_the_owned_row(mut self, shape: Owned) -> Rig {
        self.owned_row = shape;
        self
    }

    /// The listing every arm answers with: one row per seat, both started long
    /// enough ago to be past the newborn grace. The control's is always the
    /// pid-less row nothing claims; the owned seat's takes whichever shape the
    /// arm asked for.
    fn roster_body(&self) -> String {
        self.roster_of(self.owned_row)
    }

    /// The same listing with the owned row in a shape this rig is not in — what
    /// an arm needs when it holds two listings at once.
    fn roster_of(&self, owned_row: Owned) -> String {
        format!(
            "[{{\"id\":\"{OWNED_ADDRESS}\",\"sessionId\":\"{OWNED_SESSION}\",\"cwd\":\"{}\",\
             \"kind\":\"background\",\"startedAt\":1000{owned}}},\
             {{\"id\":\"{UNOWNED_ADDRESS}\",\"sessionId\":\"{UNOWNED_SESSION}\",\"cwd\":\"{}\",\
             \"kind\":\"background\",\"startedAt\":1000}}]",
            self.owned.display(),
            self.unowned.display(),
            owned = owned_row.fields(),
        )
    }

    /// The listing the NEXT poll reads. The stub answers out of a file rather
    /// than a body baked into it, so a run of two polls can stage a session
    /// that was live when the claim was taken and is pid-less now — the
    /// sequence the hold exists for, and one a fixed listing cannot express.
    fn relist(&mut self, shape: Owned) {
        self.owned_row = shape;
        write(&self.roster_path(), &self.roster_body());
    }

    fn roster_path(&self) -> PathBuf {
        self.root.join("roster.json")
    }

    /// The environment with an agent BINARY behind it: what the arm driving
    /// `run::observe_with` needs, because that path resolves its own adapter.
    fn with_an_agent_binary(self) -> Rig {
        // A context reading under the rest threshold for each session. The
        // adapter reads it off disk, and its mtime is the end the stopped-row
        // window judges each row on.
        for (worktree, session) in [
            (&self.owned, OWNED_SESSION),
            (&self.unowned, UNOWNED_SESSION),
        ] {
            write(
                &transcript_path(
                    &self.root.join(".claude"),
                    dir_key(&worktree.display().to_string()),
                    session,
                ),
                A_LIGHT_TRANSCRIPT,
            );
        }
        // `daemon status` refuses, so no replacement window opens and nothing is
        // held for one.
        let stub = self.root.join("agent.sh");
        write(&self.roster_path(), &self.roster_body());
        write(
            &stub,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {argv}\ncase \"$*\" in\n  \
                 *--version*) echo 2.1.261 ;;\n  \
                 *daemon*) exit 1 ;;\n  \
                 *) cat {roster} ;;\nesac\nexit 0\n",
                argv = self.argv.display(),
                roster = self.roster_path().display(),
            ),
        );
        executable(&stub);
        self.take_the_environment(Some(&stub));
        self
    }

    /// The same environment with NO agent binary: what the arm driving
    /// `run::observe_seamed` needs, because it hands the agent in itself and a
    /// binary on the process PATH must not be reachable from there.
    fn with_the_agent_handed_in(self) -> Rig {
        self.take_the_environment(None);
        self
    }

    fn take_the_environment(&self, claude_bin: Option<&Path>) {
        common::hermetic::export(common::hermetic::in_process_vars(
            &self.root,
            &self.machine,
            claude_bin,
        ));
        platform::clear_stop();
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

    /// Every session line the run wrote, as `<type> <actor>` pairs — the actor
    /// beside the kind, because the claim is about WHICH seat got which line.
    fn session_lines(&self) -> Vec<String> {
        self.stream()
            .into_iter()
            .filter_map(|event| {
                let kind = event["type"].as_str()?.to_string();
                let actor = event["actor"].as_str().unwrap_or("-").to_string();
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

    fn projection(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine.join("projection.json"))
            .expect("a projection is published");
        serde_json::from_str(&body).expect("the projection parses")
    }

    fn decision(&self, seat: &str) -> String {
        let document = self.projection();
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

    fn attaches(&self) -> Vec<String> {
        std::fs::read_to_string(&self.argv)
            .unwrap_or_default()
            .lines()
            .filter(|call| call.starts_with("attach"))
            .map(str::to_string)
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        platform::clear_stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The clock a run of ticks is driven on: fake time, and a stop asked for once
/// [`TICKS`] whole poll intervals have been napped away.
///
/// A nap that ENDS with the flag raised returns false and the loop leaves, so
/// the run is one tick per nap that finished quietly plus the tick before the
/// nap that raised it: two intervals of fake time is exactly two ticks. Nothing
/// here waits on the wall clock, so the interval's length costs the arm
/// nothing.
///
/// THE NAP IS ALSO THE TICK BOUNDARY, which is the only place a seamed run can
/// be told something new: `at_first_nap` runs once, between the first tick and
/// the second, so an arm can hand the second tick a listing the first one did
/// not read.
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

/// A SESSION THE DAEMON STILL LISTS IS CLAIMED, NEVER ATTACHED TO — AND THE
/// CLAIM OUTLIVES THE POLL THAT TOOK IT.
///
/// A pid-less row is the one reading the roster cannot tell apart on its own: a
/// session that ended looks exactly like one the daemon is still holding. The
/// session table is what separates them — it names the sessions this fleet has
/// claimed — so a row whose session the table names and the daemon still lists
/// is left alone, and reviving it instead re-attaches to a session that is
/// already there, spending a whole wake on a seat whose first turn is one.
///
/// TWO POLLS, and the listing MOVES between them, because that is the sequence
/// the claim exists to carry: the first poll sights a live session and claims
/// it, the second reads the same session pid-less and holds it on the claim
/// alone. The second poll starts from nothing but the session table on disk,
/// exactly as a restarted controller does.
///
/// TWO SEATS ON ONE LISTING. `UNOWNED` stands on a pid-less row the whole way
/// with nothing in the table naming its session; it is the control and it IS
/// revived — which is what makes the other seat's leave-alone a reading of the
/// claim rather than a fixture that never reached the revive arm at all.
#[test]
fn a_restart_leaves_a_claimed_pid_less_row_alone_and_revives_the_one_it_does_not_own() {
    let mut rig = Rig::new("claimed-across-a-restart")
        .with_the_owned_row(Owned::LiveIdle)
        .with_an_agent_binary();

    assert_eq!(run::observe_with(&Options { once: true }, rig.grant()), 0);
    let after_one = rig.session_lines();
    assert!(
        after_one.contains(&format!("session.adopted {OWNED_ID}")),
        "the first poll claimed the live session the table names: {after_one:?}"
    );
    assert_eq!(rig.decision(OWNED_ID), "leave-alone");
    assert_eq!(
        rig.decision(UNOWNED_ID),
        "revive",
        "the control took the arm the claimed seat was spared: {after_one:?}"
    );

    // The session hibernates: pid gone, and the state word unchanged from the
    // one it carried while it was live.
    rig.relist(Owned::PidlessDone);

    // The second poll, from a loop that starts again knowing only what the
    // session table on disk carries — which is where the claim now lives.
    assert_eq!(run::observe_with(&Options { once: true }, rig.grant()), 0);
    assert_eq!(rig.decision(OWNED_ID), "leave-alone");

    let lines = rig.session_lines();
    assert_eq!(
        rig.lines_reading(&format!("session.revived {OWNED_ID}")),
        0,
        "neither poll attached to the claimed session: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.spawned {OWNED_ID}")),
        0,
        "nor started a second session beside it: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        1,
        "and the claim was taken once, not once per poll: {lines:?}"
    );

    // The attaches the control's revive issued are the ONLY ones: the claimed
    // seat's address never reached the agent.
    assert_eq!(
        rig.attaches(),
        vec![format!("attach {UNOWNED_ADDRESS}")],
        "one attach, and it is the control's"
    );
}

/// THE RECORDED SHAPE: ONE CONTROLLER, POLLING ON.
///
/// Adoption runs once per process, so every tick after the first has no claim of
/// its own to read — and a verdict that learned the claim only from the tick
/// that took it would attach to the same session on every tick after, which is
/// the hourly revive this bug was filed on. Two ticks of ONE loop: the first
/// sights the session live and claims it, the second reads it hibernated, and
/// the claimed seat is left alone on both.
///
/// The control revives on the FIRST tick only, and that is the arrival window
/// rather than anything about ownership — its revive opened a row whose sighting
/// the next tick is still waiting on. What the control is here for is the same
/// as above: a fixture observed reaching the revive arm.
#[test]
fn one_controller_polling_twice_revives_a_claimed_pid_less_row_on_neither_tick() {
    let rig = Rig::new("claimed-across-two-ticks")
        .with_the_owned_row(Owned::LiveIdle)
        .with_the_agent_handed_in();
    let stub = StubAgent::answering(Answers {
        status: parse_roster(&rig.roster_body()),
        daemon: DaemonRead::Readable(None),
        transcript: Some(A_LIGHT_TRANSCRIPT.to_string()),
        ended_at: Some(now_ms()),
        ..Answers::default()
    });
    // The session hibernates between the ticks, which is where the claim taken
    // on the first one has to still be in hand.
    let hibernated = parse_roster(&rig.roster_of(Owned::PidlessDone));
    let clock = NapThenStop::new(Duration::from_secs(POLL_SECONDS * TICKS))
        .then(|| stub.set(|answers| answers.status = hibernated.clone()));
    let watchdog = Watchdog::armed();

    let status = run::observe_seamed(
        &Options { once: false },
        rig.grant(),
        None,
        Seams {
            clock: &clock,
            agent: &stub,
            child_path: "",
            effects_off: None,
            stop_handler: StopHandler::Unarmed,
        },
    );
    drop(watchdog);
    assert_eq!(status, 0, "the loop ended on the stop it was asked for");

    // TWO TICKS RAN, read from the agent rather than assumed: both seats are
    // decided against the fleet's own listing, so a tick is one roster read.
    assert_eq!(
        stub.calls_of(StubAgent::STATUS).len() as u64,
        TICKS,
        "the loop polled twice: {:?}",
        stub.verbs()
    );

    let lines = rig.session_lines();
    assert_eq!(
        rig.lines_reading(&format!("session.revived {OWNED_ID}")),
        0,
        "no tick attached to the claimed session: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        1,
        "the claim was taken on the first tick and not retaken: {lines:?}"
    );
    assert_eq!(rig.decision(OWNED_ID), "leave-alone");

    // The control, and with it the only attach the whole run issued.
    assert_eq!(
        rig.lines_reading(&format!("session.revived {UNOWNED_ID}")),
        1,
        "the seat no claim covers took the revive: {lines:?}"
    );
    let attached: Vec<String> = stub
        .calls_of(StubAgent::REVIVE)
        .into_iter()
        .map(|call| call.about)
        .collect();
    assert_eq!(attached, vec![UNOWNED_ADDRESS.to_string()]);
}

/// A LIVE IDLE SESSION IS CLAIMED AT ITS FIRST SIGHTING, AND THE WORD ON ITS
/// ROW HAS NO SAY IN IT.
///
/// The row a live idle session stands on reads `state: done` with its pid and
/// `status: idle` beside it, so a claim that consulted the state word would
/// pass over every seat the fleet keeps idle — and then have nothing in hand
/// when that seat hibernates, which is the revive this bug was filed on. The
/// claim is taken on the SIGHTING: a pid is there, so the session is running,
/// so it is this fleet's.
///
/// The control is the same as the arms above: `UNOWNED`, pid-less and unclaimed,
/// reaching the revive the claimed seat is spared.
#[test]
fn a_live_idle_session_is_claimed_at_its_first_sighting() {
    let rig = Rig::new("live-idle-adopted")
        .with_the_owned_row(Owned::LiveIdle)
        .with_an_agent_binary();

    assert_eq!(run::observe_with(&Options { once: true }, rig.grant()), 0);

    let lines = rig.session_lines();
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        1,
        "the live row was claimed, `done` on it and all: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.revived {OWNED_ID}")),
        0,
        "and nothing was attached to a session that is already up: {lines:?}"
    );
    assert_eq!(
        rig.decision(UNOWNED_ID),
        "revive",
        "the control reached the arm the sighting spared the other seat: {lines:?}"
    );
}

/// AND THE OTHER HALF OF THE TERM: THE CLAIM HOLDS THROUGH A HIBERNATION.
///
/// A session this fleet claimed while it was live goes pid-less, and its row
/// still reads the `done` it read while it was live. Nothing on the roster
/// separates that from a session stopped from idle or killed from outside
/// (lessons claude-code A3), so the claim is what answers: the daemon still
/// lists the row, the fleet still owns it, and attaching would spend a whole
/// wake to reach a session that is already there.
///
/// The fixture is the sequence that produces it and no shorter one: a table row
/// an earlier poll already claimed, and a listing that now carries the row
/// pid-less. What releases the hold instead is the arm below.
#[test]
fn a_claimed_session_that_has_gone_pid_less_is_left_alone() {
    let rig = Rig::new("claimed-then-hibernated")
        .with_the_claim_taken()
        .with_the_owned_row(Owned::PidlessDone)
        .with_an_agent_binary();

    assert_eq!(run::observe_with(&Options { once: true }, rig.grant()), 0);

    let lines = rig.session_lines();
    assert_eq!(
        rig.decision(OWNED_ID),
        "leave-alone",
        "the claim holds a session the daemon still lists: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.revived {OWNED_ID}")),
        0,
        "and no attach was issued: {lines:?}"
    );
    assert!(
        !rig.attaches().contains(&format!("attach {OWNED_ADDRESS}")),
        "the claimed seat's address never reached the agent: {:?}",
        rig.attaches()
    );
    assert_eq!(
        rig.decision(UNOWNED_ID),
        "revive",
        "the control reached the revive arm: {lines:?}"
    );
}

/// AN END THE ROSTER CAN NAME RELEASES THE CLAIM.
///
/// `stopped` and `failed` are the two words the listing reaches only from a
/// non-idle prior state, so they are the ends it can name on its own — and a
/// claim is not a lease: a row reading one of them is not a session anyone owns,
/// whatever the table still says, and the seat takes the revive arm.
#[test]
fn a_claimed_session_whose_row_names_an_end_is_revived() {
    let rig = Rig::new("claimed-then-stopped")
        .with_the_claim_taken()
        .with_the_owned_row(Owned::Stopped)
        .with_an_agent_binary();

    assert_eq!(run::observe_with(&Options { once: true }, rig.grant()), 0);

    let lines = rig.session_lines();
    assert_eq!(
        rig.decision(OWNED_ID),
        "revive",
        "the claim does not hold a seat whose row names an end: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.revived {OWNED_ID}")),
        1,
        "and the attach was issued: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        0,
        "a pid-less row is not claimed again either: {lines:?}"
    );
    assert!(
        rig.attaches().contains(&format!("attach {OWNED_ADDRESS}")),
        "the address the listing carried reached the agent: {:?}",
        rig.attaches()
    );
}

/// AND THE ARM THE HOLD MUST NOT SWALLOW: a pid-less `done` row NO claim covers
/// is revived, exactly as before.
///
/// The hold is the claim plus the listing, and a fixture that only ever watched
/// the claimed seat could not tell a hold that reads the claim from one that
/// reads the state word and holds every idle-looking row in the fleet.
#[test]
fn an_unclaimed_pid_less_done_row_is_revived() {
    let rig = Rig::new("unclaimed-then-hibernated")
        .with_the_owned_row(Owned::PidlessDone)
        .with_an_agent_binary();

    assert_eq!(run::observe_with(&Options { once: true }, rig.grant()), 0);

    let lines = rig.session_lines();
    assert_eq!(
        rig.decision(OWNED_ID),
        "revive",
        "nothing claims this session, so the discriminator takes it: {lines:?}"
    );
    assert_eq!(
        rig.lines_reading(&format!("session.adopted {OWNED_ID}")),
        0,
        "and a pid-less row is not claimed on the way past: {lines:?}"
    );
    assert!(
        rig.attaches().contains(&format!("attach {OWNED_ADDRESS}")),
        "the attach was addressed at the row's own id: {:?}",
        rig.attaches()
    );
}

/// A seat's own shell carries the agent's config directory, set by the
/// controller on every child it spawns, and the adapter reads it before the
/// home beside it — so a rig that leaves it standing reads its transcripts out
/// of the operator's real one. The bare witness: the two adoption arms above
/// answer the same only when the rig has shadowed it, and this arm reds bare
/// when the shadow is lost, where those two red only under the variable.
#[test]
fn the_rig_shadows_the_config_directory_a_seats_shell_carries() {
    let decoy = std::env::temp_dir().join("adoption-decoy-config-dir");
    std::env::set_var(common::hermetic::CONFIG_DIR, &decoy);
    let rig = Rig::new("shadows-the-config-dir").with_the_agent_handed_in();
    assert_eq!(
        std::env::var_os(common::hermetic::CONFIG_DIR),
        Some(rig.root.join(".claude").into_os_string()),
        "the rig's environment block leaves the agent's config directory where the shell had it"
    );
}
