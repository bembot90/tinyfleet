//! `fleet status` — the projection printed (cli PRD § `fleet status`).
//!
//! IT READS FILES AND WRITES NOTHING. The projection the controller published
//! and the policy in force are the two instruments; the process table is not
//! one of them, because a running process is not what makes a seat one of this
//! fleet's.
//!
//! THE PROJECTION IS THE ONE INSTRUMENT THIS VERB REFUSES WITHOUT (exit 5).
//! The policy prints what it could not read in its own sections and the page
//! around them still prints, with the exit table's could-not-tell at the end
//! — a page that stopped at its second section would hide the ones that had
//! answers.

use std::io::Write;
use std::path::{Path, PathBuf};

use fleet_controller::policy::{self as controller_policy, Policy};
use fleet_controller::projection::{self, Projection, SeatRow};
use fleet_controller::routines::RoutineRow;
use fleet_controller::seat::COLLECTOR_STALE_POLLS;
use fleet_controller::{clock, config, platform};
use fleet_core::item::{rules, Stop};
use fleet_core::policy as core_policy;

use crate::exit::Exit;

/// The published document, under the machine directory.
const PROJECTION: &str = "projection.json";

/// The machine's own file, whose `[controller]` object overrides the policy
/// file's keys — read here so the threshold this page measures against is the
/// one the controller fires `suggest-rest` at.
const CONFIG: &str = "config.json";

/// What `status` takes. The two flags are mutually exclusive at the parser, so
/// the pair refuses with clap's own usage line and the exit table's row 2.
#[derive(clap::Args)]
pub struct StatusArgs {
    /// print the projection document verbatim
    #[arg(long)]
    pub json: bool,

    /// print one seat's roster row and context row
    #[arg(long, value_name = "NAME", conflicts_with = "json")]
    pub seat: Option<String>,
}

pub fn status_command(args: &StatusArgs) -> Exit {
    let mut out = std::io::stdout();
    match run(args, &mut out) {
        Ok(exit) => exit,
        Err(stop) => {
            eprintln!("fleet status: {}", stop.message);
            Exit::from_status(stop.code).unwrap_or(Exit::CouldNotTell)
        }
    }
}

fn run(args: &StatusArgs, out: &mut dyn Write) -> Result<Exit, Stop> {
    let machine_dir = platform::machine_dir();
    let path = machine_dir.join(PROJECTION);
    let body = read_projection(&path)?;

    // --json is the document and not a rendering of it, so the bytes go out
    // whole. The parse above still ran: an unparsable document is the same
    // could-not-tell whichever flag asked for it.
    if args.json {
        out.write_all(body.text.as_bytes())
            .map_err(|e| wrote_nothing(&e))?;
        return Ok(Exit::Done);
    }

    let read = Read::taken(&machine_dir, &body.document);
    if let Some(seat) = args.seat.as_deref() {
        one_seat(out, &body.document, &read, seat)?;
        return Ok(said(&read.unread()));
    }

    whole_page(out, &body.document, &read)?;
    Ok(said(&read.unread()))
}

/// Every instrument that would not answer, named once on stderr, and the exit
/// the table gives that: a page carrying a section it could not fill is
/// could-not-tell and never a clean 0.
fn said(unread: &[&str]) -> Exit {
    for why in unread {
        eprintln!("fleet status: {why}");
    }
    if unread.is_empty() {
        Exit::Done
    } else {
        Exit::CouldNotTell
    }
}

// ---- the reads ---------------------------------------------------------------

/// The projection's bytes and the document they parsed to.
struct Body {
    text: String,
    document: Projection,
}

