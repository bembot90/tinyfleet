//! The work graph, reached through the `bd` binary and no other way (packs PRD
//! R20).
//!
//! The trait is what the verbs are written against, so a suite can force a
//! read-back that disagrees with the write beside it — the one failure a real
//! store will not produce on demand and the one the verbs must survive.
//!
//! Every read decodes the FIRST JSON value of the answer and ignores what
//! trails it: `bd show --json` answers a top-level array and closes with a
//! newline, and a decoder that demands the whole text be one value refuses a
//! well-formed answer over its last byte.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The binary every write and read goes through when the caller names no
/// other, resolved on the process's own `PATH`.
pub const BD: &str = "bd";

/// Where the store's export goes, relative to the project root. It is a passive
/// file the work graph regenerates, never a second copy anything reads back.
pub const EXPORT: &str = ".beads/issues.jsonl";

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
    /// `metadata.orders`, as the store holds it.
    pub orders: Option<Orders>,
    /// Whether `metadata` carried an `orders` key at all, which `orders` alone
    /// cannot say: a key holding something that is not an object is present and
    /// unreadable, and a withdrawal has to tell that from absent.
    pub has_orders_key: bool,
    /// The open items this one depends on, by id.
    pub blockers: Vec<String>,
    /// The type, as the store spells it: the JSON key is `issue_type` and a
    /// rule matches on this value.
    pub item_type: String,
    /// The item's OWN labels and no parent's, which is what the store answers.
    pub labels: Vec<String>,
    /// `metadata.flight`, as free JSON. The keys this slice fixes are read by
    /// name from it; a later slice reads more without moving this field.
    pub flight: Option<serde_json::Value>,
    /// `metadata.run`, the same way, for a run's record item. A SECOND FIELD
    /// AND NOT A SECOND READING OF THE FIRST: the two lifecycles write one
    /// top-level key each, which is what lets bd's top-level merge leave the
    /// other standing.
    pub run: Option<serde_json::Value>,
    /// When the store says this item was filed. `plan --ready N` orders by it,
    /// because the ready read's own order is the store's and not age's.
    pub created_at: String,
    /// The decoded document, as text. The negative control reads this, so the
    /// control asks the SAME answer for a token nothing wrote.
    pub document: String,
}

/// One row of the ready answer.
///
/// It is a row and not an id because the ready answer's own order is the
/// store's — measured on bd 1.2.2: priority ascending, then newest first
/// within a priority — so a caller that wants the OLDEST rows sorts on a field
/// and never takes a position in the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ready {
    pub id: String,
    pub created_at: String,
    /// The row's own labels, so a caller can leave a kind of item out of a
    /// pool without a second read of every row.
    pub labels: Vec<String>,
}

/// A new item, as the arguments a `create` takes.
///
/// There is no id here: the store names what it files, which is why [`Store`]'s
/// `create` answers one.
pub struct NewItem<'a> {
    pub title: &'a str,
    pub description: &'a str,
    /// The store's own spelling — `task` for a flight's record.
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
    /// Which resume this order is, where the writer named one: a return's
    /// dispatch carries the return ordinal (flights PRD R11). Absent on a first
    /// dispatch, because the count a reader wants is the fold's and a key that
    /// said `1` would be a second copy of it.
    pub ordinal: Option<u64>,
}

/// One row of a seat's list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub status: String,
    /// Whether `metadata` carried an `orders` key, read off THIS ROW and not
    /// off a second call: the listing answers each row's metadata, so a caller
    /// asking which of a seat's items are ordered pays one call and not one per
    /// row. The same third answer [`Item::has_orders_key`] carries — a key
    /// holding something that is not an object is present and unreadable.
    pub has_orders_key: bool,
}

/// The two ways a store call ends badly, which are two different exits: an
/// item that is not there is the record's answer, and a store that will not
/// answer is no reading at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store answered, and its answer is that there is no such item.
    Missing(String),
    /// The store could not be run, or did not answer something readable.
    Unreadable(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Missing(why) => write!(f, "{why}"),
            StoreError::Unreadable(why) => write!(f, "{why}"),
        }
    }
}

pub trait Store {
    /// The rows the store calls ready: open and unblocked, in the store's own
    /// order.
    fn ready(&self) -> Result<Vec<Ready>, StoreError>;

    fn show(&self, item: &str) -> Result<Item, StoreError>;

