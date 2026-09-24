//! `fleet review` against a real work graph.
//!
//! The delivery each arm reads is planted as a note, because what review reads
//! is the record and not the verb that wrote it: a review of a hand-written
//! delivery note is the same read as a review of one `fleet deliver` rendered.
//!
//! The size line is asserted against a numstat of known shape, so the counts
//! are read from the arm's own fixture rather than from whatever the tree
//! happens to hold.

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::{keys_agree, shared_store, Rooted, Scratch, StubEvents};
use fleet_core::item::brief::Packs;
use fleet_core::item::deliver::BASE;
use fleet_core::item::review::{self, Mode, Verdict, Wiring};
use fleet_core::item::{
    Change, Git, Project, Ring, RingOutcome, ITEM_RETURNED, ITEM_REVIEWED, VERDICT_ACCEPTED,
};
use fleet_core::store::{AssignedItem, Bd, Item, Store, StoreError};
use fleet_core::test_support::Board;

const AT: &str = "2026-09-09T04:05:06Z";
const SHA: &str = "3333333333333333333333333333333333333333";
const REVIEWER: &str = "a-reviewer";
const POLICY: &str = "[core]\nreviewer = \"a-reviewer\"\n";

/// The delivery every arm reads: two numbered calls, and a commit line the
/// review takes its diff from.
fn a_delivery() -> String {
    format!(
        "\
DELIVERED {SHA} — a-seat
commit:  {SHA}
branch:  a-seat/feat/the-work
base:    origin/main at 4444444444444444444444444444444444444444, read at {AT}
files:   a/file.rs
checks:  AC3 green
suite:   the workspace suite, rc 0
spec corrections: none
not proven: what this arm did not run
decisions: 2
  D1 the first call; not taken: the other one; because a clause
  D2 the second call; not taken: the other one; because a clause
covers: R7"
    )
}

// ---- the seams ---------------------------------------------------------------

/// A numstat of known shape: two text files and one binary, whose dashes are a
/// changed file with no line delta.
fn a_diff() -> Vec<Change> {
    vec![
        Change {
            added: Some(12),
            deleted: Some(4),
            path: "a/file.rs".to_string(),
        },
        Change {
            added: Some(30),
            deleted: Some(0),
            path: "a/tests/arm.rs".to_string(),
        },
        Change {
            added: None,
            deleted: None,
            path: "a/logo.png".to_string(),
        },
    ]
}

struct StubGit {
    diff: Vec<Change>,
    calls: Mutex<Vec<String>>,
}

impl StubGit {
    fn answering(diff: Vec<Change>) -> StubGit {
        StubGit {
            diff,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("not poisoned").clone()
    }
}

impl Git for StubGit {
    fn current_branch(&self) -> Result<String, String> {
        Err(String::from("review reads no branch"))
    }

    fn head(&self) -> Result<String, String> {
        Err(String::from("review reads no head"))
    }

    fn trunk_tip(&self) -> Result<String, String> {
        Err(String::from("review reads no trunk"))
    }

    fn staged(&self) -> Result<Vec<String>, String> {
        Err(String::from("review reads no index"))
    }

    fn status(&self) -> Result<Vec<String>, String> {
        Err(String::from("review reads no status"))
    }

    fn add_all(&self) -> Result<(), String> {
        Err(String::from("review stages nothing"))
    }

    fn commit(&self, _message: &str) -> Result<String, String> {
        Err(String::from("review commits nothing"))
    }

    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        self.calls
            .lock()
            .expect("not poisoned")
            .push(format!("numstat {from} {to}"));
        Ok(self.diff.clone())
    }
}

struct StubRing {
    calls: Mutex<Vec<(String, String)>>,
}

impl StubRing {
    fn new() -> StubRing {
        StubRing {
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
        RingOutcome::Delivered
    }
}

/// The real store with its assignee reading bent, so an arm can force the
/// disagreement the return's read-back exists to catch.
struct Doctored<'a> {
    inner: &'a dyn Store,
    assignee: String,
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
        read.assignee = Some(self.assignee.clone());
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

    fn export(&self, into: &std::path::Path) -> Result<(), StoreError> {
        self.inner.export(into)
    }
}

// ---- the rig -----------------------------------------------------------------

/// One board per arm, held in memory.
fn store() -> Board {
    let board = Board::new("review");
    board.fleet_toml(POLICY);
    board
}

