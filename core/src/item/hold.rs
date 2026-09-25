//! `fleet hold` and `fleet clear` — the seat's blocking question, raised as a
//! hold over the store's own object, and the clearance that settles it.
//!
//! ONE OBJECT AND ONE ENTRY. A park is the store's hold on the item plus the
//! `held` entry naming it, whoever raised it, so one listing shows everything
//! owed and one verb clears any of it. Neither verb here rings anybody: `hold` leaves a
//! seat about to be retired, and `clear` dispatches nothing.
//!
//! THE REFUSALS COME BEFORE THE COMMIT, as they do in `deliver`. Everything
//! `hold` can answer from the record and the question file — the item the seat
//! holds, an epic, the trunk, a file that does not read as a [`QuestionInput`]
//! — is asked while nothing has been written, so a refusal leaves the seat's
//! worktree exactly where it stood.
//!
//! WHAT `hold` COMMITS IS EVERYTHING (decision D1). A question asked mid-work
//! must lose nothing and the seat is retired the moment the flight reads the
//! park, so the staged set, the unstaged modification and the untracked file go
//! onto the branch together. A tree with nothing to commit parks on HEAD.
//!
//! A RUN'S RECORD IS HELD WITHOUT GIT. The paragraph above is a SEAT's park:
//! a worktree, a work branch and a tree to commit. A run's record has none of
//! the three — the git wiring resolves to the project root, which is whoever's
//! checkout the run was started inside — so `hold` on one reads no branch,
//! stages nothing and commits nothing, and its held entry stands on the run's
//! hash where a seat's stands on a branch and a commit. Everything after the
//! commit is the same act.
//!
//! THE QUESTION IS CARRIED TWICE. The seat hands it in as JSON: [`question_text`]
//! renders it whole as the hold's reason, which is what a person meets on the
//! store's hold list, and the `held` entry carries it typed — the question,
//! its context, its lettered options and what it is about — which is what
//! `clear` checks a letter against and a run's landing reads its licence off.

use std::io::Write;
use std::path::Path;

use crate::entry::{self, Body, Choice, HoldReason, Timeline};
use crate::input::{self, QuestionInput, Standing, QUESTION_SCHEMA};
use crate::item::deliver::held_item;
use crate::item::dispatch::refuse_an_epic;
use crate::item::run;
use crate::item::{
    recorded, signal, Events, Git, Project, Stop, Unrecorded, ITEM_ENTRY, TRUNK_BRANCH,
};
use crate::seat::actor::Actor;
use crate::store::bd::BD;
use crate::store::{HoldId, Item, ItemId, Store};

/// What a run's park answers as its branch, where a seat's answers its work
/// branch: a run's record has no branch at all, and a caller printing the park
/// must meet that as a value and not as a missing one.
pub const RUN_BRANCH: &str = "(run)";

// ---- the question ------------------------------------------------------------

/// The question, as its arguments.
pub struct Question<'a> {
    /// The item, where the seat holds more than one and named it.
    pub item: Option<&'a str>,
    /// Who is asking: a seat, or a run parking its own record.
    pub by: &'a Actor,
    /// The question the seat wrote, a JSON file of the shape
    /// [`QUESTION_SCHEMA`] gives.
    pub question: &'a Path,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
}

/// Everything the pair acts through. `git` and `project` are the question's; the
/// clearance is a person's act on the record and touches no worktree.
pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub git: &'a dyn Git,
    pub project: &'a Project,
    pub events: &'a dyn Events,
}

/// The park made, for a caller that wants to say what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub item: String,
    pub hold: String,
    pub branch: String,
    pub commit: String,
    /// The held entry's id, as the store keeps it.
    pub entry: String,
}

