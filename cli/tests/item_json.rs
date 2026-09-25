//! The item verbs' `--json` document, through the shipped binary (MVP path row
//! 2).
//!
//! Five verbs are driven here — `dispatch`, `deliver`, `review`, `hold` and
//! `clear`. `land` is the sixth and rides `ring_land.rs`, whose three
//! repositories are the only rig that can give it a remote to move. `item
//! show`, the one verb the SDK reads the store through, is here beside them.
//!
//! WHAT EACH ARM ASSERTS IS A FIELD ONLY ITS OWN VERB CAN KNOW: the seat a
//! dispatch named, the commit a delivery made, the hold a `hold` raised and a
//! `clear` cleared. A document carrying the right keys and another verb's
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
/// The reviewer seat's id, which keys its row and its `[seats.<id>]` table.
const REVIEWER_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
/// Who dispatches here: a seat typed whole, which a verb takes as given.
const BY: &str = "seat:01a0d1f1-0aec-765f-9abe-0000a2c417ec";
/// Who clears a hold by hand, typed the same way.
const PERSON: &str = "seat:01a0d1f1-0aec-765f-9abe-00000000fe25";
/// The id of the seat the rig's second dispatch names.
const OTHER_TARGET_ID: &str = "01a0d1f1-0aec-765f-9abe-00007e3fa2c0";
const POLICY: &str = "[core]\nreviewer = \"ij-a-reviewer\"\n\n\
                      [controller]\nnudge_model = \"a-cheap-model\"\n\
                      nudge_timeout_seconds = 20\n";

/// The delivery a seat hands in, as the JSON its brief's schema shows.
const DELIVERY: &str = r#"{
  "files": ["the-work.txt"],
  "checks": [{"check": "AC1", "result": "green, each rc read from its own command"}],
  "suite": {"command": "the workspace suite", "rc": 0},
  "spec_corrections": [],
  "not_proven": [{"surface": "what this arm did not run", "command": "cargo nextest run"}],
  "decisions": [
    {"call": "the delivery is the seat's", "not_taken": "composing it here", "because": "the words are the seat's"}
  ],
  "covers": []
}"#;

/// The question a hold hands in, a JSON file of the shape
/// `assets/question.schema.json` gives.
const QUESTION: &str = r#"{
  "question": "the value the spec names is not on the record — where does it come from?",
  "options": [
    {"letter": "A", "text": "read it off the item, as the spec assumes"},
    {"letter": "B", "text": "take it from the pack instead"}
  ]
}"#;

