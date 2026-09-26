//! Carrying a verdict out.
//!
//! Every act here writes its own event, once, at the layer that did the thing —
//! so a person reading the stream in the morning sees the request, its
//! collection and its outcome as three lines rather than as silence.
//!
//! Every verdict the table reaches is acted on here. `halt` is the one whose act
//! is to dispatch nothing: it is published as its own outcome, and its line and
//! its event are written once, at the transition into the hold.

use crate::adapter::{Agent, AgentRow, RemoveAnswer, RosterRead, StartSpec};
use crate::events::{self, ActorRef, EventLog};
use crate::host::{self, Host, HostRead, PaneState};
use crate::policy::Policy;
use crate::sessions::{SessionRow, Table};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What an effect DID, for the projection's row. Never a liveness claim: a
/// start that returned OK is one the listing showed as it came up, and whether
/// it is still there is the next poll's answer, not this one's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Spawned,
    Rested,
    Nudged,
    /// An attach was DISPATCHED against a pid-less row. Never a claim the row
    /// came back: the call exits 0 either way (lessons claude-code A7).
    Revived,
    /// The seat is held down by the blind guard, and this poll did nothing
    /// about it on purpose.
    Halted,
    /// The verdict was reached and deliberately not carried out, because its
    /// effect belongs to a later slice. Distinct from `None`, which is a policy
    /// that chose to do nothing: a reader has to be able to tell a quiet fleet
    /// from one nobody is acting on.
    Deferred,
    Failed,
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::None => "none",
            Outcome::Spawned => "spawned",
            Outcome::Rested => "rested",
            Outcome::Nudged => "nudged",
            Outcome::Revived => "revived",
            Outcome::Halted => "halted",
            Outcome::Deferred => "deferred",
            Outcome::Failed => "failed",
        }
    }
}

/// Everything one seat's effect needs to know about that seat, gathered by the
/// caller so this layer reads nothing for itself.
pub struct Target<'a> {
    /// The seat, and the actor every line this effect writes carries.
    pub seat: SeatId,
    /// The name the seat's session answers to: the one its newest session row
    /// recorded at start, and the seat's machine name where no row names one
    /// (`sessions::Table::session_name`). It is the start's `--name`, the first
    /// turn's argument and the address of every nudge — never an empty string,
    /// which would put an empty element in argv and make `--name` swallow the
    /// flag after it.
    pub session_name: String,
    pub project: &'a str,
    pub worktree: &'a str,
    pub model: String,
    pub posture: String,
    pub first_turn: String,
    pub transient: bool,
    /// The configuration directory this start comes up under, when it comes up
    /// under its own. `None` takes the fleet's.
    pub config_dir: Option<String>,
    /// The work item this dispatch gave the seat, where an order named one.
    pub item: Option<String>,
    /// What the start's permission document did, where the caller wrote one:
    /// `written` where the worktree carried none, `merged` where it carried a
    /// document of the project's own that the pack's rules were folded into.
    /// `None` is a start that wrote no permission document at all.
    pub settings: Option<String>,
    /// The load belt's own readings, where the caller ran one, as
    /// `transient::Belt::payload` shapes them. It rides as a value and not as a
    /// belt: the caller that measured the machine is the one that knows the
    /// shape, and this layer only carries what it was handed onto the stream.
    /// `None` is a start no belt was run for, and writes `null`.
    pub belt: Option<serde_json::Value>,
    /// The run that spawned this seat, where one did.
    /// `None` is a seat spawned outside a run, and the run's own cleanup passes
    /// over it — which is what makes the key a selector and not a label.
    pub run: Option<String>,
    /// The live session's id and its ADDRESS, when there is one (A6).
    pub session_id: Option<&'a str>,
    pub short_id: Option<&'a str>,
    pub context_tokens: Option<u64>,
}

impl Target<'_> {
    /// The directory every act about this session is made under: the one the
    /// start named, or the fleet's.
    fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref().map(Path::new)
    }
}

/// Start a woken session for this seat, and open its row.
///
/// On a failed start there is NO ROW and NO SESSION. A row for a session that
/// never came up is a dispatch the arrival window then waits on forever — so
/// the failure is an event and nothing else, the session is killed on the
/// host, and the next poll finds the seat absent and eligible again.
pub fn spawn_woken(
    agent: &dyn Agent,
    host: &dyn Host,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
    table: &mut Table,
    now_ms: u64,
) -> Outcome {
    match start_once(agent, host, policy, target, events_log) {
        Ok(dispatch_id) => {
            open_row(table, target, dispatch_id, now_ms);
            Outcome::Spawned
        }
        Err(_) => Outcome::Failed,
    }
}

