//! `fleet dispatch` through the shipped binary, with the ring reaching a stub
//! that stands in for the provider.
//!
//! The stub answers both halves the ring uses — the roster read and the one
//! print-mode turn — and records the argv of the turn, so what the ring passed
//! is read from what the stub was given rather than from the code that passed
//! it.
//!
//! One project and one store per arm process; under a `make fleet-test` run the
//! store is the run's own shared board, copied in. Nothing here reads the board
//! as a whole: each arm names its own item and its own seat, so a neighbour's
//! rows move no answer this file asserts on. The machine directory, the roster
//! and the stub's seams are per arm, because they are what each arm varies. The
//! shared project outlives the process — a shared handle has no owner to drop it
//! — so it is left under the system temp directory, named by this process's id.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const MODEL: &str = "a-cheap-model";

/// The id every arm's one seat row is keyed by. Each arm's seat carries a name
/// of its own, which is what `--to` names it by and what the order is written
/// against, so the shared board still holds one seat's work per arm.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

/// The person every arm dispatches as: a human seat on the fleet's roster, so
/// `--by lead-1` resolves to it and the order carries it as `seat:<id>`.
const LEAD_ID: &str = "01a0d1f1-0aec-765f-9abe-00000000000a";
/// An agent seat the actor arms name by its name.
const ORLA_ID: &str = "01a0d1f1-0aec-765f-9abe-00000000000b";
/// Two seats one name answers to, for the ambiguous actor.
const TWIN_A: &str = "01a0d1f1-0aec-765f-9abe-00000000000c";
const TWIN_B: &str = "01a0d1f1-0aec-765f-9abe-00000000000d";
/// A machine identity the roster lists, which the arm about the once-line's
/// absence writes into its own machine directory.
const LISTED_IDENTITY: &str = "01a0d1f1-0aec-765f-9abe-00000000000e";

const POLICY: &str = "[controller]\nnudge_model = \"a-cheap-model\"\n\
                      nudge_timeout_seconds = 20\n\
                      [seats.01a0d1f1-0aec-765f-9abe-00000000000a]\n\
                      kind = \"human\"\nname = \"lead-1\"\n\
                      [seats.01a0d1f1-0aec-765f-9abe-00000000000b]\n\
                      kind = \"agent\"\nname = \"Orla\"\n\
                      [seats.01a0d1f1-0aec-765f-9abe-00000000000c]\n\
                      kind = \"agent\"\nname = \"twin\"\n\
                      [seats.01a0d1f1-0aec-765f-9abe-00000000000d]\n\
                      kind = \"agent\"\nname = \"twin\"\n\
                      [seats.01a0d1f1-0aec-765f-9abe-00000000000e]\n\
                      kind = \"human\"\nname = \"the-person\"\n";

/// The order's `by` for an arm that dispatches `--by lead-1`.
fn lead() -> String {
    format!("seat:{LEAD_ID}")
}

fn defaults_into(machine: &Path) -> PathBuf {
    let root = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&root).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
    root
}

/// One of the binary's own defaults, written for ONE ARM and removed with it.
///
/// Owned and dropped rather than held in a `OnceLock`: a static holding a
/// `PathBuf` never runs a destructor, so a shared tree is one 19-file directory
/// left under the temp directory per test process, for ever.
struct ShippedDefaults(PathBuf);

impl ShippedDefaults {
    fn new(label: &str) -> ShippedDefaults {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-defaults-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        ShippedDefaults(defaults_into(&root))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ShippedDefaults {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(
            self.0
                .parent()
                .expect("the set sits under a directory of its own"),
        );
    }
}

/// The project every arm dispatches inside: one directory, one work graph, and
/// the basename the machine's worktree row is keyed on.
struct Project {
    root: PathBuf,
}

impl Project {
    fn shared() -> &'static Project {
        static PROJECT: OnceLock<Project> = OnceLock::new();
        PROJECT.get_or_init(|| {
            let root = std::env::temp_dir()
                .join(format!("fleet-cli-dispatch-{}", std::process::id()))
                .join("a-project");
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("the project directory is created");
            std::fs::write(root.join("fleet.toml"), POLICY).expect("the policy file is written");
            common::take_a_board(&root, "dispatch");
            Project { root }
        })
    }

    fn bd(&self, args: &[&str]) -> Output {
        Command::new("bd")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .expect("bd runs")
    }

