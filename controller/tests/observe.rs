//! Fixture tests for the observe layer.
//!
//! The `lessons::` module below is the contract named in
//! `fleet/brain/lessons/*.md` § Test inventory: each fact the code in this slice
//! exercises owes a test under the exact name the inventory carries. The facts
//! about the agent's own listing and transcript — how a row is parsed, found
//! and read, and what a transcript says — are the in-process adapter's and are
//! held by its own arms (`adapter::claude_code`'s `tests::lessons`); what is
//! here is core's.
//!
//! A seat is read from TWO answers (ruling 3): the host's, which says whether
//! its session is there, and the agent's `read`, which says what the session is
//! doing. The host's is a [`FakeHost`] here — its own panes, listed by its own
//! `list` — and the agent's is the listing RECORDED on the supported release
//! below, read by the one real adapter's own rules, so the arms decide real
//! readings against a host that is not real.

use fleet_controller::adapter::claude_code::{self, readings_from};
use fleet_controller::adapter::{
    dir_key, Activity, Agent, AgentError, Argv, BlockedOn, Capabilities, Evidence, Launch, Posture,
    Resume, SeatActivity, SeatContext, SeatRef, Version,
};
use fleet_controller::config::Seat;
use fleet_controller::events;
use fleet_controller::host::{session_for, Host, HostRead, Pane, PaneState};
use fleet_controller::observe::{
    self, observe_fleet, observe_seat, Held, RosterState, SeatObservation, STARTING_GRACE_MS,
};
use fleet_controller::platform::{self, Grant, Listing, GRANT_OK, GRANT_PENDING};
use fleet_controller::projection::{
    render, AgentView, EffectsView, PolicyView, Projection, SeatRow, SeatView, VERSION,
};
use fleet_controller::run::{self, Options, Seams, StopHandler};
use fleet_controller::test_support::{
    self, Answers, FakeClock, FakeHost, StubAgent, FIRST_PANE_PID,
};
use fleet_core::seat::identity::SeatId;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod common;

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

/// The agent's reading of `seat`'s pane `pid`, off a listing body, by the one
/// real adapter's own rules — the recording read the way `read` reads it.
fn read_off(body: &str, seat: &Seat, pid: u32) -> SeatActivity {
    let body = body.to_string();
    readings_from(
        &[SeatRef {
            seat: seat.id,
            session_id: None,
            pid: Some(pid),
            config_dir: None,
            worktree: WORKTREE.to_string(),
            screen: None,
        }],
        &|_: Option<&str>| Ok(body.clone()),
        &|_: &SeatRef, _: &str| None,
    )
    .remove(0)
}

/// A recorded listing with its pid moved onto `pid` — how a recorded row is
/// laid beside a [`FakeHost`] pane, which numbers its own — read for `seat`.
fn recorded_as(body: &str, seat: &Seat, pid: u32) -> SeatActivity {
    read_off(
        &body.replace(&RECORDED_PID.to_string(), &pid.to_string()),
        seat,
        pid,
    )
}

/// A reading built whole, for the arms whose subject is core's decision and
/// not how an adapter reaches it.
fn reading(
    seat: &Seat,
    activity: Activity,
    session: Option<&str>,
    cause: Option<&str>,
) -> SeatActivity {
    SeatActivity {
        seat: seat.id,
        activity,
        blocked_on: None,
        evidence: Evidence::Typed,
        session_id: session.map(str::to_string),
        cause: cause.map(str::to_string),
    }
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

/// An interactive row, as the recording's rows read: a pid and a status, and
/// no address and no state (lessons claude-code B10).
fn live(cwd: &str, session: &str, pid: u32) -> String {
    format!(
        r#"{{"sessionId":"{session}","cwd":"{cwd}","kind":"interactive","name":"orla",
            "pid":{pid},"status":"idle","startedAt":1000}}"#
    )
}

/// The one seat decided against a live session this suite's fake host holds
/// and a reading naming it by the pane's pid.
fn present(seat: &Seat, cwd: &str, session: &str) -> SeatObservation {
    let (host, pid) = hosting(seat);
    let read = read_off(&format!("[{}]", live(cwd, session, pid)), seat, pid);
    observe_seat(&host.list(), seat, Some(&read), settled(&host))
}

// --------------------------------------------- an agent answering by directory

