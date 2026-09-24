//! `fleet deliver` (packs PRD R6, R9, R20) against a real work graph and a git
//! seam that answers.
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

use common::{keys_agree, Graph, Rooted, StubEvents};
use fleet_core::item::brief::Packs;
use fleet_core::item::deliver::{self, Delivered, Delivery, Wiring};
use fleet_core::item::{
    control_token, label_value, last_delivery, Change, Git, Project, Ring, RingOutcome, Stop,
    ITEM_DELIVERED, TRUNK,
};
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
/// the read-back exists to catch.
struct Doctored<'a> {
    inner: &'a dyn Store,
    assignee: Option<String>,
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
    graph.assign(&item, seat);
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

/// The note a seat wrote, in the shipped grammar, with the three lines deliver
/// fills left as the seat left them.
fn a_note(scratch: &dyn Rooted, label: &str, body: &str) -> PathBuf {
    let path = scratch.root().join(format!("note-{label}.md"));
    std::fs::write(&path, body).expect("the note is written");
    path
}

const WHOLE: &str = "\
DELIVERED <sha> — <seat>
commit:  <pending>
branch:  <pending>
base:    <pending>
files:   a/file.rs
gate:    AC1 green, read from the arm's own status
suite:   the workspace suite, rc 0
spec corrections: none
not proven: nothing this arm did not run
decisions: 2
  D1 the seat's note is carried through; not taken: composing it here; because the words are the seat's
  D2 the machine lines are filled; not taken: trusting the seat's; because only a process knows them
