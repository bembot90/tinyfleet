//! Adopting a board that was there before fleet: `fleet item list`, the
//! transitional list the check reads through, and the defaults' `adopt-board`
//! doctor check over a fixture board, through the shipped binary.
//!
//! THE FIXTURE IS WHAT A PROJECT BRINGS: an item carrying the board's own bare
//! `orders` and a `reviewer`, a record-like task under a run label and a run
//! key that are not fleet's, an order index at a version this binary does not
//! read beside a `fleet.` key fleet never writes, `fleet:run` on a bug that is
//! no run's record, and a person's comment in fleet's old marker words —
//! beside a plain task, a run record fleet would have filed and an item held
//! against a seat, which the report must NOT name.
//!
//! THE BOARD IS THE STUB'S STATE, planted as the adapter would answer it: the
//! keys another writer keeps are what the store names under `foreign`
//! (fleet-urp), and that name list is what the check reads. An order index
//! this fleet does not read is the adapter's reading, so it is planted as the
//! contract answers one — unreadable — and so is a run's record at a version
//! this fleet does not read. Which of a real store's shapes read so is its
//! pack's to test.
//!
//! A ROW IS THE FIELDS FLEET READS and no raw metadata: the board's own keys
//! are named under `foreign`, and what they hold is in no row. The check counts
//! them by name. And a run's record this binary does not read is no row at all
//! — it refuses the list, and the check could not tell.
//!
//! A STORE OF ITS OWN PER RIG: the check reads the whole ready set and counts
//! it, so a neighbour's rows would move every number.
//!
//! Every rc is read from the child's own status and never off anything it
//! printed.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::hermetic::Hermetic;
use fleet_core::store::{ItemId, NewItem, OrderState, RunRecord, Stamp, Store as _};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const POLICY: &str = "[project]\nitem_prefix = \"fx\"\n";
/// The seat the fixture holds an item against, by its full id: the listing is
/// asked by that id.
const SEAT: &str = "01a0d1f1-0aec-765f-9abe-0000000ad0e7";

fn defaults_into(machine: &Path) {
    let root = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&root).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
}

impl Rig {
    /// A project with its `fleet.toml` and the binary's defaults, and no store
    /// — the file names none, and no pack carries the default's.
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-adopt-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            project: root.join("project"),
            machine: root.join("machine"),
            root,
        };
        std::fs::create_dir_all(&rig.project).expect("the project is made");
        std::fs::create_dir_all(rig.machine.join("packs")).expect("the packs dir is made");
        defaults_into(&rig.machine);
        std::fs::write(rig.project.join("fleet.toml"), POLICY).expect("the policy is written");
        rig
    }

    /// The same, over an empty store of its own on the stub.
    fn with_a_board(label: &str) -> Rig {
        let rig = Rig::new(label);
        common::take_a_store(&rig.project);
        rig
    }

    /// One item filed as a project would have filed it, answered as its id.
    fn filed(&self, title: &str, kind: &str, labels: &[&str]) -> String {
        common::store_at(&self.project)
            .create(
                &NewItem {
                    title: title.to_string(),
                    description: String::from("an item the project brought"),
                    item_type: kind.to_string(),
                    labels: labels.iter().map(|label| label.to_string()).collect(),
                    priority: None,
                },
                &common::the_test(),
            )
            .unwrap_or_else(|e| panic!("`{title}` is filed: {e}"))
            .to_string()
    }

    /// Another writer's keys on `item`, which the store names under `foreign`.
    fn metadata(&self, item: &str, payload: serde_json::Value) {
        common::with_state(&self.project, |store| {
            store.plant_metadata(item, &payload.to_string())
        });
    }

    fn fleet(&self, args: &[&str]) -> Output {
        let fleet = PathBuf::from(env!("CARGO_BIN_EXE_fleet"));
        let fleet_dir = fleet.parent().expect("the binary sits in a directory");
        let mut path = fleet_dir.display().to_string();
        if let Ok(held) = std::env::var("PATH") {
            path = format!("{path}:{held}");
        }
        Command::new(&fleet)
            .args(args)
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env("NO_COLOR", "1")
            .env("PATH", path)
            .output()
            .expect("the built binary runs")
    }

    fn doctor(&self) -> Output {
        let packs = self.machine.join("packs").display().to_string();
        self.fleet(&["doctor", "adopt-board", "--packs-dir", &packs])
    }

    fn listed(&self, args: &[&str]) -> Vec<serde_json::Value> {
        let mut all = vec!["item", "list"];
        all.extend(args);
        all.push("--json");
        let out = self.fleet(&all);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", stderr(&out));
        let document: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("one document");
        assert_eq!(document["ok"], true, "{document}");
        assert_eq!(document["verb"], "item list", "{document}");
        document["data"]["items"]
            .as_array()
            .expect("items is an array")
            .clone()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn ids_of(rows: &[serde_json::Value]) -> Vec<String> {
    let mut ids: Vec<String> = rows
        .iter()
        .map(|row| row["id"].as_str().expect("an id").to_string())
        .collect();
    ids.sort();
    ids
}

fn sorted(ids: &[&String]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    ids.sort();
    ids
}

/// The two-space lines the doctor prints under the check's row.
fn report(said: &str) -> Vec<&str> {
    said.lines()
        .filter_map(|line| line.strip_prefix("  "))
        .collect()
}

/// The report line that starts `adopt-board:` and then `what`, after the
/// row's own indent.
fn line_for<'a>(lines: &[&'a str], what: &str) -> &'a str {
    let found: Vec<&&str> = lines
        .iter()
        .filter(|line| {
            line.strip_prefix("adopt-board:")
                .map(|rest| rest.trim_start().starts_with(what))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(found.len(), 1, "one line for {what:?}: {lines:#?}");
    found[0]
}

/// Whether an id is named on a line as a whole token.
fn names(line: &str, id: &str) -> bool {
    line.split([' ', ',', ';']).any(|token| token == id)
}

// ---- the list ---------------------------------------------------------------

/// A list naming no read is the call's own fault, said before anything is
/// resolved: exit 2 from a directory holding no project at all.
#[test]
fn a_list_naming_no_read_is_usage() {
    let rig = Rig::new("usage");
    let nowhere = rig.root.join("nowhere");
    std::fs::create_dir_all(&nowhere).expect("the directory is made");
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["item", "list", "--json"])
        .current_dir(&nowhere)
        .hermetic(&rig.root.join("home"), &rig.machine, None)
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("one document");
    assert_eq!(document["ok"], false);
    assert_eq!(document["verb"], "item list");
    assert_eq!(document["refusal"]["code"], "usage");
    assert!(
        stderr(&out).starts_with("fleet item list: name what to list: --ready"),
        "{}",
        stderr(&out)
    );
}

