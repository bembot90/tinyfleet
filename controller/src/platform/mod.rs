//! The platform layer: everything the operating system supplies.
//!
//! No operating-system name appears anywhere outside this module — the two
//! implementations below are the only place a `cfg` on the target may be
//! written, so a third concern that differs is added here rather than branched
//! at the call site.
//!
//! Six concerns live here: the machine directory, the process read, the load
//! average, the constructed child PATH, the SERVICE MANAGER that writes, loads,
//! unloads and queries this machine's user service, and the PERMISSION GATE
//! that probes the worktrees before the loop acts.
//!
//! The process group and the bounded runner are not here: they are written in
//! `fleet_core::process`, because the store bounds its own calls with them, and
//! every caller imports them from there. The atomic write (`fleet_core::fs`)
//! and the resolver that reads a search path (`fleet_core::process`) are core's
//! too: neither differs by platform, and core's own callers need both.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use fleet_core::fs::write_atomic;
use fleet_core::process::{is_executable_file, resolve_on_path, run_bounded};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as sys;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as sys;

// The OTHER platform's half, compiled FOR THE ARMS ALONE. The service file is a
// pure writer from four arguments to bytes, and a text only its own platform can
// compile is a text nobody reads until that platform builds — which is the
// second half of the table above going unwatched. Nothing here is wired to
// `sys`: the selection above is still the one `cfg` on the target.
#[cfg(all(test, target_os = "macos"))]
#[allow(dead_code)]
#[path = "linux.rs"]
mod linux;
#[cfg(all(test, target_os = "linux"))]
#[allow(dead_code)]
#[path = "macos.rs"]
mod macos;

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("the controller targets macOS and Linux; Windows is not a target");

/// The fleet directory for this machine.
///
/// One resolution order, everywhere: `FLEET_DIR` is the directory itself and
/// wins outright; `FLEET_HOME` is the home it sits under; otherwise the
/// platform's own answer under the user's home.
pub fn machine_dir() -> PathBuf {
    resolve_machine_dir(
        env_dir("FLEET_DIR").as_deref(),
        env_dir("FLEET_HOME").as_deref(),
        env_dir("HOME").as_deref(),
        env_dir("XDG_STATE_HOME").as_deref(),
    )
}

/// The resolution itself, with every input passed in: the environment is read
/// once, above, so the order can be tested without touching it.
pub fn resolve_machine_dir(
    fleet_dir: Option<&Path>,
    fleet_home: Option<&Path>,
    home: Option<&Path>,
    xdg_state: Option<&Path>,
) -> PathBuf {
    if let Some(dir) = fleet_dir {
        return dir.to_path_buf();
    }
    sys::machine_dir_under(fleet_home.or(home).unwrap_or(Path::new("")), xdg_state)
}

/// The user's home, which the constructed `PATH` is built off.
pub fn home_dir() -> PathBuf {
    env_dir("HOME").unwrap_or_default()
}

fn env_dir(key: &str) -> Option<PathBuf> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Some(PathBuf::from(v)),
        _ => None,
    }
}

/// The `PATH` every child of this controller carries.
///
/// CONSTRUCTED, never inherited. A service-launched process carries a minimal
/// search path that holds neither a package manager's prefix nor the user's
/// local bin, and a session started under it carries a `PATH` that collapses
/// mid-run (lessons claude-code D1). So this is built from the
/// platform's own list and the home passed in, and the process's own `PATH`
/// contributes nothing to it.
pub fn child_path(home: &Path) -> String {
    let mut seen = std::collections::HashSet::new();
    let kept: Vec<PathBuf> = sys::child_path_dirs(home)
        .into_iter()
        .filter(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()))
        .collect();
    std::env::join_paths(&kept)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| {
            kept.iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(":")
        })
}

/// The environment a seat's session keeps from this process, beside the
/// constructed `PATH`: four values a shell needs to be one, and nothing else.
/// A variable this list does not name cannot reach a session through this
/// controller.
///
/// `FLEET_BIN` is not here, and passing it through would be wrong twice over: a
/// controller started by a service manager has none to pass, and one started
/// from inside a seat would hand on that seat's binary rather than its own. It
/// is set from this process's own executable instead.
///
/// `FLEET_ACTOR` is not here for the second of those reasons: a controller
/// started from inside a seat would make every session it starts that seat.
/// A start and a resume set it from the seat they are for.
pub const PASSED_THROUGH: [&str; 4] = ["HOME", "USER", "TMPDIR", "LANG"];

