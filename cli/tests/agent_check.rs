//! `fleet agent check` through the shipped binary: the agent contract's
//! conformance suite run against an adapter, in a temp dir the verb makes and
//! removes, answered in the exit table.
//!
//! THE ADAPTER IS `fleet-agent-stub`, this crate's example build of the
//! controller's test-support executable, and its recorded cases are the ones
//! under `controller/tests/fixtures/agent-stub/`, in the layout an adapter ships
//! beside its `adapter.toml`. The live arm runs it on a real tmux, and is
//! skipped where there is none.
//!
//! EACH ARM POINTS `TMPDIR` AT A DIRECTORY OF ITS OWN, so the temp dir the verb
//! makes is looked for where it was made — and each arm asserts nothing of it
//! is left there, whichever row of the table the run answered.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use fleet_controller::adapter::conformance::CHECKS;

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The prefix of the temp dir the verb makes, under its `TMPDIR`.
const MADE: &str = "fleet-agent-check-";

/// The stub's own recorded cases.
fn stub_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../controller/tests/fixtures/agent-stub")
}

/// A home, a machine directory, a working directory that is no project and a
/// `TMPDIR`, all under one root the arm owns and removes.
struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-agent-check-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig { root };
        for dir in [rig.work(), rig.tmp(), rig.root.join("machine")] {
            std::fs::create_dir_all(&dir).expect("the rig's directories are made");
        }
        rig
    }

    fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// `fleet agent check` with `args` and `env` after the hermetic block, run
    /// from a directory that is no project.
    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fleet"));
        cmd.args(["agent", "check"])
            .args(args)
            .current_dir(self.work())
            .hermetic(&self.root.join("home"), &self.root.join("machine"))
            .env("TMPDIR", self.tmp());
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd
    }

    fn check(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.command(args, env)
            .output()
            .expect("the built binary runs")
    }

    /// What the verb left under its `TMPDIR` of the dir it makes: nothing,
    /// whatever it answered.
    fn left(&self) -> Vec<String> {
        std::fs::read_dir(self.tmp())
            .expect("the rig's TMPDIR is readable")
            .map(|entry| entry.expect("an entry reads").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| name.starts_with(MADE))
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stub() -> String {
    common::agent_stub_path().display().to_string()
}

/// The run's lines read back: one per check, by its word, and the summary's
/// three counts.
struct Read {
    pass: Vec<String>,
    skip: Vec<String>,
    fail: Vec<String>,
    counted: (usize, usize, usize),
}

impl Read {
    /// The line of the check `name`, whichever word it carries.
    fn line(&self, name: &str) -> String {
        self.pass
            .iter()
            .chain(&self.skip)
            .chain(&self.fail)
            .find(|line| *line == name || line.starts_with(&format!("{name}: ")))
            .cloned()
            .unwrap_or_else(|| panic!("no line for `{name}`"))
    }

    fn passed(&self, name: &str) -> bool {
        self.pass.iter().any(|line| line == name)
    }

    fn failed(&self, name: &str) -> Option<&String> {
        self.fail
            .iter()
            .find(|line| line.starts_with(&format!("{name}: ")))
    }
}

/// Every line of stdout is a check's or the summary, which is the last and
/// opens `agent check: <named> — `; the checks come in [`CHECKS`]' order.
fn read(out: &Output, named: &str) -> Read {
    let text = stdout(out);
    let mut lines: Vec<&str> = text.lines().collect();
    let summary = lines.pop().expect("the run printed a summary").to_string();
    let opens = format!("agent check: {named} — ");
    let counts = summary
        .strip_prefix(&opens)
        .unwrap_or_else(|| panic!("the summary opens `{opens}`: {summary}"));
    let numbers: Vec<usize> = counts
        .split(", ")
        .zip([" passed", " failed", " skipped"])
        .map(|(part, word)| {
            part.strip_suffix(word)
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("`{part}` is a count{word}: {summary}"))
        })
        .collect();
    assert_eq!(numbers.len(), 3, "three counts: {summary}");
    let mut read = Read {
        pass: Vec::new(),
        skip: Vec::new(),
        fail: Vec::new(),
        counted: (numbers[0], numbers[1], numbers[2]),
    };
    let mut names = Vec::new();
    for line in lines {
        let rest = if let Some(name) = line.strip_prefix("PASS  ") {
            read.pass.push(name.to_string());
            name
        } else if let Some(rest) = line.strip_prefix("SKIP  ") {
            read.skip.push(rest.to_string());
            rest
        } else if let Some(rest) = line.strip_prefix("FAIL  ") {
            read.fail.push(rest.to_string());
            rest
        } else {
            panic!("a line that is no check's: `{line}` in\n{text}");
        };
        names.push(
            CHECKS
                .iter()
                .map(|(name, _)| *name)
                .find(|name| rest == *name || rest.starts_with(&format!("{name}: ")))
                .unwrap_or_else(|| panic!("`{line}` names no check")),
        );
    }
    assert_eq!(
        names,
        CHECKS.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        "one line per check, in the table's order:\n{text}"
    );
    assert_eq!(
        read.counted,
        (read.pass.len(), read.fail.len(), read.skip.len()),
        "the summary counts the lines above it:\n{text}"
    );
    read
}