    /// The open items carrying this label, by id.
    ///
    /// Ids and not documents: the list read answers no `metadata`, so a caller
    /// that needs an item's flight object reads it with [`Store::show`] and
    /// this answers which items to ask about.
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError>;

    /// One item filed, answered as the id the store gave it.
    ///
    /// A flight's record is titled by its own id, which nothing knows until
    /// this returns — so the title in [`NewItem`] is what the record carries
    /// until the caller retitles it, and the caller's read-back is what says
    /// the second write landed.
    fn create(&self, item: &NewItem, by: &str) -> Result<String, StoreError>;

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError>;

    /// The item as a person reads it — the rendering a brief carries verbatim,
    /// so a seat and a person read the same text.
    fn show_text(&self, item: &str) -> Result<String, StoreError>;

    /// Every row the store holds against this seat.
    fn assigned_to(&self, seat: &str) -> Result<Vec<Row>, StoreError>;

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError>;

    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError>;

    /// `metadata.orders`, written as one object that replaces the key whole.
    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError>;

    /// One metadata object written by the same call `set_orders` makes, for a
    /// top-level key that is not `orders`.
    ///
    /// It is the SIBLING of that method and not a generalisation of it: the
    /// write MERGES at the top level — measured on bd 1.2.2, where a second
    /// write of a different key kept the first — so a flight's object never
    /// erases the order index beside it, and the two keys keep one writer each.
    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError>;

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError>;

    /// The assignee cleared and the order index unset in ONE call.
    ///
    /// The pair is what a withdrawal always writes together, and a retire pays
    /// it on every seat it ends — so the store that talks to `bd` sends one
    /// `update` rather than two, which is a call the verb does not make while
    /// another suite is queueing behind it. The DEFAULT is the two writes in
    /// order, so an implementation that has nothing to gain by folding them
    /// says nothing; what neither form may do is leave the assignee cleared
    /// with the index still set, which the caller's read-back is what catches.
    fn withdraw_order(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.assign(item, "", by)?;
        self.unset_orders(item, by)
    }

