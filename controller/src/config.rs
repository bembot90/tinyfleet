//! `config.json` — the machine's list of managed seats.
//!
//! Local, written by `fleet` commands, and re-read whenever its mtime moves.
//! Every writer renames a temp file over the path, so a reader sees a whole old
//! file or a whole new one and no lock is needed.
//!
//! A WRITER still takes one, because a rename is atomic and a read-modify-write
//! is not: two spawns that read the same file and then each rename their own
//! version over it leave one row where there should be two.

use fleet_core::seat::identity::{
    read_identity, resolve, roster, Directory, Kind, SeatId, SeatRef, Unresolved,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Seat {
    /// The seat, and the key every row is found by.
    pub id: SeatId,
    /// The seat's own name as `fleet.toml` says it, `None` where it has none.
    pub name: Option<String>,
    pub model: Option<String>,
    /// Set on a spawned seat. THE SEAT is observed and published like any
    /// other, and is never revived or succeeded; THIS FIELD is neither read nor
    /// published by the controller, which only parses it onto the row. The
    /// porter's own tools are what read it, off this same `config.json`.
    pub transient: bool,
    /// Project to worktree, ALPHABETICAL BY PROJECT and not in the file's order.
    /// The order decides which project a row is published under when two of a
    /// seat's projects name one directory, so it is sorted on purpose rather
    /// than left to however the file was written.
    pub worktrees: Vec<(String, String)>,
}

impl Seat {
    /// The seat as every reader names it. A row of this list is an agent's:
    /// the controller runs no person's seat.
    pub fn as_ref(&self) -> SeatRef {
        SeatRef {
            id: self.id,
            name: self.name.clone(),
            kind: Kind::Agent,
        }
    }

    /// `<slug>-<short>`: the seat's worktree and configuration directory, the
    /// name its first session is started under, and how a sentence names it.
    /// Nothing is keyed on it: the session table, the stream's actor and the
    /// projection's row are the id.
    pub fn machine_name(&self) -> String {
        self.as_ref().machine_name()
    }
}

#[derive(Clone, Debug)]
pub struct MachineConfig {
    pub fleet_toml: PathBuf,
    pub seats: Vec<Seat>,
    /// Rows that named no seat or no worktree, with why. A skipped row is
    /// reported and never defaulted: a session opened in the wrong directory
    /// claims work as the wrong seat.
    pub skipped: Vec<String>,
    /// The machine's own answers for `[controller]` keys, as written; each one
    /// overrides `fleet.toml`'s per key. Read here and applied in
    /// [`crate::policy`], so this file stays the machine's document and the
    /// grammar of a key stays policy's.
    pub controller: Option<serde_json::Value>,
}

impl MachineConfig {
    /// The one row a seat argument names — its full id, eight or more of its
    /// hex digits, its name or its machine name — through core's one resolver.
    pub fn resolve(&self, arg: &str) -> Result<&Seat, Unresolved> {
        let seats: Vec<SeatRef> = self.seats.iter().map(Seat::as_ref).collect();
        resolve(&seats, arg).map(|index| &self.seats[index])
    }

    /// The row whose id this is.
    pub fn by_id(&self, id: &SeatId) -> Option<&Seat> {
        self.seats.iter().find(|seat| seat.id == *id)
    }
}

/// The seat directory a verb on this machine resolves through, over these
/// rows and the fleet's policy.
///
/// LISTED is the policy's `[seats.<id>]` roster, the transient rows — a
/// spawned seat is on no roster — and this machine's identity where
/// `identity.toml` exists; RUNNING is every row. Each seat is listed once, in
/// that order.
///
/// READING NEVER MINTS AND NEVER REFUSES: a roster or an identity file that
/// will not read lists nobody from it, and the verb that asks about a seat it
/// needed says so — `fleet start` and `fleet seat add` are the readers that
/// refuse a bad roster.
pub fn directory(rows: &[Seat], policy: &toml::Table, machine_dir: &Path) -> Directory {
    let mut listed: Vec<SeatRef> = Vec::new();
    let roster = roster(policy).unwrap_or_default();
    let transient = rows.iter().filter(|row| row.transient).map(Seat::as_ref);
    let identity = read_identity(machine_dir)
        .ok()
        .flatten()
        .map(|identity| identity.as_ref());
    for seat in roster
        .into_iter()
        .map(|seat| seat.seat)
        .chain(transient)
        .chain(identity)
    {
        if !listed.iter().any(|known| known.id == seat.id) {
            listed.push(seat);
        }
    }
    Directory {
        listed,
        running: rows.iter().map(Seat::as_ref).collect(),
    }
}

#[derive(Deserialize)]
struct RawConfig {
    fleet_toml: Option<String>,
    #[serde(default)]
    children: Vec<RawSeat>,
    #[serde(default)]
    controller: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawSeat {
    id: Option<String>,
    name: Option<String>,
    model: Option<String>,
    #[serde(default)]
    transient: bool,
    /// A sorted map: the alphabetical order of `Seat::worktrees` is this.
    #[serde(default)]
    worktrees: BTreeMap<String, String>,
}

pub fn read(path: &Path) -> Result<MachineConfig, String> {
    let body = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&body).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(body: &str) -> Result<MachineConfig, String> {
    let raw: RawConfig = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let fleet_toml = match raw.fleet_toml.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => return Err("carries no `fleet_toml` key naming the policy file".to_string()),
    };
    let mut seats = Vec::new();
    let mut skipped = Vec::new();
    for row in raw.children {
        // THE ID IS THE KEY, and a name is no stand-in for it: a row keyed by
        // its name is the shape the clean break retired, and nothing migrates
        // it.
        let id = match row.id.as_deref().map(str::trim) {
            None | Some("") => {
                skipped.push("a row carries no id".to_string());
                continue;
            }
            Some(said) => match SeatId::parse(said) {
                Ok(id) => id,
                Err(_) => {
                    skipped.push(format!("a row's id {said} is not a seat id"));
                    continue;
                }
            },
        };
        let worktrees: Vec<(String, String)> = row
            .worktrees
            .into_iter()
            .filter(|(project, path)| !project.trim().is_empty() && !path.trim().is_empty())
            .collect();
        let seat = Seat {
            id,
            name: row
                .name
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
            model: row.model,
            transient: row.transient,
            worktrees,
        };
        if seat.worktrees.is_empty() {
            skipped.push(format!(
                "{} carries no worktrees entry",
                seat.machine_name()
            ));
            continue;
        }
        seats.push(seat);
    }
    Ok(MachineConfig {
        fleet_toml,
        seats,
        skipped,
        controller: raw.controller,
    })
}

/// The file's modification time, for the re-read gate. `None` when it cannot be
/// stated, which re-reads every poll rather than never.
pub fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

// ---- the writer -------------------------------------------------------------

/// What a spawn is claiming a seat for.
pub struct TransientSeat<'a> {
    pub project: &'a str,
    pub model: &'a str,
    pub worktrees_dir: &'a Path,
}

