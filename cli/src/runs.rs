//! The run seam, filled: the three acts the controller's run pass needs and
//! that crate cannot make — re-run a run's bundle, hold a run at its crash
//! cap, and retire the seats a finished run spawned.
//!
//! WHY THE ACTS ARE HERE AND NOT IN CORE OR THE CONTROLLER. core depends on no
//! other member, and the controller takes nothing from core but its bounded
//! runner (`fleet_core::process`), so neither of them can wire a store, a
//! project's packs and the transient-seat primitives at once.
//! This module is the one that can, which is also why the tick's run pass is a
//! seam rather than a call.
//!
//! NOTHING IS HELD BETWEEN PASSES. Every act resolves the run's project afresh
//! off the machine directory, so a run opened between two passes is picked up
//! by the second with no registration of any kind.

use std::path::{Path, PathBuf};

use fleet_controller::runs::Runs;
use fleet_controller::transient::Refusal;
use fleet_controller::{clock, config, events, platform, transient};
use fleet_core::item::brief::Packs;
use fleet_core::item::hold;
use fleet_core::item::run as workflow_run;
use fleet_core::seat;
use fleet_core::store::{Bd, Store};

use crate::item::{resolve_from, Here, StreamEvents, EVENTS};
use crate::transient::{as_refusal, effect_agent, machine_of, policy_of, Where};

/// How the pass gets a store for one project.
///
/// AN OPENER AND NOT A STORE, because [`Engine`] holds no project and resolves
/// one per run: a machine whose runs belong to two projects needs a store per
/// root, and a single handle could not serve both.
pub trait Stores {
    fn open(&self, root: &Path) -> Box<dyn Store>;
}

/// The opener the binary runs on: `bd` at the project's own root.
///
/// THE BINARY IS RESOLVED ONCE, HERE, and every store this opener hands back
/// runs it by absolute path. The pass's own process is a launchd service whose
/// `PATH` holds neither a package manager's prefix nor the user's local bin, so
/// a store that searches that `PATH` finds no `bd` and refuses every tick
/// (lessons claude-code D1). The resolution is the verbs' own
/// ([`crate::item::bd_bin`]), bare-name fallback and all, so a run's re-run and
/// a person's verb write through the same file.
pub struct BdStores {
    bd: PathBuf,
}

impl BdStores {
    pub fn resolved() -> BdStores {
        BdStores {
            bd: crate::item::bd_bin(),
        }
    }
}

impl Stores for BdStores {
    fn open(&self, root: &Path) -> Box<dyn Store> {
        Box::new(Bd::at_bin(root, &self.bd))
    }
}

/// The run seam's acts, over one machine directory.
///
/// IT HOLDS NO PROJECT. A run's directory carries its inputs, its policy and
/// its bundle and nothing that says where its record lives, and a controller
/// started as a service has no working directory to resolve one from — so the
/// record is looked for, project by project, over the roots this machine
/// registers.
pub struct Engine {
    pub machine_dir: PathBuf,
    pub home: PathBuf,
    stores: Box<dyn Stores>,
}

impl Engine {
    pub fn on(machine_dir: PathBuf) -> Engine {
        Engine {
            machine_dir,
            home: platform::home_dir(),
            stores: Box::new(BdStores::resolved()),
        }
    }

    /// The projects this machine holds.
    ///
    /// The register is the standalone fleets'; an embedded fleet's policy file
    /// sits at its project's own root, so that file's directory is the project.
    /// A standalone fleet's policy is under the machine directory, which is why
    /// the machine directory itself is never taken for a project.
    fn projects(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = fleet_controller::lifecycle::registered(&self.machine_dir)
            .unwrap_or_default()
            .into_iter()
            .map(|project| PathBuf::from(project.root))
            .collect();
        if let Ok(machine) = config::read(&self.machine_dir.join("config.json")) {
            if let Some(root) = machine.fleet_toml.parent() {
                if root != self.machine_dir && !roots.iter().any(|held| held == root) {
                    roots.push(root.to_path_buf());
                }
            }
        }
        roots
    }

