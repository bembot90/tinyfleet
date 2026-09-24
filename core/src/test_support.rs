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

use crate::store::{keys, AssignedItem, Item, NewItem, Orders, Store, StoreError};

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
/// Every read is decoded by [`crate::store::item_from`], the same function
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
    pub text: Mutex<BTreeMap<String, String>>,
    /// Rows answered for a seat on top of the items assigned to it here.
    pub held: BTreeMap<String, Vec<AssignedItem>>,
    pub writes: Mutex<Vec<String>>,
    /// The holds raised, in the order they were raised: the item and the
    /// question. The nth hold's id is `hold-<n>`, so a note naming one and the
    /// call that raised it can be compared.
    pub holds: Mutex<Vec<(String, String)>>,
    /// The holds a `clear_hold` has closed, by id, so the open listing below
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
    /// While set, a write is recorded and NOT applied — the disagreement a real
    /// store will not produce on demand. Off until an arm asks for it, through
    /// [`FakeStore::ignore_writes`] and never by hand.
    pub deaf: Mutex<bool>,
    /// How many items this store has filed, which names the ones it filed
    /// itself and stamps them in filing order.
    pub filed: AtomicUsize,
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
            .insert(item.id.clone(), item);
    }

    /// What `show_text` answers for this item.
    pub fn set_text(&self, item: &str, text: &str) {
        self.text
            .lock()
            .expect("the text is not poisoned")
            .insert(item.to_string(), text.to_string());
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
            .ok_or_else(|| StoreError::Missing(format!("{item} is not here")))?;
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
            .ok_or_else(|| StoreError::Missing(format!("{item} is not here")))?;
        let now = held.assignee.as_deref().unwrap_or_default();
        if now != holder {
            return Err(crate::store::moved(item, holder, now));
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
            .ok_or_else(|| StoreError::Missing(format!("{item} is not here")))?;
        if held.status != status {
            return Err(crate::store::restatused(item, status, &held.status));
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
            .get(&item.id)
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
                .ok_or_else(|| StoreError::Missing(format!("{item} is not here")))?;
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
}

/// The metadata a seeded item carries in its own fields, as the object a read
/// decodes from, under fleet's own keys. `has_orders_key` with no orders is a
/// key holding something that is not an object, which is a third answer and
/// not an absence. A seeded run is written as the store would hold it, so a
/// seed that wants one readable carries its own version.
fn seeded_metadata(item: &Item) -> serde_json::Map<String, serde_json::Value> {
    let mut object = serde_json::Map::new();
    if let Some(run) = &item.run {
        object.insert(String::from(keys::RUN), run.clone());
    }
    match (&item.orders, item.has_orders_key) {
        (Some(orders), _) => {
            object.insert(String::from(keys::ORDERS), orders_json(orders));
        }
        (None, true) => {
            object.insert(
                String::from(keys::ORDERS),
                serde_json::Value::String(String::new()),
            );
        }
        (None, false) => {}
    }
    object
}

fn orders_json(orders: &Orders) -> serde_json::Value {
    let mut index = serde_json::Map::new();
    for (key, held) in [
        ("by", &orders.by),
        ("kind", &orders.kind),
        ("seat", &orders.seat),
        ("at", &orders.at),
    ] {
        if let Some(value) = held {
            index.insert(String::from(key), serde_json::Value::String(value.clone()));
        }
    }
    index.insert(String::from(keys::VERSION_FIELD), keys::VERSION.into());
    serde_json::Value::Object(index)
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
    row.insert(String::from("id"), item.id.clone().into());
    row.insert(String::from("title"), item.title.clone().into());
    row.insert(String::from("status"), item.status.clone().into());
    row.insert(String::from("issue_type"), item.item_type.clone().into());
    if let Some(assignee) = &item.assignee {
        row.insert(String::from("assignee"), assignee.clone().into());
    }
    if let Some(notes) = &item.notes {
        row.insert(String::from("notes"), notes.clone().into());
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
/// `fx-7cx.1` beside it). Else every id whose hash HOLDS the argument is named,
/// with a leading prefix taken off the argument first (`fx-7c` names what `7c`
/// does, and `x-7c` names nothing). One id is the item, more than one an
/// ambiguity, none a missing id.
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
        hash.contains(needle(prefix))
    })
    .cloned()
    .collect()
}

impl Store for FakeStore {
    /// The seeded ids, plus every item this store holds that it calls ready:
    /// open, with no dependency standing and no hold raised against it. A
    /// store computes its own ready set and never holds a second copy of it.
    ///
    /// OPEN AND NOT MERELY UNCLOSED: `bd ready` leaves an `in_progress` item
    /// out, measured on 1.3.0, and a fake that listed one would pass an arm
    /// asserting a withdrawal put its item back in the ready set.
    fn ready(&self) -> Result<Vec<String>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let mut ids = self.ready.clone();
        let on_hold = self.on_hold();
        for item in self
            .items
            .lock()
            .expect("the items are not poisoned")
            .values()
        {
            let open = item.status == "open";
            let free = item.blockers.is_empty() && !on_hold.contains(&item.id);
            if open && free && !ids.contains(&item.id) {
                ids.push(item.id.clone());
            }
        }
        Ok(ids)
    }

    /// The seeded list, plus every open item in this store carrying the label —
    /// so an item labelled through a write is found by the same read.
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let mut found = self.labelled.get(label).cloned().unwrap_or_default();
        for item in self
            .items
            .lock()
            .expect("the items are not poisoned")
            .values()
        {
            let carries = item.labels.iter().any(|held| held == label);
            if carries && item.status != "closed" && !found.contains(&item.id) {
                found.push(item.id.clone());
            }
        }
        Ok(found)
    }

    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError> {
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
            return Ok(id);
        }
        self.items
            .lock()
            .expect("the items are not poisoned")
            .insert(
                id.clone(),
                Item {
                    id: id.clone(),
                    title: item.title.to_string(),
                    status: String::from("open"),
                    item_type: item.item_type.to_string(),
                    labels: item.labels.iter().map(|l| l.to_string()).collect(),
                    ..Item::default()
                },
            );
        self.metadata
            .lock()
            .expect("the metadata is not poisoned")
            .remove(&id);
        Ok(id)
    }

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("set_title {item} {title} {by}"))?;
        self.moving(item, |held| held.title = title.to_string())
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
    /// the matches on stderr alone. Only this read resolves; every write here
    /// takes a whole id, so a verb that wrote under the text it was typed is a
    /// refusal on this board and never a write that quietly landed.
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
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
            "schema_version": crate::store::SCHEMA_VERSION,
            "data": data,
        });
        let opened = crate::store::opened(answer, || String::from("the board held in memory"));
        crate::store::item_from(item, &crate::store::shown(item, opened, &said)?)
    }

    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        Ok(self
            .text
            .lock()
            .expect("the text is not poisoned")
            .get(item)
            .cloned()
            .unwrap_or_default())
    }

    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let mut rows = self.held.get(seat).cloned().unwrap_or_default();
        for item in self
            .items
            .lock()
            .expect("the items are not poisoned")
            .values()
        {
            let mine = item.assignee.as_deref() == Some(seat);
            if mine && !rows.iter().any(|row| row.id == item.id) {
                rows.push(AssignedItem {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    status: item.status.clone(),
                    // Off the METADATA this store would answer a read with, and
                    // not off the seeded field, so a row whose orders key a
                    // write has removed answers here as the real listing does.
                    has_orders_key: self
                        .metadata_of(item)
                        .get(keys::ORDERS)
                        .is_some_and(|held| !held.is_null()),
                    item_type: item.item_type.clone(),
                });
            }
        }
        Ok(rows)
    }

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("assign {item} {seat} {by}"))?;
        self.moving(item, |held| held.assignee = Some(seat.to_string()))
    }

    /// One log line, and the assignment only while `from` holds the item.
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("hand_over {item} {from} {to} {by}"))?;
        self.held_by(item, from)?;
        self.moving(item, |held| held.assignee = Some(to.to_string()))
    }

    /// Appended with a newline between notes and no header, which is how the
    /// real store answers two notes on one item.
    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("note {item} {text} {by}"))?;
        self.moving(item, |held| {
            held.notes = Some(match held.notes.take() {
                Some(already) if !already.is_empty() => format!("{already}\n{text}"),
                _ => text.to_string(),
            });
        })
    }

    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("set_orders {item} {payload} {by}"))?;
        self.merged(item, payload)
    }

    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("set_metadata {item} {payload} {by}"))?;
        self.merged(item, payload)
    }

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("unset_orders {item} {by}"))?;
        self.metadata_write(item, |object| {
            object.remove(keys::ORDERS);
        })
    }

    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("reopen {item} {by}"))?;
        self.moving(item, |held| held.status = String::from("open"))
    }

    /// One log line and every move, which is what the real store's one call
    /// leaves: an arm counting the calls a retire makes counts this as one.
    /// No move is made while another seat holds the item, or while it reads a
    /// status other than the one the caller named — bd's `--if-status`, which
    /// is what keeps a closed item closed.
    fn withdraw_order(
        &self,
        item: &str,
        seat: &str,
        status: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        self.log(format!("withdraw_order {item} {by}"))?;
        self.held_by(item, seat)?;
        self.status_is(item, status)?;
        self.moving(item, |held| {
            held.assignee = Some(String::new());
            held.status = String::from("open");
        })?;
        self.metadata_write(item, |object| {
            object.remove(keys::ORDERS);
        })
    }

    /// The hold recorded and answered as an id derived from the call's own
    /// order: an arm asserting that a park named the hold it raised needs the
    /// two to agree, and a constant id would agree with a second hold too.
    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        self.log(format!("hold {item} {reason} {by}"))?;
        let mut raised = self.holds.lock().expect("the holds are not poisoned");
        raised.push((item.to_string(), reason.to_string()));
        Ok(format!("hold-{}", raised.len()))
    }

    /// Every hold this store has raised and not been told to clear.
    fn open_holds(&self) -> Result<Vec<String>, StoreError> {
        if let Some(refused) = self.refuse() {
            return refused;
        }
        let closed = self.cleared.lock().expect("the holds are not poisoned");
        let raised = self.holds.lock().expect("the holds are not poisoned");
        Ok((1..=raised.len())
            .map(|n| format!("hold-{n}"))
            .filter(|id| !closed.contains(id))
            .collect())
    }

    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("clear_hold {hold} {by}"))?;
        if self.deaf() {
            return Ok(());
        }
        self.cleared
            .lock()
            .expect("the holds are not poisoned")
            .push(hold.to_string());
        Ok(())
    }

    /// The reason goes to the log and not to the notes: the real store holds it
    /// in a field of its own that no read here answers.
    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        self.log(format!("close {item} {reason} {by}"))?;
        self.moving(item, |held| held.status = String::from("closed"))?;
        if !self.deaf() {
            self.closed
                .lock()
                .expect("the reasons are not poisoned")
                .insert(item.to_string(), reason.to_string());
        }
        Ok(())
    }

    /// One JSON object per line, under the root the CALLER names. A store that
    /// was given no root of its own logs the word and writes nothing: a fake
    /// nobody rooted is one whose arms are about the board and not the file.
    fn export(&self, into: &Path) -> Result<(), StoreError> {
        self.log(String::from("export"))?;
        if self.root.is_none() {
            return Ok(());
        }
        let into = into.join(crate::store::EXPORT);
        if let Some(dir) = into.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                StoreError::Unreadable(format!(
                    "the store's own directory {} could not be made: {e}",
                    dir.display()
                ))
            })?;
        }
        let mut body = String::new();
        for row in self.rows() {
            body.push_str(&row.to_string());
            body.push('\n');
        }
        std::fs::write(&into, body).map_err(|e| {
            StoreError::Unreadable(format!(
                "the export {} was not written: {e}",
                into.display()
            ))
        })
    }
}

