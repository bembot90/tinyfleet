//! The agent seam. Everything the controller learns about a session goes
//! through one trait, so the second agent is a second module and not a
//! rewrite.
//!
//! Observe needs four of the seven verbs — the listing, the transcript, the
//! end stamp and the version; `launch`, `stop`, `remove` and `revive` are the
//! four an effect issues. A turn for a live seat is none of them: it is typed
//! into the seat's pane by core (`crate::effect::type_turn`), and this trait's
//! listing is what says whether it was taken. Whether a session is THERE is not
//! a verb here at all: presence is the host's reading (`crate::host`, ruling
//! 3), and what this trait's listing answers is what the session is DOING.
//!
//! A START IS TWO HALVES AND ONLY ONE OF THEM IS HERE (ruling 2). The adapter
//! answers WHAT to run — the argv and the environment, in [`Launch`] — and
//! core runs it, as a session on the host (`crate::host`), and believes it
//! only when this trait's own listing shows the pane's process
//! (`crate::effect::start_once`). No adapter touches the host.

use serde::Deserialize;
use std::path::Path;

pub mod claude_code;

/// What a start passes the agent. Every field is mandatory: a start with no
/// model comes up on the cheapest available one, and a start with no posture
/// takes the agent's default rather than the fleet's (lessons claude-code A5).
#[derive(Clone, Debug)]
pub struct StartSpec {
    /// The seat's id, as its hyphenated string.
    pub seat: String,
    pub worktree: String,
    /// The session's name: `--name`, and what its start log is named by.
    pub name: String,
    /// Who the session's own verbs act as, `seat:<id>`, handed to it as
    /// `FLEET_ACTOR` [ASSUMES D7]: a seat's bare `fleet deliver` is the seat's
    /// act and never the machine's person's.
    pub actor: String,
    pub model: String,
    pub posture: String,
    pub first_turn: String,
    /// The plugin root this session loads, or `None` for a fleet that names
    /// none — the one field that is optional, because a fleet with no overlay
    /// is a fleet whose starts carry no such flag at all (lessons claude-code
    /// D5).
    pub plugin_dir: Option<String>,
    /// The configuration directory THIS session comes up under, or `None` for a
    /// start that takes the adapter's own. Every spawned seat comes up under one
    /// of its own, holding only the pack's overlay.
    ///
    /// A directory here is the session's whole configuration space: nothing
    /// from the person's home directory — settings, memory, instructions,
    /// servers — reaches it. It scopes the agent's listing with it, so a
    /// session started under one is listed under that directory and no other,
    /// and every read about this session is made under the same value.
    ///
    /// It is also the one mark of a start whose worktree fleet CREATED: a named
    /// seat comes up in a person's own checkout and carries `None`, so a start
    /// with a directory here is the only one whose worktree a launch may seed
    /// as trusted (ruling 13).
    pub config_dir: Option<String>,
}

/// What a start runs: the pane's whole command and its whole environment.
///
/// The argv is the pane's own process (reviewer call 2026-09-25, E2), its first
/// element the absolute binary, and the environment is set EXACTLY — the host
/// hands the pane these pairs and nothing of its own — so every variable a
/// session needs is one this value names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The three answers a removal has, and one of them is an alarm.
///
/// A remove that prints a worktree path DELETED it, and a seat's checkout has a
/// remote — which is measured to make the refusal the expected answer on one
/// (lessons claude-code A8). No discard flag is ever passed, so this arm must
/// not be reachable; it is a variant here rather than an assumption so a reader
/// meets the path in the log instead of meeting the missing directory.
#[derive(Clone, Debug)]
pub enum RemoveAnswer {
    Removed,
    Refused { cause: String },
    RemovedAWorktree { path: String },
}

pub trait Agent {
    /// What to run to bring a fresh woken session up in the seat's worktree.
    ///
    /// It RUNS NOTHING: the session is started by core, on the host, from the
    /// value this answers. It may WRITE — inside the start's own configuration
    /// directory, which is what that directory is for (reviewer call
    /// 2026-09-25, E8) — and `Err` is a start that must not be attempted,
    /// carrying why.
    fn launch(&self, spec: &StartSpec) -> Result<Launch, String>;

