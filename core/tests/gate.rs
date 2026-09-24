//! `fleet ask` and `fleet answer` (flights PRD R19 to R22, S4) against a real
//! work graph and a git seam that answers.
//!
//! One store for the whole binary and one item per arm, as the delivery suite
//! has it: `bd` serialises against itself on this box. Each arm also takes its
//! own SEAT name, because the item a seat holds is a query across the whole
//! store.
//!
//! The git seam is a stub with recorded calls rather than a repository: what a
//! park does with a tree that has something to commit and with one that has
//! nothing is one seam value away here, and the live path is proven against a
//! scratch project in the cli's own suite.

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::gating::{gating_bd, standing, LEFT_BEHIND};
use common::{keys_agree, shared_store, Rooted, Scratch, StubEvents};
use fleet_core::item::brief::Packs;
use fleet_core::item::gate::{self, Question, Reply, Wiring};
use fleet_core::item::run;
use fleet_core::item::{
    last_answer, last_park, Change, Git, Project, Stop, ANSWER_MARKERS, GATE_RESOLVED, ITEM_PARKED,
    PARK_MARKERS,
};
use fleet_core::store::{AssignedItem, Bd, Item, NewItem, Store, StoreError};
use fleet_core::test_support::Board;

const AT: &str = "2026-09-13T04:05:06Z";
const SHA: &str = "3333333333333333333333333333333333333333";
const HEAD: &str = "4444444444444444444444444444444444444444";
const BRANCH: &str = "a-seat/feat/the-work";
/// The hash a run's open pinned, which its park stands on where a seat's park
/// stands on a commit.
const RUN_HASH: &str = "5555555555555555555555555555555555555555";
const POLICY: &str = "[core]\nreviewer = \"a-reviewer\"\n";

/// The question a seat hands in, in the shipped grammar.
const QUESTION: &str = "\
QUESTION the table the spec names is not there — what replaces it?
A. build the table, as the spec assumes
B. read the value off the item instead
";

// ---- the seams ---------------------------------------------------------------

struct StubGit {
    branch: String,
    /// What `staged` answers AFTER `add_all` has been called.
    staged: Vec<String>,
    calls: Mutex<Vec<String>>,
}

impl StubGit {
    /// A worktree on a work branch holding work: the shape a question is asked
    /// from.
    fn holding_work() -> StubGit {
        StubGit {
            branch: BRANCH.to_string(),
            staged: vec!["a/file.rs".to_string()],
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The same worktree with nothing in it to commit.
    fn clean() -> StubGit {
        StubGit {
            staged: Vec::new(),
            ..StubGit::holding_work()
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
        Ok(HEAD.to_string())
    }

    fn trunk_tip(&self) -> Result<String, String> {
        self.record("trunk_tip");
        Ok(String::from("0000000000000000000000000000000000000000"))
    }

    fn staged(&self) -> Result<Vec<String>, String> {
        self.record("staged");
        Ok(self.staged.clone())
    }

    fn status(&self) -> Result<Vec<String>, String> {
        self.record("status");
        Ok(Vec::new())
    }

    fn add_all(&self) -> Result<(), String> {
        self.record("add_all");
        Ok(())
    }

    fn commit(&self, message: &str) -> Result<String, String> {
        self.record(&format!("commit {message}"));
        Ok(SHA.to_string())
    }

    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        self.record(&format!("numstat {from} {to}"));
        Ok(Vec::new())
    }
}

/// The real store with its two WRITES swallowed, so an arm can force the
/// disagreement the read-back exists to catch with the item untouched.
///
/// A gate that answers an id nothing raised and a note that lands nowhere are
/// the one failure a real store will not produce on demand, and between them
/// they leave the record byte for byte as they found it.
struct Swallowing<'a> {
    inner: &'a dyn Store,
}

impl Store for Swallowing<'_> {
    fn ready(&self) -> Result<Vec<String>, StoreError> {
        self.inner.ready()
    }

    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        self.inner.open_labelled(label)
    }

    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError> {
        self.inner.create(item, by)
    }

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_title(item, title, by)
    }

    fn show(&self, item: &str) -> Result<Item, StoreError> {
        self.inner.show(item)
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

    fn note(&self, _item: &str, _text: &str, _by: &str) -> Result<(), StoreError> {
        Ok(())
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

    fn gate(&self, _item: &str, _reason: &str, _by: &str) -> Result<String, StoreError> {
        Ok(String::from("fx-nothing"))
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
fn store() -> Board {
    let board = Board::new("gate");
    board.fleet_toml(POLICY);
    board
}

/// The integration ring: the one arm of this suite that parks through `bd`.
fn ring() -> &'static Scratch {
    let scratch = shared_store("gate");
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        scratch.fleet_toml(POLICY);
    });
    scratch
}

