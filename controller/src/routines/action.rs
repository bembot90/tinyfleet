//! What a due routine DOES (PRD R23): ring a seat, file an item, or run a
//! command.
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
pub fn run(routine: &Routine, machine: &Machine, now_stamp: &str) -> Done {
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
                let mut filed = file_item(routine, item, machine, now_stamp);
                filed
                    .extra
                    .push(("fallback_from".to_string(), "nudge".into()));
                return filed;
            }
        }
        return rung;
    }
    match &action.item {
        Some(item) => file_item(routine, item, machine, now_stamp),
        None => Done::plain(
            Outcome::Failed,
            "the routine carries no action to run".to_string(),
        ),
    }
}

/// The argv a dry run prints, without running anything.
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
        let seat = machine.seats.iter().find(|row| row.seat_dir == nudge.seat);
        let (worktree, display) = match seat {
            Some(row) => (row.worktree.clone(), row.display_name.clone()),
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
        Some(item) => item_argv("bd", routine, item),
        None => Vec::new(),
    }
}

/// The sentence one ring carries: the routine's own text, and the authority
/// behind it on its own line.
fn nudge_text(nudge: &super::file::Nudge) -> String {
    format!("{}\nauthority: {}", nudge.text, nudge.authority)
}

fn run_nudge(nudge: &super::file::Nudge, machine: &Machine) -> Done {
    let Some(seat) = machine.seats.iter().find(|row| row.seat_dir == nudge.seat) else {
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
        &seat.seat_dir,
        &seat.worktree,
        &machine.policy.nudge_model,
        &crate::effect::nudge_prompt(&seat.display_name, &text),
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

/// The item's own argv, with the routine's label always appended.
fn item_argv(binary: &str, routine: &Routine, item: &Item) -> Vec<String> {
    let mut argv = vec![
        binary.to_string(),
        "-C".to_string(),
        routine.project_root.display().to_string(),
        "create".to_string(),
        item.title.clone(),
        "--json".to_string(),
        "--actor".to_string(),
        ACTOR.to_string(),
    ];
    if let Some(text) = &item.description {
        argv.push("--description".to_string());
        argv.push(text.clone());
    }
    if let Some(kind) = &item.kind {
        argv.push("--type".to_string());
        argv.push(kind.clone());
    }
    if let Some(assignee) = &item.assignee {
        argv.push("--assignee".to_string());
        argv.push(assignee.clone());
    }
    if let Some(priority) = item.priority {
        argv.push("--priority".to_string());
        argv.push(priority.to_string());
    }
    let mut labels = item.labels.clone();
    let label = routine_label(&routine.name);
    if !labels.contains(&label) {
        labels.push(label);
    }
    argv.push("--labels".to_string());
    argv.push(labels.join(","));
    argv
}

/// The actor every write a routine makes to the work graph carries.
pub const ACTOR: &str = "fleet-controller";

fn file_item(routine: &Routine, item: &Item, machine: &Machine, now_stamp: &str) -> Done {
    let Some(binary) = platform::resolve_on_path(&path_for_children(machine.child_path), "bd")
    else {
        return Done::plain(
            Outcome::CouldNotTell,
            "no `bd` on the constructed child PATH, so nothing can be filed".to_string(),
        );
    };
    let binary = binary.display().to_string();
    let bound = Duration::from_secs(routine.timeout);
    let label = routine_label(&routine.name);

    if item.dedupe.as_deref() == Some("open") {
        let mut cmd = child_command(&binary, machine);
        cmd.args([
            "-C",
            &routine.project_root.display().to_string(),
            "list",
            "--label",
            &label,
            "--status",
            "open",
            "--json",
        ]);
        let run = match platform::run_bounded(cmd, bound) {
            Ok(run) => run,
            Err(why) => {
                return Done::plain(
                    Outcome::CouldNotTell,
                    format!("the dedupe query could not be run: {why}"),
                )
            }
        };
        if !run.status.success() {
            return Done::plain(
                Outcome::CouldNotTell,
                format!(
                    "the dedupe query exited {}: {}",
                    run.status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "on a signal".to_string()),
                    String::from_utf8_lossy(&run.stderr).trim()
                ),
            );
        }
        match open_ids(&String::from_utf8_lossy(&run.stdout)) {
            None => {
                return Done::plain(
                    Outcome::CouldNotTell,
                    "the dedupe query answered nothing this run can read as a list".to_string(),
                )
            }
            Some(existing) if !existing.is_empty() => {
                return Done {
                    outcome: Outcome::Deduped,
                    detail: format!(
                        "an open item already carries {label}: {}",
                        existing.join(", ")
                    ),
                    extra: vec![("existing".to_string(), existing.into())],
                }
            }
            Some(_) => {}
        }
    }

    let argv = item_argv(&binary, routine, item);
    let mut cmd = child_command(&argv[0], machine);
    cmd.args(&argv[1..]);
    let run = match platform::run_bounded(cmd, bound) {
        Ok(run) => run,
        Err(why) => return Done::plain(Outcome::Failed, format!("the create did not run: {why}")),
    };
    if !run.status.success() {
        return Done::plain(
            Outcome::Failed,
            format!(
                "the create exited {}: {}",
                run.status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "on a signal".to_string()),
                String::from_utf8_lossy(&run.stderr).trim()
            ),
        );
    }
    let Some(created) = created_id(&String::from_utf8_lossy(&run.stdout)) else {
        return Done::plain(
            Outcome::Failed,
            "the create exited 0 and named no id this run can read".to_string(),
        );
    };

    // The note is the record that this item was filed BY a routine. It is
    // best-effort on purpose: a note that did not land is reported and never
    // unfiles the item.
    let mut note = child_command(&binary, machine);
    note.args([
        "-C",
        &routine.project_root.display().to_string(),
        "note",
        &created,
        &format!("filed by routine {} at {now_stamp}", routine.name),
        "--actor",
        ACTOR,
    ]);
    let noted = platform::run_bounded(note, bound);
    let trail = match noted {
        Ok(run) if run.status.success() => String::new(),
        _ => format!(" (the note could not be appended to {created})"),
    };
    Done {
        outcome: Outcome::Filed,
        detail: format!("filed {created}{trail}"),
        extra: vec![("item".to_string(), created.into())],
    }
}

