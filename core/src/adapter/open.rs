//! The half of opening an adapter that is every adapter's: the setting read,
//! a path or a name resolved through the installed packs, and the `PATH` a
//! pack's adapter runs on — and every refusal of the setting, worded once and
//! naming the adapter's kind. Each contract's own opener hands [`resolve`] a
//! [`Wanted`] and runs what it answers as its own executable.

use std::path::{Path, PathBuf};

use crate::pack::{self, AdapterKind};

/// Where the `[store] adapter` an [`Opening`](crate::store::Opening) opens by
/// was written — or the agent's `[agent] adapter`, whose opener takes the same
/// answer — so a refusal names the thing a person wrote and not a key they
/// never did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterSource {
    /// `[store] adapter` in the project's own file.
    Setting,
    /// `fleet store check --adapter`, carried into the policy as the setting.
    Flag,
}

impl AdapterSource {
    /// What the person wrote, as a refusal quotes it.
    fn written(self, kind: AdapterKind) -> String {
        match self {
            AdapterSource::Setting => format!("[{}] adapter", kind.as_str()),
            AdapterSource::Flag => String::from("--adapter"),
        }
    }
}

/// The installed packs and the binary's defaults beneath them, as
/// [`Packs::under`](crate::item::brief::Packs::under) reads them.
///
/// DIRECTORIES AND NOT A RESOLUTION: the layers are resolved only when the
/// setting is a name, so an adapter's path opens whatever the packs hold, a
/// layering that refuses included.
#[derive(Debug, Clone, Copy)]
pub struct PackDirs<'a> {
    pub packs_dir: &'a Path,
    pub defaults_dir: &'a Path,
}

/// What a contract's opener hands the resolution: the kind, the file the key
/// is read from, where it was written, the `PATH` a named adapter runs on
/// (empty: this process's own), the packs, and the name an absent key opens.
pub struct Wanted<'a> {
    /// The kind of adapter wanted: the key `[<kind>] adapter` is read under,
    /// and the words every refusal names it by.
    pub kind: AdapterKind,
    /// The file the key is read out of, as the store's
    /// [`Opening::policy`](crate::store::Opening::policy) is.
    pub policy: &'a toml::Table,
    /// Where the key was written, as the store's
    /// [`Opening::source`](crate::store::Opening::source) is.
    pub source: AdapterSource,
    /// The `PATH` a pack's adapter runs on, as the store's
    /// [`Opening::search_path`](crate::store::Opening::search_path) is.
    pub search_path: &'a str,
    /// Where a bare name is resolved, as the store's
    /// [`Opening::packs`](crate::store::Opening::packs) is.
    pub packs: Option<PackDirs<'a>>,
    /// The name a file that writes no key opens, resolved as a name.
    pub default: &'a str,
}

/// An adapter resolved and not yet run: what its contract's opener runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the adapter is called by: the path as written for a path, else
    /// the name.
    pub named: String,
    /// The executable that answers for the adapter.
    pub entry: PathBuf,
    /// The directory holding the adapter's own `adapter.toml`: a pack's
    /// adapter directory, or a path's parent where one sits beside it.
    pub dir: Option<PathBuf>,
    /// The `PATH` to run it on, or `None` for this process's own.
    pub path: Option<String>,
}

/// The adapter of the wanted kind that `[<kind>] adapter` names: an absolute
/// path to an executable file is the adapter at that path; a name — or no key
/// at all, which is [`Wanted::default`] — is the adapter of the wanted kind
/// the installed packs carry under `name`. Anything else is refused, naming
/// what was written where it was ([`AdapterSource`]), and nothing is run.
///
/// A NAME'S ADAPTER: the highest layer holding
/// `adapters/<kind>/<name>/adapter.toml` carries the adapter WHOLE, so its
/// entry is the file beside that one and never another layer's.
///
/// It runs on [`Wanted::search_path`], with the directory of each runtime
/// that layer runs under — its own `[runtime]`, else the ones the packs it
/// imports declare — put in front where the path misses it.
pub fn resolve(wanted: &Wanted) -> Result<Resolved, String> {
    let refused = |why: Unopened| unopened(wanted.kind, wanted.source, why);
    // Each table is written out, so the census test reads both pairs here.
    let named = match wanted.kind {
        AdapterKind::Store => crate::policy::read("store", "adapter", wanted.policy),
        AdapterKind::Agent => crate::policy::read("agent", "adapter", wanted.policy),
    }
    .map_err(|unlisted| unlisted.to_string())?;
    let name = match named {
        None => wanted.default,
        Some(toml::Value::String(path)) if path.starts_with('/') => {
            let adapter = Path::new(path);
            if !crate::process::is_executable_file(adapter) {
                return Err(refused(Unopened::NotExecutable(path)));
            }
            let dir = adapter
                .parent()
                .filter(|dir| dir.join(pack::ADAPTER_MANIFEST).is_file())
                .map(Path::to_path_buf);
            return Ok(Resolved {
                named: path.clone(),
                entry: adapter.to_path_buf(),
                dir,
                path: None,
            });
        }
        Some(toml::Value::String(name)) if !name.is_empty() && !name.contains('/') => name.as_str(),
        Some(toml::Value::String(other)) => {
            return Err(refused(Unopened::NeitherForm(other.clone())));
        }
        Some(other) => return Err(refused(Unopened::NeitherForm(other.to_string()))),
    };
    let Some(installed) = wanted.packs else {
        return Err(refused(Unopened::NoPacks(name)));
    };
    // A layering that REFUSES is refused, the default's name included — which
    // adapter the fleet runs is then exactly what cannot be told.
    let packs = crate::item::brief::Packs::under(installed.packs_dir, installed.defaults_dir)
        .map_err(|stop| refused(Unopened::Layers(name, stop.message)))?;
    let Some((carrier, dir)) = pack::adapter_dir(&packs, wanted.kind, name) else {
        return Err(refused(Unopened::Nowhere(name)));
    };
    // The manifest is held to the format here, its entry's executable bit
    // included, so an adapter `fleet pack check` refuses is never run.
    let manifest =
        pack::adapter_manifest(&dir).map_err(|defect| refused(Unopened::Defect(name, defect)))?;
    let entry = dir.join(&manifest.entry);
    let path = if wanted.search_path.is_empty() {
        None
    } else {
        Some(
            crate::runtime::adapter_path(carrier, &packs.layers, wanted.search_path)
                .map_err(|stop| refused(Unopened::Unrun(name, stop.message)))?,
        )
    };
    Ok(Resolved {
        named: name.to_string(),
        entry,
        dir: Some(dir),
        path,
    })
}

