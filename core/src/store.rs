//! The work graph, reached through the `bd` binary and no other way.
//!
//! The trait is what the verbs are written against, so a suite can force a
//! read-back that disagrees with the write beside it — the one failure a real
//! store will not produce on demand and the one the verbs must survive.
//!
//! Every read decodes the FIRST JSON value of the answer and ignores what
//! trails it: `bd show --json` answers one top-level value and closes with a
//! newline, and a decoder that demands the whole text be one value refuses a
//! well-formed answer over its last byte.
//!
//! Every `bd` call carries `BD_JSON_ENVELOPE=1`, so a JSON answer comes as
//! `{"schema_version": N, "data": …}` — the shape bd v2.0 makes the default —
//! and that first value is opened in ONE place, [`opened`], before anything
//! reads it. A bd that predates the envelope answers the bare value, and that
//! reads the same.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;

use crate::process::{deadline_cause, run_bounded};

mod bd_cli;
mod bd_wire;
pub mod keys;

/// The binary every write and read goes through when the caller names no
/// other, resolved on the process's own `PATH`.
pub const BD: &str = "bd";

/// The bd release this fleet is measured against: every "measured on" claim in
/// this file was taken on it, and the defaults' `bd-version` doctor check and
/// `fleet prime`'s second line compare `bd version` with it. A bd at another
/// version is named, with the line that installs this one, and the verbs still
/// run on it.
///
/// A pin move is THIS LINE PLUS THE RE-MEASURE: every claim here re-run on the
/// new release and restated, or its code changed where the behaviour moved.
/// The doctor check carries its own copy, because a shell script cannot read
/// this, and a suite arm fails until the two agree.
pub const PINNED_BD: &str = "1.3.0";

/// Where the store's export goes, relative to the project root. It is a passive
/// file the work graph regenerates, never a second copy anything reads back.
pub const EXPORT: &str = ".beads/issues.jsonl";

/// The newest `schema_version` this binary reads — the one bd 1.3.0 answers on
/// every JSON call, enveloped or not. A higher one is warned about once and
/// read anyway, which is beads' own advice to a consumer.
pub const SCHEMA_VERSION: u64 = 1;

/// The variable that opts a `bd` call into the envelope before v2.0 makes it
/// the default.
const ENVELOPE: &str = "BD_JSON_ENVELOPE";

/// Whether this process has already said the store answers a newer schema, so
/// a verb that makes a hundred reads says it once.
static WARNED: AtomicBool = AtomicBool::new(false);

/// One item as a read answers it.
///
/// The three fields a dispatch asserts are `Option` because the store OMITS a
/// key it has no value for: an item nobody has assigned carries no `assignee`
/// at all, and an absent field is a third answer that must not read as a
/// disagreement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    /// What a person calls this item. `land` writes it into the commit subject,
    /// so a trunk's log reads as a list of what was done and not of ids.
    pub title: String,
    pub status: String,
    pub assignee: Option<String>,
    pub notes: Option<String>,
    /// `metadata["fleet.orders"]` ([`keys::ORDERS`]), as the store holds it.
    pub orders: Option<Orders>,
    /// Whether `metadata` carried a `fleet.orders` key at all, which `orders`
    /// alone cannot say: a key holding something that is not an object at
    /// [`keys::VERSION`] is present and unreadable, and a withdrawal has to
    /// tell that from absent.
    pub has_orders_key: bool,
    /// The open items that block this one by a type bd's ready set honours —
    /// one of `BLOCKING` — by id.
    pub blockers: Vec<String>,
    /// The type, as the store spells it: the JSON key is `issue_type` and a
    /// rule matches on this value.
    pub item_type: String,
    /// The item's OWN labels and no parent's, which is what the store answers.
    pub labels: Vec<String>,
    /// `metadata["fleet.run"]` ([`keys::RUN`]), as free JSON and as the store
    /// holds it, for a run's record item — its version is the reader's to
    /// check, through [`keys::versioned`]. A top-level key of its own, which is
    /// what lets bd's top-level merge leave the item's other keys standing.
    pub run: Option<serde_json::Value>,
    /// The decoded document, as text. The negative control reads this, so the
    /// control asks the SAME answer for a token nothing wrote.
    pub document: String,
}

