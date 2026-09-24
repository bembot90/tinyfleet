//! The run lifecycle's controller half: a waiting run re-run once the stream
//! has moved past where it stopped and carries a line its wake could be
//! satisfied by, a run nothing could classify re-run to a cap and then held,
//! and the seats a run spawned let go when it ends.
//!
//! WHY THE ACTS ARE A SEAM AND THE DECISION IS NOT. This crate takes nothing
//! from core but its bounded runner (`fleet_core::process`), the release it
//! supports (`fleet_core::supported`) and a seat's identity
//! (`fleet_core::seat::identity`: the id, the fleet.toml roster and the
//! resolver), and every one of the three acts needs a resolution this crate
//! cannot make: a re-run needs the project's store, its packs and its policy
//! file; a park needs the store's hold; a retire needs the
//! project's primary checkout and its worktrees directory. All of those are
//! wired in the binary, so the acts are a seam the binary fills. The
//! DECISION is different: it is a fold of the machine's own stream against one
//! cap, and the stream is this crate's own file — so it lives here, where a poll
//! can be driven against a file and a stub.
//!
//! THE STREAM IS THE STATE. Nothing is held between passes: which runs are
//! waiting, how many times each has been executed, which seats a run spawned and
//! whether its cleanup has already happened are all read back off the stream
//! every pass. A run opened between two passes is picked up by the second with
//! no registration of any kind.
//!
//! A REFUSAL NEVER STOPS THE LOOP: the poll's other work
//! is what a fleet's seats depend on, and one run that cannot be advanced is one
//! run's problem.

use crate::events::{self, EventLog, Record, CONTROLLER};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The environment variable a run's child carries, and every child of that
/// child: `fleet seat spawn` under a workflow reads its own process's copy and
/// the seat it starts is tagged with it.
///
/// THE RUN LIFECYCLE'S NAME, spelled here because this crate names no other
/// member of the workspace. The binary's suite holds this spelling and
/// `fleet_core::item::run::ENV_RUN_ID` to one string.
pub const ENV_RUN_ID: &str = "FLEET_RUN_ID";

/// The six kinds of the run lifecycle this pass folds, and the one it writes.
/// Spelled here for the same reason [`ENV_RUN_ID`] is, and pinned to core's in
/// the same place.
pub const RUN_STARTED: &str = "run.started";
pub const RUN_CLOSED: &str = "run.closed";
pub const RUN_FAILED: &str = "run.failed";
pub const RUN_WAITING: &str = "run.waiting";
pub const RUN_COULD_NOT_TELL: &str = "run.could_not_tell";
pub const RUN_CANCELLED: &str = "run.cancelled";
pub const RUN_CLEANED: &str = "run.cleaned";

/// The kind a park announces on, spelled here beside the six above.
pub const ITEM_HELD: &str = "item.held";

/// The kind a hold's clearance announces on: what a run waiting on the SDK's
/// `hold` is woken by. Spelled here for the same reason.
pub const HOLD_CLEARED: &str = "hold.cleared";

/// The item kinds a `run.waiting` that names items can be woken by: one per
/// state the SDK's `until` accepts (`ITEM_STATES` in
/// `fleet/packs/ts/assets/sdk/mod.ts`), which is the only step whose wake names
/// items at all.
pub const ITEM_WAKE_KINDS: [&str; 6] = [
    "item.dispatched",
    "item.delivered",
    "item.reviewed",
    "item.returned",
    "item.landed",
    ITEM_HELD,
];

/// The line the loop prints for a pass that refused.
pub const REFUSED: &str = "fleet observe: the run pass refused";

/// The run this process was started under, where one started it.
///
/// ONE ENVIRONMENT READ, HERE. A blank value is no run: an exported variable
/// that was never set reaches a child as an empty string, and a seat tagged with
/// the empty run would be selected by a cleanup that names no run at all.
pub fn of_this_process() -> Option<String> {
    match std::env::var(ENV_RUN_ID) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None,
    }
}

/// What the pass asks of whatever can act on a run — the three acts this crate
/// cannot make itself.
///
/// Each answers `Err` with the cause as prose. None of them writes the pass's
/// own event: `run.cleaned` is the pass's line, because the count it carries is
/// a fact about the whole cleanup and not about any one retire.
pub trait Runs {
    /// Execute the bundle the run's directory already holds, once, through the
    /// run lifecycle's back half. The events the execution earns —
    /// `run.started` and the row of the exit table it ends on — are that back
    /// half's, and this pass reads them on its next fold.
    fn rerun(&self, run: &str) -> Result<(), String>;