    /// The keys that answer the question a starting session's screen is
    /// stopped at, where this agent knows the question and a start may answer
    /// it; `None` for a screen it does not recognise.
    ///
    /// The one screen rule a start reads, and read only for a session whose
    /// worktree fleet itself created: the keys accept the agent's
    /// workspace-trust question, which the acceptance `launch` seeds should
    /// already have skipped (ruling 13) — so this is the fallback, not the
    /// path. The rule is the adapter's because the words on the screen are the
    /// agent's (ruling 2); core only captures and types.
    fn trust_keys(&self, _screen: &str) -> Option<Vec<String>> {
        None
    }

    /// Where this provider reads a session's project-local settings from,
    /// relative to the working directory the session comes up in.
    ///
    /// A permission list cannot ride the plugin root the overlay is loaded
    /// through, so a transient seat's rules are written here instead — which is
    /// why the path is the adapter's and not the spawn's: it is the one fact in
    /// that write that belongs to one provider.
    fn local_settings(&self) -> &'static str;

    /// Stop a live session BY ITS SHORT ID. The full session id exits 1 with
    /// "No job matching" (lessons claude-code A6), so the address and the key
    /// are different values and this takes the address.
    fn stop(&self, config_dir: Option<&Path>, short_id: &str) -> Result<(), String>;

    /// Delete a stopped session's row, by the same address.
    fn remove(&self, config_dir: Option<&Path>, short_id: &str) -> RemoveAnswer;

    /// Bring a hibernated session back IN PLACE, by the same short id a stop
    /// takes — same session, same id, context intact.
    ///
    /// An ATTACH and never a resume: only a flagless full-id resume continues a
    /// session and a flagged one forks it (lessons claude-code A9), and the row
    /// this reaches is a live one addressed by its short id exactly as a stop
    /// is. `Ok` is a DISPATCH and not a witness: the call exits 0 whether it
    /// revived the row or silently did nothing (A7), so the roster's next read
    /// is the only thing that says which.
    fn revive(&self, config_dir: Option<&Path>, short_id: &str) -> Result<(), String>;

    /// The listing under one configuration directory, or the adapter's own when
    /// `config_dir` is `None`.
    ///
    /// The fleet's own read is one command per poll shared by every seat: two
    /// seats must not decide against different readings of the same moment. A
    /// session under its own configuration directory appears in NO other
    /// listing (lessons claude-code A11, B10), so a row started that way is
    /// asked for under that directory and is invisible to every other read this
    /// trait has.
    fn status(&self, config_dir: Option<&Path>) -> RosterRead;

    /// The session's transcript body, or `None` when there is nothing to read,
    /// under the same directory the session was started with.
    fn transcript(
        &self,
        config_dir: Option<&Path>,
        worktree: &str,
        session_id: &str,
    ) -> Option<String>;

    /// When this session last wrote, in epoch milliseconds — the end the
    /// listing does not carry. The transcript's last write is the session's own
    /// final act, and it outlives the process (lessons claude-code C4), so it
    /// is an end a controller still has in hand long after the row left the
    /// listing: what dates a dead pane this controller did not see die.
    /// `None` when the transcript does not resolve, which is a reading nobody
    /// has rather than a session that never ended.
    ///
    /// It reads the same file [`Agent::transcript`] does, so it takes the same
    /// directory: a row asked for under the wrong one resolves no transcript at
    /// all, which reads as an end nobody has.
    fn ended_at(&self, config_dir: Option<&Path>, worktree: &str, session_id: &str) -> Option<u64>;

    /// What the agent binary reports **this poll**. A version read once at
    /// startup and republished advertises the boot version for as long as the
    /// controller lives.
    fn version(&self) -> Option<String>;
}

