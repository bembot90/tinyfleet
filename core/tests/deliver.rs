//! `fleet deliver` against a real work graph and a git seam that answers.
//!
//! One store for the whole binary and one item per arm, as the dispatch suite
//! has it: `bd` serialises against itself on this box. Each arm also takes its
//! own SEAT name, because the item a seat holds is a query across the whole
//! store.
//!
//! The git seam is a stub with recorded calls rather than a repository: what
//! deliver does with a dirty tree, an empty index and a commit that answers a
//! known sha is one seam value away here, and the live path is proven against a
//! scratch repository in the cli's own suite.

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::{fleet_of, full, keys_agree, seat_actor, signal, Graph, Rooted, StubEvents};
use fleet_core::entry::{Body, Entry, Timeline};
use fleet_core::input::{DeliveryInput, DELIVERY_SCHEMA};
use fleet_core::item::brief::Packs;
use fleet_core::item::deliver::{self, Delivered, Delivery, Wiring};
use fleet_core::item::review::{self, Mode, Verdict};
use fleet_core::item::{
    control_token, run, Change, Git, Project, Ring, RingOutcome, Stop, ITEM_ENTRY, TRUNK,
};
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::store::{AssignedItem, Item, Store, StoreError};

const REVIEWER: &str = "a-reviewer";
const AT: &str = "2026-09-09T04:05:06Z";
const SHA: &str = "1111111111111111111111111111111111111111";
const TRUNK_SHA: &str = "2222222222222222222222222222222222222222";
const BRANCH: &str = "a-seat/feat/the-work";
const POLICY: &str = "[core]\nreviewer = \"a-reviewer\"\n";

// ---- the seams ---------------------------------------------------------------

struct StubGit {
    branch: String,
    staged: Vec<String>,
    status: Vec<String>,
    commit: String,
    calls: Mutex<Vec<String>>,
}

impl StubGit {
    /// A worktree on a work branch with one file staged and nothing else
    /// touched: the shape a delivery is made from.
    fn clean() -> StubGit {
        StubGit {
            branch: BRANCH.to_string(),
            staged: vec!["a/file.rs".to_string()],
            status: vec!["M  a/file.rs".to_string()],
            commit: SHA.to_string(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("not poisoned").clone()
    }

    fn record(&self, call: &str) {
        self.calls
            .lock()
            .expect("not poisoned")
            .push(call.to_string());
    }
}

impl Git for StubGit {
    fn current_branch(&self) -> Result<String, String> {
        self.record("current_branch");
        Ok(self.branch.clone())
    }

    fn head(&self) -> Result<String, String> {
        self.record("head");
        Ok(self.commit.clone())
    }

    fn trunk_tip(&self) -> Result<String, String> {
        self.record("trunk_tip");
        Ok(TRUNK_SHA.to_string())
    }

    fn staged(&self) -> Result<Vec<String>, String> {
        self.record("staged");
        Ok(self.staged.clone())
    }

    fn status(&self) -> Result<Vec<String>, String> {
        self.record("status");
        Ok(self.status.clone())
    }

    fn add_all(&self) -> Result<(), String> {
        self.record("add_all");
        Ok(())
    }

    fn commit(&self, message: &str) -> Result<String, String> {
        self.record(&format!("commit {message}"));
        Ok(self.commit.clone())
    }

    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        self.record(&format!("numstat {from} {to}"));
        Ok(Vec::new())
    }
}

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

/// The real store with one reading bent, so an arm can force the disagreement
/// the read-back exists to catch — or with its entry append refused, which is
/// the write a real store will not refuse on demand.
struct Doctored<'a> {
    inner: &'a dyn Store,
    assignee: Option<String>,
    append: Option<String>,
    /// Answered by `append` instead of the write.
    unwritable: Option<String>,
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
        if let Some(extra) = &self.append {
            read.document.push_str(extra);
        }
        Ok(read)
    }

    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError> {
        self.inner.assigned_to(seat)
    }

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.inner.assign(item, seat, by)
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

    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.inner.reopen(item, by)
    }

    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        self.inner.hold(item, reason, by)
    }

    fn open_holds(&self) -> Result<Vec<String>, StoreError> {
        self.inner.open_holds()
    }

    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError> {
        self.inner.clear_hold(hold, by)
    }

    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        self.inner.close(item, reason, by)
    }

    fn append(
        &self,
        item: &str,
        body: &fleet_core::entry::Body,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<String, StoreError> {
        if let Some(why) = &self.unwritable {
            return Err(StoreError::Unreadable(why.clone()));
        }
        self.inner.append(item, body, by)
    }

    fn timeline(&self, item: &str) -> Result<Vec<fleet_core::entry::Entry>, StoreError> {
        self.inner.timeline(item)
    }

    fn export(&self, into: &std::path::Path) -> Result<(), StoreError> {
        self.inner.export(into)
    }
}

// ---- the rig -----------------------------------------------------------------

/// One board per arm, held in memory.
fn store() -> Graph {
    dressed(Graph::memory("deliver"))
}

/// The integration ring: the one arm of this suite that delivers through `bd`.
fn ring() -> Graph {
    dressed(Graph::real("deliver"))
}

fn dressed(graph: Graph) -> Graph {
    graph.fleet_toml(POLICY);
    graph
}

fn project(scratch: &dyn Rooted) -> Project {
    let table = fleet_core::item::table_at(&scratch.root().join("fleet.toml"));
    Project {
        root: scratch.root().to_path_buf(),
        name: "a-project".to_string(),
        guards: table.clone(),
        policy: table,
    }
}

