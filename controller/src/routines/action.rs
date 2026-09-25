//! What a due routine DOES: ring a seat, file an item, or run a command.
//!
//! Every child an action starts gets the constructed environment — the
//! platform's own search path and the four values a shell needs — with two
//! additions: the machine directory, so a tool a routine runs finds the fleet it
//! belongs to, and the directory of this executable at the FRONT of the path, so
//! a routine that calls a fleet verb by name reaches this binary and never a bare
//! name on a service's own search path.

use super::file::{Item, Routine, Run, When};
use super::{Outcome, SeatView};
use crate::adapter::Agent;
use crate::observe::RosterState;
use crate::platform;
use crate::policy::Policy;
use fleet_core::seat::actor::{Actor, ActorKind};
use fleet_core::store::{self, Filter, NewItem, Opening, Update};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// What one action produced, in the shape the terminal event is written from.
pub struct Done {
    pub outcome: Outcome,
    pub detail: String,
    /// The extra fields this outcome's event carries — the item filed, the open
    /// ones a dedupe found, the log an exec wrote.
    pub extra: Vec<(String, serde_json::Value)>,
}

impl Done {
    fn plain(outcome: Outcome, detail: String) -> Done {
        Done {
            outcome,
            detail,
            extra: Vec::new(),
        }
    }
}

/// Everything an action reads about the machine it runs on. Gathered by the
/// caller, so this layer reads no configuration for itself.
pub struct Machine<'a> {
    pub machine_dir: &'a Path,
    pub child_path: &'a str,
    pub policy: &'a Policy,
    /// The seat rows this tick observed, for a nudge's addressee.
    pub seats: &'a [SeatView],
    /// `None` is effects off, with the cause, which turns a nudge into
    /// could-not-tell and leaves an item and an exec running.
    pub agent: Option<&'a dyn Agent>,
    pub effects_off: Option<String>,
}

/// The label that makes "did this routine already file one" answerable by
/// anything other than this process's own memory.
pub fn routine_label(name: &str) -> String {
    format!("routine:{name}")
}

/// Who a routine acts as: `routine:<name>`, the typed actor its every write to
/// the work graph carries — the `--by` of the verbs it runs, and the `by` of
/// the items it files, which the store records as their author.
pub fn routine_actor(name: &str) -> Actor {
    Actor {
        kind: ActorKind::Routine,
        id: name.to_string(),
    }
}

/// The type an item that names none is filed as: the built-in store's own
/// default — measured on its pinned release, 1.3.0 — made explicit, so every
/// store files the same item the same way.
pub const DEFAULT_ITEM_TYPE: &str = "task";

/// The environment every child of an action carries.
pub fn child_command(program: &str, machine: &Machine) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear();
    cmd.env("PATH", path_for_children(machine.child_path));
    for pass in crate::adapter::claude_code::PASSED_THROUGH {
        if let Ok(value) = std::env::var(pass) {
            cmd.env(pass, value);
        }
    }
    cmd.env("FLEET_DIR", machine.machine_dir);
    cmd
}

/// The shell an exec and a check run under, as an ABSOLUTE path resolved on the
/// constructed search path.
///
/// A bare name would be resolved against whatever `PATH` this process itself
/// carries, which under a service manager is not the one the child is given — so
/// the binary that ran and the search path the fleet constructed would be two
/// different answers.
pub fn shell(machine: &Machine) -> String {
    platform::resolve_on_path(&path_for_children(machine.child_path), "sh")
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "/bin/sh".to_string())
}

/// This executable's own directory in front of the constructed path.
///
/// An executable this process cannot name leaves the constructed path alone,
/// which is a verb a routine has to spell in full rather than a name resolved to
/// the wrong file.
pub fn path_for_children(child_path: &str) -> String {
    let own = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    match own {
        Some(dir) if !dir.as_os_str().is_empty() => {
            format!("{}:{child_path}", dir.display())
        }
        _ => child_path.to_string(),
    }
}

