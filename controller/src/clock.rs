//! UTC stamps and monotonic-ish milliseconds, by arithmetic, and the [`Clock`]
//! a wait is spent against.
//!
//! No date crate here: every stamp is UTC, and the civil-from-days calendar
//! below is all a UTC formatter needs. The slice that reads local wall-clock
//! chooses its own crate.
//!
//! [`Clock`] is the seam between a wait whose subject is a DURATION and real
//! time. A wait whose subject is an OS fact — has this child exited, has this
//! pipe drained — is not behind it and cannot be: no clock makes a real process
//! exit sooner.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

pub fn stamp_secs(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
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
    let field = |from: usize, to: usize| stamp[from..to].parse::<i64>().ok();
    let (y, m, d) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let (hh, mm, ss) = (field(11, 13)?, field(14, 16)?, field(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let days = days_from_civil(y, m as u64, d as u64);
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// A proleptic Gregorian date to days since 1970-01-01 (Howard Hinnant's
/// `days_from_civil`), the exact inverse of the split below.
fn days_from_civil(y: i64, m: u64, d: u64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`), with March as the first month of the internal year so
/// the leap day falls at the end of the cycle.
fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_and_two_later_days() {
        assert_eq!(stamp_secs(0), "1970-01-01T00:00:00Z");
        assert_eq!(stamp_secs(1_788_600_000), "2026-09-05T09:20:00Z");
        // A leap day, which the month-shifted arithmetic exists to get right.
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
}
