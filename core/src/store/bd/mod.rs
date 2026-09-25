//! The store as the `bd` binary answers it, and everything that knows the
//! store is `bd`: the argv each call sends, the envelope each JSON answer is
//! opened out of, the quirks measured on the pinned release, the decoder that
//! reads a row into an [`Item`], the export file and the store's own
//! directory, and the wire types generated from beads' own spec.
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

use super::types::{Capabilities, ExportSpec};
use super::{
    already_cleared, already_closed, first_value, holder_named, tail, unchanged, validated, Filter,
    HoldId, Item, ItemId, ItemSummary, NewItem, Order, OrderState, ReadProof, RunRecord, Status,
    Store, StoreError, Update, Version, STORE_TIMEOUT,
};
use crate::entry::{self, Body, Entry};
use crate::process::{deadline_cause, run_bounded};
use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;

mod bd_cli;
mod bd_wire;
pub mod keys;

/// The binary every write and read goes through when the caller names no
/// other, resolved on the process's own `PATH`.
pub const BD: &str = "bd";

/// The bd release this fleet is measured against: every "measured on" claim in
/// this file was taken on it, and the defaults' `bd-version` doctor check
/// compares `bd version` with it. A bd at another version is named, pointed at
/// beads' installation page for this one, and the verbs still run on it.
///
/// A pin move is THIS LINE PLUS THE RE-MEASURE: every claim here re-run on the
/// new release and restated, or its code changed where the behaviour moved.
/// The doctor check carries its own copy, because a shell script cannot read
/// this, and a suite arm fails until the two agree.
pub const PINNED_BD: &str = "1.3.0";

/// Where the store's export goes, relative to the project root. It is a passive
/// file the work graph regenerates, never a second copy anything reads back.
pub const EXPORT: &str = ".beads/issues.jsonl";

/// The store's own directory, relative to the project root, which [`EXPORT`]
/// sits in: every path under it is the store's bookkeeping and not a
/// delivery's, and a landing's staged-set check does not judge it.
pub const DIR: &str = ".beads/";

/// The store's own config, relative to the project root: where the prefix its
/// ids carry is named, when a project names one.
pub const CONFIG: &str = ".beads/config.yaml";

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

/// The store as `bd` on this box, scoped to one project.
///
/// `-C <root>` on every call, so the store a verb writes to is the project's
/// whatever directory the call was made from.
pub struct Bd {
    root: PathBuf,
    bin: PathBuf,
    timeout: Duration,
}

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
            Some(serde_json::Value::Array(rows)) => rows,
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
        rows.iter()
            .enumerate()
            .map(|(at, row)| {
                decoded(row).map_err(|why| {
                    StoreError::Unreadable(format!(
                        "{} answered a row bd's wire types do not read, row {at}: {why}",
                        self.named(args)
                    ))
                })
            })
            .collect()
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

    /// A write carrying `--if-assignee <holder>`, and `--if-status` beside it
    /// where `fence` names a status too, whose exit 13 is bd's word that the
    /// item moved and nothing was written — measured on 1.3.0: `assignee
    /// mismatch: X is held by "s2", expected "s1"` and `status mismatch: X has
    /// status "closed", expected "in_progress"`, both exit 13.
    fn fenced(&self, args: &[&str], item: &str, fence: &str) -> Result<(), StoreError> {
        let out = self.run(args)?;
        if out.status.code() == Some(FENCE_MISMATCH) {
            return Err(StoreError::Moved(format!(
                "{item} is not {fence} — nothing was written ({})",
                tail(&out)
            )));
        }
        if !out.status.success() {
            return Err(self.refused(args, &out));
        }
        Ok(())
    }
}

impl Bd {
    /// The row `show` answered for the argument, before anything is decoded
    /// out of it.
    ///
    /// THE JSON IS READ BEFORE THE STATUS, and this read stays off `answered`:
    /// bd exits non-zero on an item it does not hold and prints the error
    /// object, which is the record's answer and not a refusal.
    fn shown_row(&self, item: &str) -> Result<serde_json::Value, StoreError> {
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
        Ok(row)
    }
}

