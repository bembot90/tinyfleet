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

use common::{a_delivery, seat_actor, shared_store, A_COMMIT};
use fleet_core::entry::{Body, OrderKind, Ordered};
use fleet_core::item::dispatch;
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use fleet_core::store::bd::Bd;
use fleet_core::store::{
    keys, Filter, ItemId, NewItem, Order, OrderState, RunRecord, Stamp, Store, StoreError, Update,
};
use fleet_core::test_support::Board;

/// Who the writes still taking text are made by.
const BY: &str = "the-contract";

/// Who the typed writes are made by: the same writer, as the actor it is.
fn by() -> Actor {
    Actor::typed("run:the-contract")
        .expect("typed")
        .expect("a run")
}

/// Who an order index here names as its dispatcher, the seat it names — the
/// full id, a string the store carries and never reads — and when.
const ORDERED_BY: &str = "run:an-architect";
const SEAT: &str = "01a0d1f1-0aec-765f-9abe-00000000a5ea";

/// A second seat, which an update hands an item on to.
const ANOTHER_SEAT: &str = "01a0d1f1-0aec-765f-9abe-00000000a5eb";
const AT: &str = "2026-09-13T00:00:00Z";

/// The order the index [`dispatch::index`] writes over those three reads as.
fn the_order() -> Order {
    Order {
        kind: fleet_core::store::OrderKind::Dispatch,
        by: Actor::typed(ORDERED_BY).expect("typed").expect("a run"),
        seat: Some(SeatId::parse(SEAT).expect("a seat id")),
        at: Stamp::parse(AT).expect("a stamp"),
    }
}

/// A run's record, as the run's own writer stamps it, pinned to `hash`.
fn a_record(hash: &str) -> RunRecord {
    RunRecord {
        hash: hash.to_string(),
        workflow: String::from("greet"),
        pack: String::from("ts"),
        entry: String::from("greet.ts"),
        started_at: Stamp::parse(AT).expect("a stamp"),
    }
}

/// One check: the store, the root it writes under — the export is a file and
/// the two stores keep theirs in two places — and the name of the half it is
/// running against, so a red says which store disagreed.
type Check = fn(&dyn Store, &Path, &str);

/// Every check in this file, by name. The real half walks this table, so a
/// check that is added below and left out here is a check `bd` never answers —
/// which [`the_table_names_every_check`] is what refuses.
const CHECKS: &[(&str, Check)] = &[
    ("create then show", create_then_show),
    ("show by hash", show_by_hash),
    ("resolve", resolve),
    ("list", list),
    ("show of an absent item", show_of_an_absent_item),
    ("update's title and assignee", update),
    ("update's cleared assignee", cleared),
    ("update naming nothing", unchanged),
    ("set_orders then unset_orders", orders),
    ("hand_over and withdraw_order's fences", fenced),
    ("set_metadata's merge", metadata_merge),
    ("fleet.orders and fleet.run keep each other", fleet_keys),
    ("hold, open_holds, clear_hold", holds),
    ("close", close),
    ("close of a closed item", close_of_closed),
    ("version", version),
    ("export", export),
    ("append then timeline", append_then_timeline),
    ("timeline of an absent item", timeline_of_an_absent_item),
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
                title: title.to_string(),
                description: String::from("an item the contract suite filed"),
                item_type: String::from("task"),
                labels: vec![String::from("a-label")],
                priority: None,
            },
            &by(),
        )
        .expect("the item is filed")
        .to_string()
}

/// The item handed to `seat`, which is a seat's full id.
fn hand_to(store: &dyn Store, item: &str, seat: &str) {
    store
        .update(
            &ItemId::from(item),
            &Update::assignee(SeatId::parse(seat).expect("a seat id")),
            &by(),
        )
        .expect("the assignment lands");
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
}

/// An item named by its hash alone — the part after the prefix, which is what
/// a person types — reads as the item, and the answer carries the FULL id: it
/// is what every verb acts on once it has resolved its argument.
fn show_by_hash(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item read by its hash");
    let (_, hash) = item
        .split_once('-')
        .expect("the store files under a prefix");

    let read = store.show(hash).expect("the item reads by its hash");
    assert_eq!(read.id, item, "{which}: the answer carries the full id");
    assert_eq!(read.title, "an item read by its hash", "{which}");
}

