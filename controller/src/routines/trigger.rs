//! The clock: a routine's trigger, and whether it is due at one instant (PRD
//! R22).
//!
//! Three answers and never two. `due`, `not-due` and `could-not-tell` are
//! distinct all the way through — a check that timed out, could not be started
//! or exited a status its routine calls unknown is a reading nobody has, and
//! rounding it into "not due" is how a duty stops running with nothing to say
//! so.
//!
//! `cron` is matched against LOCAL wall-clock, which is the only clock a person
//! writes a nightly duty in.

use super::file::Routine;
use std::sync::OnceLock;

/// The three clocks a routine can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Cron,
    Cooldown,
    Condition,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Cron => "cron",
            Trigger::Cooldown => "cooldown",
            Trigger::Condition => "condition",
        }
    }

    pub(super) fn parse(word: &str) -> Option<Trigger> {
        match word {
            "cron" => Some(Trigger::Cron),
            "cooldown" => Some(Trigger::Cooldown),
            "condition" => Some(Trigger::Condition),
            _ => None,
        }
    }
}

/// The five wall-clock fields a cron schedule is matched against, in this
/// machine's own zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalMinute {
    pub minute: i8,
    pub hour: i8,
    pub day: i8,
    pub month: i8,
    /// 0 through 6, Sunday first. A schedule's 7 is compared modulo 7, so both
    /// spellings of Sunday match.
    pub weekday: i8,
}

/// The five fields, in the order a schedule writes them.
const FIELDS: [(&str, i8, i8); 5] = [
    ("minute", 0, 59),
    ("hour", 0, 23),
    ("day-of-month", 1, 31),
    ("month", 1, 12),
    ("day-of-week", 0, 7),
];

/// How far forward `next_due` searches a cron schedule before answering that
/// there is no next minute. A schedule that matches nothing inside it — the
/// thirtieth of February — has no next minute at all, and the search has to
/// stop somewhere a person would call soon enough.
pub const SEARCH_LIMIT_MINUTES: u64 = 400 * 24 * 60;

/// This machine's zone, resolved once. The read walks the system's own
/// configuration, and a `next_due` search walks up to the limit above.
fn zone() -> &'static jiff::tz::TimeZone {
    static ZONE: OnceLock<jiff::tz::TimeZone> = OnceLock::new();
    ZONE.get_or_init(jiff::tz::TimeZone::system)
}

/// A UTC instant as the local wall-clock minute it falls in.
pub fn local_minute_of(secs: u64) -> Result<LocalMinute, String> {
    let stamp =
        i64::try_from(secs).map_err(|_| format!("{secs} is no instant this clock holds"))?;
    let instant = jiff::Timestamp::from_second(stamp)
        .map_err(|e| format!("{secs} is no instant this clock holds: {e}"))?;
    let local = jiff::Zoned::new(instant, zone().clone());
    Ok(LocalMinute {
        minute: local.minute(),
        hour: local.hour(),
        day: local.day(),
        month: local.month(),
        weekday: local.weekday().to_sunday_zero_offset(),
    })
}

/// Whether a schedule reads at all, with the field that does not named.
pub fn validate_cron(schedule: &str) -> Result<(), String> {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "`{schedule}` carries {} fields; a schedule is five: minute hour day-of-month month day-of-week",
            fields.len()
        ));
    }
    for (field, (name, low, high)) in fields.iter().zip(FIELDS) {
        check_field(field, name, low, high)?;
    }
    Ok(())
}

/// One field's form and bounds. A RANGE is refused with the list that replaces
/// it, because a schedule silently read as a literal would fire at a minute
/// nobody wrote.
fn check_field(field: &str, name: &str, low: i8, high: i8) -> Result<(), String> {
    if field == "*" {
        return Ok(());
    }
    if let Some(step) = field.strip_prefix("*/") {
        let read = step.parse::<i8>().ok().filter(|n| *n >= 1);
        return match read {
            Some(_) => Ok(()),
            None => Err(format!(
                "`{field}` is not a step in the {name} field; the form is */N with N at least 1"
            )),
        };
    }
    if let Some(rewrite) = range_rewrite(field, low, high) {
        return Err(format!(
            "`{field}` is a range in the {name} field, which is not a form this schedule reads; \
             write it as `{rewrite}`"
        ));
    }
    for part in field.split(',') {
        let read = part.parse::<i8>().ok().filter(|n| *n >= low && *n <= high);
        if read.is_none() {
            return Err(format!(
                "`{field}` is not a {name} field; the forms are *, */N, an integer between {low} \
                 and {high}, and a comma-separated list of them"
            ));
        }
    }
    Ok(())
}

