//! Fixture tests for the transient-seat primitives.
//!
//! The `lessons::` module below is the contract named in
//! `fleet/brain/lessons/*.md` § Test inventory: each fact the code in this slice
//! exercises owes a test under the exact name the inventory carries.
//!
//! The three verbs are driven against a STUB AGENT — a shell script this file
//! writes, which serves a roster the arm controls, records every call it was
//! given, and exits at a code the arm sets — and against a SCRATCH GIT
//! REPOSITORY carrying a local `refs/remotes/origin/main`, which is the shape a
//! spawn cuts a worktree from. So what a verb did is a reading of the machine it
//! left behind, and never of what it reported.
//!
//! ## Why no arm here touches the environment
//!
//! The belt's two readings are handed in on `Machine` rather than read from
//! `FLEET_LOAD_AVERAGE` and `FLEET_CPUS`, which are the PROCESS's environment:
//! `cargo test` shares one across every thread it runs an arm on, and this file
//! forks children while those arms run, so a `set_var` here would be both a
//! number another arm's belt could see and a write racing a fork. Every arm
//! states the two readings it is judged against. The overrides' own wiring is
//! measured in `cli/tests/seat.rs`, which sets them on the CHILD it drives.

use fleet_controller::adapter::claude_code::ClaudeCode;
use fleet_controller::adapter::{Agent, RemoveAnswer, RosterRead};
use fleet_controller::config;
use fleet_controller::events;
use fleet_controller::platform;
use fleet_controller::policy::{self, Policy};
use fleet_controller::sessions;
use fleet_controller::test_support::FakeClock;
use fleet_controller::transient::{self, Machine, Readings, Refusal, Spawn};
use fleet_core::seat::identity::SeatId;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// The id of the one named — not transient — row the refusal arms add beside a
/// spawned seat.
const NAMED_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn git_at(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
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

/// A repository with one commit and a local `refs/remotes/origin/main`, which
/// is the ref a spawn cuts a detached worktree from — built once on disk and
/// COPIED into every rig, never handed out: nextest runs one process per arm,
/// so a cache held in this process would be a cache of one, and the verbs
/// under test write to the repository they are given.
///
/// The build goes under a staging name and is renamed into place, so a process
/// that loses the race meets a directory that is already whole; a rename onto
/// a directory that holds something fails, which is how the loss is read.
fn trunk_template() -> PathBuf {
    trunk_template_at(&std::env::temp_dir().join("fleet-trunk-template-1"))
}

/// The build above under a caller-named base, so an arm can wedge a home of its
/// own rather than the one every other arm in the filter reads.
fn trunk_template_at(base: &Path) -> PathBuf {
    let home = base.join("one-commit");
    if home.join("ready").is_file() {
        return home.join("repo");
    }

    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let staging = base.join(format!(".building-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let repo = staging.join("repo");
    std::fs::create_dir_all(&repo).expect("the template root is created");
    git_at(&repo, &["init", "--quiet", "--initial-branch", "main"]);
    std::fs::write(repo.join("a-file.txt"), "the trunk\n").expect("the file is written");
    git_at(&repo, &["add", "--", "a-file.txt"]);
    git_at(
        &repo,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "the trunk"],
    );
    git_at(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(staging.join("ready"), "").expect("the template is marked whole");

    // A build that died before writing its marker leaves a home no rename can
    // ever land on, so a home still carrying no `ready` after the rename failed
    // is cleared and the build retried once: without it one wedged directory
    // reds every arm here, for every process on the box, until a hand deletes
    // it. A home that DOES carry the marker is a race lost, and is left whole.
    if std::fs::rename(&staging, &home).is_err() && !home.join("ready").is_file() {
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::rename(&staging, &home);
    }
    let _ = std::fs::remove_dir_all(&staging);
    assert!(
        home.join("ready").is_file(),
        "a template stands at {}",
        home.display()
    );
    home.join("repo")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the copy's root is created");
    for entry in std::fs::read_dir(from).expect("the template is read") {
        let entry = entry.expect("the template's entry is read");
        let target = to.join(entry.file_name());
        if entry
            .file_type()
            .expect("the entry's kind is read")
            .is_dir()
        {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the template's file is copied");
        }
    }
}

/// A whole machine in a temp directory: a scratch primary with a trunk ref, a
/// worktrees directory, a machine directory and one stub agent.
struct Rig {
    root: PathBuf,
    primary: PathBuf,
    worktrees: PathBuf,
    machine: PathBuf,
    home: PathBuf,
    stub: PathBuf,
    roster: PathBuf,
    roster_fails: PathBuf,
    /// Every configuration directory a listing was asked under, appended one per
    /// line by the stub: the seam that says WHICH directory a read was made
    /// through.
    listing_dirs: PathBuf,
    stop_keeps_the_roster: PathBuf,
    calls: PathBuf,
    start_argv: PathBuf,
    start_cwd: PathBuf,
    /// What the child could read of the seat's local settings when it came up,
    /// which is the only witness that the write happened BEFORE the start.
    settings_at_start: PathBuf,
    nudge_argv: PathBuf,
    start_exit: PathBuf,
    stop_exit: PathBuf,
    rm_exit: PathBuf,
    rm_stdout: PathBuf,
    nudge_exit: PathBuf,
    start_makes_branch: PathBuf,
    /// While this file stands, the stub's start and stop block in it — so an arm
    /// can hold a verb inside one of its windows and read what a SECOND verb
    /// gets done meanwhile. Absent by default, which is every other arm here.
    gate: PathBuf,
    /// The clock every verb driven against this rig spends its one duration-
    /// subject wait on — `cleared`'s start-watch window. Fake, so the window
    /// costs no wall clock; the listings inside it are real children and their
    /// number is the same either way.
    clock: FakeClock,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-transient-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            primary: root.join("a-project"),
            worktrees: root.join("a-project-worktrees"),
            machine: root.join("machine"),
            home: root.join("home"),
            stub: root.join("agent-stub"),
            roster: root.join("roster.json"),
            roster_fails: root.join("roster-fails"),
            listing_dirs: root.join("listing-dirs"),
            stop_keeps_the_roster: root.join("stop-keeps-the-roster"),
            calls: root.join("calls"),
            start_argv: root.join("start-argv"),
            start_cwd: root.join("start-cwd"),
            settings_at_start: root.join("settings-at-start"),
            nudge_argv: root.join("nudge-argv"),
            start_exit: root.join("start-exit"),
            stop_exit: root.join("stop-exit"),
            rm_exit: root.join("rm-exit"),
            rm_stdout: root.join("rm-stdout"),
            nudge_exit: root.join("nudge-exit"),
            start_makes_branch: root.join("start-makes-branch"),
            gate: root.join("gate"),
            clock: FakeClock::new(),
            root,
        };
        for dir in [&rig.worktrees, &rig.machine, &rig.home] {
            std::fs::create_dir_all(dir).expect("the fixture directory is created");
        }
        copy_tree(&trunk_template(), &rig.primary);
        rig.write_stub();
        rig.roster("[]");
        rig.write_config("[]");
        rig
    }

    fn git(&self, args: &[&str]) -> String {
        git_at(&self.primary, args)
    }

    /// Commit a file onto the fixture's trunk and move `origin/main` onto it,
    /// so a worktree cut from the trunk comes up carrying it — which is the
    /// shape of a project that TRACKS its own local settings document.
    fn tracked_on_trunk(&self, relative: &str, body: &str) -> &Rig {
        let path = self.primary.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the tracked file's directory is created");
        }
        std::fs::write(&path, body).expect("the tracked file is written");
        // `--force`: this path is on the box's own global excludes file, which
        // reaches a fixture repository because `GIT_CONFIG_GLOBAL=/dev/null`
        // leaves `core.excludesFile` at its default rather than empty — and a
        // project that tracks this file tracked it over that same ignore.
        self.git(&["add", "--force", "--", relative]);
        self.git(&[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "the project's own rules",
        ]);
        self.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        self
    }

    /// The stub. Every branch records the call it was given, so what a verb
    /// passed is read from what the child received.
    ///
    /// The `--bg` branch runs in the seat's own worktree, which is what lets an
    /// arm ask it to make a branch there before it fails — the case a rollback
    /// must not delete.
    fn write_stub(&self) {
        let body = format!(
            "#!/bin/sh\n\
             gate() {{\n\
             \x20 n=0\n\
             \x20 while [ -f '{gate}' ] && [ $n -lt 200 ]; do\n\
             \x20   n=$((n+1))\n\
             \x20   sleep 0.05\n\
             \x20 done\n\
             }}\n\
             case \"$1\" in\n\
             \x20 agents)\n\
             \x20   [ -f '{roster_fails}' ] && exit 1\n\
             \x20   printf '%s\\n' \"$CLAUDE_CONFIG_DIR\" >> '{listing_dirs}'\n\
             \x20   if [ -f \"$CLAUDE_CONFIG_DIR/roster.json\" ]; then\n\
             \x20     /bin/cat \"$CLAUDE_CONFIG_DIR/roster.json\"\n\
             \x20   else\n\
             \x20     /bin/cat '{roster}'\n\
             \x20   fi\n\
             \x20   ;;\n\
             \x20 --bg)\n\
             \x20   printf '%s\\n' \"$@\" > '{start_argv}'\n\
             \x20   pwd > '{start_cwd}'\n\
             \x20   /bin/cat .claude/settings.local.json > '{settings_at_start}' 2>/dev/null\n\
             \x20   echo \"start\" >> '{calls}'\n\
             \x20   gate\n\
             \x20   b=$(/bin/cat '{branch}' 2>/dev/null)\n\
             \x20   [ -n \"$b\" ] && git branch \"$b\"\n\
             \x20   exit $(/bin/cat '{start_exit}' 2>/dev/null || echo 0)\n\
             \x20   ;;\n\
             \x20 stop)\n\
             \x20   echo \"stop $2\" >> '{calls}'\n\
             \x20   gate\n\
             \x20   if [ ! -f '{stop_keeps}' ]; then\n\
             \x20     printf '[]' > '{roster}'\n\
             \x20     [ -f \"$CLAUDE_CONFIG_DIR/roster.json\" ] \\\n\
             \x20       && printf '[]' > \"$CLAUDE_CONFIG_DIR/roster.json\"\n\
             \x20   fi\n\
             \x20   exit $(/bin/cat '{stop_exit}' 2>/dev/null || echo 0)\n\
             \x20   ;;\n\
             \x20 rm)\n\
             \x20   echo \"rm $2\" >> '{calls}'\n\
             \x20   /bin/cat '{rm_stdout}' 2>/dev/null\n\
             \x20   exit $(/bin/cat '{rm_exit}' 2>/dev/null || echo 0)\n\
             \x20   ;;\n\
             \x20 -p)\n\
             \x20   printf '%s\\n' \"$@\" > '{nudge_argv}'\n\
             \x20   echo \"nudge\" >> '{calls}'\n\
             \x20   exit $(/bin/cat '{nudge_exit}' 2>/dev/null || echo 0)\n\
             \x20   ;;\n\
             \x20 *) exit 64 ;;\n\
             esac\n",
            roster = self.roster.display(),
            roster_fails = self.roster_fails.display(),
            listing_dirs = self.listing_dirs.display(),
            stop_keeps = self.stop_keeps_the_roster.display(),
            start_argv = self.start_argv.display(),
            start_cwd = self.start_cwd.display(),
            settings_at_start = self.settings_at_start.display(),
            nudge_argv = self.nudge_argv.display(),
            calls = self.calls.display(),
            branch = self.start_makes_branch.display(),
            gate = self.gate.display(),
            start_exit = self.start_exit.display(),
            stop_exit = self.stop_exit.display(),
            rm_exit = self.rm_exit.display(),
            rm_stdout = self.rm_stdout.display(),
            nudge_exit = self.nudge_exit.display(),
        );
        std::fs::write(&self.stub, body).expect("the stub is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&self.stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
    }

    fn agent(&self) -> ClaudeCode {
        ClaudeCode::with_seams(
            self.stub.display().to_string(),
            self.home.join(".claude"),
            Duration::from_secs(20),
            self.machine.clone(),
            platform::child_path(&self.home),
            Some(self.stub.clone()),
            String::new(),
        )
    }

    fn roster(&self, body: &str) -> &Rig {
        std::fs::write(&self.roster, body).expect("the roster is written");
        self
    }

    /// The listing THIS directory serves, which the fleet's own does not: the
    /// seam that lets an arm tell the two reads apart.
    fn roster_under(&self, config_dir: &Path, body: &str) -> &Rig {
        std::fs::create_dir_all(config_dir).expect("the configuration directory is made");
        std::fs::write(config_dir.join("roster.json"), body).expect("the roster is written");
        self
    }

    /// Every configuration directory a listing was asked under, in call order.
    fn listing_dirs(&self) -> Vec<String> {
        std::fs::read_to_string(&self.listing_dirs)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The per-row configuration directory this machine gives a seat of this
    /// name — the same location `transient::spawn` chooses, spelled here so an
    /// arm can assert about it.
    fn config_dir_of(&self, seat: &str) -> PathBuf {
        self.machine.join("config").join(seat)
    }

    fn seam(&self, path: &Path, body: &str) -> &Rig {
        std::fs::write(path, body).expect("the seam is written");
        self
    }

    fn config_path(&self) -> PathBuf {
        self.machine.join("config.json")
    }

    /// The seat list, with whatever rows the arm names.
    fn write_config(&self, children: &str) {
        std::fs::write(
            self.config_path(),
            format!(
                "{{\"fleet_toml\": {policy}, \"children\": {children}}}\n",
                policy = json_string(&self.root.join("fleet.toml").display().to_string()),
            ),
        )
        .expect("the seat list is written");
    }

    fn config_bytes(&self) -> Vec<u8> {
        std::fs::read(self.config_path()).unwrap_or_default()
    }

    /// The seat list's row for this machine name, read off the raw document:
    /// the row is keyed by its id, and a transient seat's machine name is
    /// `agent-` and that id's short form.
    fn row_of(&self, seat: &str) -> serde_json::Value {
        let document: serde_json::Value =
            serde_json::from_slice(&self.config_bytes()).expect("the seat list is JSON");
        let named = |row: &serde_json::Value| {
            row["id"]
                .as_str()
                .and_then(|id| SeatId::parse(id).ok())
                .is_some_and(|id| format!("agent-{}", id.short()) == seat)
        };
        document["children"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| named(row)))
            .cloned()
            .unwrap_or_else(|| panic!("the seat list carries a row for {seat}: {document}"))
    }

    /// Every entry in the worktrees directory, sorted: a spawn's name is
    /// minted, so "no worktree was made" is an empty directory and not the
    /// absence of one name.
    fn worktree_entries(&self) -> Vec<String> {
        let mut entries: Vec<String> = std::fs::read_dir(&self.worktrees)
            .map(|dir| {
                dir.filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        entries.sort();
        entries
    }

    fn table_bytes(&self) -> Vec<u8> {
        std::fs::read(sessions::path_in(&self.machine)).unwrap_or_default()
    }

    fn table_path(&self) -> PathBuf {
        sessions::path_in(&self.machine)
    }

    /// Put a table on the disk that `sessions::read` answers `None` for WITH a
    /// cause — which is the reading a verb must not round to an empty fleet.
    fn corrupt_the_table(&self) -> Vec<u8> {
        let body = b"{ this is not a session table".to_vec();
        std::fs::write(self.table_path(), &body).expect("the table is written");
        body
    }

    /// Make the event stream unwritable, so an append fails where every other
    /// act in the verb succeeds. A DIRECTORY at the stream's path: the log
    /// opens the file for append, and a directory refuses that on both targets.
    fn block_the_stream(&self) {
        let path = self.machine.join("events.jsonl");
        let _ = std::fs::remove_file(&path);
        std::fs::create_dir_all(&path).expect("the blocking directory is made");
    }

    fn table(&self) -> sessions::Table {
        sessions::read(&sessions::path_in(&self.machine))
            .0
            .unwrap_or_default()
    }

    fn events(&self) -> Vec<serde_json::Value> {
        let Ok(body) = std::fs::read_to_string(self.machine.join("events.jsonl")) else {
            return Vec::new();
        };
        body.lines()
            .map(|line| serde_json::from_str(line).expect("every line is one JSON object"))
            .collect()
    }

    fn events_of(&self, kind: &str) -> Vec<serde_json::Value> {
        self.events()
            .into_iter()
            .filter(|event| event["type"] == kind)
            .collect()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(&self.calls).unwrap_or_default()
    }

    /// How many starts the stub has been given, which is what says a SECOND
    /// spawn has reached its window while a first one's start already stands in
    /// the call log.
    fn starts(&self) -> usize {
        self.calls().lines().filter(|line| *line == "start").count()
    }

    /// Hold the stub's next start or stop inside its window, until [`release`]
    /// lifts it. The stub's own wait is bounded at ten seconds, so an arm that
    /// panics before lifting it ends rather than hangs.
    fn hold(&self) {
        std::fs::write(&self.gate, "").expect("the gate is written");
    }

    fn release(&self) {
        std::fs::remove_file(&self.gate).expect("the gate is lifted");
    }

    /// Spin until `ready` answers, or the deadline passes. The answer is the
    /// arm's to assert on: a timeout here is a reading and never a panic.
    fn until(&self, deadline: Duration, ready: impl Fn() -> bool) -> bool {
        let stop = std::time::Instant::now() + deadline;
        while std::time::Instant::now() < stop {
            if ready() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        ready()
    }

    fn start_argv(&self) -> Vec<String> {
        std::fs::read_to_string(&self.start_argv)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The argv file as it was WRITTEN, unsplit. The stub prints one argument
    /// per line and a first turn is several lines long, so the lines are not the
    /// arguments and an argument carrying newlines can only be read here.
    fn start_argv_text(&self) -> String {
        std::fs::read_to_string(&self.start_argv).expect("the stub recorded an argv")
    }

    /// A roster row, as the agent's listing shapes one.
    /// The session id onto the seat's table row, which is what a first sighting
    /// fills and what a transcript is keyed by.
    fn sight(&self, seat: &str, session_id: &str) {
        let mut table = self.table();
        let row = table
            .newest_for_mut(seat)
            .expect("the spawn wrote a row for this seat");
        row.session_id = Some(session_id.to_string());
        sessions::write(&self.table_path(), &table).expect("the table is written back");
    }

    /// The stamp the seat's row was dispatched at, which is the end the wall
    /// time is measured from.
    fn dispatched_at(&self, seat: &str) -> u64 {
        self.table()
            .newest_for(seat)
            .expect("the spawn wrote a row for this seat")
            .dispatched_at
    }

    /// A transcript where the adapter's own path rule puts one, UNDER THE
    /// SEAT'S OWN configuration directory.
    ///
    /// The row's directory and not the home default: a spawned seat comes up
    /// under one of its own, so a transcript planted under the default is a
    /// file no reader of that seat would ever open.
    fn plant_transcript(
        &self,
        seat: &str,
        worktree: &str,
        session_id: &str,
        body: &str,
    ) -> PathBuf {
        let config_dir = self
            .table()
            .newest_for(seat)
            .and_then(|row| row.config_dir.clone())
            .map(PathBuf::from)
            .expect("the spawn recorded the seat's own configuration directory");
        let path = fleet_controller::adapter::transcript_path(&config_dir, worktree, session_id);
        std::fs::create_dir_all(path.parent().expect("the transcript has a parent"))
            .expect("the transcript's directory is made");
        std::fs::write(&path, body).expect("the transcript is written");
        path
    }

    fn row(&self, session: &str, short: &str, cwd: &str, pid: u32, status: &str) -> String {
        format!(
            "{{\"sessionId\": {session}, \"id\": {short}, \"cwd\": {cwd}, \"pid\": {pid}, \
             \"status\": {status}}}",
            session = json_string(session),
            short = json_string(short),
            cwd = json_string(cwd),
            status = json_string(status),
        )
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

/// A spawned seat's machine name: `agent-` and the last eight hex digits of a
/// freshly minted id.
fn assert_agent_name(seat: &str) {
    let short = seat.strip_prefix("agent-").unwrap_or("");
    assert!(
        short.len() == 8
            && short
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{seat:?} is not agent-<8 hex>"
    );
}

/// The policy these arms run under.
///
/// `start_watch_seconds` is 10, not 1. It bounds two waits of opposite shapes
/// and only one of them costs time: the START watch ends when the stub exits,
/// which is at once, so a wide window is free there; the retire's
/// roster-cleared wait ends when the stub's `stop` has rewritten the roster
/// file, which has also already happened, so a wide window is free there too.
/// What a NARROW one buys is a red the first time this suite runs on a loaded
/// box — the shape `a_descendant_holding_the_pipe_cannot_hold_the_poll_past_the_deadline`
/// was just widened for in the drive suite. The one arm that WANTS the deadline
/// to expire (`retire_refuses_...a_row_the_stop_does_not_clear`) holds the
/// roster still, so it spends the whole window on purpose and says so.
fn a_policy() -> Policy {
    policy::parse(
        "[controller]\nstart_watch_seconds = 10\nnudge_timeout_seconds = 30\n\
         nudge_model = \"a-cheap-model\"\ndefault_model = \"a-model\"\n\
         max_transient_busy = 3\n",
    )
    .expect("the policy parses")
}

/// A load reading and a cpu count no arm here is judged against by accident.
const CALM: Readings = Readings {
    load: Some(0.1),
    cpus: Some(8),
};

fn machine_of<'a>(rig: &'a Rig, agent: &'a ClaudeCode, policy: &'a Policy) -> Machine<'a> {
    machine_reading(rig, agent, policy, CALM)
}

fn machine_reading<'a>(
    rig: &'a Rig,
    agent: &'a ClaudeCode,
    policy: &'a Policy,
    readings: Readings,
) -> Machine<'a> {
    Machine {
        machine_dir: &rig.machine,
        agent,
        policy,
        project: "a-project",
        primary: &rig.primary,
        worktrees_dir: &rig.worktrees,
        readings,
        clock: &rig.clock,
    }
}

/// One spawn, with both machine readings handed in.
///
/// NOTHING HERE TOUCHES THE ENVIRONMENT. The two overrides are the process's
/// own, shared by every thread `cargo test` runs an arm on, and this file
/// forks children while those arms run — so a `set_var` here would be both a
/// reading another arm's belt could see and a write racing a fork. The
/// override path is measured where a variable is safe: `cli/tests/seat.rs`,
/// which sets it on the CHILD it drives.
fn spawned(
    rig: &Rig,
    policy: &Policy,
    load: f64,
    cpus: u32,
    first_turn: &str,
) -> Result<transient::Spawned, Refusal> {
    spawned_with(rig, policy, load, cpus, first_turn, None)
}

/// The same spawn cut from a NAMED BASE rather than from the trunk ref, which
/// is what a reviewer's and a returned builder's worktree is, and carrying a
/// model over the policy's default.
fn spawned_at(
    rig: &Rig,
    policy: &Policy,
    base: Option<&str>,
    model: Option<&str>,
) -> Result<transient::Spawned, Refusal> {
    let agent = rig.agent();
    let machine = machine_reading(
        rig,
        &agent,
        policy,
        Readings {
            load: Some(0.1),
            cpus: Some(8),
        },
    );
    transient::spawn(
        &machine,
        &Spawn {
            first_turn: "the first turn",
            model,
            settings: None,
            item: None,
            base,
            config_files: &[],
        },
        1_000,
    )
}

/// The same spawn with the settings document the caller would have rendered.
/// `None` is what a caller offering none passes, and it is what every arm above
/// this one runs under.
fn spawned_with(
    rig: &Rig,
    policy: &Policy,
    load: f64,
    cpus: u32,
    first_turn: &str,
    settings: Option<&str>,
) -> Result<transient::Spawned, Refusal> {
    let agent = rig.agent();
    let machine = machine_reading(
        rig,
        &agent,
        policy,
        Readings {
            load: Some(load),
            cpus: Some(cpus),
        },
    );
    transient::spawn(
        &machine,
        &Spawn {
            first_turn,
            model: None,
            settings,
            item: None,
            base: None,
            config_files: &[],
        },
        1_000,
    )
}

// ---- the fixture's own template home ----------------------------------------

/// A template home left half-built — a directory at the name carrying no
/// `ready` marker — is a home the build clears rather than one it renames onto.
///
/// The wedge goes under a base of this arm's own: the shared home is one every
/// other arm in this filter reads, and nextest runs them concurrently.
#[test]
fn a_half_built_template_home_is_rebuilt() {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let base = std::env::temp_dir().join(format!(
        "fleet-trunk-template-wedged-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("one-commit");
    std::fs::create_dir_all(&home).expect("the wedged home is created");
    std::fs::write(home.join("half-a-build.txt"), "").expect("the wedge holds a file");
    assert!(
        !home.join("ready").is_file(),
        "the wedge carries no marker: {}",
        home.display()
    );

    let repo = trunk_template_at(&base);

    assert!(
        home.join("ready").is_file(),
        "a template stands at {}",
        home.display()
    );
    assert!(
        !home.join("half-a-build.txt").is_file(),
        "and the half-built home is gone rather than merged into it"
    );
    assert_eq!(
        git_at(&repo, &["rev-parse", "refs/remotes/origin/main"]),
        git_at(&repo, &["rev-parse", "HEAD"]),
        "and the trunk ref a spawn cuts from stands"
    );

    std::fs::remove_dir_all(&base).expect("the arm's own base is cleared");
}

// ---- AC1: the belt ----------------------------------------------------------

/// WHICH of `getloadavg`'s three samples the belt is judged on: the five-minute
/// one, so a suite's burst does not read as a jammed box.
///
/// The samples are crafted and not the box's own. On a quiet machine the three
/// averages are equal, so an arm that read this one off the host would pass at
/// every index; 1.0, 5.0 and 15.0 are three numbers that name the index that
/// answered them.
#[test]
fn the_belt_reads_the_five_minute_sample() {
    assert_eq!(
        platform::LOAD_SAMPLE,
        1,
        "the five-minute average is getloadavg's second sample"
    );
    assert_eq!(
        platform::belt_sample(&[1.0, 5.0, 15.0]),
        Some(5.0),
        "the belt judges the five-minute average, not the one-minute and not the fifteen"
    );
    assert_eq!(
        platform::belt_sample(&[1.0]),
        None,
        "a platform that filled only the one-minute sample is a reading nobody has, \
         which is never a machine under no load"
    );
}

/// The load belt's first leg. The refusal prints BOTH readings, and it
/// creates nothing: the seat list is byte-identical and the worktrees directory
/// is empty.
///
/// RED-PROVED IN THE SAME ARM by lowering the reading one unit: at 9.0 against a
/// ceiling of 8.0 the spawn refuses, and at 8.0 it proceeds — so the refusal is
/// the comparison's and not a spawn that cannot run in this fixture.
#[test]
fn a_load_average_over_the_ceiling_refuses_and_creates_nothing() {
    let rig = Rig::new("belt-load");
    let policy = a_policy();
    let before = rig.config_bytes();

    let refusal =
        spawned(&rig, &policy, 9.0, 8, "/work").expect_err("9.00 over a ceiling of 8.00 refuses");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    assert!(
        refusal.message.contains("load average"),
        "{}",
        refusal.message
    );
    assert!(
        refusal
            .message
            .contains("9.00 (ceiling 8.00 = 8 cpu x 1.00)"),
        "the refusal prints the load leg's reading: {}",
        refusal.message
    );
    assert!(
        refusal
            .message
            .contains("transient seats mid-turn: 0 (cap 3)"),
        "and the OTHER leg beside it, which is the number a person waits on: {}",
        refusal.message
    );
    assert_eq!(rig.config_bytes(), before, "the seat list is untouched");
    assert_eq!(
        rig.worktree_entries(),
        Vec::<String>::new(),
        "no worktree was made"
    );
    assert!(rig.calls().is_empty(), "and the agent was never called");

    // The control, one unit down: the same call at the ceiling proceeds.
    let spawn = spawned(&rig, &policy, 8.0, 8, "/work").expect("8.00 is not over 8.00");
    assert_agent_name(&spawn.seat);
}

/// The load belt's second leg, counted off the roster: a live row in a
/// TRANSIENT row's worktree carrying the agent's busy word.
///
/// RED-PROVED by lowering the count one: at two mid-turn against a cap of one
/// the spawn refuses, and with one of the two rows idle it proceeds.
#[test]
fn the_transient_cap_refuses_when_more_seats_are_mid_turn_than_the_cap() {
    let rig = Rig::new("belt-cap");
    let policy = policy::parse("[controller]\nstart_watch_seconds = 10\nmax_transient_busy = 1\n")
        .expect("the policy parses");

    let one = rig.worktrees.join("agent-0a1b2c3d").display().to_string();
    let two = rig.worktrees.join("agent-4e5f6a7b").display().to_string();
    rig.write_config(&format!(
        "[{{\"id\": \"01a0d1f1-0aec-765f-9abe-00000a1b2c3d\", \"transient\": true, \
           \"worktrees\": {{\"a-project\": {one}}}}}, \
          {{\"id\": \"01a0d1f1-0aec-765f-9abe-00004e5f6a7b\", \"transient\": true, \
           \"worktrees\": {{\"a-project\": {two}}}}}, \
          {{\"id\": \"{NAMED_ID}\", \"name\": \"a-named-seat\", \
           \"worktrees\": {{\"a-project\": {named}}}}}]",
        one = json_string(&one),
        two = json_string(&two),
        named = json_string(&rig.primary.display().to_string()),
    ));
    let before = rig.config_bytes();
    rig.roster(&format!(
        "[{}, {}, {}]",
        rig.row("s-one", "a1", &one, 11, "busy"),
        rig.row("s-two", "b2", &two, 22, "busy"),
        // The control inside the fixture: a NAMED seat mid-turn is not counted,
        // so the number the belt reads is transient seats and not sessions.
        rig.row(
            "s-named",
            "c3",
            &rig.primary.display().to_string(),
            33,
            "busy"
        ),
    ));

    let refusal = spawned(&rig, &policy, 0.1, 8, "/work")
        .expect_err("two transient seats mid-turn is over a cap of one");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    assert!(
        refusal
            .message
            .contains("transient seats mid-turn: 2 (cap 1)"),
        "the refusal counts the two transient rows and not the named one: {}",
        refusal.message
    );
    // AC1's "the same" as the load arm's, in full: the OTHER leg's reading
    // beside it, no worktree made, and the agent never called.
    assert!(
        refusal
            .message
            .contains("0.10 (ceiling 8.00 = 8 cpu x 1.00)"),
        "the refusal prints the load leg beside the one that refused: {}",
        refusal.message
    );
    assert_eq!(rig.config_bytes(), before, "the seat list is untouched");
    assert_eq!(
        rig.worktree_entries(),
        Vec::<String>::new(),
        "no worktree was made"
    );
    assert!(
        !rig.calls().contains("start"),
        "and the agent was never started: {}",
        rig.calls()
    );

    // The control, one unit down: with one of the two idle the spawn proceeds.
    rig.roster(&format!(
        "[{}, {}]",
        rig.row("s-one", "a1", &one, 11, "busy"),
        rig.row("s-two", "b2", &two, 22, "idle"),
    ));
    let spawn = spawned(&rig, &policy, 0.1, 8, "/work").expect("one mid-turn is not over one");
    assert_agent_name(&spawn.seat);
    assert!(
        spawn.seat != "agent-0a1b2c3d" && spawn.seat != "agent-4e5f6a7b",
        "and the name is a fresh one, not a row's the list already carries: {}",
        spawn.seat
    );
}

/// An unreadable roster makes the cap leg COULD NOT TELL and refuses nothing —
/// a daemon nobody can ask must not wedge every spawn in the fleet — while the
/// load leg keeps its teeth.
#[test]
fn an_unreadable_roster_makes_the_cap_leg_could_not_tell_and_the_spawn_proceeds() {
    let rig = Rig::new("belt-unreadable");
    let policy = a_policy();
    rig.seam(&rig.roster_fails, "");

    let spawn = spawned(&rig, &policy, 0.1, 8, "/work")
        .expect("a cap leg nobody could read refuses nothing");
    assert_agent_name(&spawn.seat);
    assert!(
        spawn.belt.busy.is_none() && spawn.belt.busy_unreadable.is_some(),
        "the cap leg carries its cause: {:?}",
        spawn.belt
    );
    assert!(
        spawn
            .belt
            .lines()
            .contains("COULD NOT TELL — the roster could not be read, which is not zero"),
        "and says so in the line a person reads: {}",
        spawn.belt.lines()
    );

    // The control: with the roster still unreadable, the LOAD leg still refuses.
    let refusal = spawned(&rig, &policy, 9.0, 8, "/work")
        .expect_err("the load leg keeps its teeth over an unreadable roster");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    assert!(
        refusal.message.contains("load average"),
        "{}",
        refusal.message
    );
}

// ---- AC2: the spawn ---------------------------------------------------------

/// A spawned seat comes up under the plugin root the policy names, on the same
/// element and in the same position a named seat's start carries it: only a
/// loaded plugin root gives a session the overlay's hooks (lessons claude-code
/// D5), and a transient seat is the one that runs a dispatched item.
///
/// Read from the argv the CHILD received. The control is a second rig under a
/// policy that names no root, which carries no such element at all.
#[test]
fn a_spawn_starts_the_seat_under_the_plugin_root_the_policy_names() {
    let rig = Rig::new("plugin-root");
    let policy = policy::parse(
        "[controller]\nstart_watch_seconds = 10\ndefault_model = \"a-model\"\n\
         plugin_dir = \"/an/overlay\"\n",
    )
    .expect("the policy parses");

    let spawn = spawned(&rig, &policy, 0.1, 8, "the first turn").expect("the spawn lands");
    assert_agent_name(&spawn.seat);
    let argv = rig.start_argv();
    let at = argv
        .iter()
        .position(|word| word == "--plugin-dir")
        .unwrap_or_else(|| panic!("the flag is in the argv: {argv:?}"));
    assert_eq!(argv.get(at + 1).map(String::as_str), Some("/an/overlay"));
    assert_eq!(
        argv.get(at + 2).map(String::as_str),
        Some("the first turn"),
        "the element after the root's value is the first turn: {argv:?}"
    );

    // The control: the same spawn under a policy that names none.
    let bare = Rig::new("plugin-root-control");
    let spawn = spawned(&bare, &a_policy(), 0.1, 8, "the first turn").expect("the spawn lands");
    assert_agent_name(&spawn.seat);
    let argv = bare.start_argv();
    assert!(
        !argv.iter().any(|word| word == "--plugin-dir"),
        "a fleet that names no plugin root passes no such element: {argv:?}"
    );
}

/// A JSON settings document in the shape the pack's overlay carries one, with
/// the worktree rule's path left for the caller to fill.
fn a_settings_doc(worktree: &str) -> String {
    format!("{{\"permissions\":{{\"allow\":[\"Bash(make check:*)\",\"Edit(/{worktree}/**)\"]}}}}")
}

/// A transient seat's session comes up under permission rules the spawn wrote
/// into its own worktree: the posture refuses every writing call the session
/// holds no rule for, and a permission list cannot ride the plugin root the
/// overlay is loaded through.
///
/// Three claims, and the third is the one a later reading cannot make on its
/// own: the document is at the path the adapter names, `{worktree}` reads as
/// the seat's own checkout, and THE CHILD COULD READ IT — the stub copies the
/// file out of its own working directory on the way past, so the write is
/// proved to have landed before the start and not merely before the assertion.
#[test]
fn a_spawn_writes_the_seats_permission_rules_before_its_first_turn() {
    let rig = Rig::new("settings");
    let template = a_settings_doc(transient::WORKTREE);

    let spawn = spawned_with(&rig, &a_policy(), 0.1, 8, "the first turn", Some(&template))
        .expect("the spawn lands");
    let worktree = rig.worktrees.join(&spawn.seat);
    assert_eq!(spawn.worktree, worktree);

    let want = a_settings_doc(&worktree.display().to_string());
    let path = worktree.join(".claude/settings.local.json");
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display())),
        want,
        "the document is rendered with the seat's own worktree in the rule"
    );
    assert_eq!(
        std::fs::read_to_string(&rig.settings_at_start).unwrap_or_default(),
        want,
        "and the child read those same bytes out of its working directory as it came up"
    );
    assert_eq!(
        rig.events_of(events::SESSION_SPAWNED)[0]["payload"]["settings"],
        serde_json::json!("written"),
        "and the stream says the document was written rather than merged"
    );
}

/// The control on the arm above, and the other half of the rule that only a
/// spawned seat is given one: a spawn offered no document writes none, and a
/// NAMED seat's start never reaches this verb at all — the loop starts it
/// through `effect::spawn_woken`, whose worktree is a person's own.
#[test]
fn a_spawn_offered_no_settings_writes_none() {
    let rig = Rig::new("settings-none");
    let spawn = spawned(&rig, &a_policy(), 0.1, 8, "the first turn").expect("the spawn lands");
    let claude = spawn.worktree.join(".claude");
    assert!(
        !claude.exists(),
        "{} was created by a spawn that was offered nothing",
        claude.display()
    );
    assert!(
        std::fs::read_to_string(&rig.settings_at_start)
            .unwrap_or_default()
            .is_empty(),
        "and the child found nothing to read when it came up"
    );
    assert_eq!(
        rig.events_of(events::SESSION_SPAWNED)[0]["payload"]["settings"],
        serde_json::Value::Null,
        "and the stream carries no settings word for a spawn that wrote none"
    );
}

/// The same document with a deny list, which is the other half a merge folds.
fn a_settings_doc_with_deny(worktree: &str) -> String {
    format!(
        "{{\"permissions\":{{\"allow\":[\"Bash(make check:*)\",\"Edit(/{worktree}/**)\"],\
         \"deny\":[\"Bash(git push:*)\"]}}}}"
    )
}

/// A project may TRACK `.claude/settings.local.json` on its trunk, and every
/// transient worktree is cut from that trunk — so the spawn folds the pack's
/// lists into the document it finds instead of writing over it.
///
/// THE LOAD-BEARING ASSERTION IS THE PROJECT'S RULE, NOT THE PACK'S: an arm
/// that asked only whether the pack's rules were present would read green with
/// the overwrite still in place, which is what this file measured before.
/// `Bash(make check:*)` is on both sides on purpose: a rule the project already
/// carries is not appended a second time.
#[test]
fn a_spawn_merges_the_packs_rules_into_a_settings_document_the_project_tracks() {
    let rig = Rig::new("settings-merge");
    let project = "{\n  \"permissions\": {\n    \"allow\": [\"Bash(make lint:*)\", \
                   \"Bash(make check:*)\"],\n    \"deny\": [\"Bash(rm:*)\"]\n  },\n  \
                   \"model\": \"a-project-model\"\n}\n";
    rig.tracked_on_trunk(".claude/settings.local.json", project);

    let template = a_settings_doc_with_deny(transient::WORKTREE);
    let spawn = spawned_with(&rig, &a_policy(), 0.1, 8, "the first turn", Some(&template))
        .expect("the spawn lands");
    let worktree = rig.worktrees.join(&spawn.seat);
    assert_eq!(spawn.worktree, worktree);

    let path = worktree.join(".claude/settings.local.json");
    let read = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
    let merged: serde_json::Value =
        serde_json::from_str(&read).unwrap_or_else(|e| panic!("the merged document is JSON: {e}"));

    assert_eq!(
        merged["permissions"]["allow"],
        serde_json::json!([
            "Bash(make lint:*)",
            "Bash(make check:*)",
            format!("Edit(/{}/**)", worktree.display()),
        ]),
        "the project's own allow rule survives BESIDE the pack's, first and once"
    );
    assert_eq!(
        merged["permissions"]["deny"],
        serde_json::json!(["Bash(rm:*)", "Bash(git push:*)"]),
        "and its deny rule survives beside the pack's"
    );
    assert_eq!(
        merged["model"],
        serde_json::json!("a-project-model"),
        "and a key of the project's the pack knows nothing about is untouched"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(&rig.settings_at_start).unwrap_or_default()
        )
        .unwrap_or(serde_json::Value::Null),
        merged,
        "and the child read the merged document out of its working directory as it came up"
    );
    assert_eq!(
        rig.events_of(events::SESSION_SPAWNED)[0]["payload"]["settings"],
        serde_json::json!("merged"),
        "and the stream says which of the two happened"
    );
}

/// The whole of the spawn's happy path, read off the machine it left behind.
#[test]
fn a_spawn_makes_a_detached_worktree_a_row_and_a_session_and_prints_its_name() {
    let rig = Rig::new("spawn");
    let policy = a_policy();
    let trunk = rig.git(&["rev-parse", "refs/remotes/origin/main"]);

    let spawn = spawned(
        &rig,
        &policy,
        0.1,
        8,
        "the first turn\nand its second line\n",
    )
    .expect("the spawn lands");
    assert_agent_name(&spawn.seat);
    let seat = spawn.seat.as_str();

    // The worktree, detached at the trunk ref.
    let worktree = rig.worktrees.join(seat);
    assert!(worktree.is_dir(), "{} is there", worktree.display());
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git runs");
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        trunk,
        "the worktree is at origin/main"
    );
    let branch = std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["symbolic-ref", "--quiet", "HEAD"])
        .output()
        .expect("git runs");
    assert!(
        !branch.status.success(),
        "and it is DETACHED, so no branch is checked out in it"
    );

    // The seat-list row, read back off the file.
    let seats = config::read(&rig.config_path())
        .expect("the seat list parses")
        .seats;
    let row = seats
        .iter()
        .find(|row| row.machine_name() == seat)
        .expect("the row is on the file");
    assert!(
        row.name.is_none(),
        "a transient seat has no name of its own"
    );
    assert!(row.transient, "the row says transient");
    assert_eq!(row.model.as_deref(), Some("a-model"), "the policy's model");
    assert_eq!(
        row.worktrees,
        vec![("a-project".to_string(), worktree.display().to_string())]
    );

    // The start: the file's text is the LAST argument, and the posture is the
    // transient one rather than a named seat's.
    let argv = rig.start_argv();
    assert_eq!(argv.first().map(String::as_str), Some("--bg"));
    assert!(
        rig.start_argv_text()
            .ends_with("the first turn\nand its second line\n\n"),
        "the first turn's text is the last argument, WHOLE — the stub prints one \
         argument per line, so a turn of two lines is the last two: {:?}",
        rig.start_argv_text()
    );
    let flag = |name: &str| {
        argv.iter()
            .position(|word| word == name)
            .and_then(|at| argv.get(at + 1))
            .map(String::as_str)
    };
    assert_eq!(flag("--permission-mode"), Some(policy.posture_for(true)));
    assert_eq!(flag("--model"), Some("a-model"));
    assert_eq!(flag("--name"), Some(seat));

    // The session-table row, and the one event.
    let table = rig.table();
    let opened = table
        .newest_for(seat)
        .expect("the table carries the row this start opened");
    assert!(opened.transient);
    assert_eq!(opened.worktree, worktree.display().to_string());
    assert!(
        opened.first_turn.starts_with("the first turn"),
        "the occupant marker is the turn's text: {:?}",
        opened.first_turn
    );
    assert_eq!(rig.events_of(events::SESSION_SPAWNED).len(), 1);
    assert!(rig.events_of(events::SESSION_CRASHED).is_empty());

    // A second spawn mints a seat of its own and its own directory.
    let second = spawned(&rig, &policy, 0.1, 8, "another turn").expect("the second spawn lands");
    assert_agent_name(&second.seat);
    assert_ne!(second.seat, seat);
    assert!(rig.worktrees.join(&second.seat).is_dir());
}