/// One item, held by this arm's own seat and carrying an order.
fn an_ordered_item(graph: &Graph, title: &str, seat: &str) -> String {
    let item = graph.item(title);
    let seat = full(seat);
    graph.assign(&item, &seat);
    graph
        .store()
        .set_orders(
            &item,
            &format!(
                r#"{{"fleet.orders": {{"v": 1, "by": "an-architect", "kind": "dispatch", "seat": "{seat}", "at": "{AT}"}}}}"#
            ),
            "an-architect",
        )
        .expect("the order index lands");
    item
}

/// The delivery a seat hands in, as the file the brief's schema shows it.
fn a_delivery(scratch: &dyn Rooted, label: &str, body: &serde_json::Value) -> PathBuf {
    a_file(scratch, label, &body.to_string())
}

/// A file handed in as the delivery, whatever it holds: the arms whose subject
/// is a file that does not read write their own text.
fn a_file(scratch: &dyn Rooted, label: &str, text: &str) -> PathBuf {
    let path = scratch.root().join(format!("delivery-{label}.json"));
    std::fs::write(&path, text).expect("the delivery is written");
    path
}

/// A whole delivery: every field the seat fills, two numbered calls for a
/// review to walk, and none of the commit, the branch, the base or the time,
/// which are the verb's.
fn whole() -> serde_json::Value {
    serde_json::json!({
        "files": ["a/file.rs"],
        "checks": [{"check": "AC1", "result": "green, read from the arm's own status"}],
        "suite": {"command": "the workspace suite", "rc": 0},
        "spec_corrections": [],
        "not_proven": [{"surface": "nothing this arm did not run", "command": "cargo nextest run"}],
        "decisions": [
            {
                "call": "the seat's delivery is carried through",
                "not_taken": "composing it here",
                "because": "the words are the seat's"
            },
            {
                "call": "the machine lines are filled",
                "not_taken": "trusting the seat's",
                "because": "only a process knows them"
            }
        ],
        "covers": ["R6"]
    })
}

/// A delivery as the verb reads it, for the entry an arm expects written.
fn input_of(body: &serde_json::Value) -> DeliveryInput {
    serde_json::from_value(body.clone()).expect("the delivery reads as the type")
}

/// The entry a delivery of `body` at the stub's commit, branch and base is.
fn expected(body: &serde_json::Value, commit: &str) -> Body {
    Body::Delivered(input_of(body).into_delivered(
        commit.to_string(),
        BRANCH.to_string(),
        TRUNK_SHA.to_string(),
    ))
}

/// The item's timeline, as the store answers it.
fn timeline(graph: &Graph, item: &str) -> Vec<Entry> {
    graph.store().timeline(item).expect("the timeline reads")
}

/// What an arm varies, gathered so the call below reads as the arm and not as
/// the wiring.
struct Seams<'a> {
    store: &'a dyn Store,
    git: &'a dyn Git,
    ring: &'a StubRing,
    project: &'a Project,
    packs: &'a Packs,
    events: &'a StubEvents,
}

fn deliver_with(
    item: Option<&str>,
    delivery: &PathBuf,
    by: &str,
    seams: &Seams,
) -> Result<Delivered, Stop> {
    deliver::deliver(
        &mut Vec::new(),
        &mut Vec::new(),
        &Delivery {
            item,
            // The arm's own seat, by the name its id is derived from.
            by: &seat_actor(by),
            delivery,
            at: AT,
        },
        &Wiring {
            store: seams.store,
            git: seams.git,
            packs: seams.packs,
            project: seams.project,
            ring: seams.ring,
            events: seams.events,
            // The arm's own seat, listed under the name it delivers by; the
            // arm about `by` itself lays out a fleet of its own.
            seats: &fleet_of(&[by, REVIEWER]),
        },
    )
}

fn packs(scratch: &dyn Rooted) -> Packs {
    Packs::under(scratch.packs_dir(), scratch.defaults_dir()).expect("the defaults resolve")
}

/// What the store holds against an item, as the arms read it back.
fn read(graph: &Graph, item: &str) -> Item {
    graph.store().show(item).expect("the item reads")
}

// ---- the arms ----------------------------------------------------------------

