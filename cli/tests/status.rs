//! `fleet status` through the shipped binary.
//!
//! THE FIXTURE IS THE CONTROLLER'S OWN TYPE, rendered by the controller's own
//! writer: a projection hand-written as JSON here would pin this page against a
//! shape of its own, and the one defect this verb cannot survive is reading a
//! document the controller does not write.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::projection::{
    self, EffectsView, InFlight, PolicyView, Projection, SeatRow, SeatView,
};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A policy with two `[[core.flight.rules]]` and a threshold no default shares,
/// so a page printing the compiled default instead of this file is a red.
const POLICY_WITH_RULES: &str = "\
[controller]
rest_threshold_tokens = 1000

[[core.flight.rules]]
match = { type = \"task\", labels = [\"flight\"] }
review = \"none\"

[[core.flight.rules]]
review = \"required\"
";

/// The same file with the array gone, for the no-rules line.
const POLICY_WITHOUT_RULES: &str = "\
[controller]
rest_threshold_tokens = 1000
";

struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
}

impl Rig {
    /// A machine directory and a project, with no store: the page reads the
    /// projection and the policy and never the work graph.
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-status-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            project: root.join("project"),
            machine: root.join("machine"),
            root,
        };
        std::fs::create_dir_all(&rig.machine).expect("the machine directory is made");
        std::fs::create_dir_all(&rig.project).expect("the project directory is made");
        std::fs::write(rig.project.join("fleet.toml"), POLICY_WITH_RULES)
            .expect("the project's own file is written");
        rig.policy(POLICY_WITH_RULES);
        rig
    }

    /// The policy file the projection's `fleet.path` names.
    fn policy_file(&self) -> PathBuf {
        self.machine.join("fleet.toml")
    }

    fn policy(&self, body: &str) {
        std::fs::write(self.policy_file(), body).expect("the policy is written");
    }

    fn projection_file(&self) -> PathBuf {
        self.machine.join("projection.json")
    }

    /// The machine's seat list, one row per `(id, name)`: what `--seat`
    /// resolves its argument through.
    fn list(&self, seats: &[(&str, &str)]) {
        let rows: Vec<serde_json::Value> = seats
            .iter()
            .map(|(id, name)| {
                serde_json::json!({
                    "id": id,
                    "name": name,
                    "worktrees": { "demo": format!("/wt/{name}") },
                })
            })
            .collect();
        let body = serde_json::json!({
            "fleet_toml": self.policy_file().display().to_string(),
            "children": rows,
        });
        std::fs::write(self.machine.join("config.json"), body.to_string())
            .expect("the seat list is written");
    }

    /// The document, written by the controller's own renderer.
    fn publish(&self, document: &Projection) -> String {
        let body = projection::render(document).expect("the projection renders");
        std::fs::write(self.projection_file(), &body).expect("the projection is published");
        body
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the built binary runs")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fleet"));
        command
            .args(args)
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        command
    }

    /// The machine directory as a listing plus a checksum per file, which is
    /// what AC2's read-only claim is proved against.
    fn fingerprint(&self) -> Vec<(String, u64)> {
        let mut rows = Vec::new();
        walk(&self.machine, &self.machine, &mut rows);
        rows.sort();
        rows
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn walk(base: &Path, dir: &Path, rows: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            walk(base, &path, rows);
            continue;
        }
        let name = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .display()
            .to_string();
        let bytes = std::fs::read(&path).unwrap_or_default();
        rows.push((name, checksum(&bytes)));
    }
}

