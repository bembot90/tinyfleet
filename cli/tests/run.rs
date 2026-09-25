//! `fleet run` through the shipped binary, on a scratch pack that pins a
//! runtime this rig puts on `PATH` itself.
//!
//! ONE BOARD, AND ONE ARM ALONE ON ITS OWN. The open runs are a query across
//! the whole work graph, so the arm that measures the cap would be counting
//! whatever its neighbours left open: it takes a board nobody else writes to.
//! Every other arm reads its own stream, its own run directory and its own
//! record by id, none of which a neighbour's rows move, so they share the run's
//! one board and pay a copy instead of a `bd init`. The machine directory and
//! the project stay each arm's own either way; the board is the whole of what
//! is shared.
//!
//! A WORKFLOW NAME BEGINS WITH ITS RIG'S LABEL, which is what keeps one arm's
//! records legible on a board holding every other arm's.
//!
//! THE WORKFLOW IS THE ARM'S OWN SCRIPT. The pack's bundle command copies its
//! entry verbatim and its run command is `sh {bundle}`, so an arm writes the
//! process the exit table will read and nothing stands between the two.
//!
//! THE RUNTIME IS A STUB THE RIG WRITES, not a language on the box: the check
//! core ships asks the pinned binary for its version and compares whole tokens,
//! so a stub that prints one line is the whole of what a green doctor needs —
//! and a pin at a version the stub does not print is the whole of a red one.
//! Nothing here measures Deno, or any real runtime, and nothing here should.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::hermetic::Hermetic;
use fleet_core::item::pins;
use fleet_core::item::run as workflow_run;
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::store::StoreError;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Who starts the runs here: a seat typed whole, which a verb takes as given
/// — these rigs write policies of their own and list no roster to resolve a
/// name over.
const BY: &str = "seat:01a0d1f1-0aec-765f-9abe-000000001ead";
/// Who clears and cancels a run by hand: another seat, typed the same way.
const PERSON: &str = "seat:01a0d1f1-0aec-765f-9abe-00000000fe25";

/// The controller's run pass, as it acts: under a machine's identity.
fn the_controller() -> Actor {
    Actor {
        kind: ActorKind::Controller,
        id: String::from("01a0d1f1-0aec-765f-9abe-00000000c0de"),
    }
}

/// The runtime the scratch pack pins, and the version its stub prints.
const RUNTIME: &str = "fx-runtime";
const VERSION: &str = "1.2.3";

/// The workflow's own bytes, which the bundle command copies verbatim — so the
/// arm can tell a bundle that was written from one that was not.
const WORKFLOW: &str = "#!/bin/sh\necho the scratch workflow\n";

/// The row a pack with one workflow carries it under.
const ONE: &str = "hello";

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

/// Which board a rig runs on.
enum Board {
    /// The run's one board, copied in.
    Shared,
    /// A `bd init` of this rig's own, for an arm whose subject is a reading
    /// across the whole graph.
    OfItsOwn,
}

/// What the scratch pack's manifest declares and which workflows it carries, so
/// one rig covers the refusals that are about the manifest, the one that is
/// not, and every row of the exit table.
struct Pack {
    /// `None` writes no `[runtime]` table at all.
    runtime: Option<&'static str>,
    /// One entry per `workflows/<rig label>-<row>.sh`. The pack's bundle
    /// command copies the bytes verbatim and its run command is `sh {bundle}`,
    /// so these ARE the processes the exit table reads.
    workflows: Vec<(&'static str, String)>,
    /// Packs installed beside the scratch pack and declared in its
    /// `[imports]`, each carrying no file at all and a `[runtime]` table at
    /// the version given, or none. The layering sorts them beneath the
    /// scratch pack, which is where a run looks for a pin the carrier lacks.
    imports: Vec<(&'static str, Option<&'static str>)>,
    /// Packs declared in the scratch pack's `[imports]` and not installed.
    absent: Vec<&'static str>,
    /// The manifest's `[config.<key>]` declarations, appended verbatim.
    settings: &'static str,
    /// Whether the pack's bundle command exits non-zero and writes nothing,
    /// which is the one refusal a run meets AFTER its record is filed.
    bundle_fails: bool,
}

impl Pack {
    fn pinned_at(version: &'static str) -> Pack {
        Pack {
            runtime: Some(version),
            workflows: vec![(ONE, String::from(WORKFLOW))],
            imports: Vec::new(),
            absent: Vec::new(),
            settings: "",
            bundle_fails: false,
        }
    }

    fn without_a_runtime() -> Pack {
        Pack {
            runtime: None,
            workflows: vec![(ONE, String::from(WORKFLOW))],
            imports: Vec::new(),
            absent: Vec::new(),
            settings: "",
            bundle_fails: false,
        }
    }

    /// A green pack whose one workflow is the given script.
    fn running(body: &str) -> Pack {
        Pack::of_rows(&[(ONE, body)])
    }

    /// A green pack carrying one workflow per row, each its own script.
    fn of_rows(rows: &[(&'static str, &str)]) -> Pack {
        Pack {
            runtime: Some(VERSION),
            workflows: rows
                .iter()
                .map(|(row, body)| (*row, format!("#!/bin/sh\n{body}\n")))
                .collect(),
            imports: Vec::new(),
            absent: Vec::new(),
            settings: "",
            bundle_fails: false,
        }
    }

    /// The same pack, importing one more pack that pins the runtime at
    /// `version`, or pins none.
    fn importing(mut self, name: &'static str, version: Option<&'static str>) -> Pack {
        self.imports.push((name, version));
        self
    }

    /// The same pack, importing one more pack that is not installed.
    fn importing_absent(mut self, name: &'static str) -> Pack {
        self.absent.push(name);
        self
    }

    /// The same pack, declaring the settings `fleet.toml` may set for it.
    fn declaring(mut self, settings: &'static str) -> Pack {
        self.settings = settings;
        self
    }

    /// The same pack, whose bundle command refuses.
    fn whose_bundle_fails(mut self) -> Pack {
        self.bundle_fails = true;
        self
    }
}

struct Rig {
    label: String,
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    stubs: PathBuf,
}

impl Rig {
    /// A rig on the run's shared board.
    fn new(label: &str, pack: &Pack, policy: &str) -> Rig {
        Rig::on(Board::Shared, label, pack, policy)
    }

    /// A rig on a board of its own, for the arm that counts what the board
    /// holds open.
    fn alone(label: &str, pack: &Pack, policy: &str) -> Rig {
        Rig::on(Board::OfItsOwn, label, pack, policy)
    }

    fn on(board: Board, label: &str, pack: &Pack, policy: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-run-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            label: label.to_string(),
            project: root.join("project"),
            machine: root.join("machine"),
            stubs: root.join("stubs"),
            root,
        };
        std::fs::create_dir_all(&rig.machine).expect("the machine directory is made");
        std::fs::create_dir_all(&rig.project).expect("the project directory is made");
        defaults_into(&rig.machine);

        // The runtime, as one line on stdout. The check core ships reads the
        // first line and matches whole tokens, so this is the whole contract.
        let stub = rig.stubs.join(RUNTIME);
        write(&stub, &format!("#!/bin/sh\necho \"{RUNTIME} {VERSION}\"\n"));
        executable(&stub);

        let scratch = rig.machine.join("packs/scratch");
        let bundler = scratch.join("assets/bundle.sh");
        if pack.bundle_fails {
            write(
                &bundler,
                "#!/bin/sh\necho 'the bundler would not bundle' >&2\nexit 7\n",
            );
        } else {
            write(&bundler, "#!/bin/sh\nset -eu\ncp \"$1\" \"$2\"\n");
        }
        executable(&bundler);
        for (row, body) in &pack.workflows {
            write(
                &scratch.join(format!("workflows/{}.sh", rig.workflow(row))),
                body,
            );
        }

        let runtime_table = |version: &str| {
            format!(
                "\n[runtime]\nname = \"{RUNTIME}\"\nversion = \"{version}\"\n\
                 bundle = \"sh {} {{entry}} {{bundle}}\"\nrun = \"sh {{bundle}}\"\n",
                bundler.display()
            )
        };
        let mut manifest = String::from(
            "[pack]\nname = \"scratch\"\nversion = \"0.1.0\"\nschema = 3\n\
             description = \"a scratch pack\"\n",
        );
        for name in pack
            .imports
            .iter()
            .map(|(name, _)| name)
            .chain(&pack.absent)
        {
            manifest.push_str(&format!(
                "\n[imports.{name}]\nsource = \"../{name}\"\nversion = \"0.1.0\"\n"
            ));
        }
        if let Some(version) = pack.runtime {
            manifest.push_str(&runtime_table(version));
        }
        manifest.push_str(pack.settings);
        write(&scratch.join("pack.toml"), &manifest);
        for (name, version) in &pack.imports {
            let mut imported = format!(
                "[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n\
                 description = \"a pack the scratch pack imports\"\n"
            );
            if let Some(version) = version {
                imported.push_str(&runtime_table(version));
            }
            write(
                &rig.machine.join(format!("packs/{name}/pack.toml")),
                &imported,
            );
        }

        write(&rig.project.join("fleet.toml"), policy);

        match board {
            Board::Shared => common::take_a_board(&rig.project, "run"),
            Board::OfItsOwn => {
                let init = Command::new("bd")
                    .args(["init", "--prefix", "fx", "--quiet"])
                    .args(common::bd_init_server_args("run"))
                    .current_dir(&rig.project)
                    .output()
                    .expect("bd is on the process PATH");
                assert!(
                    init.status.success(),
                    "bd init: {}",
                    String::from_utf8_lossy(&init.stderr)
                );
            }
        }
        rig
    }

    /// The name one of this rig's workflows answers to. The label leads, so a
    /// board holding every arm's records still says which arm filed which.
    fn workflow(&self, row: &str) -> String {
        format!("{}-{row}", self.label)
    }

    /// Take the runtime stub out of the directory this rig puts on the fleet
    /// process's `PATH` and put it in the installer's bin under this rig's own
    /// `HOME` — where a runtime's doctor check looks second and no session's
    /// `PATH` reaches. Answers the directory it now sits in.
    fn runtime_only_in_the_installers_bin(&self) -> PathBuf {
        let bin = self
            .root
            .join("home")
            .join(format!(".{RUNTIME}"))
            .join("bin");
        let moved = bin.join(RUNTIME);
        write(
            &moved,
            &format!("#!/bin/sh\necho \"{RUNTIME} {VERSION}\"\n"),
        );
        executable(&moved);
        std::fs::remove_file(self.stubs.join(RUNTIME)).expect("the rig's own bin gives it up");
        bin
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, &[])
    }