/// The exit bd gives a write whose `--if-assignee` no longer holds.
const FENCE_MISMATCH: i32 = 13;

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
        return Err(StoreError::Refused(format!("{item} is not in the store")));
    };
    if let Ok(bd_cli::CliError { error, code }) = bd_cli::CliError::deserialize(&row) {
        return match code.as_deref() {
            None | Some("not_found") => Err(StoreError::Refused(match ambiguous(said) {
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

/// The order index off a row's metadata, as one of its three answers.
///
/// Absent or `null` is no order. A key that is there and is not an object at
/// [`keys::VERSION`] whose fields read as an [`Order`] — a partly filled index,
/// one at a version this binary does not know, one whose `at` is no stamp — is
/// [`OrderState::Unreadable`]: present, and never guessed at as an order.
///
/// ONLY FLEET'S KEY. A bare `orders` is some other writer's, whatever shape it
/// holds, and an item carrying one and no `fleet.orders` reads as unordered.
pub(crate) fn order_of(metadata: Option<&serde_json::Value>) -> OrderState {
    let Some(held) = metadata.and_then(|m| m.get(keys::ORDERS)) else {
        return OrderState::None;
    };
    if held.is_null() {
        return OrderState::None;
    }
    let Ok(index) = keys::versioned(keys::ORDERS, held) else {
        return OrderState::Unreadable;
    };
    let mut fields = index.clone();
    fields.remove(keys::VERSION_FIELD);
    match serde_json::from_value::<Order>(serde_json::Value::Object(fields)) {
        Ok(order) => OrderState::Ordered(order),
        Err(_) => OrderState::Unreadable,
    }
}

/// A run's record off a row's metadata: absent or `null` is no record, and
/// one at [`keys::VERSION`] that reads as a [`RunRecord`] is the record.
///
/// ANYTHING ELSE REFUSES THE READ. A record this binary cannot read is a run
/// it cannot tell the state of, and the item is not answered as though it
/// carried none. The version comes off the object before the record is
/// decoded, because the writer stamps it in and the record's own fields are
/// the rest.
fn run_of(id: &str, metadata: Option<&serde_json::Value>) -> Result<Option<RunRecord>, StoreError> {
    let Some(held) = metadata
        .and_then(|m| m.get(keys::RUN))
        .filter(|held| !held.is_null())
    else {
        return Ok(None);
    };
    let refused = |why: Option<String>| {
        let version = held
            .get(keys::VERSION_FIELD)
            .map(serde_json::Value::to_string)
            .unwrap_or_else(|| String::from("none"));
        let mut said = format!(
            "{id}'s run record is not one this fleet reads ({}, {} {version})",
            keys::RUN,
            keys::VERSION_FIELD
        );
        if let Some(why) = why {
            said.push_str(&format!(" — {why}"));
        }
        StoreError::Unreadable(said)
    };
    let record = keys::versioned(keys::RUN, held).map_err(|_| refused(None))?;
    let mut fields = record.clone();
    fields.remove(keys::VERSION_FIELD);
    serde_json::from_value::<RunRecord>(serde_json::Value::Object(fields))
        .map(Some)
        .map_err(|why| refused(Some(why.to_string())))
}

/// The metadata an [`Order`] is written as: `{"fleet.orders": {…, "v": 1}}`,
/// the order's own fields with the version stamped in — the one shape
/// `order_of` reads back, built in the one place that writes it. The store
/// held in memory writes the same object, so the two cannot drift apart.
pub fn order_metadata(order: &Order) -> serde_json::Value {
    keys::stamped(keys::ORDERS, fields_of(order))
}

/// The metadata a [`RunRecord`] is written as: `{"fleet.run": {…, "v": 1}}`,
/// which `run_of` reads back.
pub fn run_metadata(run: &RunRecord) -> serde_json::Value {
    keys::stamped(keys::RUN, fields_of(run))
}

/// One of the contract's objects as its JSON fields. An order and a run's
/// record are structs of text, so neither serializes as anything else — and
/// were one ever to, the object written would carry its version and nothing
/// beside it, which reads back as an order or a record this fleet cannot
/// read, and every writer's read-back refuses that.
fn fields_of(value: &impl serde::Serialize) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::Object(fields)) => fields,
        _ => serde_json::Map::new(),
    }
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
fn blockers_of(entries: &[bd_wire::IssueWithDependencyMetadata]) -> Vec<ItemId> {
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
        .filter_map(|entry| entry.id.clone().map(ItemId::from))
        .collect()
}

impl Store for Bd {
    /// bd resolves a partial id itself — measured on 1.3.0, a whole id, then a
    /// whole hash, then a substring of one — and the answer's `id` is the full
    /// one.
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        item_from(item, &self.shown_row(item)?)
    }

    /// The show call, answering only the row's `id`: nothing else of the row
    /// is decoded, so an item whose run record this fleet does not read still
    /// resolves.
    fn resolve(&self, id: &str) -> Result<ItemId, StoreError> {
        let row = self.shown_row(id)?;
        row.get("id")
            .and_then(serde_json::Value::as_str)
            .map(ItemId::from)
            .ok_or_else(|| {
                StoreError::Unreadable(format!(
                    "{} answered a row naming no id",
                    self.named(&["show", id, "--json"])
                ))
            })
    }

    /// One listing per filter, and every one carries `-n 0`, which lifts the
    /// verb's row cap: a truncated list reads exactly like a whole one.
    ///
    /// `ready` answers its first 100 rows by default, piped or not — measured
    /// on 1.3.0, 100 of 112 — so past a hundred ready rows a ready item would
    /// be refused as not ready. `list` takes a cap too — measured on 1.3.0,
    /// none by default on a piped call (110 of 110) and 50 on a terminal (20
    /// in bd's agent mode), but a board's `list.limit` binds a piped one as
    /// well (5 of 110 with it set) — so past it a run would be started past
    /// the `[core.run] max_open` cap it is measured against, and a retire
    /// would miss the orders beyond it, which the next seat of that name
    /// inherits.
    ///
    /// A seat's items are held under its full id, which is what every
    /// assignment writes, and they are EVERY item it holds, whatever its
    /// status — `--all`, because `list` leaves closed items out by default:
    /// measured on 1.3.0, an item its holder closed was gone from `list -a
    /// <seat>` and back with `--all`, reading `closed`. A caller reads the
    /// status off each row, and the conformance suite's assignee check is the
    /// arm that found the two stores disagreeing.
    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError> {
        let seat;
        let args: Vec<&str> = match filter {
            Filter::Ready => vec!["ready", "--json", "-n", "0"],
            Filter::Label(label) => vec![
                "list", "--label", label, "--status", "open", "--json", "-n", "0",
            ],
            Filter::Assignee(held) => {
                seat = held.to_string();
                vec!["list", "-a", &seat, "--all", "--json", "-n", "0"]
            }
        };
        Ok(self
            .listed::<bd_wire::IssueWithCounts>(&args)?
            .into_iter()
            .filter_map(summary_of)
            .collect())
    }

    /// `--priority` only where the item names one: bd files an item that
    /// names none at its own default.
    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError> {
        let labels = item.labels.join(",");
        let priority = item.priority.map(|n| n.to_string());
        let by = by.to_string();
        let mut args = vec![
            "create",
            "--title",
            &item.title,
            "--description",
            &item.description,
            "--type",
            &item.item_type,
        ];
        if !labels.is_empty() {
            args.extend(["--labels", labels.as_str()]);
        }
        if let Some(priority) = &priority {
            args.extend(["--priority", priority.as_str()]);
        }
        args.extend(["--actor", &by, "--json"]);
        self.created_id(&args).map(ItemId::from)
    }

    /// ONE `update` carrying every field the change names: bd takes `--title`
    /// and `--assignee` on one call — measured on 1.3.0, where one call moved
    /// both — and the empty assignee is what clears the field, measured too:
    /// the item read back with no `assignee` at all.
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError> {
        if change.is_empty() {
            return Err(unchanged());
        }
        let assignee = change
            .assignee
            .as_ref()
            .map(|seat| seat.map(|seat| seat.to_string()).unwrap_or_default());
        let by = by.to_string();
        let mut args = vec!["update", id.as_str()];
        if let Some(title) = &change.title {
            args.extend(["--title", title.as_str()]);
        }
        if let Some(assignee) = &assignee {
            args.extend(["--assignee", assignee.as_str()]);
        }
        args.extend(["--actor", &by]);
        self.wrote(&args)
    }

    /// bd 1.3.0 refuses a plain `--assignee` from anyone but the holder on an
    /// `in_progress` item — measured: `cannot reassign X: held by "s1"
    /// (in_progress)` — and takes the same write when it names the holder with
    /// `--if-assignee`, which also writes nothing and exits 13 where the holder
    /// moved.
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
            &format!("held by {}", holder_named(from)),
        )
    }

    /// `{"fleet.orders": {…, "v": 1}}` in one `--metadata` write, which
    /// MERGES at the top level and replaces the one key's object whole —
    /// measured on bd 1.3.0, where a write of one key kept the other beside
    /// it. So the order is replaced whole and the run's record stands.
    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError> {
        let payload = order_metadata(order).to_string();
        let by = by.to_string();
        self.wrote(&[
            "update",
            id.as_str(),
            "--metadata",
            &payload,
            "--actor",
            &by,
        ])
    }

    /// ONE `update` carrying both: the empty assignee is what clears the field
    /// and `--unset-metadata` takes the one key away — measured on bd 1.3.0,
    /// where the one call left no `assignee` and no `fleet.orders`, and the
    /// `fleet.run` key and another writer's bare `orders` beside it standing.
    fn order_withdraw(&self, id: &ItemId, by: &Actor) -> Result<(), StoreError> {
        let by = by.to_string();
        self.wrote(&[
            "update",
            id.as_str(),
            "--assignee",
            "",
            "--unset-metadata",
            keys::ORDERS,
            "--actor",
            &by,
        ])
    }

    /// `{"fleet.run": {…, "v": 1}}` by the same flag [`order_set`] writes
    /// with: bd names one flag for a metadata write, and its polarity is what
    /// makes one flag safe for two keys.
    ///
    /// [`order_set`]: Store::order_set
    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        let payload = run_metadata(run).to_string();
        let by = by.to_string();
        self.wrote(&[
            "update",
            id.as_str(),
            "--metadata",
            &payload,
            "--actor",
            &by,
        ])
    }

    /// Taken from a writer who is not the holder on an `in_progress` item —
    /// measured on 1.3.0, where it is the assignee and not the status that bd
    /// keeps for the holder alone.
    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["update", item, "--status", "open", "--actor", by])
    }

    /// ONE `update` rather than three, because bd takes every flag on one call:
    /// [`order_withdraw`]'s argv, fenced, with the reopen beside it.
    ///
    /// Measured on 1.3.0: the one call left no `assignee` and no
    /// `fleet.orders`, the `fleet.run` key beside it standing, and an item its
    /// seat had marked `in_progress` open and in `bd ready` again.
    /// `--if-assignee` names the retiring seat, which is what bd 1.3.0 takes
    /// from a retirer on an item that seat marked `in_progress` — measured,
    /// where the same call without it is refused. `--if-status` is what keeps
    /// the reopen off a CLOSED item: measured, the call without it reopened an
    /// item its holder had closed, and with it wrote nothing and exited 13.
    ///
    /// [`order_withdraw`]: Store::order_withdraw
    fn order_withdraw_from(
        &self,
        id: &ItemId,
        seat: &SeatId,
        status: &Status,
        by: &Actor,
    ) -> Result<(), StoreError> {
        let seat = seat.to_string();
        let by = by.to_string();
        self.fenced(
            &[
                "update",
                id.as_str(),
                "--if-assignee",
                &seat,
                "--if-status",
                status.as_str(),
                "--assignee",
                "",
                "--unset-metadata",
                keys::ORDERS,
                "--status",
                "open",
                "--actor",
                &by,
            ],
            id,
            &format!("held by {} as {status}", holder_named(&seat)),
        )
    }

    /// bd files a hold as a gate of type human, and the held item leaves the
    /// ready set the moment it is created.
    ///
    /// `--type` is not passed: human is the type `bd gate create` takes with no
    /// flag, measured on 1.3.0, and a verb that spelled the default would be a
    /// second copy of it.
    ///
    /// THE ID COMES OFF THE STRUCTURED ANSWER and never off the printed line.
    /// `--actor` and `--json` are global flags here, so both are available on
    /// this subcommand; a parse of the prose is the form that goes quiet the
    /// day the prose changes.
    fn hold_raise(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<HoldId, StoreError> {
        let by = by.to_string();
        self.created_id(&[
            "gate",
            "create",
            "--blocks",
            id.as_str(),
            "--reason",
            reason,
            "--actor",
            &by,
            "--json",
        ])
        .map(HoldId::from)
    }

    /// A CLEAR OF A CLEARED HOLD IS READ FIRST, because bd does not refuse
    /// one. Measured on 1.3.0, on a scratch board: a second `bd gate resolve`
    /// of a resolved gate exits 0, prints `✓ Gate resolved: <id>` — with
    /// `Reason: <reason>` under it where one was passed — and writes nothing:
    /// the gate reads back `closed` with the first resolve's `closed_at` and
    /// `updated_at`, and no reason. So the gate's own row is read before the
    /// call, its status alone, and a closed one is Refused with nothing run:
    /// the act is already done. The read is also what refuses a hold that is
    /// not there, which bd's own resolve answers with exit 1 and `Error: gate
    /// not found: <id>`. A resolve landing between the read and the call is
    /// not caught: bd answers the second resolve as a resolve.
    fn hold_clear(&self, hold: &HoldId, by: &Actor) -> Result<(), StoreError> {
        let row = self.shown_row(hold)?;
        if row.get("status").and_then(serde_json::Value::as_str) == Some(Status::Closed.as_str()) {
            return Err(already_cleared(hold));
        }
        let by = by.to_string();
        self.wrote(&["gate", "resolve", hold.as_str(), "--actor", &by])
    }

    /// The listing answers which hold this is and never which item it blocks —
    /// measured on bd 1.3.0, where the blocked item appears only inside the
    /// description's prose.
    ///
    /// `-n 0` for the same reason the three reads above carry it: this verb
    /// answers its first 50 rows by default, piped or not — measured on 1.3.0,
    /// 50 of 53 — and a truncated list reads exactly like a whole one, so past
    /// fifty open holds a hold the board keeps open is absent from the listing
    /// — and `clear` refuses a hold it does not find there as one somebody has
    /// already cleared.
    fn holds_open(&self) -> Result<Vec<HoldId>, StoreError> {
        Ok(self
            .listed::<bd_wire::Issue>(&["gate", "list", "--json", "-n", "0"])?
            .into_iter()
            .filter_map(|row| row.id.map(HoldId::from))
            .collect())
    }

    /// bd 1.3.0 closes an assigned item only for an actor equal to its
    /// assignee — measured, `cannot close X: assignee is "<id>", actor is
    /// "seat:<id>"` — which is why a landing closes under the holder's own
    /// assignee string.
    ///
    /// A CLOSE OF A CLOSED ITEM IS READ FIRST, because bd does not refuse one.
    /// Measured on 1.3.0, on a scratch board: a second `bd close` of a closed
    /// item exits 0 — printing `✓ Closed <id> — <title>: <reason>`, or with
    /// `--json` the item's row — and writes nothing: the item reads back with
    /// the first close's `close_reason`, `closed_at` and `updated_at`. So the
    /// item's own row is read before the call, its status alone, and a closed
    /// one is Refused with nothing run: the act is already done. The read is
    /// also what refuses an item that is not there, which bd's own close
    /// answers with exit 1 and an error that carries no code. A close landing
    /// between the read and the call is not caught: bd answers the second
    /// close as a close.
    fn close(&self, id: &ItemId, reason: &str, by: &str) -> Result<(), StoreError> {
        let row = self.shown_row(id)?;
        if row.get("status").and_then(serde_json::Value::as_str) == Some(Status::Closed.as_str()) {
            return Err(already_closed(id));
        }
        self.wrote(&["close", id.as_str(), "--reason", reason, "--actor", by])
    }

    /// Each entry is ONE COMMENT whose text is [`entry::encode`]'s. Measured on
    /// bd 1.3.0, on a scratch board:
    /// - `bd comments add <id> <text> --actor A --json` answers the envelope
    ///   with data `{id, issue_id, author: A, text, created_at}`, and the id is
    ///   what this answers.
    /// - That id is CONTENT-DERIVED (`e9f93b1f-8828-563c-…`, version nibble 5;
    ///   beads v1.3.0 `internal/storage/issueops/derivedid.go`,
    ///   `InsertDerivedComment`). It is not time-ordered, so nothing sorts by it.
    /// - There is no edit or delete subcommand: an entry is kept as written.
    /// - A closed item takes comments.
    /// - `bd export` writes each issue's comments into the committed
    ///   `.beads/issues.jsonl`, so the timeline travels with the board.
    ///
    /// The id comes off the write's own answer, as `create`'s does. The
    /// `--actor` is what marks this call a write for `Bd::run`'s timeout
    /// message, as it does every other.
    fn append(&self, item: &ItemId, body: &Body, by: &Actor) -> Result<String, StoreError> {
        validated(item, body)?;
        let by = by.to_string();
        self.created_id(&[
            "comments",
            "add",
            item.as_str(),
            &entry::encode(body),
            "--actor",
            &by,
            "--json",
        ])
    }

    /// Measured on bd 1.3.0, on a scratch board:
    /// - `bd comments <id> --json` lists by `created_at` ASC, then id ASC
    ///   (`issueops/comments.go:28`). A live add truncates its time to the
    ///   second and advances it past the item's newest comment
    ///   (`derivedid.go:190-205`), so the listing is append order.
    /// - An item with no comments answers `data []`. A missing item exits 1
    ///   with data `{"error":"resolving <id>: no issue found matching
    ///   \"<id>\""}`, which carries no code.
    /// - The listing takes no row cap: the verb has no `-n`.
    /// - `bd show --json` answers `comment_count` and `"comments_omitted":
    ///   true`, and `bd list --json` answers `comment_count` only, so neither
    ///   is a way to read the entries.
    ///
    /// THE JSON IS READ BEFORE THE STATUS, for the reason `show`'s is: a missing
    /// item exits non-zero with the error object, which is the record's answer
    /// and not a refusal. So this stays off `answered` and off `listed`.
    fn timeline(&self, item: &ItemId) -> Result<Vec<Entry>, StoreError> {
        let args = ["comments", item.as_str(), "--json"];
        let out = self.run(&args)?;
        let rows = match self.json(&args, &out) {
            Some(serde_json::Value::Object(answer)) if answer.contains_key("error") => {
                let error = &answer["error"];
                let error = error
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| error.to_string());
                return Err(StoreError::Refused(format!("{item}: {error}")));
            }
            _ if !out.status.success() => return Err(self.refused(&args, &out)),
            Some(serde_json::Value::Array(rows)) => rows,
            Some(serde_json::Value::Null) => return Ok(Vec::new()),
            None if String::from_utf8_lossy(&out.stdout).trim().is_empty() => return Ok(Vec::new()),
            _ => {
                return Err(StoreError::Unreadable(format!(
                    "{} did not answer a list: {}",
                    self.named(&args),
                    tail(&out)
                )))
            }
        };
        let mut entries = Vec::new();
        for (at, row) in rows.iter().enumerate() {
            let comment: bd_wire::Comment = decoded(row).map_err(|why| {
                StoreError::Unreadable(format!(
                    "{} answered a row bd's wire types do not read, row {at}: {why}",
                    self.named(&args)
                ))
            })?;
            let field = |held: &Option<String>| held.clone().unwrap_or_default();
            let read = entry::read_row(
                item,
                &field(&comment.id),
                &field(&comment.author),
                &field(&comment.text),
                &field(&comment.created_at),
            )
            .map_err(StoreError::Unreadable)?;
            entries.extend(read);
        }
        Ok(entries)
    }

    /// [`EXPORT`] in [`DIR`], with no bd call: both are where bd keeps them
    /// and not anything a board is asked.
    ///
    /// WHETHER THE STORE IS VERSIONED IN A PROJECT IS READ, NOT ASSUMED, by
    /// the landing that commits the export. `bd init` writes the ignore that
    /// hides its own directory, so in a project that keeps the work graph out
    /// of git there is nothing under `.beads/` for git to take — and `git add
    /// .beads` over a directory it has been told to ignore exits 128 rather
    /// than staging nothing. The porcelain says which project this is.
    ///
    /// The item prefix is [`CONFIG`]'s `issue-prefix:`, read off the file and
    /// not asked of bd: a config that is not there, will not read or names
    /// none is `None`, which is a prefix nobody named and never one guessed.
    /// `bd init --prefix` leaves that key commented — measured on 1.3.0 — so a
    /// board a person made without naming it again answers `None`.
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        Ok(Capabilities {
            export: Some(ExportSpec {
                file: EXPORT.to_string(),
                dir: DIR.to_string(),
            }),
            scratch: true,
            item_prefix: std::fs::read_to_string(self.root.join(CONFIG))
                .ok()
                .and_then(|config| item_prefix_in(&config)),
        })
    }

    /// The version the first line of `bd --version` names — `1.3.0` of `bd
    /// version 1.3.0 (Homebrew)`, measured on 1.3.0 — as the contract's
    /// `version` is a version and not a sentence about one. A first line naming
    /// none is answered whole, so a `bd` that printed something else is quoted
    /// rather than read as a version.
    ///
    /// `-C` is asked of a directory with no `.beads` too, and bd refuses there
    /// — measured on 1.3.0, exit 1, `no beads project found` — so a project
    /// with no board answers this as it answers every other call.
    fn version(&self) -> Result<Version, StoreError> {
        let args = ["--version"];
        let out = self.answered(&args)?;
        let said = String::from_utf8_lossy(&out.stdout);
        let first = said.lines().next().unwrap_or_default().trim();
        if first.is_empty() {
            return Err(StoreError::Unreadable(format!(
                "{} answered no version: {}",
                self.named(&args),
                tail(&out)
            )));
        }
        Ok(Version {
            name: String::from(NAME),
            version: version_token(first).unwrap_or(first).to_string(),
        })
    }

    /// It regenerates that one file and touches nothing else — measured on bd
    /// 1.3.0, where two exports left every other file under `.beads` as it was
    /// in bytes, and in mtime bar the embedded engine's journal, which a bare
    /// read touches the same way.
    ///
    /// `-o` is resolved against the CALLER's directory and not against `-C`:
    /// measured on 1.3.0, `bd -C <root> export -o .beads/issues.jsonl` run
    /// from elsewhere wrote `.beads/issues.jsonl` under the caller and left the
    /// root's `.beads` without one. So the path handed over is absolute.
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError> {
        let written = into.join(EXPORT);
        // THE DIRECTORY IS MADE FIRST. `bd` writes the export through a temp
        // file beside it and makes no directory of its own, and a root whose
        // project keeps its store out of git has none until something does —
        // which is every fresh worktree, the landing lane among them.
        if let Some(dir) = written.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                StoreError::Unreadable(format!(
                    "the store's own directory {} could not be made: {e}",
                    dir.display()
                ))
            })?;
        }
        let into = written.to_string_lossy().into_owned();
        self.wrote(&["export", "-o", &into])?;
        Ok(written)
    }

    /// `bd init --prefix fx --quiet` run IN `into` — made first where it is
    /// not there — and never under `-C`, which bd 1.3.0 refuses on a directory
    /// holding no store: measured, `bd -C <empty dir> show x --json` exits 1
    /// with `Error: cannot use -C directory "<dir>": no beads project found`
    /// on stderr and nothing on stdout, which [`show`](Store::show) reads as
    /// Unreadable.
    ///
    /// NO SERVER FLAG, so the store is bd's embedded engine under
    /// `into/.beads` and never a port some other board is served on. bd writes
    /// its agent integrations beside it even under `--quiet` — measured on
    /// 1.3.0, a CLAUDE.md, an AGENTS.md, `.claude`, `.codex`, `.cursor` and a
    /// repository of its own — so `into` is a directory the caller made for
    /// this, and the answer is `into` itself.
    fn scratch(&self, into: &Path) -> Result<PathBuf, StoreError> {
        std::fs::create_dir_all(into).map_err(|e| {
            StoreError::Unreadable(format!(
                "the scratch directory {} could not be made: {e}",
                into.display()
            ))
        })?;
        let args = ["init", "--prefix", "fx", "--quiet"];
        let mut cmd = Command::new(&self.bin);
        cmd.env(ENVELOPE, "1").args(args).current_dir(into);
        let out = run_bounded(cmd, self.timeout).map_err(|why| {
            StoreError::Unreadable(format!(
                "{} in {} could not be run ({why}) — no scratch store was made",
                self.named(&args),
                into.display()
            ))
        })?;
        if !out.status.success() {
            return Err(self.refused(&args, &out));
        }
        Ok(into.to_path_buf())
    }
}

