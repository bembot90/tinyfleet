//! The agent seam. Everything the controller learns about a seat's agent goes
//! through one trait, and the trait is the agent contract's six verbs
//! (`docs/agent.md`) over the contract's own types (`fleet_core::agent`): the
//! in-process Claude Code adapter answers them today, and an adapter that is an
//! executable answers the same six.
//!
//! What the agent IS — [`Agent::capabilities`] and [`Agent::version`]; what a
//! session starts or comes back under — [`Agent::launch`] and
//! [`Agent::resume`]; what each seat's agent is DOING — [`Agent::read`]; and how
//! full its window is — [`Agent::context`], asked only of an agent that declares
//! it. Nothing else crosses: no listing row, no transcript and no short id
//! leaves an adapter (reviewer call 2026-09-25, E6).
//!
//! A turn for a live seat is none of them: it is typed into the seat's pane by
//! core (`crate::effect::type_turn`), and `read` is what says whether it was
//! taken. Whether a session is THERE is not a verb here at all: presence is the
//! host's reading (`crate::host`, ruling 3). Nor is ending one: a session is
//! stopped on the host, by its seat (`crate::effect::stop_session`).
//!
//! A START IS TWO HALVES AND ONLY ONE OF THEM IS HERE (ruling 2). The adapter
//! answers WHAT to run — the argv and the environment, an [`Argv`] — and core
//! runs it, as a session on the host, and believes it only when `read` finds
//! the pane's own process (`crate::effect::start_once`). No adapter touches the
//! host.
//!
//! EVERY CALLER OPENS THE AGENT THROUGH [`open`], so which adapter answers, and
//! whether it may issue effects at all, is decided in one place.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub use fleet_core::agent::types::{
    Activity, Argv, BlockedOn, Capabilities, Evidence, Launch, Permissions, Posture, Refusal,
    RefusalReason, Resume, SeatActivity, SeatContext, SeatRef, Version,
};

pub mod claude_code;

/// Why a verb has no answer: the adapter REFUSED the act as asked, naming its
/// reason (exit 1's `refused`), or nobody could tell (exit 3, or no answer at
/// all), carrying why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    Refused(Refusal),
    Unreadable(String),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::Refused(refusal) => write!(
                f,
                "the agent refused ({}): {}",
                match refusal.reason {
                    RefusalReason::Unsupported => "unsupported",
                    RefusalReason::Missing => "missing",
                },
                refusal.message
            ),
            AgentError::Unreadable(cause) => f.write_str(cause),
        }
    }
}

pub trait Agent {
    /// What this adapter declares about its agent: the postures it takes, the
    /// model and the first turn a seat that names none starts with, whether it
    /// answers [`Agent::context`], the releases it was measured against, and
    /// the models a posture is held to (the D3 gate).
    fn capabilities(&self) -> Result<Capabilities, AgentError>;

    /// Which agent this adapter drives, and at which version of itself **this
    /// call**: a version read once at startup and republished advertises the
    /// boot version for as long as the controller lives.
    fn version(&self) -> Result<Version, AgentError>;

    /// What to run to bring a fresh session up in the seat's worktree.
    ///
    /// It RUNS NOTHING: the session is started by core, on the host, from the
    /// value this answers. It may WRITE inside the request's configuration
    /// directory and inside the seat's worktree, and nowhere else (reviewer
    /// call 2026-09-25, E8) — its agent's scoped configuration and whatever
    /// rendering of the request's permissions its agent reads — and an error
    /// is a start that must not be attempted, carrying why.
    fn launch(&self, launch: &Launch) -> Result<Argv, AgentError>;

    /// What to run to bring a seat's ENDED session back, context intact, as a
    /// fresh session on the host: the resume of the FULL session id the table
    /// recorded, carrying the start's own flags (reviewer call 2026-09-25, E2,
    /// E3).
    ///
    /// It runs nothing, exactly as [`Agent::launch`] runs nothing, and it is
    /// believed the same way: core starts it and asks [`Agent::read`] about the
    /// pane — which must answer that session's id, because a session under any
    /// other id is a fork and not the one this meant to continue.
    fn resume(&self, resume: &Resume) -> Result<Argv, AgentError>;

    /// What each seat's agent is doing, every seat in one call, one reading per
    /// seat in the order asked.
    ///
    /// A seat is found by its `session_id`, and else by its `pid`, the pane's
    /// process — even where the session id names nothing the agent has — and
    /// NEVER by its worktree (CORRECTIONS AT REVIEW, 2026-09-25). An error is
    /// the whole call unanswered; one seat the adapter could not read is that
    /// seat's own `unknown`, and every other seat is still read.
    fn read(&self, seats: &[SeatRef]) -> Result<Vec<SeatActivity>, AgentError>;

