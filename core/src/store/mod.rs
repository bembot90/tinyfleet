//! The work graph, reached only through [`Store`], whose implementations are
//! adapters: each speaks one store's own tongue behind the trait, and nothing
//! outside an adapter's own module knows which store is answering.
//!
//! The trait is what the verbs are written against, so a suite can force a
//! read-back that disagrees with the write beside it — the one failure a real
//! store will not produce on demand and the one the verbs must survive.
//!
//! What an adapter keeps beside the graph it declares through
//! [`Store::capabilities`]: the export a landing commits and the directory it
//! sits in are read from there, and never spelled by a verb.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use crate::entry::{Body, Entry};
use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;
use types::Capabilities;

pub mod bd;
pub mod conformance;
pub mod exec;
pub mod types;

pub use types::{
    Filter, HoldId, Item, ItemId, ItemSummary, NewItem, Order, OrderKind, OrderState, ReadProof,
    RunRecord, Stamp, Status, Update, Version,
};

/// The ways a store call ends badly, which are different exits: an act the
/// record refuses, or an item not held by whom the write required, is the
/// record's answer, and a store that will not answer is no reading at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store answered, and its answer is that the act cannot be done as
    /// asked: no such item, text naming more than one, or an act already done
    /// — a close of a closed item is one.
    Refused(String),
    /// The store answered, and its answer is that the item is held by someone
    /// other than the holder a fenced write named — so NOTHING was written.
    Moved(String),
    /// The store could not be run, or did not answer something readable.
    Unreadable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Refused(why) => write!(f, "{why}"),
            StoreError::Moved(why) => write!(f, "{why}"),
            StoreError::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

/// THE STORE CONTRACT IN-PROCESS, VERB FOR VERB: each verb `docs/store.md`
/// names is one method here, taking the verb's request fields as its
/// arguments and answering its response, and an adapter out of process
/// answers the same verbs as JSON.
///
/// | Verb | Method |
/// | --- | --- |
/// | `version` | [`version`](Store::version) |
/// | `capabilities` | [`capabilities`](Store::capabilities) |
/// | `resolve` | [`resolve`](Store::resolve) |
/// | `show` | [`show`](Store::show) |
/// | `list` | [`list`](Store::list) |
/// | `timeline` | [`timeline`](Store::timeline) |
/// | `create` | [`create`](Store::create) |
/// | `update` | [`update`](Store::update) |
/// | `append` | [`append`](Store::append) |
/// | `order.set` | [`order_set`](Store::order_set) |
/// | `order.withdraw` | [`order_withdraw`](Store::order_withdraw) |
/// | `run.set` | [`run_set`](Store::run_set) |
/// | `hold.raise` | [`hold_raise`](Store::hold_raise) |
/// | `hold.clear` | [`hold_clear`](Store::hold_clear) |
/// | `holds.open` | [`holds_open`](Store::holds_open) |
/// | `close` | [`close`](Store::close) |
/// | `export` | [`export`](Store::export) |
/// | `scratch` | [`scratch`](Store::scratch) |
///
/// BESIDE THE TABLE are the fenced writes, which the contract does not name
/// yet and whose defaults are written in its verbs: [`hand_over`], an update
/// that lands only while the holder it names still holds the item;
/// [`reopen`]; and [`order_withdraw_from`], the withdrawal a retire makes,
/// fenced on the retiring seat and the status it listed. A store with a fence
/// of its own takes each in one call.
///
/// No storage shape crosses this trait: an order and a run's record go in as
/// the contract's own types, and where a store keeps them is its adapter's.
///
/// [`hand_over`]: Store::hand_over
/// [`reopen`]: Store::reopen
/// [`order_withdraw_from`]: Store::order_withdraw_from
pub trait Store {
    /// One item, which the argument may name by PART of its id: the store
    /// resolves a partial id itself, and the answer's `id` is the full one. So
    /// a verb taking an item resolves it here once, at its entry, and acts on
    /// [`Item::id`] from then on and never on the typed text.
    fn show(&self, item: &str) -> Result<Item, StoreError>;

    /// The full id of the item the argument names, by the rule [`show`]
    /// resolves one with, and nothing else of it: an id no item carries is
    /// Refused, and an argument naming more than one item is too.
    ///
    /// [`show`]: Store::show
    fn resolve(&self, id: &str) -> Result<ItemId, StoreError>;

