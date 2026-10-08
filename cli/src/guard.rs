//! fleet guard: one pre-tool payload judged through an agent adapter's [hook]
//! mapping, and --check's report of each class's target.

use crate::exit::Exit;
use crate::ui::{Stream, Tone, Ui};
use anyhow::Result;
use fleet_controller::project::{walk_up_config, Found};
use fleet_controller::{config, platform};
use fleet_core::defaults;
use fleet_core::guard::hook::HookMap;
use fleet_core::guard::{self, Class, Policy, Verdict};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) fn parse_class(name: &str) -> Result<Class, String> {
    Class::parse(name).ok_or_else(|| {
        let names: Vec<&str> = guard::CLASSES.iter().map(|c| c.name()).collect();
        format!("unknown class `{name}` — one of {}", names.join(", "))
    })
}

// ---- the guards -------------------------------------------------------------
//
// 2 is a caller who named no adapter, the same usage code the pack verbs use,
// a guard wired to an adapter whose mapping cannot be read, and one whose
// layers cannot say which classes they declare. The
// JUDGING path has no other code: a refusal is data on stdout and the exit is 0
// on every path, because a non-zero exit from a pre-tool hook is read as
// non-blocking and a guard that signalled by status would fail open exactly
// when it broke. --check is the one guard path with a verdict in its exit.
//
// The mis-wired guard is the exception, and the reason is the same table read
// the other way: the agent reads a hook's 2 as blocking, so a guard that cannot
// read the payload it was wired for blocks every call until it is wired right,
// rather than letting each one through unjudged.

pub(crate) fn command(
    ui: &Ui,
    class: Option<Class>,
    adapter: Option<&str>,
    check: bool,
) -> Result<Exit> {
    if check {
        let classes = match class {
            Some(class) => vec![class],
            None => declared_classes().map_err(|why| anyhow::anyhow!(why))?,
        };
        return Ok(guard_check(ui, &classes));
    }
    // A named class runs alone, exactly as it always has. With none, the set is
    // the layers', and it is ruling 6's one line. Either way `--adapter` names
    // whose payload the hook hands over: the payload is the agent's, and no
    // agent's shape is this binary's to assume.
    let Some(adapter) = adapter else {
        eprintln!(
            "fleet guard: name the --adapter whose [hook] mapping the payload is read through"
        );
        return Ok(Exit::Usage);
    };
    let map = match hook_map(adapter) {
        Ok(map) => map,
        Err(missing) => {
            eprintln!("fleet guard: {missing}");
            return Ok(Exit::Usage);
        }
    };
    let classes = match class {
        Some(class) => vec![class],
        None => match declared_classes() {
            Ok(classes) => classes,
            Err(why) => {
                eprintln!("fleet guard: {why}");
                return Ok(Exit::Usage);
            }
        },
    };
    Ok(guard_run(&classes, &map))
}

/// The classes the machine's layers declare, through the same packs and
/// defaults every other reader of a layering resolves: core's two and every
/// installed pack's `guard_classes`. A layering that does not resolve is an
/// `Err`, one line — which classes are in force is exactly what it cannot say.
fn declared_classes() -> Result<Vec<Class>, String> {
    let (packs_dir, defaults_dir) = defaults::pack_dirs(&platform::machine_dir(), None);
    let packs = fleet_core::item::brief::Packs::under(&packs_dir, &defaults_dir)
        .map_err(|stop| format!("the declared guard classes: {}", stop.message))?;
    guard::declared(&packs.layers)
}

