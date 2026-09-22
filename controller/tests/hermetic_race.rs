//! `refusing_stub` under libtest's threads, not nextest's one process per
//! arm: two callers never race the rename onto the shared stub path.

mod common;

use common::hermetic::refusing_stub;
use std::thread;

/// Two threads call `refusing_stub` at once, each asserting the path it gets
/// back is present and executable. Looped a bounded number of times because a
/// race can pass by luck on a quiet box; red on the pid-only name by
/// construction of the race, since libtest runs this binary's arms — this one
/// included — as threads of one process.
#[test]
fn two_threads_racing_the_stub_both_see_it_present_and_executable() {
    for _ in 0..20 {
        let a = thread::spawn(refusing_stub);
        let b = thread::spawn(refusing_stub);
        let path_a = a.join().expect("the first caller does not panic");
        let path_b = b.join().expect("the second caller does not panic");
        for path in [&path_a, &path_b] {
            assert!(path.exists(), "the stub exists at {}", path.display());
            let mode = std::fs::metadata(path)
                .expect("the stub is readable")
                .permissions();
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                mode.mode() & 0o111,
                0o111,
                "the stub is executable at {}",
                path.display()
            );
        }
    }
}