fn project(scratch: &dyn Rooted) -> Project {
    let table = fleet_core::item::table_at(&scratch.root().join("fleet.toml"));
    Project {
        root: scratch.root().to_path_buf(),
        name: "a-project".to_string(),
        guards: table.clone(),
        gates: table,
    }
}

fn packs(scratch: &dyn Rooted) -> Packs {
    Packs::under(scratch.packs_dir(), scratch.defaults_dir()).expect("the defaults resolve")
}

/// One item, held by this arm's own seat and carrying an order.
fn an_ordered_item(store: &dyn Store, title: &str, seat: &str) -> String {
    let item = an_item(store, title);
    store
        .assign(&item, seat, "a-flight")
        .expect("the seat holds it");
    store
        .set_orders(
            &item,
            &format!(
                r#"{{"orders": {{"by": "a-flight", "kind": "dispatch", "seat": "{seat}", "at": "{AT}"}}}}"#
            ),
            "a-flight",
        )
        .expect("the order index lands");
    item
}

/// One item filed through the trait, so the same builder fills either board.
fn an_item(store: &dyn Store, title: &str) -> String {
    store
        .create(
            &fleet_core::store::NewItem {
                title,
                description: "an item to park",
                item_type: "task",
                labels: &[],
            },
            "a-flight",
        )
        .expect("the item is filed")
}

fn a_note(scratch: &dyn Rooted, label: &str, body: &str) -> PathBuf {
    let path = scratch.root().join(format!("question-{label}.md"));
    std::fs::write(&path, body).expect("the note is written");
    path
}

fn read(store: &dyn Store, item: &str) -> Item {
    store.show(item).expect("the item reads")
}

fn notes_of(store: &dyn Store, item: &str) -> String {
    read(store, item).notes.unwrap_or_default()
}

/// What an arm varies, gathered so the call below reads as the arm and not as
/// the wiring.
struct Seams<'a> {
    store: &'a dyn Store,
    git: &'a dyn Git,
    project: &'a Project,
    packs: &'a Packs,
    events: &'a StubEvents,
}

fn ask_with(
    item: Option<&str>,
    note: &PathBuf,
    by: &str,
    seams: &Seams,
) -> Result<gate::Asked, Stop> {
    gate::ask(
        &mut Vec::new(),
        &Question {
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
            events: seams.events,
        },
    )
}

fn answer_with(
    item: &str,
    letter: &str,
    text: Option<&str>,
    by: &str,
    seams: &Seams,
) -> Result<gate::Replied, Stop> {
    gate::answer(
        &mut Vec::new(),
        &Reply {
            item,
            letter,
            text,
            by,
        },
        &Wiring {
            store: seams.store,
            git: seams.git,
            packs: seams.packs,
            project: seams.project,
            events: seams.events,
        },
    )
}

