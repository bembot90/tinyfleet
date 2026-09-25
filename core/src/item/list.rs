//! `fleet item list` — the items one or more of the store's set reads answer,
//! each as `item show` reads its fields.
//!
//! THE TRANSITIONAL LIST. The store's contract carries no read of every item
//! today: what it answers as a set is the ready set, the open items under one
//! label and the items held against one assignee, each as ids. So this verb is
//! those three reads as filters — at least one named, several intersected in
//! the first one's order — and each id answered is then read whole through
//! [`Store::show`]. fleet-0q4.5 replaces the three with the contract's
//! `list(filter)`, which answers rows in one call; until then a list costs one
//! store call per item it answers, and an item none of the three reads reaches
//! (closed, in progress with nobody named, blocked and unlabelled) is not in
//! it.
//!
//! A ROW IS `item show`'s FIELDS, not its timeline. The order index reads as
//! [`show::document`] reads it, and the run's record as the store answered it
//! — the record, or `null` where the item carries none. A record this fleet
//! does not read refuses the read, and so the list, naming the item: it is
//! never a row. The row carries what the item carries and no raw metadata: a
//! key the store holds that fleet does not read is the store's, and it is not
//! handed on.
//!
//! It writes nothing.
//!
//! [`show::document`]: crate::item::show::document

use serde_json::Value;

use crate::item::{show, Stop};
use crate::store::{Item, Store, StoreError};

/// What a list is asked for: each field one of the store's set reads, and a
/// list naming none of them is not a call this verb can answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The store's ready set: open, unblocked.
    pub ready: bool,
    /// The open items carrying this label.
    pub label: Option<String>,
    /// The items held against this assignee, as the store holds it.
    pub assignee: Option<String>,
}

impl Filter {
    /// The filter as a call: usage where it names no read, which a caller asks
    /// before it opens anything, because the fault is the call's wherever it is
    /// typed.
    pub fn named(&self) -> Result<(), Stop> {
        if !self.ready && self.label.is_none() && self.assignee.is_none() {
            return Err(Stop::usage(
                "name what to list: --ready, --label <LABEL> or --assignee <SEAT> — the \
                 store's contract reads no list of every item yet",
            ));
        }
        Ok(())
    }
}

/// The ids every named read answers, in the first read's order, each once.
///
/// A read that refuses refuses the list: a list missing one read's answer is
/// an intersection nobody asked for, and it reads exactly like a whole one.
pub fn ids(store: &dyn Store, filter: &Filter) -> Result<Vec<String>, Stop> {
    filter.named()?;
    let unread = |read: &str, e: StoreError| {
        Stop::could_not_tell(format!("the store's {read} could not be read: {e}"))
    };
    let mut reads: Vec<Vec<String>> = Vec::new();
    if filter.ready {
        reads.push(store.ready().map_err(|e| unread("ready set", e))?);
    }
    if let Some(label) = &filter.label {
        reads.push(
            store
                .open_labelled(label)
                .map_err(|e| unread(&format!("open items labelled {label}"), e))?,
        );
    }
    if let Some(assignee) = &filter.assignee {
        reads.push(
            store
                .assigned_to(assignee)
                .map_err(|e| unread(&format!("items held against {assignee}"), e))?
                .into_iter()
                .map(|row| row.id)
                .collect(),
        );
    }
    let mut reads = reads.into_iter();
    let first = reads.next().unwrap_or_default();
    let rest: Vec<Vec<String>> = reads.collect();
    let mut kept: Vec<String> = Vec::new();
    for id in first {
        if !kept.contains(&id) && rest.iter().all(|read| read.contains(&id)) {
            kept.push(id);
        }
    }
    Ok(kept)
}

/// The items the filter answers, each read whole.
///
/// AN ID THE SET READ ANSWERED AND `show` THEN DID NOT is could-not-tell and
/// not a row dropped: the two reads disagree, and a list that quietly left the
/// item out would answer as though it had never been there.
pub fn list(store: &dyn Store, filter: &Filter) -> Result<Vec<Item>, Stop> {
    ids(store, filter)?
        .into_iter()
        .map(|id| {
            store.show(&id).map_err(|e| {
                Stop::could_not_tell(format!("{id} was listed and could not then be read: {e}"))
            })
        })
        .collect()
}

/// The list as a caller parses it: `{"items": [<row>, …]}`.
pub fn document(items: &[Item]) -> Value {
    serde_json::json!({ "items": items.iter().map(row).collect::<Vec<_>>() })
}

/// One item's row.
pub fn row(item: &Item) -> Value {
    serde_json::json!({
        "id": item.id,
        "title": item.title,
        "status": item.status,
        "type": item.item_type,
        "labels": item.labels,
        "assignee": item.assignee,
        "order": show::order_json(&item.order),
        "run": item.run,
    })
}

/// The list as a person reads it: one line per item, or a line saying there
/// is none. No trailing newline.
pub fn render(items: &[Item]) -> String {
    if items.is_empty() {
        return String::from("(no items)");
    }
    items
        .iter()
        .map(|item| {
            let labels = if item.labels.is_empty() {
                String::from("none")
            } else {
                item.labels.join(", ")
            };
            format!(
                "{} · {}  [{}]  type {} · labels {} · assignee {}",
                item.id,
                item.title,
                item.status,
                item.item_type,
                labels,
                item.assignee.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
