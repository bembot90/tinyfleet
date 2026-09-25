//! The record's half of `fleet seat retire`: the orders a retiring seat still
//! holds, withdrawn before its row is dropped.
//!
//! The applying fake store, and for the one arm about the listing's row cap a
//! fake `bd` behind the store that talks to it. What is under test is which
//! items the query names and what the three writes leave on them — no session,
//! no worktree and no seat list, all three of which are the controller's and
//! are asserted in its own suite.

mod common;

use common::capped::{calls, capped_bd, Held};
use common::Fixture;
use fleet_core::entry::{Body, Entry, OrderWithdrawn, Withdrawal};
use fleet_core::item::{COULD_NOT_TELL, REFUSED};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use fleet_core::seat::retire;
use fleet_core::store::{AssignedItem, Bd, Item, Orders, Store};
use fleet_core::test_support::FakeStore;

/// A transient seat's full id, which is what an order assigns to: the incident
/// was one retired while an item it was given stayed ordered to it.
const SEAT: &str = "018f6a2c-1d3e-7a4b-9c5d-00000c3a5e71";
/// That seat's machine name, which is how every sentence names it.
const LABEL: &str = "agent-0c3a5e71";
/// Who retires it, in the typed form the entry and every write carry.
const BY: &str = "seat:018f6a2c-1d3e-7a4b-9c5d-0000a1b2c3d4";
/// A seat the board holds nothing against.
const NOBODY: &str = "018f6a2c-1d3e-7a4b-9c5d-00004a8c1e37";

fn seat() -> SeatId {
    SeatId::parse(SEAT).expect("the seat's id parses")
}

fn by() -> Actor {
    Actor::typed(BY)
        .expect("typed")
        .expect("the retirer is a seat")
}

const HELD: &str = "fx-held";
const UNORDERED: &str = "fx-unordered";
const CLOSED: &str = "fx-closed";
const ANOTHER: &str = "fx-another-seat";
const EPIC: &str = "fx-epic";

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
    store.seed(item(ANOTHER, "open", "agent-9d2b4f60", true));
    store
}

fn read(store: &FakeStore, id: &str) -> Item {
    store.show(id).expect("the store answers about the item")
}

fn timeline(store: &dyn Store, id: &str) -> Vec<Entry> {
    store
        .timeline(id)
        .expect("the store answers the item's timeline")
}

/// The entry a retire leaves on each item it withdraws.
fn withdrawn_at_retire() -> Body {
    Body::OrderWithdrawn(OrderWithdrawn {
        why: Withdrawal::Retire,
        seat: Some(seat()),
        cause: None,
    })
}

/// The ids of the rows `held` answered, in the order it answered them.
fn ids_of(held: &[AssignedItem]) -> Vec<String> {
    held.iter().map(|row| row.id.clone()).collect()
}

#[test]
fn a_retire_withdraws_every_open_ordered_item_the_seat_still_holds() {
    let store = board();

    let held = retire::held(&store, SEAT).expect("the board answers");
    assert_eq!(
        ids_of(&held),
        vec![HELD.to_string()],
        "the query names one item"
    );

    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

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
    assert_eq!(after.notes, None, "and nothing is noted");
    let entries = timeline(&store, HELD);
    assert_eq!(
        entries.iter().map(|entry| &entry.body).collect::<Vec<_>>(),
        vec![&withdrawn_at_retire()],
        "one entry, the withdrawal naming the seat"
    );
    assert_eq!(entries[0].by, by(), "by the retiring actor");
}

