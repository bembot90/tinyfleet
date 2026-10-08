//! What licenses a landing: the accept it lands and, for a run, the reviewer's clearance.

use crate::entry::{Clearance, Timeline, Verdict};
use crate::item::Stop;
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::{Directory, SeatId};
use crate::store::Item;

// ---- the record ---------------------------------------------------------------

/// The commit the timeline's last verdict accepted, against the one this
/// landing was given — both whole shas, so they are compared as they are.
///
/// THE ACCEPT IS THE LANDING'S OWN REVIEWER'S (fleet-pl6 (c)): the entry was
/// appended by the closer's seat, or — where a run carries this landing — by
/// that run, which reviewed as the same seat. An accept any other actor wrote
/// is refused, naming both.
pub(super) fn accepted(
    item: &str,
    timeline: &Timeline,
    commit: &str,
    closer: &Actor,
    by_run: Option<&Item>,
) -> Result<String, Stop> {
    let Some((entry, reviewed)) = timeline.last_review() else {
        return Err(Stop::refused(format!(
            "{item} carries no verdict — a landing lands a review and there is none to land"
        )));
    };
    if reviewed.verdict == Verdict::Returned {
        return Err(Stop::refused(format!(
            "the last verdict on {item} is a return — the work went back and nothing has \
             accepted it since"
        )));
    }
    if reviewed.commit != commit {
        return Err(Stop::refused(format!(
            "the last verdict on {item} accepts {} and this landing was given {commit} — a \
             landing lands the commit the review read",
            reviewed.commit
        )));
    }
    let its_run = by_run.map(|record| Actor {
        kind: ActorKind::Run,
        id: record.id.to_string(),
    });
    if entry.by != *closer && Some(&entry.by) != its_run.as_ref() {
        return Err(Stop::refused(format!(
            "the last verdict on {item} was written by {}, and this landing closes as {closer} — \
             a landing lands its own reviewer's accept",
            entry.by
        )));
    }
    Ok(reviewed.commit.clone())
}

/// The refusal of an actor that is neither a seat nor a run.
pub(super) fn neither(by: &Actor) -> Stop {
    Stop::refused(format!(
        "fleet land acts as a seat or as a run — {by} is neither"
    ))
}

/// The reviewer's own clearance of the hold this run raised about the item,
/// which is the whole licence for a run to land it (fleet-zlk D8).
///
/// The hold is raised on the RUN's record and cleared there, so that timeline
/// is where the licence is read; the landing it licenses is on the item. The
/// hold read is the LAST held entry whose `about` names the item — at this
/// commit, where it names one — and it licenses the landing only where its
/// clearance is an answer, by the reviewer's seat, with the letter `about`
/// says licenses it. Each piece missing is its own refusal.
pub(super) fn licensed(
    run: &str,
    timeline: &Timeline,
    item: &str,
    commit: &str,
    reviewer: SeatId,
    seats: &Directory,
) -> Result<(), Stop> {
    let wanted = seats.label(&reviewer);
    let about_this = timeline
        .holds_about(item, commit)
        .and_then(|(_, held)| held.about.as_ref().map(|about| (&held.hold, about)));
    let Some((hold, about)) = about_this else {
        return Err(Stop::refused(format!(
            "run {run} raised no hold about {item} — a run lands what {wanted} cleared, and \
             nobody was asked about this item"
        )));
    };
    let Some((cleared_by, cleared)) = timeline.clearance(hold) else {
        return Err(Stop::refused(format!(
            "run {run}'s hold {hold} about {item} is not cleared yet"
        )));
    };
    if cleared.how == Clearance::Cancel {
        return Err(Stop::refused(format!(
            "run {run}'s hold {hold} about {item} was cancelled, and a cancel licenses nothing"
        )));
    }
    if cleared_by.by.seat_id() != Some(reviewer) {
        return Err(Stop::refused(format!(
            "run {run}'s hold {hold} was cleared by {} and not by {wanted} — a run lands as the \
             [core] reviewer and on that seat's own clearance",
            cleared_by.by
        )));
    }
    let letter = cleared.letter.as_deref().unwrap_or_default();
    if letter != about.licenses {
        return Err(Stop::refused(format!(
            "run {run}'s hold {hold} was cleared {letter}, and {} is the letter that licenses a \
             landing",
            about.licenses
        )));
    }
    Ok(())
}
