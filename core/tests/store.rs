//! `Store` as the `bd` binary answers it — the argv a read is actually made
//! with.
//!
//! The calls whose argv is load-bearing on its own are the LIST reads: each
//! verb caps its answer unless the cap is lifted, and a truncated list is
//! indistinguishable from a whole one, so nothing downstream can notice the
//! loss. The proof has to be the argv; the count is what cannot be measured.
//! One arm per such read, and an arm is what says the argument is still there.
//!
//! The row-cap arms open the store with `Bd::at`, which runs `store::bd::BD`, a
//! bare name resolved on the process's own `PATH`, so a directory prepended to
//! `PATH` is the seam their shim enters through — and `PATH` is process-wide.
//! The other arms name their stub by absolute path through `Bd::at_bin`, which
//! no `PATH` reaches. Every arm here takes `PATH_LOCK` all the same, the arms
//! wanting the real binary included: an arm reading a real store while another
//! has the shim in front of it would be reading the shim.
//!
//! The ENVELOPE arms answer in the shape bd gives with `BD_JSON_ENVELOPE=1`,
//! the shape v2.0 makes the default, copied from bd 1.3.0's own answers: each
//! read decodes through it, and the shim records that every call asked for it.
//!
//! The DECODE arms hand `item_from` a row in the shape bd answers and run no
//! binary, so they take no lock.
//!
//! The TIMELINE arms at the end are the board held in memory's own: what its
//! comments answer, planted as bd's would be, and the append-and-read-back
//! helper over a store that drops its writes. They run no binary either.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use common::capped::{calls, capped_bd, Held};
use common::{a_delivery, seat_actor, Fixture, A_COMMIT};
use fleet_core::entry::{Body, Ordered};
use fleet_core::item::{recorded, Stop, Unrecorded, COULD_NOT_TELL, REFUSED};
use fleet_core::process::DRAIN_GRACE;
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use fleet_core::store::bd::{item_from, Bd};
use fleet_core::store::{
    self, Filter, HoldId, ItemId, ItemSummary, NewItem, Order, OrderKind, OrderState, RunRecord,
    Stamp, Store, StoreError, Update,
};
use fleet_core::test_support::the_test;

/// Serialises every arm in this binary, because the seam they share is the
/// process's `PATH` and there is one of those.
static PATH_LOCK: Mutex<()> = Mutex::new(());

/// Set from BEFORE the shim goes in front of the process's `PATH` until AFTER
/// it is put back, so it is never false while the shim is ahead. It errs the
/// safe way on purpose: a waiting arm may refuse a moment early, and can never
/// run because the flag had not caught up yet.
static SHIM_AHEAD: AtomicBool = AtomicBool::new(false);

/// Takes `PATH_LOCK`, reading a poisoned lock as the situation rather than as a
/// lock error.
///
/// A sibling that panicked with `PATH` already put back left the seam clean, so
/// this arm takes the lock and runs — its own red is the only one the binary
/// reports. A sibling that panicked with the shim still in front did not, and
/// this arm refuses rather than read whatever `bd` that leaves on `PATH`.
fn path_lock() -> MutexGuard<'static, ()> {
    match PATH_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            assert!(
                !SHIM_AHEAD.load(Ordering::SeqCst),
                "a sibling arm panicked with the shim still in front of PATH — this arm did not run"
            );
            poisoned.into_inner()
        }
    }
}

/// A `bd` on `PATH` that records the argv it was handed and answers an empty
/// list. Answers the same way whatever the verb: an arm asserting on the argv
/// wants the call made, not the answer.
fn shim(dir: &Fixture, log: &Path) {
    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{log}'; done\n\
             printf '[]\\n'\n",
            log = log.display(),
        ),
    )
    .expect("the shim is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the shim is executable");
}

/// Runs `body` with `dir` in front of the process's `PATH`, and puts `PATH`
/// back before returning — including when `body` answers a failure, so a red
/// arm never leaves the shim in front of a later one. A panic inside `body`
/// unwinds past that restore, so `SHIM_AHEAD` stays set and the next arm to
/// take `PATH_LOCK` refuses.
fn with_path_ahead<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
    let ahead = match std::env::var_os("PATH") {
        Some(rest) => format!("{}:{}", dir.display(), rest.to_string_lossy()),
        None => dir.display().to_string(),
    };
    with_path(&ahead, body)
}

