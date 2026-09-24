//! `fleet seat spawn`, `fleet seat feed`, `fleet seat retire` — the cli half of
//! the controller's transient-seat primitives.
//!
//! Everything here is what only a process knows: the project the cwd resolves
//! to, its two directories, the clock, the agent binary and the file a turn is
//! read from. The refusals, the exits and the writes are
//! `fleet_controller::transient`'s, so the belt that would refuse a spawn lives
//! beside the loop that starts the session.

use std::path::{Path, PathBuf};

use fleet_controller::adapter::claude_code::ClaudeCode;
use fleet_controller::transient::{self, Machine, Refusal};
use fleet_controller::{clock, config, platform, policy as controller, sessions};
use fleet_core::item::brief::Packs;
use fleet_core::item::land::{self, Release};
use fleet_core::item::{render, Spawn, SpawnOutcome, Spawner, Stop, COULD_NOT_TELL};
use fleet_core::seat;
use fleet_core::seat::identity::{Kind, SeatId, SeatRef};
use fleet_core::store::Store;

use crate::envelope;
use crate::exit::Exit;
use crate::item::{acting, open_store, resolve_at, Here};

/// The three verb names the envelope's documents carry, which are also the
/// words each verb's own stderr line names itself by.
const SPAWN: &str = "seat spawn";
const FEED: &str = "seat feed";
const RETIRE: &str = "seat retire";

/// What `seat spawn` takes.
#[derive(clap::Args)]
pub struct SpawnArgs {
    /// the file whose text is the session's first turn
    #[arg(long = "first-turn", value_name = "FILE")]
    pub first_turn: PathBuf,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// the model, over the policy's default
    #[arg(long, value_name = "ID")]
    pub model: Option<String>,
    /// the commit to cut the worktree at; else the trunk
    #[arg(long, value_name = "COMMIT")]
    pub base: Option<String>,
    /// the builder's checks the seat's rules let it run
    #[arg(long, value_name = "COMMAND")]
    pub touched: Option<String>,
    /// print the envelope document instead of the name
    #[arg(long)]
    pub json: bool,
}

/// What `seat feed` takes.
#[derive(clap::Args)]
pub struct FeedArgs {
    /// the transient seat to feed
    pub seat: String,
    /// the file whose text is the seat's next first turn
    #[arg(long = "first-turn", value_name = "FILE")]
    pub first_turn: PathBuf,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// print the envelope document instead of the line
    #[arg(long)]
    pub json: bool,
}

/// What `seat retire` takes.
#[derive(clap::Args)]
pub struct RetireArgs {
    /// the transient seat to retire
    pub seat: String,
    /// license a seat whose session is already gone
    #[arg(long)]
    pub dead: bool,
    /// who is retiring it; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// print the envelope document instead of the reclaim
    #[arg(long)]
    pub json: bool,
}

