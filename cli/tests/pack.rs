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

/// One of the shipped packs, by name, from the workspace this crate sits in.
fn shipped(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits inside the workspace")
        .join("packs")
        .join(name)
}

/// A copy of the shipped pack, so a defect is planted in something that was
/// valid a moment ago rather than in a hand-built lookalike.
struct Copy {
    root: PathBuf,
}

impl Copy {
    fn of_shipped(label: &str) -> Copy {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-pack-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // The WHOLE pack, walked rather than listed: a hand-written list goes
        // stale the first time the pack ships another slot. The RUNTIME pack is
        // the one copied, because it declares no imports and publishes no
        // registry — so a layering arm below reads the defect it planted and
        // not a transitive import or a shadow rule.
        copy_tree(&shipped("ts"), &root);
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
fn the_doctrine_pack_checks_clean() {
    let out = run(&[
        "pack",
        "check",
        shipped("tiny").to_str().expect("a utf-8 path"),
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

/// The runtime pack, and the doctrine pack checked over it as a real fleet
/// layers the two: clean, and the report carries the pin the manifest's
/// [runtime] table declares.
#[test]
fn the_runtime_pack_reports_its_pin_and_the_doctrine_pack_resolves_over_it() {
    let ts = shipped("ts");
    let out = run(&["pack", "check", ts.to_str().expect("a utf-8 path")]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("pack ts 0.1.0"), "{stdout}");
    assert!(stdout.contains("runtime deno 2.9.7"), "{stdout}");
    assert!(stdout.contains("slot doctor: 1 entry"), "{stdout}");

    let out = run(&[
        "pack",
        "check",
        shipped("tiny").to_str().expect("a utf-8 path"),
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
    let copy = Copy::of_shipped("ninth");
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
    let copy = Copy::of_shipped("agent");
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

#[test]
fn a_manifest_at_the_previous_schema_exits_one_and_names_the_number() {
    let copy = Copy::of_shipped("schema");
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
    let top = Copy::of_shipped("over-top");
    top.write(
        "pack.toml",
        "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
    )
    .write("agents/architect/agent.toml", "name = \"architect\"\n");
    let base = Copy::of_shipped("over-base");
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
    let top = Copy::of_shipped("clean-top");
    top.write(
        "pack.toml",
        "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
    )
    .write("agents/architect/agent.toml", "name = \"architect\"\n");
    let base = Copy::of_shipped("clean-base");

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
