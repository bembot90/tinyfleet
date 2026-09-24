//! `sessions.json` — what the controller remembers about what it started.
//!
//! Private, small and the controller's own: it holds no work item and no
//! message, and nothing outside this binary reads it. It is not the projection,
//! which is the published view of what was OBSERVED; this is the record of what
//! was DONE, and the two answer different questions.
//!
//! What it is not: a second source of truth. Every row here is derivable from
//! the event stream, and a missing, unparseable or foreign table is REBUILT from
//! it by [`rebuild`] — never trusted and never invented.

use crate::platform;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Bumped by any breaking change to the shape below.
pub const SCHEMA: u32 = 1;

pub const FILE: &str = "sessions.json";

/// One session this controller started.
///
/// The fields above the fold are what the DISPATCH knew; the four below it are
/// filled at the first sighting, because arrival is the roster's answer and
/// never the start's own return (lessons claude-code A7, A14). A row nothing has
/// sighted carries none of them, which is exactly what the arrival window reads.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct SessionRow {
    pub seat: String,
    pub project: String,
    pub worktree: String,
    /// The display name the start passed, which is how a person addresses the
    /// seat and not how this controller keys it.
    pub name: String,
    pub model: String,
    pub posture: String,
    pub first_turn: String,
    pub transient: bool,
    /// The configuration directory THIS session came up under, when it came up
    /// under its own. Absent on a named seat, which takes the fleet's.
    ///
    /// It is on the row because every later act about the session — the listing
    /// that can see it, its transcript, the stop and the removal that address it
    /// — has to be made under the same directory, and the row is the only place
    /// the controller remembers what a dispatch chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    /// The work item this dispatch gave the seat, as the order index named it.
    /// Absent on a start no order accompanied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// The id of the event line this row's CURRENT dispatch came from — the
    /// `session.spawned` or `session.adopted` that opened the row, or the
    /// `session.revived` that last re-opened it — which is the one value tying
    /// a row to the line in the stream that dispatched it. `dispatched_at` is
    /// that same line's stamp.
    pub dispatch_id: String,
    /// Epoch milliseconds, so the arrival window is arithmetic rather than a
    /// parse. Every other stamp in the fleet's published documents is a UTC
    /// string; this file is not published.
    pub dispatched_at: u64,
    /// The agent's own session id: the KEY, because the short id and the display
    /// name are both unstable (lessons claude-code A6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The ADDRESS every subcommand accepts, carried separately from the id
    /// because stopping by the full session id exits 1 (A6). Absent on an
    /// interactive row, which carries no short id at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    /// The session id this row was adopted as. Adoption is once per session, so
    /// a row whose value equals `session_id` is never claimed again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted: Option<String>,
}

/// What the controller remembers about one seat ACROSS its sessions.
///
/// Keyed on the seat and not on a row, because that is what the halt is about: a
/// seat whose dispatches keep going blind accumulates here through as many rows
/// as the loop opened, and the latch outlives every one of them.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct SeatState {
    /// Consecutive blind dispatches, decayed by a sighting and latched at the
    /// limit (`crate::decide::BLIND_LIMIT`).
    #[serde(default)]
    pub blind: u32,
    /// Whether this seat is halted. Written once on the transition into the
    /// halt, and cleared only by a `seat.clear_halt` the controller consumed.
    #[serde(default)]
    pub halted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Table {
    pub schema: u32,
    /// The stream cursor: every event at or below this sequence has been folded
    /// into a verdict already.
    #[serde(default)]
    pub consumed_seq: u64,
    /// Seat to the session id already nudged. Keyed on the SESSION and not on
    /// the seat, so a successor is nudged once again with no bookkeeping of its
    /// own.
    #[serde(default)]
    pub nudged: BTreeMap<String, String>,
    /// The blind counter and the halt latch, per seat. Absent from a table an
    /// older build wrote, which reads as a fleet nothing has gone blind on —
    /// the same thing a fresh table says.
    #[serde(default)]
    pub seats: BTreeMap<String, SeatState>,
    /// The agent daemon's pid at the last poll, once for the table. `None` is a
    /// reading nobody has, which reads as NO replacement rather than as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_pid: Option<u32>,
    #[serde(default)]
    pub sessions: Vec<SessionRow>,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            consumed_seq: 0,
            nudged: BTreeMap::new(),
            seats: BTreeMap::new(),
            daemon_pid: None,
            sessions: Vec::new(),
        }
    }
}

pub fn path_in(machine_dir: &Path) -> PathBuf {
    machine_dir.join(FILE)
}

/// The table this path holds, and the fault there was in reading it.
///
/// TWO ANSWERS, NOT ONE. `None` for the table means NOTHING CAME OFF THE DISK,
/// which is the caller's trigger to rebuild from the stream — and it covers the
/// file that is absent as squarely as the one that would not parse, because a
/// deleted table read as an empty fleet forgets a standing halt.
/// `Some(cause)` beside it is a FAULT to report, and an absent file is not one:
/// a fleet that has started nothing has no table and nothing is wrong, so it
/// answers no table and no cause.
///
/// A table of a schema this build does not know is refused whole rather than
/// half-read, the way the projection's version is: a field that moved under a
/// reader which kept going is worse than a table nobody had.
pub fn read(path: &Path) -> (Option<Table>, Option<String>) {
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None),
        Err(e) => {
            return (
                None,
                Some(format!("{} could not be read: {e}", path.display())),
            )
        }
    };
    match serde_json::from_str::<Table>(&body) {
        Ok(table) if table.schema == SCHEMA => (Some(table), None),
        Ok(table) => (
            None,
            Some(format!(
                "{} is schema {} and this controller reads {SCHEMA}",
                path.display(),
                table.schema
            )),
        ),
        Err(e) => (None, Some(format!("{} did not parse: {e}", path.display()))),
    }
}