/// The `PATH` a launchd agent is started with, which is the whole of this
/// box's search path for the controller: no package-manager prefix, where
/// `bd` actually is, and no user local bin either.
const SERVICE_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// Runs `body` with the process's `PATH` set to `path`, and puts `PATH` back
/// before returning — the same discipline and the same flag as
/// [`with_path_ahead`], because a `PATH` that is missing entries this process
/// started with is as unusable to a sibling arm as one with a shim in front.
fn with_path<T>(path: &str, body: impl FnOnce() -> T) -> T {
    let original = std::env::var_os("PATH");
    SHIM_AHEAD.store(true, Ordering::SeqCst);
    std::env::set_var("PATH", path);
    let answer = body();
    match original {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    SHIM_AHEAD.store(false, Ordering::SeqCst);
    answer
}

/// A store built on an ABSOLUTE binary runs it when the process's own `PATH`
/// is the service's and holds no `bd` at all — which is the controller's
/// situation on every tick.
///
/// THE CONTROL AND THE PROOF ARE ON ONE `PATH`: the same shim, in the same
/// directory, is unreachable by bare name and reachable by absolute path, so
/// the arm cannot go green because the shim happened to be findable.
#[test]
fn an_absolute_binary_runs_where_a_bare_name_cannot() {
    let _guard = path_lock();
    let dir = Fixture::new("store-absolute-bin");
    let log = dir.path("argv");
    shim(&dir, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let (by_name, by_path) = with_path(SERVICE_PATH, || {
        (
            Bd::at(&root).list(&Filter::Ready),
            Bd::at_bin(&root, &dir.path("bd")).list(&Filter::Ready),
        )
    });

    let refusal = by_name.expect_err("a bare `bd` is not on the service PATH");
    assert!(
        format!("{refusal:?}").contains("could not be run"),
        "the bare name must fail because nothing could be run, not for some \
         other reason — {refusal:?}"
    );
    assert_eq!(
        by_path.expect("the shim answers a list"),
        Vec::<ItemSummary>::new()
    );

    let argv: Vec<String> = std::fs::read_to_string(&log)
        .expect("the shim recorded its argv")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv.len(),
        6,
        "exactly one of the two reads reached the shim, as `ready`'s six \
         arguments — {argv:?}"
    );
}

/// The seat the row-cap arms list, by its full id: what an assignment writes
/// and what a seat's listing is asked by.
const SEAT: &str = "018f6a2c-1d3e-7a4b-9c5d-00000c3a5e71";

/// One listing, asked of a `bd` on `PATH` that holds `rows` rows and caps
/// them as the real one does: the ids the fake holds, the ids the listing
/// answered, the calls the fake was handed and the project root it ran
/// under.
fn listed_past_the_cap(
    label: &str,
    rows: usize,
    filter: &Filter,
) -> (Vec<String>, Vec<String>, Vec<Vec<String>>, PathBuf) {
    let _guard = path_lock();
    let dir = Fixture::new(label);
    let log = dir.path("argv");
    let ids: Vec<String> = (1..=rows).map(|n| format!("fx-row-{n:03}")).collect();
    let held: Vec<Held> = ids.iter().map(|id| Held { id, ordered: false }).collect();
    capped_bd(&dir, SEAT, &held, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let answered = with_path_ahead(&dir.root, || Bd::at(&root).list(filter));

    let listed: Vec<String> = answered
        .expect("the fake answers a list")
        .into_iter()
        .map(|row| row.id.to_string())
        .collect();
    (ids, listed, calls(&log), root)
}

/// The ready listing lifts the row cap, and answers the row past it: 101
/// ready rows, the 101st the one a capped read drops — `bd ready` answers its
/// first 100 by default, piped or not, and past them a ready item would be
/// refused as not ready.
#[test]
fn ready_lifts_the_row_cap() {
    let (ids, listed, calls, root) = listed_past_the_cap("store-argv", 101, &Filter::Ready);

    assert_eq!(listed, ids, "every ready row, the 101st included");
    assert_eq!(
        calls,
        vec![vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("ready"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ]],
        "the ready listing must carry `-n 0`, or it answers the first 100 rows only"
    );
}

/// A label's listing lifts the row cap, and answers the row past it: 51 open
/// rows under the run label, the 51st the one a capped read drops — and past
/// the cap a run would be started past the `[core.run] max_open` it is
/// measured against.
#[test]
fn a_label_listing_lifts_the_row_cap() {
    let (ids, listed, calls, root) = listed_past_the_cap(
        "store-argv-label",
        51,
        &Filter::Label(String::from("fleet:run")),
    );

    assert_eq!(
        listed, ids,
        "every open row under the label, the 51st included"
    );
    assert_eq!(
        calls,
        vec![vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("list"),
            String::from("--label"),
            String::from("fleet:run"),
            String::from("--status"),
            String::from("open"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ]],
        "the label's listing must carry `-n 0`, or it answers the first 50 rows only"
    );
}

/// The open-hold read lifts the row cap.
///
/// The same argument the ready read carries, against a different verb and for a
/// sharper consequence: `gate list` answers its first 50 rows with no `-n`, and
/// `clear` refuses a hold it cannot find on the open list as one somebody has
/// already cleared — so a truncated listing refuses a live question instead of
/// answering it. The argv is the proof here for the reason it is above: a board
/// big enough to drop a row costs about fifty times this arm to build, and what
/// went wrong was the argument.
#[test]
fn open_holds_lifts_the_row_cap() {
    let _guard = path_lock();
    let dir = Fixture::new("store-argv-holds");
    let log = dir.path("argv");
    shim(&dir, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let answered = with_path_ahead(&dir.root, || Bd::at(&root).holds_open());

    assert_eq!(
        answered.expect("the shim answers a list"),
        Vec::<HoldId>::new()
    );
    let argv: Vec<String> = std::fs::read_to_string(&log)
        .expect("the shim recorded its argv")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv,
        vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("gate"),
            String::from("list"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ],
        "the open-hold read must carry `-n 0`, or it answers the first 50 rows only"
    );
}

/// A seat's listing lifts the row cap, and answers the row past it: 51
/// non-closed rows against one seat, the 51st the one a capped read drops. A
/// retire takes a seat's ordered items off this listing before its name goes
/// back on the pile, so a row it cannot see is an order the next seat of that
/// name inherits. The seat is asked by its full id.
#[test]
fn a_seat_listing_lifts_the_row_cap() {
    let seat = SeatId::parse(SEAT).expect("the seat's id parses");
    let (ids, listed, calls, root) =
        listed_past_the_cap("store-argv-seat", 51, &Filter::Assignee(seat));

    assert_eq!(listed, ids, "every row the seat holds, the 51st included");
    assert_eq!(
        calls,
        vec![vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("list"),
            String::from("-a"),
            String::from(SEAT),
            String::from("--all"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ]],
        "the seat's listing must carry `-n 0`, or it answers the first 50 rows only"
    );
}

/// A `bd` that answers each verb from `answers/<key>.json` under `dir` — the
/// key is `show-<id>`, `gate-<sub>` or the verb itself — and records, one line
/// per call, what `BD_JSON_ENVELOPE` it was handed. Where `<key>.err` is
/// there too, it goes to stderr and the call exits 1, as a JSON error does.
fn envelope_bd(dir: &Fixture, log: &Path) -> PathBuf {
    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"${{BD_JSON_ENVELOPE-unset}}\" >> '{log}'\n\
             shift 2\n\
             case \"$1\" in\n\
             show|gate) key=\"$1-$2\" ;;\n\
             *) key=\"$1\" ;;\n\
             esac\n\
             answers='{answers}'\n\
             cat \"$answers/$key.json\" || exit 1\n\
             [ -e \"$answers/$key.err\" ] || exit 0\n\
             cat \"$answers/$key.err\" >&2\n\
             exit 1\n",
            log = log.display(),
            answers = dir.path("answers").display(),
        ),
    )
    .expect("the shim is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the shim is executable");
    bin
}

/// What the shim was handed for the envelope, one entry per call.
fn envelopes(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .expect("the shim recorded its calls")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Every read decodes an answer in envelope form, and every call asks for it.
///
/// The show answer carries a line after the envelope, which the FIRST-value
/// fence reads past as it read past the bare array's newline.
#[test]
fn every_read_opens_the_envelope() {
    let _guard = path_lock();
    let dir = Fixture::new("store-envelope");
    let log = dir.path("envelope");
    let bin = envelope_bd(&dir, &log);
    dir.file(
        "answers/ready.json",
        r#"{"data": [{"id": "fx-ready", "created_at": "2026-09-23T23:30:39Z", "labels": ["a"]}], "schema_version": 1}"#,
    )
    .file(
        "answers/show-fx-held.json",
        "{\"data\": [{\"id\": \"fx-held\", \"title\": \"a held item\", \"status\": \"open\", \
         \"assignee\": \"01a0d1f1-0aec-765f-9abe-000000a5ea70\", \"metadata\": {\"fleet.orders\": {\"v\": 1, \"by\": \"run:an-architect\", \
         \"kind\": \"dispatch\", \"at\": \"2026-09-23T23:30:39Z\"}}}], \
         \"schema_version\": 1}\nTip: a line after the answer\n",
    )
    .file(
        "answers/list.json",
        r#"{"data": [{"id": "fx-listed", "title": "a listed item", "status": "open", "issue_type": "task", "labels": ["a"], "metadata": {"fleet.orders": {"v": 1, "by": "run:an-architect", "kind": "dispatch", "at": "2026-09-23T23:30:39Z"}}}], "schema_version": 1}"#,
    )
    .file(
        "answers/gate-list.json",
        r#"{"data": [{"id": "fx-gate", "status": "open"}], "schema_version": 1}"#,
    )
    .file(
        "answers/create.json",
        r#"{"data": {"id": "fx-new", "status": "open"}, "schema_version": 1}"#,
    )
    .file(
        "answers/gate-create.json",
        r#"{"data": {"id": "fx-raised", "issue_type": "gate"}, "schema_version": 1}"#,
    );
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);

    let ready = store
        .list(&Filter::Ready)
        .expect("the ready answer decodes");
    assert_eq!(ready.len(), 1, "one ready row: {ready:?}");
    assert_eq!(ready[0].id, "fx-ready");
    assert_eq!(ready[0].labels, ["a"], "the row's own labels are read");

    let held = store.show("fx-held").expect("the show answer decodes");
    assert_eq!(held.id, "fx-held");
    assert_eq!(held.title, "a held item");
    assert_eq!(
        held.assignee.map(|seat| seat.to_string()).as_deref(),
        Some("01a0d1f1-0aec-765f-9abe-000000a5ea70")
    );
    assert!(
        matches!(&held.order, OrderState::Ordered(order) if order.by.to_string() == "run:an-architect"),
        "{:?}",
        held.order
    );

    let labelled = store
        .list(&Filter::Label(String::from("a")))
        .expect("the list answer decodes");
    assert_eq!(labelled.len(), 1, "{labelled:?}");
    assert_eq!(labelled[0].id, "fx-listed");
    let seat = SeatId::parse(SEAT).expect("the seat's id parses");
    let rows = store
        .list(&Filter::Assignee(seat))
        .expect("the seat listing decodes");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, "fx-listed");
    assert_eq!(
        rows[0].title, "a listed item",
        "the row's own title is read"
    );
    assert_eq!(rows[0].status, "open", "the row's own status is read");
    assert_eq!(
        rows[0].item_type, "task",
        "the row's type is read off issue_type"
    );
    assert_eq!(rows[0].labels, ["a"], "the row's own labels are read");
    assert!(
        matches!(rows[0].order, OrderState::Ordered(_)),
        "the row's own metadata is read: {:?}",
        rows[0].order
    );

    assert_eq!(
        store.holds_open().expect("the gate list decodes"),
        vec![HoldId::from("fx-gate")]
    );
    let filed = store
        .create(
            &NewItem {
                title: String::from("t"),
                description: String::from("d"),
                item_type: String::from("task"),
                ..NewItem::default()
            },
            &the_test(),
        )
        .expect("the create answer decodes");
    assert_eq!(filed, "fx-new");
    assert_eq!(
        store
            .hold_raise(&ItemId::from("fx-held"), "why", &the_test())
            .expect("the hold answer decodes"),
        HoldId::from("fx-raised")
    );

    let asked = envelopes(&log);
    assert_eq!(asked.len(), 7, "one line per call: {asked:?}");
    assert!(
        asked.iter().all(|value| value == "1"),
        "every call asks for the envelope: {asked:?}"
    );
}

