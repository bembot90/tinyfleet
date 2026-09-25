//! The workspace's shape, read from cargo rather than asserted about the files.
//!
//! The boundary this pins is a one-way edge: core holds pack-and-project work
//! and never reads the process table, so a dependency from core on the
//! controller is refused here rather than left to discipline. `cargo metadata
//! --no-deps` is the reading, because it answers about the manifests in this
//! workspace and not about the crates.io graph beneath them.

use std::process::Command;

fn metadata() -> serde_json::Value {
    // The workspace root is this crate's parent. --no-deps keeps the answer to
    // the members, and --locked keeps a test from writing the lock file.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits inside the workspace")
        .join("Cargo.toml");
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "cargo metadata exits 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("cargo metadata prints JSON")
}

fn packages(meta: &serde_json::Value) -> Vec<String> {
    meta["packages"]
        .as_array()
        .expect("packages is an array")
        .iter()
        .map(|p| p["name"].as_str().expect("a package name").to_string())
        .collect()
}

#[test]
fn the_workspace_holds_exactly_three_members() {
    let meta = metadata();
    let mut names = packages(&meta);
    names.sort();
    assert_eq!(names, ["fleet-cli", "fleet-controller", "fleet-core"]);
}

#[test]
fn core_does_not_depend_on_the_controller() {
    let meta = metadata();
    let core = meta["packages"]
        .as_array()
        .expect("packages is an array")
        .iter()
        .find(|p| p["name"] == "fleet-core")
        .expect("fleet-core is a member");
    let named: Vec<&str> = core["dependencies"]
        .as_array()
        .expect("dependencies is an array")
        .iter()
        .map(|d| d["name"].as_str().expect("a dependency name"))
        .collect();
    assert!(
        !named.contains(&"fleet-controller"),
        "fleet-core must not depend on fleet-controller; it names {named:?}"
    );
}

// The control for the arm above: without it, an empty or unreadable dependency
// list would satisfy the refusal and say nothing. The list is asserted to be
// populated rather than to hold an exact set, so a later slice's dependency
// leaves the control standing.
/// A library crate never prints, so the three presentation crates are named by
/// the binary and by neither library member. The reading is
/// each member's own dependency list, which is the same list `cargo tree -i`
/// walks from the other end.
#[test]
fn no_library_member_depends_on_the_presentation_crates() {
    let meta = metadata();
    for member in ["fleet-core", "fleet-controller"] {
        let package = meta["packages"]
            .as_array()
            .expect("packages is an array")
            .iter()
            .find(|p| p["name"] == member)
            .expect("the member is in the workspace");
        let named: Vec<&str> = package["dependencies"]
            .as_array()
            .expect("dependencies is an array")
            .iter()
            .map(|d| d["name"].as_str().expect("a dependency name"))
            .collect();
        for crate_name in ["console", "indicatif", "dialoguer"] {
            assert!(
                !named.contains(&crate_name),
                "{member} must not depend on {crate_name}; it names {named:?}"
            );
        }
    }
}

/// The control for the arm above: fleet-cli DOES name all three, so the
/// refusal above is about where they live and not about a set nobody took.
#[test]
fn the_binary_is_the_member_that_names_all_three() {
    let meta = metadata();
    let cli = meta["packages"]
        .as_array()
        .expect("packages is an array")
        .iter()
        .find(|p| p["name"] == "fleet-cli")
        .expect("fleet-cli is a member");
    let named: Vec<&str> = cli["dependencies"]
        .as_array()
        .expect("dependencies is an array")
        .iter()
        .map(|d| d["name"].as_str().expect("a dependency name"))
        .collect();
    for crate_name in ["console", "indicatif", "dialoguer"] {
        assert!(
            named.contains(&crate_name),
            "fleet-cli names {crate_name}; it names {named:?}"
        );
    }
}

#[test]
fn the_dependency_list_core_was_read_from_is_populated() {
    let meta = metadata();
    let core = meta["packages"]
        .as_array()
        .expect("packages is an array")
        .iter()
        .find(|p| p["name"] == "fleet-core")
        .expect("fleet-core is a member");
    let named: Vec<&str> = core["dependencies"]
        .as_array()
        .expect("dependencies is an array")
        .iter()
        .map(|d| d["name"].as_str().expect("a dependency name"))
        .collect();
    assert!(
        named.contains(&"serde"),
        "fleet-core's dependency list is read, not empty; it names {named:?}"
    );
}

