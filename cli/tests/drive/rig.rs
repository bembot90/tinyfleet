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
// moves, and taking it again is a reader's act.
//
// ELAPSED ASSERTIONS, THE ONE RULE. What makes a reading of a poll's own time a
// reading and not a clock check is WHERE ITS MARGIN COMES FROM, and there are
// two kinds here.
//
// A bound whose margin is A HANG THIS ARM SET is a reading. The arm gives the
// stub seconds of work and a deadline of a few hundred milliseconds, so the two
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

use fleet_controller::platform::child_path;
use fleet_controller::run;
use fleet_controller::test_support::{
    agent_stub, FakeClock, FakeServer, FakeSession, StubAgent, FIRST_PANE_PID,
};
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

/// The rig's one seat. Its row is keyed by the id and carries the seat's own
/// name; every directory, stream line, session name and argument the loop and
/// the verbs write about it is the machine name those two give.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const SEAT: &str = "orla-93b9739a";

/// The environment and the standard streams are the PROCESS's, so one
/// in-process poll runs at a time. Under `cargo test` the arms of this binary
/// share a process across threads, and two of them redirecting fd 2 together
/// would read each other's lines while two setting `FLEET_DIR` together would
/// poll each other's machine directory.
///
/// HELD FOR THE CALL AND NOT FOR THE RIG'S LIFE. Some arms hold two rigs at
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

/// A whole machine the loop runs over, on the agent stub: `fleet-agent-stub`,
/// named by `[agent] adapter` in the rig's own `fleet.toml` and spoken to
/// through the Exec like any adapter executable, its answers scripted into its
/// state beside that file. So an arm about the LOOP — what a poll reads,
/// decides, starts, types and publishes — runs with no agent binary and no
/// shell stub standing in for one.
struct Rig {
    root: PathBuf,
    /// The last component of the seat's worktree.
    leaf: String,
    /// The deadline the BUILT BINARY runs its agent calls on. `None` leaves the
    /// binary on the default it ships with.
    agent_timeout_ms: Option<u64>,
}

/// One session start as the host received it: the directory, the environment
/// the pane was handed and the argv it runs, program first.
#[derive(Clone, Debug)]
struct HostStart {
    cwd: String,
    env: Vec<(String, String)>,
    argv: Vec<String>,
}

impl HostStart {
    /// A start out of one recorded tmux client call, or `None` for a call that
    /// started nothing. The call is the host's own shape — `new-session … -c
    /// <cwd> -- /usr/bin/env -i K=V … TERM=… <argv>` — whose `TERM` is always
    /// the last assignment, so the argv is everything after it.
    fn of(args: &[String]) -> Option<HostStart> {
        let at = args.iter().position(|a| a == "new-session")?;
        let rest = &args[at..];
        let cwd = rest
            .iter()
            .position(|a| a == "-c")
            .and_then(|c| rest.get(c + 1))?
            .clone();
        let command = &rest[rest.iter().position(|a| a == "--")? + 1..];
        let term = command.iter().position(|a| a.starts_with("TERM="))?;
        let env = command[..term]
            .iter()
            .filter_map(|pair| pair.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        Some(HostStart {
            cwd,
            env,
            argv: command[term + 1..].to_vec(),
        })
    }

    /// One variable of the pane's environment, empty where it was not handed
    /// one.
    fn var(&self, key: &str) -> String {
        self.env
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }
}

impl Rig {
    /// A whole machine in a temp directory: the fleet directory, a home, a
    /// policy file naming the agent stub, and the stub's state beside it.
    fn new(name: &str) -> Rig {
        Rig::with_leaf(name, "builder-1")
    }