/// Why the adapter `[store] adapter` or `[agent] adapter` names is not
/// opened, wherever the setting was written.
enum Unopened<'s> {
    NotExecutable(&'s str),
    NeitherForm(String),
    NoPacks(&'s str),
    Layers(&'s str, String),
    Nowhere(&'s str),
    Defect(&'s str, pack::Defect),
    Unrun(&'s str, String),
}

/// EVERY REFUSAL OF THE SETTING IS WORDED HERE, and nowhere else, so what a
/// refusal says about where the setting came from is said once.
fn unopened(kind: AdapterKind, source: AdapterSource, why: Unopened) -> String {
    let written = source.written(kind);
    let named = kind.as_str();
    match why {
        Unopened::NotExecutable(path) => {
            format!("{written} names `{path}`, which is not an executable file")
        }
        Unopened::NeitherForm(said) => format!(
            "{written} is `{said}` — it is the name of {} an installed pack carries, or an \
             absolute path to an adapter executable",
            kind.indefinite()
        ),
        Unopened::NoPacks(name) => format!(
            "no {named} adapter named `{name}` resolves: no packs are installed here to carry one"
        ),
        Unopened::Layers(name, why) => {
            format!("no {named} adapter named `{name}` resolves: {why}")
        }
        Unopened::Nowhere(name) => format!(
            "no {named} adapter named `{name}` in the installed packs — `{}` installs the one \
             fleet-packs carries",
            kind.pack_line(
                crate::supported::PINNED_PACKS_SOURCE,
                name,
                crate::supported::PINNED_PACKS
            )
        ),
        Unopened::Defect(name, defect) => {
            format!("the {named} adapter `{name}` cannot be opened: {defect}")
        }
        Unopened::Unrun(name, why) => {
            format!("the {named} adapter `{name}` cannot be opened: {why}")
        }
    }
}

/// The refusals worded once, for both kinds: a value that is neither form, a
/// name with no packs behind it, and the line that installs a name.
#[cfg(test)]
mod tests {
    use super::*;

    fn wanted<'a>(kind: AdapterKind, policy: &'a toml::Table) -> Wanted<'a> {
        Wanted {
            kind,
            policy,
            source: AdapterSource::Setting,
            search_path: "",
            packs: None,
            default: "x",
        }
    }

    fn refused(wanted: &Wanted) -> String {
        match resolve(wanted) {
            Err(why) => why,
            Ok(resolved) => panic!("`{}` resolved", resolved.named),
        }
    }

    /// A value that is neither a name nor an absolute path is refused naming
    /// the key of its own kind and an adapter of that kind.
    #[test]
    fn a_value_of_neither_form_is_refused_naming_its_kind() {
        for (kind, article) in [
            (AdapterKind::Store, "a store adapter"),
            (AdapterKind::Agent, "an agent adapter"),
        ] {
            let policy: toml::Table = format!("[{}]\nadapter = 7\n", kind.as_str())
                .parse()
                .expect("the fixture policy parses");
            assert_eq!(
                refused(&wanted(kind, &policy)),
                format!(
                    "[{}] adapter is `7` — it is the name of {article} an installed pack \
                     carries, or an absolute path to an adapter executable",
                    kind.as_str()
                )
            );
        }
    }

    /// A name with no packs behind the caller resolves nowhere, naming the
    /// kind of adapter it was.
    #[test]
    fn a_name_with_no_packs_is_refused_naming_its_kind() {
        let policy = toml::Table::new();
        for kind in AdapterKind::ALL {
            assert_eq!(
                refused(&wanted(kind, &policy)),
                format!(
                    "no {} adapter named `x` resolves: no packs are installed here to carry one",
                    kind.as_str()
                )
            );
        }
    }

    /// The line that installs a name is filed under its kind's directory.
    #[test]
    fn the_pack_line_names_the_kinds_directory() {
        assert_eq!(
            AdapterKind::Store.pack_line("R", "x", "v1"),
            "fleet pack add R//adapters/store/x --version v1"
        );
        assert_eq!(
            AdapterKind::Agent.pack_line("R", "x", "v1"),
            "fleet pack add R//adapters/agent/x --version v1"
        );
    }
}
