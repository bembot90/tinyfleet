//! Fixture tests for the decide-and-effect layers.
//!
//! The `lessons::` module below is the contract named in
//! `fleet/brain/lessons/*.md` § Test inventory: each fact the code in this slice
//! exercises owes a test under the exact name the inventory carries.
//!
//! The effects are driven against the in-process [`StubAgent`], and a START
//! runs nothing of the agent's: `launch` answers the argv and the
//! environment, and the session is started on the rig's [`FakeHost`], whose
//! record of it — the argv and the environment the pane was handed — is what a
//! start arm reads. The stub answers the listing a start's watch reads from
//! `roster.json`, which lists the first few panes a fresh fake host hands out
//! ([`test_support::with_arrivals`]), so every start here is believed unless an
//! arm says otherwise.

use fleet_controller::adapter::{self as agent_seam, Agent, Launch, Permissions, Posture};
use fleet_controller::decide::{self, decide, SeatInput, Verdict};
use fleet_controller::effect::{self, Target};
use fleet_controller::events::{self, EventLog};
use fleet_controller::host::{self, Host};
use fleet_controller::observe::RosterState;
use fleet_controller::platform;
use fleet_controller::policy::{self, Policy};
use fleet_controller::sessions::{self, SessionRow, Table};
use fleet_controller::test_support::{self, Answers, FakeHost, Sent, StubAgent};
use fleet_core::seat::identity::SeatId;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A whole machine in a temp directory, with the listing its stub agent reads
/// and one fake host its starts run on.
struct Rig {
    root: PathBuf,
    host: FakeHost,
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
        let rig = Rig {
            root,
            host: FakeHost::new(),
        };
        // The rows a fresh fake host's first panes are listed as, by pid: the
        // listing every start's watch reads, and no seat's worktree.
        write(&rig.roster_path(), &test_support::with_arrivals("[]"));
        rig
    }

    /// What the stub agent's listing answers with.
    fn roster_path(&self) -> PathBuf {
        self.root.join("roster.json")
    }

    /// The one session a start on this rig's host brings up for [`S1`].
    fn session(&self) -> String {
        host::session_for(&s1())
    }

    /// The argv [`S1`]'s session was started with on this rig's host.
    fn started_argv(&self) -> Vec<String> {
        self.host
            .session(&self.session())
            .expect("the start left its session on the host")
            .argv
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

    /// The stub agent every effect here runs through, answering the listing
    /// [`Rig::roster_path`] holds as it is built.
    fn agent(&self) -> StubAgent {
        StubAgent::answering(Answers {
            listing: Ok(std::fs::read_to_string(self.roster_path()).expect("the roster is there")),
            ..Answers::default()
        })
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

/// The seat every arm here is about: the id its table rows, nudge mark, latches
/// and stream lines are keyed on.
const S1: &str = "01a0d1f1-0aec-765f-9abe-4f5e6a7b8c91";

fn s1() -> SeatId {
    SeatId::parse(S1).expect("the fixture's id parses")
}

fn a_target(worktree: &str) -> Target<'_> {
    Target {
        seat: s1(),
        session_name: "orla".to_string(),
        project: "demo",
        worktree,
        model: StubAgent::MODEL.to_string(),
        posture: Posture::Auto,
        first_turn: "/wake s1".to_string(),
        transient: false,
        config_dir: None,
        item: None,
        permissions: Permissions::default(),
        belt: None,
        run: None,
        session_id: Some("a-session"),
        context_tokens: Some(1_000),
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
        posture: Posture::Auto.into(),
        first_turn: format!("/wake {seat}"),
        transient: false,
        config_dir: None,
        item: None,
        dispatch_id: format!("dispatch-{dispatched_at}"),
        dispatched_at,
        session_id: session.map(str::to_string),
        first_seen_at: None,
        last_seen_at: None,
        adopted: None,
        ended: None,
    }
}

/// The launch a launch arm here asks for, in [`Rig::worktree`] and under the
/// adapter's own configuration directory, which is a named seat's start —
/// carrying the variables fleet sets for the seat, its actor among them.
fn a_spec(worktree: &str) -> Launch {
    Launch {
        seat: s1(),
        worktree: worktree.to_string(),
        name: "orla".to_string(),
        model: StubAgent::MODEL.to_string(),
        posture: Posture::Auto,
        first_turn: "/wake s1".to_string(),
        config_dir: None,
        env: agent_seam::seat_environment(&format!("seat:{S1}")),
        permissions: Permissions::default(),
    }
}

mod lessons {
    use super::*;

    /// lessons claude-code C5 — the rest threshold is a fraction of the agent's
    /// context window, and the window is the agent's to change. It is
    /// therefore POLICY and not a constant this code owns: a window that moves
    /// makes the number wrong with no tool reporting anything.
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
            state: RosterState::Present,
            transient: false,
            pending_rest: false,
            pending_deliberate_end: false,
            context_tokens: Some(500_000),
            rest_threshold_tokens: from_file.rest_threshold_tokens,
            already_nudged: false,
            dispatch_age_ms: None,
            sighted: false,
            arrival_window_ms: 45_000,
            halted: false,
            blind: 0,
        };
        assert_eq!(decide(&seat), Verdict::SuggestRest);
        seat.rest_threshold_tokens = silent.rest_threshold_tokens;
        assert_eq!(
            decide(&seat),
            Verdict::LeaveAlone,
            "the same reading is under the other threshold"
        );
    }

    /// lessons claude-code D1 — the child PATH is constructed, never inherited.
    /// A service environment carries neither a package manager's prefix nor
    /// the user's local bin, and a daemon started under it hands every later
    /// session a PATH that collapses mid-run.
    ///
    /// Read from what the session's PANE was handed, and against this process's
    /// own `PATH` — so a session that inherited would be caught rather than
    /// accidentally agreeing.
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

        // What a session is handed: the variables fleet sets for every seat's
        // session, which the launch carries through and the host sets on the
        // pane EXACTLY, hold the constructed value to the byte — built off the
        // home this process runs under, as the controller builds it.
        let constructed = platform::child_path(&platform::home_dir());
        let launch = rig
            .agent()
            .launch(&a_spec(&rig.worktree().display().to_string()))
            .expect("it launches");
        assert_eq!(
            launch.env.get("PATH"),
            Some(&constructed),
            "the session is handed the constructed PATH"
        );

        // The control: this process's own PATH is not what the session got. A
        // suite whose PATH happened to equal the constructed value would pass the
        // lines above with a start that inherited.
        let mine = std::env::var("PATH").unwrap_or_default();
        assert_ne!(
            launch.env.get("PATH"),
            Some(&mine),
            "the session's PATH is not this process's: {mine}"
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

        // THE SAME READING ON THE BOX'S OWN NAME, where the box has both copies:
        // python3 is the name the field failure was about and the platform
        // ships one too. A box missing either copy cannot be read, so the arm
        // says which copy it did not find rather than passing in silence.
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
    }

    /// gas-city G7 — a restart ADOPTS the sessions it already owns rather than
    /// re-hosting them, and this fleet adds the event the reference engine
    /// omits: adoption by SESSION ID, no respawn, one line each.
    ///
    /// Four table rows against one poll's reading, so the predicate is measured
    /// and not merely exercised. The LIVE session is claimed and it is the
    /// only one: a session the agent named for a pane the host holds alive is
    /// one running now, which is the whole of the sighting (the host's
    /// presence and the agent's `read` decide which ids those are, upstream of
    /// this call). The rows whose sessions nobody named live are left to the
    /// discriminator — a claim taken on a session nobody saw running is a claim
    /// on a session that may be over.
    #[test]
    fn a_restart_adopts_and_says_so() {
        let rig = Rig::new("lesson-g7");
        let mut table = Table::default();
        table.push(a_row_for(S1, "/wt/s1", 100, Some("a-session")));
        table.push(a_row_for("s2", "/wt/s2", 200, Some("a-hibernated-session")));
        table.push(a_row_for("s3", "/wt/s3", 300, Some("a-stopped-session")));
        table.push(a_row_for("s4", "/wt/s4", 400, Some("a-gone-session")));
        let mut log = rig.log();

        // The one session the agent named for a live pane this poll.
        let live = vec!["a-session".to_string()];

        let claimed = effect::adopt(&live, &mut table, &mut log, 5_000);
        assert_eq!(claimed, vec!["a-session".to_string()]);
        assert_eq!(rig.events_of(events::SESSION_ADOPTED), 1);
        assert_eq!(
            table.sessions[0].first_seen_at,
            Some(5_000),
            "the claimed row is sighted from the roster this poll read"
        );
        let adopted = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == events::SESSION_ADOPTED)
            .expect("the claim wrote its line");
        assert!(
            adopted["payload"].get("short_id").is_none(),
            "the listing's address rides nowhere, though the row carried one: {adopted}"
        );
        assert_eq!(
            table.sessions[1].first_seen_at, None,
            "a session named for no live pane is left to the discriminator: nobody saw it \
             running"
        );
        assert_eq!(table.sessions[2].first_seen_at, None, "and so is a second");
        assert_eq!(table.sessions[3].first_seen_at, None, "and so is a third");
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

        effect::halted(&s1(), "orla-4f5e6a7b8c91", decide::BLIND_LIMIT, &mut log);
        assert_eq!(
            rig.events_of(events::SESSION_HALTED),
            1,
            "one event for one transition"
        );

        // The table is not consulted: the hold comes back out of the stream.
        let rebuilt = sessions::rebuild(&stream);
        assert!(rebuilt.seat_state(S1).halted);
        assert_eq!(rebuilt.seat_state(S1).blind, decide::BLIND_LIMIT);

        // The control: a clear after it, and the same fold reads no hold. It is
        // the ORDER that decides — a clear before the halt leaves the hold
        // standing.
        let mut log = rig.log();
        log.append(
            events::SEAT_CLEAR_HALT,
            &events::ActorRef::seat(S1),
            serde_json::json!({}),
        )
        .expect("the request lands");
        let cleared = sessions::rebuild(&stream);
        assert!(!cleared.seat_state(S1).halted);
        assert_eq!(cleared.seat_state(S1).blind, 0);

        let mut log = rig.log();
        effect::halted(&s1(), "orla-4f5e6a7b8c91", decide::BLIND_LIMIT, &mut log);
        assert!(
            sessions::rebuild(&stream).seat_state(S1).halted,
            "a halt after the clear is a hold again"
        );
    }
}

