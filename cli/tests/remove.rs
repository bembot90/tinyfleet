//! `pack remove` and `pack list` through the shipped binary: the round trip, the
//! exit codes, and the exact bytes the table prints.
//!
//! Every invocation names a lock and a packs dir the test owns, and `FLEET_DIR`
//! points at that same directory, so neither the flags nor their defaults can
//! reach a real fleet.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A temporary directory removed when the test ends.
struct Temp {
    root: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Temp {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-remove-{label}-{}-{n}",
            std::process::id()
        ));
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
    /// create` and `fleet start` materialize it: the machine this rig models is
    /// one that has started, which is every machine a person adds a pack on.
    fn defaults(&self) -> &Temp {
        let root = self.path(fleet_core::defaults::DIR);
        std::fs::create_dir_all(&root).expect("the defaults dir is created");
        fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
        self
    }

    /// The binary, with this directory as the machine directory: what the two
    /// flags default to when they are left off.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .hermetic(&self.root.join("home"), &self.root, None)
            .output()
            .expect("the built binary runs")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A local repository holding one pack, committed and tagged `v1`. The box's own
/// git configuration is kept out so a global hooks path or a missing identity
/// cannot decide whether the fixture builds.
fn a_pack_repo(label: &str, name: &str) -> Temp {
    let repo = Temp::new(label);
    std::fs::write(
        repo.path("pack.toml"),
        format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n"),
    )
    .expect("the manifest is written");
    std::fs::create_dir_all(repo.path("skills/greet")).expect("the skill directory is created");
    std::fs::write(repo.path("skills/greet/SKILL.md"), "# greet\n").expect("the skill is written");

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

const HEADER: &str = "name  source  version  commit  fetched\n";

// -------------------------------------------------------- AC1: the round trip

#[test]
fn add_then_list_then_remove_then_list_again() {
    let repo = a_pack_repo("round-repo", "neighborly");
    let machine = Temp::new("round-machine");
    machine.defaults();
    let source = repo.root.to_string_lossy().into_owned();

    let out = machine.run(&[
        "pack",
        "add",
        &source,
        "--version",
        "v1",
        "--packs-dir",
        &machine.arg("packs"),
        "--lock",
        &machine.arg("packs.lock"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let listed = machine.run(&["pack", "list", "--lock", &machine.arg("packs.lock")]);
    assert_eq!(listed.status.code(), Some(0), "{}", stderr(&listed));
    let table = stdout(&listed);
    assert_eq!(table.lines().count(), 2, "the header and one row: {table}");
    let row = table.lines().nth(1).expect("one row").to_string();
    for cell in ["neighborly", source.as_str(), "v1"] {
        assert!(row.contains(cell), "the row carries `{cell}`: {row}");
    }
    let commit = row
        .split_whitespace()
        .find(|w| w.len() == 40 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_else(|| panic!("the row carries a commit: {row}"));
    assert_eq!(commit.len(), 40);
    assert!(
        row.ends_with('Z'),
        "the row ends in the fetched stamp: {row}"
    );

    let removed = machine.run(&[
        "pack",
        "remove",
        &source,
        "--packs-dir",
        &machine.arg("packs"),
        "--lock",
        &machine.arg("packs.lock"),
    ]);
    assert_eq!(removed.status.code(), Some(0), "{}", stderr(&removed));
    assert!(
        stdout(&removed).contains(&format!(
            "removed neighborly v1 — {}",
            machine.arg("packs/neighborly")
        )),
        "{}",
        stdout(&removed)
    );
    assert!(
        stdout(&removed).contains(&format!(
            "dropped {source} v1 from {}",
            machine.arg("packs.lock")
        )),
        "{}",
        stdout(&removed)
    );
    assert!(!machine.path("packs/neighborly").exists());

    let after = machine.run(&["pack", "list", "--lock", &machine.arg("packs.lock")]);
    assert_eq!(after.status.code(), Some(0), "{}", stderr(&after));
    assert_eq!(stdout(&after), HEADER);
}

/// The two flags default to the machine directory's own packs and lock, the way
/// `pack add` defaults them: this arm passes neither and reads the same paths.
#[test]
fn the_flags_default_to_the_machine_directory() {
    let repo = a_pack_repo("default-repo", "neighborly");
    let machine = Temp::new("default-machine");
    machine.defaults();
    let source = repo.root.to_string_lossy().into_owned();

    let added = machine.run(&["pack", "add", &source, "--version", "v1"]);
    assert_eq!(added.status.code(), Some(0), "{}", stderr(&added));
    assert!(machine.path("packs/neighborly").is_dir());

    let removed = machine.run(&["pack", "remove", &source]);
    assert_eq!(removed.status.code(), Some(0), "{}", stderr(&removed));
    assert!(!machine.path("packs/neighborly").exists());

    let listed = machine.run(&["pack", "list"]);
    assert_eq!(listed.status.code(), Some(0), "{}", stderr(&listed));
    assert_eq!(stdout(&listed), HEADER);
}

// ----------------------------------------------------------- AC2: a refusal

#[test]
fn a_source_the_lock_does_not_hold_exits_one_with_the_one_line() {
    let machine = Temp::new("unknown-machine");
    machine.defaults();
    let out = machine.run(&[
        "pack",
        "remove",
        "/nowhere/at/all",
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
        stderr(&out).contains("fleet pack remove: `/nowhere/at/all` is not in"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn no_source_is_a_usage_error_and_not_a_refusal() {
    let machine = Temp::new("usage-machine");
    machine.defaults();
    let out = machine.run(&["pack", "remove"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "usage goes to stderr, never stdout");
}

// ------------------------------------------------------------ AC3: the stderr

#[test]
fn a_directory_already_gone_is_said_on_stderr_and_the_line_still_drops() {
    let repo = a_pack_repo("gone-repo", "neighborly");
    let machine = Temp::new("gone-machine");
    machine.defaults();
    let source = repo.root.to_string_lossy().into_owned();

    let added = machine.run(&["pack", "add", &source, "--version", "v1"]);
    assert_eq!(added.status.code(), Some(0), "{}", stderr(&added));
    std::fs::remove_dir_all(machine.path("packs/neighborly")).expect("the directory is deleted");

    let out = machine.run(&["pack", "remove", &source]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("was already gone"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        stdout(&machine.run(&["pack", "list"])),
        HEADER,
        "the line is dropped"
    );
}

// -------------------------------------------------------- AC5: list's shape

#[test]
fn an_empty_lock_and_an_absent_one_both_print_the_header_alone() {
    let machine = Temp::new("empty-machine");
    machine.defaults();

    let absent = machine.run(&["pack", "list", "--lock", &machine.arg("nothing-here.lock")]);
    assert_eq!(absent.status.code(), Some(0), "{}", stderr(&absent));
    assert_eq!(stdout(&absent), HEADER);

    std::fs::write(machine.path("packs.lock"), "schema = 1\n").expect("the empty lock is written");
    let empty = machine.run(&["pack", "list", "--lock", &machine.arg("packs.lock")]);
    assert_eq!(empty.status.code(), Some(0), "{}", stderr(&empty));
    assert_eq!(stdout(&empty), HEADER);
}

/// The exact bytes, for two sources of different lengths: every column but the
/// last padded to its widest cell — the header's own width counted — with two
/// spaces between, in source order, and `-` where a line carries no name.
#[test]
fn two_entries_print_padded_in_source_order() {
    const FIRST: &str = "0123456789abcdef0123456789abcdef01234567";
    const SECOND: &str = "89abcdef0123456789abcdef0123456789abcdef";

    let machine = Temp::new("table-machine");
    machine.defaults();
    std::fs::write(
        machine.path("packs.lock"),
        format!(
            "schema = 1\n\n\
             [packs.\"a-longer-source\"]\n\
             name = \"gastown\"\n\
             version = \"0.4.0\"\n\
             commit = \"{FIRST}\"\n\
             fetched = \"2026-09-08T13:45:00Z\"\n\n\
             [packs.aa]\n\
             version = \"v1\"\n\
             commit = \"{SECOND}\"\n\
             fetched = \"2026-09-07T09:00:00Z\"\n"
        ),
    )
    .expect("the lock is written");

    // The four widths are this fixture's widest cells, header included and
    // counted by hand — 7 for `gastown`, 15 for `a-longer-source`, 7 for
    // `version`, 40 for a commit — and the last column is never padded. `a-` is
    // below `aa`, so source order is not the order the file was written in.
    let expected = format!(
        "{:<7}  {:<15}  {:<7}  {:<40}  {}\n",
        "name", "source", "version", "commit", "fetched"
    ) + &format!(
        "{:<7}  {:<15}  {:<7}  {:<40}  {}\n",
        "gastown", "a-longer-source", "0.4.0", FIRST, "2026-09-08T13:45:00Z"
    ) + &format!(
        "{:<7}  {:<15}  {:<7}  {:<40}  {}\n",
        "-", "aa", "v1", SECOND, "2026-09-07T09:00:00Z"
    );

    let out = machine.run(&["pack", "list", "--lock", &machine.arg("packs.lock")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), expected);
}

#[test]
fn a_lock_that_is_not_toml_exits_three_with_the_error_on_stderr() {
    let machine = Temp::new("unparsable-machine");
    machine.defaults();
    std::fs::write(machine.path("packs.lock"), "this is not toml = = =\n")
        .expect("the lock is written");

    let out = machine.run(&["pack", "list", "--lock", &machine.arg("packs.lock")]);
    assert_eq!(out.status.code(), Some(3), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("packs.lock does not parse as TOML"),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
}
