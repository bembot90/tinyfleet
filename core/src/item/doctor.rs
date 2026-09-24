//! The doctor slot as a runner: every `doctor/<name>/` entry the layers
//! resolve, its `doctor.toml`'s script run bounded, and the verdict its exit
//! gives.
//!
//! RESOLUTION IS PER FILE, as every slot's is: an entry's `doctor.toml` and its
//! script each come off the highest layer that carries that path, so a pack on
//! top can replace the declaration, the script, or both, and the defaults are
//! the bottom whatever is installed. An entry's LAYER is the one that carries
//! its `doctor.toml`, because that file is what says the check exists.
//!
//! THE EXIT AND NOT THE PROSE. 0 is pass, 1 is a finding, and anything else is
//! could-not-tell — a check's own "could not read", a shell's 127, a signal, a
//! deadline, and an entry that names nothing to run all alike. Core reads the
//! code and hands the lines back, because what a check knows is the check's and
//! a parser here would be a second opinion about it.
//!
//! BOUNDED, ALWAYS. A check is a script somebody else wrote, run before a verb
//! goes on; one that hangs must not hang the verb, so every run goes through
//! [`crate::process::run_bounded`] and a check past its bound is killed with
//! its process group and could not tell.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::item::brief::Packs;
use crate::item::run::manifest_of;
use crate::pack::Runtime;
use crate::resolve::Layer;

/// The slot a check lives under.
pub const SLOT: &str = "doctor";

/// The file inside an entry that names which script to run.
pub const DOCTOR_TOML: &str = "doctor.toml";

/// The environment variable a check reads to learn which pack's manifest it is
/// measuring.
pub const PACK_DIR: &str = "FLEET_PACK_DIR";

/// The entry that measures a pack's pinned runtime. The binary's defaults ship
/// the shape against no table of their own; a pack that pins a runtime shadows
/// it with the instance.
pub const RUNTIME_VERSION: &str = "runtime-version";

/// How long one check is given before its group is killed. A whole-call bound,
/// as the store's is: a check answers in well under a second or is waiting on
/// something that will not come.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// What one check, or a set of them, said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Finding,
    CouldNotTell,
}

impl Verdict {
    /// The word a person reads.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Finding => "finding",
            Verdict::CouldNotTell => "could not tell",
        }
    }

    /// The JSON spelling, which is the exit table's own name for the row.
    pub fn code(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Finding => "finding",
            Verdict::CouldNotTell => "could_not_tell",
        }
    }

    /// The exit a verb answering with this verdict returns.
    pub fn exit(self) -> u8 {
        match self {
            Verdict::Pass => super::DONE,
            Verdict::Finding => super::REFUSED,
            Verdict::CouldNotTell => super::COULD_NOT_TELL,
        }
    }

    /// The verdict a check's exit gives. `None` is a child that did not exit
    /// by itself — a signal — and is could-not-tell with every code past 1.
    pub fn of_exit(code: Option<i32>) -> Verdict {
        match code {
            Some(0) => Verdict::Pass,
            Some(1) => Verdict::Finding,
            _ => Verdict::CouldNotTell,
        }
    }

    /// The verdict over a set: could-not-tell beats a finding beats a pass. A
    /// set nobody could read is not one that found nothing, so the unknown
    /// wins; an empty set found nothing and passes.
    pub fn aggregate<I: IntoIterator<Item = Verdict>>(all: I) -> Verdict {
        all.into_iter()
            .fold(Verdict::Pass, |worst, one| match (worst, one) {
                (Verdict::CouldNotTell, _) | (_, Verdict::CouldNotTell) => Verdict::CouldNotTell,
                (Verdict::Finding, _) | (_, Verdict::Finding) => Verdict::Finding,
                _ => Verdict::Pass,
            })
    }
}

/// One `doctor/<name>/` entry as the layers resolve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The directory's name, which is the check's: `doctor.toml` carries none.
    pub name: String,
    /// The layer that carries `doctor.toml`, or — for an entry with none — the
    /// first resolved path under the entry.
    pub layer: String,
    /// That layer's root.
    pub root: PathBuf,
    pub description: Option<String>,
    /// The script to run, or why there is none. Every other key `doctor.toml`
    /// holds is ignored: judging the file is `pack check`'s, not the runner's.
    pub script: Result<PathBuf, String>,
}

/// Every check the layers resolve, one per distinct `doctor/<name>/`, in name
/// order.
pub fn entries(packs: &Packs) -> Vec<Entry> {
    let prefix = format!("{SLOT}/");
    let names: BTreeSet<&str> = packs
        .resolution
        .files
        .keys()
        .filter_map(|relative| relative.strip_prefix(&prefix)?.split_once('/'))
        .filter(|(_, rest)| !rest.is_empty())
        .map(|(name, _)| name)
        .collect();
    names
        .into_iter()
        .filter_map(|name| entry(packs, name))
        .collect()
}

