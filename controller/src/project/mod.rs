//! What only a process knows about the project it acts in: the project the
//! working directory (or a named root) resolves to, its machine directory,
//! packs and seats, and the store it is opened through.
//!
//! Every verb and the run pass resolve through these, so the cli and the
//! loop answer about one project.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fleet_core::item::{project_name, table_at, Project, Stop};
use fleet_core::seat::identity::Directory;
use fleet_core::store::{self, AdapterSource, Opening, PackDirs, Store, StoreError, STORE_TIMEOUT};

use crate::lifecycle::{FLEET_TOML, PROJECT_TOML};
use crate::{config, platform};

pub mod stream;
pub mod wiring;

pub struct Here {
    pub project: Project,
    pub machine_dir: PathBuf,
    pub packs_dir: PathBuf,
    /// The binary's own defaults, materialized: the resolver's bottom layer, a
    /// SIBLING of the packs directory so a caller naming its own packs dir names
    /// the pair.
    pub defaults_dir: PathBuf,
    /// Every seat this fleet lists and this machine runs, as every reader
    /// names them: a `--to` resolves among the running ones, and an actor
    /// among the listed ones.
    pub seats: Directory,
    /// The policy file in force, as one path: the embedded fleet's own
    /// `fleet.toml`, or the file a standalone fleet's machine config names.
    /// `run` copies it byte for byte, so which file it is has to be resolved
    /// once rather than derived again beside every reader.
    pub policy_file: PathBuf,
}

impl Here {
    /// The checkout `git worktree` is run from: `[project] primary` where the
    /// project's own file names one, else the root this resolved to.
    ///
    /// A relative path is read against that root, because the file it is
    /// written in sits there and a path relative to the caller's cwd would name
    /// a different directory per call.
    pub fn primary(&self) -> Result<PathBuf, Stop> {
        let named = fleet_core::policy::read("project", "primary", &self.project.policy);
        Ok(self
            .under_root("primary", named)?
            .unwrap_or_else(|| self.project.root.clone()))
    }

    /// Where a transient seat's worktree is made: `[project] worktrees` where
    /// the file names one, else a sibling of the project root named after it
    /// with `-worktrees` appended.
    pub fn worktrees_dir(&self) -> Result<PathBuf, Stop> {
        let named = fleet_core::policy::read("project", "worktrees", &self.project.policy);
        Ok(self
            .under_root("worktrees", named)?
            .unwrap_or_else(|| derived_worktrees_dir(&self.project.root)))
    }

    /// One census answer as a path, with each of the reader's three answers kept
    /// apart.
    ///
    /// An `Err` is the CENSUS refusing the pair, which is a defect in this
    /// call site and never a value — so it is could-not-tell rather than the
    /// derived fallback. A value that is present and is not a usable string is
    /// a refusal naming the key: a `[project]` that declares a worktrees
    /// directory and has it silently ignored cuts a seat's checkout somewhere
    /// other than where the project says it goes. Only ABSENT falls back.
    ///
    /// A blank string is no value, and a relative one is read against the
    /// project root: the file it is written in sits there, so reading it against
    /// the caller's cwd would name a different directory per call.
    fn under_root(
        &self,
        key: &str,
        value: Result<Option<&fleet_core::policy::Value>, fleet_core::policy::Unlisted>,
    ) -> Result<Option<PathBuf>, Stop> {
        let value = value.map_err(|unlisted| Stop::could_not_tell(unlisted.to_string()))?;
        let Some(value) = value else {
            return Ok(None);
        };
        let Some(named) = value.as_str() else {
            return Err(Stop::refused(format!(
                "`[project] {key}` in {} is {}, and a path has to be a string — this verb will \
                 not fall back to a directory the project did not name",
                self.project.root.display(),
                value.type_str()
            )));
        };
        let named = named.trim();
        if named.is_empty() {
            return Ok(None);
        }
        let path = PathBuf::from(named);
        Ok(Some(if path.is_absolute() {
            path
        } else {
            self.project.root.join(path)
        }))
    }
}

/// Where a seat's worktree goes when no `[project] worktrees` names one: a
/// sibling of the project root named after it with `-worktrees` appended.
///
/// `create` writes this value into a standalone project's own file, where there
/// is no root beside the fleet to derive it from, so the derivation is one
/// function rather than two that agree today.
pub fn derived_worktrees_dir(root: &Path) -> PathBuf {
    let mut name = root.file_name().unwrap_or_default().to_os_string();
    name.push("-worktrees");
    root.with_file_name(name)
}

