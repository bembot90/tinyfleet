//! UTC stamps, through jiff, and monotonic-ish milliseconds, by arithmetic, and the [`Clock`]
//! a wait is spent against.
//!
//! Every stamp is UTC and to the second, written and read in one shape through
//! jiff, the crate the cron trigger reads local wall-clock with.
//!
//! [`Clock`] is the seam between a wait whose subject is a DURATION and real
//! time. A wait whose subject is an OS fact — has this child exited, has this
//! pipe drained — is not behind it and cannot be: no clock makes a real process
//! exit sooner.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jiff::{civil::DateTime, tz::Offset, Timestamp};

/// The one stamp shape, as the writer renders it and the reader parses it.
const FORM: &str = "%Y-%m-%dT%H:%M:%SZ";

/// The passage of time, as a wait sees it.
///
/// Single-threaded by ruling: the caller's own thread both sleeps and is woken,
/// so [`Clock::sleep`] under a fake advances that clock and returns. Nothing here
/// parks or notifies, and no other thread can move time.
pub trait Clock {
    /// A reading a deadline is computed from. Monotonic, and comparable only
    /// against other readings of the same clock.
    fn now(&self) -> Instant;

    /// Spend `d`. A fake spends it by advancing [`Clock::now`] and returning.
    fn sleep(&self, d: Duration);

    /// The WALL-CLOCK stamp one poll carries, in epoch milliseconds.
    ///
    /// Behind the same seam as the waits, because every window the loop keeps is
    /// the difference between two of these: a rig whose naps are fake and whose
    /// stamps come from the real clock drives any number of ticks without aging
    /// a window by a millisecond, so no arm through the loop can tell a rule
    /// keyed to one of those stamps from a rule keyed to another.
    ///
    /// Defaulted to the real clock, so a caller that seams only the waits reads
    /// exactly what it read before this method existed.
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

/// Real time: each method is the bare standard-library call it wraps, so a
/// seamed site under this clock waits exactly as it did before the seam.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d)
    }
}

/// Wall-clock milliseconds since the epoch. A clock before the epoch reads 0
/// rather than panicking; every caller compares it against a roster timestamp
/// that has the same origin.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a system time, or `None` when it predates the
/// epoch — a stamp nobody can state is absent from the document rather than
/// invented.
pub fn stamp_of(t: SystemTime) -> Option<String> {
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(stamp_secs(secs))
}