    /// The items a filter matches, in the store's own order, each as the row
    /// the listing answered: ONE call, whatever the rows number. The store's
    /// ready set is open and unblocked; a label's is the open items carrying
    /// it; a seat's is the items held against its full id, which a caller
    /// reads the status of off each row.
    ///
    /// A LISTING IS NEVER CAPPED. A store whose listing answers its first rows
    /// by default is asked for all of them, because a truncated list reads
    /// exactly like a whole one.
    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError>;

    /// One item filed, answered as the id the store gave it.
    ///
    /// A run's record is titled by its own id, which nothing knows until
    /// this returns — so the title in [`NewItem`] is what the record carries
    /// until the caller retitles it through [`update`](Store::update), and the
    /// caller's read-back is what says the second write landed.
    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError>;

    /// The item's title, its assignee or both moved in ONE write. A field the
    /// change leaves out is left alone, and an assignee of `None` is the item
    /// handed to nobody.
    ///
    /// A change naming neither is Unreadable, and nothing is written: it is a
    /// caller's mistake, and never a write that quietly did nothing.
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError>;

    /// The item handed from `from` to `to`, and only while `from` still holds
    /// it — `""` for an item nobody holds. Anyone else holding it is
    /// [`StoreError::Moved`], and nothing is written.
    ///
    /// For a write whose actor is NOT the holder. The DEFAULT reads the holder
    /// and then updates the assignee, for a store with no fence of its own.
    ///
    /// The seat and the actor are text here, and an update takes both typed:
    /// a `to` that is no seat id, or a `by` that is no typed actor, is
    /// Unreadable with nothing written, and never a reading guessed at.
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        let change = match to {
            "" => Update::unassigned(),
            seat => Update::assignee(SeatId::parse(seat).map_err(|why| {
                StoreError::Unreadable(format!(
                    "{item} is not handed over: {why} — nothing was written"
                ))
            })?),
        };
        let actor = match Actor::typed(by) {
            Some(Ok(actor)) => actor,
            Some(Err(why)) => {
                return Err(StoreError::Unreadable(format!(
                    "{why} — nothing was written"
                )))
            }
            None => {
                return Err(StoreError::Unreadable(format!(
                    "`{by}` is not a typed actor, and a hand-over of {item} is written under one \
                     — nothing was written"
                )))
            }
        };
        let held = held_text(self.show(item)?.assignee);
        if held != from {
            return Err(moved(item, from, &held));
        }
        self.update(&ItemId::from(item), &change, &actor)
    }

    /// The item's order replaced whole. The assignee and the run's record are
    /// left as they were: a store keeping the two beside each other writes
    /// the order without touching the record.
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError>;

    /// The assignee cleared and the order taken away in ONE act. After any
    /// answer, an item never reads with its assignee cleared while its order
    /// stands.
    ///
    /// NO DEFAULT BODY: two writes in a row are exactly the half-withdrawal
    /// the one act exists to rule out, so every store says how it takes both
    /// at once.
    fn order_withdraw(&self, id: &ItemId, by: &Actor) -> Result<(), StoreError>;

    /// The run's record on the item that records it, replaced whole. The
    /// order beside it is left as it was.
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError>;

    /// The item's status set back to `open`, which is all this writes.
    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError>;

    /// The item reopened and its order withdrawn — the assignee cleared and
    /// the order taken away — in ONE call, and only while `seat` still holds
    /// the item under `status`: [`hand_over`](Store::hand_over)'s fence,
    /// because a retire's actor is never the seat it retires, and the status
    /// beside it, because an item closed since the caller read it is never
    /// reopened.
    ///
    /// `status` is the one the caller read the item under, and a withdrawal
    /// reads only `open` or `in_progress`. The reopen is the point of the
    /// second: an item left `in_progress` with nobody holding it is out of the
    /// ready set, and no dispatch reaches it until somebody reopens it by hand.
    ///
    /// A retire pays this on every seat it ends — so a store that takes all of
    /// it in one call is sent one, which is a call the verb does not make while
    /// another suite is queueing behind it. The DEFAULT is one read of the
    /// fence, the reopen, then [`order_withdraw`](Store::order_withdraw): a
    /// default cut short after the reopen leaves an item still held and
    /// ordered, which a second retire lists and finishes.
    fn order_withdraw_from(
        &self,
        id: &ItemId,
        seat: &SeatId,
        status: &Status,
        by: &Actor,
    ) -> Result<(), StoreError> {
        let read = self.show(id)?;
        if read.assignee != Some(*seat) {
            return Err(moved(id, &seat.to_string(), &held_text(read.assignee)));
        }
        if read.status != *status {
            return Err(restatused(id, status.as_str(), read.status.as_str()));
        }
        self.reopen(id, &by.to_string())?;
        self.order_withdraw(id, by)
    }

    /// A hold raised on this item, answered as the hold's own id.
    ///
    /// The store's own object and not a question item this fleet owns: the held
    /// item leaves the ready set the moment the hold is raised, and it comes back
    /// when somebody clears the hold. So a park needs nothing of fleet's beside
    /// the event.
    fn hold_raise(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<HoldId, StoreError>;

    /// One hold cleared, which puts the item it blocked back in the ready set.
    ///
    /// A HOLD ALREADY CLEARED IS REFUSED, naming the hold: the act is already
    /// done, which is the record's answer: `<hold> is already cleared`.
    fn hold_clear(&self, hold: &HoldId, by: &Actor) -> Result<(), StoreError>;

    /// Every hold the store still calls open, by id.
    ///
    /// IDS AND NOT DOCUMENTS, and no item on them: a listing need not say which
    /// item a hold blocks, so a caller that wants one item's hold reads that
    /// hold's id off the item's own park and asks this list whether it is still
    /// here.
    fn holds_open(&self) -> Result<Vec<HoldId>, StoreError>;

    /// The item closed, with the reason a reader gets instead of the act.
    ///
    /// AN ITEM ALREADY CLOSED IS REFUSED, and its first close's reason
    /// stands: the act is already done, which is the record's answer.
    ///
    /// `by` IS TEXT AND NOT AN [`Actor`], the one write here that keeps it: a
    /// landing closes under the holder's own assignee string, because a store
    /// may close an assigned item only for an actor equal to its assignee —
    /// the adapter's `close` says where that was measured. Whether that
    /// string becomes a typed actor is fleet-0q4's to rule, and the order
    /// writes did not rule it.
    fn close(&self, id: &ItemId, reason: &str, by: &str) -> Result<(), StoreError>;

    /// One entry appended to the item's timeline, answered as the entry's id.
    ///
    /// The body is VALIDATED FIRST, and one that breaks its kind's rules is
    /// Unreadable with nothing written: a store never keeps an entry its own
    /// reader would refuse.
    fn append(&self, item: &ItemId, body: &Body, by: &Actor) -> Result<String, StoreError>;

    /// The item's entries, in the store's order. A comment that does not carry
    /// [`crate::entry::KEY`] is a person's and is left out; one that carries it and
    /// does not read — or whose author is not a typed actor — is Unreadable
    /// and refuses the whole read, naming the comment, because a timeline with
    /// a hole in it answers every question wrong. An item the store does not
    /// hold is Refused.
    fn timeline(&self, item: &ItemId) -> Result<Vec<Entry>, StoreError>;

    /// What the store keeps beside the graph, answered without a call to the
    /// store: the export a landing commits and the directory it sits in —
    /// `None` for a store that keeps none, which a landing lands without —
    /// whether it is a scratch board, and the prefix its ids carry.
    fn capabilities(&self) -> Result<Capabilities, StoreError>;

    /// Which store answered, and at which version of itself.
    fn version(&self) -> Result<Version, StoreError>;

    /// The store's own export, written under the root the CALLER names at the
    /// file [`capabilities`](Store::capabilities) declares, and answered as
    /// the absolute path it wrote. A store that declares no export refuses it.
    ///
    /// THE DESTINATION IS THE CALLER'S AND NOT THE STORE'S. One board is read
    /// from the checkout it was resolved in and committed from whichever tree
    /// the act runs in, and a landing resolves that tree for itself — so where
    /// the export has to land is a fact of the act and not of the store.
    ///
    /// It regenerates that one file and touches nothing else, so nothing is
    /// carried forward here to keep the export's polarity right.
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError>;

    /// A new, empty store of this adapter's own kind made in the directory
    /// `into`, answered as the root a store over it is addressed at: what
    /// [`conformance`]'s checks are run against, since a check writes, and
    /// never to a project's own store.
    ///
    /// A store DECLARES it, as `scratch` in [`capabilities`], and the DEFAULT
    /// is the answer of a store that does not: Unreadable, with nothing made.
    ///
    /// [`capabilities`]: Store::capabilities
    fn scratch(&self, into: &Path) -> Result<PathBuf, StoreError> {
        let _ = into;
        Err(StoreError::Unreadable(String::from(
            "this store declares no scratch",
        )))
    }
}

