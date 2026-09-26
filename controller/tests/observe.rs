//! Fixture tests for the observe layer.
//!
//! The `lessons::` module below is the contract named in
//! `fleet/brain/lessons/*.md` § Test inventory: each fact the code in this slice
//! exercises owes a test under the exact name the inventory carries.
//!
//! A seat is read from TWO listings (ruling 3): the host's, which says whether
//! its session is there, and the agent's, which says what the session is
//! doing. The host's is a [`FakeHost`] here — its own panes, listed by its own
//! `list` — and the agent's is the listing RECORDED on the supported release
//! below, so the arms decide real rows against a host that is not real.

use fleet_controller::adapter::claude_code::{parse_roster, ClaudeCode};
use fleet_controller::adapter::{dir_key, encode_project_dir, transcript_path, Agent, RosterRead};
use fleet_controller::config::Seat;
use fleet_controller::events;
use fleet_controller::host::{session_for, Host, HostRead, Pane, PaneState, SOCKET};
use fleet_controller::observe::{
    self, context_tokens_in, observe_seat, turns_in, RosterState, Rosters, SeatObservation,
    STARTING_GRACE_MS,
};
use fleet_controller::platform::{self, Grant, Listing, GRANT_OK, GRANT_PENDING};
use fleet_controller::projection::{
    render, EffectsView, PolicyView, Projection, SeatRow, SeatView, VERSION,
};
use fleet_controller::test_support::{FakeHost, FIRST_PANE_PID};
use fleet_core::seat::identity::SeatId;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const WORKTREE: &str = "/wt/builder-1";
const OTHER: &str = "/wt/builder-2";

/// The fixed ids this suite's seats carry.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const TRANSIENT_ID: &str = "01a0d1f1-0aec-765f-9abe-00007e3fa2c0";

fn id(text: &str) -> SeatId {
    SeatId::parse(text).expect("a hand-written seat id parses")
}

fn seat() -> Seat {
    Seat {
        id: id(SEAT_ID),
        name: None,
        model: None,
        transient: false,
        worktrees: vec![("demo".to_string(), WORKTREE.to_string())],
    }
}

/// A transient seat, whose session comes up under its own configuration
/// directory.
fn transient_seat() -> Seat {
    Seat {
        id: id(TRANSIENT_ID),
        name: None,
        model: None,
        transient: true,
        worktrees: vec![("demo".to_string(), OTHER.to_string())],
    }
}

// ------------------------------------------------------- the recorded listing

/// THE LISTING RECORDED ON THE SUPPORTED RELEASE (fleet-rge6.3's first step,
/// reviewer call E14): Claude Code 2.1.280, tmux 3.7b, 2026-09-26, a scratch
/// configuration directory seeded with onboarding and the worktree's trust, a
/// scratch socket, `DISABLE_AUTOUPDATER=1` in the pane. `agents --json --all`
/// read every 0.25 s, the session started as the pane's own process with
/// `--name ma --model haiku --permission-mode default` and no first turn.
///
/// Byte for byte as the agent printed them, less the whitespace. Idle 0.70 s
/// after `new-session`; busy on the first read after a pasted turn was
/// submitted; waiting, with the cause, 2.8 s after a turn that asked for a
/// file write met the approval dialog; idle again on the next read after the
/// dialog was cancelled; and an empty listing on the first read after
/// `kill-session`, 0.22 s later. The pane listed `pid=31697 dead=0` beside
/// every one of the three rows.
const RECORDED_IDLE: &str = r#"[{"pid":31697,"cwd":"/private/tmp/fleet-measure-rge63/wt","kind":"interactive","startedAt":1790414327458,"sessionId":"d0090b9b-6edf-4cfa-8309-d14b2f1e70b4","name":"ma","status":"idle"}]"#;
const RECORDED_BUSY: &str = r#"[{"pid":31697,"cwd":"/private/tmp/fleet-measure-rge63/wt","kind":"interactive","startedAt":1790414327458,"sessionId":"d0090b9b-6edf-4cfa-8309-d14b2f1e70b4","name":"ma","status":"busy"}]"#;
const RECORDED_WAITING: &str = r#"[{"pid":31697,"cwd":"/private/tmp/fleet-measure-rge63/wt","kind":"interactive","startedAt":1790414327458,"sessionId":"d0090b9b-6edf-4cfa-8309-d14b2f1e70b4","name":"ma","status":"waiting","waitingFor":"permission prompt"}]"#;
const RECORDED_GONE: &str = "[]";

/// The pane's own pid in the recording, which the listing's row carried.
const RECORDED_PID: u32 = 31_697;
const RECORDED_SESSION: &str = "d0090b9b-6edf-4cfa-8309-d14b2f1e70b4";
const RECORDED_CWD: &str = "/private/tmp/fleet-measure-rge63/wt";
/// `session_created` of the recorded session, in whole seconds as the host
/// counts them.
const RECORDED_CREATED_MS: u64 = 1_790_414_327_000;

/// A seat standing where the recording's session stood.
fn recorded_seat() -> Seat {
    Seat {
        worktrees: vec![("measured".to_string(), RECORDED_CWD.to_string())],
        ..seat()
    }
}

/// The host's listing as the recording read it, with the pane renamed to
/// `seat`'s session — the one thing a scratch socket could not name as fleet
/// does.
fn recorded_pane(seat: &Seat, state: PaneState) -> HostRead {
    HostRead::Readable(vec![Pane {
        session: session_for(&seat.id),
        pid: Some(RECORDED_PID),
        state,
        path: match state {
            PaneState::Alive => RECORDED_CWD.to_string(),
            // A dead pane's path reads empty (measured on 3.7b), and the
            // recording's `/exit` read it so.
            PaneState::Dead { .. } => String::new(),
        },
        created_ms: Some(RECORDED_CREATED_MS),
    }])
}

/// A recorded listing with its pid moved onto `pid` — how a recorded row is
/// laid beside a [`FakeHost`] pane, which numbers its own.
fn recorded_as(body: &str, pid: u32) -> RosterRead {
    parse_roster(&body.replace(&RECORDED_PID.to_string(), &pid.to_string()))
}

// ------------------------------------------------------------- the fake host

/// A host holding one live session for `seat`, started as a start would, and
/// the pid its pane was given.
fn hosting(seat: &Seat) -> (FakeHost, u32) {
    let host = FakeHost::new();
    host.new_session(
        &session_for(&seat.id),
        Path::new(WORKTREE),
        &["/nowhere/agent".to_string()],
        &[],
    )
    .expect("the fake host starts the session");
    (host, FIRST_PANE_PID)
}

/// When the one session `host` holds was created, which the start's grace is
/// measured from.
fn created(host: &FakeHost) -> u64 {
    match host.list() {
        HostRead::Readable(panes) => panes[0].created_ms.expect("a fake pane is dated"),
        HostRead::Unreadable { cause } => panic!("{cause}"),
    }
}

/// A poll well past the start's grace for everything `host` holds.
fn settled(host: &FakeHost) -> u64 {
    created(host) + STARTING_GRACE_MS + 1_000
}

/// A host with nothing on it: a server that is not running reads so, which is
/// the fleet after a reboot.
fn nothing_hosted() -> HostRead {
    HostRead::Readable(Vec::new())
}

/// A listing built from row bodies, in the shape the agent emits.
fn roster(rows: &[String]) -> RosterRead {
    parse_roster(&format!("[{}]", rows.join(",")))
}

/// An interactive row, as the recording's rows read: a pid and a status, and
/// no address and no state (lessons claude-code B10).
fn live(cwd: &str, session: &str, pid: u32) -> String {
    format!(
        r#"{{"sessionId":"{session}","cwd":"{cwd}","kind":"interactive","name":"orla",
            "pid":{pid},"status":"idle","startedAt":1000}}"#
    )
}