/// The claude-code pack's lessons D8, core's half (the launch and the seed are
/// the pack's) — an interactive start is believed only when the agent's read
/// shows the pane's own process with a status: a row by the pane's pid and a
/// status is a start taken, a pane that died is a failed start carrying its
/// status, and a window that closes on a row with no status is a failed start,
/// its session killed with its capture kept.
#[test]
fn a_start_is_believed_only_off_its_panes_row_with_a_status() {
    let rig = Rig::new("interactive-start");
    let worktree = rig.worktree().display().to_string();

    // Over a fake host and a stub agent, in a one-second window: a row
    // carrying the pane's pid and a status is a start taken.
    let session = rig.session();
    let quick =
        policy::parse("[controller]\nstart_watch_seconds = 1\n").expect("the policy parses");
    let target = a_target(&worktree);
    let listed = StubAgent::answering(Answers {
        listing: Ok(test_support::listing(&[test_support::arrived(
            test_support::FIRST_PANE_PID,
        )])),
        ..Answers::default()
    });
    let host = FakeHost::new();
    let mut table = Table::default();
    let mut log = rig.log();
    assert_eq!(
        effect::spawn_woken(&listed, &host, &quick, &target, &mut log, &mut table, 1_000),
        effect::Outcome::Spawned
    );
    let up = host
        .session(&session)
        .expect("the seat's session is on the host, named by its id");
    assert_eq!(up.cwd, worktree, "in the seat's worktree");
    assert!(!up.argv.iter().any(|a| a == "--bg"), "{:?}", up.argv);
    let spawned = rig
        .events()
        .into_iter()
        .find(|e| e["type"] == events::SESSION_SPAWNED)
        .expect("the start wrote its line");
    assert_eq!(
        spawned["payload"]["output"],
        format!("-L {} -t {session}", host::SOCKET),
        "the output a reader is pointed at is the session itself: {spawned}"
    );

    // A pane that ended with status 1 is a failed start carrying 1, and its
    // session is killed.
    let host = FakeHost::new();
    host.end_next_start(Some(1));
    let mut table = Table::default();
    assert_eq!(
        effect::spawn_woken(&listed, &host, &quick, &target, &mut log, &mut table, 1_000),
        effect::Outcome::Failed
    );
    assert!(
        host.session(&session).is_none(),
        "the dead session is killed"
    );
    assert!(table.sessions.is_empty(), "and no row is opened");
    let crashed = rig
        .events()
        .into_iter()
        .filter(|e| e["type"] == events::SESSION_CRASHED)
        .next_back()
        .expect("the failure is an event");
    assert_eq!(crashed["payload"]["phase"], effect::PHASE_START);
    assert_eq!(crashed["payload"]["status"], 1, "{crashed}");
    let cause = crashed["payload"]["cause"].as_str().unwrap_or_default();
    assert!(cause.contains("exited 1"), "{cause}");

    // A window that closes with no believable row is a failed start, and
    // the session is killed with its capture kept. The row here carries the
    // pane's pid and NO status — listed, and not yet up (B10) — which is the
    // control on the first reading: the pid alone is not believed.
    let pid_only = format!(
        r#"{{"sessionId": "arrived", "cwd": "{}", "pid": {}}}"#,
        test_support::ARRIVED_CWD,
        test_support::FIRST_PANE_PID
    );
    let unready = StubAgent::answering(Answers {
        listing: Ok(test_support::listing(&[pid_only])),
        ..Answers::default()
    });
    let host = FakeHost::new();
    let mut table = Table::default();
    assert_eq!(
        effect::spawn_woken(&unready, &host, &quick, &target, &mut log, &mut table, 1_000),
        effect::Outcome::Failed
    );
    assert!(
        host.session(&session).is_none(),
        "the silent session is killed"
    );
    let crashed = rig
        .events()
        .into_iter()
        .filter(|e| e["type"] == events::SESSION_CRASHED)
        .next_back()
        .expect("the failure is an event");
    let cause = crashed["payload"]["cause"].as_str().unwrap_or_default();
    assert!(cause.contains("no listed row within 1s"), "{cause}");
    let output = crashed["payload"]["output"]
        .as_str()
        .expect("the capture was kept");
    assert!(
        output.starts_with(&rig.machine().join(effect::STARTS_DIR).display().to_string())
            && Path::new(output).is_file(),
        "the capture is under <machine>/starts: {output}"
    );
}

