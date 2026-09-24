// The rig the `drive_*` binaries share: `fleet observe` end to end, against a
// stub agent. Each of those files holds the arms of one subject and includes
// this one at file scope; nothing here is an arm.
//
// Every arm they hold is one the acceptance drive runs against the real fleet;
// a stub binary is what lets the same shapes be pinned offline, on either
// platform, in milliseconds.
//
// The PROSE below is pinned by no target. `make fleet-test` and `make
// lessons-check` both stay green with a backhistory sentence put back into any
// docstring in this family — neither reads a comment for its tense. The guard
// is `tools/comment-sweep`, which no make target calls and which the reviewer
// runs over the diff by hand, so a docstring here is corrected on a reading and
// never on a red.
//
// The admission is the CRATE's and not this file's: the same two targets read
// no comment in `src/` either, so a sentence in the adapter's own docstrings is
// as unpinned as one here. And tense is only half of it — a CENSUS is prose
// too. A docstring that says how many arms reach a branch, or that a branch is
// reached by none, is recomputed by no target and goes stale the moment an arm
// moves; the method for retaking one is in `mod unreadable_causes`, in
// `drive_causes.rs`, and taking it again is a reader's act.
//
// ELAPSED ASSERTIONS, THE ONE RULE. What makes a reading of a poll's own time a
// reading and not a clock check is WHERE ITS MARGIN COMES FROM, and there are
// two kinds here.
//
// A bound whose margin is A HANG THIS ARM SET is a reading. The arm gives the
// stub seconds of work and a seam of a few hundred milliseconds, so the two
// outcomes it is separating — the deadline fired, or the call waited the stub
// out — sit seconds apart, and every figure the box moves is small against that
// gap. Those bounds are absolute because the hang they are measured against is,
// and the arms carrying one say which hang beside it.
//
// A bound whose margin is THE BOX'S OWN SPEED is not a reading. "A healthy call
// finishes inside X" has no gap in it: X is a guess about this machine under
// this suite's parallelism, and it reds on a loaded box while the code is
// right. An arm that needs that comparison takes it RELATIVELY instead —
// against a poll of its own, in the same run, that sits through the whole hang,
// so load moves both figures together.
//
// The two are not a contradiction and neither retires the other. The relative
// form costs a poll per reading; the absolute form costs nothing and is
// available exactly when a hang is what the arm is measuring against.

use fleet_controller::adapter::claude_code::ClaudeCode;
use fleet_controller::adapter::{encode_project_dir, Agent, RosterRead};
use fleet_controller::platform::child_path;
use fleet_controller::run;
use fleet_controller::test_support::FakeClock;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// The FLOOR in attempts: how many tries a call gets whatever ONE try costs.
///
/// A budget in time alone cannot express this, and the fail logs are why. A
/// patient rig runs on a deadline of tens of seconds, so one discarded attempt
/// on it costs more than a ten-second budget holds — the refusals it wrote read
/// "the escapee never left the group before the call returned, 1 attempts over
/// 20.9s to 32.4s of patience" and "1 attempts over 39.442598125s". The wrapper
/// had refused BEFORE IT RETRIED ONCE, on exactly the arms a retry exists for.
/// Three, because a floor buys the rare expensive case its retry and is not
/// where the ordinary case is served.
const STUB_START_ATTEMPTS: usize = 3;

/// The CEILING in attempt time, which is what serves the ordinary case above
/// the floor.
///
/// A count alone cannot express this either, and it is the other half of the
/// same measurement. The short-seam arms of `drive_causes` run on 200 ms
/// deadlines, and under the suite's own parallelism their stub misses that seam
/// again and again: over four quiet runs of the two binaries, 56 calls
/// discarded an attempt, 24 of them needed a fourth try and the deepest needed
/// a seventh. At a floor of three alone, four of those arms refuse on a QUIET
/// box. Ten seconds is about forty tries at that seam's cost and about none at
/// a patient rig's, which is exactly why both figures are here and neither
/// alone is the patience.
const STUB_START_PATIENCE: Duration = Duration::from_secs(10);

/// The time between attempts, spent waiting for the box to be calm, and never
/// charged a discarded attempt's own cost.
///
/// Ten, the same figure as the ceiling above and a different quantity: how long
/// this wrapper will sit out a storm before saying the box is the finding. What
/// is NOT charged here is the attempt itself, which is the whole of why the
/// floor sits above.
const CALM_BUDGET: Duration = Duration::from_secs(10);

/// How fast a shell that does nothing has to run for the box to count as ready
/// to be asked again. The median is 8 ms and p90 is 12 ms on a quiet box, so
/// this is several times the ordinary cost and well under the tail that
/// discards an attempt.
const SPAWN_IS_CLEAR_WITHIN: Duration = Duration::from_millis(50);

/// How many CONSECUTIVE prompt probes read as calm.
///
/// One sample is luck and not a reading: the fail logs show four attempts fired
/// inside 12.3 to 12.5 s, each one on the strength of a single prompt probe,
/// into a box that was discarding every one of them. Three in a row at the
/// spacing below spans the tail that a single probe falls through, and one slow
/// probe resets the count.
const CALM_PROBES: usize = 3;

/// What a wait for a clear spawn came back with, so the wait can report a spent
/// patience instead of refusing on it.
///
/// Both variants carry what the wait spent, which its caller charges to the same
/// budget. `PatienceGone` carries its own clause as well, worded where the
/// patience it names is known, because the one place that refuses is
/// `Rig::witnessed` and the refusal it writes has to carry this voice beside the
/// discarded attempt's.
enum SpawnWait {
    Cleared(Duration),
    PatienceGone(Duration, String),
}

/// The five per-call seams, named once so the stub that reads a file and the
/// rig that writes it cannot spell one differently.
/// The rig's one seat. Its row is keyed by the id and carries the seat's own
/// name; every directory, stream line, session name and argument the loop and
/// the verbs write about it is the machine name those two give.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const SEAT: &str = "orla-93b9739a";

const HANG: &str = "hang";
const VERSION_HANG: &str = "version-hang-seam";
const DESCENDANT: &str = "descendant";
const VERSION_DESCENDANT: &str = "version-descendant";
const ESCAPEE: &str = "escapee";

/// The sixth, and the only ONE-SHOT one: a delay the stub takes between its
/// start mark and everything after it, consumed by the first invocation that
/// reads it. The five above describe every call a rig makes; this one describes
/// exactly one, which is what lets an arm force a kill into the gap between two
/// marks on the attempt that is then discarded and not on the retry.
const PREAMBLE: &str = "preamble";

/// The four effect branches' exit statuses, one seam each. A failed stop and a
/// failed start are then one file apart, which is what lets an arm drive the
/// half of a collection that fails without a second stub.
const START_EXIT: &str = "start-exit";
const STOP_EXIT: &str = "stop-exit";
const RM_EXIT: &str = "rm-exit";
const NUDGE_EXIT: &str = "nudge-exit";
const ATTACH_EXIT: &str = "attach-exit";
const DAEMON_EXIT: &str = "daemon-exit";

/// The environment and the standard streams are the PROCESS's, so one
/// in-process poll runs at a time. Under `cargo test` the arms of this binary
/// share a process across threads, and two of them redirecting fd 2 together
/// would read each other's lines while two setting `FLEET_DIR` together would
/// poll each other's machine directory.
///
/// HELD FOR THE CALL AND NOT FOR THE RIG'S LIFE. Eight arms hold two rigs at
/// once, and a lock taken in `Rig::new` would deadlock every one of them. Each
/// poll sets what it needs from its own rig and puts it back before it lets go,
/// so two live rigs are two sets of values and never one.
///
/// A poisoned lock is taken anyway: the panic that poisoned it already failed
/// its own arm, and refusing it here would fail every other arm for it.
static IN_PROCESS: Mutex<()> = Mutex::new(());

extern "C" {
    fn dup(oldfd: i32) -> i32;
    fn dup2(oldfd: i32, newfd: i32) -> i32;
    fn close(fd: i32) -> i32;
    /// Variadic as C declares it: on this target a fixed-arity declaration
    /// would pass the third argument in a register the callee reads off the
    /// stack.
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// `fcntl`'s set-descriptor-flags command and the close-on-exec flag it sets.
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;

/// What the probe below writes through `eprint!` and then looks for on the
/// descriptor. Distinctive, so a byte arriving from anywhere else cannot be read
/// as the probe's — and worded for the reader it lands in front of, because a
/// runner that captures it is a runner that prints it in the failing arm's own
/// output beside the refusal.
const CAPTURE_PROBE: &str =
    "fleet-rig-stderr-probe: this line is the rig probing where the print macros go";

/// Whether the print macros reach fd 2 at all under the runner in front of this
/// process — answered by writing through `eprint!` onto a redirected descriptor
/// and reading that descriptor back.
///
/// libtest captures the print macros into a per-test buffer unless it is told
/// not to, and the interception sits ABOVE the descriptor: `dup2` moves fd 2 and
/// the macro never reaches it, so a capture taken over a poll comes back empty
/// while the loop's own lines surface in the runner's per-test output instead.
///
/// A BEHAVIOURAL PROBE AND NOT A READING OF `NEXTEST`, because the condition is
/// the interception and not the runner. `cargo nextest` gives each arm its own
/// process and no capture; `cargo test -- --nocapture` turns the same capture
/// off and the whole family passes under it. An environment sniff would refuse
/// the second, which is a correct command.
///
/// FD 2 ANSWERS FOR FD 1. One libtest switch sets both, so a process whose
/// stderr reaches the descriptor has a stdout that does too.
///
/// ANSWERED ONCE PER PROCESS: libtest sets the capture for every test thread or
/// for none, so the reading cannot differ between arms.
fn stderr_reaches_the_descriptor() -> bool {
    static ANSWER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ANSWER.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("fleet-rig-capture-probe-{}", std::process::id()));
        let file = std::fs::File::create(&path).expect("the probe file opens");
        let saved = unsafe { dup(2) };
        assert!(saved >= 0, "fd 2 duplicates before the capture probe");
        assert!(
            unsafe { fcntl(saved, F_SETFD, FD_CLOEXEC) } >= 0,
            "the saved copy of fd 2 is close-on-exec"
        );
        assert!(
            unsafe { dup2(file.as_raw_fd(), 2) } >= 0,
            "fd 2 takes the probe file"
        );
        eprint!("{CAPTURE_PROBE}");
        let _ = std::io::stderr().flush();
        unsafe {
            dup2(saved, 2);
            close(saved);
        }
        // Read back and never defaulted: an unreadable probe file is neither
        // answer, and defaulting it either way is a wrong reading — a silent
        // false would refuse the correct runner, a silent true would hand back
        // the empty captures this refuses over.
        let landed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the probe file at {} reads back: {e}", path.display()));
        let _ = std::fs::remove_file(&path);
        landed.contains(CAPTURE_PROBE)
    })
}

/// This test binary's cargo target name, from the harness's own path:
/// `…/deps/<name>-<hash>`. The name is what `--test` and a `binary()` filterset
/// take; the hash is not.
///
/// A PLACEHOLDER AND NOT A GUESS when the path cannot be read, because the
/// refusal's whole value is that its command is the one to run: one of the seven
/// binaries named as if it were this one sends the reader to the wrong suite.
const UNNAMED_BINARY: &str = "<this test binary's --test name>";

fn current_test_binary() -> String {
    let exe = std::env::current_exe().ok();
    let stem = exe
        .as_deref()
        .and_then(Path::file_stem)
        .and_then(OsStr::to_str)
        .unwrap_or(UNNAMED_BINARY)
        .to_string();
    match stem.rsplit_once('-') {
        Some((name, hash))
            if !name.is_empty()
                && !hash.is_empty()
                && hash.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            name.to_string()
        }
        _ => stem,
    }
}

/// The refusal a redirect writes when the runner has already intercepted the
/// stream it is about to move, naming a runner that does not and the exact
/// command that runs THIS binary under it.
///
/// A refusal and not a failing assertion, because the two read differently to
/// the person in front of them: an arm that reds on an empty stream says the
/// controller stopped saying its line, which is a defect hunt on a clean tree.
fn capture_refusal() -> String {
    let binary = current_test_binary();
    format!(
        "this runner captures the print macros before they reach fd 2, so the redirect this \
         rig just took is a no-op: every arm reading the controller's own lines would assert \
         against an empty stream and red on a tree that is fine. Run this binary under \
         nextest, which gives each arm its own process and no capture:\n\n    \
         cargo nextest run -p fleet-cli -E 'binary({binary})'\n\nor keep cargo test and turn \
         the capture off:\n\n    cargo test -p fleet-cli --test {binary} -- --nocapture\n"
    )
}

/// One standard stream pointed at a file for as long as this lives.
///
/// The loop states its lines with `eprintln!`, which writes to fd 2 — and fd 2
/// is the test process's, which libtest hands back to no arm. So the descriptor
/// itself is moved onto a file for the call and put back after it, which is what
/// lets an arm read the loop's own words.
///
/// PUT BACK ON AN UNWIND TOO. A panic inside the redirect would otherwise leave
/// every later line of the suite writing into a temp file nobody reads.
///
/// ONLY WHERE THE MACROS REACH THE DESCRIPTOR. A runner that captures them above
/// fd 2 makes every redirect here a no-op, so `onto` refuses rather than hands
/// back a capture nothing can land in: `stderr_reaches_the_descriptor`.
struct Redirected {
    target: i32,
    saved: i32,
    path: PathBuf,
}

impl Redirected {
    fn to(target: i32, path: PathBuf) -> Redirected {
        let file = std::fs::File::create(&path).expect("the capture file opens");
        Redirected::onto(target, path, file)
    }

