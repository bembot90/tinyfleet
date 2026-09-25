//! The work graph, reached only through [`Store`], whose implementations are
//! adapters: each speaks one store's own tongue behind the trait, and nothing
//! outside an adapter's own module knows which store is answering.
//!
//! The trait is what the verbs are written against, so a suite can force a
//! read-back that disagrees with the write beside it — the one failure a real
//! store will not produce on demand and the one the verbs must survive.
//!
//! What an adapter keeps beside the graph it declares through
//! [`Store::capabilities`]: the export a landing commits and the directory it
//! sits in are read from there, and never spelled by a verb.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::entry::{Body, Entry};
use crate::seat::actor::Actor;
use types::Capabilities;

pub mod bd;
pub mod keys;
pub mod types;

pub use types::{ItemId, Order, OrderKind, OrderState, ReadProof, RunRecord, Stamp, Status};

/// One item as a read answers it, in the contract's own types field by field:
/// [`types::Item`]'s fields and the description beside them.
///
/// NOT YET [`types::Item`] ITSELF. The assignee is still the text the store
/// holds, because a board a project brought may name a person there, and
/// what a holder that is not a seat id reads as is not ruled yet; the
/// description is `item show`'s, which the contract's item does not carry.
///
/// An assignee is `Option` because the store OMITS a key it has no value for:
/// an item nobody has assigned carries no `assignee` at all, and an absent
/// field is a third answer that must not read as a disagreement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub id: ItemId,
    /// What a person calls this item. `land` writes it into the commit subject,
    /// so a trunk's log reads as a list of what was done and not of ids.
    pub title: String,
    /// What the item says, as the store keeps it: `""` where it says nothing,
    /// which the store spells by omitting the key. `item show` renders it.
    pub description: String,
    pub status: Status,
    pub assignee: Option<String>,
    /// The order index: none, one the store holds and this fleet cannot read,
    /// or the order — one value, so an order that is there and unread cannot
    /// be built as absent.
    pub order: OrderState,
    /// The open items that block this one by a type the store's ready set
    /// honours.
    pub blockers: Vec<ItemId>,
    /// The type, as the store spells it: a rule matches on this value.
    pub item_type: String,
    /// The item's OWN labels and no parent's, which is what the store answers.
    pub labels: Vec<String>,
    /// A run's record, on the item that records it. One the store holds at a
    /// shape this fleet does not read is no item at all: the read refuses.
    pub run: Option<RunRecord>,
    /// The whole text the read answered. The negative control asks it, so the
    /// control asks the SAME answer for a token nothing wrote.
    pub proof: ReadProof,
}

/// A new item, as the arguments a `create` takes.
///
/// There is no id here: the store names what it files, which is why [`Store`]'s
/// `create` answers one.
pub struct NewItem<'a> {
    pub title: &'a str,
    pub description: &'a str,
    /// The store's own spelling — `task` for a run's record.
    pub item_type: &'a str,
    pub labels: &'a [&'a str],
}

/// One item assigned to a seat, as the seat's list answers it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssignedItem {
    pub id: String,
    /// What a person calls this item, read off this row — the words a
    /// session's start carries beside the id.
    pub title: String,
    pub status: Status,
    /// The order index, read off THIS ROW and not off a second call: the
    /// listing answers each row's metadata, so a caller asking which of a
    /// seat's items are ordered pays one call and not one per row. The same
    /// three answers [`Item::order`] carries.
    pub order: OrderState,
    /// The type, as the listing spells it (`issue_type`), read off this row.
    pub item_type: String,
}

/// The ways a store call ends badly, which are different exits: an item that
/// is not there, or not held by whom the write required, is the record's
/// answer, and a store that will not answer is no reading at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store answered, and its answer is that there is no such item.
    Missing(String),
    /// The store answered, and its answer is that the item is held by someone
    /// other than the holder a fenced write named — so NOTHING was written.
    Moved(String),
    /// The store could not be run, or did not answer something readable.
    Unreadable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Missing(why) => write!(f, "{why}"),
            StoreError::Moved(why) => write!(f, "{why}"),
            StoreError::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

