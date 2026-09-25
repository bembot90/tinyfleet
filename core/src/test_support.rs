//! What a suite drives core's verbs with. Compiled into this crate's own
//! tests, and into the library itself only under the `test-support` feature,
//! which a dependent's DEV-dependency turns on: under resolver 2 that keeps it
//! out of the binary a release build produces.
//!
//! It holds the APPLYING fake store and the in-memory board over it. A suite
//! that wants a real `bd` board builds one in its own `tests/common`, which is
//! where everything needing a subprocess stays.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::entry::{self, Body, Entry};
use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;
use crate::store::bd::{
    item_from, keys, opened, order_metadata, order_of, run_metadata, shown, SCHEMA_VERSION,
};
use crate::store::types::{Capabilities, ExportSpec};
use crate::store::{
    already_cleared, already_closed, unchanged, Filter, HoldId, Item, ItemId, ItemSummary, NewItem,
    Order, OrderState, RunRecord, Status, Store, StoreError, Update, Version,
};

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
/// Every read is decoded by [`crate::store::bd::item_from`], the same function
/// the real store's reads go through: a row is built in `bd`'s own JSON shape
/// and handed to it, so the fake cannot decode a field the real store decodes
/// differently.
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
    /// What the metadata writes have left, per item, as one top-level object.
    ///
    /// The write MERGES AT THE TOP LEVEL and replaces one key's object whole,
    /// which is the polarity measured on bd 1.3.0 and pinned by an arm of the
    /// plan suite.
    pub metadata: Mutex<BTreeMap<String, serde_json::Map<String, serde_json::Value>>>,
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
    /// Why each closed item was closed. The real store holds it in a field of
    /// its own that no read decodes but every document carries, and a close
    /// reason is asserted off the document.
    pub closed: Mutex<BTreeMap<String, String>>,
    /// Answered instead of a read, where an arm is about the store refusing.
    pub unreadable: Option<String>,
    /// Where `export` writes. A store with no root logs the export and writes
    /// nothing: only a rig that gave it a root has a directory to write into.
    pub root: Option<PathBuf>,
    /// Whether this store declares no export at all: its capabilities answer
    /// none, and an export asked of it anyway is refused. Off by default.
    pub no_export: bool,
    /// While set, a write is recorded and NOT applied — the disagreement a real
    /// store will not produce on demand. Off until an arm asks for it, through
    /// [`FakeStore::ignore_writes`] and never by hand.
    pub deaf: Mutex<bool>,
    /// How many items this store has filed, which names the ones it filed
    /// itself and stamps them in filing order.
    pub filed: AtomicUsize,
    /// Each item's comments, in the order they were added: the entries an
    /// append writes and the raw text [`FakeStore::comment`] plants, kept the
    /// way bd keeps them and read through the same `read_row`.
    pub comments: Mutex<BTreeMap<String, Vec<Comment>>>,
    /// How many comments this store has taken, which names the next one and
    /// stamps it a second after the last.
    pub comment_ids: AtomicUsize,
}