/// The integration ring: the one arm of this suite that reviews through `bd`.
fn ring() -> &'static Scratch {
    let scratch = shared_store("review");
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

/// An item carrying a delivery and the order index that says who built it, in
/// one call apiece.
fn a_delivered_item(store: &dyn Store, title: &str, builder: &str) -> String {
    let item = an_item(store, title);
    store
        .assign(&item, REVIEWER, "an-architect")
        .expect("the reviewer holds it");
    store
        .set_orders(
            &item,
            &format!(
                r#"{{"fleet.orders": {{"v": 1, "by": "an-architect", "kind": "dispatch", "seat": "{builder}", "at": "{AT}"}}}}"#
            ),
            "an-architect",
        )
        .expect("the order index lands");
    store
        .note(&item, &a_delivery(), builder)
        .expect("the delivery lands");
    item
}

/// One item filed through the trait, so the same builder fills either board.
fn an_item(store: &dyn Store, title: &str) -> String {
    store
        .create(
            &fleet_core::store::NewItem {
                title,
                description: "an item to review",
                item_type: "task",
                labels: &[],
            },
            "an-architect",
        )
        .expect("the item is filed")
}

fn file(scratch: &dyn Rooted, label: &str, body: &str) -> PathBuf {
    let path = scratch.root().join(format!("findings-{label}.md"));
    std::fs::write(&path, body).expect("the file is written");
    path
}

/// An item carrying this delivery and nothing else.
fn an_item_delivering(store: &dyn Store, title: &str, delivery: &str) -> String {
    let item = an_item(store, title);
    store
        .note(&item, delivery, "s-header")
        .expect("the delivery lands");
    item
}

struct Said {
    out: String,
    err: String,
    /// The stop's message, where the verb stopped.
    stop: String,
}

fn run(scratch: &Board, item: &str, mode: Mode, git: &StubGit, ring: &StubRing) -> (Said, u8) {
    run_watched(scratch, item, mode, git, ring, &StubEvents::default())
}

/// The same run with the stream in the arm's own hands, for an arm that asserts
/// what reached it — or that nothing did.
fn run_watched(
    scratch: &Board,
    item: &str,
    mode: Mode,
    git: &StubGit,
    ring: &StubRing,
    events: &StubEvents,
) -> (Said, u8) {
    run_through(&scratch.store, scratch, item, mode, git, ring, events)
}

fn run_through(
    store: &dyn Store,
    scratch: &dyn Rooted,
    item: &str,
    mode: Mode,
    git: &StubGit,
    ring: &StubRing,
    events: &StubEvents,
) -> (Said, u8) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let answer = review::review(
        &mut out,
        &mut err,
        &Verdict {
            item,
            by: REVIEWER,
            mode,
        },
        &Wiring {
            store,
            git,
            packs: &Packs::under(scratch.packs_dir(), scratch.defaults_dir())
                .expect("the defaults resolve"),
            project: &project(scratch),
            ring,
            events,
        },
    );
    let (code, stop) = match answer {
        Ok(_) => (0, String::new()),
        Err(stop) => (stop.code, stop.message),
    };
    let said = Said {
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
        stop,
    };
    (said, code)
}

fn notes(store: &dyn Store, item: &str) -> String {
    store
        .show(item)
        .expect("the item reads")
        .notes
        .unwrap_or_default()
}

// ---- the arms ----------------------------------------------------------------

#[test]
fn show_prints_the_delivery_the_size_line_and_the_decisions_block_and_writes_nothing() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item to look at", "s-show");
    let before = scratch.json(&item);
    let git = StubGit::answering(a_diff());

    let events = StubEvents::default();
    let (said, code) = run_watched(scratch, &item, Mode::Show, &git, &StubRing::new(), &events);

    assert_eq!(code, 0, "{}", said.err);
    assert_eq!(events.count(), 0, "--show announces nothing");
    assert!(
        said.out.contains("size: 3 file(s), +42, -4 (1 binary)"),
        "the counts of the fixture diff: {}",
        said.out
    );
    assert!(
        said.out.contains("tests: yes"),
        "a changed path under tests/: {}",
        said.out
    );
    assert!(
        said.out.contains(&format!("commit:  {SHA}")),
        "the delivery note: {}",
        said.out
    );
    assert!(
        said.out.contains("D1 the first call") && said.out.contains("D2 the second call"),
        "the decisions block: {}",
        said.out
    );
    let recorded = fleet_core::item::label_value(&a_delivery(), BASE).expect("a base: line");
    let base = recorded
        .split(" at ")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .expect("a sha after ` at `");
    assert_eq!(
        git.calls(),
        vec![format!("numstat {base} {SHA}")],
        "the base the delivery recorded, against the delivery commit"
    );
    assert_eq!(before, scratch.json(&item), "--show writes nothing");
}

