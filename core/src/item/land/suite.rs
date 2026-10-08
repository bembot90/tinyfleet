//! The suite check and its one rerun, and the two children a landing runs: the marker command and the suite.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::rows::Rows;
use super::{rc_word, Landing, Wiring, NOT_TESTED, SUITE_RERUN_ROW, UNTESTED};
use crate::item::{lane, Progress, Stop, CHECK_READ};
use crate::seat::actor::Actor;

/// How often the suite's log is re-read while the suite runs. The verb has no
/// deadline of its own; this is only how often the bar's message can change.
const TICK: std::time::Duration = std::time::Duration::from_millis(200);

/// How much of a red suite's log the row prints.
const TAIL: usize = 20;

// ---- the suite check and its one rerun ---------------------------------------

/// The reading a landing's own first suite run is.
pub(super) const FIRST_READING: u64 = 1;
/// The reading the rerun is. There is no third: a second red parks.
const SECOND_READING: u64 = 2;

/// The three words a `check.read` carries under `verdict`, which are the three
/// the landed entry's own suite rows carry. `RED` reaches the stream only on a
/// reading the landing did NOT stand on — a first red that a green rerun
/// followed, or the pair a second red refuses with.
const GREEN: &str = "green";
const RED: &str = "red";
pub(super) const NO_READING: &str = "none";

/// One reading of the suite check: which it is, what the child exited, where
/// its log is, and what the wait before it did.
pub(super) struct Reading {
    n: u64,
    pub(super) rc: Option<i32>,
    took: String,
    log: PathBuf,
    /// What the rerun's wait for a quiet box ended as. `None` on the first
    /// reading, which waits for nothing.
    waited: Option<String>,
    /// The `PATH` this reading's child ran under. Empty where the caller
    /// constructed none and the child inherited this process's.
    path: String,
}

impl Reading {
    fn green(&self) -> bool {
        self.rc == Some(0)
    }

    fn verdict(&self) -> &'static str {
        if self.green() {
            GREEN
        } else {
            RED
        }
    }

    /// The row's evidence, which is also what the landed entry carries.
    fn evidence(&self, command: &str) -> String {
        let head = format!(
            "`{command}` rc {} in {}, read from the child's own exit; log {}",
            rc_word(self.rc),
            self.took,
            self.log.display()
        );
        match &self.waited {
            None => head,
            Some(waited) => format!("{head}; reading {} — {waited}", self.n),
        }
    }

    pub(super) fn payload(&self, item: &str, command: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "item": item,
            "suite": command,
            "rc": self.rc,
            "verdict": self.verdict(),
            "reading": self.n,
            "log": self.log.display().to_string(),
            // A suite that failed on the diff and one that failed because it
            // could not find its tools are told apart here and nowhere else.
            "path": if self.path.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(self.path.clone())
            },
        })
    }
}

/// The command the landing was handed, run once, and once more where the first
/// read red.
///
/// THE RERUN IS UNCONDITIONAL, and that is Alberto's ruling and not this
/// module's economy: the rerun is owed to an arm the diff did not reach, and
/// the command is one opaque line that reports no arms, so the condition has
/// nothing to read. Both readings go on the record either way, which is what
/// lets a person answer the question the condition would have.
///
/// A SECOND RED REFUSES WITH BOTH LOGS ON STDOUT. That is the whole channel the
/// park needs: the flight's landing act carries what this printed into the
/// hold's question, so the person who meets it reads both tails without this
/// verb knowing a flight exists.
#[allow(clippy::too_many_arguments)]
pub(super) fn suite_check(
    out: &mut dyn Write,
    rows: &mut Rows,
    command: &Option<String>,
    work_dir: &Path,
    landing: &Landing,
    closer: &Actor,
    wiring: &Wiring,
) -> Result<Vec<Reading>, Stop> {
    let Some(command) = command else {
        rows.read(out, wiring, NOT_TESTED, UNTESTED);
        return Ok(Vec::new());
    };

    let first = read_once(command, work_dir, FIRST_READING, None, wiring)?;
    rows.read(
        out,
        wiring,
        if first.green() { "PASS" } else { "RED" },
        first.evidence(command),
    );
    if first.green() {
        return Ok(vec![first]);
    }

    // THE WAIT, then the rerun. A suite that raced the box is rerun beside a
    // quieter one; on a box that never quietens the wait expires and the rerun
    // runs anyway, saying so on its own row.
    let waited = lane::wait_for_a_quiet_box(
        wiring.load,
        lane::rerun_wait(&wiring.project.guards)?,
        wiring.progress,
    );
    let second = read_once(
        command,
        work_dir,
        SECOND_READING,
        Some(waited.clause()),
        wiring,
    )?;
    rows.read_named(
        out,
        wiring,
        SUITE_RERUN_ROW,
        if second.green() { "PASS" } else { "RED" },
        second.evidence(command),
    );
    if second.green() {
        return Ok(vec![first, second]);
    }

    // BOTH READINGS REACH THE STREAM BEFORE THE REFUSAL, and this is the one
    // place this verb writes an event on a path that lands nothing: both
    // readings are owed to the record, and a second red never gets as far as the
    // entry the other events wait behind.
    for reading in [&first, &second] {
        announce(
            landing.item,
            CHECK_READ,
            closer,
            wiring,
            reading.payload(landing.item, Some(command)),
        )?;
    }
    for reading in [&first, &second] {
        let _ = writeln!(out, "reading {} — {}", reading.n, reading.log.display());
        for line in tail(&reading.log) {
            let _ = writeln!(out, "  {line}");
        }
    }
    Err(Stop::refused(format!(
        "the suite `{command}` exited {} and, rerun once, {} — the logs are at {} and {}",
        rc_word(first.rc),
        rc_word(second.rc),
        first.log.display(),
        second.log.display()
    )))
}