    /// A gate raised on this item, answered as the gate's own id.
    ///
    /// The store's own object and not a question item this fleet owns (flights
    /// PRD S4a): the gate is of type human, the blocked item leaves the ready
    /// set the moment it is created, and it comes back when somebody resolves
    /// the gate. So a park needs nothing of fleet's beside the event.
    fn gate(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError>;

    /// Every gate the store still calls open, by id.
    ///
    /// IDS AND NOT DOCUMENTS, and no item on them: the listing answers which
    /// gate this is and never which item it blocks — measured on bd 1.2.2,
    /// where the blocked item appears only inside the reason's prose — so a
    /// caller that wants one item's gate reads that gate's id off the item's
    /// own park and asks this list whether it is still here (flights PRD S4a).
    fn open_gates(&self) -> Result<Vec<String>, StoreError>;

    /// One gate resolved, which puts the item it blocked back in the ready set.
    fn resolve_gate(&self, gate: &str, by: &str) -> Result<(), StoreError>;

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
    /// It regenerates that one file and touches nothing else — measured on a
    /// scratch store, where two exports left the audit log beside it unchanged
    /// in bytes and in mtime — so nothing is carried forward here to keep the
    /// export's polarity right (packs PRD R20).
    fn export(&self, into: &Path) -> Result<(), StoreError>;
}

/// The store as `bd` on this box, scoped to one project.
///
/// `-C <root>` on every call, so the store a verb writes to is the project's
/// whatever directory the call was made from.
pub struct Bd {
    root: PathBuf,
    bin: PathBuf,
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
        }
    }

    /// One call, with its status read from the command itself.
    fn run(&self, args: &[&str]) -> Result<Output, StoreError> {
        Command::new(&self.bin)
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| {
                StoreError::Unreadable(format!(
                    "`{}` could not be run ({e}) — nothing was written",
                    self.bin.display()
                ))
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

    /// One call that has to succeed, its answer handed back whole.
    fn answered(&self, args: &[&str]) -> Result<Output, StoreError> {
        let out = self.run(args)?;
        if !out.status.success() {
            return Err(self.refused(args, &out));
        }
        Ok(out)
    }

    /// One listing, as its rows.
    ///
    /// AN EMPTY LISTING MAY ANSWER `null` AND NOT `[]` — measured on bd 1.2.2
    /// for `gate list` — or nothing at all, and both read as no rows: a decoder
    /// demanding an array would read "nothing here" as a store that would not
    /// answer, and refuse every answer on a fleet with nothing parked.
    fn listed(&self, args: &[&str]) -> Result<Vec<serde_json::Value>, StoreError> {
        let out = self.answered(args)?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        match first_value(&stdout) {
            Some(serde_json::Value::Array(rows)) => Ok(rows),
            Some(serde_json::Value::Null) => Ok(Vec::new()),
            None if stdout.trim().is_empty() => Ok(Vec::new()),
            _ => Err(StoreError::Unreadable(format!(
                "{} did not answer a list: {}",
                self.named(args),
                tail(&out)
            ))),
        }
    }

    /// The id off a write's OWN answer. A second read for the newest item would
    /// name whatever else landed in the store between the two calls.
    fn created_id(&self, args: &[&str]) -> Result<String, StoreError> {
        let out = self.answered(args)?;
        first_value(&String::from_utf8_lossy(&out.stdout))
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

fn text_field(row: &serde_json::Value, key: &str) -> Option<String> {
    row.get(key)?.as_str().map(str::to_string)
}

/// The order index off a document, and whether the key was there at all.
fn orders_of(row: &serde_json::Value) -> (Option<Orders>, bool) {
    let Some(held) = row.get("metadata").and_then(|m| m.get("orders")) else {
        return (None, false);
    };
    if held.is_null() {
        return (None, false);
    }
    let Some(table) = held.as_object() else {
        return (None, true);
    };
    let read = |key: &str| table.get(key).and_then(|v| v.as_str()).map(str::to_string);
    (
        Some(Orders {
            by: read("by"),
            kind: read("kind"),
            seat: read("seat"),
            at: read("at"),
            ordinal: table.get("ordinal").and_then(serde_json::Value::as_u64),
        }),
        true,
    )
}

/// The dependencies that still stand between this item and a start: an entry
/// the store reports closed has been answered and is not a blocker.
fn blockers_of(row: &serde_json::Value) -> Vec<String> {
    let Some(entries) = row.get("dependencies").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter(|entry| {
            entry
                .get("status")
                .and_then(|s| s.as_str())
                .map(|status| status != "closed")
                .unwrap_or(true)
        })
        .filter_map(|entry| text_field(entry, "id"))
        .collect()
}

impl Store for Bd {
    fn ready(&self) -> Result<Vec<Ready>, StoreError> {
        // `-n 0` lifts the read's row cap. The verb answers its first 100 rows
        // by default, and a truncated list reads exactly like a whole one — so
        // past a hundred ready rows a ready item would be refused as not ready.
        Ok(self
            .listed(&["ready", "--json", "-n", "0"])?
            .iter()
            .filter_map(|row| {
                Some(Ready {
                    id: text_field(row, "id")?,
                    created_at: text_field(row, "created_at").unwrap_or_default(),
                    labels: labels_of(row),
                })
            })
            .collect())
    }

    fn show(&self, item: &str) -> Result<Item, StoreError> {
        // THE JSON IS READ BEFORE THE STATUS, and this read stays off
        // `answered`: bd exits non-zero on an item it does not hold and prints
        // the error object, which is the record's answer and not a refusal.
        let args = ["show", item, "--json"];
        let out = self.run(&args)?;
        let Some(value) = first_value(&String::from_utf8_lossy(&out.stdout)) else {
            return Err(StoreError::Unreadable(format!(
                "{} answered no JSON: {}",
                self.named(&args),
                tail(&out)
            )));
        };
        let Some(row) = sole(value) else {
            return Err(StoreError::Missing(format!("{item} is not in the store")));
        };
        if let Some(error) = row.get("error").and_then(|e| e.as_str()) {
            return Err(StoreError::Missing(format!("{item}: {error}")));
        }
        if !out.status.success() {
            return Err(self.refused(&args, &out));
        }
        Ok(item_from(item, &row))
    }

    /// `-q`, which the JSON reads do not need and this one does: the human
    /// rendering carries a one-off tip on a store's FIRST read — measured on
    /// 1.2.2, present on call one and absent on every call after — and that
    /// line both names a provider and makes the same item render two different
    /// ways. Quiet drops it and leaves the body byte-identical.
    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        let out = self.answered(&["-q", "show", item])?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// `-n 0` for the same reason the ready read carries it: this answer's
    /// default cap is 50 rows and a truncated list reads exactly like a whole
    /// one, so past fifty open flight records a plan would be admitted onto a
    /// list an open flight already holds.
    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        Ok(self
            .listed(&[
                "list", "--label", label, "--status", "open", "--json", "-n", "0",
            ])?
            .iter()
            .filter_map(|row| text_field(row, "id"))
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

    /// `-n 0` for the same reason the reads above carry it: this answer's
    /// default cap is 50 rows and a truncated list reads exactly like a whole
    /// one, so past fifty items on one seat a retire misses the orders beyond
    /// row 50 and the next seat of that name inherits them.
    fn assigned_to(&self, seat: &str) -> Result<Vec<Row>, StoreError> {
        Ok(self
            .listed(&["list", "-a", seat, "--json", "-n", "0"])?
            .iter()
            .filter_map(|row| {
                Some(Row {
                    id: text_field(row, "id")?,
                    status: text_field(row, "status").unwrap_or_default(),
                    has_orders_key: orders_of(row).1,
                })
            })
            .collect())
    }

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["update", item, "--assignee", seat, "--actor", by])
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
        self.wrote(&["update", item, "--unset-metadata", "orders", "--actor", by])
    }

    /// Both flags on one `update`, which bd takes: the empty assignee is what
    /// clears the field, measured through the shipped binary in the cli's own
    /// seat suite.
    fn withdraw_order(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&[
            "update",
            item,
            "--assignee",
            "",
            "--unset-metadata",
            "orders",
            "--actor",
            by,
        ])
    }

    /// `--type` is not passed: human is the type `bd gate create` takes with no
    /// flag, measured on 1.2.2, and a verb that spelled the default would be a
    /// second copy of it.
    ///
    /// THE ID COMES OFF THE STRUCTURED ANSWER and never off the printed line.
    /// `--actor` and `--json` are global flags here, so both are available on
    /// this subcommand; a parse of the prose is the form that goes quiet the
    /// day the prose changes.
    fn gate(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        self.created_id(&[
            "gate", "create", "--blocks", item, "--reason", reason, "--actor", by, "--json",
        ])
    }

    /// `-n 0` for the same reason the three reads above carry it: this verb
    /// answers its first 50 rows by default and a truncated list reads exactly
    /// like a whole one, so past fifty open gates a gate the board holds open
    /// is absent from the listing — and `answer` refuses a gate it does not
    /// find there as one somebody has already resolved.
    fn open_gates(&self) -> Result<Vec<String>, StoreError> {
        Ok(self
            .listed(&["gate", "list", "--json", "-n", "0"])?
            .iter()
            .filter_map(|row| text_field(row, "id"))
            .collect())
    }

    fn resolve_gate(&self, gate: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["gate", "resolve", gate, "--actor", by])
    }

    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        self.wrote(&["close", item, "--reason", reason, "--actor", by])
    }

    /// `-o` is resolved against the CALLER's directory and not against `-C`:
    /// measured on a scratch store, `bd -C <root> export -o .beads/issues.jsonl`
    /// run from elsewhere wrote `.beads/issues.jsonl` under the caller and left
    /// the root's `.beads` without one. So the path handed over is absolute.
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

