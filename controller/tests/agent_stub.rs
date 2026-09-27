//! `fleet-agent-stub`, the fake agent kept in a file answering the agent
//! contract one process per call, driven the way fleet drives any agent
//! adapter: through `adapter::exec` and `AgentExec`, never by reading its file.
//!
//! What it measures: that every answer the stub prints reads through the
//! contract's own reader and passes the contract's schema (`fleet agent
//! schema`), that it exits by the contract's table, that a rig reads its calls
//! back in the in-process stub's own shape, and that the launch it answers
//! runs as a fake agent whose moves the next read answers — so the suites that
//! move onto it read what the in-process stub would have answered them.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fleet_controller::adapter::{
    Agent, AgentError, AgentExec, BlockedOn, Launch, Permissions, Posture, Refusal, RefusalReason,
    Resume, SeatRef,
};
use fleet_controller::test_support::agent_stub::{self, DEAF, SESSION, SLOW, STATE_FILE};
use fleet_controller::test_support::{self, Answers, Declined, StubAgent};
use fleet_core::adapter::exec::{self, Exited};
use fleet_core::agent::types::{self, Activity, CONTRACT_VERSION};
use fleet_core::seat::identity::SeatId;
use serde_json::{json, Value};

const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";
const OTHER_SEAT: &str = "0199a3c4-8f01-7a23-b456-c789d0e1f234";

/// A transcript as the one real adapter reads one: a turn that used a thousand
/// tokens of the window.
const A_LIGHT_TRANSCRIPT: &str =
    "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":1000}}}\n";

/// A bound no call here meets unless it means to.
const PATIENT: Duration = Duration::from_secs(60);

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fleet-agent-stub"))
}

/// A root of its own, per arm and per run, emptied on the way out.
struct Root(PathBuf);

