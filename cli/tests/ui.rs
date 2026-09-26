//! The ui module's policy seen from outside the process.
//!
//! What a pipe gets is asserted by every other file in this directory, which
//! reads the verbs' exact plain text. What is asserted here is the half a pipe
//! cannot show: on a pseudo-terminal the status lines carry escape sequences,
//! `NO_COLOR` and a dumb `TERM` take them away again, and the spinner appears
//! only once the wait has already outlasted the module's two-second threshold.
//!
//! The terminal is a real one, allocated by `script`: nothing here mocks a tty,
//! because the reading being taken IS whether the process believes it has one.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The escape byte every styled line starts with.
const ESC: char = '\u{1b}';

/// The terminal these arms state for themselves rather than inherit.
///
/// A pseudo-terminal is not enough on its own: `indicatif` hands back a HIDDEN
/// draw target whenever `TERM` is unset or `dumb` (`ProgressDrawTarget::term`,
/// over `console::is_dumb`, whose Unix default for an unset `TERM` is dumb), so
/// a process whose environment carries no `TERM` draws neither spinner nor bar
/// on a real tty. The environments this suite is run under include cleared
/// ones — a workflow's gate builds its suite one — so an arm about what a
/// terminal shows names the terminal it needs instead of borrowing whatever its
/// parent happened to hold.
const TERM: &str = "xterm-256color";

/// `script` with the child's stdout and stderr on a pseudo-terminal. The two
/// spellings are not interchangeable: BSD takes the command as its own
/// arguments, util-linux takes one `-c` string and names the typescript last.
#[cfg(target_os = "macos")]
fn pty_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new("script");
    command.arg("-q").arg("/dev/null").arg(program).args(args);
    command.env("TERM", TERM);
    command
}

#[cfg(not(target_os = "macos"))]
fn pty_command(program: &str, args: &[&str]) -> Command {
    let mut line = quoted(program);
    for arg in args {
        line.push(' ');
        line.push_str(&quoted(arg));
    }
    let mut command = Command::new("script");
    command
        .arg("-q")
        .arg("-e")
        .arg("-c")
        .arg(line)
        .arg("/dev/null")
        .env("TERM", TERM);
    command
}

#[cfg(not(target_os = "macos"))]
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