    /// The same, with `environment` set on the fleet process AFTER the
    /// hermetic block, for an arm about what the run child inherits from it.
    fn run_with(&self, args: &[&str], environment: &[(&str, &str)]) -> Output {
        let path = match std::env::var("PATH") {
            Ok(held) => format!("{}:{held}", self.stubs.display()),
            Err(_) => self.stubs.display().to_string(),
        };
        self.run_on_path(args, &path, environment)
    }

    /// The same, on a `PATH` the arm names outright, for the arm about what
    /// the pack's own children search when the fleet process carries a
    /// service's minimal one.
    fn run_on_path(&self, args: &[&str], path: &str, environment: &[(&str, &str)]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .arg("--packs-dir")
            .arg(self.machine.join("packs"))
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .envs(environment.iter().copied())
            .output()
            .expect("the built binary runs")
    }

    /// Every event the stream holds, newest last.
    fn events(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("an event is one JSON object"))
            .collect()
    }

    /// How many lines the stream holds, which is the cursor one run's own
    /// events are read from.
    fn stream_length(&self) -> usize {
        self.events().len()
    }

    /// The machine directory as a listing of every file under it with its size.
    ///
    /// SIZE AND NOT JUST NAME: a refusal that rewrote a file already there
    /// would leave the name listing identical, and the stream is the file most
    /// likely to be appended to.
    fn machine_listing(&self) -> Vec<String> {
        let mut found = Vec::new();
        walk(&self.machine, &self.machine, &mut found);
        found.sort();
        found
    }

    fn document(&self, item: &str) -> serde_json::Value {
        let out = Command::new("bd")
            .arg("-C")
            .arg(&self.project)
            .args(["show", item, "--json"])
            .output()
            .expect("bd runs");
        assert!(
            out.status.success(),
            "bd show {item}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let value: serde_json::Value =
            serde_json::from_str(text.trim()).expect("bd show answers JSON");
        match value {
            serde_json::Value::Array(mut rows) if !rows.is_empty() => rows.remove(0),
            other => other,
        }
    }
}

fn walk(root: &Path, dir: &Path, into: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, into);
        } else {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let relative = path.strip_prefix(root).unwrap_or(&path);
            into.push(format!("{} {size}", relative.display()));
        }
    }
}

fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is made executable");
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

/// A policy with a run cap.
fn policy_with(max_open: u64) -> String {
    format!("[core.run]\nmax_open = {max_open}\n")
}

/// The cap an arm that is not measuring one runs under. The board is shared, so
/// the open runs counted against it are every arm's — a cap low enough to be
/// reached would turn one arm's leftovers into another arm's refusal.
fn cap_that_is_not_the_subject() -> String {
    policy_with(1000)
}

/// The verb prints two lines: `<id> — <hash>` when the run is pinned, and
/// `<id> — <outcome>` when its process has answered.
fn started_line(out: &Output) -> (String, String) {
    let text = stdout(out);
    let first = text
        .lines()
        .next()
        .unwrap_or_else(|| panic!("the verb prints a first line: {text}"));
    let (id, hash) = first
        .split_once(" — ")
        .unwrap_or_else(|| panic!("the run line is `<id> — <hash>`: {first}"));
    (id.to_string(), hash.to_string())
}

fn outcome_line(out: &Output) -> String {
    let text = stdout(out);
    let second = text
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("the verb prints an outcome line: {text}"));
    let (_, word) = second
        .split_once(" — ")
        .unwrap_or_else(|| panic!("the outcome line is `<id> — <outcome>`: {second}"));
    word.to_string()
}

/// The cursor a rig that ran one workflow reads from: its stream holds that
/// run's lines and nothing else.
const WHOLE_STREAM: usize = 0;

/// The one event of a kind among the lines appended from `from` onward, and a
/// panic where there is not exactly one — so an arm that reads a payload has
/// already asserted that no second row of the table fired for that run.
fn only(rig: &Rig, from: usize, kind: &str) -> serde_json::Value {
    let found: Vec<serde_json::Value> = rig
        .events()
        .into_iter()
        .skip(from)
        .filter(|event| event["type"].as_str() == Some(kind))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one {kind} since line {from}: {found:?}"
    );
    found.into_iter().next().expect("the one event")
}

fn none_of(rig: &Rig, from: usize, kind: &str) {
    let found: Vec<serde_json::Value> = rig
        .events()
        .into_iter()
        .skip(from)
        .filter(|event| event["type"].as_str() == Some(kind))
        .collect();
    assert!(
        found.is_empty(),
        "no {kind} was written since line {from}: {found:?}"
    );
}

/// Whether the store still counts the run among the open ones.
fn is_open(rig: &Rig, id: &str) -> bool {
    rig.document(id)["status"].as_str() == Some("open")
}

// ---- AC1: the front half ------------------------------------------------------

/// The run directory, the record item's hash and `run.started`'s hash — the
/// three readings the front half claims, asserted equal against a fourth this
/// arm computes off the disk itself.
///
/// THE EVENT IS READ OFF THE STREAM FILE. `fleet event tail --json` is the
/// reading the spec named and that flag is not on this trunk; the file is what
/// the flag would print, and the payload is the same object either way.
#[test]
fn the_front_half_pins_the_inputs_the_bundle_and_one_hash_over_all_three() {
    let rig = Rig::new(
        "front",
        &Pack::pinned_at(VERSION),
        &cap_that_is_not_the_subject(),
    );
    let workflow = rig.workflow(ONE);

    let out = rig.run(&["run", &workflow, "--input", "who=the-arm", "--by", BY]);
    assert!(
        out.status.success(),
        "fleet run: {}\n{}",
        stdout(&out),
        stderr(&out)
    );

    // The FIRST line the verb prints is the id and the hash; the second is the
    // outcome, which this arm does not read.
    let (id, hash) = started_line(&out);
    let (id, hash) = (id.as_str(), hash.as_str());

    // The directory, under the machine directory, holding the three names.
    let directory = rig.machine.join(workflow_run::RUNS).join(id);
    for name in workflow_run::hashed_files() {
        assert!(
            directory.join(&name).is_file(),
            "the run directory holds {name}: {}",
            directory.display()
        );
    }

    // The policy in force, byte for byte, and the inputs as the caller gave
    // them.
    assert_eq!(
        std::fs::read(directory.join(workflow_run::POLICY)).expect("the snapshot is readable"),
        std::fs::read(rig.project.join("fleet.toml")).expect("the policy file is readable"),
        "the policy snapshot is a copy and not a re-render"
    );
    let pinned = fleet_core::item::read_table(&directory.join(workflow_run::INPUTS))
        .expect("the inputs parse as TOML");
    assert_eq!(
        pinned["inputs"]["who"].as_str(),
        Some("the-arm"),
        "the pair the caller gave is pinned: {pinned}"
    );
    assert_eq!(pinned["workflow"].as_str(), Some(workflow.as_str()));
    assert_eq!(pinned["pack"].as_str(), Some("scratch"));

    // The bundle is what the pack's own command wrote, and this pack's command
    // copies its entry.
    assert_eq!(
        std::fs::read_to_string(directory.join(workflow_run::BUNDLE))
            .expect("the bundle is readable"),
        WORKFLOW,
        "the bundle is the entry the pack's command copied"
    );

    // THE THREE HASHES. The arm's own recomputation over the same files through
    // the same digest, the record item's, and the event's.
    let recomputed = pins::hash_of(&directory, &workflow_run::hashed_files())
        .expect("the arm recomputes the hash");
    assert_eq!(recomputed, hash, "the hash is of the files on disk");

    let record = rig.document(id);
    assert_eq!(
        record["metadata"]["fleet.run"]["hash"].as_str(),
        Some(hash),
        "the hash is on the record item: {record}"
    );
    assert_eq!(
        record["metadata"]["fleet.run"]["workflow"].as_str(),
        Some(workflow.as_str())
    );
    assert_eq!(
        record["metadata"]["fleet.run"]["v"].as_u64(),
        Some(fleet_core::store::keys::VERSION),
        "the run's object carries its version: {record}"
    );
    assert_eq!(
        record["issue_type"].as_str(),
        Some(workflow_run::RECORD_TYPE)
    );
    assert!(
        record["labels"]
            .as_array()
            .expect("the labels are an array")
            .iter()
            .any(|label| label.as_str() == Some(workflow_run::LABEL)),
        "the record carries the run label: {record}"
    );

    let started = only(&rig, WHOLE_STREAM, fleet_core::item::RUN_STARTED);
    let payload = &started["payload"];
    assert_eq!(payload["run"].as_str(), Some(id));
    assert_eq!(payload["hash"].as_str(), Some(hash));
    assert_eq!(payload["workflow"].as_str(), Some(workflow.as_str()));

    // The digest is deterministic, and one byte of one file moves it — which is
    // what says the hash is over the BYTES and not over the three names.
    assert_eq!(
        pins::hash_of(&directory, &workflow_run::hashed_files()).expect("a second reading"),
        recomputed,
        "the same files answer the same string"
    );
    let mut body = std::fs::read(directory.join(workflow_run::BUNDLE)).expect("the bundle is read");
    body.push(b'\n');
    std::fs::write(directory.join(workflow_run::BUNDLE), &body).expect("one byte is added");
    assert_ne!(
        pins::hash_of(&directory, &workflow_run::hashed_files()).expect("a third reading"),
        recomputed,
        "one byte of the bundle changes the hash"
    );
}

