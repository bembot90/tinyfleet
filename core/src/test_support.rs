//! What a suite drives core's verbs with. Compiled into this crate's own
//! tests, and into the library itself only under the `test-support` feature,
//! which a dependent's DEV-dependency turns on: under resolver 2 that keeps it
//! out of the binary a release build produces.
//!
//! It holds the APPLYING fake store and the in-memory board over it. A suite
//! that wants a real `bd` board builds one in its own `tests/common`, which is
//! where everything needing a subprocess stays.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::entry::{self, Body, Entry};
use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;
use crate::store::types::{Capabilities, ExportSpec, Refusal, RefusalReason, Vocabulary};
use crate::store::{
    already_cleared, already_closed, holder_named, validated_new, writable, Filter, HoldId, Item,
    ItemId, ItemSummary, NewItem, Order, OrderState, ReadProof, RunRecord, Status, Store,
    StoreError, Update, Version, WithdrawFence,
};

pub mod stub;

/// Where the fake's export goes, relative to the root it is handed: a file of
/// its own and under a directory of its own, so an arm on the fake that passes
/// passes on what the store declared and never on a path a verb spelled.
pub const EXPORT_FILE: &str = ".store/export.jsonl";

/// The directory [`EXPORT_FILE`] sits in, which the fake declares as the
/// store's own.
pub const EXPORT_DIR: &str = ".store/";

/// Names one board's directory apart from the next in the same process.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A store held in memory, for the arms whose subject is not the store.
///
/// IT APPLIES WHAT IT IS TOLD. A write moves the stored item, so the read
/// beside it answers the write, and `export` writes a file whose bytes move
/// when an item moves — which is what lets a landing's export check run here
/// with no `bd` on the box at all. The recorded log is kept beside that, so an
/// arm can still assert the line a verb wrote.
///
/// The one failure a real store will not produce on demand — a read-back that
/// disagrees with the write beside it — is [`FakeStore::ignore_writes`], off
/// until an arm asks for it.
///
/// Every item is held as the contract's own [`Item`] — its order, its run's
/// record and the names of another writer's keys are its own fields — and a
/// read answers a clone of it, carrying the contract's JSON of it as the
/// read's proof. Nothing here reads any store's own shape.
#[derive(Default)]
pub struct FakeStore {
    pub ready: Vec<String>,
    /// Behind a lock because the trait's writes take `&self` and this is what
    /// they move.
    pub items: Mutex<BTreeMap<String, Item>>,
    /// The open items each label's read answers, on top of the items in this
    /// store that carry the label.
    pub labelled: BTreeMap<String, Vec<String>>,
    /// The id the next `create` answers, in order. An empty queue is not a
    /// failure: the store names what it files, as the real one does.
    pub creates: Mutex<Vec<String>>,
    /// What another writer keeps on each item, by key, as it was planted:
    /// values no write here reads or moves, whose names are the item's
    /// `foreign`.
    pub planted: Mutex<BTreeMap<String, serde_json::Map<String, serde_json::Value>>>,
    /// The items whose run's record is at a shape this fleet does not read,
    /// which the contract answers as no item at all: their read refuses.
    pub unreadable_runs: Mutex<BTreeSet<String>>,
    /// Rows answered for a seat, keyed by its full id, on top of the items
    /// assigned to it here.
    pub held: BTreeMap<String, Vec<ItemSummary>>,
    pub writes: Mutex<Vec<String>>,
    /// The holds raised, in the order they were raised: the item and the
    /// question. The nth hold's id is `hold-<n>`, so an entry naming one and
    /// the call that raised it can be compared.
    pub holds: Mutex<Vec<(String, String)>>,
    /// The holds a `hold_clear` has closed, by id, so the open listing below
    /// answers what this store has actually been told.
    pub cleared: Mutex<Vec<String>>,
    /// Why each closed item was closed. The contract's item carries no close
    /// reason, so an arm asserting one reads it here.
    pub closed: Mutex<BTreeMap<String, String>>,
    /// Answered instead of a read, where an arm is about the store refusing.
    pub unreadable: Option<String>,
    /// Where `export` writes. A store with no root logs the export and writes
    /// nothing: only a rig that gave it a root has a directory to write into.
    pub root: Option<PathBuf>,
    /// Whether this store declares no export at all: its capabilities answer
    /// none, and an export asked of it anyway is refused. Off by default.
    pub no_export: bool,
    /// The item types and priorities its capabilities declare: the contract's
    /// own unless an arm declares others.
    pub vocabulary: Vocabulary,
    /// While set, a write is recorded and NOT applied — the disagreement a real
    /// store will not produce on demand. Off until an arm asks for it, through
    /// [`FakeStore::ignore_writes`] and never by hand.
    pub deaf: Mutex<bool>,
    /// How many items this store has filed, which names the ones it filed
    /// itself and stamps them in filing order.
    pub filed: AtomicUsize,
    /// Each item's comments, in the order they were added: the entries an
    /// append writes and the raw text [`FakeStore::comment`] plants, read
    /// through [`entry::read_row`] as the real store's are.
    pub comments: Mutex<BTreeMap<String, Vec<Comment>>>,
    /// How many comments this store has taken, which names the next one and
    /// stamps it a second after the last.
    pub comment_ids: AtomicUsize,
    /// Whether an item this store names itself is named `fx-` and three
    /// base-36 characters rather than `fx-<n>`. Off by default. A counted id
    /// cannot be made ambiguous — every prefix of a number is another number
    /// the store already holds — so a store asked the ambiguity check through
    /// ids of its own minting, the stub's, mints these.
    pub hashed: bool,
    /// The refusal behind the last Refused or Moved this store answered, typed:
    /// the reason, the text the error carries, and the ids an ambiguity names.
    /// Every refusal here is built from one of these and its text derived from
    /// it, so the stub answers exit 1 by its reason and never by reading the
    /// text back.
    pub refusal: Mutex<Option<Refusal>>,
}