#[test]
fn a_delivery_naming_no_readable_base_is_measured_against_the_commits_parent() {
    let scratch = &store();
    let recorded = a_delivery()
        .lines()
        .find(|line| line.starts_with(&format!("{BASE}:")))
        .expect("the fixture carries a base: line")
        .to_string();
    let delivery = a_delivery().replace(&recorded, "base:    (none)");
    let item = an_item_delivering(
        &scratch.store,
        "an item whose delivery names no base",
        &delivery,
    );
    let git = StubGit::answering(a_diff());

    let (said, code) = run(scratch, &item, Mode::Show, &git, &StubRing::new());

    assert_eq!(code, 0, "{}", said.err);
    assert_eq!(
        git.calls(),
        vec![format!("numstat {SHA}^ {SHA}")],
        "the delivery commit against its parent"
    );
    let size = said
        .out
        .lines()
        .find(|line| line.starts_with("size:"))
        .expect("a size line");
    assert!(
        size.ends_with(&format!(
            " — against {SHA}^: the delivery names no readable base"
        )),
        "the size line says what it measured against: {size}"
    );
}

#[test]
fn land_walks_every_call_the_delivery_numbered_and_writes_the_count() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item to accept", "s-land");
    let git = StubGit::answering(a_diff());

    let events = StubEvents::default();
    let (said, code) = run_watched(scratch, &item, Mode::Land, &git, &StubRing::new(), &events);
    assert_eq!(code, 0, "{}", said.err);

    // The one event, its counts the walk's own.
    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(ITEM_REVIEWED);
    assert_eq!(actor, REVIEWER);
    keys_agree(ITEM_REVIEWED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["commit"], serde_json::json!(SHA));
    assert_eq!(payload["verdict"], serde_json::json!(VERDICT_ACCEPTED));
    assert_eq!(payload["accepted"], serde_json::json!(2));
    assert_eq!(payload["overruled"], serde_json::json!(0));

    let verdict =
        review::last_verdict(&notes(&scratch.store, &item)).expect("a verdict is written");
    assert!(
        verdict.starts_with(&format!("ACCEPTED {SHA} — {REVIEWER}")),
        "{verdict}"
    );
    assert!(verdict.contains("D1 ACCEPT"), "{verdict}");
    assert!(verdict.contains("D2 ACCEPT"), "{verdict}");
    assert!(
        verdict.contains("2 accepted, 0 overruled"),
        "the walk's last line: {verdict}"
    );
    assert!(
        review::last_verdict(&notes(&scratch.store, &item)).is_some()
            && fleet_core::item::last_delivery(&notes(&scratch.store, &item))
                .expect("the delivery is still findable")
                .starts_with("DELIVERED"),
        "a verdict after a delivery is not read as one"
    );
}

