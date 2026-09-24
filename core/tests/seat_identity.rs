//! What a seat IS: its id, its kind and its name, the `[seats.<id>]` table
//! that lists it, the machine's own identity file, and the one resolver every
//! seat argument goes through.
//!
//! Nothing here calls a verb. The ids a resolver arm compares are written out
//! by hand, because ids minted in one run share their first eight characters
//! — which is the point the short-id arm proves — and a unique prefix has to
//! be arranged rather than hoped for.

mod common;

use std::collections::HashSet;

use common::Fixture;
use fleet_core::item::{Stop, REFUSED, USAGE};
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::seat::identity::{
    identity_or_mint, read_identity, resolve, roster, roster_in, seat_table, Directory, Kind,
    SeatId, SeatRef, Unresolved, IDENTITY, SEAT_ACTIVE, SEAT_PARKED, SEAT_PARKED_ALIASES,
};

fn id(text: &str) -> SeatId {
    SeatId::parse(text).expect("a hand-written seat id parses")
}

fn seat(text: &str, name: Option<&str>, kind: Kind) -> SeatRef {
    SeatRef {
        id: id(text),
        name: name.map(str::to_string),
        kind,
    }
}

// ---- mint -------------------------------------------------------------------

#[test]
fn a_minted_id_is_a_v7_shown_whole_in_lowercase_and_parses_back() {
    for _ in 0..50 {
        let minted = SeatId::mint();
        let shown = minted.to_string();
        assert_eq!(shown.len(), 36, "{shown}");
        assert_eq!(
            shown.as_bytes()[14],
            b'7',
            "the version nibble says 7: {shown}"
        );
        assert_eq!(shown, shown.to_lowercase(), "shown in lowercase");
        assert_eq!(SeatId::parse(&shown), Ok(minted), "it round-trips");
    }
}

#[test]
fn parse_takes_the_hyphenated_form_in_either_case_and_any_version() {
    let upper = "0192A1B2-C3D4-4E5F-8A9B-0C1D2E3F4A5B";
    let parsed = SeatId::parse(&format!("  {upper}\n")).expect("the input is trimmed");
    assert_eq!(parsed.to_string(), upper.to_lowercase());
}

#[test]
fn parse_refuses_the_simple_form_and_a_name() {
    let simple = SeatId::mint().to_string().replace('-', "");
    assert_eq!(simple.len(), 32);
    for text in [simple.as_str(), "orla"] {
        assert_eq!(
            SeatId::parse(text),
            Err(format!(
                "{text} is not a seat id — a seat id is 36 characters, 8-4-4-4-12 hex"
            ))
        );
    }
}

#[test]
fn a_seat_id_serializes_through_its_string_form() {
    let minted = SeatId::mint();
    let json = serde_json::to_string(&minted).expect("serializes");
    assert_eq!(json, format!("\"{minted}\""));
    let back: SeatId = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, minted);
    assert!(serde_json::from_str::<SeatId>("\"orla\"").is_err());
}

