//! The runtime a pack's files run under — the `[runtime]` table a layer pins
//! or imports — and the `PATH` whose children find it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::add;
use crate::item::Stop;
use crate::lock;
use crate::pack::{self, ManifestRead, Runtime};
use crate::resolve::Layer;

/// The `[runtime]` table a run bundles and executes under, and the layer that
/// declares it — the carrier's own where it has one, else the one pack the
/// carrier imports that has one.
///
/// The DECLARING LAYER and not just the table, because the doctor check
/// measures a pack's manifest and the check's verdict is about that pack: a
/// carrier that imports its runtime has nothing pinned in its own manifest, so
/// a check run against it would read "nothing pinned" and answer green about
/// a runtime it never measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pinned {
    pub(crate) runtime: Runtime,
    pub(crate) pack: Layer,
}

pub(crate) fn manifest_of(layer: &Layer) -> Result<pack::Manifest, Stop> {
    pack::read_manifest(&layer.root).map_err(|read| match read {
        ManifestRead::Unreadable(e) => Stop::could_not_tell(format!(
            "{} could not be read: {e}",
            layer.root.join(pack::MANIFEST).display()
        )),
        ManifestRead::Defects(defects) => Stop::refused(format!(
            "the pack `{}` does not parse: {}",
            layer.name,
            defects
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )),
    })
}

