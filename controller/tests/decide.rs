//! Fixture tests for the decision table.
//!
//! One arm per verdict and one per rule, over a pure function with every term
//! passed in — so each rule is reached by a case where that rule ALONE decides,
//! and an arm that would still be green with its rule deleted is not one.

use fleet_controller::decide::{
    blind_after, decide, fleet_shape, fleet_shape_from_counts, hold, rehost_hold, replacement_hold,
    SeatInput, Verdict, BLIND_LIMIT, REHOST_HELD, REPLACEMENT_HELD, UPGRADE_SHAPE_FLOOR,
};
use fleet_controller::observe::RosterState;

const THRESHOLD: u64 = 700_000;
const WINDOW_MS: u64 = 45_000;

/// A seat nothing is asking for and nothing has dispatched to: every term at the
/// reading that decides nothing, so an arm that moves ONE of them is measuring
/// that one.
fn quiet(state: RosterState) -> SeatInput<'static> {
    SeatInput {
        seat_dir: "s1",
        state,
        unknown_cause: None,
        transient: false,
        pending_rest: false,
        pending_deliberate_end: false,
        context_tokens: Some(1_000),
        rest_threshold_tokens: THRESHOLD,
        session_id: Some("a-session"),
        already_nudged: false,
        dispatch_age_ms: None,
        sighted: false,
        adopted_and_listed: false,
        arrival_window_ms: WINDOW_MS,
        halted: false,
        blind: 0,
        // Every daemon term at the reading that opens nothing: a poll whose
        // daemon read answered no pid and no age holds no seat, so an arm that
        // moves one of them is measuring that one.
        pidless_row: matches!(state, RosterState::Stopped),
        daemon_pid_changed: false,
        daemon_uptime_ms: None,
        // And a seat this controller has never seen live, standing in no run of
        // pid-less polls, which holds nothing either: the memory an arm about
        // the re-host hold sets for itself.
        seen_live: false,
        since_pidless_ms: None,
    }
}

/// The six verdicts, each from the input that produces it and no other.
#[test]
fn every_verdict_has_a_case_that_produces_it() {
    // leave-alone: a live seat under the threshold with nothing pending.
    assert_eq!(decide(&quiet(RosterState::Present)), Verdict::LeaveAlone);

    // spawn-woken: no row at all.
    assert_eq!(decide(&quiet(RosterState::Absent)), Verdict::SpawnWoken);

    // revive: a pid-less row, no deliberate end, context under the threshold.
    assert_eq!(decide(&quiet(RosterState::Stopped)), Verdict::Revive);

    // rest: a live row with an unconsumed `seat.resting`.
    let mut resting = quiet(RosterState::Present);
    resting.pending_rest = true;
    resting.pending_deliberate_end = true;
    assert_eq!(decide(&resting), Verdict::Rest);

    // suggest-rest: a live row at the threshold that has not been nudged.
    let mut heavy = quiet(RosterState::Present);
    heavy.context_tokens = Some(THRESHOLD);
    assert_eq!(decide(&heavy), Verdict::SuggestRest);

    // halt: the latch, which this slice never sets and the table still answers.
    let mut halted = quiet(RosterState::Absent);
    halted.halted = true;
    assert_eq!(decide(&halted), Verdict::Halt);
}

/// Unknown is not absent. A roster nobody could read reaches every seat
/// as Unknown, and acting on it is acting on a fleet nobody can see.
///
/// The control is the SAME input at `Absent`, which is the verdict Unknown would
/// take if the two were conflated.
#[test]
fn unknown_is_leave_alone_and_the_same_input_absent_is_a_spawn() {
    let mut unknown = quiet(RosterState::Unknown);
    unknown.unknown_cause = Some("the listing answered with zero bytes");
    assert_eq!(decide(&unknown), Verdict::LeaveAlone);

    let mut absent = unknown.clone();
    absent.state = RosterState::Absent;
    assert_eq!(
        decide(&absent),
        Verdict::SpawnWoken,
        "the control: only the state moved, and the same seat is spawned"
    );

    // And Unknown outranks the halt latch, which outranks everything else: a
    // controller that cannot read the fleet has no business acting on any belief
    // about it, its own latch included.
    unknown.halted = true;
    assert_eq!(decide(&unknown), Verdict::LeaveAlone);
}

