//! The agent contract as checks, one function each, that any [`Agent`] is
//! asked: what it declares, what a launch answers and writes, what it answers
//! for a session nobody has and for no seats at all, the recorded cases it
//! ships, the exits it owes — and, under `--live`, whether a session it
//! launches comes up, takes a typed turn, ends and comes back.
//!
//! IN THE LIBRARY AND NOT IN A SUITE, as the store's is
//! (`fleet_core::store::conformance`): `fleet agent check` runs [`run`] against
//! an adapter where it is installed, and this crate's suites run it against the
//! stub. In the controller and not in core because the trait is (E10).
//!
//! EACH CHECK ANSWERS AND NONE PANICS. A check passes, fails with a text naming
//! what the adapter answered, or is skipped with why. [`run`] asks every check
//! whatever the one before it answered, so one run names every disagreement —
//! bar the live steps, where a session that never came up leaves nothing for
//! the steps after it to ask about, and each says so as its skip.
//!
//! EVERY ANSWER IS HELD TO THE SCHEMA TOO: each one a check reads passes the
//! agent contract's document ([`fleet_core::agent::schema`]) at its verb's
//! response, through [`fleet_core::schema::check`], beside what the check
//! itself asks of it.
//!
//! SOME CHECKS SPEAK PAST THE TRAIT. An exit, a request exactly as recorded and
//! a variable set for one call are not things a verb of [`Agent`] can carry,
//! so those checks run on the adapter as the executable it is
//! ([`AgentExec::ran`]), which every run is handed.
//!
//! EVERYTHING A CHECK MAKES IS UNDER [`Ctx::scratch`], which the caller owns
//! and removes: each launch's configuration directory and worktree, the live
//! session's, and what the adapter itself keeps beside the root its requests
//! carry.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

pub use fleet_core::adapter::check::Passed;
use fleet_core::adapter::check::{self, ensure, Answer};
use fleet_core::adapter::exec::{self, Exited, Ran, Unrun};
use fleet_core::agent::types::{self, CONTRACT_VERSION};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use fleet_core::store::types::Stamp;
use serde::Serialize;
use serde_json::{Map, Value};

use super::{
    Activity, Agent, AgentError, AgentExec, Argv, Capabilities, Launch, Permissions, Posture,
    RefusalReason, Resume, SeatActivity, SeatRef,
};
use crate::host::{self, Host, HostRead, PaneState};

/// What the checks are run against.
///
/// `exec` is the adapter as the executable it is, for the checks that speak
/// past the trait; `agent` is the same adapter through the trait, which a
/// suite's arm may answer in-process.
/// `fixtures` is the directory of recorded cases, `<verb>/<case>/`, where
/// there is one. `scratch` is a directory the caller made and removes, which
/// every launch writes under — and which the adapter's requests should carry
/// as their root, so what it keeps beside that is in there too. `model` names
/// the model every launch and resume asks for, in place of the declared
/// default. `live` is the host the live steps run a session on, and `None`
/// where they are not run.
pub struct Ctx<'a> {
    pub agent: &'a dyn Agent,
    pub exec: &'a AgentExec,
    pub fixtures: Option<&'a Path>,
    pub scratch: &'a Path,
    pub model: Option<&'a str>,
    pub live: Option<&'a Live<'a>>,
}

/// The live steps' host, the seat they start a session for, and what each
/// step learns for the next: the session's id, when the turn was typed, and
/// whether the session came up and ended.
///
/// THE HOST IS THE CALLER'S: a server of its own, never fleet's `fleet` (E1),
/// which the caller ends when the run is over, whatever the steps left on it.
pub struct Live<'a> {
    host: &'a dyn Host,
    seat: SeatId,
    session: RefCell<Session>,
}

/// What one live step leaves the next.
#[derive(Default)]
struct Session {
    /// What the launch was asked for, which the resume asks for again.
    launched: Option<Launched>,
    /// The session's id, as `read` answered it once the session came up.
    session_id: Option<String>,
    /// The second before the turn was typed, once it read busy and then idle.
    turned: Option<Stamp>,
    /// Whether the ended step ran, and so the session is gone from the host.
    ended: bool,
}

/// The launch the live steps made, as the resume repeats it.
#[derive(Clone)]
struct Launched {
    worktree: PathBuf,
    config_dir: PathBuf,
    model: String,
    posture: Posture,
}

impl<'a> Live<'a> {
    /// The live steps on `host`, for a seat minted for the run.
    pub fn on(host: &'a dyn Host) -> Live<'a> {
        Live {
            host,
            seat: SeatId::mint(),
            session: RefCell::new(Session::default()),
        }
    }

    /// The name the session runs under on the host: the seat's, as every
    /// seat's session is named ([`host::session_for`]).
    pub fn session_name(&self) -> String {
        host::session_for(&self.seat)
    }
}

/// One check: passed or skipped, or the text of what the adapter answered
/// instead.
pub type Check = fn(&Ctx) -> Result<Passed, String>;

/// Every check, by name, in the order [`run`] asks them: what the adapter is,
/// what it launches and resumes, what it reads, its recorded cases, the exits
/// it owes, and then the live steps, which run in this order on one session.
pub const CHECKS: &[(&str, Check)] = &[
    ("version", version),
    ("capabilities", capabilities),
    ("each declared posture launches", each_posture_launches),
    (
        "an undeclared posture is refused unsupported",
        undeclared_posture,
    ),
    ("resume of a session nobody has", resume_of_nobody),
    ("read of no seats", read_of_no_seats),
    ("read answers each fixture", read_fixtures),
    ("context answers each fixture", context_fixtures),
    ("an unknown verb exits 2", unknown_verb),
    ("a later schema_version exits 2", later_version),
    ("live: a launched session comes up idle", live_up),
    ("live: a typed turn reads busy, then idle", live_turn),
    ("live: context counts the turn", live_context),
    ("live: the ended session leaves its pane dead", live_ended),
    (
        "live: a resume comes back idle as the same session",
        live_resumed,
    ),
];

/// Every check against the one adapter, in [`CHECKS`]' order, each answer
/// beside its name. A failure never stops the run.
///
/// A CHECK IS ASKED ONLY AS THE NEXT ANSWER IS READ, so a caller prints each
/// answer as it lands: a live step waits on a session for up to a minute, and
/// a silent run reads as a hung one.
pub fn run<'c>(
    ctx: &'c Ctx<'c>,
) -> impl Iterator<Item = (&'static str, Result<Passed, String>)> + 'c {
    check::run(CHECKS, ctx)
}

/// The session id no resume check's agent has: well-formed, so an adapter
/// that holds ids to a shape reads it, and one no agent mints.
const NOBODY: &str = "00000000-0000-4000-8000-00000000c4ec";

/// A verb the contract does not name.
const NO_VERB: &str = "fleet-check-no-such-verb";

/// How long a live session has to come up, and to come back idle after its
/// turn: the start's own watch is half this (`start_watch_seconds`), and a
/// check that times out on a slow first exec proves nothing about the agent.
const UP_WITHIN: Duration = Duration::from_secs(60);

/// How long a typed turn has to read busy: long past the paste and the
/// separate submit, and far short of any turn's own length.
const BUSY_WITHIN: Duration = Duration::from_secs(10);

/// How long the pane has to read dead after the session is told to end.
const END_WITHIN: Duration = crate::effect::STOP_GRACE;

/// The live turn: one word, which any agent answers in one short reply.
const TURN: &str = "hello";

/// The fixtures' placeholder for the case's own directory.
const FIXTURE: &str = "{fixture}";

/// Why a live step skips a run without `--live`.
const NOT_LIVE: &str = "--live was not given: the live steps start the agent and cost a model turn";

// ---- reading an answer -------------------------------------------------------

/// An agent call's answer, or its refusal as the check's failure, naming the
/// call.
fn answered<T>(call: &str, answer: Result<T, AgentError>) -> Result<T, String> {
    answer.map_err(|e| format!("{call} answered {}", refusal(&e)))
}

/// A refusal, by its kind and its text.
fn refusal(e: &AgentError) -> String {
    match e {
        AgentError::Refused(refused) => {
            format!("Refused {} ({})", refused.reason.word(), refused.message)
        }
        AgentError::Unreadable(why) => format!("Unreadable ({why})"),
    }
}

/// The agent contract's document, generated once a run.
fn document() -> &'static Value {
    static DOCUMENT: OnceLock<Value> = OnceLock::new();
    DOCUMENT.get_or_init(fleet_core::agent::schema::document)
}

