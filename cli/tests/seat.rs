//! The `seat` noun through the shipped binary (cli PRD § Seat).
//!
//! Every exit here is read from the CHILD's own status, and every effect from
//! the machine the child left behind — the worktree on disk, the two rows in
//! their files, the stub's own record of what it was called with. Nothing is
//! read from what the verb printed about itself.
//!
//! The project is a real git repository with a local `refs/remotes/origin/main`,
//! because a spawn cuts its worktree from that ref, and one per arm: the
//! worktrees directory is keyed on the project and two arms sharing one would
//! each meet the other's `transient-1`.

mod common;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::platform;
use fleet_core::item::land::{CRITERIA, SAFE, WORK_BRANCH};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A load reading and a cpu count that let the belt through on any machine.
const CALM: [(&str, &str); 2] = [("FLEET_LOAD_AVERAGE", "0.1"), ("FLEET_CPUS", "8")];

/// The pair the two belt-leg arms force: 3.75 against a ceiling of 4, so the
/// belt lets a spawn through, and a reading nothing else in this file uses, so
/// a number either of them reads back can only have come from the belt.
const HELD: [(&str, &str); 2] = [("FLEET_LOAD_AVERAGE", "3.75"), ("FLEET_CPUS", "4")];

/// A pid no process on this box holds, read the way the verb under test reads
/// it.
///
/// `seat retire` checks the pid its roster row carried with `kill -0` after the
/// stop, and refuses over one that still answers — so a row carrying a number
/// this rig did not choose is green only while the box happens not to hold it.
/// The pid here is a child this rig spawned and reaped, which makes it gone by
/// construction; the loop is for the one case construction does not cover, a
/// box that handed the number back out between the reap and the read.
fn a_pid_that_is_gone() -> u32 {
    /// One try is the ordinary count; the rest are for that recycling box.
    const TRIES: usize = 16;

    for _ in 0..TRIES {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("a shell that exits at once is spawned");
        let pid = child.id();
        child.wait().expect("the child is reaped");
        if platform::process_alive(pid) == Some(false) {
            return pid;
        }
    }
    panic!("no pid out of {TRIES} this rig spawned and reaped read as gone");
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

struct Rig {
    /// This arm's own name, which is also the name of the board it takes: no
    /// two arms here share one.
    label: String,
    root: PathBuf,
    project: PathBuf,
    worktrees: PathBuf,
    machine: PathBuf,
    stub: PathBuf,
    roster: PathBuf,
    calls: PathBuf,
    turn: PathBuf,
    start_exit: PathBuf,
    /// The pid every roster row this rig writes carries, taken at the first row
    /// and held so the arms that assert on what a verb printed of it read the
    /// same number the row was written with.
    pid: Cell<Option<u32>>,
}

impl Rig {
    /// `worktrees_key` decides whether the project's own file names the two
    /// directories or leaves them to be derived from the root.
    fn new(label: &str, declared: bool) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-seat-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            label: label.to_string(),
            project: root.join("a-project"),
            // The DERIVED answer's own spelling, so the arm that declares
            // nothing and the arm that declares this path land in one place.
            worktrees: root.join("a-project-worktrees"),
            machine: root.join("machine"),
            stub: root.join("agent.sh"),
            roster: root.join("roster.json"),
            calls: root.join("calls"),
            turn: root.join("a-turn.md"),
            start_exit: root.join("start-exit"),
            pid: Cell::new(None),
            root,
        };
        for dir in [&rig.project, &rig.machine] {
            std::fs::create_dir_all(dir).expect("the fixture directory is created");
        }
        defaults_into(&rig.machine);
        std::fs::write(
            rig.project.join("fleet.toml"),
            if declared {
                format!(
                    "[permissions]\n\n\
                     [controller]\nnudge_model = \"a-cheap-model\"\n\
                     nudge_timeout_seconds = 30\nstart_watch_seconds = 10\n\
                     default_model = \"a-model\"\n\n\
                     [project]\nprimary = {primary}\nworktrees = {worktrees}\n",
                    primary = toml_string(&rig.project.display().to_string()),
                    worktrees = toml_string(&rig.worktrees.display().to_string()),
                )
            } else {
                "[permissions]\n\n\
                 [controller]\nnudge_model = \"a-cheap-model\"\n\
                 nudge_timeout_seconds = 30\nstart_watch_seconds = 10\n\
                 default_model = \"a-model\"\n"
                    .to_string()
            },
        )
        .expect("the policy is written");
        std::fs::write(&rig.turn, "the turn this seat comes up on\n")
            .expect("the first turn is written");
        rig.init_repo();
        std::fs::write(
            rig.machine.join("config.json"),
            format!(
                "{{\"fleet_toml\": {policy}, \"children\": []}}\n",
                policy = json_string(&rig.project.join("fleet.toml").display().to_string()),
            ),
        )
        .expect("the seat list is written");
        rig.write_stub();
        rig.roster("[]");
        rig
    }

    /// The project's declared toolchain, written into the policy this rig's
    /// own spawn reads.
    ///
    /// Written AFTER construction rather than through `new`, so an arm that
    /// declares a list and one that declares none differ in this one call and
    /// in nothing else. The replacement is asserted to have landed: a fixture
    /// edit that matched nothing would leave every arm here measuring the
    /// undeclared case and passing.
    fn declaring(&self, words: &[&str]) {
        let path = self.project.join("fleet.toml");
        let policy = std::fs::read_to_string(&path).expect("the policy is readable");
        let list = words
            .iter()
            .map(|word| toml_string(word))
            .collect::<Vec<_>>()
            .join(", ");
        let declared = policy.replace(
            "[permissions]\n",
            &format!("[permissions]\ntool_commands = [{list}]\n"),
        );
        assert!(
            declared.contains("tool_commands"),
            "the fixture's [permissions] table is the one this helper edits: {policy}"
        );
        std::fs::write(&path, declared).expect("the declaration is written");
    }

    fn init_repo(&self) {
        self.git(&["init", "--quiet", "--initial-branch", "main"]);
        std::fs::write(self.project.join("a-file.txt"), "the trunk\n")
            .expect("the file is written");
        self.git(&["add", "--", "a-file.txt", "fleet.toml"]);
        self.git(&["commit", "--quiet", "--no-gpg-sign", "-m", "the trunk"]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    }

    /// The work graph, for the arms that dispatch. Made after the repository so
    /// the store's own files are not what the trunk commit carries, and the
    /// repository the rig committed into is the one it keeps.
    ///
    /// A BOARD OF ITS OWN and never the run's shared one. A SEAT NAME IS
    /// BOARD-WIDE and every arm in this file spawns `transient-1`: a retire
    /// asks the board which open ordered items that name holds and clears
    /// them, so two arms on one board have one arm's retire withdraw the order
    /// the other's dispatch has just written — and both read back a
    /// disagreement. What it costs the run is one `bd init` for each arm here
    /// that has a store, which the wrapper's own count line names.
    fn init_store(&self) {
        common::take_a_board_alone(&self.project, &self.label);
    }

    fn bd(&self, args: &[&str]) -> Output {
        Command::new("bd")
            .arg("-C")
            .arg(&self.project)
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
        let value: serde_json::Value =
            serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
                .expect("bd create answers JSON");
        value["id"].as_str().expect("an id").to_string()
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

    fn order_of(&self, item: &str) -> (Option<String>, String, serde_json::Value) {
        let out = self.bd(&["-q", "show", item, "--json"]);
        let value: serde_json::Value =
            serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
                .expect("bd show answers JSON");
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
                .and_then(|m| m.get("fleet.orders"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )
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

    /// The drive suite's stub shape: the start records its argv and the roster
    /// branch serves a file the arm writes.
    fn write_stub(&self) {
        std::fs::write(
            &self.stub,
            format!(
                "#!/bin/sh\n\
                 case \"$1\" in\n\
                 \x20 agents) /bin/cat '{roster}' ;;\n\
                 \x20 --bg)\n\
                 \x20   printf '%s\\n' \"$@\" >> '{calls}'\n\
                 \x20   echo 'START' >> '{calls}'\n\
                 \x20   exit $(/bin/cat '{start_exit}' 2>/dev/null || echo 0)\n\
                 \x20   ;;\n\
                 \x20 stop) echo \"STOP $2\" >> '{calls}'; printf '[]' > '{roster}' ;;\n\
                 \x20 rm) echo \"RM $2\" >> '{calls}' ;;\n\
                 \x20 -p) echo \"NUDGE\" >> '{calls}' ;;\n\
                 \x20 *) exit 64 ;;\n\
                 esac\n",
                roster = self.roster.display(),
                calls = self.calls.display(),
                start_exit = self.start_exit.display(),
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

    /// A pack installed ABOVE the binary's own defaults, carrying a manifest
    /// and whatever files the arm hands it.
    ///
    /// EVERY installed pack layers on top: the bottom is the defaults directory
    /// the resolver appends, and no installed name is special — so the
    /// directory and the name here are only what a refusal would call the pack
    /// by (any name but `defaults`, which `resolve` refuses).
    fn pack(&self, name: &str, files: &[(&str, &str)]) {
        let root = self.machine.join("packs").join(name);
        std::fs::create_dir_all(&root).expect("the pack root is created");
        std::fs::write(
            root.join("pack.toml"),
            format!(
                "[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n\
                 description = \"a pack this arm layers over the defaults\"\n"
            ),
        )
        .expect("the manifest is written");
        for (relative, contents) in files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                .expect("the parent is created");
            std::fs::write(&path, contents).expect("the pack's file is written");
        }
    }

    /// Rewrite the project's `[project]` block to whatever the arm wants to
    /// declare, for the arms about how a declaration is read.
    fn declare_project(&self, block: &str) {
        let path = self.project.join("fleet.toml");
        let body = std::fs::read_to_string(&path).expect("the policy is readable");
        let head = body.split("[project]").next().unwrap_or_default();
        std::fs::write(&path, format!("{head}[project]\n{block}"))
            .expect("the policy is rewritten");
    }

    /// The pid this rig's roster rows carry — chosen at the first row written
    /// and the one source every arm here reads it from.
    fn pid(&self) -> u32 {
        match self.pid.get() {
            Some(pid) => pid,
            None => {
                let pid = a_pid_that_is_gone();
                self.pid.set(Some(pid));
                pid
            }
        }
    }

    /// A roster carrying one live idle row in the PROJECT's own checkout: a
    /// session that is no transient seat's worktree, so a cap leg read against
    /// it counts rows it read rather than an empty listing.
    fn live_in_the_checkout(&self) -> &Rig {
        self.roster(&format!(
            "[{{\"sessionId\": \"a-session\", \"id\": \"ab12\", \"cwd\": {cwd}, \
              \"pid\": {pid}, \"status\": \"idle\"}}]",
            cwd = json_string(&self.project.display().to_string()),
            pid = self.pid(),
        ))
    }

    /// A roster carrying one live idle row in this seat's worktree.
    fn live(&self, seat: &str, status: &str) -> &Rig {
        self.roster(&format!(
            "[{{\"sessionId\": \"a-session\", \"id\": \"ab12\", \"cwd\": {cwd}, \
              \"pid\": {pid}, \"status\": {status}}}]",
            cwd = json_string(&self.worktrees.join(seat).display().to_string()),
            pid = self.pid(),
            status = json_string(status),
        ))
    }

    /// The shipped binary, with the environment every arm here shares.
    ///
    /// The two git-config variables are neutralised the way `rig.git()`
    /// neutralises them: the verb runs `git worktree add` itself, and a box
    /// whose global config sets `core.hooksPath`, a template directory or a
    /// default branch would otherwise decide what this arm measures.
    fn run(&self, args: &[&str]) -> Output {
        self.run_at(args, &CALM)
    }

    /// The same, at a load and a cpu count the ARM names rather than the calm
    /// pair every other arm shares. An arm that asserts on a printed reading
    /// has to force one nothing else uses, or a hard-coded pair would satisfy
    /// it.
    fn run_at(&self, args: &[&str], readings: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fleet"));
        command
            .args(args)
            .current_dir(&self.project)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub));
        for (key, value) in readings {
            command.env(key, value);
        }
        command.output().expect("the built binary runs")
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.calls).unwrap_or_default()
    }

    fn seats(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine.join("config.json"))
            .expect("the seat list is readable");
        serde_json::from_str(&body).expect("the seat list parses")
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

fn toml_string(value: &str) -> String {
    json_string(value)
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The three verbs end to end through the shipped binary, each exit read from
/// the child's own status.
#[test]
fn the_three_verbs_run_end_to_end_through_the_shipped_binary() {
    let rig = Rig::new("end-to-end", true);

    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));
    assert_eq!(
        stdout(&spawned).trim(),
        "transient-1",
        "the name, alone, is the verb's answer on stdout"
    );
    assert!(rig.worktrees.join("transient-1").is_dir());
    let row = &rig.seats()["children"][0];
    assert_eq!(row["name"], "transient-1");
    assert_eq!(row["transient"], true);
    assert_eq!(row["model"], "a-model");
    assert!(rig.calls().contains("START"), "{}", rig.calls());

    // The feed: a live idle row takes the next turn.
    rig.live("transient-1", "idle");
    let next = rig.root.join("the-next-turn.md");
    std::fs::write(&next, "the next turn\n").expect("the turn is written");
    let fed = rig.run(&[
        "seat",
        "feed",
        "transient-1",
        "--first-turn",
        &next.display().to_string(),
    ]);
    assert_eq!(fed.status.code(), Some(0), "{}", stderr(&fed));
    assert!(rig.calls().contains("NUDGE"), "{}", rig.calls());

    // The retire: stopped, removed, both rows dropped, the reclaim printed.
    let retired = rig.run(&["seat", "retire", "transient-1"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));
    assert!(
        stdout(&retired).contains("reclaimed"),
        "the reclaim is printed: {}",
        stdout(&retired)
    );
    assert!(rig.calls().contains("STOP ab12"), "{}", rig.calls());
    assert!(rig.calls().contains("RM ab12"), "{}", rig.calls());
    assert!(!rig.worktrees.join("transient-1").exists());
    assert_eq!(
        rig.seats()["children"].as_array().map(Vec::len),
        Some(0),
        "the seat-list row is dropped"
    );
}

/// The slot the pack layers carry a transient seat's permission rules in, named
/// once here because three arms read it and the defaults' registry lists it by
/// this exact string: a rename of the slot that missed one of them reds these
/// rather than shipping a seat with no rules.
const PERMISSIONS: &str = "overlay/per-provider/claude/permissions.json";

/// The rules a transient seat comes up under, through the shipped binary and
/// against the BUNDLED pack: a session started under this fleet's posture
/// refuses every writing call it holds no rule for, so the spawn renders the
/// overlay's document into the seat's own worktree before the first turn
/// (packs PRD § The first slice).
///
/// The overlay is read off the tree rather than retyped, because the whole
/// claim is that the file in the worktree is that file with two values in it —
/// and the two placeholder assertions under it are the control that makes the
/// comparison a reading rather than a template agreeing with itself.
#[test]
fn a_spawn_renders_the_packs_permission_rules_into_the_seats_worktree() {
    let rig = Rig::new("permissions", true);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--touched",
        "make check",
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let path = worktree.join(".claude/settings.local.json");
    let written = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let defaults = ShippedDefaults::new("overlay");
    let overlay = defaults.path().join(PERMISSIONS);
    let template = std::fs::read_to_string(&overlay)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", overlay.display()));
    assert_eq!(
        written,
        template
            .replace("{touched}", "make check")
            .replace("{worktree}", &worktree.display().to_string()),
        "the seat's settings are the pack's document with the builder's gate and the worktree \
         rendered in"
    );

    assert!(
        !written.contains("{touched}") && !written.contains("{worktree}"),
        "no placeholder survives into the seat's own settings: {written}"
    );
    assert!(
        written.contains("Bash(make check:*)")
            && written.contains(&format!("Edit(/{}/**)", worktree.display())),
        "the builder's gate the spawn was handed and the seat's own checkout are both in a rule: \
         {written}"
    );

    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings parse as JSON");
    assert!(
        doc["permissions"]["allow"]
            .as_array()
            .is_some_and(|rules| !rules.is_empty()),
        "and the document the provider reads carries an allow list: {written}"
    );
}

/// A spawn handed NO builder's gate writes no rule for one — the entry that
/// would carry it is taken out, and nothing else is — and a gate that carries a
/// quote is written into its rule as JSON, so the document still parses.
#[test]
fn a_spawn_handed_no_touched_command_writes_no_rule_for_one() {
    let rig = Rig::new("permissions-untouched", true);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let written = std::fs::read_to_string(worktree.join(".claude/settings.local.json"))
        .expect("the seat's settings are written");
    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings parse as JSON");
    let allow = |doc: &serde_json::Value| -> Vec<String> {
        doc["permissions"]["allow"]
            .as_array()
            .expect("the document carries an allow list")
            .iter()
            .map(|rule| rule.as_str().unwrap_or_default().to_string())
            .collect()
    };
    let defaults = ShippedDefaults::new("overlay-untouched");
    let template: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(defaults.path().join(PERMISSIONS))
            .expect("the overlay is readable"),
    )
    .expect("the template is JSON");
    let wanted: Vec<String> = allow(&template)
        .into_iter()
        .filter(|rule| !rule.contains("{touched}"))
        .map(|rule| rule.replace("{worktree}", &worktree.display().to_string()))
        .collect();
    assert_eq!(
        allow(&doc),
        wanted,
        "the pack's list, less the one rule nobody handed a command for: {written}"
    );
    assert_eq!(doc["permissions"]["deny"], template["permissions"]["deny"]);

    // A gate carrying a quote is escaped into its rule, not spliced into the
    // document's syntax.
    let rig = Rig::new("permissions-quoted", true);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--touched",
        "make check ARGS=\"-p core\"",
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));
    let written = std::fs::read_to_string(
        rig.worktrees
            .join("transient-1")
            .join(".claude/settings.local.json"),
    )
    .expect("the seat's settings are written");
    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings still parse as JSON");
    assert!(
        allow(&doc).contains(&"Bash(make check ARGS=\"-p core\":*)".to_string()),
        "the gate is one rule, quote and all: {written}"
    );
}

