//! Fixture tests for the observe layer.
//!
//! The `lessons::` module below is the contract named in
//! `fleet/brain/lessons/*.md` § Test inventory: each fact the code in this slice
//! exercises owes a test under the exact name the inventory carries.

use fleet_controller::adapter::claude_code::{parse_roster, ClaudeCode};
use fleet_controller::adapter::{dir_key, encode_project_dir, transcript_path, Agent, RosterRead};
use fleet_controller::config::Seat;
use fleet_controller::events;
use fleet_controller::observe::{
    self, context_tokens_in, observe_seat, turns_in, Recency, RosterState, Rosters,
    FELL_BACK_TO_START, STARTING_GRACE_MS,
};
use fleet_controller::platform::{self, Grant, Listing, GRANT_OK, GRANT_PENDING};
use fleet_controller::policy::DEFAULT_STOPPED_RECENCY_HOURS;
use fleet_controller::projection::{render, EffectsView, PolicyView, Projection, SeatRow, VERSION};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const WORKTREE: &str = "/wt/builder-1";
const OTHER: &str = "/wt/builder-2";

/// The window in milliseconds, from the policy default rather than a copy of
/// the figure: an arm asserting where the edge falls reads the same number the
/// file's default carries.
const WINDOW_MS: u64 = DEFAULT_STOPPED_RECENCY_HOURS * 60 * 60 * 1000;

/// No session's transcript resolves. That is the FALLBACK reading — the window
/// runs on the start stamp — and it is what every arm here that is not about
/// the end wants, because it is the behaviour those arms were written under.
static NO_END: fn(&str, &str) -> Option<u64> = |_, _| None;

fn started() -> Recency<'static> {
    Recency {
        window_ms: WINDOW_MS,
        ended_at: &NO_END,
    }
}

fn seat() -> Seat {
    Seat {
        name: "builder-1".to_string(),
        chosen_name: Some("Rook".to_string()),
        model: None,
        transient: false,
        worktrees: vec![("demo".to_string(), WORKTREE.to_string())],
    }
}

/// A listing built from row bodies, in the shape the agent emits.
fn roster(rows: &[String]) -> RosterRead {
    parse_roster(&format!("[{}]", rows.join(",")))
}

fn live(cwd: &str, session: &str) -> String {
    format!(
        r#"{{"id":"{session}","sessionId":"{session}","cwd":"{cwd}","kind":"background",
            "pid":4242,"status":"idle","startedAt":1000}}"#
    )
}

fn waiting(cwd: &str, session: &str, cause: &str) -> String {
    format!(
        r#"{{"id":"{session}","sessionId":"{session}","cwd":"{cwd}","kind":"background",
            "pid":4242,"status":"idle","startedAt":1000,"waitingFor":"{cause}"}}"#
    )
}

/// A pid-less row the agent has registered and not yet given a process to: no
/// end marker, so `state` is what separates it from an ended one.
fn newborn(cwd: &str, session: &str, started_at: u64) -> String {
    format!(
        r#"{{"sessionId":"{session}","cwd":"{cwd}","kind":"background",
            "state":"working","startedAt":{started_at}}}"#
    )
}

fn ended(cwd: &str, session: &str, started_at: u64) -> String {
    format!(
        r#"{{"sessionId":"{session}","cwd":"{cwd}","kind":"background",
            "state":"done","startedAt":{started_at}}}"#
    )
}

