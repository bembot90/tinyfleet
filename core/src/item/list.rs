//! `fleet item list` — the items one or more of the store's listings answer,
//! each as `item show` reads its fields.
//!
//! THE FILTERS ARE THE CONTRACT'S. The store answers a listing by
//! [`store::Filter`] — the ready set, the open items under one label and the
//! items held against one seat — each as rows in one call, and this verb is
//! those three as filters: at least one named, several intersected in the
//! first one's order. The store's contract carries no listing of every item,
//! so an item none of the three reaches (closed, in progress with nobody
//! named, blocked and unlabelled) is not in it.
//!
//! A ROW IS `item show`'s FIELDS, not its timeline. The id, title, status,
//! type, labels and order are the listing's row, the order as
//! [`show::document`] reads it. The assignee and the run's record are not on a
//! listing's row, so each row is read through [`Store::show`] for those two —
//! one store call per row answered, beside the listings. A record this fleet
//! does not read refuses that read, and so the list, naming the item: it is
//! never a row. The row carries what the item carries and no raw metadata: a
//! key the store holds that fleet does not read is the store's, and it is not
//! handed on.
//!
//! It writes nothing.
//!
//! [`show::document`]: crate::item::show::document

use serde_json::Value;

use crate::item::{show, Stop};
use crate::seat::identity::SeatId;
use crate::store::{self, ItemSummary, RunRecord, Store, StoreError};

/// What a list is asked for: each field one of the store's listings, and a
/// list naming none of them is not a call this verb can answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The store's ready set: open, unblocked.
    pub ready: bool,
    /// The open items carrying this label.
    pub label: Option<String>,
    /// The items held against this seat, by its full id — the text a caller
    /// typed, which is read as a seat id before anything is opened.
    pub assignee: Option<String>,
}

impl Filter {
    /// The filter as a call: usage where it names no listing, or names a seat
    /// by something that is not a seat's full id, which a caller asks before
    /// it opens anything, because the fault is the call's wherever it is
    /// typed.
    pub fn named(&self) -> Result<(), Stop> {
        if !self.ready && self.label.is_none() && self.assignee.is_none() {
            return Err(Stop::usage(
                "name what to list: --ready, --label <LABEL> or --assignee <SEAT> — the \
                 store's contract reads no list of every item",
            ));
        }
        self.seat().map(|_| ())
    }

    /// The seat `--assignee` names. A store holds a seat's items under its
    /// full id and the listing is asked by that id, so an assignee that is no
    /// seat id — a name, eight hex digits, a person a board's own history
    /// assigned — is usage and never a listing of nothing.
    fn seat(&self) -> Result<Option<SeatId>, Stop> {
        self.assignee
            .as_deref()
            .map(|assignee| {
                SeatId::parse(assignee).map_err(|why| {
                    Stop::usage(format!(
                        "--assignee takes a seat's full id, which the store holds its items \
                         under: {why}"
                    ))
                })
            })
            .transpose()
    }

    /// The contract's filters this list names, in the order a list reads them.
    fn filters(&self) -> Result<Vec<store::Filter>, Stop> {
        let mut filters = Vec::new();
        if self.ready {
            filters.push(store::Filter::Ready);
        }
        if let Some(label) = &self.label {
            filters.push(store::Filter::Label(label.clone()));
        }
        if let Some(seat) = self.seat()? {
            filters.push(store::Filter::Assignee(seat));
        }
        Ok(filters)
    }
}

/// The listing's rows every named filter answers, in the first listing's
/// order, each once.
///
/// A listing that refuses refuses the list: a list missing one listing's
/// answer is an intersection nobody asked for, and it reads exactly like a
/// whole one.
pub fn summaries(store: &dyn Store, filter: &Filter) -> Result<Vec<ItemSummary>, Stop> {
    filter.named()?;
    let mut reads: Vec<Vec<ItemSummary>> = Vec::new();
    for asked in filter.filters()? {
        reads.push(store.list(&asked).map_err(|e| unread(&asked, e))?);
    }
    let mut reads = reads.into_iter();
    let first = reads.next().unwrap_or_default();
    let rest: Vec<Vec<ItemSummary>> = reads.collect();
    let mut kept: Vec<ItemSummary> = Vec::new();
    for row in first {
        let everywhere = rest
            .iter()
            .all(|read| read.iter().any(|other| other.id == row.id));
        if everywhere && !kept.iter().any(|held| held.id == row.id) {
            kept.push(row);
        }
    }
    Ok(kept)
}

/// A listing the store did not answer, named as the list names it.
fn unread(asked: &store::Filter, e: StoreError) -> Stop {
    let read = match asked {
        store::Filter::Ready => String::from("ready set"),
        store::Filter::Label(label) => format!("open items labelled {label}"),
        store::Filter::Assignee(seat) => format!("items held against {seat}"),
    };
    Stop::could_not_tell(format!("the store's {read} could not be read: {e}"))
}

/// The ids every named listing answers, in the first listing's order.
pub fn ids(store: &dyn Store, filter: &Filter) -> Result<Vec<String>, Stop> {
    Ok(summaries(store, filter)?
        .into_iter()
        .map(|row| row.id.to_string())
        .collect())
}

/// One item as the list answers it: the listing's row, and the two fields a
/// listing's row does not carry, read off the item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub summary: ItemSummary,
    pub assignee: Option<String>,
    pub run: Option<RunRecord>,
}

/// The items the filter answers: the listings' rows, each with its assignee
/// and its run's record read through `show`.
///
/// AN ID A LISTING ANSWERED AND `show` THEN DID NOT is could-not-tell and not
/// a row dropped: the two reads disagree, and a list that quietly left the
/// item out would answer as though it had never been there.
pub fn list(store: &dyn Store, filter: &Filter) -> Result<Vec<Row>, Stop> {
    summaries(store, filter)?
        .into_iter()
        .map(|summary| {
            let item = store.show(&summary.id).map_err(|e| {
                Stop::could_not_tell(format!(
                    "{} was listed and could not then be read: {e}",
                    summary.id
                ))
            })?;
            Ok(Row {
                summary,
                assignee: item.assignee,
                run: item.run,
            })
        })
        .collect()
}

/// The list as a caller parses it: `{"items": [<row>, …]}`.
pub fn document(rows: &[Row]) -> Value {
    serde_json::json!({ "items": rows.iter().map(row).collect::<Vec<_>>() })
}

/// One item's row.
pub fn row(row: &Row) -> Value {
    let summary = &row.summary;
    serde_json::json!({
        "id": summary.id,
        "title": summary.title,
        "status": summary.status,
        "type": summary.item_type,
        "labels": summary.labels,
        "assignee": row.assignee,
        "order": show::order_json(&summary.order),
        "run": row.run,
    })
}

/// The list as a person reads it: one line per item, or a line saying there
/// is none. No trailing newline.
pub fn render(rows: &[Row]) -> String {
    if rows.is_empty() {
        return String::from("(no items)");
    }
    rows.iter()
        .map(|row| {
            let summary = &row.summary;
            let labels = if summary.labels.is_empty() {
                String::from("none")
            } else {
                summary.labels.join(", ")
            };
            format!(
                "{} · {}  [{}]  type {} · labels {} · assignee {}",
                summary.id,
                summary.title,
                summary.status,
                summary.item_type,
                labels,
                row.assignee.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