/// A new item, as the arguments a `create` takes.
///
/// There is no id here: the store names what it files, which is why [`Store`]'s
/// `create` answers one.
pub struct NewItem<'a> {
    pub title: &'a str,
    pub description: &'a str,
    /// The store's own spelling — `task` for a run's record.
    pub item_type: &'a str,
    pub labels: &'a [&'a str],
}

/// The order index, read field by field rather than as free JSON: the read-back
/// compares four named values and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Orders {
    pub by: Option<String>,
    pub kind: Option<String>,
    pub seat: Option<String>,
    pub at: Option<String>,
}

/// One item assigned to a seat, as the seat's list answers it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssignedItem {
    pub id: String,
    /// What a person calls this item, read off this row — the words a
    /// session's start carries beside the id.
    pub title: String,
    pub status: String,
    /// Whether `metadata` carried a `fleet.orders` key, read off THIS ROW and
    /// not off a second call: the listing answers each row's metadata, so a
    /// caller asking which of a seat's items are ordered pays one call and not
    /// one per row. The same third answer [`Item::has_orders_key`] carries — a key
    /// holding something that is not an object at [`keys::VERSION`] is present
    /// and unreadable.
    pub has_orders_key: bool,
    /// The type, as the listing spells it (`issue_type`), read off this row.
    pub item_type: String,
}

/// The ways a store call ends badly, which are different exits: an item that
/// is not there, or not held by whom the write required, is the record's
/// answer, and a store that will not answer is no reading at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store answered, and its answer is that there is no such item.
    Missing(String),
    /// The store answered, and its answer is that the item is held by someone
    /// other than the holder a fenced write named — so NOTHING was written.
    Moved(String),
    /// The store could not be run, or did not answer something readable.
    Unreadable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Missing(why) => write!(f, "{why}"),
            StoreError::Moved(why) => write!(f, "{why}"),
            StoreError::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

pub trait Store {
    /// The items the store calls ready, by id: open and unblocked, in the
    /// store's own order.
    fn ready(&self) -> Result<Vec<String>, StoreError>;

    /// One item, which the argument may name by PART of its id: bd resolves a
    /// partial id itself — measured on 1.3.0, a whole id, then a whole hash,
    /// then a substring of one — and the answer's `id` is the full one. So a
    /// verb taking an item resolves it here once, at its entry, and acts on
    /// [`Item::id`] from then on and never on the typed text.
    fn show(&self, item: &str) -> Result<Item, StoreError>;

    /// The open items carrying this label, by id.
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError>;

    /// One item filed, answered as the id the store gave it.
    ///
    /// A run's record is titled by its own id, which nothing knows until
    /// this returns — so the title in [`NewItem`] is what the record carries
    /// until the caller retitles it, and the caller's read-back is what says
    /// the second write landed.
    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError>;

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError>;

    /// The item as a person reads it — the rendering a brief carries verbatim,
    /// so a seat and a person read the same text.
    fn show_text(&self, item: &str) -> Result<String, StoreError>;

    /// Every item the store holds against this seat.
    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError>;

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError>;

