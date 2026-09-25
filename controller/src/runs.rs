//! The run lifecycle's controller half: a waiting run re-run once the stream
//! carries a line its wake could be satisfied by, a run nothing could classify
//! re-run to a cap and then held, and the seats a run spawned let go when it
//! ends.
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
//! THE STREAM IS THE STATE, BUT FOR THE PARK. Nothing is held between passes:
//! which runs are waiting, how many times each has been executed, which seats a
//! run spawned and whether its cleanup has already happened are all read back
//! off the stream every pass. A run opened between two passes is picked up by
//! the second with no registration of any kind. Whether a run at the cap is
//! parked is its record's to say — a held entry of reason `max_crashes` — and
//! the pass asks it through the same seam, [`Runs::capped`].
//!
//! A REFUSAL NEVER STOPS THE LOOP: the poll's other work
//! is what a fleet's seats depend on, and one run that cannot be advanced is one
//! run's problem.

use crate::events::{self, ActorRef, EventLog, Record};
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

/// The one line every entry on an item's timeline is signalled on,
/// `{item, entry, kind}`: what a `run.waiting` whose wake names entry kinds is
/// woken by, and what the park writes for the `held` entry it raised. Spelled
/// here for the same reason the six above are, and pinned to core's in the
/// same place.
///
/// A SIGNAL AND NOT THE RECORD. The SDK's `until` and `hold` read the store
/// and name the entries that could satisfy them; the line says only that one
/// was written, and the re-run reads the record for itself.
pub const ITEM_ENTRY: &str = "item.entry";

/// Every kind an entry is, as `fleet_core::entry::KINDS` spells them and in
/// its order: the kinds a wake may name. Spelled here for the same reason, and
/// pinned to core's in the same place.
pub const ENTRY_KINDS: [&str; 7] = [
    "ordered",
    "order_withdrawn",
    "delivered",
    "reviewed",
    "held",
    "cleared",
    "landed",
];

/// The entry kind the park signals.
const HELD: &str = "held";

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
/// cannot make itself, and the one read of a run's record it cannot either.
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
    /// own id and the id of the held entry beside it — the entry `fleet clear`
    /// clears the hold through, or the hold is one nobody can clear, and the
    /// entry the park's signal names.
    fn hold(&self, run: &str, reason: &str) -> Result<(String, String), String>;

    /// The crash-cap hold the run's record carries: its last held entry of
    /// reason `max_crashes`, and whether the store still holds that hold open.
    /// `None` is a record that carries none — a seat's or the run's own ask is
    /// not one.
    ///
    /// THE PARK'S LATCH AND THE PARKED STANDING, both: the record is where a
    /// park is, and a line on the stream naming the run is not.
    fn capped(&self, run: &str) -> Result<Option<CapHold>, String>;

    /// Retire one seat the run spawned, through the transient retire path. The
    /// run is passed so the retire's own line can name what the seat was working
    /// for.
    fn retire(&self, seat: &str, run: &str) -> Result<(), String>;
}

/// A run's crash-cap hold, as its record carries it: the hold's id, and
/// whether a clearance has closed it since.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapHold {
    pub hold: String,
    pub cleared: bool,
}

/// What the pass acts through and writes to.
pub struct Pass<'a> {
    pub runs: &'a dyn Runs,
    pub events: &'a mut EventLog,
    /// Who the pass's own lines are by: the controller, under this machine's
    /// identity ([`events::controller`]).
    pub controller: &'a ActorRef,
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
    /// Whether a `run.cancelled` stands for this run. A SECOND LATCH: a cancel
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
            //
            // THE LATCH IS THE RECORD'S. The run's last event stays
            // `run.could_not_tell` after the park — nothing writes a further
            // row of the exit table for a run nobody is executing — so the pass
            // asks the record whether it already carries a `max_crashes` hold.
            // A line on the stream naming the run is not that answer: the run's
            // own ask is a held entry too (fleet-z4w), and it is not the park.
            RUN_COULD_NOT_TELL => {
                match pass.runs.capped(run) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        if let Err(why) = park(pass, run, state) {
                            refusals.push(format!("{run}: {why}"));
                            continue;
                        }
                    }
                    Err(why) => {
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
            // The seat is the line's ACTOR on both of the arms below — a seat,
            // by its id — which is what lets a spawn and a retirement of one
            // seat be read as the same subject; a line by any other kind names
            // no seat. A `session.spawned` carrying no run key was spawned
            // outside a run and enters no run's set — which is what makes the
            // key a selector and not a label.
            events::SESSION_SPAWNED => {
                if let (Some(id), Some(seat)) = (payload_str(record, "run"), record.actor.seat_id())
                {
                    runs.entry(id).or_default().spawned.insert(seat.to_string());
                }
            }
            // TAKEN IN STREAM ORDER AND NOT AS A FILTER AT THE END: a set
            // subtracted afterwards would drop a live seat along with a dead one
            // wherever one actor is spawned, retired and spawned again.
            events::SESSION_RETIRED | events::SESSION_STOPPED => {
                if let Some(seat) = record.actor.seat_id() {
                    for state in runs.values_mut() {
                        state.spawned.remove(seat);
                    }
                }
            }
            _ => {}
        }
    }
    runs
}