pub fn hold(out: &mut dyn Write, question: &Question, wiring: &Wiring) -> Result<Held, Stop> {
    let item = held_item(wiring.store, question.by, question.item)?;
    // WHICH OF THE TWO PARKS THIS IS, off the record the store already answers:
    // the run label is the only mark that tells a run's record from every other
    // item, and a run's park touches no git at all.
    let record = wiring.store.show(&item)?;
    refuse_an_epic(&record)?;
    let of_a_run = record.labels.iter().any(|label| label == run::LABEL);

    let branch = if of_a_run {
        RUN_BRANCH.to_string()
    } else {
        let branch = wiring.git.current_branch().map_err(Stop::could_not_tell)?;
        if branch == TRUNK_BRANCH {
            return Err(Stop::refused(format!(
                "the worktree at {} is on `{branch}` — a park records the branch the work is on, \
                 and the trunk is nobody's work branch",
                wiring.project.root.display()
            )));
        }
        branch
    };

    // THE FILE, read before anything is written: a question that does not
    // read is a usage stop naming its schema, with the tree where it stood.
    let asked: QuestionInput = input::read(question.question, "question", QUESTION_SCHEMA)?;
    let written = question_text(&asked);

    // (a) THE COMMIT, AND EVERYTHING IN IT. `add_all` first, then the index:
    // a tree that answers nothing staged after it is a tree with nothing to
    // commit, and the park records HEAD rather than a commit of no changes.
    // A run's record stands on the hash its open pinned instead.
    let commit = if of_a_run {
        run_hash(&record)
    } else {
        wiring.git.add_all().map_err(Stop::could_not_tell)?;
        let staged = wiring.git.staged().map_err(Stop::could_not_tell)?;
        if staged.is_empty() {
            wiring.git.head().map_err(Stop::could_not_tell)?
        } else {
            wiring
                .git
                .commit(&format!(
                    "{item}: held — {} asked a question at {}",
                    question.by, question.at
                ))
                .map_err(Stop::could_not_tell)?
        }
    };

    // (b) THE HOLD, whose id comes off the command's own answer. The open list
    // is read FIRST, because a create that fails can still have filed its hold
    // — bd 1.2.2 on an epic filed it, refused the edge and exited 1; 1.3.0
    // files both and exits 0, measured, but a bd off the pin still runs here —
    // and the listing names no item, so what the create left is what was not
    // there before it.
    let before = wiring.store.holds_open().map_err(|e| {
        parked(
            &item,
            &commit,
            &format!("the open holds could not be read before the hold was raised: {e}"),
        )
    })?;
    let hold = wiring
        .store
        .hold_raise(&ItemId::from(item.as_str()), &written, question.by)
        .map_err(|e| {
            let stop = parked(&item, &commit, &format!("the hold was not raised: {e}"));
            Stop {
                message: format!(
                    "{}{}",
                    stop.message,
                    left_behind(wiring.store, &before, question.by)
                ),
                ..stop
            }
        })?
        .to_string();

    // (c) THE HELD ENTRY, appended and read back through the one helper every
    // entry writer goes through, under the hold the store just answered.
    let standing = if of_a_run {
        Standing::Run {
            hash: commit.clone(),
        }
    } else {
        Standing::Work {
            branch: branch.clone(),
            commit: commit.clone(),
        }
    };
    let entry = Body::Held(asked.into_held(hold.clone(), HoldReason::Ask, standing));
    let entry = recorded(wiring.store, &item, &entry, question.by).map_err(|unrecorded| {
        let why = match unrecorded {
            Unrecorded::NotWritten(e) => format!("the held entry did not land: {e}"),
            Unrecorded::Unconfirmed(why) => why,
        };
        held(&item, &commit, &hold, &why)
    })?;

    // (d) THE HELD ENTRY'S SIGNAL, after the entry and its read-back.
    signal(wiring.events, question.by, &item, &entry, "held").map_err(|e| {
        held(
            &item,
            &commit,
            &hold,
            &format!("{ITEM_ENTRY} did not reach the stream: {e}"),
        )
    })?;

    let _ = writeln!(out, "{hold}");
    Ok(Held {
        item,
        hold,
        branch,
        commit,
        entry,
    })
}

/// The hash a run is pinned to, off its run record, which is what a run
/// stands on where a seat stands on a commit. A record whose open never wrote
/// the run record reads `(none)` rather than refusing: the question is the
/// point of the park and the hash is context beside it.
fn run_hash(record: &Item) -> String {
    record
        .run
        .as_ref()
        .map(|run| run.hash.clone())
        .unwrap_or_else(|| String::from("(none)"))
}

/// A question as a person reads it: the question, its context on the next line
/// where it carries one, and one `<letter>. <text>` line per option.
///
/// It is the store hold's reason, so the store's own hold list shows the whole
/// question.
pub fn question_text(question: &QuestionInput) -> String {
    let mut lines = vec![question.question.clone()];
    lines.extend(question.context.clone());
    lines.extend(
        question
            .options
            .iter()
            .map(|choice| format!("{}. {}", choice.letter, choice.text)),
    );
    lines.join("\n")
}

// ---- the crash cap -----------------------------------------------------------

/// A run's record held at `[core.run] max_crashes`, as its arguments.
pub struct Capped<'a> {
    pub run: &'a str,
    /// Why, as the controller's run pass words it: how many executions nothing
    /// could classify, and the cap.
    pub reason: &'a str,
    /// The run's own directory, where the logs a person reads before
    /// clearing are.
    pub directory: &'a Path,
    pub by: &'a Actor,
}