/// fleet-3e6: AN ITEM THE SEAT MARKED `in_progress` GOES BACK TO OPEN. Left
/// `in_progress` with nobody holding it, it is out of the ready set, and no
/// dispatch reaches it until somebody reopens it by hand — so the withdrawal
/// reopens it, and it reads open, unassigned, unordered and ready.
#[test]
fn a_retire_reopens_an_item_the_seat_marked_in_progress() {
    let store = board();
    store.seed(item(HELD, "in_progress", SEAT, true));
    assert!(
        !store
            .ready()
            .expect("the store answers")
            .contains(&HELD.to_string()),
        "an in_progress item is not ready, which is the whole defect"
    );

    let held = retire::held(&store, SEAT).expect("the board answers");
    assert_eq!(ids_of(&held), vec![HELD.to_string()], "the query names it");
    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

    let after = read(&store, HELD);
    assert_eq!(after.status, "open", "the claimed item reads open");
    assert!(
        after.assignee.as_deref().unwrap_or("").trim().is_empty() && !after.has_orders_key,
        "and unassigned and unordered: {}",
        after.document
    );
    assert!(
        store
            .ready()
            .expect("the store answers")
            .contains(&HELD.to_string()),
        "and back in the ready set"
    );
}

/// A CLOSED ITEM IS NEVER REOPENED. The listing read the item `in_progress`,
/// and its own seat closed it before the write — a landing keeps the assignee
/// and the order on the item it closes — so the fence on the listed status
/// refuses the withdrawal with nothing written, and the item stays closed.
#[test]
fn a_retire_whose_item_was_closed_after_the_listing_is_refused_and_reopens_nothing() {
    let store = board();
    store.seed(item(HELD, "in_progress", SEAT, true));
    let held = retire::held(&store, SEAT).expect("the board answers");
    store
        .close(HELD, "landed", SEAT)
        .expect("the seat lands its item after the listing");

    let stop =
        retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect_err("the item was closed");

    assert_eq!(
        stop.code, REFUSED,
        "an item closed since the listing is the record's answer: {}",
        stop.message
    );
    assert!(
        stop.message.contains(HELD) && stop.message.contains("closed"),
        "the refusal names the item and what it reads now: {}",
        stop.message
    );
    let after = read(&store, HELD);
    assert_eq!(after.status, "closed", "the item stays closed");
    assert!(
        after.has_orders_key && after.assignee.as_deref() == Some(SEAT),
        "nothing was written: {}",
        after.document
    );
    assert!(
        timeline(&store, HELD).is_empty(),
        "and no withdrawal entry either"
    );
}

/// A ROW THE CALLER HANDS IN CLOSED is refused before any write: the fence
/// would take the status it names, so the one guard against reopening a closed
/// item that no listing produced is the withdrawal's own.
#[test]
fn a_retire_handed_a_closed_row_writes_nothing() {
    let store = board();
    let row = AssignedItem {
        id: CLOSED.to_string(),
        status: String::from("closed"),
        has_orders_key: true,
        ..AssignedItem::default()
    };

    let stop = retire::withdraw(&store, &[row], &seat(), LABEL, &by()).expect_err("a closed row");

    assert!(
        stop.message.contains(CLOSED) && stop.message.contains("listed closed"),
        "{}",
        stop.message
    );
    assert!(store.wrote().is_empty(), "{:?}", store.wrote());
    assert_eq!(read(&store, CLOSED).status, "closed");
}

/// Read beside `a_retire_withdraws_every_open_ordered_item_the_seat_still_holds`:
/// the three items the query must NOT name are exactly the three shapes a
/// wider query would sweep up — an item held under no order, a closed one, and
/// another seat's.
#[test]
fn a_retire_leaves_what_the_seat_does_not_hold_under_an_open_order() {
    let store = board();

    let held = retire::held(&store, SEAT).expect("the board answers");
    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

    for untouched in [UNORDERED, CLOSED, ANOTHER] {
        let after = read(&store, untouched);
        assert_eq!(
            after.assignee.as_deref(),
            Some(if untouched == ANOTHER {
                "agent-9d2b4f60"
            } else {
                SEAT
            }),
            "{untouched} keeps its assignee"
        );
        assert!(
            timeline(&store, untouched).is_empty(),
            "{untouched} carries no withdrawal entry"
        );
    }
    assert!(
        read(&store, CLOSED).has_orders_key && read(&store, ANOTHER).has_orders_key,
        "the two ordered items this seat does not hold open keep their order"
    );
}

