//! One routine file: `orders/<name>.toml`, read into a [`Routine`] or into the
//! [`Defect`] that says why it is not one.
//!
//! Every refusal names the file and the key. A value this reader does not
//! understand is a defect and never an ignored line: a `schedule` on a cooldown
//! routine is a plan its author believes in and nothing reads, and a misspelled
//! key is a duty that silently never runs.

use super::trigger::Trigger;
use fleet_core::seat::identity::{resolve, Directory, SeatId, SeatRef};
use fleet_core::store::types::PRIORITY_MAX;
use std::path::{Path, PathBuf};

/// Where a routine was loaded from, as it is published and printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Fleet,
    Pack(String),
    Project(String),
}

impl Source {
    pub fn as_string(&self) -> String {
        match self {
            Source::Fleet => "fleet".to_string(),
            Source::Pack(name) => format!("pack:{name}"),
            Source::Project(name) => format!("project:{name}"),
        }
    }
}

/// Ring a seat with one sentence and the authority behind it.
#[derive(Clone, Debug)]
pub struct Nudge {
    /// The seat as the file names it, which is how every line about the ring
    /// names it back.
    pub seat: String,
    /// The one seat that name resolved to at load. `None` only on a file the
    /// load refused, which is never run.
    pub seat_id: Option<SeatId>,
    pub text: String,
    pub authority: String,
}

/// Whether an item is filed on every firing or only when the ring found nobody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    Absent,
}

/// File one item on the work graph.
#[derive(Clone, Debug)]
pub struct Item {
    pub title: String,
    pub description: Option<String>,
    pub labels: Vec<String>,
    /// The assignee as the file names it, which is how a line about the
    /// filing names it back.
    pub assignee: Option<String>,
    /// The one seat that name resolved to at load, which is what the item is
    /// filed assigned to. `None` where the file names no assignee, or on a
    /// file the load refused, which is never run.
    pub assignee_id: Option<SeatId>,
    pub priority: Option<i64>,
    pub kind: Option<String>,
    pub when: When,
    /// `open` or nothing. The one rule there is: file nothing while an open item
    /// already carries this routine's own label.
    pub dedupe: Option<String>,
}

/// Run one command.
#[derive(Clone, Debug)]
pub struct Exec {
    pub command: String,
}

/// Run one workflow through `fleet run`, with the inputs pinned as given.
#[derive(Clone, Debug)]
pub struct Run {
    /// The workflow's name, without its extension, as `fleet run` resolves it.
    pub workflow: String,
    /// `[action.run.inputs]`, one `--input key=value` each, in key order.
    pub inputs: Vec<(String, String)>,
}

/// The action a routine carries. Exactly one kind, with one exception: a nudge
/// beside an item whose `when` is `absent`, which is the fallback pair — a ring
/// that finds no live seat leaves the duty on the work graph instead of nowhere.
#[derive(Clone, Debug, Default)]
pub struct Action {
    pub nudge: Option<Nudge>,
    pub item: Option<Item>,
    pub exec: Option<Exec>,
    pub run: Option<Run>,
}

impl Action {
    /// The kind, as `fleet routine list` and a dry run print it.
    pub fn kind(&self) -> &'static str {
        match (&self.nudge, &self.item, &self.exec, &self.run) {
            (Some(_), Some(_), _, _) => "nudge+item",
            (Some(_), _, _, _) => "nudge",
            (_, Some(_), _, _) => "item",
            (_, _, _, Some(_)) => "run",
            _ => "exec",
        }
    }
}

/// One loaded routine, with every default already applied.
#[derive(Clone, Debug)]
pub struct Routine {
    pub name: String,
    pub path: PathBuf,
    pub source: Source,
    /// The directory a condition's check and an exec's command run in, and the
    /// store an item is filed against.
    pub project_root: PathBuf,
    pub description: String,
    pub trigger: Trigger,
    pub schedule: Option<String>,
    /// Seconds, for a cooldown.
    pub interval: Option<u64>,
    pub check: Option<String>,
    pub check_timeout: u64,
    /// The check exit statuses this routine maps to could-not-tell.
    pub check_unknown_exit: Vec<i32>,
    /// How often a condition's check runs. A check on every tick is a load, not
    /// a reading.
    pub poll: u64,
    /// The action's own bound, in seconds.
    pub timeout: u64,
    pub enabled: bool,
    pub action: Action,
}

