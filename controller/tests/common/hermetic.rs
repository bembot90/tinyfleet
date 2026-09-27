//! The env block every rig puts on the fleet processes it spawns and on the
//! process it drives in-process.
//!
//! A suite run inside a flight this fleet's own controller is flying inherits
//! that controller's environment, so an arm naming none of the roots reads the
//! RUNNING machine directory — its packs, and the agent adapter they carry —
//! or starts sessions on the operator's own tmux server. The three roots, the
//! tmux binary and the refusal flag are named here, unconditionally, in ONE
//! list — a second copy of the list is a second thing to remember to change.
//! The same list STRIPS the actor a session is started with ([`FLEET_ACTOR`]),
//! so no arm acts as the seat that ran the suite. A rig driving the loop in the
//! test process puts it on itself with [`export`].
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
/// The user's home, which every constructed `PATH` is built off.
pub const HOME: &str = "HOME";
/// The tmux binary every host call runs (`fleet_controller::host::tmux`). A
/// rig with no tmux stub of its own gets the refusing one, so no arm reaches
/// the operator's tmux, and so never the live fleet's own socket on it.
pub const TMUX_BIN: &str = "FLEET_TMUX_BIN";
/// Set, `TMUX_BIN` naming nothing is a refusal rather than a fall back to a
/// `tmux` on `PATH` (`fleet_controller::platform::hermetic`).
pub const HERMETIC: &str = "FLEET_TEST_HERMETIC";

/// Who a verb acts as where the call names no `--by`. The controller sets it
/// on every session it starts, to `seat:<id>`, so a suite run from inside a
/// fleet-started session inherits that seat — and every arm naming no actor
/// would act as it, where on a person's shell the same arm acts as the
/// machine's identity. STRIPPED, never set: an arm whose subject is the actor
/// sets it itself, after the block.
pub const FLEET_ACTOR: &str = "FLEET_ACTOR";

/// The agent stub's three knobs (`fleet_controller::test_support::agent_stub`):
/// a call that sleeps before it answers, one that writes nothing back, and a
/// launch that writes a file where it is told. Each is read off the
/// environment of the call that carries it, and every agent call inherits the
/// environment of the `fleet` that makes it, so one left set in the shell a
/// suite runs from would slow, deafen or litter every stub a rig drives.
/// STRIPPED, never set: an arm whose subject is a knob sets it itself, after
/// the block.
pub const AGENT_STUB_SLOW: &str = "FLEET_AGENT_STUB_SLOW";
pub const AGENT_STUB_DEAF: &str = "FLEET_AGENT_STUB_DEAF";
pub const AGENT_STUB_WRITE: &str = "FLEET_AGENT_STUB_WRITE";

/// The block, as name/value pairs, for a rig that holds the environment itself
/// rather than putting it on a `Command`. A `None` value is a name the block
/// REMOVES rather than sets.
///
/// The tmux binary is always the REFUSING tmux stub here: a rig whose subject
/// is the host names its own after the block. The agent is whatever the rig's
/// fleet file names, resolved through the rig's own machine directory, and
/// never one of the operator's.
pub fn vars(home: &Path, machine: &Path) -> Vec<(&'static str, Option<OsString>)> {
    vec![
        (HOME, Some(home.as_os_str().to_owned())),
        (FLEET_HOME, Some(home.as_os_str().to_owned())),
        (FLEET_DIR, Some(machine.as_os_str().to_owned())),
        (TMUX_BIN, Some(refusing_tmux().into_os_string())),
        (HERMETIC, Some(OsString::from("1"))),
        (FLEET_ACTOR, None),
        (AGENT_STUB_SLOW, None),
        (AGENT_STUB_DEAF, None),
        (AGENT_STUB_WRITE, None),
    ]
}

/// A block put on THIS process: each value set and each stripped name removed.
pub fn export(block: Vec<(&'static str, Option<OsString>)>) {
    for (key, value) in block {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

/// The env block on a `Command`, in the chain the rig already writes.
pub trait Hermetic {
    /// `home` and `machine` are the rig's own, under its temp root.
    fn hermetic(&mut self, home: &Path, machine: &Path) -> &mut Self;

    /// The same block over [`nowhere`], for an arm that owns no scratch.
    fn hermetic_nowhere(&mut self) -> &mut Self;
}

impl Hermetic for Command {
    fn hermetic(&mut self, home: &Path, machine: &Path) -> &mut Self {
        for (key, value) in vars(home, machine) {
            match value {
                Some(value) => self.env(key, value),
                None => self.env_remove(key),
            };
        }
        self
    }

    fn hermetic_nowhere(&mut self) -> &mut Self {
        let root = nowhere();
        self.hermetic(&root.join("home"), &root.join("machine"))
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

/// A tmux that refuses whatever it is asked, written once under the temp
/// directory and shared by every arm that names no tmux of its own: every host
/// call a rig did not point at a tmux stub of its own fails, naming why, and no
/// tmux server is ever started. It is BELT AND BRACES behind the refusal flag,
/// which stops the same arm one step earlier — the flag can be cleared by an
/// arm whose subject is the fallback, and this cannot.
///
/// Written to a unique name and renamed into place, because two test binaries
/// reach this at once and a third must never read half a file. The name also
/// carries a per-call counter beside the pid: libtest runs one binary's arms
/// as THREADS of one process, so the pid alone is not unique between two
/// arms of the same binary calling this at once.
pub fn refusing_tmux() -> PathBuf {
    refusing(
        "fleet-refusing-tmux",
        "no tmux binary: this rig named no FLEET_TMUX_BIN",
    )
}

/// A script at `<temp>/<stem>.sh` that prints `line` on stderr and exits 127,
/// written as [`refusing_tmux`] says.
fn refusing(stem: &str, line: &str) -> PathBuf {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{stem}.sh"));
    let mine = std::env::temp_dir().join(format!("{stem}-{}-{}.sh.tmp", std::process::id(), call));
    std::fs::write(&mine, format!("#!/bin/sh\necho \"{line}\" >&2\nexit 127\n"))
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
