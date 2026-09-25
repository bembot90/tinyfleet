//! `fleet hold` and `fleet clear` against a real work graph and a git seam that
//! answers.
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

use common::holding::{holding_bd, standing, LEFT_BEHIND};
use common::{full, keys_agree, seat_actor, shared_store, signal, Rooted, Scratch, StubEvents};
use fleet_core::entry::{self, Body, Choice, Entry, HoldReason, Timeline};
use fleet_core::input::{Checked, QuestionInput, QUESTION_SCHEMA};
use fleet_core::item::hold::{self, Clearance, Question, Wiring};
use fleet_core::item::run;
use fleet_core::item::{Change, Git, Project, Stop, ITEM_ENTRY};
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::store::bd::Bd;
use fleet_core::store::{Filter, Item, NewItem, Store, StoreError};
use fleet_core::test_support::Board;

const AT: &str = "2026-09-13T04:05:06Z";
const SHA: &str = "3333333333333333333333333333333333333333";
const HEAD: &str = "4444444444444444444444444444444444444444";
const BRANCH: &str = "a-seat/feat/the-work";
/// The hash a run's open pinned, which its park stands on where a seat's park
/// stands on a commit.
const RUN_HASH: &str = "5555555555555555555555555555555555555555";
const POLICY: &str = "[core]\nreviewer = \"a-reviewer\"\n";

/// The question a seat hands in, a JSON file of the shape
/// `assets/question.schema.json` gives.
const QUESTION: &str = r#"{
  "question": "the table the spec names is not there — what replaces it?",
  "options": [
    {"letter": "A", "text": "build the table, as the spec assumes"},
    {"letter": "B", "text": "read the value off the item instead"}
  ]
}"#;

// ---- the seams ---------------------------------------------------------------

struct StubGit {
    branch: String,
    /// What `staged` answers AFTER `add_all` has been called.
    staged: Vec<String>,
    /// What `commit` answers.
    made: String,
    calls: Mutex<Vec<String>>,
}

impl StubGit {
    /// A worktree on a work branch holding work: the shape a question is asked
    /// from.
    fn holding_work() -> StubGit {
        StubGit {
            branch: BRANCH.to_string(),
            staged: vec!["a/file.rs".to_string()],
            made: SHA.to_string(),
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
        Ok(self.made.clone())
    }

    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        self.record(&format!("numstat {from} {to}"));
        Ok(Vec::new())
    }
}

/// The real store with its two WRITES swallowed, so an arm can force the
/// disagreement the read-back exists to catch with the item untouched.
///
/// A hold that answers an id nothing raised and an append that answers an id
/// it kept nothing under are the one failure a real store will not produce on
/// demand, and between them they leave the record byte for byte as they found
/// it.
struct Swallowing<'a> {
    inner: &'a dyn Store,
}

impl Store for Swallowing<'_> {
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
        self.inner.show(item)
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
        self.inner.order_set(id, order, by)
    }

    fn order_withdraw(
        &self,
        id: &fleet_core::store::ItemId,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.order_withdraw(id, by)
    }

    fn run_set(
        &self,
        id: &fleet_core::store::ItemId,
        run: &fleet_core::store::RunRecord,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        self.inner.run_set(id, run, by)
    }

    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.inner.reopen(item, by)
    }

    fn hold_raise(
        &self,
        _id: &fleet_core::store::ItemId,
        _reason: &str,
        _by: &fleet_core::seat::actor::Actor,
    ) -> Result<fleet_core::store::HoldId, StoreError> {
        Ok(fleet_core::store::HoldId::from("fx-nothing"))
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
        _item: &fleet_core::store::ItemId,
        _body: &Body,
        _by: &fleet_core::seat::actor::Actor,
    ) -> Result<String, StoreError> {
        Ok(String::from("c-nothing"))
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

/// One board per arm, held in memory.
fn store() -> Board {
    let board = Board::new("hold");
    board.fleet_toml(POLICY);
    board
}

/// The integration ring: the one arm of this suite that parks through `bd`.
fn ring() -> &'static Scratch {
    let scratch = shared_store("hold");
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
        policy: table,
    }
}

/// One item, held by this arm's own seat and carrying an order.
fn an_ordered_item(store: &dyn Store, title: &str, seat: &str) -> String {
    let item = an_item(store, title);
    let seat = full(seat);
    store
        .update(
            &fleet_core::store::ItemId::from(item.as_str()),
            &fleet_core::store::Update::assignee(
                fleet_core::seat::identity::SeatId::parse(&seat).expect("an arm's seat id parses"),
            ),
            &fleet_core::test_support::the_test(),
        )
        .expect("the seat holds it");
    store
        .order_set(
            &fleet_core::store::ItemId::from(item.as_str()),
            &common::a_dispatch("run:a-flight", Some(&seat), AT),
            &fleet_core::test_support::the_test(),
        )
        .expect("the order lands");
    item
}

/// One item filed through the trait, so the same builder fills either board.
fn an_item(store: &dyn Store, title: &str) -> String {
    store
        .create(
            &fleet_core::store::NewItem {
                title: title.to_string(),
                description: String::from("an item to park"),
                item_type: String::from("task"),
                labels: Vec::new(),
                priority: None,
            },
            &fleet_core::test_support::the_test(),
        )
        .expect("the item is filed")
        .to_string()
}

fn a_question(scratch: &dyn Rooted, label: &str, body: &str) -> PathBuf {
    let path = scratch.root().join(format!("question-{label}.json"));
    std::fs::write(&path, body).expect("the question is written");
    path
}

/// The question a file holds, read as the verb reads it.
fn asked(body: &str) -> QuestionInput {
    serde_json::from_str(body).expect("the question is JSON of its shape")
}

fn timeline_of(store: &dyn Store, item: &str) -> Vec<Entry> {
    store
        .timeline(&fleet_core::store::ItemId::from(item))
        .expect("the timeline reads")
}

/// The one entry the store keeps under `id`, which the verb answered.
fn entry_of(store: &dyn Store, item: &str, id: &str) -> Entry {
    let entries = timeline_of(store, item);
    Timeline(&entries)
        .entry(id)
        .unwrap_or_else(|| panic!("{item}'s timeline holds {id}: {entries:?}"))
        .clone()
}

