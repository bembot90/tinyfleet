//! The orders a retiring seat still holds, withdrawn before its name is freed.
//!
//! A TRANSIENT NAME GOES BACK ON THE PILE. The spawn takes the lowest free
//! `transient-N` off the seat list, so a name a retire freed is a name the next
//! spawn takes — and an order left standing against the retired seat is
//! inherited whole by that next one: the board still assigns it the item, the
//! item still carries a `fleet.orders` key, and the new seat's first delivery
//! is refused for an item it never saw. The retire is the act that frees the name,
//! so it is the act that owes the name a clean record.
//!
//! The query is the `ordered` half of [`crate::item::deliver::holds`] — every
//! OPEN item assigned to the seat that carries a `fleet.orders` key, whatever
//! its type — so what is withdrawn here is every order the next seat of that
//! name would inherit.
//!
//! IT IS COUNTED IN STORE CALLS, because every retire pays it and a fleet
//! retires a seat per dispatched item: ONE call to read the board, then one
//! `update` carrying every write, one note and one read-back PER ITEM
//! WITHDRAWN. A seat holding nothing ordered — which is most of them — makes
//! the one read and stops.

use crate::item::deliver::holds;
use crate::item::Stop;
use crate::store::{AssignedItem, Store, StoreError};

/// The line a withdrawal leaves, so an item whose seat was retired reads as one
/// nobody holds rather than as one whose holder vanished.
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
pub fn held(store: &dyn Store, seat: &str) -> Result<Vec<AssignedItem>, Stop> {
    Ok(holds(store, seat)?.ordered)
}

/// Every one of them released: the item reopened, the assignee cleared and the
/// order index unset in one write, one note saying so, and the record read back
/// against all three.
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
/// BEFORE the act that frees the name: a withdrawal that could not be written
/// must not become a name somebody else takes.
pub fn withdraw(
    store: &dyn Store,
    items: &[AssignedItem],
    seat: &str,
    by: &str,
) -> Result<(), Stop> {
    let line = format!("{WITHDRAWN}: {seat} retired by {by}; the item is open and unassigned");
    for row in items {
        let item = row.id.as_str();
        if row.status != "open" && row.status != "in_progress" {
            return Err(nothing_written(
                item,
                &format!(
                    "it was listed {}, and only an open item is withdrawn",
                    row.status
                ),
            ));
        }
        store
            .withdraw_order(item, seat, &row.status, by)
            .map_err(|e| match e {
                StoreError::Moved(why) => moved_on(item, seat, &why),
                other => nothing_written(item, &other.to_string()),
            })?;
        store
            .note(item, &line, by)
            .map_err(|e| halfway(item, &e.to_string()))?;
        let read = store.show(item)?;
        if read.has_orders_key {
            return Err(halfway(item, "it still carries a fleet.orders key"));
        }
        if let Some(assignee) = read
            .assignee
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
        {
            return Err(halfway(item, &format!("it reads assigned to `{assignee}`")));
        }
        if read.status != "open" {
            return Err(halfway(item, &format!("it reads {}", read.status)));
        }
    }
    Ok(())
}

/// An item that moved between the listing and the write — somebody else holds
/// it now, or it was closed: the record's answer, so a refusal and not a
/// could-not-tell. The withdrawal is fenced on the retiring seat — bd 1.3.0
/// takes a retirer's clear of an `in_progress` item only so — and on the
/// listed status, and neither the new holder's claim nor a close is this
/// retire's to take away.
fn moved_on(item: &str, seat: &str, why: &str) -> Stop {
    Stop::refused(format!(
        "the order on {item} was not withdrawn: `{seat}` no longer holds it as it was listed — \
         {why}\n  the retire stops here, before the name is freed; read the item and retire again"
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
/// a half-withdrawn item behind a freed name.
fn halfway(item: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{item} was not fully withdrawn: {why}\n  finish it by hand — the item open, the assignee \
         cleared and the fleet.orders key unset — before the name is given to anybody else"
    ))
}