    /// Raise a hold on the run's record and say why, answering with the hold's
    /// own id — with the park note beside it that `fleet clear` clears the
    /// hold through, or the hold is one nobody can clear.
    fn hold(&self, run: &str, reason: &str) -> Result<String, String>;

    /// Retire one seat the run spawned, through the transient retire path. The
    /// run is passed so the retire's own line can name what the seat was working
    /// for.
    fn retire(&self, seat: &str, run: &str) -> Result<(), String>;
}

/// What the pass acts through and writes to.
pub struct Pass<'a> {
    pub runs: &'a dyn Runs,
    pub events: &'a mut EventLog,
    /// The stream this fold reads. The same file [`Pass::events`] appends to,
    /// read rather than written.
    pub stream: &'a Path,
    /// `[core.run] max_crashes` as the policy in force names it: how many times
    /// a run nothing could classify is executed AGAIN before it parks, so a run
    /// at the cap has been executed `max_crashes + 1` times.
    pub max_crashes: u64,
}

/// One run as the fold reads it.
#[derive(Default)]
struct Folded {
    /// The last lifecycle event's kind, and the sequence its line took.
    last: Option<(String, u64)>,
    /// The position `run.waiting` recorded, where the last event is one.
    recorded: u64,
    /// What the last `run.waiting` named on its wake, as far as the pass can
    /// read it.
    wake: Wake,
    /// How many executions of this run ended on `run.could_not_tell`.
    crashes: u64,
    /// Whether a `run.cleaned` already stands for this run — the latch that
    /// makes the cleanup once per run rather than once per poll.
    cleaned: bool,
    /// Whether an `item.held` already stands for this run. A SECOND LATCH AND
    /// NOT THE ONE ABOVE: the run's last event stays `run.could_not_tell` after
    /// the park — nothing writes a further row of the exit table for a run
    /// nobody is executing — so a pass reading the cap alone raises a hold on
    /// every poll for as long as the record stands.
    held: bool,
    /// Whether a `run.cancelled` stands for this run. A THIRD LATCH: a cancel
    /// stops no process, so an execution under way when it landed still writes
    /// its row of the exit table after it — and a pass reading the last line
    /// alone would take that row for the run's standing and execute it again.
    cancelled: bool,
    /// The seats `session.spawned` named this run on, in the order the stream
    /// met them.
    spawned: BTreeSet<String>,
    /// The workflow `run.started` named. The pass decides nothing on it; it is
    /// what a reader of [`readings`] is told the run was a run of.
    workflow: Option<String>,
    /// The stamp and the payload of the last lifecycle line — the reason on a
    /// failure, the wake on a wait, what was read on a could-not-tell.
    stamp: String,
    said: serde_json::Value,
    /// The hold the park raised, as `item.held` carried it.
    hold: Option<String>,
}

/// One pass over every run this machine's stream knows about.
///
/// THE STREAM IS FOLDED ONCE and every decision below is taken against that one
/// reading, so two runs are judged against the same position and a line this
/// pass writes cannot change what the same pass decides about its neighbour.
pub fn tick(pass: &mut Pass) -> Result<(), String> {
    let stream = events::read_after(pass.stream, 0);
    let head = stream.last().map(|record| record.seq).unwrap_or(0);
    let runs = fold(&stream);
    let mut refusals: Vec<String> = Vec::new();

    for (run, state) in &runs {
        let Some((kind, at)) = &state.last else {
            continue;
        };
        // CANCELLED IS AN ENDING, and the same cleanup every ending gets —
        // whatever line the run's last execution wrote after it.
        if state.cancelled {
            if let Err(why) = clean(pass, run, state) {
                refusals.push(format!("{run}: {why}"));
            }
            continue;
        }
        match kind.as_str() {
            RUN_WAITING if could_wake(&stream, head, *at, state) => {
                if let Err(why) = pass.runs.rerun(run) {
                    refusals.push(format!("{run}: {why}"));
                }
            }
            RUN_COULD_NOT_TELL if state.crashes <= pass.max_crashes => {
                if let Err(why) = pass.runs.rerun(run) {
                    refusals.push(format!("{run}: {why}"));
                }
            }
            // AT THE CAP: the hold, the park, and then the same cleanup an
            // ending gets. A run a person has to look at holds no seats while
            // they do.
            RUN_COULD_NOT_TELL => {
                if !state.held {
                    if let Err(why) = park(pass, run, state) {
                        refusals.push(format!("{run}: {why}"));
                        continue;
                    }
                }
                if let Err(why) = clean(pass, run, state) {
                    refusals.push(format!("{run}: {why}"));
                }
            }
            RUN_CLOSED | RUN_FAILED => {
                if let Err(why) = clean(pass, run, state) {
                    refusals.push(format!("{run}: {why}"));
                }
            }
            _ => {}
        }
    }

    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals.join("; "))
    }
}

