//! The agent contract answered by an executable over [`StubAgent`]:
//! `fleet-agent-stub <verb>`, the request on stdin and the answer on stdout,
//! one process per call as [`crate::adapter::AgentExec`] runs one — so every
//! agent path a suite drives goes through the Exec with no agent installed on
//! the box and no shell stub standing in for one.
//!
//! THE FAKE LIVES IN A FILE, [`STATE_FILE`] under the request's root: each call
//! loads the [`State`] from it, answers the verb from `impl Agent for
//! StubAgent` — the in-process stub's own rules, so the two seams answer one
//! set — appends the call to the state's log, and writes the state back, all
//! under [`LOCK_FILE`]. A rig writes the answers between ticks with [`script`],
//! the file twin of [`StubAgent::set`], and reads the calls back with [`calls`]
//! in the in-process stub's own [`Call`] shape. A root with no state file is
//! answered from [`Answers::default`] and nothing is written there.
//!
//! THE EXIT IS THE CONTRACT'S TABLE. 0 is the verb's response; 1 a launch or a
//! resume scripted to refuse, as `{"refused": …}`; 2 an unknown verb, a request
//! that does not decode as its verb's fields, or `context` asked of a stub
//! whose capabilities declare none; 3 a launch or a resume scripted as could
//! not tell, any verb scripted so through [`untold`], and a state that will
//! not read, as `{"error": …}`. Nothing is written on stderr but a usage line,
//! because the Exec carries stderr into every refusal it reads.
//!
//! WHAT A LAUNCH ANSWERS IS THE STUB AGAIN: `[<its own path>, "session",
//! <seat>, …]`, the flags [`StubAgent::launched`] names after it, with
//! [`ROOT_VAR`] in the environment. Run in a pane, that is the fake agent's
//! process ([`SESSION`]): it lists itself idle under its own pid, reads busy
//! for a [`TURN`] on each line typed into it, and leaves on [`EXIT`] — each
//! move written into the state's listing, so a suite on a real host reads
//! starting, idle and busy with no model behind them. Under a fake host no pane
//! runs anything, and the rig scripts the listing instead — or, over
//! `fleet-tmux-stub`, has the read follow its panes ([`follow_host`]).
//!
//! Three knobs, read off the environment for the one call that carries them:
//! [`DEAF`] answers the call and writes nothing back, [`SLOW`] sleeps before
//! the call is answered, outside the lock, and [`WRITE`] has a launch write a
//! file where it is told — inside what the contract lets a launch write, or
//! outside it, for `fleet agent check` to catch.
//!
//! A SESSION KEEPS A TRANSCRIPT: each line typed into one appends a turn to
//! the state's [`Answers::session_log`] and stamps [`Answers::last_write`], so
//! `context` after a typed turn counts it, as a live agent's would.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead as _, Read as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fleet_core::agent::types::{Seats, CONTRACT_VERSION};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};

use super::{Answers, Call, StubAgent};
use crate::adapter::{Agent, AgentError, Launch, Resume};

/// Where a root's fake is kept, relative to it.
pub const STATE_FILE: &str = ".agent-stub/state.json";

/// The file every call on a root holds an exclusive lock on, from before the
/// state is read until after it is written back.
pub const LOCK_FILE: &str = ".agent-stub/state.lock";

/// `1` in the environment: this call is answered and nothing is written back —
/// no log line, and no listing taken off [`State::listed_next`].
///
/// The agent's own names and not the store stub's `FLEET_STUB_*`: a `fleet`
/// running both stubs hands both its environment, and a knob meant for one
/// would slow or deafen the other.
pub const DEAF: &str = "FLEET_AGENT_STUB_DEAF";

/// Seconds, in the environment: how long this call sleeps before it is
/// answered, for an arm about an agent call's bound.
pub const SLOW: &str = "FLEET_AGENT_STUB_SLOW";

/// A path, in the environment: a launch writes an empty file there before it
/// answers, the path read against the request's `config_dir` — its
/// `worktree` where it names none — so `../elsewhere` lands outside both.
/// For an arm about where a launch may write (reviewer call 2026-09-25, E8).
pub const WRITE: &str = "FLEET_AGENT_STUB_WRITE";

/// The entry a [`SESSION`] appends to the state's transcript for each line
/// typed into it: one turn that used a thousand tokens of the window, in the
/// shape [`super::reading::context_of`] reads a session log in.
pub const TURN_ENTRY: &str =
    "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":1000}}}\n";