/// The lock arm: two spawns started at once take two different seats, and the
/// file carries both rows.
///
/// The row is appended INSIDE the writer's lock, so this is what a
/// read-modify-write outside it would red — both threads would read an empty
/// list and each rename its own one-row version over the other's.
#[test]
fn two_concurrent_spawns_take_two_different_names() {
    let rig = Rig::new("spawn-race");
    let policy = a_policy();

    // Both threads are handed the same calm readings, so the only thing they
    // race each other on is the writer's lock, which is what is measured.
    let names: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let (rig, policy) = (&rig, &policy);
                scope.spawn(move || {
                    let agent = rig.agent();
                    let machine = machine_of(rig, &agent, policy);
                    transient::spawn(
                        &machine,
                        &Spawn {
                            first_turn: "a turn",
                            model: None,
                            settings: None,
                            item: None,
                            base: None,
                            config_files: &[],
                        },
                        1_000,
                    )
                    .map(|spawned| spawned.seat)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("the thread finishes"))
            .map(|spawned| spawned.expect("both spawns land"))
            .collect()
    });

    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 2, "two spawns took one name: {names:?}");
    for name in &sorted {
        assert_agent_name(name);
    }
    let seats = config::read(&rig.config_path())
        .expect("the seat list parses")
        .seats;
    assert_eq!(seats.len(), 2, "and the file carries both rows");
    let mut rows: Vec<String> = seats.iter().map(config::Seat::machine_name).collect();
    rows.sort();
    assert_eq!(
        rows, sorted,
        "and they are the two seats the spawns printed"
    );
}

