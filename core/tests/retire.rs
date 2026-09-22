//! The record's half of `fleet seat retire`: the orders a retiring seat still
//! holds, withdrawn before its name goes back on the pile.
//!
//! The applying fake store and nothing else. What is under test is which items
//! the query names and what the three writes leave on them — no session, no
//! worktree and no seat list, all three of which are the controller's and are
//! asserted in its own suite.

use fleet_core::item::COULD_NOT_TELL;
use fleet_core::seat::retire::{self, WITHDRAWN};
use fleet_core::store::{Item, Orders, Row, Store};
use fleet_core::test_support::FakeStore;

/// The name the incident wore: a transient seat retired while an item it was
/// given stayed open, and handed out again to the next spawn.
const SEAT: &str = "transient-3";
const BY: &str = "an-architect";

const HELD: &str = "fx-held";
const UNORDERED: &str = "fx-unordered";
const CLOSED: &str = "fx-closed";
const ANOTHER: &str = "fx-another-seat";

fn item(id: &str, status: &str, assignee: &str, ordered: bool) -> Item {
    Item {
        id: id.to_string(),
        title: format!("{id} · an item"),
        status: status.to_string(),
        assignee: Some(assignee.to_string()),
        orders: ordered.then(|| Orders {
            by: Some(String::from("an-architect")),
            kind: Some(String::from("dispatch")),
            seat: Some(SEAT.to_string()),
            at: Some(String::from("2026-09-14T10:40:39Z")),
            ordinal: None,
        }),
        has_orders_key: ordered,
        ..Item::default()
    }
}

/// One board holding the four items the query has to tell apart: the ordered
/// one this seat holds, an unordered one it also holds, a closed one it holds
/// under an order, and another seat's ordered one.
fn board() -> FakeStore {
    let store = FakeStore::default();
    store.seed(item(HELD, "open", SEAT, true));
    store.seed(item(UNORDERED, "open", SEAT, false));
    store.seed(item(CLOSED, "closed", SEAT, true));
    store.seed(item(ANOTHER, "open", "transient-9", true));
    store
}

fn read(store: &FakeStore, id: &str) -> Item {
    store.show(id).expect("the store answers about the item")
}

#[test]
fn a_retire_withdraws_every_open_ordered_item_the_seat_still_holds() {
    let store = board();

    let held = retire::held(&store, SEAT).expect("the board answers");
    assert_eq!(held, vec![HELD.to_string()], "the query names one item");

    retire::withdraw(&store, &held, SEAT, BY).expect("the withdrawal lands");

    let after = read(&store, HELD);
    assert!(
        !after.has_orders_key,
        "no orders key survives the withdrawal: {}",
        after.document
    );
    assert!(
        after.assignee.as_deref().unwrap_or("").trim().is_empty(),
        "the item reads unassigned: {:?}",
        after.assignee
    );
    assert_eq!(after.status, "open", "the item stays open");
    let notes = after.notes.unwrap_or_default();
    assert_eq!(
        notes,
        format!("{WITHDRAWN}: {SEAT} retired by {BY}; the item stays open, unassigned"),
        "one note, naming the seat and who retired it"
    );
}

/// Read beside the arm above: the three items the query must NOT name are
/// exactly the three shapes a wider query would sweep up — an item held under
/// no order, a closed one, and another seat's.
#[test]
fn a_retire_leaves_what_the_seat_does_not_hold_under_an_open_order() {
    let store = board();

    let held = retire::held(&store, SEAT).expect("the board answers");
    retire::withdraw(&store, &held, SEAT, BY).expect("the withdrawal lands");

    for untouched in [UNORDERED, CLOSED, ANOTHER] {
        let after = read(&store, untouched);
        assert_eq!(
            after.assignee.as_deref(),
            Some(if untouched == ANOTHER {
                "transient-9"
            } else {
                SEAT
            }),
            "{untouched} keeps its assignee"
        );
        assert!(
            after.notes.is_none(),
            "{untouched} carries no withdrawal note: {:?}",
            after.notes
        );
    }
    assert!(
        read(&store, CLOSED).has_orders_key && read(&store, ANOTHER).has_orders_key,
        "the two ordered items this seat does not hold open keep their order"
    );
}

