//! Fixture tests for the decide-and-effect layers.
//!
//! The `lessons::` module below is the contract named in
//! `fleet/brain/lessons/*.md` § Test inventory: each fact the code in this slice
//! exercises owes a test under the exact name the inventory carries.
//!
//! The four verbs are driven against a STUB AGENT — a shell script this file
//! writes, which records the argv, the cwd and the `PATH` it received into files
//! the arms read. What a call passed is then a reading of what the child got and
//! never of what a log line said it sent.

use fleet_controller::adapter::claude_code::{self, ClaudeCode};
use fleet_controller::adapter::{Agent, AgentRow, RemoveAnswer, StartOutcome, StartSpec};
use fleet_controller::decide::{self, decide, SeatInput, Verdict};
use fleet_controller::effect::{self, Target};
use fleet_controller::events::{self, EventLog};
use fleet_controller::observe::RosterState;
use fleet_controller::platform;
use fleet_controller::policy::{self, Policy};
use fleet_controller::sessions::{self, SessionRow, Table};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A whole machine in a temp directory, with one stub agent on it.
struct Rig {
    root: PathBuf,
}

/// One executable `name` in `dir`, for an arm that needs a name on a search
/// path it owns. Returns where it was planted.
fn plant(dir: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).expect("the search directory is made");
    let file = dir.join(name);
    std::fs::write(&file, "#!/bin/sh\nexit 0\n").expect("the executable is written");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
        .expect("it is executable");
    file
}

