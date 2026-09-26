//! `fleet deliver` through the shipped binary, against a scratch repository and
//! a stub that stands in for the provider.
//!
//! This is where the live git path is proven: the project is a real repository,
//! the commit the verb makes is read back out of it with git, and the delivered
//! entry the store holds names that commit.
//!
//! One repository and one store per arm, because an arm's subject is the state
//! of a working tree. The fixture adds the trunk ref a delivery records its
//! base from, and points `core.hooksPath` at nothing, so what the commit runs
//! is this verb and no hook of the box's.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const REVIEWER: &str = "a-reviewer";
/// The reviewer seat's id, which keys its row; [`REVIEWER`] is its name.
const REVIEWER_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
/// One guard opted out, so a brief rendered off this file carries a line only
/// this file could have put there.
const POLICY: &str = "[guards]\nrecord = { enabled = false }\n\n\
                      [core]\nreviewer = \"a-reviewer\"\n\n\
                      [controller]\nnudge_model = \"a-cheap-model\"\n\
                      nudge_timeout_seconds = 20\n";

/// The delivery the seat hands in: the JSON its brief's schema shows. The
/// commit, the branch, the base and the time are the verb's, so it names none.
const DELIVERY: &str = r#"{
  "files": ["the-work.txt"],
  "checks": [{"check": "AC2", "result": "green, each rc read from its own command"}],
  "suite": {"command": "the workspace suite", "rc": 0},
  "spec_corrections": [],
  "not_proven": [{"surface": "what this arm did not run", "command": "cargo nextest run"}],
  "decisions": [
    {"call": "the delivery is the seat's", "not_taken": "composing it here", "because": "the words are the seat's"}
  ],
  "covers": ["R6"]
}"#;

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