    /// How much of its window each seat's session has used, how many turns it
    /// took and when it last wrote — every seat in one call, asked only of an
    /// adapter whose [`Agent::capabilities`] declare it. A reading the adapter
    /// cannot give is absent from its row, never a zero.
    fn context(&self, seats: &[SeatRef]) -> Result<Vec<SeatContext>, AgentError>;

    /// The keys that answer the question a starting session's screen is
    /// stopped at, where this agent knows the question and a start may answer
    /// it; `None` for a screen it does not recognise.
    ///
    /// NOT ONE OF THE CONTRACT'S VERBS: the in-process adapter's alone, kept
    /// from flight 11 as the fallback ruling 13 allows should a seeded trust
    /// acceptance not skip the agent's workspace-trust question. An adapter
    /// that is an executable answers `None`, and its launch's seed is the
    /// path. The rule is the adapter's because the words on the screen are the
    /// agent's (ruling 2); core only captures and types.
    fn trust_keys(&self, _screen: &str) -> Option<Vec<String>> {
        None
    }
}

// ---- opening the agent ------------------------------------------------------

/// What a caller knows when it opens the agent: where this machine keeps its
/// state, and what the in-process adapter is handed rather than reading for
/// itself.
pub struct Opening<'a> {
    /// The home the adapter's own configuration is found under when nothing
    /// configures one.
    pub home: &'a Path,
    /// The plugin root the in-process adapter loads into every session it
    /// launches or resumes: `[controller] plugin_dir` today, the adapter's own
    /// pack path once fleet-x93d.2 deletes the key (reviewer call 2026-09-25,
    /// E8). Never a field of a request.
    pub plugin_dir: Option<PathBuf>,
    /// The template a launch renders the request's permissions into, the pack
    /// layers' `overlay/per-provider/claude/permissions.json` until fleet-jymr.5
    /// moves it into the claude-code pack. `None` is an adapter whose launches
    /// write no permission document at all.
    pub permissions: Option<String>,
}

/// The agent a caller opened, and why it may issue no effect where it may
/// not.
pub struct Opened {
    /// The name `[agent] adapter` will call this adapter by.
    pub name: String,
    pub agent: Box<dyn Agent>,
    /// Why no effect may be issued through this agent — nothing it could launch
    /// resolved — or `None` for an agent that can. Reads are answered either
    /// way: a loop that cannot start a session still observes and publishes.
    pub effects_off: Option<String>,
    /// What reads for sessions Claude Code's background daemon still hosts —
    /// the upgrade refusal (ruling 10) — where this adapter's agent has such a
    /// daemon, and `None` where it has none. Beside the agent and never one of
    /// its verbs: it goes with the in-process adapter in flight 14.
    pub daemon: Option<Box<dyn claude_code::DaemonListing>>,
}

/// The one opener every caller takes: the in-process Claude Code adapter,
/// built from this process's environment, with the binary its effects exec
/// resolved ONCE — the same resolution the gate in [`Opened::effects_off`] is
/// read from, so a caller that gated on one file cannot act through another.
pub fn open(opening: &Opening) -> Result<Opened, String> {
    Ok(claude_code::open(opening))
}

// ---- what fleet sets for a seat's session -----------------------------------

/// The environment a seat's session keeps from this process, beside the
/// constructed `PATH`: four values a shell needs to be one, and nothing else.
/// A variable this list does not name cannot reach a session through this
/// controller.
///
/// `FLEET_BIN` is not here, and passing it through would be wrong twice over: a
/// controller started by a service manager has none to pass, and one started
/// from inside a seat would hand on that seat's binary rather than its own. It
/// is set from this process's own executable instead.
///
/// `FLEET_ACTOR` is not here for the second of those reasons: a controller
/// started from inside a seat would make every session it starts that seat.
/// A start and a resume set it from the seat they are for.
pub const PASSED_THROUGH: [&str; 4] = ["HOME", "USER", "TMPDIR", "LANG"];

/// The variable the plugin's shim runs its binary from, which every seat's
/// session is handed. Spelled here and not taken from the item layer's own
/// constant, because this crate names nothing of the project around it.
pub const FLEET_BIN_VAR: &str = "FLEET_BIN";

/// The variable a started session's verbs read their actor from, set to the
/// seat's own `seat:<id>`.
pub const FLEET_ACTOR_VAR: &str = "FLEET_ACTOR";

/// This process's own executable, as the absolute path the shim requires.
///
/// Whatever the operating system answers, and nothing when it answers nothing
/// or something relative: the shim refuses a relative seam, so handing one over
/// would block every Bash command of the session it reached rather than let the
/// shim look under its own root.
pub fn own_executable() -> Option<PathBuf> {
    std::env::current_exe().ok().filter(|exe| exe.is_absolute())
}

