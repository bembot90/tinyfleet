//! The three roots a routine can come from: the fleet install, every installed
//! pack, and every project.
//!
//! A pure function over a list of roots, re-run on every tick that evaluates —
//! a directory listing and a parse each — so a file dropped into a routines
//! directory is live on the next evaluation with no restart.
//!
//! The project list arrives as an argument rather than being read here. In this
//! slice it is a list of one, the embedded project, and a registry is one more
//! element and not a second reader.

use super::file::{self, Defect, Loaded, Routine, Source};
use std::path::{Path, PathBuf};

/// One place routines are read from.
#[derive(Clone, Debug)]
pub struct Root {
    pub source: Source,
    /// The directory holding `<name>.toml`.
    pub dir: PathBuf,
    /// Where this root's routines run: the directory a check and an exec are
    /// issued in, and the store an item is filed against.
    pub project_root: PathBuf,
}

/// The directory a routine file sits in, under a root that carries one.
pub const ORDERS_DIR: &str = "orders";

/// Where every installed pack lives under the machine directory.
pub const PACKS_DIR: &str = "packs";

#[derive(Debug, Default)]
pub struct Registry {
    pub routines: Vec<Routine>,
    pub defects: Vec<Defect>,
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<&Routine> {
        self.routines.iter().find(|routine| routine.name == name)
    }

    pub fn defect(&self, name: &str) -> Option<&Defect> {
        self.defects.iter().find(|defect| defect.name == name)
    }
}

/// The roots this machine reads, in the order they are listed.
///
/// `fleet_root` is the directory holding `fleet.toml`; `projects` is one entry
/// per project root, named as the source will print it.
pub fn roots(fleet_root: &Path, machine_dir: &Path, projects: &[(String, PathBuf)]) -> Vec<Root> {
    let mut roots = vec![Root {
        source: Source::Fleet,
        dir: fleet_root.join(ORDERS_DIR),
        project_root: fleet_root.to_path_buf(),
    }];
    let packs = machine_dir.join(PACKS_DIR);
    if let Ok(entries) = std::fs::read_dir(&packs) {
        let mut installed: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        installed.sort();
        for pack in installed {
            let name = pack
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            roots.push(Root {
                source: Source::Pack(name),
                dir: pack.join(ORDERS_DIR),
                // A pack's routines run where the fleet does: a pack is a library
                // of duties and carries no checkout of its own.
                project_root: fleet_root.to_path_buf(),
            });
        }
    }
    for (name, root) in projects {
        roots.push(Root {
            source: Source::Project(name.clone()),
            dir: root.join(ORDERS_DIR),
            project_root: root.clone(),
        });
    }
    // ONE DIRECTORY IS LISTED ONCE. An embedded fleet's root and its one project
    // are the same directory, so a second root over it would make every routine in
    // it a duplicate of itself and refuse the lot.
    let mut listed: Vec<PathBuf> = Vec::new();
    roots.retain(|root| {
        let seen = listed.contains(&root.dir);
        if !seen {
            listed.push(root.dir.clone());
        }
        !seen
    });
    roots
}

/// Every routine under every root, with one name in one place.
///
/// TWO FILES WITH ONE NAME REFUSE BOTH, and the defect names both paths.
/// Shadowing a routine by layering a pack over another is the resolver's
/// question and not this reader's, and picking one of two silently is how a
/// duty runs from a file nobody is looking at.
pub fn load(roots: &[Root], seats: &[String]) -> Registry {
    let mut registry = Registry::default();
    let mut seen: Vec<(String, PathBuf)> = Vec::new();
    let mut clashed: Vec<String> = Vec::new();

    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root.dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
            .collect();
        files.sort();
        for path in files {
            match file::read(&path, &root.source, &root.project_root, seats) {
                Loaded::Routine(routine) => {
                    if let Some((_, first)) = seen.iter().find(|(name, _)| name == &routine.name) {
                        registry.defects.push(Defect {
                            name: routine.name.clone(),
                            path: path.clone(),
                            source: root.source.clone(),
                            reasons: vec![format!(
                                "two routines are named `{}`: {} and {}",
                                routine.name,
                                first.display(),
                                path.display()
                            )],
                        });
                        clashed.push(routine.name.clone());
                        continue;
                    }
                    seen.push((routine.name.clone(), path.clone()));
                    registry.routines.push(*routine);
                }
                Loaded::Defective(defect) => {
                    seen.push((defect.name.clone(), path.clone()));
                    registry.defects.push(defect);
                }
            }
        }
    }

    // The first file of a clashing pair is refused too: a duplicate that let the
    // earlier root win would be layering, decided by listing order.
    for name in clashed {
        if let Some(index) = registry
            .routines
            .iter()
            .position(|routine| routine.name == name)
        {
            let routine = registry.routines.remove(index);
            let second = registry
                .defects
                .iter()
                .find(|defect| defect.name == name)
                .map(|defect| defect.path.display().to_string())
                .unwrap_or_default();
            registry.defects.push(Defect {
                name: routine.name.clone(),
                path: routine.path.clone(),
                source: routine.source.clone(),
                reasons: vec![format!(
                    "two routines are named `{}`: {} and {second}",
                    routine.name,
                    routine.path.display()
                )],
            });
        }
    }
    registry.routines.sort_by(|a, b| a.name.cmp(&b.name));
    registry.defects.sort_by(|a, b| a.name.cmp(&b.name));
    registry
}

/// The directory holding `fleet.toml`, resolved the way every other verb
/// resolves it: walk up from `start` for the file, else read the machine
/// directory's seat list for the path it names. `None` is a machine with no
/// fleet root at all, which is exit 3 rather than a guess.
pub fn fleet_root(start: Option<&Path>, machine_dir: &Path) -> Option<PathBuf> {
    let from = start
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    if let Some(from) = &from {
        let mut here = Some(from.as_path());
        while let Some(dir) = here {
            if dir.join("fleet.toml").is_file() {
                return Some(dir.to_path_buf());
            }
            here = dir.parent();
        }
    }
    let config = crate::config::read(&machine_dir.join("config.json")).ok()?;
    config.fleet_toml.parent().map(Path::to_path_buf)
}
