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

/// The verb a missing import's line names, as a person types it.
const VERB: &str = "fleet pack add";

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

/// The form a person types and the lock keys on: the repository, then `//` and
/// the subdirectory when there is one.
impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.subdir {
            None => write!(f, "{}", self.repo),
            Some(subdir) => write!(f, "{}//{subdir}", self.repo),
        }
    }
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
    /// The packs this one imports that the same checkout held and nothing
    /// installed answered: installed with it, out of the same clone, so each is
    /// pinned at the same commit. Empty on each of these.
    pub imports: Vec<Installed>,
    /// The imports that are neither installed nor in the same checkout. The
    /// pack installs without them — the packs may be added in any order — and
    /// the caller names each.
    pub missing: Vec<Missing>,
}

/// An import an installed pack declares and no installed pack answers.
///
/// A LEGAL STATE and never a refusal: the layering is the imports', never the
/// installs', so a fleet may take the importer first. What it costs is the
/// runtime or the files the import would have carried, which is why every
/// reader that meets one names it with the line that answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub importer: String,
    pub import: String,
    /// The source as the importer's manifest declares it.
    pub source: String,
    /// The `fleet pack add` that installs it, read against the importer's own
    /// source; none where there is no source to read it against, or where the
    /// importer's checkout was seen not to hold it.
    pub line: Option<String>,
}

impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` imports `{}`, which is not installed",
            self.importer, self.import
        )?;
        match &self.line {
            Some(line) => write!(f, " — `{line}` adds it"),
            None => write!(f, " — its manifest names the source `{}`", self.source),
        }
    }
}

/// Where an import's declared source points, read against its importer's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// A directory of the importer's own repository: one clone serves both.
    Inside(Source),
    /// Another repository, as a source a person could type.
    Elsewhere(String),
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
    ImportName {
        importer: String,
        import: String,
        source: String,
        found: String,
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
            Refusal::ImportName {
                importer,
                import,
                source,
                found,
            } => write!(
                f,
                "`{importer}` imports `{import}` from `{source}`, and the pack there calls \
                 itself `{found}` — an import is answered by the pack of its own name"
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

/// Where `declared` points, read against the source of the pack declaring it.
///
/// A source with a scheme, an absolute path or an scp-style `host:path` is
/// another repository as it stands. A relative one is a path from the
/// importer's own directory: while it stays inside the repository it names a
/// subdirectory of the same checkout, and the `..`s that climb past the
/// repository's root climb its URL or path instead, which is how a sibling
/// repository reads.
pub fn locate_import(importer: &Source, declared: &str) -> Located {
    let declared = declared.trim();
    if is_absolute(declared) {
        return Located::Elsewhere(declared.to_string());
    }
    let mut inside: Vec<&str> = importer
        .subdir
        .as_deref()
        .map(|s| s.split('/').filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let mut climbed = 0;
    let mut after: Vec<&str> = Vec::new();
    for part in declared.split('/') {
        match part {
            "" | "." => {}
            ".." if !after.is_empty() => {
                after.pop();
            }
            ".." if climbed == 0 && !inside.is_empty() => {
                inside.pop();
            }
            ".." => climbed += 1,
            _ if climbed > 0 => after.push(part),
            _ => inside.push(part),
        }
    }
    if climbed == 0 {
        return Located::Inside(Source {
            repo: importer.repo.clone(),
            subdir: (!inside.is_empty()).then(|| inside.join("/")),
        });
    }
    let mut repo = importer.repo.trim_end_matches('/').to_string();
    for _ in 0..climbed {
        match repo.rfind('/') {
            Some(at) if at >= scheme_end(&repo) => repo.truncate(at),
            _ => break,
        }
    }
    for part in after {
        repo.push('/');
        repo.push_str(part);
    }
    Located::Elsewhere(repo)
}

/// A source that names its own repository whatever it is read against.
fn is_absolute(source: &str) -> bool {
    if scheme_end(source) > 0 || source.starts_with('/') || source.starts_with('~') {
        return true;
    }
    // scp-style `user@host:path`: a colon before the first slash.
    match (source.find(':'), source.find('/')) {
        (Some(colon), Some(slash)) => colon < slash,
        (Some(_), None) => true,
        _ => false,
    }
}

/// The line that adds `import`, declared by a pack that came from `source` at
/// `version`. Inside the same repository it is the importer's own version,
/// which is the checkout the import was read beside; elsewhere it is the
/// version the manifest declares, the only one there is.
fn line_for(source: &Source, version: &str, import: &pack::Import) -> String {
    match locate_import(source, &import.source) {
        Located::Inside(at) => format!("{VERB} {at} --version {version}"),
        Located::Elsewhere(at) => format!("{VERB} {at} --version {}", import.version),
    }
}

/// Every import an installed pack declares that no installed pack answers,
/// importer by importer in layer order, with the line that adds each read off
/// the importer's own line in the lock.
///
/// A pack the lock does not carry — placed by hand, or by a lock that predates
/// the name key — is still named, without a line.
pub fn missing_imports(layers: &[resolve::Layer], lock_path: &Path) -> Vec<Missing> {
    let pinned = lock::read(lock_path).unwrap_or_default();
    let names: std::collections::BTreeSet<&str> =
        layers.iter().map(|layer| layer.name.as_str()).collect();
    let mut missing = Vec::new();
    for layer in layers.iter().filter(|layer| !layer.defaults) {
        let Some(manifest) = std::fs::read_to_string(layer.root.join(pack::MANIFEST))
            .ok()
            .and_then(|text| pack::parse_manifest(&text).ok())
        else {
            continue;
        };
        let at = pinned
            .iter()
            .find(|entry| entry.name.as_deref() == Some(layer.name.as_str()))
            .and_then(|entry| Some((parse_source(&entry.source).ok()?, entry.version.clone())));
        for import in &manifest.imports {
            if names.contains(import.name.as_str()) {
                continue;
            }
            missing.push(Missing {
                importer: layer.name.clone(),
                import: import.name.clone(),
                source: import.source.clone(),
                line: at
                    .as_ref()
                    .map(|(source, version)| line_for(source, version, import)),
            });
        }
    }
    missing
}

/// Fetch one pack, check it against what is installed, move it in and pin it —
/// with every import the same checkout holds that nothing installed answers.
///
/// `fetched` is the caller's own UTC stamp: this crate reads no clock, the same
/// way it resolves no machine directory.
///
/// Nothing is written outside `packs_dir` and `lock_path`, and a refusal at any
/// step leaves both as they were — the fetch lands in a scratch directory that
/// is removed on every path out, and the pack and its imports go in together
/// or not at all.
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

    let manifest = check_format(&candidate)?;
    let name = manifest.name.clone();
    let mut going = vec![(name.clone(), candidate.clone(), source_as_given)];

    // The imports the same checkout holds come in with it, out of this clone;
    // the rest are named. An import already installed is already answered.
    let installed: std::collections::BTreeSet<String> = resolve::installed(packs_dir)
        .into_iter()
        .map(|layer| layer.name)
        .collect();
    let mut missing = Vec::new();
    for import in &manifest.imports {
        if installed.contains(&import.name) {
            continue;
        }
        let not_here = |line: Option<String>| Missing {
            importer: name.clone(),
            import: import.name.clone(),
            source: import.source.clone(),
            line,
        };
        let at = match locate_import(&parsed, &import.source) {
            Located::Inside(at) => at,
            Located::Elsewhere(_) => {
                missing.push(not_here(Some(line_for(&parsed, &version_as_given, import))));
                continue;
            }
        };
        let root = match &at.subdir {
            None => scratch.root.clone(),
            Some(subdir) => scratch.root.join(subdir),
        };
        // A directory that holds the importer is not a pack beside it, and
        // one with no manifest is no pack at all.
        if candidate.starts_with(&root) || !root.join(pack::MANIFEST).is_file() {
            missing.push(not_here(None));
            continue;
        }
        let found = check_format(&root)?.name;
        if found != import.name {
            return Err(vec![Refusal::ImportName {
                importer: name.clone(),
                import: import.name.clone(),
                source: import.source.clone(),
                found,
            }]);
        }
        going.push((found, root, at.to_string()));
    }

    for (name, _, _) in &going {
        let destination = packs_dir.join(name);
        if destination.exists() {
            return Err(vec![Refusal::AlreadyInstalled {
                name: name.clone(),
                root: destination.display().to_string(),
            }]);
        }
    }
    let laid: Vec<(&str, &Path)> = going
        .iter()
        .map(|(name, root, _)| (name.as_str(), root.as_path()))
        .collect();
    check_layering(packs_dir, defaults_dir, &laid)?;

    // The imports move first: one the checkout holds inside the importer's
    // own directory would otherwise move with it.
    let mut moved: Vec<PathBuf> = Vec::new();
    for (name, root, _) in going.iter().rev() {
        let destination = packs_dir.join(name);
        if let Err(e) = std::fs::rename(root, &destination) {
            for done in &moved {
                let _ = std::fs::remove_dir_all(done);
            }
            return Err(vec![Refusal::Install(e.to_string())]);
        }
        moved.push(destination);
    }

    // No tree hash: these packs came from a repository and its commit already
    // says which bytes they are. The key belongs to the pack that has no commit.
    let entries: Vec<lock::Entry> = going
        .iter()
        .map(|(name, _, source)| lock::Entry {
            source: source.clone(),
            name: Some(name.clone()),
            version: version_as_given.clone(),
            commit: commit.clone(),
            fetched: fetched.to_string(),
            tree: None,
        })
        .collect();
    if let Err(refusal) = pin(lock_path, &entries) {
        for done in &moved {
            let _ = std::fs::remove_dir_all(done);
        }
        return Err(vec![refusal]);
    }

    let mut all = entries.into_iter().map(|entry| {
        let name = entry.name.clone().unwrap_or_default();
        Installed {
            root: packs_dir.join(&name),
            name,
            entry,
            imports: Vec::new(),
            missing: Vec::new(),
        }
    });
    let mut added = all.next().expect("the pack itself is always going");
    added.imports = all.collect();
    added.missing = missing;
    Ok(added)
}

/// Write the pins in one document and read each back. The read is what the
/// verb exits 0 on: a write that reported success and left nothing readable — a
/// lock path that swallows its bytes, a filesystem that lied — is the case a
/// returned `Ok` cannot rule out, and the only one this catches.
fn pin(lock_path: &Path, entries: &[lock::Entry]) -> Result<(), Refusal> {
    lock::append_all(lock_path, entries).map_err(Refusal::Lock)?;
    for entry in entries {
        match lock::holds(lock_path, entry) {
            Ok(true) => {}
            Ok(false) => return Err(Refusal::LockDidNotLand(entry.source.clone())),
            Err(e) => return Err(Refusal::Lock(e)),
        }
    }
    Ok(())
}

/// The candidate's own format, and its manifest, whose name the rest of the
/// verb calls it by.
fn check_format(candidate: &Path) -> Result<pack::Manifest, Vec<Refusal>> {
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
    // A report with no defects read a manifest; one without is named as the
    // pack with no name it is.
    report.manifest.ok_or_else(|| vec![Refusal::PackName(name)])
}

/// The candidates among everything already installed, laid as the resolver lays
/// them: every pack above what it imports, the binary's defaults last.
///
/// The layering is the imports', never the installs': a candidate an installed
/// pack imports goes beneath that importer, so the packs may be added in any
/// order. The defaults are the bottom layer whatever is installed, which is what
/// makes the shadow registry answer for a pack added into an empty packs dir.
fn check_layering(
    packs_dir: &Path,
    defaults_dir: &Path,
    candidates: &[(&str, &Path)],
) -> Result<(), Vec<Refusal>> {
    let mut packs = resolve::installed(packs_dir);
    for (name, root) in candidates {
        packs.push(resolve::Layer::new(*name, *root));
    }
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
