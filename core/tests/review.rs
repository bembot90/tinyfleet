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
    agent, fleet_of, full, keys_agree, seat_actor, seat_id, shared_store, signal, Rooted, Scratch,
    StubEvents,
};
use fleet_core::entry::{
    Body, CheckResult, Decision, Delivered, Entry, Finding, NotProven, Ran, Reviewed, Ruling,
    RulingKind, Size, SuiteRun, Timeline,
};
use fleet_core::item::brief::Packs;
use fleet_core::item::review::{self, Mode, Verdict, Wiring};
use fleet_core::item::show::entry_lines;
use fleet_core::item::{control_token, Change, Git, Project, Ring, RingOutcome, ITEM_ENTRY};
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::store::bd::Bd;
use fleet_core::store::{Item, OrderState, ReadProof, Store, StoreError};
use fleet_core::test_support::Board;

const AT: &str = "2026-09-09T04:05:06Z";
const SHA: &str = "3333333333333333333333333333333333333333";
/// The base the delivery was cut from, which the size line measures from.
const BASE: &str = "4444444444444444444444444444444444444444";
const REVIEWER: &str = "a-reviewer";
const POLICY: &str = "[core]\nreviewer = \"a-reviewer\"\n";

/// Every seat this suite's arms deliver as, listed, with the reviewer: the
/// fleet a return's sentence names its builder among.
const BUILDERS: [&str; 6] = [
    REVIEWER,
    "s-return",
    "s-foreign",
    "s-bent",
    "s-absent",
    CAROL,
];

/// A listed seat that is not the reviewer and holds nothing the arms review.
const CAROL: &str = "s-carol";

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

/// The same delivery as the prose `fleet deliver` wrote as a note before the
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