/// Carry one routine's action out.
pub fn run(routine: &Routine, machine: &Machine) -> Done {
    let action = &routine.action;
    if let Some(exec) = &action.exec {
        return run_exec(routine, &exec.command, machine);
    }
    if let Some(workflow) = &action.run {
        return run_workflow(routine, workflow, machine);
    }
    if let Some(nudge) = &action.nudge {
        let rung = run_nudge(nudge, machine);
        // The fallback pair: a ring that found nobody leaves the duty on the
        // work graph, and the item's own outcome is the routine's.
        if rung.outcome == Outcome::Absent {
            if let Some(item) = action
                .item
                .as_ref()
                .filter(|item| item.when == When::Absent)
            {
                let mut filed = file_item(routine, item, machine);
                filed
                    .extra
                    .push(("fallback_from".to_string(), "nudge".into()));
                return filed;
            }
        }
        return rung;
    }
    match &action.item {
        Some(item) => file_item(routine, item, machine),
        None => Done::plain(
            Outcome::Failed,
            "the routine carries no action to run".to_string(),
        ),
    }
}

/// The argv a dry run prints, without running anything. An item's is the
/// store request its filing sends: `store`, `create` and the request.
pub fn argv_of(routine: &Routine, machine: &Machine) -> Vec<String> {
    let action = &routine.action;
    if let Some(exec) = &action.exec {
        return vec![shell(machine), "-c".to_string(), exec.command.clone()];
    }
    if let Some(workflow) = &action.run {
        let binary = fleet_binary(machine).unwrap_or_else(|| FLEET.to_string());
        return run_argv(&binary, routine, workflow);
    }
    if let Some(nudge) = &action.nudge {
        let seat = view_of(nudge, machine);
        let (worktree, display) = match seat {
            Some(row) => (row.worktree.clone(), row.session_name.clone()),
            None => (String::new(), nudge.seat.clone()),
        };
        return vec![
            "claude".to_string(),
            "-p".to_string(),
            "--model".to_string(),
            machine.policy.nudge_model.clone(),
            crate::effect::nudge_prompt(&display, &nudge_text(nudge)),
            format!("(in {worktree})"),
        ];
    }
    match &action.item {
        Some(item) => vec![
            "store".to_string(),
            "create".to_string(),
            match new_item(routine, item) {
                Ok(filed) => create_request(routine, &filed),
                Err(why) => format!("(not sent: {why})"),
            },
        ],
        None => Vec::new(),
    }
}

/// The sentence one ring carries: the routine's own text, and the authority
/// behind it on its own line.
fn nudge_text(nudge: &super::file::Nudge) -> String {
    format!("{}\nauthority: {}", nudge.text, nudge.authority)
}

/// The view of the seat the nudge resolved to at load, found by its id: the
/// name the file gave is for the lines, and the row is the one the id holds.
fn view_of<'a>(nudge: &super::file::Nudge, machine: &'a Machine) -> Option<&'a SeatView> {
    let id = nudge.seat_id?;
    machine.seats.iter().find(|row| row.id == id)
}

fn run_nudge(nudge: &super::file::Nudge, machine: &Machine) -> Done {
    let Some(seat) = view_of(nudge, machine) else {
        return Done::plain(
            Outcome::CouldNotTell,
            format!(
                "the seat `{}` names no row of this machine's seat list",
                nudge.seat
            ),
        );
    };
    match seat.state {
        RosterState::Present | RosterState::PromptBlocked => {}
        RosterState::Unknown => {
            return Done::plain(
                Outcome::CouldNotTell,
                format!(
                    "the roster does not say whether `{}` has a live session",
                    nudge.seat
                ),
            )
        }
        other => {
            return Done::plain(
                Outcome::Absent,
                format!(
                    "`{}` has no live session; the roster reads {}",
                    nudge.seat,
                    other.as_str()
                ),
            )
        }
    }
    let Some(agent) = machine.agent else {
        return Done::plain(
            Outcome::CouldNotTell,
            machine
                .effects_off
                .clone()
                .unwrap_or_else(|| "effects are off".to_string()),
        );
    };
    let text = nudge_text(nudge);
    let sent = agent.nudge(
        seat.config_dir.as_deref().map(Path::new),
        &seat.session_name,
        &seat.worktree,
        &machine.policy.nudge_model,
        &crate::effect::nudge_prompt(&seat.session_name, &text),
        Duration::from_secs(machine.policy.nudge_timeout_seconds),
    );
    match sent {
        Ok(()) => Done::plain(
            Outcome::Delivered,
            format!("the ring reached `{}` in {}", nudge.seat, seat.worktree),
        ),
        Err(cause) => Done::plain(
            Outcome::Failed,
            format!("the ring to `{}` did not land: {cause}", nudge.seat),
        ),
    }
}

