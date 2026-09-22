//! The seat verbs' record half: what the end of a seat owes the work graph.
//!
//! Nothing here reads the process table. A seat's session, its worktree and its
//! row on the seat list are the controller's; these are the acts of the same
//! verbs that are only items. The cli is the one crate holding both, and it is
//! where the two halves are wired into one verb.

pub mod retire;