/// The claude-code pack's lessons D3, core's half (the declared models are the
/// pack's) — a session's posture is not honoured by every model, and the
/// downgrade is reported by nothing an instrument reads. So the fleet checks
/// that the model CAN honour the posture rather than assuming the call was
/// enough, and membership is by prefix because live ids carry suffixes naming
/// the same model.
#[test]
fn auto_is_held_to_the_declared_models_by_prefix() {
    // The gate is the agent's own declaration (E9) where the policy names
    // none: the stub's `auto` held to its own model's family.
    let agent = test_support::capabilities();
    let fleet = policy::parse("").expect("an empty policy parses");
    assert_eq!(fleet.posture_for(false), Posture::Auto);

    // By PREFIX: a dated suffix and a windowed one are the same model.
    for model in [StubAgent::MODEL, "stub-model-5-20260901", "stub-model-6-1m"] {
        assert!(
            !fleet.posture_is_ungranted(false, model, &agent),
            "{model} honours auto"
        );
    }
    assert!(
        fleet.posture_is_ungranted(false, "stub-mode-5", &agent),
        "a model outside the measured set is not in it by being close"
    );

    // The gate is on the REQUESTED posture, so a transient row asking for
    // less is not gated at all — which is what keeps a spawned builder on a
    // cheaper model startable.
    assert!(
        !fleet.posture_is_ungranted(true, "other-model-1", &agent),
        "the transient posture asks for less than the model's own default"
    );

    // And the list is policy: a fleet that names its own set moves the gate.
    let named = policy::parse("[controller]\nauto_capable_models = [\"other-model\"]\n")
        .expect("the policy parses");
    assert!(!named.posture_is_ungranted(false, "other-model-1", &agent));
    assert!(
        named.posture_is_ungranted(false, StubAgent::MODEL, &agent),
        "the named list REPLACES the declared one rather than adding to it"
    );
}