    fn item(&self, title: &str) -> String {
        let out = self.bd(&[
            "create",
            "--title",
            title,
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
        let text = String::from_utf8_lossy(&out.stdout);
        let value: serde_json::Value =
            serde_json::from_str(text.trim()).expect("bd create answers JSON");
        value["id"].as_str().expect("an id").to_string()
    }

    /// An item carrying the order index `brief` refuses to render without, and
    /// renders its order from.
    fn ordered(&self, title: &str) -> String {
        let item = self.item(title);
        assert!(self
            .bd(&[
                "update",
                &item,
                "--metadata",
                r#"{"fleet.orders": {"v": 1, "by": "run:lead-1", "kind": "dispatch", "at": "2026-09-09T00:00:00Z"}}"#,
                "--actor",
                "lead-1",
            ])
            .status
            .success());
        item
    }

    /// The item closed, so the seat it was given to holds nothing again: an arm
    /// that dispatches to its one seat more than once frees it in between.
    fn done(&self, item: &str) {
        let closed = self.bd(&[
            "close", item, "--reason", "done", "--actor", "an-arm", "--force",
        ]);
        assert!(
            closed.status.success(),
            "bd close: {}",
            String::from_utf8_lossy(&closed.stderr)
        );
    }

    /// The assignee and the index a dispatch writes, read back off the store,
    /// with the item's `notes` beside them — `None` where `bd show --json`
    /// carries no such key, which is what an item no verb noted answers.
    fn order_of(&self, item: &str) -> (Option<String>, Option<String>, serde_json::Value) {
        let out = self.bd(&["-q", "show", item, "--json"]);
        let text = String::from_utf8_lossy(&out.stdout);
        let value: serde_json::Value =
            serde_json::from_str(text.trim()).expect("bd show answers JSON");
        let row = &value[0];
        (
            row.get("assignee")
                .and_then(|a| a.as_str())
                .map(str::to_string),
            row.get("notes").map(|notes| notes.to_string()),
            row.get("metadata")
                .and_then(|m| m.get("fleet.orders"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
    }
}

/// One arm's machine directory, seat worktree and provider stub.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    worktree: PathBuf,
    stub: PathBuf,
    roster: PathBuf,
    nudge_argv: PathBuf,
    nudge_exit: PathBuf,
    /// One seat name per arm, under a prefix no other crate's rigs use. The
    /// store is the run's one board, and `seat holds an item` is a query across
    /// the whole of it, so two arms on one seat name — in this file or in
    /// another crate's — would each be refused for the other's work.
    seat: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-ring-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let machine = root.join("machine");
        let worktree = root.join("worktree");
        std::fs::create_dir_all(&machine).expect("the machine directory is created");
        std::fs::create_dir_all(&worktree).expect("the seat's worktree is created");
        defaults_into(&machine);

        let rig = Rig {
            stub: root.join("agent.sh"),
            roster: root.join("roster.json"),
            nudge_argv: root.join("nudge-argv"),
            nudge_exit: root.join("nudge-exit"),
            seat: format!("s-cli-{label}"),
            root,
            machine,
            worktree,
        };
        std::fs::write(
            rig.machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"id": "{SEAT_ID}", "name": "{seat}",
                      "worktrees": {{"a-project": {worktree}}}}}
                   ]}}"#,
                seat = rig.seat,
                fleet_toml = json_string(
                    &Project::shared()
                        .root
                        .join("fleet.toml")
                        .display()
                        .to_string()
                ),
                worktree = json_string(&rig.worktree.display().to_string()),
            ),
        )
        .expect("the machine config is written");
        rig.write_stub();
        rig.roster("[]");
        rig
    }

    /// The stub: `agents` is the roster read, `-p` is the one print-mode turn.
    /// Nothing is inherited by an effect's child, so `cat` is named by its
    /// absolute path rather than found on a `PATH` the adapter rebuilds.
    fn write_stub(&self) {
        std::fs::write(
            &self.stub,
            format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 \x20 agents) /bin/cat '{roster}' ;;\n\
                 \x20 -p)\n\
                 \x20   printf '%s\\n' \"$@\" > '{argv}'\n\
                 \x20   exit $(/bin/cat '{exit_file}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 *) exit 64 ;;\n\
                 esac\n",
                roster = self.roster.display(),
                argv = self.nudge_argv.display(),
                exit_file = self.nudge_exit.display(),
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

    /// A roster carrying one LIVE row in this seat's worktree.
    fn live(&self) -> &Rig {
        self.roster(&format!(
            r#"[{{"sessionId": "abcdef", "id": "s0", "cwd": {cwd}, "pid": 4242}}]"#,
            cwd = json_string(&self.worktree.display().to_string())
        ))
    }

    fn nudge_exits(&self, code: u8) -> &Rig {
        std::fs::write(&self.nudge_exit, format!("{code}\n")).expect("the seam is written");
        self
    }

    /// The shipped binary, with the belt's two readings forced.
    ///
    /// The transient arm below reaches the load belt, whose load leg reads this
    /// machine unless it is told otherwise — so under a full workspace run the
    /// belt could refuse first and that arm would pass on a withdrawal it was
    /// not written for. Forcing a calm pair makes it the arm its own comment
    /// describes.
    fn run(&self, args: &[&str]) -> Output {
        self.run_with_agent(args, self.stub.clone())
    }

    /// The same, with the agent binary seam named by the caller: the leg that
    /// resolves it is the one an arm makes answer could-not-tell.
    fn run_with_agent(&self, args: &[&str], bin: PathBuf) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .arg("--packs-dir")
            .arg(self.machine.join("packs"))
            .current_dir(&Project::shared().root)
            .hermetic(&self.root.join("home"), &self.machine, Some(&bin))
            .env("FLEET_LOAD_AVERAGE", "0.1")
            .env("FLEET_CPUS", "8")
            .output()
            .expect("the built binary runs")
    }

    /// What the turn was given, as one text. The stub writes one argument per
    /// line and the prompt is many lines long, so the lines are not the
    /// arguments and only the whole text can be read for a value.
    fn nudge_argv(&self) -> String {
        std::fs::read_to_string(&self.nudge_argv).unwrap_or_default()
    }

    /// The item's timeline as `fleet item show --json` lists it: the reader's
    /// own document, off the shipped binary. That verb renders from the store
    /// alone, so it takes no `--packs-dir`.
    fn timeline(&self, item: &str) -> Vec<serde_json::Value> {
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["item", "show", item, "--json"])
            .current_dir(&Project::shared().root)
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub))
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let document: serde_json::Value =
            serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
                .expect("item show answers one document");
        document["data"]["timeline"]
            .as_array()
            .expect("the document carries a timeline")
            .clone()
    }
}

