//! The event stream (PRD R25): append-only JSONL, one object per line,
//! monotonically sequenced.
//!
//! The content rule is the whole design: emit the events a person cares about —
//! a start, a stop, a substrate that moved under the fleet — and nothing per
//! poll (lessons gas-city G17). A stream that reports the controller's own
//! health drowns the three lines a person came to read.

use crate::clock;
use serde::Serialize;
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct Event<'a> {
    pub id: String,
    pub seq: u64,
    pub ts: String,
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub actor: &'a str,
    pub payload: serde_json::Value,
}

/// The actor on an event the controller itself is the subject of. Every other
/// event names the seat.
pub const CONTROLLER: &str = "controller";

/// The four events a seat's workflow emits through `fleet event` (PRD R20).
///
/// The type is a string at the append, because the stream carries types this
/// module does not enumerate and an enum there would refuse them. These names
/// live in ONE place so the CLI that writes them, the consumer that folds them
/// and a test that enumerates them cannot spell them differently.
pub const SEAT_WOKE: &str = "seat.woke";
pub const SEAT_RESTING: &str = "seat.resting";
pub const SEAT_HANDED_OFF: &str = "seat.handed_off";
pub const SEAT_EXITED: &str = "seat.exited";
/// The remedy a person writes when a seat is halted (R14). A REQUEST, like a
/// rest: the controller consumes it on its tick and resets the blind counter and
/// the latch. It is a seat-emitted type because a person writes it through the
/// seat's own event family, not because the seat's workflow emits it.
pub const SEAT_CLEAR_HALT: &str = "seat.clear_halt";

/// The five, in the order `fleet event`'s usage names them.
pub const SEAT_TYPES: [&str; 5] = [
    SEAT_WOKE,
    SEAT_RESTING,
    SEAT_HANDED_OFF,
    SEAT_EXITED,
    SEAT_CLEAR_HALT,
];

/// The types this controller writes. A seat never writes one of these and the
/// controller never writes one of the four above: the stream runs both ways and
/// the two halves do not overlap.
pub const CONTROLLER_STARTED: &str = "controller.started";
pub const CONTROLLER_STOPPED: &str = "controller.stopped";
pub const SUBSTRATE_MOVED: &str = "substrate.moved";
pub const SESSION_SPAWNED: &str = "session.spawned";
pub const SESSION_RESTED: &str = "session.rested";
pub const SESSION_NUDGED: &str = "session.nudged";
pub const SESSION_CRASHED: &str = "session.crashed";
/// The four this slice adds (R25). Each is written once by the layer that did
/// the thing: the effect layer revives, adopt at startup claims, the halt
/// transition latches, and a counted blind dispatch says so.
pub const SESSION_REVIVED: &str = "session.revived";
pub const SESSION_ADOPTED: &str = "session.adopted";
pub const SESSION_HALTED: &str = "session.halted";
pub const DISPATCH_BLIND: &str = "dispatch.blind";
/// A dispatch whose seat came up LOGGED OUT (flights PRD R13). Written once per
/// row, at the first sighting whose transcript carries the provider's
/// not-logged-in answer, and it carries the seat and the item the order index
/// named. The controller only reports it: holding the item, retiring the seat
/// and halting the flight's dispatching are the advance's.
pub const DISPATCH_FAILED: &str = "dispatch.failed";
/// A transient seat retired (PRD R32, cli PRD § `fleet seat retire`). It carries
/// the reclaim — the worktree's bytes, the pid, and whether `--dead` licensed
/// it — because a retire that verified from outside owes the numbers it read.
pub const SESSION_STOPPED: &str = "session.stopped";
/// A seat retired by a flight, with what it cost (flights PRD R10, R12). It
/// carries the reclaim `session.stopped` does and, beside it, the three cost
/// readings and the branch and commit the worktree held — each of them null
/// where the reading could not be taken, because a zero would read as free.
///
/// A SECOND LINE AND NOT A WIDER `session.stopped`: the reclaim is what every
/// retire owes and the cost is what a retire INSIDE A FLIGHT owes, so the two
/// have one writer each and a hand-run retire is unchanged.
pub const SESSION_RETIRED: &str = "session.retired";
/// A project registered with a standalone fleet (PRD R3). Written by
/// `fleet create --standalone`, carrying the root and the name the registry row
/// took — the two fields a reader of the registry would otherwise have to open
/// the file for.
pub const PROJECT_REGISTERED: &str = "project.registered";

