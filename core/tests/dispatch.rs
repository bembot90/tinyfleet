//! `fleet dispatch` against a work graph held in memory, and through `Exec` on
//! the stub adapter's store for the ring.
//!
//! One store for the ring's arms and one item per arm. Each arm also takes its
//! own SEAT name, because the seat-holds-an-item read is a query across the
//! whole store and two arms sharing a seat would be reading each other's work.
//!
//! The read-back failures are forced through a store that answers something
//! other than what was written: it is the one failure a real store will not
//! produce on demand, and the verb's whole contract is surviving it.

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::{
    a_dispatch, agent, full, keys_agree, seat_id, signal, sweep_dead_stores, Fixture, Graph,
    Rooted, StubEvents,
};
use fleet_core::entry::{Body, Entry, OrderWithdrawn, Ordered, Timeline, Withdrawal};
use fleet_core::item::brief::{self, Packs, TRANSIENT};
use fleet_core::item::dispatch::{self, Order, Wiring, NOT_TOLD, WITHDRAWN};
use fleet_core::item::{
    control_token, table_at, Project, Ring, RingOutcome, Spawn, SpawnOutcome, Spawner, ITEM_ENTRY,
};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::{Directory, Kind, SeatId, SeatRef};
use fleet_core::store::{self, Filter, Item, OrderState, ReadProof, Store, StoreError};

const POLICY: &str = "[guards]\n";
/// The builder's checks every arm's order hands over, as a workflow would.
const TOUCHED: &str = "make check";
/// Who gives every order here: a lead's seat, in the typed form every write
/// carries.
const BY: &str = "seat:01a0d1f1-0aec-765f-9abe-0000001ead01";
const AT: &str = "2026-09-08T18:46:55Z";

/// Who gave an order already standing on an item, which is not [`BY`].
const SOMEONE: &str = "run:someone";

/// [`BY`], typed.
fn by() -> Actor {
    Actor::typed(BY).expect("typed").expect("a seat")
}

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
    /// The builder's checks each spawn was asked to write a rule for.
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
    assignee: Option<SeatId>,
    /// The seat the order index reads back naming.
    seat: Option<SeatId>,
    append: Option<String>,
    /// Whether the order's own write is refused, after the entry before it
    /// landed.
    refuse_order: bool,
}

