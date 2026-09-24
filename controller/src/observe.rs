//! What one poll saw: a seat's slice of the roster, and the context its session
//! is carrying.

use crate::adapter::{dir_key, AgentRow, RosterRead};
use crate::config::Seat;
use fleet_core::seat::identity::SeatId;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// How long a pid-less row that carries no end marker may claim to be starting.
///
/// The birth window is well under a second against a five-second poll, so this
/// is deliberately far larger than the phenomenon: being generous costs a few
/// polls before a genuinely stuck row is noticed, being tight reads a newborn as
/// absent, which is the defect the split exists to prevent.
pub const STARTING_GRACE_MS: u64 = 30_000;

/// How far back an ended row is still reported rather than treated as history,
/// and WHERE THAT DISTANCE IS MEASURED FROM.
///
/// The window is measured from the session's END. Keyed on the start instead, a
/// 25-hour session that ended a minute ago is history and its transcript is
/// discarded, while one that started and ended 23 hours ago ranks as freshly
/// stopped — the reading is about how long a session RAN and not about how long
/// ago it finished.
///
/// THE LISTING CARRIES NO END STAMP. Measured 2026-09-08 on 2.1.261: `agents
/// --json --all` answers cwd, id, kind, name, pid, sessionId, startedAt, state
/// and status, on stopped rows included, and `AgentRow` parses none that could
/// serve. The end this controller has in hand is the session TRANSCRIPT's last
/// write: the same file `observe` already resolves to read context, which
/// outlives the process that wrote it. A transcript that does not resolve
/// leaves the start stamp as the only reading there is, and a row filtered on
/// it says so in its own cause rather than passing for an end-keyed one.
pub struct Recency<'a> {
    /// The window itself, from policy — `controller.stopped_recency_hours`,
    /// rendered to milliseconds by the caller that read it.
    pub window_ms: u64,
    /// When this session last wrote, by worktree and session id. `None` is a
    /// transcript that did not resolve, which is the fallback's whole trigger.
    pub ended_at: &'a dyn Fn(&str, &str) -> Option<u64>,
}

/// What a stopped row was filtered on, for the row that carries the reading.
/// A fallback nobody can see is a start-keyed window under another name.
pub const FELL_BACK_TO_START: &str =
    "the stopped-row window was measured from this session's START: its transcript did not \
     resolve, so no end is known for it";

/// What the roster said about one seat.
///
/// `Unknown` is not `Absent`, and the type is what makes conflating them
/// unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RosterState {
    Present,
    /// A live session stopped in front of a human. Split from `Present` rather
    /// than carried as a flag on it so every rule that matches on the state has
    /// to say which side this falls on.
    PromptBlocked,
    Starting,
    Stopped,
    Absent,
    Unknown,
}

impl RosterState {
    /// The published spelling. These six strings are an external contract: the
    /// projection is read by tools that are not this binary, and the reference
    /// controller publishes the same six.
    pub fn as_str(&self) -> &'static str {
        match self {
            RosterState::Present => "present",
            RosterState::PromptBlocked => "prompt-blocked",
            RosterState::Starting => "starting",
            RosterState::Stopped => "stopped",
            RosterState::Absent => "absent",
            RosterState::Unknown => "unknown",
        }
    }

    /// Whether a seat in this state has a session whose transcript answers for
    /// it. A live one does and an ended one does — its transcript outlives the
    /// process (lessons claude-code C4) — and the reference reads no context for
    /// the other four, so neither does this: the row-for-row parity between the
    /// two projections is what the acceptance is.
    pub fn has_context_reading(&self) -> bool {
        matches!(self, RosterState::Present | RosterState::Stopped)
    }
}