/// The root whose state a [`SESSION`] writes its moves into: set by the stub's
/// own launch and resume in the environment they answer, since a pane's
/// process is handed no request.
pub const ROOT_VAR: &str = "FLEET_AGENT_STUB_ROOT";

/// The argument a launch's argv runs the stub under: the fake agent's process.
pub const SESSION: &str = "session";

/// The line a [`SESSION`] leaves on.
pub const EXIT: &str = "/exit";

/// How long a [`SESSION`] reads busy on a line typed into it before it reads
/// idle again: long enough for a poll a second apart to see it.
pub const TURN: Duration = Duration::from_secs(2);

/// How long a call waits on another call's lock before it answers could not
/// tell: well inside the agent call's own 20 s bound, and far past any one
/// call's hold, which is a load, a verb and a save.
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// Every verb of the contract, which is every verb this stub answers.
const VERBS: [&str; 6] = [
    StubAgent::CAPABILITIES,
    StubAgent::VERSION_CALL,
    StubAgent::LAUNCH,
    StubAgent::RESUME,
    StubAgent::READ,
    StubAgent::CONTEXT,
];

/// The fake as its file holds it: what it answers, the listings queued ahead
/// of [`Answers::listing`] ([`StubAgent::list_next`]), the two things only a
/// fake in a file is told, and every call it answered, in order.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct State {
    pub answers: Answers,
    pub listed_next: VecDeque<Result<String, String>>,
    /// Verbs answered could not tell (exit 3) with this error, whatever
    /// [`Answers`] says — a version call that fails, say, beside the null
    /// version that says no agent is installed ([`untold`]).
    pub untold: BTreeMap<String, String>,
    /// The `fleet-tmux-stub` state whose panes a read follows: a listed row
    /// under a pane that has taken a submit reads busy, as a session that took
    /// a typed turn does, until the pane's record is cleared ([`follow_host`]).
    /// The one link between the two fakes, for the suites that drive the built
    /// `fleet`, where no pane runs a process to say it for itself.
    pub host: Option<PathBuf>,
    pub calls: Vec<Logged>,
}

/// One call the stub answered: its verb and its whole request, envelope and
/// all.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Logged {
    pub verb: String,
    pub request: Value,
}

/// How a call ends, by the row of the contract's exit table it answers.
enum Answer {
    /// Exit 0: the verb's response body.
    Body(Value),
    /// Exit 1: the agent's own refusal.
    Refused(Value),
    /// Exit 2: a request this stub does not speak, and why.
    Usage(String),
    /// Exit 3: nothing could be told, and why.
    Untold(String),
}

/// One call: the verb off argv, the request off stdin, the answer on stdout and
/// the exit by the contract's table — or, under [`SESSION`], the fake agent's
/// own process.
pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(SESSION) {
        return session(&args[1..]);
    }
    let verb = args.first().cloned().unwrap_or_default();
    let mut request = String::new();
    let answer = match std::io::stdin().read_to_string(&mut request) {
        Err(e) => Answer::Usage(format!("the request could not be read off stdin: {e}")),
        Ok(_) => match slow() {
            Err(why) => Answer::Usage(why),
            Ok(wait) => {
                std::thread::sleep(wait);
                let deaf = std::env::var(DEAF).is_ok_and(|value| value == "1");
                answered(&verb, &request, deaf)
            }
        },
    };
    let (code, mut printed) = match answer {
        Answer::Body(body) => (0, body),
        Answer::Refused(refusal) => (1, json!({ "refused": refusal })),
        Answer::Usage(why) => {
            eprintln!("fleet-agent-stub {verb}: {why}");
            return ExitCode::from(2);
        }
        Answer::Untold(why) => (3, json!({ "error": why })),
    };
    if let Value::Object(body) = &mut printed {
        body.insert(
            String::from("schema_version"),
            Value::from(CONTRACT_VERSION),
        );
    }
    println!("{printed}");
    ExitCode::from(code)
}

/// [`SLOW`]'s wait, none where it is unset.
fn slow() -> Result<Duration, String> {
    match std::env::var(SLOW) {
        Err(_) => Ok(Duration::ZERO),
        Ok(secs) => secs
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
            .ok_or_else(|| format!("{SLOW} is `{secs}`, which is no number of seconds")),
    }
}