/// The hold on a run's record at `[core.run] max_crashes` and the held entry
/// that names it, answered as the hold's own id and the entry's.
///
/// THE SAME PARK `hold` MAKES ON A RUN'S RECORD, with the question written
/// here rather than by a seat: the hold carries the whole question as its
/// reason, and ONE `held` entry — reason `max_crashes`, on the run's hash — is
/// the record of it, which [`clear`] reads the hold off. A hold with no entry
/// beside it is one `fleet clear` refuses — "carries no open hold" — and its
/// open hold blocks the record's close too, so the run it stopped would hold
/// a `[core.run] max_open` slot until it is cancelled.
///
/// NO EVENT. The entry's signal is the controller's own line: the pass writes
/// it once this answers, by the entry's id. The latch is the entry itself —
/// the pass asks the record whether it carries a `max_crashes` hold before it
/// parks — so a park that stopped before its entry is asked for again on the
/// next poll, and one whose signal did not land is not parked twice.
///
/// A FAILURE AFTER THE HOLD WITHDRAWS IT. The next poll raises a hold of its
/// own, and one left behind with no entry naming it is exactly the hold this
/// park exists not to leave.
pub fn park_at_the_cap(capped: &Capped, store: &dyn Store) -> Result<(String, String), Stop> {
    let record = store.show(capped.run)?;
    let question = cap_question(capped);
    let hold = store
        .hold_raise(&record.id, &question_text(&question), capped.by)
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "the hold was not raised: {e}\n  {} carries no park",
                capped.run
            ))
        })?;
    let entry = Body::Held(question.into_held(
        hold.to_string(),
        HoldReason::MaxCrashes,
        Standing::Run {
            hash: run_hash(&record),
        },
    ));
    recorded(store, capped.run, &entry, capped.by)
        .map(|entry| (hold.to_string(), entry))
        .map_err(|unrecorded| {
            let why = match unrecorded {
                Unrecorded::NotWritten(e) => format!("the held entry did not land: {e}"),
                Unrecorded::Unconfirmed(why) => why,
            };
            let withdrawn = match store.hold_clear(&hold, capped.by) {
                Ok(()) => format!("the hold {hold} is withdrawn and the next poll parks it again"),
                Err(e) => format!(
                    "the hold {hold} STANDS on {} with no held entry naming it, and withdrawing \
                     it failed: {e}",
                    capped.run
                ),
            };
            Stop::could_not_tell(format!("{why}\n  {withdrawn}"))
        })
}

/// The question a crash-cap park asks, as the input a seat's question is: the
/// pass's own reading as the question, where the logs are as its context, and
/// one lettered option per thing a person can do about it.
///
/// THE OPTIONS SAY WHAT EACH ONE DOES. A clearance clears the hold and does
/// nothing else — nothing executes a held run again — so the letter records
/// the decision and the cancel verb is what acts on the first of them.
pub fn cap_question(capped: &Capped) -> QuestionInput {
    let run = capped.run;
    let choice = |letter: &str, text: String| Choice {
        letter: letter.to_string(),
        text,
    };
    QuestionInput {
        question: format!("{} — nothing executes it again.", capped.reason),
        context: Some(format!(
            "Its stdout.log and stderr.log are in {}.",
            capped.directory.display()
        )),
        options: vec![
            choice(
                "A",
                format!(
                    "cancel it: fleet cancel {run} closes its record and clears this hold, with \
                     or without an answer"
                ),
            ),
            choice(
                "B",
                String::from(
                    "keep it for now: this answer clears the hold, and the record stays open, \
                     holding a [core.run] max_open slot, until it is cancelled",
                ),
            ),
        ],
        about: None,
    }
}

// ---- the clearance -----------------------------------------------------------

/// A person's clearance, as its arguments.
pub struct Clearance<'a> {
    pub item: &'a str,
    /// The option's letter, as the person typed it.
    pub letter: &'a str,
    /// What they said beyond the letter, where the options did not carry it.
    pub text: Option<&'a str>,
    /// Who cleared it.
    pub by: &'a Actor,
}

/// The clearance written, for a caller that wants to say what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleared {
    pub item: String,
    pub hold: String,
    pub letter: String,
    /// The cleared entry's id, as the store keeps it.
    pub entry: String,
}

