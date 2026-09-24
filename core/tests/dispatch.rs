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
use fleet_core::store::{AssignedItem, Item, Store, StoreError};

const POLICY: &str = "[guards]\n";
/// The builder's gate every arm's order hands over, as a workflow would.
const TOUCHED: &str = "make check";
const BY: &str = "lead-1";
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
    /// The builder's gate each spawn was asked to write a rule for.
    touched: Mutex<Vec<Option<String>>>,
}

impl StubSpawner {
    fn answering(outcome: SpawnOutcome) -> StubSpawner {
        StubSpawner {
            outcome,
            calls: Mutex::new(Vec::new()),
            touched: Mutex::new(Vec::new()),
        }
    }
}

impl Spawner for StubSpawner {
    fn spawn(&self, ask: &Spawn) -> SpawnOutcome {
        self.calls
            .lock()
            .expect("not poisoned")
            .push(ask.first_turn.to_path_buf());
        self.touched
            .lock()
            .expect("not poisoned")
            .push(ask.touched.map(str::to_string));
        self.outcome.clone()
    }
}

/// The real store with one reading bent, so an arm can force the disagreement
/// the read-back exists to catch.
struct Doctored<'a> {
    inner: &'a dyn Store,
    assignee: Option<String>,
    /// The seat the order index reads back naming.
    seat: Option<String>,
    append: Option<String>,
}