/// What a [`FakeStore`] holds between two calls, as JSON: every item, hold,
/// comment and count, and the seeds and knobs an arm set on it. The stub keeps
/// one of these in a file, since each of its calls is a process of its own.
///
/// Not held: the root, which is the request's; `deaf`, which is one call's;
/// `hashed`, which is the stub's; and the last refusal, which is one answer's.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub ready: Vec<String>,
    pub items: BTreeMap<String, Item>,
    pub labelled: BTreeMap<String, Vec<String>>,
    pub creates: Vec<String>,
    pub planted: BTreeMap<String, serde_json::Map<String, serde_json::Value>>,
    pub unreadable_runs: BTreeSet<String>,
    pub held: BTreeMap<String, Vec<ItemSummary>>,
    pub writes: Vec<String>,
    pub holds: Vec<(String, String)>,
    pub cleared: Vec<String>,
    pub closed: BTreeMap<String, String>,
    pub unreadable: Option<String>,
    pub no_export: bool,
    pub vocabulary: Vocabulary,
    pub filed: usize,
    pub comments: BTreeMap<String, Vec<Comment>>,
    pub comment_ids: usize,
}

/// One comment as the store keeps it: the four fields a timeline row is read
/// by, the text as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub text: String,
    pub at: String,
}

impl FakeStore {
    /// A store whose export writes under this root.
    pub fn at(root: &Path) -> FakeStore {
        FakeStore {
            root: Some(root.to_path_buf()),
            ..FakeStore::default()
        }
    }

    /// The store `state` describes, whose export writes under `root`.
    pub fn from_state(state: State, root: &Path) -> FakeStore {
        FakeStore {
            ready: state.ready,
            items: Mutex::new(state.items),
            labelled: state.labelled,
            creates: Mutex::new(state.creates),
            planted: Mutex::new(state.planted),
            unreadable_runs: Mutex::new(state.unreadable_runs),
            held: state.held,
            writes: Mutex::new(state.writes),
            holds: Mutex::new(state.holds),
            cleared: Mutex::new(state.cleared),
            closed: Mutex::new(state.closed),
            unreadable: state.unreadable,
            root: Some(root.to_path_buf()),
            no_export: state.no_export,
            vocabulary: state.vocabulary,
            filed: AtomicUsize::new(state.filed),
            comments: Mutex::new(state.comments),
            comment_ids: AtomicUsize::new(state.comment_ids),
            ..FakeStore::default()
        }
    }

    /// What this store holds, for the next call to be answered from.
    pub fn state(&self) -> State {
        State {
            ready: self.ready.clone(),
            items: self
                .items
                .lock()
                .expect("the items are not poisoned")
                .clone(),
            labelled: self.labelled.clone(),
            creates: self
                .creates
                .lock()
                .expect("the queue is not poisoned")
                .clone(),
            planted: self
                .planted
                .lock()
                .expect("the planted keys are not poisoned")
                .clone(),
            unreadable_runs: self
                .unreadable_runs
                .lock()
                .expect("the marks are not poisoned")
                .clone(),
            held: self.held.clone(),
            writes: self.wrote(),
            holds: self.raised(),
            cleared: self
                .cleared
                .lock()
                .expect("the holds are not poisoned")
                .clone(),
            closed: self
                .closed
                .lock()
                .expect("the reasons are not poisoned")
                .clone(),
            unreadable: self.unreadable.clone(),
            no_export: self.no_export,
            vocabulary: self.vocabulary.clone(),
            filed: self.filed.load(Ordering::SeqCst),
            comments: self
                .comments
                .lock()
                .expect("the comments are not poisoned")
                .clone(),
            comment_ids: self.comment_ids.load(Ordering::SeqCst),
        }
    }

    /// The refusal kept as this store's last, and the error whose text is its
    /// message: Moved for a fence the item did not meet, Refused for the rest.
    fn refused(
        &self,
        reason: RefusalReason,
        message: String,
        candidates: Vec<ItemId>,
    ) -> StoreError {
        let error = match reason {
            RefusalReason::Moved => StoreError::Moved(message.clone()),
            _ => StoreError::Refused(message.clone()),
        };
        *self.refusal.lock().expect("the refusal is not poisoned") = Some(Refusal {
            reason,
            message,
            candidates,
        });
        error
    }

    /// Nothing by that id, in this store's words.
    fn not_here(&self, id: &str) -> StoreError {
        self.refused(
            RefusalReason::Missing,
            format!("{id} is not here"),
            Vec::new(),
        )
    }

    /// An act already done, in the words the store's shared refusal gives it.
    fn already(&self, done: StoreError) -> StoreError {
        self.refused(RefusalReason::Already, done.to_string(), Vec::new())
    }