/// The words a project declares its seats need become rules AFTER the pack's
/// own list, and the pack's own list carries the plain read verbs.
///
/// Both halves matter and neither implies the other: the read verbs belong to
/// every seat on any project, so a project that forgets to declare them still
/// gets them; the toolchain is the project's, so a pack never hardcodes one.
/// The list stays a list — the assertion that no rule is a wildcard over Bash
/// is what the mutant rendering the whole toolchain as one `Bash(*)` reds.
#[test]
fn a_projects_declared_tool_commands_are_rules_after_the_packs_own() {
    let rig = Rig::new("tool-commands", true);
    rig.declaring(&["make", "cargo", "sh"]);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let path = worktree.join(".claude/settings.local.json");
    let written = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings parse as JSON");
    let allow: Vec<String> = doc["permissions"]["allow"]
        .as_array()
        .expect("the document carries an allow list")
        .iter()
        .map(|rule| rule.as_str().unwrap_or_default().to_string())
        .collect();

    // The defaults, which no project declared.
    for verb in ["ls", "grep", "sed", "mkdir", "cd", "which", "command"] {
        assert!(
            allow.iter().any(|rule| rule == &format!("Bash({verb}:*)")),
            "the pack's own list carries the read verb `{verb}`: {written}"
        );
    }

    // The project's, in the order declared, after the pack's own.
    let at = |rule: &str| allow.iter().position(|held| held == rule);
    let default = at("Bash(bd:*)").expect("the pack's own rule is still in the list");
    let make = at("Bash(make:*)").expect("the declared `make` is a rule");
    let cargo = at("Bash(cargo:*)").expect("the declared `cargo` is a rule");
    assert!(
        default < make && make < cargo,
        "the project's words come after the pack's own list, in the order declared: {written}"
    );

    // A word the defaults already carry is not added twice.
    assert_eq!(
        allow.iter().filter(|rule| *rule == "Bash(sh:*)").count(),
        1,
        "`sh` is deduplicated against the list it is already in: {written}"
    );

    // The list stays a list.
    assert!(
        !allow
            .iter()
            .any(|rule| rule == "Bash(*)" || rule == "Bash(*:*)"),
        "no rule is a wildcard over Bash: {written}"
    );
}