/// The bound on one store call, fixed and not policy.
///
/// No legitimate store call comes close: every one is a local read or write
/// of one project's store. A call that outruns it is a store that is not
/// answering, and it is killed and read as Unreadable rather than left to hang
/// the verb. A landing's suite is not a store call and is not bounded here.
pub const STORE_TIMEOUT: Duration = Duration::from_secs(60);

// ---- the opener ---------------------------------------------------------------

/// What [`open`] is handed: the project, its own file, where a binary is
/// looked for and how long one call may take. A caller builds one and every
/// store it opens comes out of this one function.
pub struct Opening<'a> {
    /// The project's root, which every call of the store opened is scoped to.
    pub root: &'a Path,
    /// The project's OWN file, which `[store] adapter` is read out of: a
    /// declared project's `.fleet/project.toml`, else an embedded fleet's
    /// `fleet.toml`. A standalone fleet's file is the fleet's, not the
    /// project's, and names no store.
    pub policy: &'a toml::Table,
    /// The search path the built-in store's binary is resolved on: the
    /// caller's constructed child `PATH`, and never this process's own, which
    /// under a service holds neither a package manager's prefix nor the user's
    /// local bin (lessons claude-code D1).
    pub search_path: &'a str,
    /// Whether a binary nothing resolves is could not tell. Not strict, it is
    /// the bare name, which the process's own `PATH` then answers or does not.
    pub strict: bool,
    /// The bound on each call of the store opened: [`STORE_TIMEOUT`], or less
    /// for a caller that cannot wait that long.
    pub timeout: Duration,
}