/// The seat and the directory a claim took. The name is the seat's machine
/// name, `agent-<short>`: a transient seat has no name of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    pub id: SeatId,
    pub name: String,
    pub worktree: PathBuf,
}

/// A claim that did not complete, and whether it left a worktree behind.
#[derive(Debug)]
pub enum ClaimError {
    /// Nothing was created; there is nothing to roll back.
    Nothing(String),
    /// The worktree was made and the row was not, so the caller owes a rollback.
    Made { worktree: PathBuf, why: String },
}

/// Mint a fresh seat id, cut its worktree, and append its row — under ONE lock,
/// so the row is never written for a tree that was not made. An id is never
/// handed out twice, so nothing a retired seat left behind can be inherited by
/// a later one.
///
/// THE DOCUMENT IS EDITED, NOT RE-SERIALIZED FROM [`parse`]. That reader takes
/// the fields the controller folds on and drops the rest —
/// `an_unknown_field_is_tolerated` is the arm that says so — so a writer built
/// on it would silently delete every key another tool keeps here.
pub fn claim_transient_seat(
    path: &Path,
    seat: &TransientSeat,
    make: impl FnOnce(&str, &Path) -> Result<(), String>,
) -> Result<Claimed, ClaimError> {
    let _held = crate::platform::lock_beside(path).map_err(ClaimError::Nothing)?;
    let mut document = read_document(path).map_err(ClaimError::Nothing)?;
    let minted = SeatRef {
        id: SeatId::mint(),
        name: None,
        kind: Kind::Agent,
    };
    let name = minted.machine_name();
    let worktree = seat.worktrees_dir.join(&name);
    if worktree.exists() {
        return Err(ClaimError::Nothing(format!(
            "{} already exists, so nothing was created — remove it, or retire the seat that \
             left it behind",
            worktree.display()
        )));
    }

    make(&name, &worktree).map_err(ClaimError::Nothing)?;

    let failed = |why: String| ClaimError::Made {
        worktree: worktree.clone(),
        why,
    };
    children_of(&mut document)
        .map_err(&failed)?
        // NO `name`: the key is the seat's own name, and a transient seat has
        // none. Its machine name is derived from the id wherever it is needed.
        .push(serde_json::json!({
            "id": minted.id.to_string(),
            "kind": minted.kind.as_str(),
            "transient": true,
            "model": seat.model,
            "worktrees": { seat.project: worktree.display().to_string() },
        }));
    write_document(path, &document).map_err(failed)?;
    Ok(Claimed {
        id: minted.id,
        name,
        worktree,
    })
}