#[derive(Clone, Debug)]
pub struct SeatObservation {
    pub state: RosterState,
    pub unknown_cause: Option<String>,
    /// The agent's own cause string for a seat stopped in front of a human,
    /// carried verbatim and never matched on.
    pub waiting_for: Option<String>,
    /// Set exactly when this row survived the stopped-row window on its START
    /// stamp because no end was known for it. `None` on every other row,
    /// stopped ones with a resolved end included.
    pub recency_fallback: Option<String>,
    /// The session whose transcript answers for this seat, when there is one.
    pub session_id: Option<String>,
    /// That session's ADDRESS, which is a different value from its id (lessons
    /// claude-code A6) and is what an effect issues against. Absent on a row
    /// that carries none.
    pub short_id: Option<String>,
    pub project: Option<String>,
    pub worktree: Option<String>,
    /// Whether this seat's worktrees held a pid-less row at all this poll,
    /// including one the recency window aged out of the reading above. `Absent`
    /// collapses "no row anywhere" and "only rows too old to answer" into one
    /// state, and the replacement window has to tell them apart: the row a
    /// replacement is re-hosting can be older than the bound (lessons
    /// claude-code A10, the 2026-09-04 outage).
    pub pidless_row: bool,
    /// Whether the row this reading came from names an end the ROSTER can name
    /// — `stopped` or `failed`, the two words it reaches only from a non-idle
    /// prior state. Hibernation, a stop from idle and a kill from outside the
    /// fleet read identically on every roster field (lessons claude-code A3),
    /// so none of them is here: an end the listing cannot name is one the
    /// controller's own record answers for. False on a seat no row matched,
    /// which is a reading nobody took rather than a session that ended.
    pub state_names_an_end: bool,
}

/// The listings one poll read: the fleet's own, and one per seat that comes up
/// under its own configuration directory.
///
/// A session started under its own configuration directory is held by its own
/// daemon, and THAT DAEMON'S LISTING IS THE ONLY ONE THAT NAMES IT — the
/// fleet's answers as if the session did not exist. So a poll reads once per
/// distinct directory rather than once, and each seat is decided against the
/// listing that could see it.
///
/// The listings are kept APART rather than concatenated into one vector: a
/// per-seat read that could not be made has to leave that seat Unknown while
/// every other seat is still decided, and rows folded into one list carry no
/// record of which read failed.
pub struct Rosters {
    fleet: RosterRead,
    per_seat: BTreeMap<SeatId, RosterRead>,
}

impl Rosters {
    /// The fleet's listing plus one per distinct configuration directory.
    ///
    /// `config_dir_of` answers the directory a seat's own row names, or `None`
    /// for a seat that comes up under the fleet's. `read` is the listing under
    /// one directory. Both are seams so the fold is exercised without a daemon:
    /// the defect this shape exists to prevent is reading every row under the
    /// adapter's own directory, which is invisible in any rig where the two
    /// reads answer alike.
    pub fn gather(
        seats: &[Seat],
        config_dir_of: &dyn Fn(&SeatId) -> Option<String>,
        read: &dyn Fn(Option<&Path>) -> RosterRead,
    ) -> Rosters {
        let fleet = read(None);
        let mut by_dir: BTreeMap<String, RosterRead> = BTreeMap::new();
        let mut per_seat = BTreeMap::new();
        for seat in seats {
            let Some(dir) = config_dir_of(&seat.id) else {
                continue;
            };
            let listing = match by_dir.get(&dir) {
                Some(read) => read.clone(),
                None => {
                    let listing = read(Some(Path::new(&dir)));
                    by_dir.insert(dir, listing.clone());
                    listing
                }
            };
            per_seat.insert(seat.id, listing);
        }
        Rosters { fleet, per_seat }
    }

    /// The fleet's own listing, for the readings that are about the fleet rather
    /// than about one seat.
    pub fn fleet(&self) -> &RosterRead {
        &self.fleet
    }

    /// The listing this seat is decided against.
    pub fn for_seat(&self, id: &SeatId) -> &RosterRead {
        self.per_seat.get(id).unwrap_or(&self.fleet)
    }