/// A file that is not a routine, and why.
#[derive(Clone, Debug)]
pub struct Defect {
    pub name: String,
    pub path: PathBuf,
    pub source: Source,
    /// Every reason this file was refused, in the order they were found. Joined
    /// into one row by the printer, so one broken file is one line.
    pub reasons: Vec<String>,
}

impl Defect {
    pub fn reason(&self) -> String {
        self.reasons.join("; ")
    }
}

/// The action's own default bound.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
/// A condition's check bound when the file names none.
pub const DEFAULT_CHECK_TIMEOUT_SECONDS: u64 = 60;
/// How often a condition's check runs when the file names no `poll`.
pub const DEFAULT_POLL_SECONDS: u64 = 600;

/// The keys `[order]` carries, and the trigger each trigger parameter belongs
/// to. A key outside this table is a defect; a key under the wrong trigger is
/// a defect too, and this is the one place either question is answered.
const ROUTINE_KEYS: [(&str, Option<Trigger>); 10] = [
    ("description", None),
    ("trigger", None),
    ("timeout", None),
    ("enabled", None),
    ("schedule", Some(Trigger::Cron)),
    ("interval", Some(Trigger::Cooldown)),
    ("check", Some(Trigger::Condition)),
    ("check_timeout", Some(Trigger::Condition)),
    ("check_unknown_exit", Some(Trigger::Condition)),
    ("poll", Some(Trigger::Condition)),
];

const NUDGE_KEYS: [&str; 3] = ["seat", "text", "authority"];
const ITEM_KEYS: [&str; 8] = [
    "title",
    "description",
    "labels",
    "assignee",
    "priority",
    "type",
    "when",
    "dedupe",
];
const EXEC_KEYS: [&str; 1] = ["command"];
const RUN_KEYS: [&str; 2] = ["workflow", "inputs"];

/// `30s` `15m` `6h` `1d` to seconds. The form is an integer and one of `smhd`,
/// and nothing else parses.
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let (digits, unit) = text.split_at(text.len().saturating_sub(1));
    let scale = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => 0,
    };
    let whole = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    if scale == 0 || !whole {
        return Err(format!(
            "`{text}` is not a duration; the form is an integer and one of s/m/h/d (30s, 15m, 6h, 1d)"
        ));
    }
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(scale))
        .ok_or_else(|| format!("`{text}` is a duration this clock cannot hold"))
}

/// The stem of a routine file, refused unless it is one this registry can name.
pub fn routine_name_of(path: &Path) -> Result<String, String> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let shaped = !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if shaped {
        Ok(stem)
    } else {
        Err(format!(
            "the file name `{stem}` is not a routine name; the form is lower-case letters, digits and dashes"
        ))
    }
}

/// Read one file into a routine or a defect.
///
/// `seats` is the machine's seat directory: a nudge action names a row this
/// machine runs, because a seat it does not carry is a ring nobody would ever
/// answer, and an item's assignee names any seat the fleet knows.
pub fn read(path: &Path, source: &Source, project_root: &Path, seats: &Directory) -> Loaded {
    let name = match routine_name_of(path) {
        Ok(name) => name,
        Err(reason) => {
            return Loaded::Defective(Defect {
                name: path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: path.to_path_buf(),
                source: source.clone(),
                reasons: vec![reason],
            })
        }
    };
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) => {
            return Loaded::Defective(Defect {
                name,
                path: path.to_path_buf(),
                source: source.clone(),
                reasons: vec![format!("cannot be read: {e}")],
            })
        }
    };
    parse(&name, path, source, project_root, &body, seats)
}

/// What one file read to.
pub enum Loaded {
    Routine(Box<Routine>),
    Defective(Defect),
}

