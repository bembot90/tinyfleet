//! `fleet item show`'s two shapes: the rendering a person and a brief read, and
//! the document a caller parses.
//!
//! No store here. The item and its entries are built by hand, because what this
//! suite proves is the text one reading becomes — the golden below is the
//! spec's lines, written out whole, so a line that moved is a diff and not a
//! substring that still happens to be there.

use fleet_core::entry::{
    to_json, About, Body, CheckResult, CheckRow, Choice, Classification, Clearance, Cleared,
    Decision, Delivered, Entry, Finding, Held, HoldReason, Landed, NotProven, NotTested, OrderKind,
    OrderWithdrawn, Ordered, Ran, Reviewed, Ruling, RulingKind, Size, SpecCorrection, SuiteRun,
    Verdict, Withdrawal, WorkBranch,
};
use fleet_core::item::show::{document, entry_lines, render};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use fleet_core::store::{Item, Orders};

const C1: &str = "1111111111111111111111111111111111111111";
const C2: &str = "2222222222222222222222222222222222222222";
const C3: &str = "3333333333333333333333333333333333333333";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const SEAT: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const REVIEWER: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

fn seat() -> SeatId {
    SeatId::parse(SEAT).expect("a hand-written seat id parses")
}

fn actor(text: &str) -> Actor {
    Actor::typed(text).expect("typed").expect("an actor")
}

fn words(text: &str) -> String {
    text.to_string()
}

fn entry(n: u32, by: &str, body: Body) -> Entry {
    Entry {
        id: format!("c-{n}"),
        at: format!("2026-09-24T10:00:0{n}Z"),
        by: actor(by),
        body,
    }
}

/// An item carrying every field the rendering reads.
fn an_item() -> Item {
    Item {
        id: words("fx-1"),
        title: words("an item with a record"),
        description: words("the item's own words\nover two lines"),
        status: words("in_progress"),
        item_type: words("task"),
        labels: vec![words("fleet"), words("core")],
        assignee: Some(words(SEAT)),
        orders: Some(Orders {
            by: Some(words("run:lead-1")),
            kind: Some(words("dispatch")),
            seat: Some(words(SEAT)),
            at: Some(words("2026-09-23T10:00:00Z")),
        }),
        has_orders_key: true,
        blockers: vec![words("fx-2"), words("fx-3")],
        ..Item::default()
    }
}

/// One entry of each kind, in the order an item meets them.
fn a_timeline() -> Vec<Entry> {
    let builder = format!("seat:{SEAT}");
    let reviewer = format!("seat:{REVIEWER}");
    vec![
        entry(
            1,
            "run:lead-1",
            Body::Ordered(Ordered {
                order: OrderKind::Dispatch,
                seat: Some(seat()),
            }),
        ),
        entry(
            2,
            "run:lead-1",
            Body::OrderWithdrawn(OrderWithdrawn {
                why: Withdrawal::SpawnRefused,
                seat: Some(seat()),
                cause: Some(words("the spawner is at its ceiling")),
            }),
        ),
        entry(
            3,
            &builder,
            Body::Delivered(Delivered {
                commit: words(C1),
                branch: words("work/orla-4a5b"),
                base: words(BASE),
                files: vec![words("core/src/entry.rs"), words("core/src/item/show.rs")],
                checks: vec![CheckResult {
                    check: words("the build is clean"),
                    result: words("PASS"),
                }],
                suite: SuiteRun::Ran(Ran {
                    command: words("cargo nextest run -p fleet-core"),
                    rc: 0,
                }),
                spec_corrections: vec![SpecCorrection {
                    premise: words("the module list sits at 100-116"),
                    refuted_by: words("it moved to 104-121"),
                }],
                not_proven: vec![NotProven {
                    surface: words("a real bd store"),
                    command: words("fleet item show fx-1"),
                }],
                decisions: vec![Decision {
                    call: words("read the author with Actor::typed"),
                    not_taken: words("a new parser"),
                    because: words("typed is the one reader"),
                }],
                covers: vec![words("fleet-zlk.1"), words("fleet-zlk.4")],
            }),
        ),
        entry(
            4,
            &reviewer,
            Body::Reviewed(Reviewed {
                verdict: Verdict::Returned,
                commit: words(C1),
                size: Size {
                    files: 2,
                    added: 120,
                    deleted: 8,
                    binary: 1,
                    tests: true,
                    executable: false,
                    base: words(BASE),
                },
                walk: Vec::new(),
                findings: vec![
                    Finding {
                        text: words("the golden misses a kind"),
                    },
                    Finding {
                        text: words("a finding that runs\nover two lines"),
                    },
                ],
            }),
        ),
        entry(
            5,
            &builder,
            Body::Held(Held {
                hold: words("fx-h1"),
                reason: HoldReason::Ask,
                question: words("where does the value come from?"),
                context: Some(words("the spec names it\nand does not say where it lives")),
                options: vec![
                    Choice {
                        letter: words("A"),
                        text: words("read it off the item"),
                    },
                    Choice {
                        letter: words("B"),
                        text: words("take it from the pack"),
                    },
                ],
                branch: Some(words("work/orla-4a5b")),
                commit: Some(words(C2)),
                run_hash: None,
                about: Some(About {
                    items: vec![words("fx-2"), words("fx-3")],
                    commit: Some(words(C2)),
                    licenses: words("A"),
                }),
            }),
        ),
        entry(
            6,
            &reviewer,
            Body::Cleared(Cleared {
                hold: words("fx-h1"),
                how: Clearance::Answer,
                letter: Some(words("A")),
                text: Some(words("and keep it on the item")),
            }),
        ),
        entry(
            7,
            &reviewer,
            Body::Landed(Landed {
                sha: words(C3),
                old: words(C2),
                squash_of: words(C1),
                run: Some(words("fx-r9")),
                test: SuiteRun::NotTested(NotTested {
                    not_tested: words("no --test was handed in"),
                }),
                checks: vec![CheckRow {
                    check: words("verdict"),
                    verdict: words("PASS"),
                    evidence: words("ACCEPTED on the commit\nread off the timeline"),
                }],
                work_branch: WorkBranch {
                    branch: Some(words("work/orla-4a5b")),
                    classification: Classification::CarriesUnlandedWork,
                },
            }),
        ),
    ]
}

