//! The pinning a flight and a run share: the files written into a directory,
//! and the directory's hash read back off the disk.
//!
//! ONE WRITER AND ONE DIGEST FOR BOTH LIFECYCLES. Each caller decides which
//! files its directory holds and in what order the hash reads them — those are
//! the lifecycle's own facts — and neither owns the bytes-to-hex walk, so a
//! flight's hash and a run's cannot come to disagree about what hashing a
//! directory means.

use std::path::Path;

use crate::digest::Sha256;
use crate::item::Stop;

/// The files written, each under the directory, with the directories above
/// them made first.
///
/// It writes and does not hash: a caller whose directory gains a file from a
/// CHILD between the two — a run's bundle — has to hash after that child has
/// run, and a function that did both would make the two orders impossible to
/// tell apart.
pub fn write_files(directory: &Path, files: &[(String, Vec<u8>)]) -> Result<(), Stop> {
    for (relative, body) in files {
        let path = directory.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Stop::could_not_tell(format!("{} could not be made: {e}", parent.display()))
            })?;
        }
        std::fs::write(&path, body).map_err(|e| {
            Stop::could_not_tell(format!("{} could not be written: {e}", path.display()))
        })?;
    }
    Ok(())
}

/// The directory's hash: each file's relative path, then its bytes, in the
/// order given.
///
/// The bytes are read back OFF THE DISK rather than hashed from the buffers
/// just written, so the hash is of the files a reader will open and not of
/// what this process meant to put in them.
pub fn hash_of(directory: &Path, relatives: &[String]) -> Result<String, Stop> {
    let mut digest = Sha256::new();
    for relative in relatives {
        let path = directory.join(relative);
        let body = std::fs::read(&path).map_err(|e| {
            Stop::could_not_tell(format!(
                "{} could not be read back for the hash: {e}",
                path.display()
            ))
        })?;
        digest.update(relative.as_bytes());
        digest.update(&body);
    }
    Ok(digest.hex())
}