/// An answer as JSON, `schema_version` included, held to the verb's response
/// schema.
fn conforms(verb: &str, body: &impl Serialize) -> Result<(), String> {
    let mut value = serde_json::to_value(body).map_err(|e| e.to_string())?;
    if let Value::Object(fields) = &mut value {
        fields.insert(
            String::from("schema_version"),
            Value::from(CONTRACT_VERSION),
        );
    }
    schema_holds(verb, &value)
}

/// A whole answer, as the adapter printed it, held to the verb's response
/// schema.
fn schema_holds(verb: &str, answer: &Value) -> Result<(), String> {
    fleet_core::schema::check(document(), &format!("/verbs/{verb}/response"), answer)
        .map_err(|why| format!("{verb}'s answer does not pass the contract's schema: {why}"))
}

/// What the adapter declares, as it answered it: the checks that launch read
/// the postures and the model off it, and the capabilities check is what
/// holds it to the contract's rules.
fn declared(ctx: &Ctx) -> Result<Capabilities, String> {
    answered("capabilities", ctx.agent.declared())
}

/// The model a launch asks for: the one the run names, else the declared
/// default.
fn model(ctx: &Ctx, declared: &Capabilities) -> String {
    ctx.model
        .map(str::to_string)
        .unwrap_or_else(|| declared.default_model.clone())
}

/// A launch's answer held to what every launch answers: a program to run.
fn runs_something(call: &str, argv: &Argv) -> Result<(), String> {
    ensure(
        argv.argv
            .first()
            .is_some_and(|program| !program.trim().is_empty()),
        || {
            format!(
                "{call} answered the argv {:?}, which runs nothing — a session's command \
                 starts with its program",
                argv.argv
            )
        },
    )
}

// ---- what the adapter is -------------------------------------------------------

fn version(ctx: &Ctx) -> Answer {
    let version = answered("version", ctx.agent.version())?;
    conforms("version", &version)?;
    ensure(!version.name.trim().is_empty(), || {
        String::from("version answered a blank name — it names the agent the adapter drives")
    })?;
    Ok(Passed::Pass)
}

/// The declaration as the contract's rules hold it
/// ([`Capabilities::validate`]), and two more the contract's reader does not
/// hold it to (fleet-14p8.2): a default model that is not blank, since a seat
/// that names none is launched on it, and a gate keyed only by postures the
/// adapter takes.
fn capabilities(ctx: &Ctx) -> Answer {
    let declared = declared(ctx)?;
    conforms("capabilities", &declared)?;
    declared.validate()?;
    ensure(!declared.default_model.trim().is_empty(), || {
        String::from(
            "default_model is blank — a seat whose policy names no model is launched on it",
        )
    })?;
    let outside: Vec<&str> = declared
        .posture_models
        .keys()
        .filter(|posture| !declared.postures.contains(posture))
        .map(|posture| posture.word())
        .collect();
    ensure(outside.is_empty(), || {
        format!(
            "posture_models keys {}, which postures does not declare — a gate on a posture \
             the agent does not take holds nothing",
            outside.join(", ")
        )
    })?;
    Ok(Passed::Pass)
}

// ---- launching and resuming ----------------------------------------------------

/// The configuration directory and worktree the offline launches name, made
/// under the scratch dir.
fn offline_dirs(ctx: &Ctx) -> Result<(PathBuf, PathBuf), String> {
    let config_dir = ctx.scratch.join("offline/config");
    let worktree = ctx.scratch.join("offline/worktree");
    for dir in [&config_dir, &worktree] {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    }
    Ok((config_dir, worktree))
}

/// The launch the checks ask for: a seat of their own, its name, the model,
/// the posture, the declared first turn with the name filled in, and fleet's
/// own variables for the seat.
fn a_launch(
    seat: SeatId,
    worktree: &Path,
    config_dir: &Path,
    model: String,
    posture: Posture,
    declared: &Capabilities,
) -> Launch {
    let name = format!("fleet-check-{}", seat.short());
    Launch {
        seat,
        worktree: worktree.display().to_string(),
        first_turn: declared.first_turn.replace("{seat}", &name),
        name,
        model,
        posture,
        config_dir: Some(config_dir.display().to_string()),
        env: super::seat_environment(&Actor::seat(seat).to_string()),
        permissions: Permissions::default(),
    }
}