    /// The project whose store holds this run's record, resolved.
    ///
    /// THE STORE IS THE INDEX and the walk stops at the first answer: a run id
    /// is the store's own, so two projects cannot both hold one. A project that
    /// will not resolve is skipped rather than refused — the run may be in the
    /// next one, and a machine is not broken because one of its roots moved.
    fn project_holding(&self, run: &str) -> Result<Here, String> {
        for root in self.projects() {
            let Ok(here) = resolve_from(&root, self.machine_dir.clone(), None) else {
                continue;
            };
            if self.stores.open(&here.project.root).show(run).is_ok() {
                return Ok(here);
            }
        }
        Err(format!(
            "no project registered with this machine holds a record for {run}"
        ))
    }
}

/// The controller decides which runs move — a fold of the machine's own stream
/// — and these are the three acts that decision needs: each resolves the
/// project whose store holds the run, and a machine with one project asks one
/// store one question.
impl Runs for Engine {
    fn rerun(&self, run: &str) -> Result<(), String> {
        let here = self.project_holding(run)?;
        let store = self.stores.open(&here.project.root);
        let packs =
            Packs::under(&here.packs_dir, &here.defaults_dir).map_err(|stop| stop.message)?;
        let events = StreamEvents::at(self.machine_dir.join(EVENTS));
        let fleet_bin = std::env::current_exe()
            .map_err(|e| format!("this process cannot name its own binary: {e}"))?;
        let at = clock::now_stamp();
        // This pass is a launchd service, so the run's children are handed a
        // constructed search path rather than this process's own.
        let child_path = platform::child_path(&self.home);
        // The loop has no terminal of its own and its stdout is the service's
        // log, so the run's two lines go where a refusal already goes.
        workflow_run::rerun(
            &mut std::io::stderr(),
            &workflow_run::Again {
                run,
                by: events::CONTROLLER,
                at: &at,
                machine_dir: &self.machine_dir,
                fleet_bin: &fleet_bin,
            },
            &workflow_run::Wiring {
                store: store.as_ref(),
                project: &here.project,
                packs: &packs,
                policy_file: &here.policy_file,
                events: &events,
                stream: &events,
                child_path: &child_path,
            },
        )
        .map(|_| ())
        .map_err(|stop| stop.message)
    }

    /// THE PARK AND NOT THE BARE HOLD: the note beside the hold is what
    /// `fleet clear` clears it through, and the packs are resolved here
    /// because the note is written in the park grammar they carry.
    fn hold(&self, run: &str, reason: &str) -> Result<String, String> {
        let here = self.project_holding(run)?;
        let store = self.stores.open(&here.project.root);
        let packs =
            Packs::under(&here.packs_dir, &here.defaults_dir).map_err(|stop| stop.message)?;
        let directory = self.machine_dir.join(workflow_run::RUNS).join(run);
        hold::park_at_the_cap(
            &hold::Capped {
                run,
                reason,
                directory: &directory,
                by: events::CONTROLLER,
            },
            store.as_ref(),
            &packs,
        )
        .map_err(|stop| stop.message)
    }

    fn retire(&self, seat: &str, run: &str) -> Result<(), String> {
        let here = self.project_holding(run)?;
        let agent = effect_agent(&here, &self.home).map_err(|stop| stop.message)?;
        let policy = policy_of(&here).map_err(|stop| stop.message)?;
        let at = Where::of(&here).map_err(|stop| stop.message)?;
        let machine = machine_of(&here, &at, &agent, &policy);
        // THE RECORD'S HALF, which the controller reaches no work graph to do
        // for itself. A cleanup retires seats whose items were delivered and
        // seats whose items are still open — a park leaves the order standing —
        // and the name this frees is the one the next spawn takes.
        let store = self.stores.open(&here.project.root);
        let withdrawal = |going: &str| withdrawn_from(store.as_ref(), going, events::CONTROLLER);
        // THE PRICED RETIRE and not the bare one: a seat a run spawned costs
        // what any spawned seat costs, and the run is the item it was working
        // for. It writes `session.retired`, which is the line the cleanup's
        // count is a count of.
        transient::priced_with(&machine, seat, run, clock::now_ms(), &withdrawal)
            .map(|_| ())
            .map_err(|refusal| refusal.message)
    }
}

