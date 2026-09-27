//! How [`super::StubAgent`] reads the listing and the session log it is
//! scripted with: the rows a suite writes as JSON, and the log a session
//! appends its turns to. The same rules answer the in-process stub and
//! `fleet-agent-stub`, so the two seams read one listing one way.
//!
//! THE SHAPES ARE THE STUB'S OWN, kept from the in-process adapter they were
//! first written for when fleet-x93d.2 deleted it: what a real agent lists and
//! logs is its adapter's business, read by that adapter and answered in the
//! contract's own types. A suite scripts these shapes and core never reads
//! them.

use std::collections::BTreeMap;
use std::time::SystemTime;

use fleet_core::store::types::Stamp;
use serde::Deserialize;

use crate::adapter::{dir_key, Activity, BlockedOn, Evidence, SeatActivity, SeatContext, SeatRef};

/// One session as a scripted listing reports it. Every field a suite writes
/// that this does not name is read past.
#[derive(Clone, Debug, Deserialize)]
struct Row {
    #[serde(rename = "sessionId")]
    session_id: String,
    /// The session's process: what a row is attributed to a seat by where its
    /// session id does not, the pane's own pid (E2).
    #[serde(default)]
    pid: Option<u32>,
    /// What the session is DOING: `idle`, `busy` or `waiting`, and absent on a
    /// row listed before it says.
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "startedAt", default)]
    started_at: Option<u64>,
    /// Present ONLY while the session is stopped in front of a human, naming
    /// the cause. Read for PRESENCE: a cause the stub does not recognise
    /// still blocks the seat.
    #[serde(rename = "waitingFor", default)]
    waiting_for: Option<String>,
}

/// A session that is mid-turn.
const BUSY: &str = "busy";

/// A session at its prompt.
const IDLE: &str = "idle";

/// A session stopped in front of a human, read as a block on its own, so a
/// row that names no cause still blocks.
const WAITING: &str = "waiting";

/// The one `waitingFor` value that names its block.
const PERMISSION_PROMPT: &str = "permission prompt";

/// The listing's answer, or why it is not one: empty output is unreadable,
/// never a reading of zero, and an empty JSON array is a listing that
/// answered and said there is nothing.
fn parse_listing(stdout: &str) -> Result<Vec<Row>, String> {
    if stdout.trim().is_empty() {
        return Err("the listing answered with zero bytes and a success status".to_string());
    }
    serde_json::from_str::<Vec<Row>>(stdout)
        .map_err(|e| format!("the listing did not parse as JSON rows: {e}"))
}

/// Every seat's reading off the listings `listing` answers — one per distinct
/// configuration directory the seats name, `None` for the stub's own — in the
/// order the seats were asked, one each.
///
/// An unreadable listing is its own directory's: its seats read `unknown`
/// carrying why, and every other directory's are read as usual. A seat's row
/// is the one carrying the seat's `session_id`, and where none does the one
/// whose pid is the seat's pane's — never by the working directory. Two rows
/// under one key: the newest started. A seat with no row is `starting`, and
/// an idle seat whose log's first turn is the logged-out answer is blocked on
/// it.
pub fn readings_from(
    seats: &[SeatRef],
    listing: &dyn Fn(Option<&str>) -> Result<String, String>,
    transcript: &dyn Fn(&SeatRef, &str) -> Option<String>,
) -> Vec<SeatActivity> {
    let mut read: BTreeMap<Option<String>, Result<Vec<Row>, String>> = BTreeMap::new();
    for seat in seats {
        let dir = seat
            .config_dir
            .as_deref()
            .map(|dir| dir_key(dir).to_string());
        if let std::collections::btree_map::Entry::Vacant(unread) = read.entry(dir) {
            let rows = listing(unread.key().as_deref()).and_then(|stdout| parse_listing(&stdout));
            unread.insert(rows);
        }
    }
    seats
        .iter()
        .map(|seat| {
            let dir = seat
                .config_dir
                .as_deref()
                .map(|dir| dir_key(dir).to_string());
            match &read[&dir] {
                Err(cause) => SeatActivity {
                    seat: seat.seat,
                    activity: Activity::Unknown,
                    blocked_on: None,
                    evidence: Evidence::Typed,
                    session_id: None,
                    cause: Some(format!("the listing could not be read: {cause}")),
                },
                Ok(rows) => match matched(seat, rows) {
                    None => SeatActivity {
                        seat: seat.seat,
                        activity: Activity::Starting,
                        blocked_on: None,
                        evidence: Evidence::Typed,
                        session_id: None,
                        cause: Some(match seat.pid {
                            Some(pid) => format!("the listing names no row with pid {pid}"),
                            None => "the listing can name no row for it".to_string(),
                        }),
                    },
                    Some(row) => {
                        let reading = reading_of(seat, row);
                        let logged_out = reading.activity == Activity::Idle
                            && transcript(seat, &row.session_id)
                                .is_some_and(|body| logged_out_first_turn(&body));
                        if logged_out {
                            SeatActivity {
                                activity: Activity::Blocked,
                                blocked_on: Some(BlockedOn::LoggedOut),
                                ..reading
                            }
                        } else {
                            reading
                        }
                    }
                },
            }
        })
        .collect()
}

