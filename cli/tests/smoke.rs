//! The harness itself: that a test binary builds, runs the shipped executable
//! and reads its exit status. Every later slice's fixture tests land beside
//! these.

use std::process::Command;

mod common;
use common::hermetic::Hermetic;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(args)
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs")
}

#[test]
fn version_prints() {
    let out = run(&["--version"]);
    assert!(out.status.success(), "--version exits 0");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn no_arguments_prints_usage_to_stderr() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "usage goes to stderr, never stdout");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Usage: fleet"), "{stderr}");
    for family in ["observe", "seat", "event", "pack", "guard"] {
        assert!(
            stderr.contains(family),
            "the usage omits {family}: {stderr}"
        );
    }
}
