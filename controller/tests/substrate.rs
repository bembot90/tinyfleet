//! The agent release the loop expects, AS THE LOOP READS IT: the releases the
//! agent's adapter declares it was measured against (ruling 8), which its
//! `capabilities` answer carries as `measured`. No fleet file pins one.
//!
//! A test binary of its own, for the reason `grant.rs` is one: these arms drive
//! `run::observe_runs`, which reads the PROCESS's environment for the machine
//! directory, so the rigs are serialized on the lock below.
//!
//! What it measures is the thing the projection's own unit arms cannot: that
//! the expectation the adapter declares is the one the poll publishes AND the
//! one it writes `substrate.moved` against.
//!
//! The agent is `fleet-agent-stub`, named by `[agent] adapter` and spoken to
//! through the Exec as any adapter executable is: it declares the releases the
//! arm scripts, and it answers the version the arm scripts.

use fleet_controller::platform::{self, Grant};
use fleet_controller::run::{self, Options};
use fleet_controller::test_support::agent_stub;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

mod common;

/// The environment is the process's, so one rig runs at a time. A poisoned lock
/// is taken anyway: the panic that poisoned it already failed its own arm, and
/// refusing it here would fail every other arm for it.
static ENV: Mutex<()> = Mutex::new(());

/// The releases every rig's adapter declares it was measured against, first
/// and second — so an arm can run the one a reader of `measured.first()` alone
/// would call a spread.
const MEASURED: [&str; 2] = ["9.9.9", "9.9.10"];

/// A release outside [`MEASURED`] — the spread every arm below either
/// announces or does not.
const ANOTHER: &str = "0.0.1";

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

/// The rig: a machine directory naming no seat, a policy file with `[agent]
/// adapter` naming the stub, and the stub declaring [`MEASURED`] and answering
/// `version` with the release the arm chooses.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str, running: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("fleet-substrate-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            machine: root.join("machine"),
            root,
            _held: held,
        };
        write(
            &rig.root.join("fleet.toml"),
            &format!(
                "[controller]\npoll_seconds = 1\n\n[agent]\nadapter = \"{}\"\n",
                Rig::adapter()
            ),
        );
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": []}}\n",
                rig.root.join("fleet.toml").display()
            ),
        );
        agent_stub::script(&rig.root, |a| {
            a.version = Some(running.to_string());
            a.capabilities.measured = MEASURED.iter().map(|m| m.to_string()).collect();
        });
        common::hermetic::export(common::hermetic::vars(&rig.root, &rig.machine));
        rig
    }

    /// The name `[agent] adapter` calls the stub by: its path, as written.
    fn adapter() -> String {
        agent_stub::path().display().to_string()
    }

    /// One poll, the way `fleet observe --once` takes it.
    fn poll(&self) {
        let gate = Grant::new(platform::directory_listing(), Duration::from_secs(5));
        assert_eq!(run::observe_runs(&Options { once: true }, gate, None), 0);
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

/// A LIVE RELEASE OUTSIDE WHAT THE ADAPTER WAS MEASURED AGAINST is one
/// `substrate.moved`, naming the adapter by the name `[agent] adapter` calls it
/// and the first release it declares as the one expected, and the poll still
/// exits 0. The projection's agent block carries the same two.
#[test]
fn a_release_outside_the_measured_ones_writes_substrate_moved_naming_the_adapter() {
    assert!(!MEASURED.contains(&ANOTHER));
    let rig = Rig::new("outside", ANOTHER);
    rig.poll();

    let published = rig.projection();
    assert_eq!(published["agent_version"], ANOTHER, "{published}");
    assert_eq!(
        published["agent_version_expected"], MEASURED[0],
        "{published}"
    );
    assert_eq!(published["agent"]["expected"], MEASURED[0], "{published}");

    let moves = rig.moves();
    assert_eq!(moves.len(), 1, "one move is one event: {moves:?}");
    assert_eq!(moves[0]["payload"]["agent"], Rig::adapter());
    assert_eq!(moves[0]["payload"]["observed"], ANOTHER);
    assert_eq!(moves[0]["payload"]["expected"], MEASURED[0]);
}

/// The control: a live release AMONG the measured ones is no spread — the
/// second declared as much as the first — and the expectation published is
/// the live one itself, so a reader comparing the two finds none.
///
/// RED-PROOF: with the live release held to the first declared alone, the
/// second release running writes a move and publishes the first as expected.
#[test]
fn a_release_among_the_measured_ones_writes_nothing() {
    for (label, running) in [("first", MEASURED[0]), ("second", MEASURED[1])] {
        let rig = Rig::new(&format!("among-{label}"), running);
        rig.poll();
        let published = rig.projection();
        assert_eq!(published["agent_version"], running, "{label}: {published}");
        assert_eq!(
            published["agent_version_expected"], running,
            "{label}: {published}"
        );
        assert_eq!(rig.moves(), Vec::<serde_json::Value>::new(), "{label}");
    }
}