/// A14 in this verb's clothes: a start that exits inside its watch window rolls
/// the worktree and the row back, and NEVER a branch.
#[test]
fn a_start_that_fails_rolls_back_the_worktree_and_the_row_and_never_the_branch() {
    let rig = Rig::new("spawn-rollback");
    let policy = a_policy();
    rig.seam(&rig.start_exit, "1\n");
    // The stub makes this branch in the worktree it was started in, BEFORE it
    // fails — which is the commit-bearing ref a rollback must leave alone.
    rig.seam(&rig.start_makes_branch, "a-branch-the-seat-made\n");

    let refusal = spawned(&rig, &policy, 0.1, 8, "a turn").expect_err("the start failed");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    assert!(
        refusal.message.contains("rolled back"),
        "{}",
        refusal.message
    );

    let crashed = rig.events_of(events::SESSION_CRASHED);
    assert_eq!(crashed.len(), 1, "one crash line: {crashed:?}");
    assert_eq!(crashed[0]["payload"]["phase"], "start");
    assert!(
        refusal.message.contains(
            crashed[0]["payload"]["output"]
                .as_str()
                .expect("the crash names the output file")
        ),
        "the refusal names the file the adapter wrote: {}",
        refusal.message
    );
    assert!(
        rig.events_of(events::SESSION_SPAWNED).is_empty(),
        "and no spawned line stands beside it"
    );

    assert_eq!(
        rig.worktree_entries(),
        Vec::<String>::new(),
        "the worktree is gone"
    );
    // The row is GONE, not the file byte-identical: the rollback rewrites the
    // seat list through the writer, so the bytes move and the row must not.
    let seats = config::read(&rig.config_path())
        .expect("the seat list parses")
        .seats;
    assert!(
        seats.is_empty(),
        "the seat-list row survived the rollback: {seats:?}"
    );
    assert!(
        rig.table().sessions.is_empty(),
        "and no session-table row was opened"
    );

    // THE BRANCH SURVIVES. This is the assertion the rollback exists to keep
    // true: refs are the landing verb's, and a branch a failed seat left is what
    // a re-dispatch resumes from.
    let branches = rig.git(&["branch", "--list", "a-branch-the-seat-made"]);
    assert!(
        branches.contains("a-branch-the-seat-made"),
        "the rollback deleted a branch: {branches:?}"
    );
}