/// The seat's row among `rows`: by its session id, else by its pane's pid,
/// the newest started where two carry the same.
fn matched<'r>(seat: &SeatRef, rows: &'r [Row]) -> Option<&'r Row> {
    // The first listed, unless a later one started strictly after it: a row
    // with no start stamp is the oldest.
    let newest = |found: Vec<&'r Row>| {
        found.into_iter().reduce(|best, row| {
            if row.started_at.unwrap_or(0) > best.started_at.unwrap_or(0) {
                row
            } else {
                best
            }
        })
    };
    let id = seat.session_id.as_deref().map(str::trim).unwrap_or("");
    if !id.is_empty() {
        if let Some(row) = newest(rows.iter().filter(|row| row.session_id == id).collect()) {
            return Some(row);
        }
    }
    let pid = seat.pid?;
    newest(rows.iter().filter(|row| row.pid == Some(pid)).collect())
}

/// A found row's reading, all of it typed: `waitingFor` present is blocked
/// whatever the status beside it, its cause carried verbatim; otherwise the
/// status is the activity, a row with none is `starting`, and a word the stub
/// does not read is `unknown`, naming it.
fn reading_of(seat: &SeatRef, row: &Row) -> SeatActivity {
    let typed =
        |activity: Activity, blocked_on: Option<BlockedOn>, cause: Option<String>| SeatActivity {
            seat: seat.seat,
            activity,
            blocked_on,
            evidence: Evidence::Typed,
            session_id: Some(row.session_id.clone()),
            cause,
        };
    if let Some(cause) = &row.waiting_for {
        let on = (cause == PERMISSION_PROMPT).then_some(BlockedOn::Permission);
        return typed(Activity::Blocked, on, Some(cause.clone()));
    }
    match row.status.as_deref() {
        Some(IDLE) => typed(Activity::Idle, None, None),
        Some(BUSY) => typed(Activity::Busy, None, None),
        Some(WAITING) => typed(Activity::Blocked, None, Some(format!("status {WAITING}"))),
        None => typed(
            Activity::Starting,
            None,
            Some("the listing's row carries no status yet".to_string()),
        ),
        Some(other) => typed(
            Activity::Unknown,
            None,
            Some(format!(
                "the listing's status is `{other}`, a word this adapter does not read"
            )),
        ),
    }
}

#[derive(Deserialize)]
struct LogEntry {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default)]
    message: Option<LogMessage>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default, rename = "isApiErrorMessage")]
    is_api_error: bool,
}

#[derive(Deserialize)]
struct LogMessage {
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

impl Usage {
    fn window(&self) -> u64 {
        self.input_tokens.unwrap_or(0)
            + self.cache_read_input_tokens.unwrap_or(0)
            + self.cache_creation_input_tokens.unwrap_or(0)
    }
}

/// The main-chain assistant entries of a log, a torn or malformed line
/// skipped rather than fatal.
fn turns_of(body: &str) -> impl Iterator<Item = LogEntry> + '_ {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<LogEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
}

/// The cause the log's first turn carries when the session has no credential.
pub const AUTHENTICATION_FAILED: &str = "authentication_failed";

/// Whether the log's first main-chain turn is the logged-out answer: an entry
/// written in place of a model turn, its cause [`AUTHENTICATION_FAILED`], its
/// window zero.
pub fn logged_out_first_turn(body: &str) -> bool {
    turns_of(body)
        .map(|e| {
            let window = e
                .message
                .and_then(|m| m.usage)
                .map(|u| u.window())
                .unwrap_or(0);
            e.is_api_error && e.error.as_deref() == Some(AUTHENTICATION_FAILED) && window == 0
        })
        .next()
        .unwrap_or(false)
}

/// One seat's context off its log's body and its last write: the window the
/// last turn that stated one carried, the turns taken — counting a turn that
/// stated a zero window — and the write's stamp. No body is the seat's id
/// alone, a reading nobody has and never a zero.
pub fn context_of(seat: &SeatRef, body: Option<&str>, written: Option<SystemTime>) -> SeatContext {
    SeatContext {
        seat: seat.seat,
        tokens: body.and_then(|body| {
            turns_of(body)
                .filter_map(|e| e.message)
                .filter_map(|m| m.usage)
                .map(|u| u.window())
                .filter(|&tokens| tokens != 0)
                .last()
        }),
        window: None,
        turns: body.map(|body| {
            turns_of(body)
                .filter_map(|e| e.message)
                .filter(|m| m.usage.is_some())
                .count() as u64
        }),
        last_write: written
            .and_then(crate::clock::stamp_of)
            .and_then(|stamp| Stamp::parse(&stamp)),
    }
}