impl Rig {
    fn new(name: &str) -> Rig {
        let root =
            std::env::temp_dir().join(format!("fleet-effects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("wt")).expect("the worktree is made");
        let rig = Rig { root };
        rig.write_stub(0);
        rig
    }

    fn machine(&self) -> PathBuf {
        self.root.join("machine")
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn worktree(&self) -> PathBuf {
        self.root.join("wt")
    }

    fn stub_path(&self) -> PathBuf {
        self.root.join("agent-stub")
    }

    fn argv_path(&self) -> PathBuf {
        self.root.join("argv")
    }

    fn cwd_path(&self) -> PathBuf {
        self.root.join("cwd")
    }

    fn path_path(&self) -> PathBuf {
        self.root.join("child-path")
    }

    /// A stub that records what it received and then exits `code`.
    ///
    /// It writes its own words to stdout and stderr as well, so the arm about
    /// where a start's output goes has something to find in the file.
    fn write_stub(&self, code: i32) {
        let body = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > '{argv}'\n\
             pwd > '{cwd}'\n\
             printf '%s' \"$PATH\" > '{path}'\n\
             echo 'the stub spoke on stdout'\n\
             echo 'the stub spoke on stderr' >&2\n\
             exit {code}\n",
            argv = self.argv_path().display(),
            cwd = self.cwd_path().display(),
            path = self.path_path().display(),
        );
        write(&self.stub_path(), &body);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(self.stub_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The same stub, kept alive past any watch window an arm here sets.
    fn write_slow_stub(&self) {
        let body = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > '{argv}'\n\
             pwd > '{cwd}'\n\
             printf '%s' \"$PATH\" > '{path}'\n\
             echo 'the stub spoke on stdout'\n\
             sleep 30\n",
            argv = self.argv_path().display(),
            cwd = self.cwd_path().display(),
            path = self.path_path().display(),
        );
        write(&self.stub_path(), &body);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(self.stub_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// An adapter whose READ binary and whose EFFECT binary are the same stub.
    ///
    /// The two are separate fields on purpose, so an arm about which one a call
    /// execs has to set them apart —
    /// `an_adapter_with_no_effect_binary_refuses_every_verb_and_execs_nothing`
    /// below sets the effect one to `None`, and every other arm here wants the
    /// two equal. Which BINARY an effect execs when they differ is a question
    /// only the built controller can be asked, and its arm is the drive suite's
    /// `effects::an_effect_execs_the_resolved_binary_and_not_the_first_claude_on_this_processs_path`.
    fn agent(&self) -> ClaudeCode {
        self.agent_with_effect_bin(Some(self.stub_path()))
    }

    fn agent_with_effect_bin(&self, effect_bin: Option<PathBuf>) -> ClaudeCode {
        self.agent_with_seams(effect_bin, String::new())
    }

    /// The same adapter with a CONFIGURED credential scope — what production
    /// carries when the operator set a config directory themselves. Empty, the
    /// reading above, is the unconfigured one.
    fn agent_with_credential_seam(&self, credential_dir: &str) -> ClaudeCode {
        self.agent_with_seams(Some(self.stub_path()), credential_dir.to_string())
    }

    fn agent_with_seams(&self, effect_bin: Option<PathBuf>, credential_dir: String) -> ClaudeCode {
        ClaudeCode::with_seams(
            self.stub_path().display().to_string(),
            self.home().join(".claude"),
            Duration::from_secs(10),
            self.machine(),
            platform::child_path(&self.home()),
            effect_bin,
            credential_dir,
        )
    }

    /// A stub that writes its whole environment to a file, and the file it
    /// writes to. Two arms below read the environment a child was handed, one
    /// for the NAMES it carries and one for two of their VALUES.
    fn write_env_recording_stub(&self) -> PathBuf {
        let leaked = self.root.join("leaked");
        let body = format!("#!/bin/sh\nenv > '{}'\n", leaked.display());
        write(&self.stub_path(), &body);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(self.stub_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        leaked
    }

    /// The argv the stub received, one element per line.
    fn argv(&self) -> Vec<String> {
        std::fs::read_to_string(self.argv_path())
            .unwrap_or_else(|e| panic!("the stub recorded no argv: {e}"))
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The argv file as it was written, unsplit. An element carrying newlines —
    /// the nudge prompt is several paragraphs — is several LINES here, so an arm
    /// about that element reads the text and never `argv().last()`.
    fn argv_text(&self) -> String {
        std::fs::read_to_string(self.argv_path()).expect("the stub recorded an argv")
    }

    /// The directory the stub found itself in, CANONICAL. `pwd` resolves
    /// symlinks and the temp directory is one on macOS, so an arm comparing it
    /// against the path it configured has to put both sides in the same form.
    fn recorded_cwd(&self) -> PathBuf {
        let printed = std::fs::read_to_string(self.cwd_path()).expect("the stub recorded its cwd");
        PathBuf::from(printed.trim())
    }

    /// The same path the arm configured, in that form.
    fn canonical_worktree(&self) -> PathBuf {
        std::fs::canonicalize(self.worktree()).expect("the worktree is there to resolve")
    }

    fn recorded_path(&self) -> String {
        std::fs::read_to_string(self.path_path()).expect("the stub recorded its PATH")
    }

    fn events(&self) -> Vec<serde_json::Value> {
        let path = self.machine().join("events.jsonl");
        let Ok(body) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        body.lines()
            .map(|l| serde_json::from_str(l).expect("every line is one JSON object"))
            .collect()
    }

    fn events_of(&self, kind: &str) -> usize {
        self.events().iter().filter(|e| e["type"] == kind).count()
    }

    fn log(&self) -> EventLog {
        EventLog::open(&self.machine().join("events.jsonl"))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the directory is made");
    }
    std::fs::write(path, body).expect("the file is written");
}

fn a_policy() -> Policy {
    policy::parse("[controller]\nstart_watch_seconds = 30\nnudge_timeout_seconds = 5\n")
        .expect("the policy parses")
}

fn a_target<'a>(worktree: &'a str, short: Option<&'a str>) -> Target<'a> {
    Target {
        seat_dir: "s1",
        display_name: "orla".to_string(),
        project: "demo",
        worktree,
        model: "claude-opus-5".to_string(),
        posture: "auto".to_string(),
        first_turn: "/wake s1".to_string(),
        transient: false,
        config_dir: None,
        item: None,
        settings: None,
        belt: None,
        run: None,
        session_id: Some("a-session"),
        short_id: short,
        context_tokens: Some(1_000),
    }
}

/// A seat input with every term at the reading that decides nothing, so an arm
/// that moves ONE of them is measuring that one.
fn a_seat(state: RosterState) -> SeatInput<'static> {
    SeatInput {
        seat_dir: "s1",
        state,
        unknown_cause: None,
        transient: false,
        pending_rest: false,
        pending_deliberate_end: false,
        context_tokens: Some(1_000),
        rest_threshold_tokens: 700_000,
        session_id: Some("a-session"),
        already_nudged: false,
        dispatch_age_ms: None,
        sighted: false,
        adopted_and_listed: false,
        arrival_window_ms: 45_000,
        halted: false,
        blind: 0,
        pidless_row: matches!(state, RosterState::Stopped),
        daemon_pid_changed: false,
        daemon_uptime_ms: None,
        seen_live: false,
        since_pidless_ms: None,
    }
}

/// A table row for a seat, sighted or not.
fn a_row_for(seat: &str, worktree: &str, dispatched_at: u64, session: Option<&str>) -> SessionRow {
    SessionRow {
        seat: seat.to_string(),
        project: "demo".to_string(),
        worktree: worktree.to_string(),
        name: "orla".to_string(),
        model: "a-model".to_string(),
        posture: "auto".to_string(),
        first_turn: format!("/wake {seat}"),
        transient: false,
        config_dir: None,
        item: None,
        dispatch_id: format!("dispatch-{dispatched_at}"),
        dispatched_at,
        session_id: session.map(str::to_string),
        short_id: None,
        first_seen_at: None,
        last_seen_at: None,
        adopted: None,
    }
}

/// The position of a flag in an argv, and the element after it.
fn flag_value<'a>(argv: &'a [String], flag: &str) -> &'a str {
    let at = argv
        .iter()
        .position(|a| a == flag)
        .unwrap_or_else(|| panic!("{flag} is not in the argv: {argv:?}"));
    argv.get(at + 1)
        .unwrap_or_else(|| panic!("{flag} is the last element of {argv:?}"))
}

mod lessons {
    use super::*;

    /// claude-code A3 — a hibernated session and a deliberately stopped one are
    /// identical across every roster field, so the discriminator cannot come
    /// from the row.
    ///
    /// Measured HERE as the property the table has to have: two inputs equal in
    /// every roster-derived term and differing only in the EVENT get different
    /// verdicts, and no roster state produces that difference on its own.
    #[test]
    fn hibernation_reads_as_a_deliberate_stop() {
        let stopped = SeatInput {
            seat_dir: "s1",
            state: RosterState::Stopped,
            unknown_cause: None,
            transient: false,
            pending_rest: false,
            pending_deliberate_end: false,
            context_tokens: Some(1_000),
            rest_threshold_tokens: 700_000,
            session_id: Some("a-session"),
            already_nudged: false,
            dispatch_age_ms: None,
            sighted: false,
            adopted_and_listed: false,
            arrival_window_ms: 45_000,
            halted: false,
            blind: 0,
            pidless_row: true,
            daemon_pid_changed: false,
            daemon_uptime_ms: None,
            seen_live: false,
            since_pidless_ms: None,
        };
        // The hibernated one: nothing was asked for, so the session comes back
        // in place with its context intact.
        assert_eq!(decide(&stopped), Verdict::Revive);

        // The deliberately stopped one: FIELD FOR FIELD the same row, and the
        // only thing that moved is the event the ending ritual emitted.
        let mut ended = stopped.clone();
        ended.pending_deliberate_end = true;
        assert_eq!(decide(&ended), Verdict::SpawnWoken);

        // THE 2.1.261 RE-READ, and the reason the discriminator cannot move to
        // the row: `done` is what a live IDLE session carries, with its pid and
        // status beside it, and what a hibernated, a stopped-from-idle and a
        // killed session carry pid-less. One word, four sessions, and only two
        // of the five state words name an end at all — each reached only from a
        // non-idle prior state.
        let rows: Vec<AgentRow> = serde_json::from_str(
            r#"[{"sessionId":"live-idle","id":"aa","cwd":"/wt/s1","pid":4242,
                 "state":"done","status":"idle"},
                {"sessionId":"hibernated","id":"bb","cwd":"/wt/s2","state":"done"},
                {"sessionId":"stopped-from-blocked","id":"cc","cwd":"/wt/s3","state":"stopped"},
                {"sessionId":"failed-mid-start","id":"dd","cwd":"/wt/s4","state":"failed"}]"#,
        )
        .expect("the roster parses");
        assert!(
            rows[0].is_live() && rows[0].marked_done(),
            "a live idle session carries the same word a finished one does"
        );
        assert!(!rows[1].is_live() && rows[1].marked_done());
        assert!(
            !rows[0].names_an_end() && !rows[1].names_an_end(),
            "so the word names no end, on either side of the pid"
        );
        assert!(
            rows[2].names_an_end() && rows[3].names_an_end(),
            "and the two the roster CAN name are the ones idle never reaches"
        );

        // And the event is a term of its own: a controller that tried to read
        // the difference off the row would have to find a roster state that
        // produces the spawn without it, and there is none among the six.
        for state in [
            RosterState::Present,
            RosterState::PromptBlocked,
            RosterState::Starting,
            RosterState::Stopped,
            RosterState::Unknown,
        ] {
            let mut row = stopped.clone();
            row.state = state;
            assert_ne!(
                decide(&row),
                Verdict::SpawnWoken,
                "{state:?} produced a spawn with no deliberate-end event"
            );
        }
    }

    /// claude-code A5 — a start with no model flag comes up on the cheapest
    /// available model, so the model is mandatory on every call and is what
    /// keeps a seat in the class its configuration declares.
    ///
    /// Read from what the CHILD received, and positionally: a `--name` given an
    /// empty value would swallow the flag after it, so the arm asserts that the
    /// element after the name is still `--model`.
    #[test]
    fn start_names_the_model() {
        let rig = Rig::new("start-names-the-model");
        let worktree = rig.worktree().display().to_string();
        let spec = StartSpec {
            seat_dir: "s1".to_string(),
            worktree: worktree.clone(),
            name: "orla".to_string(),
            model: "claude-opus-5".to_string(),
            posture: "auto".to_string(),
            first_turn: "/wake s1".to_string(),
            plugin_dir: None,
            config_dir: None,
        };
        let outcome = rig.agent().start(&spec, Duration::from_secs(30));
        assert!(
            matches!(outcome, StartOutcome::Started { .. }),
            "a stub that exits 0 inside the window is a start that did not fail: {outcome:?}"
        );

        let argv = rig.argv();
        assert_eq!(flag_value(&argv, "--model"), "claude-opus-5");
        assert_eq!(flag_value(&argv, "--name"), "orla");
        let name_at = argv.iter().position(|a| a == "--name").unwrap();
        assert_eq!(
            argv.get(name_at + 2).map(String::as_str),
            Some("--model"),
            "the flag after the name's value is --model, not its argument: {argv:?}"
        );
        assert!(argv.iter().any(|a| a == "--bg"));
        assert_eq!(argv.last().map(String::as_str), Some("/wake s1"));
        assert_eq!(rig.recorded_cwd(), rig.canonical_worktree());

        // The control on the model: a different model reaches the child as that
        // one, so the reading above is the spec's and not a constant in the
        // adapter.
        let mut other = spec.clone();
        other.model = "claude-sonnet-5".to_string();
        rig.agent().start(&other, Duration::from_secs(30));
        assert_eq!(flag_value(&rig.argv(), "--model"), "claude-sonnet-5");
    }

    /// claude-code A6 — identity and invocation address are different values:
    /// stopping by the full session id exits 1 with "No job matching", and only
    /// the short id on the roster row works.
    ///
    /// The subject is what the rest collection ISSUES: the session id is on the
    /// target beside the short id, and the argv the child received has to carry
    /// the address and not the key.
    #[test]
    fn stop_takes_the_short_id() {
        let rig = Rig::new("stop-takes-the-short-id");
        rig.agent()
            .stop(None, "ab12")
            .expect("a stub that exits 0 is a stop that landed");
        assert_eq!(rig.argv(), vec!["stop".to_string(), "ab12".to_string()]);

        // Through the collection, where the two values sit side by side: the
        // target carries `a-session` as its id and `ab12` as its address.
        let worktree = rig.worktree().display().to_string();
        let mut table = Table::default();
        let mut log = rig.log();
        let collected = effect::rest(
            &rig.agent(),
            &a_policy(),
            &a_target(&worktree, Some("ab12")),
            &mut log,
            &mut table,
            1_000,
        );
        assert!(matches!(collected, effect::Rested::Collected));
        let last = rig.argv();
        assert_eq!(
            last,
            vec!["rm".to_string(), "ab12".to_string()],
            "the removal takes the same address the stop did"
        );
        let rested = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == events::SESSION_RESTED)
            .expect("the collection wrote its event");
        assert_eq!(rested["payload"]["predecessor"], "a-session");
        assert_eq!(
            rested["payload"]["predecessor_address"], "ab12",
            "the event carries both, because they are two values"
        );
    }

    /// claude-code A14 — a start that cannot start prints its reason and exits
    /// non-zero inside the watch window, leaving no roster row. A controller
    /// that reads its child's own exit status collects that failure for free and
    /// turns it into a failed outcome with a logged cause.
    ///
    /// The control is the SAME call against a stub that exits 0, which is the
    /// start that did not fail: without it "Failed" would be an answer this
    /// adapter might give to everything.
    #[test]
    fn a_failed_start_exits_inside_the_watch_window() {
        let rig = Rig::new("failed-start");
        rig.write_stub(1);
        let worktree = rig.worktree().display().to_string();
        let mut table = Table::default();
        let mut log = rig.log();
        let outcome = effect::spawn_woken(
            &rig.agent(),
            &a_policy(),
            &a_target(&worktree, None),
            &mut log,
            &mut table,
            1_000,
        );
        assert_eq!(outcome, effect::Outcome::Failed);
        assert_eq!(rig.events_of(events::SESSION_CRASHED), 1);
        assert_eq!(
            rig.events_of(events::SESSION_SPAWNED),
            0,
            "a start that failed is not a session this controller believes in"
        );
        assert!(
            table.sessions.is_empty(),
            "and it opens no row, which the arrival window would then wait on forever"
        );
        let crashed = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == events::SESSION_CRASHED)
            .expect("the failure is an event");
        assert_eq!(crashed["payload"]["phase"], effect::PHASE_START);
        let cause = crashed["payload"]["cause"].as_str().unwrap_or_default();
        assert!(
            cause.contains("exited 1"),
            "the cause carries the child's own status: {cause}"
        );
        let output = crashed["payload"]["output"].as_str().unwrap_or_default();
        assert!(
            std::fs::read_to_string(output)
                .expect("the cause names the file the output went to")
                .contains("the stub spoke on stderr"),
            "and the file holds what the child printed"
        );

        // The control: the same call against a stub that exits 0.
        rig.write_stub(0);
        let mut table = Table::default();
        let outcome = effect::spawn_woken(
            &rig.agent(),
            &a_policy(),
            &a_target(&worktree, None),
            &mut log,
            &mut table,
            1_000,
        );
        assert_eq!(outcome, effect::Outcome::Spawned);
        assert_eq!(table.sessions.len(), 1);
        assert_eq!(rig.events_of(events::SESSION_SPAWNED), 1);
    }