/// THE INTEGRATION RING of this suite, and the one arm here that delivers
/// through `bd`: the reassignment and the delivered entry written and read
/// back through the store the verb actually talks to.
#[test]
fn a_clean_delivery_commits_reassigns_and_writes_the_delivered_entry() {
    let scratch = &ring();
    let seat = "s-clean";
    let item = an_ordered_item(scratch, "an item to deliver", seat);
    let delivery = a_delivery(scratch, "clean", &whole());
    let git = StubGit::clean();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &ring,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect("the delivery is made");

    assert_eq!(delivered.item, item, "the seat's one ordered item");
    assert_eq!(delivered.commit, SHA);
    assert_eq!(
        delivered.reviewer,
        full(REVIEWER),
        "the seat the reviewer policy names, by its id"
    );

    // The timeline ENDS IN THE DELIVERED ENTRY, by the seat: the three
    // machine fields from the verb's own values, all whole shas where they are
    // shas, and every other field the seat's JSON as it was handed in.
    let entries = timeline(scratch, &item);
    let last = entries.last().expect("the timeline carries an entry");
    assert_eq!(last.body, expected(&whole(), SHA), "the entry, whole");
    assert_eq!(last.by, seat_actor(seat), "written by the seat delivering");
    assert_eq!(delivered.entry, last.id, "and answered by its id");

    let read = read(scratch, &item);
    assert_eq!(read.assignee.as_deref(), Some(full(REVIEWER).as_str()));

    assert!(
        git.calls()
            .iter()
            .any(|call| call.starts_with(&format!("commit {item}:"))),
        "the commit subject names the item by its full id: {:?}",
        git.calls()
    );
    // The one event: the delivered entry's signal, by the seat delivering,
    // naming the entry the timeline holds. The commit is the entry's.
    assert_eq!(
        events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            seat_actor(seat).to_string(),
            signal(&item, &last.id, "delivered"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &events.all()[0].2, &[]);

    let rung = ring.calls();
    assert_eq!(rung.len(), 1);
    assert_eq!(rung[0].0, full(REVIEWER));
    assert!(
        rung[0].1.contains(&item) && rung[0].1.contains(SHA),
        "{:?}",
        rung
    );
}

/// RED-PROOF of the key check every verb suite's signal goes through: a signal
/// that carried the commit beside its three keys — a copy of the entry on the
/// stream, which [ASSUMES D2] rules out — is refused by it.
#[test]
#[should_panic(expected = "item.entry's payload keys")]
fn a_signal_carrying_the_commit_fails_the_key_check() {
    keys_agree(
        ITEM_ENTRY,
        &serde_json::json!({
            "item": "fx-item",
            "entry": "fx-entry",
            "kind": "delivered",
            "commit": SHA,
        }),
        &[],
    );
}

/// Another writer's bare `orders` and the bare `run` label are not fleet's:
/// on the delivered item they ride through the delivery byte-identical, and a
/// second item the seat is assigned that carries only them is not an order it
/// holds (fleet-4j6 AC1, the delivery).
#[test]
fn another_writers_orders_key_and_run_label_ride_through_a_delivery() {
    let scratch = &store();
    let seat = "s-foreign";
    let item = an_ordered_item(scratch, "an item another tool indexes too", seat);
    let theirs = scratch.item("an item only another tool ordered");
    scratch.assign(&theirs, &full(seat));
    for held in [&item, &theirs] {
        scratch
            .store()
            .set_metadata(held, common::FOREIGN_ORDERS, "another-tool")
            .expect("the other writer's key lands");
        scratch.label(held, common::FOREIGN_LABEL);
    }
    let before = common::foreign_of(scratch.store(), &item);
    let delivery = a_delivery(scratch, "foreign", &whole());
    let git = StubGit::clean();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &ring,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect("the delivery is made");

    assert_eq!(delivered.item, item, "the seat's one ordered item");
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full(REVIEWER).as_str())
    );
    assert_eq!(
        common::foreign_of(scratch.store(), &item),
        before,
        "the other writer's key and label are byte-identical"
    );
    assert_eq!(
        read(scratch, &theirs).assignee.as_deref(),
        Some(full(seat).as_str()),
        "the item only another tool ordered is left where it was"
    );
}

#[test]
fn the_trunk_is_refused_and_nothing_is_committed() {
    let scratch = &store();
    let seat = "s-trunk";
    let item = an_ordered_item(scratch, "an item on the trunk", seat);
    let delivery = a_delivery(scratch, "trunk", &whole());
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::clean()
    };

    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("the trunk is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(stop.message.contains("main"), "{}", stop.message);
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is untouched");
}

#[test]
fn a_file_outside_the_staged_set_is_refused_by_name() {
    let scratch = &store();
    let seat = "s-loose";
    let item = an_ordered_item(scratch, "an item beside a loose file", seat);
    let delivery = a_delivery(scratch, "loose", &whole());
    let before = scratch.json(&item);
    let git = StubGit {
        status: vec![
            "M  a/file.rs".to_string(),
            " M b/forgotten.rs".to_string(),
            "?? c/untracked.rs".to_string(),
        ],
        ..StubGit::clean()
    };

    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("the loose file is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains("b/forgotten.rs"),
        "the first one, by name: {}",
        stop.message
    );
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is untouched");
}

/// The control for the arm above: the same status lines with the two loose
/// paths staged, which is a tree the same reading calls clean.
#[test]
fn a_staged_file_with_more_work_on_it_is_the_delivery() {
    let git = StubGit {
        status: vec!["MM a/file.rs".to_string(), " M b/also.rs".to_string()],
        staged: vec!["a/file.rs".to_string(), "b/also.rs".to_string()],
        ..StubGit::clean()
    };
    assert_eq!(
        deliver::outside(&git.status, &git.staged),
        None,
        "a path in the staged set is the delivery whatever else was done to it"
    );
    assert_eq!(
        deliver::outside(&git.status, &["a/file.rs".to_string()]),
        Some("b/also.rs".to_string()),
        "and the same reading finds the one that is not"
    );
}

/// THE FIRST HALF OF THE PAIR, and worth nothing without the second below: a
/// seat resuming after `fleet hold` at the held commit, nothing left to stage,
/// HEAD ahead of the base. The delivery is that commit and no commit is made.
#[test]
fn a_clean_tree_ahead_of_the_base_delivers_head_and_commits_nothing() {
    let scratch = &store();
    let seat = "s-ahead";
    let item = an_ordered_item(scratch, "an item held at its finished commit", seat);
    let delivery = a_delivery(scratch, "ahead", &whole());
    let git = StubGit {
        staged: Vec::new(),
        status: Vec::new(),
        ..StubGit::clean()
    };
    // The sha HEAD holds BEFORE the command runs, read through the same seam
    // deliver reads, so the assertion below compares two readings and not one
    // constant against itself.
    let was = git.head().expect("the stub answers HEAD");
    assert_ne!(
        was, TRUNK_SHA,
        "the arm's premise: HEAD is ahead of the base"
    );
    let mut out = Vec::new();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver::deliver(
        &mut out,
        &mut Vec::new(),
        &Delivery {
            item: Some(&item),
            by: &seat_actor(seat),
            delivery: &delivery,
            at: AT,
        },
        &Wiring {
            store: scratch.store(),
            git: &git,
            packs: &packs(scratch),
            project: &project(scratch),
            ring: &ring,
            events: &events,
            seats: &fleet_of(&[seat, REVIEWER]),
        },
    )
    .expect("the clean tree ahead of the base is delivered");

    assert_eq!(
        delivered.commit, was,
        "the delivery is the commit HEAD already named"
    );
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    let entries = timeline(scratch, &item);
    let (entry, body) = Timeline(&entries)
        .last_delivery()
        .expect("the delivery is on the timeline");
    assert_eq!(body.commit, was, "the entry's commit is that sha");
    assert_eq!(entry.id, delivered.entry, "and it is the entry answered");
    let read = read(scratch, &item);
    assert_eq!(
        read.assignee.as_deref(),
        Some(full(REVIEWER).as_str()),
        "the handoff is recorded"
    );
    let (_, payload) = events.one(ITEM_ENTRY);
    assert_eq!(payload, signal(&item, &entry.id, "delivered"));
    assert!(
        String::from_utf8_lossy(&out).contains(deliver::AS_IS),
        "the seat is told nothing was committed: {}",
        String::from_utf8_lossy(&out)
    );
}