/// The kinds a timeline lists, in its order.
fn kinds(timeline: &[serde_json::Value]) -> Vec<&str> {
    timeline
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap_or_default())
        .collect()
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_live_row_in_the_seats_worktree_is_rung_with_the_item_and_the_brief() {
    let project = Project::shared();
    let rig = Rig::new("live");
    rig.live();
    let item = project.item("a ready item for a live seat");

    let out = rig.run(&[
        "dispatch",
        &item,
        "--to",
        &rig.seat,
        "--by",
        "lead-1",
        "--touched",
        "make the-touched-gate",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let timeline = rig.timeline(&item);
    assert_eq!(kinds(&timeline), ["ordered"], "{timeline:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!(
            "ordered {item} to {}-93b9739a — entry {}\n",
            rig.seat,
            timeline[0]["id"].as_str().expect("the entry's id")
        )
    );
    // The builder's checks the call handed in are the ones the brief names.
    let brief = std::fs::read_to_string(rig.machine.join("briefs").join(format!("{item}.md")))
        .expect("the brief is written");
    assert!(
        brief.contains("```\nmake the-touched-gate\n```"),
        "the brief renders the touched command handed to dispatch:\n{brief}"
    );

    let argv = rig.nudge_argv();
    let mut lines = argv.lines();
    assert_eq!(lines.next(), Some("-p"), "print mode is the first argument");
    assert_eq!(lines.next(), Some("--model"));
    assert_eq!(
        lines.next(),
        Some(MODEL),
        "the policy's model, not a default"
    );
    assert!(argv.contains(&item), "the prompt names the item:\n{argv}");
    assert!(
        argv.contains(
            &rig.machine
                .join("briefs")
                .join(format!("{item}.md"))
                .display()
                .to_string()
        ),
        "the prompt names the brief's path:\n{argv}"
    );
    assert!(
        argv.contains(&format!("{}-93b9739a", rig.seat)),
        "the seat is addressed by the machine name its name resolved to:\n{argv}"
    );

    // The record carries the seat's FULL ID, whatever name the `--to` said: in
    // the assignee, the index and the ordered entry.
    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(assignee.as_deref(), Some(SEAT_ID));
    assert_eq!(notes, None, "no note is written");
    assert_eq!(orders["seat"], serde_json::json!(SEAT_ID));
    assert_eq!(timeline[0]["seat"], serde_json::json!(SEAT_ID));
    assert_eq!(timeline[0]["order"], serde_json::json!("dispatch"));
    assert_eq!(
        timeline[0]["by"],
        serde_json::json!(format!("seat:{LEAD_ID}"))
    );
}

/// ACCEPTANCE 8 of fleet-zlk.5: the order is an entry, and the document says
/// which. `dispatch --json` answers `data.entry`, the id of the one ordered
/// entry `fleet item show --json` lists, and `bd show --json` carries no
/// `notes` key at all — no verb here writes a note.
#[test]
fn a_dispatch_answers_the_ordered_entry_item_show_lists_and_writes_no_note() {
    let project = Project::shared();
    let rig = Rig::new("entry");
    rig.live();
    let item = project.item("a ready item whose order is an entry");

    let out = rig.run(&[
        "dispatch", &item, "--to", &rig.seat, "--by", "lead-1", "--json",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
            .expect("stdout is the one document");
    let data = &document["data"];

    let timeline = rig.timeline(&item);
    assert_eq!(kinds(&timeline), ["ordered"], "{timeline:?}");
    assert_eq!(data["entry"], timeline[0]["id"], "{data}");
    assert!(data["entry"].is_string(), "{data}");

    let shown = project.bd(&["-q", "show", &item, "--json"]);
    let shown: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&shown.stdout).trim())
            .expect("bd show answers JSON");
    assert!(
        shown[0].get("notes").is_none(),
        "bd show carries no notes key: {shown}"
    );
}

/// The ring addresses the session by the name its newest session row RECORDED
/// at start, and not by the seat's machine name today: a seat renamed since its
/// session started is still answering to the old name. The seat here was
/// started as `orla-93b9739a`, and the seat list names it otherwise now.
#[test]
fn a_ring_addresses_the_session_by_the_name_its_row_recorded() {
    let project = Project::shared();
    let rig = Rig::new("recorded-name");
    rig.live();
    std::fs::write(
        rig.machine.join("sessions.json"),
        format!(
            r#"{{"schema": 2, "sessions": [{{
                 "seat": "{SEAT_ID}", "project": "a-project", "worktree": {worktree},
                 "name": "orla-93b9739a", "model": "a-model", "posture": "auto",
                 "first_turn": "/wake orla-93b9739a", "transient": false,
                 "dispatch_id": "a-dispatch", "dispatched_at": 1000
               }}]}}"#,
            worktree = json_string(&rig.worktree.display().to_string()),
        ),
    )
    .expect("the session table is written");
    let item = project.item("a ready item for a renamed seat");

    let out = rig.run(&["dispatch", &item, "--to", &rig.seat, "--by", "lead-1"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let argv = rig.nudge_argv();
    assert!(
        argv.contains("session named `orla-93b9739a`"),
        "the ring addresses the name the session was started under:\n{argv}"
    );
    assert!(
        !argv.contains(&format!("{}-93b9739a", rig.seat)),
        "and not the machine name the seat carries now:\n{argv}"
    );
}

#[test]
fn an_empty_roster_exits_four_and_the_three_writes_stand() {
    let project = Project::shared();
    let rig = Rig::new("absent");
    let item = project.item("a ready item for a seat that is not up");

    let out = rig.run(&["dispatch", &item, "--to", &rig.seat, "--by", "lead-1"]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ORDERED, NOT RUNG"),
        "{}",
        stderr(&out)
    );
    assert!(
        out.stdout.is_empty(),
        "the order line is stdout's on success only"
    );
    assert!(
        rig.nudge_argv().is_empty(),
        "an absent seat is not rung at all"
    );

    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(assignee.as_deref(), Some(SEAT_ID), "the assignment stands");
    assert_eq!(notes, None, "nothing was noted");
    assert_eq!(
        kinds(&rig.timeline(&item)),
        ["ordered"],
        "the ordered entry stands"
    );
    assert_eq!(
        orders["kind"],
        serde_json::json!("dispatch"),
        "the index stands"
    );
    // A dispatch handed no builder's checks: the brief names the absence where
    // the command goes.
    let brief = std::fs::read_to_string(rig.machine.join("briefs").join(format!("{item}.md")))
        .expect("the brief stands");
    assert!(
        brief.contains(fleet_core::item::brief::DERIVE_TOUCHED),
        "the brief names the absent touched command:\n{brief}"
    );
}

#[test]
fn a_ring_the_provider_refuses_exits_one_and_the_three_writes_stand() {
    let project = Project::shared();
    let rig = Rig::new("failed");
    rig.live().nudge_exits(1);
    let item = project.item("a ready item whose ring will not land");

    let out = rig.run(&["dispatch", &item, "--to", &rig.seat, "--by", "lead-1"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ORDERED, NOT RUNG"),
        "{}",
        stderr(&out)
    );
    assert!(
        !rig.nudge_argv().is_empty(),
        "the turn was attempted, and it is its exit that refused"
    );

    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(assignee.as_deref(), Some(SEAT_ID));
    assert_eq!(notes, None, "nothing was noted");
    assert_eq!(kinds(&rig.timeline(&item)), ["ordered"]);
    assert_eq!(orders["by"], serde_json::json!(lead()));
}

/// AC2's third clause: a spawn the controller refuses withdraws the order in
/// the same act, so a refusal never leaves an ordered item nobody holds.
///
/// The refusal here is the WORKTREE's: this suite's shared project is not a git
/// repository, so `git worktree add` cannot resolve `origin/main` — which is a
/// spawn that gets past the belt and fails before it has made anything. The
/// spawn's own refusals are measured against a real repository in `seat.rs`;
/// what this arm is about is the withdrawal on the other side of the seam.
#[test]
fn a_spawn_the_controller_refuses_withdraws_the_order() {
    let project = Project::shared();
    let rig = Rig::new("transient");
    let item = project.item("a ready item nobody can be spawned for");

    let out = rig.run(&["dispatch", &item, "--by", "lead-1"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("was not dispatched") && stderr(&out).contains("withdrawn"),
        "{}",
        stderr(&out)
    );

    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(assignee, None, "nobody was ever assigned");
    assert_eq!(orders, serde_json::Value::Null, "no orders key survives");
    assert_eq!(notes, None, "nothing was noted");
    let timeline = rig.timeline(&item);
    assert_eq!(
        kinds(&timeline),
        ["ordered", "order_withdrawn"],
        "the withdrawal is on the record: {timeline:?}"
    );
    assert_eq!(timeline[1]["why"], serde_json::json!("spawn_refused"));
    assert!(
        timeline[1]["cause"].is_string(),
        "it names the cause: {timeline:?}"
    );
    assert!(
        stderr(&out).contains("withdrawn: DISPATCH WITHDRAWN — spawn refused: "),
        "{}",
        stderr(&out)
    );
}

/// AC2 of the could-not-tell spec — the spawner's could-not-tell leg, end to end: an agent
/// binary that does not resolve is a question about the environment and not a
/// verdict on the spawn, so the verb exits 3 and the order it wrote is still
/// there for the retry.
///
/// READ BESIDE `a_spawn_the_controller_refuses_withdraws_the_order` above,
/// which takes the other leg through the same seam: that one exits 1 and the
/// order is gone. A spawner that answered `CouldNotTell` on every path would
/// pass this arm and red that one.
#[test]
fn an_unresolvable_agent_binary_exits_three_and_the_order_stands() {
    let project = Project::shared();
    let rig = Rig::new("untold");
    let item = project.item("a ready item whose spawn nobody could observe");

    let out = rig.run_with_agent(
        &["dispatch", &item, "--by", "lead-1"],
        rig.root.join("no-such-agent"),
    );
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("is not an executable file"),
        "the cause is named: {}",
        stderr(&out)
    );
    assert!(
        !stderr(&out).contains("withdrawn"),
        "nothing was withdrawn: {}",
        stderr(&out)
    );

    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(assignee, None, "no seat came up, so nobody was assigned");
    assert_eq!(
        orders["kind"],
        serde_json::json!("dispatch"),
        "the order stands: {orders}"
    );
    assert_eq!(notes, None, "nothing was noted");
    assert_eq!(
        kinds(&rig.timeline(&item)),
        ["ordered"],
        "the order and nothing after it: the cause is the exit's message"
    );
    assert!(
        stderr(&out).contains("could not tell: DISPATCH COULD NOT TELL"),
        "the could-not-tell's words are stderr's: {}",
        stderr(&out)
    );
}

/// The dispatch a verb gives with no `--by` and no `FLEET_ACTOR`: the command
/// every actor arm below starts from. The hermetic block strips the actor the
/// environment carries, which the arm after this one proves.
fn nameless(rig: &Rig, args: &[&str]) -> Command {
    nameless_under(rig, args, None)
}

/// The same, with `FLEET_ACTOR` set on the command BEFORE the hermetic block
/// is put on it — the way a suite run from inside a fleet-started session
/// inherits one.
fn nameless_under(rig: &Rig, args: &[&str], inherited: Option<&str>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fleet"));
    if let Some(actor) = inherited {
        command.env("FLEET_ACTOR", actor);
    }
    command
        .args(args)
        .arg("--packs-dir")
        .arg(rig.machine.join("packs"))
        .current_dir(&Project::shared().root)
        .hermetic(&rig.root.join("home"), &rig.machine, Some(&rig.stub))
        .env("FLEET_LOAD_AVERAGE", "0.1")
        .env("FLEET_CPUS", "8");
    command
}

/// THE SUITE NEVER ACTS AS THE SEAT THAT RAN IT. A session the controller
/// started carries `FLEET_ACTOR=seat:<id>`, and every rig's hermetic block
/// strips it: a verb the rig runs with no `--by` acts as the rig's machine
/// identity, never as the inherited seat.
#[test]
fn an_inherited_fleet_actor_is_stripped_by_the_hermetic_block() {
    let project = Project::shared();
    let rig = Rig::new("inherited-actor");
    rig.live();
    let item = project.item("a ready item a suite inside a seat dispatches");
    let inherited = format!("seat:{ORLA_ID}");

    let out = nameless_under(
        &rig,
        &["dispatch", &item, "--to", &rig.seat],
        Some(&inherited),
    )
    .output()
    .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (_, _, orders) = project.order_of(&item);
    assert_ne!(
        orders["by"],
        serde_json::json!(inherited),
        "the inherited seat acted"
    );
    assert_eq!(
        orders["by"],
        serde_json::json!(format!("seat:{}", identity_of(&rig))),
        "the machine's identity acts, not the inherited seat"
    );
}

/// The id in this rig's machine identity, as the verb left it.
fn identity_of(rig: &Rig) -> String {
    fleet_core::seat::identity::read_identity(&rig.machine)
        .expect("the identity reads")
        .expect("the machine has an identity")
        .id
        .to_string()
}

/// A VERB ALWAYS HAS AN ACTOR: with no `--by` and no `FLEET_ACTOR` it acts as
/// this machine's identity, minted here where the machine had none, and says
/// so on one stderr line naming the verb that lists it.
#[test]
fn a_dispatcher_the_call_does_not_name_is_this_machines_identity() {
    let project = Project::shared();
    let rig = Rig::new("nameless");
    rig.live();
    let item = project.item("a ready item nobody named the dispatcher of");
    assert!(!rig.machine.join("identity.toml").exists());

    let out = nameless(&rig, &["dispatch", &item, "--to", &rig.seat])
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let id = identity_of(&rig);
    let (_, _, orders) = project.order_of(&item);
    assert_eq!(orders["by"], serde_json::json!(format!("seat:{id}")));

    let said = stderr(&out);
    let short = &id[id.len() - 8..];
    let once = format!(
        "fleet dispatch: this machine had no identity, so one was minted at {}; acting as this \
         machine's identity human-{short} ({id}), which {} does not list — fleet seat add --human \
         lists it\n",
        rig.machine.join("identity.toml").display(),
        // The walk resolves from the working directory, which the platform
        // hands back canonical.
        std::fs::canonicalize(project.root.join("fleet.toml"))
            .expect("the policy file resolves")
            .display()
    );
    assert!(said.contains(&once), "{said}");
    assert_eq!(said.matches("fleet seat add --human").count(), 1, "{said}");

    // The second call reads the identity the first minted: no mint prefix.
    project.done(&item);
    let item = project.item("a second item the same machine dispatches");
    let out = nameless(&rig, &["dispatch", &item, "--to", &rig.seat])
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!(
            "fleet dispatch: acting as this machine's identity human-{short} ({id})"
        )),
        "{}",
        stderr(&out)
    );
    let (_, _, orders) = project.order_of(&item);
    assert_eq!(orders["by"], serde_json::json!(format!("seat:{id}")));
}

