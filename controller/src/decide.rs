//! One verdict per seat: a pure function from what one poll saw to what the
//! effects layer should do about it.
//!
//! Pure and total, with every term passed in. Nothing here reads a file, a
//! clock or the roster, so every arm of the table is reachable from a fixture
//! and the answer survives a controller restart: each term is read fresh by the
//! caller each poll and none of them is a restored belief.

use crate::observe::RosterState;

/// Consecutive blind dispatches tolerated before the seat is left down.
///
/// One miss is ordinary latency, two tolerates a slow poll, three means the loop
/// is not waiting on latency. A constant rather than a policy key: the number is
/// a property of the arrival-window design, and a fleet that could tune it down
/// to one would turn every slow arrival into a halt.
pub const BLIND_LIMIT: u32 = 3;

/// The six verdicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    LeaveAlone,
    SpawnWoken,
    Revive,
    Rest,
    SuggestRest,
    Halt,
}

impl Verdict {
    /// The published spelling. These six strings reach the projection, which is
    /// read by tools that are not this binary.
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::LeaveAlone => "leave-alone",
            Verdict::SpawnWoken => "spawn-woken",
            Verdict::Revive => "revive",
            Verdict::Rest => "rest",
            Verdict::SuggestRest => "suggest-rest",
            Verdict::Halt => "halt",
        }
    }

    /// Whether carrying this verdict out creates a session. The transient filter
    /// is defined on this predicate rather than on a list of arms, so a seventh
    /// verdict that creates one is covered by construction.
    ///
    /// `Rest` is one of the three: it is defined as stopping a live session and
    /// bringing a fresh woken successor up, so for a spawned seat it is a
    /// re-spawn wearing another name.
    pub fn creates_a_session(&self) -> bool {
        matches!(self, Verdict::SpawnWoken | Verdict::Revive | Verdict::Rest)
    }
}

/// What one poll knows about one seat. Every field is this poll's own reading.
#[derive(Clone, Debug)]
pub struct SeatInput<'a> {
    pub seat_dir: &'a str,
    pub state: RosterState,
    /// Set exactly when `state` is `Unknown`, and carried so the caller can log
    /// what it could not see rather than a bare verdict.
    pub unknown_cause: Option<&'a str>,
    /// A spawned seat: never revived, never rested, never succeeded.
    pub transient: bool,
    /// An unconsumed `seat.resting` stands for this seat.
    pub pending_rest: bool,
    /// An unconsumed `seat.resting` or `seat.exited` stands for this seat — the
    /// discriminator a dead pane cannot supply: a seat that said goodbye and
    /// one that crashed leave the same one.
    pub pending_deliberate_end: bool,
    /// `None` is a reading nobody took, which is not a reading of zero.
    pub context_tokens: Option<u64>,
    pub rest_threshold_tokens: u64,
    /// The session the seat is standing on, when there is one.
    pub session_id: Option<&'a str>,
    /// Whether this seat's session id is already in the nudged map.
    pub already_nudged: bool,
    /// How long ago the newest dispatch for this seat went out, in milliseconds.
    /// `None` is a seat this controller has dispatched for and never recorded,
    /// or one it has not dispatched for at all.
    pub dispatch_age_ms: Option<u64>,
    /// Whether that dispatch has been answered by a sighting. A sighting is the
    /// roster's answer and never the effect's own return (lessons claude-code
    /// A7, A14).
    pub sighted: bool,
    pub arrival_window_ms: u64,
    /// The halt latch and the blind count, both read from the session table this
    /// poll: the latch outlives a restart and the counter is what reaches it.
    pub halted: bool,
    pub blind: u32,
}

/// The verdict for one seat, with the transient filter over the table.
///
/// The filter is a WRAPPER and not three guards inside the table, which is its
/// whole value: it converts every session-creating verdict at one chokepoint,
/// so an arm added later is covered by [`Verdict::creates_a_session`] instead of
/// by whoever adds it remembering the rule. `leave-alone`, `halt` and
/// `suggest-rest` pass through untouched — a spawned seat is still observed,
/// still reported, and can still be told it is heavy; what it cannot be is
/// brought back.
pub fn decide(input: &SeatInput) -> Verdict {
    let verdict = decide_table(input);
    if input.transient && verdict.creates_a_session() {
        return Verdict::LeaveAlone;
    }
    verdict
}