impl Store for Doctored<'_> {
    fn resolve(&self, id: &str) -> Result<fleet_core::store::ItemId, StoreError> {
        self.inner.resolve(id)
    }

    fn list(
        &self,
        filter: &fleet_core::store::Filter,
    ) -> Result<Vec<fleet_core::store::ItemSummary>, StoreError> {
        self.inner.list(filter)
    }

    fn create(
        &self,
        item: &fleet_core::store::NewItem,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<fleet_core::store::ItemId, StoreError> {
        self.inner.create(item, by)
    }

    fn show(&self, item: &str) -> Result<Item, StoreError> {
        let mut read = self.inner.show(item)?;
        if let Some(assignee) = self.assignee {
            read.assignee = Some(assignee);
        }
        if let (Some(seat), OrderState::Ordered(index)) = (self.seat, &mut read.order) {
            index.seat = Some(seat);
        }
        // THE FAKE PLANTS THE TOKEN IN ITS PROOF: the read's own text, which is
        // what the negative control asks.
        if let Some(extra) = &self.append {
            read.proof = ReadProof::of(format!("{}{extra}", read.proof.as_str()));
        }
        Ok(read)
    }

    fn update(
        &self,
        id: &fleet_core::store::ItemId,
        change: &fleet_core::store::Update,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.update(id, change, by)
    }

    fn order_set(
        &self,
        id: &fleet_core::store::ItemId,
        order: &fleet_core::store::Order,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        if self.refuse_order {
            return Err(StoreError::Unreadable(String::from(
                "the store did not answer the order's write",
            )));
        }
        self.inner.order_set(id, order, by)
    }

    fn order_withdraw(
        &self,
        id: &fleet_core::store::ItemId,
        fence: &fleet_core::store::WithdrawFence,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.order_withdraw(id, fence, by)
    }

    fn run_set(
        &self,
        id: &fleet_core::store::ItemId,
        run: &fleet_core::store::RunRecord,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.run_set(id, run, by)
    }

    fn hold_raise(
        &self,
        id: &fleet_core::store::ItemId,
        reason: &str,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<fleet_core::store::HoldId, StoreError> {
        self.inner.hold_raise(id, reason, by)
    }

    fn holds_open(&self) -> Result<Vec<fleet_core::store::HoldId>, StoreError> {
        self.inner.holds_open()
    }

    fn hold_clear(
        &self,
        hold: &fleet_core::store::HoldId,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.hold_clear(hold, by)
    }

    fn close(
        &self,
        id: &fleet_core::store::ItemId,
        reason: &str,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.close(id, reason, by)
    }

    fn append(
        &self,
        item: &fleet_core::store::ItemId,
        body: &fleet_core::entry::Body,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<String, StoreError> {
        self.inner.append(item, body, by)
    }

    fn timeline(
        &self,
        item: &fleet_core::store::ItemId,
    ) -> Result<Vec<fleet_core::entry::Entry>, StoreError> {
        self.inner.timeline(item)
    }

    fn capabilities(&self) -> Result<fleet_core::store::types::Capabilities, StoreError> {
        self.inner.capabilities()
    }

    fn version(&self) -> Result<fleet_core::store::Version, StoreError> {
        self.inner.version()
    }

    fn export(&self, into: &std::path::Path) -> Result<std::path::PathBuf, StoreError> {
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
    err: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        Rig::on(Graph::memory(label), label)
    }

    /// The same rig on the store the stub adapter keeps, through `Exec`: THE
    /// INTEGRATION RING of this suite.
    fn ringed(label: &str) -> Rig {
        Rig::on(Graph::exec("dispatch"), label)
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
        // Every arm names its seats by name, which is how a `--to` names one;
        // each is keyed by an id of its own, as the machine's rows are, and
        // each runs here and is listed.
        let running: Vec<SeatRef> = seats.iter().map(|name| agent(name)).collect();
        let directory = Directory {
            listed: running.clone(),
            running,
        };
        self.run_in(item, to, &directory, store, ring, spawner)
    }

    /// The same run over a directory the arm lays out itself.
    fn run_in(
        &self,
        item: &str,
        to: Option<&str>,
        seats: &Directory,
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
                by: &by(),
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
            err: String::from_utf8(err).expect("stderr is utf-8"),
        }
    }
}

/// THE STORE IS HANDED THE TYPED ACTOR. A dispatch by `seat:<id>` hands the
/// adapter `seat:<id>` as the `by` of every write it makes of the item — read
/// off the log the stub keeps of every write it is handed, since no read of the
/// contract answers who asked for a write — the index read back carries the
/// same actor, and the ordered entry's author is the same actor.
#[test]
fn a_dispatch_by_a_seat_hands_the_store_the_seats_typed_actor() {
    let rig = Rig::ringed("audit");
    let Graph::Exec(scratch) = &rig.graph else {
        unreachable!("the ring is the store through Exec");
    };
    let item = rig.graph.item("an item a seat dispatches");
    let answer = rig.run(
        &item,
        Some("orla"),
        &[String::from("Orla")],
        rig.graph.store(),
        &StubRing::answering(RingOutcome::Delivered),
        &StubSpawner::answering(SpawnOutcome::Refused(String::from("unused"))),
    );
    assert_eq!(answer.code, None, "{}", answer.why);

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert!(
        matches!(&read.order, OrderState::Ordered(index) if index.by == by()),
        "the index read back names the dispatcher: {:?}",
        read.order
    );
    let entries = timeline_of(&rig, &item);
    assert_eq!(
        entries.iter().map(|entry| &entry.by).collect::<Vec<_>>(),
        vec![&by()],
        "the ordered entry is the typed actor's: {entries:?}"
    );

    let writes: Vec<String> = scratch
        .rig(|store| store.wrote())
        .into_iter()
        .filter(|line| line.split(' ').nth(1) == Some(item.as_str()))
        .collect();
    assert!(
        writes.len() >= 2,
        "the assignee and the index are each a write: {writes:?}"
    );
    assert!(
        writes.iter().all(|line| line.ends_with(&format!(" {BY}"))),
        "the actor handed with every write the dispatch made is {BY}: {writes:?}"
    );
}

/// The item's entries, read back through the store the arm dispatched into.
fn timeline_of(rig: &Rig, item: &str) -> Vec<Entry> {
    rig.graph
        .store()
        .timeline(&store::ItemId::from(item))
        .expect("the timeline reads back")
}

/// The entry a dispatch appends: the order, to `seat` or to no seat yet.
fn ordered_to(seat: Option<SeatId>) -> Body {
    Body::Ordered(Ordered {
        order: store::OrderKind::Dispatch,
        seat,
    })
}

/// The line a dispatch prints on success, for an order to the seat a sentence
/// names `seat` or — `None` — to a transient one, under the entry recording it.
fn order_line(item: &str, seat: Option<&str>, entry: &str) -> String {
    match seat {
        Some(seat) => format!("ordered {item} to {seat} — entry {entry}"),
        None => format!("ordered {item} to a transient seat — entry {entry}"),
    }
}

fn index_of(rig: &Rig, item: &str) -> store::Order {
    ordered_index(rig.graph.store().show(item).expect("the item reads back"))
}

/// The order a read's index holds, where it holds one this fleet reads.
fn ordered_index(read: Item) -> store::Order {
    match read.order {
        OrderState::Ordered(index) => index,
        other => panic!("the index reads as an order, not {other:?}"),
    }
}

/// The order a dispatch by [`BY`] at [`AT`] writes, to `seat` or to no seat
/// yet, as the index reads it.
fn wanted_index(seat: Option<&str>) -> store::Order {
    store::Order {
        kind: store::OrderKind::Dispatch,
        by: by(),
        seat: seat.map(seat_id),
        at: store::Stamp::parse(AT).expect("a stamp"),
    }
}

#[test]
fn a_named_dispatch_writes_the_assignee_the_ordered_entry_and_the_index() {
    // THE INTEGRATION RING of this suite, and the arm here that dispatches
    // through `Exec`: the assignee, the ordered entry and the four index fields
    // written and read back through an adapter out of process, over the
    // contract's JSON.
    //
    // THE ORDER NAMES THE SEAT BY ITS ID. `--to orla` is how a person names
    // Orla, and the assignee, the index's seat, the event and the ring all
    // carry her full id: a name is hers to change, and work keyed by it would
    // move with the rename or strand under the old one.
    let rig = Rig::ringed("named");
    let item = rig.graph.item("a ready item");
    let orla = full("Orla");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));

    let answer = rig.run(
        &item,
        Some("orla"),
        &[String::from("Orla")],
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, None, "{}", answer.why);
    let seat = orla;

    // ONE ENTRY, THE ORDER, by the dispatcher and naming her id.
    let entries = timeline_of(&rig, &item);
    assert_eq!(entries.len(), 1, "one entry: {entries:?}");
    assert_eq!(entries[0].body, ordered_to(Some(seat_id("Orla"))));
    assert_eq!(entries[0].by, by());
    assert_eq!(
        answer.out,
        format!(
            "{}\n",
            order_line(&item, Some(&agent("Orla").machine_name()), &entries[0].id)
        ),
        "stdout names the item, the seat as a sentence does, and the entry"
    );

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(read.assignee, Some(seat_id("Orla")));
    assert_eq!(ordered_index(read), wanted_index(Some("Orla")));

    // The one event: the ordered entry's signal, by the dispatcher, naming the
    // entry the timeline holds. The seat is the entry's and the index's.
    assert_eq!(
        rig.events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            BY.to_string(),
            signal(&item, &entries[0].id, "ordered"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &rig.events.all()[0].2, &[]);

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
    // The brief a person and the seat read names her as a sentence does: by
    // her machine name, not by the id the record carries.
    let brief = std::fs::read_to_string(rig.briefs().join(format!("{item}.md")))
        .expect("the brief is on disk");
    assert!(
        brief.contains(&format!("You are `orla-{}`", seat_id("Orla").short())),
        "{brief}"
    );
}