/// The fold: one walk of the stream, every run's state off it.
fn fold(stream: &[Record]) -> BTreeMap<String, Folded> {
    let mut runs: BTreeMap<String, Folded> = BTreeMap::new();
    for record in stream {
        match record.kind.as_str() {
            RUN_STARTED | RUN_CLOSED | RUN_FAILED | RUN_WAITING | RUN_COULD_NOT_TELL
            | RUN_CANCELLED => {
                let Some(id) = payload_str(record, "run") else {
                    continue;
                };
                let state = runs.entry(id).or_default();
                state.last = Some((record.kind.clone(), record.seq));
                state.stamp = record.ts.clone();
                state.said = record.payload.clone();
                if record.kind == RUN_STARTED {
                    state.workflow = payload_str(record, "workflow");
                }
                (state.recorded, state.wake) = if record.kind == RUN_WAITING {
                    (
                        record
                            .payload
                            .get("seq")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0),
                        wake_of(record),
                    )
                } else {
                    (0, Wake::Any)
                };
                if record.kind == RUN_COULD_NOT_TELL {
                    state.crashes += 1;
                }
                if record.kind == RUN_CANCELLED {
                    state.cancelled = true;
                }
            }
            RUN_CLEANED => {
                if let Some(id) = payload_str(record, "run") {
                    runs.entry(id).or_default().cleaned = true;
                }
            }
            // The park's latch. The kind ties a record to a list by `item`,
            // which for a run is the run's own id.
            ITEM_HELD => {
                if let Some(id) = payload_str(record, "item") {
                    let state = runs.entry(id).or_default();
                    state.held = true;
                    state.hold = payload_str(record, "hold");
                }
            }
            // The seat is the line's ACTOR on both of the arms below — its id —
            // which is what lets a spawn and a retirement of one seat be read
            // as the same subject. A `session.spawned` carrying no run key was spawned
            // outside a run and enters no run's set — which is what makes the
            // key a selector and not a label.
            events::SESSION_SPAWNED => {
                if let Some(id) = payload_str(record, "run") {
                    runs.entry(id)
                        .or_default()
                        .spawned
                        .insert(record.actor.clone());
                }
            }
            // TAKEN IN STREAM ORDER AND NOT AS A FILTER AT THE END: a set
            // subtracted afterwards would drop a live seat along with a dead one
            // wherever one actor is spawned, retired and spawned again.
            events::SESSION_RETIRED | events::SESSION_STOPPED => {
                for state in runs.values_mut() {
                    state.spawned.remove(&record.actor);
                }
            }
            _ => {}
        }
    }
    runs
}

/// Where one run stands, read off its last lifecycle line and the park's latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// `run.started` is the last line: an execution is under way, or its
    /// process is gone and wrote no row of the exit table. The stream cannot
    /// tell those two apart, and neither can a reader of it.
    Open,
    Waiting,
    /// `run.could_not_tell` with no park behind it: the pass executes it again
    /// while it is under `[core.run] max_crashes`, and parks it at the cap.
    CouldNotTell,
    /// `run.could_not_tell` with the park's latch standing: a hold on the
    /// record, and nothing executes it until a person clears it.
    Held,
    Failed,
    Closed,
    /// `run.cancelled` stands, whatever an execution under way wrote after it.
    Cancelled,
}

/// One run as a reader outside the pass is told it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub run: String,
    /// The workflow `run.started` named, where the stream holds that line.
    pub workflow: Option<String>,
    pub standing: Standing,
    /// The stamp on the last lifecycle line, as the writer put it there.
    pub stamp: String,
    /// That line's payload whole: the reason on a failure, the wake on a wait,
    /// the exit and what was read on a could-not-tell.
    pub said: serde_json::Value,
    /// How many executions ended on `run.could_not_tell`.
    pub crashes: u64,
    /// The hold the park raised, where it is held.
    pub hold: Option<String>,
}

