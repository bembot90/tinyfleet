//! The item verbs' `--json` document, through the shipped binary (cli PRD § The
//! JSON envelope; MVP path row 2).
//!
//! Five verbs are driven here — `dispatch`, `deliver`, `review`, `ask` and
//! `answer`. `land` is the sixth and rides `ring_land.rs`, whose three
//! repositories are the only rig that can give it a remote to move.
//!
//! WHAT EACH ARM ASSERTS IS A FIELD ONLY ITS OWN VERB CAN KNOW: the seat a
//! dispatch named, the commit a delivery made, the gate an ask raised and an
//! answer resolved. A document carrying the right keys and another verb's
//! values would pass an arm that only counted them.
//!
//! One repository, work graph and machine directory per arm, the way
//! `tests/deliver.rs` builds them: an arm's subject is the state of a working
//! tree. AN ARM DRIVES EVERY VERB ITS RIG CAN, because a rig's cost is its bd
//! calls and they queue on the run's one board: the dispatch arm carries the
//! rendering claims on the calls it makes anyway, the review arm reads the
//! commit off the delivery it needs first, and the usage arm owns no rig at all.
//! Every seat name here is prefixed `ij-`, because "which item does this seat
//! hold" is a query across the whole of the run's shared board.
//!
//! Every rc is read from the child's own status and never off anything it
//! printed.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const REVIEWER: &str = "ij-a-reviewer";
const BY: &str = "an-architect";
const POLICY: &str = "[gates]\nsuite = \"make check\"\n\n\
                      [core]\nreviewer = \"ij-a-reviewer\"\n\n\
                      [controller]\nnudge_model = \"a-cheap-model\"\n\
                      nudge_timeout_seconds = 20\n";

/// The note a delivery hands in, in the delivery-note grammar.
const NOTE: &str = "\
DELIVERED <sha> — <seat>
commit:  <pending>
branch:  <pending>
base:    <pending>
files:   the-work.txt
gate:    AC1 green, each rc read from its own command
suite:   the workspace suite, rc 0
spec corrections: none
not proven: what this arm did not run
decisions: 1
  D1 the note is the seat's; not taken: composing it here; because the words are the seat's
covers: none
";

/// The question an ask hands in, in the question grammar.
const QUESTION: &str = "\
QUESTION the value the spec names is not on the record — where does it come from?
A. read it off the item, as the spec assumes
B. take it from the pack instead
";

/// AC3's fixture: what `dispatch` prints on stdout with no `--json`, captured by
/// running the shipped binary at origin/main afaf9360e.
///
/// `dispatch` is the verb this arm reads because its rendering carries no value
/// the rig varies — the note is the pack's own text with the dispatcher's name
/// in it — so the fixture is bytes rather than a template with the ids put back.
const TRUNK_DISPATCH_STDOUT: &str = "dispatched by an-architect — orders given\n";

fn defaults_into(machine: &Path) -> PathBuf {
    let root = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&root).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
    root
}

fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Stdout as the ONE document it is under `--json`, and the assertion that
/// there is nothing else on it: the flag's whole promise to a caller is that it
/// can parse the stream rather than search it.
fn one_document(out: &Output) -> serde_json::Value {
    let text = stdout(out);
    let mut lines = text.lines();
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("stdout carries a document: {text:?}"));
    assert_eq!(lines.next(), None, "and nothing else on stdout: {text:?}");
    serde_json::from_str(first).unwrap_or_else(|e| panic!("the document parses: {e}: {first}"))
}

/// The document's `data`, with the envelope's own two fields read first.
fn data_of(out: &Output, verb: &str) -> serde_json::Value {
    let document = one_document(out);
    assert_eq!(document["ok"], serde_json::json!(true), "{document}");
    assert_eq!(document["verb"], serde_json::json!(verb), "{document}");
    document["data"].clone()
}

