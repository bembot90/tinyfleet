//! The contract both stores answer, asserted against each of them.
//!
//! The arms of every other suite here run against the board held in memory, so
//! what that board answers has to be what `bd` answers — and the only way to
//! say that is to ask the same question of both and compare the answers to the
//! same expectation. Every check below is one function; each has an arm of its
//! own against [`common::Board`]'s store, and [`every_check_holds_against_bd_too`]
//! runs the whole table against `bd`.
//!
//! WHY THE REAL HALF IS ONE ARM AND NOT TEN. A nextest arm is its own process,
//! so a shared store is shared only with itself and every arm that wants `bd`
//! pays a `bd init` — 3.5 s on an idle box, three times that under the run's
//! own parallelism. Ten arms would buy ten inits and the same ten readings.
//!
//! WHAT IS ASSERTED IS THE CONTRACT AND NOT THE ROW. The real store writes
//! fields no trait method answers — a close reason, an updated stamp — and two
//! stores agreeing byte for byte on a document is not what the verbs need. What
//! they need is that a write moves what the read beside it answers, in the same
//! direction, and that is what each check names.
//!
//! The real half runs every check against ONE board, so a check reads the item
//! it filed and never the whole store: a listing asserted whole here would be
//! asserting what the check before it left behind.

mod common;

use std::path::Path;

use common::shared_store;
use fleet_core::store::{Bd, NewItem, Store};
use fleet_core::test_support::Board;

const BY: &str = "the-contract";

/// One check: the store, the root it writes under — the export is a file and
/// the two stores keep theirs in two places — and the name of the half it is
/// running against, so a red says which store disagreed.
type Check = fn(&dyn Store, &Path, &str);

/// Every check in this file, by name. The real half walks this table, so a
/// check that is added below and left out here is a check `bd` never answers —
/// which [`the_table_names_every_check`] is what refuses.
const CHECKS: &[(&str, Check)] = &[
    ("create then show", create_then_show),
    ("show of an absent item", show_of_an_absent_item),
    ("assign", assign),
    ("note", note),
    ("set_title", set_title),
    ("set_orders then unset_orders", orders),
    ("set_metadata's merge", metadata_merge),
    ("gate, open_gates, resolve_gate", gates),
    ("close", close),
    ("export", export),
];

/// One check against the board held in memory, under a root of its own.
fn in_memory(label: &str, check: Check) {
    let board = Board::new(label);
    check(&board.store, &board.root, "the board held in memory");
}

fn filed(store: &dyn Store, title: &str) -> String {
    store
        .create(
            &NewItem {
                title,
                description: "an item the contract suite filed",
                item_type: "task",
                labels: &["a-label"],
            },
            BY,
        )
        .expect("the item is filed")
}

// ---- the checks --------------------------------------------------------------

fn create_then_show(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item to read back");
    assert!(!item.is_empty(), "{which}: the store names what it files");

    let read = store.show(&item).expect("the item reads");
    assert_eq!(read.id, item, "{which}: the read is of the item asked for");
    assert_eq!(read.title, "an item to read back", "{which}");
    assert_eq!(read.status, "open", "{which}: a fresh item is open");
    assert_eq!(read.item_type, "task", "{which}");
    assert!(
        read.labels.iter().any(|label| label == "a-label"),
        "{which}: the labels the new item carried: {:?}",
        read.labels
    );
    assert_eq!(
        read.assignee, None,
        "{which}: an item nobody has assigned carries no assignee at all"
    );
    assert_eq!(read.notes, None, "{which}: and no notes");
}

fn show_of_an_absent_item(store: &dyn Store, _: &Path, which: &str) {
    match store.show("fx-nobody-filed-this") {
        Err(fleet_core::store::StoreError::Missing(_)) => {}
        other => panic!("{which}: an absent item is Missing, and answered {other:?}"),
    }
}