fn waiting(cwd: &str, session: &str, pid: u32, cause: &str) -> String {
    format!(
        r#"{{"sessionId":"{session}","cwd":"{cwd}","kind":"interactive","name":"orla",
            "pid":{pid},"status":"waiting","startedAt":1000,"waitingFor":"{cause}"}}"#
    )
}

/// The one seat decided against a live session this suite's fake host holds
/// and a listing naming it by the pane's pid.
fn present(seat: &Seat, cwd: &str, session: &str) -> SeatObservation {
    let (host, pid) = hosting(seat);
    observe_seat(
        &roster(&[live(cwd, session, pid)]),
        &host.list(),
        seat,
        settled(&host),
    )
}

mod lessons {
    use super::*;

    /// claude-code A1 — the pin is a measurement, not a version number: the
    /// release each behaviour was measured against is published BESIDE the
    /// version the binary reports this poll, and a spread between them is a
    /// flag, never a refusal.
    #[test]
    fn version_pin_is_published_beside_the_live_version() {
        let drifted = projection(Some("2.1.262"), Some("2.1.261"));
        let body: serde_json::Value = serde_json::from_str(&render(&drifted).unwrap()).unwrap();
        assert_eq!(body["agent_version"], "2.1.262");
        assert_eq!(body["agent_version_expected"], "2.1.261");
        assert_eq!(
            body["seats"].as_array().map(Vec::len),
            Some(0),
            "a spread publishes the document, it does not refuse it"
        );

        // A binary that did not answer publishes a named absence, not the pin.
        let silent = projection(None, Some("2.1.261"));
        let body: serde_json::Value = serde_json::from_str(&render(&silent).unwrap()).unwrap();
        assert!(body["agent_version"].is_null());
        assert_eq!(body["agent_version_expected"], "2.1.261");
    }

