//! The env block every rig puts on the fleet processes it spawns and on the
//! process it drives in-process.
//!
//! A suite run inside a flight this fleet's own controller is flying inherits
//! that controller's environment, so an arm naming none of the roots reads the
//! RUNNING machine directory and execs the account's real agent binary. The
//! three roots, the agent binary and the refusal flag are named here,
//! unconditionally, in ONE list — a second copy of the list is a second thing
//! to remember to change. A rig driving the loop in the test process takes
//! [`in_process_vars`], which is that list plus the agent's config directory.
//!
//! Included by `#[path]` into each crate's `tests/common` rather than copied,
//! and the only file under `fleet/*/tests` that spells any of the names.
#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The machine directory itself: read first and winning outright.
pub const FLEET_DIR: &str = "FLEET_DIR";
/// The home the machine directory sits under, read when `FLEET_DIR` is unset
/// and BEFORE the user's own home. No rig set it before this block existed,
/// which is why a rig naming only `HOME` still resolved to the live directory.
pub const FLEET_HOME: &str = "FLEET_HOME";
/// The user's home: the adapter's transcript locations key off it.
pub const HOME: &str = "HOME";
/// The agent binary.
pub const CLAUDE_BIN: &str = "FLEET_CLAUDE_BIN";
/// Set, `CLAUDE_BIN` naming nothing is a refusal rather than a fall back to a
/// `claude` on `PATH` — see
/// `fleet_controller::adapter::claude_code::configured_bin`.
pub const HERMETIC: &str = "FLEET_TEST_HERMETIC";

/// The agent's config directory, read BEFORE the home beside it
/// (`fleet_controller::adapter::claude_code::config_dir_from`), so a rig whose
/// transcripts live under its own `home/.claude` is read out of the operator's
/// real one whenever this is set — and a controller sets it on every child it
/// spawns, so a seat's own shell carries it.
///
/// NOT in [`vars`]: a child built from that block reads this same setting as its
/// CREDENTIAL scope, where unset and set-to-a-path are different answers
/// (`credential_dir_from`, lessons claude-code A11), so it is shadowed only for
/// a rig that drives the loop IN THIS PROCESS.
pub const CONFIG_DIR: &str = "CLAUDE_CONFIG_DIR";

/// The block, as name/value pairs, for a rig that holds the environment itself
/// rather than putting it on a `Command`.
///
/// `claude_bin` is the rig's own stub where it has one. `None` names the
/// REFUSING stub instead: the variable is set either way, so the resolution
/// never reaches a `claude` on the process `PATH` whether or not the refusal
/// flag is read.
pub fn vars(
    home: &Path,
    machine: &Path,
    claude_bin: Option<&Path>,
) -> Vec<(&'static str, OsString)> {
    let bin = match claude_bin {
        Some(bin) => bin.to_path_buf(),
        None => refusing_stub(),
    };
    vec![
        (HOME, home.as_os_str().to_owned()),
        (FLEET_HOME, home.as_os_str().to_owned()),
        (FLEET_DIR, machine.as_os_str().to_owned()),
        (CLAUDE_BIN, bin.into_os_string()),
        (HERMETIC, OsString::from("1")),
    ]
}

/// [`vars`] plus [`CONFIG_DIR`]: the block for a rig that drives the loop in the
/// test process itself, where the adapter resolves its config directory off this
/// process's environment.
pub fn in_process_vars(
    home: &Path,
    machine: &Path,
    claude_bin: Option<&Path>,
) -> Vec<(&'static str, OsString)> {
    let mut block = vars(home, machine, claude_bin);
    block.push((CONFIG_DIR, home.join(".claude").into_os_string()));
    block
}

/// The env block on a `Command`, in the chain the rig already writes.
pub trait Hermetic {
    /// `home` and `machine` are the rig's own, under its temp root; `claude_bin`
    /// is its stub where it has one and `None` where it has none.
    fn hermetic(&mut self, home: &Path, machine: &Path, claude_bin: Option<&Path>) -> &mut Self;

    /// The same block over [`nowhere`], for an arm that owns no scratch.
    fn hermetic_nowhere(&mut self) -> &mut Self;
}

impl Hermetic for Command {
    fn hermetic(&mut self, home: &Path, machine: &Path, claude_bin: Option<&Path>) -> &mut Self {
        for (key, value) in vars(home, machine, claude_bin) {
            self.env(key, value);
        }
        self
    }

    fn hermetic_nowhere(&mut self) -> &mut Self {
        let root = nowhere();
        self.hermetic(&root.join("home"), &root.join("machine"), None)
    }
}

/// A root for an arm that names no scratch of its own — one about `--help`, or
/// about a verb reading only the flags it was handed.
///
/// Nothing is written under it and it need not exist: its EMPTINESS is the
/// fixture. What it defends is the resolution, which without it reaches the
/// machine directory of whatever fleet is running on this box, and answers out
/// of that fleet's policy rather than out of the arm's.
pub fn nowhere() -> PathBuf {
    std::env::temp_dir().join(format!("fleet-nowhere-{}", std::process::id()))
}

/// A stub that refuses whatever it is asked, written once under the temp
/// directory and shared by every arm that names no binary of its own.
///
/// What it defends is the arm that never named one: with this in the variable
/// the resolution stops here instead of finding the real agent on the process
/// `PATH` and spawning it under the real account. It is BELT AND BRACES behind
/// the refusal flag, which stops the same arm one step earlier — the flag can
/// be cleared by an arm whose subject is the fallback, and this cannot.
///
/// Written to a unique name and renamed into place, because two test binaries
/// reach this at once and a third must never read half a file. The name also
/// carries a per-call counter beside the pid: libtest runs one binary's arms
/// as THREADS of one process, so the pid alone is not unique between two
/// arms of the same binary calling this at once.
pub fn refusing_stub() -> PathBuf {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join("fleet-refusing-agent.sh");
    let mine = std::env::temp_dir().join(format!(
        "fleet-refusing-agent-{}-{}.sh.tmp",
        std::process::id(),
        call
    ));
    std::fs::write(
        &mine,
        "#!/bin/sh\necho \"no agent binary: this rig named no FLEET_CLAUDE_BIN\" >&2\nexit 127\n",
    )
    .expect("the refusing stub is written");
    make_executable(&mine);
    std::fs::rename(&mine, &path).expect("the refusing stub is renamed into place");
    path
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut mode = std::fs::metadata(path)
        .expect("the stub is readable")
        .permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(path, mode).expect("the stub is made executable");
}
