//! The binary's embedded defaults, materialized into a machine's own directory.
//!
//! They arrive without a fetch and without a pack: the bytes travelled inside
//! the executable ([`crate::embedded`]), so the entry this pins names the binary
//! that carried them rather than a commit somebody could check out. `add` is the
//! other half of this — a pack by source and version — and the two write the
//! same lock through the same [`lock::Entry`].
//!
//! THEY ARE MATERIALIZED AND NOT SERVED FROM MEMORY because every reader takes a
//! PATH: the doctor's scripts are executed, the hooks and permissions files are
//! copied into a worktree. The directory this writes is the resolver's bottom
//! layer, a SIBLING of the packs directory and never inside it, so nothing under
//! `packs/` is a thing that is not a pack.
//!
//! A MATERIALIZED SET IS REFRESHED, AND SOMEBODY ELSE'S IS NEVER OVERWRITTEN.
//! The two are one reading: a copy whose files still hash to the line that
//! pinned them is this binary's own, and is replaced when the embedded set has
//! moved on; a copy that does not — one a person edited, one no line accounts
//! for — is theirs, and is named rather than written over. THE VERSION ANSWERS
//! NEITHER QUESTION: the set is pinned at the binary's own version, so a rebuilt
//! binary at the same version carries a different set under the same string, and
//! a comparison of the two strings calls it already installed.

use crate::digest::Sha256;
use crate::{embedded, lock};
use std::fmt;
use std::path::{Path, PathBuf};

/// The source this set is keyed on in `packs.lock`. Not a git URL, because there
/// is nothing to fetch.
pub const SOURCE: &str = "embedded:defaults";

/// What `commit` says for a set with no repository behind it. The lock's field
/// is what a version resolved to on the day, and this one resolved to the binary
/// that shipped it.
pub const COMMIT: &str = "embedded";

/// The directory the set is written to, under the machine directory.
pub const DIR: &str = "defaults";

/// The name the resolver's bottom layer carries, which a refusal calls it by.
pub const LAYER: &str = "defaults";

/// The source a PREVIOUS binary pinned the bundled core pack under, and the
/// name it installed it as.
///
/// They are here because this module is what retires them: a machine that ran
/// that binary carries `packs/core` and a line keyed on this source. The
/// resolver holds no special place for any installed name, so such a copy
/// layers as an ordinary pack ABOVE the defaults and answers every template out
/// of its own tree, and `fleet pack remove core` cannot take it out because the
/// lock is keyed by source.
pub const RETIRED_SOURCE: &str = "bundled:core";
pub const RETIRED_NAME: &str = "core";

/// What the install did to the copy on disk. The caller says which in one line:
/// a no-op, a first install and a replacement read the same otherwise, and a
/// replacement nobody could see happen is the one worth naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    /// Nothing stood there, and the embedded set was written in.
    Fresh,
    /// A copy this binary had written stood there holding another set, and the
    /// embedded one replaced it.
    Refreshed,
    /// What stands there is already the embedded set, file for file. Nothing was
    /// written.
    Already,
}

/// What became of the bundled core pack a previous binary installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retired {
    /// The copy hashed to the line that pinned it, so it was this binary's
    /// predecessor's own: the directory and the line are both gone.
    Removed(PathBuf),
    /// A copy no hash accounts for — one a person edited, or one pinned before
    /// the tree key existed. BOTH the directory and the line are left where
    /// they stand: dropping the line alone would leave a pack nothing accounts
    /// for, still layering above the defaults, with no record naming it.
    LeftStanding { root: PathBuf, why: String },
}

