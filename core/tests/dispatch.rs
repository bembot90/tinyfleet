//! `fleet dispatch` (packs PRD R4, R9, R10, R20) against a real work graph.
//!
//! One store for the whole binary and one item per arm: `bd` serialises against
//! itself on this box, so a store per arm buys isolation at a price and no
//! speed. Each arm also takes its own SEAT name, because the seat-holds-an-item
//! read is a query across the whole store and two arms sharing a seat would be
//! reading each other's work.
//!
//! The read-back failures are forced through a store that answers something
//! other than what was written: it is the one failure a real store will not
//! produce on demand, and the verb's whole contract is surviving it.

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::{keys_agree, sweep_dead_stores, Fixture, Graph, Rooted, StubEvents};
use fleet_core::item::brief::{self, Packs, TRANSIENT};
use fleet_core::item::dispatch::{self, Order, Wiring, NOT_TOLD, WITHDRAWN};
use fleet_core::item::{
    control_token, render, table_at, Project, Ring, RingOutcome, Spawn, SpawnOutcome, Spawner,
    ITEM_DISPATCHED,
};
use fleet_core::store::{Item, Row, Store, StoreError};

const POLICY: &str = "[gates]\nsuite = \"make check\"\n";
const BY: &str = "architect-1";
const AT: &str = "2026-09-08T18:46:55Z";

// ---- the seams ---------------------------------------------------------------

struct StubRing {
    outcome: RingOutcome,
    calls: Mutex<Vec<(String, String)>>,
}

impl StubRing {
    fn answering(outcome: RingOutcome) -> StubRing {
        StubRing {
            outcome,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("not poisoned").clone()
    }
}

impl Ring for StubRing {
    fn ring(&self, seat: &str, text: &str) -> RingOutcome {
        self.calls
            .lock()
            .expect("not poisoned")
            .push((seat.to_string(), text.to_string()));
        self.outcome.clone()
    }
}

struct StubSpawner {
    outcome: SpawnOutcome,
    calls: Mutex<Vec<PathBuf>>,
}

impl StubSpawner {
    fn answering(outcome: SpawnOutcome) -> StubSpawner {
        StubSpawner {
            outcome,
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Spawner for StubSpawner {
    fn spawn(&self, ask: &Spawn) -> SpawnOutcome {
        self.calls
            .lock()
            .expect("not poisoned")
            .push(ask.first_turn.to_path_buf());
        self.outcome.clone()
    }
}

/// The real store with one reading bent, so an arm can force the disagreement
/// the read-back exists to catch.
struct Doctored<'a> {
    inner: &'a dyn Store,
    assignee: Option<String>,
    append: Option<String>,
}

impl Store for Doctored<'_> {
    fn ready(&self) -> Result<Vec<fleet_core::store::Ready>, StoreError> {
        self.inner.ready()
    }

    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        self.inner.open_labelled(label)
    }

    fn create(&self, item: &fleet_core::store::NewItem, by: &str) -> Result<String, StoreError> {
        self.inner.create(item, by)
    }

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_title(item, title, by)
    }

    fn show(&self, item: &str) -> Result<Item, StoreError> {
        let mut read = self.inner.show(item)?;
        if let Some(assignee) = &self.assignee {
            read.assignee = Some(assignee.clone());
        }
        if let Some(extra) = &self.append {
            read.document.push_str(extra);
        }
        Ok(read)
    }

    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        self.inner.show_text(item)
    }

    fn assigned_to(&self, seat: &str) -> Result<Vec<Row>, StoreError> {
        self.inner.assigned_to(seat)
    }

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.inner.assign(item, seat, by)
    }

    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError> {
        self.inner.note(item, text, by)
    }

    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_orders(item, payload, by)
    }

    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_metadata(item, payload, by)
    }

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.inner.unset_orders(item, by)
    }

    fn gate(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        self.inner.gate(item, reason, by)
    }

    fn open_gates(&self) -> Result<Vec<String>, StoreError> {
        self.inner.open_gates()
    }

    fn resolve_gate(&self, gate: &str, by: &str) -> Result<(), StoreError> {
        self.inner.resolve_gate(gate, by)
    }

    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        self.inner.close(item, reason, by)
    }

    fn export(&self, into: &std::path::Path) -> Result<(), StoreError> {
        self.inner.export(into)
    }
}