    /// claude-code B1 — the roster is one command, and the reader tolerates
    /// fields it does not know. Field presence is kind-dependent, so a reader
    /// that requires a field on every row fails on the first mixed listing:
    /// here a background row beside the recorded interactive one, which
    /// carries neither the address nor the state the other does.
    #[test]
    fn the_roster_is_one_command() {
        let recorded = RECORDED_IDLE.trim_start_matches('[').trim_end_matches(']');
        let mixed = parse_roster(&format!(
            r#"[
              {{"id":"aa","sessionId":"aa","cwd":"/wt/builder-1","kind":"background",
               "pid":1,"status":"idle","state":"running","name":"orla","startedAt":10,
               "someFieldNobodyHasSeen":"harmless"}},
              {recorded}
            ]"#
        ));
        match mixed {
            RosterRead::Readable(rows) => {
                assert_eq!(rows.len(), 2, "both kinds survive one read");
                assert_eq!(rows[0].state.as_deref(), Some("running"));
                assert_eq!(
                    rows[1].state, None,
                    "an interactive row carries no state word"
                );
                assert_eq!(rows[1].pid, Some(RECORDED_PID));
            }
            RosterRead::Unreadable { cause } => panic!("the listing must parse: {cause}"),
        }
    }

    /// claude-code B2 — there is no token figure anywhere in the listing, so
    /// context accounting cannot come from it. A number that looks like one on a
    /// row is not the seat's context: the transcript is.
    #[test]
    fn the_roster_carries_no_token_field() {
        let (host, pid) = hosting(&seat());
        let read = parse_roster(&format!(
            r#"[{{"sessionId":"aa","cwd":"/wt/builder-1","kind":"interactive",
                 "pid":{pid},"startedAt":10,"tokens":999999,"input_tokens":999999}}]"#
        ));
        let seen = observe_seat(&read, &host.list(), &seat(), settled(&host));
        assert_eq!(seen.state, RosterState::Present);

        let from_transcript =
            context_tokens_in(r#"{"type":"assistant","message":{"usage":{"input_tokens":7}}}"#);
        assert_eq!(
            from_transcript,
            Some(7),
            "the reading is the transcript's, and 999999 has no path into it"
        );
    }

    /// claude-code B4 — the listing has been observed answering with zero bytes
    /// and a success status while sessions were live. Empty is UNREADABLE, never
    /// a reading of zero; the control is an empty JSON array, which is a listing
    /// that answered and said there is nothing.
    #[test]
    fn the_roster_read_can_go_silently_dead() {
        let (host, _) = hosting(&seat());
        let silent = observe_seat(&parse_roster(""), &host.list(), &seat(), settled(&host));
        assert_eq!(silent.state, RosterState::Unknown);
        assert!(
            silent.unknown_cause.is_some(),
            "the cause travels with the Unknown"
        );

        let answered = observe_seat(&parse_roster("[]"), &nothing_hosted(), &seat(), 2_000);
        assert_eq!(
            answered.state,
            RosterState::Absent,
            "a listing that answered and said nothing is not the same read"
        );
    }

    /// claude-code B5 — `cwd` names a seat and proves nothing. A row is the
    /// seat's by the PANE's pid, whatever directory it stands in, and a live row
    /// standing in the seat's worktree with no session on the host behind it
    /// is a session fleet does not host: not the seat's, and not a seat to
    /// start a second session beside either.
    #[test]
    fn cwd_names_a_seat_and_proves_nothing() {
        let mine = present(&seat(), WORKTREE, "aa");
        assert_eq!(mine.state, RosterState::Present);
        assert_eq!(mine.session_id.as_deref(), Some("aa"));
        assert_eq!(mine.project.as_deref(), Some("demo"));

        // Two live rows in the worktree: the pane's pid picks one, and the
        // other is somebody else's session standing in the same directory.
        let (host, pid) = hosting(&seat());
        let contested = observe_seat(
            &roster(&[live(WORKTREE, "not-mine", 4242), live(WORKTREE, "aa", pid)]),
            &host.list(),
            &seat(),
            settled(&host),
        );
        assert_eq!(contested.state, RosterState::Present);
        assert_eq!(
            contested.session_id.as_deref(),
            Some("aa"),
            "the pid attributes the row, and the directory does not"
        );

        // No session on the host, and a live row in the worktree: Unknown,
        // naming what the listing holds, and never the seat's session.
        let unhosted = observe_seat(
            &roster(&[live(WORKTREE, "somebody-elses", 4242)]),
            &nothing_hosted(),
            &seat(),
            2_000,
        );
        assert_eq!(unhosted.state, RosterState::Unknown);
        assert_eq!(
            unhosted.session_id, None,
            "an unhosted session is not the seat's"
        );

        // The control: the same row in another seat's worktree is nothing to
        // this one.
        let elsewhere = observe_seat(
            &roster(&[live(OTHER, "bb", 4242)]),
            &nothing_hosted(),
            &seat(),
            2_000,
        );
        assert_eq!(elsewhere.state, RosterState::Absent);
    }

    /// claude-code C1 — the transcript path is an encoding, and every context
    /// instrument resolves the same way, so a change to it blinds them all at
    /// once. The rule is EVERY non-alphanumeric character, not the separator
    /// alone: a worktree carrying a dot, an underscore or a space is the case a
    /// separator-only reader publishes a null context for forever.
    #[test]
    fn the_transcript_path_encoding() {
        let path = transcript_path(Path::new("/home/av/.claude"), "/wt/builder-1", "aa-bb");
        assert_eq!(
            path,
            Path::new("/home/av/.claude/projects/-wt-builder-1/aa-bb.jsonl")
        );

        assert_eq!(
            encode_project_dir("/Users/av/.claude/jobs/tmp"),
            "-Users-av--claude-jobs-tmp",
            "a dot is a dash, and a dot after a separator is two"
        );
        assert_eq!(
            encode_project_dir("/wt/my_seat/a b.c"),
            "-wt-my-seat-a-b-c",
            "an underscore, a space and a dot are all dashes"
        );
        assert_eq!(encode_project_dir("plain123"), "plain123");
        // Measured on 2.1.280: the scoped session's transcript landed at
        // `<config dir>/projects/-private-tmp-fleet-measure-rge63-wt/<id>.jsonl`.
        assert_eq!(
            encode_project_dir(RECORDED_CWD),
            "-private-tmp-fleet-measure-rge63-wt"
        );
    }

    /// claude-code B8 — one field says a session is stopped in front of a human,
    /// and it is keyed on PRESENCE. The vocabulary is the agent's, so a cause
    /// this fleet has never seen must still stop the seat rather than read as a
    /// healthy one; the control below is the same row without the field.
    ///
    /// On the INTERACTIVE row the recording read (B10): `waiting` and
    /// `permission prompt` at the approval dialog.
    #[test]
    fn waiting_for_names_the_block() {
        let at_the_dialog = recorded_seat();
        let blocked = observe_seat(
            &parse_roster(RECORDED_WAITING),
            &recorded_pane(&at_the_dialog, PaneState::Alive),
            &at_the_dialog,
            RECORDED_CREATED_MS + 30_000,
        );
        assert_eq!(blocked.state, RosterState::PromptBlocked);
        assert_eq!(blocked.waiting_for.as_deref(), Some("permission prompt"));
        assert_eq!(blocked.activity.as_deref(), Some("waiting"));
        assert_eq!(blocked.session_id.as_deref(), Some(RECORDED_SESSION));

        let (host, pid) = hosting(&seat());
        let unrecognised = observe_seat(
            &roster(&[waiting(
                WORKTREE,
                "aa",
                pid,
                "a cause nobody has enumerated",
            )]),
            &host.list(),
            &seat(),
            settled(&host),
        );
        assert_eq!(
            unrecognised.state,
            RosterState::PromptBlocked,
            "presence, never the value: an unknown cause still stops the seat"
        );

        let control = observe_seat(
            &parse_roster(RECORDED_BUSY),
            &recorded_pane(&at_the_dialog, PaneState::Alive),
            &at_the_dialog,
            RECORDED_CREATED_MS + 30_000,
        );
        assert_eq!(control.state, RosterState::Present);
        assert_eq!(control.waiting_for, None);

        // The reference publishes no context reading for this state, and the
        // acceptance is row-for-row parity with it.
        assert!(!RosterState::PromptBlocked.has_context_reading());
        assert!(RosterState::Present.has_context_reading());
        assert!(RosterState::Stopped.has_context_reading());
        assert!(!RosterState::Starting.has_context_reading());
    }

    /// claude-code B10 — an interactive session is listed WITHOUT AN ADDRESS,
    /// its pid is the pane's, its activity is a three-word status, its blocked
    /// cause is typed, and its end leaves no row behind.
    ///
    /// Re-measured on the supported 2.1.280 (reviewer call E14; B10 was first
    /// read on 2.1.282), and the recording is the fixture: every row the agent
    /// printed, decided against the pane the host listed beside it.
    #[test]
    fn an_interactive_row_is_listed_without_an_address() {
        let recorded = recorded_seat();
        let alive = recorded_pane(&recorded, PaneState::Alive);
        let at = RECORDED_CREATED_MS + 30_000;

        for (body, word) in [
            (RECORDED_IDLE, "idle"),
            (RECORDED_BUSY, "busy"),
            (RECORDED_WAITING, "waiting"),
        ] {
            let rows = match parse_roster(body) {
                RosterRead::Readable(rows) => rows,
                RosterRead::Unreadable { cause } => panic!("{cause}: {body}"),
            };
            assert_eq!(rows.len(), 1);
            let row = &rows[0];
            assert!(
                !body.contains("\"id\""),
                "no address on an interactive row: {body}"
            );
            assert_eq!(row.state, None, "and none of A3's state words: {body}");
            assert_eq!(
                row.pid,
                Some(RECORDED_PID),
                "the row's pid IS the pane's: {body}"
            );
            assert_eq!(row.status.as_deref(), Some(word));
            assert_eq!(
                row.waiting_for.is_some(),
                word == "waiting",
                "the cause is present exactly while the session waits: {body}"
            );

            // And decided by the pid: the seat is live, its activity is the
            // status.
            let seen = observe_seat(&parse_roster(body), &alive, &recorded, at);
            assert!(
                matches!(
                    seen.state,
                    RosterState::Present | RosterState::PromptBlocked
                ),
                "{word}: {seen:?}"
            );
            assert_eq!(seen.activity.as_deref(), Some(word));
            assert_eq!(seen.pane_pid, Some(RECORDED_PID));
            assert_eq!(seen.project.as_deref(), Some("measured"));
        }

        // `kill-session`: the next read lists nothing, and the host holds no
        // session. The seat is absent, and no pid-less row stands in for an
        // end — there is no stopped-row window left to measure.
        let killed = observe_seat(
            &parse_roster(RECORDED_GONE),
            &nothing_hosted(),
            &recorded,
            at,
        );
        assert_eq!(killed.state, RosterState::Absent);
        assert_eq!(killed.session_id, None);

        // `/exit`: the row went as fast (0.22 s), and the pane stayed, dead
        // with status 0 and its pid, under remain-on-exit. The end is the
        // host's reading and the listing has nothing to add to it.
        let exited = observe_seat(
            &parse_roster(RECORDED_GONE),
            &recorded_pane(&recorded, PaneState::Dead { status: Some(0) }),
            &recorded,
            at,
        );
        assert_eq!(exited.state, RosterState::Stopped);
        assert_eq!(exited.exit_status, Some(0));
        assert_eq!(exited.pane_pid, Some(RECORDED_PID));
    }

    /// claude-code C2 — the entry shape: there is no single context number, and
    /// the reading is the arithmetic over the input tokens and both cache
    /// figures on the LAST main-chain assistant entry.
    #[test]
    fn the_transcript_entry_shape() {
        let body = r#"
{"type":"user","message":{"usage":{"input_tokens":900}}}
{"type":"assistant","message":{"usage":{"input_tokens":1,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}}
{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30}}}
{"type":"assistant","message":{"usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"usage":{"inp"#;
        assert_eq!(
            context_tokens_in(body),
            Some(60),
            "the last entry stating a window, summed over all three figures"
        );
        assert_eq!(
            context_tokens_in(r#"{"type":"assistant","message":{}}"#),
            None,
            "an entry with no usage block states no window"
        );
        assert_eq!(context_tokens_in(""), None);
    }

    /// The turn reader beside the window reader: the SAME filter, one step
    /// shorter.
    ///
    /// The two part company on exactly one entry shape, and the fixture is
    /// built so they must answer differently: four main-chain assistant entries
    /// carry a usage block and one of them sums to zero, so the window reader
    /// answers the last non-zero and the turn reader answers 4. A reader that
    /// had copied the window's own filter would answer 3 here.
    #[test]
    fn the_turn_reader_counts_the_entry_the_window_reader_skips() {
        let body = r#"
{"type":"user","message":{"usage":{"input_tokens":900}}}
{"type":"assistant","message":{"usage":{"input_tokens":1}}}
{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":2600000}}}
{"type":"assistant","message":{"usage":{"input_tokens":0,"cache_read_input_tokens":0}}}
{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20}}}
{"type":"assistant","message":{"usage":{"input_tokens":40,"cache_read_input_tokens":20}}}
{"type":"assistant","message":{"usage":{"inp"#;
        assert_eq!(turns_in(body), 4, "the zero-usage entry is a turn");
        assert_eq!(
            context_tokens_in(body),
            Some(60),
            "and the window reader still skips it, which is what makes the count above a second \
             reading rather than a copy"
        );

        // An entry with NO usage block is not a turn either reader counts: the
        // agent made no call, so there is nothing to count.
        assert_eq!(
            turns_in(r#"{"type":"assistant","message":{}}"#),
            0,
            "no usage block is no turn"
        );
        assert_eq!(
            turns_in(""),
            0,
            "and an empty transcript is a measured zero"
        );

        // The sidechain control, as C3's own arm has it: with the flag
        // cleared, the entry IS counted — so the skip is a filter and not an
        // inference.
        let control = body.replace("\"isSidechain\":true", "\"isSidechain\":false");
        assert_eq!(
            turns_in(&control),
            5,
            "the control must count the entry the skip drops, or the skip proves nothing"
        );
    }

    /// claude-code C3 — a sidechain entry is a subagent's turn carrying the
    /// subagent's window. The flag is on every entry, so the skip is a filter
    /// and not an inference; the control below is the same file with the flag
    /// cleared, where the entry IS the reading.
    #[test]
    fn sidechains_carry_another_window() {
        let with_subagent = r#"
{"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":11}}}
{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":2600000}}}"#;
        assert_eq!(context_tokens_in(with_subagent), Some(11));

        let control = with_subagent.replace("\"isSidechain\":true", "\"isSidechain\":false");
        assert_eq!(
            context_tokens_in(&control),
            Some(2_600_000),
            "the control must read the entry the skip drops, or the skip proves nothing"
        );

        let absent_flag = r#"{"type":"assistant","message":{"usage":{"input_tokens":42}}}"#;
        assert_eq!(
            context_tokens_in(absent_flag),
            Some(42),
            "an absent flag reads as main chain"
        );
    }
    /// claude-code D4 — the host's file-access dialog does not refuse, it BLOCKS
    /// until somebody answers, and whether a guarded read under it returns an
    /// error or simply hangs was never measured. So the gate is built so the
    /// answer does not matter: a listing that outlasts the bound is PENDING
    /// exactly as a refusal is, the detail says which one was seen, and while
    /// any seat is pending no effect is issued.
    ///
    /// The reader is handed in, because a real dialog needs the service loaded
    /// in a desktop session and a reset aimed at the identifier of the service
    /// currently running this fleet.
    #[test]
    fn a_blocked_grant_read_is_pending() {
        let blocked = Arc::new(AtomicBool::new(true));
        let held = Arc::clone(&blocked);
        let listing: Listing = Arc::new(move |_: &Path| {
            while held.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        });
        let mut gate = Grant::new(listing, Duration::from_millis(50));
        let read = gate.poll(&[PathBuf::from(WORKTREE)]);

        // A platform with no dialog in front of a read answers ok without
        // probing, and one with a dialog answers pending — each asserted, so
        // neither box ships a green that measured nothing.
        if platform::grant_is_gated() {
            assert_eq!(read.state, GRANT_PENDING, "{read:?}");
            let detail = read.detail.clone().expect("a pending grant names why");
            assert!(
                detail.contains(WORKTREE),
                "the detail names the path: {detail}"
            );
            assert!(
                detail.contains("outstanding"),
                "and which of the two it was: {detail}"
            );

            // While it is pending, effects are OFF with the grant as the cause —
            // through the shipped rule the loop reads, not a second copy of it.
            let held = fleet_controller::projection::effects_of(read.detail.as_deref(), None);
            assert!(
                !held.acting,
                "no effect is issued while the grant is pending"
            );
            assert_eq!(held.view.state, "off");
            assert_eq!(held.view.cause.as_deref(), read.detail.as_deref());

            // The dialog is answered. The parked call returns, the next poll
            // reads it, and nothing was restarted.
            blocked.store(false, Ordering::SeqCst);
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut answered = gate.poll(&[PathBuf::from(WORKTREE)]);
            while !answered.is_ok() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
                answered = gate.poll(&[PathBuf::from(WORKTREE)]);
            }
            assert_eq!(answered.state, GRANT_OK, "{answered:?}");
            assert_eq!(answered.detail, None);
            let on = fleet_controller::projection::effects_of(answered.detail.as_deref(), None);
            assert!(on.acting, "an answered grant issues effects again");
            assert_eq!(on.view.state, "on");
        } else {
            blocked.store(false, Ordering::SeqCst);
            assert_eq!(
                read.state, GRANT_OK,
                "this platform puts no dialog in front of a directory read"
            );
            assert_eq!(read.detail, None);
            let on = fleet_controller::projection::effects_of(read.detail.as_deref(), None);
            assert!(on.acting);
            assert_eq!(on.view.state, "on");
        }

        // On every platform: a refusal is the SAME state as a timeout, which is
        // the whole of the rule the measurement could not settle.
        let denied: Listing = Arc::new(|_: &Path| Err("operation not permitted".to_string()));
        let mut refused = Grant::new(denied, Duration::from_secs(5));
        let read = refused.poll(&[PathBuf::from(WORKTREE)]);
        if platform::grant_is_gated() {
            assert_eq!(
                read.state, GRANT_PENDING,
                "a refusal is pending, not denied"
            );
            assert!(
                read.detail
                    .unwrap_or_default()
                    .contains("operation not permitted"),
                "and the detail says which one it was"
            );
        } else {
            assert_eq!(read.state, GRANT_OK);
        }
    }

    /// claude-code A11 — the configuration directory scopes the provider's
    /// listing: a session started under a per-row directory is listed under
    /// that directory and under no other, so every read about that session has
    /// to be made under the same directory. It held for interactive rows too
    /// (B10, re-read on 2.1.280: the scratch directory's listing named the
    /// session and its transcript landed under it).
    ///
    /// Measured live on this box on 2026-09-12 on 2.1.261, before the code was
    /// written: the fleet's `agents --json --all` answered 9 rows and named none
    /// of the session under the scratch directory, whose own listing answered
    /// exactly 1 and named only it.
    ///
    /// The fixture is the FOLD, because that is where the consequence lands: the
    /// directories asked for are recorded, and the row is decided against the
    /// one that could see it.
    #[test]
    fn the_config_dir_scopes_the_daemon() {
        let named = seat();
        let spawned = transient_seat();
        let seats = vec![named.clone(), spawned.clone()];
        let per_row_dir = "/machine/config/builder-9";
        let (host, pid) = hosting(&spawned);

        // Which directories the poll asked for, in call order.
        let asked = std::cell::RefCell::new(Vec::new());
        let reads = |dir: Option<&Path>| {
            asked
                .borrow_mut()
                .push(dir.map(|d| d.display().to_string()));
            match dir {
                None => roster(&[]),
                Some(_) => roster(&[live(OTHER, "spawned-session", pid)]),
            }
        };
        let rosters = Rosters::gather(
            &seats,
            &|seat: &SeatId| (*seat == spawned.id).then(|| per_row_dir.to_string()),
            &reads,
        );

        // ONE READ PER DISTINCT DIRECTORY, the fleet's among them: a poll that
        // read once could not see the spawned row at all, and one that read per
        // seat would ask the fleet's twice.
        assert_eq!(
            asked.into_inner(),
            vec![None, Some(per_row_dir.to_string())],
            "the fleet's directory and the row's own, once each"
        );

        // And the row is decided against the listing that could see it, while
        // the fleet's — the same moment, the same pane — names no row for it.
        let at = settled(&host);
        assert_eq!(
            observe_seat(rosters.for_seat(&spawned.id), &host.list(), &spawned, at).state,
            RosterState::Present
        );
        assert_eq!(
            observe_seat(rosters.fleet(), &host.list(), &spawned, at).state,
            RosterState::Unknown
        );
    }
}

// ------------------------------------------------ presence against activity
//
// One arm per case of `observe_seat`, in its own order (fleet-rge6.3): what
// the host holds for the seat, and what the listing names beside it.

/// The host's own listing could not be read: Unknown, with the host's cause,
/// whatever the agent's listing says — a seat whose presence nobody read is
/// never decided on its activity alone.
#[test]
fn an_unreadable_host_is_unknown_with_the_hosts_cause() {
    let unreadable = HostRead::Unreadable {
        cause: "the server said no".to_string(),
    };
    let seen = observe_seat(
        &roster(&[live(WORKTREE, "aa", 4242)]),
        &unreadable,
        &seat(),
        2_000,
    );
    assert_eq!(seen.state, RosterState::Unknown);
    assert_eq!(seen.unknown_cause.as_deref(), Some("the server said no"));
    assert_eq!(seen.session_id, None);
}

/// No session on the host and no live row in the seat's worktrees: Absent,
/// which is the one state a spawn is issued from.
#[test]
fn no_session_and_no_live_row_is_absent() {
    let seen = observe_seat(
        &parse_roster(RECORDED_GONE),
        &nothing_hosted(),
        &seat(),
        2_000,
    );
    assert_eq!(seen.state, RosterState::Absent);
    assert_eq!(seen.unknown_cause, None);
    assert_eq!(seen.pane_pid, None);

    // Another seat's session on the host is not this one's.
    let (host, _) = hosting(&transient_seat());
    let beside = observe_seat(&parse_roster("[]"), &host.list(), &seat(), settled(&host));
    assert_eq!(beside.state, RosterState::Absent);
}

/// No session on the host, and the listing names a LIVE session standing in
/// one of the seat's worktrees: Unknown, naming the session, its pid and where
/// it stands. A session fleet does not host is not the seat's (B5) — and a
/// seat read absent here would be started beside it.
#[test]
fn no_session_beside_a_live_row_in_the_worktree_is_unknown_naming_the_row() {
    let unhosted = recorded_seat();
    let seen = observe_seat(
        &parse_roster(RECORDED_IDLE),
        &nothing_hosted(),
        &unhosted,
        2_000,
    );
    assert_eq!(seen.state, RosterState::Unknown);
    assert_eq!(
        seen.unknown_cause.as_deref(),
        Some(
            format!(
                "no tmux session {} on {SOCKET}; the listing names session {RECORDED_SESSION}, \
                 pid {RECORDED_PID}, in {RECORDED_CWD}",
                session_for(&unhosted.id)
            )
            .as_str()
        )
    );
    assert_eq!(seen.session_id, None, "and it is not handed the session");
}

/// The seat's pane is DEAD: Stopped, with the status it exited with and the
/// pid it had. The listing is not asked — the row left with its process — and
/// an unreadable one changes nothing.
#[test]
fn a_dead_pane_is_stopped_with_its_exit_status() {
    let (host, pid) = hosting(&seat());
    host.end(&session_for(&seat().id), Some(3));
    let seen = observe_seat(&parse_roster("[]"), &host.list(), &seat(), settled(&host));
    assert_eq!(seen.state, RosterState::Stopped);
    assert_eq!(seen.exit_status, Some(3));
    assert_eq!(seen.pane_pid, Some(pid));
    assert_eq!(seen.worktree.as_deref(), Some(WORKTREE));

    let blind = observe_seat(&parse_roster(""), &host.list(), &seat(), settled(&host));
    assert_eq!(blind.state, RosterState::Stopped);

    // A signal leaves no status, and the end is still an end.
    let (host, _) = hosting(&seat());
    host.end(&session_for(&seat().id), None);
    let signalled = observe_seat(&parse_roster("[]"), &host.list(), &seat(), settled(&host));
    assert_eq!(signalled.state, RosterState::Stopped);
    assert_eq!(signalled.exit_status, None);
}

/// A LIVE pane beside a listing that could not be read: Unknown, naming both
/// — never Present on a reading nobody took (reviewer call 2026-09-25 (1)).
#[test]
fn a_live_pane_beside_an_unreadable_listing_is_unknown_naming_both() {
    let (host, pid) = hosting(&seat());
    let unreadable = RosterRead::Unreadable {
        cause: "the listing timed out".to_string(),
    };
    let seen = observe_seat(&unreadable, &host.list(), &seat(), settled(&host));
    assert_eq!(seen.state, RosterState::Unknown);
    let cause = seen
        .unknown_cause
        .expect("the cause travels with the Unknown");
    assert!(cause.contains(&format!("pid {pid}")), "{cause}");
    assert!(cause.contains("the listing timed out"), "{cause}");
    assert_eq!(seen.pane_pid, Some(pid));
}

/// A live pane and a row whose pid is the pane's: Present, or PromptBlocked
/// where the row carries a blocked cause, with the row's status carried as the
/// seat's activity and its session as the seat's.
#[test]
fn a_live_pane_and_its_row_is_present_with_the_status_as_activity() {
    let (host, pid) = hosting(&seat());
    let idle = observe_seat(
        &recorded_as(RECORDED_IDLE, pid),
        &host.list(),
        &seat(),
        settled(&host),
    );
    assert_eq!(idle.state, RosterState::Present);
    assert_eq!(idle.activity.as_deref(), Some("idle"));
    assert_eq!(idle.session_id.as_deref(), Some(RECORDED_SESSION));
    assert_eq!(idle.pane_pid, Some(pid));
    assert_eq!(idle.exit_status, None);

    let busy = observe_seat(
        &recorded_as(RECORDED_BUSY, pid),
        &host.list(),
        &seat(),
        settled(&host),
    );
    assert_eq!(busy.state, RosterState::Present);
    assert_eq!(busy.activity.as_deref(), Some("busy"));

    let blocked = observe_seat(
        &recorded_as(RECORDED_WAITING, pid),
        &host.list(),
        &seat(),
        settled(&host),
    );
    assert_eq!(blocked.state, RosterState::PromptBlocked);
    assert_eq!(blocked.waiting_for.as_deref(), Some("permission prompt"));

    // A row that says it waits and names no cause is blocked all the same: the
    // one definition a typed turn refuses on (`AgentRow::blocked_on`), so the
    // projection never calls present a seat a nudge would refuse.
    let causeless = observe_seat(
        &recorded_as(
            &RECORDED_WAITING.replace(r#","waitingFor":"permission prompt""#, ""),
            pid,
        ),
        &host.list(),
        &seat(),
        settled(&host),
    );
    assert_eq!(causeless.state, RosterState::PromptBlocked);
    assert_eq!(causeless.waiting_for.as_deref(), Some("status waiting"));
}

/// A live pane the listing names no row for YET: Starting, while the session
/// is younger than the grace — an interactive row was listed 0.5–0.75 s after
/// its session was made (2.1.280), and a poll can land inside that.
#[test]
fn a_young_live_pane_with_no_row_is_starting() {
    let (host, pid) = hosting(&seat());
    let seen = observe_seat(
        &parse_roster("[]"),
        &host.list(),
        &seat(),
        created(&host) + 500,
    );
    assert_eq!(seen.state, RosterState::Starting);
    assert_eq!(seen.pane_pid, Some(pid));
    assert_eq!(seen.unknown_cause, None);
}

/// The same pane past the grace, still unnamed: Unknown, saying the host holds
/// the pid alive and the listing names no row with it. The row with the
/// recording's own pid is somebody else's, and it changes nothing.
#[test]
fn an_older_live_pane_with_no_row_is_unknown_naming_the_pid() {
    let (host, pid) = hosting(&seat());
    let at_the_edge = created(&host) + STARTING_GRACE_MS - 1;
    assert_eq!(
        observe_seat(&parse_roster("[]"), &host.list(), &seat(), at_the_edge).state,
        RosterState::Starting,
        "one millisecond inside the grace is still starting"
    );

    let seen = observe_seat(
        &parse_roster(RECORDED_IDLE),
        &host.list(),
        &seat(),
        created(&host) + STARTING_GRACE_MS,
    );
    assert_eq!(seen.state, RosterState::Unknown);
    assert_eq!(
        seen.unknown_cause.as_deref(),
        Some(
            format!(
                "tmux holds pid {pid} alive for {}; the listing names no row with that pid",
                seat().machine_name()
            )
            .as_str()
        )
    );
}

/// The grant as the projection publishes it: the state, the detail while it is
/// pending, and the effects view the two of them produce.
#[test]
fn the_projection_publishes_the_grant_and_holds_effects_while_it_is_pending() {
    let mut pending = projection(Some("2.1.261"), None);
    let detail = "the listing of /wt/builder-1 has not answered within 10s".to_string();
    let held = fleet_controller::projection::effects_of(Some(&detail), None);
    assert!(!held.acting);
    pending.grant = GRANT_PENDING.to_string();
    pending.grant_detail = Some(detail.clone());
    pending.effects = held.view;

    let body: serde_json::Value = serde_json::from_str(&render(&pending).unwrap()).unwrap();
    assert_eq!(body["grant"], GRANT_PENDING);
    assert_eq!(body["grant_detail"], detail);
    assert_eq!(body["effects"]["state"], "off");
    assert_eq!(
        body["effects"]["cause"], detail,
        "the cause names the grant and not the binary"
    );

    // The control: an ok grant publishes the state, no detail key at all, and
    // effects on — so the three assertions above are the pending reading's and
    // not a document that always says the same thing.
    let mut fine = projection(Some("2.1.261"), None);
    let on = fleet_controller::projection::effects_of(None, None);
    assert!(on.acting);
    fine.effects = on.view;
    let body: serde_json::Value = serde_json::from_str(&render(&fine).unwrap()).unwrap();
    assert_eq!(body["grant"], GRANT_OK);
    assert!(
        body.get("grant_detail").is_none(),
        "an ok grant carries no detail key: {body}"
    );
    assert_eq!(body["effects"]["state"], "on");

    // And a grant that is pending OUTRANKS an unresolvable binary in the cause:
    // one is a question a person can answer and the other is not.
    let both = fleet_controller::projection::effects_of(Some(&detail), Some("no agent binary"));
    assert_eq!(both.view.cause.as_deref(), Some(detail.as_str()));
    assert!(!both.acting);
    let binary_alone = fleet_controller::projection::effects_of(None, Some("no agent binary"));
    assert_eq!(binary_alone.view.cause.as_deref(), Some("no agent binary"));
    assert!(!binary_alone.acting);
    let _ = EffectsView::on();
}

/// A pid in a state file reads as a live collector to the next person who opens
/// it, so the published document carries no pid, no handle and no session id —
/// on any row, in any state.
#[test]
fn the_projection_carries_no_pid_and_no_handle() {
    let (host, pid) = hosting(&seat());
    let read = parse_roster(&format!(
        r#"[{{"id":"short-id","sessionId":"a-session-id","cwd":"/wt/builder-1",
             "kind":"background","pid":{pid},"startedAt":10}}]"#
    ));
    let seen = observe_seat(&read, &host.list(), &seat(), settled(&host));
    assert_eq!(seen.state, RosterState::Present);
    assert_eq!(seen.pane_pid, Some(pid));

    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    let orla = Seat {
        name: Some("Orla".to_string()),
        ..seat()
    };
    document.seats = vec![SeatRow::from_observation(&orla, &seen, Some(1234))];
    let body = render(&document).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["seats"][0]["context_tokens"], 1234);
    for forbidden in [
        "\"pid\"",
        "\"handle\"",
        "\"session_id\"",
        &pid.to_string(),
        "short-id",
    ] {
        assert!(
            !body.contains(forbidden),
            "the projection must not carry {forbidden}:\n{body}"
        );
    }
}

