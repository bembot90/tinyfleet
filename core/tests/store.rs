//! `Store` as the `bd` binary answers it — the argv a read is actually made
//! with.
//!
//! The calls whose argv is load-bearing on its own are the LIST reads: each
//! verb caps its answer unless the cap is lifted, and a truncated list is
//! indistinguishable from a whole one, so nothing downstream can notice the
//! loss. The proof has to be the argv; the count is what cannot be measured.
//! One arm per such read, and an arm is what says the argument is still there.
//!
//! `Bd::at` runs `store::BD`, a bare name resolved on the process's own
//! `PATH`, so a directory prepended to `PATH` is the seam a shim enters
//! through for those arms — and `PATH` is process-wide. Every arm here
//! therefore takes `PATH_LOCK`, the arms wanting the real binary included: an
//! arm reading a real store while another has the shim in front of it would be
//! reading the shim.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use common::capped::{calls, capped_bd, Held};
use common::Fixture;
use fleet_core::store::{Bd, Store};

/// Serialises every arm in this binary, because the seam they share is the
/// process's `PATH` and there is one of those.
static PATH_LOCK: Mutex<()> = Mutex::new(());

/// Set from BEFORE the shim goes in front of the process's `PATH` until AFTER
/// it is put back, so it is never false while the shim is ahead. It errs the
/// safe way on purpose: a waiting arm may refuse a moment early, and can never
/// run because the flag had not caught up yet.
static SHIM_AHEAD: AtomicBool = AtomicBool::new(false);

/// Takes `PATH_LOCK`, reading a poisoned lock as the situation rather than as a
/// lock error.
///
/// A sibling that panicked with `PATH` already put back left the seam clean, so
/// this arm takes the lock and runs — its own red is the only one the binary
/// reports. A sibling that panicked with the shim still in front did not, and
/// this arm refuses rather than read whatever `bd` that leaves on `PATH`.
fn path_lock() -> MutexGuard<'static, ()> {
    match PATH_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            assert!(
                !SHIM_AHEAD.load(Ordering::SeqCst),
                "a sibling arm panicked with the shim still in front of PATH — this arm did not run"
            );
            poisoned.into_inner()
        }
    }
}

/// A `bd` on `PATH` that records the argv it was handed and answers an empty
/// list. Answers the same way whatever the verb: an arm asserting on the argv
/// wants the call made, not the answer.
fn shim(dir: &Fixture, log: &Path) {
    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{log}'; done\n\
             printf '[]\\n'\n",
            log = log.display(),
        ),
    )
    .expect("the shim is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the shim is executable");
}

/// Runs `body` with `dir` in front of the process's `PATH`, and puts `PATH`
/// back before returning — including when `body` answers a failure, so a red
/// arm never leaves the shim in front of a later one. A panic inside `body`
/// unwinds past that restore, so `SHIM_AHEAD` stays set and the next arm to
/// take `PATH_LOCK` refuses.
fn with_path_ahead<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
    let ahead = match std::env::var_os("PATH") {
        Some(rest) => format!("{}:{}", dir.display(), rest.to_string_lossy()),
        None => dir.display().to_string(),
    };
    with_path(&ahead, body)
}