/// The claude-code pack's lessons A9, core's half (the resume's argv is the
/// pack's) — a resume by the FULL id keeps the session, and a revive believes
/// only the pane's row carrying that same id.
///
/// What a revive does with the resume: the dead pane cleared, a new session
/// whose command is the argv the agent answered, believed on the pane's row
/// carrying the SAME id. And the fork a resume can still be: a row under any
/// other id is Failed, killed, and moves no row.
#[test]
fn a_revive_resumes_the_full_id_and_kills_a_fork() {
    // ---- THE REVIVE: a seat whose pane died, and the table's row for it.
    let rig = Rig::new("lesson-a9-kept");
    let worktree = rig.worktree().display().to_string();
    let dead = a_live_pane(&rig);
    rig.host.end(&rig.session(), Some(0));
    let resumed_pane = test_support::FIRST_PANE_PID + 1;
    write(
        &rig.roster_path(),
        &resumed_listing(resumed_pane, "a-session"),
    );
    let mut table = Table::default();
    table.push(a_row_for(S1, &worktree, 500, Some("a-session")));
    let mut log = rig.log();
    let outcome = effect::revive(
        &rig.agent(),
        &rig.host,
        &a_policy(),
        &a_target(&worktree),
        &mut log,
        &mut table,
        1_000,
    );
    assert_eq!(outcome, effect::Outcome::Revived);
    assert_eq!(
        rig.started_argv(),
        [
            StubAgent::PROGRAM,
            "--resume",
            "a-session",
            "--model",
            StubAgent::MODEL,
            "--posture",
            "auto",
        ],
        "the seat's session is now the resume the agent answered, as the pane's own process"
    );
    let pane = rig.host.session(&rig.session()).expect("the resume stands");
    assert_eq!(pane.pid, resumed_pane, "a NEW session, and not pane {dead}");
    let revived = rig
        .events()
        .into_iter()
        .find(|e| e["type"] == events::SESSION_REVIVED)
        .expect("the revive wrote its line");
    assert_eq!(revived["payload"]["session"], "a-session");
    assert!(
        revived["payload"].get("address").is_none(),
        "no address rides the line: {revived}"
    );
    assert_eq!(
        (
            table.sessions[0].dispatch_id.as_str(),
            table.sessions[0].session_id.as_deref()
        ),
        (revived["id"].as_str().unwrap_or_default(), None),
        "the row is re-opened as this dispatch, and waits on a sighting"
    );

    // ---- THE FORK: the same revive, with the resume's pane listed under
    // another id.
    let rig = Rig::new("lesson-a9-fork");
    let worktree = rig.worktree().display().to_string();
    a_live_pane(&rig);
    rig.host.end(&rig.session(), Some(0));
    write(
        &rig.roster_path(),
        &resumed_listing(test_support::FIRST_PANE_PID + 1, "a-fork"),
    );
    let mut table = Table::default();
    table.push(a_row_for(S1, &worktree, 500, Some("a-session")));
    let before = table.sessions.clone();
    let mut log = rig.log();
    let outcome = effect::revive(
        &rig.agent(),
        &rig.host,
        &a_policy(),
        &a_target(&worktree),
        &mut log,
        &mut table,
        1_000,
    );
    assert_eq!(outcome, effect::Outcome::Failed);
    assert!(
        rig.host.session(&rig.session()).is_none(),
        "the fork is killed, and nothing of the revive is left on the host"
    );
    let crashed: Vec<serde_json::Value> = rig
        .events()
        .into_iter()
        .filter(|e| e["type"] == events::SESSION_CRASHED)
        .collect();
    assert_eq!(crashed.len(), 1, "{crashed:?}");
    assert_eq!(crashed[0]["payload"]["phase"], effect::PHASE_REVIVE);
    assert_eq!(crashed[0]["payload"]["session"], "a-session");
    let cause = crashed[0]["payload"]["cause"].as_str().unwrap_or_default();
    assert!(
        cause.contains("a-fork") && cause.contains("fork"),
        "the cause names the fork: {cause}"
    );
    assert_eq!(rig.events_of(events::SESSION_REVIVED), 0);
    assert_eq!(table.sessions, before, "and no row moved");

    // ---- A seat whose table names no session has nothing to resume, and
    // nothing is started for it.
    let rig = Rig::new("lesson-a9-no-session");
    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    let mut log = rig.log();
    let target = Target {
        session_id: None,
        ..a_target(&worktree)
    };
    assert_eq!(
        effect::revive(
            &rig.agent(),
            &rig.host,
            &a_policy(),
            &target,
            &mut log,
            &mut table,
            1_000
        ),
        effect::Outcome::None
    );
    assert!(
        rig.host.calls_of(FakeHost::NEW_SESSION).is_empty(),
        "{:?}",
        rig.host.verbs()
    );
}

/// Adoption is recorded once per SESSION, not once per call: a second adopt
/// over the table the first one claimed from claims nothing and writes no line.
#[test]
fn a_second_adopt_over_the_same_table_claims_nothing() {
    let rig = Rig::new("adopt-once");
    let mut table = Table::default();
    table.push(a_row_for(S1, "/wt/s1", 100, Some("a-session")));
    let mut log = rig.log();
    let live = vec!["a-session".to_string()];

    let first = effect::adopt(&live, &mut table, &mut log, 5_000);
    assert_eq!(first, vec!["a-session".to_string()]);
    assert_eq!(table.sessions[0].adopted.as_deref(), Some("a-session"));

    let second = effect::adopt(&live, &mut table, &mut log, 6_000);
    assert!(
        second.is_empty(),
        "the session was claimed once already: {second:?}"
    );
    assert_eq!(rig.events_of(events::SESSION_ADOPTED), 1);
}

/// The listing a revive's watch reads: one row, the resume's pane's own
/// process, carrying `session` and a status.
fn resumed_listing(pid: u32, session: &str) -> String {
    format!(
        r#"[{{"sessionId": "{session}", "cwd": "/nowhere/resumed", "kind": "interactive",
             "pid": {pid}, "status": "idle"}}]"#
    )
}