/// An entry that is not one command word is refused AT THE RENDER, naming the
/// entry — and no seat comes up.
///
/// The three shapes are one rule each: a space would make the rule match a
/// command line rather than a command, a glob would widen it past anything the
/// project wrote down, and a leading dash is an option wearing a word's place.
#[test]
fn a_tool_command_that_is_not_one_word_is_refused_at_the_render() {
    for entry in ["make check", "make*", "-rf"] {
        let rig = Rig::new("tool-commands-refused", true);
        rig.declaring(&[entry]);
        let spawned = rig.run(&[
            "seat",
            "spawn",
            "--first-turn",
            &rig.turn.display().to_string(),
        ]);
        assert_ne!(
            spawned.status.code(),
            Some(0),
            "a list carrying `{entry}` does not spawn a seat"
        );
        let said = stderr(&spawned);
        assert!(
            said.contains(entry) && said.contains("is not one command word"),
            "the refusal names the entry `{entry}`: {said}"
        );
        assert!(
            !rig.worktrees
                .join("transient-1")
                .join(".claude/settings.local.json")
                .exists(),
            "and no seat came up under a document this refusal never wrote ({entry})"
        );
    }
}

/// The document a pack ABOVE the defaults carries at the permission slot is the
/// one a transient seat comes up under, and it replaces the default WHOLE list
/// rather than adding to it (packs PRD § Shadowing; the registry lists the
/// slot).
///
/// The fixture differs from [`a_pack_above_the_defaults_carrying_no_permission_rules_leaves_the_default_document_in_place`]
/// in exactly one file, so the pair is a comparison and not two assertions: this
/// one's pack carries the slot, that one's carries nothing, and both spawn the
/// same way.
#[test]
fn a_pack_above_the_defaults_shadowing_the_permission_slot_is_what_the_seat_comes_up_under() {
    // The registry lists this path, or the layering refuses the shadow instead of
    // resolving it — read here so a rename of the slot reds this arm at the
    // registry rather than at an unexplained refusal from the spawn.
    let defaults = ShippedDefaults::new("registry");
    let registry = fleet_core::registry::read(defaults.path())
        .expect("the defaults publish a shadow registry")
        .expect("the shadow registry parses");
    assert!(
        registry.lists(PERMISSIONS),
        "the registry lists `{PERMISSIONS}` as shadowable: {:?}",
        registry.shadows
    );

    let rig = Rig::new("shadowed-permissions", true);
    rig.pack("zeta", &[(PERMISSIONS, SHADOW_RULES)]);

    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--touched",
        "make check",
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let path = worktree.join(".claude/settings.local.json");
    let written = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));

    assert_eq!(
        written,
        SHADOW_RULES
            .replace("{touched}", "make check")
            .replace("{worktree}", &worktree.display().to_string()),
        "the seat's settings are the SHADOWING pack's document with the builder's gate and the \
         worktree rendered in"
    );
    assert!(
        written.contains(SHADOW_MARK),
        "the shadowing pack's own rule is in the document: {written}"
    );
    assert!(
        !written.contains(DEFAULT_MARK),
        "and the default list is replaced whole rather than merged into: {written}"
    );
    assert!(
        !written.contains("{touched}") && !written.contains("{worktree}"),
        "no placeholder survives into the seat's own settings: {written}"
    );

    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings parse as JSON");
    assert_eq!(
        doc["permissions"]["allow"].as_array().map(Vec::len),
        Some(3),
        "and the allow list the provider reads is the shadow's three rules: {written}"
    );
}