    /// The id the store names its `n`th item by, where it mints hashed ids:
    /// three base-36 characters of an FNV-1a hash over `n`, and over `n` and a
    /// salt where that id is already held.
    fn hashed_id(&self, n: usize) -> String {
        const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let items = self.items.lock().expect("the items are not poisoned");
        (0_u64..)
            .map(|salt| {
                let mut hash = format!("{n}.{salt}")
                    .bytes()
                    .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
                    });
                let mut id = String::from("fx-");
                for _ in 0..3 {
                    id.push(char::from(DIGITS[(hash % 36) as usize]));
                    hash /= 36;
                }
                id
            })
            .find(|id| !items.contains_key(id))
            .expect("some salt names an id nothing holds")
    }

    pub fn wrote(&self) -> Vec<String> {
        self.writes.lock().expect("the log is not poisoned").clone()
    }

    pub fn raised(&self) -> Vec<(String, String)> {
        self.holds
            .lock()
            .expect("the holds are not poisoned")
            .clone()
    }

    /// One item put in the store as it stands, without a write.
    pub fn seed(&self, item: Item) {
        self.items
            .lock()
            .expect("the items are not poisoned")
            .insert(item.id.to_string(), item);
    }

    /// One stored item moved without a write, for a field the trait carries no
    /// verb for.
    pub fn amend(&self, item: &str, change: impl FnOnce(&mut Item)) {
        let mut items = self.items.lock().expect("the items are not poisoned");
        let held = items
            .get_mut(item)
            .unwrap_or_else(|| panic!("{item} is in this store"));
        change(held);
    }

    /// The stored item as it stands, for an arm asserting on state rather than
    /// on the log.
    pub fn stored(&self, item: &str) -> Option<Item> {
        self.items
            .lock()
            .expect("the items are not poisoned")
            .get(item)
            .cloned()
    }

    /// Every write from here on is recorded and thrown away, so the read beside
    /// it disagrees with it. The failure the verbs' read-backs exist to catch,
    /// and the one a real store will not produce on demand.
    pub fn ignore_writes(&self) {
        *self.deaf.lock().expect("the knob is not poisoned") = true;
    }

    /// One raw comment planted on an item, answered as its id: a person's text,
    /// or an entry an append would refuse to write. No write is logged and the
    /// knob is not asked, because this is a rig putting the board in a state.
    pub fn comment(&self, item: &str, author: &str, text: &str) -> String {
        let comment = self.minted(author, text);
        let id = comment.id.clone();
        self.comments
            .lock()
            .expect("the comments are not poisoned")
            .entry(item.to_string())
            .or_default()
            .push(comment);
        id
    }

    /// The next comment's id, `c-<n>`, and its time, a second after the last.
    fn minted(&self, author: &str, text: &str) -> Comment {
        let n = self.comment_ids.fetch_add(1, Ordering::SeqCst) + 1;
        Comment {
            id: format!("c-{n}"),
            author: author.to_string(),
            text: text.to_string(),
            at: format!("2026-01-01T00:{:02}:{:02}Z", n / 60, n % 60),
        }
    }

    /// ANOTHER WRITER'S KEYS put on the item: each key of the object is named
    /// in the item's `foreign` and its value kept as given, replacing one the
    /// same key held. Every key is another writer's, whatever it is called —
    /// the item's order and run's record are its own fields, which only the
    /// trait's writes and [`FakeStore::amend`] move. The trait writes only the
    /// contract's types, so this is a rig's way in, and it is logged as
    /// nothing: the rig is putting the board in a state.
    pub fn plant_metadata(&self, item: &str, payload: &str) {
        let Some(serde_json::Value::Object(theirs)) = crate::store::first_value(payload) else {
            panic!("another writer's keys on {item} are one JSON object: {payload}");
        };
        let mut items = self.items.lock().expect("the items are not poisoned");
        let held = items
            .get_mut(item)
            .unwrap_or_else(|| panic!("{item} is in this store"));
        let mut planted = self
            .planted
            .lock()
            .expect("the planted keys are not poisoned");
        let kept = planted.entry(item.to_string()).or_default();
        for (key, value) in theirs {
            if !held.foreign.contains(&key) {
                held.foreign.push(key.clone());
            }
            kept.insert(key, value);
        }
    }

    /// The item's run's record put at a shape this fleet does not read, which
    /// no [`RunRecord`] can hold: every read of the item refuses from here
    /// on, as the contract reads one.
    pub fn unreadable_run(&self, item: &str) {
        assert!(self.stored(item).is_some(), "{item} is in this store");
        self.unreadable_runs
            .lock()
            .expect("the marks are not poisoned")
            .insert(item.to_string());
    }

    /// Writes land again.
    pub fn apply_writes(&self) {
        *self.deaf.lock().expect("the knob is not poisoned") = false;
    }

    fn deaf(&self) -> bool {
        *self.deaf.lock().expect("the knob is not poisoned")
    }

    fn log(&self, line: String) -> Result<(), StoreError> {
        self.writes
            .lock()
            .expect("the log is not poisoned")
            .push(line);
        Ok(())
    }

    fn refuse<T>(&self) -> Option<Result<T, StoreError>> {
        self.unreadable
            .as_ref()
            .map(|why| Err(StoreError::Unreadable(why.clone())))
    }

    /// One stored item moved. A write to an item nobody filed is the store's
    /// own answer — there is no such item — and never a silent success.
    fn moving(&self, item: &str, change: impl FnOnce(&mut Item)) -> Result<(), StoreError> {
        if self.deaf() {
            return Ok(());
        }
        let mut items = self.items.lock().expect("the items are not poisoned");
        let held = items.get_mut(item).ok_or_else(|| self.not_here(item))?;
        change(held);
        Ok(())
    }

    /// The fence a fenced update and a fenced withdrawal write behind: the
    /// item held by `holder` (`""` for nobody), or [`StoreError::Moved`] and
    /// nothing written — the rule bd's `--if-assignee` keeps.
    fn held_by(&self, item: &str, holder: &str) -> Result<(), StoreError> {
        let items = self.items.lock().expect("the items are not poisoned");
        let held = items.get(item).ok_or_else(|| self.not_here(item))?;
        let now = held
            .assignee
            .map(|seat| seat.to_string())
            .unwrap_or_default();
        if now != holder {
            return Err(self.refused(RefusalReason::Moved, moved(item, holder, &now), Vec::new()));
        }
        Ok(())
    }

    /// The fence beside it: the item reading `status`, or
    /// [`StoreError::Moved`] and nothing written — the rule bd's `--if-status`
    /// keeps.
    fn status_is(&self, item: &str, status: &str) -> Result<(), StoreError> {
        let items = self.items.lock().expect("the items are not poisoned");
        let held = items.get(item).ok_or_else(|| self.not_here(item))?;
        if held.status != status {
            return Err(self.refused(
                RefusalReason::Moved,
                restatused(item, status, held.status.as_str()),
                Vec::new(),
            ));
        }
        Ok(())
    }

    /// The items an open hold stands against, which is what takes one out of
    /// the ready set until somebody clears that hold.
    fn on_hold(&self) -> Vec<String> {
        let closed = self.cleared.lock().expect("the holds are not poisoned");
        let raised = self.holds.lock().expect("the holds are not poisoned");
        raised
            .iter()
            .enumerate()
            .filter(|(n, _)| !closed.contains(&format!("hold-{}", n + 1)))
            .map(|(_, (item, _))| item.clone())
            .collect()
    }

    /// One item's row: the contract's JSON of it, with what another writer
    /// planted on it beside that.
    fn row(&self, item: &Item) -> serde_json::Value {
        let planted = self
            .planted
            .lock()
            .expect("the planted keys are not poisoned");
        row_of(item, planted.get(item.id.as_str()))
    }

    /// Every item's row, in id order.
    fn rows(&self) -> Vec<serde_json::Value> {
        let items = self.items.lock().expect("the items are not poisoned");
        items.values().map(|item| self.row(item)).collect()
    }

    /// One stored item as a listing's row answers it: the item's own fields,
    /// or the refusal its read answers, which refuses the listing it is in.
    fn summary(&self, item: &Item) -> Result<ItemSummary, StoreError> {
        self.readable(item)?;
        Ok(ItemSummary {
            id: item.id.clone(),
            title: item.title.clone(),
            status: item.status.clone(),
            item_type: item.item_type.clone(),
            labels: item.labels.clone(),
            assignee: item.assignee,
            order: item.order.clone(),
            run: item.run.clone(),
            foreign: item.foreign.clone(),
        })
    }

    /// The item, where its run's record is one this fleet reads; else the
    /// refusal the contract answers for a record it does not.
    fn readable(&self, item: &Item) -> Result<(), StoreError> {
        let unread = self
            .unreadable_runs
            .lock()
            .expect("the marks are not poisoned")
            .contains(item.id.as_str());
        if unread {
            return Err(StoreError::Unreadable(format!(
                "{}'s run record is not one this fleet reads",
                item.id
            )));
        }
        Ok(())
    }

    /// Each id as the summary of the item stored under it, and an id-only
    /// summary where nothing is: a seeded id names no item of its own.
    fn summaries(&self, ids: Vec<String>) -> Result<Vec<ItemSummary>, StoreError> {
        let items = self.items.lock().expect("the items are not poisoned");
        ids.into_iter()
            .map(|id| match items.get(&id) {
                Some(item) => self.summary(item),
                None => Ok(ItemSummary {
                    id: ItemId::from(id),
                    ..ItemSummary::default()
                }),
            })
            .collect()
    }

    /// The seeded ids, plus every item this store holds that it calls ready:
    /// open, with no dependency standing and no hold raised against it. A
    /// store computes its own ready set and never holds a second copy of it.
    ///
    /// OPEN AND NOT MERELY UNCLOSED: `bd ready` leaves an `in_progress` item
    /// out, measured on 1.3.0, and a fake that listed one would pass an arm
    /// asserting a withdrawal put its item back in the ready set.
    fn ready_ids(&self) -> Vec<String> {
        let mut ids = self.ready.clone();
        let on_hold = self.on_hold();
        for item in self
            .items
            .lock()
            .expect("the items are not poisoned")
            .values()
        {
            let open = item.status == Status::Open;
            let free = item.blockers.is_empty() && !on_hold.iter().any(|held| item.id == *held);
            if open && free && !ids.iter().any(|id| item.id == *id) {
                ids.push(item.id.to_string());
            }
        }
        ids
    }

    /// The seeded list, plus every open item in this store carrying the label —
    /// so an item labelled through a write is found by the same read.
    fn labelled_ids(&self, label: &str) -> Vec<String> {
        let mut found = self.labelled.get(label).cloned().unwrap_or_default();
        for item in self
            .items
            .lock()
            .expect("the items are not poisoned")
            .values()
        {
            let carries = item.labels.iter().any(|held| held == label);
            if carries && item.status != Status::Closed && !found.iter().any(|id| item.id == *id) {
                found.push(item.id.to_string());
            }
        }
        found
    }

    /// The seeded rows for the seat, plus every item this store holds assigned
    /// to it, under the seat's full id.
    fn assigned(&self, seat: &SeatId) -> Result<Vec<ItemSummary>, StoreError> {
        let mut rows = self
            .held
            .get(&seat.to_string())
            .cloned()
            .unwrap_or_default();
        for item in self
            .items
            .lock()
            .expect("the items are not poisoned")
            .values()
        {
            let mine = item.assignee == Some(*seat);
            if mine && !rows.iter().any(|row| row.id == item.id) {
                rows.push(self.summary(item)?);
            }
        }
        Ok(rows)
    }

    /// The one item the text names, as it stands, or the store's word that
    /// the text names none or more than one.
    ///
    /// THE ARGUMENT IS RESOLVED BY THE CONTRACT'S RULES, [`named`]. Only a
    /// read resolves; every write here takes a whole id, so a verb that wrote
    /// under the text it was typed is a refusal on this board and never a
    /// write that quietly landed.
    fn the_one(&self, item: &str) -> Result<Item, StoreError> {
        let items = self.items.lock().expect("the items are not poisoned");
        let matches: Vec<&Item> = named(items.keys(), item)
            .into_iter()
            .filter_map(|id| items.get(&id))
            .collect();
        match matches.as_slice() {
            [held] => Ok((*held).clone()),
            [] => Err(self.refused(
                RefusalReason::Missing,
                format!("{item} is not in the store"),
                Vec::new(),
            )),
            many => {
                let candidates: Vec<ItemId> = many.iter().map(|held| held.id.clone()).collect();
                let named = candidates
                    .iter()
                    .map(ItemId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(self.refused(
                    RefusalReason::Ambiguous,
                    format!(
                        "`{item}` matches more than one item — {named} — and more of the id says \
                         which one this is"
                    ),
                    candidates,
                ))
            }
        }
    }
}

