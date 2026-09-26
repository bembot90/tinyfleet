//! `fleet doctor` through the shipped binary: every check the pack layers
//! carry, run from inside a scratch project, over the binary's own defaults
//! and a scratch pack each arm writes.
//!
//! THE DEFAULTS ARE THE BINARY'S OWN, written by the function `fleet start`
//! writes them with, so the rows an arm reads are the ones a person's first
//! `fleet doctor` prints. Every tool those checks ask is a stub this rig puts
//! first on `PATH`: the agent binary answers its version at the supported pin
//! and the isolation pair by whether the credential knob is set, bd answers
//! its version at the pin and an empty list to every read the adopt-board
//! check's `fleet item list` makes, and the pinned runtime prints one line.
//! Nothing here reads a board, starts a session or asks a real tool anything;
//! the adopt-board check over a real board is `adopt.rs`'s.
//!
//! THE PINS ARE READ FROM CORE and not spelled here, so a supported-version
//! move leaves this suite green and the doctor's own copies are core's suite
//! to hold in step.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The runtime the scratch pack pins, and the version its stub prints.
const RUNTIME: &str = "fx-runtime";
const VERSION: &str = "1.2.3";

/// A project whose record guard has its prefix: every default check passes.
const CONFIGURED: &str = "[project]\nitem_prefix = \"fx\"\n";
/// A project whose record guard has none: guards-installed is a finding.
const UNCONFIGURED: &str = "[project]\n";

/// Every check the binary's defaults carry, in the order the verb runs them.
const DEFAULTS: [&str; 7] = [
    "adopt-board",
    "bd-version",
    "claude-code-version",
    "fleet-packs-version",
    "guards-installed",
    "isolation-pair",
    "runtime-version",
];

/// The row `runtime-version` gives when no installed pack pins a runtime.
const NOTHING_PINNED: &str = "pass runtime-version (defaults) — nothing pinned: no installed \
                              pack declares a [runtime] table";

fn defaults_into(machine: &Path) -> PathBuf {
    let root = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&root).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
    root
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent directory is made");
    }
    std::fs::write(path, body).expect("the file is written");
}

fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("the stub is made executable");
}

struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    stubs: PathBuf,
}

impl Rig {
    /// A project holding `policy` as its `fleet.toml`, the defaults and no
    /// pack installed, and the stubs every default check asks.
    fn new(label: &str, policy: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-doctor-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            project: root.join("project"),
            machine: root.join("machine"),
            stubs: root.join("stubs"),
            root,
        };
        std::fs::create_dir_all(rig.machine.join("packs")).expect("the packs dir is made");
        defaults_into(&rig.machine);
        write(&rig.project.join("fleet.toml"), policy);