/// The document's refusal, the same way.
fn refusal_of(out: &Output, verb: &str) -> serde_json::Value {
    let document = one_document(out);
    assert_eq!(document["ok"], serde_json::json!(false), "{document}");
    assert_eq!(document["verb"], serde_json::json!(verb), "{document}");
    document["refusal"].clone()
}

/// One arm: its own repository, work graph, machine directory and stub.
struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    /// The reviewer's checkout, which a delivery's ring looks for a session in.
    worktree: PathBuf,
    /// The dispatch target's checkout, for the same reason.
    target_worktree: PathBuf,
    /// A second target's checkout: a seat holds one item, so a rig that
    /// dispatches twice names two seats.
    other_worktree: PathBuf,
    stub: PathBuf,
    roster: PathBuf,
    note: PathBuf,
    question: PathBuf,
    /// The seat that delivers and asks. One per arm: the store is the run's
    /// shared board and "which item does this seat hold" reads all of it.
    seat: String,
    /// The seat a dispatch names, which is never the one holding the work.
    target: String,
    /// The seat the rig's second dispatch names.
    other_target: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-item-json-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("a-project");
        let machine = root.join("machine");
        let worktree = root.join("worktree");
        let target_worktree = root.join("target-worktree");
        let other_worktree = root.join("other-worktree");
        for dir in [
            &project,
            &machine,
            &worktree,
            &target_worktree,
            &other_worktree,
            &root.join("hooks"),
        ] {
            std::fs::create_dir_all(dir).expect("the fixture directory is created");
        }
        defaults_into(&machine);

        let rig = Rig {
            stub: root.join("agent.sh"),
            roster: root.join("roster.json"),
            note: root.join("note.md"),
            question: root.join("question.md"),
            seat: format!("ij-a-builder-{label}"),
            target: format!("ij-a-target-{label}"),
            other_target: format!("ij-another-target-{label}"),
            root,
            project,
            machine,
            worktree,
            target_worktree,
            other_worktree,
        };
        std::fs::write(rig.project.join("fleet.toml"), POLICY).expect("the policy is written");
        common::take_a_board(&rig.project, "item-json");
        std::fs::write(&rig.note, NOTE).expect("the delivery note is written");
        std::fs::write(&rig.question, QUESTION).expect("the question is written");
        std::fs::write(
            rig.machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"name": "{REVIEWER}", "chosen_name": "Kite",
                      "worktrees": {{"a-project": {worktree}}}}},
                     {{"name": "{target}", "chosen_name": "Pell",
                      "worktrees": {{"a-project": {target_worktree}}}}},
                     {{"name": "{other_target}", "chosen_name": "Orla",
                      "worktrees": {{"a-project": {other_worktree}}}}}
                   ]}}"#,
                target = rig.target,
                other_target = rig.other_target,
                fleet_toml = json_string(&rig.project.join("fleet.toml").display().to_string()),
                worktree = json_string(&rig.worktree.display().to_string()),
                target_worktree = json_string(&rig.target_worktree.display().to_string()),
                other_worktree = json_string(&rig.other_worktree.display().to_string()),
            ),
        )
        .expect("the machine config is written");
        rig.write_stub();
        rig.roster("[]");
        rig
    }

    /// The repository as a delivery and an ask find it: the policy committed on
    /// the trunk, a trunk ref to record a base from, and a work branch with one
    /// file staged.
    fn init_repo(&self) {
        self.git(&[
            "config",
            "core.hooksPath",
            &self.root.join("hooks").display().to_string(),
        ]);
        self.git(&["add", "--", "fleet.toml", ".beads"]);
        self.git(&["commit", "--quiet", "--no-gpg-sign", "-m", "the policy"]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        self.git(&["checkout", "--quiet", "-b", "ij-a-seat/feat/the-work"]);
        std::fs::write(self.project.join("the-work.txt"), "the work\n")
            .expect("the work is written");
        self.git(&["add", "--", "the-work.txt"]);
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn bd(&self, args: &[&str]) -> Output {
        Command::new("bd")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .output()
            .expect("bd runs")
    }

    /// One item nobody has been given yet: what a dispatch takes.
    fn a_ready_item(&self) -> String {
        let out = self.bd(&[
            "create",
            "--title",
            "an item the SDK will drive",
            "--description",
            "a scratch item",
            "--type",
            "task",
            "--json",
        ]);
        assert!(
            out.status.success(),
            "bd create: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let value: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("bd create answers JSON");
        value["id"].as_str().expect("an id").to_string()
    }

    /// The same item, held by this arm's seat under a standing order: what
    /// `deliver` and `ask` read to find the work they are acting on.
    fn an_item_ordered_to_the_seat(&self) -> String {
        let item = self.a_ready_item();
        assert!(self
            .bd(&[
                "update",
                &item,
                "--assignee",
                &self.seat,
                "--metadata",
                &format!(
                    r#"{{"orders": {{"by": "{BY}", "kind": "dispatch", "seat": "{seat}", "at": "2026-09-18T00:00:00Z"}}}}"#,
                    seat = self.seat
                ),
                "--actor",
                BY,
            ])
            .status
            .success());
        item
    }

    /// The record half and the working tree together.
    fn an_ordered_item(&self) -> String {
        let item = self.an_item_ordered_to_the_seat();
        self.init_repo();
        item
    }

    fn item_json(&self, item: &str) -> serde_json::Value {
        let out = self.bd(&["-q", "show", item, "--json"]);
        let value: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("bd show answers JSON");
        value[0].clone()
    }

    fn notes_of(&self, item: &str) -> String {
        self.item_json(item)["notes"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    /// The stub: `agents` is the roster read, `-p` is the one print-mode turn.
    fn write_stub(&self) {
        std::fs::write(
            &self.stub,
            format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 \x20 agents) /bin/cat '{roster}' ;;\n\
                 \x20 -p) /bin/cat > /dev/null ;;\n\
                 \x20 *) exit 64 ;;\n\
                 esac\n",
                roster = self.roster.display(),
            ),
        )
        .expect("the stub is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&self.stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
    }

    fn roster(&self, body: &str) -> &Rig {
        std::fs::write(&self.roster, body).expect("the roster is written");
        self
    }

    /// A roster carrying a live row in each of the three checkouts, so a ring
    /// at any seat is delivered.
    fn live(&self) -> &Rig {
        self.roster(&format!(
            r#"[{{"sessionId": "abcdef", "id": "s0", "cwd": {reviewer}, "pid": 4242}},
                {{"sessionId": "beefed", "id": "s1", "cwd": {target}, "pid": 4243}},
                {{"sessionId": "cafe00", "id": "s2", "cwd": {other}, "pid": 4244}}]"#,
            reviewer = json_string(&self.worktree.display().to_string()),
            target = json_string(&self.target_worktree.display().to_string()),
            other = json_string(&self.other_worktree.display().to_string()),
        ))
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args)
            .env("BEADS_ACTOR", &self.seat)
            .output()
            .expect("the built binary runs")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fleet"));
        command
            .args(args)
            .arg("--packs-dir")
            .arg(self.machine.join("packs"))
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub))
            // The identity the delivery's and the ask's own commits are made
            // under. Named here because `HOME` is the rig's.
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid");
        command
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// ---- AC1: one arm per verb, each reading a field only that verb knows; AC3,
// ---- the human rendering byte for byte against the trunk's, rides dispatch --

