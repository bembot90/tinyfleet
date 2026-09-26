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
//! `core/src/store/exec.rs` runs one — bar the store stub, the board held in
//! memory answering the whole contract, for the runs that pass.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use fleet_core::store::conformance::CHECKS;
use fleet_core::store::exec::Exec;
use fleet_core::store::{Filter, Store};

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

    /// `fleet store check` with `args` as [`check`](Rig::check) runs it, left
    /// running with its stdout on a pipe the arm reads.
    fn spawned(&self, args: &[&str]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["store", "check"])
            .args(args)
            .current_dir(self.project())
            .hermetic(&self.root.join("home"), &self.root.join("machine"), None)
            .env("TMPDIR", self.tmp())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
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

/// The binary's defaults under the rig's machine directory, which a name is
/// resolved over.
fn defaults_under(rig: &Rig) {
    let defaults = rig.root.join("machine").join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&defaults).expect("the defaults dir is made");
    fleet_core::embedded::write_all(&defaults).expect("the embedded defaults are written");
}

/// The refusal a name no installed pack carries is answered with: the line
/// that installs the pack fleet-packs carries under it, at the pinned tag.
fn nowhere(name: &str) -> String {
    format!(
        "fleet store check: no store adapter named `{name}` in the installed packs — `{}` \
         installs the one fleet-packs carries\n",
        fleet_core::store::pack_line(
            fleet_core::supported::PINNED_PACKS_SOURCE,
            name,
            fleet_core::supported::PINNED_PACKS
        )
    )
}

