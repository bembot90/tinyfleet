//! `fleet prime` — what a session is handed at its start.
//!
//! THE EXIT IS ALWAYS DONE AND NOTHING IS READ FROM STDIN. A session-start hook
//! that fails closed takes the session down with it, so every branch here ends
//! on a line rather than on a status: a layering this fleet cannot resolve, a
//! rules file that is not there and a tracker that will not answer each say so
//! in their own words and the next part still prints.
//!
//! Line 2 is the tracker's version against the one the store was measured on
//! (`store::PINNED_BD`). Another version is NAMED AND NOT REFUSED: the verbs
//! still run on it, and the line says so beside the one that installs the pin.

use fleet_controller::{config, platform};
use fleet_core::add;
use fleet_core::defaults;
use fleet_core::guard;
use fleet_core::lock;
use fleet_core::process::run_bounded;
use fleet_core::resolve::{self, Layer};
use fleet_core::store::{Bd, Store, PINNED_BD};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::exit::Exit;

/// The bound on the item listing. Well under a hook's own deadline: a store
/// that will not answer costs the line and never the session start.
const ITEMS_TIMEOUT: Duration = Duration::from_secs(5);

/// The bound on `bd version`, which reads no store and answered in 60ms on the
/// box this was written on: a tracker that hangs costs line 2 and never more
/// than this of a session start.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// The item-tracker binary when nothing names one, resolved on the constructed
/// child PATH and never by a bare name (lessons claude-code D1).
const DEFAULT_BD: &str = "bd";

/// The rules file, as a path under a pack's `assets` slot.
const RULES: &str = "assets/rules.md";

pub fn command() -> Exit {
    let version = env!("CARGO_PKG_VERSION");
    let cwd = std::env::current_dir().unwrap_or_default();
    let machine_dir = platform::machine_dir();
    let machine = config::read(&machine_dir.join("config.json")).ok();

    // The order `fleet guard` resolves its policy in, so the two verbs answer
    // about one file: an embedded fleet's own, and — for a declared project,
    // which wins at its own level — the FLEET's, named by the machine directory
    // (controller PRD § Two modes).
    let fleet_toml = match crate::walk_up_config(&cwd) {
        Some(crate::Found::Embedded(path)) => Some(path),
        Some(crate::Found::Declared(_)) | None => machine.as_ref().map(|m| m.fleet_toml.clone()),
    };

    let Some(fleet_toml) = fleet_toml else {
        println!(
            "fleet {version} — no fleet config found above {}",
            cwd.display()
        );
        return Exit::Done;
    };

    let policy = crate::read_text(&fleet_toml);
    let layering = layers(&machine_dir.join("packs"), &machine_dir.join(defaults::DIR));
    println!(
        "fleet {version} — packs: {}; guards: {}",
        packs_of(&layering, &machine_dir.join(lock::LOCK)),
        guards_of(&policy),
    );
    println!("{}", bd_line(resolve_bd()));

    if let Some(rules) = rules_of(&layering) {
        print!("{rules}");
        if !rules.ends_with('\n') {
            println!();
        }
    }

    if let Some(machine) = machine {
        if let Some((seat, project_root)) = seat_here(&machine, &cwd) {
            print_items(&project_root, &seat);
        }
    }

    Exit::Done
}