covers: R6
";

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
    note: &PathBuf,
    by: &str,
    seams: &Seams,
) -> Result<Delivered, Stop> {
    deliver::deliver(
        &mut Vec::new(),
        &mut Vec::new(),
        &Delivery {
            item,
            by,
            note,
            at: AT,
        },
        &Wiring {
            store: seams.store,
            git: seams.git,
            packs: seams.packs,
            project: seams.project,
            ring: seams.ring,
            events: seams.events,
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
/// through `bd`: the reassignment and the note written and read back through
/// the store the verb actually talks to.
#[test]
fn a_clean_delivery_commits_reassigns_and_writes_the_note_it_rendered() {
    let scratch = &ring();
    let seat = "s-clean";
    let item = an_ordered_item(scratch, "an item to deliver", seat);
    let note = a_note(scratch, "clean", WHOLE);
    let git = StubGit::clean();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver_with(
        None,
        &note,
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
    assert_eq!(delivered.reviewer, REVIEWER, "the reviewer policy names");

    let expected = WHOLE
        .trim_end()
        .replace(
            "DELIVERED <sha> — <seat>",
            &format!("DELIVERED {SHA} — {seat}"),
        )
        .replace("commit:  <pending>", &format!("commit:  {SHA}"))
        .replace("branch:  <pending>", &format!("branch:  {BRANCH}"))
        .replace(
            "base:    <pending>",
            &format!("base:    {TRUNK} at {TRUNK_SHA}, read at {AT}"),
        );
    assert_eq!(
        delivered.note, expected,
        "the three lines, and nothing else"
    );

    let read = read(scratch, &item);
    assert_eq!(read.assignee.as_deref(), Some(REVIEWER));
    assert_eq!(
        last_delivery(read.notes.as_deref().expect("the item carries notes")).as_deref(),
        Some(expected.as_str()),
        "the note the store holds is the note that was rendered"
    );

    assert!(
        git.calls()
            .iter()
            .any(|call| call.starts_with(&format!("commit {item}:"))),
        "the commit subject names the item by its full id: {:?}",
        git.calls()
    );
    // The one event, carrying the three values the note's own machine lines do.
    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(ITEM_DELIVERED);
    assert_eq!(actor, seat, "the actor is the seat delivering");
    keys_agree(ITEM_DELIVERED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["commit"], serde_json::json!(SHA));
    assert_eq!(payload["branch"], serde_json::json!(BRANCH));
    assert_eq!(payload["base"], serde_json::json!(TRUNK_SHA));

    let rung = ring.calls();
    assert_eq!(rung.len(), 1);
    assert_eq!(rung[0].0, REVIEWER);
    assert!(
        rung[0].1.contains(&item) && rung[0].1.contains(SHA),
        "{:?}",
        rung
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
    scratch.assign(&theirs, seat);
    for held in [&item, &theirs] {
        scratch
            .store()
            .set_metadata(held, common::FOREIGN_ORDERS, "another-tool")
            .expect("the other writer's key lands");
        scratch.label(held, common::FOREIGN_LABEL);
    }
    let before = common::foreign_of(scratch.store(), &item);
    let note = a_note(scratch, "foreign", WHOLE);
    let git = StubGit::clean();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver_with(
        None,
        &note,
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
    assert_eq!(read(scratch, &item).assignee.as_deref(), Some(REVIEWER));
    assert_eq!(
        common::foreign_of(scratch.store(), &item),
        before,
        "the other writer's key and label are byte-identical"
    );
    assert_eq!(
        read(scratch, &theirs).assignee.as_deref(),
        Some(seat),
        "the item only another tool ordered is left where it was"
    );
}

#[test]
fn the_trunk_is_refused_and_nothing_is_committed() {
    let scratch = &store();
    let seat = "s-trunk";
    let item = an_ordered_item(scratch, "an item on the trunk", seat);
    let note = a_note(scratch, "trunk", WHOLE);
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::clean()
    };

    let stop = deliver_with(
        None,
        &note,
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
    let note = a_note(scratch, "loose", WHOLE);
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
        &note,
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
/// seat resuming after `fleet ask` at the parked commit, nothing left to stage,
/// HEAD ahead of the base. The delivery is that commit and no commit is made.
#[test]
fn a_clean_tree_ahead_of_the_base_delivers_head_and_commits_nothing() {
    let scratch = &store();
    let seat = "s-ahead";
    let item = an_ordered_item(scratch, "an item parked at its finished commit", seat);
    let note = a_note(scratch, "ahead", WHOLE);
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
            by: seat,
            note: &note,
            at: AT,
        },
        &Wiring {
            store: scratch.store(),
            git: &git,
            packs: &packs(scratch),
            project: &project(scratch),
            ring: &ring,
            events: &events,
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
    assert_eq!(
        label_value(&delivered.note, deliver::COMMIT).as_deref(),
        Some(was.as_str()),
        "the note's commit line carries that sha: {}",
        delivered.note
    );
    let read = read(scratch, &item);
    assert_eq!(
        read.assignee.as_deref(),
        Some(REVIEWER),
        "the handoff is recorded"
    );
    assert_eq!(
        last_delivery(read.notes.as_deref().expect("the item carries notes")).as_deref(),
        Some(delivered.note.as_str()),
        "the note the store holds is the note that was rendered"
    );
    let (_, payload) = events.one(ITEM_DELIVERED);
    assert_eq!(payload["commit"], serde_json::json!(was));
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
    let note = a_note(scratch, "empty", WHOLE);
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
        &note,
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
    for named in ["fleet ask", "AHEAD", TRUNK, "stage the work"] {
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

#[test]
fn a_note_with_no_marker_and_a_note_with_no_commit_line_are_usage_errors() {
    let scratch = &store();
    let seat = "s-note";
    let item = an_ordered_item(scratch, "an item whose note is malformed", seat);
    let before = scratch.json(&item);

    let unmarked = a_note(
        scratch,
        "unmarked",
        &WHOLE.replace("DELIVERED <sha> — <seat>", "delivered, I think"),
    );
    let uncommitted = a_note(
        scratch,
        "uncommitted",
        &WHOLE.replace("commit:  <pending>\n", ""),
    );

    for (note, absent) in [(&unmarked, "DELIVERED"), (&uncommitted, "commit:")] {
        let git = StubGit::clean();
        let stop = deliver_with(
            Some(&item),
            note,
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
        .expect_err("the note is refused");

        assert_eq!(stop.code, 2, "{}", stop.message);
        assert!(
            stop.message.contains(absent),
            "it names what is absent: {}",
            stop.message
        );
        assert!(
            !git.calls().iter().any(|call| call.starts_with("commit ")),
            "nothing was committed: {:?}",
            git.calls()
        );
    }
    assert_eq!(before, scratch.json(&item), "the item is untouched");
}

#[test]
fn a_fleet_naming_no_reviewer_refuses_before_the_commit() {
    let events = StubEvents::default();
    let scratch = &store();
    let seat = "s-nobody";
    let item = an_ordered_item(scratch, "an item with nowhere to go", seat);
    let note = a_note(scratch, "nobody", WHOLE);
    let mut nameless = project(scratch);
    nameless.guards = "[landing]\nci_marker = \"printf '[skip ci]'\"\n"
        .parse()
        .expect("the policy parses");
    let git = StubGit::clean();

    let stop = deliver_with(
        Some(&item),
        &note,
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

#[test]
fn a_read_back_that_disagrees_exits_three_and_prints_both_values() {
    let scratch = &store();
    let seat = "s-doctored";
    let item = an_ordered_item(scratch, "an item whose read-back is bent", seat);
    let note = a_note(scratch, "doctored", WHOLE);

    let doctored = Doctored {
        inner: scratch.store(),
        assignee: Some("somebody-else".to_string()),
        append: None,
    };

    let events = StubEvents::default();
    let stop = deliver_with(
        Some(&item),
        &note,
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
        stop.message.contains("somebody-else") && stop.message.contains(REVIEWER),
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
    let note = a_note(scratch, "control", WHOLE);

    let planted = Doctored {
        inner: scratch.store(),
        assignee: None,
        append: Some(control_token().to_string()),
    };

    let stop = deliver_with(
        Some(&item),
        &note,
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
    let note = a_note(scratch, "count", WHOLE);
    let unordered = scratch.item("an item with no order on it");
    scratch.assign(&unordered, seat);

    let stop = deliver_with(
        None,
        &note,
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
    assert!(stop.message.contains(seat), "{}", stop.message);

    let first = an_ordered_item(scratch, "the first ordered item", seat);
    let second = an_ordered_item(scratch, "the second ordered item", seat);
    let stop = deliver_with(
        None,
        &note,
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
    let note = a_note(scratch, "suffix", WHOLE);
    let git = StubGit::clean();
    let ring = StubRing::answering(RingOutcome::Delivered);
    let events = StubEvents::default();

    let delivered = deliver_with(
        Some(suffix),
        &note,
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
    for verb in ["assign", "note"] {
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
    let (_, payload) = events.one(ITEM_DELIVERED);
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
        let note = a_note(scratch, label, WHOLE);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let ring = StubRing::answering(outcome);
        let git = StubGit::clean();

        let delivered = deliver::deliver(
            &mut out,
            &mut err,
            &Delivery {
                item: Some(&item),
                by: &seat,
                note: &note,
                at: AT,
            },
            &Wiring {
                store: scratch.store(),
                git: &git,
                packs: &packs(scratch),
                events: &StubEvents::default(),
                project: &project(scratch),
                ring: &ring,
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
        assert_eq!(
            read(scratch, &item).assignee.as_deref(),
            Some(REVIEWER),
            "{label}: the handoff is recorded whatever the doorbell did"
        );
    }
}