/// `--to` names a seat this machine RUNS. A person is listed in the fleet and
/// runs nowhere, so an order to one is refused before anything is written, as
/// an argument two running seats both answer to is.
#[test]
fn a_person_and_an_ambiguous_name_are_refused_before_any_write() {
    let rig = Rig::new("not-running");
    let item = rig.graph.item("a ready item nobody runnable is named for");
    let before = rig.graph.json(&item);
    let alberto = SeatRef {
        id: seat_id("Alberto"),
        name: Some(String::from("Alberto")),
        kind: Kind::Human,
    };
    let (orla, other) = (agent("Orla"), agent("Orla the second"));
    let directory = Directory {
        listed: vec![alberto, orla.clone(), other.clone()],
        running: vec![orla.clone(), other.clone()],
    };
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));

    let answer = rig.run_in(
        &item,
        Some("alberto"),
        &directory,
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(1), "{}", answer.why);
    assert!(
        answer.why.starts_with("alberto names no seat"),
        "the human is not a running seat: {}",
        answer.why
    );

    // Two running seats answer to one name: exit 1, naming both.
    let mut twice = directory.clone();
    twice.running[1].name = Some(String::from("orla"));
    let answer = rig.run_in(
        &item,
        Some("orla"),
        &twice,
        rig.graph.store(),
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(1), "{}", answer.why);
    for seat in [&orla, &other] {
        assert!(
            answer.why.contains(&seat.id.to_string()),
            "both candidates are named: {}",
            answer.why
        );
    }
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
    assert!(ring.calls().is_empty(), "nobody is rung");
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

