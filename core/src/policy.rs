//! The census: every key the verbs read, listed once, with the only reader
//! that may reach one.
//!
//! A verb that reads a key not listed here is a defect, so the reader answers
//! an unlisted pair with an error and never with a value — the pair is checked
//! before the config is even walked. Two tests hold the other half: that every
//! table named below is one of the eight, and that every call site in this
//! workspace names a listed pair.

use std::fmt;

/// The value type [`read`] answers with, re-exported so a caller can NAME it.
///
/// core is this workspace's TOML reader and the cli takes no `toml` dependency
/// of its own, so without this a caller can only chain methods off the answer —
/// which is enough to take a value and not enough to tell a value of the wrong
/// shape from an absent one in a signature.
pub use toml::Value;

/// The eight tables, and no ninth. `core` and `core.flight` are the fleet's own
/// policy, `core.run` the run lifecycle's, `guards` its opt-outs and
/// `guards.targets` what a pack's guards refuse on, `landing` the landing's CI
/// marker, `permissions` a seat's command words, and `project` the two
/// directories a transient seat is made in.
pub const TABLES: [&str; 8] = [
    "core",
    "core.flight",
    "core.run",
    "guards",
    "guards.targets",
    "landing",
    "permissions",
    "project",
];

/// The pairs the verbs and the guards may read. `guards` is a map keyed by guard
/// name, so its entry is the pattern every guard's row matches.
pub const CENSUS: [(&str, &str); 18] = [
    ("core", "reviewer"),
    // The runs open at once. It is `[core.run]` and not a second key under
    // `[core.flight]` because a run is not a flight: the two caps are set by
    // different people for different reasons, and a fleet that flies one flight
    // at a time still runs four workflows.
    ("core.run", "max_open"),
    // How many times a run that nothing could classify is executed again before
    // the controller parks it. It is `[core.run]` and not a second key under
    // `[core.flight]` for the reason `max_open` is: a crashing workflow and a
    // crashing dispatched seat are two failures with two causes, and a fleet
    // that tolerates one has said nothing about the other.
    ("core.run", "max_crashes"),
    // The `[[core.flight.rules]]` array, which `fleet status` prints off the
    // policy in force.
    ("core.flight", "rules"),
    // The landing lane's two keys. `lanes` is the directory the fleet's own
    // worktrees sit in, read against the machine directory; `rerun_wait_seconds`
    // bounds how long a rerun waits for the box's load to fall before running
    // anyway.
    ("core.flight", "lanes"),
    ("core.flight", "rerun_wait_seconds"),
    ("guards", "*.enabled"),
    // The marker a landing's commit carries. The two TEST commands are in no
    // table: they are the workflow's, handed to `land` and `dispatch` by
    // whatever calls them, and a file that sets either is refused ([`MOVED`]).
    ("landing", "ci_marker"),
    // The command words a transient seat on this project is allowed to run,
    // rendered one `Bash(<word>:*)` rule each into the seat's own permission
    // document. It is the PROJECT's because a toolchain is, the same rule the
    // production-write lists below are read under: a pack reads a list and
    // never hardcodes one.
    ("permissions", "tool_commands"),
    // The targets a pack's two guard classes read: the glob the release-ref
    // class matches a push's destination against, and the three lists the
    // production-write class reads one per check.
    // Core's own two classes need no pair here — the guards wildcard above is
    // their switch and the bare-id check's target is not a policy key.
    ("guards.targets", "release_ref_glob"),
    ("guards.targets", "prod_buckets"),
    ("guards.targets", "prod_projects"),
    ("guards.targets", "prod_apps"),
    // The three surfaces that are a PRODUCT's own rather than a cloud target:
    // the build-tool goals that deploy, the module functions declared as
    // production writes, and the workflow-and-ref pairs a dispatch reaches
    // production through. Lists like the three above, for the same reason —
    // which goals and which functions deploy is a fact about one repository.
    ("guards.targets", "prod_make_goals"),
    ("guards.targets", "prod_dagger_functions"),
    ("guards.targets", "prod_workflow_refs"),
    // Where a spawn cuts a transient seat's worktree from, and where it puts it.
    // Both are paths, and both have an answer derived from the project root when
    // the file names neither.
    ("project", "primary"),
    ("project", "worktrees"),
];

/// The pairs a policy file may NOT set, each with where its value is set
/// instead. They stay named under the table they were set in, so a file that
/// sets one hears where the test command went as well as that its table is
/// gone ([`MOVED_TABLES`]).
///
/// A test command is the workflow's and not the project's: the workflow hands
/// `fleet land --test` the command the landing runs on the tree that lands, and
/// `fleet dispatch --touched` the one the builder's brief names. A file that
/// still sets either is REFUSED rather than ignored — a key nothing reads is a
/// gate a person believes is in force, and the landing it stood behind would
/// go untested with nobody told.
pub const MOVED: [(&str, &str, &str); 2] = [
    (
        "gates",
        "suite",
        "`takeoff.test` under [packs.tiny] in fleet.toml, or `--input test=<command>` on `fleet \
         run takeoff`; a landing run by hand takes `fleet land --test <command>`",
    ),
    (
        "gates",
        "touched",
        "`takeoff.touched` under [packs.tiny] in fleet.toml, or `--input touched=<command>` on \
         `fleet run takeoff`; a dispatch run by hand takes `fleet dispatch --touched <command>`",
    ),
];