/// The mapping a payload is read and a refusal written through: the `[hook]`
/// table of the agent adapter `--adapter` names — by an absolute path to the
/// adapter's directory, or by a bare name the machine's packs carry over its
/// defaults, as a store adapter's name is looked up.
///
/// Each `Err` is one line naming what was missing. The manifest is read
/// through the pack format's own reader, so a mapping `fleet pack check`
/// refuses is never applied.
fn hook_map(adapter: &str) -> Result<HookMap, String> {
    let dir = match adapter {
        path if path.starts_with('/') => PathBuf::from(path),
        name if !name.is_empty() && !name.contains('/') => {
            let (packs_dir, defaults_dir) = defaults::pack_dirs(&platform::machine_dir(), None);
            let packs = fleet_core::item::brief::Packs::under(&packs_dir, &defaults_dir).map_err(
                |stop| {
                    format!(
                        "the agent adapter `{name}` is looked up in the installed packs: {}",
                        stop.message
                    )
                },
            )?;
            match fleet_core::pack::adapter_dir(&packs, fleet_core::pack::AdapterKind::Agent, name)
            {
                Some((_, dir)) => dir,
                None => {
                    return Err(format!(
                        "no installed pack carries the agent adapter `{name}` — `{}/{}/{name}/{}` \
                         resolves nowhere",
                        fleet_core::pack::ADAPTERS,
                        fleet_core::pack::AdapterKind::Agent.as_str(),
                        fleet_core::pack::ADAPTER_MANIFEST
                    ))
                }
            }
        }
        other => {
            return Err(format!(
                "--adapter `{other}` is neither an agent adapter's name nor an absolute path to \
                 its directory"
            ))
        }
    };
    let manifest = fleet_core::pack::adapter_manifest(&dir).map_err(|defect| defect.to_string())?;
    let Some(table) = manifest.hook else {
        return Err(format!(
            "the agent adapter at {} declares no [{}] — a guard has no mapping to read its \
             payload through",
            dir.display(),
            fleet_core::pack::HOOK_TABLE
        ));
    };
    HookMap::parse(&table)
}

/// The payload read once and judged by each class in turn, in the order given:
/// THE FIRST REFUSAL IS THE ONE PRINTED, because a hook's stdout is one
/// document and a second would make it unreadable.
fn guard_run(classes: &[Class], map: &HookMap) -> Exit {
    let mut body = String::new();
    if std::io::stdin().read_to_string(&mut body).is_err() {
        return Exit::Done;
    }
    let Some(payload) = map.payload(&body) else {
        return Exit::Done;
    };
    let cwd = payload.cwd.as_deref().map(Path::new);
    for &class in classes {
        let mut policy = resolve_policy(class, cwd);
        caller_readings(class, &payload.command, cwd, &mut policy);
        // A defect in the reader allows, which is the contract the shell-trap
        // class states and the reason it has no raw-text fallback.
        let verdict = std::panic::catch_unwind(|| guard::judge(class, &payload.command, &policy));
        if let Ok(Verdict::Refused(denial)) = verdict {
            println!("{}", map.deny(&denial.reason()));
            break;
        }
    }
    Exit::Done
}

/// One line per check of each class, and 1 where any class has a target
/// that is not configured.
fn guard_check(ui: &Ui, classes: &[Class]) -> Exit {
    let mut all = true;
    for &class in classes {
        all &= class_check(ui, class);
    }
    if all {
        Exit::Done
    } else {
        Exit::Refused
    }
}

/// One class's lines, and whether every target it needs is configured.
fn class_check(ui: &Ui, class: Class) -> bool {
    let policy = resolve_policy(class, None);
    let mut all = true;
    for check in class.checks() {
        let name = class.name();
        match class.target_of(check) {
            None => ui.status(
                Stream::Out,
                Tone::Good,
                name,
                &format!("{check}: configured"),
                None,
            ),
            Some(key) => {
                if guard::configured(class, check, &policy) {
                    ui.status(
                        Stream::Out,
                        Tone::Good,
                        name,
                        &format!("{check}: configured"),
                        Some(key),
                    );
                } else {
                    ui.status(
                        Stream::Out,
                        Tone::Bad,
                        name,
                        &format!("{check}: not configured"),
                        Some(key),
                    );
                    all = false;
                }
            }
        }
    }
    all
}

// A DECLARED PROJECT FIRST, then the embedded file, then neither. The walk is
// the item verbs' (`fleet_controller::project::walk_up_config`), and the two answers differ in
// where the guards come from: an embedded fleet keeps its policy beside the
// work, so its own fleet.toml carries both the switches and the target; a
// standalone project declares only itself, so the switches are the FLEET's —
// the file the machine directory names — and only the target is the project's.
// Failing both, every check runs and the bare-id check has no target, so it
// refuses nothing.
fn resolve_policy(class: Class, cwd: Option<&Path>) -> Policy {
    let start = cwd
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let found = start.as_deref().and_then(walk_up_config);
    // An unreadable or unparsable file reads as an empty table, which leaves
    // every check on and prints nothing: opt-out, never opt-in. That holds for
    // the machine's file as much as for the project's, so an absent machine
    // config below leaves the class on rather than switching it off.
    let machine_policy = || {
        config::read(&config::path_in(&platform::machine_dir()))
            .map(|machine| guard::enabled_in(class, &read_text(&machine.fleet_toml)))
            .unwrap_or(true)
    };

    match found {
        Some(Found::Embedded(path)) => {
            let text = read_text(&path);
            guard::policy_in(class, &text)
        }
        Some(Found::Declared(path)) => {
            let text = read_text(&path);
            Policy {
                enabled: machine_policy(),
                ..guard::policy_in(class, &text)
            }
        }
        None => Policy {
            enabled: machine_policy(),
            ..Policy::default()
        },
    }
}