/// Open the row this dispatch produced. Nothing here is a sighting: the four
/// sighting fields stay absent until the roster shows the session (A7).
fn open_row(table: &mut Table, target: &Target, dispatch_id: String, now_ms: u64) {
    table.push(SessionRow {
        seat: target.seat.to_string(),
        project: target.project.to_string(),
        worktree: target.worktree.to_string(),
        name: target.session_name.clone(),
        model: target.model.clone(),
        posture: target.posture.clone(),
        first_turn: target.first_turn.clone(),
        transient: target.transient,
        config_dir: target.config_dir.clone(),
        item: target.item.clone(),
        dispatch_id,
        dispatched_at: now_ms,
        session_id: None,
        short_id: None,
        first_seen_at: None,
        last_seen_at: None,
        adopted: None,
    });
}

/// The sentence one nudge carries (lessons claude-code C5).
///
/// It names the reading, the threshold it crossed and the command that answers
/// it, because a suggestion whose recipient has to go and look up all three is
/// one that costs more attention than it saves. It suggests and never enforces:
/// there is deliberately no path from the threshold to an automatic rest. It is
/// the whole of what is typed into the seat's session ([`type_turn`]): no turn
/// of anybody else's carries it.
///
/// The seat is named by its session's name in both places. The command's
/// argument resolves through the machine-name rule, which matches on the id
/// part alone, so it names the seat even after the seat is renamed.
pub fn nudge_text(session_name: &str, tokens: u64, threshold: u64, seat: &str) -> String {
    format!(
        "{session_name}: context at {tokens} tokens, over the rest threshold {threshold} — \
         rest when your work allows: fleet event rest {seat} --reason <why>"
    )
}

/// What a rest did, so the caller can log the half that failed.
pub enum Rested {
    /// The predecessor was stopped, the successor started and the predecessor's
    /// row removed. The rest is collected and the event is consumed.
    Collected,
    /// The stop did not exit 0. Nothing was started and nothing was removed, the
    /// rest stays pending, and the next poll retries. A `seat.resting` with no
    /// `session.rested` after it is the alarm, and this is exactly that.
    StopFailed(String),
    /// The stop exited 0 and the successor's start failed. The predecessor is
    /// down, its row still stands, nothing was removed and the rest stays pending.
    StartFailed(String),
    /// There was nothing to stop: a rest is defined on a live row and this seat
    /// has no address to issue against.
    NoAddress,
}

pub const PHASE_START: &str = "start";

/// A `session.crashed` payload, written by the layer that met the failure:
/// the cause, the file holding the session's last screen where one was kept,
/// and the pane's exit status where it died with one.
pub fn crashed_payload(
    phase: &str,
    cause: &str,
    output: Option<&str>,
    status: Option<i32>,
) -> serde_json::Value {
    serde_json::json!({ "phase": phase, "cause": cause, "output": output, "status": status })
}

/// Names the event types an EFFECT writes, so a reader of the stream and a
/// reader of this file agree on the vocabulary.
///
/// `session.stopped` is on the list and is written by `crate::transient`, which
/// is the other caller of this module's `spawn_woken`: a retire is an effect
/// carried out the same way, and a list that named only this file's own appends
/// would leave the one end-of-life line a reader could not find here.
/// `session.retired` is on it for the same reason, one writer over: it is the
/// priced retire's own line and the cost half of the same act.
pub const WRITES: [&str; 9] = [
    events::SESSION_SPAWNED,
    events::SESSION_RESTED,
    events::SESSION_NUDGED,
    events::SESSION_CRASHED,
    events::SESSION_REVIVED,
    events::SESSION_HALTED,
    events::SESSION_STOPPED,
    events::SESSION_RETIRED,
    events::DISPATCH_BLIND,
];

/// Where a start's capture goes, under the machine directory: the pane's text
/// as it stood when the start was given up on, which is the only account of
/// why a session that never listed did not.
pub const STARTS_DIR: &str = "starts";

/// How often a start's watch reads the pane and the listing. The listing
/// answered in 100–160 ms (lessons claude-code B10) and an interactive row
/// was listed 0.5–0.75 s after the session was made, carrying its status half
/// a second later (measured on 2.1.280, 2026-09-26), so a quarter second
/// believes a start within one read of its row and costs a handful of reads.
pub const WATCH_TICK: Duration = Duration::from_millis(250);

