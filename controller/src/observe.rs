//! What one poll saw: a seat's presence on the host, its activity on the
//! agent's listing, and the context its session is carrying.
//!
//! TWO READS, SPLIT BY QUESTION (ruling 3). Whether a seat's session is THERE
//! is the host's answer: the session fleet started for the seat exists and its
//! pane is alive, or it is dead and holds the exit status the agent left. What
//! the session is DOING — starting, idle, busy, stopped in front of a human —
//! is the agent's listing's answer. Neither stands in for the other, and where
//! the two disagree the seat is Unknown with both pieces of evidence rather
//! than whichever one a rule happened to read first.

use crate::adapter::{dir_key, AgentRow, RosterRead};
use crate::config::Seat;
use crate::host::{self, HostRead, Pane, PaneState};
use fleet_core::seat::identity::SeatId;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// How long a live pane the listing does not name yet may claim to be starting.
///
/// An interactive row was listed 0.5–0.75 s after its session was made
/// (measured on 2.1.280, 2026-09-26), against a five-second poll, so this is
/// deliberately far larger than the phenomenon: being generous costs a few
/// polls before a session that never lists is noticed, and being tight reads a
/// newborn as a disagreement between the host and the listing.
pub const STARTING_GRACE_MS: u64 = 30_000;

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
    /// Set exactly when `state` is `Unknown`, and naming BOTH readings where it
    /// is the two of them disagreeing: what the host holds and what the listing
    /// names.
    pub unknown_cause: Option<String>,
    /// The agent's own cause string for a seat stopped in front of a human,
    /// carried verbatim and never matched on — or `status waiting` for a row
    /// that says it waits and names no cause (`AgentRow::blocked_on`).
    pub waiting_for: Option<String>,
    /// What the listing says the session is doing, in the agent's own word
    /// (`idle`, `busy`, `waiting` on 2.1.280), carried verbatim. It is ACTIVITY
    /// and never presence: no liveness decision keys on it, and a row that
    /// carries none is a reading nobody has rather than an idle session.
    pub activity: Option<String>,
    /// The session whose transcript answers for this seat, when there is one.
    pub session_id: Option<String>,
    /// That session's ADDRESS, which is a different value from its id (lessons
    /// claude-code A6). An interactive row carries none, so this is `None` on
    /// every seat the host runs.
    pub short_id: Option<String>,
    pub project: Option<String>,
    pub worktree: Option<String>,
    /// The pid of the pane the host holds for this seat — the agent itself,
    /// since nothing sits between (E2) — alive or dead. `None` where the host
    /// holds no session for the seat.
    pub pane_pid: Option<u32>,
    /// The status the pane's process exited with, on a `Stopped` seat; `None`
    /// on every other state, and on a pane that died with no status to report.
    pub exit_status: Option<i32>,
}

/// The listings one poll read: the fleet's own, and one per seat that comes up
/// under its own configuration directory.
///
/// A session started under its own configuration directory is LISTED under
/// that directory and under no other (lessons claude-code A11, B10) — the
/// fleet's listing answers as if the session did not exist. So a poll reads
/// once per distinct directory rather than once, and each seat is decided
/// against the listing that could see it.
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
    /// one directory. Both are seams so the fold is exercised without an agent:
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

/// One seat's reading, from the host's listing and the agent's.
///
/// PRESENCE IS THE HOST'S. The seat's session is the one named by
/// [`host::session_for`] on fleet's own server, and nothing else is: a session
/// the host does not hold for the seat is not the seat's, wherever it stands.
/// A dead pane is an END — a session the host keeps cannot hibernate or be
/// re-hosted under it — and reads `Stopped` with the status it exited with.
///
/// ACTIVITY IS THE LISTING'S, and a row is the seat's by its PID: the pane's
/// process is the agent (E2), so the row whose pid is the pane's is this
/// session and no other. A working directory names a seat and proves nothing
/// about who started the session in it (lessons claude-code B5), so no row is
/// attributed by where it stands.
///
/// WHERE THE TWO DISAGREE THE SEAT IS UNKNOWN, carrying both readings: a live
/// pane the listing names no row for once the start's grace is spent, a live
/// row in the seat's worktree with no session on the host behind it, and a
/// live pane beside a listing that could not be read. None of them is a seat
/// to spawn over and none of them is a seat to believe present.
pub fn observe_seat(
    read: &RosterRead,
    host: &HostRead,
    seat: &Seat,
    now_ms: u64,
) -> SeatObservation {
    let session = host::session_for(&seat.id);
    let panes = match host {
        HostRead::Unreadable { cause } => return unknown(seat, None, cause.clone()),
        HostRead::Readable(panes) => panes,
    };
    let Some(pane) = panes.iter().find(|pane| pane.session == session) else {
        return unhosted(read, seat, &session);
    };
    match pane.state {
        // An end, and the reading does not need the listing: an interactive
        // row is gone by the next read after its process is (lessons
        // claude-code B10), so there is no row left to read it from.
        PaneState::Dead { status } => {
            let mut stopped = unmatched(seat);
            stopped.state = RosterState::Stopped;
            stopped.pane_pid = pane.pid;
            stopped.exit_status = status;
            stopped
        }
        PaneState::Alive => hosted(read, seat, &session, pane, now_ms),
    }
}