/// `model` and `transient` act in the porter's own tools, which read the same
/// `config.json`; the controller parses them onto the seat and publishes
/// neither. A reader that found either here would be reading the porter's
/// intent as the fleet's state, and a row that says neither is a permanent seat
/// on the fleet's default model — which is a reading rather than a silence.
///
/// The forbidden-key shape above, over a PRESENT row: `from_observation` reads
/// the `Seat` for its id and its name and nothing else, so the two fields are
/// set to show that the projection is not handed them rather than to feed them
/// in, and the state is what has to be asserted — an Unknown row carries
/// neither key either, and an arm that read one would pass while a leak scoped
/// to a seat that was found went by.
#[test]
fn the_projection_carries_no_model_and_no_transient() {
    let configured = Seat {
        model: Some("a-model".to_string()),
        transient: true,
        ..seat()
    };
    let seen = present(&configured, WORKTREE, "a-session");
    assert_eq!(
        seen.state,
        RosterState::Present,
        "the row below is a seat that was FOUND, which is the shape a leak would ride"
    );

    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    // The row the loop publishes: the seat as its object — which, for this
    // seat, carries no name.
    document.seats = vec![SeatRow::from_observation(&configured, &seen, Some(1234))];
    let body = render(&document).unwrap();

    // The positive control: the row IS this seat's, so the absences below are
    // the projection's silence and not an empty document.
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["seats"][0]["seat"]["id"], configured.id.to_string());
    assert_eq!(parsed["seats"][0]["roster_state"], "present");

    for forbidden in ["\"model\"", "\"transient\"", "a-model"] {
        assert!(
            !body.contains(forbidden),
            "the projection must not carry {forbidden}:\n{body}"
        );
    }
}