/// The payload table core declares for the kind, against the payload the verb
/// wrote: a key invented and a key dropped are the same defect.
#[test]
fn run_started_declares_the_keys_it_carries() {
    assert_eq!(
        fleet_core::item::payload_keys(fleet_core::item::RUN_STARTED),
        Some(["run", "hash", "workflow"].as_slice()),
        "the kind's payload table is where the writer and a fold agree"
    );
}

// ---- AC2: the four refusals ---------------------------------------------------

/// What every refusal arm asserts: the exit, the sentence, and a machine
/// directory identical before and after.
fn refuses(rig: &Rig, args: &[&str], expected: &str) -> String {
    let before = rig.machine_listing();
    let out = rig.run(args);
    assert!(
        !out.status.success(),
        "the call is refused: {}",
        stdout(&out)
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "a refusal is exit 1: {}",
        stderr(&out)
    );
    let said = stderr(&out);
    assert!(
        said.contains(expected),
        "the refusal names `{expected}`: {said}"
    );
    assert_eq!(
        rig.machine_listing(),
        before,
        "a refusal writes nothing under the machine directory"
    );
    said
}

#[test]
fn an_unknown_workflow_refuses_by_name_and_writes_nothing() {
    let rig = Rig::new(
        "unknown",
        &Pack::pinned_at(VERSION),
        &cap_that_is_not_the_subject(),
    );
    let said = refuses(
        &rig,
        &["run", "nowhere", "--by", BY],
        "no workflow named `nowhere`",
    );
    assert!(
        said.contains("workflows/nowhere."),
        "the refusal names the path it looked for: {said}"
    );
    // The control: the SAME rig resolves the workflow that is there, so what
    // was refused is the name and not the resolution.
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn a_pack_with_no_runtime_table_refuses_by_name_and_writes_nothing() {
    let rig = Rig::new(
        "no-runtime",
        &Pack::without_a_runtime(),
        &cap_that_is_not_the_subject(),
    );
    refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "declares no [runtime] table",
    );
}

/// fleet-4fw: the carrier's runtime would come from a pack it imports, and that
/// pack is not installed. The refusal names the import and the line that adds
/// it, read off the carrier's own line in the lock.
#[test]
fn a_carrier_whose_import_is_not_installed_refuses_naming_the_line_that_adds_it() {
    let rig = Rig::new(
        "absent-import",
        &Pack::without_a_runtime().importing_absent("ts"),
        &cap_that_is_not_the_subject(),
    );
    fleet_core::lock::write(
        &rig.machine.join(fleet_core::lock::LOCK),
        &[fleet_core::lock::Entry {
            source: "https://example.invalid/o/fleet//packs/scratch".into(),
            name: Some("scratch".into()),
            version: "main".into(),
            commit: "0".repeat(40),
            fetched: "2026-09-23T00:00:00Z".into(),
            tree: None,
        }],
    )
    .expect("the lock is written");
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "`scratch` imports `ts`, which is not installed",
    );
    assert!(
        said.contains("fleet pack add https://example.invalid/o/fleet//packs/ts --version main"),
        "the refusal names the line that adds it: {said}"
    );
    assert!(
        !said.contains("core"),
        "the binary's bottom layer is not `core` on a person's surface: {said}"
    );
}

#[test]
fn a_red_doctor_refuses_and_writes_nothing() {
    // The stub prints VERSION; the pack pins another. That is the whole of the
    // red: the check core ships reads the first line and matches whole tokens.
    let rig = Rig::new(
        "red-doctor",
        &Pack::pinned_at("9.9.9"),
        &cap_that_is_not_the_subject(),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "runtime check is red",
    );
    assert!(
        said.contains("runtime-version: broken"),
        "the check's own lines reach the person: {said}"
    );
}

// ---- the runtime through the imports ------------------------------------------

/// A carrier with no `[runtime]` table runs on the pin of a pack it imports,
/// and stays the pack the run is pinned to: the file is the carrier's, the
/// runtime is the import's.
///
/// THE IMPORTED PACK CARRIES NO FILE, so the one thing the run can have taken
/// from it is the table — a bundle written and an outcome read are the whole
/// of the proof that the table was read from beneath the carrier.
#[test]
fn a_carrier_without_a_pin_runs_on_the_runtime_a_pack_it_imports_declares() {
    let rig = Rig::new(
        "imported-pin",
        &Pack::without_a_runtime().importing("lower", Some(VERSION)),
        &cap_that_is_not_the_subject(),
    );
    let workflow = rig.workflow(ONE);
    let out = rig.run(&["run", &workflow, "--by", BY]);
    assert!(
        out.status.success(),
        "fleet run: {}\n{}",
        stdout(&out),
        stderr(&out)
    );
    assert_eq!(outcome_line(&out), "closed");
    let (id, _) = started_line(&out);
    let directory = rig.machine.join(workflow_run::RUNS).join(&id);
    assert_eq!(
        std::fs::read_to_string(directory.join(workflow_run::BUNDLE))
            .expect("the bundle is readable"),
        WORKFLOW,
        "the import's bundle command wrote the carrier's entry"
    );
    let pinned = fleet_core::item::read_table(&directory.join(workflow_run::INPUTS))
        .expect("the inputs parse as TOML");
    assert_eq!(
        pinned["pack"].as_str(),
        Some("scratch"),
        "the carrier keeps owning the file: {pinned}"
    );
}

/// Two imports each declaring a pin, beneath a carrier declaring none, are a
/// refusal that names both — never the higher of the two.
#[test]
fn two_pins_beneath_the_carrier_refuse_naming_both_and_write_nothing() {
    let rig = Rig::new(
        "two-pins",
        &Pack::without_a_runtime()
            .importing("lower", Some(VERSION))
            .importing("other", Some(VERSION)),
        &cap_that_is_not_the_subject(),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "2 packs it imports each declare one",
    );
    assert!(
        said.contains("`lower`") && said.contains("`other`"),
        "the refusal names both packs: {said}"
    );
}

/// A carrier's own pin wins over an import's, in both directions: the import's
/// pin at a version the stub does not print is not read when the carrier
/// pins the printed one, and the carrier's pin at that version is read — and
/// red — when the import pins the printed one.
#[test]
fn a_carrier_with_its_own_pin_keeps_it_over_an_imports() {
    let rig = Rig::new(
        "own-pin",
        &Pack::pinned_at(VERSION).importing("lower", Some("9.9.9")),
        &cap_that_is_not_the_subject(),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert!(
        out.status.success(),
        "the carrier's own pin is the one measured: {}",
        stderr(&out)
    );
    assert_eq!(outcome_line(&out), "closed");

    let rig = Rig::new(
        "own-pin-red",
        &Pack::pinned_at("9.9.9").importing("lower", Some(VERSION)),
        &cap_that_is_not_the_subject(),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "`scratch`'s runtime check is red",
    );
    assert!(
        !said.contains("`lower`"),
        "the import's green pin is not consulted: {said}"
    );
}

/// The doctor measures the pack that DECLARES the pin. A check run against
/// the carrier would read its manifest, find nothing pinned, and answer green
/// about a runtime it never measured — so the import pins a version the stub
/// does not print, and the refusal has to be the red one, naming the import.
#[test]
fn the_doctor_measures_the_pack_that_declares_the_imported_pin() {
    let rig = Rig::new(
        "imported-red",
        &Pack::without_a_runtime().importing("lower", Some("9.9.9")),
        &cap_that_is_not_the_subject(),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "`lower`'s runtime check is red",
    );
    assert!(
        said.contains("runtime-version: broken"),
        "the check's own lines reach the person: {said}"
    );
}

/// The cap counts the runs the STORE still holds open, so the run this arm
/// leaves standing is a waiting one: a workflow that exits 0 closes its own
/// record and frees the cap, which is the second half of what this arm says.
///
/// A BOARD OF ITS OWN, because the count is across the whole graph: on the
/// board the arms beside this one share, whether the first of these two calls
/// is allowed would be decided by what they had left open.
#[test]
fn the_open_run_cap_refuses_naming_the_open_runs_and_writes_nothing() {
    let rig = Rig::alone("cap", &Pack::running(WAITS), &policy_with(1));
    let workflow = rig.workflow(ONE);
    let first = rig.run(&["run", &workflow, "--by", BY]);
    assert!(first.status.success(), "{}", stderr(&first));
    let (id, _) = started_line(&first);
    assert_eq!(outcome_line(&first), "waiting");

    let said = refuses(
        &rig,
        &["run", &workflow, "--by", BY],
        "`[core.run] max_open` is 1",
    );
    assert!(said.contains(&id), "the refusal names the open run: {said}");
}

/// The run directory's one entry, which is the record a run filed: the
/// directory is named by the id the store answered, and it is made before the
/// bundle command runs.
fn the_one_run_directory(rig: &Rig) -> String {
    let names: Vec<String> = std::fs::read_dir(rig.machine.join(workflow_run::RUNS))
        .expect("the runs directory was made")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "one run was filed: {names:?}");
    names.into_iter().next().expect("the one name")
}

