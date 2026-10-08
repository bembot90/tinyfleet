//! The agent contract's conformance suite ([`conformance`]), which
//! `fleet agent check` runs against an adapter. The contract itself — the
//! trait, the executable that answers it and its opener — is
//! [`fleet_core::agent`]'s; the suite stays in this crate because its live
//! steps start a session on the host ([`crate::host`]) and wait by the effect
//! layer's own bounds.

use std::collections::BTreeMap;
use std::path::PathBuf;

pub mod conformance;

// TRANSITIONAL (e2-e1, fleet-3llk.6): deleted by e2-e2 (fleet-3llk.7), which
// points every caller at `fleet_core::agent`.
pub use fleet_core::agent::{
    contexts, exec, named, open, readings, waiting_on, Activity, AdapterSource, Agent, AgentError,
    AgentExec, Argv, BlockedOn, Capabilities, Evidence, Launch, Opened, Opening, PackDirs,
    Permissions, Posture, Refusal, RefusalReason, Resume, SeatActivity, SeatContext, SeatRef,
    Setting, Unanswered, Version, DEFAULT_AGENT_ADAPTER,
};

// ---- what fleet sets for a seat's session -----------------------------------

/// The variable the plugin's shim runs its binary from, which every seat's
/// session is handed. Spelled here and not taken from the item layer's own
/// constant, because this crate names nothing of the project around it.
pub const FLEET_BIN_VAR: &str = "FLEET_BIN";

/// The variable a started session's verbs read their actor from, set to the
/// seat's own `seat:<id>`.
pub const FLEET_ACTOR_VAR: &str = "FLEET_ACTOR";

/// This process's own executable, as the absolute path the shim requires.
///
/// Whatever the operating system answers, and nothing when it answers nothing
/// or something relative: the shim refuses a relative seam, so handing one over
/// would block every Bash command of the session it reached rather than let the
/// shim look under its own root.
pub fn own_executable() -> Option<PathBuf> {
    std::env::current_exe().ok().filter(|exe| exe.is_absolute())
}

/// The variables fleet sets for a seat's session, whatever agent runs in it
/// (D1, D7): the constructed `PATH`, the four a shell needs
/// ([`crate::platform::PASSED_THROUGH`]), this process's own
/// executable as `FLEET_BIN`, and WHO THE SESSION ACTS AS — `actor`, so its own
/// bare verbs are the seat's.
///
/// NOTHING IS INHERITED (lessons claude-code D1). A service-launched process
/// carries a minimal `PATH`, and a session that inherits it hands the collapsed
/// search path to every tool call it makes, long after the start that caused
/// it; so the `PATH` is the platform's, built off the home, and the process's
/// own contributes nothing. What an agent's adapter adds for its own agent it
/// answers in its [`Argv`]'s environment, and core sets both, exactly.
pub fn seat_environment(actor: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".to_string(),
        crate::platform::child_path(&crate::platform::home_dir()),
    );
    for pass in crate::platform::PASSED_THROUGH {
        if let Ok(value) = std::env::var(pass) {
            env.insert(pass.to_string(), value);
        }
    }
    if let Some(bin) = own_executable() {
        env.insert(FLEET_BIN_VAR.to_string(), bin.display().to_string());
    }
    env.insert(FLEET_ACTOR_VAR.to_string(), actor.to_string());
    env
}

/// The environment a session's pane runs with: fleet's own for the seat, with
/// the adapter's answered variables over it — the pairs the host is handed,
/// and nothing of its own.
pub fn pane_environment(fleet: &BTreeMap<String, String>, argv: &Argv) -> Vec<(String, String)> {
    let mut env = fleet.clone();
    for (key, value) in &argv.env {
        env.insert(key.clone(), value.clone());
    }
    env.into_iter().collect()
}

/// A directory path with any trailing separator removed — the one form both
/// sides of a comparison are put in, so a configured path and a reported one
/// that differ only there are the same directory. The root is left alone,
/// because trimming it away leaves nothing to compare.
pub fn dir_key(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        path
    } else {
        trimmed
    }
}