/// Where one call's words go under the machine directory: named by the
/// session and the moment, so two calls for one seat never write over each
/// other and an operator reading the directory can tell which is which.
pub fn log_path(machine_dir: &Path, dir: &str, session_name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let safe: String = session_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    machine_dir.join(dir).join(format!("{safe}-{stamp}.log"))
}

/// Where a start's capture is kept: `<machine>/starts/<name>-<ms>.log`.
pub fn start_capture_path(machine_dir: &Path, session_name: &str) -> PathBuf {
    log_path(machine_dir, STARTS_DIR, session_name)
}

/// What a start's watch saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Watched {
    /// The listing shows a row whose pid is the pane's and which carries a
    /// status (lessons claude-code B10): the agent is up, and up as this
    /// pane's process. `trust_answered` is a start whose screen stopped at the
    /// workspace-trust question and was answered from it — the fallback the
    /// seeded acceptance should leave unused, recorded so a reader of the
    /// stream sees that it was not.
    Started { trust_answered: bool },
    /// The pane died, the session went, or the window closed with no row. The
    /// session HAS BEEN KILLED — a failed start leaves nothing on the host — and
    /// `screen` is its last capture where one could be taken.
    Failed {
        cause: String,
        /// The pane's exit status where it died with one.
        status: Option<i32>,
        screen: Option<String>,
    },
}

/// Watch a session the host has just started until the agent's own listing
/// shows it, reading the pane and the listing every [`WATCH_TICK`] until
/// `window` closes.
///
/// A DEAD PANE is a failed start carrying its exit status (the pane keeps it,
/// remain-on-exit), which replaces the in-band exit a background start used to
/// report (lessons claude-code A14, retired). A LIVE PANE is believed only when
/// a listed row carries its pid and a status — the pid, because the pane's
/// process IS the agent (E2) and a row in the same worktree proves nothing
/// (B5); the status, because a row is listed half a second before it carries
/// one and a start is not taken until it does (B10). Neither inside the window
/// is a failure too: a session that runs and never lists is one no read will
/// ever see.
///
/// `answer_trust` is a start into a worktree fleet created, and only such a
/// start may answer the agent's workspace-trust question from the screen
/// (ruling 13). The screen is read only while no row is listed, and answered
/// at most once.
///
/// Every `Failed` kills the session first: its capture is taken, then the
/// session ended, so a failed start's rollback finds nothing left on the host.
pub fn watch_start(
    agent: &dyn Agent,
    host: &dyn Host,
    session: &str,
    config_dir: Option<&Path>,
    answer_trust: bool,
    window: Duration,
) -> Watched {
    let deadline = Instant::now() + window;
    let mut trust_answered = false;
    let failed = |cause: String, status: Option<i32>| {
        let screen = host.capture(session).ok();
        let _ = host.kill(session);
        Watched::Failed {
            cause,
            status,
            screen,
        }
    };
    loop {
        // An unreadable host listing concludes nothing: the pane may be fine
        // and the read not, and the window is what bounds the wait.
        if let HostRead::Readable(panes) = host.list() {
            match panes.into_iter().find(|pane| pane.session == session) {
                None => {
                    return failed(format!("the session {session} is gone from the host"), None)
                }
                Some(pane) => match pane.state {
                    PaneState::Dead { status } => {
                        return failed(
                            format!(
                                "the session exited {} inside its {}s watch window",
                                status
                                    .map(|code| code.to_string())
                                    .unwrap_or_else(|| "on a signal".to_string()),
                                window.as_secs()
                            ),
                            status,
                        )
                    }
                    PaneState::Alive => {
                        if let (Some(pid), RosterRead::Readable(rows)) =
                            (pane.pid, agent.status(config_dir))
                        {
                            if rows
                                .iter()
                                .any(|row| row.pid == Some(pid) && row.status.is_some())
                            {
                                return Watched::Started { trust_answered };
                            }
                        }
                        if answer_trust && !trust_answered {
                            if let Some(keys) = host
                                .capture(session)
                                .ok()
                                .and_then(|screen| agent.trust_keys(&screen))
                            {
                                let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                                trust_answered = host.keys(session, &keys).is_ok();
                            }
                        }
                    }
                },
            }
        }
        let now = Instant::now();
        if now >= deadline {
            return failed(format!("no listed row within {}s", window.as_secs()), None);
        }
        std::thread::sleep(WATCH_TICK.min(deadline - now));
    }
}

