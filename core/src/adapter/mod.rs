//! What every adapter executable is spoken to through, whichever contract it
//! answers.
//!
//! An adapter is a program a pack ships and fleet runs, one process per call:
//! the store's today (`docs/store.md`), the agent's next. The contracts differ
//! in their verbs, their request and answer types, their version and the words
//! a refusal is put in; they share the call itself — the verb as the first
//! argument, the request on stdin inside one envelope, the answer the first
//! JSON value of stdout at the contract's `schema_version`, the exit read
//! through one table, a bound that kills the process's whole group, and the
//! last line the adapter said, carried into whatever the caller refuses with.
//! That shared half is [`exec`], and it lives here so that no contract's copy
//! of it drifts from another's.
//!
//! THIS MODULE KNOWS NO CONTRACT. It never depends on `store`, and it never
//! words a refusal: [`exec::run`] answers a typed row of the exit table, and
//! each contract's own caller turns that row into its own sentence, naming its
//! own contract.

pub mod exec;