pub fn write(path: &Path, table: &Table) -> Result<(), String> {
    let mut body = serde_json::to_string_pretty(table).map_err(|e| e.to_string())?;
    body.push('\n');
    platform::write_atomic(path, body.as_bytes()).map_err(|e| e.to_string())
}

/// The table under the lock that guards a read-modify-write of it, with the
/// lock held for as long as the returned handle lives.
///
/// [`write`] renames a whole document over the path, so two writers that each
/// read this table and then rename their own version over it leave one of the
/// two edits. The lock is [`platform::lock_beside`] on the table's own path,
/// which is what the transient verbs take (`transient::Machine::table_under_lock`),
/// so every writer of this file excludes every other.
///
/// THE TWO ANSWERS [`read`] GIVES, NOT ONE, because what a caller owes a table
/// it cannot read is the caller's own: a verb refuses rather than rename an
/// empty table over a document that may hold every other writer's row, the halt
/// latch and the cursor, while the loop — whose copy is the stream's own fold
/// — writes over it and heals it. `Err` is the LOCK alone: without it there is
/// no read-modify-write to make, so there is nothing else to answer.
pub fn read_under_lock(
    path: &Path,
) -> Result<(std::fs::File, Option<Table>, Option<String>), String> {
    let held = platform::lock_beside(path).map_err(|e| {
        format!(
            "the session table's lock at {} could not be taken: {e}",
            path.display()
        )
    })?;
    let (table, cause) = read(path);
    Ok((held, table, cause))
}

/// The table written back under a lock the caller is holding. The handle is
/// taken by reference so a caller cannot drop the lock and still write.
pub fn write_under_lock(_held: &std::fs::File, path: &Path, table: &Table) -> Result<(), String> {
    write(path, table)
}

/// The moves one writer made, applied onto the table another writer left.
///
/// THREE-WAY AND KEYED ON `dispatch_id`: `base` is the table this writer last
/// agreed with the file on, `mine` is that table as this writer has moved it,
/// and `fresh` is what the file holds now. A row `mine` moved away from `base`
/// is this writer's; every other row is the file's, so a row pushed, edited or
/// dropped under the lock since `base` stands. A row in `base` that `mine` does
/// not hold is out of the result too, and one `fresh` does not hold is not put
/// back.
///
/// `session_id` cannot key this: it is absent on exactly the row a spawn has
/// just opened and nothing has sighted, which is the row the merge is for.
/// `dispatch_id` is on every row and ties it to the one line in the stream that
/// made it — and [`Table::redispatch`], which moves a row onto a new dispatch,
/// reads here as the drop of the old key beside the push of the new, which is
/// what it is.
///
/// The cursor, the daemon's pid, the nudge ledger and the per-seat latches are
/// the loop's alone — no verb writes one — so `mine`'s value stands wherever it
/// has moved off `base`.
pub fn merge(base: &Table, mine: &Table, fresh: Table) -> Table {
    let mut merged = fresh;
    merged.schema = SCHEMA;
    merged.consumed_seq = merged.consumed_seq.max(mine.consumed_seq);
    if mine.daemon_pid != base.daemon_pid {
        merged.daemon_pid = mine.daemon_pid;
    }
    for (seat, session) in &mine.nudged {
        if base.nudged.get(seat) != Some(session) {
            merged.nudged.insert(seat.clone(), session.clone());
        }
    }
    for (seat, state) in &mine.seats {
        if base.seats.get(seat) != Some(state) {
            merged.seats.insert(seat.clone(), state.clone());
        }
    }

    let keys = |table: &Table| -> BTreeSet<String> {
        table
            .sessions
            .iter()
            .map(|row| row.dispatch_id.clone())
            .collect()
    };
    let base_keys = keys(base);
    let mine_keys = keys(mine);
    let base_rows: BTreeMap<&str, &SessionRow> = base
        .sessions
        .iter()
        .map(|row| (row.dispatch_id.as_str(), row))
        .collect();
    let mine_rows: BTreeMap<&str, &SessionRow> = mine
        .sessions
        .iter()
        .map(|row| (row.dispatch_id.as_str(), row))
        .collect();

    // A row in `base` that `mine` does not hold is this writer's own drop.
    merged.sessions.retain(|row| {
        !base_keys.contains(&row.dispatch_id) || mine_keys.contains(&row.dispatch_id)
    });
    for row in &mut merged.sessions {
        let Some(moved) = mine_rows.get(row.dispatch_id.as_str()) else {
            continue;
        };
        if base_rows.get(row.dispatch_id.as_str()) != Some(moved) {
            *row = (*moved).clone();
        }
    }
    let held = keys(&merged);
    for row in &mine.sessions {
        if !held.contains(&row.dispatch_id) && !base_keys.contains(&row.dispatch_id) {
            merged.sessions.push(row.clone());
        }
    }
    merged
}

impl Table {
    /// Whether this fleet has CLAIMED this session — the adoption's own write on
    /// the row, which outlives the poll that made it and the process that made
    /// it, because this file is what a restart reads back.
    ///
    /// Adoption runs once per process and this answers every poll, which is why
    /// a caller asks the table rather than the set a poll just claimed: a
    /// session claimed on the first poll is still this fleet's on the fiftieth.
    pub fn is_adopted(&self, session_id: &str) -> bool {
        self.sessions
            .iter()
            .any(|row| row.adopted.as_deref() == Some(session_id))
    }