// ---- the rig -----------------------------------------------------------------

struct Rig {
    graph: Graph,
    fixture: Fixture,
    packs: Packs,
    project: Project,
    /// The stream this rig's runs append to. One per arm, because an arm that
    /// counted appends across a shared one would be counting another arm's.
    events: StubEvents,
}

struct Answer {
    code: Option<u8>,
    why: String,
    out: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        Rig::on(Graph::memory(label), label)
    }

    /// The same rig on the store `bd` answers: THE INTEGRATION RING of this
    /// suite, taken by one arm.
    fn ringed(label: &str) -> Rig {
        Rig::on(Graph::real("dispatch"), label)
    }

    fn on(graph: Graph, label: &str) -> Rig {
        let fixture = Fixture::new(label);
        fixture.file("fleet.toml", POLICY);
        fixture.materialize_defaults();
        let gates = table_at(&fixture.path("fleet.toml"));
        let packs = Packs::under(
            &fixture.path("packs"),
            &fixture.path(fleet_core::defaults::DIR),
        )
        .expect("the defaults resolve");
        Rig {
            project: Project {
                root: graph.root().to_path_buf(),
                name: String::from("a-project"),
                guards: gates.clone(),
                gates,
            },
            packs,
            fixture,
            graph,
            events: StubEvents::default(),
        }
    }

    fn briefs(&self) -> PathBuf {
        self.fixture.path("briefs")
    }

    fn run(
        &self,
        item: &str,
        to: Option<&str>,
        seats: &[String],
        store: &dyn Store,
        ring: &dyn Ring,
        spawner: &dyn Spawner,
    ) -> Answer {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let answer = dispatch::dispatch(
            &mut out,
            &mut err,
            &Order {
                item,
                to,
                by: BY,
                at: AT,
                brief: None,
                base: None,
                model: None,
            },
            &Wiring {
                store,
                project: &self.project,
                packs: &self.packs,
                briefs_dir: &self.briefs(),
                seats,
                ring,
                spawner,
                events: &self.events,
            },
        );
        Answer {
            code: answer.as_ref().err().map(|refused| refused.stop.code),
            why: answer
                .as_ref()
                .err()
                .map(|refused| refused.stop.message.clone())
                .unwrap_or_default(),
            out: String::from_utf8(out).expect("stdout is utf-8"),
        }
    }
}

/// The note the rig's own copy of the template renders for this dispatcher.
fn note_for(rig: &Rig, by: &str) -> String {
    let path = rig
        .fixture
        .path(fleet_core::defaults::DIR)
        .join(brief::DISPATCH_NOTE);
    let template = std::fs::read_to_string(&path).expect("the rig's dispatch note is readable");
    render(&template, &[("by", by)])
        .expect("the dispatch note renders")
        .trim()
        .to_string()
}

fn index_of(rig: &Rig, item: &str) -> fleet_core::store::Orders {
    rig.graph
        .store()
        .show(item)
        .expect("the item reads back")
        .orders
        .expect("the index is an object")
}