/// The item's own labels, absent when the key is absent — which the store
/// spells as `null` and not as an empty array.
fn labels_of(row: &serde_json::Value) -> Vec<String> {
    row.get("labels")
        .and_then(|l| l.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// One document, read into the fields a verb asserts on.
pub fn item_from(id: &str, row: &serde_json::Value) -> Item {
    let (orders, has_orders_key) = orders_of(row);
    Item {
        item_type: text_field(row, "issue_type").unwrap_or_default(),
        labels: labels_of(row),
        // The same read `orders_of` makes, one key over: absent when the key
        // is absent, so a flight that wrote nothing is told from one that
        // wrote an empty object.
        flight: row
            .get("metadata")
            .and_then(|m| m.get("flight"))
            .filter(|held| !held.is_null())
            .cloned(),
        run: row
            .get("metadata")
            .and_then(|m| m.get("run"))
            .filter(|held| !held.is_null())
            .cloned(),
        created_at: text_field(row, "created_at").unwrap_or_default(),
        id: text_field(row, "id").unwrap_or_else(|| id.to_string()),
        title: text_field(row, "title").unwrap_or_default(),
        status: text_field(row, "status").unwrap_or_default(),
        assignee: text_field(row, "assignee"),
        notes: row
            .get("notes")
            .and_then(|n| n.as_str())
            .map(str::to_string),
        orders,
        has_orders_key,
        blockers: blockers_of(row),
        document: row.to_string(),
    }
}