// ---- the fixture board ------------------------------------------------------

/// The fixture board, listed and then checked. One rig carries both.
#[test]
fn the_check_names_each_class_on_a_fixture_board_with_its_count_and_ids() {
    let rig = Rig::with_a_board("fixture");

    let plain = rig.filed("a plain task", "task", &[]);
    let bare_orders = rig.filed("ordered the board's own way", "task", &[]);
    rig.metadata(
        &bare_orders,
        serde_json::json!({
            "orders": { "owner": "alice", "kind": "build" },
            "reviewer": "bob",
        }),
    );
    let foreign_run = rig.filed("a run the board's own way", "task", &["takeoff:run"]);
    rig.metadata(
        &foreign_run,
        serde_json::json!({ "takeoff.run": { "started": "monday" } }),
    );
    let odd_version = rig.filed("an order at another version", "task", &[]);
    rig.metadata(&odd_version, serde_json::json!({ "fleet.lane": "b" }));
    common::with_state(&rig.project, |store| {
        store.amend(&odd_version, |item| item.order = OrderState::Unreadable)
    });
    let stray_label = rig.filed("a bug under the run label", "bug", &["fleet:run"]);
    let record = rig.filed("a run fleet filed", "task", &["fleet:run"]);
    common::store_at(&rig.project)
        .run_set(
            &ItemId::from(record.as_str()),
            &a_run_record(),
            &common::the_test(),
        )
        .expect("the record is written");
    let marked = rig.filed("delivered by hand", "task", &[]);
    common::with_state(&rig.project, |store| {
        store.comment(
            &marked,
            "Alberto Vildosola",
            "DELIVERED abc1234 on fx-branch, then ACCEPTED — a person's words",
        )
    });
    let held = rig.filed("held against a seat", "task", &[]);
    common::hand_to(&rig.project, &held, SEAT);

    // The list: each read, and two of them together.
    let ready = rig.listed(&["--ready"]);
    let everything = [
        &plain,
        &bare_orders,
        &foreign_run,
        &odd_version,
        &stray_label,
        &record,
        &marked,
        &held,
    ];
    assert_eq!(ids_of(&ready), sorted(&everything), "the ready set");
    let row = |id: &str| {
        ready
            .iter()
            .find(|row| row["id"] == id)
            .unwrap_or_else(|| panic!("{id} is listed"))
            .clone()
    };
    assert_eq!(row(&bare_orders)["order"], serde_json::Value::Null);
    assert_eq!(
        row(&bare_orders)["foreign"],
        serde_json::json!(["orders", "reviewer"]),
        "the board's own keys, by name"
    );
    for held in ["alice", "bob"] {
        assert!(
            !row(&bare_orders).to_string().contains(held),
            "what the board's own key holds is not in the row: {}",
            row(&bare_orders)
        );
    }
    assert_eq!(
        row(&odd_version)["foreign"],
        serde_json::json!(["fleet.lane"]),
        "a fleet. key fleet never writes is not fleet's, and fleet.orders is"
    );
    assert_eq!(
        row(&foreign_run)["foreign"],
        serde_json::json!(["takeoff.run"])
    );
    for own in [&plain, &record] {
        assert_eq!(
            row(own)["foreign"],
            serde_json::json!([]),
            "fleet's own keys are never foreign: {}",
            row(own)
        );
    }
    assert_eq!(
        row(&odd_version)["order"],
        serde_json::json!({ "unreadable": true })
    );
    assert_eq!(
        row(&record)["run"],
        serde_json::to_value(a_run_record()).expect("a record is JSON"),
        "the record, as it reads"
    );
    assert_eq!(row(&stray_label)["run"], serde_json::Value::Null);
    assert_eq!(row(&stray_label)["type"], "bug");
    for listed in &ready {
        assert!(
            listed.get("metadata").is_none(),
            "no row carries raw metadata: {listed}"
        );
    }
    assert_eq!(
        ids_of(&rig.listed(&["--label", "fleet:run"])),
        sorted(&[&stray_label, &record])
    );
    assert_eq!(ids_of(&rig.listed(&["--assignee", SEAT])), sorted(&[&held]));
    assert_eq!(
        ids_of(&rig.listed(&["--ready", "--label", "takeoff:run"])),
        sorted(&[&foreign_run])
    );

    // The check.
    let out = rig.doctor();
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "{said}{}", stderr(&out));
    let head = said.lines().next().unwrap_or_default();
    assert_eq!(
        head,
        "finding adopt-board (defaults) — adopt-board: found — 4 items to adopt; \
         `fleet item show <id> --json` reads each, and the adopt skill walks the mapping",
        "{said}"
    );
    let lines = report(&said);
    assert!(
        line_for(&lines, "read 8 items through `fleet item list --json`")
            .ends_with("the ready set, and the open items labelled fleet:run"),
        "{said}"
    );
    assert!(
        line_for(&lines, "1. names fleet owns").ends_with("— 2 items"),
        "{said}"
    );
    assert_eq!(
        line_for(&lines, "fleet.orders at a version"),
        format!(
            "adopt-board:    fleet.orders at a version or shape fleet does not read: 1 item — \
             {odd_version}"
        )
    );
    assert!(
        line_for(&lines, "fleet.run at a version")
            .contains("none read — one refuses the list itself"),
        "{said}"
    );
    assert_eq!(
        line_for(&lines, "fleet:run on an item that is not a run record"),
        format!(
            "adopt-board:    fleet:run on an item that is not a run record (a task carrying \
             fleet.run): 1 item — {stray_label}"
        )
    );
    assert_eq!(
        line_for(&lines, "a fleet. key or fleet: label"),
        format!(
            "adopt-board:    a fleet. key or fleet: label fleet never writes: 1 item — \
             {odd_version}; names fleet.lane"
        )
    );
    assert!(
        line_for(&lines, "2. conventions of the board itself").ends_with("— 2 items"),
        "{said}"
    );
    assert_eq!(
        line_for(&lines, "an order-like metadata key"),
        format!(
            "adopt-board:    an order-like metadata key that is not fleet.orders: 1 item — \
             {bare_orders}; keys orders"
        )
    );
    assert_eq!(
        line_for(&lines, "a run-like metadata key"),
        format!(
            "adopt-board:    a run-like metadata key that is not fleet.run: 1 item — \
             {foreign_run}; keys takeoff.run"
        )
    );
    assert_eq!(
        line_for(&lines, "an assignee-like metadata key"),
        format!(
            "adopt-board:    an assignee-like metadata key: 1 item — {bare_orders}; keys reviewer"
        )
    );
    assert_eq!(
        line_for(&lines, "a run label that is not fleet:run"),
        format!(
            "adopt-board:    a run label that is not fleet:run: 1 item — {foreign_run}; labels \
             takeoff:run"
        )
    );
    assert_eq!(
        line_for(&lines, "types on the items read"),
        "adopt-board:    types on the items read, for [[core.flight.rules]] to match: bug 1, task 7"
    );
    let third = line_for(&lines, "3. the marker words fleet once wrote");
    assert!(third.contains("DELIVERED"), "{third}");
    assert!(
        third.contains("not readable through fleet"),
        "the third class is named and not counted: {third}"
    );

    // The controls: nothing fleet wrote, nothing plain, no comment and no
    // assignee is named on any line of the report.
    for control in [&plain, &record, &marked, &held] {
        assert!(
            !lines.iter().any(|line| names(line, control)),
            "{control} is named nowhere: {said}"
        );
    }
}