/// Every machine-readable seat is the one object `{id, name?, kind}`: the id
/// whole, the name only where the seat has one, and the kind as its word. The
/// order is asserted on the text, because a document a person reads puts the
/// id first.
#[test]
fn a_seat_serializes_as_its_id_its_name_where_it_has_one_and_its_kind() {
    let a = "11111111-aaaa-7bbb-8ccc-0000deadbeef";
    let named = serde_json::to_string(&seat(a, Some("Orla"), Kind::Agent)).expect("serializes");
    assert_eq!(
        named,
        format!(r#"{{"id":"{a}","name":"Orla","kind":"agent"}}"#)
    );

    let nameless = serde_json::to_value(seat(a, None, Kind::Human)).expect("serializes");
    assert_eq!(nameless, serde_json::json!({ "id": a, "kind": "human" }));
    assert!(
        nameless.get("name").is_none(),
        "a seat with no name carries no key: {nameless}"
    );
}

// ---- short [ASSUMES D1] -----------------------------------------------------

/// The 48-bit millisecond timestamp a v7 id opens with, read off the id itself.
fn millis(id: &SeatId) -> u64 {
    let hex: String = id
        .to_string()
        .chars()
        .filter(|c| *c != '-')
        .take(12)
        .collect();
    u64::from_str_radix(&hex, 16).expect("twelve hex digits")
}

/// THE PREFIX OF A V7 IS A CLOCK. Its first eight hex digits are the top 32
/// bits of the millisecond timestamp, so they change once every 65,536 ms and
/// every seat minted in the same minute shares them; the last eight are
/// random. That is why the short id is the tail.
#[test]
fn the_short_id_is_the_tail_because_the_head_is_a_clock() {
    let ids: Vec<SeatId> = (0..100).map(|_| SeatId::mint()).collect();

    let shorts: HashSet<String> = ids.iter().map(SeatId::short).collect();
    assert_eq!(shorts.len(), 100, "every short id is distinct");
    for minted in &ids {
        let shown = minted.to_string();
        assert_eq!(minted.short(), shown[28..], "the last 8 characters");
    }

    let first = millis(&ids[0]) >> 16;
    let last = millis(&ids[99]) >> 16;
    if first == last {
        let heads: HashSet<String> = ids.iter().map(|i| i.to_string()[..8].to_string()).collect();
        assert_eq!(
            heads.len(),
            1,
            "one 65,536 ms bucket, one prefix for all 100: {heads:?}"
        );
    } else {
        // The loop straddled a bucket edge: the prefixes change exactly there.
        for minted in &ids {
            assert_eq!(
                u64::from_str_radix(&minted.to_string()[..8], 16).unwrap(),
                millis(minted) >> 16
            );
        }
    }
}

// ---- slug and machine_name --------------------------------------------------

#[test]
fn the_slug_is_the_name_reduced_and_the_kind_when_there_is_none() {
    let a = "11111111-aaaa-7bbb-8ccc-0000deadbeef";
    for (name, slug) in [
        (Some("Orla"), "orla"),
        (Some("Zoë Q."), "zo-q"),
        (Some("R2-D2"), "r2-d2"),
        (Some("  "), "agent"),
        (None, "agent"),
    ] {
        let s = seat(a, name, Kind::Agent);
        assert_eq!(s.slug(), slug, "{name:?}");
        assert_eq!(s.machine_name(), format!("{slug}-deadbeef"));
    }
    assert_eq!(seat(a, Some("  "), Kind::Human).slug(), "human");

    let agent = seat(a, None, Kind::Agent);
    assert_eq!(agent.machine_name(), format!("agent-{}", agent.id.short()));
    let human = seat(a, None, Kind::Human);
    assert_eq!(human.machine_name(), format!("human-{}", human.id.short()));
}

#[test]
fn a_kind_is_one_of_two_lowercase_words() {
    assert_eq!(Kind::parse(" agent "), Some(Kind::Agent));
    assert_eq!(Kind::parse("human"), Some(Kind::Human));
    assert_eq!(Kind::parse("Agent"), None);
    assert_eq!(Kind::parse("robot"), None);
    assert_eq!(Kind::Agent.as_str(), "agent");
    assert_eq!(Kind::Human.as_str(), "human");
}

// ---- roster -----------------------------------------------------------------

const FIRST: &str = "01920000-0000-7000-8000-00000000000a";
const SECOND: &str = "01920000-0000-7000-8000-00000000000b";
const THIRD: &str = "01920000-0000-7000-8000-00000000000c";

#[test]
fn a_roster_reads_its_rows_in_id_order() {
    let body = format!(
        "[controller]\npoll_seconds = 5\n\
         \n[seats.{THIRD}]\nkind = \"agent\"\n\
         \n[seats.{FIRST}]\nkind = \"agent\"\nname = \" Orla \"\nmodel = \"a-model\"\nstatus = \"parked\"\n\
         \n[seats.{SECOND}]\nkind = \"human\"\nname = \"Alberto\"\n"
    );
    let seats = roster_in(&body).expect("the roster reads");
    assert_eq!(
        seats.iter().map(|s| s.seat.id).collect::<Vec<_>>(),
        [id(FIRST), id(SECOND), id(THIRD)],
        "in id order, whatever order the file wrote them in"
    );

    assert_eq!(seats[0].seat, seat(FIRST, Some("Orla"), Kind::Agent));
    assert_eq!(seats[0].model.as_deref(), Some("a-model"));
    assert!(seats[0].parked);

    assert_eq!(seats[1].seat, seat(SECOND, Some("Alberto"), Kind::Human));
    assert!(seats[1].model.is_none());
    assert!(!seats[1].parked);

    assert_eq!(seats[2].seat, seat(THIRD, None, Kind::Agent));
    assert!(seats[2].model.is_none());
    assert!(!seats[2].parked, "a row that says nothing is active");

    let table: toml::Table = toml::from_str(&body).unwrap();
    assert_eq!(roster(&table), Ok(seats), "the two readers are one reading");
}

#[test]
fn a_blank_name_and_a_blank_model_are_no_value() {
    let seats = roster_in(&format!(
        "[seats.{FIRST}]\nkind = \"agent\"\nname = \"  \"\nmodel = \"\"\n"
    ))
    .unwrap();
    assert!(seats[0].seat.name.is_none());
    assert!(seats[0].model.is_none());
}

#[test]
fn a_roster_refuses_each_retired_or_wrong_shape_with_its_reason() {
    for (body, refusal) in [
        (
            "[seats.orla]\nkind = \"agent\"\n".to_string(),
            "[seats.orla] is keyed by a name — a seat is keyed by its id now; \
             fleet seat add --agent --name orla mints one"
                .to_string(),
        ),
        (
            format!("[seats.{FIRST}]\nname = \"Orla\"\n"),
            format!("[seats.{FIRST}] carries no kind — say kind = \"agent\" or kind = \"human\""),
        ),
        (
            format!("[seats.{FIRST}]\nkind = \"robot\"\n"),
            format!("[seats.{FIRST}] kind = \"robot\" is neither agent nor human"),
        ),
        (
            format!("[seats.{FIRST}]\nkind = \"agent\"\nchosen_name = \"Kite\"\n"),
            format!("[seats.{FIRST}] carries chosen_name, which is name now"),
        ),
        (
            format!("[seats.{FIRST}]\nkind = \"human\"\nmodel = \"a-model\"\n"),
            format!(
                "[seats.{FIRST}] is a human seat and carries model — only an agent seat runs on a model"
            ),
        ),
        (
            format!("[seats.{FIRST}]\nkind = \"human\"\nstatus = \"parked\"\n"),
            format!(
                "[seats.{FIRST}] is a human seat and carries status — only an agent seat is parked"
            ),
        ),
        (
            format!("[seats.{FIRST}]\nkind = \"agent\"\nstatus = \"park\"\n"),
            format!(
                "[seats.{FIRST}] status = \"park\" is none of `active`, `parked`, \
                 `vacationing`, `chartered` — a status this reader does not know is refused \
                 rather than read as active, because reading it as active starts a seat \
                 somebody meant to keep still"
            ),
        ),
        ("seats = 3\n".to_string(), "seats is not a table".to_string()),
    ] {
        assert_eq!(roster_in(&body), Err(refusal), "{body}");
    }
}

#[test]
fn a_key_the_reader_does_not_know_is_ignored() {
    let seats = roster_in(&format!(
        "[seats.{FIRST}]\nkind = \"agent\"\nclass = \"architect\"\n"
    ))
    .expect("an unknown key is not a refusal");
    assert_eq!(seats.len(), 1);
    assert!(seats[0].model.is_none());
}

#[test]
fn a_file_with_no_seats_is_an_empty_roster() {
    assert_eq!(roster_in("[controller]\npoll_seconds = 5\n"), Ok(vec![]));
    assert_eq!(roster_in(""), Ok(vec![]));
    assert!(
        roster_in("[seats\n").is_err(),
        "a file that does not parse is an error"
    );
}

fn with_status(said: &str) -> Result<bool, String> {
    roster_in(&format!(
        "[seats.{FIRST}]\nkind = \"agent\"\nstatus = \"{said}\"\n"
    ))
    .map(|seats| seats[0].parked)
}

/// THE STATUS IS READ FROM THE ADMITTED WORDS AND NO OTHERS. A fifth value
/// is refused rather than rounded to active: rounding starts the seat
/// somebody wrote it to keep still, which is the one mistake this key
/// exists to prevent.
#[test]
fn a_status_that_is_neither_word_is_refused_and_never_read_as_active() {
    for said in ["park", "parked.", "inactive", "off", "Active!"] {
        let refused = with_status(said).expect_err("an unrecognised status is refused");
        assert!(refused.contains(said), "{refused}");
        assert!(refused.contains(&format!("[seats.{FIRST}]")), "{refused}");
        assert!(
            refused.contains(SEAT_ACTIVE)
                && refused.contains(SEAT_PARKED)
                && SEAT_PARKED_ALIASES.iter().all(|a| refused.contains(a)),
            "the refusal names all four it knows: {refused}"
        );
    }

    // The control: the readings it DOES take, so the refusals above are
    // about an unadmitted word and not about a reader that refuses every
    // status.
    assert_eq!(with_status("active"), Ok(false));
    assert_eq!(with_status("ACTIVE"), Ok(false));
    assert_eq!(with_status("parked"), Ok(true));
    assert_eq!(with_status("PARKED"), Ok(true));
    assert_eq!(
        with_status("  "),
        Ok(false),
        "a blank was not an answer either, so it is the absent reading"
    );
}

/// `vacationing` and `chartered` are the harness's words for a seat it keeps
/// and does not run, which is this reader's `parked`.
#[test]
fn a_vacationing_or_chartered_seat_is_parked() {
    for said in ["vacationing", "Vacationing", "chartered", "CHARTERED"] {
        assert_eq!(with_status(said), Ok(true), "{said}");
    }
}

// ---- seat_table -------------------------------------------------------------

#[test]
fn a_seat_table_is_the_text_one_appended_row_takes_and_reads_back() {
    let bare = seat(FIRST, None, Kind::Human);
    assert_eq!(
        seat_table(&bare, None),
        format!("\n[seats.{FIRST}]\nkind = \"human\"\n")
    );

    let named = seat(FIRST, Some("Or\"la\\"), Kind::Agent);
    let text = seat_table(&named, Some("a-model"));
    assert_eq!(
        text,
        format!(
            "\n[seats.{FIRST}]\nkind = \"agent\"\nname = \"Or\\\"la\\\\\"\nmodel = \"a-model\"\n"
        )
    );
    let back = roster_in(&text).expect("the appended table reads back");
    assert_eq!(back[0].seat, named);
    assert_eq!(back[0].model.as_deref(), Some("a-model"));
}

// ---- resolve ----------------------------------------------------------------

const ORLA: &str = "11111111-aaaa-7bbb-8ccc-0000deadbeef";
const KITE: &str = "22222222-aaaa-7bbb-8ccc-0000cafef00d";
const NAMELESS: &str = "33333333-aaaa-7bbb-8ccc-000012345678";

fn three() -> Vec<SeatRef> {
    vec![
        seat(ORLA, Some("Orla"), Kind::Agent),
        seat(KITE, Some("Kite"), Kind::Agent),
        seat(NAMELESS, None, Kind::Human),
    ]
}

#[test]
fn a_seat_is_found_by_its_id_a_prefix_its_short_its_name_or_its_machine_name() {
    let seats = three();
    for (arg, index) in [
        (ORLA, 0),
        ("  11111111-AAAA-7BBB-8CCC-0000DEADBEEF ", 0),
        ("22222222", 1),
        ("33333333-aaaa", 2),
        ("cafef00d", 1),
        ("0000deadbeef", 0),
        ("12345678", 2),
        ("ORLA", 0),
        ("kite", 1),
        ("orla-deadbeef", 0),
        ("human-12345678", 2),
    ] {
        assert_eq!(resolve(&seats, arg), Ok(index), "{arg}");
    }
}

/// A MACHINE NAME MATCHES ON ITS ID PART, so a seat renamed after its home was
/// named is still found by the old machine name.
#[test]
fn a_machine_name_is_rename_stable() {
    assert_eq!(resolve(&three(), "wren-deadbeef"), Ok(0));
}

#[test]
fn a_seven_character_fragment_names_no_seat() {
    assert!(matches!(
        resolve(&three(), "1111111"),
        Err(Unresolved::Missing { .. })
    ));
}

/// A full id is exact: an id nobody holds is missing even where its prefix
/// matches a seat that is there.
#[test]
fn a_full_id_nobody_holds_is_missing_and_runs_no_fragment_logic() {
    assert!(matches!(
        resolve(&three(), "11111111-aaaa-7bbb-8ccc-0000deadbeee"),
        Err(Unresolved::Missing { .. })
    ));
}

#[test]
fn two_seats_with_one_name_are_ambiguous_and_both_are_listed() {
    let other = "44444444-aaaa-7bbb-8ccc-0000feedface";
    let mut seats = three();
    seats.push(seat(other, Some("orla"), Kind::Agent));
    let refused = resolve(&seats, "Orla").expect_err("two seats answer");
    assert_eq!(refused.code(), 1);
    assert_eq!(
        refused,
        Unresolved::Ambiguous {
            arg: "Orla".to_string(),
            candidates: vec![
                format!("orla-deadbeef ({ORLA})"),
                format!("orla-feedface ({other})"),
            ],
        }
    );
    assert_eq!(
        refused.to_string(),
        format!("Orla names 2 seats — orla-deadbeef ({ORLA}), orla-feedface ({other}) — say more of the id")
    );
    let stop = Stop::from(refused);
    assert_eq!(stop.code, REFUSED);
}

/// One seat reached by two rules is one candidate, not two.
#[test]
fn a_seat_matched_twice_is_one_candidate() {
    let seats = vec![seat(ORLA, Some("deadbeef"), Kind::Agent)];
    assert_eq!(resolve(&seats, "deadbeef"), Ok(0));
}

#[test]
fn an_empty_argument_is_a_usage_error() {
    for arg in ["", "   "] {
        let refused = resolve(&three(), arg).expect_err("blank");
        assert_eq!(refused, Unresolved::Blank);
        assert_eq!(refused.code(), 2);
        assert_eq!(refused.to_string(), "names no seat — the argument is empty");
        assert_eq!(Stop::from(refused).code, USAGE);
    }
}

#[test]
fn a_missing_seat_lists_every_seat_the_fleet_knows() {
    let refused = resolve(&three(), "nobody").expect_err("nobody is there");
    assert_eq!(refused.code(), 1);
    assert_eq!(
        refused.to_string(),
        format!(
            "nobody names no seat — the seats are orla-deadbeef ({ORLA}), \
             kite-cafef00d ({KITE}), human-12345678 ({NAMELESS})"
        )
    );
    let stop = Stop::from(refused.clone());
    assert_eq!((stop.code, stop.message), (REFUSED, refused.to_string()));

    assert_eq!(
        resolve(&[], "nobody").expect_err("no seats").to_string(),
        "nobody names no seat — the seats are none"
    );
}

// ---- the directory ------------------------------------------------------------

/// Orla and Kite run here; the nameless human is listed and runs nowhere; a
/// transient seat runs and is listed through its row.
fn directory() -> Directory {
    let transient = seat("55555555-aaaa-7bbb-8ccc-0000abcdef01", None, Kind::Agent);
    Directory {
        listed: vec![
            seat(ORLA, Some("Orla"), Kind::Agent),
            seat(KITE, Some("Kite"), Kind::Agent),
            seat(NAMELESS, None, Kind::Human),
            transient.clone(),
        ],
        running: vec![
            seat(ORLA, Some("Orla"), Kind::Agent),
            seat(KITE, Some("Kite"), Kind::Agent),
            transient,
        ],
    }
}

/// WORK GOES ONLY TO A RUNNING SEAT: a person is listed and never run, so a
/// running resolve of one is missing, naming the seats that do run.
#[test]
fn a_running_resolve_finds_an_agent_and_misses_a_person() {
    let dir = directory();
    assert_eq!(dir.resolve_running("orla").map(|s| s.id), Ok(id(ORLA)));
    let refused = dir
        .resolve_running("human-12345678")
        .expect_err("a person runs nowhere");
    assert!(matches!(refused, Unresolved::Missing { .. }), "{refused:?}");
    assert!(
        refused.to_string().contains("kite-cafef00d") && !refused.to_string().contains(NAMELESS),
        "{refused}"
    );
    assert_eq!(
        dir.resolve_listed("human-12345678").map(|s| s.id),
        Ok(id(NAMELESS))
    );
}

#[test]
fn a_label_is_the_machine_name_of_a_known_seat_and_else_the_id() {
    let dir = directory();
    assert_eq!(dir.label(&id(ORLA)), "orla-deadbeef");
    assert_eq!(dir.label(&id(NAMELESS)), "human-12345678");
    let stranger = id("66666666-aaaa-7bbb-8ccc-0000000000aa");
    assert_eq!(dir.label(&stranger), stranger.to_string());
}

/// THE SEAT AN ACTOR IS, BY KIND, with no directory asked: `seat:<full id>` is
/// that seat whether or not any fleet lists it — the machine's identity minted
/// by this very call is on no roster yet — a seat's id is read in either case,
/// and every other kind is no seat at all, whatever its id looks like.
#[test]
fn a_typed_seat_actor_is_its_id_and_any_other_kind_is_no_seat() {
    let seat_of = |text: &str| {
        Actor::typed(text)
            .and_then(Result::ok)
            .and_then(|actor| actor.seat_id())
    };
    assert_eq!(seat_of(&format!("seat:{ORLA}")), Some(id(ORLA)));
    assert_eq!(
        seat_of(&format!("seat:{}", ORLA.to_uppercase())),
        Some(id(ORLA))
    );
    let stranger = "66666666-aaaa-7bbb-8ccc-0000000000aa";
    assert_eq!(seat_of(&format!("seat:{stranger}")), Some(id(stranger)));
    assert_eq!(seat_of("seat:orla"), None, "a bad id is no seat");
    assert_eq!(seat_of("run:fleet-abc"), None);
    assert_eq!(seat_of("routine:nightly"), None);
    assert_eq!(seat_of(&format!("controller:{ORLA}")), None);
    assert_eq!(seat_of("Orla"), None, "a bare name is no typed actor");
}

// ---- identity ---------------------------------------------------------------

#[test]
fn a_machine_with_no_identity_file_has_no_identity() {
    let dir = Fixture::new("identity-none");
    assert_eq!(read_identity(&dir.root), Ok(None));
}

#[test]
fn the_first_call_mints_the_identity_and_the_second_reads_it() {
    let dir = Fixture::new("identity-mint");
    let machine = dir.path("machine");
    let (minted, fresh) = identity_or_mint(&machine).expect("mints");
    assert!(fresh, "minted by this call");
    assert!(minted.name.is_none());
    assert_eq!(
        std::fs::read_to_string(machine.join(IDENTITY)).unwrap(),
        format!(
            "# This machine's identity: who acts when a fleet verb runs here with no\n\
             # --by and no FLEET_ACTOR. The id is the key; add name = \"...\" if you\n\
             # want one. Written by fleet.\n\
             id = \"{}\"\n\
             kind = \"human\"\n",
            minted.id
        )
    );
    let names: Vec<_> = std::fs::read_dir(&machine)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, [IDENTITY], "no temporary file is left behind");

    let (again, fresh) = identity_or_mint(&machine).expect("reads");
    assert!(!fresh, "read, not minted");
    assert_eq!(again, minted);
    assert_eq!(
        again.as_ref(),
        SeatRef {
            id: minted.id,
            name: None,
            kind: Kind::Human
        }
    );
}

