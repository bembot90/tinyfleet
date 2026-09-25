//! An item's record as typed entries: the wire text one entry is, the decoder
//! that tells a person's comment from an entry and refuses a malformed one, the
//! per-kind rules, and the fold every reader asks.
//!
//! No store here. Every entry is built by hand and every text is written out,
//! because what this suite proves is the shape of the text a store will keep —
//! the store that keeps it is the next piece's.

use fleet_core::entry::{
    decode, encode, full_sha, read_row, to_json, About, Body, CheckResult, CheckRow, Choice,
    Classification, Clearance, Cleared, Decision, Delivered, Entry, Finding, Held, HoldReason,
    Landed, NotProven, NotTested, OrderWithdrawn, Ordered, Ran, Read, Reviewed, Ruling, RulingKind,
    Size, SpecCorrection, SuiteRun, Timeline, Verdict, Withdrawal, WorkBranch, KEY, KINDS, VERSION,
};
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::seat::identity::SeatId;
use fleet_core::store::OrderKind;

const C1: &str = "1111111111111111111111111111111111111111";
const C2: &str = "2222222222222222222222222222222222222222";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const SEAT: &str = "0192a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

fn seat() -> SeatId {
    SeatId::parse(SEAT).expect("a hand-written seat id parses")
}

fn words(text: &str) -> String {
    text.to_string()
}

// ---- one sample per kind --------------------------------------------------------

fn ordered(seat: Option<SeatId>) -> Body {
    Body::Ordered(Ordered {
        order: OrderKind::Dispatch,
        seat,
    })
}

fn withdrawn() -> Body {
    Body::OrderWithdrawn(OrderWithdrawn {
        why: Withdrawal::SpawnRefused,
        seat: Some(seat()),
        cause: Some(words("the spawner is at its ceiling")),
    })
}

fn delivered(commit: &str) -> Body {
    Body::Delivered(Delivered {
        commit: words(commit),
        branch: words("work/orla-4a5b"),
        base: words(BASE),
        files: vec![words("core/src/entry.rs")],
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
            command: words("fleet item show fleet-1"),
        }],
        decisions: vec![Decision {
            call: words("read the author with Actor::typed"),
            not_taken: words("a new Actor::parse"),
            because: words("typed is the one reader flight 5 landed"),
        }],
        covers: vec![words("fleet-zlk.1")],
    })
}

fn reviewed(verdict: Verdict, commit: &str, findings: Vec<Finding>) -> Body {
    Body::Reviewed(Reviewed {
        verdict,
        commit: words(commit),
        size: Size {
            files: 2,
            added: 550,
            deleted: 1,
            binary: 0,
            tests: true,
            executable: false,
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
        findings,
    })
}

fn returned(commit: &str) -> Body {
    reviewed(
        Verdict::Returned,
        commit,
        vec![Finding {
            text: words("the fold ignores a landing"),
        }],
    )
}

fn held_body(hold: &str, about: Option<About>) -> Held {
    Held {
        hold: words(hold),
        reason: HoldReason::Ask,
        question: words("Which letter licenses the landing?"),
        context: Some(words("two options, both built")),
        options: vec![
            Choice {
                letter: words("A"),
                text: words("land it"),
            },
            Choice {
                letter: words("B"),
                text: words("return it"),
            },
        ],
        branch: Some(words("work/orla-4a5b")),
        commit: Some(words(C1)),
        run_hash: None,
        about,
    }
}

fn held(hold: &str) -> Body {
    Body::Held(held_body(hold, None))
}

fn cleared(hold: &str) -> Body {
    Body::Cleared(Cleared {
        hold: words(hold),
        how: Clearance::Answer,
        letter: Some(words("A")),
        text: Some(words("land it")),
    })
}

fn landed() -> Body {
    Body::Landed(Landed {
        sha: words(C2),
        old: words(BASE),
        squash_of: words(C1),
        run: Some(words("r-7")),
        test: SuiteRun::NotTested(NotTested {
            not_tested: words("no test command was handed"),
        }),
        checks: vec![CheckRow {
            check: words("review"),
            verdict: words("PASS"),
            evidence: words("ACCEPTED at 1111111"),
        }],
        work_branch: WorkBranch {
            branch: Some(words("work/orla-4a5b")),
            classification: Classification::CarriesUnlandedWork,
        },
    })
}

fn one_of_each() -> Vec<Body> {
    vec![
        ordered(Some(seat())),
        withdrawn(),
        delivered(C1),
        returned(C1),
        Body::Held(held_body(
            "h1",
            Some(About {
                items: vec![words("fleet-7")],
                commit: Some(words(C1)),
                licenses: words("A"),
            }),
        )),
        cleared("h1"),
        landed(),
    ]
}

fn entry(id: &str, body: Body) -> Entry {
    Entry {
        id: words(id),
        at: words("2026-09-24T10:00:00Z"),
        by: Actor::seat(seat()),
        body,
    }
}

fn timeline(bodies: Vec<Body>) -> Vec<Entry> {
    bodies
        .into_iter()
        .enumerate()
        .map(|(n, body)| entry(&format!("c{}", n + 1), body))
        .collect()
}

fn object(text: &str) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::from_str(text).expect("the wire text is JSON") {
        serde_json::Value::Object(object) => object,
        other => panic!("the wire text is not an object: {other}"),
    }
}