// ---- AC3: the feed ----------------------------------------------------------

/// A spawned transient seat with a live idle row on the roster, for the feed and
/// retire arms to act on.
fn a_spawned_seat(rig: &Rig, policy: &Policy, status: &str) -> (String, u32) {
    let spawn = spawned(rig, policy, 0.1, 8, "the turn it came up with")
        .expect("the fixture's spawn lands");
    let worktree = rig.worktrees.join(&spawn.seat).display().to_string();
    // A pid this process has already reaped, so `process_alive` answers a
    // measured `false` at the retire's last probe rather than an unknown.
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("a child spawns");
    let pid = child.id();
    child.wait().expect("the child is reaped");
    rig.roster(&format!(
        "[{}]",
        rig.row("a-session", "ab12", &worktree, pid, status)
    ));
    (spawn.seat, pid)
}

/// The feed's four refusals, each with its own status.
#[test]
fn feed_refuses_a_named_row_an_absent_session_an_unreadable_roster_and_a_busy_one() {
    let rig = Rig::new("feed-refusals");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    // A named row: 6. Added beside the transient one the spawn wrote, through
    // the same file, so both kinds are on one list.
    let listed = std::fs::read_to_string(rig.config_path()).expect("the seat list is readable");
    let mut document: serde_json::Value =
        serde_json::from_str(&listed).expect("the seat list parses");
    document["children"]
        .as_array_mut()
        .expect("children is an array")
        .push(serde_json::json!({
            "id": NAMED_ID,
            "name": "a-named-seat",
            "worktrees": { "a-project": rig.primary.display().to_string() },
        }));
    std::fs::write(
        rig.config_path(),
        serde_json::to_string_pretty(&document).expect("it serializes"),
    )
    .expect("the seat list is written");

    let named =
        transient::feed(&machine, "a-named-seat", "a turn").expect_err("a named seat is not fed");
    assert_eq!(named.code, 6, "{}", named.message);
    assert!(named.message.contains("named seat"), "{}", named.message);

    // A name no row carries at all: 1, and not the 6 above.
    let unknown =
        transient::feed(&machine, "nobody", "a turn").expect_err("an unknown seat is refused");
    assert_eq!(unknown.code, 1, "{}", unknown.message);

    // No live row: 4.
    let before = rig.table_bytes();
    rig.roster("[]");
    let absent = transient::feed(&machine, &seat, "a turn").expect_err("nothing to feed");
    assert_eq!(absent.code, 4, "{}", absent.message);
    assert_eq!(rig.table_bytes(), before, "and the table is untouched");

    // An unreadable roster: 3, which is not the 4 above — a session nobody could
    // ask is a question and not an absence.
    rig.seam(&rig.roster_fails, "");
    let unreadable =
        transient::feed(&machine, &seat, "a turn").expect_err("an unreadable roster is a question");
    assert_eq!(unreadable.code, 3, "{}", unreadable.message);
    let _ = std::fs::remove_file(&rig.roster_fails);

    // A live row the agent reports mid-turn: 1, and the table byte-identical.
    let worktree = rig.worktrees.join(&seat).display().to_string();
    rig.roster(&format!(
        "[{}]",
        rig.row("a-session", "ab12", &worktree, 4242, "busy")
    ));
    let busy = transient::feed(&machine, &seat, "a turn").expect_err("a seat holding a turn");
    assert_eq!(busy.code, 1, "{}", busy.message);
    assert!(busy.message.contains(&seat), "{}", busy.message);
    assert_eq!(rig.table_bytes(), before, "the table is byte-identical");
    assert!(
        !rig.calls().contains("nudge"),
        "and no turn was delivered: {}",
        rig.calls()
    );
}

/// The feed's happy path: the marker moves, the turn is delivered, and the
/// move is journaled on the stream.
#[test]
fn a_live_idle_row_is_fed_and_the_occupant_marker_moves() {
    let rig = Rig::new("feed");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let fed = transient::feed(&machine, &seat, "the next turn\nwith a second line\n")
        .expect("an idle seat is fed");
    assert_eq!(fed.prior, "the turn it came up with");

    let argv = std::fs::read_to_string(&rig.nudge_argv).expect("the stub recorded the turn");
    assert!(
        argv.contains("the next turn"),
        "the file's text reached the agent: {argv}"
    );

    let marker = rig
        .table()
        .newest_for(&seat)
        .expect("the row stands")
        .first_turn
        .clone();
    assert_eq!(marker, "the next turn\nwith a second line\n");

    let nudged = rig.events_of(events::SESSION_NUDGED);
    assert_eq!(nudged.len(), 1, "one line for one move: {nudged:?}");
    assert_eq!(
        nudged[0]["payload"]["prior_first_turn"],
        "the turn it came up with"
    );
    assert_eq!(nudged[0]["payload"]["first_turn"], "the next turn");
    assert_eq!(nudged[0]["payload"]["outcome"], "delivered");
    assert_eq!(nudged[0]["payload"]["put_back"], false);
}

