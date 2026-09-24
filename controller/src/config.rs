//! `config.json` — the machine's list of managed seats.
//!
//! Local, written by `fleet` commands, and re-read whenever its mtime moves.
//! Every writer renames a temp file over the path, so a reader sees a whole old
//! file or a whole new one and no lock is needed.
//!
//! A WRITER still takes one, because a rename is atomic and a read-modify-write
//! is not: two spawns that read the same file and then each rename their own
//! version over it leave one row where there should be two.

use fleet_core::seat::identity::{Kind, SeatId, SeatRef};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Seat {
    /// The seat directory, and the identity key everything else is joined on.
    pub name: String,
    pub chosen_name: Option<String>,
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
    name: Option<String>,
    chosen_name: Option<String>,
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
        let name = match row.name.as_deref().map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => {
                skipped.push("a row carries no `name`".to_string());
                continue;
            }
        };
        let worktrees: Vec<(String, String)> = row
            .worktrees
            .into_iter()
            .filter(|(project, path)| !project.trim().is_empty() && !path.trim().is_empty())
            .collect();
        if worktrees.is_empty() {
            skipped.push(format!("{name} carries no `worktrees` entry"));
            continue;
        }
        seats.push(Seat {
            name,
            chosen_name: row.chosen_name,
            model: row.model,
            transient: row.transient,
            worktrees,
        });
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
        .push(serde_json::json!({
            "name": name,
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

/// Drop one row by name, under the same lock. `Ok(false)` is a name the file
/// does not carry, which is a retire of a row something else already dropped and
/// not a failure.
pub fn drop_seat(path: &Path, name: &str) -> Result<bool, String> {
    let _held = crate::platform::lock_beside(path)?;
    let mut document = read_document(path)?;
    let children = children_of(&mut document)?;
    let before = children.len();
    children.retain(|row| row["name"] != name);
    let dropped = children.len() != before;
    write_document(path, &document)?;
    Ok(dropped)
}

// ---- the `[seats]` render ---------------------------------------------------

/// One agent seat, as `fleet start` renders it into a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedSeat {
    /// The seat's machine name, `<slug>-<short>`: the key the render still
    /// upserts on, until the rows are keyed by id.
    pub name: String,
    pub id: SeatId,
    /// Always present: a start with no model flag comes up on the cheapest
    /// available model (lessons claude-code A5), so the row carries the policy's
    /// default where the table names none.
    pub model: String,
    /// Project to worktree.
    pub worktrees: Vec<(String, String)>,
}

/// What a render moved.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rendered {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    /// Named rows whose seat left the table or turned parked.
    pub dropped: Vec<String>,
}

impl Rendered {
    pub fn moved(&self) -> bool {
        !self.added.is_empty() || !self.updated.is_empty() || !self.dropped.is_empty()
    }
}

/// Render the fleet's active agent seats into `config.json`'s rows, upserting
/// by the machine name.
///
/// UNDER THE SAME LOCK AND THE SAME DOCUMENT-EDIT DISCIPLINE as
/// [`claim_transient_seat`]: the document is edited in place and never
/// re-serialized from [`parse`], so a transient row, an unknown key and every
/// byte of the rest of the file survive a render that did not touch them.
///
/// A TRANSIENT ROW IS NEVER DROPPED. The rows this reconciles are the named
/// ones; a transient seat is a spawn's and a retire's, and a render that swept
/// it would take a live session's row out from under the loop.
pub fn render_seats(path: &Path, seats: &[RenderedSeat]) -> Result<Rendered, String> {
    let _held = crate::platform::lock_beside(path)?;
    let mut document = read_document(path)?;
    let mut moved = Rendered::default();
    {
        let children = children_of(&mut document)?;
        let wanted: Vec<&str> = seats.iter().map(|s| s.name.as_str()).collect();
        children.retain(|row| {
            // A row with NO NAME is not a named seat, so it is not one this
            // render reconciles: `parse` tolerates it and reports it, and a
            // render that swept it would delete somebody's data over a key it
            // has nothing to say about.
            let Some(named) = row["name"].as_str() else {
                return true;
            };
            let transient = row["transient"].as_bool().unwrap_or(false);
            let keep = transient || wanted.contains(&named);
            if !keep {
                moved.dropped.push(named.to_string());
            }
            keep
        });

        for seat in seats {
            let row = row_of(seat);
            let fields = row.as_object().expect("a row is an object").clone();
            match children
                .iter_mut()
                .find(|existing| existing["name"].as_str() == Some(seat.name.as_str()))
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
                    // A row this render owns keeps no `chosen_name`: the key
                    // is the shape seat identity retired, and one left behind
                    // publishes a display name the fleet's own file no longer
                    // says.
                    if let Some(object) = existing.as_object_mut() {
                        changed |= object.remove("chosen_name").is_some();
                    }
                    if changed {
                        moved.updated.push(seat.name.clone());
                    }
                }
                None => {
                    children.push(row);
                    moved.added.push(seat.name.clone());
                }
            }
        }
    }
    write_document(path, &document)?;
    Ok(moved)
}