/// What an install left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub root: PathBuf,
    pub entry: lock::Entry,
    pub landed: Landed,
    /// `None` where no line named the bundled core pack, which is every machine
    /// that has only ever run a binary carrying these defaults.
    pub retired: Option<Retired>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A set stands where this one would go and is not this binary's to
    /// replace: no line accounts for it, or its files no longer hash to the line
    /// that pinned them. `why` says which.
    Shadowed {
        why: String,
        root: String,
    },
    /// A tree could not be read, so whether the copy is current could not be
    /// told. Nothing is written on a reading nobody got.
    Unreadable(String),
    Write(String),
    /// What landed does not hash to the set this binary carries — a file lost on
    /// the way out.
    NotTheSet(String),
    Lock(String),
    /// The lock returned `Ok` and does not carry the line.
    NotPinned(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Shadowed { why, root } => write!(
                f,
                "{why} is at {root}; this binary will not write over it — remove it, or keep it"
            ),
            Refusal::Unreadable(why) => write!(
                f,
                "the defaults could not be read to compare them with the set this binary \
                 carries: {why}"
            ),
            Refusal::Write(why) => write!(f, "the defaults could not be written: {why}"),
            Refusal::NotTheSet(root) => write!(
                f,
                "what landed at {root} is not the set this binary carries"
            ),
            Refusal::Lock(why) => write!(f, "{why}"),
            Refusal::NotPinned(path) => write!(
                f,
                "{path} was written and does not carry the defaults' line"
            ),
        }
    }
}

/// The embedded defaults written to `root` and pinned, refreshing a copy of an
/// older set.
///
/// `version` is the binary's own, which is what the lock records: the set has no
/// version of its own that a person could fetch again. It says which binary
/// wrote the copy and it is never the test of whether the copy is current —
/// that is the tree hash beside it.
pub fn install(
    root: &Path,
    packs_dir: &Path,
    lock_path: &Path,
    version: &str,
    fetched: &str,
) -> Result<Installed, Refusal> {
    // The RECORD is read first, and a directory with no line behind it is
    // answered before anything is written: somebody's own defaults are refused
    // whatever this binary carries.
    let lines = lock::read(lock_path).map_err(|e| Refusal::Lock(e.to_string()))?;
    let pinned = lines.iter().find(|e| e.source == SOURCE).cloned();

    // THE PREDECESSOR'S PACK GOES FIRST, before this binary's own set is
    // written: left standing it layers ABOVE the defaults, as any installed pack
    // does, and answers every template out of its own tree.
    let retired = retire_bundled_core(
        packs_dir,
        lock_path,
        lines.iter().find(|e| e.source == RETIRED_SOURCE),
    )?;

    let standing = root.exists();
    if standing && pinned.is_none() {
        return Err(Refusal::Shadowed {
            why: "an unrecorded defaults directory".to_string(),
            root: root.display().to_string(),
        });
    }

    let carried = embedded_hash();
    let entry = lock::Entry {
        source: SOURCE.to_string(),
        name: Some(DIR.to_string()),
        version: version.to_string(),
        commit: COMMIT.to_string(),
        fetched: fetched.to_string(),
        tree: Some(carried.clone()),
    };

    if let (true, Some(pinned)) = (standing, pinned) {
        let installed = tree_hash(root).map_err(Refusal::Unreadable)?;
        // A copy whose files no longer hash to the line that pinned them is not
        // this binary's any more, whatever the line says.
        if pinned.tree.as_deref().is_some_and(|hash| hash != installed) {
            return Err(Refusal::Shadowed {
                why: "a defaults directory edited since the line that pinned it was written"
                    .to_string(),
                root: root.display().to_string(),
            });
        }
        if installed == carried {
            // Nothing to write. The line gains the hash where it was written
            // before there was one to write — so the reading above can be taken
            // on the next run — and keeps its version and `fetched`, which say
            // which binary put these bytes here and when. No bytes moved.
            let mut entry = pinned;
            if entry.tree.is_none() {
                entry.tree = Some(installed);
                pin(lock_path, &entry)?;
            }
            return Ok(Installed {
                root: root.to_path_buf(),
                entry,
                landed: Landed::Already,
                retired,
            });
        }
    }

    let aside = swap_in(root, &carried)?;
    if let Err(refusal) = pin(lock_path, &entry) {
        // The copy that was moved aside goes back: a refresh that cannot write
        // its own line leaves the machine the defaults it had, not none at all.
        restore(root, aside);
        return Err(refusal);
    }
    if let Some(aside) = aside {
        let _ = std::fs::remove_dir_all(aside);
    }

    Ok(Installed {
        root: root.to_path_buf(),
        entry,
        landed: if standing {
            Landed::Refreshed
        } else {
            Landed::Fresh
        },
        retired,
    })
}