/// THE CONTROL for the arm above: the same fixture, the same second pack, and
/// the one file removed from it.
///
/// Without this, an arm asserting the shadow's content proves only that some
/// file was read — a spawn that always read the highest layer, or that read the
/// shadow by luck of a directory walk, would pass it. Here the second pack
/// carries nothing at the slot and the default document is what the seat comes
/// up under.
#[test]
fn a_pack_above_the_defaults_carrying_no_permission_rules_leaves_the_default_document_in_place() {
    let rig = Rig::new("unshadowed-permissions", true);
    rig.pack("zeta", &[]);

    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--touched",
        "make check",
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let path = worktree.join(".claude/settings.local.json");
    let written = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));

    let defaults = ShippedDefaults::new("overlay");
    let overlay = defaults.path().join(PERMISSIONS);
    let template = std::fs::read_to_string(&overlay)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", overlay.display()));
    assert_eq!(
        written,
        template
            .replace("{touched}", "make check")
            .replace("{worktree}", &worktree.display().to_string()),
        "a pack above the defaults that carries no permission rules leaves theirs in place"
    );
    assert!(
        written.contains(DEFAULT_MARK),
        "the defaults' own rule is in the document: {written}"
    );
    assert!(
        !written.contains(SHADOW_MARK),
        "and nothing of the shadow fixture is: {written}"
    );
}

/// The shadowing pack's document: the default shape, none of the default rules, and both
/// placeholders, so the arm reading it is reading a rendering and not a copy.
const SHADOW_RULES: &str = r#"{
  "permissions": {
    "allow": [
      "Bash(the-shadowing-packs-own-verb:*)",
      "Bash({touched}:*)",
      "Edit(/{worktree}/**)"
    ],
    "deny": [
      "AskUserQuestion"
    ]
  }
}
"#;

/// One rule out of each document, so an arm can say which one was read rather
/// than only that the documents differ.
const SHADOW_MARK: &str = "Bash(the-shadowing-packs-own-verb:*)";
const DEFAULT_MARK: &str = "Bash(bd:*)";

/// The five trunk-push shapes the default permissions document denies, spelled
/// exactly as `tools/spawn-builder`'s `SPAWN_DENY` spells them — the
/// product-of-the-fleet half of that list; its launchctl and porter shapes
/// stay this repository's own and are not the defaults' to deny.
const TRUNK_PUSH_DENY: [&str; 5] = [
    "Bash(git push origin HEAD:main*)",
    "Bash(git push origin main*)",
    "Bash(git push --force*)",
    "Bash(git push -f *)",
    "Bash(git push * --delete*)",
];

/// A transient seat comes up denied the five trunk-push shapes, read from the
/// FILE THE SPAWN WROTE and never from the pack — so a merge or a render step
/// that silently dropped one of the five reds here even though the pack's own
/// document on disk is untouched.
#[test]
fn a_spawn_denies_the_five_trunk_push_shapes_to_a_transient_seat() {
    let rig = Rig::new("trunk-push-deny", true);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let path = worktree.join(".claude/settings.local.json");
    let written = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings parse as JSON");
    let deny: Vec<&str> = doc["permissions"]["deny"]
        .as_array()
        .expect("a deny list")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for shape in TRUNK_PUSH_DENY {
        assert!(
            deny.contains(&shape),
            "the seat's own written settings deny `{shape}`: {written}"
        );
    }
}

/// A project that already tracks its own `.claude/settings.local.json` on the
/// trunk — carrying a deny rule of its own — keeps that rule beside the five
/// once the spawn's merge folds the pack's rules in
/// (`transient.rs`'s `merged_settings`).
#[test]
fn a_project_settings_file_that_already_denies_a_shape_keeps_it_beside_the_five() {
    let rig = Rig::new("trunk-push-deny-merge", true);
    let settings_dir = rig.project.join(".claude");
    std::fs::create_dir_all(&settings_dir).expect("the .claude directory is created");
    std::fs::write(
        settings_dir.join("settings.local.json"),
        "{\n  \"permissions\": {\n    \"allow\": [],\n    \"deny\": [\n      \
         \"Bash(rm -rf /*)\"\n    ]\n  }\n}\n",
    )
    .expect("the project's own settings are written");
    // `-f`: this box's own `~/.config/git/ignore` excludes
    // `**/.claude/settings.local.json` by default, which is a fact about the
    // machine running the suite and not about the fixture's repository.
    rig.git(&["add", "-f", "--", ".claude/settings.local.json"]);
    rig.git(&[
        "commit",
        "--quiet",
        "--no-gpg-sign",
        "-m",
        "track the project's own deny rule",
    ]);
    rig.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);

    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let worktree = rig.worktrees.join("transient-1");
    let path = worktree.join(".claude/settings.local.json");
    let written = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&written).expect("the seat's settings parse as JSON");
    let deny: Vec<&str> = doc["permissions"]["deny"]
        .as_array()
        .expect("a deny list")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        deny.contains(&"Bash(rm -rf /*)"),
        "the project's own pre-existing deny rule survives the merge: {written}"
    );
    for shape in TRUNK_PUSH_DENY {
        assert!(
            deny.contains(&shape),
            "the pack's `{shape}` is folded in beside the project's own rule: {written}"
        );
    }
}

/// With the project's own file naming neither directory, the primary is the
/// project root and the worktrees directory is its `-worktrees` sibling.
#[test]
fn a_project_that_declares_neither_directory_derives_both_from_its_root() {
    let rig = Rig::new("derived", false);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));
    assert!(
        rig.worktrees.join("transient-1").is_dir(),
        "the derived sibling is {}",
        rig.worktrees.display()
    );
    // And the worktree is registered in the project root, which is the derived
    // primary: a `git worktree list` there names it.
    let listed = rig.git(&["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains("transient-1"),
        "the derived primary is the project root: {listed}"
    );
}

/// A DECLARED PROJECT WINS AT ITS OWN LEVEL (controller PRD § Two modes): a directory
/// carrying its own `.fleet/project.toml` resolves standalone even with a
/// `fleet.toml` beside it, so the project's NAME and its worktrees directory are
/// the declaration's.
///
/// The two files disagree on both — the neighbour names the rig's own worktrees
/// directory and leaves the name to the basename — so each assertion is a
/// reading of one file rather than two answers that happen to agree.
#[test]
fn a_declaration_beside_a_fleet_toml_resolves_standalone() {
    let rig = Rig::new("declared-beside", true);
    let declared = rig.root.join("declared-worktrees");
    let path = rig.project.join(".fleet/project.toml");
    std::fs::create_dir_all(path.parent().expect("the declaration sits under .fleet"))
        .expect("the declaration's directory is made");
    std::fs::write(
        &path,
        format!(
            "[project]\nname = \"a-declared-project\"\nitem_prefix = \"dp\"\n\
             primary = {primary}\nworktrees = {worktrees}\n\n\
             [landing]\nci_marker = \"printf '[skip ci]'\"\n",
            primary = toml_string(&rig.project.display().to_string()),
            worktrees = toml_string(&declared.display().to_string()),
        ),
    )
    .expect("the declaration is written");

    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--project",
        "a-declared-project",
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));
    assert!(
        declared.join("transient-1").is_dir(),
        "the worktree is cut under the declaration's directory {}",
        declared.display()
    );
    assert!(
        !rig.worktrees.join("transient-1").exists(),
        "and not under the neighbour's {}",
        rig.worktrees.display()
    );

    // The control on the name: the basename the embedded file would have
    // answered with is a project this directory does not resolve to, and the
    // usage error says what it does resolve to.
    let out = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--project",
        "a-project",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("a-declared-project"),
        "the name is the declaration's: {}",
        stderr(&out)
    );
}