/// The transient filter, on each of the three session-creating verdicts, with
/// the named seat's answer beside it as the control.
#[test]
fn a_transient_row_takes_no_session_creating_verdict_and_the_other_two_pass() {
    let mut resting = quiet(RosterState::Present);
    resting.pending_rest = true;
    let mut heavy = quiet(RosterState::Present);
    heavy.context_tokens = Some(THRESHOLD);
    let mut halted = quiet(RosterState::Absent);
    halted.halted = true;

    let cases = [
        (quiet(RosterState::Absent), Verdict::SpawnWoken),
        (quiet(RosterState::Stopped), Verdict::Revive),
        (resting, Verdict::Rest),
    ];
    for (named, expected) in cases {
        assert_eq!(
            decide(&named),
            expected,
            "the control: a named seat in this state takes {expected:?}"
        );
        let mut spawned = named.clone();
        spawned.transient = true;
        assert_eq!(
            decide(&spawned),
            Verdict::LeaveAlone,
            "a transient row takes no {expected:?}"
        );
    }

    // The two that pass through untouched: a spawned seat is still observed,
    // still reported, and can still be told it is heavy.
    for (mut passes, expected) in [(heavy, Verdict::SuggestRest), (halted, Verdict::Halt)] {
        passes.transient = true;
        assert_eq!(decide(&passes), expected);
    }
}

/// Rest outranks spawn: a seat that asked to shed context is answered before
/// anything else it is eligible for.
///
/// The case is one where BOTH arms are otherwise live — a live row carrying a
/// pending rest that is ALSO at the threshold, so the suggest arm below would
/// fire — and the spawn half is the same event on a pid-less row, which is where
/// the discriminator sends it instead.
#[test]
fn rest_outranks_spawn_and_spawn_outranks_suggest() {
    let mut both = quiet(RosterState::Present);
    both.pending_rest = true;
    both.pending_deliberate_end = true;
    both.context_tokens = Some(THRESHOLD);
    assert_eq!(
        decide(&both),
        Verdict::Rest,
        "the rest is answered before the suggestion it is also due"
    );

    // Spawn over suggest: an ABSENT seat cannot be nudged — there is no session
    // to nudge — so the reading that separates them is a seat whose context is
    // at the threshold and whose row is gone.
    let mut gone_and_heavy = quiet(RosterState::Absent);
    gone_and_heavy.context_tokens = Some(THRESHOLD);
    assert_eq!(decide(&gone_and_heavy), Verdict::SpawnWoken);

    // The control on that pair: the same heaviness on a LIVE row is the
    // suggestion, so the answer above is the state's and not the threshold's.
    let mut live_and_heavy = gone_and_heavy.clone();
    live_and_heavy.state = RosterState::Present;
    assert_eq!(decide(&live_and_heavy), Verdict::SuggestRest);
}

/// The halt guard outranks the spawn it guards, or it could never fire — the
/// runaway IS the spawn arm taken repeatedly.
#[test]
fn the_halt_guard_outranks_the_spawn_it_guards() {
    let mut latched = quiet(RosterState::Absent);
    latched.halted = true;
    assert_eq!(decide(&latched), Verdict::Halt);

    // The control: the same absent seat with the latch off is the spawn the
    // guard exists to stop.
    latched.halted = false;
    assert_eq!(decide(&latched), Verdict::SpawnWoken);
}