/// THE SECOND HALF, and the one that keeps the first worth something: the same
/// clean tree with HEAD AT the base is a seat that built nothing, and is still
/// refused — with the resumed-after-ask case and the way out named.
#[test]
fn a_clean_tree_at_the_base_is_refused_and_the_refusal_names_the_way_out() {
    let scratch = &store();
    let seat = "s-empty";
    let item = an_ordered_item(scratch, "an item with nothing staged", seat);
    let delivery = a_delivery(scratch, "empty", &whole());
    let before = scratch.json(&item);
    let git = StubGit {
        staged: Vec::new(),
        status: Vec::new(),
        commit: TRUNK_SHA.to_string(),
        ..StubGit::clean()
    };
    assert_eq!(
        git.head().expect("the stub answers HEAD"),
        TRUNK_SHA,
        "the arm's premise: HEAD is the base"
    );

    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("an empty delivery at the base is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    for named in ["fleet hold", "AHEAD", TRUNK, "stage the work"] {
        assert!(
            stop.message.contains(named),
            "the refusal names `{named}`: {}",
            stop.message
        );
    }
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is untouched");
}

/// `review --show` over the item, as the reviewer runs it: what it read.
fn review_shown(scratch: &Graph, item: &str) -> (review::Read, String) {
    let mut out = Vec::new();
    let read = review::review(
        &mut out,
        &mut Vec::new(),
        &Verdict {
            item,
            by: &seat_actor(REVIEWER),
            mode: Mode::Show,
        },
        &review::Wiring {
            store: scratch.store(),
            git: &StubGit::clean(),
            packs: &packs(scratch),
            project: &project(scratch),
            ring: &StubRing::answering(RingOutcome::Delivered),
            events: &StubEvents::default(),
            seats: &fleet_of(&[REVIEWER]),
        },
    )
    .expect("the review reads the delivery");
    (read, String::from_utf8(out).expect("the page is utf-8"))
}

/// A DELIVERY IS JSON, AND THE DELIVERED ENTRY IS BUILT FROM IT. The file reads
/// as the binary's own type, the entry written carries it whole beside the
/// three values the verb fills, and `review --show` reads its commit and both
/// numbered calls off the entry.
#[test]
fn a_delivery_file_is_delivered_and_review_reads_the_entry_built_from_it() {
    let scratch = &store();
    let seat = "s-json";
    let item = an_ordered_item(scratch, "an item delivered as JSON", seat);
    let delivery = a_delivery(scratch, "json", &whole());

    deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the delivery is made");

    let entries = timeline(scratch, &item);
    let (_, body) = Timeline(&entries)
        .last_delivery()
        .expect("the delivery is on the timeline");
    assert_eq!(
        Body::Delivered(body.clone()),
        expected(&whole(), SHA),
        "the entry is the seat's JSON and the verb's three values"
    );

    let (shown, page) = review_shown(scratch, &item);
    assert_eq!(shown.commit, SHA, "the review reads the delivered commit");
    assert_eq!(
        review::decisions(body),
        vec!["D1".to_string(), "D2".to_string()],
        "and both numbered calls"
    );
    for call in ["D1 the seat's delivery", "D2 the machine lines"] {
        assert!(page.contains(call), "the page shows `{call}`:\n{page}");
    }
}

/// THE ENTRY IS THE ONLY WRITE ABOUT THE DELIVERY. Of what the verb wrote, one
/// line is the reassignment and one the delivered entry, by the seat; no line
/// is a note.
#[test]
fn a_delivery_appends_one_delivered_entry_and_writes_no_note() {
    let scratch = &store();
    let seat = "s-no-note";
    let item = an_ordered_item(scratch, "an item delivered without a note", seat);
    let delivery = a_delivery(scratch, "no-note", &whole());
    let Graph::Memory(board) = scratch else {
        unreachable!("the rig is in memory");
    };
    board.forget_writes();

    let delivered = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the delivery is made");

    let wrote = board.store.wrote();
    assert_eq!(
        wrote,
        vec![
            format!("assign {item} {} {}", full(REVIEWER), seat_actor(seat)),
            format!("append {item} delivered {}", seat_actor(seat)),
        ],
        "the reassignment and the entry, and nothing else"
    );
    let entries = timeline(scratch, &item);
    let last = entries.last().expect("the timeline carries an entry");
    assert_eq!(last.body, expected(&whole(), SHA));
    assert_eq!(last.by, seat_actor(seat));
    assert_eq!(delivered.entry, last.id, "the id answered is the entry's");
}