/// The project's store, as `[store] adapter` in its own file names it: the one
/// way a verb opens it.
///
/// A pack's adapter runs on the constructed child PATH, as `fleet prime` and
/// the controller's run pass run it, so the store a session is told about, the
/// one a verb writes through and the one the pass reads resolve the same files
/// (lessons claude-code D1). A store that cannot be opened at all is could not
/// tell.
pub fn open_store(here: &Here) -> Result<Box<dyn Store>, Stop> {
    Ok(open_store_at(
        &here.project.root,
        &here.project.policy,
        PackDirs {
            packs_dir: &here.packs_dir,
            defaults_dir: &here.defaults_dir,
        },
        STORE_TIMEOUT,
    )?)
}

/// A project's store, opened as every verb opens one: `[store] adapter` out
/// of the project's own file (`policy`), its adapter resolved through the
/// machine's packs and run on the constructed child PATH — never this
/// process's own (lessons claude-code D1) — each call bounded by `timeout`.
pub fn open_store_at(
    root: &Path,
    policy: &toml::Table,
    packs: PackDirs<'_>,
    timeout: Duration,
) -> Result<Box<dyn Store>, StoreError> {
    store::open(&Opening {
        root,
        policy,
        source: AdapterSource::Setting,
        search_path: &platform::child_path(&platform::home_dir()),
        timeout,
        packs: Some(packs),
    })
}

/// What the nearest directory above the caller that says anything says it is.
pub enum Found {
    /// A project declaring itself to the fleet this machine runs.
    Declared(PathBuf),
    /// A fleet keeping its own policy beside the work.
    Embedded(PathBuf),
}

impl Found {
    /// The directory the walk stopped at: an embedded fleet's own
    /// directory, or the directory that declares the project.
    pub fn root(&self) -> Option<&Path> {
        match self {
            Found::Embedded(file) => file.parent(),
            Found::Declared(file) => file.parent().and_then(Path::parent),
        }
    }
}

/// The one resolution order, level by level: A DECLARED PROJECT WINS AT ITS OWN
/// LEVEL. A directory carrying `.fleet/project.toml` is a standalone project
/// even where a `fleet.toml` sits beside it, because the declaration is that
/// directory's own statement about itself and the neighbour may be some other
/// tool's file.
pub fn walk_up_config(start: &Path) -> Option<Found> {
    let mut here = Some(start);
    while let Some(dir) = here {
        let declared = dir.join(PROJECT_TOML);
        if declared.is_file() {
            return Some(Found::Declared(declared));
        }
        let embedded = dir.join(FLEET_TOML);
        if embedded.is_file() {
            return Some(Found::Embedded(embedded));
        }
        here = dir.parent();
    }
    None
}

/// A DECLARED PROJECT FIRST at each level, then the embedded file: a directory
/// carrying its own `.fleet/project.toml` is a standalone project even where a
/// `fleet.toml` sits beside it, because the declaration is that directory's own
/// statement about itself and the neighbour may be some other tool's file.
/// Failing a declaration, a `fleet.toml` in the nearest directory that has one
/// is an embedded fleet, which keeps its policy beside the work. The walk is
/// the same one the guards take.
pub fn resolve_at(chosen_packs_dir: Option<PathBuf>) -> Result<Here, Stop> {
    let cwd = std::env::current_dir()
        .map_err(|e| Stop::could_not_tell(format!("the current directory cannot be read: {e}")))?;
    resolve_from(&cwd, platform::machine_dir(), chosen_packs_dir)
}

/// The same walk, from a directory and a machine directory the CALLER names
/// rather than from its own.
///
/// The tick takes this one: a controller started as a service has no working
/// directory to resolve a project from, and the directory it names is one the
/// machine registers.
///
/// THE MACHINE DIRECTORY IS AN ARGUMENT AND NOT AN ENVIRONMENT READ. Everything
/// this walk derives from it — the machine config, the guards, the policy file,
/// the seats and the packs directory — is the CALLER's machine directory, so a
/// caller that already holds one (the [`crate::runs::Engine`] does) resolves
/// under that one and not under whatever `FLEET_DIR` this process happens to
/// carry. `resolve_at` above is the single site that asks the environment.
pub fn resolve_from(
    cwd: &Path,
    machine_dir: PathBuf,
    chosen_packs_dir: Option<PathBuf>,
) -> Result<Here, Stop> {
    let cwd = cwd.to_path_buf();
    let machine = config::read(&config::path_in(&machine_dir));

    if let Some(found) = walk_up_config(&cwd) {
        if let Some(dir) = found.root() {
            return Ok(match &found {
                Found::Declared(_) => declared_at(dir, &machine, &machine_dir, &chosen_packs_dir),
                Found::Embedded(file) => {
                    embedded_at(dir, file, &machine, &machine_dir, &chosen_packs_dir)
                }
            });
        }
    }

    // THE WALK IS NOT THE ONLY ANSWER, and a directory it fails on is not a
    // fleetless one. A seat's worktree cut beside a project whose own
    // `fleet.toml` is not committed carries neither file above it, and the
    // machine directory still names the fleet — the same fallback `fleet prime`
    // takes (`fleet-cli's prime::command`) and the guards take
    // (`fleet-cli's resolve_policy`), so every reader answers about one fleet.
    //
    // THE ROOT IS THE CALLER'S OWN CHECKOUT, and the fallback is owed only to a
    // caller that has one. This root is where every verb's git runs and where
    // its store is read (`RealGit { root }`, the store's opener), so a root taken from the
    // fleet's own directory would point a seat's `deliver` at the primary's
    // working tree rather than at the branch the seat built on. A committed
    // policy file resolves a seat's worktree to ITSELF, and this reproduces that
    // one answer rather than inventing a second (the transient-seat resolution spec). Outside
    // every checkout the refusal stands, because there is no tree to act in and
    // a guessed project puts a seat in the wrong one — the same conjunction
    // `fleet start` refuses on (`fleet-cli's lifecycle::Fleet::resolve`).
    if let (Ok(named), Some(root)) = (&machine, checkout_above(&cwd)) {
        if named.fleet_toml.is_file() {
            return Ok(embedded_at(
                &root,
                &named.fleet_toml,
                &machine,
                &machine_dir,
                &chosen_packs_dir,
            ));
        }
    }

    Err(Stop::could_not_tell(format!(
        "no `{FLEET_TOML}` and no `{PROJECT_TOML}` above {} — `fleet create` writes one",
        cwd.display()
    )))
}