// ---- 1. the wire text -----------------------------------------------------------

#[test]
fn every_kind_encodes_and_decodes_back_to_the_same_body() {
    let bodies = one_of_each();
    let kinds: Vec<&str> = bodies.iter().map(Body::kind).collect();
    assert_eq!(kinds, KINDS, "one sample per kind, in KINDS' order");
    for body in bodies {
        body.validate().expect("every sample is well-formed");
        let text = encode(&body);
        assert!(!text.contains('\n'), "written compact: {text}");
        let written = object(&text);
        assert_eq!(written.get(KEY), Some(&serde_json::json!(1)), "{text}");
        assert_eq!(VERSION, 1);
        assert_eq!(
            written.get("kind"),
            Some(&serde_json::json!(body.kind())),
            "{text}"
        );
        assert_eq!(decode(&text), Read::Entry(body), "{text}");
    }
}

#[test]
fn an_ordered_dispatch_is_this_text_to_the_byte() {
    assert_eq!(
        encode(&ordered(Some(seat()))),
        format!(r#"{{"fleet.entry":1,"kind":"ordered","order":"dispatch","seat":"{SEAT}"}}"#)
    );
}

#[test]
fn an_ordered_review_reads_as_an_entry() {
    let text = r#"{"fleet.entry":1,"kind":"ordered","order":"review"}"#;
    assert_eq!(
        decode(text),
        Read::Entry(Body::Ordered(Ordered {
            order: OrderKind::Review,
            seat: None,
        }))
    );
}

#[test]
fn a_full_sha_is_forty_lowercase_hex_and_nothing_else() {
    assert!(full_sha(C1));
    assert!(full_sha(BASE));
    assert!(!full_sha("1a2b3c4"));
    assert!(
        !full_sha(&BASE.to_uppercase()),
        "uppercase is not the git form"
    );
    assert!(!full_sha(&format!("{BASE}0")), "41 characters");
    assert!(!full_sha(&BASE.replace('a', "g")), "g is not hex");
    assert!(!full_sha(""));
}

// ---- 2. a person's text ---------------------------------------------------------

#[test]
fn a_persons_words_and_an_object_without_the_key_are_not_entries() {
    assert_eq!(decode("a person's words"), Read::NotAnEntry);
    assert_eq!(decode("{\"kind\":\"delivered\"}"), Read::NotAnEntry);
    assert_eq!(decode("[1, 2]"), Read::NotAnEntry, "JSON, but no object");
}

// ---- 3. an entry that does not read ---------------------------------------------

fn unreadable(text: &str) -> String {
    match decode(text) {
        Read::Unreadable(why) => why,
        other => panic!("{text} decoded to {other:?}"),
    }
}

fn delivered_with(edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> String {
    let mut written = object(&encode(&delivered(C1)));
    edit(&mut written);
    serde_json::Value::Object(written).to_string()
}

#[test]
fn another_version_is_unreadable_and_names_the_version() {
    let text = delivered_with(|written| {
        written.insert(words(KEY), serde_json::json!(2));
    });
    assert_eq!(
        unreadable(&text),
        "carries fleet.entry 2, and this binary reads version 1"
    );
}

#[test]
fn an_unknown_field_is_unreadable_and_names_the_field() {
    let text = delivered_with(|written| {
        written.insert(words("colour"), serde_json::json!("blue"));
    });
    let why = unreadable(&text);
    assert!(why.contains("colour"), "{why}");
}

#[test]
fn a_missing_field_is_unreadable_and_names_the_field() {
    let text = delivered_with(|written| {
        written.remove("not_proven");
    });
    let why = unreadable(&text);
    assert!(why.contains("not_proven"), "{why}");
}

#[test]
fn a_body_that_reads_but_breaks_a_rule_is_unreadable_with_the_rule() {
    let text = delivered_with(|written| {
        written.insert(words("commit"), serde_json::json!("1a2b3c4"));
    });
    assert_eq!(
        unreadable(&text),
        "`commit` is not a full 40-hex sha: 1a2b3c4"
    );
}

// ---- 4. the per-kind rules ------------------------------------------------------

fn refusal(body: Body) -> String {
    body.validate().expect_err("the body breaks a rule")
}

#[test]
fn a_delivery_naming_a_short_commit_is_refused() {
    let Body::Delivered(mut delivery) = delivered(C1) else {
        unreachable!()
    };
    delivery.commit = words("1a2b3c4");
    assert_eq!(
        refusal(Body::Delivered(delivery)),
        "`commit` is not a full 40-hex sha: 1a2b3c4"
    );
}

#[test]
fn a_hold_carrying_a_run_hash_beside_a_branch_is_refused() {
    let mut hold = held_body("h1", None);
    hold.run_hash = Some(words("9f8e7d"));
    let why = refusal(Body::Held(hold));
    assert!(why.contains("`run_hash`"), "{why}");
    assert!(why.contains("`branch`"), "{why}");
}

#[test]
fn a_cancel_carrying_a_letter_is_refused() {
    let why = refusal(Body::Cleared(Cleared {
        hold: words("h1"),
        how: Clearance::Cancel,
        letter: Some(words("A")),
        text: None,
    }));
    assert!(why.contains("`letter`"), "{why}");
}

#[test]
fn a_return_with_no_findings_is_refused() {
    let why = refusal(reviewed(Verdict::Returned, C1, Vec::new()));
    assert!(why.contains("`findings`"), "{why}");
    assert!(
        why.contains("a return carries at least one finding"),
        "{why}"
    );
}

#[test]
fn a_licence_naming_a_letter_no_option_carries_is_refused() {
    let hold = held_body(
        "h1",
        Some(About {
            items: vec![words("fleet-7")],
            commit: None,
            licenses: words("C"),
        }),
    );
    let why = refusal(Body::Held(hold));
    assert!(why.contains("`about.licenses`"), "{why}");
}

// ---- 5. a store's row -----------------------------------------------------------

#[test]
fn a_row_holding_a_persons_words_reads_as_no_entry() {
    assert_eq!(
        read_row(
            "fleet-7",
            "c9",
            "Alberto Vildosola",
            "looks good to me",
            "t"
        ),
        Ok(None)
    );
}

#[test]
fn a_row_whose_author_is_no_actor_is_refused_naming_the_comment() {
    let text = encode(&delivered(C1));
    let why = read_row("fleet-7", "c9", "Alberto Vildosola", &text, "t")
        .expect_err("a bare name is no typed actor");
    assert_eq!(
        why,
        "fleet-7's comment c9 names its author Alberto Vildosola, which is not an actor \
         reference (<kind>:<id>)"
    );
}

#[test]
fn a_row_carrying_a_malformed_entry_is_refused_naming_the_comment() {
    let text = delivered_with(|written| {
        written.insert(words(KEY), serde_json::json!(2));
    });
    let why = read_row("fleet-7", "c9", &format!("seat:{SEAT}"), &text, "t")
        .expect_err("a malformed entry is never skipped");
    assert_eq!(
        why,
        "fleet-7's comment c9 carries fleet.entry and does not read: carries fleet.entry 2, \
         and this binary reads version 1"
    );
}

#[test]
fn a_row_carrying_an_entry_reads_as_that_entry() {
    let text = encode(&delivered(C1));
    let read = read_row("fleet-7", "c9", &format!("seat:{SEAT}"), &text, "at-9")
        .expect("reads")
        .expect("is an entry");
    assert_eq!(
        read,
        Entry {
            id: words("c9"),
            at: words("at-9"),
            by: Actor::seat(seat()),
            body: delivered(C1),
        }
    );
}

// ---- 6. the fold ----------------------------------------------------------------

fn commit_of(found: Option<(&Entry, &Delivered)>) -> Option<String> {
    found.map(|(_, delivery)| delivery.commit.clone())
}

#[test]
fn a_redelivery_after_a_return_stands_and_a_landing_carries_it_away() {
    let entries = timeline(vec![delivered(C1), returned(C1), delivered(C2), landed()]);
    let fold = Timeline(&entries);
    assert_eq!(commit_of(fold.standing_delivery()), Some(words(C2)));
    assert_eq!(commit_of(fold.carried_delivery()), None);
    assert_eq!(commit_of(fold.last_delivery()), Some(words(C2)));
    assert_eq!(
        fold.last_landing().map(|(entry, _)| entry.id.as_str()),
        Some("c4")
    );
    assert_eq!(
        fold.last_review().map(|(entry, _)| entry.id.as_str()),
        Some("c2")
    );
}

#[test]
fn a_returned_delivery_does_not_stand() {
    let entries = timeline(vec![delivered(C1), returned(C1)]);
    let fold = Timeline(&entries);
    assert_eq!(commit_of(fold.standing_delivery()), None);
    assert_eq!(commit_of(fold.carried_delivery()), None);
    assert_eq!(commit_of(fold.last_delivery()), Some(words(C1)));
}

#[test]
fn a_delivery_with_no_landing_after_it_is_carried() {
    let entries = timeline(vec![landed(), delivered(C1)]);
    let fold = Timeline(&entries);
    assert_eq!(commit_of(fold.carried_delivery()), Some(words(C1)));
}

#[test]
fn a_withdrawn_order_is_no_current_order() {
    let entries = timeline(vec![ordered(None), withdrawn()]);
    assert!(Timeline(&entries).current_order().is_none());
}

#[test]
fn the_last_order_is_the_current_one() {
    let entries = timeline(vec![ordered(None), ordered(Some(seat()))]);
    let (entry, order) = Timeline(&entries).current_order().expect("an order stands");
    assert_eq!(entry.id, "c2");
    assert_eq!(order.seat, Some(seat()));
}

#[test]
fn a_cleared_hold_is_not_open_and_the_next_one_is() {
    let entries = timeline(vec![held("h1"), cleared("h1"), held("h2")]);
    let fold = Timeline(&entries);
    let (entry, hold) = fold.open_hold().expect("h2 is open");
    assert_eq!((entry.id.as_str(), hold.hold.as_str()), ("c3", "h2"));
    assert_eq!(
        fold.clearance("h1").map(|(entry, _)| entry.id.as_str()),
        Some("c2")
    );
    assert!(fold.clearance("h2").is_none());
    assert_eq!(
        fold.held("h1").map(|(entry, _)| entry.id.as_str()),
        Some("c1")
    );
    assert_eq!(
        fold.entry("c2").map(|entry| entry.body.kind()),
        Some("cleared")
    );
    assert!(fold.entry("c9").is_none());
}

#[test]
fn every_hold_cleared_leaves_none_open() {
    let entries = timeline(vec![held("h1"), cleared("h1")]);
    assert!(Timeline(&entries).open_hold().is_none());
}

#[test]
fn a_clearance_with_no_hold_before_it_still_answers_by_its_hold() {
    let entries = timeline(vec![cleared("h1"), held("h1")]);
    let fold = Timeline(&entries);
    assert_eq!(
        fold.clearance("h1").map(|(entry, _)| entry.id.as_str()),
        Some("c1")
    );
    assert_eq!(
        fold.open_hold().map(|(entry, _)| entry.id.as_str()),
        Some("c2"),
        "a clearance before the hold does not close it"
    );
}

#[test]
fn a_hold_about_an_item_is_found_by_that_item() {
    let entries = timeline(vec![Body::Held(held_body(
        "h1",
        Some(About {
            items: vec![words("fleet-7")],
            commit: None,
            licenses: words("A"),
        }),
    ))]);
    let fold = Timeline(&entries);
    let (_, hold) = fold.holds_about("fleet-7").expect("h1 is about fleet-7");
    assert_eq!(hold.hold, "h1");
    assert!(fold.holds_about("fleet-8").is_none());
}

// ---- 7. the reader's JSON -------------------------------------------------------

#[test]
fn to_json_carries_the_actor_typed_the_kind_and_no_key() {
    let json = to_json(&entry("c1", delivered(C1)));
    assert_eq!(
        json.get("by"),
        Some(&serde_json::json!({"kind": ActorKind::Seat.as_str(), "id": SEAT}))
    );
    assert_eq!(json.get("kind"), Some(&serde_json::json!("delivered")));
    assert_eq!(json.get("id"), Some(&serde_json::json!("c1")));
    assert_eq!(
        json.get("at"),
        Some(&serde_json::json!("2026-09-24T10:00:00Z"))
    );
    assert_eq!(json.get("commit"), Some(&serde_json::json!(C1)));
    assert!(json.get(KEY).is_none(), "{json}");
}