/// One arm: its own repository, work graph, machine directory and stub.
struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    worktree: PathBuf,
    stub: PathBuf,
    roster: PathBuf,
    nudge_argv: PathBuf,
    /// The delivery file, beside the project and never in it: a file in the
    /// tree would be one the delivery left unstaged.
    delivery: PathBuf,
    /// One seat name per arm, which the policy lists and the item is ordered
    /// to.
    seat: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-deliver-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("a-project");
        let machine = root.join("machine");
        let worktree = root.join("worktree");
        for dir in [&project, &machine, &worktree, &root.join("hooks")] {
            std::fs::create_dir_all(dir).expect("the fixture directory is created");
        }
        defaults_into(&machine);

        let rig = Rig {
            stub: root.join("agent.sh"),
            roster: root.join("roster.json"),
            nudge_argv: root.join("nudge-argv"),
            delivery: root.join("delivery.json"),
            seat: format!("a-builder-{label}"),
            root,
            project,
            machine,
            worktree,
        };
        rig.init_store();
        std::fs::write(&rig.delivery, DELIVERY).expect("the delivery is written");
        std::fs::write(
            rig.machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"id": "{REVIEWER_ID}", "name": "{REVIEWER}",
                      "worktrees": {{"a-project": {worktree}}}}}
                   ]}}"#,
                fleet_toml = json_string(&rig.project.join("fleet.toml").display().to_string()),
                worktree = json_string(&rig.worktree.display().to_string()),
            ),
        )
        .expect("the machine config is written");
        rig.write_stub();
        rig.roster("[]");
        rig
    }

    /// The policy, listing the delivering seat and the reviewer: `deliver`
    /// finds the item a seat holds by resolving its actor among the seats the
    /// fleet lists, and hands it to the one listed seat `[core] reviewer`
    /// names, by that seat's id. Then the store the policy names, and the
    /// repository, which never versions the store.
    fn init_store(&self) {
        std::fs::write(
            self.project.join("fleet.toml"),
            format!(
                "{POLICY}{}\n[seats.{REVIEWER_ID}]\nkind = \"agent\"\nname = \"{REVIEWER}\"\n",
                common::seat_table_of(&self.seat)
            ),
        )
        .expect("the policy is written");
        common::take_a_store(&self.project);
        self.git(&["init", "--quiet", "--initial-branch", "main"]);
        common::store_outside_git(&self.project);
    }

    /// The delivering seat's full id, which its items are assigned to.
    fn seat_id(&self) -> String {
        common::seat_id_of(&self.seat)
    }

    /// The repository as a delivery finds it: the policy committed on the
    /// trunk, a trunk ref to record a base from, and a work branch with one
    /// file staged.
    ///
    /// Made HERE, after the item has been made, so this fixture is one that
    /// has actually been used and the tree a seat starts a delivery from is
    /// still clean.
    fn init_repo(&self) {
        self.git(&[
            "config",
            "core.hooksPath",
            &self.root.join("hooks").display().to_string(),
        ]);
        self.git(&["add", "--", "fleet.toml"]);
        self.git(&["commit", "--quiet", "--no-gpg-sign", "-m", "the policy"]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        self.git(&["checkout", "--quiet", "-b", "a-seat/feat/the-work"]);
        std::fs::write(self.project.join("the-work.txt"), "the work\n")
            .expect("the work is written");
        self.git(&["add", "--", "the-work.txt"]);
    }

    /// The repository as a TRANSIENT SEAT finds it (the transient-seat resolution spec): the
    /// policy file written but never committed, the trunk ref on the trunk,
    /// and the work staged in a linked worktree cut beside the primary — so
    /// nothing above that checkout carries a `fleet.toml`.
    ///
    /// THE CHECKOUT SHARES THE PRIMARY'S STORE. With the policy committed, a
    /// verb run there resolves the checkout as its project, and the store is
    /// one per project whichever checkout reaches it.
    ///
    /// Returns the seat's checkout, as the binary will be handed it.
    fn init_repo_in_a_linked_worktree(&self, commit_the_policy: bool) -> PathBuf {
        self.git(&[
            "config",
            "core.hooksPath",
            &self.root.join("hooks").display().to_string(),
        ]);
        if commit_the_policy {
            self.git(&["add", "--", "fleet.toml"]);
        }
        // `--allow-empty`: where the policy is left out there is nothing to
        // add, and the arms want the trunk's commit either way.
        self.git(&[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "--allow-empty",
            "-m",
            "the trunk",
        ]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let seat = self.root.join("a-project-worktrees/agent-1b7e4c09");
        self.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "a-seat/feat/the-work",
            &seat.display().to_string(),
            "HEAD",
        ]);
        common::share_store(&self.project, &seat);
        std::fs::write(seat.join("the-work.txt"), "the work\n").expect("the work is written");
        self.git_in(&seat, &["add", "--", "the-work.txt"]);
        seat
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.project, args)
    }

    fn git_in(&self, cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(cwd)
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

    /// One item, ordered and held by the delivering seat, with the repository
    /// in the shape a delivery finds it.
    fn an_ordered_item(&self) -> String {
        let item = self.an_item_ordered_to_the_seat();
        self.init_repo();
        item
    }

    /// The record half alone, for an arm that builds its own working tree:
    /// the item held by the seat and carrying the order index `brief`
    /// refuses to render without.
    fn an_item_ordered_to_the_seat(&self) -> String {
        let item = common::filed(&self.project, "an item to deliver", &[]);
        common::hand_to(&self.project, &item, &self.seat_id());
        common::ordered(
            &self.project,
            &item,
            fleet_core::store::OrderKind::Dispatch,
            "run:an-architect",
            Some(&self.seat_id()),
        );
        item
    }

    /// The item as the store answers it.
    fn item_json(&self, item: &str) -> serde_json::Value {
        common::shown(&self.project, item)
    }

    /// The item's last delivered entry as `fleet item show --json` lists it:
    /// the reader's own document, off the shipped binary. That verb renders
    /// from the store alone, so it takes no `--packs-dir`.
    fn delivered(&self, item: &str) -> serde_json::Value {
        self.last_entry(item, "delivered")
    }

    /// The item's last entry of this kind, read the same way.
    fn last_entry(&self, item: &str, kind: &str) -> serde_json::Value {
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["item", "show", item, "--json"])
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub))
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let document: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("item show answers one document");
        document["data"]["timeline"]
            .as_array()
            .expect("the document carries a timeline")
            .iter()
            .rev()
            .find(|entry| entry["kind"] == kind)
            .cloned()
            .unwrap_or_else(|| panic!("the timeline carries a {kind} entry: {document}"))
    }

    /// The stub: `agents` is the roster read, `-p` is the one print-mode turn.
    fn write_stub(&self) {
        std::fs::write(
            &self.stub,
            format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 \x20 agents) /bin/cat '{roster}' ;;\n\
                 \x20 -p) printf '%s\\n' \"$@\" > '{argv}' ;;\n\
                 \x20 *) exit 64 ;;\n\
                 esac\n",
                roster = self.roster.display(),
                argv = self.nudge_argv.display(),
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

    /// A roster carrying one LIVE row in the reviewer's worktree.
    fn live(&self) -> &Rig {
        self.roster(&format!(
            r#"[{{"sessionId": "abcdef", "id": "s0", "cwd": {cwd}, "pid": 4242}}]"#,
            cwd = json_string(&self.worktree.display().to_string())
        ))
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_from(&self.project, args)
    }

    /// The same binary from a directory the CALLER names, for an arm whose
    /// subject is what the cwd resolves to.
    fn run_from(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .arg("--packs-dir")
            .arg(self.machine.join("packs"))
            .current_dir(cwd)
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub))
            // The identity the delivery's own commit is made under. Named here
            // because `HOME` is the rig's: without it `git commit` reads the
            // operator's global configuration, and these arms passed on this
            // box because of whose account they ran under.
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .env("FLEET_ACTOR", &self.seat)
            .output()
            .expect("the built binary runs")
    }

    fn nudge_argv(&self) -> String {
        std::fs::read_to_string(&self.nudge_argv).unwrap_or_default()
    }

    /// Every event the machine directory's stream holds, newest last.
    fn events(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("an event is one JSON object"))
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_live_reviewer_is_rung_with_the_item_and_the_commit_the_delivery_made() {
    let rig = Rig::new("live");
    rig.live();
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    // The commit the verb made, read out of the repository rather than out of
    // what the verb said about it.
    let head = rig.git(&["rev-parse", "HEAD"]);
    assert_eq!(head.len(), 40, "a commit is 40 hex: {head}");
    assert_eq!(
        rig.git(&["show", "--name-only", "--format=", "HEAD"]),
        "the-work.txt",
        "the staged set, and nothing beside it"
    );
    assert!(
        rig.git(&["log", "-1", "--format=%s"]).starts_with(&item),
        "the commit subject names the item by its full id"
    );
    // The delivered file is in the commit and nothing is left over: the store
    // is kept outside the repository, so the verb's own store calls leave the
    // tree the next verb reads as clean as the delivery did.
    assert_eq!(
        rig.git(&["status", "--porcelain"]),
        "",
        "nothing of the delivery is left unstaged"
    );

    let record = rig.item_json(&item);
    assert_eq!(record["assignee"], serde_json::json!(REVIEWER_ID));
    let delivered = rig.delivered(&item);
    assert_eq!(
        delivered["commit"].as_str(),
        Some(head.as_str()),
        "the entry names the commit that was made: {delivered}"
    );
    assert_eq!(
        delivered["branch"].as_str(),
        Some("a-seat/feat/the-work"),
        "and the branch it sits on: {delivered}"
    );
    let base = rig.git(&["rev-parse", "refs/remotes/origin/main"]);
    assert_eq!(
        delivered["base"].as_str(),
        Some(base.as_str()),
        "and the base it read, whole: {delivered}"
    );
    assert_eq!(
        delivered["by"],
        serde_json::json!(format!("seat:{}", rig.seat_id())),
        "written by the seat delivering"
    );
    assert_eq!(
        delivered["files"],
        serde_json::json!(["the-work.txt"]),
        "and the rest is the seat's JSON: {delivered}"
    );

    // The event, off the stream the binary wrote: the delivered entry's
    // signal, naming the entry that carries the commit.
    let last = rig
        .events()
        .last()
        .cloned()
        .expect("the stream carries the delivery");
    assert_eq!(
        last["type"].as_str(),
        Some(fleet_core::item::ITEM_ENTRY),
        "{last}"
    );
    // The seat named itself by name; the cli hands the verb its id, typed.
    assert_eq!(
        last["actor"],
        serde_json::json!({ "kind": "seat", "id": rig.seat_id().to_string() })
    );
    assert_eq!(
        last["payload"],
        serde_json::json!({ "item": item, "entry": delivered["id"], "kind": "delivered" }),
        "{last}\n{delivered}"
    );

    let argv = rig.nudge_argv();
    assert!(
        argv.contains(&item) && argv.contains(&head),
        "the reviewer is rung with the item and the commit:\n{argv}"
    );
    assert!(
        argv.contains("a-reviewer-93b9739a"),
        "the reviewer is addressed by the machine name its name resolved to:\n{argv}"
    );
}