/// Every open ordered item this seat still holds, released, answered by id.
///
/// AN UNREADABLE BOARD STOPS THE RETIRE HERE, where the hand verb lets one
/// through for a seat its session row names no item for. The difference is what
/// the caller already knows: this pass resolved the run's project BY ASKING
/// THIS STORE for the run's own record, so a store that will not answer the
/// question below has stopped answering since — a question, and never a seat
/// that was given nothing.
fn withdrawn_from(store: &dyn Store, seat: &str, by: &str) -> Result<Vec<String>, Refusal> {
    let held = seat::retire::held(store, seat).map_err(as_refusal)?;
    if held.is_empty() {
        return Ok(Vec::new());
    }
    seat::retire::withdraw(store, &held, seat, by).map_err(as_refusal)?;
    Ok(held.into_iter().map(|row| row.id).collect())
}

/// The run seam's own half of the retire, which no end-to-end arm reaches: the
/// cli is a binary crate, so a `cfg(test)` module here is the one route to
/// [`withdrawn_from`] from an arm, and the machine, worktree and session table
/// an [`Engine`] resolves around it are the controller suite's.
#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::item::COULD_NOT_TELL;
    use fleet_core::seat::retire::WITHDRAWN;
    use fleet_core::store::{Item, Orders};
    use fleet_core::test_support::FakeStore;

    const SEAT: &str = "agent-0c3a5e71";
    const PARKED: &str = "fx-parked";

    /// The state the leak lives in: a run's item PARKED, so it is still open
    /// and the order naming the seat still stands when the cleanup retires it.
    fn a_parked_item() -> FakeStore {
        let store = FakeStore::default();
        store.seed(Item {
            id: PARKED.to_string(),
            title: String::from("an item a seat was dispatched and parked"),
            status: String::from("open"),
            assignee: Some(SEAT.to_string()),
            orders: Some(Orders {
                by: Some(String::from("a-run")),
                kind: Some(String::from("run")),
                seat: Some(SEAT.to_string()),
                at: Some(String::from("2026-09-14T10:40:39Z")),
            }),
            has_orders_key: true,
            ..Item::default()
        });
        store
    }

    #[test]
    fn the_cleanups_withdrawal_releases_the_parked_item_the_seat_holds() {
        let store = a_parked_item();

        let withdrawn = withdrawn_from(&store, SEAT, events::CONTROLLER)
            .expect("the board answers and the withdrawal lands");
        assert_eq!(withdrawn, vec![PARKED.to_string()]);

        let after = store.show(PARKED).expect("the store answers");
        assert!(
            after.assignee.as_deref().unwrap_or("").trim().is_empty(),
            "the item reads unassigned: {:?}",
            after.assignee
        );
        assert!(
            !after.has_orders_key,
            "and carries no orders key: {}",
            after.document
        );
        assert_eq!(after.status, "open", "the work itself is still to be done");
        assert_eq!(
            after.notes.unwrap_or_default(),
            format!("{WITHDRAWN}: {SEAT} retired by controller; the item is open and unassigned"),
            "the withdrawal says who took it, and the cleanup is the controller"
        );
    }

    /// A seat holding nothing ordered is retired as it always was: the board is
    /// read once and nothing is written.
    #[test]
    fn a_seat_holding_nothing_ordered_is_withdrawn_from_without_a_write() {
        let store = FakeStore::default();

        let withdrawn =
            withdrawn_from(&store, SEAT, events::CONTROLLER).expect("the board answers");

        assert!(withdrawn.is_empty(), "{withdrawn:?}");
        assert!(
            store.wrote().is_empty(),
            "nothing written: {:?}",
            store.wrote()
        );
    }

    /// A board that will not answer stops the cleanup's retire rather than
    /// freeing the name over silence, and the exit is carried across unchanged.
    #[test]
    fn a_board_that_will_not_answer_stops_the_cleanups_retire() {
        let store = FakeStore {
            unreadable: Some(String::from("bd is not on this path")),
            ..FakeStore::default()
        };

        let refused = withdrawn_from(&store, SEAT, events::CONTROLLER)
            .expect_err("an unreadable board is a question");

        assert_eq!(refused.code, COULD_NOT_TELL);
        assert!(
            refused.message.contains("bd is not on this path"),
            "{}",
            refused.message
        );
    }

    /// The `PATH` a launchd agent is started with, which is the whole of this
    /// box's search path for the controller: no package-manager prefix, where
    /// `bd` actually is, and no user local bin either.
    const SERVICE_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

    /// Serialises the arms here that move the process's environment, because
    /// `cargo test` runs this binary's arms as threads of one process.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Variables moved for one arm and put back when it ends — on a panic too,
    /// so a red arm never leaves the service `PATH` behind for a sibling.
    struct EnvHeld(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl EnvHeld {
        fn set(moved: &[(&'static str, &std::ffi::OsStr)]) -> EnvHeld {
            let held = EnvHeld(
                moved
                    .iter()
                    .map(|(key, _)| (*key, std::env::var_os(key)))
                    .collect(),
            );
            for (key, value) in moved {
                std::env::set_var(key, value);
            }
            held
        }
    }

    impl Drop for EnvHeld {
        fn drop(&mut self) {
            for (key, before) in self.0.iter().rev() {
                match before {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// The opener the binary builds runs the `bd` it resolved, by absolute
    /// path, when the process's own `PATH` is the service's and holds no `bd`
    /// at all — which is the controller's situation on every tick.
    ///
    /// THE ENGINE AND NOT THE OPENER: [`Engine::on`] is the one place the pass
    /// gets its stores, so the arm reaches the store through it. THE CONTROL
    /// AND THE PROOF ARE ON ONE `PATH`: the shim is named to the resolver by
    /// absolute path and is unreachable by bare name, so a store that fell
    /// back to the bare name would be refused rather than find it.
    #[test]
    fn the_engines_store_runs_the_resolved_bd_on_a_service_path() {
        let _lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!("fleet-runs-bd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("project");
        std::fs::create_dir_all(&root).expect("the project root is created");

        // A `bd` that records the argv it was handed and answers an empty list.
        let log = dir.join("argv");
        let bd = dir.join("bd");
        std::fs::write(
            &bd,
            format!(
                "#!/bin/sh\n\
                 for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{log}'; done\n\
                 printf '[]\\n'\n",
                log = log.display(),
            ),
        )
        .expect("the shim is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bd, std::fs::Permissions::from_mode(0o755))
            .expect("the shim is executable");

        let (by_name, by_engine) = {
            let _held = EnvHeld::set(&[
                ("PATH", SERVICE_PATH.as_ref()),
                ("FLEET_BD_BIN", bd.as_os_str()),
            ]);
            (
                Bd::at_bin(&root, Path::new(fleet_core::store::BD)).ready(),
                Engine::on(dir.join("machine")).stores.open(&root).ready(),
            )
        };

        let refusal = by_name.expect_err("a bare `bd` is not on the service PATH");
        assert!(
            format!("{refusal:?}").contains("could not be run"),
            "the bare name must fail because nothing could be run, not for some \
             other reason — {refusal:?}"
        );
        assert!(by_engine
            .expect("the engine's store runs the shim, which answers a list")
            .is_empty());
        let argv: Vec<String> = std::fs::read_to_string(&log)
            .expect("the shim recorded its argv")
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(
            argv,
            vec![
                String::from("-C"),
                root.display().to_string(),
                String::from("ready"),
                String::from("--json"),
                String::from("-n"),
                String::from("0"),
            ],
            "exactly the engine's read reached the shim"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
