//! `fleet run <workflow> [--input key=value]…` — the whole run lifecycle: the
//! resolution, the run directory, the pinned inputs, the bundle the pack
//! writes and the hash over all three, then the pack's run command and the
//! exit table its process answers on.
//!
//! EVERY READ AND EVERY REFUSAL PRECEDES EVERY WRITE, as `fly`'s do. The cap,
//! the workflow's resolution, the `[runtime]` table the owning pack declares
//! or imports and its doctor all answer before the record item is filed, so a
//! refusal here leaves the machine directory exactly as it found it — which is
//! what makes "refuses by name before anything is written" a property a reader
//! can check by listing the directory twice.
//!
//! WHAT CORE KNOWS ABOUT A WORKFLOW is the `[runtime]` table and nothing else:
//! it substitutes the five placeholders into the pack's two lines, execs them,
//! hashes what the bundle child left, and reads of the run child its exit and
//! the one line of JSON the exit table turns into a reason or a wake
//! condition. It parses no language and reads no other output.
//!
//! A WORKFLOW PROCESS IS SHORT-LIVED. It runs to closed, failed or waiting and
//! exits, so what a run holds between its runs is its record, its directory
//! and its events — never a process somebody has to find again.
//!
//! THE CARRIER'S SETTINGS RIDE THE INPUTS FILE, under `config`: the values
//! `[packs.<name>]` sets for the pack that carries the workflow, over the
//! defaults its manifest declares ([`crate::settings`]). They are pinned there
//! rather than in a fourth file so the hash already covers them, the document
//! the workflow is handed on stdin already carries them, and a run directory
//! written before they existed hashes as it always did. A re-run reads them off
//! that file and never off `fleet.toml`.
//!
//! WRITE ORDER is the record, then the directory, then the pins, then the
//! event, as `plan`'s and `fly`'s are, and the back half keeps the polarity:
//! the logs, then the record's close, then the event. A crash between them
//! leaves a run whose record says how it ended and whose stream does not,
//! which a fold reads as the record says; the reverse would announce an end no
//! record holds.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::add;
use crate::entry::{self, Body};
use crate::item::brief::Packs;
use crate::item::doctor::{self, Invocation, Verdict, DOCTOR_TOML, RUNTIME_VERSION, SLOT};
use crate::item::pins;
use crate::item::{
    control_token, read_table, recorded, signal, Events, Project, Stop, Unrecorded, RUN_CANCELLED,
    RUN_CLOSED, RUN_COULD_NOT_TELL, RUN_FAILED, RUN_STARTED, RUN_WAITING,
};
use crate::lock;
use crate::pack::{self, Runtime};
use crate::policy;
use crate::resolve::Layer;
use crate::seat::actor::Actor;
use crate::settings;
use crate::store::{keys, Item, NewItem, Store, StoreError};

/// Where the run directories go, under the machine directory.
pub const RUNS: &str = "runs";

/// The three names inside one, and the order the hash reads them in. `BUNDLE`
/// is the one the pack's own bundle command writes; the other two are pinned
/// here before that child runs, and `INPUTS` carries the carrier's resolved
/// settings under [`CONFIG`] beside the inputs themselves.
pub const INPUTS: &str = "inputs.toml";
pub const POLICY: &str = "policy.toml";
pub const BUNDLE: &str = "bundle";

/// The key the inputs file pins the carrier's settings under, and the key the
/// document on the workflow's stdin carries them under.
pub const CONFIG: &str = "config";

/// The slot a workflow name resolves through, overlay first.
pub const WORKFLOWS: &str = "workflows";

/// The label a run's record item carries, and the only mark that tells one from
/// every other item in the store. Namespaced, because a board fleet is added
/// to may already label its own items `run`, and a bare `run` means nothing to
/// fleet.
pub const LABEL: &str = "fleet:run";

/// The store's own word for the record item's type.
pub const RECORD_TYPE: &str = "task";

/// The title the record carries between the create and the retitle.
pub const UNTITLED: &str = "a run being filed";

/// The store's own word for a record that is closed.
const CLOSED: &str = "closed";

/// Runs open at once where `[core.run] max_open` names no number.
pub const MAX_OPEN: u64 = 4;

/// Executions of one run that nothing could classify, where `[core.run]
/// max_crashes` names no number. Two, as a flight's dispatch cap is: a first
/// answer nobody could read is a fact about one execution, and a second that
/// reads the same way is a fact about the run.
pub const MAX_CRASHES: u64 = 2;

/// What the run child said, captured whole beside the pins.
///
/// Neither is hashed: the hash is over what the run is PINNED to, and what a
/// run said is not an input to it. They sit in the same directory because the
/// reason on `run.failed` is one line and the context for it is these.
pub const STDOUT_LOG: &str = "stdout.log";
pub const STDERR_LOG: &str = "stderr.log";

/// The six names a workflow reads its own run off, and the machine directory
/// every child of this fleet carries.
///
/// THEY ARE WRITTEN HERE AND NOWHERE ELSE, so
/// the environment a workflow meets and any doc that lists it cannot drift
/// apart. `FLEET_DIR` is the controller's name for the same directory: a child
/// of a run and a child of a routine are told it the same way. `FLEET_PROJECT`
/// is the project root the run was started inside: the child's own cwd is the
/// run directory, under the machine and above no `fleet.toml`, so every verb a
/// workflow calls back into resolves its project from this name and not from
/// where it was started.
///
/// Beside them, five PASS-THROUGHS that are not names of the run: the five in
/// [`ENV_PASSED_THROUGH`], each copied from this process's own environment when
/// set there. `PATH` is not one of them — it is CONSTRUCTED, and
/// [`Wiring::child_path`] says out of what. The dispatch a workflow calls back
/// into resolves the agent binary on a child `PATH` the controller constructs
/// from `HOME` — under no `HOME` that path ends in a RELATIVE `.local/bin` and
/// finds nothing — or from `FLEET_CLAUDE_BIN`, which the controller reads
/// verbatim and refuses unless absolute, so it is forwarded as written and
/// never resolved here. `USER`, `TMPDIR` and `LANG` are here for the seat that
/// dispatch starts: the adapter's own pass-through list
/// (`fleet_controller::adapter::claude_code::PASSED_THROUGH`) copies them off
/// whatever process it runs in, and under a workflow that process is this
/// child — a seat started with no `USER` finds no keychain entry and comes up
/// logged out. Like `HOME`, they name the person's session and nothing of the
/// run. Nothing else crosses the clearing.
pub const ENV_RUN_ID: &str = "FLEET_RUN_ID";
pub const ENV_STREAM: &str = "FLEET_STREAM";
pub const ENV_STREAM_SEQ: &str = "FLEET_STREAM_SEQ";
pub const ENV_RUN_DIR: &str = "FLEET_RUN_DIR";
pub const ENV_BIN: &str = "FLEET_BIN";
pub const ENV_PROJECT: &str = "FLEET_PROJECT";
pub const ENV_DIR: &str = "FLEET_DIR";
pub const ENV_PASSED_THROUGH: [&str; 5] = ["HOME", "FLEET_CLAUDE_BIN", "USER", "TMPDIR", "LANG"];

