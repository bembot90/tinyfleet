//! Carrying a verdict out (PRD R16, R18, R19, R21).
//!
//! Every act here writes its own event, once, at the layer that did the thing
//! (R25) — so a person reading the stream in the morning sees the request, its
//! collection and its outcome as three lines rather than as silence.
//!
//! Every verdict the table reaches is acted on here. `halt` is the one whose act
//! is to dispatch nothing: it is published as its own outcome, and its line and
//! its event are written once, at the transition into the hold.

use crate::adapter::{Agent, RemoveAnswer, StartOutcome, StartSpec};
use crate::events::{self, EventLog};
use crate::policy::Policy;
use crate::sessions::{SessionRow, Table};
use std::path::Path;
use std::time::Duration;

/// What an effect DID, for the projection's row. Never a liveness claim: a
/// start that returned OK is a start that did not fail, and arrival is the
/// roster's answer on the next poll (lessons claude-code A7).
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
    pub seat_dir: &'a str,
    /// The name a person addresses the seat by, lowercased, falling back to the
    /// seat directory — never an empty string, which would put an empty element
    /// in argv and make `--name` swallow the flag after it.
    pub display_name: String,
    pub project: &'a str,
    pub worktree: &'a str,
    pub model: String,
    pub posture: String,
    pub first_turn: String,
    pub transient: bool,
    /// The configuration directory this start comes up under, when it comes up
    /// under its own (flights PRD R13). `None` takes the fleet's.
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
    /// The run that spawned this seat, where one did (controller PRD R37).
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
    /// start named, or the fleet's (flights PRD R13).
    fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref().map(Path::new)
    }
}

/// The seat's display name: the chosen one lowercased, else the directory.
pub fn display_name(chosen: Option<&str>, seat_dir: &str) -> String {
    match chosen.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_lowercase(),
        _ => seat_dir.to_string(),
    }
}

