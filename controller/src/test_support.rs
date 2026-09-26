//! What a suite drives the controller with. Compiled into this crate's own
//! tests, and into the library itself only under the `test-support` feature,
//! which a dependent's DEV-dependency turns on: under resolver 2 that keeps it
//! out of the binary a release build produces.

use crate::adapter::{Agent, AgentRow, Launch, RosterRead, StartSpec};
use crate::clock::Clock;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Time a test owns. [`Clock::sleep`] advances it by exactly the duration asked
/// for and returns, so a wait spent against this clock costs no wall clock and a
/// deadline computed from [`Clock::now`] expires the instant fake time passes it.
///
/// Single-threaded by ruling: the sleeping thread is the one that advances, so
/// there is nothing here to park on or notify.
///
/// The one real reading is the ORIGIN, taken once at construction because stable
/// Rust offers no other way to obtain an `Instant`; every later reading is that
/// origin plus the fake time spent since.
pub struct FakeClock {
    origin: Instant,
    /// The wall-clock stamp the origin was taken at, so the epoch milliseconds
    /// this clock answers with age by exactly the fake time spent. Real at
    /// construction rather than an invented epoch: the rows a poll reads carry
    /// the agent's own stamps, and a clock starting at zero would read every one
    /// of them as written in the future.
    origin_ms: u64,
    spent: Mutex<Duration>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            origin_ms: crate::clock::now_ms(),
            spent: Mutex::new(Duration::ZERO),
        }
    }

    /// Move fake time forward without a wait having asked for it.
    pub fn advance(&self, by: Duration) {
        let mut spent = self.spent.lock().expect("the fake clock's own lock");
        *spent += by;
    }

    /// Fake time spent since construction — what an arm asserts a wait consumed.
    pub fn spent(&self) -> Duration {
        *self.spent.lock().expect("the fake clock's own lock")
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.origin + self.spent()
    }

    fn sleep(&self, d: Duration) {
        self.advance(d)
    }

    fn now_ms(&self) -> u64 {
        self.origin_ms + self.spent().as_millis() as u64
    }
}

/// One call the stub received, in the order it arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// One of [`StubAgent`]'s verb constants, so an arm and the stub name a verb
    /// from the same value.
    pub verb: &'static str,
    /// Which session or seat the call was about — the address, the seat
    /// directory or the session id it named. Empty for a verb that names none.
    pub about: String,
}

/// What the stub answers with, one field per verb.
///
/// Set per arm at construction, and settable again between ticks through
/// [`StubAgent::set`]: a loop's second tick can be told something its first was
/// not, which is how a roster that changes under the controller is driven.
#[derive(Clone, Debug)]
pub struct Answers {
    pub status: RosterRead,
    pub version: Option<String>,
    /// `Ok` is a launch built from the start's own spec ([`StubAgent::launched`]),
    /// and `Err` a launch the agent refuses, with its cause.
    pub launch: Result<(), String>,
    /// The same for a resume ([`StubAgent::resumed`]).
    pub resume: Result<(), String>,
    pub transcript: Option<String>,
    pub ended_at: Option<u64>,
}

impl Default for Answers {
    /// A fleet the agent can see and nothing is running in: an empty roster that
    /// READ, and every effect succeeding.
    fn default() -> Self {
        Self {
            status: RosterRead::Readable(Vec::new()),
            version: Some(StubAgent::VERSION.to_string()),
            launch: Ok(()),
            resume: Ok(()),
            transcript: None,
            ended_at: None,
        }
    }
}

/// An agent with no process behind it: every verb answers from [`Answers`] and
/// records that it was asked.
///
/// The record is what tells a tick that RAN from one that returned early, so it
/// is appended to and never rewritten — the order the calls arrived in is half
/// of what an arm about the loop's sequence asserts.
pub struct StubAgent {
    answers: Mutex<Answers>,
    calls: Mutex<Vec<Call>>,
    starts: Mutex<Vec<StartSpec>>,
    /// Listings answered ahead of [`Answers::status`], one per read, in order:
    /// see [`StubAgent::list_next`].
    listed_next: Mutex<std::collections::VecDeque<RosterRead>>,
}

impl StubAgent {
    pub const START: &'static str = "start";
    pub const RESUME: &'static str = "resume";
    pub const STATUS: &'static str = "status";
    pub const TRANSCRIPT: &'static str = "transcript";
    pub const ENDED_AT: &'static str = "ended_at";
    pub const VERSION_CALL: &'static str = "version";

