//! What one poll saw: a seat's presence on the host and its activity as the
//! agent reads it.
//!
//! TWO READS, SPLIT BY QUESTION (ruling 3). Whether a seat's session is THERE
//! is the host's answer: the session fleet started for the seat exists and its
//! pane is alive, or it is dead and holds the exit status the agent left. What
//! the session is DOING — starting, idle, busy, stopped in front of a human —
//! is the agent's answer to `read`. Neither stands in for the other, and where
//! the two disagree the seat is Unknown with both pieces of evidence rather
//! than whichever one a rule happened to read first.

use crate::adapter::{self, dir_key, Activity, Agent, BlockedOn, SeatActivity, SeatRef};
use crate::config::Seat;
use crate::host::{self, HostRead, Pane, PaneState};
use fleet_core::seat::identity::SeatId;
use std::collections::BTreeMap;

/// How long a live pane the agent names no session for yet may claim to be
/// starting.
///
/// An interactive row was listed 0.5–0.75 s after its session was made
/// (measured on Claude Code 2.1.280, 2026-09-26), against a five-second poll,
/// so this is deliberately far larger than the phenomenon: being generous
/// costs a few polls before a session that never lists is noticed, and being
/// tight reads a newborn as a disagreement between the host and the agent.
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

    /// Whether a seat in this state has a session whose context answers for
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
    /// is the two of them disagreeing: what the host holds and what the agent
    /// answered.
    pub unknown_cause: Option<String>,
    /// What a seat stopped in front of a human waits on, as a person reads it
    /// ([`adapter::waiting_on`]): the agent's own sentence, carried verbatim
    /// and never matched on.
    pub waiting_for: Option<String>,
    /// What the seat waits on, typed, where the agent could name it — the one
    /// the logged-out line keys on ([`logged_out_dispatch`]).
    pub blocked_on: Option<BlockedOn>,
    /// What the agent says the session is doing. It is ACTIVITY and never
    /// presence: no liveness decision keys on it.
    pub activity: Option<Activity>,
    /// The session whose context answers for this seat, when there is one.
    pub session_id: Option<String>,
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

/// What core holds about a seat before it asks the agent: the session id a
/// read last answered and the configuration directory its session was started
/// under, both off the session table.
#[derive(Clone, Debug, Default)]
pub struct Held {
    pub session_id: Option<String>,
    pub config_dir: Option<String>,
}

/// Every seat's reading this poll, in the seats' order: the host's listing,
/// read once and shared, and ONE `read` of the agent about every seat whose
/// pane is alive (reviewer call 2026-09-25, E1; ruling 3).
///
/// Two seats must not decide against different readings of the same moment,
/// so the agent is asked about all of them at once — how it reads them, one
/// listing per configuration directory or otherwise, is the adapter's. A seat
/// with no live pane is asked about by nobody: its presence is already
/// answered, and an ended session's activity is nothing to read.
pub fn observe_fleet(
    agent: &dyn Agent,
    host: &HostRead,
    seats: &[Seat],
    held: &dyn Fn(&SeatId) -> Held,
    now_ms: u64,
) -> Vec<SeatObservation> {
    let asked: Vec<SeatRef> = seats
        .iter()
        .filter_map(|seat| {
            let pane = live_pane(host, seat)?;
            let held = held(&seat.id);
            Some(SeatRef {
                seat: seat.id,
                session_id: held.session_id,
                pid: pane.pid,
                config_dir: held.config_dir,
                worktree: placed(seat, &pane.path)
                    .1
                    .unwrap_or_else(|| pane.path.clone()),
                screen: None,
            })
        })
        .collect();
    let read: BTreeMap<SeatId, SeatActivity> = adapter::readings(agent, &asked)
        .into_iter()
        .map(|reading| (reading.seat, reading))
        .collect();
    seats
        .iter()
        .map(|seat| observe_seat(host, seat, read.get(&seat.id), now_ms))
        .collect()
}

/// The seat's live pane on the host, where the host was read and holds one.
fn live_pane<'h>(host: &'h HostRead, seat: &Seat) -> Option<&'h Pane> {
    let HostRead::Readable(panes) = host else {
        return None;
    };
    let session = host::session_for(&seat.id);
    panes
        .iter()
        .find(|pane| pane.session == session && pane.state == PaneState::Alive)
}

/// One seat's reading, from the host's listing and the agent's answer about it.
///
/// PRESENCE IS THE HOST'S. The seat's session is the one named by
/// [`host::session_for`] on fleet's own server, and nothing else is: a session
/// the host does not hold for the seat is not the seat's, wherever it stands,
/// and a seat the host holds nothing for is Absent whatever the agent has
/// running elsewhere — no seat is found by its working directory (CORRECTIONS
/// AT REVIEW, 2026-09-25; lessons claude-code B5). A dead pane is an END and
/// reads `Stopped` with the status it exited with.
///
/// ACTIVITY IS THE AGENT'S, and `reading` is its answer about the live pane:
/// found by the session's id, else by the pane's pid, since the pane's process
/// is the agent (E2).
///
/// WHERE THE TWO DISAGREE THE SEAT IS UNKNOWN, carrying both readings: a live
/// pane the agent names no session for once the start's grace is spent, and a
/// live pane beside a reading that could not be made. Neither is a seat to
/// spawn over and neither is a seat to believe present.
pub fn observe_seat(
    host: &HostRead,
    seat: &Seat,
    reading: Option<&SeatActivity>,
    now_ms: u64,
) -> SeatObservation {
    let session = host::session_for(&seat.id);
    let panes = match host {
        HostRead::Unreadable { cause } => return unknown(seat, None, cause.clone()),
        HostRead::Readable(panes) => panes,
    };
    let Some(pane) = panes.iter().find(|pane| pane.session == session) else {
        return unmatched(seat);
    };
    match pane.state {
        // An end, and the reading does not need the agent: an interactive
        // session is gone from its listing by the next read after its process
        // is (lessons claude-code B10), so there is nothing left to ask.
        PaneState::Dead { status } => {
            let mut stopped = unmatched(seat);
            stopped.state = RosterState::Stopped;
            stopped.pane_pid = pane.pid;
            stopped.exit_status = status;
            stopped
        }
        PaneState::Alive => hosted(reading, seat, &session, pane, now_ms),
    }
}