#[test]
fn an_empty_roster_exits_zero_and_the_delivery_stands() {
    let rig = Rig::new("absent");
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("DELIVERED, NOT RUNG"),
        "deliver says the delivery stands: {}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains(REVIEWER),
        "and who it is waiting on: {}",
        stdout(&out)
    );
    assert!(
        rig.nudge_argv().is_empty(),
        "an absent reviewer is not rung at all"
    );

    let record = rig.item_json(&item);
    assert_eq!(
        record["assignee"],
        serde_json::json!(REVIEWER_ID),
        "the reassignment recorded the handoff whatever the doorbell did"
    );
    assert!(
        rig.delivered(&item)["commit"].is_string(),
        "and the delivery is on the record"
    );
}

/// The refusals through the binary: each rc read from its own command, and the
/// tree the same after each as before.
#[test]
fn the_trunk_and_an_unclean_tree_are_refused_by_the_shipped_binary() {
    let rig = Rig::new("refused");
    let item = rig.an_ordered_item();
    let delivery = rig.delivery.display().to_string();
    let work = rig.git(&["rev-parse", "refs/heads/a-seat/feat/the-work"]);

    std::fs::write(rig.project.join("forgotten.txt"), "not in the delivery\n")
        .expect("the loose file is written");
    let out = rig.run(&["deliver", "--delivery", &delivery]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("forgotten.txt"),
        "the loose file, by name: {}",
        stderr(&out)
    );
    std::fs::remove_file(rig.project.join("forgotten.txt")).expect("the loose file is removed");
    assert_eq!(
        rig.git(&["rev-parse", "refs/heads/a-seat/feat/the-work"]),
        work,
        "the refusal on the work branch committed nothing to it"
    );

    rig.git(&["stash", "--keep-index", "--quiet"]);
    rig.git(&["checkout", "--quiet", "main"]);
    let out = rig.run(&["deliver", "--delivery", &delivery, "--item", &item]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("main"), "{}", stderr(&out));

    assert_eq!(
        rig.git(&["rev-parse", "HEAD"]),
        rig.git(&["rev-parse", "refs/remotes/origin/main"]),
        "nothing was committed by either refusal"
    );
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(rig.seat_id()),
        "and the item is still the seat's"
    );
}