/// A BUNDLE THAT FAILS CLOSES THE RECORD FILED FOR IT, says so on the stream and
/// names it. The bundle is the one refusal a run meets after its record is
/// filed — the record is what names the directory the bundle is written into —
/// and a record left open there is one no stream line names: `status` does not
/// list it, the controller neither re-runs nor cleans it, and it holds a
/// `[core.run] max_open` slot for good.
#[test]
fn a_bundle_that_fails_closes_the_record_filed_for_it_and_names_it() {
    let rig = Rig::new(
        "bundle-fails",
        &Pack::pinned_at(VERSION).whose_bundle_fails(),
        &cap_that_is_not_the_subject(),
    );

    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let id = the_one_run_directory(&rig);
    let said = stderr(&out);
    assert!(
        said.contains("the bundler would not bundle"),
        "the refusal is the bundle command's: {said}"
    );
    // The bundle line's own path carries the id, so the record is looked for
    // AS the record and not as a substring of that path.
    assert!(
        said.contains(&format!("the record {id} ")),
        "and it names the record: {said}"
    );

    assert!(
        !is_open(&rig, &id),
        "the record leaves the open set the cap is measured against"
    );
    let failed = only(&rig, WHOLE_STREAM, fleet_core::item::RUN_FAILED);
    assert_eq!(failed["payload"]["run"].as_str(), Some(id.as_str()));
    assert!(
        failed["payload"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("the bundler would not bundle")),
        "the reason is the refusal the verb printed: {failed}"
    );
    none_of(&rig, WHOLE_STREAM, fleet_core::item::RUN_STARTED);
}

// ---- AC1: the exit table ------------------------------------------------------

/// The four workflows the table is read against. Each says its piece on stdout
/// and exits on the code its row names.
const CLOSES: &str = "echo 'the work is done'\nexit 0";
const FAILS: &str = "echo '{\"why\":\"the thing broke\"}'\nexit 1";
const WAITS: &str = "echo '{\"for\":\"a delivery\"}'\nexit 2";
const STRANGE: &str = "echo 'nothing to see'\nexit 7";

/// The sequence the waiting workflow's own line carries. It is above anything
/// the rig's stream reaches on its own, so the position at exit and the
/// position the child was handed can never be read for one another.
const MOVED_TO: u64 = 99;

/// The waiting workflow, and a line of its own appended to the stream it was
/// told about — so the sequence at exit is [`MOVED_TO`] whatever the stream
/// stood at when the child started.
fn waits_after_moving_the_stream() -> String {
    format!(
        "echo '{{\"id\":\"x\",\"seq\":{MOVED_TO},\"ts\":\"t\",\"type\":\"probe.wrote\",\
         \"actor\":{{\"kind\":\"run\",\"id\":\"the-workflow\"}},\"payload\":{{}}}}' >> \"$FLEET_STREAM\"\n\
         echo \"started_at=$FLEET_STREAM_SEQ\"\n\
         echo '{{\"for\":\"a delivery\"}}'\n\
         exit 2"
    )
}

