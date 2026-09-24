//! The layers: an ordered list of pack roots, top first, resolved to one file
//! per relative path.
//!
//! The caller assembles the list — the fleet's own pack, then its imports in
//! the order its manifest declares them, then the binary's defaults last. This
//! resolver never reads a manifest to find its imports, and refuses one level
//! down: a listed import declaring imports of its own is named rather than
//! followed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::pack;
use crate::registry;

/// One pack root, with the name a refusal calls it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub root: PathBuf,
    /// The binary's own defaults, which carry no manifest: the bottom layer is a
    /// directory this binary wrote, not a pack somebody installed, so the
    /// import rules below read past it.
    pub defaults: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Layer {
            name: name.into(),
            root: root.into(),
            defaults: false,
        }
    }

    /// The bottom layer: the directory [`crate::defaults`] materialized.
    pub fn defaults(root: impl Into<PathBuf>) -> Self {
        Layer {
            name: crate::defaults::LAYER.to_string(),
            root: root.into(),
            defaults: true,
        }
    }
}

/// A path a higher layer carries and a lower one also holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadowed {
    pub path: String,
    pub over: String,
    pub under: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolution {
    /// Every relative path under the eight slots, against the layer that
    /// carries it: the highest one that holds the path.
    pub files: BTreeMap<String, String>,
    /// Agent directory names, against the layer that carries each. Unique
    /// across every layer, or the resolution refused.
    pub agents: BTreeMap<String, String>,
    pub shadowed: Vec<Shadowed>,
}

/// Any one of these refuses the whole resolution: a layering that is wrong
/// anywhere resolves to nothing, never to a best effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Unreadable {
        layer: String,
        error: String,
    },
    Manifest {
        layer: String,
        defect: String,
    },
    TransitiveImport {
        layer: String,
        import: String,
    },
    AgentNameCollision {
        agent: String,
        upper: String,
        lower: String,
    },
    UnlistedShadow {
        path: String,
        pack: String,
    },
    Registry {
        pack: String,
        error: String,
    },
    ImportCycle {
        packs: Vec<String>,
    },
    /// An installed pack whose own name is the bottom layer's. A resolution
    /// names the layer that carries each path BY NAME, so two layers under one
    /// name resolve a path to whichever of them sits higher — which for a file
    /// only the defaults hold is the wrong root.
    DefaultsName {
        root: String,
    },
    /// The binary's defaults have not been materialized. Named rather than
    /// resolved as an empty bottom layer, because an empty bottom publishes no
    /// registry and checks nothing.
    NoDefaults {
        root: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Unreadable { layer, error } => {
                write!(f, "layer `{layer}` cannot be read: {error}")
            }
            Refusal::Manifest { layer, defect } => {
                write!(f, "layer `{layer}`: {defect}")
            }
            Refusal::TransitiveImport { layer, import } => write!(
                f,
                "layer `{layer}` declares its own import `{import}` — \
                 imports are one level deep"
            ),
            Refusal::AgentNameCollision {
                agent,
                upper,
                lower,
            } => write!(
                f,
                "the agent name `{agent}` is in both `{upper}` and `{lower}` — \
                 a collision is refused, never resolved by precedence"
            ),
            Refusal::UnlistedShadow { path, pack } => write!(
                f,
                "`{path}` shadows `{pack}` and is not listed in {}",
                registry::REGISTRY
            ),
            Refusal::Registry { pack, error } => {
                write!(f, "`{pack}`'s {}: {error}", registry::REGISTRY)
            }
            Refusal::ImportCycle { packs } => write!(
                f,
                "the installed packs import each other in a cycle — {} — and no order \
                 puts every pack before the ones it imports",
                packs.join(", ")
            ),
            Refusal::DefaultsName { root } => write!(
                f,
                "the pack at {root} calls itself `{}`, which is the binary's own bottom \
                 layer — rename it or take it out",
                crate::defaults::LAYER
            ),
            Refusal::NoDefaults { root } => write!(
                f,
                "the defaults this binary carries are not at {root} — `fleet start` \
                 writes them, and every template resolves through them"
            ),
        }
    }
}

/// The layers a verb resolves through: the installed packs in import order, the
/// binary's own defaults appended last.
///
/// THE ONE PLACE THE BOTTOM IS PLACED. The registry rule reads the BOTTOM
/// layer's declaration of its own surface, so a caller that assembled the list
/// itself and forgot resolves with a real pack as the bottom — and then every
/// shadow that pack does not list is refused, or, where it publishes no
/// registry, nothing is checked at all.
pub fn layers(packs_dir: &Path, defaults_dir: &Path) -> Result<Vec<Layer>, Vec<Refusal>> {
    layers_over(installed(packs_dir), defaults_dir)
}

