//! `fleet routine list | check | run | history`.
//!
//! One family module and one arm in the dispatch, so a second family's landing
//! and this one's meet in one place. The four verbs are one clap enum here and
//! each answers in the exit table's own words; the dispatch names the family
//! and nothing else.
//!
//! Every human-facing line of `run` goes to STDERR and the event line to
//! stdout: a caller piping this verb wants the row it produced, and a note
//! about a lock it took over is not that row.

use fleet_controller::adapter::claude_code::ClaudeCode;
use fleet_controller::adapter::{dir_key, Agent};
use fleet_controller::policy::Policy;
use fleet_controller::routines::action::Machine;
use fleet_controller::routines::file::Routine;
use fleet_controller::routines::load::Registry;
use fleet_controller::routines::trigger::Due;
use fleet_controller::routines::{self, action, load, state, trigger, Outcome, SeatView};
use fleet_controller::{clock, config, events, observe, platform, policy, sessions};
use fleet_core::seat::identity::SeatId;
use std::path::{Path, PathBuf};

use crate::exit::Exit;

/// How many history rows are printed when the caller names no count.
const HISTORY_DEFAULT: usize = 20;

/// The family's four verbs. Which arguments each takes, and which of them is
/// required, is clap's to refuse; what a stamp or a count MEANS is still this
/// module's, because clap can say a value is missing and not that it is a time.
#[derive(clap::Subcommand)]
pub enum Verb {
    /// every routine loaded, with its next due and its last outcome
    List,

    /// evaluate one routine's trigger now, without firing it
    #[command(long_about = "\
evaluate one routine's trigger now and print the three-valued answer: 0 due, 1
not due, 3 could not tell. Writes nothing.")]
    Check {
        /// the routine's name
        name: String,
        /// the instant to evaluate at, as a UTC stamp
        #[arg(long, value_name = "STAMP")]
        now: Option<String>,
        /// when it last fired, over the state's own record
        #[arg(long = "last-fired", value_name = "STAMP")]
        last_fired: Option<String>,
    },

    /// fire one routine now and print the event row it produced
    #[command(long_about = "\
fire one routine now, outside its schedule, and print the event row.
--force overrides the trigger, never the lock; --dry-run writes nothing at all.")]
    Run {
        /// the routine's name
        name: String,
        /// fire it whether or not its trigger is due
        #[arg(long)]
        force: bool,
        /// say what would happen and write nothing
        #[arg(long = "dry-run")]
        dry_run: bool,
    },

    /// the routine events on this fleet's stream, one row each
    History {
        /// one routine's rows; without it, every routine's
        name: Option<String>,
        /// start at this sequence
        #[arg(long, value_name = "SEQ", default_value_t = 0)]
        since: u64,
        /// how many rows to print
        #[arg(short = 'n', value_name = "COUNT", default_value_t = HISTORY_DEFAULT)]
        count: usize,
    },
}

pub fn command(verb: &Verb) -> Exit {
    match verb {
        Verb::List => list(),
        Verb::Check {
            name,
            now,
            last_fired,
        } => check(name, now.as_deref(), last_fired.as_deref()),
        Verb::Run {
            name,
            force,
            dry_run,
        } => run(name, *force, *dry_run),
        Verb::History { name, since, count } => history(name.as_deref(), *since, *count),
    }
}

/// A value clap accepted and this module cannot read as what it must mean.
fn usage(why: &str) -> Exit {
    eprintln!("fleet routine: {why}");
    Exit::Usage
}

/// A status the trigger and the action layers state as a number, read back
/// into the table. A number outside it is those layers' own defect and reads as
/// could-not-tell rather than as a status this module invented.
fn exit_of(code: u8) -> Exit {
    Exit::from_status(code).unwrap_or(Exit::CouldNotTell)
}

/// Everything the four verbs read off the machine, resolved once.
struct Fleet {
    machine_dir: PathBuf,
    child_path: String,
    policy: Policy,
    seats: Vec<config::Seat>,
    registry: Registry,
}

/// Resolve the machine and load every routine on it. `Err` is a fleet root
/// nothing on this box names, which is could-not-tell and never an empty
/// registry.
fn resolve() -> Result<Fleet, String> {
    let machine_dir = platform::machine_dir();
    let Some(fleet_root) = load::fleet_root(None, &machine_dir) else {
        return Err(format!(
            "no `fleet.toml` above this directory and none named by {}",
            machine_dir.join("config.json").display()
        ));
    };
    let seats = config::read(&machine_dir.join("config.json"))
        .map(|machine| machine.seats)
        .unwrap_or_default();
    // A nudge names a row this machine runs; an item's assignee names any seat
    // the fleet lists besides.
    let directory = config::directory(
        &seats,
        &fleet_core::item::table_at(&fleet_root.join("fleet.toml")),
        &machine_dir,
    );
    let registry = load::load(
        &load::roots(&fleet_root, &machine_dir, &projects_of(&fleet_root)),
        &directory,
    );
    // A policy that will not read is the defaults: the four verbs below need
    // the nudge model and its bound, and refusing to LIST routines over a policy
    // file is a refusal nobody asked for.
    let policy = policy::load(&fleet_root.join("fleet.toml"))
        .or_else(|_| policy::parse(""))
        .map_err(|why| format!("the policy could not be read: {why}"))?;
    Ok(Fleet {
        child_path: platform::child_path(&platform::home_dir()),
        machine_dir,
        policy,
        seats,
        registry,
    })
}