/// The table. First match wins, and the order IS the policy.
fn decide_table(input: &SeatInput) -> Verdict {
    // 1. Cannot-see is never treated as empty. It outranks everything,
    //    the halt guard included: a controller that cannot read the fleet has no
    //    business acting on any belief about it.
    if input.state == RosterState::Unknown {
        return Verdict::LeaveAlone;
    }

    // 2. The halt guard outranks the spawn it guards, or it could never fire —
    //    the runaway IS the spawn arm taken repeatedly.
    if input.halted {
        return Verdict::Halt;
    }

    // 3. Rest outranks spawn. A seat that asked to shed context is answered
    //    before anything else it might be eligible for, and above the arrival
    //    hold below: a rest stops a LIVE session and brings a successor up, so
    //    it is the one path that frees a seat which asked for it, and a hold
    //    that outranked it would disable that path for a whole window.
    //
    //    A live session is the precondition and not a detail: the collection
    //    order starts at `stop`, and there is nothing to stop in a dead pane. A
    //    pending rest on one falls through to the discriminator below, where the
    //    same event reads as the deliberate end it is.
    if input.pending_rest && is_live(input.state) {
        return Verdict::Rest;
    }

    // 4. A dispatch is out and its window has not closed.
    //    Dispatching again here is the amplifier: the roster reads the same
    //    absence every poll, so without this a seat is dispatched once per poll
    //    for as long as arrival takes. A SIGHTING is what closes the window,
    //    because the start's own return cannot witness arrival.
    if !input.sighted && arrival_window_open(input) {
        return Verdict::LeaveAlone;
    }

    // 5. A dead pane: the seat's session ENDED, with the host holding its exit
    //    status. How it ended is not something the pane says — a seat that
    //    said goodbye and one that crashed leave the same dead pane — so the
    //    discriminator is an event the ending ritual wrote plus a context
    //    guard.
    if input.state == RosterState::Stopped {
        // 5a. The seat ended on purpose and wants a successor. Reviving here
        //     brings back a session that has already said goodbye: it would sit
        //     idle, read present from then on, and the successor the handoff
        //     exists to produce would never come up.
        if input.pending_deliberate_end {
            return Verdict::SpawnWoken;
        }
        // 5b. No such event and room left to work: bring the session back,
        //     context intact and no wake burned.
        // 5c. At or over the threshold is the one dangerous unmarked case — a
        //     revived at-ceiling session reads healthy on every board and can do
        //     no work — so it is spawned over instead. An UNMEASURABLE context
        //     falls here too: the rule licenses a revive on a measured
        //     under-threshold reading, and a term nobody measured is not one.
        return match input.context_tokens {
            Some(tokens) if tokens < input.rest_threshold_tokens => Verdict::Revive,
            _ => Verdict::SpawnWoken,
        };
    }

    // 6. A seat the host holds no session for. Nothing is being stood on, so
    //    arm 5's question does not arise — there is no pane to revive.
    if input.state == RosterState::Absent {
        return Verdict::SpawnWoken;
    }

    // 7. Suggested once per session and never enforced (lessons claude-code C5).
    //    Keying the dedupe on the session id re-arms it for a successor
    //    with no bookkeeping of its own.
    if is_live(input.state) && !input.already_nudged {
        if let Some(tokens) = input.context_tokens {
            if tokens >= input.rest_threshold_tokens {
                return Verdict::SuggestRest;
            }
        }
    }

    // A starting seat lands here: its pane is alive and the listing has not
    // named it yet, and spawning beside it is what puts two sessions in one
    // worktree.
    Verdict::LeaveAlone
}

/// A row holding a live session, whichever of the two live states it is in. A
/// seat stopped in front of a human holds a row and a pid; what it cannot do is
/// act.
fn is_live(state: RosterState) -> bool {
    matches!(state, RosterState::Present | RosterState::PromptBlocked)
}

/// Whether a dispatch already issued for this seat is still inside its window.
///
/// Keyed to the DISPATCH the controller recorded and never to the row's start
/// stamp: a revive targets a row that already exists, so that stamp is the
/// session's birth and says nothing about when this controller acted.
fn arrival_window_open(input: &SeatInput) -> bool {
    match input.dispatch_age_ms {
        Some(age) => age < input.arrival_window_ms,
        None => false,
    }
}