/// The retired variable is spelled in two halves so this suite, like the
/// code, carries the whole name nowhere (`git grep` over the crates finds
/// none of it).
const RETIRED_ACTOR_VARIABLE: &str = concat!("BEADS", "_ACTOR");

/// The variable bd once read for its actor is read by no verb: set alone, the
/// verb acts as the machine's identity and never as the name it holds.
#[test]
fn the_retired_actor_variable_is_not_read() {
    let project = Project::shared();
    let rig = Rig::new("retired-variable");
    rig.live();
    let item = project.item("a ready item under the retired variable");

    let out = nameless(&rig, &["dispatch", &item, "--to", &rig.seat])
        .env(RETIRED_ACTOR_VARIABLE, "someone")
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let (_, _, orders) = project.order_of(&item);
    assert_eq!(
        orders["by"],
        serde_json::json!(format!("seat:{}", identity_of(&rig)))
    );
    let timeline = rig.timeline(&item);
    assert_eq!(
        timeline[0]["by"],
        serde_json::json!(format!("seat:{}", identity_of(&rig))),
        "the entry's author is the identity too: {timeline:?}"
    );
}

/// The grammar of `--by` and `FLEET_ACTOR`: a seat argument resolves over the
/// roster to `seat:<id>`, a typed actor is taken as given, and what resolves to
/// no one seat — or is typed with a bad id — is refused before anything is
/// written.
#[test]
fn an_actor_is_a_seat_argument_or_a_typed_actor() {
    let project = Project::shared();
    let rig = Rig::new("actor-forms");
    rig.live();

    let given = |by: &[&str], env: Option<&str>| {
        let item = project.item("a ready item an actor arm dispatches");
        let mut call = nameless(&rig, &["dispatch", &item, "--to", &rig.seat]);
        call.args(by);
        if let Some(actor) = env {
            call.env("FLEET_ACTOR", actor);
        }
        let out = call.output().expect("the built binary runs");
        (item, out)
    };
    // The seat takes one item at a time, so each dispatch that lands frees it.

    let (item, out) = given(&["--by", "orla"], None);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        project.order_of(&item).2["by"],
        serde_json::json!(format!("seat:{ORLA_ID}"))
    );
    assert!(
        !stderr(&out).contains("fleet seat add --human"),
        "a named actor is not the identity: {}",
        stderr(&out)
    );
    project.done(&item);

    let (item, out) = given(&[], Some("orla"));
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        project.order_of(&item).2["by"],
        serde_json::json!(format!("seat:{ORLA_ID}"))
    );
    project.done(&item);

    let (item, out) = given(&["--by", "run:fleet-abc"], None);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(project.order_of(&item).2["by"], "run:fleet-abc");
    project.done(&item);

    let (item, out) = given(&["--by", "nobody"], None);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let said = stderr(&out);
    assert!(
        said.contains("fleet dispatch: --by nobody names no seat — the seats are "),
        "{said}"
    );
    for id in [LEAD_ID, ORLA_ID, TWIN_A, TWIN_B] {
        assert!(said.contains(id), "the seats are listed: {said}");
    }
    assert_eq!(project.order_of(&item).2, serde_json::Value::Null);

    let (item, out) = given(&[], Some("nobody"));
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("fleet dispatch: FLEET_ACTOR nobody names no seat"),
        "{}",
        stderr(&out)
    );
    assert_eq!(project.order_of(&item).2, serde_json::Value::Null);

    let (item, out) = given(&["--by", "twin"], None);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let said = stderr(&out);
    assert!(said.contains("--by twin names 2 seats"), "{said}");
    assert!(said.contains(TWIN_A) && said.contains(TWIN_B), "{said}");
    assert_eq!(project.order_of(&item).2, serde_json::Value::Null);

    let (item, out) = given(&["--by", "seat:not-a-uuid"], None);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("`seat:not-a-uuid` is a typed actor with a bad id — "),
        "{}",
        stderr(&out)
    );
    assert_eq!(project.order_of(&item).2, serde_json::Value::Null);
}