/// A STORE THAT TAKES THE ENTRY AND DOES NOT KEEP IT is caught by the entry's
/// own read-back: exit 3, saying the commit STANDS and the item is the
/// reviewer's now — because the commit is real, and a caller that read this
/// as nothing having happened would deliver twice. Nothing is announced and
/// nobody is rung.
#[test]
fn a_delivered_entry_the_store_does_not_keep_exits_three_and_the_commit_stands() {
    let scratch = &store();
    let seat = "s-unkept";
    an_ordered_item(scratch, "an item whose store forgets its writes", seat);
    let delivery = a_delivery(scratch, "unkept", &whole());
    let Graph::Memory(board) = scratch else {
        unreachable!("the rig is in memory");
    };
    board.store.ignore_writes();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &StubGit::clean(),
            ring: &ring,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("the entry is not on the timeline");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains("does not hold the delivered entry"),
        "the stop names the entry the timeline does not hold: {}",
        stop.message
    );
    assert!(
        stop.message
            .contains(&format!("the commit {SHA} STANDS on the work branch and "))
            && stop
                .message
                .ends_with(&format!(" is reassigned to {}", full(REVIEWER))),
        "the commit STANDS: {}",
        stop.message
    );
    assert_eq!(events.count(), 0, "nothing is announced");
    assert!(ring.calls().is_empty(), "nobody is rung");
}

/// An entry the store REFUSES to write is exit 3 too, and says the commit
/// STANDS and the item carries no delivery.
#[test]
fn a_delivered_entry_the_store_refuses_exits_three_and_the_commit_stands() {
    let scratch = &store();
    let seat = "s-refused";
    let item = an_ordered_item(scratch, "an item whose store refuses the entry", seat);
    let delivery = a_delivery(scratch, "refused", &whole());
    let refusing = Doctored {
        inner: scratch.store(),
        assignee: None,
        append: None,
        unwritable: Some("the store would not take the comment".to_string()),
    };
    let events = StubEvents::default();

    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: &refusing,
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("the entry is not written");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "the delivered entry did not land: the store would not take the comment\n  the \
             commit {SHA} STANDS on the work branch and {item} carries no delivery"
        )
    );
    assert_eq!(events.count(), 0, "nothing is announced");
}

/// A FILE THAT DOES NOT READ IS REFUSED BEFORE ANY WRITE. Each of the four is
/// exit 2 naming the schema, and in every one the commit was never asked for
/// and the store was never written: the read sits before the commit, the
/// reassignment and the entry.
#[test]
fn a_delivery_that_does_not_read_is_refused_at_two_before_anything_is_written() {
    let mut unknown = whole();
    unknown["colour"] = serde_json::json!("blue");
    let mut unproven = whole();
    unproven
        .as_object_mut()
        .expect("an object")
        .remove("not_proven");
    let mut fileless = whole();
    fileless["files"] = serde_json::json!([]);
    // The prose note a seat handed in before this verb read JSON.
    let prose = "DELIVERED <sha> — <seat>\ncommit:  <pending>\nfiles:   a/file.rs\n";

    for (label, text, named) in [
        ("colour", unknown.to_string(), "colour"),
        ("unproven", unproven.to_string(), "not_proven"),
        ("fileless", fileless.to_string(), "names no file"),
        ("prose", prose.to_string(), "expected value"),
    ] {
        let scratch = &store();
        let seat = format!("s-unread-{label}");
        let item = an_ordered_item(
            scratch,
            &format!("an item whose delivery is {label}"),
            &seat,
        );
        let delivery = a_file(scratch, label, &text);
        let Graph::Memory(board) = scratch else {
            unreachable!("the rig is in memory");
        };
        board.forget_writes();
        let git = StubGit::clean();
        let events = StubEvents::default();

        let stop = deliver_with(
            None,
            &delivery,
            &seat,
            &Seams {
                store: scratch.store(),
                git: &git,
                ring: &StubRing::answering(RingOutcome::Delivered),
                project: &project(scratch),
                packs: &packs(scratch),
                events: &events,
            },
        )
        .expect_err("the delivery does not read");

        assert_eq!(stop.code, 2, "{label}: {}", stop.message);
        assert!(
            stop.message.contains(DELIVERY_SCHEMA),
            "{label}: the refusal names the schema: {}",
            stop.message
        );
        assert!(
            stop.message.contains(named),
            "{label}: and what is wrong, `{named}`: {}",
            stop.message
        );
        assert!(
            !git.calls().iter().any(|call| call.starts_with("commit ")),
            "{label}: nothing was committed: {:?}",
            git.calls()
        );
        assert_eq!(
            board.store.wrote(),
            Vec::<String>::new(),
            "{label}: nothing was written"
        );
        assert_eq!(events.count(), 0, "{label}: nothing was announced");
        assert_eq!(
            read(scratch, &item).assignee.as_deref(),
            Some(full(&seat).as_str()),
            "{label}: the item is still the seat's"
        );
    }
}

/// FLEET-4RL, CLOSED BY THE SHAPE: there is no delivery grammar for a value to
/// break. A `because` naming a delivery and a verdict, and a check result
/// naming a return, are values of the entry and nothing else — carried whole,
/// newlines and all, the read-back passes, and a review reads the commit.
#[test]
fn a_marker_inside_a_value_is_carried_whole_in_the_entry() {
    let scratch = &store();
    let seat = "s-4rl";
    let item = an_ordered_item(scratch, "an item whose delivery quotes markers", seat);
    let mut body = whole();
    body["decisions"][0]["because"] =
        serde_json::json!("fine\nDELIVERED deadbeef — x\nACCEPTED deadbeef — y");
    body["checks"][0]["result"] = serde_json::json!("RETURNED WITH FINDINGS a — b");
    let delivery = a_delivery(scratch, "4rl", &body);

    deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the delivery is made, and its read-back passes");

    let entries = timeline(scratch, &item);
    let (_, delivered) = Timeline(&entries)
        .last_delivery()
        .expect("the delivery is on the timeline");
    assert_eq!(
        delivered.decisions[0].because, "fine\nDELIVERED deadbeef — x\nACCEPTED deadbeef — y",
        "the value, as the seat wrote it"
    );
    assert_eq!(
        delivered.checks[0].result, "RETURNED WITH FINDINGS a — b",
        "and the other, as the seat wrote it"
    );
    assert_eq!(Body::Delivered(delivered.clone()), expected(&body, SHA));
    let (shown, _) = review_shown(scratch, &item);
    assert_eq!(shown.commit, SHA, "and a review reads the delivered commit");
}