/// One row decoded into a wire type, or where it would not decode: the key's
/// path and the row's id beside serde's reason, which alone — "invalid type:
/// integer `3`, expected a string" — names neither the item nor the key, and a
/// read refused over one field on one row leaves nothing else to find it by.
fn decoded<'de, Row: Deserialize<'de>>(row: &'de serde_json::Value) -> Result<Row, String> {
    serde_path_to_error::deserialize(row).map_err(|why| {
        let id = row
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or("a row naming no id");
        format!("`{}` of {id}: {}", why.path(), why.inner())
    })
}

/// One row, read into the fields a verb asserts on, through bd's own wire
/// type. A row that does not decode into it — or that carries a run's record
/// this fleet does not read — is a store that did not answer something
/// readable.
pub fn item_from(id: &str, row: &serde_json::Value) -> Result<Item, StoreError> {
    let wire: bd_wire::IssueDetails = decoded(row).map_err(|why| {
        StoreError::Unreadable(format!(
            "{id} answered a row bd's wire types do not read: {why}"
        ))
    })?;
    let id = wire.id.unwrap_or_else(|| id.to_string());
    let metadata = wire.metadata.as_ref();
    Ok(Item {
        run: run_of(&id, metadata)?,
        order: order_of(metadata),
        item_type: wire.issue_type.unwrap_or_default(),
        // The item's own labels, absent when the key is absent — which the
        // store spells as `null` and not as an empty array.
        labels: wire.labels.unwrap_or_default(),
        title: wire.title.unwrap_or_default(),
        description: wire.description.unwrap_or_default(),
        status: Status::from(wire.status.unwrap_or_default()),
        assignee: wire.assignee,
        blockers: blockers_of(wire.dependencies.as_deref().unwrap_or_default()),
        proof: ReadProof::of(row.to_string()),
        id: ItemId::from(id),
    })
}

