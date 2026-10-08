//! The agent seam. Everything the controller learns about a seat's agent goes
//! through one trait, and the trait is the agent contract's six verbs
//! (`docs/agent.md`) over the contract's own types (`fleet_core::agent`),
//! answered by an adapter that is an executable ([`AgentExec`]): the one an
//! installed pack carries under the name `[agent] adapter` writes, or the one
//! at the path it names.
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

use fleet_core::agent::types::TIMEOUT_VAR;
use fleet_core::item::brief::Packs;
use fleet_core::pack;
pub use fleet_core::store::{AdapterSource, PackDirs};

pub mod conformance;
pub mod exec;

pub use exec::AgentExec;

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

    /// The same `capabilities` verb, answered as the adapter gave it and NOT
    /// yet held to [`Capabilities::validate`]: what `fleet doctor` reads,
    /// because an adapter that answered what the contract's rules refuse is a
    /// finding, and one that did not answer is could not tell. Every other
    /// caller asks [`Agent::capabilities`], which an adapter executable holds
    /// to those rules first; the default is that answer.
    fn declared(&self) -> Result<Capabilities, AgentError> {
        self.capabilities()
    }

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
}

// ---- opening the agent ------------------------------------------------------

/// What a caller knows when it opens the agent: the fleet's own file, which
/// names the adapter, where a name is resolved, and where this machine keeps
/// its state.
pub struct Opening<'a> {
    /// The fleet's OWN file, which `[agent] adapter` is read out of: the key is
    /// the fleet's and fleet-wide, never a project's (ruling 11).
    pub policy: &'a toml::Table,
    /// Where the `[agent] adapter` in `policy` was written, which a refusal of
    /// it names: the fleet's file, or a flag a verb carried into it.
    pub source: AdapterSource,
    /// The root every request of an adapter executable carries: the directory
    /// the fleet's own file is in.
    pub root: &'a Path,
    /// Where an adapter's bare name is resolved, or `None` for a caller with no
    /// machine's packs behind it, where a name resolves nowhere —
    /// [`DEFAULT_AGENT_ADAPTER`] among them.
    pub packs: Option<PackDirs<'a>>,
    /// The bound on each call of an adapter executable opened: what
    /// [`TIMEOUT_VAR`] sets, else [`fleet_core::agent::types::AGENT_TIMEOUT`].
    pub timeout: std::time::Duration,
    /// The home the constructed `PATH` a pack's adapter runs on is built off.
    pub home: &'a Path,
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
    /// The same agent as the executable it is: what `fleet agent check` speaks
    /// to past the verbs' own types, for a check about an exit or a recorded
    /// case.
    pub exec: AgentExec,
    /// The directory holding the adapter's own `adapter.toml`, where it has
    /// one: a pack's adapter, or an executable named by path that sits beside
    /// one. What the adapter ships beside it — its `fixtures/` — is read from
    /// here.
    pub dir: Option<PathBuf>,
}

/// The adapter a fleet whose file names none opens, by name through the
/// installed packs like any other, as the store's missing key opens its own
/// default (reviewer call 2026-09-25, E12): the one the claude-code pack
/// carries. It is also the first, and today the only, agent `fleet create`
/// installs a pack for and writes into `[agent] adapter`.
///
/// THE ONE SPELLING of that name in core, cli and controller: the create
/// menu, the file it writes and the suites all read it here.
pub const DEFAULT_AGENT_ADAPTER: &str = "claude-code";