/// One start, its event, and the id the row is keyed on.
///
/// The adapter answers what to run ([`Agent::launch`]), the host runs it as the
/// seat's own session ([`host::session_for`]), and [`watch_start`] decides
/// whether it came up. A session already on the host under the seat's name is
/// looked at first: a DEAD one — a pane kept after its agent ended — is
/// cleared, because it is no session and it holds the name; a LIVE one is a
/// start refused, never a session killed, because this start did not make it.
pub fn start_once(
    agent: &dyn Agent,
    host: &dyn Host,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
) -> Result<String, String> {
    // The plugin root comes off POLICY and not off the target: every start the
    // controller makes goes through this one construction, so a second builder
    // of a target cannot forget it and the two cannot disagree.
    let spec = StartSpec {
        seat: target.seat.to_string(),
        worktree: target.worktree.to_string(),
        name: target.session_name.clone(),
        actor: Actor::seat(target.seat).to_string(),
        model: target.model.clone(),
        posture: target.posture.clone(),
        first_turn: target.first_turn.clone(),
        plugin_dir: policy
            .plugin_dir
            .as_ref()
            .map(|dir| dir.display().to_string()),
        config_dir: target.config_dir.clone(),
    };
    let session = host::session_for(&target.seat);
    let window = Duration::from_secs(policy.start_watch_seconds);
    let watched = match cleared(host, &session)
        .and_then(|()| agent.launch(&spec))
        .and_then(|launch| {
            host.new_session(
                &session,
                Path::new(target.worktree),
                &launch.argv,
                &launch.env,
            )
            .map_err(|cause| format!("the host did not start the session: {cause}"))
        }) {
        // Nothing was started, so nothing is killed: a refusal here may be
        // over a live session that is not this start's to end.
        Err(cause) => Watched::Failed {
            cause,
            status: None,
            screen: None,
        },
        Ok(()) => watch_start(
            agent,
            host,
            &session,
            target.config_dir(),
            target.config_dir.is_some(),
            window,
        ),
    };
    match watched {
        Watched::Started { trust_answered } => {
            let payload = serde_json::json!({
                "worktree": target.worktree,
                "project": target.project,
                "name": target.session_name,
                "model": target.model,
                "posture": target.posture,
                "first_turn": target.first_turn,
                "transient": target.transient,
                "config_dir": target.config_dir,
                "item": target.item,
                "settings": target.settings,
                "belt": target.belt,
                "run": target.run,
                // The session's words are in its pane, so the output a reader
                // is pointed at is the session itself, on fleet's own server.
                "output": format!("-L {} -t {session}", host::SOCKET),
                "trust_answered": trust_answered,
            });
            Ok(append(
                events_log,
                events::SESSION_SPAWNED,
                &ActorRef::seat(target.seat),
                payload,
            ))
        }
        Watched::Failed {
            cause,
            status,
            screen,
        } => {
            let output = screen
                .and_then(|screen| keep_capture(events_log.dir()?, &target.session_name, &screen));
            append(
                events_log,
                events::SESSION_CRASHED,
                &ActorRef::seat(target.seat),
                crashed_payload(PHASE_START, &cause, output.as_deref(), status),
            );
            Err(cause)
        }
    }
}

/// The seat's name on the host made free for a start, or why it cannot be.
///
/// A dead pane under it is killed — its agent is gone and the pane is only its
/// last screen — and a live one refuses the start. An unreadable host listing
/// is left to the start itself, which fails at the host in the host's own
/// words if the name is taken.
fn cleared(host: &dyn Host, session: &str) -> Result<(), String> {
    let HostRead::Readable(panes) = host.list() else {
        return Ok(());
    };
    match panes.iter().find(|pane| pane.session == session) {
        None => Ok(()),
        Some(pane) if pane.state == PaneState::Alive => Err(format!(
            "the session {session} is already running on the host, and this start did not \
             start it"
        )),
        Some(_) => host
            .kill(session)
            .map_err(|cause| format!("the dead session {session} could not be cleared: {cause}")),
    }
}

/// Write a start's last capture under the machine directory and answer where,
/// or `None` where it could not be kept — the start has failed either way, and
/// a capture that could not be written is not a second failure.
fn keep_capture(machine_dir: &Path, session_name: &str, screen: &str) -> Option<String> {
    let path = start_capture_path(machine_dir, session_name);
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, screen).ok()?;
    Some(path.display().to_string())
}