/// One item as the contract's JSON — the text a read's proof carries and the
/// line the export writes — with another writer's keys beside it under
/// `metadata`, where any were planted.
///
/// THOSE VALUES ARE ANSWERED BECAUSE A CHECK READS THEM. The conformance
/// check on another writer's keys asks that a write here left them as
/// planted, and it reads them off the proof under `metadata`, the name it
/// gives what another writer keeps on an item.
fn row_of(
    item: &Item,
    planted: Option<&serde_json::Map<String, serde_json::Value>>,
) -> serde_json::Value {
    let mut row = serde_json::to_value(item).expect("an item is the contract's JSON");
    if let Some(theirs) = planted.filter(|theirs| !theirs.is_empty()) {
        row["metadata"] = serde_json::Value::Object(theirs.clone());
    }
    row
}

/// A holder as a log line names one: the seat, or `nobody`.
fn logged(holder: &Option<SeatId>) -> String {
    holder
        .map(|seat| seat.to_string())
        .unwrap_or_else(|| String::from("nobody"))
}

/// The ids a `show` argument names, by the contract's three rules — which bd
/// 1.3.0 answered alike on a scratch board. A whole id names itself. Else a
/// whole HASH, the part after the prefix, names its item (`7cx` is `fx-7cx`,
/// even with a child `fx-7cx.1` beside it). Else every id whose hash OPENS
/// WITH the argument is named, with a leading prefix taken off the argument
/// first (`fx-7c` names what `7c` does, and `x-7c` names nothing), so `35`
/// names nothing beside `fx-h35`, which holds it but does not open with it.
/// One id is the item, more than one an ambiguity, none a missing id.
fn named<'a>(ids: impl Iterator<Item = &'a String> + Clone, given: &str) -> Vec<String> {
    let parts = |id: &'a str| id.split_once('-').unwrap_or(("", id));
    let needle = |prefix: &str| {
        given
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('-'))
            .unwrap_or(given)
    };
    if let Some(whole) = ids.clone().find(|id| id.as_str() == given) {
        return vec![whole.clone()];
    }
    let hashed: Vec<String> = ids
        .clone()
        .filter(|id| {
            let (prefix, hash) = parts(id);
            hash == needle(prefix)
        })
        .cloned()
        .collect();
    if !hashed.is_empty() {
        return hashed;
    }
    ids.filter(|id| {
        let (prefix, hash) = parts(id);
        hash.starts_with(needle(prefix))
    })
    .cloned()
    .collect()
}