#[test]
fn a_fleet_naming_no_reviewer_refuses_before_the_commit() {
    let events = StubEvents::default();
    let scratch = &store();
    let seat = "s-nobody";
    let item = an_ordered_item(scratch, "an item with nowhere to go", seat);
    let delivery = a_delivery(scratch, "nobody", &whole());
    let mut nameless = project(scratch);
    nameless.guards = "[landing]\nci_marker = \"printf '[skip ci]'\"\n"
        .parse()
        .expect("the policy parses");
    let git = StubGit::clean();

    let stop = deliver_with(
        Some(&item),
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &nameless,
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("a delivery has nowhere to go");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(stop.message.contains("reviewer"), "{}", stop.message);
    assert_eq!(events.count(), 0, "a refusal appends nothing");
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
}

/// `[core] reviewer` is any seat argument, and deliver hands the item to the
/// ONE LISTED SEAT it names, by that seat's full id: a reviewer written by name
/// is assigned, read back and rung by id, and a value that names no seat of
/// this fleet is refused naming the key before anything is committed.
#[test]
fn a_reviewer_named_by_name_is_assigned_by_its_full_id_and_a_stranger_is_refused() {
    let scratch = &store();
    let seat = "s-to-kite";
    let item = an_ordered_item(scratch, "an item Kite reviews", seat);
    let delivery = a_delivery(scratch, "to-kite", &whole());
    let policy = |reviewer: &str| {
        let mut named = project(scratch);
        named.guards = format!("[core]\nreviewer = \"{reviewer}\"\n")
            .parse()
            .expect("the policy parses");
        named
    };
    let fleet = fleet_of(&[seat, "Kite"]);
    let run = |project: &Project, git: &StubGit, ring: &StubRing| {
        deliver::deliver(
            &mut Vec::new(),
            &mut Vec::new(),
            &Delivery {
                item: Some(&item),
                by: &seat_actor(seat),
                delivery: &delivery,
                at: AT,
            },
            &Wiring {
                store: scratch.store(),
                git,
                packs: &packs(scratch),
                project,
                ring,
                events: &StubEvents::default(),
                seats: &fleet,
            },
        )
    };

    // A VALUE NAMING NO LISTED SEAT is exit 1 naming the key, before the commit.
    // Asked first, while the seat still holds the item `--item` names.
    let git = StubGit::clean();
    let stop = run(
        &policy("nobody"),
        &git,
        &StubRing::answering(RingOutcome::Delivered),
    )
    .expect_err("nobody is no seat");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message
            .starts_with("[core] reviewer = \"nobody\" nobody names no seat — the seats are "),
        "{}",
        stop.message
    );
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full(seat).as_str()),
        "the refusal wrote nothing"
    );

    // KITE, BY NAME, is assigned and rung by her full id.
    let ring = StubRing::answering(RingOutcome::Delivered);
    let delivered = run(&policy("Kite"), &StubGit::clean(), &ring).expect("Kite reviews it");
    assert_eq!(
        delivered.reviewer,
        full("Kite"),
        "the reviewer is Kite's id"
    );
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full("Kite").as_str()),
        "the item is assigned to Kite's full id and never to the word the policy wrote"
    );
    let rung = ring.calls();
    assert_eq!(rung.len(), 1, "{rung:?}");
    assert_eq!(rung[0].0, full("Kite"), "the ring addresses her id");
}

#[test]
fn a_read_back_that_disagrees_exits_three_and_prints_both_values() {
    let scratch = &store();
    let seat = "s-doctored";
    an_ordered_item(scratch, "an item whose read-back is bent", seat);
    let delivery = a_delivery(scratch, "doctored", &whole());

    let doctored = Doctored {
        inner: scratch.store(),
        assignee: Some("somebody-else".to_string()),
        append: None,
        unwritable: None,
    };

    // No `--item`: the bent reading is `show`'s, which the holder check on a
    // named item asks too, and the listing the seat's holding is read off is
    // not bent — so the disagreement is met where it is planted, at the
    // read-back.
    let events = StubEvents::default();
    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: &doctored,
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("the read-back disagrees");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains("somebody-else") && stop.message.contains(&full(REVIEWER)),
        "both values: {}",
        stop.message
    );
    assert_eq!(
        events.count(),
        0,
        "the event follows the read-back, so a disagreement announces nothing"
    );
}

#[test]
fn the_negative_control_catches_a_planted_token() {
    let scratch = &store();
    let seat = "s-control";
    let item = an_ordered_item(scratch, "an item whose read is not its own", seat);
    let delivery = a_delivery(scratch, "control", &whole());

    let planted = Doctored {
        inner: scratch.store(),
        assignee: None,
        append: Some(control_token().to_string()),
        unwritable: None,
    };

    let stop = deliver_with(
        Some(&item),
        &delivery,
        seat,
        &Seams {
            store: &planted,
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a read carrying the token is no read at all");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(stop.message.contains(control_token()), "{}", stop.message);
}

#[test]
fn a_seat_holding_no_ordered_item_is_refused_and_two_are_named() {
    let scratch = &store();
    let seat = "s-count";
    let delivery = a_delivery(scratch, "count", &whole());
    let unordered = scratch.item("an item with no order on it");
    scratch.assign(&unordered, &full(seat));

    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("an unordered item is not a delivery");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&seat_actor(seat).to_string()),
        "{}",
        stop.message
    );

    let first = an_ordered_item(scratch, "the first ordered item", seat);
    let second = an_ordered_item(scratch, "the second ordered item", seat);
    let stop = deliver_with(
        None,
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &StubGit::clean(),
            ring: &StubRing::answering(RingOutcome::Delivered),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("two ordered items is a question only the seat can answer");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&first) && stop.message.contains(&second),
        "every candidate is named: {}",
        stop.message
    );
}

