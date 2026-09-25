//! `fleet status` — the projection printed.
//!
//! IT READS, AND IT WRITES NOTHING. The projection the controller published,
//! the policy in force, the event stream the runs are read off and the store of
//! every project this machine registers, which the open holds are read off, are
//! the four instruments; the process table is not one of them, because a
//! running process is not what makes a seat one of this fleet's.
//!
//! THE PROJECTION IS THE ONE INSTRUMENT THIS VERB REFUSES WITHOUT (exit 5).
//! The policy, the stream and the stores print what they could not read in
//! their own sections and the page around them still prints, with the exit
//! table's could-not-tell at the end — a page that stopped at its second
//! section would hide the ones that had answers.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use fleet_controller::events;
use fleet_controller::policy::{self as controller_policy, Policy};
use fleet_controller::projection::{self, Projection, SeatRow};
use fleet_controller::routines::RoutineRow;
use fleet_controller::runs::{self, Reading, Standing};
use fleet_controller::seat::COLLECTOR_STALE_POLLS;
use fleet_controller::{clock, config, platform};
use fleet_core::item::{rules, Stop};
use fleet_core::policy as core_policy;
use fleet_core::store::Store;

use crate::exit::Exit;
use crate::item::{open_store, resolve_from};
use crate::runs::registered_roots;

/// The published document, under the machine directory.
const PROJECTION: &str = "projection.json";

/// The machine's own file, whose `[controller]` object overrides the policy
/// file's keys — read here so the threshold this page measures against is the
/// one the controller fires `suggest-rest` at.
const CONFIG: &str = "config.json";

/// The stream the runs section is read off, under the machine directory.
const STREAM: &str = "events.jsonl";