/// Every declared posture launches: an argv that runs something, and nothing
/// written outside the request's configuration directory and worktree (E8) —
/// the scratch dir walked before each launch and after it.
fn each_posture_launches(ctx: &Ctx) -> Answer {
    let declared = declared(ctx)?;
    ensure(!declared.postures.is_empty(), || {
        String::from("the capabilities declare no posture to launch")
    })?;
    let (config_dir, worktree) = offline_dirs(ctx)?;
    for posture in &declared.postures {
        let call = format!("launch as {}", posture.word());
        let before = walked(ctx.scratch);
        let asked = a_launch(
            SeatId::mint(),
            &worktree,
            &config_dir,
            model(ctx, &declared),
            *posture,
            &declared,
        );
        let argv = answered(&call, ctx.agent.launch(&asked))?;
        conforms("launch", &argv)?;
        runs_something(&call, &argv)?;
        let stray = strayed(&before, &walked(ctx.scratch), &[&config_dir, &worktree]);
        ensure(stray.is_empty(), || {
            format!(
                "{call} wrote outside its config_dir and worktree: {}",
                stray.join(", ")
            )
        })?;
    }
    Ok(Passed::Pass)
}

/// A posture the capabilities do not declare is refused `unsupported`.
fn undeclared_posture(ctx: &Ctx) -> Answer {
    let declared = declared(ctx)?;
    let Some(posture) = Posture::ALL
        .into_iter()
        .find(|posture| !declared.postures.contains(posture))
    else {
        return Ok(Passed::Skip(String::from(
            "every posture is declared, so there is none to refuse",
        )));
    };
    let (config_dir, worktree) = offline_dirs(ctx)?;
    let asked = a_launch(
        SeatId::mint(),
        &worktree,
        &config_dir,
        model(ctx, &declared),
        posture,
        &declared,
    );
    match ctx.agent.launch(&asked) {
        Err(AgentError::Refused(refused)) if refused.reason == RefusalReason::Unsupported => {
            Ok(Passed::Pass)
        }
        Ok(argv) => Err(format!(
            "launch as {} answered the argv {:?}, and a posture the capabilities do not \
             declare is refused unsupported",
            posture.word(),
            argv.argv
        )),
        Err(e) => Err(format!(
            "launch as {} answered {}, and a posture the capabilities do not declare is \
             refused unsupported",
            posture.word(),
            refusal(&e)
        )),
    }
}

/// A resume of a session the agent never had answers an argv — the adapter
/// cannot tell before the agent runs — or is refused `missing`; never could
/// not tell.
fn resume_of_nobody(ctx: &Ctx) -> Answer {
    let declared = declared(ctx)?;
    let Some(posture) = declared.postures.first().copied() else {
        return Err(String::from(
            "the capabilities declare no posture to resume under",
        ));
    };
    let (config_dir, worktree) = offline_dirs(ctx)?;
    let asked = Resume {
        session_id: NOBODY.to_string(),
        worktree: worktree.display().to_string(),
        config_dir: Some(config_dir.display().to_string()),
        model: model(ctx, &declared),
        posture,
    };
    let call = format!("resume of {NOBODY}");
    match ctx.agent.resume(&asked) {
        Ok(argv) => {
            conforms("resume", &argv)?;
            runs_something(&call, &argv)?;
            Ok(Passed::Pass)
        }
        Err(AgentError::Refused(refused)) if refused.reason == RefusalReason::Missing => {
            Ok(Passed::Pass)
        }
        Err(e) => Err(format!(
            "{call} answered {}, and a session nobody has is resumed or refused missing",
            refusal(&e)
        )),
    }
}

/// A read about no seats answers no rows.
fn read_of_no_seats(ctx: &Ctx) -> Answer {
    let rows = answered("read of no seats", ctx.agent.read(&[]))?;
    conforms("read", &serde_json::json!({ "seats": rows }))?;
    ensure(rows.is_empty(), || {
        format!("read of no seats answered {} rows: {rows:?}", rows.len())
    })?;
    Ok(Passed::Pass)
}

// ---- where a launch wrote ------------------------------------------------------

/// A file's length and last write, or `None` for a directory.
type Seen = Option<(u64, Option<SystemTime>)>;

/// Everything under `dir`, by path: each file's length and last write, and
/// each directory. Links are read as themselves and never followed.
fn walked(dir: &Path) -> BTreeMap<PathBuf, Seen> {
    let mut seen = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                seen.insert(path.clone(), None);
                stack.push(path);
            } else {
                seen.insert(path, Some((meta.len(), meta.modified().ok())));
            }
        }
    }
    seen
}

/// Every path made, changed or removed between two walks that is not inside
/// one of `allowed`, each named with what happened to it.
fn strayed(
    before: &BTreeMap<PathBuf, Seen>,
    after: &BTreeMap<PathBuf, Seen>,
    allowed: &[&Path],
) -> Vec<String> {
    let inside = |path: &Path| allowed.iter().any(|dir| path.starts_with(dir));
    let mut stray = Vec::new();
    for (path, now) in after {
        if inside(path) {
            continue;
        }
        match before.get(path) {
            None => stray.push(format!("{} (made)", path.display())),
            Some(then) if then != now && now.is_some() => {
                stray.push(format!("{} (changed)", path.display()))
            }
            Some(_) => {}
        }
    }
    for path in before.keys() {
        if !inside(path) && !after.contains_key(path) {
            stray.push(format!("{} (removed)", path.display()));
        }
    }
    stray
}

// ---- the recorded cases --------------------------------------------------------

fn read_fixtures(ctx: &Ctx) -> Answer {
    replayed(ctx, "read")
}

/// Asked only of an adapter that declares `context`: one that does not is
/// never asked it.
fn context_fixtures(ctx: &Ctx) -> Answer {
    if !declared(ctx)?.context {
        return Ok(Passed::Skip(String::from(
            "the capabilities declare no context",
        )));
    }
    replayed(ctx, "context")
}

/// Every case under `<fixtures>/<verb>/`, each replayed and compared; the
/// failures named by case, every one of them.
fn replayed(ctx: &Ctx, verb: &str) -> Answer {
    let Some(fixtures) = ctx.fixtures else {
        return Ok(Passed::Skip(String::from(
            "no fixtures: the adapter ships no fixtures/ beside an adapter.toml, and \
             --fixtures names none",
        )));
    };
    let exec = ctx.exec;
    let dir = fixtures.join(verb);
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    cases.sort();
    if cases.is_empty() {
        return Ok(Passed::Skip(format!(
            "{} holds no case under {verb}/",
            fixtures.display()
        )));
    }
    let failed: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            replay(exec, verb, case).err().map(|why| {
                let name = case.file_name().unwrap_or_default().to_string_lossy();
                format!("{name}: {why}")
            })
        })
        .collect();
    if failed.is_empty() {
        Ok(Passed::Pass)
    } else {
        Err(failed.join("; "))
    }
}