/// The one opener every caller takes: the agent `[agent] adapter` in the
/// fleet's own file names. An absolute path to an executable file is an
/// adapter that answers the contract at that path; a name is the agent adapter
/// the installed packs carry under it; and no key at all is
/// [`DEFAULT_AGENT_ADAPTER`], resolved as a name. Anything else is refused,
/// naming what was written where it was ([`AdapterSource`]), and nothing is
/// run.
///
/// THE DEFAULT'S NAME IS A NAME LIKE ANY OTHER: where no installed pack
/// carries it, it is refused as every name no pack carries is, naming the line
/// that installs the one fleet-packs carries — and so is that name with an
/// underscore for its dash, since there is one spelling (ruling 17). No
/// adapter answers inside this process.
///
/// AN ADAPTER EXECUTABLE'S GATE IS ITS OWN ANSWERS: [`Opened::effects_off`]
/// carries why where its capabilities or its version do not answer, or its
/// version says no agent is installed. A loop still observes through it.
pub fn open(opening: &Opening) -> Result<Opened, String> {
    let named = fleet_core::policy::read("agent", "adapter", opening.policy)
        .map_err(|unlisted| unlisted.to_string())?;
    match named {
        None => by_name(opening, DEFAULT_AGENT_ADAPTER),
        Some(toml::Value::String(path)) if path.starts_with('/') => {
            let adapter = Path::new(path);
            if !fleet_core::process::is_executable_file(adapter) {
                return Err(unopened(opening.source, Unopened::NotExecutable(path)));
            }
            let dir = adapter
                .parent()
                .filter(|dir| dir.join(pack::ADAPTER_MANIFEST).is_file())
                .map(Path::to_path_buf);
            Ok(gated(
                path.clone(),
                AgentExec::at(adapter, opening.root).with_timeout(opening.timeout),
                dir,
            ))
        }
        Some(toml::Value::String(name)) if !name.is_empty() && !name.contains('/') => {
            by_name(opening, name)
        }
        Some(toml::Value::String(other)) => Err(unopened(
            opening.source,
            Unopened::NeitherForm(other.clone()),
        )),
        Some(other) => Err(unopened(
            opening.source,
            Unopened::NeitherForm(other.to_string()),
        )),
    }
}

/// The agent adapter the installed packs carry under `name`: the highest layer
/// holding `adapters/agent/<name>/adapter.toml` carries the adapter WHOLE, so
/// its entry is the file beside that one and never another layer's.
///
/// It runs on the constructed child `PATH` built off [`Opening::home`], with
/// the directory of each runtime that layer runs under put in front where the
/// path misses it, as a store adapter's does.
fn by_name(opening: &Opening, name: &str) -> Result<Opened, String> {
    let Some(installed) = opening.packs else {
        return Err(unopened(opening.source, Unopened::NoPacks(name)));
    };
    // A layering that REFUSES is refused, the default's name included — which
    // adapter the fleet runs is then exactly what cannot be told.
    let packs = Packs::under(installed.packs_dir, installed.defaults_dir)
        .map_err(|stop| unopened(opening.source, Unopened::Layers(name, stop.message)))?;
    let Some((carrier, dir)) = pack::adapter_dir(&packs, pack::AdapterKind::Agent, name) else {
        return Err(unopened(opening.source, Unopened::Nowhere(name)));
    };
    // The manifest is held to the format here, its entry's executable bit
    // included, so an adapter `fleet pack check` refuses is never run.
    let manifest = pack::adapter_manifest(&dir)
        .map_err(|defect| unopened(opening.source, Unopened::Defect(name, defect)))?;
    let entry = dir.join(&manifest.entry);
    let path = fleet_core::runtime::adapter_path(
        carrier,
        &packs.layers,
        &crate::platform::child_path(opening.home),
    )
    .map_err(|stop| unopened(opening.source, Unopened::Unrun(name, stop.message)))?;
    Ok(gated(
        name.to_string(),
        AgentExec::at(&entry, opening.root)
            .with_timeout(opening.timeout)
            .on_path(path),
        Some(dir),
    ))
}

/// An adapter executable opened, and its effects gate read off its own
/// answers: its capabilities, held to the contract, and its version, which
/// names an installed agent.
fn gated(name: String, agent: AgentExec, dir: Option<PathBuf>) -> Opened {
    let effects_off = match agent.capabilities().and_then(|_| agent.version()) {
        Ok(Version {
            version: Some(_), ..
        }) => None,
        Ok(Version {
            name: agent_name,
            version: None,
        }) => Some(format!(
            "{name} answers that no {agent_name} is installed (its version is null), so no \
             session can be started through it"
        )),
        Err(why) => Some(why.to_string()),
    };
    Opened {
        name,
        exec: agent.clone(),
        agent: Box::new(agent),
        effects_off,
        dir,
    }
}

/// The name `[agent] adapter` in `policy` calls the fleet's agent by, as
/// written where it is a string, and [`DEFAULT_AGENT_ADAPTER`] where the file
/// writes none: what a caller whose [`open`] refused still publishes.
pub fn named(policy: &toml::Table) -> String {
    match fleet_core::policy::read("agent", "adapter", policy) {
        Ok(Some(toml::Value::String(name))) => name.clone(),
        _ => DEFAULT_AGENT_ADAPTER.to_string(),
    }
}