/// Where one run stands, read off its last lifecycle line and, for a run
/// nothing could classify, whether its record carries the park.
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
    /// `run.could_not_tell` with a `max_crashes` hold on the record: nothing
    /// executes it again, whether or not a person has cleared the hold since.
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
    /// Whether that hold is cleared, where it is held.
    pub cleared: bool,
}

/// Every run the stream holds a lifecycle line for, in id order.
///
/// THE PASS'S OWN FOLD, and not a second reading of the same lines: a page that
/// called a run held by some rule of its own could disagree with the pass
/// that is deciding whether to execute it, and the page is the one a person
/// believes. The record's open or closed carries no row of the exit table, so
/// the standing is the stream's — except the park, which is the record's:
/// `capped` is what [`Runs::capped`] answered for each run it holds, and a run
/// whose last line is `run.could_not_tell` reads held where it holds one.
pub fn readings(stream: &[Record], capped: &BTreeMap<String, CapHold>) -> Vec<Reading> {
    fold(stream)
        .into_iter()
        .filter_map(|(run, state)| {
            let (kind, _) = state.last?;
            let park = capped.get(&run);
            let standing = match kind.as_str() {
                _ if state.cancelled => Standing::Cancelled,
                RUN_STARTED => Standing::Open,
                RUN_WAITING => Standing::Waiting,
                RUN_COULD_NOT_TELL if park.is_some() => Standing::Held,
                RUN_COULD_NOT_TELL => Standing::CouldNotTell,
                RUN_FAILED => Standing::Failed,
                _ => Standing::Closed,
            };
            let park = park.filter(|_| standing == Standing::Held);
            Some(Reading {
                run,
                workflow: state.workflow,
                standing,
                stamp: state.stamp,
                said: state.said,
                crashes: state.crashes,
                hold: park.map(|park| park.hold.clone()),
                cleared: park.is_some_and(|park| park.cleared),
            })
        })
        .collect()
}

/// What a waiting run is waiting for, read off the wake its `run.waiting`
/// carries: the condition the SDK's wrapper printed, which the back half stores
/// as it was printed.
#[derive(Default)]
enum Wake {
    /// What `until` and `hold` throw: the items whose records the step
    /// re-reads, the entry kinds any of which could satisfy it, and `since`,
    /// the stream's position when the waiting execution started.
    Items {
        items: Vec<String>,
        kinds: Vec<String>,
        since: u64,
    },
    /// One id: the child run `start` opened. The verb throws the id alone, and
    /// the stream says whether it is a run — [`could_wake`] asks it.
    Id(String),
    /// Every other shape, and no wake at all: re-run on any move.
    #[default]
    Any,
}

/// The wake a `run.waiting` carries, as a [`Wake`]: `{items, kinds, since}`
/// with at least one item, at least one kind, every kind an entry kind
/// ([`ENTRY_KINDS`]) and `since` a position, is [`Wake::Items`]; a non-empty
/// string is [`Wake::Id`]; everything else is [`Wake::Any`].
///
/// EVERY SHAPE THE MATCH DOES NOT KNOW IS `Any` AND NOT A REFUSAL — a wake a
/// workflow threw itself, an items wake missing a part or naming a kind no
/// entry is, a payload with no wake at all. `Any` is the behaviour that stood
/// before the match: re-run on any move.
///
/// THE CLEAN BREAK: a bundle pinned before the SDK read the store throws a
/// bare list of items, or a hold's id, or either inside `{"waiting": …}`, on
/// every re-run for as long as it waits. None of those says which entries could
/// satisfy it or where its execution started, so each is `Any` — a hold's id
/// among them, which names no run the stream started — and the re-run reads for
/// itself.
fn wake_of(record: &Record) -> Wake {
    match record.payload.get("wake") {
        Some(serde_json::Value::String(id)) if !id.is_empty() => Wake::Id(id.clone()),
        Some(wake @ serde_json::Value::Object(_)) => items_of(wake).unwrap_or_default(),
        _ => Wake::Any,
    }
}