fn read_projection(path: &Path) -> Result<Body, Stop> {
    let text = std::fs::read_to_string(path).map_err(|e| Stop {
        code: Exit::NoCollector.code(),
        message: format!(
            "there is no projection at {} to print ({e}) — run `fleet start`",
            path.display()
        ),
    })?;
    let document: Projection = serde_json::from_str(&text).map_err(|e| {
        Stop::could_not_tell(format!(
            "the projection at {} does not parse: {e}",
            path.display()
        ))
    })?;
    if document.version != projection::VERSION {
        return Err(Stop::could_not_tell(format!(
            "the projection at {} is version {}, and this binary reads version {} — a document \
             half-read is worse than one refused",
            path.display(),
            document.version,
            projection::VERSION
        )));
    }
    Ok(Body { text, document })
}

/// Everything the page needs beside the projection, each instrument's failure
/// kept as its own answer rather than folded into an absence.
struct Read {
    age: Option<u64>,
    stale: bool,
    policy: Result<Policy, String>,
    rules: Result<Vec<RuleRow>, String>,
}

impl Read {
    fn taken(machine_dir: &Path, document: &Projection) -> Read {
        let age = clock::seconds_since_stamp(&document.generated_at);
        let window = document.fleet.poll_seconds * COLLECTOR_STALE_POLLS;
        // A stamp nobody can date is NOT fresh: the collector's own rule, so a
        // page and a refused `rest` cannot disagree about the same document.
        let stale = age.map(|age| age > window).unwrap_or(true);
        let policy_file = PathBuf::from(&document.fleet.path);
        Read {
            age,
            stale,
            policy: effective_policy(&policy_file, machine_dir),
            rules: read_rules(&policy_file),
        }
    }

    /// Every instrument here that would not answer. The page names each of
    /// them in its own section too; this is what stderr and the exit read.
    fn unread(&self) -> Vec<&str> {
        [self.policy.as_ref().err(), self.rules.as_ref().err()]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect()
    }
}

/// The policy in force, as the controller computes it: the file the projection
/// names, with the machine's own `[controller]` object over it. A machine file
/// that will not read leaves the policy file's own values standing, which is
/// what the loop does with one.
fn effective_policy(policy_file: &Path, machine_dir: &Path) -> Result<Policy, String> {
    let file = controller_policy::load(policy_file).map_err(|e| {
        format!(
            "{} could not be read for the rest threshold: {e}",
            policy_file.display()
        )
    })?;
    let Ok(machine) = config::read(&machine_dir.join(CONFIG)) else {
        return Ok(file);
    };
    let over = controller_policy::overrides_in(machine.controller.as_ref());
    Ok(file.overlaid(&over))
}

/// One `[[core.flight.rules]]` entry as the page prints it.
struct RuleRow {
    /// The match clause, in the words the matcher reads it by.
    clause: String,
    /// The keys this rule sets, in [`rules::KEYS`] order.
    sets: Vec<String>,
}

fn read_rules(policy_file: &Path) -> Result<Vec<RuleRow>, String> {
    let table = fleet_core::item::read_table(policy_file)
        .map_err(|e| format!("the rules table could not be read: {e}"))?;
    let value = core_policy::read("core.flight", "rules", &table)
        .map_err(|unlisted| unlisted.to_string())?;
    Ok(rule_rows(value))
}

fn rule_rows(value: Option<&core_policy::Value>) -> Vec<RuleRow> {
    let Some(entries) = value.and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    entries
        .iter()
        // An entry that is not a table declares no clause and no value, which
        // is the matcher's own reading of one, so neither counts a rule.
        .filter_map(|entry| entry.as_table())
        .map(|rule| {
            let filled = rules::Effective {
                review: string_of(rule.get("review")),
                gate: string_of(rule.get("gate")),
            };
            RuleRow {
                clause: clause_of(rule.get("match")),
                sets: filled
                    .rows()
                    .into_iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect(),
            }
        })
        .collect()
}

