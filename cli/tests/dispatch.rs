//! `fleet dispatch` through the shipped binary, with the ring reaching a stub
//! that stands in for the provider (packs PRD R4).
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
const POLICY: &str = "[controller]\nnudge_model = \"a-cheap-model\"\n\
                      nudge_timeout_seconds = 20\n";

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

    /// An item carrying the order note `brief` refuses to render without.
    fn ordered(&self, title: &str) -> String {
        let item = self.item(title);
        assert!(self
            .bd(&[
                "note",
                &item,
                "dispatched by lead-1 — orders given",
                "--actor",
                "lead-1",
            ])
            .status
            .success());
        item
    }

    /// The three fields a dispatch writes, read back off the store.
    fn order_of(&self, item: &str) -> (Option<String>, String, serde_json::Value) {
        let out = self.bd(&["-q", "show", item, "--json"]);
        let text = String::from_utf8_lossy(&out.stdout);
        let value: serde_json::Value =
            serde_json::from_str(text.trim()).expect("bd show answers JSON");
        let row = &value[0];
        (
            row.get("assignee")
                .and_then(|a| a.as_str())
                .map(str::to_string),
            row.get("notes")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string(),
            row.get("metadata")
                .and_then(|m| m.get("orders"))
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
                     {{"name": "{seat}", "chosen_name": "Orla",
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
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "dispatched by lead-1 — orders given\n"
    );
    // The builder's gate the call handed in is the one the brief names.
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
        argv.contains("Orla"),
        "the seat is addressed by its name:\n{argv}"
    );

    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(assignee.as_deref(), Some(rig.seat.as_str()));
    assert!(notes.contains("orders given"), "{notes}");
    assert_eq!(orders["seat"], serde_json::json!(rig.seat));
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
        "the note line is stdout's on success only"
    );
    assert!(
        rig.nudge_argv().is_empty(),
        "an absent seat is not rung at all"
    );

    let (assignee, notes, orders) = project.order_of(&item);
    assert_eq!(
        assignee.as_deref(),
        Some(rig.seat.as_str()),
        "the assignment stands"
    );
    assert!(notes.contains("orders given"), "the note stands: {notes}");
    assert_eq!(
        orders["kind"],
        serde_json::json!("dispatch"),
        "the index stands"
    );
    // A dispatch handed no builder's gate: the brief names the absence where
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
    assert_eq!(assignee.as_deref(), Some(rig.seat.as_str()));
    assert!(notes.contains("orders given"), "{notes}");
    assert_eq!(orders["by"], serde_json::json!("lead-1"));
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
    assert!(
        notes.contains("DISPATCH WITHDRAWN"),
        "the withdrawal is on the record: {notes}"
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
    assert!(
        notes.contains("DISPATCH COULD NOT TELL"),
        "the could-not-tell is on the record: {notes}"
    );
    assert!(
        !notes.contains("DISPATCH WITHDRAWN"),
        "and the withdrawal is not: {notes}"
    );
}

#[test]
fn a_dispatcher_the_call_does_not_name_is_a_usage_error() {
    let rig = Rig::new("nameless");
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["dispatch", "fx-1", "--to", &rig.seat])
        .current_dir(&Project::shared().root)
        .hermetic(&rig.root.join("home"), &rig.machine, None)
        .env_remove("FLEET_ACTOR")
        .env_remove("BEADS_ACTOR")
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--by"), "{}", stderr(&out));

    // The control: the same call with the environment naming an actor gets past
    // the usage gate and refuses on the record instead.
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["dispatch", "fx-nope", "--to", &rig.seat])
        .arg("--packs-dir")
        .arg(rig.machine.join("packs"))
        .current_dir(&Project::shared().root)
        .hermetic(&rig.root.join("home"), &rig.machine, None)
        .env("BEADS_ACTOR", "lead-1")
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
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
        "the builder's gate the call handed in: {body}"
    );
    assert_eq!(
        stderr(&out).trim(),
        format!("brief: {} bytes", out.stdout.len())
    );
}