/// `dispatch --json`: the seat the order named, which no other verb's document
/// carries — and on the same call the human rendering moved to stderr BYTE FOR
/// BYTE, with stdout the document alone. A second item, dispatched without the
/// flag, puts the trunk's bytes on stdout: that call names no `--json`, which
/// is how the fixture was taken rather than derived from the code that prints
/// it.
///
/// The pair is what makes the fixture load-bearing: a flag that SUPPRESSED the
/// rendering rather than moving it would pass the plain call and lose a person
/// the page. Two items and two seats, because an item carries one order and a
/// seat holds one item.
#[test]
fn dispatch_prints_the_seat_it_named_and_the_trunks_bytes_on_the_stream_the_flag_chooses() {
    let rig = Rig::new("dispatch");
    rig.live();

    let item = rig.a_ready_item();
    let out = rig.run(&["dispatch", &item, "--to", &rig.target, "--by", BY]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        TRUNK_DISPATCH_STDOUT,
        "without the flag, the trunk's bytes on stdout, unchanged"
    );

    let item = rig.a_ready_item();
    let out = rig.run(&[
        "dispatch",
        &item,
        "--to",
        &rig.other_target,
        "--by",
        BY,
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        TRUNK_DISPATCH_STDOUT,
        "under it, the same bytes on the other stream"
    );

    let data = data_of(&out, "dispatch");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(data["state"], serde_json::json!("dispatched"), "{data}");
    assert_eq!(
        data["seat"],
        serde_json::json!(rig.other_target),
        "the seat the order named: {data}"
    );
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(rig.other_target),
        "and it is the seat the record now holds"
    );
}

