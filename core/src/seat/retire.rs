//! The orders a retiring seat still holds, withdrawn before its row is dropped.
//!
//! THE WORK IS THE REASON. An order left standing against a retired seat is
//! work nobody holds: the board still assigns the item to a seat that no
//! longer exists, the item still carries its order index, and so nobody
//! will deliver it and nothing will dispatch it again. The retire is the act
//! that ends the seat, so it is the act that releases what the seat held and
//! puts its items back where a dispatch can reach them.
//!
//! The query is the `ordered` half of [`crate::item::deliver::holds`] — every
//! OPEN item assigned to the seat that carries an order index, whatever
//! its type — so what is withdrawn here is every order the retired seat would
//! otherwise strand.
//!
//! IT IS COUNTED IN STORE CALLS, because every retire pays it and a fleet
//! retires a seat per dispatched item: ONE call to read the board, then one
//! `update` carrying every write, one `order_withdrawn` entry with its
//! timeline read-back, and one read-back of the item PER ITEM WITHDRAWN. A
//! seat holding nothing ordered — which is most of them — makes the one read
//! and stops.

use crate::entry::{Body, OrderWithdrawn, Withdrawal};
use crate::item::deliver::holds;
use crate::item::{recorded, Stop, Unrecorded};
use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;
use crate::store::{ItemSummary, OrderState, Status, Store, StoreError};

/// The words a retire says on stderr for each item it withdrew. The record's
/// own half is the `order_withdrawn` entry, so an item whose seat was retired
/// reads as one nobody holds rather than as one whose holder vanished.
pub const WITHDRAWN: &str = "ORDER WITHDRAWN at retire";

/// The open ordered items this seat holds, as the listing's rows, in ONE store
/// call.
///
/// The listing answers each row's own metadata, so whether a row is ordered is
/// read off the row and never off a second call per row: a seat holding nothing
/// — the common case, and the one every retire pays — costs exactly one read.
/// The row's status rides along because the withdrawal is fenced on it.
///
/// An empty list is the answer for a seat that was given nothing, and it is the
/// answer a retire needs before it asks anybody for a name to write under: a
/// seat holding nothing ordered is retired exactly as it was before this
/// existed.
///
/// `seat` is the row's full id, which is what an order assigns to.
pub fn held(store: &dyn Store, seat: &SeatId) -> Result<Vec<ItemSummary>, Stop> {
    Ok(holds(store, seat)?.ordered)
}

/// Every one of them released: the item reopened, the assignee cleared and the
/// order index unset in one write, one `order_withdrawn` entry naming the seat,
/// and the record read back against all three.
///
/// THE ITEM GOES BACK TO OPEN, and so back in the ready set where nothing
/// blocks it: one the seat marked `in_progress` would otherwise sit there with
/// nobody holding it and no dispatch reaching it. Nothing here judges the work
/// — an item nobody finished is still to be done, and what is being taken away
/// is the claim that a seat which no longer exists is the one doing it.
///
/// ONLY AN OPEN OR `in_progress` ROW IS WRITTEN, and the write is fenced on the
/// status the listing read: an item closed between the listing and the write —
/// a landing by its own seat keeps the assignee and the order on it — is
/// refused with nothing written and never reopened.
///
/// Every stop names what it did and did not write, because the caller runs this
/// BEFORE the act that drops the seat's row: a withdrawal that could not be
/// written must not become an order held by a seat that no longer exists.
///
/// `seat` is the row's id, which the write is fenced on because it is the
/// assignee and which the entry names; `label` is how every sentence names the
/// seat; `by` is who retires it: the entry's author, and its string form is
/// what every other write carries.
pub fn withdraw(
    store: &dyn Store,
    items: &[ItemSummary],
    seat: &SeatId,
    label: &str,
    by: &Actor,
) -> Result<(), Stop> {
    let withdrawn = Body::OrderWithdrawn(OrderWithdrawn {
        why: Withdrawal::Retire,
        seat: Some(*seat),
        cause: None,
    });
    for row in items {
        let item = row.id.as_str();
        if !matches!(row.status, Status::Open | Status::InProgress) {
            return Err(nothing_written(
                item,
                &format!(
                    "it was listed {}, and only an open item is withdrawn",
                    row.status
                ),
            ));
        }
        store
            .order_withdraw_from(&row.id, seat, &row.status, by)
            .map_err(|e| match e {
                StoreError::Moved(why) => moved_on(item, label, &why),
                other => nothing_written(item, &other.to_string()),
            })?;
        recorded(store, item, &withdrawn, by).map_err(|unrecorded| match unrecorded {
            Unrecorded::NotWritten(e) => halfway(
                item,
                &format!("the order_withdrawn entry did not land: {e}"),
            ),
            Unrecorded::Unconfirmed(why) => halfway(item, &why),
        })?;
        let read = store.show(item)?;
        if !matches!(read.order, OrderState::None) {
            return Err(halfway(item, "it still carries its order index"));
        }
        if let Some(assignee) = read
            .assignee
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
        {
            return Err(halfway(item, &format!("it reads assigned to `{assignee}`")));
        }
        if read.status != Status::Open {
            return Err(halfway(item, &format!("it reads {}", read.status)));
        }
    }
    Ok(())
}

/// An item that moved between the listing and the write — somebody else holds
/// it now, or it was closed: the record's answer, so a refusal and not a
/// could-not-tell. The withdrawal is fenced on the retiring seat — a store may
/// take a retirer's clear of an `in_progress` item only so — and on the
/// listed status, and neither the new holder's claim nor a close is this
/// retire's to take away.
fn moved_on(item: &str, seat: &str, why: &str) -> Stop {
    Stop::refused(format!(
        "the order on {item} was not withdrawn: `{seat}` no longer holds it as it was listed — \
         {why}\n  the retire stops here, before the seat's row is dropped; read the item and \
         retire again"
    ))
}

/// A write that did not land where nothing of this item has moved yet.
fn nothing_written(item: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "the order on {item} could not be withdrawn: {why}\n  NOTHING was written — the item is \
         as it was and the seat still holds it"
    ))
}

/// A withdrawal that started and did not finish. The item is named with what is
/// still on it, because a caller reading this as "nothing happened" would leave
/// a half-withdrawn item behind a dropped row.
fn halfway(item: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{item} was not fully withdrawn: {why}\n  finish it by hand — the item open, the assignee \
         cleared and the order index unset — before the seat's row is dropped"
    ))
}