/// The projects routines are read from: in this slice the one embedded project,
/// which is the fleet root itself.
fn projects_of(fleet_root: &std::path::Path) -> Vec<(String, PathBuf)> {
    let name = fleet_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    vec![(name, fleet_root.to_path_buf())]
}

/// The seat rows a ring reads. The roster is asked for ONLY when a routine's
/// action carries a nudge: a verb that listed sessions to run an exec routine
/// would refuse on a machine with no agent installed.
fn seat_views(fleet: &Fleet, needs_roster: bool) -> Vec<SeatView> {
    // The table read ONCE for the whole pass: every spawned seat's session is
    // held under a configuration directory of its own, which a read and a ring
    // both have to go through, and it lives on that seat's own row — as does the
    // name the ring addresses its session by.
    let recorded = sessions::read(&sessions::path_in(&fleet.machine_dir)).0;
    let config_dir_of = |seat: &SeatId| {
        recorded
            .as_ref()
            .and_then(|table| table.newest_for(&seat.to_string()))
            .and_then(|row| row.config_dir.clone())
    };
    let read = if needs_roster {
        let home = platform::home_dir();
        let mut agent = ClaudeCode::new(&home, &fleet.machine_dir);
        if let Ok(bin) = ClaudeCode::resolve_effect_bin(
            fleet_controller::adapter::claude_code::configured_bin().as_deref(),
            &agent.child_path,
        ) {
            agent = agent.with_effect_bin(bin);
        }
        // One listing per distinct directory, the same fold the loop makes: a
        // spawned seat is named by its own daemon's listing alone, so a pass that
        // read once would rank every such seat absent and ring nobody.
        let rosters =
            observe::Rosters::gather(&fleet.seats, &config_dir_of, &|dir| agent.status(dir));
        Some((rosters, agent))
    } else {
        None
    };
    fleet
        .seats
        .iter()
        .filter_map(|seat| {
            let (_, worktree) = seat.worktrees.first()?;
            let state = match &read {
                Some((rosters, agent)) => {
                    let under = config_dir_of(&seat.id);
                    let recency = observe::Recency {
                        window_ms: fleet.policy.stopped_recency_hours * 60 * 60 * 1000,
                        ended_at: &|worktree: &str, session: &str| {
                            agent.ended_at(
                                under.as_deref().map(Path::new),
                                dir_key(worktree),
                                session,
                            )
                        },
                    };
                    observe::observe_seat(
                        rosters.for_seat(&seat.id),
                        seat,
                        clock::now_ms(),
                        &recency,
                    )
                    .state
                }
                None => observe::RosterState::Absent,
            };
            let session_name = match &recorded {
                Some(table) => table.session_name(&seat.as_ref()),
                None => seat.machine_name(),
            };
            Some(SeatView {
                id: seat.id,
                session_name,
                worktree: dir_key(worktree).to_string(),
                state,
                config_dir: config_dir_of(&seat.id),
            })
        })
        .collect()
}

fn machine_of<'a>(
    fleet: &'a Fleet,
    seats: &'a [SeatView],
    agent: Option<&'a dyn Agent>,
    effects_off: Option<String>,
) -> Machine<'a> {
    Machine {
        machine_dir: &fleet.machine_dir,
        child_path: &fleet.child_path,
        policy: &fleet.policy,
        seats,
        agent,
        effects_off,
    }
}

// ---- list -------------------------------------------------------------------

fn list() -> Exit {
    let fleet = match resolve() {
        Ok(fleet) => fleet,
        Err(why) => {
            eprintln!("fleet routine list: {why}");
            return Exit::CouldNotTell;
        }
    };
    let (state, why) = state::read(&fleet.machine_dir);
    if let Some(why) = why {
        eprintln!("fleet routine list: the routines state stands empty; {why}");
    }
    let now = routines::now_secs();
    for row in routines::rows(&fleet.registry, &state, now) {
        println!(
            "{}  {}  {}  next {}  last {}  streak {}",
            row.name,
            row.source,
            row.trigger,
            row.next_due.as_deref().unwrap_or("none"),
            row.last_outcome.as_deref().unwrap_or("none"),
            row.failing_streak
        );
    }
    for defect in &fleet.registry.defects {
        println!(
            "DEFECT  {}  {}  {} — {}",
            defect.name,
            defect.source.as_string(),
            defect.path.display(),
            defect.reason()
        );
    }
    Exit::Done
}

