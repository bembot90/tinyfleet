//! The macOS half of the platform layer.

use super::{resolve_on_path, run_bounded, ServiceFile};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// macOS has no XDG state directory, so the second argument is not a shape this
/// platform reads.
pub fn machine_dir_under(home: &Path, _xdg_state: Option<&Path>) -> PathBuf {
    home.join(".fleet")
}

/// The directories a child of this controller searches, in order.
///
/// Built from this list and the home passed in, never from the process's own
/// `PATH`: a service-launched process carries a minimal one that holds neither
/// the package manager's prefix nor the user's local bin, and every session
/// claimed from a daemon started under it inherits that (lessons claude-code
/// D1).
///
/// THE SYSTEM DIRECTORIES COME FIRST, ahead of the package manager's prefix and
/// the user's local bin, so a name the platform also ships resolves to the
/// platform's copy — a broken prefix binary cannot shadow it, and a child of
/// this controller resolves that name the way every shell on the box does. The
/// prefix keeps every name the system does not ship, which is nearly all of
/// them. The order is pinned by an arm in `tests/effects.rs`; a name moved back
/// in front of the system directories fails there rather than going quiet.
pub fn child_path_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local").join("bin"),
    ]
}

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn __error() -> *mut i32;
    fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
    fn getuid() -> u32;
}

// ---- the service manager ----------------------------------------------------

/// The manager this platform's user services are loaded through.
pub const SERVICE_BIN: &str = "launchctl";

/// This platform puts a file-access dialog in front of a directory read, so the
/// gate probes (lessons claude-code D4).
pub const GRANT_IS_GATED: bool = true;

/// How long the after-write reading is given. It is a local call to a file
/// tool, so a deadline it ever meets is a tool that is not answering.
const ASIDE_TIMEOUT: Duration = Duration::from_secs(20);

/// The per-user domain the label is loaded into.
fn domain() -> String {
    // SAFETY: getuid takes nothing, returns the effective uid and cannot fail.
    format!("gui/{}", unsafe { getuid() })
}

/// The agent file: the label's own property list under the user's agents
/// directory.
///
/// `FLEET_DIR` is carried into the service's environment ONLY when the caller
/// has one — a scratch machine directory has to reach the service — and the
/// user's `PATH` is never baked in: the controller constructs its children's
/// search path, and a path frozen here at install time is the one every later
/// session inherits (lessons claude-code D1).
pub fn service_file(
    home: &Path,
    label: &str,
    exe: &Path,
    machine_dir: &Path,
    fleet_dir: Option<&Path>,
) -> ServiceFile {
    let environment = match fleet_dir {
        Some(dir) => format!(
            "  <key>EnvironmentVariables</key>\n  <dict>\n    <key>FLEET_DIR</key>\n    \
             <string>{}</string>\n  </dict>\n",
            escape(&dir.display().to_string())
        ),
        None => String::new(),
    };
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key>\n\
         \x20 <string>{label}</string>\n\
         \x20 <key>ProgramArguments</key>\n\
         \x20 <array>\n\
         \x20   <string>{exe}</string>\n\
         \x20   <string>observe</string>\n\
         \x20 </array>\n\
         \x20 <key>RunAtLoad</key>\n\
         \x20 <true/>\n\
         \x20 <key>KeepAlive</key>\n\
         \x20 <true/>\n\
         \x20 <key>StandardOutPath</key>\n\
         \x20 <string>{out}</string>\n\
         \x20 <key>StandardErrorPath</key>\n\
         \x20 <string>{err}</string>\n\
         {environment}\
         </dict>\n\
         </plist>\n",
        label = escape(label),
        exe = escape(&exe.display().to_string()),
        out = escape(&machine_dir.join("service.out.log").display().to_string()),
        err = escape(&machine_dir.join("service.err.log").display().to_string()),
    );
    ServiceFile {
        path: home
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{label}.plist")),
        text,
    }
}

/// The five characters a property list's own parser would otherwise read as
/// markup. A path a person chose may carry any of them.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Where this platform's manager puts what the service printed: the file the
/// agent above names.
pub fn service_stderr(machine_dir: &Path) -> String {
    machine_dir.join("service.err.log").display().to_string()
}

pub fn load_argv(_label: &str, file: &Path) -> Vec<Vec<String>> {
    vec![vec![
        "bootstrap".to_string(),
        domain(),
        file.display().to_string(),
    ]]
}

pub fn unload_argv(label: &str) -> Vec<String> {
    vec!["bootout".to_string(), format!("{}/{label}", domain())]
}

pub fn running_argv(label: &str) -> Vec<String> {
    vec!["print".to_string(), format!("{}/{label}", domain())]
}

/// The pid out of the manager's own report. A service that is loaded and not
/// running carries no such line, which is `None` and not a zero.
pub fn pid_in(stdout: &str) -> Option<u32> {
    stdout
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("pid = "))
        .and_then(|pid| pid.trim().parse().ok())
}

/// The file is checked by this platform's own parser where that tool resolves,
/// because a service file that does not parse loads as nothing and says so
/// nowhere a person reads.
pub fn after_write(child_path: &str, _label: &str, file: &Path) -> Vec<String> {
    let Some(linter) = resolve_on_path(child_path, "plutil") else {
        return vec![format!(
            "the service file was not checked: `plutil` is not on the constructed search path \
             {child_path}"
        )];
    };
    let mut command = Command::new(&linter);
    command.args(["-lint", &file.display().to_string()]);
    match run_bounded(command, ASIDE_TIMEOUT) {
        Ok(run) if run.status.success() => {
            vec![format!("the service file parses: {}", file.display())]
        }
        Ok(run) => vec![format!(
            "the service file does NOT parse: {}",
            String::from_utf8_lossy(&run.stdout)
                .trim()
                .lines()
                .next()
                .unwrap_or(String::from_utf8_lossy(&run.stderr).trim())
        )],
        Err(why) => vec![format!("the service file could not be checked: {why}")],
    }
}

/// The five-minute load average, or `None` when the C library would not answer —
/// which is a reading nobody has and never a machine under no load.
///
/// Which of the three samples is the belt's is `super::LOAD_SAMPLE`'s to say,
/// so the two platforms cannot read different minutes.
pub fn load_average_5m() -> Option<f64> {
    let mut samples = [0.0f64; 3];
    // SAFETY: the pointer is to an array of three, and 3 is the count passed.
    let filled = unsafe { getloadavg(samples.as_mut_ptr(), 3) };
    super::belt_sample(&samples[..filled.clamp(0, 3) as usize])
}

/// "No such process". Any other failure is a question this layer cannot answer,
/// and answering it "gone" is how a live seat gets dispatched over.
const ESRCH: i32 = 3;

/// Signal 0 is the existence probe: it validates the pid and delivers nothing.
/// `/bin/ps` is deliberately not the source.
pub fn process_alive(pid: u32) -> Option<bool> {
    // 0 addresses the caller's whole process group rather than a process.
    if pid == 0 {
        return None;
    }
    if unsafe { kill(pid as i32, 0) } == 0 {
        return Some(true);
    }
    match unsafe { *__error() } {
        ESRCH => Some(false),
        _ => None,
    }
}