/// Where the stream is and how far it has got.
///
/// A seam of its own rather than two more methods on [`Events`]: appending is
/// every verb's business and these two readings are the run's alone. A
/// workflow is handed the stream's path and the position it started from so it
/// can say what it is waiting for, and [`Stream::seq`] is read AGAIN when the
/// process exits — a position held from before the child ran would be a
/// promise that nothing moved while it did. Core opens the file for neither.
pub trait Stream {
    fn path(&self) -> PathBuf;
    fn seq(&self) -> u64;
}

/// The run started, as its arguments.
pub struct Order<'a> {
    /// The workflow's name, without its extension.
    pub workflow: &'a str,
    /// `--input key=value`, in the order the caller gave them.
    pub inputs: &'a [(String, String)],
    pub by: &'a Actor,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
    /// Where the run directory goes: a process fact, resolved by the caller
    /// like every other one.
    pub machine_dir: &'a Path,
    /// The fleet binary a workflow calls back into — `{fleet}` in the pack's
    /// templates. A process fact too, so core asks no environment for it.
    pub fleet_bin: &'a Path,
}

pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub project: &'a Project,
    pub packs: &'a Packs,
    /// The policy file in force, copied byte for byte: the cli resolves which
    /// file that is.
    pub policy_file: &'a Path,
    pub events: &'a dyn Events,
    /// The same stream [`Wiring::events`] appends to, read rather than written.
    pub stream: &'a dyn Stream,
    /// The `PATH` the pack's own children run under, constructed by the caller
    /// from `platform::child_path` and never read off this process. Empty means
    /// the caller has none and the children carry this process's `PATH`
    /// instead. What the pinned runtime adds to it: [`child_path_for`].
    pub child_path: &'a str,
}

/// The run started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub run: String,
    pub hash: String,
    pub directory: PathBuf,
    pub workflow: String,
}

/// How the run ended, as the exit table read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Closed,
    Failed,
    Waiting,
    CouldNotTell,
}

impl Ended {
    /// The kind this outcome is announced on. One function and not a match at
    /// each site, so the four rows of the table and the four kinds stay a
    /// bijection.
    pub fn kind(self) -> &'static str {
        match self {
            Ended::Closed => RUN_CLOSED,
            Ended::Failed => RUN_FAILED,
            Ended::Waiting => RUN_WAITING,
            Ended::CouldNotTell => RUN_COULD_NOT_TELL,
        }
    }

    /// The word the verb's own second line ends on.
    pub fn word(self) -> &'static str {
        match self {
            Ended::Closed => "closed",
            Ended::Failed => "failed",
            Ended::Waiting => "waiting",
            Ended::CouldNotTell => "could not tell",
        }
    }

    /// Whether the record item is closed with the event.
    ///
    /// THE OPEN SET IS THE STORE'S and not a field beside it: `[core.run]
    /// max_open` is measured with `bd list --status open`, so a run leaves that
    /// set by being closed and by nothing else. A waiting run and one nothing
    /// could classify are both still open — the first because the controller
    /// re-runs it, the second because retiring a run nobody could read would
    /// spend the cap's answer on a guess.
    pub fn retires_the_record(self) -> bool {
        matches!(self, Ended::Closed | Ended::Failed)
    }
}

/// The run, start to exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub started: Started,
    pub ended: Ended,
}

/// One workflow resolved: the file, and the pack that carries it.
///
/// The PACK and not just the path, because the `[runtime]` table that says how
/// to bundle the file is read from the layer the file came off, or from the
/// packs that layer imports — never from whatever else is installed beneath
/// it. Which of those it is: [`read_the_runtime`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub name: String,
    pub relative: String,
    pub entry: PathBuf,
    pub pack: Layer,
}

pub fn run(out: &mut dyn Write, order: &Order, wiring: &Wiring) -> Result<Ran, Stop> {
    // (a) THE READS. Nothing below writes until every one of them answered.
    // A test command in the policy file first: the workflow a person is
    // opening is where it is set now, and a run opened beside it would land
    // untested while they believe otherwise.
    wiring.project.refuse_moved()?;
    let open = open_runs(wiring)?;
    refuse_at_the_cap(&open, wiring)?;
    let resolved = resolve_workflow(order.workflow, wiring)?;
    let pinned = read_the_runtime(&resolved, wiring, order.machine_dir)?;
    let path = child_path_for(&pinned.runtime.name, wiring.child_path);
    the_doctor_is_green(&resolved, &pinned, &path, wiring)?;
    let policy_bytes = std::fs::read(wiring.policy_file).map_err(|e| {
        Stop::could_not_tell(format!(
            "the policy in force at {} could not be read: {e}",
            wiring.policy_file.display()
        ))
    })?;
    let config = read_the_settings(&policy_bytes, &resolved, wiring)?;
    let inputs = render_inputs(order, &resolved, config)?;

    // (b) THE RECORD, which is what names the directory: the store names what
    // it files, so the id this run is titled by does not exist until the create
    // answers.
    let id = file_the_record(order, &resolved, wiring)?;

    // (c) THE PINS, then the bundle the pack writes, then the hash over both.
    // A refusal from here to `run.started` is a run that never started, and
    // the record filed for it is closed on the way out.
    let directory = order.machine_dir.join(RUNS).join(&id);
    let pinning = Pinning {
        id: &id,
        directory: &directory,
        inputs,
        policy_bytes,
        resolved: &resolved,
        pinned: &pinned,
        path: &path,
    };
    let hash = pin_and_bundle(pinning, order, wiring)
        .map_err(|stop| never_started(&id, stop, order.by, wiring))?;

    wiring
        .events
        .append(
            RUN_STARTED,
            order.by,
            serde_json::json!({
                "run": id,
                "hash": hash,
                "workflow": resolved.name,
            }),
        )
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{id} is started and {RUN_STARTED} did not reach the stream: {e}\n  the record \
                 STANDS and the run is open"
            ))
        })?;

    writeln!(out, "{id} — {hash}")
        .map_err(|e| Stop::could_not_tell(format!("the run line could not be written: {e}")))?;
    let started = Started {
        run: id,
        hash,
        directory,
        workflow: resolved.name.clone(),
    };

    // (d) THE BACK HALF: the pack's run line, the exit table, one event.
    let ended = execute(out, &started, &resolved, &pinned, order, &path, wiring)?;
    Ok(Ran { started, ended })
}

/// One run to be executed AGAIN, as its arguments.
///
/// A SEPARATE SHAPE FROM [`Order`] and not a flag on it, because a re-run names
/// no workflow and pins no inputs: both were decided when the run was opened and
/// both are read back off the record and the directory. What a caller supplies
/// is the id, who is asking, when, and the two process facts core asks no
/// environment for.
pub struct Again<'a> {
    pub run: &'a str,
    pub by: &'a Actor,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
    pub machine_dir: &'a Path,
    pub fleet_bin: &'a Path,
}