/// Set, a seam naming no binary is a refusal rather than a fall back to one on
/// `PATH`: what a suite puts on every process it drives, so an arm that missed
/// its stub never reaches the operator's own programs.
pub const HERMETIC_VAR: &str = "FLEET_TEST_HERMETIC";

/// Whether [`HERMETIC_VAR`] is set to anything but blank or `0`.
pub fn hermetic() -> bool {
    match std::env::var(HERMETIC_VAR) {
        Ok(value) => !matches!(value.trim(), "" | "0"),
        Err(_) => false,
    }
}

/// The suffix a document's lock file carries.
pub const LOCK_SUFFIX: &str = ".lock";

/// The lock that guards a read-modify-write of `path`, held for as long as the
/// returned handle lives.
///
/// BESIDE THE DOCUMENT, never on it: [`fleet_core::fs::write_atomic`] renames a
/// temp file over the path, so a lock taken on the destination is released by
/// the rename that replaced the inode under it. The lock file is CREATED AND
/// LEFT — unlinking it lets a second process hold a descriptor on an inode
/// nobody else can reach, whose lock then guards nothing.
///
/// It blocks with no deadline, so the hold IS the wait every other caller
/// spends: what a caller may hold it across is the read, the edit and the
/// rename it guards, and nothing slower. A child process, an agent start or a
/// watch window inside the critical section makes one verb block every other
/// for that whole window, and this layer has no basis to pick a timeout that
/// would cap it.
///
/// Two callers widen it on purpose and each says so where it takes it:
/// [`crate::config::claim_transient_seat`], whose subject is the SEAT and which
/// must hold the lock across the `make` that cuts the worktree, so the row is
/// never written for a tree that was not made, and
/// [`crate::transient::feed`], which holds the occupant marker's move and its
/// put-back together so a second feed cannot land between them.
pub fn lock_beside(path: &Path) -> Result<std::fs::File, String> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(LOCK_SUFFIX);
    let lock = path.with_file_name(name);
    if let Some(dir) = lock.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let handle = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock)
        .map_err(|e| format!("{}: {e}", lock.display()))?;
    handle
        .lock()
        .map_err(|e| format!("{}: {e}", lock.display()))?;
    Ok(handle)
}

/// Whether a pid is a live process — `None` when the platform could not tell,
/// which is a third answer and never rounded into "gone".
pub fn process_alive(pid: u32) -> Option<bool> {
    sys::process_alive(pid)
}

/// Which of `getloadavg`'s three samples — 1, 5, 15 minutes — the load belt is
/// judged on: the FIVE-MINUTE one.
///
/// A test suite puts this box at load 14 to 22 against a ceiling of one per cpu,
/// and the one-minute sample makes that burst indistinguishable from a jammed
/// box, so the belt refuses every dispatch for the suite's whole duration.
pub const LOAD_SAMPLE: usize = 1;

/// The sample the belt judges, out of as many as the platform filled.
///
/// A platform that filled fewer than that answers `None`, which is a reading
/// nobody has rather than a machine under no load.
pub fn belt_sample(filled: &[f64]) -> Option<f64> {
    filled.get(LOAD_SAMPLE).copied()
}

extern "C" {
    fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
}

/// The five-minute load average, or `None` when this platform would not answer.
///
/// `None` is a leg of the load belt that cannot be judged, never a machine under
/// no load: a reading nobody took must not read as room to start on.
///
/// The C library's `getloadavg` on both platforms. On Linux it is the C call
/// rather than `/proc/loadavg`, unlike that platform's process read: both libc
/// implementations it ships read that file themselves, so parsing it here would
/// be a second parser for the same bytes. Which of the three samples is the
/// belt's is [`LOAD_SAMPLE`]'s to say, so the two platforms cannot read
/// different minutes.
pub fn load_average_5m() -> Option<f64> {
    let mut samples = [0.0f64; 3];
    // SAFETY: the pointer is to an array of three, and 3 is the count passed.
    let filled = unsafe { getloadavg(samples.as_mut_ptr(), 3) };
    belt_sample(&samples[..filled.clamp(0, 3) as usize])
}

/// How many processors the load average is read against. `None` when the
/// standard library cannot say, which makes the same leg unjudgeable.
pub fn cpus() -> Option<u32> {
    std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u32)
}

static STOP: AtomicBool = AtomicBool::new(false);

/// SIGTERM and SIGINT both mean "the operator is ending this controller", and
/// the loop owes a `controller.stopped` event to either.
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

extern "C" {
    fn signal(signum: i32, handler: usize) -> usize;
}

extern "C" fn raise_stop(_sig: i32) {
    STOP.store(true, Ordering::SeqCst);
}