/// The spec's lines, whole: the fields, the blocked-by line, the description
/// as it is, the count, and one block per kind with its details four spaces
/// in, each list's rows two further, and every continuation line two under the
/// line it starts on.
#[test]
fn an_item_with_one_entry_of_each_kind_renders_the_golden_text() {
    let golden = format!(
        "\
fx-1 · an item with a record  [in_progress]
type task · labels fleet, core · assignee {SEAT}
order dispatch by run:lead-1 at 2026-09-23T10:00:00Z, seat {SEAT}
blocked by fx-2, fx-3

the item's own words
over two lines

timeline (7 entries)
2026-09-24T10:00:01Z  run:lead-1  ordered dispatch → {SEAT}
2026-09-24T10:00:02Z  run:lead-1  order withdrawn (spawn_refused), seat {SEAT}: the spawner is at its ceiling
2026-09-24T10:00:03Z  seat:{SEAT}  delivered {C1} on work/orla-4a5b, base {BASE}
    files: core/src/entry.rs, core/src/item/show.rs
    checks:
      - the build is clean: PASS
    suite: cargo nextest run -p fleet-core, rc 0
    spec corrections:
      - the module list sits at 100-116 — refuted by it moved to 104-121
    not proven:
      - a real bd store — fleet item show fx-1
    decisions:
      D1 read the author with Actor::typed; not taken: a new parser; because typed is the one reader
    covers: fleet-zlk.1, fleet-zlk.4
2026-09-24T10:00:04Z  seat:{REVIEWER}  returned {C1} with 2 finding(s)
    size: 2 file(s), +120, -8 (1 binary) — tests: yes, executable: no, against {BASE}
    F1 the golden misses a kind
    F2 a finding that runs
      over two lines
2026-09-24T10:00:05Z  seat:{SEAT}  held fx-h1 — ask: where does the value come from?
    the spec names it
    and does not say where it lives
    A. read it off the item
    B. take it from the pack
    on work/orla-4a5b at {C2}
    about fx-2, fx-3 at {C2}; A licenses a landing
2026-09-24T10:00:06Z  seat:{REVIEWER}  cleared fx-h1 — answered A: and keep it on the item
2026-09-24T10:00:07Z  seat:{REVIEWER}  landed {C3} (range {C2}..{C3}; squash of {C1}; through run fx-r9)
    test: NOT TESTED — no --test was handed in
    1. verdict          PASS       ACCEPTED on the commit
      read off the timeline
    work branch: work/orla-4a5b — carries_unlanded_work"
    );
    let rendered = render(&an_item(), &a_timeline());
    assert_eq!(
        rendered, golden,
        "\n--- rendered ---\n{rendered}\n--- golden ---\n{golden}"
    );
}