/// A delivery the adapter reports failed puts the marker back, says so in a
/// SECOND line, and leaves the table byte-identical to before the feed.
#[test]
fn a_delivery_the_adapter_refuses_puts_the_occupant_marker_back() {
    let rig = Rig::new("feed-putback");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let before = rig.table_bytes();
    rig.seam(&rig.nudge_exit, "1\n");

    let refusal = transient::feed(&machine, &seat, "the turn that will not land")
        .expect_err("a delivery the adapter refuses");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    assert!(refusal.message.contains("put back"), "{}", refusal.message);

    assert_eq!(
        rig.table_bytes(),
        before,
        "the table is byte-identical to before the feed"
    );
    let nudged = rig.events_of(events::SESSION_NUDGED);
    assert_eq!(nudged.len(), 2, "the attempt and the put-back: {nudged:?}");
    assert_eq!(nudged[0]["payload"]["put_back"], false);
    assert_eq!(nudged[1]["payload"]["put_back"], true);
    assert_eq!(
        nudged[1]["payload"]["first_turn"], "the turn it came up with",
        "and the second line names the turn the marker went back to"
    );
}

// ---- AC4: the retire --------------------------------------------------------

/// The retire's happy path, verified from outside.
#[test]
fn a_retire_stops_removes_prunes_drops_both_rows_and_prints_the_reclaim() {
    let rig = Rig::new("retire");
    let policy = a_policy();
    let (seat, pid) = a_spawned_seat(&rig, &policy, "idle");
    // A branch made in the worktree before the retire, which must survive it.
    let worktree = rig.worktrees.join(&seat);
    std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["branch", "a-branch-the-seat-left"])
        .output()
        .expect("git runs");

    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let reclaimed = transient::retire(&machine, &seat, false).expect("the retire lands");

    assert_eq!(reclaimed.pid, Some(pid));
    assert!(
        reclaimed.bytes.is_some_and(|bytes| bytes > 0),
        "the reclaim carries the worktree's bytes, measured before the removal: {:?}",
        reclaimed.bytes
    );
    assert_eq!(
        reclaimed.removal.as_deref(),
        Some("removed ab12"),
        "the reclaim names the address the removal was issued against"
    );

    let calls = rig.calls();
    assert!(
        calls.contains("stop ab12"),
        "stopped BY ITS SHORT ID: {calls}"
    );
    assert!(
        calls.contains("rm ab12"),
        "and removed by the same: {calls}"
    );

    assert!(!worktree.exists(), "the worktree is gone");
    let listed = rig.git(&["worktree", "list", "--porcelain"]);
    assert!(
        !listed.contains(&worktree.display().to_string()),
        "and git no longer registers it: {listed}"
    );
    let seats = config::read(&rig.config_path())
        .expect("the seat list parses")
        .seats;
    assert!(
        !seats.iter().any(|row| row.machine_name() == seat),
        "the seat-list row is dropped"
    );
    assert!(
        rig.table().newest_for(&seat).is_none(),
        "and so is the session-table row"
    );

    let stopped = rig.events_of(events::SESSION_STOPPED);
    assert_eq!(stopped.len(), 1, "one line: {stopped:?}");
    assert_eq!(stopped[0]["payload"]["pid"], pid);
    assert_eq!(stopped[0]["payload"]["dead"], false);
    assert_eq!(stopped[0]["payload"]["bytes"], reclaimed.bytes.unwrap());

    let branches = rig.git(&["branch", "--list", "a-branch-the-seat-left"]);
    assert!(
        branches.contains("a-branch-the-seat-left"),
        "the retire deleted a branch: {branches:?}"
    );
}

/// Spawn, retire, spawn again: the second seat is a new one. Its id and its
/// machine name are not the retired seat's, so nothing the first left behind —
/// an order, a branch name, a configuration directory — can be inherited by
/// the second. Red on a counter that hands the lowest free name back out,
/// under which both seats took the counter's first name.
#[test]
fn a_spawn_after_a_retire_mints_a_seat_the_retired_one_never_was() {
    let rig = Rig::new("retire-then-spawn");
    let policy = a_policy();
    let (first, _) = a_spawned_seat(&rig, &policy, "idle");
    let first_id = rig.row_of(&first)["id"].clone();

    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    transient::retire(&machine, &first, false).expect("the retire lands");

    let second = spawned(&rig, &policy, 0.1, 8, "another turn").expect("the second spawn lands");
    assert_ne!(
        second.seat, first,
        "the retired seat's name was handed out again"
    );
    assert_agent_name(&first);
    assert_agent_name(&second.seat);
    assert_eq!(second.worktree, rig.worktrees.join(&second.seat));

    let second_id = rig.row_of(&second.seat)["id"].clone();
    for id in [&first_id, &second_id] {
        let id = id
            .as_str()
            .unwrap_or_else(|| panic!("the row carries an id: {id}"));
        assert!(
            fleet_core::seat::identity::SeatId::parse(id).is_ok(),
            "{id} is a whole seat id"
        );
    }
    assert_ne!(
        second_id, first_id,
        "the retired seat's id was handed out again"
    );
}

/// The per-row configuration directory: the spawn makes one and the session row
/// names it, every listing about the seat is asked UNDER it, and the retire
/// takes it back — after the outside probes, which read through it.
///
/// The two listings are made to DIFFER: the fleet's names no live row and the
/// row's own names the session. On the fleet's answer alone the retire takes its
/// no-live-session branch, never stops anything, and removes the worktree of a
/// running session — so the assertions below are the difference and not a
/// restatement.
#[test]
fn a_retire_reads_and_acts_under_the_rows_own_configuration_directory() {
    let rig = Rig::new("retire-per-row-directory");
    let policy = a_policy();
    let (seat, pid) = a_spawned_seat(&rig, &policy, "idle");
    let config_dir = rig.config_dir_of(&seat);
    let worktree = rig.worktrees.join(&seat).display().to_string();

    // The spawn made it, and the row names it.
    assert!(
        config_dir.is_dir(),
        "the spawn made {}",
        config_dir.display()
    );
    assert_eq!(
        rig.table()
            .newest_for(&seat)
            .and_then(|row| row.config_dir.clone())
            .as_deref(),
        Some(config_dir.display().to_string().as_str()),
        "and the session row names it"
    );

    // THE FLEET'S listing names no live row; the ROW'S OWN names the session.
    rig.roster("[]");
    rig.roster_under(
        &config_dir,
        &format!("[{}]", rig.row("a-session", "ab12", &worktree, pid, "idle")),
    );

    // The listings the SPAWN already made — its belt reads the fleet's — so the
    // assertion below is about this retire's own reads and not the fixture's.
    let before = rig.listing_dirs().len();

    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let reclaimed = transient::retire(&machine, &seat, false).expect("the retire lands");

    // It SAW the session, which the fleet's listing does not carry: the pid and
    // the address both come off the row's own listing.
    assert_eq!(
        reclaimed.pid,
        Some(pid),
        "the live row was seen through the row's own directory"
    );
    assert_eq!(reclaimed.removal.as_deref(), Some("removed ab12"));
    let calls = rig.calls();
    assert!(calls.contains("stop ab12"), "and stopped: {calls}");
    assert!(calls.contains("rm ab12"), "and removed: {calls}");

    // Every listing was asked UNDER that directory and none under the fleet's,
    // which is what the fold has to get right for the probes above to mean
    // anything.
    let dirs = rig.listing_dirs();
    let mine = &dirs[before..];
    assert!(
        !mine.is_empty(),
        "the retire made listings at all: {dirs:?}"
    );
    assert!(
        mine.iter().all(|d| Path::new(d) == config_dir),
        "every listing this retire made was under {}: {mine:?}",
        config_dir.display()
    );
    // The control on that: the spawn's own belt read DID go through the fleet's,
    // so the two directories are distinguishable in this rig and the assertion
    // above is a difference rather than a tautology.
    assert!(
        dirs[..before].iter().any(|d| Path::new(d) != config_dir),
        "the fixture's own reads went through the fleet's directory: {dirs:?}"
    );

    // And the directory goes with the seat.
    assert!(
        !config_dir.exists(),
        "{} is gone after the retire",
        config_dir.display()
    );
}

/// The retire's refusals: a named seat, an unreadable roster, and a row the
/// stop does not clear — the last of which removes NOTHING.
#[test]
fn retire_refuses_a_named_seat_an_unreadable_roster_and_a_row_the_stop_does_not_clear() {
    let rig = Rig::new("retire-refusals");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let listed = std::fs::read_to_string(rig.config_path()).expect("the seat list is readable");
    let mut document: serde_json::Value =
        serde_json::from_str(&listed).expect("the seat list parses");
    document["children"]
        .as_array_mut()
        .expect("children is an array")
        .push(serde_json::json!({
            "id": NAMED_ID,
            "name": "a-named-seat",
            "worktrees": { "a-project": rig.primary.display().to_string() },
        }));
    std::fs::write(
        rig.config_path(),
        serde_json::to_string_pretty(&document).expect("it serializes"),
    )
    .expect("the seat list is written");

    let named = transient::retire(&machine, "a-named-seat", false)
        .expect_err("a named seat rests, it does not retire");
    assert_eq!(named.code, 6, "{}", named.message);

    rig.seam(&rig.roster_fails, "");
    let unreadable =
        transient::retire(&machine, &seat, false).expect_err("an unreadable roster is a question");
    assert_eq!(unreadable.code, 3, "{}", unreadable.message);
    let _ = std::fs::remove_file(&rig.roster_fails);

    // A stop the roster does not answer: the row stays, and nothing is removed.
    //
    // The ONE arm here that spends its whole start-watch window on purpose —
    // the wait ends only at the deadline, because the roster never clears — so
    // it runs on a one-second policy of its own rather than on `a_policy`'s ten.
    // The window costs no wall clock under the rig's fake clock; what it does
    // bound is the NUMBER OF ROSTER LISTINGS inside it, and each of those is a
    // real child. One second is five; ten would be fifty.
    let brief =
        policy::parse("[controller]\nstart_watch_seconds = 1\n").expect("the policy parses");
    let machine = machine_of(&rig, &agent, &brief);
    let worktree = rig.worktrees.join(&seat);
    let config_before = rig.config_bytes();
    let table_before = rig.table_bytes();
    rig.seam(&rig.stop_keeps_the_roster, "");
    let stuck = transient::retire(&machine, &seat, false)
        .expect_err("a row the stop does not clear is a refusal");
    assert_eq!(stuck.code, 1, "{}", stuck.message);
    assert!(
        stuck.message.contains("nothing was removed"),
        "{}",
        stuck.message
    );
    assert!(worktree.exists(), "the worktree stands");
    assert_eq!(rig.config_bytes(), config_before, "the seat list stands");
    assert_eq!(rig.table_bytes(), table_before, "the table stands");
    assert!(
        !rig.calls().contains("rm "),
        "and the removal was never issued: {}",
        rig.calls()
    );
    // The whole window, and not one slice more: five 200 ms slices against the
    // one-second policy above. Fake time is the witness that the wait HAPPENED
    // while costing none of the box's, so a wait that quietly stopped being
    // spent — or one spent against the thread instead of the clock — reds here.
    assert_eq!(
        rig.clock.spent(),
        Duration::from_secs(1),
        "the refusal came at the end of the start-watch window"
    );
}

/// The withdrawal seam, on both sides: the record's half of a retire is the
/// LAST act that can still stop it, so a withdrawal that will not write leaves
/// the seat-list row — and the name — exactly where it was.
///
/// The seam is driven with closures rather than a store, because what is under
/// test here is the ORDER of two acts: this crate reaches no work graph, and
/// what the withdrawal itself writes is the core suite's.
#[test]
fn a_retire_whose_withdrawal_refuses_never_frees_the_name() {
    let rig = Rig::new("retire-withdrawal");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let listed = rig.config_bytes();
    let refused = transient::retire_with(&machine, &seat, false, &|_| {
        Err(Refusal {
            code: 3,
            message: String::from("fx-held was not fully withdrawn"),
        })
    })
    .expect_err("a withdrawal that will not write stops the retire");

    assert_eq!(refused.code, 3, "{}", refused.message);
    assert!(
        refused.message.contains("fx-held") && refused.message.contains("row STANDS"),
        "the refusal carries the cause and says what is still standing: {}",
        refused.message
    );
    assert_eq!(
        rig.config_bytes(),
        listed,
        "the seat list stands, so the seat's row is not dropped"
    );

    // The same seat again, with the cause cleared: the name goes now, and what
    // the seam answered rides back on the reclaim.
    let reclaimed = transient::retire_with(&machine, &seat, false, &|going| {
        Ok(vec![format!("fx-held ({going})")])
    })
    .expect("the retire lands once the withdrawal can be written");

    assert_eq!(
        reclaimed.withdrawn,
        vec![format!("fx-held ({seat})")],
        "the caller is told what the retire took off the seat"
    );
    assert_ne!(
        rig.config_bytes(),
        listed,
        "and the row is off the list this time"
    );
}