/// One case: `request.json` sent as recorded, `{fixture}` in every string of it
/// and of `env.json` filled with the case's own directory, under `env.json`'s
/// variables for this call alone; the answer passes when it equals
/// `answer.json`, its `schema_version` taken out of both.
///
/// A `last_write` IS COMPARED BY PRESENCE AND NOT BY VALUE. It is the time a
/// recorded file was last written, which a checkout does not keep, so where
/// `answer.json` carries one the answer carries one — a stamp, which the schema
/// holds it to — and where it carries none, neither does the answer.
fn replay(exec: &AgentExec, verb: &str, case: &Path) -> Result<(), String> {
    let request = filled(recorded(case, "request.json")?, case);
    let env = match case.join("env.json").is_file() {
        false => Vec::new(),
        true => match filled(recorded(case, "env.json")?, case) {
            Value::Object(vars) => vars
                .into_iter()
                .map(|(key, value)| match value {
                    Value::String(value) => Ok((key, value)),
                    other => Err(format!("env.json sets {key} to {other}, which is not text")),
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => return Err(format!("env.json is {other}, which is not an object")),
        },
    };
    let wanted = recorded(case, "answer.json")?;
    let ran = exec
        .ran(verb, &request, &env)
        .map_err(|unrun| unran(verb, unrun))?;
    let Exited::Answered(stdout) = &ran.exited else {
        return Err(format!("{verb} {}", exited(&ran)));
    };
    let answer = exec::first_value(stdout)
        .ok_or_else(|| format!("{verb} answered no JSON value: {}", ran.said))?;
    schema_holds(verb, &answer)?;
    match difference(&compared(answer, verb), &compared(wanted, verb), "") {
        None => Ok(()),
        Some(why) => Err(why),
    }
}

/// A case's file, as JSON.
fn recorded(case: &Path, file: &str) -> Result<Value, String> {
    let path = case.join(file);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{file} is not JSON: {e}"))
}

/// `{fixture}` in every string of `value` replaced with the case's directory.
fn filled(value: Value, case: &Path) -> Value {
    let dir = case.display().to_string();
    match value {
        Value::String(text) => Value::String(text.replace(FIXTURE, &dir)),
        Value::Array(items) => Value::Array(items.into_iter().map(|v| filled(v, case)).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, v)| (key, filled(v, case)))
                .collect(),
        ),
        other => other,
    }
}

/// An answer as a case compares it: `schema_version` out, and each row's
/// `last_write` reduced to whether it is there.
fn compared(mut answer: Value, verb: &str) -> Value {
    if let Value::Object(fields) = &mut answer {
        fields.remove("schema_version");
        if verb == "context" {
            if let Some(Value::Array(rows)) = fields.get_mut("seats") {
                for row in rows.iter_mut().filter_map(Value::as_object_mut) {
                    if let Some(written) = row.get_mut("last_write") {
                        *written = Value::from("<a stamp>");
                    }
                }
            }
        }
    }
    answer
}

/// Where `got` first parts from `wanted`, by its path, or `None` where the two
/// are equal.
fn difference(got: &Value, wanted: &Value, at: &str) -> Option<String> {
    let here = || {
        if at.is_empty() {
            String::from("the answer")
        } else {
            at.to_string()
        }
    };
    match (got, wanted) {
        (Value::Object(got), Value::Object(wanted)) => {
            for (key, want) in wanted {
                let path = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                match got.get(key) {
                    None => return Some(format!("{path}: missing, and answer.json holds {want}")),
                    Some(value) => {
                        if let Some(why) = difference(value, want, &path) {
                            return Some(why);
                        }
                    }
                }
            }
            got.iter()
                .find(|(key, _)| !wanted.contains_key(*key))
                .map(|(key, value)| {
                    let path = if at.is_empty() {
                        key.clone()
                    } else {
                        format!("{at}.{key}")
                    };
                    format!("{path}: answered {value}, and answer.json holds none")
                })
        }
        (Value::Array(got), Value::Array(wanted)) => {
            let parted = got
                .iter()
                .zip(wanted)
                .enumerate()
                .find_map(|(n, (value, want))| difference(value, want, &format!("{at}[{n}]")));
            parted.or_else(|| {
                (got.len() != wanted.len()).then(|| {
                    format!(
                        "{}: answered {} rows, and answer.json holds {}",
                        here(),
                        got.len(),
                        wanted.len()
                    )
                })
            })
        }
        (got, wanted) if got == wanted => None,
        (got, wanted) => Some(format!(
            "{}: answered {got}, and answer.json holds {wanted}",
            here()
        )),
    }
}

// ---- the exits -----------------------------------------------------------------

/// A verb the contract does not name exits 2.
fn unknown_verb(ctx: &Ctx) -> Answer {
    let exec = ctx.exec;
    let request = types::request(Map::new(), exec.root());
    usage(exec, NO_VERB, &request, &format!("`{NO_VERB}`"))
}

/// A request at a `schema_version` the adapter does not speak exits 2.
fn later_version(ctx: &Ctx) -> Answer {
    let exec = ctx.exec;
    let request = exec::envelope(Map::new(), exec.root(), CONTRACT_VERSION + 1);
    usage(
        exec,
        "version",
        &request,
        &format!("version at schema_version {}", CONTRACT_VERSION + 1),
    )
}

/// The call exits 2, or the failure naming how it did exit.
fn usage(exec: &AgentExec, verb: &str, request: &Value, call: &str) -> Answer {
    let ran = exec
        .ran(verb, request, &[])
        .map_err(|unrun| unran(call, unrun))?;
    match ran.exited {
        Exited::Usage => Ok(Passed::Pass),
        _ => Err(format!(
            "{call} {}, and the contract's answer is exit 2",
            exited(&ran)
        )),
    }
}

/// How a call that ran exited, in a phrase.
fn exited(ran: &Ran) -> String {
    let code = match &ran.exited {
        Exited::Answered(_) => String::from("exited 0"),
        Exited::Refused(_) => String::from("exited 1"),
        Exited::Usage => String::from("exited 2"),
        Exited::CouldNotTell(_) => String::from("exited 3"),
        Exited::OffTable(code) => format!("exited {code}"),
        Exited::Signalled => String::from("was ended by a signal"),
    };
    if ran.said.is_empty() {
        code
    } else {
        format!("{code}: {}", ran.said)
    }
}

/// A call that never reached its exit.
fn unran(call: &str, unrun: Unrun) -> String {
    match unrun {
        Unrun::CouldNotRun(why) => format!("{call} could not be run: {why}"),
        Unrun::Deadline(why) => format!("{call} {why}"),
    }
}

// ---- the live steps ------------------------------------------------------------

