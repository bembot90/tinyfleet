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
use crate::store::{ItemId, Store, StoreError};

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

/// The store's two answers in the exit vocabulary: an item that is not there is
/// refused on the record, and a store that would not answer is could-not-tell.
/// The store's own sentence is the message, word for word.
impl From<StoreError> for Stop {
    fn from(e: StoreError) -> Stop {
        match e {
            StoreError::Refused(why) | StoreError::Moved(why) => Stop::refused(why),
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
/// [`recorded`] answered, and `kind` the entry's kind as
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

/// The payload keys the events table names for the two above. `outcome` and
/// `attempt` ride `step.closed`; the first four ride both.
pub const STEP_PAYLOAD: [&str; 6] = ["item", "flight", "step", "run", "outcome", "attempt"];

/// Where a typed event goes. Core never opens the stream file: the cli wires
/// this to the controller's writer, the same way `dispatch` reaches its spawn.
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

/// A progress surface that draws nothing, for a caller that has no terminal to
/// draw on.
pub struct Silent;

impl Progress for Silent {
    fn row(&self) {}
    fn message(&self, _text: &str) {}
    fn finish(&self) {}
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

    /// The paths `git diff --cached --name-only` prints.
    fn staged(&self) -> Result<Vec<String>, String>;

    /// `git status --porcelain`, one line per entry, classified by the caller:
    /// the two status columns are the index's and the working tree's, and only
    /// the second says whether a path was left unstaged.
    fn status(&self) -> Result<Vec<String>, String>;

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