    /// The item handed from `from` to `to`, and only while `from` still holds
    /// it — `""` for an item nobody holds. Anyone else holding it is
    /// [`StoreError::Moved`], and nothing is written.
    ///
    /// For a write whose actor is NOT the holder. bd 1.3.0 refuses a plain
    /// `--assignee` from anyone but the holder on an `in_progress` item —
    /// measured: `cannot reassign X: held by "s1" (in_progress)` — and takes the
    /// same write when it names the holder with `--if-assignee`, which also
    /// writes nothing and exits 13 where the holder moved. The DEFAULT reads the
    /// holder and then assigns, for a store with no fence of its own.
    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        let held = self.show(item)?.assignee.unwrap_or_default();
        if held != from {
            return Err(moved(item, from, &held));
        }
        self.assign(item, to, by)
    }

    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError>;

    /// `metadata["fleet.orders"]`, written as one object that replaces the key
    /// whole.
    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError>;

    /// One metadata object written by the same call `set_orders` makes, for a
    /// top-level key that is not `fleet.orders`.
    ///
    /// It is the SIBLING of that method and not a generalisation of it: the
    /// write MERGES at the top level — measured on bd 1.3.0, where a second
    /// write of a different key kept the first — so a run's object never
    /// erases the order index beside it, and the two keys keep one writer each.
    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError>;

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError>;

    /// The assignee cleared and the order index unset in ONE call, and only
    /// while `seat` still holds the item: [`hand_over`](Store::hand_over)'s
    /// fence, because a retire's actor is never the seat it retires.
    ///
    /// The pair is what a withdrawal always writes together, and a retire pays
    /// it on every seat it ends — so the store that talks to `bd` sends one
    /// `update` rather than two, which is a call the verb does not make while
    /// another suite is queueing behind it. The DEFAULT is the two writes in
    /// order, so an implementation that has nothing to gain by folding them
    /// says nothing; what neither form may do is leave the assignee cleared
    /// with the index still set, which the caller's read-back is what catches.
    fn withdraw_order(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.hand_over(item, seat, "", by)?;
        self.unset_orders(item, by)
    }

    /// A hold raised on this item, answered as the hold's own id.
    ///
    /// The store's own object and not a question item this fleet owns: bd files
    /// it as a gate of type human, the held item leaves the ready set the moment
    /// it is created, and it comes back when somebody clears the hold. So a park
    /// needs nothing of fleet's beside the event.
    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError>;

    /// Every hold the store still calls open, by id.
    ///
    /// IDS AND NOT DOCUMENTS, and no item on them: the listing answers which
    /// hold this is and never which item it blocks — measured on bd 1.3.0,
    /// where the blocked item appears only inside the description's prose — so
    /// a caller that wants one item's hold reads that hold's id off the item's
    /// own park and asks this list whether it is still here.
    fn open_holds(&self) -> Result<Vec<String>, StoreError>;

    /// One hold cleared, which puts the item it blocked back in the ready set.
    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError>;

    /// The item closed, with the reason a reader gets instead of the act.
    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError>;

    /// The store's own export, written at [`EXPORT`] under the root the CALLER
    /// names.
    ///
    /// THE DESTINATION IS THE CALLER'S AND NOT THE STORE'S. One board is read
    /// from the checkout it was resolved in and committed from whichever tree
    /// the act runs in, and a landing resolves that tree for itself — so where
    /// the export has to land is a fact of the act and not of the store.
    ///
    /// It regenerates that one file and touches nothing else — measured on bd
    /// 1.3.0, where two exports left every other file under `.beads` as it was
    /// in bytes, and in mtime bar the embedded engine's journal, which a bare
    /// read touches the same way — so nothing is carried forward here to keep
    /// the export's polarity right.
    fn export(&self, into: &Path) -> Result<(), StoreError>;
}

/// The store as `bd` on this box, scoped to one project.
///
/// `-C <root>` on every call, so the store a verb writes to is the project's
/// whatever directory the call was made from.
pub struct Bd {
    root: PathBuf,
    bin: PathBuf,
    timeout: Duration,
}

/// The bound on one store call, fixed and not policy.
///
/// No legitimate store call comes close: every one is a local read or write
/// of one project's store. A call that outruns it is a store that is not
/// answering, and it is killed and read as Unreadable rather than left to hang
/// the verb. A landing's suite is not a store call and is not bounded here.
const STORE_TIMEOUT: Duration = Duration::from_secs(60);

impl Bd {
    pub fn at(root: &Path) -> Bd {
        Bd::at_bin(root, Path::new(BD))
    }

    /// The same store over a binary the CALLER resolved, run by that path and
    /// never searched for again.
    ///
    /// A process whose own `PATH` is not the one a person's shell has — a
    /// launchd service carries neither a package manager's prefix nor the
    /// user's local bin — can reach `bd` no other way.
    pub fn at_bin(root: &Path, bin: &Path) -> Bd {
        Bd {
            root: root.to_path_buf(),
            bin: bin.to_path_buf(),
            timeout: STORE_TIMEOUT,
        }
    }

    /// The same store under a shorter bound, for a caller that cannot wait the
    /// whole of `STORE_TIMEOUT` — a session-start hook is one.
    pub fn with_timeout(self, timeout: Duration) -> Bd {
        Bd { timeout, ..self }
    }