/// A fragment resolves as `show` resolves it, and the answer is the FULL id
/// and nothing else: the item's hash alone names the item. An id no item
/// carries is Refused, as `show`'s is.
fn resolve(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item resolved by its hash");
    let (_, hash) = item
        .split_once('-')
        .expect("the store files under a prefix");

    let resolved = store.resolve(hash).expect("the hash resolves");
    assert_eq!(resolved, item, "{which}: the answer is the full id");

    match store.resolve("fx-nobody-filed-this") {
        Err(StoreError::Refused(_)) => {}
        other => panic!("{which}: an absent id is Refused, and answered {other:?}"),
    }
}

/// Each filter's listing answers the item it matches as one row, carrying the
/// item's title, status, type, own labels and order — asked by MEMBERSHIP, as
/// every check here asks, because the real half's board holds what the checks
/// before it filed.
fn list(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item a listing answers");
    let row_in = |filter: &Filter| {
        store
            .list(filter)
            .unwrap_or_else(|e| panic!("{which}: the {filter:?} listing reads: {e}"))
            .into_iter()
            .find(|row| row.id == item)
    };

    let ready = row_in(&Filter::Ready)
        .unwrap_or_else(|| panic!("{which}: a fresh item is in the ready set"));
    assert_eq!(ready.title, "an item a listing answers", "{which}");
    assert_eq!(ready.status, "open", "{which}");
    assert_eq!(ready.item_type, "task", "{which}");
    assert_eq!(ready.labels, ["a-label"], "{which}: the row's own labels");
    assert_eq!(ready.order, OrderState::None, "{which}");

    assert!(
        row_in(&Filter::Label(String::from("a-label"))).is_some(),
        "{which}: an open item is in its label's listing"
    );
    let seat = SeatId::parse(SEAT).expect("a seat id");
    assert!(
        row_in(&Filter::Assignee(seat)).is_none(),
        "{which}: an item nobody holds is in no seat's listing"
    );
    hand_to(store, &item, SEAT);
    assert!(
        row_in(&Filter::Assignee(seat)).is_some(),
        "{which}: an item assigned to the seat is in the seat's listing"
    );
}

fn show_of_an_absent_item(store: &dyn Store, _: &Path, which: &str) {
    match store.show("fx-nobody-filed-this") {
        Err(fleet_core::store::StoreError::Refused(_)) => {}
        other => panic!("{which}: an absent item is Refused, and answered {other:?}"),
    }
}

/// The title alone, the assignee alone and both in one update: each moves the
/// field it names, and the read beside it answers the move and leaves the
/// other field as it was.
fn update(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "the title it was filed under");
    let id = ItemId::from(item.as_str());
    let read = |store: &dyn Store| store.show(&item).expect("the item reads");

    store
        .update(
            &id,
            &Update::title(String::from("the title it carries now")),
            &by(),
        )
        .expect("the title lands");
    let now = read(store);
    assert_eq!(now.title, "the title it carries now", "{which}");
    assert_eq!(
        now.assignee, None,
        "{which}: a title alone leaves the assignee absent"
    );

    hand_to(store, &item, SEAT);
    let now = read(store);
    assert_eq!(now.assignee.as_deref(), Some(SEAT), "{which}");
    assert_eq!(
        now.title, "the title it carries now",
        "{which}: an assignee alone leaves the title"
    );

    store
        .update(
            &id,
            &Update {
                title: Some(String::from("the title and the holder both moved")),
                assignee: Some(Some(SeatId::parse(ANOTHER_SEAT).expect("a seat id"))),
            },
            &by(),
        )
        .expect("the one update lands");
    let now = read(store);
    assert_eq!(now.title, "the title and the holder both moved", "{which}");
    assert_eq!(
        now.assignee.as_deref(),
        Some(ANOTHER_SEAT),
        "{which}: the last write is what the read answers"
    );
}