/// `review --show` through the same binary, over the delivery the verb above
/// wrote: the two verbs meet on the record and nowhere else.
#[test]
fn review_show_reads_the_delivery_the_binary_wrote() {
    let rig = Rig::new("review");
    rig.live();
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let head = rig.git(&["rev-parse", "HEAD"]);

    let out = rig.run(&["review", &item, "--by", REVIEWER]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = stdout(&out);
    assert!(
        said.contains("size: 1 file(s), +1, -0"),
        "the size of the one-line file this delivery added: {said}"
    );
    assert!(
        said.contains(&format!("delivered {head} on a-seat/feat/the-work, base ")),
        "the delivered entry it read: {said}"
    );
    assert!(said.contains("D1 the delivery is the seat's"), "{said}");
}

/// The flag the note went in under is gone, and says where the delivery goes
/// now: exit 2 naming `--delivery` and the schema, whatever the file holds and
/// before the project is read, so nothing is committed.
#[test]
fn the_old_note_flag_is_usage_naming_the_delivery_flag() {
    let rig = Rig::new("old-note");
    let item = rig.an_ordered_item();
    let note = rig.root.join("n.md");
    std::fs::write(&note, "DELIVERED <sha> — <seat>\ncommit:  <pending>\n")
        .expect("the note is written");

    let out = rig.run(&["deliver", "--note", &note.display().to_string()]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(
            "--note is gone: a delivery is a JSON file — fleet deliver --delivery <file>; its \
             shape is assets/delivery.schema.json, which the brief shows"
        ),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        rig.git(&["rev-parse", "HEAD"]),
        rig.git(&["rev-parse", "refs/remotes/origin/main"]),
        "nothing was committed"
    );
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(rig.seat_id()),
        "and the item is still the seat's"
    );
}