    /// One call, with its status read from the command itself, bounded by
    /// this store's timeout.
    ///
    /// The envelope is asked for on EVERY call and not only on the JSON reads:
    /// bd applies it to a `--json` answer alone — measured on 1.3.0, where the
    /// rendering, the export and a write's own line were byte-identical with
    /// it and without — so one setting here cannot leave a read out.
    ///
    /// A call that outruns the bound is killed with its whole process group.
    /// On a WRITE — told by its `--actor`, which every call that changes an
    /// item carries and no read does — the refusal also says what a kill
    /// cannot: whether the write landed before it.
    fn run(&self, args: &[&str]) -> Result<Output, StoreError> {
        let mut cmd = Command::new(&self.bin);
        cmd.env(ENVELOPE, "1").arg("-C").arg(&self.root).args(args);
        run_bounded(cmd, self.timeout).map_err(|why| {
            if why != deadline_cause(self.timeout) {
                return StoreError::Unreadable(format!(
                    "`{}` could not be run ({why}) — nothing was written",
                    self.bin.display()
                ));
            }
            let mut refusal = format!("{} {why}", self.named(args));
            if args.contains(&"--actor") {
                refusal.push_str(
                    " — the write's effect cannot be told, so the item must be read \
                     before anything is written to it again",
                );
            }
            StoreError::Unreadable(refusal)
        })
    }

    /// The call as a message names it: the binary this store runs and the argv
    /// it ran, so a refusal never names a binary or a flag the call did not.
    fn named(&self, args: &[&str]) -> String {
        let args: Vec<&str> = args
            .iter()
            .map(|arg| if arg.is_empty() { "''" } else { arg })
            .collect();
        format!("`{} {}`", self.bin.display(), args.join(" "))
    }

    fn refused(&self, args: &[&str], out: &Output) -> StoreError {
        StoreError::Unreadable(format!(
            "{} {}: {}",
            self.named(args),
            out.status,
            tail(out)
        ))
    }

    /// The JSON a call answered, opened out of its envelope.
    fn json(&self, args: &[&str], out: &Output) -> Option<serde_json::Value> {
        first_value(&String::from_utf8_lossy(&out.stdout))
            .map(|value| opened(value, || self.named(args)))
    }

    /// One call that has to succeed, its answer handed back whole.
    fn answered(&self, args: &[&str]) -> Result<Output, StoreError> {
        let out = self.run(args)?;
        if !out.status.success() {
            return Err(self.refused(args, &out));
        }
        Ok(out)
    }

    /// One listing, as its rows, each decoded into the wire type bd's spec
    /// names for that answer. A row that does not decode is a store that did
    /// not answer something readable.
    ///
    /// AN EMPTY LISTING MAY ANSWER `null` AND NOT `[]` — measured on bd 1.3.0
    /// for `gate list` — or nothing at all, and both read as no rows: a decoder
    /// demanding an array would read "nothing here" as a store that would not
    /// answer, and refuse every answer on a fleet with nothing held.
    fn listed<Row: serde::de::DeserializeOwned>(
        &self,
        args: &[&str],
    ) -> Result<Vec<Row>, StoreError> {
        let out = self.answered(args)?;
        let rows = match self.json(args, &out) {
            Some(rows @ serde_json::Value::Array(_)) => rows,
            Some(serde_json::Value::Null) => return Ok(Vec::new()),
            None if String::from_utf8_lossy(&out.stdout).trim().is_empty() => return Ok(Vec::new()),
            _ => {
                return Err(StoreError::Unreadable(format!(
                    "{} did not answer a list: {}",
                    self.named(args),
                    tail(&out)
                )))
            }
        };
        serde_json::from_value(rows).map_err(|why| {
            StoreError::Unreadable(format!(
                "{} answered a row bd's wire types do not read: {why}",
                self.named(args)
            ))
        })
    }