/// Arm the stop flag. Nothing but the flag is touched from the handler, because
/// the loop is what owns the shutdown and the event it owes.
pub fn install_stop_handler() {
    unsafe {
        signal(SIGTERM, raise_stop as usize);
        signal(SIGINT, raise_stop as usize);
    }
}

pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// Raise the stop flag from inside the process, which is how a suite driving the
/// loop in-process ends a run of ticks: the loop leaves on the flag alone and
/// the only other writer is a signal, which a test binary cannot raise at one
/// arm without raising it at every thread beside it.
///
/// The flag is process-wide, so an arm that raises it lowers it again with
/// [`clear_stop`] before the next one reads it.
#[cfg(any(test, feature = "test-support"))]
pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

#[cfg(any(test, feature = "test-support"))]
pub fn clear_stop() {
    STOP.store(false, Ordering::SeqCst);
}

// ---- the service manager ----------------------------------------------------
//
// ONE LABEL, TWO MANAGERS. The file's text, the argv each verb runs and how a
// pid is read back out differ per platform and live in the two `sys` modules;
// the label, the resolution of the binary, the write and the three verbs are
// one path here.

/// The label this machine's user service is registered under. One string on
/// both platforms, carrying the fleet's own name and no project's.
pub const SERVICE_LABEL: &str = "dev.fleet.controller";

/// The variable that puts a recording stub in place of the platform's service
/// binary, in the shape `FLEET_BIN` already has: an ABSOLUTE path, never
/// a name this layer would then have to search for.
pub const SERVICE_BIN_ENV: &str = "FLEET_SERVICE_BIN";

/// How long one service command is given. A load or a query is a local call to
/// the platform's own manager, so a minute is a deadline nothing healthy meets.
const SERVICE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the after-write reading is given. It is a local call to the
/// platform's own tool — a file linter on one, the login manager on the other —
/// so a deadline it ever meets is a tool that is not answering.
const ASIDE_TIMEOUT: Duration = Duration::from_secs(20);

/// Where the service file goes and what it says.
///
/// PURE: every input is an argument, so the text each platform writes is read
/// by an arm on a box that would never load it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceFile {
    pub path: PathBuf,
    pub text: String,
}

/// The manager for this machine's user service.
pub struct Service {
    file: ServiceFile,
    bin: PathBuf,
    child_path: String,
}

impl Service {
    /// Resolve the platform's service binary and compose the file this fleet's
    /// service would be.
    ///
    /// The binary is resolved on the CONSTRUCTED child path and never taken by
    /// bare name, which is the same rule the effect binary is under: a service
    /// -launched process carries a search path that is not the operator's
    /// shell's. `bin_override` is the seam, and it is an absolute path.
    pub fn resolve(
        home: &Path,
        machine_dir: &Path,
        exe: &Path,
        fleet_dir: Option<&Path>,
        bin_override: Option<&str>,
    ) -> Result<Service, String> {
        let child_path = child_path(home);
        let named = bin_override.map(str::trim).filter(|v| !v.is_empty());
        let bin = match named {
            Some(named) => {
                let path = PathBuf::from(named);
                if !is_executable_file(&path) {
                    return Err(format!(
                        "{SERVICE_BIN_ENV} names {named}, which is not a file this process can \
                         execute"
                    ));
                }
                path
            }
            None => resolve_on_path(&child_path, sys::SERVICE_BIN).ok_or_else(|| {
                format!(
                    "`{}` is not on the constructed search path {child_path}",
                    sys::SERVICE_BIN
                )
            })?,
        };
        Ok(Service {
            file: sys::service_file(home, SERVICE_LABEL, exe, machine_dir, fleet_dir),
            bin,
            child_path,
        })
    }

    pub fn label(&self) -> &str {
        SERVICE_LABEL
    }

    /// Where a person reads what the service itself printed. A path on one
    /// platform and a command on the other, so a caller names it without
    /// knowing which.
    pub fn stderr_of(machine_dir: &Path) -> String {
        sys::service_stderr(machine_dir)
    }

    pub fn file(&self) -> &Path {
        &self.file.path
    }

    /// Write the file, and answer whether it CHANGED. A second run writes
    /// nothing and says so, which is what makes the first-run work idempotent
    /// rather than merely repeatable.
    pub fn write(&self) -> Result<bool, String> {
        if std::fs::read_to_string(&self.file.path).ok().as_deref() == Some(self.file.text.as_str())
        {
            return Ok(false);
        }
        write_atomic(&self.file.path, self.file.text.as_bytes())
            .map_err(|e| format!("{}: {e}", self.file.path.display()))?;
        Ok(true)
    }