/// A stop whose interrupt leaves the pane alive is KILLED once the grace has
/// run out, and it is `Ok` only on the host's word that the session is gone.
///
/// The default fake is the agent measured: through the claude-code pack, on
/// 2.1.280, one `C-c` ended a busy session's turn and left an idle one asking
/// for a second press, and neither pane died (fleet-rge6.4, 2026-09-26). So the wait is the whole
/// grace, and the kill after it is what ends the session. The control is a
/// program that dies on the interrupt: killed at once, which says the grace is
/// a bound and not a sleep.
#[test]
fn a_stop_whose_interrupt_leaves_the_pane_alive_is_killed_after_the_grace() {
    let rig = Rig::new("stop-grace");
    a_live_pane(&rig);
    let started = std::time::Instant::now();
    effect::stop_session(&rig.host, &rig.session()).expect("the kill lands and the host says so");
    let spent = started.elapsed();
    assert!(
        spent >= effect::STOP_GRACE,
        "the pane stayed alive, so the whole grace was given: {spent:?}"
    );
    let verbs = rig.host.verbs();
    let pressed = rig.host.calls_of(FakeHost::KEYS);
    assert_eq!(
        pressed.len(),
        1,
        "one interrupt, into the seat's session: {verbs:?}"
    );
    assert_eq!(pressed[0].about, rig.session());
    let killed = verbs
        .iter()
        .position(|verb| *verb == FakeHost::KILL)
        .expect("the session was killed");
    assert!(
        verbs.iter().position(|verb| *verb == FakeHost::KEYS) < Some(killed),
        "the interrupt first, the kill after: {verbs:?}"
    );
    assert_eq!(
        &verbs[killed + 1..],
        [FakeHost::LIST],
        "and the host read after the kill is the witness: {verbs:?}"
    );
    assert!(rig.host.session(&rig.session()).is_none());

    // The control: a program that dies on the interrupt is killed at once.
    let rig = Rig::new("stop-grace-exits");
    rig.host.exit_on_interrupt();
    a_live_pane(&rig);
    let started = std::time::Instant::now();
    effect::stop_session(&rig.host, &rig.session()).expect("the stop lands");
    assert!(
        started.elapsed() < effect::STOP_GRACE,
        "a pane that died on the interrupt is not waited on: {:?}",
        started.elapsed()
    );
    assert_eq!(
        rig.host.calls_of(FakeHost::KILL).len(),
        1,
        "a dead pane is killed too: it is no session, and it holds the seat's name"
    );
    assert!(rig.host.session(&rig.session()).is_none());

    // A kill the host refuses is a stop that did not land, and says why.
    let rig = Rig::new("stop-refused");
    rig.host.exit_on_interrupt();
    a_live_pane(&rig);
    rig.host
        .fail(FakeHost::KILL, Some("the arm kept the session"));
    let cause = effect::stop_session(&rig.host, &rig.session()).expect_err("a kill that failed");
    assert!(cause.contains("the arm kept the session"), "{cause}");

    // A host nobody can read after the kill is no witness either.
    rig.host.fail(FakeHost::KILL, None);
    rig.host
        .fail(FakeHost::LIST, Some("the arm blinded the host"));
    let cause = effect::stop_session(&rig.host, &rig.session()).expect_err("an unread host");
    assert!(
        cause.contains("could not be read") && cause.contains("the arm blinded the host"),
        "{cause}"
    );

    // And a session already gone is stopped: the ask is that it not be there.
    let rig = Rig::new("stop-gone");
    effect::stop_session(&rig.host, &rig.session()).expect("nothing to stop is a landed stop");
}

/// A stop that did not land leaves the rest PENDING: nothing is started and no
/// `session.rested` is written — which is the alarm, a `seat.resting` with no
/// collection after it.
///
/// The control collects the rest, and its line names the predecessor and the
/// successor's dispatch and NO removal of any kind: the predecessor was the
/// host's session, and the stop took it.
#[test]
fn a_rest_whose_stop_failed_starts_nothing() {
    let rig = Rig::new("rest-stop-failed");
    rig.host.exit_on_interrupt();
    a_live_pane(&rig);
    rig.host
        .fail(FakeHost::KILL, Some("the arm kept the session"));
    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    let mut log = rig.log();
    let answer = effect::rest(
        &rig.agent(),
        &rig.host,
        &a_policy(),
        &a_target(&worktree),
        &mut log,
        &mut table,
        1_000,
    );
    let effect::Rested::StopFailed(cause) = answer else {
        panic!("a kill that failed is a stop that failed");
    };
    assert!(cause.contains("the arm kept the session"), "{cause}");
    assert_eq!(rig.events_of(events::SESSION_RESTED), 0);
    assert_eq!(rig.events_of(events::SESSION_SPAWNED), 0);
    assert_eq!(
        rig.host.calls_of(FakeHost::NEW_SESSION).len(),
        1,
        "the pane's own start and no successor: {:?}",
        rig.host.verbs()
    );
    assert!(table.sessions.is_empty());

    // The control: the same call with the kill landing collects the rest.
    rig.host.fail(FakeHost::KILL, None);
    let answer = effect::rest(
        &rig.agent(),
        &rig.host,
        &a_policy(),
        &a_target(&worktree),
        &mut log,
        &mut table,
        1_000,
    );
    assert!(matches!(answer, effect::Rested::Collected));
    assert_eq!(rig.events_of(events::SESSION_RESTED), 1);
    assert_eq!(rig.events_of(events::SESSION_SPAWNED), 1);
    let rested = rig
        .events()
        .into_iter()
        .find(|e| e["type"] == events::SESSION_RESTED)
        .expect("the collection wrote its line");
    assert_eq!(rested["payload"]["predecessor"], "a-session");
    for gone in ["removed", "predecessor_address"] {
        assert!(
            rested["payload"].get(gone).is_none(),
            "a rest writes no {gone}: {rested}"
        );
    }
    assert_eq!(
        rig.host.session(&rig.session()).map(|pane| pane.pid),
        Some(test_support::FIRST_PANE_PID + 1),
        "the successor holds the seat's name on the host"
    );
}