    /// What [`Answers::default`] reports as the live agent version.
    pub const VERSION: &'static str = "0.0.0-stub";

    /// Where this provider would keep a session's project-local settings. No
    /// observe path reads it; it is here because the trait has the verb.
    pub const LOCAL_SETTINGS: &'static str = ".stub/settings.json";

    pub fn new() -> StubAgent {
        StubAgent::answering(Answers::default())
    }

    pub fn answering(answers: Answers) -> StubAgent {
        StubAgent {
            answers: Mutex::new(answers),
            calls: Mutex::new(Vec::new()),
            starts: Mutex::new(Vec::new()),
            listed_next: Mutex::new(Default::default()),
        }
    }

    /// Answer the next reads of the listing with `reads`, one each and in
    /// order, and [`Answers::status`] once they are spent — how an arm says
    /// what a row read before a turn was typed and what it reads after.
    pub fn list_next(&self, reads: impl IntoIterator<Item = RosterRead>) {
        self.listed_next
            .lock()
            .expect("the stub agent's own lock")
            .extend(reads);
    }

    /// Change what the next call is told.
    pub fn set(&self, change: impl FnOnce(&mut Answers)) {
        change(&mut self.answers.lock().expect("the stub agent's own lock"));
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .expect("the stub agent's own lock")
            .clone()
    }

    /// The verbs alone, in order — what an arm asserting a tick's SHAPE reads.
    pub fn verbs(&self) -> Vec<&'static str> {
        self.calls().into_iter().map(|call| call.verb).collect()
    }

    pub fn calls_of(&self, verb: &str) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.verb == verb)
            .collect()
    }

    /// Every start's whole specification, so an arm reads what the effect asked
    /// for rather than only that it asked.
    pub fn starts(&self) -> Vec<StartSpec> {
        self.starts
            .lock()
            .expect("the stub agent's own lock")
            .clone()
    }

    fn answers(&self) -> Answers {
        self.answers
            .lock()
            .expect("the stub agent's own lock")
            .clone()
    }

    fn record(&self, verb: &'static str, about: impl Into<String>) {
        self.calls
            .lock()
            .expect("the stub agent's own lock")
            .push(Call {
                verb,
                about: about.into(),
            });
    }
}

impl Default for StubAgent {
    fn default() -> Self {
        StubAgent::new()
    }
}

impl StubAgent {
    /// The program a stub launch names first. Nothing runs it: a host that is
    /// a fake starts no process, and one that is real is never handed a stub.
    pub const PROGRAM: &'static str = "/nowhere/stub-agent";