/// One listing row as the summary a listing answers, through the readers
/// [`item_from`] reads a row with: the status and the labels as the store
/// spells them, the order by [`order_of`], and the type off `issue_type`. A
/// row naming no id is no row.
fn summary_of(row: bd_wire::IssueWithCounts) -> Option<ItemSummary> {
    Some(ItemSummary {
        order: order_of(row.metadata.as_ref()),
        id: ItemId::from(row.id?),
        title: row.title.unwrap_or_default(),
        status: Status::from(row.status.unwrap_or_default()),
        item_type: row.issue_type.unwrap_or_default(),
        labels: row.labels.unwrap_or_default(),
    })
}

// ---- the opener's half: which `bd`, and the store over it ---------------------

/// What `[store] adapter` says to select this store, which is also what a
/// project that names no adapter gets.
pub const NAME: &str = "bd";

/// The `bd` a store [`open`] makes runs: `FLEET_BD_BIN` when it names an
/// ABSOLUTE path to an executable file, else the first `bd` on `search_path` —
/// the shape every other binary this workspace runs is resolved by, and never
/// this process's own `PATH`.
///
/// WHERE NOTHING RESOLVES, `strict` is could not tell, saying why. Not strict,
/// it is the bare name, which the process's own `PATH` then answers or does
/// not: the store fails the way it always has rather than in some new way, and
/// its refusal names the bare `bd` it tried.
pub fn resolve(search_path: &str, strict: bool) -> Result<PathBuf, StoreError> {
    let found = match std::env::var("FLEET_BD_BIN").ok().as_deref().map(str::trim) {
        Some(bin) if !bin.is_empty() => {
            if Path::new(bin).is_absolute() {
                resolve_on_path(search_path, bin).ok_or_else(|| {
                    format!("the item-tracker seam names `{bin}`, which is not an executable file")
                })
            } else {
                Err(format!(
                    "the item-tracker seam names `{bin}`, which is not an absolute path"
                ))
            }
        }
        _ => resolve_on_path(search_path, BD)
            .ok_or_else(|| format!("no `{BD}` on the constructed child PATH")),
    };
    match found {
        Ok(bin) => Ok(bin),
        Err(why) if strict => Err(StoreError::Unreadable(why)),
        Err(_) => Ok(PathBuf::from(BD)),
    }
}

