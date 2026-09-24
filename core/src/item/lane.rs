//! The landing lane: the worktree the fleet owns per project, and the lock
//! every landing queues on.
//!
//! ONE QUEUE PER PROJECT, AND `land` IS WHAT TAKES IT. The tick's landings and
//! a person's `fleet land` wait on the same object because both reach it the
//! same way — from the machine directory and the project name they already
//! hand the verb — so neither caller has to know the other exists.
//!
//! THE LOCK SITS BESIDE THE LANE, NEVER INSIDE IT. A landing refuses on a
//! working tree it did not expect, so a file the fleet keeps inside the lane
//! would be one every landing had to explain. The path is derived from the
//! lane's own by suffix, which is the derivation the controller's document lock
//! already uses.
//!
//! IT BLOCKS WITH NO DEADLINE. A landing is bounded by its own suite and not by
//! a timeout this layer could pick; what a waiting caller gets instead is the
//! holder's name, read from the lock file, printed before the wait begins.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::item::{Progress, Stop};
use crate::policy;

/// `[core.flight] lanes` where the policy names none: one directory under the
/// machine directory holding one worktree per project.
pub const LANES: &str = "lanes";

/// What the lane's own directory is called, ahead of the project's name.
///
/// A CHECKOUT'S BASENAME IS READ AS A NAME BY TOOLS THE FLEET DOES NOT OWN. A
/// project whose repository resolves a seat from the directory it is run in
/// reads a lane named after the project as a seat of that name and refuses on
/// every landing, so the lane wears a prefix no seat-name resolver claims.
pub const LANE_PREFIX: &str = "lane-";

/// The suffix the lock file carries beside the lane, as the controller's own
/// document lock derives one.
pub const LOCK_SUFFIX: &str = ".lock";

/// `[core.flight] rerun_wait_seconds` where the policy names none: how long the
/// rerun waits for the box's load to fall before running anyway.
pub const RERUN_WAIT_SECONDS: u64 = 300;

/// The line a landing prints when the lane is held. It opens at column zero and
/// names the holder, because the reader deciding whether to wait is a person
/// watching one landing and wondering whose the other one is.
pub const WAITING: &str = "waiting on the lane:";

/// How often a wait re-reads: the lock is re-tried by blocking on it rather
/// than polled, so this paces only the bar's message.
const TICK: Duration = Duration::from_millis(200);

/// Where this project's lane lives: `[core.flight] lanes` under the machine
/// directory, `lanes/` where the policy names none, with `lane-<project>`
/// beneath it.
///
/// A relative value is read against the MACHINE directory and not the caller's
/// cwd, because the fleet's own policy file is what writes it and a landing is
/// run from wherever a reviewer stands.
///
/// RESOLVING THE LANE IS WHAT ADOPTS ONE CUT UNDER THE PROJECT'S BARE NAME, so
/// that a caller cannot reach the path without taking the older directory with
/// it and leaving a machine's build cache stranded beside a freshly cut lane.
pub fn directory(machine_dir: &Path, guards: &toml::Table, project: &str) -> Result<PathBuf, Stop> {
    let named = policy::read("core.flight", "lanes", guards)
        .map_err(|unlisted| Stop::could_not_tell(unlisted.to_string()))?;
    let lanes = match named {
        None => machine_dir.join(LANES),
        Some(value) => {
            let Some(text) = value.as_str() else {
                return Err(Stop::refused(format!(
                    "`[core.flight] lanes` is {}, and a directory has to be a string — this verb \
                     will not fall back to one the fleet did not name",
                    value.type_str()
                )));
            };
            let text = text.trim();
            if text.is_empty() {
                machine_dir.join(LANES)
            } else {
                let path = PathBuf::from(text);
                if path.is_absolute() {
                    path
                } else {
                    machine_dir.join(path)
                }
            }
        }
    };
    let lane = lanes.join(format!("{LANE_PREFIX}{project}"));
    adopt(&lane, &lanes.join(project))?;
    Ok(lane)
}