/// The four lifecycle words under `seat` still answer with the `event`
/// spelling, at the usage status.
#[test]
fn the_four_lifecycle_words_under_seat_still_name_the_event_spelling() {
    let rig = Rig::new("lifecycle", true);
    for verb in ["woke", "rest", "handed-off", "exited"] {
        let out = rig.run(&["seat", verb, "a-seat"]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`seat {verb}`: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains(&format!("say fleet event {verb}")),
            "`seat {verb}` names the rewrite: {}",
            stderr(&out)
        );
    }

    // The control: the verbs this noun DOES answer are not met by that line —
    // `seat spawn --help` is a page, not a rewrite.
    let out = rig.run(&["seat", "spawn", "--help"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("--first-turn"), "{}", stdout(&out));

    // And the family's help page lists exactly the verbs it answers. Read from
    // the Commands block and not from the whole page: the about text above it
    // names the four on purpose, to say where they went.
    let page = rig.run(&["seat", "--help"]);
    let page = stdout(&page);
    let listed: Vec<String> = page
        .lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .skip(1)
        .take_while(|line| line.starts_with("  ") || line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect();
    assert_eq!(
        listed,
        vec![
            "spawn".to_string(),
            "feed".to_string(),
            "retire".to_string(),
            "nudge".to_string()
        ],
        "`seat --help` lists the verbs it answers and the four rewrites stay hidden: {page}"
    );
}

/// `--project` selects nothing and CHECKS: a name this directory does not
/// resolve to is a usage error, which is the mistake that spawns a seat against
/// the wrong checkout.
#[test]
fn a_project_the_directory_does_not_resolve_to_is_a_usage_error() {
    let rig = Rig::new("project-flag", true);
    let out = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--project",
        "somewhere-else",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("a-project"), "{}", stderr(&out));
    assert!(
        !rig.worktrees.join("transient-1").exists(),
        "and nothing was created"
    );

    // The control: the same call naming the project this directory IS resolves
    // past the check and spawns.
    let out = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--project",
        "a-project",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
}

/// A `[project]` path that is declared and is not a string is a REFUSAL naming
/// the key, never the derived fallback.
///
/// A project that says where its worktrees go and has that silently ignored
/// cuts a seat's checkout somewhere other than where it said — which is the one
/// outcome a misdeclaration must not have.
#[test]
fn a_project_path_that_is_not_a_string_refuses_and_never_falls_back() {
    let rig = Rig::new("project-misdeclared", true);
    rig.declare_project("worktrees = 42\n");

    let out = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("[project] worktrees") && stderr(&out).contains("integer"),
        "the refusal names the key and what it found: {}",
        stderr(&out)
    );
    assert!(
        !rig.worktrees.join("transient-1").exists(),
        "and nothing was cut in the derived directory"
    );

    // The control, and the other half of the rule: a key that is ABSENT falls
    // back as before, so the refusal above is the declaration's and not a verb
    // that refuses whenever the block is short.
    rig.declare_project("primary = ");
    std::fs::write(
        rig.project.join("fleet.toml"),
        std::fs::read_to_string(rig.project.join("fleet.toml"))
            .expect("the policy is readable")
            .replace("[project]\nprimary = ", ""),
    )
    .expect("the policy is rewritten");
    let out = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(rig.worktrees.join("transient-1").is_dir());
}

