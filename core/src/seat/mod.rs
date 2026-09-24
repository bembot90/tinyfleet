//! The seat verbs' record half: what the end of a seat owes the work graph.
//!
//! Nothing here reads the process table. A seat's session, its worktree and its
//! row on the seat list are the controller's; these are the acts of the same
//! verbs that are only items. The cli is the one crate holding both, and it is
//! where the two halves are wired into one verb.
//!
//! `identity` is what a seat IS: its id, kind and name, the fleet.toml table
//! that lists it, the machine's own identity file, and the one resolver every
//! seat argument goes through. `actor` is who acts: a seat, a run, a routine or
//! the controller, as one typed `<kind>:<id>`.

pub mod actor;
pub mod identity;
pub mod retire;
