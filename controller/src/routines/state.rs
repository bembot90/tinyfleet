//! `orders/state.json` under the machine directory: what each routine last did.
//!
//! It is a record and never a belief. Nothing here says a routine is running;
//! the two stamps say when it was last asked and when it last fired, and the
//! streak counts the terminal outcomes that were not a success.
//!
//! A file that is missing or will not parse reads as an EMPTY one and says so.
//! That makes every routine due rather than silently never-due, which is the
//! direction a duty's owner can see.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bumped by any breaking change to the shape below.
pub const SCHEMA: u32 = 1;

/// The directory the routines' own files sit in, under the machine directory.
pub const ORDERS_DIR: &str = "orders";
/// Where a check's and an exec's output goes.
pub const LOGS_DIR: &str = "logs";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutineState {
    /// UTC stamps, in the one shape this fleet writes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_evaluated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_fired: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_outcome: Option<String>,
    /// Consecutive terminal outcomes in failed, absent and could-not-tell. A
    /// success of any kind puts it back to 0.
    #[serde(default)]
    pub failing_streak: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct State {
    pub schema: u32,
    /// Keyed `orders` in the file: the state's own format keeps the word the
    /// routine directory and the `[order]` table keep.
    #[serde(default, rename = "orders")]
    pub routines: BTreeMap<String, RoutineState>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            routines: BTreeMap::new(),
        }
    }
}

impl State {
    pub fn entry(&self, name: &str) -> RoutineState {
        self.routines.get(name).cloned().unwrap_or_default()
    }

    pub fn put(&mut self, name: &str, entry: RoutineState) {
        self.routines.insert(name.to_string(), entry);
    }
}

pub fn dir_in(machine_dir: &Path) -> PathBuf {
    machine_dir.join(ORDERS_DIR)
}

pub fn path_in(machine_dir: &Path) -> PathBuf {
    dir_in(machine_dir).join("state.json")
}

pub fn logs_dir_in(machine_dir: &Path) -> PathBuf {
    dir_in(machine_dir).join(LOGS_DIR)
}

pub fn lock_path_in(machine_dir: &Path, name: &str) -> PathBuf {
    dir_in(machine_dir).join(format!("{name}.lock"))
}

/// The state, and the reason it stands empty when it does.
pub fn read(machine_dir: &Path) -> (State, Option<String>) {
    let path = path_in(machine_dir);
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (State::default(), None),
        Err(e) => return (State::default(), Some(format!("{}: {e}", path.display()))),
    };
    match serde_json::from_str::<State>(&body) {
        Ok(state) if state.schema == SCHEMA => (state, None),
        Ok(state) => (
            State::default(),
            Some(format!(
                "{} is schema {}, which this build does not read",
                path.display(),
                state.schema
            )),
        ),
        Err(e) => (State::default(), Some(format!("{}: {e}", path.display()))),
    }
}

pub fn write(machine_dir: &Path, state: &State) -> std::io::Result<()> {
    let mut body = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    body.push('\n');
    crate::platform::write_atomic(&path_in(machine_dir), body.as_bytes())
}

/// Whether a lock file holds a live process, and which. `Held` is a run this
/// tick keeps its hands off; `Stale` is one that died holding it, which the
/// next taker says out loud and then takes over.
pub enum Lock {
    Free,
    Held(u32),
    Stale(u32),
}

pub fn read_lock(machine_dir: &Path, name: &str) -> Lock {
    let Ok(body) = std::fs::read_to_string(lock_path_in(machine_dir, name)) else {
        return Lock::Free;
    };
    let Ok(pid) = body.trim().parse::<u32>() else {
        return Lock::Free;
    };
    // A platform that cannot tell is read as HELD: the cost of waiting a tick
    // is one skipped firing, and the cost of the other answer is two processes
    // running one duty at once.
    match crate::platform::process_alive(pid) {
        Some(false) => Lock::Stale(pid),
        _ => Lock::Held(pid),
    }
}

pub fn take_lock(machine_dir: &Path, name: &str) -> std::io::Result<()> {
    let path = lock_path_in(machine_dir, name);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{}\n", std::process::id()))
}

pub fn release_lock(machine_dir: &Path, name: &str) {
    let _ = std::fs::remove_file(lock_path_in(machine_dir, name));
}