    /// What this platform read about the file it was just handed, in words a
    /// caller prints without knowing which platform wrote them.
    pub fn after_write(&self) -> Vec<String> {
        sys::after_write(&self.child_path, &self.file.path)
    }

    /// Load the service. NOTHING ELSE HERE STARTS IT: the write above is the
    /// install and this is the second deliberate act.
    pub fn load(&self) -> Result<(), String> {
        for argv in sys::load_argv(SERVICE_LABEL, &self.file.path) {
            self.run(&argv)?;
        }
        Ok(())
    }

    pub fn unload(&self) -> Result<(), String> {
        self.run(&sys::unload_argv(SERVICE_LABEL))
    }

    /// The pid the manager reports for the label, or `None` when it reports
    /// none — which is a service that is not loaded rather than one this layer
    /// could not read. An `Err` is the third answer.
    pub fn running(&self) -> Result<Option<u32>, String> {
        let argv = sys::running_argv(SERVICE_LABEL);
        let mut command = Command::new(&self.bin);
        command.args(&argv).env("PATH", &self.child_path);
        match run_bounded(command, SERVICE_TIMEOUT) {
            // A manager that does not know the label answers non-zero, which is
            // "not loaded" and not a fault.
            Ok(run) if !run.status.success() => Ok(None),
            Ok(run) => Ok(sys::pid_in(&String::from_utf8_lossy(&run.stdout))),
            Err(why) => Err(format!("{} {}: {why}", self.bin.display(), argv.join(" "))),
        }
    }

    fn run(&self, argv: &[String]) -> Result<(), String> {
        let mut command = Command::new(&self.bin);
        command.args(argv).env("PATH", &self.child_path);
        match run_bounded(command, SERVICE_TIMEOUT) {
            Ok(run) if run.status.success() => Ok(()),
            Ok(run) => Err(format!(
                "{} {} exited {}: {}",
                self.bin.display(),
                argv.join(" "),
                run.status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or("on a signal".into()),
                String::from_utf8_lossy(&run.stderr).trim()
            )),
            Err(why) => Err(format!("{} {}: {why}", self.bin.display(), argv.join(" "))),
        }
    }
}

// ---- the permission gate ----------------------------------------------------
//
// THE DIALOG DOES NOT REFUSE, IT BLOCKS (lessons claude-code D4). Whether a
// guarded read under it returns a permission error or simply hangs was never
// measured, so the gate is built so the answer does not matter: a timeout counts
// as PENDING exactly as a refusal does, and the detail says which one was seen.
//
// A probe that outran the bound is PARKED and read again next poll rather than
// started again, or a blocked dialog accumulates one parked thread per poll per
// seat. That is why the bound is this layer's — a runner that killed the child
// at the deadline would leave nothing to answer when the person finally does.

pub const GRANT_OK: &str = "ok";
pub const GRANT_PENDING: &str = "pending";

/// The band one probe runs on: the top of a band from one incident — a read
/// that exceeded 1.5 s and never returned, and a recurrence under load that
/// cleared in 5 s — and not a controlled percentile (D4).
pub const GRANT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// One worktree's listing, as the gate is handed it.
pub type Listing = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// Whether this platform puts a file-access dialog in front of a directory read.
/// A reading, so an arm asserts what holds on the box it runs on rather than
/// returning without measuring.
pub fn grant_is_gated() -> bool {
    sys::GRANT_IS_GATED
}

/// The listing the loop probes with: the directory read to its end, which is
/// what the host's permission layer stands in front of.
///
/// A DIRECTORY THAT IS NOT THERE ANSWERS OK. The gate asks one question — is
/// there a dialog standing between this process and this directory — and an
/// absent path is not a question a person can answer: the worktrees a seat's
/// row names are the person's to create, so a fleet whose seats have no
/// worktrees yet is the ordinary first-run state and not a fleet held with
/// every effect off. What the permission layer returns is a denial or a wait,
/// never a missing file.
pub fn directory_listing() -> Listing {
    Arc::new(|path: &Path| match std::fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries {
                entry.map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    })
}

/// What the gate read this poll.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantRead {
    pub state: &'static str,
    /// Present exactly while the state is pending, naming the path and which of
    /// the two answers it was.
    pub detail: Option<String>,
}

impl GrantRead {
    pub fn is_ok(&self) -> bool {
        self.state == GRANT_OK
    }
}