    /// The launch the stub answers for `spec`: the argv in the shape the one
    /// real adapter builds — the name, the model, the posture, the plugin root
    /// where one is named, then the first turn — and an environment carrying
    /// the actor and the configuration directory, so an arm reads what a start
    /// asked for off the host's own record of the session.
    pub fn launched(spec: &StartSpec) -> Launch {
        let mut argv: Vec<String> = [
            StubAgent::PROGRAM,
            "--name",
            &spec.name,
            "--model",
            &spec.model,
            "--permission-mode",
            &spec.posture,
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        if let Some(plugin_dir) = &spec.plugin_dir {
            argv.push("--plugin-dir".to_string());
            argv.push(plugin_dir.clone());
        }
        argv.push(spec.first_turn.clone());
        Launch {
            argv,
            env: StubAgent::session_env(spec),
        }
    }

    /// The resume the stub answers for `session_id` under `spec`, in the shape
    /// the one real adapter builds: the full id, then the model, the posture
    /// and the plugin root where one is named — no name and no first turn —
    /// under the start's environment.
    pub fn resumed(session_id: &str, spec: &StartSpec) -> Launch {
        let mut argv: Vec<String> = [
            StubAgent::PROGRAM,
            "--resume",
            session_id,
            "--model",
            &spec.model,
            "--permission-mode",
            &spec.posture,
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        if let Some(plugin_dir) = &spec.plugin_dir {
            argv.push("--plugin-dir".to_string());
            argv.push(plugin_dir.clone());
        }
        Launch {
            argv,
            env: StubAgent::session_env(spec),
        }
    }

    fn session_env(spec: &StartSpec) -> Vec<(String, String)> {
        let mut env = vec![("FLEET_ACTOR".to_string(), spec.actor.clone())];
        if let Some(dir) = &spec.config_dir {
            env.push(("CLAUDE_CONFIG_DIR".to_string(), dir.clone()));
        }
        env
    }
}

/// An operator's own agent state file, as a spawn's seed copies from it: the
/// three onboarding keys and nothing of the operator's that a seat must not
/// get. A rig whose spawns start under a configuration directory of their own
/// plants it with [`plant_operator_state`], or every such spawn is refused
/// for a key it cannot copy.
pub const OPERATOR_STATE: &str = r#"{
  "hasCompletedOnboarding": true,
  "lastOnboardingVersion": "0.0.0-test",
  "oauthAccount": {"emailAddress": "nobody@example.invalid"}
}"#;

/// [`OPERATOR_STATE`] at `<home>/.claude.json`, which is where the adapter
/// reads it when no configuration directory is configured.
pub fn plant_operator_state(home: &Path) {
    std::fs::create_dir_all(home).expect("the home is made");
    std::fs::write(
        home.join(crate::adapter::claude_code::STATE_FILE),
        OPERATOR_STATE,
    )
    .expect("the operator's state file is planted");
}

/// The working directory an [`arrived`] row stands in: a directory no seat
/// has, so the row is the start's to find by its pid and no seat's to match by
/// its directory.
pub const ARRIVED_CWD: &str = "/nowhere/arrived";

/// The row the listing shows for a session a start brought up on a
/// [`FakeHost`], found by the pane's pid and carrying a status, which is what
/// a start's watch believes (lessons claude-code B10).
///
/// It stands in [`ARRIVED_CWD`] and not in the seat's worktree. A row is a
/// seat's by the pane's pid and never by where it stands (fleet-rge6.3), so
/// the next poll reads it as the seat's live session all the same; the
/// directory only keeps a listed arrival from reading as a session the host
/// does not hold standing in some seat's worktree.
pub fn arrived(pid: u32) -> AgentRow {
    AgentRow {
        session_id: format!("arrived-{pid}"),
        cwd: ARRIVED_CWD.to_string(),
        pid: Some(pid),
        state: None,
        status: Some("idle".to_string()),
        started_at: None,
        waiting_for: None,
    }
}

/// [`arrived`] for the first `n` panes a fresh [`FakeHost`] or a fresh
/// `fleet-tmux-stub` state hands out, in order.
pub fn arrivals(n: u32) -> Vec<AgentRow> {
    (0..n).map(|k| arrived(FIRST_PANE_PID + k)).collect()
}

/// How many arrivals [`with_arrivals`] lists: more panes than any one arm's
/// host hands out.
pub const LISTED_ARRIVALS: u32 = 8;

/// A listing's JSON body — what a stub agent serves for `agents` — with the
/// rows of [`arrivals`] after the arm's own, so every start on a fresh host
/// is believed and no seat is handed a session by its directory.
///
/// Appended AS TEXT, so the arm's own rows keep the bytes it wrote: an arm
/// reading its row back out of the file finds its own spelling. A body that
/// is not a JSON array is served as the arm gave it — an arm serving a listing
/// that does not parse means exactly that.
pub fn with_arrivals(body: &str) -> String {
    let rows: Vec<String> = arrivals(LISTED_ARRIVALS)
        .iter()
        .map(|row| {
            serde_json::json!({
                "sessionId": row.session_id,
                "cwd": row.cwd,
                "pid": row.pid,
                "status": row.status,
            })
            .to_string()
        })
        .collect();
    let trimmed = body.trim_end();
    match trimmed.strip_suffix(']') {
        Some(head) if trimmed.trim_start().starts_with('[') => {
            let separator = if head.trim_end().ends_with('[') {
                ""
            } else {
                ", "
            };
            format!("{head}{separator}{}]", rows.join(", "))
        }
        _ => body.to_string(),
    }
}

impl Agent for StubAgent {
    fn launch(&self, spec: &StartSpec) -> Result<Launch, String> {
        self.record(StubAgent::START, spec.name.clone());
        self.starts
            .lock()
            .expect("the stub agent's own lock")
            .push(spec.clone());
        self.answers().launch.map(|()| StubAgent::launched(spec))
    }

    /// The one real screen rule, read as the adapter reads it: an arm drives
    /// the fallback by putting the agent's own question on a fake pane.
    fn trust_keys(&self, screen: &str) -> Option<Vec<String>> {
        crate::adapter::claude_code::trust_keys(screen)
    }