/// An agent nothing opened: every verb answers could not tell, carrying why
/// [`open`] refused it. What a loop holds where the fleet's agent does not
/// open, so the loop's own start refuses in its own order — the files it
/// reads first, then the agent — rather than the process exiting at the open.
pub struct Unanswered {
    cause: String,
}

impl Unanswered {
    pub fn new(cause: String) -> Unanswered {
        Unanswered { cause }
    }

    fn refused<T>(&self) -> Result<T, AgentError> {
        Err(AgentError::Unreadable(self.cause.clone()))
    }
}

impl Agent for Unanswered {
    fn capabilities(&self) -> Result<Capabilities, AgentError> {
        self.refused()
    }

    fn version(&self) -> Result<Version, AgentError> {
        self.refused()
    }

    fn launch(&self, _launch: &Launch) -> Result<Argv, AgentError> {
        self.refused()
    }

    fn resume(&self, _resume: &Resume) -> Result<Argv, AgentError> {
        self.refused()
    }

    fn read(&self, _seats: &[SeatRef]) -> Result<Vec<SeatActivity>, AgentError> {
        self.refused()
    }

    fn context(&self, _seats: &[SeatRef]) -> Result<Vec<SeatContext>, AgentError> {
        self.refused()
    }
}

/// Where the pack carrying the agent adapter `name` sits in a repository laid
/// out as fleet-packs is: the source `fleet create` installs it from.
pub fn pack_source(repo: &str, name: &str) -> String {
    format!(
        "{repo}//{}/{}/{name}",
        pack::ADAPTERS,
        pack::AdapterKind::Agent.as_str()
    )
}

/// The line that installs the agent adapter `name` out of `repo` at
/// `version`: what a refusal of a name no installed pack carries names, and
/// what `fleet create` names where it could not install that pack.
pub fn pack_line(repo: &str, name: &str, version: &str) -> String {
    format!(
        "fleet pack add {} --version {version}",
        pack_source(repo, name)
    )
}