/// An empty gate listing is `null` inside the envelope — measured on 1.3.0 —
/// and reads as no gates, as the bare `null` did.
#[test]
fn an_empty_listing_inside_the_envelope_is_no_rows() {
    let _guard = path_lock();
    let dir = Fixture::new("store-envelope-empty");
    let log = dir.path("envelope");
    let bin = envelope_bd(&dir, &log);
    dir.file(
        "answers/gate-list.json",
        "{\"data\": null, \"schema_version\": 1}\n",
    );
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    assert_eq!(
        Bd::at_bin(&root, &bin)
            .holds_open()
            .expect("an empty listing is an answer"),
        Vec::<HoldId>::new()
    );
}

/// A not_found error is an item that is not there (exit 1), and so is an
/// error with no code at all, which is what bd 1.3.0 answers. Any other code
/// is a store that did not answer (exit 3).
#[test]
fn a_show_error_is_classified_by_its_code() {
    let _guard = path_lock();
    let dir = Fixture::new("store-envelope-errors");
    let log = dir.path("envelope");
    let bin = envelope_bd(&dir, &log);
    dir.file(
        "answers/show-fx-uncoded.json",
        r#"{"data": {"error": "no issues found matching the provided IDs", "hint": "some IDs may reference deleted/purged records with no trace left in the live database — try 'bd history <id>' to check"}, "schema_version": 1}"#,
    )
    .file(
        "answers/show-fx-uncoded.err",
        "Issue fx-uncoded not found\nHint: this ID may have never existed, or may reference a \
         deleted/purged record with no trace left in the live database — try 'bd history \
         fx-uncoded'\n",
    )
    .file(
        "answers/show-fx-coded.json",
        r#"{"data": {"error": "no issue found", "code": "not_found", "hint": "check the id"}, "schema_version": 1}"#,
    )
    .file("answers/show-fx-coded.err", "Error: no issue found\n")
    .file(
        "answers/show-fx-broken.json",
        r#"{"data": {"error": "the database is locked", "code": "database_error"}, "schema_version": 1}"#,
    )
    .file("answers/show-fx-broken.err", "Error: the database is locked\n");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);

    for item in ["fx-uncoded", "fx-coded"] {
        let answer = store.show(item).expect_err("the store holds no such item");
        assert!(
            matches!(answer, StoreError::Refused(_)),
            "{item}: {answer:?}"
        );
        assert_eq!(Stop::from(answer).code, REFUSED, "{item}");
    }

    let answer = store
        .show("fx-broken")
        .expect_err("the store did not answer");
    assert!(
        matches!(&answer, StoreError::Unreadable(why) if why.contains("database_error")),
        "{answer:?}"
    );
    assert_eq!(Stop::from(answer).code, COULD_NOT_TELL);
}

/// An id naming more than one item answers the same JSON error a missing one
/// does — measured on bd 1.3.0 — and is told apart by stderr alone, which
/// names the matches. It is still the record's answer (exit 1), and the
/// refusal carries every match bd listed.
///
/// Two stderr shapes, because 1.3.0 moved the words: `63` answers the pinned
/// 1.3.0's `ambiguous issue ID:`, and `64` the `ambiguous ID` of 1.2.2, which a
/// bd off the pin still answers and a verb still runs on.
#[test]
fn an_ambiguous_show_is_missing_and_names_the_matches_off_stderr() {
    let _guard = path_lock();
    let dir = Fixture::new("store-envelope-ambiguous");
    let log = dir.path("envelope");
    let bin = envelope_bd(&dir, &log);
    let error =
        r#"{"data": {"error": "no issues found matching the provided IDs"}, "schema_version": 1}"#;
    dir.file("answers/show-63.json", error)
        .file(
            "answers/show-63.err",
            "Error fetching 63: ambiguous issue ID: \"63\" matches 2 issues: [fx-63h fx-63u]\n\
             Use more characters to disambiguate\n",
        )
        .file("answers/show-64.json", error)
        .file(
            "answers/show-64.err",
            "Error fetching 64: ambiguous ID \"64\" matches 2 issues: [fx-64h fx-64u]\nUse more \
             characters to disambiguate\n",
        );
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    for id in ["63", "64"] {
        let answer = Bd::at_bin(&root, &bin)
            .show(id)
            .expect_err("an ambiguous id is no one item");
        assert_eq!(
            answer,
            StoreError::Refused(format!(
                "`{id}` matches more than one item — fx-{id}h, fx-{id}u — and more of the id \
                 says which one this is"
            ))
        );
        assert_eq!(Stop::from(answer).code, REFUSED);
    }
}

/// A schema_version above the one this binary knows is read anyway: beads'
/// consumer advice is to warn and parse, and a key added in a newer schema is
/// one nothing here reads.
#[test]
fn a_newer_schema_is_read_anyway() {
    let _guard = path_lock();
    let dir = Fixture::new("store-envelope-newer");
    let log = dir.path("envelope");
    let bin = envelope_bd(&dir, &log);
    dir.file(
        "answers/show-fx-later.json",
        r#"{"data": [{"id": "fx-later", "title": "from a newer bd", "status": "open", "a_new_key": 7}], "schema_version": 2}"#,
    );
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let read = Bd::at_bin(&root, &bin)
        .show("fx-later")
        .expect("a newer schema is still read");
    assert_eq!(read.title, "from a newer bd");
}

/// A `bd` that never answers: it records its own pid and the pid of the child
/// it leaves in its group, then waits on that child for longer than any arm
/// runs.
fn hung_bd(dir: &Fixture) -> PathBuf {
    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             sleep 30 &\n\
             printf '%s\\n' \"$$\" \"$!\" > '{pids}'\n\
             wait\n",
            pids = dir.path("pids").display(),
        ),
    )
    .expect("the shim is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the shim is executable");
    bin
}

/// Whether `pid` still names a process, asked for up to two seconds: a killed
/// grandchild is reaped by whoever adopted it, not at once.
fn still_running(pid: &str) -> bool {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        let alive = std::process::Command::new("/bin/sh")
            .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
            .status()
            .expect("the probe runs")
            .success();
        if !alive || Instant::now() >= until {
            return alive;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A store that does not answer is Unreadable inside its bound plus
/// `DRAIN_GRACE`, and the call leaves nothing of its own running: the stub
/// and the child it forked share the group the bound kills.
///
/// The bound is SHORTENED through `with_timeout`, because the store's own is a
/// minute; the refusal names the call and the bound it ran on, and a read's
/// refusal says nothing about a write.
#[test]
fn a_store_that_does_not_answer_is_unreadable_within_its_bound() {
    let _guard = path_lock();
    let dir = Fixture::new("store-hung-read");
    let bin = hung_bd(&dir);
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let bound = Duration::from_secs(1);

    let started = Instant::now();
    let answer = Bd::at_bin(&root, &bin)
        .with_timeout(bound)
        .list(&Filter::Ready);
    let took = started.elapsed();

    let Err(StoreError::Unreadable(why)) = answer else {
        panic!("a store that never answers is Unreadable: {answer:?}");
    };
    assert!(
        took < bound + DRAIN_GRACE,
        "the refusal came inside the bound plus DRAIN_GRACE: {took:?}"
    );
    assert_eq!(
        why,
        format!(
            "`{} ready --json -n 0` did not answer within 1s",
            bin.display()
        )
    );
    assert_eq!(Stop::from(StoreError::Unreadable(why)).code, COULD_NOT_TELL);

    let pids = std::fs::read_to_string(dir.path("pids")).expect("the stub recorded its pids");
    let pids: Vec<&str> = pids.lines().collect();
    assert_eq!(pids.len(), 2, "the stub and its child: {pids:?}");
    for pid in pids {
        assert!(!still_running(pid), "{pid} outlived the bound's kill");
    }
}

/// The same bound on a WRITE says what the kill cannot: whether the write
/// landed, so the item is read before anything is written to it again.
#[test]
fn a_write_that_does_not_answer_says_its_effect_cannot_be_told() {
    let _guard = path_lock();
    let dir = Fixture::new("store-hung-write");
    let bin = hung_bd(&dir);
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let seat = SeatId::parse(SEAT).expect("the seat's id parses");
    let answer = Bd::at_bin(&root, &bin)
        .with_timeout(Duration::from_millis(200))
        .update(&ItemId::from("fx-1"), &Update::assignee(seat), &the_test());

    let Err(StoreError::Unreadable(why)) = answer else {
        panic!("a write that never answers is Unreadable: {answer:?}");
    };
    assert!(
        why.starts_with(&format!(
            "`{} update fx-1 --assignee {SEAT} --actor run:the-test` did not answer within 200ms \
             — the write's effect cannot be told",
            bin.display()
        )),
        "{why}"
    );
}

/// A `bd` at an absolute path that records each call's argv as ONE line, each
/// argument in brackets, and answers every call with `answer`.
fn argv_bd(dir: &Fixture, log: &Path, answer: &str) -> PathBuf {
    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             printf '[%s]' \"$@\" >> '{log}'\n\
             printf '\\n' >> '{log}'\n\
             printf '%s\\n' '{answer}'\n",
            log = log.display(),
        ),
    )
    .expect("the shim is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the shim is executable");
    bin
}