/// Drop one row by id, under the same lock. `Ok(false)` is an id the file does
/// not carry, which is a retire of a row something else already dropped and not
/// a failure.
pub fn drop_seat(path: &Path, id: &SeatId) -> Result<bool, String> {
    let _held = crate::platform::lock_beside(path)?;
    let mut document = read_document(path)?;
    let children = children_of(&mut document)?;
    let before = children.len();
    children.retain(|row| row_id(row) != Some(*id));
    let dropped = children.len() != before;
    write_document(path, &document)?;
    Ok(dropped)
}

// ---- the `[seats]` render ---------------------------------------------------

/// One agent seat, as `fleet start` renders it into a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedSeat {
    /// The key the render upserts on.
    pub id: SeatId,
    /// The seat's own name as `fleet.toml` says it, written onto the row where
    /// there is one and taken off it where there is none.
    pub name: Option<String>,
    /// Always present: a start with no model flag comes up on the cheapest
    /// available model (lessons claude-code A5), so the row carries the policy's
    /// default where the table names none.
    pub model: String,
    /// Project to worktree.
    pub worktrees: Vec<(String, String)>,
}

impl RenderedSeat {
    /// `<slug>-<short>`, which is how a render's report names the row.
    pub fn machine_name(&self) -> String {
        SeatRef {
            id: self.id,
            name: self.name.clone(),
            kind: Kind::Agent,
        }
        .machine_name()
    }
}

/// What a render moved, each row by its machine name.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rendered {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    /// Rows whose seat left the table or turned parked.
    pub dropped: Vec<String>,
}

impl Rendered {
    pub fn moved(&self) -> bool {
        !self.added.is_empty() || !self.updated.is_empty() || !self.dropped.is_empty()
    }
}

/// Render the fleet's active agent seats into `config.json`'s rows, upserting
/// by the seat id — so a seat renamed in `fleet.toml` updates its own row, and
/// is neither dropped nor added.
///
/// UNDER THE SAME LOCK AND THE SAME DOCUMENT-EDIT DISCIPLINE as
/// [`claim_transient_seat`]: the document is edited in place and never
/// re-serialized from [`parse`], so a transient row, an unknown key and every
/// byte of the rest of the file survive a render that did not touch them.
///
/// A TRANSIENT ROW IS NEVER DROPPED. The rows this reconciles are the table's;
/// a transient seat is a spawn's and a retire's, and a render that swept it
/// would take a live session's row out from under the loop.
pub fn render_seats(path: &Path, seats: &[RenderedSeat]) -> Result<Rendered, String> {
    let _held = crate::platform::lock_beside(path)?;
    let mut document = read_document(path)?;
    let mut moved = Rendered::default();
    {
        let children = children_of(&mut document)?;
        let wanted: Vec<SeatId> = seats.iter().map(|s| s.id).collect();
        children.retain(|row| {
            // A row with NO ID is not a seat this list can name, so it is not
            // one this render reconciles: `parse` skips it and reports it, and
            // a render that swept it would delete somebody's data over a key
            // it has nothing to say about.
            let Some(id) = row_id(row) else {
                return true;
            };
            let transient = row["transient"].as_bool().unwrap_or(false);
            let keep = transient || wanted.contains(&id);
            if !keep {
                moved.dropped.push(
                    SeatRef {
                        id,
                        name: row["name"].as_str().map(str::to_string),
                        kind: Kind::Agent,
                    }
                    .machine_name(),
                );
            }
            keep
        });

        for seat in seats {
            let row = row_of(seat);
            let fields = row.as_object().expect("a row is an object").clone();
            match children
                .iter_mut()
                .find(|existing| row_id(existing) == Some(seat.id))
            {
                Some(existing) => {
                    // FIELD BY FIELD, and only the fields this render owns: a
                    // key another tool keeps on the row is not swept by a render
                    // that has nothing to say about it.
                    let mut changed = false;
                    for (key, value) in &fields {
                        if existing.get(key.as_str()) != Some(value) {
                            existing[key.as_str()] = value.clone();
                            changed = true;
                        }
                    }
                    if let Some(object) = existing.as_object_mut() {
                        // A seat with no name has no `name` on its row: one
                        // left behind would be read as the seat's own.
                        if seat.name.is_none() {
                            changed |= object.remove("name").is_some();
                        }
                        // A row this render owns keeps no `chosen_name`: the
                        // key is the shape seat identity retired, and one left
                        // behind publishes a display name the fleet's own file
                        // no longer says.
                        changed |= object.remove("chosen_name").is_some();
                    }
                    if changed {
                        moved.updated.push(seat.machine_name());
                    }
                }
                None => {
                    children.push(row);
                    moved.added.push(seat.machine_name());
                }
            }
        }
    }
    write_document(path, &document)?;
    Ok(moved)
}