/// The live steps' host and state, or the skip a run without `--live` answers.
fn live_of<'c>(ctx: &'c Ctx) -> Result<&'c Live<'c>, Passed> {
    ctx.live.ok_or_else(|| Passed::Skip(NOT_LIVE.to_string()))
}

/// What one look at the session found.
enum Looked {
    /// The pane is gone from the host, or the host could not be read.
    Nothing(String),
    /// The pane is dead, with the status its process exited with.
    Dead(Option<i32>),
    /// The pane is alive, and `read` answered this about it.
    Read(SeatActivity),
}

/// One look: the host's pane for the seat, and `read`'s answer about it,
/// asked as a poll asks — the pane's pid, the session id where one is known,
/// the directories, and the pane's screen.
fn look(ctx: &Ctx, live: &Live, launched: &Launched, session_id: Option<&str>) -> Looked {
    let name = live.session_name();
    let panes = match live.host.list() {
        HostRead::Readable(panes) => panes,
        HostRead::Unreadable { cause } => return Looked::Nothing(cause),
    };
    let Some(pane) = panes.into_iter().find(|pane| pane.session == name) else {
        return Looked::Nothing(format!("the session {name} is gone from the host"));
    };
    if let PaneState::Dead { status } = pane.state {
        return Looked::Dead(status);
    }
    let asked = SeatRef {
        seat: live.seat,
        session_id: session_id.map(str::to_string),
        pid: pane.pid,
        config_dir: Some(launched.config_dir.display().to_string()),
        worktree: launched.worktree.display().to_string(),
        screen: live.host.capture(&name).ok(),
    };
    match ctx.agent.read(std::slice::from_ref(&asked)) {
        Ok(rows) => {
            if let Err(why) = conforms("read", &serde_json::json!({ "seats": rows })) {
                return Looked::Nothing(why);
            }
            match rows.into_iter().find(|row| row.seat == live.seat) {
                Some(row) => Looked::Read(row),
                None => Looked::Nothing(String::from("read answered no row for the seat")),
            }
        }
        Err(e) => Looked::Nothing(format!("read answered {}", refusal(&e))),
    }
}

/// Look every tick until a reading `wanted` holds of, or `within` passes: the
/// reading, or why not — the pane dead, a blocked reading nothing here will
/// answer, or the time out, naming every activity read on the way.
fn watch(
    ctx: &Ctx,
    live: &Live,
    launched: &Launched,
    session_id: Option<&str>,
    within: Duration,
    wanted: impl Fn(&SeatActivity) -> bool,
) -> Result<SeatActivity, String> {
    let deadline = Instant::now() + within;
    let mut trail: Vec<&'static str> = Vec::new();
    loop {
        let last = match look(ctx, live, launched, session_id) {
            Looked::Read(row) if wanted(&row) => return Ok(row),
            Looked::Read(row) => {
                let word = row.activity.word();
                if trail.last() != Some(&word) {
                    trail.push(word);
                }
                if row.activity == Activity::Blocked {
                    return Err(format!(
                        "read answered blocked{}{}",
                        row.blocked_on
                            .map(|on| format!(" on {}", on.word()))
                            .unwrap_or_default(),
                        row.cause
                            .map(|cause| format!(" ({cause})"))
                            .unwrap_or_default()
                    ));
                }
                row.cause.unwrap_or_default()
            }
            Looked::Dead(status) => {
                return Err(format!(
                    "the pane died, {}, after reading {}",
                    status_of(status),
                    read_so_far(&trail)
                ))
            }
            Looked::Nothing(why) => why,
        };
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "within {}s read answered {}{}",
                within.as_secs(),
                read_so_far(&trail),
                if last.is_empty() {
                    String::new()
                } else {
                    format!(" — last: {last}")
                }
            ));
        }
        std::thread::sleep(crate::effect::WATCH_TICK.min(deadline - now));
    }
}

fn read_so_far(trail: &[&str]) -> String {
    if trail.is_empty() {
        String::from("nothing")
    } else {
        trail.join(" → ")
    }
}

fn status_of(status: Option<i32>) -> String {
    status
        .map(|code| format!("exit status {code}"))
        .unwrap_or_else(|| String::from("ended by a signal"))
}

/// The worktree the live session runs in, made a git repository as every
/// worktree fleet makes is — so a launch may trust it (ruling 13) — and the
/// configuration directory beside it.
fn live_dirs(ctx: &Ctx) -> Result<(PathBuf, PathBuf), String> {
    let config_dir = ctx.scratch.join("live/config");
    let worktree = ctx.scratch.join("live/worktree");
    for dir in [&config_dir, &worktree] {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    }
    let path = crate::platform::child_path(&crate::platform::home_dir());
    let git = fleet_core::process::resolve_on_path(&path, "git")
        .ok_or_else(|| format!("no `git` on the constructed child PATH ({path})"))?;
    let mut init = std::process::Command::new(git);
    init.env_clear()
        .env("PATH", &path)
        .args(["init", "--quiet"])
        .arg(&worktree);
    let out = fleet_core::process::run_bounded(init, Duration::from_secs(20))
        .map_err(|why| format!("git init could not be run: {why}"))?;
    ensure(out.status.success(), || {
        format!(
            "git init {} failed: {}",
            worktree.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )
    })?;
    Ok((config_dir, worktree))
}

/// The session a launch answers, started on the host as the seat's own under
/// fleet's variables for the seat with the adapter's over them, as every start
/// is (E2), comes up: `read` answers it idle, with a session id, within
/// [`UP_WITHIN`].
fn live_up(ctx: &Ctx) -> Answer {
    let live = match live_of(ctx) {
        Ok(live) => live,
        Err(skip) => return Ok(skip),
    };
    let declared = declared(ctx)?;
    let posture = declared
        .postures
        .first()
        .copied()
        .ok_or_else(|| String::from("the capabilities declare no posture to launch"))?;
    let (config_dir, worktree) = live_dirs(ctx)?;
    let launched = Launched {
        worktree,
        config_dir,
        model: model(ctx, &declared),
        posture,
    };
    let asked = a_launch(
        live.seat,
        &launched.worktree,
        &launched.config_dir,
        launched.model.clone(),
        posture,
        &declared,
    );
    let fleets = asked.env.clone();
    let argv = answered("launch", ctx.agent.launch(&asked))?;
    conforms("launch", &argv)?;
    runs_something("launch", &argv)?;
    live.host
        .new_session(
            &live.session_name(),
            &launched.worktree,
            &argv.argv,
            &super::pane_environment(&fleets, &argv),
        )
        .map_err(|cause| format!("the host did not start the session: {cause}"))?;
    live.session.borrow_mut().launched = Some(launched.clone());
    let up = watch(ctx, live, &launched, None, UP_WITHIN, |row| {
        row.activity == Activity::Idle
    })
    .map_err(|why| format!("the launched session did not come up idle: {why}"))?;
    let session_id = up.session_id.ok_or_else(|| {
        String::from("read answered the session idle with no session_id, which a resume needs")
    })?;
    live.session.borrow_mut().session_id = Some(session_id);
    Ok(Passed::Pass)
}