/// ONLY A SEAT HOLDS WORK, AND THE KIND SAYS WHICH ACTOR IS ONE. A seat's
/// typed actor finds the item assigned to its id; a run holds nothing, so a
/// run's delivery without `--item` is refused naming the flag — before the
/// commit and before any write — and the same run naming the item delivers it
/// as it always did, signing the entry with its own string form.
#[test]
fn a_run_is_no_seat_and_delivers_only_the_item_it_names() {
    let scratch = &store();
    let item = an_ordered_item(scratch, "an item Orla holds", "Orla");
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full("Orla").as_str()),
        "the premise: the item is assigned to her id"
    );
    let delivery = a_delivery(scratch, "by-kind", &whole());
    let fleet = fleet_of(&["Orla", REVIEWER]);
    let run = |by: &Actor, item: Option<&str>, git: &StubGit| {
        deliver::deliver(
            &mut Vec::new(),
            &mut Vec::new(),
            &Delivery {
                item,
                by,
                delivery: &delivery,
                at: AT,
            },
            &Wiring {
                store: scratch.store(),
                git,
                packs: &packs(scratch),
                project: &project(scratch),
                ring: &StubRing::answering(RingOutcome::Delivered),
                events: &StubEvents::default(),
                seats: &fleet,
            },
        )
    };
    let typed = |text: &str| Actor::typed(text).expect("typed").expect("good");

    let a_run = typed("run:r1");
    let git = StubGit::clean();
    let stop = run(&a_run, None, &git).expect_err("run:r1 is no seat");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert_eq!(
        stop.message,
        "run:r1 is not a seat, so it holds nothing — pass --item <id>"
    );
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full("Orla").as_str()),
        "the refusal wrote nothing"
    );
    // The same by kind, and never by the id: a routine or the controller
    // whose id is Orla's own is still no seat.
    for other in [
        typed("routine:nightly"),
        typed(&format!("controller:{}", full("Orla"))),
    ] {
        let stop = run(&other, None, &StubGit::clean()).expect_err("no seat");
        assert_eq!(
            stop.message,
            format!("{other} is not a seat, so it holds nothing — pass --item <id>")
        );
    }

    // THE CONTROL: Orla's own typed actor finds the item assigned to her id.
    let delivered = run(&seat_actor("Orla"), None, &StubGit::clean()).expect("Orla finds her item");
    assert_eq!(delivered.item, item, "the item assigned to Orla's id");
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full(REVIEWER).as_str())
    );

    // WITH --item, A RUN STILL HOLDS NO SEAT'S ITEM (fleet-pl6 (a)): naming an
    // item Orla holds is refused by the run's kind, before the commit.
    let second = an_ordered_item(scratch, "an item no run holds", "Orla");
    let git = StubGit::clean();
    let stop = run(&a_run, Some(&second), &git).expect_err("a run holds no item");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert_eq!(
        stop.message,
        "a run holds no item — --item names an item the acting seat holds"
    );
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "nothing was committed: {:?}",
        git.calls()
    );
    assert_eq!(
        read(scratch, &second).assignee.as_deref(),
        Some(full("Orla").as_str()),
        "the refusal wrote nothing"
    );

    // A RUN'S OWN RECORD is the one item it holds: the run named by the
    // record's id delivers it, and the delivered entry is the run's.
    let record = scratch.item("a run's record");
    scratch.label(&record, run::LABEL);
    let its_run = Actor {
        kind: ActorKind::Run,
        id: record.clone(),
    };
    let delivered = run(&its_run, Some(&record), &StubGit::clean()).expect("its own record");
    assert_eq!(delivered.item, record);
    let entries = timeline(scratch, &record);
    let (entry, body) = Timeline(&entries)
        .last_delivery()
        .expect("the delivery is on the timeline");
    assert_eq!(entry.by, its_run, "written by the run");
    assert_eq!(body.commit, SHA);
}