/// The clause in the same two halves the matcher tests: the item's own type and
/// its own labels, both optional, neither inherited.
fn clause_of(value: Option<&core_policy::Value>) -> String {
    let Some(clause) = value else {
        return String::from("every item");
    };
    let Some(clause) = clause.as_table() else {
        return String::from("a match clause that is not a table, which matches no item");
    };
    let mut halves: Vec<String> = Vec::new();
    if let Some(wanted) = clause.get("type").and_then(|value| value.as_str()) {
        halves.push(format!("type {wanted}"));
    }
    if let Some(wanted) = clause.get("labels").and_then(|value| value.as_array()) {
        let labels: Vec<&str> = wanted.iter().filter_map(|label| label.as_str()).collect();
        halves.push(format!("labels [{}]", labels.join(", ")));
    }
    if halves.is_empty() {
        return String::from("every item");
    }
    halves.join(" and ")
}

fn string_of(value: Option<&core_policy::Value>) -> Option<String> {
    value?.as_str().map(str::to_string)
}

// ---- the page ----------------------------------------------------------------

fn whole_page(out: &mut dyn Write, document: &Projection, read: &Read) -> Result<(), Stop> {
    let write = |result: std::io::Result<()>| result.map_err(|e| wrote_nothing(&e));

    write(first_line(out, document, read))?;
    if document.grant != platform::GRANT_OK {
        write(writeln!(
            out,
            "\nGRANT PENDING — {}",
            document
                .grant_detail
                .as_deref()
                .unwrap_or("no detail published")
        ))?;
    }

    write(writeln!(out, "\nroster"))?;
    if document.seats.is_empty() {
        write(writeln!(out, "  no seat is configured"))?;
    }
    for row in &document.seats {
        write(writeln!(out, "  {}", roster_row(row)))?;
    }

    write(writeln!(
        out,
        "\nin flight  {}",
        match &document.in_flight {
            Some(held) => format!("{} — {}", held.seat, held.effect),
            None => String::from("nothing"),
        }
    ))?;
    write(writeln!(
        out,
        "effects  {}{}",
        document.effects.state,
        match &document.effects.cause {
            Some(cause) => format!(" — {cause}"),
            None => String::new(),
        }
    ))?;

    write(context_section(out, &document.seats, read))?;
    write(rules_section(out, read))?;
    write(routines_section(out, &document.orders))?;
    Ok(())
}

/// The one line a reader who reads nothing else gets: how old the reading is,
/// and what published it.
fn first_line(out: &mut dyn Write, document: &Projection, read: &Read) -> std::io::Result<()> {
    let age = match (read.age, read.stale) {
        (Some(age), true) => format!("STALE, {age}s old"),
        (Some(age), false) => format!("{age}s old"),
        (None, _) => String::from("STALE, an age nothing can read it at"),
    };
    let agent = match (&document.agent_version, &document.agent_version_expected) {
        (Some(read), Some(expected)) if read != expected => {
            format!("agent {read} (expected {expected})")
        }
        (Some(read), _) => format!("agent {read}"),
        (None, Some(expected)) => format!("agent not read (expected {expected})"),
        (None, None) => String::from("agent not read"),
    };
    writeln!(
        out,
        "projection {} — {age} — controller {}, {agent}",
        document.generated_at, document.controller_version
    )
}

fn roster_row(row: &SeatRow) -> String {
    let mut line = row.seat_dir.clone();
    if let Some(name) = &row.chosen_name {
        line.push_str(&format!(" ({name})"));
    }
    line.push_str(&format!("  {}", row.roster_state));
    if let Some(waiting) = &row.waiting_for {
        line.push_str(&format!(", waiting for {waiting}"));
    }
    line.push_str(&format!(
        "  decision {}, outcome {}",
        row.decision, row.outcome
    ));
    line.push_str(&format!(
        "  project {}, worktree {}",
        row.project.as_deref().unwrap_or("—"),
        row.worktree.as_deref().unwrap_or("—")
    ));
    if row.halted {
        line.push_str(&format!("  HALTED, {} blind dispatch(es)", row.blind));
    }
    line
}