/// The options [`QUESTION`] offers, as the held entry carries them.
fn question_options() -> Vec<Choice> {
    asked(QUESTION).options
}

/// What an arm varies, gathered so the call below reads as the arm and not as
/// the wiring.
struct Seams<'a> {
    store: &'a dyn Store,
    git: &'a dyn Git,
    project: &'a Project,
    events: &'a StubEvents,
}

fn hold_with(
    item: Option<&str>,
    question: &PathBuf,
    by: &str,
    seams: &Seams,
) -> Result<hold::Held, Stop> {
    hold::hold(
        &mut Vec::new(),
        &Question {
            item,
            // The arm's own seat, by the name its id is derived from.
            by: &seat_actor(by),
            question,
            at: AT,
        },
        &Wiring {
            store: seams.store,
            git: seams.git,
            project: seams.project,
            events: seams.events,
        },
    )
}

fn clear_with(
    item: &str,
    letter: &str,
    text: Option<&str>,
    by: &str,
    seams: &Seams,
) -> Result<hold::Cleared, Stop> {
    hold::clear(
        &mut Vec::new(),
        &Clearance {
            item,
            letter,
            text,
            by: &seat_actor(by),
        },
        &Wiring {
            store: seams.store,
            git: seams.git,
            project: seams.project,
            events: seams.events,
        },
    )
}

/// The park an arm needs before it can clear one: a real hold, made the way the
/// arm above it measures.
fn a_held_item(scratch: &Board, label: &str, seat: &str) -> (String, String) {
    let item = an_ordered_item(&scratch.store, &format!("an item held for {label}"), seat);
    let question = a_question(scratch, label, QUESTION);
    let held = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the park is made");
    (item, held.hold)
}

// ---- AC1: the hold -----------------------------------------------------------

/// THE INTEGRATION RING of this suite, and the one arm here that drives `bd`:
/// the hold is the store's OWN object, and what a real store does with one —
/// carrying the reason and taking the item off the ready set — is the half an
/// in-memory board could only agree with itself about.
#[test]
fn a_clean_hold_commits_the_whole_tree_raises_the_hold_and_parks() {
    let scratch = ring();
    let bd = &Bd::at(&scratch.root);
    let seat = "g-clean";
    let item = an_ordered_item(bd, "an item with a question on it", seat);
    let question = a_question(scratch, "clean", QUESTION);
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let mut out: Vec<u8> = Vec::new();
    let held = hold::hold(
        &mut out,
        &Question {
            item: None,
            by: &seat_actor(seat),
            question: &question,
            at: AT,
        },
        &Wiring {
            store: bd,
            git: &git,
            project: &project(scratch),
            events: &events,
        },
    )
    .expect("the question is asked");

    assert_eq!(held.item, item, "the seat's one ordered item");
    assert_eq!(held.commit, SHA);
    assert_eq!(held.branch, BRANCH);
    assert_eq!(
        String::from_utf8(out).expect("stdout is utf-8"),
        format!("{}\n", held.hold),
        "stdout is the hold id and nothing else"
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
            .any(|call| call.starts_with(&format!("commit {item}: held"))),
        "the commit subject names the item by its full id and the park: {calls:?}"
    );

    // (b) THE HOLD, carrying the question's whole text, and the item off the
    // ready set behind it.
    // `-n 0` for the reason the store's own read carries it: this board is the
    // run's, every ring arm on it contributes holds, and a capped listing drops
    // rows without saying so.
    let holds = String::from_utf8_lossy(&scratch.bd(&["gate", "list", "--json", "-n", "0"]).stdout)
        .to_string();
    assert!(
        holds.contains(&held.hold),
        "the store lists the hold open: {holds}"
    );
    // The text as the listing's JSON spells it, its newlines escaped.
    let spelled = serde_json::to_string(&hold::question_text(&asked(QUESTION))).expect("it writes");
    assert!(
        holds.contains(spelled.trim_matches('"')),
        "and carries the question's whole text as its reason, options and all: {holds}"
    );
    let ready = bd.list(&Filter::Ready).expect("the ready read answers");
    assert!(
        !ready.iter().any(|row| row.id == item),
        "the hold takes the item off the ready set"
    );

    // (c) THE HELD ENTRY, by the seat, on bd's own comments: the hold, the
    // question and its options, and the branch and the whole commit it stopped
    // on.
    let entry = entry_of(bd, &item, &held.entry);
    assert_eq!(entry.by, seat_actor(seat), "by the seat that asked");
    assert_eq!(
        entry.body,
        Body::Held(entry::Held {
            hold: held.hold.clone(),
            reason: HoldReason::Ask,
            question: asked(QUESTION).question,
            context: None,
            options: question_options(),
            branch: Some(BRANCH.to_string()),
            commit: Some(SHA.to_string()),
            run_hash: None,
            about: None,
        })
    );

    // (d) THE ONE EVENT: the held entry's signal, by the seat that asked,
    // typed. The reason, the branch, the commit and the hold are the entry's.
    assert_eq!(
        events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            seat_actor(seat).to_string(),
            signal(&item, &entry.id, "held"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &events.all()[0].2, &[]);
}

/// `--item` naming its item by a suffix parks under the full id: the verb
/// resolves the argument once and every write, the hold, the commit and the
/// event carry the id the store answered.
#[test]
fn an_item_named_by_its_suffix_is_held_under_its_full_id() {
    let scratch = &store();
    let seat = "g-suffix";
    let item = an_ordered_item(&scratch.store, "an item named by its suffix", seat);
    let suffix = item.strip_prefix("fx-").expect("the board files under fx-");
    scratch.forget_writes();
    let question = a_question(scratch, "suffix", QUESTION);
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let held = hold_with(
        Some(suffix),
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &events,
        },
    )
    .expect("the question is asked");

    assert_eq!(held.item, item);
    let wrote = scratch.store.wrote();
    for verb in ["hold_raise", "append"] {
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
            .any(|call| call.starts_with(&format!("commit {item}: held"))),
        "the commit subject names the full id: {:?}",
        git.calls()
    );
    let entries = timeline_of(&scratch.store, &item);
    assert!(
        Timeline(&entries).held(&held.hold).is_some(),
        "the held entry is on {item}: {entries:?}"
    );
    let (_, payload) = events.one(ITEM_ENTRY);
    assert_eq!(payload, signal(&item, &held.entry, "held"));
}