/// The other form of each kind that has one: an order to a transient seat, a
/// retire's withdrawal naming nothing else, a delivery with every list empty
/// and no suite run, an acceptance with its walk and one with none, a hold on a
/// run about nothing, a bare answer and a cancel, and a landing that ran its
/// test with no run and no branch.
#[test]
fn the_other_form_of_each_kind_renders_its_own_line() {
    let cases: Vec<(Body, String)> = vec![
        (
            Body::Ordered(Ordered {
                order: OrderKind::Dispatch,
                seat: None,
            }),
            "ordered dispatch → a transient seat, not yet named".to_string(),
        ),
        (
            Body::OrderWithdrawn(OrderWithdrawn {
                why: Withdrawal::Retire,
                seat: None,
                cause: None,
            }),
            "order withdrawn (retire)".to_string(),
        ),
        (
            Body::Delivered(Delivered {
                commit: words(C1),
                branch: words("work/a"),
                base: words(BASE),
                files: vec![words("a.rs")],
                checks: Vec::new(),
                suite: SuiteRun::NotTested(NotTested {
                    not_tested: words("no suite reaches a.rs"),
                }),
                spec_corrections: Vec::new(),
                not_proven: vec![NotProven {
                    surface: words("the whole of it"),
                    command: words("cargo test"),
                }],
                decisions: Vec::new(),
                covers: Vec::new(),
            }),
            format!(
                "delivered {C1} on work/a, base {BASE}
    files: a.rs
    checks: none
    suite: NOT TESTED — no suite reaches a.rs
    spec corrections: none
    not proven:
      - the whole of it — cargo test
    decisions: none
    covers: none"
            ),
        ),
        (
            Body::Reviewed(Reviewed {
                verdict: Verdict::Accepted,
                commit: words(C1),
                size: Size {
                    files: 1,
                    added: 3,
                    deleted: 0,
                    binary: 0,
                    tests: false,
                    executable: true,
                    base: words(BASE),
                },
                walk: vec![
                    Ruling {
                        decision: 1,
                        ruling: RulingKind::Accept,
                    },
                    Ruling {
                        decision: 2,
                        ruling: RulingKind::Overrule,
                    },
                ],
                findings: Vec::new(),
            }),
            format!(
                "accepted {C1}
    size: 1 file(s), +3, -0 — tests: no, executable: yes, against {BASE}
    walk: D1 accept, D2 overrule"
            ),
        ),
        (
            Body::Reviewed(Reviewed {
                verdict: Verdict::Accepted,
                commit: words(C1),
                size: Size {
                    files: 1,
                    added: 3,
                    deleted: 0,
                    binary: 0,
                    tests: true,
                    executable: false,
                    base: words(BASE),
                },
                walk: Vec::new(),
                findings: Vec::new(),
            }),
            format!(
                "accepted {C1}
    size: 1 file(s), +3, -0 — tests: yes, executable: no, against {BASE}
    walk: none"
            ),
        ),
        (
            Body::Held(Held {
                hold: words("fx-h2"),
                reason: HoldReason::MaxCrashes,
                question: words("the run crashed three times: retry it?"),
                context: None,
                options: vec![Choice {
                    letter: words("A"),
                    text: words("retry"),
                }],
                branch: None,
                commit: None,
                run_hash: Some(words("abc123")),
                about: None,
            }),
            "held fx-h2 — max_crashes: the run crashed three times: retry it?
    A. retry
    on the run's hash abc123"
                .to_string(),
        ),
        (
            Body::Cleared(Cleared {
                hold: words("fx-h2"),
                how: Clearance::Answer,
                letter: Some(words("A")),
                text: None,
            }),
            "cleared fx-h2 — answered A".to_string(),
        ),
        (
            Body::Cleared(Cleared {
                hold: words("fx-h2"),
                how: Clearance::Cancel,
                letter: None,
                text: None,
            }),
            "cleared fx-h2 — cancelled".to_string(),
        ),
        (
            Body::Landed(Landed {
                sha: words(C3),
                old: words(C2),
                squash_of: words(C1),
                run: None,
                test: SuiteRun::Ran(Ran {
                    command: words("make test"),
                    rc: 0,
                }),
                checks: vec![
                    CheckRow {
                        check: words("verdict"),
                        verdict: words("PASS"),
                        evidence: words("ACCEPTED"),
                    },
                    CheckRow {
                        check: words("trunk"),
                        verdict: words("PASS"),
                        evidence: words("not moved"),
                    },
                ],
                work_branch: WorkBranch {
                    branch: None,
                    classification: Classification::NotGiven,
                },
            }),
            format!(
                "landed {C3} (range {C2}..{C3}; squash of {C1})
    test: make test, rc 0
    1. verdict          PASS       ACCEPTED
    2. trunk            PASS       not moved
    work branch: (none) — not_given"
            ),
        ),
    ];
    for (body, wanted) in cases {
        let kind = body.kind();
        let lines = entry_lines(&entry(1, "routine:nightly", body)).join("\n");
        let wanted = format!("2026-09-24T10:00:01Z  routine:nightly  {wanted}");
        assert_eq!(lines, wanted, "the {kind} entry");
    }
}