/// FNV-1a over the file's bytes: a checksum and not a timestamp, so a rewrite
/// with the same content is not read as a change and a changed byte is.
fn checksum(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A nameless agent seat's object, keyed by `id`.
fn view(id: &str) -> SeatView {
    SeatView {
        id: id.to_string(),
        name: None,
        kind: "agent".to_string(),
    }
}

/// A seat row with everything null that a quiet seat publishes as null.
fn seat(id: &str) -> SeatRow {
    SeatRow {
        seat: view(id),
        roster_state: "present".to_string(),
        roster_unknown_cause: None,
        waiting_for: None,
        roster_recency_fallback: None,
        context_tokens: None,
        project: Some("demo".to_string()),
        worktree: Some(format!("/wt/{id}")),
        decision: "leave-alone".to_string(),
        outcome: "none".to_string(),
        blind: 0,
        halted: false,
    }
}

/// The document, with the policy path and the stamp the caller chose.
fn document(policy_file: &Path, generated_at: &str, seats: Vec<SeatRow>) -> Projection {
    Projection {
        version: projection::VERSION,
        generated_at: generated_at.to_string(),
        controller_version: "9.9.9".to_string(),
        agent_version: Some("2.1.261".to_string()),
        agent_version_expected: Some("2.1.261".to_string()),
        fleet: PolicyView {
            path: policy_file.display().to_string(),
            mtime: None,
            poll_seconds: 5,
            claude_code: None,
            plugin_dir: None,
        },
        fleet_parse_error: None,
        in_flight: None,
        effects: EffectsView::on(),
        grant: "ok".to_string(),
        grant_detail: None,
        seats,
        orders: Vec::new(),
    }
}

/// AC1, the whole page off a FRESH projection: every section in order.
#[test]
fn the_page_prints_every_section() {
    let rig = Rig::new("page");

    let mut busy = seat_named(BUILDER_1, "Orla");
    busy.context_tokens = Some(250);
    let mut held = seat(BUILDER_2);
    held.roster_state = "prompt-blocked".to_string();
    held.waiting_for = Some("a permission dialog".to_string());
    held.halted = true;
    held.blind = 3;

    let mut doc = document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![busy, held],
    );
    doc.in_flight = Some(InFlight {
        seat: busy_view(),
        effect: "dispatch".to_string(),
    });
    rig.publish(&doc);

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);

    // (a) the first line: fresh, so no STALE, and both versions beside it.
    let first = page.lines().next().expect("the page has a first line");
    assert!(first.contains("controller 9.9.9"), "{first}");
    assert!(first.contains("agent 2.1.261"), "{first}");
    assert!(first.contains("s old"), "{first}");
    assert!(
        !first.contains("STALE"),
        "a fresh projection is not stale: {first}"
    );

    // (b) the grant is answered, so no banner.
    assert!(!page.contains("GRANT PENDING"), "{page}");

    // (c) the roster, with the halt mark and the blind count. A seat is named
    // by its machine name [ASSUMES D13]: Orla's line starts with hers, and a
    // nameless seat's with its kind's.
    let roster: Vec<&str> = page
        .lines()
        .skip_while(|line| *line != "roster")
        .skip(1)
        .take(2)
        .collect();
    assert!(
        roster[0].starts_with("  orla-e8a04b17  present"),
        "the roster line starts orla-<short>: {roster:?}"
    );
    assert!(
        roster[1].starts_with("  agent-1d0e4f58  prompt-blocked"),
        "{roster:?}"
    );
    assert!(
        !page.contains(BUILDER_1),
        "no human line spells the id: {page}"
    );
    assert!(
        page.contains("decision leave-alone, outcome none"),
        "{page}"
    );
    assert!(page.contains("worktree /wt/Orla"), "{page}");
    assert!(
        page.contains("prompt-blocked, waiting for a permission dialog"),
        "{page}"
    );
    assert!(page.contains("HALTED, 3 blind dispatch(es)"), "{page}");

    // (d) in_flight and effects, one line each.
    assert!(
        page.contains("\nin flight  orla-e8a04b17 — dispatch\n"),
        "{page}"
    );
    assert!(page.contains("\neffects  on\n"), "{page}");

    // (e) the context section, against the FILE's threshold and not a default.
    assert!(
        page.contains("context  (rest threshold 1000 tokens)"),
        "{page}"
    );
    assert!(
        page.contains("\n  orla-e8a04b17  250 tokens, 25% of the threshold, 750 left\n"),
        "the context row is keyed on the machine name: {page}"
    );
    assert!(
        page.contains("\n  agent-1d0e4f58  —\n"),
        "a seat with no reading: {page}"
    );

    // (f) the rules table, both rules.
    assert!(page.contains("[[core.flight.rules]]"), "{page}");
    assert!(
        page.contains("type task and labels [flight] → review=none"),
        "{page}"
    );
    assert!(page.contains("every item → review=required"), "{page}");
    assert!(!page.contains("no rules are set"), "{page}");

    // (g) the routines, as the document carries them.
    assert!(
        page.contains("\nroutines\n  no routine is loaded\n"),
        "{page}"
    );
}

/// AC1's other half: a STALE projection, a pending grant and a policy naming
/// no rules.
#[test]
fn a_stale_projection_a_pending_grant_and_no_rules() {
    let rig = Rig::new("stale");
    rig.policy(POLICY_WITHOUT_RULES);

    let mut doc = document(
        &rig.policy_file(),
        "2026-01-01T00:00:00Z",
        vec![seat("builder-1")],
    );
    doc.grant = "pending".to_string();
    doc.grant_detail = Some("a listing nobody has answered".to_string());
    doc.agent_version = Some("2.1.260".to_string());
    rig.publish(&doc);

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);

    let first = page.lines().next().expect("the page has a first line");
    assert!(first.contains("STALE,"), "{first}");
    assert!(
        first.contains("s old"),
        "the age is printed with it: {first}"
    );
    assert!(
        first.contains("agent 2.1.260 (expected 2.1.261)"),
        "a spread names both: {first}"
    );

    assert!(
        page.contains("GRANT PENDING — a listing nobody has answered"),
        "{page}"
    );
    assert!(page.contains("no rules are set"), "{page}");
}

/// Two seats' ids, keying their rows on the seat list and in the projection.
const BUILDER_1: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const BUILDER_2: &str = "01a0d1f1-0aec-765f-9abe-2b7c1d0e4f58";

/// A row naming the seat by its id and its own name, as the loop publishes
/// it.
fn seat_named(id: &str, name: &str) -> SeatRow {
    let mut row = seat(id);
    row.seat.name = Some(name.to_string());
    row.worktree = Some(format!("/wt/{name}"));
    row
}

/// The object Orla's row carries, for the in-flight line to name.
fn busy_view() -> SeatView {
    seat_named(BUILDER_1, "Orla").seat
}

