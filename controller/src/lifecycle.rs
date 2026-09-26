//! `fleet create`, `fleet start`, `fleet stop` — everything about them that is
//! not argv.
//!
//! THERE IS NO `fleet install`. The work an install verb would have done — the
//! machine directory, the seat list, the `[seats]` render, the service file and
//! the telemetry disclosure — is [`first_run`], one function `fleet start` calls
//! before it loads anything and a later installer script can call instead.
//! Nothing in it starts anything: load is a second deliberate act.
//!
//! The two writers below are the whole of what `fleet create` decides. Neither
//! touches the project's work-graph store: the controller never initialises and
//! never rewrites one, and nothing here shells out to it.

use crate::events::{self, EventLog};
use crate::policy::{self, Policy};
use crate::{clock, config, platform};
use fleet_core::seat::identity::{roster_in, Kind, SeatRef};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The fleet's own file, embedded beside the work.
pub const FLEET_TOML: &str = "fleet.toml";
/// The project's own declaration, where the fleet stands on its own.
pub const PROJECT_TOML: &str = ".fleet/project.toml";
/// The machine's register of standalone projects.
pub const PROJECTS: &str = "projects.toml";

/// How long a load or an unload is given to show up on the stream.
///
/// Not the load command's own exit, which returns as soon as the manager has
/// accepted the job: what is waited for is the controller's own first line, and
/// a poll interval plus a cold start fits inside this with room over.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the wait re-reads the stream.
const CONFIRM_SLICE: Duration = Duration::from_millis(100);

/// Which file `fleet create` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Embedded,
    Standalone,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Embedded => "embedded",
            Mode::Standalone => "standalone",
        }
    }
}

// ---- the two writers --------------------------------------------------------

/// The `Written by` line's subject: the flags a caller actually passed, plus a
/// clause for every answer that came from a prompt instead.
///
/// A header naming a flag nobody typed is a claim a later reader cannot check
/// against anything, and it is the first line of the file `create` leaves behind.
pub fn written_by(flags: &[&str], asked: bool) -> String {
    let call = if flags.is_empty() {
        "`fleet create`".to_string()
    } else {
        format!("`fleet create {}`", flags.join(" "))
    };
    match (asked, flags.is_empty()) {
        (false, _) => call,
        (true, true) => format!("{call}, answered at its prompts"),
        (true, false) => format!("{call}, the rest answered at its prompts"),
    }
}

/// An embedded fleet's policy file: the smallest file that runs.
///
/// Every key the controller defaults is LEFT OUT — the poll interval, the
/// models, the postures, the windows — because a written default is a value
/// nobody chose that a reader has to check against the code anyway. What is
/// written is the two things the defaults would get wrong: the guards, which are
/// opt-out and so have to be visible to be opted out of, and telemetry, which is
/// off and said.
///
/// The agent is written on a line of its own rather than read out of the
/// invocation, because it is a choice this file records and the invocation is
/// only how the choice was made.
///
/// `store` is the store adapter `fleet create` installed a pack for, written as
/// `[store] adapter`; `None` writes no table, for a fleet whose store pack is
/// installed later.
pub fn embedded_text(agent: &str, store: Option<&str>, written_by: &str) -> String {
    format!(
        "# This fleet is EMBEDDED: this file is its policy and it sits at the\n\
         # project's root, so the fleet and the project are one directory.\n\
         # Its agent is `{agent}`.\n\
         # Written by {written_by}.\n\
         #\n\
         # Every key the controller defaults is left out on purpose. Add one here\n\
         # to override it for this fleet.\n\
         \n\
         # Opt-out, never opt-in: a guard is on unless this file turns it off.\n\
         [guards]\n\
         shell-trap.enabled = true\n\
         record.enabled = true\n\
         \n\
         # Off, and said. No metric leaves this machine.\n\
         [telemetry]\n\
         enabled = false\n\
         \n\
         {store}\
         # One table per seat, keyed by the seat's id. fleet seat add writes them\n\
         # and fleet start renders the agent seats into the machine's seat list.\n\
         # A row looks like this:\n\
         #\n\
         #   [seats.01a0d1f1-0aec-765f-9abe-d4f993b9739a]\n\
         #   kind = \"agent\"\n\
         #   name = \"what a person calls it\"\n\
         #   model = \"{model}\"\n\
         #   status = \"active\"\n\
         [seats]\n",
        model = policy::DEFAULT_MODEL,
        store = store_table(store),
    )
}

/// The `[store]` table naming the store adapter `fleet create` installed, the
/// line above it saying what the name is and a blank line after it; the empty
/// string where it installed none.
fn store_table(store: Option<&str>) -> String {
    match store {
        Some(name) => format!(
            "# The store this fleet's items live in: the store adapter an installed\n\
             # pack carries under this name.\n\
             [store]\n\
             adapter = \"{}\"\n\
             \n",
            basic(name)
        ),
        None => String::new(),
    }
}

/// A standalone project's declaration.
///
/// The two directories are written OUT even though an embedded fleet derives
/// them from where its own file sits: a standalone fleet's file sits somewhere
/// else entirely, so there is nothing here for it to derive them from.
///
/// `store` as [`embedded_text`] takes it: the `[store]` table goes in the
/// project's own file, which is the one the store is opened by.
pub fn project_text(
    name: &str,
    item_prefix: Option<&str>,
    primary: &Path,
    worktrees: &Path,
    store: Option<&str>,
    written_by: &str,
) -> String {
    let prefix = match item_prefix {
        Some(prefix) => format!("item_prefix = \"{}\"\n", basic(prefix)),
        // Left for the person, with the line said: a prefix this file guessed
        // is one the record's own ids would disagree with.
        None => "# item_prefix = \"the prefix this project's items carry\"\n".to_string(),
    };
    format!(
        "# This project is declared to the STANDALONE fleet this machine runs.\n\
         # Written by {written_by}.\n\
         \n\
         [project]\n\
         name = \"{name}\"\n\
         {prefix}\
         primary = \"{primary}\"\n\
         worktrees = \"{worktrees}\"\n\
         \n\
         {store}\
         # Owed, and left for the person: a marker this file guessed would\n\
         # skip a pipeline nobody chose to skip. The test commands are not set\n\
         # here — a workflow hands them to the landing; for takeoff they are\n\
         # takeoff.test and takeoff.touched under [packs.tiny] in fleet.toml.\n\
         [landing]\n\
         # ci_marker = \"the marker a landing's commit carries\"\n",
        name = basic(name),
        primary = basic(&primary.display().to_string()),
        worktrees = basic(&worktrees.display().to_string()),
        store = store_table(store),
    )
}