/// `deliver --json`: the commit the delivery made, which only this verb
/// produces. Then `review --json` on that delivery: the state the verdict moved
/// the item to — and the `--show` that moves it nowhere, whose state is null
/// rather than a fourth word.
#[test]
fn deliver_prints_the_commit_it_made_and_review_the_state_its_verdict_moved_the_item_to() {
    let rig = Rig::new("review");
    rig.live();
    let item = rig.an_ordered_item();

    let out = rig.run(&[
        "deliver",
        "--note",
        &rig.note.display().to_string(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    // The commit read out of the repository, not out of what the verb said.
    let head = rig.git(&["rev-parse", "HEAD"]);
    assert_eq!(head.len(), 40, "a commit is 40 hex: {head}");

    let data = data_of(&out, "deliver");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(data["state"], serde_json::json!("delivered"), "{data}");
    assert_eq!(
        data["commit"],
        serde_json::json!(head),
        "the commit the delivery made: {data}"
    );

    let out = rig.run(&["review", &item, "--by", REVIEWER, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "review");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert!(
        data["state"].is_null(),
        "a --show writes no verdict and moves the item nowhere: {data}"
    );
    assert!(
        stderr(&out).contains("size: 1 file(s), +1, -0"),
        "and the page a person reads is on the other stream: {}",
        stderr(&out)
    );

    let out = rig.run(&["review", &item, "--land", "--by", REVIEWER, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "review");
    assert_eq!(data["state"], serde_json::json!("reviewed"), "{data}");
    assert!(
        rig.notes_of(&item).contains("ACCEPTED"),
        "and the verdict is on the record: {}",
        rig.notes_of(&item)
    );
}

/// `ask --json` and `answer --json`: the gate id, which only these two carry,
/// and which the second reads back from the first.
#[test]
fn ask_and_answer_print_the_gate_one_raised_and_the_other_resolved() {
    let rig = Rig::new("gate");
    rig.live();
    let item = rig.an_ordered_item();

    let out = rig.run(&[
        "ask",
        "--note",
        &rig.question.display().to_string(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "ask");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(data["state"], serde_json::json!("parked"), "{data}");
    let gate = data["gate"].as_str().expect("a gate id").to_string();
    assert!(!gate.is_empty(), "the gate the ask raised: {data}");
    assert!(
        rig.notes_of(&item).contains(&gate),
        "which the park on the record names too: {}",
        rig.notes_of(&item)
    );

    let out = rig.run(&["answer", &item, "B", "--by", "a-person", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "answer");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(data["state"], serde_json::json!("resolved"), "{data}");
    assert_eq!(
        data["gate"],
        serde_json::json!(gate),
        "the same gate the ask raised: {data}"
    );
}

// ---- AC2: one refusal arm per verb ------------------------------------------

/// The refusal shape and the unchanged exit code, on the stop each verb reaches
/// through core: the item is not there, the seat holds nothing, the record
/// carries no delivery and no park.
///
/// Every one of these is exit 1 on the trunk and exit 1 here — the flag prints a
/// document, it does not reclassify an outcome.
#[test]
fn a_refused_verb_prints_the_refusal_shape_and_the_exit_code_it_always_had() {
    let rig = Rig::new("refused");
    rig.live();
    // An item nobody delivered and nobody parked, held by nobody: each verb
    // below stops on its own first gate.
    let item = rig.a_ready_item();
    let question = rig.question.display().to_string();
    let note = rig.note.display().to_string();

    let calls: [(&str, Vec<&str>); 5] = [
        (
            "dispatch",
            vec![
                "dispatch",
                "no-such-item-0",
                "--to",
                &rig.target,
                "--by",
                BY,
            ],
        ),
        ("deliver", vec!["deliver", "--note", &note]),
        ("review", vec!["review", &item, "--by", REVIEWER]),
        ("ask", vec!["ask", "--note", &question]),
        ("answer", vec!["answer", &item, "A", "--by", "a-person"]),
    ];

    for (verb, args) in calls {
        let plain = rig.run(&args);
        assert_eq!(
            plain.status.code(),
            Some(1),
            "{verb} refuses at 1 without the flag: {}",
            stderr(&plain)
        );

        let mut with_flag = args.clone();
        with_flag.push("--json");
        let out = rig.run(&with_flag);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{verb} refuses at the same 1 with it: {}",
            stderr(&out)
        );

        let refusal = refusal_of(&out, verb);
        assert_eq!(
            refusal["code"],
            serde_json::json!("refused"),
            "{verb}: the exit table's row by name, not its number"
        );
        let why = refusal["why"].as_str().expect("a why").to_string();
        assert!(!why.is_empty(), "{verb}: the refusal says why");
        assert!(
            stderr(&out).contains(&why),
            "{verb}: and says the same sentence on stderr: {}",
            stderr(&out)
        );
    }
}

/// The usage class, which is refused BEFORE any work: no `--by` and no actor in
/// the environment is exit 2, and the document says `usage` where the one above
/// says `refused`.
///
/// The control the arm above needs: a `code` that never varied would read as
/// correct in both.
///
/// NO RIG: the refusal is each verb's first act, before it resolves a machine
/// directory or reads a board, so the binary runs over `hermetic_nowhere` from
/// the temp directory, and the note paths it is handed are never opened. A verb
/// that reached for a board first would find none there and answer a different
/// exit.
#[test]
fn a_missing_by_is_the_usage_row_before_the_verb_does_anything() {
    let calls: [(&str, Vec<&str>); 5] = [
        ("dispatch", vec!["dispatch", "no-such-item-0"]),
        ("deliver", vec!["deliver", "--note", "no-such-note.md"]),
        ("review", vec!["review", "no-such-item-0"]),
        ("ask", vec!["ask", "--note", "no-such-question.md"]),
        ("answer", vec!["answer", "no-such-item-0", "A"]),
    ];

    for (verb, args) in calls {
        let mut with_flag = args.clone();
        with_flag.push("--json");
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(&with_flag)
            .current_dir(std::env::temp_dir())
            .hermetic_nowhere()
            .env_remove("BEADS_ACTOR")
            .env_remove("FLEET_ACTOR")
            .output()
            .expect("the built binary runs");
        assert_eq!(
            out.status.code(),
            Some(2),
            "{verb} is usage: {}",
            stderr(&out)
        );
        let refusal = refusal_of(&out, verb);
        assert_eq!(refusal["code"], serde_json::json!("usage"), "{verb}");
        assert!(
            refusal["why"]
                .as_str()
                .unwrap_or_default()
                .contains("--by <name>"),
            "{verb}: the refusal names the flag: {refusal}"
        );
    }
}