/// A seat whose pane is alive: a session the agent found is present — blocked
/// where it waits on a human — a young one it names no session for yet is
/// starting, and an older one it still names none for, or one it could not
/// read at all, is the two readings disagreeing.
fn hosted(
    reading: Option<&SeatActivity>,
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
    let Some(reading) = reading else {
        return unknown(
            seat,
            pane.pid,
            format!("{alive}, and the agent was not asked about it"),
        );
    };
    let found = reading.session_id.is_some();
    let live = match reading.activity {
        // Keyed on the activity being BLOCKED and never on what it names: an
        // unrecognised wait is still a seat that cannot act, and reading it as
        // healthy is the whole defect (lessons claude-code B8). The seat the
        // projection calls blocked and the one a typed turn refuses are the
        // same seat, because both read this one answer (`effect::type_turn`).
        Activity::Blocked => Some(RosterState::PromptBlocked),
        Activity::Idle | Activity::Busy => Some(RosterState::Present),
        // A session the agent FOUND whose activity it cannot say is still a
        // session there: a word the agent has no reading for is not a fifth
        // state to reason about, and never an absence.
        Activity::Starting | Activity::Unknown if found => Some(RosterState::Present),
        Activity::Starting | Activity::Unknown => None,
    };
    if let Some(state) = live {
        let (project, worktree) = placed(seat, &pane.path);
        let fallback = unmatched(seat);
        return SeatObservation {
            state,
            unknown_cause: None,
            waiting_for: adapter::waiting_on(reading),
            blocked_on: reading.blocked_on,
            activity: Some(reading.activity),
            session_id: reading.session_id.clone(),
            project: project.or(fallback.project),
            worktree: worktree.or(fallback.worktree),
            pane_pid: pane.pid,
            exit_status: None,
        };
    }
    let cause = reading
        .cause
        .clone()
        .unwrap_or_else(|| "the agent names no session for it".to_string());
    if reading.activity == Activity::Unknown {
        // Reviewer call 2026-09-25 (1): never Present on a reading nobody
        // made. Nothing is spawned beside a live pane or revived over it
        // either way.
        return unknown(seat, pane.pid, format!("{alive}, and {cause}"));
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
        starting.activity = Some(reading.activity);
        starting.pane_pid = pane.pid;
        return starting;
    }
    unknown(seat, pane.pid, format!("{alive}; {cause}"))
}

/// The project and worktree a seat's session stands in: the seat's worktree
/// the pane's own directory is, where it is one. Named, never guessed — a seat
/// on several projects whose pane stands in none of them has no one answer.
fn placed(seat: &Seat, path: &str) -> (Option<String>, Option<String>) {
    seat.worktrees
        .iter()
        .find(|(_, worktree)| dir_key(worktree) == dir_key(path))
        .map(|(project, worktree)| (Some(project.clone()), Some(worktree.clone())))
        .unwrap_or((None, None))
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
        blocked_on: None,
        activity: None,
        session_id: None,
        project,
        worktree,
        pane_pid: None,
        exit_status: None,
    }
}

// ------------------------------------------------------- the logged-out dispatch

/// The cause a logged-out dispatch's `dispatch.failed` line carries: the
/// provider's own word for the answer it gave in place of a first turn (lessons
/// claude-code A11), as the stream has always spelled it. The reading itself
/// is the adapter's — `blocked_on: logged_out` — and this is only the line's
/// word for it.
pub const AUTHENTICATION_FAILED: &str = "authentication_failed";

/// Whether this poll's reading of one seat is a LOGGED-OUT DISPATCH, which is
/// the one line the controller writes about one.
///
/// Four terms, and each of them is a different reason not to write:
///
/// - `transient` — a named seat's session is the person's own and its login is
///   theirs to fix; only a dispatch can fail.
/// - `state` — the reading comes off a live session, because a logged-out
///   session IS live: it has a pid and a prompt, and only the agent's reading
///   says otherwise.
/// - `already_sighted` — once per ROW. The reading stands for as long as the
///   session does, and a line per poll is the noise the stream's content rule
///   is against.
/// - `blocked_on` — the reading itself: the agent says the seat waits on a
///   login. Anything else, `None` included, is a seat that is not known to be
///   logged out.
pub fn logged_out_dispatch(
    transient: bool,
    state: RosterState,
    already_sighted: bool,
    blocked_on: Option<BlockedOn>,
) -> bool {
    transient
        && matches!(state, RosterState::Present | RosterState::PromptBlocked)
        && !already_sighted
        && blocked_on == Some(BlockedOn::LoggedOut)
}