/// A seat the host holds no session for: `Absent`, unless the listing names a
/// live session standing in one of its worktrees — which is a session fleet
/// does not host, and so not the seat's (B5), and not a seat to start a second
/// session beside either.
fn unhosted(read: &RosterRead, seat: &Seat, session: &str) -> SeatObservation {
    let rows = match read {
        // Nothing on the host, and nothing to say whether a live session stands
        // in the worktree regardless: a start into that is a second session,
        // so the seat is left unread rather than read absent.
        RosterRead::Unreadable { cause } => {
            return unknown(
                seat,
                None,
                format!(
                    "no tmux session {session} on {}, and the listing could not be read: {cause}",
                    host::SOCKET
                ),
            )
        }
        RosterRead::Readable(rows) => rows,
    };
    let standing = rows.iter().filter(|row| row.is_live()).find_map(|row| {
        seat.worktrees
            .iter()
            .find(|(_, path)| dir_key(path) == row.cwd_key())
            .map(|(_, path)| (row, path))
    });
    match standing {
        None => unmatched(seat),
        Some((row, worktree)) => unknown(
            seat,
            None,
            format!(
                "no tmux session {session} on {}; the listing names session {}, pid {}, in {}",
                host::SOCKET,
                row.session_id,
                row.pid.map(|pid| pid.to_string()).unwrap_or_default(),
                dir_key(worktree)
            ),
        ),
    }
}

/// A seat whose pane is alive: the listing's row with the pane's pid is its
/// activity, a young session the listing has not named yet is starting, and an
/// older one it still does not name is the two reads disagreeing.
fn hosted(
    read: &RosterRead,
    seat: &Seat,
    session: &str,
    pane: &Pane,
    now_ms: u64,
) -> SeatObservation {
    let alive = match pane.pid {
        Some(pid) => format!("tmux holds pid {pid} alive for {}", seat.machine_name()),
        None => format!(
            "tmux holds {session} alive for {} with no pid to read",
            seat.machine_name()
        ),
    };
    let rows = match read {
        // Reviewer call 2026-09-25 (1): never Present on a listing nobody read.
        // Nothing is spawned beside a live pane or revived over it either way.
        RosterRead::Unreadable { cause } => {
            return unknown(
                seat,
                pane.pid,
                format!("{alive}, and the listing could not be read: {cause}"),
            )
        }
        RosterRead::Readable(rows) => rows,
    };
    if let Some(row) = pane
        .pid
        .and_then(|pid| rows.iter().find(|row| row.pid == Some(pid)))
    {
        // PRESENCE of the field, never its value: an unrecognised cause is still
        // a seat that cannot act, and reading it as healthy is the whole defect
        // (lessons claude-code B8, and B10 on the interactive row). Read through
        // the row's one definition of a block, which a turn typed into the seat
        // reads too (`crate::effect::type_turn`), so the seat the projection
        // calls blocked and the one a turn refuses are the same seat — a
        // `waiting` status with no cause beside it included.
        let blocked = row.blocked_on();
        let state = match blocked {
            Some(_) => RosterState::PromptBlocked,
            None => RosterState::Present,
        };
        let (project, worktree) = placed(seat, &[row.cwd.as_str(), pane.path.as_str()]);
        return SeatObservation {
            state,
            unknown_cause: None,
            waiting_for: blocked,
            activity: row.status.clone(),
            session_id: Some(row.session_id.clone()),
            short_id: row.id.clone(),
            project,
            worktree,
            pane_pid: pane.pid,
            exit_status: None,
        };
    }
    // The age is the SESSION's, which the host counts in whole seconds: a start
    // is younger than the grace for every read inside it, and one the host
    // cannot date is no newborn anybody can vouch for.
    let young = pane
        .created_ms
        .is_some_and(|created| now_ms.saturating_sub(created) < STARTING_GRACE_MS);
    if young {
        let mut starting = unmatched(seat);
        starting.state = RosterState::Starting;
        starting.pane_pid = pane.pid;
        return starting;
    }
    unknown(
        seat,
        pane.pid,
        match pane.pid {
            Some(_) => format!("{alive}; the listing names no row with that pid"),
            None => format!("{alive}; the listing can name no row for it"),
        },
    )
}

/// The project and worktree a seat's session stands in: the first of `paths`
/// that one of the seat's worktrees is, and the seat's one worktree where none
/// is. Named, never guessed — the row's directory and the pane's are the two
/// readings of where the session is, and a seat on several projects with
/// neither matching has no one answer.
fn placed(seat: &Seat, paths: &[&str]) -> (Option<String>, Option<String>) {
    paths
        .iter()
        .find_map(|path| {
            seat.worktrees
                .iter()
                .find(|(_, worktree)| dir_key(worktree) == dir_key(path))
        })
        .map(|(project, worktree)| (Some(project.clone()), Some(worktree.clone())))
        .unwrap_or_else(|| {
            let fallback = unmatched(seat);
            (fallback.project, fallback.worktree)
        })
}

/// A seat nobody could read, with why — both readings, where it is the two of
/// them disagreeing.
fn unknown(seat: &Seat, pane_pid: Option<u32>, cause: String) -> SeatObservation {
    let mut unknown = unmatched(seat);
    unknown.state = RosterState::Unknown;
    unknown.unknown_cause = Some(cause);
    unknown.pane_pid = pane_pid;
    unknown
}

/// A seat no session answers for. It still names where it would be found, but
/// only when that is unambiguous: a seat registered on several projects has no
/// one answer, and the fields are absent rather than guessed.
fn unmatched(seat: &Seat) -> SeatObservation {
    let (project, worktree) = match seat.worktrees.as_slice() {
        [(project, path)] => (Some(project.clone()), Some(path.clone())),
        _ => (None, None),
    };
    SeatObservation {
        state: RosterState::Absent,
        unknown_cause: None,
        waiting_for: None,
        activity: None,
        session_id: None,
        short_id: None,
        project,
        worktree,
        pane_pid: None,
        exit_status: None,
    }
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
