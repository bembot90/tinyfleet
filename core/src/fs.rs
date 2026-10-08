//! Whole-document writes, the same on every platform.

use std::io::{self, Write};
use std::path::Path;

/// Write a whole document or none of it: a temp file in the destination
/// directory, flushed, then renamed over the path. A reader holding the old
/// path sees a whole old document or a whole new one, never a torn one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let write = || -> io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn an_atomic_write_leaves_no_temp_file_behind() {
        let dir = std::env::temp_dir().join(format!("fleet-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("projection.json");
        write_atomic(&path, b"{\"version\":1}").expect("the write lands");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\":1}");
        write_atomic(&path, b"{\"version\":2}").expect("the rewrite lands");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\":2}");
        let strays: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "projection.json")
            .collect();
        assert!(strays.is_empty(), "temp files survived: {strays:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other end of the same property: a write that CANNOT land leaves
    /// nothing behind either. The temp file carries the publishing process's
    /// pid, so a rename that keeps failing is one stray document per poll in the
    /// directory the operator reads.
    #[test]
    fn a_write_whose_rename_fails_leaves_no_temp_file_behind() {
        let dir = std::env::temp_dir().join(format!("fleet-atomic-blocked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("projection.json");
        // A non-empty directory standing where the document goes: the rename
        // over it is what fails, which is the write's own failure path and not a
        // missing parent.
        std::fs::create_dir_all(path.join("occupied")).unwrap();

        let failed = write_atomic(&path, b"{\"version\":1}");
        assert!(failed.is_err(), "the rename over a directory cannot land");
        assert!(
            path.join("occupied").exists(),
            "the destination is untouched, so the failure above is the rename's"
        );
        let strays: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "projection.json")
            .collect();
        assert!(
            strays.is_empty(),
            "the failing branch left its temp file behind: {strays:?}"
        );

        // The control: with the way clear the same call lands, so the empty
        // listing above is a directory this write can be observed writing into.
        std::fs::remove_dir_all(&path).unwrap();
        write_atomic(&path, b"{\"version\":1}").expect("the write lands");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\":1}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The write's OWN failure, which is the other cleanup site: the arm above
    /// fails the rename and reaches only that one.
    ///
    /// The temp path is occupied by a file this process cannot open for writing,
    /// so `File::create` fails where the destination is fine. That file is the
    /// measurement twice over — it is at the temp path only if the name carries
    /// this process's pid, and it is gone afterwards only if the write-failure
    /// branch cleans up.
    ///
    /// THE DENIAL IS THE MODE BIT, AND UID 0 IGNORES IT — so each uid asserts
    /// the outcome its own call has. Root opens the `0o444` file for writing,
    /// takes the SUCCESS path through the same call, and reads the landing: the
    /// destination carries the bytes and the planted file is gone, renamed onto
    /// the destination. That keeps the name reading on both branches, because a
    /// `write_atomic` that spelled its temp file differently would leave the
    /// planted one standing either way. What root cannot reach is the cleanup
    /// site, which is why the branch below it is the one this arm is named for.
    ///
    /// NEITHER BRANCH MAY RETURN WITHOUT ASSERTING. `libtest` captures a passing
    /// arm's output, so a uid that states a skip and returns ships `ok` having
    /// measured nothing and telling the gate nothing. An arm that cannot take
    /// its reading on some box asserts what holds on every box instead.
    ///
    /// The sibling arm's technique is not the alternative: a DIRECTORY at the
    /// temp path denies `File::create` to root too, but the failure branch
    /// cleans up with `remove_file`, which does not remove a directory, so the
    /// third assertion loses its subject.
    #[test]
    fn a_write_that_cannot_open_its_temp_file_cleans_up_and_names_it_per_process() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("fleet-atomic-unwritable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("projection.json");
        std::fs::create_dir_all(&dir).unwrap();

        // Spelled the way write_atomic spells it: a write that names its temp
        // file differently opens a fresh path and lands, which reds the arm.
        let tmp = dir.join(format!(".projection.json.tmp-{}", std::process::id()));
        std::fs::write(&tmp, b"debris from a previous write").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444)).unwrap();

        // SAFETY: geteuid takes nothing, returns the effective uid and cannot fail.
        let as_root = unsafe { libc::geteuid() } == 0;
        let written = write_atomic(&path, b"{\"version\":1}");
        if as_root {
            written.expect("uid 0 opens the read-only temp file, so the write lands");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "{\"version\":1}",
                "the destination carries what uid 0 wrote through the planted temp file"
            );
            assert!(
                !tmp.exists(),
                "the planted file was not the one the write used, so it was never \
                 renamed away: {}",
                tmp.display()
            );
        } else {
            assert!(
                written.is_err(),
                "the temp file cannot be opened for writing, so the write fails: {written:?}"
            );
            assert!(
                !path.exists(),
                "and nothing was renamed over the destination"
            );
            assert!(
                !tmp.exists(),
                "the write-failure branch left its temp file behind: {}",
                tmp.display()
            );
        }

        // The control for the branch that FAILED: with the temp path clear the
        // same call lands, so that failure is the unwritable temp file's and not
        // this directory's. Under uid 0 nothing failed and this repeats a write
        // that already landed, which is why that branch reads the destination
        // where this control would.
        write_atomic(&path, b"{\"version\":1}").expect("the write lands");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\":1}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The property the temp-file-and-rename exists for, and the one an
    /// in-place truncating write fails: a reader holding the path sees a whole
    /// document on every read, through twenty rewrites of half a megabyte.
    ///
    /// The read count is asserted too — a reader that never ran would report
    /// zero short reads and prove nothing.
    #[test]
    fn a_reader_never_catches_a_half_written_document() {
        const SIZE: usize = 512_000;
        let dir = std::env::temp_dir().join(format!("fleet-atomic-torn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("projection.json");
        let old = "a".repeat(SIZE);
        let new = "b".repeat(SIZE);
        write_atomic(&path, old.as_bytes()).expect("the first write lands");

        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let reader_stop = stop.clone();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            let (mut reads, mut torn) = (0u32, 0u32);
            while !reader_stop.load(Ordering::SeqCst) {
                if let Ok(body) = std::fs::read_to_string(&reader_path) {
                    reads += 1;
                    if body.len() != SIZE {
                        torn += 1;
                    }
                }
            }
            (reads, torn)
        });

        for turn in 0..20 {
            let body = if turn % 2 == 0 { &new } else { &old };
            write_atomic(&path, body.as_bytes()).expect("the rewrite lands");
        }
        stop.store(true, Ordering::SeqCst);
        let (reads, torn) = reader.join().expect("the reader thread finishes");

        assert!(
            reads > 20,
            "the reader must have read during the writes: {reads}"
        );
        assert_eq!(torn, 0, "{torn} of {reads} reads caught a partial document");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