/// A transient seat, whose session comes up under its own configuration
/// directory.
fn transient_seat() -> Seat {
    Seat {
        name: "builder-9".to_string(),
        chosen_name: None,
        model: None,
        transient: true,
        worktrees: vec![("demo".to_string(), OTHER.to_string())],
    }
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
    /// that requires a field on every row fails on the first mixed listing.
    #[test]
    fn the_roster_is_one_command() {
        let mixed = parse_roster(
            r#"[
              {"id":"aa","sessionId":"aa","cwd":"/wt/builder-1","kind":"background",
               "pid":1,"status":"idle","state":"running","name":"rook","startedAt":10,
               "someFieldNobodyHasSeen":"harmless"},
              {"sessionId":"bb","cwd":"/wt/builder-2","kind":"interactive"}
            ]"#,
        );
        match mixed {
            RosterRead::Readable(rows) => {
                assert_eq!(rows.len(), 2, "both kinds survive one read");
                assert!(rows[0].is_live());
                assert!(!rows[1].is_live(), "an interactive row carries no pid");
            }
            RosterRead::Unreadable { cause } => panic!("the listing must parse: {cause}"),
        }
    }

    /// claude-code B2 — there is no token figure anywhere in the listing, so
    /// context accounting cannot come from it. A number that looks like one on a
    /// row is not the seat's context: the transcript is.
    #[test]
    fn the_roster_carries_no_token_field() {
        let read = parse_roster(
            r#"[{"id":"aa","sessionId":"aa","cwd":"/wt/builder-1","kind":"background",
                 "pid":1,"startedAt":10,"tokens":999999,"input_tokens":999999}]"#,
        );
        let seen = observe_seat(&read, &seat(), 2_000, &started());
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
        let silent = observe_seat(&parse_roster(""), &seat(), 2_000, &started());
        assert_eq!(silent.state, RosterState::Unknown);
        assert!(
            silent.unknown_cause.is_some(),
            "the cause travels with the Unknown"
        );

        let answered = observe_seat(&parse_roster("[]"), &seat(), 2_000, &started());
        assert_eq!(
            answered.state,
            RosterState::Absent,
            "a listing that answered and said nothing is not the same read"
        );
    }

    /// claude-code B5 — `cwd` names a seat and proves nothing. One worktree per
    /// seat makes the match a function; two live rows in one worktree cannot be
    /// told apart, and picking one is how an unattributed session gets handed to
    /// a seat as its own.
    #[test]
    fn cwd_names_a_seat_and_proves_nothing() {
        let mine = observe_seat(&roster(&[live(WORKTREE, "aa")]), &seat(), 2_000, &started());
        assert_eq!(mine.state, RosterState::Present);
        assert_eq!(mine.session_id.as_deref(), Some("aa"));
        assert_eq!(mine.project.as_deref(), Some("demo"));

        let elsewhere = observe_seat(&roster(&[live(OTHER, "bb")]), &seat(), 2_000, &started());
        assert_eq!(
            elsewhere.state,
            RosterState::Absent,
            "a row in another seat's worktree is not this seat's"
        );

        let contested = observe_seat(
            &roster(&[live(WORKTREE, "aa"), live(WORKTREE, "bb")]),
            &seat(),
            2_000,
            &started(),
        );
        assert_eq!(contested.state, RosterState::Unknown);
        assert!(
            contested.session_id.is_none(),
            "an unattributed pair names no session"
        );

        // Two ENDED rows beside a live one is a seat working normally beside its
        // own history: the partition is by pid, never by count.
        let history = observe_seat(
            &roster(&[
                live(WORKTREE, "aa"),
                format!(
                    r#"{{"sessionId":"old","cwd":"{WORKTREE}","kind":"background",
                        "state":"done","startedAt":500}}"#
                ),
            ]),
            &seat(),
            2_000,
            &started(),
        );
        assert_eq!(history.state, RosterState::Present);
        assert_eq!(history.session_id.as_deref(), Some("aa"));
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
    }

    /// claude-code B8 — one field says a session is stopped in front of a human,
    /// and it is keyed on PRESENCE. The vocabulary is the agent's, so a cause
    /// this fleet has never seen must still stop the seat rather than read as a
    /// healthy one; the control below is the same row without the field.
    #[test]
    fn waiting_for_names_the_block() {
        let blocked = observe_seat(
            &roster(&[waiting(WORKTREE, "aa", "input needed")]),
            &seat(),
            2_000,
            &started(),
        );
        assert_eq!(blocked.state, RosterState::PromptBlocked);
        assert_eq!(blocked.waiting_for.as_deref(), Some("input needed"));
        assert_eq!(blocked.session_id.as_deref(), Some("aa"));

        let unrecognised = observe_seat(
            &roster(&[waiting(WORKTREE, "aa", "a cause nobody has enumerated")]),
            &seat(),
            2_000,
            &started(),
        );
        assert_eq!(
            unrecognised.state,
            RosterState::PromptBlocked,
            "presence, never the value: an unknown cause still stops the seat"
        );

        let control = observe_seat(&roster(&[live(WORKTREE, "aa")]), &seat(), 2_000, &started());
        assert_eq!(control.state, RosterState::Present);
        assert_eq!(control.waiting_for, None);

        // The reference publishes no context reading for this state, and the
        // acceptance is row-for-row parity with it.
        assert!(!RosterState::PromptBlocked.has_context_reading());
        assert!(RosterState::Present.has_context_reading());
        assert!(RosterState::Stopped.has_context_reading());
        assert!(!RosterState::Starting.has_context_reading());
    }

    /// claude-code A2 — a row with no pid is two different states: a session
    /// that has ENDED, and one that has not finished STARTING. `state` is what
    /// tells them apart, and reading pid alone collapses the second into the
    /// first and spawns over a newborn.
    #[test]
    fn pid_null_is_two_states() {
        let newborn = newborn(WORKTREE, "new", 100_000);
        let ended = ended(WORKTREE, "old", 90_000);

        // Inside the grace window, a pid-less row with no end marker is starting.
        let starting = observe_seat(
            &roster(std::slice::from_ref(&newborn)),
            &seat(),
            110_000,
            &started(),
        );
        assert_eq!(starting.state, RosterState::Starting);
        assert_eq!(starting.session_id.as_deref(), Some("new"));

        // Past it, the same row is not: the grace bound is what separates a slow
        // start from a row that will never gain a pid.
        let aged = observe_seat(
            &roster(std::slice::from_ref(&newborn)),
            &seat(),
            100_000 + STARTING_GRACE_MS + 1,
            &started(),
        );
        assert_eq!(aged.state, RosterState::Stopped);

        // A row carrying the end marker is stopped inside the same window.
        let stopped = observe_seat(
            &roster(std::slice::from_ref(&ended)),
            &seat(),
            110_000,
            &started(),
        );
        assert_eq!(stopped.state, RosterState::Stopped);
        assert_eq!(stopped.session_id.as_deref(), Some("old"));

        // The ordering: a starting row outranks the ended ones beside it, even
        // when an ended row started later.
        let both = observe_seat(
            &roster(&[ended.clone(), newborn.clone()]),
            &seat(),
            110_000,
            &started(),
        );
        assert_eq!(both.state, RosterState::Starting);
        assert_eq!(both.session_id.as_deref(), Some("new"));

        // Past the recency bound every ended row is history and the seat reads
        // plainly absent.
        let history = observe_seat(
            &roster(&[ended]),
            &seat(),
            90_000 + WINDOW_MS + 1,
            &started(),
        );
        assert_eq!(history.state, RosterState::Absent);
        assert_eq!(history.session_id, None);
    }

    /// The window is measured from the session's END, and this is what happens
    /// when there is no end to measure from.
    ///
    /// The listing carries no end stamp on any row (measured 2.1.261), so the
    /// end is the transcript's last write, and a transcript that does not
    /// resolve leaves the start stamp as the only reading there is. A row kept
    /// on that reading says so: the fallback is a different question answered —
    /// how long the session RAN, not how long ago it finished — and a reader
    /// that cannot tell the two apart is back where this rule started.
    #[test]
    fn a_stopped_row_with_no_end_falls_back_to_its_start_and_names_the_fallback() {
        let old = ended(WORKTREE, "old", 1_000);
        let ended_now: fn(&str, &str) -> Option<u64> = |_, _| Some(1_000_000);
        let ends = Recency {
            window_ms: WINDOW_MS,
            ended_at: &ended_now,
        };

        // With an end that resolves: judged on it, and nothing to say.
        let on_its_end = observe_seat(
            &roster(std::slice::from_ref(&old)),
            &seat(),
            1_060_000,
            &ends,
        );
        assert_eq!(on_its_end.state, RosterState::Stopped);
        assert_eq!(
            on_its_end.recency_fallback, None,
            "a row judged on its end has no fallback to report"
        );

        // Without one: the same row, the same window, judged on its start — and
        // the reading is named rather than passed off as the one above.
        let on_its_start = observe_seat(
            &roster(std::slice::from_ref(&old)),
            &seat(),
            1_060_000,
            &started(),
        );
        assert_eq!(on_its_start.state, RosterState::Stopped);
        assert_eq!(
            on_its_start.recency_fallback.as_deref(),
            Some(FELL_BACK_TO_START),
            "a row kept on its start stamp says which stamp kept it"
        );

        // And the fallback is a real reading and not a pass: the same row is
        // history once its START is outside the window, which is the behaviour
        // the end-keyed rule replaces and the only one available here.
        let past_it = observe_seat(&roster(&[old]), &seat(), 1_000 + WINDOW_MS + 1, &started());
        assert_eq!(past_it.state, RosterState::Absent);
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

    /// The turn reader beside the window reader (flights PRD R12): the SAME
    /// filter, one step shorter.
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
    /// DAEMON, and this slice's consequence: a per-row directory is a per-row
    /// daemon, and a per-row daemon is a per-row ROSTER, so the fleet's listing
    /// does not name a session started under one and every read about that
    /// session has to be made under the same directory.
    ///
    /// Measured live on this box on 2026-09-12 on 2.1.261, before the code was
    /// written: two daemons side by side, the fleet's pid unchanged across the
    /// probe; the fleet's `agents --json --all` answered 9 rows and named none of
    /// the session under the scratch directory, whose own listing answered
    /// exactly 1 and named only it; and `claude logs <short id>` from the fleet's
    /// directory exited 1 with "No job matching" for that same live session.
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

        // Which directories the poll asked for, in call order.
        let asked = std::cell::RefCell::new(Vec::new());
        let reads = |dir: Option<&Path>| {
            asked
                .borrow_mut()
                .push(dir.map(|d| d.display().to_string()));
            match dir {
                None => roster(&[live(WORKTREE, "named-session")]),
                Some(_) => roster(&[live(OTHER, "spawned-session")]),
            }
        };
        let rosters = Rosters::gather(
            &seats,
            &|seat: &str| (seat == "builder-9").then(|| per_row_dir.to_string()),
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

        // And the row is decided against the listing that could see it, while the
        // fleet's — the same moment, the same fold — reads it absent.
        assert_eq!(
            observe_seat(rosters.for_seat(&spawned.name), &spawned, 2_000, &started()).state,
            RosterState::Present
        );
        assert_eq!(
            observe_seat(rosters.fleet(), &spawned, 2_000, &started()).state,
            RosterState::Absent
        );
    }
}