    /// claude-code C5 — the rest threshold is a fraction of the agent's context
    /// window, and the window is the agent's to change. It is therefore POLICY
    /// and not a constant this code owns: a window that moves makes the number
    /// wrong with no tool reporting anything.
    #[test]
    fn the_context_threshold_is_a_fraction_of_a_moving_window() {
        let from_file = policy::parse("[controller]\nrest_threshold_tokens = 400000\n")
            .expect("the policy parses");
        assert_eq!(from_file.rest_threshold_tokens, 400_000);

        // The default is a figure the file can move, not one the table reads
        // for itself.
        let silent = policy::parse("").expect("an empty policy parses");
        assert_eq!(
            silent.rest_threshold_tokens,
            policy::DEFAULT_REST_THRESHOLD_TOKENS
        );

        // And the decision keys on the value it is HANDED: one seat, one
        // reading, two thresholds, two verdicts.
        let mut seat = SeatInput {
            seat_dir: "s1",
            state: RosterState::Present,
            unknown_cause: None,
            transient: false,
            pending_rest: false,
            pending_deliberate_end: false,
            context_tokens: Some(500_000),
            rest_threshold_tokens: from_file.rest_threshold_tokens,
            session_id: Some("a-session"),
            already_nudged: false,
            dispatch_age_ms: None,
            sighted: false,
            adopted_and_listed: false,
            arrival_window_ms: 45_000,
            halted: false,
            blind: 0,
            pidless_row: false,
            daemon_pid_changed: false,
            daemon_uptime_ms: None,
            seen_live: false,
            since_pidless_ms: None,
        };
        assert_eq!(decide(&seat), Verdict::SuggestRest);
        seat.rest_threshold_tokens = silent.rest_threshold_tokens;
        assert_eq!(
            decide(&seat),
            Verdict::LeaveAlone,
            "the same reading is under the other threshold"
        );
    }

    /// claude-code D1 — the child PATH is constructed, never inherited. A
    /// service environment carries neither a package manager's prefix nor the
    /// user's local bin, and a daemon started under it hands every later session
    /// a PATH that collapses mid-run.
    ///
    /// Read from what the CHILD received, and against a process `PATH` that has
    /// been set to something the constructed one does not contain — so a child
    /// that inherited would be caught rather than accidentally agreeing.
    #[test]
    fn the_child_path_is_constructed() {
        let rig = Rig::new("child-path");
        let home = rig.home();
        let built = platform::child_path(&home);

        // The entries the service environment lacks are in it, keyed on the home
        // it was handed rather than on this box's own.
        let entries: Vec<PathBuf> = std::env::split_paths(&built).collect();
        assert!(
            entries.contains(&home.join(".local").join("bin")),
            "the user's local bin, under the home passed in: {built}"
        );
        assert!(
            entries.contains(&PathBuf::from("/usr/bin"))
                && entries.contains(&PathBuf::from("/bin")),
            "and the base of any POSIX system: {built}"
        );
        let elsewhere = platform::child_path(Path::new("/elsewhere"));
        let somewhere_else = platform::child_path(Path::new("/somewhere-else"));
        assert!(elsewhere.contains("/elsewhere/.local/bin"), "{elsewhere}");
        assert!(
            somewhere_else.contains("/somewhere-else/.local/bin"),
            "{somewhere_else}"
        );
        assert_ne!(
            elsewhere, somewhere_else,
            "a different home moves the entry that is keyed on it"
        );

        // What the child actually got. The stub is spawned through the adapter,
        // which is the one place the PATH is set, and the value it recorded is
        // the constructed one to the byte.
        let spec = StartSpec {
            seat_dir: "s1".to_string(),
            worktree: rig.worktree().display().to_string(),
            name: "orla".to_string(),
            model: "claude-opus-5".to_string(),
            posture: "auto".to_string(),
            first_turn: "/wake s1".to_string(),
            plugin_dir: None,
            config_dir: None,
        };
        rig.agent().start(&spec, Duration::from_secs(30));
        assert_eq!(rig.recorded_path(), built);

        // The control: this process's own PATH is not what the child got. A
        // suite whose PATH happened to equal the constructed value would pass the
        // line above with an adapter that inherited.
        let mine = std::env::var("PATH").unwrap_or_default();
        assert_ne!(
            rig.recorded_path(),
            mine,
            "the child's PATH is not this process's: {mine}"
        );
    }

    /// THE SYSTEM DIRECTORIES COME FIRST ON THE CONSTRUCTED PATH, ahead of the
    /// package manager's prefix and the user's local bin.
    ///
    /// THE ORDER IS PINNED WHOLE, not sampled. A pair of "x is before y"
    /// readings passes under orders nobody intended; the equality below fails on
    /// any edit that moves a directory, adds one or drops one, which is the
    /// point — a prefix put back in front of the system directories has to fail
    /// here rather than go quiet and be found again in a landing's suite log.
    ///
    /// THEN BOTH DIRECTIONS OF WHAT THE ORDER MEANS, over the resolver that
    /// reads it. An order-only reading says the list changed, never that it
    /// changed correctly: the prefix must keep every name the system does not
    /// ship, and it is nearly all of them.
    #[test]
    #[cfg(target_os = "macos")]
    fn the_child_path_puts_the_system_directories_before_the_prefix() {
        let home = Path::new("/a-home");
        let entries: Vec<PathBuf> = std::env::split_paths(&platform::child_path(home)).collect();
        assert_eq!(
            entries,
            vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/sbin"),
                PathBuf::from("/sbin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/a-home/.local/bin"),
            ],
            "the constructed order, whole"
        );

