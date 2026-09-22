//! The defaults this binary carries: every file under the crate's `defaults/`
//! tree, embedded by `build.rs` and written out whole.
//!
//! THE TABLE IS THE BINARY'S OWN and no verb reads it directly. Every reader of
//! an asset takes a PATH — the doctor's scripts are executed, the hooks and
//! permissions files are copied into a worktree — so what the verbs resolve
//! through is the directory [`write_all`] materializes, and the resolver's
//! bottom layer is that directory (`crate::defaults`).

use std::io;
use std::path::Path;

/// One embedded default: its path relative to the tree's root, its bytes, and
/// the executable bit the source carries.
pub struct File {
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub executable: bool,
}

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

/// Every embedded default written under `dir`, directories made as needed.
///
/// WHOLE OR NOT AT ALL is the caller's: this writes file by file and the caller
/// writes into a scratch directory it renames in (`crate::defaults::install`),
/// so a write that fails half way leaves nothing a verb resolves through.
pub fn write_all(dir: &Path) -> io::Result<()> {
    for file in FILES {
        let target = dir.join(file.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, file.bytes)?;
        if file.executable {
            make_executable(&target)?;
        }
    }
    Ok(())
}

/// The paths in the table, in the order it carries them.
pub fn paths() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|file| file.path)
}

/// One embedded default by path, or none.
pub fn bytes(path: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.bytes)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut mode = std::fs::metadata(path)?.permissions();
    mode.set_mode(0o755);
    std::fs::set_permissions(path, mode)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}
