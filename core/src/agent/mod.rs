//! The agent contract: what fleet asks the program a seat runs, and what that
//! program's adapter answers, as fleet's own types.
//!
//! A seat is fleet's worker and the agent the program it runs. fleet drives
//! the agent's session itself — the host starts it, types into it, reads its
//! screen and ends it — and an adapter never touches the host. What only the
//! agent's own adapter can say is what this contract asks, in six verbs:
//! `capabilities` and `version`, what the agent is; `launch` and `resume`, the
//! argv and environment a session starts or comes back under; `read`, what
//! each seat's agent is doing; and `context`, how full its window is, asked
//! only of an agent that declares it.
//!
//! THE CALL IS THE STORE'S. An agent adapter is spoken to through the same
//! [`crate::adapter::exec`] the store's is — the verb, the envelope with the
//! same `root`, the exit table, the bound and its group kill — so this module
//! states only what is the agent contract's own: its version, its verbs'
//! request and answer types, and its refusal's reasons.
//!
//! THE CONTRACT LIVES HERE WHOLE, laid out as the store's is
//! ([`crate::store`]): the trait fleet drives an agent through, the executable
//! that answers it ([`exec`]), its opener ([`open`]), its schema and its types
//! (E10, reversed 2026-10-07). The suite that holds an adapter to this
//! contract is the controller's (`fleet_controller::adapter::conformance`),
//! because its live steps run a session on the controller's host. The JSON is
//! core's, so the schema `fleet agent schema` prints and the page
//! `docs/agent.md` shows are generated from and held to one set of types,
//! whichever crate calls them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::adapter::open::{resolve, Wanted};
use crate::agent::types::TIMEOUT_VAR;
use crate::pack::AdapterKind;

pub mod exec;
pub mod schema;
pub mod types;

pub use exec::AgentExec;
pub use types::{
    Activity, Argv, BlockedOn, Capabilities, Evidence, Launch, Permissions, Posture, Refusal,
    RefusalReason, Resume, SeatActivity, SeatContext, SeatRef, Version,
};

pub use crate::adapter::{AdapterSource, PackDirs};

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
                refusal.reason.word(),
                refusal.message
            ),
            AgentError::Unreadable(cause) => f.write_str(cause),
        }
    }
}

/// The agent seam. Everything the controller learns about a seat's agent goes
/// through one trait, and the trait is the agent contract's six verbs
/// (`docs/agent.md`) over the contract's own types ([`types`]),
/// answered by an adapter that is an executable ([`AgentExec`]): the one an
/// installed pack carries under the name `[agent] adapter` writes, or the one
/// at the path it names.
///
/// What the agent IS — [`Agent::capabilities`] and [`Agent::version`]; what a
/// session starts or comes back under — [`Agent::launch`] and
/// [`Agent::resume`]; what each seat's agent is DOING — [`Agent::read`]; and how
/// full its window is — [`Agent::context`], asked only of an agent that declares
/// it. Nothing else crosses: no listing row, no transcript and no short id
/// leaves an adapter (reviewer call 2026-09-25, E6).
///
/// A turn for a live seat is none of them: it is typed into the seat's pane by
/// the controller (`fleet_controller::effect::type_turn`), and `read` is what says
/// whether it was taken. Whether a session is THERE is not a verb here at all:
/// presence is the host's reading (`fleet_controller::host`, ruling 3). Nor is
/// ending one: a session is stopped on the host, by its seat
/// (`fleet_controller::effect::stop_session`).
///
/// A START IS TWO HALVES AND ONLY ONE OF THEM IS HERE (ruling 2). The adapter
/// answers WHAT to run — the argv and the environment, an [`Argv`] — and the
/// controller runs it, as a session on the host, and believes it only when `read` finds
/// the pane's own process (`fleet_controller::effect::start_once`). No adapter
/// touches the host.
///
/// EVERY CALLER OPENS THE AGENT THROUGH [`open`], so which adapter answers, and
/// whether it may issue effects at all, is decided in one place.
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
/// names the adapter, where a name is resolved, and the search path a pack's
/// adapter runs on.
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
    /// [`TIMEOUT_VAR`] sets, else [`types::AGENT_TIMEOUT`].
    pub timeout: std::time::Duration,
    /// The PATH a pack's agent adapter runs on: the caller's constructed child
    /// PATH (`fleet_controller::platform::child_path`), as the store's
    /// [`Opening::search_path`](crate::store::Opening::search_path) is. EMPTY
    /// skips the runtimes, and the adapter carries this process's own `PATH`.
    pub search_path: &'a str,
}

