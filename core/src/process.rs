//! The bounded runner: a child that leads a process group of its own, a
//! deadline that kills the group, and both pipes drained from threads of their
//! own.
//!
//! IT IS CORE'S so an adapter's call (`adapter::exec`) can be bounded here, and
//! core depends on no other member of the workspace. The controller's platform layer re-exports
//! every public name here, so its callers reach them by the paths they always
//! had.

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

// `setpgid` and `killpg` are POSIX and identical on both targets, so they are
// declared once here and no platform carries a half of its own.
extern "C" {
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn killpg(pgrp: i32, sig: i32) -> i32;
}

const SIGKILL: i32 = 9;

/// Put the calling process into a process group of its own, led by itself.
///
/// Called between fork and exec, where only async-signal-safe calls are legal:
/// `setpgid` is one and nothing else may join it here.
pub fn own_process_group() -> io::Result<()> {
    if unsafe { setpgid(0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Kill a whole process group, named by the pid of the leader that
/// `own_process_group` made. What a listing forked outlives the listing
/// otherwise, and a descendant holding the inherited pipe is a drain no
/// deadline reaches.
pub fn kill_process_group(leader: u32) {
    if !is_killable_group(leader) {
        return;
    }
    unsafe { killpg(leader as i32, SIGKILL) };
}

/// Whether a group id names a group other than the caller's own.
///
/// `killpg(0, …)` signals the CALLER's group, so a 0 arriving here would SIGKILL
/// this controller and everything it is running. The predicate is separate from
/// the call so an arm can read the refusal without a live `killpg` to observe it
/// by: every caller today passes a spawned child's pid, which is never 0, so
/// pinning the guard by exercising `kill_process_group` would mean killing the
/// process that runs the assertion.
pub fn is_killable_group(leader: u32) -> bool {
    leader != 0
}

// ---- the bounded runner -----------------------------------------------------
//
// ONE RUNNER, TWO COLLECTIONS. The adapter wants the child's own bytes and the
// routines module wants them on disk, and both want the same deadline with the
// same process-group kill behind it. So the spawn, the wait and the kill are
// one path here and the two collections differ only in where the child's
// output goes. The first has a fed twin, for an adapter executable whose
// request goes in on stdin.

/// A bounded run whose output went somewhere this process never read.
#[derive(Debug)]
pub struct Exit {
    pub ok: bool,
    pub code: Option<i32>,
}

/// Why a call that outran its deadline is unreadable. The WHOLE deadline is
/// named: a sub-second one renders as `0s` under a whole-seconds format, which
/// reads as a deadline nobody set.
pub fn deadline_cause(timeout: Duration) -> String {
    format!("did not answer within {timeout:?}")
}

/// Which pipe a drained buffer came off, so the two threads can answer down one
/// channel and still be told apart.
enum Stream {
    Out,
    Err,
}

/// The window the drains are given once nothing here still wants the answer.
/// Two sites: after the group kill on either path, where what the kill
/// reached closes its pipe end at once, and after the child's own exit, where
/// the bytes are already written and only the handover is outstanding. Neither
/// is a wait for an answer.
///
/// The figure has a floor and no ceiling.
/// `a_listing_that_answered_is_read_when_a_descendant_outlives_its_deadline`
/// holds a pipe through a 100 ms in-group descendant, so a grace under that
/// publishes a listing that answered as Unknown; above it the value is a
/// courtesy, because nothing here bounds correctness — it bounds only how long
/// a released drain is left to finish unobserved.
pub const DRAIN_GRACE: Duration = Duration::from_millis(200);

/// How often the wait asks whether the child has exited.
const WAIT_SLICE: Duration = Duration::from_millis(20);

/// Spawn a child leading a process group of its own, with the stdout and
/// stderr the caller has already set and the stdin it names: null wherever
/// nothing is fed, because a bounded child has nobody to answer a prompt, and
/// a pipe where a request is.
fn spawn_in_group(cmd: &mut Command, stdin: Stdio) -> Result<Child, String> {
    cmd.stdin(stdin);
    // Between fork and exec, where only async-signal-safe calls are legal.
    unsafe {
        cmd.pre_exec(own_process_group);
    }
    // The binary and the OS's own reason, both: a spawn fails for a missing
    // path, a file that is not executable and a directory alike, and a cause
    // naming none of them reaches the operator as a seat that is Unknown for no
    // stated reason.
    cmd.spawn().map_err(|e| {
        format!(
            "could not start {}: {e}",
            cmd.get_program().to_string_lossy()
        )
    })
}

/// Wait for the child, or kill its whole group at the deadline. `None` is the
/// deadline's answer, with the child already reaped.
///
/// The reap belongs to the deadline branch: a killed child that is never waited
/// on is a zombie per outrun call. It costs this branch nothing it would have to
/// bound — the child is a direct one the kill above has already reached, so the
/// wait collects a status the kill has already produced — while a child in
/// uninterruptible sleep is outside that, the signal staying pending and this
/// wait having no bound of its own.
fn wait_or_kill(
    child: &mut Child,
    group: u32,
    deadline: Instant,
) -> Result<Option<ExitStatus>, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(e) => return Err(format!("could not wait on the child: {e}")),
        }
        if Instant::now() >= deadline {
            kill_process_group(group);
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(WAIT_SLICE);
    }
}

/// Run a command with a deadline, draining both pipes from their own threads so
/// a large answer cannot deadlock against the wait.
///
/// Two mechanisms, and each bounds what the other cannot. The child leads a
/// process group of its own and the deadline kills the GROUP, so everything the
/// listing forked and left there dies with it. A holder that LEFT the group — a
/// descendant that called `setsid`, which is what a process meaning to outlive
/// its parent does — keeps the pipe through that kill, so every collection is
/// also bounded in time: the remaining deadline or `DRAIN_GRACE`, whichever is
/// longer, while the answer is still wanted, `DRAIN_GRACE` again after the kill,
/// and a drain that has not answered by then is detached rather than joined.
/// Nothing here ever blocks on a pipe without a bound, on either path out of the
/// wait; the worst case is the deadline and two graces.
///
/// The BLAST RADIUS is the group and that is deliberate: a helper the listing
/// forked and left in the group dies at the deadline whether or not it was meant
/// to outlive the listing, because a keeper holding the inherited pipe is
/// indistinguishable from a listing that has not finished. Nothing under
/// `fleet/` says a listing may spawn one, and a process that genuinely means to
/// outlive its parent calls `setsid` and so leaves the group — which is the case
/// the bounded collect below exists for.
///
/// The price of that is a parked thread and its pipe end, per call, for as long
/// as a holder outside the group lives — paid only in that case, and paid
/// because a poll that waits for it is a poll with no deadline at all. Measured
/// under a running controller: two PIPE descriptors per escaped poll, exactly
/// the pair, held for the holder's life and never given back (8 open
/// descriptors of every type then 14 across three escaped polls, twice; and 6
/// then 6 with the same holder kept inside the group, where the kill reaches it
/// and the drain returns).
///
/// A command that outruns the deadline on either path is killed and reported as
/// an error, which reaches the caller as Unknown: a listing that hangs must not
/// hang the poll.
pub fn run_bounded(cmd: Command, timeout: Duration) -> Result<Output, String> {
    collected(cmd, None, timeout)
}

/// [`run_bounded`], with `input` fed to the child's stdin: the same group, the
/// same deadline and the same drains, and one more pipe.
///
/// THE FEED IS A DETACHED THREAD OF ITS OWN, as each drain is. It writes the
/// input and drops the pipe, which is the child's end of file, and a broken
/// pipe is ignored: a child that exits without reading its request has
/// answered, and its exit says how. Written ahead of the wait instead, an
/// input past the pipe's buffer blocks on a child that is itself blocked on a
/// full stdout, and nothing then reaches the deadline.
///
/// The deadline covers the whole call, the feed included: a child that never
/// reads is killed with its group at the deadline, which breaks the pipe the
/// feed is blocked on and lets that thread end.
pub fn run_bounded_fed(cmd: Command, input: Vec<u8>, timeout: Duration) -> Result<Output, String> {
    collected(cmd, Some(input), timeout)
}

/// The one path both piped entry points take, fed or not.
fn collected(mut cmd: Command, feed: Option<Vec<u8>>, timeout: Duration) -> Result<Output, String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let stdin = match feed {
        Some(_) => Stdio::piped(),
        None => Stdio::null(),
    };
    let mut child = spawn_in_group(&mut cmd, stdin)?;
    // The group is named by the leader's pid, which is the child's own.
    let group = child.id();
    if let Some(input) = feed {
        let mut pipe = child.stdin.take().expect("stdin is piped");
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
    let mut out = child.stdout.take().expect("stdout is piped");
    let mut err = child.stderr.take().expect("stderr is piped");
    let (tx, rx) = std::sync::mpsc::channel();
    let out_tx = tx.clone();
    // Both handles are dropped, which detaches: a drain is never joined, because
    // a join is the one wait here that no bound can cut short.
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = out_tx.send((Stream::Out, buf));
    });
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        let _ = tx.send((Stream::Err, buf));
    });
    let deadline = Instant::now() + timeout;
    let Some(status) = wait_or_kill(&mut child, group, deadline)? else {
        // The grace collects the drains the kill just released, so their
        // threads and pipe ends go back on the ordinary path. A drain still
        // held by something outside the group is left parked: that pair is
        // the price, and waiting for it is the deadline not being one.
        let _ = drained_by(&rx, Instant::now() + DRAIN_GRACE);
        return Err(deadline_cause(timeout));
    };
    // The child has exited, so nothing here is waiting for an answer any more:
    // the bytes are written and what is outstanding is the handover. A deadline
    // already spent — the wait sleeps between reads, so an exit inside the
    // deadline is seen after it — would hand the drains a zero window and
    // publish a listing that answered as Unknown naming that deadline.
    let handover = deadline.max(Instant::now() + DRAIN_GRACE);
    // The kill below names the group by a pid the reap above has already given
    // back, so the OS is free to hand it to an unrelated process from that
    // instant. Two outcomes and only one is benign: an empty group has nothing
    // to signal, and a group that took the pid inside that window is SIGKILLed
    // instead. It is kept at that price because the alternative is a drain
    // thread and its pipe end parked for the life of every in-group descendant
    // that outlives its listing, and closing the window needs a wait that
    // observes without reaping, which `try_wait` is not.
    let Some((stdout, stderr)) = drained_by(&rx, handover) else {
        kill_process_group(group);
        let _ = drained_by(&rx, Instant::now() + DRAIN_GRACE);
        return Err(deadline_cause(timeout));
    };
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// The same deadline and the same group kill, with the child's two streams on a
/// FILE instead of a pipe.
///
/// No drain and no grace, because there is no pipe to hold: a descendant that
/// outlives the child keeps the file open and writes into it, which costs this
/// process nothing. The file is truncated at the open, so one log path per call
/// is the caller's to arrange.
pub fn run_bounded_to_file(
    mut cmd: Command,
    log: &Path,
    timeout: Duration,
) -> Result<Exit, String> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let handle = std::fs::File::create(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let second = handle
        .try_clone()
        .map_err(|e| format!("{}: {e}", log.display()))?;
    cmd.stdout(Stdio::from(handle)).stderr(Stdio::from(second));
    let mut child = spawn_in_group(&mut cmd, Stdio::null())?;
    let group = child.id();
    let deadline = Instant::now() + timeout;
    match wait_or_kill(&mut child, group, deadline)? {
        Some(status) => Ok(Exit {
            ok: status.success(),
            code: status.code(),
        }),
        None => Err(deadline_cause(timeout)),
    }
}