/// The rest collection, in a fixed order: stop, start the successor, then
/// remove the predecessor.
///
/// THE ORDER IS LOAD-BEARING AND THE REMOVAL IS ONLY EVER AFTER A SUCCESSFUL
/// STOP. A remove aimed at a live row neither refuses nor spares it, and a stop
/// addressed by the full session id exits 1 with the row untouched — so a
/// removal that follows an unread stop deletes a row whose session is still
/// running (lessons claude-code A6, A8).
///
/// The successor is started BEFORE the predecessor's row is removed, so a
/// removal that refuses leaves the seat with a successor rather than with
/// nothing.
pub fn rest(
    agent: &dyn Agent,
    host: &dyn Host,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
    table: &mut Table,
    now_ms: u64,
) -> Rested {
    let Some(short_id) = target.short_id else {
        return Rested::NoAddress;
    };
    if let Err(cause) = agent.stop(target.config_dir(), short_id) {
        return Rested::StopFailed(cause);
    }
    let started = start_once(agent, host, policy, target, events_log);
    let dispatch_id = match started {
        Ok(id) => id,
        // The stop landed and the start did not. The predecessor is down, its
        // row still stands, and the crash event says so; the removal is not
        // taken, because a row nothing replaced is the only trace of the seat.
        Err(cause) => return Rested::StartFailed(cause),
    };
    let removed = match agent.remove(target.config_dir(), short_id) {
        RemoveAnswer::Removed => "removed".to_string(),
        RemoveAnswer::Refused { cause } => format!("refused: {cause}"),
        RemoveAnswer::RemovedAWorktree { path } => format!("removed a worktree at {path}"),
    };
    open_row(table, target, dispatch_id.clone(), now_ms);
    if let Some(session_id) = target.session_id {
        table.forget(session_id);
    }
    append(
        events_log,
        events::SESSION_RESTED,
        &ActorRef::seat(target.seat),
        serde_json::json!({
            "predecessor": target.session_id,
            "predecessor_address": short_id,
            "successor_dispatch": dispatch_id,
            "removed": removed,
        }),
    );
    Rested::Collected
}

/// Bring a hibernated row back in place.
///
/// The attach is addressed by the row's SHORT ID, so the session continues under
/// the same id with its context intact — a flagged resume would fork it (lessons
/// claude-code A9). The row is then RE-OPENED as a fresh dispatch: an attach
/// exits 0 whether it revived the row or did nothing (A7), so the arrival window
/// has to be keyed to this act and answered by a sighting, or the revive would be
/// issued again on every poll.
///
/// A pid-less row with no short id cannot be revived and says so, doing nothing:
/// the row is left to the next poll rather than spawned over here.
pub fn revive(
    agent: &dyn Agent,
    target: &Target,
    events_log: &mut EventLog,
    table: &mut Table,
    now_ms: u64,
) -> Outcome {
    let (Some(short_id), Some(session_id)) = (target.short_id, target.session_id) else {
        eprintln!(
            "fleet observe: {} is due a revive and its row carries no short id, which is the \
             address an attach takes; nothing is done and the row is left to the next poll",
            target.session_name
        );
        return Outcome::None;
    };
    let attached = agent.revive(target.config_dir(), short_id);
    let outcome = match &attached {
        Ok(()) => "dispatched".to_string(),
        Err(cause) => format!("failed: {cause}"),
    };
    let dispatch_id = append(
        events_log,
        events::SESSION_REVIVED,
        &ActorRef::seat(target.seat),
        serde_json::json!({
            "session": session_id,
            "address": short_id,
            "worktree": target.worktree,
            "project": target.project,
            // The identity fields every other row-opening line carries: an
            // attach is its own dispatch, so a rebuild that meets this line
            // with no row to match — a trimmed stream — opens one from these
            // rather than from empty strings.
            "name": target.session_name,
            "model": target.model,
            "posture": target.posture,
            "first_turn": target.first_turn,
            "transient": target.transient,
            "outcome": outcome,
        }),
    );
    match attached {
        Ok(()) => {
            // A row this controller did not open — a session it adopted, or one
            // it met on the roster — gets one now: the attach is its own
            // dispatch either way, and a dispatch with no row is a window that
            // never opens and a revive re-issued every poll.
            if !table.redispatch(session_id, dispatch_id.clone(), now_ms) {
                open_row(table, target, dispatch_id, now_ms);
            }
            Outcome::Revived
        }
        // A failed attach opened no window and moved no row, exactly as a failed
        // start opens none: the next poll finds the same pid-less row and
        // decides about it again.
        Err(_) => Outcome::Failed,
    }
}

