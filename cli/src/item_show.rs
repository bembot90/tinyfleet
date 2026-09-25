//! `fleet item show <id> [--json]` — one item and its timeline, read — and
//! `fleet item list`, the items the store's set reads answer.
//!
//! The SDK's reads of the store: a workflow asks these verbs and never `bd`,
//! so the store stays the adapter's. Neither writes anything.
//!
//! The rendering and the document are core's (`fleet_core::item::show`); this
//! module resolves the project, opens its store, reads the item and its
//! timeline once, and prints one of the two. A missing item is the refused
//! row, 1, and a store that will not answer — a malformed entry on the
//! timeline among them — is could-not-tell, 3.
//!
//! THE LIST IS TRANSITIONAL (`fleet_core::item::list`): its three filters are
//! the store's three set reads today, at least one named and several
//! intersected, until fleet-0q4.5 gives the contract a `list(filter)`. A list
//! naming none is usage, 2, said before the store is opened.

use fleet_core::item::list::{self, Filter};
use fleet_core::item::{show, Stop};
use fleet_core::store::{Item, Store};

use crate::envelope;
use crate::exit::Exit;
use crate::item::{open_store, resolve_at, stop_exit};

/// The names the envelope and the refusal line carry.
const VERB: &str = "item show";
const LIST: &str = "item list";

#[derive(clap::Subcommand)]
pub enum Verb {
    /// print one item and its timeline
    Show {
        /// the item, by its id or a part of it
        id: String,
        /// print the item and its timeline as one JSON document
        #[arg(long)]
        json: bool,
    },
    /// list the items the store's set reads answer
    #[command(long_about = "\
list the items the store's set reads answer, one line each: its id, title,
status, type, labels and assignee. Name at least one read; several are
intersected, in the first one's order.

THE TRANSITIONAL LIST: the store reads no list of every item yet, so the
filters are the three sets it does read, and each item they answer is then
read whole. The contract's own list replaces them.

--json prints {\"items\": [...]}, each row item show's fields without the
timeline, plus the run's record where the item carries one. It writes
nothing. Exit 2 when no read is named, 3 when the store does not answer or
an item carries a run record this fleet does not read.")]
    List {
        /// the store's ready set: open and unblocked
        #[arg(long)]
        ready: bool,
        /// the open items carrying this label
        #[arg(long, value_name = "LABEL")]
        label: Option<String>,
        /// the items held against this assignee
        #[arg(long, value_name = "SEAT")]
        assignee: Option<String>,
        /// print the items as one JSON document
        #[arg(long)]
        json: bool,
    },
}

pub fn command(verb: &Verb) -> Exit {
    match verb {
        Verb::Show { id, json } => match read(id) {
            Ok((text, document)) => {
                if *json {
                    println!("{}", envelope::ok(VERB, &document));
                } else {
                    println!("{text}");
                }
                Exit::Done
            }
            Err(stop) => refused(VERB, &stop, *json),
        },
        Verb::List {
            ready,
            label,
            assignee,
            json,
        } => {
            let filter = Filter {
                ready: *ready,
                label: label.clone(),
                assignee: assignee.clone(),
            };
            match listed(&filter) {
                Ok(items) => {
                    if *json {
                        println!("{}", envelope::ok(LIST, &list::document(&items)));
                    } else {
                        println!("{}", list::render(&items));
                    }
                    Exit::Done
                }
                Err(stop) => refused(LIST, &stop, *json),
            }
        }
    }
}

fn refused(verb: &str, stop: &Stop, json: bool) -> Exit {
    let exit = stop_exit(stop.code);
    eprintln!("fleet {verb}: {}", stop.message);
    if json {
        println!("{}", envelope::refusal(verb, exit, &stop.message));
    }
    exit
}

/// The filter checked before anything is resolved — a list naming no read is
/// the call's own fault wherever it is typed — then the items, each read whole.
fn listed(filter: &Filter) -> Result<Vec<Item>, Stop> {
    filter.named()?;
    let here = resolve_at(None)?;
    let store = open_store(&here.project.root);
    list::list(&store, filter)
}

/// The item resolved from what was typed — the store resolves a part of an id,
/// and the timeline is read under the full one it answered — then both shapes.
fn read(id: &str) -> Result<(String, serde_json::Value), Stop> {
    let here = resolve_at(None)?;
    let store = open_store(&here.project.root);
    let item = store.show(id)?;
    let timeline = store.timeline(&item.id)?;
    Ok((
        show::render(&item, &timeline),
        show::document(&item, &timeline),
    ))
}