    /// The newest row this controller opened for a seat, sighted or not.
    pub fn newest_for(&self, seat: &str) -> Option<&SessionRow> {
        self.sessions
            .iter()
            .filter(|row| row.seat == seat)
            .max_by_key(|row| row.dispatched_at)
    }

    /// The same row, to write to. A seat is carried through as many rows as the
    /// loop opened, so a writer that took the first in file order would move a
    /// dispatch two successors ago.
    pub fn newest_for_mut(&mut self, seat: &str) -> Option<&mut SessionRow> {
        self.sessions
            .iter_mut()
            .filter(|row| row.seat == seat)
            .max_by_key(|row| row.dispatched_at)
    }

    /// The newest row for a seat that is standing on a live session — the one a
    /// stop is addressed to.
    pub fn newest_sighted_for(&self, seat: &str) -> Option<&SessionRow> {
        self.sessions
            .iter()
            .filter(|row| row.seat == seat && row.session_id.is_some())
            .max_by_key(|row| row.first_seen_at.unwrap_or(row.dispatched_at))
    }

    pub fn push(&mut self, row: SessionRow) {
        self.sessions.push(row);
    }

    /// Fold one sighting into the table, and say whether anything moved.
    ///
    /// MATCHED BY WORKTREE, on the newest unsighted row first: the start's own
    /// return carries no session id, so the first row the roster shows in that
    /// directory is the one the dispatch produced. A row already carrying this
    /// session id only has its last sighting moved.
    ///
    /// A sighting that matches no row at all changes nothing and is not an
    /// error: a session this controller did not start is still observed and
    /// published, it is simply not the controller's own.
    pub fn sight(
        &mut self,
        seat: &str,
        worktree: &str,
        session_id: &str,
        short_id: Option<&str>,
        now_ms: u64,
    ) -> bool {
        if let Some(row) = self
            .sessions
            .iter_mut()
            .find(|row| row.session_id.as_deref() == Some(session_id))
        {
            row.last_seen_at = Some(now_ms);
            // A short id the first sighting could not read is still owed: the
            // address is what a stop needs, and a row that never gets one is a
            // session this controller can observe and cannot act on.
            if row.short_id.is_none() {
                if let Some(short) = short_id {
                    row.short_id = Some(short.to_string());
                    return true;
                }
            }
            return true;
        }
        let candidate = self
            .sessions
            .iter_mut()
            .filter(|row| row.seat == seat && row.session_id.is_none() && row.worktree == worktree)
            .max_by_key(|row| row.dispatched_at);
        match candidate {
            Some(row) => {
                row.session_id = Some(session_id.to_string());
                row.short_id = short_id.map(str::to_string);
                row.first_seen_at = Some(now_ms);
                row.last_seen_at = Some(now_ms);
                true
            }
            None => false,
        }
    }

    /// Re-open a row as a fresh dispatch — what a revive does to the row it
    /// attaches to.
    ///
    /// The four sighting fields are cleared and the dispatch stamp is moved, so
    /// the arrival window is keyed to THIS dispatch and a sighting is what
    /// closes it. Without that the row would still read as sighted and the
    /// revive would be issued again on every poll, which is the amplifier the
    /// window exists to stop. The next sighting fills this same row again: it is
    /// the newest unsighted one for the seat in that directory.
    pub fn redispatch(&mut self, session_id: &str, dispatch_id: String, now_ms: u64) -> bool {
        let Some(row) = self
            .sessions
            .iter_mut()
            .find(|row| row.session_id.as_deref() == Some(session_id))
        else {
            return false;
        };
        row.dispatch_id = dispatch_id;
        row.dispatched_at = now_ms;
        row.session_id = None;
        row.short_id = None;
        row.first_seen_at = None;
        row.last_seen_at = None;
        true
    }

    /// Drop a row by session id — what a rest does to the predecessor once the
    /// remove has answered.
    pub fn forget(&mut self, session_id: &str) -> bool {
        let before = self.sessions.len();
        self.sessions
            .retain(|row| row.session_id.as_deref() != Some(session_id));
        self.sessions.len() != before
    }

    /// Whether this seat's current session has already had its one nudge.
    pub fn is_nudged(&self, seat: &str, session_id: &str) -> bool {
        self.nudged.get(seat).map(String::as_str) == Some(session_id)
    }

    pub fn mark_nudged(&mut self, seat: &str, session_id: &str) {
        self.nudged.insert(seat.to_string(), session_id.to_string());
    }

    /// This seat's blind counter and halt latch, defaulted for a seat the table
    /// has never carried one for.
    pub fn seat_state(&self, seat: &str) -> SeatState {
        self.seats.get(seat).cloned().unwrap_or_default()
    }

    pub fn set_seat_state(&mut self, seat: &str, state: SeatState) {
        self.seats.insert(seat.to_string(), state);
    }
}