/// Each call the shim was handed, its argv after `-C <root>` in brackets.
fn argvs(log: &Path, root: &Path) -> Vec<String> {
    let lead = format!("[-C][{}]", root.display());
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| line.strip_prefix(&lead).unwrap_or(line).to_string())
        .collect()
}

/// An update is ONE call naming each field the change moves: the title, the
/// assignee as the seat's full id, both on one call, and a cleared assignee as
/// the empty string, which is what bd clears the field with.
#[test]
fn an_update_is_one_call_naming_each_field_it_moves() {
    let _guard = path_lock();
    let dir = Fixture::new("store-update-argv");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, "");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);
    let id = ItemId::from("fx-1");
    let seat = SeatId::parse(SEAT).expect("the seat's id parses");

    for change in [
        Update::title(String::from("a new title")),
        Update::assignee(seat),
        Update {
            title: Some(String::from("both")),
            assignee: Some(Some(seat)),
        },
        Update::unassigned(),
    ] {
        store
            .update(&id, &change, &the_test())
            .unwrap_or_else(|e| panic!("{change:?} lands: {e}"));
    }
    assert_eq!(
        argvs(&log, &root),
        [
            String::from("[update][fx-1][--title][a new title][--actor][run:the-test]"),
            format!("[update][fx-1][--assignee][{SEAT}][--actor][run:the-test]"),
            format!("[update][fx-1][--title][both][--assignee][{SEAT}][--actor][run:the-test]"),
            String::from("[update][fx-1][--assignee][][--actor][run:the-test]"),
        ]
    );
}

/// The one metadata object an `update --metadata` call carried, off its argv.
fn metadata_of(argv: &str) -> serde_json::Value {
    let payload = argv
        .strip_prefix("[update][fx-1][--metadata][")
        .and_then(|rest| rest.strip_suffix("][--actor][run:the-test]"))
        .unwrap_or_else(|| panic!("one metadata write on fx-1: {argv}"));
    serde_json::from_str(payload).expect("the metadata is one JSON object")
}

/// AN ORDER AND A RUN'S RECORD ARE EACH ONE `--metadata` WRITE of fleet's own
/// dotted key, the version stamped in beside the contract's fields — the shape
/// bd merges at the top level, so neither write erases the other key. The
/// verb hands the adapter the typed value; the key is the adapter's alone.
#[test]
fn an_order_and_a_run_record_are_one_metadata_write_of_fleets_own_key_each() {
    let _guard = path_lock();
    let dir = Fixture::new("store-order-run-argv");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, "");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);
    let id = ItemId::from("fx-1");
    let at = "2026-09-24T10:00:00Z";

    store
        .order_set(
            &id,
            &Order {
                kind: store::OrderKind::Dispatch,
                by: Actor::typed("run:an-architect")
                    .expect("typed")
                    .expect("a run"),
                seat: Some(SeatId::parse(SEAT).expect("the seat's id parses")),
                at: Stamp::parse(at).expect("a stamp"),
            },
            &the_test(),
        )
        .expect("the order lands");
    store
        .order_set(
            &id,
            &Order {
                kind: store::OrderKind::Review,
                by: Actor::typed("run:an-architect")
                    .expect("typed")
                    .expect("a run"),
                seat: None,
                at: Stamp::parse(at).expect("a stamp"),
            },
            &the_test(),
        )
        .expect("an order naming no seat lands");
    store
        .run_set(
            &id,
            &RunRecord {
                hash: String::from("h1"),
                workflow: String::from("greet"),
                pack: String::from("ts"),
                entry: String::from("greet.ts"),
                started_at: Stamp::parse(at).expect("a stamp"),
            },
            &the_test(),
        )
        .expect("the run's record lands");

    let asked = argvs(&log, &root);
    assert_eq!(asked.len(), 3, "one call per write: {asked:?}");
    assert_eq!(
        metadata_of(&asked[0]),
        serde_json::json!({ "fleet.orders": {
            "v": 1, "by": "run:an-architect", "kind": "dispatch", "seat": SEAT, "at": at,
        }})
    );
    assert_eq!(
        metadata_of(&asked[1]),
        serde_json::json!({ "fleet.orders": {
            "v": 1, "by": "run:an-architect", "kind": "review", "at": at,
        }}),
        "an order naming no seat carries no seat key"
    );
    assert_eq!(
        metadata_of(&asked[2]),
        serde_json::json!({ "fleet.run": {
            "v": 1, "hash": "h1", "workflow": "greet", "pack": "ts", "entry": "greet.ts",
            "started_at": at,
        }})
    );
}

/// A withdrawal is ONE update clearing the assignee and unsetting fleet's
/// order key; the retire's fenced withdrawal is the same call, fenced on the
/// seat and the status it listed, reopening the item beside it.
#[test]
fn a_withdrawal_is_one_update_clearing_the_assignee_and_the_order() {
    let _guard = path_lock();
    let dir = Fixture::new("store-withdraw-argv");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, "");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);
    let id = ItemId::from("fx-1");
    let seat = SeatId::parse(SEAT).expect("the seat's id parses");

    store
        .order_withdraw(&id, &the_test())
        .expect("the withdrawal lands");
    store
        .order_withdraw_from(
            &id,
            &seat,
            &fleet_core::store::Status::InProgress,
            &the_test(),
        )
        .expect("the fenced withdrawal lands");
    assert_eq!(
        argvs(&log, &root),
        [
            String::from(
                "[update][fx-1][--assignee][][--unset-metadata][fleet.orders][--actor][run:the-test]"
            ),
            format!(
                "[update][fx-1][--if-assignee][{SEAT}][--if-status][in_progress][--assignee][]\
                 [--unset-metadata][fleet.orders][--status][open][--actor][run:the-test]"
            ),
        ]
    );
}

/// A CLEAR READS THE HOLD FIRST, because bd 1.3.0 answers a resolve of a
/// resolved gate with exit 0 and writes nothing: an open hold is read and then
/// resolved, and a closed one is Refused with nothing but the read run.
#[test]
fn a_hold_clear_reads_the_hold_and_resolves_only_an_open_one() {
    let _guard = path_lock();
    let root_of = |dir: &Fixture| {
        let root = dir.path("project");
        std::fs::create_dir_all(&root).expect("the project root is created");
        root
    };

    let dir = Fixture::new("store-hold-clear-open");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, r#"[{"id": "fx-gate", "status": "open"}]"#);
    let root = root_of(&dir);
    Bd::at_bin(&root, &bin)
        .hold_clear(&HoldId::from("fx-gate"), &the_test())
        .expect("an open hold clears");
    assert_eq!(
        argvs(&log, &root),
        [
            String::from("[show][fx-gate][--json]"),
            String::from("[gate][resolve][fx-gate][--actor][run:the-test]"),
        ]
    );

    let dir = Fixture::new("store-hold-clear-cleared");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, r#"[{"id": "fx-gate", "status": "closed"}]"#);
    let root = root_of(&dir);
    assert_eq!(
        Bd::at_bin(&root, &bin).hold_clear(&HoldId::from("fx-gate"), &the_test()),
        Err(StoreError::Refused(String::from(
            "fx-gate is already cleared"
        )))
    );
    assert_eq!(
        argvs(&log, &root),
        [String::from("[show][fx-gate][--json]")],
        "nothing but the read was run"
    );
}