/// ANOTHER WRITER'S `orders` IS NOT AN ORDER: an item the retiring seat is
/// assigned that carries a bare `orders` and no `fleet.orders` is neither
/// named nor written, and the withdrawal of an item carrying both unsets
/// fleet's key alone (fleet-4j6 AC2).
#[test]
fn a_retire_leaves_another_writers_orders_key_untouched() {
    const THEIRS: &str = "fx-theirs";
    let store = board();
    store.seed(item(THEIRS, "open", SEAT, false));
    for held in [THEIRS, HELD] {
        store
            .set_metadata(held, common::FOREIGN_ORDERS, "another-tool")
            .expect("the other writer's key lands");
    }
    let before = [
        common::foreign_of(&store, THEIRS),
        common::foreign_of(&store, HELD),
    ];

    let held = retire::held(&store, SEAT).expect("the board answers");
    assert_eq!(
        ids_of(&held),
        vec![HELD.to_string()],
        "the query names fleet's order alone"
    );
    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

    let theirs = read(&store, THEIRS);
    assert_eq!(
        theirs.assignee.as_deref(),
        Some(SEAT),
        "{THEIRS} keeps its assignee"
    );
    assert!(timeline(&store, THEIRS).is_empty(), "and carries no entry");
    assert!(
        !read(&store, HELD).has_orders_key,
        "fleet's own order is withdrawn"
    );
    assert_eq!(
        [
            common::foreign_of(&store, THEIRS),
            common::foreign_of(&store, HELD),
        ],
        before,
        "the other writer's key is byte-identical on both"
    );
}

/// AN EPIC IS NEVER HELD, AND ITS ORDER IS STILL WITHDRAWN: dispatch and
/// deliver do not count an epic as work the seat carries, but an order left
/// on one is still an order standing against a seat that will no longer
/// exist, and nothing would dispatch it again.
#[test]
fn a_retire_withdraws_an_ordered_epic_the_seat_still_names() {
    let store = board();
    store.seed(Item {
        item_type: String::from("epic"),
        ..item(EPIC, "open", SEAT, true)
    });

    let mut held = retire::held(&store, SEAT).expect("the board answers");
    held.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(
        ids_of(&held),
        vec![EPIC.to_string(), HELD.to_string()],
        "the query names the ordered epic beside the ordered task"
    );

    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

    let after = read(&store, EPIC);
    assert!(
        !after.has_orders_key && after.assignee.as_deref().unwrap_or("").trim().is_empty(),
        "the epic reads unordered and unassigned: {}",
        after.document
    );
    assert_eq!(after.status, "open", "the epic stays open");
}

#[test]
fn a_retire_of_a_seat_holding_nothing_ordered_writes_nothing() {
    let store = board();

    let held = retire::held(&store, NOBODY).expect("the board answers");
    assert!(held.is_empty(), "the seat holds nothing: {held:?}");
    let nobody = SeatId::parse(NOBODY).expect("the id parses");
    retire::withdraw(&store, &held, &nobody, "agent-4a8c1e37", &by()).expect("nothing to withdraw");

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
/// are the combined update and the entry's append — the read-backs are the
/// timeline and `show`, which this log does not carry and which happen once per
/// item written.
#[test]
fn a_retire_withdrawing_one_item_makes_one_update_and_one_append() {
    let store = board();

    let held = retire::held(&store, SEAT).expect("the board answers");
    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

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
        wrote[1].starts_with(&format!("append {HELD} order_withdrawn")),
        "then the one entry: {wrote:?}"
    );
}

/// The second half of the acceptance: a write the store will not apply stops
/// the retire, so the CALLER never reaches the act that drops the seat's row.
///
/// The disagreement is the one a real store will not produce on demand — the
/// writes are recorded and thrown away — and it is what the read-back after
/// each item exists to catch.
#[test]
fn a_retire_whose_withdrawal_does_not_land_refuses_and_names_the_item() {
    let store = board();
    let held = retire::held(&store, SEAT).expect("the board answers");
    store.ignore_writes();

    let stop = retire::withdraw(&store, &held, &seat(), LABEL, &by())
        .expect_err("the read-back disagrees");

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
        "the item is as it was, so the seat's row must not be dropped: {}",
        after.document
    );
}

