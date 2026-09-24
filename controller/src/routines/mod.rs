//! Routines: standing duties the controller's own tick evaluates and fires.
//!
//! One clock and one record. There is no second daemon and no per-routine service
//! job: every routine from the fleet install, from every installed pack and from
//! every project is read on the tick, and a controller that is down fires
//! nothing and says so by the absence of its own events.
//!
//! The stream is the ledger. A firing writes `routine.fired` and then one terminal
//! event, and a NOT-DUE evaluation writes nothing at all — a stream that
//! reported every quiet minute would drown the three lines a person came to
//! read.

pub mod action;
pub mod file;
pub mod load;
pub mod state;
pub mod trigger;

use crate::clock;
use crate::events::{self, EventLog};
use crate::observe::RosterState;
use file::Routine;
use serde::{Deserialize, Serialize};
use state::{RoutineState, State};
use std::path::Path;
use trigger::Due;

/// A file holding the instant every routine clock reads, for a caller driving the
/// tick against a clock of its own. Unset is this machine's own.
pub const CLOCK_SEAM: &str = "FLEET_ORDERS_CLOCK";

/// What one routine's action produced, terminally.
///
/// Four are a success and three are not, and the streak below counts only the
/// second kind — a could-not-tell that reset the streak would hide a duty whose
/// instrument has been unreadable for a week.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    Filed,
    Deduped,
    Ran,
    Failed,
    Absent,
    CouldNotTell,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Delivered => "delivered",
            Outcome::Filed => "filed",
            Outcome::Deduped => "deduped",
            Outcome::Ran => "ran",
            Outcome::Failed => "failed",
            Outcome::Absent => "absent",
            Outcome::CouldNotTell => "could-not-tell",
        }
    }

    /// Whether this outcome carries the streak forward. A success of any kind
    /// puts it back to 0.
    pub fn is_failing(self) -> bool {
        matches!(
            self,
            Outcome::Failed | Outcome::Absent | Outcome::CouldNotTell
        )
    }

    /// The exit `fleet routine run` carries for this outcome, from the one exit
    /// table every verb shares.
    pub fn exit_code(self) -> u8 {
        match self {
            Outcome::Delivered | Outcome::Filed | Outcome::Deduped | Outcome::Ran => 0,
            Outcome::CouldNotTell => 3,
            Outcome::Absent => 4,
            Outcome::Failed => 1,
        }
    }
}

/// One seat as a routine's ring reads it: where it works, what a person calls it,
/// and what the roster said about it this tick.
#[derive(Clone, Debug)]
pub struct SeatView {
    pub seat_dir: String,
    pub display_name: String,
    pub worktree: String,
    pub state: RosterState,
    /// The configuration directory this seat's session is held under, where its
    /// own row names one. A ring that reached a spawned seat through the
    /// fleet's directory would start a second session beside it.
    pub config_dir: Option<String>,
}

/// One routine's row in the projection.
#[derive(Serialize, Deserialize)]
pub struct RoutineRow {
    pub name: String,
    pub source: String,
    pub trigger: String,
    /// The next instant this routine is expected to be asked about. Null is a
    /// cron schedule with no matching minute inside the search limit.
    pub next_due: Option<String>,
    pub last_outcome: Option<String>,
    pub last_fired: Option<String>,
    /// What the reference let run unseen: consecutive terminal outcomes that
    /// were not a success. Published every tick, so a duty that has been failing
    /// for a week is a number a reader meets rather than a silence.
    pub failing_streak: u64,
}

/// The instant every routine clock reads, in epoch seconds.
///
/// The seam is a FILE and not a value, because a caller stepping the clock does
/// it under a controller that is already running.
pub fn now_secs() -> u64 {
    let seam = std::env::var(CLOCK_SEAM)
        .ok()
        .filter(|p| !p.trim().is_empty());
    let stepped = seam
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|body| clock::secs_of_stamp(body.trim()));
    stepped.unwrap_or_else(|| clock::now_ms() / 1000)
}

/// The projection's routines array, rendered from the registry and the state.
pub fn rows(registry: &load::Registry, state: &State, now: u64) -> Vec<RoutineRow> {
    registry
        .routines
        .iter()
        .map(|routine| {
            let entry = state.entry(&routine.name);
            let last_fired = entry.last_fired.as_deref().and_then(clock::secs_of_stamp);
            let last_evaluated = entry
                .last_evaluated
                .as_deref()
                .and_then(clock::secs_of_stamp);
            RoutineRow {
                name: routine.name.clone(),
                source: routine.source.as_string(),
                trigger: routine.trigger.as_str().to_string(),
                next_due: trigger::next_due(routine, now, last_fired, last_evaluated)
                    .map(clock::stamp_secs),
                last_outcome: entry.last_outcome.clone(),
                last_fired: entry.last_fired.clone(),
                failing_streak: entry.failing_streak,
            }
        })
        .collect()
}

/// Everything one pass over the routines writes to.
pub struct Pass<'a> {
    pub machine: action::Machine<'a>,
    pub events: &'a mut EventLog,
    pub state: &'a mut State,
}

impl Pass<'_> {
    fn machine_dir(&self) -> &Path {
        self.machine.machine_dir
    }

    fn flush(&mut self) {
        if let Err(e) = state::write(self.machine_dir(), self.state) {
            eprintln!("fleet observe: could not write the routines state: {e}");
        }
    }

    fn append(&mut self, kind: &str, routine: &str, payload: serde_json::Value) {
        if let Err(e) = self.events.append(kind, routine, payload) {
            eprintln!("fleet observe: could not append {kind} for {routine}: {e}");
        }
    }
}

