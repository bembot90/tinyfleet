//! The pack format: the nine top-level names, the manifest, and the slot rules.
//!
//! The format is the reference's verbatim (`brain/lessons/gas-city.md` G24), so a
//! rule here the reference does not carry is a defect of this file rather than
//! of the pack it refuses.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::registry;

/// The manifest's file name, and the eight slot directories beside it. A tenth
/// name at a pack's top level is a defect: the schema is the reference's, and a
/// name they did not define is a typo.
///
/// `workflows` is the one slot this format carries that the reference does not,
/// and it is the slot `fleet run` resolves a name through — which is why it is
/// here rather than beside the manifest's own keys.
pub const MANIFEST: &str = "pack.toml";
pub const SLOTS: [&str; 7] = [
    "agents",
    "skills",
    "orders",
    "doctor",
    "overlay",
    "assets",
    "workflows",
];

/// A manifest declaring any other number is refused rather than read: the keys
/// beneath it are not this parser's. There is no dual read — a pack that has not
/// been moved to the current number is refused by name, not half-understood.
pub const SCHEMA: i64 = 3;

/// The two forms an agent directory may take, one per reference pack. A
/// directory holding neither is a defect.
pub const AGENT_FORMS: [&str; 2] = ["agent.toml", "prompt.template.md"];

/// The five names a runtime template may hold, single brace. Core fills these
/// and execs the line, so a sixth name is a template core cannot fill: it is
/// refused when the manifest is read rather than when the exec fails.
pub const PLACEHOLDERS: [&str; 5] = ["entry", "bundle", "run_dir", "fleet", "inputs"];

const PACK_KEYS: [&str; 4] = ["name", "version", "schema", "description"];
const TOP_TABLES: [&str; 4] = ["pack", "imports", "named_session", "runtime"];
const RUNTIME_KEYS: [&str; 4] = ["name", "version", "bundle", "run"];

/// One import declared by the manifest, keyed by import name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub name: String,
    pub source: String,
    pub version: String,
}

/// An always-on agent. `scope` is optional: the reference's own fleet manifest
/// omits it and its imported pack's five entries carry it, so a parse that
/// required it would refuse a real file. Parsed and kept, acted on by no verb
/// in this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSession {
    pub template: String,
    pub mode: String,
    pub scope: Option<String>,
}

/// The one thing core knows about a workflow's language: which binary, pinned to
/// which version, and the two command lines that bundle a workflow and run the
/// bundle. All four keys are required of a table that is present at all —
/// `version` is what a pack's doctor check measures against, and a bundle line
/// without a run line is half a contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub name: String,
    pub version: String,
    pub bundle: String,
    pub run: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub schema: i64,
    pub description: Option<String>,
    pub imports: Vec<Import>,
    pub named_sessions: Vec<NamedSession>,
    /// Absent on a pack that carries no workflows. This binary ships no pack;
    /// the doctrine pack in this workspace is on that side of it.
    pub runtime: Option<Runtime>,
}

/// One thing wrong with a pack. Every variant prints as one line, because the
/// check names the first defect and every further one on its own line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Defect {
    Unreadable(String),
    NoManifest,
    ManifestUnparsable(String),
    UnknownManifestTable(String),
    UnknownManifestKey(String),
    MissingManifestKey(String),
    ManifestKeyType { key: String, want: &'static str },
    UnknownRuntimeKey(String),
    RuntimeTemplate { key: String, why: String },
    Schema(i64),
    SlotNotADirectory(String),
    UnknownTopLevel(String),
    AgentWithoutDefinition(String),
    SkillWithoutSkillMd(String),
    DoctorWithoutDoctorToml(String),
    NotToml { path: String, error: String },
    RegistryUnparsable(String),
    RegistrySchema(i64),
    RegistryKey(String),
    RegistryPathMissing(String),
}