    fn with_leaf(name: &str, leaf: &str) -> Rig {
        let root = std::env::temp_dir().join(format!("fleet-drive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            root,
            leaf: leaf.to_string(),
            agent_timeout_ms: None,
        };
        std::fs::create_dir_all(rig.machine()).unwrap();
        std::fs::create_dir_all(rig.home()).unwrap();
        // The seat's worktree is a real directory: a start is issued IN it, and
        // a spawn whose working directory is not there fails before the child
        // runs a line — which is a defect of the rig, not of the controller.
        std::fs::create_dir_all(rig.worktree()).unwrap();
        // The host a start runs its session on: this rig's own tmux stub, which
        // every fleet the rig runs is pointed at.
        common::stub_tmux(&rig.root.join("tmux"));
        rig.write_policy(POLICY_1S);
        rig.write_config(&rig.one_seat_config(rig.policy_path()));
        rig.write_roster("[]");
        // The release the stub declares it was measured against, and the one
        // it answers with: a fleet whose agent is where its adapter expects it
        // (ruling 8), so an arm about a move makes the move itself.
        rig.set_measured(&["9.9.9"]);
        rig.set_version("9.9.9");
        rig
    }

    /// The argv a start ran, after its program and after the `session` and
    /// the seat the stub's own launch puts ahead of its flags.
    fn flags_of(&self, argv: &[String]) -> Vec<String> {
        let mut flags = argv.iter().skip(1).peekable();
        if flags.peek().map(|a| a.as_str()) == Some(agent_stub::SESSION) {
            flags.next();
            if flags.peek().is_some_and(|a| !a.starts_with("--")) {
                flags.next();
            }
        }
        flags.cloned().collect()
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
    /// The actor the controller's own lines carry: the controller, under this
    /// machine's identity — which the controller mints where there is none.
    fn controller_actor(&self) -> serde_json::Value {
        let identity = fleet_core::seat::identity::read_identity(&self.machine())
            .expect("the identity reads")
            .expect("the controller minted one");
        serde_json::json!({ "kind": "controller", "id": identity.id.to_string() })
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
    /// The effects carried out on the seat's session, in the order the host
    /// received them — read off the tmux stub's own record of every call it
    /// was run with: `start <args>` for a session started, `interrupt` for the
    /// `C-c` a stop types, `kill` for a session killed. A start or a stop is the
    /// host's alone (fleet-rge6.4), so the host's record is the whole order.
    fn calls(&self) -> Vec<String> {
        self.host()
            .invocations
            .iter()
            .filter_map(|args| {
                if let Some(start) = HostStart::of(args) {
                    return Some(format!("start {}", self.flags_of(&start.argv).join(" ")));
                }
                if args.iter().any(|arg| arg == "kill-session") {
                    return Some("kill".to_string());
                }
                (args.iter().any(|arg| arg == "send-keys") && args.iter().any(|arg| arg == "C-c"))
                    .then(|| "interrupt".to_string())
            })
            .collect()
    }

    /// Make every kill the host takes from now on answer and keep its session,
    /// or take it again where `false` — a stop that did not land, which only
    /// the host's own reading after it can tell (`FakeServer::kill_keeps`).
    fn keep_kills(&self, keep: bool) {
        let mut host = self.host();
        host.kill_keeps = keep;
        host.save(&self.tmux_state_path())
            .expect("the tmux stub's state is written");
    }

    /// The link `FLEET_TMUX_BIN` names for every fleet this rig runs: this
    /// rig's own `fleet-tmux-stub`, whose state sits beside it.
    fn tmux_link(&self) -> PathBuf {
        self.root.join("tmux").join("tmux")
    }

    fn tmux_state_path(&self) -> PathBuf {
        self.root.join("tmux").join("tmux-stub.json")
    }

    /// The tmux stub's fake server as the controller left it.
    fn host(&self) -> FakeServer {
        FakeServer::load(&self.tmux_state_path()).expect("the tmux stub's state reads")
    }

    /// Make every session the host starts from now on end at once with `code`
    /// and show the line a start that fails prints, or start them to run where
    /// `None` — the seam a start failure is driven through now that the start
    /// is a session on the host and not the stub's own exit.
    fn set_start_exit(&self, code: Option<i32>) {
        let mut host = self.host();
        host.end_every_start = code.map(Some);
        host.screen_every_start = code.map(|_| "the start spoke\n".to_string());
        host.save(&self.tmux_state_path())
            .expect("the tmux stub's state is written");
    }

    /// Every session start the host was asked for, in order, as the host
    /// received it — read off the tmux stub's own record of each
    /// `new-session`, so a start whose session was killed since still reads.
    fn host_starts(&self) -> Vec<HostStart> {
        self.host()
            .invocations
            .iter()
            .filter_map(|args| HostStart::of(args))
            .collect()
    }

    /// The last start the host was asked for.
    fn last_start(&self) -> HostStart {
        self.host_starts()
            .pop()
            .unwrap_or_else(|| panic!("no start reached the host: {:?}", self.host()))
    }

    /// WHICH FILE the last start ran, as the pane's own program.
    ///
    /// The argv beside it says what the call passed; only this says which binary
    /// received it, and the two are different questions the moment more than one
    /// program on the box answers to the same name.
    fn start_bin(&self) -> PathBuf {
        PathBuf::from(&self.last_start().argv[0])
    }

    /// The `FLEET_BIN` the last start was handed — the binary the plugin's hooks
    /// in the session it opens will run. Empty is a start that carried none.
    fn start_fleet_bin(&self) -> String {
        self.last_start().var("FLEET_BIN")
    }

    /// The `FLEET_ACTOR` the last start was handed — who the session's own
    /// bare verbs act as. Empty is a start that carried none.
    fn start_actor(&self) -> String {
        self.last_start().var("FLEET_ACTOR")
    }

    /// The seat's live pane on the tmux stub, as a start leaves one, and a
    /// listing carrying `session`'s row in the seat's worktree under the
    /// pane's pid — idle, and busy once the pane has taken a submit: a session
    /// that takes a typed turn. The agent stub reads it so by following the
    /// tmux stub's panes (`agent_stub::follow_host`).
    fn write_roster_taking(&self, session: &str) {
        let pid = common::live_pane(&self.tmux_state_path(), SEAT_ID, &self.worktree());
        let row = |status: &str| {
            format!(
                r#"[{{"sessionId":"{session}","cwd":"{}","kind":"interactive",
                      "pid":{pid},"status":"{status}","startedAt":1000}}]"#,
                self.worktree().display()
            )
        };
        agent_stub::follow_host(&self.root, Some(&self.tmux_state_path()));
        self.write_roster(&row("idle"));
    }