/// Arm 1. A project whose file names no store is checked on the default name,
/// resolved through the installed packs as any name is — and on a machine
/// where no pack carries it, the run could not open one: exit 3, naming the
/// line that installs it, with nothing made.
///
/// RED-PROOF: on the base a file naming no store was checked on the built-in
/// store, and no pack was looked for.
#[test]
fn a_project_naming_no_store_is_checked_on_the_default_name_through_the_packs() {
    let rig = Rig::new("default");
    defaults_under(&rig);
    let out = rig.check(&[]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert_eq!(stderr(&out), nowhere(fleet_core::store::DEFAULT_ADAPTER));
    assert_eq!(stdout(&out), "", "no check line was printed");
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

/// Arm 4. `--adapter` takes an absolute path or a name, and a relative path —
/// a separator in what would be a name — is usage, with nothing made.
#[test]
fn a_relative_adapter_path_is_usage() {
    let rig = Rig::new("relative");
    for given in ["relative/path", "./x", ""] {
        let out = rig.check(&["--adapter", given]);
        assert_eq!(out.status.code(), Some(2), "{given}: {}", stderr(&out));
        assert_eq!(
            stderr(&out),
            format!(
                "fleet store check: --adapter takes an absolute path to an executable or the \
                 name of a store adapter an installed pack carries, and `{given}` is neither\n"
            )
        );
    }
    assert!(rig.left().is_empty(), "nothing was made: {:?}", rig.left());
}

/// Arm 4b. `--adapter` naming an adapter BY NAME checks the one the installed
/// packs carry under it, as `[store] adapter` naming it would: the pack's
/// entry is asked. A name no pack carries could not be opened: exit 3, naming
/// the line that installs it.
///
/// RED-PROOF: on the base a name was a relative path, and both runs were usage.
#[test]
fn an_adapter_named_by_the_flag_resolves_through_the_packs() {
    let rig = Rig::new("flag-named");
    defaults_under(&rig);
    let pack = rig.root.join("machine/packs/tracker");
    let adapter = pack.join("adapters/store/x");
    std::fs::create_dir_all(&adapter).expect("the adapter's directory is made");
    std::fs::write(
        pack.join("pack.toml"),
        "[pack]\nname = \"tracker\"\nversion = \"0.1.0\"\nschema = 3\n",
    )
    .expect("the pack's manifest is written");
    std::fs::write(
        adapter.join("adapter.toml"),
        "[adapter]\nname = \"x\"\nkind = \"store\"\nversion = \"0.1.0\"\nentry = \"main.sh\"\n",
    )
    .expect("the adapter's manifest is written");
    let stub = rig.stub(
        "capabilities) echo '{\"schema_version\":1,\"scratch\":false}' ;;\n\
         *) echo '{\"schema_version\":1}' ;;",
    );
    std::fs::rename(&stub, adapter.join("main.sh")).expect("the stub is the adapter's entry");

    let out = rig.check(&["--adapter", "x"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "fleet store check: x declares no scratch capability, and the check runs only on a \
         store it makes for the purpose — nothing was run\n"
    );
    assert_eq!(rig.argv(), ["capabilities"], "the pack's entry was asked");

    let out = rig.check(&["--adapter", "nowhere"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert_eq!(stderr(&out), nowhere("nowhere"));
    assert_eq!(rig.argv(), ["capabilities"], "and nothing else was asked");
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 5. An adapter path nothing is at could not be run: exit 3, naming the
/// path and the flag that named it, which is not the project's `[store]
/// adapter`.
///
/// RED-PROOF: on the base the refusal opens `[store] adapter names`, a key
/// this run was never handed.
#[test]
fn an_adapter_nothing_is_at_is_could_not_tell_naming_the_path() {
    let rig = Rig::new("nonexistent");
    let out = rig.check(&["--adapter", "/nonexistent"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "fleet store check: --adapter names `/nonexistent`, which is not an executable file\n"
    );
    assert_eq!(stdout(&out), "", "no check line was printed");
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 6. A project naming a store adapter by name is checked on the adapter
/// the machine's installed packs carry under it: the pack's entry is the one
/// asked, and its answer is the row — here, a scratch it does not declare.
///
/// RED-PROOF: on the base the name is neither form, and the check exits 3
/// without asking anything.
#[test]
fn a_project_naming_an_adapter_is_checked_on_the_one_its_packs_carry() {
    let rig = Rig::new("named");
    let machine = rig.root.join("machine");
    let defaults = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&defaults).expect("the defaults dir is made");
    fleet_core::embedded::write_all(&defaults).expect("the embedded defaults are written");
    let pack = machine.join("packs/tracker");
    let adapter = pack.join("adapters/store/x");
    std::fs::create_dir_all(&adapter).expect("the adapter's directory is made");
    std::fs::write(
        pack.join("pack.toml"),
        "[pack]\nname = \"tracker\"\nversion = \"0.1.0\"\nschema = 3\n",
    )
    .expect("the pack's manifest is written");
    std::fs::write(
        adapter.join("adapter.toml"),
        "[adapter]\nname = \"x\"\nkind = \"store\"\nversion = \"0.1.0\"\nentry = \"main.sh\"\n",
    )
    .expect("the adapter's manifest is written");
    let stub = rig.stub(
        "version) echo '{\"schema_version\":1,\"name\":\"x\",\"version\":\"0.1.0\"}' ;;\n\
         capabilities) echo '{\"schema_version\":1,\"scratch\":false}' ;;\n\
         *) echo '{\"schema_version\":1}' ;;",
    );
    std::fs::rename(&stub, adapter.join("main.sh")).expect("the stub is the adapter's entry");
    std::fs::write(
        rig.project().join("fleet.toml"),
        "[store]\nadapter = \"x\"\n",
    )
    .expect("the project's file names the adapter");

    let out = rig.check(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "fleet store check: x declares no scratch capability, and the check runs only on a \
         store it makes for the purpose — nothing was run\n"
    );
    assert_eq!(rig.argv(), ["capabilities"], "the pack's entry was asked");
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 7. Each check's line is on stdout as its check is answered, before the
/// next check is asked: an adapter whose `version` waits on a file the arm
/// writes has the first check's line read off the pipe while the run is still
/// waiting on the second.
///
/// The wait is bounded inside the stub, under the store call's own bound, so
/// the run ends whichever way the arm goes and leaves no process behind.
///
/// RED-PROOF: a run that prints once every check has answered prints nothing
/// while `version` waits, and no line is read within the bound.
#[test]
fn each_line_is_printed_as_its_check_is_answered() {
    let rig = Rig::new("streamed");
    let release = rig.root.join("release");
    let adapter = rig.stub(&format!(
        "capabilities) echo '{{\"schema_version\":1,\"scratch\":true}}' ;;\n\
         scratch)\n\
           into=$(printf '%s' \"$request\" | sed -n 's/.*\"into\":\"\\([^\"]*\\)\".*/\\1/p')\n\
           printf '{{\"schema_version\":1,\"root\":\"%s\"}}\\n' \"$into\" ;;\n\
         version)\n\
           i=0\n\
           while [ ! -f '{release}' ] && [ $i -lt 400 ]; do sleep 0.1; i=$((i+1)); done\n\
           echo '{{\"schema_version\":1}}' ;;\n\
         *) echo '{{\"schema_version\":1}}' ;;",
        release = release.display(),
    ));
    let mut child = rig.spawned(&["--adapter", &adapter.to_string_lossy()]);
    let pipe = child.stdout.take().expect("stdout is piped");
    let (lines, read) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });

    let first = read.recv_timeout(Duration::from_secs(20));
    let running = child.try_wait().expect("the run's state reads").is_none();
    std::fs::write(&release, "").expect("the stub's wait is released");
    let status = child.wait().expect("the run ends");
    reader.join().expect("the reader ends");
    let rest: Vec<String> = read.try_iter().collect();

    let first = first.expect("a line is on stdout while the second check waits");
    assert!(
        ["PASS  empty listings", "FAIL  empty listings: "]
            .iter()
            .any(|opens| first.starts_with(opens)),
        "the first line is the first check's: {first}"
    );
    assert!(running, "the first line was read before the run ended");
    assert_eq!(
        status.code(),
        Some(1),
        "the bare stub fails checks: {rest:?}"
    );
    assert_eq!(
        rest.len(),
        CHECKS.len(),
        "every other check's line and the summary followed: {rest:?}"
    );
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// The two checks that plant another writer's keys, skipped where the run
/// hands no other writer in — which `fleet store check` never does, since no
/// verb of the contract plants one.
const NO_OTHER_WRITER: [&str; 2] = [
    "another writer's keys: no other writer was handed to this run, so nothing plants another \
     tool's keys",
    "another writer's keys are listed as foreign: no other writer was handed to this run, so \
     nothing plants another tool's keys",
];

/// Arm 8. The store stub, handed by path, passes every check the run asks of
/// it: one PASS line for every check but the two that plant another writer's
/// keys, and those two skipped.
#[test]
fn the_store_stub_passes_every_check_it_is_asked() {
    let rig = Rig::new("stub");
    let stub = common::stub_path();
    let out = rig.check(&["--adapter", &stub.to_string_lossy()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let read = read(&out, "stub");
    assert!(read.fail.is_empty(), "no check failed: {:?}", read.fail);
    assert_eq!(read.skip, NO_OTHER_WRITER, "{}", read.summary);
    assert_eq!(
        read.summary,
        format!(
            "store check: stub — {} passed, 0 failed, 2 skipped",
            CHECKS.len() - 2
        )
    );
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}

/// Arm 9. A project `take_a_store` keeps on the stub is checked on the stub
/// with no flag — its own file names it — and the project's own store is the
/// empty one the helper made, which the run never writes to.
#[test]
fn a_project_kept_on_the_stub_is_checked_on_it() {
    let rig = Rig::new("stub-store");
    common::take_a_store(&rig.project());
    let stub = common::stub_path();
    let policy = std::fs::read_to_string(rig.project().join("fleet.toml"))
        .expect("the project's file reads");
    assert!(
        policy.contains(&format!("[store]\nadapter = \"{}\"", stub.display())),
        "{policy}"
    );

    let out = rig.check(&[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let read = read(&out, "stub");
    assert!(read.fail.is_empty(), "no check failed: {:?}", read.fail);
    assert_eq!(read.skip, NO_OTHER_WRITER, "{}", read.summary);

    let own = Exec::at(&stub, &rig.project());
    assert_eq!(
        own.list(&Filter::Ready).expect("the project's store reads"),
        Vec::new(),
        "the check ran on a scratch of its own, and the project's store is as it was made"
    );
    assert!(
        rig.left().is_empty(),
        "the temp dir is gone: {:?}",
        rig.left()
    );
}