/// The variables fleet sets for a seat's session, whatever agent runs in it
/// (D1, D7): the constructed `PATH`, the four a shell needs, this process's own
/// executable as `FLEET_BIN`, and WHO THE SESSION ACTS AS — `actor`, so its own
/// bare verbs are the seat's.
///
/// NOTHING IS INHERITED (lessons claude-code D1). A service-launched process
/// carries a minimal `PATH`, and a session that inherits it hands the collapsed
/// search path to every tool call it makes, long after the start that caused
/// it; so the `PATH` is the platform's, built off the home, and the process's
/// own contributes nothing. What an agent's adapter adds for its own agent it
/// answers in its [`Argv`]'s environment, and core sets both, exactly.
pub fn seat_environment(actor: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".to_string(),
        crate::platform::child_path(&crate::platform::home_dir()),
    );
    for pass in PASSED_THROUGH {
        if let Ok(value) = std::env::var(pass) {
            env.insert(pass.to_string(), value);
        }
    }
    if let Some(bin) = own_executable() {
        env.insert(FLEET_BIN_VAR.to_string(), bin.display().to_string());
    }
    env.insert(FLEET_ACTOR_VAR.to_string(), actor.to_string());
    env
}

/// The environment a session's pane runs with: fleet's own for the seat, with
/// the adapter's answered variables over it — the pairs the host is handed,
/// and nothing of its own.
pub fn pane_environment(fleet: &BTreeMap<String, String>, argv: &Argv) -> Vec<(String, String)> {
    let mut env = fleet.clone();
    for (key, value) in &argv.env {
        env.insert(key.clone(), value.clone());
    }
    env.into_iter().collect()
}

// ---- reading the answers ------------------------------------------------------

/// Every asked seat's reading, one per seat in the order asked: `read`'s own
/// row for the seat, and `unknown` carrying why where there is none — the
/// whole call unanswered, or an answer that left the seat out. A seat nobody
/// read is a question, never a seat the agent says is idle.
pub fn readings(agent: &dyn Agent, seats: &[SeatRef]) -> Vec<SeatActivity> {
    if seats.is_empty() {
        return Vec::new();
    }
    let answered = agent.read(seats);
    seats
        .iter()
        .map(|asked| match &answered {
            Ok(rows) => rows
                .iter()
                .find(|row| row.seat == asked.seat)
                .cloned()
                .unwrap_or_else(|| unread(asked, "the agent answered no reading for it".into())),
            Err(why) => unread(asked, format!("the agent could not be read: {why}")),
        })
        .collect()
}

fn unread(asked: &SeatRef, cause: String) -> SeatActivity {
    SeatActivity {
        seat: asked.seat,
        activity: Activity::Unknown,
        blocked_on: None,
        evidence: Evidence::Typed,
        session_id: None,
        cause: Some(cause),
    }
}

/// Every asked seat's context, keyed by the seat: `context`'s own rows where
/// the adapter declares the verb and answered, and nothing otherwise — a seat
/// with no row is a reading nobody has, which every reader renders as blind.
pub fn contexts(
    agent: &dyn Agent,
    declared: bool,
    seats: &[SeatRef],
) -> BTreeMap<fleet_core::seat::identity::SeatId, SeatContext> {
    if !declared || seats.is_empty() {
        return BTreeMap::new();
    }
    match agent.context(seats) {
        Ok(rows) => rows.into_iter().map(|row| (row.seat, row)).collect(),
        Err(_) => BTreeMap::new(),
    }
}

/// An activity's own word, as the contract spells it.
pub fn word(activity: Activity) -> &'static str {
    match activity {
        Activity::Starting => "starting",
        Activity::Busy => "busy",
        Activity::Idle => "idle",
        Activity::Blocked => "blocked",
        Activity::Unknown => "unknown",
    }
}

/// A blocked reason's own word, as the contract spells it.
pub fn blocked_word(on: BlockedOn) -> &'static str {
    match on {
        BlockedOn::Permission => "permission",
        BlockedOn::Question => "question",
        BlockedOn::LoggedOut => "logged_out",
        BlockedOn::UsageLimit => "usage_limit",
    }
}

/// What a BLOCKED reading waits on, as a person reads it: the adapter's own
/// sentence where it gave one, else the reason's word, else the bare word
/// `blocked` — read for the activity's PRESENCE and never by matching what it
/// names (lessons claude-code B8), so a wait nobody can name still stops the
/// seat. `None` for a reading that is not blocked.
pub fn waiting_on(reading: &SeatActivity) -> Option<String> {
    (reading.activity == Activity::Blocked).then(|| {
        reading
            .cause
            .clone()
            .or_else(|| reading.blocked_on.map(|on| blocked_word(on).to_string()))
            .unwrap_or_else(|| word(Activity::Blocked).to_string())
    })
}

/// A directory path with any trailing separator removed — the one form both
/// sides of a comparison are put in, so a configured path and a reported one
/// that differ only there are the same directory. The root is left alone,
/// because trimming it away leaves nothing to compare.
pub fn dir_key(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        path
    } else {
        trimmed
    }
}
