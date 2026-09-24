//! What a suite drives the controller with. Compiled into this crate's own
//! tests, and into the library itself only under the `test-support` feature,
//! which a dependent's DEV-dependency turns on: under resolver 2 that keeps it
//! out of the binary a release build produces.

use crate::adapter::{Agent, DaemonRead, RemoveAnswer, RosterRead, StartOutcome, StartSpec};
use crate::clock::Clock;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Time a test owns. [`Clock::sleep`] advances it by exactly the duration asked
/// for and returns, so a wait spent against this clock costs no wall clock and a
/// deadline computed from [`Clock::now`] expires the instant fake time passes it.
///
/// Single-threaded by ruling: the sleeping thread is the one that advances, so
/// there is nothing here to park on or notify.
///
/// The one real reading is the ORIGIN, taken once at construction because stable
/// Rust offers no other way to obtain an `Instant`; every later reading is that
/// origin plus the fake time spent since.
pub struct FakeClock {
    origin: Instant,
    /// The wall-clock stamp the origin was taken at, so the epoch milliseconds
    /// this clock answers with age by exactly the fake time spent. Real at
    /// construction rather than an invented epoch: the rows a poll reads carry
    /// the agent's own stamps, and a clock starting at zero would read every one
    /// of them as written in the future.
    origin_ms: u64,
    spent: Mutex<Duration>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            origin_ms: crate::clock::now_ms(),
            spent: Mutex::new(Duration::ZERO),
        }
    }

    /// Move fake time forward without a wait having asked for it.
    pub fn advance(&self, by: Duration) {
        let mut spent = self.spent.lock().expect("the fake clock's own lock");
        *spent += by;
    }

    /// Fake time spent since construction — what an arm asserts a wait consumed.
    pub fn spent(&self) -> Duration {
        *self.spent.lock().expect("the fake clock's own lock")
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.origin + self.spent()
    }

    fn sleep(&self, d: Duration) {
        self.advance(d)
    }

    fn now_ms(&self) -> u64 {
        self.origin_ms + self.spent().as_millis() as u64
    }
}

/// One call the stub received, in the order it arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// One of [`StubAgent`]'s verb constants, so an arm and the stub name a verb
    /// from the same value.
    pub verb: &'static str,
    /// Which session or seat the call was about — the address, the seat
    /// directory or the session id it named. Empty for a verb that names none.
    pub about: String,
}

/// What the stub answers with, one field per verb.
///
/// Set per arm at construction, and settable again between ticks through
/// [`StubAgent::set`]: a loop's second tick can be told something its first was
/// not, which is how a roster that changes under the controller is driven.
#[derive(Clone, Debug)]
pub struct Answers {
    pub status: RosterRead,
    pub version: Option<String>,
    pub daemon: DaemonRead,
    pub start: StartOutcome,
    pub stop: Result<(), String>,
    pub revive: Result<(), String>,
    pub nudge: Result<(), String>,
    pub remove: RemoveAnswer,
    pub transcript: Option<String>,
    pub ended_at: Option<u64>,
}

impl Default for Answers {
    /// A fleet the agent can see and nothing is running in: an empty roster that
    /// READ, a daemon that is not running, and every effect succeeding.
    fn default() -> Self {
        Self {
            status: RosterRead::Readable(Vec::new()),
            version: Some(StubAgent::VERSION.to_string()),
            daemon: DaemonRead::Readable(None),
            start: StartOutcome::Started { log: String::new() },
            stop: Ok(()),
            revive: Ok(()),
            nudge: Ok(()),
            remove: RemoveAnswer::Removed,
            transcript: None,
            ended_at: None,
        }
    }
}

/// An agent with no process behind it: every verb answers from [`Answers`] and
/// records that it was asked.
///
/// The record is what tells a tick that RAN from one that returned early, so it
/// is appended to and never rewritten — the order the calls arrived in is half
/// of what an arm about the loop's sequence asserts.
pub struct StubAgent {
    answers: Mutex<Answers>,
    calls: Mutex<Vec<Call>>,
    starts: Mutex<Vec<StartSpec>>,
}

impl StubAgent {
    pub const START: &'static str = "start";
    pub const STOP: &'static str = "stop";
    pub const REMOVE: &'static str = "remove";
    pub const REVIVE: &'static str = "revive";
    pub const DAEMON: &'static str = "daemon";
    pub const NUDGE: &'static str = "nudge";
    pub const STATUS: &'static str = "status";
    pub const TRANSCRIPT: &'static str = "transcript";
    pub const ENDED_AT: &'static str = "ended_at";
    pub const VERSION_CALL: &'static str = "version";

    /// What [`Answers::default`] reports as the live agent version.
    pub const VERSION: &'static str = "0.0.0-stub";

    /// Where this provider would keep a session's project-local settings. No
    /// observe path reads it; it is here because the trait has the verb.
    pub const LOCAL_SETTINGS: &'static str = ".stub/settings.json";

    pub fn new() -> StubAgent {
        StubAgent::answering(Answers::default())
    }

    pub fn answering(answers: Answers) -> StubAgent {
        StubAgent {
            answers: Mutex::new(answers),
            calls: Mutex::new(Vec::new()),
            starts: Mutex::new(Vec::new()),
        }
    }