// ---- check ------------------------------------------------------------------

fn check(name: &str, now_at: Option<&str>, last_fired: Option<&str>) -> Exit {
    let now = match stamp_or(now_at, routines::now_secs()) {
        Ok(now) => now,
        Err(why) => return usage(&format!("check: {why}")),
    };
    let fired = match last_fired {
        None => None,
        Some(text) => match clock::secs_of_stamp(text) {
            Some(secs) => Some(secs),
            None => return usage(&format!("check: `{text}` is not a UTC stamp")),
        },
    };

    let fleet = match resolve() {
        Ok(fleet) => fleet,
        Err(why) => {
            eprintln!("fleet routine check: {why}");
            return Exit::CouldNotTell;
        }
    };
    let Some(routine) = known(&fleet, name, "check") else {
        return Exit::Refused;
    };
    let fired = fired.or_else(|| {
        state::read(&fleet.machine_dir)
            .0
            .entry(name)
            .last_fired
            .as_deref()
            .and_then(clock::secs_of_stamp)
    });

    let seats = seat_views(&fleet, false);
    let machine = machine_of(&fleet, &seats, None, None);
    let answer = trigger::evaluate(routine, now, fired, &|routine| {
        action::run_check(routine, &machine)
    });
    println!("{} — {}", answer.word(), answer.reason());
    match answer {
        Due::Due(_) => Exit::Done,
        Due::NotDue(_) => Exit::Refused,
        Due::CouldNotTell(_) => Exit::CouldNotTell,
    }
}

/// The routine under this name, with a defective file and an unknown one told
/// apart on stderr. Both are exit 1: the thing named is not one this fleet can
/// act on.
fn known<'a>(fleet: &'a Fleet, name: &str, verb: &str) -> Option<&'a Routine> {
    if let Some(routine) = fleet.registry.get(name) {
        return Some(routine);
    }
    match fleet.registry.defect(name) {
        Some(defect) => eprintln!(
            "fleet routine {verb}: `{name}` is a defective routine file — {}",
            defect.reason()
        ),
        None => eprintln!("fleet routine {verb}: no routine is named `{name}`"),
    }
    None
}

fn stamp_or(text: Option<&str>, fallback: u64) -> Result<u64, String> {
    match text {
        None => Ok(fallback),
        Some(text) => {
            clock::secs_of_stamp(text).ok_or_else(|| format!("`{text}` is not a UTC stamp"))
        }
    }
}

// ---- run --------------------------------------------------------------------

