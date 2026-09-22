//! One verdict per seat (PRD R9, R12, R13): a pure function from what one poll
//! saw to what the effects layer should do about it.
//!
//! Pure and total, with every term passed in. Nothing here reads a file, a
//! clock or the roster, so every arm of the table is reachable from a fixture
//! and the answer survives a controller restart: each term is read fresh by the
//! caller each poll and none of them is a restored belief.

use crate::observe::RosterState;

/// Consecutive blind dispatches tolerated before the seat is left down (R14).
///
/// One miss is ordinary latency, two tolerates a slow poll, three means the loop
/// is not waiting on latency. A constant rather than a policy key: the number is
/// a property of the arrival-window design, and a fleet that could tune it down
/// to one would turn every slow arrival into a halt.
pub const BLIND_LIMIT: u32 = 3;

/// The six verdicts (PRD § Observe, decide, effect, publish).
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
    /// verdict that creates one is covered by construction (PRD R12).
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
    /// discriminator the roster cannot supply (lessons claude-code A3).
    pub pending_deliberate_end: bool,
    /// `None` is a reading nobody took, which is not a reading of zero.
    pub context_tokens: Option<u64>,
    pub rest_threshold_tokens: u64,
    /// The session the seat is standing on, when there is one.
    pub session_id: Option<&'a str>,
    /// Whether this seat's session id is already in the nudged map (R21).
    pub already_nudged: bool,
    /// How long ago the newest dispatch for this seat went out, in milliseconds.
    /// `None` is a seat this controller has dispatched for and never recorded,
    /// or one it has not dispatched for at all.
    pub dispatch_age_ms: Option<u64>,
    /// Whether that dispatch has been answered by a sighting. A sighting is the
    /// roster's answer and never the effect's own return (lessons claude-code
    /// A7, A14).
    pub sighted: bool,
    /// Whether the session this seat stands on is one this fleet has CLAIMED and
    /// the daemon still LISTS as not ended, both read this poll.
    ///
    /// Two halves, and each is needed. The claim is the adoption's write on the
    /// session table, which outlives the poll that made it — so this term is
    /// true on the fiftieth poll of a session claimed on the first, and a term
    /// filled from the claims one poll took would be false on every poll but
    /// that one. The listing is this poll's own reading of the row: a session the
    /// daemon marks ended is not one anyone owns, whatever the table still says
    /// about it.
    pub adopted_and_listed: bool,
    pub arrival_window_ms: u64,
    /// The halt latch and the blind count, both read from the session table this
    /// poll: the latch outlives a restart and the counter is what reaches it.
    pub halted: bool,
    pub blind: u32,
    /// Whether this seat's worktrees hold at least one pid-less row this poll —
    /// true on every `Stopped` reading, and on an `Absent` one whose only rows
    /// the recency window aged out. The replacement hold is defined on a ROW IN
    /// TRANSIT, and a seat with no row at all is not standing on one (R11).
    pub pidless_row: bool,
    /// Whether the agent daemon's pid differs from the one the last poll
    /// recorded. A daemon that was replaced ends every hosted process at once
    /// (lessons claude-code A10).
    pub daemon_pid_changed: bool,
    /// How long the agent daemon has been up, when that could be read. `None`
    /// is a reading nobody took, which opens nothing.
    pub daemon_uptime_ms: Option<u64>,
    /// Whether THIS CONTROLLER has read a live row for this seat on an earlier
    /// poll.
    ///
    /// False is a seat no poll of this controller has seen standing on a
    /// session, and that seat takes the table's ordinary verdict: there is no
    /// sighting for a pid-less row to be in transit FROM. The reading is the
    /// controller's own and not the row's start stamp, which is the session's
    /// birth and says nothing about whether anyone ever saw it alive.
    pub seen_live: bool,
    /// How long ago this controller read the FIRST pid-less poll of the run of
    /// them this seat is standing in, when it is standing in one.
    ///
    /// The transit window is measured from here rather than from the last live
    /// sighting, and the two are one number only while the polls are evenly
    /// spaced. They are not: a run is executed ON THE POLLING THREAD, so a land
    /// step that takes twenty minutes is twenty minutes in which no row is read
    /// at all, and the poll that follows holds a sighting that old for every
    /// seat — past any window, on the very poll the hold exists for. What the
    /// hold asks is how long THIS ROW has been pid-less, and this is that
    /// reading — the reading the poll after a run's land step turns on.
    pub since_pidless_ms: Option<u64>,
}