/// A stop that landed followed by a start that did not is its own answer: the
/// predecessor is gone from the host, its row stays, and no rest is written.
#[test]
fn a_rest_whose_start_failed_after_its_stop_landed_keeps_the_predecessors_row() {
    let rig = Rig::new("rest-start-failed");
    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    table.push(a_row_for(S1, &worktree, 500, Some("a-session")));
    let mut log = rig.log();
    rig.host.exit_on_interrupt();
    a_live_pane(&rig);
    // The start fails at the host, before any session is made.
    rig.host
        .fail(FakeHost::NEW_SESSION, Some("the arm refused this session"));
    let answer = effect::rest(
        &rig.agent(),
        &rig.host,
        &a_policy(),
        &a_target(&worktree),
        &mut log,
        &mut table,
        1_000,
    );
    assert!(matches!(answer, effect::Rested::StartFailed(_)));
    assert!(
        rig.host.session(&rig.session()).is_none(),
        "the stop landed: the predecessor is gone from the host"
    );
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

// ---- typing a turn ------------------------------------------------------------
//
// A turn for a live seat is PASTED into its own session and believed only when
// the listing turns busy (fleet-rge6.5). The arms below drive `type_turn` on a
// stub agent and a fake host: the host holds the seat's pane and records every
// paste and submit, and the agent's listing is answered read by read, so an arm
// says what the row read before the send and what it read after.

/// The seat's live pane on the rig's host, as the start would have left it,
/// and the pid the host gave it.
fn a_live_pane(rig: &Rig) -> u32 {
    rig.host
        .new_session(
            &rig.session(),
            &rig.worktree(),
            &["/nowhere/agent".to_string()],
            &[],
        )
        .expect("the fake host starts the pane");
    rig.host
        .session(&rig.session())
        .expect("the pane stands")
        .pid
}

/// The listing's row for the pane with `pid`, reading `status`, as the JSON
/// the stub reads.
fn a_row_reading(pid: u32, status: &str) -> serde_json::Value {
    serde_json::json!({
        "sessionId": "a-session",
        "cwd": "/anywhere",
        "pid": pid,
        "status": status,
    })
}

/// The same row stopped in front of a person on `cause`.
fn a_row_waiting_for(pid: u32, cause: &str) -> serde_json::Value {
    let mut row = a_row_reading(pid, "waiting");
    row["waitingFor"] = serde_json::Value::from(cause);
    row
}

fn listing(rows: Vec<serde_json::Value>) -> Result<String, String> {
    Ok(serde_json::Value::from(rows).to_string())
}

/// A stub agent whose listing reads `first` once and then `then` on every read
/// after it: the row before the send, and the row the poll meets.
fn an_agent_reading(first: serde_json::Value, then: serde_json::Value) -> StubAgent {
    let agent = StubAgent::answering(Answers {
        listing: listing(vec![then]),
        ..Answers::default()
    });
    agent.list_next([listing(vec![first])]);
    agent
}

fn turn_for(seat: &SeatId) -> effect::TurnTarget<'_> {
    effect::TurnTarget {
        seat,
        config_dir: None,
    }
}

/// The bound every arm types under: one poll's worth past the first reads.
const BOUND: Duration = Duration::from_secs(1);

/// An idle row that turns busy on the same pid after the send is DELIVERED:
/// one paste of the text and one submit, into the seat's own session — and no
/// launch and no resume, because a turn for a live seat starts nothing.
#[test]
fn an_idle_seat_that_turns_busy_is_delivered_with_one_paste_and_one_submit() {
    let rig = Rig::new("typed-delivered");
    let pid = a_live_pane(&rig);
    let agent = an_agent_reading(a_row_reading(pid, "idle"), a_row_reading(pid, "busy"));
    let seat = s1();

    let typed = effect::type_turn(
        &agent,
        &rig.host,
        &turn_for(&seat),
        "two lines\nof one turn",
        BOUND,
    );
    assert_eq!(typed, effect::Typed::Delivered);
    assert_eq!(
        rig.host.sends(&rig.session()),
        vec![
            Sent::Paste("two lines\nof one turn".to_string()),
            Sent::Submit
        ],
        "one paste, whole, and one submit"
    );
    for verb in [StubAgent::LAUNCH, StubAgent::RESUME] {
        assert!(
            agent.calls_of(verb).is_empty(),
            "a turn for a live seat makes no {verb} call: {:?}",
            agent.verbs()
        );
    }
}

/// An idle row that STAYS idle after the send is a failed turn, whatever the
/// host answered: the send returning is a dispatch and never a witness
/// (fleet-fmver defect 2). A typing that believed the host's `Ok` reads this
/// arm delivered.
#[test]
fn an_idle_seat_that_stays_idle_is_failed_and_never_believed_off_the_send() {
    let rig = Rig::new("typed-failed");
    let pid = a_live_pane(&rig);
    let agent = an_agent_reading(a_row_reading(pid, "idle"), a_row_reading(pid, "idle"));
    let seat = s1();

    let typed = effect::type_turn(&agent, &rig.host, &turn_for(&seat), "a turn", BOUND);
    match typed {
        effect::Typed::Failed(cause) => assert!(
            cause.contains("typed and not taken: still idle after 1s"),
            "{cause}"
        ),
        other => panic!("a row that never turned busy is a failed turn: {other:?}"),
    }
    assert_eq!(
        rig.host.sends(&rig.session()),
        vec![Sent::Paste("a turn".to_string()), Sent::Submit],
        "the text was typed; it is the taking that failed"
    );
}

