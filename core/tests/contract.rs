//! The contract both stores answer, asked of each of them.
//!
//! The arms of every other suite here run against the board held in memory, so
//! what that board answers has to be what `bd` answers — and the only way to
//! say that is to ask the same question of both and compare the answers to the
//! same expectation. The questions are the library's own checks,
//! [`fleet_core::store::conformance`], which `fleet store check` asks of an
//! adapter; this file is only their runner. Each check has an arm of its own
//! against [`Board`]'s store, two arms ask the whole table of `bd`: once on a
//! board a rig made, and once on the scratch store the adapter makes itself —
//! and one asks it of the same board held in memory behind the contract's JSON,
//! the stub, through `Exec`.
//!
//! WHY THE REAL HALF IS ONE ARM AND NOT TWENTY. A nextest arm is its own
//! process, so a shared store is shared only with itself and every arm that
//! wants `bd` pays a `bd init` — 3.5 s on an idle box, three times that under
//! the run's own parallelism. Twenty arms would buy twenty inits and the same
//! twenty readings.

mod common;

use std::path::{Path, PathBuf};

use common::board::note_bd_init;
use common::{a_delivery, seat_actor, shared_store, Scratch};
use fleet_core::entry::{Body, Entry};
use fleet_core::seat::actor::Actor;
use fleet_core::store::bd::Bd;
use fleet_core::store::conformance::{self, AnotherWriter, Ctx, Passed, CHECKS};
use fleet_core::store::exec::Exec;
use fleet_core::store::types::{Capabilities, Priorities, Vocabulary};
use fleet_core::store::{
    Filter, HoldId, Item, ItemId, ItemSummary, NewItem, Order, RunRecord, Store, StoreError,
    Update, Version,
};
use fleet_core::test_support::{stub_path, Board, FakeStore};

/// The ids bd 1.3.0 minted on a scratch board, in the order it minted them,
/// which the board held in memory files under in place of its own `fx-<n>`.
///
/// ITS OWN CANNOT BE MADE AMBIGUOUS. Every prefix of a number is another
/// number the board already holds — `1` is `fx-1` wherever `fx-10` and
/// `fx-11` are — so a fragment two of its items open with always resolves to
/// a third, and the ambiguity check has nothing to ask.
const MINTED: [&str; 30] = [
    "fx-bvx", "fx-0li", "fx-byb", "fx-17w", "fx-oby", "fx-dup", "fx-am9", "fx-m97", "fx-avr",
    "fx-1jw", "fx-9va", "fx-5du", "fx-ws8", "fx-vjo", "fx-tnc", "fx-2e0", "fx-0yc", "fx-bzs",
    "fx-mn4", "fx-l21", "fx-8en", "fx-9vb", "fx-1ve", "fx-vs4", "fx-ttz", "fx-cyc", "fx-2am",
    "fx-sm2", "fx-5pq", "fx-zui",
];

/// One check against a fresh board held in memory, which files under
/// [`MINTED`]; the store that is not there is one whose every read answers
/// Unreadable, and another writer's keys are planted through the board's rig.
///
/// A skip is a red here: every check is asked of this board.
fn in_memory(name: &str) {
    let check = CHECKS
        .iter()
        .find(|(named, _)| *named == name)
        .map(|(_, check)| *check)
        .unwrap_or_else(|| panic!("`{name}` is a check on the table"));
    let label: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let board = Board::new(&format!("contract-{label}"));
    *board
        .store
        .creates
        .lock()
        .expect("the queue is not poisoned") = MINTED.iter().map(|id| id.to_string()).collect();
    let absent = FakeStore {
        unreadable: Some(String::from("the board held in memory is not there")),
        ..FakeStore::default()
    };
    let plant = |item: &str, payload: &str| {
        board.set_metadata(item, payload);
        Ok(())
    };
    let ctx = Ctx {
        store: &board.store,
        root: &board.root,
        absent: &absent,
        another_writer: Some(&plant),
    };
    match check(&ctx) {
        Ok(Passed::Pass) => {}
        Ok(Passed::Skip(why)) => {
            panic!("{name} — the board held in memory: skipped, and it is asked every check: {why}")
        }
        Err(why) => panic!("{name} — the board held in memory: {why}"),
    }
}

