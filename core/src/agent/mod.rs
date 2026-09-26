//! The agent contract: what fleet asks the program a seat runs, and what that
//! program's adapter answers, as fleet's own types.
//!
//! A seat is fleet's worker and the agent the program it runs. fleet drives
//! the agent's session itself — the host starts it, types into it, reads its
//! screen and ends it — and an adapter never touches the host. What only the
//! agent's own adapter can say is what this contract asks, in six verbs:
//! `capabilities` and `version`, what the agent is; `launch` and `resume`, the
//! argv and environment a session starts or comes back under; `read`, what
//! each seat's agent is doing; and `context`, how full its window is, asked
//! only of an agent that declares it.
//!
//! THE CALL IS THE STORE'S. An agent adapter is spoken to through the same
//! [`crate::adapter::exec`] the store's is — the verb, the envelope with the
//! same `root`, the exit table, the bound and its group kill — so this module
//! states only what is the agent contract's own: its version, its verbs'
//! request and answer types, and its refusal's reasons.
//!
//! THE TYPES LIVE HERE AND THE TRAIT DOES NOT (reviewer call 2026-09-25, E10).
//! The trait fleet drives an agent through, and the suite that holds an
//! adapter to this contract, are the controller's; the JSON is core's, so the
//! schema `fleet agent schema` prints and the page `docs/agent.md` shows are
//! generated from and held to one set of types, whichever crate calls them.

pub mod schema;
pub mod types;
