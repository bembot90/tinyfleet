//! The `pack check` subcommand through the shipped binary: the three exit
//! codes, and the defect lines a person reads.
//!
//! 0 is a valid pack, 1 is a pack with defects or a layering that refuses, 2 is
//! a caller who did not say what to check.

use std::path::{Path, PathBuf};
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

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination directory is created");
    for entry in std::fs::read_dir(from).expect("the source directory is readable") {
        let entry = entry.expect("an entry is readable");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file is copied");
        }
    }
}

/// One of the fixture packs, by name: `tiny`, shaped as the doctrine pack, or
/// `ts`, shaped as the runtime pack ([`fleet_core::test_support::fixture_pack`]).
fn fixture(name: &str) -> PathBuf {
    fleet_core::test_support::fixture_pack(name)
}

/// A copy of a fixture pack, so a defect is planted in something that was
/// valid a moment ago rather than in a hand-built lookalike.
struct Copy {
    root: PathBuf,
}

impl Copy {
    fn of_fixture(label: &str) -> Copy {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-pack-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // The WHOLE pack, walked rather than listed: a hand-written list goes
        // stale the first time the fixture carries another slot. The RUNTIME
        // pack is the one copied, because it declares no imports and publishes
        // no registry — so a layering arm below reads the defect it planted and
        // not a transitive import or a shadow rule.
        copy_tree(&fixture("ts"), &root);
        Copy { root }
    }