/// Every file under `dir` with its bytes, to hold a fixtures dir to what it
/// was before a run.
fn contents(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).expect("the fixtures read") {
            let path = entry.expect("an entry reads").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("a fixture reads");
                found.push((path, bytes));
            }
        }
    }
    found.sort();
    found
}

/// The offline checks the stub passes whatever it is handed: everything but
/// the fixtures and the live steps.
const OFFLINE: [&str; 8] = [
    "version",
    "capabilities",
    "each declared posture launches",
    "resume of a session nobody has",
    "read of no seats",
    "an unknown verb exits 2",
    "a later schema_version exits 2",
    "read answers each fixture",
];

/// The five live steps, in order.
const LIVE: [&str; 5] = [
    "live: a launched session comes up idle",
    "live: a typed turn reads busy, then idle",
    "live: context counts the turn",
    "live: the ended session leaves its pane dead",
    "live: a resume comes back idle as the same session",
];

/// Arm 1 (the acceptance). Every offline check passes on the agent stub with
/// its own fixtures: only PASS and SKIP lines, the live steps skipped because
/// `--live` was not given and the one posture check skipped because the stub
/// declares all three, a summary naming `stub`, exit 0 — and nothing left of
/// the temp dir, and not a byte of the fixtures moved.
#[test]
fn every_offline_check_passes_on_the_agent_stub_with_its_own_fixtures() {
    let rig = Rig::new("offline");
    let fixtures = stub_fixtures();
    let before = contents(&fixtures);
    let started = Instant::now();
    let out = rig.check(
        &[
            "--adapter",
            &stub(),
            "--fixtures",
            &fixtures.display().to_string(),
        ],
        &[],
    );
    let took = started.elapsed();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let read = read(&out, "stub");
    assert!(read.fail.is_empty(), "no check failed: {:?}", read.fail);
    for name in OFFLINE
        .iter()
        .chain(["context answers each fixture"].iter())
    {
        assert!(read.passed(name), "`{name}` passed:\n{}", stdout(&out));
    }
    assert!(
        read.line("an undeclared posture is refused unsupported")
            .contains("every posture is declared"),
        "{}",
        stdout(&out)
    );
    for name in LIVE {
        assert!(
            read.line(name).contains("--live"),
            "`{name}` is skipped for want of --live:\n{}",
            stdout(&out)
        );
    }
    assert_eq!(read.counted, (9, 0, 6), "{}", stdout(&out));
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
    assert_eq!(contents(&fixtures), before, "the fixtures are as they were");
    eprintln!("the offline check took {took:?}");
}

/// Arm 2. A launch that writes outside its configuration directory and its
/// worktree fails the launch check, naming the path it wrote; one that writes
/// inside its configuration directory passes it (E8).
#[test]
fn a_launch_writing_outside_its_config_dir_fails_naming_the_path() {
    let rig = Rig::new("stray");
    let out = rig.check(
        &["--adapter", &stub()],
        &[("FLEET_AGENT_STUB_WRITE", "../stray-from-launch")],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let read = read(&out, "stub");
    let failed = read
        .failed("each declared posture launches")
        .unwrap_or_else(|| panic!("the launch check failed:\n{}", stdout(&out)));
    assert!(
        failed.contains("stray-from-launch") && failed.contains("outside"),
        "the failure names the path and where it is: {failed}"
    );
    assert_eq!(
        read.fail.len(),
        1,
        "only the launch check failed: {:?}",
        read.fail
    );
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );

    let inside = rig.check(
        &["--adapter", &stub()],
        &[("FLEET_AGENT_STUB_WRITE", "seeded-by-launch")],
    );
    assert_eq!(inside.status.code(), Some(0), "{}", stdout(&inside));
    assert!(read_passes(&inside, "each declared posture launches"));
}

fn read_passes(out: &Output, name: &str) -> bool {
    read(out, "stub").passed(name)
}