    /// Change what the next call is told.
    pub fn set(&self, change: impl FnOnce(&mut Answers)) {
        change(&mut self.answers.lock().expect("the stub agent's own lock"));
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .expect("the stub agent's own lock")
            .clone()
    }

    /// The verbs alone, in order — what an arm asserting a tick's SHAPE reads.
    pub fn verbs(&self) -> Vec<&'static str> {
        self.calls().into_iter().map(|call| call.verb).collect()
    }

    pub fn calls_of(&self, verb: &str) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.verb == verb)
            .collect()
    }

    /// Every start's whole specification, so an arm reads what the effect asked
    /// for rather than only that it asked.
    pub fn starts(&self) -> Vec<StartSpec> {
        self.starts
            .lock()
            .expect("the stub agent's own lock")
            .clone()
    }

    fn answers(&self) -> Answers {
        self.answers
            .lock()
            .expect("the stub agent's own lock")
            .clone()
    }

    fn record(&self, verb: &'static str, about: impl Into<String>) {
        self.calls
            .lock()
            .expect("the stub agent's own lock")
            .push(Call {
                verb,
                about: about.into(),
            });
    }
}

impl Default for StubAgent {
    fn default() -> Self {
        StubAgent::new()
    }
}

impl Agent for StubAgent {
    fn start(&self, spec: &StartSpec, _watch: Duration) -> StartOutcome {
        self.record(StubAgent::START, spec.name.clone());
        self.starts
            .lock()
            .expect("the stub agent's own lock")
            .push(spec.clone());
        self.answers().start
    }

    fn local_settings(&self) -> &'static str {
        StubAgent::LOCAL_SETTINGS
    }

    fn stop(&self, _config_dir: Option<&Path>, short_id: &str) -> Result<(), String> {
        self.record(StubAgent::STOP, short_id);
        self.answers().stop
    }

    fn remove(&self, _config_dir: Option<&Path>, short_id: &str) -> RemoveAnswer {
        self.record(StubAgent::REMOVE, short_id);
        self.answers().remove
    }

    fn revive(&self, _config_dir: Option<&Path>, short_id: &str) -> Result<(), String> {
        self.record(StubAgent::REVIVE, short_id);
        self.answers().revive
    }

    fn daemon(&self) -> DaemonRead {
        self.record(StubAgent::DAEMON, "");
        self.answers().daemon
    }

    fn nudge(
        &self,
        _config_dir: Option<&Path>,
        session_name: &str,
        _worktree: &str,
        _model: &str,
        _prompt: &str,
        _timeout: Duration,
    ) -> Result<(), String> {
        self.record(StubAgent::NUDGE, session_name);
        self.answers().nudge
    }

    fn status(&self, config_dir: Option<&Path>) -> RosterRead {
        self.record(
            StubAgent::STATUS,
            config_dir
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
        );
        self.answers().status
    }

    fn transcript(
        &self,
        _config_dir: Option<&Path>,
        _worktree: &str,
        session_id: &str,
    ) -> Option<String> {
        self.record(StubAgent::TRANSCRIPT, session_id);
        self.answers().transcript
    }

    fn ended_at(
        &self,
        _config_dir: Option<&Path>,
        _worktree: &str,
        session_id: &str,
    ) -> Option<u64> {
        self.record(StubAgent::ENDED_AT, session_id);
        self.answers().ended_at
    }

    fn version(&self) -> Option<String> {
        self.record(StubAgent::VERSION_CALL, "");
        self.answers().version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sleep advances the clock by EXACTLY what it was asked for — not a
    /// slice, not a rounding, and the sum of several is the sum of their
    /// durations.
    #[test]
    fn a_sleep_advances_the_fake_clock_by_exactly_the_duration_it_was_given() {
        let clock = FakeClock::new();
        let start = clock.now();

        clock.sleep(Duration::from_secs(5));
        assert_eq!(clock.now() - start, Duration::from_secs(5));
        assert_eq!(clock.spent(), Duration::from_secs(5));

        clock.sleep(Duration::from_millis(250));
        assert_eq!(clock.now() - start, Duration::from_millis(5_250));

        clock.sleep(Duration::ZERO);
        assert_eq!(clock.now() - start, Duration::from_millis(5_250));
    }

    /// The property every seamed deadline rests on, asserted from BOTH sides: a
    /// clock that over-advances expires a deadline early and fails the
    /// nanosecond-short assert; one that under-advances never reaches it and
    /// fails the arrival assert.
    #[test]
    fn a_deadline_taken_from_the_fake_clock_expires_when_fake_time_passes_it_and_not_before() {
        let clock = FakeClock::new();
        let deadline = clock.now() + Duration::from_secs(30);

        assert!(clock.now() < deadline, "a deadline is not expired at once");

        clock.sleep(Duration::from_secs(30) - Duration::from_nanos(1));
        assert!(
            clock.now() < deadline,
            "a nanosecond short of the deadline is short of the deadline"
        );

        clock.sleep(Duration::from_nanos(1));
        assert!(clock.now() >= deadline, "fake time reached the deadline");

        clock.sleep(Duration::from_secs(1));
        assert!(clock.now() >= deadline, "and stays past it");
    }
}