impl Store for FakeStore {
    /// The ready set and a label's items as the ids [`FakeStore::ready`] and
    /// [`FakeStore::labelled`] seed, each the summary of the item stored under
    /// it; a seat's items as its seeded rows and the items assigned to it.
    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        match filter {
            Filter::Ready => self.summaries(self.ready_ids()),
            Filter::Label(label) => self.summaries(self.labelled_ids(label)),
            Filter::Assignee(seat) => self.assigned(seat),
        }
    }

    /// The id `show` would answer, resolved by the same [`named`], and nothing
    /// else of the item read.
    fn resolve(&self, id: &str) -> Result<ItemId, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        Ok(self.the_one(id)?.id)
    }

    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError> {
        validated_new(item)?;
        self.log(format!(
            "create {} {} {} [{}] {by}",
            item.title,
            item.description,
            item.item_type,
            item.labels.join(",")
        ))?;
        let n = self.filed.fetch_add(1, Ordering::SeqCst) + 1;
        let queued = {
            let mut queued = self.creates.lock().expect("the queue is not poisoned");
            (!queued.is_empty()).then(|| queued.remove(0))
        };
        let id = match queued {
            Some(id) => id,
            None if self.hashed => self.hashed_id(n),
            None => format!("fx-{n}"),
        };
        if self.deaf() {
            return Ok(ItemId::from(id));
        }
        self.items
            .lock()
            .expect("the items are not poisoned")
            .insert(
                id.clone(),
                Item {
                    id: ItemId::from(id.as_str()),
                    title: item.title.clone(),
                    description: item.description.clone(),
                    status: Status::Open,
                    item_type: item.item_type.clone(),
                    labels: item.labels.clone(),
                    ..Item::default()
                },
            );
        self.planted
            .lock()
            .expect("the planted keys are not poisoned")
            .remove(&id);
        self.unreadable_runs
            .lock()
            .expect("the marks are not poisoned")
            .remove(&id);
        Ok(ItemId::from(id))
    }

    /// One log line naming each field the change moves — `title <t>`,
    /// `assignee <seat>` or `assignee nobody`, `status open` — and the fence
    /// it names, `if_assignee <seat>` or `if_assignee nobody`; then the moves
    /// themselves, only while the fence holds.
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError> {
        writable(change)?;
        let mut line = format!("update {id}");
        if let Some(title) = &change.title {
            line.push_str(&format!(" title {title}"));
        }
        if let Some(assignee) = &change.assignee {
            line.push_str(&format!(" assignee {}", logged(assignee)));
        }
        if let Some(status) = &change.status {
            line.push_str(&format!(" status {status}"));
        }
        if let Some(holder) = &change.if_assignee {
            line.push_str(&format!(" if_assignee {}", logged(holder)));
        }
        self.log(format!("{line} {by}"))?;
        if let Some(holder) = &change.if_assignee {
            self.held_by(id, &holder.map(|seat| seat.to_string()).unwrap_or_default())?;
        }
        self.moving(id, |held| {
            if let Some(title) = &change.title {
                held.title = title.clone();
            }
            if let Some(assignee) = &change.assignee {
                held.assignee = *assignee;
            }
            if let Some(status) = &change.status {
                held.status = status.clone();
            }
        })
    }

    /// The stored item, cloned, with its row as the read's proof — or, where
    /// its run's record is one this fleet does not read, the refusal the
    /// contract answers for that.
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let held = self.the_one(item)?;
        self.readable(&held)?;
        Ok(Item {
            proof: ReadProof::of(self.row(&held).to_string()),
            ..held
        })
    }

    /// The order replaced whole; the run's record and another writer's keys
    /// beside it stand.
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError> {
        self.log(format!("order_set {id} {} {by}", json_of(order)))?;
        self.moving(id, |held| held.order = OrderState::Ordered(order.clone()))
    }

    /// One log line and every move, which is what the real store's one call
    /// leaves: nobody holding the item, no order and, where the fence says
    /// `reopen`, the status open. No move is made
    /// while the item misses a fence the caller named — bd's `--if-assignee`
    /// and `--if-status`.
    fn order_withdraw(
        &self,
        id: &ItemId,
        fence: &WithdrawFence,
        by: &Actor,
    ) -> Result<(), StoreError> {
        let mut line = format!("order_withdraw {id}");
        if let Some(holder) = &fence.if_assignee {
            line.push_str(&format!(" if_assignee {}", logged(holder)));
        }
        if let Some(status) = &fence.if_status {
            line.push_str(&format!(" if_status {status}"));
        }
        if fence.reopen {
            line.push_str(" reopen");
        }
        self.log(format!("{line} {by}"))?;
        if let Some(holder) = &fence.if_assignee {
            self.held_by(id, &holder.map(|seat| seat.to_string()).unwrap_or_default())?;
        }
        if let Some(status) = &fence.if_status {
            self.status_is(id, status.as_str())?;
        }
        self.moving(id, |held| {
            held.assignee = None;
            held.order = OrderState::None;
            if fence.reopen {
                held.status = Status::Open;
            }
        })
    }

    /// The run's record replaced whole; the order beside it stands.
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        self.log(format!("run_set {id} {} {by}", json_of(run)))?;
        self.moving(id, |held| held.run = Some(run.clone()))
    }

    /// The hold recorded and answered as an id derived from the call's own
    /// order, `hold-<n>`: an arm asserting that a park named the hold it raised
    /// needs the two to agree, and a constant id would agree with a second
    /// hold too.
    fn hold_raise(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<HoldId, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        self.log(format!("hold_raise {id} {reason} {by}"))?;
        let mut raised = self.holds.lock().expect("the holds are not poisoned");
        raised.push((id.to_string(), reason.to_string()));
        Ok(HoldId::from(format!("hold-{}", raised.len())))
    }

    /// A hold this store never raised is Refused as bd refuses a gate it does
    /// not hold, and one already cleared is Refused in the adapter's words.
    fn hold_clear(&self, hold: &HoldId, by: &Actor) -> Result<(), StoreError> {
        self.log(format!("hold_clear {hold} {by}"))?;
        let raised = self.holds.lock().expect("the holds are not poisoned").len();
        if !(1..=raised).any(|n| *hold == format!("hold-{n}")) {
            return Err(self.not_here(hold));
        }
        let mut cleared = self.cleared.lock().expect("the holds are not poisoned");
        if cleared.iter().any(|held| *hold == *held) {
            return Err(self.already(already_cleared(hold)));
        }
        if !self.deaf() {
            cleared.push(hold.to_string());
        }
        Ok(())
    }

    /// Every hold this store has raised and not been told to clear.
    fn holds_open(&self) -> Result<Vec<HoldId>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let closed = self.cleared.lock().expect("the holds are not poisoned");
        let raised = self.holds.lock().expect("the holds are not poisoned");
        Ok((1..=raised.len())
            .map(|n| format!("hold-{n}"))
            .filter(|id| !closed.contains(id))
            .map(HoldId::from)
            .collect())
    }

    /// The reason goes to the log and not onto the item: the real store holds
    /// it in a field of its own that no read here decodes.
    ///
    /// An item already closed is Refused and its first reason stands, which is
    /// what bd's adapter answers after its own read.
    fn close(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<(), StoreError> {
        self.log(format!("close {id} {reason} {by}"))?;
        let closed = self
            .items
            .lock()
            .expect("the items are not poisoned")
            .get(id.as_str())
            .is_some_and(|held| held.status == Status::Closed);
        if closed {
            return Err(self.already(already_closed(id)));
        }
        self.moving(id, |held| held.status = Status::Closed)?;
        if !self.deaf() {
            self.closed
                .lock()
                .expect("the reasons are not poisoned")
                .insert(id.to_string(), reason.to_string());
        }
        Ok(())
    }

    /// Held to the rules bd's append holds it to, with the same refusal, and
    /// kept as one comment whose text is the entry's encoding. A deaf store
    /// answers the id it minted and keeps nothing, which is the disagreement
    /// the read-back is there to catch.
    fn append(&self, item: &ItemId, body: &Body, by: &Actor) -> Result<String, StoreError> {
        self.log(format!("append {item} {} {by}", body.kind()))?;
        crate::store::validated(item, body)?;
        if !self
            .items
            .lock()
            .expect("the items are not poisoned")
            .contains_key(item.as_str())
        {
            return Err(self.not_here(item));
        }
        let comment = self.minted(&by.to_string(), &entry::encode(body));
        let id = comment.id.clone();
        if self.deaf() {
            return Ok(id);
        }
        self.comments
            .lock()
            .expect("the comments are not poisoned")
            .entry(item.to_string())
            .or_default()
            .push(comment);
        Ok(id)
    }

    /// Every comment read through [`entry::read_row`], the function bd's
    /// timeline reads its rows with, in the order they were added.
    fn timeline(&self, item: &ItemId) -> Result<Vec<Entry>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        if !self
            .items
            .lock()
            .expect("the items are not poisoned")
            .contains_key(item.as_str())
        {
            return Err(self.not_here(item));
        }
        let comments = self.comments.lock().expect("the comments are not poisoned");
        let mut entries = Vec::new();
        for comment in comments.get(item.as_str()).into_iter().flatten() {
            let read = entry::read_row(
                item,
                &comment.id,
                &comment.author,
                &comment.text,
                &comment.at,
            )
            .map_err(StoreError::Unreadable)?;
            entries.extend(read);
        }
        Ok(entries)
    }

    /// [`EXPORT_FILE`] in [`EXPORT_DIR`], or no export at all where
    /// [`FakeStore::no_export`] is set. No seat types a command at a store
    /// held in memory, so it declares none.
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        Ok(Capabilities {
            export: (!self.no_export).then(|| ExportSpec {
                file: EXPORT_FILE.to_string(),
                dir: EXPORT_DIR.to_string(),
            }),
            scratch: true,
            item_prefix: None,
            cli: None,
            items: self.vocabulary.clone(),
        })
    }

    /// `into` itself, with nothing made: a store held in memory is a scratch
    /// store already, and one over the answered root is a fresh [`FakeStore`].
    fn scratch(&self, into: &Path) -> Result<PathBuf, StoreError> {
        Ok(into.to_path_buf())
    }

    fn version(&self) -> Result<Version, StoreError> {
        Ok(Version {
            name: String::from("fake"),
            version: String::from("0"),
        })
    }

    /// One JSON object per line, at [`EXPORT_FILE`] under the root the CALLER
    /// names, answered as that path. A store that was given no root of its own
    /// logs the word and writes nothing: a fake nobody rooted is one whose arms
    /// are about the board and not the file. A store that declares no export
    /// refuses it, and writes nothing.
    ///
    /// Each line is an item's row, and carries the item's comments under
    /// `comments`, which is where its entries live — so an append moves the
    /// export's bytes as it moves the real one's.
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError> {
        if self.no_export {
            return Err(StoreError::Unreadable(String::from(
                "this store declares no export",
            )));
        }
        self.log(String::from("export"))?;
        let into = into.join(EXPORT_FILE);
        if self.root.is_none() {
            return Ok(into);
        }
        if let Some(dir) = into.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                StoreError::Unreadable(format!(
                    "the store's own directory {} could not be made: {e}",
                    dir.display()
                ))
            })?;
        }
        let comments = self
            .comments
            .lock()
            .expect("the comments are not poisoned")
            .clone();
        let mut body = String::new();
        for mut row in self.rows() {
            let id = row["id"].as_str().unwrap_or_default().to_string();
            if let Some(held) = comments.get(&id).filter(|held| !held.is_empty()) {
                row["comments"] = json_of(held);
            }
            body.push_str(&row.to_string());
            body.push('\n');
        }
        std::fs::write(&into, body).map_err(|e| {
            StoreError::Unreadable(format!(
                "the export {} was not written: {e}",
                into.display()
            ))
        })?;
        Ok(into)
    }
}

