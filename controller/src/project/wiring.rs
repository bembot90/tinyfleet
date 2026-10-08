//! What a transient-seat verb and the run pass are wired with, from a resolved
//! project: its two directories, the agent, the host and the policy, gathered
//! into the controller's `transient::Machine`.

use std::path::{Path, PathBuf};

use fleet_core::agent::{self, Agent, Permissions};
use fleet_core::item::Stop;

use super::{open_store, Here};
use crate::host::{Host, TmuxHost};
use crate::policy::{load, Policy};
use crate::transient::{self, Machine};
use crate::{config, platform};

/// The two directories a verb acts in, resolved once so the borrow they are
/// handed down as has an owner in the caller's frame.
pub struct Where {
    primary: PathBuf,
    worktrees: PathBuf,
}

impl Where {
    pub fn of(here: &Here) -> Result<Where, Stop> {
        Ok(Where {
            primary: here.primary()?,
            worktrees: here.worktrees_dir()?,
        })
    }
}

pub fn machine_of<'a>(
    here: &'a Here,
    at: &'a Where,
    agent: &'a dyn Agent,
    host: &'a dyn Host,
    policy: &'a Policy,
) -> Machine<'a> {
    Machine {
        machine_dir: &here.machine_dir,
        agent,
        host,
        policy,
        project: &here.project.name,
        primary: &at.primary,
        worktrees_dir: &at.worktrees,
        // This is where the two overrides and the platform are read, for a
        // binary's verbs and the run seam's Engine, which is what keeps a
        // variable out of a process that forks children while its own threads
        // are running.
        readings: transient::Readings::taken(),
    }
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
pub fn seat_named(machine_dir: &Path, arg: &str) -> Result<config::Seat, Stop> {
    let path = machine_dir.join("config.json");
    let machine = config::read(&path).map_err(|why| {
        Stop::could_not_tell(format!(
            "the seat list could not be read, so no seat can be named: {why}"
        ))
    })?;
    machine.resolve(arg).cloned().map_err(Stop::from)
}

/// What a spawned seat may run without asking, in fleet's own words: the
/// store's own command word, the project's `[permissions] tool_commands` after
/// it, and the builder's checks, the one command the caller handed in. A spawn
/// handed none names none, and its seat's rules then name no command nobody
/// gave.
///
/// THE STORE'S WORD IS THE STORE'S TO DECLARE (`capabilities().cli`), rendered
/// as a tool command is, so no store's name is written in core: a seat reaches
/// its fleet's store from its shell whichever store that is. A store that
/// does not open, or declares no word, adds none — no seat reaches it from a
/// shell.
pub fn permissions_of(here: &Here, touched: Option<&str>) -> Result<Permissions, Stop> {
    here.project.refuse_moved()?;
    let mut commands: Vec<String> = store_word_of(here).into_iter().collect();
    for word in tool_commands_of(here)? {
        if !commands.contains(&word) {
            commands.push(word);
        }
    }
    Ok(Permissions {
        commands,
        touched: touched
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(str::to_string),
    })
}

/// The first word a seat types to reach this project's store, as the store
/// declares it.
fn store_word_of(here: &Here) -> Option<String> {
    open_store(here).ok()?.capabilities().ok()?.cli
}

/// `[permissions] tool_commands`, checked entry by entry to be one command word.
///
/// The check is AT THE CALL and names the entry, because the rule it is
/// rendered into is matched by its opening token: a word carrying a space, a
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
/// something other than a command word inside the rule it is rendered into.
fn is_command_word(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && !word
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '*' | '?' | '[' | ']'))
}

/// The policy the verbs read, from the file the machine's seat list names.
pub fn policy_of(here: &Here) -> Result<Policy, Stop> {
    load(&fleet_toml_of(here)?).map_err(Stop::could_not_tell)
}

/// The file the machine's seat list names: the fleet's own.
fn fleet_toml_of(here: &Here) -> Result<PathBuf, Stop> {
    config::read(&here.machine_dir.join("config.json"))
        .map(|machine| machine.fleet_toml)
        .map_err(|cause| Stop::could_not_tell(format!("the seat list: {cause}")))
}

/// The agent, opened the one way every caller opens it — an agent that
/// cannot issue effects is a refusal here, naming why its own answers say so.
pub fn effect_agent(here: &Here, home: &Path) -> Result<Box<dyn Agent>, Stop> {
    let setting = agent::Setting::read(&fleet_toml_of(here)?, &here.machine_dir)
        .map_err(Stop::could_not_tell)?;
    let search_path = platform::child_path(home);
    let opened = agent::open(&setting.opening(&search_path)).map_err(Stop::could_not_tell)?;
    match opened.effects_off {
        Some(why) => Err(Stop::could_not_tell(why)),
        None => Ok(Box::new(opened.agent)),
    }
}
/// The host a spawn starts its session on, resolved ONCE on the constructed
/// `PATH` the adapter's children carry, or the refusal naming why there is
/// none — for a verb whose whole act is a session started, and which must not
/// claim a name or make a worktree for a start that cannot happen.
pub fn spawn_host(home: &Path) -> Result<TmuxHost, Stop> {
    TmuxHost::resolve(&platform::child_path(home)).map_err(Stop::could_not_tell)
}

/// The host for a verb that starts no session: resolved the same way, and
/// where it does not resolve, a host that refuses every call with the cause —
/// so a machine with no host can still feed and retire, and a call that did
/// need one names why it failed.
pub fn verb_host(home: &Path) -> Box<dyn Host> {
    crate::host::resolve(&platform::child_path(home))
}