        // BOTH DIRECTIONS, one command, on directories this arm owns and in the
        // shape the pin above proves the real path has: a system pair ahead of a
        // prefix pair. `shared` is in both and must resolve to the system copy;
        // `prefix-only` is in one and must still resolve to the prefix's.
        let rig = Rig::new("child-path-order");
        let system = rig.root.join("system-dir");
        let prefix = rig.root.join("prefix-dir");
        let shared = plant(&system, "tt-shared");
        let shadowed = plant(&prefix, "tt-shared");
        let only = plant(&prefix, "tt-prefix-only");
        let shaped = std::env::join_paths([&system, &prefix])
            .expect("the shaped path joins")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            platform::resolve_on_path(&shaped, "tt-shared"),
            Some(shared),
            "a name both halves hold resolves to the system's copy, not {}",
            shadowed.display()
        );
        assert_eq!(
            platform::resolve_on_path(&shaped, "tt-prefix-only"),
            Some(only),
            "and a name only the prefix holds still resolves there"
        );

        // THE SAME PAIR ON THE BOX'S OWN NAMES, where the box has them: python3
        // is the name the field failure was about and the platform ships one
        // too, and `bd` is the item tracker, which only the prefix ships. A box
        // missing either copy cannot be read for that half, so the arm says
        // which copy it did not find rather than passing in silence.
        let built = platform::child_path(&platform::home_dir());
        let system_python = PathBuf::from("/usr/bin/python3");
        let prefix_python = PathBuf::from("/opt/homebrew/bin/python3");
        if system_python.exists() && prefix_python.exists() {
            assert_eq!(
                platform::resolve_on_path(&built, "python3"),
                Some(system_python),
                "the interpreter a child of this controller runs is the platform's"
            );
        } else {
            println!(
                "python3 not read on this box: /usr/bin {} , /opt/homebrew/bin {}",
                system_python.exists(),
                prefix_python.exists()
            );
        }
        let prefix_bd = PathBuf::from("/opt/homebrew/bin/bd");
        if prefix_bd.exists() && !PathBuf::from("/usr/bin/bd").exists() {
            assert_eq!(
                platform::resolve_on_path(&built, "bd"),
                Some(prefix_bd),
                "and the item tracker, which only the prefix ships, is still found"
            );
        } else {
            println!("bd not read on this box: /opt/homebrew/bin/bd absent or shadowed");
        }
    }

    /// claude-code D2 — collecting a child's output through a pipe waits for EOF
    /// on the pipe rather than for the child, so anything still holding the
    /// write end keeps the caller blocked. A file has no EOF to wait for.
    ///
    /// The measurement is both halves: the file exists and holds the child's
    /// own words, AND the call returns while a child that is still running holds
    /// what would have been the pipe.
    #[test]
    fn start_output_goes_to_a_file() {
        let rig = Rig::new("start-output");
        let spec = StartSpec {
            seat_dir: "s1".to_string(),
            worktree: rig.worktree().display().to_string(),
            name: "orla".to_string(),
            model: "claude-opus-5".to_string(),
            posture: "auto".to_string(),
            first_turn: "/wake s1".to_string(),
            plugin_dir: None,
            config_dir: None,
        };
        let outcome = rig.agent().start(&spec, Duration::from_secs(30));
        let StartOutcome::Started { log } = outcome else {
            panic!("the start did not fail: {outcome:?}");
        };
        assert!(
            log.starts_with(&rig.machine().display().to_string()),
            "the file is under the machine directory: {log}"
        );
        let body = std::fs::read_to_string(&log).expect("the start's output file is there");
        assert!(body.contains("the stub spoke on stdout"), "{body}");
        assert!(
            body.contains("the stub spoke on stderr"),
            "both streams go to the one file: {body}"
        );

        // The half a pipe would fail: a child that is STILL RUNNING and holding
        // the output. The call returns at its watch window rather than at the
        // child's EOF, which is the whole of the finding.
        rig.write_slow_stub();
        let started = std::time::Instant::now();
        let outcome = rig.agent().start(&spec, Duration::from_millis(300));
        let elapsed = started.elapsed();
        assert!(
            matches!(outcome, StartOutcome::Started { .. }),
            "a child still running when the window closes is OK: {outcome:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the call returned at its window and not at the child's 30-second end: {elapsed:?}"
        );
    }

    /// claude-code D3 — a session's permission mode is not honoured by every
    /// model, and the downgrade is reported by nothing an instrument reads. So
    /// the fleet checks that the model CAN honour the posture rather than
    /// assuming the call was enough, and membership is by prefix because live
    /// ids carry suffixes naming the same model.
    #[test]
    fn the_permission_posture_is_model_gated() {
        let fleet = policy::parse("").expect("an empty policy parses");
        assert_eq!(fleet.posture_for(false), policy::POSTURE_AUTO);

        // By PREFIX: a dated suffix and a windowed one are the same model.
        assert!(fleet.model_can_honour("claude-opus-5"));
        assert!(fleet.model_can_honour("claude-opus-5-20260901"));
        assert!(fleet.model_can_honour("claude-sonnet-5-1m"));
        assert!(
            !fleet.model_can_honour("claude-haiku-4-5-20251001"),
            "a model outside the measured set is not in it by being close"
        );

        // The gate is on the REQUESTED posture, so a transient row asking for
        // less is not gated at all — which is what keeps a spawned builder on a
        // cheaper model startable.
        assert!(fleet.posture_is_ungranted(false, "claude-haiku-4-5-20251001"));
        assert!(
            !fleet.posture_is_ungranted(true, "claude-haiku-4-5-20251001"),
            "the transient posture asks for less than the model's own default"
        );
        assert!(!fleet.posture_is_ungranted(false, "claude-opus-5"));

        // And the list is policy: a fleet that names its own set moves the gate.
        let named = policy::parse("[controller]\nauto_capable_models = [\"claude-haiku-4-5\"]\n")
            .expect("the policy parses");
        assert!(named.model_can_honour("claude-haiku-4-5-20251001"));
        assert!(
            !named.model_can_honour("claude-opus-5"),
            "the named list REPLACES the default rather than adding to it"
        );
    }

    /// claude-code A10 — a newer client REPLACES the daemon and re-hosts the
    /// sessions under it, leaving every row pid-less for a window that was 21–61
    /// seconds on one replacement and under 7 on another.
    ///
    /// So the window is not a constant to code against, and the rule is to HOLD
    /// a pid-less row while the daemon's pid has moved or its uptime is under
    /// the arrival window. Measured here as the property the table has to have:
    /// the row a spawn would otherwise be issued over is held, and the SAME row
    /// with the daemon quiet is dispatched against — so the arm reads the daemon
    /// and not the row.
    #[test]
    fn a_newer_client_replaces_the_daemon_and_rehosts() {
        let mut aged_out = a_seat(RosterState::Absent);
        // The outage's own shape: a row older than the recency bound, which
        // reads Absent, standing in the worktree while the daemon re-hosts it.
        aged_out.pidless_row = true;

        // The control FIRST: with no daemon reading at all this is the spawn
        // that put two live rows in one worktree on 2026-09-04.
        assert_eq!(decide(&aged_out), Verdict::SpawnWoken);

        aged_out.daemon_uptime_ms = Some(1_000);
        assert_eq!(
            decide(&aged_out),
            Verdict::LeaveAlone,
            "a row in transit is not one to spawn over"
        );
        assert!(decide::replacement_hold(&aged_out)
            .expect("the hold names itself")
            .starts_with(decide::REPLACEMENT_HELD));

        // The pid half covers the poll loop that was stalled or slept across the
        // replacement and arrives after the uptime has elapsed.
        let mut stalled = aged_out.clone();
        stalled.daemon_uptime_ms = Some(60 * 60 * 1000);
        assert_eq!(decide(&stalled), Verdict::SpawnWoken);
        stalled.daemon_pid_changed = true;
        assert_eq!(decide(&stalled), Verdict::LeaveAlone);

        // And the reading itself: the pid and the uptime come from the daemon's
        // own account, summed over its tokens, with an unknown unit refusing the
        // whole uptime rather than inventing a young one.
        let read = claude_code::parse_daemon_status("pid: 4242\nuptime: 1m 30s\n")
            .expect("the status parses");
        assert_eq!(read.pid, 4242);
        assert_eq!(read.uptime_ms, Some(90_000));
        assert_eq!(
            claude_code::parse_daemon_status("pid: 4242\nuptime: 3 fortnights\n")
                .expect("the pid still parses")
                .uptime_ms,
            None,
            "an age nobody can read is not a fresh one"
        );
        assert!(claude_code::parse_daemon_status("running\n").is_none());
    }

    /// claude-code A7 — an attach EXITS ZERO whether it revived the row or
    /// silently did nothing, so its own return is not a witness that the session
    /// came back.
    ///
    /// Two halves. The effect's: a revive against a stub that exits 0 leaves the
    /// row UNSIGHTED, waiting on the roster. The counter's: a poll that still
    /// reads the row pid-less after that revive counts it as a blind dispatch —
    /// which is what stops a silently failing revive being retried forever.
    #[test]
    fn attach_exit_is_not_a_witness() {
        let rig = Rig::new("lesson-a7");
        let worktree = rig.worktree().display().to_string();
        let mut table = Table::default();
        let mut log = rig.log();

        let outcome = effect::revive(
            &rig.agent(),
            &a_target(&worktree, Some("ab12")),
            &mut log,
            &mut table,
            1_000,
        );
        assert_eq!(outcome, effect::Outcome::Revived);
        assert_eq!(rig.argv(), vec!["attach".to_string(), "ab12".to_string()]);
        assert_eq!(
            table.sessions[0].session_id, None,
            "the exit is a dispatch and not an arrival: the row waits on a sighting"
        );
        assert_eq!(table.sessions[0].first_seen_at, None);

        // The counter's half: the revive is a dispatch, so a roster that still
        // reads pid-less afterwards counts it.
        assert_eq!(
            decide::blind_after(0, RosterState::Stopped, Verdict::Revive),
            1
        );
        assert_eq!(
            decide::blind_after(0, RosterState::Present, Verdict::Revive),
            0,
            "and a sighting is what does not count it"
        );
    }

    /// claude-code A9 — only a FLAGLESS FULL-ID resume continues a session; a
    /// flagged one forks it, and the fork is a session the controller then holds
    /// a row pointing away from.
    ///
    /// So a revive reaches its row through the attach the address takes, and the
    /// argv it sends carries the address and NOTHING else. Read from what the
    /// child received and never from a log line.
    #[test]
    fn resume_continues_only_a_flagless_full_id() {
        let rig = Rig::new("lesson-a9");
        let worktree = rig.worktree().display().to_string();
        let mut table = Table::default();
        let mut log = rig.log();
        effect::revive(
            &rig.agent(),
            &a_target(&worktree, Some("ab12")),
            &mut log,
            &mut table,
            1_000,
        );

        let argv = rig.argv();
        assert_eq!(argv, vec!["attach".to_string(), "ab12".to_string()]);
        assert!(
            !argv.iter().any(|word| word.starts_with('-')),
            "no flag rides along, because a flagged resume forks: {argv:?}"
        );
        assert!(
            !argv.iter().any(|word| word == "a-session"),
            "and the ADDRESS is what it takes, never the identity: {argv:?}"
        );

        // A row with no address cannot be revived and nothing is issued for it.
        let rig = Rig::new("lesson-a9-no-address");
        let worktree = rig.worktree().display().to_string();
        let mut table = Table::default();
        let mut log = rig.log();
        assert_eq!(
            effect::revive(
                &rig.agent(),
                &a_target(&worktree, None),
                &mut log,
                &mut table,
                1_000
            ),
            effect::Outcome::None
        );
        assert!(
            !rig.argv_path().exists(),
            "a row with no address issues nothing"
        );
    }

    /// claude-code B9 — a resume of a session that is ALREADY RUNNING starts a
    /// copy, and the copy is a row of its own: by short id or by full id, the
    /// original keeps its id and its pid and a second row appears beside it. So
    /// a verb that wants a turn on a LIVE session cannot resume it, and this
    /// controller's one such verb is the nudge.
    ///
    /// Read from the argv the child received: a print-mode turn of its own,
    /// carrying no resume and no address that would name the session. The
    /// address is what makes the forked copy — it is present on the revive,
    /// which addresses a session that is NOT running — so a nudge that carried
    /// one would be the fork this lesson is about.
    #[test]
    fn a_live_session_is_reached_without_a_resume() {
        let rig = Rig::new("lesson-b9");
        let worktree = rig.worktree().display().to_string();
        let mut table = Table::default();
        let mut log = rig.log();
        assert_eq!(
            effect::nudge(
                &rig.agent(),
                &a_policy(),
                &a_target(&worktree, Some("ab12")),
                &mut log,
                &mut table,
            ),
            effect::Outcome::Nudged
        );

        let argv = rig.argv();
        assert_eq!(
            argv.first().map(String::as_str),
            Some("-p"),
            "the turn is the nudge's own: {argv:?}"
        );
        for forking in ["--resume", "-r", "--continue", "-c", "attach"] {
            assert!(
                !argv.iter().any(|word| word == forking),
                "{forking} would start a copy beside the live session: {argv:?}"
            );
        }
        for address in ["a-session", "ab12"] {
            assert!(
                !argv.iter().any(|word| word == address),
                "and no address rides along for one to resume: {argv:?}"
            );
        }
    }

    /// gas-city G7 — a restart ADOPTS the sessions it already owns rather than
    /// re-hosting them, and this fleet adds the event the reference engine
    /// omits: adoption by SESSION ID, no respawn, one line each.
    ///
    /// Four table rows against one roster, so the predicate is measured and not
    /// merely exercised. The LIVE row is claimed and it is the only one: a
    /// session carrying a pid is one the daemon is running now, which is the
    /// whole of the sighting. The pid-less rows are left to the discriminator
    /// whether they read `done` or `stopped` — a claim taken on a row nobody
    /// saw running is a claim on a session that may be over — and so is the row
    /// the roster does not carry at all.
    ///
    /// The live row reads `state: done`, which is what a live IDLE session
    /// carries (lessons claude-code A3): a claim consulting the state word
    /// would take none of the four, and one consulting nothing would take
    /// three.
    #[test]
    fn a_restart_adopts_and_says_so() {
        let rig = Rig::new("lesson-g7");
        let mut table = Table::default();
        table.push(a_row_for("s1", "/wt/s1", 100, Some("a-session")));
        table.push(a_row_for("s2", "/wt/s2", 200, Some("a-hibernated-session")));
        table.push(a_row_for("s3", "/wt/s3", 300, Some("a-stopped-session")));
        table.push(a_row_for("s4", "/wt/s4", 400, Some("a-gone-session")));
        let mut log = rig.log();

        let roster: Vec<AgentRow> = serde_json::from_str(
            r#"[{"sessionId":"a-session","id":"ab12","cwd":"/wt/s1","pid":4242,
                 "state":"done","status":"idle"},
                {"sessionId":"a-hibernated-session","id":"cd34","cwd":"/wt/s2","state":"done"},
                {"sessionId":"a-stopped-session","id":"ef56","cwd":"/wt/s3","state":"stopped"}]"#,
        )
        .expect("the roster parses");

        let claimed = effect::adopt(&roster, &mut table, &mut log, 5_000);
        assert_eq!(claimed, vec!["a-session".to_string()]);
        assert_eq!(rig.events_of(events::SESSION_ADOPTED), 1);
        assert_eq!(
            table.sessions[0].first_seen_at,
            Some(5_000),
            "the claimed row is sighted from the roster this poll read"
        );
        assert_eq!(table.sessions[0].short_id.as_deref(), Some("ab12"));
        assert_eq!(
            table.sessions[1].first_seen_at, None,
            "a pid-less row is left to the discriminator: nobody saw this session running"
        );
        assert_eq!(
            table.sessions[2].first_seen_at, None,
            "and so is one whose row names an end"
        );
        assert_eq!(
            table.sessions[3].first_seen_at, None,
            "and so is one the roster does not carry at all"
        );
        assert!(
            !rig.argv_path().exists(),
            "and adoption issues nothing: no start, no attach"
        );
    }

    /// gas-city G14 — a hold that a restart silently clears is a hold that
    /// protects nothing, and a hold recorded only in a trace is one nobody can
    /// answer.
    ///
    /// So the latch survives the table being lost — it is folded back out of the
    /// stream, where a `session.halted` with no clear after it is a STANDING
    /// hold — and it is announced once, by the layer that latched it.
    #[test]
    fn a_hold_persists_and_is_announced() {
        let rig = Rig::new("lesson-g14");
        let stream = rig.machine().join("events.jsonl");
        let mut log = rig.log();

        effect::halted("s1", decide::BLIND_LIMIT, &mut log);
        assert_eq!(
            rig.events_of(events::SESSION_HALTED),
            1,
            "one event for one transition"
        );

        // The table is not consulted: the hold comes back out of the stream.
        let rebuilt = sessions::rebuild(&stream);
        assert!(rebuilt.seat_state("s1").halted);
        assert_eq!(rebuilt.seat_state("s1").blind, decide::BLIND_LIMIT);
        assert_eq!(
            rebuilt.daemon_pid, None,
            "and the daemon pid starts at none, which reads as no replacement"
        );

        // The control: a clear after it, and the same fold reads no hold. It is
        // the ORDER that decides — a clear before the halt leaves the hold
        // standing.
        let mut log = rig.log();
        log.append(events::SEAT_CLEAR_HALT, "s1", serde_json::json!({}))
            .expect("the request lands");
        let cleared = sessions::rebuild(&stream);
        assert!(!cleared.seat_state("s1").halted);
        assert_eq!(cleared.seat_state("s1").blind, 0);

        let mut log = rig.log();
        effect::halted("s1", decide::BLIND_LIMIT, &mut log);
        assert!(
            sessions::rebuild(&stream).seat_state("s1").halted,
            "a halt after the clear is a hold again"
        );
    }
}