/// A first turn this process cannot read is a usage error and not a refusal:
/// nothing about the fleet is wrong.
#[test]
fn a_first_turn_file_that_is_not_there_is_a_usage_error() {
    let rig = Rig::new("no-turn", true);
    let out = rig.run(&["seat", "spawn", "--first-turn", "/nowhere/at/all.md"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("/nowhere/at/all.md"),
        "{}",
        stderr(&out)
    );
}

/// The seam that was a placeholder: `fleet dispatch` with no `--to` runs the
/// real spawner and assigns the item to the name it printed.
#[test]
fn dispatch_without_a_seat_spawns_through_the_real_spawner_and_assigns_the_name() {
    let rig = Rig::new("dispatch", true);
    rig.init_store();
    let item = rig.item("a ready item for a seat nobody has spawned yet");

    let out = rig.run(&[
        "dispatch",
        &item,
        "--by",
        "an-architect",
        "--packs-dir",
        &rig.machine.join("packs").display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(rig.calls().contains("START"), "{}", rig.calls());

    let (assignee, notes, orders) = rig.order_of(&item);
    assert_eq!(
        assignee.as_deref(),
        Some("transient-1"),
        "the item is assigned to the seat the spawn printed"
    );
    assert_eq!(orders["seat"], serde_json::json!("transient-1"));
    assert!(notes.contains("orders given"), "{notes}");
    let worktree = rig.worktrees.join("transient-1");
    assert!(worktree.is_dir());

    // THE EVENT, off the stream the binary wrote, and its base read by this arm
    // from the worktree's OWN HEAD — the commit the seat starts from, and the
    // one fact a ref the spawn named could already have moved past.
    let events = rig.events();
    let last = events.last().expect("the stream carries the dispatch");
    assert_eq!(last["type"].as_str(), Some("item.dispatched"), "{last}");
    assert_eq!(last["actor"].as_str(), Some("an-architect"));
    assert_eq!(last["payload"]["item"].as_str(), Some(item.as_str()));
    assert_eq!(last["payload"]["seat"].as_str(), Some("transient-1"));
    let head = seen(&worktree, &["rev-parse", "HEAD"]);
    assert_eq!(head.len(), 40, "a commit is 40 hex: {head}");
    assert_eq!(
        last["payload"]["base"].as_str(),
        Some(head.as_str()),
        "the base is the worktree's own HEAD: {last}"
    );
    assert_eq!(
        head,
        seen(&rig.project, &["rev-parse", "refs/remotes/origin/main"]),
        "which is the trunk the primary carried at the spawn"
    );
}

/// A dispatch to a NAMED seat writes the same event with no base at all: no
/// worktree was cut, so there is no commit this order started from.
#[test]
fn a_named_dispatch_writes_the_event_with_no_base() {
    let rig = Rig::new("dispatch-named", true);
    rig.init_store();
    let item = rig.item("a ready item for a seat that already exists");
    std::fs::write(
        rig.machine.join("config.json"),
        format!(
            "{{\"fleet_toml\": {policy}, \"children\": [{{\"name\": \"s-cli-named\", \
             \"chosen_name\": \"Orla\", \"worktrees\": {{\"a-project\": {tree}}}}}]}}\n",
            policy = json_string(&rig.project.join("fleet.toml").display().to_string()),
            tree = json_string(&rig.project.display().to_string()),
        ),
    )
    .expect("the seat list is written");

    let out = rig.run(&[
        "dispatch",
        &item,
        "--to",
        "s-cli-named",
        "--by",
        "an-architect",
        "--packs-dir",
        &rig.machine.join("packs").display().to_string(),
    ]);
    // The roster is empty, so the ring finds no live session: exit 4, and the
    // order — and its event — stand.
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("ORDERED, NOT RUNG"),
        "{}",
        stderr(&out)
    );

    let last = rig
        .events()
        .last()
        .cloned()
        .expect("the stream carries the dispatch");
    assert_eq!(last["type"].as_str(), Some("item.dispatched"), "{last}");
    assert_eq!(last["payload"]["seat"].as_str(), Some("s-cli-named"));
    assert!(
        last["payload"].get("base").is_none(),
        "a named dispatch carries no base: {last}"
    );
}

/// The half of the belt-legs control that runs NO belt: a dispatch to a named
/// seat starts nothing, measures nothing, and its stdout is the order line
/// alone.
///
/// ONE CONTROL IN TWO ARMS, with the arm below, at the one forced pair [`HELD`]
/// and in the one file. A verb that printed the legs unconditionally fails
/// here; one that printed them nowhere fails there. The two are two arms and
/// not one because each drives a full dispatch end to end through the real
/// store, and the pair carried in one arm runs to nextest's slow mark — so
/// either of them changed is both of them read.
#[test]
fn a_dispatch_that_starts_nothing_prints_no_belt_legs() {
    let rig = Rig::new("dispatch-belt-none", true);
    rig.init_store();

    // The seat is a named one, live in the project's own checkout, so the ring
    // lands and the verb exits 0 with the order line on stdout.
    std::fs::write(
        rig.machine.join("config.json"),
        format!(
            "{{\"fleet_toml\": {policy}, \"children\": [{{\"name\": \"s-cli-belts\", \
             \"chosen_name\": \"Orla\", \"worktrees\": {{\"a-project\": {tree}}}}}]}}\n",
            policy = json_string(&rig.project.join("fleet.toml").display().to_string()),
            tree = json_string(&rig.project.display().to_string()),
        ),
    )
    .expect("the seat list is written");
    rig.live_in_the_checkout();
    let named = rig.item("a ready item for a seat that is already up");
    let out = rig.run_at(
        &[
            "dispatch",
            &named,
            "--to",
            "s-cli-belts",
            "--by",
            "an-architect",
            "--packs-dir",
            &rig.machine.join("packs").display().to_string(),
        ],
        &HELD,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "dispatched by an-architect — orders given\n",
        "a dispatch that started nothing measured nothing, and says so by \
         printing nothing about it"
    );
}

/// The belt's two legs on a dispatch's OWN output, and its two readings on the
/// stream — through the built binary, at the forced pair [`HELD`].
///
/// The other half of the control is the arm above: this one is the dispatch
/// that RUNS a belt, and its stdout is the order line with both legs under it.
#[test]
fn a_dispatch_prints_the_belts_two_legs_and_the_stream_carries_its_readings() {
    let rig = Rig::new("dispatch-belt-legs", true);
    rig.init_store();
    rig.live_in_the_checkout();

    let item = rig.item("a ready item for a seat the belt lets through");
    let out = rig.run_at(
        &[
            "dispatch",
            &item,
            "--by",
            "an-architect",
            "--packs-dir",
            &rig.machine.join("packs").display().to_string(),
        ],
        &HELD,
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "dispatched by an-architect — orders given\n\
         \x20 load average (5m)       : 3.75 (ceiling 4.00 = 4 cpu x 1.00)\n\
         \x20 transient seats mid-turn: 0 (cap 3)\n",
        "the order line, then the belt's two legs in `seat spawn`'s own words"
    );

    // And the same two readings on the stream, as NUMBERS: the line a person
    // reads and the record a report is built from are the one measurement.
    let spawned = rig
        .events()
        .into_iter()
        .find(|event| event["type"].as_str() == Some("session.spawned"))
        .expect("the stream carries the spawn");
    let belt = &spawned["payload"]["belt"];
    assert_eq!(belt["load"], serde_json::json!(3.75), "{belt}");
    assert_eq!(belt["load_ceiling"], serde_json::json!(4.0), "{belt}");
    assert_eq!(belt["cpus"], serde_json::json!(4), "{belt}");
    assert_eq!(belt["mid_turn"], serde_json::json!(0), "{belt}");
    assert_eq!(belt["mid_turn_cap"], serde_json::json!(3), "{belt}");
}

/// `git` in a directory of the arm's choosing, answering its trimmed stdout.
fn seen(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// And under the load override the same dispatch refuses at 1 with the order
/// withdrawn, carrying the BELT's own refusal text: the cause a person reads on
/// a withdrawal is the cause the spawn gave.
#[test]
fn dispatch_under_the_load_override_refuses_and_withdraws_the_order() {
    let rig = Rig::new("dispatch-belt", true);
    rig.init_store();
    let item = rig.item("a ready item the machine cannot take");

    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args([
            "dispatch",
            &item,
            "--by",
            "an-architect",
            "--packs-dir",
            &rig.machine.join("packs").display().to_string(),
        ])
        .current_dir(&rig.project)
        .hermetic(&rig.root.join("home"), &rig.machine, Some(&rig.stub))
        .env("FLEET_LOAD_AVERAGE", "99.0")
        .env("FLEET_CPUS", "8")
        .output()
        .expect("the built binary runs");

    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("load average"),
        "the refusal is the belt's: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("the order was withdrawn"),
        "{}",
        stderr(&out)
    );
    assert!(rig.calls().is_empty(), "the agent was never called");
    assert!(
        !rig.worktrees.join("transient-1").exists(),
        "and no worktree was made"
    );

    let (assignee, notes, orders) = rig.order_of(&item);
    assert_eq!(assignee, None, "nobody was ever assigned");
    assert_eq!(orders, serde_json::Value::Null, "no orders key survives");
    assert!(
        notes.contains("DISPATCH WITHDRAWN"),
        "the withdrawal is on the record: {notes}"
    );
}

// ---- the work branch a retire releases --------------------------------------

/// The branch the seat holds while its item is landed. A real builder's, so the
/// primary's branch list carries it before the retire and is asked after.
const WORK: &str = "transient-1/feat/the-work";

/// The landing note a reviewer's `fleet land` leaves on the item, in the row
/// grammar `land` writes: the criterion off the verb's own list, the verdict, and
/// the evidence that opens with the branch it classified.
fn a_landing(verdict: &str, branch: &str) -> String {
    format!(
        "LANDED 4444444444444444444444444444444444444444 on main by a-reviewer\n\
         5. {current:<16} {pass:<10} nothing\n\
         6. {branch_row:<16} {verdict:<10} {branch} — what the landing read\n\
         7. {clean:<16} {pass:<10} nothing\n",
        current = CRITERIA[4],
        branch_row = CRITERIA[WORK_BRANCH],
        clean = CRITERIA[6],
        pass = "PASS",
    )
}

/// A seat the board has dispatched an item to: the store, the item ORDERED to
/// `transient-1` and open, and the worktree the spawn cut. It is the state a
/// seat is in while it works, and the one a retire has an order to answer for.
fn a_dispatched_seat(rig: &Rig) -> String {
    rig.init_store();
    let item = rig.item("an item a seat was dispatched");
    let out = rig.run(&[
        "dispatch",
        &item,
        "--by",
        "an-architect",
        "--packs-dir",
        &rig.machine.join("packs").display().to_string(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    item
}

/// A dispatched seat standing on its work branch, its item carrying a landing
/// that says `verdict` about that branch. What the arms differ in is that one
/// word.
///
/// THE ITEM IS CLOSED, because `fleet land` closes it with the landed sha in
/// the reason (the land verb, step l) — so a seat retired after its landing holds
/// nothing open, which is the state these arms are about.
fn a_landed_seat(rig: &Rig, verdict: &str) -> String {
    let item = a_dispatched_seat(rig);

    let worktree = rig.worktrees.join("transient-1");
    seen(&worktree, &["checkout", "--quiet", "-b", WORK]);
    assert_eq!(
        seen(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]),
        WORK,
        "the seat's worktree holds the branch, which is what makes the landing's own delete exit 1"
    );
    assert!(
        rig.git(&["branch", "--list"]).contains(WORK),
        "and the primary's branch list carries it: {}",
        rig.git(&["branch", "--list"])
    );

    let note = rig.bd(&[
        "note",
        &item,
        &a_landing(verdict, WORK),
        "--actor",
        "a-reviewer",
    ]);
    assert!(
        note.status.success(),
        "bd note: {}",
        String::from_utf8_lossy(&note.stderr)
    );
    // `--force`, because this fixture skips the delivery: bd 1.3.0 refuses a
    // close by an actor that is not the item's assignee, and `fleet land` only
    // ever closes as the assignee — the reviewer its delivery handed the item
    // to — while here the item is still the seat's.
    let closed = rig.bd(&[
        "close",
        &item,
        "--reason",
        "landed 4444444444444444444444444444444444444444",
        "--actor",
        "a-reviewer",
        "--force",
    ]);
    assert!(
        closed.status.success(),
        "bd close: {}",
        String::from_utf8_lossy(&closed.stderr)
    );
    rig.live("transient-1", "idle");
    item
}

/// The promise the retire arms below stand on: the pid a roster row here
/// carries answers ESRCH to `kill -0` at the moment the row is written, and it
/// is that pid the row carries.
///
/// `seat retire`'s last probe reads the roster's pid with the same call and
/// refuses over one that still names a process, so a row carrying a number no
/// arm chose reds every retire here on a box that happens to hold it — a red
/// the verb under test did nothing to earn.
#[test]
fn the_roster_fixtures_pid_names_no_live_process() {
    let rig = Rig::new("gone-pid", true);
    rig.live("transient-1", "idle");

    let pid = rig.pid();
    assert_eq!(
        platform::process_alive(pid),
        Some(false),
        "the pid this rig wrote into its roster is a live process on this box"
    );
    let written = std::fs::read_to_string(&rig.roster).expect("the roster is readable");
    assert!(
        written.contains(&format!("\"pid\": {pid}")),
        "and the row carries that pid and no other: {written}"
    );
}

/// The landing said SAFE about the branch this seat is standing on, so the
/// retire that takes the worktree finishes the delete the landing could not.
#[test]
fn a_retire_deletes_the_work_branch_its_landing_classified_safe() {
    let rig = Rig::new("release-safe", true);
    a_landed_seat(&rig, SAFE);

    let retired = rig.run(&["seat", "retire", "transient-1"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));
    assert!(
        stdout(&retired).contains(&format!(
            "work branch {WORK}: {SAFE} on the landing — deleted"
        )),
        "the delete is printed: {}",
        stdout(&retired)
    );
    assert_eq!(
        rig.git(&["branch", "--list"]),
        "* main",
        "the primary's branch list reads the trunk alone"
    );
}

/// The control, one word apart: a landing that did not read SAFE leaves the
/// branch standing and says which reading spared it.
#[test]
fn a_retire_leaves_a_branch_no_landing_called_safe() {
    let rig = Rig::new("release-carries", true);
    a_landed_seat(&rig, "CARRIES UNLANDED WORK");

    let retired = rig.run(&["seat", "retire", "transient-1"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));
    assert!(
        stdout(&retired).contains("work branch kept — ")
            && stdout(&retired).contains("CARRIES UNLANDED WORK"),
        "the reason is printed: {}",
        stdout(&retired)
    );
    let listed = rig.git(&["branch", "--list"]);
    assert!(listed.contains(WORK), "and the branch stands: {listed}");
}

// ---- the order a retire withdraws -------------------------------------------

/// The record's half of the retire, through the shipped binary and a real work
/// graph: the seat is retired while the item it was dispatched is still OPEN,
/// and that item reads unassigned, with no orders key, carrying the withdrawal
/// note.
///
/// THE NAME IS WHY. `transient-1` goes back on the pile the moment the row
/// comes off the seat list, and the next spawn takes it — so an order still
/// standing against it is one that seat inherits, and its first delivery is
/// refused for an item it never saw.
///
/// The state is the one a seat is retired in when its work did NOT land: a
/// parked item, a crashed seat, a flight that ended. A seat retired after a
/// landing holds a closed item and this verb writes nothing, which is what the
/// branch-release arms above run through.
///
/// ONE ARM AND NOT TWO. Which items the query does and does not name is the
/// core suite's, over a store with four shapes on it; what only this arm can
/// say is that real `bd` empties the assignee and drops the key.
#[test]
fn a_retire_withdraws_the_order_the_seat_still_holds() {
    let rig = Rig::new("retire-withdraws", true);
    let item = a_dispatched_seat(&rig);
    rig.live("transient-1", "idle");

    let retired = rig.run(&["seat", "retire", "transient-1"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));

    let (assignee, notes, orders) = rig.order_of(&item);
    assert!(
        assignee.as_deref().unwrap_or("").trim().is_empty(),
        "the item reads unassigned: {assignee:?}"
    );
    assert!(orders.is_null(), "and carries no orders key: {orders}");
    assert!(
        notes.contains("ORDER WITHDRAWN at retire: transient-1 retired by")
            && notes.contains("the item stays open, unassigned"),
        "the withdrawal is on the record: {notes}"
    );
    // The dispatch's own order note is still there under it: the withdrawal
    // APPENDS, and an item whose record was replaced would read as one nobody
    // was ever given.
    assert!(
        notes.contains("dispatched by an-architect"),
        "the order note it answers stands: {notes}"
    );
}

/// fleet-reb: the same retire over an item the seat CLAIMED. bd 1.3.0 refuses
/// a plain `--assignee` from anyone but the holder on an `in_progress` item —
/// `cannot reassign X: held by "transient-1" (in_progress)` — and a retire's
/// actor is never the seat it retires, so only the withdrawal's
/// `--if-assignee <seat>` lets it through. The fence is bd's, so the arm is
/// the real binary's.
#[test]
fn a_retire_withdraws_an_item_the_seat_marked_in_progress() {
    let rig = Rig::new("retire-in-progress", true);
    let item = a_dispatched_seat(&rig);
    let claimed = rig.bd(&[
        "update",
        &item,
        "--status",
        "in_progress",
        "--actor",
        "transient-1",
    ]);
    assert!(
        claimed.status.success(),
        "bd update: {}",
        String::from_utf8_lossy(&claimed.stderr)
    );
    rig.live("transient-1", "idle");

    let retired = rig.run(&["seat", "retire", "transient-1"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));

    let (assignee, notes, orders) = rig.order_of(&item);
    assert!(
        assignee.as_deref().unwrap_or("").trim().is_empty(),
        "the claimed item reads unassigned: {assignee:?}"
    );
    assert!(orders.is_null(), "and carries no orders key: {orders}");
    assert!(
        notes.contains("ORDER WITHDRAWN at retire: transient-1 retired by"),
        "the withdrawal is on the record: {notes}"
    );
}

// ---- the SDK's flag (cli PRD § The JSON envelope) ---------------------------

/// The document a `--json` run printed on stdout, parsed, with the whole of
/// stdout asserted to be that one document and nothing beside it.
fn document(out: &Output) -> serde_json::Value {
    let printed = stdout(out);
    assert_eq!(
        printed.lines().count(),
        1,
        "under the flag stdout is one document and nothing else: {printed}"
    );
    serde_json::from_str(printed.trim()).unwrap_or_else(|e| panic!("{e}: {printed}"))
}

/// AC1, spawn — `seat spawn --json` answers the envelope carrying what the
/// controller's `Spawned` holds: the name, the worktree, the belt's readings
/// and the commit the cut was read back at.
///
/// The belt is its PAYLOAD and not the two sentences stderr prints, because the
/// document is the outcome and not a rendering of it.
#[test]
fn the_json_spawn_prints_the_seat_the_worktree_the_belt_and_the_base() {
    let rig = Rig::new("json-spawn", true);

    let spawned = rig.run_at(
        &[
            "seat",
            "spawn",
            "--first-turn",
            &rig.turn.display().to_string(),
            "--json",
        ],
        // A load and a cpu count no other arm uses, so the numbers below are
        // this run's readings and not a pair that happens to match.
        &[("FLEET_LOAD_AVERAGE", "0.37"), ("FLEET_CPUS", "4")],
    );
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));

    let parsed = document(&spawned);
    assert_eq!(parsed["ok"], serde_json::Value::Bool(true));
    assert_eq!(parsed["verb"], "seat spawn");
    assert_eq!(parsed["data"]["seat"], "transient-1");
    assert_eq!(
        parsed["data"]["worktree"],
        rig.worktrees.join("transient-1").display().to_string()
    );
    assert_eq!(parsed["data"]["belt"]["load"], 0.37);
    assert_eq!(parsed["data"]["belt"]["cpus"], 4u64);
    assert_eq!(
        parsed["data"]["base"],
        rig.git(&["rev-parse", "refs/remotes/origin/main"]),
        "the base is the commit the worktree's own HEAD reads back at"
    );

    // The person's two lines are printed under the flag too, so a run nobody is
    // parsing still says what the belt read.
    assert!(
        stderr(&spawned).contains("load average (5m)") && stderr(&spawned).contains("worktree: "),
        "{}",
        stderr(&spawned)
    );
}

/// AC1, feed — `seat feed --json` answers the seat and the two turns, each as
/// its first line, which is the shape the controller's own journal carries them
/// in.
#[test]
fn the_json_feed_prints_the_seat_and_the_turn_that_replaced_the_last() {
    let rig = Rig::new("json-feed", true);
    let spawned = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));
    rig.live("transient-1", "idle");

    let next = rig.root.join("the-next-turn.md");
    std::fs::write(&next, "the next turn this seat takes\nand a second line\n")
        .expect("the turn is written");
    let fed = rig.run(&[
        "seat",
        "feed",
        "transient-1",
        "--first-turn",
        &next.display().to_string(),
        "--json",
    ]);
    assert_eq!(fed.status.code(), Some(0), "{}", stderr(&fed));

    let parsed = document(&fed);
    assert_eq!(parsed["ok"], serde_json::Value::Bool(true));
    assert_eq!(parsed["verb"], "seat feed");
    assert_eq!(parsed["data"]["seat"], "transient-1");
    assert_eq!(
        parsed["data"]["first_turn"],
        "the next turn this seat takes"
    );
    assert_eq!(
        parsed["data"]["prior_first_turn"], "the turn this seat comes up on",
        "the turn that was in the seat, which is what the marker held"
    );
    assert!(rig.calls().contains("NUDGE"), "{}", rig.calls());
}