/// The pairs a policy file may NOT set because nothing reads them any more:
/// the keys the flight engine read, left behind when it moved out of core.
///
/// Refused for [`MOVED`]'s reason — a key nothing reads is a gate a person
/// believes is in force — and with nowhere to set the value instead, because
/// nothing in the fleet enforces it now.
pub const RETIRED: [(&str, &str); 7] = [
    ("core", "max_returns"),
    ("core.flight", "max_open"),
    ("core.flight", "max_seats"),
    ("core.flight", "review"),
    ("core.flight", "escape_window_days"),
    ("core.flight", "max_crashes"),
    ("project", "trunk"),
];

/// The tables a policy file may NOT carry at all, each with where its keys are
/// set instead.
///
/// `[gates]` held three purposes under one name — the landing's marker, a
/// seat's command words and the targets a pack's guards refuse on — and each
/// key now sits in the table named for its purpose. A file that still carries
/// the table is refused for [`MOVED`]'s reason: every key in it is one nothing
/// reads, so each is a setting a person believes is in force.
pub const MOVED_TABLES: [(&str, &str); 1] = [(
    "gates",
    "`ci_marker` under [landing], `tool_commands` under [permissions], and \
     `release_ref_glob` and the `prod_*` lists under [guards.targets]",
)];

/// A pair [`MOVED`] or [`RETIRED`] names, or a table [`MOVED_TABLES`] names,
/// found set in a policy file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub table: &'static str,
    /// `None` is the whole table, found carried at all.
    pub key: Option<&'static str>,
    /// Where the value is set instead, as a person reads it. `None` is a
    /// [`RETIRED`] pair, whose value is set nowhere.
    pub to: Option<&'static str>,
}

impl fmt::Display for Moved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = self.table;
        match (self.key, self.to) {
            (Some(key), Some(to)) => write!(
                f,
                "[{table}] {key} is not project policy, and nothing reads it — a test command \
                 is the workflow's: set {to}, and delete the key"
            ),
            (Some(key), None) => write!(f, "[{table}] {key} is no longer read — delete it"),
            (None, Some(to)) => write!(
                f,
                "[{table}] is not a policy table, and nothing reads it — its keys are set by \
                 purpose: {to}; move each one there, and delete the table"
            ),
            (None, None) => write!(f, "[{table}] is no longer read — delete it"),
        }
    }
}

/// Every [`MOVED`] pair this config sets, in that table's order, then every
/// [`MOVED_TABLES`] table it carries, then every [`RETIRED`] pair in that
/// table's order.
///
/// A key present with ANY value counts, an empty string included: the refusal
/// is about where the setting lives, and a blank one in the old place is still
/// a person looking for it there. A table counts the same way, an empty one
/// included.
pub fn moved(config: &toml::Table) -> Vec<Moved> {
    let moved = MOVED.iter().map(|(table, key, to)| Moved {
        table,
        key: Some(key),
        to: Some(to),
    });
    let tables = MOVED_TABLES.iter().map(|(table, to)| Moved {
        table,
        key: None,
        to: Some(to),
    });
    let retired = RETIRED.iter().map(|(table, key)| Moved {
        table,
        key: Some(key),
        to: None,
    });
    moved
        .chain(tables)
        .chain(retired)
        .filter(|found| {
            let held = table_in(config, found.table);
            match found.key {
                Some(key) => held.is_some_and(|table| table.contains_key(key)),
                None => held.is_some(),
            }
        })
        .collect()
}

/// The table the config holds under this name, a dotted name walked one
/// segment at a time.
fn table_in<'c>(config: &'c toml::Table, table: &str) -> Option<&'c toml::Table> {
    let mut here = config;
    for segment in table.split('.') {
        here = here.get(segment).and_then(toml::Value::as_table)?;
    }
    Some(here)
}

/// A pair no census row covers. The reader hands this back instead of the
/// value it could have found, because finding it is the defect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unlisted {
    pub table: String,
    pub key: String,
}

impl fmt::Display for Unlisted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} is not in the census — a verb reads no key the census does not name",
            self.table, self.key
        )
    }
}

/// The table and the key are the first two arguments and are written as
/// literals at every call site, so the pair a verb reads can be read out of the
/// source without running it.
pub fn read<'v>(
    table: &str,
    key: &str,
    config: &'v toml::Table,
) -> Result<Option<&'v toml::Value>, Unlisted> {
    if !in_census(table, key) {
        return Err(Unlisted {
            table: table.to_string(),
            key: key.to_string(),
        });
    }
    let mut value: Option<&toml::Value> = None;
    for segment in table.split('.').chain(key.split('.')) {
        let here = match value {
            None => config.get(segment),
            Some(v) => v.as_table().and_then(|t| t.get(segment)),
        };
        match here {
            Some(next) => value = Some(next),
            None => return Ok(None),
        }
    }
    Ok(value)
}

/// A census key may carry one leading `*`, which stands for the one name a map
/// table is keyed by; every other key matches literally.
pub fn in_census(table: &str, key: &str) -> bool {
    CENSUS.iter().any(|(t, k)| {
        *t == table
            && match k.strip_prefix('*') {
                Some(suffix) => key.len() > suffix.len() && key.ends_with(suffix),
                None => *k == key,
            }
    })
}