/// A findings body that QUOTES a marker at column zero is still written, read
/// back and read out whole.
///
/// A region starts at the last marker of its kind and ends at the next of
/// another, so an un-indented body line opening on a marker would be read as
/// the region's own start or its end, and the read-back inside this verb would
/// then report a verdict it had just written correctly as disagreeing.
#[test]
fn a_findings_body_quoting_a_marker_is_written_and_read_back_whole() {
    let scratch = &store();
    let builder = "s-quoting";
    let item = a_delivered_item(
        &scratch.store,
        "an item returned with a quoted marker",
        builder,
    );
    let findings = file(
        scratch,
        "quoting",
        "F1 the note opens on the wrong word. It reads\nDELIVERED abc — someone\n\
         where it must read RE-DELIVERED, and a reader anchoring on the last\n\
         ACCEPTED abc — someone\nfinds this note instead.\n",
    );
    let git = StubGit::answering(a_diff());
    let ring = StubRing::new();

    // Exit 0 IS the assertion: this verb reads its own write back and answers 3
    // on a disagreement, so a truncated region could not get here.
    let (said, code) = run(scratch, &item, Mode::Return(&findings), &git, &ring);
    assert_eq!(code, 0, "{}", said.err);

    let verdict =
        review::last_verdict(&notes(&scratch.store, &item)).expect("a verdict is written");
    assert!(
        verdict.starts_with(&format!("RETURNED WITH FINDINGS {SHA}")),
        "the region starts at the marker this verb wrote, not at a line of its body: {verdict}"
    );
    assert!(
        verdict.contains("DELIVERED abc — someone") && verdict.contains("finds this note instead."),
        "and it carries the whole body, quoted markers and all: {verdict}"
    );
    assert!(
        verdict
            .lines()
            .skip(1)
            .all(|line| !line.starts_with("DELIVERED")
                && !line.starts_with("ACCEPTED")
                && !line.starts_with("LANDED")),
        "no line of the body opens a marker at column zero: {verdict}"
    );
    // The control: the body reached the note as the reviewer wrote it, moved
    // off column zero and not edited — so what was measured is the indentation
    // and not a verb that dropped the awkward lines.
    assert!(
        verdict.contains("  DELIVERED abc — someone"),
        "the body is indented, not rewritten: {verdict}"
    );
    assert!(
        fleet_core::item::last_delivery(&notes(&scratch.store, &item))
            .expect("the delivery is still findable")
            .starts_with("DELIVERED "),
        "and the delivery region is still the delivery's"
    );
}

#[test]
fn a_return_writes_the_findings_count_first_and_hands_the_item_back() {
    let scratch = ring();
    let bd = &Bd::at(&scratch.root);
    let builder = "s-return";
    let item = a_delivered_item(bd, "an item to return", builder);
    let findings = file(
        scratch,
        "two",
        "F1 the first finding, with what to measure.\nF2 the second one.\n",
    );
    let git = StubGit::answering(a_diff());
    let ring = StubRing::new();

    let events = StubEvents::default();
    let (said, code) = run_through(
        bd,
        scratch,
        &item,
        Mode::Return(&findings),
        &git,
        &ring,
        &events,
    );
    assert_eq!(code, 0, "{}", said.err);

    assert_eq!(events.count(), 1, "exactly one event");
    let (actor, payload) = events.one(ITEM_RETURNED);
    assert_eq!(actor, REVIEWER);
    keys_agree(ITEM_RETURNED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["commit"], serde_json::json!(SHA));
    assert_eq!(
        payload["findings"],
        serde_json::json!(2),
        "the count the verdict's first line carries"
    );

    let verdict = review::last_verdict(&notes(bd, &item)).expect("a verdict is written");
    let mut lines = verdict.lines();
    assert_eq!(
        lines.next(),
        Some(format!("RETURNED WITH FINDINGS {SHA} — {REVIEWER}").as_str())
    );
    assert_eq!(
        lines.next(),
        Some("findings: 2"),
        "the count is the first line after the marker: {verdict}"
    );
    assert!(verdict.contains("F1 the first finding"), "{verdict}");

    assert_eq!(
        bd.show(&item).expect("the item reads").assignee.as_deref(),
        Some(builder),
        "the item goes back to the seat the order named"
    );
    let rung = ring.calls();
    assert_eq!(rung.len(), 1);
    assert_eq!(rung[0].0, builder);
    assert!(rung[0].1.contains(&item), "{:?}", rung);
}

/// Another writer's bare `orders` — whose `seat` names a seat fleet never
/// ordered — and the bare `run` label are not fleet's: a return goes to the
/// seat fleet's own index names, an accept is written as any other, and both
/// ride through byte-identical (fleet-4j6 AC1, the review).
#[test]
fn another_writers_orders_key_and_run_label_ride_through_a_review() {
    let scratch = &store();
    let builder = "s-foreign";
    let returned = a_delivered_item(&scratch.store, "an item to return", builder);
    let accepted = a_delivered_item(&scratch.store, "an item to accept", builder);
    for item in [&returned, &accepted] {
        scratch
            .store
            .set_metadata(item, common::FOREIGN_ORDERS, "another-tool")
            .expect("the other writer's key lands");
        scratch.label(item, common::FOREIGN_LABEL);
    }
    let before = [
        common::foreign_of(&scratch.store, &returned),
        common::foreign_of(&scratch.store, &accepted),
    ];
    let findings = file(scratch, "foreign", "F1 the one finding.\n");
    let ring = StubRing::new();

    let (said, code) = run(
        scratch,
        &returned,
        Mode::Return(&findings),
        &StubGit::answering(a_diff()),
        &ring,
    );
    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    assert_eq!(
        scratch
            .store
            .show(&returned)
            .expect("the item reads")
            .assignee
            .as_deref(),
        Some(builder),
        "the return goes to the seat fleet's index names"
    );
    assert_eq!(ring.calls()[0].0, builder, "and that seat is rung");

    let (said, code) = run(
        scratch,
        &accepted,
        Mode::Land,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );
    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    assert!(
        review::last_verdict(&notes(&scratch.store, &accepted))
            .is_some_and(|verdict| verdict.starts_with("ACCEPTED")),
        "the accept is written"
    );

    assert_eq!(
        [
            common::foreign_of(&scratch.store, &returned),
            common::foreign_of(&scratch.store, &accepted),
        ],
        before,
        "the other writer's key and label are byte-identical"
    );
}

