//! The shadow registry: the whole list of files a pack on top may replace.
//!
//! Anything a pack carries that is not listed here is that pack's own
//! implementation, and a pack that needs one of them changed files an issue
//! against it rather than a copy.

use std::path::Path;

/// The registry's path inside the pack that publishes it, and the only schema
/// this reader knows.
pub const REGISTRY: &str = "assets/shadow-registry.toml";
pub const SCHEMA: i64 = 1;

/// One file a pack on top may replace, and one line saying what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shadow {
    pub path: String,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Registry {
    pub shadows: Vec<Shadow>,
}

impl Registry {
    pub fn lists(&self, path: &str) -> bool {
        self.shadows.iter().any(|s| s.path == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    Unparsable(String),
    Schema(i64),
    Key(String),
}

/// `None` when the pack publishes no registry, which is every pack that is not
/// the bottom layer. The distinction is the resolver's: a bottom layer with no
/// registry shadows freely, and one with a registry shadows only what it lists.
pub fn read(root: &Path) -> Option<Result<Registry, RegistryError>> {
    let path = root.join(REGISTRY);
    if !path.is_file() {
        return None;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => return Some(Err(RegistryError::Unparsable(e.to_string()))),
    };
    Some(parse(&text))
}

pub fn parse(text: &str) -> Result<Registry, RegistryError> {
    let doc: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| RegistryError::Unparsable(e.to_string()))?;

    match doc.get("schema") {
        Some(toml::Value::Integer(n)) if *n == SCHEMA => {}
        Some(toml::Value::Integer(n)) => return Err(RegistryError::Schema(*n)),
        Some(_) => return Err(RegistryError::Key("schema".into())),
        None => return Err(RegistryError::Key("schema".into())),
    }

    let mut shadows = Vec::new();
    match doc.get("shadow") {
        None => {}
        Some(toml::Value::Array(rows)) => {
            for row in rows {
                let Some(row) = row.as_table() else {
                    return Err(RegistryError::Key("path".into()));
                };
                let path = row.get("path").and_then(toml::Value::as_str);
                let purpose = row.get("purpose").and_then(toml::Value::as_str);
                match (path, purpose) {
                    (Some(path), Some(purpose)) => shadows.push(Shadow {
                        path: path.to_string(),
                        purpose: purpose.to_string(),
                    }),
                    (None, _) => return Err(RegistryError::Key("path".into())),
                    (_, None) => return Err(RegistryError::Key("purpose".into())),
                }
            }
        }
        Some(_) => return Err(RegistryError::Key("shadow".into())),
    }

    Ok(Registry { shadows })
}

/// The listed paths the pack does not hold. A row naming a file that is not
/// there is a defect of the pack publishing the registry, not of the pack on
/// top of it.
pub fn missing(root: &Path, registry: &Registry) -> Vec<String> {
    registry
        .shadows
        .iter()
        .filter(|s| !root.join(&s.path).exists())
        .map(|s| s.path.clone())
        .collect()
}
