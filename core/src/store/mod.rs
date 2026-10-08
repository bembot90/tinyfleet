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
use std::time::Duration;

use crate::adapter::open::{resolve, Wanted};
use crate::entry::{Body, Entry};
use crate::pack::AdapterKind;
use crate::seat::actor::Actor;
use types::Capabilities;

pub mod conformance;
pub mod exec;
pub mod schema;
pub mod types;

pub use types::{
    Filter, HoldId, Item, ItemId, ItemSummary, NewItem, Order, OrderKind, OrderState, ReadProof,
    RunRecord, Stamp, Status, Update, Version, WithdrawFence,
};

/// The first JSON value of an answer, with whatever trails it discarded —
/// read as every adapter's answer is ([`crate::adapter::exec::first_value`]),
/// and reached here, beside the trait, by the store's own readers of a
/// request or a proof.
pub use crate::adapter::exec::first_value;

/// The ways a store call ends badly, which are different exits: an act the
/// record refuses, or an item not held by whom the write required, is the
/// record's answer; a write no store takes is the caller's mistake; and a
/// store that will not answer is no reading at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store answered, and its answer is that the act cannot be done as
    /// asked: no such item, text naming more than one, or an act already done
    /// — a close of a closed item is one.
    Refused(String),
    /// The store answered, and its answer is that the item is held by someone
    /// other than the holder a fenced write named — so NOTHING was written.
    Moved(String),
    /// The write asked for is one the contract refuses whatever the store
    /// holds — an update setting a status other than `open` — and it is
    /// answered before the store is asked, with nothing written.
    Usage(String),
    /// The store could not be run, or did not answer something readable.
    Unreadable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Refused(why) => write!(f, "{why}"),
            StoreError::Moved(why) => write!(f, "{why}"),
            StoreError::Usage(why) => write!(f, "{why}"),
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
/// | `capabilities` | [`capabilities`](Store::capabilities), and [`declared`](Store::declared) unvalidated |
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
/// NOTHING BESIDE THE TABLE: a write a verb makes under a fence — a hand-over
/// fenced on its holder, a reopen, a retire's withdrawal fenced on the seat and
/// the status it listed — is one of these verbs carrying the fence as its own
/// fields, so every store answers it, an adapter out of process included, and
/// takes it in one call.
///
/// No storage shape crosses this trait: an order and a run's record go in as
/// the contract's own types, and where a store keeps them is its adapter's.
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
    /// A row carries the item's holder and its run's record, read as [`show`]
    /// reads them: a row a person holds, or one carrying a record this fleet
    /// does not read, refuses the listing Unreadable in `show`'s words, and is
    /// never a row held by nobody or carrying no run.
    ///
    /// A LISTING IS NEVER CAPPED. A store whose listing answers its first rows
    /// by default is asked for all of them, because a truncated list reads
    /// exactly like a whole one.
    ///
    /// [`show`]: Store::show
    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError>;

    /// One item filed, answered as the id the store gave it.
    ///
    /// A run's record is titled by its own id, which nothing knows until
    /// this returns — so the title in [`NewItem`] is what the record carries
    /// until the caller retitles it through [`update`](Store::update), and the
    /// caller's read-back is what says the second write landed.
    ///
    /// An item that does not validate — a priority past 4 — is Unreadable,
    /// and nothing is sent: the store is never asked to file it.
    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError>;

    /// The item's title, its assignee and its status moved in ONE write. A
    /// field the change leaves out is left alone, an assignee of `None` is
    /// the item handed to nobody, and the status is `open` or left alone.
    ///
    /// A change carrying `if_assignee` lands only while that seat holds the
    /// item — `None` for nobody — and is [`StoreError::Moved`] otherwise, with
    /// nothing written.
    ///
    /// A change naming no field is Unreadable, and one setting a status other
    /// than `open` is [`StoreError::Usage`]; both are answered before the
    /// store is asked, with nothing written: a caller's mistake, and never a
    /// write that quietly did something else.
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError>;

    /// The item's order replaced whole. The assignee and the run's record are
    /// left as they were: a store keeping the two beside each other writes
    /// the order without touching the record.
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError>;

    /// The assignee cleared and the order taken away in ONE act — and the
    /// status set to `open` in the same act where the fence says `reopen`.
    /// After any answer, an item never reads with its assignee cleared while
    /// its order stands.
    ///
    /// A fence the item does not meet — another holder than `if_assignee`,
    /// another status than `if_status` — is [`StoreError::Moved`], with
    /// nothing written. [`WithdrawFence::default`] is the plain withdrawal.
    /// One that reopens names the status its caller read as well as the
    /// holder, so an item closed since that read is Moved and never reopened.
    ///
    /// NO DEFAULT BODY: two writes in a row are exactly the half-withdrawal
    /// the one act exists to rule out, so every store says how it takes all
    /// of it at once.
    fn order_withdraw(
        &self,
        id: &ItemId,
        fence: &WithdrawFence,
        by: &Actor,
    ) -> Result<(), StoreError>;

    /// The run's record on the item that records it, replaced whole. The
    /// order beside it is left as it was.
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError>;

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
    /// `by` is an [`Actor`] as every write's is, and a landing closes as the
    /// seat that holds the item. A store that fences a close on the assignee
    /// matches that seat to its own assignee inside its adapter.
    fn close(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<(), StoreError>;

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
    /// whether it is a scratch board, the prefix its ids carry, the command
    /// a seat types against it, which the guards police, and the types and
    /// priorities its items take, which a routine's item is held to.
    fn capabilities(&self) -> Result<Capabilities, StoreError>;

    /// The same `capabilities` verb, answered as the store gave it and NOT yet
    /// held to [`Capabilities::validate`]: what `fleet doctor` reads, because
    /// a store that answered what the contract's rules refuse is a finding,
    /// and one that did not answer is could not tell. Every other caller asks
    /// [`capabilities`](Store::capabilities), which an adapter executable
    /// holds to those rules first; the default is that answer.
    fn declared(&self) -> Result<Capabilities, StoreError> {
        self.capabilities()
    }

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
    /// Where the `[store] adapter` in `policy` was written, which a refusal
    /// of it names: the project's own file, or a flag a verb carried into it.
    pub source: AdapterSource,
    /// The `PATH` a pack's store adapter runs on: the caller's constructed
    /// child `PATH`, and never this process's own, which under a service holds
    /// neither a package manager's prefix nor the user's local bin
    /// (lessons claude-code D1) — so an entry that execs its runtime, and the
    /// store's own binary, would find neither. The directory of the runtime the
    /// adapter's pack runs under goes in front where this misses it, as a run
    /// line's does ([`child_path_for`](crate::runtime::child_path_for)).
    ///
    /// EMPTY is a caller with no controller behind it, and the adapter then
    /// carries this process's own `PATH`, as a run line's children do.
    pub search_path: &'a str,
    /// The bound on each call of the store opened: [`STORE_TIMEOUT`], or less
    /// for a caller that cannot wait that long.
    pub timeout: Duration,
    /// Where an adapter's bare name is resolved, or `None` for a caller with
    /// no machine's packs behind it, where a name resolves nowhere.
    pub packs: Option<PackDirs<'a>>,
}