/// A lane cut under the project's bare name, taken over under the prefixed one.
/// A machine that already has one keeps it, with its build cache and its
/// registration, rather than cutting a second beside it.
///
/// IT RUNS ONLY WHERE THE PREFIXED NAME IS ABSENT, which makes it a migration
/// that happens once per machine and a pair of stats on every landing after it.
///
/// THE LOCK MOVES FIRST, and only onto a name no lock file holds yet: a landing
/// already queued under the old name holds that file's inode, and carrying the
/// name onto it is what keeps the two landings serialised across the migration
/// instead of letting them run at once on one worktree.
///
/// A WORKTREE MOVES THROUGH GIT AND NOT BY RENAME. The primary records the
/// lane's path, and a directory moved behind git's back is one the next
/// `git worktree prune` unregisters. A move that will not run leaves the lane
/// where it is and refuses the landing, because the two recoveries a person has
/// from here both start with the lane still being there.
fn adopt(lane: &Path, old: &Path) -> Result<(), Stop> {
    if lane.exists() || !old.is_dir() {
        return Ok(());
    }
    let (from, to) = (lock_path(old), lock_path(lane));
    if from.exists() && !to.exists() {
        std::fs::rename(&from, &to).map_err(|e| {
            Stop::could_not_tell(format!(
                "the lane's lock {} could not be carried to {}: {e} — the lane at {} is \
                 where it was",
                from.display(),
                to.display(),
                old.display()
            ))
        })?;
    }
    if !old.join(".git").exists() {
        return std::fs::rename(old, lane).map_err(|e| {
            Stop::could_not_tell(format!(
                "the lane at {} could not be taken over as {}: {e}",
                old.display(),
                lane.display()
            ))
        });
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(old)
        .args(["worktree", "move"])
        .arg(old)
        .arg(lane)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "`git worktree move` could not be run to take over the lane at {}: {e}",
                old.display()
            ))
        })?;
    if !out.status.success() {
        return Err(Stop::could_not_tell(format!(
            "the lane at {} could not be taken over as {} {}: {} — it is left where it is, and a \
             landing does not cut a second lane beside it",
            old.display(),
            lane.display(),
            match out.status.code() {
                Some(code) => format!("(`git worktree move` exited {code})"),
                None => String::from("(`git worktree move` was killed by a signal)"),
            },
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// The lock file that guards a lane, derived from the lane's own path by
/// suffix. One derivation and not two that agree today.
pub fn lock_path(lane: &Path) -> PathBuf {
    let mut name = lane.file_name().unwrap_or_default().to_os_string();
    name.push(LOCK_SUFFIX);
    lane.with_file_name(name)
}

/// `[core.flight] rerun_wait_seconds`, as a duration.
///
/// A value that is not an integer is a refusal naming the key rather than the
/// default: a fleet that wrote a wait and had it silently ignored reruns on a
/// box it meant to wait for.
pub fn rerun_wait(guards: &toml::Table) -> Result<Duration, Stop> {
    let named = policy::read("core.flight", "rerun_wait_seconds", guards)
        .map_err(|unlisted| Stop::could_not_tell(unlisted.to_string()))?;
    let Some(value) = named else {
        return Ok(Duration::from_secs(RERUN_WAIT_SECONDS));
    };
    match value.as_integer() {
        Some(seconds) if seconds >= 0 => Ok(Duration::from_secs(seconds as u64)),
        _ => Err(Stop::refused(format!(
            "`[core.flight] rerun_wait_seconds` is {}, and a wait has to be a whole number of \
             seconds that is not negative",
            value.type_str()
        ))),
    }
}

/// The lane, held. The lock is released when this is dropped, which is when the
/// landing returns — whatever its exit.
///
/// THE FILE IS LEFT IN PLACE. Unlinking it would let the next caller hold a
/// descriptor on an inode nobody else can reach, whose lock then guards
/// nothing.
pub struct Held {
    file: File,
    path: PathBuf,
}

impl Held {
    /// The lock file this holds, for a caller that wants to say where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The holder's line, written into the file AFTER the lock is held so that
    /// what a waiting caller reads is the holder and never a claim nobody won.
    ///
    /// A write that fails costs the next caller its holder line and nothing
    /// else, so it does not take the landing with it.
    fn claim(&self, item: &str, at: &str) {
        let _ = std::fs::write(&self.path, format!("{item} {at}\n"));
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// The lane taken for one landing, blocking until it is free.
///
/// The holder is read and printed BEFORE the wait, because a line written after
/// the wait ended is one nobody waiting ever saw. The read can fail — a holder
/// that has not written its line yet is the ordinary race — and an unreadable
/// holder is printed as such rather than waited on in silence.
///
/// An instrument that would not answer is exit 3 with nothing written: a lock
/// file that cannot be created is not a landing that may proceed unserialised.
pub fn take(
    out: &mut dyn Write,
    progress: &dyn Progress,
    lane: &Path,
    item: &str,
    at: &str,
) -> Result<Held, Stop> {
    let path = lock_path(lane);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| {
            Stop::could_not_tell(format!(
                "the lane's directory {} could not be made: {e} — a landing queues on the lane or \
                 it does not run",
                dir.display()
            ))
        })?;
    }
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "the lane's lock {} could not be opened: {e} — a landing queues on the lane or it \
                 does not run",
                path.display()
            ))
        })?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let holder = holder_of(&path);
            let _ = writeln!(out, "{WAITING} {holder}");
            progress.message(&format!("{WAITING} {holder}"));
            // The blocking wait, taken after the line: the bar's message is the
            // last thing it says, so a landing that sits here says whose.
            file.lock().map_err(|e| {
                Stop::could_not_tell(format!(
                    "the lane's lock {} could not be taken: {e}",
                    path.display()
                ))
            })?;
        }
        Err(std::fs::TryLockError::Error(e)) => {
            return Err(Stop::could_not_tell(format!(
                "the lane's lock {} could not be taken: {e}",
                path.display()
            )))
        }
    }
    let held = Held { file, path };
    held.claim(item, at);
    Ok(held)
}