/// The `PATH` a launchd agent is started with, which is the whole of this
/// box's search path for the controller: no package-manager prefix, where
/// `bd` actually is, and no user local bin either.
const SERVICE_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// Runs `body` with the process's `PATH` set to `path`, and puts `PATH` back
/// before returning — the same discipline and the same flag as
/// [`with_path_ahead`], because a `PATH` that is missing entries this process
/// started with is as unusable to a sibling arm as one with a shim in front.
fn with_path<T>(path: &str, body: impl FnOnce() -> T) -> T {
    let original = std::env::var_os("PATH");
    SHIM_AHEAD.store(true, Ordering::SeqCst);
    std::env::set_var("PATH", path);
    let answer = body();
    match original {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    SHIM_AHEAD.store(false, Ordering::SeqCst);
    answer
}

/// A store built on an ABSOLUTE binary runs it when the process's own `PATH`
/// is the service's and holds no `bd` at all — which is the controller's
/// situation on every tick.
///
/// THE CONTROL AND THE PROOF ARE ON ONE `PATH`: the same shim, in the same
/// directory, is unreachable by bare name and reachable by absolute path, so
/// the arm cannot go green because the shim happened to be findable.
#[test]
fn an_absolute_binary_runs_where_a_bare_name_cannot() {
    let _guard = path_lock();
    let dir = Fixture::new("store-absolute-bin");
    let log = dir.path("argv");
    shim(&dir, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let (by_name, by_path) = with_path(SERVICE_PATH, || {
        (
            Bd::at(&root).ready(),
            Bd::at_bin(&root, &dir.path("bd")).ready(),
        )
    });

    let refusal = by_name.expect_err("a bare `bd` is not on the service PATH");
    assert!(
        format!("{refusal:?}").contains("could not be run"),
        "the bare name must fail because nothing could be run, not for some \
         other reason — {refusal:?}"
    );
    assert_eq!(
        by_path.expect("the shim answers a list"),
        Vec::<String>::new()
    );

    let argv: Vec<String> = std::fs::read_to_string(&log)
        .expect("the shim recorded its argv")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv.len(),
        6,
        "exactly one of the two reads reached the shim, as `ready`'s six \
         arguments — {argv:?}"
    );
}

/// The ready read lifts the row cap.
///
/// The shim records the argv rather than the row count, because the count is
/// the thing that cannot be read: 100 rows is the honest answer for a pool of
/// 100 and the silent one for a pool of 200. `-n 0` in the argv is the whole
/// difference. A store big enough to show the loss directly costs about 110
/// seconds to build, which is why it is not built here.
#[test]
fn ready_lifts_the_row_cap() {
    let _guard = path_lock();
    let dir = Fixture::new("store-argv");
    let log = dir.path("argv");
    shim(&dir, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let answered = with_path_ahead(&dir.root, || Bd::at(&root).ready());

    assert_eq!(
        answered.expect("the shim answers a list"),
        Vec::<String>::new()
    );
    let argv: Vec<String> = std::fs::read_to_string(&log)
        .expect("the shim recorded its argv")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv,
        vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("ready"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ],
        "the ready read must carry `-n 0`, or it answers the first 100 rows only"
    );
}

/// The open-gate read lifts the row cap.
///
/// The same argument the ready read carries, against a different verb and for a
/// sharper consequence: `gate list` answers its first 50 rows with no `-n`, and
/// `answer` refuses a gate it cannot find on the open list as one somebody has
/// already resolved — so a truncated listing refuses a live question instead of
/// answering it. The argv is the proof here for the reason it is above: a board
/// big enough to drop a row costs about fifty times this arm to build, and what
/// went wrong was the argument.
#[test]
fn open_gates_lifts_the_row_cap() {
    let _guard = path_lock();
    let dir = Fixture::new("store-argv-gates");
    let log = dir.path("argv");
    shim(&dir, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let answered = with_path_ahead(&dir.root, || Bd::at(&root).open_gates());

    assert_eq!(
        answered.expect("the shim answers a list"),
        Vec::<String>::new()
    );
    let argv: Vec<String> = std::fs::read_to_string(&log)
        .expect("the shim recorded its argv")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv,
        vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("gate"),
            String::from("list"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ],
        "the open-gate read must carry `-n 0`, or it answers the first 50 rows only"
    );
}

/// A seat's listing lifts the row cap, and answers the row past it.
///
/// The one list read here proved by the COUNT as well as by the argv, because
/// the fake that caps it is cheap: 51 non-closed rows against one seat, the
/// 51st the one a capped read drops. A retire takes a seat's ordered items off
/// this listing before its name goes back on the pile, so a row it cannot see
/// is an order the next seat of that name inherits.
#[test]
fn a_seat_listing_lifts_the_row_cap() {
    let _guard = path_lock();
    let dir = Fixture::new("store-argv-seat");
    let log = dir.path("argv");
    let ids: Vec<String> = (1..=51).map(|n| format!("fx-row-{n:02}")).collect();
    let rows: Vec<Held> = ids.iter().map(|id| Held { id, ordered: false }).collect();
    capped_bd(&dir, "transient-3", &rows, &log);

    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let answered = with_path_ahead(&dir.root, || Bd::at(&root).assigned_to("transient-3"));

    let held: Vec<String> = answered
        .expect("the fake answers a list")
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(held, ids, "every row the seat holds, the 51st included");
    assert_eq!(
        calls(&log),
        vec![vec![
            String::from("-C"),
            root.display().to_string(),
            String::from("list"),
            String::from("-a"),
            String::from("transient-3"),
            String::from("--json"),
            String::from("-n"),
            String::from("0"),
        ]],
        "the seat's listing must carry `-n 0`, or it answers the first 50 rows only"
    );
}
