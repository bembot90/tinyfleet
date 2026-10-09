//! The run lifecycle's controller half: a waiting run re-run once the stream
//! carries a line its wake could be satisfied by, a run nothing could classify
//! re-run to a cap and then held, and the seats a run spawned let go when it
//! ends.
//!
//! WHY THE ACTS ARE A SEAM AND THE DECISION IS NOT. Every one of the three
//! acts needs a project resolved: a re-run needs the project's store, its
//! packs and its policy file; a park needs the store's hold; a retire needs
//! the project's primary checkout and its worktrees directory. That
//! resolution is wired by [`Engine`] below, and the acts stay a seam so a poll
//! can be driven against a file and a stub. The DECISION is different: it is
//! a fold of the machine's own stream against one cap, and the stream is this
//! crate's own file — so it lives here.
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
use std::path::{Path, PathBuf};

use fleet_core::entry::{Body, HoldReason};
use fleet_core::item::brief::Packs;
use fleet_core::item::hold;
use fleet_core::item::run as workflow_run;
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::seat::identity::identity_or_mint;
use fleet_core::store::ItemId;

use crate::project::stream::StreamEvents;
use crate::project::wiring::{verb_host, Wired};
use crate::project::{open_store, resolve_from, Here};
use crate::transient::{self, withdrawn_from, Refusal};
use crate::{clock, config, platform};

/// The environment variable a run's child carries, and every child of that
/// child: `fleet seat spawn` under a workflow reads its own process's copy and
/// the seat it starts is tagged with it.
///
/// THE RUN LIFECYCLE'S NAME: core's own, re-exported here.
pub use fleet_core::item::run::ENV_RUN_ID;

/// The six kinds of the run lifecycle this pass folds, and the one it writes:
/// core's, re-exported here.
pub use fleet_core::item::{
    RUN_CANCELLED, RUN_CLEANED, RUN_CLOSED, RUN_COULD_NOT_TELL, RUN_FAILED, RUN_STARTED,
    RUN_WAITING,
};

/// The one line every entry on an item's timeline is signalled on,
/// `{item, entry, kind}`: what a `run.waiting` whose wake names entry kinds is
/// woken by, and what the park writes for the `held` entry it raised. Core's,
/// re-exported here.
///
/// A SIGNAL AND NOT THE RECORD. The SDK's `until` and `hold` read the store
/// and name the entries that could satisfy them; the line says only that one
/// was written, and the re-run reads the record for itself.
pub use fleet_core::item::ITEM_ENTRY;

/// Every kind an entry is — `fleet_core::entry::KINDS`, re-exported under this
/// name: the kinds a wake may name.
pub use fleet_core::entry::KINDS as ENTRY_KINDS;

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

/// What the pass asks of whatever can act on a run — the three acts and the
/// one read of a run's record, which [`Engine`] makes and a suite's stub stands
/// in for.
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

// ---- the seam, filled -------------------------------------------------------
//
// The run seam, filled: the three acts the controller's run pass needs — re-run
// a run's bundle, hold a run at its crash cap, and retire the seats a finished
// run spawned — and the one read, whether a run's record already carries that
// hold.
//
// WHY THE ACTS ARE A SEAM AND NOT A CALL. core depends on no other member, so
// it cannot wire a store, a project's packs and the transient-seat primitives
// at once; [`Engine`] can, and the tick's run pass still takes it through
// [`Runs`] so a poll can be driven against a file and a stub
// (controller/tests/runs.rs).
//
// NOTHING IS HELD BETWEEN PASSES. Every act resolves the run's project afresh
// off the machine directory, so a run opened between two passes is picked up
// by the second with no registration of any kind.

/// The run seam's acts, over one machine directory.
///
/// IT HOLDS NO PROJECT. A run's directory carries its inputs, its policy and
/// its bundle and nothing that says where its record lives, and a controller
/// started as a service has no working directory to resolve one from — so the
/// record is looked for, project by project, over the roots this machine
/// registers.
pub struct Engine {
    pub machine_dir: PathBuf,
    pub home: PathBuf,
}

impl Engine {
    pub fn on(machine_dir: PathBuf) -> Engine {
        Engine {
            machine_dir,
            home: platform::home_dir(),
        }
    }

