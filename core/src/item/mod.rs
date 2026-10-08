//! The item verbs: the work a project's items move through.
//!
//! `dispatch` and `brief` are the two this slice carries. They share this
//! module's seams — the store, the ring and the spawner — and its templates,
//! which are files in the resolved pack layers rather than literals here, so a
//! pack on top replaces any of them whole.
//!
//! Nothing in here reads the process table or the environment. What only a
//! process knows — the project root, the machine directory, the clock, the
//! dispatcher's name — is resolved by the cli and handed in.

pub mod brief;
pub mod deliver;
pub mod dispatch;
pub mod doctor;
pub mod events;
pub use events::{
    payload_keys, signal, Events, CHECK_READ, ITEM_ENTRY, RUN_CANCELLED, RUN_CLEANED, RUN_CLOSED,
    RUN_COULD_NOT_TELL, RUN_FAILED, RUN_STARTED, RUN_WAITING, STEP_CLOSED, STEP_STARTED,
};
pub mod hold;
pub mod land;
pub mod lane;
pub mod list;
pub mod pins;
pub mod review;
pub mod rules;
pub mod run;
pub mod show;

use std::path::{Path, PathBuf};

use crate::entry::{to_json, Body, Entry, Timeline};
use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;
use crate::store::{Item, ItemId, Store, StoreError};

/// The exits, one vocabulary shared by every verb. A verb answers with one of
/// these and the cli does nothing but return it.
pub const DONE: u8 = 0;
pub const REFUSED: u8 = 1;
pub const USAGE: u8 = 2;
pub const COULD_NOT_TELL: u8 = 3;
pub const NO_SESSION: u8 = 4;

/// A verb that stopped, with the exit a script reads and the sentence a person
/// does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub code: u8,
    pub message: String,
}

impl Stop {
    pub fn refused(message: impl Into<String>) -> Stop {
        Stop {
            code: REFUSED,
            message: message.into(),
        }
    }

    /// An instrument the answer needed would not answer. Nothing this verb
    /// could have written is written.
    pub fn could_not_tell(message: impl Into<String>) -> Stop {
        Stop {
            code: COULD_NOT_TELL,
            message: message.into(),
        }
    }

    /// The call itself is wrong — a missing argument, or a file handed in that
    /// the grammar does not read. Nothing about the record is being judged.
    pub fn usage(message: impl Into<String>) -> Stop {
        Stop {
            code: USAGE,
            message: message.into(),
        }
    }
}

/// The store's answers in the exit vocabulary: an item that is not there is
/// refused on the record, a write no store takes is usage, and a store that
/// would not answer is could-not-tell. The store's own sentence is the
/// message, word for word.
impl From<StoreError> for Stop {
    fn from(e: StoreError) -> Stop {
        match e {
            StoreError::Refused(why) | StoreError::Moved(why) => Stop::refused(why),
            StoreError::Usage(why) => Stop::usage(why),
            StoreError::Unreadable(why) => Stop::could_not_tell(why),
        }
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a ring found at the other end.
///
/// `Absent` is a seat with no live session, which is not a failure: the
/// assignment already recorded the handoff, and the seat's successor reads the
/// order at wake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RingOutcome {
    Delivered,
    Absent,
    Failed(String),
}

pub trait Ring {
    fn ring(&self, seat: &str, text: &str) -> RingOutcome;
}

/// Where a long act says how far it has got. The cli draws it; core states the
/// three things it has to say and nothing about how they look.
///
/// Nothing here changes an exit or a row: a progress surface that could decide
/// something would be a second verdict nobody reads.
pub trait Progress {
    /// One more row read.
    fn row(&self);

    /// What the wait says about itself while it lasts.
    fn message(&self, text: &str);

    /// Nothing more is coming.
    fn finish(&self);
}

/// The trunk, as the local ref names it. `deliver` records the base it read
/// and never fetches: a fetch moves what a second verb is about to check
/// against, and `land` is the verb that owns that.
pub const TRUNK: &str = "origin/main";

/// The branch a delivery may not sit on.
pub const TRUNK_BRANCH: &str = "main";

/// One path a diff changed, in the three columns `git diff --numstat` prints.
///
/// A binary file's columns are dashes, which is a changed file with no line
/// delta rather than a zero: `None` keeps the two apart, because a size line
/// that read a dash as 0 would report a 40MB asset as an empty change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub added: Option<u64>,
    pub deleted: Option<u64>,
    pub path: String,
}