pub trait Store {
    /// The items the store calls ready, by id: open and unblocked, in the
    /// store's own order.
    fn ready(&self) -> Result<Vec<String>, StoreError>;

    /// One item, which the argument may name by PART of its id: the store
    /// resolves a partial id itself, and the answer's `id` is the full one. So
    /// a verb taking an item resolves it here once, at its entry, and acts on
    /// [`Item::id`] from then on and never on the typed text.
    fn show(&self, item: &str) -> Result<Item, StoreError>;

    /// The open items carrying this label, by id.
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError>;

    /// One item filed, answered as the id the store gave it.
    ///
    /// A run's record is titled by its own id, which nothing knows until
    /// this returns — so the title in [`NewItem`] is what the record carries
    /// until the caller retitles it, and the caller's read-back is what says
    /// the second write landed.
    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError>;

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError>;

    /// Every item the store holds against this seat.
    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError>;

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError>;

    /// The item handed from `from` to `to`, and only while `from` still holds
    /// it — `""` for an item nobody holds. Anyone else holding it is
    /// [`StoreError::Moved`], and nothing is written.
    ///
    /// For a write whose actor is NOT the holder. The DEFAULT reads the holder
    /// and then assigns, for a store with no fence of its own.
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        let held = self.show(item)?.assignee.unwrap_or_default();
        if held != from {
            return Err(moved(item, from, &held));
        }
        self.assign(item, to, by)
    }

    /// `metadata["fleet.orders"]`, written as one object that replaces the key
    /// whole.
    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError>;

    /// One metadata object written by the same call `set_orders` makes, for a
    /// top-level key that is not `fleet.orders`.
    ///
    /// It is the SIBLING of that method and not a generalisation of it: the
    /// write MERGES at the top level, so a run's object never erases the order
    /// index beside it, and the two keys keep one writer each.
    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError>;

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError>;

    /// The item's status set back to `open`, which is all this writes.
    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError>;

    /// The item reopened, the assignee cleared and the order index unset in
    /// ONE call, and only while `seat` still holds the item under `status`:
    /// [`hand_over`](Store::hand_over)'s fence, because a retire's actor is
    /// never the seat it retires, and the status beside it, because an item
    /// closed since the caller read it is never reopened.
    ///
    /// `status` is the one the caller read the item under, and a withdrawal
    /// reads only `open` or `in_progress`. The reopen is the point of the
    /// second: an item left `in_progress` with nobody holding it is out of the
    /// ready set, and no dispatch reaches it until somebody reopens it by hand.
    ///
    /// The three are what a withdrawal always writes together, and a retire
    /// pays them on every seat it ends — so a store that takes all three in one
    /// call is sent one rather than three, which is a call the verb does not
    /// make while another suite is queueing behind it. The DEFAULT is the writes in
    /// order behind one read of the fence, the reopen FIRST: a default cut
    /// short after it leaves an item still held and ordered, which a second
    /// retire lists and finishes. What no form may do is leave the assignee
    /// cleared with the index still set, which the caller's read-back catches.
    fn withdraw_order(
        &self,
        item: &str,
        seat: &str,
        status: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        let read = self.show(item)?;
        let held = read.assignee.unwrap_or_default();
        if held != seat {
            return Err(moved(item, seat, &held));
        }
        if read.status != status {
            return Err(restatused(item, status, read.status.as_str()));
        }
        self.reopen(item, by)?;
        self.hand_over(item, seat, "", by)?;
        self.unset_orders(item, by)
    }

    /// A hold raised on this item, answered as the hold's own id.
    ///
    /// The store's own object and not a question item this fleet owns: the held
    /// item leaves the ready set the moment the hold is raised, and it comes back
    /// when somebody clears the hold. So a park needs nothing of fleet's beside
    /// the event.
    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError>;

    /// Every hold the store still calls open, by id.
    ///
    /// IDS AND NOT DOCUMENTS, and no item on them: a listing need not say which
    /// item a hold blocks, so a caller that wants one item's hold reads that
    /// hold's id off the item's own park and asks this list whether it is still
    /// here.
    fn open_holds(&self) -> Result<Vec<String>, StoreError>;

    /// One hold cleared, which puts the item it blocked back in the ready set.
    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError>;

    /// The item closed, with the reason a reader gets instead of the act.
    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError>;

    /// One entry appended to the item's timeline, answered as the entry's id.
    ///
    /// The body is VALIDATED FIRST, and one that breaks its kind's rules is
    /// Unreadable with nothing written: a store never keeps an entry its own
    /// reader would refuse.
    fn append(&self, item: &str, body: &Body, by: &Actor) -> Result<String, StoreError>;

    /// The item's entries, in the store's order. A comment that does not carry
    /// [`crate::entry::KEY`] is a person's and is left out; one that carries it and
    /// does not read — or whose author is not a typed actor — is Unreadable
    /// and refuses the whole read, naming the comment, because a timeline with
    /// a hole in it answers every question wrong. An item the store does not
    /// hold is Missing.
    fn timeline(&self, item: &str) -> Result<Vec<Entry>, StoreError>;

    /// What the store keeps beside the graph, answered without a call to the
    /// store: the export a landing commits and the directory it sits in —
    /// `None` for a store that keeps none, which a landing lands without —
    /// whether it is a scratch board, and the prefix its ids carry.
    fn capabilities(&self) -> Result<Capabilities, StoreError>;

    /// The store's own export, written under the root the CALLER names at the
    /// file [`capabilities`](Store::capabilities) declares, and answered as
    /// the absolute path it wrote. A store that declares no export refuses it.
    ///
    /// THE DESTINATION IS THE CALLER'S AND NOT THE STORE'S. One board is read
    /// from the checkout it was resolved in and committed from whichever tree
    /// the act runs in, and a landing resolves that tree for itself — so where
    /// the export has to land is a fact of the act and not of the store.
    ///
    /// It regenerates that one file and touches nothing else, so nothing is
    /// carried forward here to keep the export's polarity right.
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError>;
}