/// A row names its seat as the one object every document carries: `{id, name,
/// kind}`, and nothing beside it. The two keys it replaced are gone rather than
/// kept alongside — a reader that still found `seat_dir` would go on keying on
/// it — and a seat with no name of its own carries no `name` key at all, which
/// a reader can tell from a name that is empty.
#[test]
fn a_row_names_its_seat_as_the_id_the_name_and_the_kind() {
    let orla = Seat {
        name: Some("Orla".to_string()),
        ..seat()
    };
    let seen = present(&orla, WORKTREE, "a-session");
    let nameless = Seat {
        id: id(TRANSIENT_ID),
        worktrees: vec![("demo".to_string(), OTHER.to_string())],
        ..seat()
    };
    let unseen = observe_seat(&roster(&[]), &nothing_hosted(), &nameless, 2_000);

    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    document.seats = vec![
        SeatRow::from_observation(&orla, &seen, Some(1234)),
        SeatRow::from_observation(&nameless, &unseen, None),
    ];
    let body = render(&document).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert_eq!(
        parsed["seats"][0]["seat"],
        serde_json::json!({ "id": SEAT_ID, "name": "Orla", "kind": "agent" }),
        "{body}"
    );
    for retired in ["seat_dir", "chosen_name"] {
        assert!(
            parsed["seats"][0].get(retired).is_none(),
            "the row carries no {retired}: {body}"
        );
    }
    assert_eq!(
        parsed["seats"][1]["seat"],
        serde_json::json!({ "id": TRANSIENT_ID, "kind": "agent" }),
        "{body}"
    );
    assert!(
        parsed["seats"][1]["seat"].get("name").is_none(),
        "a nameless seat's object has no name key: {body}"
    );

    // And the reader `fleet status` uses takes the document back.
    let read: Projection = serde_json::from_str(&body).expect("the writer's own document");
    assert_eq!(read.seats[0].seat.name.as_deref(), Some("Orla"));
    assert_eq!(read.seats[1].seat.name, None);
}