/// An agent whose `read` answers every directory from its own listing — the
/// one real adapter's rules over listings an arm hands in per directory — and
/// records which directories it was asked under. Nothing else it answers is
/// asked of it here.
struct ByDirectory<F: Fn(Option<&str>) -> Result<String, String>> {
    listing: F,
    asked: RefCell<Vec<Option<String>>>,
}

impl<F: Fn(Option<&str>) -> Result<String, String>> ByDirectory<F> {
    fn new(listing: F) -> Self {
        ByDirectory {
            listing,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl<F: Fn(Option<&str>) -> Result<String, String>> Agent for ByDirectory<F> {
    fn capabilities(&self) -> Result<Capabilities, AgentError> {
        Ok(claude_code::capabilities())
    }

    fn version(&self) -> Result<Version, AgentError> {
        Err(AgentError::Unreadable("not asked here".to_string()))
    }

    fn launch(&self, _: &Launch) -> Result<Argv, AgentError> {
        Err(AgentError::Unreadable("not asked here".to_string()))
    }

    fn resume(&self, _: &Resume) -> Result<Argv, AgentError> {
        Err(AgentError::Unreadable("not asked here".to_string()))
    }

    fn read(&self, seats: &[SeatRef]) -> Result<Vec<SeatActivity>, AgentError> {
        Ok(readings_from(
            seats,
            &|dir: Option<&str>| {
                self.asked.borrow_mut().push(dir.map(str::to_string));
                (self.listing)(dir)
            },
            &|_: &SeatRef, _: &str| None,
        ))
    }

    fn context(&self, _: &[SeatRef]) -> Result<Vec<SeatContext>, AgentError> {
        Ok(Vec::new())
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

    /// claude-code B8, core's half (the listing's half is the adapter's
    /// `waiting_for_names_the_block`) — a session stopped in front of a human is keyed on the
    /// agent's reading being BLOCKED, never on what it names: the vocabulary
    /// is the agent's, so a cause this fleet has never seen must still stop the
    /// seat rather than read as a healthy one; the control is the busy row.
    ///
    /// On the INTERACTIVE row the recording read (B10): `waiting` and
    /// `permission prompt` at the approval dialog, carried verbatim.
    #[test]
    fn a_blocked_reading_is_prompt_blocked_whatever_it_names() {
        let at_the_dialog = recorded_seat();
        let blocked = observe_seat(
            &recorded_pane(&at_the_dialog, PaneState::Alive),
            &at_the_dialog,
            Some(&read_off(RECORDED_WAITING, &at_the_dialog, RECORDED_PID)),
            RECORDED_CREATED_MS + 30_000,
        );
        assert_eq!(blocked.state, RosterState::PromptBlocked);
        assert_eq!(blocked.waiting_for.as_deref(), Some("permission prompt"));
        assert_eq!(blocked.blocked_on, Some(BlockedOn::Permission));
        assert_eq!(blocked.activity, Some(Activity::Blocked));
        assert_eq!(blocked.session_id.as_deref(), Some(RECORDED_SESSION));

        let (host, _) = hosting(&seat());
        let unrecognised = observe_seat(
            &host.list(),
            &seat(),
            Some(&reading(
                &seat(),
                Activity::Blocked,
                Some("aa"),
                Some("a cause nobody has enumerated"),
            )),
            settled(&host),
        );
        assert_eq!(
            unrecognised.state,
            RosterState::PromptBlocked,
            "presence, never the value: an unknown cause still stops the seat"
        );
        assert_eq!(
            unrecognised.waiting_for.as_deref(),
            Some("a cause nobody has enumerated")
        );
        // And a block the agent names nothing for at all is blocked all the
        // same, published as the bare word.
        let nameless = observe_seat(
            &host.list(),
            &seat(),
            Some(&reading(&seat(), Activity::Blocked, None, None)),
            settled(&host),
        );
        assert_eq!(nameless.state, RosterState::PromptBlocked);
        assert_eq!(nameless.waiting_for.as_deref(), Some("blocked"));

        let control = observe_seat(
            &recorded_pane(&at_the_dialog, PaneState::Alive),
            &at_the_dialog,
            Some(&read_off(RECORDED_BUSY, &at_the_dialog, RECORDED_PID)),
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

    /// claude-code B10, core's half (the adapter's is
    /// `an_interactive_row_is_listed_without_an_address`) — the recording, decided: every row the agent printed,
    /// read by the adapter's rules and laid beside the pane the host listed,
    /// is the seat's live session with its activity; and the session's end is
    /// the HOST's reading — the listing's empty answer after `kill-session`
    /// beside no pane is absent, and `/exit`'s dead pane is stopped.
    #[test]
    fn the_recording_decided_is_the_seats_live_session_and_its_end_the_hosts() {
        let recorded = recorded_seat();
        let alive = recorded_pane(&recorded, PaneState::Alive);
        let at = RECORDED_CREATED_MS + 30_000;

        for (body, activity) in [
            (RECORDED_IDLE, Activity::Idle),
            (RECORDED_BUSY, Activity::Busy),
            (RECORDED_WAITING, Activity::Blocked),
        ] {
            let read = read_off(body, &recorded, RECORDED_PID);
            let seen = observe_seat(&alive, &recorded, Some(&read), at);
            assert!(
                matches!(
                    seen.state,
                    RosterState::Present | RosterState::PromptBlocked
                ),
                "{body}: {seen:?}"
            );
            assert_eq!(seen.activity, Some(activity));
            assert_eq!(seen.pane_pid, Some(RECORDED_PID));
            assert_eq!(seen.project.as_deref(), Some("measured"));
        }

        // `kill-session`: the next read lists nothing, and the host holds no
        // session. The seat is absent, and nothing stands in for an end.
        let killed = observe_seat(&nothing_hosted(), &recorded, None, at);
        assert_eq!(killed.state, RosterState::Absent);
        assert_eq!(killed.session_id, None);

        // `/exit`: the row went as fast (0.22 s), and the pane stayed, dead
        // with status 0 and its pid, under remain-on-exit. The end is the
        // host's reading and the agent has nothing to add to it.
        let exited = observe_seat(
            &recorded_pane(&recorded, PaneState::Dead { status: Some(0) }),
            &recorded,
            Some(&read_off(RECORDED_GONE, &recorded, RECORDED_PID)),
            at,
        );
        assert_eq!(exited.state, RosterState::Stopped);
        assert_eq!(exited.exit_status, Some(0));
        assert_eq!(exited.pane_pid, Some(RECORDED_PID));
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
}

// ------------------------------------------------ presence against activity
//
// One arm per case of `observe_seat`, in its own order (fleet-rge6.3): what
// the host holds for the seat, and what the agent answered beside it.

/// The host's own listing could not be read: Unknown, with the host's cause,
/// whatever the agent says — a seat whose presence nobody read is never
/// decided on its activity alone.
#[test]
fn an_unreadable_host_is_unknown_with_the_hosts_cause() {
    let unreadable = HostRead::Unreadable {
        cause: "the server said no".to_string(),
    };
    let seen = observe_seat(
        &unreadable,
        &seat(),
        Some(&reading(&seat(), Activity::Idle, Some("aa"), None)),
        2_000,
    );
    assert_eq!(seen.state, RosterState::Unknown);
    assert_eq!(seen.unknown_cause.as_deref(), Some("the server said no"));
    assert_eq!(seen.session_id, None);
}

/// No session on the host: Absent, which is the one state a spawn is issued
/// from.
#[test]
fn no_session_and_no_live_row_is_absent() {
    let seen = observe_seat(&nothing_hosted(), &seat(), None, 2_000);
    assert_eq!(seen.state, RosterState::Absent);
    assert_eq!(seen.unknown_cause, None);
    assert_eq!(seen.pane_pid, None);

    // Another seat's session on the host is not this one's.
    let (host, _) = hosting(&transient_seat());
    let beside = observe_seat(&host.list(), &seat(), None, settled(&host));
    assert_eq!(beside.state, RosterState::Absent);
}

/// No session on the host, and the agent running a LIVE session standing in
/// the seat's worktree: still ABSENT. No seat is found by its working
/// directory, anywhere (CORRECTIONS AT REVIEW, 2026-09-25; lessons claude-code
/// B5) — the agent is only ever asked about a seat's own pane, so a session
/// fleet does not host is invisible to the seat and never handed to it.
///
/// This replaces fleet-rge6.3's Unknown for the same shape, which read the
/// listing's rows by directory; the upgrade refusal (ruling 10) is what stops a
/// start beside a session Claude Code's daemon still hosts.
#[test]
fn no_session_beside_a_live_row_in_the_worktree_is_absent_and_never_the_seats() {
    let unhosted = recorded_seat();
    let asked = RefCell::new(0);
    let agent = ByDirectory::new(|_: Option<&str>| {
        *asked.borrow_mut() += 1;
        Ok(RECORDED_IDLE.to_string())
    });
    let seen = observe_fleet(
        &agent,
        &nothing_hosted(),
        std::slice::from_ref(&unhosted),
        &|_: &SeatId| Held::default(),
        2_000,
    );
    assert_eq!(seen[0].state, RosterState::Absent);
    assert_eq!(seen[0].unknown_cause, None);
    assert_eq!(seen[0].session_id, None, "and it is not handed the session");
    assert_eq!(
        *asked.borrow(),
        0,
        "a seat with no pane is asked about by nobody"
    );
}

/// The seat's pane is DEAD: Stopped, with the status it exited with and the
/// pid it had. The agent's answer is not read — the session left with its
/// process — and one nobody could make changes nothing.
#[test]
fn a_dead_pane_is_stopped_with_its_exit_status() {
    let (host, pid) = hosting(&seat());
    host.end(&session_for(&seat().id), Some(3));
    let seen = observe_seat(&host.list(), &seat(), None, settled(&host));
    assert_eq!(seen.state, RosterState::Stopped);
    assert_eq!(seen.exit_status, Some(3));
    assert_eq!(seen.pane_pid, Some(pid));
    assert_eq!(seen.worktree.as_deref(), Some(WORKTREE));

    let blind = observe_seat(
        &host.list(),
        &seat(),
        Some(&reading(
            &seat(),
            Activity::Unknown,
            None,
            Some("the listing could not be read: zero bytes"),
        )),
        settled(&host),
    );
    assert_eq!(blind.state, RosterState::Stopped);

    // A signal leaves no status, and the end is still an end.
    let (host, _) = hosting(&seat());
    host.end(&session_for(&seat().id), None);
    let signalled = observe_seat(&host.list(), &seat(), None, settled(&host));
    assert_eq!(signalled.state, RosterState::Stopped);
    assert_eq!(signalled.exit_status, None);
}

/// A LIVE pane beside a reading the agent could not make: Unknown, naming both
/// — never Present on a reading nobody took (reviewer call 2026-09-25 (1)) —
/// and whatever the pane's age: a young pane is not starting on a read that
/// never happened.
#[test]
fn a_live_pane_beside_an_unreadable_listing_is_unknown_naming_both() {
    let (host, pid) = hosting(&seat());
    let unreadable = read_off("", &seat(), pid);
    assert_eq!(unreadable.activity, Activity::Unknown);
    for at in [created(&host) + 500, settled(&host)] {
        let seen = observe_seat(&host.list(), &seat(), Some(&unreadable), at);
        assert_eq!(seen.state, RosterState::Unknown, "at {at}");
        let cause = seen
            .unknown_cause
            .expect("the cause travels with the Unknown");
        assert!(cause.contains(&format!("pid {pid}")), "{cause}");
        assert!(cause.contains("zero bytes"), "{cause}");
        assert_eq!(seen.pane_pid, Some(pid));
    }

    // A pane the agent was never asked about is the same could-not-tell.
    let unasked = observe_seat(&host.list(), &seat(), None, settled(&host));
    assert_eq!(unasked.state, RosterState::Unknown);
}

/// A live pane and a reading of it: Present, or PromptBlocked where the agent
/// says it is blocked, with the reading's activity carried as the seat's and
/// its session as the seat's.
#[test]
fn a_live_pane_and_its_row_is_present_with_the_status_as_activity() {
    let (host, pid) = hosting(&seat());
    let idle = observe_seat(
        &host.list(),
        &seat(),
        Some(&recorded_as(RECORDED_IDLE, &seat(), pid)),
        settled(&host),
    );
    assert_eq!(idle.state, RosterState::Present);
    assert_eq!(idle.activity, Some(Activity::Idle));
    assert_eq!(idle.session_id.as_deref(), Some(RECORDED_SESSION));
    assert_eq!(idle.pane_pid, Some(pid));
    assert_eq!(idle.exit_status, None);
    assert_eq!(idle.project.as_deref(), Some("demo"));

    let busy = observe_seat(
        &host.list(),
        &seat(),
        Some(&recorded_as(RECORDED_BUSY, &seat(), pid)),
        settled(&host),
    );
    assert_eq!(busy.state, RosterState::Present);
    assert_eq!(busy.activity, Some(Activity::Busy));

    let blocked = observe_seat(
        &host.list(),
        &seat(),
        Some(&recorded_as(RECORDED_WAITING, &seat(), pid)),
        settled(&host),
    );
    assert_eq!(blocked.state, RosterState::PromptBlocked);
    assert_eq!(blocked.waiting_for.as_deref(), Some("permission prompt"));

    // A row that says it waits and names no cause is blocked all the same: the
    // one answer a typed turn refuses on (`effect::type_turn`), so the
    // projection never calls present a seat a nudge would refuse.
    let causeless = observe_seat(
        &host.list(),
        &seat(),
        Some(&recorded_as(
            &RECORDED_WAITING.replace(r#","waitingFor":"permission prompt""#, ""),
            &seat(),
            pid,
        )),
        settled(&host),
    );
    assert_eq!(causeless.state, RosterState::PromptBlocked);
    assert_eq!(causeless.waiting_for.as_deref(), Some("status waiting"));

    // A session the agent FOUND whose activity it cannot say — a status word
    // it has no reading for, or a row listed before its status — is still a
    // session there, never an absence and never a fifth state.
    for activity in [Activity::Unknown, Activity::Starting] {
        let found = observe_seat(
            &host.list(),
            &seat(),
            Some(&reading(&seat(), activity, Some("aa"), None)),
            settled(&host),
        );
        assert_eq!(found.state, RosterState::Present, "{activity:?}");
        assert_eq!(found.session_id.as_deref(), Some("aa"));
    }
}

/// A live pane the agent names no session for YET: Starting, while the session
/// is younger than the grace — an interactive row was listed 0.5–0.75 s after
/// its session was made (2.1.280), and a poll can land inside that.
#[test]
fn a_young_live_pane_with_no_row_is_starting() {
    let (host, pid) = hosting(&seat());
    let seen = observe_seat(
        &host.list(),
        &seat(),
        Some(&read_off(RECORDED_GONE, &seat(), pid)),
        created(&host) + 500,
    );
    assert_eq!(seen.state, RosterState::Starting);
    assert_eq!(seen.pane_pid, Some(pid));
    assert_eq!(seen.unknown_cause, None);
}

/// The same pane past the grace, still unnamed: Unknown, saying the host holds
/// the pid alive and the agent names no session for it. The row with the
/// recording's own pid is somebody else's, and it changes nothing.
#[test]
fn an_older_live_pane_with_no_row_is_unknown_naming_the_pid() {
    let (host, pid) = hosting(&seat());
    let at_the_edge = created(&host) + STARTING_GRACE_MS - 1;
    assert_eq!(
        observe_seat(
            &host.list(),
            &seat(),
            Some(&read_off(RECORDED_GONE, &seat(), pid)),
            at_the_edge
        )
        .state,
        RosterState::Starting,
        "one millisecond inside the grace is still starting"
    );

    let seen = observe_seat(
        &host.list(),
        &seat(),
        Some(&read_off(RECORDED_IDLE, &seat(), pid)),
        created(&host) + STARTING_GRACE_MS,
    );
    assert_eq!(seen.state, RosterState::Unknown);
    assert_eq!(
        seen.unknown_cause.as_deref(),
        Some(
            format!(
                "tmux holds pid {pid} alive for {}; the listing names no row with pid {pid}",
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
    let read = read_off(
        &format!(
            r#"[{{"id":"short-id","sessionId":"a-session-id","cwd":"/wt/builder-1",
                 "kind":"background","pid":{pid},"status":"idle","startedAt":10}}]"#
        ),
        &seat(),
        pid,
    );
    let seen = observe_seat(&host.list(), &seat(), Some(&read), settled(&host));
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
    let unseen = observe_seat(&nothing_hosted(), &nameless, None, 2_000);

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
    let (host, pid) = hosting(&seat());
    let seen = observe_seat(
        &host.list(),
        &seat(),
        Some(&read_off("", &seat(), pid)),
        settled(&host),
    );
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
/// The match attributes nothing (the pid does); it is what names the project a
/// present seat's pane stands in.
#[test]
fn a_trailing_separator_on_either_side_is_the_same_directory() {
    let with_slash = format!("{WORKTREE}/");

    let mut slashed = seat();
    slashed.worktrees = vec![("demo".to_string(), with_slash.clone())];
    assert_eq!(
        present(&slashed, WORKTREE, "a-session").project.as_deref(),
        Some("demo"),
        "a configured worktree with one holds a pane standing in one without"
    );

    // And a pane reported with one stands in a worktree configured without.
    let host = FakeHost::new();
    host.new_session(
        &session_for(&seat().id),
        Path::new(&with_slash),
        &["/nowhere/agent".to_string()],
        &[],
    )
    .expect("the fake host starts the session");
    let read = reading(&seat(), Activity::Idle, Some("a-session"), None);
    let seen = observe_seat(&host.list(), &seat(), Some(&read), settled(&host));
    assert_eq!(seen.project.as_deref(), Some("demo"));
    assert_eq!(seen.worktree.as_deref(), Some(WORKTREE));

    assert_eq!(dir_key("/"), "/", "the root is left alone");

    // The control: a seat on two projects whose pane stands in NEITHER names
    // no project, so the two matches above are the normalisation's and not a
    // matcher that says yes to everything.
    let mut several = seat();
    several.worktrees = vec![
        ("demo".to_string(), "/wt/elsewhere".to_string()),
        ("other".to_string(), OTHER.to_string()),
    ];
    let (host, _) = hosting(&several);
    let seen = observe_seat(&host.list(), &several, Some(&read), settled(&host));
    assert_eq!(seen.state, RosterState::Present);
    assert_eq!(seen.project, None);
}

/// A seat no session answers for still names where it would be found — but
/// only when that is one place. Registered on several projects it has no one
/// answer, and the fields are absent rather than guessed.
#[test]
fn a_seat_on_several_projects_names_no_worktree_when_no_row_matches() {
    let mut several = seat();
    several.worktrees = vec![
        ("demo".to_string(), WORKTREE.to_string()),
        ("other".to_string(), OTHER.to_string()),
    ];
    let seen = observe_seat(&nothing_hosted(), &several, None, 2_000);
    assert_eq!(seen.state, RosterState::Absent);
    assert!(
        seen.project.is_none() && seen.worktree.is_none(),
        "several projects have no one answer, and a guess is worse than none"
    );

    // The control: one project, and the same absent seat names both.
    let seen = observe_seat(&nothing_hosted(), &seat(), None, 2_000);
    assert_eq!(seen.state, RosterState::Absent);
    assert_eq!(seen.project.as_deref(), Some("demo"));
    assert_eq!(seen.worktree.as_deref(), Some(WORKTREE));
}

fn projection(agent_version: Option<&str>, expected: Option<&str>) -> Projection {
    Projection {
        version: VERSION,
        generated_at: "2026-09-06T00:00:00Z".to_string(),
        controller_version: "0.1.0".to_string(),
        agent: AgentView {
            adapter: claude_code::NAME.to_string(),
            name: Some(claude_code::AGENT.to_string()),
            version: agent_version.map(str::to_string),
            expected: expected.map(str::to_string),
            postures: vec![Posture::Ask, Posture::Auto, Posture::Unattended],
        },
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

// ------------------------------------------------------ the projection's agent

/// E13 — the projection names the agent in words that name no vendor: which
/// adapter answers, which agent it drives at which version, the release it is
/// expected at and the postures it takes — and the two fields it replaces stay
/// as its mirrors until fleet-x93d.2, so `agent.version` IS `agent_version`
/// and `agent.expected` IS `agent_version_expected`.
///
/// Driven through the loop itself, one poll against a stub agent and a fake
/// host, because the mirrors are the loop's to fill: a fixture document would
/// only agree with itself.
#[test]
fn the_projections_agent_block_mirrors_the_version_fields_and_names_the_adapter() {
    let root = std::env::temp_dir().join(format!("fleet-observe-agent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let machine = root.join("machine");
    let worktree = root.join("wt");
    std::fs::create_dir_all(&worktree).expect("the worktree is made");
    std::fs::create_dir_all(&machine).expect("the machine directory is made");
    std::fs::write(root.join("fleet.toml"), "[controller]\npoll_seconds = 1\n")
        .expect("the policy is written");
    std::fs::write(
        machine.join("config.json"),
        format!(
            "{{\"fleet_toml\": \"{}\", \"children\": [\
             {{\"id\": \"{SEAT_ID}\", \"worktrees\": {{\"demo\": \"{}\"}}}}]}}\n",
            root.join("fleet.toml").display(),
            worktree.display()
        ),
    )
    .expect("the seat list is written");
    common::hermetic::export(common::hermetic::in_process_vars(&root, &machine, None));
    platform::clear_stop();

    let host = FakeHost::new();
    host.new_session(
        &session_for(&id(SEAT_ID)),
        &worktree,
        &["/nowhere/agent".to_string()],
        &[],
    )
    .expect("the fake host starts the seat's session");
    let stub = StubAgent::answering(Answers {
        listing: Ok(test_support::listing(&test_support::arrivals(1))),
        ..Answers::default()
    });
    let clock = FakeClock::new();
    assert_eq!(
        run::observe_seamed(
            &Options { once: true },
            Grant::new(platform::directory_listing(), Duration::from_secs(5)),
            None,
            Seams {
                clock: &clock,
                adapter: "an-adapter",
                agent: &stub,
                host: &host,
                daemon: None,
                child_path: "",
                effects_off: Some("this arm issues no effect".to_string()),
                stop_handler: StopHandler::Unarmed,
            },
        ),
        0
    );
    let body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(machine.join("projection.json"))
            .expect("a projection is published"),
    )
    .expect("the projection parses");
    let _ = std::fs::remove_dir_all(&root);

    let agent = &body["agent"];
    assert_eq!(agent["adapter"], "an-adapter", "{body}");
    assert_eq!(agent["name"], StubAgent::NAME);
    assert_eq!(agent["version"], StubAgent::VERSION);
    assert_eq!(
        agent["version"], body["agent_version"],
        "agent.version is agent_version"
    );
    assert_eq!(
        agent["expected"],
        fleet_core::supported::PINNED_CLAUDE_CODE,
        "a fleet that pins nothing expects what the adapter was measured against"
    );
    assert_eq!(
        agent["expected"], body["agent_version_expected"],
        "agent.expected is agent_version_expected"
    );
    assert_eq!(
        agent["postures"],
        serde_json::json!(["ask", "auto", "unattended"])
    );
    // And the seat it read is the host's live pane, found by its pid.
    assert_eq!(body["seats"][0]["roster_state"], "present", "{body}");
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
/// one twice — which is the mutant it exists to kill: a poll that asks about
/// every seat under the adapter's own directory.
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
    let agent = ByDirectory::new(|dir: Option<&str>| {
        Ok(match dir {
            None => format!("[{}]", live(WORKTREE, "named-session", named_pid)),
            Some(dir) => {
                assert_eq!(dir, per_row_dir, "the read is under the row's own");
                format!("[{}]", live(OTHER, "spawned-session", spawned_pid))
            }
        })
    });
    let held = |seat: &SeatId| Held {
        session_id: None,
        config_dir: (*seat == spawned.id).then(|| per_row_dir.to_string()),
    };
    let seen = observe_fleet(&agent, &host.list(), &seats, &held, at);
    assert_eq!(
        agent.asked.borrow().clone(),
        vec![None, Some(per_row_dir.to_string())],
        "ONE read, asking the fleet's directory and the row's own, once each"
    );
    assert_eq!(seen[1].state, RosterState::Present, "{:?}", seen[1]);
    assert_eq!(seen[1].session_id.as_deref(), Some("spawned-session"));
    // And the named seat is still decided against the fleet's, unchanged.
    assert_eq!(seen[0].state, RosterState::Present, "{:?}", seen[0]);
    assert_eq!(seen[0].session_id.as_deref(), Some("named-session"));

    // THE CONTROL, and the reason the arm is a measurement rather than a
    // restatement: the same seat asked about under the FLEET's directory is
    // named nothing for its live pane, so the pair differs and the directory
    // is what made the difference.
    let unscoped = observe_fleet(
        &agent,
        &host.list(),
        &seats,
        &|_: &SeatId| Held::default(),
        at,
    );
    assert_eq!(unscoped[1].state, RosterState::Unknown, "{:?}", unscoped[1]);
    assert_eq!(unscoped[1].session_id, None);
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
    let agent = ByDirectory::new(|dir: Option<&str>| match dir {
        None => Ok(format!("[{}]", live(WORKTREE, "named-session", named_pid))),
        Some(_) => Err("its listing's socket is unreachable".to_string()),
    });
    let held = |seat: &SeatId| Held {
        session_id: None,
        config_dir: (*seat == spawned.id).then(|| "/machine/config/builder-9".to_string()),
    };
    let seen = observe_fleet(&agent, &host.list(), &seats, &held, at);

    let blind = &seen[1];
    assert_eq!(blind.state, RosterState::Unknown, "{blind:?}");
    assert!(
        blind
            .unknown_cause
            .as_deref()
            .unwrap_or_default()
            .contains("socket is unreachable"),
        "the cause is carried: {blind:?}"
    );

    let decided = &seen[0];
    assert_eq!(decided.state, RosterState::Present, "{decided:?}");
    assert!(decided.unknown_cause.is_none());
}

/// The session a seat is asked about by is the one the table last sighted for
/// it, and the agent's answer is found by it before the pane's pid: a seat
/// whose held session the agent still has reads that session.
#[test]
fn a_seat_is_asked_about_by_the_session_the_table_holds_for_it() {
    let (host, pid) = hosting(&seat());
    let agent = ByDirectory::new(|_: Option<&str>| {
        Ok(format!(
            "[{}, {}]",
            live(WORKTREE, "held-session", 7),
            live(WORKTREE, "by-the-pid", pid)
        ))
    });
    let held = |_: &SeatId| Held {
        session_id: Some("held-session".to_string()),
        config_dir: None,
    };
    let seen = observe_fleet(
        &agent,
        &host.list(),
        std::slice::from_ref(&seat()),
        &held,
        settled(&host),
    );
    assert_eq!(seen[0].session_id.as_deref(), Some("held-session"));

    // The control: holding none, the pane's pid is what finds it.
    let seen = observe_fleet(
        &agent,
        &host.list(),
        std::slice::from_ref(&seat()),
        &|_: &SeatId| Held::default(),
        settled(&host),
    );
    assert_eq!(seen[0].session_id.as_deref(), Some("by-the-pid"));
}

// ------------------------------------------------------- the logged-out dispatch

/// AC3 — a transient seat the agent reads BLOCKED ON LOGGED_OUT yields exactly
/// ONE line, and a seat it reads any other way yields none.
///
/// "Exactly one" is the second poll: the first sighting writes the line and every
/// poll after it is already sighted, which is what stops a line per interval.
#[test]
fn a_logged_out_first_turn_yields_one_dispatch_failure_and_an_answered_one_yields_none() {
    let logged_out = Some(BlockedOn::LoggedOut);

    // The first sighting of a transient seat reading logged out.
    assert!(observe::logged_out_dispatch(
        true,
        RosterState::PromptBlocked,
        false,
        logged_out
    ));
    // The same seat on every later poll: already sighted, so nothing more.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::PromptBlocked,
        true,
        logged_out
    ));
    // A seat blocked on something else, one variable apart from the first case.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::PromptBlocked,
        false,
        Some(BlockedOn::Permission)
    ));
    // A NAMED seat's session is the person's own, whatever the agent says.
    assert!(!observe::logged_out_dispatch(
        false,
        RosterState::PromptBlocked,
        false,
        logged_out
    ));
    // A reading that names no reason is a seat not known to be logged out.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::Present,
        false,
        None
    ));
    // And the reading comes off a LIVE session: an absent seat has no first turn.
    assert!(!observe::logged_out_dispatch(
        true,
        RosterState::Absent,
        false,
        logged_out
    ));

    // The reading as the loop meets it: the adapter's blocked-on-logged-out
    // answer, laid beside a live pane, is the prompt-blocked seat the line is
    // written for, carrying what it waits on.
    let (host, _) = hosting(&seat());
    let mut answer = reading(&seat(), Activity::Blocked, Some("aa"), None);
    answer.blocked_on = logged_out;
    let seen = observe_seat(&host.list(), &seat(), Some(&answer), settled(&host));
    assert_eq!(seen.state, RosterState::PromptBlocked);
    assert_eq!(seen.blocked_on, logged_out);
    assert_eq!(seen.waiting_for.as_deref(), Some("logged_out"));
    assert!(observe::logged_out_dispatch(
        true,
        seen.state,
        false,
        seen.blocked_on
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