    /// Who the pass's acts are made by [ASSUMES D8]: the controller, under this
    /// machine's own identity — minted here where the machine has none yet, as
    /// a verb run on it with no actor would mint it. Every write the three acts
    /// make to a work graph, and every line they add to the stream, carries it.
    fn controller(&self) -> Result<Actor, String> {
        let (identity, _) = identity_or_mint(&self.machine_dir)
            .map_err(|why| format!("could not tell who acts: {why}"))?;
        Ok(Actor {
            kind: ActorKind::Controller,
            id: identity.id.to_string(),
        })
    }

    /// The project whose store holds this run's record, resolved.
    ///
    /// THE STORE IS THE INDEX and the walk stops at the first answer: a run id
    /// is the store's own, so two projects cannot both hold one. A project that
    /// will not resolve is skipped rather than refused — the run may be in the
    /// next one, and a machine is not broken because one of its roots moved.
    fn project_holding(&self, run: &str) -> Result<Here, String> {
        for root in registered_roots(&self.machine_dir) {
            let Ok(here) = resolve_from(&root, self.machine_dir.clone(), None) else {
                continue;
            };
            let holding = open_store(&here).is_ok_and(|store| store.show(run).is_ok());
            if holding {
                return Ok(here);
            }
        }
        Err(format!(
            "no project registered with this machine holds a record for {run}"
        ))
    }
}

/// The projects this machine holds: what the run pass looks a run's record up
/// across, and what `fleet status` counts the open holds over.
///
/// The register is the standalone fleets'; an embedded fleet's policy file
/// sits at its project's own root, so that file's directory is the project.
/// A standalone fleet's policy is under the machine directory, which is why
/// the machine directory itself is never taken for a project.
pub fn registered_roots(machine_dir: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = crate::lifecycle::registered(machine_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|project| PathBuf::from(project.root))
        .collect();
    if let Ok(machine) = config::read(&config::path_in(machine_dir)) {
        if let Some(root) = machine.fleet_toml.parent() {
            if root != machine_dir && !roots.iter().any(|held| held == root) {
                roots.push(root.to_path_buf());
            }
        }
    }
    roots
}

/// The controller decides which runs move — a fold of the machine's own stream
/// — and these are the three acts that decision needs: each resolves the
/// project whose store holds the run, and a machine with one project asks one
/// store one question.
impl Runs for Engine {
    fn rerun(&self, run: &str) -> Result<(), String> {
        let here = self.project_holding(run)?;
        let by = self.controller()?;
        let store = open_store(&here).map_err(|stop| stop.message)?;
        let packs =
            Packs::under(&here.packs_dir, &here.defaults_dir).map_err(|stop| stop.message)?;
        let events = StreamEvents::at(events::path_in(&self.machine_dir));
        let fleet_bin = std::env::current_exe()
            .map_err(|e| format!("this process cannot name its own binary: {e}"))?;
        let at = clock::now_stamp();
        // This pass is a launchd service, so the run's children are handed a
        // constructed search path rather than this process's own.
        let child_path = platform::child_path(&self.home);
        // The loop has no terminal of its own and its stdout is the service's
        // log, so the run's two lines go where a refusal already goes.
        workflow_run::rerun(
            &mut std::io::stderr(),
            &workflow_run::Again {
                run,
                by: &by,
                at: &at,
                machine_dir: &self.machine_dir,
                fleet_bin: &fleet_bin,
            },
            &workflow_run::Wiring {
                store: store.as_ref(),
                project: &here.project,
                packs: &packs,
                policy_file: &here.policy_file,
                events: &events,
                stream: &events,
                child_path: &child_path,
            },
        )
        .map(|_| ())
        .map_err(|stop| stop.message)
    }

    /// THE PARK AND NOT THE BARE HOLD: the held entry beside the hold is
    /// what `fleet clear` clears it through, and what the pass's signal names.
    fn hold(&self, run: &str, reason: &str) -> Result<(String, String), String> {
        let here = self.project_holding(run)?;
        let by = self.controller()?;
        let store = open_store(&here).map_err(|stop| stop.message)?;
        let directory = self.machine_dir.join(workflow_run::RUNS).join(run);
        hold::park_at_the_cap(
            &hold::Capped {
                run,
                reason,
                directory: &directory,
                by: &by,
            },
            store.as_ref(),
        )
        .map_err(|stop| stop.message)
    }

