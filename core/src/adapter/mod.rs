//! What every adapter executable is spoken to through, whichever contract it
//! answers.
//!
//! An adapter is a program a pack ships and fleet runs, one process per call:
//! the store's (`docs/store.md`) and the agent's (`docs/agent.md`). The
//! contracts differ in their verbs, their request and answer types, their
//! version and the words a refusal is put in; they share the call itself — the
//! verb as the first argument, the request on stdin inside one envelope, the
//! answer the first JSON value of stdout at the contract's `schema_version`,
//! the exit read through one table, a bound that kills the process's whole
//! group, and the last line the adapter said, carried into whatever the caller
//! refuses with. That shared half is [`exec`], and it lives here so that no
//! contract's copy of it drifts from another's.
//!
//! The half of opening one that is every adapter's is here too ([`open`]):
//! the setting read, a path or a name resolved through the installed packs,
//! and the PATH a pack's adapter runs on.
//!
//! THIS MODULE KNOWS NO CONTRACT. It never depends on `store`, and what it
//! words names the kind its caller passes: [`open::resolve`] words every
//! refusal of an adapter setting once, naming the adapter's kind, and
//! [`exec::run`] answers a typed row of the exit table, which [`exec::row`]
//! words naming the contract its caller passes.

pub mod check;
pub mod exec;
pub mod open;

pub use open::{AdapterSource, PackDirs};