/// The parse and every refusal, with the file's text passed in.
pub fn parse(
    name: &str,
    path: &Path,
    source: &Source,
    project_root: &Path,
    body: &str,
    seats: &Directory,
) -> Loaded {
    let mut reasons: Vec<String> = Vec::new();
    let defective = |reasons: Vec<String>| {
        Loaded::Defective(Defect {
            name: name.to_string(),
            path: path.to_path_buf(),
            source: source.clone(),
            reasons,
        })
    };

    let document: toml::Table = match toml::from_str(body) {
        Ok(table) => table,
        Err(e) => return defective(vec![format!("is not valid TOML: {e}")]),
    };
    for key in document.keys() {
        if key != "order" && key != "action" {
            reasons.push(format!(
                "carries a `{key}` table; a routine file carries [order] and one [action.<kind>] and no third"
            ));
        }
    }
    let Some(routine) = table_at(&document, "order") else {
        reasons.push("carries no [order] table".to_string());
        return defective(reasons);
    };

    // The trigger first: every trigger parameter below is read against it, so
    // a file that does not name one cannot be judged at all.
    let trigger = match routine.get("trigger") {
        Some(toml::Value::String(word)) => match Trigger::parse(word) {
            Some(trigger) => Some(trigger),
            None => {
                reasons.push(format!(
                    "[order] trigger is `{word}`; the three are cron, cooldown and condition"
                ));
                None
            }
        },
        Some(other) => {
            reasons.push(format!(
                "[order] trigger is {}, not a string",
                kind_of(other)
            ));
            None
        }
        None => {
            reasons.push("[order] carries no `trigger`".to_string());
            None
        }
    };

    for key in routine.keys() {
        match ROUTINE_KEYS.iter().find(|(known, _)| known == key) {
            None => reasons.push(format!("[order] carries an unknown key `{key}`")),
            Some((_, Some(belongs))) => {
                if trigger.is_some_and(|named| named != *belongs) {
                    reasons.push(format!(
                        "[order] carries `{key}`, which belongs to trigger `{}`, under trigger `{}`",
                        belongs.as_str(),
                        trigger.expect("the trigger is named").as_str()
                    ));
                }
            }
            Some((_, None)) => {}
        }
    }

    let description = match routine.get("description") {
        Some(toml::Value::String(text)) if !text.trim().is_empty() && !text.contains('\n') => {
            text.trim().to_string()
        }
        Some(toml::Value::String(_)) => {
            reasons.push("[order] description is empty or spans more than one line".to_string());
            String::new()
        }
        Some(other) => {
            reasons.push(format!(
                "[order] description is {}, not a string",
                kind_of(other)
            ));
            String::new()
        }
        None => {
            reasons.push("[order] carries no `description`".to_string());
            String::new()
        }
    };

    let enabled = match routine.get("enabled") {
        Some(toml::Value::Boolean(on)) => *on,
        Some(other) => {
            reasons.push(format!(
                "[order] enabled is {}, not a boolean",
                kind_of(other)
            ));
            true
        }
        None => true,
    };
    let timeout = duration_key(&mut reasons, routine, "timeout", DEFAULT_TIMEOUT_SECONDS);

    let mut schedule = None;
    let mut interval = None;
    let mut check = None;
    let mut check_timeout = DEFAULT_CHECK_TIMEOUT_SECONDS;
    let mut check_unknown_exit = Vec::new();
    let mut poll = DEFAULT_POLL_SECONDS;

    match trigger {
        Some(Trigger::Cron) => match routine.get("schedule") {
            Some(toml::Value::String(text)) => {
                if let Err(why) = crate::routines::trigger::validate_cron(text) {
                    reasons.push(format!("[order] schedule {why}"));
                }
                schedule = Some(text.clone());
            }
            Some(other) => reasons.push(format!(
                "[order] schedule is {}, not a string",
                kind_of(other)
            )),
            None => reasons
                .push("[order] carries no `schedule`, which trigger `cron` reads".to_string()),
        },
        Some(Trigger::Cooldown) => match routine.get("interval") {
            Some(toml::Value::String(text)) => match parse_duration(text) {
                Ok(seconds) => interval = Some(seconds),
                Err(why) => reasons.push(format!("[order] interval {why}")),
            },
            Some(other) => reasons.push(format!(
                "[order] interval is {}, not a string",
                kind_of(other)
            )),
            None => reasons
                .push("[order] carries no `interval`, which trigger `cooldown` reads".to_string()),
        },
        Some(Trigger::Condition) => {
            match routine.get("check") {
                Some(toml::Value::String(text)) if !text.trim().is_empty() => {
                    check = Some(text.clone())
                }
                Some(toml::Value::String(_)) => reasons.push("[order] check is empty".to_string()),
                Some(other) => {
                    reasons.push(format!("[order] check is {}, not a string", kind_of(other)))
                }
                None => reasons.push(
                    "[order] carries no `check`, which trigger `condition` reads".to_string(),
                ),
            }
            check_timeout = duration_key(
                &mut reasons,
                routine,
                "check_timeout",
                DEFAULT_CHECK_TIMEOUT_SECONDS,
            );
            poll = duration_key(&mut reasons, routine, "poll", DEFAULT_POLL_SECONDS);
            check_unknown_exit = unknown_exits(&mut reasons, routine);
        }
        None => {}
    }

    let action = read_action(&mut reasons, &document, seats);

    if !reasons.is_empty() {
        return defective(reasons);
    }
    Loaded::Routine(Box::new(Routine {
        name: name.to_string(),
        path: path.to_path_buf(),
        source: source.clone(),
        project_root: project_root.to_path_buf(),
        description,
        trigger: trigger.expect("a file with no trigger is already a defect"),
        schedule,
        interval,
        check,
        check_timeout,
        check_unknown_exit,
        poll,
        timeout,
        enabled,
        action,
    }))
}