/// `--dead` licenses the removal by a COMPLETED roster read that names no live
/// row, and refuses over one that does.
/// The pid probe's THIRD answer: a reading the platform cannot take is
/// could-not-tell and refuses at 3, never rounded to clean.
///
/// The specimen is pid 0, which the process read answers `None` for on both
/// platforms — 0 addresses the caller's own process group rather than a
/// process. The row is live by the roster's rule (it carries a pid), so the
/// retire runs its whole sequence and stops at the last probe.
#[test]
fn a_pid_the_platform_cannot_read_refuses_at_could_not_tell_and_never_clean() {
    let rig = Rig::new("retire-unknown-pid");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let worktree = rig.worktrees.join(&seat).display().to_string();
    rig.roster(&format!(
        "[{}]",
        rig.row("a-session", "ab12", &worktree, 0, "idle")
    ));
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let refusal = transient::retire(&machine, &seat, false).expect_err("a probe nobody could take");
    assert_eq!(refusal.code, 3, "{}", refusal.message);
    assert!(
        refusal.message.contains("pid 0") && refusal.message.contains("not verified"),
        "{}",
        refusal.message
    );

    // The control, one field down: the same sequence with a pid the platform CAN
    // read answers 0 — so the 3 above is the probe's and not this fixture's.
    let (other, _) = a_spawned_seat(&rig, &policy, "idle");
    transient::retire(&machine, &other, false).expect("a readable pid verifies");
}

#[test]
fn dead_retires_without_a_stop_and_refuses_over_a_live_row() {
    let rig = Rig::new("retire-dead");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let live = transient::retire(&machine, &seat, true)
        .expect_err("--dead over a roster naming a live row");
    assert_eq!(live.code, 1, "{}", live.message);
    assert!(
        live.message.contains("--dead was the wrong flag"),
        "{}",
        live.message
    );
    assert!(rig.worktrees.join(&seat).exists(), "nothing was removed");

    rig.roster("[]");
    let reclaimed = transient::retire(&machine, &seat, true).expect("--dead over an empty roster");
    assert!(
        reclaimed.dead,
        "the reclaim records that --dead licensed it"
    );
    assert_eq!(
        reclaimed.pid, None,
        "there was no live row to read one from"
    );
    assert!(
        !rig.calls().contains("stop "),
        "and no stop was issued at all: {}",
        rig.calls()
    );
    assert!(!rig.worktrees.join(&seat).exists(), "the worktree is gone");
    assert_eq!(
        rig.events_of(events::SESSION_STOPPED)[0]["payload"]["dead"],
        true
    );
}

// ---- the table: unreadable is not empty, and a writer holds the lock --------

/// A session table that will not parse is could-not-tell on EVERY verb, and the
/// file is byte-identical after.
///
/// `sessions::read`'s `None` covers a file that is absent as squarely as one
/// that is corrupt, and rounding the second to an empty table renames it over
/// the real one — taking every other seat's row, the halt latch and the cursor
/// with it, silently. The two are told apart by the cause beside the `None`.
#[test]
fn a_session_table_that_will_not_parse_refuses_on_every_verb_and_writes_nothing() {
    let rig = Rig::new("table-corrupt");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let planted = rig.corrupt_the_table();

    let fed = transient::feed(&machine, &seat, "a turn").expect_err("an unreadable table");
    assert_eq!(fed.code, 3, "{}", fed.message);
    assert!(
        fed.message.contains("not an empty fleet"),
        "{}",
        fed.message
    );
    assert_eq!(rig.table_bytes(), planted, "the table is byte-identical");

    let retired = transient::retire(&machine, &seat, false).expect_err("an unreadable table");
    assert_eq!(retired.code, 3, "{}", retired.message);
    assert_eq!(rig.table_bytes(), planted, "the table is byte-identical");
    assert!(
        rig.worktrees.join(&seat).exists(),
        "and the worktree still stands"
    );

    let spawn = spawned(&rig, &policy, 0.1, 8, "a turn").expect_err("an unreadable table");
    assert_eq!(spawn.code, 3, "{}", spawn.message);
    assert_eq!(rig.table_bytes(), planted, "the table is byte-identical");
    assert!(
        spawn.message.contains("rolled back"),
        "the spawn's refusal is below the rollback window, so it says what it undid: {}",
        spawn.message
    );
    assert_eq!(
        rig.worktree_entries(),
        vec![seat.clone()],
        "and the worktree that spawn made is gone again"
    );

    // THE CONTROL, and the other half of `sessions::read`'s contract: a table
    // that is ABSENT carries no cause and IS the empty table, so the same verb
    // proceeds. Without this the three refusals above would also be satisfied by
    // a build that refused on every table it read.
    std::fs::remove_file(rig.table_path()).expect("the table is removed");
    let spawn = spawned(&rig, &policy, 0.1, 8, "a turn")
        .expect("a table that was never written is not a defect");
    assert_agent_name(&spawn.seat);
    assert_ne!(spawn.seat, seat);
}

/// A verb's read-modify-write of the session table WAITS on the table's lock.
///
/// This is the arm and not a race between two feeds, because a race is not a
/// measurement: driven both ways, two concurrent feeds pass with the lock and
/// pass without it — the interleaving that loses a row is real and is not one
/// a test can make happen on demand. What CAN be made to happen on demand is
/// the wait: the arm holds the lock itself and reads whether the verb has got
/// past it, which is the property the lock exists for and which nothing else
/// in this file would notice going missing.
///
/// The 400 ms is a floor and not a deadline. Every other feed here finishes in
/// single-digit milliseconds, so a feed still unfinished after 400 is one that
/// is waiting; and the release below is what turns the wait back into a green,
/// so a build that never took the lock reds on the first assertion rather than
/// hanging on the second.
#[test]
fn a_verbs_write_of_the_session_table_waits_on_its_lock() {
    let rig = Rig::new("table-lock");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");

    let held = platform::lock_beside(&rig.table_path()).expect("the arm takes the lock first");

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    std::thread::scope(|scope| {
        let (rig, policy) = (&rig, &policy);
        let inside = done.clone();
        let seat = seat.clone();
        let feeding = scope.spawn(move || {
            let agent = rig.agent();
            let machine = machine_of(rig, &agent, policy);
            let fed = transient::feed(&machine, &seat, "the next turn");
            inside.store(true, Ordering::SeqCst);
            fed
        });

        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !done.load(Ordering::SeqCst),
            "the feed got past a lock this arm is holding, so its write is unguarded"
        );

        // Released: the wait ends and the feed lands, which is what says the
        // assertion above read a WAIT and not a verb that had already failed.
        drop(held);
        feeding
            .join()
            .expect("the thread finishes")
            .expect("the feed lands once the lock is free");
    });

    assert_eq!(
        rig.table()
            .newest_for(&seat)
            .map(|row| row.first_turn.as_str()),
        Some("the next turn"),
        "and the move it was waiting to make is on the file"
    );
}

/// Two verbs writing the table on one machine lose no row.
///
/// A smoke reading beside the lock arm above, not a substitute for it: the
/// interleaving this would catch is not one the arm can force, so it is here to
/// say the two feeds agree on the file and NOT as the evidence that the lock
/// works.
#[test]
fn two_verbs_writing_the_session_table_lose_no_row() {
    let rig = Rig::new("table-race");
    let policy = a_policy();
    let (one, _) = a_spawned_seat(&rig, &policy, "idle");
    let (two, _) = a_spawned_seat(&rig, &policy, "idle");
    assert_ne!(one, two, "two seats, two rows");

    // One roster naming both worktrees live and idle, so both feeds get past
    // the roster read and reach the table.
    rig.roster(&format!(
        "[{}, {}]",
        rig.row(
            "session-one",
            "aa11",
            &rig.worktrees.join(&one).display().to_string(),
            4242,
            "idle"
        ),
        rig.row(
            "session-two",
            "bb22",
            &rig.worktrees.join(&two).display().to_string(),
            4343,
            "idle"
        ),
    ));

    std::thread::scope(|scope| {
        for (seat, turn) in [(&one, "the turn for one"), (&two, "the turn for two")] {
            let (rig, policy) = (&rig, &policy);
            scope.spawn(move || {
                let agent = rig.agent();
                let machine = machine_of(rig, &agent, policy);
                transient::feed(&machine, seat, turn).expect("both feeds land");
            });
        }
    });

    let table = rig.table();
    assert_eq!(
        table.newest_for(&one).map(|row| row.first_turn.as_str()),
        Some("the turn for one"),
        "the first seat's move survived the second's write"
    );
    assert_eq!(
        table.newest_for(&two).map(|row| row.first_turn.as_str()),
        Some("the turn for two"),
        "and the second's survived the first's"
    );
    assert_eq!(
        table.sessions.len(),
        2,
        "both rows stand: {:?}",
        table.sessions
    );
}

/// A SPAWN'S critical section is its own write and not its start: a feed issued
/// while a spawn sits in its start-watch window lands inside that window.
///
/// The gate is what makes this a measurement rather than a race. The stub's
/// start blocks in the window until this arm lifts it, so the spawn is provably
/// still inside it while the feed runs — the second assertion is that half, and
/// without it a green would also be satisfied by a spawn that had already
/// finished.
///
/// The 400 ms is a floor and not a deadline, the same floor
/// `a_verbs_write_of_the_session_table_waits_on_its_lock` reads the other way:
/// every feed in this file finishes in single-digit milliseconds, so one still
/// unfinished after 400 is one waiting on a lock.
///
/// THE LAST TWO ASSERTIONS ARE WHY THE LOCK IS RE-TAKEN RATHER THAN DROPPED:
/// the feed's move and the spawn's row are both on the file afterwards, which
/// says the spawn folded its row into a table read UNDER the lock after the
/// start, and not into a copy read before it — that copy carries the marker as
/// it stood before the feed, and writing it back would undo the feed silently.
#[test]
fn a_spawn_in_its_watch_window_does_not_block_a_feed() {
    let rig = Rig::new("spawn-window-feed");
    let policy = a_policy();
    let (fed, _) = a_spawned_seat(&rig, &policy, "idle");
    assert_eq!(
        rig.starts(),
        1,
        "the fixture's own spawn is the first start"
    );

    // From here the stub's start blocks until the release below.
    rig.hold();

    let spawn_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let feed_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let second = std::thread::scope(|scope| {
        let (rig, policy) = (&rig, &policy);
        let spawning_done = spawn_done.clone();
        let spawning = scope.spawn(move || {
            let spawned = spawned(rig, policy, 0.1, 8, "the second seat's turn");
            spawning_done.store(true, Ordering::SeqCst);
            spawned
        });

        assert!(
            rig.until(Duration::from_secs(10), || rig.starts() == 2),
            "the second spawn reached its start window: {}",
            rig.calls()
        );

        let feeding_done = feed_done.clone();
        let seat = fed.clone();
        let feeding = scope.spawn(move || {
            let agent = rig.agent();
            let machine = machine_of(rig, &agent, policy);
            let out = transient::feed(&machine, &seat, "the next turn");
            feeding_done.store(true, Ordering::SeqCst);
            out
        });

        std::thread::sleep(Duration::from_millis(400));
        assert!(
            feed_done.load(Ordering::SeqCst),
            "the feed is still waiting, so the spawn holds the table's lock across its start"
        );
        assert!(
            !spawn_done.load(Ordering::SeqCst),
            "and the spawn had not finished, which is what says the feed landed INSIDE its \
             window rather than after it"
        );

        rig.release();
        feeding
            .join()
            .expect("the feeding thread finishes")
            .expect("the feed lands");
        let spawn = spawning
            .join()
            .expect("the spawning thread finishes")
            .expect("the spawn lands once its window closes");
        assert_agent_name(&spawn.seat);
        assert_ne!(spawn.seat, fed);
        spawn.seat
    });

    let table = rig.table();
    assert_eq!(
        table.newest_for(&fed).map(|row| row.first_turn.as_str()),
        Some("the next turn"),
        "the feed's move survived the spawn's write"
    );
    assert!(
        table.newest_for(&second).is_some(),
        "and the spawn's own row is on the same file: {:?}",
        table.sessions
    );
}