#[test]
fn a_tree_with_nothing_to_commit_parks_on_head() {
    let scratch = &store();
    let seat = "g-empty";
    let item = an_ordered_item(&scratch.store, "an item asked about before any work", seat);
    let question = a_question(scratch, "empty", QUESTION);
    let git = StubGit::clean();

    let held = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the question is asked");

    assert_eq!(held.commit, HEAD, "the park records HEAD");
    assert!(
        !git.calls().iter().any(|call| call.starts_with("commit ")),
        "and nothing was committed: {:?}",
        git.calls()
    );
    let Body::Held(entry) = entry_of(&scratch.store, &item, &held.entry).body else {
        panic!("the entry is a held one");
    };
    assert_eq!(
        entry.commit.as_deref(),
        Some(HEAD),
        "the entry stands on HEAD"
    );
}

#[test]
fn the_trunk_is_refused_and_the_item_is_untouched() {
    let scratch = &store();
    let seat = "g-trunk";
    let item = an_ordered_item(&scratch.store, "an item asked about on the trunk", seat);
    let question = a_question(scratch, "trunk", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };

    let stop = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
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
/// and asked from the same worktree on the trunk. The run label is what
/// separates them, so a `hold` that does not read it answers both the same and
/// one of the two arms reds.
///
/// The record is asked about BY ITS OWN RUN, the SDK's `hold --item <run>`
/// (fleet-pl6 (a)): a run's record is the one item a run holds, and the holder
/// check on `--item` passes it.
#[test]
fn a_runs_record_parks_off_the_trunk_and_performs_no_git_act() {
    let scratch = &store();
    let item = an_item(&scratch.store, "a run being asked about");
    scratch.label(&item, run::LABEL);
    scratch.set_metadata(
        &item,
        &format!(r#"{{"fleet.run": {{"v": 1, "hash": "{RUN_HASH}", "workflow": "takeoff", "pack": "ts", "entry": "takeoff.ts", "started_at": "{AT}"}}}}"#),
    );
    let its_run = Actor {
        kind: ActorKind::Run,
        id: item.clone(),
    };
    let question = a_question(scratch, "run", QUESTION);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };
    let events = StubEvents::default();

    let held = hold::hold(
        &mut Vec::new(),
        &Question {
            item: Some(&item),
            by: &its_run,
            question: &question,
            at: AT,
        },
        &Wiring {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &events,
        },
    )
    .expect("a run's record is held from the trunk");

    // (a) NO GIT ACT AT ALL — not the branch read the refusal stands on, and
    // not the add and commit that would land in whoever's checkout the project
    // root is.
    assert!(
        git.calls().is_empty(),
        "git was never asked: {:?}",
        git.calls()
    );
    assert_eq!(held.branch, hold::RUN_BRANCH);
    assert_eq!(held.commit, RUN_HASH, "the hash the run's open pinned");

    // (b) THE HELD ENTRY, by the run, standing on the hash its open pinned and
    // on no branch or commit.
    let entry = entry_of(&scratch.store, &item, &held.entry);
    assert_eq!(entry.by, its_run);
    assert_eq!(
        entry.body,
        Body::Held(entry::Held {
            hold: held.hold.clone(),
            reason: HoldReason::Ask,
            question: asked(QUESTION).question,
            context: None,
            options: question_options(),
            branch: None,
            commit: None,
            run_hash: Some(RUN_HASH.to_string()),
            about: None,
        })
    );

    // (c) THE SIGNAL, reaching the stream exactly as a seat's does, by the
    // run.
    assert_eq!(
        events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            its_run.to_string(),
            signal(&item, &held.entry, "held"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &events.all()[0].2, &[]);

    // A SEAT naming the same record is refused before anything: the record is
    // its own run's to hold (fleet-pl6 (a)).
    let git = StubGit::holding_work();
    let stop = hold_with(
        Some(&item),
        &question,
        "g-run",
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a seat does not hold a run's record");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.starts_with(&format!(
            "{item} is run {item}'s record, and {} is not that run",
            seat_actor("g-run")
        )),
        "{}",
        stop.message
    );
    assert!(git.calls().is_empty(), "git was never asked");
}

