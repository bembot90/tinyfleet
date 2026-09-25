//! The run seam, filled: the three acts the controller's run pass needs and
//! that crate cannot make — re-run a run's bundle, hold a run at its crash
//! cap, and retire the seats a finished run spawned — and the one read, whether
//! a run's record already carries that hold.
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

use fleet_controller::runs::{CapHold, Runs};
use fleet_controller::transient::Refusal;
use fleet_controller::{clock, config, platform, transient};
use fleet_core::entry::{Body, HoldReason};
use fleet_core::item::brief::Packs;
use fleet_core::item::hold;
use fleet_core::item::run as workflow_run;
use fleet_core::seat;
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::seat::identity::{identity_or_mint, SeatId};
use fleet_core::store::{self, ItemId, Opening, PackDirs, Store, StoreError, STORE_TIMEOUT};

use crate::item::{resolve_from, Here, StreamEvents, EVENTS};
use crate::transient::{as_refusal, effect_agent, machine_of, policy_of, Where};

/// How the pass gets a store for one project.
///
/// AN OPENER AND NOT A STORE, because [`Engine`] holds no project and resolves
/// one per run: a machine whose runs belong to two projects needs a store per
/// project, and a single handle could not serve both.
pub trait Stores {
    fn open(&self, here: &Here) -> Result<Box<dyn Store>, StoreError>;
}

/// The opener the binary runs on: the store `[store] adapter` names in each
/// project's own file, through the one opener every verb takes.
///
/// THE SEARCH PATH IS CONSTRUCTED ONCE, HERE, and every store this opener
/// hands back resolves its binary on it. The pass's own process is a launchd
/// service whose `PATH` holds neither a package manager's prefix nor the
/// user's local bin, so a store that searched that `PATH` would find nothing
/// and refuse every tick (lessons claude-code D1). It is the verbs' own search
/// path, bare-name fallback and all, so a run's re-run and a person's verb
/// write through the same file.
pub struct ProjectStores {
    search_path: String,
}

impl ProjectStores {
    pub fn resolved() -> ProjectStores {
        ProjectStores {
            search_path: platform::child_path(&platform::home_dir()),
        }
    }
}

impl Stores for ProjectStores {
    fn open(&self, here: &Here) -> Result<Box<dyn Store>, StoreError> {
        store::open(&Opening {
            root: &here.project.root,
            policy: &here.project.policy,
            search_path: &self.search_path,
            strict: false,
            timeout: STORE_TIMEOUT,
            packs: Some(PackDirs {
                packs_dir: &here.packs_dir,
                defaults_dir: &here.defaults_dir,
            }),
        })
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
            stores: Box::new(ProjectStores::resolved()),
        }
    }

    /// Who the pass's acts are made by [ASSUMES D8]: the controller, under this
    /// machine's own identity — minted here where the machine has none yet, as
    /// a verb run on it with no actor would mint it. Every write the three acts
    /// make to a work graph, and every line they add to the stream, carries it.
    fn controller(&self) -> Result<Actor, String> {
        let (identity, _) = identity_or_mint(&self.machine_dir)
            .map_err(|why| format!("could not tell who acts: {why}"))?;
        Ok(Actor {
            kind: ActorKind::Controller,
            id: identity.id.to_string(),
        })
    }

    /// The project whose store holds this run's record, resolved.
    ///
    /// THE STORE IS THE INDEX and the walk stops at the first answer: a run id
    /// is the store's own, so two projects cannot both hold one. A project that
    /// will not resolve is skipped rather than refused — the run may be in the
    /// next one, and a machine is not broken because one of its roots moved.
    fn project_holding(&self, run: &str) -> Result<Here, String> {
        for root in registered_roots(&self.machine_dir) {
            let Ok(here) = resolve_from(&root, self.machine_dir.clone(), None) else {
                continue;
            };
            let holding = self.stores.open(&here).and_then(|store| store.show(run));
            if holding.is_ok() {
                return Ok(here);
            }
        }
        Err(format!(
            "no project registered with this machine holds a record for {run}"
        ))
    }
}

/// The projects this machine holds: what the run pass looks a run's record up
/// across, and what `fleet status` counts the open holds over.
///
/// The register is the standalone fleets'; an embedded fleet's policy file
/// sits at its project's own root, so that file's directory is the project.
/// A standalone fleet's policy is under the machine directory, which is why
/// the machine directory itself is never taken for a project.
pub(crate) fn registered_roots(machine_dir: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = fleet_controller::lifecycle::registered(machine_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|project| PathBuf::from(project.root))
        .collect();
    if let Ok(machine) = config::read(&machine_dir.join("config.json")) {
        if let Some(root) = machine.fleet_toml.parent() {
            if root != machine_dir && !roots.iter().any(|held| held == root) {
                roots.push(root.to_path_buf());
            }
        }
    }
    roots
}