    /// The same, onto the END of a file that may already hold lines.
    ///
    /// What a tick-by-tick arm needs: one capture across a sequence of polls,
    /// with the descriptor given back between them so an assertion the arm makes
    /// between two ticks lands on the suite's own stream and not in the file.
    fn appending(target: i32, path: PathBuf) -> Redirected {
        let file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .expect("the capture file opens");
        Redirected::onto(target, path, file)
    }

    fn onto(target: i32, path: PathBuf, file: std::fs::File) -> Redirected {
        assert!(stderr_reaches_the_descriptor(), "{}", capture_refusal());
        let saved = unsafe { dup(target) };
        assert!(saved >= 0, "fd {target} duplicates before it is redirected");
        // CLOSE-ON-EXEC, and `dup` returns a descriptor without it. The saved
        // descriptor is a copy of the harness's own pipe, so a child spawned
        // inside this window would otherwise inherit that pipe and hold it for
        // its own life, which the harness reads as a leaked test.
        assert!(
            unsafe { fcntl(saved, F_SETFD, FD_CLOEXEC) } >= 0,
            "the saved copy of fd {target} is close-on-exec"
        );
        assert!(
            unsafe { dup2(file.as_raw_fd(), target) } >= 0,
            "fd {target} takes the capture file"
        );
        Redirected {
            target,
            saved,
            path,
        }
    }

    /// What landed on the stream, read after the descriptor is back.
    fn taken(self) -> Vec<u8> {
        let path = self.path.clone();
        drop(self);
        std::fs::read(&path).unwrap_or_default()
    }
}

impl Drop for Redirected {
    fn drop(&mut self) {
        // Rust buffers stdout by line and stderr not at all; flushing both costs
        // nothing and is what keeps a half-written line out of the next arm's
        // capture.
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        unsafe {
            dup2(self.saved, self.target);
            close(self.saved);
        }
    }
}

/// Environment variables moved for one in-process poll and put back after it.
///
/// The loop reads its machine directory, its home and four of its seams from
/// the process's own environment. Every value this sets is recorded as it was
/// first, so an arm leaves the process as it found it — which the arms reading
/// `std::env::var("PATH")` after a poll depend on.
struct EnvHeld {
    before: Vec<(String, Option<OsString>)>,
}

impl EnvHeld {
    fn new() -> EnvHeld {
        EnvHeld { before: Vec::new() }
    }