/// The bundled core pack a previous binary installed, taken out with its line.
///
/// ONLY THIS BINARY'S PREDECESSOR'S OWN COPY IS REMOVED — one whose files still
/// hash to the line that pinned them — which is the same reading the defaults'
/// own refresh takes of the directory it would replace. A copy a person edited,
/// or one pinned before the tree key existed, is named and left where it stands
/// WITH its line, because a line dropped without its directory leaves a pack no
/// record accounts for.
///
/// The directory goes before the line, as `remove` does: what a failure between
/// them leaves is a line with no directory, which the next run clears.
fn retire_bundled_core(
    packs_dir: &Path,
    lock_path: &Path,
    line: Option<&lock::Entry>,
) -> Result<Option<Retired>, Refusal> {
    let Some(line) = line else {
        return Ok(None);
    };
    let name = line.name.as_deref().unwrap_or(RETIRED_NAME);
    let root = packs_dir.join(name);

    if root.is_dir() {
        let Some(pinned) = line.tree.as_deref() else {
            return Ok(Some(Retired::LeftStanding {
                root,
                why: "its line carries no tree hash, so it cannot be told from a copy \
                      somebody edited"
                    .to_string(),
            }));
        };
        let standing = tree_hash(&root).map_err(Refusal::Unreadable)?;
        if standing != pinned {
            return Ok(Some(Retired::LeftStanding {
                root,
                why: "it was edited since the line that pinned it was written".to_string(),
            }));
        }
        std::fs::remove_dir_all(&root)
            .map_err(|e| Refusal::Write(format!("{}: {e}", root.display())))?;
    }

    lock::remove(lock_path, RETIRED_SOURCE).map_err(|e| Refusal::Lock(e.to_string()))?;
    match lock::read(lock_path) {
        Ok(lines) if lines.iter().any(|e| e.source == RETIRED_SOURCE) => {
            Err(Refusal::NotPinned(lock_path.display().to_string()))
        }
        Ok(_) => Ok(Some(Retired::Removed(root))),
        Err(e) => Err(Refusal::Lock(e.to_string())),
    }
}

/// The embedded set into place, as a whole or not at all: a scratch directory
/// beside the target, the standing copy moved aside, the scratch renamed in, and
/// what landed hashed against what this binary carries. What comes back is the
/// copy moved aside, which is the caller's undo until its line is written.
///
/// The standing copy is MOVED and not deleted, because until the new one has
/// landed and hashed it is the only set this machine has.
fn swap_in(root: &Path, carried: &str) -> Result<Option<PathBuf>, Refusal> {
    let beside = root.parent().unwrap_or(Path::new("."));
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| DIR.to_string());
    let scratch = beside.join(format!(".{name}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    if let Err(e) = std::fs::create_dir_all(&scratch).and_then(|()| embedded::write_all(&scratch)) {
        let _ = std::fs::remove_dir_all(&scratch);
        return Err(Refusal::Write(format!("{}: {e}", scratch.display())));
    }

    let mut aside = None;
    if root.exists() {
        let moved = beside.join(format!(".{name}.replaced-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&moved);
        if let Err(e) = std::fs::rename(root, &moved) {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(Refusal::Write(format!("{}: {e}", moved.display())));
        }
        aside = Some(moved);
    }
    if let Err(e) = std::fs::rename(&scratch, root) {
        let _ = std::fs::remove_dir_all(&scratch);
        restore(root, aside);
        return Err(Refusal::Write(format!("{}: {e}", root.display())));
    }

    // WHAT LANDED is hashed, not the source: a file lost on the way out is what
    // this catches.
    match tree_hash(root) {
        Ok(landed) if landed == carried => Ok(aside),
        Ok(_) => {
            restore(root, aside);
            Err(Refusal::NotTheSet(root.display().to_string()))
        }
        Err(why) => {
            restore(root, aside);
            Err(Refusal::Unreadable(why))
        }
    }
}

/// The copy that was moved aside, put back where it stood. With none moved
/// aside — a first install — what is undone is the copy that landed.
fn restore(root: &Path, aside: Option<PathBuf>) {
    let _ = std::fs::remove_dir_all(root);
    if let Some(aside) = aside {
        let _ = std::fs::rename(&aside, root);
    }
}

/// The line written and read back: no verb exits 0 on a write it has not read
/// back.
fn pin(lock_path: &Path, entry: &lock::Entry) -> Result<(), Refusal> {
    lock::append(lock_path, entry).map_err(|e| Refusal::Lock(e.to_string()))?;
    match lock::holds(lock_path, entry) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Refusal::NotPinned(lock_path.display().to_string())),
        Err(e) => Err(Refusal::Lock(e.to_string())),
    }
}

