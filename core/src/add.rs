//! `fleet pack add`: one pack fetched by git source and version, checked against
//! what is already installed, and pinned in the lock.
//!
//! Every git call goes through the binary on `PATH` by `std::process::Command`:
//! the epic ships no new dependency, and a verb that shells out to git is one an
//! operator can reproduce by hand from the refusal it printed.
//!
//! The functions here are pure over paths — the packs directory, the lock, and
//! the stamp the caller read from its own clock — so the cli resolves the
//! machine directory and this crate never learns the platform layer exists.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::lock;
use crate::pack;
use crate::resolve;

/// The prefix that pins a version to an exact commit. Anything else is a tag or
/// a branch name, resolved by git at fetch time and recorded as what it was.
pub const SHA: &str = "sha:";

/// The scratch directory's name inside the packs dir, with the process id, so
/// two adds running at once never share one. It is a dotted name, which is also
/// how the installed-pack scan tells it apart from a pack.
const SCRATCH: &str = ".fleet-add";

/// A git source, optionally naming a subdirectory inside the repository with the
/// two-slash form Gas City's own manifests use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub repo: String,
    pub subdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// A tag or a branch name, resolved by git.
    Named(String),
    /// A full 40-hex commit, already resolved.
    Sha(String),
}

impl Version {
    /// What git is asked to check out. The two forms differ in what they promise
    /// a later reader, not in how they are fetched.
    pub fn revision(&self) -> &str {
        match self {
            Version::Named(name) => name,
            Version::Sha(sha) => sha,
        }
    }
}

/// What the verb installed, for the caller to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub root: PathBuf,
    pub entry: lock::Entry,
}

/// Any one of these leaves the packs directory and the lock as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    EmptySource,
    MalformedSource(String),
    EscapingSubdir(String),
    EmptyVersion,
    CaretVersion(String),
    MalformedSha(String),
    Scratch(String),
    Git {
        step: &'static str,
        status: String,
        stderr: String,
    },
    NoSubdir {
        source: String,
        subdir: String,
    },
    Defect {
        pack: String,
        defect: String,
    },
    PackName(String),
    AlreadyInstalled {
        name: String,
        root: String,
    },
    Layering(String),
    Install(String),
    Lock(lock::LockError),
    LockDidNotLand(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::EmptySource => write!(f, "no source — a source is a git URL or a local path"),
            Refusal::MalformedSource(s) => write!(
                f,
                "`{s}` is not a source — a git URL or a local path, \
                 optionally followed by `//` and a subdirectory inside it"
            ),
            Refusal::EscapingSubdir(s) => write!(
                f,
                "`{s}` names a subdirectory outside the repository — \
                 the part after `//` is a path inside it"
            ),
            Refusal::EmptyVersion => write!(
                f,
                "no version — pass --version with a tag, a branch or `{SHA}<40 hex>`"
            ),
            Refusal::CaretVersion(v) => write!(
                f,
                "`{v}` is a caret range — pin a tag or `{SHA}<40 hex>`; \
                 ranges resolve against a registry this verb does not have"
            ),
            Refusal::MalformedSha(v) => write!(
                f,
                "`{v}` is not a commit — `{SHA}` takes a full 40-character hex sha"
            ),
            Refusal::Scratch(e) => write!(f, "the packs directory cannot be prepared: {e}"),
            Refusal::Git {
                step,
                status,
                stderr,
            } => {
                write!(f, "git {step} {status}: {}", one_line(stderr))
            }
            Refusal::NoSubdir { source, subdir } => {
                write!(f, "`{source}`: the repository holds no `{subdir}`")
            }
            Refusal::Defect { pack, defect } => write!(f, "{pack}: {defect}"),
            Refusal::PackName(n) => write!(
                f,
                "the manifest names this pack `{n}`, which is not a directory name"
            ),
            Refusal::AlreadyInstalled { name, root } => write!(
                f,
                "a pack named `{name}` is already installed at `{root}` — \
                 remove it before adding another"
            ),
            Refusal::Layering(r) => write!(f, "{r}"),
            Refusal::Install(e) => {
                write!(f, "the pack cannot be moved into the packs directory: {e}")
            }
            Refusal::Lock(e) => write!(f, "{e}"),
            Refusal::LockDidNotLand(source) => write!(
                f,
                "{} was written and does not carry `{source}` — the pin did not land",
                lock::LOCK
            ),
        }
    }
}