/// AC1, retire — `seat retire --json` answers what the controller's `Reclaimed`
/// holds, plus what became of the work branch: the name, the word, and the
/// reading behind it.
#[test]
fn the_json_retire_prints_the_reclaim_and_what_became_of_the_work_branch() {
    let rig = Rig::new("json-retire", true);
    a_landed_seat(&rig, SAFE);

    let retired = rig.run(&["seat", "retire", "transient-1", "--json"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));

    let parsed = document(&retired);
    assert_eq!(parsed["ok"], serde_json::Value::Bool(true));
    assert_eq!(parsed["verb"], "seat retire");
    assert_eq!(parsed["data"]["seat"], "transient-1");
    assert_eq!(
        parsed["data"]["worktree"],
        rig.worktrees.join("transient-1").display().to_string()
    );
    assert!(
        parsed["data"]["bytes"].as_u64().is_some(),
        "the worktree was walked: {parsed}"
    );
    assert_eq!(parsed["data"]["pid"], u64::from(rig.pid()));
    assert_eq!(parsed["data"]["branch"]["name"], WORK);
    assert_eq!(parsed["data"]["branch"]["disposition"], "deleted");
    assert_eq!(
        rig.git(&["branch", "--list"]),
        "* main",
        "and the branch is gone, which is what the document says happened"
    );
}