/// Adoption is recorded once per SESSION, not once per call: a second adopt
/// over the table the first one claimed from claims nothing and writes no line.
#[test]
fn a_second_adopt_over_the_same_table_claims_nothing() {
    let rig = Rig::new("adopt-once");
    let mut table = Table::default();
    table.push(a_row_for("s1", "/wt/s1", 100, Some("a-session")));
    let mut log = rig.log();
    let roster: Vec<AgentRow> = serde_json::from_str(
        r#"[{"sessionId":"a-session","id":"ab12","cwd":"/wt/s1","pid":4242}]"#,
    )
    .expect("the roster parses");

    let first = effect::adopt(&roster, &mut table, &mut log, 5_000);
    assert_eq!(first, vec!["a-session".to_string()]);
    assert_eq!(table.sessions[0].adopted.as_deref(), Some("a-session"));

    let second = effect::adopt(&roster, &mut table, &mut log, 6_000);
    assert!(
        second.is_empty(),
        "the session was claimed once already: {second:?}"
    );
    assert_eq!(rig.events_of(events::SESSION_ADOPTED), 1);
}

/// The plugin root rides every start the controller makes: policy names it, the
/// spawn passes it, and the child's argv carries `--plugin-dir <dir>`
/// IMMEDIATELY BEFORE the first turn — which is the position that says the flag
/// took the directory as its value and did not swallow the prompt.
///
/// Read from what the CHILD received, through the same `spawn_woken` the loop
/// takes, because the claim is about every start and not about one struct. The
/// control is the same spawn under a policy that names no root: no such element
/// is in the argv at all, so what is measured is the key and not a flag the
/// adapter always passes.
#[test]
fn a_start_carries_the_plugin_root_the_policy_names() {
    let rig = Rig::new("start-plugin-root");
    let worktree = rig.worktree().display().to_string();
    let named =
        policy::parse("[controller]\nstart_watch_seconds = 30\nplugin_dir = \"/an/overlay\"\n")
            .expect("the policy parses");
    let mut table = Table::default();
    let mut log = rig.log();
    let outcome = effect::spawn_woken(
        &rig.agent(),
        &named,
        &a_target(&worktree, None),
        &mut log,
        &mut table,
        1_000,
    );
    assert_eq!(outcome, effect::Outcome::Spawned);

    let argv = rig.argv();
    assert_eq!(flag_value(&argv, "--plugin-dir"), "/an/overlay");
    let at = argv
        .iter()
        .position(|word| word == "--plugin-dir")
        .expect("the flag is in the argv");
    assert_eq!(
        argv.get(at + 2).map(String::as_str),
        Some("/wake s1"),
        "the element after the root's value is the first turn: {argv:?}"
    );
    assert_eq!(argv.last().map(String::as_str), Some("/wake s1"));

    // The control: the same start under a policy that names none.
    let bare = Rig::new("start-no-plugin-root");
    let elsewhere = bare.worktree().display().to_string();
    let mut table = Table::default();
    let mut log = bare.log();
    let outcome = effect::spawn_woken(
        &bare.agent(),
        &a_policy(),
        &a_target(&elsewhere, None),
        &mut log,
        &mut table,
        1_000,
    );
    assert_eq!(outcome, effect::Outcome::Spawned);
    let argv = bare.argv();
    assert!(
        !argv.iter().any(|word| word == "--plugin-dir"),
        "a fleet that names no plugin root passes no such element: {argv:?}"
    );
    assert_eq!(argv.last().map(String::as_str), Some("/wake s1"));
}