/// The arrival hold: a dispatch that has gone out and has not been answered by
/// a sighting holds the seat, whatever the roster says.
///
/// Three readings, because the rule has three terms and each alone releases it:
/// the window closing, a sighting arriving, and there being no dispatch at all.
#[test]
fn a_dispatch_inside_its_window_with_no_sighting_holds_the_seat() {
    let mut waiting = quiet(RosterState::Absent);
    waiting.dispatch_age_ms = Some(WINDOW_MS - 1);
    assert_eq!(
        decide(&waiting),
        Verdict::LeaveAlone,
        "the seat reads absent and is not dispatched to again"
    );

    let mut closed = waiting.clone();
    closed.dispatch_age_ms = Some(WINDOW_MS);
    assert_eq!(
        decide(&closed),
        Verdict::SpawnWoken,
        "the window is closed at exactly the window, not one tick later"
    );

    let mut answered = waiting.clone();
    answered.sighted = true;
    assert_eq!(
        decide(&answered),
        Verdict::SpawnWoken,
        "a sighting answers the window, so the seat is eligible again"
    );

    let mut never = waiting.clone();
    never.dispatch_age_ms = None;
    assert_eq!(decide(&never), Verdict::SpawnWoken);

    // And it holds a STOPPED row too, which is the "whatever the roster says"
    // half: without the hold this input takes the discriminator's revive.
    let mut stopped = waiting.clone();
    stopped.state = RosterState::Stopped;
    assert_eq!(decide(&stopped), Verdict::LeaveAlone);

    // The one thing the hold does NOT outrank: a rest the seat asked for. A
    // hold above it would disable, for a whole window, the one path that frees
    // a seat which asked to shed context — so the ordering is pinned by the
    // case where both rules are live at once and only one can answer.
    let mut asked = waiting.clone();
    asked.state = RosterState::Present;
    asked.pending_rest = true;
    asked.pending_deliberate_end = true;
    assert_eq!(decide(&asked), Verdict::Rest);

    // The control on that pair: the same open window with no rest asked for is
    // the hold, so the Rest above is the event's and not the window's absence.
    let mut unasked = asked.clone();
    unasked.pending_rest = false;
    unasked.pending_deliberate_end = false;
    unasked.context_tokens = Some(THRESHOLD);
    assert_eq!(
        decide(&unasked),
        Verdict::LeaveAlone,
        "a live row inside an unanswered window is held, suggestion included"
    );
}

/// The discriminator for a pid-less row that is not a newborn, all three arms.
///
/// The roster cannot tell a hibernated session from a deliberately stopped one
/// (lessons claude-code A3), so the answer comes from an event and a context
/// guard — and the third arm is the one that makes the fall dangerous: a revived
/// at-ceiling session reads healthy on every board and can do no work.
#[test]
fn a_stopped_row_is_read_by_its_end_event_and_then_by_its_context() {
    let mut ended = quiet(RosterState::Stopped);
    ended.pending_deliberate_end = true;
    assert_eq!(
        decide(&ended),
        Verdict::SpawnWoken,
        "the seat said goodbye and wants a successor, not its own session back"
    );

    let under = quiet(RosterState::Stopped);
    assert_eq!(decide(&under), Verdict::Revive);

    let mut at_ceiling = quiet(RosterState::Stopped);
    at_ceiling.context_tokens = Some(THRESHOLD);
    assert_eq!(
        decide(&at_ceiling),
        Verdict::SpawnWoken,
        "at the threshold, not merely over it"
    );

    let mut unmeasurable = quiet(RosterState::Stopped);
    unmeasurable.context_tokens = None;
    assert_eq!(
        decide(&unmeasurable),
        Verdict::SpawnWoken,
        "a term nobody measured is not a measured under-threshold reading"
    );

    // The edge on the other side, so the comparison is `<` and not `<=`.
    let mut just_under = quiet(RosterState::Stopped);
    just_under.context_tokens = Some(THRESHOLD - 1);
    assert_eq!(decide(&just_under), Verdict::Revive);
}