/// `review --land` through the same binary: the accept on the record, as the
/// reviewed entry `fleet item show` reads and `--json` names by its id, and the
/// one signal on the stream naming it.
#[test]
fn review_land_writes_the_accept_on_the_record_and_the_event_on_the_stream() {
    let rig = Rig::new("review-land");
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let head = rig.git(&["rev-parse", "HEAD"]);

    let out = rig.run(&["review", &item, "--land", "--by", REVIEWER, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("review --json answers one document");

    let accept = rig.last_entry(&item, "reviewed");
    assert_eq!(accept["verdict"], serde_json::json!("accepted"), "{accept}");
    assert_eq!(accept["commit"], serde_json::json!(head), "{accept}");
    assert_eq!(
        accept["size"]["base"],
        rig.delivered(&item)["base"],
        "measured from the delivery's base: {accept}"
    );
    assert_eq!(
        accept["by"],
        serde_json::json!(format!("seat:{REVIEWER_ID}")),
        "appended by the reviewer: {accept}"
    );
    assert_eq!(
        document["data"]["entry"], accept["id"],
        "--json names the entry it wrote: {document}"
    );

    let last = rig
        .events()
        .last()
        .cloned()
        .expect("the stream carries the verdict");
    assert_eq!(
        last["type"].as_str(),
        Some(fleet_core::item::ITEM_ENTRY),
        "{last}"
    );
    assert_eq!(
        last["actor"],
        serde_json::json!({ "kind": "seat", "id": REVIEWER_ID })
    );
    assert_eq!(
        last["payload"],
        serde_json::json!({ "item": item, "entry": accept["id"], "kind": "reviewed" }),
        "the signal names the accept, which carries the walk: {last}"
    );
    assert_eq!(
        document["data"]["verdict"],
        serde_json::json!("accepted"),
        "{document}"
    );
    let walk = accept["walk"].as_array().expect("the walk is a list");
    assert!(
        !walk.is_empty()
            && walk.iter().enumerate().all(|(k, ruling)| {
                *ruling == serde_json::json!({ "decision": k + 1, "ruling": "accept" })
            }),
        "every call the delivery listed, accepted by its number: {accept}"
    );
}

/// `review --return` through the same binary, fed the findings file tiny's
/// takeoff writes when a person answers B at its hold — the fixture its own
/// suite asserts it writes byte for byte, because that suite runs a fake
/// binary and never learns whether the real one reads the file.
///
/// The control is the file takeoff wrote before its findings were JSON: a
/// marker line and a sentence, numbering nothing, which the verb refuses with
/// exit 2 and without handing the item over.
#[test]
fn review_return_takes_the_findings_file_takeoff_writes_on_b() {
    let rig = Rig::new("review-return");
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let head = rig.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(REVIEWER_ID),
        "the premise: the delivery handed the item to the reviewer"
    );

    let prose = rig.root.join("findings.md");
    std::fs::write(
        &prose,
        format!(
            "RETURNED {item} at {head}\nThe person answered B at the run's hold: B. return to \
             the builder.\n"
        ),
    )
    .expect("the old file is written");
    let out = rig.run(&[
        "review",
        &item,
        "--return",
        &prose.display().to_string(),
        "--by",
        REVIEWER,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("assets/findings.schema.json"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(REVIEWER_ID),
        "a file that does not read hands nothing over"
    );

    let takeoff = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../packs/ts/assets/sdk/testdata/takeoff_findings_b.json");
    let out = rig.run(&[
        "review",
        &item,
        "--return",
        &takeoff.display().to_string(),
        "--by",
        REVIEWER,
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    // The state is the entry kind a return writes, and the verdict rides
    // beside it.
    let document: serde_json::Value =
        serde_json::from_str(stdout(&out).trim()).expect("review --json answers one document");
    assert_eq!(
        (&document["data"]["state"], &document["data"]["verdict"]),
        (
            &serde_json::json!("reviewed"),
            &serde_json::json!("returned")
        ),
        "{document}"
    );

    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(rig.seat_id()),
        "the item goes back to the seat the order named"
    );
    let returned = rig.last_entry(&item, "reviewed");
    assert_eq!(
        returned["verdict"],
        serde_json::json!("returned"),
        "{returned}"
    );
    assert_eq!(returned["commit"], serde_json::json!(head), "{returned}");
    assert_eq!(
        returned["by"],
        serde_json::json!(format!("seat:{REVIEWER_ID}")),
        "{returned}"
    );
    assert_eq!(
        returned["findings"],
        serde_json::json!([
            { "text": "The person answered B at the run's hold: B. return to the builder." }
        ]),
        "the one finding the file carries, as it carries it: {returned}"
    );
    let last = rig
        .events()
        .last()
        .cloned()
        .expect("the stream carries the return");
    assert_eq!(
        last["type"].as_str(),
        Some(fleet_core::item::ITEM_ENTRY),
        "{last}"
    );
    assert_eq!(
        last["payload"],
        serde_json::json!({ "item": item, "entry": returned["id"], "kind": "reviewed" }),
        "{last}"
    );
    assert_eq!(document["data"]["entry"], returned["id"], "{document}");
}

/// The size line's counts over the rows `git diff --numstat` printed.
fn counted(rows: &str) -> String {
    let (mut files, mut added, mut deleted) = (0usize, 0u64, 0u64);
    for row in rows.lines() {
        let columns: Vec<&str> = row.splitn(3, '\t').collect();
        files += 1;
        added += columns[0]
            .parse::<u64>()
            .expect("a text file's added count");
        deleted += columns[1]
            .parse::<u64>()
            .expect("a text file's deleted count");
    }
    format!("size: {files} file(s), +{added}, -{deleted}")
}

#[test]
fn review_measures_every_commit_since_the_base_the_delivery_recorded() {
    let rig = Rig::new("commits");
    let item = rig.an_ordered_item();
    rig.git(&["commit", "--quiet", "--no-gpg-sign", "-m", "first"]);
    std::fs::write(rig.project.join("the-rest.txt"), "the rest\n")
        .expect("the second file is written");
    rig.git(&["add", "--", "the-rest.txt"]);

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let rows = rig.git(&["diff", "--numstat", "origin/main...HEAD"]);
    assert!(
        rows.contains("the-work.txt") && rows.contains("the-rest.txt"),
        "both commits' files: {rows}"
    );

    let out = rig.run(&["review", &item, "--by", REVIEWER]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = stdout(&out);
    assert!(
        said.contains(&counted(&rows)),
        "the counts git gives from the recorded base, {}: {said}",
        counted(&rows)
    );
}

/// A work branch not rebased onto a trunk that moved: the trunk's new file is
/// not counted as a change the delivery reverses.
#[test]
fn review_does_not_count_a_moved_trunk_against_the_delivery() {
    let rig = Rig::new("stale");
    let item = rig.an_ordered_item();
    rig.git(&["checkout", "--quiet", "main"]);
    std::fs::write(rig.project.join("trunk-only.txt"), "the trunk moved\n")
        .expect("the trunk's file is written");
    rig.git(&["add", "--", "trunk-only.txt"]);
    rig.git(&[
        "commit",
        "--quiet",
        "--no-gpg-sign",
        "-m",
        "the trunk moves",
        "--",
        "trunk-only.txt",
    ]);
    rig.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    rig.git(&["checkout", "--quiet", "a-seat/feat/the-work"]);

    let out = rig.run(&["deliver", "--delivery", &rig.delivery.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let rows = rig.git(&["diff", "--numstat", "origin/main...HEAD"]);
    assert!(!rows.contains("trunk-only.txt"), "{rows}");
    assert!(
        rig.git(&["diff", "--numstat", "origin/main", "HEAD"])
            .contains("trunk-only.txt"),
        "the trunk moved past the branch's fork point"
    );

    let out = rig.run(&["review", &item, "--by", REVIEWER]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = stdout(&out);
    assert!(
        said.contains(&counted(&rows)),
        "the counts git gives from the merge-base, {}: {said}",
        counted(&rows)
    );
}

/// AC1 and AC2 of the transient-seat resolution spec for `deliver`, as the PAIR that separates
/// "it resolved" from "it fell back": the same call, from the same directory,
/// once with the machine config naming the fleet and once with no machine
/// config at all.
///
/// The directory is a linked worktree cut beside a primary whose `fleet.toml`
/// is written and never committed, so the checkout carries none and nothing
/// above it does either. The refusal below is what proves the walk found
/// nothing there, so the pass above is the fallback.
#[test]
fn a_seat_worktree_beside_an_uncommitted_policy_delivers_through_the_machine_config() {
    let rig = Rig::new("fallback");
    let item = rig.an_item_ordered_to_the_seat();
    let seat = rig.init_repo_in_a_linked_worktree(false);

    let out = rig.run_from(&seat, &["brief", &item, "--touched", "make check"]);
    assert_eq!(out.status.code(), Some(0), "brief: {}", stderr(&out));
    assert!(
        stdout(&out).contains("- record: off"),
        "the guards are read off the file the machine config names: {}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("make check"),
        "and the builder's checks are the ones the call handed in: {}",
        stdout(&out)
    );

    let out = rig.run_from(
        &seat,
        &["deliver", "--delivery", &rig.delivery.display().to_string()],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let head = rig.git_in(&seat, &["rev-parse", "HEAD"]);
    assert_eq!(
        rig.git_in(&seat, &["show", "--name-only", "--format=", "HEAD"]),
        "the-work.txt",
        "the commit is made in the SEAT's checkout and carries the staged set"
    );
    let record = rig.item_json(&item);
    assert_eq!(record["assignee"], serde_json::json!(REVIEWER_ID));
    assert_eq!(
        rig.delivered(&item)["commit"].as_str(),
        Some(head.as_str()),
        "the delivered entry names the commit that was made"
    );
}

/// AC2's control arm, on the same fixture: with no machine config there is no
/// second answer, and every item verb is the walk's own refusal.
#[test]
fn with_no_machine_config_the_item_verbs_still_refuse_at_three() {
    let rig = Rig::new("no-config");
    let item = rig.an_item_ordered_to_the_seat();
    let seat = rig.init_repo_in_a_linked_worktree(false);
    std::fs::remove_file(rig.machine.join("config.json")).expect("the machine config is removed");

    // The path the refusal names is the one the BINARY read, which on this
    // platform is the resolved `/private` spelling of the rig's own.
    let named = std::fs::canonicalize(&seat).expect("the seat's checkout is there");
    let delivery = rig.delivery.display().to_string();
    let calls: [&[&str]; 4] = [
        &["brief", "THE-ITEM"],
        &["deliver", "--delivery", "THE-DELIVERY"],
        &["review", "THE-ITEM", "--by", "a-reviewer"],
        &[
            "land",
            "THE-ITEM",
            "0123456789abcdef0123456789abcdef01234567",
            "--by",
            "a-reviewer",
        ],
    ];
    for call in calls {
        let args: Vec<String> = call
            .iter()
            .map(|word| match *word {
                "THE-ITEM" => item.clone(),
                "THE-DELIVERY" => delivery.clone(),
                other => other.to_string(),
            })
            .collect();
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = rig.run_from(&seat, &borrowed);
        assert_eq!(out.status.code(), Some(3), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(&format!(
                "no `fleet.toml` and no `.fleet/project.toml` above {}",
                named.display()
            )),
            "{args:?}: {}",
            stderr(&out)
        );
    }
}

/// The control the fallback arm above is only readable beside: THE SAME
/// FIXTURE with the policy file committed, so the walk up from the seat's
/// checkout finds it there and no fallback is taken.
///
/// This is what the shape under test has to reproduce — the delivery is made
/// in the SEAT's checkout, on the seat's own branch, and never in the primary.
#[test]
fn a_seat_worktree_carrying_the_policy_delivers_from_its_own_checkout() {
    let rig = Rig::new("walk-hit");
    let item = rig.an_item_ordered_to_the_seat();
    let seat = rig.init_repo_in_a_linked_worktree(true);

    let out = rig.run_from(
        &seat,
        &["deliver", "--delivery", &rig.delivery.display().to_string()],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let head = rig.git_in(&seat, &["rev-parse", "HEAD"]);
    assert_eq!(
        rig.git_in(&seat, &["show", "--name-only", "--format=", "HEAD"]),
        "the-work.txt",
        "the commit is made in the seat's own checkout"
    );
    assert_eq!(
        rig.git_in(&seat, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "a-seat/feat/the-work",
        "and on the seat's own branch"
    );
    assert_eq!(
        rig.delivered(&item)["commit"].as_str(),
        Some(head.as_str()),
        "the delivered entry names the commit that was made"
    );
}