/// The real store with its reading bent once an assignment has been written,
/// so an arm can force the disagreement the return's read-back exists to catch
/// — while the holder the review reads first is still the real one. The
/// assignee is bent where one is named, and a token nothing wrote is planted
/// in the read's proof where one is named.
struct Doctored<'a> {
    inner: &'a dyn Store,
    assignee: Option<String>,
    plant: Option<&'static str>,
    assigned: std::sync::atomic::AtomicBool,
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
        if self.assigned.load(std::sync::atomic::Ordering::SeqCst) {
            if let Some(assignee) = &self.assignee {
                read.assignee = Some(assignee.clone());
            }
            if let Some(token) = self.plant {
                read.proof = ReadProof::of(format!("{}{token}", read.proof.as_str()));
            }
        }
        Ok(read)
    }

    fn update(
        &self,
        id: &fleet_core::store::ItemId,
        change: &fleet_core::store::Update,
        by: &fleet_core::seat::actor::Actor,
    ) -> Result<(), StoreError> {
        if change.assignee.is_some() {
            self.assigned
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.inner.update(id, change, by)
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

    fn close(
        &self,
        id: &fleet_core::store::ItemId,
        reason: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.inner.close(id, reason, by)
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
        .update(
            &fleet_core::store::ItemId::from(item.as_str()),
            &fleet_core::store::Update::assignee(seat_id(REVIEWER)),
            &fleet_core::test_support::the_test(),
        )
        .expect("the reviewer holds it");
    store
        .set_orders(
            &item,
            &format!(
                r#"{{"fleet.orders": {{"v": 1, "by": "run:an-architect", "kind": "dispatch", "seat": "{seat}", "at": "{AT}"}}}}"#
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
                title: title.to_string(),
                description: String::from("an item to review"),
                item_type: String::from("task"),
                labels: Vec::new(),
                priority: None,
            },
            &fleet_core::test_support::the_test(),
        )
        .expect("the item is filed")
        .to_string()
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

/// An item carrying this delivery, held by the reviewer as deliver leaves it.
fn an_item_delivering(store: &dyn Store, title: &str, delivery: Delivered) -> String {
    let item = an_item(store, title);
    store
        .update(
            &fleet_core::store::ItemId::from(item.as_str()),
            &fleet_core::store::Update::assignee(seat_id(REVIEWER)),
            &fleet_core::test_support::the_test(),
        )
        .expect("the reviewer holds it");
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
    /// The reviewed entry's id the verb answered, where it wrote one.
    entry: Option<String>,
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
    run_as(
        store,
        scratch,
        item,
        mode,
        &seat_actor(REVIEWER),
        git,
        ring,
        events,
    )
}

/// The same run with the ACTOR in the arm's hands: every arm above reviews as
/// the reviewer seat, and the holder arms are about who else asks.
#[allow(clippy::too_many_arguments)]
fn run_as(
    store: &dyn Store,
    scratch: &dyn Rooted,
    item: &str,
    mode: Mode,
    by: &Actor,
    git: &StubGit,
    ring: &StubRing,
    events: &StubEvents,
) -> (Said, u8) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let answer = review::review(
        &mut out,
        &mut err,
        &Verdict { item, by, mode },
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
    let (code, stop, entry) = match answer {
        Ok(read) => (0, String::new(), read.entry),
        Err(stop) => (stop.code, stop.message, None),
    };
    let said = Said {
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
        stop,
        entry,
    };
    (said, code)
}

/// The item's last reviewed entry, as land reads it.
fn last_review(store: &dyn Store, item: &str) -> Option<Entry> {
    let entries = store.timeline(item).expect("the timeline reads");
    Timeline(&entries)
        .last_review()
        .map(|(entry, _)| entry.clone())
}

/// The reviewed entry's body, for an arm that asserts it whole.
fn reviewed(entry: &Entry) -> &Reviewed {
    match &entry.body {
        Body::Reviewed(reviewed) => reviewed,
        other => panic!("the entry is a reviewed one: {other:?}"),
    }
}

/// The size [`a_diff`] measures, from the delivery's base: two text files and
/// a binary, one under a tests directory, nothing executable in a tree that
/// holds none of them.
fn a_diffs_size() -> Size {
    Size {
        files: 3,
        added: 42,
        deleted: 4,
        binary: 1,
        tests: true,
        executable: false,
        base: BASE.to_string(),
    }
}

// ---- the arms ----------------------------------------------------------------

/// `--show` over a delivered entry: the size line measured from the
/// entry's base to its commit, a blank line, then the entry as `fleet item
/// show` renders it — its decisions among it. Nothing is written.
#[test]
fn show_prints_the_size_line_then_the_delivered_entry_and_writes_nothing() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item to look at", "s-show");
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

/// THE CLEAN BREAK: a record carrying the prose delivery `fleet deliver` wrote
/// as a note before the entry — here a comment on the timeline that is no
/// entry — and no delivered entry, carries no delivery. There is no migration.
#[test]
fn a_prose_delivery_with_no_delivered_entry_carries_no_delivery() {
    let scratch = &store();
    let item = an_item(&scratch.store, "an item delivered as prose");
    scratch.hand_to(&item, &full(REVIEWER));
    scratch
        .store
        .comment(&item, &full("s-prose"), &a_prose_delivery());
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
        last_review(&scratch.store, &item),
        None,
        "no verdict is written"
    );
    assert_eq!(
        scratch.store.timeline(&item).expect("the timeline reads"),
        Vec::new(),
        "and the prose is no entry"
    );
}

/// `--land` leaves the timeline ending in the accept: this commit whole, the
/// size measured from the delivery's base, and every call the delivery listed
/// ruled accepted by its number — appended by the reviewer, answered as the id
/// the timeline holds it under.
#[test]
fn land_appends_the_accept_walking_every_call_the_delivery_numbered() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item to accept", "s-land");
    let git = StubGit::answering(a_diff());

    let events = StubEvents::default();
    let (said, code) = run_watched(scratch, &item, Mode::Land, &git, &StubRing::new(), &events);
    assert_eq!(code, 0, "{}", said.err);

    let entries = scratch.store.timeline(&item).expect("the timeline reads");
    let last = entries.last().expect("the timeline carries entries");
    assert_eq!(
        last.body,
        Body::Reviewed(Reviewed {
            verdict: fleet_core::entry::Verdict::Accepted,
            commit: SHA.to_string(),
            size: a_diffs_size(),
            walk: vec![
                Ruling {
                    decision: 1,
                    ruling: RulingKind::Accept,
                },
                Ruling {
                    decision: 2,
                    ruling: RulingKind::Accept,
                },
            ],
            findings: Vec::new(),
        }),
        "the timeline ends in the accept"
    );
    assert_eq!(last.by, seat_actor(REVIEWER), "appended by the reviewer");
    assert_eq!(
        said.entry.as_deref(),
        Some(last.id.as_str()),
        "the id answered is the entry's"
    );
    assert_eq!(
        git.calls(),
        vec![format!("numstat {BASE} {SHA}")],
        "one measurement, from the entry's base"
    );
    assert_eq!(
        said.out, "size: 3 file(s), +42, -4 (1 binary) — tests: yes, executable: no\n",
        "the line printed is the same measurement"
    );

    // The one event: the reviewed entry's signal, by the reviewer. The verdict
    // and the walk are the entry's.
    assert_eq!(
        events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            seat_actor(REVIEWER).to_string(),
            signal(&item, &last.id, "reviewed"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &events.all()[0].2, &[]);
}

/// A STORE THAT TAKES THE VERDICT AND DOES NOT KEEP IT is caught by the entry's
/// own read-back: exit 3, saying the verdict STANDS — the write was made, and
/// a caller that read this as nothing having happened would review twice.
/// Nothing is announced.
#[test]
fn a_reviewed_entry_the_store_does_not_keep_exits_three_and_the_verdict_stands() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item whose store forgets", "s-unkept");
    scratch.store.ignore_writes();
    let events = StubEvents::default();

    let (said, code) = run_watched(
        scratch,
        &item,
        Mode::Land,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
        &events,
    );

    assert_eq!(code, 3, "{}{}{}", said.out, said.err, said.stop);
    assert!(
        said.stop.contains("does not hold the reviewed entry"),
        "the stop names the entry the timeline does not hold: {}",
        said.stop
    );
    assert!(
        said.stop
            .ends_with(&format!("\n  the verdict on {item} STANDS")),
        "{}",
        said.stop
    );
    assert_eq!(events.count(), 0, "nothing is announced");
}

/// A finding that QUOTES a marker at column zero is carried whole: a finding
/// is a field of the entry and not a line of a note, so no text in it can open
/// or end anything, and the entry reads back as the reviewer wrote it.
#[test]
fn a_finding_quoting_a_marker_is_carried_whole() {
    let scratch = &store();
    let builder = "s-quoting";
    let item = a_delivered_item(
        &scratch.store,
        "an item returned with a quoted marker",
        builder,
    );
    let quoting = "the note opens on the wrong word. It reads\nDELIVERED abc — someone\n\
                   where it must read RE-DELIVERED, and a reader anchoring on the last\n\
                   ACCEPTED abc — someone\nfinds this note instead.";
    let findings = findings(scratch, "quoting", &[quoting]);
    let git = StubGit::answering(a_diff());
    let ring = StubRing::new();

    let (said, code) = run(scratch, &item, Mode::Return(&findings), &git, &ring);
    assert_eq!(code, 0, "{}", said.err);

    let entry = last_review(&scratch.store, &item).expect("a verdict is written");
    assert_eq!(
        reviewed(&entry).findings,
        vec![Finding {
            text: quoting.to_string()
        }],
        "the finding is the reviewer's text, byte for byte"
    );
    assert_eq!(
        delivered_entry(&scratch.store, &item).body,
        Body::Delivered(a_delivery()),
        "and the delivery is still the entry it was"
    );
}

/// `--return` through `bd`: the timeline ends in the return — this commit, the
/// size, no walk and the findings in the file's order — appended by the
/// reviewer, with the item handed to the seat the order index names.
#[test]
fn a_return_appends_the_findings_and_hands_the_item_back() {
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

    let entries = bd.timeline(&item).expect("the timeline reads");
    let last = entries.last().expect("the timeline carries entries");
    // The one event: the return is the same entry kind as the accept, and its
    // signal says no more than the accept's does.
    assert_eq!(
        events.all(),
        vec![(
            ITEM_ENTRY.to_string(),
            seat_actor(REVIEWER).to_string(),
            signal(&item, &last.id, "reviewed"),
        )],
        "exactly one event, the entry's signal"
    );
    keys_agree(ITEM_ENTRY, &events.all()[0].2, &[]);
    assert_eq!(
        last.body,
        Body::Reviewed(Reviewed {
            verdict: fleet_core::entry::Verdict::Returned,
            commit: SHA.to_string(),
            size: a_diffs_size(),
            walk: Vec::new(),
            findings: vec![
                Finding {
                    text: "the first finding, with what to measure.".to_string(),
                },
                Finding {
                    text: "the second one.".to_string(),
                },
            ],
        }),
        "the timeline ends in the return, its findings in the file's order"
    );
    assert_eq!(last.by, seat_actor(REVIEWER), "appended by the reviewer");
    assert_eq!(said.entry.as_deref(), Some(last.id.as_str()));

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
    let order = scratch.store.show(&item).expect("the item reads").order;
    assert!(
        matches!(&order, OrderState::Ordered(index) if index.seat.map(|seat| seat.to_string()) == Some(full(builder))),
        "the premise: the order index carries the builder's id: {order:?}"
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
    assert_eq!(
        last_review(&scratch.store, &accepted).map(|entry| reviewed(&entry).verdict),
        Some(fleet_core::entry::Verdict::Accepted),
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
        assignee: Some("somebody-else".to_string()),
        plant: None,
        assigned: std::sync::atomic::AtomicBool::new(false),
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

/// THE NEGATIVE CONTROL on the return's read-back: a read whose proof carries
/// a token nothing wrote is not reading this item, however right its assignee
/// reads — a could-not-tell, and nobody is rung.
#[test]
fn the_negative_control_catches_a_planted_token_on_a_return() {
    let scratch = &store();
    let builder = "s-planted";
    let item = a_delivered_item(&scratch.store, "an item whose read is not its own", builder);
    let findings = findings(scratch, "planted", &["the one finding."]);
    let planted = Doctored {
        inner: &scratch.store,
        assignee: None,
        plant: Some(control_token()),
        assigned: std::sync::atomic::AtomicBool::new(false),
    };
    let ring = StubRing::new();
    let events = StubEvents::default();

    let (said, code) = run_through(
        &planted,
        scratch,
        &item,
        Mode::Return(&findings),
        &StubGit::answering(a_diff()),
        &ring,
        &events,
    );

    assert_eq!(code, 3, "{}{}{}", said.out, said.err, said.stop);
    assert!(
        said.stop.contains(control_token()) && said.stop.contains("not reading this item"),
        "{}",
        said.stop
    );
    assert_eq!(
        events.count(),
        0,
        "a read that is not this item's announces nothing"
    );
    assert!(ring.calls().is_empty(), "rung: {:?}", ring.calls());
}

/// A findings file `--return` does not read is a usage stop, and it stops
/// before the hand-over: the item is still the reviewer's, nothing is written,
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
    let entry = last_review(&scratch.store, &item).expect("a verdict is written");
    let accept = reviewed(&entry);
    assert_eq!(accept.verdict, fleet_core::entry::Verdict::Accepted);
    assert!(
        accept.walk.is_empty(),
        "no call is walked: {:?}",
        accept.walk
    );
}

/// The verdict template retires with the note: the binary's own defaults carry
/// no `assets/verdict.md`, and the shadow registry — the list of paths a pack
/// may replace — names no such slot.
#[test]
fn the_defaults_carry_no_verdict_template_and_the_registry_no_row_for_one() {
    assert!(
        fleet_core::embedded::bytes("assets/verdict.md").is_none(),
        "the embedded defaults carry no verdict template"
    );
    let registry = String::from_utf8(
        fleet_core::embedded::bytes("assets/shadow-registry.toml")
            .expect("the defaults carry the registry")
            .to_vec(),
    )
    .expect("the registry is text");
    assert!(
        !registry.contains("assets/verdict.md"),
        "the registry names no verdict template:\n{registry}"
    );
    // The control: the registry is the one read, and it names its neighbour.
    assert!(
        registry.contains("assets/findings.schema.json"),
        "{registry}"
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

// ---- who writes a verdict ----------------------------------------------------

/// The item's timeline, whole, for an arm asserting a refusal left it as it
/// was.
fn timeline(store: &dyn Store, item: &str) -> Vec<fleet_core::entry::Entry> {
    store.timeline(item).expect("the timeline reads")
}

/// fleet-pl6 (b): A VERDICT IS THE HOLDER'S. A seat that does not hold the item
/// — here a listed seat, while the reviewer holds it — writes neither verdict:
/// each writing mode is exit 1 naming the holder and the seat, and nothing is
/// measured, written, announced or rung. `--show` writes nothing, so the same
/// seat still reads.
///
/// RED-PROOF: HEAD wrote the accept `s-carol` asked for (items#4).
#[test]
fn a_seat_that_does_not_hold_the_item_writes_no_verdict() {
    let scratch = &store();
    let item = a_delivered_item(
        &scratch.store,
        "an item another seat tries to review",
        "s-held",
    );
    let carol = seat_actor(CAROL);
    let before = timeline(&scratch.store, &item);
    let findings = findings(scratch, "carol", &["the one finding."]);

    for mode in [Mode::Land, Mode::Return(&findings)] {
        let git = StubGit::answering(a_diff());
        let ring = StubRing::new();
        let events = StubEvents::default();
        let (said, code) = run_as(
            &scratch.store,
            scratch,
            &item,
            mode,
            &carol,
            &git,
            &ring,
            &events,
        );
        assert_eq!(code, 1, "{}{}{}", said.out, said.err, said.stop);
        assert_eq!(
            said.stop,
            format!(
                "{item} is held by {} and not by {carol} — a verdict is the holder's, and a \
                 delivered item's holder is its reviewer",
                full(REVIEWER)
            )
        );
        assert!(
            git.calls().is_empty(),
            "nothing was measured: {:?}",
            git.calls()
        );
        assert_eq!(events.count(), 0, "nothing reached the stream");
        assert!(ring.calls().is_empty(), "rung: {:?}", ring.calls());
    }
    assert_eq!(
        timeline(&scratch.store, &item),
        before,
        "the timeline is unchanged"
    );
    assert_eq!(
        scratch
            .store
            .show(&item)
            .expect("the item reads")
            .assignee
            .as_deref(),
        Some(full(REVIEWER).as_str()),
        "the item is still the reviewer's"
    );

    // The control: the same seat's --show reads the delivery.
    let (said, code) = run_as(
        &scratch.store,
        scratch,
        &item,
        Mode::Show,
        &carol,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
        &StubEvents::default(),
    );
    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    assert_eq!(timeline(&scratch.store, &item), before);
}

/// A run's record, as `fleet run` files one: an item under the run label.
fn a_run_record(store: &dyn Store) -> String {
    store
        .create(
            &fleet_core::store::NewItem {
                title: String::from("a run of takeoff"),
                description: String::from("a run's record"),
                item_type: String::from("task"),
                labels: vec![fleet_core::item::run::LABEL.to_string()],
                priority: None,
            },
            &fleet_core::test_support::the_test(),
        )
        .expect("the run's record is filed")
        .to_string()
}

fn the_run(record: &str) -> Actor {
    Actor {
        kind: ActorKind::Run,
        id: record.to_string(),
    }
}

/// A RUN REVIEWS AS THE `[core] reviewer`, as a run's landing already closes as
/// it: `run:<record>` writes the verdict on an item that seat holds, and the
/// entry is the run's. An item another seat holds is refused naming the run,
/// the reviewer and the holder; a run id naming no run record is refused as
/// land refuses it.
#[test]
fn a_run_reviews_as_the_core_reviewer_and_only_what_that_seat_holds() {
    let scratch = &store();
    let record = a_run_record(&scratch.store);
    let run = the_run(&record);

    let item = a_delivered_item(&scratch.store, "an item a run accepts", "s-run-built");
    let (said, code) = run_as(
        &scratch.store,
        scratch,
        &item,
        Mode::Land,
        &run,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
        &StubEvents::default(),
    );
    assert_eq!(code, 0, "{}{}", said.err, said.stop);
    let entries = timeline(&scratch.store, &item);
    let (entry, _) = Timeline(&entries)
        .last_review()
        .expect("the run's verdict is on the timeline");
    assert_eq!(entry.by, run, "the entry is the run's");

    let other = a_delivered_item(&scratch.store, "an item another seat holds", "s-run-held");
    scratch.hand_to(&other, &full("s-run-held"));
    let before = timeline(&scratch.store, &other);
    let (said, code) = run_as(
        &scratch.store,
        scratch,
        &other,
        Mode::Land,
        &run,
        &StubGit::answering(a_diff()),
        &StubRing::new(),
        &StubEvents::default(),
    );
    assert_eq!(code, 1, "{}{}", said.err, said.stop);
    assert_eq!(
        said.stop,
        format!(
            "run {record} reviews as the [core] reviewer {}, and {other} is held by {}",
            full(REVIEWER),
            full("s-run-held")
        )
    );
    assert_eq!(timeline(&scratch.store, &other), before);

    let (said, code) = run_as(
        &scratch.store,
        scratch,
        &item,
        Mode::Land,
        &the_run("fx-nothing"),
        &StubGit::answering(a_diff()),
        &StubRing::new(),
        &StubEvents::default(),
    );
    assert_eq!(code, 1, "{}{}", said.err, said.stop);
    assert_eq!(said.stop, "run:fx-nothing names no run record");
}

/// A routine and the controller hold nothing and review nothing: each writing
/// mode refuses them by their kind.
#[test]
fn a_routine_or_the_controller_writes_no_verdict() {
    let scratch = &store();
    let item = a_delivered_item(&scratch.store, "an item a routine tries", "s-kind");
    let before = timeline(&scratch.store, &item);

    for (kind, word) in [
        (ActorKind::Routine, "routine"),
        (ActorKind::Controller, "controller"),
    ] {
        let by = Actor {
            kind,
            id: full(REVIEWER),
        };
        let (said, code) = run_as(
            &scratch.store,
            scratch,
            &item,
            Mode::Land,
            &by,
            &StubGit::answering(a_diff()),
            &StubRing::new(),
            &StubEvents::default(),
        );
        assert_eq!(code, 1, "{word}: {}{}", said.err, said.stop);
        assert_eq!(said.stop, format!("a {word} writes no verdict"));
    }
    assert_eq!(timeline(&scratch.store, &item), before);
}
