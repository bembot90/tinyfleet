//! What a refusal before the push puts back.

use std::io::Write;
use std::path::PathBuf;

use super::{Landing, Wiring};
use crate::item::{Stop, TRUNK};

// ---- putting the tree back ---------------------------------------------------

/// What a refusal before the push has to undo, gathered before the first git
/// write because that is the only moment it can be read.
pub(super) struct Tree {
    /// Each `--also` path and the bytes it held, `None` where it was not there.
    saved: Vec<(PathBuf, Option<Vec<u8>>)>,
    /// The land branch, once it exists.
    pub(super) branch: Option<String>,
    /// Whether the push has run. Nothing is put back after it.
    pub(super) pushed: bool,
}

impl Tree {
    pub(super) fn of(landing: &Landing, wiring: &Wiring) -> Result<Tree, Stop> {
        let mut saved = Vec::new();
        for path in landing.also {
            let full = wiring.project.root.join(path);
            match std::fs::read(&full) {
                Ok(bytes) => saved.push((full, Some(bytes))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => saved.push((full, None)),
                // Not usage: the path was named correctly and the INSTRUMENT
                // would not answer, and a refusal that could not read what it
                // has to put back is one that has read nothing.
                Err(e) => {
                    return Err(Stop::could_not_tell(format!(
                        "the --also path {} could not be read: {e}",
                        full.display()
                    )))
                }
            }
        }
        Ok(Tree {
            saved,
            branch: None,
            pushed: false,
        })
    }

    pub(super) fn put_back(&mut self, err: &mut dyn Write, wiring: &Wiring) {
        if self.pushed {
            return;
        }
        if let Some(branch) = self.branch.take() {
            // THE RESET COMES FIRST. A squash is staged and on disk, and by the
            // time one exists HEAD already names the trunk — so a detach alone
            // succeeds and undoes nothing. The reset is also what puts the
            // store's export back, which is why nothing here restores it by
            // hand.
            if let Err(cause) = wiring.git.reset_hard(TRUNK) {
                let _ = writeln!(err, "the squash could not be put back: {cause}");
            }
            if let Err(cause) = wiring.git.detach(TRUNK) {
                let _ = writeln!(
                    err,
                    "the checkout could not be detached onto {TRUNK}: {cause}"
                );
            }
            if let Err(cause) = wiring.git.delete_branch(&branch) {
                let _ = writeln!(
                    err,
                    "the land branch {branch} could not be removed: {cause}"
                );
            }
        }
        for (path, bytes) in &self.saved {
            let put = match bytes {
                Some(bytes) => std::fs::write(path, bytes),
                // It was not there before, so putting it back is removing it —
                // and a file already gone is the state asked for, not a failure.
                None => match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                },
            };
            if let Err(cause) = put {
                let _ = writeln!(
                    err,
                    "the --also path {} could not be put back: {cause}",
                    path.display()
                );
            }
        }
    }
}