/// AC2, the flags: `--json` is the file's bytes and nothing else; `--seat`
/// is one seat's two rows; an unknown seat is 1; both flags together are 2.
#[test]
fn the_two_flags_print_what_they_name_and_refuse_what_they_cannot() {
    let rig = Rig::new("flags");
    rig.list(&[(BUILDER_1, "builder-1"), (BUILDER_2, "builder-2")]);
    let mut row = seat_named(BUILDER_1, "builder-1");
    row.context_tokens = Some(500);
    let bytes = rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![row, seat_named(BUILDER_2, "builder-2")],
    ));

    let json = rig.run(&["status", "--json"]);
    assert_eq!(json.status.code(), Some(0), "{}", stderr(&json));
    assert_eq!(
        String::from_utf8_lossy(&json.stdout),
        bytes,
        "the document goes out byte for byte"
    );

    let one = rig.run(&["status", "--seat", "builder-1"]);
    assert_eq!(one.status.code(), Some(0), "{}", stderr(&one));
    let one_page = stdout(&one);
    let rows: Vec<&str> = one_page.lines().map(str::trim_end).collect();
    assert_eq!(rows.len(), 2, "two rows and nothing else: {rows:?}");
    assert!(rows[0].starts_with("builder-1-e8a04b17  "), "{rows:?}");
    assert!(
        rows[1].contains("500 tokens, 50% of the threshold, 500 left"),
        "{rows:?}"
    );
    assert!(
        !one_page.contains("builder-2"),
        "and no other seat's row: {one_page}"
    );

    let unknown = rig.run(&["status", "--seat", "builder-9"]);
    assert_eq!(unknown.status.code(), Some(1), "{}", stderr(&unknown));
    assert!(
        stderr(&unknown).contains("builder-9"),
        "{}",
        stderr(&unknown)
    );

    let both = rig.run(&["status", "--seat", "builder-1", "--json"]);
    assert_eq!(both.status.code(), Some(2), "{}", stderr(&both));
    assert!(both.stdout.is_empty(), "usage goes to stderr, never stdout");
    assert!(stderr(&both).contains("Usage:"), "{}", stderr(&both));
}

/// `--seat` resolves its argument through the seat list, as every seat argument
/// does: a name matches WITHOUT REGARD TO CASE, and the row printed is the one
/// the projection keys by the id it resolved to.
#[test]
fn status_seat_resolves_its_argument_and_matches_a_name_in_any_case() {
    let rig = Rig::new("seat-any-case");
    rig.list(&[(BUILDER_1, "Orla"), (BUILDER_2, "Kite")]);
    let mut orla = seat_named(BUILDER_1, "Orla");
    orla.context_tokens = Some(500);
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![orla, seat_named(BUILDER_2, "Kite")],
    ));

    let out = rig.run(&["status", "--seat", "ORLA"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);
    let rows: Vec<&str> = page.lines().map(str::trim_end).collect();
    assert_eq!(rows.len(), 2, "Orla's two rows and nothing else: {rows:?}");
    assert!(rows[0].starts_with("orla-e8a04b17  "), "{rows:?}");
    assert!(
        rows[1].contains("500 tokens, 50% of the threshold, 500 left"),
        "{rows:?}"
    );
    assert!(!page.contains("Kite"), "and no other seat's row: {page}");

    // The same seat by its machine name and by its short id.
    for arg in ["orla-e8a04b17", "e8a04b17"] {
        let by = rig.run(&["status", "--seat", arg]);
        assert_eq!(by.status.code(), Some(0), "{arg}: {}", stderr(&by));
        assert!(
            stdout(&by).starts_with("orla-e8a04b17  "),
            "{arg}: {}",
            stdout(&by)
        );
    }

    // A seat the list carries and the projection does not is refused naming
    // its machine name.
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat_named(BUILDER_2, "Kite")],
    ));
    let missing = rig.run(&["status", "--seat", "orla"]);
    assert_eq!(missing.status.code(), Some(1), "{}", stderr(&missing));
    assert!(
        stderr(&missing).contains("the projection carries no row for `orla-e8a04b17`"),
        "{}",
        stderr(&missing)
    );
}

/// AC2, the refusals: no projection is 5 with nothing on stdout, an
/// unparseable one is 3 naming the file, and a version this binary does not
/// read is the same 3 rather than a half-read page.
#[test]
fn the_three_refusals_carry_their_own_exits_and_name_their_file() {
    let rig = Rig::new("refusals");

    let missing = rig.run(&["status"]);
    assert_eq!(missing.status.code(), Some(5), "{}", stderr(&missing));
    assert!(
        missing.stdout.is_empty(),
        "nothing is printed: {}",
        stdout(&missing)
    );
    assert!(
        stderr(&missing).contains("fleet start"),
        "the refusal names the verb that fixes it: {}",
        stderr(&missing)
    );

    std::fs::write(rig.projection_file(), "{ not json at all").expect("the fixture is written");
    let torn = rig.run(&["status"]);
    assert_eq!(torn.status.code(), Some(3), "{}", stderr(&torn));
    assert!(
        stderr(&torn).contains(&rig.projection_file().display().to_string()),
        "{}",
        stderr(&torn)
    );

    let mut ahead = document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        Vec::new(),
    );
    ahead.version = projection::VERSION + 1;
    rig.publish(&ahead);
    let unknown = rig.run(&["status"]);
    assert_eq!(unknown.status.code(), Some(3), "{}", stderr(&unknown));
    assert!(
        stderr(&unknown).contains(&format!("version {}", projection::VERSION + 1)),
        "{}",
        stderr(&unknown)
    );
}