/// An Unknown seat publishes its cause and no context figure: a reading nobody
/// took is a named absence, never a stale number carried forward.
#[test]
fn an_unknown_seat_publishes_its_cause_and_no_reading() {
    let (host, _) = hosting(&seat());
    let seen = observe_seat(&parse_roster(""), &host.list(), &seat(), settled(&host));
    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    document.seats = vec![SeatRow::from_observation(&seat(), &seen, None)];
    let parsed: serde_json::Value = serde_json::from_str(&render(&document).unwrap()).unwrap();
    assert_eq!(parsed["seats"][0]["roster_state"], "unknown");
    assert!(parsed["seats"][0]["context_tokens"].is_null());
    assert!(parsed["seats"][0]["roster_unknown_cause"]
        .as_str()
        .is_some_and(|c| !c.is_empty()));
}

/// A trailing separator is not a different directory. Both sides of the match
/// are put in one form, so a configured path and a reported one that differ only
/// there are the same place — and the root is left alone, because trimming it
/// away leaves nothing to compare.
///
/// The match no longer attributes a row (the pid does); it is what finds a
/// live session standing in a seat's worktree with no session on the host, and
/// what names the project a present seat stands in.
#[test]
fn a_trailing_separator_on_either_side_is_the_same_directory() {
    let with_slash = format!("{WORKTREE}/");

    let read = roster(&[live(&with_slash, "a-session", 4242)]);
    assert_eq!(
        observe_seat(&read, &nothing_hosted(), &seat(), 2_000).state,
        RosterState::Unknown,
        "a reported cwd with a trailing separator stands in a worktree without one"
    );

    let mut slashed = seat();
    slashed.worktrees = vec![("demo".to_string(), with_slash.clone())];
    let read = roster(&[live(WORKTREE, "a-session", 4242)]);
    assert_eq!(
        observe_seat(&read, &nothing_hosted(), &slashed, 2_000).state,
        RosterState::Unknown,
        "and a configured worktree with one holds a cwd without"
    );
    assert_eq!(
        present(&slashed, WORKTREE, "a-session").project.as_deref(),
        Some("demo"),
        "and a present seat is placed in it"
    );

    assert_eq!(dir_key("/"), "/", "the root is left alone");

    // The control: a directory that differs by more than a separator is a
    // different directory, so the two matches above are the normalisation's and
    // not a matcher that says yes to everything.
    let read = roster(&[live(OTHER, "a-session", 4242)]);
    assert_eq!(
        observe_seat(&read, &nothing_hosted(), &seat(), 2_000).state,
        RosterState::Absent
    );
}