/// `--item` NAMES AN ITEM THE ACTING SEAT HOLDS (fleet-pl6 (a)), read the way
/// the listing without `--item` reads a holding: the seat's id is the
/// assignee, the order index stands, and the item is no epic. Anything else is
/// exit 1 naming the holder and the actor, before the commit — and a run's
/// record is its own run's, whoever else names it.
#[test]
fn an_item_named_by_a_seat_that_does_not_hold_it_is_refused_naming_both() {
    let scratch = &store();
    let item = an_ordered_item(scratch, "an item Aoife holds", "Aoife");
    let delivery = a_delivery(scratch, "holder", &whole());
    let fleet = fleet_of(&["Aoife", "Bram", REVIEWER]);
    let run = |by: &Actor, item: &str, git: &StubGit| {
        deliver::deliver(
            &mut Vec::new(),
            &mut Vec::new(),
            &Delivery {
                item: Some(item),
                by,
                delivery: &delivery,
                at: AT,
            },
            &Wiring {
                store: scratch.store(),
                git,
                packs: &packs(scratch),
                project: &project(scratch),
                ring: &StubRing::answering(RingOutcome::Delivered),
                events: &StubEvents::default(),
                seats: &fleet,
            },
        )
    };
    let refused = |by: &Actor, item: &str, holder: &str| {
        let git = StubGit::clean();
        let stop = run(by, item, &git).expect_err("not the holder");
        assert_eq!(stop.code, 1, "{}", stop.message);
        assert_eq!(
            stop.message,
            format!(
                "{item} is held by {holder} and not by {by} — --item names an item the acting \
                 seat holds"
            )
        );
        assert!(
            !git.calls().iter().any(|call| call.starts_with("commit ")),
            "nothing was committed: {:?}",
            git.calls()
        );
    };

    // BRAM NAMING AOIFE'S ITEM: exit 1 naming both, and the item stays hers.
    refused(&seat_actor("Bram"), &item, &full("Aoife"));
    assert_eq!(
        read(scratch, &item).assignee.as_deref(),
        Some(full("Aoife").as_str()),
        "the refusal wrote nothing"
    );

    // Assigned to Bram and never ordered: assigned is not held.
    let unordered = scratch.item("an item assigned to Bram and never ordered");
    scratch.assign(&unordered, &full("Bram"));
    refused(&seat_actor("Bram"), &unordered, &full("Bram"));

    // An ordered epic assigned to Bram: an epic is never work a seat holds.
    let epic = an_ordered_item(scratch, "an epic ordered to Bram", "Bram");
    scratch.item_type(&epic, "epic");
    refused(&seat_actor("Bram"), &epic, &full("Bram"));

    // Assigned to nobody.
    let nobody = scratch.item("an item nobody holds");
    refused(&seat_actor("Bram"), &nobody, "nobody");

    // A SEAT NAMING A RUN'S RECORD: the record is its own run's to hold.
    let record = scratch.item("a run's record a seat names");
    scratch.label(&record, run::LABEL);
    let git = StubGit::clean();
    let stop = run(&seat_actor("Bram"), &record, &git).expect_err("not that run");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "{record} is run {record}'s record, and {} is not that run — a run's record is its \
             own run's to hold",
            seat_actor("Bram")
        )
    );
    assert!(!git.calls().iter().any(|call| call.starts_with("commit ")));

    // THE CONTROL: Aoife naming her own item delivers it.
    let delivered = run(&seat_actor("Aoife"), &item, &StubGit::clean()).expect("hers");
    assert_eq!(delivered.item, item);
}

/// `--item` naming its item by a suffix delivers under the full id: the verb
/// resolves the argument once and every write, the commit, the event and the
/// ring carry the id the store answered.
#[test]
fn an_item_named_by_its_suffix_is_delivered_under_its_full_id() {
    let scratch = &store();
    let seat = "s-suffix";
    let item = an_ordered_item(scratch, "an item named by its suffix", seat);
    let suffix = item.strip_prefix("fx-").expect("the board files under fx-");
    let Graph::Memory(board) = scratch else {
        unreachable!("the rig is in memory");
    };
    board.forget_writes();
    let delivery = a_delivery(scratch, "suffix", &whole());
    let git = StubGit::clean();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver_with(
        Some(suffix),
        &delivery,
        seat,
        &Seams {
            store: scratch.store(),
            git: &git,
            ring: &ring,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect("the delivery is made");

    assert_eq!(delivered.item, item);
    let wrote = board.store.wrote();
    for verb in ["assign", "append"] {
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
    assert!(
        git.calls()
            .iter()
            .any(|call| call.starts_with(&format!("commit {item}:"))),
        "the commit subject names the full id: {:?}",
        git.calls()
    );
    let (_, payload) = events.one(ITEM_ENTRY);
    assert_eq!(payload["item"], serde_json::json!(item));
    let rung = ring.calls();
    assert!(
        rung.len() == 1 && rung[0].1.starts_with(&item),
        "the ring names the full id: {rung:?}"
    );
}

/// The two rings that are not a delivery: neither changes the exit, because the
/// reassignment already recorded the handoff.
#[test]
fn an_absent_reviewer_and_a_failed_ring_both_leave_the_delivery_standing() {
    let scratch = &store();
    for (label, outcome, expect) in [
        ("absent", RingOutcome::Absent, deliver::STANDS),
        (
            "failed",
            RingOutcome::Failed("the provider said no".to_string()),
            deliver::STANDS,
        ),
    ] {
        let seat = format!("s-ring-{label}");
        let item = an_ordered_item(scratch, &format!("an item rung {label}"), &seat);
        let delivery = a_delivery(scratch, label, &whole());
        let mut out = Vec::new();
        let mut err = Vec::new();
        let ring = StubRing::answering(outcome);
        let git = StubGit::clean();

        let delivered = deliver::deliver(
            &mut out,
            &mut err,
            &Delivery {
                item: Some(&item),
                by: &seat_actor(&seat),
                delivery: &delivery,
                at: AT,
            },
            &Wiring {
                store: scratch.store(),
                git: &git,
                packs: &packs(scratch),
                events: &StubEvents::default(),
                project: &project(scratch),
                ring: &ring,
                seats: &fleet_of(&[&seat, REVIEWER]),
            },
        )
        .expect("the delivery stands");

        assert_eq!(delivered.commit, SHA);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        );
        assert!(said.contains(expect), "{label}: {said}");
        if label == "absent" {
            // The line names the reviewer as a person reads it — its machine
            // name — and not by the id the record holds.
            assert!(
                said.contains(&format!(
                    "no live session for {};",
                    common::agent(REVIEWER).machine_name()
                )),
                "{label}: {said}"
            );
        }
        assert_eq!(
            read(scratch, &item).assignee.as_deref(),
            Some(full(REVIEWER).as_str()),
            "{label}: the handoff is recorded whatever the doorbell did"
        );
    }
}