pub fn spawn_command(args: &SpawnArgs) -> Exit {
    let here = match resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let first_turn = match turn_text(&args.first_turn) {
        Ok(text) => text,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let home = platform::home_dir();
    let agent = match effect_agent(&here, &home) {
        Ok(agent) => agent,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let policy = match policy_of(&here) {
        Ok(policy) => policy,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let at = match Where::of(&here) {
        Ok(at) => at,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let settings = match settings_of(&here, args.touched.as_deref()) {
        Ok(settings) => settings,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let config_files = match config_files_of(&here) {
        Ok(files) => files,
        Err(stop) => return stopped(SPAWN, &stop, args.json),
    };
    let machine = machine_of(&here, &at, &agent, &policy);

    match transient::spawn(
        &machine,
        &transient::Spawn {
            first_turn: &first_turn,
            model: args.model.as_deref(),
            settings: Some(&settings),
            item: None,
            base: args.base.as_deref(),
            config_files: &config_files,
        },
        clock::now_ms(),
    ) {
        Ok(spawned) => {
            // Both belt readings on the way out too, because the number a
            // person wants when the next spawn refuses is the one this one saw.
            eprintln!("{}", spawned.belt.lines());
            eprintln!("worktree: {}", spawned.worktree.display());
            if args.json {
                // THE DOCUMENT REPLACES THE NAME on stdout and never joins it:
                // under the flag stdout is one document and nothing else
                // (`envelope.rs`). The belt is its payload rather than its two
                // sentences, for the same reason.
                println!(
                    "{}",
                    envelope::ok(
                        SPAWN,
                        &serde_json::json!({
                            // The seat as its object: the id the spawn
                            // minted, an agent's, and nameless.
                            "seat": SeatRef {
                                id: spawned.id,
                                name: None,
                                kind: Kind::Agent,
                            },
                            "worktree": spawned.worktree.display().to_string(),
                            "belt": spawned.belt.payload(),
                            "base": spawned.base,
                        })
                    )
                );
            } else {
                // The seat's machine name, `agent-<short>`, alone on stdout:
                // what a person reads and types back. `dispatch` assigns the
                // seat's id, which the spawner seam hands it directly.
                println!("{}", spawned.seat);
            }
            Exit::Done
        }
        Err(refusal) => refused(SPAWN, &refusal, args.json),
    }
}

pub fn feed_command(args: &FeedArgs) -> Exit {
    let here = match resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return stopped(FEED, &stop, args.json),
    };
    let row = match seat_named(&here.machine_dir, &args.seat) {
        Ok(row) => row,
        Err(stop) => return stopped(FEED, &stop, args.json),
    };
    let seat = row.machine_name();
    let first_turn = match turn_text(&args.first_turn) {
        Ok(text) => text,
        Err(stop) => return stopped(FEED, &stop, args.json),
    };
    let home = platform::home_dir();
    let agent = match effect_agent(&here, &home) {
        Ok(agent) => agent,
        Err(stop) => return stopped(FEED, &stop, args.json),
    };
    let policy = match policy_of(&here) {
        Ok(policy) => policy,
        Err(stop) => return stopped(FEED, &stop, args.json),
    };
    let at = match Where::of(&here) {
        Ok(at) => at,
        Err(stop) => return stopped(FEED, &stop, args.json),
    };
    let machine = machine_of(&here, &at, &agent, &policy);

    match transient::feed(&machine, &seat, &first_turn) {
        Ok(fed) => {
            if args.json {
                // THE TURNS ARE THEIR FIRST LINES, which is the shape the
                // controller's own journal carries them in: the whole text is
                // the session table's, and a second copy of it could disagree
                // with the first.
                println!(
                    "{}",
                    envelope::ok(
                        FEED,
                        &serde_json::json!({
                            "seat": row.as_ref(),
                            "prior_first_turn": first_line(&fed.prior),
                            "first_turn": first_line(&fed.next),
                        })
                    )
                );
            } else {
                println!("fed {} — the turn in the seat is now the new one", fed.seat);
            }
            Exit::Done
        }
        Err(refusal) => refused(FEED, &refusal, args.json),
    }
}

pub fn retire_command(args: &RetireArgs) -> Exit {
    let here = match resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return stopped(RETIRE, &stop, args.json),
    };
    // Who the withdrawal is written by, resolved before anything moves: the
    // retire is somebody's act, and a verb always has an actor.
    let by = match acting(RETIRE, args.by.as_deref(), &here) {
        Ok(by) => by.to_string(),
        Err(stop) => return stopped(RETIRE, &stop, args.json),
    };
    let row = match seat_named(&here.machine_dir, &args.seat) {
        Ok(row) => row,
        Err(stop) => return stopped(RETIRE, &stop, args.json),
    };
    let seat = row.machine_name();
    let home = platform::home_dir();
    let agent = match effect_agent(&here, &home) {
        Ok(agent) => agent,
        Err(stop) => return stopped(RETIRE, &stop, args.json),
    };
    let policy = match policy_of(&here) {
        Ok(policy) => policy,
        Err(stop) => return stopped(RETIRE, &stop, args.json),
    };
    let at = match Where::of(&here) {
        Ok(at) => at,
        Err(stop) => return stopped(RETIRE, &stop, args.json),
    };
    let machine = machine_of(&here, &at, &agent, &policy);

    // THE ITEM THIS SEAT WAS DISPATCHED, and the notes that answer for it, read
    // BEFORE the retire drops the row that names it. Neither is a reading this
    // verb refuses over: a seat nobody can name an item for retires exactly as
    // it always did and keeps its branch.
    let dispatched = held_item(&here, &row.id);
    let notes = dispatched
        .as_deref()
        .and_then(|item| notes_of(&here, item))
        .unwrap_or_default();

    // THE RECORD'S HALF OF THE RETIRE, which the controller reaches no work
    // graph to do for itself. It runs inside the retire, at the last moment the
    // verb can still stop: the name this frees is the one the next spawn takes,
    // and an order left standing against it is one that seat would inherit.
    let store = open_store(&here.project.root);
    // THE ROW'S ID, which is what the order was assigned to; the note and the
    // sentences name the seat by its machine name. The retire hands its
    // withdrawal the name it resolved, which is this same row.
    let id = row.id.to_string();
    let withdrawal = |seat: &str| -> Result<Vec<String>, Refusal> {
        let held = match seat::retire::held(&store, &id) {
            Ok(held) => held,
            // A BOARD THAT WILL NOT ANSWER IS A QUESTION wherever this fleet
            // gave this seat something, and the retire stops on it rather than
            // freeing the name over silence. A seat whose own session row names
            // no item was dispatched nothing HERE: a project with no work graph
            // at all spawns, feeds and retires its seats exactly as it did
            // before this, and the line says the board went unread.
            Err(stop) if dispatched.is_none() => {
                eprintln!(
                    "the board was not read, so no order was withdrawn from {seat}: {}",
                    stop.message
                );
                Vec::new()
            }
            Err(stop) => return Err(as_refusal(stop)),
        };
        if held.is_empty() {
            return Ok(Vec::new());
        }
        seat::retire::withdraw(&store, &held, &id, &here.seats.label(&row.id), &by)
            .map_err(as_refusal)?;
        Ok(held.into_iter().map(|row| row.id).collect())
    };

    match transient::retire_with(&machine, &seat, args.dead, &withdrawal) {
        Ok(reclaimed) => {
            if !args.json {
                println!(
                    "retired {} — reclaimed {} from {}{}{}",
                    reclaimed.seat,
                    match reclaimed.bytes {
                        Some(bytes) => format!("{bytes} bytes"),
                        None => "an unmeasured number of bytes".to_string(),
                    },
                    reclaimed.worktree,
                    match reclaimed.pid {
                        Some(pid) => format!(", pid {pid} confirmed gone"),
                        None => ", no live session to stop".to_string(),
                    },
                    if reclaimed.dead {
                        " (--dead: the roster named no live session)"
                    } else {
                        ""
                    }
                );
            }
            if let Some(removal) = &reclaimed.removal {
                eprintln!("session: {removal}");
            }
            for item in &reclaimed.withdrawn {
                eprintln!(
                    "{}: {item} — it is open and unassigned",
                    seat::retire::WITHDRAWN
                );
            }
            let branch =
                release_the_branch(&machine, &notes, reclaimed.branch.as_deref(), args.json);
            if args.json {
                println!(
                    "{}",
                    envelope::ok(
                        RETIRE,
                        &serde_json::json!({
                            "seat": row.as_ref(),
                            "worktree": reclaimed.worktree,
                            "bytes": reclaimed.bytes,
                            "pid": reclaimed.pid,
                            "branch": branch,
                        })
                    )
                );
            }
            Exit::Done
        }
        Err(refusal) => refused(RETIRE, &refusal, args.json),
    }
}

/// The work branch the landing already classified, deleted now that the
/// worktree holding it is gone.
///
/// IT NEVER TAKES THE EXIT WITH IT, either way. The seat is reclaimed by the
/// time this runs, and a retire reported as failed over a ref would be one a
/// person runs again against a seat that is not there.
fn release_the_branch(
    machine: &Machine,
    notes: &str,
    held: Option<&str>,
    json: bool,
) -> serde_json::Value {
    match land::release(notes, held) {
        Release::Delete(branch) => match transient::delete_branch(machine, &branch) {
            Ok(()) => {
                if !json {
                    println!(
                        "work branch {branch}: {} on the landing — deleted",
                        land::SAFE
                    );
                }
                disposition(
                    Some(&branch),
                    "deleted",
                    &format!("{} on the landing", land::SAFE),
                )
            }
            Err(cause) => {
                eprintln!(
                    "work branch {branch}: {} on the landing — kept: {cause}",
                    land::SAFE
                );
                disposition(Some(&branch), "kept", &cause.to_string())
            }
        },
        // A seat that was on no branch at all has nothing to say here; every
        // other keep names the branch and the reading that spared it.
        Release::Keep(why) => {
            if held.is_some() && !json {
                println!("work branch kept — {why}");
            }
            disposition(held, "kept", &why)
        }
    }
}

/// What became of the work branch, as the envelope's reader branches on it: the
/// name, the verdict in one word, and the reading behind it. A seat that stood
/// on no branch has `null` for the name and the same two words for the rest.
fn disposition(branch: Option<&str>, what: &str, why: &str) -> serde_json::Value {
    serde_json::json!({ "name": branch, "disposition": what, "why": why })
}

/// A core stop as the controller's refusal, which is what the retire's seam
/// answers in. The exit is carried across unchanged: a store that would not
/// answer stays a question and is never rounded into a verdict.
pub(crate) fn as_refusal(stop: Stop) -> Refusal {
    Refusal {
        code: stop.code,
        message: stop.message,
    }
}

/// The item this seat's newest session row was dispatched, where one names it.
fn held_item(here: &Here, seat: &SeatId) -> Option<String> {
    sessions::read(&sessions::path_in(&here.machine_dir))
        .0?
        .newest_for(&seat.to_string())?
        .item
        .clone()
}

/// That item's notes. A store that will not answer reads as no notes, which is
/// the answer that keeps the branch.
fn notes_of(here: &Here, item: &str) -> Option<String> {
    open_store(&here.project.root).show(item).ok()?.notes
}

// ---- the spawner seam -------------------------------------------------------

/// The spawner `dispatch` reaches when it is given no `--to`, which is the one
/// the controller's primitives implement. The row, the worktree, the first
/// turn, the belt and the retire are the controller's; the order note, the
/// brief's content and the assignment stay `dispatch`'s.
///
/// It answers `Refusal` on every path this verb has, including the belt's: a
/// dispatch whose spawn refuses withdraws the order in the same act, so the
/// cause a person reads here is the cause of the withdrawal.
///
/// EVERY STOP ON THESE PATHS ALREADY CARRIES ITS EXIT, and [`outcome_of`] is
/// the one place that reads it, so a leg that learns to tell a question from a
/// verdict is told apart here without a second table.
pub struct TransientSpawner<'a> {
    pub here: &'a Here,
    pub home: PathBuf,
}

impl Spawner for TransientSpawner<'_> {
    fn spawn(&self, ask: &Spawn) -> SpawnOutcome {
        let text = match turn_text(ask.first_turn) {
            Ok(text) => text,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let agent = match effect_agent(self.here, &self.home) {
            Ok(agent) => agent,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let policy = match policy_of(self.here) {
            Ok(policy) => policy,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let at = match Where::of(self.here) {
            Ok(at) => at,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let settings = match settings_of(self.here, ask.touched) {
            Ok(settings) => settings,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let config_files = match config_files_of(self.here) {
            Ok(files) => files,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let machine = machine_of(self.here, &at, &agent, &policy);
        match transient::spawn(
            &machine,
            &transient::Spawn {
                first_turn: &text,
                model: ask.model,
                settings: Some(&settings),
                item: Some(ask.item),
                base: ask.base,
                config_files: &config_files,
            },
            clock::now_ms(),
        ) {
            // The seat's FULL ID, which is what dispatch assigns the item to
            // and writes into the index: the record is keyed by the seat.
            Ok(spawned) => SpawnOutcome::Spawned {
                seat: spawned.id.to_string(),
                base: spawned.base,
                // The same two lines `seat spawn` prints, from the same render:
                // the two verbs run one belt and a person reading either reads
                // the same words about it.
                belt: Some(spawned.belt.lines()),
            },
            Err(refusal) => outcome_of(refusal.code, refusal.message),
        }
    }
}

/// One leg's stop, as the outcome dispatch acts on.
///
/// ONLY THE COULD-NOT-TELL EXIT LEAVES THE ORDER STANDING. A usage error is
/// dispatch's own defect — it built a call this process cannot read — and is
/// deterministic, so an order left standing for it would wait on a retry that
/// cannot succeed; it reads as the refusal it is (decision A of that spec).
fn outcome_of(code: u8, message: String) -> SpawnOutcome {
    match code {
        COULD_NOT_TELL => SpawnOutcome::CouldNotTell(message),
        _ => SpawnOutcome::Refused(message),
    }
}

// ---- what only a process knows ----------------------------------------------

/// The two directories a verb acts in, resolved once so the borrow they are
/// handed down as has an owner in the caller's frame.
pub(crate) struct Where {
    primary: PathBuf,
    worktrees: PathBuf,
}

impl Where {
    pub(crate) fn of(here: &Here) -> Result<Where, Stop> {
        Ok(Where {
            primary: here.primary()?,
            worktrees: here.worktrees_dir()?,
        })
    }
}

pub(crate) fn machine_of<'a>(
    here: &'a Here,
    at: &'a Where,
    agent: &'a ClaudeCode,
    policy: &'a controller::Policy,
) -> Machine<'a> {
    Machine {
        machine_dir: &here.machine_dir,
        agent,
        policy,
        project: &here.project.name,
        primary: &at.primary,
        worktrees_dir: &at.worktrees,
        // The binary is where the two overrides and the platform are read,
        // which is what keeps a variable out of a process that forks children
        // while its own threads are running.
        readings: transient::Readings::taken(),
        // The binary's waits are the box's: `SystemClock::sleep` is the bare
        // standard-library call, so a seamed site under it waits exactly as it
        // did before the seam.
        clock: &clock::SystemClock,
    }
}

/// The project this directory resolves to, with `--project` checked against it.
///
/// `--project` SELECTS NOTHING here: choosing among registered projects is the
/// standalone registry's, which `fleet create` brings. What it does is catch a
/// caller who thinks they are somewhere else, which is exactly the mistake that
/// spawns a seat against the wrong checkout.
pub(crate) fn resolved(project: Option<&str>) -> Result<Here, Stop> {
    let here = resolve_at(None)?;
    if let Some(named) = project {
        if named != here.project.name {
            return Err(Stop::usage(format!(
                "--project {named} names a project this directory does not resolve to — here is \
                 `{}`, at {}",
                here.project.name,
                here.project.root.display()
            )));
        }
    }
    Ok(here)
}

/// The one row a seat argument names — its full id, eight or more of its hex
/// digits, its name or its machine name — resolved through the seat list
/// before anything is asked of the controller.
///
/// THE ROW IS WHAT IS HANDED ON: its id is what the session table, the stream
/// and the projection key the seat on, and its machine name is what a sentence
/// and the work graph name it by. A refusal is the resolver's own, with the
/// exit it carries; a seat list nobody could read is could-not-tell, never a
/// fleet with no seats.
pub(crate) fn seat_named(machine_dir: &Path, arg: &str) -> Result<config::Seat, Stop> {
    let path = machine_dir.join("config.json");
    let machine = config::read(&path).map_err(|why| {
        Stop::could_not_tell(format!(
            "the seat list could not be read, so no seat can be named: {why}"
        ))
    })?;
    machine.resolve(arg).cloned().map_err(Stop::from)
}

/// The slot the pack layers carry a transient seat's permission rules in.
const PERMISSIONS: &str = "overlay/per-provider/claude/permissions.json";

/// The overlay directory whose files belong in a spawned seat's own
/// CONFIGURATION space, rather than in the worktree or on the plugin root. That
/// directory holds only what the pack's overlay puts there, so nothing from the
/// person's home directory — settings, memory, instructions, servers — reaches
/// a spawned seat.
///
/// The defaults carry none today: the guards reach a session through the plugin root
/// and the permissions through the worktree's own settings file, so a spawned
/// seat's configuration directory is empty and its emptiness is the isolation.
/// The read is here so a pack that starts carrying one needs no second edit.
const CONFIG_OVERLAY: &str = "overlay/per-provider/claude/config";

/// The settings document a spawn writes into the seat's worktree, rendered out
/// of the pack layers with everything but the worktree filled in.
///
/// `{worktree}` IS HANDED BACK TO THE RENDERER AS ITSELF, because the path is
/// claimed inside the spawn and no caller knows it: one pass here keeps
/// [`render`]'s guarantee that a placeholder nobody offered is an error rather
/// than a literal a seat would come up under, and leaves the spawn one
/// substitution.
///
/// A layering that carries no such file REFUSES the spawn. A seat started
/// without rules under this posture can neither edit nor commit, and it reports
/// that as a wall of denials hours later rather than as a refusal here.
///
/// `{touched}` is the builder's checks, the command the caller handed in,
/// escaped as JSON because it is written inside one of the document's
/// strings. A spawn handed none gets NO RULE for one: the entry carrying the
/// placeholder is taken out before the render, so the seat's rules never name
/// a command nobody gave.
fn settings_of(here: &Here, touched: Option<&str>) -> Result<String, Stop> {
    here.project.refuse_moved()?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    let template = packs.read(PERMISSIONS)?;
    let (template, touched) = match touched.map(str::trim).filter(|c| !c.is_empty()) {
        Some(command) => (template, inside_a_json_string(command)),
        None => (without_the_touched_rule(template)?, String::new()),
    };
    let rendered = render(
        &template,
        &[("touched", &touched), ("worktree", transient::WORKTREE)],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{PERMISSIONS}` names `{{{name}}}`, which a spawn has no value for"
        ))
    })?;
    with_tool_commands(rendered, &tool_commands_of(here)?)
}

/// The placeholder a permissions template writes the builder's checks under.
const TOUCHED: &str = "{touched}";

/// A command as the characters that stand for it between a JSON string's
/// quotes.
fn inside_a_json_string(command: &str) -> String {
    let quoted = serde_json::Value::String(command.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

/// The template with every allow entry that names `{touched}` taken out.
///
/// A template that names none is handed back UNTOUCHED rather than round-tripped
/// through a parse, so a pack whose rules carry no builder's checks comes up under
/// its own bytes whatever the dispatch was handed.
fn without_the_touched_rule(template: String) -> Result<String, Stop> {
    if !template.contains(TOUCHED) {
        return Ok(template);
    }
    let mut doc: serde_json::Value = serde_json::from_str(&template).map_err(|why| {
        Stop::could_not_tell(format!(
            "`{PERMISSIONS}` names {TOUCHED} and is not the JSON its rule can be taken out of: \
             {why}"
        ))
    })?;
    if let Some(allow) = doc
        .get_mut("permissions")
        .and_then(|permissions| permissions.get_mut("allow"))
        .and_then(serde_json::Value::as_array_mut)
    {
        allow.retain(|rule| !rule.as_str().is_some_and(|rule| rule.contains(TOUCHED)));
    }
    let mut out = serde_json::to_string_pretty(&doc).map_err(|why| {
        Stop::could_not_tell(format!(
            "the seat's own settings could not be written: {why}"
        ))
    })?;
    out.push('\n');
    Ok(out)
}

/// `[permissions] tool_commands`, checked entry by entry to be one command word.
///
/// The check is AT THE RENDER and names the entry, because the rule it is
/// written into is matched by its opening token: a word carrying a space, a
/// glob character or a leading dash would widen a seat's posture past the list
/// the project meant to write, and the widening would not be visible anywhere
/// but in the seat's own settings file hours later.
fn tool_commands_of(here: &Here) -> Result<Vec<String>, Stop> {
    let declared =
        match fleet_core::policy::read("permissions", "tool_commands", &here.project.policy) {
            Ok(Some(value)) => value,
            _ => return Ok(Vec::new()),
        };
    let entries = declared.as_array().ok_or_else(|| {
        Stop::usage("[permissions] tool_commands is not a list of command words".to_string())
    })?;
    let mut words = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry.as_str().filter(|word| is_command_word(word)) {
            Some(word) => words.push(word.to_string()),
            None => {
                return Err(Stop::usage(format!(
                    "[permissions] tool_commands entry `{entry}` is not one command word"
                )))
            }
        }
    }
    Ok(words)
}

/// One command word: a bare name, or a path relative to the repository.
///
/// Everything refused here is refused for the same reason — it would put
/// something other than a command word inside `Bash(<word>:*)`.
fn is_command_word(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && !word
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '*' | '?' | '[' | ']'))
}

/// The project's words as rules after the pack's own allow list, deduplicated
/// against it.
///
/// A project declaring none gets the rendered document back UNTOUCHED rather
/// than round-tripped through a parse: the bytes a seat comes up under are then
/// the pack's document with two values in it, which is the claim
/// [`a_spawn_renders_the_packs_permission_rules_into_the_seats_worktree`] makes
/// of every fleet that declares no toolchain.
fn with_tool_commands(rendered: String, words: &[String]) -> Result<String, Stop> {
    if words.is_empty() {
        return Ok(rendered);
    }
    let mut doc: serde_json::Value = serde_json::from_str(&rendered).map_err(|why| {
        Stop::could_not_tell(format!(
            "`{PERMISSIONS}` is not the JSON a rule can be added to: {why}"
        ))
    })?;
    let allow = doc
        .get_mut("permissions")
        .and_then(|permissions| permissions.get_mut("allow"))
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| {
            Stop::could_not_tell(format!(
                "`{PERMISSIONS}` carries no `permissions.allow` list for [permissions] \
                 tool_commands to be added to"
            ))
        })?;
    for word in words {
        let rule = serde_json::Value::String(format!("Bash({word}:*)"));
        if !allow.contains(&rule) {
            allow.push(rule);
        }
    }
    let mut out = serde_json::to_string_pretty(&doc).map_err(|why| {
        Stop::could_not_tell(format!(
            "the seat's own settings could not be written: {why}"
        ))
    })?;
    out.push('\n');
    Ok(out)
}

/// The overlay files a spawned seat's configuration directory is seeded with.
fn config_files_of(here: &Here) -> Result<Vec<(String, String)>, Stop> {
    Packs::under(&here.packs_dir, &here.defaults_dir)?.read_under(CONFIG_OVERLAY)
}

/// The policy the verbs read, from the file the machine's seat list names.
pub(crate) fn policy_of(here: &Here) -> Result<controller::Policy, Stop> {
    let machine = config::read(&here.machine_dir.join("config.json"))
        .map_err(|cause| Stop::could_not_tell(format!("the seat list: {cause}")))?;
    controller::load(&machine.fleet_toml).map_err(Stop::could_not_tell)
}

/// The adapter with its effect binary resolved ONCE, which is the contract
/// `ClaudeCode` states: a verb that let it fall back to a bare name would exec
/// a file nothing checked.
pub(crate) fn effect_agent(here: &Here, home: &Path) -> Result<ClaudeCode, Stop> {
    let agent = ClaudeCode::new(home, &here.machine_dir);
    let child_path = platform::child_path(home);
    let bin = ClaudeCode::resolve_effect_bin(
        fleet_controller::adapter::claude_code::configured_bin().as_deref(),
        &child_path,
    )
    .map_err(Stop::could_not_tell)?;
    Ok(agent.with_effect_bin(bin))
}

/// The first turn's TEXT. A file that is not there is a usage error and not a
/// refusal: nothing about the fleet is wrong, the call named a path this
/// process cannot read.
fn turn_text(path: &Path) -> Result<String, Stop> {
    std::fs::read_to_string(path).map_err(|e| {
        Stop::usage(format!(
            "the first turn at {} could not be read: {e}",
            path.display()
        ))
    })
}

pub(crate) fn stopped(verb: &str, stop: &Stop, json: bool) -> Exit {
    refusing(verb, stop.code, &stop.message, json)
}

fn refused(verb: &str, refusal: &Refusal, json: bool) -> Exit {
    refusing(verb, refusal.code, &refusal.message, json)
}

/// The one refusal path: the person's line on stderr, the document on stdout
/// where one was asked for, and the row on `$?`.
///
/// THE ROW IS READ ONCE AND USED FOR BOTH, so the `code` a caller branches on
/// and the number it reads from `$?` cannot come apart — including on a status
/// outside the exit table, which lands on could-not-tell in both places.
fn refusing(verb: &str, code: u8, message: &str, json: bool) -> Exit {
    let exit = Exit::from_status(code).unwrap_or(Exit::CouldNotTell);
    eprintln!("fleet {verb}: {message}");
    if json {
        println!("{}", envelope::refusal(verb, exit, message));
    }
    exit
}

/// A turn's first line, which is what the envelope carries of it.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// The spawner's exit-to-outcome table, which nothing outside this binary can
/// reach: the cli is a binary crate, so a `cfg(test)` module here is the one
/// route to [`outcome_of`] from an arm.
#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::item::{REFUSED, USAGE};

    /// AC3 of the could-not-tell spec, the half no end-to-end arm can reach: `turn_text` is
    /// the only usage-coded leg the spawner has, and `dispatch` writes the file
    /// it then reads, so a usage error cannot be staged through the binary. The
    /// withdrawal the other half of AC3 names is measured against a REFUSED leg
    /// end-to-end in `fleet/cli/tests/dispatch.rs`.
    #[test]
    fn only_the_could_not_tell_exit_leaves_the_order_standing() {
        assert_eq!(
            outcome_of(USAGE, String::from("the first turn could not be read")),
            SpawnOutcome::Refused(String::from("the first turn could not be read")),
            "a usage error is dispatch's own defect and withdraws the order"
        );
        assert_eq!(
            outcome_of(REFUSED, String::from("the belt is full")),
            SpawnOutcome::Refused(String::from("the belt is full")),
            "a refusal is a verdict"
        );
        assert_eq!(
            outcome_of(
                COULD_NOT_TELL,
                String::from("the agent binary is unresolvable")
            ),
            SpawnOutcome::CouldNotTell(String::from("the agent binary is unresolvable")),
            "a could-not-tell is a question, and the order stands under it"
        );
    }
}
