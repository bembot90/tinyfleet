//! `fleet item list`'s read and its document, over the applying fake store.
//!
//! The store's three set reads are the filters, so what this suite proves is
//! which ids each one answers, what two of them answer together, and that a
//! row carries `item show`'s fields plus the run's object and the metadata as
//! the store holds it — the one field the defaults' `adopt-board` check reads a
//! board's own conventions from. The shipped binary and a real board are the
//! cli's `adopt.rs`.

use fleet_core::item::list::{document, ids, list, render, Filter};
use fleet_core::item::{COULD_NOT_TELL, USAGE};
use fleet_core::store::Item;
use fleet_core::test_support::FakeStore;

fn item(id: &str, status: &str, labels: &[&str]) -> Item {
    Item {
        id: id.to_string(),
        title: format!("the item {id}"),
        status: status.to_string(),
        item_type: String::from("task"),
        labels: labels.iter().map(|label| label.to_string()).collect(),
        ..Item::default()
    }
}

/// Five items: a plain one, one carrying a board's own `orders` beside a
/// `fleet.orders` at a version this binary does not read, a run's record, a
/// `fleet.run` at a version it does not read, and one in progress held
/// against a seat — out of the ready set and in the assignee's read.
fn a_store() -> FakeStore {
    let store = FakeStore::default();
    store.seed(item("fx-1", "open", &[]));
    store.seed(item("fx-2", "open", &["backend"]));
    store.seed(item("fx-3", "open", &["fleet:run"]));
    store.seed(item("fx-4", "open", &[]));
    let mut held = item("fx-5", "in_progress", &[]);
    held.assignee = Some(String::from("s1"));
    store.seed(held);

    let mut metadata = store.metadata.lock().expect("the metadata is not poisoned");
    metadata.insert(
        String::from("fx-2"),
        serde_json::json!({
            "orders": { "owner": "alice", "kind": "build" },
            "fleet.orders": { "v": 2, "seat": "s9" },
        })
        .as_object()
        .cloned()
        .expect("an object"),
    );
    metadata.insert(
        String::from("fx-3"),
        serde_json::json!({ "fleet.run": { "v": 1, "hash": "h1" } })
            .as_object()
            .cloned()
            .expect("an object"),
    );
    metadata.insert(
        String::from("fx-4"),
        serde_json::json!({ "fleet.run": { "v": 9 } })
            .as_object()
            .cloned()
            .expect("an object"),
    );
    drop(metadata);
    store
}

fn filter(ready: bool, label: Option<&str>, assignee: Option<&str>) -> Filter {
    Filter {
        ready,
        label: label.map(str::to_string),
        assignee: assignee.map(str::to_string),
    }
}

#[test]
fn a_list_naming_no_read_is_usage_and_reads_nothing() {
    let mut store = a_store();
    // A store that would refuse any read: the usage answer comes first.
    store.unreadable = Some(String::from("not asked"));
    let stop = ids(&store, &Filter::default()).expect_err("no filter is refused");
    assert_eq!(stop.code, USAGE, "{}", stop.message);
    assert!(stop.message.contains("--ready"), "{}", stop.message);
}

#[test]
fn each_filter_answers_its_own_read() {
    let store = a_store();
    assert_eq!(
        ids(&store, &filter(true, None, None)).expect("the ready set"),
        ["fx-1", "fx-2", "fx-3", "fx-4"],
        "open and unblocked; the item in progress is not ready"
    );
    assert_eq!(
        ids(&store, &filter(false, Some("fleet:run"), None)).expect("the label"),
        ["fx-3"]
    );
    assert_eq!(
        ids(&store, &filter(false, None, Some("s1"))).expect("the assignee"),
        ["fx-5"]
    );
}

/// Two reads named: the ids both answer. The control is each read alone, which
/// answers more than the pair.
#[test]
fn two_filters_answer_what_both_reads_answer() {
    let store = a_store();
    assert_eq!(
        ids(&store, &filter(true, Some("backend"), None)).expect("ready and labelled"),
        ["fx-2"]
    );
    assert!(ids(&store, &filter(true, None, Some("s1")))
        .expect("ready and held")
        .is_empty());
}

#[test]
fn a_store_that_does_not_answer_is_could_not_tell() {
    let mut store = a_store();
    store.unreadable = Some(String::from("the store is down"));
    let stop = list(&store, &filter(true, None, None)).expect_err("refused");
    assert_eq!(stop.code, COULD_NOT_TELL, "{}", stop.message);
    assert!(
        stop.message.contains("the store is down"),
        "{}",
        stop.message
    );
}

/// The rows: `item show`'s fields, the run's object read at its version, and
/// the metadata whole — fleet's two keys and the board's own beside them.
#[test]
fn a_row_carries_the_fields_the_run_and_the_metadata() {
    let store = a_store();
    let items = list(&store, &filter(true, None, None)).expect("the ready set");
    let document = document(&items);
    let rows = document["items"].as_array().expect("items is an array");
    assert_eq!(rows.len(), 4, "{document}");

    for row in rows {
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("a row is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["assignee", "id", "labels", "metadata", "order", "run", "status", "title", "type"],
            "{row}"
        );
    }
    let by_id = |id: &str| {
        rows.iter()
            .find(|row| row["id"] == id)
            .unwrap_or_else(|| panic!("{id} is a row: {document}"))
    };

    let plain = by_id("fx-1");
    assert_eq!(plain["order"], serde_json::Value::Null);
    assert_eq!(plain["run"], serde_json::Value::Null);
    assert_eq!(plain["metadata"], serde_json::Value::Null);
    assert_eq!(plain["type"], "task");
    assert_eq!(plain["status"], "open");

    let foreign = by_id("fx-2");
    assert_eq!(foreign["order"], serde_json::json!({ "unreadable": true }));
    assert_eq!(foreign["metadata"]["orders"]["owner"], "alice");
    assert_eq!(foreign["metadata"]["fleet.orders"]["v"], 2);
    assert_eq!(foreign["labels"], serde_json::json!(["backend"]));

    let record = by_id("fx-3");
    assert_eq!(record["run"], serde_json::json!({ "v": 1, "hash": "h1" }));
    assert_eq!(
        by_id("fx-4")["run"],
        serde_json::json!({ "unreadable": true })
    );

    let rendered = render(&items);
    assert_eq!(rendered.lines().count(), 4, "{rendered}");
    assert!(
        rendered.lines().any(|line| line
            == "fx-3 · the item fx-3  [open]  type task · labels fleet:run · assignee none"),
        "{rendered}"
    );
    assert_eq!(render(&[]), "(no items)");
}