/// A run record as fleet writes one.
fn a_run_record() -> RunRecord {
    RunRecord {
        hash: String::from("h1"),
        workflow: String::from("greet"),
        pack: String::from("ts"),
        entry: String::from("greet.ts"),
        started_at: Stamp::parse("2026-09-25T00:00:00Z").expect("a stamp"),
    }
}

/// A run's record this binary does not read refuses the list that reaches
/// it, naming the item — so the check could not read the board, and says so
/// with the list's refusal under the row.
#[test]
fn a_run_record_fleet_does_not_read_refuses_the_list_and_the_check_could_not_tell() {
    let rig = Rig::with_a_board("run-version");
    let odd = rig.filed("a run at another version", "task", &["fleet:run"]);
    common::with_state(&rig.project, |store| store.unreadable_run(&odd));

    let out = rig.fleet(&["item", "list", "--label", "fleet:run", "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let named = format!("{odd}'s run record is not one this fleet reads");
    assert!(stderr(&out).contains(&named), "{}", stderr(&out));

    let out = rig.doctor();
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "{said}{}", stderr(&out));
    assert!(
        said.lines().next().unwrap_or_default().starts_with(
            "could not tell adopt-board (defaults) — adopt-board: could not read the board"
        ),
        "{said}"
    );
    assert!(
        report(&said).iter().any(|line| line.contains(&named)),
        "the list's refusal, naming the item, is printed under the row: {said}"
    );
}

// ---- nothing to adopt -------------------------------------------------------

/// An empty board: nothing read, nothing to adopt. Then one plain item: read,
/// and still nothing — the types listing is a reading and never a finding.
#[test]
fn an_empty_board_and_a_plain_one_have_nothing_to_adopt() {
    let rig = Rig::with_a_board("empty");
    let out = rig.doctor();
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        said.lines().next().unwrap_or_default(),
        "pass adopt-board (defaults) — adopt-board: nothing to adopt — no items read",
        "{said}"
    );

    rig.filed("a plain task", "task", &["backend"]);
    let out = rig.doctor();
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{said}{}", stderr(&out));
    assert_eq!(
        said.lines().next().unwrap_or_default(),
        "pass adopt-board (defaults) — adopt-board: nothing to adopt — none of the 1 item read \
         carries a name fleet owns off its schema, or a convention to map",
        "{said}"
    );
}