/// How far back the runs section lists a failed run, in hours.
///
/// A WINDOW AND NOT A LAST-READ MARK. "Failed since you last looked" needs a
/// mark of when somebody last looked, and that mark is a file this verb would
/// write — and it writes nothing. A day covers a night's flight and the morning
/// that reads it; a failure older than that is counted on the page and left to
/// the stream to list.
const FAILED_WINDOW_HOURS: u64 = 24;

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
        one_seat(out, &machine_dir, &body.document, &read, seat)?;
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
    runs: Result<RunsRead, String>,
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
            runs: read_runs(&machine_dir.join(STREAM), machine_dir),
        }
    }

    /// Every instrument here that would not answer. The page names each of
    /// them in its own section too; this is what stderr and the exit read.
    fn unread(&self) -> Vec<&str> {
        [
            self.policy.as_ref().err(),
            self.rules.as_ref().err(),
            self.runs.as_ref().err(),
            self.runs
                .as_ref()
                .ok()
                .and_then(|runs| runs.holds.as_ref().err()),
        ]
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
    /// The match clause, in words a person reads it by.
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
        // is nothing to print, so neither counts a rule.
        .filter_map(|entry| entry.as_table())
        .map(|rule| {
            let filled = rules::Effective {
                review: string_of(rule.get("review")),
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

/// The clause in its two halves: the item's own type and its own labels, both
/// optional, neither inherited.
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

/// Every run the stream holds, and the holds the stores still call open.
///
/// THE RUNS ARE THE STREAM'S AND NOT THE RUN'S RECORD OR ITS DIRECTORY. The
/// directory holds the pins and the logs and never how the run ended; the
/// record is open or closed, which cannot tell a failure from a close or a
/// wait from a crash. The stream carries one row of the exit table per
/// execution and the park's latch, and it is what the run pass decides every
/// re-run and every park on — so the page and the pass read one fold.
///
/// THE HOLDS ARE THE STORES' AND NOT THE STREAM'S. A hold raised or cleared on
/// another machine, or by hand with `bd`, writes no line here, and a fold of
/// the park and clearance lines counts it wrong; the store is where a hold is
/// open or is not.
struct RunsRead {
    readings: Vec<Reading>,
    holds: Result<BTreeSet<String>, String>,
}

fn read_runs(path: &Path, machine_dir: &Path) -> Result<RunsRead, String> {
    // A stream that is not there is a machine nobody has run anything on, and
    // every count is zero. One that is there and will not open is not that, and
    // the fold's reader would answer it as empty all the same.
    if path.exists() {
        std::fs::File::open(path).map_err(|e| {
            format!(
                "the stream at {} could not be read for the runs section: {e}",
                path.display()
            )
        })?;
    }
    let stream = events::read_after(path, 0);
    Ok(RunsRead {
        readings: runs::readings(&stream),
        holds: open_holds_on(machine_dir),
    })
}

/// Every hold the store of each project this machine registers still calls
/// open, as one set.
///
/// MACHINE-LEVEL, because the runs are: a machine's runs span its projects, so
/// every registered project is asked, over the roots the run pass looks a
/// run's record up across. A root that will not resolve is skipped, as the
/// pass skips it — a machine is not broken because one of its roots moved. A
/// store that will not answer is not skipped: a count without it is a count
/// of some of the holds, printed as all of them.
fn open_holds_on(machine_dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut open = BTreeSet::new();
    for root in registered_roots(machine_dir) {
        let Ok(here) = resolve_from(&root, machine_dir.to_path_buf(), None) else {
            continue;
        };
        let holds = open_store(&here.project.root).open_holds().map_err(|e| {
            format!(
                "the holds were not counted — {}'s store did not answer: {e}",
                root.display()
            )
        })?;
        open.extend(holds);
    }
    Ok(open)
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
            Some(held) => format!("{} — {}", held.seat.machine_name(), held.effect),
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
    write(runs_section(out, &read.runs))?;
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

/// A seat's line, opening on its machine name [ASSUMES D13]: the name a person
/// reads and types back, which carries the seat's own name and the tail of its
/// id. `--json` is where the id is spelled whole.
fn roster_row(row: &SeatRow) -> String {
    let mut line = row.seat.machine_name();
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
    let seat = row.seat.machine_name();
    let Some(tokens) = row.context_tokens else {
        return format!("{seat}  —");
    };
    match threshold {
        None => format!("{seat}  {tokens} tokens"),
        Some(threshold) => format!(
            "{seat}  {tokens} tokens, {}% of the threshold, {} left",
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

/// The runs a person has to know about, in the order they answer them: the
/// failures inside the window, the parks, the executions nothing could
/// classify, the waits, and the runs still executing. A closed run is not
/// listed, nor a cancelled one, which a person ended themselves; a failure
/// before the window is counted and not listed. Then the holds the stores
/// still call open, runs' and items' alike, because a park is what the
/// morning's first read is of.
fn runs_section(out: &mut dyn Write, read: &Result<RunsRead, String>) -> std::io::Result<()> {
    let read = match read {
        Ok(read) => read,
        Err(why) => {
            writeln!(out, "\nruns")?;
            writeln!(out, "  {why}")?;
            return writeln!(out, "\nholds  not counted — the stream did not read");
        }
    };
    let window = FAILED_WINDOW_HOURS * 60 * 60;
    // A stamp nobody can date is listed: a failure hidden because its line was
    // torn is the one this section exists not to hide.
    let recent = |reading: &&Reading| {
        clock::seconds_since_stamp(&reading.stamp).is_none_or(|age| age <= window)
    };
    let of = |standing: Standing| -> Vec<&Reading> {
        read.readings
            .iter()
            .filter(|reading| reading.standing == standing)
            .collect()
    };
    let failed = of(Standing::Failed);
    let listed: Vec<&Reading> = failed.iter().copied().filter(recent).collect();
    let earlier = failed.len() - listed.len();
    let held = of(Standing::Held);
    let unread = of(Standing::CouldNotTell);
    let waiting = of(Standing::Waiting);
    let open = of(Standing::Open);

    writeln!(
        out,
        "\nruns  {} failed in the last {FAILED_WINDOW_HOURS} hours, {} held, {} could not tell, \
         {} waiting, {} open",
        listed.len(),
        held.len(),
        unread.len(),
        waiting.len(),
        open.len()
    )?;
    for reading in [listed, held, unread, waiting, open].concat() {
        writeln!(out, "  {}", run_row(reading, read.holds.as_ref().ok()))?;
    }
    match earlier {
        0 => {}
        1 => writeln!(
            out,
            "  1 earlier failure is not listed — `fleet event tail --type run.failed` lists it"
        )?,
        n => writeln!(
            out,
            "  {n} earlier failures are not listed — `fleet event tail --type run.failed` lists \
             them"
        )?,
    }
    match &read.holds {
        Ok(open) => writeln!(out, "\nholds  {} open", open.len()),
        Err(why) => writeln!(out, "\nholds  not counted — {why}"),
    }
}

/// A parked run's hold reads cleared where the stores' open set does not hold
/// it. A set that was not read says nothing either way, and the holds line
/// names why.
fn run_row(reading: &Reading, open: Option<&BTreeSet<String>>) -> String {
    let said = |key: &str| said_of(reading.said.get(key));
    let what = match reading.standing {
        Standing::Failed => format!("FAILED at {} — {}", reading.stamp, said("reason")),
        Standing::Held => format!(
            "HELD at {} on hold {}{} — nothing could classify {} execution(s)",
            reading.stamp,
            reading.hold.as_deref().unwrap_or("—"),
            match (&reading.hold, open) {
                (Some(hold), Some(open)) if !open.contains(hold) => ", cleared",
                _ => "",
            },
            reading.crashes
        ),
        // The last line quoted, as it stood: it is what could not be read, and
        // bare it runs into the prose around it.
        Standing::CouldNotTell => format!(
            "could not tell at {}, {} execution(s) so far — exit {}, read {}",
            reading.stamp,
            reading.crashes,
            match reading.said.get("exit").and_then(|exit| exit.as_i64()) {
                Some(code) => code.to_string(),
                None => String::from("on a signal"),
            },
            match reading.said.get("read") {
                None | Some(serde_json::Value::Null) => String::from("nothing"),
                Some(line) => line.to_string(),
            }
        ),
        Standing::Waiting => format!("waiting since {} for {}", reading.stamp, said("wake")),
        Standing::Open => format!("open since {}", reading.stamp),
        Standing::Closed => format!("closed at {}", reading.stamp),
        Standing::Cancelled => format!("cancelled, last line at {}", reading.stamp),
    };
    format!(
        "{}  {}  {what}",
        reading.run,
        reading.workflow.as_deref().unwrap_or("—")
    )
}

/// A payload value as one line: a string as itself, an absence as `nothing`,
/// and anything else as the compact JSON the workflow wrote.
fn said_of(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::from("nothing"),
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// `--seat <name>`: the roster row and the context row, and nothing else.
///
/// The argument goes through the seat list's resolver, as every seat argument
/// does — so it matches a name without regard to case, a machine name and the
/// id — and the projection's row is the one keyed by the id it resolved to.
fn one_seat(
    out: &mut dyn Write,
    machine_dir: &Path,
    document: &Projection,
    read: &Read,
    seat: &str,
) -> Result<(), Stop> {
    let machine = config::read(&machine_dir.join(CONFIG)).map_err(|why| {
        Stop::could_not_tell(format!(
            "the seat list could not be read, so no seat can be named: {why}"
        ))
    })?;
    let named = machine.resolve(seat).map_err(Stop::from)?;
    let key = named.id.to_string();
    let Some(row) = document.seats.iter().find(|row| row.seat.id == key) else {
        return Err(Stop::refused(format!(
            "the projection carries no row for `{}` — the collector is what makes a seat one \
             of this fleet's",
            named.machine_name()
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