    /// The id off a write's OWN answer. A second read for the newest item would
    /// name whatever else landed in the store between the two calls.
    fn created_id(&self, args: &[&str]) -> Result<String, StoreError> {
        let out = self.answered(args)?;
        self.json(args, &out)
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(|id| id.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                StoreError::Unreadable(format!(
                    "{} answered no id: {}",
                    self.named(args),
                    tail(&out)
                ))
            })
    }

    fn wrote(&self, args: &[&str]) -> Result<(), StoreError> {
        self.answered(args).map(|_| ())
    }

    /// A write carrying `--if-assignee <holder>`, whose exit 13 is bd's word
    /// that the holder moved and nothing was written — measured on 1.3.0:
    /// `assignee mismatch: X is held by "s2", expected "s1"`, exit 13.
    fn fenced(&self, args: &[&str], item: &str, holder: &str) -> Result<(), StoreError> {
        let out = self.run(args)?;
        if out.status.code() == Some(FENCE_MISMATCH) {
            return Err(StoreError::Moved(format!(
                "{item} is not held by {} — nothing was written ({})",
                holder_named(holder),
                tail(&out)
            )));
        }
        if !out.status.success() {
            return Err(self.refused(args, &out));
        }
        Ok(())
    }
}

/// The exit bd gives a write whose `--if-assignee` no longer holds.
const FENCE_MISMATCH: i32 = 13;

/// The refusal a fenced hand-over answers when the item's holder is not the
/// one the write named.
pub(crate) fn moved(item: &str, expected: &str, held: &str) -> StoreError {
    StoreError::Moved(format!(
        "{item} is held by {} and not by {} — nothing was written",
        holder_named(held),
        holder_named(expected)
    ))
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
fn tail(out: &Output) -> String {
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
pub fn first_value(text: &str) -> Option<serde_json::Value> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>();
    stream.next().and_then(Result::ok)
}

/// The answer inside bd's JSON envelope, or the value itself where it carries
/// none.
///
/// A `schema_version` above [`SCHEMA_VERSION`] is WARNED ABOUT AND READ: a
/// newer bd adds keys far more often than it moves one, and a store that
/// refused every answer from it would stop the fleet over a field nothing here
/// reads. `from` names the call for that warning, and is asked only when one
/// is due.
pub fn opened(value: serde_json::Value, from: impl FnOnce() -> String) -> serde_json::Value {
    let serde_json::Value::Object(mut answer) = value else {
        return value;
    };
    let newer = answer
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .filter(|version| *version > SCHEMA_VERSION);
    if let Some(version) = newer {
        if !WARNED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "fleet: {} answered schema_version {version}, newer than the \
                 {SCHEMA_VERSION} this binary knows — reading it anyway",
                from()
            );
        }
    }
    // BOTH KEYS make an envelope. A bare error carries `schema_version` beside
    // `error` and no `data` — measured on 1.3.0 with the envelope off — and is
    // handed back whole for `show` to read.
    if answer.contains_key("schema_version") && answer.contains_key("data") {
        return answer.remove("data").unwrap_or_default();
    }
    serde_json::Value::Object(answer)
}

/// The row an opened `show` answer holds, or the store's word that there is
/// none. The one reading of that answer, which the fake store's `show` makes
/// too. `said` is what the call wrote on stderr.
///
/// An error CARRYING A CODE is classified by it: `not_found` is the record's
/// answer, and any other code is a store that did not answer. An error with NO
/// code is read by its key alone, as an item that is not there — measured on
/// bd 1.3.0, whose missing id answers an error, a hint and no code.
///
/// AN ARGUMENT NAMING MORE THAN ONE ITEM is the record's answer too, and is
/// told from one naming none by stderr alone: bd 1.3.0 answers both with the
/// same JSON error, and only its stderr says `ambiguous issue ID: … matches N
/// issues: [...]`. So the refusal names the matches bd listed, and a verb that
/// acts on one item never guesses which.
pub fn shown(
    item: &str,
    value: serde_json::Value,
    said: &str,
) -> Result<serde_json::Value, StoreError> {
    let Some(row) = sole(value) else {
        return Err(StoreError::Missing(format!("{item} is not in the store")));
    };
    if let Ok(bd_cli::CliError { error, code }) = bd_cli::CliError::deserialize(&row) {
        return match code.as_deref() {
            None | Some("not_found") => Err(StoreError::Missing(match ambiguous(said) {
                Some(matches) => format!(
                    "`{item}` matches more than one item — {matches} — and more of the id says \
                     which one this is"
                ),
                None => format!("{item}: {error}"),
            })),
            Some(code) => Err(StoreError::Unreadable(format!(
                "{item} could not be read ({code}): {error}"
            ))),
        };
    }
    Ok(row)
}