/// The control for the arm above: the same item, named the same way, from the
/// same worktree on the trunk, without the run label — held by the seat asking.
#[test]
fn an_item_that_is_not_a_runs_record_is_refused_the_park_on_the_trunk() {
    let scratch = &store();
    let seat = "g-not-a-run";
    let item = an_ordered_item(&scratch.store, "an item that is not a run's record", seat);
    scratch.set_metadata(
        &item,
        &format!(r#"{{"fleet.run": {{"v": 1, "hash": "{RUN_HASH}", "workflow": "takeoff", "pack": "ts", "entry": "takeoff.ts", "started_at": "{AT}"}}}}"#),
    );
    let question = a_question(scratch, "not-a-run", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };

    let stop = hold_with(
        Some(&item),
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
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

/// The bare `run` label is another writer's word and not fleet's run label: a
/// seat's item carrying it, and a bare `run` key beside it, is held like any
/// seat's item — the whole tree committed on the work branch — and never as a
/// run's park, which touches no git (fleet-4j6 AC3).
#[test]
fn an_item_labelled_bare_run_is_held_and_commits_like_any_seats_item() {
    let scratch = &store();
    let seat = "g-bare-run";
    let item = an_ordered_item(&scratch.store, "an item another tool labels run", seat);
    scratch.label(&item, common::FOREIGN_LABEL);
    scratch.set_metadata(
        &item,
        &format!(r#"{{"run": {{"hash": "{RUN_HASH}", "workflow": "theirs"}}}}"#),
    );
    let question = a_question(scratch, "bare-run", QUESTION);
    let git = StubGit::holding_work();

    let held = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect("the question is asked");

    assert_eq!(held.item, item, "the seat's one ordered item");
    assert_eq!(held.branch, BRANCH, "the work branch, not a run's");
    assert_eq!(held.commit, SHA, "the commit of the tree, not a run's hash");
    let calls = git.calls();
    assert!(
        calls.iter().any(|call| call == "add_all")
            && calls
                .iter()
                .any(|call| call.starts_with(&format!("commit {item}: held"))),
        "the whole tree is committed: {calls:?}"
    );
}

#[test]
fn a_seat_holding_no_ordered_item_is_refused() {
    let scratch = &store();
    let seat = "g-unordered";
    // Assigned and NOT ordered: the row is held and the order index is absent.
    let item = an_item(&scratch.store, "an item nobody ordered");
    scratch.hand_to(&item, &full(seat));
    let question = a_question(scratch, "unordered", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit::holding_work();

    let stop = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("an unordered worktree is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&seat_actor(seat).to_string()) && stop.message.contains(&item),
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

/// A JSON QUESTION IS A HOLD, AND ONE A PERSON CLEARS. The store's hold
/// carries [`hold::question_text`] as its reason — the question, its context
/// on the next line, one `<letter>. <text>` line per option — and the held
/// entry carries the question typed, context and all, which `fleet clear`
/// checks the letter against.
#[test]
fn a_json_question_is_held_under_its_text_and_cleared_by_its_letter() {
    let scratch = &store();
    let seat = "g-json";
    let item = an_ordered_item(&scratch.store, "an item asked about in JSON", seat);
    let body = r#"{
  "question": "Which table does the value come from?",
  "context": "The spec names one the tree does not have.",
  "options": [
    {"letter": "A", "text": "build the table"},
    {"letter": "B", "text": "read the value off the item"}
  ]
}"#;
    let question = a_question(scratch, "json", body);
    let git = StubGit::holding_work();

    let held = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .unwrap_or_else(|stop| panic!("a JSON question is held: {}", stop.message));
    assert_eq!(held.item, item);

    let text = hold::question_text(&asked(body));
    assert_eq!(
        text,
        "Which table does the value come from?\n\
         The spec names one the tree does not have.\n\
         A. build the table\n\
         B. read the value off the item"
    );
    let raised = scratch.store.raised();
    assert_eq!(raised.len(), 1, "one hold: {raised:?}");
    assert_eq!(
        raised[0],
        (item.clone(), text),
        "the store hold's reason is the question's text"
    );

    let Body::Held(entry) = entry_of(&scratch.store, &item, &held.entry).body else {
        panic!("the entry is a held one");
    };
    assert_eq!(
        entry.context.as_deref(),
        Some("The spec names one the tree does not have."),
        "the context rides the entry"
    );
    assert_eq!(entry.options, asked(body).options);

    // THE HELD ENTRY IS WHAT A CLEARANCE READS.
    let cleared = clear_with(
        &item,
        "B",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .unwrap_or_else(|stop| panic!("the held entry is answerable: {}", stop.message));
    assert_eq!(cleared.hold, held.hold);
    assert_eq!(cleared.letter, "B");
    assert!(
        !scratch
            .store
            .holds_open()
            .expect("the open list answers")
            .iter()
            .any(|open| *open == *held.hold),
        "the hold is cleared"
    );
}

/// A FILE THAT DOES NOT READ IS USAGE, BEFORE ANYTHING IS WRITTEN. Each file
/// here breaks one rule of `assets/question.schema.json`, and each is exit 2
/// naming the schema with nothing staged, no hold raised, nothing on the
/// stream and the item byte for byte where it stood.
///
/// RED-PROOF, the duplicate: the question grammar this replaces read its
/// options with `options_in`, which keeps both of two options under one
/// letter, so a question naming A twice was a tree committed and a hold raised.
#[test]
fn a_question_that_does_not_read_is_usage_naming_the_schema_and_nothing_is_written() {
    let scratch = &store();
    let seat = "g-unread";
    let item = an_ordered_item(&scratch.store, "an item asked about badly", seat);
    let before = scratch.json(&item);
    let wrote = scratch.store.wrote();

    for (label, body, why) in [
        (
            "no-options",
            r#"{"question": "Which one?", "options": []}"#,
            "names no option",
        ),
        (
            "lower",
            r#"{"question": "Which one?", "options": [{"letter": "a", "text": "one"}]}"#,
            "`options[0].letter` a, which is not one capital letter",
        ),
        (
            "twice",
            r#"{"question": "Which one?", "options": [
                {"letter": "A", "text": "one"}, {"letter": "A", "text": "two"}
            ]}"#,
            "names option A twice",
        ),
        (
            "two-lines",
            r#"{"question": "Which one?\nAnd why?", "options": [{"letter": "A", "text": "one"}]}"#,
            "asks its question over more than one line",
        ),
        (
            "unknown-key",
            r#"{"question": "Which one?", "options": [{"letter": "A", "text": "one"}],
                "urgency": "high"}"#,
            "unknown field `urgency`",
        ),
    ] {
        let question = a_question(scratch, label, body);
        let git = StubGit::holding_work();
        let events = StubEvents::default();

        let stop = hold_with(
            None,
            &question,
            seat,
            &Seams {
                store: &scratch.store,
                git: &git,
                project: &project(scratch),
                events: &events,
            },
        )
        .expect_err("a question that does not read is refused");

        assert_eq!(stop.code, 2, "{label}: {}", stop.message);
        assert!(
            stop.message.contains(QUESTION_SCHEMA),
            "{label}: the refusal names the schema: {}",
            stop.message
        );
        assert!(
            stop.message.contains(why),
            "{label}: the refusal says `{why}`: {}",
            stop.message
        );
        assert!(
            !git.calls().iter().any(|call| call == "add_all"),
            "{label}: nothing was staged: {:?}",
            git.calls()
        );
        assert!(
            scratch.store.raised().is_empty(),
            "{label}: no hold was raised: {:?}",
            scratch.store.raised()
        );
        assert_eq!(events.count(), 0, "{label}: nothing reached the stream");
    }
    assert_eq!(
        scratch.store.wrote(),
        wrote,
        "the store was written nothing"
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

#[test]
fn a_read_back_that_disagrees_is_could_not_tell() {
    let scratch = &store();
    let seat = "g-readback";
    let item = an_ordered_item(&scratch.store, "an item whose entry does not land", seat);
    let question = a_question(scratch, "readback", QUESTION);
    let before = scratch.json(&item);
    let events = StubEvents::default();

    let stop = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &Swallowing {
                inner: &scratch.store,
            },
            git: &StubGit::holding_work(),
            project: &project(scratch),
            events: &events,
        },
    )
    .expect_err("a held entry nobody can read back is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "{item}'s timeline does not hold the held entry c-nothing the store answered for \
             it\n  the commit {SHA} STANDS on the work branch and the hold fx-nothing STANDS on \
             {item}"
        ),
        "the message names the entry it did not read and what stands"
    );
    assert_eq!(events.count(), 0, "and nothing reached the stream");
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

/// A HELD ENTRY THE STORE REFUSES is could-not-tell, saying the commit and the
/// hold both STAND: here a commit that is not a whole sha, which no held entry
/// is written with.
#[test]
fn a_held_entry_the_store_refuses_is_could_not_tell_and_names_what_stands() {
    let scratch = &store();
    let seat = "g-refused";
    let item = an_ordered_item(&scratch.store, "an item whose entry is refused", seat);
    let question = a_question(scratch, "refused", QUESTION);
    let events = StubEvents::default();
    let git = StubGit {
        made: String::from("abc1234"),
        ..StubGit::holding_work()
    };

    let stop = hold_with(
        None,
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &events,
        },
    )
    .expect_err("an entry the store refuses is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.starts_with(&format!(
            "the held entry did not land: the held entry for {item} does not validate: `commit` \
             is not a full 40-hex sha: abc1234"
        )),
        "{}",
        stop.message
    );
    assert!(
        stop.message.ends_with(&format!(
            "\n  the commit abc1234 STANDS on the work branch and the hold hold-1 STANDS on \
             {item}"
        )),
        "{}",
        stop.message
    );
    assert!(
        timeline_of(&scratch.store, &item).is_empty(),
        "nothing was appended"
    );
    assert_eq!(events.count(), 0, "and nothing reached the stream");
}

/// An epic is refused before any git act: no seat is ever given one, and a
/// park on one is a hold the store cannot tie to it. A seat naming one is
/// refused by the holder check `--item` carries (fleet-pl6 (a)) — an epic is
/// never an item a seat holds — before the type check behind it is reached.
#[test]
fn an_epic_is_refused_before_the_commit_and_nothing_is_written() {
    let scratch = &store();
    let seat = "g-epic";
    let item = scratch
        .store
        .create(
            &NewItem {
                title: String::from("an epic somebody asked about"),
                description: String::from("an epic"),
                item_type: String::from("epic"),
                labels: Vec::new(),
                priority: None,
            },
            &fleet_core::test_support::the_test(),
        )
        .expect("the epic is filed")
        .to_string();
    scratch.hand_to(&item, &full(seat));
    let question = a_question(scratch, "epic", QUESTION);
    let before = scratch.json(&item);
    let wrote = scratch.store.wrote();
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let stop = hold_with(
        Some(&item),
        &question,
        seat,
        &Seams {
            store: &scratch.store,
            git: &git,
            project: &project(scratch),
            events: &events,
        },
    )
    .expect_err("an epic is refused");

    assert_eq!(stop.code, 1, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "{item} is held by {} and not by {} — --item names an item the acting seat holds",
            full(seat),
            seat_actor(seat)
        ),
        "the refusal names the epic and why"
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
    assert!(scratch.store.raised().is_empty(), "no hold was raised");
    assert_eq!(events.count(), 0, "and nothing reached the stream");
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

/// The row the holding fake answers for the item a seat holds.
fn a_held_row(item: &str, seat: &str) -> String {
    let seat = full(seat);
    format!(
        r#"{{"id":"{item}","title":"an item whose hold fails","status":"open","issue_type":"task","assignee":"{seat}","metadata":{{"fleet.orders":{{"v":1,"by":"run:a-flight","kind":"dispatch","seat":"{seat}","at":"{AT}"}}}}}}"#
    )
}

/// The calls the holding fake was handed, with the `-C <root>` every call opens
/// on dropped. A line that does not open on it is the rest of a reason the
/// question's own newlines split, and not a call.
fn holding_calls(log: &std::path::Path) -> Vec<String> {
    common::capped::calls(log)
        .into_iter()
        .filter(|call| call.first().map(String::as_str) == Some("-C"))
        .map(|call| call[2..].join(" "))
        .collect()
}

/// A `gate create` that files its gate and then exits 1 — bd 1.2.2 on an epic,
/// which 1.3.0 no longer does — leaves an open hold blocking nothing. The park reads the open list before
/// the create and again after it, clears what is new, and names it; a hold
/// that was open before the park is somebody else's and is left alone.
#[test]
fn a_hold_left_behind_by_a_failed_create_is_cleared_and_named() {
    let scratch = &store();
    let seat = "g-left";
    let item = "fx-left";
    let dir = common::Fixture::new("hold-left-behind");
    let log = dir.path("argv");
    let bin = holding_bd(&dir, &a_held_row(item, seat), &["g-before"], true, &log);
    let bd = Bd::at_bin(scratch.root(), &bin);
    let question = a_question(scratch, "left", QUESTION);
    let events = StubEvents::default();

    let stop = hold_with(
        Some(item),
        &question,
        seat,
        &Seams {
            store: &bd,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            events: &events,
        },
    )
    .expect_err("a hold that was not raised is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(&format!(
            "the store raised the hold {LEFT_BEHIND} all the same, and it is withdrawn"
        )),
        "the refusal names the hold it cleared: {}",
        stop.message
    );
    assert!(
        stop.message.contains(SHA) && stop.message.contains(&format!("{item} carries no park")),
        "and still says what stands: {}",
        stop.message
    );
    assert!(
        !stop.message.contains("g-before"),
        "a hold open before the park is not this park's: {}",
        stop.message
    );
    assert_eq!(
        standing(&dir),
        vec![String::from("g-before")],
        "the hold left behind is cleared and the one before it stands"
    );

    let calls = holding_calls(&log);
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

/// The same failure where the clear fails too: the hold stands, and the
/// refusal names it as standing on the store with no park naming it — and no
/// store's command, which is the adapter's to know and not fleet's to print.
#[test]
fn a_hold_left_behind_that_cannot_be_cleared_is_named_as_standing() {
    let scratch = &store();
    let seat = "g-left-stands";
    let item = "fx-left-stands";
    let dir = common::Fixture::new("hold-left-stands");
    let log = dir.path("argv");
    let bin = holding_bd(&dir, &a_held_row(item, seat), &[], false, &log);
    let bd = Bd::at_bin(scratch.root(), &bin);
    let question = a_question(scratch, "left-stands", QUESTION);

    let stop = hold_with(
        Some(item),
        &question,
        seat,
        &Seams {
            store: &bd,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            events: &StubEvents::default(),
        },
    )
    .expect_err("a hold that was not raised is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(&format!(
            "the store raised the hold {LEFT_BEHIND} all the same, and it STANDS with no park \
             naming it"
        )) && stop.message.contains(&format!(
            "; the hold {LEFT_BEHIND} stands on the store with no park naming it"
        )),
        "the refusal names the hold and what stands: {}",
        stop.message
    );
    // FLEET'S OWN WORDS NAME NO bd. The store's answer is quoted as the store
    // gave it, and this store's is a bd's argv, so the line is read with that
    // answer taken out: what is before it and what follows it are fleet's.
    let line = stop
        .message
        .lines()
        .find(|line| line.contains("withdrawing it failed: "))
        .unwrap_or_else(|| panic!("no withdrawal line: {}", stop.message));
    let (before, rest) = line
        .split_once("withdrawing it failed: ")
        .expect("the line names the failed withdrawal");
    let (_, after) = rest
        .rsplit_once("; ")
        .expect("the store's answer is followed by what stands");
    for words in [before, after] {
        assert!(
            !words
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| word == "bd"),
            "and no bd in fleet's words `{words}`: {}",
            stop.message
        );
    }
    assert_eq!(standing(&dir), vec![String::from(LEFT_BEHIND)]);
}