/// AC2's read-only claim, PROVED: a listing of the machine directory with a
/// checksum per file, taken before and after every shape of run this verb has,
/// and compared whole.
#[test]
fn every_run_leaves_the_machine_directory_byte_identical() {
    let rig = Rig::new("readonly");
    std::fs::write(rig.machine.join("a-file"), "before\n").expect("the control's file is written");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));

    let before = rig.fingerprint();
    assert!(
        before.len() >= 3,
        "the instrument sees the machine directory's files: {before:?}"
    );

    for args in [
        vec!["status"],
        vec!["status", "--json"],
        vec!["status", "--seat", "builder-1"],
        vec!["status", "--seat", "builder-9"],
    ] {
        let out = rig.run(&args);
        assert!(
            out.status.code().is_some(),
            "{args:?} ran to completion: {}",
            stderr(&out)
        );
        assert_eq!(
            rig.fingerprint(),
            before,
            "{args:?} changed the machine directory"
        );
    }

    // The control: the instrument above CAN see a change, so the four equal
    // readings are the verb's doing and not a fingerprint that answers the
    // same whatever the directory holds.
    std::fs::write(rig.machine.join("a-file"), "after\n").expect("the control's file is rewritten");
    assert_ne!(
        rig.fingerprint(),
        before,
        "the fingerprint reads a changed byte"
    );
}

// ---- the runs section --------------------------------------------------------

/// The stamp a line older than the failure window carries.
const LONG_AGO: &str = "2026-01-01T00:00:00Z";

/// Lines onto the rig's stream in the controller's own write shape, each with
/// the stamp the arm chose — which the controller's appender, stamping the wall
/// clock, cannot give a line meant to sit outside the failure window.
fn stream(rig: &Rig, lines: &[(String, &str, serde_json::Value)]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(rig.machine.join("events.jsonl"))
        .expect("the stream is opened");
    for (n, (ts, kind, payload)) in lines.iter().enumerate() {
        let seq = n as u64 + 1;
        let line = serde_json::to_string(&fleet_controller::events::Event {
            id: format!("ev-{seq}"),
            seq,
            ts: ts.clone(),
            kind,
            actor: &fleet_controller::events::ActorRef::seat("lead-1"),
            payload: payload.clone(),
        })
        .expect("the line serializes");
        writeln!(file, "{line}").expect("the line is appended");
    }
}

/// One line of the stream, at `ts`.
fn line(
    ts: &str,
    kind: &'static str,
    payload: serde_json::Value,
) -> (String, &'static str, serde_json::Value) {
    (ts.to_string(), kind, payload)
}

/// `run.started` for one run of `takeoff`, as core's front half writes it.
fn started(ts: &str, run: &str) -> (String, &'static str, serde_json::Value) {
    line(
        ts,
        fleet_core::item::RUN_STARTED,
        serde_json::json!({ "run": run, "hash": "h", "workflow": "takeoff" }),
    )
}

/// `run.could_not_tell` for one run: an execution nothing could classify.
fn crashed(ts: &str, run: &str) -> (String, &'static str, serde_json::Value) {
    line(
        ts,
        fleet_core::item::RUN_COULD_NOT_TELL,
        serde_json::json!({ "run": run, "exit": 7, "read": "nothing to see" }),
    )
}

/// One entry's signal, as every writer of one writes it.
fn signalled(
    ts: &str,
    item: &str,
    entry: &str,
    kind: &str,
) -> (String, &'static str, serde_json::Value) {
    line(
        ts,
        fleet_core::item::ITEM_ENTRY,
        serde_json::json!({ "item": item, "entry": entry, "kind": kind }),
    )
}

/// The page's runs section, from its header to the blank line that ends it.
fn runs_section(page: &str) -> String {
    let from = page
        .find("\nruns  ")
        .unwrap_or_else(|| panic!("the page carries a runs section: {page}"));
    let rest = &page[from + 1..];
    let to = rest.find("\n\n").map(|at| at + 1).unwrap_or(rest.len());
    rest[..to].to_string()
}

/// The one row of the section that names `run`.
fn row_of(section: &str, run: &str) -> String {
    section
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("{run} ")))
        .unwrap_or_else(|| panic!("the section carries a row for {run}: {section}"))
        .to_string()
}