/// The back half again, over the bundle the run directory already holds.
///
/// THE SMALLEST ENTRY A SECOND CALLER NEEDS. `run` above is the front half and
/// the back half in one act; this is the back half alone, for the controller,
/// which re-runs a run whose stream moved past the position it stopped at.
///
/// NEVER A FRESH BUNDLE. The program a run is pinned to is the bundle written at
/// the open, and re-bundling would run a different program under the same id and
/// the same hash. So the pack's bundle command is not run, and the hash over the
/// directory is RECOMPUTED and checked against the one on the record: a
/// directory that moved under a run is refused rather than executed, because a
/// re-run whose program changed is not a re-run of anything.
///
/// It writes [`RUN_STARTED`] before the child, as the open does, so a reader
/// counting that kind on the stream counts executions and not opens.
pub fn rerun(out: &mut dyn Write, again: &Again, wiring: &Wiring) -> Result<Ended, Stop> {
    let record = read(wiring.store, again.run)?;
    // A CLOSED RECORD IS AN ENDED RUN, however it came to be closed — a cancel,
    // or a person's own close — and whatever the stream last said about it.
    if record.status == CLOSED {
        return Err(Stop::refused(format!(
            "{} is closed — a closed run is not executed again",
            again.run
        )));
    }
    let object = object_of(&record)?.ok_or_else(|| {
        Stop::refused(format!(
            "{} carries no `{}` object — a re-run is over a run this fleet opened, and the \
             record does not say it opened one",
            again.run,
            keys::RUN
        ))
    })?;
    let pinned = |key: &str| -> Result<String, Stop> {
        object
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                Stop::refused(format!(
                    "{}'s `{}` object names no {key} — the pins a re-run reads are the ones the \
                     open wrote",
                    again.run,
                    keys::RUN
                ))
            })
    };
    let hash = pinned("hash")?;
    let workflow = pinned("workflow")?;
    let relative = pinned("entry")?;
    let pack_name = pinned("pack")?;

    let directory = again.machine_dir.join(RUNS).join(again.run);
    if !directory.join(BUNDLE).is_file() {
        return Err(Stop::refused(format!(
            "{} holds no {BUNDLE} — the program a run is pinned to is the bundle, and this \
             re-run will not write a new one",
            directory.display()
        )));
    }
    let found = pins::hash_of(&directory, &hashed_files())?;
    if found != hash {
        return Err(Stop::refused(format!(
            "{} hashes {found} and {} is pinned to {hash} — what is in the directory is not what \
             this run was opened against",
            directory.display(),
            again.run
        )));
    }

    let pack = wiring
        .packs
        .layers
        .iter()
        .find(|layer| layer.name == pack_name)
        .cloned()
        .ok_or_else(|| {
            Stop::refused(format!(
                "{} is pinned to the pack `{pack_name}`, which is not one of the installed layers",
                again.run
            ))
        })?;
    let resolved = Resolved {
        name: workflow.clone(),
        entry: pack.root.join(&relative),
        relative,
        pack,
    };
    let pinned = read_the_runtime(&resolved, wiring, again.machine_dir)?;
    let path = child_path_for(&pinned.runtime.name, wiring.child_path);

    // The two `Order` fields a re-run does not carry. The workflow is the pinned
    // name so the shape reads true; the inputs are empty because the child is
    // handed the pinned file and never this list.
    let no_inputs: Vec<(String, String)> = Vec::new();
    let order = Order {
        workflow: &workflow,
        inputs: &no_inputs,
        by: again.by,
        at: again.at,
        machine_dir: again.machine_dir,
        fleet_bin: again.fleet_bin,
    };
    let started = Started {
        run: again.run.to_string(),
        hash,
        directory,
        workflow: workflow.clone(),
    };
    wiring
        .events
        .append(
            RUN_STARTED,
            again.by,
            serde_json::json!({
                "run": started.run,
                "hash": started.hash,
                "workflow": started.workflow,
            }),
        )
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{} is running again and {RUN_STARTED} did not reach the stream: {e}\n  the \
                 record STANDS and the child has NOT been started",
                started.run
            ))
        })?;
    execute(out, &started, &resolved, &pinned, &order, &path, wiring)
}

/// The relative paths of one run directory, in the order the hash reads them.
///
/// A reader that wants to recompute a hash takes the list from here rather than
/// from its own traversal, so the order is one function and not two that agree
/// today. It takes no directory because a run's three names are fixed, where a
/// flight's briefs are its list's.
pub fn hashed_files() -> Vec<String> {
    vec![INPUTS.to_string(), POLICY.to_string(), BUNDLE.to_string()]
}

// ---- the cancel ----------------------------------------------------------------

/// One run to be cancelled, as its arguments.
pub struct Cancel<'a> {
    pub run: &'a str,
    /// Who is cancelling it.
    pub by: &'a Actor,
}

/// The run cancelled, and the holds the cancel cleared on its record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancelled {
    pub run: String,
    pub holds: Vec<String>,
}