enum Probe {
    Answered,
    /// The probe outran the bound and the call has still not returned.
    Parked(Receiver<Result<(), String>>),
    /// The probe ANSWERED, with an error. Nothing is outstanding, so the path is
    /// probed again next poll and the detail is the fresh probe's rather than
    /// this one's — which is why the reason is not carried here.
    Refused,
}

/// The gate, across polls.
pub struct Grant {
    probed: bool,
    timeout: Duration,
    listing: Listing,
    probes: BTreeMap<PathBuf, Probe>,
}

impl Grant {
    /// The gate this platform has. On a platform with no file-access dialog it
    /// reads `ok` and probes nothing, which is a reading and not a skip.
    pub fn new(listing: Listing, timeout: Duration) -> Grant {
        Grant {
            probed: sys::GRANT_IS_GATED,
            timeout,
            listing,
            probes: BTreeMap::new(),
        }
    }

    /// Probe every path that has not answered, read every parked probe without
    /// starting a second one for it, and report the fleet's grant.
    pub fn poll(&mut self, paths: &[PathBuf]) -> GrantRead {
        if !self.probed {
            return GrantRead {
                state: GRANT_OK,
                detail: None,
            };
        }
        let mut pending: Option<String> = None;
        for path in paths {
            let detail = self.probe(path);
            if pending.is_none() {
                pending = detail;
            }
        }
        match pending {
            Some(detail) => GrantRead {
                state: GRANT_PENDING,
                detail: Some(detail),
            },
            None => GrantRead {
                state: GRANT_OK,
                detail: None,
            },
        }
    }

    /// One path's answer: `None` once it has answered, and the detail while it
    /// has not.
    fn probe(&mut self, path: &Path) -> Option<String> {
        match self.probes.get(path) {
            Some(Probe::Answered) => return None,
            Some(Probe::Parked(rx)) => {
                match rx.try_recv() {
                    Ok(answer) => return self.settle(path, Some(answer)),
                    // Still outstanding: the dialog has not been answered, and a
                    // second probe would be one more parked thread.
                    Err(TryRecvError::Empty) => return Some(timed_out_detail(path, self.timeout)),
                    Err(TryRecvError::Disconnected) => return self.settle(path, None),
                }
            }
            // A refusal answered, so it is probed again; a path nobody has
            // probed is probed for the first time.
            Some(Probe::Refused) | None => {}
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let listing = Arc::clone(&self.listing);
        let probed = path.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(listing(&probed));
        });
        match rx.recv_timeout(self.timeout) {
            Ok(answer) => self.settle(path, Some(answer)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                self.probes.insert(path.to_path_buf(), Probe::Parked(rx));
                Some(timed_out_detail(path, self.timeout))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => self.settle(path, None),
        }
    }

    /// Record a probe that has settled and answer its detail: `None` for an
    /// answer, the refusal for an error, and [`PROBE_VANISHED`] when the sender
    /// is gone (`answer` is `None`).
    fn settle(&mut self, path: &Path, answer: Option<Result<(), String>>) -> Option<String> {
        match answer {
            Some(Ok(())) => {
                self.probes.insert(path.to_path_buf(), Probe::Answered);
                None
            }
            Some(Err(why)) => {
                self.probes.insert(path.to_path_buf(), Probe::Refused);
                Some(refused_detail(path, &why))
            }
            None => {
                self.probes.insert(path.to_path_buf(), Probe::Refused);
                Some(refused_detail(path, PROBE_VANISHED))
            }
        }
    }
}

/// A probe whose thread ended without sending: nothing answered, so it is
/// pending with a reason rather than an ok nobody measured.
const PROBE_VANISHED: &str = "the probe ended without answering";

fn timed_out_detail(path: &Path, timeout: Duration) -> String {
    format!(
        "the listing of {} has not answered within {timeout:?} and is still outstanding",
        path.display()
    )
}

