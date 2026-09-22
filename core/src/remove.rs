//! `fleet pack remove`: the inverse of [`crate::add`], keyed by the source as
//! typed.
//!
//! The lock is keyed by source and the directory by the manifest's name, so the
//! `name` the lock now carries is what joins the two: a source with no name on
//! its line names no directory, and this verb refuses rather than guess.
//!
//! The two writes go directory first, then the lock. The order is what a failure
//! between them leaves: a line with no directory, which a re-run of this verb
//! clears and a re-add of the same source replaces. The other order leaves a
//! directory with no line, which `add` refuses as a name collision and this verb
//! refuses as a source it does not hold — a state no verb clears (cli PRD
//! § `fleet pack remove`).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::defaults;
use crate::lock;
use crate::pack;
use crate::resolve;

/// What the verb removed, for the caller to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub name: String,
    pub root: PathBuf,
    pub entry: lock::Entry,
    /// What the caller says on stderr beside a success: a removal is still a
    /// removal when the directory was gone already or a neighbour could not be
    /// read for its imports.
    pub notices: Vec<Notice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    DirectoryAlreadyGone(PathBuf),
    /// An installed pack whose own check fails cannot be read for its imports,
    /// so the walk names it and passes over it.
    Unreadable(String),
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Notice::DirectoryAlreadyGone(root) => write!(
                f,
                "`{}` was already gone — the lock line is dropped",
                root.display()
            ),
            Notice::Unreadable(pack) => write!(
                f,
                "`{pack}` does not pass its own check and was not read for imports"
            ),
        }
    }
}

/// Any one of these leaves the packs directory and the lock as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotInLock { source: String, lock: String },
    NoName { source: String, version: String },
    Defaults(String),
    Imported { name: String, importer: String },
    Directory { root: String, error: String },
    Lock(lock::LockError),
    LockDidNotDrop { source: String, lock: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NotInLock { source, lock } => {
                write!(f, "`{source}` is not in `{lock}`")
            }
            Refusal::NoName { source, version } => write!(
                f,
                "`{source}` has no `name` in the lock, so the directory it \
                 installed cannot be found — re-add it with \
                 `fleet pack add {source} --version {version}` to pin the name, \
                 which replaces the line; the directory is untouched"
            ),
            Refusal::Defaults(source) => write!(
                f,
                "`{source}` is the binary's own defaults, the bottom layer every \
                 pack resolves over — it is not removable"
            ),
            Refusal::Imported { name, importer } => {
                write!(f, "`{importer}` imports `{name}`")
            }
            Refusal::Directory { root, error } => {
                write!(f, "`{root}` cannot be removed: {error}")
            }
            Refusal::Lock(e) => write!(f, "{e}"),
            Refusal::LockDidNotDrop { source, lock } => write!(
                f,
                "`{lock}` was written and still carries `{source}` — the drop did not land"
            ),
        }
    }
}

/// Take one pack out: its directory, then its line.
///
/// `source` is matched against the lock's keys exactly as `add` wrote them —
/// trimmed, as typed. Nothing outside `packs_dir` and `lock_path` is written.
pub fn remove(packs_dir: &Path, lock_path: &Path, source: &str) -> Result<Removed, Vec<Refusal>> {
    let source = source.trim().to_string();
    // The bottom layer is not in the packs directory and has no pack to take
    // out, so it is answered off the source alone and before the lock is read.
    if source == defaults::SOURCE {
        return Err(vec![Refusal::Defaults(source)]);
    }
    let entries = lock::read(lock_path).map_err(|e| vec![Refusal::Lock(e)])?;
    let Some(entry) = entries.iter().find(|e| e.source == source).cloned() else {
        return Err(vec![Refusal::NotInLock {
            source,
            lock: lock_path.display().to_string(),
        }]);
    };
    let Some(name) = entry.name.clone() else {
        return Err(vec![Refusal::NoName {
            source,
            version: entry.version.clone(),
        }]);
    };

    let root = packs_dir.join(&name);
    let mut notices = Vec::new();
    let importers = importers(packs_dir, &root, &name, &source, &mut notices);
    if !importers.is_empty() {
        return Err(importers
            .into_iter()
            .map(|importer| Refusal::Imported {
                name: name.clone(),
                importer,
            })
            .collect());
    }

    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|e| {
            vec![Refusal::Directory {
                root: root.display().to_string(),
                error: e.to_string(),
            }]
        })?;
    } else {
        notices.push(Notice::DirectoryAlreadyGone(root.clone()));
    }

    drop_line(lock_path, &source).map_err(|r| vec![r])?;

    Ok(Removed {
        name,
        root,
        entry,
        notices,
    })
}

/// Drop the line and read the drop back. The read is what the verb exits 0 on,
/// the same pin `add` owes its own write (packs PRD R9).
fn drop_line(lock_path: &Path, source: &str) -> Result<(), Refusal> {
    lock::remove(lock_path, source).map_err(Refusal::Lock)?;
    match lock::read(lock_path) {
        Ok(entries) if entries.iter().any(|e| e.source == source) => Err(Refusal::LockDidNotDrop {
            source: source.to_string(),
            lock: lock_path.display().to_string(),
        }),
        Ok(_) => Ok(()),
        Err(e) => Err(Refusal::Lock(e)),
    }
}

/// Every installed pack that declares an import of this one, by name or by the
/// source as typed. Both are checked because a manifest may name either.
///
/// The walk is over the packs directory alone, which is where every caller of
/// the resolver on this tree assembles its layers from; an own-pack root outside
/// it would have to be added here.
fn importers(
    packs_dir: &Path,
    removing: &Path,
    name: &str,
    source: &str,
    notices: &mut Vec<Notice>,
) -> Vec<String> {
    let mut found = Vec::new();
    for layer in resolve::installed(packs_dir) {
        if layer.root == removing {
            continue;
        }
        let Some(manifest) = pack::check(&layer.root).manifest else {
            notices.push(Notice::Unreadable(layer.name));
            continue;
        };
        if manifest
            .imports
            .iter()
            .any(|i| i.name == name || i.source == source)
        {
            found.push(layer.name);
        }
    }
    found
}
