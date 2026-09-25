//! `fleet review` against a real work graph.
//!
//! The delivery each arm reads is appended as a delivered entry, because what
//! review reads is the record and not the verb that wrote it: a review of an
//! entry appended here is the same read as a review of one `fleet deliver`
//! wrote.
//!
//! The size line is asserted against a numstat of known shape, so the counts
//! are read from the arm's own fixture rather than from whatever the tree
//! happens to hold.

mod common;

use std::path::PathBuf;
use std::sync::Mutex;

use common::{
    agent, fleet_of, full, keys_agree, seat_actor, shared_store, Rooted, Scratch, StubEvents,
};
use fleet_core::entry::{
    Body, CheckResult, Decision, Delivered, NotProven, Ran, SuiteRun, Timeline,
};
use fleet_core::item::brief::Packs;
use fleet_core::item::review::{self, Mode, Verdict, Wiring};
use fleet_core::item::show::entry_lines;
use fleet_core::item::{
    Change, Git, Project, Ring, RingOutcome, ITEM_RETURNED, ITEM_REVIEWED, VERDICT_ACCEPTED,
};
use fleet_core::store::{AssignedItem, Bd, Item, Store, StoreError};
use fleet_core::test_support::Board;

const AT: &str = "2026-09-09T04:05:06Z";
const SHA: &str = "3333333333333333333333333333333333333333";
/// The base the delivery was cut from, which the size line measures from.
const BASE: &str = "4444444444444444444444444444444444444444";
const REVIEWER: &str = "a-reviewer";
const POLICY: &str = "[core]\nreviewer = \"a-reviewer\"\n";

/// Every seat this suite's arms deliver as, listed, with the reviewer: the
/// fleet a return's sentence names its builder among.
const BUILDERS: [&str; 5] = [REVIEWER, "s-return", "s-foreign", "s-bent", "s-absent"];

/// The delivery every arm reads: two numbered calls, the commit the review
/// takes its diff to, and the base it takes it from.
fn a_delivery() -> Delivered {
    let decision = |call: &str| Decision {
        call: call.to_string(),
        not_taken: "the other one".to_string(),
        because: "a clause".to_string(),
    };
    Delivered {
        commit: SHA.to_string(),
        branch: "a-seat/feat/the-work".to_string(),
        base: BASE.to_string(),
        files: vec!["a/file.rs".to_string()],
        checks: vec![CheckResult {
            check: "AC3".to_string(),
            result: "green".to_string(),
        }],
        suite: SuiteRun::Ran(Ran {
            command: "the workspace suite".to_string(),
            rc: 0,
        }),
        spec_corrections: Vec::new(),
        not_proven: vec![NotProven {
            surface: "what this arm did not run".to_string(),
            command: "cargo nextest run".to_string(),
        }],
        decisions: vec![decision("the first call"), decision("the second call")],
        covers: vec!["R7".to_string()],
    }
}

/// The same delivery as the prose note `fleet deliver` wrote before the
/// delivered entry: a record carrying only this carries no delivery.
fn a_prose_delivery() -> String {
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
    outcome: RingOutcome,
    calls: Mutex<Vec<(String, String)>>,
}