/// The words a stderr line names an ambiguity with: bd 1.3.0's, measured, and
/// bd 1.2.2's, which 1.3.0 moved — kept, so a bd off the pin still refuses an
/// ambiguous id by its matches rather than as an id that is not there.
const AMBIGUOUS: [&str; 2] = ["ambiguous issue ID", "ambiguous ID"];

/// The items bd named for an ambiguous id, off its stderr line — `Error
/// fetching a: ambiguous issue ID: "a" matches 7 issues: [fx-a64 fx-a82 …]`,
/// measured on 1.3.0 — joined for a refusal, or that whole line where it names
/// none in brackets. `None` where stderr says nothing of an ambiguity.
fn ambiguous(said: &str) -> Option<String> {
    let line = said
        .lines()
        .find(|line| AMBIGUOUS.iter().any(|words| line.contains(words)))?;
    let listed = line
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(ids, _)| ids.split_whitespace().collect::<Vec<_>>().join(", "))
        .filter(|ids| !ids.is_empty());
    Some(listed.unwrap_or_else(|| line.trim().to_string()))
}

/// The one element a `show` answers about. The answer is an array of one; an
/// object is the shape an error takes, and is handed back as it is so the
/// caller can read the error key out of it.
fn sole(value: serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Array(mut rows) => {
            if rows.is_empty() {
                None
            } else {
                Some(rows.remove(0))
            }
        }
        other => Some(other),
    }
}

/// The order index off a document's metadata, and whether the key was there at
/// all. The metadata is bd's raw JSON and not a typed map, so a `fleet.orders`
/// holding something that is not an object — or an object at a version this
/// binary does not know — still reads as present, and never as an order.
///
/// ONLY FLEET'S KEY. A bare `orders` is some other writer's, whatever shape it
/// holds, and an item carrying one and no `fleet.orders` reads as unordered.
fn orders_of(metadata: Option<&serde_json::Value>) -> (Option<Orders>, bool) {
    let Some(held) = metadata.and_then(|m| m.get(keys::ORDERS)) else {
        return (None, false);
    };
    if held.is_null() {
        return (None, false);
    }
    let Ok(table) = keys::versioned(keys::ORDERS, held) else {
        return (None, true);
    };
    let read = |key: &str| table.get(key).and_then(|v| v.as_str()).map(str::to_string);
    (
        Some(Orders {
            by: read("by"),
            kind: read("kind"),
            seat: read("seat"),
            at: read("at"),
        }),
        true,
    )
}

/// The dependency types bd's ready set honours as blocking, MEASURED on bd
/// 1.3.0 in a scratch board: one item per type, each depending on one open
/// item, then `bd ready --json -n 0`. These three took their item out of the
/// ready set, and a hold raised by `bd gate create --blocks` is a `blocks`
/// edge. `parent-child`, `related`, `discovered-from`, `replies-to`,
/// `relates-to`, `duplicates`, `supersedes`, `authored-by`, `assigned-to`,
/// `approved-by`, `attests`, `tracks`, `until`, `caused-by`, `validates` and
/// `delegated-from` left it ready, and `bd dep add` refuses a type it does not
/// know — a `parent-child` edge passes on a blocked parent's blockers, and is
/// not one itself.
const BLOCKING: [&str; 3] = ["blocks", "conditional-blocks", "waits-for"];

/// The dependencies that still stand between this item and a start: an entry
/// the store reports closed has been answered, and one of a type bd's ready set
/// does not honour never stood, so neither is a blocker.
fn blockers_of(entries: &[bd_wire::IssueWithDependencyMetadata]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| entry.status.as_deref() != Some("closed"))
        // An entry that names no type is kept, the same cautious reading as a
        // missing status.
        .filter(|entry| {
            entry
                .dependency_type
                .as_deref()
                .map(|kind| BLOCKING.contains(&kind))
                .unwrap_or(true)
        })
        .filter_map(|entry| entry.id.clone())
        .collect()
}

impl Store for Bd {
    fn ready(&self) -> Result<Vec<String>, StoreError> {
        // `-n 0` lifts the read's row cap. The verb answers its first 100 rows
        // by default, piped or not — measured on 1.3.0, 100 of 112 — and a
        // truncated list reads exactly like a whole one, so past a hundred
        // ready rows a ready item would be refused as not ready.
        Ok(self
            .listed::<bd_wire::IssueWithCounts>(&["ready", "--json", "-n", "0"])?
            .into_iter()
            .filter_map(|row| row.id)
            .collect())
    }