/// Start a woken session for this seat, and open its row.
///
/// On a failed start there is NO ROW. A row for a session that never came up is
/// a dispatch the arrival window then waits on forever, which is the false
/// success A14 is about — so the failure is an event and nothing else, and the
/// next poll finds the seat absent and eligible again.
pub fn spawn_woken(
    agent: &dyn Agent,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
    table: &mut Table,
    now_ms: u64,
) -> Outcome {
    match start_once(agent, policy, target, events_log) {
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
        seat: target.seat_dir.to_string(),
        project: target.project.to_string(),
        worktree: target.worktree.to_string(),
        name: target.display_name.clone(),
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

/// The sentence one nudge carries (PRD R21, lessons claude-code C5).
///
/// It names the reading, the threshold it crossed and the command that answers
/// it, because a suggestion whose recipient has to go and look up all three is
/// one that costs more attention than it saves. It suggests and never enforces:
/// there is deliberately no path from the threshold to an automatic rest.
pub fn nudge_text(display_name: &str, tokens: u64, threshold: u64, seat_dir: &str) -> String {
    format!(
        "{display_name}: context at {tokens} tokens, over the rest threshold {threshold} — \
         rest when your work allows: fleet event rest {seat_dir} --reason <why>"
    )
}

/// The turn that carries it: one message, to one session, verbatim.
///
/// The nudge runs as a print-mode turn in the seat's own worktree, so the
/// session it addresses is one the agent can already see; the instruction is to
/// send EXACTLY ONE message and nothing else, because a turn that takes
/// initiative here is a second voice in a seat's session that nobody asked for.
pub fn nudge_prompt(display_name: &str, text: &str) -> String {
    format!(
        "Send exactly one message to the session named `{display_name}` through the \
         cross-session send tool, with this text verbatim and nothing added:\n\n{text}\n\n\
         Send that one message and then stop. Do not act on the message yourself, do not \
         open any file, and do not reply here with anything but whether the send returned."
    )
}

/// What a rest did, so the caller can log the half that failed.
pub enum Rested {
    /// The predecessor was stopped, the successor started and the predecessor's
    /// row removed. The rest is collected and the event is consumed.
    Collected,
    /// The stop did not exit 0. Nothing was started and nothing was removed, the
    /// rest stays pending, and the next poll retries — the alarm the PRD names
    /// is exactly this: a `seat.resting` with no `session.rested` after it.
    StopFailed(String),
    /// The stop exited 0 and the successor's start failed. The predecessor is
    /// down, its row still stands, nothing was removed and the rest stays pending.
    StartFailed(String),
    /// There was nothing to stop: a rest is defined on a live row and this seat
    /// has no address to issue against.
    NoAddress,
}

pub const PHASE_START: &str = "start";

/// A `session.crashed` payload, written by the layer that met the failure.
pub fn crashed_payload(phase: &str, cause: &str, log: &str) -> serde_json::Value {
    serde_json::json!({ "phase": phase, "cause": cause, "output": log })
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

/// One start, its event, and the id the row is keyed on.
pub fn start_once(
    agent: &dyn Agent,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
) -> Result<String, String> {
    // The plugin root comes off POLICY and not off the target: every start the
    // controller makes goes through this one construction, so a second builder
    // of a target cannot forget it and the two cannot disagree.
    let spec = StartSpec {
        seat_dir: target.seat_dir.to_string(),
        worktree: target.worktree.to_string(),
        name: target.display_name.clone(),
        model: target.model.clone(),
        posture: target.posture.clone(),
        first_turn: target.first_turn.clone(),
        plugin_dir: policy
            .plugin_dir
            .as_ref()
            .map(|dir| dir.display().to_string()),
        config_dir: target.config_dir.clone(),
    };
    match agent.start(&spec, Duration::from_secs(policy.start_watch_seconds)) {
        StartOutcome::Started { log } => {
            let payload = serde_json::json!({
                "worktree": target.worktree,
                "project": target.project,
                "name": target.display_name,
                "model": target.model,
                "posture": target.posture,
                "first_turn": target.first_turn,
                "transient": target.transient,
                "config_dir": target.config_dir,
                "item": target.item,
                "settings": target.settings,
                "belt": target.belt,
                "run": target.run,
                "output": log,
            });
            Ok(append(
                events_log,
                events::SESSION_SPAWNED,
                target.seat_dir,
                payload,
            ))
        }
        StartOutcome::Failed { cause, log } => {
            append(
                events_log,
                events::SESSION_CRASHED,
                target.seat_dir,
                crashed_payload(PHASE_START, &cause, &log),
            );
            Err(cause)
        }
    }
}

/// The rest collection, in the order the PRD fixes: stop, start the successor,
/// then remove the predecessor (R16).
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
    let started = start_once(agent, policy, target, events_log);
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
        target.seat_dir,
        serde_json::json!({
            "predecessor": target.session_id,
            "predecessor_address": short_id,
            "successor_dispatch": dispatch_id,
            "removed": removed,
        }),
    );
    Rested::Collected
}

/// Bring a hibernated row back in place (R10).
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
            target.seat_dir
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
        target.seat_dir,
        serde_json::json!({
            "session": session_id,
            "address": short_id,
            "worktree": target.worktree,
            "project": target.project,
            // The identity fields every other row-opening line carries: an
            // attach is its own dispatch, so a rebuild that meets this line
            // with no row to match — a trimmed stream — opens one from these
            // rather than from empty strings.
            "name": target.display_name,
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

/// Claim, at startup, every session the table names that the roster still LISTS
/// (R17).
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
/// A row is claimed when it is LIVE and never by its state (R17, gas-city G7:
/// "adopts every live session it names"). The state word cannot carry the
/// question — a live idle session reads `done` (lessons claude-code A3), so a
/// claim gated on it takes no idle seat at all — and liveness is a sighting
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
            &row.seat,
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

/// The one line and the one event a halt transition writes (R14).
///
/// Announced ONCE per transition into the halt and never once per poll: the
/// caller writes this only when the latch moved.
pub fn halted(seat_dir: &str, blind: u32, events_log: &mut EventLog) {
    eprintln!(
        "fleet observe: {seat_dir} has gone blind on {blind} consecutive dispatches and is HELD \
         DOWN; nothing further is dispatched for it until `fleet event clear-halt {seat_dir}`"
    );
    append(
        events_log,
        events::SESSION_HALTED,
        seat_dir,
        serde_json::json!({ "blind": blind }),
    );
}

/// The event a counted blind dispatch writes, carrying the count it moved to —
/// which is what a rebuild folds the counter back out of.
pub fn blind_dispatch(seat_dir: &str, blind: u32, verdict: &str, events_log: &mut EventLog) {
    append(
        events_log,
        events::DISPATCH_BLIND,
        seat_dir,
        serde_json::json!({ "blind": blind, "verdict": verdict }),
    );
}

/// One nudge, its event, and the session id that must never be nudged again.
pub fn nudge(
    agent: &dyn Agent,
    policy: &Policy,
    target: &Target,
    events_log: &mut EventLog,
    table: &mut Table,
) -> Outcome {
    let (Some(session_id), Some(tokens)) = (target.session_id, target.context_tokens) else {
        return Outcome::None;
    };
    let text = nudge_text(
        &target.display_name,
        tokens,
        policy.rest_threshold_tokens,
        target.seat_dir,
    );
    let sent = agent.nudge(
        target.config_dir(),
        target.seat_dir,
        target.worktree,
        &policy.nudge_model,
        &nudge_prompt(&target.display_name, &text),
        Duration::from_secs(policy.nudge_timeout_seconds),
    );
    let outcome = match &sent {
        Ok(()) => "sent".to_string(),
        Err(cause) => format!("failed: {cause}"),
    };
    // The session is marked WHETHER OR NOT the turn landed. The budget is one
    // nudge per session and a retry loop against a session that cannot be
    // reached is the noise R21 exists to prevent; the event carries the failure
    // for the person who reads the stream.
    table.mark_nudged(target.seat_dir, session_id);
    append(
        events_log,
        events::SESSION_NUDGED,
        target.seat_dir,
        serde_json::json!({
            "session": session_id,
            "context_tokens": tokens,
            "threshold": policy.rest_threshold_tokens,
            "outcome": outcome,
        }),
    );
    match sent {
        Ok(()) => Outcome::Nudged,
        Err(_) => Outcome::Failed,
    }
}

/// Append one event and answer with its id, which is what a row is keyed on. A
/// stream that cannot be written is stated and does not stop the loop: the
/// effect happened either way, and a controller that refused to act because it
/// could not journal would leave the fleet down over a full disk.
fn append(
    events_log: &mut EventLog,
    kind: &str,
    actor: &str,
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