#[test]
fn eight_threads_minting_at_once_all_answer_one_id() {
    let dir = Fixture::new("identity-race");
    let machine = dir.path("machine");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let machine = machine.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                identity_or_mint(&machine).expect("every thread answers")
            })
        })
        .collect();
    let answers: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let ids: HashSet<SeatId> = answers.iter().map(|(m, _)| m.id).collect();
    assert_eq!(ids.len(), 1, "one id: {ids:?}");
    assert_eq!(
        answers.iter().filter(|(_, fresh)| *fresh).count(),
        1,
        "exactly one thread minted it"
    );
}

#[test]
fn an_identity_file_that_is_not_a_human_is_refused_naming_its_path() {
    let dir = Fixture::new("identity-agent");
    let minted = SeatId::mint();
    dir.file(IDENTITY, &format!("id = \"{minted}\"\nkind = \"agent\"\n"));
    let refused = read_identity(&dir.root).expect_err("an agent is not a machine's identity");
    assert!(
        refused.starts_with(&format!("{}: ", dir.path(IDENTITY).display())),
        "{refused}"
    );
    assert!(
        identity_or_mint(&dir.root).is_err(),
        "and is never re-minted"
    );
}

#[test]
fn an_identity_file_with_a_name_reads_it_and_ignores_other_keys() {
    let dir = Fixture::new("identity-named");
    let minted = SeatId::mint();
    dir.file(
        IDENTITY,
        &format!("id = \"{minted}\"\nkind = \"human\"\nname = \" Alberto \"\nshell = \"zsh\"\n"),
    );
    let read = read_identity(&dir.root).unwrap().expect("present");
    assert_eq!(read.id, minted);
    assert_eq!(read.name.as_deref(), Some("Alberto"));
}