/// The named dispatch, counted in writes: the assignee, ONE ordered entry and
/// the index, and not a note among them — the order is the entry, and the
/// index beside it is the projection every verb decides on.
#[test]
fn a_named_dispatch_appends_one_ordered_entry_and_writes_no_note() {
    let rig = Rig::new("entry");
    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    let item = rig.graph.item("a ready item whose order is an entry");
    board.forget_writes();
    let seat = String::from("s-entry");
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

    let entries = timeline_of(&rig, &item);
    assert_eq!(
        entries.iter().map(|entry| &entry.body).collect::<Vec<_>>(),
        vec![&ordered_to(Some(seat_id(&seat)))],
        "the timeline is the one order, naming the seat's id"
    );
    assert_eq!(entries[0].by, by(), "by the dispatcher's actor");
    assert_eq!(index_of(&rig, &item), wanted_index(Some(&seat)));

    let wrote = board.store.wrote();
    assert!(
        !wrote.iter().any(|line| line.starts_with("note ")),
        "no note is written: {wrote:?}"
    );
    let verbs: Vec<&str> = wrote
        .iter()
        .filter_map(|line| line.split(' ').next())
        .collect();
    assert_eq!(
        verbs,
        ["update", "append", "order_set"],
        "the assignee, then the entry, then the order: {wrote:?}"
    );
    assert_eq!(
        answer.out,
        format!(
            "{}\n",
            order_line(&item, Some(&agent(&seat).machine_name()), &entries[0].id)
        )
    );
}

