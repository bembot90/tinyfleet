//! Which dolt engine a rig's scratch board runs on, and which board it takes.
//!
//! `fleet/tools/dolt-test-server` starts ONE server for a `make fleet-test` run
//! and names its port in the environment; every rig that reads this module puts
//! its own board on it. With the variable unset the board falls back to bd's
//! embedded engine, so one binary under a bare `cargo test` still runs.
//!
//! The same wrapper initialises the run's boards once and names the directory
//! holding them, so a rig COPIES one instead of paying a `bd init` of its own.
//!
//! This file is the single definition, included by the cli crate's own test
//! module through `#[path]`: a second copy would let one crate's rigs keep
//! writing to a port the other had learned to refuse.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The variable `fleet/tools/dolt-test-server` exports.
pub const PORT_VAR: &str = "FLEET_TEST_DOLT_PORT";

/// bd's default server port, which on this box holds the fleet's live work
/// graph — every seat's `bd` talks to it. A scratch board is never put there.
const LIVE_PORT: &str = "3307";

static NEXT_DATABASE: AtomicUsize = AtomicUsize::new(0);

/// The directory `fleet/tools/dolt-test-server` fills with the run's boards.
pub const BOARDS_VAR: &str = "FLEET_TEST_BD_BOARDS";

/// The file every `bd init` of a run appends a line to, so the count is read
/// off a counter rather than off a claim.
pub const INIT_LOG_VAR: &str = "FLEET_TEST_BD_INIT_LOG";

/// The rigs whose subject is a WHOLE-BOARD reading — the size of the ready set,
/// the open flights a cap is measured against — so another rig's rows would
/// move the answer they assert on. Each takes a board of its own; every other
/// rig shares one. The wrapper's `SOLO_BOARDS` has to cover this list, and a
/// label past what it made falls back to a `bd init` of its own rather than
/// onto the shared board.
const SOLO: [&str; 2] = ["plan-pool", "fly-pins"];

/// The run's initialised board for `label`, or none when no run made one.
///
/// A rig copies what this names into its own root. The copy carries the config
/// that points at the database, so the rows are the run's one board while the
/// files beside them — `fleet.toml`, the packs directory — stay the rig's own.
pub fn run_board(label: &str) -> Option<PathBuf> {
    let boards = PathBuf::from(std::env::var(BOARDS_VAR).ok()?);
    let name = match SOLO.iter().position(|solo| *solo == label) {
        Some(n) => format!("solo{}", n + 1),
        None => String::from("shared"),
    };
    let board = boards.join(name);
    board.join(".beads").is_dir().then_some(board)
}

/// The extra `bd init` arguments that put this rig's board on the run's server,
/// empty when no run has started one.
///
/// THE RUN'S INIT COUNT IS TAKEN HERE, because every real `bd init` in these
/// suites calls this immediately before running one — a count kept anywhere
/// else would be a second list of the init sites to keep current.
pub fn bd_init_server_args(label: &str) -> Vec<String> {
    note_bd_init(label);
    let port = match std::env::var(PORT_VAR) {
        Ok(port) => port.trim().to_string(),
        Err(_) => return Vec::new(),
    };
    if port.is_empty() {
        return Vec::new();
    }
    assert_ne!(
        port, LIVE_PORT,
        "{PORT_VAR}={LIVE_PORT} names the fleet's live board on this box, not a test server"
    );
    vec![
        "--server".to_string(),
        "--server-host".to_string(),
        "127.0.0.1".to_string(),
        "--server-port".to_string(),
        port,
        "--database".to_string(),
        database_name(label),
    ]
}

/// One line for one `bd init`. Silent where no run is counting.
///
/// Public for the one init these suites do not run themselves: the bd
/// adapter's own `scratch`, which inits on bd's embedded engine and so takes
/// no server arguments, and is counted here all the same.
pub fn note_bd_init(label: &str) {
    use std::io::Write;
    let Ok(path) = std::env::var(INIT_LOG_VAR) else {
        return;
    };
    if let Ok(mut log) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = writeln!(log, "rig {label} {}", std::process::id());
    }
}

/// One database name per rig: two rigs sharing a name on the one server would
/// share a board. The pid separates binaries, the counter separates rigs inside
/// one, and every character a MySQL identifier does not take becomes `_`.
fn database_name(label: &str) -> String {
    let n = NEXT_DATABASE.fetch_add(1, Ordering::SeqCst);
    let label: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("fx_{label}_{}_{n}", std::process::id())
}