/// An identity the roster lists is a person the fleet knows: the verb acts as
/// it and says nothing about `fleet seat add --human`.
#[test]
fn a_listed_identity_acts_without_the_once_line() {
    let project = Project::shared();
    let rig = Rig::new("listed-identity");
    rig.live();
    std::fs::write(
        rig.machine.join("identity.toml"),
        format!("id = \"{LISTED_IDENTITY}\"\nkind = \"human\"\n"),
    )
    .expect("the identity is written");
    let item = project.item("a ready item a listed person dispatches");

    let out = nameless(&rig, &["dispatch", &item, "--to", &rig.seat])
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        !stderr(&out).contains("fleet seat add --human"),
        "{}",
        stderr(&out)
    );
    assert!(
        !stderr(&out).contains("acting as this machine's identity"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        project.order_of(&item).2["by"],
        serde_json::json!(format!("seat:{LISTED_IDENTITY}"))
    );
}

#[test]
fn a_lock_on_dispatch_is_a_usage_error() {
    let rig = Rig::new("lock");
    let call = [
        "dispatch",
        "fx-nope",
        "--to",
        rig.seat.as_str(),
        "--by",
        "lead-1",
    ];

    let out = rig.run(&[&call[..], &["--lock", "/nonexistent"]].concat());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--lock"), "{}", stderr(&out));

    // The control: without --lock the call gets past the usage gate and refuses
    // on the record instead.
    let out = rig.run(&call);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
}