/// The session that came up, or the skip that says it did not.
fn came_up(live: &Live) -> Result<(Launched, String), Passed> {
    let session = live.session.borrow();
    match (&session.launched, &session.session_id) {
        (Some(launched), Some(session_id)) => Ok((launched.clone(), session_id.clone())),
        _ => Err(Passed::Skip(String::from(
            "the launched session did not come up, so there is nothing to ask",
        ))),
    }
}

/// One word, typed as a fleet types a turn — a bracketed paste, then a
/// separate submit (E4) — reads busy within [`BUSY_WITHIN`], and then idle.
fn live_turn(ctx: &Ctx) -> Answer {
    let live = match live_of(ctx) {
        Ok(live) => live,
        Err(skip) => return Ok(skip),
    };
    let (launched, session_id) = match came_up(live) {
        Ok(up) => up,
        Err(skip) => return Ok(skip),
    };
    // The second before the turn: a transcript written for it is written
    // inside this second or after it.
    let before = crate::clock::stamp_of(SystemTime::now()).and_then(|at| Stamp::parse(&at));
    live.host
        .send(&live.session_name(), TURN)
        .map_err(|cause| format!("the turn could not be typed: {cause}"))?;
    watch(
        ctx,
        live,
        &launched,
        Some(&session_id),
        BUSY_WITHIN,
        |row| row.activity == Activity::Busy,
    )
    .map_err(|why| format!("the typed turn did not read busy: {why}"))?;
    watch(ctx, live, &launched, Some(&session_id), UP_WITHIN, |row| {
        row.activity == Activity::Idle
    })
    .map_err(|why| format!("the turn did not come back idle: {why}"))?;
    live.session.borrow_mut().turned = before;
    Ok(Passed::Pass)
}

/// `context`, asked while the session still runs — an agent may rewrite what
/// it reads from as it exits — counts at least the one turn, and names a last
/// write no earlier than the second the turn was typed.
fn live_context(ctx: &Ctx) -> Answer {
    let live = match live_of(ctx) {
        Ok(live) => live,
        Err(skip) => return Ok(skip),
    };
    let (launched, session_id) = match came_up(live) {
        Ok(up) => up,
        Err(skip) => return Ok(skip),
    };
    if !declared(ctx)?.context {
        return Ok(Passed::Skip(String::from(
            "the capabilities declare no context",
        )));
    }
    let Some(turned) = live.session.borrow().turned.clone() else {
        return Ok(Passed::Skip(String::from(
            "the typed turn did not read busy and then idle, so there is none to count",
        )));
    };
    let asked = SeatRef {
        seat: live.seat,
        session_id: Some(session_id),
        pid: None,
        config_dir: Some(launched.config_dir.display().to_string()),
        worktree: launched.worktree.display().to_string(),
        screen: None,
    };
    let rows = answered("context", ctx.agent.context(std::slice::from_ref(&asked)))?;
    conforms("context", &serde_json::json!({ "seats": rows }))?;
    let row = rows
        .into_iter()
        .find(|row| row.seat == live.seat)
        .ok_or_else(|| String::from("context answered no row for the seat"))?;
    ensure(row.turns.is_some_and(|turns| turns >= 1), || {
        format!(
            "context answered turns {}, and the session took one",
            row.turns
                .map_or_else(|| String::from("absent"), |n| n.to_string())
        )
    })?;
    match &row.last_write {
        Some(written) if *written >= turned => Ok(Passed::Pass),
        Some(written) => Err(format!(
            "context answered last_write {written}, before the turn typed at {turned}"
        )),
        None => Err(String::from("context answered no last_write after a turn")),
    }
}