    /// Every text typed into the seat's session on the tmux stub, in order.
    fn typed(&self) -> Vec<String> {
        common::pasted_into(&self.tmux_state_path(), SEAT_ID)
    }

    /// The argv the last start's pane runs, after its program ([`Rig::flags_of`]).
    fn start_argv(&self) -> Vec<String> {
        self.flags_of(&self.last_start().argv)
    }

    /// The directory the start was issued in, CANONICAL — the temp directory is
    /// a symlink, so both sides of a comparison go in one form.
    fn start_cwd(&self) -> PathBuf {
        let cwd = self.last_start().cwd;
        std::fs::canonicalize(&cwd).unwrap_or_else(|_| PathBuf::from(cwd))
    }

    /// The `PATH` the last start's pane was handed.
    fn start_path(&self) -> String {
        self.last_start().var("PATH")
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

    fn stderr_path(&self) -> PathBuf {
        self.root.join("controller.err")
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
    ///
    /// The body is followed by the `[agent]` table naming the agent stub, so
    /// every policy an arm writes runs the fleet on the stub — a body that
    /// does not parse still does not.
    fn write_policy(&self, body: &str) {
        let path = self.policy_path();
        let before = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());
        let body = format!(
            "{body}\n[agent]\nadapter = {}\n",
            serde_json::to_string(&common::agent_stub_path().display().to_string())
                .expect("a path is JSON text")
        );
        write(&path, &body);
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
    /// The listing the stub serves: the arm's rows, then the rows a start's
    /// watch believes the tmux stub's panes by (`test_support::with_arrivals`),
    /// which stand in no seat's worktree.
    ///
    /// And the HOST beside it (ruling 3): a live row the arm lists in the
    /// seat's worktree is the seat's session only where the host holds a live
    /// pane for the seat with that pid, so the pane is put there — see
    /// [`Rig::host_the_listed_seat`].
    fn write_roster(&self, body: &str) {
        let listing = fleet_controller::test_support::with_arrivals(body);
        agent_stub::script(&self.root, |a| a.listing = Ok(listing));
        self.host_the_listed_seat(body);
    }

    /// The seat's session on the rig's tmux stub, standing for the live row an
    /// arm lists in the seat's worktree: a pane alive under the seat's session
    /// name, carrying the row's pid, which is what attributes the row to the
    /// seat (the claude-code pack's lessons B10). A pane a start already left
    /// there takes the row's pid rather than a second pane being made.
    ///
    /// A listing with NO live row in the worktree takes away a pane the rig
    /// planted — the session went, and its row with it — and leaves one a start
    /// made (its pid is the stub's own, from [`FIRST_PANE_PID`] up), which the
    /// controller's own start owns.
    fn host_the_listed_seat(&self, body: &str) {
        let Ok(mut host) = FakeServer::load(&self.tmux_state_path()) else {
            return;
        };
        let rows: Vec<serde_json::Value> = serde_json::from_str(body).unwrap_or_default();
        let worktree = self.worktree().display().to_string();
        let listed = rows.iter().find_map(|row| {
            let cwd = row["cwd"].as_str()?;
            (cwd.trim_end_matches('/') == worktree.trim_end_matches('/'))
                .then(|| row["pid"].as_u64())
                .flatten()
        });
        match listed {
            Some(pid) => {
                let now = now_ms();
                let pane = host
                    .sessions
                    .entry(SEAT_ID.to_string())
                    .or_insert_with(|| FakeSession {
                        cwd: worktree.clone(),
                        argv: Vec::new(),
                        env: Vec::new(),
                        pid: 0,
                        ended: None,
                        created_ms: now - now % 1000 - 60_000,
                        screen: String::new(),
                        sent: Vec::new(),
                    });
                pane.pid = pid as u32;
                pane.ended = None;
            }
            None => {
                let planted = host
                    .sessions
                    .get(SEAT_ID)
                    .is_some_and(|pane| pane.pid < FIRST_PANE_PID && pane.ended.is_none());
                if planted {
                    host.sessions.remove(SEAT_ID);
                }
            }
        }
        host.save(&self.tmux_state_path())
            .expect("the tmux stub's state is written");
    }

    /// A session of the seat's that this controller started, sighted and then
    /// ENDED: its row on the session table carrying the session id a sighting
    /// wrote, dispatched long enough ago that the arrival
    /// window is closed, and its pane on the host dead with `status`. The
    /// listing names nothing — an interactive row leaves with its process (the
    /// claude-code pack's lessons B10) — so the table is where the loop reads the
    /// session from, as a real poll after a real end does.
    fn a_dead_session(&self, session: &str, status: Option<i32>) {
        write(
            &self.machine().join("sessions.json"),
            &serde_json::json!({
                "schema": 2,
                "sessions": [{
                    "seat": SEAT_ID,
                    "project": "demo",
                    "worktree": self.worktree().display().to_string(),
                    "name": SEAT,
                    "model": StubAgent::MODEL,
                    "posture": "auto",
                    "first_turn": format!("/wake {SEAT}"),
                    "transient": false,
                    "dispatch_id": "an-earlier-dispatch",
                    "dispatched_at": 1000,
                    "session_id": session,
                    "first_seen_at": 1000,
                    "last_seen_at": 1000,
                }],
            })
            .to_string(),
        );
        self.end_the_seat(status);
    }

    /// The seat HELD DOWN on the session table — its halt latch set at the
    /// blind limit, as three blind dispatches leave it — so a poll decides
    /// nothing for it until a clear-halt.
    fn hold_the_seat(&self) {
        let path = self.machine().join("sessions.json");
        let mut table: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("the session table is written"),
        )
        .expect("the session table parses");
        table["seats"] = serde_json::json!({ SEAT_ID: { "blind": 3, "halted": true } });
        write(&path, &table.to_string());
    }

    /// The seat's pane on the host ended with `status`, kept dead as
    /// remain-on-exit keeps it: the end of a seat's session, as the host reads
    /// it (ruling 3). The pane is made first where the host holds none.
    fn end_the_seat(&self, status: Option<i32>) {
        let mut host = self.host();
        let now = now_ms();
        let pane = host
            .sessions
            .entry(SEAT_ID.to_string())
            .or_insert_with(|| FakeSession {
                cwd: self.worktree().display().to_string(),
                argv: Vec::new(),
                env: Vec::new(),
                pid: 4242,
                ended: None,
                created_ms: now - now % 1000 - 60_000,
                screen: String::new(),
                sent: Vec::new(),
            });
        pane.ended = Some(status);
        host.save(&self.tmux_state_path())
            .expect("the tmux stub's state is written");
    }

    /// The log every session reads as, last written now: the stub keeps one
    /// for all of them, which is every arm's one session.
    fn write_transcript(&self, _session: &str, body: &str) {
        agent_stub::script(&self.root, |a| {
            a.session_log = Some(body.to_string());
            a.last_write = Some(now_ms());
        });
    }

    /// The same transcript, with its last write placed in the past.
    ///
    /// The controller reads a session's END off this mtime, so an arm about a
    /// session that finished hours ago has to age the FILE and not only the
    /// row's start stamp — the two answer different questions and that is the
    /// whole subject of the window.
    fn age_transcript(&self, _session: &str, ms_ago: u64) {
        agent_stub::script(&self.root, |a| a.last_write = Some(now_ms() - ms_ago));
    }

    /// What the agent's version reports from now on. `FAIL` makes the read fail
    /// — a version call answered could not tell — and `SILENT` answers with a
    /// null version, the adapter's word for no agent installed.
    fn set_version(&self, version: &str) {
        let failing = (version == "FAIL").then_some("the version call failed");
        agent_stub::untold(&self.root, StubAgent::VERSION_CALL, failing);
        agent_stub::script(&self.root, |a| {
            a.version = match version {
                "FAIL" | "SILENT" => None,
                version => Some(version.to_string()),
            }
        });
    }

    /// The fleet's file with `[agent] adapter` naming `adapter` in place of the
    /// agent stub: the policy the rig writes, re-written with its last table
    /// swapped. For an arm whose adapter has to behave as the stub cannot.
    fn name_the_agent(&self, adapter: &Path) {
        let path = self.policy_path();
        let body = std::fs::read_to_string(&path).expect("the rig wrote its policy");
        let stub = serde_json::to_string(&common::agent_stub_path().display().to_string())
            .expect("a path is JSON text");
        let named = serde_json::to_string(&adapter.display().to_string())
            .expect("a path is JSON text");
        let written = format!("[agent]\nadapter = {stub}\n");
        assert!(body.contains(&written), "the policy names the stub: {body}");
        write(&path, &body.replace(&written, &format!("[agent]\nadapter = {named}\n")));
    }

    /// An adapter that is the agent stub, and sleeps `seconds` before it is
    /// the stub for each verb in `slow` — so the calls of those verbs outrun a
    /// deadline shorter than the sleep, and every other verb answers at once.
    /// `capabilities` is never among them in an arm that polls: a loop whose
    /// agent declares nothing does not start.
    ///
    /// WARMED BEFORE IT IS NAMED: a script written a moment ago can wait on
    /// this platform's first-exec assessment for longer than any deadline an
    /// arm sets, so it is run once here, on a bound of its own, and the loop
    /// only ever meets it warm. `sleep` and the stub it execs are warm already.
    fn slow_adapter(&self, slow: &[&str], seconds: u64) -> PathBuf {
        let path = self.root.join("slow-agent");
        write(
            &path,
            &format!(
                "#!/bin/sh\n\
                 case \"$1\" in {}) sleep {seconds} ;; esac\n\
                 exec '{}' \"$@\"\n",
                slow.join("|"),
                common::agent_stub_path().display()
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let warmed = Command::new(&path)
            .arg("warm")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("the slow adapter runs");
        assert!(
            warmed.code().is_some(),
            "the slow adapter exits on its own when warmed"
        );
        path
    }

    /// The releases the stub declares its agent was measured against from now
    /// on (ruling 8): what the loop holds the live version to, and what the
    /// projection's `expected` is read off.
    fn set_measured(&self, releases: &[&str]) {
        agent_stub::script(&self.root, |a| {
            a.capabilities.measured = releases.iter().map(|r| r.to_string()).collect()
        });
    }

    /// The built binary's `observe`, with this rig's environment.
    fn command(&self) -> Command {
        let mut cmd = self.binary();
        cmd.arg("observe");
        cmd
    }

    /// The built binary with this rig's environment and no subcommand — what
    /// the `fleet event` arms run.
    fn binary(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fleet"));
        use common::hermetic::Hermetic as _;
        cmd.hermetic(&self.home(), &self.machine())
            .env(common::hermetic::TMUX_BIN, self.tmux_link())
            // The routines' clock seam is always pointed at this rig's own file.
            // An arm that never writes it leaves the routines on the machine's
            // clock, which is what every arm that is not about routines wants.
            .env("FLEET_ORDERS_CLOCK", self.clock_path())
            .env_remove("FLEET_AGENT_TIMEOUT_MS");
        if let Some(ms) = self.agent_timeout_ms {
            cmd.env("FLEET_AGENT_TIMEOUT_MS", ms.to_string());
        }
        cmd
    }

    /// One poll, in this process.
    fn observe(&self) -> Output {
        self.poll_in_process(&[])
    }

    /// One poll through the BUILT BINARY, for an arm whose subject is WHICH
    /// EXECUTABLE IS RUNNING: a routine's children are handed the running
    /// binary's own directory in front of the constructed path —
    /// `routines::action::path_for_children`, over `current_exe` — and in this
    /// process that directory holds the test binary and no `fleet`.
    fn observe_out_of_process(&self) -> Output {
        self.command()
            .arg("--once")
            .output()
            .expect("the built binary runs")
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
    /// `observe` subcommand reaches after its argument parsing: it opens the
    /// agent this fleet's file names, reads its effects gate off the agent's
    /// own answers, and ticks. Calling it here is the same loop over the same
    /// fixture with one process fewer between the arm and it.
    ///
    /// THE ONE THING THE BINARY ASKS FOR AND THIS DOES NOT: the process's
    /// SIGINT and SIGTERM handlers. They belong to the whole binary, and a test
    /// process holding them answers the harness's SIGTERM by living on.
    ///
    /// TWO THINGS THE ARM DOES NOT GET, and none is any arm's subject. The two
    /// lines `main` adds to a refused startup — its own error chain — are
    /// absent, while the loop's own line naming the path it could not read is
    /// what the two refusal arms assert on.
    ///
    /// The status is the loop's own `u8` dressed as a wait status, which is the
    /// same number the binary's exit table maps it to for the two the loop can
    /// answer: `0` and `EXIT_NO_POLICY`.
    fn poll_in_process(&self, overrides: &[(&str, Option<&OsStr>)]) -> Output {
        let _held = IN_PROCESS.lock().unwrap_or_else(|p| p.into_inner());

        let mut env = EnvHeld::new();
        for (key, value) in common::hermetic::vars(&self.home(), &self.machine()) {
            env.set(key, value);
        }
        env.set(
            common::hermetic::TMUX_BIN,
            Some(self.tmux_link().into_os_string()),
        );
        // The routines' clock seam is always pointed at this rig's own file. An
        // arm that never writes it leaves the routines on the machine's clock,
        // which is what every arm that is not about routines wants.
        env.set(
            "FLEET_ORDERS_CLOCK",
            Some(self.clock_path().into_os_string()),
        );
        // The rig's own tmux stub, as the binary routes are given it: the loop
        // reads every seat's presence off the host (ruling 3), so a loop with
        // no host reads every seat unknown.
        env.set(
            common::hermetic::TMUX_BIN,
            Some(self.tmux_link().into_os_string()),
        );
        env.set(
            "FLEET_AGENT_TIMEOUT_MS",
            self.agent_timeout_ms
                .map(|ms| OsString::from(ms.to_string())),
        );
        for (key, value) in overrides {
            env.set(key, value.map(OsStr::to_os_string));
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
        let engine = fleet_controller::runs::Engine::on(self.machine());
        let status = run::observe_seamed(
            &run::Options { once: true },
            fleet_controller::platform::Grant::new(
                fleet_controller::platform::directory_listing(),
                fleet_controller::platform::GRANT_PROBE_TIMEOUT,
            ),
            Some(&engine as &dyn fleet_controller::runs::Runs),
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

        let mut env = EnvHeld::new();
        for (key, value) in common::hermetic::vars(&self.home(), &self.machine()) {
            env.set(key, value);
        }
        env.set(
            "FLEET_ORDERS_CLOCK",
            Some(self.clock_path().into_os_string()),
        );
        // The rig's own tmux stub, as the binary routes are given it: the loop
        // reads every seat's presence off the host (ruling 3), so a loop with
        // no host reads every seat unknown.
        env.set(
            common::hermetic::TMUX_BIN,
            Some(self.tmux_link().into_os_string()),
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
        let engine = fleet_controller::runs::Engine::on(self.machine());
        let started = {
            let _out = Redirected::appending(1, self.root.join("in-process.out"));
            let _err = Redirected::appending(2, self.stderr_path());
            run::Observer::start(
                fleet_controller::platform::Grant::new(
                    fleet_controller::platform::directory_listing(),
                    fleet_controller::platform::GRANT_PROBE_TIMEOUT,
                ),
                Some(&engine as &dyn fleet_controller::runs::Runs),
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
    /// the bound, and prints the elapsed to stderr on success so the ceiling is
    /// re-measurable from a run.
    ///
    /// THE BOUND IS AN ANTI-HANG CEILING AND NEVER A READING: it ends the moment
    /// the document says so, and it exists only so a controller that never
    /// publishes fails the arm instead of hanging the lane.
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
/// unique among them. `fleet_core::fs::write_atomic` is the same shape and is
/// read for it and not reused: the suite does not reach into the crate's own
/// helpers for a fixture write.
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

/// The window `settled_toward_zero` gives a nonzero count to become zero, and
/// the unit any arm holding a child open across that window states its own hold
/// against. Named so a slice that moves the settle moves every hold that has to
/// outlive it: a literal at those sites is a coupling the compiler cannot keep.
const SETTLE_WINDOW: Duration = Duration::from_secs(3);

const HOUR_MS: u64 = 60 * 60 * 1000;

const POLICY_1S: &str = "[controller]\npoll_seconds = 1\n";

fn live_row(cwd: &Path, session: &str) -> String {
    format!(
        r#"[{{"sessionId":"{session}","cwd":"{}","kind":"interactive",
              "pid":4242,"status":"idle","startedAt":1000}}]"#,
        cwd.display()
    )
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
        r#"[{{"sessionId":"{session}","cwd":"{}","kind":"interactive",
              "pid":4242,"status":"idle","startedAt":1000,"waitingFor":"{cause}"}}]"#,
        cwd.display()
    )
}

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