/// Every row and every counter, folded back out of the event stream.
///
/// This is what makes the table not a second source of truth: a table that is
/// missing, unparseable or of a schema this build refuses is REBUILT here rather
/// than trusted or invented, and every field it carries is derivable from the
/// lines the controller itself wrote.
///
/// The fold: a `session.spawned` opens a row, and a `session.adopted` is a
/// SIGHTING matched by [`Table::sight`]'s rule — a spawned line carries no
/// session id, so only the seat and worktree can find the row it opened — which
/// opens a row of its own only when nothing matches, and marks the row adopted.
/// A `session.revived` re-opens the row its session is carried through as a
/// fresh dispatch, keyed on that line's id and stamped at its own time, because
/// the arrival window is arithmetic over that stamp. A `session.rested` closes
/// the predecessor it names; a `dispatch.blind` moves
/// the seat's counter to the count it carries; a `session.halted` sets the
/// latch; a `seat.clear_halt` clears both. The cursor is the last sequence
/// folded, so the next tick reads from the line after it and no event is
/// consumed twice.
///
/// The daemon pid is deliberately NOT folded and starts at `None`: the stream
/// carries no daemon reading, and `None` reads as no replacement rather than as
/// a false one.
pub fn rebuild(events_path: &Path) -> Table {
    let mut table = Table::default();
    for record in crate::events::read_after(events_path, 0) {
        table.consumed_seq = record.seq;
        let seat = record.actor.clone();
        let payload = &record.payload;
        let text = |key: &str| {
            payload
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };
        // The line's own stamp, in the milliseconds the arrival window is
        // arithmetic over. A line carrying no stamp this parser reads leaves a
        // row dispatched at zero, which is a window that closed long ago rather
        // than one that never does.
        let stamp_ms = crate::clock::secs_of_stamp(&record.ts)
            .map(|secs| secs * 1000)
            .unwrap_or(0);
        let opened = || SessionRow {
            seat: seat.clone(),
            project: text("project").unwrap_or_default(),
            worktree: text("worktree").unwrap_or_default(),
            name: text("name").unwrap_or_default(),
            model: text("model").unwrap_or_default(),
            posture: text("posture").unwrap_or_default(),
            first_turn: text("first_turn").unwrap_or_default(),
            transient: payload
                .get("transient")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            // THE LINE'S OWN ID, which is what the live path keys the row on: a
            // start's dispatch id is the append's return, so no writer puts it
            // in a payload and a payload read here is always empty.
            config_dir: text("config_dir"),
            item: text("item"),
            dispatch_id: record.id.clone(),
            dispatched_at: stamp_ms,
            session_id: text("session"),
            short_id: text("short_id"),
            first_seen_at: None,
            last_seen_at: None,
            adopted: None,
        };
        match record.kind.as_str() {
            crate::events::SESSION_SPAWNED => table.push(opened()),
            crate::events::SESSION_ADOPTED => {
                let session_id = text("session").unwrap_or_default();
                let worktree = text("worktree").unwrap_or_default();
                let short_id = text("short_id");
                if !table.sight(&seat, &worktree, &session_id, short_id.as_deref(), stamp_ms) {
                    table.push(SessionRow {
                        first_seen_at: Some(stamp_ms),
                        last_seen_at: Some(stamp_ms),
                        ..opened()
                    });
                }
                if let Some(row) = table
                    .sessions
                    .iter_mut()
                    .find(|row| row.session_id.as_deref() == Some(session_id.as_str()))
                {
                    row.adopted = Some(session_id);
                }
            }
            // The same two calls the live revive makes (`effect::revive`):
            // re-open the row this session is carried through as a fresh
            // dispatch, and where the seat has no row at all, open one — an
            // attach is its own dispatch either way.
            //
            // A SIGHTING MOVES THE TABLE AND WRITES NO EVENT, so a stream that
            // carried no `session.adopted` for this session has no row the id
            // below can address, and a redispatch keyed on it alone would open
            // a second row for a session that has one. The sighting the live
            // table had is replayed first, by the same rule the adopted arm
            // above uses — this seat's newest unsighted row in that directory —
            // and the redispatch then clears it again exactly as the live one
            // does, so the rebuilt row is the live row and not a twin of it.
            crate::events::SESSION_REVIVED => {
                let session_id = text("session").unwrap_or_default();
                let worktree = text("worktree").unwrap_or_default();
                let address = text("address");
                table.sight(&seat, &worktree, &session_id, address.as_deref(), stamp_ms);
                if !table.redispatch(&session_id, record.id.clone(), stamp_ms) {
                    table.push(SessionRow {
                        session_id: None,
                        ..opened()
                    });
                }
            }
            crate::events::SESSION_RESTED => {
                if let Some(predecessor) = text("predecessor") {
                    table.forget(&predecessor);
                }
            }
            crate::events::DISPATCH_BLIND => {
                let mut state = table.seat_state(&seat);
                state.blind = payload
                    .get("blind")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(state.blind as u64 + 1) as u32;
                table.set_seat_state(&seat, state);
            }
            crate::events::SESSION_HALTED => {
                let mut state = table.seat_state(&seat);
                state.halted = true;
                state.blind = state.blind.max(crate::decide::BLIND_LIMIT);
                table.set_seat_state(&seat, state);
            }
            // A standing latch is a `session.halted` with no clear after it, so
            // the clear is what folds it back down — the same act the tick
            // takes, replayed.
            crate::events::SEAT_CLEAR_HALT => {
                table.set_seat_state(&seat, SeatState::default());
            }
            _ => {}
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_row(seat: &str, worktree: &str, dispatched_at: u64) -> SessionRow {
        SessionRow {
            seat: seat.to_string(),
            project: "demo".to_string(),
            worktree: worktree.to_string(),
            name: "orla".to_string(),
            model: "a-model".to_string(),
            posture: "auto".to_string(),
            first_turn: "/wake s1".to_string(),
            transient: false,
            config_dir: None,
            item: None,
            dispatch_id: format!("dispatch-{dispatched_at}"),
            dispatched_at,
            session_id: None,
            short_id: None,
            first_seen_at: None,
            last_seen_at: None,
            adopted: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fleet-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the directory is made");
        dir
    }

    /// The three ways a table is not there, each an EMPTY table plus a cause —
    /// never an invented one and never a half-read one.
    #[test]
    fn a_missing_unparseable_or_foreign_table_answers_no_table_and_says_which() {
        let dir = scratch("read");
        let path = path_in(&dir);

        // ABSENT ANSWERS NO TABLE, which is what sends the caller to the stream.
        // It is not a fault beside it: a fleet that has started nothing yet has
        // no table and nothing is wrong, so it is the one case with a `None`
        // cause — and reading THAT as a table would forget a standing halt.
        let (table, why) = read(&path);
        assert!(table.is_none(), "nothing came off the disk");
        assert_eq!(why, None, "a table that was never written is not a defect");

        std::fs::write(&path, "{not json at all").unwrap();
        let (table, why) = read(&path);
        assert!(table.is_none());
        assert!(
            why.unwrap_or_default().contains("did not parse"),
            "an unparseable table says so"
        );

        // A schema this build does not read is refused WHOLE. Half-reading a
        // table whose fields moved is worse than starting from none.
        std::fs::write(&path, "{\"schema\": 99, \"consumed_seq\": 7}").unwrap();
        let (table, why) = read(&path);
        assert!(table.is_none(), "nothing was taken from it");
        assert!(why.unwrap_or_default().contains("schema 99"));

        // The control: a table of THIS schema round-trips whole and IS answered,
        // so the three `None`s above are the read's answer and not its only one.
        let mut written = Table {
            consumed_seq: 12,
            ..Table::default()
        };
        written.push(a_row("s1", "/wt/s1", 100));
        written.mark_nudged("s1", "a-session");
        write(&path, &written).expect("the table lands");
        let (back, why) = read(&path);
        assert_eq!(why, None);
        let back = back.expect("a table this build reads comes back");
        assert_eq!(back.consumed_seq, 12);
        assert_eq!(back.sessions.len(), 1);
        assert_eq!(back.nudged.get("s1").map(String::as_str), Some("a-session"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A sighting fills the newest UNSIGHTED row for that seat in that
    /// directory, and a later one only moves the last-seen stamp.
    #[test]
    fn a_sighting_fills_the_newest_unsighted_row_and_then_only_moves_its_stamp() {
        let mut table = Table::default();
        table.push(a_row("s1", "/wt/s1", 100));
        table.push(a_row("s1", "/wt/s1", 200));

        assert!(table.sight("s1", "/wt/s1", "the-session", Some("ab12"), 300));
        let newest = table
            .sessions
            .iter()
            .find(|row| row.dispatched_at == 200)
            .expect("the newer row is there");
        assert_eq!(newest.session_id.as_deref(), Some("the-session"));
        assert_eq!(newest.short_id.as_deref(), Some("ab12"));
        assert_eq!(newest.first_seen_at, Some(300));
        let older = table
            .sessions
            .iter()
            .find(|row| row.dispatched_at == 100)
            .expect("the older row is there");
        assert!(
            older.session_id.is_none(),
            "the older dispatch is not the one this session came from"
        );

        // The same session again moves the last sighting and nothing else.
        assert!(table.sight("s1", "/wt/s1", "the-session", Some("ab12"), 400));
        let newest = table
            .sessions
            .iter()
            .find(|row| row.dispatched_at == 200)
            .unwrap();
        assert_eq!(newest.first_seen_at, Some(300), "the FIRST sighting stands");
        assert_eq!(newest.last_seen_at, Some(400));

        // A directory this seat has no row for changes nothing: a session the
        // controller did not start is observed and published, not adopted.
        assert!(!table.sight("s1", "/wt/elsewhere", "another", Some("cd34"), 500));
        assert!(!table.sight("s2", "/wt/s1", "another", Some("cd34"), 500));
    }

    /// The two lookups the loop and the collection use, and the forget that ends
    /// a row.
    #[test]
    fn the_newest_row_the_newest_sighted_one_and_the_forget_that_ends_it() {
        let mut table = Table::default();
        assert!(table.newest_for("s1").is_none());
        table.push(a_row("s1", "/wt/s1", 100));
        table.push(a_row("s1", "/wt/s1", 300));
        table.push(a_row("s2", "/wt/s2", 200));

        assert_eq!(table.newest_for("s1").map(|r| r.dispatched_at), Some(300));
        assert_eq!(table.newest_for("s2").map(|r| r.dispatched_at), Some(200));
        assert!(
            table.newest_sighted_for("s1").is_none(),
            "a row nothing has sighted is not one a stop can be aimed at"
        );

        table.sight("s1", "/wt/s1", "the-session", Some("ab12"), 400);
        assert_eq!(
            table
                .newest_sighted_for("s1")
                .and_then(|r| r.short_id.clone()),
            Some("ab12".to_string())
        );

        assert!(table.forget("the-session"));
        assert!(table.newest_sighted_for("s1").is_none());
        assert!(
            !table.forget("the-session"),
            "forgetting what is not there changes nothing and says so"
        );
    }

    /// The nudge mark is the SESSION's and not the seat's, which is what re-arms
    /// the one nudge for a successor with no bookkeeping of its own.
    #[test]
    fn the_nudge_mark_is_keyed_on_the_session_and_a_successor_re_arms_it() {
        let mut table = Table::default();
        assert!(!table.is_nudged("s1", "a-session"));
        table.mark_nudged("s1", "a-session");
        assert!(table.is_nudged("s1", "a-session"));
        assert!(
            !table.is_nudged("s1", "the-successor"),
            "a new session for the same seat has had none"
        );
        assert!(!table.is_nudged("s2", "a-session"));

        table.mark_nudged("s1", "the-successor");
        assert!(table.is_nudged("s1", "the-successor"));
        assert!(
            !table.is_nudged("s1", "a-session"),
            "one mark per seat, naming the session it belongs to"
        );
    }

    /// A `session.adopted` folds onto the row its session was spawned into and
    /// never opens a second one for the same session.
    #[test]
    fn a_spawned_then_adopted_session_rebuilds_to_one_row() {
        use crate::events::{EventLog, SESSION_ADOPTED, SESSION_SPAWNED};
        let dir = scratch("adopted-fold");
        let spawned = serde_json::json!({
            "worktree": "/wt/s1", "project": "demo", "name": "orla", "model": "a-model",
            "posture": "auto", "first_turn": "/wake s1", "transient": false, "output": "",
        });
        let adopted = serde_json::json!({
            "session": "a-session", "short_id": "ab12", "worktree": "/wt/s1",
            "project": "demo", "name": "orla", "model": "a-model", "posture": "auto",
            "first_turn": "/wake s1", "transient": false,
        });
        let stream = |name: &str, lines: &[(&str, &serde_json::Value)]| {
            let path = dir.join(name);
            let mut log = EventLog::open(&path);
            for (kind, payload) in lines {
                log.append(kind, "s1", (*payload).clone())
                    .expect("the line lands");
            }
            path
        };

        let path = stream(
            "spawned-then-adopted.jsonl",
            &[(SESSION_SPAWNED, &spawned), (SESSION_ADOPTED, &adopted)],
        );
        let rows = rebuild(&path).sessions;
        assert_eq!(rows.len(), 1, "one session is one row: {rows:?}");
        assert_eq!(rows[0].session_id.as_deref(), Some("a-session"));
        assert_eq!(rows[0].short_id.as_deref(), Some("ab12"));
        assert!(rows[0].first_seen_at.is_some(), "the adoption sighted it");
        assert_eq!(
            rows[0].adopted.as_deref(),
            Some("a-session"),
            "and a restart over the rebuilt table does not claim it again"
        );

        let path = stream("adopted-alone.jsonl", &[(SESSION_ADOPTED, &adopted)]);
        let rows = rebuild(&path).sessions;
        assert_eq!(
            rows.len(),
            1,
            "an adoption nothing spawned opens its row: {rows:?}"
        );
        assert_eq!(rows[0].session_id.as_deref(), Some("a-session"));

        let path = stream(
            "adopted-twice.jsonl",
            &[(SESSION_ADOPTED, &adopted), (SESSION_ADOPTED, &adopted)],
        );
        let rows = rebuild(&path).sessions;
        assert_eq!(
            rows.len(),
            1,
            "two adoptions of one session are one row: {rows:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One raw stream line, so an arm can pin the `id` and the `ts` the fold
    /// reads off it. The writer generates both, and two lines appended in the
    /// same second carry the same stamp — which is a fixture that cannot tell
    /// the two lines apart on the value being asserted.
    fn line(seq: u64, id: &str, ts: &str, kind: &str, payload: serde_json::Value) -> String {
        serde_json::json!({
            "id": id,
            "seq": seq,
            "ts": ts,
            "type": kind,
            "actor": "s1",
            "payload": payload,
        })
        .to_string()
    }

    fn stream_of(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
        let path = dir.join(name);
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(&path, &body).expect("the stream lands");
        path
    }

    fn ms_of(ts: &str) -> u64 {
        crate::clock::secs_of_stamp(ts).expect("the fixture's stamp parses") * 1000
    }

    fn spawned_payload() -> serde_json::Value {
        serde_json::json!({
            "worktree": "/wt/s1", "project": "demo", "name": "orla", "model": "a-model",
            "posture": "auto", "first_turn": "/wake s1", "transient": false, "output": "",
        })
    }

    /// A row this fold opens is keyed on its LINE's own id, which is the value
    /// the live path takes from the append's own return — no writer puts a
    /// dispatch id in a payload, so a payload read here keys every rebuilt row
    /// on the empty string.
    #[test]
    fn a_spawned_line_rebuilds_to_a_row_keyed_on_that_lines_own_id() {
        let dir = scratch("spawned-dispatch-id");
        let ts = "2026-09-12T10:00:00Z";
        let path = stream_of(
            &dir,
            "spawned.jsonl",
            &[line(
                1,
                "ev-spawned",
                ts,
                crate::events::SESSION_SPAWNED,
                spawned_payload(),
            )],
        );

        let rows = rebuild(&path).sessions;
        assert_eq!(rows.len(), 1, "one spawned line is one row: {rows:?}");
        assert_eq!(
            rows[0].dispatch_id, "ev-spawned",
            "the row is keyed on the line that opened it"
        );
        assert_eq!(rows[0].dispatched_at, ms_of(ts));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `session.revived` moves the row it attaches to onto THIS dispatch —
    /// the revived line's id and its stamp — because the arrival window is
    /// arithmetic over that stamp and a window keyed to the spawn would have
    /// closed long before the attach.
    ///
    /// BOTH SEQUENCES THE STREAM CAN CARRY, because they reach the row by
    /// different rules: a spawn and then a revive, which is the ordinary one and
    /// carries no session id anywhere in the stream, since a sighting moves the
    /// table and writes no event; and an adoption before the revive, which does.
    /// One row either way, and it is the row the spawn opened rather than a twin
    /// of it.
    #[test]
    fn a_revived_line_moves_the_rows_dispatch_to_its_own_id_and_stamp() {
        let dir = scratch("revived-dispatch-id");
        let spawn_ts = "2026-09-12T10:00:00Z";
        let adopt_ts = "2026-09-12T10:00:30Z";
        let revive_ts = "2026-09-12T11:15:00Z";
        let adopted = serde_json::json!({
            "session": "a-session", "short_id": "ab12", "worktree": "/wt/s1",
            "project": "demo", "name": "orla", "model": "a-model", "posture": "auto",
            "first_turn": "/wake s1", "transient": false,
        });
        let revived = serde_json::json!({
            "session": "a-session", "address": "ab12", "worktree": "/wt/s1",
            "project": "demo", "outcome": "dispatched",
        });
        let spawned_line = line(
            1,
            "ev-spawned",
            spawn_ts,
            crate::events::SESSION_SPAWNED,
            spawned_payload(),
        );
        let revived_line = line(
            3,
            "ev-revived",
            revive_ts,
            crate::events::SESSION_REVIVED,
            revived,
        );

        for (name, lines) in [
            (
                "spawned-then-revived.jsonl",
                vec![spawned_line.clone(), revived_line.clone()],
            ),
            (
                "adopted-then-revived.jsonl",
                vec![
                    spawned_line.clone(),
                    line(
                        2,
                        "ev-adopted",
                        adopt_ts,
                        crate::events::SESSION_ADOPTED,
                        adopted.clone(),
                    ),
                    revived_line.clone(),
                ],
            ),
        ] {
            let rows = rebuild(&stream_of(&dir, name, &lines)).sessions;
            assert_eq!(
                rows.len(),
                1,
                "{name}: the revive moved a row, not opened one: {rows:?}"
            );
            assert_eq!(
                rows[0].dispatch_id, "ev-revived",
                "{name}: the attach is its own dispatch and the row is keyed on it"
            );
            assert_eq!(
                rows[0].dispatched_at,
                ms_of(revive_ts),
                "{name}: and the window is keyed to the attach, not to the spawn"
            );
            assert!(
                rows[0].session_id.is_none() && rows[0].first_seen_at.is_none(),
                "{name}: the four sighting fields are cleared, so a sighting is what \
                 closes this window: {:?}",
                rows[0]
            );
            assert_eq!(
                rows[0].name, "orla",
                "{name}: and the fields the spawn knew are the row's still"
            );
        }
        assert_ne!(
            ms_of(revive_ts),
            ms_of(spawn_ts),
            "the fixture's two stamps differ, so the stamp assertions discriminate"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `session.revived` line no row matches opens one FROM THAT LINE, so the
    /// four identity fields are the line's rather than empty strings — an empty
    /// display name is an argv defect at the next start, not a cosmetic gap
    /// (`effect::Target::display_name`'s doc). Reachable from a TRIMMED stream:
    /// this fold reads from sequence 0 and the live loop always writes a
    /// row-opening line before the revive that attaches to it.
    #[test]
    fn a_revived_line_alone_opens_a_row_carrying_the_lines_identity_fields() {
        let dir = scratch("revived-alone");
        let ts = "2026-09-12T11:15:00Z";
        let revived = serde_json::json!({
            "session": "a-session", "address": "ab12", "worktree": "/wt/s1",
            "project": "demo", "name": "orla", "model": "a-model", "posture": "auto",
            "first_turn": "/wake s1", "transient": false, "outcome": "dispatched",
        });
        let path = stream_of(
            &dir,
            "revived-alone.jsonl",
            &[line(
                1,
                "ev-revived",
                ts,
                crate::events::SESSION_REVIVED,
                revived,
            )],
        );

        let rows = rebuild(&path).sessions;
        assert_eq!(
            rows.len(),
            1,
            "a revived line no row matches opens one: {rows:?}"
        );
        assert_eq!(
            (
                rows[0].name.as_str(),
                rows[0].model.as_str(),
                rows[0].posture.as_str(),
                rows[0].first_turn.as_str(),
            ),
            ("orla", "a-model", "auto", "/wake s1"),
            "the fallback row's four fields are the line's: {:?}",
            rows[0]
        );
        assert_eq!(
            rows[0].dispatch_id, "ev-revived",
            "and the attach is its own dispatch, so the row is keyed on it"
        );
        assert!(
            rows[0].session_id.is_none() && rows[0].first_seen_at.is_none(),
            "opened unsighted, exactly as the live `open_row` opens it: {:?}",
            rows[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An adoption nothing spawned opens its row on that LINE's id: the stream
    /// holds no earlier line for this session, and the adopted line is both the
    /// row's stamp and the only act of record tying it to a dispatch — an empty
    /// id would read as an item nothing dispatched.
    #[test]
    fn an_adopted_line_that_opens_a_fresh_row_keys_it_on_that_lines_id() {
        let dir = scratch("adopted-dispatch-id");
        let ts = "2026-09-12T10:00:00Z";
        let adopted = serde_json::json!({
            "session": "a-session", "short_id": "ab12", "worktree": "/wt/s1",
            "project": "demo", "name": "orla", "model": "a-model", "posture": "auto",
            "first_turn": "/wake s1", "transient": false,
        });
        let path = stream_of(
            &dir,
            "adopted-alone.jsonl",
            &[line(
                1,
                "ev-adopted",
                ts,
                crate::events::SESSION_ADOPTED,
                adopted,
            )],
        );

        let rows = rebuild(&path).sessions;
        assert_eq!(rows.len(), 1, "the adoption opened its row: {rows:?}");
        assert_eq!(rows[0].dispatch_id, "ev-adopted");
        assert_eq!(rows[0].dispatched_at, ms_of(ts));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn table_of(rows: &[SessionRow]) -> Table {
        Table {
            sessions: rows.to_vec(),
            ..Table::default()
        }
    }

    fn ids(table: &Table) -> Vec<&str> {
        table
            .sessions
            .iter()
            .map(|row| row.dispatch_id.as_str())
            .collect()
    }

    /// Each side keeps what it moved, and neither takes the other's.
    ///
    /// The loop carries this table across polls while the verbs write it under
    /// its lock, so the two disagree by the time the loop puts its copy back:
    /// the merge is what decides which of the two answers each row.
    #[test]
    fn the_merge_keeps_this_writers_moves_and_every_row_this_writer_never_read() {
        let base = table_of(&[a_row("s1", "/wt/s1", 10), a_row("s2", "/wt/s2", 20)]);

        // This writer's copy: it sighted its own row and moved the cursor.
        let mut mine = base.clone();
        mine.sessions[0].session_id = Some("sess-1".to_string());
        mine.consumed_seq = 7;
        mine.set_seat_state(
            "s1",
            SeatState {
                blind: 2,
                halted: false,
            },
        );

        // The file as another writer left it: a row pushed, and an edit to a row
        // this writer did not touch.
        let mut fresh = base.clone();
        fresh.sessions[1].first_turn = "/wake s2 again".to_string();
        fresh.push(a_row("s3", "/wt/s3", 30));

        let merged = merge(&base, &mine, fresh);

        assert_eq!(
            ids(&merged),
            vec!["dispatch-10", "dispatch-20", "dispatch-30"],
            "the pushed row is there and neither standing row was dropped"
        );
        assert_eq!(
            merged.sessions[0].session_id.as_deref(),
            Some("sess-1"),
            "the sighting this writer made stands"
        );
        assert_eq!(
            merged.sessions[1].first_turn, "/wake s2 again",
            "and the edit this writer never read is NOT renamed away"
        );
        assert_eq!(merged.consumed_seq, 7, "the cursor is this writer's alone");
        assert_eq!(
            merged.seat_state("s1"),
            SeatState {
                blind: 2,
                halted: false
            },
            "and so is the blind counter"
        );
    }

    /// A drop is a move like any other, on either side.
    ///
    /// The loop forgets a predecessor's row when a rest is collected, and a
    /// retire drops every row a seat has: a merge that re-pushed what it could
    /// not find would put both back on the next poll.
    #[test]
    fn the_merge_puts_back_neither_side_s_dropped_row() {
        let base = table_of(&[a_row("s1", "/wt/s1", 10), a_row("s2", "/wt/s2", 20)]);

        // `mine` holds no `s2`; the file holds no `s1`.
        let mut mine = base.clone();
        mine.sessions.retain(|row| row.seat != "s2");
        let mut fresh = base.clone();
        fresh.sessions.retain(|row| row.seat != "s1");

        let merged = merge(&base, &mine, fresh);

        assert!(
            ids(&merged).is_empty(),
            "neither drop is undone: {:?}",
            ids(&merged)
        );

        // The pairing: the same merge with nothing dropped keeps both, so the
        // reading above is the drops and not a merge that empties the table.
        let kept = merge(&base, &base.clone(), base.clone());
        assert_eq!(ids(&kept), vec!["dispatch-10", "dispatch-20"]);
    }

    /// The lock is what makes the read-modify-write one act.
    ///
    /// A holder takes the table's lock and edits its copy; a second writer's
    /// read-modify-write is started beside it and must WAIT — a read taken while
    /// the holder is mid-edit reads a table the holder is about to rename over,
    /// and the later write of the two then carries only its own row.
    #[test]
    fn a_read_modify_write_waits_for_the_lock_so_neither_writers_row_is_lost() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let dir = scratch("locked");
        let path = path_in(&dir);
        write(&path, &Table::default()).expect("the empty table is written");

        let (held, theirs, why) = read_under_lock(&path).expect("the holder takes the lock");
        let mut theirs = theirs.unwrap_or_else(|| panic!("the holder's read: {why:?}"));
        theirs.push(a_row("holder", "/wt/holder", 10));

        let entered = Arc::new(AtomicBool::new(false));
        let passed = Arc::new(AtomicBool::new(false));
        let waiter = {
            let path = path.clone();
            let entered = Arc::clone(&entered);
            let passed = Arc::clone(&passed);
            std::thread::spawn(move || {
                entered.store(true, Ordering::SeqCst);
                let (mine, base) = (
                    table_of(&[a_row("waiter", "/wt/waiter", 20)]),
                    Table::default(),
                );
                let (lock, fresh, why) = read_under_lock(&path).expect("the waiter takes the lock");
                passed.store(true, Ordering::SeqCst);
                let fresh = fresh.unwrap_or_else(|| panic!("the waiter's read: {why:?}"));
                let merged = merge(&base, &mine, fresh);
                write_under_lock(&lock, &path, &merged).expect("the waiter writes");
            })
        };

        while !entered.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !passed.load(Ordering::SeqCst),
            "the second read is still waiting while the lock is held"
        );

        write_under_lock(&held, &path, &theirs).expect("the holder writes");
        drop(held);
        waiter
            .join()
            .expect("the waiter finishes once the lock is free");

        assert!(
            passed.load(Ordering::SeqCst),
            "and it got through once the holder let go — the pairing on the reading above"
        );
        let (table, why) = read(&path);
        let table = table.unwrap_or_else(|| panic!("the table reads back: {why:?}"));
        assert_eq!(
            ids(&table),
            vec!["dispatch-10", "dispatch-20"],
            "both writers' rows stand"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