impl FakeStore {
    /// One metadata payload merged at the top level, which is both metadata
    /// writes' polarity.
    fn merged(&self, item: &str, payload: &str) -> Result<(), StoreError> {
        let Some(serde_json::Value::Object(written)) = crate::store::first_value(payload) else {
            return Err(StoreError::Unreadable(format!(
                "the payload is not one JSON object: {payload}"
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
    fn ready(&self) -> Result<Vec<String>, StoreError> {
        (**self).ready()
    }
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        (**self).show(item)
    }
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        (**self).open_labelled(label)
    }
    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError> {
        (**self).create(item, by)
    }
    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError> {
        (**self).set_title(item, title, by)
    }
    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        (**self).show_text(item)
    }
    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError> {
        (**self).assigned_to(seat)
    }
    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        (**self).assign(item, seat, by)
    }
    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError> {
        (**self).note(item, text, by)
    }
    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        (**self).set_orders(item, payload, by)
    }
    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        (**self).set_metadata(item, payload, by)
    }
    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError> {
        (**self).unset_orders(item, by)
    }
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        (**self).hand_over(item, from, to, by)
    }
    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        (**self).reopen(item, by)
    }
    fn withdraw_order(
        &self,
        item: &str,
        seat: &str,
        status: &str,
        by: &str,
    ) -> Result<(), StoreError> {
        (**self).withdraw_order(item, seat, status, by)
    }
    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        (**self).hold(item, reason, by)
    }
    fn open_holds(&self) -> Result<Vec<String>, StoreError> {
        (**self).open_holds()
    }
    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError> {
        (**self).clear_hold(hold, by)
    }
    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        (**self).close(item, reason, by)
    }
    fn export(&self, into: &Path) -> Result<(), StoreError> {
        (**self).export(into)
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
        let id = self
            .store
            .create(
                &NewItem {
                    title,
                    description: "a board item",
                    item_type: "task",
                    labels: &[],
                },
                "the-test",
            )
            .expect("the item is filed");
        self.store.set_text(&id, &format!("{id} · {title}\nOPEN\n"));
        id
    }

    /// The whole document, as text: what an arm compares before and after.
    pub fn json(&self, item: &str) -> String {
        self.store
            .show(item)
            .unwrap_or_else(|e| panic!("show {item}: {e}"))
            .document
    }

    /// The writes a rig makes for its own setup, as the store's own calls: a
    /// rig is putting an item in a state, not asserting on the write.
    pub fn assign(&self, item: &str, seat: &str) {
        self.store
            .assign(item, seat, "the-test")
            .unwrap_or_else(|e| panic!("assign {item}: {e}"));
    }

    pub fn note(&self, item: &str, text: &str, by: &str) {
        self.store
            .note(item, text, by)
            .unwrap_or_else(|e| panic!("note {item}: {e}"));
    }

    pub fn set_metadata(&self, item: &str, payload: &str) {
        self.store
            .set_metadata(item, payload, "the-test")
            .unwrap_or_else(|e| panic!("set_metadata {item}: {e}"));
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
        self.amend(item, |held| held.blockers.push(blocker.to_string()));
    }

    pub fn status(&self, item: &str, status: &str) {
        self.amend(item, |held| held.status = status.to_string());
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