/// The item the routine files, as the store contract takes one: the file's
/// fields, the default type where it names none, and the routine's own label
/// always among the labels. HELD TO THE CONTRACT here, before anything is
/// sent, so an item a store must not file never reaches one — the refusal is
/// the reason why.
fn new_item(routine: &Routine, item: &Item) -> Result<NewItem, String> {
    let mut labels = item.labels.clone();
    let label = routine_label(&routine.name);
    if !labels.contains(&label) {
        labels.push(label);
    }
    let priority = item
        .priority
        .map(|n| u8::try_from(n).map_err(|_| format!("priority is {n}; the range is 0 to 4")))
        .transpose()?;
    let filed = NewItem {
        title: item.title.clone(),
        description: item.description.clone().unwrap_or_default(),
        item_type: item
            .kind
            .clone()
            .unwrap_or_else(|| DEFAULT_ITEM_TYPE.to_string()),
        labels,
        priority,
    };
    filed.validate()?;
    Ok(filed)
}

/// The create's request as one line of the contract's JSON, without the root,
/// which is the machine's and not the routine's.
fn create_request(routine: &Routine, filed: &NewItem) -> String {
    let fields = serde_json::json!({ "item": filed, "by": routine_actor(&routine.name) });
    let serde_json::Value::Object(fields) = fields else {
        return fields.to_string();
    };
    let mut request = store::types::request(fields, &routine.project_root);
    if let Some(fields) = request.as_object_mut() {
        fields.remove("root");
    }
    request.to_string()
}

/// File the routine's item through the project's own store, as the routine.
///
/// The item is held to the contract first, so one a store must not file is
/// failed before the store is asked anything. The store is then OPENED AS
/// EVERY VERB OPENS IT, out of the project's own file, strictly on the
/// constructed child path and under the routine's own bound. The dedupe is
/// the store's label listing, every row of it; the item is created, then
/// handed to the seat the load resolved its assignee to. The routine's actor
/// is the item's author on the record, so no note says so again [ASSUMES D9].
fn file_item(routine: &Routine, item: &Item, machine: &Machine) -> Done {
    let filed = match new_item(routine, item) {
        Ok(filed) => filed,
        Err(why) => return Done::plain(Outcome::Failed, format!("the item was not filed: {why}")),
    };
    let policy = match store::project_policy(&routine.project_root) {
        Ok(policy) => policy,
        Err(why) => return Done::plain(Outcome::CouldNotTell, why.to_string()),
    };
    let opened = store::open(&Opening {
        root: &routine.project_root,
        policy: &policy,
        search_path: &path_for_children(machine.child_path),
        strict: true,
        timeout: Duration::from_secs(routine.timeout),
    });
    let store = match opened {
        Ok(store) => store,
        Err(why) => {
            return Done::plain(
                Outcome::CouldNotTell,
                format!("the store could not be opened: {why}"),
            )
        }
    };
    let by = routine_actor(&routine.name);
    let label = routine_label(&routine.name);

    if item.dedupe.as_deref() == Some("open") {
        match store.list(&Filter::Label(label.clone())) {
            Err(why) => {
                return Done::plain(
                    Outcome::CouldNotTell,
                    format!("the dedupe query could not be read: {why}"),
                )
            }
            Ok(rows) if !rows.is_empty() => {
                let existing: Vec<String> = rows.iter().map(|row| row.id.to_string()).collect();
                return Done {
                    outcome: Outcome::Deduped,
                    detail: format!(
                        "an open item already carries {label}: {}",
                        existing.join(", ")
                    ),
                    extra: vec![("existing".to_string(), existing.into())],
                };
            }
            Ok(_) => {}
        }
    }

    let id = match store.create(&filed, &by) {
        Ok(id) => id,
        Err(why) => return Done::plain(Outcome::Failed, format!("the create did not land: {why}")),
    };
    // Every answer from here on names the item: it is on the record whatever
    // becomes of its assignee.
    let named = |outcome: Outcome, detail: String| Done {
        outcome,
        detail,
        extra: vec![("item".to_string(), id.to_string().into())],
    };
    match (&item.assignee, item.assignee_id) {
        (_, Some(seat)) => {
            if let Err(why) = store.update(&id, &Update::assignee(seat), &by) {
                return named(
                    Outcome::Failed,
                    format!("{id} was filed, and its assignee did not land: {why}"),
                );
            }
        }
        (Some(said), None) => {
            return named(
                Outcome::Failed,
                format!(
                    "the routine's assignee {said} names no seat of this fleet — {id} was \
                     filed unassigned"
                ),
            )
        }
        (None, None) => {}
    }
    named(Outcome::Filed, format!("filed {id}"))
}