    /// Every row of every READABLE listing this poll took, folded into one.
    ///
    /// For the readings that are about the fleet's sessions rather than about one
    /// seat's verdict — adoption is the one, and a session under a per-row
    /// directory has to be claimable like any other, or a controller that
    /// restarted would never own the spawned seats it started.
    ///
    /// A listing that could not be read contributes nothing and says nothing:
    /// this is not the type that carries could-not-tell, and a caller that needs
    /// that distinction per seat asks [`Rosters::for_seat`].
    pub fn all_rows(&self) -> Vec<AgentRow> {
        let mut rows = Vec::new();
        for read in std::iter::once(&self.fleet).chain(self.per_seat.values()) {
            if let RosterRead::Readable(found) = read {
                rows.extend(found.iter().cloned());
            }
        }
        rows
    }
}

/// One seat's view of one roster read.
///
/// Rows are matched to seats by working directory and never by the short id.
/// A working directory NAMES a seat and proves nothing about who dispatched the
/// session (lessons claude-code B5), so two live rows in one worktree are
/// Unknown rather than a contest: there is no honest way to pick, and picking
/// arbitrarily is how unattributed sessions get handed to a seat as its own.
pub fn observe_seat(
    read: &RosterRead,
    seat: &Seat,
    now_ms: u64,
    recency: &Recency,
) -> SeatObservation {
    let rows = match read {
        RosterRead::Unreadable { cause } => {
            let mut unknown = unmatched(seat);
            unknown.state = RosterState::Unknown;
            unknown.unknown_cause = Some(cause.clone());
            return unknown;
        }
        RosterRead::Readable(rows) => rows,
    };

    // Every row standing in one of this seat's worktrees, carrying the project
    // whose path it matched: a seat has a worktree per project and the published
    // row names the one the session is actually in.
    let matched: Vec<Match> = rows
        .iter()
        .filter_map(|row| {
            seat.worktrees
                .iter()
                .find(|(_, path)| dir_key(path) == row.cwd_key())
                .map(|(project, path)| Match {
                    row,
                    project,
                    worktree: path,
                })
        })
        .collect();

    // Partition by pid, never by count: two ended rows beside a live one is a
    // seat working normally beside its own history, and the unattributable case
    // is competing LIVE claims.
    let (live, pidless): (Vec<Match>, Vec<Match>) =
        matched.into_iter().partition(|m| m.row.is_live());
    // Held before the window filter below consumes the rows: a seat whose only
    // rows aged out reads `Absent`, and the replacement hold needs to know the
    // rows were there.
    let a_pidless_row_stands = !pidless.is_empty();

    if live.len() > 1 {
        let mut unknown = live[0].seen(RosterState::Unknown);
        unknown.session_id = None;
        // And its address with it: an unattributable pair leaves no session to
        // name, so it must leave nothing to act against either.
        unknown.short_id = None;
        unknown.unknown_cause = Some(format!(
            "{} live rows stand in {}'s worktrees; a seat is one session, and a \
             working directory names a seat without proving a session is that \
             seat's, so these are unattributed rather than the seat's",
            live.len(),
            seat.machine_name()
        ));
        return unknown;
    }
    if let [only] = live.as_slice() {
        // PRESENCE of the field, never its value: an unrecognised cause is still
        // a seat that cannot act, and reading it as healthy is the whole defect.
        let state = match only.row.waiting_for {
            Some(_) => RosterState::PromptBlocked,
            None => RosterState::Present,
        };
        return only.seen(state);
    }

    // No live row. A starting row outranks the ended ones: it is the only one
    // about to become live, and reading it as absent is what puts a second
    // session in one worktree.
    let (starting, stopped): (Vec<Match>, Vec<Match>) = pidless
        .into_iter()
        .partition(|m| m.row.is_starting(now_ms, STARTING_GRACE_MS));
    if let Some(newborn) = newest(&starting) {
        return newborn.seen(RosterState::Starting);
    }
    // The window, measured from each row's own END. A row whose transcript
    // resolves is ranked on its last write; one whose transcript does not is
    // ranked on its start, and carries that reading with it so the projection
    // can say which of the two it is.
    // The end is asked for ONCE per row: it is a filesystem read, and two calls
    // could answer differently across a write.
    let recent: Vec<(Match, u64, Option<String>)> = stopped
        .into_iter()
        .filter_map(|m| {
            let (stamp, fell_back) = match (recency.ended_at)(m.worktree, &m.row.session_id) {
                Some(end) => (Some(end), None),
                None => (m.row.started_at, Some(FELL_BACK_TO_START.to_string())),
            };
            match stamp {
                Some(t) if now_ms.saturating_sub(t) <= recency.window_ms => Some((m, t, fell_back)),
                _ => None,
            }
        })
        .collect();
    // Newest by the stamp each row was JUDGED on, so the row that answers is the
    // one that finished last and not the one that started last.
    match recent.iter().max_by_key(|(_, stamp, _)| *stamp) {
        // An ended row still answers for context: the transcript outlives the
        // process that wrote it (lessons claude-code C4).
        Some((newest, _, fell_back)) => {
            let mut seen = newest.seen(RosterState::Stopped);
            seen.recency_fallback = fell_back.clone();
            seen
        }
        None => {
            let mut nothing_recent = unmatched(seat);
            nothing_recent.pidless_row = a_pidless_row_stands;
            nothing_recent
        }
    }
}

