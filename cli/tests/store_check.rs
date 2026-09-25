//! `fleet store check` through the shipped binary: the store contract's
//! conformance suite run against an adapter, on a scratch store the adapter
//! makes in a temp dir the verb makes and removes, answered in the exit table.
//!
//! EACH ARM POINTS `TMPDIR` AT A DIRECTORY OF ITS OWN, so the temp dir the verb
//! makes is looked for where it was made — and each arm asserts nothing of it
//! is left there, whichever row of the table the run answered.
//!
//! The stubs are `#!/bin/sh` adapters that append their verb to `argv` beside
//! them and read their request off stdin, one process per call, as
//! `core/src/store/exec.rs` runs one.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_core::store::conformance::CHECKS;

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The prefix of the temp dir the verb makes, under its `TMPDIR`.
const MADE: &str = "fleet-store-check-";

/// A project, a home, a machine directory and a `TMPDIR`, all under one root
/// the arm owns and removes.
struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-store-check-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig { root };
        for dir in [rig.project(), rig.tmp(), rig.root.join("machine")] {
            std::fs::create_dir_all(&dir).expect("the rig's directories are made");
        }
        std::fs::write(
            rig.project().join("fleet.toml"),
            "# a project whose file names no store\n",
        )
        .expect("the project's file is written");
        rig
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// `fleet store check` with `args`, run from inside the project.
    fn check(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["store", "check"])
            .args(args)
            .current_dir(self.project())
            .hermetic(&self.root.join("home"), &self.root.join("machine"), None)
            .env("TMPDIR", self.tmp())
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

    /// An adapter stub at `<root>/adapter` whose `case "$1"` arms are `arms`,
    /// the request read into `$request` first.
    fn stub(&self, arms: &str) -> PathBuf {
        let bin = self.root.join("adapter");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n\
                 printf '%s\\n' \"$1\" >> '{argv}'\n\
                 request=$(cat)\n\
                 case \"$1\" in\n{arms}\nesac\n",
                argv = self.root.join("argv").display(),
            ),
        )
        .expect("the stub is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
        bin
    }

    /// Every verb the stub was called with, in order.
    fn argv(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("argv"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
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

/// The run's lines read back: one per check, by its word, and the summary's
/// three counts.
struct Read {
    pass: Vec<String>,
    skip: Vec<String>,
    fail: Vec<String>,
    summary: String,
    counted: (usize, usize, usize),
}

/// Every line of stdout is a check's or the summary, which is the last.
fn read(out: &Output, adapter: &str) -> Read {
    let text = stdout(out);
    let mut lines: Vec<&str> = text.lines().collect();
    let summary = lines.pop().expect("the run printed a summary").to_string();
    let opens = format!("store check: {adapter} — ");
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
        summary: summary.clone(),
        counted: (numbers[0], numbers[1], numbers[2]),
    };
    for line in lines {
        if let Some(name) = line.strip_prefix("PASS  ") {
            read.pass.push(name.to_string());
        } else if let Some(rest) = line.strip_prefix("SKIP  ") {
            read.skip.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("FAIL  ") {
            read.fail.push(rest.to_string());
        } else {
            panic!("a line that is no check's: `{line}` in\n{text}");
        }
    }
    assert_eq!(
        read.pass.len() + read.skip.len() + read.fail.len(),
        CHECKS.len(),
        "one line per check:\n{text}"
    );
    assert_eq!(
        read.counted,
        (read.pass.len(), read.fail.len(), read.skip.len()),
        "the summary counts the lines above it:\n{text}"
    );
    read
}

/// Arm 1. A project whose file names no store is checked on the built-in bd:
/// every check passes or is skipped, and the one skipped is another writer's
/// keys, since the verb hands no other writer in.
///
/// This costs one `bd init`, the bd adapter's own scratch, on bd's embedded
/// engine inside the verb's temp dir.
#[test]
fn a_project_naming_no_store_is_checked_on_the_built_in_bd() {
    let rig = Rig::new("bd");
    common::note_bd_init("store-check");
    let out = rig.check(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let read = read(&out, "bd");
    assert!(read.fail.is_empty(), "no check failed: {:?}", read.fail);
    assert_eq!(
        read.skip,
        [
            "another writer's keys: no other writer was handed to this run, so nothing plants \
          another tool's keys"
        ],
        "{}",
        read.summary
    );
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 2. An adapter whose capabilities declare no scratch is refused with
/// exit 1 and nothing else is asked of it, because the check runs only on a
/// store it makes for the purpose.
#[test]
fn an_adapter_declaring_no_scratch_is_refused_and_nothing_else_is_called() {
    let rig = Rig::new("no-scratch");
    let adapter = rig.stub(
        "capabilities) echo '{\"schema_version\":1,\"scratch\":false}' ;;\n\
         *) echo '{\"schema_version\":1}' ;;",
    );
    let out = rig.check(&["--adapter", &adapter.to_string_lossy()]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        format!(
            "fleet store check: {} declares no scratch capability, and the check runs only on \
             a store it makes for the purpose — nothing was run\n",
            adapter.display()
        )
    );
    assert_eq!(stdout(&out), "", "no check line was printed");
    assert_eq!(rig.argv(), ["capabilities"], "nothing else was called");
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 3. An adapter that makes a scratch store and answers every other verb
/// with a bare envelope fails the checks that read an answer back, and every
/// check is still asked: one line each, and a summary that counts them.
///
/// RED-PROOF: a runner that stops at the first failure prints one FAIL line,
/// and this arm asks for two — "create then show" and "holds", which are not
/// adjacent in the table.
#[test]
fn every_check_is_asked_of_an_adapter_that_answers_nothing_back() {
    let rig = Rig::new("bare");
    let adapter = rig.stub(
        "capabilities) echo '{\"schema_version\":1,\"scratch\":true}' ;;\n\
         scratch)\n\
           into=$(printf '%s' \"$request\" | sed -n 's/.*\"into\":\"\\([^\"]*\\)\".*/\\1/p')\n\
           printf '{\"schema_version\":1,\"root\":\"%s\"}\\n' \"$into\" ;;\n\
         *) echo '{\"schema_version\":1}' ;;",
    );
    let out = rig.check(&["--adapter", &adapter.to_string_lossy()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    // The version the stub answers names nothing, so the summary names the
    // adapter by its path.
    let read = read(&out, &adapter.display().to_string());
    for check in ["create then show", "holds"] {
        assert!(
            read.fail
                .iter()
                .any(|line| line.starts_with(&format!("{check}: "))),
            "a FAIL line for {check}: {:?}",
            read.fail
        );
    }
    assert!(
        rig.argv().contains(&String::from("scratch")),
        "the adapter was asked for its scratch store: {:?}",
        rig.argv()
    );
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 4. `--adapter` takes an absolute path, and a relative one is usage,
/// with nothing made.
#[test]
fn a_relative_adapter_path_is_usage() {
    let rig = Rig::new("relative");
    let out = rig.check(&["--adapter", "relative/path"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "fleet store check: --adapter takes an absolute path to an executable, and \
         relative/path is not one\n"
    );
    assert!(rig.left().is_empty(), "nothing was made: {:?}", rig.left());
}

/// Arm 5. An adapter path nothing is at could not be run: exit 3, naming the
/// path.
#[test]
fn an_adapter_nothing_is_at_is_could_not_tell_naming_the_path() {
    let rig = Rig::new("nonexistent");
    let out = rig.check(&["--adapter", "/nonexistent"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("/nonexistent"),
        "the refusal names the path: {}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), "", "no check line was printed");
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}