/// Every check against one `bd` store, in the table's order, and one red naming
/// each check that did not hold — a skip among them, as every check is asked
/// of `bd`.
fn every_check_holds(store: &Bd, root: &Path, which: &str, plant: &AnotherWriter) {
    let nowhere = Gone::empty("contract-absent");
    let absent = Bd::at(&nowhere.0);
    let ctx = Ctx {
        store,
        root,
        absent: &absent,
        another_writer: Some(plant),
    };
    let failed: Vec<String> = conformance::run(&ctx)
        .filter_map(|(name, answer)| match answer {
            Ok(Passed::Pass) => None,
            Ok(Passed::Skip(why)) => Some(format!("{name}: skipped — {why}")),
            Err(why) => Some(format!("{name}: {why}")),
        })
        .collect();
    assert!(
        failed.is_empty(),
        "{which} — {} of the {} checks did not hold:\n{}",
        failed.len(),
        CHECKS.len(),
        failed.join("\n")
    );
}

/// Another writer's metadata onto an item on a real board, through the binary,
/// under an actor that is no fleet actor.
fn planted_on(root: &Path) -> impl Fn(&str, &str) -> Result<(), String> + '_ {
    move |item: &str, payload: &str| {
        let out = std::process::Command::new("bd")
            .arg("-C")
            .arg(root)
            .args([
                "update",
                item,
                "--metadata",
                payload,
                "--actor",
                "another-tool",
            ])
            .output()
            .map_err(|e| format!("bd could not be run: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
}

/// A directory under the system temp directory, removed when this is dropped —
/// a red included.
struct Gone(PathBuf);

impl Gone {
    /// A name of its own, and nothing there yet.
    fn named(label: &str) -> Gone {
        let dir = std::env::temp_dir().join(format!("fleet-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Gone(dir)
    }

    /// The same, made and left empty.
    fn empty(label: &str) -> Gone {
        let gone = Gone::named(label);
        std::fs::create_dir_all(&gone.0).expect("the empty directory is made");
        gone
    }
}

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- one arm per check, on the board held in memory ------------------------

#[test]
fn a_fresh_store_lists_nothing_ready_nothing_labelled_and_no_open_hold() {
    in_memory("empty listings");
}

#[test]
fn version_names_the_store_and_its_version() {
    in_memory("version");
}

#[test]
fn a_declared_export_validates() {
    in_memory("capabilities");
}

/// The capabilities check holds what a store declares about its items to the
/// contract: a store declaring no type, or a range upside down, fails it by
/// name, and the store declaring its own types in range passes.
#[test]
fn declared_items_with_no_type_or_a_range_upside_down_fail_the_capabilities_check() {
    let check = CHECKS
        .iter()
        .find(|(named, _)| *named == "capabilities")
        .map(|(_, check)| *check)
        .expect("capabilities is a check on the table");
    let absent = FakeStore::default();
    let root = Gone::empty("contract-declared-items");
    let asked = |types: &[&str], min: u8, max: u8| {
        let store = FakeStore {
            vocabulary: Vocabulary {
                types: types.iter().map(|word| word.to_string()).collect(),
                priority: Priorities { min, max },
            },
            ..FakeStore::default()
        };
        check(&Ctx {
            store: &store,
            root: &root.0,
            absent: &absent,
            another_writer: None,
        })
    };
    assert_eq!(
        asked(&[], 0, 4),
        Err(String::from(
            "what the store declares does not validate: items types is empty — a store takes \
             at least one"
        ))
    );
    assert_eq!(
        asked(&["story"], 3, 1),
        Err(String::from(
            "what the store declares does not validate: items priority min 3 is above max 1"
        ))
    );
    assert_eq!(asked(&["story"], 1, 3), Ok(Passed::Pass));
}

#[test]
fn a_create_answers_an_id_the_next_read_answers_the_new_items_fields_for() {
    in_memory("create then show");
}

#[test]
fn a_fragment_resolves_to_the_full_id_and_reads_as_the_item() {
    in_memory("resolve by fragment");
}

#[test]
fn an_id_nobody_filed_is_refused_and_not_unreadable() {
    in_memory("missing is refused");
}

#[test]
fn a_fragment_naming_two_items_is_refused_naming_both() {
    in_memory("ambiguous is refused with candidates");
}

#[test]
fn a_store_that_is_not_there_is_unreadable_and_never_refused() {
    in_memory("unreadable is could not tell");
}

#[test]
fn a_hold_takes_an_item_out_of_the_ready_set_its_clear_brings_it_back_and_a_close_takes_it_out() {
    in_memory("ready");
}

#[test]
fn a_labels_listing_answers_its_open_items() {
    in_memory("label filter");
}

#[test]
fn a_seats_listing_answers_its_items_closed_ones_included() {
    in_memory("assignee filter");
}

#[test]
fn update_title_and_assignee_move_what_show_answers() {
    in_memory("update");
}

#[test]
fn an_order_write_replaces_the_order_and_keeps_the_run_record() {
    in_memory("an order write keeps the run record");
}

#[test]
fn a_run_write_keeps_the_order() {
    in_memory("a run write keeps the order");
}

#[test]
fn order_withdraw_clears_the_assignee_and_the_order_together() {
    in_memory("order.withdraw clears both");
}

#[test]
fn entries_appended_are_the_timelines_last_in_the_order_they_were_appended() {
    in_memory("timeline is append-only and ordered");
}

#[test]
fn a_hold_is_open_until_it_is_cleared_and_a_second_clear_is_refused() {
    in_memory("holds");
}

#[test]
fn a_close_moves_the_status_and_a_second_close_is_refused() {
    in_memory("close");
}

#[test]
fn an_export_writes_the_file_and_its_bytes_move_when_an_item_does() {
    in_memory("export");
}

#[test]
fn an_order_set_is_the_order_the_next_read_answers() {
    in_memory("order.set reads back");
}

#[test]
fn an_update_naming_nothing_is_unreadable_and_moves_nothing() {
    in_memory("update naming nothing");
}

#[test]
fn a_fenced_write_lands_only_while_the_holder_it_names_holds_the_item() {
    in_memory("fenced writes");
}

#[test]
fn an_update_fenced_on_a_holder_lands_only_while_that_holder_holds_the_item() {
    in_memory("fenced update refuses moved");
}

#[test]
fn an_update_to_open_reopens_the_item_and_any_other_status_is_usage() {
    in_memory("reopen through update");
}

#[test]
fn a_fenced_withdrawal_reopens_in_the_same_act_or_is_moved_with_nothing_written() {
    in_memory("fenced withdraw with reopen");
}

#[test]
fn another_writers_keys_are_neither_read_nor_moved_by_fleets_writes() {
    in_memory("another writer's keys");
}

#[test]
fn another_writers_keys_are_named_foreign_and_fleets_own_never_are() {
    in_memory("another writer's keys are listed as foreign");
}

// ---- the real half ----------------------------------------------------------

/// THE OTHER HALF: every check, against the store `bd` answers.
///
/// ON A BOARD OF ITS OWN, never a copy of the run's shared one: the first
/// check lists a store nothing has written to, and the shared board holds
/// every other rig's rows. A check that reds here and greens in its own arm
/// above is the in-memory board having drifted from the real store, which is
/// the whole reason this file exists.
#[test]
fn every_check_holds_against_bd_too() {
    let scratch = Scratch::fresh("contract-checks");
    let bd = Bd::at(&scratch.root);
    // The file the export check reads is the one bd declares, and bd declares
    // its own.
    let declared = bd
        .capabilities()
        .expect("bd's capabilities read")
        .export
        .expect("bd declares an export");
    assert_eq!(declared.file, ".beads/issues.jsonl");
    assert_eq!(declared.file, fleet_core::store::bd::EXPORT);
    every_check_holds(&bd, &scratch.root, "bd", &planted_on(&scratch.root));
}

/// THE ADAPTER'S OWN SCRATCH: `bd init` in a directory of the caller's, on
/// bd's embedded engine, answered as that directory — and every check holds on
/// a store over it, which is what `fleet store check` will ask of it.
///
/// That init is not one of the rigs', so it is counted here, where
/// `fleet/tools/dolt-test-server` reads its run's count.
#[test]
fn the_bd_adapter_makes_a_scratch_store_every_check_holds_on() {
    let dir = Gone::named("contract-scratch");
    let adapter = Bd::at(&dir.0);
    assert!(
        adapter
            .capabilities()
            .expect("bd's capabilities read")
            .scratch,
        "bd declares a scratch store"
    );
    note_bd_init("contract-scratch");
    let root = adapter
        .scratch(&dir.0)
        .unwrap_or_else(|e| panic!("bd makes a scratch store in {}: {e}", dir.0.display()));
    assert_eq!(root, dir.0, "the answer is the directory it was handed");
    assert!(
        root.join(".beads").is_dir(),
        "and the store is in it: {}",
        root.display()
    );
    every_check_holds(
        &Bd::at(&root),
        &root,
        "bd's own scratch",
        &planted_on(&root),
    );
}

/// THE THIRD HALF: every check against the stub, through `Exec` — the board
/// held in memory answering the contract's JSON, one process per call, as an
/// adapter out of process answers it. The store is the stub's own scratch,
/// asked for through `Exec` as `fleet store check` asks; the store that is not
/// there is the stub at a root holding none.
///
/// TWO SKIPS AND NO MORE: the two checks on another writer's keys. The
/// contract has no verb that plants another tool's keys, so this run hands no
/// other writer in, as `fleet store check` hands none, and every other check is
/// asked of the stub.
#[test]
fn every_check_holds_against_the_stub_through_exec() {
    let stub = stub_path();
    assert_eq!(
        stub.canonicalize().expect("the stub is there"),
        Path::new(env!("CARGO_BIN_EXE_fleet-store-stub"))
            .canonicalize()
            .expect("cargo built the stub"),
        "stub_path names the executable cargo built for this crate's tests"
    );
    let dir = Gone::named("contract-stub");
    let root = Exec::at(&stub, &dir.0)
        .scratch(&dir.0)
        .unwrap_or_else(|e| panic!("the stub makes a scratch store in {}: {e}", dir.0.display()));
    assert_eq!(root, dir.0, "the answer is the directory it was handed");
    let nowhere = Gone::empty("contract-stub-absent");
    let store = Exec::at(&stub, &root);
    let absent = Exec::at(&stub, &nowhere.0);
    let ctx = Ctx {
        store: &store,
        root: &root,
        absent: &absent,
        another_writer: None,
    };
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    for (name, answer) in conformance::run(&ctx) {
        match answer {
            Ok(Passed::Pass) => {}
            Ok(Passed::Skip(_)) => skipped.push(name),
            Err(why) => failed.push(format!("{name}: {why}")),
        }
    }
    assert!(
        failed.is_empty(),
        "the stub — {} of the {} checks did not hold:\n{}",
        failed.len(),
        CHECKS.len(),
        failed.join("\n")
    );
    assert_eq!(
        skipped,
        [
            "another writer's keys",
            "another writer's keys are listed as foreign"
        ],
        "only the checks that plant another writer's keys are skipped"
    );
}

/// The store held in memory declares a scratch store and answers the directory
/// it was handed, a store over which is a fresh one of its own.
#[test]
fn the_board_held_in_memory_declares_a_scratch_and_answers_the_directory() {
    let dir = Gone::named("contract-fake-scratch");
    let store = FakeStore::default();
    assert!(store.capabilities().expect("they read").scratch);
    assert_eq!(store.scratch(&dir.0), Ok(dir.0.clone()));
}

/// The board held in memory names the ids whose hash OPENS WITH a fragment, as
/// the contract reads one and bd 1.3.0 answered on a scratch board that minted
/// these ids: `3` is `fx-37v` alone though `fx-h35` and `fx-pz3` hold a 3,
/// `35` inside `fx-h35` and `z` inside `fx-6az` and `fx-pz3` open no hash and
/// name nothing, and `0` opens two.
#[test]
fn the_board_held_in_memory_names_the_ids_whose_hash_opens_with_a_fragment() {
    let board = Board::new("contract-fragment-prefix");
    let minted = ["fx-37v", "fx-h35", "fx-pz3", "fx-6az", "fx-01o", "fx-0wf"];
    *board
        .store
        .creates
        .lock()
        .expect("the queue is not poisoned") = minted.iter().map(|id| id.to_string()).collect();
    for id in minted {
        assert_eq!(board.item(&format!("the item minted as {id}")), id);
    }

    assert_eq!(board.store.resolve("3"), Ok(ItemId::from("fx-37v")));
    for inside in ["35", "z"] {
        match board.store.resolve(inside) {
            Err(StoreError::Refused(why)) => assert!(
                !why.contains("more than one item"),
                "`{inside}` opens no hash and is missing, not ambiguous: {why}"
            ),
            answer => panic!("`{inside}` opens no hash and is Refused, not {answer:?}"),
        }
    }
    match board.store.resolve("0") {
        Err(StoreError::Refused(why)) => assert!(
            why.contains("fx-01o") && why.contains("fx-0wf"),
            "`0` opens both and the refusal names them: {why}"
        ),
        answer => panic!("`0` opens two hashes and is Refused, not {answer:?}"),
    }
}

/// A store that declares no scratch is asked for one anyway: the trait's own
/// answer, Unreadable, with nothing made.
#[test]
fn a_store_declaring_no_scratch_is_unreadable_when_asked_for_one() {
    let dir = Gone::named("contract-no-scratch");
    let store = NoScratch(FakeStore::default());
    assert!(!store.capabilities().expect("they read").scratch);
    assert_eq!(
        store.scratch(&dir.0),
        Err(StoreError::Unreadable(String::from(
            "this store declares no scratch"
        )))
    );
    assert!(!dir.0.exists(), "and nothing is made");
}

/// The board held in memory with the scratch taken away: every verb its own,
/// the capability undeclared and `scratch` left to the trait's default.
struct NoScratch(FakeStore);

impl Store for NoScratch {
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        self.0.show(item)
    }
    fn resolve(&self, id: &str) -> Result<ItemId, StoreError> {
        self.0.resolve(id)
    }
    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError> {
        self.0.list(filter)
    }
    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError> {
        self.0.create(item, by)
    }
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError> {
        self.0.update(id, change, by)
    }
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError> {
        self.0.order_set(id, order, by)
    }
    fn order_withdraw(
        &self,
        id: &ItemId,
        fence: &fleet_core::store::WithdrawFence,
        by: &Actor,
    ) -> Result<(), StoreError> {
        self.0.order_withdraw(id, fence, by)
    }
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        self.0.run_set(id, run, by)
    }
    fn hold_raise(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<HoldId, StoreError> {
        self.0.hold_raise(id, reason, by)
    }
    fn hold_clear(&self, hold: &HoldId, by: &Actor) -> Result<(), StoreError> {
        self.0.hold_clear(hold, by)
    }
    fn holds_open(&self) -> Result<Vec<HoldId>, StoreError> {
        self.0.holds_open()
    }
    fn close(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<(), StoreError> {
        self.0.close(id, reason, by)
    }
    fn append(&self, item: &ItemId, body: &Body, by: &Actor) -> Result<String, StoreError> {
        self.0.append(item, body, by)
    }
    fn timeline(&self, item: &ItemId) -> Result<Vec<Entry>, StoreError> {
        self.0.timeline(item)
    }
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        Ok(Capabilities {
            scratch: false,
            ..self.0.capabilities()?
        })
    }
    fn version(&self) -> Result<Version, StoreError> {
        self.0.version()
    }
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError> {
        self.0.export(into)
    }
}

/// bd names itself `bd`, and its version is a whole token of the first line
/// `bd --version` prints — asked of the binary beside it, so the arm holds on
/// whatever bd this box has.
#[test]
fn bds_version_is_named_on_the_first_line_it_prints() {
    let scratch = shared_store("contract");
    let answered = Bd::at(&scratch.root).version().expect("bd's version reads");
    let printed = scratch.bd(&["--version"]);
    assert!(printed.status.success(), "bd --version runs");
    let first = String::from_utf8_lossy(&printed.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    assert!(!first.is_empty(), "bd --version prints a line");
    assert_eq!(answered.name, "bd");
    assert!(
        first
            .split_whitespace()
            .any(|token| token.strip_prefix('v').unwrap_or(token) == answered.version),
        "{:?} is a token of `{first}`",
        answered.version
    );
}

/// The JSON a call to the binary answered, opened out of its envelope where it
/// carries one.
fn answered(out: &std::process::Output, what: &str) -> serde_json::Value {
    assert!(
        out.status.success(),
        "{what}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer = fleet_core::store::first_value(&String::from_utf8_lossy(&out.stdout))
        .unwrap_or_else(|| panic!("{what} answers JSON"));
    fleet_core::store::bd::opened(answer, String::new)
}

/// A COMMENT A PERSON WROTE ON THE BOARD IS NOT AN ENTRY, and one carrying
/// fleet's key that does not read is never skipped. Both are planted through
/// the binary, because the author is bd's own field: `--actor` with no
/// `<kind>:` is what a person's own `bd comments add` writes.
#[test]
fn a_persons_comment_is_left_out_and_a_malformed_entry_refuses_the_read() {
    let scratch = shared_store("contract");
    let bd = Bd::at(&scratch.root);
    let item = scratch.item("an item a person commented on");

    answered(
        &scratch.bd(&["comments", "add", &item, "a person's words", "--json"]),
        "bd comments add",
    );
    assert_eq!(
        bd.timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "a person's comment is not an entry"
    );

    let planted = answered(
        &scratch.bd(&[
            "comments",
            "add",
            &item,
            r#"{"fleet.entry":1,"kind":"ordered","order":"dispatch"}"#,
            "--actor",
            "Alberto Vildosola",
            "--json",
        ]),
        "bd comments add",
    );
    let comment = planted["id"].as_str().expect("the comment has an id");
    match bd.timeline(&ItemId::from(item.as_str())) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.contains(comment) && why.contains("Alberto Vildosola"),
            "the refusal names the comment and its author: {why}"
        ),
        other => panic!("an entry whose author is no actor refuses the read: {other:?}"),
    }
}

/// An entry that breaks its kind's rules is refused BEFORE the binary is asked,
/// so nothing is written and the item's comments are what they were.
#[test]
fn an_entry_that_does_not_validate_is_refused_and_nothing_is_written() {
    let scratch = shared_store("contract");
    let bd = Bd::at(&scratch.root);
    let item = scratch.item("an item a short sha is appended to");
    let comments = || answered(&scratch.bd(&["comments", &item, "--json"]), "bd comments");
    let before = comments();

    match bd.append(
        &ItemId::from(item.as_str()),
        &a_delivery("1111111"),
        &seat_actor("a-short-seat"),
    ) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.contains(&item) && why.contains("nothing was written"),
            "the refusal names the item and says nothing was written: {why}"
        ),
        other => panic!("a delivery naming a 7-character commit is refused: {other:?}"),
    }
    assert_eq!(comments(), before, "and bd lists nothing new");
}

/// Every check on the table has an arm of its own above, and every arm names a
/// check on the table: a check added to the library and left without an arm,
/// or an arm naming no check, is a red here rather than a silence.
#[test]
fn the_table_names_every_check() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract.rs");
    let arms = std::fs::read_to_string(&here).expect("this file is readable");
    let mut armed: Vec<&str> = arms
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("in_memory(\""))
        .filter_map(|rest| rest.split_once("\");").map(|(name, _)| name))
        .collect();
    armed.sort_unstable();
    let mut table: Vec<&str> = CHECKS.iter().map(|(name, _)| *name).collect();
    table.sort_unstable();
    assert_eq!(
        armed, table,
        "every check on the table has an arm of its own, and every arm a check"
    );
}