/// The two readings core cannot make, because it opens no path and runs no
/// process: the application the deployment file beside the command names, and
/// the project this machine is currently configured for. Both are targets the
/// production-write class NARROWS with — a command that names neither an
/// application nor a project reaches one anyway — so both are resolved here and
/// handed in, and an unreadable answer leaves the check without that target
/// rather than refusing on it.
///
/// The configured project costs a subprocess, so it is read only when the text
/// could possibly need it. That filter is deliberately crude and one-directional:
/// it can only leave the reading UNTAKEN, which is the silent direction the
/// class already fails in.
///
/// The shell-trap and record classes read a third: the command the project's
/// store declares, which an adapter answers as a process. It is asked only for
/// a text [`guard::reads_the_cli`] says a declaration could change the verdict
/// on, and a store that does not answer leaves the default adapter's command
/// policed rather than none.
fn caller_readings(class: Class, command: &str, cwd: Option<&Path>, policy: &mut Policy) {
    if matches!(class, Class::ShellTrap | Class::Record) {
        if guard::reads_the_cli(command) {
            if let Some(declared) = store_cli(cwd) {
                policy.cli = declared;
            }
        }
        return;
    }
    if class != Class::ProductionWrite {
        return;
    }
    if let Some(dir) = cwd {
        policy.cwd_app = guard::app_in(&read_text(&dir.join(FLY_CONFIG)));
    }
    if !policy.targets.prod_projects.is_empty() && command.contains(CLOUD_TOOL) {
        policy.active_project = configured_project();
    }
}

/// The deployment file an application's own directory carries, and the command
/// word whose project this machine holds a default for. Both are one provider's
/// spelling and neither is core's, which is why they sit in the caller.
const FLY_CONFIG: &str = "fly.toml";
const CLOUD_TOOL: &str = "gcloud";

/// How long the configured-project read is given before it is abandoned. A
/// pre-tool hook runs before EVERY shell call, so an unbounded read here would
/// hang the session rather than fail it.
const PROJECT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The project this machine is configured for, or `None` where it could not be
/// read at all — an unset value, a non-zero exit, a missing binary and a read
/// that ran past its bound are one answer, because the class treats every one of
/// them the same way.
fn configured_project() -> Option<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new(CLOUD_TOOL)
            .args(["config", "get-value", "project"])
            .output();
        let _ = sender.send(out);
    });
    let out = receiver.recv_timeout(PROJECT_READ_TIMEOUT).ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if value.is_empty() || value == "(unset)" {
        None
    } else {
        Some(value)
    }
}

/// How long the store is given to declare its command. An adapter's answer is
/// a process, and one that will not answer costs the reading and never the
/// shell call behind the hook.
const STORE_CLI_TIMEOUT: Duration = Duration::from_secs(2);

/// The command the project's store declares, through the opener every verb
/// takes: `Some(None)` for a store declaring none, and `None` where it could
/// not be read — no project above the caller, a store that will not open or
/// will not answer — which leaves the policy's default in place.
///
/// The store read is the one a verb run here opens, on the same constructed
/// child PATH.
fn store_cli(cwd: Option<&Path>) -> Option<Option<String>> {
    let start = cwd
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let root = walk_up_config(&start)?.root()?.to_path_buf();
    let policy = fleet_core::store::project_policy(&root).ok()?;
    let (packs_dir, defaults_dir) = defaults::pack_dirs(&platform::machine_dir(), None);
    let store = fleet_controller::project::open_store_at(
        &root,
        &policy,
        fleet_core::store::PackDirs {
            packs_dir: &packs_dir,
            defaults_dir: &defaults_dir,
        },
        STORE_CLI_TIMEOUT,
    )
    .ok()?;
    Some(store.capabilities().ok()?.cli)
}

pub(crate) fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}
