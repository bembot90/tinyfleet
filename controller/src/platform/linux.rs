//! The Linux half of the platform layer.

use super::{resolve_on_path, run_bounded, ServiceFile};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub fn machine_dir_under(home: &Path, xdg_state: Option<&Path>) -> PathBuf {
    match xdg_state {
        Some(state) => state.join("fleet"),
        None => home.join(".fleet"),
    }
}

/// The directories a child of this controller searches, in order.
///
/// Built from this list and the home passed in, never from the process's own
/// `PATH`: a service-launched process carries a minimal one, and every session
/// claimed from a daemon started under it inherits that (lessons claude-code
/// D1). There is no `sbin` pair here and no package-manager prefix: a systemd
/// user unit's own environment holds neither, and the agent installs under the
/// home's local bin.
pub fn child_path_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local").join("bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]
}

extern "C" {
    fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
}

/// The five-minute load average, or `None` when the C library would not answer —
/// which is a reading nobody has and never a machine under no load.
///
/// The C call rather than `/proc/loadavg`, unlike the process read below: both
/// libc implementations this target ships read that file themselves, so parsing
/// it here would be a second parser for the same bytes. Which of the three
/// samples is the belt's is `super::LOAD_SAMPLE`'s to say, so the two platforms
/// cannot read different minutes.
pub fn load_average_5m() -> Option<f64> {
    let mut samples = [0.0f64; 3];
    // SAFETY: the pointer is to an array of three, and 3 is the count passed.
    let filled = unsafe { getloadavg(samples.as_mut_ptr(), 3) };
    super::belt_sample(&samples[..filled.clamp(0, 3) as usize])
}

/// The process table is `/proc`, not `/bin/ps`. A `/proc` that is not mounted
/// answers `None`: without it this layer cannot tell a gone process from a
/// filesystem it cannot see.
pub fn process_alive(pid: u32) -> Option<bool> {
    if pid == 0 || !Path::new("/proc/self").is_dir() {
        return None;
    }
    Some(Path::new("/proc").join(pid.to_string()).is_dir())
}

// ---- the service manager ----------------------------------------------------

/// The manager this platform's user services are loaded through.
pub const SERVICE_BIN: &str = "systemctl";

/// This platform puts no file-access dialog in front of a directory read, so
/// the gate reads `ok` without probing (lessons claude-code D4).
pub const GRANT_IS_GATED: bool = false;

/// How long the after-write reading is given.
const ASIDE_TIMEOUT: Duration = Duration::from_secs(20);

/// The unit file under the user's own unit directory.
///
/// `FLEET_DIR` is carried into the service's environment ONLY when the caller
/// has one — a scratch machine directory has to reach the service — and no
/// `PATH` is written: the controller constructs its children's search path, and
/// one frozen here at install time is what every later session inherits
/// (lessons claude-code D1).
pub fn service_file(
    home: &Path,
    label: &str,
    exe: &Path,
    machine_dir: &Path,
    fleet_dir: Option<&Path>,
) -> ServiceFile {
    let environment = match fleet_dir {
        Some(dir) => format!("Environment=FLEET_DIR={}\n", dir.display()),
        None => String::new(),
    };
    let text = format!(
        "[Unit]\n\
         Description=the fleet controller\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} observe\n\
         Restart=always\n\
         WorkingDirectory={machine}\n\
         {environment}\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe.display(),
        machine = machine_dir.display(),
    );
    ServiceFile {
        path: home
            .join(".config")
            .join("systemd")
            .join("user")
            .join(format!("{label}.service")),
        text,
    }
}

/// This platform's manager collects what the service printed into the user's
/// own journal rather than into a file, so what is named is the command that
/// reads it.
pub fn service_stderr(_machine_dir: &Path) -> String {
    format!(
        "the user journal, read with: journalctl --user -u {}.service",
        super::SERVICE_LABEL
    )
}

/// TWO COMMANDS, in this order: the manager is told to re-read its unit
/// directory, and only then is the unit enabled and started. A unit written
/// after the last read is one the second command would not find.
pub fn load_argv(label: &str, _file: &Path) -> Vec<Vec<String>> {
    vec![
        vec!["--user".to_string(), "daemon-reload".to_string()],
        vec![
            "--user".to_string(),
            "enable".to_string(),
            "--now".to_string(),
            unit(label),
        ],
    ]
}

pub fn unload_argv(label: &str) -> Vec<String> {
    vec!["--user".to_string(), "stop".to_string(), unit(label)]
}

pub fn running_argv(label: &str) -> Vec<String> {
    vec![
        "--user".to_string(),
        "show".to_string(),
        unit(label),
        "--property=MainPID".to_string(),
    ]
}

fn unit(label: &str) -> String {
    format!("{label}.service")
}

/// The pid out of the manager's own report. A unit that is loaded and not
/// running answers zero, which is no pid rather than process 0.
pub fn pid_in(stdout: &str) -> Option<u32> {
    stdout
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("MainPID="))
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .filter(|pid| *pid != 0)
}

/// LINGERING IS READ AND NEVER SET. A user service stops when the last session
/// of that user ends unless lingering is enabled, and enabling it is a change to
/// the machine's own login policy — so the command is printed for the person and
/// this layer runs nothing.
pub fn after_write(child_path: &str, _label: &str, _file: &Path) -> Vec<String> {
    let Some(bin) = resolve_on_path(child_path, "loginctl") else {
        return vec![format!(
            "lingering was not read: `loginctl` is not on the constructed search path {child_path}"
        )];
    };
    let mut command = Command::new(&bin);
    command.args(["show-user", "--property=Linger"]);
    match run_bounded(command, ASIDE_TIMEOUT) {
        Ok(run)
            if run.status.success()
                && String::from_utf8_lossy(&run.stdout).contains("Linger=yes") =>
        {
            vec!["lingering is on: the controller survives a logout".to_string()]
        }
        Ok(run) if run.status.success() => vec![format!(
            "lingering is off, so the controller stops at logout — run this yourself: {} \
             enable-linger",
            bin.display()
        )],
        Ok(run) => vec![format!(
            "lingering could not be read: {}",
            String::from_utf8_lossy(&run.stderr)
                .trim()
                .lines()
                .next()
                .unwrap_or("no reason given")
        )],
        Err(why) => vec![format!("lingering could not be read: {why}")],
    }
}
