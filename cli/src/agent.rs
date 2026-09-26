//! `fleet agent schema`.
//!
//! One family module and one arm in the dispatch, shaped as `fleet store` is:
//! the agent adapter's family, as that is the store adapter's (ruling 18,
//! 2026-09-25). `schema` prints the agent contract's JSON Schema document
//! ([`schema::text`]), which the core suite holds byte for byte to the
//! committed `core/src/agent/agent.schema.json`. It reads no project and runs
//! no adapter.

use fleet_core::agent::schema;

use crate::exit::Exit;

/// The family's verbs.
#[derive(clap::Subcommand)]
pub enum Verb {
    /// print the agent contract as a JSON Schema document
    #[command(long_about = "\
print the agent contract (docs/agent.md) as one JSON Schema document: each
verb's request and response, the refusal and the error, generated from the
types this fleet reads and writes. Exits 0.")]
    Schema,
}

pub fn command(verb: &Verb) -> Exit {
    match verb {
        Verb::Schema => {
            print!("{}", schema::text());
            Exit::Done
        }
    }
}