/// One value inside a TOML basic string.
///
/// A directory basename is whatever a person named it, and a quote or a
/// backslash in one writes a file that does not parse — which is a declaration
/// every later verb refuses, from a name nobody thought was special. The set is
/// the format's own: the two delimiters and the control characters, each as the
/// short escape where it has one.
pub fn basic(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// A declaration somebody already wrote, read for the keys a registration needs.
///
/// A `.fleet/project.toml` that is already there is a DECLARED PROJECT and not a
/// collision: `fleet create --standalone` registers it rather than writing over
/// it, so what this reads is the one key the register keys a row on. A file that
/// does not parse, or that names no project, is refused with what is missing.
pub fn validate_declaration(path: &Path) -> Result<String, String> {
    declared_string(&parsed_declaration(path)?, path, "name")
}

/// Every key a declaration somebody wrote has to carry, in the order they are
/// read — and the order a person meets them, because the first missing one is
/// the whole refusal.
pub const DECLARED_KEYS: [&str; 4] = ["name", "item_prefix", "primary", "worktrees"];

/// The same file, read for EVERY key a registration needs rather than the one
/// the register is keyed on.
///
/// Held to this only where a person wrote the file: the declaration this verb
/// writes itself leaves the prefix commented for the person to fill in and names
/// a worktrees directory that is made on the first spawn, so a registration is
/// not the moment either exists.
///
/// `primary` is a checkout this machine has; `worktrees` is a directory the
/// first spawn makes, so it is read as a place rather than as a thing that is
/// there — a FILE at that path is the declaration nothing can work under.
pub fn validate_written_declaration(path: &Path) -> Result<String, String> {
    let table = parsed_declaration(path)?;
    let mut values = Vec::new();
    for key in DECLARED_KEYS {
        values.push(declared_string(&table, path, key)?);
    }
    let primary = Path::new(&values[2]);
    if !primary.is_dir() {
        return Err(format!(
            "{} names `[project] primary` {}, which is not a directory on this machine",
            path.display(),
            primary.display()
        ));
    }
    let worktrees = Path::new(&values[3]);
    if worktrees.exists() && !worktrees.is_dir() {
        return Err(format!(
            "{} names `[project] worktrees` {}, and a seat's checkout cannot be cut inside a file",
            path.display(),
            worktrees.display()
        ));
    }
    Ok(values.swap_remove(0))
}

fn parsed_declaration(path: &Path) -> Result<toml::Table, String> {
    let body = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    body.parse()
        .map_err(|e| format!("{} does not parse: {e}", path.display()))
}

/// One `[project]` key as a value, or the refusal NAMING IT: a person fixes the
/// key the sentence spells, and "something is missing" is not that sentence.
fn declared_string(table: &toml::Table, path: &Path, key: &str) -> Result<String, String> {
    table
        .get("project")
        .and_then(|project| project.get(key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "{} carries no `[project] {key}`, which a project declared to this fleet needs",
                path.display()
            )
        })
}

// ---- the registry -----------------------------------------------------------

/// One registered project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub root: String,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    project: Vec<Project>,
}

