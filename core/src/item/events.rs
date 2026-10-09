//! The kinds the item verbs and a run write on the event stream, the payload
//! keys each carries, and the seam they are written through.

use crate::seat::actor::Actor;

/// The one a run's front half writes: every input pinned, the directory
/// hashed, and the record standing.
pub const RUN_STARTED: &str = "run.started";

/// The four the back half writes, one per row of the exit table the workflow
/// answers on.
///
/// EXACTLY ONE OF THEM PER RUN, and a run that has one is a run whose process
/// is gone: a workflow is short-lived by design, so the stream and not the
/// process table is where a reader learns how one ended. [`RUN_CLOSED`] and
/// [`RUN_FAILED`] are the two whose record is closed with them; [`RUN_WAITING`]
/// leaves the record open at the sequence it carries, and
/// [`RUN_COULD_NOT_TELL`] leaves it open with what was read, because a run
/// nothing could classify is not one to retire on a guess.
pub const RUN_CLOSED: &str = "run.closed";
pub const RUN_FAILED: &str = "run.failed";
pub const RUN_WAITING: &str = "run.waiting";
pub const RUN_COULD_NOT_TELL: &str = "run.could_not_tell";

/// The one a PERSON's `fleet cancel` writes: the run ended by hand, its record
/// closed with it.
///
/// NOT A ROW OF THE EXIT TABLE. No process answered it — a cancel stops none —
/// so it ends the run whatever an execution under way writes after it, and a
/// fold that meets it reads the run as ended for good.
pub const RUN_CANCELLED: &str = "run.cancelled";

/// The one the CONTROLLER writes when a run's seats are let go.
///
/// Named here beside the five above because it is the same lifecycle's
/// vocabulary and a kind spelled twice is two kinds — the controller crate
/// takes nothing from this one but the bounded runner (`crate::process`), so it
/// carries its own spelling and a test in the binary holds the two to one
/// string.
///
/// IT IS NOT AN ENDING. A run ends on one of the four rows of the exit table;
/// this says the seats that run spawned are gone, which is a fact about the
/// machine and not about the workflow.
pub const RUN_CLEANED: &str = "run.cleaned";

/// The one line every entry on an item's timeline is signalled on: the item,
/// the id the store gave the entry, and the entry's kind, and nothing else
/// [ASSUMES D2].
///
/// A SIGNAL AND NOT A COPY. What the entry says is the record's, read off the
/// timeline; a payload that carried the commit or the verdict would be a second
/// record a reader could believe over the first. The line says only that the
/// entry was written, which is what a waiting run is woken by.
///
/// Every verb writes one per entry, after the entry is read back, through
/// [`signal`]. The controller writes it too, for the `held` entry its crash
/// cap's park writes, and spells it again on its own side of the seam.
pub const ITEM_ENTRY: &str = "item.entry";

/// The one a landing's suite reading writes: a fact about the machine that
/// ran the suite, and not an entry's signal [ASSUMES D3].
pub const CHECK_READ: &str = "check.read";

/// One entry's signal, by the entry's own author: `entry` is the id
/// [`recorded`](super::recorded) answered, and `kind` the entry's kind as
/// [`crate::entry::KINDS`] spells it.
///
/// The stream's refusal is answered as prose, and each verb words it as its
/// own STANDS line: what stands is the entry, and the verb is what knows it.
pub fn signal(
    events: &dyn Events,
    by: &Actor,
    item: &str,
    entry: &str,
    kind: &str,
) -> Result<(), String> {
    events.append(
        ITEM_ENTRY,
        by,
        serde_json::json!({ "item": item, "entry": entry, "kind": kind }),
    )
}

/// The payload keys the entry signal and the suite reading carry — and
/// [`RUN_STARTED`], which is neither and is here for the same reason — or
/// `None` for every other kind.
///
/// The table is HERE and not in each verb, so the writer and the fold cannot
/// disagree about what a kind carries: every writer asserts its payload against
/// this and the fold reads the same names. `item` is first on both item rows —
/// it is the key a reader ties a line to a record by.
pub fn payload_keys(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        ITEM_ENTRY => &["item", "entry", "kind"],
        // `log` is where the reading it carries can be read back. It is on the
        // kind and not only on the rerun's: a pair of readings a person is
        // asked to judge names two files, and one of them is the first.
        // `path` is the search path the reading's child ran under, so a suite
        // that failed on the diff and one that failed because it could not find
        // its tools are told apart from the stream alone.
        CHECK_READ => &["item", "suite", "rc", "verdict", "reading", "log", "path"],
        // `run` first, as `item` is first on both rows above: it is the key a
        // reader of the stream ties a hash and a workflow name to.
        RUN_STARTED => &["run", "hash", "workflow"],
        // The close carries the id alone: everything else about the run was
        // said on `run.started` and stands on the record.
        RUN_CLOSED => &["run"],
        // `reason` and `wake` are the workflow's OWN last line, read as JSON
        // and carried whole — core parses no further, so what a fold reads
        // under them is whatever shape the workflow's language writes.
        RUN_FAILED => &["run", "reason"],
        // `seq` is the stream's position when the process exited, which is
        // what a re-run measures the stream against.
        RUN_WAITING => &["run", "wake", "seq"],
        // `exit` is the code, or null where the process died on a signal, and
        // `read` is the last line as it stood — the two readings that say why
        // no other row fitted.
        RUN_COULD_NOT_TELL => &["run", "exit", "read"],
        // The id alone, as the close's: the holds the cancel cleared each
        // have a `cleared` entry and its signal of their own, and a list here
        // would be a second copy of them.
        RUN_CANCELLED => &["run"],
        // `count` is how many seats were retired, and it is the whole payload
        // beside the id: which seats they were is on each one's own
        // `session.retired`, and a list here would be a second copy of it.
        RUN_CLEANED => &["run", "count"],
        _ => return None,
    })
}

/// The two a workflow step writes on the run lifecycle. NOTHING IN CORE WRITES
/// EITHER: the SDK's steps do, through the stream, and the vocabulary is stated
/// here so the fold reads one name and the writer adds an act rather than a
/// name.
pub const STEP_STARTED: &str = "step.started";
pub const STEP_CLOSED: &str = "step.closed";

/// Where a typed event goes. Core never opens the stream file: the controller
/// wires this to its own writer, the same way `dispatch` reaches its spawn.
///
/// The actor is handed over typed, and the writer stores it as the stream's
/// `{kind, id}` object.
pub trait Events {
    fn append(
        &self,
        kind: &str,
        actor: &crate::seat::actor::Actor,
        payload: serde_json::Value,
    ) -> Result<(), String>;
}
