//! `pack add` through the shipped binary: the three exit codes, the lock it
//! writes, and the stamp only the binary supplies.
//!
//! Every invocation passes `--packs-dir` and `--lock` into a directory the test
//! owns, so a run installs nothing where a second test or a real fleet reads.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(args)
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A temporary directory removed when the test ends.
struct Temp {
    root: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Temp {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-add-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the temp root is created");
        Temp { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn arg(&self, relative: &str) -> String {
        self.path(relative).to_string_lossy().into_owned()
    }

    /// The bottom layer `add` lays its candidate over, materialized as `fleet
    /// create` and `fleet start` materialize it. Every arm here names its own
    /// packs directory with `--packs-dir`, and the verb reads the defaults
    /// BESIDE whichever one it was given, so the machine this rig models is one
    /// that has started.
    fn defaults(&self) -> &Temp {
        let root = self.path(fleet_core::defaults::DIR);
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

/// A local repository holding one pack, committed and tagged `v1`. The box's own
/// git configuration is kept out so a global hooks path or a missing identity
/// cannot decide whether the fixture builds.
fn a_pack_repo(label: &str) -> Temp {
    a_repo(
        label,
        &[
            (
                "pack.toml",
                "[pack]\nname = \"neighborly\"\nversion = \"0.1.0\"\nschema = 3\n",
            ),
            ("skills/greet/SKILL.md", "# greet\n"),
        ],
    )
}

/// A local repository holding the files given, committed and tagged `v1`.
fn a_repo(label: &str, files: &[(&str, &str)]) -> Temp {
    let repo = Temp::new(label);
    for (relative, body) in files {
        let path = repo.path(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory is made");
        std::fs::write(&path, body).expect("the file is written");
    }

    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["add", "--all"],
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

/// `YYYY-MM-DDTHH:MM:SSZ`, which is the stamp the binary reads from its own
/// clock and the one field a caller cannot hand it.
fn is_rfc3339_utc(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|i| b[*i].is_ascii_digit())
}

#[test]
fn a_pack_is_installed_pinned_and_reported() {
    let repo = a_pack_repo("ok-repo");
    let machine = Temp::new("ok-machine");
    machine.defaults();

    let out = run(&[
        "pack",
        "add",
        &repo.root.to_string_lossy(),
        "--version",
        "v1",
        "--packs-dir",
        &machine.arg("packs"),
        "--lock",
        &machine.arg("packs.lock"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("added neighborly v1 at"),
        "{}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("pinned in"), "{}", stdout(&out));
    assert!(machine
        .path("packs/neighborly/skills/greet/SKILL.md")
        .is_file());

    let lock = std::fs::read_to_string(machine.path("packs.lock")).expect("the lock is written");
    assert!(lock.starts_with("schema = 1\n"), "{lock}");
    assert!(lock.contains("version = \"v1\"\n"), "{lock}");
    let fetched = field(&lock, "fetched");
    assert!(
        is_rfc3339_utc(&fetched),
        "the binary stamps RFC 3339 UTC: `{fetched}` in {lock}"
    );
    let commit = field(&lock, "commit");
    assert!(
        commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit()),
        "{lock}"
    );
}

#[test]
fn a_caret_version_exits_one_with_the_one_line() {
    let repo = a_pack_repo("caret-repo");
    let machine = Temp::new("caret-machine");
    machine.defaults();

    let out = run(&[
        "pack",
        "add",
        &repo.root.to_string_lossy(),
        "--version",
        "^1.2",
        "--packs-dir",
        &machine.arg("packs"),
        "--lock",
        &machine.arg("packs.lock"),
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert_eq!(
        stderr(&out).lines().count(),
        1,
        "one line: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("is a caret range — pin a tag or `sha:<40 hex>`"),
        "{}",
        stderr(&out)
    );
    assert!(!machine.path("packs").exists());
    assert!(!machine.path("packs.lock").exists());
}

#[test]
fn no_source_and_no_version_are_usage_errors_and_not_refusals() {
    let out = run(&["pack", "add"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "usage goes to stderr, never stdout");

    // The argument surface is clap's, so the sentence is its wording; what
    // this arm holds is that each of the two is a usage error naming the option
    // and not a refusal about the pack.
    let out = run(&["pack", "add", "/tmp/somewhere"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("required arguments were not provided")
            && stderr(&out).contains("--version <VERSION>"),
        "{}",
        stderr(&out)
    );

    let out = run(&["pack", "add", "/tmp/somewhere", "--version"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("a value is required for '--version <VERSION>'"),
        "{}",
        stderr(&out)
    );
}

/// fleet-4fw: tiny added alone brings ts, which the same checkout holds, and
/// says so on its output.
#[test]
fn an_import_the_same_checkout_holds_is_added_and_reported() {
    const TINY: &str = "[pack]\nname = \"tiny\"\nversion = \"0.1.0\"\nschema = 3\n\n\
                        [imports.ts]\nsource = \"../ts\"\nversion = \"0.1.0\"\n";
    let repo = a_repo(
        "import-repo",
        &[
            ("packs/tiny/pack.toml", TINY),
            (
                "packs/ts/pack.toml",
                "[pack]\nname = \"ts\"\nversion = \"0.1.0\"\nschema = 3\n",
            ),
        ],
    );
    let machine = Temp::new("import-machine");
    machine.defaults();

    let out = run(&[
        "pack",
        "add",
        &format!("{}//packs/tiny", repo.root.to_string_lossy()),
        "--version",
        "v1",
        "--packs-dir",
        &machine.arg("packs"),
        "--lock",
        &machine.arg("packs.lock"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = stdout(&out);
    assert!(said.contains("added tiny v1 at"), "{said}");
    assert!(
        said.contains("added ts v1 at") && said.contains("which tiny imports"),
        "the import is named as added, and why: {said}"
    );
    assert!(machine.path("packs/ts/pack.toml").is_file());
}

/// fleet-4fw: an import from another repository is not fetched, and the add
/// says so on stderr with the line that adds it, still exiting 0.
#[test]
fn an_import_from_elsewhere_is_named_with_the_line_that_adds_it() {
    let repo = a_repo(
        "elsewhere-repo",
        &[(
            "pack.toml",
            "[pack]\nname = \"tiny\"\nversion = \"0.1.0\"\nschema = 3\n\n\
             [imports.ts]\nsource = \"https://example.invalid/o/ts\"\nversion = \"v0.1.0\"\n",
        )],
    );
    let machine = Temp::new("elsewhere-machine");
    machine.defaults();

    let out = run(&[
        "pack",
        "add",
        &repo.root.to_string_lossy(),
        "--version",
        "v1",
        "--packs-dir",
        &machine.arg("packs"),
        "--lock",
        &machine.arg("packs.lock"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("added tiny v1 at"),
        "{}",
        stdout(&out)
    );
    assert!(
        stderr(&out).contains(
            "`tiny` imports `ts`, which is not installed — \
             `fleet pack add https://example.invalid/o/ts --version v0.1.0` adds it"
        ),
        "{}",
        stderr(&out)
    );
}

/// One `key = "value"` out of a lock, by name.
fn field(lock: &str, key: &str) -> String {
    lock.lines()
        .find_map(|line| line.strip_prefix(&format!("{key} = ")))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_else(|| panic!("the lock carries no `{key}`: {lock}"))
}