/// Each seat's reading against the threshold `suggest-rest` fires at, and the
/// window left under it — the reading the reference called runway.
fn context_section(out: &mut dyn Write, seats: &[SeatRow], read: &Read) -> std::io::Result<()> {
    match &read.policy {
        Err(why) => {
            writeln!(out, "\ncontext")?;
            writeln!(out, "  {why}")?;
            for row in seats {
                writeln!(out, "  {}", context_row(row, None))?;
            }
            Ok(())
        }
        Ok(policy) => {
            let threshold = policy.rest_threshold_tokens;
            writeln!(out, "\ncontext  (rest threshold {threshold} tokens)")?;
            for row in seats {
                writeln!(out, "  {}", context_row(row, Some(threshold)))?;
            }
            Ok(())
        }
    }
}

/// A seat with no reading prints a dash: null in the document is a measured
/// absence and never a zero, and a zero here would read as a fresh session.
fn context_row(row: &SeatRow, threshold: Option<u64>) -> String {
    let Some(tokens) = row.context_tokens else {
        return format!("{}  —", row.seat_dir);
    };
    match threshold {
        None => format!("{}  {tokens} tokens", row.seat_dir),
        Some(threshold) => format!(
            "{}  {tokens} tokens, {}% of the threshold, {} left",
            row.seat_dir,
            percent_of(tokens, threshold),
            threshold.saturating_sub(tokens)
        ),
    }
}

/// Integer percent, and a threshold of zero is no percentage rather than a
/// division this binary would not survive.
fn percent_of(tokens: u64, threshold: u64) -> String {
    match threshold {
        0 => String::from("no"),
        threshold => format!("{}", tokens.saturating_mul(100) / threshold),
    }
}

fn rules_section(out: &mut dyn Write, read: &Read) -> std::io::Result<()> {
    writeln!(out, "\n[[core.flight.rules]]")?;
    match &read.rules {
        Err(why) => writeln!(out, "  {why}"),
        Ok(rows) if rows.is_empty() => writeln!(out, "  no rules are set"),
        Ok(rows) => {
            for row in rows {
                let sets = if row.sets.is_empty() {
                    String::from("sets nothing")
                } else {
                    row.sets.join(", ")
                };
                writeln!(out, "  {} → {sets}", row.clause)?;
            }
            Ok(())
        }
    }
}

fn routines_section(out: &mut dyn Write, routines: &[RoutineRow]) -> std::io::Result<()> {
    writeln!(out, "\nroutines")?;
    if routines.is_empty() {
        return writeln!(out, "  no routine is loaded");
    }
    for routine in routines {
        writeln!(
            out,
            "  {}  {}  next due {}, last {} at {}, failing streak {}",
            routine.name,
            routine.trigger,
            routine.next_due.as_deref().unwrap_or("—"),
            routine.last_outcome.as_deref().unwrap_or("—"),
            routine.last_fired.as_deref().unwrap_or("—"),
            routine.failing_streak
        )?;
    }
    Ok(())
}

/// `--seat <name>`: the roster row and the context row, and nothing else.
fn one_seat(
    out: &mut dyn Write,
    document: &Projection,
    read: &Read,
    seat: &str,
) -> Result<(), Stop> {
    let Some(row) = document
        .seats
        .iter()
        .find(|row| row.seat_dir == seat || row.chosen_name.as_deref() == Some(seat))
    else {
        return Err(Stop::refused(format!(
            "the projection carries no row for `{seat}` — the collector is what makes a seat one \
             of this fleet's"
        )));
    };
    let threshold = read
        .policy
        .as_ref()
        .ok()
        .map(|policy| policy.rest_threshold_tokens);
    writeln!(out, "{}", roster_row(row)).map_err(|e| wrote_nothing(&e))?;
    writeln!(out, "{}", context_row(row, threshold)).map_err(|e| wrote_nothing(&e))?;
    Ok(())
}

fn wrote_nothing(e: &std::io::Error) -> Stop {
    Stop::could_not_tell(format!("the page could not be written: {e}"))
}