fn run_exec(routine: &Routine, command: &str, machine: &Machine) -> Done {
    let log = log_path(machine.machine_dir, &routine.name, "exec");
    let mut cmd = child_command(&shell(machine), machine);
    cmd.arg("-c")
        .arg(command)
        .current_dir(&routine.project_root);
    match platform::run_bounded_to_file(cmd, &log, Duration::from_secs(routine.timeout)) {
        Ok(exit) if exit.ok => Done {
            outcome: Outcome::Ran,
            detail: format!("the command exited 0; its output is at {}", log.display()),
            extra: vec![("log".to_string(), log.display().to_string().into())],
        },
        Ok(exit) => Done {
            outcome: Outcome::Failed,
            detail: format!(
                "the command exited {}; its output is at {}",
                exit.code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "on a signal".to_string()),
                log.display()
            ),
            extra: vec![("log".to_string(), log.display().to_string().into())],
        },
        Err(why) => Done {
            outcome: Outcome::Failed,
            detail: format!("{why}; its output is at {}", log.display()),
            extra: vec![("log".to_string(), log.display().to_string().into())],
        },
    }
}

/// The verb a run action calls, by name, on the constructed child path — which
/// has this executable's own directory in front, so the `fleet` a routine runs
/// is the one that fired it.
const FLEET: &str = "fleet";

fn fleet_binary(machine: &Machine) -> Option<String> {
    platform::resolve_on_path(&path_for_children(machine.child_path), FLEET)
        .map(|path| path.display().to_string())
}

/// `fleet run <workflow> [--input key=value]… --by routine:<name>`: the routine
/// is the runner, so `run.started` carries it as the actor, typed — a bare name
/// would be read as a seat argument, and a routine is no seat.
fn run_argv(binary: &str, routine: &Routine, workflow: &Run) -> Vec<String> {
    let mut argv = vec![
        binary.to_string(),
        "run".to_string(),
        workflow.workflow.clone(),
    ];
    for (key, value) in &workflow.inputs {
        argv.push("--input".to_string());
        argv.push(format!("{key}={value}"));
    }
    argv.push("--by".to_string());
    argv.push(routine_actor(&routine.name).to_string());
    argv
}

/// The run's id, off the verb's own first line — `<id> — <hash>` — which is
/// the only place it exists: the store names the record when the run opens,
/// so the routine's opening event cannot carry it and the terminal one does.
fn run_id_of(stdout: &str) -> Option<String> {
    let first = stdout.lines().next()?;
    let (id, _) = first.split_once(" — ")?;
    Some(id.to_string()).filter(|id| !id.is_empty())
}

/// The word the verb's second line ends on, where it printed one.
fn run_word_of(stdout: &str) -> Option<String> {
    let second = stdout.lines().nth(1)?;
    let (_, word) = second.split_once(" — ")?;
    Some(word.to_string())
}