/// A row already busy before the send is QUEUED: the text is typed and waits
/// for the turn in hand, and it is never called delivered (E5) — a busy
/// reading after the send cannot tell the queued turn from the one ahead of it.
#[test]
fn a_busy_seat_is_queued_and_never_called_delivered() {
    let rig = Rig::new("typed-queued");
    let pid = a_live_pane(&rig);
    let agent = an_agent_reading(a_row_reading(pid, "busy"), a_row_reading(pid, "busy"));
    let seat = s1();

    let typed = effect::type_turn(&agent, &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert_eq!(typed, effect::Typed::Queued);
    assert_eq!(
        rig.host.sends(&rig.session()),
        vec![Sent::Paste("a turn".to_string()), Sent::Submit]
    );
}

/// A row stopped in front of a person is refused BEFORE ANY BYTE: keys sent at
/// a dialog answer it (the claude-code pack's lessons B8, B10). By the field,
/// carrying the cause the row names, and by the status word alone where no
/// field is there.
#[test]
fn a_blocked_seat_is_refused_before_any_byte() {
    let rig = Rig::new("typed-blocked");
    let pid = a_live_pane(&rig);
    let seat = s1();

    let asked = a_row_waiting_for(pid, "permission prompt");
    let agent = an_agent_reading(asked.clone(), asked);
    let typed = effect::type_turn(&agent, &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert_eq!(
        typed,
        effect::Typed::Blocked("permission prompt".to_string())
    );

    let waiting = a_row_reading(pid, "waiting");
    let agent = an_agent_reading(waiting.clone(), waiting);
    let typed = effect::type_turn(&agent, &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert!(
        matches!(&typed, effect::Typed::Blocked(cause) if cause.contains("waiting")),
        "{typed:?}"
    );

    assert!(
        rig.host.sends(&rig.session()).is_empty(),
        "nothing was typed at the dialog: {:?}",
        rig.host.sends(&rig.session())
    );
    assert!(rig.host.calls_of(FakeHost::SEND).is_empty());
}

/// No session to type into is ABSENT, and nothing is sent: no pane under the
/// seat's session, a pane that has died, and a live pane no listed row carries.
/// A listing that could not be read is a failure that says so, never absence.
#[test]
fn a_seat_with_no_live_pane_or_no_listed_row_is_absent_and_nothing_is_sent() {
    let rig = Rig::new("typed-absent");
    let seat = s1();
    let idle = |pid| an_agent_reading(a_row_reading(pid, "idle"), a_row_reading(pid, "idle"));

    // No pane at all.
    let typed = effect::type_turn(&idle(4242), &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert_eq!(typed, effect::Typed::Absent);

    // A live pane whose pid no row carries: a row in the seat's worktree
    // proves nothing (B5), and the row here is some other process's.
    let pid = a_live_pane(&rig);
    let typed = effect::type_turn(&idle(pid + 1), &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert_eq!(typed, effect::Typed::Absent);

    // A dead pane, whose pid a row still carries.
    rig.host.end(&rig.session(), Some(0));
    let typed = effect::type_turn(&idle(pid), &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert_eq!(typed, effect::Typed::Absent);

    // A listing that could not be read.
    let unreadable = StubAgent::answering(Answers {
        listing: Err("the listing timed out".to_string()),
        ..Answers::default()
    });
    let rig = Rig::new("typed-unreadable");
    a_live_pane(&rig);
    let typed = effect::type_turn(&unreadable, &rig.host, &turn_for(&seat), "a turn", BOUND);
    assert!(
        matches!(&typed, effect::Typed::Failed(cause) if cause.contains("the listing timed out")),
        "{typed:?}"
    );
    assert!(
        rig.host.sends(&rig.session()).is_empty(),
        "nothing is typed into a session nobody could read"
    );
}

/// One nudge marks its session whatever became of the turn, and the event
/// carries the reading, the threshold and the outcome — `sent` only where the
/// listing witnessed the turn taken, and each other outcome in its own words.
#[test]
fn a_nudge_marks_its_session_and_states_what_it_carried() {
    let rig = Rig::new("nudge");
    let policy = policy::parse("[controller]\nnudge_timeout_seconds = 1\n").expect("it parses");
    let worktree = rig.worktree().display().to_string();
    let pid = a_live_pane(&rig);
    let mut log = rig.log();
    let nudged = |agent: &StubAgent, table: &mut Table, log: &mut EventLog| {
        let outcome = effect::nudge(agent, &rig.host, &policy, &a_target(&worktree), log, table);
        let event = rig
            .events()
            .into_iter()
            .filter(|e| e["type"] == events::SESSION_NUDGED)
            .next_back()
            .expect("the nudge wrote its event");
        (outcome, event)
    };

    let agent = an_agent_reading(a_row_reading(pid, "idle"), a_row_reading(pid, "busy"));
    let mut table = Table::default();
    let (outcome, event) = nudged(&agent, &mut table, &mut log);
    assert_eq!(outcome, effect::Outcome::Nudged);
    assert!(table.is_nudged(S1, "a-session"));
    assert!(
        !table.is_nudged(S1, "another-session"),
        "the mark is the session's and not the seat's"
    );
    assert_eq!(event["payload"]["session"], "a-session");
    assert_eq!(event["payload"]["context_tokens"], 1_000);
    assert_eq!(event["payload"]["threshold"], policy.rest_threshold_tokens);
    assert_eq!(event["payload"]["outcome"], "sent");

    // What was typed is the sentence itself, verbatim — no prompt around it
    // and no model to carry it — as one paste and one submit.
    let text = effect::nudge_text("orla", 1_000, policy.rest_threshold_tokens, "orla");
    assert_eq!(
        rig.host.sends(&rig.session()),
        vec![Sent::Paste(text.clone()), Sent::Submit]
    );
    for needle in ["orla", "700000", "fleet event rest orla"] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    for verb in [StubAgent::LAUNCH, StubAgent::RESUME] {
        assert!(agent.calls_of(verb).is_empty(), "{:?}", agent.verbs());
    }

    // A turn that was typed and never taken is still one nudge: the budget is
    // per session, and a retry loop against a session that will not take one
    // is the noise that budget exists to prevent.
    let agent = an_agent_reading(a_row_reading(pid, "idle"), a_row_reading(pid, "idle"));
    let mut table = Table::default();
    let (outcome, event) = nudged(&agent, &mut table, &mut log);
    assert_eq!(outcome, effect::Outcome::Failed);
    assert!(table.is_nudged(S1, "a-session"));
    assert!(
        event["payload"]["outcome"]
            .as_str()
            .unwrap_or_default()
            .starts_with("failed: typed and not taken"),
        "{event}"
    );

    // A seat mid-turn is queued, and marked.
    let agent = an_agent_reading(a_row_reading(pid, "busy"), a_row_reading(pid, "busy"));
    let mut table = Table::default();
    let (outcome, event) = nudged(&agent, &mut table, &mut log);
    assert_eq!(outcome, effect::Outcome::Nudged);
    assert!(table.is_nudged(S1, "a-session"));
    assert_eq!(event["payload"]["outcome"], "queued: the seat was mid-turn");

    // A seat at a dialog is refused before any byte, and marked.
    let before = rig.host.sends(&rig.session()).len();
    let asked = a_row_waiting_for(pid, "permission prompt");
    let agent = an_agent_reading(asked.clone(), asked);
    let mut table = Table::default();
    let (outcome, event) = nudged(&agent, &mut table, &mut log);
    assert_eq!(outcome, effect::Outcome::Failed);
    assert!(table.is_nudged(S1, "a-session"));
    assert_eq!(
        event["payload"]["outcome"],
        "refused: blocked on permission prompt"
    );
    assert_eq!(rig.host.sends(&rig.session()).len(), before, "no byte");
}

/// A session's pane carries THIS PROCESS'S OWN EXECUTABLE as `FLEET_BIN`, so
/// a session this fleet spawns runs its hooks through the binary that spawned
/// it — and the environment it carries is BUILT and not inherited: nothing
/// this process happens to export reaches it (lessons claude-code D1).
///
/// Asserted BY VALUE against this process's own path, which is what tells the
/// value from a pass-through: this process's own `FLEET_BIN` is unset under a
/// plain shell and names the seat's binary inside a flight, and neither is this
/// test binary.
#[test]
fn a_session_carries_this_processs_own_executable_and_nothing_it_exported() {
    let rig = Rig::new("fleet-bin");
    let own = std::env::current_exe().expect("this process names its own executable");
    assert!(own.is_absolute(), "{} is absolute", own.display());
    let owed = format!("FLEET_BIN={}", own.display());

    let worktree = rig.worktree().display().to_string();
    let mut table = Table::default();
    let mut log = rig.log();
    let outcome = effect::spawn_woken(
        &rig.agent(),
        &rig.host,
        &a_policy(),
        &a_target(&worktree),
        &mut log,
        &mut table,
        1_000,
    );
    assert_eq!(outcome, effect::Outcome::Spawned);
    let env = rig
        .host
        .session(&rig.session())
        .expect("the start left its session on the host")
        .env;
    assert!(
        env.contains(&("FLEET_BIN".to_string(), own.display().to_string())),
        "the start carries {owed}: {env:?}"
    );
    for owed in ["PATH", "FLEET_ACTOR"] {
        assert!(
            env.iter().any(|(name, _)| name == owed),
            "the start carries {owed}: {env:?}"
        );
    }
    // CARGO_ is this process's own, set by the test runner and named by
    // nothing fleet passes through. A session that inherited would carry a
    // dozen.
    assert!(
        !env.iter().any(|(name, _)| name.starts_with("CARGO")),
        "nothing this process exported reached the session: {env:?}"
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
            &rig.host,
            &a_policy(),
            &a_target(&worktree),
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
    // controller opened gets its session id, the roster being the only source —
    // of a session whose pane has since died. The resume's pane is the host's
    // second, and the listing shows it carrying the session it resumed.
    assert!(table.sight(S1, &worktree, "a-session", spawn_ms + 10));
    rig.host.end(&rig.session(), Some(0));
    write(
        &rig.roster_path(),
        &resumed_listing(test_support::FIRST_PANE_PID + 1, "a-session"),
    );
    let revive_ms = fleet_controller::clock::now_ms();
    assert_eq!(
        effect::revive(
            &rig.agent(),
            &rig.host,
            &a_policy(),
            &a_target(&worktree),
            &mut log,
            &mut table,
            revive_ms,
        ),
        effect::Outcome::Revived
    );
    let live = table.sessions[0].clone();
    assert_ne!(
        live.dispatch_id, rebuilt.sessions[0].dispatch_id,
        "the revive moved the live row onto a dispatch of its own"
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
            Some(live.posture.word()),
            Some(live.first_turn.as_str()),
            Some(live.transient),
        ),
        "the revived line carries what a row opened from it needs: {revived}"
    );
}

/// A session table written while it still kept each session's short address
/// READS: the key is ignored, the row it sat on is read whole, and the table
/// written back carries no such key. No table is migrated and none is refused
/// over a field this build no longer keeps (ruling 17).
#[test]
fn a_table_written_with_the_retired_short_address_still_reads() {
    let rig = Rig::new("retired-address");
    let path = rig.machine().join("sessions.json");
    write(
        &path,
        &serde_json::json!({
            "schema": sessions::SCHEMA,
            "sessions": [{
                "seat": S1, "project": "demo", "worktree": "/wt/s1", "name": "orla",
                "model": "a-model", "posture": "auto", "first_turn": "/wake orla",
                "transient": false, "dispatch_id": "a-dispatch", "dispatched_at": 1000,
                "session_id": "a-session", "short_id": "ab12", "first_seen_at": 1000,
            }],
        })
        .to_string(),
    );

    let (table, cause) = sessions::read(&path);
    assert_eq!(cause, None, "the table is no defect");
    let table = table.expect("the table reads");
    assert_eq!(
        table
            .newest_for(S1)
            .and_then(|row| row.session_id.as_deref()),
        Some("a-session"),
        "and the row the retired key sat on is read whole"
    );

    sessions::write(&path, &table).expect("the table is written back");
    let written = std::fs::read_to_string(&path).expect("the table is on disk");
    assert!(
        !written.contains("ab12"),
        "and the address does not ride back out: {written}"
    );
}
