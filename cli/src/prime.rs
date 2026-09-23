//! `fleet prime` — what a session is handed at its start.
//!
//! THE EXIT IS ALWAYS DONE AND NOTHING IS READ FROM STDIN. A session-start hook
//! that fails closed takes the session down with it, so every branch here ends
//! on a line rather than on a status: a layering this fleet cannot resolve, a
//! rules file that is not there and a tracker that will not answer each say so
//! in their own words and the next part still prints.

use fleet_controller::{config, platform};
use fleet_core::add;
use fleet_core::defaults;
use fleet_core::guard;
use fleet_core::lock;
use fleet_core::resolve::{self, Layer};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::exit::Exit;

/// The bound on the item listing. Well under a hook's own deadline: a store
/// that will not answer costs the line and never the session start.
const ITEMS_TIMEOUT: Duration = Duration::from_secs(5);

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
/// NOT `fleet_core::store::Bd`, which makes this same call: that reader waits
/// on `bd` without a bound, and a session-start hook needs a deadline (see
/// `ITEMS_TIMEOUT`). It also answers `Row { id, status }`, and this line
/// carries the title.
fn items(project_root: &Path, seat: &str) -> Result<Vec<(String, String)>, String> {
    let bin = resolve_bd()?;
    let mut cmd = Command::new(bin);
    cmd.arg("-C")
        .arg(project_root)
        .args(["list", "-a", seat, "--json"]);
    let out = platform::run_bounded(cmd, ITEMS_TIMEOUT)?;
    if !out.ok {
        let cause = out
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no message")
            .trim()
            .to_string();
        return Err(format!("the listing exited {:?} — {cause}", out.code));
    }
    let rows: serde_json::Value =
        serde_json::from_str(&out.stdout).map_err(|e| format!("the listing is not JSON: {e}"))?;
    let Some(rows) = rows.as_array() else {
        return Err("the listing is not an array".to_string());
    };
    Ok(rows
        .iter()
        .filter(|row| matches!(row["status"].as_str(), Some("open" | "in_progress")))
        .filter_map(|row| {
            Some((
                row["id"].as_str()?.to_string(),
                row["title"].as_str().unwrap_or("").to_string(),
            ))
        })
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