/// Every project this machine's fleet has registered.
pub fn registered(machine_dir: &Path) -> Result<Vec<Project>, String> {
    let path = machine_dir.join(PROJECTS);
    match std::fs::read_to_string(&path) {
        Ok(body) => Ok(toml::from_str::<Registry>(&body)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .project),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Append one project to the register. `Ok(false)` is a root already registered,
/// which is a second `fleet create --standalone` in the same project and not a
/// failure.
///
/// UNDER THE LOCK, because this is a read-modify-write and the atomic rename
/// under it is not: two registrations that each read the same file would leave
/// one row.
///
/// THE TABLE'S TEXT IS APPENDED, never the document re-serialized — the same
/// discipline [`crate::config::render_seats`] is under, and for the same
/// reason: a rewrite through a type this module declares drops every comment a
/// person wrote and every key that type does not model.
pub fn register(machine_dir: &Path, root: &Path, name: &str) -> Result<bool, String> {
    let path = machine_dir.join(PROJECTS);
    let _held = platform::lock_beside(&path)?;
    if registered(machine_dir)?
        .iter()
        .any(|p| p.root == root.display().to_string())
    {
        return Ok(false);
    }
    let mut body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() {
        body.push('\n');
    }
    body.push_str(&format!(
        "[[project]]\nroot = \"{}\"\nname = \"{}\"\n",
        basic(&root.display().to_string()),
        basic(name)
    ));
    platform::write_atomic(&path, body.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // The read-back this write owes its own record: a file that was written and
    // does not parse is a register the next registration would refuse to read.
    registered(machine_dir)?
        .iter()
        .any(|p| p.root == root.display().to_string())
        .then_some(true)
        .ok_or_else(|| format!("{} was written and does not carry the row", path.display()))
}

/// Write the `project.registered` line the registry owes the stream.
pub fn registered_event(machine_dir: &Path, root: &Path, name: &str) -> Result<(), String> {
    EventLog::open(&machine_dir.join("events.jsonl"))
        .append(
            events::PROJECT_REGISTERED,
            &events::controller(machine_dir),
            serde_json::json!({ "root": root.display().to_string(), "name": name }),
        )
        .map_err(|e| e.to_string())
}

// ---- the first run ----------------------------------------------------------

/// What the first run is given.
pub struct FirstRun<'a> {
    pub machine_dir: &'a Path,
    /// The policy file the seat list will name, absolute.
    pub fleet_toml: &'a Path,
    /// The project a rendered seat's worktree is keyed under, and where that
    /// worktree goes. `None` where the caller resolved no project — a start run
    /// from outside every project of a standalone fleet — and then no row is
    /// rendered, because a row's worktrees map has no project to key on and a
    /// guessed one puts a seat in the wrong checkout.
    pub project: Option<ProjectAt<'a>>,
    pub policy: &'a Policy,
    pub service: &'a platform::Service,
}

/// The project a render keys its worktrees under.
pub struct ProjectAt<'a> {
    pub name: &'a str,
    pub worktrees_dir: &'a Path,
}

/// What the first run did, step by step.
pub struct FirstRunReport {
    /// One line per step, each saying whether it did anything.
    pub lines: Vec<String>,
    /// True where any step wrote. A second start moves nothing and says so.
    pub changed: bool,
}

/// The install work, in one function, with NOTHING STARTED (lessons gas-city
/// G1).
///
/// Every step is idempotent, so a second `fleet start` repeats none of it and
/// says so line by line. The order is the one a reader needs: the directory
/// before the file that lives in it, the seat list before the rows rendered into
/// it, and the service file last — written, and not loaded.
pub fn first_run(run: &FirstRun) -> Result<FirstRunReport, String> {
    let mut lines = Vec::new();
    let mut changed = false;

    std::fs::create_dir_all(run.machine_dir)
        .map_err(|e| format!("{}: {e}", run.machine_dir.display()))?;
    lines.push(format!("machine directory: {}", run.machine_dir.display()));

    let config_path = run.machine_dir.join("config.json");
    if write_seat_list(&config_path, run.fleet_toml)? {
        changed = true;
        lines.push(format!("seat list: written at {}", config_path.display()));
    } else {
        lines.push(format!("seat list: already at {}", config_path.display()));
    }

    // WHICH PROJECT THE ROWS ARE KEYED ON, said before they are counted: the
    // project a row's worktrees map is keyed under is not always the directory
    // the start was run in, and a row keyed on the wrong project spawns a
    // session into a checkout that does not exist.
    if let Some(project) = &run.project {
        lines.push(format!(
            "project: rows keyed on {} at {}",
            project.name,
            project.worktrees_dir.display()
        ));
    }

    let (seats, humans) = rendered_seats(run)?;
    let render = config::render_seats(&config_path, &seats)?;
    changed |= render.moved();
    let mut said = match (seats.len(), render.moved()) {
        (0, _) if run.project.is_none() => "seats: no project resolves from this directory, so \
             no row was rendered — run `fleet start` from inside a registered project to render \
             them"
            .to_string(),
        (0, _) => "seats: the policy names no agent seat, so no row was rendered".to_string(),
        (n, false) => format!("seats: {n} row(s) already rendered"),
        (n, true) => format!(
            "seats: {n} row(s) rendered — {} added, {} updated, {} dropped",
            render.added.len(),
            render.updated.len(),
            render.dropped.len()
        ),
    };
    // A person's seat is on the roster and on no row, and the line says so
    // rather than leave a count that reads one short.
    if humans > 0 {
        said.push_str(&format!(
            "; {humans} human seat(s) listed and not rendered — the controller runs agent seats \
             only"
        ));
    }
    lines.push(said);

    let wrote = run.service.write()?;
    changed |= wrote;
    lines.push(format!(
        "service file: {} at {}",
        if wrote { "written" } else { "already" },
        run.service.file().display()
    ));
    lines.extend(run.service.after_write());
    // The one line that is a promise rather than a report: this function loads
    // nothing, and the arm named for it counts the manager's calls.
    lines.push(format!(
        "nothing was started: `fleet start` loads {} as its second act",
        run.service.label()
    ));

    let (said, wrote) = telemetry_disclosure(run.fleet_toml)?;
    changed |= wrote;
    lines.push(said);

    Ok(FirstRunReport { lines, changed })
}

/// The seat list naming the fleet this machine runs, written where none is
/// there. `Ok(false)` is a file already standing, which is left exactly as it
/// is: its `children` are the rendered rows and its `[controller]` object is
/// the machine's own overrides, and neither is this writer's to replace.
///
/// THE ONE WRITER OF THIS RECORD, because `fleet create --standalone` writes it
/// before the first start on a machine that has no fleet yet: two writers would
/// be two shapes of the same file, and the one the controller reads is this.
pub fn write_seat_list(config_path: &Path, fleet_toml: &Path) -> Result<bool, String> {
    if config_path.exists() {
        return Ok(false);
    }
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "fleet_toml": fleet_toml.display().to_string(),
            "children": [],
        }))
        .map_err(|e| e.to_string())?
    );
    platform::write_atomic(config_path, body.as_bytes())
        .map_err(|e| format!("{}: {e}", config_path.display()))?;
    Ok(true)
}