/// Claim, at startup, every session the table names that the roster still
/// LISTS.
///
/// LISTED AND NOT ENDED, and never the pid: a row the daemon carries without one
/// is a session it still holds — hibernated, or between hosts — and a session
/// the table already names is this controller's to claim whichever of the two it
/// is, because adoption issues nothing and costs nothing. Only the agent's own
/// end-of-life marker puts a row out of reach, which leaves a session that has
/// finished to the discriminator. Claiming on the pid instead hands every
/// pid-less row of a session the fleet already owns to the revive arm, which
/// spends a dispatch re-attaching to a session that is already there.
///
/// BY SESSION ID, never by name and never by re-issuing a start: a controller
/// that resumed with its own flags would fork the session it meant to reclaim
/// and then hold a row pointing at a dead twin (lessons claude-code A9). Nothing
/// is dispatched here — the row is marked sighted from the roster this poll
/// already read, so the seat's first verdict is taken against a session the
/// controller knows it owns.
///
/// One `session.adopted` each, which is the line the reference engine adopts
/// without (lessons gas-city G7), and ONCE PER SESSION rather than once per
/// process: a row already adopted as its session id is not claimed again, so a
/// restart — or a `--once` poll, which is a process per poll — writes nothing
/// for a session an earlier start claimed.
///
/// A row is claimed when it is LIVE and never by its state (gas-city G7:
/// "adopts every live session it names"). The state word cannot carry the
/// question — a live idle session reads `done` (lessons claude-code A3), so a
/// claim keyed on it takes no idle seat at all — and liveness is a sighting
/// this controller made: what is claimed is a session observed running, which
/// is then held through the pid-less stretches that follow it. A session the
/// roster no longer carries, and one it carries pid-less with no sighting
/// behind it, are both left alone: those rows fall to the discriminator on this
/// same poll.
pub fn adopt(
    rows: &[crate::adapter::AgentRow],
    table: &mut Table,
    events_log: &mut EventLog,
    now_ms: u64,
) -> Vec<String> {
    let mut claimed = Vec::new();
    for row in &mut table.sessions {
        let Some(session_id) = row.session_id.clone() else {
            continue;
        };
        if row.adopted.as_ref() == Some(&session_id) {
            continue;
        }
        let Some(listed) = rows
            .iter()
            .find(|listed| listed.session_id == session_id && listed.is_live())
        else {
            continue;
        };
        row.first_seen_at.get_or_insert(now_ms);
        row.last_seen_at = Some(now_ms);
        if row.short_id.is_none() {
            row.short_id = listed.id.clone();
        }
        row.adopted = Some(session_id);
        claimed.push(row.clone());
    }
    let mut ids = Vec::with_capacity(claimed.len());
    for row in claimed {
        let session_id = row.session_id.clone().unwrap_or_default();
        append(
            events_log,
            events::SESSION_ADOPTED,
            &ActorRef::seat(&row.seat),
            serde_json::json!({
                "session": session_id,
                "short_id": row.short_id,
                "worktree": row.worktree,
                "project": row.project,
                "name": row.name,
                "model": row.model,
                "posture": row.posture,
                "first_turn": row.first_turn,
                "transient": row.transient,
            }),
        );
        ids.push(session_id);
    }
    ids
}

/// The one line and the one event a halt transition writes.
///
/// Announced ONCE per transition into the halt and never once per poll: the
/// caller writes this only when the latch moved. The line names the seat by its
/// machine name, which is what a person types back; the event's actor is the
/// seat, by its id.
pub fn halted(seat: &SeatId, machine_name: &str, blind: u32, events_log: &mut EventLog) {
    eprintln!(
        "fleet observe: {machine_name} has gone blind on {blind} consecutive dispatches and is \
         HELD DOWN; nothing further is dispatched for it until `fleet event clear-halt \
         {machine_name}`"
    );
    append(
        events_log,
        events::SESSION_HALTED,
        &ActorRef::seat(seat),
        serde_json::json!({ "blind": blind }),
    );
}