/// Call `fleet run` in the routine's project root, bounded by the routine's
/// own timeout, and read the run off what the verb printed.
///
/// THE VERB'S EXIT IS THE OUTCOME: 0 is a run that closed or is waiting, 1 a
/// run that failed, 3 one nobody could classify — the shared exit table, read
/// back rather than re-derived from the stream. Both streams go to a log
/// beside an exec's, and the run id rides the terminal event as `run`.
fn run_workflow(routine: &Routine, workflow: &Run, machine: &Machine) -> Done {
    let log = log_path(machine.machine_dir, &routine.name, "run");
    let Some(binary) = fleet_binary(machine) else {
        return Done::plain(
            Outcome::CouldNotTell,
            format!("no `{FLEET}` on the constructed child PATH, so no run can be opened"),
        );
    };
    let argv = run_argv(&binary, routine, workflow);
    let mut cmd = child_command(&argv[0], machine);
    cmd.args(&argv[1..]).current_dir(&routine.project_root);
    let ran = match platform::run_bounded(cmd, Duration::from_secs(routine.timeout)) {
        Ok(ran) => ran,
        Err(why) => {
            return Done::plain(
                Outcome::Failed,
                format!("`{FLEET} run {}` {why}", workflow.workflow),
            )
        }
    };
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let (stdout, stderr) = (
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr),
    );
    let _ = std::fs::write(&log, format!("{stdout}{stderr}"));
    let mut extra: Vec<(String, serde_json::Value)> =
        vec![("log".to_string(), log.display().to_string().into())];
    let run = run_id_of(&stdout);
    if let Some(id) = &run {
        extra.push(("run".to_string(), id.clone().into()));
    }
    let named = run.as_deref().unwrap_or("no run");
    let said = run_word_of(&stdout)
        .or_else(|| stderr.lines().last().map(str::to_string))
        .unwrap_or_default();
    let (outcome, detail) = match ran.status.code() {
        Some(0) if run.is_some() => (Outcome::Ran, format!("{named} {said}")),
        Some(0) => (
            Outcome::CouldNotTell,
            format!("`{FLEET} run` exited 0 and printed no run line; {said}"),
        ),
        Some(3) => (
            Outcome::CouldNotTell,
            format!("{named} could not be told: {said}"),
        ),
        Some(code) => (Outcome::Failed, format!("{named} exited {code}: {said}")),
        None => (Outcome::Failed, format!("{named} ended on a signal")),
    };
    Done {
        outcome,
        detail: format!("{detail}; the verb's output is at {}", log.display()),
        extra,
    }
}

/// Where one run's output goes: one file per routine per kind, under the machine
/// directory's routines logs.
pub fn log_path(machine_dir: &Path, name: &str, kind: &str) -> PathBuf {
    super::state::logs_dir_in(machine_dir).join(format!("{name}-{kind}.log"))
}

/// Run a condition's check the way the tick runs it: `sh -c` in the order's
/// project root, stdin null, both streams on a file, bounded by
/// `check_timeout` through the controller's own runner.
pub fn run_check(routine: &Routine, machine: &Machine) -> super::trigger::CheckOutcome {
    use super::trigger::CheckOutcome;
    let log = log_path(machine.machine_dir, &routine.name, "check");
    let Some(command) = routine.check.as_deref() else {
        return CheckOutcome::NotStarted("the routine names no check".to_string());
    };
    let mut cmd = child_command(&shell(machine), machine);
    cmd.arg("-c")
        .arg(command)
        .current_dir(&routine.project_root);
    match platform::run_bounded_to_file(cmd, &log, Duration::from_secs(routine.check_timeout)) {
        Ok(exit) => match exit.code {
            Some(status) => CheckOutcome::Exited(status),
            // A child that ended on a signal carries no status the routine's own
            // table can speak about, which is the same answer as a deadline.
            None => CheckOutcome::Timeout("the check ended on a signal".to_string()),
        },
        Err(why) if why.contains("did not answer within") => CheckOutcome::Timeout(why),
        Err(why) => CheckOutcome::NotStarted(why),
    }
}
