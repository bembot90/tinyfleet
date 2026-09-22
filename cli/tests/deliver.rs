//! `fleet deliver` through the shipped binary, against a scratch repository and
//! a stub that stands in for the provider (packs PRD R6, R7, R10).
//!
//! This is where the live git path is proven: the project is a real repository,
//! the commit the verb makes is read back out of it with git, and the delivery
//! note the store holds names that commit.
//!
//! One repository per arm, because an arm's subject is the state of a working
//! tree. `bd init` writes the repository and its first commit; the fixture adds
//! the trunk ref a delivery records its base from, and points `core.hooksPath`
//! at nothing, so what the commit runs is this verb and not the store's own
//! hooks.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const REVIEWER: &str = "a-reviewer";
const POLICY: &str = "[gates]\nsuite = \"make check\"\n\n\
                      [core]\nreviewer = \"a-reviewer\"\n\n\
                      [controller]\nnudge_model = \"a-cheap-model\"\n\
                      nudge_timeout_seconds = 20\n";

/// The note the seat wrote. The three lines deliver fills are left as the seat
/// left them, and every other line is the seat's own word.
const NOTE: &str = "\
DELIVERED <sha> — <seat>
commit:  <pending>
branch:  <pending>
base:    <pending>
files:   the-work.txt
gate:    AC2 green, each rc read from its own command
suite:   the workspace suite, rc 0
spec corrections: none
not proven: what this arm did not run
decisions: 1
  D1 the note is the seat's; not taken: composing it here; because the words are the seat's