/// The same, over a list the caller assembled — `add` lays a candidate that is
/// not installed yet — so the bottom is still placed in one function.
///
/// AN ABSENT DEFAULTS DIRECTORY IS A REFUSAL and never an empty layer: a
/// directory that is not there walks to no files, publishes no registry, and
/// turns the shadow rule off without saying so, which reads downstream as a
/// template that is simply missing.
pub fn layers_over(packs: Vec<Layer>, defaults_dir: &Path) -> Result<Vec<Layer>, Vec<Refusal>> {
    if !defaults_dir.is_dir() {
        return Err(vec![Refusal::NoDefaults {
            root: defaults_dir.display().to_string(),
        }]);
    }
    let mut layers = ordered(packs)?;
    layers.push(Layer::defaults(defaults_dir));
    Ok(layers)
}

/// Every pack in the packs directory, named by its own manifest.
///
/// A dotted entry is scratch from an add in flight and never a pack; a
/// directory with no manifest is not one either.
pub fn installed(packs_dir: &Path) -> Vec<Layer> {
    let mut layers = Vec::new();
    for entry in pack::dir_names(packs_dir).unwrap_or_default() {
        if entry.starts_with('.') {
            continue;
        }
        let root = packs_dir.join(&entry);
        if !root.join(pack::MANIFEST).is_file() {
            continue;
        }
        let name = std::fs::read_to_string(root.join(pack::MANIFEST))
            .ok()
            .and_then(|text| pack::parse_manifest(&text).ok())
            .map(|m| m.name)
            .unwrap_or(entry);
        layers.push(Layer::new(name, root));
    }
    layers
}

/// The installed packs in the order [`resolve`] wants them: every pack before
/// each pack it imports, ties broken by name.
///
/// THE DEFAULTS ARE NOT ORDERED HERE. The registry rule reads the BOTTOM
/// layer's declaration of its own surface, and the bottom is the binary's own
/// defaults whatever is installed — so the resolver's caller appends that layer
/// after this returns rather than letting a name sort it.
///
/// An import naming a pack that is not installed is not this function's
/// subject: it orders what is here, and `resolve` is what refuses a layering.
pub fn ordered(layers: Vec<Layer>) -> Result<Vec<Layer>, Vec<Refusal>> {
    let mut rest = layers;
    rest.sort_by(|a, b| a.name.cmp(&b.name));

    let imports: BTreeMap<String, BTreeSet<String>> = rest
        .iter()
        .map(|layer| (layer.name.clone(), imports_of(&layer.root)))
        .collect();

    let mut placed: Vec<Layer> = Vec::new();
    let mut waiting = rest;
    while !waiting.is_empty() {
        // A pack nobody still waiting imports may go next: everything that
        // imports it is already placed above it.
        let next = waiting.iter().position(|candidate| {
            !waiting.iter().any(|other| {
                other.name != candidate.name
                    && imports_of_name(&imports, &other.name).contains(&candidate.name)
            })
        });
        match next {
            Some(index) => placed.push(waiting.remove(index)),
            None => {
                return Err(vec![Refusal::ImportCycle {
                    packs: waiting.into_iter().map(|l| l.name).collect(),
                }])
            }
        }
    }
    Ok(placed)
}

/// The absolute path a relative slot path resolves to, or none.
///
/// The resolution names the LAYER that carries each path and not the file, so
/// the layers are read here against that name: a pack on top holding the same
/// path is the one this answers with, which is what makes any of the four
/// templates shadowable whole.
pub fn slot_path(resolution: &Resolution, layers: &[Layer], relative: &str) -> Option<PathBuf> {
    let carrier = resolution.files.get(relative)?;
    let layer = layers.iter().find(|layer| &layer.name == carrier)?;
    Some(layer.root.join(relative))
}

fn imports_of(root: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(root.join(pack::MANIFEST))
        .ok()
        .and_then(|text| pack::parse_manifest(&text).ok())
        .map(|manifest| manifest.imports.into_iter().map(|i| i.name).collect())
        .unwrap_or_default()
}

fn imports_of_name<'m>(
    imports: &'m BTreeMap<String, BTreeSet<String>>,
    name: &str,
) -> &'m BTreeSet<String> {
    static NONE: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    imports
        .get(name)
        .unwrap_or_else(|| NONE.get_or_init(BTreeSet::new))
}

