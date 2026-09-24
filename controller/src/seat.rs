//! The four seat events, from the writing end.
//!
//! A seat's lifecycle is not a state this controller infers — the roster cannot
//! tell a hibernated session from a deliberately stopped one (lessons
//! claude-code A3) — so it is four events any workflow emits through the CLI,
//! and the controller consumes them on its tick. There is no marker file and no
//! write to the work graph anywhere in this path.

use crate::clock;
use crate::config;
use crate::events::{self, EventLog};
use std::path::Path;

/// The refusals `rest` has, each its own status because each has its own fix.
///
/// 5 is a fleet nobody is collecting — a controller to start. 4 is a seat that
/// is already down — nothing to stop. 6 is a seat of the kind that never rests.
/// 2 is what every other unverifiable answer in this binary is: the caller
/// cannot act on it.
pub const EXIT_READ_BACK_FAILED: u8 = 2;
pub const EXIT_NO_SESSION: u8 = 4;
pub const EXIT_NO_COLLECTOR: u8 = 5;
pub const EXIT_TRANSIENT: u8 = 6;

/// A clear-halt for a seat that is not halted. 1 rather than a status of its
/// own: nothing about the fleet is wrong, the request simply names a seat with
/// no hold on it, and the refusal prints the state it read instead.
pub const EXIT_NOT_HALTED: u8 = 1;

/// How stale the projection may be before the collector counts as gone.
///
/// Freshness is the collector's only liveness signal, so the check is the
/// published document's age against the interval it says it is published at —
/// three of them, which tolerates a poll spent inside a slow effect without
/// tolerating a controller that stopped.
pub const COLLECTOR_STALE_POLLS: u64 = 3;

/// Write one seat event and confirm it landed.
///
/// `woke`, `handed-off` and `exited` are RECORDS and always write: they state
/// what happened, and a record that refused because the fleet looked quiet is
/// one nobody can go back and take. `rest` is the one that asks for something,
/// so it is the one that can refuse.
///
/// THE WRITE IS READ BACK BEFORE THIS ANSWERS 0. A caller acting on a 0 stops
/// working and waits for a successor, so "it was written" has to be a reading of
/// the file and not of this process's own intent.
pub fn record(
    machine_dir: &Path,
    kind: &str,
    seat: &str,
    reason: Option<&str>,
) -> Result<String, (u8, String)> {
    if kind == events::SEAT_RESTING {
        rest_is_answerable(machine_dir, seat)?;
    }
    if kind == events::SEAT_CLEAR_HALT {
        clear_halt_is_answerable(machine_dir, seat)?;
    }
    let payload = match kind {
        events::SEAT_RESTING | events::SEAT_CLEAR_HALT => {
            serde_json::json!({ "reason": reason.unwrap_or_default() })
        }
        _ => serde_json::json!({}),
    };
    let stream = machine_dir.join("events.jsonl");
    let mut log = EventLog::open(&stream);
    if let Err(e) = log.append(kind, seat, payload) {
        return Err((1, format!("could not append to {}: {e}", stream.display())));
    }
    match events::read_after(&stream, log.seq().saturating_sub(1)).last() {
        Some(record) if record.kind == kind && record.actor == seat => {
            Ok(format!("{kind} {seat} — seq {}", record.seq))
        }
        other => Err((
            EXIT_READ_BACK_FAILED,
            format!(
                "the stream's last line reads {} rather than the {kind} for {seat} just \
                 written; the record is not confirmed",
                other
                    .map(|r| format!("a {} for {}", r.kind, r.actor))
                    .unwrap_or_else(|| "nothing".to_string())
            ),
        )),
    }
}