/// Every run the stream holds a lifecycle line for, in id order.
///
/// THE PASS'S OWN FOLD, and not a second reading of the same lines: a page that
/// called a run held by some rule of its own could disagree with the pass
/// that is deciding whether to execute it, and the page is the one a person
/// believes. The run's record in the store is not read — the stream is what the
/// pass decides on, and the record's open or closed carries no row of the exit
/// table.
pub fn readings(stream: &[Record]) -> Vec<Reading> {
    fold(stream)
        .into_iter()
        .filter_map(|(run, state)| {
            let (kind, _) = state.last?;
            let standing = match kind.as_str() {
                _ if state.cancelled => Standing::Cancelled,
                RUN_STARTED => Standing::Open,
                RUN_WAITING => Standing::Waiting,
                RUN_COULD_NOT_TELL if state.held => Standing::Held,
                RUN_COULD_NOT_TELL => Standing::CouldNotTell,
                RUN_FAILED => Standing::Failed,
                _ => Standing::Closed,
            };
            Some(Reading {
                run,
                workflow: state.workflow,
                standing,
                stamp: state.stamp,
                said: state.said,
                crashes: state.crashes,
                hold: state.hold.filter(|_| standing == Standing::Held),
            })
        })
        .collect()
}

/// What a waiting run is waiting for, read off the wake its `run.waiting`
/// carries: the condition the SDK's wrapper printed, which the back half stores
/// as it was printed.
#[derive(Default)]
enum Wake {
    /// The items `until` still has outstanding.
    Items(Vec<String>),
    /// One id: the hold `hold` raised, or the child run `start` opened. The two
    /// verbs throw the id alone, and nothing in the wake says which it is — the
    /// stream does, and [`could_wake`] asks it.
    Id(String),
    /// Every other shape, and no wake at all: re-run on any move.
    #[default]
    Any,
}

/// The wake a `run.waiting` carries, as a [`Wake`].
///
/// EVERY SHAPE THE MATCH DOES NOT KNOW IS `Any` AND NOT A REFUSAL — a wake a
/// workflow threw itself, an empty list nothing could satisfy, a list holding
/// something other than ids, a payload with no wake at all. `Any` is the
/// behaviour that stood before the match: re-run on any move.
///
/// A WAKE STILL WRAPPED AS `{"waiting": <condition>}` IS READ AS THE CONDITION
/// INSIDE IT. The SDK printed that wrapper once, and a run's bundle is pinned in
/// its directory, so a run bundled then prints it on every re-run for as long
/// as it waits — and read whole it would fall to `Any`.
fn wake_of(record: &Record) -> Wake {
    let Some(wake) = record.payload.get("wake") else {
        return Wake::Any;
    };
    let condition = match wake.as_object() {
        Some(wrapped) if wrapped.len() == 1 => wrapped.get("waiting").unwrap_or(wake),
        _ => wake,
    };
    match condition {
        serde_json::Value::String(id) if !id.is_empty() => Wake::Id(id.clone()),
        serde_json::Value::Array(waiting) => {
            let items: Vec<String> = waiting
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
            if !items.is_empty() && items.len() == waiting.len() {
                Wake::Items(items)
            } else {
                Wake::Any
            }
        }
        _ => Wake::Any,
    }
}