/// An update naming neither field is refused before the binary is asked.
#[test]
fn an_update_naming_nothing_runs_nothing() {
    let _guard = path_lock();
    let dir = Fixture::new("store-update-nothing");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, "");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let answer =
        Bd::at_bin(&root, &bin).update(&ItemId::from("fx-1"), &Update::default(), &the_test());
    assert_eq!(
        answer,
        Err(StoreError::Unreadable(String::from(
            "an update names neither a title nor an assignee — nothing was written"
        )))
    );
    assert_eq!(argvs(&log, &root), Vec::<String>::new(), "bd was not asked");
}

/// A create carries `--priority` where the item names one, and no such flag
/// where it names none.
#[test]
fn a_create_names_its_priority_only_where_the_item_does() {
    let _guard = path_lock();
    let dir = Fixture::new("store-create-priority");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, r#"{"id": "fx-new"}"#);
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let store = Bd::at_bin(&root, &bin);

    for priority in [Some(0), None] {
        let filed = store
            .create(
                &NewItem {
                    title: String::from("t"),
                    description: String::from("d"),
                    item_type: String::from("task"),
                    labels: vec![String::from("a"), String::from("b")],
                    priority,
                },
                &the_test(),
            )
            .expect("the create answer decodes");
        assert_eq!(filed, "fx-new");
    }
    assert_eq!(
        argvs(&log, &root),
        [
            "[create][--title][t][--description][d][--type][task][--labels][a,b][--priority][0]\
             [--actor][run:the-test][--json]",
            "[create][--title][t][--description][d][--type][task][--labels][a,b]\
             [--actor][run:the-test][--json]",
        ]
    );
}

/// A create whose priority is past 4 is refused with the item's own reason,
/// and bd is never asked to file it; the board held in memory refuses it in
/// the same words.
#[test]
fn a_create_past_priority_four_is_refused_before_bd_is_asked() {
    let _guard = path_lock();
    let dir = Fixture::new("store-create-priority-five");
    let log = dir.path("argv");
    let bin = argv_bd(&dir, &log, r#"{"id": "fx-new"}"#);
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let item = NewItem {
        title: String::from("t"),
        description: String::from("d"),
        item_type: String::from("task"),
        labels: Vec::new(),
        priority: Some(5),
    };
    let refusal = Err(StoreError::Unreadable(String::from(
        "the item `t` does not validate: priority is 5; the range is 0 to 4 — nothing was written",
    )));

    assert_eq!(Bd::at_bin(&root, &bin).create(&item, &the_test()), refusal);
    assert_eq!(argvs(&log, &root), Vec::<String>::new(), "bd was not asked");

    let board = fleet_core::test_support::Board::new("store-fake-priority-five");
    assert_eq!(board.store.create(&item, &the_test()), refusal);
}

/// A `bd` at an absolute path that records each call's argv as `argv_bd`'s
/// does, and answers every call with `stdout`, `stderr` and exit `code`.
fn answering_bd(dir: &Fixture, log: &Path, stdout: &str, stderr: &str, code: i32) -> PathBuf {
    std::fs::write(dir.path("stdout"), stdout).expect("the answer is written");
    std::fs::write(dir.path("stderr"), stderr).expect("the answer is written");
    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             printf '[%s]' \"$@\" >> '{log}'\n\
             printf '\\n' >> '{log}'\n\
             cat '{out}'\n\
             cat '{err}' >&2\n\
             exit {code}\n",
            log = log.display(),
            out = dir.path("stdout").display(),
            err = dir.path("stderr").display(),
        ),
    )
    .expect("the shim is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the shim is executable");
    bin
}

/// What bd 1.3.0 prints on stderr for an `update` of an item it does not
/// hold — measured on a scratch board, the same three lines whatever flags the
/// call carried.
fn unheld_update(id: &str) -> String {
    format!(
        "Error resolving {id}: no issue found matching \"{id}\"\n\
         Error: 1 of 1 issues failed to update\n  \
         {id}: resolving issue: no issue found matching \"{id}\"\n"
    )
}