/// A run ended by hand: every hold standing on its record cleared — a
/// `cleared` entry of how `cancel` on the record, then the store's hold — the
/// record closed as cancelled, then [`RUN_CANCELLED`] and one `cleared` signal
/// per cleared entry on the stream.
///
/// THE WAY OUT FOR A RUN NOTHING ELSE ENDS. A run whose process died before it
/// wrote a row of the exit table, one held at `[core.run] max_crashes`, one
/// waiting on a wake that will never come: each holds its record open, and the
/// open records are what `[core.run] max_open` counts.
///
/// THE HOLDS ARE THE STORE'S ANSWER, not the timeline's: the record's own open
/// dependencies that the store lists as open holds. A hold nothing on the
/// record names blocks the close exactly as one a held entry names does, and
/// is cancelled the same way.
///
/// IT STOPS NO PROCESS AND RETIRES NO SEAT. A run holds no process between its
/// executions, and one under way when the cancel lands runs to its end; the
/// controller's run pass reads [`RUN_CANCELLED`] as an ending — it executes the
/// run no more, whatever that execution writes after it — and retires the seats
/// the run spawned with the cleanup every ending gets.
///
/// THE STORE'S WRITES, THEN THE STREAM'S, and [`RUN_CANCELLED`] ahead of the
/// holds' lines: a pass that read a cleared hold before the cancel that
/// cleared it would be reading a run still standing.
pub fn cancel(
    out: &mut dyn Write,
    cancel: &Cancel,
    store: &dyn Store,
    events: &dyn Events,
) -> Result<Cancelled, Stop> {
    let run = cancel.run;
    // The string form every write and every line carries.
    let by = cancel.by.to_string();
    let record = store.show(run).map_err(|e| match e {
        StoreError::Missing(why) => Stop::refused(format!(
            "{run} is not an item this project's store holds — {why}"
        )),
        // A read never answers `Moved`, which only a fenced write does.
        StoreError::Unreadable(why) | StoreError::Moved(why) => {
            Stop::could_not_tell(format!("{run} could not be read: {why}"))
        }
    })?;
    // Resolved once: the close, the events and the line name the record's own
    // id from here on, whatever part of it was typed.
    let run = record.id.as_str();
    if !record.labels.iter().any(|label| label == LABEL) {
        return Err(Stop::refused(format!(
            "{run} is not a run's record — it carries no `{LABEL}` label, and `fleet cancel` ends \
             runs and nothing else"
        )));
    }
    if record.status == CLOSED {
        return Err(Stop::refused(format!(
            "{run} is closed already — the run has ended and there is nothing to cancel"
        )));
    }

    let open = store
        .open_holds()
        .map_err(|e| Stop::could_not_tell(format!("the store's holds could not be read: {e}")))?;
    let holds: Vec<String> = record
        .blockers
        .iter()
        .filter(|blocker| open.contains(blocker))
        .cloned()
        .collect();
    // ONE CLEARED ENTRY PER HOLD, before the store's hold is cleared: the
    // record says the hold was cancelled, never that somebody chose no letter.
    let mut entries: Vec<String> = Vec::with_capacity(holds.len());
    for hold in &holds {
        let cancelled = Body::Cleared(entry::Cleared {
            hold: hold.clone(),
            how: entry::Clearance::Cancel,
            letter: None,
            text: None,
        });
        let entry = recorded(store, run, &cancelled, cancel.by).map_err(|unrecorded| {
            let why = match unrecorded {
                Unrecorded::NotWritten(e) => {
                    format!("the cleared entry for {hold} did not land on {run}: {e}")
                }
                Unrecorded::Unconfirmed(why) => why,
            };
            Stop::could_not_tell(format!("{why}\n  {run} is NOT cancelled"))
        })?;
        store.clear_hold(hold, &by).map_err(|e| {
            Stop::could_not_tell(format!(
                "{hold} on {run} was not cleared: {e}\n  {run} is NOT cancelled"
            ))
        })?;
        entries.push(entry);
    }

    store.close(run, "the run cancelled", &by).map_err(|e| {
        Stop::could_not_tell(format!(
            "{run}'s record did not close: {e}\n  {} cleared and {run} is NOT cancelled",
            named_holds(&holds)
        ))
    })?;
    let read_back = read(store, run)?;
    if read_back.status != CLOSED {
        return Err(disagrees(run, "status", CLOSED, &read_back.status));
    }

    let written = |e: String| {
        Stop::could_not_tell(format!(
            "{run} is cancelled and its lines did not all reach the stream: {e}\n  the record \
             is CLOSED and {} cleared",
            named_holds(&holds)
        ))
    };
    events
        .append(RUN_CANCELLED, cancel.by, serde_json::json!({ "run": run }))
        .map_err(written)?;
    for entry in &entries {
        signal(events, cancel.by, run, entry, "cleared").map_err(written)?;
    }

    let line = if holds.is_empty() {
        format!("{run} — cancelled")
    } else {
        format!("{run} — cancelled, {} cleared", named_holds(&holds))
    };
    writeln!(out, "{line}")
        .map_err(|e| Stop::could_not_tell(format!("the cancel line could not be written: {e}")))?;
    Ok(Cancelled {
        run: run.to_string(),
        holds,
    })
}

/// The holds a cancel cleared, named as the line about them names them.
fn named_holds(holds: &[String]) -> String {
    match holds {
        [] => String::from("no hold"),
        [one] => format!("hold {one}"),
        many => format!("holds {}", many.join(", ")),
    }
}

// ---- (a) the reads -----------------------------------------------------------

/// Every open record carrying the run label, by id, sorted.
///
/// The front half opens runs and closes none, so every run this fleet has
/// started and not had closed for it is here — which is the set the cap is
/// measured against and the set a refusal names.
pub fn open_runs(wiring: &Wiring) -> Result<Vec<String>, Stop> {
    let mut open = wiring
        .store
        .open_labelled(LABEL)
        .map_err(|e| Stop::could_not_tell(format!("the work graph could not be read: {e}")))?;
    open.sort();
    Ok(open)
}

/// `[core.run] max_open`, or the default where the fleet named none.
pub fn max_open(guards: &toml::Table) -> Result<u64, Stop> {
    let value = policy::read("core.run", "max_open", guards)
        .map_err(|unlisted| Stop::could_not_tell(unlisted.to_string()))?;
    match value {
        None => Ok(MAX_OPEN),
        Some(value) => match value.as_integer() {
            Some(number) if number >= 0 => Ok(number as u64),
            _ => Err(Stop::refused(format!(
                "`[core.run] max_open` is {}, and a cap has to be a whole number — this fleet \
                 will not fall back to a cap it did not name",
                value.type_str()
            ))),
        },
    }
}

/// `[core.run] max_crashes`, or the default where the fleet named none.
///
/// A value of the wrong shape is refused rather than defaulted, as
/// [`max_open`]'s is: a fleet that wrote a cap down has said what it wants, and
/// falling back would run a crashing workflow the number of times this code
/// chose instead.
pub fn max_crashes(guards: &toml::Table) -> Result<u64, Stop> {
    let value = policy::read("core.run", "max_crashes", guards)
        .map_err(|unlisted| Stop::could_not_tell(unlisted.to_string()))?;
    match value {
        None => Ok(MAX_CRASHES),
        Some(value) => match value.as_integer() {
            Some(number) if number >= 0 => Ok(number as u64),
            _ => Err(Stop::refused(format!(
                "`[core.run] max_crashes` is {}, and a cap has to be a whole number — this fleet \
                 will not fall back to a cap it did not name",
                value.type_str()
            ))),
        },
    }
}

fn refuse_at_the_cap(open: &[String], wiring: &Wiring) -> Result<(), Stop> {
    let cap = max_open(&wiring.project.guards)?;
    if open.len() as u64 >= cap {
        let held = if open.is_empty() {
            String::from("none")
        } else {
            open.join(", ")
        };
        return Err(Stop::refused(format!(
            "{} run(s) are open and `[core.run] max_open` is {cap} — the open runs are {held}, \
             and one has to close or the cap has to be raised",
            open.len(),
        )));
    }
    Ok(())
}

/// `workflows/<name>.<ext>` through the layers, overlay first.
///
/// The EXTENSION IS NOT THE CALLER'S to give: a name is a name whatever language
/// the pack that carries it is written in, which is the whole point of core
/// being language-blind. So every resolved path under the slot whose stem is
/// the name is a candidate, and two of them is a refusal rather than a pick —
/// a fleet where `takeoff.ts` and `takeoff.sh` both resolve has two answers to
/// one name and core is not the one to choose.
fn resolve_workflow(name: &str, wiring: &Wiring) -> Result<Resolved, Stop> {
    let under = format!("{WORKFLOWS}/");
    let mut found: Vec<String> = wiring
        .packs
        .resolution
        .files
        .keys()
        .filter(|relative| {
            relative
                .strip_prefix(&under)
                .and_then(|tail| tail.rsplit_once('.'))
                .is_some_and(|(stem, _)| stem == name)
        })
        .cloned()
        .collect();
    found.sort();

    let relative = match found.as_slice() {
        [] => {
            return Err(Stop::refused(format!(
                "no workflow named `{name}` — no installed pack carries `{under}{name}.<ext>`"
            )))
        }
        [one] => one.clone(),
        many => {
            return Err(Stop::refused(format!(
                "`{name}` names {} files and a run takes one — {}",
                many.len(),
                many.join(", ")
            )))
        }
    };

    let carrier = wiring
        .packs
        .resolution
        .files
        .get(&relative)
        .expect("the path came off this resolution");
    let pack = wiring
        .packs
        .layers
        .iter()
        .find(|layer| &layer.name == carrier)
        .ok_or_else(|| {
            Stop::could_not_tell(format!(
                "`{relative}` resolves to the pack `{carrier}`, which is not one of the layers"
            ))
        })?
        .clone();
    Ok(Resolved {
        name: name.to_string(),
        entry: pack.root.join(&relative),
        relative,
        pack,
    })
}