/// One session as the listing reports it.
///
/// There is deliberately no token field: the listing carries none (lessons
/// claude-code B2), and context comes from the transcript.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentRow {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    /// The SHORT id: the address every subcommand accepts, and never the
    /// identity (lessons claude-code A6). Absent on an interactive row, which
    /// carries no address of its own.
    #[serde(default)]
    pub id: Option<String>,
    pub cwd: String,
    /// The session's process. An interactive row always carries it, and it is
    /// what a row is attributed to a seat BY: the pane's own pid (E2). A row
    /// without one is a background shape no seat the host runs can produce.
    #[serde(default)]
    pub pid: Option<u32>,
    /// A background row's five-word state (lessons claude-code A3). An
    /// interactive row never carries it (B10), and nothing decides on it.
    #[serde(default)]
    pub state: Option<String>,
    /// What the agent says this session is DOING right now, in the agent's own
    /// vocabulary. Absent on a row that carries none, which is a reading nobody
    /// has rather than an idle session — so it is matched for equality against
    /// the one word the cap leg counts ([`BUSY`]) and never read as a negative.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "startedAt", default)]
    pub started_at: Option<u64>,
    /// Present ONLY while the session is stopped in front of a human, and its
    /// value names the cause (lessons claude-code B8). Read for PRESENCE and
    /// never by matching the value: the vocabulary is the agent's, so a cause
    /// this fleet does not recognise must still stop the seat rather than read
    /// as a healthy one.
    #[serde(rename = "waitingFor", default)]
    pub waiting_for: Option<String>,
}

/// The agent's own word for a session that is mid-turn. One word, matched for
/// equality: the vocabulary is the agent's, so a status this fleet does not
/// recognise counts as not-busy rather than as a fifth state to reason about.
pub const BUSY: &str = "busy";

/// The agent's word for a session stopped in front of a human, which an
/// interactive row carries beside [`AgentRow::waiting_for`] (lessons claude-code
/// B10). Read as a block on its own too, so a row that names no cause is still
/// one nothing is typed into.
pub const WAITING: &str = "waiting";

impl AgentRow {
    pub fn is_live(&self) -> bool {
        self.pid.is_some()
    }

    /// Whether this row is a session mid-turn.
    pub fn is_busy(&self) -> bool {
        self.status.as_deref() == Some(BUSY)
    }

    /// What this row is stopped in front of a human on, where it is: the cause
    /// [`AgentRow::waiting_for`] names, else the [`WAITING`] status word itself.
    /// Read for presence (B8), so a cause this fleet does not know still blocks.
    pub fn blocked_on(&self) -> Option<String> {
        self.waiting_for.clone().or_else(|| {
            (self.status.as_deref() == Some(WAITING)).then(|| format!("status {WAITING}"))
        })
    }

    /// The row's working directory in the form seat matching compares.
    pub fn cwd_key(&self) -> &str {
        dir_key(&self.cwd)
    }
}

/// A directory path with any trailing separator removed — the one form both
/// sides of the seat match are put in, so a configured path and a reported one
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

/// A whole-fleet read, or the reason there is none.
///
/// `Unreadable` carries why. The distinction is the type's whole job: a listing
/// that answers with zero bytes and exit 0 while sessions are live (lessons
/// claude-code B4) must not reach a seat as "no sessions", because that is every
/// seat reading absent at once.
#[derive(Clone, Debug)]
pub enum RosterRead {
    Readable(Vec<AgentRow>),
    Unreadable { cause: String },
}

/// The transcript path encoding (lessons claude-code C1): the agent keys a
/// per-project directory on the project path with every non-alphanumeric
/// character replaced by a dash — the separators, and the dots, underscores and
/// spaces beside them.
///
/// Censused on this fleet's own machine: of 235 per-project directories, zero
/// carry any character outside `[A-Za-z0-9-]`, and a path under `.claude`
/// resolves to `--claude`, so the dash is not the separator's alone.
///
/// TWO PARTS OF THE ENCODING ARE NOT HANDLED HERE, because no specimen on this
/// machine exercises them: a project path past roughly 200 characters, which
/// the agent truncates and gives a hash suffix (the longest local directory is
/// 136), and a non-ASCII character, which this maps to a dash without a
/// measurement saying it should. Either yields a path that does not exist,
/// which every reader renders as a seat with no context reading.
///
/// Pure, and separate from the read, because the encoding is what can be wrong
/// and a test must reach it without a filesystem.
pub fn encode_project_dir(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The session's transcript under the agent's configuration directory.
pub fn transcript_path(config_dir: &Path, worktree: &str, session_id: &str) -> std::path::PathBuf {
    config_dir
        .join("projects")
        .join(encode_project_dir(worktree))
        .join(format!("{session_id}.jsonl"))
}