/// The git a verb reads and writes through.
///
/// Every operation answers a typed value or a refusal naming the step and what
/// git said; none of them rounds a failure to a default, because a verb that
/// read "no branch" as "main" would refuse a delivery for the wrong reason.
pub trait Git {
    /// The branch HEAD is on.
    fn current_branch(&self) -> Result<String, String>;

    /// The commit HEAD names now.
    fn head(&self) -> Result<String, String>;

    /// [`TRUNK`] as the local ref holds it. No fetch.
    fn trunk_tip(&self) -> Result<String, String>;

    /// The paths `git diff --cached --name-only -z` prints, raw.
    fn staged(&self) -> Result<Vec<String>, String>;

    /// `git status --porcelain -z`, one typed entry per path, classified by the
    /// caller: the two status columns are the index's and the working tree's,
    /// and only the second says whether a path was left unstaged.
    fn status(&self) -> Result<Vec<StatusLine>, String>;

    /// Everything the working tree holds put in the index: tracked changes,
    /// unstaged modifications and untracked files alike.
    ///
    /// It is a SECOND operation beside [`Git::commit`] and not a flag on it,
    /// because the two verbs that commit want different sets: a delivery is the
    /// staged set the seat chose, and a park is everything the seat has.
    fn add_all(&self) -> Result<(), String>;

    /// The staged set committed with this message, answered as the commit the
    /// commit produced — read from HEAD in the same act, never from the branch
    /// tip a later one could move.
    fn commit(&self, message: &str) -> Result<String, String>;

    /// `git diff --numstat <from>...<to>`: counted from the two commits'
    /// merge-base, so what `<from>` gained since is not read as reversed.
    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String>;
}

/// One `git diff --numstat` line. A path holding a tab cannot be told from the
/// columns, so a line with more than three fields is read as its first two
/// counts and the rest as the path.
pub fn numstat_line(line: &str) -> Option<Change> {
    let mut fields = line.splitn(3, '\t');
    let added = fields.next()?;
    let deleted = fields.next()?;
    let path = fields.next()?;
    if path.is_empty() {
        return None;
    }
    let count = |field: &str| field.parse::<u64>().ok();
    Some(Change {
        added: count(added),
        deleted: count(deleted),
        path: path.to_string(),
    })
}

/// One entry of `git status --porcelain -z`: the index's column, the working
/// tree's column, and the path, raw — the `-z` form quotes nothing, so the path
/// is the bytes a person types. A rename or copy also names where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub index: char,
    pub worktree: char,
    pub path: String,
    pub from: Option<String>,
}

/// The v1 porcelain line, minus git's quoting: `XY from -> path` for a rename
/// or copy, `XY path` otherwise.
impl std::fmt::Display for StatusLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.from {
            Some(from) => write!(f, "{}{} {from} -> {}", self.index, self.worktree, self.path),
            None => write!(f, "{}{} {}", self.index, self.worktree, self.path),
        }
    }
}

/// Every `git status --porcelain -z` entry. Each is `XY <path>` ended by a NUL,
/// and a rename or copy (`R` or `C` in either column) is followed by its origin
/// as a field of its own — the new name first. An entry of any other shape is
/// refused rather than read as a path, because a path misread here is one a
/// delivery would carry or leave behind without a word.
pub fn status_entries(z: &str) -> Result<Vec<StatusLine>, String> {
    let malformed = |field: &str| {
        format!("`git status --porcelain -z` answered an entry that is not `XY <path>`: {field:?}")
    };
    let mut fields = z.split('\0').filter(|field| !field.is_empty());
    let mut entries = Vec::new();
    while let Some(field) = fields.next() {
        let bytes = field.as_bytes();
        if bytes.len() < 4 || !bytes[0].is_ascii() || !bytes[1].is_ascii() || bytes[2] != b' ' {
            return Err(malformed(field));
        }
        let index = char::from(bytes[0]);
        let worktree = char::from(bytes[1]);
        let from = if [index, worktree].iter().any(|c| matches!(c, 'R' | 'C')) {
            Some(fields.next().ok_or_else(|| malformed(field))?.to_string())
        } else {
            None
        };
        entries.push(StatusLine {
            index,
            worktree,
            path: field[3..].to_string(),
            from,
        });
    }
    Ok(entries)
}

