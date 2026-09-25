//! `fleet store schema` through the shipped binary: the store contract's JSON
//! Schema document, printed whole and exited 0.
//!
//! What the binary prints is held to the committed file byte for byte, and
//! the file is held to the types by the core suite's drift arm — so an
//! adapter author reading the binary reads what the tests pinned.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

/// The document as it is committed.
const COMMITTED: &str = include_str!("../../core/src/store/store.schema.json");

/// The store's documentation, whose verbs table names the verbs.
const STORE_DOC: &str = include_str!("../../docs/store.md");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A home and a machine directory the arm owns, removed when it ends.
struct Machine {
    root: PathBuf,
}

impl Machine {
    fn new(label: &str) -> Machine {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-store-schema-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the temp root is created");
        Machine { root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .hermetic(&self.root.join("home"), &self.root, None)
            .output()
            .expect("the built binary runs")
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The first backticked word of each row of docs/store.md's verbs table: the
/// verbs the contract names.
fn documented_verbs() -> Vec<String> {
    let table = STORE_DOC
        .split("\n## Verbs\n")
        .nth(1)
        .expect("docs/store.md has a Verbs section");
    let mut verbs: Vec<String> = table
        .lines()
        .skip_while(|line| !line.starts_with("| Verb |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| line.split('`').nth(1).map(str::to_string))
        .collect();
    verbs.sort();
    verbs
}

/// The binary prints the committed document, whole, and exits 0 with
/// nothing on stderr.
#[test]
fn schema_prints_the_committed_document_and_exits_zero() {
    let machine = Machine::new("prints");
    let out = machine.run(&["store", "schema"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout) == COMMITTED,
        "fleet store schema does not print core/src/store/store.schema.json — run `cargo run -p \
         fleet-cli -- store schema > core/src/store/store.schema.json`"
    );
}

/// `.verbs | keys` is exactly the verbs docs/store.md's table names.
///
/// RED-PROOF: a row added to the table, or a verb dropped from the document,
/// fails here naming both lists.
#[test]
fn the_documents_verbs_are_the_rows_of_the_docs_verbs_table() {
    let machine = Machine::new("verbs");
    let out = machine.run(&["store", "schema"]);
    let document: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("the document is JSON");
    let verbs: Vec<String> = document["verbs"]
        .as_object()
        .expect("verbs is an object")
        .keys()
        .cloned()
        .collect();
    let documented = documented_verbs();
    assert_eq!(documented.len(), 18, "the table's rows: {documented:?}");
    assert_eq!(verbs, documented);
    assert_eq!(document["schema_version"], 1);
}