impl StubRing {
    fn new() -> StubRing {
        StubRing::answering(RingOutcome::Delivered)
    }

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
/// one call apiece. The index and the assignee carry seat ids, and the entry
/// the builder's typed actor, as dispatch and deliver write them.
fn a_delivered_item(store: &dyn Store, title: &str, builder: &str) -> String {
    let item = an_item(store, title);
    let seat = full(builder);
    store
        .assign(&item, &full(REVIEWER), "an-architect")
        .expect("the reviewer holds it");
    store
        .set_orders(
            &item,
            &format!(
                r#"{{"fleet.orders": {{"v": 1, "by": "an-architect", "kind": "dispatch", "seat": "{seat}", "at": "{AT}"}}}}"#
            ),
            "an-architect",
        )
        .expect("the order index lands");
    store
        .append(&item, &Body::Delivered(a_delivery()), &seat_actor(builder))
        .expect("the delivery lands");
    item
}

/// The item's last delivered entry, as review reads it.
fn delivered_entry(store: &dyn Store, item: &str) -> fleet_core::entry::Entry {
    let entries = store.timeline(item).expect("the timeline reads");
    Timeline(&entries)
        .last_delivery()
        .map(|(entry, _)| entry.clone())
        .expect("the item carries a delivered entry")
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
    let path = scratch.root().join(format!("findings-{label}.json"));
    std::fs::write(&path, body).expect("the file is written");
    path
}

/// A findings file in the shape `--return` reads: one finding per text, in
/// order.
fn findings(scratch: &dyn Rooted, label: &str, texts: &[&str]) -> PathBuf {
    let findings: Vec<serde_json::Value> = texts
        .iter()
        .map(|text| serde_json::json!({ "text": text }))
        .collect();
    file(
        scratch,
        label,
        &serde_json::json!({ "findings": findings }).to_string(),
    )
}

/// An item carrying this delivery and nothing else.
fn an_item_delivering(store: &dyn Store, title: &str, delivery: Delivered) -> String {
    let item = an_item(store, title);
    store
        .append(&item, &Body::Delivered(delivery), &seat_actor("s-header"))
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
            by: &seat_actor(REVIEWER),
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
            seats: &fleet_of(&BUILDERS),
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

/// `--show` over a delivered entry and no note: the size line measured from the
/// entry's base to its commit, a blank line, then the entry as `fleet item
/// show` renders it — its decisions among it. Nothing is written.
#[test]
fn show_prints_the_size_line_then_the_delivered_entry_and_writes_nothing() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item to look at", "s-show");
    assert_eq!(
        notes(&scratch.store, &item),
        "",
        "the premise: the delivery is an entry and there is no note"
    );
    let before = scratch.json(&item);
    let git = StubGit::answering(a_diff());

    let events = StubEvents::default();
    let (said, code) = run_watched(scratch, &item, Mode::Show, &git, &StubRing::new(), &events);

    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    assert_eq!(events.count(), 0, "--show announces nothing");
    assert_eq!(
        git.calls(),
        vec![format!("numstat {BASE} {SHA}")],
        "the entry's base, against the entry's commit"
    );
    let size = "size: 3 file(s), +42, -4 (1 binary) — tests: yes, executable: no";
    let entry = entry_lines(&delivered_entry(&scratch.store, &item)).join("\n");
    assert_eq!(
        said.out,
        format!("{size}\n\n{entry}\n"),
        "the size line, a blank line, then the entry"
    );
    for call in [
        "D1 the first call; not taken: the other one; because a clause",
        "D2 the second call; not taken: the other one; because a clause",
    ] {
        assert!(
            said.out.contains(call),
            "the decision `{call}`: {}",
            said.out
        );
    }
    assert_eq!(before, scratch.json(&item), "--show writes nothing");
}

/// THE CLEAN BREAK: a record whose notes carry the prose delivery `fleet
/// deliver` wrote before the entry, and no delivered entry, carries no
/// delivery. There is no migration.
#[test]
fn a_prose_delivery_note_with_no_delivered_entry_carries_no_delivery() {
    let scratch = &store();
    let item = an_item(&scratch.store, "an item delivered as prose");
    scratch
        .store
        .note(&item, &a_prose_delivery(), &full("s-prose"))
        .expect("the prose note lands");
    let git = StubGit::answering(a_diff());

    for mode in [Mode::Show, Mode::Land] {
        let (said, code) = run(scratch, &item, mode, &git, &StubRing::new());
        assert_eq!(code, 1, "{}{}", said.out, said.err);
        assert_eq!(
            said.stop,
            format!("{item} carries no delivery — a review reads one and there is none to read")
        );
    }
    assert!(
        git.calls().is_empty(),
        "nothing was measured: {:?}",
        git.calls()
    );
    assert_eq!(
        review::last_verdict(&notes(&scratch.store, &item)),
        None,
        "no verdict is written"
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
    assert_eq!(actor, seat_actor(REVIEWER).to_string());
    keys_agree(ITEM_REVIEWED, &payload, &[]);
    assert_eq!(payload["item"], serde_json::json!(item));
    assert_eq!(payload["commit"], serde_json::json!(SHA));
    assert_eq!(payload["verdict"], serde_json::json!(VERDICT_ACCEPTED));
    assert_eq!(payload["accepted"], serde_json::json!(2));
    assert_eq!(payload["overruled"], serde_json::json!(0));

    let verdict =
        review::last_verdict(&notes(&scratch.store, &item)).expect("a verdict is written");
    assert!(
        verdict.starts_with(&format!("ACCEPTED {SHA} — {}", seat_actor(REVIEWER))),
        "{verdict}"
    );
    assert!(verdict.contains("D1 ACCEPT"), "{verdict}");
    assert!(verdict.contains("D2 ACCEPT"), "{verdict}");
    assert!(
        verdict.contains("2 accepted, 0 overruled"),
        "the walk's last line: {verdict}"
    );
    assert!(
        !notes(&scratch.store, &item).contains("DELIVERED"),
        "the verdict walked an entry, and no note carries a delivery: {}",
        notes(&scratch.store, &item)
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
    let findings = findings(
        scratch,
        "quoting",
        &[
            "the note opens on the wrong word. It reads\nDELIVERED abc — someone\n\
           where it must read RE-DELIVERED, and a reader anchoring on the last\n\
           ACCEPTED abc — someone\nfinds this note instead.",
        ],
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
    // and not a verb that dropped the awkward lines. The finding opens on its
    // `F1`, and the lines that continue its text sit two further in.
    assert!(
        verdict.contains("\n  F1 the note opens on the wrong word. It reads\n"),
        "the finding is numbered by its place in the file: {verdict}"
    );
    assert!(
        verdict.contains("\n    DELIVERED abc — someone\n")
            && verdict.contains("\n    finds this note instead."),
        "its continuation lines are indented under it, not rewritten: {verdict}"
    );
    assert_eq!(
        delivered_entry(&scratch.store, &item).body,
        Body::Delivered(a_delivery()),
        "and the delivery is still the entry it was"
    );
}

#[test]
fn a_return_writes_the_findings_count_first_and_hands_the_item_back() {
    let scratch = ring();
    let bd = &Bd::at(&scratch.root);
    let builder = "s-return";
    let item = a_delivered_item(bd, "an item to return", builder);
    let findings = findings(
        scratch,
        "two",
        &[
            "the first finding, with what to measure.",
            "the second one.",
        ],
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
    assert_eq!(actor, seat_actor(REVIEWER).to_string());
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
        Some(format!("RETURNED WITH FINDINGS {SHA} — {}", seat_actor(REVIEWER)).as_str())
    );
    assert_eq!(
        lines.next(),
        Some("findings: 2"),
        "the count is the first line after the marker: {verdict}"
    );
    assert_eq!(
        verdict
            .lines()
            .filter(|line| line.trim_start().starts_with('F'))
            .collect::<Vec<_>>(),
        [
            "  F1 the first finding, with what to measure.",
            "  F2 the second one."
        ],
        "one F-line per finding, numbered by its place in the file: {verdict}"
    );

    assert_eq!(
        bd.show(&item).expect("the item reads").assignee.as_deref(),
        Some(full(builder).as_str()),
        "the item goes back to the seat the order named, by its id"
    );
    let rung = ring.calls();
    assert_eq!(rung.len(), 1);
    assert_eq!(rung[0].0, full(builder));
    assert!(rung[0].1.contains(&item), "{:?}", rung);
    assert!(
        rung[0].1.contains("with 2 finding(s)"),
        "the ring carries the count: {rung:?}"
    );
}

/// A return goes to `fleet.orders.seat`, which is the builder's full id: the
/// item is reassigned to that id and the id is rung, and where the builder has
/// no live session the line that says so names the seat as a person reads it —
/// its machine name — and not by the id the record holds.
#[test]
fn a_return_reassigns_to_the_orders_seat_id_and_an_absent_builder_is_named_by_label() {
    let scratch = &store();
    let builder = "s-absent";
    let item = a_delivered_item(&scratch.store, "an item returned to nobody live", builder);
    assert_eq!(
        scratch
            .store
            .show(&item)
            .expect("the item reads")
            .orders
            .and_then(|orders| orders.seat),
        Some(full(builder)),
        "the premise: the order index carries the builder's id"
    );
    let findings = findings(scratch, "absent", &["the one finding."]);
    let ring = StubRing::answering(RingOutcome::Absent);

    let (said, code) = run(
        scratch,
        &item,
        Mode::Return(&findings),
        &StubGit::answering(a_diff()),
        &ring,
    );
    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    assert_eq!(
        scratch
            .store
            .show(&item)
            .expect("the item reads")
            .assignee
            .as_deref(),
        Some(full(builder).as_str()),
        "the return reassigns to the order's seat id"
    );
    let rung = ring.calls();
    assert_eq!(rung.len(), 1, "{rung:?}");
    assert_eq!(rung[0].0, full(builder), "the id is rung");
    assert!(
        said.out.contains(&format!(
            "{}: no live session for {};",
            review::STANDS,
            agent(builder).machine_name()
        )),
        "{}",
        said.out
    );
    assert!(!said.out.contains(&full(builder)), "{}", said.out);
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
    let findings = findings(scratch, "foreign", &["the one finding."]);
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
        Some(full(builder).as_str()),
        "the return goes to the seat fleet's index names"
    );
    assert_eq!(ring.calls()[0].0, full(builder), "and that seat is rung");

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
    let findings = findings(scratch, "bent", &["the one finding."]);
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
        said.stop.contains("somebody-else") && said.stop.contains(&full(builder)),
        "the value read and the value wanted: {}",
        said.stop
    );
    assert!(
        said.stop.contains(&format!(
            "RERUN: bd update {item} --assignee {}",
            full(builder)
        )),
        "{}",
        said.stop
    );
    assert!(ring.calls().is_empty(), "rung: {:?}", ring.calls());
}

/// A findings file `--return` does not read is a usage stop, and it stops
/// before the hand-over: the item is still the reviewer's, no note is written,
/// nothing reaches the stream and nobody is rung.
///
/// Each file is one way a return is not one — a list that numbers nothing, a
/// key the schema does not name, and the numbered lines a return took before
/// its findings were JSON, which do not parse.
#[test]
fn a_findings_file_that_does_not_read_is_a_usage_error_and_writes_nothing() {
    let scratch = &store();
    let item = a_delivered_item(
        &scratch.store,
        "an item returned with findings that do not read",
        "s-none",
    );
    let before = scratch.json(&item);
    let schema = fleet_core::input::FINDINGS_SCHEMA;

    for (label, body, says) in [
        (
            "empty",
            r#"{"findings": []}"#,
            "numbers no finding — a return that numbers nothing is a question",
        ),
        (
            "unknown-key",
            r#"{"findings": [{"text": "the one finding.", "line": 12}]}"#,
            schema,
        ),
        (
            "not-json",
            "F1 the one finding.\nF2 the second one.\n",
            schema,
        ),
    ] {
        let unread = file(scratch, label, body);
        let ring = StubRing::new();
        let events = StubEvents::default();

        let (said, code) = run_watched(
            scratch,
            &item,
            Mode::Return(&unread),
            &StubGit::answering(a_diff()),
            &ring,
            &events,
        );

        assert_eq!(code, 2, "{label}: {}{}{}", said.out, said.err, said.stop);
        assert!(
            said.stop
                .starts_with(&format!("the findings at {}", unread.display()))
                && said.stop.contains(says),
            "{label}: the file, and why it does not read: {}",
            said.stop
        );
        assert_eq!(
            before,
            scratch.json(&item),
            "{label}: nothing was written, and the item is not reassigned"
        );
        assert_eq!(events.count(), 0, "{label}: nothing reached the stream");
        assert!(ring.calls().is_empty(), "{label}: rung: {:?}", ring.calls());
    }

    // The control: the same call with one finding gets past the read.
    let numbered = findings(scratch, "one", &["the one finding."]);
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
    assert_eq!(
        said.stop,
        format!("{item} carries no delivery — a review reads one and there is none to read")
    );
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

/// The walk answers per decision the entry lists, named by its place in the
/// list: the array is the count, so there is no header to disagree with it.
#[test]
fn the_decisions_read_are_the_ones_the_entry_lists() {
    let delivery = a_delivery();
    assert_eq!(review::decisions(&delivery), vec!["D1", "D2"]);

    let none = Delivered {
        decisions: Vec::new(),
        ..a_delivery()
    };
    assert!(
        review::decisions(&none).is_empty(),
        "an empty list is not a call"
    );
}

#[test]
fn land_over_a_delivery_listing_no_decisions_walks_zero() {
    let scratch = &store();
    let item = an_item_delivering(
        &scratch.store,
        "an item whose delivery lists no decisions",
        Delivered {
            decisions: Vec::new(),
            ..a_delivery()
        },
    );

    let (said, code) = run(
        scratch,
        &item,
        Mode::Land,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
    );

    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    let verdict =
        review::last_verdict(&notes(&scratch.store, &item)).expect("a verdict is written");
    assert!(
        verdict
            .lines()
            .any(|line| line == "decisions: 0 accepted, 0 overruled"),
        "no call is walked, and the count says so: {verdict}"
    );
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