#[derive(Clone, Copy)]
struct Match<'a> {
    row: &'a AgentRow,
    project: &'a str,
    worktree: &'a str,
}

impl Match<'_> {
    fn seen(&self, state: RosterState) -> SeatObservation {
        SeatObservation {
            state,
            unknown_cause: None,
            waiting_for: self.row.waiting_for.clone(),
            recency_fallback: None,
            session_id: Some(self.row.session_id.clone()),
            short_id: self.row.id.clone(),
            project: Some(self.project.to_string()),
            worktree: Some(self.worktree.to_string()),
            pidless_row: !self.row.is_live(),
            state_names_an_end: self.row.names_an_end(),
        }
    }
}

/// A seat no row matched. It still names where it would be found, but only when
/// that is unambiguous: a seat registered on several projects has no one answer,
/// and the fields are absent rather than guessed.
fn unmatched(seat: &Seat) -> SeatObservation {
    let (project, worktree) = match seat.worktrees.as_slice() {
        [(project, path)] => (Some(project.clone()), Some(path.clone())),
        _ => (None, None),
    };
    SeatObservation {
        state: RosterState::Absent,
        unknown_cause: None,
        waiting_for: None,
        recency_fallback: None,
        session_id: None,
        short_id: None,
        project,
        worktree,
        pidless_row: false,
        state_names_an_end: false,
    }
}

fn newest<'a, 'b>(rows: &'b [Match<'a>]) -> Option<&'b Match<'a>> {
    rows.iter().max_by_key(|m| m.row.started_at.unwrap_or(0))
}

// ------------------------------------------------------------------ transcript

#[derive(Deserialize)]
struct TranscriptEntry {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default)]
    message: Option<TranscriptMessage>,
    /// The provider's own machine-readable cause on an entry it wrote in place
    /// of a model turn. Absent on every ordinary entry.
    #[serde(default)]
    error: Option<String>,
    /// Whether the provider wrote this entry itself instead of the model
    /// answering.
    #[serde(default, rename = "isApiErrorMessage")]
    is_api_error: bool,
}

