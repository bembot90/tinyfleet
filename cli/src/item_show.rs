//! `fleet item show <id> [--json]` — one item and its timeline, read.
//!
//! The SDK's one read of the store: a workflow asks this verb and never `bd`,
//! so the store stays the adapter's. It writes nothing.
//!
//! The rendering and the document are core's (`fleet_core::item::show`); this
//! module resolves the project, opens its store, reads the item and its
//! timeline once, and prints one of the two. A missing item is the refused
//! row, 1, and a store that will not answer — a malformed entry on the
//! timeline among them — is could-not-tell, 3.

use fleet_core::item::{show, Stop};
use fleet_core::store::Store;

use crate::envelope;
use crate::exit::Exit;
use crate::item::{open_store, resolve_at, stop_exit};

/// The name the envelope and the refusal line carry.
const VERB: &str = "item show";

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
            Err(stop) => {
                let exit = stop_exit(stop.code);
                eprintln!("fleet {VERB}: {}", stop.message);
                if *json {
                    println!("{}", envelope::refusal(VERB, exit, &stop.message));
                }
                exit
            }
        },
    }
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