#[test]
fn an_identity_file_with_a_bad_or_missing_id_is_refused_naming_its_path() {
    for body in [
        "kind = \"human\"\n",
        "id = \"orla\"\nkind = \"human\"\n",
        "id = \n",
    ] {
        let dir = Fixture::new("identity-bad");
        dir.file(IDENTITY, body);
        let refused = read_identity(&dir.root).expect_err(body);
        assert!(
            refused.starts_with(&format!("{}: ", dir.path(IDENTITY).display())),
            "{refused}"
        );
    }
}

// ---- actor ------------------------------------------------------------------

#[test]
fn a_typed_actor_is_kind_colon_id_and_shows_the_same_way() {
    let seat_id = id(ORLA);
    let upper = format!("seat:{}", ORLA.to_uppercase());
    let actor = Actor::typed(&upper).expect("typed").expect("good");
    assert_eq!(actor, Actor::seat(seat_id));
    assert_eq!(
        actor.to_string(),
        format!("seat:{ORLA}"),
        "stored lowercase"
    );
    assert_eq!(actor.seat_id(), Some(seat_id));

    for (text, kind) in [
        ("run:fleet-abc", ActorKind::Run),
        ("routine:nightly", ActorKind::Routine),
        ("controller:0192", ActorKind::Controller),
    ] {
        let actor = Actor::typed(text).expect("typed").expect("good");
        assert_eq!(actor.kind, kind);
        assert_eq!(actor.to_string(), text);
        assert_eq!(actor.seat_id(), None, "only a seat actor is a seat");
    }

    assert_eq!(ActorKind::Seat.as_str(), "seat");
    assert_eq!(ActorKind::Run.as_str(), "run");
    assert_eq!(ActorKind::Routine.as_str(), "routine");
    assert_eq!(ActorKind::Controller.as_str(), "controller");
}

#[test]
fn text_that_is_no_typed_actor_is_none_and_a_bad_id_is_refused() {
    for text in ["orla", "person:x", "Seat:x", ""] {
        assert!(Actor::typed(text).is_none(), "{text}");
    }
    for text in [
        "seat:not-a-uuid",
        "run:",
        "routine:two words",
        "controller:",
    ] {
        let refused = Actor::typed(text).expect("typed").expect_err(text);
        assert!(
            refused.starts_with(&format!("`{text}` is a typed actor with a bad id — ")),
            "{refused}"
        );
    }
}

#[test]
fn an_actor_serializes_through_its_string_form() {
    let actor = Actor::typed("run:fleet-abc").unwrap().unwrap();
    let json = serde_json::to_string(&actor).unwrap();
    assert_eq!(json, "\"run:fleet-abc\"");
    assert_eq!(serde_json::from_str::<Actor>(&json).unwrap(), actor);
    assert!(serde_json::from_str::<Actor>("\"orla\"").is_err());
    assert!(serde_json::from_str::<Actor>("\"seat:orla\"").is_err());
}