/// `[action.<kind>]`: exactly one, or the fallback pair.
fn read_action(reasons: &mut Vec<String>, document: &toml::Table, seats: &Directory) -> Action {
    let Some(action) = table_at(document, "action") else {
        reasons.push("carries no [action.<kind>] table".to_string());
        return Action::default();
    };
    let mut built = Action::default();
    for key in action.keys() {
        if !matches!(key.as_str(), "nudge" | "item" | "exec" | "run") {
            reasons.push(format!(
                "[action.{key}] is not an action kind; the four are nudge, item, exec and run"
            ));
        }
    }
    if let Some(table) = table_at(action, "nudge") {
        unknown_keys(reasons, table, "action.nudge", &NUDGE_KEYS);
        let seat = required_line(reasons, table, "action.nudge", "seat");
        // RESOLVED HERE, at load, through the resolver every seat argument
        // takes: the ring then goes to the row this found, by its id, and a
        // seat nobody holds — or two that both answer — refuses the file.
        let seat_id = if seat.is_empty() {
            None
        } else {
            match seats.resolve_running(&seat) {
                Ok(found) => Some(found.id),
                Err(unresolved) => {
                    reasons.push(format!("[action.nudge] seat is {seat}, which {unresolved}"));
                    None
                }
            }
        };
        built.nudge = Some(Nudge {
            text: required_line(reasons, table, "action.nudge", "text"),
            authority: required_line(reasons, table, "action.nudge", "authority"),
            seat,
            seat_id,
        });
    }
    if let Some(table) = table_at(action, "item") {
        unknown_keys(reasons, table, "action.item", &ITEM_KEYS);
        built.item = Some(read_item(reasons, table, seats));
    }
    if let Some(table) = table_at(action, "exec") {
        unknown_keys(reasons, table, "action.exec", &EXEC_KEYS);
        built.exec = Some(Exec {
            command: required_line(reasons, table, "action.exec", "command"),
        });
    }

    if let Some(table) = table_at(action, "run") {
        unknown_keys(reasons, table, "action.run", &RUN_KEYS);
        built.run = Some(Run {
            workflow: required_line(reasons, table, "action.run", "workflow"),
            inputs: read_inputs(reasons, table),
        });
    }

    let named = [
        built.nudge.is_some(),
        built.item.is_some(),
        built.exec.is_some(),
        built.run.is_some(),
    ]
    .iter()
    .filter(|there| **there)
    .count();
    let fallback_pair = built.nudge.is_some()
        && built.exec.is_none()
        && built.run.is_none()
        && built
            .item
            .as_ref()
            .is_some_and(|item| item.when == When::Absent);
    if named == 0 {
        reasons.push("carries no [action.<kind>] table".to_string());
    } else if named > 1 && !fallback_pair {
        reasons.push(
            "carries more than one action; the one pair allowed is [action.nudge] beside an \
             [action.item] whose `when` is `absent`"
                .to_string(),
        );
    }
    built
}

