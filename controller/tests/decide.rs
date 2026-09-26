//! Fixture tests for the decision table.
//!
//! One arm per verdict and one per rule, over a pure function with every term
//! passed in — so each rule is reached by a case where that rule ALONE decides,
//! and an arm that would still be green with its rule deleted is not one.

use fleet_controller::decide::{
    blind_after, decide, fleet_shape, fleet_shape_from_counts, SeatInput, Verdict, BLIND_LIMIT,
    SHAPE_FLOOR,
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
        arrival_window_ms: WINDOW_MS,
        halted: false,
        blind: 0,
    }
}

/// The six verdicts, each from the input that produces it and no other.
#[test]
fn every_verdict_has_a_case_that_produces_it() {
    // leave-alone: a live seat under the threshold with nothing pending.
    assert_eq!(decide(&quiet(RosterState::Present)), Verdict::LeaveAlone);

    // spawn-woken: no row at all.
    assert_eq!(decide(&quiet(RosterState::Absent)), Verdict::SpawnWoken);

    // revive: a dead pane, no deliberate end, context under the threshold.
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
/// fire — and the spawn half is the same event on a dead pane, which is where
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

/// The discriminator for a dead pane, all three arms.
///
/// A dead pane cannot tell a crashed session from a deliberately ended one, so
/// the answer comes from an event and a context guard — and the third arm is
/// the one that makes the fall dangerous: a revived at-ceiling session reads
/// healthy on every board and can do no work.
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
/// there is nothing to stop in a dead pane — so the same event reads through
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

/// A DEAD PANE IS AN END, and nothing holds it: the session a host keeps cannot
/// hibernate or be re-hosted under it, so no poll waits for it to come back.
/// The stopped seat that no event and no ceiling covers is revived on the
/// first poll it reads dead, whatever a claim on its session said — the
/// daemon-era holds (lessons claude-code A10, and the claim of A3) answered
/// behaviour a pane cannot have.
#[test]
fn a_dead_pane_is_decided_on_the_poll_it_reads_dead() {
    assert_eq!(decide(&quiet(RosterState::Stopped)), Verdict::Revive);

    // And the arrival hold still stands over it: a dispatch out and unanswered
    // holds a dead pane as it holds an absent seat.
    let mut dispatched = quiet(RosterState::Stopped);
    dispatched.dispatch_age_ms = Some(WINDOW_MS - 1);
    assert_eq!(decide(&dispatched), Verdict::LeaveAlone);
    dispatched.dispatch_age_ms = Some(WINDOW_MS);
    assert_eq!(decide(&dispatched), Verdict::Revive);
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

    // A dead pane under a session-creating verdict is a dispatch, and a
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

/// The server-gone shape: half the seats or more with no session on the host,
/// and never fewer than two.
#[test]
fn the_server_gone_shape_is_half_the_fleet_and_never_fewer_than_two() {
    // The floor: one of one is half a fleet by the fraction and is a single
    // seat down by the count.
    assert_eq!(fleet_shape_from_counts(1, 1), None);
    assert_eq!(fleet_shape_from_counts(SHAPE_FLOOR, 5), None);
    assert_eq!(
        fleet_shape_from_counts(2, 4).map(|shape| (shape.absent, shape.total)),
        Some((2, 4)),
        "two of four is half, and two is the floor"
    );
    assert_eq!(fleet_shape_from_counts(3, 7), None, "three of seven is not");
    assert!(fleet_shape_from_counts(4, 7).is_some());

    // And over the states a poll produces: only a seat with NO session on the
    // host counts. A dead pane is a session the server still holds, so a
    // fleet of dead panes is a fleet whose server is up — and a live or a
    // starting seat is no absence at all.
    assert_eq!(
        fleet_shape(&[
            RosterState::Absent,
            RosterState::Absent,
            RosterState::Present,
            RosterState::Starting,
        ])
        .map(|shape| (shape.absent, shape.total)),
        Some((2, 4))
    );
    assert_eq!(
        fleet_shape(&[
            RosterState::Stopped,
            RosterState::Stopped,
            RosterState::Absent,
            RosterState::Present,
        ]),
        None,
        "dead panes are not the server gone"
    );
    let shape = fleet_shape(&[RosterState::Absent, RosterState::Absent])
        .expect("every seat absent is the shape");
    assert!(
        shape.describe().contains("2 of 2") && shape.describe().contains("tmux server gone"),
        "{}",
        shape.describe()
    );
}