impl Root {
    fn new(arm: &str) -> Root {
        let dir =
            std::env::temp_dir().join(format!("fleet-agent-stub-{arm}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the root is made");
        Root(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn agent(&self) -> AgentExec {
        AgentExec::at(&stub(), &self.0).with_timeout(PATIENT)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn seat(id: &str) -> SeatId {
    SeatId::parse(id).expect("a seat id")
}

fn a_launch(root: &Path) -> Launch {
    Launch {
        seat: seat(SEAT),
        worktree: root.display().to_string(),
        name: String::from("orla-e5f60718"),
        model: String::from("a-model"),
        posture: Posture::Auto,
        first_turn: String::from("/wake orla-e5f60718"),
        config_dir: Some(root.join("config").display().to_string()),
        env: [(String::from("FLEET_ACTOR"), format!("seat:{SEAT}"))]
            .into_iter()
            .collect(),
        permissions: Permissions::default(),
    }
}

fn a_resume(root: &Path) -> Resume {
    Resume {
        session_id: String::from("a-session"),
        worktree: root.display().to_string(),
        config_dir: None,
        model: String::from("a-model"),
        posture: Posture::Ask,
    }
}

/// The two seats a read and a context are asked about: one found by its pid,
/// one the listing names nothing for.
fn two_seats(root: &Path) -> Vec<SeatRef> {
    [(SEAT, Some(4242)), (OTHER_SEAT, None)]
        .into_iter()
        .map(|(id, pid)| SeatRef {
            seat: seat(id),
            session_id: None,
            pid,
            config_dir: None,
            worktree: root.display().to_string(),
            screen: None,
        })
        .collect()
}

/// A listing with the first seat's pane at a permission prompt.
fn a_blocked_listing() -> String {
    test_support::listing(&[json!({
        "sessionId": "a-session",
        "cwd": "/anywhere",
        "pid": 4242,
        "status": "waiting",
        "waitingFor": "permission prompt",
    })
    .to_string()])
}

/// A verb's fields as a request, in the envelope every adapter is sent.
fn request(root: &Path, fields: Value) -> Value {
    match fields {
        Value::Object(fields) => types::request(fields, root),
        other => unreachable!("a verb's fields are an object, not {other}"),
    }
}

/// One raw call, read off the exit table the Exec reads.
fn ran(root: &Path, verb: &str, fields: Value) -> Exited {
    exec::run(&stub(), verb, &request(root, fields), PATIENT, None)
        .unwrap_or_else(|unrun| panic!("the stub ran for {verb}: {unrun:?}"))
        .exited
}

/// The body of an answer, held to the contract's schema at `pointer`.
fn passes(stdout: &str, pointer: &str) -> Value {
    let value = exec::first_value(stdout).unwrap_or_else(|| panic!("a JSON value: {stdout}"));
    fleet_core::schema::check(&fleet_core::agent::schema::document(), pointer, &value)
        .unwrap_or_else(|why| panic!("{pointer} refuses {value}: {why}"));
    value
}

/// EVERY VERB ANSWERS THROUGH THE EXEC IN THE CONTRACT'S SHAPE: each of the six
/// exits 0 with an answer the contract's reader decodes and its schema passes,
/// and what the Exec reads back is what the in-process stub answers over the
/// same answers — its launch and its resume excepted, whose program is the
/// stub again ([`SESSION`]).
#[test]
fn every_verb_answers_through_the_exec_and_every_answer_passes_the_contracts_schema() {
    let root = Root::new("verbs");
    let answers = Answers {
        listing: Ok(a_blocked_listing()),
        session_log: Some(A_LIGHT_TRANSCRIPT.to_string()),
        last_write: Some(1_758_800_000_000),
        ..Answers::default()
    };
    agent_stub::script(root.path(), |a| *a = answers.clone());
    let seats = json!({ "seats": two_seats(root.path()) });
    let asked = [
        ("capabilities", json!({})),
        ("version", json!({})),
        (
            "launch",
            serde_json::to_value(a_launch(root.path())).unwrap(),
        ),
        (
            "resume",
            serde_json::to_value(a_resume(root.path())).unwrap(),
        ),
        ("read", seats.clone()),
        ("context", seats),
    ];
    for (verb, fields) in asked {
        let Exited::Answered(stdout) = ran(root.path(), verb, fields) else {
            panic!("{verb} answered on exit 0");
        };
        passes(&stdout, &format!("/verbs/{verb}/response"));
        types::answer::<Value>(&stdout).unwrap_or_else(|why| {
            panic!("{verb}'s answer reads at version {CONTRACT_VERSION}: {why}")
        });
    }

    let in_process = StubAgent::answering(answers);
    let agent = root.agent();
    assert_eq!(agent.capabilities(), in_process.capabilities());
    assert_eq!(agent.version(), in_process.version());
    let asked = two_seats(root.path());
    assert_eq!(agent.read(&asked), in_process.read(&asked));
    assert_eq!(agent.context(&asked), in_process.context(&asked));

    let launched = agent.launch(&a_launch(root.path())).expect("a launch");
    let own = in_process.launch(&a_launch(root.path())).expect("a launch");
    assert_eq!(
        launched.argv[..3],
        [
            stub().display().to_string(),
            SESSION.to_string(),
            SEAT.to_string()
        ]
    );
    assert_eq!(
        launched.argv[3..],
        own.argv[1..],
        "the in-process stub's flags follow"
    );
    assert_eq!(
        launched.env.get(agent_stub::ROOT_VAR).map(String::as_str),
        Some(root.path().display().to_string().as_str()),
        "the session is told whose state to write into"
    );
    let resumed = agent.resume(&a_resume(root.path())).expect("a resume");
    let own = in_process.resume(&a_resume(root.path())).expect("a resume");
    assert_eq!(
        resumed.argv[..2],
        [stub().display().to_string(), SESSION.to_string()]
    );
    assert_eq!(
        resumed.argv[2..],
        own.argv[1..],
        "the full id and the flags follow"
    );
}

/// A READ SCRIPTED AS BLOCKED IS BLOCKED ON A PERMISSION through the Exec, and
/// a seat the listing names nothing for is starting — the one real adapter's
/// rules over the scripted listing.
#[test]
fn a_read_scripted_as_blocked_on_a_permission_answers_it() {
    let root = Root::new("blocked");
    agent_stub::script(root.path(), |a| a.listing = Ok(a_blocked_listing()));
    let read = root.agent().read(&two_seats(root.path())).expect("a read");
    assert_eq!(read[0].activity, Activity::Blocked, "{read:?}");
    assert_eq!(read[0].blocked_on, Some(BlockedOn::Permission), "{read:?}");
    assert_eq!(read[0].session_id.as_deref(), Some("a-session"));
    assert_eq!(read[1].activity, Activity::Starting, "{read:?}");
}

/// EXIT 1 IS THE SCRIPTED REFUSAL AND EXIT 3 THE SCRIPTED COULD-NOT-TELL, each
/// in the contract's own shape, and the Exec reads the first as the agent's
/// refusal with its message and the second as unreadable carrying why.
#[test]
fn a_scripted_refusal_is_exit_1_and_a_scripted_could_not_tell_is_exit_3() {
    let root = Root::new("declined");
    let refusal = Refusal {
        reason: RefusalReason::Unsupported,
        message: String::from("the stub takes no such posture"),
    };
    agent_stub::script(root.path(), |a| {
        a.launch = Err(Declined::Refused(refusal.clone()));
        a.resume = Err(Declined::Untold(String::from("the stub keeps no sessions")));
    });

    let launch = serde_json::to_value(a_launch(root.path())).unwrap();
    let Exited::Refused(stdout) = ran(root.path(), "launch", launch) else {
        panic!("a scripted refusal exits 1");
    };
    passes(&stdout, "/refusal");
    let resume = serde_json::to_value(a_resume(root.path())).unwrap();
    let Exited::CouldNotTell(Some(error)) = ran(root.path(), "resume", resume) else {
        panic!("a scripted could-not-tell exits 3 with its error");
    };
    assert_eq!(error, "the stub keeps no sessions");

    assert_eq!(
        root.agent().launch(&a_launch(root.path())),
        Err(AgentError::Refused(refusal))
    );
    match root.agent().resume(&a_resume(root.path())) {
        Err(AgentError::Unreadable(why)) => {
            assert!(why.contains("the stub keeps no sessions"), "{why}")
        }
        other => panic!("a could-not-tell is unreadable: {other:?}"),
    }
}

/// EXIT 2 IS A REQUEST THE STUB DOES NOT SPEAK: a verb that is none of the six,
/// a request that is no JSON object, one at another schema version, one whose
/// fields do not read as its verb's, and `context` asked of a stub whose
/// capabilities declare none. None of them is logged.
#[test]
fn a_request_the_stub_does_not_speak_is_exit_2_and_never_logged() {
    let root = Root::new("usage");
    agent_stub::script(root.path(), |a| a.capabilities.context = false);

    assert_eq!(ran(root.path(), "screen", json!({})), Exited::Usage);
    assert_eq!(
        ran(root.path(), "launch", json!({"name": 7})),
        Exited::Usage
    );
    assert_eq!(
        ran(
            root.path(),
            "context",
            json!({ "seats": two_seats(root.path()) })
        ),
        Exited::Usage
    );
    for text in ["not json", r#"{"schema_version":2,"root":"/x"}"#] {
        let mut child = Command::new(stub())
            .arg("version")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the stub runs");
        child
            .stdin
            .take()
            .expect("a stdin")
            .write_all(text.as_bytes())
            .expect("the request is written");
        assert_eq!(
            child.wait().expect("the stub exits").code(),
            Some(2),
            "{text}"
        );
    }
    assert_eq!(agent_stub::calls(root.path()), Vec::new());
}

/// THE CALLS READ BACK IN THE IN-PROCESS STUB'S OWN SHAPE: the same six verbs
/// asked of the stub through the Exec and of a `StubAgent` in memory leave the
/// same calls — so an arm written against `verbs()` and `calls_of()` ports by
/// swapping where it reads them — and the launches read back whole.
#[test]
fn the_calls_read_back_as_the_in_process_stub_records_them() {
    let root = Root::new("calls");
    agent_stub::script(root.path(), |_| {});
    let seats = two_seats(root.path());

    let agent = root.agent();
    let in_process = StubAgent::new();
    for each in [&agent as &dyn Agent, &in_process as &dyn Agent] {
        each.capabilities().expect("capabilities");
        each.version().expect("a version");
        each.launch(&a_launch(root.path())).expect("a launch");
        each.resume(&a_resume(root.path())).expect("a resume");
        each.read(&seats).expect("a read");
    }
    // The Exec asks capabilities before every context, so the in-process stub
    // is asked the same pair.
    agent.context(&seats).expect("a context");
    in_process.capabilities().expect("capabilities");
    in_process.context(&seats).expect("a context");

    assert_eq!(agent_stub::calls(root.path()), in_process.calls());
    assert_eq!(
        agent_stub::calls_of(root.path(), StubAgent::READ)[0].about,
        format!("{SEAT},{OTHER_SEAT}")
    );
    assert_eq!(agent_stub::starts(root.path()), in_process.starts());
    assert_eq!(agent_stub::verbs(root.path()), in_process.verbs());
}

/// THE LISTINGS QUEUED AHEAD ARE TAKEN ONE PER READ, across the processes that
/// answer each: the file twin of `StubAgent::list_next`.
#[test]
fn the_listings_queued_ahead_are_taken_one_per_read_and_then_the_standing_one() {
    let root = Root::new("queued");
    agent_stub::script(root.path(), |a| a.listing = Ok(a_blocked_listing()));
    let row = |status: &str| {
        test_support::listing(&[
            json!({"sessionId": "a-session", "pid": 4242, "status": status}).to_string(),
        ])
    };
    agent_stub::list_next(root.path(), [Ok(row("idle")), Ok(row("busy"))]);
    let seats = two_seats(root.path());
    let read = || root.agent().read(&seats).expect("a read")[0].activity;
    assert_eq!(
        [read(), read(), read(), read()],
        [
            Activity::Idle,
            Activity::Busy,
            Activity::Blocked,
            Activity::Blocked
        ]
    );
}

/// A DEAF CALL IS ANSWERED AND WRITES NOTHING BACK: no log line, and the
/// listing queued ahead is still there for the next read.
#[test]
fn a_deaf_call_is_answered_and_writes_nothing_back() {
    let root = Root::new("deaf");
    agent_stub::script(root.path(), |_| {});
    agent_stub::list_next(
        root.path(),
        [Ok(test_support::listing(&[
            json!({"sessionId": "s", "pid": 4242, "status": "busy"}).to_string(),
        ]))],
    );
    let seats = json!({ "seats": two_seats(root.path()) });
    let deaf = one_call_under(root.path(), "read", seats, &[(DEAF, "1")]);
    let answered: Value = types::answer(&deaf).expect("a deaf call answers");
    assert_eq!(answered["seats"][0]["activity"], "busy", "{answered}");
    assert_eq!(
        agent_stub::calls(root.path()),
        Vec::new(),
        "nothing is logged"
    );
    assert_eq!(
        root.agent().read(&two_seats(root.path())).expect("a read")[0].activity,
        Activity::Busy,
        "the queued listing was left for the next read"
    );
    assert_eq!(agent_stub::verbs(root.path()), [StubAgent::READ]);
}

/// A SLOW CALL SLEEPS BEFORE IT ANSWERS: the seconds the knob names, which a
/// bound shorter than them cuts off — the shape the agent call's own bound is
/// measured against.
#[test]
fn a_slow_call_sleeps_the_seconds_it_is_given_before_it_answers() {
    let root = Root::new("slow");
    let begun = Instant::now();
    let answered = one_call_under(root.path(), "version", json!({}), &[(SLOW, "0.5")]);
    assert!(
        begun.elapsed() >= Duration::from_millis(500),
        "{:?}",
        begun.elapsed()
    );
    assert!(answered.contains("0.0.0-stub"), "{answered}");

    let mut cmd = Command::new(stub());
    cmd.arg("version").env(SLOW, "30");
    let cut = fleet_core::process::run_bounded_fed(
        cmd,
        request(root.path(), json!({})).to_string().into_bytes(),
        Duration::from_millis(300),
    );
    assert!(
        cut.is_err(),
        "a bound under the sleep cuts the call off: {cut:?}"
    );
}

/// A VERB SCRIPTED UNTOLD IS EXIT 3 with the error it was given, whatever the
/// answers say, and answers again once cleared — the version call that fails,
/// beside the null version that says no agent is installed.
#[test]
fn a_verb_scripted_untold_is_exit_3_and_answers_again_once_cleared() {
    let root = Root::new("untold");
    agent_stub::untold(
        root.path(),
        StubAgent::VERSION_CALL,
        Some("no answer today"),
    );
    assert_eq!(
        ran(root.path(), "version", json!({})),
        Exited::CouldNotTell(Some(String::from("no answer today")))
    );
    agent_stub::untold(root.path(), StubAgent::VERSION_CALL, None);
    assert_eq!(
        root.agent()
            .version()
            .expect("a version")
            .version
            .as_deref(),
        Some(StubAgent::VERSION)
    );
    assert_eq!(
        agent_stub::verbs(root.path()),
        [StubAgent::VERSION_CALL; 2],
        "a call answered could not tell is still a call"
    );
}

/// A READ THAT FOLLOWS THE HOST reads a listed row busy once the fake pane
/// under its pid has taken a submit, and idle again once the pane's record is
/// cleared — a session taking a typed turn, on a host where no pane runs one.
#[test]
fn a_read_following_the_host_reads_a_row_busy_once_its_pane_took_a_submit() {
    let root = Root::new("follows");
    let host = root.path().join("tmux-stub.json");
    let mut server = test_support::FakeServer::default();
    server
        .start("orla", "/wt", &[String::from("/bin/agent")], &[])
        .expect("the pane starts");
    server.save(&host).expect("the host is written");
    let pid = server.sessions["orla"].pid;
    agent_stub::script(root.path(), |a| {
        a.listing = Ok(test_support::listing(&[json!({
            "sessionId": "a-session", "pid": pid, "status": "idle"
        })
        .to_string()]))
    });
    let asked = [SeatRef {
        pid: Some(pid),
        ..two_seats(root.path()).remove(1)
    }];
    let read = || root.agent().read(&asked).expect("a read")[0].activity;

    assert_eq!(
        read(),
        Activity::Idle,
        "an unfollowed read is the listing's"
    );
    agent_stub::follow_host(root.path(), Some(&host));
    assert_eq!(read(), Activity::Idle, "nothing typed yet");
    server.submit("orla").expect("the pane takes a submit");
    server.save(&host).expect("the host is written");
    assert_eq!(
        read(),
        Activity::Busy,
        "a turn typed and submitted is taken"
    );
    server
        .sessions
        .get_mut("orla")
        .expect("the pane")
        .sent
        .clear();
    server.save(&host).expect("the host is written");
    assert_eq!(read(), Activity::Idle, "the turn ended");
}

/// A ROOT WITH NO STATE IS ANSWERED FROM THE DEFAULTS and nothing is written
/// under it: `fleet agent check` hands the stub a root of its own choosing.
#[test]
fn a_root_with_no_state_is_answered_from_the_defaults_and_left_untouched() {
    let root = Root::new("stateless");
    let agent = root.agent();
    assert_eq!(
        agent.capabilities().expect("capabilities"),
        Answers::default().capabilities
    );
    assert_eq!(
        agent.version().expect("a version").version.as_deref(),
        Some(StubAgent::VERSION)
    );
    agent.read(&two_seats(root.path())).expect("a read");
    assert!(
        !root.path().join(STATE_FILE).exists(),
        "no state is written where there was none"
    );
}

/// THE LAUNCH IT ANSWERS RUNS AS A FAKE AGENT: started with the argv and the
/// environment the launch answered, the session is read idle under its own pid,
/// busy on a line typed into it, idle again after its turn, and gone on
/// `/exit` — the moves a suite on a real host reads, with no model behind them.
/// A resume's session is read under the id its resume named.
#[test]
fn a_launched_session_reads_idle_then_busy_on_a_line_and_leaves_on_exit() {
    let root = Root::new("session");
    agent_stub::script(root.path(), |a| a.listing = Ok(String::from("[]")));
    let agent = root.agent();

    let launched = agent.launch(&a_launch(root.path())).expect("a launch");
    let mut session = Session::start(&launched, root.path());
    session.reads(&agent, Activity::Idle);
    let listed = session.read(&agent);
    assert!(
        listed
            .session_id
            .as_deref()
            .is_some_and(|id| id.starts_with("stub-session-")),
        "{listed:?}"
    );
    session.types("do the thing");
    session.reads(&agent, Activity::Busy);
    session.reads(&agent, Activity::Idle);
    session.leaves();
    assert_eq!(
        session.read(&agent).activity,
        Activity::Starting,
        "a session that left takes its row with it"
    );

    let resumed = agent.resume(&a_resume(root.path())).expect("a resume");
    let mut session = Session::start(&resumed, root.path());
    session.reads(&agent, Activity::Idle);
    assert_eq!(
        session.read(&agent).session_id.as_deref(),
        Some("a-session")
    );
    session.leaves();
}

/// One call run by hand, under extra environment, answering its stdout.
fn one_call_under(root: &Path, verb: &str, fields: Value, env: &[(&str, &str)]) -> String {
    let mut child = Command::new(stub())
        .arg(verb)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the stub runs");
    child
        .stdin
        .take()
        .expect("a stdin")
        .write_all(request(root, fields).to_string().as_bytes())
        .expect("the request is written");
    let out = child.wait_with_output().expect("the stub exits");
    assert_eq!(out.status.code(), Some(0), "{verb} answers");
    String::from_utf8(out.stdout).expect("the answer is text")
}

/// A session's process, started from what a launch or a resume answered.
struct Session {
    child: std::process::Child,
    seat: SeatRef,
}

impl Session {
    fn start(argv: &fleet_controller::adapter::Argv, worktree: &Path) -> Session {
        let child = Command::new(&argv.argv[0])
            .args(&argv.argv[1..])
            .env_clear()
            .envs(&argv.env)
            .current_dir(worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the session starts");
        let seat = SeatRef {
            seat: seat(SEAT),
            session_id: None,
            pid: Some(child.id()),
            config_dir: None,
            worktree: worktree.display().to_string(),
            screen: None,
        };
        Session { child, seat }
    }

    fn read(&self, agent: &AgentExec) -> fleet_controller::adapter::SeatActivity {
        agent
            .read(std::slice::from_ref(&self.seat))
            .expect("a read")
            .remove(0)
    }

    /// Read until the session is `activity`, or panic naming what it read.
    fn reads(&self, agent: &AgentExec, activity: Activity) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let read = self.read(agent);
            if read.activity == activity {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the session never read {activity:?}: {read:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn types(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().expect("a stdin");
        writeln!(stdin, "{line}").expect("the line is typed");
    }

    fn leaves(&mut self) {
        self.types(agent_stub::EXIT);
        let status = self.child.wait().expect("the session exits");
        assert_eq!(status.code(), Some(0), "a session leaves cleanly on /exit");
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