/// `[action.run.inputs]`: a table of strings, or nothing. Every value is a
/// string because every `--input` is one: a number written bare would reach
/// the workflow as text either way, and refusing it says so at the file.
fn read_inputs(reasons: &mut Vec<String>, table: &toml::Table) -> Vec<(String, String)> {
    let mut inputs = Vec::new();
    match table.get("inputs") {
        None => {}
        Some(toml::Value::Table(pairs)) => {
            for (key, value) in pairs {
                match value {
                    toml::Value::String(text) => inputs.push((key.clone(), text.clone())),
                    other => reasons.push(format!(
                        "[action.run.inputs] {key} is {}, not a string",
                        kind_of(other)
                    )),
                }
            }
        }
        Some(other) => reasons.push(format!(
            "[action.run] inputs is {}, not a table of strings",
            kind_of(other)
        )),
    }
    inputs
}

fn read_item(reasons: &mut Vec<String>, table: &toml::Table, seats: &Directory) -> Item {
    let when = match table.get("when") {
        Some(toml::Value::String(word)) if word == "always" => When::Always,
        Some(toml::Value::String(word)) if word == "absent" => When::Absent,
        Some(other) => {
            reasons.push(format!(
                "[action.item] when is {}; the two are always and absent",
                shown(other)
            ));
            When::Always
        }
        None => When::Always,
    };
    let dedupe = match table.get("dedupe") {
        Some(toml::Value::String(word)) if word == "open" => Some("open".to_string()),
        Some(other) => {
            reasons.push(format!(
                "[action.item] dedupe is {}; the one rule there is `open`",
                shown(other)
            ));
            None
        }
        None => None,
    };
    // The contract's range and a word, and no more, at load: the types and
    // the range inside this one that the project's store takes are its own
    // declaration, read where the item is filed.
    let priority = match table.get("priority") {
        Some(toml::Value::Integer(n)) if (0..=i64::from(PRIORITY_MAX)).contains(n) => Some(*n),
        Some(other) => {
            reasons.push(format!(
                "[action.item] priority is {}; the range is 0 to {PRIORITY_MAX}",
                shown(other)
            ));
            None
        }
        None => None,
    };
    let kind = match table.get("type") {
        Some(toml::Value::String(word)) if !word.is_empty() => Some(word.clone()),
        Some(other) => {
            reasons.push(format!(
                "[action.item] type is {}; a type is a word the project's store declares",
                shown(other)
            ));
            None
        }
        None => None,
    };
    let labels = match table.get("labels") {
        Some(toml::Value::Array(items)) => {
            let mut kept = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    toml::Value::String(text) => kept.push(text.clone()),
                    other => reasons.push(format!(
                        "[action.item] labels carries {}, not a string",
                        kind_of(other)
                    )),
                }
            }
            kept
        }
        Some(other) => {
            reasons.push(format!(
                "[action.item] labels is {}, not a list of strings",
                kind_of(other)
            ));
            Vec::new()
        }
        None => Vec::new(),
    };
    // [ASSUMES D14] RESOLVED HERE, at load, among every seat the fleet knows
    // and every seat this machine runs: the item is filed assigned to the id
    // this found, which is what a holder check compares, and a name nobody
    // holds — or two seats both answer to — refuses the file.
    let assignee = optional_string(reasons, table, "action.item", "assignee");
    let assignee_id = assignee.as_deref().and_then(|said| {
        let known = known_seats(seats);
        match resolve(&known, said) {
            Ok(index) => Some(known[index].id),
            Err(unresolved) => {
                reasons.push(format!(
                    "[action.item] assignee is {said}, which {unresolved}"
                ));
                None
            }
        }
    });
    Item {
        title: required_line(reasons, table, "action.item", "title"),
        description: optional_string(reasons, table, "action.item", "description"),
        labels,
        assignee,
        assignee_id,
        priority,
        kind,
        when,
        dedupe,
    }
}

/// The listed seats and the running ones, each once: the seats an item may be
/// filed to.
fn known_seats(seats: &Directory) -> Vec<SeatRef> {
    let mut known: Vec<SeatRef> = Vec::new();
    for seat in seats.listed.iter().chain(&seats.running) {
        if !known.iter().any(|kept| kept.id == seat.id) {
            known.push(seat.clone());
        }
    }
    known
}

