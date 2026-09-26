//! The doctor slot as a runner: which checks the layers resolve, what each one
//! runs, and the verdict its exit gives — over real folders and real `sh`.
//!
//! THE LAYERS ARE THE VERBS' OWN: the binary's defaults written by the same
//! function `fleet start` writes them with, and a packs directory beside them
//! resolved through `Packs::under`, so a check found here is one a verb would
//! find. The scripts are the arm's own, because the subject is the runner and
//! not any check the binary ships.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Fixture;
use fleet_core::item::brief::Packs;
use fleet_core::item::doctor::{self, Entry, Invocation, Verdict};

/// A machine directory holding the defaults and an empty packs directory.
fn machine(label: &str) -> Fixture {
    let fixture = Fixture::new(label);
    fixture.materialize_defaults();
    fixture.dir("packs");
    fixture
}

/// One installed pack named `name`, carrying no runtime and no imports.
fn pack(machine: &Fixture, name: &str) {
    machine.file(
        &format!("packs/{name}/pack.toml"),
        &format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n"),
    );
}

fn packs(machine: &Fixture) -> Packs {
    match Packs::under(
        &machine.path("packs"),
        &machine.path(fleet_core::defaults::DIR),
    ) {
        Ok(packs) => packs,
        Err(stop) => panic!("the layers resolve: {}", stop.message),
    }
}

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

fn found(packs: &Packs, name: &str) -> Entry {
    doctor::entry(packs, name).unwrap_or_else(|| panic!("the layers carry doctor/{name}"))
}

/// An entry the runner can be handed straight, over a script the arm writes.
fn scripted(at: &Fixture, body: &str) -> Entry {
    at.file("check.sh", body);
    Entry {
        name: String::from("fx-check"),
        layer: String::from("scratch"),
        root: at.root.clone(),
        description: None,
        script: Ok(at.path("check.sh")),
    }
}

/// The `PATH` every arm's script runs on: the system's own directories, which
/// hold `sh` and everything a script here calls.
const SYSTEM_PATH: &str = "/usr/bin:/bin";

fn invocation<'a>(pack_dir: &'a Path, timeout: Duration) -> Invocation<'a> {
    Invocation {
        pack_dir,
        path: SYSTEM_PATH,
        cwd: None,
        env: &[],
        timeout,
    }
}

fn ran(entry: &Entry) -> doctor::Checked {
    doctor::run(entry, &invocation(&entry.root, doctor::TIMEOUT))
}

#[test]
fn the_defaults_alone_resolve_every_check_they_ship() {
    let machine = machine("doctor-defaults");
    let packs = packs(&machine);

    let entries = doctor::entries(&packs);

    assert_eq!(
        names(&entries),
        [
            "adopt-board",
            "claude-code-version",
            "fleet-packs-version",
            "guards-installed",
            "isolation-pair",
            "runtime-version",
            "tmux-version",
        ],
        "one entry per doctor directory, in name order"
    );
    for entry in &entries {
        assert_eq!(entry.layer, fleet_core::defaults::LAYER, "{}", entry.name);
        assert_eq!(entry.root, machine.path(fleet_core::defaults::DIR));
        let script = entry
            .script
            .as_ref()
            .unwrap_or_else(|why| panic!("{} runs: {why}", entry.name));
        assert!(script.ends_with("run.sh"), "{}", script.display());
        assert!(
            entry.description.is_some(),
            "{} describes itself",
            entry.name
        );
    }
}

#[test]
fn a_pack_on_top_shadows_a_check_per_file_and_adds_its_own() {
    let machine = machine("doctor-shadow");
    pack(&machine, "scratch");
    machine
        .file(
            "packs/scratch/doctor/guards-installed/doctor.toml",
            "description = \"the scratch copy\"\nrun = \"run.sh\"\n",
        )
        .file("packs/scratch/doctor/guards-installed/run.sh", "exit 0\n")
        .file(
            "packs/scratch/doctor/fx-extra/doctor.toml",
            "description = \"the scratch pack's own\"\nrun = \"run.sh\"\n",
        )
        .file("packs/scratch/doctor/fx-extra/run.sh", "exit 0\n");
    // A second pack carrying the declaration and not the script: resolution is
    // per file, so the script still comes off the defaults.
    pack(&machine, "scratch2");
    machine.file(
        "packs/scratch2/doctor/isolation-pair/doctor.toml",
        "description = \"the scratch2 copy\"\nrun = \"run.sh\"\n",
    );
    let packs = packs(&machine);
    let scratch = machine.path("packs/scratch");
    let defaults = machine.path(fleet_core::defaults::DIR);

    let guards = found(&packs, "guards-installed");
    assert_eq!(guards.layer, "scratch");
    assert_eq!(guards.root, scratch);
    assert_eq!(
        guards.script,
        Ok(scratch.join("doctor/guards-installed/run.sh")),
        "the script is the shadowing pack's"
    );
    assert_eq!(guards.description.as_deref(), Some("the scratch copy"));

    let extra = found(&packs, "fx-extra");
    assert_eq!(extra.layer, "scratch");
    assert!(
        names(&doctor::entries(&packs)).contains(&"fx-extra"),
        "a pack's own check is among the entries"
    );

    let pair = found(&packs, "isolation-pair");
    assert_eq!(
        pair.layer, "scratch2",
        "the layer is the doctor.toml's carrier"
    );
    assert_eq!(
        pair.script,
        Ok(defaults.join("doctor/isolation-pair/run.sh")),
        "the script resolves on its own, to the defaults' copy"
    );

    // Red-proof: without the scratch copy the same name reads the defaults.
    std::fs::remove_dir_all(scratch.join("doctor/guards-installed"))
        .expect("the scratch copy is removed");
    let packs = self::packs(&machine);
    assert_eq!(
        found(&packs, "guards-installed").layer,
        fleet_core::defaults::LAYER
    );
}