/// A seat no session answers for still names where it would be found — but
/// only when that is one place. Registered on several projects it has no one answer, and
/// the fields are absent rather than guessed.
#[test]
fn a_seat_on_several_projects_names_no_worktree_when_no_row_matches() {
    let empty = roster(&[]);

    let mut several = seat();
    several.worktrees = vec![
        ("demo".to_string(), WORKTREE.to_string()),
        ("other".to_string(), OTHER.to_string()),
    ];
    let seen = observe_seat(&empty, &nothing_hosted(), &several, 2_000);
    assert_eq!(seen.state, RosterState::Absent);
    assert!(
        seen.project.is_none() && seen.worktree.is_none(),
        "several projects have no one answer, and a guess is worse than none"
    );

    // The control: one project, and the same absent seat names both.
    let seen = observe_seat(&empty, &nothing_hosted(), &seat(), 2_000);
    assert_eq!(seen.state, RosterState::Absent);
    assert_eq!(seen.project.as_deref(), Some("demo"));
    assert_eq!(seen.worktree.as_deref(), Some(WORKTREE));
}

fn projection(agent_version: Option<&str>, expected: Option<&str>) -> Projection {
    Projection {
        version: VERSION,
        generated_at: "2026-09-06T00:00:00Z".to_string(),
        controller_version: "0.1.0".to_string(),
        agent_version: agent_version.map(str::to_string),
        agent_version_expected: expected.map(str::to_string),
        fleet: PolicyView {
            path: "/fleet/fleet.toml".to_string(),
            mtime: Some("2026-09-06T00:00:00Z".to_string()),
            poll_seconds: 5,
            claude_code: expected.map(str::to_string),
            plugin_dir: None,
        },
        fleet_parse_error: None,
        in_flight: None,
        effects: fleet_controller::projection::EffectsView::on(),
        grant: platform::GRANT_OK.to_string(),
        grant_detail: None,
        seats: Vec::new(),
        orders: Vec::new(),
    }
}

// ------------------------------------------------ the per-row config directory

/// AC2, D1 — a spawned seat is seen through ITS OWN configuration directory and
/// is unnamed by the fleet's, which is the whole cost of giving each spawned
/// seat its own configuration: a per-row directory is a per-row listing.
///
/// Measured live on this box on 2026-09-12 before the code was written: the
/// fleet's `agents --json --all` answered 9 rows and named none of the session
/// started under a scratch directory, whose own listing answered exactly 1 and
/// named only it.
///
/// The two listings differ in content, so the arm cannot pass by reading either
/// one twice — which is the mutant it exists to kill: a fold that asks for every
/// row under the adapter's own directory.
#[test]
fn a_transient_row_is_seen_through_its_own_directory_and_unseen_through_the_fleets() {
    let named = seat();
    let spawned = transient_seat();
    let seats = vec![named.clone(), spawned.clone()];
    let per_row_dir = "/machine/config/builder-9";
    // One host holding both seats' sessions, the named one's first.
    let (host, named_pid) = hosting(&named);
    host.new_session(
        &session_for(&spawned.id),
        Path::new(OTHER),
        &["/nowhere/agent".to_string()],
        &[],
    )
    .expect("the fake host starts the spawned seat's session");
    let spawned_pid = named_pid + 1;
    let at = settled(&host);

    // The fleet's listing names the NAMED seat's session and not the spawned
    // one's; the per-row listing names the spawned one's and nothing else.
    let reads = |dir: Option<&Path>| match dir {
        None => roster(&[live(WORKTREE, "named-session", named_pid)]),
        Some(dir) => {
            assert_eq!(
                dir,
                Path::new(per_row_dir),
                "the read is under the row's own"
            );
            roster(&[live(OTHER, "spawned-session", spawned_pid)])
        }
    };
    let rosters = Rosters::gather(
        &seats,
        &|seat: &SeatId| (*seat == spawned.id).then(|| per_row_dir.to_string()),
        &reads,
    );

    let seen = observe_seat(rosters.for_seat(&spawned.id), &host.list(), &spawned, at);
    assert_eq!(seen.state, RosterState::Present, "{seen:?}");
    assert_eq!(seen.session_id.as_deref(), Some("spawned-session"));

    // THE CONTROL, and the reason the arm is a measurement rather than a
    // restatement: the same seat decided against the FLEET's listing names no
    // row for its live pane, so the pair differs and the fold is what made the
    // difference.
    let missed = observe_seat(rosters.fleet(), &host.list(), &spawned, at);
    assert_eq!(missed.state, RosterState::Unknown, "{missed:?}");
    assert_eq!(missed.session_id, None);

    // And the named seat is still decided against the fleet's, unchanged.
    let named_seen = observe_seat(rosters.for_seat(&named.id), &host.list(), &named, at);
    assert_eq!(named_seen.state, RosterState::Present, "{named_seen:?}");
    assert_eq!(named_seen.session_id.as_deref(), Some("named-session"));
}