/// An items wake, where every part of it is there and reads.
fn items_of(wake: &serde_json::Value) -> Option<Wake> {
    let strings = |key: &str| -> Option<Vec<String>> {
        let list = wake.get(key)?.as_array()?;
        let strings = list
            .iter()
            .map(|value| value.as_str().map(str::to_string))
            .collect::<Option<Vec<String>>>()?;
        (!strings.is_empty()).then_some(strings)
    };
    let items = strings("items")?;
    let kinds = strings("kinds")?;
    let known = |kind: &String| ENTRY_KINDS.contains(&kind.as_str());
    if !kinds.iter().all(known) {
        return None;
    }
    let since = wake.get("since")?.as_u64()?;
    Some(Wake::Items {
        items,
        kinds,
        since,
    })
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
/// AN ITEMS WAKE IS WOKEN BY AN [`ITEM_ENTRY`] ABOVE ITS `since`, for one of its
/// items, signalling an entry of one of its kinds — wherever that line sits
/// against the recorded position, and with no move needed past it. The
/// recorded position is read when the process exits, after `until` or `hold`
/// read the store and threw, so a delivery or a clearance written in between
/// sits at or below it (fleet-0oi); `since` is where the execution STARTED, and
/// what the store held before that the execution read. It cannot loop: the
/// re-run's own wait starts from a position above the line that woke it.
///
/// A CHILD WAKES ITS PARENT ON EITHER END, WHEREVER ON THE STREAM IT SITS, for
/// the same reason: `start` read the stream and threw before the parent exited.
/// Waking on it wherever it is loops on nothing: the re-run replays past the
/// ended child, so the run's last line is no longer this wait. `start` returns
/// on the child's close and fails on its failure or its cancel, and each is a
/// re-run's to read.
///
/// AN ID IS A RUN'S ONLY WHERE THE STREAM SAYS SO: a run that was started. An
/// id the stream does not know as one is a word a workflow threw itself — or a
/// hold's id from a bundle pinned before holds woke through the store — and a
/// match on it would hold the run waiting for a line that is never coming, so
/// it wakes on any move.
fn could_wake(stream: &[Record], head: u64, at: u64, state: &Folded) -> bool {
    let moved = head > state.recorded.max(at);
    match &state.wake {
        Wake::Any => moved,
        Wake::Items {
            items,
            kinds,
            since,
        } => stream.iter().any(|record| {
            record.kind == ITEM_ENTRY
                && payload_str(record, "item").is_some_and(|item| items.contains(&item))
                && payload_str(record, "kind").is_some_and(|kind| kinds.contains(&kind))
                && record.seq > *since
        }),
        Wake::Id(id) => {
            let names = |record: &Record| payload_str(record, "run").as_ref() == Some(id);
            let known = stream
                .iter()
                .any(|record| record.kind == RUN_STARTED && names(record));
            if !known {
                return moved;
            }
            stream.iter().any(|record| {
                matches!(
                    record.kind.as_str(),
                    RUN_CLOSED | RUN_FAILED | RUN_CANCELLED
                ) && names(record)
            })
        }
    }
}

/// The hold on the run's record, and the signal of the held entry beside it.
///
/// THE HOLD COMES FIRST AND THE SIGNAL SECOND, as every other park's does: a
/// hold nobody signalled is a person's question still standing, where a signal
/// with no hold behind it names an entry the record does not carry.
fn park(pass: &mut Pass, run: &str, state: &Folded) -> Result<(), String> {
    let reason = format!(
        "{run} has been executed {} time(s) and nothing could classify the last one — \
         `[core.run] max_crashes` is {}",
        state.crashes, pass.max_crashes
    );
    let (_, entry) = pass.runs.hold(run, &reason)?;
    pass.events
        .append(
            ITEM_ENTRY,
            pass.controller,
            serde_json::json!({ "item": run, "entry": entry, "kind": HELD }),
        )
        .map_err(|e| format!("{run} is held and {ITEM_ENTRY} did not reach the stream: {e}"))
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
            pass.controller,
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