/// Both drained buffers, or `None` when the deadline passed with one still
/// unread. A stream whose thread ended without sending answers empty rather than
/// blocking the other one.
fn drained_by(
    rx: &std::sync::mpsc::Receiver<(Stream, Vec<u8>)>,
    deadline: Instant,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let (mut out, mut err) = (None, None);
    for _ in 0..2 {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((Stream::Out, buf)) => out = Some(buf),
            Ok((Stream::Err, buf)) => err = Some(buf),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return None,
        }
    }
    Some((out.unwrap_or_default(), err.unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The group id the kill refuses. 0 is the caller's own group in `killpg`,
    /// so the refusal is what keeps a future caller from SIGKILLing this
    /// controller; a spawned child's pid, which is what every caller passes
    /// today, is killable.
    #[test]
    fn the_kill_refuses_the_callers_own_group_and_no_other() {
        assert!(!is_killable_group(0), "0 is killpg's own-group id");
        assert!(is_killable_group(1));
        assert!(is_killable_group(u32::MAX));

        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("a child spawns");
        assert!(
            is_killable_group(child.id()),
            "a spawned child's pid is the id every caller passes"
        );
        let _ = child.wait();
    }

    /// `drained_by`'s three answers, one of which its callers rely on and no
    /// arm and no production path reaches: a sender that dropped without
    /// sending, which happens when a drain thread ends before its `send`.
    ///
    /// The channel is driven by hand rather than through `run_bounded`, because
    /// the two drains there always send and the disconnect is what a panicking
    /// one would leave behind.
    #[test]
    fn a_drain_answers_both_buffers_a_timeout_or_a_sender_that_dropped() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send((Stream::Out, b"out".to_vec())).expect("out sends");
        tx.send((Stream::Err, b"err".to_vec())).expect("err sends");
        let both = drained_by(&rx, Instant::now() + Duration::from_secs(5))
            .expect("both buffers are in the channel");
        assert_eq!(both, (b"out".to_vec(), b"err".to_vec()));

        // One unread at the deadline is None — the caller's signal to kill and
        // report. The sender is held to the end of the arm, so this is a
        // timeout and not the disconnect below wearing its name.
        let (held, rx) = std::sync::mpsc::channel();
        held.send((Stream::Out, b"out".to_vec()))
            .expect("out sends");
        assert!(
            drained_by(&rx, Instant::now() + Duration::from_millis(50)).is_none(),
            "one stream still unread at the deadline is no answer"
        );

        // A stream whose thread ended without sending: the answer is the other
        // buffer and an empty one beside it, taken at once rather than waited
        // for. The five-second deadline is the witness — a disconnect read as a
        // timeout would spend it.
        let (gone, rx) = std::sync::mpsc::channel();
        gone.send((Stream::Err, b"err".to_vec()))
            .expect("err sends");
        drop(gone);
        let started = Instant::now();
        let (out, err) = drained_by(&rx, Instant::now() + Duration::from_secs(5))
            .expect("a dropped sender answers rather than blocking the other stream");
        assert!(out.is_empty(), "the stream that never sent reads empty");
        assert_eq!(err, b"err".to_vec());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the disconnect answers at once: {:?}",
            started.elapsed()
        );
        drop(held);
    }

    /// A listing that answered inside its deadline is READ, not published as
    /// Unknown naming that deadline.
    ///
    /// The wait sleeps between reads, so an exit that lands inside the deadline
    /// is seen after it, and an in-group descendant holding the inherited pipe
    /// keeps the drains from delivering at the moment of that read. Handing them
    /// whatever is left of the deadline there is a window that can be zero.
    ///
    /// The descendant's 100 ms is the floor `DRAIN_GRACE` is read against: the
    /// stub answers at once and holds the pipe for that long, so this arm is
    /// what a grace shorter than the hold would red.
    #[test]
    fn a_listing_that_answered_is_read_when_a_descendant_outlives_its_deadline() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "( sleep 0.1 ) & echo answered"]);
        let run = run_bounded(cmd, Duration::from_millis(60))
            .expect("the listing answered inside its deadline and is read");
        assert!(run.status.success(), "the child exited 0");
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "answered");
    }

    /// The fed runner hands the child its input whole, and the child's answer
    /// comes back whole: a MiB is past any pipe's buffer both ways, so the
    /// feed has to run beside the drains — a feed written ahead of the wait
    /// blocks on a child that is itself blocked on a full stdout.
    #[test]
    fn a_fed_run_hands_the_child_its_input_whole() {
        let input: Vec<u8> = (0..1usize << 20).map(|n| (n % 251) as u8).collect();
        let run = run_bounded_fed(Command::new("cat"), input.clone(), Duration::from_secs(30))
            .expect("cat answered inside its deadline");
        assert!(run.status.success(), "cat exited 0");
        assert_eq!(run.stdout.len(), input.len(), "every byte fed came back");
        assert!(run.stdout == input, "and in the order it was fed");
    }

    /// The cause names the deadline it ran on. Under a sub-second one a whole-
    /// seconds format renders "0s", which reads as a deadline nobody set.
    #[test]
    fn the_deadline_cause_names_a_sub_second_deadline_as_itself() {
        let cause = deadline_cause(Duration::from_millis(200));
        assert!(
            cause.contains("did not answer within 200ms"),
            "a sub-second deadline is named in full: {cause}"
        );
        assert!(
            !cause.contains("0s"),
            "and never rounded to a deadline of zero: {cause}"
        );
    }

    /// The second collection: the same deadline and the same group kill, with
    /// the bytes on a file. The three readings are the exit, the file, and the
    /// deadline's own refusal — and the last one leaves what the child had
    /// already written where a person can read it.
    #[test]
    fn a_bounded_run_to_a_file_carries_its_exit_and_its_deadline_alike() {
        let dir = std::env::temp_dir().join(format!("fleet-bounded-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = dir.join("logs").join("one.log");

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 0"]);
        let ran = run_bounded_to_file(cmd, &log, Duration::from_secs(5)).expect("the child ran");
        assert!(ran.ok);
        assert_eq!(ran.code, Some(0));
        let body = std::fs::read_to_string(&log).expect("the log is written");
        assert!(body.contains("out") && body.contains("err"), "{body:?}");

        // A non-zero exit is an answer and not an error: the caller reads the
        // code and names the log.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "exit 7"]);
        let failed = run_bounded_to_file(cmd, &log, Duration::from_secs(5)).expect("the child ran");
        assert!(!failed.ok);
        assert_eq!(failed.code, Some(7));

        // The deadline: a child that outruns it is killed and reported, and the
        // line it wrote first is still on the file.
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo started; sleep 5"]);
        let outran = run_bounded_to_file(cmd, &log, Duration::from_millis(200))
            .expect_err("the child outran its deadline");
        assert!(outran.contains("did not answer within"), "{outran}");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            "started",
            "what the child wrote before the kill is still readable"
        );

        // A binary that is not there is a start failure, which names it.
        let missing = run_bounded_to_file(
            Command::new(dir.join("nothing-here")),
            &log,
            Duration::from_secs(5),
        )
        .expect_err("a missing binary cannot start");
        assert!(missing.contains("could not start"), "{missing}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
