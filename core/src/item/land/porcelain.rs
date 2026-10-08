//! What git printed, read back: the push's range line and the porcelain's paths.

use std::path::Path;

use super::Wiring;
use crate::item::{StatusLine, Stop, TRUNK_BRANCH};
use crate::store::types::ExportSpec;

// ---- the readings -------------------------------------------------------------

/// The one reading of the landed sha, and of the range's other end: the line
/// git's own push prints, and nothing else.
pub(super) fn range(output: &str) -> Option<(String, String)> {
    let tail = format!("HEAD -> {TRUNK_BRANCH}");
    output.lines().find_map(|line| {
        let line = line.trim();
        if !line.ends_with(&tail) {
            return None;
        }
        let (span, _) = line.split_once(char::is_whitespace)?;
        let (old, new) = span.split_once("..")?;
        (is_hex(old) && is_hex(new)).then(|| (old.to_string(), new.to_string()))
    })
}

/// One end of the push's range line, whole: what it resolves to in the tree
/// that pushed it. The push abbreviates, and every sha this landing writes is
/// the resolved one.
///
/// A name this checkout cannot resolve is could-not-tell, AFTER the push: the
/// work is on the trunk, and the sentence says what stands and what was not
/// written.
pub(super) fn full(short: &str, item: &str, wiring: &Wiring) -> Result<String, Stop> {
    match wiring.git.rev(short) {
        Ok(Some(sha)) => Ok(sha),
        Ok(None) | Err(_) => Err(Stop::could_not_tell(format!(
            "the push named {short}, which resolves to no commit in this checkout — the landing \
             STANDS on {TRUNK_BRANCH} and {item} carries no landed entry and is not closed"
        ))),
    }
}

fn is_hex(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The first path `git status --porcelain` reports that `--also` did not admit.
/// A landing squashes a commit, so every working-tree change is outside it but
/// the ones the reviewer named — and the store's export, which this verb
/// rewrites itself between its own first refusal and the re-run after it.
///
/// THE EXPORT AND NOT THE DIRECTORY: a refusal from (e) on hard-resets the
/// tree, so a path this check lets past is a path that reset discards without a
/// word. A tracked edit to any other file under the store's directory — a hook,
/// its config — is a seat's work and refuses here like any other.
pub(super) fn outside_also(
    status: &[StatusLine],
    also: &[String],
    export: Option<&ExportSpec>,
) -> Option<String> {
    status
        .iter()
        .map(|line| &line.path)
        .find(|&path| !also.contains(path) && !is_store_export(path, export))
        .cloned()
}

/// The store paths a landing rewrites itself, in both spellings the porcelain
/// uses for them: the export by name, and the store's whole directory, which is
/// what an untracked-but-unignored store answers with. A store that declares no
/// export has neither.
pub(super) fn is_store_export(path: &str, export: Option<&ExportSpec>) -> bool {
    export.is_some_and(|e| path == e.file || path == e.dir)
}

/// A path set with the store's own directory taken out, sorted and deduplicated
/// so the two sides of the staged-set check are compared as sets. A store that
/// declares no export has no directory to take out.
pub(super) fn outside_store(paths: &[String], export: Option<&ExportSpec>) -> Vec<String> {
    let mut kept: Vec<String> = paths
        .iter()
        .filter(|path| export.is_none_or(|e| !path.starts_with(&e.dir)))
        .cloned()
        .collect();
    kept.sort();
    kept.dedup();
    kept
}

/// A file as three independent readings — when it was written, how long it is,
/// and what is in it. `None` is a file that is not there, which is a fourth
/// answer and not a fourth reading.
///
/// Three because one is not enough: a filesystem whose mtime is coarser than
/// this act reads two writes in a tick as one, and a rewrite that happens to
/// keep the length is not a rewrite this verb may miss. The content is hashed
/// rather than kept, because the question is only whether two readings of one
/// path differ.
pub(super) fn fingerprint(path: &Path) -> Option<(std::time::SystemTime, u64, u64)> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((meta.modified().ok()?, meta.len(), hasher.finish()))
}