/// The verdict for one seat, with the transient filter over the table.
///
/// The filter is a WRAPPER and not three guards inside the table, which is its
/// whole value: it converts every session-creating verdict at one chokepoint,
/// so an arm added later is covered by [`Verdict::creates_a_session`] instead of
/// by whoever adds it remembering the rule. `leave-alone`, `halt` and
/// `suggest-rest` pass through untouched — a spawned seat is still observed,
/// still reported, and can still be told it is heavy; what it cannot be is
/// brought back (PRD R12, R21).
pub fn decide(input: &SeatInput) -> Verdict {
    let verdict = decide_table(input);
    if input.transient && verdict.creates_a_session() {
        return Verdict::LeaveAlone;
    }
    verdict
}

/// The table. First match wins, and the order IS the policy.
fn decide_table(input: &SeatInput) -> Verdict {
    // 1. Cannot-see is never treated as empty (PRD R6). It outranks everything,
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
    //    A live row is the precondition and not a detail: the collection order
    //    starts at `stop`, and there is nothing to stop on a pid-less row. A
    //    pending rest on one falls through to the discriminator below, where the
    //    same event reads as the deliberate end it is.
    if input.pending_rest && is_live(input.state) {
        return Verdict::Rest;
    }

    // 4. A dispatch is out and its window has not closed (R13's first half).
    //    Dispatching again here is the amplifier: the roster reads the same
    //    absence every poll, so without this a seat is dispatched once per poll
    //    for as long as arrival takes. A SIGHTING is what closes the window,
    //    because the start's own return cannot witness arrival.
    if !input.sighted && arrival_window_open(input) {
        return Verdict::LeaveAlone;
    }

    // 4b. The daemon was replaced moments ago and is re-hosting its sessions
    //     (R11, lessons claude-code A10). Every hosted row goes pid-less at once
    //     and comes back within about a minute, so a pid-less row here is a
    //     session in transit rather than a seat that needs anything.
    //
    //     Above arms 5 and 6 and below the arrival hold and the rest arm, which
    //     is the placement the arrival hold already has: a rest was ASKED FOR by
    //     the seat, and a hold that outranked it would disable the one path that
    //     frees a seat which asked to shed context.
    //
    //     It holds arm 5's revive as well as arm 6's spawn: a revive is a
    //     dispatch too, the daemon is already bringing the row back on its own,
    //     and one window of patience costs a seat nothing.
    //
    // 4c. And the same hold from the other evidence: this controller saw a live
    //     session for the seat on an earlier poll and does not now, with the
    //     daemon itself untouched, which is one session's HOST being replaced
    //     under an unchanged daemon. Both holds answer one question — is this
    //     pid-less row in transit — so they are taken at one arm and told apart
    //     by the reason each prints.
    if hold(input).is_some() {
        return Verdict::LeaveAlone;
    }

    // 5. A pid-less row that is not a newborn (R10). The roster cannot tell a
    //    hibernated session from a deliberately stopped one, so the
    //    discriminator is an event the ending ritual wrote plus a context guard.
    if input.state == RosterState::Stopped {
        // 5a. The seat ended on purpose and wants a successor. Reviving here
        //     brings back a session that has already said goodbye: it would sit
        //     idle, read present from then on, and the successor the handoff
        //     exists to produce would never come up.
        if input.pending_deliberate_end {
            return Verdict::SpawnWoken;
        }
        // 5b. The fleet has claimed the session under this row and the daemon
        //     still lists it: what stands here is a session this controller
        //     owns and the agent still holds, not a seat that is gone.
        //     Attaching to it spends a dispatch — and, on a seat whose first
        //     turn is a wake, a whole wake — to reach a session that is already
        //     there, and the row reads the same on the next poll, so the cost
        //     repeats for as long as the session stays pid-less.
        //
        //     The hold stands while the daemon lists the row and releases on
        //     an end that can be named — `stopped` or `failed` on the row, a
        //     deliberate-end event, or the row leaving the listing — so a
        //     session killed from outside the fleet is held rather than
        //     revived, which is the price of a roster that reads a killed
        //     session and a hibernating one alike.
        if input.adopted_and_listed {
            return Verdict::LeaveAlone;
        }
        // 5c. No such event and room left to work: bring the session back in
        //     place, context intact and no wake burned.
        // 5d. At or over the threshold is the one dangerous unmarked case — a
        //     revived at-ceiling session reads healthy on every board and can do
        //     no work — so it is spawned over instead. An UNMEASURABLE context
        //     falls here too: the rule licenses a revive on a measured
        //     under-threshold reading, and a term nobody measured is not one.
        return match input.context_tokens {
            Some(tokens) if tokens < input.rest_threshold_tokens => Verdict::Revive,
            _ => Verdict::SpawnWoken,
        };
    }

    // 6. A seat with no row at all. Nothing is being stood on, so arm 5's
    //    question does not arise — there is no row to revive.
    if input.state == RosterState::Absent {
        return Verdict::SpawnWoken;
    }

    // 7. Suggested once per session and never enforced (R21, lessons claude-code
    //    C5). Keying the dedupe on the session id re-arms it for a successor
    //    with no bookkeeping of its own.
    if is_live(input.state) && !input.already_nudged {
        if let Some(tokens) = input.context_tokens {
            if tokens >= input.rest_threshold_tokens {
                return Verdict::SuggestRest;
            }
        }
    }

    // A starting row lands here: it is the only row about to become live, and
    // spawning beside it is what puts two live rows in one worktree.
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

/// The prefix every replacement-window hold's reason carries, so the log line
/// and the verdict cannot disagree about which leave-alone this is.
pub const REPLACEMENT_HELD: &str = "replacement window: held";

/// Whether the agent daemon is inside a replacement window this poll (R11).
///
/// Two readings, either of which names one. The uptime is the primary: a daemon
/// younger than the arrival window is one that has just replaced another, and it
/// answers for every poll of the window rather than for the single poll a pid
/// change is visible on. The pid change covers what the uptime cannot — a poll
/// loop that was stalled, slept or restarted across the replacement and arrives
/// after the window has already elapsed.
///
/// AN UNREADABLE DAEMON OPENS NOTHING, and that direction is deliberate: a hold
/// on a reading nobody took would stop every dispatch on the fleet for as long as
/// the read stayed broken.
pub fn replacement_window_open(input: &SeatInput) -> bool {
    input.daemon_pid_changed
        || matches!(input.daemon_uptime_ms, Some(up) if up < input.arrival_window_ms)
}

/// The reason this seat is held for the replacement window, when it is.
///
/// ONE FUNCTION, TWO READERS: the table's arm above returns its verdict from
/// this, and the loop logs this string once per transition into the hold. A rule
/// stated twice is a rule that disagrees with itself the first time either copy
/// moves.
pub fn replacement_hold(input: &SeatInput) -> Option<String> {
    // The two states a dispatch would be issued from, and no other. A STARTING
    // row is pid-less too and is already held by the arm above it, so holding it
    // here would put a replacement's name on a newborn nothing is waiting on.
    if !matches!(input.state, RosterState::Stopped | RosterState::Absent) {
        return None;
    }
    if !input.pidless_row || !replacement_window_open(input) {
        return None;
    }
    Some(format!(
        "{REPLACEMENT_HELD} — {}'s row is pid-less and the daemon {}. A replacement ends every \
         hosted process and re-hosts the sessions within about a minute, so this row is in \
         transit rather than gone: nothing is spawned over it and nothing is attached to it \
         until it returns or the window closes",
        input.seat_dir,
        daemon_age(input)
    ))
}

/// The prefix every re-host hold's reason carries, so the two holds are told
/// apart by the first words of the line rather than by reading the clause.
pub const REHOST_HELD: &str = "re-host window: held";

/// The reason this seat is held while the HOST of its session is replaced,
/// when it is.
///
/// The evidence is this controller's own earlier sighting, and it has to be:
/// replacing one session's host leaves the daemon's pid and its uptime
/// untouched, so [`replacement_window_open`] reads nothing, the row reads
/// pid-less like any other, and the arm below it spends a dispatch — a whole
/// wake, on a seat whose first turn is one — on a session that is already
/// there and comes back on its own within about a minute.
///
/// A DELIBERATE END IS NOT HELD. A seat that wrote its own ending event has
/// said this row is finished, which is the one reading that tells a row that
/// ended from a row being re-hosted at the poll it goes pid-less on; holding it
/// would delay every successor by a window for a distinction the event has
/// already made.
///
/// TWO TERMS AND NOT ONE, because the sighting and the window answer different
/// questions: [`SeatInput::seen_live`] is whether there is a live row for this
/// one to be in transit FROM, and [`SeatInput::since_pidless_ms`] is how long
/// the transit has run. A window measured from the sighting instead is a window
/// the length of the gap between two polls, and the loop's gaps are not the poll
/// interval — a run occupies the polling thread for as long as its steps take
/// — the reading the poll after a run's land step turns on.
pub fn rehost_hold(input: &SeatInput) -> Option<String> {
    // The two states a dispatch would be issued from, and no other, exactly as
    // the replacement hold above: a live row is not in transit and a starting
    // one is already held by the arm over both of them.
    if !matches!(input.state, RosterState::Stopped | RosterState::Absent) {
        return None;
    }
    if input.pending_deliberate_end {
        return None;
    }
    if !input.seen_live {
        return None;
    }
    let since = input.since_pidless_ms?;
    if since >= input.arrival_window_ms {
        return None;
    }
    Some(format!(
        "{REHOST_HELD} — {}'s row carried a live session under this controller and {} {}s after \
         it did. Replacing a session's HOST takes the pid off its row while the daemon itself \
         stands unchanged, and the session is listed again within about a minute, so this row is \
         RE-HOSTING rather than gone: nothing is spawned over it and nothing is attached to it \
         until it is listed again or the {}s window closes",
        input.seat_dir,
        match input.state {
            RosterState::Stopped => "has read pid-less",
            _ => "has stood on no row the listing answers for",
        },
        since / 1000,
        input.arrival_window_ms / 1000
    ))
}

/// The hold in force this poll, whichever of the two it is.
///
/// ONE FUNCTION, TWO READERS, as each hold is on its own: the table's arm
/// reaches its verdict through this and the loop logs what it returns once per
/// transition into a hold, so the verdict and the line cannot disagree about
/// which hold this is.
///
/// The replacement window is asked FIRST. A daemon that was replaced re-hosts
/// every session it holds at once, so its reason is the one that explains the
/// whole poll, and a seat inside both windows is better described by it.
pub fn hold(input: &SeatInput) -> Option<String> {
    replacement_hold(input).or_else(|| rehost_hold(input))
}

/// How the hold's reason describes the daemon it is waiting on.
fn daemon_age(input: &SeatInput) -> String {
    match input.daemon_uptime_ms {
        Some(up) if up < input.arrival_window_ms => format!(
            "has been up {}s, under the {}s arrival window",
            up / 1000,
            input.arrival_window_ms / 1000
        ),
        // Reachable only through the pid-change half, so it says that rather
        // than printing an age no arm read.
        _ => "changed pid since the last poll".to_string(),
    }
}

/// The blind counter after this poll (R13, R14).
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
        // A starting row is neither a sighting nor a dispatch, so it moves the
        // counter in no direction. The guard stays reachable either way: the row
        // becomes present within a poll or two and decays it, or it ages past
        // the starting bound, reads stopped, and takes an arm that increments.
        RosterState::Starting => previous,
        // Stopped counts exactly as Absent, and this is what bounds
        // spawn-over-stopped: a seat that keeps reading no-live after being
        // dispatched accumulates here and reaches the halt guard, whatever the
        // reason the dispatch did not take.
        //
        // A REVIVE COUNTS AS A DISPATCH, and that is load-bearing rather than
        // tidy: an attach exits 0 and prints the same line whether it revived
        // the row or did nothing (lessons claude-code A7), so a revive that
        // silently fails would otherwise be retried every poll forever and never
        // reach the guard — a runaway that is quiet instead of loud.
        RosterState::Stopped | RosterState::Absent => {
            if verdict.creates_a_session() {
                previous + 1
            } else {
                previous
            }
        }
    }
}