/// A RETIRE'S critical section is its own write and not its stop: a feed issued
/// while a retire sits in its stop lands inside it.
///
/// The same gate, on the stub's `stop` and BEFORE the branch that empties the
/// roster — so the feed this arm runs meanwhile still reads the roster the
/// fixture wrote, and what it is waiting on can only be the table's lock.
///
/// The last two assertions are the re-read: the feed's move is on the file and
/// the retired seat's row is off it, which a retire writing back the copy it
/// read before its stop could not both do.
#[test]
fn a_retire_in_its_stop_does_not_block_a_feed() {
    let rig = Rig::new("retire-stop-feed");
    let policy = a_policy();
    let (going, pid) = a_spawned_seat(&rig, &policy, "idle");
    let (fed, _) = a_spawned_seat(&rig, &policy, "idle");
    let leaving = rig.worktrees.join(&going).display().to_string();
    let staying = rig.worktrees.join(&fed).display().to_string();
    rig.roster(&format!(
        "[{}, {}]",
        rig.row("a-session", "ab12", &leaving, pid, "idle"),
        rig.row("another-session", "cd34", &staying, pid, "idle"),
    ));

    rig.hold();

    let retire_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let feed_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    std::thread::scope(|scope| {
        let (rig, policy) = (&rig, &policy);
        let retiring_done = retire_done.clone();
        let going = going.clone();
        let retiring = scope.spawn(move || {
            let agent = rig.agent();
            let machine = machine_of(rig, &agent, policy);
            let out = transient::retire(&machine, &going, false);
            retiring_done.store(true, Ordering::SeqCst);
            out
        });

        assert!(
            rig.until(Duration::from_secs(10), || rig.calls().contains("stop ")),
            "the retire reached its stop: {}",
            rig.calls()
        );

        let feeding_done = feed_done.clone();
        let seat = fed.clone();
        let feeding = scope.spawn(move || {
            let agent = rig.agent();
            let machine = machine_of(rig, &agent, policy);
            let out = transient::feed(&machine, &seat, "the next turn");
            feeding_done.store(true, Ordering::SeqCst);
            out
        });

        std::thread::sleep(Duration::from_millis(400));
        assert!(
            feed_done.load(Ordering::SeqCst),
            "the feed is still waiting, so the retire holds the table's lock across its stop"
        );
        assert!(
            !retire_done.load(Ordering::SeqCst),
            "and the retire had not finished, which is what says the feed landed INSIDE its \
             stop rather than after it"
        );

        rig.release();
        feeding
            .join()
            .expect("the feeding thread finishes")
            .expect("the feed lands");
        retiring
            .join()
            .expect("the retiring thread finishes")
            .expect("the retire lands once its stop returns");
    });

    let table = rig.table();
    assert_eq!(
        table.newest_for(&fed).map(|row| row.first_turn.as_str()),
        Some("the next turn"),
        "the feed's move survived the retire's write"
    );
    assert!(
        table.newest_for(&going).is_none(),
        "and the retired seat's row is off the same file: {:?}",
        table.sessions
    );
}

/// A feed moves the NEWEST row for the seat, not the first in file order.
///
/// A seat is carried through as many rows as the loop opened, and `newest_for`
/// is what every other reader in the crate keys on — so a writer that took
/// `iter_mut().find()` would move a dispatch two successors ago and leave the
/// row the seat is actually sitting in untouched.
#[test]
fn a_feed_moves_the_newest_row_for_the_seat_and_not_the_first_in_file_order() {
    let rig = Rig::new("feed-newest");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");

    // A SECOND row for the same seat: LAST in file order and NEWEST by its
    // dispatch stamp. That is the shape that tells the two rules apart — the
    // spawn's row stays at index 0, so `iter().find()` meets it first while
    // `newest_for` answers the appended one. (With the rows the other way round
    // both rules give the same answer and the arm measures nothing.)
    let mut table = rig.table();
    let older_first_turn = table
        .newest_for(&seat)
        .expect("the spawn's row is there")
        .first_turn
        .clone();
    let newest = sessions::SessionRow {
        dispatched_at: 9_000,
        first_turn: "the turn the successor came up with".to_string(),
        dispatch_id: "a-later-dispatch".to_string(),
        ..table
            .newest_for(&seat)
            .expect("the spawn's row is there")
            .clone()
    };
    table.sessions.push(newest);
    assert_eq!(
        table.sessions[0].first_turn, older_first_turn,
        "the OLDER row is first in file order, which is what the two rules disagree about"
    );
    sessions::write(&rig.table_path(), &table).expect("the table is written");

    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let fed = transient::feed(&machine, &seat, "the next turn").expect("the seat is fed");
    assert_eq!(
        fed.prior, "the turn the successor came up with",
        "the marker that moved is the NEWEST row's"
    );

    let table = rig.table();
    let moved: Vec<(u64, String)> = table
        .sessions
        .iter()
        .map(|row| (row.dispatched_at, row.first_turn.clone()))
        .collect();
    assert!(
        moved.contains(&(9_000, "the next turn".to_string())),
        "the newest row carries the new turn: {moved:?}"
    );
    assert!(
        moved.contains(&(1_000, older_first_turn.clone())),
        "and the older row is untouched: {moved:?}"
    );
}

// ---- the retire's two missing acts -----------------------------------------

/// A retire over a roster that names no live row still issues the adapter's
/// REMOVE, addressed by the short id the session table remembers.
///
/// A seat whose session is already gone still has a session row to delete, and
/// a retire that skipped it would leave the agent holding one per dead seat.
#[test]
fn a_dead_seats_retire_removes_the_session_row_by_the_address_the_table_holds() {
    let rig = Rig::new("retire-dead-removes");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");

    // The table's row gets the address a sighting would have written onto it.
    let mut table = rig.table();
    let row = table.newest_for_mut(&seat).expect("the row is there");
    row.session_id = Some("a-session".to_string());
    row.short_id = Some("ab12".to_string());
    sessions::write(&rig.table_path(), &table).expect("the table is written");

    rig.roster("[]");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let reclaimed = transient::retire(&machine, &seat, true).expect("--dead over an empty roster");

    let calls = rig.calls();
    assert!(
        calls.contains("rm ab12"),
        "the removal is issued by the table's address: {calls}"
    );
    assert!(
        !calls.contains("stop "),
        "and no stop is issued, because there was nothing live to stop: {calls}"
    );
    assert_eq!(reclaimed.removal.as_deref(), Some("removed ab12"));

    // The control: a seat whose table row carries NO address issues no removal
    // at all and says so, rather than issuing one against an empty string.
    let (bare, _) = a_spawned_seat(&rig, &policy, "idle");
    rig.roster("[]");
    let reclaimed = transient::retire(&machine, &bare, true).expect("--dead over an empty roster");
    assert_eq!(
        reclaimed.removal, None,
        "no address anywhere is no removal, not a removal of nothing"
    );
}

/// AC4's last clause: a worktree a forced removal cannot delete is exit 1
/// NAMING IT, with both rows still standing.
///
/// The failure is forced by taking the `.git` file out of the worktree, which
/// makes `git worktree remove --force` answer "is not a working tree" while the
/// directory stays exactly where it was. That is git's own refusal and not a
/// seam this rig invented.
#[test]
fn a_worktree_a_forced_removal_cannot_delete_is_a_refusal_naming_it() {
    let rig = Rig::new("retire-stuck-worktree");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let worktree = rig.worktrees.join(&seat);
    std::fs::remove_file(worktree.join(".git")).expect("the worktree's git link is removed");

    let config_before = rig.config_bytes();
    let table_before = rig.table_bytes();
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let refusal =
        transient::retire(&machine, &seat, false).expect_err("a worktree git will not remove");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    // NAMED BY THE REFUSAL ITSELF, which is why this reads the FIRST characters
    // and not `contains`: git's own stderr for this failure echoes the path
    // ("fatal: '<path>' is not a working tree"), so a `contains` is satisfied by
    // the cause travelling and would pass over a refusal that names nothing.
    assert!(
        refusal
            .message
            .starts_with(&format!("{} could not be removed", worktree.display())),
        "the refusal opens by naming the worktree that survives: {}",
        refusal.message
    );
    // WHAT IT SAYS IS GONE IS WHAT WENT: the stop ran and the session row was
    // removed before the worktree was reached, so a refusal claiming nothing
    // else was removed would be describing a machine in a state this one is not
    // in. The removal's own answer is quoted, which is the line `rm ab12` in the
    // stub's call log.
    assert!(
        refusal
            .message
            .contains("the session is stopped and its session row is already gone (removed ab12)"),
        "the refusal names the two acts that already landed: {}",
        refusal.message
    );
    assert!(
        refusal
            .message
            .contains("the seat-list row and the session table both stand"),
        "and the two that did not: {}",
        refusal.message
    );
    assert!(
        rig.calls().contains("rm ab12"),
        "the removal it reports really was issued: {}",
        rig.calls()
    );
    assert!(worktree.exists(), "and it is still there");
    assert_eq!(rig.config_bytes(), config_before, "the seat list stands");
    assert_eq!(rig.table_bytes(), table_before, "the table stands");
}

/// A refusal from the verification comes AFTER both rows are dropped, so it
/// says so: a re-run would be refused at the seat list with "is not a row",
/// which tells the person nothing about what is still standing.
#[test]
fn a_refusal_after_the_rows_are_dropped_says_they_are_gone_and_what_stands() {
    let rig = Rig::new("retire-after-drop");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let worktree = rig.worktrees.join(&seat).display().to_string();
    // pid 0 is the probe the platform cannot answer, which lands in the
    // verification below the two drops.
    rig.roster(&format!(
        "[{}]",
        rig.row("a-session", "ab12", &worktree, 0, "idle")
    ));
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    let refusal = transient::retire(&machine, &seat, false).expect_err("the probe cannot answer");
    assert_eq!(refusal.code, 3, "{}", refusal.message);
    assert!(
        refusal
            .message
            .contains("ARE ALREADY\n             DROPPED")
            || refusal.message.contains("ARE ALREADY DROPPED"),
        "the refusal says the two rows are gone: {}",
        refusal.message
    );
    assert!(
        refusal.message.contains(&worktree) && refusal.message.contains("pid 0"),
        "and names what is left to finish by hand: {}",
        refusal.message
    );

    // The reading that makes it worth saying: the rows really are gone, so a
    // re-run meets the seat list rather than the probe.
    let again = transient::retire(&machine, &seat, false).expect_err("the row is gone");
    assert_eq!(again.code, 1, "{}", again.message);
    assert!(again.message.contains("names no seat"), "{}", again.message);
}

// ---- the ledger -------------------------------------------------------------

/// A line that cannot reach the stream is REPORTED, never swallowed: the act it
/// journals has already happened, so a silent failure leaves the machine moved
/// and the ledger saying nothing.
#[test]
fn a_journal_line_that_cannot_land_is_a_refusal_naming_the_event() {
    let rig = Rig::new("ledger-blocked");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);

    rig.block_the_stream();

    let fed = transient::feed(&machine, &seat, "the next turn")
        .expect_err("a feed whose journal cannot land");
    assert_eq!(fed.code, 3, "{}", fed.message);
    assert!(
        fed.message.contains(events::SESSION_NUDGED) && fed.message.contains("the act stands"),
        "the refusal names the line that did not land: {}",
        fed.message
    );
    // The act it journals DID happen, which is why the refusal is a 3 and not a
    // 1: the marker is where the feed moved it.
    assert_eq!(
        rig.table()
            .newest_for(&seat)
            .map(|row| row.first_turn.as_str()),
        Some("the next turn"),
        "the marker moved, and the ledger is what could not be written"
    );

    let retired =
        transient::retire(&machine, &seat, false).expect_err("a retire whose journal cannot land");
    assert_eq!(retired.code, 3, "{}", retired.message);
    assert!(
        retired.message.contains(events::SESSION_STOPPED),
        "{}",
        retired.message
    );
}

mod lessons {
    use super::*;

    /// A8 — removing a session answers three ways, and one of them deletes a
    /// checkout.
    ///
    /// The adapter's three answers are read from the exit AND the stdout,
    /// because two of them share an exit status; the retire then carries each
    /// one through to its own outcome. THE THIRD IS AN ALARM AND NOT A FAILURE:
    /// no discard flag is ever passed, so it must not be reachable — and a
    /// reader meets the deleted path in the report rather than meeting the
    /// missing directory.
    #[test]
    fn remove_answers_three_ways() {
        let rig = Rig::new("remove-three");
        let policy = a_policy();
        let agent = rig.agent();

        // 1. Succeeds and keeps the worktree: rc 0, nothing printed.
        assert!(matches!(agent.remove(None, "ab12"), RemoveAnswer::Removed));

        // 2. Refuses: rc 1, both the row and the worktree left standing. The
        //    cause travels, because "worktree has commits that are not pushed
        //    anywhere" is what the operator has to act on.
        rig.seam(&rig.rm_exit, "1\n");
        let refused = agent.remove(None, "ab12");
        let RemoveAnswer::Refused { cause } = &refused else {
            panic!("a non-zero exit is a refusal: {refused:?}");
        };
        assert!(cause.contains("exited 1"), "{cause}");

        // 3. Succeeds AND deletes the worktree: rc 0, printing the path. Told
        //    apart from (1) by the stdout alone, which is why the answer is not
        //    the exit status.
        rig.seam(&rig.rm_exit, "0\n");
        rig.seam(
            &rig.rm_stdout,
            "Removed session and its worktree at /some/checkout\n",
        );
        let deleted = agent.remove(None, "ab12");
        let RemoveAnswer::RemovedAWorktree { path } = &deleted else {
            panic!("a printed path is the third answer: {deleted:?}");
        };
        assert_eq!(path, "/some/checkout");

        // And the retire carries that third answer through as an ALARM naming
        // the path, rather than as a silent success.
        let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
        let machine = machine_of(&rig, &agent, &policy);
        let reclaimed = transient::retire(&machine, &seat, false).expect("the retire lands");
        let removal = reclaimed.removal.expect("the removal answered");
        assert!(
            removal.starts_with("ALARM"),
            "the third answer reaches the report as an alarm: {removal}"
        );
        assert!(removal.contains("/some/checkout"), "{removal}");
        assert_eq!(
            rig.events_of(events::SESSION_STOPPED)[0]["payload"]["removal"],
            removal,
            "and it is on the stream, where a person reading the morning's lines finds it"
        );
    }
}