// ---- AC2: the clearance ------------------------------------------------------

/// The seams a clearance acts through: the board's store, and a stream the arm
/// reads.
fn clearing<'a>(
    scratch: &'a Board,
    git: &'a StubGit,
    project: &'a Project,
    events: &'a StubEvents,
) -> Seams<'a> {
    Seams {
        store: &scratch.store,
        git,
        project,
        events,
    }
}

/// `clear A` appends `cleared{hold, answer, A}` by the clearer, then clears the
/// store's hold and announces it — and a second clearance of the same item
/// finds no open hold on its timeline.
#[test]
fn a_clearance_writes_the_cleared_entry_clears_the_hold_and_announces_it() {
    let scratch = &store();
    let seat = "g-answer";
    let (item, hold_id) = a_held_item(scratch, "answer", seat);
    let events = StubEvents::default();

    let mut out: Vec<u8> = Vec::new();
    let cleared = hold::clear(
        &mut out,
        &Clearance {
            item: &item,
            letter: "A",
            text: None,
            by: &seat_actor("a-person"),
        },
        &Wiring {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            events: &events,
        },
    )
    .expect("the answer is written");

    assert_eq!(cleared.hold, hold_id, "the hold the held entry named");
    assert_eq!(cleared.letter, "A");

    // THE CLEARED ENTRY, by the person, after the held one.
    let entry = entry_of(&scratch.store, &item, &cleared.entry);
    assert_eq!(entry.by, seat_actor("a-person"), "by who cleared it");
    assert_eq!(
        entry.body,
        Body::Cleared(entry::Cleared {
            hold: hold_id.clone(),
            how: entry::Clearance::Answer,
            letter: Some(String::from("A")),
            text: None,
        })
    );
    let entries = timeline_of(&scratch.store, &item);
    assert!(
        Timeline(&entries).open_hold().is_none(),
        "the timeline carries no open hold: {entries:?}"
    );

    // The hold is off the open list and the item is ready again.
    assert!(
        !scratch
            .store
            .holds_open()
            .expect("the open list answers")
            .iter()
            .any(|open| *open == *hold_id),
        "the hold is cleared"
    );
    assert!(
        scratch
            .store
            .list(&Filter::Ready)
            .expect("the ready read answers")
            .iter()
            .any(|row| row.id == item),
        "and the item is back in the ready set"
    );
    let wrote = scratch.store.wrote();
    let appended = wrote
        .iter()
        .rposition(|line| line.starts_with(&format!("append {item} cleared ")));
    let resolved = wrote
        .iter()
        .position(|line| line.starts_with(&format!("hold_clear {hold_id} ")));
    assert!(
        appended.is_some() && appended < resolved,
        "the entry is appended before the store's hold is cleared: {wrote:?}"
    );

    // The cleared entry's signal, by the clearer. The hold and the letter are
    // the entry's.
    assert_eq!(
        events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            seat_actor("a-person").to_string(),
            signal(&item, &cleared.entry, "cleared"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &events.all()[0].2, &[]);
    assert!(
        String::from_utf8(out)
            .expect("stdout is utf-8")
            .contains(&hold_id),
        "and the line says which hold was cleared"
    );

    // A SECOND CLEARANCE finds nothing open on the timeline, and writes nothing.
    let before = scratch.json(&item);
    let wrote = scratch.store.wrote();
    let git = StubGit::holding_work();
    let project = project(scratch);
    let events = StubEvents::default();
    let stop = clear_with(
        &item,
        "B",
        None,
        "a-person",
        &clearing(scratch, &git, &project, &events),
    )
    .expect_err("a second clearance is refused");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "{item} carries no open hold — a clearance settles a question somebody asked, and \
             this item has none"
        )
    );
    assert_eq!(scratch.store.wrote(), wrote, "nothing was written");
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
    assert_eq!(events.count(), 0, "and nothing reached the stream");
}