        // The agent binary: its version at the supported pin, and the
        // isolation pair's two arms told apart by whether the credential knob
        // is SET, which is the whole of what the pair measures.
        rig.stub(
            "claude",
            &format!(
                "case \"${{1:-}}\" in\n\
                 --version) echo \"{} (Claude Code)\"; exit 0 ;;\n\
                 esac\n\
                 if [ \"${{CLAUDE_SECURESTORAGE_CONFIG_DIR+set}}\" = set ]; then\n\
                 echo '{{\"loggedIn\": true}}'\n\
                 exit 0\n\
                 fi\n\
                 echo '{{\"loggedIn\": false}}'\n\
                 exit 1",
                fleet_core::supported::PINNED_CLAUDE_CODE
            ),
        );
        // bd: its version at the pin, and an empty board to any other call —
        // `-C <root> ready …` and `-C <root> list …` are the adopt-board
        // check's two reads, through `fleet item list`.
        rig.stub(
            "bd",
            &format!(
                "case \"${{1:-}}\" in\n\
                 version) echo \"bd version {} (stub)\" ;;\n\
                 *) echo '[]' ;;\n\
                 esac",
                fleet_core::store::bd::PINNED_BD
            ),
        );
        rig.stub(RUNTIME, &format!("echo \"{RUNTIME} {VERSION}\""));
        rig
    }

    fn stub(&self, name: &str, body: &str) {
        let path = self.stubs.join(name);
        write(&path, &format!("#!/bin/sh\n{body}\n"));
        executable(&path);
    }

    /// Install pack `name`, pinning the runtime at `version` or pinning none.
    fn pack(&self, name: &str, version: Option<&str>) {
        let mut manifest = format!(
            "[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n\
             description = \"a scratch pack\"\n"
        );
        if let Some(version) = version {
            manifest.push_str(&format!(
                "\n[runtime]\nname = \"{RUNTIME}\"\nversion = \"{version}\"\n\
                 bundle = \"cp {{entry}} {{bundle}}\"\nrun = \"sh {{bundle}}\"\n"
            ));
        }
        write(&self.pack_dir(name).join("pack.toml"), &manifest);
    }

    fn pack_dir(&self, name: &str) -> PathBuf {
        self.machine.join("packs").join(name)
    }

    /// A check `name` in pack `pack`, whose script is `body`.
    fn check(&self, pack: &str, name: &str, body: &str) {
        let entry = self.pack_dir(pack).join("doctor").join(name);
        write(
            &entry.join("doctor.toml"),
            &format!("description = \"the {name} check\"\nrun = \"run.sh\"\n"),
        );
        write(&entry.join("run.sh"), &format!("#!/bin/sh\n{body}\n"));
    }

    fn uncheck(&self, pack: &str, name: &str) {
        std::fs::remove_dir_all(self.pack_dir(pack).join("doctor").join(name))
            .expect("the check is removed");
    }

    /// Move the runtime stub out of the rig's own bin and into the installer's
    /// bin under this rig's `HOME`, where no caller's `PATH` reaches.
    fn runtime_only_in_the_installers_bin(&self) {
        let moved = self
            .root
            .join("home")
            .join(format!(".{RUNTIME}"))
            .join("bin")
            .join(RUNTIME);
        write(
            &moved,
            &format!("#!/bin/sh\necho \"{RUNTIME} {VERSION}\"\n"),
        );
        executable(&moved);
        std::fs::remove_file(self.stubs.join(RUNTIME)).expect("the rig's own bin gives it up");
    }

    fn doctor(&self, args: &[&str]) -> Output {
        self.doctor_in(&self.project, args)
    }

    fn doctor_in(&self, cwd: &Path, args: &[&str]) -> Output {
        let fleet = PathBuf::from(env!("CARGO_BIN_EXE_fleet"));
        let fleet_dir = fleet.parent().expect("the binary sits in a directory");
        let mut path = format!("{}:{}", self.stubs.display(), fleet_dir.display());
        if let Ok(held) = std::env::var("PATH") {
            path = format!("{path}:{held}");
        }
        Command::new(&fleet)
            .arg("doctor")
            .args(args)
            .arg("--packs-dir")
            .arg(self.machine.join("packs"))
            .current_dir(cwd)
            .hermetic(
                &self.root.join("home"),
                &self.machine,
                Some(&self.stubs.join("claude")),
            )
            .env("NO_COLOR", "1")
            .env_remove("CLAUDE_SECURESTORAGE_CONFIG_DIR")
            // bd-version asks the binary this names before any bd on PATH, and
            // the store `fleet item list` opens asks nothing else: the stub,
            // so neither reaches a real bd.
            .env("FLEET_BD_BIN", self.stubs.join("bd"))
            .env("PATH", path)
            .output()
            .expect("the built binary runs")
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

/// The row lines: every line that is not a follow-on two-space line.
fn rows(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.starts_with("  "))
        .collect()
}

/// The one row whose subject is `subject`: `<name> (<layer>)`, or
/// `<name> for <pack> (<layer>)` on a runtime-version row.
fn row<'a>(text: &'a str, subject: &str) -> &'a str {
    let found: Vec<&str> = rows(text)
        .into_iter()
        .filter(|line| {
            line.split_once(" — ")
                .map(|(head, _)| head.ends_with(&format!(" {subject}")))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(found.len(), 1, "one row for {subject}: {text}");
    found[0]
}

/// The two-space lines that follow the row for `subject`.
fn followed_by<'a>(text: &'a str, subject: &str) -> Vec<&'a str> {
    let head = row(text, subject);
    text.lines()
        .skip_while(|line| *line != head)
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .collect()
}