/// A store that takes the append and does not keep it: the entry's own
/// read-back is what catches it, and the stop names the entry it could not
/// find. Without that read-back the dispatch would go on to the index and read
/// back a disagreement about something else.
#[test]
fn an_ordered_entry_the_store_does_not_keep_exits_three_naming_it() {
    let rig = Rig::new("unkept");
    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    let item = rig
        .graph
        .item("a ready item whose store forgets its writes");
    board.store.ignore_writes();
    let seat = String::from("s-unkept");
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
    assert_eq!(answer.code, Some(3), "{}", answer.why);
    assert!(
        answer.why.contains("does not hold the ordered entry"),
        "the stop names the ordered entry the timeline does not hold: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("order index"),
        "and it stops at the entry, before the index: {}",
        answer.why
    );
    assert!(ring.calls().is_empty(), "nobody is rung");
    assert_eq!(rig.events.count(), 0, "and nothing is announced");
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
    rig.graph.order(&item, &a_dispatch(SOMEONE, None, AT));

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
        answer.why.contains(SOMEONE),
        "the standing order is named: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

/// An order index this fleet cannot read is present and unreadable — never
/// absent, never an order it can weigh — and a dispatch could-not-tell over
/// it, naming the version it reads, with nothing written (fleet-4j6 AC4).
///
/// Seeded as the contract reads one. Which of a store's own shapes read so —
/// an `at` that is no stamp, a version this fleet does not know or none — is
/// the adapter's reading, and bd's unit tests hold it.
#[test]
fn an_order_index_that_does_not_read_is_unreadable_and_dispatch_says_could_not_tell() {
    let rig = Rig::new("unreadable-order");
    let item = rig.graph.item("an item whose order index does not read");
    let Graph::Memory(board) = &rig.graph else {
        unreachable!("the rig is in memory");
    };
    board.amend(&item, |held| held.order = OrderState::Unreadable);
    let read = rig.graph.store().show(&item).expect("the item reads");
    assert_eq!(
        read.order,
        OrderState::Unreadable,
        "present and unreadable: {}",
        read.proof.as_str()
    );

    let before = rig.graph.json(&item);
    let seat = String::from("s-unreadable");
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

    assert_eq!(answer.code, Some(3), "{}", answer.why);
    assert!(
        answer
            .why
            .contains("order index is not one this fleet can read")
            && answer.why.contains("v 1"),
        "the index and the version this fleet reads are named: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&item), before, "the item is untouched");
    assert!(ring.calls().is_empty(), "nobody is rung");
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
    rig.graph.set_metadata(&item, common::FOREIGN_ORDERS);
    rig.graph.label(&item, common::FOREIGN_LABEL);
    let before = common::foreign_of(rig.graph.store(), &item);
    let read = rig.graph.store().show(&item).expect("the item reads");
    assert_eq!(
        read.order,
        OrderState::None,
        "a bare `orders` reads as no order: {}",
        read.proof.as_str()
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
        index_of(&rig, &item).seat,
        Some(seat_id(&seat)),
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
                title: String::from("an epic whose children are the work"),
                description: String::from("an epic"),
                item_type: String::from("epic"),
                labels: Vec::new(),
                priority: None,
            },
            &fleet_core::test_support::the_test(),
        )
        .expect("the epic is filed")
        .to_string();
    assert!(
        rig.graph
            .store()
            .list(&Filter::Ready)
            .expect("the ready read answers")
            .iter()
            .any(|row| row.id == item),
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
    rig.graph.order(item, &a_dispatch(SOMEONE, Some(seat), AT));
}

#[test]
fn a_seat_already_holding_an_item_is_refused() {
    let rig = Rig::new("busy");
    let seat = String::from("s-busy");
    let claimed = rig.graph.item("the item this seat is already on");
    rig.graph.hand_to(&claimed, &full(&seat));
    ordered(&rig, &claimed, &full(&seat));
    rig.graph.status(&claimed, "in_progress");
    let given = rig
        .graph
        .item("a second item it was given and has not started");
    rig.graph.hand_to(&given, &full(&seat));
    ordered(&rig, &given, &full(&seat));
    let stale = rig.graph.item("an item assigned to it that nobody ordered");
    rig.graph.hand_to(&stale, &full(&seat));

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
    rig.graph.hand_to(&bug, &full(&seat));
    let epic = rig
        .graph
        .item("an epic still carrying the seat as assignee");
    rig.graph.item_type(&epic, "epic");
    rig.graph.hand_to(&epic, &full(&seat));
    ordered(&rig, &epic, &full(&seat));

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
    assert_eq!(read.assignee, Some(seat_id(&seat)));
    assert!(
        matches!(read.order, OrderState::Ordered(_)),
        "the order is written"
    );
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
        assignee: Some(seat_id("somebody-else")),
        seat: None,
        append: None,
        refuse_order: false,
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
        answer.why.contains(&full("somebody-else")),
        "the value read: {}",
        answer.why
    );
    assert!(
        answer.why.contains(&full(&seat)),
        "the value wanted: {}",
        answer.why
    );
    // THE REPAIR IS A READ, and fleet's own: the line names the item to read
    // with fleet's verb, never a store's command to type.
    assert!(
        answer
            .why
            .contains(&format!("\n  READ: fleet item show {item}")),
        "the read that shows what the item holds: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("RERUN") && !answer.why.contains("--assignee"),
        "and no write to make again: {}",
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

/// The index's own disagreement names the field that disagreed and the read
/// that shows the item, and never a write to make again: the command that
/// writes an order is the store's, and fleet does not print a store's command.
#[test]
fn an_index_that_reads_back_wrong_names_the_field_and_the_read() {
    let rig = Rig::new("disagree-index");
    let item = rig.graph.item("a ready item whose index reads back wrong");
    let seat = String::from("s-disagree-index");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let bent = Doctored {
        inner: rig.graph.store(),
        assignee: None,
        seat: Some(seat_id("somebody-else")),
        append: None,
        refuse_order: false,
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
            "{item} read back with order.seat == {}",
            full("somebody-else")
        )),
        "the field and the value read: {}",
        answer.why
    );
    assert!(
        answer
            .why
            .contains(&format!("\n  READ: fleet item show {item}")),
        "the read that shows what the item holds: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("RERUN") && !answer.why.contains("set the order"),
        "and no write to make again: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("fleet.orders") && !answer.why.contains(r#""v":1"#),
        "and no storage shape — which key a store keeps the order under is its \
         adapter's: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("--assignee"),
        "and not the assignee's: {}",
        answer.why
    );
    assert!(ring.calls().is_empty(), "nobody is rung");
}

/// An order the store will not take, after the entry before it landed, says
/// the entry STANDS and names the read that shows the item — and no write to
/// make again, which would be the store's command and not fleet's.
#[test]
fn an_order_the_store_does_not_take_names_the_read_and_what_stands() {
    let rig = Rig::new("unordered");
    let item = rig.graph.item("a ready item whose order is not taken");
    let seat = String::from("s-unordered");
    let ring = StubRing::answering(RingOutcome::Delivered);
    let spawner = StubSpawner::answering(SpawnOutcome::Refused(String::from("unused")));
    let refusing = Doctored {
        inner: rig.graph.store(),
        assignee: None,
        seat: None,
        append: None,
        refuse_order: true,
    };

    let answer = rig.run(
        &item,
        Some(&seat),
        std::slice::from_ref(&seat),
        &refusing,
        &ring,
        &spawner,
    );
    assert_eq!(answer.code, Some(3), "{}", answer.why);
    assert!(
        answer.why.contains(&format!(
            "the order did not land: the store did not answer the order's write\n  READ: fleet \
             item show {item}\n  the ordered entry on {item} STANDS"
        )),
        "the cause, the read and what stands: {}",
        answer.why
    );
    assert!(
        !answer.why.contains("RERUN") && !answer.why.contains("set the order"),
        "and no write to make again: {}",
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
        refuse_order: false,
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
        "the order line is stdout's on success only"
    );

    let read = rig.graph.store().show(&item).expect("the item reads back");
    assert_eq!(read.assignee, Some(seat_id(&seat)));
    assert!(
        matches!(read.order, OrderState::Ordered(_)),
        "the three writes stand"
    );
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
    assert_eq!(read.assignee, Some(seat_id(&seat)));
    assert_eq!(
        timeline_of(&rig, &item)
            .iter()
            .map(|entry| &entry.body)
            .collect::<Vec<_>>(),
        vec![&ordered_to(Some(seat_id(&seat)))],
        "the ordered entry is on the full id's item"
    );
    assert_eq!(ordered_index(read), wanted_index(Some(&seat)));

    let wrote = board.store.wrote();
    for verb in ["update", "append", "order_set"] {
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

    let (_, payload) = rig.events.one(ITEM_ENTRY);
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
/// carried through `Exec` from the adapter's typed refusal: its reason and the
/// ids it names as candidates, which the refusal reads back out.
///
/// The two items are filed under ids of the arm's choosing, queued and filed
/// under one hold of the stub's lock, so the ambiguity is certain whatever
/// else the shared store holds: the suffix opens both hashes and is longer
/// than any hash the stub mints of its own.
#[test]
fn an_ambiguous_suffix_is_refused_naming_the_items_it_matches() {
    let rig = Rig::ringed("ambiguous");
    let Graph::Exec(scratch) = &rig.graph else {
        unreachable!("the ring is the store through Exec");
    };
    let (first, second) = scratch.rig(|stub| {
        stub.creates
            .lock()
            .expect("the queue is not poisoned")
            .extend([String::from("fx-ambq1"), String::from("fx-ambq2")]);
        let filed = |title: &str| {
            stub.create(
                &store::NewItem {
                    title: title.to_string(),
                    description: String::from("a scratch item"),
                    item_type: String::from("task"),
                    ..store::NewItem::default()
                },
                &fleet_core::test_support::the_test(),
            )
            .expect("the item is filed")
            .to_string()
        };
        (
            filed("one item a suffix matches"),
            filed("another item the same suffix matches"),
        )
    });
    assert_eq!(
        (first.as_str(), second.as_str()),
        ("fx-ambq1", "fx-ambq2"),
        "the store files under the ids queued"
    );
    let suffix = "ambq";

    let before = rig.graph.json(&first);
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
            .contains(&format!("{suffix} matches more than one item"))
            && answer.why.contains(&first)
            && answer.why.contains(&second),
        "the refusal names both items: {}",
        answer.why
    );
    assert_eq!(rig.graph.json(&first), before, "the item is untouched");
    assert!(ring.calls().is_empty());
    assert_eq!(rig.events.count(), 0, "a refusal appends nothing");
}

/// The board held in memory tells the same ambiguity in the same words: the
/// text refused, and every item it names.
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
            seat: full("t1"),
            belt: None,
        });

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, None, "{}", answer.why);

        // TWO ORDERED ENTRIES: the order before the spawn, to no seat — the
        // brief is the first turn — and the one naming the seat the spawn made.
        let entries = timeline_of(&rig, &item);
        assert_eq!(
            entries.iter().map(|entry| &entry.body).collect::<Vec<_>>(),
            vec![&ordered_to(None), &ordered_to(Some(seat_id("t1")))],
            "the order, then the seat that holds it"
        );
        assert!(entries.iter().all(|entry| entry.by == by()));
        let (_, current) = Timeline(&entries)
            .current_order()
            .expect("the order stands");
        assert_eq!(
            current.seat,
            Some(seat_id("t1")),
            "the current order's seat"
        );
        // THE NEGATIVE HALF of the belt pair below: this spawner read no belt,
        // and stdout is the order line and nothing under it, naming the entry
        // that seated the order.
        assert_eq!(
            answer.out,
            format!("{}\n", order_line(&item, None, &entries[1].id))
        );

        let read = rig.graph.store().show(&item).expect("the item reads back");
        assert_eq!(read.assignee, Some(seat_id("t1")));
        assert_eq!(
            index_of(&rig, &item),
            wanted_index(Some("t1")),
            "the seat joins the index"
        );
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
        // The builder's checks the order was handed reach the spawn, which
        // writes the seat's rule for them.
        assert_eq!(
            spawner.touched.lock().expect("not poisoned").as_slice(),
            &[Some(TOUCHED.to_string())]
        );
        // The one event: the signal of the ordered entry that SEATED the
        // order, the second, by the verb's own `by` — and none for the first,
        // which named no seat.
        assert_eq!(
            rig.events.all(),
            vec![(
                ITEM_ENTRY.to_string(),
                BY.to_string(),
                signal(&item, &entries[1].id, "ordered"),
            )],
            "exactly one event, the seated order's signal"
        );
        keys_agree(ITEM_ENTRY, &rig.events.all()[0].2, &[]);

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
            "the brief names the builder's checks the order was handed"
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
        assert_eq!(
            read.order,
            OrderState::None,
            "no order index survives a withdrawal"
        );
        assert_eq!(read.assignee, None, "nobody was ever assigned");
        // The order stays as history, and the withdrawal after it says why.
        let entries = timeline_of(&rig, &item);
        assert_eq!(
            entries.iter().map(|entry| &entry.body).collect::<Vec<_>>(),
            vec![
                &ordered_to(None),
                &Body::OrderWithdrawn(OrderWithdrawn {
                    why: Withdrawal::SpawnRefused,
                    seat: None,
                    cause: Some(String::from(
                        "fleet seat spawn is not built (controller slice 5)"
                    )),
                }),
            ],
            "the order, then its withdrawal naming the cause"
        );
        assert!(entries.iter().all(|entry| entry.by == by()));
        assert!(Timeline(&entries).current_order().is_none());
        assert_eq!(
            answer.err,
            format!("withdrawn: {WITHDRAWN}: fleet seat spawn is not built (controller slice 5)\n"),
            "the withdrawal's words are stderr's"
        );
        assert_eq!(
            rig.events.count(),
            0,
            "a withdrawn order announces nothing: the event follows the seat"
        );
    }

    /// A spawner that answers something no seat id parses from has handed back
    /// a seat no record can key on: the verb stops at could-not-tell before it
    /// assigns or announces anything, and the order it wrote stands.
    #[test]
    fn a_spawned_seat_that_is_no_seat_id_is_could_not_tell_before_the_assignee() {
        let rig = Rig::new("no-id");
        let item = rig.graph.item("a ready item whose spawner answers a name");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::Spawned {
            seat: String::from("agent-1d0e4f58"),
            belt: None,
        });

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, Some(3), "{}", answer.why);
        assert!(answer.why.contains("`agent-1d0e4f58`"), "{}", answer.why);

        let read = rig.graph.store().show(&item).expect("the item reads back");
        assert_eq!(read.assignee, None, "nobody was assigned");
        assert!(
            matches!(read.order, OrderState::Ordered(_)),
            "the order stands: {:?}",
            read.order
        );
        assert_eq!(rig.events.count(), 0, "and nothing was announced");
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
        assert!(
            matches!(read.order, OrderState::Ordered(_)),
            "the order stands: {:?}",
            read.order
        );
        // THE ORDER AND NOTHING AFTER IT: no withdrawal, and no entry for what
        // could not be observed — the cause is the exit's message.
        assert_eq!(
            timeline_of(&rig, &item)
                .iter()
                .map(|entry| &entry.body)
                .collect::<Vec<_>>(),
            vec![&ordered_to(None)],
            "the order stands alone"
        );
        assert_eq!(
            answer.err,
            format!("could not tell: {NOT_TOLD}: the agent binary does not resolve\n"),
            "the could-not-tell's words are stderr's"
        );
        assert!(!answer.err.contains(WITHDRAWN), "{}", answer.err);
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
            seat: full("t4"),
            belt: Some(read.to_string()),
        });

        let answer = rig.run(&item, None, &[], rig.graph.store(), &ring, &spawner);
        assert_eq!(answer.code, None, "{}", answer.why);
        let entries = timeline_of(&rig, &item);
        assert_eq!(
            answer.out,
            format!("{}\n{read}\n", order_line(&item, None, &entries[1].id)),
            "the order line, then both legs, on the verb's own stdout"
        );
    }

    #[test]
    fn a_stream_that_refuses_leaves_the_order_standing_and_exits_3() {
        let rig = Rig::new("streamless");
        let item = rig.graph.item("a ready item the stream will not take");
        let ring = StubRing::answering(RingOutcome::Delivered);
        let spawner = StubSpawner::answering(SpawnOutcome::Spawned {
            seat: full("t3"),
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
                by: &by(),
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
                seats: &Directory::default(),
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
            matches!(read.order, OrderState::Ordered(_)),
            "the order is on the record: the entry and the index precede the event"
        );
    }
}