/// The verb answered from the request's text.
fn answered(verb: &str, request: &str, deaf: bool) -> Answer {
    if !VERBS.contains(&verb) {
        return Answer::Usage(format!("`{verb}` is no verb of the agent contract"));
    }
    let fields = match envelope(request) {
        Ok(fields) => fields,
        Err(why) => return Answer::Usage(why),
    };
    let root = match fields.get("root").and_then(Value::as_str) {
        Some(root) => PathBuf::from(root),
        None => return Answer::Usage(String::from("the request carries no `root` as text")),
    };
    // Decoded before the state is touched, so a request the stub does not
    // speak is refused whatever the state holds, and never logged.
    let asked = match Asked::of(verb, &fields) {
        Ok(asked) => asked,
        Err(why) => return Answer::Usage(why),
    };
    if let Asked::Launch(launch) = &asked {
        if let Err(why) = written(launch) {
            return Answer::Untold(why);
        }
    }
    let logged = Logged {
        verb: verb.to_string(),
        request: Value::Object(fields),
    };
    let answer = if deaf || !root.join(STATE_FILE).is_file() {
        load(&root).map(|mut state| asked.answer(&mut state, &root))
    } else {
        with_state(&root, |state| {
            let answer = asked.answer(state, &root);
            if !matches!(answer, Answer::Usage(_)) {
                state.calls.push(logged);
            }
            answer
        })
    };
    answer.unwrap_or_else(Answer::Untold)
}

/// The file [`WRITE`] names written for `launch`, where it names one.
fn written(launch: &Launch) -> Result<(), String> {
    let Ok(path) = std::env::var(WRITE) else {
        return Ok(());
    };
    let base = launch.config_dir.as_deref().unwrap_or(&launch.worktree);
    let file = Path::new(base).join(path);
    std::fs::write(&file, "").map_err(|e| format!("{} could not be written: {e}", file.display()))
}

/// A request decoded as its verb's fields.
enum Asked {
    Capabilities,
    Version,
    Launch(Launch),
    Resume(Resume),
    Read(Seats),
    Context(Seats),
}

impl Asked {
    fn of(verb: &str, fields: &Map<String, Value>) -> Result<Asked, String> {
        Ok(match verb {
            StubAgent::CAPABILITIES => Asked::Capabilities,
            StubAgent::VERSION_CALL => Asked::Version,
            StubAgent::LAUNCH => Asked::Launch(whole(fields, verb)?),
            StubAgent::RESUME => Asked::Resume(whole(fields, verb)?),
            StubAgent::READ => Asked::Read(whole(fields, verb)?),
            StubAgent::CONTEXT => Asked::Context(whole(fields, verb)?),
            other => unreachable!("`{other}` is one of the contract's verbs"),
        })
    }

    fn verb(&self) -> &'static str {
        match self {
            Asked::Capabilities => StubAgent::CAPABILITIES,
            Asked::Version => StubAgent::VERSION_CALL,
            Asked::Launch(_) => StubAgent::LAUNCH,
            Asked::Resume(_) => StubAgent::RESUME,
            Asked::Read(_) => StubAgent::READ,
            Asked::Context(_) => StubAgent::CONTEXT,
        }
    }

    /// The answer the in-process stub gives over `state` — its listings
    /// following the host where one is followed — with the listings it did not
    /// take put back as they were.
    fn answer(&self, state: &mut State, root: &Path) -> Answer {
        if let Some(error) = state.untold.get(self.verb()) {
            return Answer::Untold(error.clone());
        }
        let taken = match &state.host {
            Some(host) => match typed_into(host) {
                Ok(taken) => taken,
                Err(why) => return Answer::Untold(why),
            },
            None => Vec::new(),
        };
        let followed = |listing: &Result<String, String>| match listing {
            Ok(body) => Ok(busy_under(body, &taken)),
            Err(why) => Err(why.clone()),
        };
        let agent = StubAgent::answering(Answers {
            listing: followed(&state.answers.listing),
            ..state.answers.clone()
        });
        agent.list_next(state.listed_next.iter().map(followed));
        let answered = match self {
            Asked::Capabilities => agent.capabilities().map(body),
            Asked::Version => agent.version().map(body),
            Asked::Launch(launch) => agent
                .launch(launch)
                .map(|argv| body(own(argv, Some(&launch.seat.to_string()), root))),
            Asked::Resume(resume) => agent.resume(resume).map(|argv| body(own(argv, None, root))),
            Asked::Read(asked) => agent
                .read(&asked.seats)
                .map(|seats| json!({ "seats": seats })),
            Asked::Context(asked) => {
                if !state.answers.capabilities.context {
                    return Answer::Usage(String::from(
                        "context is asked of an agent whose capabilities declare none",
                    ));
                }
                agent
                    .context(&asked.seats)
                    .map(|seats| json!({ "seats": seats }))
            }
        };
        let left = agent
            .listed_next
            .into_inner()
            .expect("the stub agent's own lock")
            .len();
        let spent = state.listed_next.len() - left;
        state.listed_next.drain(..spent);
        match answered {
            Ok(value) => Answer::Body(value),
            Err(AgentError::Refused(refusal)) => Answer::Refused(body(refusal)),
            Err(AgentError::Unreadable(why)) => Answer::Untold(why),
        }
    }
}