/// The controller decides which runs move — a fold of the machine's own stream
/// — and these are the three acts that decision needs: each resolves the
/// project whose store holds the run, and a machine with one project asks one
/// store one question.
impl Runs for Engine {
    fn rerun(&self, run: &str) -> Result<(), String> {
        let here = self.project_holding(run)?;
        let by = self.controller()?;
        let store = self.stores.open(&here).map_err(|e| e.to_string())?;
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
                by: &by,
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

    /// THE PARK AND NOT THE BARE HOLD: the held entry beside the hold is
    /// what `fleet clear` clears it through, and what the pass's signal names.
    fn hold(&self, run: &str, reason: &str) -> Result<(String, String), String> {
        let here = self.project_holding(run)?;
        let by = self.controller()?;
        let store = self.stores.open(&here).map_err(|e| e.to_string())?;
        let directory = self.machine_dir.join(workflow_run::RUNS).join(run);
        hold::park_at_the_cap(
            &hold::Capped {
                run,
                reason,
                directory: &directory,
                by: &by,
            },
            store.as_ref(),
        )
        .map_err(|stop| stop.message)
    }

    /// The LAST held entry of reason `max_crashes` on the run's timeline, and
    /// whether the store still lists its hold open. The run's own asks are
    /// held entries too, and none of them is the park.
    fn capped(&self, run: &str) -> Result<Option<CapHold>, String> {
        let here = self.project_holding(run)?;
        let store = self
            .stores
            .open(&here)
            .map_err(|e| format!("{run}'s store could not be opened: {e}"))?;
        let entries = store
            .timeline(&ItemId::from(run))
            .map_err(|e| format!("{run}'s timeline could not be read: {e}"))?;
        let Some(hold) = entries.iter().rev().find_map(|entry| match &entry.body {
            Body::Held(held) if held.reason == HoldReason::MaxCrashes => Some(held.hold.clone()),
            _ => None,
        }) else {
            return Ok(None);
        };
        let open = store
            .holds_open()
            .map_err(|e| format!("the store's open holds could not be read for {run}: {e}"))?;
        Ok(Some(CapHold {
            cleared: !open.iter().any(|held| *held == hold),
            hold,
        }))
    }

    fn retire(&self, seat: &str, run: &str) -> Result<(), String> {
        let here = self.project_holding(run)?;
        let by = self.controller()?;
        let agent = effect_agent(&here, &self.home).map_err(|stop| stop.message)?;
        let policy = policy_of(&here).map_err(|stop| stop.message)?;
        let at = Where::of(&here).map_err(|stop| stop.message)?;
        let machine = machine_of(&here, &at, &agent, &policy);
        // THE RECORD'S HALF, which the controller reaches no work graph to do
        // for itself. A cleanup retires seats whose items were delivered and
        // seats whose items are still open — a park leaves the order standing —
        // and the name this frees is the one the next spawn takes.
        let store = self.stores.open(&here).map_err(|e| e.to_string())?;
        // The retire hands its withdrawal the machine name of the row it
        // resolved; the order was assigned to that row's ID, so the name is
        // resolved back to it here, exactly, through the short id it carries.
        let withdrawal = |going: &str| {
            let row = here
                .seats
                .resolve_running(going)
                .map_err(|unresolved| Refusal {
                    code: unresolved.code(),
                    message: unresolved.to_string(),
                })?;
            withdrawn_from(store.as_ref(), &row.id, &here.seats.label(&row.id), &by)
        };
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
///
/// `seat` is the seat's id, which is what the order was assigned to, and
/// `label` how the withdrawal note names it.
fn withdrawn_from(
    store: &dyn Store,
    seat: &SeatId,
    label: &str,
    by: &Actor,
) -> Result<Vec<String>, Refusal> {
    let held = seat::retire::held(store, seat).map_err(as_refusal)?;
    if held.is_empty() {
        return Ok(Vec::new());
    }
    seat::retire::withdraw(store, &held, seat, label, by).map_err(as_refusal)?;
    Ok(held.into_iter().map(|row| row.id.to_string()).collect())
}

/// The run seam's own half of the retire, which no end-to-end arm reaches: the
/// cli is a binary crate, so a `cfg(test)` module here is the one route to
/// [`withdrawn_from`] from an arm, and the machine, worktree and session table
/// an [`Engine`] resolves around it are the controller suite's.
#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::entry::{Body, OrderWithdrawn, Withdrawal};
    use fleet_core::item::COULD_NOT_TELL;
    use fleet_core::store::{Item, Order, OrderKind, OrderState, Stamp, Status};
    use fleet_core::test_support::FakeStore;

    /// The seat's full id, which the order was assigned to and the withdrawal
    /// names, and its machine name, which a sentence says.
    const SEAT: &str = "018f6a2c-1d3e-7a4b-9c5d-00000c3a5e71";
    const LABEL: &str = "agent-0c3a5e71";
    /// This machine's identity, which the controller acts under.
    const MACHINE: &str = "018f6a2c-1d3e-7a4b-9c5d-0000a1b2c3d4";

    fn seat() -> SeatId {
        SeatId::parse(SEAT).expect("the seat's id parses")
    }

    /// The pass's own actor, as [`Engine::controller`] builds it.
    fn controller() -> Actor {
        Actor {
            kind: ActorKind::Controller,
            id: MACHINE.to_string(),
        }
    }
    const PARKED: &str = "fx-parked";

    /// The state the leak lives in: a run's item PARKED, so it is still open
    /// and the order naming the seat still stands when the cleanup retires it.
    fn a_parked_item() -> FakeStore {
        let store = FakeStore::default();
        store.seed(Item {
            id: PARKED.into(),
            title: String::from("an item a seat was dispatched and parked"),
            status: Status::Open,
            assignee: Some(seat()),
            order: OrderState::Ordered(Order {
                kind: OrderKind::Dispatch,
                by: Actor {
                    kind: ActorKind::Run,
                    id: String::from("a-run"),
                },
                seat: Some(seat()),
                at: Stamp::parse("2026-09-14T10:40:39Z").expect("a stamp"),
            }),
            ..Item::default()
        });
        store
    }

    #[test]
    fn the_cleanups_withdrawal_releases_the_parked_item_the_seat_holds() {
        let store = a_parked_item();

        let withdrawn = withdrawn_from(&store, &seat(), LABEL, &controller())
            .expect("the board answers and the withdrawal lands");
        assert_eq!(withdrawn, vec![PARKED.to_string()]);

        let after = store.show(PARKED).expect("the store answers");
        assert!(
            after.assignee.is_none(),
            "the item reads unassigned: {:?}",
            after.assignee
        );
        assert_eq!(
            after.order,
            OrderState::None,
            "and carries no order index: {}",
            after.proof.as_str()
        );
        assert_eq!(
            after.status,
            Status::Open,
            "the work itself is still to be done"
        );
        let entries = store
            .timeline(&ItemId::from(PARKED))
            .expect("the store answers the timeline");
        assert_eq!(
            entries
                .iter()
                .map(|entry| (&entry.body, &entry.by))
                .collect::<Vec<_>>(),
            vec![(
                &Body::OrderWithdrawn(OrderWithdrawn {
                    why: Withdrawal::Retire,
                    seat: Some(seat()),
                    cause: None,
                }),
                &controller(),
            )],
            "the withdrawal names the seat and says who took it, and the cleanup is the \
             controller"
        );
        assert!(
            store
                .wrote()
                .iter()
                .all(|line| line.ends_with(&format!(" controller:{MACHINE}"))),
            "every write is the controller's, typed: {:?}",
            store.wrote()
        );
    }

    /// A seat holding nothing ordered is retired as it always was: the board is
    /// read once and nothing is written.
    #[test]
    fn a_seat_holding_nothing_ordered_is_withdrawn_from_without_a_write() {
        let store = FakeStore::default();

        let withdrawn =
            withdrawn_from(&store, &seat(), LABEL, &controller()).expect("the board answers");

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
            unreadable: Some(String::from("the store is not on this path")),
            ..FakeStore::default()
        };

        let refused = withdrawn_from(&store, &seat(), LABEL, &controller())
            .expect_err("an unreadable board is a question");

        assert_eq!(refused.code, COULD_NOT_TELL);
        assert!(
            refused.message.contains("the store is not on this path"),
            "{}",
            refused.message
        );
    }

    /// [ASSUMES D8] The pass acts as the controller under THIS MACHINE'S
    /// identity: minted by the first act where the machine has none, and read
    /// back unchanged by every act after it.
    #[test]
    fn the_engine_acts_as_the_controller_under_this_machines_identity() {
        let dir = std::env::temp_dir().join(format!("fleet-runs-actor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let engine = Engine::on(dir.join("machine"));

        let first = engine.controller().expect("the identity is minted");
        let (identity, minted) =
            identity_or_mint(&dir.join("machine")).expect("the identity reads back");
        assert!(!minted, "the engine's first act minted it");
        assert_eq!(first.kind, ActorKind::Controller);
        assert_eq!(first.id, identity.id.to_string());
        assert_eq!(first.to_string(), format!("controller:{}", identity.id));
        assert_eq!(
            engine.controller().expect("the identity reads"),
            first,
            "every act after the first is the same controller"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The opener the binary runs on resolves every store's binary on the
    /// constructed child PATH — the search path the verbs hand the same opener
    /// — and never on the pass's own, which under a launchd service holds
    /// neither a package manager's prefix nor the user's local bin. That the
    /// opener runs what it resolved by absolute path is the store's own arm.
    #[test]
    fn the_engines_stores_hand_the_constructed_child_path() {
        assert_eq!(
            ProjectStores::resolved().search_path,
            platform::child_path(&platform::home_dir())
        );
    }
}