/// One nudge per session, keyed on the session id.
///
/// The second reading is the same seat at the same weight with the flag set, and
/// the third is that flag cleared — which is what a NEW session id does, since
/// the caller keys the map on the id rather than on the seat.
#[test]
fn a_session_already_nudged_is_never_nudged_again() {
    let mut heavy = quiet(RosterState::Present);
    heavy.context_tokens = Some(THRESHOLD);
    assert_eq!(decide(&heavy), Verdict::SuggestRest);

    let mut again = heavy.clone();
    again.already_nudged = true;
    assert_eq!(decide(&again), Verdict::LeaveAlone);

    let mut successor = again.clone();
    successor.session_id = Some("another-session");
    successor.already_nudged = false;
    assert_eq!(
        decide(&successor),
        Verdict::SuggestRest,
        "a new session re-arms the one nudge with no bookkeeping of its own"
    );

    // Under the threshold is no suggestion whatever the flag says, so the arm
    // above is the threshold's and not the flag's alone.
    let mut light = heavy.clone();
    light.context_tokens = Some(THRESHOLD - 1);
    assert_eq!(decide(&light), Verdict::LeaveAlone);
}

/// A starting row is left alone: it is the only row about to become live, and
/// spawning beside it is what puts two live rows in one worktree.
///
/// The control is the same input at `Absent`, which is a spawn — so this arm
/// reads the state and not a table that answers leave-alone for everything.
#[test]
fn a_starting_row_is_left_alone_and_the_same_seat_absent_is_spawned() {
    assert_eq!(decide(&quiet(RosterState::Starting)), Verdict::LeaveAlone);
    assert_eq!(decide(&quiet(RosterState::Absent)), Verdict::SpawnWoken);

    // And it is left alone AT THE THRESHOLD too, which is the one reading the
    // suggest arm's live guard holds on its own: every other state that reaches
    // that arm is live, so a guard-less arm would nudge a session that has not
    // finished coming up.
    let mut newborn_and_heavy = quiet(RosterState::Starting);
    newborn_and_heavy.context_tokens = Some(THRESHOLD);
    assert_eq!(decide(&newborn_and_heavy), Verdict::LeaveAlone);

    // The control: the same reading on a live row IS the suggestion, so the
    // answer above is the state's and not a table that suggests nothing.
    let mut live_and_heavy = newborn_and_heavy.clone();
    live_and_heavy.state = RosterState::Present;
    assert_eq!(decide(&live_and_heavy), Verdict::SuggestRest);
}

/// A seat stopped in front of a human holds a live row and cannot act. It is
/// never spawned over, and it can still be told it is heavy — a suggestion is
/// the only thing that reaches it — but a rest it asked for still outranks that.
#[test]
fn a_prompt_blocked_row_holds_its_session_and_still_takes_a_rest() {
    assert_eq!(
        decide(&quiet(RosterState::PromptBlocked)),
        Verdict::LeaveAlone
    );

    let mut heavy = quiet(RosterState::PromptBlocked);
    heavy.context_tokens = Some(THRESHOLD);
    assert_eq!(decide(&heavy), Verdict::SuggestRest);

    let mut asked = heavy.clone();
    asked.pending_rest = true;
    assert_eq!(
        decide(&asked),
        Verdict::Rest,
        "the rest is the one path that frees a blocked seat, so it outranks the nudge"
    );
}

/// A `seat.resting` for a seat whose session has ALREADY gone is not a rest.
///
/// A rest is defined as stopping a live session and bringing a successor up, and
/// there is nothing to stop on a pid-less row — so the same event reads through
/// the discriminator as the deliberate end it is, and the seat gets a successor
/// rather than a stop aimed at nothing.
#[test]
fn a_rest_whose_session_is_already_gone_becomes_the_successor_it_asked_for() {
    let mut gone = quiet(RosterState::Stopped);
    gone.pending_rest = true;
    gone.pending_deliberate_end = true;
    assert_eq!(decide(&gone), Verdict::SpawnWoken);

    // The control: the same event on a live row is the rest itself.
    let mut live = gone.clone();
    live.state = RosterState::Present;
    assert_eq!(decide(&live), Verdict::Rest);
}