impl fmt::Display for Defect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Defect::Unreadable(e) => write!(f, "the pack directory cannot be read: {e}"),
            Defect::NoManifest => write!(f, "no {MANIFEST} at the pack's top level"),
            Defect::ManifestUnparsable(e) => write!(f, "{MANIFEST} does not parse as TOML: {e}"),
            Defect::UnknownManifestTable(t) => {
                write!(f, "{MANIFEST} holds an unknown top-level table `{t}`")
            }
            Defect::UnknownManifestKey(k) => write!(f, "[pack] holds an unknown key `{k}`"),
            Defect::MissingManifestKey(k) => write!(f, "{MANIFEST} is missing `{k}`"),
            Defect::ManifestKeyType { key, want } => {
                write!(f, "{MANIFEST}'s `{key}` is not {want}")
            }
            Defect::UnknownRuntimeKey(k) => write!(
                f,
                "[runtime] holds an unknown key `{k}` — the table is {}",
                RUNTIME_KEYS.join(", ")
            ),
            Defect::RuntimeTemplate { key, why } => {
                write!(
                    f,
                    "[runtime]'s `{key}` is not a template core can fill: {why}"
                )
            }
            Defect::Schema(n) => write!(f, "[pack] schema is {n}, not {SCHEMA}"),
            Defect::SlotNotADirectory(s) => write!(f, "the slot `{s}` is not a directory"),
            Defect::UnknownTopLevel(n) => write!(
                f,
                "unknown top-level name `{n}` — a pack holds {MANIFEST} and the eight slots"
            ),
            Defect::AgentWithoutDefinition(p) => write!(
                f,
                "`{p}` holds neither {} nor {}",
                AGENT_FORMS[0], AGENT_FORMS[1]
            ),
            Defect::SkillWithoutSkillMd(p) => write!(f, "`{p}` holds no SKILL.md"),
            Defect::DoctorWithoutDoctorToml(p) => write!(f, "`{p}` holds no doctor.toml"),
            Defect::NotToml { path, error } => {
                write!(f, "`{path}` does not parse as TOML: {error}")
            }
            Defect::RegistryUnparsable(e) => {
                write!(f, "`{}` does not parse as TOML: {e}", registry::REGISTRY)
            }
            Defect::RegistrySchema(n) => write!(
                f,
                "`{}` schema is {n}, not {}",
                registry::REGISTRY,
                registry::SCHEMA
            ),
            Defect::RegistryKey(k) => {
                write!(f, "`{}` has a [[shadow]] missing `{k}`", registry::REGISTRY)
            }
            Defect::RegistryPathMissing(p) => write!(
                f,
                "`{}` lists `{p}`, which the pack does not hold",
                registry::REGISTRY
            ),
        }
    }
}

/// One slot the pack carries, and how many entries sit directly under it. The
/// check prints one of these per slot found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub name: &'static str,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub manifest: Option<Manifest>,
    pub slots: Vec<Slot>,
    pub defects: Vec<Defect>,
}

impl Report {
    pub fn is_valid(&self) -> bool {
        self.defects.is_empty()
    }
}

/// A pure function over a path: it reads the pack and answers, and writes
/// nothing. The exit codes and the printing are the cli's.
pub fn check(root: &Path) -> Report {
    let mut defects = Vec::new();
    let mut slots = Vec::new();

    let names = match dir_names(root) {
        Ok(names) => names,
        Err(e) => {
            return Report {
                manifest: None,
                slots,
                defects: vec![Defect::Unreadable(e)],
            }
        }
    };

    let mut has_manifest = false;
    for name in &names {
        if name == MANIFEST {
            has_manifest = true;
        } else if let Some(slot) = SLOTS.iter().find(|s| *s == name) {
            if root.join(name).is_dir() {
                slots.push(Slot {
                    name: slot,
                    entries: dir_names(&root.join(name)).map(|e| e.len()).unwrap_or(0),
                });
            } else {
                defects.push(Defect::SlotNotADirectory(name.clone()));
            }
        } else {
            defects.push(Defect::UnknownTopLevel(name.clone()));
        }
    }

    let manifest = if has_manifest {
        match fs::read_to_string(root.join(MANIFEST)) {
            Ok(text) => match parse_manifest(&text) {
                Ok(m) => Some(m),
                Err(mut found) => {
                    defects.append(&mut found);
                    None
                }
            },
            Err(e) => {
                defects.push(Defect::Unreadable(e.to_string()));
                None
            }
        }
    } else {
        defects.push(Defect::NoManifest);
        None
    };

    defects.append(&mut check_slots(root));
    defects.append(&mut check_registry(root));

    Report {
        manifest,
        slots,
        defects,
    }
}