/// An adapter carrying no effect binary REFUSES all five verbs rather than
/// falling back to the one the reads use.
///
/// The fallback is the shape the gate exists to prevent: effects read `off` in
/// the projection exactly when this binary did not resolve, so a verb that ran
/// anyway would act while the published document says nothing is being acted on.
///
/// The control is the same rig with the effect binary handed in, which is every
/// other arm's adapter — so what is measured here is the field and not a stub
/// that cannot run.
#[test]
fn an_adapter_with_no_effect_binary_refuses_every_verb_and_execs_nothing() {
    let rig = Rig::new("no-effect-binary");
    let ungated = rig.agent_with_effect_bin(None);
    let worktree = rig.worktree().display().to_string();
    let spec = StartSpec {
        seat_dir: "s1".to_string(),
        worktree: worktree.clone(),
        name: "orla".to_string(),
        model: "claude-opus-5".to_string(),
        posture: "auto".to_string(),
        first_turn: "/wake s1".to_string(),
        plugin_dir: None,
        config_dir: None,
    };

    match ungated.start(&spec, Duration::from_secs(30)) {
        StartOutcome::Failed { cause, .. } => assert!(
            cause.contains("no agent binary is resolved"),
            "the refusal says why: {cause}"
        ),
        other => panic!("a start with no resolved binary must not run: {other:?}"),
    }
    let stopped = ungated.stop(None, "ab12");
    assert!(stopped.is_err(), "{stopped:?}");
    let revived = ungated.revive(None, "ab12");
    assert!(revived.is_err(), "{revived:?}");
    assert!(matches!(
        ungated.remove(None, "ab12"),
        RemoveAnswer::Refused { .. }
    ));
    assert!(ungated
        .nudge(
            None,
            "s1",
            &worktree,
            "a-model",
            "hello",
            Duration::from_secs(5)
        )
        .is_err());

    // NOTHING RAN. The stub records its argv on every branch, so a file that is
    // not there is the reading: no verb reached a program.
    assert!(
        !rig.argv_path().exists(),
        "a refused verb exec'd something: {:?}",
        std::fs::read_to_string(rig.argv_path())
    );

    // The control: the same rig, the same stub, with the effect binary handed
    // in — every verb runs and the stub records it.
    let gated = rig.agent();
    gated.stop(None, "ab12").expect("the stub exits 0");
    assert_eq!(rig.argv(), vec!["stop".to_string(), "ab12".to_string()]);

    // And the reads never depended on it: `version` answers through `bin` with
    // the effect binary absent, which is what keeps a controller that cannot
    // exec anything still observing and publishing.
    let reading = rig.agent_with_effect_bin(None);
    assert!(
        reading.version().is_some(),
        "observe reads through `bin`, which the effect gate does not touch"
    );
}