    fn resume(&self, session_id: &str, spec: &StartSpec) -> Result<Launch, String> {
        self.record(StubAgent::RESUME, session_id);
        self.answers()
            .resume
            .map(|()| StubAgent::resumed(session_id, spec))
    }

    fn local_settings(&self) -> &'static str {
        StubAgent::LOCAL_SETTINGS
    }

    fn status(&self, config_dir: Option<&Path>) -> RosterRead {
        self.record(
            StubAgent::STATUS,
            config_dir
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
        );
        let next = self
            .listed_next
            .lock()
            .expect("the stub agent's own lock")
            .pop_front();
        next.unwrap_or_else(|| self.answers().status)
    }

    fn transcript(
        &self,
        _config_dir: Option<&Path>,
        _worktree: &str,
        session_id: &str,
    ) -> Option<String> {
        self.record(StubAgent::TRANSCRIPT, session_id);
        self.answers().transcript
    }

    fn ended_at(
        &self,
        _config_dir: Option<&Path>,
        _worktree: &str,
        session_id: &str,
    ) -> Option<u64> {
        self.record(StubAgent::ENDED_AT, session_id);
        self.answers().ended_at
    }

    fn version(&self) -> Option<String> {
        self.record(StubAgent::VERSION_CALL, "");
        self.answers().version
    }
}

// ---- the host fake ----------------------------------------------------------
//
// ONE MODEL, TWO FACES (reviewer call 2026-09-25, E7). [`FakeServer`] is a
// host server with no process behind it: sessions, their panes, what was typed
// into each, and the paste buffers. [`FakeHost`] puts it behind the `Host`
// trait for the in-process suites; the `fleet-tmux-stub` binary loads it from
// a JSON file, applies one client call, and writes it back, for the suites
// that drive the built `fleet` and cannot hand it a trait object. So the two
// seams answer the same verbs with the same rules, and a rule changed in one
// is changed in both.

/// One thing typed into a session, in the order it arrived.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sent {
    /// A paste, whole.
    Paste(String),
    /// The separate keystroke that submits what was pasted.
    Submit,
    /// Named keys pressed through `keys`, other than a lone submit.
    Keys(Vec<String>),
}

/// One session the fake server holds, and everything the verbs did to it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FakeSession {
    pub cwd: String,
    /// The pane's command, as the start handed it.
    pub argv: Vec<String>,
    /// The pane's whole environment, as the start handed it.
    pub env: Vec<(String, String)>,
    pub pid: u32,
    /// `None` while the pane is alive; `Some(status)` once it has ended, and
    /// `Some(None)` for an end with no status to report.
    pub ended: Option<Option<i32>>,
    /// Whole seconds, as a real host counts them, in epoch milliseconds.
    pub created_ms: u64,
    /// What a capture answers: every paste as it was pasted and a line break
    /// per submit, unless an arm has set it outright.
    pub screen: String,
    pub sent: Vec<Sent>,
}

/// The first pid a fake pane is given: above the highest pid either platform
/// ever hands out (99 998 on macOS, and Linux's ceiling of 4 194 304), so a
/// retire's probe of a fake pane's pid reads a process that cannot exist —
/// never some other process that happens to hold the number.
pub const FIRST_PANE_PID: u32 = 4_200_000;

/// The key an interrupt is pressed as.
pub const INTERRUPT: &str = "C-c";