/// The four a routine's firing writes (PRD R23). They are the ledger: the cli
/// reads a routine's history from the stream with a sequence cursor, so there is
/// no second file beside it and no row nobody sequenced.
///
/// A firing is `routine.fired` and then exactly one of the three terminal types;
/// a NOT-DUE evaluation writes none of them. The payload names the routine
/// under the key `order`, the file format's word, until the format is renamed.
pub const ROUTINE_FIRED: &str = "routine.fired";
pub const ROUTINE_COMPLETED: &str = "routine.completed";
pub const ROUTINE_FAILED: &str = "routine.failed";
pub const ROUTINE_COULD_NOT_TELL: &str = "routine.could_not_tell";

/// The four, for a reader that filters the stream down to one routine's history.
pub const ROUTINE_TYPES: [&str; 4] = [
    ROUTINE_FIRED,
    ROUTINE_COMPLETED,
    ROUTINE_FAILED,
    ROUTINE_COULD_NOT_TELL,
];

pub const CONTROLLER_TYPES: [&str; 19] = [
    CONTROLLER_STARTED,
    CONTROLLER_STOPPED,
    SUBSTRATE_MOVED,
    SESSION_SPAWNED,
    SESSION_RESTED,
    SESSION_NUDGED,
    SESSION_CRASHED,
    SESSION_REVIVED,
    SESSION_ADOPTED,
    SESSION_HALTED,
    SESSION_STOPPED,
    SESSION_RETIRED,
    PROJECT_REGISTERED,
    DISPATCH_BLIND,
    DISPATCH_FAILED,
    ROUTINE_FIRED,
    ROUTINE_COMPLETED,
    ROUTINE_FAILED,
    ROUTINE_COULD_NOT_TELL,
];

/// What a [`DISPATCH_FAILED`] line carries: the seat that came up logged out, the
/// item the order index named, and the provider's own cause.
///
/// The item is `None` on a start no order accompanied, and the key is present
/// carrying null rather than dropped, so a reader folding the line meets a field
/// it can read as absent instead of a shape that varies.
pub fn dispatch_failed_payload(seat: &str, item: Option<&str>, cause: &str) -> serde_json::Value {
    serde_json::json!({ "seat": seat, "item": item, "cause": cause })
}

/// Whether a `seat.resting` or a `seat.exited` — the two the discriminator for a
/// pid-less row reads as a deliberate end (PRD R10, lessons claude-code A3).
pub fn is_deliberate_end(kind: &str) -> bool {
    kind == SEAT_RESTING || kind == SEAT_EXITED
}

/// One line of the stream, as a consumer reads it back.
///
/// Separate from [`Event`], which is the write shape: a reader takes the fields
/// it folds on and owns them, so a line carrying a type this build does not know
/// still parses and can be counted.
#[derive(Clone, Debug)]
pub struct Record {
    pub seq: u64,
    /// The line's own id — the value a row that this event opened is keyed on.
    /// Empty when the line carries none, which a rebuild reads as a row nothing
    /// ties to a dispatch rather than as a missing line.
    pub id: String,
    /// The stamp the writer put on the line, empty when the line carries none.
    /// A reader that REQUIRED it would drop a line the sequence still numbers.
    pub ts: String,
    pub kind: String,
    pub actor: String,
    pub payload: serde_json::Value,
}