fn last_line(text: &str) -> &str {
    text.lines().last().unwrap_or("")
}

// ---- 1. the defaults, all green ---------------------------------------------

/// The control every other arm is read against: the defaults alone, over a
/// project whose guards are configured, every check passing.
#[test]
fn the_defaults_pass_on_a_configured_project() {
    let rig = Rig::new("green", CONFIGURED);
    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));

    let rows = rows(&said);
    assert_eq!(
        rows.len(),
        DEFAULTS.len() + 1,
        "a row per check and the summary: {said}"
    );
    for (line, name) in rows.iter().zip(DEFAULTS) {
        assert!(
            line.starts_with(&format!("pass {name} (defaults) — ")),
            "{name} passes, in name order: {said}"
        );
    }
    assert_eq!(
        row(&said, "guards-installed (defaults)"),
        "pass guards-installed (defaults) — record bare-id: configured — [project] item_prefix"
    );
    assert_eq!(
        row(&said, "isolation-pair (defaults)"),
        "pass isolation-pair (defaults) — isolation-pair: holds"
    );
    assert_eq!(row(&said, "runtime-version (defaults)"), NOTHING_PINNED);
    assert_eq!(
        row(&said, "adopt-board (defaults)"),
        "pass adopt-board (defaults) — adopt-board: nothing to adopt — no items read"
    );
    assert_eq!(
        row(&said, "fleet-packs-version (defaults)"),
        format!(
            "pass fleet-packs-version (defaults) — fleet-packs-version: nothing installed from \
             fleet-packs — no line of the lock names {}",
            fleet_core::supported::PINNED_PACKS_SOURCE
        )
    );
    assert_eq!(
        last_line(&said),
        "doctor 7 checks — 7 pass, 0 finding, 0 could not tell"
    );
    assert!(
        !said.lines().any(|line| line.starts_with("  ")),
        "a passing row is followed by nothing: {said}"
    );
}

// ---- 2. a finding -----------------------------------------------------------

/// The record guard with no prefix: guards-installed reports it, the lines the
/// person acts on follow its row, and the rest still pass.
#[test]
fn an_unconfigured_guard_is_a_finding_with_its_lines_below_the_row() {
    let rig = Rig::new("finding", UNCONFIGURED);
    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{said}{}", stderr(&out));

    assert!(
        row(&said, "guards-installed (defaults)")
            .starts_with("finding guards-installed (defaults) — "),
        "{said}"
    );
    assert!(
        followed_by(&said, "guards-installed (defaults)")
            .iter()
            .any(|line| line.contains("record bare-id: not configured")),
        "the --check lines follow the row: {said}"
    );
    for name in DEFAULTS.iter().filter(|name| **name != "guards-installed") {
        assert!(
            row(&said, &format!("{name} (defaults)")).starts_with("pass "),
            "{name} still passes: {said}"
        );
    }
    assert_eq!(
        last_line(&said),
        "doctor 7 checks — 6 pass, 1 finding, 0 could not tell"
    );
}