/// The active agent seats of the fleet's policy file, as rows, and beside them
/// how many human seats it lists.
///
/// THE SEATS ARE READ THROUGH CORE'S ROSTER and nowhere else [ASSUMES D2], so
/// the clean break is refused here as everywhere: a table keyed by a name
/// stops the start rather than being read around. The roster is read before
/// the project is asked for, so a start that renders nothing refuses it too.
///
/// A PERSON'S SEAT IS LISTED AND NEVER RENDERED: the controller runs agent
/// seats only, and a parked agent is kept and not run.
///
/// RENDERING IS `fleet start`'S ACT and never the loop's: the controller re-reads
/// `config.json` and never this table, so a changed `[seats]` table is followed
/// by a stop and a start.
fn rendered_seats(run: &FirstRun) -> Result<(Vec<config::RenderedSeat>, usize), String> {
    let body = std::fs::read_to_string(run.fleet_toml)
        .map_err(|e| format!("{}: {e}", run.fleet_toml.display()))?;
    let roster = roster_in(&body)?;
    let humans = roster
        .iter()
        .filter(|seat| seat.seat.kind == Kind::Human)
        .count();
    let Some(at) = &run.project else {
        return Ok((Vec::new(), humans));
    };
    let rows = roster
        .into_iter()
        .filter(|seat| seat.seat.kind == Kind::Agent && !seat.parked)
        .map(|seat| config::RenderedSeat {
            id: seat.seat.id,
            name: seat.seat.name.clone(),
            model: run.policy.model_for(seat.model.as_deref()),
            worktrees: vec![(
                at.name.to_string(),
                worktree_for(at.worktrees_dir, &seat.seat)
                    .display()
                    .to_string(),
            )],
        })
        .collect();
    Ok((rows, humans))
}

/// A seat's worktree under `dir`: the one entry already there whose name ends
/// in `-<short>`, and `<dir>/<machine name>` where there is none — or more than
/// one, which is no answer either.
///
/// A RENAME MOVES NO WORKTREE [ASSUMES D1]. The slug is the part of a machine
/// name a person can change and the short id the part they cannot, so a
/// worktree made under the seat's old name is found by the part it kept.
fn worktree_for(dir: &Path, seat: &SeatRef) -> PathBuf {
    let tail = format!("-{}", seat.id.short());
    let fallback = || dir.join(seat.machine_name());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return fallback();
    };
    let mut found = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(&tail));
    match (found.next(), found.next()) {
        (Some(only), None) => only.path(),
        _ => fallback(),
    }
}

/// Telemetry: off, and asked (lessons gas-city G2).
///
/// NOTHING IS ASKED, and that is the disclosure. A question with nothing behind
/// it — no metric is collected, none is sent, and there is no endpoint — is a
/// consent dialog for an act that does not happen; what a person is owed is the
/// key, so they can find it when a later release would have to change it. The
/// key is written false where the file does not carry it, so the answer is on
/// the record and not merely in this sentence.
fn telemetry_disclosure(fleet_toml: &Path) -> Result<(String, bool), String> {
    let body = std::fs::read_to_string(fleet_toml)
        .map_err(|e| format!("{}: {e}", fleet_toml.display()))?;
    let table: toml::Table = body
        .parse()
        .map_err(|e| format!("{}: {e}", fleet_toml.display()))?;
    let telemetry = table.get("telemetry");

    // THE VALUE, never the key's presence: a file that says `enabled = true` is
    // reported as saying true, whatever this fleet does about it, or the line
    // announces the opposite of what the record holds.
    if let Some(stated) = telemetry.and_then(|t| t.get("enabled")) {
        let reads = stated.as_bool();
        return Ok((
            match reads {
                Some(false) => format!(
                    "telemetry: off — nothing leaves this machine; `[telemetry] enabled` reads \
                     false in {}",
                    fleet_toml.display()
                ),
                Some(true) => format!(
                    "telemetry: `[telemetry] enabled` reads TRUE in {} — nothing in this fleet \
                     sends a metric and no code reads that key to send one, so the file says \
                     one thing and the fleet does another",
                    fleet_toml.display()
                ),
                None => format!(
                    "telemetry: `[telemetry] enabled` in {} is {}, which is neither true nor \
                     false; nothing leaves this machine either way",
                    fleet_toml.display(),
                    stated.type_str()
                ),
            },
            false,
        ));
    }

    // A `[telemetry]` table that is there without the key takes the key INSIDE
    // it: a second `[telemetry]` header appended at the end is a duplicate
    // table, which TOML forbids — every later read of this file, the
    // controller's own re-read included, would then fail on a file this
    // function corrupted.
    if telemetry.is_some() {
        let Some(header) = header_line(&body, "telemetry") else {
            return Ok((
                format!(
                    "telemetry: off — nothing leaves this machine; `[telemetry]` in {} is written \
                     as a dotted key, so `enabled = false` was not added and is yours to write",
                    fleet_toml.display()
                ),
                false,
            ));
        };
        let mut lines: Vec<&str> = body.lines().collect();
        lines.insert(header + 1, "enabled = false");
        let whole = format!("{}\n", lines.join("\n"));
        return write_back(
            fleet_toml,
            &whole,
            "written false into the table already there",
        );
    }

    let mut whole = body;
    if !whole.ends_with('\n') {
        whole.push('\n');
    }
    whole.push_str("\n[telemetry]\nenabled = false\n");
    write_back(fleet_toml, &whole, "written false")
}

/// The index of the line that opens `[<name>]`, or `None` where the table is
/// declared some other way.
///
/// `[ telemetry ]` and `[telemetry]  # our knobs` are the same header to TOML,
/// so they are the same header here: a caller that misses them takes its
/// dotted-key branch and tells a person their file holds a dotted key they
/// never wrote.
fn header_line(body: &str, name: &str) -> Option<usize> {
    body.lines().position(|line| {
        let Some((inside, after)) = line
            .trim_start()
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
        else {
            return false;
        };
        let after = after.trim();
        inside.trim() == name && (after.is_empty() || after.starts_with('#'))
    })
}