fn assign(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item to hand over");
    store
        .assign(&item, "a-seat", BY)
        .expect("the assignment lands");
    assert_eq!(
        store
            .show(&item)
            .expect("the item reads")
            .assignee
            .as_deref(),
        Some("a-seat"),
        "{which}"
    );

    store
        .assign(&item, "another-seat", BY)
        .expect("the second assignment lands");
    assert_eq!(
        store
            .show(&item)
            .expect("the item reads")
            .assignee
            .as_deref(),
        Some("another-seat"),
        "{which}: the last write is what the read answers"
    );
}

fn note(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item to write up");
    store
        .note(&item, "the first note\nover two lines", BY)
        .expect("the note lands");
    let notes = store
        .show(&item)
        .expect("the item reads")
        .notes
        .unwrap_or_default();
    assert!(
        notes.contains("the first note") && notes.contains("over two lines"),
        "{which}: the note is readable whole: {notes:?}"
    );

    store
        .note(&item, "the second note", BY)
        .expect("the second note lands");
    let notes = store
        .show(&item)
        .expect("the item reads")
        .notes
        .unwrap_or_default();
    assert!(
        notes.contains("the first note") && notes.contains("the second note"),
        "{which}: a note is appended and never a replacement: {notes:?}"
    );
    assert!(
        notes.find("the first note") < notes.find("the second note"),
        "{which}: in the order they were written: {notes:?}"
    );
}

fn set_title(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "the title it was filed under");
    store
        .set_title(&item, "the title it carries now", BY)
        .expect("the title lands");
    assert_eq!(
        store.show(&item).expect("the item reads").title,
        "the title it carries now",
        "{which}"
    );
}