    fn show(&self, item: &str) -> Result<Item, StoreError> {
        // THE JSON IS READ BEFORE THE STATUS, and this read stays off
        // `answered`: bd exits non-zero on an item it does not hold and prints
        // the error object, which is the record's answer and not a refusal.
        let args = ["show", item, "--json"];
        let out = self.run(&args)?;
        let Some(value) = self.json(&args, &out) else {
            return Err(StoreError::Unreadable(format!(
                "{} answered no JSON: {}",
                self.named(&args),
                tail(&out)
            )));
        };
        let row = shown(item, value, &String::from_utf8_lossy(&out.stderr))?;
        if !out.status.success() {
            return Err(self.refused(&args, &out));
        }
        item_from(item, &row)
    }

    /// `-q`, which the JSON reads do not need and this one does: the human
    /// rendering carries a one-off tip on a store's FIRST read — measured on
    /// 1.3.0, present on call one and absent on every call after — and that
    /// line both names a provider and makes the same item render two different
    /// ways. Quiet drops it and leaves the body byte-identical.
    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        let out = self.answered(&["-q", "show", item])?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// `-n 0` for the same reason the ready read carries it: this answer takes
    /// a cap — measured on 1.3.0, none by default on a piped call (110 of 110)
    /// and 50 on a terminal (20 in bd's agent mode), but a board's `list.limit`
    /// binds a piped one too (5 of 110 with it set) — and a truncated list
    /// reads exactly like a whole one, so past the cap a run would be started
    /// past the `[core.run] max_open` cap it is measured against.
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        Ok(self
            .listed::<bd_wire::IssueWithCounts>(&[
                "list", "--label", label, "--status", "open", "--json", "-n", "0",
            ])?
            .into_iter()
            .filter_map(|row| row.id)
            .collect())
    }

    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError> {
        let labels = item.labels.join(",");
        let mut args = vec![
            "create",
            "--title",
            item.title,
            "--description",
            item.description,
            "--type",
            item.item_type,
        ];
        if !labels.is_empty() {
            args.extend(["--labels", labels.as_str()]);
        }
        args.extend(["--actor", by, "--json"]);
        self.created_id(&args)
    }

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["update", item, "--title", title, "--actor", by])
    }

    /// `-n 0` for the same reason the reads above carry it: this answer takes
    /// the same cap as `open_labelled`'s, and a truncated list reads exactly
    /// like a whole one, so past the cap a retire misses the orders beyond it
    /// and the next seat of that name inherits them.
    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError> {
        Ok(self
            .listed::<bd_wire::IssueWithCounts>(&["list", "-a", seat, "--json", "-n", "0"])?
            .into_iter()
            .filter_map(|row| {
                Some(AssignedItem {
                    has_orders_key: orders_of(row.metadata.as_ref()).1,
                    id: row.id?,
                    title: row.title.unwrap_or_default(),
                    status: row.status.unwrap_or_default(),
                    item_type: row.issue_type.unwrap_or_default(),
                })
            })
            .collect())
    }

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["update", item, "--assignee", seat, "--actor", by])
    }

    fn hand_over(&self, item: &str, from: &str, to: &str, by: &str) -> Result<(), StoreError> {
        self.fenced(
            &[
                "update",
                item,
                "--if-assignee",
                from,
                "--assignee",
                to,
                "--actor",
                by,
            ],
            item,
            from,
        )
    }

    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["note", item, text, "--actor", by])
    }

    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["update", item, "--metadata", payload, "--actor", by])
    }

    /// The same argv `set_orders` uses: bd names one flag for a metadata write
    /// and the polarity above is what makes one flag safe for two keys.
    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["update", item, "--metadata", payload, "--actor", by])
    }

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&[
            "update",
            item,
            "--unset-metadata",
            keys::ORDERS,
            "--actor",
            by,
        ])
    }

    /// Both flags on one `update`, which bd takes: the empty assignee is what
    /// clears the field, measured on 1.3.0 — the one call left no `assignee`
    /// and no `fleet.orders`, and the `fleet.run` key beside it standing.
    /// `--if-assignee` names the retiring seat, which is what bd 1.3.0 takes
    /// from a retirer on an item that seat marked `in_progress` — measured,
    /// where the same call without it is refused.
    fn withdraw_order(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.fenced(
            &[
                "update",
                item,
                "--if-assignee",
                seat,
                "--assignee",
                "",
                "--unset-metadata",
                keys::ORDERS,
                "--actor",
                by,
            ],
            item,
            seat,
        )
    }

    /// `--type` is not passed: human is the type `bd gate create` takes with no
    /// flag, measured on 1.3.0, and a verb that spelled the default would be a
    /// second copy of it.
    ///
    /// THE ID COMES OFF THE STRUCTURED ANSWER and never off the printed line.
    /// `--actor` and `--json` are global flags here, so both are available on
    /// this subcommand; a parse of the prose is the form that goes quiet the
    /// day the prose changes.
    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        self.created_id(&[
            "gate", "create", "--blocks", item, "--reason", reason, "--actor", by, "--json",
        ])
    }

    /// `-n 0` for the same reason the three reads above carry it: this verb
    /// answers its first 50 rows by default, piped or not — measured on 1.3.0,
    /// 50 of 53 — and a truncated list reads exactly like a whole one, so past
    /// fifty open holds a hold the board keeps open is absent from the listing
    /// — and `clear` refuses a hold it does not find there as one somebody has
    /// already cleared.
    fn open_holds(&self) -> Result<Vec<String>, StoreError> {
        Ok(self
            .listed::<bd_wire::Issue>(&["gate", "list", "--json", "-n", "0"])?
            .into_iter()
            .filter_map(|row| row.id)
            .collect())
    }

    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["gate", "resolve", hold, "--actor", by])
    }

    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["close", item, "--reason", reason, "--actor", by])
    }

    /// `-o` is resolved against the CALLER's directory and not against `-C`:
    /// measured on 1.3.0, `bd -C <root> export -o .beads/issues.jsonl` run
    /// from elsewhere wrote `.beads/issues.jsonl` under the caller and left the
    /// root's `.beads` without one. So the path handed over is absolute.
    fn export(&self, into: &Path) -> Result<(), StoreError> {
        let into = into.join(EXPORT);
        // THE DIRECTORY IS MADE FIRST. `bd` writes the export through a temp
        // file beside it and makes no directory of its own, and a root whose
        // project keeps its store out of git has none until something does —
        // which is every fresh worktree, the landing lane among them.
        if let Some(dir) = into.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                StoreError::Unreadable(format!(
                    "the store's own directory {} could not be made: {e}",
                    dir.display()
                ))
            })?;
        }
        let into = into.to_string_lossy().into_owned();
        self.wrote(&["export", "-o", &into])
    }
}