#[test]
fn a_named_dispatch_writes_the_assignee_the_note_and_the_index() {
    // THE INTEGRATION RING of this suite, and the one arm here that dispatches
    // through `bd`: the assignee, the note and the four index fields written
    // and read back through the store the verb actually talks to.
    let rig = Rig::ringed("named");
    let item = rig.graph.item("a ready item");
    let seat = String::from("s-named");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));

    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, None, "{}", answer.why);
    assert_eq!(answer.out, format!("{}\n", note_for(&rig, BY)));

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(read.assignee.as_deref(), Some(seat.as_str()));
    assert_eq!(
        brief::order_line(read.notes.as_deref()).as_deref(),
        Some(note_for(&rig, BY).as_str()),
        "the note is the template's line, byte for byte"
    );
    let index = read.orders.expect("the index is an object");
    assert_eq!(index.by.as_deref(), Some(BY));
    assert_eq!(index.kind.as_deref(), Some("dispatch"));
    assert_eq!(index.seat.as_deref(), Some(seat.as_str()));
    assert_eq!(index.at.as_deref(), Some(AT));

    // The one event. A NAMED SEAT CARRIES NO BASE: no worktree was cut, so
    // there is no commit this order started from.
    assert_eq!(rig.events.count(), 1, "exactly one event");
    let (actor, payload) = rig.events.one(ITEM_DISPATCHED);
    assert_eq!(actor, BY);
    keys_agree(ITEM_DISPATCHED, &payload, &["base", "role", "reason"]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["seat"], serde_json::json!(seat));

    let calls = ring.calls();
    assert_eq!(calls.len(), 1, "one ring, to the seat that was named");
    assert_eq!(calls[0].0, seat);
    assert!(calls[0].1.contains(&item), "{}", calls[0].1);
    assert!(
        calls[0].1.contains(
            &rig.briefs()
                .join(format!("{item}.md"))
                .display()
                .to_string()
        ),
        "the ring names the brief's path: {}",
        calls[0].1
    );
    assert!(rig.briefs().join(format!("{item}.md")).is_file());
}

/// The rig's copy of the template is rewritten, so the note is measured against
/// the file and never against a line that happens to match the shipped one.
#[test]
fn a_named_dispatch_writes_the_note_the_pack_file_renders() {
    let rig = Rig::new("variant");
    rig.fixture.file(
        &format!("{}/{}", fleet_core::defaults::DIR, brief::DISPATCH_NOTE),
        "a variant order from {by} — orders given\n",
    );
    let wanted = note_for(&rig, BY);
    assert_eq!(
        wanted,
        format!("a variant order from {BY} — orders given"),
        "the rig reads the rewritten file"
    );

    let item = rig.graph.item("a ready item under a variant note");
    let seat = String::from("s-variant");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, None, "{}", answer.why);
    assert_eq!(answer.out, format!("{wanted}\n"));

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(
        brief::order_line(read.notes.as_deref()).as_deref(),
        Some(wanted.as_str()),
        "the note is the rewritten template's line, byte for byte"
    );
}

