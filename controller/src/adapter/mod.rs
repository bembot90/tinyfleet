//! The agent contract's conformance suite ([`conformance`]), which
//! `fleet agent check` runs against an adapter. The contract itself — the
//! trait, the executable that answers it and its opener — is
//! [`fleet_core::agent`]'s; the suite stays in this crate because its live
//! steps start a session on the host ([`crate::host`]) and wait by the effect
//! layer's own bounds.

pub mod conformance;