/// Every file under a directory, hashed: each relative path and then its bytes,
/// in path order, each length-framed so a name that ends where the next begins
/// cannot hash as its neighbour.
///
/// This is the reading that says whether two copies are the same set, and it is
/// a walk of its own rather than the item pinning's, which hashes a file list
/// its caller already holds: what this needs is the list itself, read off the
/// disk, so a file added or dropped moves the answer. [`embedded_hash`] is the
/// same framing over the table this binary carries, so the two are comparable.
pub fn tree_hash(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    for relative in &files {
        let path = root.join(relative);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        frame(&mut digest, relative, &bytes);
    }
    Ok(digest.hex())
}

/// The embedded set under the same framing as [`tree_hash`], so what this binary
/// carries and what stands on disk are one comparison.
pub fn embedded_hash() -> String {
    let mut files: Vec<(&str, &[u8])> = embedded::FILES
        .iter()
        .map(|file| (file.path, file.bytes))
        .collect();
    files.sort_by(|a, b| a.0.cmp(b.0));
    let mut digest = Sha256::new();
    for (relative, bytes) in files {
        frame(&mut digest, relative, bytes);
    }
    digest.hex()
}

fn frame(digest: &mut Sha256, relative: &str, bytes: &[u8]) {
    digest.update(&(relative.len() as u64).to_be_bytes());
    digest.update(relative.as_bytes());
    digest.update(&(bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

/// The OS litter a file browser leaves in the machine directory is not walked:
/// hashed, it would read a copy somebody merely LOOKED AT as one they edited,
/// and the next install would refuse it. The build leaves the same names out
/// of the embedded set, so the two sides of the comparison agree.
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        if crate::pack::is_os_litter(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            // A name this cannot spell is an error and never a hash: two names
            // that differ only outside UTF-8 would hash the same replacement.
            let relative = relative
                .to_str()
                .ok_or_else(|| format!("{} is not a name this can read", path.display()))?;
            out.push(relative.to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    /// The version a rebuilt binary keeps: every arm that is about a set moving
    /// uses one string on both sides, because that is the case the version
    /// comparison could not see.
    const VERSION: &str = "9.9.9";

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fleet-defaults-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory is made");
        dir
    }

    /// Every file under `root`, as (relative path, bytes), in path order — so
    /// two directories are compared by what they hold and not by one file.
    fn files_under(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut names = Vec::new();
        walk(root, root, &mut names).expect("the directory reads");
        names.sort();
        names
            .into_iter()
            .map(|relative| {
                let bytes = std::fs::read(root.join(&relative)).expect("the file reads");
                (relative, bytes)
            })
            .collect()
    }

    fn line(lock_path: &Path) -> lock::Entry {
        lock::read(lock_path)
            .expect("the lock reads")
            .into_iter()
            .find(|e| e.source == SOURCE)
            .expect("the defaults' line is on the lock")
    }

    /// What the swap left beside the directory, which is nothing: a scratch copy
    /// or a directory moved aside and not cleaned up is a second set standing
    /// beside the one the resolver reads.
    fn leftovers(beside: &Path, name: &str) -> Vec<String> {
        std::fs::read_dir(beside)
            .expect("the machine directory is readable")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|entry| entry != name && entry != lock::LOCK)
            .collect()
    }

    /// The install, its pin, and the two readings that make the pin a record:
    /// what landed is the embedded set file for file, and the lock carries the
    /// line back.
    #[test]
    fn the_embedded_set_lands_whole_and_is_pinned_with_the_binarys_own_version() {
        let dir = scratch("lands");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        let done = install(&root, &packs, &lock_path, VERSION, "2026-09-12").expect("it lands");
        assert_eq!(done.landed, Landed::Fresh);
        assert_eq!(done.root, root);
        assert_eq!(done.entry.source, SOURCE);
        assert_eq!(done.entry.name.as_deref(), Some(DIR));
        assert_eq!(done.entry.version, VERSION);
        assert_eq!(done.entry.commit, COMMIT);
        assert_eq!(
            done.entry.tree,
            Some(embedded_hash()),
            "the line does not carry the hash of the set that was written"
        );
        assert!(lock::holds(&lock_path, &done.entry).expect("the lock reads"));

        // THE WHOLE SET, walked and compared file by file against the table.
        let here = files_under(&root);
        let mut there: Vec<(String, Vec<u8>)> = embedded::FILES
            .iter()
            .map(|f| (f.path.to_string(), f.bytes.to_vec()))
            .collect();
        there.sort_by(|a, b| a.0.cmp(&b.0));
        assert!(there.len() > 1, "the embedded set is more than one file");
        assert_eq!(
            there, here,
            "what landed is not the set this binary carries"
        );

        // A second install of the SAME SET leaves it standing and writes
        // nothing.
        let again =
            install(&root, &packs, &lock_path, VERSION, "2026-09-13").expect("the same set stands");
        assert_eq!(again.landed, Landed::Already);
        assert_eq!(
            again.entry.fetched, "2026-09-12",
            "the standing entry is the one on the lock, not this call's stamp"
        );
        assert_eq!(leftovers(&dir, DIR), Vec::<String>::new());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rebuilt binary carries a moved set at the same version, and the copy on
    /// the machine is replaced rather than read as already installed. The set
    /// this binary carries cannot be moved from a test, so the OTHER side is:
    /// a directory pinned at a hash of its own, one file short.
    #[test]
    fn a_set_that_moved_replaces_the_copy_and_restamps_its_line() {
        let dir = scratch("refresh");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);

        // The older set: the embedded one with its last file withheld, pinned by
        // its own hash, which is exactly what an earlier binary left behind.
        std::fs::create_dir_all(&root).unwrap();
        embedded::write_all(&root).expect("the set is written");
        let dropped = embedded::FILES
            .last()
            .expect("the embedded set holds a file")
            .path;
        std::fs::remove_file(root.join(dropped)).unwrap();
        let older = tree_hash(&root).expect("the older set hashes");
        assert_ne!(older, embedded_hash(), "the older set is a different set");
        pin(
            &lock_path,
            &lock::Entry {
                source: SOURCE.to_string(),
                name: Some(DIR.to_string()),
                version: VERSION.to_string(),
                commit: COMMIT.to_string(),
                fetched: "2026-09-12".to_string(),
                tree: Some(older.clone()),
            },
        )
        .expect("the older line is pinned");

        let again =
            install(&root, &packs, &lock_path, VERSION, "2026-09-19").expect("the moved set lands");
        assert_eq!(
            again.landed,
            Landed::Refreshed,
            "the copy that stood here was left where it was"
        );
        assert!(
            root.join(dropped).is_file(),
            "the file the older set was missing did not come back"
        );
        assert_eq!(
            tree_hash(&root).expect("what stands hashes"),
            embedded_hash()
        );

        // The line, restamped: this call's stamp and the hash of what landed.
        let pinned = line(&lock_path);
        assert_eq!(pinned, again.entry);
        assert_eq!(pinned.fetched, "2026-09-19");
        assert_eq!(pinned.version, VERSION);
        assert_eq!(pinned.tree, Some(embedded_hash()));
        assert_ne!(
            pinned.tree,
            Some(older),
            "the hash on the line did not move with the set"
        );

        // The swap cleaned up after itself: no scratch copy and nothing moved
        // aside, either of which is a second set beside the one that is read.
        assert_eq!(leftovers(&dir, DIR), Vec::<String>::new());

        // And the next call, with nothing moved, writes nothing.
        let third = install(&root, &packs, &lock_path, VERSION, "2026-09-20")
            .expect("the standing set stands");
        assert_eq!(third.landed, Landed::Already);
        assert_eq!(line(&lock_path).fetched, "2026-09-19");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A directory with no line behind it is somebody's own, and is named rather
    /// than adopted or written over.
    #[test]
    fn a_defaults_directory_with_no_line_behind_it_is_refused_as_somebody_elses() {
        let dir = scratch("unrecorded");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        std::fs::create_dir_all(&root).unwrap();
        let theirs = root.join("assets/rules.md");
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        std::fs::write(&theirs, "# somebody's own\n").unwrap();

        let refusal = install(&root, &packs, &dir.join(lock::LOCK), VERSION, "x")
            .expect_err("an unrecorded directory refuses");
        assert!(refusal.to_string().contains("an unrecorded"), "{refusal}");
        assert!(
            refusal.to_string().contains("will not write over it"),
            "{refusal}"
        );
        assert_eq!(
            std::fs::read_to_string(&theirs).unwrap(),
            "# somebody's own\n",
            "their copy was written over"
        );
        assert!(
            !dir.join(lock::LOCK).exists(),
            "a refusal wrote the lock anyway"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copy whose files no longer hash to the line that pinned them is not
    /// this binary's to refresh.
    #[test]
    fn a_copy_edited_since_it_was_pinned_is_refused_rather_than_refreshed() {
        let dir = scratch("edited");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        install(&root, &packs, &lock_path, VERSION, "2026-09-12").expect("it lands");

        let theirs = root.join("assets/rules.md");
        let edited = format!(
            "{}\na line a person put here\n",
            std::fs::read_to_string(&theirs).unwrap()
        );
        std::fs::write(&theirs, &edited).unwrap();

        let refusal = install(&root, &packs, &lock_path, VERSION, "2026-09-19")
            .expect_err("an edited copy refuses");
        assert!(
            refusal.to_string().contains("edited since"),
            "the refusal does not say what it read: {refusal}"
        );
        assert_eq!(
            std::fs::read_to_string(&theirs).unwrap(),
            edited,
            "their edit was written over"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The version is not the test and refuses nothing on its own: the same set
    /// under another version is already installed, and the line keeps the
    /// version that says which binary wrote these bytes.
    #[test]
    fn the_same_set_under_another_version_is_already_installed() {
        let dir = scratch("versions");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        install(&root, &packs, &lock_path, "1.0.0", "2026-09-12").expect("it lands");

        let again =
            install(&root, &packs, &lock_path, "2.0.0", "2026-09-19").expect("the same set stands");
        assert_eq!(again.landed, Landed::Already);
        assert_eq!(again.entry.version, "1.0.0");
        assert_eq!(line(&lock_path).version, "1.0.0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A line written before the tree key: the copy is the embedded set, so
    /// nothing is written, and the line gains the hash that will answer next
    /// time. Its version and stamp stand — no bytes moved, so what they say is
    /// still true.
    #[test]
    fn a_line_written_before_the_tree_key_gains_the_hash_and_keeps_its_stamp() {
        let dir = scratch("migrate");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        install(&root, &packs, &lock_path, "1.0.0", "2026-09-12").expect("it lands");

        // The lock as it was written before the key existed.
        let before = lock::Entry {
            tree: None,
            ..line(&lock_path)
        };
        lock::write(&lock_path, std::slice::from_ref(&before))
            .expect("the pre-key lock is written");
        assert_eq!(line(&lock_path).tree, None);

        let again = install(&root, &packs, &lock_path, VERSION, "2026-09-19")
            .expect("the standing set stands");
        assert_eq!(again.landed, Landed::Already);
        let pinned = line(&lock_path);
        assert_eq!(
            pinned.tree,
            Some(embedded_hash()),
            "the line did not gain the hash of what stands on disk"
        );
        assert_eq!(pinned.version, "1.0.0", "the version was restamped");
        assert_eq!(pinned.fetched, "2026-09-12", "the stamp was moved");
        assert_eq!(pinned, again.entry);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The hash is of the SET: a file whose bytes move, a file added and a file
    /// dropped each move it, and a second copy of one set hashes the same.
    #[test]
    fn the_tree_hash_reads_the_files_and_not_the_directory_it_read_them_from() {
        let dir = scratch("hash");
        let mine = dir.join("a-copy");
        let second = dir.join("a-second-copy");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        embedded::write_all(&mine).expect("the set writes");
        embedded::write_all(&second).expect("the set writes again");
        let first = tree_hash(&mine).expect("the copy hashes");
        assert_eq!(
            first,
            tree_hash(&second).expect("the second copy hashes"),
            "two copies of one set hash differently"
        );
        assert_eq!(
            first,
            embedded_hash(),
            "the walk and the table hash the same set differently"
        );

        let asset = mine.join("assets/rules.md");
        let body = std::fs::read_to_string(&asset).unwrap();
        std::fs::write(&asset, format!("{body}\na line a landing added\n")).unwrap();
        let moved = tree_hash(&mine).expect("the moved set hashes");
        assert_ne!(first, moved, "a file's bytes moved and the hash did not");

        std::fs::write(mine.join("assets/a-new-asset.md"), "added\n").unwrap();
        let added = tree_hash(&mine).expect("the grown set hashes");
        assert_ne!(moved, added, "a file was added and the hash did not move");

        std::fs::remove_file(mine.join("assets/a-new-asset.md")).unwrap();
        assert_eq!(
            moved,
            tree_hash(&mine).expect("the shrunk set hashes"),
            "a file was dropped and the hash did not come back"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Finder writes a `.DS_Store` into the installed defaults the first time
    /// somebody opens the machine directory, and Explorer its two. The copy is
    /// still this binary's set: it hashes to the line that pinned it, and the
    /// next install finds it already standing rather than refusing it as edited.
    #[test]
    fn os_litter_in_the_installed_set_leaves_its_hash_and_its_pin_standing() {
        let dir = scratch("litter");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        install(&root, &packs, &lock_path, VERSION, "2026-09-12").expect("it lands");

        for sub in ["", "assets/", "doctor/"] {
            for name in crate::pack::OS_LITTER {
                std::fs::write(
                    root.join(format!("{sub}{name}")),
                    b"\x00\x00\x00\x01Bud1\x00\x00\x10\x00\xff\xfe",
                )
                .expect("the litter is written");
            }
        }
        assert!(root.join("assets/.DS_Store").is_file(), "the litter landed");

        assert_eq!(
            tree_hash(&root).expect("the littered copy hashes"),
            embedded_hash(),
            "the litter is not part of the set, so it moves no hash"
        );
        let again = install(&root, &packs, &lock_path, VERSION, "2026-09-13")
            .expect("a littered copy is not an edited one");
        assert_eq!(again.landed, Landed::Already);
        assert_eq!(again.entry, line(&lock_path), "the pin stands as written");

        // The control: a file that is not litter is still an edit, so the read
        // above is the litter being skipped and not the comparison gone blind.
        std::fs::write(root.join("assets/.DS_Store.bak"), "x\n").unwrap();
        assert!(
            matches!(
                install(&root, &packs, &lock_path, VERSION, "2026-09-14"),
                Err(Refusal::Shadowed { .. })
            ),
            "a file somebody added is still refused"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The build walks the crate's `defaults/` tree, and a checkout that Finder
    /// has opened carries a `.DS_Store` in it. That file is never embedded: a
    /// set carrying it would hash differently from every copy [`tree_hash`]
    /// reads, which leaves the litter out, and would put a name outside every
    /// slot into the bottom layer.
    #[test]
    fn the_embedded_set_carries_no_os_litter() {
        let litter: Vec<&str> = embedded::FILES
            .iter()
            .map(|file| file.path)
            .filter(|path| path.split('/').any(crate::pack::is_os_litter))
            .collect();
        assert_eq!(litter, Vec::<&str>::new());
    }

    /// THE REGISTRY IS PINNED TO THE SET (the reference's build-pack lesson,
    /// `brain/lessons/gas-city.md` G24): every registered path is in the table,
    /// and every path in the table is registered or is the registry itself. A
    /// file added to `defaults/` with no row reds this, and so does a row whose
    /// file left.
    #[test]
    fn every_embedded_path_is_registered_and_every_row_is_embedded() {
        let registry = registry::parse(
            std::str::from_utf8(
                embedded::bytes(registry::REGISTRY).expect("the set carries the registry"),
            )
            .expect("the registry is UTF-8"),
        )
        .expect("the registry parses");

        let embedded: Vec<&str> = embedded::paths().collect();
        assert!(
            embedded.len() > 1,
            "the table is empty, so both directions below are vacuous"
        );

        let unembedded: Vec<&str> = registry
            .shadows
            .iter()
            .map(|s| s.path.as_str())
            .filter(|path| !embedded.contains(path))
            .collect();
        assert_eq!(
            unembedded,
            Vec::<&str>::new(),
            "the registry lists a path this binary does not carry"
        );

        let unlisted: Vec<&str> = embedded
            .iter()
            .copied()
            .filter(|path| *path != registry::REGISTRY && !registry.lists(path))
            .collect();
        assert_eq!(
            unlisted,
            Vec::<&str>::new(),
            "this binary carries a default no row lets a pack replace"
        );
    }

    /// A machine the PREVIOUS binary left behind: `packs/core` beside a
    /// `bundled:core` line pinned at that tree's own hash. The install takes it
    /// out with its line, because the resolver holds no place for it any more
    /// and a copy left standing layers above the defaults and answers every
    /// template out of the stale tree.
    fn a_bundled_core_pack(packs: &Path, lock_path: &Path, body: &str) -> PathBuf {
        let root = packs.join(RETIRED_NAME);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("pack.toml"), "[pack]\nname = \"core\"\n").unwrap();
        std::fs::write(root.join("assets/rules.md"), body).unwrap();
        pin(
            lock_path,
            &lock::Entry {
                source: RETIRED_SOURCE.to_string(),
                name: Some(RETIRED_NAME.to_string()),
                version: "0.1.0".to_string(),
                commit: "bundled".to_string(),
                fetched: "2026-09-01".to_string(),
                tree: Some(tree_hash(&root).expect("the seeded pack hashes")),
            },
        )
        .expect("the bundled line is pinned");
        root
    }

    #[test]
    fn the_bundled_core_pack_a_previous_binary_left_is_retired_with_its_line() {
        let dir = scratch("retire");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        let theirs = a_bundled_core_pack(&packs, &lock_path, "the stale rules\n");

        let done = install(&root, &packs, &lock_path, VERSION, "2026-09-22").expect("it lands");

        assert_eq!(done.retired, Some(Retired::Removed(theirs.clone())));
        assert!(!theirs.exists(), "the pack is still on disk");
        let lines = lock::read(&lock_path).expect("the lock reads");
        assert!(
            !lines.iter().any(|e| e.source == RETIRED_SOURCE),
            "the line is still on the lock: {lines:?}"
        );
        assert!(
            lines.iter().any(|e| e.source == SOURCE),
            "and the defaults' own line is there: {lines:?}"
        );

        // The control on "it was the retirement and not the fixture": a second
        // install over the same machine finds no line and retires nothing.
        let again = install(&root, &packs, &lock_path, VERSION, "2026-09-23")
            .expect("the standing set stands");
        assert_eq!(again.retired, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other half: a copy nothing accounts for is NAMED and left where it
    /// stands, directory and line both, because a line dropped without its
    /// directory leaves a pack no record accounts for.
    #[test]
    fn a_bundled_core_pack_edited_since_it_was_pinned_is_named_and_left_standing() {
        let dir = scratch("retire-edited");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        let theirs = a_bundled_core_pack(&packs, &lock_path, "the rules as pinned\n");
        std::fs::write(theirs.join("assets/rules.md"), "a line a person put here\n").unwrap();

        let done = install(&root, &packs, &lock_path, VERSION, "2026-09-22").expect("it lands");

        match done.retired {
            Some(Retired::LeftStanding { root: named, why }) => {
                assert_eq!(named, theirs);
                assert!(why.contains("edited since"), "{why}");
            }
            other => panic!("an edited copy was not left standing: {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(theirs.join("assets/rules.md")).unwrap(),
            "a line a person put here\n",
            "their copy was removed"
        );
        assert!(
            lock::read(&lock_path)
                .expect("the lock reads")
                .iter()
                .any(|e| e.source == RETIRED_SOURCE),
            "the line went without the directory"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A line whose directory a person already deleted: the line goes, and
    /// nothing is removed that is not there.
    #[test]
    fn a_bundled_core_line_whose_directory_is_gone_loses_the_line_alone() {
        let dir = scratch("retire-gone");
        let root = dir.join(DIR);
        let packs = dir.join("packs");
        let lock_path = dir.join(lock::LOCK);
        let theirs = a_bundled_core_pack(&packs, &lock_path, "the stale rules\n");
        std::fs::remove_dir_all(&theirs).unwrap();

        let done = install(&root, &packs, &lock_path, VERSION, "2026-09-22").expect("it lands");

        assert_eq!(done.retired, Some(Retired::Removed(theirs)));
        assert!(!lock::read(&lock_path)
            .expect("the lock reads")
            .iter()
            .any(|e| e.source == RETIRED_SOURCE));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
