//! A pack's settings: the keys its manifest declares under `[config.<key>]`,
//! and the values a fleet sets for them in `fleet.toml` under the pack's name.
//!
//! ```toml
//! [packs.tiny]
//! takeoff.test = "cargo nextest run --workspace"
//! ```
//!
//! THE DECLARATION IS THE CENSUS. A key under `[packs.<name>]` the installed
//! pack does not declare is refused by name, and so is a section for a pack
//! that is not installed, for the reason [`crate::policy`] answers an unlisted
//! pair with an error: a value nothing reads is a setting a person believes is
//! in force and is not. The whole `[packs]` table is judged at once, against
//! every installed pack, so a section left behind by a pack taken out is named
//! the first time anything reads the table rather than never.
//!
//! What a workflow reads is the RESOLVED table of the pack that carries it —
//! each declared default, with every value the fleet set laid over it — and a
//! run pins that table beside its inputs, so a re-run reads the values the run
//! was opened with and not whatever the file says today.

use std::collections::BTreeMap;
use std::fmt;

use crate::pack::{self, Setting};
use crate::resolve::Layer;

/// The table in `fleet.toml` a pack's section sits under.
pub const TABLE: &str = "packs";

/// One thing wrong with `[packs]` against the installed packs. Every variant
/// names the section, and the key where there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// `[packs]` itself, or one pack's section, written as something other
    /// than a table.
    NotATable { at: String },
    NotInstalled {
        pack: String,
        installed: Vec<String>,
    },
    Undeclared {
        pack: String,
        key: String,
        declared: Vec<String>,
    },
    WrongType {
        pack: String,
        key: String,
        kind: String,
        found: String,
    },
    /// An installed pack whose manifest does not parse, so what it declares
    /// cannot be read.
    Manifest { pack: String, defect: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NotATable { at } => write!(f, "`{at}` in fleet.toml is not a table"),
            Refusal::NotInstalled { pack, installed } => write!(
                f,
                "fleet.toml has a [{TABLE}.{pack}] section and no pack named `{pack}` is \
                 installed — the installed packs are {}",
                listed(installed)
            ),
            Refusal::Undeclared {
                pack,
                key,
                declared,
            } => write!(
                f,
                "[{TABLE}.{pack}] sets `{key}`, which the pack `{pack}` does not declare — its \
                 pack.toml declares {}",
                listed(declared)
            ),
            Refusal::WrongType {
                pack,
                key,
                kind,
                found,
            } => write!(
                f,
                "[{TABLE}.{pack}] sets `{key}` to a{} {found}, and the pack `{pack}` declares it \
                 {kind}",
                if found.starts_with(['a', 'e', 'i', 'o', 'u']) {
                    "n"
                } else {
                    ""
                }
            ),
            Refusal::Manifest { pack, defect } => write!(
                f,
                "the pack `{pack}` does not parse, so the settings it declares cannot be read: \
                 {defect}"
            ),
        }
    }
}

fn listed(names: &[String]) -> String {
    if names.is_empty() {
        String::from("none")
    } else {
        names.join(", ")
    }
}

/// Every installed pack's settings as a workflow it carries reads them, keyed
/// by pack name and then by the setting's dotted name: the declared defaults,
/// with the values `[packs.<name>]` sets laid over them.
///
/// A key declared with no default and set by nobody is ABSENT from the pack's
/// table, which is the one way TOML can say "no value". The binary's own
/// defaults layer is not a pack and declares nothing.
pub fn resolve(
    policy: &toml::Table,
    layers: &[Layer],
) -> Result<BTreeMap<String, toml::Table>, Vec<Refusal>> {
    let mut refusals = Vec::new();
    let mut declared: BTreeMap<String, Vec<Setting>> = BTreeMap::new();
    for layer in layers.iter().filter(|layer| !layer.defaults) {
        let manifest = std::fs::read_to_string(layer.root.join(pack::MANIFEST))
            .map_err(|e| e.to_string())
            .and_then(|text| {
                pack::parse_manifest(&text).map_err(|defects| {
                    defects
                        .iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                })
            });
        match manifest {
            Ok(manifest) => {
                declared.insert(layer.name.clone(), manifest.config);
            }
            Err(defect) => refusals.push(Refusal::Manifest {
                pack: layer.name.clone(),
                defect,
            }),
        }
    }

    let mut resolved: BTreeMap<String, toml::Table> = declared
        .iter()
        .map(|(name, settings)| {
            let defaults = settings
                .iter()
                .filter_map(|s| Some((s.key.clone(), s.default.clone()?)))
                .collect();
            (name.clone(), defaults)
        })
        .collect();

    let sections = match policy.get(TABLE) {
        None => None,
        Some(toml::Value::Table(sections)) => Some(sections),
        Some(_) => {
            refusals.push(Refusal::NotATable {
                at: TABLE.to_string(),
            });
            None
        }
    };
    for (name, section) in sections.into_iter().flatten() {
        let Some(settings) = declared.get(name) else {
            // A pack whose manifest would not parse is already refused above
            // under its own name; calling its section uninstalled too would
            // send the person looking for a pack that is there.
            if !refusals
                .iter()
                .any(|r| matches!(r, Refusal::Manifest { pack, .. } if pack == name))
            {
                refusals.push(Refusal::NotInstalled {
                    pack: name.clone(),
                    installed: layers
                        .iter()
                        .filter(|layer| !layer.defaults)
                        .map(|layer| layer.name.clone())
                        .collect(),
                });
            }
            continue;
        };
        let Some(section) = section.as_table() else {
            refusals.push(Refusal::NotATable {
                at: format!("{TABLE}.{name}"),
            });
            continue;
        };
        let into = resolved.entry(name.clone()).or_default();
        set(name, section, "", settings, into, &mut refusals);
    }

    if refusals.is_empty() {
        Ok(resolved)
    } else {
        Err(refusals)
    }
}

/// One section's values, walked against the declarations.
///
/// TOML reads `takeoff.test = "…"` as a table `takeoff` holding `test`, so a
/// table is descended into while its name is the start of a declared one, and
/// a value is taken where the dotted path it sits at is declared whole. Either
/// spelling — dotted or quoted — lands on the same setting.
fn set(
    pack: &str,
    section: &toml::Table,
    prefix: &str,
    settings: &[Setting],
    into: &mut toml::Table,
    refusals: &mut Vec<Refusal>,
) {
    for (name, value) in section {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        if let Some(setting) = settings.iter().find(|s| s.key == key) {
            if setting.admits(value) {
                into.insert(key, value.clone());
            } else {
                refusals.push(Refusal::WrongType {
                    pack: pack.to_string(),
                    key,
                    kind: setting.kind.clone().unwrap_or_default(),
                    found: value.type_str().to_string(),
                });
            }
            continue;
        }
        let beneath = format!("{key}.");
        match value.as_table() {
            Some(table) if settings.iter().any(|s| s.key.starts_with(&beneath)) => {
                set(pack, table, &key, settings, into, refusals);
            }
            _ => refusals.push(Refusal::Undeclared {
                pack: pack.to_string(),
                key,
                declared: settings.iter().map(|s| s.key.clone()).collect(),
            }),
        }
    }
}