/// The replacement window: a pid-less row is HELD while the daemon is
/// mid-replacement, and dispatched against once the window closes.
///
/// Two readings open it and either alone does — an uptime under the arrival
/// window, and a pid that moved — because a poll loop stalled or slept across
/// the replacement arrives after the uptime half has already elapsed. Each is
/// set alone here, against a fixture where every other term is at the reading
/// that decides nothing.
#[test]
fn a_pidless_row_is_held_while_the_daemon_is_being_replaced() {
    // The control FIRST: the same row with the daemon quiet is the revive the
    // hold displaces, so the arm below reads the window and not the state.
    let quiet_daemon = quiet(RosterState::Stopped);
    assert_eq!(decide(&quiet_daemon), Verdict::Revive);
    assert_eq!(replacement_hold(&quiet_daemon), None);

    let mut young = quiet_daemon.clone();
    young.daemon_uptime_ms = Some(WINDOW_MS - 1);
    assert_eq!(decide(&young), Verdict::LeaveAlone);
    let why = replacement_hold(&young).expect("the hold names itself");
    assert!(why.starts_with(REPLACEMENT_HELD), "{why}");
    assert!(
        why.contains("under the 45s arrival window"),
        "the hold says which reading opened it: {why}"
    );

    // The window closes on the uptime alone, at the boundary the arrival window
    // fixes: AT the window it is no longer under it.
    let mut aged = young.clone();
    aged.daemon_uptime_ms = Some(WINDOW_MS);
    assert_eq!(decide(&aged), Verdict::Revive);

    // The other half, with the uptime unreadable: a pid that moved holds the
    // row on its own.
    let mut moved = quiet_daemon.clone();
    moved.daemon_pid_changed = true;
    assert_eq!(decide(&moved), Verdict::LeaveAlone);
    assert!(replacement_hold(&moved)
        .expect("the pid half holds too")
        .contains("changed pid"));

    // An unreadable daemon opens NOTHING on its own: both terms at the reading
    // nobody took is the quiet fixture above, which revives.
    let mut unreadable = quiet_daemon.clone();
    unreadable.daemon_uptime_ms = None;
    unreadable.daemon_pid_changed = false;
    assert_eq!(decide(&unreadable), Verdict::Revive);
}

/// Where the hold sits in the table: under the pending-rest arm and the arrival
/// hold, over the Stopped and Absent spawn arms.
///
/// Each pair differs in ONE term from a case the hold would otherwise take, so
/// an arm moved up or down the table reds exactly here.
#[test]
fn the_replacement_hold_sits_under_rest_and_over_the_spawn_arms() {
    // Over arm 6: an Absent seat whose only rows aged out is held, and the same
    // seat with no row at all is spawned — which is the distinction the hold is
    // defined on, since nothing is being stood on there to duplicate.
    let mut aged_out = quiet(RosterState::Absent);
    aged_out.pidless_row = true;
    aged_out.daemon_pid_changed = true;
    assert_eq!(decide(&aged_out), Verdict::LeaveAlone);

    let mut no_row = aged_out.clone();
    no_row.pidless_row = false;
    assert_eq!(
        decide(&no_row),
        Verdict::SpawnWoken,
        "a seat standing on no row has no duplicate to make"
    );

    // Over arm 5: the same held row would otherwise be spawned over, not merely
    // revived — the deliberate-end arm is held too.
    let mut ended = quiet(RosterState::Stopped);
    ended.pending_deliberate_end = true;
    ended.daemon_pid_changed = true;
    assert_eq!(decide(&ended), Verdict::LeaveAlone);

    // Under the rest arm: a rest on a LIVE row outranks it. The hold does not
    // reach a live row at all, so the pair that separates them is a live row
    // whose rest is answered through an open window.
    let mut resting = quiet(RosterState::Present);
    resting.pending_rest = true;
    resting.daemon_pid_changed = true;
    assert_eq!(
        decide(&resting),
        Verdict::Rest,
        "a seat that asked to shed context is answered through the window"
    );

    // Under the arrival hold: both answer leave-alone, so the reading that tells
    // them apart is the hold's own reason, which the arrival case does not
    // produce.
    let mut waiting = quiet(RosterState::Stopped);
    waiting.dispatch_age_ms = Some(WINDOW_MS - 1);
    assert_eq!(decide(&waiting), Verdict::LeaveAlone);
    assert_eq!(
        replacement_hold(&waiting),
        None,
        "the arrival hold is not the replacement one, whatever they publish"
    );
}