/// The stated rule this does not share with [`stamp_of`]: a clock before the
/// epoch renders AS the epoch rather than refusing, because `generated_at` is
/// not an optional field and a document carrying no stamp at all is worse to
/// read than one stamped impossibly early. `stamp_of` refuses instead, because
/// the mtime it stamps is an absent field by design when it cannot be stated.
pub fn now_stamp() -> String {
    stamp_secs(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

/// A second past jiff's last timestamp (9999-12-30T22:00:00Z) renders as that
/// timestamp; no clock reading reaches it.
pub fn stamp_secs(secs: u64) -> String {
    let at = i64::try_from(secs)
        .ok()
        .and_then(|s| Timestamp::from_second(s).ok())
        .unwrap_or(Timestamp::MAX);
    at.strftime(FORM).to_string()
}

/// How long ago a stamp this module wrote was written, in seconds.
///
/// `None` is a stamp that does not parse or one in the future, and both are
/// answers a freshness check must not round into "fresh": a document nobody can
/// date is not one anybody can call current.
pub fn seconds_since_stamp(stamp: &str) -> Option<u64> {
    let secs = secs_of_stamp(stamp)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    now.checked_sub(secs)
}

/// `YYYY-MM-DDTHH:MM:SSZ` back to epoch seconds — the inverse of [`stamp_secs`],
/// and strict about the shape, because a lenient parse of a stamp this fleet
/// did not write reads as a date nobody meant.
pub fn secs_of_stamp(stamp: &str) -> Option<u64> {
    let bytes = stamp.as_bytes();
    if bytes.len() != 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'Z' {
        return None;
    }
    let at = DateTime::strptime(FORM, stamp).ok()?;
    let secs = u64::try_from(Offset::UTC.to_timestamp(at).ok()?.as_second()).ok()?;
    (stamp_secs(secs) == stamp).then_some(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_and_two_later_days() {
        assert_eq!(stamp_secs(0), "1970-01-01T00:00:00Z");
        assert_eq!(stamp_secs(1_788_600_000), "2026-09-05T09:20:00Z");
        // A leap day, which the calendar must get right.
        assert_eq!(stamp_secs(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    /// The stamp reader is the exact inverse of the writer, across the same
    /// dates the writer's own arm names — the epoch, an ordinary day, a leap day
    /// and both edges of a day.
    #[test]
    fn a_stamp_this_module_wrote_reads_back_as_the_second_it_was_written_from() {
        for secs in [0, 1_788_600_000, 1_709_164_800, 86_399, 86_400] {
            assert_eq!(
                secs_of_stamp(&stamp_secs(secs)),
                Some(secs),
                "{secs} did not round-trip through {}",
                stamp_secs(secs)
            );
        }
    }

    /// A stamp this fleet did not write is refused rather than read leniently:
    /// a freshness check that accepted a wrong date would call a stale document
    /// current, which is the one answer it exists to prevent.
    #[test]
    fn a_stamp_of_another_shape_is_no_reading_at_all() {
        for bad in [
            "",
            "2026-09-08",
            "2026-09-08T00:00:00",
            "2026-09-08 00:00:00Z",
            "2026-09-08T00:00:00.000Z",
            "2026-13-08T00:00:00Z",
            "2026-09-32T00:00:00Z",
            "2026-09-08T24:00:00Z",
            "2026-09-08T00:60:00Z",
            "not-a-stamp-at-all!!!",
        ] {
            assert_eq!(secs_of_stamp(bad), None, "{bad:?} is not a stamp");
        }
    }

    /// The age a freshness check reads. A stamp in the FUTURE is `None` and never
    /// a zero: a clock that disagrees with the writer's is a reading nobody can
    /// use, and rounding it to "just now" calls a document current on the
    /// strength of the disagreement.
    #[test]
    fn the_age_of_a_stamp_is_none_when_it_has_not_happened_yet() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("this clock is after the epoch")
            .as_secs();
        assert_eq!(seconds_since_stamp(&stamp_secs(now - 30)), Some(30));
        assert_eq!(seconds_since_stamp(&stamp_secs(now)), Some(0));
        assert_eq!(
            seconds_since_stamp(&stamp_secs(now + 60)),
            None,
            "a stamp from the future is no age"
        );
        assert_eq!(seconds_since_stamp("not-a-stamp"), None);
    }

    #[test]
    fn seconds_within_a_day_are_split_into_hours_minutes_seconds() {
        assert_eq!(stamp_secs(86_399), "1970-01-01T23:59:59Z");
        assert_eq!(stamp_secs(86_400), "1970-01-02T00:00:00Z");
    }

    #[test]
    fn the_writer_renders_each_calendar_edge_as_it_always_has() {
        for (secs, stamp) in [
            (0, "1970-01-01T00:00:00Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (68_169_599, "1972-02-28T23:59:59Z"),
            (68_169_600, "1972-02-29T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (951_868_800, "2000-03-01T00:00:00Z"),
            (978_307_199, "2000-12-31T23:59:59Z"),
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_740_787_200, "2025-03-01T00:00:00Z"),
            (2_147_483_648, "2038-01-19T03:14:08Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z"),
            (4_107_628_800, "2100-03-02T00:00:00Z"),
            (253_402_207_199, "9999-12-30T21:59:59Z"),
        ] {
            assert_eq!(stamp_secs(secs), stamp, "{secs} renders as {stamp}");
            assert_eq!(secs_of_stamp(stamp), Some(secs), "{stamp} reads as {secs}");
        }
    }

    #[test]
    fn every_stamp_the_writer_makes_reads_back_across_the_range() {
        for secs in (0..=253_402_207_199).step_by(997_331) {
            let stamp = stamp_secs(secs);
            assert_eq!(stamp.len(), 20, "{stamp} is not 20 bytes");
            assert_eq!(
                secs_of_stamp(&stamp),
                Some(secs),
                "{secs} did not round-trip through {stamp}"
            );
        }
    }

    /// A date the calendar does not hold is refused rather than rolled into
    /// the next month: a cursor at 02-31 is a moment nobody meant.
    #[test]
    fn an_impossible_date_is_no_reading_at_all() {
        for bad in [
            "2026-02-31T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-10-07T12:00:60Z",
            "2026-+9-08T00:00:00Z",
            "2026-09-08T-1:00:00Z",
            "2026-09-08T 1:00:00Z",
        ] {
            assert_eq!(secs_of_stamp(bad), None, "{bad:?} is not a date");
        }
    }
}