fn row_of(seat: &RenderedSeat) -> serde_json::Value {
    serde_json::json!({
        "name": seat.name,
        "id": seat.id.to_string(),
        "kind": Kind::Agent.as_str(),
        "model": seat.model,
        "worktrees": seat
            .worktrees
            .iter()
            .map(|(project, path)| (project.clone(), serde_json::Value::String(path.clone())))
            .collect::<serde_json::Map<String, serde_json::Value>>(),
    })
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

    const ONE_SEAT: &str = r#"{
      "fleet_toml": "/fleet/fleet.toml",
      "children": [
        {"name": "builder-1", "chosen_name": "Orla", "model": "a-model",
         "worktrees": {"demo": "/wt/builder-1"}},
        {"name": "builder-2", "worktrees": {}},
        {"chosen_name": "nameless", "worktrees": {"demo": "/wt/x"}}
      ]
    }"#;

    #[test]
    fn a_row_with_no_name_or_no_worktree_is_skipped_loudly() {
        let config = parse(ONE_SEAT).expect("the file parses");
        assert_eq!(config.fleet_toml, PathBuf::from("/fleet/fleet.toml"));
        assert_eq!(config.seats.len(), 1);
        assert_eq!(config.seats[0].name, "builder-1");
        assert_eq!(
            config.seats[0].worktrees,
            vec![("demo".to_string(), "/wt/builder-1".to_string())]
        );
        assert_eq!(config.skipped.len(), 2, "both bad rows are reported");
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
        let config = parse(
            r#"{"fleet_toml":"/f.toml","children":[
                 {"name":"builder-1","model":"a-model","transient":true,
                  "worktrees":{"demo":"/wt/builder-1"}},
                 {"name":"builder-2","worktrees":{"demo":"/wt/builder-2"}}
               ]}"#,
        )
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
        let config = parse(
            r#"{"fleet_toml":"/f.toml","controller":{"poll_seconds":11},
                "children":[{"name":"a","worktrees":{"p":"/w"}}]}"#,
        )
        .expect("the file parses");
        assert_eq!(config.controller.as_ref().unwrap()["poll_seconds"], 11);

        // The control: a file that carries no such object says so, which is a
        // fleet running on its policy alone rather than on an empty override.
        let bare = parse(r#"{"fleet_toml":"/f.toml","children":[]}"#).expect("it parses");
        assert!(bare.controller.is_none());
    }

    /// The render: an upsert by name, a named row dropped when its seat
    /// leaves the table, and a transient row and an unknown key that survive it
    /// BYTE FOR BYTE — which is what the document-edit discipline is for.
    #[test]
    fn a_render_upserts_by_name_drops_what_left_and_keeps_every_other_byte() {
        let dir = std::env::temp_dir().join(format!("fleet-render-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{
  "fleet_toml": "/f.toml",
  "autopilot": {"on": true},
  "children": [
    {"name": "agent-7e3fa2c0", "transient": true, "spawned_by": "somebody",
     "worktrees": {"p": "/wt/t1"}},
    {"name": "one", "model": "an-old-model", "chosen_name": "Kite",
     "worktrees": {"p": "/wt/one"}, "kept_by_another_tool": 7},
    {"name": "gone", "worktrees": {"p": "/wt/gone"}},
    {"chosen_name": "a row with no name", "worktrees": {"p": "/wt/x"}}
  ]
}
"#,
        )
        .unwrap();
        let before: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        let id = |text: &str| SeatId::parse(text).expect("a hand-written seat id parses");
        let seats = vec![
            RenderedSeat {
                name: "one".to_string(),
                id: id("01a0d1f1-0aec-765f-9abe-d4f993b9739a"),
                model: "a-new-model".to_string(),
                worktrees: vec![("p".to_string(), "/wt/one".to_string())],
            },
            RenderedSeat {
                name: "two".to_string(),
                id: id("01a0d1f1-0aec-765f-9abe-5c21e8a04b17"),
                model: "a-model".to_string(),
                worktrees: vec![("p".to_string(), "/wt/two".to_string())],
            },
        ];
        let moved = render_seats(&path, &seats).expect("the render lands");
        assert_eq!(moved.added, vec!["two"]);
        assert_eq!(moved.updated, vec!["one"]);
        assert_eq!(moved.dropped, vec!["gone"]);
        assert!(moved.moved());

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let rows = after["children"].as_array().unwrap();
        let row = |name: &str| rows.iter().find(|r| r["name"] == name).expect(name);
        assert_eq!(row("one")["model"], "a-new-model");
        assert_eq!(row("one")["id"], "01a0d1f1-0aec-765f-9abe-d4f993b9739a");
        assert_eq!(row("one")["kind"], "agent");
        assert!(
            row("one").get("chosen_name").is_none(),
            "a row this render owns keeps no chosen_name, whatever it carried: {after}"
        );
        assert_eq!(
            row("one")["kept_by_another_tool"],
            7,
            "a key this render has nothing to say about survives it"
        );
        assert_eq!(row("two")["id"], "01a0d1f1-0aec-765f-9abe-5c21e8a04b17");
        assert_eq!(row("two")["kind"], "agent");
        assert!(row("two").get("chosen_name").is_none(), "{after}");
        assert!(!rows.iter().any(|r| r["name"] == "gone"));

        // The transient row and the unknown top-level key, unchanged.
        assert_eq!(*row("agent-7e3fa2c0"), before["children"][0]);
        assert_eq!(after["autopilot"], before["autopilot"]);
        assert_eq!(after["fleet_toml"], before["fleet_toml"]);

        // A row with NO NAME is not a named seat, so it is not one this render
        // reconciles: `parse` tolerates it and reports it, and a render that
        // swept it would delete somebody's data over a key it has nothing to
        // say about. It is not counted as dropped either.
        let nameless: Vec<&serde_json::Value> =
            rows.iter().filter(|r| r["name"].is_null()).collect();
        assert_eq!(nameless.len(), 1, "the nameless row survives: {after}");
        assert_eq!(*nameless[0], before["children"][3]);
        assert_eq!(
            moved.dropped,
            vec!["gone"],
            "only the named row whose seat left the table is dropped"
        );

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
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["agent-7e3fa2c0", "one", "two"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two claims back to back, well inside one second: two fresh ids, two
    /// machine names and two worktrees, and each row carries its seat's id and
    /// kind. A v7's first eight hex digits are its clock and repeat for a
    /// minute, so a name cut from them would be one directory twice; the
    /// machine name is cut from the tail [ASSUMES D1].
    #[test]
    fn two_claims_back_to_back_mint_two_ids_and_two_worktrees() {
        let dir = std::env::temp_dir().join(format!("fleet-claim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let rows = after["children"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "{after}");
        for (row, claimed) in rows.iter().zip([&one, &two]) {
            assert_eq!(
                *row,
                serde_json::json!({
                    "name": claimed.name,
                    "id": claimed.id.to_string(),
                    "kind": "agent",
                    "transient": true,
                    "model": "a-model",
                    "worktrees": {"p": claimed.worktree.display().to_string()},
                })
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_field_is_tolerated() {
        let config = parse(
            r#"{"fleet_toml":"/f.toml","autopilot":{"on":true},
                "children":[{"name":"a","worktrees":{"p":"/w"},"spawned_by":"x"}]}"#,
        )
        .expect("unknown fields do not deny the read");
        assert_eq!(config.seats.len(), 1);
    }
}