/// The event a counted blind dispatch writes, carrying the count it moved to —
/// which is what a rebuild folds the counter back out of.
pub fn blind_dispatch(seat: &SeatId, blind: u32, verdict: &str, events_log: &mut EventLog) {
    append(
        events_log,
        events::DISPATCH_BLIND,
        &ActorRef::seat(seat),
        serde_json::json!({ "blind": blind, "verdict": verdict }),
    );
}

/// One nudge, its event, and the session id that must never be nudged again.
///
/// The sentence is TYPED into the seat's own session ([`type_turn`]) under the
/// policy's bound. `sent` is written only where the listing witnessed the turn
/// taken; every other outcome says what it was instead.
pub fn nudge(
    agent: &dyn Agent,
    host: &dyn Host,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
    table: &mut Table,
) -> Outcome {
    let (Some(session_id), Some(tokens)) = (target.session_id, target.context_tokens) else {
        return Outcome::None;
    };
    let text = nudge_text(
        &target.session_name,
        tokens,
        policy.rest_threshold_tokens,
        &target.session_name,
    );
    let typed = type_turn(
        agent,
        host,
        &TurnTarget {
            seat: &target.seat,
            config_dir: target.config_dir(),
        },
        &text,
        Duration::from_secs(policy.nudge_timeout_seconds),
    );
    // The session is marked WHATEVER BECAME OF THE TURN. The budget is one
    // nudge per session and a retry loop against a session that cannot be
    // reached is the noise that budget exists to prevent; the event carries the
    // outcome for the person who reads the stream.
    let seat = target.seat.to_string();
    table.mark_nudged(&seat, session_id);
    append(
        events_log,
        events::SESSION_NUDGED,
        &ActorRef::seat(&seat),
        serde_json::json!({
            "session": session_id,
            "context_tokens": tokens,
            "threshold": policy.rest_threshold_tokens,
            "outcome": typed.recorded(),
        }),
    );
    match typed {
        Typed::Delivered | Typed::Queued => Outcome::Nudged,
        Typed::Blocked(_) | Typed::Absent | Typed::Failed(_) => Outcome::Failed,
    }
}

// ---- typing a turn ----------------------------------------------------------

/// The seat a turn is typed for: its session on the host is named by its id
/// ([`host::session_for`]), and its row is listed under `config_dir`, the
/// directory its session was started under (`None` for the adapter's own).
pub struct TurnTarget<'a> {
    pub seat: &'a SeatId,
    pub config_dir: Option<&'a Path>,
}

/// What typing one turn into a seat's session came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    /// The row was idle, the turn was typed, and the row turned busy on the
    /// same pid inside the bound: the session took it.
    Delivered,
    /// The row was already busy, so the turn was typed and WAITS behind the one
    /// in hand. Never called delivered (reviewer call 2026-09-25, E5): a busy
    /// row after the send cannot tell the queued turn from the one ahead of it.
    /// On Claude Code 2.1.280 a turn typed into a busy session ran as its own
    /// turn once the first had ended (measured 2026-09-26).
    Queued,
    /// The row stood in front of a human, carrying what on, and NOTHING WAS
    /// TYPED: keys sent at a dialog answer it (lessons claude-code B8, B10). On
    /// 2.1.280 a paste and a submit at a permission prompt lost the text and
    /// approved the tool call (measured 2026-09-26).
    Blocked(String),
    /// No live pane under the seat's session, or no listed row carrying the
    /// pane's pid: there is no session to type into, and nothing was.
    Absent,
    /// Why the turn was not taken: a reading that could not be made, a host
    /// that refused the text, or a row that never turned busy.
    Failed(String),
}

impl Typed {
    /// What a `session.nudged` line records for this outcome: `sent` for a
    /// witnessed turn and for nothing else.
    pub fn recorded(&self) -> String {
        match self {
            Typed::Delivered => "sent".to_string(),
            Typed::Queued => "queued: the seat was mid-turn".to_string(),
            Typed::Blocked(cause) => format!("refused: blocked on {cause}"),
            Typed::Absent => "failed: the seat has no live session to type into — no live pane, \
                              or no listed row carrying its pid"
                .to_string(),
            Typed::Failed(cause) => format!("failed: {cause}"),
        }
    }
}

/// How often a typed turn's listing is read for the row turning busy. On Claude
/// Code 2.1.280 the row read busy on the first read after the submit, 0.14 s
/// on, and a one-word turn held busy for half a second and more (measured
/// 2026-09-26), so a quarter second meets even the shortest turn.
pub const TURN_TICK: Duration = WATCH_TICK;