/// A value the fake writes into a log line or its export, as JSON.
fn json_of<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("the value is JSON")
}

/// The refusal the fake answers where a write fenced on its holder finds
/// another holding the item. The fake's words and no store's: bd's are its
/// adapter's, and an adapter's are its own.
fn moved(item: &str, expected: &str, held: &str) -> String {
    format!(
        "{item} is held by {} and not by {} — nothing was written",
        holder_named(held),
        holder_named(expected)
    )
}

/// The refusal the fake answers where a fenced withdrawal finds the item in
/// another status than the one it named.
fn restatused(item: &str, expected: &str, now: &str) -> String {
    format!("{item} reads {now} and not {expected} — nothing was written")
}

/// A store REACHED THROUGH A HANDLE, so a caller that hands the store away
/// still reads what the verbs wrote to it.
///
/// A seam that takes an owned `Box<dyn Store>` leaves an arm nothing to assert
/// on once it has answered; a clone of one of these is the same store.
impl<S: Store + ?Sized> Store for std::sync::Arc<S> {
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        (**self).show(item)
    }
    fn resolve(&self, id: &str) -> Result<ItemId, StoreError> {
        (**self).resolve(id)
    }
    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError> {
        (**self).list(filter)
    }
    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError> {
        (**self).create(item, by)
    }
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError> {
        (**self).update(id, change, by)
    }
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError> {
        (**self).order_set(id, order, by)
    }
    fn order_withdraw(
        &self,
        id: &ItemId,
        fence: &WithdrawFence,
        by: &Actor,
    ) -> Result<(), StoreError> {
        (**self).order_withdraw(id, fence, by)
    }
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        (**self).run_set(id, run, by)
    }
    fn hold_raise(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<HoldId, StoreError> {
        (**self).hold_raise(id, reason, by)
    }
    fn hold_clear(&self, hold: &HoldId, by: &Actor) -> Result<(), StoreError> {
        (**self).hold_clear(hold, by)
    }
    fn holds_open(&self) -> Result<Vec<HoldId>, StoreError> {
        (**self).holds_open()
    }
    fn close(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<(), StoreError> {
        (**self).close(id, reason, by)
    }
    fn append(&self, item: &ItemId, body: &Body, by: &Actor) -> Result<String, StoreError> {
        (**self).append(item, body, by)
    }
    fn timeline(&self, item: &ItemId) -> Result<Vec<Entry>, StoreError> {
        (**self).timeline(item)
    }
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        (**self).capabilities()
    }
    fn version(&self) -> Result<Version, StoreError> {
        (**self).version()
    }
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError> {
        (**self).export(into)
    }
    fn scratch(&self, into: &Path) -> Result<PathBuf, StoreError> {
        (**self).scratch(into)
    }
}