/// The removal's three answers, read from the exit AND the stdout (lessons
/// claude-code A8), because two of them share an exit status and only one of
/// those two is benign.
#[test]
fn a_removal_that_printed_a_path_is_told_from_one_that_did_not() {
    let rig = Rig::new("remove-answers");
    assert!(matches!(
        rig.agent().remove(None, "ab12"),
        RemoveAnswer::Removed
    ));

    rig.write_stub(1);
    assert!(matches!(
        rig.agent().remove(None, "ab12"),
        RemoveAnswer::Refused { .. }
    ));

    // The alarm: exit 0 with a worktree path printed, which is the outcome that
    // DELETED a checkout. No discard flag is ever passed, so this must not be
    // reachable — and a reader meets the path in the log rather than the missing
    // directory.
    let body = format!(
        "#!/bin/sh\necho 'Removed session and worktree {}'\n",
        rig.worktree().display()
    );
    write(&rig.stub_path(), &body);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(rig.stub_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    match rig.agent().remove(None, "ab12") {
        RemoveAnswer::RemovedAWorktree { path } => {
            assert_eq!(path, rig.worktree().display().to_string())
        }
        other => panic!("a printed path is the third answer: {other:?}"),
    }
}

/// A stop that does not exit 0 leaves the rest PENDING: nothing is started,
/// nothing is removed, and no `session.rested` is written — which is the alarm,
/// a `seat.resting` with no collection after it.
#[test]
fn a_rest_whose_stop_failed_starts_nothing_and_removes_nothing() {
    let rig = Rig::new("rest-stop-failed");
    rig.write_stub(1);
    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    let mut log = rig.log();
    let answer = effect::rest(
        &rig.agent(),
        &a_policy(),
        &a_target(&worktree, Some("ab12")),
        &mut log,
        &mut table,
        1_000,
    );
    assert!(matches!(answer, effect::Rested::StopFailed(_)));
    assert_eq!(rig.argv(), vec!["stop".to_string(), "ab12".to_string()]);
    assert_eq!(rig.events_of(events::SESSION_RESTED), 0);
    assert_eq!(rig.events_of(events::SESSION_SPAWNED), 0);
    assert!(table.sessions.is_empty());

    // The control: the same call with the stop exiting 0 collects the rest.
    rig.write_stub(0);
    let answer = effect::rest(
        &rig.agent(),
        &a_policy(),
        &a_target(&worktree, Some("ab12")),
        &mut log,
        &mut table,
        1_000,
    );
    assert!(matches!(answer, effect::Rested::Collected));
    assert_eq!(rig.events_of(events::SESSION_RESTED), 1);
    assert_eq!(rig.events_of(events::SESSION_SPAWNED), 1);
}

/// A stop that exits 0 followed by a start that does not is its own answer: the
/// predecessor is down, its row stays, nothing is removed and no rest is written.
#[test]
fn a_rest_whose_start_failed_after_its_stop_landed_removes_nothing() {
    let rig = Rig::new("rest-start-failed");
    write(
        &rig.stub_path(),
        "#!/bin/sh\n[ \"$1\" = stop ] && exit 0\nexit 1\n",
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(rig.stub_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    let mut predecessor = a_row_for("s1", &worktree, 500, Some("a-session"));
    predecessor.short_id = Some("ab12".to_string());
    table.push(predecessor);
    let mut log = rig.log();
    let answer = effect::rest(
        &rig.agent(),
        &a_policy(),
        &a_target(&worktree, Some("ab12")),
        &mut log,
        &mut table,
        1_000,
    );
    assert!(matches!(answer, effect::Rested::StartFailed(_)));
    let crashed: Vec<serde_json::Value> = rig
        .events()
        .into_iter()
        .filter(|e| e["type"] == events::SESSION_CRASHED)
        .collect();
    assert_eq!(crashed.len(), 1, "{crashed:?}");
    assert_eq!(crashed[0]["payload"]["phase"], effect::PHASE_START);
    assert_eq!(rig.events_of(events::SESSION_RESTED), 0);
    assert_eq!(rig.events_of(events::SESSION_SPAWNED), 0);
    assert!(
        table
            .sessions
            .iter()
            .any(|row| row.session_id.as_deref() == Some("a-session")),
        "the predecessor's row still stands: {:?}",
        table.sessions
    );
}

/// One nudge marks its session whatever the turn returned, and the event carries
/// the reading, the threshold and the outcome.
#[test]
fn a_nudge_marks_its_session_and_states_what_it_carried() {
    let rig = Rig::new("nudge");
    let worktree = rig.worktree().display().to_string();
    let policy = a_policy();
    let mut table = Table::default();
    let mut log = rig.log();
    let outcome = effect::nudge(
        &rig.agent(),
        &policy,
        &a_target(&worktree, Some("ab12")),
        &mut log,
        &mut table,
    );
    assert_eq!(outcome, effect::Outcome::Nudged);
    assert!(table.is_nudged("s1", "a-session"));
    assert!(
        !table.is_nudged("s1", "another-session"),
        "the mark is the session's and not the seat's"
    );

    let event = rig
        .events()
        .into_iter()
        .find(|e| e["type"] == events::SESSION_NUDGED)
        .expect("the nudge wrote its event");
    assert_eq!(event["payload"]["session"], "a-session");
    assert_eq!(event["payload"]["context_tokens"], 1_000);
    assert_eq!(event["payload"]["threshold"], policy.rest_threshold_tokens);
    assert_eq!(event["payload"]["outcome"], "sent");

    // The turn itself: print mode, the nudge model, the seat's own worktree.
    let argv = rig.argv();
    assert_eq!(argv.first().map(String::as_str), Some("-p"));
    assert_eq!(flag_value(&argv, "--model"), policy.nudge_model);
    assert_eq!(rig.recorded_cwd(), rig.canonical_worktree());
    // The prompt spans several lines, so it is read from the argv file whole:
    // its last LINE is not its last ELEMENT.
    let prompt = rig.argv_text();
    for needle in [
        "orla",
        "700000",
        "fleet event rest s1",
        "exactly one message",
    ] {
        assert!(
            prompt.contains(needle),
            "the prompt carries the sentence the seat is meant to receive ({needle}): {prompt}"
        );
    }

    // A turn that failed is still one nudge: the budget is per session, and a
    // retry loop against a session that cannot be reached is the noise that
    // budget exists to prevent.
    rig.write_stub(1);
    let mut table = Table::default();
    let outcome = effect::nudge(
        &rig.agent(),
        &policy,
        &a_target(&worktree, Some("ab12")),
        &mut log,
        &mut table,
    );
    assert_eq!(outcome, effect::Outcome::Failed);
    assert!(table.is_nudged("s1", "a-session"));
    let failed = rig
        .events()
        .into_iter()
        .filter(|e| e["type"] == events::SESSION_NUDGED)
        .next_back()
        .expect("the failure is an event too");
    assert!(failed["payload"]["outcome"]
        .as_str()
        .unwrap_or_default()
        .starts_with("failed:"));
}

/// The environment a child carries is BUILT and not inherited: the constructed
/// PATH, four values a shell needs, and the agent's own configuration directory
/// — and nothing else this process happens to export.
#[test]
fn a_child_carries_the_built_environment_and_nothing_this_process_exported() {
    let rig = Rig::new("built-environment");
    let leaked = rig.write_env_recording_stub();

    rig.agent().stop(None, "ab12").expect("the stub exits 0");
    let env = std::fs::read_to_string(&leaked).expect("the child recorded its environment");
    let names: Vec<&str> = env
        .lines()
        .filter_map(|line| line.split('=').next())
        .filter(|name| !name.is_empty())
        .collect();
    for owed in [
        "PATH",
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_SECURESTORAGE_CONFIG_DIR",
    ] {
        assert!(names.contains(&owed), "the child carries {owed}: {names:?}");
    }
    // CARGO_ is this process's own, set by the test runner and named by nothing
    // the adapter passes through. A child that inherited would carry a dozen.
    assert!(
        !names.iter().any(|name| name.starts_with("CARGO")),
        "nothing this process exported reached the child: {names:?}"
    );
}

/// The VALUE of the credential scope a child carries, which the arm above can
/// only say is present: it is the CONFIGURED config-directory setting — empty
/// when nothing configured one — and never the resolved directory the child's
/// own `CLAUDE_CONFIG_DIR` names.
///
/// The two variables are asserted APART, on one child, because the drift a
/// later hand makes is setting the credential knob from `config_dir` beside it,
/// and a child whose two variables agree is the logged-out child this whole
/// seam exists to prevent (lessons claude-code A11). An arm reading only the
/// unconfigured case would pass that drift whenever the resolved directory
/// happened to be blank, so the second half hands a configured value the
/// resolved directory cannot equal.
#[test]
fn a_child_carries_the_configured_credential_scope_and_not_the_resolved_config_directory() {
    let rig = Rig::new("credential-scope");
    let leaked = rig.write_env_recording_stub();
    let resolved = rig.home().join(".claude");

    // Unconfigured: DEFINED AND EMPTY, which is the reading that restores the
    // operator's own credential. An unset variable is the suffixed lookup, so
    // the line's presence and its emptiness are both the claim — and the
    // assertion is on the exact line, because `contains` on the name alone is
    // satisfied by any value at all.
    rig.agent().stop(None, "ab12").expect("the stub exits 0");
    let env = std::fs::read_to_string(&leaked).expect("the child recorded its environment");
    let lines: Vec<&str> = env.lines().collect();
    assert!(
        lines.contains(&"CLAUDE_SECURESTORAGE_CONFIG_DIR="),
        "the unconfigured credential scope is defined and empty, and the child's lines are \
         {lines:?}"
    );

    // Configured: the value the operator gave, beside a config directory that
    // is a different path. One child, two readings.
    rig.agent_with_credential_seam("/opt/cfg")
        .stop(None, "ab12")
        .expect("the stub exits 0");
    let env = std::fs::read_to_string(&leaked).expect("the child recorded its environment");
    let lines: Vec<&str> = env.lines().collect();
    assert!(
        lines.contains(&"CLAUDE_SECURESTORAGE_CONFIG_DIR=/opt/cfg"),
        "the credential scope is the configured value: {lines:?}"
    );
    let config_line = format!("CLAUDE_CONFIG_DIR={}", resolved.display());
    assert!(
        lines.contains(&config_line.as_str()),
        "and the config directory is still this rig's own ({config_line}): {lines:?}"
    );
    // The two apart, stated as the inequality the drift would break: a
    // credential knob taking `config_dir` writes this rig's scratch directory
    // into the line asserted above.
    assert_ne!(
        "/opt/cfg",
        resolved.display().to_string(),
        "the two values are distinguishable in this rig, or the pair above proves nothing"
    );
}

/// Every child carries THIS PROCESS'S OWN EXECUTABLE as `FLEET_BIN`, so a
/// session this fleet spawns runs its plugin hooks through the binary that
/// spawned it. Without it the plugin's shim looks under its root's `target/`,
/// and a root with no build there blocks every Bash command the session makes.
///
/// EVERY CHILD, and not only the start: a session is claimed from the agent's
/// background daemon, and any of the adapter's calls — a listing included —
/// can be the one that starts that daemon (lessons claude-code D1). So the value
/// is read off a start and off a read, one child each.
///
/// Asserted BY VALUE against this process's own path, which is what tells the
/// value from a pass-through: this process's own `FLEET_BIN` is unset under a
/// plain shell and names the seat's binary inside a flight, and neither is this
/// test binary.
#[test]
fn every_child_carries_this_processs_own_executable_as_fleet_bin() {
    let rig = Rig::new("fleet-bin");
    let leaked = rig.write_env_recording_stub();
    let own = std::env::current_exe().expect("this process names its own executable");
    assert!(own.is_absolute(), "{} is absolute", own.display());
    let owed = format!("FLEET_BIN={}", own.display());

    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    let mut log = rig.log();
    let outcome = effect::spawn_woken(
        &rig.agent(),
        &a_policy(),
        &a_target(&worktree, None),
        &mut log,
        &mut table,
        1_000,
    );
    assert_eq!(outcome, effect::Outcome::Spawned);
    let env = std::fs::read_to_string(&leaked).expect("the start recorded its environment");
    let lines: Vec<&str> = env.lines().collect();
    assert!(
        lines.contains(&owed.as_str()),
        "the start carries {owed}: {lines:?}"
    );

    std::fs::remove_file(&leaked).expect("the start's record is cleared");
    let _ = rig.agent().daemon();
    let env = std::fs::read_to_string(&leaked).expect("the read recorded its environment");
    let lines: Vec<&str> = env.lines().collect();
    assert!(
        lines.contains(&owed.as_str()),
        "the daemon read carries {owed} too: {lines:?}"
    );
}

/// The table a rebuild folds out of the stream carries the DISPATCH the live
/// table holds — the property the unit arms in `sessions::tests` approximate,
/// measured here against the live path itself rather than against a fixture.
///
/// Both effects that key a row on a dispatch: the spawn, whose id is the
/// append's own return, and the revive after a sighting, which is the sequence
/// the stream carries NO line for — the sighting moves the table and writes
/// none — so a fold that matched the revived line by session id alone would
/// hold two rows here where the live table holds one.
///
/// The stamps agree to the SECOND and are asserted as that: the live row's is
/// the poll's millisecond clock and the rebuilt one is parsed back from the
/// line's `ts`, which carries seconds. Which of two stamps the revived row took
/// is the unit arm's discrimination, on a fixture whose two lines are 75 minutes
/// apart; here both acts fall in the same second and could not tell them apart.
#[test]
fn a_rebuilt_row_carries_the_dispatch_the_live_table_holds() {
    let rig = Rig::new("rebuilt-dispatch");
    let worktree = rig.worktree().display().to_string();
    let stream = rig.machine().join("events.jsonl");
    let mut table = Table::default();
    let mut log = rig.log();

    let spawn_ms = fleet_controller::clock::now_ms();
    assert_eq!(
        effect::spawn_woken(
            &rig.agent(),
            &a_policy(),
            &a_target(&worktree, None),
            &mut log,
            &mut table,
            spawn_ms,
        ),
        effect::Outcome::Spawned
    );
    let live = table.sessions[0].clone();
    assert!(
        !live.dispatch_id.is_empty(),
        "the live row is keyed on the append's own return, or the pairs below are two empties"
    );

    let rebuilt = sessions::rebuild(&stream);
    assert_eq!(rebuilt.sessions.len(), 1, "{:?}", rebuilt.sessions);
    assert_eq!(
        rebuilt.sessions[0].dispatch_id, live.dispatch_id,
        "the rebuilt row is keyed on the same dispatch the live one is"
    );
    assert!(
        rebuilt.sessions[0]
            .dispatched_at
            .abs_diff(live.dispatched_at)
            < 2_000,
        "and stamped at the same instant to the second: rebuilt {} against live {}",
        rebuilt.sessions[0].dispatched_at,
        live.dispatched_at
    );

    // The revive, on the row a sighting filled — which is how every row the
    // controller opened gets its session id, the roster being the only source.
    assert!(table.sight("s1", &worktree, "a-session", Some("ab12"), spawn_ms + 10));
    let revive_ms = fleet_controller::clock::now_ms();
    assert_eq!(
        effect::revive(
            &rig.agent(),
            &a_target(&worktree, Some("ab12")),
            &mut log,
            &mut table,
            revive_ms,
        ),
        effect::Outcome::Revived
    );
    let live = table.sessions[0].clone();
    assert_ne!(
        live.dispatch_id, rebuilt.sessions[0].dispatch_id,
        "the attach moved the live row onto a dispatch of its own"
    );

    let rebuilt = sessions::rebuild(&stream);
    assert_eq!(
        rebuilt.sessions.len(),
        1,
        "one session is one row through a revive too: {:?}",
        rebuilt.sessions
    );
    assert_eq!(
        rebuilt.sessions[0].dispatch_id, live.dispatch_id,
        "and the row it kept is keyed on the revived line, as the live one is"
    );
    assert!(
        rebuilt.sessions[0]
            .dispatched_at
            .abs_diff(live.dispatched_at)
            < 2_000,
        "rebuilt {} against live {}",
        rebuilt.sessions[0].dispatched_at,
        live.dispatched_at
    );
    assert_eq!(
        rebuilt.sessions[0].name, live.name,
        "the row the revive moved is the one the spawn opened, so what the start \
         knew is still on it"
    );

    // And the WRITER's half of that: the revived line carries the identity
    // fields itself, so the fold's fallback — the arm a trimmed stream reaches,
    // where there is no earlier row to move — opens a row from the line rather
    // than from empty strings. A key the fold reads and no writer puts on the
    // line reads as an empty field, so this is measured on the stream.
    let revived = rig
        .events()
        .into_iter()
        .find(|e| e["type"] == events::SESSION_REVIVED)
        .expect("the revive wrote its line");
    assert_eq!(
        (
            revived["payload"]["name"].as_str(),
            revived["payload"]["model"].as_str(),
            revived["payload"]["posture"].as_str(),
            revived["payload"]["first_turn"].as_str(),
            revived["payload"]["transient"].as_bool(),
        ),
        (
            Some(live.name.as_str()),
            Some(live.model.as_str()),
            Some(live.posture.as_str()),
            Some(live.first_turn.as_str()),
            Some(live.transient),
        ),
        "the revived line carries what a row opened from it needs: {revived}"
    );
}