/// The seat's live pane and the listed row carrying its pid, read FRESH: the
/// host's listing, then the agent's under the seat's own directory.
///
/// The row is found BY THE PANE'S PID and by nothing else — the pane's process
/// IS the agent (E2), and a row in the same worktree proves nothing (B5). No
/// pane, a dead one, or no row carrying its pid is [`Typed::Absent`]; a listing
/// that could not be read is [`Typed::Failed`] naming it, never an absence.
pub fn seat_row(
    agent: &dyn Agent,
    host: &dyn Host,
    target: &TurnTarget,
) -> Result<AgentRow, Typed> {
    let session = host::session_for(target.seat);
    let panes = match host.list() {
        HostRead::Readable(panes) => panes,
        HostRead::Unreadable { cause } => {
            return Err(Typed::Failed(format!(
                "the host's listing could not be read: {cause}"
            )))
        }
    };
    let Some(pid) = panes
        .iter()
        .find(|pane| pane.session == session && pane.state == PaneState::Alive)
        .and_then(|pane| pane.pid)
    else {
        return Err(Typed::Absent);
    };
    match agent.status(target.config_dir) {
        RosterRead::Readable(rows) => rows
            .into_iter()
            .find(|row| row.pid == Some(pid))
            .ok_or(Typed::Absent),
        RosterRead::Unreadable { cause } => Err(Typed::Failed(format!(
            "the agent's listing could not be read: {cause}"
        ))),
    }
}

/// Type one turn into a seat's own session and say whether it was taken — the
/// one path the controller's rest suggestion, `fleet seat nudge`, a routine's
/// ring and `fleet seat feed` all take.
///
/// In order: the pane and its row are read fresh ([`seat_row`]); a row stopped
/// in front of a human is refused BEFORE ANY BYTE; the text goes to the host as
/// one paste and a separate submit ([`Host::send`]); and a row that was idle is
/// then read every [`TURN_TICK`] for `busy` on the same pid until `bound`
/// closes. The host's `Ok` is a dispatch and never a witness: only the listing
/// says the turn was taken (lessons claude-code D8). A row that was busy
/// already is [`Typed::Queued`] at once.
pub fn type_turn(
    agent: &dyn Agent,
    host: &dyn Host,
    target: &TurnTarget,
    text: &str,
    bound: Duration,
) -> Typed {
    let row = match seat_row(agent, host, target) {
        Ok(row) => row,
        Err(typed) => return typed,
    };
    if let Some(cause) = row.blocked_on() {
        return Typed::Blocked(cause);
    }
    if let Err(cause) = host.send(&host::session_for(target.seat), text) {
        return Typed::Failed(format!("the host did not take the text: {cause}"));
    }
    if row.is_busy() {
        return Typed::Queued;
    }
    let deadline = Instant::now() + bound;
    let mut last = status_word(Some(&row));
    loop {
        // An unreadable listing concludes nothing: the turn may have been
        // taken and the read not, and the bound is what ends the wait.
        if let RosterRead::Readable(rows) = agent.status(target.config_dir) {
            let listed = rows.iter().find(|listed| listed.pid == row.pid);
            if listed.is_some_and(AgentRow::is_busy) {
                return Typed::Delivered;
            }
            last = status_word(listed);
        }
        let now = Instant::now();
        if now >= deadline {
            return Typed::Failed(format!(
                "typed and not taken: still {last} after {}s",
                bound.as_secs()
            ));
        }
        std::thread::sleep(TURN_TICK.min(deadline - now));
    }
}

/// The word a turn that was not taken names its row by: the row's status, or
/// why there is none to name.
fn status_word(row: Option<&AgentRow>) -> String {
    match row {
        Some(row) => row
            .status
            .clone()
            .unwrap_or_else(|| "no status".to_string()),
        None => "unlisted".to_string(),
    }
}

/// Append one event and answer with its id, which is what a row is keyed on. A
/// stream that cannot be written is stated and does not stop the loop: the
/// effect happened either way, and a controller that refused to act because it
/// could not journal would leave the fleet down over a full disk.
fn append(
    events_log: &mut EventLog,
    kind: &str,
    actor: &ActorRef,
    payload: serde_json::Value,
) -> String {
    match events_log.append_id(kind, actor, payload) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("fleet observe: could not append {kind} for {actor}: {e}");
            String::new()
        }
    }
}