/// A project that declares itself. THE GUARDS ARE THE FLEET'S DECLARATION AND
/// NOT THE PROJECT'S, so a standalone fleet reads them from the file its
/// machine directory names rather than from the project beside the work.
fn declared_at(
    dir: &Path,
    machine: &Result<config::MachineConfig, String>,
    machine_dir: &Path,
    chosen_packs_dir: &Option<PathBuf>,
) -> Here {
    let policy = table_at(&dir.join(PROJECT_TOML));
    let guards = machine
        .as_ref()
        .map(|machine| table_at(&machine.fleet_toml))
        .unwrap_or_default();
    let policy_file = machine
        .as_ref()
        .ok()
        .map(|machine| machine.fleet_toml.clone())
        .unwrap_or_else(|| machine_dir.join(FLEET_TOML));
    let (packs_dir, defaults_dir) =
        fleet_core::defaults::pack_dirs(machine_dir, chosen_packs_dir.as_deref());
    let project = Project {
        root: dir.to_path_buf(),
        name: project_name(&policy).unwrap_or_else(|| basename(dir)),
        policy,
        guards,
    };
    Here {
        seats: seats_of(machine, &project, machine_dir),
        project,
        packs_dir,
        defaults_dir,
        machine_dir: machine_dir.to_path_buf(),
        policy_file,
    }
}

/// An embedded fleet, keeping its policy beside the work: one file carries both
/// the project's policy and the guards. `policy_file` is passed rather than
/// derived from `dir`, because the fallback above reaches this root through a
/// machine config that may name the file by some other spelling.
fn embedded_at(
    dir: &Path,
    policy_file: &Path,
    machine: &Result<config::MachineConfig, String>,
    machine_dir: &Path,
    chosen_packs_dir: &Option<PathBuf>,
) -> Here {
    let policy = table_at(policy_file);
    let (packs_dir, defaults_dir) =
        fleet_core::defaults::pack_dirs(machine_dir, chosen_packs_dir.as_deref());
    let project = Project {
        root: dir.to_path_buf(),
        name: basename(dir),
        guards: policy.clone(),
        policy,
    };
    Here {
        seats: seats_of(machine, &project, machine_dir),
        project,
        packs_dir,
        defaults_dir,
        machine_dir: machine_dir.to_path_buf(),
        policy_file: policy_file.to_path_buf(),
    }
}

/// The checkout `start` sits in: the nearest directory at or above it carrying
/// a `.git`, which is a DIRECTORY in a primary and a FILE in a linked worktree.
///
/// Read off the filesystem rather than out of `git rev-parse`, because this
/// runs before any verb has decided it is going to shell out at all, and a
/// resolution that spawned a process would spawn it on every call.
fn checkout_above(start: &Path) -> Option<PathBuf> {
    let mut here = Some(start);
    while let Some(dir) = here {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        here = dir.parent();
    }
    None
}

/// The seat directory over the machine's rows and the fleet's own policy —
/// the project's guards table, which is the fleet's file in either mode. A
/// machine config that will not read runs nothing here, and the roster and
/// this machine's identity are still listed.
fn seats_of(
    machine: &Result<config::MachineConfig, String>,
    project: &Project,
    machine_dir: &Path,
) -> Directory {
    let rows = machine
        .as_ref()
        .map(|machine| machine.seats.as_slice())
        .unwrap_or_default();
    config::directory(rows, &project.guards, machine_dir)
}

fn basename(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}