/// The pids of the panes on the fake host at `host` that have taken a submit.
fn typed_into(host: &Path) -> Result<Vec<u64>, String> {
    Ok(super::FakeServer::load(host)?
        .sessions
        .values()
        .filter(|pane| pane.sent.contains(&super::Sent::Submit))
        .map(|pane| u64::from(pane.pid))
        .collect())
}

/// The listing with every row under one of `taken`'s pids reading busy. A
/// listing that is no JSON array is left as it was.
fn busy_under(listing: &str, taken: &[u64]) -> String {
    if taken.is_empty() {
        return listing.to_string();
    }
    let Ok(mut rows) = serde_json::from_str::<Vec<Value>>(listing) else {
        return listing.to_string();
    };
    for row in &mut rows {
        if row["pid"].as_u64().is_some_and(|pid| taken.contains(&pid)) {
            row["status"] = Value::from("busy");
        }
    }
    Value::Array(rows).to_string()
}

/// A response type as its JSON body.
fn body(response: impl serde::Serialize) -> Value {
    serde_json::to_value(response).expect("a response body is JSON")
}

/// The in-process stub's argv, run by this executable instead: its program is
/// this stub under [`SESSION`] and the launch's seat, its flags after — and
/// [`ROOT_VAR`] in the environment, naming the state the session writes into.
/// A resume's request names no seat, and its argv none: the session id is its
/// flag.
fn own(mut argv: crate::adapter::Argv, seat: Option<&str>, root: &Path) -> crate::adapter::Argv {
    let program = std::env::current_exe()
        .map(|exe| exe.display().to_string())
        .unwrap_or_else(|_| String::from("fleet-agent-stub"));
    let flags = argv.argv.split_off(1.min(argv.argv.len()));
    argv.argv = [program, SESSION.to_string()]
        .into_iter()
        .chain(seat.map(str::to_string))
        .chain(flags)
        .collect();
    argv.env
        .insert(ROOT_VAR.to_string(), root.display().to_string());
    argv
}

/// The request's fields, held to the envelope: one JSON object at the agent
/// contract's `schema_version`.
fn envelope(request: &str) -> Result<Map<String, Value>, String> {
    let Some(Value::Object(fields)) = fleet_core::adapter::exec::first_value(request) else {
        return Err(String::from("the request is not one JSON object"));
    };
    match fields.get("schema_version") {
        Some(version) if version.as_u64() == Some(CONTRACT_VERSION) => Ok(fields),
        other => Err(format!(
            "the request's schema_version is {}; this stub speaks {CONTRACT_VERSION}",
            other.map_or_else(|| String::from("missing"), Value::to_string)
        )),
    }
}

/// The whole request read as the verb's fields, which sit beside the
/// envelope's keys rather than under a key of their own.
fn whole<T: DeserializeOwned>(fields: &Map<String, Value>, verb: &str) -> Result<T, String> {
    serde_json::from_value(Value::Object(fields.clone()))
        .map_err(|why| format!("the {verb} request does not read: {why}"))
}

// ---- the session ------------------------------------------------------------------