/// A WRITE ON AN ITEM BD DOES NOT HOLD IS REFUSED, and nothing was written:
/// the contract's `missing`, for every write the adapter makes, whichever of
/// bd's three shapes of that answer it came in. An exit 1 naming another id,
/// or saying anything else, is still a store that did not answer.
#[test]
fn a_write_on_an_item_bd_does_not_hold_is_refused() {
    let _guard = path_lock();
    let gone = "fx-zzzz";
    let id = ItemId::from(gone);
    let seat = SeatId::parse(SEAT).expect("the seat's id parses");
    let run = RunRecord {
        hash: String::from("h1"),
        workflow: String::from("greet"),
        pack: String::from("ts"),
        entry: String::from("greet.ts"),
        started_at: Stamp::parse("2026-09-25T10:00:00Z").expect("a stamp"),
    };
    type Write<'a> = Box<dyn Fn(&Bd) -> Result<(), StoreError> + 'a>;
    let writes: Vec<(&str, Write)> = vec![
        (
            "update",
            Box::new(|bd| bd.update(&id, &Update::title(String::from("t")), &the_test())),
        ),
        ("run.set", Box::new(|bd| bd.run_set(&id, &run, &the_test()))),
        (
            "order.withdraw",
            Box::new(|bd| bd.order_withdraw(&id, &the_test())),
        ),
        ("reopen", Box::new(|bd| bd.reopen(gone, "run:the-test"))),
        (
            "hand_over",
            Box::new(|bd| bd.hand_over(gone, SEAT, "", "run:the-test")),
        ),
        (
            "order_withdraw_from",
            Box::new(|bd| {
                bd.order_withdraw_from(
                    &id,
                    &seat,
                    &fleet_core::store::Status::InProgress,
                    &the_test(),
                )
            }),
        ),
    ];
    for (n, (verb, write)) in writes.iter().enumerate() {
        let dir = Fixture::new(&format!("store-missing-write-{n}"));
        let log = dir.path("argv");
        let bin = answering_bd(&dir, &log, "", &unheld_update(gone), 1);
        let root = dir.path("project");
        std::fs::create_dir_all(&root).expect("the project root is created");
        match write(&Bd::at_bin(&root, &bin)) {
            Err(StoreError::Refused(why)) => assert_eq!(
                why,
                format!(
                    "{gone} is not in the store — nothing was written (Error resolving {gone}: \
                     no issue found matching \"{gone}\")"
                ),
                "{verb}"
            ),
            other => panic!("{verb} on an item bd does not hold is Refused: {other:?}"),
        }
    }

    let answered = |label: &str, stdout: &str, stderr: &str| {
        let dir = Fixture::new(label);
        let log = dir.path("argv");
        let bin = answering_bd(&dir, &log, stdout, stderr, 1);
        let root = dir.path("project");
        std::fs::create_dir_all(&root).expect("the project root is created");
        (dir, Bd::at_bin(&root, &bin))
    };
    let (_dir, bd) = answered(
        "store-missing-append",
        &format!(
            r#"{{"data":{{"error":"resolving {gone}: no issue found matching \"{gone}\""}},"schema_version":1}}"#
        ),
        "",
    );
    match bd.append(&id, &an_order(), &the_test()) {
        Err(StoreError::Refused(why)) => assert!(
            why.starts_with(&format!("{gone} is not in the store — nothing was written")),
            "{why}"
        ),
        other => panic!("an append to an item bd does not hold is Refused: {other:?}"),
    }
    let (_dir, bd) = answered(
        "store-missing-hold",
        &format!(r#"{{"data":{{"error":"issue not found: {gone}"}},"schema_version":1}}"#),
        "",
    );
    assert_eq!(
        bd.hold_raise(&id, "why", &the_test()),
        Err(StoreError::Refused(format!(
            "{gone} is not in the store — nothing was written (issue not found: {gone})"
        )))
    );

    for (label, said) in [
        ("store-missing-other", unheld_update("fx-other")),
        (
            "store-missing-held",
            format!("Error: cannot reassign {gone}: held by \"s1\" (in_progress)\n"),
        ),
        (
            "store-missing-ambiguous",
            String::from(
                "Error resolving fx-z: ambiguous issue ID: \"fx-z\" matches 2 issues: \
                 [fx-zzzz fx-zzza]\n",
            ),
        ),
    ] {
        let (_dir, bd) = answered(label, "", &said);
        match bd.update(&id, &Update::title(String::from("t")), &the_test()) {
            Err(StoreError::Unreadable(_)) => {}
            other => panic!("{label}: an exit 1 that names {gone} as no missing item is could not tell: {other:?}"),
        }
    }
}

/// The version is `bd`, at the version the FIRST line `bd --version` prints
/// names: its first token opening on a digit, bare of a leading `v`. A line
/// naming none is answered whole and trimmed, so what bd said is still read.
#[test]
fn the_version_is_the_one_bd_s_first_line_names() {
    let _guard = path_lock();
    for (label, printed, version) in [
        ("store-version", "  bd version 9.9.9 (a stub)  ", "9.9.9"),
        ("store-version-v", "bd version v2.0.1", "2.0.1"),
        (
            "store-version-none",
            "  not a version at all  ",
            "not a version at all",
        ),
    ] {
        let dir = Fixture::new(label);
        let log = dir.path("argv");
        let bin = argv_bd(&dir, &log, printed);
        let root = dir.path("project");
        std::fs::create_dir_all(&root).expect("the project root is created");

        let answered = Bd::at_bin(&root, &bin)
            .version()
            .expect("the version reads");
        assert_eq!(answered.name, "bd", "{label}");
        assert_eq!(answered.version, version, "{label}");
        assert_eq!(argvs(&log, &root), ["[--version]"], "{label}");
    }
}

/// A `show` row in the shape bd 1.3.0 answers, holding one dependency on an
/// open item, of `kind` — or of no stated type at all when `kind` is `None`.
fn depending_on(kind: Option<&str>) -> serde_json::Value {
    let mut dependency = serde_json::json!({
        "id": "fx-up",
        "title": "upstream",
        "status": "open",
        "issue_type": "task",
    });
    if let Some(kind) = kind {
        dependency["dependency_type"] = kind.into();
    }
    serde_json::json!({
        "id": "fx-down",
        "title": "downstream",
        "status": "open",
        "issue_type": "task",
        "dependencies": [dependency],
    })
}

/// A link bd's ready set does not honour blocks nothing, however open the item
/// at its far end — and `why_not_ready` reads `blockers` back to a person as
/// what an item is "blocked by".
#[test]
fn an_open_discovered_from_link_is_not_a_blocker() {
    let item =
        item_from("fx-down", &depending_on(Some("discovered-from"))).expect("the row decodes");
    assert!(
        item.blockers.is_empty(),
        "bd answers an item whose only open link is discovered-from as ready: {:?}",
        item.blockers
    );
}

#[test]
fn an_open_blocks_dependency_is_a_blocker() {
    let item = item_from("fx-down", &depending_on(Some("blocks"))).expect("the row decodes");
    assert_eq!(item.blockers, vec![String::from("fx-up")]);
}

/// The cautious reading, as for a missing status: an entry that does not say
/// what kind of link it is may be one that blocks.
#[test]
fn a_dependency_of_no_stated_type_is_a_blocker() {
    let item = item_from("fx-down", &depending_on(None)).expect("the row decodes");
    assert_eq!(item.blockers, vec![String::from("fx-up")]);
}

// ---- the timeline on the board held in memory -------------------------------

fn an_order() -> Body {
    Body::Ordered(Ordered {
        order: OrderKind::Dispatch,
        seat: None,
    })
}

/// The contract suite's bd-only plants, made on the fake: its comments are
/// read by the same `read_row` bd's are, so a person's text is left out and an
/// entry whose author is no actor refuses the whole read, naming the comment.
#[test]
fn the_fake_leaves_a_persons_comment_out_and_refuses_a_malformed_entry() {
    let board = fleet_core::test_support::Board::new("store-fake-plants");
    let item = board.item("an item a person commented on");

    board
        .store
        .comment(&item, "Alberto Vildosola", "a person's words");
    assert_eq!(
        board
            .store
            .timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "a person's comment is not an entry"
    );

    let comment = board.store.comment(
        &item,
        "Alberto Vildosola",
        r#"{"fleet.entry":1,"kind":"ordered","order":"dispatch"}"#,
    );
    match board.store.timeline(&ItemId::from(item.as_str())) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.contains(&comment) && why.contains("Alberto Vildosola"),
            "the refusal names the comment and its author: {why}"
        ),
        other => panic!("an entry whose author is no actor refuses the read: {other:?}"),
    }
}

/// The fake keeps bd's refusal word for word: a body that breaks its kind's
/// rules is never written, and an item it does not hold is Refused.
#[test]
fn the_fake_refuses_an_entry_that_does_not_validate_and_an_item_it_does_not_hold() {
    let board = fleet_core::test_support::Board::new("store-fake-refusals");
    let item = board.item("an item a short sha is appended to");
    let by = seat_actor("a-short-seat");

    match board
        .store
        .append(&ItemId::from(item.as_str()), &a_delivery("1111111"), &by)
    {
        Err(StoreError::Unreadable(why)) => assert!(
            why.starts_with(&format!(
                "the delivered entry for {item} does not validate: `commit`"
            )) && why.ends_with(" — nothing was written"),
            "{why}"
        ),
        other => panic!("a delivery naming a 7-character commit is refused: {other:?}"),
    }
    assert_eq!(
        board
            .store
            .timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "and nothing was written"
    );

    match board
        .store
        .append(&ItemId::from("fx-nobody-filed-this"), &an_order(), &by)
    {
        Err(StoreError::Refused(_)) => {}
        other => panic!("an append to an item nobody filed is Refused: {other:?}"),
    }
}

/// A store that drops its writes still answers an id, which is exactly why a
/// verb reads the timeline back rather than trusting it.
#[test]
fn a_deaf_store_answers_an_id_and_keeps_no_entry() {
    let board = fleet_core::test_support::Board::new("store-fake-deaf");
    let item = board.item("an item whose entry is dropped");
    board.store.ignore_writes();

    let id = board
        .store
        .append(
            &ItemId::from(item.as_str()),
            &an_order(),
            &seat_actor("a-deaf-seat"),
        )
        .expect("the append answers");
    assert!(!id.is_empty(), "the store names what it was handed");
    assert_eq!(
        board
            .store
            .timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "and the timeline holds nothing"
    );
}

#[test]
fn recorded_on_a_deaf_store_is_unconfirmed() {
    let board = fleet_core::test_support::Board::new("store-fake-recorded-deaf");
    let item = board.item("an item whose entry is dropped");
    board.store.ignore_writes();

    match recorded(
        &board.store,
        &item,
        &a_delivery(A_COMMIT),
        &seat_actor("a-deaf-seat"),
    ) {
        Err(Unrecorded::Unconfirmed(why)) => assert!(
            why.contains(&item) && why.contains("does not hold") && why.contains("delivered"),
            "the refusal names the item, the kind and what the timeline lacks: {why}"
        ),
        other => panic!("a write the store dropped is unconfirmed: {other:?}"),
    }
}

/// The helper's one success: the id the store answered, read back with the
/// body and the actor written, and the one log line the fake keeps per append.
#[test]
fn recorded_answers_the_id_it_read_back() {
    let board = fleet_core::test_support::Board::new("store-fake-recorded");
    let item = board.item("an item an entry is recorded on");
    let by = seat_actor("a-recording-seat");
    board.forget_writes();

    let id = recorded(&board.store, &item, &an_order(), &by).expect("the entry is recorded");
    let timeline = board
        .store
        .timeline(&ItemId::from(item.as_str()))
        .expect("the timeline reads");
    let read = fleet_core::entry::Timeline(&timeline)
        .entry(&id)
        .expect("the entry is on the timeline");
    assert_eq!(read.body, an_order());
    assert_eq!(read.by, by);
    assert_eq!(board.store.wrote(), [format!("append {item} ordered {by}")]);
}