/// The `[runtime]` table a run bundles and executes under, and the layer that
/// declares it — the carrier's own where it has one, else the one pack the
/// carrier imports that has one.
///
/// The DECLARING LAYER and not just the table, because the doctor check
/// measures a pack's manifest and the check's verdict is about that pack: a
/// carrier that imports its runtime has nothing pinned in its own manifest, so
/// a check run against it would read "nothing pinned" and answer green about
/// a runtime it never measured.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pinned {
    runtime: Runtime,
    pack: Layer,
}

pub(crate) fn manifest_of(layer: &Layer) -> Result<pack::Manifest, Stop> {
    let manifest = layer.root.join(pack::MANIFEST);
    let text = std::fs::read_to_string(&manifest).map_err(|e| {
        Stop::could_not_tell(format!("{} could not be read: {e}", manifest.display()))
    })?;
    pack::parse_manifest(&text).map_err(|defects| {
        Stop::refused(format!(
            "the pack `{}` does not parse: {}",
            layer.name,
            defects
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })
}

/// The runtime the carrier's workflow runs under: the carrier's own `[runtime]`
/// table first, else the nearest layer beneath it, in the resolved order, among
/// the packs the carrier imports — transitively, though the resolver holds
/// imports to one level — that declares one.
///
/// THE CARRIER'S OWN PIN WINS OUTRIGHT and the imports are not read: a pack
/// that pinned a runtime said what its workflows run under, and an import's
/// pin beneath it is that import's business. Beneath a carrier with none, two
/// declaring imports are a refusal naming both, never the higher one: core is
/// not the one to choose between two languages a pack asked for at once.
///
/// NONE AT ALL, where an import the carrier declares is not installed, names
/// that import and the line that adds it (fleet-4fw): the missing pack is the
/// likeliest carrier of the table, and the person reading the refusal is the
/// one who can add it. `machine_dir` is where the lock that line is read off
/// lives.
fn read_the_runtime(
    resolved: &Resolved,
    wiring: &Wiring,
    machine_dir: &Path,
) -> Result<Pinned, Stop> {
    let own = manifest_of(&resolved.pack)?;
    if let Some(runtime) = own.runtime {
        return Ok(Pinned {
            runtime,
            pack: resolved.pack.clone(),
        });
    }

    let layers = &wiring.packs.layers;
    let beneath = layers
        .iter()
        .position(|layer| layer.name == resolved.pack.name)
        .map(|at| at + 1)
        .unwrap_or(layers.len());
    let mut imported: BTreeSet<String> = own.imports.into_iter().map(|i| i.name).collect();
    let mut frontier: Vec<String> = imported.iter().cloned().collect();
    while let Some(name) = frontier.pop() {
        let Some(layer) = layers.iter().find(|layer| layer.name == name) else {
            continue;
        };
        for import in manifest_of(layer)?.imports {
            if imported.insert(import.name.clone()) {
                frontier.push(import.name);
            }
        }
    }

    let mut declaring: Vec<Pinned> = Vec::new();
    for layer in layers.iter().skip(beneath) {
        if !imported.contains(&layer.name) {
            continue;
        }
        if let Some(runtime) = manifest_of(layer)?.runtime {
            declaring.push(Pinned {
                runtime,
                pack: layer.clone(),
            });
        }
    }
    match declaring.len() {
        0 => {
            let unanswered: Vec<String> =
                add::missing_imports(layers, &machine_dir.join(lock::LOCK))
                    .into_iter()
                    .filter(|missing| {
                        missing.importer == resolved.pack.name
                            || imported.contains(&missing.importer)
                    })
                    .map(|missing| missing.to_string())
                    .collect();
            if unanswered.is_empty() {
                Err(Stop::refused(format!(
                    "`{}` carries `{}` and declares no [runtime] table, and no pack it imports \
                     declares one — that table is the one thing fleet reads about a workflow's \
                     language, so there is no command to bundle this file with",
                    resolved.pack.name, resolved.relative
                )))
            } else {
                Err(Stop::refused(format!(
                    "`{}` carries `{}` and declares no [runtime] table, and no installed pack it \
                     imports declares one: {}",
                    resolved.pack.name,
                    resolved.relative,
                    unanswered.join("; ")
                )))
            }
        }
        1 => Ok(declaring.remove(0)),
        _ => Err(Stop::refused(format!(
            "`{}` carries `{}` and declares no [runtime] table, and {} packs it imports each \
             declare one — {} — so the file has two runtimes and fleet does not choose between them",
            resolved.pack.name,
            resolved.relative,
            declaring.len(),
            declaring
                .iter()
                .map(|pinned| format!("`{}`", pinned.pack.name))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The `PATH` the doctor check, the bundle line and the run line all run
/// under: the caller's constructed child path, with the directory the pinned
/// runtime resolves from in front of it when that path does not already hold
/// the binary.
///
/// CONSTRUCTED, NEVER COPIED. A controller re-running a run is a service, and
/// a service's own `PATH` is the manager's — it holds neither a package
/// manager's prefix nor the user's local bin — so a run line handed that
/// `PATH` cannot exec the runtime its pack pins. The base comes from the
/// caller because the platform's directories are the caller's knowledge; an
/// empty base is a caller with no controller behind it, and the children then
/// carry this process's own `PATH` as they always have.
///
/// THE PREPEND MIRRORS THE PINNED RUNTIME'S DOCTOR CHECK, which looks for the
/// binary on `PATH` and then in the installer's bin — `<NAME>_INSTALL/bin`
/// where that variable names a root, else `$HOME/.<name>/bin`, which no
/// session `PATH` carries on its own. The check and the two lines it clears
/// have to resolve the same file, so the directory the check would accept is
/// put where the lines will look. Only when the base misses it: a runtime the
/// base already resolves keeps the platform's order, where the system
/// directories lead.
pub fn child_path_for(runtime: &str, base: &str) -> String {
    if base.is_empty() {
        return std::env::var("PATH").unwrap_or_default();
    }
    let name = runtime;
    if holding(base, name).is_some() {
        return base.to_string();
    }
    let found = holding(&std::env::var("PATH").unwrap_or_default(), name)
        .or_else(|| installer_bin(name).filter(|bin| is_program(&bin.join(name))));
    match found {
        Some(dir) => format!("{}:{base}", dir.display()),
        None => base.to_string(),
    }
}

/// The installer's bin for a runtime of this name, as its own doctor check
/// spells it.
fn installer_bin(name: &str) -> Option<PathBuf> {
    let variable: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .chain("_INSTALL".chars())
        .collect();
    let root = match std::env::var(&variable) {
        Ok(named) if !named.trim().is_empty() => PathBuf::from(named),
        _ => PathBuf::from(std::env::var("HOME").ok()?).join(format!(".{name}")),
    };
    Some(root.join("bin"))
}

/// The first directory of `path` that holds `name` as a program.
fn holding(path: &str, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find(|dir| is_program(&dir.join(name)))
}

fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|found| found.is_file() && found.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The pinned runtime's own doctor check, run as a child against the pack that
/// declares the pin.
///
/// THE CHECK'S EXIT AND NOT ITS PROSE: core reads rc alone and hands the lines
/// back to the person, because what the check knows about a runtime is the
/// check's and a parser here would be a second opinion about it.
///
/// It runs on [`child_path_for`], the `PATH` the bundle and run lines will
/// run on: a check measuring some other search path is green about a binary
/// those lines cannot exec, or red about one they can.
///
/// It runs through [`doctor::run`], so it is bounded by [`doctor::TIMEOUT`]:
/// a check that gave no exit of its own — past the bound, never started, or
/// killed by a signal — did not answer, which is could-not-tell and not red.
/// Any exit but 0 is red, as it always was.
fn the_doctor_is_green(
    resolved: &Resolved,
    pinned: &Pinned,
    path: &str,
    wiring: &Wiring,
) -> Result<(), Stop> {
    let declared = format!("{SLOT}/{RUNTIME_VERSION}/{DOCTOR_TOML}");
    let unmeasurable = || {
        Stop::could_not_tell(format!(
            "no installed pack carries `{declared}` — the pinned runtime cannot be measured, and \
             a run core cannot measure the runtime of is one it will not open"
        ))
    };
    let entry = doctor::entry(wiring.packs, RUNTIME_VERSION).ok_or_else(unmeasurable)?;
    if let Err(why) = &entry.script {
        return Err(if wiring.packs.resolution.files.contains_key(&declared) {
            Stop::could_not_tell(why.clone())
        } else {
            unmeasurable()
        });
    }

    let checked = doctor::run(
        &entry,
        &Invocation {
            pack_dir: &pinned.pack.root,
            path,
            cwd: None,
            env: &[],
            timeout: doctor::TIMEOUT,
        },
    );
    if checked.verdict == Verdict::Pass {
        return Ok(());
    }
    if checked.exit.is_none() {
        return Err(Stop::could_not_tell(format!(
            "`{}`'s runtime check did not answer, so `{}` is not opened — {}",
            pinned.pack.name, resolved.name, checked.line
        )));
    }
    let said = checked.stdout.trim();
    Err(Stop::refused(format!(
        "`{}`'s runtime check is red, so `{}` is not opened — {}",
        pinned.pack.name,
        resolved.name,
        if said.is_empty() {
            String::from("the check said nothing and exited non-zero")
        } else {
            said.replace('\n', "\n  ")
        }
    )))
}

/// The settings the carrier's workflow reads: `[packs]` judged whole against
/// every installed pack, then the carrier's own resolved table.
///
/// READ OFF THE BYTES THAT ARE PINNED and not off the project's parsed table,
/// so the policy snapshot in the run directory and the settings beside the
/// inputs are one reading of one file. A file that does not parse is
/// could-not-tell rather than no settings: reading it as empty would pin the
/// defaults over values the person wrote.
fn read_the_settings(
    policy: &[u8],
    resolved: &Resolved,
    wiring: &Wiring,
) -> Result<toml::Table, Stop> {
    let table = std::str::from_utf8(policy)
        .map_err(|e| e.to_string())
        .and_then(|text| text.parse::<toml::Table>().map_err(|e| e.to_string()))
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "the policy in force at {} does not parse, so the settings its [{}] table sets \
                 cannot be read: {e}",
                wiring.policy_file.display(),
                settings::TABLE
            ))
        })?;
    let mut every = settings::resolve(&table, &wiring.packs.layers).map_err(|found| {
        Stop::refused(format!(
            "`{}` is not opened — {}",
            resolved.name,
            found
                .iter()
                .map(|refusal| refusal.to_string())
                .collect::<Vec<_>>()
                .join("\n  ")
        ))
    })?;
    Ok(every.remove(&resolved.pack.name).unwrap_or_default())
}

/// The inputs as the file the run is pinned against, with the carrier's
/// settings beside them.
///
/// A key given twice is a refusal and not a last-one-wins: a run is a function
/// of its pinned inputs, and a caller that named one key two ways has not said
/// which run it wants.
fn render_inputs(order: &Order, resolved: &Resolved, config: toml::Table) -> Result<String, Stop> {
    let mut table = toml::map::Map::new();
    for (key, value) in order.inputs {
        if table.contains_key(key) {
            return Err(Stop::usage(format!(
                "`--input {key}=` is given twice — a run pins one value per key"
            )));
        }
        table.insert(key.clone(), toml::Value::String(value.clone()));
    }
    let mut snapshot = toml::map::Map::new();
    snapshot.insert(
        String::from("workflow"),
        toml::Value::String(resolved.name.clone()),
    );
    snapshot.insert(
        String::from("entry"),
        toml::Value::String(resolved.relative.clone()),
    );
    snapshot.insert(
        String::from("pack"),
        toml::Value::String(resolved.pack.name.clone()),
    );
    snapshot.insert(
        String::from("by"),
        toml::Value::String(order.by.to_string()),
    );
    snapshot.insert(
        String::from("started_at"),
        toml::Value::String(order.at.to_string()),
    );
    snapshot.insert(String::from("inputs"), toml::Value::Table(table));
    snapshot.insert(String::from(CONFIG), toml::Value::Table(config));
    toml::to_string(&toml::Value::Table(snapshot))
        .map_err(|e| Stop::could_not_tell(format!("the inputs do not render as TOML: {e}")))
}

// ---- (b) the record ----------------------------------------------------------

fn file_the_record(order: &Order, resolved: &Resolved, wiring: &Wiring) -> Result<String, Stop> {
    let description = format!(
        "A run of `{}` from the pack `{}` on {}.",
        resolved.name, resolved.pack.name, wiring.project.name
    );
    let id = wiring
        .store
        .create(
            &NewItem {
                title: UNTITLED,
                description: &description,
                item_type: RECORD_TYPE,
                labels: &[LABEL],
            },
            &order.by.to_string(),
        )
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "the run's record was not filed: {e}\n  NOTHING was written"
            ))
        })?;
    wiring
        .store
        .set_title(&id, &id, &order.by.to_string())
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{id} is filed and the title did not land: {e}\n  the record STANDS"
            ))
        })?;
    Ok(id)
}