/// The built-in store for [`super::open`]: the project's root, over the `bd`
/// [`resolve`] answers, under the caller's bound.
pub fn open(at: &super::Opening) -> Result<Box<dyn Store>, StoreError> {
    let bin = resolve(at.search_path, at.strict)?;
    Ok(Box::new(Bd::at_bin(at.root, &bin).with_timeout(at.timeout)))
}

/// The first `name` on `path` that is there and executable, as an absolute
/// path. A `name` that is already a path resolves to itself. The controller's
/// own resolver, copied: core takes nothing from the controller.
fn resolve_on_path(path: &str, name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.contains('/') {
        let named = PathBuf::from(name);
        return super::executable_file(&named).then_some(named);
    }
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| super::executable_file(candidate))
}

/// The bound on `bd version`, which reads no store and answered in 60ms on the
/// box this was written on: a `bd` that hangs costs [`tracker_line`] no more
/// than this.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// The `bd` a session's verbs reach, resolved strictly on `search_path` as
/// [`open`] resolves it, against [`PINNED_BD`], as one line: the pin, another
/// version with where to install the pin, or a `bd` that did not answer.
pub fn tracker_line(search_path: &str) -> String {
    let first = resolve(search_path, true)
        .map_err(|why| why.to_string())
        .and_then(|bin| version_of(&bin));
    match first {
        Ok(first) if carries_pin(&first) => format!("bd: {PINNED_BD}, the pinned version"),
        Ok(first) => format!(
            "bd: {}, not the pinned {PINNED_BD} — the verbs still run, on answers fleet was not \
             measured against; {}",
            named_version(&first),
            install_pointer()
        ),
        Err(why) => format!("bd: could not be read — {why}; {}", install_pointer()),
    }
}