/// A host server with no process behind it.
///
/// Every verb answers in the words a real server uses where a caller reads
/// them — a duplicate start, a session or pane not found — so the one
/// classification the real host makes (a kill of a session already gone is
/// `Ok`) is exercised against the fake too.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FakeServer {
    /// Keyed by name, so a listing comes back in name order, as a real
    /// server's does.
    #[serde(default)]
    pub sessions: std::collections::BTreeMap<String, FakeSession>,
    #[serde(default)]
    pub started: u32,
    /// Paste buffers loaded and not yet pasted. The stub binary's alone: the
    /// in-process fake pastes in one step.
    #[serde(default)]
    pub buffers: std::collections::BTreeMap<String, String>,
    /// Every argument list the stub binary was run with, in order — how a cli
    /// suite reads what `fleet` asked for, an attach included. The in-process
    /// fake records [`Call`]s instead.
    #[serde(default)]
    pub invocations: Vec<Vec<String>>,
    /// The whole environment each `attach-session` the stub binary was run
    /// under, in order. The attach is the one client run with the PERSON'S
    /// environment rather than a constructed one, so what it carried — and
    /// what it did not, `TMUX` above all — is a suite's to read. The
    /// in-process fake builds that command and runs nothing, so it records
    /// none.
    #[serde(default)]
    pub attach_envs: Vec<Vec<(String, String)>>,
    /// Where set, EVERY session started ends at once with this status and
    /// shows [`FakeServer::screen_every_start`] — a program that exits before
    /// anyone reads its pane — until an arm clears it. The stub binary's
    /// suites set it on the state file, since no client call can time a pane's
    /// death inside a start.
    #[serde(default)]
    pub end_every_start: Option<Option<i32>>,
    #[serde(default)]
    pub screen_every_start: Option<String>,
    /// Where set, a `C-c` pressed into a live pane ends it, as a program that
    /// dies on the interrupt does: dead with no status, since a process ended by
    /// a signal carries none.
    ///
    /// Unset, the interrupt ends nothing, which is the agent measured: on Claude
    /// Code 2.1.280 one `C-c` ended a busy session's turn and left an idle one
    /// asking for a second press, and neither pane died (fleet-rge6.4,
    /// 2026-09-26). So a stop against the default waits out its whole grace;
    /// a suite whose arms stop sessions without being about that wait sets
    /// this, and the arm about the grace does not.
    #[serde(default)]
    pub exit_on_interrupt: bool,
    /// Where set, a kill answers and takes nothing: the session stays, as on a
    /// host whose kill did not land — which only the host's own reading after
    /// it can tell from one that did.
    #[serde(default)]
    pub kill_keeps: bool,
}