/// The hash and the run's own object onto the record, read back.
///
/// The object is written WHOLE, as a flight's is: bd's metadata write merges at
/// the top level and replaces one key's object, so a write of one key alone
/// would drop the rest of this one.
fn write_the_pins(
    id: &str,
    hash: &str,
    resolved: &Resolved,
    order: &Order,
    wiring: &Wiring,
) -> Result<(), Stop> {
    let mut object = serde_json::Map::new();
    object.insert("hash".into(), hash.into());
    object.insert("workflow".into(), resolved.name.clone().into());
    object.insert("pack".into(), resolved.pack.name.clone().into());
    object.insert("entry".into(), resolved.relative.clone().into());
    object.insert("started_at".into(), order.at.into());
    let payload = keys::stamped(keys::RUN, object).to_string();
    wiring
        .store
        .set_metadata(id, &payload, &order.by.to_string())
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "the pins did not land on {id}: {e}\n  the run directory is WRITTEN and the \
                 record does not name it\n  RERUN: bd update {id} --metadata '{payload}' --actor \
                 {}",
                order.by
            ))
        })?;

    let read_back = read(wiring.store, id)?;
    let object = object_of(&read_back)?;
    let field = |key: &str| {
        object
            .and_then(|object| object.get(key))
            .and_then(|value| value.as_str())
            .unwrap_or("(absent)")
            .to_string()
    };
    if field("hash") != hash {
        return Err(disagrees(id, "hash", hash, &field("hash")));
    }
    if field("workflow") != resolved.name {
        return Err(disagrees(
            id,
            "workflow",
            &resolved.name,
            &field("workflow"),
        ));
    }
    let control = control_token();
    if read_back.document.contains(control) {
        return Err(Stop::could_not_tell(format!(
            "the read-back on {id} carries {control}, which nothing wrote — the read is not \
             reading this item"
        )));
    }
    Ok(())
}