#[test]
fn an_unnamed_letter_is_accepted_with_text_and_refused_without_it() {
    let scratch = &store();
    let git = StubGit::holding_work();
    let project = project(scratch);
    let events = StubEvents::default();

    // With --text: the letter the options do not name is what a person saw
    // that the seat did not, and the text is on the entry.
    let seat = "g-text";
    let (item, hold_id) = a_held_item(scratch, "text", seat);
    let said = "neither — the spec is wrong and the bead is going back";
    let cleared = clear_with(
        &item,
        "C",
        Some(said),
        "a-person",
        &clearing(scratch, &git, &project, &events),
    )
    .expect("an unnamed letter with text is an answer");
    assert_eq!(cleared.hold, hold_id);
    assert_eq!(
        entry_of(&scratch.store, &item, &cleared.entry).body,
        Body::Cleared(entry::Cleared {
            hold: hold_id,
            how: entry::Clearance::Answer,
            letter: Some(String::from("C")),
            text: Some(said.to_string()),
        })
    );

    // Without it: usage, and the item untouched.
    let seat = "g-notext";
    let (item, _) = a_held_item(scratch, "notext", seat);
    let before = scratch.json(&item);
    let stop = clear_with(
        &item,
        "C",
        None,
        "a-person",
        &clearing(scratch, &git, &project, &events),
    )
    .expect_err("an unnamed letter with nothing said is refused");
    assert_eq!(stop.code, 2, "{}", stop.message);
    assert!(
        stop.message.contains("--text") && stop.message.contains("A, B"),
        "the refusal names the options and the way past them: {}",
        stop.message
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
    assert_eq!(
        timeline_of(&scratch.store, &item).len(),
        1,
        "the held entry alone"
    );
}

/// The two refusals before anything is written: an item whose timeline
/// carries no open hold — one nobody held, and one whose hold only a person's
/// comment names — and an open held entry whose hold the store no longer lists
/// open, cleared by hand.
#[test]
fn no_open_hold_and_a_hold_cleared_by_hand_are_both_refused() {
    let scratch = &store();
    let git = StubGit::holding_work();
    let project = project(scratch);
    let events = StubEvents::default();

    // An item nobody held, and one carrying only a park in prose, as a
    // person's comment — no entry, and no question it can clear.
    let unheld = scratch.item("an item nobody held");
    let noted = scratch.item("an item prose alone parks");
    let hold = scratch
        .store
        .hold_raise(
            &fleet_core::store::ItemId::from(noted.as_str()),
            "a hold only prose names",
            &fleet_core::test_support::the_test(),
        )
        .expect("the hold is raised");
    scratch.store.comment(
        &noted,
        "a-flight",
        &format!("PARKED {noted} — ask\nbranch:  b\ncommit:  {SHA}\nhold:    {hold}"),
    );
    for item in [&unheld, &noted] {
        let before = scratch.json(item);
        let stop = clear_with(
            item,
            "A",
            None,
            "a-person",
            &clearing(scratch, &git, &project, &events),
        )
        .expect_err("an item with no open hold is refused");
        assert_eq!(stop.code, 1, "{}", stop.message);
        assert_eq!(
            stop.message,
            format!(
                "{item} carries no open hold — a clearance settles a question somebody asked, \
                 and this item has none"
            )
        );
        assert_eq!(before, scratch.json(item), "the item is byte-identical");
    }

    // A held entry whose hold was cleared by hand, and so is not open.
    let seat = "g-by-hand";
    let (item, hold_id) = a_held_item(scratch, "by-hand", seat);
    scratch
        .store
        .hold_clear(
            &fleet_core::store::HoldId::from(hold_id.as_str()),
            &fleet_core::test_support::the_test(),
        )
        .expect("the hold is cleared by hand");
    let before = scratch.json(&item);
    let stop = clear_with(
        &item,
        "A",
        None,
        "a-person",
        &clearing(scratch, &git, &project, &events),
    )
    .expect_err("a hold already cleared is refused");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "{item}'s hold {hold_id} is not one the store lists open — it has been cleared \
             already, or by hand"
        )
    );
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
    assert_eq!(
        timeline_of(&scratch.store, &item).len(),
        1,
        "no cleared entry was written"
    );
    assert_eq!(events.count(), 0, "and nothing reached the stream");
}

