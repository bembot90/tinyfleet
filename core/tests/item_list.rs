//! `fleet item list`'s read and its document, over the applying fake store.
//!
//! The store's three listings are the filters, so what this suite proves is
//! which ids each one answers, what two of them answer together, and that a
//! row carries `item show`'s fields plus the run's record — the typed fields
//! the item carries, and no raw metadata — and the names of the store's keys
//! that are not fleet's. The shipped binary and a real board are the cli's
//! `adopt.rs`.

use fleet_core::item::list::{document, ids, list, render, Filter};
use fleet_core::item::{COULD_NOT_TELL, USAGE};
use fleet_core::seat::identity::SeatId;
use fleet_core::store::{Item, Status};
use fleet_core::test_support::FakeStore;

fn item(id: &str, status: &str, labels: &[&str]) -> Item {
    Item {
        id: id.into(),
        title: format!("the item {id}"),
        status: Status::from(status),
        item_type: String::from("task"),
        labels: labels.iter().map(|label| label.to_string()).collect(),
        ..Item::default()
    }
}

/// Five items: a plain one, one carrying a board's own `orders` beside a
/// `fleet.orders` at a version this binary does not read, a run's record, one
/// carrying a readable order index, and one in progress held against a seat —
/// out of the ready set and in the assignee's read.
fn a_store() -> FakeStore {
    let store = FakeStore::default();
    store.seed(item("fx-1", "open", &[]));
    store.seed(item("fx-2", "open", &["backend"]));
    store.seed(item("fx-3", "open", &["fleet:run"]));
    store.seed(item("fx-4", "open", &[]));
    let mut held = item("fx-5", "in_progress", &[]);
    held.assignee = Some(SeatId::parse(HOLDER).expect("the holder's id parses"));
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
    metadata.insert(String::from("fx-3"), run_record());
    metadata.insert(
        String::from("fx-4"),
        serde_json::json!({ "fleet.orders": {
            "v": 1, "by": BY, "kind": "dispatch", "seat": SEAT, "at": AT,
        }})
        .as_object()
        .cloned()
        .expect("an object"),
    );
    drop(metadata);
    store
}

/// The seat fx-5 is held against, by its full id.
const HOLDER: &str = "01a0d1f1-0aec-765f-9abe-0000000005e1";

/// Who gave fx-4's order, the seat it names and when.
const BY: &str = "run:lead-1";
const SEAT: &str = "01a0d1f1-0aec-765f-9abe-00000005ea71";
const AT: &str = "2026-09-24T10:00:00Z";

/// A run's record as the run writes it, its version stamped in.
fn run_record() -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({ "fleet.run": {
        "v": 1, "hash": "h1", "workflow": "greet", "pack": "ts", "entry": "greet.ts",
        "started_at": AT,
    }})
    .as_object()
    .cloned()
    .expect("an object")
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

/// A seat is listed by its full id, which is what the store holds its items
/// under: a name, a short id or a person a board assigned is usage, said before
/// any listing is read — never a listing of nothing.
#[test]
fn an_assignee_that_is_no_seat_id_is_usage_and_reads_nothing() {
    let mut store = a_store();
    store.unreadable = Some(String::from("not asked"));
    for assignee in ["s1", "0000005e1", "Alberto Vildosola"] {
        let stop = ids(&store, &filter(true, None, Some(assignee))).expect_err("refused");
        assert_eq!(stop.code, USAGE, "{assignee}: {}", stop.message);
        assert!(
            stop.message.contains("--assignee takes a seat's full id"),
            "{assignee}: {}",
            stop.message
        );
    }
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
        ids(&store, &filter(false, None, Some(HOLDER))).expect("the assignee"),
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
    assert!(ids(&store, &filter(true, None, Some(HOLDER)))
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

/// The rows: `item show`'s fields and the run's record, each as the item
/// carries it, and the board's own keys by name under `foreign` — never what
/// they hold, and never fleet's own two.
#[test]
fn a_row_carries_the_fields_the_run_record_and_the_boards_own_keys_by_name() {
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
            ["assignee", "foreign", "id", "labels", "order", "run", "status", "title", "type"],
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
    assert_eq!(plain["type"], "task");
    assert_eq!(plain["status"], "open");

    let foreign = by_id("fx-2");
    assert_eq!(foreign["order"], serde_json::json!({ "unreadable": true }));
    assert!(
        !foreign.to_string().contains("alice"),
        "the board's own key is not handed on: {foreign}"
    );
    assert_eq!(foreign["labels"], serde_json::json!(["backend"]));
    assert_eq!(
        foreign["foreign"],
        serde_json::json!(["orders"]),
        "the board's own key is named, and fleet's beside it is not: {foreign}"
    );
    for id in ["fx-1", "fx-3", "fx-4"] {
        assert_eq!(
            by_id(id)["foreign"],
            serde_json::json!([]),
            "fleet's own keys are never foreign: {document}"
        );
    }

    let record = by_id("fx-3");
    assert_eq!(
        record["run"],
        serde_json::json!({
            "hash": "h1", "workflow": "greet", "pack": "ts", "entry": "greet.ts",
            "started_at": AT,
        })
    );
    assert_eq!(
        by_id("fx-4")["order"],
        serde_json::json!({ "by": BY, "kind": "dispatch", "seat": SEAT, "at": AT })
    );

    let rendered = render(&items);
    assert_eq!(rendered.lines().count(), 4, "{rendered}");
    assert!(
        rendered.lines().any(|line| line
            == "fx-3 · the item fx-3  [open]  type task · labels fleet:run · assignee none"),
        "{rendered}"
    );
    assert_eq!(render(&[]), "(no items)");

    // The assignee is the listing's own field, off the seat's listing.
    let held = list(&store, &filter(false, None, Some(HOLDER))).expect("the seat's items");
    let held = fleet_core::item::list::document(&held);
    assert_eq!(held["items"][0]["assignee"], HOLDER, "{held}");
    assert_eq!(held["items"][0]["status"], "in_progress", "{held}");
}

/// A `fleet.run` this binary does not read is no row: it refuses the listing
/// it is in, and so the list, naming the item and the version it carries —
/// never a row that reads as though the item carried no run.
#[test]
fn a_run_record_this_fleet_does_not_read_refuses_the_list() {
    let store = a_store();
    store
        .metadata
        .lock()
        .expect("the metadata is not poisoned")
        .insert(
            String::from("fx-3"),
            serde_json::json!({ "fleet.run": { "v": 2, "hash": "h1" } })
                .as_object()
                .cloned()
                .expect("an object"),
        );
    let stop = list(&store, &filter(true, None, None)).expect_err("the read refuses");
    assert_eq!(stop.code, COULD_NOT_TELL, "{}", stop.message);
    assert!(
        stop.message
            .contains("fx-3's run record is not one this fleet reads (fleet.run, v 2)"),
        "{}",
        stop.message
    );
}