pub fn resolve(layers: &[Layer]) -> Result<Resolution, Vec<Refusal>> {
    let mut refusals = Vec::new();
    let mut carried: Vec<BTreeSet<String>> = Vec::new();
    let mut agents: Vec<BTreeSet<String>> = Vec::new();

    for layer in layers {
        match walk_slots(&layer.root) {
            Ok(files) => carried.push(files),
            Err(error) => {
                refusals.push(Refusal::Unreadable {
                    layer: layer.name.clone(),
                    error,
                });
                carried.push(BTreeSet::new());
            }
        }
        agents.push(
            pack::dir_names(&layer.root.join("agents"))
                .unwrap_or_default()
                .into_iter()
                .filter(|name| layer.root.join("agents").join(name).is_dir())
                .collect(),
        );
    }

    for layer in layers.iter().filter(|layer| !layer.defaults) {
        if layer.name == crate::defaults::LAYER {
            refusals.push(Refusal::DefaultsName {
                root: layer.root.display().to_string(),
            });
        }
    }

    // The top layer is the one whose imports the caller already walked; every
    // pack beneath it is one of those imports and may declare none of its own at
    // this depth. The defaults layer carries no manifest at all and is read
    // past: it is a directory this binary wrote, not a pack.
    for layer in layers.iter().skip(1).filter(|layer| !layer.defaults) {
        match std::fs::read_to_string(layer.root.join(pack::MANIFEST)) {
            Ok(text) => match pack::parse_manifest(&text) {
                Ok(manifest) => {
                    for import in manifest.imports {
                        refusals.push(Refusal::TransitiveImport {
                            layer: layer.name.clone(),
                            import: import.name,
                        });
                    }
                }
                Err(defects) => refusals.extend(defects.into_iter().map(|d| Refusal::Manifest {
                    layer: layer.name.clone(),
                    defect: d.to_string(),
                })),
            },
            Err(e) => refusals.push(Refusal::Manifest {
                layer: layer.name.clone(),
                defect: e.to_string(),
            }),
        }
    }

    for (upper, names) in agents.iter().enumerate() {
        for name in names {
            if let Some(lower) = agents
                .iter()
                .enumerate()
                .skip(upper + 1)
                .find(|(_, lower)| lower.contains(name))
                .map(|(i, _)| i)
            {
                refusals.push(Refusal::AgentNameCollision {
                    agent: name.clone(),
                    upper: layers[upper].name.clone(),
                    lower: layers[lower].name.clone(),
                });
            }
        }
    }

    let mut resolution = Resolution::default();
    for (index, files) in carried.iter().enumerate() {
        for path in files {
            match resolution.files.get(path) {
                Some(over) => resolution.shadowed.push(Shadowed {
                    path: path.clone(),
                    over: over.clone(),
                    under: layers[index].name.clone(),
                }),
                None => {
                    resolution
                        .files
                        .insert(path.clone(), layers[index].name.clone());
                }
            }
        }
    }
    for (index, names) in agents.iter().enumerate() {
        for name in names {
            resolution
                .agents
                .entry(name.clone())
                .or_insert_with(|| layers[index].name.clone());
        }
    }

    refusals.extend(unlisted_shadows(layers, &carried));

    if refusals.is_empty() {
        Ok(resolution)
    } else {
        Err(refusals)
    }
}

/// The bottom layer's registry is the whole surface a pack above it may
/// replace. A bottom layer publishing no registry is not this rule's subject:
/// nothing has declared what may be shadowed, so nothing is refused.
fn unlisted_shadows(layers: &[Layer], carried: &[BTreeSet<String>]) -> Vec<Refusal> {
    if layers.len() < 2 {
        return Vec::new();
    }
    let bottom = layers.len() - 1;
    let base = &layers[bottom];
    let registry = match registry::read(&base.root) {
        None => return Vec::new(),
        Some(Ok(registry)) => registry,
        Some(Err(e)) => {
            return vec![Refusal::Registry {
                pack: base.name.clone(),
                error: match e {
                    registry::RegistryError::Unparsable(e) => e,
                    registry::RegistryError::Schema(n) => format!("schema is {n}"),
                    registry::RegistryError::Key(k) => format!("a [[shadow]] is missing `{k}`"),
                },
            }]
        }
    };

    let mut refusals = Vec::new();
    for path in &carried[bottom] {
        // The registry is a pack's declaration of its own surface, not part of
        // the surface: every pack that publishes one carries it at this path,
        // so reading a higher layer's copy as a shadow would refuse the normal
        // case.
        if path == registry::REGISTRY {
            continue;
        }
        let shadowing = carried[..bottom].iter().any(|files| files.contains(path));
        if shadowing && !registry.lists(path) {
            refusals.push(Refusal::UnlistedShadow {
                path: path.clone(),
                pack: base.name.clone(),
            });
        }
    }
    refusals
}

/// Every file under the eight slots, as a path relative to the pack root. The
/// manifest is not walked: it is the pack's identity and is never shadowed.
fn walk_slots(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = BTreeSet::new();
    for slot in pack::SLOTS {
        let dir = root.join(slot);
        if dir.is_dir() {
            walk(&dir, slot, &mut files)?;
        }
    }
    Ok(files)
}

fn walk(dir: &Path, prefix: &str, into: &mut BTreeSet<String>) -> Result<(), String> {
    for name in pack::dir_names(dir)? {
        let path = dir.join(&name);
        let relative = format!("{prefix}/{name}");
        if path.is_dir() {
            walk(&path, &relative, into)?;
        } else {
            into.insert(relative);
        }
    }
    Ok(())
}