/// The blind counter after this poll.
///
/// Pure, and separate from [`decide`] because it is arithmetic over the same
/// observation rather than a verdict — but it is policy, so it is provable here
/// rather than buried in the effects layer. The counter moves ONCE PER WINDOW
/// and needs no rule of its own for that: this increments on a dispatch, and the
/// arrival hold is what makes a dispatch possible only once per window.
///
/// A sighting DECREMENTS by one and never clears. Clearing is right for a
/// permanently blind seat and wrong for an intermittent one: a flapping roster
/// would zero the count on each good poll and let the next failure start again
/// from zero, so a guard built for a stuck seat would never fire against a
/// flapping one.
pub fn blind_after(previous: u32, state: RosterState, verdict: Verdict) -> u32 {
    // A halt is LATCHED. At the limit a sighting would drop the count below it
    // and the next poll would dispatch again, so the halt would lift itself with
    // no operator act. Above the limit the only way down is a clear-halt.
    if previous >= BLIND_LIMIT {
        return previous;
    }
    match state {
        // Cannot-see is not a sighting, and must not decay the counter — that
        // would let an unreadable roster quietly launder a runaway back to zero.
        RosterState::Unknown => previous,
        // A blocked seat is a SIGHTING like any other live row: the session is
        // sitting right there and the dispatch plainly arrived. Counting it
        // blind would drive a stuck-but-visible seat to the limit and blame the
        // dispatcher for a session stopped in front of a human.
        RosterState::Present | RosterState::PromptBlocked => previous.saturating_sub(1),
        // A starting seat is neither a sighting nor a dispatch, so it moves the
        // counter in no direction. The guard stays reachable either way: the
        // session is listed within a poll or two and decays it, or its pane
        // dies, reads stopped, and takes an arm that increments.
        RosterState::Starting => previous,
        // Stopped counts exactly as Absent, and this is what bounds
        // spawn-over-stopped: a seat that keeps reading no live session after being
        // dispatched accumulates here and reaches the halt guard, whatever the
        // reason the dispatch did not take.
        //
        // A REVIVE COUNTS AS A DISPATCH, and that is load-bearing rather than
        // tidy: a resume that came up and died again, or never came up as the
        // session it resumed, leaves the seat reading stopped or absent once
        // more, so a revive that keeps failing would otherwise be retried every
        // poll forever and never reach the guard — a runaway that is quiet
        // instead of loud.
        RosterState::Stopped | RosterState::Absent => {
            if verdict.creates_a_session() {
                previous + 1
            } else {
                previous
            }
        }
    }
}

/// Seats with no session below which one poll is never the server-gone shape.
///
/// Half a roster of one or two is a single seat that is down, and announcing
/// that as a fleet-wide event would spend the line's whole meaning on the
/// smallest fleets.
pub const SHAPE_FLOOR: usize = 2;

/// The whole-fleet shape a poll is carrying, when it is carrying one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetShape {
    pub absent: usize,
    pub total: usize,
}

impl FleetShape {
    pub fn describe(&self) -> String {
        format!(
            "fleet: SERVER-GONE SHAPE — {} of {} configured seats have no session on the host in \
             ONE poll. Every seat absent at once is fleet's own tmux server gone — a reboot, or \
             the server ended — so the seat lines below are ONE event rather than {} \
             independent absences; each seat is still dispatched on its own terms",
            self.absent, self.total, self.absent
        )
    }
}

/// The threshold itself, over counts rather than states.
///
/// Split out so the live path and any other caller cannot drift: a rule stated
/// twice is a rule that disagrees with itself the first time either copy moves.
pub fn fleet_shape_from_counts(absent: usize, total: usize) -> Option<FleetShape> {
    if absent >= SHAPE_FLOOR && absent * 2 >= total {
        Some(FleetShape { absent, total })
    } else {
        None
    }
}

/// The one fleet-wide reading: pure, over the roster states this poll already
/// produced, and it changes NO seat's verdict.
///
/// Deliberately not threaded into [`decide`], which stays a pure function of one
/// seat's input: no arm of the table needs the fleet-level fact to reach the
/// right answer, because the arrival window already bounds each dispatch.
pub fn fleet_shape(states: &[RosterState]) -> Option<FleetShape> {
    fleet_shape_from_counts(
        states
            .iter()
            .filter(|state| **state == RosterState::Absent)
            .count(),
        states.len(),
    )
}