/// One run of the suite check, into its own log. The second reading's log sits beside
/// the first rather than over it: a reader comparing two reds needs both.
fn read_once(
    command: &str,
    work_dir: &Path,
    n: u64,
    waited: Option<String>,
    wiring: &Wiring,
) -> Result<Reading, Stop> {
    let log = work_dir.join(if n == FIRST_READING {
        "suite.log".to_string()
    } else {
        format!("suite.{n}.log")
    });
    let run = suite(
        command,
        &wiring.project.root,
        wiring.child_path,
        &log,
        wiring.progress,
    )?;
    Ok(Reading {
        n,
        rc: run.code,
        took: run.took,
        log,
        waited,
        path: wiring.child_path.to_string(),
    })
}

/// One event this verb writes. The landing is on the trunk and its entry is on
/// the item whatever this says, which is why the failure names both rather than
/// reading as a landing that did not happen.
pub(super) fn announce(
    item: &str,
    kind: &str,
    closer: &Actor,
    wiring: &Wiring,
    payload: serde_json::Value,
) -> Result<(), Stop> {
    wiring.events.append(kind, closer, payload).map_err(|e| {
        Stop::could_not_tell(format!(
            "{kind} did not reach the stream: {e}\n  the landing on {item} STANDS and its landed \
             entry is on the record"
        ))
    })
}

// ---- the two children ---------------------------------------------------------

/// A command in the project root, with these lines on its stdin. Its trimmed
/// stdout is the answer; a non-zero exit is an instrument that would not answer.
pub(super) fn stdin_command(
    command: &str,
    root: &Path,
    child_path: &str,
    lines: &[String],
) -> Result<String, Stop> {
    let mut child = shell(command, root, child_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Stop::could_not_tell(format!("`{command}` could not be run: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut body = lines.join("\n");
        body.push('\n');
        let _ = stdin.write_all(body.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| Stop::could_not_tell(format!("`{command}` could not be read: {e}")))?;
    if !out.status.success() {
        return Err(Stop::could_not_tell(format!(
            "`{command}` exited {}: {}",
            rc_word(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What the suite did.
struct Ran {
    code: Option<i32>,
    took: String,
}

/// The suite as a child of this act, with no deadline of the verb's own: the
/// project decides how long its own suite takes. Its rc is read from the child's
/// own exit and never from anything it printed.
fn suite(
    command: &str,
    root: &Path,
    child_path: &str,
    log: &Path,
    progress: &dyn Progress,
) -> Result<Ran, Stop> {
    let file = std::fs::File::create(log).map_err(|e| {
        Stop::could_not_tell(format!(
            "the suite's log at {} could not be opened: {e}",
            log.display()
        ))
    })?;
    let both = file.try_clone().map_err(|e| {
        Stop::could_not_tell(format!(
            "the suite's log could not be shared with stderr: {e}"
        ))
    })?;
    let started = std::time::Instant::now();
    let mut child = shell(command, root, child_path)
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(both))
        .spawn()
        .map_err(|e| Stop::could_not_tell(format!("`{command}` could not be run: {e}")))?;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                progress.message(&format!("{command} — {} line(s)", lines_in(log)));
                std::thread::sleep(TICK);
            }
            Err(e) => {
                return Err(Stop::could_not_tell(format!(
                    "`{command}` could not be waited on: {e}"
                )))
            }
        }
    };
    Ok(Ran {
        code: status.code(),
        took: format!("{:.1}s", started.elapsed().as_secs_f64()),
    })
}

fn shell(command: &str, root: &Path, child_path: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(root);
    if !child_path.is_empty() {
        cmd.env("PATH", child_path);
    }
    cmd
}

fn lines_in(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

fn tail(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL)..]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}