pub use crate::adapter::{AdapterSource, PackDirs};

/// The store adapter a project's file that names none opens, by name through
/// the installed packs like any other: the one the bd pack carries, which
/// `fleet create` installs unless it is told otherwise.
pub const DEFAULT_ADAPTER: &str = "bd";

/// The project's store, as `[store] adapter` in its own file names it: an
/// absolute path to an executable file is an adapter that answers the contract
/// at that path; a name is the store adapter the installed packs carry under
/// it; and no key at all is [`DEFAULT_ADAPTER`], resolved as a name. Anything
/// else is could not tell, naming what was written where it was
/// ([`AdapterSource`]), and nothing is run.
///
/// ONE OPENER FOR EVERY CALLER — the verbs, the run pass and `fleet prime` —
/// so the store a verb writes to and the one the pass reads are one store.
///
/// AN ADAPTER NAMED BY PATH RUNS ON THIS PROCESS'S OWN `PATH`: no pack
/// declares what it runs under, so there is no runtime to put in front of the
/// caller's search path, and an entry that execs one finds it only where the
/// person's own shell does. A named adapter runs on the search path
/// ([`Opening::search_path`]).
pub fn open(at: &Opening) -> Result<Box<dyn Store>, StoreError> {
    let resolved = resolve(&Wanted {
        kind: AdapterKind::Store,
        policy: at.policy,
        source: at.source,
        search_path: at.search_path,
        packs: at.packs,
        default: DEFAULT_ADAPTER,
    })
    .map_err(StoreError::Unreadable)?;
    let store = exec::Exec::at(&resolved.entry, at.root).with_timeout(at.timeout);
    Ok(match resolved.path {
        Some(path) => Box::new(store.on_path(path)),
        None => Box::new(store),
    })
}

/// The adapter `[store] adapter` names, as a line names it to a person:
/// [`DEFAULT_ADAPTER`] where the key names nothing, and otherwise the file
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
        _ => String::from(DEFAULT_ADAPTER),
    }
}

/// The project's own file as a table, for a caller that has resolved no
/// project around the root: `<root>/.fleet/project.toml` where it is a file,
/// else `<root>/fleet.toml`, else an empty table — which opens
/// [`DEFAULT_ADAPTER`]. A file that will not read or parse is could not tell,
/// naming it.
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

/// The body an append is handed, held to its kind's rules before anything is
/// written — the one refusal every store answers, word for word.
pub(crate) fn validated(item: &str, body: &Body) -> Result<(), StoreError> {
    body.validate().map_err(|why| {
        StoreError::Unreadable(format!(
            "the {} entry for {item} does not validate: {why} — nothing was written",
            body.kind()
        ))
    })
}

/// The item a create is handed, held to [`NewItem::validate`] before anything
/// is sent — the one refusal every store answers, word for word.
pub(crate) fn validated_new(item: &NewItem) -> Result<(), StoreError> {
    item.validate().map_err(|why| {
        StoreError::Unreadable(format!(
            "the item `{}` does not validate: {why} — nothing was written",
            item.title
        ))
    })
}

/// The change, or the refusal every store answers for one it must not take,
/// word for word and before anything is run: a change naming no field, and a
/// status other than `open`.
pub(crate) fn writable(change: &Update) -> Result<(), StoreError> {
    if change.is_empty() {
        return Err(StoreError::Unreadable(String::from(
            "an update names no title, assignee or status — nothing was written",
        )));
    }
    match &change.status {
        Some(status) if *status != Status::Open => Err(StoreError::Usage(format!(
            "an update sets a status only to open, and this one names `{status}` — nothing was \
             written"
        ))),
        _ => Ok(()),
    }
}