/// A cleared entry that does not read back is could-not-tell, and the store's
/// hold is left standing: the clearance is written before the hold is cleared.
#[test]
fn a_cleared_entry_that_does_not_read_back_leaves_the_hold_standing() {
    let scratch = &store();
    let seat = "g-clear-deaf";
    let (item, hold_id) = a_held_item(scratch, "clear-deaf", seat);
    let git = StubGit::holding_work();
    let project = project(scratch);
    let events = StubEvents::default();

    let stop = clear_with(
        &item,
        "A",
        None,
        "a-person",
        &Seams {
            store: &Swallowing {
                inner: &scratch.store,
            },
            git: &git,
            project: &project,
            events: &events,
        },
    )
    .expect_err("an entry nobody can read back is could-not-tell");
    assert_eq!(stop.code, 3, "{}", stop.message);
    assert_eq!(
        stop.message,
        format!(
            "{item}'s timeline does not hold the cleared entry c-nothing the store answered for \
             it\n  {hold_id} is not cleared and {item} is still blocked"
        )
    );
    assert!(
        scratch
            .store
            .holds_open()
            .expect("the open list answers")
            .iter()
            .any(|open| *open == *hold_id),
        "the hold stands"
    );
    assert_eq!(events.count(), 0, "and nothing reached the stream");
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
        &format!(r#"{{"fleet.run": {{"v": 1, "hash": "{RUN_HASH}", "workflow": "takeoff", "pack": "ts", "entry": "takeoff.ts", "started_at": "{AT}"}}}}"#),
    );
    run
}