fn unknown_keys(reasons: &mut Vec<String>, table: &toml::Table, where_: &str, known: &[&str]) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            reasons.push(format!("[{where_}] carries an unknown key `{key}`"));
        }
    }
}

fn required_line(
    reasons: &mut Vec<String>,
    table: &toml::Table,
    where_: &str,
    key: &str,
) -> String {
    match table.get(key) {
        Some(toml::Value::String(text)) if !text.trim().is_empty() => text.clone(),
        Some(toml::Value::String(_)) => {
            reasons.push(format!("[{where_}] {key} is empty"));
            String::new()
        }
        Some(other) => {
            reasons.push(format!(
                "[{where_}] {key} is {}, not a string",
                kind_of(other)
            ));
            String::new()
        }
        None => {
            reasons.push(format!("[{where_}] carries no `{key}`"));
            String::new()
        }
    }
}

fn optional_string(
    reasons: &mut Vec<String>,
    table: &toml::Table,
    where_: &str,
    key: &str,
) -> Option<String> {
    match table.get(key) {
        Some(toml::Value::String(text)) => Some(text.clone()),
        Some(other) => {
            reasons.push(format!(
                "[{where_}] {key} is {}, not a string",
                kind_of(other)
            ));
            None
        }
        None => None,
    }
}

fn duration_key(reasons: &mut Vec<String>, table: &toml::Table, key: &str, default: u64) -> u64 {
    match table.get(key) {
        Some(toml::Value::String(text)) => match parse_duration(text) {
            Ok(seconds) => seconds,
            Err(why) => {
                reasons.push(format!("[order] {key} {why}"));
                default
            }
        },
        Some(other) => {
            reasons.push(format!("[order] {key} is {}, not a string", kind_of(other)));
            default
        }
        None => default,
    }
}

/// `check_unknown_exit`: an integer or a list of them. 0 is refused, because 0
/// is the exit that means due; an empty list is refused, because it reads as a
/// mapping its author wrote and nothing carries.
fn unknown_exits(reasons: &mut Vec<String>, table: &toml::Table) -> Vec<i32> {
    let mut kept: Vec<i32> = Vec::new();
    match table.get("check_unknown_exit") {
        None => return kept,
        Some(toml::Value::Integer(n)) => push_exit(reasons, &mut kept, *n),
        Some(toml::Value::Array(items)) => {
            if items.is_empty() {
                reasons.push(
                    "[order] check_unknown_exit is an empty list; leave the key out to map no exit \
                     to could-not-tell"
                        .to_string(),
                );
            }
            for item in items {
                match item {
                    toml::Value::Integer(n) => push_exit(reasons, &mut kept, *n),
                    other => reasons.push(format!(
                        "[order] check_unknown_exit carries {}, not an integer",
                        kind_of(other)
                    )),
                }
            }
        }
        Some(other) => reasons.push(format!(
            "[order] check_unknown_exit is {}, not an integer or a list of integers",
            kind_of(other)
        )),
    }
    kept
}

fn push_exit(reasons: &mut Vec<String>, kept: &mut Vec<i32>, value: i64) {
    if value == 0 {
        reasons.push(
            "[order] check_unknown_exit names 0, which is the exit that means due".to_string(),
        );
        return;
    }
    match i32::try_from(value) {
        Ok(status) => kept.push(status),
        Err(_) => reasons.push(format!(
            "[order] check_unknown_exit names {value}, which is no exit status"
        )),
    }
}

fn table_at<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Table> {
    table.get(key).and_then(|value| value.as_table())
}

fn kind_of(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "a string",
        toml::Value::Integer(_) => "an integer",
        toml::Value::Float(_) => "a float",
        toml::Value::Boolean(_) => "a boolean",
        toml::Value::Datetime(_) => "a datetime",
        toml::Value::Array(_) => "a list",
        toml::Value::Table(_) => "a table",
    }
}

fn shown(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => format!("`{text}`"),
        toml::Value::Integer(n) => format!("`{n}`"),
        other => kind_of(other).to_string(),
    }
}
