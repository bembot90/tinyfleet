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
use common::{keys_agree, shared_store, Rooted, Scratch, StubEvents};
use fleet_core::item::brief::Packs;
use fleet_core::item::hold::{self, Clearance, Question, Wiring};
use fleet_core::item::run;
use fleet_core::item::{
    last_answer, last_park, Change, Git, Project, Stop, ANSWER_MARKERS, HOLD_CLEARED, ITEM_HELD,
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
/// A hold that answers an id nothing raised and a note that lands nowhere are
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

    fn hold(&self, _item: &str, _reason: &str, _by: &str) -> Result<String, StoreError> {
        Ok(String::from("fx-nothing"))
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

    fn export(&self, into: &std::path::Path) -> Result<(), StoreError> {
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
                r#"{{"fleet.orders": {{"v": 1, "by": "a-flight", "kind": "dispatch", "seat": "{seat}", "at": "{AT}"}}}}"#
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

fn hold_with(
    item: Option<&str>,
    note: &PathBuf,
    by: &str,
    seams: &Seams,
) -> Result<hold::Held, Stop> {
    hold::hold(
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

/// The park an arm needs before it can clear one: a real hold, made the way the
/// arm above it measures.
fn a_held_item(scratch: &Board, label: &str, seat: &str) -> (String, String) {
    let item = an_ordered_item(&scratch.store, &format!("an item held for {label}"), seat);
    let note = a_note(scratch, label, QUESTION);
    let held = hold_with(
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
    let note = a_note(scratch, "clean", QUESTION);
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let mut out: Vec<u8> = Vec::new();
    let held = hold::hold(
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

    // (b) THE HOLD, carrying the note's whole text, and the item off the ready
    // set behind it.
    // `-n 0` for the reason the store's own read carries it: this board is the
    // run's, every ring arm on it contributes holds, and a capped listing drops
    // rows without saying so.
    let holds = String::from_utf8_lossy(&scratch.bd(&["gate", "list", "--json", "-n", "0"]).stdout)
        .to_string();
    assert!(
        holds.contains(&held.hold),
        "the store lists the hold open: {holds}"
    );
    assert!(
        holds.contains("the table the spec names is not there"),
        "and carries the question as its reason: {holds}"
    );
    let ready = bd.ready().expect("the ready read answers");
    assert!(
        !ready.contains(&item),
        "the hold takes the item off the ready set"
    );

    // (c) THE PARK REGION, with the four values and the question beneath them.
    let park = last_park(&notes_of(bd, &item)).expect("the item carries a park");
    assert_eq!(
        park, held.note,
        "the note the store holds is the one written"
    );
    assert!(
        park.starts_with(&format!("{} {item} — ask", PARK_MARKERS[0])),
        "{park}"
    );
    for line in [
        format!("branch:  {BRANCH}"),
        format!("commit:  {SHA}"),
        format!("hold:    {}", held.hold),
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
    let (actor, payload) = events.one(ITEM_HELD);
    assert_eq!(actor, seat, "the actor is the seat that asked");
    keys_agree(ITEM_HELD, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["reason"], serde_json::json!("ask"));
    assert_eq!(payload["branch"], serde_json::json!(BRANCH));
    assert_eq!(payload["commit"], serde_json::json!(SHA));
    assert_eq!(payload["hold"], serde_json::json!(held.hold));
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
    let note = a_note(scratch, "suffix", QUESTION);
    let git = StubGit::holding_work();
    let events = StubEvents::default();

    let held = hold_with(
        Some(suffix),
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
    .expect("the question is asked");

    assert_eq!(held.item, item);
    let wrote = scratch.store.wrote();
    for verb in ["hold", "note"] {
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
    let park = last_park(&notes_of(&scratch.store, &item)).expect("the item carries a park");
    assert!(
        park.starts_with(&format!("{} {item} — ask", PARK_MARKERS[0])),
        "{park}"
    );
    let (_, payload) = events.one(ITEM_HELD);
    assert_eq!(payload["item"], serde_json::json!(item));
}

#[test]
fn a_tree_with_nothing_to_commit_parks_on_head() {
    let scratch = &store();
    let seat = "g-empty";
    let item = an_ordered_item(&scratch.store, "an item asked about before any work", seat);
    let note = a_note(scratch, "empty", QUESTION);
    let git = StubGit::clean();

    let held = hold_with(
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

    assert_eq!(held.commit, HEAD, "the park records HEAD");
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

    let stop = hold_with(
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
/// difference between them, so a `hold` that does not read it answers both the
/// same and one of the two arms reds.
#[test]
fn a_runs_record_parks_off_the_trunk_and_performs_no_git_act() {
    let scratch = &store();
    let seat = "g-run";
    let item = an_item(&scratch.store, "a run being asked about");
    scratch.label(&item, run::LABEL);
    scratch.set_metadata(
        &item,
        &format!(r#"{{"fleet.run": {{"v": 1, "hash": "{RUN_HASH}", "workflow": "takeoff"}}}}"#),
    );
    let note = a_note(scratch, "run", QUESTION);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };
    let events = StubEvents::default();

    let held = hold_with(
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

    // (b) THE PARK, carrying the two values and the hold the store raised.
    let park = last_park(&notes_of(&scratch.store, &item)).expect("the item carries a park");
    for line in [
        format!("branch:  {}", hold::RUN_BRANCH),
        format!("commit:  {RUN_HASH}"),
        format!("hold:    {}", held.hold),
    ] {
        assert!(park.contains(&line), "the park carries `{line}`:\n{park}");
    }

    // (c) THE EVENT the SDK's hold step reads, reaching the stream exactly as a
    // seat's does.
    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(ITEM_HELD);
    assert_eq!(actor, seat);
    keys_agree(ITEM_HELD, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["reason"], serde_json::json!("ask"));
    assert_eq!(payload["branch"], serde_json::json!(hold::RUN_BRANCH));
    assert_eq!(payload["commit"], serde_json::json!(RUN_HASH));
    assert_eq!(payload["hold"], serde_json::json!(held.hold));
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
        &format!(r#"{{"fleet.run": {{"v": 1, "hash": "{RUN_HASH}", "workflow": "takeoff"}}}}"#),
    );
    let note = a_note(scratch, "not-a-run", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit {
        branch: "main".to_string(),
        ..StubGit::holding_work()
    };

    let stop = hold_with(
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
    let note = a_note(scratch, "bare-run", QUESTION);
    let git = StubGit::holding_work();

    let held = hold_with(
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
    scratch.assign(&item, seat);
    let note = a_note(scratch, "unordered", QUESTION);
    let before = scratch.json(&item);
    let git = StubGit::holding_work();

    let stop = hold_with(
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

    let stop = hold_with(
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
        stop.message.contains(hold::QUESTION_NOTE),
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
    let stop = hold_with(
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
        stop.message.contains(hold::QUESTION_MARKERS[0]),
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

    let stop = hold_with(
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
/// one, and a park on one is a hold the store cannot tie to it.
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

    let stop = hold_with(
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
    assert!(scratch.store.raised().is_empty(), "no hold was raised");
    assert_eq!(events.count(), 0, "and nothing reached the stream");
    assert_eq!(before, scratch.json(&item), "the item is byte-identical");
}

/// The row the holding fake answers for the item a seat holds.
fn a_held_row(item: &str, seat: &str) -> String {
    format!(
        r#"{{"id":"{item}","title":"an item whose hold fails","status":"open","issue_type":"task","assignee":"{seat}","metadata":{{"fleet.orders":{{"v":1,"by":"a-flight","kind":"dispatch","seat":"{seat}","at":"{AT}"}}}}}}"#
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
    let note = a_note(scratch, "left", QUESTION);
    let events = StubEvents::default();

    let stop = hold_with(
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
/// refusal names it with the command that clears it.
#[test]
fn a_hold_left_behind_that_cannot_be_cleared_is_named_with_its_command() {
    let scratch = &store();
    let seat = "g-left-stands";
    let item = "fx-left-stands";
    let dir = common::Fixture::new("hold-left-stands");
    let log = dir.path("argv");
    let bin = holding_bd(&dir, &a_held_row(item, seat), &[], false, &log);
    let bd = Bd::at_bin(scratch.root(), &bin);
    let note = a_note(scratch, "left-stands", QUESTION);

    let stop = hold_with(
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
    .expect_err("a hold that was not raised is could-not-tell");

    assert_eq!(stop.code, 3, "{}", stop.message);
    assert!(
        stop.message.contains(&format!(
            "the store raised the hold {LEFT_BEHIND} all the same, and it STANDS with no park \
             naming it"
        )) && stop
            .message
            .contains(&format!("`bd gate resolve {LEFT_BEHIND}` clears it")),
        "the refusal names the hold and the command: {}",
        stop.message
    );
    assert_eq!(standing(&dir), vec![String::from(LEFT_BEHIND)]);
}

// ---- AC2: the clearance ------------------------------------------------------

#[test]
fn a_clearance_writes_the_answer_clears_the_hold_and_announces_it() {
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

    assert_eq!(cleared.hold, hold_id, "the hold the park named");
    assert_eq!(cleared.letter, "A");

    let answered =
        last_answer(&notes_of(&scratch.store, &item)).expect("the item carries an answer");
    assert_eq!(answered, cleared.note);
    assert!(
        answered.starts_with(&format!("{} {hold_id} — a-person", ANSWER_MARKERS[0])),
        "{answered}"
    );
    assert!(answered.contains("letter:  A"), "{answered}");
    assert!(
        answered.contains("text:    (none)"),
        "an unsaid text is `(none)` and not a blank line:\n{answered}"
    );

    // The hold is off the open list and the item is ready again.
    assert!(
        !scratch
            .store
            .open_holds()
            .expect("the open list answers")
            .contains(&hold_id),
        "the hold is cleared"
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
    let (actor, payload) = events.one(HOLD_CLEARED);
    assert_eq!(actor, "a-person");
    keys_agree(HOLD_CLEARED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["hold"], serde_json::json!(hold_id));
    assert_eq!(payload["letter"], serde_json::json!("A"));
    assert!(
        String::from_utf8(out)
            .expect("stdout is utf-8")
            .contains(&hold_id),
        "and the line says which hold was cleared"
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
    let (item, hold_id) = a_held_item(scratch, "text", seat);
    let cleared = clear_with(
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
    assert_eq!(cleared.hold, hold_id);
    let answered =
        last_answer(&notes_of(&scratch.store, &item)).expect("the item carries an answer");
    assert!(answered.contains("letter:  C"), "{answered}");
    assert!(
        answered.contains("the spec is wrong and the bead is going back"),
        "{answered}"
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
fn no_park_and_a_hold_already_cleared_are_both_refused() {
    let scratch = &store();

    // An item nobody parked.
    let unparked = scratch.item("an item nobody parked");
    let before = scratch.json(&unparked);
    let stop = clear_with(
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

    // And a second clearance on one item, whose hold the store does not list open.
    let seat = "g-twice";
    let (item, hold_id) = a_held_item(scratch, "twice", seat);
    clear_with(
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
    let stop = clear_with(
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
    .expect_err("a hold already cleared is refused");
    assert_eq!(stop.code, 1, "{}", stop.message);
    assert!(
        stop.message.contains(&hold_id),
        "the refusal names the hold: {}",
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
        &format!(r#"{{"fleet.run": {{"v": 1, "hash": "{RUN_HASH}", "workflow": "takeoff"}}}}"#),
    );
    run
}

/// A run held at `[core.run] max_crashes` is CLEARABLE: the park carries the
/// PARKED note `fleet clear` reads — the run's branch, the hash its open
/// pinned, the hold the store raised, and a question with lettered options —
/// so the clearance clears the hold the cap raised and the record is no longer
/// blocked by it.
///
/// THE ONE PARK THAT HAD NO NOTE. A hold raised with nothing on the record
/// naming it is a question `fleet clear` refuses as "carries no park", and the
/// open hold blocks the record's close as well, so the run held its
/// `[core.run] max_open` slot for good.
#[test]
fn a_run_held_at_the_crash_cap_is_cleared_like_any_other_park() {
    let scratch = &store();
    let run = a_runs_record(scratch, "a run nothing could classify");
    let directory = scratch.root.join("runs").join(&run);

    let hold_id = hold::park_at_the_cap(
        &hold::Capped {
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
    let cleared = clear_with(
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
        cleared.hold, hold_id,
        "the clearance clears the hold the cap raised"
    );
    assert!(
        !scratch
            .store
            .open_holds()
            .expect("the open list answers")
            .contains(&hold_id),
        "and the store lists it open no longer"
    );
    let (_, payload) = events.one(HOLD_CLEARED);
    assert_eq!(payload["item"], serde_json::json!(run));

    // (b) THE PARK IT ANSWERED: a run's, standing on the hash its open pinned.
    let park = last_park(&notes_of(&scratch.store, &run)).expect("the record carries a park");
    for line in [
        format!("{} {run} — {}", PARK_MARKERS[0], hold::CAPPED),
        format!("branch:  {}", hold::RUN_BRANCH),
        format!("commit:  {RUN_HASH}"),
        format!("hold:    {hold_id}"),
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
    assert_eq!(raised.len(), 1, "one hold: {raised:?}");
    let letters: Vec<char> = hold::options_in(&raised[0].1)
        .into_iter()
        .map(|(letter, _)| letter)
        .collect();
    assert_eq!(
        letters,
        vec!['A', 'B'],
        "the hold's reason is the whole question, options and all, as a seat's is: {}",
        raised[0].1
    );
}

// ---- AC5: the defaults -------------------------------------------------------

#[test]
fn the_defaults_carry_both_templates_and_the_registry_names_them() {
    let scratch = Board::new("hold-pack");
    let installed = scratch.defaults_dir.clone();
    for (slot, marker) in [
        (hold::QUESTION_NOTE, hold::QUESTION_MARKERS[0]),
        (hold::ANSWER_NOTE, ANSWER_MARKERS[0]),
    ] {
        let body = std::fs::read_to_string(installed.join(slot)).expect("the template is readable");
        assert!(
            body.starts_with(marker),
            "`{slot}` opens on its own marker:\n{body}"
        );
    }

    let registry = std::fs::read_to_string(installed.join("assets/shadow-registry.toml"))
        .expect("the registry is readable");
    for slot in [hold::QUESTION_NOTE, hold::ANSWER_NOTE] {
        assert!(
            registry.contains(slot),
            "and the registry names `{slot}`:\n{registry}"
        );
    }
}