/// One check by name, or `None` where no layer carries a file under it.
pub fn entry(packs: &Packs, name: &str) -> Option<Entry> {
    let under = format!("{SLOT}/{name}/");
    let declared = format!("{under}{DOCTOR_TOML}");
    let declares = packs.resolution.files.get(&declared);
    let carrier = match declares {
        Some(carrier) => carrier,
        None => packs
            .resolution
            .files
            .range(under.clone()..)
            .next()
            .filter(|(relative, _)| relative.starts_with(&under))
            .map(|(_, carrier)| carrier)?,
    };
    let root = packs
        .layers
        .iter()
        .find(|layer| &layer.name == carrier)
        .map(|layer| layer.root.clone())?;
    let mut entry = Entry {
        name: name.to_string(),
        layer: carrier.clone(),
        root,
        description: None,
        script: Err(format!("`{SLOT}/{name}` holds no {DOCTOR_TOML}")),
    };
    if declares.is_none() {
        return Some(entry);
    }

    let toml_path = entry.root.join(&declared);
    let parsed: toml::Table = match std::fs::read_to_string(&toml_path)
        .map_err(|e| e.to_string())
        .and_then(|text| text.parse::<toml::Table>().map_err(|e| e.to_string()))
    {
        Ok(parsed) => parsed,
        Err(e) => {
            entry.script = Err(format!(
                "{} does not parse as TOML: {e}",
                toml_path.display()
            ));
            return Some(entry);
        }
    };
    entry.description = parsed
        .get("description")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    entry.script = match parsed.get("run").and_then(toml::Value::as_str) {
        None => Err(format!(
            "{} names no `run` script for the check to run",
            toml_path.display()
        )),
        Some(script) => packs
            .slot(&format!("{under}{script}"))
            .map_err(|stop| stop.message),
    };
    Some(entry)
}

/// How one check is run: what it measures, and on which search path.
pub struct Invocation<'a> {
    /// The pack whose manifest the check reads, as [`PACK_DIR`].
    pub pack_dir: &'a Path,
    /// The whole `PATH` the check runs on, constructed by the caller.
    pub path: &'a str,
    /// Where it runs, or this process's own directory.
    pub cwd: Option<&'a Path>,
    /// Anything else the caller hands it. Everything not named here or above is
    /// this process's own, inherited.
    pub env: &'a [(&'a str, &'a OsStr)],
    pub timeout: Duration,
}

/// What one run of a check gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub verdict: Verdict,
    /// The check's own exit, or `None` where it gave none: never started, past
    /// its bound, or killed by a signal.
    pub exit: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// The one line a row shows: the last thing the check said, or why it said
    /// nothing.
    pub line: String,
}

/// Run one check, bounded, and read its verdict off its exit. An entry that
/// cannot be run is could-not-tell here and nothing is spawned.
pub fn run(entry: &Entry, how: &Invocation) -> Checked {
    let unread = |why: String| Checked {
        verdict: Verdict::CouldNotTell,
        exit: None,
        stdout: String::new(),
        stderr: String::new(),
        line: why,
    };
    let script = match &entry.script {
        Ok(script) => script,
        Err(why) => return unread(why.clone()),
    };

    let mut command = Command::new("sh");
    command
        .arg(script)
        .env(PACK_DIR, how.pack_dir)
        .env("PATH", how.path);
    for (key, value) in how.env {
        command.env(key, value);
    }
    if let Some(cwd) = how.cwd {
        command.current_dir(cwd);
    }
    let out = match crate::process::run_bounded(command, how.timeout) {
        Ok(out) => out,
        Err(why) => return unread(why),
    };

    let exit = out.status.code();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let line = last_line(&stdout)
        .or_else(|| last_line(&stderr))
        .map(str::to_string)
        .unwrap_or_else(|| match exit {
            Some(code) => format!("said nothing and exited {code}"),
            None => String::from("was killed by a signal and said nothing"),
        });
    Checked {
        verdict: Verdict::of_exit(exit),
        exit,
        stdout,
        stderr,
        line,
    }
}

fn last_line(said: &str) -> Option<&str> {
    said.lines().map(str::trim).rfind(|line| !line.is_empty())
}

/// Every installed pack that pins a runtime, top first: the set a runtime
/// check measures. A manifest that cannot be read or parsed is kept, with the
/// reason, so the caller can say which pack it could not measure.
pub fn pinned(packs: &Packs) -> Vec<(Layer, Result<Runtime, String>)> {
    packs
        .layers
        .iter()
        .filter(|layer| !layer.defaults)
        .filter_map(|layer| match manifest_of(layer) {
            Ok(manifest) => manifest.runtime.map(|runtime| (layer.clone(), Ok(runtime))),
            Err(stop) => Some((layer.clone(), Err(stop.message))),
        })
        .collect()
}