/// The project's store, as `[store] adapter` in its own file names it: the
/// built-in store's name, or no key at all, is that store; an absolute path to
/// an executable file is an adapter that answers the contract at that path.
/// Anything else is could not tell, naming what the file said, and nothing is
/// run.
///
/// ONE OPENER FOR EVERY CALLER — the verbs, the run pass and `fleet prime` —
/// so the store a verb writes to and the one the pass reads are one store.
pub fn open(at: &Opening) -> Result<Box<dyn Store>, StoreError> {
    let named = crate::policy::read("store", "adapter", at.policy)
        .map_err(|unlisted| StoreError::Unreadable(unlisted.to_string()))?;
    match named {
        None => bd::open(at),
        Some(toml::Value::String(name)) if name == bd::NAME => bd::open(at),
        Some(toml::Value::String(path)) if path.starts_with('/') => {
            let adapter = Path::new(path);
            if !executable_file(adapter) {
                return Err(StoreError::Unreadable(format!(
                    "[store] adapter names `{path}`, which is not an executable file"
                )));
            }
            Ok(Box::new(
                exec::Exec::at(adapter, at.root).with_timeout(at.timeout),
            ))
        }
        Some(toml::Value::String(other)) => Err(neither_form(other)),
        Some(other) => Err(neither_form(&other.to_string())),
    }
}

/// The adapter `[store] adapter` names, as a line names it to a person: the
/// built-in store's name where the key names nothing, and otherwise the file
/// name of what it names, which for a bare name is that name.
///
/// A NAME AND NOT A CHECK: read beside a store [`open`] opened, which has
/// already refused every value that is neither form.
pub fn adapter_name(policy: &toml::Table) -> String {
    match crate::policy::read("store", "adapter", policy) {
        Ok(Some(toml::Value::String(named))) => Path::new(named)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| named.clone()),
        _ => String::from(bd::NAME),
    }
}

/// The refusal a `[store] adapter` answers that is neither form.
fn neither_form(said: &str) -> StoreError {
    StoreError::Unreadable(format!(
        "[store] adapter is `{said}` — it is \"bd\" or an absolute path to an adapter executable"
    ))
}