impl Store for Doctored<'_> {
    fn ready(&self) -> Result<Vec<String>, StoreError> {
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
        if let (Some(seat), Some(index)) = (&self.seat, read.orders.as_mut()) {
            index.seat = Some(seat.clone());
        }
        if let Some(extra) = &self.append {
            read.document.push_str(extra);
        }
        Ok(read)
    }

    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        self.inner.show_text(item)
    }

    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError> {
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
        let policy = table_at(&fixture.path("fleet.toml"));
        let packs = Packs::under(
            &fixture.path("packs"),
            &fixture.path(fleet_core::defaults::DIR),
        )
        .expect("the defaults resolve");
        Rig {
            project: Project {
                root: graph.root().to_path_buf(),
                name: String::from("a-project"),
                guards: policy.clone(),
                policy,
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
                touched: Some(TOUCHED),
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
            r#"{"fleet.orders":{"v":1,"by":"someone","kind":"dispatch","at":"then"}}"#,
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

/// A `fleet.orders` at a version this binary does not know, or at none, is
/// present and unreadable — never absent, never an order — and a dispatch
/// could-not-tell over it with nothing written (fleet-4j6 AC4).
#[test]
fn a_fleet_orders_at_an_unknown_version_is_unreadable_and_dispatch_could_not_tell() {
    let rig = Rig::new("unversioned");
    for (label, payload) in [
        (
            "v2",
            r#"{"fleet.orders":{"v":2,"by":"a-newer-fleet","kind":"dispatch","at":"then"}}"#,
        ),
        (
            "no-v",
            r#"{"fleet.orders":{"by":"an-older-fleet","kind":"dispatch","at":"then"}}"#,
        ),
    ] {
        let item = rig.graph.item(&format!("an item ordered at {label}"));
        rig.graph
            .store()
            .set_orders(&item, payload, "another-fleet")
            .expect("the order index lands");
        let read = rig.graph.store().show(&item).expect("the item reads");
        assert!(
            read.has_orders_key && read.orders.is_none(),
            "{label}: present and unreadable: {:?} {}",
            read.orders,
            read.document
        );

        let before = rig.graph.json(&item);
        let seat = format!("s-unversioned-{label}");
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

        assert_eq!(answer.code, Some(3), "{label}: {}", answer.why);
        assert!(
            answer.why.contains("`fleet.orders` this fleet cannot read")
                && answer.why.contains("v 1"),
            "{label}: the key and the version this fleet reads are named: {}",
            answer.why
        );
        assert_eq!(
            rig.graph.json(&item),
            before,
            "{label}: the item is untouched"
        );
        assert!(ring.calls().is_empty(), "{label}: nobody is rung");
    }
    assert_eq!(rig.events.count(), 0, "a could-not-tell appends nothing");
}

/// Another writer's bare `orders`, of any shape, and the bare `run` label are
/// not fleet's: the item dispatches as an unordered one, and both are
/// byte-identical afterwards (fleet-4j6 AC1, the dispatch).
#[test]
fn another_writers_orders_key_and_run_label_are_neither_read_nor_moved() {
    let rig = Rig::new("foreign");
    let seat = String::from("s-foreign");
    let item = rig
        .graph
        .item("an item another tool keeps its own index on");
    rig.graph
        .store()
        .set_metadata(&item, common::FOREIGN_ORDERS, "another-tool")
        .expect("the other writer's key lands");
    rig.graph.label(&item, common::FOREIGN_LABEL);
    let before = common::foreign_of(rig.graph.store(), &item);
    let read = rig.graph.store().show(&item).expect("the item reads");
    assert!(
        !read.has_orders_key && read.orders.is_none(),
        "a bare `orders` reads as no order: {}",
        read.document
    );

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
    assert_eq!(
        index_of(&rig, &item).seat.as_deref(),
        Some(seat.as_str()),
        "fleet's own index names the seat"
    );
    assert_eq!(
        common::foreign_of(rig.graph.store(), &item),
        before,
        "the other writer's key and label are byte-identical"
    );
}

/// An epic is refused BY ITS TYPE and not by the ready list: the store calls an
/// open, unblocked epic ready, and a dispatch that trusted the list would hand
/// a seat the one kind of item nobody builds.
///
/// The transient path, because it is the one that starts a seat: a refusal
/// that came after the spawn would leave a session running on an epic.
#[test]
fn an_epic_is_refused_by_name_and_nothing_is_written() {
    let rig = Rig::new("epic");
    let item = rig
        .graph
        .store()
        .create(
            &fleet_core::store::NewItem {
                title: "an epic whose children are the work",
                description: "an epic",
                item_type: "epic",
                labels: &[],
            },
            "the-test",
        )
        .expect("the epic is filed");
    assert!(
        rig.graph
            .store()
            .ready()
            .expect("the ready read answers")
            .contains(&item),
        "the store calls the epic ready, so the ready check alone lets it through"
    );

    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    let before = rig.graph.json(&item);
    let wrote = board.store.wrote();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(
        answer.why.contains(&item)
            && answer
                .why
                .contains("an epic is never dispatched — its children are"),
        "the refusal names the epic and why: {}",
        answer.why
    );
    assert_eq!(board.store.wrote(), wrote, "the store was written nothing");
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
    assert!(
        spawner.calls.lock().expect("not poisoned").is_empty(),
        "no seat was started"
    );
    assert!(ring.calls().is_empty());
    assert!(
        !rig.briefs().join(format!("{item}.md")).exists(),
        "no brief was written"
    );
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

/// An order already on the item, as a dispatch would have left it.
fn ordered(rig: &Rig, item: &str, seat: &str) {
    rig.graph
        .store()
        .set_orders(
            item,
            &dispatch::index("someone", dispatch::KIND, Some(seat), "then"),
            "someone",
        )
        .expect("the order index lands");
}

#[test]
fn a_seat_already_holding_an_item_is_refused() {
    let rig = Rig::new("busy");
    let seat = String::from("s-busy");
    let claimed = rig.graph.item("the item this seat is already on");
    rig.graph.assign(&claimed, &seat);
    ordered(&rig, &claimed, &seat);
    rig.graph.status(&claimed, "in_progress");
    let given = rig
        .graph
        .item("a second item it was given and has not started");
    rig.graph.assign(&given, &seat);
    ordered(&rig, &given, &seat);
    let stale = rig.graph.item("an item assigned to it that nobody ordered");
    rig.graph.assign(&stale, &seat);

    let item = rig.graph.item("a third item nobody may give it");
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
    for (held, status) in [(&claimed, "in_progress"), (&given, "open")] {
        assert!(
            answer.why.contains(&format!("{held} ({status})")),
            "each ordered item it holds is named with its status: {}",
            answer.why
        );
    }
    assert!(
        !answer.why.contains(&stale),
        "an item nobody ordered is not a hold: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
}

/// The switch sitting's refusal (tinytown-tnkuq.23): a seat whose only
/// assigned items are a bug nobody ordered and an epic still naming it is
/// holding nothing, and a dispatch to it proceeds.
///
/// The epic CARRIES AN ORDER here, so what lets it through is its type and not
/// the missing key the bug is let through on.
#[test]
fn a_seat_assigned_only_unordered_work_and_an_epic_is_dispatched() {
    let rig = Rig::new("stale");
    let seat = String::from("s-stale");
    let bug = rig
        .graph
        .item("a bug assigned a month ago and never ordered");
    rig.graph.assign(&bug, &seat);
    let epic = rig
        .graph
        .item("an epic still carrying the seat as assignee");
    rig.graph.item_type(&epic, "epic");
    rig.graph.assign(&epic, &seat);
    ordered(&rig, &epic, &seat);

    let item = rig.graph.item("the item the seat is given now");
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
    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(read.assignee.as_deref(), Some(seat.as_str()));
    assert!(read.orders.is_some(), "the order is written");
    assert_eq!(ring.calls().len(), 1, "the seat is rung");
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
        seat: None,
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
    // THE REPAIR FOR THE FIELD THAT DISAGREED: a lost assignee is set again,
    // and handing it the index write instead would leave it lost.
    assert!(
        answer.why.contains(&format!(
            "RERUN: bd update {item} --assignee {seat} --actor {BY}"
        )),
        "the assignee's own repair: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("--metadata"),
        "and not the index's: {}",
        answer.why
    );
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

/// The index's own disagreement gets the index's own repair: the whole
/// `fleet.orders` object written again, and not the assignee's write.
#[test]
fn an_index_that_reads_back_wrong_is_handed_the_index_repair() {
    let rig = Rig::new("disagree-index");
    let item = rig.graph.item("a ready item whose index reads back wrong");
    let seat = String::from("s-disagree-index");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let bent = Doctored {
        inner: rig.graph.store(),
        assignee: None,
        seat: Some(String::from("somebody-else")),
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
        answer.why.contains(&format!(
            "{item} read back with fleet.orders.seat == somebody-else"
        )),
        "the field and the value read: {}",
        answer.why
    );
    assert!(
        answer.why.contains(&format!(
            "RERUN: bd update {item} --metadata '{}' --actor {BY}",
            dispatch::index(BY, "dispatch", Some(&seat), AT)
        )),
        "the index's own repair, versioned: {}",
        answer.why
    );
    assert!(
        answer.why.contains(r#""fleet.orders":{"#) && answer.why.contains(r#""v":1"#),
        "the repair writes fleet's key at its version: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("--assignee"),
        "and not the assignee's: {}",
        answer.why
    );
    assert!(ring.calls().is_empty(), "nobody is rung");
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
        seat: None,
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

// ---- an item named by part of its id -----------------------------------------

/// A SUFFIX IS RESOLVED ONCE, at the verb's entry, and every write after it
/// carries the id the store answered: the store resolves a partial id itself,
/// so a ready check against the typed text refused a ready item as not ready,
/// and a write under it would be a second spelling of the item.
#[test]
fn a_suffix_is_dispatched_under_the_full_id_it_resolves_to() {
    let rig = Rig::new("suffix");
    let item = rig.graph.item("an item named by its suffix");
    let suffix = item.strip_prefix("fx-").expect("the board files under fx-");
    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    board.forget_writes();
    let seat = String::from("s-suffix");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));

    let answer = rig.run(
        suffix,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, None, "{}", answer.why);

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(read.assignee.as_deref(), Some(seat.as_str()));
    assert_eq!(
        brief::order_line(read.notes.as_deref()).as_deref(),
        Some(note_for(&rig, BY).as_str()),
        "the order note is on the full id's item"
    );
    let index = read.orders.expect("the index is an object");
    assert_eq!(index.seat.as_deref(), Some(seat.as_str()));
    assert_eq!(index.at.as_deref(), Some(AT));

    let wrote = board.store.wrote();
    for verb in ["assign", "note", "set_orders"] {
        assert!(
            wrote
                .iter()
                .any(|line| line.starts_with(&format!("{verb} {item} "))),
            "{verb} is written under {item}: {wrote:?}"
        );
    }
    assert!(
        !wrote
            .iter()
            .any(|line| line.split(' ').nth(1) == Some(suffix)),
        "no write names the suffix: {wrote:?}"
    );

    let (_, payload) = rig.events.one(ITEM_DISPATCHED);
    assert_eq!(payload["item"], serde_json::json!(item));
    let calls = ring.calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0].1.starts_with(&format!("{item} is yours"))
            && calls[0].1.contains(&format!("{item}.md")),
        "the ring names the full id and its brief: {}",
        calls[0].1
    );
    assert!(rig.briefs().join(format!("{item}.md")).is_file());
}

/// An argument naming more than one item is refused with the ones it named,
/// read off `bd`'s own words — its JSON carries no more than "no issues found",
/// and the matches are on stderr. Through `bd`, because that shape is `bd`'s.
///
/// A parent and its child make the ambiguity certain whatever else the shared
/// board holds: all but the last character of the parent's hash is in both ids,
/// and too short to be anybody's whole hash.
#[test]
fn an_ambiguous_suffix_is_refused_naming_the_items_it_matches() {
    let rig = Rig::ringed("ambiguous");
    let parent = rig.graph.item("a parent a suffix matches");
    let out = rig.graph.bd(&[
        "create",
        "--title",
        "its child, which the same suffix matches",
        "--description",
        "a scratch item",
        "--type",
        "task",
        "--parent",
        &parent,
        "--json",
    ]);
    assert!(
        out.status.success(),
        "bd create --parent: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let child = fleet_core::store::first_value(&String::from_utf8_lossy(&out.stdout))
        .and_then(|value| value.get("id")?.as_str().map(str::to_string))
        .expect("the child is filed with an id");
    let hash = parent
        .strip_prefix("fx-")
        .expect("the board files under fx-");
    let suffix = &hash[..hash.len() - 1];

    let before = rig.graph.json(&parent);
    let seat = String::from("s-ambiguous");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        suffix,
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(
        answer
            .why
            .contains(&format!("`{suffix}` matches more than one item"))
            && answer.why.contains(&child)
            && answer.why.matches(parent.as_str()).count() >= 2,
        "the refusal names at least the parent and the child: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&parent), before, "the item is untouched");
    assert!(ring.calls().is_empty());
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

/// The board held in memory tells the same ambiguity the same way: it answers
/// `bd`'s JSON and `bd`'s stderr, and the one reading of both is the store's.
#[test]
fn the_board_in_memory_refuses_an_ambiguous_suffix_the_same_way() {
    let rig = Rig::new("ambiguous-memory");
    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    board
        .store
        .creates
        .lock()
        .expect("the queue is not poisoned")
        .extend([String::from("fx-63h"), String::from("fx-63u")]);
    let first = rig.graph.item("one item a suffix matches");
    let second = rig.graph.item("another item the same suffix matches");
    let wrote = board.store.wrote();

    let seat = String::from("s-ambiguous-memory");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        "63",
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(
        answer.why.contains("`63` matches more than one item")
            && answer.why.contains(&first)
            && answer.why.contains(&second),
        "the refusal names both: {}",
        answer.why
    );
    assert_eq!(board.store.wrote(), wrote, "the store was written nothing");
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

#[test]
fn a_suffix_nothing_matches_is_refused() {
    let rig = Rig::new("unmatched");
    rig.graph.item("an item the suffix does not match");
    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    let wrote = board.store.wrote();
    let seat = String::from("s-unmatched");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let answer = rig.run(
        "zzzz",
        Some(&seat),
        std::slice::from_ref(&seat),
        rig.graph.store(),
        &ring,
        &spawner,
    );

    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(answer.why.contains("zzzz"), "{}", answer.why);
    assert_eq!(board.store.wrote(), wrote, "the store was written nothing");
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
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
        // The builder's gate the order was handed reaches the spawn, which
        // writes the seat's rule for it.
        assert_eq!(
            spawner.touched.lock().expect("not poisoned").as_slice(),
            &[Some(TOUCHED.to_string())]
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
            Some(TOUCHED),
        )
        .expect("the brief renders");
        assert_eq!(written, printed, "byte for byte");
        assert!(
            String::from_utf8_lossy(&written).contains(&format!("```\n{TOUCHED}\n```")),
            "the brief names the builder's gate the order was handed"
        );
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
                touched: Some(TOUCHED),
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