/// AC1's other half for retire — the branch a landing did not call SAFE is
/// `kept`, with the reading that spared it, so a caller branching on the word
/// meets both.
#[test]
fn the_json_retire_says_kept_where_the_landing_did_not_read_safe() {
    let rig = Rig::new("json-retire-kept", true);
    a_landed_seat(&rig, "CARRIES UNLANDED WORK");

    let retired = rig.run(&["seat", "retire", "transient-1", "--json"]);
    assert_eq!(retired.status.code(), Some(0), "{}", stderr(&retired));

    let parsed = document(&retired);
    assert_eq!(parsed["data"]["branch"]["name"], WORK);
    assert_eq!(parsed["data"]["branch"]["disposition"], "kept");
    assert!(
        parsed["data"]["branch"]["why"]
            .as_str()
            .expect("the why is a string")
            .contains("CARRIES UNLANDED WORK"),
        "{parsed}"
    );
    assert!(
        rig.git(&["branch", "--list"]).contains(WORK),
        "and the branch stands"
    );
}

/// A permission slot whose document names a placeholder a spawn has no value
/// for: the one could-not-tell leg `seat spawn` reaches before it creates
/// anything.
const UNFILLABLE_RULES: &str = "{\"permissions\": {\"allow\": [\"Bash({nowhere})\"]}}\n";

/// A could-not-tell and a usage error are DIFFERENT refusal codes, so a caller
/// branching on the document never reads a question as a verdict.
///
/// The exit table already keeps them apart on `$?`; this is the half the
/// document must not flatten. A transport that answered both with one code
/// would make a spawn worth retrying indistinguishable from a call that can
/// never succeed — and nothing else in this suite is watching that, so this arm
/// is the only thing standing between the two.
#[test]
fn a_could_not_tell_and_a_usage_error_are_different_refusal_codes() {
    let rig = Rig::new("json-could-not-tell", true);
    rig.pack("zeta", &[(PERMISSIONS, UNFILLABLE_RULES)]);

    let unreadable = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
        "--json",
    ]);
    assert_eq!(unreadable.status.code(), Some(3), "{}", stderr(&unreadable));
    let question = document(&unreadable);
    assert_eq!(question["ok"], serde_json::Value::Bool(false));
    assert_eq!(question["verb"], "seat spawn");
    assert_eq!(question["refusal"]["code"], "could_not_tell");
    assert!(
        question["refusal"]["why"]
            .as_str()
            .expect("the why is a string")
            .contains("nowhere"),
        "the why names the placeholder: {question}"
    );
    // The person's line is the words it always was, under the flag too: the
    // document names the verb as `seat spawn` and this is the only thing
    // watching that the stderr line still reads `fleet seat spawn:`.
    assert!(
        stderr(&unreadable).starts_with("fleet seat spawn: "),
        "{}",
        stderr(&unreadable)
    );

    // The other row, from the same verb on the same fixture: a first turn this
    // process cannot read is the caller's own defect and stays `usage`.
    let missing = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        "/nowhere/at/all.md",
        "--json",
    ]);
    assert_eq!(missing.status.code(), Some(2), "{}", stderr(&missing));
    let defect = document(&missing);
    assert_eq!(defect["ok"], serde_json::Value::Bool(false));
    assert_eq!(defect["refusal"]["code"], "usage");

    assert_ne!(
        question["refusal"]["code"], defect["refusal"]["code"],
        "the two answers are distinguishable in the document, not only on $?"
    );
    assert_ne!(
        unreadable.status.code(),
        missing.status.code(),
        "and the exit codes the flag never touches still tell them apart"
    );

    // The control the two assertions need: without the flag neither call prints
    // a document at all, so the codes above are the flag's own answer.
    let plain = rig.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &rig.turn.display().to_string(),
    ]);
    assert_eq!(plain.status.code(), Some(3), "{}", stderr(&plain));
    assert_eq!(stdout(&plain), "");
}

/// AC3 — the human rendering is what it was: without the flag `seat spawn`
/// still prints the seat's name ALONE on stdout, which is the byte `dispatch`
/// reads to assign an item to.
///
/// The `--json` run beside it is the control: the same fixture, the same spawn,
/// and stdout that is not the name — so the pin above is the flagless form's
/// and not something every run of this verb prints.
#[test]
fn the_human_rendering_of_a_spawn_is_the_name_alone_without_the_flag() {
    let plain = Rig::new("human-spawn", true);
    let spawned = plain.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &plain.turn.display().to_string(),
    ]);
    assert_eq!(spawned.status.code(), Some(0), "{}", stderr(&spawned));
    assert_eq!(
        stdout(&spawned),
        "transient-1\n",
        "byte for byte: the name, a newline, and nothing else"
    );
    assert!(
        stderr(&spawned).contains("load average (5m)")
            && stderr(&spawned).contains("transient seats mid-turn")
            && stderr(&spawned).contains(&format!(
                "worktree: {}",
                plain.worktrees.join("transient-1").display()
            )),
        "and the two belt lines and the worktree are still the person's, on stderr: {}",
        stderr(&spawned)
    );

    let flagged = Rig::new("human-spawn-control", true);
    let other = flagged.run(&[
        "seat",
        "spawn",
        "--first-turn",
        &flagged.turn.display().to_string(),
        "--json",
    ]);
    assert_eq!(other.status.code(), Some(0), "{}", stderr(&other));
    assert_ne!(
        stdout(&other),
        "transient-1\n",
        "the flag replaces the name on stdout rather than joining it"
    );
}