#[test]
fn a_by_on_brief_is_a_usage_error() {
    let project = Project::shared();
    let rig = Rig::new("brief-by");
    let item = project.ordered("a ready item briefed with a name");

    let out = rig.run(&["brief", &item, "--by", "lead-1"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--by"), "{}", stderr(&out));

    let out = rig.run(&["brief", &item]);
    assert_eq!(out.status.code(), Some(0), "the control: {}", stderr(&out));
}

#[test]
fn each_verbs_help_lists_only_the_options_it_reads() {
    let help = |verb: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args([verb, "--help"])
            .hermetic_nowhere()
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let dispatch = help("dispatch");
    assert!(dispatch.contains("--by"), "{dispatch}");
    assert!(dispatch.contains("--touched"), "{dispatch}");
    assert!(!dispatch.contains("--lock"), "{dispatch}");

    let brief = help("brief");
    assert!(brief.contains("--to"), "{brief}");
    assert!(brief.contains("--touched"), "{brief}");

    let land = help("land");
    assert!(land.contains("--test"), "{land}");
    assert!(!brief.contains("--by"), "{brief}");
    assert!(!brief.contains("--lock"), "{brief}");
}

#[test]
fn brief_prints_the_first_turn_and_says_what_it_cost() {
    let project = Project::shared();
    let rig = Rig::new("brief");
    let item = project.ordered("a ready item with an order on it");

    let out = rig.run(&["brief", &item, "--touched", "make check"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(body.contains(&item), "{body}");
    let defaults = ShippedDefaults::new("brief");
    let template = std::fs::read_to_string(defaults.path().join("assets/brief.md"))
        .expect("the shipped brief is readable");
    assert_eq!(
        body.lines().next(),
        template
            .lines()
            .next()
            .map(|line| line.replace("{item_id}", &item))
            .as_deref(),
        "the first line is the one only `{{item_id}}` fills"
    );
    assert!(body.contains("(transient)"), "{body}");
    assert!(
        body.contains("```\nmake check\n```"),
        "the builder's checks the call handed in: {body}"
    );
    assert_eq!(
        stderr(&out).trim(),
        format!("brief: {} bytes", out.stdout.len())
    );
}

/// `brief --to` resolves its argument among the seats this machine runs, as
/// dispatch's does, and the brief names the seat by the machine name it
/// resolved to; a name no running seat answers to is refused, exit 1.
#[test]
fn brief_to_a_seat_says_its_machine_name_and_a_stranger_is_refused() {
    let project = Project::shared();
    let rig = Rig::new("brief-to");
    let item = project.ordered("a ready item briefed for a named seat");

    let out = rig.run(&["brief", &item, "--to", &rig.seat]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let body = String::from_utf8_lossy(&out.stdout);
    assert!(
        body.contains(&format!("You are `{}-93b9739a`", rig.seat)),
        "{body}"
    );

    let out = rig.run(&["brief", &item, "--to", "s-cli-nobody"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("s-cli-nobody names no seat"),
        "{}",
        stderr(&out)
    );
    assert!(out.stdout.is_empty(), "a refused brief prints nothing");
}