/// Whether a rest can be collected, or which half failed.
///
/// The order is fixed: no collector outranks no session, because a fleet
/// nobody is polling would not act on a rest for a seat that IS live either.
fn rest_is_answerable(machine_dir: &Path, seat: &str) -> Result<(), (u8, String)> {
    let document = fresh_projection(machine_dir)?;

    let seats = document["seats"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let row = seats.iter().find(|row| row["seat_dir"] == seat);
    let state = match row {
        None => None,
        Some(row) => row["roster_state"].as_str(),
    };
    match state {
        Some("present") | Some("prompt-blocked") => {}
        _ => {
            return Err((
                EXIT_NO_SESSION,
                format!(
                    "{seat} has no live session — {}",
                    match state {
                        Some(state) => format!("its row reads {state}"),
                        None => "the projection carries no row for it".to_string(),
                    }
                ),
            ))
        }
    }

    // THE SEAT LIST AND NOT THE PROJECTION. The projection publishes neither a
    // seat's model nor its transience — it reports what was OBSERVED, and which
    // kind of seat a row is is a configuration — so this reads the same
    // `config.json` the controller parses it from.
    if is_transient(machine_dir, seat) {
        return Err((
            EXIT_TRANSIENT,
            format!(
                "{seat} is a transient row, and only named seats rest — use \
                 `fleet seat retire {seat}` instead"
            ),
        ));
    }
    Ok(())
}

/// Whether a clear-halt can be consumed, or which half failed.
///
/// The same collector check a rest takes, and for the same reason: a clear-halt
/// is a REQUEST the controller consumes on its tick, so writing one into a fleet
/// nobody is polling would leave the seat held with a line nothing reads. The
/// second half is the seat's own halt flag, read from the published row — a
/// clear for a seat that is not halted is a typo, and the state it read is what
/// tells the person which seat they meant.
fn clear_halt_is_answerable(machine_dir: &Path, seat: &str) -> Result<(), (u8, String)> {
    let document = fresh_projection(machine_dir)?;
    let seats = document["seats"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let row = seats.iter().find(|row| row["seat_dir"] == seat);
    match row {
        Some(row) if row["halted"].as_bool() == Some(true) => Ok(()),
        Some(row) => Err((
            EXIT_NOT_HALTED,
            format!(
                "{seat} is not halted — its row reads {} with {} blind dispatch(es), and there \
                 is no hold to lift",
                row["roster_state"].as_str().unwrap_or("no roster state"),
                row["blind"].as_u64().unwrap_or(0)
            ),
        )),
        None => Err((
            EXIT_NOT_HALTED,
            format!("{seat} is not halted — the projection carries no row for it"),
        )),
    }
}

/// Whether a collector is plainly consuming this machine's fleet.
///
/// THE COLLECTOR'S OWN RULE and not a second one beside it: it answers off the
/// same [`fresh_projection`] a rest is refused by, so a caller asking whether a
/// controller is up and `fleet event rest` deciding whether anybody would read
/// it cannot disagree.
///
/// A projection that is absent, unparsable, undatable or older than
/// [`COLLECTOR_STALE_POLLS`] intervals all answer `false` — none of them may be
/// rounded into "a controller is running", because each of them is a fleet
/// nobody is polling.
pub fn collector_is_consuming(machine_dir: &Path) -> bool {
    fresh_projection(machine_dir).is_ok()
}

/// The published document, refused unless a collector is plainly consuming.
fn fresh_projection(machine_dir: &Path) -> Result<serde_json::Value, (u8, String)> {
    let path = machine_dir.join("projection.json");
    let refuse = |why: String| Err((EXIT_NO_COLLECTOR, why));
    let Ok(body) = std::fs::read_to_string(&path) else {
        return refuse(format!(
            "no collector is consuming — there is no projection at {}, so nothing would read \
             this rest",
            path.display()
        ));
    };
    let Ok(document) = serde_json::from_str::<serde_json::Value>(&body) else {
        return refuse(format!(
            "no collector is consuming — the projection at {} does not parse",
            path.display()
        ));
    };
    let poll_seconds = document["fleet"]["poll_seconds"]
        .as_u64()
        .unwrap_or(crate::policy::DEFAULT_POLL_SECONDS);
    let generated_at = document["generated_at"].as_str().unwrap_or_default();
    // A stamp that does not parse, and one in the future, are both `None` — and
    // neither may be rounded into "fresh": a document nobody can date is not one
    // anybody can call current.
    match clock::seconds_since_stamp(generated_at) {
        Some(age) if age <= poll_seconds * COLLECTOR_STALE_POLLS => {}
        _ => {
            return refuse(format!(
                "no collector is consuming — the projection at {} was generated at \
                 {generated_at}, which is not inside {COLLECTOR_STALE_POLLS} poll intervals \
                 of {poll_seconds}s",
                path.display()
            ))
        }
    }
    Ok(document)
}

/// Whether the seat list marks this row transient. A list that cannot be read
/// answers `false`: refusing a named seat's rest over an unreadable file would
/// leave the one kind of seat that does rest unable to ask.
fn is_transient(machine_dir: &Path, seat: &str) -> bool {
    config::read(&machine_dir.join("config.json"))
        .ok()
        .map(|config| {
            config
                .seats
                .iter()
                .any(|row| row.name == seat && row.transient)
        })
        .unwrap_or(false)
}