/// AC1: every standing a run can be in, read off the stream the run pass
/// decides on — the failure in the window with its reason, the park with its
/// hold, the could-not-tell with what was read, the wait with its wake, the
/// open run — and the two the section leaves out: the closed run, and the
/// failure before the window, which is counted and not listed.
///
/// THE PARKS ARE THE RECORDS'. Each run whose last line is `run.could_not_tell`
/// is a record on a registered board, and the two held carry the crash cap's
/// park, one of them cleared by hand — so the page says cleared on the one the
/// store no longer lists open, and the stream carries no park line at all.
#[test]
fn the_runs_section_reads_every_standing_off_the_stream() {
    let rig = Rig::new("runs");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    rig.a_registered_board();
    let [crash, park, heard] = [rig.a_run_record(), rig.a_run_record(), rig.a_run_record()];
    let now = fleet_controller::clock::now_stamp();
    let now = now.as_str();
    stream(
        &rig,
        &[
            started(now, "fx-open"),
            started(now, "fx-wait"),
            line(
                now,
                fleet_core::item::RUN_WAITING,
                serde_json::json!({ "run": "fx-wait", "wake": { "for": "a delivery" }, "seq": 2 }),
            ),
            started(now, &crash),
            crashed(now, &crash),
            started(now, &park),
            crashed(now, &park),
            started(now, &heard),
            line(
                now,
                fleet_core::item::RUN_COULD_NOT_TELL,
                serde_json::json!({ "run": heard, "exit": null, "read": null }),
            ),
            started(now, "fx-fail"),
            line(
                now,
                fleet_core::item::RUN_FAILED,
                serde_json::json!({ "run": "fx-fail", "reason": { "why": "fleet land refused" } }),
            ),
            started(LONG_AGO, "fx-stale"),
            line(
                LONG_AGO,
                fleet_core::item::RUN_FAILED,
                serde_json::json!({ "run": "fx-stale", "reason": { "why": "long ago" } }),
            ),
            started(now, "fx-done"),
            line(
                now,
                fleet_core::item::RUN_CLOSED,
                serde_json::json!({ "run": "fx-done" }),
            ),
        ],
    );
    let park_hold = rig.parked_at_the_cap(&park);
    let heard_hold = rig.parked_at_the_cap(&heard);
    let out = rig.bd(&["gate", "resolve", &heard_hold]);
    assert!(out.status.success(), "bd gate resolve: {}", stderr(&out));

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);
    let section = runs_section(&page);

    assert!(
        section.starts_with(
            "runs  1 failed in the last 24 hours, 2 held, 1 could not tell, 1 waiting, 1 open\n"
        ),
        "{section}"
    );
    let failed = row_of(&section, "fx-fail");
    assert!(failed.contains("takeoff  FAILED at "), "{failed}");
    assert!(
        failed.contains(r#"{"why":"fleet land refused"}"#),
        "the reason is the workflow's own: {failed}"
    );
    let parked = row_of(&section, &park);
    assert!(parked.contains("HELD at "), "{parked}");
    assert!(
        parked.contains(&format!("on hold {park_hold} — ")),
        "the park the record carries, on its hold, which the store holds open: {parked}"
    );
    let cleared = row_of(&section, &heard);
    assert!(
        cleared.contains(&format!("on hold {heard_hold}, cleared — ")),
        "a park whose hold was cleared still stands, and says so: {cleared}"
    );
    let crash_run = crash;
    let crash = row_of(&section, &crash_run);
    assert!(crash.contains("could not tell at "), "{crash}");
    assert!(
        crash.contains(r#"exit 7, read "nothing to see""#),
        "{crash}"
    );
    let wait = row_of(&section, "fx-wait");
    assert!(wait.contains("waiting since "), "{wait}");
    assert!(wait.contains(r#"{"for":"a delivery"}"#), "{wait}");
    let open = row_of(&section, "fx-open");
    assert!(open.contains("open since "), "{open}");

    // The order is the order a person answers them in.
    let at = |run: &str| {
        section
            .find(&format!("  {run} "))
            .expect("the row is there")
    };
    assert!(at("fx-fail") < at(&park), "{section}");
    assert!(at(&park) < at(&crash_run), "{section}");
    assert!(at(&crash_run) < at("fx-wait"), "{section}");
    assert!(at("fx-wait") < at("fx-open"), "{section}");

    assert!(
        !section.contains("fx-done"),
        "a closed run is not listed: {section}"
    );
    assert!(
        !section.contains("fx-stale"),
        "a failure before the window is not listed: {section}"
    );
    assert!(
        section.contains("1 earlier failure is not listed"),
        "and it is counted: {section}"
    );

    assert!(page.contains("\nholds  1 open\n"), "{page}");
    let lines =
        std::fs::read_to_string(rig.machine.join("events.jsonl")).expect("the stream is there");
    assert!(
        !lines.contains(fleet_core::item::ITEM_ENTRY),
        "the stream carries no park line at all: {lines}"
    );
}

/// A failure and a wait the SDK printed read as text on the page: the reason as
/// the words the workflow threw, and the wake as the hold it is waiting on.
///
/// THE PAYLOADS ARE THE SDK'S OWN LAST LINES as the back half stores them — a
/// thrown `Error`'s message is a JSON string, and so is the hold id `hold`
/// waits on. The row that printed `{"reason":"…"}` was the wrapper's key
/// stored inside the event's own, and this arm holds the other half of that
/// fix: a reason that is text is printed as the text.
#[test]
fn a_failure_and_a_wait_the_sdk_printed_read_as_text_on_the_page() {
    let rig = Rig::new("runs-text");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    let now = fleet_controller::clock::now_stamp();
    let now = now.as_str();
    stream(
        &rig,
        &[
            started(now, "fx-fail"),
            line(
                now,
                fleet_core::item::RUN_FAILED,
                serde_json::json!({ "run": "fx-fail", "reason": "takeoff: no `items` input" }),
            ),
            started(now, "fx-wait"),
            signalled(now, "fx-wait", "fx-held", "held"),
            line(
                now,
                fleet_core::item::RUN_WAITING,
                serde_json::json!({ "run": "fx-wait", "wake": "fx-hold", "seq": 3 }),
            ),
        ],
    );

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let section = runs_section(&stdout(&out));
    let failed = row_of(&section, "fx-fail");
    assert!(
        failed.ends_with(" — takeoff: no `items` input"),
        "the reason is the text, bare: {failed}"
    );
    let wait = row_of(&section, "fx-wait");
    assert!(wait.ends_with(" for fx-hold"), "{wait}");
}

/// A run held at the crash cap and then cancelled is off the page: it is not
/// listed as held, and the hold the cancel cleared is not counted.
///
/// THE LINES ARE THE ONES THE PARK AND `fleet cancel` WRITE: the held entry's
/// signal, `run.cancelled`, then one cleared entry's signal per hold it
/// cleared. A cancelled run is not asked whether it is parked, so no store is
/// behind this one and the page still reads whole.
#[test]
fn a_cancelled_run_is_neither_listed_held_nor_counted_as_owed_a_clearance() {
    let rig = Rig::new("runs-cancelled");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    let now = fleet_controller::clock::now_stamp();
    let now = now.as_str();
    stream(
        &rig,
        &[
            started(now, "fx-gone"),
            line(
                now,
                fleet_core::item::RUN_COULD_NOT_TELL,
                serde_json::json!({ "run": "fx-gone", "exit": 7, "read": null }),
            ),
            signalled(now, "fx-gone", "fx-held", "held"),
            line(
                now,
                fleet_core::item::RUN_CANCELLED,
                serde_json::json!({ "run": "fx-gone" }),
            ),
            signalled(now, "fx-gone", "fx-cleared", "cleared"),
        ],
    );

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);
    let section = runs_section(&page);
    assert!(
        section.starts_with(
            "runs  0 failed in the last 24 hours, 0 held, 0 could not tell, 0 waiting, 0 open\n"
        ),
        "{section}"
    );
    assert!(
        !section.contains("fx-gone"),
        "a cancelled run is not listed: {section}"
    );
    assert!(page.contains("\nholds  0 open\n"), "{page}");
}

/// AC1, the quiet page: no stream at all is a fleet nobody has run anything on,
/// which is every count at zero and no row — never a could-not-tell.
#[test]
fn a_machine_with_no_stream_prints_every_count_at_zero() {
    let rig = Rig::new("runs-quiet");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        Vec::new(),
    ));

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);
    assert!(
        page.contains(
            "\nruns  0 failed in the last 24 hours, 0 held, 0 could not tell, 0 waiting, 0 open\n\n"
        ),
        "{page}"
    );
    assert!(page.contains("\nholds  0 open\n"), "{page}");
}