covers: R6
";

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
    note: PathBuf,
    /// One seat name per arm. The store is the run's shared board, and `which
    /// item does this seat hold` is a query across the whole of it, so two arms
    /// on one seat name would each be refused for the other's ordered item.
    /// The REVIEWER needs no such treatment: every arm names the item it
    /// reviews, so no read of theirs goes through the assignee.
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
            note: root.join("note.md"),
            seat: format!("a-builder-{label}"),
            root,
            project,
            machine,
            worktree,
        };
        rig.init_store();
        std::fs::write(&rig.note, NOTE).expect("the note is written");
        std::fs::write(
            rig.machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"name": "{REVIEWER}", "chosen_name": "Kite",
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

    fn init_store(&self) {
        std::fs::write(self.project.join("fleet.toml"), POLICY).expect("the policy is written");
        common::take_a_board(&self.project, "deliver");
    }

    /// The repository as a delivery finds it: the policy committed on the
    /// trunk, a trunk ref to record a base from, and a work branch with one
    /// file staged.
    ///
    /// The store's own files are committed HERE, after the item has been made,
    /// because `bd` appends to a log this repository versions on every call:
    /// the tree a seat starts a delivery from is clean, and this fixture is one
    /// that has actually been used.
    fn init_repo(&self) {
        self.git(&[
            "config",
            "core.hooksPath",
            &self.root.join("hooks").display().to_string(),
        ]);
        self.git(&["add", "--", "fleet.toml", ".beads"]);
        self.git(&["commit", "--quiet", "--no-gpg-sign", "-m", "the policy"]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        self.git(&["checkout", "--quiet", "-b", "a-seat/feat/the-work"]);
        std::fs::write(self.project.join("the-work.txt"), "the work\n")
            .expect("the work is written");
        self.git(&["add", "--", "the-work.txt"]);
    }

    /// The repository as a TRANSIENT SEAT finds it (the transient-seat resolution spec): the
    /// policy file written but never committed, the board and the trunk ref on
    /// the trunk, and the work staged in a linked worktree cut beside the
    /// primary — so nothing above that checkout carries a `fleet.toml`.
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
        self.git(&["add", "--", ".beads"]);
        self.git(&["commit", "--quiet", "--no-gpg-sign", "-m", "the board"]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let seat = self.root.join("a-project-worktrees/transient-1");
        self.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "a-seat/feat/the-work",
            &seat.display().to_string(),
            "HEAD",
        ]);
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

    fn bd(&self, args: &[&str]) -> Output {
        Command::new("bd")
            .arg("-C")
            .arg(&self.project)
            .args(args)
            .output()
            .expect("bd runs")
    }

    /// One item, ordered and held by the delivering seat, with the repository
    /// in the shape a delivery finds it.
    fn an_ordered_item(&self) -> String {
        let item = self.an_item_ordered_to_the_seat();
        self.init_repo();
        item
    }

    /// The order note `brief` refuses to render without. `deliver` reads the
    /// metadata key instead, so only an arm that also briefs needs this.
    fn an_order_note_on(&self, item: &str) {
        assert!(self
            .bd(&[
                "note",
                item,
                "dispatched by an-architect — orders given",
                "--actor",
                "an-architect",
            ])
            .status
            .success());
    }

    /// The record half alone, for an arm that builds its own working tree.
    fn an_item_ordered_to_the_seat(&self) -> String {
        let out = self.bd(&[
            "create",
            "--title",
            "an item to deliver",
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
        let item = value["id"].as_str().expect("an id").to_string();
        assert!(self
            .bd(&[
                "update",
                &item,
                "--assignee",
                &self.seat,
                "--metadata",
                &format!(
                    r#"{{"orders": {{"by": "an-architect", "kind": "dispatch", "seat": "{seat}", "at": "2026-09-09T00:00:00Z"}}}}"#,
                    seat = self.seat
                ),
                "--actor",
                "an-architect",
            ])
            .status
            .success());
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
            .env("BEADS_ACTOR", &self.seat)
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

    let out = rig.run(&["deliver", "--note", &rig.note.display().to_string()]);
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
    // The delivered file is in the commit and nothing of it is left over. The
    // whole tree is NOT asserted clean: this verb's own store calls append to a
    // log this repository versions, which is the tree the next verb reads.
    assert_eq!(
        rig.git(&["status", "--porcelain", "--", "the-work.txt"]),
        "",
        "nothing of the delivered file is left unstaged"
    );

    let record = rig.item_json(&item);
    assert_eq!(record["assignee"], serde_json::json!(REVIEWER));
    let notes = record["notes"].as_str().unwrap_or_default();
    assert!(
        notes.contains(&format!("commit:  {head}")),
        "the note names the commit that was made: {notes}"
    );
    assert!(
        notes.contains("branch:  a-seat/feat/the-work"),
        "and the branch it sits on: {notes}"
    );
    assert!(
        notes.contains("base:    origin/main at"),
        "and the base it read: {notes}"
    );

    // The event, off the stream the binary wrote, carrying the same three values
    // the note's machine lines do.
    let last = rig
        .events()
        .last()
        .cloned()
        .expect("the stream carries the delivery");
    assert_eq!(last["type"].as_str(), Some("item.delivered"), "{last}");
    assert_eq!(last["actor"].as_str(), Some(rig.seat.as_str()));
    assert_eq!(last["payload"]["item"].as_str(), Some(item.as_str()));
    assert_eq!(last["payload"]["commit"].as_str(), Some(head.as_str()));
    assert_eq!(
        last["payload"]["branch"].as_str(),
        Some("a-seat/feat/the-work")
    );
    assert!(
        notes.contains(&format!(
            "base:    origin/main at {}",
            last["payload"]["base"].as_str().expect("a base")
        )),
        "the event's base is the note's own: {last}\n{notes}"
    );

    let argv = rig.nudge_argv();
    assert!(
        argv.contains(&item) && argv.contains(&head),
        "the reviewer is rung with the item and the commit:\n{argv}"
    );
    assert!(
        argv.contains("Kite"),
        "the reviewer is addressed by name:\n{argv}"
    );
}

#[test]
fn an_empty_roster_exits_zero_and_the_delivery_stands() {
    let rig = Rig::new("absent");
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--note", &rig.note.display().to_string()]);
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
        serde_json::json!(REVIEWER),
        "the reassignment recorded the handoff whatever the doorbell did"
    );
    assert!(record["notes"]
        .as_str()
        .unwrap_or_default()
        .contains("DELIVERED "));
}

/// The refusals through the binary: each rc read from its own command, and the
/// tree the same after each as before (R10).
#[test]
fn the_trunk_and_an_unclean_tree_are_refused_by_the_shipped_binary() {
    let rig = Rig::new("refused");
    let item = rig.an_ordered_item();
    let note = rig.note.display().to_string();
    let work = rig.git(&["rev-parse", "refs/heads/a-seat/feat/the-work"]);

    std::fs::write(rig.project.join("forgotten.txt"), "not in the delivery\n")
        .expect("the loose file is written");
    let out = rig.run(&["deliver", "--note", &note]);
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
    let out = rig.run(&["deliver", "--note", &note, "--item", &item]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("main"), "{}", stderr(&out));

    assert_eq!(
        rig.git(&["rev-parse", "HEAD"]),
        rig.git(&["rev-parse", "refs/remotes/origin/main"]),
        "nothing was committed by either refusal"
    );
    assert_eq!(
        rig.item_json(&item)["assignee"],
        serde_json::json!(rig.seat),
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

    let out = rig.run(&["deliver", "--note", &rig.note.display().to_string()]);
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
        said.contains(&format!("commit:  {head}")),
        "the delivery note it read: {said}"
    );
    assert!(said.contains("D1 the note is the seat's"), "{said}");
}

/// `review --land` through the same binary: the accept on the record, and the
/// one event on the stream carrying the walk's own counts.
#[test]
fn review_land_writes_the_accept_on_the_record_and_the_event_on_the_stream() {
    let rig = Rig::new("review-land");
    let item = rig.an_ordered_item();

    let out = rig.run(&["deliver", "--note", &rig.note.display().to_string()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let head = rig.git(&["rev-parse", "HEAD"]);

    let out = rig.run(&["review", &item, "--land", "--by", REVIEWER]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let notes = rig.item_json(&item)["notes"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        notes.contains(&format!("ACCEPTED {head} — {REVIEWER}")),
        "the verdict is on the record: {notes}"
    );

    let last = rig
        .events()
        .last()
        .cloned()
        .expect("the stream carries the verdict");
    assert_eq!(last["type"].as_str(), Some("item.reviewed"), "{last}");
    assert_eq!(last["actor"].as_str(), Some(REVIEWER));
    assert_eq!(last["payload"]["item"].as_str(), Some(item.as_str()));
    assert_eq!(last["payload"]["commit"].as_str(), Some(head.as_str()));
    assert_eq!(last["payload"]["verdict"].as_str(), Some("accepted"));
    assert_eq!(
        last["payload"]["overruled"],
        serde_json::json!(0),
        "an accept overrules nothing: {last}"
    );
    let accepted = last["payload"]["accepted"].as_u64().expect("a count");
    assert!(
        notes.contains(&format!("{accepted} accepted, 0 overruled")),
        "the event's counts are the walk's own: {last}\n{notes}"
    );
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

    let out = rig.run(&["deliver", "--note", &rig.note.display().to_string()]);
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

    let out = rig.run(&["deliver", "--note", &rig.note.display().to_string()]);
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
    rig.an_order_note_on(&item);
    let seat = rig.init_repo_in_a_linked_worktree(false);

    let out = rig.run_from(&seat, &["brief", &item]);
    assert_eq!(out.status.code(), Some(0), "brief: {}", stderr(&out));
    assert!(
        stdout(&out).contains("make check"),
        "the suite is read off the file the machine config names: {}",
        stdout(&out)
    );

    let out = rig.run_from(
        &seat,
        &["deliver", "--note", &rig.note.display().to_string()],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let head = rig.git_in(&seat, &["rev-parse", "HEAD"]);
    assert_eq!(
        rig.git_in(&seat, &["show", "--name-only", "--format=", "HEAD"]),
        "the-work.txt",
        "the commit is made in the SEAT's checkout and carries the staged set"
    );
    let record = rig.item_json(&item);
    assert_eq!(record["assignee"], serde_json::json!(REVIEWER));
    let notes = record["notes"].as_str().unwrap_or_default();
    assert!(
        notes.contains(&format!("commit:  {head}")),
        "the note names the commit that was made: {notes}"
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
    let note = rig.note.display().to_string();
    let calls: [&[&str]; 4] = [
        &["brief", "THE-ITEM"],
        &["deliver", "--note", "THE-NOTE"],
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
                "THE-NOTE" => note.clone(),
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
        &["deliver", "--note", &rig.note.display().to_string()],
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
    let notes = rig.item_json(&item)["notes"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        notes.contains(&format!("commit:  {head}")),
        "the note names the commit that was made: {notes}"
    );
}