/// What the controller answered when asked for a seat to give this item to.
///
/// `seat` is the spawned seat's full id, which is what dispatch assigns the
/// item to and writes into its index.
///
/// `belt` is both readings the spawn was let through on, rendered by the
/// spawner into the lines a person reads. Core measures no machine and knows
/// nothing of the belt's shape, so the text rides the answer rather than the
/// numbers. `None` is a spawner that ran no belt, and prints nothing.
///
/// `Refused` is a VERDICT and `CouldNotTell` is a question. Dispatch withdraws
/// an order on the first and leaves it standing on the second, so a spawn
/// nobody could observe is never recorded as a spawn that was declined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnOutcome {
    Spawned { seat: String, belt: Option<String> },
    Refused(String),
    CouldNotTell(String),
}

/// What a spawn is asked for, as ONE STRUCT and not a parameter list.
///
/// Every field here is a fact the caller resolved and the spawner only carries,
/// and the set has grown twice: a field added to this struct is a field named
/// at each call site, where a fourth positional argument would silently
/// re-order at every implementor of [`Spawner`] across the crate boundary.
pub struct Spawn<'a> {
    /// The file whose text is the session's first turn.
    pub first_turn: &'a Path,
    /// The work item this spawn is being made for.
    ///
    /// It is carried so the spawn can RECORD it — the controller's own record
    /// of what it started has no other route to the work graph, and a report
    /// about a seat that cannot name the item it was holding is one a person
    /// cannot act on. Nothing below this seam reads the work graph.
    pub item: &'a str,
    /// The commit the seat's worktree is cut from, where the caller names one
    /// — `fleet seat spawn --base`. `None` cuts from the trunk.
    pub base: Option<&'a str>,
    /// The model this seat runs on, where the caller names one. `None` leaves
    /// the fleet's policy default.
    pub model: Option<&'a str>,
    /// The builder's own checks, as the dispatch was handed them: the command the
    /// seat's permission rules let it run. `None` writes no rule for one.
    pub touched: Option<&'a str>,
}

pub trait Spawner {
    /// Start a seat, as the request describes it.
    fn spawn(&self, spawn: &Spawn) -> SpawnOutcome;
}

/// The project a verb acts inside, as the cli resolved it.
///
/// `policy` and `guards` are two tables and not one because a standalone fleet
/// keeps them in two files: the project declares its own policy, and the fleet
/// declares which guards its seats run. An embedded fleet hands the same table
/// twice, which is the shape its one `fleet.toml` actually has.
pub struct Project {
    pub root: PathBuf,
    pub name: String,
    /// The project's whole policy file, every table in it.
    pub policy: toml::Table,
    pub guards: toml::Table,
}

impl Project {
    /// A refusal where either file still sets a test command, naming each key
    /// and where it is set instead ([`crate::policy::MOVED`]), or still carries
    /// a table whose keys moved, naming where each is set now
    /// ([`crate::policy::MOVED_TABLES`]) — or a key nothing reads any more,
    /// naming it to delete ([`crate::policy::RETIRED`]).
    ///
    /// Read by every verb that would have read one — `land`, the brief, a
    /// spawn's rules and `run` — BEFORE it writes anything, so a fleet whose
    /// landings a person believes are tested hears otherwise from the first
    /// verb that lands, briefs or opens a run.
    pub fn refuse_moved(&self) -> Result<(), Stop> {
        let mut found = crate::policy::moved(&self.policy);
        for also in crate::policy::moved(&self.guards) {
            if !found.contains(&also) {
                found.push(also);
            }
        }
        if found.is_empty() {
            return Ok(());
        }
        Err(Stop::refused(
            found
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  "),
        ))
    }
}

/// One policy file as a table. Core is this workspace's TOML reader, so the cli
/// resolves the paths and every parse happens here.
///
/// An unreadable or unparsable file reads as an empty table, which is what the
/// guards' own reader does with one: a missing key leaves a default, and a
/// brief still renders.
pub fn table_at(path: &Path) -> toml::Table {
    read_table(path).unwrap_or_default()
}

/// The same file with the two failures kept apart, for a reader that has to
/// tell an empty policy from one it could not read — `fleet status` prints the
/// `[[core.flight.rules]]` table off the policy in force and answers
/// could-not-tell where the file will not parse.
pub fn read_table(path: &Path) -> Result<toml::Table, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    text.parse::<toml::Table>()
        .map_err(|e| format!("{} is not readable TOML: {e}", path.display()))
}