/// fleet-3krx.1 — the fleet-packs pin through the shipped binary: the check
/// reads the machine's lock through `fleet pack list`, so a line from the
/// pinned source at another tag is a finding naming the lines that replace it,
/// and the same line at the pin holds. The lock is written by hand, in the
/// shape `fleet pack add` writes it, so no pack is fetched.
#[test]
fn a_pack_from_fleet_packs_at_another_tag_is_a_finding() {
    let source = fleet_core::supported::PINNED_PACKS_SOURCE;
    let pin = fleet_core::supported::PINNED_PACKS;
    let rig = Rig::new("packs-pin", CONFIGURED);
    let lock = |version: &str| {
        write(
            &rig.machine.join("packs.lock"),
            &format!(
                "schema = 1\n\n[packs.\"{source}//adapters/store/bd\"]\nname = \"bd\"\n\
                 version = \"{version}\"\ncommit = \"{commit}\"\nfetched = \"2026-09-25T00:00:00Z\"\n",
                commit = "0".repeat(40)
            ),
        )
    };

    lock("v0.0.9");
    let out = rig.doctor(&["fleet-packs-version"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{said}{}", stderr(&out));
    assert!(
        row(&said, "fleet-packs-version (defaults)").starts_with(
            "finding fleet-packs-version (defaults) — fleet-packs-version: broken — 1 pack(s)"
        ),
        "{said}"
    );
    assert!(
        followed_by(&said, "fleet-packs-version (defaults)")
            .iter()
            .any(|line| line.contains(&format!(
                "bd is pinned at v0.0.9, not the supported {pin} — `fleet pack remove \
                 {source}//adapters/store/bd`"
            ))),
        "the pack that moved is named below the row: {said}"
    );

    lock(pin);
    let out = rig.doctor(&["fleet-packs-version"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        row(&said, "fleet-packs-version (defaults)"),
        format!("pass fleet-packs-version (defaults) — fleet-packs-version: holds — bd at {pin}")
    );
}

// ---- 3. could-not-tell wins -------------------------------------------------

/// A check that could not tell outweighs one that found something, and a check
/// that exits anything past 1 could not tell. The red proof is the same rig
/// with the two could-not-tells taken out: the finding is then the answer.
#[test]
fn a_check_that_could_not_tell_wins_over_a_finding() {
    let rig = Rig::new("unknown", CONFIGURED);
    rig.pack("scratch", None);
    rig.check("scratch", "fx-unread", "echo unreadable >&2\nexit 3");
    rig.check("scratch", "fx-found", "echo found\nexit 1");
    rig.check("scratch", "fx-seven", "exit 7");

    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "{said}{}", stderr(&out));
    assert_eq!(
        row(&said, "fx-unread (scratch)"),
        "could not tell fx-unread (scratch) — unreadable"
    );
    assert_eq!(followed_by(&said, "fx-unread (scratch)"), ["  unreadable"]);
    assert_eq!(
        row(&said, "fx-found (scratch)"),
        "finding fx-found (scratch) — found"
    );
    assert_eq!(
        row(&said, "fx-seven (scratch)"),
        "could not tell fx-seven (scratch) — said nothing and exited 7"
    );
    assert_eq!(
        last_line(&said),
        "doctor 10 checks — 7 pass, 1 finding, 2 could not tell"
    );

    rig.uncheck("scratch", "fx-unread");
    rig.uncheck("scratch", "fx-seven");
    let out = rig.doctor(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
}

// ---- 4. shadowing -----------------------------------------------------------

/// A pack's entry replaces the defaults' of the same name, and the row names
/// the pack. The red proof is the entry taken out: the defaults' check runs,
/// and on this project it is a finding.
#[test]
fn a_packs_entry_shadows_the_defaults_of_the_same_name() {
    let rig = Rig::new("shadow", UNCONFIGURED);
    rig.pack("scratch", None);
    rig.check("scratch", "guards-installed", "echo shadowed-by-scratch");

    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        row(&said, "guards-installed (scratch)"),
        "pass guards-installed (scratch) — shadowed-by-scratch"
    );
    assert!(
        !said.contains("shell-trap"),
        "the defaults' check did not run: {said}"
    );

    rig.uncheck("scratch", "guards-installed");
    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        row(&said, "guards-installed (defaults)").starts_with("finding "),
        "{said}"
    );
}

// ---- 5. runtime-version, per pinned pack ------------------------------------

/// runtime-version runs once for the pack that pins a runtime, measuring that
/// pack's pin, and a pack that pins none gets no row of its own.
#[test]
fn runtime_version_runs_once_for_each_pinned_pack() {
    let rig = Rig::new("pinned", CONFIGURED);
    rig.pack("scratch", Some(VERSION));
    rig.pack("plain", None);

    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        row(&said, "runtime-version for scratch (defaults)"),
        "pass runtime-version for scratch (defaults) — runtime-version: holds"
    );
    assert!(
        !said.contains("for plain"),
        "a pack pinning nothing: {said}"
    );
    assert!(!said.contains("nothing pinned"), "{said}");
    assert_eq!(
        rows(&said)
            .iter()
            .filter(|line| line.contains(" runtime-version "))
            .count(),
        1,
        "{said}"
    );
}