/// Why the adapter `[agent] adapter` names is not opened, wherever the setting
/// was written.
enum Unopened<'s> {
    NotExecutable(&'s str),
    NeitherForm(String),
    NoPacks(&'s str),
    Layers(&'s str, String),
    Nowhere(&'s str),
    Defect(&'s str, pack::Defect),
    Unrun(&'s str, String),
}

/// EVERY REFUSAL OF THE SETTING IS WORDED HERE, and nowhere else, so what a
/// refusal says about where the setting came from is said once.
fn unopened(source: AdapterSource, why: Unopened) -> String {
    let written = match source {
        AdapterSource::Setting => "[agent] adapter",
        AdapterSource::Flag => "--adapter",
    };
    match why {
        Unopened::NotExecutable(path) => {
            format!("{written} names `{path}`, which is not an executable file")
        }
        Unopened::NeitherForm(said) => format!(
            "{written} is `{said}` — it is the name of an agent adapter an installed pack \
             carries, or an absolute path to an adapter executable"
        ),
        Unopened::NoPacks(name) => format!(
            "no agent adapter named `{name}` resolves: no packs are installed here to carry one"
        ),
        Unopened::Layers(name, why) => {
            format!("no agent adapter named `{name}` resolves: {why}")
        }
        Unopened::Nowhere(name) => format!(
            "no agent adapter named `{name}` in the installed packs — `{}` installs the one \
             fleet-packs carries",
            pack_line(
                fleet_core::supported::PINNED_PACKS_SOURCE,
                name,
                fleet_core::supported::PINNED_PACKS
            )
        ),
        Unopened::Defect(name, defect) => {
            format!("the agent adapter `{name}` cannot be opened: {defect}")
        }
        Unopened::Unrun(name, why) => {
            format!("the agent adapter `{name}` cannot be opened: {why}")
        }
    }
}

/// What an [`Opening`] borrows about the fleet, owned, for a caller that holds
/// only where the fleet's file and the machine directory are: the file as a
/// table, the directory it is in, and the machine's packs over the binary's
/// defaults.
pub struct Setting {
    pub policy: toml::Table,
    pub root: PathBuf,
    pub packs_dir: PathBuf,
    pub defaults_dir: PathBuf,
}

impl Setting {
    /// The fleet whose own file is `fleet_toml`, on the machine at
    /// `machine_dir`. A file that will not read or parse is an `Err` naming
    /// it: the adapter it names cannot be told.
    pub fn read(fleet_toml: &Path, machine_dir: &Path) -> Result<Setting, String> {
        Ok(Setting::of(
            fleet_core::item::read_table(fleet_toml)?,
            fleet_toml,
            machine_dir,
        ))
    }

    /// The fleet whose own file will sit at `fleet_toml` and name the adapter
    /// `name` in `[agent] adapter`, before that file is written: what `fleet
    /// create` asks the adapter it just installed through, as every later
    /// verb will open it.
    pub fn naming(name: &str, fleet_toml: &Path, machine_dir: &Path) -> Setting {
        let mut agent = toml::Table::new();
        agent.insert("adapter".to_string(), toml::Value::String(name.to_string()));
        let mut policy = toml::Table::new();
        policy.insert("agent".to_string(), toml::Value::Table(agent));
        Setting::of(policy, fleet_toml, machine_dir)
    }

    /// The same over a table the caller already holds.
    pub fn of(policy: toml::Table, fleet_toml: &Path, machine_dir: &Path) -> Setting {
        Setting {
            policy,
            root: fleet_toml
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            packs_dir: machine_dir.join("packs"),
            defaults_dir: machine_dir.join(fleet_core::defaults::DIR),
        }
    }

    /// The opening over this fleet, as `[agent] adapter` in its own file
    /// names the adapter.
    pub fn opening<'a>(&'a self, home: &'a Path) -> Opening<'a> {
        Opening {
            policy: &self.policy,
            source: AdapterSource::Setting,
            root: &self.root,
            packs: Some(PackDirs {
                packs_dir: &self.packs_dir,
                defaults_dir: &self.defaults_dir,
            }),
            timeout: fleet_core::agent::types::timeout_from(
                std::env::var(TIMEOUT_VAR).ok().as_deref(),
            ),
            home,
        }
    }
}

// ---- what fleet sets for a seat's session -----------------------------------

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
/// (D1, D7): the constructed `PATH`, the four a shell needs
/// ([`crate::platform::PASSED_THROUGH`]), this process's own
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
    for pass in crate::platform::PASSED_THROUGH {
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
/// names (the claude-code pack's lessons B8), so a wait nobody can name still stops the
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

/// The opener's arms: `[agent] adapter` by path, by a name a fixture pack
/// carries, by a name nothing carries, and left out.
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::exec::tests::{answers, declared, recorded, scratch, verbs_in, write_stub};
    use super::*;

    /// An agent that keeps the contract and names an installed agent.
    fn a_good_agent() -> String {
        format!(
            "case \"$1\" in\n\
             capabilities) cat <<'JSON'\n{}\nJSON\n;;\n\
             version) echo '{{\"schema_version\":1,\"name\":\"quill\",\"version\":\"2.4.0\"}}' ;;\n\
             esac",
            declared(false)
        )
    }

    /// A machine directory's packs and the binary's defaults beneath them, as
    /// `fleet start` leaves them.
    struct Machine {
        dir: PathBuf,
        packs_dir: PathBuf,
        defaults_dir: PathBuf,
    }

    impl Machine {
        fn new(label: &str) -> Machine {
            let dir = scratch(label);
            let defaults_dir = dir.join(fleet_core::defaults::DIR);
            std::fs::create_dir_all(&defaults_dir).expect("the defaults dir is made");
            fleet_core::embedded::write_all(&defaults_dir).expect("the defaults are written");
            let packs_dir = dir.join("packs");
            std::fs::create_dir_all(&packs_dir).expect("the packs dir is made");
            Machine {
                dir,
                packs_dir,
                defaults_dir,
            }
        }

        fn packs(&self) -> PackDirs<'_> {
            PackDirs {
                packs_dir: &self.packs_dir,
                defaults_dir: &self.defaults_dir,
            }
        }

        /// The agent adapter `name` in an installed pack of the same name: its
        /// `adapter.toml`, and an entry `main` recording into the adapter's
        /// directory and answering `answer` — executable or not as asked.
        fn adapter(&self, name: &str, answer: &str, executable: bool) -> PathBuf {
            let pack = self.packs_dir.join(name);
            std::fs::create_dir_all(&pack).expect("the pack's directory is made");
            std::fs::write(
                pack.join("pack.toml"),
                format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n"),
            )
            .expect("the pack's manifest is written");
            let dir = pack.join(format!("adapters/agent/{name}"));
            std::fs::create_dir_all(&dir).expect("the adapter's directory is made");
            std::fs::write(
                dir.join("adapter.toml"),
                format!(
                    "[adapter]\nname = \"{name}\"\nkind = \"agent\"\nversion = \"0.1.0\"\n\
                     entry = \"main\"\n"
                ),
            )
            .expect("the adapter's manifest is written");
            let entry = dir.join("main");
            write_stub(&entry, &dir, answer);
            if !executable {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644))
                    .expect("the entry is made not executable");
            }
            dir
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn policy_of(text: &str) -> toml::Table {
        text.parse().expect("the fixture policy parses")
    }

    fn named(value: &str) -> toml::Table {
        policy_of(&format!("[agent]\nadapter = {value:?}\n"))
    }

    /// The opener over `policy`, rooted at `root`, with `packs` to resolve a
    /// name through, under a minute's bound: the first exec of a script just
    /// written can take tens of seconds on a busy macOS box.
    fn opened(
        policy: &toml::Table,
        source: AdapterSource,
        root: &Path,
        packs: Option<PackDirs>,
    ) -> Result<Opened, String> {
        open(&Opening {
            policy,
            source,
            root,
            packs,
            timeout: Duration::from_secs(60),
            home: root,
        })
    }

    fn refused(result: Result<Opened, String>) -> String {
        match result {
            Err(why) => why,
            Ok(opened) => panic!("`{}` opened", opened.name),
        }
    }

    /// A name no installed pack carries is refused before anything runs,
    /// naming the line that installs the one fleet-packs carries — and the
    /// default's name with an underscore for its dash is such a name: there
    /// is one spelling (ruling 17).
    #[test]
    fn a_name_no_pack_carries_refuses_with_the_pack_add_line() {
        let machine = Machine::new("open-nowhere");
        let underscored = DEFAULT_AGENT_ADAPTER.replace('-', "_");
        for name in ["nowhere", underscored.as_str()] {
            let why = refused(opened(
                &named(name),
                AdapterSource::Setting,
                &machine.dir,
                Some(machine.packs()),
            ));
            assert_eq!(
                why,
                format!(
                    "no agent adapter named `{name}` in the installed packs — `fleet pack add \
                     {}//adapters/agent/{name} --version {}` installs the one fleet-packs carries",
                    fleet_core::supported::PINNED_PACKS_SOURCE,
                    fleet_core::supported::PINNED_PACKS
                )
            );
        }
    }

    /// A name a fixture pack carries under `adapters/agent/x/` opens its
    /// entry: the gate asks its capabilities and its version, each one
    /// process carrying the root the opener was handed, and effects are on.
    #[test]
    fn a_name_a_pack_carries_opens_its_entry() {
        let machine = Machine::new("open-x");
        let dir = machine.adapter("x", &a_good_agent(), true);
        let opened = opened(
            &named("x"),
            AdapterSource::Setting,
            &machine.dir,
            Some(machine.packs()),
        )
        .expect("the pack's adapter opens");
        assert_eq!(opened.name, "x");
        assert_eq!(opened.effects_off, None);
        assert_eq!(verbs_in(&dir), ["capabilities", "version"]);
        assert_eq!(
            recorded(&dir, "version")["root"],
            machine.dir.display().to_string()
        );
        assert_eq!(
            opened.agent.version().expect("the entry answers").version,
            Some("2.4.0".to_string())
        );
    }

    /// An entry that is in the adapter's directory but not executable is the
    /// pack format's defect, and nothing is run.
    #[test]
    fn a_non_executable_entry_is_the_pack_defect_line() {
        let machine = Machine::new("open-defect");
        let dir = machine.adapter("x", &a_good_agent(), false);
        let why = refused(opened(
            &named("x"),
            AdapterSource::Setting,
            &machine.dir,
            Some(machine.packs()),
        ));
        assert!(
            why.starts_with("the agent adapter `x` cannot be opened: "),
            "{why}"
        );
        assert!(why.contains("not executable"), "{why}");
        assert!(verbs_in(&dir).is_empty(), "nothing ran");
    }

    /// A fleet whose file names no adapter, and one naming the default's
    /// name, open the pack's adapter where an installed pack carries it — and
    /// where none does, are refused as any name no pack carries is, naming
    /// the line that installs it. With no packs behind the caller at all, the
    /// default's name resolves nowhere either.
    ///
    /// RED-PROOF: with the default's name opening an adapter of its own where
    /// no pack carries it, the first half opens and nothing is refused.
    #[test]
    fn the_default_opens_the_pack_carrying_it_and_is_refused_where_none_does() {
        let machine = Machine::new("open-default");
        for policy in [policy_of(""), named(DEFAULT_AGENT_ADAPTER)] {
            let why = refused(opened(
                &policy,
                AdapterSource::Setting,
                &machine.dir,
                Some(machine.packs()),
            ));
            assert_eq!(
                why,
                format!(
                    "no agent adapter named `{DEFAULT_AGENT_ADAPTER}` in the installed packs — \
                     `{}` installs the one fleet-packs carries",
                    pack_line(
                        fleet_core::supported::PINNED_PACKS_SOURCE,
                        DEFAULT_AGENT_ADAPTER,
                        fleet_core::supported::PINNED_PACKS
                    )
                )
            );
        }
        assert_eq!(
            refused(opened(
                &policy_of(""),
                AdapterSource::Setting,
                &machine.dir,
                None
            )),
            format!(
                "no agent adapter named `{DEFAULT_AGENT_ADAPTER}` resolves: no packs are \
                 installed here to carry one"
            )
        );

        let dir = machine.adapter(DEFAULT_AGENT_ADAPTER, &a_good_agent(), true);
        for policy in [policy_of(""), named(DEFAULT_AGENT_ADAPTER)] {
            let _ = std::fs::remove_file(dir.join("argv"));
            let opened = opened(
                &policy,
                AdapterSource::Setting,
                &machine.dir,
                Some(machine.packs()),
            )
            .expect("the pack's adapter opens");
            assert_eq!(opened.name, DEFAULT_AGENT_ADAPTER);
            assert_eq!(opened.exec.entry(), dir.join("main"));
            assert_eq!(verbs_in(&dir), ["capabilities", "version"]);
        }
    }

    /// A machine whose defaults were never written has no layering to read,
    /// and a layering that is there and refuses reads no better: either way
    /// every name is refused, the default's among them, naming why.
    ///
    /// RED-PROOF: with a refusing layering read as the default's name
    /// resolving nowhere, the refusal names the pack add line and not why.
    #[test]
    fn with_no_layering_every_name_is_refused_naming_why() {
        let dir = scratch("open-no-defaults");
        let packs_dir = dir.join("packs");
        std::fs::create_dir_all(&packs_dir).expect("the packs dir is made");
        let defaults_dir = dir.join(fleet_core::defaults::DIR);
        let unwritten = PackDirs {
            packs_dir: &packs_dir,
            defaults_dir: &defaults_dir,
        };
        for (policy, name) in [(policy_of(""), DEFAULT_AGENT_ADAPTER), (named("x"), "x")] {
            let why = refused(opened(
                &policy,
                AdapterSource::Setting,
                &dir,
                Some(unwritten),
            ));
            assert!(
                why.starts_with(&format!("no agent adapter named `{name}` resolves: ")),
                "{why}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);

        let machine = Machine::new("open-refusing");
        // Two packs importing each other: a cycle no layering resolves.
        for (name, other) in [("a", "b"), ("b", "a")] {
            let pack = machine.packs_dir.join(name);
            std::fs::create_dir_all(&pack).expect("the pack's directory is made");
            std::fs::write(
                pack.join("pack.toml"),
                format!(
                    "[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n\n\
                     [imports.{other}]\nsource = \"../{other}\"\nversion = \"0.1.0\"\n"
                ),
            )
            .expect("the pack's manifest is written");
        }
        let why = refused(opened(
            &policy_of(""),
            AdapterSource::Setting,
            &machine.dir,
            Some(machine.packs()),
        ));
        assert!(
            why.starts_with(&format!(
                "no agent adapter named `{DEFAULT_AGENT_ADAPTER}` resolves: "
            )),
            "{why}"
        );
    }

    /// An absolute path to an executable is the adapter at that path, and its
    /// name is the path as written. A path that is no executable file, and a
    /// value that is neither form, are refused naming what the file said —
    /// or the flag, where a flag carried it.
    #[test]
    fn a_path_opens_its_executable_and_anything_else_is_refused_by_its_form() {
        let root = scratch("open-path");
        let bin = root.join("adapter");
        write_stub(&bin, &root, &a_good_agent());
        let opened_path = opened(
            &named(&bin.display().to_string()),
            AdapterSource::Setting,
            &root,
            None,
        )
        .expect("an executable opens");
        assert_eq!(opened_path.name, bin.display().to_string());
        assert_eq!(opened_path.effects_off, None);

        let absent = root.join("absent");
        assert_eq!(
            refused(opened(
                &named(&absent.display().to_string()),
                AdapterSource::Setting,
                &root,
                None
            )),
            format!(
                "[agent] adapter names `{}`, which is not an executable file",
                absent.display()
            )
        );
        assert_eq!(
            refused(opened(
                &named(&absent.display().to_string()),
                AdapterSource::Flag,
                &root,
                None
            )),
            format!(
                "--adapter names `{}`, which is not an executable file",
                absent.display()
            )
        );
        for (value, said) in [("\"bin/adapter\"", "bin/adapter"), ("\"\"", ""), ("3", "3")] {
            assert_eq!(
                refused(opened(
                    &policy_of(&format!("[agent]\nadapter = {value}\n")),
                    AdapterSource::Setting,
                    &root,
                    None
                )),
                format!(
                    "[agent] adapter is `{said}` — it is the name of an agent adapter an \
                     installed pack carries, or an absolute path to an adapter executable"
                )
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An adapter executable whose capabilities break the contract, whose
    /// version does not answer, or whose version names no installed agent,
    /// opens with effects OFF and the cause named — the capabilities' field
    /// among it — and a loop still reads through it.
    ///
    /// RED-PROOF: with the gate never asked, every opening here has effects
    /// on.
    #[test]
    fn an_adapter_whose_capabilities_or_version_do_not_answer_has_effects_off() {
        let root = scratch("open-gate");
        let empty_postures = String::from(
            "case \"$1\" in\n\
             capabilities) echo '{\"schema_version\":1,\"postures\":[],\"default_model\":\"m\",\"first_turn\":\"/wake {seat}\",\"measured\":[\"2.4.0\"]}' ;;\n\
             version) echo '{\"schema_version\":1,\"name\":\"quill\",\"version\":\"2.4.0\"}' ;;\n\
             esac",
        );
        let no_version = format!(
            "case \"$1\" in\n\
             capabilities) cat <<'JSON'\n{}\nJSON\n;;\n\
             version) echo 'the agent is not here' >&2; exit 3 ;;\n\
             esac",
            declared(false)
        );
        let null_version = format!(
            "case \"$1\" in\n\
             capabilities) cat <<'JSON'\n{}\nJSON\n;;\n\
             version) {} ;;\n\
             esac",
            declared(false),
            answers(r#"{"schema_version":1,"name":"quill","version":null}"#, 0)
        );
        for (label, body, cause) in [
            ("postures", empty_postures, "postures is empty".to_string()),
            (
                "unanswered",
                no_version,
                "version could not tell: the agent is not here".to_string(),
            ),
            (
                "null",
                null_version,
                "answers that no quill is installed (its version is null)".to_string(),
            ),
        ] {
            let dir = root.join(label);
            std::fs::create_dir_all(&dir).expect("the stub's directory is made");
            let bin = dir.join("adapter");
            write_stub(&bin, &dir, &body);
            let opened = opened(
                &named(&bin.display().to_string()),
                AdapterSource::Setting,
                &root,
                None,
            )
            .expect("the adapter opens, effects or not");
            let off = opened
                .effects_off
                .unwrap_or_else(|| panic!("{label}: effects are on"));
            assert!(off.contains(&cause), "{label}: {off}");
            assert!(
                off.contains(&bin.display().to_string()),
                "{label}: the cause names the adapter: {off}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