/// The agent a caller opened, and why it may issue no effect where it may
/// not.
pub struct Opened {
    /// The name `[agent] adapter` will call this adapter by.
    pub name: String,
    /// The same agent as the executable it is: what `fleet agent check` speaks
    /// to past the verbs' own types, for a check about an exit or a recorded
    /// case.
    pub agent: AgentExec,
    /// Why no effect may be issued through this agent — nothing it could launch
    /// resolved — or `None` for an agent that can. Reads are answered either
    /// way: a loop that cannot start a session still observes and publishes.
    pub effects_off: Option<String>,
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
    let resolved = resolve(&Wanted {
        kind: AdapterKind::Agent,
        policy: opening.policy,
        source: opening.source,
        search_path: opening.search_path,
        packs: opening.packs,
        default: DEFAULT_AGENT_ADAPTER,
    })?;
    let agent = AgentExec::at(&resolved.entry, opening.root).with_timeout(opening.timeout);
    let agent = match resolved.path {
        Some(path) => agent.on_path(path),
        None => agent,
    };
    Ok(gated(resolved.named, agent, resolved.dir))
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
        agent,
        effects_off,
        dir,
    }
}

/// The name `[agent] adapter` in `policy` calls the fleet's agent by, as
/// written where it is a string, and [`DEFAULT_AGENT_ADAPTER`] where the file
/// writes none: what a caller whose [`open`] refused still publishes.
pub fn named(policy: &toml::Table) -> String {
    match crate::policy::read("agent", "adapter", policy) {
        Ok(Some(toml::Value::String(name))) => name.clone(),
        _ => DEFAULT_AGENT_ADAPTER.to_string(),
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
            crate::item::read_table(fleet_toml)?,
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
        let (packs_dir, defaults_dir) = crate::defaults::pack_dirs(machine_dir, None);
        Setting {
            policy,
            root: fleet_toml
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            packs_dir,
            defaults_dir,
        }
    }

    /// The opening over this fleet, as `[agent] adapter` in its own file
    /// names the adapter.
    pub fn opening<'a>(&'a self, search_path: &'a str) -> Opening<'a> {
        Opening {
            policy: &self.policy,
            source: AdapterSource::Setting,
            root: &self.root,
            packs: Some(PackDirs {
                packs_dir: &self.packs_dir,
                defaults_dir: &self.defaults_dir,
            }),
            timeout: crate::agent::types::timeout_from(std::env::var(TIMEOUT_VAR).ok().as_deref()),
            search_path,
        }
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
) -> BTreeMap<crate::seat::identity::SeatId, SeatContext> {
    if !declared || seats.is_empty() {
        return BTreeMap::new();
    }
    match agent.context(seats) {
        Ok(rows) => rows.into_iter().map(|row| (row.seat, row)).collect(),
        Err(_) => BTreeMap::new(),
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
            .or_else(|| reading.blocked_on.map(|on| on.word().to_string()))
            .unwrap_or_else(|| Activity::Blocked.word().to_string())
    })
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
            let defaults_dir = dir.join(crate::defaults::DIR);
            std::fs::create_dir_all(&defaults_dir).expect("the defaults dir is made");
            crate::embedded::write_all(&defaults_dir).expect("the defaults are written");
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

    /// The PATH every opener's adapter here runs on: the stubs need only
    /// `/bin/sh` and `cat`, and core has no constructed child PATH of its own
    /// (`fleet_controller::platform::child_path` is the controller's).
    const SEARCH: &str = "/usr/bin:/bin";

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
            search_path: SEARCH,
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
                    crate::supported::PINNED_PACKS_SOURCE,
                    crate::supported::PINNED_PACKS
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
                    AdapterKind::Agent.pack_line(
                        crate::supported::PINNED_PACKS_SOURCE,
                        DEFAULT_AGENT_ADAPTER,
                        crate::supported::PINNED_PACKS
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
            assert_eq!(opened.agent.entry(), dir.join("main"));
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
        let defaults_dir = dir.join(crate::defaults::DIR);
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