impl FakeServer {
    /// A server read from the file at `path`, or an empty one where there is
    /// no file yet.
    pub fn load(path: &Path) -> Result<FakeServer, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("{}: not a fake host state: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FakeServer::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Write the server to `path` whole, through a rename, so a reader never
    /// meets half a state.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        crate::platform::write_atomic(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn start(
        &mut self,
        name: &str,
        cwd: &str,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<(), String> {
        if self.sessions.contains_key(name) {
            return Err(format!("duplicate session: {name}"));
        }
        let pid = FIRST_PANE_PID + self.started;
        self.started += 1;
        let now_ms = crate::clock::now_ms();
        self.sessions.insert(
            name.to_string(),
            FakeSession {
                cwd: cwd.to_string(),
                argv: argv.to_vec(),
                env: env.to_vec(),
                pid,
                created_ms: now_ms - now_ms % 1000,
                screen: self.screen_every_start.clone().unwrap_or_default(),
                sent: Vec::new(),
                ended: self.end_every_start,
            },
        );
        Ok(())
    }

    fn pane(&mut self, name: &str) -> Result<&mut FakeSession, String> {
        self.sessions
            .get_mut(name)
            .ok_or_else(|| format!("can't find pane: ={name}:"))
    }

    /// A paste into a pane that has ended is refused in the real server's
    /// words (measured on 3.7b); keys into one are taken and go nowhere.
    pub fn paste(&mut self, name: &str, text: &str) -> Result<(), String> {
        let pane = self.pane(name)?;
        if pane.ended.is_some() {
            return Err("target pane has exited".to_string());
        }
        pane.screen.push_str(text);
        pane.sent.push(Sent::Paste(text.to_string()));
        Ok(())
    }

    pub fn submit(&mut self, name: &str) -> Result<(), String> {
        let pane = self.pane(name)?;
        pane.screen.push('\n');
        pane.sent.push(Sent::Submit);
        Ok(())
    }

    pub fn press(&mut self, name: &str, keys: &[String]) -> Result<(), String> {
        let exits = self.exit_on_interrupt;
        let pane = self.pane(name)?;
        pane.sent.push(Sent::Keys(keys.to_vec()));
        if exits && pane.ended.is_none() && keys.iter().any(|key| key == INTERRUPT) {
            pane.ended = Some(None);
        }
        Ok(())
    }

    /// Give the session's pane `pid`, as a host whose pane runs a process the
    /// arm chose — the pid a retire's last probe reads.
    pub fn set_pid(&mut self, name: &str, pid: u32) -> Result<(), String> {
        self.pane(name)?.pid = pid;
        Ok(())
    }

    pub fn capture(&mut self, name: &str) -> Result<String, String> {
        Ok(self.pane(name)?.screen.clone())
    }

    /// Replace what a capture of the session answers.
    pub fn set_screen(&mut self, name: &str, screen: &str) -> Result<(), String> {
        self.pane(name)?.screen = screen.to_string();
        Ok(())
    }

    /// Whether there was a session to kill ([`FakeServer::kill_keeps`] keeps
    /// it all the same).
    pub fn kill(&mut self, name: &str) -> bool {
        if self.kill_keeps {
            return self.sessions.contains_key(name);
        }
        self.sessions.remove(name).is_some()
    }

    /// End the session's pane with `status`, and keep it, as remain-on-exit
    /// keeps a real one.
    pub fn end(&mut self, name: &str, status: Option<i32>) -> Result<(), String> {
        self.pane(name)?.ended = Some(status);
        Ok(())
    }

    /// Every pane, as the real host lists them: a dead pane keeps the pid it
    /// had and reads its path empty (measured on 3.7b).
    pub fn panes(&self) -> Vec<crate::host::Pane> {
        self.sessions
            .iter()
            .map(|(name, s)| crate::host::Pane {
                session: name.clone(),
                pid: Some(s.pid),
                state: match s.ended {
                    None => crate::host::PaneState::Alive,
                    Some(status) => crate::host::PaneState::Dead { status },
                },
                path: if s.ended.is_none() {
                    s.cwd.clone()
                } else {
                    String::new()
                },
                created_ms: Some(s.created_ms),
            })
            .collect()
    }
}

/// The host seam with no server behind it: every verb answers from a
/// [`FakeServer`] and records that it was asked.
///
/// A verb set failing through [`FakeHost::fail`] answers that cause and
/// changes nothing; a failing `list` reads Unreadable with it.
pub struct FakeHost {
    server: Mutex<FakeServer>,
    calls: Mutex<Vec<Call>>,
    failing: Mutex<std::collections::BTreeMap<&'static str, String>>,
    version: Mutex<Option<String>>,
    /// What the NEXT session started does before anyone reads it: ends with a
    /// status, or shows a screen. Taken by that start and gone after it.
    next_start: Mutex<NextStart>,
}

/// See [`FakeHost::end_next_start`] and [`FakeHost::draw_next_start`].
#[derive(Default)]
struct NextStart {
    end: Option<Option<i32>>,
    screen: Option<String>,
}

impl FakeHost {
    pub const NEW_SESSION: &'static str = "new_session";
    pub const SEND: &'static str = "send";
    pub const KEYS: &'static str = "keys";
    pub const CAPTURE: &'static str = "capture";
    pub const KILL: &'static str = "kill";
    pub const LIST: &'static str = "list";
    pub const ATTACH: &'static str = "attach";
    pub const VERSION_CALL: &'static str = "version";

    /// What `version` answers until an arm sets another: the release measured.
    pub const VERSION: &'static str = "3.7b";

    /// The program the attach command names. `true`, so a caller that runs
    /// the command it was handed gets an exit 0 and touches no terminal.
    pub const ATTACH_PROGRAM: &'static str = "/usr/bin/true";

    pub fn new() -> FakeHost {
        FakeHost {
            server: Mutex::new(FakeServer::default()),
            calls: Mutex::new(Vec::new()),
            failing: Mutex::new(Default::default()),
            version: Mutex::new(Some(FakeHost::VERSION.to_string())),
            next_start: Mutex::new(NextStart::default()),
        }
    }

    /// Make `verb` answer `cause` from now on, or answer again where `None`.
    pub fn fail(&self, verb: &'static str, cause: Option<&str>) {
        let mut failing = self.failing.lock().expect("the fake host's own lock");
        match cause {
            Some(cause) => failing.insert(verb, cause.to_string()),
            None => failing.remove(verb),
        };
    }

    pub fn set_version(&self, version: Option<&str>) {
        *self.version.lock().expect("the fake host's own lock") = version.map(str::to_string);
    }

    /// Make the next session started end at once with `status`, as a program
    /// that exits before anyone reads its pane does — so a start's watch meets
    /// a dead pane on its first read.
    pub fn end_next_start(&self, status: Option<i32>) {
        self.next_start
            .lock()
            .expect("the fake host's own lock")
            .end = Some(status);
    }