/// THE RE-HOST HOLD: a row this controller saw LIVE on an earlier poll and
/// reads pid-less on this one is held for the arrival window, not revived.
///
/// Replacing one session's host leaves the daemon untouched, so every term the
/// replacement hold reads is quiet and the row is the ordinary revive shape.
/// The controller's own earlier sighting is the whole of the evidence, so each
/// pair below moves that ONE term against a fixture the revive arm otherwise
/// takes.
#[test]
fn a_row_seen_live_under_this_controller_is_held_while_its_host_is_replaced() {
    // The control FIRST, and it is the recorded defect: a seat this controller
    // has never seen live takes the revive, which is what the held seat below
    // would have got.
    let never_seen = quiet(RosterState::Stopped);
    assert_eq!(decide(&never_seen), Verdict::Revive);
    assert_eq!(rehost_hold(&never_seen), None);

    let mut re_hosting = never_seen.clone();
    re_hosting.seen_live = true;
    re_hosting.since_pidless_ms = Some(WINDOW_MS - 1);
    assert_eq!(decide(&re_hosting), Verdict::LeaveAlone);
    let why = rehost_hold(&re_hosting).expect("the hold names itself");
    assert!(why.starts_with(REHOST_HELD), "{why}");
    assert!(
        why.contains(
            "s1's row carried a live session under this controller and has read pid-less 44s \
             after it did"
        ),
        "the hold names the seat, what its row reads and how long it has: {why}"
    );

    // The window closes at the arrival window and not a millisecond later: AT
    // the window the pid-less run is no longer inside it, and the revive is
    // taken exactly once, because the dispatch it issues opens the arrival hold.
    let mut aged = re_hosting.clone();
    aged.since_pidless_ms = Some(WINDOW_MS);
    assert_eq!(decide(&aged), Verdict::Revive);
    assert_eq!(rehost_hold(&aged), None);
    let mut dispatched = aged.clone();
    dispatched.dispatch_age_ms = Some(0);
    assert_eq!(
        decide(&dispatched),
        Verdict::LeaveAlone,
        "the poll after the revive waits on the arrival window it opened"
    );

    // A row that vanished from the listing entirely is the same transit: the
    // spawn arm would put a second session in a worktree that holds one.
    let mut unlisted = quiet(RosterState::Absent);
    assert_eq!(decide(&unlisted), Verdict::SpawnWoken);
    unlisted.seen_live = true;
    unlisted.since_pidless_ms = Some(WINDOW_MS - 1);
    assert_eq!(decide(&unlisted), Verdict::LeaveAlone);
    assert!(rehost_hold(&unlisted)
        .expect("an unlisted row is held too")
        .contains("has stood on no row the listing answers for"));

    // A DELIBERATE END IS NOT HELD, which is where this hold parts from the
    // replacement one above: the seat's own ending event says the row is
    // finished, so the successor is not delayed a window for it.
    let mut ended = re_hosting.clone();
    ended.pending_deliberate_end = true;
    assert_eq!(decide(&ended), Verdict::SpawnWoken);
    assert_eq!(rehost_hold(&ended), None);
    let mut replaced = ended.clone();
    replaced.daemon_pid_changed = true;
    assert_eq!(
        decide(&replaced),
        Verdict::LeaveAlone,
        "the daemon's replacement holds a deliberate end, and this hold does not"
    );
}