/// A work graph held in memory, with a directory on disk beside it.
///
/// The shape a `bd` scratch board has — a root, a packs directory, one item by title, a
/// document as text — over [`FakeStore`] instead of over a `bd` subprocess. A
/// rig takes one of these where its subject is a verb's logic; the one arm per
/// suite whose subject is the store itself takes a a `bd` scratch board.
pub struct Board {
    pub root: PathBuf,
    pub packs_dir: PathBuf,
    /// The binary's own defaults, materialized beside the packs directory as
    /// `fleet start` materializes them: the resolver's bottom layer.
    pub defaults_dir: PathBuf,
    pub store: FakeStore,
}

impl Board {
    pub fn new(label: &str) -> Board {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-board-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the board root is created");
        let packs_dir = root.join("packs");
        std::fs::create_dir_all(&packs_dir).expect("the packs dir is created");
        let defaults_dir = root.join(crate::defaults::DIR);
        std::fs::create_dir_all(&defaults_dir).expect("the defaults dir is created");
        crate::embedded::write_all(&defaults_dir).expect("the embedded defaults are written");
        let store = FakeStore::at(&root);
        Board {
            root,
            packs_dir,
            defaults_dir,
            store,
        }
    }

    /// A copy of a shipped pack under this board's packs directory.
    pub fn install(&self, name: &str, from: &Path) -> PathBuf {
        let into = self.packs_dir.join(name);
        copy_tree(from, &into);
        into
    }