/// One document, read into the fields a verb asserts on, through bd's own wire
/// type. A row that does not decode into it is a store that did not answer
/// something readable.
pub fn item_from(id: &str, row: &serde_json::Value) -> Result<Item, StoreError> {
    let wire = bd_wire::IssueDetails::deserialize(row).map_err(|why| {
        StoreError::Unreadable(format!(
            "{id} answered a row bd's wire types do not read: {why}"
        ))
    })?;
    let (orders, has_orders_key) = orders_of(wire.metadata.as_ref());
    Ok(Item {
        item_type: wire.issue_type.unwrap_or_default(),
        // The item's own labels, absent when the key is absent — which the
        // store spells as `null` and not as an empty array.
        labels: wire.labels.unwrap_or_default(),
        // The same read `orders_of` makes, one key over: absent when the key
        // is absent, so a run that wrote nothing is told from one that wrote
        // an empty object. A bare `run` is some other writer's and reads as
        // no run at all.
        run: wire
            .metadata
            .as_ref()
            .and_then(|m| m.get(keys::RUN))
            .filter(|held| !held.is_null())
            .cloned(),
        id: wire.id.unwrap_or_else(|| id.to_string()),
        title: wire.title.unwrap_or_default(),
        status: wire.status.unwrap_or_default(),
        assignee: wire.assignee,
        notes: wire.notes,
        orders,
        has_orders_key,
        blockers: blockers_of(wire.dependencies.as_deref().unwrap_or_default()),
        document: row.to_string(),
    })
}