    /// The LAST held entry of reason `max_crashes` on the run's timeline, and
    /// whether the store still lists its hold open. The run's own asks are
    /// held entries too, and none of them is the park.
    fn capped(&self, run: &str) -> Result<Option<CapHold>, String> {
        let here = self.project_holding(run)?;
        let store = open_store(&here)
            .map_err(|stop| format!("{run}'s store could not be opened: {stop}"))?;
        let entries = store
            .timeline(&ItemId::from(run))
            .map_err(|e| format!("{run}'s timeline could not be read: {e}"))?;
        let Some(hold) = entries.iter().rev().find_map(|entry| match &entry.body {
            Body::Held(held) if held.reason == HoldReason::MaxCrashes => Some(held.hold.clone()),
            _ => None,
        }) else {
            return Ok(None);
        };
        let open = store
            .holds_open()
            .map_err(|e| format!("the store's open holds could not be read for {run}: {e}"))?;
        Ok(Some(CapHold {
            cleared: !open.iter().any(|held| *held == hold),
            hold,
        }))
    }

    fn retire(&self, seat: &str, run: &str) -> Result<(), String> {
        let here = self.project_holding(run)?;
        let by = self.controller()?;
        let wired = Wired::of(&here, &self.home, |home| Ok(verb_host(home)))
            .map_err(|stop| stop.message)?;
        let machine = wired.machine(&here);
        // THE RECORD'S HALF. A cleanup retires seats whose items were delivered and
        // seats whose items are still open — a park leaves the order standing —
        // and the name this frees is the one the next spawn takes.
        let store = open_store(&here).map_err(|stop| stop.message)?;
        // The retire hands its withdrawal the machine name of the row it
        // resolved; the order was assigned to that row's ID, so the name is
        // resolved back to it here, exactly, through the short id it carries.
        let withdrawal = |going: &str| {
            let row = here
                .seats
                .resolve_running(going)
                .map_err(|unresolved| Refusal {
                    code: unresolved.code(),
                    message: unresolved.to_string(),
                })?;
            withdrawn_from(
                store.as_ref(),
                &row.id,
                &here.seats.label(&row.id),
                &by,
                None,
            )
        };
        // THE PRICED RETIRE and not the bare one: a seat a run spawned costs
        // what any spawned seat costs, and the run is the item it was working
        // for. It writes `session.retired`, which is the line the cleanup's
        // count is a count of.
        transient::priced_with(&machine, seat, run, clock::now_ms(), &withdrawal)
            .map(|_| ())
            .map_err(|refusal| refusal.message)
    }
}