/// The roster read every verb takes is the WHOLE-FLEET one, and an unreadable
/// listing is a third answer rather than an empty fleet (lessons claude-code
/// B4). The control is the same stub answering normally.
#[test]
fn an_unreadable_listing_is_a_third_answer_on_every_verb_that_asks() {
    let rig = Rig::new("unreadable");
    let agent = rig.agent();
    assert!(matches!(agent.status(None), RosterRead::Readable(_)));
    rig.seam(&rig.roster_fails, "");
    assert!(matches!(agent.status(None), RosterRead::Unreadable { .. }));
}

// ---- the priced retire -------------------------------------------------------

/// A transcript in the observe suite's own shape: four main-chain assistant
/// entries carrying a usage block, one of them summing to zero, one sidechain
/// entry carrying somebody else's window, and a torn last line.
///
/// The three numbers it pins are different on purpose, so no two of them can be
/// satisfied by one wrong reading: the last stated window is 60, the turns are
/// 4, and the sidechain's 2,600,000 is neither.
const A_TRANSCRIPT: &str = r#"
{"type":"user","message":{"usage":{"input_tokens":900}}}
{"type":"assistant","message":{"usage":{"input_tokens":1,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}}
{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":2600000}}}
{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30}}}
{"type":"assistant","message":{"usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"usage":{"input_tokens":40,"cache_read_input_tokens":20,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"usage":{"inp"#;

/// The cost, the branch and the commit all read BEFORE the reclaim, and
/// `session.retired` carrying them beside the reclaim.
#[test]
fn a_priced_retire_reads_the_cost_the_branch_and_the_commit_before_it_reclaims() {
    let rig = Rig::new("priced");
    let policy = a_policy();
    let (seat, pid) = a_spawned_seat(&rig, &policy, "idle");
    let worktree = rig.worktrees.join(&seat).display().to_string();
    rig.sight(&seat, "a-session");
    rig.plant_transcript(&seat, &worktree, "a-session", A_TRANSCRIPT);

    // A branch on the seat's own worktree, which is what the park a flight
    // raises records and what only a reading before the removal can take.
    std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["checkout", "--quiet", "-b", "a-builder/feat/the-work"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(&worktree)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git runs");
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();

    let dispatched_at = rig.dispatched_at(&seat);
    let now = dispatched_at + 90_000;
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let priced = transient::priced(&machine, &seat, "an-item", now).expect("the retire lands");

    assert_eq!(
        priced.cost.context_tokens,
        Some(60),
        "the last main-chain window, summed over all three figures — not the sidechain's"
    );
    assert_eq!(
        priced.cost.turns,
        Some(4),
        "main-chain assistant entries carrying a usage block, the zero-usage one COUNTED: a turn \
         that made a call is a turn whatever the call cost"
    );
    assert_eq!(
        priced.cost.wall_ms,
        Some(90_000),
        "the clock handed in, less the row's own dispatched_at"
    );
    assert_eq!(
        priced.cost.branch.as_deref(),
        Some("a-builder/feat/the-work")
    );
    assert_eq!(priced.cost.commit.as_deref(), Some(head.as_str()));

    // The retire underneath is unchanged and still verifies from outside.
    assert_eq!(priced.reclaimed.pid, Some(pid));
    assert!(priced.reclaimed.bytes.is_some_and(|bytes| bytes > 0));
    assert!(!Path::new(&worktree).exists(), "the worktree is gone");
    assert!(
        rig.table().newest_for(&seat).is_none(),
        "and so is the session-table row"
    );

    // BOTH LINES, because they answer different questions: the reclaim is what
    // every retire owes and the cost is what a retire inside a flight owes.
    let stopped = rig.events_of(events::SESSION_STOPPED);
    assert_eq!(stopped.len(), 1, "the reclaim is one line: {stopped:?}");
    let retired = rig.events_of(events::SESSION_RETIRED);
    assert_eq!(retired.len(), 1, "and the cost is another: {retired:?}");
    let payload = &retired[0]["payload"];
    assert_eq!(payload["seat"], seat);
    assert_eq!(payload["item"], "an-item");
    assert_eq!(payload["context_tokens"], 60);
    assert_eq!(payload["turns"], 4);
    assert_eq!(payload["wall_ms"], 90_000);
    assert_eq!(payload["branch"], "a-builder/feat/the-work");
    assert_eq!(payload["commit"], head);
    assert_eq!(payload["pid"], pid, "and the reclaim beside it");
    assert_eq!(payload["bytes"], priced.reclaimed.bytes.unwrap());
    assert_eq!(retired[0]["actor"], seat);
}

/// A transcript nobody can read prices nothing and the retire STILL RUNS: a
/// seat nobody can price is still a seat to reclaim, and the event says which
/// half was unread rather than reporting a zero.
#[test]
fn a_transcript_that_cannot_be_read_leaves_the_cost_null_and_reclaims_anyway() {
    let rig = Rig::new("priced-blind");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let worktree = rig.worktrees.join(&seat).display().to_string();
    // Sighted, so the session id is there to key a transcript by — and NO
    // transcript planted, which is the one variable this arm moves against the
    // one above it.
    rig.sight(&seat, "a-session");

    let dispatched_at = rig.dispatched_at(&seat);
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let priced = transient::priced(&machine, &seat, "an-item", dispatched_at + 1_000)
        .expect("the retire lands whether or not the seat could be priced");

    assert_eq!(priced.cost.context_tokens, None);
    assert_eq!(priced.cost.turns, None);
    assert_eq!(
        priced.cost.wall_ms,
        Some(1_000),
        "the wall time comes off the table and not off the transcript, so it is still read"
    );
    assert!(!Path::new(&worktree).exists(), "and the seat is reclaimed");

    let retired = rig.events_of(events::SESSION_RETIRED);
    assert_eq!(retired.len(), 1, "{retired:?}");
    let payload = &retired[0]["payload"];
    assert!(payload["context_tokens"].is_null(), "{payload}");
    assert!(payload["turns"].is_null(), "{payload}");
    assert_eq!(
        payload["transcript"], false,
        "the event SAYS the transcript was unread, which is what keeps a null from reading as a \
         session that took no turns"
    );
}

/// A RUN'S CLEANUP RETIRES THROUGH HERE, and the seat it retires can still hold
/// an open ordered item — a park leaves the order standing — so the priced
/// retire owes the record the same withdrawal the hand verb owes it: the leak
/// that left a parked item carrying an order naming a seat that no longer
/// exists.
///
/// The seam is driven with a closure rather than a store, because this crate
/// reaches no work graph: what the withdrawal itself writes onto an item is the
/// core suite's, what the cleanup hands in is the cli's, and what is under test
/// here is that the priced path RUNS what it was handed. The mutant is the
/// defect itself — the priced path handing `withdraws_nothing` — and it reds
/// the recorded call and the answer below it.
///
/// The ORDER of the two acts is the retire's own and is asserted in
/// `a_retire_whose_withdrawal_refuses_never_frees_the_name` above; this arm
/// takes the pass-through and the line it leaves on the stream.
#[test]
fn a_priced_retire_runs_the_withdrawal_the_cleanup_hands_it() {
    let rig = Rig::new("priced-withdrawal");
    let policy = a_policy();
    let (seat, _) = a_spawned_seat(&rig, &policy, "idle");
    let agent = rig.agent();
    let machine = machine_of(&rig, &agent, &policy);
    let now = rig.dispatched_at(&seat) + 1_000;

    let asked = std::cell::RefCell::new(Vec::new());
    let priced = transient::priced_with(&machine, &seat, "a-run", now, &|going| {
        asked.borrow_mut().push(going.to_string());
        Ok(vec![format!("fx-parked ({going})")])
    })
    .expect("the retire lands");

    assert_eq!(
        *asked.borrow(),
        vec![seat.clone()],
        "the seat going is the one the withdrawal is asked about, once"
    );
    assert_eq!(
        priced.reclaimed.withdrawn,
        vec![format!("fx-parked ({seat})")],
        "and the caller is told what the retire took off it"
    );

    // A cleanup runs inside a service with no terminal, so the stream is where
    // the withdrawal is read back.
    let retired = rig.events_of(events::SESSION_RETIRED);
    assert_eq!(retired.len(), 1, "{retired:?}");
    assert_eq!(
        retired[0]["payload"]["withdrawn"],
        serde_json::json!([format!("fx-parked ({seat})")]),
        "{}",
        retired[0]["payload"]
    );
}

// ---- the base: a seat cut from a named commit --------------------------------

/// A spawn carrying a base cuts the worktree AT THAT COMMIT, and the base it
/// answers with is read back off the worktree's own HEAD.
///
/// THE CONTROL IS IN THE ARM. The same rig spawns a second seat with no base at
/// all and that one is cut at the trunk, so the first seat's HEAD is the base's
/// doing and not a fixture whose trunk happens to sit there: the two commits
/// are different, and the two worktrees answer with the two of them.
#[test]
fn a_spawn_with_a_base_cuts_the_worktree_at_that_commit_and_one_without_cuts_the_trunk() {
    let rig = Rig::new("base-cut");
    let policy = a_policy();

    // One commit behind the trunk, which is where the first seat is cut.
    let behind = rig.git(&["rev-parse", "HEAD"]);
    std::fs::write(rig.primary.join("a-second-file.txt"), "moved on\n")
        .expect("the file is written");
    rig.git(&["add", "--", "a-second-file.txt"]);
    rig.git(&[
        "commit",
        "--quiet",
        "--no-gpg-sign",
        "-m",
        "the trunk moves",
    ]);
    rig.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let trunk = rig.git(&["rev-parse", "HEAD"]);
    assert_ne!(behind, trunk, "the fixture's two commits are two commits");

    let cut = spawned_at(&rig, &policy, Some(&behind), None).expect("the spawn lands");
    assert_eq!(
        cut.base.as_deref(),
        Some(behind.as_str()),
        "the base answered is the commit the worktree was cut at"
    );
    assert_eq!(
        head_of(&cut.worktree),
        behind,
        "and the worktree's own HEAD says so"
    );
    assert!(
        !cut.worktree.join("a-second-file.txt").exists(),
        "the tree is the one that commit describes, and not the trunk's"
    );

    let plain = spawned_at(&rig, &policy, None, None).expect("the second spawn lands");
    assert_eq!(
        head_of(&plain.worktree),
        trunk,
        "a spawn naming no base still cuts from the trunk ref"
    );
}

/// A base the primary cannot resolve REFUSES NAMING IT, with nothing made: no
/// name claimed on the seat list, no worktree, no session row.
#[test]
fn a_base_that_does_not_resolve_refuses_naming_it_and_makes_nothing() {
    let rig = Rig::new("base-unresolvable");
    let policy = a_policy();
    let before = rig.config_bytes();

    let refusal = spawned_at(
        &rig,
        &policy,
        Some("f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0"),
        None,
    )
    .expect_err("a commit that is not in the primary refuses");
    assert_eq!(refusal.code, 1, "{}", refusal.message);
    assert!(
        refusal
            .message
            .contains("f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0"),
        "the refusal names the base it could not resolve: {}",
        refusal.message
    );
    assert_eq!(
        rig.config_bytes(),
        before,
        "the seat list is byte-identical: no name was claimed"
    );
    assert_eq!(
        rig.worktree_entries(),
        Vec::<String>::new(),
        "and no worktree was made"
    );
    assert!(
        rig.events_of(events::SESSION_SPAWNED).is_empty(),
        "and nothing was started"
    );

    // THE CONTROL: the same rig, the same call, with a base that DOES resolve.
    let head = rig.git(&["rev-parse", "HEAD"]);
    spawned_at(&rig, &policy, Some(&head), None)
        .expect("the refusal above is the base's and not this fixture's");
}

/// The Order's model reaches the adapter's start, read off the stub's own argv,
/// and it is the value the caller passed rather than the policy's default.
#[test]
fn a_spawn_passes_the_callers_model_to_the_start_over_the_policys_default() {
    let rig = Rig::new("base-model");
    let policy = a_policy();
    assert_eq!(
        policy.model_for(None),
        "a-model",
        "the fixture's default is the value this arm must not read back"
    );

    let spawn = spawned_at(&rig, &policy, None, Some("a-named-model")).expect("the spawn lands");

    let argv = rig.start_argv();
    let flag = argv
        .iter()
        .position(|word| word == "--model")
        .and_then(|at| argv.get(at + 1))
        .map(String::as_str);
    assert_eq!(
        flag,
        Some("a-named-model"),
        "the start was given the caller's model: {argv:?}"
    );
    let row = config::read(&rig.config_path())
        .expect("the seat list parses")
        .seats
        .into_iter()
        .find(|seat| seat.machine_name() == spawn.seat)
        .expect("the row is on the file");
    assert_eq!(
        row.model.as_deref(),
        Some("a-named-model"),
        "and the row records the model the seat actually runs on"
    );
}

/// One worktree's HEAD, read the way the spawn reads it.
fn head_of(worktree: &Path) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["rev-parse", "HEAD"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git rev-parse HEAD in {}: {}",
        worktree.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