/// A run held at `[core.run] max_crashes` is CLEARABLE: the park is exactly
/// one `held` entry on the record — reason `max_crashes`, standing on the hash
/// its open pinned, under the hold the store raised, with a question and its
/// lettered options — so the clearance clears the hold the cap raised and the
/// record is no longer blocked by it.
///
/// ONE ENTRY WHERE THERE WERE TWO DISAGREEING RECORDS: the park note said
/// `max_crashes` where the controller's line said the pass's reason, and the
/// entry is now the record of the park. The park answers the entry's id beside
/// the hold's, which the controller's signal names.
#[test]
fn a_run_held_at_the_crash_cap_is_cleared_like_any_other_park() {
    let scratch = &store();
    let run = a_runs_record(scratch, "a run nothing could classify");
    let directory = scratch.root.join("runs").join(&run);

    let controller = Actor {
        kind: ActorKind::Controller,
        id: full("this-machine"),
    };
    let capped = hold::Capped {
        run: &run,
        reason: CAPPED_REASON,
        directory: &directory,
        by: &controller,
    };
    let (hold_id, entry_id) =
        hold::park_at_the_cap(&capped, &scratch.store).expect("the park is made");

    // (a) THE ONE HELD ENTRY, by the controller, under the id the park
    // answered.
    let cap = hold::cap_question(&capped);
    let entries = timeline_of(&scratch.store, &run);
    assert_eq!(entries.len(), 1, "exactly one entry: {entries:?}");
    assert_eq!(entries[0].id, entry_id, "the entry the park answered");
    assert_eq!(entries[0].by, controller);
    assert_eq!(
        entries[0].body,
        Body::Held(entry::Held {
            hold: hold_id.clone(),
            reason: HoldReason::MaxCrashes,
            question: cap.question.clone(),
            context: cap.context.clone(),
            options: cap.options.clone(),
            branch: None,
            commit: None,
            run_hash: Some(RUN_HASH.to_string()),
            about: None,
        })
    );

    // (b) THE ANSWER, which is what a person meets first.
    let events = StubEvents::default();
    let cleared = clear_with(
        &run,
        "B",
        None,
        "a-person",
        &Seams {
            store: &scratch.store,
            git: &StubGit::holding_work(),
            project: &project(scratch),
            events: &events,
        },
    )
    .unwrap_or_else(|stop| panic!("the crash cap's park is answerable: {}", stop.message));
    assert_eq!(
        cleared.hold, hold_id,
        "the clearance clears the hold the cap raised"
    );
    assert!(
        !scratch
            .store
            .holds_open()
            .expect("the open list answers")
            .iter()
            .any(|open| *open == *hold_id),
        "and the store lists it open no longer"
    );
    let (_, payload) = events.one(ITEM_ENTRY);
    assert_eq!(payload, signal(&run, &cleared.entry, "cleared"));

    // (c) THE QUESTION IS A QUESTION INPUT, read by the rules a seat's is held
    // to, and the hold's reason is its whole text, options and all.
    cap.check()
        .unwrap_or_else(|why| panic!("the cap's question reads as a seat's would: {why}"));
    assert_eq!(
        cap.question,
        format!("{CAPPED_REASON} — nothing executes it again.")
    );
    assert_eq!(
        cap.context.as_deref(),
        Some(
            format!(
                "Its stdout.log and stderr.log are in {}.",
                directory.display()
            )
            .as_str()
        )
    );
    assert!(
        cap.options[0]
            .text
            .starts_with(&format!("cancel it: fleet cancel {run} closes its record")),
        "and names the verb that ends the run: {:?}",
        cap.options
    );
    assert_eq!(cap.about, None);
    let raised = scratch.store.raised();
    assert_eq!(raised.len(), 1, "one hold: {raised:?}");
    assert_eq!(
        raised[0].1,
        hold::question_text(&cap),
        "the hold's reason is the whole question, options and all, as a seat's is"
    );
}

/// A HELD ENTRY THE STORE REFUSES WITHDRAWS THE HOLD, so the next poll parks
/// the run again rather than leaving a hold no entry names: here a reason
/// over two lines, which no held entry's question is.
#[test]
fn a_crash_cap_park_whose_entry_is_refused_withdraws_its_hold() {
    let scratch = &store();
    let run = a_runs_record(scratch, "a run whose park is refused");
    let directory = scratch.root.join("runs").join(&run);
    let controller = Actor {
        kind: ActorKind::Controller,
        id: full("this-machine"),
    };

    let stop = hold::park_at_the_cap(
        &hold::Capped {
            run: &run,
            reason: "one line\nand another",
            directory: &directory,
            by: &controller,
        },
        &scratch.store,
    )
    .expect_err("a refused entry is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.starts_with(&format!(
            "the held entry did not land: the held entry for {run} does not validate: \
             `question` runs over more than one line"
        )),
        "{}",
        stop.message
    );
    assert!(
        stop.message
            .ends_with("\n  the hold hold-1 is withdrawn and the next poll parks it again"),
        "{}",
        stop.message
    );
    assert_eq!(
        scratch.store.raised().len(),
        1,
        "the one hold the park raised"
    );
    assert!(
        !scratch
            .store
            .holds_open()
            .expect("the open list answers")
            .iter()
            .any(|open| *open == *String::from("hold-1")),
        "is withdrawn"
    );
    assert!(
        timeline_of(&scratch.store, &run).is_empty(),
        "and nothing is on the record"
    );
}

// ---- AC5: the defaults -------------------------------------------------------

/// The park and answer grammars are gone from the defaults and the registry,
/// as the question's is: a park and a clearance are entries. The question's
/// schema is what the registry names.
#[test]
fn the_defaults_carry_no_park_or_answer_template_and_the_question_schema() {
    let scratch = Board::new("hold-pack");
    let installed = scratch.defaults_dir.clone();
    let registry = std::fs::read_to_string(installed.join("assets/shadow-registry.toml"))
        .expect("the registry is readable");
    for gone in [
        "assets/park-note.md",
        "assets/answer-note.md",
        "assets/question-note.md",
    ] {
        assert!(
            !installed.join(gone).exists(),
            "`{gone}` is gone: a park and its clearance are entries"
        );
        assert!(
            !registry.contains(gone),
            "and the registry names no `{gone}`:\n{registry}"
        );
    }
    assert!(
        registry.contains(&format!("path = \"{QUESTION_SCHEMA}\"")),
        "the registry names `{QUESTION_SCHEMA}`:\n{registry}"
    );
}