#[test]
fn an_entry_that_cannot_be_run_is_could_not_tell_and_spawns_nothing() {
    let machine = machine("doctor-unrunnable");
    pack(&machine, "scratch");
    let marker = machine.path("spawned");
    let touch = format!("touch '{}'\n", marker.display());
    machine
        .file("packs/scratch/doctor/bare/run.sh", &touch)
        .file(
            "packs/scratch/doctor/no-run/doctor.toml",
            "description = \"names nothing\"\n",
        )
        .file("packs/scratch/doctor/no-run/run.sh", &touch)
        .file("packs/scratch/doctor/not-toml/doctor.toml", "run = = \n")
        .file("packs/scratch/doctor/not-toml/run.sh", &touch)
        .file(
            "packs/scratch/doctor/missing/doctor.toml",
            "run = \"missing.sh\"\n",
        )
        .file("packs/scratch/doctor/missing/run.sh", &touch);
    let packs = packs(&machine);

    let cases = [
        ("bare", "`doctor/bare` holds no doctor.toml"),
        ("no-run", "names no `run` script for the check to run"),
        ("not-toml", "does not parse as TOML"),
        (
            "missing",
            "no installed pack carries `doctor/missing/missing.sh`",
        ),
    ];
    for (name, said) in cases {
        let entry = found(&packs, name);
        assert_eq!(entry.layer, "scratch", "{name}");
        let why = entry
            .script
            .as_ref()
            .expect_err(&format!("{name} cannot be run"));
        assert!(why.contains(said), "{name}: {why}");

        let checked = ran(&entry);
        assert_eq!(checked.verdict, Verdict::CouldNotTell, "{name}");
        assert_eq!(checked.exit, None, "{name}");
        assert_eq!(&checked.line, why, "{name}: the line is the reason");
        assert!(checked.stdout.is_empty() && checked.stderr.is_empty());
    }
    assert!(
        !marker.exists(),
        "no script ran for an entry that cannot be run"
    );
}

#[test]
fn the_exit_gives_the_verdict() {
    let rows: [(&str, Verdict, Option<i32>); 6] = [
        ("exit 0\n", Verdict::Pass, Some(0)),
        ("exit 1\n", Verdict::Finding, Some(1)),
        ("exit 3\n", Verdict::CouldNotTell, Some(3)),
        ("exit 2\n", Verdict::CouldNotTell, Some(2)),
        ("exit 7\n", Verdict::CouldNotTell, Some(7)),
        ("kill -9 $$\n", Verdict::CouldNotTell, None),
    ];
    for (body, verdict, exit) in rows {
        let at = Fixture::new("doctor-exit");
        let checked = ran(&scripted(&at, body));
        assert_eq!((checked.verdict, checked.exit), (verdict, exit), "{body}");
    }

    assert_eq!(
        Verdict::aggregate([Verdict::Pass, Verdict::Finding]),
        Verdict::Finding
    );
    assert_eq!(
        Verdict::aggregate([Verdict::Finding, Verdict::CouldNotTell, Verdict::Pass]),
        Verdict::CouldNotTell
    );
    assert_eq!(Verdict::aggregate([]), Verdict::Pass);
}

#[test]
fn a_verdict_spells_itself_three_ways() {
    let rows = [
        (Verdict::Pass, "pass", "pass", 0),
        (Verdict::Finding, "finding", "finding", 1),
        (Verdict::CouldNotTell, "could not tell", "could_not_tell", 3),
    ];
    for (verdict, word, code, exit) in rows {
        assert_eq!(verdict.word(), word);
        assert_eq!(verdict.code(), code);
        assert_eq!(verdict.exit(), exit);
        assert_eq!(Verdict::of_exit(Some(i32::from(exit))), verdict);
    }
    assert_eq!(Verdict::of_exit(None), Verdict::CouldNotTell);
}