/// Every class this binary carries, on or off, read through the same function
/// the hook path reads. It ITERATES rather than naming them, so a class a pack
/// adds reaches this line without an edit here.
fn guards_of(policy: &str) -> String {
    guard::CLASSES
        .iter()
        .map(|class| {
            format!(
                "{} {}",
                class.name(),
                on_off(guard::enabled_in(*class, policy))
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn on_off(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off"
    }
}

/// What this fleet's packs resolve to, or why they do not.
///
/// BOTH refusing calls are made before line 1 is printed, and either one lands
/// here: a refusal is carried rather than raised, because the names are one
/// part of that line and the guards are another, and a layering nobody can
/// resolve must not cost the reader the half that is still readable.
enum Layering {
    Resolved {
        layers: Vec<Layer>,
        resolution: resolve::Resolution,
    },
    Refused(String),
}

/// The installed packs, ordered over the binary's defaults — the same calls
/// every other reader of a layering makes, so a fleet reads one way.
///
/// A fleet with nothing installed still RESOLVES: the defaults are the bottom
/// layer whatever is beside them, so the rules file below has an answer before
/// anyone has added a pack. What is empty then is the PACKS line, which
/// [`packs_of`] reads off the layers that are not the defaults.
fn layers(packs_dir: &Path, defaults_dir: &Path) -> Layering {
    let ordered = match resolve::layers(packs_dir, defaults_dir) {
        Ok(ordered) => ordered,
        Err(refusals) => return Layering::Refused(first_refusal(&refusals)),
    };
    match resolve::resolve(&ordered) {
        Ok(resolution) => Layering::Resolved {
            layers: ordered,
            resolution,
        },
        Err(refusals) => Layering::Refused(first_refusal(&refusals)),
    }
}

/// The installed names, and after them every import one of them declares that
/// nothing installed answers, with the line that adds it (fleet-4fw): a session
/// is told here, at its start, rather than by the first run it opens.
fn packs_of(layering: &Layering, lock_path: &Path) -> String {
    match layering {
        Layering::Refused(why) => format!("could not be resolved — {why}"),
        Layering::Resolved { layers, .. } => {
            let installed: Vec<&str> = layers
                .iter()
                .filter(|layer| !layer.defaults)
                .map(|layer| layer.name.as_str())
                .collect();
            if installed.is_empty() {
                return "none installed".to_string();
            }
            let missing: Vec<String> = add::missing_imports(layers, lock_path)
                .iter()
                .map(|missing| missing.to_string())
                .collect();
            if missing.is_empty() {
                installed.join(", ")
            } else {
                format!("{} ({})", installed.join(", "), missing.join("; "))
            }
        }
    }
}

/// The rules file the resolution answers with: the highest layer that CARRIES
/// the path, which is not always the highest layer.
///
/// A layering that refused has no resolution to ask, so nothing is printed —
/// line 1 already said why.
fn rules_of(layering: &Layering) -> Option<String> {
    let Layering::Resolved { layers, resolution } = layering else {
        return None;
    };
    let path = resolve::slot_path(resolution, layers, RULES)?;
    std::fs::read_to_string(path).ok()
}

/// A refusal list is never empty where it is returned, and a reader owed one
/// sentence gets the first: the rest are on `fleet pack check`.
fn first_refusal(refusals: &[resolve::Refusal]) -> String {
    refusals
        .first()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "the layering refused without saying why".to_string())
}

/// The seat whose row names this directory as a worktree, and that worktree.
///
/// The match is on the directory itself rather than on any parent of it: a row
/// names a worktree root, and a session under some other checkout inside it is
/// not that seat's (lessons claude-code B5 — a cwd names a seat and proves
/// nothing, so the weakest reading is the one taken).
fn seat_here(machine: &config::MachineConfig, cwd: &Path) -> Option<(String, PathBuf)> {
    let here = canonical(cwd);
    for seat in &machine.seats {
        for (_project, path) in &seat.worktrees {
            let root = PathBuf::from(path);
            if canonical(&root) == here {
                return Some((seat.name.clone(), root));
            }
        }
    }
    None
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Line 2: the tracker a session's verbs reach, by the same resolution the
/// item lines take, against the pinned release — the pin, another version
/// with the line that installs the pin, or a tracker that did not answer,
/// which is this line's third answer as it is the item line's.
fn bd_line(bin: Result<PathBuf, String>) -> String {
    match bin.and_then(|bin| version_of(&bin)) {
        Ok(first) if carries_pin(&first) => format!("bd: {PINNED_BD}, the pinned version"),
        Ok(first) => format!(
            "bd: {}, not the pinned {PINNED_BD} — the verbs still run, on answers fleet was not \
             measured against; install the pin: {}",
            named_version(&first),
            install_line()
        ),
        Err(why) => format!(
            "bd: could not be read — {why}; the pinned version is {PINNED_BD}: {}",
            install_line()
        ),
    }
}

/// The first line `bd version` printed, which is where bd prints its own.
fn version_of(bin: &Path) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.arg("version");
    let out = run_bounded(cmd, VERSION_TIMEOUT)
        .map_err(|why| format!("`{} version` {why}", bin.display()))?;
    let said = String::from_utf8_lossy(&out.stdout);
    let first = said.lines().next().unwrap_or_default().trim();
    if !out.status.success() || first.is_empty() {
        return Err(format!(
            "`{} version` {} and printed no version",
            bin.display(),
            out.status
        ));
    }
    Ok(first.to_string())
}

/// The pin as a WHOLE token of that line, bare or with a leading `v` — the
/// reading the defaults' `bd-version` doctor check takes, so the two agree.
fn carries_pin(first: &str) -> bool {
    first
        .split_whitespace()
        .any(|token| token.strip_prefix('v').unwrap_or(token) == PINNED_BD)
}

/// The version a line names: its first token that opens on a digit, or the
/// whole line where none does, so a tracker that answered something else is
/// quoted rather than read as a version.
fn named_version(first: &str) -> String {
    first
        .split_whitespace()
        .map(|token| token.strip_prefix('v').unwrap_or(token))
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
        .unwrap_or_else(|| format!("`{first}`"))
}

/// The line that installs the pinned release — beads' own `go install`
/// guidance, at the pin's tag. The defaults' doctor check prints the same.
fn install_line() -> String {
    format!(
        "CGO_ENABLED=0 go install -tags gms_pure_go github.com/steveyegge/beads/cmd/bd@v{PINNED_BD}"
    )
}

fn print_items(project_root: &Path, seat: &str) {
    match items(project_root, seat) {
        Ok(items) if items.is_empty() => println!("item: none"),
        Ok(items) => {
            for (id, title) in items {
                println!("item: {id} — {title}");
            }
        }
        // A third answer, beside some items and none: the store was not read,
        // and reporting that as `none` would tell a seat it is free.
        Err(why) => println!("item: could not be read — {why}"),
    }
}

/// The seat's open and in-progress items, `(id, title)`, in the order the
/// tracker listed them.
///
/// The store's own reader, under `ITEMS_TIMEOUT` rather than the store's
/// bound: a session-start hook cannot wait a minute. The binary is resolved
/// strictly and never falls back to the bare name the verbs keep: a session
/// the controller started carries a `PATH` a bare `bd` finds nothing on
/// (lessons claude-code D1), so a tracker nothing resolves is this line's
/// third answer.
fn items(project_root: &Path, seat: &str) -> Result<Vec<(String, String)>, String> {
    let bin = resolve_bd()?;
    let store = Bd::at_bin(project_root, &bin).with_timeout(ITEMS_TIMEOUT);
    Ok(store
        .assigned_to(seat)
        .map_err(|why| why.to_string())?
        .into_iter()
        .filter(|row| matches!(row.status.as_str(), "open" | "in_progress"))
        .map(|row| (row.id, row.title))
        .collect())
}

/// `FLEET_BD_BIN` when it names an ABSOLUTE path, else the first `bd` on the
/// constructed child PATH — the shape every other binary this workspace runs
/// is resolved by.
pub(crate) fn resolve_bd() -> Result<PathBuf, String> {
    let child_path = platform::child_path(&platform::home_dir());
    match std::env::var("FLEET_BD_BIN").ok().as_deref().map(str::trim) {
        Some(bin) if !bin.is_empty() => {
            if !Path::new(bin).is_absolute() {
                return Err(format!(
                    "the item-tracker seam names `{bin}`, which is not an absolute path"
                ));
            }
            platform::resolve_on_path(&child_path, bin).ok_or_else(|| {
                format!("the item-tracker seam names `{bin}`, which is not an executable file")
            })
        }
        _ => platform::resolve_on_path(&child_path, DEFAULT_BD)
            .ok_or_else(|| format!("no `{DEFAULT_BD}` on the constructed child PATH")),
    }
}