/// THE HOLD'S WINDOW IS SPENT ON THE PID-LESS RUN, NOT ON THE SIGHTING, and the
/// two terms are asked separately — the reading the poll after a run's land step turns on.
///
/// The controller's poll gaps are not the poll interval: a run executes on the
/// polling thread, so the poll after a twenty-minute land step carries a live
/// sighting twenty minutes old for every seat on the fleet while the rows it
/// reads went pid-less inside that gap. A window measured from the sighting is
/// closed on the very poll the hold exists for, which is the incident. So the
/// arm drives the pair the loop cannot: the same seat, the same fresh transit,
/// with and without the sighting that licenses a hold at all.
#[test]
fn a_pid_less_run_is_held_from_the_poll_it_opened_and_needs_a_sighting_to_be_held_at_all() {
    // THE POLL THE ROW WENT PID-LESS ON, reached after a gap of any length: the
    // run is 0ms old because this is the poll that opened it.
    let mut opened = quiet(RosterState::Stopped);
    opened.seen_live = true;
    opened.since_pidless_ms = Some(0);
    assert_eq!(decide(&opened), Verdict::LeaveAlone);
    assert!(rehost_hold(&opened)
        .expect("the poll that opens the run holds it")
        .contains("has read pid-less 0s after it did"));

    // THE SAME TRANSIT WITH NO SIGHTING BEHIND IT is not held: a row this
    // controller never saw live is not in transit FROM anything, and it is the
    // control that keeps the term above from standing in for both halves.
    let mut unsighted = opened.clone();
    unsighted.seen_live = false;
    assert_eq!(decide(&unsighted), Verdict::Revive);
    assert_eq!(rehost_hold(&unsighted), None);

    // And a seat that HAS been seen live but stands in no pid-less run at all
    // — the term nobody read — holds nothing either.
    let mut no_run = opened.clone();
    no_run.since_pidless_ms = None;
    assert_eq!(decide(&no_run), Verdict::Revive);
    assert_eq!(rehost_hold(&no_run), None);
}

/// The two holds are told apart by the line each prints, and a seat inside both
/// windows is described by the daemon's, which explains the whole poll.
#[test]
fn a_held_seats_line_names_which_hold_and_the_daemons_is_the_one_read_first() {
    let mut both = quiet(RosterState::Stopped);
    both.seen_live = true;
    both.since_pidless_ms = Some(WINDOW_MS - 1);
    both.daemon_uptime_ms = Some(WINDOW_MS - 1);
    let why = hold(&both).expect("a held seat has a reason");
    assert!(why.starts_with(REPLACEMENT_HELD), "{why}");
    assert!(
        rehost_hold(&both).is_some(),
        "and the other hold stands too, unread"
    );

    // The daemon quiet, the sighting fresh: the other line, from the same
    // function, so the verdict and the log cannot name different holds.
    let mut re_hosting = both.clone();
    re_hosting.daemon_uptime_ms = None;
    assert!(hold(&re_hosting)
        .expect("the re-host hold answers when the daemon's does not")
        .starts_with(REHOST_HELD));

    // Neither: the quiet fixture, which is revived and holds no line at all.
    assert_eq!(hold(&quiet(RosterState::Stopped)), None);
}