    fn write(&self, relative: &str, contents: &str) -> &Copy {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the parent is created");
        }
        std::fs::write(path, contents).expect("the file is written");
        self
    }

    fn arg(&self) -> &str {
        self.root.to_str().expect("the temp path is utf-8")
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_doctrine_shaped_pack_checks_clean() {
    let out = run(&[
        "pack",
        "check",
        fixture("tiny").to_str().expect("a utf-8 path"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack tiny 0.1.0"), "{stdout}");
    assert!(
        stdout.contains("slot assets"),
        "one line per slot: {stdout}"
    );
    // The control for the arm below: a pack that pins no runtime prints no
    // runtime line, so the line is the pin as parsed and not a fixed row.
    assert!(!stdout.contains("runtime"), "{stdout}");
}

/// A runtime pack, and the doctrine-shaped pack checked over it as a fleet
/// layers the two: clean, and the report carries the pin the manifest's
/// [runtime] table declares.
#[test]
fn a_runtime_pack_reports_its_pin_and_the_doctrine_shaped_pack_resolves_over_it() {
    let ts = fixture("ts");
    let out = run(&["pack", "check", ts.to_str().expect("a utf-8 path")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack ts 0.1.0"), "{stdout}");
    assert!(stdout.contains("runtime deno 2.4.5"), "{stdout}");
    assert!(stdout.contains("slot doctor: 1 entry"), "{stdout}");

    let out = run(&[
        "pack",
        "check",
        fixture("tiny").to_str().expect("a utf-8 path"),
        "--over",
        ts.to_str().expect("a utf-8 path"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack tiny 0.1.0"), "{stdout}");
    assert!(
        stdout.contains("resolved"),
        "the layering over the runtime pack resolves: {stdout}"
    );
}

#[test]
fn a_ninth_top_level_name_exits_one_and_names_it() {
    let copy = Copy::of_fixture("ninth");
    copy.write("bin/dispatch", "#!/bin/sh\n");
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unknown top-level name `bin`"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_agent_directory_holding_neither_form_exits_one_and_names_it() {
    let copy = Copy::of_fixture("agent");
    copy.write("agents/dispatcher/notes.md", "nothing\n");
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out)
            .contains("`agents/dispatcher` holds neither agent.toml nor prompt.template.md"),
        "{}",
        stderr(&out)
    );
}

/// An adapter filed where its manifest says is one `adapters` line; an
/// unknown kind directory, and a manifest naming another kind than its
/// directory, each exit 1 with the defect line.
///
/// RED-PROOF: on the base `adapters` is an unknown top-level name.
#[test]
fn the_adapters_slot_is_counted_and_a_kind_out_of_place_exits_one() {
    let copy = Copy::of_fixture("adapters");
    let toml = |kind: &str| {
        format!(
            "[adapter]\nname = \"x\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
             entry = \"main.sh\"\n"
        )
    };
    copy.write("adapters/store/x/adapter.toml", &toml("store"))
        .write("adapters/store/x/main.sh", "#!/bin/sh\n");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        copy.root.join("adapters/store/x/main.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("the entry is executable");
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("slot adapters: 1 entry"), "{stdout}");

    copy.write("adapters/db/y/adapter.toml", &toml("db"))
        .write("adapters/store/x/adapter.toml", &toml("agent"));
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(1));
    for line in [
        "`adapters/db` is no adapter kind — an adapter is filed under adapters/store/ or \
         adapters/agent/",
        "`adapters/store/x/adapter.toml` says kind `agent`, and it is filed under \
         `adapters/store`",
    ] {
        assert!(stderr(&out).contains(line), "{line}\n{}", stderr(&out));
    }
}

/// An agent adapter's `[hook]` is held to the mapping's shape: a `[hook]` on a
/// store adapter, a `deny` with no `{reason}`, and a pointer that is not a
/// JSON pointer each exit 1 with one defect line.
///
/// RED-PROOF: on the base every `[hook]` is the same unknown-table line.
#[test]
fn a_hook_out_of_place_or_out_of_shape_is_one_defect_line_each() {
    let copy = Copy::of_fixture("hook");
    let manifest = |kind: &str, name: &str, hook: &str| {
        format!(
            "[adapter]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
             entry = \"main.sh\"\n\n[hook]\nshell_tool = \"sh\"\ntool = \"/call\"\n{hook}"
        )
    };
    for (kind, name, hook) in [
        (
            "store",
            "s",
            "command = \"/line\"\ndeny = '{\"why\":{reason}}'\n",
        ),
        (
            "agent",
            "a",
            "command = \"/line\"\ndeny = '{\"why\":\"no\"}'\n",
        ),
        (
            "agent",
            "b",
            "command = \"line\"\ndeny = '{\"why\":{reason}}'\n",
        ),
    ] {
        let dir = format!("adapters/{kind}/{name}");
        copy.write(&format!("{dir}/adapter.toml"), &manifest(kind, name, hook))
            .write(&format!("{dir}/main.sh"), "#!/bin/sh\n");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            copy.root.join(format!("{dir}/main.sh")),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("the entry is executable");
    }
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(1));
    let said = stderr(&out);
    for line in [
        "ts: `adapters/store/s/adapter.toml`'s `[hook]` is a table only an agent adapter holds",
        "ts: `adapters/agent/a/adapter.toml`'s [hook] `deny` holds no {reason}",
        "ts: `adapters/agent/b/adapter.toml`'s [hook] `command` is `line`, which is not a JSON \
         pointer",
    ] {
        assert_eq!(
            said.lines().filter(|said| said.starts_with(line)).count(),
            1,
            "{line}\n{said}"
        );
    }
    assert_eq!(said.lines().count(), 3, "one line each: {said}");
}

/// A pack that turns on a guard class core does not carry exits 1 with one
/// defect line naming the class it wrote and the four it could have.
///
/// RED-PROOF: on the base the line is the unknown-key one and names no class.
#[test]
fn a_guard_class_outside_the_four_is_one_defect_line_naming_the_four() {
    let copy = Copy::of_fixture("guard-classes");
    copy.write(
        "pack.toml",
        "[pack]\nname = \"ts\"\nversion = \"0.1.0\"\nschema = 3\nguard_classes = [\"shell\"]\n",
    );
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(1));
    let said = stderr(&out);
    assert_eq!(said.lines().count(), 1, "one line: {said}");
    for name in [
        "`shell`",
        "shell-trap",
        "record",
        "release-ref",
        "production-write",
    ] {
        assert!(said.contains(name), "the line names {name}: {said}");
    }
}

#[test]
fn a_manifest_at_the_previous_schema_exits_one_and_names_the_number() {
    let copy = Copy::of_fixture("schema");
    copy.write(
        "pack.toml",
        "[pack]\nname = \"ts\"\nversion = \"0.1.0\"\nschema = 2\n",
    );
    let out = run(&["pack", "check", copy.arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("[pack] schema is 2, not 3"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn over_runs_the_resolver_and_refuses_a_collision() {
    let top = Copy::of_fixture("over-top");
    top.write(
        "pack.toml",
        "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
    )
    .write("agents/architect/agent.toml", "name = \"architect\"\n");
    let base = Copy::of_fixture("over-base");
    base.write("agents/architect/agent.toml", "name = \"architect\"\n");

    let out = run(&["pack", "check", top.arg(), "--over", base.arg()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("the agent name `architect` is in both `top` and `ts`"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn over_resolves_two_clean_layers() {
    let top = Copy::of_fixture("clean-top");
    top.write(
        "pack.toml",
        "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
    )
    .write("agents/architect/agent.toml", "name = \"architect\"\n");
    let base = Copy::of_fixture("clean-base");

    let out = run(&["pack", "check", top.arg(), "--over", base.arg()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("resolved"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn no_directory_is_a_usage_error_and_not_a_defect() {
    let out = run(&["pack", "check"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "usage goes to stderr, never stdout");
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}
