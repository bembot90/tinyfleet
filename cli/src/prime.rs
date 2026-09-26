//! `fleet prime` — what a session is handed at its start.
//!
//! THE EXIT IS ALWAYS DONE AND NOTHING IS READ FROM STDIN. A session-start hook
//! that fails closed takes the session down with it, so every branch here ends
//! on a line rather than on a status: a layering this fleet cannot resolve, a
//! rules file that is not there and a store that will not answer each say so
//! in their own words and the next part still prints.
//!
//! Line 2 is the project's store as it names itself: its answer to the
//! contract's `version`, through the opener every verb takes, and the adapter
//! that gave it. It COMPARES WITH NO PIN. Which release a store was measured
//! on is its doctor check's to say, and the doctor is what `fleet run` and a
//! person consult for it.

use fleet_controller::{config, platform};
use fleet_core::add;
use fleet_core::defaults;
use fleet_core::guard;
use fleet_core::lock;
use fleet_core::resolve::{self, Layer};
use fleet_core::seat::identity::SeatId;
use fleet_core::store::types::Version;
use fleet_core::store::{self, AdapterSource, Filter, Opening, PackDirs, Status};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::exit::Exit;

/// The bound on the item listing. Well under a hook's own deadline: a store
/// that will not answer costs the line and never the session start.
const ITEMS_TIMEOUT: Duration = Duration::from_secs(5);

/// The bound on line 2's `version` call, which reads no items: a store that
/// will not answer it within this costs the line and never the session start.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// The rules file, as a path under a pack's `assets` slot.
const RULES: &str = "assets/rules.md";

pub fn command() -> Exit {
    let version = env!("CARGO_PKG_VERSION");
    let cwd = std::env::current_dir().unwrap_or_default();
    let machine_dir = platform::machine_dir();
    let machine = config::read(&machine_dir.join("config.json")).ok();

    // The order `fleet guard` resolves its policy in, so the two verbs answer
    // about one file: an embedded fleet's own, and — for a declared project,
    // which wins at its own level — the FLEET's, named by the machine
    // directory.
    let found = crate::walk_up_config(&cwd);
    let fleet_toml = match &found {
        Some(crate::Found::Embedded(path)) => Some(path.clone()),
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
    // The items are held under the seat's full id, which is what every
    // dispatch assigns: a name moves, and the record is keyed by the seat.
    let seat = machine
        .as_ref()
        .and_then(|machine| seat_here(machine, &cwd));
    let project = project_root(
        seat.as_ref().map(|(_, root)| root.as_path()),
        found.as_ref(),
    );
    println!("{}", store_line(project.as_deref()));

    if let Some(rules) = rules_of(&layering) {
        print!("{rules}");
        if !rules.ends_with('\n') {
            println!();
        }
    }

    if let Some((seat, project_root)) = seat {
        print_items(&project_root, &seat.id);
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

/// The project line 2 reads the store of: the seat's worktree where a row
/// names this directory, which is the root the item line reads, so the two
/// lines name one store; else the directory the walk found the project's own
/// file in.
fn project_root(seat_root: Option<&Path>, found: Option<&crate::Found>) -> Option<PathBuf> {
    if let Some(root) = seat_root {
        return Some(root.to_path_buf());
    }
    match found? {
        crate::Found::Embedded(file) => file.parent(),
        crate::Found::Declared(file) => file.parent().and_then(Path::parent),
    }
    .map(Path::to_path_buf)
}

/// Line 2: the store's name and version and the adapter that answered them, or
/// why they could not be read, or that there is no project here to have one.
fn store_line(project_root: Option<&Path>) -> String {
    let Some(root) = project_root else {
        return String::from("store: none (no project here)");
    };
    match store_version(root) {
        Ok((version, adapter)) => format!(
            "store: {} {} (adapter {adapter})",
            version.name, version.version
        ),
        Err(why) => format!("store: could not be read — {why}"),
    }
}

/// The store the project's own file names, asked its version through the
/// opener every verb takes, and the adapter's name: the line names the store a
/// verb run here writes to, and an adapter that will not open or answer is the
/// line's answer.
fn store_version(root: &Path) -> Result<(Version, String), String> {
    let policy = store::project_policy(root).map_err(|why| why.to_string())?;
    let machine_dir = platform::machine_dir();
    let store = store::open(&Opening {
        root,
        policy: &policy,
        source: AdapterSource::Setting,
        search_path: &platform::child_path(&platform::home_dir()),
        timeout: VERSION_TIMEOUT,
        packs: Some(PackDirs {
            packs_dir: &machine_dir.join("packs"),
            defaults_dir: &machine_dir.join(defaults::DIR),
        }),
    })
    .map_err(|why| why.to_string())?;
    let version = store.version().map_err(|why| why.to_string())?;
    Ok((version, store::adapter_name(&policy)))
}

/// The seat whose row names this directory as a worktree, and that worktree.
///
/// The match is on the directory itself rather than on any parent of it: a row
/// names a worktree root, and a session under some other checkout inside it is
/// not that seat's (lessons claude-code B5 — a cwd names a seat and proves
/// nothing, so the weakest reading is the one taken).
fn seat_here<'a>(
    machine: &'a config::MachineConfig,
    cwd: &Path,
) -> Option<(&'a config::Seat, PathBuf)> {
    let here = canonical(cwd);
    for seat in &machine.seats {
        for (_project, path) in &seat.worktrees {
            let root = PathBuf::from(path);
            if canonical(&root) == here {
                return Some((seat, root));
            }
        }
    }
    None
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn print_items(project_root: &Path, seat: &SeatId) {
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
/// The store the project's own file names, through the opener every verb
/// takes, under `ITEMS_TIMEOUT` rather than the store's bound: a session-start
/// hook cannot wait a minute. The adapter runs on the constructed child PATH,
/// because a session the controller started carries a `PATH` that finds
/// nothing (lessons claude-code D1), so a store that does not answer on it is
/// this line's third answer.
fn items(project_root: &Path, seat: &SeatId) -> Result<Vec<(String, String)>, String> {
    let policy = store::project_policy(project_root).map_err(|why| why.to_string())?;
    let machine_dir = platform::machine_dir();
    let store = store::open(&Opening {
        root: project_root,
        policy: &policy,
        source: AdapterSource::Setting,
        search_path: &platform::child_path(&platform::home_dir()),
        timeout: ITEMS_TIMEOUT,
        packs: Some(PackDirs {
            packs_dir: &machine_dir.join("packs"),
            defaults_dir: &machine_dir.join(defaults::DIR),
        }),
    })
    .map_err(|why| why.to_string())?;
    Ok(store
        .list(&Filter::Assignee(*seat))
        .map_err(|why| why.to_string())?
        .into_iter()
        .filter(|row| matches!(row.status, Status::Open | Status::InProgress))
        .map(|row| (row.id.to_string(), row.title))
        .collect())
}