    fn set(&mut self, key: &str, value: Option<OsString>) {
        self.before.push((key.to_string(), std::env::var_os(key)));
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

impl Drop for EnvHeld {
    fn drop(&mut self) {
        for (key, before) in self.before.iter().rev() {
            match before {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct Rig {
    root: PathBuf,
    /// The last component of the seat's worktree. One arm gives it a dot,
    /// because a path the encoding has to touch beyond its separators is the
    /// case a separator-only reader publishes a null context for.
    leaf: String,
    /// Where the agent keeps its state. `None` means the default under `HOME`.
    scoped_config_dir: Option<PathBuf>,
    /// The deadline the BUILT BINARY runs its agent calls on. `None` leaves the
    /// binary on the default it ships with.
    agent_timeout_ms: Option<u64>,
    /// How long the stub's listing branch sleeps before answering — the shape a
    /// deadline is measured against end to end.
    hang_seconds: Option<u64>,
    /// The same, for the stub's `--version` branch. Its own seam, so an arm can
    /// hang one call of the poll and leave the other answering.
    version_hang_seconds: Option<u64>,
    /// How long a descendant the stub forks holds the inherited pipe after the
    /// stub itself has answered and exited. The shape a kill aimed at the child
    /// alone does not reach.
    descendant_seconds: Option<u64>,
    /// The same, for the stub's `--version` branch. The poll makes two calls and
    /// each has its own pipe, so a holder on one of them is a case the listing
    /// branch's seam cannot reach.
    version_descendant_seconds: Option<u64>,
    /// The same, for a descendant that calls `setsid` and so leaves the child's
    /// process group: the shape a kill aimed at the GROUP does not reach either.
    escapee_seconds: Option<u64>,
    /// A delay the stub takes on the line after its start mark, ONCE: the rig
    /// arms the seam for the first call it builds and the stub removes the file
    /// as it reads it, so exactly one stub invocation pays it.
    ///
    /// MILLISECONDS AND NOT SECONDS, unlike its five siblings, because what it
    /// buys is a kill landing INSIDE the gap between two marks a few
    /// microseconds apart — a gap the seams around it are sized in. The stub
    /// sleeps a fractional second, which the escape helper beside it already
    /// depends on.
    ///
    /// A `Cell` because the arming happens in `binary`, which every call goes
    /// through with `&self`, and the arming is a `take`: a seam `binary`
    /// rewrote would delay the retry too, and an arm whose every attempt is
    /// killed mid-setup spends the whole patience and refuses.
    one_shot_preamble_ms: std::cell::Cell<Option<u64>>,
    /// How many attempts `witnessed` has thrown away for want of a witness —
    /// the stub that never started, the escape that never left the group, or the
    /// per-arm witness below. Read by the control arm, which asserts a stub that
    /// DID start is never retried.
    discarded_attempts: std::cell::Cell<usize>,
    /// WHY each discarded attempt was thrown away, in the order they happened —
    /// the same sentence the stderr line and the refusal open with.
    ///
    /// A count says how many attempts were lost and nothing about which
    /// condition lost them, so an arm that cares which one it provoked has to
    /// assert over a reason rather than over a number. The preamble arm is the
    /// case: under load the box discards a second attempt of its own beside the
    /// one the arm injected, and a count pinned at one reds on the box while the
    /// FIRST reason still says exactly what the arm arranged.
    discard_reasons: std::cell::RefCell<Vec<String>>,
    /// A third witness, named by the arm that owes it and removed before every
    /// attempt like the other two.
    ///
    /// The two above are the rig's own and describe the call shape; this one is
    /// a file the STUB writes partway through its body, so an arm whose subject
    /// begins after that point can say where its setup ends. An attempt that
    /// comes back without it was killed inside its own setup and never reached
    /// the case the arm names, so it is discarded on the same budget rather than
    /// asserted over.
    owed_witness: std::cell::RefCell<Option<PathBuf>>,
    /// The time `witnessed` will spend WAITING FOR THE BOX TO BE CALM across
    /// all of its retries — never a discarded attempt's own cost, which is the
    /// field below. `CALM_BUDGET` everywhere but in the arms that PIN the
    /// refusal, which set it low so a pin that spends it costs a fraction of a
    /// second instead of ten of them.
    calm_budget: std::cell::Cell<Duration>,
    /// The attempt time `witnessed` will spend ABOVE the floor of
    /// `STUB_START_ATTEMPTS` before it refuses. `STUB_START_PATIENCE` everywhere
    /// but in the arm that pins the floor, which sets it to nothing so that the
    /// retry it reads can only have come from the floor.
    attempt_budget: std::cell::Cell<Duration>,
    /// The shell `wait_for_a_clear_spawn` probes the box with. A real one
    /// everywhere but in the arm that pins what happens when no probe can run,
    /// which points it at a path that is not there.
    spawn_probe: std::cell::RefCell<PathBuf>,
    /// The wall clock the last witnessed call spent, counting the attempt that
    /// RETURNED and no discarded one. Every arm asserting on a call's duration
    /// reads this instead of timing around the call: a clock around the wrapper
    /// would charge a discarded attempt's whole seam to the reading, which turns
    /// the bounded-versus-unbounded comparisons into a second flake in place of
    /// the one this bead removes.
    last_call: std::cell::Cell<Duration>,
    /// Whether the stub `stub_adapter` last wrote invokes the escape helper.
    ///
    /// DERIVED FROM THE BODY AND NEVER DECLARED, because a flag an arm sets by
    /// hand goes stale the day a body changes and the arm does not: it says the
    /// escape is owed by a stub that does not run one, and the wrapper spends a
    /// whole budget of patience discarding readings that were never coming. The
    /// other way in is the seam, `escapee_seconds`, which the built binary
    /// passes to the stub this rig wrote itself.
    stub_escapes: std::cell::Cell<bool>,
}

impl Rig {
    /// A whole machine in a temp directory: the fleet directory, a home the
    /// transcripts sit under, a policy file, and a stub agent on the path the
    /// adapter is told to use.
    fn new(name: &str) -> Rig {
        Rig::with_leaf(name, "builder-1")
    }

    fn with_leaf(name: &str, leaf: &str) -> Rig {
        let root = std::env::temp_dir().join(format!("fleet-drive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            root,
            leaf: leaf.to_string(),
            scoped_config_dir: None,
            agent_timeout_ms: None,
            hang_seconds: None,
            version_hang_seconds: None,
            descendant_seconds: None,
            version_descendant_seconds: None,
            escapee_seconds: None,
            one_shot_preamble_ms: std::cell::Cell::new(None),
            discarded_attempts: std::cell::Cell::new(0),
            discard_reasons: std::cell::RefCell::new(Vec::new()),
            owed_witness: std::cell::RefCell::new(None),
            calm_budget: std::cell::Cell::new(CALM_BUDGET),
            attempt_budget: std::cell::Cell::new(STUB_START_PATIENCE),
            spawn_probe: std::cell::RefCell::new(PathBuf::from("/bin/sh")),
            last_call: std::cell::Cell::new(Duration::ZERO),
            stub_escapes: std::cell::Cell::new(false),
        };
        std::fs::create_dir_all(rig.machine()).unwrap();
        std::fs::create_dir_all(rig.home()).unwrap();
        // The seat's worktree is a real directory: a start is issued IN it, and
        // a spawn whose working directory is not there fails before the child
        // runs a line — which is a defect of the rig, not of the controller.
        std::fs::create_dir_all(rig.worktree()).unwrap();
        rig.write_policy(POLICY_1S);
        rig.write_config(&rig.one_seat_config(rig.policy_path()));
        rig.write_roster("[]");
        rig.write_stub_agent();
        rig.set_version("9.9.9");
        rig
    }

    /// The seat list as every arm but the re-point one uses it.
    fn one_seat_config(&self, policy: PathBuf) -> String {
        format!(
            r#"{{"fleet_toml": "{}", "children": [
                 {{"id":"{SEAT_ID}","name":"Orla",
                   "worktrees":{{"demo":"{}"}}}}
               ]}}"#,
            policy.display(),
            self.worktree().display()
        )
    }

    /// The same list with the two keys the porter's own tools read off this
    /// file. The controller parses them onto the seat and publishes neither, so
    /// an arm that wants `run.rs` to be what drops them has to feed them in
    /// here — nothing downstream of the parse can put them back.
    fn one_seat_config_carrying_model_and_transient(&self, policy: PathBuf) -> String {
        format!(
            r#"{{"fleet_toml": "{}", "children": [
                 {{"id":"{SEAT_ID}","name":"Orla",
                   "model":"a-model","transient":true,
                   "worktrees":{{"demo":"{}"}}}}
               ]}}"#,
            policy.display(),
            self.worktree().display()
        )
    }

    /// The stub's own first act, written before it does anything else.
    ///
    /// A call whose stub never wrote this took no reading: the adapter's group
    /// kill landed on a process that had not run a line, so what came back
    /// describes the box and not the code under test. Distinct from
    /// `the-descendant-started`, which is the FORK's witness and answers a
    /// different question — that marker stays what it is.
    fn stub_started_path(&self) -> PathBuf {
        self.root.join("the-stub-started")
    }

    /// Where a stub body writes its OWN pid, for an arm whose claim is about
    /// the child one call spawned.
    ///
    /// Every arm in this binary shares one parent, so a reading taken as a
    /// count over `std::process::id()`'s children is satisfied — and broken —
    /// by any sibling's kill-then-wait window. One pid in one file, parsed the
    /// way `escape_witness` parses the escapee's, is a reading no sibling can
    /// reach. The rig's root is remade per arm, so a pid read here is the one
    /// this arm's own call wrote.
    fn stub_pid_path(&self) -> PathBuf {
        self.root.join("the-stub-pid")
    }

    /// Run a call that spawns the stub, and re-run it while its stub never
    /// started.
    ///
    /// THE RETRY CONDITION IS A WITNESS THAT IS MISSING AND NOTHING ELSE. A stub
    /// that ran and then misbehaved — exited non-zero, answered wrongly, hung
    /// past its seam — wrote the marker on its first line, so its reading is
    /// returned on the first attempt and this wrapper cannot mask it. That is
    /// what `a_stub_that_ran_and_misbehaved_is_not_retried` pins, and the counter
    /// it reads is `discarded_attempts`.
    ///
    /// THERE ARE THREE WITNESSES, and only the first is owed by every call. The
    /// stub's start marker says the child ran a line; the escape's own file,
    /// owed by a call that runs the escape helper, says the descendant reached
    /// `setsid` before the group kill landed; `owed_witness`, named by the arm
    /// itself, says the stub reached the point its subject begins at. An attempt
    /// missing any of them took no reading — the case the arm names never
    /// existed on it — and is discarded on one budget. All three are removed
    /// before every attempt: a witness left by the attempt before would
    /// otherwise be read as this one's.
    ///
    /// THE THIRD IS THE ARM'S OWN AND THE OTHER TWO ARE THIS RIG'S. An arm whose
    /// stub does work before the state the arm is about — a spawn chain the box
    /// can stretch past the seam — asserts on a case that only sometimes
    /// happened, and reds on the box rather than on the code when it did not.
    /// Naming that midpoint as a witness moves it from an assertion to a
    /// precondition, which is what the other two already are.
    ///
    /// THE SECOND WITNESS IS A PID AND NOT AN EXISTENCE. The helper creates the
    /// file and writes into it as two steps, so a reader between them finds it
    /// empty; an empty read is the same non-answer as an absent file, and
    /// `escapee_pid` is what would have paid for the difference.
    ///
    /// WHAT A DISCARDED ESCAPE COSTS, stated because it is larger than the
    /// stub-start case: the attempt spends its whole seam plus the drain grace
    /// before it can be judged, and an escapee that reached `setsid` just too
    /// late lives out its configured life holding a pipe of a call nobody is
    /// reading any more. Neither reaches the arm's own figures, which come from
    /// `last_call`; both are load on the box while the next attempt runs.
    ///
    /// THE PATIENCE IS A FLOOR IN ATTEMPTS AND A CEILING IN TIME, AND THE CALM
    /// IS NEITHER. A call keeps its retry while it is under `STUB_START_ATTEMPTS`
    /// tries OR under `attempt_budget` of attempt time, so the expensive attempt
    /// is served by the floor and the cheap one by the ceiling; the time it
    /// spends WAITING between them is `calm_budget` and is a third quantity.
    /// Both halves are measured. A patient rig's one discarded attempt cost 21
    /// to 39 seconds against a ten-second budget — "1 attempts over
    /// 39.442598125s of patience", a refusal written before a single retry — and
    /// a 200 ms-seam arm of `drive_causes` needed a fourth try in 24 calls of
    /// four quiet runs and a seventh in the deepest, which a floor of three
    /// refuses on a quiet box.
    ///
    /// A DISCARDED ATTEMPT'S OWN COST IS NEVER CHARGED TO THE CALM BUDGET, which
    /// is what made the old single budget spend itself before it had waited for
    /// anything.
    ///
    /// AND THE TIME BETWEEN ATTEMPTS IS SPENT WAITING FOR A SPAWN TO CLEAR, not
    /// idling. A re-attempt fired straight back into the storm that discarded
    /// the last one is another sample of the same bad moment; before each one
    /// this waits for a trivial shell to start promptly, and charges that wait
    /// to the calm budget so the wait cannot spend itself twice.
    ///
    /// THE FIRST ATTEMPT FIRES WITHOUT A CALM WAIT, as it always has: a call on
    /// a box that is fine pays nothing for this wrapper at all.
    ///
    /// THIS IS THE ONE PLACE A SPENT BUDGET IS REFUSED. The wait reports what it
    /// spent and whether a probe cleared, and never refuses on its own, so the
    /// refusal written here opens with the discarded attempt's own sentence —
    /// which condition failed — and states both figures after it: the attempts
    /// discarded and the calm waited. It carries the wait's clause as well when
    /// that is where the budget went. The reader gets one message whichever half
    /// ran out, and a pin on either voice holds on both paths.
    ///
    /// The measured reason any of it exists: a spawned `/bin/sh` reaches its
    /// first line in 8 ms at the median and 12 ms at p90, but one spawn in
    /// ninety under a loaded box took 3963 ms — past the deadlines these arms
    /// run on, so the kill lands on a stub that has done nothing and every arm
    /// asserting what the stub did reds together.
    ///
    /// A discarded attempt says so on stderr, which libtest prints only when
    /// the arm fails, so a green run says nothing about what it discarded.
    fn witnessed<T>(&self, call: impl Fn() -> T) -> T {
        // What a discarded attempt would otherwise leave behind. `observe`
        // publishes a projection and APPENDS events, so an attempt that took no
        // reading would still double an event count and overwrite a seeded
        // projection; both are put back byte-for-byte before the re-run, and a
        // file that did not exist is removed again rather than left. Those two
        // are the whole set: `run.rs` writes the projection through
        // `platform::write_atomic` and `events.rs` appends the log, and nothing
        // else under `src/` writes into the machine directory.
        let carried = [self.machine().join("projection.json"), self.events_path()]
            .map(|p| (p.clone(), std::fs::read(&p).ok()));
        let budget = self.calm_budget.get();
        let attempt_budget = self.attempt_budget.get();
        let mut calm = Duration::ZERO;
        let mut attempted = Duration::ZERO;
        let mut discarded = 0_usize;

        let escape_owed = self.escape_is_owed();
        let owed = self.owed_witness.borrow().clone();

        loop {
            let _ = std::fs::remove_file(self.stub_started_path());
            if escape_owed {
                let _ = std::fs::remove_file(self.escaped_path());
            }
            if let Some(path) = &owed {
                let _ = std::fs::remove_file(path);
            }
            let started = Instant::now();
            let out = call();
            let attempt = started.elapsed();
            let missing = if !self.stub_started_path().exists() {
                "the stub never started".to_string()
            } else if escape_owed && self.escape_witness().is_none() {
                "the escapee never left the group".to_string()
            } else if let Some(path) = owed.as_ref().filter(|p| !p.exists()) {
                format!(
                    "the stub never reached the witness this arm owes, {}",
                    path.display()
                )
            } else {
                self.last_call.set(attempt);
                return out;
            };

            attempted += attempt;
            discarded += 1;
            self.discarded_attempts
                .set(self.discarded_attempts.get() + 1);
            self.discard_reasons.borrow_mut().push(missing.clone());
            let refusal = |calm: Duration| {
                format!(
                    "{missing} before the call returned, {discarded} attempts discarded \
                     over {attempted:?} and {calm:?} of calm waited: the box is not \
                     getting this arm's stub through the setup it needs inside the seam \
                     the arm runs on. That is the class, and a longer budget is not the \
                     answer to it"
                )
            };
            // THE FLOOR OR THE CEILING, and a call keeps its retry while EITHER
            // still holds: the floor is what an expensive attempt has, and the
            // ceiling is what a cheap one has. Refusing on the floor alone reds
            // the 200 ms-seam arms on a quiet box; refusing on the ceiling alone
            // gives a patient rig no retry at all.
            assert!(
                discarded < STUB_START_ATTEMPTS || attempted < attempt_budget,
                "{}",
                refusal(calm)
            );
            eprintln!(
                "{missing} before the call returned in {attempt:?}; {discarded} attempts \
                 discarded over {attempted:?}, {} ms of the calm budget spent; waiting \
                 for a spawn to clear, then retrying",
                calm.as_millis()
            );
            // SATURATING, because the calm spent can exceed the budget: the
            // wait's loop tests its patience and then runs a probe, so the
            // reading it hands back is the test plus that probe. A bare
            // subtraction panics there — "overflow when subtracting durations" —
            // and a zero patience is the right answer anyway: the wait refuses
            // on it at once, in its own voice, which is where a spent calm
            // budget belongs.
            match self.wait_for_a_clear_spawn(budget.saturating_sub(calm)) {
                SpawnWait::Cleared(waited) => calm += waited,
                SpawnWait::PatienceGone(waited, clause) => {
                    calm += waited;
                    panic!(
                        "{}. The rest of it went into the wait: {clause}",
                        refusal(calm)
                    );
                }
            }
            for (path, before) in &carried {
                match before {
                    Some(body) => std::fs::write(path, body).expect("state put back"),
                    None => {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
    }

    /// Wait, within `patience`, for the box to start a process promptly again.
    ///
    /// The probe is a shell that does nothing, and the reading is how long it
    /// takes to run at all: under the contention that discards attempts, a
    /// no-op spawn is slow too, so a probe that comes back promptly is the
    /// signal that a re-attempt has a chance. Either way it REPORTS and never
    /// refuses — the refusal for a spent budget is `witnessed`'s alone, so the
    /// reader gets one message whichever half of the patience ran out — and
    /// what it spent is charged to the same budget, because patience is not
    /// spendable twice.
    ///
    /// A probe that cannot be RUN is never clear, whatever it costs: an absent
    /// binary fails in microseconds, and treating fast-and-failed as ready would
    /// make this wait a no-op exactly when it is being tested.
    ///
    /// CALM IS `CALM_PROBES` PROMPT PROBES IN A ROW, and one slow probe resets
    /// the count. A single prompt sample is a reading of one moment and not of
    /// the box: the fail logs show four attempts fired inside 12.3 to 12.5 s,
    /// each one released by a single prompt probe, into a storm that discarded
    /// every one of them.
    fn wait_for_a_clear_spawn(&self, patience: Duration) -> SpawnWait {
        let started = Instant::now();
        let mut in_a_row = 0_usize;
        while started.elapsed() < patience {
            let probe = Instant::now();
            let ran = Command::new(self.spawn_probe.borrow().as_path())
                .args(["-c", ":"])
                .status();
            if matches!(ran, Ok(status) if status.success())
                && probe.elapsed() < SPAWN_IS_CLEAR_WITHIN
            {
                in_a_row += 1;
                if in_a_row >= CALM_PROBES {
                    return SpawnWait::Cleared(started.elapsed());
                }
            } else {
                in_a_row = 0;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        SpawnWait::PatienceGone(
            started.elapsed(),
            format!(
                "no spawn cleared inside {patience:?} of patience: a shell that does nothing did \
                 not start promptly {CALM_PROBES} times in a row on this box, so a re-attempt \
                 would only be another sample of the same storm. That is the class, and a longer \
                 budget is not the answer to it"
            ),
        )
    }

    /// What the last witnessed call spent, discarded attempts excluded.
    fn last_call(&self) -> Duration {
        self.last_call.get()
    }

    /// Whether a call through this rig runs the escape helper, and so owes the
    /// escape witness before its reading is one.
    ///
    /// Two ways in and both are read from what the call will actually do: the
    /// seam the built binary hands the stub, and the body of the stub
    /// `stub_adapter` last wrote. An arm that drives neither owes nothing, and
    /// the wrapper never waits on a witness no call was going to produce.
    fn escape_is_owed(&self) -> bool {
        self.escapee_seconds.is_some() || self.stub_escapes.get()
    }

    /// Name the file this rig's next calls owe, in the stub's own body order:
    /// a path the stub writes AFTER whatever setup the arm's subject begins
    /// past. Set once per arm, before the call that owes it.
    fn owes_witness(&self, path: &Path) {
        *self.owed_witness.borrow_mut() = Some(path.to_path_buf());
    }

    /// Wait for a witness file to appear, and answer how long it took.
    ///
    /// THE BOUND IS AN ANTI-HANG CEILING AND NEVER A READING. Nothing is
    /// asserted about how long the fork took — the return is the arm's
    /// precondition, not its subject — so this is a lower-bound wait in the
    /// sense the file header means: it ends as soon as the witness is there and
    /// the ceiling exists only so a fork that never happens fails the suite
    /// instead of hanging it. A bound small enough to be reached under load is
    /// therefore a red on the box and not on the code, which is exactly what
    /// the callers' figures are chosen against.
    ///
    /// THE PANIC CARRIES THE BOUND AND THE ELAPSED, because a bare absence
    /// cannot be told from a fork that was one poll late: a reader has to see
    /// "7.2 s against a 6 s bound" to know whether to raise the ceiling or to
    /// go looking for the fork.
    ///
    /// The elapsed goes to stderr on every wait, which libtest prints on a red
    /// and `--success-output immediate` prints on a green — so the ceiling is
    /// re-measurable from a run of the suite rather than from this comment.
    fn witness_within(&self, path: &Path, bound: Duration, what: &str) -> Duration {
        let started = Instant::now();
        while !path.exists() && started.elapsed() < bound {
            std::thread::sleep(Duration::from_millis(20));
        }
        let waited = started.elapsed();
        assert!(
            path.exists(),
            "{what}: waited {waited:?} against a {bound:?} bound and {} never appeared",
            path.display()
        );
        eprintln!(
            "witness: {} appeared after {waited:?} against a {bound:?} bound",
            path.display()
        );
        waited
    }

    /// The pid the escape witness carries, or `None` while the file is absent or
    /// still empty — the two shapes of "no escape has been witnessed yet".
    fn escape_witness(&self) -> Option<u32> {
        std::fs::read_to_string(self.escaped_path())
            .ok()
            .and_then(|body| body.trim().parse::<u32>().ok())
    }

    /// One listing read through the witnessed path — the adapter half of
    /// `witnessed`, as `observe` is the built-binary half. Every arm that reads
    /// a stub adapter goes through here, so the retry lives in one place for
    /// both call shapes.
    fn read_with(&self, agent: &ClaudeCode) -> RosterRead {
        self.witnessed(|| agent.status(None))
    }

    fn events_path(&self) -> PathBuf {
        self.machine().join("events.jsonl")
    }

    /// The routines directory under this rig's fleet root, which is the directory
    /// holding the policy file the seat list names.
    fn routines_dir(&self) -> PathBuf {
        self.root.join("orders")
    }

    fn write_routine(&self, name: &str, body: &str) {
        write(&self.routines_dir().join(format!("{name}.toml")), body);
    }

    fn routines_state_path(&self) -> PathBuf {
        self.machine().join("orders").join("state.json")
    }

    /// One routine's row of the routines state, or `None` while the file or the row
    /// is not there.
    fn routine_state(&self, name: &str) -> Option<serde_json::Value> {
        let body = std::fs::read_to_string(self.routines_state_path()).ok()?;
        let document: serde_json::Value = serde_json::from_str(&body).ok()?;
        document.get("orders")?.get(name).cloned()
    }

    /// The instant every routine clock reads, as a file the binary re-reads on
    /// every tick — so an arm can step a controller that is already running.
    /// While the file is not there, the routines run on this machine's own clock.
    fn clock_path(&self) -> PathBuf {
        self.root.join("clock")
    }

    fn set_clock(&self, secs: u64) {
        write(
            &self.clock_path(),
            &fleet_controller::clock::stamp_secs(secs),
        );
    }

    /// Every routine event on the stream, in file order.
    fn routine_events(&self) -> Vec<serde_json::Value> {
        self.events()
            .into_iter()
            .filter(|event| {
                event["type"]
                    .as_str()
                    .is_some_and(|kind| kind.starts_with("routine."))
            })
            .collect()
    }

    /// The projection's row for one routine.
    fn routine_row(&self, name: &str) -> Option<serde_json::Value> {
        self.try_projection()?
            .get("orders")?
            .as_array()?
            .iter()
            .find(|row| row["name"] == name)
            .cloned()
    }

    fn machine(&self) -> PathBuf {
        self.root.join("machine")
    }
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    fn worktree(&self) -> PathBuf {
        self.root.join("wt").join(&self.leaf)
    }
    fn policy_path(&self) -> PathBuf {
        self.root.join("fleet.toml")
    }
    fn second_policy_path(&self) -> PathBuf {
        self.root.join("elsewhere.toml")
    }
    /// Every configuration directory a listing was asked for, one per line and
    /// in call order. It is what an arm reads to say WHICH directory a read was
    /// made under, which is the whole of the per-row isolation — every spawned
    /// seat under a configuration directory of its own: a fold that asked for
    /// every row under the fleet's own leaves this file carrying nothing else.
    fn listing_dirs_path(&self) -> PathBuf {
        self.root.join("listing-dirs")
    }

    /// The directories, in call order. An absent file is no listing at all.
    fn listing_dirs(&self) -> Vec<String> {
        std::fs::read_to_string(self.listing_dirs_path())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn roster_path(&self) -> PathBuf {
        self.root.join("roster.json")
    }
    fn stub_path(&self) -> PathBuf {
        self.root.join("claude-stub")
    }

    /// One per-call seam, as a file the stub reads at an absolute path.
    fn seam_path(&self, name: &str) -> PathBuf {
        self.root.join(format!("seam-{name}"))
    }

    /// The body `daemon status` prints. Absent is an empty body, which parses to
    /// no status at all — a read that answered and named no daemon, which opens
    /// no replacement window.
    fn daemon_status_path(&self) -> PathBuf {
        self.root.join("daemon-status")
    }

    /// A daemon of this pid, up this long, as the agent prints it.
    fn write_daemon(&self, pid: u32, uptime: &str) {
        write(
            &self.daemon_status_path(),
            &format!("pid: {pid}\nuptime: {uptime}\n"),
        );
    }

    /// Every effect call the stub received, one line each, in the order they
    /// arrived. APPENDED and never rewritten: the rest collection's whole
    /// contract is an order, and a file that held only the latest call could not
    /// say what came before it.
    fn calls_path(&self) -> PathBuf {
        self.root.join("calls.log")
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.calls_path())
            .map(|body| body.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The argv, cwd and `PATH` the last start received — read from what the
    /// CHILD got, never from the controller's own log.
    fn start_argv_path(&self) -> PathBuf {
        self.root.join("start-argv")
    }

    /// WHICH FILE the last start exec'd, as the child's own `$0`.
    ///
    /// The argv beside it says what the call passed; only this says which binary
    /// received it, and the two are different questions the moment more than one
    /// program on the box answers to `claude`.
    fn start_bin_path(&self) -> PathBuf {
        self.root.join("start-bin")
    }

    fn start_bin(&self) -> PathBuf {
        let printed =
            std::fs::read_to_string(self.start_bin_path()).expect("the start recorded its own $0");
        PathBuf::from(printed.trim())
    }

    /// The `FLEET_BIN` the last start was handed — the binary the plugin's hooks
    /// in the session it opens will run. Empty is a start that carried none.
    fn start_fleet_bin_path(&self) -> PathBuf {
        self.root.join("start-fleet-bin")
    }

    fn start_fleet_bin(&self) -> String {
        std::fs::read_to_string(self.start_fleet_bin_path())
            .expect("the start recorded its FLEET_BIN")
    }

    /// A copy of the recording stub under the ONE NAME the default seam resolves,
    /// planted where the CONSTRUCTED path finds it — the home's `.local/bin`,
    /// which `platform::child_path` carries and which this rig owns because it
    /// sets `HOME`.
    fn plant_stub_on_the_constructed_path(&self) -> PathBuf {
        let dir = self.home().join(".local").join("bin");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(fleet_controller::adapter::claude_code::DEFAULT_BIN);
        std::fs::copy(self.stub_path(), &path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    fn start_cwd_path(&self) -> PathBuf {
        self.root.join("start-cwd")
    }
    fn start_path_path(&self) -> PathBuf {
        self.root.join("start-path")
    }
    fn nudge_argv_path(&self) -> PathBuf {
        self.root.join("nudge-argv")
    }

    fn start_argv(&self) -> Vec<String> {
        std::fs::read_to_string(self.start_argv_path())
            .unwrap_or_else(|e| panic!("no start reached the stub: {e}"))
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The directory the start was issued in, CANONICAL — `pwd` resolves
    /// symlinks and the temp directory is one, so both sides go in one form.
    fn start_cwd(&self) -> PathBuf {
        let printed =
            std::fs::read_to_string(self.start_cwd_path()).expect("the start recorded its cwd");
        PathBuf::from(printed.trim())
    }

    fn start_path(&self) -> String {
        std::fs::read_to_string(self.start_path_path()).expect("the start recorded its PATH")
    }

    fn sessions(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine().join("sessions.json"))
            .expect("the controller wrote a session table");
        serde_json::from_str(&body).expect("the session table parses")
    }

    fn try_sessions(&self) -> Option<serde_json::Value> {
        let body = std::fs::read_to_string(self.machine().join("sessions.json")).ok()?;
        serde_json::from_str(&body).ok()
    }

    /// Set a seam, or clear it. Absent is off — a seam written as an empty file
    /// would read as a `sleep` with no argument rather than as no sleep at all.
    fn set_seam(&self, name: &str, value: Option<u64>) {
        let path = self.seam_path(name);
        match value {
            Some(seconds) => write(&path, &seconds.to_string()),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// The same stub agent under the ONE NAME the default seam resolves,
    /// `DEFAULT_BIN`, in a directory of the rig's own — and the directory, for a
    /// `PATH` that holds it and nothing else. A reading taken with this on
    /// `PATH` is the default's, and a reading taken with a box's own `PATH` in
    /// place would be whatever agent that box has installed.
    fn write_default_named_stub(&self) -> PathBuf {
        let dir = self.root.join("default-bin");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(fleet_controller::adapter::claude_code::DEFAULT_BIN);
        std::fs::copy(self.stub_path(), &path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }
    fn version_path(&self) -> PathBuf {
        self.root.join("agent-version")
    }

    /// The `--version` branch's own first act, and a TIME and not an existence.
    ///
    /// Distinct from `the-stub-started`, which every branch writes and which
    /// `witnessed` reads for existence alone: after a poll that marker's stamp
    /// is the version call's only because the version call happens to run last,
    /// and a third call added to a poll would silently retarget a reading taken
    /// off it. This one is the version branch's and says so.
    fn version_started_path(&self) -> PathBuf {
        self.root.join("the-version-call-started")
    }

    /// The seconds the `--version` branch sleeps, or absent for none. Written by
    /// `set_version_hang` and read by the stub on every call.
    fn version_hang_path(&self) -> PathBuf {
        self.root.join("version-hang")
    }

    /// Drop the mark, so the next reading cannot be an older poll's. Every arm
    /// timing a version call clears it immediately before the poll it times.
    fn clear_version_start_mark(&self) {
        let _ = std::fs::remove_file(self.version_started_path());
    }

    /// How long the `--version` call lasted, from that mark to the instant
    /// passed in — which the caller takes once the poll it is timing has
    /// returned.
    ///
    /// Read instead of the poll's whole elapsed because a poll pays a process
    /// start-up and a listing call besides the wait, and both move with the box
    /// while a deadline does not. A mark that is not there is a version call
    /// that never reached the binary, which is a panic naming the path and
    /// never a duration.
    fn version_call_lasted(&self, until: SystemTime) -> Duration {
        let path = self.version_started_path();
        let started = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or_else(|e| {
                panic!(
                    "the version call's start mark at {} reads: {e}",
                    path.display()
                )
            });
        until
            .duration_since(started)
            .expect("the poll returned after the version call started")
    }

    /// The listing branch's own first act, a TIME for the reason the version
    /// branch's mark is one.
    fn listing_started_path(&self) -> PathBuf {
        self.root.join("the-listing-call-started")
    }

    /// Drop both call marks, so a span read after the next poll is that poll's.
    fn clear_call_start_marks(&self) {
        let _ = std::fs::remove_file(self.listing_started_path());
        self.clear_version_start_mark();
    }

    /// How long the listing call lasted: its start mark to the version branch's,
    /// which is the first stamp after the listing's collect returns because a
    /// poll calls the listing and then the version (`run.rs`).
    ///
    /// Read instead of the poll's whole elapsed for the reason
    /// `version_call_lasted` is. A missing mark, or a version mark not later than
    /// the listing's, is a panic naming both paths and never a duration.
    fn listing_call_lasted(&self) -> Duration {
        let listing = self.listing_started_path();
        let version = self.version_started_path();
        let stamp = |path: &Path| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .unwrap_or_else(|e| {
                    panic!(
                        "the listing call's span, {} to {}, is not a reading: {} reads: {e}",
                        listing.display(),
                        version.display(),
                        path.display()
                    )
                })
        };
        let (started, ended) = (stamp(listing.as_path()), stamp(version.as_path()));
        match ended.duration_since(started) {
            Ok(span) if !span.is_zero() => span,
            _ => panic!(
                "the listing call's span, {} to {}, is not a reading: the version \
                 mark is not later than the listing mark",
                listing.display(),
                version.display()
            ),
        }
    }
    fn stderr_path(&self) -> PathBuf {
        self.root.join("controller.err")
    }
    fn escape_path(&self) -> PathBuf {
        self.root.join("escape")
    }
    /// The escapee touches this once it has left the process group, and the
    /// arms read it as the witness that the escape happened at all. It carries
    /// the escapee's own pid, which is what an arm reading the holder's state
    /// after the kill has to name — the file says the escape happened, and the
    /// pid in it says which process to look at.
    fn escaped_path(&self) -> PathBuf {
        self.root.join("escaped")
    }

    /// The escapee's pid, from the witness above.
    ///
    /// Polled and not read once: the waiter inside the helper returns as soon as
    /// the file EXISTS, so a reader arriving between the create and the write
    /// finds it empty rather than missing, and an empty read is not an answer.
    ///
    /// A call taken through `witnessed` has already been held to this same
    /// reading, so the first turn answers; the poll is what covers an arm that
    /// reaches the helper by some other path.
    fn escapee_pid(&self) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(pid) = self.escape_witness() {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "the escape witness never carried a pid: {}",
                self.escaped_path().display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    /// The same witness, one line per escape and never removed. `escaped_path`
    /// is rewritten per call, so under a LOOP it says only whether the latest
    /// escape has happened yet; an arm that needs to know how many of its polls
    /// escaped has to count, because an escape can lose its race and the poll
    /// still happens.
    fn escape_log_path(&self) -> PathBuf {
        self.root.join("escaped.log")
    }

    /// Counted by MATCHES and not by lines: the escapee appends through two
    /// interpreters and a shell, and a count that a mangled separator turns into
    /// a constant 1 is a difference of zero between any two readings — which
    /// reads exactly like an escape that never happened.
    fn escapes_so_far(&self) -> usize {
        std::fs::read_to_string(self.escape_log_path())
            .map(|body| body.matches("escaped").count())
            .unwrap_or(0)
    }

    /// The policy file, written so the controller's mtime gate SEES the write.
    ///
    /// `run.rs`'s `tick` re-reads policy only when the file's mtime differs from
    /// the one it last read, and this volume stamps at one-second granularity —
    /// so two writes inside one second are one write to the loop, and an arm
    /// that wanted the second read had to buy the granularity back in wall
    /// clock. The stamp is put where the gate must see it instead: strictly
    /// ahead of the stamp the file already carried, and asserted to have moved,
    /// so the wait is zero and the arm reads the bump rather than the clock.
    ///
    /// ONLY A WRITE THAT REPLACES ONE, and this narrowness is load-bearing. The
    /// gate can miss only a write that FOLLOWS an earlier one; a first write has
    /// no earlier stamp to collide with and no reader yet. Bumping it too was
    /// measured raising a neighbouring arm's failure rate about threefold under
    /// two concurrent runs of this binary — a stub that then missed its start
    /// twice instead of once — so the first write is left exactly as it was.
    fn write_policy(&self, body: &str) {
        let path = self.policy_path();
        let before = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());
        write(&path, body);
        let Some(was) = before else {
            return;
        };
        bump_mtime_past(&path, was);
        let after = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("the policy file this write just made carries an mtime");
        assert_ne!(
            after,
            was,
            "the policy write left the file's mtime where it was, so the \
             controller's gate cannot see it and every arm past this point is \
             reading a file the loop never re-read: {}",
            path.display()
        );
    }
    /// The policy file's mtime AS THE GATE READS IT — off the file, never off
    /// the projection, which carries the stamp of the policy in FORCE and so
    /// answers a different question whenever a write did not parse.
    fn policy_mtime(&self) -> std::time::SystemTime {
        std::fs::metadata(self.policy_path())
            .and_then(|m| m.modified())
            .expect("the policy file carries an mtime")
    }

    fn write_config(&self, body: &str) {
        write(&self.machine().join("config.json"), body);
    }
    fn write_roster(&self, body: &str) {
        write(&self.roster_path(), body);
    }

    /// The session's transcript, under the agent's configuration directory at
    /// the encoding the agent uses: EVERY non-alphanumeric character is a dash.
    /// Spelled out here rather than called from the crate, so a change to the
    /// implementation's rule is caught instead of followed.
    fn write_transcript_under(&self, config_dir: &Path, session: &str, body: &str) {
        let encoded: String = self
            .worktree()
            .display()
            .to_string()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        write(
            &config_dir
                .join("projects")
                .join(encoded)
                .join(format!("{session}.jsonl")),
            body,
        );
    }

    fn write_transcript(&self, session: &str, body: &str) {
        self.write_transcript_under(&self.home().join(".claude"), session, body);
    }

    /// The same transcript, with its last write placed in the past.
    ///
    /// The controller reads a session's END off this mtime, so an arm about a
    /// session that finished hours ago has to age the FILE and not only the
    /// row's start stamp — the two answer different questions and that is the
    /// whole subject of the window.
    fn age_transcript(&self, session: &str, ms_ago: u64) {
        let path = self
            .home()
            .join(".claude")
            .join("projects")
            .join(encode_project_dir(&self.worktree().display().to_string()))
            .join(format!("{session}.jsonl"));
        let when = std::time::SystemTime::now() - Duration::from_millis(ms_ago);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap_or_else(|e| panic!("the transcript is there to age: {} ({e})", path.display()))
            .set_times(std::fs::FileTimes::new().set_modified(when))
            .expect("the transcript's mtime moves");
    }

    /// The stub reads the version it should report out of a file, so an arm can
    /// move it — or make the read fail — under a running controller.
    ///
    /// `cat` is named at an ABSOLUTE path, so the listing and the version branch
    /// resolve nothing on `PATH`: a control that breaks the search path can then
    /// keep this same program and move the seam alone.
    ///
    /// EVERY SEAM IS A FILE AT AN INTERPOLATED ABSOLUTE PATH, and none of them
    /// is an environment variable. The adapter clears the environment of every
    /// child it spawns and rebuilds it — the constructed `PATH` and four values
    /// a shell needs, and nothing else (lessons claude-code D1) — so a variable
    /// this rig exported to the CONTROLLER never reaches the stub the controller
    /// spawns. The rig writes each per-call seam before the call and removes the
    /// ones its fields leave unset, so a seam is on for exactly the calls that
    /// asked for it. The `PREAMBLE` seam is the one exception and is consumed
    /// rather than re-written: the stub empties it with the same builtin
    /// redirect its marks use, so the next invocation reads an empty body and
    /// `[ -n ]` reads that as off. Emptying and not `rm`, because a bare `rm`
    /// here would be a `PATH` resolution and a fork on every stub invocation in
    /// the suite, including the arms that break the search path on purpose.
    ///
    /// WHAT STILL RESOLVES ON `PATH`, in full. `sleep` is a bare name and six
    /// seams reach it: the preamble above the `case`, the hang and the
    /// descendant fork under `agents`, and the version hang, the file-driven
    /// version hang and the version descendant fork under `--version`. The
    /// escapee seam runs `write_escape_helper`'s script, which adds three more
    /// bare names: `rm`, `python3` and a `sleep` of its own.
    /// NO START MARK IS AMONG THEM — the stub's own at the top and the listing
    /// and version branches' inside it are each a redirect onto a builtin at an
    /// interpolated absolute path, so each resolves nothing and costs no fork.
    /// The preamble's own consumption is that same redirect.
    ///
    /// The `PATH` those bare names resolve on is the CONSTRUCTED one, which the
    /// adapter sets on every child and which no arm can move: it is built from
    /// the platform's own list and `HOME`. So an arm that breaks the search path
    /// varies what the CONTROLLER resolves its own binary on and nothing the stub
    /// sees, and it is free to set any seam here.
    fn write_stub_agent(&self) {
        let cat = cat_bin();
        let started = self.stub_started_path();
        let started = started.display();
        let version_started = self.version_started_path();
        let version_started = version_started.display();
        let listing_started = self.listing_started_path();
        let listing_started = listing_started.display();
        let version_hang = self.version_hang_path();
        let version_hang = version_hang.display();
        let seam_version_descendant = self.seam_path(VERSION_DESCENDANT);
        let seam_version_descendant = seam_version_descendant.display();
        let seam_version_hang = self.seam_path(VERSION_HANG);
        let seam_version_hang = seam_version_hang.display();
        let seam_descendant = self.seam_path(DESCENDANT);
        let seam_descendant = seam_descendant.display();
        let seam_escapee = self.seam_path(ESCAPEE);
        let seam_escapee = seam_escapee.display();
        let seam_hang = self.seam_path(HANG);
        let seam_hang = seam_hang.display();
        let seam_preamble = self.seam_path(PREAMBLE);
        let seam_preamble = seam_preamble.display();
        let roster = self.roster_path();
        let roster = roster.display();
        let version = self.version_path();
        let version = version.display();
        let escape = self.escape_path();
        let escape = escape.display();
        let escaped = self.escaped_path();
        let escaped = escaped.display();
        let calls = self.calls_path();
        let calls = calls.display();
        let start_bin = self.start_bin_path();
        let start_bin = start_bin.display();
        let start_fleet_bin = self.start_fleet_bin_path();
        let start_fleet_bin = start_fleet_bin.display();
        let start_argv = self.start_argv_path();
        let start_argv = start_argv.display();
        let start_cwd = self.start_cwd_path();
        let start_cwd = start_cwd.display();
        let start_path = self.start_path_path();
        let start_path = start_path.display();
        let nudge_argv = self.nudge_argv_path();
        let nudge_argv = nudge_argv.display();
        let seam_start_exit = self.seam_path(START_EXIT);
        let seam_start_exit = seam_start_exit.display();
        let seam_stop_exit = self.seam_path(STOP_EXIT);
        let seam_stop_exit = seam_stop_exit.display();
        let seam_rm_exit = self.seam_path(RM_EXIT);
        let seam_rm_exit = seam_rm_exit.display();
        let seam_nudge_exit = self.seam_path(NUDGE_EXIT);
        let seam_nudge_exit = seam_nudge_exit.display();
        let seam_attach_exit = self.seam_path(ATTACH_EXIT);
        let seam_attach_exit = seam_attach_exit.display();
        let seam_daemon_exit = self.seam_path(DAEMON_EXIT);
        let seam_daemon_exit = seam_daemon_exit.display();
        let daemon_status = self.daemon_status_path();
        let daemon_status = daemon_status.display();
        let listing_dirs = self.listing_dirs_path();
        let listing_dirs = listing_dirs.display();
        write(
            &self.stub_path(),
            &format!(
                "#!/bin/sh\n\
                 : > '{started}'\n\
                 p=$({cat} '{seam_preamble}' 2>/dev/null)\n\
                 : > '{seam_preamble}'\n\
                 [ -n \"$p\" ] && sleep \"$p\"\n\
                 case \"$1\" in\n\
                 \x20 --version)\n\
                 \x20   : > '{version_started}'\n\
                 \x20   d=$({cat} '{seam_version_descendant}' 2>/dev/null)\n\
                 \x20   [ -n \"$d\" ] && ( sleep \"$d\" ) &\n\
                 \x20   s=$({cat} '{seam_version_hang}' 2>/dev/null)\n\
                 \x20   [ -n \"$s\" ] && sleep \"$s\"\n\
                 \x20   h=$({cat} '{version_hang}' 2>/dev/null)\n\
                 \x20   [ -n \"$h\" ] && sleep \"$h\"\n\
                 \x20   v=$({cat} '{version}')\n\
                 \x20   [ \"$v\" = FAIL ] && exit 1\n\
                 \x20   [ \"$v\" = SILENT ] && exit 0\n\
                 \x20   echo \"$v (a stub)\"\n\
                 \x20   ;;\n\
                 \x20 agents)\n\
                 \x20   : > '{listing_started}'\n\
                 \x20   d=$({cat} '{seam_descendant}' 2>/dev/null)\n\
                 \x20   [ -n \"$d\" ] && ( sleep \"$d\" ) &\n\
                 \x20   e=$({cat} '{seam_escapee}' 2>/dev/null)\n\
                 \x20   [ -n \"$e\" ] && '{escape}' \"$e\" '{escaped}'\n\
                 \x20   h=$({cat} '{seam_hang}' 2>/dev/null)\n\
                 \x20   [ -n \"$h\" ] && sleep \"$h\"\n\
                 \x20   printf '%s\\n' \"$CLAUDE_CONFIG_DIR\" >> '{listing_dirs}'\n\
                 \x20   if [ -f \"$CLAUDE_CONFIG_DIR/roster.json\" ]; then\n\
                 \x20     {cat} \"$CLAUDE_CONFIG_DIR/roster.json\"\n\
                 \x20   else\n\
                 \x20     {cat} '{roster}'\n\
                 \x20   fi\n\
                 \x20   ;;\n\
                 \x20 --bg)\n\
                 \x20   printf '%s' \"$0\" > '{start_bin}'\n\
                 \x20   printf '%s' \"${{FLEET_BIN-}}\" > '{start_fleet_bin}'\n\
                 \x20   printf '%s\\n' \"$@\" > '{start_argv}'\n\
                 \x20   pwd > '{start_cwd}'\n\
                 \x20   printf '%s' \"$PATH\" > '{start_path}'\n\
                 \x20   echo \"start $*\" >> '{calls}'\n\
                 \x20   echo 'the start spoke'\n\
                 \x20   exit $({cat} '{seam_start_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 stop)\n\
                 \x20   echo \"stop $2\" >> '{calls}'\n\
                 \x20   exit $({cat} '{seam_stop_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 rm)\n\
                 \x20   echo \"rm $2\" >> '{calls}'\n\
                 \x20   exit $({cat} '{seam_rm_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 attach)\n\
                 \x20   echo \"attach $2\" >> '{calls}'\n\
                 \x20   exit $({cat} '{seam_attach_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 daemon)\n\
                 \x20   {cat} '{daemon_status}' 2>/dev/null\n\
                 \x20   exit $({cat} '{seam_daemon_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 -p)\n\
                 \x20   printf '%s\\n' \"$@\" > '{nudge_argv}'\n\
                 \x20   echo \"nudge $2 $3\" >> '{calls}'\n\
                 \x20   exit $({cat} '{seam_nudge_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 *) exit 64;;\n\
                 esac\n"
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(self.stub_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        self.write_escape_helper();
    }

    /// Fork a descendant that LEAVES the child's process group and holds the
    /// inherited pipe, and do not return until it has actually left.
    ///
    /// The wait is the point. An escapee still inside the group when the kill
    /// lands is an ordinary descendant, so an arm that races the interpreter's
    /// start measures the case it was not written for — and passes.
    ///
    /// Two witnesses, because they answer different questions: the per-call file
    /// says whether THIS call's escape has happened, and the log beside it
    /// counts escapes across calls for an arm running the loop.
    ///
    /// THE WAIT IS THE PIPE READ. The shell execs `python3`, which opens a pipe
    /// and forks: the child calls `setsid`, writes its pid to the marker, appends
    /// the escaped line to the log, signals the pipe and then sleeps the life,
    /// while the parent reads the one byte and exits. So when this helper returns
    /// the marker is there or `python3` failed — no polling, and no forks of an
    /// external `sleep` inside the seam the arm is measured on.
    fn write_escape_helper(&self) {
        write(
            &self.escape_path(),
            "#!/bin/sh\n\
             rm -f \"$2\"\n\
             exec python3 -c 'import os,sys,time\n\
             r,w = os.pipe()\n\
             if os.fork() == 0:\n\
             \x20   os.close(r)\n\
             \x20   os.setsid()\n\
             \x20   open(sys.argv[1],\"w\").write(str(os.getpid()))\n\
             \x20   open(sys.argv[1]+\".log\",\"a\").write(\"escaped\\n\")\n\
             \x20   os.write(w, b\"x\")\n\
             \x20   os.close(w)\n\
             \x20   time.sleep(float(sys.argv[2]))\n\
             \x20   os._exit(0)\n\
             os.close(w)\n\
             os.read(r, 1)' \"$2\" \"$1\"\n",
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(self.escape_path(), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }

    /// What `claude --version` reports from now on. `FAIL` makes the read fail
    /// the way a broken or missing binary does, and `SILENT` answers with
    /// success and nothing to read, which is the other way to have no version.
    fn set_version(&self, version: &str) {
        write(&self.version_path(), version);
    }

    /// How long the `--version` branch sleeps from now on, in seconds.
    ///
    /// A FILE and not the `version_hang_seconds` seam beside it, for the one
    /// reason `set_version` is a file too: the stub re-reads it per call, so an
    /// arm running the LOOP can turn the hang on between polls. The seam is read
    /// from the environment at the child's spawn, which fixes it for the life of
    /// a controller and cannot drive a poll that differs from the one before it.
    fn set_version_hang(&self, seconds: u64) {
        write(&self.version_hang_path(), &seconds.to_string());
    }

    /// The hang above, off again. Absent is no hang, so this removes the file
    /// rather than writing a zero — `sleep 0` is a hang of no seconds, which is
    /// a different thing to say and one the stub would have to branch on.
    fn clear_version_hang(&self) {
        std::fs::remove_file(self.version_hang_path()).expect("the hang file is there to remove");
    }

    /// A one-purpose agent binary and an adapter pointed at it.
    ///
    /// The adapter is built from its fields rather than through `new`: `new`
    /// reads the process environment, which every other test in this binary
    /// shares. The built binary's own deadline is set per arm through
    /// `agent_timeout_ms`, which reaches only that child.
    /// The marker goes in as the line AFTER the shebang, so it is the stub's own
    /// first act and no arm's body has to remember it. A body that carries no
    /// shebang is refused rather than silently left unwitnessed — an
    /// unwitnessed stub would retry three times and panic, which reads as this
    /// bead's class when it is really a malformed body.
    fn stub_adapter(&self, name: &str, body: &str, timeout: Duration) -> ClaudeCode {
        let path = self.root.join(name);
        let (shebang, rest) = body
            .split_once('\n')
            .filter(|(first, _)| first.starts_with("#!"))
            .unwrap_or_else(|| panic!("a stub body opens with a shebang line: {body:?}"));
        // Whether the next witnessed call owes an escape, read off the body
        // rather than declared beside it. It describes the stub most recently
        // written because that is the one the next call runs — an arm that
        // builds an escaping stub and then a plain control has stopped owing it
        // by the time the control is read.
        self.stub_escapes
            .set(body.contains(&self.escape_path().display().to_string()));
        write(
            &path,
            &format!(
                "{shebang}\n: > '{}'\n{rest}",
                self.stub_started_path().display()
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        self.adapter_at(path.display().to_string(), timeout)
    }

    /// An adapter pointed at one program, with every seam this rig's own.
    ///
    /// The child PATH is the PLATFORM's, taken through the same call the built
    /// binary makes, so an arm reading what a stub received compares it against
    /// the constructed value and not against a copy of it written here.
    /// Its READ binary and its EFFECT binary are the same program: every arm
    /// built through here drives `status` or `version`, which are reads, and an
    /// adapter that carried no effect binary would refuse the four verbs.
    fn adapter_at(&self, bin: String, timeout: Duration) -> ClaudeCode {
        ClaudeCode::with_seams(
            bin.clone(),
            self.home().join(".claude"),
            timeout,
            self.machine(),
            child_path(&self.home()),
            Some(PathBuf::from(bin)),
            String::new(),
        )
    }

    /// The built binary, and the stub seams written to disk beside it.
    ///
    /// THE SEAMS ARE FILES AND NOT EXPORTS. What this process exports reaches
    /// the controller and stops there: the adapter clears the environment of
    /// every child it spawns, so a variable set here would never reach the stub.
    /// The five per-call ones are written before the call and removed when the
    /// rig's field is `None`, which is exactly what the `env_remove` pairs did;
    /// the one-shot sixth is written once and removed by the stub.
    ///
    /// The five variables the controller ITSELF reads stay exports, because the
    /// controller is this process's own child and nothing clears its
    /// environment.
    fn command(&self) -> Command {
        let mut cmd = self.binary();
        cmd.arg("observe");
        cmd
    }

    /// The six per-call seams written where the stub will read them. Every call
    /// a rig makes goes through here, whichever side of the process boundary it
    /// runs on.
    fn arm_seams(&self) {
        self.set_seam(HANG, self.hang_seconds);
        self.set_seam(VERSION_HANG, self.version_hang_seconds);
        self.set_seam(DESCENDANT, self.descendant_seconds);
        self.set_seam(VERSION_DESCENDANT, self.version_descendant_seconds);
        self.set_seam(ESCAPEE, self.escapee_seconds);
        // The sixth is armed at most once and is never cleared here: the stub
        // removes the file as it reads it, and a rig that re-armed it would
        // delay every attempt instead of one.
        if let Some(ms) = self.one_shot_preamble_ms.take() {
            write(
                &self.seam_path(PREAMBLE),
                &format!("{}.{:03}", ms / 1000, ms % 1000),
            );
        }
    }

    /// The built binary with this rig's environment and no subcommand — what
    /// the `fleet event` arms run.
    fn binary(&self) -> Command {
        self.arm_seams();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fleet"));
        use common::hermetic::Hermetic as _;
        cmd.hermetic(&self.home(), &self.machine(), Some(&self.stub_path()))
            // The routines' clock seam is always pointed at this rig's own file.
            // An arm that never writes it leaves the routines on the machine's
            // clock, which is what every arm that is not about routines wants.
            .env("FLEET_ORDERS_CLOCK", self.clock_path())
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("FLEET_AGENT_TIMEOUT_MS");
        if let Some(dir) = &self.scoped_config_dir {
            cmd.env("CLAUDE_CONFIG_DIR", dir);
        }
        if let Some(ms) = self.agent_timeout_ms {
            cmd.env("FLEET_AGENT_TIMEOUT_MS", ms.to_string());
        }
        cmd
    }

    /// One poll, re-run if its stub never started.
    fn observe(&self) -> Output {
        self.witnessed(|| self.observe_unwitnessed())
    }

    /// One poll, taken as it came.
    ///
    /// FOR ARMS WHOSE PREMISE IS A STUB THAT DOES NOT EXECUTE — a binary that is
    /// not there, a blank seam resolving to a default that is not there. The
    /// marker's absence is those arms' subject, so the wrapper above would spend
    /// three attempts on it and then panic in this bead's name for a reading the
    /// arm meant to take.
    fn observe_unwitnessed(&self) -> Output {
        self.poll_in_process(&[])
    }

    /// One poll through the BUILT BINARY, re-run if its stub never started.
    ///
    /// TWO KINDS OF ARM NEED THE SEPARATE PROCESS.
    ///
    /// One is an arm whose subject is WHICH EXECUTABLE IS RUNNING: a routine's
    /// children are handed the running binary's own directory in front of the
    /// constructed path — `routines::action::path_for_children`, over
    /// `current_exe` — and in this process that directory holds the test binary
    /// and no `fleet`.
    ///
    /// The other is an arm whose stub forks an ESCAPEE. That descendant calls
    /// `setsid` in order to outlive the call, so a poll run in this process
    /// hands it the test binary's own streams and it holds them after the arm
    /// has returned — which nextest reads as a leak.
    fn observe_out_of_process(&self) -> Output {
        self.witnessed(|| {
            self.command()
                .arg("--once")
                .output()
                .expect("the built binary runs")
        })
    }

    /// One poll with the environment MOVED for its duration — the arms whose
    /// subject is a variable the loop itself reads, and which therefore cannot
    /// take the rig's own value for it.
    fn observe_with_env(&self, overrides: &[(&str, Option<&OsStr>)]) -> Output {
        self.poll_in_process(overrides)
    }

    /// ONE POLL, RUN IN THIS PROCESS.
    ///
    /// `run::observe_seamed` under `run::Wiring::resolve` is what the binary's
    /// `observe` subcommand reaches after its argument parsing: it builds the
    /// agent this fleet runs from the environment, resolves the binary its
    /// effects exec, and ticks. Calling it here is the same loop over the same
    /// fixture with one process fewer between the arm and it.
    ///
    /// THE ONE THING THE BINARY ASKS FOR AND THIS DOES NOT: the process's
    /// SIGINT and SIGTERM handlers. They belong to the whole binary, and a test
    /// process holding them answers the harness's SIGTERM by living on.
    ///
    /// TWO THINGS THE ARM DOES NOT GET, and neither is any arm's subject. The
    /// run seam the binary fills is `None` here, because `fleet-cli` is one
    /// binary and no library, so its `Engine` is unreachable from a test of it;
    /// no fixture in this file opens a run, so the pass would ask no store
    /// anything. And the two lines `main` adds to a refused startup — its own
    /// error chain — are absent, while the loop's own line naming the path it
    /// could not read is what the two refusal arms assert on.
    ///
    /// The status is the loop's own `u8` dressed as a wait status, which is the
    /// same number the binary's exit table maps it to for the two the loop can
    /// answer: `0` and `EXIT_NO_POLICY`.
    fn poll_in_process(&self, overrides: &[(&str, Option<&OsStr>)]) -> Output {
        let _held = IN_PROCESS.lock().unwrap_or_else(|p| p.into_inner());
        self.arm_seams();

        let mut env = EnvHeld::new();
        for (key, value) in
            common::hermetic::vars(&self.home(), &self.machine(), Some(&self.stub_path()))
        {
            env.set(key, Some(value));
        }
        // The routines' clock seam is always pointed at this rig's own file. An
        // arm that never writes it leaves the routines on the machine's clock,
        // which is what every arm that is not about routines wants.
        env.set(
            "FLEET_ORDERS_CLOCK",
            Some(self.clock_path().into_os_string()),
        );
        env.set(
            "CLAUDE_CONFIG_DIR",
            self.scoped_config_dir
                .as_ref()
                .map(|dir| dir.clone().into_os_string()),
        );
        env.set(
            "FLEET_AGENT_TIMEOUT_MS",
            self.agent_timeout_ms
                .map(|ms| OsString::from(ms.to_string())),
        );
        for (key, value) in overrides {
            env.set(key, value.map(OsStr::to_os_string));
            // An arm that names the agent-binary seam ITSELF owns the
            // resolution, so the refusal standing in for a name nobody set is
            // lifted with it: an arm whose subject IS the `PATH` fallback names
            // that seam blank or absent, and would otherwise be refused before
            // it could read what the fallback answers.
            if *key == common::hermetic::CLAUDE_BIN {
                env.set(common::hermetic::HERMETIC, None);
            }
        }

        // The stop flag is process-wide and the loop reads it to decide whether
        // it owes a `controller.stopped` event, so every poll starts from a
        // fleet nobody has asked to stop.
        fleet_controller::platform::clear_stop();

        // Its own pair of files and not `stderr_path`, which names the stream a
        // SPAWNED loop writes: the arm that refuses to count a stream nobody
        // captured reads that path's absence.
        let out = Redirected::to(1, self.root.join("in-process.out"));
        let err = Redirected::to(2, self.root.join("in-process.err"));
        let clock = FakeClock::new();
        let wiring = run::Wiring::resolve();
        let status = run::observe_seamed(
            &run::Options { once: true },
            fleet_controller::platform::Grant::new(
                fleet_controller::platform::directory_listing(),
                fleet_controller::platform::GRANT_PROBE_TIMEOUT,
            ),
            None,
            wiring.seams(&clock, run::StopHandler::Unarmed),
        );
        let stderr = err.taken();
        let stdout = out.taken();
        drop(env);

        Output {
            status: ExitStatus::from_raw(i32::from(status) << 8),
            stdout,
            stderr,
        }
    }

    /// ONE LOOP, DRIVEN POLL BY POLL IN THIS PROCESS.
    ///
    /// For the arms whose subject lives ACROSS polls: an announcement that
    /// stands, a latch that has already said its line, a policy path the last
    /// poll moved. A sequence of `--once` polls is not their shape — each one
    /// starts a loop that remembers nothing, so the state under test is gone
    /// before the second poll can read it. `run::Observer` is that state and
    /// `Ticks::tick` is one poll of it, so an arm holds the loop, edits the
    /// fixture and polls again.
    ///
    /// THE TWO THINGS A SPAWNED LOOP GIVES AN ARM THAT THIS DOES NOT, and
    /// neither is one of these arms' subject. A poll here runs when the arm says
    /// so rather than when the interval elapses, so an arm reads no wait and no
    /// ordering between a fixture edit and a poll already in flight. And the
    /// exit status, the signal handling and the two lines `main` adds to a
    /// refused startup belong to the binary, which is not in the picture; the
    /// startup refusals are held by the two arms that run through
    /// `poll_in_process` and by the acceptance drive against the real fleet.
    ///
    /// The environment and the lock are held for the WHOLE sequence, because the
    /// loop keeps reading both between polls. The captured streams are not: each
    /// tick takes the descriptors and gives them back, so an assertion the arm
    /// makes between two polls is printed where a person can read it.
    fn driving(&self, body: impl FnOnce(&mut Ticks)) {
        let _held = IN_PROCESS.lock().unwrap_or_else(|p| p.into_inner());
        self.arm_seams();

        let mut env = EnvHeld::new();
        for (key, value) in
            common::hermetic::vars(&self.home(), &self.machine(), Some(&self.stub_path()))
        {
            env.set(key, Some(value));
        }
        env.set(
            "FLEET_ORDERS_CLOCK",
            Some(self.clock_path().into_os_string()),
        );
        env.set(
            "CLAUDE_CONFIG_DIR",
            self.scoped_config_dir
                .as_ref()
                .map(|dir| dir.clone().into_os_string()),
        );
        env.set(
            "FLEET_AGENT_TIMEOUT_MS",
            self.agent_timeout_ms
                .map(|ms| OsString::from(ms.to_string())),
        );

        fleet_controller::platform::clear_stop();

        // The stream the counting arms read, created empty BEFORE the first poll
        // exactly as `spawn_loop` creates it before the child — so a rig that has
        // driven always has one, and the refusal
        // `counting_a_stream_that_was_never_captured_is_a_refusal_and_never_a_zero`
        // pins stays reachable only by a rig that has done neither.
        write(&self.stderr_path(), "");

        let clock = FakeClock::new();
        let wiring = run::Wiring::resolve();
        let started = {
            let _out = Redirected::appending(1, self.root.join("in-process.out"));
            let _err = Redirected::appending(2, self.stderr_path());
            run::Observer::start(
                fleet_controller::platform::Grant::new(
                    fleet_controller::platform::directory_listing(),
                    fleet_controller::platform::GRANT_PROBE_TIMEOUT,
                ),
                None,
                wiring.seams(&clock, run::StopHandler::Unarmed),
            )
        };
        let mut ticks = Ticks {
            rig: self,
            observer: started.expect("the loop reads what it needs and starts"),
        };
        body(&mut ticks);
        drop(ticks);
        drop(env);
    }

    /// The loop, not one poll: what the running-controller arms need. Its stderr
    /// is kept in every case, because the once-per-change logging clauses are
    /// stated on that stream and nowhere else.
    ///
    /// BIND THE GUARD AFTER THE RIG. Nothing enforces it: the returned
    /// `Controller` holds no reference to this `Rig`, so the order the two are
    /// declared in is the order they drop in, and a guard declared first stops
    /// the loop after the rig has removed the directory the loop then recreates.
    /// The sentence lives here as well as on `Controller` because a call site
    /// writes `rig.spawn_loop()` and never names the type.
    fn spawn_loop(&self) -> Controller {
        let log = std::fs::File::create(self.stderr_path()).expect("the log file opens");
        Controller {
            child: self
                .command()
                .stdout(Stdio::null())
                .stderr(Stdio::from(log))
                .spawn()
                .expect("the built binary starts"),
        }
    }

    /// How many lines of the captured stream carry `needle`. A stream that is
    /// not there is a rig that captured nothing, and answering 0 for it would
    /// let a zero-count assertion pass against it.
    fn stderr_lines_with(&self, needle: &str) -> usize {
        let path = self.stderr_path();
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("the captured stream at {} reads: {e}", path.display()))
            .lines()
            .filter(|l| l.contains(needle))
            .count()
    }

    /// The same poll, on a bound the caller names, that CARRIES ITS ELAPSED
    /// either way.
    ///
    /// `wait_until` below answers a bare false after a fixed twenty seconds, so
    /// its callers' `assert!(rig.wait_until(…), "the loop polls again")` says
    /// nothing about whether the box was one poll short or the controller never
    /// published at all — the two readings a person deciding whether to raise a
    /// ceiling has to tell apart. This one panics with the what, the elapsed and
    /// the bound, in the shape `witness_within` uses, and prints the elapsed to
    /// stderr on success so the ceiling is re-measurable from a run.
    ///
    /// THE BOUND IS AN ANTI-HANG CEILING AND NEVER A READING, the same sense
    /// `witness_within`'s is: it ends the moment the document says so, and it
    /// exists only so a controller that never publishes fails the arm instead of
    /// hanging the lane.
    fn wait_until_within(
        &self,
        ready: impl Fn(&serde_json::Value) -> bool,
        bound: Duration,
        what: &str,
    ) {
        let started = Instant::now();
        loop {
            if let Some(published) = self.try_projection() {
                if ready(&published) {
                    eprintln!(
                        "witness: {what} after {:?} against a {bound:?} bound",
                        started.elapsed()
                    );
                    return;
                }
            }
            assert!(
                started.elapsed() < bound,
                "{what}: waited {:?} against a {bound:?} bound and it never appeared",
                started.elapsed()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Poll the published document until it says what the arm is waiting for.
    /// A timeout returns false rather than hanging the suite.
    fn wait_until(&self, ready: impl Fn(&serde_json::Value) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Some(published) = self.try_projection() {
                if ready(&published) {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn try_projection(&self) -> Option<serde_json::Value> {
        let body = std::fs::read_to_string(self.machine().join("projection.json")).ok()?;
        serde_json::from_str(&body).ok()
    }

    fn projection(&self) -> serde_json::Value {
        self.try_projection()
            .expect("a poll publishes a projection")
    }

    /// The published document as it was written. A forbidden KEY is a question
    /// about the parse; a forbidden STRING is a question about the bytes a
    /// reader opens, and only this answers the second.
    fn projection_body(&self) -> String {
        std::fs::read_to_string(self.machine().join("projection.json"))
            .expect("a poll publishes a projection")
    }

    fn events(&self) -> Vec<serde_json::Value> {
        let path = self.machine().join("events.jsonl");
        let Ok(body) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        body.lines()
            .map(|l| serde_json::from_str(l).expect("every line is one JSON object"))
            .collect()
    }

    fn events_of(&self, kind: &str) -> usize {
        self.events().iter().filter(|e| e["type"] == kind).count()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One loop, held between polls, for the arms `Rig::driving` hands it to.
///
/// The rig rides along so a poll captures into the same stream the counting
/// helpers read.
struct Ticks<'a> {
    rig: &'a Rig,
    observer: run::Observer<'a>,
}

impl Ticks<'_> {
    /// One poll of the held loop, with the two streams it states its lines on
    /// captured for the poll's own duration.
    fn tick(&mut self) {
        let _out = Redirected::appending(1, self.rig.root.join("in-process.out"));
        let _err = Redirected::appending(2, self.rig.stderr_path());
        self.observer.tick();
    }

    /// Poll until the published document says what the arm is waiting for, or
    /// refuse after `at_most` polls.
    ///
    /// FOR A PRECONDITION, NEVER FOR A SUBJECT. The budget is a count of POLLS
    /// and not a span of wall clock: one arm sets its agent deadline to a
    /// fraction of a second in order to outrun it on purpose, and a healthy call
    /// that this box was too busy to finish inside that fraction is not the
    /// reading that arm came for. Every arm whose subject is what a poll did
    /// asserts on the tick it drove.
    fn until(&mut self, at_most: usize, ready: impl Fn(&serde_json::Value) -> bool) {
        for _ in 0..at_most {
            self.tick();
            if let Some(published) = self.rig.try_projection() {
                if ready(&published) {
                    return;
                }
            }
        }
        panic!(
            "{at_most} polls and the document still does not say what the arm waits for: {}",
            self.rig.projection_body()
        );
    }
}

/// The file's mtime moved back, without touching a byte of it.
///
/// For the one arm whose witness that a re-read RAN is the published stamp of
/// the policy in force. That stamp is `YYYY-MM-DDTHH:MM:SSZ`, so two polls
/// inside one second publish one stamp however many re-reads ran between them,
/// and a moved-stamp assertion taken in this process reads a re-read that did
/// happen as one that never did. A stamp the arm SETS is a difference the reader
/// can see whatever second the run lands in. Backwards rather than forwards,
/// because a file stamped in the future is a second thing to explain.
/// Put a file's mtime strictly ahead of the stamp it carried before the write,
/// past this volume's ONE-SECOND granularity.
///
/// A gate that re-reads on a moved mtime is blind to two writes inside one
/// second — the file changed and the stamp did not. The step is two seconds
/// and not one because the stamp is truncated to the second, so a one-second
/// step can land on the second already recorded.
fn bump_mtime_past(path: &Path, before: std::time::SystemTime) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("the file to bump at {} opens: {e}", path.display()));
    let now = file
        .metadata()
        .and_then(|m| m.modified())
        .unwrap_or_else(|e| panic!("the file at {} states its mtime: {e}", path.display()));
    let floor = if before > now { before } else { now };
    file.set_times(std::fs::FileTimes::new().set_modified(floor + Duration::from_secs(2)))
        .unwrap_or_else(|e| panic!("the mtime at {} moves: {e}", path.display()));
}

fn age_mtime(path: &Path, by: Duration) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap_or_else(|e| panic!("the file to age at {} opens: {e}", path.display()));
    let was = file.metadata().expect("the file states its times");
    let modified = was.modified().expect("the file states its mtime");
    file.set_times(
        std::fs::FileTimes::new()
            .set_modified(modified.checked_sub(by).expect("the mtime moves back")),
    )
    .unwrap_or_else(|e| panic!("the mtime at {} moves: {e}", path.display()));
}

/// A running controller, stopped when it leaves scope — by a return or by an
/// unwind alike, which is what an arm that fails between the spawn and its own
/// kill needs. Both paths are pinned:
/// `a_loop_that_leaves_scope_is_stopped_and_not_left_polling` takes the first
/// and `a_loop_whose_arm_panics_is_stopped_by_the_unwind_that_leaves_its_scope`
/// the second.
///
/// BIND IT AFTER THE RIG, at every site — the constraint is on `spawn_loop`,
/// where a call site meets it, and nothing enforces it here.
struct Controller {
    child: std::process::Child,
}

impl Controller {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The loop's exit status if it has already exited, and `None` while it is
    /// running.
    ///
    /// `platform::process_alive` cannot answer this one: a child of this process
    /// that dies and is not waited on is a ZOMBIE, and a pid liveness check
    /// reads a zombie as alive — the same property `zombie_children_of` exists
    /// for. An arm that has to know its controller is still polling reads the
    /// handle, which is the only reader that sees the difference.
    ///
    /// A `Some` IS TERMINAL FOR THIS HANDLE. `try_wait` reaps when it answers
    /// one, so from that point the pid this struct carries stops naming this
    /// child and the OS may hand it to anything: `pid()` returns a stale number
    /// and `signal_and_wait` would `/bin/kill` whatever holds it now.
    /// `Drop` and `wait` are safe after it: `kill` refuses on the cached status
    /// and `Drop` discards that, and `wait` answers from the same cache without
    /// a second `waitpid`. The one arm that calls this asserts `None`, so no
    /// caller today reads the handle past a `Some`.
    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().expect("the child is waitable")
    }

    /// Signal the loop and wait for it, which is what the arms asserting a clean
    /// stop need. `Drop` does the same with a kill for every other arm.
    fn signal_and_wait(&mut self, signal: &str) -> std::process::ExitStatus {
        let signalled = Command::new("/bin/kill")
            .args([signal, &self.child.id().to_string()])
            .status()
            .expect("kill runs");
        assert!(signalled.success(), "the signal reaches the loop");
        self.child.wait().expect("the controller exits")
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every fixture file this suite writes, WHOLE OR NOT AT ALL.
///
/// A truncate-then-write is two syscalls, and the loop under test polls the
/// files this writes: `run.rs` re-reads the policy on an mtime change and states
/// a line when the fresh policy differs from the one in force, while
/// `policy::parse` reads an empty body as a policy of defaults. So a poll landing
/// between the truncate and the write sees a policy that really does differ, and
/// states a line the arm counting them did not expect — the controller behaving
/// correctly on a file the FIXTURE tore. The window is microseconds on a quiet
/// box and a whole poll on a loaded one.
///
/// A temp file in the destination directory and then a rename, which is one
/// syscall the reader sees whole. The temp name carries the pid and a counter
/// because arms run in parallel threads of one process, so a pid alone is not
/// unique among them. `platform::write_atomic` in `fleet-controller` is the same
/// shape and is read for it and not reused: the suite does not reach into the
/// crate's own helpers for a fixture write.
///
/// The temp file is unlinked when the rename fails, so a failure leaves the
/// directory as it found it rather than seeding the next arm's listing with a
/// stray name.
fn write(path: &Path, body: &str) {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    let name = path.file_name().expect("a fixture write names a file");
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = std::fs::File::create(&tmp).and_then(|mut f| f.write_all(body.as_bytes()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        panic!("the fixture body reaches {}: {e}", tmp.display());
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        panic!("the fixture body lands at {}: {e}", path.display());
    }
}

/// How many children `pid` is holding as zombies — killed, and never waited on.
/// `ps` is what reads that state: a pid liveness check answers "alive" for a
/// zombie, which is what makes an unreaped child invisible. The parent is a
/// parameter because the accumulation is claimed of the CONTROLLER's process and
/// this suite's own process is only the in-process reading of it.
fn zombie_children_of(pid: u32) -> usize {
    let out = Command::new("/bin/ps")
        .args(["-o", "stat=,ppid=", "-ax"])
        .output()
        .expect("/bin/ps runs");
    let parent = pid.to_string();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| {
            let mut fields = line.split_whitespace();
            let stat = fields.next().unwrap_or_default();
            let ppid = fields.next().unwrap_or_default();
            stat.starts_with('Z') && ppid == parent
        })
        .count()
}

/// Whether THIS pid is being held as a zombie — a state read of one process, not
/// a count over a parent's children.
///
/// What a positive control needs and a count cannot give it. Every arm in this
/// binary shares one parent, so a count at `std::process::id()` is satisfied by
/// any other arm's kill-then-wait window; a control asserting its own child's pid
/// is in `Z` is satisfied by nothing another arm can do. `ps` printing no line is
/// the pid gone, which is not this state either.
fn is_a_zombie(pid: u32) -> bool {
    let out = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("/bin/ps runs");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .is_some_and(|stat| stat.starts_with('Z'))
}

/// Whether `pid` is a live process, read as a STATE and not as a signal.
///
/// The subject is the escapee, which called `setsid` and is a child of nothing
/// this suite waits on — so a `Z` line is the one reading that would answer
/// "there" for a process the kill already reached, and it is excluded here.
/// `ps` printing no line at all is the pid being gone.
fn process_is_alive(pid: u32) -> bool {
    let out = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("/bin/ps runs");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(|stat| !stat.starts_with('Z'))
        .unwrap_or(false)
}

/// The count at the INSTANT it is asked for, settled only in the green
/// direction. A reap already in flight is given the same window to finish, so a
/// zero here is still a zero; but a count that never reaches it is answered with
/// the raw reading taken at the instant, never with whatever the last poll of
/// that window happened to see.
///
/// The difference is the whole content of a term named for a poll. Settling in
/// both directions means a term labelled "after poll one" is read three seconds
/// and several polls later, so under a per-poll leak the figure belongs to a
/// later poll than its own label — and the reader of the red is told the wrong
/// number about the wrong moment.
fn zombie_children_at(pid: u32) -> usize {
    settled_toward_zero(|| zombie_children_of(pid))
}

/// The count above, answered together with the pid it was read AT.
///
/// An equality written about the count alone is satisfied by a term read at this
/// test binary's own pid, which holds no zombies of its own — so an arm whose
/// claim is about the CONTROLLER's process takes its terms from here and asserts
/// the pid beside the count. The pid travels with the reading because a control
/// that re-derives it at the assert judges its own expression and not the one the
/// count was taken with.
///
/// WHAT SUCH A CONTROL PINS IS THE ARGUMENT, NOT THE READ. The pid comes back
/// from this tuple unchanged, so a body that counted at some other pid while
/// echoing the one it was handed satisfies every equality written about it: with
/// the count taken at `std::process::id()` and the argument still returned,
/// `make fleet-test` is rc 0 and every arm over this helper stays green.
///
/// Nothing stronger is available in the direction those arms prove. The count's
/// own `ps` parse holds a ppid only on the lines it matched, and the reading
/// they prove is ZERO — a zero matched no line, and so carries nothing but the
/// argument. The reading that does catch such a body is one taken at a pid known
/// to hold a zombie, where an echo of the argument answers zero; it costs
/// `settled_toward_zero`'s whole window, which is spent trying to settle away
/// the very reading such a control is asking for.
fn zombie_children_at_with_pid(pid: u32) -> (usize, u32) {
    (zombie_children_at(pid), pid)
}

/// The settle above, with the reading PASSED IN.
///
/// The pid form can only be handed the counts this box happens to produce, and
/// on a healthy run that is zero at the first read every time — so the loop
/// below is never entered, and the two counting arms of `drive_children.rs` are
/// what separate this helper from a settle that answers with the window's LAST
/// reading rather than the instant's. A count that moves is the only shape the
/// two answer differently, and it is a parameter here so an arm can hand one
/// over without leaving zombies in this process for the arms that count them.
///
/// THE TWO DIRECTIONS CARRY DIFFERENT TOLERANCES, and the asymmetry is the whole
/// contract. A NONZERO reading is given `SETTLE_WINDOW` to become zero. A ZERO
/// reading is given nothing: the first read answers, and an arm proving a count
/// is zero is therefore reading one instant and not a window. So the direction
/// an arm has to PROVE is the direction with no tolerance in it.
///
/// What that costs is bounded and deliberate: a reap DEFERRED rather than
/// removed is outside the claim. A controller that holds each outrun poll's
/// child as a zombie for about a second and then waits answers zero at whatever
/// instant a term is taken, and every arm over this helper stays green — the
/// claim is that the child is reaped, not that it is reaped inside any window,
/// and an arm that wanted the second one would have to read at the instant of
/// each poll rather than settle at all. A reap REMOVED is what these arms are
/// for, and that one they read.
fn settled_toward_zero(read: impl Fn() -> usize) -> usize {
    let at_the_instant = read();
    if at_the_instant == 0 {
        return 0;
    }
    let deadline = Instant::now() + SETTLE_WINDOW;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        if read() == 0 {
            return 0;
        }
    }
    at_the_instant
}

/// How many file descriptors `pid` holds open. `lsof` is the panel's own
/// instrument for the leak this reads, and the only one of the two counts it
/// took that any process table this suite may run on answers the same way: a
/// thread count needs an operating-system name, which `src/platform/mod.rs`
/// reserves to itself. Each parked drain thread is what holds one of the pipe
/// ends counted here, so the two grow together.
///
/// `None` when the tool is not on this box, which an arm refuses rather than
/// reads as a count.
fn open_fds_of(pid: u32) -> Option<usize> {
    open_descriptors_of(pid).map(|(all, _)| all)
}

/// One `lsof` reading, counted twice: every row, and the PIPE rows alone.
///
/// The price `run_bounded` states is a drain PAIR — two pipe ends — and a tally
/// of every descriptor is satisfied by a build that gives the pair back and
/// leaks two files or sockets instead, so an arm reading that price counts the
/// type it names. Both counts come off the same call because an arm that
/// compares them is comparing one instant.
///
/// TYPE is `lsof`'s fifth column on every row it prints, and an anonymous pipe
/// is `PIPE` here and `FIFO` where Linux names it. The COMMAND column is the
/// first, and this reads a process the suite itself built, whose name has no
/// space in it.
///
/// `None` when the tool is not on this box or answered no rows, which an arm
/// refuses rather than reads as a count.
fn open_descriptors_of(pid: u32) -> Option<(usize, usize)> {
    let out = Command::new(lsof_bin()?)
        .args(["-p", &pid.to_string()])
        .output()
        .ok()?;
    let body = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<&str> = body
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect();
    let pipes = rows
        .iter()
        .filter(|line| matches!(line.split_whitespace().nth(4), Some("PIPE") | Some("FIFO")))
        .count();
    (!rows.is_empty()).then_some((rows.len(), pipes))
}

/// `lsof` at its absolute path, probed rather than resolved on `PATH`, which is
/// how `/bin/kill` and `/bin/cat` are named in this file. It ships in the base
/// system at `/usr/sbin` on macOS and at `/usr/bin` where Linux distributions
/// carry it, and the two sites are searched in that order.
///
/// `None` where neither answers, which is the one refusal the callers keep: an
/// arm reading a descriptor count refuses rather than reading absence as zero.
fn lsof_bin() -> Option<&'static str> {
    ["/usr/sbin/lsof", "/usr/bin/lsof"]
        .into_iter()
        .find(|candidate| Path::new(candidate).exists())
}

/// `cat` at its absolute path, probed rather than assumed: the stub is written
/// with it so it resolves nothing on `PATH`, and a box with no `cat` at either
/// site fails here, naming the reason, instead of as an unreadable listing in
/// every arm.
fn cat_bin() -> &'static str {
    ["/bin/cat", "/usr/bin/cat"]
        .into_iter()
        .find(|candidate| Path::new(candidate).exists())
        .expect("no cat at /bin/cat or /usr/bin/cat: the stub cannot read without PATH")
}

// --- the seams the timing arms race, hoisted so an arm can read them ---------
//
// Inside their own functions these figures are unreadable from anywhere else, so
// nothing in the suite could tell a raised seam from the one it replaced. They
// are pinned as RELATIONS by one arm of `drive_children.rs`, never as the right
// numbers: which number is right is still an open question in this fleet's own
// record.

/// The seam of the arm whose descendant is forked by the stub's own `sh` and is
/// the case as soon as that fork returns. Nothing has to happen in a second
/// process before the arm's subject exists, so this is the cheap end.
const HELD_PIPE_SEAM_MS: u64 = 1000;

/// The life of the holder the two escapee arms race, and the unit their ratio
/// assertions are taken against.
const ESCAPEE_SECONDS: u64 = 10;

/// The exit path's seam, and the deadline path's beside it. They are two consts
/// and not one because they are two arms a future slice may size apart; that
/// they are EQUAL today is asserted rather than assumed.
///
/// THE FIGURE IS NOT THE ESCAPE BUDGET. The deadline's clock runs from the
/// spawn, so the seam has to cover the whole chain — the `sh`, the `python3`
/// start, the `setsid` — and what is left for the escape itself is the seam
/// less that start cost, which the box's load sets. Measured by delaying the
/// `setsid` in the escape helper and reading which side of the witness assert
/// the two arms land on, under `make fleet-test`: a chain of 2.7 s clears the
/// 3000 ms seam and one of 2.9 s does not, so the start cost is one to three
/// tenths of a second here; the panel that filed this row brackets the same
/// boundary between 1.5 s and 2.5 s at load averages of 8 to 11, where the
/// start cost is most of a second. So a slice raising the seam by a tenth is
/// not buying a tenth of escape on every box.
const ESCAPEE_EXIT_SEAM_MS: u64 = 3000;
const ESCAPEE_DEADLINE_SEAM_MS: u64 = 3000;

/// The third seam an escape has to clear: the parked-pair loop's, whose holder
/// leaves the group the same way. It is a const so the floor below reaches it —
/// as a literal at its own arm it could go back to the cheap seam with the gate
/// green, and the comment there naming the escapee figure would be a coupling
/// nothing held.
///
/// It carries no ceiling: that arm asserts a descriptor count and no ratio, and
/// its holder lives far past anything the seam plus the grace comes to.
const ESCAPEE_PARKED_PAIR_SEAM_MS: u64 = 3000;

/// How many times over a bounded listing call's span has to fit inside the
/// patient one's for the arms that read a holder to call it bounded. One const
/// for the four sites: the three arms that assert the ratio, and the ceiling
/// that checks their arithmetic is satisfiable — a ceiling reading a different
/// multiple from the arms would certify a ratio nobody asserts.
const BOUNDED_RATIO_MULTIPLE: u32 = 3;

/// How far above the cheap seam an escapee seam has to sit. The escape is not
/// one fork: it is an `sh`, a `python3` and a `setsid` before the holder the arm
/// names exists at all, and the deadline's clock runs from the spawn of that
/// whole chain — so the figure that suffices where nothing has to escape cannot
/// be the figure here.
///
/// Three is a FLOOR read off two measurements this fleet has already taken, not
/// a claim that three is the right multiple: 1000 ms was measured losing the
/// race under load, which is why these two seams are 3000 ms, and the
/// deadline-path arm is on record still losing it about one run in four AT
/// 3000 ms. So the gap is at least this and is not settled above it, and a pin
/// that fixed the figure
/// would have to be edited by the slice that settles it.
const ESCAPEE_SEAM_FLOOR_MULTIPLE: u64 = 3;

// Why the floor is a multiple and not a measured margin: a margin from a sampler
// dominated by its own poll is a number about the sampler. The 200-sample
// readings that say so — the escape helper's and a plain in-group fork's — are on
// the work item that trimmed them out of here, which the boundary check forbids
// this directory from naming.

/// How long after a descendant's own sleep an arm waits before reading its
/// finish marker absent. One const for the four sites in `drive_causes.rs` that
/// wait `<DESCENDANT const> * 1000 + START_COST_MARGIN_MS`.
///
/// WHAT IT COVERS is one quantity: how far past its nominal sleep a descendant
/// left ALIVE writes its marker. The arm polls the descendant's START marker
/// first, so the fork and the parent stub's exec are already paid when the wait
/// begins; what remains is the `sleep` and the write.
///
/// MEASURED, 2026-09-16, `fleet/tools/descendant-overshoot-probe.py`, which
/// reproduces that descendant verbatim and leaves it alive. Three samples of 20
/// runs at a 5 s sleep: under a full-width `make fleet-test` at 1-minute load
/// 8.23, min 3.33 median 9.03 MAX 14.34 ms; under the same suite at load 7.82,
/// min 2.94 median 8.50 MAX 15.20 ms; on a quiet box at load 4.77, min 5.64
/// median 9.33 MAX 12.86 ms. Twice the largest of the three is 31 ms.
///
/// THE FIGURE IS NOT THAT, AND THE GAP IS DELIBERATE. The same box in the same
/// window stalled a neighbouring shape — a fresh stub's first exec, measured by
/// `fleet/tools/start-cost-probe.py` over 900 samples — for 755 ms during that
/// suite's test phase and 8055 ms during its build phase. A descendant's `sleep`
/// is schedulable the same way, and 20 runs cannot see a tail that rare. So this
/// holds the pre-measurement value until a sample large enough to read the rate
/// says otherwise. The ladder that would license a cut, and the N it needs, are
/// on the work item filed off the one that named this constant — which the
/// boundary check forbids this directory from citing by id.
const START_COST_MARGIN_MS: u64 = 1500;

/// The window `settled_toward_zero` gives a nonzero count to become zero, and
/// the unit any arm holding a child open across that window states its own hold
/// against. Named so a slice that moves the settle moves every hold that has to
/// outlive it: a literal at those sites is a coupling the compiler cannot keep.
const SETTLE_WINDOW: Duration = Duration::from_secs(3);

const HOUR_MS: u64 = 60 * 60 * 1000;

const POLICY_1S: &str =
    "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"9.9.9\"\n";

fn live_row(cwd: &Path, session: &str) -> String {
    format!(
        r#"[{{"id":"{session}","sessionId":"{session}","cwd":"{}","kind":"background",
              "pid":4242,"status":"idle","startedAt":1000}}]"#,
        cwd.display()
    )
}

/// A row the agent has finished with: pid-less and carrying the end marker, so
/// the recency window is what decides whether it is still reported.
///
/// The ADDRESS and the IDENTITY are separate parameters because the controller
/// reads them from separate fields — `id` is what an attach is addressed by,
/// `sessionId` is what the transcript and the end stamp are keyed under. A row
/// writing one value into both cannot fail an arm that issues a call against
/// the wrong one, so an arm whose subject is that distinction takes this
/// spelling and not `ended_row`.
fn ended_row_addressed(cwd: &Path, short_id: &str, session: &str, started_at_ms: u64) -> String {
    format!(
        r#"[{{"id":"{short_id}","sessionId":"{session}","cwd":"{}","kind":"background",
              "state":"done","startedAt":{started_at_ms}}}]"#,
        cwd.display()
    )
}

/// The same row for an arm that is not about the address: one value answers for
/// both fields, which is what the agent writes when the short id IS the session.
fn ended_row(cwd: &Path, session: &str, started_at_ms: u64) -> String {
    ended_row_addressed(cwd, session, session, started_at_ms)
}

/// Wall-clock milliseconds, the same origin the built binary stamps its rows
/// against — an arm that places a row "25 hours ago" is placing it against this
/// clock and not against a figure of its own.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_millis() as u64
}

fn blocked_row(cwd: &Path, session: &str, cause: &str) -> String {
    format!(
        r#"[{{"id":"{session}","sessionId":"{session}","cwd":"{}","kind":"background",
              "pid":4242,"status":"idle","startedAt":1000,"waitingFor":"{cause}"}}]"#,
        cwd.display()
    )
}

/// How long the descendants of the drain-pipe arm in `drive_children.rs` live.
/// It is read twice — as the stub's hang and as the descendant's sleep — and
/// once more as the bound that arm's own window has to sit inside, which is what
/// makes its no-growth assertion able to fail.
const DRAIN_LOOP_DESCENDANT_SECONDS: u64 = 60;

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The position of a flag in an argv, and the element after it. Read
/// POSITIONALLY rather than by scanning for the value, because a flag whose
/// value went missing makes the NEXT flag its argument — which a scan for the
/// value alone cannot see.
fn flag_value<'a>(argv: &'a [String], flag: &str) -> &'a str {
    let at = argv
        .iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("{flag} is not in the argv: {argv:?}"));
    argv.get(at + 1)
        .unwrap_or_else(|| panic!("{flag} is the last element of {argv:?}"))
}