/// What the pins, the bundle and the hash are made of, for the one run whose
/// record is already filed.
struct Pinning<'a> {
    id: &'a str,
    directory: &'a Path,
    inputs: String,
    policy_bytes: Vec<u8>,
    resolved: &'a Resolved,
    pinned: &'a Pinned,
    path: &'a str,
}

/// The directory, the two pins, the bundle the pack writes, the hash over all
/// three, and the hash on the record — answered as the hash.
fn pin_and_bundle(pinning: Pinning, order: &Order, wiring: &Wiring) -> Result<String, Stop> {
    let directory = pinning.directory;
    std::fs::create_dir_all(directory).map_err(|e| {
        Stop::could_not_tell(format!(
            "the run directory {} could not be made: {e}",
            directory.display()
        ))
    })?;
    pins::write_files(
        directory,
        &[
            (INPUTS.to_string(), pinning.inputs.into_bytes()),
            (POLICY.to_string(), pinning.policy_bytes),
        ],
    )?;
    bundle(
        directory,
        pinning.resolved,
        pinning.pinned,
        order,
        pinning.path,
    )?;
    let hash = pins::hash_of(directory, &hashed_files())?;
    write_the_pins(pinning.id, &hash, pinning.resolved, order, wiring)?;
    Ok(hash)
}

/// A run whose record was filed and which never started, closed as failed with
/// the refusal as its reason — and the refusal handed back naming the record
/// either way.
///
/// THE RECORD IS WHAT THE CAP COUNTS. A refusal between the record and
/// `run.started` — a bundle command that exits non-zero is the one a person
/// meets — would otherwise leave an open record no stream line names: `status`
/// does not list it, the controller neither re-runs nor cleans it, and it holds
/// one of `[core.run] max_open`'s slots for good.
///
/// THE CLOSE, THEN THE EVENT, as the back half's polarity has it: `run.failed`
/// over a record still open would announce an end the store does not hold. The
/// refusal keeps its own exit, because what went wrong is still what it says.
fn never_started(id: &str, stop: Stop, by: &Actor, wiring: &Wiring) -> Stop {
    let fate = match wiring
        .store
        .close(id, "the run failed before it started", &by.to_string())
    {
        Err(e) => format!(
            "the record {id} filed for this run STANDS open, and its close did not land: {e} — \
             `fleet cancel {id}` closes it"
        ),
        Ok(()) => match wiring.events.append(
            RUN_FAILED,
            by,
            serde_json::json!({ "run": id, "reason": stop.message }),
        ) {
            Ok(()) => format!("the record {id} filed for this run is closed as failed"),
            Err(e) => format!(
                "the record {id} filed for this run is closed as failed, and {RUN_FAILED} did \
                 not reach the stream: {e}"
            ),
        },
    };
    Stop {
        code: stop.code,
        message: format!("{}\n  {fate}", stop.message),
    }
}

// ---- the two children ---------------------------------------------------------

/// The five names a pack's templates are written against.
///
/// ONE BUILDER FOR BOTH LINES. The bundle command and the run command take the
/// same five, so a set built once is what makes that a fact rather than two
/// lists that happen to agree today.
struct Placeholders {
    entry: String,
    bundle: String,
    run_dir: String,
    fleet: String,
    inputs: String,
}

impl Placeholders {
    fn of(directory: &Path, resolved: &Resolved, order: &Order) -> Placeholders {
        Placeholders {
            entry: resolved.entry.display().to_string(),
            bundle: directory.join(BUNDLE).display().to_string(),
            run_dir: directory.display().to_string(),
            fleet: order.fleet_bin.display().to_string(),
            inputs: directory.join(INPUTS).display().to_string(),
        }
    }

    fn pairs(&self) -> [(&str, &str); 5] {
        [
            ("entry", self.entry.as_str()),
            ("bundle", self.bundle.as_str()),
            ("run_dir", self.run_dir.as_str()),
            ("fleet", self.fleet.as_str()),
            ("inputs", self.inputs.as_str()),
        ]
    }
}

// ---- the bundle --------------------------------------------------------------

/// The pack's bundle line, substituted and run as a child in the run directory.
///
/// THE LINE IS A SHELL LINE. A pack declares `deno bundle {entry} --output
/// {bundle}`, which is a command line and not an argv — core has no way to
/// split one into words that is right for every language's tooling — so it is
/// handed to `sh` whole, and the child's exit is all core reads of it.
///
/// It runs on [`child_path_for`], as the run line does: both lines exec the
/// binary the pack pins.
fn bundle(
    directory: &Path,
    resolved: &Resolved,
    pinned: &Pinned,
    order: &Order,
    path: &str,
) -> Result<(), Stop> {
    let written = directory.join(BUNDLE);
    let places = Placeholders::of(directory, resolved, order);
    let line = pack::substitute(&pinned.runtime.bundle, &places.pairs());
    let answered = Command::new("sh")
        .arg("-c")
        .arg(&line)
        .current_dir(directory)
        .env("PATH", path)
        .output()
        .map_err(|e| Stop::could_not_tell(format!("`{line}` did not run: {e}")))?;
    if !answered.status.success() {
        return Err(Stop::refused(format!(
            "the bundle command of `{}` exited {} — `{line}`\n  {}",
            pinned.pack.name,
            answered
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| String::from("on a signal")),
            String::from_utf8_lossy(&answered.stderr)
                .trim()
                .replace('\n', "\n  ")
        )));
    }
    if !written.is_file() {
        return Err(Stop::refused(format!(
            "the bundle command of `{}` exited 0 and wrote no {BUNDLE} — `{line}`\n  the program \
             a run is pinned to is the bundle, so there is nothing to hash",
            pinned.pack.name
        )));
    }
    Ok(())
}