    /// Make the next session started show `screen` from its first capture, as
    /// a program that stops at a question before it does anything else.
    pub fn draw_next_start(&self, screen: &str) {
        self.next_start
            .lock()
            .expect("the fake host's own lock")
            .screen = Some(screen.to_string());
    }

    /// Make every `C-c` pressed into a live pane end it from now on
    /// ([`FakeServer::exit_on_interrupt`]).
    pub fn exit_on_interrupt(&self) {
        self.server().exit_on_interrupt = true;
    }

    /// Make every kill from now on answer and keep its session, or take it
    /// again where `false` ([`FakeServer::kill_keeps`]).
    pub fn keep_kills(&self, keep: bool) {
        self.server().kill_keeps = keep;
    }

    /// Give the session's pane `pid` ([`FakeServer::set_pid`]).
    pub fn set_pid(&self, name: &str, pid: u32) {
        self.server()
            .set_pid(name, pid)
            .unwrap_or_else(|why| panic!("the fake host has no session to renumber: {why}"));
    }

    /// End the session's pane with `status`, and keep it dead in the listing.
    pub fn end(&self, name: &str, status: Option<i32>) {
        self.server()
            .end(name, status)
            .unwrap_or_else(|why| panic!("the fake host has no session to end: {why}"));
    }

    /// The first half of a send alone, as the real host's `paste` is: the text
    /// pasted and not submitted. Recorded as a `send`.
    pub fn paste(&self, name: &str, text: &str) -> Result<(), String> {
        self.asked(FakeHost::SEND, name)?;
        self.server().paste(name, text)
    }

    /// Replace what a capture of the session answers.
    pub fn set_screen(&self, name: &str, screen: &str) {
        self.server()
            .set_screen(name, screen)
            .unwrap_or_else(|why| panic!("the fake host has no session to draw: {why}"));
    }

    /// Everything typed into the session, the pastes and the submits in the
    /// order they arrived; empty for a session that is not there.
    pub fn sends(&self, name: &str) -> Vec<Sent> {
        self.session(name).map(|s| s.sent).unwrap_or_default()
    }

    /// The session as the fake holds it — its start's cwd, argv and env
    /// included — or `None` where it is not there.
    pub fn session(&self, name: &str) -> Option<FakeSession> {
        self.server().sessions.get(name).cloned()
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("the fake host's own lock").clone()
    }

    /// The verbs alone, in order.
    pub fn verbs(&self) -> Vec<&'static str> {
        self.calls().into_iter().map(|call| call.verb).collect()
    }

    pub fn calls_of(&self, verb: &str) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.verb == verb)
            .collect()
    }

    fn server(&self) -> std::sync::MutexGuard<'_, FakeServer> {
        self.server.lock().expect("the fake host's own lock")
    }

    /// Record the call, then the cause it was set to fail with, if any.
    fn asked(&self, verb: &'static str, about: &str) -> Result<(), String> {
        self.calls
            .lock()
            .expect("the fake host's own lock")
            .push(Call {
                verb,
                about: about.to_string(),
            });
        match self
            .failing
            .lock()
            .expect("the fake host's own lock")
            .get(verb)
        {
            Some(cause) => Err(cause.clone()),
            None => Ok(()),
        }
    }
}

impl Default for FakeHost {
    fn default() -> Self {
        FakeHost::new()
    }
}

