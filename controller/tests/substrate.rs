//! The Claude Code release the loop expects, AS THE LOOP READS IT: the
//! fleet's own `[substrate]` pin where its file writes one, and otherwise the
//! release fleet supports (`fleet_core::supported::PINNED_CLAUDE_CODE`).
//!
//! A test binary of its own, for the reason `grant.rs` is one: these arms drive
//! `run::observe_with`, which reads the PROCESS's environment for the machine
//! directory and the agent binary, so the rigs are serialized on the lock below.
//!
//! What it measures is the thing the policy's own unit arms cannot: that the
//! expectation a file that pins nothing falls to is the one the poll publishes
//! AND the one it writes `substrate.moved` against.

use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options};
use fleet_core::supported::PINNED_CLAUDE_CODE;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

mod common;

/// The environment is the process's, so one rig runs at a time. A poisoned lock
/// is taken anyway: the panic that poisoned it already failed its own arm, and
/// refusing it here would fail every other arm for it.
static ENV: Mutex<()> = Mutex::new(());

/// A release the supported one is not, in the shape `claude --version` prints
/// it — the spread every arm below either announces or does not.
const ANOTHER: &str = "0.0.1";

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

/// The rig: a machine directory naming no seat, a policy file whose body the
/// arm chooses, and an agent stub answering `--version` with the release the
/// arm chooses and an empty roster to everything else.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str, policy: &str, running: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("fleet-substrate-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            machine: root.join("machine"),
            root,
            _held: held,
        };
        write(&rig.root.join("fleet.toml"), policy);
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": []}}\n",
                rig.root.join("fleet.toml").display()
            ),
        );
        let stub = rig.root.join("agent.sh");
        write(
            &stub,
            &format!(
                "#!/bin/sh\ncase \"$*\" in\n  *--version*) echo \"{running} (Claude Code)\" ;;\n  \
                 *) printf '%s' '[]' ;;\nesac\nexit 0\n"
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
        for (key, value) in common::hermetic::in_process_vars(&rig.root, &rig.machine, Some(&stub))
        {
            std::env::set_var(key, value);
        }
        rig
    }

    /// One poll, the way `fleet observe --once` takes it.
    fn poll(&self) {
        let gate = Grant::new(platform::directory_listing(), Duration::from_secs(5));
        assert_eq!(run::observe_with(&Options { once: true }, gate), 0);
    }

    fn projection(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine.join("projection.json"))
            .expect("a projection is published");
        serde_json::from_str(&body).expect("the projection parses")
    }

    fn moves(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["type"] == "substrate.moved")
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// fleet-2jt — A FLEET THAT PINS NOTHING EXPECTS THE RELEASE FLEET SUPPORTS: a
/// different live release is one `substrate.moved` against that release, and
/// the poll still exits 0. The file's own word stays null on the projection, so
/// a reader tells the fleet's pin from the default it fell to.
#[test]
fn no_pin_and_another_release_running_writes_substrate_moved_against_the_supported_one() {
    assert_ne!(PINNED_CLAUDE_CODE, ANOTHER);
    let rig = Rig::new("unpinned", "[controller]\npoll_seconds = 1\n", ANOTHER);
    rig.poll();

    let published = rig.projection();
    assert_eq!(published["agent_version"], ANOTHER, "{published}");
    assert_eq!(
        published["agent_version_expected"], PINNED_CLAUDE_CODE,
        "{published}"
    );
    assert!(published["fleet"]["claude_code"].is_null(), "{published}");

    let moves = rig.moves();
    assert_eq!(moves.len(), 1, "one move is one event: {moves:?}");
    assert_eq!(moves[0]["payload"]["agent"], "claude_code");
    assert_eq!(moves[0]["payload"]["observed"], ANOTHER);
    assert_eq!(moves[0]["payload"]["expected"], PINNED_CLAUDE_CODE);
}

/// The control: the supported release running under the same file is no
/// spread, so the event above is the release's and not the missing pin's.
#[test]
fn no_pin_and_the_supported_release_running_writes_nothing() {
    let rig = Rig::new(
        "unpinned-agrees",
        "[controller]\npoll_seconds = 1\n",
        PINNED_CLAUDE_CODE,
    );
    rig.poll();
    assert_eq!(rig.projection()["agent_version"], PINNED_CLAUDE_CODE);
    assert_eq!(rig.moves(), Vec::<serde_json::Value>::new());
}

/// A FLEET'S OWN PIN STILL WINS: the release it pins running is no spread even
/// though it is not the supported one, and the supported one running is a
/// spread against the pin.
#[test]
fn a_fleets_own_pin_wins_over_the_supported_release() {
    let pinned =
        format!("[controller]\npoll_seconds = 1\n\n[substrate]\nclaude_code = \"{ANOTHER}\"\n");

    let rig = Rig::new("pinned-agrees", &pinned, ANOTHER);
    rig.poll();
    let published = rig.projection();
    assert_eq!(published["agent_version_expected"], ANOTHER, "{published}");
    assert_eq!(published["fleet"]["claude_code"], ANOTHER, "{published}");
    assert_eq!(rig.moves(), Vec::<serde_json::Value>::new());
    drop(rig);

    let rig = Rig::new("pinned-moved", &pinned, PINNED_CLAUDE_CODE);
    rig.poll();
    let moves = rig.moves();
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert_eq!(moves[0]["payload"]["observed"], PINNED_CLAUDE_CODE);
    assert_eq!(moves[0]["payload"]["expected"], ANOTHER);
}