#[test]
fn a_blocked_item_is_refused_and_nothing_is_written() {
    let rig = Rig::new("blocked");
    let item = rig.graph.item("an item behind a blocker");
    let blocker = rig.graph.item("the blocker");
    rig.graph.blocked_by(&item, &blocker);

    let before = rig.graph.json(&item);
    let seat = String::from("s-blocked");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(answer.why.contains(&blocker), "{}", answer.why);
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
    assert!(ring.calls().is_empty());
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

#[test]
fn an_item_already_ordered_is_refused_and_nothing_is_written() {
    let rig = Rig::new("ordered");
    let item = rig.graph.item("an item somebody already gave away");
    rig.graph
        .store()
        .set_orders(
            &item,
            r#"{"orders":{"by":"someone","kind":"dispatch","at":"then"}}"#,
            "someone",
        )
        .expect("the order index lands");

    let before = rig.graph.json(&item);
    let seat = String::from("s-ordered");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(
        answer.why.contains("already carries an order"),
        "{}",
        answer.why
    );
    assert!(
        answer.why.contains("someone"),
        "the standing order is named: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

#[test]
fn a_seat_the_machine_does_not_run_is_refused() {
    let rig = Rig::new("stranger");
    let item = rig.graph.item("a ready item");
    let before = rig.graph.json(&item);
    let known = vec![String::from("s-known")];
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));

    let answer = rig.run(
        &item,
        Some("s-stranger"),
        &known,
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(answer.why.contains("s-stranger"), "{}", answer.why);
    assert!(
        answer.why.contains("s-known"),
        "the seats it does run are named: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
}

#[test]
fn a_seat_already_holding_an_item_is_refused() {
    let rig = Rig::new("busy");
    let seat = String::from("s-busy");
    let held = rig.graph.item("the item this seat is already on");
    rig.graph.assign(&held, &seat);
    rig.graph.status(&held, "in_progress");

    let item = rig.graph.item("a second item nobody may give it");
    let before = rig.graph.json(&item);
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(
        answer.why.contains(&held),
        "the item it holds is named: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
}

#[test]
fn a_read_back_that_disagrees_exits_three_with_both_values() {
    let rig = Rig::new("disagree");
    let item = rig.graph.item("a ready item");
    let seat = String::from("s-disagree");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let bent = Doctored {
        inner: rig.graph.store(),
        assignee: Some(String::from("somebody-else")),
        append: None,
    };

    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        &bent,
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(3), "{}", answer.why);
    assert!(
        answer.why.contains("somebody-else"),
        "the value read: {}",
        answer.why
    );
    assert!(
        answer.why.contains(&seat),
        "the value wanted: {}",
        answer.why
    );
    assert!(answer.why.contains("RERUN: bd update"), "{}", answer.why);
    assert!(
        ring.calls().is_empty(),
        "nothing is rung on a read-back that disagrees"
    );
    assert_eq!(
        rig.events.count(),
        0,
        "the event follows the read-back, so a disagreement announces nothing"
    );

    // The control: the same store with nothing bent writes and reads back
    // clean, so the exit above is the disagreement and not the arm's setup.
    let clean = rig.graph.item("a second ready item");
    let seat = String::from("s-disagree-control");
    let answer = rig.run(
        &clean,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, None, "{}", answer.why);
}

#[test]
fn the_negative_control_catches_a_read_that_is_not_this_items() {
    let rig = Rig::new("control");
    let item = rig.graph.item("a ready item");
    let seat = String::from("s-control");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    // A read that answers yes to everything would satisfy every other
    // assertion. This one answers yes to a token nothing wrote.
    let bent = Doctored {
        inner: rig.graph.store(),
        assignee: None,
        append: Some(control_token().to_string()),
    };

    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        &bent,
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(3), "{}", answer.why);
    assert!(
        answer.why.contains("is not reading this item"),
        "{}",
        answer.why
    );
}

#[test]
fn a_ring_that_finds_no_live_session_leaves_the_order_standing() {
    let rig = Rig::new("absent");
    let item = rig.graph.item("a ready item");
    let seat = String::from("s-absent");
    let ring = StubRing::answering(RingOutcome::Absent);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));

    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(4), "{}", answer.why);
    assert!(
        answer.why.starts_with("ORDERED, NOT RUNG"),
        "{}",
        answer.why
    );
    assert_eq!(
        answer.out.len(),
        0,
        "the note line is stdout's on success only"
    );

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(read.assignee.as_deref(), Some(seat.as_str()));
    assert!(read.orders.is_some(), "the three writes stand");
    assert!(rig.briefs().join(format!("{item}.md")).is_file());
}

/// The shared store's sweep, over a root of its own: the system temp directory
/// holds other runs' stores, and those are not this arm's to judge.
#[test]
fn the_sweep_removes_a_dead_runs_store_and_keeps_a_live_ones() {
    let root = Fixture::new("sweep");
    let living = format!("fleet-store-sweep-{}-0", std::process::id());

    // A dead pid: a child of this process, reaped before its store is planted.
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("a child spawns");
    let gone = child.id();
    child.wait().expect("the child is reaped");
    let dead = format!("fleet-store-sweep-{gone}-0");
    let not_a_store = format!("fleet-pack-sweep-{gone}-0");

    root.dir(&living)
        .file(
            &format!("{dead}/.beads/config.yaml"),
            "a store's own file\n",
        )
        .dir(&not_a_store);

    sweep_dead_stores(&root.root);

    assert!(
        root.path(&living).is_dir(),
        "a live process's store is kept"
    );
    assert!(
        !root.path(&dead).exists(),
        "a reaped process's store is removed, contents and all"
    );
    assert!(
        root.path(&not_a_store).is_dir(),
        "a name that is not a store's is never touched"
    );
}

/// AC2 — the transient path, through the spawner seam. The live spawn is the
/// controller's and is not proven here.
mod transient {
    use super::*;