/// The same pack pinned at a version the runtime does not print.
#[test]
fn a_runtime_off_its_pin_is_a_finding() {
    let rig = Rig::new("repinned", CONFIGURED);
    rig.pack("scratch", Some("9.9.9"));

    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{said}{}", stderr(&out));
    assert!(
        row(&said, "runtime-version for scratch (defaults)").starts_with(
            "finding runtime-version for scratch (defaults) — runtime-version: broken"
        ),
        "{said}"
    );
}

/// The runtime only in the installer's bin, which the caller's `PATH` does not
/// reach: the row passes because it runs on the `PATH` a run gives the pack's
/// lines, which puts that bin in front.
#[test]
fn runtime_version_runs_on_the_path_a_run_gives_the_pack() {
    let rig = Rig::new("installer", CONFIGURED);
    rig.pack("scratch", Some(VERSION));
    rig.runtime_only_in_the_installers_bin();

    let out = rig.doctor(&[]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        row(&said, "runtime-version for scratch (defaults)"),
        "pass runtime-version for scratch (defaults) — runtime-version: holds"
    );
}

// ---- 6. selection -----------------------------------------------------------

/// A named check runs alone.
#[test]
fn a_named_check_runs_alone() {
    let rig = Rig::new("one", CONFIGURED);
    let out = rig.doctor(&["isolation-pair"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        rows(&said),
        [
            "pass isolation-pair (defaults) — isolation-pair: holds",
            "doctor 1 check — 1 pass, 0 finding, 0 could not tell",
        ]
    );
}

/// A name given twice runs once.
#[test]
fn a_check_named_twice_runs_once() {
    let rig = Rig::new("twice", CONFIGURED);
    let out = rig.doctor(&["runtime-version", "runtime-version"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        rows(&said),
        [
            NOTHING_PINNED,
            "doctor 1 check — 1 pass, 0 finding, 0 could not tell",
        ]
    );
}

/// A name no layer carries is usage, said before anything runs: the check
/// that would leave a marker has not left it. Its control is the same check
/// named alone, which does.
#[test]
fn a_name_no_layer_carries_is_usage_and_nothing_runs() {
    let rig = Rig::new("nosuch", CONFIGURED);
    let marker = rig.root.join("marker");
    rig.pack("scratch", None);
    rig.check(
        "scratch",
        "fx-marker",
        &format!("touch '{}'", marker.display()),
    );

    let out = rig.doctor(&["nosuch"]);
    let said = stderr(&out);
    assert_eq!(out.status.code(), Some(2), "{said}");
    assert!(said.contains("nosuch"), "{said}");
    let carried = said
        .split_once("the layers carry: ")
        .map(|(_, list)| list.trim())
        .unwrap_or_else(|| panic!("the refusal lists the checks: {said}"));
    for name in DEFAULTS {
        assert!(carried.contains(name), "{name} is listed: {said}");
    }
    assert!(stdout(&out).is_empty(), "no row printed: {}", stdout(&out));
    assert!(!marker.exists(), "no check ran");

    let out = rig.doctor(&["fx-marker"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(marker.exists(), "the control: the check leaves its marker");
}

// ---- 7. --json --------------------------------------------------------------

/// The finding under --json: one document on stdout, `ok` true because the
/// report is the outcome, the aggregate in `data.verdict` and on the exit, and
/// the human rows moved to stderr.
#[test]
fn json_carries_every_row_and_the_exit_carries_the_aggregate() {
    let rig = Rig::new("json", UNCONFIGURED);
    let out = rig.doctor(&["--json"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{said}{}", stderr(&out));
    assert_eq!(said.lines().count(), 1, "one line: {said}");
    let document: serde_json::Value = serde_json::from_str(&said).expect("the document parses");

    assert_eq!(document["ok"], true);
    assert_eq!(document["verb"], "doctor");
    assert_eq!(document["data"]["verdict"], "finding");
    assert_eq!(
        document["data"]["counts"],
        serde_json::json!({ "pass": 6, "finding": 1, "could_not_tell": 0 })
    );
    let checks = document["data"]["checks"]
        .as_array()
        .expect("checks is an array");
    assert_eq!(checks.len(), DEFAULTS.len(), "{said}");
    let keys = [
        "description",
        "exit",
        "layer",
        "line",
        "measures",
        "name",
        "stderr",
        "stdout",
        "verdict",
    ];
    for check in checks {
        let object = check.as_object().expect("a check is an object");
        let mut held: Vec<&str> = object.keys().map(String::as_str).collect();
        held.sort_unstable();
        assert_eq!(held, keys, "{check}");
    }
    let guards = checks
        .iter()
        .find(|check| check["name"] == "guards-installed")
        .expect("guards-installed is a row");
    assert_eq!(guards["exit"], 1);
    assert_eq!(guards["verdict"], "finding");
    assert_eq!(guards["layer"], "defaults");
    assert!(
        guards["stdout"]
            .as_str()
            .is_some_and(|text| text.contains("record bare-id: not configured")),
        "{guards}"
    );
    let runtime = checks
        .iter()
        .find(|check| check["name"] == "runtime-version")
        .expect("runtime-version is a row");
    assert_eq!(runtime["exit"], serde_json::Value::Null);
    assert_eq!(runtime["measures"], serde_json::Value::Null);

    let aside = stderr(&out);
    assert!(
        aside.contains("finding guards-installed (defaults) — "),
        "the rows are on stderr: {aside}"
    );
}

/// No project under --json: the verb's own refusal, not a report.
#[test]
fn json_with_no_project_is_a_could_not_tell_refusal() {
    let rig = Rig::new("json-nowhere", CONFIGURED);
    let elsewhere = rig.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("the directory is made");
    let out = rig.doctor_in(&elsewhere, &["--json"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "{said}{}", stderr(&out));
    let document: serde_json::Value = serde_json::from_str(&said).expect("the document parses");
    assert_eq!(document["ok"], false);
    assert_eq!(document["verb"], "doctor");
    assert_eq!(document["refusal"]["code"], "could_not_tell");
}

/// A name no layer carries under --json: the usage refusal.
#[test]
fn json_with_a_name_no_layer_carries_is_a_usage_refusal() {
    let rig = Rig::new("json-nosuch", CONFIGURED);
    let out = rig.doctor(&["nosuch", "--json"]);
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(2), "{said}{}", stderr(&out));
    let document: serde_json::Value = serde_json::from_str(&said).expect("the document parses");
    assert_eq!(document["ok"], false);
    assert_eq!(document["refusal"]["code"], "usage");
}

// ---- 8. no project ----------------------------------------------------------

#[test]
fn no_project_is_could_not_tell() {
    let rig = Rig::new("nowhere", CONFIGURED);
    let elsewhere = rig.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("the directory is made");
    let out = rig.doctor_in(&elsewhere, &[]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).starts_with("fleet doctor: no `fleet.toml`"),
        "{}",
        stderr(&out)
    );
}

// ---- 9. help ----------------------------------------------------------------

#[test]
fn the_help_says_the_exit_and_the_root_lists_doctor() {
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["doctor", "--help"])
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("3 wins over 1"), "{}", stdout(&out));

    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .arg("--help")
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    let page = stdout(&out);
    let listed = page
        .split_once("Commands:")
        .map(|(_, commands)| commands)
        .unwrap_or_default();
    assert!(
        listed
            .lines()
            .any(|line| line.split_whitespace().next() == Some("doctor")),
        "`fleet --help` lists doctor: {page}"
    );
}