/// The first line `bd version` printed, which is where bd prints its own.
fn version_of(bin: &Path) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.arg("version");
    let out = run_bounded(cmd, VERSION_TIMEOUT)
        .map_err(|why| format!("`{} version` {why}", bin.display()))?;
    let said = String::from_utf8_lossy(&out.stdout);
    let first = said.lines().next().unwrap_or_default().trim();
    if !out.status.success() || first.is_empty() {
        return Err(format!(
            "`{} version` {} and printed no version",
            bin.display(),
            out.status
        ));
    }
    Ok(first.to_string())
}

/// The pin as a WHOLE token of that line, bare or with a leading `v` — the
/// reading the defaults' `bd-version` doctor check takes, so the two agree.
fn carries_pin(first: &str) -> bool {
    first
        .split_whitespace()
        .any(|token| token.strip_prefix('v').unwrap_or(token) == PINNED_BD)
}

/// The version a line names: its first token that opens on a digit, or the
/// whole line where none does, so a `bd` that answered something else is
/// quoted rather than read as a version.
fn named_version(first: &str) -> String {
    version_token(first)
        .map(str::to_string)
        .unwrap_or_else(|| format!("`{first}`"))
}

/// The first token of a line that opens on a digit, bare of a leading `v` —
/// where bd prints its own version, on every release this was read on.
fn version_token(first: &str) -> Option<&str> {
    first
        .split_whitespace()
        .map(|token| token.strip_prefix('v').unwrap_or(token))
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))
}