pub fn clear(out: &mut dyn Write, clearance: &Clearance, wiring: &Wiring) -> Result<Cleared, Stop> {
    // Resolved once: from here on the clearance names the id the store
    // answered, so the entry, the event and every refusal carry the full one.
    let read = wiring.store.show(clearance.item)?;
    let clearance = &Clearance {
        item: &read.id,
        ..*clearance
    };
    // THE QUESTION IS THE TIMELINE'S OPEN HOLD: the last held entry no
    // cleared entry naming its hold came after.
    let entries = wiring.store.timeline(&read.id)?;
    let Some((_, held)) = Timeline(&entries).open_hold() else {
        return Err(Stop::refused(format!(
            "{} carries no open hold — a clearance settles a question somebody asked, and this \
             item has none",
            clearance.item
        )));
    };
    let hold = held.hold.clone();

    // THE OPEN LIST IS FILTERED BY THE ENTRY'S OWN HOLD ID and by nothing else:
    // the listing answers which holds are open and never which item each one
    // blocks, so the item's own record is what ties the two together.
    let open = wiring.store.holds_open()?;
    if !open.iter().any(|held| *held == hold) {
        return Err(Stop::refused(format!(
            "{}'s hold {hold} is not one the store lists open — it has been cleared already, or \
             by hand",
            clearance.item
        )));
    }

    let letter = one_letter(clearance.letter)?;
    let named = held
        .options
        .iter()
        .any(|choice| choice.letter == letter.to_string());
    if !named && clearance.text.is_none() {
        return Err(Stop::usage(format!(
            "the question on {} names no option `{letter}` — its options are {}, and a letter \
             outside them needs `--text <text>` saying what was decided",
            clearance.item,
            if held.options.is_empty() {
                "none".to_string()
            } else {
                held.options
                    .iter()
                    .map(|choice| choice.letter.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )));
    }

    // THE CLEARED ENTRY, by the clearer: the answer a run's landing reads its
    // licence off, appended and read back before the store's hold is cleared.
    let answer = Body::Cleared(entry::Cleared {
        hold: hold.clone(),
        how: entry::Clearance::Answer,
        letter: Some(letter.to_string()),
        text: clearance.text.map(String::from),
    });
    let entry =
        recorded(wiring.store, clearance.item, &answer, clearance.by).map_err(|unrecorded| {
            match unrecorded {
                Unrecorded::NotWritten(e) => Stop::from(e),
                Unrecorded::Unconfirmed(why) => Stop::could_not_tell(format!(
                    "{why}\n  {hold} is not cleared and {} is still blocked",
                    clearance.item
                )),
            }
        })?;

    wiring
        .store
        .hold_clear(&HoldId::from(hold.as_str()), clearance.by)?;
    let still = wiring.store.holds_open()?;
    if still.iter().any(|held| *held == hold) {
        return Err(Stop::could_not_tell(format!(
            "{hold} is still on the store's open list after it was cleared — the answer on {} \
             STANDS and the item is still blocked",
            clearance.item
        )));
    }

    signal(
        wiring.events,
        clearance.by,
        clearance.item,
        &entry,
        "cleared",
    )
    .map_err(|e| {
        Stop::could_not_tell(format!(
            "{ITEM_ENTRY} did not reach the stream: {e}\n  the answer on {} STANDS and {hold} is \
             cleared",
            clearance.item
        ))
    })?;

    let _ = writeln!(out, "{} answered {letter} — {hold} cleared", clearance.item);
    Ok(Cleared {
        item: clearance.item.to_string(),
        hold,
        letter: letter.to_string(),
        entry,
    })
}

fn one_letter(given: &str) -> Result<char, Stop> {
    let trimmed = given.trim();
    let mut chars = trimmed.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), None) if letter.is_ascii_alphabetic() => Ok(letter.to_ascii_uppercase()),
        _ => Err(Stop::usage(format!(
            "`{given}` is not a letter — a clearance names one of the question's options by the \
             letter it carries"
        ))),
    }
}

// ---- the stops ---------------------------------------------------------------

/// A failure after the commit and before the hold. The commit is real and the
/// message says so, because a caller that read this as "nothing happened" would
/// ask twice.
fn parked(item: &str, commit: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{why}\n  the commit {commit} STANDS on the work branch and {item} carries no park"
    ))
}

/// Every hold a failed create left open, withdrawn, and one line each saying
/// so — or saying that it stands and what clears it.
///
/// WHAT IS NEW ON THE OPEN LIST IS THIS PARK'S, because the listing never names
/// the item a hold blocks. A hold open before the create is somebody else's and
/// is left alone.
fn left_behind(store: &dyn Store, before: &[HoldId], by: &Actor) -> String {
    let after = match store.holds_open() {
        Ok(after) => after,
        Err(e) => {
            return format!(
                "\n  the open holds could not be read again: {e} — a hold the store raised all \
                 the same is not known, and `{BD} gate list` lists every open one"
            )
        }
    };
    after
        .iter()
        .filter(|hold| !before.contains(hold))
        .map(|hold| match store.hold_clear(hold, by) {
            Ok(()) => {
                format!("\n  the store raised the hold {hold} all the same, and it is withdrawn")
            }
            Err(e) => format!(
                "\n  the store raised the hold {hold} all the same, and it STANDS with no park \
                 naming it — withdrawing it failed: {e}; `{BD} gate resolve {hold}` clears it"
            ),
        })
        .collect()
}

/// A failure after the hold. The hold is on the store's list and a person will
/// meet it there, so the message names it rather than leaving one nobody can
/// tie to an item.
fn held(item: &str, commit: &str, hold: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{why}\n  the commit {commit} STANDS on the work branch and the hold {hold} STANDS on \
         {item}"
    ))
}