/// The fields' absences each say so: no labels, no assignee, no order index,
/// no blockers (and so no line for them), no description, no entries.
#[test]
fn an_item_with_nothing_on_it_says_none_where_each_field_is() {
    let bare = Item {
        id: words("fx-9"),
        title: words("a bare item"),
        status: words("open"),
        item_type: words("bug"),
        ..Item::default()
    };
    assert_eq!(
        render(&bare, &[]),
        "\
fx-9 · a bare item  [open]
type bug · labels none · assignee none
order none

(no description)

timeline (no entries)"
    );

    // A key holding no readable index is its own answer, and an index missing
    // its seat is an order nobody has been named for yet.
    let unreadable = Item {
        has_orders_key: true,
        ..bare.clone()
    };
    assert!(render(&unreadable, &[]).contains("\norder unreadable\n"));
    let transient = Item {
        orders: Some(Orders {
            by: Some(words("run:lead-1")),
            kind: Some(words("dispatch")),
            seat: None,
            at: Some(words("2026-09-23T10:00:00Z")),
        }),
        has_orders_key: true,
        ..bare
    };
    assert!(render(&transient, &[])
        .contains("\norder dispatch by run:lead-1 at 2026-09-23T10:00:00Z, seat not yet named\n"));
}

/// The document carries the spec's keys and no others, the index as its four
/// fields, and each entry as the entry model's own JSON.
#[test]
fn the_document_carries_the_fields_and_each_entry_as_the_entry_model_writes_it() {
    let item = an_item();
    let timeline = a_timeline();
    let doc = document(&item, &timeline);

    let mut keys: Vec<&str> = doc
        .as_object()
        .expect("the document is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "assignee",
            "blockers",
            "description",
            "id",
            "labels",
            "order",
            "status",
            "timeline",
            "title",
            "type"
        ]
    );
    assert_eq!(doc["id"], "fx-1");
    assert_eq!(doc["title"], "an item with a record");
    assert_eq!(doc["description"], "the item's own words\nover two lines");
    assert_eq!(doc["status"], "in_progress");
    assert_eq!(doc["type"], "task");
    assert_eq!(doc["labels"], serde_json::json!(["fleet", "core"]));
    assert_eq!(doc["assignee"], SEAT);
    assert_eq!(
        doc["order"],
        serde_json::json!({
            "by": "run:lead-1",
            "kind": "dispatch",
            "seat": SEAT,
            "at": "2026-09-23T10:00:00Z",
        })
    );
    assert_eq!(doc["blockers"], serde_json::json!(["fx-2", "fx-3"]));
    let entries = doc["timeline"].as_array().expect("the timeline is a list");
    assert_eq!(entries.len(), timeline.len());
    for (n, entry) in timeline.iter().enumerate() {
        assert_eq!(entries[n], to_json(entry), "timeline[{n}]");
    }

    // The three answers the index has, and an assignee nobody holds as null.
    let bare = Item {
        id: words("fx-9"),
        ..Item::default()
    };
    let doc = document(&bare, &[]);
    assert_eq!(doc["order"], serde_json::Value::Null);
    assert_eq!(doc["assignee"], serde_json::Value::Null);
    assert_eq!(doc["timeline"], serde_json::json!([]));
    let unreadable = Item {
        has_orders_key: true,
        ..bare
    };
    assert_eq!(
        document(&unreadable, &[])["order"],
        serde_json::json!({ "unreadable": true })
    );
}