#[derive(Deserialize)]
struct TranscriptMessage {
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

/// Context for a session: the last main-chain assistant entry's input tokens
/// plus both cache figures.
///
/// There is no single context number in the file — the arithmetic over those
/// three fields is the reading (lessons claude-code C2). Sidechain entries are
/// skipped because a sidechain is a subagent's turn carrying the subagent's
/// window (C3); the flag is on every entry, so the skip is a filter and not an
/// inference.
///
/// An entry stating no window is skipped: no usage block, or a usage block
/// summing to zero. The agent writes a zero-usage entry for a turn that made no
/// model call, which is main-chain and does carry usage, so a plain last-entry
/// reader publishes 0 for a loaded session. The zero is the test rather than the
/// marker the agent puts on those entries, because a marker string that changes
/// republishes the 0 while a usage schema that moves yields no reading at all —
/// which every reader renders as blind instead.
///
/// A malformed line is skipped rather than fatal: transcripts are read while
/// they are being appended to, so a torn final line is an expected transient.
pub fn context_tokens_in(body: &str) -> Option<u64> {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<TranscriptEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
        .filter_map(|e| e.message)
        .filter_map(|m| m.usage)
        .map(|u| {
            u.input_tokens.unwrap_or(0)
                + u.cache_read_input_tokens.unwrap_or(0)
                + u.cache_creation_input_tokens.unwrap_or(0)
        })
        .filter(|&tokens| tokens != 0)
        .next_back()
}

/// The provider's own cause on the entry it writes in place of a first model
/// turn when the session has no credential (lessons claude-code A11).
pub const AUTHENTICATION_FAILED: &str = "authentication_failed";

/// Whether this transcript's first main-chain assistant entry is the provider's
/// LOGGED-OUT answer.
///
/// A seat started under its own configuration directory is logged out unless the
/// credential knob is defined-but-empty beside it, and the roster cannot say so:
/// such a session is LIVE and idle, with a pid and a state, exactly like one
/// mid-turn. The transcript is the only surface that carries the reading.
///
/// THREE TERMS, ALL REQUIRED: the entry is one the provider wrote itself, its
/// cause is [`AUTHENTICATION_FAILED`], and its window is zero. The conjunction
/// is the safe direction — a term that moves in a later release yields NO
/// reading, and a reading nobody has costs one uncaught logged-out seat, where a
/// looser match would fail a dispatch that was fine.
///
/// It reads the FIRST such entry and not the last: what is being asked is how
/// the session ANSWERED ITS FIRST TURN, and an entry further down belongs to a
/// session that was already working.
pub fn logged_out_first_turn(body: &str) -> bool {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<TranscriptEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
        .map(|e| {
            let window = e
                .message
                .and_then(|m| m.usage)
                .map(|u| {
                    u.input_tokens.unwrap_or(0)
                        + u.cache_read_input_tokens.unwrap_or(0)
                        + u.cache_creation_input_tokens.unwrap_or(0)
                })
                .unwrap_or(0);
            e.is_api_error && e.error.as_deref() == Some(AUTHENTICATION_FAILED) && window == 0
        })
        .next()
        .unwrap_or(false)
}

/// Whether this poll's reading of one row is a LOGGED-OUT DISPATCH, which is the
/// one line the controller writes about one.
///
/// Four terms, and each of them is a different reason not to write:
///
/// - `transient` — a named seat's session is the person's own and its login is
///   theirs to fix; only a dispatch can fail.
/// - `state` — the reading comes off a live row, because a logged-out session IS
///   live: it has a pid and an idle status, and only its transcript says
///   otherwise.
/// - `already_sighted` — once per ROW. The reading stands for as long as the
///   transcript does, and a line per poll is the noise the stream's content rule
///   is against.
/// - `transcript` — the reading itself, and `None` is a transcript that did not
///   resolve, which is a reading nobody has rather than a session that is fine.
pub fn logged_out_dispatch(
    transient: bool,
    state: RosterState,
    already_sighted: bool,
    transcript: Option<&str>,
) -> bool {
    transient
        && matches!(state, RosterState::Present | RosterState::PromptBlocked)
        && !already_sighted
        && transcript.map(logged_out_first_turn).unwrap_or(false)
}

/// How many turns a session took: main-chain assistant entries carrying a usage
/// block.
///
/// THE SAME FILTER AS [`context_tokens_in`], ONE STEP SHORTER. A turn that made
/// a model call is a turn whatever the call cost, so the zero-usage entry that
/// reader skips — the agent's line for a turn that called no model — is COUNTED
/// here: a session's turns and the window its last turn carried are two
/// different questions, and the second one is why that skip exists.
///
/// An entry with no usage block at all is not a turn the agent made a call for
/// and is not counted, which is the one thing the two readers agree to drop.
/// Sidechains are skipped for C3's reason: a sidechain is a subagent's turn and
/// the seat did not take it.
///
/// A malformed line is skipped rather than fatal, as it is there: a transcript
/// is read while it is being appended to, so a torn final line is expected.
pub fn turns_in(body: &str) -> u64 {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<TranscriptEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
        .filter_map(|e| e.message)
        .filter(|m| m.usage.is_some())
        .count() as u64
}