/// The runtime the carrier's workflow runs under: the carrier's own `[runtime]`
/// table first, else the nearest layer beneath it, in the resolved order, among
/// the packs the carrier imports — transitively, though the resolver holds
/// imports to one level — that declares one.
///
/// THE CARRIER'S OWN PIN WINS OUTRIGHT and the imports are not read: a pack
/// that pinned a runtime said what its workflows run under, and an import's
/// pin beneath it is that import's business. Beneath a carrier with none, two
/// declaring imports are a refusal naming both, never the higher one: core is
/// not the one to choose between two languages a pack asked for at once.
///
/// NONE AT ALL, where an import the carrier declares is not installed, names
/// that import and the line that adds it (fleet-4fw): the missing pack is the
/// likeliest carrier of the table, and the person reading the refusal is the
/// one who can add it. `machine_dir` is where the lock that line is read off
/// lives.
pub(crate) fn read_the_runtime(
    pack: &Layer,
    relative: &str,
    layers: &[Layer],
    machine_dir: &Path,
) -> Result<Pinned, Stop> {
    let (mut declaring, imported) = declaring(pack, layers)?;
    match declaring.len() {
        0 => {
            let unanswered: Vec<String> =
                add::missing_imports(layers, &machine_dir.join(lock::LOCK))
                    .into_iter()
                    .filter(|missing| {
                        missing.importer == pack.name
                            || imported.contains(&missing.importer)
                    })
                    .map(|missing| missing.to_string())
                    .collect();
            if unanswered.is_empty() {
                Err(Stop::refused(format!(
                    "`{}` carries `{}` and declares no [runtime] table, and no pack it imports \
                     declares one — that table is the one thing fleet reads about a workflow's \
                     language, so there is no command to bundle this file with",
                    pack.name, relative
                )))
            } else {
                Err(Stop::refused(format!(
                    "`{}` carries `{}` and declares no [runtime] table, and no installed pack it \
                     imports declares one: {}",
                    pack.name,
                    relative,
                    unanswered.join("; ")
                )))
            }
        }
        1 => Ok(declaring.remove(0)),
        _ => Err(Stop::refused(format!(
            "`{}` carries `{}` and declares no [runtime] table, and {} packs it imports each \
             declare one — {} — so the file has two runtimes and fleet does not choose between them",
            pack.name,
            relative,
            declaring.len(),
            declaring
                .iter()
                .map(|pinned| format!("`{}`", pinned.pack.name))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The `PATH` the doctor check, the bundle line and the run line all run
/// under: the caller's constructed child path, with the directory the pinned
/// runtime resolves from in front of it when that path does not already hold
/// the binary.
///
/// CONSTRUCTED, NEVER COPIED. A controller re-running a run is a service, and
/// a service's own `PATH` is the manager's — it holds neither a package
/// manager's prefix nor the user's local bin — so a run line handed that
/// `PATH` cannot exec the runtime its pack pins. The base comes from the
/// caller because the platform's directories are the caller's knowledge; an
/// empty base is a caller with no controller behind it, and the children then
/// carry this process's own `PATH` as they always have.
///
/// THE PREPEND MIRRORS THE PINNED RUNTIME'S DOCTOR CHECK, which looks for the
/// binary on `PATH` and then in the installer's bin — `<NAME>_INSTALL/bin`
/// where that variable names a root, else `$HOME/.<name>/bin`, which no
/// session `PATH` carries on its own. The check and the two lines it clears
/// have to resolve the same file, so the directory the check would accept is
/// put where the lines will look. Only when the base misses it: a runtime the
/// base already resolves keeps the platform's order, where the system
/// directories lead.
pub fn child_path_for(runtime: &str, base: &str) -> String {
    if base.is_empty() {
        return std::env::var("PATH").unwrap_or_default();
    }
    let name = runtime;
    if crate::process::holding(base, name).is_some() {
        return base.to_string();
    }
    let found =
        crate::process::holding(&std::env::var("PATH").unwrap_or_default(), name).or_else(|| {
            installer_bin(name).filter(|bin| crate::process::is_executable_file(&bin.join(name)))
        });
    match found {
        Some(dir) => format!("{}:{base}", dir.display()),
        None => base.to_string(),
    }
}

/// Every layer whose `[runtime]` the files `carrier` carries run under, by the
/// rule [`read_the_runtime`] reads a workflow's by: the carrier's own table
/// alone where it has one, else each pack it imports that declares one, in the
/// resolved order — and the names the carrier imports, which a refusal of none
/// reads the missing ones against.
fn declaring(carrier: &Layer, layers: &[Layer]) -> Result<(Vec<Pinned>, BTreeSet<String>), Stop> {
    let own = manifest_of(carrier)?;
    if let Some(runtime) = own.runtime {
        let pinned = Pinned {
            runtime,
            pack: carrier.clone(),
        };
        return Ok((vec![pinned], BTreeSet::new()));
    }

    let beneath = layers
        .iter()
        .position(|layer| layer.name == carrier.name)
        .map(|at| at + 1)
        .unwrap_or(layers.len());
    let mut imported: BTreeSet<String> = own.imports.into_iter().map(|i| i.name).collect();
    let mut frontier: Vec<String> = imported.iter().cloned().collect();
    while let Some(name) = frontier.pop() {
        let Some(layer) = layers.iter().find(|layer| layer.name == name) else {
            continue;
        };
        for import in manifest_of(layer)?.imports {
            if imported.insert(import.name.clone()) {
                frontier.push(import.name);
            }
        }
    }

    let mut declaring: Vec<Pinned> = Vec::new();
    for layer in layers.iter().skip(beneath) {
        if !imported.contains(&layer.name) {
            continue;
        }
        if let Some(runtime) = manifest_of(layer)?.runtime {
            declaring.push(Pinned {
                runtime,
                pack: layer.clone(),
            });
        }
    }
    Ok((declaring, imported))
}

/// The names of the runtimes a file `carrier` carries runs under, as
/// [`declaring`] finds them: none for a pack that declares none and imports
/// none that does, which is a file that execs no runtime of a pack's.
///
/// What the store's opener and the agent's put in front of an adapter's search
/// path, through [`child_path_for`], so a pack's adapter finds the runtime its
/// entry execs where a run line would.
pub fn runtimes_of(carrier: &Layer, layers: &[Layer]) -> Result<Vec<String>, Stop> {
    let (declaring, _) = declaring(carrier, layers)?;
    Ok(declaring
        .into_iter()
        .map(|pinned| pinned.runtime.name)
        .collect())
}

/// The `PATH` an adapter carried by `carrier` runs on: `base`, with the
/// directory of each runtime [`runtimes_of`] names put in front where `base`
/// misses it, as a run line's is ([`child_path_for`]). Both adapter openers
/// call it, the store's and the agent's.
pub fn adapter_path(carrier: &Layer, layers: &[Layer], base: &str) -> Result<String, Stop> {
    Ok(runtimes_of(carrier, layers)?
        .iter()
        .fold(base.to_string(), |path, runtime| {
            child_path_for(runtime, &path)
        }))
}

/// The installer's bin for a runtime of this name, as its own doctor check
/// spells it.
fn installer_bin(name: &str) -> Option<PathBuf> {
    let variable: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .chain("_INSTALL".chars())
        .collect();
    let root = match std::env::var(&variable) {
        Ok(named) if !named.trim().is_empty() => PathBuf::from(named),
        _ => PathBuf::from(std::env::var("HOME").ok()?).join(format!(".{name}")),
    };
    Some(root.join("bin"))
}