impl crate::host::Host for FakeHost {
    fn new_session(
        &self,
        name: &str,
        cwd: &Path,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<(), String> {
        self.asked(FakeHost::NEW_SESSION, name)?;
        let mut server = self.server();
        server.start(name, &cwd.to_string_lossy(), argv, env)?;
        let next = std::mem::take(&mut *self.next_start.lock().expect("the fake host's own lock"));
        if let Some(screen) = next.screen {
            server.set_screen(name, &screen)?;
        }
        if let Some(status) = next.end {
            server.end(name, status)?;
        }
        Ok(())
    }

    fn send(&self, name: &str, text: &str) -> Result<(), String> {
        self.asked(FakeHost::SEND, name)?;
        let mut server = self.server();
        server.paste(name, text)?;
        server.submit(name)
    }

    fn keys(&self, name: &str, keys: &[&str]) -> Result<(), String> {
        self.asked(FakeHost::KEYS, name)?;
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        self.server().press(name, &keys)
    }

    fn capture(&self, name: &str) -> Result<String, String> {
        self.asked(FakeHost::CAPTURE, name)?;
        self.server().capture(name)
    }

    fn kill(&self, name: &str) -> Result<(), String> {
        self.asked(FakeHost::KILL, name)?;
        self.server().kill(name);
        Ok(())
    }

    fn list(&self) -> crate::host::HostRead {
        match self.asked(FakeHost::LIST, "") {
            Ok(()) => crate::host::HostRead::Readable(self.server().panes()),
            Err(cause) => crate::host::HostRead::Unreadable { cause },
        }
    }

    fn attach(&self, name: &str, write: bool) -> std::process::Command {
        // An attach never fails here: the command is built, not run.
        let _ = self.asked(FakeHost::ATTACH, name);
        let mut cmd = std::process::Command::new(FakeHost::ATTACH_PROGRAM);
        cmd.args([
            FakeHost::ATTACH,
            name,
            if write { "write" } else { "read-only" },
        ]);
        cmd
    }

    fn version(&self) -> Option<String> {
        self.asked(FakeHost::VERSION_CALL, "").ok()?;
        self.version
            .lock()
            .expect("the fake host's own lock")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The arrivals follow the arm's own rows, whose bytes are kept, and the
    /// result still parses as the listing it was — an empty one, a one-row
    /// one — while a body that is no array is left exactly as the arm wrote it.
    #[test]
    fn the_arrivals_follow_the_arms_own_rows_and_keep_their_bytes() {
        let empty = with_arrivals("[]");
        let rows = match crate::adapter::claude_code::parse_roster(&empty) {
            RosterRead::Readable(rows) => rows,
            RosterRead::Unreadable { cause } => panic!("{cause}: {empty}"),
        };
        assert_eq!(rows.len(), LISTED_ARRIVALS as usize);
        assert_eq!(rows[0].pid, Some(FIRST_PANE_PID));

        let own = "[{\"sessionId\": \"a-session\", \"cwd\": \"/wt\", \"pid\": 4242}]\n";
        let listed = with_arrivals(own);
        assert!(
            listed.starts_with("[{\"sessionId\": \"a-session\", \"cwd\": \"/wt\", \"pid\": 4242}"),
            "{listed}"
        );
        let rows = match crate::adapter::claude_code::parse_roster(&listed) {
            RosterRead::Readable(rows) => rows,
            RosterRead::Unreadable { cause } => panic!("{cause}: {listed}"),
        };
        assert_eq!(rows.len(), LISTED_ARRIVALS as usize + 1);

        assert_eq!(with_arrivals("not a listing"), "not a listing");
        assert_eq!(with_arrivals(""), "");
    }

    /// A sleep advances the clock by EXACTLY what it was asked for — not a
    /// slice, not a rounding, and the sum of several is the sum of their
    /// durations.
    #[test]
    fn a_sleep_advances_the_fake_clock_by_exactly_the_duration_it_was_given() {
        let clock = FakeClock::new();
        let start = clock.now();

        clock.sleep(Duration::from_secs(5));
        assert_eq!(clock.now() - start, Duration::from_secs(5));
        assert_eq!(clock.spent(), Duration::from_secs(5));

        clock.sleep(Duration::from_millis(250));
        assert_eq!(clock.now() - start, Duration::from_millis(5_250));

        clock.sleep(Duration::ZERO);
        assert_eq!(clock.now() - start, Duration::from_millis(5_250));
    }

    /// The property every seamed deadline rests on, asserted from BOTH sides: a
    /// clock that over-advances expires a deadline early and fails the
    /// nanosecond-short assert; one that under-advances never reaches it and
    /// fails the arrival assert.
    #[test]
    fn a_deadline_taken_from_the_fake_clock_expires_when_fake_time_passes_it_and_not_before() {
        let clock = FakeClock::new();
        let deadline = clock.now() + Duration::from_secs(30);

        assert!(clock.now() < deadline, "a deadline is not expired at once");

        clock.sleep(Duration::from_secs(30) - Duration::from_nanos(1));
        assert!(
            clock.now() < deadline,
            "a nanosecond short of the deadline is short of the deadline"
        );

        clock.sleep(Duration::from_nanos(1));
        assert!(clock.now() >= deadline, "fake time reached the deadline");

        clock.sleep(Duration::from_secs(1));
        assert!(clock.now() >= deadline, "and stays past it");
    }
}