// ---- could not read ---------------------------------------------------------

/// A project whose file names no store, on a machine where no pack carries
/// the default's adapter: the list cannot be read and names the pack that
/// would carry it, and the check says so and could not tell.
///
/// RED-PROOF: on the base a file naming no store opened the built-in store,
/// and its refusal named no pack.
#[test]
fn a_project_whose_board_cannot_be_read_could_not_tell() {
    let rig = Rig::new("unread");

    let out = rig.fleet(&["item", "list", "--ready", "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("one document");
    assert_eq!(document["refusal"]["code"], "could_not_tell");

    let out = rig.doctor();
    let said = stdout(&out);
    assert_eq!(out.status.code(), Some(3), "{said}{}", stderr(&out));
    assert_eq!(
        said.lines().next().unwrap_or_default(),
        "could not tell adopt-board (defaults) — adopt-board: could not read the board — \
         `fleet item list --ready --json` exited 3, so nothing was scanned",
        "{said}"
    );
    let refusal = format!(
        "fleet item list: no store adapter named `{}` in the installed packs — `{}` installs the \
         one fleet-packs carries",
        fleet_core::store::DEFAULT_ADAPTER,
        fleet_core::store::pack_line(
            fleet_core::supported::PINNED_PACKS_SOURCE,
            fleet_core::store::DEFAULT_ADAPTER,
            fleet_core::supported::PINNED_PACKS
        )
    );
    assert!(
        stderr(&out).contains(&refusal) || report(&said).iter().any(|line| *line == refusal),
        "the list's own refusal is printed under the row: {said}"
    );
}