// ---- (d) the run ---------------------------------------------------------------

/// The pack's run line, substituted and executed, and the one event its exit
/// earns.
///
/// THE LINE IS A SHELL LINE, as the bundle's is, and for the same reason.
fn execute(
    out: &mut dyn Write,
    started: &Started,
    resolved: &Resolved,
    pinned: &Pinned,
    order: &Order,
    path: &str,
    wiring: &Wiring,
) -> Result<Ended, Stop> {
    let directory = started.directory.as_path();
    let places = Placeholders::of(directory, resolved, order);
    let line = pack::substitute(&pinned.runtime.run, &places.pairs());
    let document = the_inputs_as_json(directory)?;
    let stdout_log = directory.join(STDOUT_LOG);

    let at_start = wiring.stream.seq();
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(&line)
        .current_dir(directory)
        .env_clear()
        .env("PATH", path)
        .env(ENV_DIR, order.machine_dir)
        .env(ENV_RUN_ID, &started.run)
        .env(ENV_STREAM, wiring.stream.path())
        .env(ENV_STREAM_SEQ, at_start.to_string())
        .env(ENV_RUN_DIR, directory)
        .env(ENV_BIN, order.fleet_bin)
        .env(ENV_PROJECT, &wiring.project.root);
    for pass in ENV_PASSED_THROUGH {
        if let Some(value) = std::env::var_os(pass) {
            command.env(pass, value);
        }
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::from(log(&stdout_log)?))
        .stderr(Stdio::from(log(&directory.join(STDERR_LOG))?))
        .spawn()
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "`{line}` did not start: {e}\n  {} is open and how it ended is not announced",
                started.run
            ))
        })?;

    // A workflow that never reads its inputs closes the pipe, and that is a
    // choice it is allowed to make: the document is offered, not imposed.
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(document.as_bytes()) {
            if e.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(Stop::could_not_tell(format!(
                    "the pinned inputs did not reach `{line}` on stdin: {e}"
                )));
            }
        }
    }
    let status = child.wait().map_err(|e| {
        Stop::could_not_tell(format!(
            "`{line}` was started and its exit could not be read: {e}"
        ))
    })?;

    let said = last_line(&stdout_log)?;
    let (ended, payload) = read_the_exit(&started.run, &status, said.as_deref(), wiring);

    if ended.retires_the_record() {
        wiring
            .store
            .close(
                &started.run,
                &format!("the run {}", ended.word()),
                &order.by.to_string(),
            )
            .map_err(|e| {
                Stop::could_not_tell(format!(
                    "{} {} and the record did not close: {e}\n  the run directory is WRITTEN, \
                     nothing is announced, and the run still counts against `[core.run] max_open`",
                    started.run,
                    ended.word()
                ))
            })?;
    }
    wiring
        .events
        .append(ended.kind(), order.by, payload)
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{} {} and {} did not reach the stream: {e}\n  the record STANDS",
                started.run,
                ended.word(),
                ended.kind()
            ))
        })?;

    writeln!(out, "{} — {}", started.run, ended.word())
        .map_err(|e| Stop::could_not_tell(format!("the outcome line could not be written: {e}")))?;
    Ok(ended)
}

/// The exit table, as one match: the row, and the payload that row carries.
///
/// THE LAST LINE IS READ ONLY WHERE THE ROW USES IT. A 0 is closed whatever the
/// workflow printed, and a 1 or a 2 whose last line is not JSON falls to
/// could-not-tell rather than to a reason nobody wrote — a failure with an
/// empty reason reads like a failure somebody explained.
fn read_the_exit(
    id: &str,
    status: &ExitStatus,
    said: Option<&str>,
    wiring: &Wiring,
) -> (Ended, serde_json::Value) {
    let json = said.and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok());
    match (status.code(), json) {
        (Some(0), _) => (Ended::Closed, serde_json::json!({ "run": id })),
        (Some(1), Some(reason)) => (
            Ended::Failed,
            serde_json::json!({ "run": id, "reason": reason }),
        ),
        (Some(2), Some(wake)) => (
            Ended::Waiting,
            serde_json::json!({ "run": id, "wake": wake, "seq": wiring.stream.seq() }),
        ),
        (code, _) => (
            Ended::CouldNotTell,
            serde_json::json!({ "run": id, "exit": code, "read": said }),
        ),
    }
}

/// One of the two logs, opened for the child to write into.
fn log(path: &Path) -> Result<std::fs::File, Stop> {
    std::fs::File::create(path)
        .map_err(|e| Stop::could_not_tell(format!("{} could not be opened: {e}", path.display())))
}

/// The pinned inputs as the JSON document the workflow is handed.
///
/// READ BACK OFF THE PINNED FILE rather than re-rendered from the order, so
/// what the workflow is given and what the hash is over are the same inputs.
fn the_inputs_as_json(directory: &Path) -> Result<String, Stop> {
    let path = directory.join(INPUTS);
    let table = read_table(&path).map_err(Stop::could_not_tell)?;
    serde_json::to_string(&table).map_err(|e| {
        Stop::could_not_tell(format!("{} does not render as JSON: {e}", path.display()))
    })
}

/// The last line of the log that carries anything.
///
/// A trailing newline makes the literal last line empty, and a workflow that
/// ends on one has still said something — so the last line with content is the
/// reading, and a log with no content at all reads as nothing said.
fn last_line(path: &Path) -> Result<Option<String>, Stop> {
    let body = std::fs::read(path).map_err(|e| {
        Stop::could_not_tell(format!(
            "{} could not be read back: {e}\n  the run ran and its exit cannot be classified",
            path.display()
        ))
    })?;
    Ok(String::from_utf8_lossy(&body)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string()))
}

// ---- the readings' shared refusals -------------------------------------------

fn read(store: &dyn Store, id: &str) -> Result<Item, Stop> {
    store
        .show(id)
        .map_err(|e: StoreError| Stop::could_not_tell(format!("{id} could not be read: {e}")))
}

/// The run's own object off its record, at the one version this binary reads,
/// or `None` where the record carries no `fleet.run` at all.
///
/// A key at another version, or at none, is could-not-tell naming the key and
/// the version: a run a newer fleet opened is not executed again, or read
/// back, as though this binary knew its shape.
fn object_of(record: &Item) -> Result<Option<&serde_json::Map<String, serde_json::Value>>, Stop> {
    let Some(held) = record.run.as_ref() else {
        return Ok(None);
    };
    keys::versioned(keys::RUN, held).map(Some).map_err(|why| {
        Stop::could_not_tell(format!(
            "{}'s {why} — this fleet will not guess at a run's shape it does not know",
            record.id
        ))
    })
}

fn disagrees(id: &str, key: &str, wrote: &str, read_back: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{id}'s {key} reads back `{read_back}` and this run wrote `{wrote}` — the record does not \
         hold what it was told"
    ))
}