/// The grant as the projection publishes it (R26, R33): the state, the detail
/// while it is pending, and the effects view the two of them produce.
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
/// on any row, in any state (PRD R26).
#[test]
fn the_projection_carries_no_pid_and_no_handle() {
    let read = parse_roster(
        r#"[{"id":"short-id","sessionId":"a-session-id","cwd":"/wt/builder-1",
             "kind":"background","pid":4242,"startedAt":10}]"#,
    );
    let seen = observe_seat(&read, &seat(), 2_000, &started());
    assert_eq!(seen.state, RosterState::Present);

    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    document.seats = vec![SeatRow::from_observation(
        "builder-1",
        Some("Rook"),
        &seen,
        Some(1234),
    )];
    let body = render(&document).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["seats"][0]["context_tokens"], 1234);
    for forbidden in [
        "\"pid\"",
        "\"handle\"",
        "\"session_id\"",
        "4242",
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
/// The forbidden-key shape above, over a PRESENT row: `from_observation` never
/// reads the `Seat`, so the two fields are set to show that the projection is
/// not handed them rather than to feed them in, and the state is what has to be
/// asserted — an Unknown row carries neither key either, and an arm that read
/// one would pass while a leak scoped to a seat that was found went by.
#[test]
fn the_projection_carries_no_model_and_no_transient() {
    let configured = Seat {
        model: Some("a-model".to_string()),
        transient: true,
        ..seat()
    };
    let seen = observe_seat(
        &roster(&[live(WORKTREE, "a-session")]),
        &configured,
        2_000,
        &started(),
    );
    assert_eq!(
        seen.state,
        RosterState::Present,
        "the row below is a seat that was FOUND, which is the shape a leak would ride"
    );

    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    document.seats = vec![SeatRow::from_observation(
        &configured.name,
        configured.chosen_name.as_deref(),
        &seen,
        Some(1234),
    )];
    let body = render(&document).unwrap();

    // The positive control: the row IS this seat's, so the absences below are
    // the projection's silence and not an empty document.
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["seats"][0]["seat_dir"], "builder-1");
    assert_eq!(parsed["seats"][0]["chosen_name"], "Rook");
    assert_eq!(parsed["seats"][0]["roster_state"], "present");

    for forbidden in ["\"model\"", "\"transient\"", "a-model"] {
        assert!(
            !body.contains(forbidden),
            "the projection must not carry {forbidden}:\n{body}"
        );
    }
}