/// One pass over every loaded routine.
///
/// The state is written BEFORE the action on every firing: a duty that happened
/// and was not recorded fires again forever, and a double firing costs one turn.
pub fn tick(registry: &load::Registry, pass: &mut Pass, now: u64) {
    let mut moved = false;
    for routine in &registry.routines {
        let entry = pass.state.entry(&routine.name);
        let last_evaluated = entry
            .last_evaluated
            .as_deref()
            .and_then(clock::secs_of_stamp);
        if !trigger::is_due_an_evaluation(routine, now, last_evaluated) {
            continue;
        }
        // A run holding the lock owns this routine for as long as it holds it.
        if let state::Lock::Held(pid) = state::read_lock(pass.machine_dir(), &routine.name) {
            eprintln!(
                "fleet observe: {} is held by a run at pid {pid}; this tick skips it",
                routine.name
            );
            continue;
        }
        let last_fired = entry.last_fired.as_deref().and_then(clock::secs_of_stamp);
        let machine = &pass.machine;
        let answer = trigger::evaluate(routine, now, last_fired, &|routine| {
            action::run_check(routine, machine)
        });
        match answer {
            Due::NotDue(_) => {
                let seen = RoutineState {
                    last_evaluated: Some(clock::stamp_secs(now)),
                    ..entry
                };
                pass.state.put(&routine.name, seen);
                moved = true;
            }
            Due::CouldNotTell(why) => {
                record_could_not_tell(routine, pass, now, &why, None);
                moved = false;
            }
            Due::Due(reason) => {
                fire(routine, pass, now, &reason, None);
                moved = false;
            }
        }
    }
    if moved {
        pass.flush();
    }
}

/// A trigger that could not answer: the state moves, the streak grows, and one
/// event says which instrument was unreadable.
pub fn record_could_not_tell(
    routine: &Routine,
    pass: &mut Pass,
    now: u64,
    why: &str,
    by: Option<&str>,
) -> Outcome {
    let mut entry = pass.state.entry(&routine.name);
    entry.last_evaluated = Some(clock::stamp_secs(now));
    entry.last_outcome = Some(Outcome::CouldNotTell.as_str().to_string());
    entry.failing_streak += 1;
    let streak = entry.failing_streak;
    pass.state.put(&routine.name, entry);
    pass.flush();
    let mut payload = serde_json::json!({
        "order": routine.name,
        "reason": why,
        "streak": streak,
    });
    add_by(&mut payload, by);
    pass.append(events::ROUTINE_COULD_NOT_TELL, &routine.name, payload);
    Outcome::CouldNotTell
}

/// Fire one routine: the state, the opening event, the action, the terminal event.
pub fn fire(
    routine: &Routine,
    pass: &mut Pass,
    now: u64,
    reason: &str,
    by: Option<&str>,
) -> Outcome {
    let stamp = clock::stamp_secs(now);
    let mut entry = pass.state.entry(&routine.name);
    entry.last_evaluated = Some(stamp.clone());
    entry.last_fired = Some(stamp.clone());
    pass.state.put(&routine.name, entry);
    pass.flush();

    let mut opening = serde_json::json!({
        "order": routine.name,
        "source": routine.source.as_string(),
        "trigger": routine.trigger.as_str(),
        "reason": reason,
    });
    add_by(&mut opening, by);
    pass.append(events::ROUTINE_FIRED, &routine.name, opening);

    let started = std::time::Instant::now();
    let done = action::run(routine, &pass.machine, &stamp);
    let duration_ms = started.elapsed().as_millis() as u64;

    let mut entry = pass.state.entry(&routine.name);
    entry.last_outcome = Some(done.outcome.as_str().to_string());
    entry.failing_streak = if done.outcome.is_failing() {
        entry.failing_streak + 1
    } else {
        0
    };
    let streak = entry.failing_streak;
    pass.state.put(&routine.name, entry);
    pass.flush();

    let (kind, mut payload) = match done.outcome {
        Outcome::CouldNotTell => (
            events::ROUTINE_COULD_NOT_TELL,
            serde_json::json!({
                "order": routine.name,
                "reason": done.detail,
                "streak": streak,
            }),
        ),
        Outcome::Failed | Outcome::Absent => (
            events::ROUTINE_FAILED,
            serde_json::json!({
                "order": routine.name,
                "outcome": done.outcome.as_str(),
                "detail": done.detail,
                "duration_ms": duration_ms,
                "streak": streak,
            }),
        ),
        _ => (
            events::ROUTINE_COMPLETED,
            serde_json::json!({
                "order": routine.name,
                "outcome": done.outcome.as_str(),
                "detail": done.detail,
                "duration_ms": duration_ms,
            }),
        ),
    };
    if let Some(object) = payload.as_object_mut() {
        for (key, value) in done.extra {
            object.insert(key, value);
        }
    }
    add_by(&mut payload, by);
    pass.append(kind, &routine.name, payload);
    done.outcome
}

/// The half of an event payload that says a person asked for this firing by
/// hand. Absent on the tick's own, which is the fleet's ordinary clock.
fn add_by(payload: &mut serde_json::Value, by: Option<&str>) {
    if let (Some(object), Some(by)) = (payload.as_object_mut(), by) {
        object.insert("by".to_string(), by.into());
    }
}

/// The state a routine carries into an evaluation, as epoch seconds.
pub fn stamps_of(entry: &RoutineState) -> (Option<u64>, Option<u64>) {
    (
        entry.last_fired.as_deref().and_then(clock::secs_of_stamp),
        entry
            .last_evaluated
            .as_deref()
            .and_then(clock::secs_of_stamp),
    )
}