/// Where to install the pinned release: beads' own installation page, read at
/// the pin's tag so it describes the release named, because bd installs
/// several ways and which one fits is the machine's. The defaults' doctor
/// check points at the same page, and a pin move checks the page is still at
/// this path at the new tag.
fn install_pointer() -> String {
    format!(
        "install the pinned bd {PINNED_BD} by beads' own instructions: \
         https://github.com/gastownhall/beads/blob/v{PINNED_BD}/docs/getting-started/installation.md"
    )
}

/// The resolution and the opener over it. `FLEET_BD_BIN` is process-wide and
/// these arms move it, so they hold one lock and put it back on a panic too.
/// The item prefix [`CONFIG`] carries, where it carries one.
///
/// A two-line reader rather than a YAML dependency: the one key wanted is at
/// the top level and is written as `issue-prefix: <value>`, and a file that
/// says it some other way answers `None`, which is the same as saying nothing.
pub fn item_prefix_in(config: &str) -> Option<String> {
    config
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| line.strip_prefix("issue-prefix:"))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod opening_tests {
    use super::{open, resolve, Filter, StoreError, BD, STORE_TIMEOUT};
    use crate::store::Opening;
    use std::path::{Path, PathBuf};

    /// Serialises the arms here, because `cargo test` runs this binary's arms
    /// as threads of one process and the seam is its environment.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Variables moved for one arm and put back when it ends — on a panic too,
    /// so a red arm never leaves the seam set for a sibling. `None` is a
    /// variable taken away.
    struct EnvHeld(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl EnvHeld {
        fn set(moved: &[(&'static str, Option<&std::ffi::OsStr>)]) -> EnvHeld {
            let held = EnvHeld(
                moved
                    .iter()
                    .map(|(key, _)| (*key, std::env::var_os(key)))
                    .collect(),
            );
            for (key, value) in moved {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            held
        }
    }

    impl Drop for EnvHeld {
        fn drop(&mut self) {
            for (key, before) in self.0.iter().rev() {
                match before {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fleet-bd-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("project")).expect("the project root is created");
        dir
    }

    /// The store the opener makes runs the `bd` it resolved on the search path
    /// it was HANDED, by absolute path — the situation of the controller's run
    /// pass on every tick, whose own `PATH` is a service's and holds no `bd`.
    /// Then the same through `FLEET_BD_BIN`, over a search path holding none.
    ///
    /// THE STUB IS ON NO ENTRY OF THIS PROCESS'S `PATH`, and it is what records
    /// the argv: a store that ran a bare `bd` would have reached some other
    /// file and left the log empty. The process's `PATH` is not moved to prove
    /// it, because this binary's own arms read it.
    #[test]
    fn the_bd_on_a_search_path_is_run_by_absolute_path() {
        let _lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = scratch("search-path");
        let root = dir.join("project");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("the stub's directory is created");

        // A `bd` that records the argv it was handed and answers an empty list.
        let log = dir.join("argv");
        let bd = bin.join("bd");
        std::fs::write(
            &bd,
            format!(
                "#!/bin/sh\n\
                 for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{log}'; done\n\
                 printf '[]\\n'\n",
                log = log.display(),
            ),
        )
        .expect("the stub is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bd, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
        let own = std::env::var_os("PATH").unwrap_or_default();
        assert!(
            !std::env::split_paths(&own).any(|entry| entry == bin),
            "the stub is on no entry of this process's own PATH"
        );

        let search_path = format!("/usr/bin:/bin:{}", bin.display());
        let policy = toml::Table::new();
        let opening = |search_path: &str| {
            open(&Opening {
                root: &root,
                policy: &policy,
                search_path,
                strict: true,
                timeout: STORE_TIMEOUT,
            })
            .map(|store| store.list(&Filter::Ready))
        };
        let (resolved, by_search_path, by_seam) = {
            let _held = EnvHeld::set(&[("FLEET_BD_BIN", None)]);
            let resolved = resolve(&search_path, true);
            let by_search_path = opening(&search_path);
            let _seam = EnvHeld::set(&[("FLEET_BD_BIN", Some(bd.as_os_str()))]);
            (resolved, by_search_path, opening("/usr/bin:/bin"))
        };

        assert_eq!(resolved, Ok(bd.clone()), "the absolute path, and no other");
        for (read, how) in [(by_search_path, "search path"), (by_seam, "seam")] {
            assert!(
                read.expect("the bd resolves")
                    .expect("the stub answers a list")
                    .is_empty(),
                "through the {how}"
            );
        }
        let argv: Vec<String> = std::fs::read_to_string(&log)
            .expect("the stub recorded its argv")
            .lines()
            .map(str::to_string)
            .collect();
        let one = [
            String::from("-C"),
            root.display().to_string(),
            String::from("ready"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ];
        assert_eq!(
            argv,
            [one.clone(), one].concat(),
            "exactly the two reads reached the stub"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Where nothing resolves — no `bd` on the search path, or a seam naming a
    /// path that is not absolute — a strict caller is refused, saying why, and
    /// any other is handed the bare name.
    #[test]
    fn nothing_resolving_is_refused_when_strict_and_the_bare_name_otherwise() {
        let _lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = scratch("unresolved");
        let empty = dir.join("project").display().to_string();

        let unresolved = |seam: Option<&str>| {
            let _held = EnvHeld::set(&[("FLEET_BD_BIN", seam.map(std::ffi::OsStr::new))]);
            (resolve(&empty, true), resolve(&empty, false))
        };
        for (seam, why) in [
            (None, String::from("no `bd` on the constructed child PATH")),
            (
                Some("relative/bd"),
                String::from(
                    "the item-tracker seam names `relative/bd`, which is not an absolute path",
                ),
            ),
            (
                Some("/no/such/bd"),
                String::from(
                    "the item-tracker seam names `/no/such/bd`, which is not an executable file",
                ),
            ),
        ] {
            let (strict, lenient) = unresolved(seam);
            assert_eq!(strict, Err(StoreError::Unreadable(why)), "{seam:?}");
            assert_eq!(lenient, Ok(Path::new(BD).to_path_buf()), "{seam:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The mapping [`item_from`] makes of fleet's two keys, one arm per answer.
#[cfg(test)]
mod tests {
    use super::{item_from, item_prefix_in, OrderState, StoreError};
    use crate::seat::actor::Actor;
    use crate::store::{Order, OrderKind, RunRecord, Stamp};

    /// The config's one key, read four ways: set, quoted, commented, and a
    /// file that names it not at all.
    #[test]
    fn the_item_prefix_is_read_from_the_store_config_or_is_absent() {
        assert_eq!(
            item_prefix_in("# a comment\nissue-prefix: ap\n"),
            Some("ap".to_string())
        );
        assert_eq!(
            item_prefix_in("issue-prefix: \"ap\"\n"),
            Some("ap".to_string())
        );
        assert_eq!(
            item_prefix_in("# issue-prefix: \"\"\n"),
            None,
            "a commented line is not a value"
        );
        assert_eq!(item_prefix_in("issue-prefix:\n"), None);
        assert_eq!(item_prefix_in("database: dolt\n"), None);
    }

    const BY: &str = "seat:01a0d1f1-0aec-765f-9abe-0000001ead01";
    const SEAT: &str = "01a0d1f1-0aec-765f-9abe-00000005ea71";
    const AT: &str = "2026-09-24T10:00:00Z";

    fn row(metadata: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "id": "fx-1", "title": "an item", "metadata": metadata })
    }

    fn order_of(index: serde_json::Value) -> OrderState {
        item_from("fx-1", &row(serde_json::json!({ "fleet.orders": index })))
            .expect("an order index never refuses the read")
            .order
    }

    #[test]
    fn an_order_index_at_its_version_is_the_order() {
        assert_eq!(
            order_of(serde_json::json!({
                "v": 1, "by": BY, "kind": "dispatch", "seat": SEAT, "at": AT,
            })),
            OrderState::Ordered(Order {
                kind: OrderKind::Dispatch,
                by: Actor::typed(BY).expect("typed").expect("a seat"),
                seat: Some(crate::seat::identity::SeatId::parse(SEAT).expect("a seat id")),
                at: Stamp::parse(AT).expect("a stamp"),
            })
        );
        assert!(
            matches!(
                order_of(serde_json::json!({ "v": 1, "by": BY, "kind": "dispatch", "at": AT })),
                OrderState::Ordered(Order { seat: None, .. })
            ),
            "a transient dispatch's index names no seat yet"
        );
    }

    #[test]
    fn an_absent_or_null_order_index_is_none() {
        let absent = item_from("fx-1", &row(serde_json::json!({}))).expect("reads");
        assert_eq!(absent.order, OrderState::None);
        assert_eq!(order_of(serde_json::Value::Null), OrderState::None);
    }

    /// Not an object, at another version or none, partly filled, a field that
    /// is not the type it names, or one the order does not carry: each is an
    /// index this fleet cannot read, and none is taken for absent.
    #[test]
    fn an_order_index_that_does_not_read_as_an_order_is_unreadable() {
        for (label, index) in [
            ("a string", serde_json::json!("not an object")),
            (
                "v 2",
                serde_json::json!({ "v": 2, "by": BY, "kind": "dispatch", "at": AT }),
            ),
            (
                "no v",
                serde_json::json!({ "by": BY, "kind": "dispatch", "at": AT }),
            ),
            (
                "no by",
                serde_json::json!({ "v": 1, "kind": "dispatch", "at": AT }),
            ),
            (
                "a bare by",
                serde_json::json!({ "v": 1, "by": "alberto", "kind": "dispatch", "at": AT }),
            ),
            (
                "a kind fleet gives none of",
                serde_json::json!({ "v": 1, "by": BY, "kind": "build", "at": AT }),
            ),
            (
                "an at that is no stamp",
                serde_json::json!({ "v": 1, "by": BY, "kind": "dispatch", "at": "then" }),
            ),
            (
                "a seat that is no id",
                serde_json::json!({ "v": 1, "by": BY, "kind": "dispatch", "seat": "s1", "at": AT }),
            ),
            (
                "a field it does not carry",
                serde_json::json!({ "v": 1, "by": BY, "kind": "dispatch", "at": AT, "extra": 1 }),
            ),
        ] {
            assert_eq!(order_of(index), OrderState::Unreadable, "{label}");
        }
    }

    #[test]
    fn a_run_record_at_its_version_is_the_record_and_absent_is_none() {
        let read = item_from(
            "fx-1",
            &row(serde_json::json!({ "fleet.run": {
                "v": 1, "hash": "h1", "workflow": "greet", "pack": "ts",
                "entry": "greet.ts", "started_at": AT,
            }})),
        )
        .expect("a record at v 1 reads");
        assert_eq!(
            read.run,
            Some(RunRecord {
                hash: String::from("h1"),
                workflow: String::from("greet"),
                pack: String::from("ts"),
                entry: String::from("greet.ts"),
                started_at: Stamp::parse(AT).expect("a stamp"),
            })
        );
        for metadata in [
            serde_json::json!({}),
            serde_json::json!({ "fleet.run": null }),
        ] {
            let read = item_from("fx-1", &row(metadata.clone())).expect("reads");
            assert_eq!(read.run, None, "{metadata}");
        }
    }

    /// A record at another version, at none, not an object, or at v 1 and not
    /// a record, refuses the whole read — the item is not answered as one
    /// carrying no run.
    #[test]
    fn a_run_record_this_fleet_does_not_read_refuses_the_read() {
        for (held, version) in [
            (serde_json::json!({ "v": 2, "hash": "h1" }), "v 2"),
            (serde_json::json!({ "hash": "h1" }), "v none"),
            (serde_json::json!("a string"), "v none"),
            (serde_json::json!({ "v": 1, "hash": "h1" }), "v 1"),
        ] {
            let refusal = item_from("fx-1", &row(serde_json::json!({ "fleet.run": held })))
                .expect_err("the read refuses");
            let wanted =
                format!("fx-1's run record is not one this fleet reads (fleet.run, {version})");
            assert!(
                matches!(&refusal, StoreError::Unreadable(why) if why.starts_with(&wanted)),
                "{held}: {refusal:?}"
            );
        }
    }
}