/// A git URL or path, and the subdirectory after `//` when there is one.
///
/// The scheme's own `//` is stepped over: a source is split on the FIRST `//`
/// after `<scheme>://`, which is the form the reference's manifests use and the
/// only one that reads unambiguously.
pub fn parse_source(raw: &str) -> Result<Source, Refusal> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Refusal::EmptySource);
    }
    let after_scheme = scheme_end(raw);
    let Some(at) = raw[after_scheme..].find("//") else {
        return Ok(Source {
            repo: raw.to_string(),
            subdir: None,
        });
    };
    let split = after_scheme + at;
    let repo = raw[..split].trim_end_matches('/');
    let subdir = raw[split + 2..].trim_matches('/');
    if repo.is_empty() || subdir.is_empty() {
        return Err(Refusal::MalformedSource(raw.to_string()));
    }
    if subdir
        .split('/')
        .any(|part| part == ".." || part.is_empty())
    {
        return Err(Refusal::EscapingSubdir(raw.to_string()));
    }
    Ok(Source {
        repo: repo.to_string(),
        subdir: Some(subdir.to_string()),
    })
}

/// The index just past `<scheme>://`, or 0 where the source carries no scheme.
fn scheme_end(s: &str) -> usize {
    match s.find("://") {
        Some(at)
            if at > 0
                && s[..at]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) =>
        {
            at + 3
        }
        _ => 0,
    }
}

pub fn parse_version(raw: &str) -> Result<Version, Refusal> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Refusal::EmptyVersion);
    }
    if raw.starts_with('^') {
        return Err(Refusal::CaretVersion(raw.to_string()));
    }
    if let Some(sha) = raw.strip_prefix(SHA) {
        if sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(Version::Sha(sha.to_ascii_lowercase()));
        }
        return Err(Refusal::MalformedSha(raw.to_string()));
    }
    Ok(Version::Named(raw.to_string()))
}

/// Fetch one pack, check it against what is installed, move it in and pin it.
///
/// `fetched` is the caller's own UTC stamp: this crate reads no clock, the same
/// way it resolves no machine directory.
///
/// Nothing is written outside `packs_dir` and `lock_path`, and a refusal at any
/// step leaves both as they were — the fetch lands in a scratch directory that
/// is removed on every path out.
pub fn add(
    packs_dir: &Path,
    defaults_dir: &Path,
    lock_path: &Path,
    source: &str,
    version: &str,
    fetched: &str,
) -> Result<Installed, Vec<Refusal>> {
    let source_as_given = source.trim().to_string();
    let version_as_given = version.trim().to_string();
    let parsed = parse_source(source).map_err(|r| vec![r])?;
    let wanted = parse_version(version).map_err(|r| vec![r])?;

    std::fs::create_dir_all(packs_dir).map_err(|e| vec![Refusal::Scratch(e.to_string())])?;
    let scratch = Scratch::under(packs_dir).map_err(|r| vec![r])?;

    let commit = fetch(&parsed, &wanted, &scratch.root).map_err(|r| vec![r])?;

    // The clone's own `.git` is not part of the pack, and a pack whose root is
    // the repository root would fail its own format check on it.
    let _ = std::fs::remove_dir_all(scratch.root.join(".git"));

    let candidate = match &parsed.subdir {
        None => scratch.root.clone(),
        Some(subdir) => {
            let inner = scratch.root.join(subdir);
            if !inner.is_dir() {
                return Err(vec![Refusal::NoSubdir {
                    source: source_as_given,
                    subdir: subdir.clone(),
                }]);
            }
            inner
        }
    };

    let name = check_format(&candidate)?;
    let destination = packs_dir.join(&name);
    if destination.exists() {
        return Err(vec![Refusal::AlreadyInstalled {
            name,
            root: destination.display().to_string(),
        }]);
    }
    check_layering(packs_dir, defaults_dir, &name, &candidate)?;

    std::fs::rename(&candidate, &destination).map_err(|e| vec![Refusal::Install(e.to_string())])?;

    // No tree hash: this pack came from a repository and its commit already
    // says which bytes it is. The key belongs to the pack that has no commit.
    let entry = lock::Entry {
        source: source_as_given,
        name: Some(name.clone()),
        version: version_as_given,
        commit,
        fetched: fetched.to_string(),
        tree: None,
    };
    if let Err(refusal) = pin(lock_path, &entry) {
        let _ = std::fs::remove_dir_all(&destination);
        return Err(vec![refusal]);
    }

    Ok(Installed {
        name,
        root: destination,
        entry,
    })
}