/// A write refused before it reached the store is not written, which is a
/// different sentence from a write the store took and the read did not show.
#[test]
fn recorded_of_an_entry_that_does_not_validate_is_not_written() {
    let board = fleet_core::test_support::Board::new("store-fake-recorded-invalid");
    let item = board.item("an item a short sha is recorded on");

    match recorded(
        &board.store,
        &item,
        &a_delivery("1111111"),
        &seat_actor("a-short-seat"),
    ) {
        Err(Unrecorded::NotWritten(StoreError::Unreadable(why))) => {
            assert!(why.contains("nothing was written"), "{why}")
        }
        other => panic!("a body that does not validate is not written: {other:?}"),
    }
}

// ---- the opener: `[store] adapter`, read out of the project's own file ------

/// A project's policy as the opener reads it, parsed from `text`.
fn policy_of(text: &str) -> toml::Table {
    text.parse().expect("the fixture policy parses")
}

/// The opener over `policy` for the project at `root`, not strict and under
/// `timeout`, on a search path holding nothing.
fn opened(
    root: &Path,
    policy: &toml::Table,
    timeout: Duration,
) -> Result<Box<dyn Store>, StoreError> {
    store::open(&store::Opening {
        root,
        policy,
        search_path: "",
        strict: false,
        timeout,
        packs: None,
    })
}

/// An adapter executable in `dir` that copies its request to `request.json`
/// and then runs `answer`.
fn an_adapter(dir: &Fixture, answer: &str) -> PathBuf {
    let bin = dir.path("adapter");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\ncat > '{dir}/request.json'\n{answer}\n",
            dir = dir.root.display(),
        ),
    )
    .expect("the adapter is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the adapter is executable");
    bin
}

/// `[store] adapter` naming an executable by absolute path opens the store
/// that executable answers: a `show` is one call to it, carrying the project's
/// root, and the item is the one it answered. The bound the caller hands in is
/// the call's own — an adapter that outruns it is could not tell.
#[test]
fn an_adapter_named_by_absolute_path_is_the_store_opened() {
    let dir = Fixture::new("store-open-exec");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let adapter = an_adapter(
        &dir,
        r#"printf '%s\n' '{"schema_version":1,"item":{"id":"fx-c3d4","title":"an item the adapter holds","status":"open","type":"task","labels":[],"order":{"state":"none"}}}'"#,
    );
    let policy = policy_of(&format!("[store]\nadapter = {:?}\n", adapter.display()));

    let store =
        opened(&root, &policy, store::STORE_TIMEOUT).expect("the adapter is an executable file");
    let item = store.show("c3d4").expect("the adapter answered an item");

    assert_eq!(item.id, ItemId::from("fx-c3d4"));
    assert_eq!(item.title, "an item the adapter holds");
    let request: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path("request.json")).expect("the adapter was called"),
    )
    .expect("the request is one JSON value");
    assert_eq!(
        request["root"],
        serde_json::json!(root.display().to_string()),
        "the request names the root the opener was handed: {request}"
    );

    let slow = Fixture::new("store-open-exec-slow");
    let adapter = an_adapter(&slow, "sleep 5");
    let policy = policy_of(&format!("[store]\nadapter = {:?}\n", adapter.display()));
    let started = Instant::now();
    let refused = opened(&root, &policy, Duration::from_millis(300))
        .expect("the adapter is an executable file")
        .show("c3d4");
    assert!(
        matches!(&refused, Err(StoreError::Unreadable(why)) if why.contains("did not answer within")),
        "an adapter that outruns the opener's bound is could not tell: {refused:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the bound was the opener's and not the store's own: {:?}",
        started.elapsed()
    );
}

/// `"bd"`, or no `[store] adapter` at all, opens the built-in store: the one
/// whose capabilities are answered without a call and declare its export.
#[test]
fn the_built_in_store_is_opened_where_the_file_names_it_or_names_nothing() {
    let dir = Fixture::new("store-open-built-in");
    for policy in [
        policy_of(""),
        policy_of("[store]\n"),
        policy_of(&format!("[store]\nadapter = {:?}\n", store::bd::NAME)),
    ] {
        let store =
            opened(&dir.root, &policy, store::STORE_TIMEOUT).expect("the built-in store opens");
        let export = store
            .capabilities()
            .expect("the built-in store answers without a call")
            .export
            .expect("and declares its export");
        assert_eq!(export.file, store::bd::EXPORT, "{policy:?}");
    }
}

/// The built-in store's item prefix is its own config's `issue-prefix:`,
/// answered without a call — and `None` where the config names none, or where
/// there is no config to read, which is never a prefix guessed.
#[test]
fn the_built_in_stores_item_prefix_is_its_own_configs() {
    let dir = Fixture::new("store-item-prefix");
    let prefix = || {
        opened(&dir.root, &policy_of(""), store::STORE_TIMEOUT)
            .expect("the built-in store opens")
            .capabilities()
            .expect("the built-in store answers without a call")
            .item_prefix
    };
    assert_eq!(prefix(), None, "no config at all");
    dir.file(
        ".beads/config.yaml",
        "# issue-prefix: \"\"\ndatabase: dolt\n",
    );
    assert_eq!(prefix(), None, "a config naming none");
    dir.file(
        ".beads/config.yaml",
        "# the store's own\nissue-prefix: \"zz\"\n",
    );
    assert_eq!(prefix().as_deref(), Some("zz"));
}

/// A path that is no executable file, and a value that is neither form, are
/// could not tell before anything is run, each naming what the file said.
#[test]
fn an_adapter_that_is_no_executable_or_neither_form_is_could_not_tell() {
    let dir = Fixture::new("store-open-refused");
    dir.file("not-executable", "#!/bin/sh\nexit 0\n");
    for path in [
        dir.path("not-executable"),
        dir.path("absent"),
        dir.root.clone(),
    ] {
        let policy = policy_of(&format!("[store]\nadapter = {:?}\n", path.display()));
        match opened(&dir.root, &policy, store::STORE_TIMEOUT) {
            Err(StoreError::Unreadable(why)) => assert_eq!(
                why,
                format!(
                    "[store] adapter names `{}`, which is not an executable file",
                    path.display()
                )
            ),
            Err(other) => panic!("wanted Unreadable, got {other:?}"),
            Ok(_) => panic!("{} opened as an adapter", path.display()),
        }
    }
    for (value, named) in [("\"bin/adapter\"", "bin/adapter"), ("\"\"", ""), ("3", "3")] {
        let policy = policy_of(&format!("[store]\nadapter = {value}\n"));
        match opened(&dir.root, &policy, store::STORE_TIMEOUT) {
            Err(StoreError::Unreadable(why)) => assert_eq!(
                why,
                format!(
                    "[store] adapter is `{named}` — it is \"bd\", the name of a store adapter \
                     an installed pack carries, or an absolute path to an adapter executable"
                )
            ),
            Err(other) => panic!("wanted Unreadable, got {other:?}"),
            Ok(_) => panic!("{value} opened a store"),
        }
    }
}

// ---- a bare name, resolved through the installed packs ----------------------

/// A machine directory's packs and the binary's defaults beneath them, as
/// `fleet start` leaves them, with a pack per call of [`Machine::pack`].
struct Machine {
    dir: Fixture,
    packs_dir: PathBuf,
    defaults_dir: PathBuf,
}

impl Machine {
    fn new(label: &str) -> Machine {
        let dir = Fixture::new(label);
        let defaults_dir = dir.materialize_defaults();
        let packs_dir = dir.path("packs");
        std::fs::create_dir_all(&packs_dir).expect("the packs dir is created");
        Machine {
            dir,
            packs_dir,
            defaults_dir,
        }
    }