/// What the lock file says about its holder, as one phrase for the waiting
/// line. A file that will not read, or one nobody has claimed yet, is named as
/// unread rather than reported as free.
fn holder_of(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return format!(
            "a landing that has not said which, the lock at {} unreadable",
            path.display()
        );
    };
    let line = text.lines().next().unwrap_or_default().trim();
    match line.split_once(char::is_whitespace) {
        Some((item, at)) => format!("{item} since {}", at.trim()),
        None if !line.is_empty() => format!("{line} since (none)"),
        None => format!(
            "a landing that has not said which, the lock at {}",
            path.display()
        ),
    }
}

/// The box's five-minute load and the ceiling it is judged against, as the
/// caller reads them.
///
/// A SEAM AND NOT A VALUE, because the wait exists to see the load FALL and one
/// reading cannot show that. `None` is a leg nobody could read — never a box
/// under no load — and a wait that cannot read the load does not wait.
pub trait Load {
    /// The five-minute average and the ceiling, read NOW.
    fn read(&self) -> Option<(f64, f64)>;
}

/// The load nobody read, for a caller with no controller behind it: every
/// reading is absent, so a rerun runs at once and says it did not wait.
pub struct Unread;

impl Load for Unread {
    fn read(&self) -> Option<(f64, f64)> {
        None
    }
}

/// What a wait for the box to quieten ended as. Each is a clause the second
/// gate row prints, so the row says which of the three happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waited {
    /// The load was already under the ceiling, or fell under it in time.
    Quiet(String),
    /// The wait ran out and the rerun runs anyway.
    Expired(String),
    /// No reading, so there was nothing to wait for.
    Unreadable,
}

impl Waited {
    /// The clause the second gate row and the second `check.read` carry.
    pub fn clause(&self) -> String {
        match self {
            Waited::Quiet(text) => text.clone(),
            Waited::Expired(text) => text.clone(),
            Waited::Unreadable => {
                "the box's load could not be read, so the rerun did not wait for it".to_string()
            }
        }
    }
}

/// Wait for the five-minute load to fall under the belt's ceiling, bounded by
/// `[core.flight] rerun_wait_seconds`.
///
/// THE EXPIRY RERUNS ANYWAY AND SAYS SO. A suite that raced the box is rerun
/// beside a quieter one where the box quietens, and on a box that never does the
/// rerun still happens — with the row saying the wait expired, so the reader of
/// a second red knows which kind of box it was taken on.
pub fn wait_for_a_quiet_box(load: &dyn Load, limit: Duration, progress: &dyn Progress) -> Waited {
    let Some((first, ceiling)) = load.read() else {
        return Waited::Unreadable;
    };
    if first <= ceiling {
        return Waited::Quiet(format!(
            "the box was already quiet — load {first:.2} against a ceiling of {ceiling:.2}, no wait"
        ));
    }
    let started = Instant::now();
    loop {
        if started.elapsed() >= limit {
            let (now, ceiling) = load.read().unwrap_or((first, ceiling));
            return Waited::Expired(format!(
                "the wait of {}s expired with the box still busy — load {now:.2} against a ceiling \
                 of {ceiling:.2}, and the rerun ran anyway",
                limit.as_secs()
            ));
        }
        let Some((now, ceiling)) = load.read() else {
            return Waited::Unreadable;
        };
        if now <= ceiling {
            return Waited::Quiet(format!(
                "the box quietened after {:.1}s — load {now:.2} against a ceiling of {ceiling:.2}",
                started.elapsed().as_secs_f64()
            ));
        }
        progress.message(&format!(
            "waiting for the box — load {now:.2} against a ceiling of {ceiling:.2}"
        ));
        std::thread::sleep(TICK);
    }
}