/// An Unknown seat publishes its cause and no context figure: a reading nobody
/// took is a named absence, never a stale number carried forward.
#[test]
fn an_unknown_seat_publishes_its_cause_and_no_reading() {
    let seen = observe_seat(&parse_roster(""), &seat(), 2_000, &started());
    let mut document = projection(Some("2.1.261"), Some("2.1.261"));
    document.seats = vec![SeatRow::from_observation("builder-1", None, &seen, None)];
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
#[test]
fn a_trailing_separator_on_either_side_is_the_same_directory() {
    let with_slash = format!("{WORKTREE}/");

    let read = roster(&[live(&with_slash, "a-session")]);
    assert_eq!(
        observe_seat(&read, &seat(), 2_000, &started()).state,
        RosterState::Present,
        "a reported cwd with a trailing separator matches a worktree without one"
    );

    let mut slashed = seat();
    slashed.worktrees = vec![("demo".to_string(), with_slash.clone())];
    let read = roster(&[live(WORKTREE, "a-session")]);
    assert_eq!(
        observe_seat(&read, &slashed, 2_000, &started()).state,
        RosterState::Present,
        "and a configured worktree with one matches a cwd without"
    );

    assert_eq!(dir_key("/"), "/", "the root is left alone");

    // The control: a directory that differs by more than a separator is a
    // different directory, so the two matches above are the normalisation's and
    // not a matcher that says yes to everything.
    let read = roster(&[live(OTHER, "a-session")]);
    assert_eq!(
        observe_seat(&read, &seat(), 2_000, &started()).state,
        RosterState::Absent
    );
}

/// A seat no row matched still names where it would be found — but only when
/// that is one place. Registered on several projects it has no one answer, and
/// the fields are absent rather than guessed.
#[test]
fn a_seat_on_several_projects_names_no_worktree_when_no_row_matches() {
    let empty = roster(&[]);

    let mut several = seat();
    several.worktrees = vec![
        ("demo".to_string(), WORKTREE.to_string()),
        ("other".to_string(), OTHER.to_string()),
    ];
    let seen = observe_seat(&empty, &several, 2_000, &started());
    assert_eq!(seen.state, RosterState::Absent);
    assert!(
        seen.project.is_none() && seen.worktree.is_none(),
        "several projects have no one answer, and a guess is worse than none"
    );

    // The control: one project, and the same absent seat names both.
    let seen = observe_seat(&empty, &seat(), 2_000, &started());
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
/// is absent from the fleet's, which is the whole cost of the isolation R13
/// rules: a per-row directory is a per-row daemon, and a per-row daemon is a
/// per-row roster.
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

    // The fleet's listing names the NAMED seat's session and not the spawned
    // one's; the per-row listing names the spawned one's and nothing else.
    let reads = |dir: Option<&Path>| match dir {
        None => roster(&[live(WORKTREE, "named-session")]),
        Some(dir) => {
            assert_eq!(
                dir,
                Path::new(per_row_dir),
                "the read is under the row's own"
            );
            roster(&[live(OTHER, "spawned-session")])
        }
    };
    let rosters = Rosters::gather(
        &seats,
        &|seat: &str| (seat == "builder-9").then(|| per_row_dir.to_string()),
        &reads,
    );

    let seen = observe_seat(rosters.for_seat(&spawned.name), &spawned, 2_000, &started());
    assert_eq!(seen.state, RosterState::Present, "{seen:?}");
    assert_eq!(seen.session_id.as_deref(), Some("spawned-session"));

    // THE CONTROL, and the reason the arm is a measurement rather than a
    // restatement: the same seat decided against the FLEET's listing is absent,
    // so the pair differs and the fold is what made the difference.
    let missed = observe_seat(rosters.fleet(), &spawned, 2_000, &started());
    assert_eq!(missed.state, RosterState::Absent, "{missed:?}");
    assert_eq!(missed.session_id, None);

    // And the named seat is still decided against the fleet's, unchanged.
    let named_seen = observe_seat(rosters.for_seat(&named.name), &named, 2_000, &started());
    assert_eq!(named_seen.state, RosterState::Present, "{named_seen:?}");
    assert_eq!(named_seen.session_id.as_deref(), Some("named-session"));
}

/// AC2 — a per-row listing nobody could read leaves THAT row Unknown, with the
/// cause, and every other seat decided. The failure this forbids is one
/// unreachable daemon publishing the whole fleet as blind.
#[test]
fn a_per_row_listing_that_cannot_be_read_leaves_that_row_unknown_and_the_rest_decided() {
    let named = seat();
    let spawned = transient_seat();
    let seats = vec![named.clone(), spawned.clone()];
    let reads = |dir: Option<&Path>| match dir {
        None => roster(&[live(WORKTREE, "named-session")]),
        Some(_) => RosterRead::Unreadable {
            cause: "its daemon's socket is unreachable".to_string(),
        },
    };
    let rosters = Rosters::gather(
        &seats,
        &|seat: &str| (seat == "builder-9").then(|| "/machine/config/builder-9".to_string()),
        &reads,
    );

    let blind = observe_seat(rosters.for_seat(&spawned.name), &spawned, 2_000, &started());
    assert_eq!(blind.state, RosterState::Unknown, "{blind:?}");
    assert!(
        blind
            .unknown_cause
            .as_deref()
            .unwrap_or_default()
            .contains("socket is unreachable"),
        "the cause is carried: {blind:?}"
    );

    let decided = observe_seat(rosters.for_seat(&named.name), &named, 2_000, &started());
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
        r#"{"type":"assistant","isSidechain":false,"message":{"model":"claude-haiku-4-5","#,
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
    let payload = events::dispatch_failed_payload(
        "builder-9",
        Some("an-item"),
        fleet_controller::observe::AUTHENTICATION_FAILED,
    );
    assert_eq!(payload["seat"], "builder-9");
    assert_eq!(payload["item"], "an-item");
    assert_eq!(payload["cause"], "authentication_failed");
    // A start no order accompanied carries the key as null rather than dropping
    // it, so a reader meets a field it can read as absent.
    let no_item = events::dispatch_failed_payload("builder-9", None, "authentication_failed");
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