/// The bound on one store call, fixed and not policy.
///
/// No legitimate store call comes close: every one is a local read or write
/// of one project's store. A call that outruns it is a store that is not
/// answering, and it is killed and read as Unreadable rather than left to hang
/// the verb. A landing's suite is not a store call and is not bounded here.
pub const STORE_TIMEOUT: Duration = Duration::from_secs(60);

/// The refusal a fenced hand-over answers when the item's holder is not the
/// one the write named.
pub(crate) fn moved(item: &str, expected: &str, held: &str) -> StoreError {
    StoreError::Moved(format!(
        "{item} is held by {} and not by {} — nothing was written",
        holder_named(held),
        holder_named(expected)
    ))
}

/// The refusal a fenced withdrawal answers when the item's status is not the
/// one the write named.
pub(crate) fn restatused(item: &str, expected: &str, now: &str) -> StoreError {
    StoreError::Moved(format!(
        "{item} reads {now} and not {expected} — nothing was written"
    ))
}

/// The body an append is handed, held to its kind's rules before anything is
/// written — the one refusal both stores answer, word for word.
pub(crate) fn validated(item: &str, body: &Body) -> Result<(), StoreError> {
    body.validate().map_err(|why| {
        StoreError::Unreadable(format!(
            "the {} entry for {item} does not validate: {why} — nothing was written",
            body.kind()
        ))
    })
}

/// A holder as a refusal names one: the seat, or nobody for `""`.
fn holder_named(seat: &str) -> String {
    if seat.is_empty() {
        String::from("nobody")
    } else {
        format!("`{seat}`")
    }
}

/// The first JSON value of an answer, with whatever trails it discarded.
///
/// Beside the trait and not inside an adapter: the contract's own envelope
/// ([`types::answer`]) reads an answer through it as an adapter's reads do.
pub fn first_value(text: &str) -> Option<serde_json::Value> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>();
    stream.next().and_then(Result::ok)
}