/// AC2 — a per-row listing nobody could read leaves THAT row Unknown, with the
/// cause, and every other seat decided. The failure this forbids is one
/// unreadable listing publishing the whole fleet as blind.
#[test]
fn a_per_row_listing_that_cannot_be_read_leaves_that_row_unknown_and_the_rest_decided() {
    let named = seat();
    let spawned = transient_seat();
    let seats = vec![named.clone(), spawned.clone()];
    let (host, named_pid) = hosting(&named);
    host.new_session(
        &session_for(&spawned.id),
        Path::new(OTHER),
        &["/nowhere/agent".to_string()],
        &[],
    )
    .expect("the fake host starts the spawned seat's session");
    let at = settled(&host);
    let reads = |dir: Option<&Path>| match dir {
        None => roster(&[live(WORKTREE, "named-session", named_pid)]),
        Some(_) => RosterRead::Unreadable {
            cause: "its listing's socket is unreachable".to_string(),
        },
    };
    let rosters = Rosters::gather(
        &seats,
        &|seat: &SeatId| (*seat == spawned.id).then(|| "/machine/config/builder-9".to_string()),
        &reads,
    );

    let blind = observe_seat(rosters.for_seat(&spawned.id), &host.list(), &spawned, at);
    assert_eq!(blind.state, RosterState::Unknown, "{blind:?}");
    assert!(
        blind
            .unknown_cause
            .as_deref()
            .unwrap_or_default()
            .contains("socket is unreachable"),
        "the cause is carried: {blind:?}"
    );

    let decided = observe_seat(rosters.for_seat(&named.id), &host.list(), &named, at);
    assert_eq!(decided.state, RosterState::Present, "{decided:?}");
    assert!(decided.unknown_cause.is_none());
}

/// AC2 — the transcript is read under the ROW's directory. The adapter resolves
/// the path from the directory it is handed, so a body written under one is
/// unreadable through the other.
#[test]
fn a_transcript_resolves_under_the_directory_it_is_read_with() {
    let root = std::env::temp_dir().join(format!("tt-observe-transcript-{}", std::process::id()));
    let fleet_dir = root.join("fleet-config");
    let row_dir = root.join("row-config");
    let session = "a-session";
    let body = "{\"type\":\"assistant\"}\n";

    let path = transcript_path(&row_dir, WORKTREE, session);
    std::fs::create_dir_all(path.parent().expect("the transcript has a parent"))
        .expect("the scratch directory is made");
    std::fs::write(&path, body).expect("the transcript is written");
    assert!(
        path.exists(),
        "the fixture is on disk at {}",
        path.display()
    );

    let agent = ClaudeCode::with_seams(
        "claude".to_string(),
        fleet_dir.clone(),
        Duration::from_secs(1),
        root.clone(),
        String::new(),
        None,
        String::new(),
    );
    assert_eq!(
        agent
            .transcript(Some(&row_dir), WORKTREE, session)
            .as_deref(),
        Some(body),
        "read under the row's directory it resolves"
    );
    // The control on that positive: the SAME call under the adapter's own
    // directory resolves nothing, so the argument is what decided it.
    assert_eq!(
        agent.transcript(None, WORKTREE, session),
        None,
        "and under the fleet's it does not"
    );

    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------- the logged-out dispatch

/// The provider's logged-out first turn, as it was read off a real transcript on
/// this box on 2026-09-12 (2.1.261): one synthetic assistant entry carrying the
/// cause, the flag and a window of zero.
fn logged_out_body() -> String {
    concat!(
        r#"{"type":"user","isSidechain":false,"message":{"role":"user"}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"isApiErrorMessage":true,"#,
        r#""error":"authentication_failed","message":{"model":"<synthetic>","#,
        r#""usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"#,
        r#""cache_read_input_tokens":0},"content":[{"type":"text","#,
        r#""text":"Not logged in · Please run /login"}]}}"#,
        "\n",
    )
    .to_string()
}

/// A first turn that ANSWERED, from the same probe's other arm: a real assistant
/// entry with a non-zero window.
fn answered_body() -> String {
    concat!(
        r#"{"type":"user","isSidechain":false,"message":{"role":"user"}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"message":{"model":"claude-sonnet-4-5","#,
        r#""usage":{"input_tokens":10,"output_tokens":219,"#,
        r#""cache_creation_input_tokens":36062,"cache_read_input_tokens":0}}}"#,
        "\n",
    )
    .to_string()
}

/// AC3 — a transient row whose transcript carries the logged-out shape yields
/// exactly ONE line, and a row whose first turn carries a window yields none.
///
/// "Exactly one" is the second poll: the first sighting writes the line and every
/// poll after it is already sighted, which is what stops a line per interval.
#[test]
fn a_logged_out_first_turn_yields_one_dispatch_failure_and_an_answered_one_yields_none() {
    let logged_out = logged_out_body();
    let answered = answered_body();

    // The first sighting of a transient row reading logged out.
    assert!(observe::logged_out_dispatch(
        true,
        RosterState::Present,
        false,
        Some(&logged_out)
    ));
    // The same row on every later poll: already sighted, so nothing more.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::Present,
        true,
        Some(&logged_out)
    ));
    // A row that ANSWERED, one variable apart from the first case.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::Present,
        false,
        Some(&answered)
    ));
    // A NAMED seat's session is the person's own, whatever its transcript says.
    assert!(!observe::logged_out_dispatch(
        false,
        RosterState::Present,
        false,
        Some(&logged_out)
    ));
    // A transcript that did not resolve is a reading nobody has.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::Present,
        false,
        None
    ));
    // And the reading comes off a LIVE row: an absent seat has no first turn.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::Absent,
        false,
        Some(&logged_out)
    ));

    // The line's own content: the seat and the item the order index named.
    let builder = SeatView::from(&seat().as_ref());
    let payload = events::dispatch_failed_payload(
        &builder,
        Some("an-item"),
        fleet_controller::observe::AUTHENTICATION_FAILED,
    );
    assert_eq!(payload["seat"]["id"], SEAT_ID);
    assert_eq!(payload["seat"]["kind"], "agent");
    assert_eq!(payload["item"], "an-item");
    assert_eq!(payload["cause"], "authentication_failed");
    // A start no order accompanied carries the key as null rather than dropping
    // it, so a reader meets a field it can read as absent.
    let no_item = events::dispatch_failed_payload(&builder, None, "authentication_failed");
    assert!(
        no_item.get("item").is_some_and(|v| v.is_null()),
        "{no_item}"
    );
}

/// AC3 — the reader's three terms, each one alone insufficient. The conjunction
/// is the safe direction: a term that moves in a later release yields NO reading,
/// and a reading nobody has costs one uncaught logged-out seat where a looser
/// match would fail a dispatch that was fine.
#[test]
fn the_logged_out_reader_needs_all_three_terms() {
    assert!(observe::logged_out_first_turn(&logged_out_body()));
    // The flag alone: an entry the provider wrote for some other cause.
    let other_cause = logged_out_body().replace("authentication_failed", "overloaded_error");
    assert!(!observe::logged_out_first_turn(&other_cause));
    // The cause alone, on an entry the provider did not write itself.
    let not_flagged = logged_out_body().replace(r#""isApiErrorMessage":true,"#, "");
    assert!(!observe::logged_out_first_turn(&not_flagged));
    // The pair, with a window: not a turn that never reached the model.
    let with_window = logged_out_body().replace(r#""input_tokens":0"#, r#""input_tokens":42"#);
    assert!(!observe::logged_out_first_turn(&with_window));
    // And the FIRST entry is the one read: a session that answered and later met
    // an auth failure was not a failed dispatch.
    let later = format!("{}{}", answered_body(), logged_out_body());
    assert!(!observe::logged_out_first_turn(&later));
}