/// One comment as the store keeps it: bd's four fields, the text as written.
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// A metadata object merged onto the item as ANOTHER WRITER leaves one: a
    /// key fleet does not own, or one of fleet's own at a shape no writer here
    /// makes. The trait writes only the contract's types, so this is a rig's
    /// way in, and it is logged as nothing — the rig is putting the board in a
    /// state. It merges at the top level, as every metadata write here does.
    pub fn plant_metadata(&self, item: &str, payload: &str) {
        let metadata = crate::store::first_value(payload)
            .unwrap_or_else(|| panic!("the metadata for {item} is JSON: {payload}"));
        self.merged(item, metadata)
            .unwrap_or_else(|e| panic!("the metadata on {item}: {e}"));
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
        let held = items
            .get_mut(item)
            .ok_or_else(|| StoreError::Refused(format!("{item} is not here")))?;
        change(held);
        Ok(())
    }

    /// The fence a hand-over writes behind: the item held by `holder` (`""`
    /// for nobody), or [`StoreError::Moved`] and nothing written — the rule
    /// bd's `--if-assignee` keeps.
    fn held_by(&self, item: &str, holder: &str) -> Result<(), StoreError> {
        let items = self.items.lock().expect("the items are not poisoned");
        let held = items
            .get(item)
            .ok_or_else(|| StoreError::Refused(format!("{item} is not here")))?;
        let now = held
            .assignee
            .map(|seat| seat.to_string())
            .unwrap_or_default();
        if now != holder {
            return Err(crate::store::moved(item, holder, &now));
        }
        Ok(())
    }

    /// The fence beside it: the item reading `status`, or
    /// [`StoreError::Moved`] and nothing written — the rule bd's `--if-status`
    /// keeps.
    fn status_is(&self, item: &str, status: &str) -> Result<(), StoreError> {
        let items = self.items.lock().expect("the items are not poisoned");
        let held = items
            .get(item)
            .ok_or_else(|| StoreError::Refused(format!("{item} is not here")))?;
        if held.status != status {
            return Err(crate::store::restatused(item, status, held.status.as_str()));
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

    fn close_reason(&self, item: &str) -> Option<String> {
        self.closed
            .lock()
            .expect("the reasons are not poisoned")
            .get(item)
            .cloned()
    }

    /// The metadata object this item's reads are decoded from: what the writes
    /// have left, or what the seeded item carries where nothing has written.
    fn metadata_of(&self, item: &Item) -> serde_json::Map<String, serde_json::Value> {
        self.metadata
            .lock()
            .expect("the metadata is not poisoned")
            .get(item.id.as_str())
            .cloned()
            .unwrap_or_else(|| seeded_metadata(item))
    }

    /// One metadata write, over the object the item already reads as.
    fn metadata_write(
        &self,
        item: &str,
        change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
    ) -> Result<(), StoreError> {
        if self.deaf() {
            return Ok(());
        }
        let seeded = {
            let items = self.items.lock().expect("the items are not poisoned");
            let held = items
                .get(item)
                .ok_or_else(|| StoreError::Refused(format!("{item} is not here")))?;
            seeded_metadata(held)
        };
        let mut all = self.metadata.lock().expect("the metadata is not poisoned");
        let object = all.entry(item.to_string()).or_insert(seeded);
        change(object);
        Ok(())
    }

    /// Every item as one JSON object, in id order.
    fn rows(&self) -> Vec<serde_json::Value> {
        let items = self.items.lock().expect("the items are not poisoned");
        items
            .values()
            .map(|item| {
                row_of(
                    item,
                    &self.metadata_of(item),
                    self.close_reason(&item.id).as_deref(),
                )
            })
            .collect()
    }

    /// One stored item as a listing's row answers it, its order read off the
    /// METADATA this store would answer a read with, and not off the seeded
    /// field, by the reading the real listing's rows take — so a row whose
    /// index a write has removed answers here as the real listing does.
    fn summary(&self, item: &Item) -> ItemSummary {
        ItemSummary {
            id: item.id.clone(),
            title: item.title.clone(),
            status: item.status.clone(),
            item_type: item.item_type.clone(),
            labels: item.labels.clone(),
            order: order_of(Some(&serde_json::Value::Object(self.metadata_of(item)))),
        }
    }

    /// Each id as the summary of the item stored under it, and an id-only
    /// summary where nothing is: a seeded id names no item of its own.
    fn summaries(&self, ids: Vec<String>) -> Vec<ItemSummary> {
        let items = self.items.lock().expect("the items are not poisoned");
        ids.into_iter()
            .map(|id| match items.get(&id) {
                Some(item) => self.summary(item),
                None => ItemSummary {
                    id: ItemId::from(id),
                    ..ItemSummary::default()
                },
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
    fn assigned(&self, seat: &SeatId) -> Vec<ItemSummary> {
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
                rows.push(self.summary(item));
            }
        }
        rows
    }

    /// The answer `bd show --json` gives under the envelope — the row as an
    /// array of one, or an error coded `not_found` as beads' contract spells
    /// it — opened and read by the same functions the real store's `show` goes
    /// through, so the fake cannot classify an answer the real store classifies
    /// differently. bd 1.3.0's own error carries no code, and the real half of
    /// the contract suite is what reads that one.
    ///
    /// THE ARGUMENT IS RESOLVED AS bd RESOLVES IT, by [`named`], and an
    /// ambiguous one answers what bd 1.3.0 answers: the error with no code, and
    /// the matches on stderr alone. Only a read resolves; every write here
    /// takes a whole id, so a verb that wrote under the text it was typed is a
    /// refusal on this board and never a write that quietly landed.
    fn shown_row(&self, item: &str) -> Result<serde_json::Value, StoreError> {
        let matches = {
            let items = self.items.lock().expect("the items are not poisoned");
            named(items.keys(), item)
                .into_iter()
                .filter_map(|id| items.get(&id).cloned())
                .collect::<Vec<Item>>()
        };
        let (data, said) = match matches.as_slice() {
            [held] => {
                let metadata = self.metadata_of(held);
                let reason = self.close_reason(&held.id);
                (
                    serde_json::json!([row_of(held, &metadata, reason.as_deref())]),
                    String::new(),
                )
            }
            [] => (
                serde_json::json!({
                    "error": "no issues found matching the provided IDs",
                    "code": "not_found",
                }),
                format!(
                    "Issue {item} not found\nHint: this ID may have never existed, or may \
                     reference a deleted/purged record with no trace left in the live database \
                     — try 'bd history {item}'\n"
                ),
            ),
            many => (
                serde_json::json!({ "error": "no issues found matching the provided IDs" }),
                format!(
                    "Error fetching {item}: ambiguous issue ID: \"{item}\" matches {} issues: \
                     [{}]\nUse more characters to disambiguate\n",
                    many.len(),
                    many.iter()
                        .map(|held| held.id.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
            ),
        };
        let answer = serde_json::json!({
            "schema_version": SCHEMA_VERSION,
            "data": data,
        });
        let answer = opened(answer, || String::from("the board held in memory"));
        shown(item, answer, &said)
    }
}

/// The metadata a seeded item carries in its own fields, as the object a read
/// decodes from, under fleet's own keys and at the version each is written at.
/// An unreadable order is a key holding something that is not an object, which
/// is a third answer and not an absence. A seed that wants a run's record the
/// fleet cannot read, or an index at another version, writes that metadata
/// itself: the item's own fields hold only what reads.
fn seeded_metadata(item: &Item) -> serde_json::Map<String, serde_json::Value> {
    let mut object = serde_json::Map::new();
    if let Some(run) = &item.run {
        object.extend(top_level(run_metadata(run)));
    }
    match &item.order {
        OrderState::Ordered(order) => {
            object.extend(top_level(order_metadata(order)));
        }
        OrderState::Unreadable => {
            object.insert(
                String::from(keys::ORDERS),
                serde_json::Value::String(String::new()),
            );
        }
        OrderState::None => {}
    }
    object
}

/// The top-level keys of a metadata object the adapter built, which is what
/// a write merges in.
fn top_level(metadata: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    match metadata {
        serde_json::Value::Object(keys) => keys,
        other => panic!("the adapter's metadata is one object: {other}"),
    }
}

/// One item in the store's own JSON shape — the row a read is decoded from and
/// the line the export writes. A field the store has no value for is OMITTED
/// rather than written null, which is what the real store does and what tells
/// an absent assignee from an empty one.
fn row_of(
    item: &Item,
    metadata: &serde_json::Map<String, serde_json::Value>,
    close_reason: Option<&str>,
) -> serde_json::Value {
    let mut row = serde_json::Map::new();
    row.insert(String::from("id"), item.id.as_str().into());
    row.insert(String::from("title"), item.title.clone().into());
    if !item.description.is_empty() {
        row.insert(String::from("description"), item.description.clone().into());
    }
    row.insert(String::from("status"), item.status.as_str().into());
    row.insert(String::from("issue_type"), item.item_type.clone().into());
    if let Some(assignee) = &item.assignee {
        row.insert(String::from("assignee"), assignee.to_string().into());
    }
    if let Some(reason) = close_reason {
        row.insert(String::from("close_reason"), reason.into());
    }
    row.insert(
        String::from("labels"),
        serde_json::Value::Array(item.labels.iter().map(|l| l.clone().into()).collect()),
    );
    if !metadata.is_empty() {
        row.insert(
            String::from("metadata"),
            serde_json::Value::Object(metadata.clone()),
        );
    }
    if !item.blockers.is_empty() {
        row.insert(
            String::from("dependencies"),
            serde_json::Value::Array(
                item.blockers
                    .iter()
                    .map(|id| serde_json::json!({ "id": id, "status": "open" }))
                    .collect(),
            ),
        );
    }
    serde_json::Value::Object(row)
}

/// The ids a `show` argument names, by the rule bd 1.3.0 resolves one with —
/// measured on a scratch board. A whole id names itself. Else a whole HASH, the
/// part after the prefix, names its item (`7cx` is `fx-7cx`, even with a child
/// `fx-7cx.1` beside it). Else every id whose hash OPENS WITH the argument is
/// named, with a leading prefix taken off the argument first (`fx-7c` names
/// what `7c` does, and `x-7c` names nothing), so `35` names nothing beside
/// `fx-h35`, which holds it but does not open with it. One id is the item,
/// more than one an ambiguity, none a missing id.
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
        Ok(match filter {
            Filter::Ready => self.summaries(self.ready_ids()),
            Filter::Label(label) => self.summaries(self.labelled_ids(label)),
            Filter::Assignee(seat) => self.assigned(seat),
        })
    }

    /// The id `show` would answer, resolved by the same [`named`] and read
    /// off the same row, and nothing else of it decoded.
    fn resolve(&self, id: &str) -> Result<ItemId, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let row = self.shown_row(id)?;
        row.get("id")
            .and_then(serde_json::Value::as_str)
            .map(ItemId::from)
            .ok_or_else(|| StoreError::Unreadable(format!("{id} answered a row naming no id")))
    }

    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError> {
        self.log(format!(
            "create {} {} {} [{}] {by}",
            item.title,
            item.description,
            item.item_type,
            item.labels.join(",")
        ))?;
        let n = self.filed.fetch_add(1, Ordering::SeqCst) + 1;
        let id = {
            let mut queued = self.creates.lock().expect("the queue is not poisoned");
            if queued.is_empty() {
                format!("fx-{n}")
            } else {
                queued.remove(0)
            }
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
        self.metadata
            .lock()
            .expect("the metadata is not poisoned")
            .remove(&id);
        Ok(ItemId::from(id))
    }

    /// One log line naming each field the change moves — `title <t>`,
    /// `assignee <seat>` or `assignee nobody` — and the moves themselves. An
    /// assignee handed to nobody is ABSENT, as bd answers one it cleared.
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError> {
        if change.is_empty() {
            return Err(unchanged());
        }
        let mut line = format!("update {id}");
        if let Some(title) = &change.title {
            line.push_str(&format!(" title {title}"));
        }
        if let Some(assignee) = &change.assignee {
            match assignee {
                Some(seat) => line.push_str(&format!(" assignee {seat}")),
                None => line.push_str(" assignee nobody"),
            }
        }
        self.log(format!("{line} {by}"))?;
        self.moving(id, |held| {
            if let Some(title) = &change.title {
                held.title = title.clone();
            }
            if let Some(assignee) = &change.assignee {
                held.assignee = *assignee;
            }
        })
    }

    /// The row [`FakeStore::shown_row`] answers, decoded as the real store's
    /// `show` decodes it.
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        item_from(item, &self.shown_row(item)?)
    }

    /// One log line, and the assignment only while `from` holds the item.
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("hand_over {item} {from} {to} {by}"))?;
        let to = match to {
            "" => None,
            seat => Some(SeatId::parse(seat).map_err(|why| {
                StoreError::Unreadable(format!(
                    "{item} is not handed over: {why} — nothing was written"
                ))
            })?),
        };
        self.held_by(item, from)?;
        self.moving(item, |held| held.assignee = to)
    }

    /// The order written as the bd adapter writes it — the same object, built
    /// by the same function — and merged at the top level, so the run's record
    /// beside it stands.
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError> {
        let metadata = order_metadata(order);
        self.log(format!("order_set {id} {metadata} {by}"))?;
        self.merged(id, metadata)
    }

    /// One log line and both moves, which is what the real store's one call
    /// leaves: the assignee ABSENT, as bd answers one it cleared, and the key
    /// gone.
    fn order_withdraw(&self, id: &ItemId, by: &Actor) -> Result<(), StoreError> {
        self.log(format!("order_withdraw {id} {by}"))?;
        self.moving(id, |held| held.assignee = None)?;
        self.metadata_write(id, |object| {
            object.remove(keys::ORDERS);
        })
    }

    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        let metadata = run_metadata(run);
        self.log(format!("run_set {id} {metadata} {by}"))?;
        self.merged(id, metadata)
    }

    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("reopen {item} {by}"))?;
        self.moving(item, |held| held.status = Status::Open)
    }

    /// One log line and every move, which is what the real store's one call
    /// leaves: an arm counting the calls a retire makes counts this as one.
    /// No move is made while another seat holds the item, or while it reads a
    /// status other than the one the caller named — bd's `--if-status`, which
    /// is what keeps a closed item closed.
    fn order_withdraw_from(
        &self,
        id: &ItemId,
        seat: &SeatId,
        status: &Status,
        by: &Actor,
    ) -> Result<(), StoreError> {
        self.log(format!("order_withdraw_from {id} {seat} {status} {by}"))?;
        self.held_by(id, &seat.to_string())?;
        self.status_is(id, status.as_str())?;
        self.moving(id, |held| {
            held.assignee = None;
            held.status = Status::Open;
        })?;
        self.metadata_write(id, |object| {
            object.remove(keys::ORDERS);
        })
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
            return Err(StoreError::Refused(format!("{hold} is not here")));
        }
        let mut cleared = self.cleared.lock().expect("the holds are not poisoned");
        if cleared.iter().any(|held| *hold == *held) {
            return Err(already_cleared(hold));
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
    fn close(&self, id: &ItemId, reason: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("close {id} {reason} {by}"))?;
        let closed = self
            .items
            .lock()
            .expect("the items are not poisoned")
            .get(id.as_str())
            .is_some_and(|held| held.status == Status::Closed);
        if closed {
            return Err(already_closed(id));
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
            return Err(StoreError::Refused(format!("{item} is not here")));
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
            return Err(StoreError::Refused(format!("{item} is not here")));
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
    /// [`FakeStore::no_export`] is set.
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        Ok(Capabilities {
            export: (!self.no_export).then(|| ExportSpec {
                file: EXPORT_FILE.to_string(),
                dir: EXPORT_DIR.to_string(),
            }),
            scratch: true,
            item_prefix: None,
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
    /// An item's line carries its comments, which is where its entries live —
    /// measured on bd 1.3.0, whose export writes them under `comments` with the
    /// fields a comment read answers — so an append moves the export's bytes as
    /// it moves the real one's.
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
                row["comments"] = held
                    .iter()
                    .map(|comment| {
                        serde_json::json!({
                            "id": comment.id,
                            "issue_id": id,
                            "author": comment.author,
                            "text": comment.text,
                            "created_at": comment.at,
                        })
                    })
                    .collect();
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

impl FakeStore {
    /// One metadata object merged at the top level, which is every metadata
    /// write's polarity.
    fn merged(&self, item: &str, metadata: serde_json::Value) -> Result<(), StoreError> {
        let serde_json::Value::Object(written) = metadata else {
            return Err(StoreError::Unreadable(format!(
                "the payload is not one JSON object: {metadata}"
            )));
        };
        self.metadata_write(item, |object| {
            for (key, value) in written {
                object.insert(key, value);
            }
        })
    }
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
    fn order_withdraw(&self, id: &ItemId, by: &Actor) -> Result<(), StoreError> {
        (**self).order_withdraw(id, by)
    }
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        (**self).run_set(id, run, by)
    }
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        (**self).hand_over(item, from, to, by)
    }
    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        (**self).reopen(item, by)
    }
    fn order_withdraw_from(
        &self,
        id: &ItemId,
        seat: &SeatId,
        status: &Status,
        by: &Actor,
    ) -> Result<(), StoreError> {
        (**self).order_withdraw_from(id, seat, status, by)
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
    fn close(&self, id: &ItemId, reason: &str, by: &str) -> Result<(), StoreError> {
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

    /// A metadata object merged onto the item as ANOTHER WRITER leaves one —
    /// [`FakeStore::plant_metadata`].
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