    pub fn fleet_toml(&self, body: &str) -> &Board {
        std::fs::write(self.root.join("fleet.toml"), body).expect("the policy file is written");
        self
    }

    /// One item, by title, answered as its id.
    pub fn item(&self, title: &str) -> String {
        self.store
            .create(
                &NewItem {
                    title: title.to_string(),
                    description: String::from("a board item"),
                    item_type: String::from("task"),
                    ..NewItem::default()
                },
                &the_test(),
            )
            .expect("the item is filed")
            .to_string()
    }

    /// The whole document, as text: what an arm compares before and after.
    pub fn json(&self, item: &str) -> String {
        self.store
            .show(item)
            .unwrap_or_else(|e| panic!("show {item}: {e}"))
            .proof
            .as_str()
            .to_string()
    }

    /// The writes a rig makes for its own setup, as the store's own calls: a
    /// rig is putting an item in a state, not asserting on the write. The
    /// seat is a seat's full id, which is what an assignment writes.
    pub fn hand_to(&self, item: &str, seat: &str) {
        let seat = crate::seat::identity::SeatId::parse(seat)
            .unwrap_or_else(|e| panic!("a rig hands {item} to a seat: {e}"));
        self.store
            .update(&ItemId::from(item), &Update::assignee(seat), &the_test())
            .unwrap_or_else(|e| panic!("hand {item} to {seat}: {e}"));
    }

    /// The item's order, written as a dispatch writes one.
    pub fn order(&self, item: &str, order: Order) {
        self.store
            .order_set(&ItemId::from(item), &order, &the_test())
            .unwrap_or_else(|e| panic!("the order on {item}: {e}"));
    }

    /// The item's run record, written as a run writes one.
    pub fn run(&self, item: &str, run: RunRecord) {
        self.store
            .run_set(&ItemId::from(item), &run, &the_test())
            .unwrap_or_else(|e| panic!("the run record on {item}: {e}"));
    }

    /// ANOTHER WRITER'S KEYS put on the item — [`FakeStore::plant_metadata`].
    pub fn set_metadata(&self, item: &str, payload: &str) {
        self.store.plant_metadata(item, payload);
    }

    /// A field the trait carries no verb for, moved on the stored item.
    pub fn amend(&self, item: &str, change: impl FnOnce(&mut Item)) {
        self.store.amend(item, change);
    }

    pub fn label(&self, item: &str, label: &str) {
        self.amend(item, |held| held.labels.push(label.to_string()));
    }

    /// One open dependency between two items, which is what takes the first out
    /// of the ready set.
    pub fn blocked_by(&self, item: &str, blocker: &str) {
        self.amend(item, |held| held.blockers.push(ItemId::from(blocker)));
    }

    pub fn status(&self, item: &str, status: &str) {
        self.amend(item, |held| held.status = Status::from(status));
    }

    /// The board's writes forgotten, so the rig's own setup is not in the log
    /// an arm reads.
    pub fn forget_writes(&self) {
        self.store
            .writes
            .lock()
            .expect("the log is not poisoned")
            .clear();
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The actor a rig's own setup writes under: a run, because the setup is no
/// seat's act.
pub fn the_test() -> Actor {
    Actor::typed("run:the-test")
        .expect("a typed actor")
        .expect("with a run's id")
}

/// The `fleet-store-stub` executable this crate's own test build made: in
/// `target/<profile>/`, the directory above the running test binary's `deps/`,
/// which is where `env!("CARGO_BIN_EXE_fleet-store-stub")` points a suite of
/// this crate. The library cannot name that variable, since cargo sets it only
/// for a crate's test targets.
///
/// FOR CORE'S TESTS ONLY. Another crate's test build does not build this
/// crate's executables, so the file here is whatever an earlier core build
/// left, or nothing; the cli's rigs run the copy their own build makes.
pub fn stub_path() -> PathBuf {
    let running = std::env::current_exe().expect("the running test binary has a path");
    let stub = running
        .parent()
        .and_then(Path::parent)
        .expect("a test binary runs from target/<profile>/deps")
        .join("fleet-store-stub");
    assert!(
        stub.is_file(),
        "{} is built by a test build of fleet-core",
        stub.display()
    );
    stub
}

/// A fixture pack the suites run fleet's own code on, by its directory under
/// this crate's `tests/fixtures/packs`: `tiny`, shaped as the doctrine pack
/// (it imports `ts` from the folder beside it, shadows four of the defaults'
/// paths and pins no runtime), and `ts`, shaped as the runtime pack (a
/// `[runtime]` table and a doctor entry of its own). Each carries only what an
/// arm reads off it. The real packs live in the fleet-packs repository, and
/// the checks on their own content run there, against a pinned fleet.
pub fn fixture_pack(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/packs")
        .join(name)
}

pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination directory is created");
    for entry in std::fs::read_dir(from).expect("the source directory is readable") {
        let entry = entry.expect("an entry is readable");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file is copied");
        }
    }
}