/// Every transition `blind_after` has, each moved by one term.
#[test]
fn the_blind_counter_decays_on_a_sighting_and_climbs_on_a_dispatch() {
    // A sighting decrements by one and never clears: a flapping roster that
    // cleared the count on each good poll would never reach the guard.
    assert_eq!(
        blind_after(2, RosterState::Present, Verdict::LeaveAlone),
        1,
        "a sighting decays the count by one"
    );
    assert_eq!(
        blind_after(2, RosterState::PromptBlocked, Verdict::LeaveAlone),
        1,
        "a seat stopped in front of a human is sitting right there"
    );
    assert_eq!(blind_after(0, RosterState::Present, Verdict::LeaveAlone), 0);

    // An unreadable roster HOLDS: it is not a sighting, and decaying on it would
    // let a broken read launder a runaway back to zero.
    assert_eq!(blind_after(2, RosterState::Unknown, Verdict::LeaveAlone), 2);

    // A starting row is neither a sighting nor a dispatch.
    assert_eq!(
        blind_after(2, RosterState::Starting, Verdict::SpawnWoken),
        2
    );

    // A pid-less row under a session-creating verdict is a dispatch, and a
    // REVIVE counts as one: an attach exits 0 whether it took or not.
    assert_eq!(blind_after(0, RosterState::Absent, Verdict::SpawnWoken), 1);
    assert_eq!(blind_after(1, RosterState::Stopped, Verdict::Revive), 2);
    assert_eq!(
        blind_after(1, RosterState::Stopped, Verdict::LeaveAlone),
        1,
        "a poll that dispatched nothing counts nothing"
    );

    // The latch: at the limit a sighting does not drop the count below it, or
    // the halt would lift itself with no operator act.
    assert_eq!(
        blind_after(BLIND_LIMIT, RosterState::Present, Verdict::LeaveAlone),
        BLIND_LIMIT
    );
    assert_eq!(
        blind_after(BLIND_LIMIT, RosterState::Absent, Verdict::SpawnWoken),
        BLIND_LIMIT,
        "and it does not climb past the limit either"
    );
    // The control on the latch: one below it, the same sighting decays.
    assert_eq!(
        blind_after(BLIND_LIMIT - 1, RosterState::Present, Verdict::LeaveAlone),
        BLIND_LIMIT - 2
    );
}

/// Three consecutive blind dispatches reach the limit, and the next verdict is
/// the halt — the guard's arm above, driven by the counter here.
#[test]
fn three_blind_dispatches_reach_the_limit_and_the_next_verdict_is_the_halt() {
    let mut seat = quiet(RosterState::Absent);
    let mut blind = 0;
    for _ in 0..BLIND_LIMIT {
        let verdict = decide(&seat);
        assert_eq!(verdict, Verdict::SpawnWoken);
        blind = blind_after(blind, seat.state, verdict);
    }
    assert_eq!(blind, BLIND_LIMIT);

    seat.halted = blind >= BLIND_LIMIT;
    assert_eq!(decide(&seat), Verdict::Halt);
}

/// The upgrade shape: half the seats or more on pid-less rows, and never
/// fewer than two.
#[test]
fn the_upgrade_shape_is_half_the_fleet_and_never_fewer_than_two() {
    // The floor: one of one is half a fleet by the fraction and is a single
    // hibernation by the count.
    assert_eq!(fleet_shape_from_counts(1, 1), None);
    assert_eq!(fleet_shape_from_counts(UPGRADE_SHAPE_FLOOR, 5), None);
    assert_eq!(
        fleet_shape_from_counts(2, 4).map(|shape| (shape.pidless, shape.total)),
        Some((2, 4)),
        "two of four is half, and two is the floor"
    );
    assert_eq!(fleet_shape_from_counts(3, 7), None, "three of seven is not");
    assert!(fleet_shape_from_counts(4, 7).is_some());

    // And over the states a poll produces: a live row is not pid-less, and
    // neither is a starting one — the shape is about sessions that STOPPED.
    assert_eq!(
        fleet_shape(&[
            RosterState::Stopped,
            RosterState::Absent,
            RosterState::Present,
            RosterState::Starting,
        ])
        .map(|shape| (shape.pidless, shape.total)),
        Some((2, 4))
    );
    assert_eq!(
        fleet_shape(&[
            RosterState::Stopped,
            RosterState::Present,
            RosterState::Present,
            RosterState::Present,
        ]),
        None
    );
}
