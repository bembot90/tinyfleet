//! `packs.lock`: the one record `fleet pack add` writes.
//!
//! The format is Gas City's verbatim — a top-level `schema`, then one
//! `[packs."<source as given>"]` table per pack carrying `version`, `commit` and
//! `fetched`. The source is the key because it is what a person typed and what
//! a re-add matches on; the commit is what the version resolved to on the day.
//!
//! The renderer is `toml`'s own: the workspace pin carries the `display` half,
//! so the document's shape is a type and the escaping of a source that a person
//! typed is the serializer's rather than this file's.

use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::Path;

pub const LOCK: &str = "packs.lock";
pub const SCHEMA: i64 = 1;

/// One installed pack, as the lock records it.
///
/// `source` is the table's key and never one of its fields, which is why it is
/// skipped: the serialized form of an entry is the body of `[packs."<source>"]`
/// and the keys are written in the order they are declared here.
///
/// `name` is the manifest's, which is what the pack's directory is called: it is
/// the only join from a source a person typed to the directory that source
/// installed. It is optional because a lock written before it existed carries no
/// such key and still reads, and left out of the rendered table when absent, so
/// the schema stays 1 — a reader that ignores the key reads the file as before.
///
/// `tree` is the hash of the files the install put on disk, for the one line
/// whose version cannot say which bytes those are: the binary's defaults travel
/// inside the binary and are pinned at its own version, so two different sets
/// pin the same string. A fetched pack needs no such key —
/// its commit already names its bytes — and leaves it empty. Optional and
/// omitted on the same rule as `name`, and not a column of `fleet pack list`:
/// the installer reads it, a person does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    #[serde(skip_serializing)]
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub version: String,
    pub commit: String,
    pub fetched: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    Unreadable(String),
    Unparsable(String),
    Schema(i64),
    NoSchema,
    NotATable(String),
    Key { source: String, key: &'static str },
    Unwritable(String),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Unreadable(e) => write!(f, "{LOCK} cannot be read: {e}"),
            LockError::Unparsable(e) => write!(f, "{LOCK} does not parse as TOML: {e}"),
            LockError::Schema(n) => write!(f, "{LOCK} schema is {n}, not {SCHEMA}"),
            LockError::NoSchema => write!(f, "{LOCK} declares no `schema`"),
            LockError::NotATable(k) => write!(f, "{LOCK}'s `{k}` is not a table"),
            LockError::Key { source, key } => {
                write!(f, "{LOCK}'s [packs.\"{source}\"] is missing `{key}`")
            }
            LockError::Unwritable(e) => write!(f, "{LOCK} cannot be written: {e}"),
        }
    }
}

/// Every entry the lock holds, sorted by source. A lock that is not there yet is
/// an empty one rather than an error: the first `add` on a machine creates it.
pub fn read(path: &Path) -> Result<Vec<Entry>, LockError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|e| LockError::Unreadable(e.to_string()))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Vec<Entry>, LockError> {
    // A blank document is an empty lock, the same answer an absent file gets: a
    // path that accepted the bytes and kept none of them is what the read-back
    // exists to catch, and it reads here as "the pin is not in it".
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let doc: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| LockError::Unparsable(e.to_string()))?;

    match doc.get("schema") {
        Some(toml::Value::Integer(n)) if *n == SCHEMA => {}
        Some(toml::Value::Integer(n)) => return Err(LockError::Schema(*n)),
        Some(_) => return Err(LockError::NotATable("schema".into())),
        None => return Err(LockError::NoSchema),
    }

    let packs = match doc.get("packs") {
        None => return Ok(Vec::new()),
        Some(toml::Value::Table(t)) => t,
        Some(_) => return Err(LockError::NotATable("packs".into())),
    };

    let mut entries = Vec::new();
    for (source, value) in packs {
        let Some(table) = value.as_table() else {
            return Err(LockError::NotATable(format!("packs.{source}")));
        };
        let field = |key: &'static str| -> Result<String, LockError> {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| LockError::Key {
                    source: source.clone(),
                    key,
                })
        };
        entries.push(Entry {
            source: source.clone(),
            name: table
                .get("name")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            version: field("version")?,
            commit: field("commit")?,
            fetched: field("fetched")?,
            tree: table
                .get("tree")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        });
    }
    Ok(entries)
}

/// The document as a type: the schema first, then the packs table, in the order
/// a reader meets them. An empty map is left out entirely, so a lock holding no
/// pack is the schema line alone and not a bare `[packs]` header.
#[derive(Serialize)]
struct Document<'a> {
    schema: i64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    packs: BTreeMap<&'a str, &'a Entry>,
}

/// The whole document, rendered from the entries it holds. Sources are the keys
/// of a sorted map, so re-writing an unchanged lock produces the same bytes.
pub fn render(entries: &[Entry]) -> String {
    let document = Document {
        schema: SCHEMA,
        packs: entries.iter().map(|e| (e.source.as_str(), e)).collect(),
    };
    // Four owned strings and an integer: there is no value in this shape the
    // serializer can refuse, and a panic here would be this file's own defect
    // rather than a caller's input.
    toml::to_string(&document).expect("the lock's document serializes")
}

/// The whole document or none of it: a synced temp file beside the lock, renamed
/// over the path, so a symlink or read-only file standing there is replaced.
pub fn write(path: &Path, entries: &[Entry]) -> Result<(), LockError> {
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir).map_err(|e| LockError::Unwritable(e.to_string()))?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or(LOCK);
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let landed = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(render(entries).as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    landed.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        LockError::Unwritable(e.to_string())
    })
}

/// Add or replace one entry, keyed by source, and write the whole document. A
/// re-add of the same source moves its pin rather than growing a second table
/// the reader would have to choose between.
pub fn append(path: &Path, entry: &Entry) -> Result<(), LockError> {
    append_all(path, std::slice::from_ref(entry))
}

/// The same for several entries in one write, so a pack and the imports that
/// came in with it are pinned together or not at all.
pub fn append_all(path: &Path, added: &[Entry]) -> Result<(), LockError> {
    let mut entries = read(path)?;
    entries.retain(|e| added.iter().all(|a| a.source != e.source));
    entries.extend(added.iter().cloned());
    write(path, &entries)
}

/// Drop one entry by source and write the whole document. A source the lock does
/// not carry writes the document back unchanged: the caller that asks for a
/// source is the one that decides whether its absence is a refusal.
pub fn remove(path: &Path, source: &str) -> Result<(), LockError> {
    let mut entries = read(path)?;
    entries.retain(|e| e.source != source);
    write(path, &entries)
}

/// Whether the lock on disk carries this entry, field for field. The read-back
/// every verb owes its own record before it exits 0: a write that returned `Ok`
/// and landed nothing is the failure this answers.
pub fn holds(path: &Path, entry: &Entry) -> Result<bool, LockError> {
    Ok(read(path)?.iter().any(|e| e == entry))
}