fn orders(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item with an order on it");
    let read = store.show(&item).expect("the item reads");
    assert!(
        !read.has_orders_key,
        "{which}: nothing has written an order yet"
    );
    assert_eq!(read.orders, None, "{which}");

    store
        .set_orders(
            &item,
            r#"{"orders":{"by":"an-architect","kind":"dispatch","seat":"a-seat","at":"2026-09-13T00:00:00Z"}}"#,
            BY,
        )
        .expect("the order index lands");
    let read = store.show(&item).expect("the item reads");
    assert!(read.has_orders_key, "{which}: the key is there");
    let index = read.orders.expect("the index is an object");
    assert_eq!(index.by.as_deref(), Some("an-architect"), "{which}");
    assert_eq!(index.kind.as_deref(), Some("dispatch"), "{which}");
    assert_eq!(index.seat.as_deref(), Some("a-seat"), "{which}");
    assert_eq!(index.at.as_deref(), Some("2026-09-13T00:00:00Z"), "{which}");
    assert_eq!(
        index.ordinal, None,
        "{which}: a key nobody wrote is absent and not zero"
    );

    store.unset_orders(&item, BY).expect("the withdrawal lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(read.orders, None, "{which}: the index is gone");
    assert!(
        !read.has_orders_key,
        "{which}: and the key with it — a withdrawal is told from an unreadable value"
    );
}

fn metadata_merge(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item with two metadata keys");
    store
        .set_metadata(&item, r#"{"a_prior_key":{"kept":true}}"#, BY)
        .expect("the first key lands");
    store
        .set_metadata(&item, r#"{"run":{"items":["fx-one"]}}"#, BY)
        .expect("the second key lands");

    let read = store.show(&item).expect("the item reads");
    assert!(
        read.document.contains("a_prior_key") && read.document.contains("kept"),
        "{which}: the write MERGES at the top level: {}",
        read.document
    );
    assert_eq!(
        read.run,
        Some(serde_json::json!({ "items": ["fx-one"] })),
        "{which}: and the key it wrote is there"
    );

    store
        .set_metadata(&item, r#"{"run":{"hash":"deadbeef"}}"#, BY)
        .expect("a second write of the same key lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.run,
        Some(serde_json::json!({ "hash": "deadbeef" })),
        "{which}: one key's own object is REPLACED, not merged into"
    );
    assert!(
        read.document.contains("a_prior_key"),
        "{which}: the top level still merged: {}",
        read.document
    );
}

fn gates(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item to park");
    let gate = store
        .gate(&item, "the question this park asks", BY)
        .expect("the gate is raised");
    assert!(!gate.is_empty(), "{which}: the store names the gate");
    assert!(
        store
            .open_gates()
            .expect("the open listing answers")
            .contains(&gate),
        "{which}: a raised gate is open"
    );

    store.resolve_gate(&gate, BY).expect("the gate resolves");
    assert!(
        !store
            .open_gates()
            .expect("the open listing answers")
            .contains(&gate),
        "{which}: and a resolved one is not"
    );
}

fn close(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item to close");
    assert_eq!(
        store.show(&item).expect("the item reads").status,
        "open",
        "{which}"
    );

    store
        .close(&item, "closed by the contract suite", BY)
        .expect("the close lands");
    assert_eq!(
        store.show(&item).expect("the item reads").status,
        "closed",
        "{which}: the close is what the read answers"
    );
}

fn export(store: &dyn Store, root: &Path, which: &str) {
    let item = filed(store, "an item the export carries");
    store.export(root).expect("the export runs");

    let into = root.join(fleet_core::store::EXPORT);
    let before = std::fs::read(&into)
        .unwrap_or_else(|e| panic!("{which}: the export left a file at {}: {e}", into.display()));
    assert!(!before.is_empty(), "{which}: and the file is not empty");
    assert!(
        String::from_utf8_lossy(&before).contains(&item),
        "{which}: carrying the item that was filed"
    );

    store
        .note(&item, "a note the second export has to carry", BY)
        .expect("the note lands");
    store.export(root).expect("the second export runs");
    let after = std::fs::read(&into).expect("the export is still there");
    assert_ne!(
        before, after,
        "{which}: the bytes move when an item does, which is what a landing's gate reads"
    );
}

// ---- the arms ----------------------------------------------------------------

#[test]
fn a_create_answers_an_id_the_next_read_answers_the_new_items_fields_for() {
    in_memory("contract-create", create_then_show);
}

#[test]
fn a_read_of_an_item_nobody_filed_is_missing_and_not_unreadable() {
    in_memory("contract-absent", show_of_an_absent_item);
}

#[test]
fn an_assign_moves_the_assignee_the_next_read_answers() {
    in_memory("contract-assign", assign);
}

#[test]
fn a_note_is_readable_whole_and_a_second_note_keeps_the_first() {
    in_memory("contract-note", note);
}

#[test]
fn a_set_title_moves_the_title_the_next_read_answers() {
    in_memory("contract-title", set_title);
}

#[test]
fn set_orders_writes_the_four_fields_and_unset_orders_takes_the_key_away() {
    in_memory("contract-orders", orders);
}

#[test]
fn a_metadata_write_merges_at_the_top_level_and_replaces_one_keys_object_whole() {
    in_memory("contract-metadata", metadata_merge);
}

#[test]
fn a_gate_is_on_the_open_listing_until_it_is_resolved() {
    in_memory("contract-gate", gates);
}

#[test]
fn a_close_moves_the_status_the_next_read_answers() {
    in_memory("contract-close", close);
}

#[test]
fn an_export_writes_the_file_and_its_bytes_move_when_an_item_does() {
    in_memory("contract-export", export);
}

/// THE OTHER HALF: every check above, against the store `bd` answers.
///
/// One arm and one board for all of them, because a nextest arm is its own
/// process and each board costs a `bd init`. A check that reds here and greens
/// in its own arm above is the in-memory board having drifted from the real
/// store, which is the whole reason this file exists.
#[test]
fn every_check_holds_against_bd_too() {
    let scratch = shared_store("contract");
    let bd = Bd::at(&scratch.root);
    for (name, check) in CHECKS {
        check(&bd, &scratch.root, &format!("bd — {name}"));
    }
}

/// The table the real half walks names every check this file holds, so a check
/// added above and left out of it is a red here rather than a silence.
#[test]
fn the_table_names_every_check() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract.rs");
    let arms = std::fs::read_to_string(&here).expect("this file is readable");
    let written = arms
        .lines()
        .filter(|line| line.trim_start().starts_with("in_memory("))
        .count();
    assert_eq!(
        CHECKS.len(),
        written,
        "every check with an arm of its own is on the table the real half walks"
    );
}