/// Every event in the file above `after`, as the file holds them.
///
/// A line that does not parse, or that carries no `seq`, is SKIPPED rather than
/// ending the read: the stream is appended to while it is read, so a torn last
/// line is an expected transient and the lines before it are still the record.
/// A file that is not there is an empty read, which is a fleet nobody has asked
/// anything of yet.
pub fn read_after(path: &Path, after: u64) -> Vec<Record> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(&line).ok()?;
            let seq = value.get("seq")?.as_u64()?;
            if seq <= after {
                return None;
            }
            Some(Record {
                seq,
                id: value
                    .get("id")
                    .and_then(|id| id.as_str())
                    .unwrap_or_default()
                    .to_string(),
                ts: value
                    .get("ts")
                    .and_then(|ts| ts.as_str())
                    .unwrap_or_default()
                    .to_string(),
                kind: value.get("type")?.as_str()?.to_string(),
                actor: value.get("actor")?.as_str()?.to_string(),
                payload: value
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            })
        })
        .collect()
}

/// One line of the stream as a TAIL reads it (cli PRD § `fleet event tail`).
///
/// Separate from [`Record`], which is the fold's shape: a tail prints the line's
/// own bytes and never re-serializes, because one JSON object per line exactly
/// as stored is the contract and a re-serialization would reorder the keys. The
/// two parsed names are the filters' and nothing else's, and each is optional:
/// a stored line carrying a sequence and no actor is still a line of the stream,
/// where [`read_after`] drops it.
#[derive(Clone, Debug)]
pub struct RawLine {
    pub seq: u64,
    pub ts: String,
    pub kind: Option<String>,
    pub actor: Option<String>,
    /// The line exactly as stored, its trailing newline stripped.
    pub text: String,
}

/// One line the by-id reader answered.
#[derive(Clone, Debug)]
pub struct IdMatch {
    pub seq: u64,
    pub value: serde_json::Value,
}

/// The stamp shape [`crate::clock::stamp_secs`] writes, named for a refusal a
/// caller can act on.
pub const STAMP_SHAPE: &str = "YYYY-MM-DDTHH:MM:SSZ";

/// A `--since` value that is neither a sequence nor a stamp of [`STAMP_SHAPE`].
#[derive(Debug, PartialEq, Eq)]
pub struct MalformedStamp;

/// Every line above `after`, in file order, with the bytes each was stored as.
pub fn read_lines_after(path: &Path, after: u64) -> Vec<RawLine> {
    read_lines_from(path, 0, after).0
}

/// The lines above `after` in the file's bytes from `offset` on, and the offset
/// just past the last COMPLETE line read.
///
/// The second answer is a follower's cursor, and only a newline-terminated line
/// is answered: an unterminated tail is held and re-read on the next pass rather
/// than skipped, because the writer is still finishing it and [`read_after`]'s
/// skip — right for a snapshot — would lose that event for good. A line that
/// does not parse, or that carries no `seq`, is skipped as in the snapshot.
pub fn read_lines_from(path: &Path, offset: u64, after: u64) -> (Vec<RawLine>, u64) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return (Vec::new(), offset);
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return (Vec::new(), offset);
    }
    let mut body = Vec::new();
    if file.read_to_end(&mut body).is_err() {
        return (Vec::new(), offset);
    }
    let Some(last_newline) = body.iter().rposition(|byte| *byte == b'\n') else {
        return (Vec::new(), offset);
    };
    let complete = last_newline + 1;
    let text = String::from_utf8_lossy(&body[..complete]).into_owned();
    let lines = text
        .lines()
        .filter_map(|line| raw_line(line, after))
        .collect();
    (lines, offset + complete as u64)
}

fn raw_line(line: &str, after: u64) -> Option<RawLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let seq = value.get("seq")?.as_u64()?;
    if seq <= after {
        return None;
    }
    Some(RawLine {
        seq,
        ts: string_field(&value, "ts").unwrap_or_default(),
        kind: string_field(&value, "type"),
        actor: string_field(&value, "actor"),
        text: line.to_string(),
    })
}

fn string_field(value: &serde_json::Value, name: &str) -> Option<String> {
    Some(value.get(name)?.as_str()?.to_string())
}

/// Every line whose `id` is `id`, as the file holds them.
///
/// Ids are unique by construction — [`event_id`] writes the wall clock's
/// nanoseconds and then the sequence — so a second match is a corrupted stream,
/// which the caller says rather than picking one of the two.
pub fn find_by_id(path: &Path, id: &str) -> Vec<IdMatch> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(&line).ok()?;
            let seq = value.get("seq")?.as_u64()?;
            (value.get("id")?.as_str()? == id).then_some(IdMatch { seq, value })
        })
        .collect()
}