/// The return's second write is read back like its first: an assignee that
/// reads back as anyone but the builder is a could-not-tell, and nobody is rung.
#[test]
fn a_return_whose_assignee_reads_back_as_somebody_else_could_not_tell_and_rings_nobody() {
    let scratch = &store();
    let builder = "s-bent";
    let item = a_delivered_item(&scratch.store, "an item whose return is misread", builder);
    let findings = file(scratch, "bent", "F1 the one finding.\n");
    let bent = Doctored {
        inner: &scratch.store,
        assignee: "somebody-else".to_string(),
    };
    let ring = StubRing::new();
    let events = StubEvents::default();

    let (said, code) = run_through(
        &bent,
        scratch,
        &item,
        Mode::Return(&findings),
        &StubGit::answering(a_diff()),
        &ring,
        &events,
    );

    assert_eq!(code, 3, "{}{}{}", said.out, said.err, said.stop);
    assert_eq!(
        events.count(),
        0,
        "the event follows the read-back, so a disagreement announces nothing"
    );
    assert!(
        said.stop.contains("somebody-else") && said.stop.contains(builder),
        "the value read and the value wanted: {}",
        said.stop
    );
    assert!(
        said.stop
            .contains(&format!("RERUN: bd update {item} --assignee {builder}")),
        "{}",
        said.stop
    );
    assert!(ring.calls().is_empty(), "rung: {:?}", ring.calls());
}

#[test]
fn a_return_that_numbers_nothing_is_a_usage_error_and_writes_nothing() {
    let scratch = &store();
    let item = a_delivered_item(
        &scratch.store,
        "an item returned with no findings",
        "s-none",
    );
    let before = scratch.json(&item);
    let findings = file(
        scratch,
        "none",
        "This reads like a question and numbers nothing.\nIt asks about F-something in prose.\n",
    );

    let (said, code) = run(
        scratch,
        &item,
        Mode::Return(&findings),
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );

    assert_eq!(code, 2, "{}{}", said.out, said.err);
    assert_eq!(before, scratch.json(&item), "nothing was written");

    // The control: the same call with one numbered finding gets past the gate.
    let numbered = file(scratch, "one", "F1 the one finding.\n");
    let (said, code) = run(
        scratch,
        &item,
        Mode::Return(&numbered),
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );
    assert_eq!(code, 0, "{}{}", said.out, said.err);
}

#[test]
fn an_item_with_no_delivery_is_refused() {
    let scratch = &store();
    let item = an_item(&scratch.store, "an item nobody has delivered");

    let (said, code) = run(
        scratch,
        &item,
        Mode::Show,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );
    assert_eq!(code, 1, "{}{}", said.out, said.err);
}

/// The size line is a measurement and names no tier: core is one reviewer's
/// read at every size, and the tiers are the pack's.
#[test]
fn the_size_line_carries_counts_and_no_tier() {
    let line = review::rendered_size(&a_diff(), &PathBuf::from("/nowhere-at-all"));
    assert_eq!(
        line,
        "size: 3 file(s), +42, -4 (1 binary) — tests: yes, executable: no"
    );
    for tier in ["T0", "T1", "T2", "T3", "tier"] {
        assert!(!line.contains(tier), "{line} names {tier}");
    }
    assert_eq!(
        review::rendered_size(&[], &PathBuf::from("/nowhere-at-all")),
        "size: 0 file(s), +0, -0 — tests: no, executable: no",
        "an empty diff reads as empty and not as absent"
    );
}