/// The comma list a range would have been, when both ends read and the span is
/// inside the field's bounds. `None` leaves the caller's general refusal to
/// speak, so a `1-` or a `9-3` is not answered with a rewrite that is wrong.
fn range_rewrite(field: &str, low: i8, high: i8) -> Option<String> {
    let (from, to) = field.split_once('-')?;
    let from = from.parse::<i8>().ok()?;
    let to = to.parse::<i8>().ok()?;
    if from > to || from < low || to > high {
        return Some(format!("{from}-{to}"));
    }
    Some(
        (from..=to)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// Does this minute match all five fields?
pub fn cron_matches(schedule: &str, when: &LocalMinute) -> Result<bool, String> {
    validate_cron(schedule)?;
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    let actual = [when.minute, when.hour, when.day, when.month, when.weekday];
    for (index, (field, value)) in fields.iter().zip(actual).enumerate() {
        if !field_matches(field, value, index == 4) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn field_matches(field: &str, value: i8, is_weekday: bool) -> bool {
    if field == "*" {
        return true;
    }
    if let Some(step) = field.strip_prefix("*/") {
        return step
            .parse::<i8>()
            .ok()
            .filter(|n| *n >= 1)
            .is_some_and(|n| value % n == 0);
    }
    field.split(',').any(|part| match part.parse::<i8>() {
        // 0 and 7 are both Sunday, which is why the weekday field compares
        // modulo 7 and no other field does.
        Ok(wanted) if is_weekday => wanted % 7 == value % 7,
        Ok(wanted) => wanted == value,
        Err(_) => false,
    })
}

/// What running a condition's command produced. The three kinds are separate
/// because only one of them carries an exit status to read.
#[derive(Clone, Debug)]
pub enum CheckOutcome {
    Exited(i32),
    /// The command ran past its `check_timeout`, or ended on a signal — neither
    /// is a status the routine's own table can speak about.
    Timeout(String),
    NotStarted(String),
}

/// A trigger's three answers, each carrying the sentence a person reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Due {
    Due(String),
    NotDue(String),
    CouldNotTell(String),
}

impl Due {
    pub fn word(&self) -> &'static str {
        match self {
            Due::Due(_) => "due",
            Due::NotDue(_) => "not-due",
            Due::CouldNotTell(_) => "could-not-tell",
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Due::Due(why) | Due::NotDue(why) | Due::CouldNotTell(why) => why,
        }
    }
}

/// Whether this routine is due at `now`, in epoch seconds.
///
/// `last_fired` is the caller's: nothing here reads or writes state, and the
/// only disk this touches is `run_check`'s command.
pub fn evaluate(
    routine: &Routine,
    now: u64,
    last_fired: Option<u64>,
    run_check: &dyn Fn(&Routine) -> CheckOutcome,
) -> Due {
    if !routine.enabled {
        return Due::NotDue("the routine is disabled".to_string());
    }
    match routine.trigger {
        Trigger::Cron => cron(routine, now, last_fired),
        Trigger::Cooldown => cooldown(routine, now, last_fired),
        Trigger::Condition => condition(routine, run_check),
    }
}

fn cron(routine: &Routine, now: u64, last_fired: Option<u64>) -> Due {
    let Some(schedule) = routine.schedule.as_deref() else {
        return Due::CouldNotTell("the routine names no schedule to match".to_string());
    };
    let minute = match local_minute_of(now) {
        Ok(minute) => minute,
        Err(why) => return Due::CouldNotTell(why),
    };
    let matched = match cron_matches(schedule, &minute) {
        Ok(matched) => matched,
        Err(why) => return Due::CouldNotTell(why),
    };
    let stamp = crate::clock::stamp_secs(now - now % 60);
    if !matched {
        return Due::NotDue(format!("schedule `{schedule}` does not match {stamp}"));
    }
    if last_fired.is_some_and(|fired| fired / 60 == now / 60) {
        return Due::NotDue(format!(
            "schedule `{schedule}` matches {stamp}, and the routine already fired in that minute"
        ));
    }
    Due::Due(format!("schedule `{schedule}` matches {stamp}"))
}

fn cooldown(routine: &Routine, now: u64, last_fired: Option<u64>) -> Due {
    let Some(interval) = routine.interval else {
        return Due::CouldNotTell("the routine names no interval".to_string());
    };
    let Some(fired) = last_fired else {
        return Due::Due(format!("never fired; the interval is {interval}s"));
    };
    let elapsed = now.saturating_sub(fired);
    let sentence = format!(
        "{elapsed}s since {}, the interval is {interval}s",
        crate::clock::stamp_secs(fired)
    );
    if elapsed >= interval {
        Due::Due(sentence)
    } else {
        Due::NotDue(sentence)
    }
}

/// The routine §3 fixes, and the one place it is written: a check that outran its
/// bound or could not start is could-not-tell; a status the routine names is
/// could-not-tell, READ BEFORE the 0 test; 0 is due; anything else is not.
fn condition(routine: &Routine, run_check: &dyn Fn(&Routine) -> CheckOutcome) -> Due {
    if routine.check.is_none() {
        return Due::CouldNotTell("the routine names no check to run".to_string());
    }
    match run_check(routine) {
        CheckOutcome::Timeout(detail) => Due::CouldNotTell(format!(
            "the check did not finish within {}s: whether it is due is unknown ({detail})",
            routine.check_timeout
        )),
        CheckOutcome::NotStarted(detail) => {
            Due::CouldNotTell(format!("the check could not be run: {detail}"))
        }
        CheckOutcome::Exited(status) if routine.check_unknown_exit.contains(&status) => {
            Due::CouldNotTell(format!(
                "the check exited {status}, which this routine maps to could-not-tell"
            ))
        }
        CheckOutcome::Exited(0) => Due::Due("the check exited 0".to_string()),
        CheckOutcome::Exited(status) => Due::NotDue(format!("the check exited {status}")),
    }
}

/// When this routine is next expected to be asked, in epoch seconds. `None` is a
/// cron schedule with no matching minute inside the search limit, which a reader
/// sees as a next due of `none` rather than as a date nobody meant.
pub fn next_due(
    routine: &Routine,
    now: u64,
    last_fired: Option<u64>,
    last_evaluated: Option<u64>,
) -> Option<u64> {
    match routine.trigger {
        Trigger::Cron => {
            let schedule = routine.schedule.as_deref()?;
            let mut minute = now - now % 60;
            // A minute the routine has already fired in is behind it, whatever the
            // schedule says about it.
            if last_fired.is_some_and(|fired| fired / 60 == minute / 60) {
                minute += 60;
            }
            for _ in 0..SEARCH_LIMIT_MINUTES {
                let local = local_minute_of(minute).ok()?;
                if cron_matches(schedule, &local).ok()? {
                    return Some(minute);
                }
                minute += 60;
            }
            None
        }
        Trigger::Cooldown => Some(match (last_fired, routine.interval) {
            (Some(fired), Some(interval)) => fired.saturating_add(interval),
            _ => now,
        }),
        Trigger::Condition => Some(match last_evaluated {
            Some(seen) => seen.saturating_add(routine.poll),
            None => now,
        }),
    }
}

/// How long a routine waits between evaluations.
///
/// A cron routine is asked once per LOCAL MINUTE and not once per sixty seconds:
/// a span drifts forward by the poll interval at every evaluation, and a
/// schedule read that way skips a whole minute — the one its author wrote — once
/// every `60 / poll_seconds` evaluations. A cooldown has no minute of its own to
/// key on, so it takes the span; a condition takes its own `poll`.
pub fn is_due_an_evaluation(routine: &Routine, now: u64, last_evaluated: Option<u64>) -> bool {
    let Some(seen) = last_evaluated else {
        return true;
    };
    match routine.trigger {
        Trigger::Cron => seen / 60 != now / 60,
        Trigger::Cooldown => now.saturating_sub(seen) >= 60,
        Trigger::Condition => now.saturating_sub(seen) >= routine.poll,
    }
}