/// The first sequence in file order whose stamp is at or after `stamp`, or
/// `None` for a stamp past the end of the stream.
///
/// The comparison is lexical, which is chronological for the fixed-width shape
/// [`crate::clock::stamp_secs`] writes — and a value of any other shape is
/// refused rather than read leniently, because a stamp nobody meant resolves to
/// a cursor nobody meant.
pub fn seq_at_or_after(path: &Path, stamp: &str) -> Result<Option<u64>, MalformedStamp> {
    if clock::secs_of_stamp(stamp).is_none() {
        return Err(MalformedStamp);
    }
    Ok(read_lines_after(path, 0)
        .into_iter()
        .find(|line| line.ts.as_str() >= stamp)
        .map(|line| line.seq))
}

pub struct EventLog {
    path: PathBuf,
    seq: u64,
}

impl EventLog {
    /// Continue the stream at the file's own last sequence, so a restart does
    /// not renumber lines a reader has already seen.
    pub fn open(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            seq: last_seq(path).unwrap_or(0),
        }
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Append one event, at the next sequence THE FILE has room for.
    ///
    /// The file is re-read for its last sequence and the higher of the two wins,
    /// because the stream has a second writer: `fleet event` appends from its own
    /// process while this loop is running, and a sequence held only in memory
    /// would hand a controller event the number a seat event already took. The
    /// read costs one pass per append and appends are rare by the content rule
    /// above — nothing is written per poll.
    pub fn append(
        &mut self,
        kind: &str,
        actor: &str,
        payload: serde_json::Value,
    ) -> std::io::Result<()> {
        self.append_id(kind, actor, payload).map(|_| ())
    }

    /// The same append, answering with the id it wrote. A row keyed on a
    /// dispatch needs the id of the event that opened it, and reading it back
    /// off the file afterwards would be a second read of a line this call
    /// already had in hand.
    pub fn append_id(
        &mut self,
        kind: &str,
        actor: &str,
        payload: serde_json::Value,
    ) -> std::io::Result<String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        // Held until `file` drops: a writer in another process must not read the
        // same last sequence between this read and this append (PRD R25).
        file.lock()?;
        self.seq = self.seq.max(last_seq(&self.path).unwrap_or(0)) + 1;
        let id = event_id(self.seq);
        let event = Event {
            id: id.clone(),
            seq: self.seq,
            ts: clock::now_stamp(),
            kind,
            actor,
            payload,
        };
        let line = serde_json::to_string(&event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{line}")?;
        Ok(id)
    }
}

/// Unique without a dependency: the sequence this stream is at, under the wall
/// clock's nanoseconds, which no second line in one stream shares.
fn event_id(seq: u64) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:032x}-{seq:08x}")
}