/// The run lifecycle's vocabulary is spelled in two crates and is one fact.
///
/// The controller crate takes nothing from core but its bounded runner
/// (`fleet_core::process`), the release it supports (`fleet_core::supported`)
/// and a seat's identity (`fleet_core::seat::identity`: the id, the fleet.toml
/// roster and the resolver), so the kinds its run pass folds and writes, and the
/// environment variable a run's children carry, are spelled there as well as in
/// core. This is the one member that can see both, which makes it the place
/// the two spellings are held to one string — a kind spelled twice is two
/// kinds, and a fold that met the second would report a stream with no runs in
/// it.
#[test]
fn the_run_vocabulary_is_one_string_in_both_crates() {
    use fleet_controller::runs;
    use fleet_core::item;

    for (controller, core) in [
        (runs::RUN_STARTED, item::RUN_STARTED),
        (runs::RUN_CLOSED, item::RUN_CLOSED),
        (runs::RUN_FAILED, item::RUN_FAILED),
        (runs::RUN_WAITING, item::RUN_WAITING),
        (runs::RUN_COULD_NOT_TELL, item::RUN_COULD_NOT_TELL),
        (runs::RUN_CANCELLED, item::RUN_CANCELLED),
        (runs::RUN_CLEANED, item::RUN_CLEANED),
        (runs::ITEM_ENTRY, item::ITEM_ENTRY),
        (runs::ENV_RUN_ID, item::run::ENV_RUN_ID),
    ] {
        assert_eq!(
            controller, core,
            "the controller's spelling and core's are one string"
        );
    }
}

/// The kinds a wake may name are core's entry kinds, in core's order: a wake
/// the SDK throws naming a kind is readable, and none waits on a kind no
/// signal carries.
#[test]
fn the_kinds_a_wake_names_are_core_s_entry_kinds() {
    assert_eq!(
        fleet_controller::runs::ENTRY_KINDS,
        fleet_core::entry::KINDS,
        "the controller's spelling and core's are one list"
    );
}

/// The stream kinds the verbs wrote before every entry was one signal —
/// retired, and spelled nowhere a writer or a reader of the stream lives: no
/// string literal in core, the binary or the controller, and none in the SDK a
/// workflow runs on. A line of one of them now would be a kind nothing reads.
#[test]
fn no_source_spells_a_retired_item_kind() {
    const RETIRED: [&str; 7] = [
        "item.delivered",
        "item.reviewed",
        "item.returned",
        "item.landed",
        "item.dispatched",
        "item.held",
        "hold.cleared",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits inside the workspace");
    let grep = |needle: &str, paths: &[&str]| -> Vec<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["grep", "-n", "-F", "-e", needle, "--"])
            .args(paths)
            .output()
            .expect("git grep runs");
        // 0 is found and 1 is found nothing; anything else is git failing,
        // which would read as a clean tree.
        assert!(
            matches!(out.status.code(), Some(0 | 1)),
            "git grep answers: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    };
    let rust = ["core/src", "cli/src", "controller/src"];
    let sdk = ["packs/ts/assets/sdk/mod.ts"];

    // THE CONTROL: the kind that replaced them is found, as a literal, by the
    // same search — so an empty answer below is the tree's and not the
    // search's.
    assert!(
        !grep("\"item.entry\"", &rust).is_empty(),
        "core and the controller spell item.entry"
    );

    let mut found: Vec<String> = Vec::new();
    for kind in RETIRED {
        found.extend(grep(&format!("\"{kind}\""), &rust));
        for quote in ['"', '\'', '`'] {
            found.extend(grep(&format!("{quote}{kind}{quote}"), &sdk));
        }
    }
    assert!(found.is_empty(), "retired kinds spelled: {found:#?}");
}

/// And the run's crash cap has one default, for the same reason: the controller
/// reads `[core.run] max_crashes` off the policy in force and core's own reader
/// answers the same key.
#[test]
fn the_run_crash_cap_has_one_default_in_both_crates() {
    assert_eq!(
        fleet_controller::policy::DEFAULT_RUN_MAX_CRASHES,
        fleet_core::item::run::MAX_CRASHES,
        "a fleet naming no cap gets the same number whichever crate answers"
    );
    assert_eq!(fleet_core::item::run::MAX_CRASHES, 2);
}

/// `run.cleaned` declares the keys it carries, where the four rows of the exit
/// table do — the writer is the controller and the fold is core's, and the
/// payload table is where the two agree.
#[test]
fn the_cleanup_declares_the_keys_it_carries() {
    assert_eq!(
        fleet_core::item::payload_keys(fleet_core::item::RUN_CLEANED),
        Some(["run", "count"].as_slice()),
    );
}