/// The fake agent's process: listed idle under its own pid, busy for a
/// [`TURN`] on each line typed into it — a [`TURN_ENTRY`] in the transcript —
/// and gone on [`EXIT`] or at the end of its input. A launch's session is listed under an id of its own; a resume's
/// under the id its `--resume` names.
fn session(args: &[String]) -> ExitCode {
    let Some(root) = std::env::var_os(ROOT_VAR).map(PathBuf::from) else {
        eprintln!("fleet-agent-stub {SESSION}: {ROOT_VAR} names no state to write into");
        return ExitCode::from(2);
    };
    let pid = std::process::id();
    let session_id = args
        .iter()
        .position(|arg| arg == "--resume")
        .and_then(|at| args.get(at + 1))
        .cloned()
        .unwrap_or_else(|| format!("stub-session-{pid}"));
    let cwd = std::env::current_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    let listed = |status: Option<&str>| {
        let moved = with_state(&root, |state| {
            if let Ok(listing) = &mut state.answers.listing {
                *listing = relisted(
                    listing,
                    pid,
                    status.map(|status| (&session_id, &cwd, status)),
                );
            }
            // A turn begun is a turn in the transcript, written as it starts,
            // as a live agent writes the prompt it took.
            if status == Some("busy") {
                state
                    .answers
                    .session_log
                    .get_or_insert_with(String::new)
                    .push_str(TURN_ENTRY);
                state.answers.last_write = Some(crate::clock::now_ms());
            }
        });
        if let Err(why) = moved {
            eprintln!("fleet-agent-stub {SESSION}: {why}");
        }
    };
    listed(Some("idle"));
    for line in std::io::stdin().lock().lines() {
        match line {
            Ok(line) if line.trim() == EXIT => break,
            Ok(_) => {
                listed(Some("busy"));
                std::thread::sleep(TURN);
                listed(Some("idle"));
            }
            Err(_) => break,
        }
    }
    listed(None);
    ExitCode::SUCCESS
}

/// The listing with the row under `pid` replaced by `row` — a session id, a
/// directory and a status — or taken out where there is none. A listing that
/// is no JSON array is left as it was: an arm serving one that does not parse
/// means exactly that.
fn relisted(listing: &str, pid: u32, row: Option<(&String, &String, &str)>) -> String {
    let Ok(mut rows) = serde_json::from_str::<Vec<Value>>(listing) else {
        return listing.to_string();
    };
    rows.retain(|listed| listed["pid"].as_u64() != Some(u64::from(pid)));
    if let Some((session_id, cwd, status)) = row {
        rows.push(json!({
            "sessionId": session_id,
            "cwd": cwd,
            "kind": "interactive",
            "pid": pid,
            "status": status,
            "startedAt": crate::clock::now_ms(),
        }));
    }
    Value::Array(rows).to_string()
}

// ---- the state file ---------------------------------------------------------------

/// The executable this crate's own test build made: in `target/<profile>/`, the
/// directory above the running test binary's `deps/`, which is where
/// `env!("CARGO_BIN_EXE_fleet-agent-stub")` points a suite of this crate. The
/// library cannot name that variable, since cargo sets it only for a crate's
/// test targets.
///
/// FOR THIS CRATE'S TESTS ONLY. Another crate's test build does not build this
/// crate's executables, so the file here is whatever an earlier controller
/// build left, or nothing; the cli's rigs run the copy their own build makes,
/// its example `fleet-agent-stub`.
pub fn path() -> PathBuf {
    let running = std::env::current_exe().expect("the running test binary has a path");
    let stub = running
        .parent()
        .and_then(Path::parent)
        .expect("a test binary runs from target/<profile>/deps")
        .join("fleet-agent-stub");
    assert!(
        stub.is_file(),
        "{} is built by a test build of fleet-controller",
        stub.display()
    );
    stub
}

/// Change what the stub at `root` answers from its next call on — the file
/// twin of [`StubAgent::set`]. A root with no state is given one, starting
/// from [`Answers::default`].
pub fn script(root: &Path, change: impl FnOnce(&mut Answers)) {
    with_state(root, |state| change(&mut state.answers))
        .unwrap_or_else(|why| panic!("the agent stub's state at {}: {why}", root.display()));
}

/// Answer the next reads at `root` with `listings`, one each and in order —
/// the file twin of [`StubAgent::list_next`].
pub fn list_next(root: &Path, listings: impl IntoIterator<Item = Result<String, String>>) {
    with_state(root, |state| state.listed_next.extend(listings))
        .unwrap_or_else(|why| panic!("the agent stub's state at {}: {why}", root.display()));
}

