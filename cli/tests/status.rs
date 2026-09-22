//! `fleet status` through the shipped binary (cli PRD § `fleet status`).
//!
//! THE FIXTURE IS THE CONTROLLER'S OWN TYPE, rendered by the controller's own
//! writer: a projection hand-written as JSON here would pin this page against a
//! shape of its own, and the one defect this verb cannot survive is reading a
//! document the controller does not write.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::projection::{self, EffectsView, InFlight, PolicyView, Projection, SeatRow};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A policy with two `[[core.flight.rules]]` and a threshold no default shares,
/// so a page printing the compiled default instead of this file is a red.
const POLICY_WITH_RULES: &str = "\
[controller]
rest_threshold_tokens = 1000

[core.flight]
max_open = 1

[[core.flight.rules]]
match = { type = \"task\", labels = [\"flight\"] }
review = \"none\"

[[core.flight.rules]]
gate = \"review\"
";

/// The same file with the array gone, for the no-rules line.
const POLICY_WITHOUT_RULES: &str = "\
[controller]
rest_threshold_tokens = 1000

[core.flight]
max_open = 1
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

    /// The document, written by the controller's own renderer.
    fn publish(&self, document: &Projection) -> String {
        let body = projection::render(document).expect("the projection renders");
        std::fs::write(self.projection_file(), &body).expect("the projection is published");
        body
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("the built binary runs")
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

/// A seat row with everything null that a quiet seat publishes as null.
fn seat(seat_dir: &str) -> SeatRow {
    SeatRow {
        seat_dir: seat_dir.to_string(),
        chosen_name: None,
        roster_state: "present".to_string(),
        roster_unknown_cause: None,
        waiting_for: None,
        roster_recency_fallback: None,
        context_tokens: None,
        project: Some("demo".to_string()),
        worktree: Some(format!("/wt/{seat_dir}")),
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

    let mut busy = seat("builder-1");
    busy.chosen_name = Some("Rook".to_string());
    busy.context_tokens = Some(250);
    let mut held = seat("builder-2");
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
        seat: "builder-1".to_string(),
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

    // (c) the roster, with the halt mark and the blind count.
    assert!(page.contains("builder-1 (Rook)  present"), "{page}");
    assert!(
        page.contains("decision leave-alone, outcome none"),
        "{page}"
    );
    assert!(page.contains("worktree /wt/builder-1"), "{page}");
    assert!(
        page.contains("prompt-blocked, waiting for a permission dialog"),
        "{page}"
    );
    assert!(page.contains("HALTED, 3 blind dispatch(es)"), "{page}");

    // (d) in_flight and effects, one line each.
    assert!(
        page.contains("\nin flight  builder-1 — dispatch\n"),
        "{page}"
    );
    assert!(page.contains("\neffects  on\n"), "{page}");

    // (e) the context section, against the FILE's threshold and not a default.
    assert!(
        page.contains("context  (rest threshold 1000 tokens)"),
        "{page}"
    );
    assert!(
        page.contains("builder-1  250 tokens, 25% of the threshold, 750 left"),
        "{page}"
    );
    assert!(
        page.contains("builder-2  —"),
        "a seat with no reading: {page}"
    );

    // (f) the rules table, both rules.
    assert!(page.contains("[[core.flight.rules]]"), "{page}");
    assert!(
        page.contains("type task and labels [flight] → review=none"),
        "{page}"
    );
    assert!(page.contains("every item → gate=review"), "{page}");
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

/// AC2, the flags: `--json` is the file's bytes and nothing else; `--seat`
/// is one seat's two rows; an unknown seat is 1; both flags together are 2.
#[test]
fn the_two_flags_print_what_they_name_and_refuse_what_they_cannot() {
    let rig = Rig::new("flags");
    let mut row = seat("builder-1");
    row.context_tokens = Some(500);
    let bytes = rig.publish(&document(
        &rig.policy_file(),
        &fleet_controller::clock::now_stamp(),
        vec![row, seat("builder-2")],
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
    assert!(rows[0].starts_with("builder-1"), "{rows:?}");
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