/// AC2: a real `fleet run` whose workflow exits 1 — the row a takeoff whose
/// `fleet land` refused ends on — and the page a person reads afterwards names
/// it, with the reason the workflow gave.
///
/// THE RUN IS THE BINARY'S OWN, on a scratch pack whose runtime is a stub this
/// arm writes, as `run.rs`'s rig does: what is measured is that the lines the
/// back half writes are the lines this page reads, and a fixture written here
/// could only measure agreement with itself.
#[test]
fn a_run_that_exits_one_is_on_the_page() {
    const RUNTIME: &str = "fx-runtime";
    let rig = Rig::new("run-fails");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));

    let defaults = rig.machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&defaults).expect("the defaults dir is made");
    fleet_core::embedded::write_all(&defaults).expect("the embedded defaults are written");

    // The runtime, as the one line on stdout its doctor check reads.
    let stubs = rig.root.join("stubs");
    let stub = stubs.join(RUNTIME);
    written(&stub, &format!("#!/bin/sh\necho \"{RUNTIME} 1.2.3\"\n"));
    executable(&stub);

    let scratch = rig.machine.join("packs/scratch");
    let bundler = scratch.join("assets/bundle.sh");
    written(&bundler, "#!/bin/sh\nset -eu\ncp \"$1\" \"$2\"\n");
    executable(&bundler);
    written(
        &scratch.join("workflows/status-fails.sh"),
        "#!/bin/sh\necho '{\"why\":\"fleet land refused\"}'\nexit 1\n",
    );
    written(
        &scratch.join("pack.toml"),
        &format!(
            "[pack]\nname = \"scratch\"\nversion = \"0.1.0\"\nschema = 3\n\
             description = \"a scratch pack\"\n\n[runtime]\nname = \"{RUNTIME}\"\n\
             version = \"1.2.3\"\nbundle = \"sh {} {{entry}} {{bundle}}\"\n\
             run = \"sh {{bundle}}\"\n",
            bundler.display()
        ),
    );
    written(
        &rig.project.join("fleet.toml"),
        "[core.run]\nmax_open = 1000\n",
    );
    common::take_a_board(&rig.project, "status");

    let path = match std::env::var("PATH") {
        Ok(held) => format!("{}:{held}", stubs.display()),
        Err(_) => stubs.display().to_string(),
    };
    let ran = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["run", "status-fails", "--by", "run:lead-1", "--packs-dir"])
        .arg(rig.machine.join("packs"))
        .current_dir(&rig.project)
        .hermetic(&rig.root.join("home"), &rig.machine, None)
        .env("PATH", &path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("the built binary runs");
    assert_eq!(
        ran.status.code(),
        Some(1),
        "the workflow's exit 1 is the verb's: {}",
        stderr(&ran)
    );
    let said = stdout(&ran);
    let run = said
        .lines()
        .next()
        .and_then(|line| line.split_once(" — "))
        .map(|(id, _)| id.to_string())
        .unwrap_or_else(|| panic!("the run prints its id first: {said}"));

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let section = runs_section(&stdout(&out));
    assert!(
        section.starts_with("runs  1 failed in the last 24 hours,"),
        "{section}"
    );
    let row = row_of(&section, &run);
    assert!(row.contains("status-fails  FAILED at "), "{row}");
    assert!(row.contains(r#"{"why":"fleet land refused"}"#), "{row}");
}

// ---- the holds: every registered project's store -----------------------------

/// The question a hold hands in, of the shape `assets/question.schema.json`
/// gives.
const QUESTION: &str = r#"{
  "question": "Ship?",
  "options": [{"letter": "A", "text": "yes"}, {"letter": "B", "text": "no"}]
}"#;

impl Rig {
    /// A board at the project, registered on the rig's machine as `fleet
    /// create --standalone` registers one: the store status asks for the open
    /// holds.
    ///
    /// ITS OWN `bd init` AND NOT THE RUN'S SHARED BOARD: the count is of every
    /// hold the board holds open, so a neighbour's hold would move it.
    fn a_registered_board(&self) {
        common::take_a_board_alone(&self.project, "status-holds");
        self.registered();
    }

    fn registered(&self) {
        fleet_controller::lifecycle::register(&self.machine, &self.project, "demo")
            .expect("the project is registered on the machine");
    }

    fn bd(&self, args: &[&str]) -> Output {
        Command::new("bd")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .output()
            .expect("bd runs")
    }

    /// A run's record on the board: an item carrying the run label, which a
    /// hold parks without touching git.
    fn a_run_record(&self) -> String {
        let out = self.bd(&[
            "create",
            "--title",
            "a run's record",
            "--type",
            "task",
            "--labels",
            fleet_core::item::run::LABEL,
            "--json",
        ]);
        assert!(out.status.success(), "bd create: {}", stderr(&out));
        let value: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("bd create answers JSON");
        value["id"].as_str().expect("an id").to_string()
    }

    /// The run's record held through `fleet hold`, by the run itself, and the
    /// hold it raised.
    fn held_by_its_run(&self, run: &str) -> String {
        let question = self.root.join("question.json");
        std::fs::write(&question, QUESTION).expect("the question is written");
        let by = format!("run:{run}");
        let question = question.display().to_string();
        let out = self.run(&[
            "hold",
            "--question",
            &question,
            "--item",
            run,
            "--by",
            &by,
            "--json",
        ]);
        assert_eq!(out.status.code(), Some(0), "fleet hold: {}", stderr(&out));
        let document: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("hold --json answers a document");
        document["data"]["hold"]
            .as_str()
            .unwrap_or_else(|| panic!("the hold `hold` raised: {document}"))
            .to_string()
    }

    /// The run's record parked at `[core.run] max_crashes`, as the run pass's
    /// hold makes the park — the hold and the `max_crashes` held entry, by the
    /// controller — and the hold it raised. The pass's signal is not written:
    /// the page reads the record.
    fn parked_at_the_cap(&self, run: &str) -> String {
        use fleet_core::seat::actor::{Actor, ActorKind};
        let controller = Actor {
            kind: ActorKind::Controller,
            id: String::from("01a0d1f1-0aec-765f-9abe-00000000c0de"),
        };
        let (hold, _) = fleet_core::item::hold::park_at_the_cap(
            &fleet_core::item::hold::Capped {
                run,
                reason: "executed 3 time(s) and nothing could classify the last one",
                directory: &self.machine.join("runs").join(run),
                by: &controller,
            },
            &fleet_core::store::Bd::at(&self.project),
        )
        .unwrap_or_else(|stop| panic!("the park is made: {}", stop.message));
        hold
    }
}

/// Two runs parked at the crash cap on a registered board, and the FIRST one's
/// hold cleared by hand with bd's own verb — which writes no line on the
/// stream. Each run's own `run.started` and `run.could_not_tell` go on first,
/// so the park lands on a run the fold holds.
///
/// Answers the page and the two `(run, hold)` pairs, the cleared one first.
fn two_parks_one_cleared_by_hand(rig: &Rig) -> (String, [(String, String); 2]) {
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    rig.a_registered_board();
    let runs = [rig.a_run_record(), rig.a_run_record()];
    let now = fleet_controller::clock::now_stamp();
    stream(
        rig,
        &[
            started(&now, &runs[0]),
            crashed(&now, &runs[0]),
            started(&now, &runs[1]),
            crashed(&now, &runs[1]),
        ],
    );
    let parks = runs.map(|run| {
        let hold = rig.parked_at_the_cap(&run);
        (run, hold)
    });

    let out = rig.bd(&["gate", "resolve", &parks[0].1]);
    assert!(out.status.success(), "bd gate resolve: {}", stderr(&out));
    let lines =
        std::fs::read_to_string(rig.machine.join("events.jsonl")).expect("the stream is there");
    assert!(
        !lines.contains(fleet_core::item::ITEM_ENTRY),
        "neither park nor the hand clear wrote a line on the stream: {lines}"
    );

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    (stdout(&out), parks)
}

/// AC1: the hold count is the STORE's open holds, off every project the
/// machine registers — two raised at the cap and one of them cleared by hand
/// with `bd gate resolve` is one open. RED-PROOF: the stream's fold counted
/// two, because the stream never saw the hand clear.
#[test]
fn the_hold_count_is_the_stores_and_sees_a_hold_cleared_by_hand() {
    let rig = Rig::new("holds-count");
    let (page, _) = two_parks_one_cleared_by_hand(&rig);
    assert!(page.contains("\nholds  1 open\n"), "{page}");
}

/// AC3: a parked run whose hold was cleared by hand reads ", cleared", and the
/// one whose hold the store still holds open does not.
#[test]
fn a_parked_run_whose_hold_was_cleared_by_hand_reads_cleared() {
    let rig = Rig::new("holds-row");
    let (page, [(heard, heard_hold), (standing, standing_hold)]) =
        two_parks_one_cleared_by_hand(&rig);
    let section = runs_section(&page);
    assert!(section.contains(", 2 held, "), "{section}");
    let row = row_of(&section, &heard);
    assert!(row.contains("HELD at "), "{row}");
    assert!(
        row.contains(&format!("on hold {heard_hold}, cleared — ")),
        "the hold cleared by hand reads cleared: {row}"
    );
    let row = row_of(&section, &standing);
    assert!(
        row.contains(&format!("on hold {standing_hold} — ")),
        "the hold the store holds open does not: {row}"
    );
}

/// fleet-z4w on the page: a run whose record carries only its OWN ask — raised
/// through `fleet hold` by the run, its held entry signalled on the stream —
/// is not parked, and reads as the could-not-tell it is. The ask's hold is
/// still counted open: it is a hold the store holds.
///
/// RED-PROOF: the page read the stream's park line naming the run as its park,
/// and printed it held on the ask.
#[test]
fn a_run_whose_record_carries_only_its_own_ask_reads_could_not_tell() {
    let rig = Rig::new("holds-ask");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    rig.a_registered_board();
    let run = rig.a_run_record();
    let now = fleet_controller::clock::now_stamp();
    stream(&rig, &[started(&now, &run), crashed(&now, &run)]);
    let ask = rig.held_by_its_run(&run);
    let lines =
        std::fs::read_to_string(rig.machine.join("events.jsonl")).expect("the stream is there");
    assert!(
        lines.contains(&format!("\"item\":\"{run}\"")) && lines.contains("\"kind\":\"held\""),
        "the ask is signalled on the stream: {lines}"
    );

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let page = stdout(&out);
    let section = runs_section(&page);
    assert!(
        section.contains(", 0 held, 1 could not tell, "),
        "{section}"
    );
    let row = row_of(&section, &run);
    assert!(row.contains("could not tell at "), "{row}");
    assert!(!row.contains(&ask), "the ask is no park: {row}");
    assert!(page.contains("\nholds  1 open\n"), "{page}");
}

/// A run at could-not-tell whose record no registered project holds cannot be
/// asked whether it is parked: the runs section says it is not all read,
/// naming the run, stderr says the same, and the exit is could-not-tell, 3 —
/// the page around it still prints.
#[test]
fn a_run_whose_park_cannot_be_read_leaves_the_runs_not_all_read_at_exit_3() {
    let rig = Rig::new("runs-unread");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    let now = fleet_controller::clock::now_stamp();
    stream(&rig, &[started(&now, "fx-lost"), crashed(&now, "fx-lost")]);

    let out = rig.run(&["status"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let page = stdout(&out);
    let section = runs_section(&page);
    let why = "the runs were not all read — whether fx-lost is held could not be read: no \
               project registered with this machine holds a record for fx-lost";
    assert!(section.contains(&format!("\n  {why}\n")), "{section}");
    assert!(
        stderr(&out).contains(&format!("fleet status: {why}")),
        "{}",
        stderr(&out)
    );
    assert!(
        row_of(&section, "fx-lost").contains("could not tell at "),
        "the run is listed where the stream puts it: {section}"
    );
    assert!(page.contains("\nholds  0 open\n"), "{page}");
}

/// AC2: a registered project whose bd cannot run leaves the holds uncounted,
/// named on the page and on stderr, and the exit is could-not-tell, 3 — the
/// page around it still prints.
#[test]
fn a_registered_project_whose_bd_cannot_run_leaves_the_holds_uncounted_at_exit_3() {
    let rig = Rig::new("holds-unread");
    rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![seat("builder-1")],
    ));
    rig.registered();
    let bd = rig.root.join("stubs/bd");
    written(&bd, "#!/bin/sh\necho 'this bd cannot run' >&2\nexit 1\n");
    executable(&bd);

    let out = rig
        .command(&["status"])
        .env("FLEET_BD_BIN", &bd)
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let page = stdout(&out);
    let why = format!(
        "the holds were not counted — {}'s store did not answer: ",
        rig.project.display()
    );
    assert!(
        page.contains(&format!("\nholds  not counted — {why}")),
        "{page}"
    );
    assert!(
        stderr(&out).contains(&format!("fleet status: {why}")),
        "{}",
        stderr(&out)
    );
    assert!(
        page.contains("\nruns  0 failed in the last 24 hours,"),
        "the page around it still prints: {page}"
    );
}

fn written(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent directory is made");
    }
    std::fs::write(path, body).expect("the file is written");
}

fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("the stub is made executable");
}