/// The walk answers per call, so the block the delivery wrote is what it reads.
#[test]
fn the_decisions_read_are_the_ones_the_note_numbered() {
    let delivery = a_delivery();
    assert_eq!(review::decisions(&delivery), vec!["D1", "D2"]);
    assert!(review::decisions_block(&delivery).starts_with("decisions: 2"));

    let none = delivery.replace(
        "decisions: 2\n  D1 the first call; not taken: the other one; because a clause\n  D2 the second call; not taken: the other one; because a clause",
        "decisions: none",
    );
    assert!(
        review::decisions(&none).is_empty(),
        "a measured zero is not a call"
    );
    assert_eq!(review::decisions_block(&none), "decisions: none");
}

#[test]
fn land_refuses_a_header_counting_more_calls_than_the_walk_finds_and_writes_nothing() {
    let scratch = &store();
    let delivery = a_delivery().replace("decisions: 2", "decisions: 3");
    let item = an_item_delivering(&scratch.store, "an item whose header says three", &delivery);

    let (said, code) = run(
        scratch,
        &item,
        Mode::Land,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );

    assert_eq!(code, 1, "{}{}", said.out, said.err);
    assert!(
        said.stop.contains("decisions: 3") && said.stop.contains("found 2"),
        "both numbers: {}",
        said.stop
    );
    assert!(said.stop.contains("indented"), "{}", said.stop);
    assert_eq!(
        review::last_verdict(&notes(&scratch.store, &item)),
        None,
        "no verdict is written"
    );
}

#[test]
fn land_refuses_calls_at_column_zero_that_the_header_counts() {
    let scratch = &store();
    let delivery = a_delivery().replace("\n  D", "\nD");
    assert_eq!(review::decisions(&delivery), Vec::<String>::new());
    let item = an_item_delivering(
        &scratch.store,
        "an item listing its calls unindented",
        &delivery,
    );

    let (said, code) = run(
        scratch,
        &item,
        Mode::Land,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );

    assert_eq!(code, 1, "{}{}", said.out, said.err);
    assert!(
        said.stop.contains("decisions: 2") && said.stop.contains("found 0"),
        "both numbers: {}",
        said.stop
    );
    assert_eq!(review::last_verdict(&notes(&scratch.store, &item)), None);
}

#[test]
fn land_on_a_header_of_none_or_zero_over_no_calls_walks_zero() {
    let scratch = &store();
    let block = "decisions: 2\n  D1 the first call; not taken: the other one; because a clause\n  D2 the second call; not taken: the other one; because a clause";
    for header in ["decisions: none", "decisions: 0"] {
        let delivery = a_delivery().replace(block, header);
        assert!(delivery.contains(&format!("\n{header}\n")), "{delivery}");
        let item = an_item_delivering(
            &scratch.store,
            &format!("an item whose {header}"),
            &delivery,
        );

        let (said, code) = run(
            scratch,
            &item,
            Mode::Land,
            &StubGit::answering(a_diff()),
            &StubRing::new(),
        );

        assert_eq!(code, 0, "{header}: {}{}", said.err, said.stop);
        let verdict =
            review::last_verdict(&notes(&scratch.store, &item)).expect("a verdict is written");
        assert!(
            verdict.contains("0 accepted, 0 overruled"),
            "{header}: {verdict}"
        );
    }
}

/// The tests: answer reads a path's directories and its file's stem, never a
/// word that merely contains the letters.
#[test]
fn the_tests_answer_reads_test_directories_and_test_stems_only() {
    let table = [
        ("a/tests/arm.rs", "yes"),
        ("test/x.py", "yes"),
        ("src/tests.rs", "yes"),
        ("src/foo_test.go", "yes"),
        ("src/foo_tests.rs", "yes"),
        ("web/foo.test.ts", "yes"),
        ("tools/test_helpers.py", "yes"),
        ("brain/attestation.md", "no"),
        ("src/contest.rs", "no"),
        ("src/latest.rs", "no"),
        ("docs/testing-guide.md", "no"),
        ("src/protest/mod.rs", "no"),
        ("a/file.rs", "no"),
    ];
    let wrong: Vec<String> = table
        .iter()
        .filter_map(|(path, want)| {
            let line = review::rendered_size(
                &[Change {
                    added: Some(1),
                    deleted: Some(0),
                    path: path.to_string(),
                }],
                &PathBuf::from("/nowhere-at-all"),
            );
            (!line.contains(&format!("tests: {want},"))).then(|| format!("{path}: {line}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