/// fleet-reb: an item another seat took between the listing and the write is
/// the record's answer. The withdrawal is fenced on the retiring seat — bd
/// 1.3.0's `--if-assignee`, which this fake keeps too — so it is REFUSED with
/// nothing written, and the new holder's claim and order stand.
#[test]
fn a_retire_whose_item_moved_to_another_seat_is_refused_and_writes_nothing() {
    let store = board();
    let held = retire::held(&store, SEAT).expect("the board answers");
    store
        .assign(HELD, "agent-9d2b4f60", "the-test")
        .expect("another seat takes the item after the listing");

    let stop =
        retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect_err("the holder moved");

    assert_eq!(
        stop.code, REFUSED,
        "an item somebody else holds is the record's answer: {}",
        stop.message
    );
    assert!(
        stop.message.contains(HELD)
            && stop
                .message
                .contains(&format!("`{LABEL}` no longer holds it"))
            && stop.message.contains("`agent-9d2b4f60`"),
        "the refusal names the item, the retiring seat and the holder now: {}",
        stop.message
    );
    let after = read(&store, HELD);
    assert!(
        after.has_orders_key && after.assignee.as_deref() == Some("agent-9d2b4f60"),
        "nothing was written: {}",
        after.document
    );
    assert!(
        timeline(&store, HELD).is_empty(),
        "and no withdrawal entry either"
    );
}

/// A board nobody could read is a QUESTION and never an empty hold: a retire
/// that took silence for "this seat holds nothing" would drop the row with the
/// order still standing, which is the whole defect.
#[test]
fn a_retire_that_cannot_read_the_board_refuses_rather_than_reading_no_hold() {
    let store = FakeStore {
        unreadable: Some(String::from("`bd` could not be run")),
        held: [(
            SEAT.to_string(),
            vec![AssignedItem {
                id: HELD.to_string(),
                title: String::new(),
                status: String::from("open"),
                has_orders_key: true,
                item_type: String::from("task"),
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

/// PAST FIFTY ITEMS ON ONE SEAT the ordered one is still withdrawn: the seat
/// holds 51 open items, and the only ordered one is the 51st — the row `bd
/// list` drops when nothing lifts its cap of 50.
///
/// Through the store that talks to `bd`, because the cap is the binary's and
/// the applying fake has none: the arm is red for a listing that reads the
/// first page and takes it for the whole.
#[test]
fn a_retire_withdraws_an_ordered_item_past_the_fiftieth_row() {
    let dir = Fixture::new("retire-row-51");
    let log = dir.path("calls");
    let ids: Vec<String> = (1..=51).map(|n| format!("fx-row-{n:02}")).collect();
    let rows: Vec<Held> = ids
        .iter()
        .map(|id| Held {
            id,
            ordered: id == "fx-row-51",
        })
        .collect();
    let bin = capped_bd(&dir, SEAT, &rows, &log);
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);

    let held = retire::held(&store, SEAT).expect("the board answers");
    assert_eq!(
        ids_of(&held),
        vec![String::from("fx-row-51")],
        "the query names the ordered item at row 51"
    );

    retire::withdraw(&store, &held, &seat(), LABEL, &by()).expect("the withdrawal lands");

    let after = store
        .show("fx-row-51")
        .expect("the store answers about the item");
    assert!(
        !after.has_orders_key && after.assignee.is_none() && after.status == "open",
        "the item reads open, unassigned and unordered: {}",
        after.document
    );
    assert_eq!(
        timeline(&store, "fx-row-51")
            .iter()
            .map(|entry| &entry.body)
            .collect::<Vec<_>>(),
        vec![&withdrawn_at_retire()],
        "and carries the withdrawal"
    );
    let withdrawals: Vec<String> = calls(&log)
        .into_iter()
        .filter(|call| call.get(2).map(String::as_str) == Some("update"))
        .map(|call| call[3].clone())
        .collect();
    assert_eq!(
        withdrawals,
        vec![String::from("fx-row-51")],
        "one withdrawal, of the item at row 51"
    );
}