/// The run seam's own half of the retire, which no end-to-end arm reaches:
/// [`transient::withdrawn_from`] is called here with no unread name, as the
/// [`Engine`] calls it, so a `cfg(test)` module here is where its strict path is
/// pinned, and the machine, worktree and session table an [`Engine`] resolves
/// around it are the controller suite's.
#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::entry::{Body, OrderWithdrawn, Withdrawal};
    use fleet_core::item::COULD_NOT_TELL;
    use fleet_core::seat::identity::SeatId;
    use fleet_core::store::{Item, Order, OrderKind, OrderState, Stamp, Status, Store};
    use fleet_core::test_support::FakeStore;

    /// The seat's full id, which the order was assigned to and the withdrawal
    /// names, and its machine name, which a sentence says.
    const SEAT: &str = "018f6a2c-1d3e-7a4b-9c5d-00000c3a5e71";
    const LABEL: &str = "agent-0c3a5e71";
    /// This machine's identity, which the controller acts under.
    const MACHINE: &str = "018f6a2c-1d3e-7a4b-9c5d-0000a1b2c3d4";

    fn seat() -> SeatId {
        SeatId::parse(SEAT).expect("the seat's id parses")
    }

    /// The pass's own actor, as [`Engine::controller`] builds it.
    fn controller() -> Actor {
        Actor {
            kind: ActorKind::Controller,
            id: MACHINE.to_string(),
        }
    }
    const PARKED: &str = "fx-parked";

    /// The state the leak lives in: a run's item PARKED, so it is still open
    /// and the order naming the seat still stands when the cleanup retires it.
    fn a_parked_item() -> FakeStore {
        let store = FakeStore::default();
        store.seed(Item {
            id: PARKED.into(),
            title: String::from("an item a seat was dispatched and parked"),
            status: Status::Open,
            assignee: Some(seat()),
            order: OrderState::Ordered(Order {
                kind: OrderKind::Dispatch,
                by: Actor {
                    kind: ActorKind::Run,
                    id: String::from("a-run"),
                },
                seat: Some(seat()),
                at: Stamp::parse("2026-09-14T10:40:39Z").expect("a stamp"),
            }),
            ..Item::default()
        });
        store
    }

    #[test]
    fn the_cleanups_withdrawal_releases_the_parked_item_the_seat_holds() {
        let store = a_parked_item();

        let withdrawn = withdrawn_from(&store, &seat(), LABEL, &controller(), None)
            .expect("the board answers and the withdrawal lands");
        assert_eq!(withdrawn, vec![PARKED.to_string()]);

        let after = store.show(PARKED).expect("the store answers");
        assert!(
            after.assignee.is_none(),
            "the item reads unassigned: {:?}",
            after.assignee
        );
        assert_eq!(
            after.order,
            OrderState::None,
            "and carries no order index: {}",
            after.proof.as_str()
        );
        assert_eq!(
            after.status,
            Status::Open,
            "the work itself is still to be done"
        );
        let entries = store
            .timeline(&ItemId::from(PARKED))
            .expect("the store answers the timeline");
        assert_eq!(
            entries
                .iter()
                .map(|entry| (&entry.body, &entry.by))
                .collect::<Vec<_>>(),
            vec![(
                &Body::OrderWithdrawn(OrderWithdrawn {
                    why: Withdrawal::Retire,
                    seat: Some(seat()),
                    cause: None,
                }),
                &controller(),
            )],
            "the withdrawal names the seat and says who took it, and the cleanup is the \
             controller"
        );
        assert!(
            store
                .wrote()
                .iter()
                .all(|line| line.ends_with(&format!(" controller:{MACHINE}"))),
            "every write is the controller's, typed: {:?}",
            store.wrote()
        );
    }

    /// A seat holding nothing ordered is retired as it always was: the board is
    /// read once and nothing is written.
    #[test]
    fn a_seat_holding_nothing_ordered_is_withdrawn_from_without_a_write() {
        let store = FakeStore::default();

        let withdrawn =
            withdrawn_from(&store, &seat(), LABEL, &controller(), None).expect("the board answers");

        assert!(withdrawn.is_empty(), "{withdrawn:?}");
        assert!(
            store.wrote().is_empty(),
            "nothing written: {:?}",
            store.wrote()
        );
    }

    /// A board that will not answer stops the cleanup's retire rather than
    /// freeing the name over silence, and the exit is carried across unchanged.
    #[test]
    fn a_board_that_will_not_answer_stops_the_cleanups_retire() {
        let store = FakeStore {
            unreadable: Some(String::from("the store is not on this path")),
            ..FakeStore::default()
        };

        let refused = withdrawn_from(&store, &seat(), LABEL, &controller(), None)
            .expect_err("an unreadable board is a question");

        assert_eq!(refused.code, COULD_NOT_TELL);
        assert!(
            refused.message.contains("the store is not on this path"),
            "{}",
            refused.message
        );
    }

    /// [ASSUMES D8] The pass acts as the controller under THIS MACHINE'S
    /// identity: minted by the first act where the machine has none, and read
    /// back unchanged by every act after it.
    #[test]
    fn the_engine_acts_as_the_controller_under_this_machines_identity() {
        let dir = std::env::temp_dir().join(format!("fleet-runs-actor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let engine = Engine::on(dir.join("machine"));

        let first = engine.controller().expect("the identity is minted");
        let (identity, minted) =
            identity_or_mint(&dir.join("machine")).expect("the identity reads back");
        assert!(!minted, "the engine's first act minted it");
        assert_eq!(first.kind, ActorKind::Controller);
        assert_eq!(first.id, identity.id.to_string());
        assert_eq!(first.to_string(), format!("controller:{}", identity.id));
        assert_eq!(
            engine.controller().expect("the identity reads"),
            first,
            "every act after the first is the same controller"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