fn row_of(seat: &RenderedSeat) -> serde_json::Value {
    let mut row = serde_json::json!({
        "id": seat.id.to_string(),
        "kind": Kind::Agent.as_str(),
        "model": seat.model,
        "worktrees": seat
            .worktrees
            .iter()
            .map(|(project, path)| (project.clone(), serde_json::Value::String(path.clone())))
            .collect::<serde_json::Map<String, serde_json::Value>>(),
    });
    if let Some(name) = &seat.name {
        row["name"] = serde_json::Value::String(name.clone());
    }
    row
}

/// A row's `id`, where it carries one that parses. Every writer finds a row by
/// this and by nothing else.
fn row_id(row: &serde_json::Value) -> Option<SeatId> {
    row["id"].as_str().and_then(|id| SeatId::parse(id).ok())
}

/// The seat list as a raw document.
///
/// A file that is not there reads as an empty object rather than as an error:
/// `fleet create` writes the first one, and a spawn into a machine directory
/// that has none is a fleet with no seats rather than a fault.
fn read_document(path: &Path) -> Result<serde_json::Value, String> {
    match std::fs::read_to_string(path) {
        Ok(body) => serde_json::from_str(&body).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_document(path: &Path, document: &serde_json::Value) -> Result<(), String> {
    let mut body = serde_json::to_string_pretty(document).map_err(|e| e.to_string())?;
    body.push('\n');
    crate::platform::write_atomic(path, body.as_bytes()).map_err(|e| e.to_string())
}

/// The `children` array, made where the document carries none. A `children` key
/// that is not an array is refused rather than replaced: it is somebody's data.
fn children_of(document: &mut serde_json::Value) -> Result<&mut Vec<serde_json::Value>, String> {
    let object = document
        .as_object_mut()
        .ok_or_else(|| "the seat list is not a JSON object".to_string())?;
    let children = object
        .entry("children")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    children
        .as_array_mut()
        .ok_or_else(|| "the seat list's `children` is not an array".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed ids this suite's rows carry. Every one is a v7 minted in the
    /// same minute, so they share their first eight digits and differ in the
    /// tail a short id is cut from [ASSUMES D1].
    const ORLA: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
    const PELL: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
    const SPAWNED: &str = "01a0d1f1-0aec-765f-9abe-00007e3fa2c0";
    const GONE: &str = "01a0d1f1-0aec-765f-9abe-0000000a90e5";

    fn id(text: &str) -> SeatId {
        SeatId::parse(text).expect("a hand-written seat id parses")
    }

    const ONE_SEAT: &str = r#"{
      "fleet_toml": "/fleet/fleet.toml",
      "children": [
        {"id": "01a0d1f1-0aec-765f-9abe-d4f993b9739a", "name": "Orla", "model": "a-model",
         "worktrees": {"demo": "/wt/orla-93b9739a"}},
        {"id": "01a0d1f1-0aec-765f-9abe-5c21e8a04b17", "worktrees": {}},
        {"name": "builder-1", "worktrees": {"demo": "/wt/x"}},
        {"id": "builder-2", "worktrees": {"demo": "/wt/y"}}
      ]
    }"#;

    /// A row is keyed by its id, so a row with none — an old name-keyed row
    /// among them, which nothing migrates — and a row whose id is not one are
    /// each skipped with their own line, as a row with no worktree still is.
    #[test]
    fn a_row_with_no_id_a_bad_id_or_no_worktree_is_skipped_loudly() {
        let config = parse(ONE_SEAT).expect("the file parses");
        assert_eq!(config.fleet_toml, PathBuf::from("/fleet/fleet.toml"));
        assert_eq!(config.seats.len(), 1);
        assert_eq!(config.seats[0].id, id(ORLA));
        assert_eq!(config.seats[0].name.as_deref(), Some("Orla"));
        assert_eq!(config.seats[0].machine_name(), "orla-93b9739a");
        assert_eq!(
            config.seats[0].worktrees,
            vec![("demo".to_string(), "/wt/orla-93b9739a".to_string())]
        );
        assert_eq!(
            config.skipped,
            vec![
                "agent-e8a04b17 carries no worktrees entry".to_string(),
                "a row carries no id".to_string(),
                "a row's id builder-2 is not a seat id".to_string(),
            ],
            "each bad row is reported with its own line"
        );
    }

    /// Every seat argument a verb takes comes through here: the name, the
    /// machine name, eight hex digits of the id and the whole id each find the
    /// same row, and the row found by id is that one too.
    #[test]
    fn resolve_finds_a_row_by_name_short_form_machine_name_and_full_id() {
        let config = parse(&format!(
            r#"{{"fleet_toml":"/f.toml","children":[
                 {{"id":"{ORLA}","name":"Orla","worktrees":{{"p":"/wt/o"}}}},
                 {{"id":"{PELL}","worktrees":{{"p":"/wt/p"}}}}
               ]}}"#
        ))
        .expect("the file parses");

        for arg in ["Orla", "orla", "orla-93b9739a", "93b9739a", ORLA] {
            let found = config.resolve(arg).unwrap_or_else(|e| panic!("{arg}: {e}"));
            assert_eq!(found.id, id(ORLA), "{arg}");
        }
        for arg in ["agent-e8a04b17", "e8a04b17", PELL] {
            let found = config.resolve(arg).unwrap_or_else(|e| panic!("{arg}: {e}"));
            assert_eq!(found.id, id(PELL), "{arg}");
        }
        assert_eq!(
            config.by_id(&id(PELL)).map(Seat::machine_name).as_deref(),
            Some("agent-e8a04b17")
        );

        // The controls: a name nobody holds is missing and lists the seats,
        // and the first eight digits every seat of that minute shares are
        // ambiguous and name both — never the first one found.
        let missing = config.resolve("Kite").expect_err("nobody is Kite");
        assert!(matches!(missing, Unresolved::Missing { .. }), "{missing}");
        assert!(missing.to_string().contains("orla-93b9739a"), "{missing}");
        let both = config.resolve("01a0d1f1").expect_err("the clock is shared");
        assert!(matches!(both, Unresolved::Ambiguous { .. }), "{both}");
        assert!(config.by_id(&id(GONE)).is_none());
    }

    /// The directory a verb resolves through: the roster, the transient rows
    /// and this machine's identity are listed, each once; every row runs; and
    /// a person, listed, runs nowhere. A roster that will not read lists
    /// nobody from it and refuses nothing.
    #[test]
    fn the_directory_lists_the_roster_the_transient_rows_and_the_identity() {
        const ME: &str = "01a0d1f1-0aec-765f-9abe-00000000c0de";
        let machine_dir =
            std::env::temp_dir().join(format!("fleet-directory-{}", std::process::id()));
        std::fs::create_dir_all(&machine_dir).expect("the machine dir is made");
        std::fs::write(
            machine_dir.join(fleet_core::seat::identity::IDENTITY),
            format!("id = \"{ME}\"\nkind = \"human\"\nname = \"Alberto\"\n"),
        )
        .expect("the identity is written");
        let config = parse(&format!(
            r#"{{"fleet_toml":"/f.toml","children":[
                 {{"id":"{ORLA}","name":"Orla","worktrees":{{"p":"/wt/o"}}}},
                 {{"id":"{SPAWNED}","transient":true,"worktrees":{{"p":"/wt/s"}}}}
               ]}}"#
        ))
        .expect("the file parses");
        let policy: toml::Table = format!(
            "[seats.{ORLA}]\nkind = \"agent\"\nname = \"Orla\"\n\
             [seats.{ME}]\nkind = \"human\"\nname = \"Alberto\"\n"
        )
        .parse()
        .expect("the policy parses");

        let dir = directory(&config.seats, &policy, &machine_dir);
        let listed: Vec<SeatId> = dir.listed.iter().map(|seat| seat.id).collect();
        // The roster in id order, then the transient row; the identity is on
        // the roster already.
        assert_eq!(listed, [id(ME), id(ORLA), id(SPAWNED)], "each once");
        let running: Vec<SeatId> = dir.running.iter().map(|seat| seat.id).collect();
        assert_eq!(running, [id(ORLA), id(SPAWNED)]);
        assert!(
            dir.resolve_running("alberto").is_err(),
            "a person runs nowhere"
        );
        assert_eq!(dir.resolve_listed("alberto").map(|s| s.id), Ok(id(ME)));
        assert_eq!(
            dir.resolve_listed("agent-7e3fa2c0").map(|s| s.id),
            Ok(id(SPAWNED))
        );

        let broken: toml::Table = "[seats.orla]\nkind = \"agent\"\n".parse().unwrap();
        let dir = directory(&config.seats, &broken, &machine_dir);
        let listed: Vec<SeatId> = dir.listed.iter().map(|seat| seat.id).collect();
        assert_eq!(listed, [id(SPAWNED), id(ME)], "the roster lists nobody");
        let _ = std::fs::remove_dir_all(&machine_dir);
    }

    #[test]
    fn a_file_with_no_policy_path_is_an_error_and_not_an_empty_fleet() {
        let err = parse(r#"{"children": []}"#).expect_err("no policy path is refused");
        assert!(
            err.contains("fleet_toml"),
            "the message names the key: {err}"
        );
    }

    /// What this arm measures: both fields are read off the row, and a row that
    /// says neither is a permanent seat on the fleet's default model — which is
    /// a reading rather than a silence.
    ///
    /// The controller parses them and no more. That it publishes neither is
    /// measured in `tests/observe.rs`, by
    /// `the_projection_carries_no_model_and_no_transient`; their consumer is the
    /// porter, which reads this same `config.json` in another crate this suite
    /// cannot see (`tools/porter/src/config.rs`).
    #[test]
    fn a_seats_model_and_transient_are_read_from_the_row_and_default_without_it() {
        let config = parse(&format!(
            r#"{{"fleet_toml":"/f.toml","children":[
                 {{"id":"{SPAWNED}","model":"a-model","transient":true,
                  "worktrees":{{"demo":"/wt/agent-7e3fa2c0"}}}},
                 {{"id":"{ORLA}","worktrees":{{"demo":"/wt/agent-93b9739a"}}}}
               ]}}"#
        ))
        .expect("the file parses");

        assert_eq!(config.seats[0].model.as_deref(), Some("a-model"));
        assert!(config.seats[0].transient, "a row that says so is transient");

        // The control: the row that says neither, so the two readings above are
        // the row's and not a parser that answers the same for everything.
        assert!(config.seats[1].model.is_none());
        assert!(
            !config.seats[1].transient,
            "a row that says nothing is a permanent seat"
        );
    }

    /// The machine's own answers for `[controller]` keys, read off the file and
    /// handed to policy as written.
    #[test]
    fn the_machines_own_controller_answers_are_read_off_the_file() {
        let config = parse(&format!(
            r#"{{"fleet_toml":"/f.toml","controller":{{"poll_seconds":11}},
                "children":[{{"id":"{ORLA}","worktrees":{{"p":"/w"}}}}]}}"#
        ))
        .expect("the file parses");
        assert_eq!(config.controller.as_ref().unwrap()["poll_seconds"], 11);

        // The control: a file that carries no such object says so, which is a
        // fleet running on its policy alone rather than on an empty override.
        let bare = parse(r#"{"fleet_toml":"/f.toml","children":[]}"#).expect("it parses");
        assert!(bare.controller.is_none());
    }

    /// A scratch directory of this arm's own, remade empty.
    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fleet-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn rendered(id_text: &str, name: Option<&str>, model: &str, tree: &str) -> RenderedSeat {
        RenderedSeat {
            id: id(id_text),
            name: name.map(str::to_string),
            model: model.to_string(),
            worktrees: vec![("p".to_string(), tree.to_string())],
        }
    }

    /// The render: an upsert by id, a row dropped when its seat leaves the
    /// table, and a transient row, a row with no id and an unknown key that
    /// survive it BYTE FOR BYTE — which is what the document-edit discipline is
    /// for.
    #[test]
    fn a_render_upserts_by_id_drops_what_left_and_keeps_every_other_byte() {
        let dir = scratch("render");
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            format!(
                r#"{{
  "fleet_toml": "/f.toml",
  "autopilot": {{"on": true}},
  "children": [
    {{"id": "{SPAWNED}", "kind": "agent", "transient": true, "spawned_by": "somebody",
     "worktrees": {{"p": "/wt/t1"}}}},
    {{"id": "{ORLA}", "name": "Kite", "model": "an-old-model", "chosen_name": "Kite",
     "worktrees": {{"p": "/wt/kite-93b9739a"}}, "kept_by_another_tool": 7}},
    {{"id": "{GONE}", "name": "Gone", "worktrees": {{"p": "/wt/gone"}}}},
    {{"name": "a-row-keyed-by-its-name", "worktrees": {{"p": "/wt/x"}}}}
  ]
}}
"#
            ),
        )
        .unwrap();
        let before: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        let seats = vec![
            rendered(ORLA, Some("Kite"), "a-new-model", "/wt/kite-93b9739a"),
            rendered(PELL, None, "a-model", "/wt/agent-e8a04b17"),
        ];
        let moved = render_seats(&path, &seats).expect("the render lands");
        assert_eq!(moved.added, vec!["agent-e8a04b17"]);
        assert_eq!(moved.updated, vec!["kite-93b9739a"]);
        assert_eq!(moved.dropped, vec!["gone-000a90e5"]);
        assert!(moved.moved());

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let rows = after["children"].as_array().unwrap();
        let row = |id: &str| rows.iter().find(|r| r["id"] == id).expect(id);
        assert_eq!(row(ORLA)["model"], "a-new-model");
        assert_eq!(row(ORLA)["name"], "Kite", "the seat's own name, not a slug");
        assert_eq!(row(ORLA)["kind"], "agent");
        assert!(
            row(ORLA).get("chosen_name").is_none(),
            "a row this render owns keeps no chosen_name, whatever it carried: {after}"
        );
        assert_eq!(
            row(ORLA)["kept_by_another_tool"],
            7,
            "a key this render has nothing to say about survives it"
        );
        assert_eq!(
            *row(PELL),
            serde_json::json!({
                "id": PELL,
                "kind": "agent",
                "model": "a-model",
                "worktrees": {"p": "/wt/agent-e8a04b17"},
            }),
            "a seat with no name is a row with no name"
        );
        assert!(!rows.iter().any(|r| r["id"] == GONE));

        // The transient row and the unknown top-level key, unchanged.
        assert_eq!(*row(SPAWNED), before["children"][0]);
        assert_eq!(after["autopilot"], before["autopilot"]);
        assert_eq!(after["fleet_toml"], before["fleet_toml"]);

        // A row with NO ID is not a seat this list can name, so it is not one
        // this render reconciles: `parse` skips it and reports it, and a render
        // that swept it would delete somebody's data over a key it has nothing
        // to say about. It is not counted as dropped either.
        let keyless: Vec<&serde_json::Value> = rows.iter().filter(|r| r["id"].is_null()).collect();
        assert_eq!(keyless.len(), 1, "the row with no id survives: {after}");
        assert_eq!(*keyless[0], before["children"][3]);

        // A FIXED POINT: the same render again moves nothing and rewrites no
        // byte, which is what makes a second `fleet start` a no-op.
        let bytes = std::fs::read_to_string(&path).unwrap();
        let again = render_seats(&path, &seats).expect("the render lands");
        assert!(!again.moved(), "{again:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);

        // And the parse still reads the rows it is meant to, so the document
        // above is a seat list and not merely valid JSON.
        let read = parse(&bytes).expect("the rendered file parses");
        assert_eq!(
            read.seats
                .iter()
                .map(Seat::machine_name)
                .collect::<Vec<_>>(),
            ["agent-7e3fa2c0", "kite-93b9739a", "agent-e8a04b17"]
        );
        assert_eq!(read.skipped, vec!["a row carries no id".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A RENAME IS AN UPDATE. The row is found by its id, so a seat renamed
    /// from Kite to Wren is the same row with a new name and the worktree it
    /// had — no drop and no add, which is what a render keyed on the machine
    /// name did. A name taken off the table comes off the row.
    #[test]
    fn a_renamed_seat_updates_its_own_row_and_drops_and_adds_nothing() {
        let dir = scratch("rename");
        let path = dir.join("config.json");
        std::fs::write(&path, "{\"fleet_toml\": \"/f.toml\", \"children\": []}\n").unwrap();
        let tree = "/wt/kite-93b9739a";
        render_seats(&path, &[rendered(ORLA, Some("Kite"), "a-model", tree)])
            .expect("the first render lands");

        let moved = render_seats(&path, &[rendered(ORLA, Some("Wren"), "a-model", tree)])
            .expect("the rename lands");
        assert_eq!(
            moved,
            Rendered {
                added: Vec::new(),
                updated: vec!["wren-93b9739a".to_string()],
                dropped: Vec::new(),
            }
        );
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let rows = after["children"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "{after}");
        assert_eq!(rows[0]["id"], ORLA);
        assert_eq!(rows[0]["name"], "Wren");
        assert_eq!(
            rows[0]["worktrees"]["p"], tree,
            "a rename moves no worktree"
        );

        let unnamed = render_seats(&path, &[rendered(ORLA, None, "a-model", tree)])
            .expect("the unnaming lands");
        assert_eq!(unnamed.updated, vec!["agent-93b9739a"]);
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            after["children"][0].get("name").is_none(),
            "a seat with no name keeps none on its row: {after}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two claims back to back, well inside one second: two fresh ids, two
    /// machine names and two worktrees, and each row carries its seat's id and
    /// kind and no name — a transient seat has none of its own. A v7's first
    /// eight hex digits are its clock and repeat for a minute, so a name cut
    /// from them would be one directory twice; the machine name is cut from the
    /// tail [ASSUMES D1].
    #[test]
    fn two_claims_back_to_back_mint_two_ids_and_two_worktrees() {
        let dir = scratch("claim");
        let worktrees = dir.join("worktrees");
        std::fs::create_dir_all(&worktrees).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, "{\"fleet_toml\": \"/f.toml\", \"children\": []}\n").unwrap();

        let seat = TransientSeat {
            project: "p",
            model: "a-model",
            worktrees_dir: &worktrees,
        };
        let make = |_: &str, tree: &Path| std::fs::create_dir(tree).map_err(|e| e.to_string());
        let one = claim_transient_seat(&path, &seat, make).expect("the first claim lands");
        let two = claim_transient_seat(&path, &seat, make).expect("the second claim lands");

        assert_ne!(one.id, two.id, "two claims minted one id");
        assert_ne!(one.name, two.name, "two claims took one name");
        assert_ne!(one.worktree, two.worktree, "two claims cut one directory");
        for claimed in [&one, &two] {
            assert_eq!(claimed.name, format!("agent-{}", claimed.id.short()));
            assert_eq!(claimed.worktree, worktrees.join(&claimed.name));
            assert!(claimed.worktree.is_dir(), "{}", claimed.worktree.display());
        }

        let body = std::fs::read_to_string(&path).unwrap();
        let after: serde_json::Value = serde_json::from_str(&body).unwrap();
        let rows = after["children"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "{after}");
        for (row, claimed) in rows.iter().zip([&one, &two]) {
            assert_eq!(
                *row,
                serde_json::json!({
                    "id": claimed.id.to_string(),
                    "kind": "agent",
                    "transient": true,
                    "model": "a-model",
                    "worktrees": {"p": claimed.worktree.display().to_string()},
                })
            );
        }
        // And the row reads back as the seat the claim named: the machine name
        // the parse derives is the one the worktree was cut under.
        let read = parse(&body).expect("the claimed file parses");
        assert_eq!(
            read.seats
                .iter()
                .map(Seat::machine_name)
                .collect::<Vec<_>>(),
            [one.name.clone(), two.name.clone()]
        );

        // The drop is by id, and an id the file does not carry drops nothing.
        assert_eq!(drop_seat(&path, &one.id), Ok(true));
        assert_eq!(drop_seat(&path, &one.id), Ok(false));
        let left = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(left.seats.len(), 1);
        assert_eq!(left.seats[0].id, two.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_field_is_tolerated() {
        let config = parse(&format!(
            r#"{{"fleet_toml":"/f.toml","autopilot":{{"on":true}},
                "children":[{{"id":"{ORLA}","worktrees":{{"p":"/w"}},"spawned_by":"x"}}]}}"#
        ))
        .expect("unknown fields do not deny the read");
        assert_eq!(config.seats.len(), 1);
    }
}