/// Write the policy file back, and READ IT BACK as TOML before saying it landed:
/// this function is the one place `fleet start` edits a person's own file, and a
/// file it made unparsable is a fleet that cannot start again.
fn write_back(fleet_toml: &Path, whole: &str, how: &str) -> Result<(String, bool), String> {
    whole.parse::<toml::Table>().map_err(|e| {
        format!(
            "{} would not parse after the edit: {e}",
            fleet_toml.display()
        )
    })?;
    platform::write_atomic(fleet_toml, whole.as_bytes())
        .map_err(|e| format!("{}: {e}", fleet_toml.display()))?;
    Ok((
        format!(
            "telemetry: off — nothing leaves this machine; `[telemetry] enabled` was {how} in {}",
            fleet_toml.display()
        ),
        true,
    ))
}

// ---- load, confirm, unload --------------------------------------------------

/// Why a load or an unload could not be confirmed.
pub struct NotConfirmed {
    pub why: String,
    /// Where a person reads what the service itself printed.
    pub stderr: String,
    pub stream: PathBuf,
}

impl NotConfirmed {
    pub fn sentence(&self) -> String {
        format!(
            "{} — nothing fresh reached {}; what the service printed is at {}",
            self.why,
            self.stream.display(),
            self.stderr
        )
    }
}

/// The highest sequence the stream holds right now — the line a confirmation
/// has to be ABOVE.
///
/// Read BEFORE the load, because an old `controller.started` from a previous
/// run is exactly what a confirmation must not accept.
pub fn stream_head(machine_dir: &Path) -> u64 {
    EventLog::open(&machine_dir.join("events.jsonl")).seq()
}

/// Wait for one fresh event of `kind` above `above`.
///
/// NEVER THE LOAD COMMAND'S OWN EXIT: the manager answers as soon as it has
/// accepted the job, which says nothing about whether the controller came up.
pub fn confirm(
    machine_dir: &Path,
    kind: &str,
    above: u64,
    timeout: Duration,
) -> Result<u64, NotConfirmed> {
    let stream = machine_dir.join("events.jsonl");
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(seq) = events::read_after(&stream, above)
            .into_iter()
            .find(|record| record.kind == kind)
            .map(|record| record.seq)
        {
            return Ok(seq);
        }
        if Instant::now() >= deadline {
            return Err(NotConfirmed {
                why: format!("no {kind} was written within {timeout:?}"),
                stderr: platform::Service::stderr_of(machine_dir),
                stream,
            });
        }
        std::thread::sleep(CONFIRM_SLICE);
    }
}