/// The session told to end as fleet ends one — an interrupt, and a second
/// one close behind it, since an agent may take the first as asking whether
/// to leave — leaves its pane dead, keeping how its process ended (ruling 3):
/// the host's reading, which no adapter answers. The dead pane is cleared
/// after, as a revive clears it.
fn live_ended(ctx: &Ctx) -> Answer {
    let live = match live_of(ctx) {
        Ok(live) => live,
        Err(skip) => return Ok(skip),
    };
    if let Err(skip) = came_up(live) {
        return Ok(skip);
    }
    let name = live.session_name();
    let _ = live.host.keys(&name, &["C-c"]);
    std::thread::sleep(host::SUBMIT_GAP);
    let _ = live.host.keys(&name, &["C-c"]);
    let deadline = Instant::now() + END_WITHIN;
    let ended = loop {
        if let HostRead::Readable(panes) = live.host.list() {
            match panes.into_iter().find(|pane| pane.session == name) {
                None => {
                    break Err(String::from(
                        "the session is gone from the host, and a pane that ends is kept dead",
                    ))
                }
                Some(pane) => {
                    if let PaneState::Dead { .. } = pane.state {
                        break Ok(());
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            break Err(format!(
                "the pane was still alive {}s after two interrupts",
                END_WITHIN.as_secs()
            ));
        }
        std::thread::sleep(crate::effect::WATCH_TICK);
    };
    let cleared = live
        .host
        .kill(&name)
        .map_err(|cause| format!("the ended session could not be cleared: {cause}"));
    live.session.borrow_mut().ended = true;
    ended.and(cleared).map(|()| Passed::Pass)
}

/// The resume of the session's full id, with the launch's model and posture,
/// started on the host as the launch was, comes back idle as the SAME session
/// — any other id is a fork (E3).
fn live_resumed(ctx: &Ctx) -> Answer {
    let live = match live_of(ctx) {
        Ok(live) => live,
        Err(skip) => return Ok(skip),
    };
    let (launched, session_id) = match came_up(live) {
        Ok(up) => up,
        Err(skip) => return Ok(skip),
    };
    if !live.session.borrow().ended {
        return Ok(Passed::Skip(String::from(
            "the session was not ended, so there is none to resume",
        )));
    }
    let asked = Resume {
        session_id: session_id.clone(),
        worktree: launched.worktree.display().to_string(),
        config_dir: Some(launched.config_dir.display().to_string()),
        model: launched.model.clone(),
        posture: launched.posture,
    };
    let argv = answered("resume", ctx.agent.resume(&asked))?;
    conforms("resume", &argv)?;
    runs_something("resume", &argv)?;
    let fleets = super::seat_environment(&Actor::seat(live.seat).to_string());
    let name = live.session_name();
    let _ = live.host.kill(&name);
    live.host
        .new_session(
            &name,
            &launched.worktree,
            &argv.argv,
            &super::pane_environment(&fleets, &argv),
        )
        .map_err(|cause| format!("the host did not start the resumed session: {cause}"))?;
    let back = watch(ctx, live, &launched, Some(&session_id), UP_WITHIN, |row| {
        row.activity == Activity::Idle
    })
    .map_err(|why| format!("the resumed session did not come back idle: {why}"))?;
    match back.session_id {
        Some(found) if found == session_id => Ok(Passed::Pass),
        Some(found) => Err(format!(
            "the resume of {session_id} came back as {found}, a fork and not the session it \
             resumed"
        )),
        None => Err(String::from(
            "read answered the resumed session idle with no session_id",
        )),
    }
}

/// The table against the in-process stub, and against the stub bent one verb
/// at a time: each check that holds an answer to a rule fails an adapter that
/// breaks it, naming what it answered. The executable's own checks — the
/// fixtures, the exits, the live steps — are the cli suite's
/// (`cli/tests/agent_check.rs`), which runs `fleet-agent-stub`.
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::adapter::Refusal;
    use crate::test_support::{Answers, Declined, StubAgent};

    /// A scratch dir of the arm's own, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Scratch {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "fleet-agent-conformance-{label}-{}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the scratch dir is made");
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A launch answered otherwise.
    type Launched = Box<dyn Fn(&Launch) -> Result<Argv, AgentError>>;

    /// The stub, with one verb answered otherwise where the arm says.
    struct Bent {
        inner: StubAgent,
        launch: Option<Launched>,
        read: Option<Vec<SeatActivity>>,
    }

    impl Bent {
        fn over(answers: Answers) -> Bent {
            Bent {
                inner: StubAgent::answering(answers),
                launch: None,
                read: None,
            }
        }
    }

    impl Agent for Bent {
        fn capabilities(&self) -> Result<Capabilities, AgentError> {
            self.inner.capabilities()
        }
        fn version(&self) -> Result<super::super::Version, AgentError> {
            self.inner.version()
        }
        fn launch(&self, launch: &Launch) -> Result<Argv, AgentError> {
            match &self.launch {
                Some(bent) => bent(launch),
                None => self.inner.launch(launch),
            }
        }
        fn resume(&self, resume: &Resume) -> Result<Argv, AgentError> {
            self.inner.resume(resume)
        }
        fn read(&self, seats: &[SeatRef]) -> Result<Vec<SeatActivity>, AgentError> {
            match &self.read {
                Some(rows) => Ok(rows.clone()),
                None => self.inner.read(seats),
            }
        }
        fn context(&self, seats: &[SeatRef]) -> Result<Vec<super::super::SeatContext>, AgentError> {
            self.inner.context(seats)
        }
    }

    /// An adapter executable at `<scratch>/exits-two` that exits 2 on every
    /// call: what the checks that speak past the trait are handed beside the
    /// in-process stub.
    fn exits_two(scratch: &Scratch) -> AgentExec {
        use std::os::unix::fs::PermissionsExt;
        let bin = scratch.0.join("exits-two");
        std::fs::write(&bin, "#!/bin/sh\nexit 2\n").expect("the executable is written");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("the executable is made executable");
        AgentExec::at(&bin, &scratch.0)
    }

    /// One check, by name, against `agent`, run in `scratch`.
    fn asked(agent: &dyn Agent, scratch: &Scratch, name: &str) -> Answer {
        let exec = exits_two(scratch);
        let ctx = Ctx {
            agent,
            exec: &exec,
            fixtures: None,
            scratch: &scratch.0,
            model: None,
            live: None,
        };
        let (_, check) = CHECKS
            .iter()
            .find(|(named, _)| *named == name)
            .unwrap_or_else(|| panic!("`{name}` is a check"));
        check(&ctx)
    }

    /// The failure a check answered, or a panic naming what it answered.
    fn failure(answer: Answer) -> String {
        answer.expect_err("the check failed")
    }

    /// The whole table on the in-process stub: nothing fails; the live steps
    /// are skipped, saying why, and the two exit checks pass on an executable
    /// that exits 2.
    #[test]
    fn the_in_process_stub_passes_every_check_it_is_asked() {
        let scratch = Scratch::new("stub");
        let agent = StubAgent::new();
        let exec = exits_two(&scratch);
        let ctx = Ctx {
            agent: &agent,
            exec: &exec,
            fixtures: None,
            scratch: &scratch.0,
            model: None,
            live: None,
        };
        let answers: Vec<_> = run(&ctx).collect();
        assert_eq!(answers.len(), CHECKS.len());
        for (name, answer) in answers {
            match answer {
                Ok(Passed::Pass) => {}
                Ok(Passed::Skip(why)) => assert!(
                    name.starts_with("live: ")
                        || name == "an undeclared posture is refused unsupported"
                        || why.starts_with("no fixtures"),
                    "`{name}` is skipped only where it cannot be asked: {why}"
                ),
                Err(why) => panic!("`{name}` failed: {why}"),
            }
        }
    }

    /// A blank default model fails the capabilities, naming the field
    /// (fleet-14p8.2's hand-off).
    #[test]
    fn a_blank_default_model_fails_the_capabilities() {
        let scratch = Scratch::new("blank-model");
        let mut answers = Answers::default();
        answers.capabilities.default_model = String::from("  ");
        let why = failure(asked(
            &StubAgent::answering(answers),
            &scratch,
            "capabilities",
        ));
        assert!(why.contains("default_model is blank"), "{why}");
    }

    /// A gate keyed by a posture the adapter does not take fails the
    /// capabilities, naming the posture (fleet-14p8.2's hand-off).
    #[test]
    fn a_gate_on_an_undeclared_posture_fails_the_capabilities() {
        let scratch = Scratch::new("gate");
        let mut answers = Answers::default();
        answers.capabilities.postures = vec![Posture::Ask];
        answers.capabilities.posture_models =
            BTreeMap::from([(Posture::Auto, vec![String::from("quill-large")])]);
        let why = failure(asked(
            &StubAgent::answering(answers),
            &scratch,
            "capabilities",
        ));
        assert!(why.contains("posture_models keys auto"), "{why}");
    }

    /// A launch whose argv runs nothing fails the launch check (fleet-14p8.2's
    /// hand-off).
    #[test]
    fn a_launch_answering_no_program_fails() {
        let scratch = Scratch::new("no-program");
        let mut agent = Bent::over(Answers::default());
        agent.launch = Some(Box::new(|_| Ok(Argv::default())));
        let why = failure(asked(&agent, &scratch, "each declared posture launches"));
        assert!(why.contains("runs nothing"), "{why}");
    }

    /// A launch that writes beside its configuration directory fails, naming
    /// the file; one that writes inside it and inside the worktree passes (E8).
    #[test]
    fn a_launch_is_held_to_writing_inside_its_config_dir_and_worktree() {
        let scratch = Scratch::new("writes");
        let mut agent = Bent::over(Answers::default());
        agent.launch = Some(Box::new(|launch| {
            let config = PathBuf::from(launch.config_dir.as_deref().unwrap_or_default());
            std::fs::write(config.join("seeded.json"), "{}").expect("written");
            std::fs::write(Path::new(&launch.worktree).join("settings.json"), "{}")
                .expect("written");
            Ok(StubAgent::launched(launch))
        }));
        assert_eq!(
            asked(&agent, &scratch, "each declared posture launches"),
            Ok(Passed::Pass)
        );

        agent.launch = Some(Box::new(|launch| {
            let config = PathBuf::from(launch.config_dir.as_deref().unwrap_or_default());
            let beside = config.parent().expect("the config dir sits in one");
            std::fs::write(beside.join("stray.json"), "{}").expect("written");
            Ok(StubAgent::launched(launch))
        }));
        let why = failure(asked(&agent, &scratch, "each declared posture launches"));
        assert!(
            why.contains("wrote outside") && why.contains("stray.json (made)"),
            "{why}"
        );
    }

    /// A posture the capabilities do not declare, launched, fails unless the
    /// launch is refused unsupported.
    #[test]
    fn an_undeclared_posture_is_refused_unsupported_or_the_check_fails() {
        let scratch = Scratch::new("undeclared");
        let mut answers = Answers::default();
        answers.capabilities.postures = vec![Posture::Ask];
        answers.capabilities.posture_models = BTreeMap::new();
        let mut agent = Bent::over(answers);
        let name = "an undeclared posture is refused unsupported";
        let why = failure(asked(&agent, &scratch, name));
        assert!(why.contains("launch as auto answered the argv"), "{why}");

        agent.launch = Some(Box::new(|launch| {
            if launch.posture == Posture::Ask {
                return Ok(StubAgent::launched(launch));
            }
            Err(AgentError::Refused(Refusal {
                reason: RefusalReason::Unsupported,
                message: String::from("the stub takes no such posture"),
            }))
        }));
        assert_eq!(asked(&agent, &scratch, name), Ok(Passed::Pass));
    }

    /// A resume of a session nobody has passes as an argv or a refusal
    /// missing, and fails as could not tell.
    #[test]
    fn a_resume_of_nobody_is_an_argv_or_missing_and_never_could_not_tell() {
        let scratch = Scratch::new("resume");
        let name = "resume of a session nobody has";
        let refused = Answers {
            resume: Err(Declined::Refused(Refusal {
                reason: RefusalReason::Missing,
                message: String::from("no such session"),
            })),
            ..Answers::default()
        };
        assert_eq!(
            asked(&StubAgent::answering(refused), &scratch, name),
            Ok(Passed::Pass)
        );
        let untold = Answers {
            resume: Err(Declined::Untold(String::from("the index is locked"))),
            ..Answers::default()
        };
        let why = failure(asked(&StubAgent::answering(untold), &scratch, name));
        assert!(why.contains("Unreadable (the index is locked)"), "{why}");
    }

    /// A read about no seats that answers a row fails.
    #[test]
    fn a_read_of_no_seats_answering_a_row_fails() {
        let scratch = Scratch::new("no-seats");
        let mut agent = Bent::over(Answers::default());
        agent.read = Some(vec![SeatActivity {
            seat: SeatId::mint(),
            activity: Activity::Idle,
            blocked_on: None,
            evidence: super::super::Evidence::Typed,
            session_id: None,
            cause: None,
        }]);
        let why = failure(asked(&agent, &scratch, "read of no seats"));
        assert!(why.contains("answered 1 rows"), "{why}");
    }

    /// Context's recorded cases are skipped for an adapter that declares no
    /// context, before anything else is asked.
    #[test]
    fn the_context_cases_are_skipped_where_no_context_is_declared() {
        let scratch = Scratch::new("no-context");
        let mut answers = Answers::default();
        answers.capabilities.context = false;
        assert_eq!(
            asked(
                &StubAgent::answering(answers),
                &scratch,
                "context answers each fixture"
            ),
            Ok(Passed::Skip(String::from(
                "the capabilities declare no context"
            )))
        );
    }

    /// A recorded `last_write` is compared by whether it is there, and every
    /// other field by its value.
    #[test]
    fn a_last_write_is_compared_by_presence() {
        let got = serde_json::json!({"schema_version": 1, "seats": [
            {"seat": "a", "turns": 1, "last_write": "2026-09-26T10:00:00Z"}]});
        let wanted = serde_json::json!({"seats": [
            {"seat": "a", "turns": 1, "last_write": "2026-09-20T09:00:00Z"}]});
        assert_eq!(
            difference(
                &compared(got.clone(), "context"),
                &compared(wanted, "context"),
                ""
            ),
            None
        );
        let none = serde_json::json!({"seats": [{"seat": "a", "turns": 1}]});
        assert_eq!(
            difference(&compared(got, "context"), &compared(none, "context"), ""),
            Some(String::from(
                "seats[0].last_write: answered \"<a stamp>\", and answer.json holds none"
            ))
        );
    }

    /// `{fixture}` is filled in every string, however deep, and nowhere else.
    #[test]
    fn the_fixture_placeholder_is_filled_in_every_string() {
        let request = serde_json::json!({"root": "{fixture}", "seats": [
            {"config_dir": "{fixture}/config", "pid": 7}], "n": 1});
        assert_eq!(
            filled(request, Path::new("/cases/read/idle")),
            serde_json::json!({"root": "/cases/read/idle", "seats": [
                {"config_dir": "/cases/read/idle/config", "pid": 7}], "n": 1})
        );
    }

    #[test]
    fn a_difference_is_named_by_its_path() {
        let got = serde_json::json!({"seats": [{"seat": "a", "activity": "idle"}]});
        let wanted = serde_json::json!({"seats": [{"seat": "a", "activity": "busy"}]});
        assert_eq!(
            difference(&got, &wanted, ""),
            Some(String::from(
                "seats[0].activity: answered \"idle\", and answer.json holds \"busy\""
            ))
        );
        assert_eq!(difference(&got, &got, ""), None);
    }
}