/// The manifest's own rules, over its text. Split from [`check`] so a manifest
/// can be judged without a directory to hold it.
pub fn parse_manifest(text: &str) -> Result<Manifest, Vec<Defect>> {
    let doc: toml::Table = match text.parse() {
        Ok(doc) => doc,
        Err(e) => return Err(vec![Defect::ManifestUnparsable(e.to_string())]),
    };

    let mut defects = Vec::new();
    for key in doc.keys() {
        if !TOP_TABLES.contains(&key.as_str()) {
            defects.push(Defect::UnknownManifestTable(key.clone()));
        }
    }

    let pack = match doc.get("pack") {
        Some(toml::Value::Table(t)) => t.clone(),
        Some(_) => {
            defects.push(Defect::ManifestKeyType {
                key: "pack".into(),
                want: "a table",
            });
            return Err(defects);
        }
        None => {
            defects.push(Defect::MissingManifestKey("[pack]".into()));
            return Err(defects);
        }
    };

    for key in pack.keys() {
        if !PACK_KEYS.contains(&key.as_str()) {
            defects.push(Defect::UnknownManifestKey(key.clone()));
        }
    }

    let name = string_key(&pack, "pack.name", "name", &mut defects, true).unwrap_or_default();
    // Every measured manifest carries `version`, and one of them carries it
    // empty; an absent one defaults rather than refuses, because refusing it is
    // a rule the reference does not carry.
    let version = string_key(&pack, "pack.version", "version", &mut defects, false)
        .unwrap_or_else(|| String::from(""));
    let description = string_key(
        &pack,
        "pack.description",
        "description",
        &mut defects,
        false,
    );

    let schema = match pack.get("schema") {
        Some(toml::Value::Integer(n)) => {
            if *n != SCHEMA {
                defects.push(Defect::Schema(*n));
            }
            *n
        }
        Some(_) => {
            defects.push(Defect::ManifestKeyType {
                key: "pack.schema".into(),
                want: "an integer",
            });
            0
        }
        None => {
            defects.push(Defect::MissingManifestKey("pack.schema".into()));
            0
        }
    };

    let imports = parse_imports(&doc, &mut defects);
    let named_sessions = parse_named_sessions(&doc, &mut defects);
    let runtime = parse_runtime(&doc, &mut defects);

    if defects.is_empty() {
        Ok(Manifest {
            name,
            version,
            schema,
            description,
            imports,
            named_sessions,
            runtime,
        })
    } else {
        Err(defects)
    }
}