/// Arm 3. A relative `--adapter` is neither form `[agent] adapter` takes:
/// exit 2, the sentence saying so, no check line, nothing made.
#[test]
fn a_relative_adapter_is_usage_and_nothing_runs() {
    let rig = Rig::new("relative");
    let out = rig.check(&["--adapter", "target/debug/fleet-agent-stub"], &[]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "fleet agent check: --adapter takes an absolute path to an executable or the name of \
         an agent adapter an installed pack carries, and `target/debug/fleet-agent-stub` is \
         neither\n"
    );
    assert_eq!(stdout(&out), "", "no check line was printed");
    assert!(rig.left().is_empty(), "nothing was made: {:?}", rig.left());
}

/// Arm 4. `--fixtures` naming no directory is usage too.
#[test]
fn fixtures_naming_no_directory_is_usage() {
    let rig = Rig::new("no-fixtures-dir");
    let out = rig.check(&["--adapter", &stub(), "--fixtures", "nowhere"], &[]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "fleet agent check: --fixtures names `nowhere`, which is not a directory\n"
    );
    assert_eq!(stdout(&out), "");
}

/// Arm 5. An adapter that ships no fixtures beside an `adapter.toml`, run with
/// no `--fixtures`, has the two fixture checks skipped, saying why — and
/// passes.
#[test]
fn an_adapter_with_no_fixtures_has_the_fixture_checks_skipped() {
    let rig = Rig::new("unfixtured");
    let out = rig.check(&["--adapter", &stub()], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let read = read(&out, "stub");
    for name in ["read answers each fixture", "context answers each fixture"] {
        assert!(
            read.line(name).contains("no fixtures"),
            "`{name}` is skipped for want of fixtures:\n{}",
            stdout(&out)
        );
    }
}

/// Arm 6. A recorded case the adapter answers otherwise fails its check,
/// naming the case and the first field where the two part.
#[test]
fn a_case_answered_otherwise_fails_naming_the_case_and_the_field() {
    let rig = Rig::new("otherwise");
    let copy = rig.root.join("fixtures");
    for (path, bytes) in contents(&stub_fixtures()) {
        let to = copy.join(
            path.strip_prefix(stub_fixtures())
                .expect("under the fixtures"),
        );
        std::fs::create_dir_all(to.parent().expect("a file sits in a dir")).expect("made");
        std::fs::write(&to, bytes).expect("copied");
    }
    let answer = copy.join("read/idle-by-pid/answer.json");
    let text = std::fs::read_to_string(&answer).expect("the answer reads");
    std::fs::write(&answer, text.replacen("\"idle\"", "\"busy\"", 1)).expect("rewritten");

    let out = rig.check(
        &[
            "--adapter",
            &stub(),
            "--fixtures",
            &copy.display().to_string(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let read = read(&out, "stub");
    let failed = read
        .failed("read answers each fixture")
        .unwrap_or_else(|| panic!("the read fixtures failed:\n{}", stdout(&out)));
    assert!(
        failed.contains("idle-by-pid") && failed.contains("seats[0].activity"),
        "the failure names the case and the field: {failed}"
    );
    assert!(
        !failed.contains("blocked-on-permission"),
        "the case that matches is not named: {failed}"
    );
}

/// The tmux on this box's `PATH`, where there is one.
fn real_tmux() -> Option<PathBuf> {
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("tmux"))
        .find(|tmux| tmux.is_file())
}

/// Arm 7 (the live acceptance). `--live` against the stub's own session on a
/// real tmux: every live step passes — up idle, a typed turn busy then idle,
/// context counting it, the pane dead after the session ends, a resume back
/// idle as the same session — the summary names the stub's version, and no
/// `fleet-check-<pid>` server is left. Skipped where the box has no tmux.
#[test]
fn every_live_step_passes_on_the_stubs_session_on_a_real_tmux() {
    let Some(tmux) = real_tmux() else {
        eprintln!("skipped: no tmux on PATH");
        return;
    };
    let rig = Rig::new("live");
    let child = rig
        .command(
            &["--adapter", &stub(), "--live"],
            &[("FLEET_TMUX_BIN", &tmux.display().to_string())],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built binary runs");
    let pid = child.id();
    let out = child.wait_with_output().expect("the run ends");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let read = read(&out, "stub 0.0.0-stub");
    for name in LIVE {
        assert!(read.passed(name), "`{name}` passed:\n{}", stdout(&out));
    }
    let listed = Command::new(&tmux)
        .env_clear()
        .args(["-L", &format!("fleet-check-{pid}"), "ls"])
        .output()
        .expect("tmux runs");
    assert!(
        !listed.status.success(),
        "no fleet-check-{pid} server is left: {}",
        String::from_utf8_lossy(&listed.stdout)
    );
    // Nor its socket, which tmux leaves behind however its server ends.
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata(&rig.root)
        .expect("the rig is there")
        .uid();
    let socket = PathBuf::from(format!("/tmp/tmux-{uid}/fleet-check-{pid}"));
    assert!(!socket.exists(), "{} is removed", socket.display());
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}