/// The id the rig's first dispatch target is keyed by.
const TARGET_ID: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";

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
    delivery: PathBuf,
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
            delivery: root.join("delivery.json"),
            question: root.join("question.json"),
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
        // The policy lists the seat that delivers and asks: those verbs find
        // the item a seat holds by resolving its actor among the listed seats.
        // It lists the reviewer too, under its row's id: a delivery goes to the
        // one listed seat `[core] reviewer` names.
        std::fs::write(
            rig.project.join("fleet.toml"),
            format!(
                "{POLICY}{}\n[seats.{REVIEWER_ID}]\nkind = \"agent\"\nname = \"{REVIEWER}\"\n",
                common::seat_table_of(&rig.seat)
            ),
        )
        .expect("the policy is written");
        common::take_a_board(&rig.project, "item-json");
        std::fs::write(&rig.delivery, DELIVERY).expect("the delivery is written");
        std::fs::write(&rig.question, QUESTION).expect("the question is written");
        std::fs::write(
            rig.machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"id": "{REVIEWER_ID}", "name": "{REVIEWER}",
                      "worktrees": {{"a-project": {worktree}}}}},
                     {{"id": "{TARGET_ID}", "name": "{target}",
                      "worktrees": {{"a-project": {target_worktree}}}}},
                     {{"id": "{OTHER_TARGET_ID}", "name": "{other_target}",
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

    /// The repository as a delivery and a hold find it: the policy committed on
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
    /// `deliver` and `hold` read to find the work they are acting on.
    fn an_item_ordered_to_the_seat(&self) -> String {
        let item = self.a_ready_item();
        // Assigned to the seat's id, as every dispatch assigns.
        let seat = common::seat_id_of(&self.seat);
        assert!(self
            .bd(&[
                "update",
                &item,
                "--assignee",
                &seat,
                "--metadata",
                &format!(
                    r#"{{"fleet.orders": {{"v": 1, "by": "{BY}", "kind": "dispatch", "seat": "{seat}", "at": "2026-09-18T00:00:00Z"}}}}"#
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
            .env("FLEET_ACTOR", &self.seat)
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
            // The identity the delivery's and the hold's own commits are made
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

/// `dispatch --json`: the seat the order named and the ordered entry that
/// records it, which no other verb's document carries — and on the same call
/// the human rendering moved to stderr BYTE FOR BYTE, with stdout the document
/// alone. A second item, dispatched without the flag, puts the order line on
/// stdout: the item, the seat by its machine name and the entry `item show`
/// lists, each read off the record rather than off the code that prints them.
///
/// The pair is what makes the line load-bearing: a flag that SUPPRESSED the
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
        rig.order_line(&item, TARGET_ID, &rig.target),
        "without the flag, the order line on stdout"
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
    let line = rig.order_line(&item, OTHER_TARGET_ID, &rig.other_target);
    assert_eq!(
        stderr(&out),
        line,
        "under it, the same line on the other stream"
    );

    // The seat the order named, as its object: the FULL ID its name resolved
    // to — the id is what the record holds — beside the name it was asked by
    // and its kind.
    let data = data_of(&out, "dispatch");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(
        data["state"],
        serde_json::json!("ordered"),
        "the kind of the entry the verb wrote: {data}"
    );
    assert_eq!(
        data["seat"]["id"],
        serde_json::json!(OTHER_TARGET_ID),
        "the seat the order named: {data}"
    );
    assert_eq!(
        data["seat"]["name"],
        serde_json::json!(rig.other_target),
        "{data}"
    );
    assert_eq!(data["seat"]["kind"], serde_json::json!("agent"), "{data}");
    assert!(
        data["entry"]
            .as_str()
            .is_some_and(|entry| line.ends_with(&format!(" — entry {entry}\n"))),
        "the entry the line names: {data}"
    );
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(OTHER_TARGET_ID),
        "and it is the seat the record now holds"
    );
}

/// `deliver --json`: the commit the delivery made, which only this verb
/// produces, and the delivered entry it wrote, by its id. Then `review --json`
/// on that delivery: the entry kind the verdict wrote and which verdict it was
/// — and the `--show` that writes nothing, whose state and verdict are null
/// rather than a fourth word.
#[test]
fn deliver_prints_the_commit_it_made_and_review_the_state_its_verdict_moved_the_item_to() {
    let rig = Rig::new("review");
    rig.live();
    let item = rig.an_ordered_item();

    let out = rig.run(&[
        "deliver",
        "--delivery",
        &rig.delivery.display().to_string(),
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
    // The entry the delivery wrote, by the id `item show` lists it under.
    let shown = rig.item_show(&[&item, "--json"]);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr(&shown));
    let timeline = data_of(&shown, "item show")["timeline"].clone();
    let last = timeline
        .as_array()
        .and_then(|entries| entries.last())
        .expect("the timeline carries an entry");
    assert_eq!(last["kind"], serde_json::json!("delivered"), "{timeline}");
    assert_eq!(last["commit"], serde_json::json!(head), "{timeline}");
    assert!(data["entry"].is_string(), "{data}");
    assert_eq!(
        data["entry"], last["id"],
        "the delivered entry's id: {data}"
    );

    let out = rig.run(&["review", &item, "--by", REVIEWER, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "review");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert!(
        data["state"].is_null() && data["verdict"].is_null() && data["entry"].is_null(),
        "a --show writes no verdict and moves the item nowhere: {data}"
    );
    assert!(
        data.as_object()
            .is_some_and(|data| data.contains_key("verdict")),
        "the key is there, null: {data}"
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
    assert_eq!(data["verdict"], serde_json::json!("accepted"), "{data}");
    // The verdict on the record, by the id `item show` lists it under.
    let shown = rig.item_show(&[&item, "--json"]);
    assert_eq!(shown.status.code(), Some(0), "{}", stderr(&shown));
    let timeline = data_of(&shown, "item show")["timeline"].clone();
    let last = timeline
        .as_array()
        .and_then(|entries| entries.last())
        .expect("the timeline carries an entry");
    assert_eq!(last["kind"], serde_json::json!("reviewed"), "{timeline}");
    assert_eq!(last["verdict"], serde_json::json!("accepted"), "{timeline}");
    assert_eq!(data["entry"], last["id"], "the reviewed entry's id: {data}");
}

/// `hold --json` and `clear --json`: the hold id, which only these two carry,
/// and which the second reads back from the first; and the entry each wrote, by
/// the id `item show` lists it under.
#[test]
fn hold_and_clear_print_the_hold_one_raised_and_the_other_cleared() {
    let rig = Rig::new("hold");
    rig.live();
    let item = rig.an_ordered_item();

    let out = rig.run(&[
        "hold",
        "--question",
        &rig.question.display().to_string(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "hold");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(data["state"], serde_json::json!("held"), "{data}");
    let hold = data["hold"].as_str().expect("a hold id").to_string();
    assert!(!hold.is_empty(), "the hold `hold` raised: {data}");
    let last_entry = |kind: &str| {
        let shown = rig.item_show(&[&item, "--json"]);
        assert_eq!(shown.status.code(), Some(0), "{}", stderr(&shown));
        let timeline = data_of(&shown, "item show")["timeline"].clone();
        let last = timeline
            .as_array()
            .and_then(|entries| entries.last())
            .cloned()
            .expect("the timeline carries an entry");
        assert_eq!(last["kind"], serde_json::json!(kind), "{timeline}");
        assert_eq!(last["hold"], serde_json::json!(hold), "{timeline}");
        last
    };
    let held = last_entry("held");
    assert!(data["entry"].is_string(), "{data}");
    assert_eq!(data["entry"], held["id"], "the held entry's id: {data}");

    let out = rig.run(&["clear", &item, "B", "--by", PERSON, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let data = data_of(&out, "clear");
    assert_eq!(data["item"], serde_json::json!(item), "{data}");
    assert_eq!(data["state"], serde_json::json!("cleared"), "{data}");
    assert_eq!(
        data["hold"],
        serde_json::json!(hold),
        "the same hold `hold` raised: {data}"
    );
    let cleared = last_entry("cleared");
    assert_eq!(cleared["letter"], serde_json::json!("B"), "{cleared}");
    assert!(data["entry"].is_string(), "{data}");
    assert_eq!(
        data["entry"], cleared["id"],
        "the cleared entry's id: {data}"
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
    // An item nobody delivered and nobody held, assigned to nobody: each verb
    // below stops on its own first gate.
    let item = rig.a_ready_item();
    let question = rig.question.display().to_string();
    let delivery = rig.delivery.display().to_string();

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
        ("deliver", vec!["deliver", "--delivery", &delivery]),
        ("review", vec!["review", &item, "--by", REVIEWER]),
        ("hold", vec!["hold", "--question", &question]),
        ("clear", vec!["clear", &item, "A", "--by", PERSON]),
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

/// The usage class, which is refused BEFORE any work: an empty `--by` names no
/// seat at all, which is exit 2, and the document says `usage` where the one
/// above says `refused`. A verb always has an actor otherwise — with no `--by`
/// it is `FLEET_ACTOR` or the machine's identity — so this is the one actor
/// refusal left in the usage row.
///
/// The control the arm above needs: a `code` that never varied would read as
/// correct in both.
#[test]
fn an_empty_by_is_the_usage_row_before_the_verb_writes_anything() {
    let rig = Rig::new("usage");
    let item = rig.a_ready_item();
    let question = rig.question.display().to_string();
    let delivery = rig.delivery.display().to_string();
    let calls: [(&str, Vec<&str>); 5] = [
        ("dispatch", vec!["dispatch", &item, "--to", &rig.target]),
        ("deliver", vec!["deliver", "--delivery", &delivery]),
        ("review", vec!["review", &item]),
        ("hold", vec!["hold", "--question", &question]),
        ("clear", vec!["clear", &item, "A"]),
    ];

    let stream = rig.machine.join("events.jsonl");
    let lines = || {
        std::fs::read_to_string(&stream)
            .map(|body| body.lines().count())
            .unwrap_or(0)
    };
    let before = lines();
    for (verb, args) in calls {
        let mut with_flag = args.clone();
        with_flag.extend(["--by", "", "--json"]);
        let out = rig.run(&with_flag);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{verb} is usage: {}",
            stderr(&out)
        );
        let refusal = refusal_of(&out, verb);
        assert_eq!(refusal["code"], serde_json::json!("usage"), "{verb}");
        assert_eq!(
            refusal["why"],
            serde_json::json!("--by names no seat — the argument is empty"),
            "{verb}: the refusal names the flag: {refusal}"
        );
    }
    assert_eq!(lines(), before, "nothing reached the stream");
}

/// The old spellings of the pair, `ask` and `answer`, are the usage row naming
/// `hold` and `clear`, whatever followed them — and they read nothing, so no
/// rig: a seat's rules or a person's habit still typing the old verb meets the
/// rewrite and never a sentence about a flag.
#[test]
fn the_old_spellings_ask_and_answer_are_usage_naming_hold_and_clear() {
    let hold = "say fleet hold --question <file>";
    let clear = "say fleet clear <item> <letter> [--text <text>]";
    for (args, rewrite) in [
        (vec!["ask"], hold),
        (vec!["ask", "--note", "no-such-question.md", "--json"], hold),
        (vec!["ask", "--help"], hold),
        (vec!["answer"], clear),
        (
            vec!["answer", "no-such-item-0", "A", "--by", "a-person"],
            clear,
        ),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(&args)
            .current_dir(std::env::temp_dir())
            .hermetic_nowhere()
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(rewrite),
            "{args:?} names the rewrite: {}",
            stderr(&out)
        );
        assert!(
            out.stdout.is_empty(),
            "{args:?} prints no document: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}

/// The flag a prose question went in under is gone, and says where the
/// question goes now: `hold --note` is the usage row naming `--question` and
/// the schema, before the project is read, so it needs no rig — and under
/// `--json` the same sentence is the refusal's `why`.
#[test]
fn the_old_note_flag_of_hold_is_usage_naming_the_question_flag() {
    let rewrite = "--note is gone: a question is a JSON file — fleet hold --question <file>; its \
                   shape is assets/question.schema.json, which the brief shows";
    for json in [false, true] {
        let mut args = vec!["hold", "--note", "no-such-question.md"];
        if json {
            args.push("--json");
        }
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(&args)
            .current_dir(std::env::temp_dir())
            .hermetic_nowhere()
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(rewrite),
            "{args:?} names the rewrite: {}",
            stderr(&out)
        );
        if json {
            let refusal = refusal_of(&out, "hold");
            assert_eq!(refusal["code"], serde_json::json!("usage"), "{refusal}");
            assert_eq!(refusal["why"], serde_json::json!(rewrite), "{refusal}");
        }
    }
}

// ---- `fleet item show`: the SDK's one read of the store ---------------------

impl Rig {
    /// The line a dispatch prints for `item`, ordered to the seat keyed by `id`
    /// under `name`: its machine name, and the one entry `item show` lists.
    fn order_line(&self, item: &str, id: &str, name: &str) -> String {
        use fleet_core::seat::identity::{Kind, SeatId, SeatRef};

        let out = self.item_show(&[item, "--json"]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let data = data_of(&out, "item show");
        let timeline = data["timeline"].as_array().expect("a timeline");
        assert_eq!(timeline.len(), 1, "the one ordered entry: {data}");
        let seat = SeatRef {
            id: SeatId::parse(id).expect("the rig's seat id parses"),
            name: Some(name.to_string()),
            kind: Kind::Agent,
        };
        format!(
            "ordered {item} to {} — entry {}\n",
            seat.machine_name(),
            timeline[0]["id"].as_str().expect("the entry's id")
        )
    }

    /// `fleet item show`, which takes no `--packs-dir`: it renders from the
    /// store alone.
    fn item_show(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["item", "show"])
            .args(args)
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub))
            .output()
            .expect("the built binary runs")
    }
}

/// `item show --json` answers the item under its full id, typed by its hash
/// alone, with the one entry fleet appended — and nothing of the person's
/// comment beside it, which is on the item and is not an entry. A missing item
/// is the refused row, 1, as a refusal document; an item whose timeline holds a
/// malformed entry is could-not-tell, 3, because a timeline with a hole in it
/// answers every question wrong.
#[test]
fn item_show_prints_the_item_and_its_entries_and_refuses_what_it_cannot_read() {
    use fleet_core::entry::{Body, OrderKind, Ordered};
    use fleet_core::seat::actor::Actor;
    use fleet_core::store::{Bd, Store};

    let rig = Rig::new("show");
    let item = rig.a_ready_item();
    let by = Actor::typed(BY).expect("typed").expect("a seat");
    let store = Bd::at(&rig.project);
    let appended = store
        .append(
            &item,
            &Body::Ordered(Ordered {
                order: OrderKind::Dispatch,
                seat: None,
            }),
            &by,
        )
        .expect("the entry is appended");
    let words = "a person's own words, never an entry";
    let out = rig.bd(&["comments", "add", &item, words]);
    assert!(out.status.success(), "bd comments add: {}", stderr(&out));

    let hash = item
        .split_once('-')
        .map(|(_, hash)| hash)
        .expect("the id carries a prefix");
    let out = rig.item_show(&[hash, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!stdout(&out).contains(words), "{}", stdout(&out));
    let data = data_of(&out, "item show");
    assert_eq!(data["id"], serde_json::json!(item), "the full id: {data}");
    assert_eq!(data["title"], "an item the SDK will drive");
    assert_eq!(data["description"], "a scratch item");
    let timeline = data["timeline"].as_array().expect("a timeline");
    assert_eq!(timeline.len(), 1, "one entry: {data}");
    assert_eq!(timeline[0]["id"], serde_json::json!(appended));
    assert_eq!(timeline[0]["kind"], "ordered");
    assert_eq!(
        timeline[0]["by"],
        serde_json::json!({ "kind": "seat", "id": by.id })
    );

    // The person's rendering: the same item, the same one entry.
    let out = rig.item_show(&[&item]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.starts_with(&format!("{item} · an item the SDK will drive  [open]\n")),
        "{text}"
    );
    assert!(text.contains("\ntimeline (1 entries)\n"), "{text}");
    assert!(
        text.contains(&format!(
            "  {BY}  ordered dispatch → a transient seat, not yet named"
        )),
        "{text}"
    );
    assert!(!text.contains(words), "{text}");

    let out = rig.item_show(&["no-such-item-0", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let refusal = refusal_of(&out, "item show");
    assert_eq!(refusal["code"], serde_json::json!("refused"));
    let why = refusal["why"].as_str().expect("a why").to_string();
    assert!(
        stderr(&out).contains(&format!("fleet item show: {why}")),
        "the same sentence on stderr: {}",
        stderr(&out)
    );

    let broken = rig.a_ready_item();
    let out = rig.bd(&[
        "comments",
        "add",
        &broken,
        r#"{"fleet.entry":1,"kind":"ordered","order":"dispatch","bogus":1}"#,
        "--actor",
        BY,
    ]);
    assert!(out.status.success(), "bd comments add: {}", stderr(&out));
    let out = rig.item_show(&[&broken, "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert_eq!(
        refusal_of(&out, "item show")["code"],
        serde_json::json!("could_not_tell")
    );
    let out = rig.item_show(&[&broken]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(out.stdout.is_empty(), "{}", stdout(&out));
    assert!(
        stderr(&out).starts_with("fleet item show: "),
        "{}",
        stderr(&out)
    );
}