    #[test]
    fn a_spawned_seat_takes_the_assignee_the_index_and_the_brief() {
        let rig = Rig::new("spawned");
        let item = rig
            .graph
            .item("a ready item for a seat that does not exist yet");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::Spawned {
            seat: String::from("t1"),
            base: Some(String::from("0123456789abcdef0123456789abcdef01234567")),
            belt: None,
        });

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, None, "{}", answer.why);
        // THE NEGATIVE HALF of the belt pair below: this spawner read no belt,
        // and stdout is the order line and nothing under it.
        assert_eq!(answer.out, format!("{}\n", note_for(&rig, BY)));

        let read = rig.graph.store().show(&item).expect("the item reads back");
        assert_eq!(read.assignee.as_deref(), Some("t1"));
        let index = index_of(&rig, &item);
        assert_eq!(
            index.seat.as_deref(),
            Some("t1"),
            "the seat joins the index"
        );
        assert_eq!(index.kind.as_deref(), Some("dispatch"));
        assert!(
            ring.calls().is_empty(),
            "the spawn's own first turn IS the ring, so nothing is nudged"
        );

        // The brief the spawner was handed is the brief `fleet brief` prints,
        // byte for byte: one renderer, one file, two callers.
        let path = rig.briefs().join(format!("{item}.md"));
        assert_eq!(
            spawner.calls.lock().expect("not poisoned").as_slice(),
            std::slice::from_ref(&path)
        );
        // The one event, and the base the SPAWNER answered — the commit the
        // seat's own worktree was cut at (decision D1).
        assert_eq!(rig.events.count(), 1, "exactly one event");
        let (actor, payload) = rig.events.one(ITEM_DISPATCHED);
        assert_eq!(actor, BY, "the actor is the verb's own `by`");
        keys_agree(ITEM_DISPATCHED, &payload, &["role", "reason"]);
        assert_eq!(payload["item"], serde_json::json!(item));
        assert_eq!(payload["seat"], serde_json::json!("t1"));
        assert_eq!(
            payload["base"],
            serde_json::json!("0123456789abcdef0123456789abcdef01234567")
        );

        let written = std::fs::read(&path).expect("the brief is on disk");
        let mut printed: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        brief::for_item(
            &mut printed,
            &mut err,
            &rig.packs,
            &rig.project,
            rig.graph.store(),
            &item,
            TRANSIENT,
        )
        .expect("the brief renders");
        assert_eq!(written, printed, "byte for byte");
    }

    #[test]
    fn a_refused_spawn_withdraws_the_order_in_the_same_act() {
        let rig = Rig::new("refused");
        let item = rig.graph.item("a ready item nobody can be spawned for");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from(
            "fleet seat spawn is not built (controller slice 5)",
        )));

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, Some(1), "{}", answer.why);
        assert!(answer.why.contains("controller slice 5"), "{}", answer.why);
        assert!(answer.why.contains("withdrawn"), "{}", answer.why);

        let read = rig.graph.store().show(&item).expect("the item reads back");
        assert!(
            !read.has_orders_key,
            "no orders key survives a withdrawal: {:?}",
            read.orders
        );
        assert_eq!(read.assignee, None, "nobody was ever assigned");
        let notes = read.notes.unwrap_or_default();
        assert_eq!(
            notes.matches(WITHDRAWN).count(),
            1,
            "one withdrawal note, not two:\n{notes}"
        );
        assert!(
            notes.contains(&note_for(&rig, BY)),
            "the order note stays as history:\n{notes}"
        );
        assert_eq!(
            rig.events.count(),
            0,
            "a withdrawn order announces nothing: the event follows the seat"
        );
    }

    /// AC1 of the could-not-tell spec — a spawn nobody could OBSERVE is not a refusal, so
    /// the order it was given under is still there and the retry that could
    /// still succeed has one to succeed under.
    ///
    /// Read beside `a_refused_spawn_withdraws_the_order_in_the_same_act`: the
    /// two arms differ in the outcome alone and disagree on every write, which
    /// is the distinction the third variant exists for.
    #[test]
    fn a_spawn_that_could_not_be_told_leaves_the_order_standing_and_exits_3() {
        let rig = Rig::new("untold");
        let item = rig
            .graph
            .item("a ready item whose spawn nobody could observe");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::CouldNotTell(String::from(
            "the agent binary does not resolve",
        )));

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, Some(3), "{}", answer.why);
        assert!(
            answer.why.contains("the agent binary does not resolve"),
            "{}",
            answer.why
        );
        assert!(answer.why.contains("the order stands"), "{}", answer.why);

        let read = rig.graph.store().show(&item).expect("the item reads back");
        assert!(read.has_orders_key, "the order stands: {:?}", read.orders);
        let notes = read.notes.unwrap_or_default();
        assert_eq!(
            notes.matches(NOT_TOLD).count(),
            1,
            "one could-not-tell note, naming the cause:\n{notes}"
        );
        assert!(
            notes.contains(
                "DISPATCH COULD NOT TELL — the spawn could not be observed: the agent \
                            binary does not resolve"
            ),
            "the note names the cause:\n{notes}"
        );
        assert_eq!(
            notes.matches(WITHDRAWN).count(),
            0,
            "nothing was withdrawn:\n{notes}"
        );
        assert!(
            notes.contains(&note_for(&rig, BY)),
            "the order note stands:\n{notes}"
        );
        assert_eq!(
            rig.events.count(),
            0,
            "no seat came up, so nothing is announced"
        );
    }

    /// THE POSITIVE HALF of the belt pair. Read beside
    /// `a_spawned_seat_takes_the_assignee_the_index_and_the_brief`, whose
    /// spawner reads no belt and whose stdout is the order line alone: a verb
    /// that printed the legs unconditionally would pass this arm and red that
    /// one, and a verb that printed them never would pass that one and red this.
    ///
    /// The text is the SPAWNER'S, carried through unread: core measures no
    /// machine, so an arm that expected a shape here would be asserting on a
    /// sentence this crate does not write.
    #[test]
    fn the_belt_the_spawner_read_prints_under_the_order_line() {
        let rig = Rig::new("belted");
        let item = rig
            .graph
            .item("a ready item for a seat the belt let through");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let read = "  load average (5m)       : 0.10 (ceiling 8.00 = 8 cpu x 1.00)\n  \
                    transient seats mid-turn: 0 (cap 3)";
        let spawner = StubSpawner::answering(SpawnOutcome::Spawned {
            seat: String::from("t4"),
            base: None,
            belt: Some(read.to_string()),
        });

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, None, "{}", answer.why);
        assert_eq!(
            answer.out,
            format!("{}\n{read}\n", note_for(&rig, BY)),
            "the order line, then both legs, on the verb's own stdout"
        );
    }

    #[test]
    fn a_spawner_that_read_no_base_writes_the_key_absent_rather_than_a_guess() {
        let rig = Rig::new("baseless");
        let item = rig
            .graph
            .item("a ready item whose worktree would not answer");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::Spawned {
            seat: String::from("t2"),
            base: None,
            belt: None,
        });

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, None, "{}", answer.why);
        let (_, payload) = rig.events.one(ITEM_DISPATCHED);
        keys_agree(ITEM_DISPATCHED, &payload, &["base", "role", "reason"]);
    }

    #[test]
    fn a_stream_that_refuses_leaves_the_order_standing_and_exits_3() {
        let rig = Rig::new("streamless");
        let item = rig.graph.item("a ready item the stream will not take");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::Spawned {
            seat: String::from("t3"),
            base: None,
            belt: None,
        });

        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let events = StubEvents::refusing("the stream is read-only");
        let answer = dispatch::dispatch(
            &mut out,
            &mut err,
            &Order {
                item: &item,
                to: None,
                by: BY,
                at: AT,
                brief: None,
                base: None,
                model: None,
            },
            &Wiring {
                store: rig.graph.store(),
                project: &rig.project,
                packs: &rig.packs,
                briefs_dir: &rig.briefs(),
                seats: &[],
                ring: &ring,
                spawner: &spawner,
                events: &events,
            },
        );
        let stop = answer.err().expect("the append refused").stop;
        assert_eq!(stop.code, 3, "{}", stop.message);
        assert!(stop.message.contains("STANDS"), "{}", stop.message);

        let read = rig.graph.store().show(&item).expect("the item reads back");
        assert!(
            read.has_orders_key,
            "the order is on the record: the note precedes the event"
        );
    }
}