fn on_a_pty(args: &[&str], env: &[(&str, &str)]) -> String {
    let mut command = pty_command(env!("CARGO_BIN_EXE_fleet"), args);
    command.hermetic_nowhere();
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command
        .output()
        .expect("`script` allocates the pseudo-terminal these arms are about");
    // The two streams are one on a terminal, which is the point: what a person
    // sees is this.
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn on_a_pipe(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(args)
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs")
}

/// The fixture shaped as the doctrine pack, which is what these arms hand
/// `pack check`: a whole pack whose report has a line per slot and no runtime
/// row.
fn a_whole_pack() -> PathBuf {
    fleet_core::test_support::fixture_pack("tiny")
}

struct Temp {
    root: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Temp {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-ui-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the temp root is created");
        Temp { root }
    }

    fn arg(&self, relative: &str) -> String {
        self.root.join(relative).to_string_lossy().into_owned()
    }

    /// The bottom layer `add` lays its candidate over, materialized as `fleet
    /// create` and `fleet start` materialize it: the machine this rig models is
    /// one that has started, which is every machine a person adds a pack on.
    fn defaults(&self) -> &Temp {
        let root = self.root.join(fleet_core::defaults::DIR);
        std::fs::create_dir_all(&root).expect("the defaults dir is created");
        fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
        self
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A local repository holding one pack, committed and tagged `v1`.
fn a_pack_repo(label: &str) -> Temp {
    let repo = Temp::new(label);
    std::fs::write(
        repo.root.join("pack.toml"),
        "[pack]\nname = \"neighborly\"\nversion = \"0.1.0\"\nschema = 3\n",
    )
    .expect("the manifest is written");
    std::fs::create_dir_all(repo.root.join("skills/greet")).expect("the skill directory is made");
    std::fs::write(repo.root.join("skills/greet/SKILL.md"), "# greet\n")
        .expect("the skill is written");

    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["add", "--", "pack.toml", "skills/greet/SKILL.md"],
        vec!["commit", "--quiet", "--no-gpg-sign", "-m", "the pack"],
        vec!["tag", "v1"],
    ] {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo.root)
            .args(&args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    repo
}

#[test]
fn a_status_line_on_a_terminal_carries_escape_sequences() {
    let page = on_a_pty(
        &["pack", "check", a_whole_pack().to_str().expect("utf-8")],
        &[],
    );
    assert!(page.contains(ESC), "no styling on a terminal: {page:?}");
    assert!(
        page.contains("\u{1b}[32m"),
        "the done tone is green: {page:?}"
    );
    assert!(
        page.contains("pack") && page.contains("tiny 0.1.0"),
        "the text survives the styling: {page:?}"
    );
}

/// The control the arm above needs: the same invocation on a pipe carries no
/// escape sequence at all, so what was measured is the terminal and not a
/// module that always styles.
#[test]
fn the_same_invocation_on_a_pipe_carries_none() {
    let out = on_a_pipe(&["pack", "check", a_whole_pack().to_str().expect("utf-8")]);
    let page = String::from_utf8_lossy(&out.stdout);
    assert!(!page.contains(ESC), "a pipe is plain: {page:?}");
    assert!(page.contains("pack tiny 0.1.0 — schema 3"), "{page:?}");
}

/// `NO_COLOR` and a dumb `TERM` each take the palette away on a terminal that
/// would otherwise have had one.
#[test]
fn no_color_and_a_dumb_term_are_plain_on_a_terminal() {
    for env in [[("NO_COLOR", "1")], [("TERM", "dumb")]] {
        let page = on_a_pty(
            &["pack", "check", a_whole_pack().to_str().expect("utf-8")],
            &env,
        );
        assert!(!page.contains(ESC), "{env:?} is plain: {page:?}");
        assert!(
            page.contains("pack tiny 0.1.0 — schema 3"),
            "{env:?}: {page:?}"
        );
    }
}

/// A verb that finishes inside the threshold shows nothing: the spinner's own
/// message is the string it would have drawn.
#[test]
fn a_verb_that_finishes_fast_draws_no_spinner() {
    let repo = a_pack_repo("fast-repo");
    let machine = Temp::new("fast-machine");
    machine.defaults();
    let page = on_a_pty(
        &[
            "pack",
            "add",
            repo.root.to_str().expect("utf-8"),
            "--version",
            "v1",
            "--packs-dir",
            &machine.arg("packs"),
            "--lock",
            &machine.arg("packs.lock"),
        ],
        &[],
    );
    assert!(
        // The verb is styled and its subject is not, so the subject is what a
        // terminal reading matches on.
        page.contains("neighborly v1 at"),
        "the verb ran: {page:?}"
    );
    assert!(
        !page.contains("fetching"),
        "a wait under the threshold draws nothing: {page:?}"
    );
}

/// The other half, and the only arm that makes the threshold a rule rather
/// than a silence: the same verb over a clone slower than the threshold draws
/// the spinner. The wait is made by a `git` first on the child's PATH — the
/// name `fleet_core::add` resolves — and not by a seam the test sets.
#[test]
fn a_wait_past_the_threshold_draws_the_spinner() {
    let repo = a_pack_repo("slow-repo");
    let machine = Temp::new("slow-machine");
    machine.defaults();
    let shim = Temp::new("slow-git");
    let real = real_git();
    std::fs::write(
        shim.root.join("git"),
        format!(
            "#!/bin/sh\nfor word in \"$@\"; do\n  if [ \"$word\" = clone ]; then sleep 4; break; fi\ndone\nexec {real} \"$@\"\n"
        ),
    )
    .expect("the shim is written");
    make_executable(&shim.root.join("git"));

    let path = format!(
        "{}:{}",
        shim.root.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let page = on_a_pty(
        &[
            "pack",
            "add",
            repo.root.to_str().expect("utf-8"),
            "--version",
            "v1",
            "--packs-dir",
            &machine.arg("packs"),
            "--lock",
            &machine.arg("packs.lock"),
        ],
        &[("PATH", &path)],
    );
    assert!(
        // The verb is styled and its subject is not, so the subject is what a
        // terminal reading matches on.
        page.contains("neighborly v1 at"),
        "the slow clone still succeeded: {page:?}"
    );
    assert!(
        page.contains("fetching"),
        "a wait past the threshold is drawn: {page:?}"
    );
}

fn real_git() -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg("command -v git")
        .output()
        .expect("the shell answers");
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(!path.is_empty(), "git is on this box's PATH");
    path
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .expect("the shim is there")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("the shim is made executable");
}