    fn packs(&self) -> store::PackDirs<'_> {
        store::PackDirs {
            packs_dir: &self.packs_dir,
            defaults_dir: &self.defaults_dir,
        }
    }

    /// A pack named `name` installed here, its manifest carrying `more` after
    /// the `[pack]` table.
    fn pack(&self, name: &str, more: &str) -> PathBuf {
        self.dir.file(
            &format!("packs/{name}/pack.toml"),
            &format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n{more}"),
        );
        self.dir.path(&format!("packs/{name}"))
    }

    /// The store adapter `name` in the pack at `pack`: its `adapter.toml`, and
    /// a `main` that records its request beside itself and answers a show
    /// of an item titled `title`.
    fn adapter(&self, pack: &Path, name: &str, title: &str) -> PathBuf {
        let dir = pack.join(format!("adapters/store/{name}"));
        std::fs::create_dir_all(&dir).expect("the adapter's directory is created");
        std::fs::write(
            dir.join("adapter.toml"),
            format!(
                "[adapter]\nname = \"{name}\"\nkind = \"store\"\nversion = \"0.1.0\"\n\
                 entry = \"main\"\n"
            ),
        )
        .expect("the adapter's manifest is written");
        let entry = dir.join("main");
        std::fs::write(
            &entry,
            format!(
                "#!/bin/sh\ncat > '{request}'\nprintf '%s\\n' \
                 '{{\"schema_version\":1,\"item\":{{\"id\":\"fx-c3d4\",\"title\":\"{title}\",\
                 \"status\":\"open\",\"type\":\"task\",\"labels\":[],\"order\":{{\"state\":\"none\"}}}}}}'\n",
                request = dir.join("request.json").display(),
            ),
        )
        .expect("the adapter's entry is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755))
            .expect("the entry is executable");
        dir
    }
}

/// The opener over `policy` with `packs` to resolve a name through.
fn opened_over(
    root: &Path,
    policy: &toml::Table,
    packs: Option<store::PackDirs>,
) -> Result<Box<dyn Store>, StoreError> {
    store::open(&store::Opening {
        root,
        policy,
        search_path: "",
        strict: false,
        timeout: store::STORE_TIMEOUT,
        packs,
    })
}

fn named(name: &str) -> toml::Table {
    policy_of(&format!("[store]\nadapter = {name:?}\n"))
}

/// A bare name is the store adapter an installed pack carries under
/// `adapters/store/<name>/`: its entry is the store opened, called with the
/// project's root, and the item is the one it answered.
///
/// RED-PROOF: on the base the name is neither form, and the open refuses.
#[test]
fn a_bare_name_opens_the_store_adapter_an_installed_pack_carries() {
    let machine = Machine::new("store-open-named");
    let pack = machine.pack("tracker", "");
    let adapter = machine.adapter(&pack, "x", "an item the adapter in the pack holds");
    let root = machine.dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let store = opened_over(&root, &named("x"), Some(machine.packs()))
        .expect("the name resolves to the pack's adapter");
    let item = store.show("c3d4").expect("the adapter answered an item");

    assert_eq!(item.title, "an item the adapter in the pack holds");
    let request: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(adapter.join("request.json")).expect("the entry was called"),
    )
    .expect("the request is one JSON value");
    assert_eq!(
        request["root"],
        serde_json::json!(root.display().to_string()),
        "{request}"
    );
}

/// Two packs carrying one adapter name: the higher layer's is the store
/// opened, and the lower one's is reached only once the higher carries none.
#[test]
fn a_higher_layer_carrying_the_name_shadows_a_lower_one() {
    let machine = Machine::new("store-open-shadowed");
    // `top` imports `base`, which puts it above whatever the names sort to.
    let base = machine.pack("base", "");
    let top = machine.pack(
        "top",
        "\n[imports.base]\nsource = \"../base\"\nversion = \"0.1.0\"\n",
    );
    machine.adapter(&base, "x", "the lower layer");
    let over = machine.adapter(&top, "x", "the higher layer");

    let title = || {
        opened_over(&machine.dir.root, &named("x"), Some(machine.packs()))
            .expect("the name resolves")
            .show("c3d4")
            .expect("the adapter answered")
            .title
    };
    assert_eq!(title(), "the higher layer");

    // The control: the lower layer's adapter is one the opener reaches, so
    // the answer above is precedence and not the only adapter there.
    std::fs::remove_dir_all(&over).expect("the higher adapter is taken out");
    assert_eq!(title(), "the lower layer");
}

/// A name no installed pack carries is could not tell, naming the install
/// that would carry it; a caller with no packs behind it resolves no name at
/// all; and a layering that does not resolve says why.
#[test]
fn a_name_resolved_nowhere_is_could_not_tell_naming_how_to_install_one() {
    let machine = Machine::new("store-open-nowhere");
    let pack = machine.pack("tracker", "");
    machine.adapter(&pack, "x", "not this one");

    let refusal = |packs| match opened_over(&machine.dir.root, &named("y"), packs) {
        Err(StoreError::Unreadable(why)) => why,
        Err(other) => panic!("wanted Unreadable, got {other:?}"),
        Ok(_) => panic!("`y` opened a store"),
    };
    assert_eq!(
        refusal(Some(machine.packs())),
        "no store adapter named `y` in the installed packs — `fleet pack add \
         <repo>//adapters/store/y --version <version>` installs one"
    );
    assert_eq!(
        refusal(None),
        "no store adapter named `y` resolves: no packs are installed here to carry one"
    );
    let absent = machine.dir.path("absent-defaults");
    let why = refusal(Some(store::PackDirs {
        packs_dir: &machine.packs_dir,
        defaults_dir: &absent,
    }));
    assert!(
        why.starts_with("no store adapter named `y` resolves: the pack layers do not resolve: ")
            && why.contains("`fleet start` writes them"),
        "{why}"
    );
}

/// An adapter the format refuses is never run: an entry that lost its
/// executable bit is could not tell with the fix, and so is a manifest filed
/// under a kind it does not name.
#[test]
fn an_adapter_the_format_refuses_is_could_not_tell_and_never_run() {
    let machine = Machine::new("store-open-defective");
    let pack = machine.pack("tracker", "");
    let adapter = machine.adapter(&pack, "x", "never read");
    let entry = adapter.join("main");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644))
        .expect("the entry loses its executable bit");

    let refusal = || match opened_over(&machine.dir.root, &named("x"), Some(machine.packs())) {
        Err(StoreError::Unreadable(why)) => why,
        Err(other) => panic!("wanted Unreadable, got {other:?}"),
        Ok(_) => panic!("a defective adapter opened"),
    };
    assert_eq!(
        refusal(),
        format!(
            "the store adapter `x` cannot be opened: the adapter entry `{e}` is not \
             executable — `chmod +x {e}` makes it one",
            e = entry.display()
        )
    );

    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755))
        .expect("the bit is put back");
    std::fs::write(
        adapter.join("adapter.toml"),
        "[adapter]\nname = \"x\"\nkind = \"agent\"\nversion = \"0.1.0\"\nentry = \"main\"\n",
    )
    .expect("the manifest is rewritten");
    assert_eq!(
        refusal(),
        "the store adapter `x` cannot be opened: `adapters/store/x/adapter.toml` says kind \
         `agent`, and it is filed under `adapters/store`"
    );
    assert!(
        !adapter.join("request.json").exists(),
        "the entry was never run"
    );
}

/// The project's own file: a declared project's `.fleet/project.toml` first,
/// else an embedded fleet's `fleet.toml`, else nothing — and a file that does
/// not parse is could not tell, naming it.
#[test]
fn the_projects_own_file_is_its_declaration_else_its_fleet_file() {
    let dir = Fixture::new("store-project-policy");
    assert_eq!(
        store::project_policy(&dir.root).expect("no file is an empty policy"),
        toml::Table::new()
    );

    dir.file("fleet.toml", "[store]\nadapter = \"/embedded\"\n");
    assert_eq!(
        store::project_policy(&dir.root).expect("the fleet file reads"),
        policy_of("[store]\nadapter = \"/embedded\"\n")
    );

    dir.file(".fleet/project.toml", "[store]\nadapter = \"/declared\"\n");
    assert_eq!(
        store::project_policy(&dir.root).expect("the declaration reads"),
        policy_of("[store]\nadapter = \"/declared\"\n"),
        "the declaration is the project's own statement, and wins"
    );

    dir.file(".fleet/project.toml", "[store\n");
    match store::project_policy(&dir.root) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.starts_with(&format!(
                "{} does not parse as TOML: ",
                dir.path(".fleet/project.toml").display()
            )),
            "{why}"
        ),
        other => panic!("wanted Unreadable, got {other:?}"),
    }
}
