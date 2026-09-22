//! The census: every key the verbs read, listed once, with the only reader
//! that may reach one.
//!
//! A verb that reads a key not listed here is a defect, so the reader answers
//! an unlisted pair with an error and never with a value — the pair is checked
//! before the config is even walked. Two tests hold the other half: that every
//! table named below is one of the five, and that every call site in this
//! workspace names a listed pair.

use std::fmt;

/// The value type [`read`] answers with, re-exported so a caller can NAME it.
///
/// core is this workspace's TOML reader and the cli takes no `toml` dependency
/// of its own, so without this a caller can only chain methods off the answer —
/// which is enough to take a value and not enough to tell a value of the wrong
/// shape from an absent one in a signature.
pub use toml::Value;

/// The six tables, and no seventh. `core` and `core.flight` are the fleet's own
/// policy, `core.run` the run lifecycle's, `guards` its opt-outs, `gates` the
/// project's two gates and the targets a pack's guards refuse on, and `project`
/// the two directories a transient seat is made in.
pub const TABLES: [&str; 6] = [
    "core",
    "core.flight",
    "core.run",
    "guards",
    "gates",
    "project",
];

/// The pairs the verbs and the guards may read. `guards` is a map keyed by guard
/// name, so its entry is the pattern every guard's row matches.
pub const CENSUS: [(&str, &str); 27] = [
    ("core", "reviewer"),
    ("core", "max_returns"),
    // The runs open at once. It is `[core.run]` and not a second key under
    // `[core.flight]` because a run is not a flight: the two caps are set by
    // different people for different reasons, and a fleet that flies one flight
    // at a time still runs four workflows.
    ("core.run", "max_open"),
    // How many times a run that nothing could classify is executed again before
    // the controller parks it (controller PRD R36). It is `[core.run]` and not a
    // second key under `[core.flight]` for the reason `max_open` is: a crashing
    // workflow and a crashing dispatched seat are two failures with two causes,
    // and a fleet that tolerates one has said nothing about the other.
    ("core.run", "max_crashes"),
    // The flight keys `fly` reads at takeoff. `max_open` caps the open
    // flights, `review` names the review policy pinned into the snapshot,
    // `escape_window_days` rides the same snapshot, and `rules` is the
    // `[[core.flight.rules]]` array the matcher fills an item's absent keys
    // from (flights PRD R4, S5b).
    ("core.flight", "max_open"),
    ("core.flight", "max_seats"),
    ("core.flight", "review"),
    ("core.flight", "escape_window_days"),
    ("core.flight", "rules"),
    // How many times a crashed item is re-dispatched before it parks (flights
    // PRD R34). The advance reads it off the flight's own pinned snapshot, like
    // every other cap in force.
    ("core.flight", "max_crashes"),
    // The landing lane's two keys (flights PRD R16, R18; S3d). `lanes` is the
    // directory the fleet's own worktrees sit in, read against the machine
    // directory; `rerun_wait_seconds` bounds how long a rerun waits for the
    // box's load to fall before running anyway.
    ("core.flight", "lanes"),
    ("core.flight", "rerun_wait_seconds"),
    ("guards", "*.enabled"),
    // The project's TWO gates, which are not one. `suite` is what the landing
    // runs, once, over the whole tree; `touched` is the command a dispatched
    // seat runs over its own diff before it delivers. A project declaring only
    // the first leaves a seat the brief's derivation sentence and never the
    // landing's suite.
    ("gates", "suite"),
    ("gates", "touched"),
    ("gates", "ci_marker"),
    // The command words a transient seat on this project is allowed to run,
    // rendered one `Bash(<word>:*)` rule each into the seat's own permission
    // document. It is the PROJECT's because a toolchain is, the same rule the
    // three production-write lists above are read under: a pack reads a list
    // and never hardcodes one.
    ("gates", "tool_commands"),
    // The targets a pack's two guard classes read: the glob the release-ref
    // class matches a push's destination against, and the three lists the
    // production-write class reads one per check (packs PRD § The guards).
    // Core's own two classes need no pair here — the guards wildcard above is
    // their switch and the bare-id check's target is not a policy key.
    ("gates", "release_ref_glob"),
    ("gates", "prod_buckets"),
    ("gates", "prod_projects"),
    ("gates", "prod_apps"),
    // The three surfaces that are a PRODUCT's own rather than a cloud target:
    // the build-tool goals that deploy, the module functions declared as
    // production writes, and the workflow-and-ref pairs a dispatch reaches
    // production through. Lists like the three above, for the same reason —
    // which goals and which functions deploy is a fact about one repository.
    ("gates", "prod_make_goals"),
    ("gates", "prod_dagger_functions"),
    ("gates", "prod_workflow_refs"),
    // Where a spawn cuts a transient seat's worktree from, and where it puts it
    // (controller PRD R30). Both are paths, and both have an answer derived from
    // the project root when the file names neither.
    ("project", "primary"),
    ("project", "worktrees"),
    // The trunk strategy, default `advance`; `fly` refuses any other value by
    // name (flights PRD R5).
    ("project", "trunk"),
];

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