#[test]
fn a_retire_of_a_seat_holding_nothing_ordered_writes_nothing() {
    let store = board();

    let held = retire::held(&store, "transient-4").expect("the board answers");
    assert!(held.is_empty(), "the seat holds nothing: {held:?}");
    retire::withdraw(&store, &held, "transient-4", BY).expect("nothing to withdraw");

    assert!(
        store.wrote().is_empty(),
        "a seat holding nothing ordered retires exactly as it did before: {:?}",
        store.wrote()
    );
}

/// WHAT THE RETIRE COSTS THE BOARD, counted rather than described: every retire
/// in this fleet pays it, and the arms that drive the shipped binary queue
/// behind each one of these calls.
///
/// The reading is the write log plus the reads the query cannot make: `held`
/// answers off ONE listing, so the only calls a withdrawal of one item makes
/// are the combined update and the note — the read-back is `show`, which this
/// log does not carry and which happens once per item written.
#[test]
fn a_retire_withdrawing_one_item_makes_one_update_and_one_note() {
    let store = board();

    let held = retire::held(&store, SEAT).expect("the board answers");
    retire::withdraw(&store, &held, SEAT, BY).expect("the withdrawal lands");

    let wrote = store.wrote();
    assert_eq!(
        wrote.len(),
        2,
        "two writes and no more for one withdrawn item: {wrote:?}"
    );
    assert!(
        wrote[0].starts_with(&format!("withdraw_order {HELD}")),
        "the assignee and the index move together: {wrote:?}"
    );
    assert!(
        wrote[1].starts_with(&format!("note {HELD}")),
        "then the one note: {wrote:?}"
    );
}

/// The second half of the acceptance: a write the store will not apply stops
/// the retire, so the CALLER never reaches the act that frees the name.
///
/// The disagreement is the one a real store will not produce on demand — the
/// writes are recorded and thrown away — and it is what the read-back after
/// each item exists to catch.
#[test]
fn a_retire_whose_withdrawal_does_not_land_refuses_and_names_the_item() {
    let store = board();
    let held = retire::held(&store, SEAT).expect("the board answers");
    store.ignore_writes();

    let stop = retire::withdraw(&store, &held, SEAT, BY).expect_err("the read-back disagrees");

    assert_eq!(
        stop.code, COULD_NOT_TELL,
        "a store that did not apply the write is a question: {}",
        stop.message
    );
    assert!(
        stop.message.contains(HELD) && stop.message.contains("orders key"),
        "the stop names the item and what is still on it: {}",
        stop.message
    );
    let after = read(&store, HELD);
    assert!(
        after.has_orders_key && after.assignee.as_deref() == Some(SEAT),
        "the item is as it was, so the name must not be freed: {}",
        after.document
    );
}

/// A board nobody could read is a QUESTION and never an empty hold: a retire
/// that took silence for "this seat holds nothing" would free the name with the
/// order still standing, which is the whole defect.
#[test]
fn a_retire_that_cannot_read_the_board_refuses_rather_than_reading_no_hold() {
    let store = FakeStore {
        unreadable: Some(String::from("`bd` could not be run")),
        held: [(
            SEAT.to_string(),
            vec![Row {
                id: HELD.to_string(),
                status: String::from("open"),
                has_orders_key: true,
            }],
        )]
        .into_iter()
        .collect(),
        ..FakeStore::default()
    };

    let stop = retire::held(&store, SEAT).expect_err("an unreadable board is no reading");

    assert_eq!(stop.code, COULD_NOT_TELL, "{}", stop.message);
    assert!(
        stop.message.contains("could not be run"),
        "the store's own cause reaches the caller: {}",
        stop.message
    );
}