/// Make `verb` at `root` answer could not tell with `error` from its next call
/// on, or answer again where `None` ([`State::untold`]).
pub fn untold(root: &Path, verb: &str, error: Option<&str>) {
    with_state(root, |state| match error {
        Some(error) => state.untold.insert(verb.to_string(), error.to_string()),
        None => state.untold.remove(verb),
    })
    .unwrap_or_else(|why| panic!("the agent stub's state at {}: {why}", root.display()));
}

/// Make every read at `root` follow the panes of the `fleet-tmux-stub` whose
/// state is `host`, or stop where `None` ([`State::host`]).
pub fn follow_host(root: &Path, host: Option<&Path>) {
    with_state(root, |state| state.host = host.map(Path::to_path_buf))
        .unwrap_or_else(|why| panic!("the agent stub's state at {}: {why}", root.display()));
}

/// Every call the stub at `root` answered, in the in-process stub's [`Call`]
/// shape — so an arm written against [`StubAgent::calls`] reads this one the
/// same way.
pub fn calls(root: &Path) -> Vec<Call> {
    logged(root)
        .into_iter()
        .filter_map(|call| {
            let verb = VERBS.into_iter().find(|verb| *verb == call.verb)?;
            let about = match verb {
                StubAgent::LAUNCH => call.request["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                StubAgent::RESUME => call.request["session_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                StubAgent::READ | StubAgent::CONTEXT => {
                    serde_json::from_value::<Seats>(call.request.clone())
                        .map(|asked| super::about(&asked.seats))
                        .unwrap_or_default()
                }
                _ => String::new(),
            };
            Some(Call { verb, about })
        })
        .collect()
}

/// The verbs alone, in order ([`StubAgent::verbs`]).
pub fn verbs(root: &Path) -> Vec<&'static str> {
    calls(root).into_iter().map(|call| call.verb).collect()
}

/// The calls of one verb ([`StubAgent::calls_of`]).
pub fn calls_of(root: &Path, verb: &str) -> Vec<Call> {
    calls(root)
        .into_iter()
        .filter(|call| call.verb == verb)
        .collect()
}

/// Every launch's whole request ([`StubAgent::starts`]).
pub fn starts(root: &Path) -> Vec<Launch> {
    logged(root)
        .into_iter()
        .filter(|call| call.verb == StubAgent::LAUNCH)
        .map(|call| {
            serde_json::from_value(call.request)
                .unwrap_or_else(|why| panic!("a logged launch reads as one: {why}"))
        })
        .collect()
}

/// Every call as the log holds it, the request whole.
pub fn logged(root: &Path) -> Vec<Logged> {
    load(root)
        .unwrap_or_else(|why| panic!("the agent stub's state at {}: {why}", root.display()))
        .calls
}

/// The fake the root's state file describes, or the default where there is
/// none.
pub fn load(root: &Path) -> Result<State, String> {
    let path = root.join(STATE_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|why| {
            format!(
                "{} does not read as the agent stub's state: {why}",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(format!("{} could not be read: {e}", path.display())),
    }
}

/// `act` on the root's state, under its lock, and the state written back after
/// it — made, from the default, where there is none.
pub fn with_state<T>(root: &Path, act: impl FnOnce(&mut State) -> T) -> Result<T, String> {
    let dir = root.join(STATE_FILE);
    let dir = dir.parent().expect("the state file sits in a directory");
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    let _held = locked(root)?;
    let mut state = load(root)?;
    let answer = act(&mut state);
    let text = serde_json::to_vec_pretty(&state).expect("the state is JSON");
    let path = root.join(STATE_FILE);
    fleet_core::fs::write_atomic(&path, &text)
        .map_err(|e| format!("{} could not be written: {e}", path.display()))?;
    Ok(answer)
}

/// The root's lock, held until the file is dropped, or why it could not be
/// taken inside [`LOCK_WAIT`].
///
/// AN OS LOCK AND NOT A LOCKFILE'S PRESENCE: the kernel lets it go when the
/// holder dies, so a call killed at the agent's bound leaves no lock behind.
fn locked(root: &Path) -> Result<File, String> {
    let path = root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{} could not be opened: {e}", path.display()))?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(format!(
                    "the agent stub at {} was held by another call for {}s",
                    root.display(),
                    LOCK_WAIT.as_secs()
                ))
            }
            Err(TryLockError::Error(e)) => {
                return Err(format!("{} could not be locked: {e}", path.display()))
            }
        }
    }
}