/// `[project] name`, which a standalone project declares because its directory
/// name is the checkout's and not the project's.
pub fn project_name(table: &toml::Table) -> Option<String> {
    table
        .get("project")?
        .as_table()?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

/// A template's placeholder is `{name}` with a lower-case name, and every one a
/// template writes must be a name the caller offered.
///
/// An unknown placeholder is an error rather than a literal: a brief that
/// printed `{seat}` where a seat's name belongs is a first turn nobody can act
/// on, and it reads as prose to everything downstream.
pub fn render(template: &str, values: &[(&str, &str)]) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_placeholder(&after[..close]) => {
                let name = &after[..close];
                match values.iter().find(|(key, _)| *key == name) {
                    Some((_, value)) => out.push_str(value),
                    None => return Err(name.to_string()),
                }
                rest = &after[close + 1..];
            }
            // A brace that opens no placeholder is text. Nothing is scanned
            // inside a value once it is written out, so a value that itself
            // holds braces is carried through and never re-read.
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn is_placeholder(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A token nothing wrote, for the read-backs to ask their own answer about.
///
/// Every other assertion a read-back makes is of the form "the record says what
/// we asked for", and a read that answered yes to everything would satisfy all
/// of them. This asks the same answer for something that cannot be there.
///
/// ONE TOKEN PER PROCESS, and deliberately so: a token minted per call cannot
/// be planted, so the check that reads it could never be shown failing and
/// would be a green with no subject. Nothing writes this string to a store
/// either way, which is the whole of what the control needs.
pub fn control_token() -> &'static str {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TOKEN.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("fleet-control-{}-{nanos}", std::process::id())
    })
}

/// The negative control every read-back ends on: a read whose proof carries
/// [`control_token`] is not reading the item `id` names.
pub(crate) fn refuse_planted(read: &Item, id: &str) -> Result<(), Stop> {
    let control = control_token();
    if read.proof.carries(control) {
        return Err(Stop::could_not_tell(format!(
            "the read-back on {id} carries {control}, which nothing wrote — the read is not \
             reading this item"
        )));
    }
    Ok(())
}

/// One read of `item`, asserting its assignee against the ARGUMENT, then the
/// negative control.
pub(crate) fn assignee_reads_back(
    store: &dyn Store,
    item: &str,
    wanted: SeatId,
) -> Result<(), Stop> {
    let read = store.show(item)?;
    if read.assignee != Some(wanted) {
        return Err(Stop::could_not_tell(format!(
            "{item} read back with assignee ==\n{}\n  wanted:\n{wanted}\n  READ: fleet item \
             show {item}",
            read.assignee
                .map_or_else(|| String::from("(absent)"), |held| held.to_string())
        )));
    }
    refuse_planted(&read, item)
}

/// Why an entry a verb wrote is not on the record: the store refused the write,
/// or took it and the read after it does not show it. Two halves because they
/// are two sentences — "nothing was written" and "the write's effect cannot be
/// told" — and each verb says its own STANDS line for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrecorded {
    NotWritten(StoreError),
    Unconfirmed(String),
}

/// One entry appended to `item`'s timeline and READ BACK, answered as the id
/// the store gave it.
///
/// The read-back asks four things of the timeline: that it holds the id the
/// append answered, that the entry under it carries the body and the actor
/// written, and — the control — that it carries no entry under
/// [`control_token`], which nothing writes. A read that answered yes to
/// everything would pass the first three.
///
/// Each verb that calls it maps [`Unrecorded`] to its own wording.
pub fn recorded(
    store: &dyn Store,
    item: &str,
    body: &Body,
    by: &Actor,
) -> Result<String, Unrecorded> {
    let written = ItemId::from(item);
    let id = store
        .append(&written, body, by)
        .map_err(Unrecorded::NotWritten)?;
    let entries = store
        .timeline(&written)
        .map_err(|e| Unrecorded::Unconfirmed(e.to_string()))?;
    let timeline = Timeline(&entries);
    let kind = body.kind();
    let Some(read) = timeline.entry(&id) else {
        return Err(Unrecorded::Unconfirmed(format!(
            "{item}'s timeline does not hold the {kind} entry {id} the store answered for it"
        )));
    };
    if read.body != *body || read.by != *by {
        let written = Entry {
            id: id.clone(),
            at: read.at.clone(),
            by: by.clone(),
            body: body.clone(),
        };
        return Err(Unrecorded::Unconfirmed(format!(
            "{item}'s {kind} entry {id} read back as {} and {} was written",
            to_json(read),
            to_json(&written)
        )));
    }
    let token = control_token();
    if timeline.entry(token).is_some() {
        return Err(Unrecorded::Unconfirmed(format!(
            "the read-back of {item}'s timeline carries {token}, which nothing wrote — the read \
             is not reading this item"
        )));
    }
    Ok(id)
}