/// The highest `seq` in the file. A line that does not parse is skipped: a torn
/// last line must not renumber the stream from zero.
fn last_seq(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            serde_json::from_str::<serde_json::Value>(&line)
                .ok()?
                .get("seq")?
                .as_u64()
        })
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reopened_stream_continues_the_sequence() {
        let dir = std::env::temp_dir().join(format!("fleet-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("events.jsonl");

        let mut log = EventLog::open(&path);
        log.append("controller.started", CONTROLLER, serde_json::json!({}))
            .unwrap();
        log.append("controller.stopped", CONTROLLER, serde_json::json!({}))
            .unwrap();
        assert_eq!(log.seq(), 2);

        let reopened = EventLog::open(&path);
        assert_eq!(reopened.seq(), 2, "the stream resumes, never renumbers");

        let body = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2, "one JSON object per line");
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        for field in ["id", "seq", "ts", "type", "actor", "payload"] {
            assert!(first.get(field).is_some(), "the envelope carries {field}");
        }
        assert_eq!(first["type"], "controller.started");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two events in one stream never share an id: a reader de-duplicating on
    /// it would collapse them into one.
    #[test]
    fn every_event_carries_an_id_of_its_own() {
        let dir = std::env::temp_dir().join(format!("fleet-events-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("events.jsonl");
        let mut log = EventLog::open(&path);
        for _ in 0..5 {
            log.append("controller.started", CONTROLLER, serde_json::json!({}))
                .unwrap();
        }
        let ids: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["id"].to_string())
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), 5);
        assert_eq!(unique.len(), 5, "ids repeated: {ids:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The consumer's read: everything ABOVE the cursor, in file order, with a
    /// torn line skipped rather than ending the read.
    ///
    /// The stream is appended to while it is read, so a torn last line is an
    /// expected transient — and a reader that stopped at it would lose every
    /// line after the next append.
    #[test]
    fn the_reader_answers_the_lines_above_the_cursor_and_skips_what_will_not_parse() {
        let dir = std::env::temp_dir().join(format!("fleet-events-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("events.jsonl");

        assert!(
            read_after(&path, 0).is_empty(),
            "a stream that is not there is an empty read, not a failure"
        );

        let mut log = EventLog::open(&path);
        log.append(SEAT_WOKE, "s1", serde_json::json!({})).unwrap();
        log.append(SEAT_RESTING, "s1", serde_json::json!({"reason": "a nap"}))
            .unwrap();
        log.append(SEAT_EXITED, "s2", serde_json::json!({}))
            .unwrap();

        let all = read_after(&path, 0);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].kind, SEAT_WOKE);
        assert_eq!(all[1].actor, "s1");
        assert_eq!(all[1].payload["reason"], "a nap");
        assert_eq!(all[2].seq, 3);

        // The cursor: at 2, only the third line is above it, and at the last
        // sequence there is nothing left to read.
        assert_eq!(read_after(&path, 2).len(), 1);
        assert_eq!(read_after(&path, 2)[0].kind, SEAT_EXITED);
        assert!(read_after(&path, 3).is_empty());

        // A torn last line, and a whole one after it: the torn one is skipped and
        // the read keeps going, so nothing behind it is lost.
        let mut body = std::fs::read_to_string(&path).unwrap();
        body.push_str("{\"seq\":4,\"type\":\"seat.wo");
        body.push('\n');
        body.push_str("{\"id\":\"x\",\"seq\":5,\"ts\":\"t\",\"type\":\"seat.woke\",\"actor\":\"s3\",\"payload\":{}}\n");
        std::fs::write(&path, body).unwrap();
        let after = read_after(&path, 3);
        assert_eq!(after.len(), 1, "the torn line is skipped: {after:?}");
        assert_eq!(after[0].seq, 5);
        assert_eq!(after[0].actor, "s3");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The stream has a second writer, and two events never share a sequence.
    ///
    /// `fleet event` appends from its own process while the loop is running, so a
    /// sequence held only in memory would hand a controller event the number a
    /// seat event already took — and a reader keyed on the cursor would then skip
    /// one of the two.
    #[test]
    fn a_second_writer_never_takes_a_sequence_the_first_one_holds() {
        let dir = std::env::temp_dir().join(format!("fleet-events-two-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("events.jsonl");

        // The loop's log, opened and holding a sequence in memory.
        let mut loop_log = EventLog::open(&path);
        loop_log
            .append(CONTROLLER_STARTED, CONTROLLER, serde_json::json!({}))
            .unwrap();

        // The CLI's, a separate handle on the same file — which is what a second
        // process is.
        let mut cli = EventLog::open(&path);
        cli.append(SEAT_RESTING, "s1", serde_json::json!({"reason": "x"}))
            .unwrap();

        // And the loop writes again, without having re-opened.
        loop_log
            .append(SESSION_RESTED, "s1", serde_json::json!({}))
            .unwrap();

        let lines = read_after(&path, 0);
        let seqs: Vec<u64> = lines.iter().map(|r| r.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3], "no sequence is taken twice: {seqs:?}");
        assert_eq!(lines[2].kind, SESSION_RESTED);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two writers appending at once never share a sequence: the read of the
    /// last one and the append after it happen under the stream's lock.
    #[test]
    fn two_writers_racing_on_one_stream_never_share_a_sequence() {
        const WRITERS: u64 = 2;
        const PER_WRITER: u64 = 400;
        let dir = std::env::temp_dir().join(format!("fleet-events-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("events.jsonl");

        let start = std::sync::Barrier::new(WRITERS as usize);
        std::thread::scope(|scope| {
            for writer in 0..WRITERS {
                let (path, start) = (&path, &start);
                scope.spawn(move || {
                    // Its own open: to flock, a separate open file description is
                    // what a second process is.
                    let mut log = EventLog::open(path);
                    let actor = format!("w{writer}");
                    start.wait();
                    for _ in 0..PER_WRITER {
                        log.append(SEAT_WOKE, &actor, serde_json::json!({}))
                            .unwrap();
                    }
                });
            }
        });

        let total = WRITERS * PER_WRITER;
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            body.lines().count() as u64,
            total,
            "every append is one line"
        );

        let mut seqs: Vec<u64> = read_after(&path, 0).iter().map(|r| r.seq).collect();
        seqs.sort_unstable();
        let mut duplicated: Vec<u64> = seqs
            .windows(2)
            .filter(|w| w[0] == w[1])
            .map(|w| w[0])
            .collect();
        duplicated.dedup();
        let mut distinct = seqs.clone();
        distinct.dedup();
        let expected: Vec<u64> = (1..=total).collect();
        assert!(
            seqs == expected,
            "sequences taken twice: {duplicated:?} ({} lines read, {} distinct, {total} appended)",
            seqs.len(),
            distinct.len()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The vocabulary is named in one place, and the two halves do not overlap:
    /// a seat never writes a controller event and the controller never writes one
    /// of the four.
    #[test]
    fn the_two_halves_of_the_vocabulary_are_named_once_and_do_not_overlap() {
        for kind in SEAT_TYPES {
            assert!(
                !CONTROLLER_TYPES.contains(&kind),
                "{kind} is on both halves of the stream"
            );
            assert!(kind.starts_with("seat."), "{kind}");
        }
        let mut all: Vec<&str> = SEAT_TYPES
            .iter()
            .chain(CONTROLLER_TYPES.iter())
            .copied()
            .collect();
        let listed = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), listed, "a type is named once: {all:?}");

        // The transient verbs' own end-of-life line, which is the controller's
        // and not a seat's: a seat says `seat.exited`, and `session.stopped` is
        // what `fleet seat retire` writes ABOUT one.
        assert!(CONTROLLER_TYPES.contains(&SESSION_STOPPED));
        assert!(!SEAT_TYPES.contains(&SESSION_STOPPED));
        assert_ne!(
            SESSION_STOPPED, SEAT_EXITED,
            "the two ends are different lines with different authors"
        );

        // The logged-out dispatch's line is the CONTROLLER's: a seat cannot
        // report that its own session has no credential, because a session that
        // came up logged out never ran a turn that could write one.
        assert!(CONTROLLER_TYPES.contains(&DISPATCH_FAILED));
        assert!(!SEAT_TYPES.contains(&DISPATCH_FAILED));
        assert_ne!(
            DISPATCH_FAILED, DISPATCH_BLIND,
            "a dispatch nobody could SEE and one that came up logged out are \
             different lines with different remedies"
        );
        // Its payload names the seat, the item the order index named, and the
        // cause — and the item key is PRESENT carrying null on a start no order
        // accompanied, so a reader folding the line meets a field it can read as
        // absent rather than a shape that varies.
        let named = dispatch_failed_payload("builder-9", Some("an-item"), "authentication_failed");
        assert_eq!(named["seat"], "builder-9");
        assert_eq!(named["item"], "an-item");
        assert_eq!(named["cause"], "authentication_failed");
        let unordered = dispatch_failed_payload("builder-9", None, "authentication_failed");
        assert!(
            unordered.get("item").is_some_and(|item| item.is_null()),
            "{unordered}"
        );

        // The cost line is the controller's too, and it is a DIFFERENT line
        // from the reclaim: one retire writes both, and a reader looking for
        // what a seat cost must not find the reclaim and stop.
        assert!(CONTROLLER_TYPES.contains(&SESSION_RETIRED));
        assert!(!SEAT_TYPES.contains(&SESSION_RETIRED));
        assert_ne!(
            SESSION_RETIRED, SESSION_STOPPED,
            "the reclaim and the cost are two lines with two payloads"
        );

        // The two the discriminator reads as a deliberate end, and the two it
        // does not — which is the whole of `is_deliberate_end`.
        assert!(is_deliberate_end(SEAT_RESTING));
        assert!(is_deliberate_end(SEAT_EXITED));
        assert!(!is_deliberate_end(SEAT_WOKE));
        assert!(!is_deliberate_end(SEAT_HANDED_OFF));
        assert!(!is_deliberate_end(SESSION_RESTED));
        assert!(!is_deliberate_end(SESSION_STOPPED));
        assert!(!is_deliberate_end(SESSION_RETIRED));
    }

    /// The tail's reader: the line's OWN bytes, a torn last line skipped, and a
    /// stored line carrying a sequence but no actor or type still answered —
    /// which is where it parts from `read_after`, whose fold needs both names.
    #[test]
    fn the_raw_reader_answers_the_stored_bytes_and_keeps_a_line_the_fold_would_drop() {
        let dir = std::env::temp_dir().join(format!("fleet-events-raw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");

        // Written by hand, with the keys in an order no serializer here would
        // choose: what comes back is the line, not a re-serialization of it.
        let stored = "{\"payload\":{},\"actor\":\"a-seat\",\"type\":\"seat.woke\",\"ts\":\"2026-09-08T00:00:01Z\",\"seq\":1,\"id\":\"x-1\"}";
        let nameless = "{\"seq\":2,\"ts\":\"2026-09-08T00:00:02Z\",\"id\":\"x-2\"}";
        std::fs::write(&path, format!("{stored}\n{nameless}\n{{\"seq\":3,\"ty")).unwrap();

        let lines = read_lines_after(&path, 0);
        assert_eq!(lines.len(), 2, "the torn last line is skipped: {lines:?}");
        assert_eq!(lines[0].text, stored, "the bytes are the file's own");
        assert_eq!(lines[0].seq, 1);
        assert_eq!(lines[0].ts, "2026-09-08T00:00:01Z");
        assert_eq!(lines[0].actor.as_deref(), Some("a-seat"));
        assert_eq!(lines[0].kind.as_deref(), Some("seat.woke"));

        assert_eq!(lines[1].text, nameless);
        assert_eq!(lines[1].actor, None, "a line with no actor is still a line");
        assert_eq!(lines[1].kind, None);
        // The control that makes the pair a reading: the fold drops that line
        // and the raw reader keeps it.
        assert_eq!(read_after(&path, 1).len(), 0);

        assert_eq!(read_lines_after(&path, 1).len(), 1, "the cursor holds");
        assert!(read_lines_after(&path, 2).is_empty());
        assert!(read_lines_after(&dir.join("nothing.jsonl"), 0).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The follower's cursor: an unterminated tail is HELD, and the same bytes
    /// are answered once the writer's newline lands. A reader that consumed it
    /// would lose the event for good.
    #[test]
    fn an_unterminated_tail_is_held_until_its_newline_lands() {
        let dir = std::env::temp_dir().join(format!("fleet-events-follow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let whole = "{\"seq\":1,\"ts\":\"t\",\"type\":\"seat.woke\",\"actor\":\"a-seat\"}";
        std::fs::write(&path, format!("{whole}\n")).unwrap();

        let (first, offset) = read_lines_from(&path, 0, 0);
        assert_eq!(first.len(), 1);
        assert_eq!(offset, whole.len() as u64 + 1);

        // Half a line: nothing is answered and the cursor does not move.
        let half = "{\"seq\":2,\"ts\":\"t\",\"type\":\"seat.re";
        std::fs::write(&path, format!("{whole}\n{half}")).unwrap();
        let (nothing, held) = read_lines_from(&path, offset, 0);
        assert!(nothing.is_empty(), "the half line is held: {nothing:?}");
        assert_eq!(held, offset, "and the cursor stays where it was");

        // The writer finishes it, and the whole line is answered once.
        let second = "{\"seq\":2,\"ts\":\"t\",\"type\":\"seat.resting\",\"actor\":\"a-seat\"}";
        std::fs::write(&path, format!("{whole}\n{second}\n")).unwrap();
        let (finished, moved) = read_lines_from(&path, held, 0);
        assert_eq!(finished.len(), 1, "{finished:?}");
        assert_eq!(finished[0].text, second);
        assert_eq!(moved, (whole.len() + second.len() + 2) as u64);
        assert!(read_lines_from(&path, moved, 0).0.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The by-id reader's three answers. Two matches is a corrupted stream and
    /// not a pick: both sequences come back, because the caller names them.
    #[test]
    fn the_by_id_reader_answers_none_one_or_every_line_that_carries_the_id() {
        let dir = std::env::temp_dir().join(format!("fleet-events-by-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(
            &path,
            "{\"id\":\"one\",\"seq\":1,\"ts\":\"t\",\"type\":\"seat.woke\",\"actor\":\"a\"}\n\
             {\"id\":\"two\",\"seq\":2,\"ts\":\"t\",\"type\":\"seat.woke\",\"actor\":\"a\"}\n\
             {\"id\":\"two\",\"seq\":3,\"ts\":\"t\",\"type\":\"seat.woke\",\"actor\":\"a\"}\n",
        )
        .unwrap();

        assert!(find_by_id(&path, "nothing").is_empty());
        let one = find_by_id(&path, "one");
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].seq, 1);
        assert_eq!(one[0].value["type"], "seat.woke");
        let two: Vec<u64> = find_by_id(&path, "two").iter().map(|m| m.seq).collect();
        assert_eq!(two, vec![2, 3], "both lines come back: {two:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The stamp resolver, at a line's own stamp, between two, past the end, and
    /// on a value of another shape — which is a refusal and never a cursor.
    #[test]
    fn a_stamp_resolves_to_the_first_sequence_at_or_after_it() {
        let dir = std::env::temp_dir().join(format!("fleet-events-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        // Ten seconds apart, so there is a stamp BETWEEN two lines to ask about.
        std::fs::write(
            &path,
            "{\"id\":\"a\",\"seq\":1,\"ts\":\"2026-09-08T00:00:00Z\",\"type\":\"t\",\"actor\":\"a\"}\n\
             {\"id\":\"b\",\"seq\":2,\"ts\":\"2026-09-08T00:00:10Z\",\"type\":\"t\",\"actor\":\"a\"}\n\
             {\"id\":\"c\",\"seq\":3,\"ts\":\"2026-09-08T00:00:20Z\",\"type\":\"t\",\"actor\":\"a\"}\n",
        )
        .unwrap();

        assert_eq!(seq_at_or_after(&path, "2026-09-08T00:00:10Z"), Ok(Some(2)));
        assert_eq!(
            seq_at_or_after(&path, "2026-09-08T00:00:11Z"),
            Ok(Some(3)),
            "a stamp between two lines resolves to the later one"
        );
        assert_eq!(seq_at_or_after(&path, "2026-09-07T00:00:00Z"), Ok(Some(1)));
        assert_eq!(
            seq_at_or_after(&path, "2026-09-08T00:00:21Z"),
            Ok(None),
            "a stamp past the end resolves to nothing at all"
        );
        for bad in ["", "2026-09-08", "2026-09-08T00:00:00", "not-a-stamp"] {
            assert_eq!(
                seq_at_or_after(&path, bad),
                Err(MalformedStamp),
                "{bad:?} is not a stamp"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_torn_line_does_not_renumber_the_stream() {
        let dir = std::env::temp_dir().join(format!("fleet-events-torn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(
            &path,
            "{\"seq\":7,\"type\":\"controller.started\"}\n{\"seq\":8,\"ty",
        )
        .unwrap();
        assert_eq!(EventLog::open(&path).seq(), 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