/// Write the pin and read it back. The read is what the verb exits 0 on: a write
/// that reported success and left nothing readable — a lock path that swallows
/// its bytes, a filesystem that lied — is the case a returned `Ok` cannot rule
/// out, and the only one this catches.
fn pin(lock_path: &Path, entry: &lock::Entry) -> Result<(), Refusal> {
    lock::append(lock_path, entry).map_err(Refusal::Lock)?;
    match lock::holds(lock_path, entry) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Refusal::LockDidNotLand(entry.source.clone())),
        Err(e) => Err(Refusal::Lock(e)),
    }
}

/// The candidate's own format, and the name the rest of the verb calls it by.
fn check_format(candidate: &Path) -> Result<String, Vec<Refusal>> {
    let report = pack::check(candidate);
    let name = report
        .manifest
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| String::from("the fetched pack"));
    if !report.is_valid() {
        return Err(report
            .defects
            .into_iter()
            .map(|defect| Refusal::Defect {
                pack: name.clone(),
                defect: defect.to_string(),
            })
            .collect());
    }
    // The name becomes a directory under the packs dir, so a manifest that spells
    // it with a separator or a dot-dot would install outside it.
    if name.is_empty() || Path::new(&name).components().count() != 1 || name.starts_with('.') {
        return Err(vec![Refusal::PackName(name)]);
    }
    Ok(name)
}

/// The candidate among everything already installed, laid as the resolver lays
/// them: every pack above what it imports, the binary's defaults last.
///
/// The layering is the imports', never the installs': a candidate an installed
/// pack imports goes beneath that importer, so the packs may be added in any
/// order. The defaults are the bottom layer whatever is installed, which is what
/// makes the shadow registry answer for a pack added into an empty packs dir.
fn check_layering(
    packs_dir: &Path,
    defaults_dir: &Path,
    name: &str,
    candidate: &Path,
) -> Result<(), Vec<Refusal>> {
    let mut packs = resolve::installed(packs_dir);
    packs.push(resolve::Layer::new(name, candidate));
    let layering = |refusals: Vec<resolve::Refusal>| -> Vec<Refusal> {
        refusals
            .into_iter()
            .map(|r| Refusal::Layering(r.to_string()))
            .collect()
    };
    let layers = resolve::layers_over(packs, defaults_dir).map_err(layering)?;
    resolve::resolve(&layers).map(|_| ()).map_err(layering)
}

/// Clone with no checkout, check out the version, read the commit. Three calls
/// rather than one `clone --branch`, because a sha is not a ref `clone` accepts
/// and one path through the three keeps the tag, the branch and the sha alike.
fn fetch(source: &Source, version: &Version, into: &Path) -> Result<String, Refusal> {
    git(
        "clone",
        &[
            "clone",
            "--no-checkout",
            "--quiet",
            &source.repo,
            &into.display().to_string(),
        ],
    )?;
    let repo = into.display().to_string();
    git(
        "checkout",
        &["-C", &repo, "checkout", "--quiet", version.revision()],
    )?;
    let head = git("rev-parse", &["-C", &repo, "rev-parse", "HEAD"])?;
    let commit = head.trim().to_string();
    if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Refusal::Git {
            step: "rev-parse",
            status: String::from("answered no commit"),
            stderr: commit,
        });
    }
    Ok(commit.to_ascii_lowercase())
}

fn git(step: &'static str, args: &[&str]) -> Result<String, Refusal> {
    // A private source with no credentials on this box would otherwise sit at a
    // prompt no verb can answer; refused is the only outcome a caller can act on.
    let out = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| Refusal::Git {
            step,
            status: String::from("could not be run"),
            stderr: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(Refusal::Git {
            step,
            status: match out.status.code() {
                Some(code) => format!("exited {code}"),
                None => String::from("was killed by a signal"),
            },
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The fetch's working directory, removed on every path out of [`add`] — the
/// success path included, where the pack has been renamed out of it already.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn under(packs_dir: &Path) -> Result<Scratch, Refusal> {
        let root = packs_dir.join(format!("{SCRATCH}-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|e| Refusal::Scratch(e.to_string()))?;
        }
        Ok(Scratch { root })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// git writes several lines and a refusal is one line; the rest stays reachable
/// by running the same command the message names.
fn one_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
        .to_string()
}