/// The stamp the projection was last published at, which is the last tick a
/// person can point to. Read FROM THE FILE and never from the process table: a
/// pid is not a tick.
pub fn last_tick(machine_dir: &Path) -> Option<String> {
    let body = std::fs::read_to_string(machine_dir.join("projection.json")).ok()?;
    let document: serde_json::Value = serde_json::from_str(&body).ok()?;
    document
        .get("generated_at")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The stamp a lock entry and an event carry.
pub fn stamp() -> String {
    clock::now_stamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header claiming a flag nobody passed is the only thing a later reader
    /// has to go on, so an answer given at a prompt is said to be one.
    #[test]
    fn the_written_by_line_names_only_the_flags_the_call_carried() {
        assert_eq!(
            written_by(&["--embedded", "--agent claude_code"], false),
            "`fleet create --embedded --agent claude_code`"
        );
        assert_eq!(
            written_by(&[], true),
            "`fleet create`, answered at its prompts"
        );
        assert_eq!(
            written_by(&["--embedded"], true),
            "`fleet create --embedded`, the rest answered at its prompts"
        );
        for line in [written_by(&[], true), written_by(&["--embedded"], true)] {
            assert!(
                !line.contains("--agent"),
                "a flag nobody passed is not named: {line}"
            );
        }

        // Every rendering reaches the file the same way, so the arm above is
        // about what `create` writes and not about a helper nobody calls.
        let asked = embedded_text("claude_code", None, &written_by(&[], true));
        assert!(
            asked.contains("# Written by `fleet create`, answered at its prompts."),
            "{asked}"
        );
        assert!(!asked.contains("--embedded"), "{asked}");
        assert!(!asked.contains("--agent"), "{asked}");
        assert!(
            asked.contains("# Its agent is `claude_code`."),
            "the choice survives the flag it was not made with: {asked}"
        );
        let declared = project_text(
            "a-project",
            None,
            Path::new("/p/a-project"),
            Path::new("/p/wt"),
            None,
            &written_by(&[], true),
        );
        assert!(
            declared.contains("# Written by `fleet create`, answered at its prompts."),
            "{declared}"
        );
        assert!(!declared.contains("--standalone"), "{declared}");
    }

    #[test]
    fn the_embedded_file_names_the_mode_the_command_the_guards_and_telemetry() {
        let text = embedded_text(
            "claude_code",
            Some("tk"),
            &written_by(&["--embedded", "--agent claude_code"], false),
        );
        assert!(text.contains("EMBEDDED"), "{text}");
        assert!(
            text.contains("[store]\nadapter = \"tk\"\n\n# One table per seat"),
            "the store create installed is named, before the seats table: {text}"
        );
        let storeless = embedded_text(
            "claude_code",
            None,
            &written_by(&["--embedded", "--agent claude_code"], false),
        );
        assert!(
            !storeless.contains("[store]") && storeless.contains("enabled = false\n\n# One table"),
            "no store installed, no table: {storeless}"
        );
        assert!(
            text.contains("fleet create --embedded --agent claude_code"),
            "{text}"
        );
        assert!(
            text.contains("[guards]\nshell-trap.enabled = true\nrecord.enabled = true"),
            "{text}"
        );
        assert!(text.contains("[telemetry]\nenabled = false"), "{text}");
        assert!(
            text.ends_with(&format!(
                "\n\
                 # One table per seat, keyed by the seat's id. fleet seat add writes them\n\
                 # and fleet start renders the agent seats into the machine's seat list.\n\
                 # A row looks like this:\n\
                 #\n\
                 #   [seats.01a0d1f1-0aec-765f-9abe-d4f993b9739a]\n\
                 #   kind = \"agent\"\n\
                 #   name = \"what a person calls it\"\n\
                 #   model = \"{}\"\n\
                 #   status = \"active\"\n\
                 [seats]\n",
                policy::DEFAULT_MODEL
            )),
            "the file ends on the seats table, keyed by an id: {text}"
        );

        // Nothing the controller defaults: the keys a reader would otherwise
        // check against the code are absent, and the file still parses.
        for key in ["poll_seconds", "posture", "first_turn"] {
            assert!(
                !text.contains(&format!("\n{key} =")),
                "{key} is defaulted: {text}"
            );
        }
        let policy = policy::parse(&text).expect("the written file parses as policy");
        assert_eq!(policy.poll_seconds, policy::DEFAULT_POLL_SECONDS);
        assert!(roster_in(&text)
            .expect("the seats table parses through core's roster")
            .is_empty());

        // The two guards read as ON through core's own reader, which is the
        // whole point of writing them: the spellings here are the ones it walks.
        let table: toml::Table = text.parse().expect("the file is a table");
        assert_eq!(
            table["guards"]["shell-trap"]["enabled"].as_bool(),
            Some(true),
            "the guard spelling core's reader walks: {text}"
        );
        assert_eq!(table["guards"]["record"]["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn the_standalone_file_declares_the_project_and_leaves_the_gates_owed() {
        let text = project_text(
            "a-project",
            Some("ap"),
            Path::new("/p/a-project"),
            Path::new("/p/a-project-worktrees"),
            Some("tk"),
            &written_by(&["--standalone"], false),
        );
        assert!(text.contains("name = \"a-project\""), "{text}");
        assert!(text.contains("item_prefix = \"ap\""), "{text}");
        assert!(text.contains("primary = \"/p/a-project\""), "{text}");
        assert!(
            text.contains("worktrees = \"/p/a-project-worktrees\""),
            "{text}"
        );
        assert!(
            !text.contains("suite =") && text.contains("takeoff.test"),
            "no test command is written or owed here, and the file says where it is set: {text}"
        );
        assert!(text.contains("# ci_marker ="), "{text}");

        // A project whose store says no prefix gets the line said and not a
        // guess: the commented form is what a person fills in.
        let unsaid = project_text(
            "a-project",
            None,
            Path::new("/p/a-project"),
            Path::new("/p/wt"),
            None,
            &written_by(&["--standalone"], false),
        );
        assert!(unsaid.contains("# item_prefix ="), "{unsaid}");
        assert!(!unsaid.contains("\nitem_prefix ="), "{unsaid}");
        assert!(!unsaid.contains("[store]"), "no store installed: {unsaid}");

        let table: toml::Table = text.parse().expect("the written file parses");
        assert_eq!(table["project"]["item_prefix"].as_str(), Some("ap"));
        assert_eq!(
            table["store"]["adapter"].as_str(),
            Some("tk"),
            "the project's own file names its store, which is the file the store opens by: {text}"
        );
        assert!(
            table["landing"].as_table().expect("a table").is_empty(),
            "the marker is commented: {text}"
        );
    }

    /// A root is registered once. A second registration of the same project is
    /// the same file and not a second row.
    #[test]
    fn a_project_is_registered_once_and_read_back() {
        let dir = std::env::temp_dir().join(format!("fleet-registry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            registered(&dir).expect("an absent register is empty"),
            vec![]
        );
        assert!(register(&dir, Path::new("/p/one"), "one").expect("it registers"));
        assert!(!register(&dir, Path::new("/p/one"), "one").expect("the same root is already in"));
        assert!(register(&dir, Path::new("/p/two"), "two").expect("a second root registers"));

        let rows = registered(&dir).expect("the register reads back");
        assert_eq!(
            rows,
            vec![
                Project {
                    root: "/p/one".to_string(),
                    name: "one".to_string()
                },
                Project {
                    root: "/p/two".to_string(),
                    name: "two".to_string()
                },
            ]
        );
        let body = std::fs::read_to_string(dir.join(PROJECTS)).unwrap();
        assert_eq!(
            body.matches("[[project]]").count(),
            2,
            "one table per project: {body}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The disclosure writes the key ONCE, and says which of the two it did.
    #[test]
    fn the_telemetry_key_is_written_false_when_the_file_does_not_carry_it() {
        let dir = std::env::temp_dir().join(format!("fleet-telemetry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FLEET_TOML);

        std::fs::write(&path, "[controller]\npoll_seconds = 5\n").unwrap();
        let (said, wrote) = telemetry_disclosure(&path).expect("it reads");
        assert!(wrote);
        assert!(said.contains("nothing leaves this machine"), "{said}");
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("[telemetry]\nenabled = false"), "{body}");
        assert!(
            body.contains("poll_seconds = 5"),
            "the file it was added to: {body}"
        );

        // A second run neither writes nor changes what it says it did.
        let (said, wrote) = telemetry_disclosure(&path).expect("it reads");
        assert!(!wrote);
        assert!(said.contains("nothing leaves this machine"), "{said}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);

        // And it is never written TRUE, whatever the file said before.
        assert!(!body.contains("enabled = true"), "{body}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `[telemetry]` TABLE that is already there takes the key INSIDE it.
    ///
    /// A second `[telemetry]` header appended at the end is a duplicate table,
    /// which TOML forbids — so the file this function wrote would fail its own
    /// parser, the controller's re-read and every later `fleet start`. The
    /// reading that says so is the parse afterwards.
    #[test]
    fn a_telemetry_table_without_the_key_takes_it_and_the_file_still_parses() {
        let dir = std::env::temp_dir().join(format!("fleet-telemetry-2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FLEET_TOML);

        // The table is there, and the key is not: a commented line and a key of
        // somebody else's are both this shape.
        std::fs::write(
            &path,
            "[controller]\npoll_seconds = 5\n\n[telemetry]\n# enabled = false\nendpoint = \"none\"\n",
        )
        .unwrap();
        let (said, wrote) = telemetry_disclosure(&path).expect("it reads");
        assert!(wrote, "{said}");
        let body = std::fs::read_to_string(&path).unwrap();

        // The reading the finding is about: ONE header, and the file parses.
        assert_eq!(
            body.matches("[telemetry]").count(),
            1,
            "a second table header would be a duplicate key: {body}"
        );
        let table: toml::Table = body.parse().expect("the edited file still parses");
        assert_eq!(table["telemetry"]["enabled"].as_bool(), Some(false));
        assert_eq!(
            table["telemetry"]["endpoint"].as_str(),
            Some("none"),
            "the key already in the table survives: {body}"
        );
        assert_eq!(table["controller"]["poll_seconds"].as_integer(), Some(5));

        // A second run reads the key it just wrote and changes nothing.
        let (_, wrote) = telemetry_disclosure(&path).expect("it reads");
        assert!(!wrote);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A header stays a header through inner spaces and a trailing comment,
    /// both of which are valid TOML: each takes the key INSIDE the table, and
    /// a file that really is a dotted key still takes the other branch.
    #[test]
    fn a_spaced_or_commented_telemetry_header_takes_the_key_inside_the_table() {
        let dir = std::env::temp_dir().join(format!("fleet-telemetry-4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FLEET_TOML);

        // The plain header is the third row and not a fourth test: a green that
        // covered only the two malformed shapes would read the same as a reader
        // that accepts anything bracketed.
        for (shape, header) in [
            ("inner spaces", "[ telemetry ]"),
            ("a trailing comment", "[telemetry]  # our knobs"),
            ("the plain header", "[telemetry]"),
        ] {
            std::fs::write(
                &path,
                format!("[controller]\npoll_seconds = 5\n\n{header}\nendpoint = \"none\"\n"),
            )
            .unwrap();
            let (said, wrote) = telemetry_disclosure(&path).expect("it reads");
            assert!(wrote, "{shape}: the key was not written: {said}");
            assert!(
                !said.contains("dotted key"),
                "{shape} is not a dotted key: {said}"
            );

            let body = std::fs::read_to_string(&path).unwrap();
            assert_eq!(
                body.matches("telemetry").count(),
                1,
                "{shape}: one table header, not a duplicate: {body}"
            );
            assert!(
                body.contains(header),
                "{shape}: the header as it was written survives: {body}"
            );
            let table: toml::Table = body.parse().expect("the edited file still parses");
            assert_eq!(
                table["telemetry"]["enabled"].as_bool(),
                Some(false),
                "{shape}: {body}"
            );
            assert_eq!(
                table["telemetry"]["endpoint"].as_str(),
                Some("none"),
                "{shape}: the key already in the table survives: {body}"
            );
            assert_eq!(table["controller"]["poll_seconds"].as_integer(), Some(5));
        }

        // The control the three rows need: a table declared as a dotted key
        // really does take the dotted-key branch, so the reader tells the
        // shapes apart rather than reporting every file as a header it found.
        let dotted = "telemetry.endpoint = \"none\"\n";
        std::fs::write(&path, dotted).unwrap();
        let (said, wrote) = telemetry_disclosure(&path).expect("it reads");
        assert!(!wrote, "a dotted key is not edited: {said}");
        assert!(said.contains("written as a dotted key"), "{said}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            dotted,
            "the file is left untouched: {said}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The line reports the key's VALUE and not its presence: a file that says
    /// `enabled = true` is reported as saying true.
    #[test]
    fn the_disclosure_reads_the_value_and_never_announces_a_true_key_as_off() {
        let dir = std::env::temp_dir().join(format!("fleet-telemetry-3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FLEET_TOML);

        std::fs::write(&path, "[telemetry]\nenabled = true\n").unwrap();
        let (said, wrote) = telemetry_disclosure(&path).expect("it reads");
        assert!(!wrote, "a key that is there is not rewritten");
        assert!(said.contains("reads TRUE"), "{said}");
        assert!(
            !said.contains("telemetry: off"),
            "a true key is not announced as off: {said}"
        );

        // The control: the same call on a false key says off, so the line above
        // is the value's and not a sentence this function always prints.
        std::fs::write(&path, "[telemetry]\nenabled = false\n").unwrap();
        let (said, _) = telemetry_disclosure(&path).expect("it reads");
        assert!(said.contains("telemetry: off"), "{said}");
        assert!(said.contains("reads false"), "{said}");

        // And a value that is neither says which it is rather than rounding.
        std::fs::write(&path, "[telemetry]\nenabled = \"maybe\"\n").unwrap();
        let (said, _) = telemetry_disclosure(&path).expect("it reads");
        assert!(said.contains("neither true nor false"), "{said}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// TOML basic strings are escaped: a quote or a backslash in a directory
    /// name writes a declaration that does not parse otherwise, and every later
    /// verb refuses it.
    #[test]
    fn a_name_or_a_path_carrying_a_quote_is_escaped_and_the_file_parses() {
        let text = project_text(
            "a \"quoted\" project",
            Some("a\\b"),
            Path::new("/p/a \"quoted\" project"),
            Path::new("/p/wt"),
            None,
            &written_by(&["--standalone"], false),
        );
        let table: toml::Table = text.parse().expect("the written file parses");
        assert_eq!(
            table["project"]["name"].as_str(),
            Some("a \"quoted\" project"),
            "the value reads back as it was given: {text}"
        );
        assert_eq!(table["project"]["item_prefix"].as_str(), Some("a\\b"));
        assert_eq!(
            table["project"]["primary"].as_str(),
            Some("/p/a \"quoted\" project")
        );

        // The escaper's own table, including the control characters that have
        // no short form.
        assert_eq!(basic("plain"), "plain");
        assert_eq!(basic("a\"b"), "a\\\"b");
        assert_eq!(basic("a\\b"), "a\\\\b");
        assert_eq!(basic("a\nb\tc"), "a\\nb\\tc");
        assert_eq!(basic("a\u{1}b"), "a\\u0001b");
        assert_eq!(basic("a\u{7f}b"), "a\\u007Fb");
    }

    /// The register is APPENDED, not re-serialized: a comment and a key this
    /// module does not model both survive a second registration.
    #[test]
    fn a_registration_appends_its_table_and_keeps_what_the_file_already_held() {
        let dir = std::env::temp_dir().join(format!("fleet-registry-2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert!(register(&dir, Path::new("/p/one"), "one").expect("it registers"));

        // A person's own comment and a key nothing here models, written into
        // the file between registrations.
        let path = dir.join(PROJECTS);
        let mut body = std::fs::read_to_string(&path).unwrap();
        body.push_str("# a note somebody left\nnotes = \"kept\"\n");
        std::fs::write(&path, &body).unwrap();

        assert!(register(&dir, Path::new("/p/two"), "two").expect("it registers"));
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.starts_with(&body),
            "what the file held is untouched and the new table follows it: {after}"
        );
        assert!(after.contains("# a note somebody left"), "{after}");
        assert!(after.contains("notes = \"kept\""), "{after}");
        assert_eq!(after.matches("[[project]]").count(), 2, "{after}");
        assert_eq!(
            registered(&dir).expect("it reads back").len(),
            2,
            "and both rows read back"
        );

        // A name carrying a quote is escaped here too, or the register stops
        // parsing on the row that named it.
        assert!(register(&dir, Path::new("/p/\"three\""), "a \"name\"").expect("it registers"));
        let rows = registered(&dir).expect("the register still parses");
        assert_eq!(rows[2].name, "a \"name\"");
        assert_eq!(rows[2].root, "/p/\"three\"");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A RENAME MOVES NO WORKTREE. The row takes the seat's new name, and its
    /// worktree is the one directory already standing that ends in
    /// the seat's short id — which is the part of the name a rename keeps.
    #[test]
    fn a_renamed_seat_keeps_the_worktree_that_ends_in_its_short_form() {
        let dir = std::env::temp_dir().join(format!("fleet-worktree-for-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let worktrees = dir.join("a-project-worktrees");
        std::fs::create_dir_all(worktrees.join("orla-93b9739a")).unwrap();
        // Another seat's worktree, which shares no short id with this one.
        std::fs::create_dir_all(worktrees.join("kite-e8a04b17")).unwrap();
        let fleet_toml = dir.join("fleet.toml");
        std::fs::write(
            &fleet_toml,
            "[seats.01a0d1f1-0aec-765f-9abe-d4f993b9739a]\nkind = \"agent\"\nname = \"Wren\"\n\
             \n[seats.01a0d1f1-0aec-765f-9abe-7a9e1c4f05d2]\nkind = \"human\"\nname = \"Orla\"\n",
        )
        .unwrap();
        let machine_dir = dir.join("machine");
        let policy = policy::parse("").expect("an empty policy parses");
        let service = platform::Service::resolve(
            &dir,
            &machine_dir,
            Path::new("/usr/local/bin/fleet"),
            None,
            Some("/bin/sh"),
        )
        .expect("the service is composed");
        let run = FirstRun {
            machine_dir: &machine_dir,
            fleet_toml: &fleet_toml,
            project: Some(ProjectAt {
                name: "a-project",
                worktrees_dir: &worktrees,
            }),
            policy: &policy,
            service: &service,
        };

        let (rows, humans) = rendered_seats(&run).expect("the roster renders");
        assert_eq!(humans, 1, "the human seat is counted and not rendered");
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].name.as_deref(), Some("Wren"));
        assert_eq!(rows[0].machine_name(), "wren-93b9739a");
        assert_eq!(
            rows[0].id.to_string(),
            "01a0d1f1-0aec-765f-9abe-d4f993b9739a"
        );
        assert_eq!(
            rows[0].worktrees,
            vec![(
                "a-project".to_string(),
                worktrees.join("orla-93b9739a").display().to_string()
            )],
            "the worktree the seat already had, under the name it had then"
        );

        // With no directory ending in the short id, the worktree is the one the
        // seat's machine name gives — and so it is where no directory stands
        // at all.
        std::fs::remove_dir_all(worktrees.join("orla-93b9739a")).unwrap();
        let (rows, _) = rendered_seats(&run).expect("the roster renders");
        assert_eq!(
            rows[0].worktrees[0].1,
            worktrees.join("wren-93b9739a").display().to_string()
        );
        let roster = roster_in(&std::fs::read_to_string(&fleet_toml).unwrap()).unwrap();
        let seat = &roster
            .iter()
            .find(|s| s.seat.kind == Kind::Agent)
            .expect("the agent seat is on the roster")
            .seat;
        assert_eq!(
            worktree_for(&dir.join("no-such-directory"), seat),
            dir.join("no-such-directory/wren-93b9739a")
        );

        // The control: two directories ending in the short id are no answer,
        // so neither is taken and the machine name is.
        std::fs::create_dir_all(worktrees.join("orla-93b9739a")).unwrap();
        std::fs::create_dir_all(worktrees.join("kite-93b9739a")).unwrap();
        assert_eq!(
            worktree_for(&worktrees, seat),
            worktrees.join("wren-93b9739a")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