fn run(name: &str, force: bool, dry_run: bool) -> Exit {
    let fleet = match resolve() {
        Ok(fleet) => fleet,
        Err(why) => {
            eprintln!("fleet routine run: {why}");
            return Exit::CouldNotTell;
        }
    };
    let Some(routine) = known(&fleet, name, "run") else {
        return Exit::Refused;
    };

    // The lock, before anything else. `--force` overrides the trigger and never
    // this: two processes running one duty at once is what it is here for.
    match state::read_lock(&fleet.machine_dir, name) {
        state::Lock::Held(pid) => {
            eprintln!("fleet routine run: `{name}` is held by a live run at pid {pid}; skipped");
            return Exit::Refused;
        }
        state::Lock::Stale(pid) => {
            eprintln!(
                "fleet routine run: taking over `{name}`'s lock from pid {pid}, which is gone"
            )
        }
        state::Lock::Free => {}
    }
    // A dry run protects nothing, so it writes nothing — the lock file
    // included. It has already refused a live holder above.
    if !dry_run {
        if let Err(e) = state::take_lock(&fleet.machine_dir, name) {
            eprintln!("fleet routine run: could not take `{name}`'s lock: {e}");
            return Exit::CouldNotTell;
        }
    }

    let needs_roster = routine.action.nudge.is_some();
    let seats = seat_views(&fleet, needs_roster);
    let home = platform::home_dir();
    let mut agent = ClaudeCode::new(&home, &fleet.machine_dir);
    let resolved = ClaudeCode::resolve_effect_bin(
        fleet_controller::adapter::claude_code::configured_bin().as_deref(),
        &agent.child_path,
    );
    let effects_off = match &resolved {
        Ok(bin) => {
            agent = agent.with_effect_bin(bin.clone());
            None
        }
        Err(why) => Some(why.clone()),
    };
    let carrier: Option<&dyn Agent> = resolved.is_ok().then_some(&agent);

    let now = routines::now_secs();
    let (mut routine_state, why) = state::read(&fleet.machine_dir);
    if let Some(why) = why {
        eprintln!("fleet routine run: the routines state stands empty; {why}");
    }
    let fired = routine_state
        .entry(name)
        .last_fired
        .as_deref()
        .and_then(clock::secs_of_stamp);

    let exit = {
        let machine = machine_of(&fleet, &seats, carrier, effects_off);
        let answer = if force {
            Due::Due("--force".to_string())
        } else {
            trigger::evaluate(routine, now, fired, &|routine| {
                action::run_check(routine, &machine)
            })
        };
        if dry_run {
            println!(
                "{} {} {}",
                routine.name,
                routine.source.as_string(),
                routine.trigger.as_str()
            );
            println!("{} — {}", answer.word(), answer.reason());
            for word in action::argv_of(routine, &machine) {
                println!("argv {word}");
            }
            Exit::Done
        } else {
            match answer {
                Due::NotDue(why) => {
                    eprintln!("fleet routine run: `{name}` is not due — {why}");
                    Exit::Refused
                }
                Due::CouldNotTell(why) => {
                    let mut events =
                        events::EventLog::open(&fleet.machine_dir.join("events.jsonl"));
                    let mut pass = routines::Pass {
                        machine: machine_of(&fleet, &seats, carrier, None),
                        events: &mut events,
                        state: &mut routine_state,
                    };
                    let outcome =
                        routines::record_could_not_tell(routine, &mut pass, now, &why, Some("run"));
                    print_last_row(&fleet.machine_dir, name);
                    exit_of(outcome.exit_code())
                }
                Due::Due(reason) => {
                    let mut events =
                        events::EventLog::open(&fleet.machine_dir.join("events.jsonl"));
                    let mut pass = routines::Pass {
                        machine,
                        events: &mut events,
                        state: &mut routine_state,
                    };
                    let outcome = routines::fire(routine, &mut pass, now, &reason, Some("run"));
                    print_last_row(&fleet.machine_dir, name);
                    // An absent ring that a fallback item answered is that
                    // item's outcome, so only a bare absent carries exit 4.
                    exit_of(outcome_exit(outcome))
                }
            }
        }
    };
    if !dry_run {
        state::release_lock(&fleet.machine_dir, name);
    }
    exit
}

fn outcome_exit(outcome: Outcome) -> u8 {
    outcome.exit_code()
}

/// The terminal row this run wrote, read back off the stream it wrote it to.
fn print_last_row(machine_dir: &std::path::Path, name: &str) {
    let rows = routine_rows(machine_dir, Some(name), 0);
    match rows.last() {
        Some(line) => println!("{line}"),
        None => eprintln!("fleet routine run: the event this run wrote is not on the stream"),
    }
}

// ---- history ----------------------------------------------------------------

fn history(name: Option<&str>, since: u64, count: usize) -> Exit {
    let machine_dir = platform::machine_dir();
    let stream = machine_dir.join("events.jsonl");
    if !stream.is_file() {
        eprintln!(
            "fleet routine history: no event stream at {}, so no routine has a history yet",
            stream.display()
        );
        return Exit::Done;
    }
    let rows = routine_rows(&machine_dir, name, since);
    let from = rows.len().saturating_sub(count);
    for line in &rows[from..] {
        println!("{line}");
    }
    Exit::Done
}

/// Every routine event on the stream, in file order, as the line a person reads.
fn routine_rows(machine_dir: &std::path::Path, name: Option<&str>, since: u64) -> Vec<String> {
    events::read_after(&machine_dir.join("events.jsonl"), since)
        .into_iter()
        .filter(|record| events::ROUTINE_TYPES.contains(&record.kind.as_str()))
        .filter(|record| {
            name.is_none_or(|wanted| {
                record.payload.get("order").and_then(|o| o.as_str()) == Some(wanted)
            })
        })
        .map(|record| {
            let routine = record
                .payload
                .get("order")
                .and_then(|o| o.as_str())
                .unwrap_or(&record.actor.id)
                .to_string();
            format!(
                "{}  {}  {}  {}  {}",
                record.seq,
                record.ts,
                routine,
                record.kind,
                tail_of(&record)
            )
        })
        .collect()
}

/// The last cell: the outcome for a firing that ended, and the reason for one
/// that opened or could not be told.
fn tail_of(record: &events::Record) -> String {
    let field = |key: &str| {
        record
            .payload
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    match field("outcome") {
        Some(outcome) => match field("detail") {
            Some(detail) => format!("{outcome} — {detail}"),
            None => outcome,
        },
        None => field("reason").unwrap_or_default(),
    }
}

// ---- the retired spelling --------------------------------------------------

/// `fleet order …`: the family's name for one release before this one. Usage,
/// with the one rewrite that helps, and nothing read off the words after it.
pub fn old_name() -> Exit {
    eprintln!("fleet order: the family is `fleet routine` now — use `fleet routine list | check | run | history`");
    Exit::Usage
}