/// The project's own file as a table, for a caller that has resolved no
/// project around the root: `<root>/.fleet/project.toml` where it is a file,
/// else `<root>/fleet.toml`, else an empty table — which opens the built-in
/// store. A file that will not read or parse is could not tell, naming it.
pub fn project_policy(root: &Path) -> Result<toml::Table, StoreError> {
    let Some(file) = [root.join(".fleet/project.toml"), root.join("fleet.toml")]
        .into_iter()
        .find(|file| file.is_file())
    else {
        return Ok(toml::Table::new());
    };
    let text = std::fs::read_to_string(&file).map_err(|e| {
        StoreError::Unreadable(format!("{} could not be read: {e}", file.display()))
    })?;
    text.parse::<toml::Table>().map_err(|e| {
        StoreError::Unreadable(format!("{} does not parse as TOML: {e}", file.display()))
    })
}

/// A file that is there and executable. Beside the opener and not inside an
/// adapter: the opener asks it of an adapter's path, and the built-in adapter
/// of a binary it resolves.
pub(crate) fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The refusal a fenced hand-over answers when the item's holder is not the
/// one the write named.
pub(crate) fn moved(item: &str, expected: &str, held: &str) -> StoreError {
    StoreError::Moved(format!(
        "{item} is held by {} and not by {} — nothing was written",
        holder_named(held),
        holder_named(expected)
    ))
}

/// The refusal a fenced withdrawal answers when the item's status is not the
/// one the write named.
pub(crate) fn restatused(item: &str, expected: &str, now: &str) -> StoreError {
    StoreError::Moved(format!(
        "{item} reads {now} and not {expected} — nothing was written"
    ))
}

/// The body an append is handed, held to its kind's rules before anything is
/// written — the one refusal both stores answer, word for word.
pub(crate) fn validated(item: &str, body: &Body) -> Result<(), StoreError> {
    body.validate().map_err(|why| {
        StoreError::Unreadable(format!(
            "the {} entry for {item} does not validate: {why} — nothing was written",
            body.kind()
        ))
    })
}

/// The refusal an update naming neither a title nor an assignee answers — the
/// one both stores answer, word for word, before anything is run.
pub(crate) fn unchanged() -> StoreError {
    StoreError::Unreadable(String::from(
        "an update names neither a title nor an assignee — nothing was written",
    ))
}

/// The refusal a close of an item already closed answers.
pub(crate) fn already_closed(id: &ItemId) -> StoreError {
    StoreError::Refused(format!("{id} is already closed"))
}

/// The refusal a clear of a hold already cleared answers.
pub(crate) fn already_cleared(hold: &HoldId) -> StoreError {
    StoreError::Refused(format!("{hold} is already cleared"))
}

/// The refusal a read answers for an item whose holder is no seat id: a
/// person holds it, and it is never read as held by nobody. `held` is the
/// holder as the store spells it.
pub(crate) fn not_a_seat(item: &str, held: &str) -> StoreError {
    StoreError::Unreadable(format!(
        "{item} is held by {held}, which is not a seat of this fleet — a person holds it, and \
         fleet reads only seat holders"
    ))
}

/// The holder a fence compares as text, where a fenced write names its holder
/// as text: the seat's id, or `""` for nobody.
fn held_text(held: Option<SeatId>) -> String {
    held.map(|seat| seat.to_string()).unwrap_or_default()
}

/// A holder as a refusal names one: the seat, or nobody for `""`.
fn holder_named(seat: &str) -> String {
    if seat.is_empty() {
        String::from("nobody")
    } else {
        format!("`{seat}`")
    }
}

/// What a call said last: the last line of its stderr that is not blank, else
/// of its stdout, cut to 160 characters.
///
/// Beside the trait and not inside an adapter: the built-in store and an
/// adapter executable both carry it into their refusals.
pub(crate) fn tail(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let body = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout)
    } else {
        stderr
    };
    body.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no output")
        .chars()
        .take(160)
        .collect()
}

/// The first JSON value of an answer, with whatever trails it discarded.
///
/// Beside the trait and not inside an adapter: the contract's own envelope
/// ([`types::answer`]) reads an answer through it as an adapter's reads do.
pub fn first_value(text: &str) -> Option<serde_json::Value> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>();
    stream.next().and_then(Result::ok)
}