fn refused_detail(path: &Path, why: &str) -> String {
    format!("the listing of {} came back refused: {why}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    // Declared here because it is POSIX, identical on both targets, and no
    // crate is added to ask a one-word question. Only the arm whose denial uid
    // 0 ignores reads it.
    extern "C" {
        fn geteuid() -> u32;
    }

    #[test]
    fn the_machine_directory_resolves_in_one_order_on_both_platforms() {
        let d = Path::new("/named/dir");
        let h2 = Path::new("/home2");
        let h1 = Path::new("/home1");
        let xdg = Path::new("/state");
        assert_eq!(
            resolve_machine_dir(Some(d), Some(h2), Some(h1), Some(xdg)),
            d,
            "FLEET_DIR is the directory itself and wins outright"
        );
        assert_eq!(
            resolve_machine_dir(None, Some(h2), Some(h1), None),
            h2.join(".fleet"),
            "FLEET_HOME is the home it sits under, and beats HOME"
        );
        assert_eq!(
            resolve_machine_dir(None, None, Some(h1), None),
            h1.join(".fleet")
        );
    }

    /// The agent contract's own deadline, named whole by the cause the runner
    /// gives it — the one reading of `deadline_cause` that needs that
    /// constant.
    #[test]
    fn the_deadline_cause_names_the_adapters_deadline() {
        assert!(
            fleet_core::process::deadline_cause(fleet_core::agent::types::AGENT_TIMEOUT)
                .contains("did not answer within 20s")
        );
    }

    /// The one cell of the platform table that differs between the two.
    #[test]
    #[cfg(target_os = "macos")]
    fn the_state_directory_is_not_a_shape_this_platform_reads() {
        let under_state =
            resolve_machine_dir(None, None, Some(Path::new("/h")), Some(Path::new("/s")));
        assert_eq!(under_state, Path::new("/h/.fleet"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_state_directory_moves_the_machine_directory() {
        let under_state =
            resolve_machine_dir(None, None, Some(Path::new("/h")), Some(Path::new("/s")));
        assert_eq!(under_state, Path::new("/s/fleet"));
    }

    /// The constructed child PATH: built from the platform's own list and
    /// the home passed in, and from NOTHING this process carries.
    ///
    /// The last assertion is the one that makes the rest a reading: a directory
    /// this process's own `PATH` names and the platform's list does not must be
    /// absent, or "constructed" and "inherited" would be the same string on a
    /// developer's box.
    #[test]
    fn the_child_path_is_built_from_the_platform_list_and_the_home_it_is_given() {
        let built = child_path(Path::new("/h"));
        let entries: Vec<PathBuf> = std::env::split_paths(&built).collect();
        assert!(
            entries.contains(&PathBuf::from("/h/.local/bin")),
            "the entry keyed on the home passed in: {built}"
        );
        assert!(entries.contains(&PathBuf::from("/usr/bin")));
        assert!(entries.contains(&PathBuf::from("/bin")));
        assert!(
            !entries.contains(&PathBuf::from("")),
            "no empty element, which is the current directory on a search path: {built}"
        );

        // A different home moves the entry keyed on it and no other.
        let elsewhere: Vec<PathBuf> =
            std::env::split_paths(&child_path(Path::new("/elsewhere"))).collect();
        assert!(elsewhere.contains(&PathBuf::from("/elsewhere/.local/bin")));
        assert!(!elsewhere.contains(&PathBuf::from("/h/.local/bin")));

        // No entry twice: a search path that repeats a directory searches it
        // twice for every miss.
        let mut seen = entries.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(
            seen.len(),
            entries.len(),
            "a directory is listed once: {built}"
        );

        // The reading that separates constructed from inherited on this box.
        let mine = std::env::var("PATH").unwrap_or_default();
        let unique_to_this_process = std::env::split_paths(&mine)
            .find(|dir| !entries.contains(dir))
            .map(|dir| dir.display().to_string());
        match unique_to_this_process {
            Some(dir) => assert!(
                !built.contains(&dir),
                "a directory only this process's PATH names reached the built one: {dir}"
            ),
            // A box whose whole PATH is inside the platform's list says nothing
            // either way, so the arm says so rather than passing quietly.
            None => assert!(
                !mine.is_empty(),
                "this process carries no PATH at all, so nothing here was compared"
            ),
        }
    }

    /// The macOS service file, from the four arguments and nothing else.
    ///
    /// The last two assertions are the ones a mutant reaches: the user's whole
    /// `PATH` baked into the file is the defect lessons claude-code D1 names,
    /// and a caller with no fleet directory writes no environment dict at all —
    /// so a writer that always wrote one would hand the service a variable the
    /// caller did not set.
    #[test]
    fn the_macos_service_file_names_the_executable_and_bakes_in_no_path() {
        let file = macos::service_file(
            Path::new("/h"),
            "a.label",
            Path::new("/bin/fleet"),
            Path::new("/m"),
            Some(Path::new("/scratch")),
        );
        assert_eq!(
            file.path,
            PathBuf::from("/h/Library/LaunchAgents/a.label.plist")
        );
        assert!(
            file.text.contains("<string>a.label</string>"),
            "{}",
            file.text
        );
        assert!(
            file.text
                .contains("<string>/bin/fleet</string>\n    <string>observe</string>"),
            "the program arguments are the executable and the loop: {}",
            file.text
        );
        assert!(
            file.text.contains("<key>RunAtLoad</key>\n  <true/>"),
            "{}",
            file.text
        );
        assert!(
            file.text.contains("<key>KeepAlive</key>\n  <true/>"),
            "{}",
            file.text
        );
        assert!(
            file.text.contains("<string>/m/service.out.log</string>"),
            "{}",
            file.text
        );
        assert!(
            file.text.contains("<string>/m/service.err.log</string>"),
            "{}",
            file.text
        );
        assert!(
            file.text
                .contains("<key>FLEET_DIR</key>\n    <string>/scratch</string>"),
            "a scratch machine directory reaches the service: {}",
            file.text
        );
        assert!(
            !file.text.contains("PATH"),
            "the user's whole PATH must not be baked in: {}",
            file.text
        );

        // The control for the environment dict: a caller with no fleet
        // directory writes none, so the key above is the argument's and not a
        // constant this writer always emits.
        let bare = macos::service_file(
            Path::new("/h"),
            "a.label",
            Path::new("/bin/fleet"),
            Path::new("/m"),
            None,
        );
        assert!(!bare.text.contains("EnvironmentVariables"), "{}", bare.text);

        // A path carrying markup is escaped, or the file this writes does not
        // parse as one.
        let marked = macos::service_file(
            Path::new("/h"),
            "a.label",
            Path::new("/bin/a&b"),
            Path::new("/m"),
            None,
        );
        assert!(
            marked.text.contains("<string>/bin/a&amp;b</string>"),
            "{}",
            marked.text
        );
    }

    /// The Linux unit, read on whichever box this suite runs on.
    #[test]
    fn the_linux_unit_names_the_executable_restarts_and_wants_the_default_target() {
        let file = linux::service_file(
            Path::new("/h"),
            "a.label",
            Path::new("/bin/fleet"),
            Path::new("/m"),
            Some(Path::new("/scratch")),
        );
        assert_eq!(
            file.path,
            PathBuf::from("/h/.config/systemd/user/a.label.service")
        );
        assert!(
            file.text.contains("ExecStart=/bin/fleet observe"),
            "{}",
            file.text
        );
        assert!(file.text.contains("Restart=always"), "{}", file.text);
        assert!(
            file.text.contains("WantedBy=default.target"),
            "{}",
            file.text
        );
        assert!(
            file.text.contains("Environment=FLEET_DIR=/scratch"),
            "{}",
            file.text
        );
        assert!(
            !file.text.contains("PATH"),
            "the user's whole PATH must not be baked in: {}",
            file.text
        );

        let bare = linux::service_file(
            Path::new("/h"),
            "a.label",
            Path::new("/bin/fleet"),
            Path::new("/m"),
            None,
        );
        assert!(!bare.text.contains("Environment="), "{}", bare.text);
    }

    /// The pid each manager reports, and the answer each gives for a service
    /// that is loaded and not running — which is no pid on both, by two
    /// different spellings.
    #[test]
    fn each_manager_reads_a_pid_out_of_its_own_report_and_no_pid_out_of_neither() {
        assert_eq!(
            macos::pid_in("\tstate = running\n\tpid = 4242\n\tprogram = /bin/fleet\n"),
            Some(4242)
        );
        assert_eq!(macos::pid_in("\tstate = waiting\n"), None);
        assert_eq!(linux::pid_in("MainPID=4242\n"), Some(4242));
        assert_eq!(
            linux::pid_in("MainPID=0\n"),
            None,
            "zero is no pid, never process 0"
        );
    }

    /// The gate's three answers, with the D4 property the middle one exists
    /// for: a probe that outran the bound is READ AGAIN rather than started
    /// again, so a blocked dialog costs one parked thread per seat and not one
    /// per poll.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_probe_that_outran_the_bound_is_parked_and_never_started_twice() {
        use std::sync::atomic::AtomicUsize;

        let started = Arc::new(AtomicUsize::new(0));
        let answer = Arc::new(AtomicBool::new(false));
        let (counted, flag) = (Arc::clone(&started), Arc::clone(&answer));
        let listing: Listing = Arc::new(move |_: &Path| {
            counted.fetch_add(1, Ordering::SeqCst);
            while !flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        let mut grant = Grant::new(listing, Duration::from_millis(50));
        let paths = vec![PathBuf::from("/wt/one")];

        let first = grant.poll(&paths);
        assert_eq!(first.state, GRANT_PENDING);
        assert!(
            first
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("/wt/one"),
            "the detail names the path: {first:?}"
        );
        assert_eq!(started.load(Ordering::SeqCst), 1);

        // A second poll while the probe is outstanding reads the same parked
        // one: still pending, and the listing was not called again.
        let second = grant.poll(&paths);
        assert_eq!(second.state, GRANT_PENDING);
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "an outstanding probe is not re-probed"
        );

        // The dialog is answered. The parked call returns, and the next poll
        // reads it — with no restart and no second probe.
        answer.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut read = grant.poll(&paths);
        while !read.is_ok() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            read = grant.poll(&paths);
        }
        assert_eq!(read.state, GRANT_OK, "{read:?}");
        assert_eq!(read.detail, None);
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "the answer came off the parked probe"
        );

        // And an answered path is never probed again.
        assert!(grant.poll(&paths).is_ok());
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }

    /// The other two answers: a listing that answers at once is ok, and one that
    /// comes back an error is pending with the detail naming which it was —
    /// the D4 rule that a refusal and a timeout are one state.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_listing_that_answers_is_ok_and_one_that_refuses_is_pending() {
        let fine: Listing = Arc::new(|_: &Path| Ok(()));
        let mut grant = Grant::new(fine, Duration::from_secs(5));
        let read = grant.poll(&[PathBuf::from("/wt/one"), PathBuf::from("/wt/two")]);
        assert_eq!(read.state, GRANT_OK);
        assert_eq!(read.detail, None);

        let denied: Listing = Arc::new(|_: &Path| Err("operation not permitted".to_string()));
        let mut grant = Grant::new(denied, Duration::from_secs(5));
        let read = grant.poll(&[PathBuf::from("/wt/one")]);
        assert_eq!(read.state, GRANT_PENDING);
        let detail = read.detail.unwrap_or_default();
        assert!(detail.contains("refused"), "{detail}");
        assert!(detail.contains("operation not permitted"), "{detail}");

        // An empty fleet has nothing to probe and is not pending.
        let fine: Listing = Arc::new(|_: &Path| Ok(()));
        assert!(Grant::new(fine, Duration::from_secs(5)).poll(&[]).is_ok());
    }

    /// The default listing's three answers, and the one that decides whether a
    /// first-run fleet is held: a directory that is there reads OK; a directory
    /// that is NOT THERE also reads ok, because an absent path is not a dialog
    /// a person can answer and the worktrees a seat's row names are theirs to
    /// create; and a path this process may not read is an error, which is the
    /// grant question the gate exists for.
    #[test]
    fn the_directory_listing_answers_ok_for_a_missing_path_and_errs_for_an_unreadable_one() {
        use std::os::unix::fs::PermissionsExt;
        let listing = directory_listing();
        let dir = std::env::temp_dir().join(format!("fleet-listing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        assert_eq!(listing(&dir), Ok(()));
        assert_eq!(
            listing(&dir.join("nothing-here")),
            Ok(()),
            "a worktree the person has not created yet is not a grant question"
        );

        // The control: an error is still an error, or the arm above would say
        // only that this listing never refuses anything. A directory with no
        // execute or read bit cannot be listed, which is the same shape a
        // permission layer's denial arrives in. uid 0 ignores the mode bit, so
        // that box asserts what holds there instead of returning unmeasured.
        let closed = dir.join("closed");
        std::fs::create_dir_all(&closed).unwrap();
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        // SAFETY: geteuid takes nothing, returns the effective uid and cannot fail.
        if unsafe { geteuid() } == 0 {
            assert_eq!(
                listing(&closed),
                Ok(()),
                "uid 0 reads a directory whatever its mode says"
            );
        } else {
            let refused = listing(&closed).expect_err("a directory this process may not read");
            assert!(
                !refused.is_empty(),
                "the reason travels with the refusal: {refused:?}"
            );
        }
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On a platform with no dialog in front of a read, the gate reads `ok` and
    /// probes nothing — which is a reading, not a skip: the listing here would
    /// block for ever if it were ever called.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_ungated_platform_reads_ok_without_probing() {
        let never: Listing = Arc::new(|_: &Path| {
            std::thread::sleep(Duration::from_secs(600));
            Ok(())
        });
        let mut grant = Grant::new(never, Duration::from_millis(50));
        let started = Instant::now();
        let read = grant.poll(&[PathBuf::from("/wt/one")]);
        assert_eq!(read.state, GRANT_OK);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn this_process_reads_alive_and_a_reaped_child_reads_gone() {
        assert_eq!(process_alive(std::process::id()), Some(true));
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("a shell runs");
        let pid = child.id();
        child.wait().expect("the child is reaped");
        assert_eq!(
            process_alive(pid),
            Some(false),
            "a reaped pid is gone, not unknown"
        );
    }
}