/// Every row of the exit table, each read off the code its own workflow exits
/// on.
///
/// ONE RIG AND FOUR WORKFLOWS. The rows differ by the script and by nothing
/// else, so a rig apiece would pay four boards and four packs to vary one file.
/// Each row's assertions are taken from the position the stream stood at before
/// that row ran — the same claim a row on a stream of its own makes, and the
/// reason `only` and `none_of` take a cursor.
#[test]
fn the_exit_table_reads_each_row_off_the_code_its_workflow_exits_on() {
    let waits = waits_after_moving_the_stream();
    let rig = Rig::new(
        "exits",
        &Pack::of_rows(&[
            ("closed", CLOSES),
            ("failed", FAILS),
            ("waiting", &waits),
            ("strange", STRANGE),
        ]),
        &cap_that_is_not_the_subject(),
    );

    // ---- exit 0: the run closes and its record is retired.
    let from = rig.stream_length();
    let out = rig.run(&["run", &rig.workflow("closed"), "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    assert_eq!(outcome_line(&out), "closed");

    let closed = only(&rig, from, fleet_core::item::RUN_CLOSED);
    assert_eq!(closed["payload"]["run"].as_str(), Some(id.as_str()));
    assert!(!is_open(&rig, &id), "a closed run leaves the open set");

    // The close is the run's own and not a second row of the table firing.
    none_of(&rig, from, fleet_core::item::RUN_FAILED);
    none_of(&rig, from, fleet_core::item::RUN_WAITING);
    none_of(&rig, from, fleet_core::item::RUN_COULD_NOT_TELL);

    // ---- exit 1 on a last line that reads: the run fails with that reason.
    let from = rig.stream_length();
    let out = rig.run(&["run", &rig.workflow("failed"), "--by", BY]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    assert_eq!(outcome_line(&out), "failed");

    let failed = only(&rig, from, fleet_core::item::RUN_FAILED);
    assert_eq!(failed["payload"]["run"].as_str(), Some(id.as_str()));
    assert_eq!(
        failed["payload"]["reason"]["why"].as_str(),
        Some("the thing broke"),
        "the reason is the workflow's own object, carried whole: {failed}"
    );
    assert!(!is_open(&rig, &id), "a failed run leaves the open set");
    none_of(&rig, from, fleet_core::item::RUN_COULD_NOT_TELL);

    // ---- exit 2: the wake condition, and the SEQUENCE AT EXIT rather than the
    // one the child was handed. The workflow appends a line of its own to the
    // stream it was told about, so the two readings must differ: a `seq` copied
    // from `FLEET_STREAM_SEQ` would answer the position the run started at, and
    // the stream has moved past it by the time the process exits.
    let from = rig.stream_length();
    let out = rig.run(&["run", &rig.workflow("waiting"), "--by", BY]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a wait is not a failure: {}",
        stderr(&out)
    );
    let (id, _) = started_line(&out);
    assert_eq!(outcome_line(&out), "waiting");

    let waiting = only(&rig, from, fleet_core::item::RUN_WAITING);
    assert_eq!(waiting["payload"]["run"].as_str(), Some(id.as_str()));
    assert_eq!(
        waiting["payload"]["wake"]["for"].as_str(),
        Some("a delivery"),
        "the wake condition is the workflow's own object: {waiting}"
    );
    assert_eq!(
        waiting["payload"]["seq"].as_u64(),
        Some(MOVED_TO),
        "the sequence is the one the stream stood at when the process exited: {waiting}"
    );

    // The position the child WAS handed is this run's own `run.started` line,
    // which is not the one the exit read.
    let started = only(&rig, from, fleet_core::item::RUN_STARTED);
    let at_start = started["seq"]
        .as_u64()
        .unwrap_or_else(|| panic!("the stream numbers its lines: {started}"));
    assert_ne!(
        at_start, MOVED_TO,
        "the two positions are different numbers"
    );
    let directory = rig.machine.join(workflow_run::RUNS).join(&id);
    let said = std::fs::read_to_string(directory.join(workflow_run::STDOUT_LOG))
        .expect("the log is readable");
    assert!(
        said.contains(&format!("started_at={at_start}")),
        "the child was handed the position it started at, which is not {MOVED_TO}: {said}"
    );
    assert!(is_open(&rig, &id), "a waiting run stays open");

    // ---- any other exit: could not tell, with what was read.
    let from = rig.stream_length();
    let out = rig.run(&["run", &rig.workflow("strange"), "--by", BY]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    assert_eq!(outcome_line(&out), "could not tell");

    let unread = only(&rig, from, fleet_core::item::RUN_COULD_NOT_TELL);
    assert_eq!(unread["payload"]["run"].as_str(), Some(id.as_str()));
    assert_eq!(unread["payload"]["exit"].as_i64(), Some(7));
    assert_eq!(
        unread["payload"]["read"].as_str(),
        Some("nothing to see"),
        "what was read is carried, so a reader sees what could not be classified: {unread}"
    );
    assert!(
        is_open(&rig, &id),
        "a run nothing could classify is not retired on a guess"
    );
}

/// The payload tables core declares for the four kinds, against what the verb
/// writes: a key invented and a key dropped are the same defect.
#[test]
fn the_back_halfs_kinds_declare_the_keys_they_carry() {
    for (kind, keys) in [
        (fleet_core::item::RUN_CLOSED, ["run"].as_slice()),
        (fleet_core::item::RUN_FAILED, ["run", "reason"].as_slice()),
        (
            fleet_core::item::RUN_WAITING,
            ["run", "wake", "seq"].as_slice(),
        ),
        (
            fleet_core::item::RUN_COULD_NOT_TELL,
            ["run", "exit", "read"].as_slice(),
        ),
    ] {
        assert_eq!(
            fleet_core::item::payload_keys(kind),
            Some(keys),
            "{kind}'s payload table is where the writer and a fold agree"
        );
    }
}

// ---- AC2: the environment and the inputs --------------------------------------

/// The five names and the machine directory, and the pinned inputs as JSON,
/// echoed by the workflow into its own log.
#[test]
fn the_environment_and_the_pinned_inputs_reach_the_workflow() {
    let echoes = "echo \"run_id=$FLEET_RUN_ID\"\n\
                  echo \"stream=$FLEET_STREAM\"\n\
                  echo \"stream_seq=$FLEET_STREAM_SEQ\"\n\
                  echo \"run_dir=$FLEET_RUN_DIR\"\n\
                  echo \"bin=$FLEET_BIN\"\n\
                  echo \"dir=$FLEET_DIR\"\n\
                  echo \"stdin=$(cat)\"\n\
                  exit 0";
    let rig = Rig::new(
        "environment",
        &Pack::running(echoes),
        &cap_that_is_not_the_subject(),
    );
    let workflow = rig.workflow(ONE);
    let out = rig.run(&["run", &workflow, "--input", "who=the-arm", "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);

    let directory = rig.machine.join(workflow_run::RUNS).join(&id);
    let said = std::fs::read_to_string(directory.join(workflow_run::STDOUT_LOG))
        .expect("the log is readable");
    for expected in [
        format!("run_id={id}"),
        format!("stream={}", rig.machine.join("events.jsonl").display()),
        format!("run_dir={}", directory.display()),
        format!("dir={}", rig.machine.display()),
        String::from("stream_seq=1"),
    ] {
        assert!(
            said.contains(&expected),
            "the log carries `{expected}`: {said}"
        );
    }
    assert!(
        said.contains(&format!("bin={}", env!("CARGO_BIN_EXE_fleet"))),
        "FLEET_BIN is the binary that is running, not one off PATH: {said}"
    );

    // The document on stdin is the pinned file, read as JSON.
    let line = said
        .lines()
        .find_map(|line| line.strip_prefix("stdin="))
        .unwrap_or_else(|| panic!("the workflow echoed its stdin: {said}"));
    let document: serde_json::Value = serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("stdin is one JSON document ({e}): {line}"));
    assert_eq!(document["inputs"]["who"].as_str(), Some("the-arm"));
    assert_eq!(document["workflow"].as_str(), Some(workflow.as_str()));
    assert_eq!(document["pack"].as_str(), Some("scratch"));
    assert_eq!(document["by"].as_str(), Some(BY));

    // stderr is captured whole too, in its own file beside the pins.
    assert!(
        directory.join(workflow_run::STDERR_LOG).is_file(),
        "the run directory holds {}",
        workflow_run::STDERR_LOG
    );
}

/// The project root the run was started inside reaches the workflow as
/// `FLEET_PROJECT`, while its cwd stays the run directory: the root travels by
/// name, because a verb the workflow calls back into resolves its project from
/// where it is started, and the run directory sits above no `fleet.toml`.
#[test]
fn the_project_root_reaches_the_workflow_as_fleet_project() {
    let echoes = "echo \"project=$FLEET_PROJECT\"\n\
                  echo \"cwd=$(pwd)\"\n\
                  exit 0";
    let rig = Rig::new(
        "project",
        &Pack::running(echoes),
        &cap_that_is_not_the_subject(),
    );
    let workflow = rig.workflow(ONE);
    let out = rig.run(&["run", &workflow, "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);

    let directory = rig.machine.join(workflow_run::RUNS).join(&id);
    let said = std::fs::read_to_string(directory.join(workflow_run::STDOUT_LOG))
        .expect("the log is readable");
    let echoed = |key: &str| -> PathBuf {
        let value = said
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_else(|| panic!("the workflow echoed `{key}`: {said}"));
        assert!(!value.is_empty(), "`{key}` is set: {said}");
        Path::new(value)
            .canonicalize()
            .unwrap_or_else(|e| panic!("`{key}{value}` resolves: {e}"))
    };
    let canonical = |path: &Path| {
        path.canonicalize()
            .unwrap_or_else(|e| panic!("{} resolves: {e}", path.display()))
    };
    assert_eq!(
        echoed("project="),
        canonical(&rig.project),
        "FLEET_PROJECT is the project the run was started inside: {said}"
    );
    assert!(
        echoed("project=").join("fleet.toml").is_file(),
        "the root FLEET_PROJECT names holds the policy file: {said}"
    );
    assert_eq!(
        echoed("cwd="),
        canonical(&directory),
        "the child's own cwd is still the run directory: {said}"
    );
}

/// `HOME`, `FLEET_CLAUDE_BIN`, `USER`, `TMPDIR` and `LANG` pass through from
/// the fleet process to the workflow, verbatim, while the clearing holds for
/// everything else: the dispatch a workflow calls resolves the agent binary
/// from the first two — a child under no `HOME` searches a relative
/// `.local/bin` and finds nothing — and hands the last three to the seat it
/// starts, which reads its keychain login off `USER` and comes up logged out
/// without one.
///
/// The binary named here is no file at all, which is what shows the value is
/// copied and not resolved; the stray name set beside it is the control that
/// the clearing still stands. `TMPDIR` names a real directory because the
/// fleet process under test is handed this same value.
#[test]
fn the_agent_binary_seams_reach_the_workflow_from_the_environment() {
    let echoes = "echo \"home=$HOME\"\n\
                  echo \"claude_bin=$FLEET_CLAUDE_BIN\"\n\
                  echo \"user=$USER\"\n\
                  echo \"tmpdir=$TMPDIR\"\n\
                  echo \"lang=$LANG\"\n\
                  echo \"stray=$A_STRAY_NAME\"\n\
                  exit 0";
    let rig = Rig::new(
        "seams",
        &Pack::running(echoes),
        &cap_that_is_not_the_subject(),
    );
    let workflow = rig.workflow(ONE);
    let named = "/a/binary/nobody/resolves";
    let temp = rig.root.join("a-temp-of-its-own");
    std::fs::create_dir_all(&temp).expect("the arm's own temp directory is created");
    let temp = temp.display().to_string();
    let user = "a-user-of-this-arms-own";
    let lang = "xx_XX.UTF-8";
    let out = rig.run_with(
        &["run", &workflow, "--by", BY],
        &[
            ("FLEET_CLAUDE_BIN", named),
            ("USER", user),
            ("TMPDIR", &temp),
            ("LANG", lang),
            ("A_STRAY_NAME", "leaks"),
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);

    let directory = rig.machine.join(workflow_run::RUNS).join(&id);
    let said = std::fs::read_to_string(directory.join(workflow_run::STDOUT_LOG))
        .expect("the log is readable");
    let echoed = |key: &str| -> &str {
        said.lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_else(|| panic!("the workflow echoed `{key}`: {said}"))
    };
    assert_eq!(
        echoed("home="),
        rig.root.join("home").display().to_string(),
        "HOME is the fleet process's own, as the rig set it: {said}"
    );
    assert_eq!(
        echoed("claude_bin="),
        named,
        "FLEET_CLAUDE_BIN is forwarded as written, not resolved: {said}"
    );
    assert_eq!(
        echoed("user="),
        user,
        "USER reaches the seat a dispatch from this workflow starts: {said}"
    );
    assert_eq!(
        echoed("tmpdir="),
        temp,
        "TMPDIR is the fleet process's own, verbatim: {said}"
    );
    assert_eq!(
        echoed("lang="),
        lang,
        "LANG is the fleet process's own, verbatim: {said}"
    );
    assert_eq!(
        echoed("stray="),
        "",
        "a name the fleet process carries and the run does not pass through is cleared: {said}"
    );
}

/// The system directories and the one directory that holds `bd` — where the
/// store finds it by bare name on a box whose constructed child PATH holds no
/// `bd`, the fallback the verbs' resolver keeps — the whole of what a fleet
/// process needs, and nothing a rig's runtime stub is ever written to.
fn a_path_no_runtime_is_on() -> String {
    let held = std::env::var("PATH").unwrap_or_default();
    let bd = std::env::split_paths(&held)
        .find(|dir| dir.join("bd").is_file())
        .expect("bd is on this process's PATH");
    format!("/usr/bin:/bin:{}", bd.display())
}

/// The pack's lines run on a `PATH` the caller constructs, with the directory
/// the pinned runtime resolves from in front — never on the fleet process's
/// own. A controller re-running a run is a launchd service carrying the
/// manager's minimal search path, and a runtime installed where the pack's
/// doctor check looks second is on no session's `PATH` either.
///
/// THE CONTROL IS THE PROCESS PATH ITSELF, asserted to reach neither the
/// directory the rig normally puts the stub in nor the installer's bin: a run
/// line handed this process's `PATH` resolves no runtime at all, which is the
/// reading this arm would then take off the log.
#[test]
fn the_pack_lines_run_on_a_constructed_path_reaching_the_installers_bin() {
    let body = format!("{RUNTIME}\nexit 0");
    let rig = Rig::new(
        "installed",
        &Pack::running(&body),
        &cap_that_is_not_the_subject(),
    );
    let bin = rig.runtime_only_in_the_installers_bin();
    let path = a_path_no_runtime_is_on();
    for absent in [&bin, &rig.stubs] {
        let absent = absent.display().to_string();
        assert!(
            !path.split(':').any(|dir| dir == absent),
            "the fleet process's own PATH does not reach {absent}: {path}"
        );
    }

    let workflow = rig.workflow(ONE);
    let before = rig.stream_length();
    let out = rig.run_on_path(&["run", &workflow, "--by", BY], &path, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    let started = only(&rig, before, fleet_core::item::RUN_STARTED);
    assert_eq!(started["payload"]["run"].as_str(), Some(id.as_str()));

    let said = std::fs::read_to_string(
        rig.machine
            .join(workflow_run::RUNS)
            .join(&id)
            .join(workflow_run::STDOUT_LOG),
    )
    .expect("the log is readable");
    assert!(
        said.contains(&format!("{RUNTIME} {VERSION}")),
        "the run line resolved {RUNTIME} from {}: {said}",
        bin.display()
    );
}

// ---- AC3: a last line nothing can read ----------------------------------------

/// An exit of 1 whose last line is not JSON.
///
/// The control is the `failed` row of
/// `the_exit_table_reads_each_row_off_the_code_its_workflow_exits_on`, which
/// runs the SAME code path with the same exit and differs only in what the
/// workflow printed — so what separates the two answers is the last line and
/// nothing else.
#[test]
fn an_unparseable_last_line_on_exit_one_is_could_not_tell_and_never_failed() {
    let rig = Rig::new(
        "unparseable",
        &Pack::running("echo 'the thing broke'\nexit 1"),
        &cap_that_is_not_the_subject(),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    assert_eq!(outcome_line(&out), "could not tell");

    none_of(&rig, WHOLE_STREAM, fleet_core::item::RUN_FAILED);
    let unread = only(&rig, WHOLE_STREAM, fleet_core::item::RUN_COULD_NOT_TELL);
    assert_eq!(unread["payload"]["exit"].as_i64(), Some(1));
    assert_eq!(
        unread["payload"]["read"].as_str(),
        Some("the thing broke"),
        "the line that could not be read is carried, not dropped: {unread}"
    );
    assert!(is_open(&rig, &id), "the run is not retired on a guess");
}

/// A workflow that says nothing at all on an exit of 1: the `read` is null
/// rather than an empty reason.
#[test]
fn a_silent_exit_of_one_is_could_not_tell_with_nothing_read() {
    let rig = Rig::new(
        "silent",
        &Pack::running("exit 1"),
        &cap_that_is_not_the_subject(),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));

    none_of(&rig, WHOLE_STREAM, fleet_core::item::RUN_FAILED);
    let unread = only(&rig, WHOLE_STREAM, fleet_core::item::RUN_COULD_NOT_TELL);
    assert!(
        unread["payload"]["read"].is_null(),
        "nothing read is null and not an empty reason: {unread}"
    );
}

// ---- the pack's settings: [packs.<name>] ----------------------------------------

/// Three settings the scratch pack declares, in both of the spellings a
/// declaration may take: one quoted whole, two as a dotted table path. One is
/// set by the fleet, one falls to its default, and one has neither.
const DECLARED: &str = "\n[config.\"greeting.word\"]\n\
     description = \"the word the scratch workflow greets with\"\n\
     type = \"string\"\n\
     \n[config.greeting.times]\n\
     description = \"how many times it greets\"\n\
     type = \"integer\"\n\
     default = 2\n\
     \n[config.unset]\n\
     description = \"declared, with no default, and set by nobody\"\n";

/// A workflow that echoes the document it was handed and then waits, so the
/// same run can be executed again.
const ECHOES_AND_WAITS: &str = "echo \"stdin=$(cat)\"\n\
     echo '{\"for\":\"a delivery\"}'\n\
     exit 2";

/// The policy the settings arms run under: the cap that is not the subject, and
/// one `[packs.<name>]` section.
fn policy_setting(section: &str) -> String {
    format!("{}\n{section}", cap_that_is_not_the_subject())
}

/// The document the run child was handed on stdin, off its own log.
fn handed(directory: &Path) -> serde_json::Value {
    let said = std::fs::read_to_string(directory.join(workflow_run::STDOUT_LOG))
        .expect("the log is readable");
    let line = said
        .lines()
        .find_map(|line| line.strip_prefix("stdin="))
        .unwrap_or_else(|| panic!("the workflow echoed its stdin: {said}"));
    serde_json::from_str(line)
        .unwrap_or_else(|e| panic!("stdin is one JSON document ({e}): {line}"))
}

/// A value `fleet.toml` sets for a declared key reaches the workflow on the
/// channel its inputs ride, beside the default of a key it does not set; a key
/// with neither is absent, which is what `run.config` answers undefined for.
/// The same table is pinned in the run directory's inputs file.
#[test]
fn a_declared_setting_reaches_the_workflow_beside_its_declared_default() {
    let rig = Rig::new(
        "settings",
        &Pack::running(ECHOES_AND_WAITS).declaring(DECLARED),
        &policy_setting("[packs.scratch]\ngreeting.word = \"ahoy\"\n"),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    let directory = rig.machine.join(workflow_run::RUNS).join(&id);

    let document = handed(&directory);
    let config = &document["config"];
    assert_eq!(
        config["greeting.word"].as_str(),
        Some("ahoy"),
        "the value the fleet set: {document}"
    );
    assert_eq!(
        config["greeting.times"].as_i64(),
        Some(2),
        "the default the pack declared, where the fleet set none: {document}"
    );
    assert!(
        config.get("unset").is_none(),
        "a key with no value and no default is absent: {document}"
    );

    let pinned = fleet_core::item::read_table(&directory.join(workflow_run::INPUTS))
        .expect("the inputs parse as TOML");
    assert_eq!(
        pinned["config"]["greeting.word"].as_str(),
        Some("ahoy"),
        "the settings are pinned with the inputs: {pinned}"
    );
}

/// A key the installed pack does not declare is refused before anything is
/// written, naming the key and the pack.
#[test]
fn an_undeclared_setting_refuses_naming_the_key_and_the_pack_and_writes_nothing() {
    let rig = Rig::new(
        "undeclared",
        &Pack::running(ECHOES_AND_WAITS).declaring(DECLARED),
        &policy_setting("[packs.scratch]\ngreeting.wrod = \"ahoy\"\n"),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "greeting.wrod",
    );
    assert!(
        said.contains("`scratch`"),
        "the refusal names the pack: {said}"
    );
    assert!(
        said.contains("greeting.word"),
        "and what the pack does declare: {said}"
    );
}

/// A section for a pack that is not installed is refused the same way, naming
/// the section — over an installed pack that declares no settings at all, so
/// what refuses is the section and never a declaration.
#[test]
fn a_section_for_a_pack_not_installed_refuses_naming_it_and_writes_nothing() {
    let rig = Rig::new(
        "uninstalled",
        &Pack::running(ECHOES_AND_WAITS),
        &policy_setting("[packs.absent]\ngreeting.word = \"ahoy\"\n"),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "[packs.absent]",
    );
    assert!(
        said.contains("scratch"),
        "the refusal names the packs that are installed: {said}"
    );
}

/// A policy that still sets a TEST COMMAND under `[gates]` opens no run: the
/// refusal names the key and the pack setting that replaces it, before
/// anything is written — the workflow being opened is where the command is set
/// now, and a run beside the old key would land untested while the person who
/// wrote it believes otherwise.
#[test]
fn a_policy_setting_a_gates_test_command_opens_no_run_and_names_where_it_moved() {
    for (key, setting) in [("suite", "takeoff.test"), ("touched", "takeoff.touched")] {
        let rig = Rig::new(
            &format!("moved-{key}"),
            &Pack::running(ECHOES_AND_WAITS),
            &policy_setting(&format!("[gates]\n{key} = \"make check\"\n")),
        );
        let said = refuses(
            &rig,
            &["run", &rig.workflow(ONE), "--by", BY],
            &format!("[gates] {key}"),
        );
        assert!(
            said.contains(&format!("`{setting}` under [packs.tiny]")),
            "the refusal names the pack setting that replaces it: {said}"
        );
    }
}

/// A policy that still carries a `[gates]` table opens no run, whatever the
/// table holds: the refusal names the table and where each of its keys is set
/// now, before anything is written — a marker, a command list or a guard target
/// left under the old name is one nothing reads.
#[test]
fn a_policy_carrying_a_gates_table_opens_no_run_and_names_the_new_homes() {
    let rig = Rig::new(
        "moved-gates",
        &Pack::running(ECHOES_AND_WAITS),
        &policy_setting("[gates]\nci_marker = \"printf '[skip ci]'\"\n"),
    );
    let said = refuses(
        &rig,
        &["run", &rig.workflow(ONE), "--by", BY],
        "[gates] is not a policy table",
    );
    for home in [
        "`ci_marker` under [landing]",
        "`tool_commands` under [permissions]",
        "`release_ref_glob` and the `prod_*` lists under [guards.targets]",
    ] {
        assert!(said.contains(home), "the refusal names {home}: {said}");
    }
}

/// A policy that still sets a key NOTHING READS opens no run: the refusal names
/// the key and says to delete it, before anything is written — a cap nothing
/// enforces is one a person believes is in force. One key at the top of the
/// policy and one a table down, so the refusal is not a reading of one table.
#[test]
fn a_policy_setting_a_key_nothing_reads_opens_no_run_and_names_it() {
    for (label, section, named) in [
        (
            "returns",
            "[core]\nmax_returns = 3\n",
            "[core] max_returns is no longer read — delete it",
        ),
        (
            "seats",
            "[core.flight]\nmax_seats = 2\n",
            "[core.flight] max_seats is no longer read — delete it",
        ),
    ] {
        let rig = Rig::new(
            &format!("retired-{label}"),
            &Pack::running(ECHOES_AND_WAITS),
            &policy_setting(section),
        );
        refuses(&rig, &["run", &rig.workflow(ONE), "--by", BY], named);
    }
}

/// The stream a re-run in this process appends to: the same file, through the
/// same log type, the binary's own writer uses.
struct Stream(PathBuf);

impl fleet_core::item::Events for Stream {
    fn append(
        &self,
        kind: &str,
        actor: &fleet_core::seat::actor::Actor,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        fleet_controller::events::EventLog::open(&self.0)
            .append(
                kind,
                &fleet_controller::events::ActorRef::new(actor.kind.as_str(), actor.id.clone()),
                payload,
            )
            .map_err(|e| e.to_string())
    }
}

impl workflow_run::Stream for Stream {
    fn path(&self) -> PathBuf {
        self.0.clone()
    }

    fn seq(&self) -> u64 {
        fleet_controller::events::EventLog::open(&self.0).seq()
    }
}

/// A run is pinned to the settings it was opened with: a re-run after
/// `fleet.toml` changed hands the workflow the first value, while a fresh run
/// over the same file reads the new one — the control that says the edit was
/// readable and it is the pin that held.
///
/// THE RE-RUN IS CORE'S OWN ENTRY, called in this process as the controller's
/// run pass calls it, with the edited file as the policy in force.
#[test]
fn a_rerun_reads_the_settings_the_run_was_opened_with_and_not_the_edited_file() {
    let rig = Rig::new(
        "pinned-settings",
        &Pack::running(ECHOES_AND_WAITS).declaring(DECLARED),
        &policy_setting("[packs.scratch]\ngreeting.word = \"ahoy\"\n"),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(outcome_line(&out), "waiting");
    let (id, _) = started_line(&out);
    let directory = rig.machine.join(workflow_run::RUNS).join(&id);
    assert_eq!(
        handed(&directory)["config"]["greeting.word"].as_str(),
        Some("ahoy")
    );

    let policy_file = rig.project.join("fleet.toml");
    write(
        &policy_file,
        &policy_setting("[packs.scratch]\ngreeting.word = \"avast\"\n"),
    );

    let store = fleet_core::store::bd::Bd::at(&rig.project);
    let packs = fleet_core::item::brief::Packs::under(
        &rig.machine.join("packs"),
        &rig.machine.join(fleet_core::defaults::DIR),
    )
    .unwrap_or_else(|stop| panic!("the layers resolve: {}", stop.message));
    let table = fleet_core::item::table_at(&policy_file);
    let project = fleet_core::item::Project {
        root: rig.project.clone(),
        name: String::from("project"),
        policy: table.clone(),
        guards: table,
    };
    let stream = Stream(rig.machine.join("events.jsonl"));
    let mut said = Vec::new();
    let ended = workflow_run::rerun(
        &mut said,
        &workflow_run::Again {
            run: &id,
            by: &Actor::typed(BY).expect("typed").expect("a seat"),
            at: "2026-09-22T00:00:00Z",
            machine_dir: &rig.machine,
            fleet_bin: Path::new(env!("CARGO_BIN_EXE_fleet")),
        },
        &workflow_run::Wiring {
            store: &store,
            project: &project,
            packs: &packs,
            policy_file: &policy_file,
            events: &stream,
            stream: &stream,
            child_path: "",
        },
    )
    .unwrap_or_else(|stop| panic!("the re-run executes: {}", stop.message));
    assert_eq!(ended, workflow_run::Ended::Waiting);
    assert_eq!(
        handed(&directory)["config"]["greeting.word"].as_str(),
        Some("ahoy"),
        "the re-run is handed the value the run was opened with"
    );

    let fresh = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(fresh.status.code(), Some(0), "{}", stderr(&fresh));
    let (fresh_id, _) = started_line(&fresh);
    assert_eq!(
        handed(&rig.machine.join(workflow_run::RUNS).join(&fresh_id))["config"]["greeting.word"]
            .as_str(),
        Some("avast"),
        "a run opened after the edit reads the edited file"
    );
}

/// The real store with one reading bent: every read's proof carries a token
/// nothing wrote, which is the one answer the pins' read-back asks of it.
struct Planted(fleet_core::store::bd::Bd);

impl fleet_core::store::Store for Planted {
    fn resolve(&self, id: &str) -> Result<fleet_core::store::ItemId, StoreError> {
        self.0.resolve(id)
    }

    fn list(
        &self,
        filter: &fleet_core::store::Filter,
    ) -> Result<Vec<fleet_core::store::ItemSummary>, StoreError> {
        self.0.list(filter)
    }

    fn show(&self, item: &str) -> Result<fleet_core::store::Item, StoreError> {
        let mut read = self.0.show(item)?;
        read.proof = fleet_core::store::ReadProof::of(format!(
            "{}{}",
            read.proof.as_str(),
            fleet_core::item::control_token()
        ));
        Ok(read)
    }

    fn create(
        &self,
        item: &fleet_core::store::NewItem,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<fleet_core::store::ItemId, StoreError> {
        self.0.create(item, by)
    }

    fn update(
        &self,
        id: &fleet_core::store::ItemId,
        change: &fleet_core::store::Update,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.0.update(id, change, by)
    }

    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        self.0.hand_over(item, from, to, by)
    }

    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.0.set_orders(item, payload, by)
    }

    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.0.set_metadata(item, payload, by)
    }

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.0.unset_orders(item, by)
    }

    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.0.reopen(item, by)
    }

    fn withdraw_order(
        &self,
        item: &str,
        seat: &str,
        status: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.0.withdraw_order(item, seat, status, by)
    }

    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        self.0.hold(item, reason, by)
    }

    fn open_holds(&self) -> Result<Vec<String>, StoreError> {
        self.0.open_holds()
    }

    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError> {
        self.0.clear_hold(hold, by)
    }

    fn close(
        &self,
        id: &fleet_core::store::ItemId,
        reason: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.0.close(id, reason, by)
    }

    fn append(
        &self,
        item: &str,
        body: &fleet_core::entry::Body,
        by: &Actor,
    ) -> Result<String, StoreError> {
        self.0.append(item, body, by)
    }

    fn timeline(&self, item: &str) -> Result<Vec<fleet_core::entry::Entry>, StoreError> {
        self.0.timeline(item)
    }

    fn capabilities(&self) -> Result<fleet_core::store::types::Capabilities, StoreError> {
        self.0.capabilities()
    }

    fn version(&self) -> Result<fleet_core::store::Version, StoreError> {
        self.0.version()
    }

    fn export(&self, into: &Path) -> Result<PathBuf, StoreError> {
        self.0.export(into)
    }
}

/// THE NEGATIVE CONTROL on the pins' read-back: a read whose proof carries a
/// token nothing wrote is not reading the record, however right its hash and
/// workflow read — a could-not-tell naming the token, and no process started.
///
/// Core's own entry, called in this process as the cli calls it, over a store
/// that plants the token in every read's proof.
#[test]
fn the_pins_read_back_catches_a_planted_token() {
    let rig = Rig::new(
        "planted",
        &Pack::running("echo 'nothing runs'"),
        &cap_that_is_not_the_subject(),
    );
    let store = Planted(fleet_core::store::bd::Bd::at(&rig.project));
    let packs = fleet_core::item::brief::Packs::under(
        &rig.machine.join("packs"),
        &rig.machine.join(fleet_core::defaults::DIR),
    )
    .unwrap_or_else(|stop| panic!("the layers resolve: {}", stop.message));
    let policy_file = rig.project.join("fleet.toml");
    let table = fleet_core::item::table_at(&policy_file);
    let project = fleet_core::item::Project {
        root: rig.project.clone(),
        name: String::from("project"),
        policy: table.clone(),
        guards: table,
    };
    let stream = Stream(rig.machine.join("events.jsonl"));
    let child_path = match std::env::var("PATH") {
        Ok(held) => format!("{}:{held}", rig.stubs.display()),
        Err(_) => rig.stubs.display().to_string(),
    };
    let workflow = rig.workflow(ONE);

    let stop = workflow_run::run(
        &mut Vec::new(),
        &workflow_run::Order {
            workflow: &workflow,
            inputs: &[],
            by: &Actor::typed(BY).expect("typed").expect("a seat"),
            at: "2026-09-25T00:00:00Z",
            machine_dir: &rig.machine,
            fleet_bin: Path::new(env!("CARGO_BIN_EXE_fleet")),
        },
        &workflow_run::Wiring {
            store: &store,
            project: &project,
            packs: &packs,
            policy_file: &policy_file,
            events: &stream,
            stream: &stream,
            child_path: &child_path,
        },
    )
    .expect_err("a read that is not the record's is refused");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(fleet_core::item::control_token())
            && stop.message.contains("not reading this item"),
        "{}",
        stop.message
    );
}

// ---- the cancel ----------------------------------------------------------------

/// The same wiring the controller's run pass hands core's re-run, over this
/// rig's project and its policy file as it stands.
fn rerun_in_this_process(
    rig: &Rig,
    id: &str,
) -> Result<workflow_run::Ended, fleet_core::item::Stop> {
    let store = fleet_core::store::bd::Bd::at(&rig.project);
    let packs = fleet_core::item::brief::Packs::under(
        &rig.machine.join("packs"),
        &rig.machine.join(fleet_core::defaults::DIR),
    )
    .unwrap_or_else(|stop| panic!("the layers resolve: {}", stop.message));
    let policy_file = rig.project.join("fleet.toml");
    let table = fleet_core::item::table_at(&policy_file);
    let project = fleet_core::item::Project {
        root: rig.project.clone(),
        name: String::from("project"),
        policy: table.clone(),
        guards: table,
    };
    let stream = Stream(rig.machine.join("events.jsonl"));
    workflow_run::rerun(
        &mut Vec::new(),
        &workflow_run::Again {
            run: id,
            by: &the_controller(),
            at: "2026-09-23T00:00:00Z",
            machine_dir: &rig.machine,
            fleet_bin: Path::new(env!("CARGO_BIN_EXE_fleet")),
        },
        &workflow_run::Wiring {
            store: &store,
            project: &project,
            packs: &packs,
            policy_file: &policy_file,
            events: &stream,
            stream: &stream,
            child_path: "",
        },
    )
}

/// The ids of every hold the store still lists open.
fn open_holds(rig: &Rig) -> Vec<String> {
    use fleet_core::store::Store;
    fleet_core::store::bd::Bd::at(&rig.project)
        .open_holds()
        .expect("the store lists its holds")
}

/// `fleet cancel` on a waiting run: the record is closed, `run.cancelled` names
/// it, and the run is never executed again — not even where the re-run the
/// controller's pass calls is asked for it outright, because a closed record
/// is refused before anything is executed.
#[test]
fn a_cancelled_waiting_run_is_closed_announced_and_never_executed_again() {
    let rig = Rig::new(
        "cancel-wait",
        &Pack::running(WAITS),
        &cap_that_is_not_the_subject(),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    assert!(is_open(&rig, &id), "a waiting run holds its record open");

    let from = rig.stream_length();
    let out = rig.run(&["cancel", &id, "--by", PERSON]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), format!("{id} — cancelled"));
    assert!(
        !is_open(&rig, &id),
        "the record leaves the open set `[core.run] max_open` counts"
    );
    assert!(
        rig.document(&id)["close_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("cancelled")),
        "and says why it closed: {}",
        rig.document(&id)
    );
    let cancelled = only(&rig, from, fleet_core::item::RUN_CANCELLED);
    assert_eq!(cancelled["payload"]["run"].as_str(), Some(id.as_str()));
    assert_eq!(
        cancelled["actor"],
        serde_json::json!({ "kind": "seat", "id": &PERSON["seat:".len()..] })
    );
    let keys: Vec<&str> = cancelled["payload"]
        .as_object()
        .expect("the payload is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        Some(keys.as_slice()),
        fleet_core::item::payload_keys(fleet_core::item::RUN_CANCELLED),
        "the payload carries the keys its kind declares"
    );
    none_of(&rig, from, fleet_core::item::ITEM_ENTRY);

    let from = rig.stream_length();
    let stop = rerun_in_this_process(&rig, &id).expect_err("a closed run is not executed");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&format!("{id} is closed")),
        "refused as closed, before anything is read off the directory: {}",
        stop.message
    );
    none_of(&rig, from, fleet_core::item::RUN_STARTED);
}

/// A record whose `fleet.run` is at a version this binary does not know, or at
/// none, refuses the read of the record itself: could-not-tell naming the key
/// and the version, and never executed again as though its shape were known —
/// whatever another writer's bare `run` beside it holds (fleet-4j6).
#[test]
fn a_run_object_at_an_unknown_version_is_could_not_tell_and_never_executed() {
    use fleet_core::store::Store;
    let rig = Rig::new(
        "rerun-version",
        &Pack::running(WAITS),
        &cap_that_is_not_the_subject(),
    );
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    let store = fleet_core::store::bd::Bd::at(&rig.project);
    let pinned = rig.document(&id)["metadata"]["fleet.run"].clone();

    for (found, object) in [("v 2", Some(2)), ("no v", None)] {
        let mut written = pinned.clone();
        match object {
            Some(v) => written["v"] = serde_json::json!(v),
            None => {
                written
                    .as_object_mut()
                    .expect("the run's object is an object")
                    .remove("v");
            }
        }
        let payload = serde_json::json!({ "fleet.run": written, "run": pinned }).to_string();
        store
            .set_metadata(&id, &payload, "a-newer-fleet")
            .expect("the object is rewritten");

        let from = rig.stream_length();
        let stop = rerun_in_this_process(&rig, &id).expect_err("the run is not executed");
        assert_eq!(stop.code, 3, "{found}: {}", stop.message);
        let named = match object {
            Some(v) => format!("{id}'s run record is not one this fleet reads (fleet.run, v {v})"),
            None => format!("{id}'s run record is not one this fleet reads (fleet.run, v none)"),
        };
        assert!(
            stop.message.contains(&named),
            "{found}: the key and the version are named: {}",
            stop.message
        );
        none_of(&rig, from, fleet_core::item::RUN_STARTED);
    }
}

/// One run held on a real store the way the controller's run pass leaves one
/// at `[core.run] max_crashes`: a workflow nothing could classify, then the hold
/// on its record. `entered` is the park the seam makes now, the hold and its
/// held entry; unset, it is a bare hold nothing on the record names.
fn a_run_held_at_the_cap(rig: &Rig, entered: bool) -> (String, String) {
    use fleet_core::store::Store;
    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    let store = fleet_core::store::bd::Bd::at(&rig.project);
    let hold = if entered {
        fleet_core::item::hold::park_at_the_cap(
            &fleet_core::item::hold::Capped {
                run: &id,
                reason: "executed 3 time(s) and nothing could classify the last one",
                directory: &rig.machine.join(workflow_run::RUNS).join(&id),
                by: &the_controller(),
            },
            &store,
        )
        .unwrap_or_else(|stop| panic!("the park is made: {}", stop.message))
        .0
    } else {
        store
            .hold(&id, "a hold nothing on the record names", "controller")
            .expect("the bare hold is raised")
    };
    (id, hold)
}

/// The crash cap's park, cleared through the shipped binary on a real store:
/// `fleet clear` finds the hold the held entry names, writes the cleared entry,
/// clears the hold the store raised, and says so on the stream.
#[test]
fn a_run_held_at_the_crash_cap_clears_through_fleet_clear() {
    let rig = Rig::new(
        "clear-park",
        &Pack::running(STRANGE),
        &cap_that_is_not_the_subject(),
    );
    let (id, hold) = a_run_held_at_the_cap(&rig, true);
    assert!(open_holds(&rig).contains(&hold), "{hold} stands");

    let from = rig.stream_length();
    let out = rig.run(&["clear", &id, "B", "--by", PERSON]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!open_holds(&rig).contains(&hold), "the hold is cleared");

    // The record: one held entry at the cap, by the controller, and the
    // person's clearance of it — which the stream's one signal names.
    use fleet_core::store::Store;
    let entries = fleet_core::store::bd::Bd::at(&rig.project)
        .timeline(&id)
        .expect("the record's timeline reads");
    assert_eq!(entries.len(), 2, "{entries:?}");
    let cleared = only(&rig, from, fleet_core::item::ITEM_ENTRY);
    assert_eq!(
        cleared["payload"],
        serde_json::json!({ "item": id, "entry": entries[1].id, "kind": "cleared" })
    );
    assert!(
        matches!(
            &entries[0].body,
            fleet_core::entry::Body::Held(held)
                if held.hold == hold
                    && held.reason == fleet_core::entry::HoldReason::MaxCrashes
                    && held.run_hash.is_some()
        ),
        "{entries:?}"
    );
    assert_eq!(entries[0].by, the_controller());
    assert_eq!(
        entries[1].body,
        fleet_core::entry::Body::Cleared(fleet_core::entry::Cleared {
            hold: hold.clone(),
            how: fleet_core::entry::Clearance::Answer,
            letter: Some(String::from("B")),
            text: None,
        })
    );
    assert_eq!(entries[1].by.to_string(), PERSON);
}

/// `fleet cancel` on a held run clears the hold standing on its record and
/// closes it — here a bare hold nothing on the record names, which `fleet
/// clear` cannot reach and which kept the record's close blocked. The holds
/// are the store's own answer, so a held entry's hold is found the same way.
#[test]
fn a_cancel_clears_the_hold_on_a_held_runs_record_and_closes_it() {
    let rig = Rig::new(
        "cancel-park",
        &Pack::running(STRANGE),
        &cap_that_is_not_the_subject(),
    );
    let (id, bare) = a_run_held_at_the_cap(&rig, false);
    assert!(open_holds(&rig).contains(&bare), "{bare} stands");

    let from = rig.stream_length();
    let out = rig.run(&["cancel", &id, "--by", PERSON]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out).trim(),
        format!("{id} — cancelled, hold {bare} cleared"),
        "the line names the hold it cleared"
    );
    assert!(
        !open_holds(&rig).contains(&bare),
        "the bare hold is cleared"
    );
    assert!(!is_open(&rig, &id), "and the record is closed");
    only(&rig, from, fleet_core::item::RUN_CANCELLED);

    // THE RECORD SAYS CANCELLED, and never a letter nobody chose: one cleared
    // entry for the hold, by whoever cancelled, carrying no letter and no text
    // — and one signal on the stream naming it, after the cancel.
    //
    // RED-PROOF: before the cleared entry, the only record of this was the
    // stream's line, its letter null.
    use fleet_core::store::Store;
    let entries = fleet_core::store::bd::Bd::at(&rig.project)
        .timeline(&id)
        .expect("the closed record's timeline reads");
    let clearances: Vec<_> = entries
        .iter()
        .filter(|entry| matches!(entry.body, fleet_core::entry::Body::Cleared(_)))
        .collect();
    assert_eq!(clearances.len(), 1, "one per hold: {entries:?}");
    let cleared = only(&rig, from, fleet_core::item::ITEM_ENTRY);
    assert_eq!(
        cleared["payload"],
        serde_json::json!({ "item": id, "entry": clearances[0].id, "kind": "cleared" })
    );
    assert_eq!(
        cleared["actor"],
        serde_json::json!({ "kind": "seat", "id": &PERSON["seat:".len()..] }),
        "by who cancelled"
    );
    let events = rig.events();
    let at = |kind: &str| {
        events
            .iter()
            .skip(from)
            .position(|event| event["type"].as_str() == Some(kind))
    };
    assert!(
        at(fleet_core::item::RUN_CANCELLED) < at(fleet_core::item::ITEM_ENTRY),
        "the cancel precedes the clearance's signal: {events:?}"
    );
    assert_eq!(
        clearances[0].body,
        fleet_core::entry::Body::Cleared(fleet_core::entry::Cleared {
            hold: bare.clone(),
            how: fleet_core::entry::Clearance::Cancel,
            letter: None,
            text: None,
        })
    );
    assert_eq!(clearances[0].by.to_string(), PERSON, "by who cancelled");
}

/// The refusals, each exit 1 on the record as it stands and each leaving it so:
/// an id the store does not hold, an item that is not a run's record, and a run
/// already closed. An empty `--by` is usage, exit 2.
#[test]
fn a_cancel_refuses_what_is_not_an_open_run() {
    let rig = Rig::new(
        "cancel-refuses",
        &Pack::running(WAITS),
        &cap_that_is_not_the_subject(),
    );

    refuses(
        &rig,
        &["cancel", "fx-nothing-here", "--by", BY],
        "fx-nothing-here",
    );

    let made = Command::new("bd")
        .arg("-C")
        .arg(&rig.project)
        .args([
            "create",
            "an item that is not a run",
            "--type",
            "task",
            "--json",
        ])
        .output()
        .expect("bd runs");
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let item: serde_json::Value =
        serde_json::from_slice(&made.stdout).expect("bd create answers JSON");
    let item = item["id"].as_str().expect("an id").to_string();
    let said = refuses(&rig, &["cancel", &item, "--by", BY], &item);
    assert!(said.contains("not a run"), "{said}");
    assert!(is_open(&rig, &item), "the item is untouched");

    let out = rig.run(&["run", &rig.workflow(ONE), "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (id, _) = started_line(&out);
    let out = rig.run(&["cancel", &id, "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let from = rig.stream_length();
    let said = refuses(&rig, &["cancel", &id, "--by", BY], "closed");
    assert!(said.contains(&id), "{said}");
    none_of(&rig, from, fleet_core::item::RUN_CANCELLED);

    // A verb always has an actor, so the one usage refusal left is an empty
    // `--by`: a seat argument that names nothing at all.
    let out = rig.run(&["cancel", &id, "--by", ""]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("fleet cancel: --by names no seat — the argument is empty"),
        "{}",
        stderr(&out)
    );
}