/// The park an arm needs before it can answer one: a real ask, made the way the
/// arm above it measures.
fn a_parked_item(scratch: &Board, label: &str, seat: &str) -> (String, String) {
    let item = an_ordered_item(&scratch.store, &format!("an item parked for {label}"), seat);
    let note = a_note(scratch, label, QUESTION);
    let asked = ask_with(
        None,
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the park is made");
    (item, asked.gate)
}

// ---- AC1: the ask ------------------------------------------------------------

/// THE INTEGRATION RING of this suite, and the one arm here that drives `bd`:
/// the gate is the store's OWN object, and what a real store does with one —
/// carrying the reason and taking the item off the ready set — is the half an
/// in-memory board could only agree with itself about.
#[test]
fn a_clean_ask_commits_the_whole_tree_raises_the_gate_and_parks() {
    let scratch = ring();
    let bd = &Bd::at(&scratch.root);
    let seat = "g-clean";
    let item = an_ordered_item(bd, "an item with a question on it", seat);
    let note = a_note(scratch, "clean", QUESTION);
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let mut out: Vec<u8> = Vec::new();
    let asked = gate::ask(
        &mut out,
        &Question {
            item: None,
            by: seat,
            note: &note,
            at: AT,
        },
        &Wiring {
            store: bd,
            git: &git,
            packs: &packs(scratch),
            project: &project(scratch),
            events: &events,
        },
    )
    .expect("the question is asked");

    assert_eq!(asked.item, item, "the seat's one ordered item");
    assert_eq!(asked.commit, SHA);
    assert_eq!(asked.branch, BRANCH);
    assert_eq!(
        String::from_utf8(out).expect("stdout is utf-8"),
        format!("{}\n", asked.gate),
        "stdout is the gate id and nothing else"
    );

    // (a) THE WHOLE TREE. The add-all precedes the index read, and the commit
    // subject names the item and the park.
    let calls = git.calls();
    let staged_at = calls.iter().position(|call| call == "staged");
    let added_at = calls.iter().position(|call| call == "add_all");
    assert!(
        added_at.is_some() && added_at < staged_at,
        "add_all comes before the index is read: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with(&format!("commit {item}: parked"))),
        "the commit subject names the item by its full id and the park: {calls:?}"
    );

    // (b) THE GATE, carrying the note's whole text, and the item off the ready
    // set behind it.
    // `-n 0` for the reason the store's own read carries it: this board is the
    // run's, every ring arm on it contributes gates, and a capped listing drops
    // rows without saying so.
    let gates = String::from_utf8_lossy(&scratch.bd(&["gate", "list", "--json", "-n", "0"]).stdout)
        .to_string();
    assert!(
        gates.contains(&asked.gate),
        "the store lists the gate open: {gates}"
    );
    assert!(
        gates.contains("the table the spec names is not there"),
        "and carries the question as its reason: {gates}"
    );
    let ready = bd.ready().expect("the ready read answers");
    assert!(
        !ready.contains(&item),
        "the gate takes the item off the ready set"
    );

    // (c) THE PARK REGION, with the four values and the question beneath them.
    let park = last_park(&notes_of(bd, &item)).expect("the item carries a park");
    assert_eq!(
        park, asked.note,
        "the note the store holds is the one written"
    );
    assert!(
        park.starts_with(&format!("{} {item} — ask", PARK_MARKERS[0])),
        "{park}"
    );
    for line in [
        format!("branch:  {BRANCH}"),
        format!("commit:  {SHA}"),
        format!("gate:    {}", asked.gate),
    ] {
        assert!(park.contains(&line), "the park carries `{line}`:\n{park}");
    }
    assert!(
        park.contains("the table the spec names is not there")
            && park.contains("A. build the table")
            && park.contains("B. read the value"),
        "and the question with both options beneath them:\n{park}"
    );
    assert!(
        !park
            .lines()
            .skip(1)
            .any(|line| line.starts_with("QUESTION")),
        "the question is moved OFF column zero, so no line of it can end the region:\n{park}"
    );

    // (d) THE ONE EVENT, with the five keys the table names.
    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(ITEM_PARKED);
    assert_eq!(actor, seat, "the actor is the seat that asked");
    keys_agree(ITEM_PARKED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["reason"], serde_json::json!("ask"));
    assert_eq!(payload["branch"], serde_json::json!(BRANCH));
    assert_eq!(payload["commit"], serde_json::json!(SHA));
    assert_eq!(payload["gate"], serde_json::json!(asked.gate));
}

#[test]
fn a_tree_with_nothing_to_commit_parks_on_head() {
    let scratch = &store();
    let seat = "g-empty";
    let item = an_ordered_item(&scratch.store, "an item asked about before any work", seat);
    let note = a_note(scratch, "empty", QUESTION);
    let git = StubGit::clean();

    let asked = ask_with(
        None,
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the question is asked");

    assert_eq!(asked.commit, HEAD, "the park records HEAD");
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "and nothing was committed: {:?}",
        git.calls()
    );
    let park = last_park(&notes_of(&scratch.store, &item)).expect("the item carries a park");
    assert!(park.contains(&format!("commit:  {HEAD}")), "{park}");
}

#[test]
fn the_trunk_is_refused_and_the_item_is_untouched() {
    let scratch = &store();
    let seat = "g-trunk";
    let item = an_ordered_item(&scratch.store, "an item asked about on the trunk", seat);
    let note = a_note(scratch, "trunk", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };

    let stop = ask_with(
        None,
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("the trunk is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(stop.message.contains("main"), "{}", stop.message);
    assert!(
        !git.calls().iter().any(|call| call == "add_all"),
        "nothing was staged: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

/// THE PAIR THE DISCRIMINATOR HAS TO SEPARATE, and the reason the arm below it
/// is written at all: the two items are filed the same way, named the same way
/// and asked from the same worktree on the trunk. The run label is the only
/// difference between them, so an `ask` that does not read it answers both the
/// same and one of the two arms reds.
#[test]
fn a_runs_record_parks_off_the_trunk_and_performs_no_git_act() {
    let scratch = &store();
    let seat = "g-run";
    let item = an_item(&scratch.store, "a run being asked about");
    scratch.label(&item, run::LABEL);
    scratch.set_metadata(
        &item,
        &format!(r#"{{"run": {{"hash": "{RUN_HASH}", "workflow": "takeoff"}}}}"#),
    );
    let note = a_note(scratch, "run", QUESTION);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };
    let events = StubEvents::default();

    let asked = ask_with(
        Some(&item),
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect("a run's record is parked from the trunk");

    // (a) NO GIT ACT AT ALL — not the branch read the refusal stands on, and
    // not the add and commit that would land in whoever's checkout the project
    // root is.
    assert!(
        git.calls().is_empty(),
        "git was never asked: {:?}",
        git.calls()
    );
    assert_eq!(asked.branch, gate::RUN_BRANCH);
    assert_eq!(asked.commit, RUN_HASH, "the hash the run's open pinned");

    // (b) THE PARK, carrying the two values and the gate the store raised.
    let park = last_park(&notes_of(&scratch.store, &item)).expect("the item carries a park");
    for line in [
        format!("branch:  {}", gate::RUN_BRANCH),
        format!("commit:  {RUN_HASH}"),
        format!("gate:    {}", asked.gate),
    ] {
        assert!(park.contains(&line), "the park carries `{line}`:\n{park}");
    }

    // (c) THE EVENT the SDK's gate step reads, reaching the stream exactly as a
    // seat's does.
    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(ITEM_PARKED);
    assert_eq!(actor, seat);
    keys_agree(ITEM_PARKED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["reason"], serde_json::json!("ask"));
    assert_eq!(payload["branch"], serde_json::json!(gate::RUN_BRANCH));
    assert_eq!(payload["commit"], serde_json::json!(RUN_HASH));
    assert_eq!(payload["gate"], serde_json::json!(asked.gate));
}

/// The control for the arm above: the same item, named the same way, from the
/// same worktree on the trunk, without the run label.
#[test]
fn an_item_that_is_not_a_runs_record_is_refused_the_park_on_the_trunk() {
    let scratch = &store();
    let seat = "g-not-a-run";
    let item = an_item(&scratch.store, "an item that is not a run's record");
    scratch.set_metadata(
        &item,
        &format!(r#"{{"run": {{"hash": "{RUN_HASH}", "workflow": "takeoff"}}}}"#),
    );
    let note = a_note(scratch, "not-a-run", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };

    let stop = ask_with(
        Some(&item),
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("the trunk is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(stop.message.contains("main"), "{}", stop.message);
    assert!(
        !git.calls().iter().any(|call| call == "add_all"),
        "nothing was staged: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

#[test]
fn a_seat_holding_no_ordered_item_is_refused() {
    let scratch = &store();
    let seat = "g-unordered";
    // Assigned and NOT ordered: the row is held and the order index is absent.
    let item = an_item(&scratch.store, "an item nobody ordered");
    scratch.assign(&item, seat);
    let note = a_note(scratch, "unordered", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit::holding_work();

    let stop = ask_with(
        None,
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("an unordered worktree is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(seat) && stop.message.contains(&item),
        "the refusal names the seat and what it does hold: {}",
        stop.message
    );
    assert!(
        git.calls().is_empty(),
        "git was never asked: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

#[test]
fn a_note_with_no_lettered_option_is_usage_and_names_the_grammar() {
    let scratch = &store();
    let seat = "g-options";
    let item = an_ordered_item(&scratch.store, "an item asked about with no options", seat);
    let note = a_note(
        scratch,
        "options",
        "QUESTION the table is not there — what now?\nI think we should talk about it.\n",
    );
    let before = scratch.json(&item);
    let git = StubGit::holding_work();

    let stop = ask_with(
        None,
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a question with no options is refused");

    assert_eq!(stop.code, 2, "{}", stop.message);
    assert!(
        stop.message.contains(gate::QUESTION_NOTE),
        "the refusal names the grammar: {}",
        stop.message
    );
    assert!(
        !git.calls().iter().any(|call| call == "add_all"),
        "nothing was staged: {:?}",
        git.calls()
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");

    // And a note that does not open on the marker is the same exit.
    let unmarked = a_note(scratch, "unmarked", "What should we do?\nA. one\nB. two\n");
    let stop = ask_with(
        None,
        &unmarked,
        seat,
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a note with no marker is refused");
    assert_eq!(stop.code, 2, "{}", stop.message);
    assert!(
        stop.message.contains(gate::QUESTION_MARKERS[0]),
        "{}",
        stop.message
    );
}

#[test]
fn a_read_back_that_disagrees_is_could_not_tell() {
    let scratch = &store();
    let seat = "g-readback";
    let item = an_ordered_item(&scratch.store, "an item whose park does not land", seat);
    let note = a_note(scratch, "readback", QUESTION);
    let before = scratch.json(&item);
    let events = StubEvents::default();

    let stop = ask_with(
        None,
        &note,
        seat,
        &Seams {
            store: &Swallowing {
                inner: &scratch.store,
            },
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("a park nobody can read back is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(&item) && stop.message.contains("(absent)"),
        "the message names the item and what it read: {}",
        stop.message
    );
    assert_eq!(events.count(), 0, "and nothing reached the stream");
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

/// An epic is refused by its type before any git act: no seat is ever given
/// one, and a park on one is a gate the store cannot tie to it.
#[test]
fn an_epic_is_refused_before_the_commit_and_nothing_is_written() {
    let scratch = &store();
    let seat = "g-epic";
    let item = scratch
        .store
        .create(
            &NewItem {
                title: "an epic somebody asked about",
                description: "an epic",
                item_type: "epic",
                labels: &[],
            },
            "a-flight",
        )
        .expect("the epic is filed");
    scratch.assign(&item, seat);
    let note = a_note(scratch, "epic", QUESTION);
    let before = scratch.json(&item);
    let wrote = scratch.store.wrote();
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let stop = ask_with(
        Some(&item),
        &note,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("an epic is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&item)
            && stop
                .message
                .contains("an epic is never dispatched — its children are"),
        "the refusal names the epic and why: {}",
        stop.message
    );
    assert!(
        git.calls().is_empty(),
        "git was never asked: {:?}",
        git.calls()
    );
    assert_eq!(
        scratch.store.wrote(),
        wrote,
        "the store was written nothing"
    );
    assert!(scratch.store.raised().is_empty(), "no gate was raised");
    assert_eq!(events.count(), 0, "and nothing reached the stream");
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

/// The row the gating fake answers for the item a seat holds.
fn a_held_row(item: &str, seat: &str) -> String {
    format!(
        r#"{{"id":"{item}","title":"an item whose gate fails","status":"open","issue_type":"task","assignee":"{seat}","metadata":{{"orders":{{"by":"a-flight","kind":"dispatch","seat":"{seat}","at":"{AT}"}}}}}}"#
    )
}

/// The calls the gating fake was handed, with the `-C <root>` every call opens
/// on dropped. A line that does not open on it is the rest of a reason the
/// question's own newlines split, and not a call.
fn gating_calls(log: &std::path::Path) -> Vec<String> {
    common::capped::calls(log)
        .into_iter()
        .filter(|call| call.first().map(String::as_str) == Some("-C"))
        .map(|call| call[2..].join(" "))
        .collect()
}

/// A `gate create` that files its gate and then exits 1 — bd 1.2.2 on an epic
/// — leaves an open gate blocking nothing. The park reads the open list before
/// the create and again after it, resolves what is new, and names it; a gate
/// that was open before the park is somebody else's and is left alone.
#[test]
fn a_gate_left_behind_by_a_failed_create_is_resolved_and_named() {
    let scratch = &store();
    let seat = "g-left";
    let item = "fx-left";
    let dir = common::Fixture::new("gate-left-behind");
    let log = dir.path("argv");
    let bin = gating_bd(&dir, &a_held_row(item, seat), &["g-before"], true, &log);
    let bd = Bd::at_bin(scratch.root(), &bin);
    let note = a_note(scratch, "left", QUESTION);
    let events = StubEvents::default();

    let stop = ask_with(
        Some(item),
        &note,
        seat,
        &Seams {
            store: &bd,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .expect_err("a gate that was not raised is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(&format!(
            "the store raised the gate {LEFT_BEHIND} all the same, and it is withdrawn"
        )),
        "the refusal names the gate it resolved: {}",
        stop.message
    );
    assert!(
        stop.message.contains(SHA) && stop.message.contains(&format!("{item} carries no park")),
        "and still says what stands: {}",
        stop.message
    );
    assert!(
        !stop.message.contains("g-before"),
        "a gate open before the park is not this park's: {}",
        stop.message
    );
    assert_eq!(
        standing(&dir),
        vec![String::from("g-before")],
        "the gate left behind is resolved and the one before it stands"
    );

    let calls = gating_calls(&log);
    let at = |prefix: &str| {
        calls
            .iter()
            .position(|call| call.starts_with(prefix))
            .unwrap_or_else(|| panic!("no `{prefix}` in {calls:?}"))
    };
    let listed: Vec<usize> = calls
        .iter()
        .enumerate()
        .filter(|(_, call)| call.starts_with("gate list"))
        .map(|(n, _)| n)
        .collect();
    let created = at("gate create");
    assert!(
        listed.len() == 2 && listed[0] < created && created < listed[1],
        "the open list is read once before the create and once after it: {calls:?}"
    );
    assert!(
        at(&format!("gate resolve {LEFT_BEHIND}")) > listed[1],
        "{calls:?}"
    );
    assert!(
        !calls
            .iter()
            .any(|call| call.starts_with("gate resolve g-before")),
        "{calls:?}"
    );
    assert_eq!(events.count(), 0, "nothing reached the stream");
}

/// The same failure where the resolve fails too: the gate stands, and the
/// refusal names it with the command that resolves it.
#[test]
fn a_gate_left_behind_that_cannot_be_resolved_is_named_with_its_command() {
    let scratch = &store();
    let seat = "g-left-stands";
    let item = "fx-left-stands";
    let dir = common::Fixture::new("gate-left-stands");
    let log = dir.path("argv");
    let bin = gating_bd(&dir, &a_held_row(item, seat), &[], false, &log);
    let bd = Bd::at_bin(scratch.root(), &bin);
    let note = a_note(scratch, "left-stands", QUESTION);

    let stop = ask_with(
        Some(item),
        &note,
        seat,
        &Seams {
            store: &bd,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a gate that was not raised is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(&format!(
            "the store raised the gate {LEFT_BEHIND} all the same, and it STANDS with no park \
             naming it"
        )) && stop
            .message
            .contains(&format!("`bd gate resolve {LEFT_BEHIND}` resolves it")),
        "the refusal names the gate and the command: {}",
        stop.message
    );
    assert_eq!(standing(&dir), vec![String::from(LEFT_BEHIND)]);
}

// ---- AC2: the answer ---------------------------------------------------------

#[test]
fn an_answer_writes_the_region_resolves_the_gate_and_announces_it() {
    let scratch = &store();
    let seat = "g-answer";
    let (item, gate_id) = a_parked_item(scratch, "answer", seat);
    let events = StubEvents::default();

    let mut out: Vec<u8> = Vec::new();
    let replied = gate::answer(
        &mut out,
        &Reply {
            item: &item,
            letter: "A",
            text: None,
            by: "a-person",
        },
        &Wiring {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            packs: &packs(scratch),
            project: &project(scratch),
            events: &events,
        },
    )
    .expect("the answer is written");

    assert_eq!(replied.gate, gate_id, "the gate the park named");
    assert_eq!(replied.letter, "A");

    let answered =
        last_answer(&notes_of(&scratch.store, &item)).expect("the item carries an answer");
    assert_eq!(answered, replied.note);
    assert!(
        answered.starts_with(&format!("{} {gate_id} — a-person", ANSWER_MARKERS[0])),
        "{answered}"
    );
    assert!(answered.contains("letter:  A"), "{answered}");
    assert!(
        answered.contains("text:    (none)"),
        "an unsaid text is `(none)` and not a blank line:\n{answered}"
    );

    // The gate is off the open list and the item is ready again.
    assert!(
        !scratch
            .store
            .open_gates()
            .expect("the open list answers")
            .contains(&gate_id),
        "the gate is resolved"
    );
    assert!(
        scratch
            .store
            .ready()
            .expect("the ready read answers")
            .contains(&item),
        "and the item is back in the ready set"
    );

    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(GATE_RESOLVED);
    assert_eq!(actor, "a-person");
    keys_agree(GATE_RESOLVED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["gate"], serde_json::json!(gate_id));
    assert_eq!(payload["letter"], serde_json::json!("A"));
    assert!(
        String::from_utf8(out)
            .expect("stdout is utf-8")
            .contains(&gate_id),
        "and the line says which gate was resolved"
    );

    // THE PARK REGION IS STILL READABLE UNDER THE ANSWER. The answer ends it
    // rather than extending it, so the question and its options are unchanged.
    let park = last_park(&notes_of(&scratch.store, &item)).expect("the park is still there");
    assert!(
        !park.contains(ANSWER_MARKERS[0]),
        "the park region stops at the answer:\n{park}"
    );
    assert!(park.contains("A. build the table"), "{park}");
}

#[test]
fn an_unnamed_letter_is_accepted_with_text_and_refused_without_it() {
    let scratch = &store();

    // With --text: the letter the options do not name is what a person saw
    // that the seat did not, and the text is on the note.
    let seat = "g-text";
    let (item, gate_id) = a_parked_item(scratch, "text", seat);
    let replied = answer_with(
        &item,
        "C",
        Some("neither — the spec is wrong and the bead is going back"),
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("an unnamed letter with text is an answer");
    assert_eq!(replied.gate, gate_id);
    let answered =
        last_answer(&notes_of(&scratch.store, &item)).expect("the item carries an answer");
    assert!(answered.contains("letter:  C"), "{answered}");
    assert!(
        answered.contains("the spec is wrong and the bead is going back"),
        "{answered}"
    );

    // Without it: usage, and the item untouched.
    let seat = "g-notext";
    let (item, _) = a_parked_item(scratch, "notext", seat);
    let before = scratch.json(&item);
    let stop = answer_with(
        &item,
        "C",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("an unnamed letter with nothing said is refused");
    assert_eq!(stop.code, 2, "{}", stop.message);
    assert!(
        stop.message.contains("--text") && stop.message.contains("A, B"),
        "the refusal names the options and the way past them: {}",
        stop.message
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

#[test]
fn no_park_and_a_gate_already_resolved_are_both_refused() {
    let scratch = &store();

    // An item nobody parked.
    let unparked = scratch.item("an item nobody parked");
    let before = scratch.json(&unparked);
    let stop = answer_with(
        &unparked,
        "A",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("an item with no park is refused");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(stop.message.contains(&unparked), "{}", stop.message);
    assert_eq!(
        before,
        scratch.json(&unparked),
        "the item is byte-identical"
    );

    // And a second answer on one item, whose gate the store does not list open.
    let seat = "g-twice";
    let (item, gate_id) = a_parked_item(scratch, "twice", seat);
    answer_with(
        &item,
        "A",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the first answer lands");
    let before = scratch.json(&item);
    let stop = answer_with(
        &item,
        "B",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a gate already resolved is refused");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&gate_id),
        "the refusal names the gate: {}",
        stop.message
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

// ---- the crash cap's park ----------------------------------------------------

/// The reason the controller's run pass hands the park, in its own words.
const CAPPED_REASON: &str = "fx-capped has been executed 3 time(s) and nothing could classify \
                             the last one — `[core.run] max_crashes` is 2";

/// A run's record the way a run's open files it: the run label, and the hash
/// the open pinned.
fn a_runs_record(scratch: &Board, title: &str) -> String {
    let run = an_item(&scratch.store, title);
    scratch.label(&run, run::LABEL);
    scratch.set_metadata(
        &run,
        &format!(r#"{{"run": {{"hash": "{RUN_HASH}", "workflow": "takeoff"}}}}"#),
    );
    run
}

/// A run parked at `[core.run] max_crashes` is ANSWERABLE: the park carries the
/// PARKED note `fleet answer` reads — the run's branch, the hash its open
/// pinned, the gate the store raised, and a question with lettered options —
/// so the answer resolves the gate the cap raised and the record is no longer
/// blocked by it.
///
/// THE ONE PARK THAT HAD NO NOTE. A gate raised with nothing on the record
/// naming it is a question `fleet answer` refuses as "carries no park", and the
/// open gate blocks the record's close as well, so the run held its
/// `[core.run] max_open` slot for good.
#[test]
fn a_run_parked_at_the_crash_cap_is_answered_like_any_other_park() {
    let scratch = &store();
    let run = a_runs_record(scratch, "a run nothing could classify");
    let directory = scratch.root.join("runs").join(&run);

    let gate_id = gate::park_at_the_cap(
        &gate::Capped {
            run: &run,
            reason: CAPPED_REASON,
            directory: &directory,
            by: "controller",
        },
        &scratch.store,
        &packs(scratch),
    )
    .expect("the park is made");

    // (a) THE ANSWER, which is what a person meets first.
    let events = StubEvents::default();
    let replied = answer_with(
        &run,
        "B",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &events,
        },
    )
    .unwrap_or_else(|stop| panic!("the crash cap's park is answerable: {}", stop.message));
    assert_eq!(
        replied.gate, gate_id,
        "the answer resolves the gate the cap raised"
    );
    assert!(
        !scratch
            .store
            .open_gates()
            .expect("the open list answers")
            .contains(&gate_id),
        "and the store lists it open no longer"
    );
    let (_, payload) = events.one(GATE_RESOLVED);
    assert_eq!(payload["item"], serde_json::json!(run));

    // (b) THE PARK IT ANSWERED: a run's, standing on the hash its open pinned.
    let park = last_park(&notes_of(&scratch.store, &run)).expect("the record carries a park");
    for line in [
        format!("{} {run} — {}", PARK_MARKERS[0], gate::CAPPED),
        format!("branch:  {}", gate::RUN_BRANCH),
        format!("commit:  {RUN_HASH}"),
        format!("gate:    {gate_id}"),
    ] {
        assert!(park.contains(&line), "the park carries `{line}`:\n{park}");
    }
    assert!(
        park.contains(CAPPED_REASON),
        "the question is the pass's own reading:\n{park}"
    );
    assert!(
        park.contains(&directory.display().to_string()),
        "and says where the logs are:\n{park}"
    );
    assert!(
        park.contains(&format!("`fleet cancel {run}`")),
        "and names the verb that ends the run:\n{park}"
    );
    let raised = scratch.store.raised();
    assert_eq!(raised.len(), 1, "one gate: {raised:?}");
    let letters: Vec<char> = gate::options_in(&raised[0].1)
        .into_iter()
        .map(|(letter, _)| letter)
        .collect();
    assert_eq!(
        letters,
        vec!['A', 'B'],
        "the gate's reason is the whole question, options and all, as a seat's is: {}",
        raised[0].1
    );
}

// ---- R22: what the resume reads off the record -------------------------------

#[test]
fn the_resume_reads_the_park_and_the_answer_that_stands_over_it() {
    let scratch = &store();
    let seat = "g-resume";
    let (item, _) = a_parked_item(scratch, "resume", seat);

    // Before the answer there is no resume: a park nobody has settled resumes
    // nothing, which is the state the store's own gate keeps the item out of
    // the ready set for.
    assert_eq!(
        gate::resume_of(&notes_of(&scratch.store, &item)),
        None,
        "an unanswered park is no resume"
    );

    answer_with(
        &item,
        "B",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the answer lands");

    let resume = gate::resume_of(&notes_of(&scratch.store, &item)).expect("the park is answered");
    assert_eq!(resume.branch, BRANCH);
    assert_eq!(resume.commit, SHA);
    assert_eq!(resume.letter, "B");
    assert_eq!(
        resume.question,
        "the table the spec names is not there — what replaces it?"
    );
    assert_eq!(
        resume.options,
        vec![
            "A. build the table, as the spec assumes".to_string(),
            "B. read the value off the item instead".to_string(),
        ]
    );
    assert_eq!(
        resume.answer, "B. read the value off the item instead",
        "the answer is the letter with the option it named"
    );

    // The section the brief carries, from the pack's own template.
    let section =
        gate::resume_section(&packs(scratch), &item, &resume).expect("the section renders");
    assert!(
        section.starts_with(&format!(
            "{} {item} — cut from {SHA} on {BRANCH}",
            gate::RESUME_SECTION
        )),
        "{section}"
    );
    for line in [
        "question: the table the spec names is not there",
        "A. build the table, as the spec assumes",
        "B. read the value off the item instead",
        "answer:   B. read the value off the item instead",
    ] {
        assert!(
            section.contains(line),
            "the section carries `{line}`:\n{section}"
        );
    }
}

#[test]
fn a_verdict_that_accepted_the_parked_commit_reads_as_accepted() {
    let scratch = &store();
    let seat = "g-accepted";
    let (item, _) = a_parked_item(scratch, "accepted", seat);
    let notes = notes_of(&scratch.store, &item);

    // Unanswered, and with no verdict: nothing to land.
    assert_eq!(gate::accepted_over_the_park(&notes), None);

    answer_with(
        &item,
        "A",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            packs: &packs(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the answer lands");
    assert_eq!(
        gate::accepted_over_the_park(&notes_of(&scratch.store, &item)),
        None,
        "an answered park with no verdict behind it is still a builder's work"
    );

    // A verdict naming SOME OTHER commit does not reach this park.
    let elsewhere = format!("ACCEPTED {HEAD} — a-reviewer\nitem:    {item}\nsize: 1 file(s)\n");
    assert!(scratch.store.note(&item, &elsewhere, "a-reviewer").is_ok());
    assert_eq!(
        gate::accepted_over_the_park(&notes_of(&scratch.store, &item)),
        None,
        "a verdict over another commit is another commit's"
    );

    // And one naming the PARKED commit is the landing this item is owed.
    let here = format!("ACCEPTED {SHA} — a-reviewer\nitem:    {item}\nsize: 1 file(s)\n");
    assert!(scratch.store.note(&item, &here, "a-reviewer").is_ok());
    assert_eq!(
        gate::accepted_over_the_park(&notes_of(&scratch.store, &item)).as_deref(),
        Some(SHA),
        "the verdict stands over the commit the park preserved"
    );
}

#[test]
fn a_declared_gate_is_answered_once_and_only_at_its_own_step() {
    let notes = "\
PARKED fx-1 — gate review
branch:  a/b
commit:  1111111
gate:    fx-g1
ANSWERED fx-g1 — a-person
letter:  A
text:    (none)
";
    assert!(gate::answered_at(notes, "gate review"));
    assert!(
        !gate::answered_at(notes, "gate delivery"),
        "a park at another step is not this step's"
    );
    assert!(
        !gate::answered_at("PARKED fx-1 — gate review\ngate:    fx-g1\n", "gate review"),
        "a park nobody has answered is not answered"
    );
}

// ---- AC5: the defaults -------------------------------------------------------

#[test]
fn the_defaults_carry_both_templates_and_the_registry_names_them() {
    let scratch = Board::new("gate-pack");
    let installed = scratch.defaults_dir.clone();
    for (slot, marker) in [
        (gate::QUESTION_NOTE, gate::QUESTION_MARKERS[0]),
        (gate::ANSWER_NOTE, ANSWER_MARKERS[0]),
    ] {
        let body = std::fs::read_to_string(installed.join(slot)).expect("the template is readable");
        assert!(
            body.starts_with(marker),
            "`{slot}` opens on its own marker:\n{body}"
        );
    }

    // The RESUME block rides the park note, which is the template the park's
    // own grammar lives in.
    let park = std::fs::read_to_string(installed.join("assets/park-note.md"))
        .expect("the park note is readable");
    assert!(
        fleet_core::item::marker_block(&park, gate::RESUME_SECTION).is_some(),
        "the park note carries a `{}` block:\n{park}",
        gate::RESUME_SECTION
    );

    let registry = std::fs::read_to_string(installed.join("assets/shadow-registry.toml"))
        .expect("the registry is readable");
    for slot in [gate::QUESTION_NOTE, gate::ANSWER_NOTE] {
        assert!(
            registry.contains(slot),
            "and the registry names `{slot}`:\n{registry}"
        );
    }
}