/// Whether the waiting run is to be executed again: `at` is the sequence its
/// `run.waiting` line took and `head` the stream's last.
///
/// THE RUN'S OWN ANNOUNCEMENT DOES NOT WAKE IT. The position on the payload is
/// the stream as the child left it, and the `run.waiting` line is appended
/// ABOVE that position — so a comparison against the payload alone is true the
/// instant it is written, and every waiting run would be re-run on the next
/// poll whatever the stream did. The higher of the two is the line that says
/// "somebody else wrote something".
///
/// AND WHAT MOVED HAS TO BE WHAT IT IS WAITING FOR. Without that every line any
/// seat writes while a run waits — a delivery elsewhere, a rest, another run's
/// steps — costs one child execution of the whole workflow that ends waiting in
/// the same place.
///
/// THE ITEMS' WINDOW OPENS AT THE RECORDED POSITION, not at the `run.waiting`
/// line above it: the back half takes the stream's position and appends after
/// it, so a line that landed in that gap is one a narrower window would lose
/// the run's wake to. The state is not compared, only the item: an `until`
/// names one state and the item's other states are cheap to admit, where
/// reading the state from the wake the payload does not carry would be a
/// guess.
///
/// A HOLD WAKES ON ITS CLEARANCE AND A CHILD ON EITHER END, WHEREVER ON THE
/// STREAM IT SITS. The recorded position is read when the process exits, after
/// `hold` or `start` read the stream and threw, so a clearance or an end that
/// landed in between sits at or below it — and a match that looked only above
/// it, or waited for the stream to move, would keep the run on a hold already
/// cleared. Waking on it wherever it is loops on nothing: the re-run replays
/// past the cleared hold or the ended child, so the run's last line is no
/// longer this wait. `start` returns on the child's close and fails on its
/// failure or its cancel, and each is a re-run's to read.
///
/// AN ID IS A HOLD'S OR A RUN'S ONLY WHERE THE STREAM SAYS SO: a hold that was
/// raised or cleared, or a run that was started. An id the stream knows as
/// neither is a word a workflow threw itself, and a match on it would hold the
/// run waiting for a line that is never coming — so it wakes on any move.
fn could_wake(stream: &[Record], head: u64, at: u64, state: &Folded) -> bool {
    let moved = head > state.recorded.max(at);
    match &state.wake {
        Wake::Any => moved,
        Wake::Items(items) => {
            moved
                && stream.iter().any(|record| {
                    record.seq > state.recorded
                        && ITEM_WAKE_KINDS.contains(&record.kind.as_str())
                        && payload_str(record, "item").is_some_and(|item| items.contains(&item))
                })
        }
        Wake::Id(id) => {
            let names = |record: &Record, key: &str| payload_str(record, key).as_ref() == Some(id);
            let known = stream.iter().any(|record| match record.kind.as_str() {
                ITEM_HELD | HOLD_CLEARED => names(record, "hold"),
                RUN_STARTED => names(record, "run"),
                _ => false,
            });
            if !known {
                return moved;
            }
            stream.iter().any(|record| match record.kind.as_str() {
                HOLD_CLEARED => names(record, "hold"),
                RUN_CLOSED | RUN_FAILED | RUN_CANCELLED => names(record, "run"),
                _ => false,
            })
        }
    }
}

/// The hold on the run's record and the `item.held` that announces it.
///
/// THE HOLD COMES FIRST AND THE EVENT SECOND, as every other park's does: a hold
/// nobody announced is a person's question still standing, where an announcement
/// with no hold behind it is a run a reader believes is held and which the store
/// will hand straight back.
fn park(pass: &mut Pass, run: &str, state: &Folded) -> Result<(), String> {
    let reason = format!(
        "{run} has been executed {} time(s) and nothing could classify the last one — \
         `[core.run] max_crashes` is {}",
        state.crashes, pass.max_crashes
    );
    let hold = pass.runs.hold(run, &reason)?;
    pass.events
        .append(
            ITEM_HELD,
            CONTROLLER,
            serde_json::json!({
                "item": run,
                "reason": reason,
                // A run holds neither: what it is pinned to is its directory's
                // hash, which `run.started` already carries. The keys are
                // present carrying null rather than dropped, so a fold meets a
                // field it reads as absent and not a shape that varies.
                "branch": serde_json::Value::Null,
                "commit": serde_json::Value::Null,
                "hold": hold,
            }),
        )
        .map_err(|e| format!("{run} is held and {ITEM_HELD} did not reach the stream: {e}"))
}

/// Every seat the run spawned, retired, and one `run.cleaned` with the count.
///
/// ONCE PER RUN. The latch is `run.cleaned` on the stream and not a field held
/// between passes, so a controller that restarts mid-cleanup finishes it and one
/// that restarts after it does nothing.
///
/// THE LINE IS WRITTEN EVEN WHERE THE COUNT IS ZERO: a run that spawned no seat
/// has been cleaned, and a pass that wrote nothing would walk its seats again
/// every poll for as long as the record stood.
fn clean(pass: &mut Pass, run: &str, state: &Folded) -> Result<(), String> {
    if state.cleaned {
        return Ok(());
    }
    let mut count = 0u64;
    let mut refusals: Vec<String> = Vec::new();
    for seat in &state.spawned {
        match pass.runs.retire(seat, run) {
            Ok(()) => count += 1,
            Err(why) => refusals.push(format!("{seat}: {why}")),
        }
    }
    // A SEAT THAT WOULD NOT RETIRE HOLDS THE LATCH OPEN. The line says how many
    // seats are gone, and writing it over a seat still standing would retire the
    // rest and never come back for that one.
    if !refusals.is_empty() {
        return Err(refusals.join("; "));
    }
    pass.events
        .append(
            RUN_CLEANED,
            CONTROLLER,
            serde_json::json!({ "run": run, "count": count }),
        )
        .map_err(|e| format!("{run}'s seats are retired and {RUN_CLEANED} did not land: {e}"))
}

fn payload_str(record: &Record, key: &str) -> Option<String> {
    record
        .payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}