/// Seats on pid-less rows below which one poll is never the upgrade shape.
///
/// Half a roster of one or two is a single hibernation, and announcing that as a
/// fleet-wide event would spend the line's whole meaning on the smallest fleets.
pub const UPGRADE_SHAPE_FLOOR: usize = 2;

/// The whole-fleet shape a poll is carrying, when it is carrying one (R15).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetShape {
    pub pidless: usize,
    pub total: usize,
}

impl FleetShape {
    pub fn describe(&self) -> String {
        format!(
            "fleet: CLI-UPGRADE SHAPE — {} of {} configured seats stand on pid-less rows in ONE \
             poll. Upgrading the agent replaces the daemon and stops every session at once, so \
             the seat lines below are ONE event rather than {} independent absences; each seat \
             is still dispatched on its own terms",
            self.pidless, self.total, self.pidless
        )
    }
}

/// The threshold itself, over counts rather than states.
///
/// Split out so the live path and any other caller cannot drift: a rule stated
/// twice is a rule that disagrees with itself the first time either copy moves.
pub fn fleet_shape_from_counts(pidless: usize, total: usize) -> Option<FleetShape> {
    if pidless >= UPGRADE_SHAPE_FLOOR && pidless * 2 >= total {
        Some(FleetShape { pidless, total })
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
            .filter(|state| matches!(state, RosterState::Stopped | RosterState::Absent))
            .count(),
        states.len(),
    )
}