#[test]
fn the_line_is_the_last_thing_the_check_said() {
    let rows = [
        ("printf 'a\\nlast\\n\\n'; exit 1\n", "last"),
        ("echo said; echo why >&2; exit 3\n", "said"),
        ("echo why >&2; exit 3\n", "why"),
        ("exit 1\n", "said nothing and exited 1"),
        ("kill -9 $$\n", "was killed by a signal and said nothing"),
    ];
    for (body, line) in rows {
        let at = Fixture::new("doctor-line");
        assert_eq!(ran(&scripted(&at, body)).line, line, "{body}");
    }
}

#[test]
fn a_check_past_its_bound_is_killed_and_could_not_tell() {
    let at = Fixture::new("doctor-bound");
    let entry = scripted(&at, "sleep 30\n");

    let started = Instant::now();
    let checked = doctor::run(&entry, &invocation(&at.root, Duration::from_millis(200)));
    let took = started.elapsed();

    assert_eq!(checked.verdict, Verdict::CouldNotTell);
    assert_eq!(checked.exit, None);
    assert_eq!(checked.line, "did not answer within 200ms");
    assert!(
        took < Duration::from_secs(5),
        "the bound cut the check short, in {took:?}"
    );
    assert_eq!(doctor::TIMEOUT, Duration::from_secs(60));
}

#[test]
fn the_check_runs_in_the_invocations_environment_and_directory() {
    let at = Fixture::new("doctor-env");
    at.dir("pack").dir("cwd").dir("bin");
    let pack_dir = at.path("pack");
    // Canonical, because `sh` reads its `$PWD` off the directory it was
    // started in, and the temp directory is a symlink on this platform.
    let cwd = std::fs::canonicalize(at.path("cwd")).expect("the cwd resolves");
    let path = format!("{SYSTEM_PATH}:{}", at.path("bin").display());
    let extra = std::ffi::OsStr::new("the extra value");
    let env = [("FX_EXTRA", extra)];
    let entry = scripted(
        &at,
        "printf '%s\\n' \"$FLEET_PACK_DIR\" \"$PWD\" \"$PATH\" \"$FX_EXTRA\"\n",
    );

    let checked = doctor::run(
        &entry,
        &Invocation {
            pack_dir: &pack_dir,
            path: &path,
            cwd: Some(&cwd),
            env: &env,
            timeout: doctor::TIMEOUT,
        },
    );

    assert_eq!(checked.verdict, Verdict::Pass, "{}", checked.stderr);
    let lines: Vec<&str> = checked.stdout.lines().collect();
    assert_eq!(
        lines,
        [
            pack_dir.display().to_string().as_str(),
            cwd.display().to_string().as_str(),
            path.as_str(),
            "the extra value",
        ]
    );
}

#[test]
fn pinned_is_every_pack_that_declares_a_runtime_top_first() {
    let machine = machine("doctor-pinned");
    machine
        .file(
            "packs/top/pack.toml",
            "[pack]\nname = \"top\"\nversion = \"0.1.0\"\nschema = 3\n\n\
             [imports.mid]\nsource = \"../mid\"\nversion = \"0.1.0\"\n\n\
             [runtime]\nname = \"fx-runtime\"\nversion = \"1.2.3\"\n\
             bundle = \"cp {entry} {bundle}\"\nrun = \"sh {bundle}\"\n",
        )
        .file(
            "packs/mid/pack.toml",
            "[pack]\nname = \"mid\"\nversion = \"0.1.0\"\nschema = 3\n",
        );
    let packs = packs(&machine);
    let order: Vec<&str> = packs.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(order, ["top", "mid", fleet_core::defaults::LAYER]);

    let pinned = doctor::pinned(&packs);

    let read: Vec<(&str, Result<&str, &String>)> = pinned
        .iter()
        .map(|(layer, runtime)| {
            (
                layer.name.as_str(),
                runtime.as_ref().map(|r| r.name.as_str()),
            )
        })
        .collect();
    assert_eq!(read, [("top", Ok("fx-runtime"))]);
}

#[test]
fn pinned_names_a_manifest_that_does_not_parse() {
    let machine = machine("doctor-pinned-broken");
    machine.file("packs/top/pack.toml", "[pack\nname = \n");
    let packs = packs(&machine);

    let pinned = doctor::pinned(&packs);

    assert_eq!(pinned.len(), 1, "{pinned:?}");
    assert_eq!(pinned[0].0.name, "top");
    assert!(pinned[0].1.is_err(), "{:?}", pinned[0].1);
}

/// The runtime check `fleet run` takes goes through this module's runner: the
/// function that runs it builds no command of its own.
#[test]
fn the_runs_doctor_builds_no_command_of_its_own() {
    let source: PathBuf = [env!("CARGO_MANIFEST_DIR"), "src", "item", "run.rs"]
        .iter()
        .collect();
    let text = std::fs::read_to_string(&source).expect("run.rs is readable");
    let start = text
        .find("fn the_doctor_is_green(")
        .expect("run.rs carries the run's doctor");
    let body = &text[start..];
    let end = body.find("\n}\n").expect("the function ends");

    assert!(
        !body[..end].contains("Command::new"),
        "the_doctor_is_green runs its check through doctor::run"
    );
}