/// The ids an open-item query answered, or `None` when the answer is not a list
/// this run can read — which is a third answer and never an empty one.
fn open_ids(stdout: &str) -> Option<Vec<String>> {
    if stdout.trim().is_empty() {
        return Some(Vec::new());
    }
    let first = first_json(stdout)?;
    let rows = first.as_array()?;
    Some(
        rows.iter()
            .filter_map(|row| row.get("id")?.as_str().map(str::to_string))
            .collect(),
    )
}

/// The id a create printed, read from the create's own output.
///
/// The output may be a top-level array and may trail bytes this reader has no
/// contract for, so the FIRST JSON value is decoded and the rest ignored. A
/// guessed id would put a wrong number on the record.
fn created_id(stdout: &str) -> Option<String> {
    let first = first_json(stdout)?;
    let object = match &first {
        serde_json::Value::Array(rows) => rows.first()?,
        other => other,
    };
    object
        .get("id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn first_json(text: &str) -> Option<serde_json::Value> {
    serde_json::Deserializer::from_str(text)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
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

/// `fleet run <workflow> [--input key=value]… --by <routine>`: the routine is
/// the runner, so `run.started` carries its name as the actor the way the
/// routine's own events do, and one actor reads across the four.
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
    argv.push(routine.name.clone());
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
/// run that failed, 3 one nobody could classify — the cli PRD's table, read
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
pub fn run_check(routine: &Routine, machine: &Machine) -> super::gate::CheckOutcome {
    use super::gate::CheckOutcome;
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