/// An assignee handed to nobody reads ABSENT, as an item nobody ever held
/// does, and never as an empty holder.
fn cleared(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item handed to nobody");
    hand_to(store, &item, SEAT);
    store
        .update(&ItemId::from(item.as_str()), &Update::unassigned(), &by())
        .expect("the clearing lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(read.assignee, None, "{which}: {}", read.proof.as_str());
    assert_eq!(
        read.title, "an item handed to nobody",
        "{which}: and the title stands"
    );
}

/// An update naming neither field is Unreadable, word for word, and nothing
/// moves.
fn unchanged(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item an empty update reaches");
    hand_to(store, &item, SEAT);
    match store.update(&ItemId::from(item.as_str()), &Update::default(), &by()) {
        Err(StoreError::Unreadable(why)) => assert_eq!(
            why, "an update names neither a title nor an assignee — nothing was written",
            "{which}"
        ),
        other => panic!("{which}: an update naming nothing: {other:?}"),
    }
    let read = store.show(&item).expect("the item reads");
    assert_eq!(read.title, "an item an empty update reaches", "{which}");
    assert_eq!(read.assignee.as_deref(), Some(SEAT), "{which}");
}

fn orders(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item with an order on it");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.order,
        OrderState::None,
        "{which}: nothing has written an order yet"
    );

    store
        .set_orders(
            &item,
            &dispatch::index(ORDERED_BY, dispatch::KIND, Some(SEAT), AT),
            BY,
        )
        .expect("the order index lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(read.order, OrderState::Ordered(the_order()), "{which}");

    store.unset_orders(&item, BY).expect("the withdrawal lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.order,
        OrderState::None,
        "{which}: the index is gone — a withdrawal is told from an unreadable value"
    );
}

/// A fenced write lands only while the holder it names still holds the item,
/// and is `Moved` with NOTHING written where somebody else does — bd's
/// `--if-assignee`, which both halves keep. A withdrawal is fenced on the
/// status it names too — bd's `--if-status` — so a closed item is never
/// reopened by one, and the one that lands leaves the item open.
fn fenced(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item a fenced write reaches");
    hand_to(store, &item, SEAT);
    store
        .set_orders(&item, r#"{"fleet.orders":{"v":1,"seat":"a-seat"}}"#, BY)
        .expect("the order index lands");
    let untouched = |store: &dyn Store, status: &str, what: &str| {
        let read = store.show(&item).expect("the item reads");
        assert_eq!(
            read.assignee.as_deref(),
            Some(SEAT),
            "{which}: {what} — nothing was written, the holder stands"
        );
        assert_ne!(
            read.order,
            OrderState::None,
            "{which}: {what} — and so does its order"
        );
        assert_eq!(read.status, status, "{which}: {what} — and its status");
    };

    match store.withdraw_order(&item, "another-seat", "open", BY) {
        Err(StoreError::Moved(why)) => assert!(
            why.contains(&item) && why.contains("another-seat"),
            "{which}: the refusal names the item and the holder it expected: {why}"
        ),
        other => panic!("{which}: a withdraw naming a seat that does not hold it: {other:?}"),
    }
    untouched(store, "open", "another seat named");

    match store.withdraw_order(&item, SEAT, "in_progress", BY) {
        Err(StoreError::Moved(why)) => assert!(
            why.contains(&item) && why.contains("in_progress"),
            "{which}: the refusal names the item and the status it expected: {why}"
        ),
        other => panic!("{which}: a withdraw naming a status the item is not in: {other:?}"),
    }
    untouched(store, "open", "another status named");

    store
        .close(&ItemId::from(item.as_str()), "landed by its seat", SEAT)
        .expect("the holder closes it");
    match store.withdraw_order(&item, SEAT, "open", BY) {
        Err(StoreError::Moved(why)) => assert!(
            why.contains(&item) && why.contains("closed"),
            "{which}: the refusal names the item and the status it reads: {why}"
        ),
        other => panic!("{which}: a withdraw of an item closed since it was read: {other:?}"),
    }
    untouched(store, "closed", "a closed item");

    store.reopen(&item, BY).expect("the reopen lands");
    untouched(store, "open", "a reopen");

    store
        .withdraw_order(&item, SEAT, "open", BY)
        .expect("the holder's withdraw lands");
    let read = store.show(&item).expect("the item reads");
    assert!(
        read.assignee.as_deref().unwrap_or_default().is_empty(),
        "{which}: the assignee is cleared: {:?}",
        read.assignee
    );
    assert_eq!(read.order, OrderState::None, "{which}: and the order unset");
    assert_eq!(read.status, "open", "{which}: and the item open");

    match store.hand_over(&item, SEAT, "the-builder", BY) {
        Err(StoreError::Moved(_)) => {}
        other => panic!("{which}: a hand-over from a seat that no longer holds it: {other:?}"),
    }
    store
        .hand_over(&item, "", "the-builder", BY)
        .expect("a hand-over of an item nobody holds, from nobody, lands");
    store
        .hand_over(&item, "the-builder", "a-reviewer", BY)
        .expect("a hand-over from its holder lands");
    assert_eq!(
        store
            .show(&item)
            .expect("the item reads")
            .assignee
            .as_deref(),
        Some("a-reviewer"),
        "{which}"
    );
}

fn metadata_merge(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item with two metadata keys");
    store
        .set_metadata(&item, r#"{"a_prior_key":{"kept":true}}"#, BY)
        .expect("the first key lands");
    store
        .set_metadata(&item, &a_run_object("fx-one"), BY)
        .expect("the second key lands");

    let read = store.show(&item).expect("the item reads");
    assert!(
        read.proof.carries("a_prior_key") && read.proof.carries("kept"),
        "{which}: the write MERGES at the top level: {}",
        read.proof.as_str()
    );
    assert_eq!(
        read.run,
        Some(a_record("fx-one")),
        "{which}: and the key it wrote is there"
    );

    store
        .set_metadata(
            &item,
            r#"{"fleet.run":{"v":1,"hash":"deadbeef","workflow":"w","pack":"p","entry":"e","started_at":"2026-09-13T00:00:00Z"}}"#,
            BY,
        )
        .expect("a second write of the same key lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.run,
        Some(RunRecord {
            hash: String::from("deadbeef"),
            workflow: String::from("w"),
            pack: String::from("p"),
            entry: String::from("e"),
            started_at: Stamp::parse(AT).expect("a stamp"),
        }),
        "{which}: one key's own object is REPLACED, not merged into"
    );
    assert!(
        read.proof.carries("a_prior_key"),
        "{which}: the top level still merged: {}",
        read.proof.as_str()
    );
}

/// A run's object, as the run's own writer stamps it.
fn a_run_object(hash: &str) -> String {
    let object = serde_json::to_value(a_record(hash))
        .ok()
        .and_then(|value| value.as_object().cloned())
        .expect("a record is an object");
    keys::stamped(keys::RUN, object).to_string()
}

/// FLEET'S TWO KEYS, EACH WITH ITS OWN WRITER, KEEP EACH OTHER (fleet-4j6):
/// the run's object and the order index are dotted top-level keys, so a write
/// of either — and the order's withdrawal — leaves the other standing, and
/// another writer's bare `orders` beside them is neither read nor removed.
fn fleet_keys(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item carrying both of fleet's keys");
    let foreign = r#"{"orders":{"seat":"another-tools-seat","by":7}}"#;
    let foreign_of = |read: &fleet_core::store::Item| {
        let document: serde_json::Value =
            serde_json::from_str(read.proof.as_str()).expect("the document is JSON");
        document["metadata"]["orders"].clone()
    };

    store
        .set_metadata(&item, &a_run_object("h1"), BY)
        .expect("the run's object lands");
    store
        .set_orders(
            &item,
            &dispatch::index(ORDERED_BY, dispatch::KIND, Some(SEAT), AT),
            BY,
        )
        .expect("the order index lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.run,
        Some(a_record("h1")),
        "{which}: the index's write kept the run's object: {}",
        read.proof.as_str()
    );
    assert_eq!(
        read.order,
        OrderState::Ordered(the_order()),
        "{which}: and the index reads at its version"
    );

    store
        .set_metadata(&item, &a_run_object("h2"), BY)
        .expect("the run's object is rewritten");
    store
        .set_metadata(&item, foreign, "another-tool")
        .expect("another writer's key lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.run,
        Some(a_record("h2")),
        "{which}: the run's own write replaced its object"
    );
    assert_eq!(
        read.order,
        OrderState::Ordered(the_order()),
        "{which}: and kept the index, which the bare `orders` beside it is not: {}",
        read.proof.as_str()
    );

    hand_to(store, &item, SEAT);
    store
        .withdraw_order(&item, SEAT, "open", BY)
        .expect("the holder's withdraw lands");
    let read = store.show(&item).expect("the item reads");
    assert_eq!(
        read.order,
        OrderState::None,
        "{which}: the order is withdrawn: {}",
        read.proof.as_str()
    );
    assert_eq!(
        read.run,
        Some(a_record("h2")),
        "{which}: and the run's object stands"
    );
    assert_eq!(
        foreign_of(&read),
        serde_json::json!({ "seat": "another-tools-seat", "by": 7 }),
        "{which}: and so does the other writer's key"
    );
}

fn holds(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item to park");
    let hold = store
        .hold(&item, "the question this park asks", BY)
        .expect("the hold is raised");
    assert!(!hold.is_empty(), "{which}: the store names the hold");
    assert!(
        store
            .open_holds()
            .expect("the open listing answers")
            .contains(&hold),
        "{which}: a raised hold is open"
    );

    store.clear_hold(&hold, BY).expect("the hold clears");
    assert!(
        !store
            .open_holds()
            .expect("the open listing answers")
            .contains(&hold),
        "{which}: and a cleared one is not"
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
        .close(
            &ItemId::from(item.as_str()),
            "closed by the contract suite",
            BY,
        )
        .expect("the close lands");
    assert_eq!(
        store.show(&item).expect("the item reads").status,
        "closed",
        "{which}: the close is what the read answers"
    );
}

/// A close of an item already closed is Refused, naming the item — the act is
/// already done — and the first close's reason is the one the item keeps.
fn close_of_closed(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item closed twice");
    let id = ItemId::from(item.as_str());
    store
        .close(&id, "the first close's reason", BY)
        .expect("the first close lands");
    match store.close(&id, "the second close's reason", BY) {
        Err(StoreError::Refused(why)) => {
            assert_eq!(why, format!("{item} is already closed"), "{which}")
        }
        other => panic!("{which}: a close of a closed item: {other:?}"),
    }
    let read = store.show(&item).expect("the item reads");
    assert_eq!(read.status, "closed", "{which}");
    assert!(
        read.proof.carries("the first close's reason")
            && !read.proof.carries("the second close's reason"),
        "{which}: the first close's reason stands: {}",
        read.proof.as_str()
    );
}

/// The store names itself and the version it is at, neither of them empty.
fn version(store: &dyn Store, _: &Path, which: &str) {
    let answered = store.version().expect("the version reads");
    assert!(!answered.name.is_empty(), "{which}: {answered:?}");
    assert!(!answered.version.is_empty(), "{which}: {answered:?}");
}

/// The export lands at the file the store's own capabilities declare, under
/// the root the caller named, and the store answers that path.
fn export(store: &dyn Store, root: &Path, which: &str) {
    let item = filed(store, "an item the export carries");
    let declared = store
        .capabilities()
        .expect("the capabilities read")
        .export
        .unwrap_or_else(|| panic!("{which}: this store declares an export"));
    let into = root.join(&declared.file);
    assert_eq!(
        store.export(root).expect("the export runs"),
        into,
        "{which}: the store answers the path it wrote, the declared file under the root"
    );

    let before = std::fs::read(&into)
        .unwrap_or_else(|e| panic!("{which}: the export left a file at {}: {e}", into.display()));
    assert!(!before.is_empty(), "{which}: and the file is not empty");
    assert!(
        String::from_utf8_lossy(&before).contains(&item),
        "{which}: carrying the item that was filed"
    );

    store
        .append(
            &item,
            &Body::Ordered(Ordered {
                order: OrderKind::Dispatch,
                seat: None,
            }),
            &seat_actor("the-contract-seat"),
        )
        .expect("the entry the second export has to carry lands");
    assert_eq!(
        store.export(root).expect("the second export runs"),
        into,
        "{which}: and the same path the second time"
    );
    let after = std::fs::read(&into).expect("the export is still there");
    assert_ne!(
        before, after,
        "{which}: the bytes move when an item does, which is what a landing's gate reads"
    );
}

/// An entry appended is on the item's timeline, in the order it was appended,
/// with the body that was written and the actor who wrote it — and an item
/// nothing has appended to answers an empty timeline, not a refusal.
fn append_then_timeline(store: &dyn Store, _: &Path, which: &str) {
    let item = filed(store, "an item with a timeline");
    assert_eq!(
        store.timeline(&item).expect("the timeline reads"),
        Vec::new(),
        "{which}: a filed item's timeline is empty"
    );

    let by = seat_actor("the-contract-seat");
    let ordered = Body::Ordered(Ordered {
        order: OrderKind::Dispatch,
        seat: None,
    });
    let delivered = a_delivery(A_COMMIT);
    let first = store
        .append(&item, &ordered, &by)
        .expect("the order entry lands");
    let second = store
        .append(&item, &delivered, &by)
        .expect("the delivery entry lands");
    assert_ne!(first, second, "{which}: each entry has an id of its own");

    let timeline = store.timeline(&item).expect("the timeline reads");
    let ids: Vec<&str> = timeline.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(
        ids,
        [first.as_str(), second.as_str()],
        "{which}: the two entries, in the order they were appended"
    );
    assert_eq!(timeline[0].body, ordered, "{which}");
    assert_eq!(timeline[1].body, delivered, "{which}");
    for entry in &timeline {
        assert_eq!(entry.by, by, "{which}: the actor who appended it");
        assert!(!entry.at.is_empty(), "{which}: and the store's time");
    }
}

fn timeline_of_an_absent_item(store: &dyn Store, _: &Path, which: &str) {
    match store.timeline("fx-nobody-filed-this") {
        Err(StoreError::Refused(_)) => {}
        other => panic!("{which}: an absent item's timeline is Refused, and answered {other:?}"),
    }
}

// ---- the arms ----------------------------------------------------------------

#[test]
fn a_create_answers_an_id_the_next_read_answers_the_new_items_fields_for() {
    in_memory("contract-create", create_then_show);
}

#[test]
fn a_read_by_hash_answers_the_item_under_its_full_id() {
    in_memory("contract-hash", show_by_hash);
}

#[test]
fn resolve_answers_the_full_id_for_a_fragment() {
    in_memory("contract-resolve", resolve);
}

#[test]
fn a_listing_answers_the_items_row_under_each_filter() {
    in_memory("contract-list", list);
}

#[test]
fn a_read_of_an_item_nobody_filed_is_missing_and_not_unreadable() {
    in_memory("contract-absent", show_of_an_absent_item);
}

#[test]
fn update_title_and_assignee_move_what_show_answers() {
    in_memory("contract-update", update);
}

#[test]
fn an_assignee_cleared_by_update_reads_absent() {
    in_memory("contract-cleared", cleared);
}

#[test]
fn an_update_naming_nothing_is_unreadable_and_moves_nothing() {
    in_memory("contract-unchanged", unchanged);
}

#[test]
fn set_orders_writes_the_four_fields_and_unset_orders_takes_the_key_away() {
    in_memory("contract-orders", orders);
}

#[test]
fn a_fenced_write_lands_only_while_the_holder_it_names_holds_the_item() {
    in_memory("contract-fenced", fenced);
}

#[test]
fn a_metadata_write_merges_at_the_top_level_and_replaces_one_keys_object_whole() {
    in_memory("contract-metadata", metadata_merge);
}

#[test]
fn fleet_orders_and_fleet_run_keep_each_other_through_every_write() {
    in_memory("contract-fleet-keys", fleet_keys);
}

#[test]
fn a_hold_is_on_the_open_listing_until_it_is_cleared() {
    in_memory("contract-hold", holds);
}

#[test]
fn a_close_moves_the_status_the_next_read_answers() {
    in_memory("contract-close", close);
}

#[test]
fn closing_a_closed_item_is_refused() {
    in_memory("contract-close-closed", close_of_closed);
}

#[test]
fn version_names_the_store_and_its_version() {
    in_memory("contract-version", version);
}

#[test]
fn an_export_writes_the_file_and_its_bytes_move_when_an_item_does() {
    in_memory("contract-export", export);
}

#[test]
fn an_entry_appended_is_on_the_timeline_in_the_order_it_was_appended() {
    in_memory("contract-timeline", append_then_timeline);
}

#[test]
fn the_timeline_of_an_item_nobody_filed_is_missing() {
    in_memory("contract-timeline-absent", timeline_of_an_absent_item);
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
    // The file the export check reads is the one bd declares, and bd declares
    // its own.
    let declared = bd
        .capabilities()
        .expect("bd's capabilities read")
        .export
        .expect("bd declares an export");
    assert_eq!(declared.file, ".beads/issues.jsonl");
    assert_eq!(declared.file, fleet_core::store::bd::EXPORT);
    for (name, check) in CHECKS {
        check(&bd, &scratch.root, &format!("bd — {name}"));
    }
}

/// bd names itself `bd`, and its version is the first line `bd --version`
/// prints, trimmed — asked of the binary beside it, so the arm holds on
/// whatever bd this box has.
#[test]
fn bds_version_is_the_first_line_it_prints() {
    let scratch = shared_store("contract");
    let answered = Bd::at(&scratch.root).version().expect("bd's version reads");
    let printed = scratch.bd(&["--version"]);
    assert!(printed.status.success(), "bd --version runs");
    let first = String::from_utf8_lossy(&printed.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    assert!(!first.is_empty(), "bd --version prints a line");
    assert_eq!(answered.name, "bd");
    assert_eq!(answered.version, first);
}

/// The JSON a call to the binary answered, opened out of its envelope where it
/// carries one.
fn answered(out: &std::process::Output, what: &str) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{what}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer = fleet_core::store::first_value(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|| panic!("{what} answers JSON"));
    fleet_core::store::bd::opened(answer, String::new)
}

/// A COMMENT A PERSON WROTE ON THE BOARD IS NOT AN ENTRY, and one carrying
/// fleet's key that does not read is never skipped. Both are planted through
/// the binary, because the author is bd's own field: `--actor` with no
/// `<kind>:` is what a person's own `bd comments add` writes.
#[test]
fn a_persons_comment_is_left_out_and_a_malformed_entry_refuses_the_read() {
    let scratch = shared_store("contract");
    let bd = Bd::at(&scratch.root);
    let item = scratch.item("an item a person commented on");

    answered(
        &scratch.bd(&["comments", "add", &item, "a person's words", "--json"]),
        "bd comments add",
    );
    assert_eq!(
        bd.timeline(&item).expect("the timeline reads"),
        Vec::new(),
        "a person's comment is not an entry"
    );

    let planted = answered(
        &scratch.bd(&[
            "comments",
            "add",
            &item,
            r#"{"fleet.entry":1,"kind":"ordered","order":"dispatch"}"#,
            "--actor",
            "Alberto Vildosola",
            "--json",
        ]),
        "bd comments add",
    );
    let comment = planted["id"].as_str().expect("the comment has an id");
    match bd.timeline(&item) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.contains(comment) && why.contains("Alberto Vildosola"),
            "the refusal names the comment and its author: {why}"
        ),
        other => panic!("an entry whose author is no actor refuses the read: {other:?}"),
    }
}

/// An entry that breaks its kind's rules is refused BEFORE the binary is asked,
/// so nothing is written and the item's comments are what they were.
#[test]
fn an_entry_that_does_not_validate_is_refused_and_nothing_is_written() {
    let scratch = shared_store("contract");
    let bd = Bd::at(&scratch.root);
    let item = scratch.item("an item a short sha is appended to");
    let comments = || answered(&scratch.bd(&["comments", &item, "--json"]), "bd comments");
    let before = comments();

    match bd.append(&item, &a_delivery("1111111"), &seat_actor("a-short-seat")) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.contains(&item) && why.contains("nothing was written"),
            "the refusal names the item and says nothing was written: {why}"
        ),
        other => panic!("a delivery naming a 7-character commit is refused: {other:?}"),
    }
    assert_eq!(comments(), before, "and bd lists nothing new");
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