/// Every `{name}` a template holds, in order, or the first reason the text is
/// not one core can fill. Single brace: a note template is single-brace too, and
/// a workflow's own text is a different surface that never reaches here.
pub fn placeholders(template: &str) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('}') else {
            return Err(String::from(
                "a `{` with no `}` after it, so the name it opens cannot be read",
            ));
        };
        let name = &rest[..close];
        if !PLACEHOLDERS.contains(&name) {
            return Err(format!(
                "unknown placeholder `{{{name}}}` — core fills {}",
                PLACEHOLDERS
                    .iter()
                    .map(|p| format!("{{{p}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        found.push(name.to_string());
        rest = &rest[close + 1..];
    }
    Ok(found)
}

/// Fills a runtime template. The text was judged when the manifest was read, so
/// no unknown name reaches here; a known one the caller supplies no value for is
/// LEFT STANDING rather than emptied, because an exec line with an argument
/// silently blanked is a different command from the one the pack declared.
pub fn substitute(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::from(template);
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

fn parse_runtime(doc: &toml::Table, defects: &mut Vec<Defect>) -> Option<Runtime> {
    let table = match doc.get("runtime") {
        Some(toml::Value::Table(t)) => t,
        Some(_) => {
            defects.push(Defect::ManifestKeyType {
                key: "runtime".into(),
                want: "a table",
            });
            return None;
        }
        None => return None,
    };

    for key in table.keys() {
        if !RUNTIME_KEYS.contains(&key.as_str()) {
            defects.push(Defect::UnknownRuntimeKey(key.clone()));
        }
    }

    let mut read = |key: &str| string_key(table, &format!("runtime.{key}"), key, defects, true);
    let name = read("name");
    let version = read("version");
    let bundle = read("bundle");
    let run = read("run");

    for (key, text) in [("bundle", &bundle), ("run", &run)] {
        let Some(text) = text else { continue };
        if let Err(why) = placeholders(text) {
            defects.push(Defect::RuntimeTemplate {
                key: format!("runtime.{key}"),
                why,
            });
        }
    }

    Some(Runtime {
        name: name?,
        version: version?,
        bundle: bundle?,
        run: run?,
    })
}

fn parse_imports(doc: &toml::Table, defects: &mut Vec<Defect>) -> Vec<Import> {
    let table = match doc.get("imports") {
        Some(toml::Value::Table(t)) => t,
        Some(_) => {
            defects.push(Defect::ManifestKeyType {
                key: "imports".into(),
                want: "a table",
            });
            return Vec::new();
        }
        None => return Vec::new(),
    };
    let mut imports = Vec::new();
    for (name, value) in table {
        let Some(entry) = value.as_table() else {
            defects.push(Defect::ManifestKeyType {
                key: format!("imports.{name}"),
                want: "a table",
            });
            continue;
        };
        let source = string_key(
            entry,
            &format!("imports.{name}.source"),
            "source",
            defects,
            true,
        );
        let version = string_key(
            entry,
            &format!("imports.{name}.version"),
            "version",
            defects,
            true,
        );
        if let (Some(source), Some(version)) = (source, version) {
            imports.push(Import {
                name: name.clone(),
                source,
                version,
            });
        }
    }
    imports
}

fn parse_named_sessions(doc: &toml::Table, defects: &mut Vec<Defect>) -> Vec<NamedSession> {
    let array = match doc.get("named_session") {
        Some(toml::Value::Array(a)) => a,
        Some(_) => {
            defects.push(Defect::ManifestKeyType {
                key: "named_session".into(),
                want: "an array of tables",
            });
            return Vec::new();
        }
        None => return Vec::new(),
    };
    let mut sessions = Vec::new();
    for (i, value) in array.iter().enumerate() {
        let Some(entry) = value.as_table() else {
            defects.push(Defect::ManifestKeyType {
                key: format!("named_session[{i}]"),
                want: "a table",
            });
            continue;
        };
        let template = string_key(
            entry,
            &format!("named_session[{i}].template"),
            "template",
            defects,
            true,
        );
        let mode = string_key(
            entry,
            &format!("named_session[{i}].mode"),
            "mode",
            defects,
            true,
        );
        let scope = string_key(
            entry,
            &format!("named_session[{i}].scope"),
            "scope",
            defects,
            false,
        );
        if let (Some(template), Some(mode)) = (template, mode) {
            sessions.push(NamedSession {
                template,
                mode,
                scope,
            });
        }
    }
    sessions
}

fn string_key(
    table: &toml::Table,
    label: &str,
    key: &str,
    defects: &mut Vec<Defect>,
    required: bool,
) -> Option<String> {
    match table.get(key) {
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => {
            defects.push(Defect::ManifestKeyType {
                key: label.to_string(),
                want: "a string",
            });
            None
        }
        None => {
            if required {
                defects.push(Defect::MissingManifestKey(label.to_string()));
            }
            None
        }
    }
}

/// The four slots whose entries carry a shape. `overlay` and `assets` hold
/// whatever the pack drops there, so neither has a rule to break.
fn check_slots(root: &Path) -> Vec<Defect> {
    let mut defects = Vec::new();

    for name in dir_names(&root.join("agents")).unwrap_or_default() {
        let dir = root.join("agents").join(&name);
        let held = AGENT_FORMS.iter().any(|f| dir.join(f).is_file());
        if !held {
            defects.push(Defect::AgentWithoutDefinition(format!("agents/{name}")));
        }
    }

    for name in dir_names(&root.join("skills")).unwrap_or_default() {
        if !root.join("skills").join(&name).join("SKILL.md").is_file() {
            defects.push(Defect::SkillWithoutSkillMd(format!("skills/{name}")));
        }
    }

    for name in dir_names(&root.join("doctor")).unwrap_or_default() {
        if !root
            .join("doctor")
            .join(&name)
            .join("doctor.toml")
            .is_file()
        {
            defects.push(Defect::DoctorWithoutDoctorToml(format!("doctor/{name}")));
        }
    }

    // Every file directly under it is the slot's own format. A
    // subdirectory is not walked: neither reference nests one.
    for slot in ["orders"] {
        for name in dir_names(&root.join(slot)).unwrap_or_default() {
            let path = root.join(slot).join(&name);
            if !path.is_file() {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(text) => {
                    if let Err(e) = text.parse::<toml::Table>() {
                        defects.push(Defect::NotToml {
                            path: format!("{slot}/{name}"),
                            error: e.to_string(),
                        });
                    }
                }
                Err(e) => defects.push(Defect::Unreadable(e.to_string())),
            }
        }
    }

    defects
}

fn check_registry(root: &Path) -> Vec<Defect> {
    match registry::read(root) {
        None => Vec::new(),
        Some(Err(e)) => vec![match e {
            registry::RegistryError::Unparsable(e) => Defect::RegistryUnparsable(e),
            registry::RegistryError::Schema(n) => Defect::RegistrySchema(n),
            registry::RegistryError::Key(k) => Defect::RegistryKey(k),
        }],
        Some(Ok(reg)) => registry::missing(root, &reg)
            .into_iter()
            .map(Defect::RegistryPathMissing)
            .collect(),
    }
}

/// A pack's walks read past OS litter rather than refusing the pack or
/// resolving the litter as slot files.
pub use crate::os_litter::{is_os_litter, OS_LITTER};

/// The names directly under a directory, sorted, so a report reads the same on
/// every filesystem, with [`OS_LITTER`] left out. Every walk of a pack — the
/// check here and the resolver's — lists a directory through this one reader,
/// which is what keeps them agreeing on what a pack holds.
pub(crate) fn dir_names(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_os_litter(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}
