//! `fleet ask` and `fleet answer` — the seat's blocking question and the reply
//! that settles it, over the store's own gate.
//!
//! ONE OBJECT AND ONE EVENT. A park is the store's gate on the item plus
//! `item.parked`, whoever raised it, so one listing shows everything owed and
//! one verb answers any of it. Neither verb here rings anybody: `ask` leaves a
//! seat about to be retired, and `answer` dispatches nothing.
//!
//! THE REFUSALS COME BEFORE THE COMMIT, as they do in `deliver`. Everything
//! `ask` can answer from the record and the note — the item the seat holds, an
//! epic, the trunk, a note the grammar does not read — is asked while nothing
//! has been written, so a refusal leaves the seat's worktree exactly where it
//! stood.
//!
//! WHAT `ask` COMMITS IS EVERYTHING (decision D1). A question asked mid-work
//! must lose nothing and the seat is retired the moment the flight reads the
//! park, so the staged set, the unstaged modification and the untracked file go
//! onto the branch together. A tree with nothing to commit parks on HEAD.
//!
//! A RUN'S RECORD IS PARKED WITHOUT GIT. The paragraph above is a SEAT's park:
//! a worktree, a work branch and a tree to commit. A run's record has none of
//! the three — the git wiring resolves to the project root, which is whoever's
//! checkout the run was started inside — so `ask` on one reads no branch,
//! stages nothing and commits nothing, and its park names [`RUN_BRANCH`] where
//! a seat's names its branch. Everything after the commit is the same act.
//!
//! THE QUESTION IS CARRIED TWICE AND WRITTEN ONCE. Its whole text is the gate's
//! reason, which is what a person meets on the store's gate list, and the same
//! text sits under the park note's four lines MOVED OFF COLUMN ZERO — so no
//! line a seat wrote inside its question can end the region it is written in.

use std::io::Write;
use std::path::Path;

use crate::item::brief::Packs;
use crate::item::deliver::held_item;
use crate::item::dispatch::refuse_an_epic;
use crate::item::run;
use crate::item::{
    control_token, label_value, last_answer, last_park, marker_block, opens_with, render, Events,
    Git, Project, Stop, ANSWER_MARKERS, GATE_RESOLVED, ITEM_PARKED, PARK_MARKERS, TRUNK_BRANCH,
};
use crate::store::{Item, Store, BD};

/// The park-note grammar, in core's pack and shadowable like every other asset.
pub const PARK_NOTE: &str = "assets/park-note.md";

/// What a reading nobody could take is written as, on the park note and in the
/// payload beside it. A blank line and an unread one are the same bytes and not
/// the same fact.
pub const UNREAD: &str = "(none)";

/// What a run's record parks on where a seat's park names its work branch. It
/// is not [`UNREAD`]: a branch nobody could read and an item that has no branch
/// at all are two different facts, and a reader cutting a worktree from a park
/// must meet the second as a value and not as a missing one.
pub const RUN_BRANCH: &str = "(run)";

/// The key the run's own hash sits under in the record's `run` object, written
/// by [`run::run`] at the open.
const HASH: &str = "hash";

/// The two grammars this pair reads, both of them slots in the pack.
pub const QUESTION_NOTE: &str = "assets/question-note.md";
pub const ANSWER_NOTE: &str = "assets/answer-note.md";

/// The one a question opens on. It is the SEAT's marker and not a note's: the
/// text lives inside a park region rather than opening one of its own.
pub const QUESTION_MARKERS: [&str; 1] = ["QUESTION"];

/// What the park note's reason reads as for each of the parks raised here: a
/// seat's own question, and a run the controller stopped executing at
/// `[core.run] max_crashes`, named by the key that stopped it.
pub const ASK: &str = "ask";
pub const CAPPED: &str = "max_crashes";

/// The label the park note carries the gate an answer resolves under.
pub const GATE: &str = "gate";

// ---- the question ------------------------------------------------------------

/// The question, as its arguments.
pub struct Question<'a> {
    /// The item, where the seat holds more than one and named it.
    pub item: Option<&'a str>,
    /// The seat asking.
    pub by: &'a str,
    /// The note the seat wrote, in the pack's question grammar.
    pub note: &'a Path,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
}

/// Everything the pair acts through. `git` and `project` are the question's; the
/// answer is a person's act on the record and touches no worktree.
pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub git: &'a dyn Git,
    pub packs: &'a Packs,
    pub project: &'a Project,
    pub events: &'a dyn Events,
}

/// The park made, for a caller that wants to say what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub item: String,
    pub gate: String,
    pub branch: String,
    pub commit: String,
    /// The park note as it was written.
    pub note: String,
}

pub fn ask(out: &mut dyn Write, question: &Question, wiring: &Wiring) -> Result<Asked, Stop> {
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

    let written = read_note(question.note)?;
    grammar_holds(&wiring.packs.read(QUESTION_NOTE)?, &written)?;

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
                    "{item}: parked — {} asked a question at {}",
                    question.by, question.at
                ))
                .map_err(Stop::could_not_tell)?
        }
    };

    // (b) THE GATE, whose id comes off the command's own answer. The open list
    // is read FIRST, because a create that fails can still have filed its gate
    // — bd 1.2.2 on an epic files it, refuses the edge and exits 1 — and the
    // listing names no item, so what the create left is what was not there
    // before it.
    let before = wiring.store.open_gates().map_err(|e| {
        parked(
            &item,
            &commit,
            &format!("the open gates could not be read before the gate was raised: {e}"),
        )
    })?;
    let gate = wiring
        .store
        .gate(&item, &written, question.by)
        .map_err(|e| {
            let stop = parked(&item, &commit, &format!("the gate was not raised: {e}"));
            Stop {
                message: format!(
                    "{}{}",
                    stop.message,
                    left_behind(wiring.store, &before, question.by)
                ),
                ..stop
            }
        })?;

    // (c) THE PARK NOTE, read back as the last park region.
    let note = park_note(wiring.packs, &item, ASK, &branch, &commit, &gate, &written)?;
    wiring.store.note(&item, &note, question.by).map_err(|e| {
        gated(
            &item,
            &commit,
            &gate,
            &format!("the park note did not land: {e}"),
        )
    })?;
    read_back(&item, &note, wiring.store)?;

    // (d) THE EVENT, after the note and its read-back.
    wiring
        .events
        .append(
            ITEM_PARKED,
            question.by,
            serde_json::json!({
                "item": item,
                "reason": ASK,
                "branch": branch,
                "commit": commit,
                "gate": gate,
            }),
        )
        .map_err(|e| {
            gated(
                &item,
                &commit,
                &gate,
                &format!("{ITEM_PARKED} did not reach the stream: {e}"),
            )
        })?;

    let _ = writeln!(out, "{gate}");
    Ok(Asked {
        item,
        gate,
        branch,
        commit,
        note,
    })
}

/// The hash a run is pinned to, off its record's own `run` object, which is
/// what a run stands on where a seat stands on a commit. A record whose open
/// never wrote the object reads [`UNREAD`] rather than refusing: the question
/// is the point of the park and the hash is context beside it.
fn run_hash(record: &Item) -> String {
    record
        .run
        .as_ref()
        .and_then(|object| object.get(HASH))
        .and_then(|value| value.as_str())
        .unwrap_or(UNREAD)
        .to_string()
}

fn read_note(path: &Path) -> Result<String, Stop> {
    std::fs::read_to_string(path).map_err(|e| {
        Stop::usage(format!(
            "the note at {} could not be read: {e} — `--note <file>` names the question the seat \
             wrote",
            path.display()
        ))
    })
}

/// The note the seat handed in, against the pack's grammar: the marker it opens
/// on, and at least one lettered option.
fn grammar_holds(template: &str, written: &str) -> Result<Vec<(char, String)>, Stop> {
    let Some(first) = written.lines().find(|line| !line.trim().is_empty()) else {
        return Err(Stop::usage(
            "the note is empty — a question is the one thing a person is being asked".to_string(),
        ));
    };
    if !opens_with(first, &QUESTION_MARKERS) {
        return Err(Stop::usage(format!(
            "the note opens on `{first}` — it opens on `{}` at column zero, which \
             `{QUESTION_NOTE}` names, or no reader can anchor on it",
            QUESTION_MARKERS[0]
        )));
    }
    let options = options_in(written);
    if options.is_empty() {
        return Err(Stop::usage(format!(
            "the note names no lettered option — `{QUESTION_NOTE}` names one per line as a \
             capital letter, a period and the text, and a question with none is a conversation:\n{}",
            marker_block(template, QUESTION_MARKERS[0]).unwrap_or_default()
        )));
    }
    Ok(options)
}

/// Every lettered option a text names, in its order.
///
/// The lines are TRIMMED before they are read, because the same function reads
/// a question as the seat wrote it and the same question moved off column zero
/// inside a park region.
pub fn options_in(text: &str) -> Vec<(char, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let mut letters = line.chars();
            let letter = letters.next()?;
            if !letter.is_ascii_uppercase() || letters.next()? != '.' {
                return None;
            }
            let said = line.get(2..)?.trim();
            (!said.is_empty()).then(|| (letter, said.to_string()))
        })
        .collect()
}

/// The four lines the pack's park grammar names, with the question beneath them.
fn park_note(
    packs: &Packs,
    item: &str,
    reason: &str,
    branch: &str,
    commit: &str,
    gate: &str,
    question: &str,
) -> Result<String, Stop> {
    let template = packs.read(PARK_NOTE)?;
    let block = marker_block(&template, PARK_MARKERS[0]).ok_or_else(|| {
        Stop::could_not_tell(format!(
            "`{PARK_NOTE}` carries no `{}` block — the pack's park grammar names one",
            PARK_MARKERS[0]
        ))
    })?;
    let head = render(
        &block,
        &[
            ("item", item),
            ("reason", reason),
            ("branch", branch),
            ("commit", commit),
            ("gate", gate),
        ],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{PARK_NOTE}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
        ))
    })?;
    Ok(format!("{head}\n{}", off_column_zero(question)))
}

/// Every line of a text moved off column zero, so no line of it can end the
/// region it is written inside.
fn off_column_zero(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("  {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// One read, asserting the park region against the note that was written — plus
/// a token nothing wrote.
fn read_back(item: &str, note: &str, store: &dyn Store) -> Result<(), Stop> {
    let read = store.show(item)?;
    let seen = read.notes.as_deref().and_then(last_park);
    if seen.as_deref().map(normalised) != Some(normalised(note)) {
        return Err(Stop::could_not_tell(format!(
            "{item} read back with its last park ==\n{}\n  wanted:\n{note}",
            seen.as_deref().unwrap_or("(absent)")
        )));
    }
    let control = control_token();
    if read.document.contains(control) {
        return Err(Stop::could_not_tell(format!(
            "the read-back on {item} carries {control}, which nothing wrote — the read is not \
             reading this item"
        )));
    }
    Ok(())
}

// ---- the crash cap -----------------------------------------------------------

/// A run's record parked at `[core.run] max_crashes`, as its arguments.
pub struct Capped<'a> {
    pub run: &'a str,
    /// Why, as the controller's run pass words it: how many executions nothing
    /// could classify, and the cap.
    pub reason: &'a str,
    /// The run's own directory, where the logs a person reads before
    /// answering are.
    pub directory: &'a Path,
    pub by: &'a str,
}

/// The gate on a run's record at `[core.run] max_crashes` and the park note
/// that names it, answered as the gate's own id.
///
/// THE SAME PARK `ask` MAKES ON A RUN'S RECORD, with the question written here
/// rather than by a seat: the gate carries the whole question as its reason,
/// and the note carries the four lines [`answer`] reads the gate off. A gate
/// with no note beside it is the one park `fleet answer` refuses — "carries no
/// park" — and its open gate blocks the record's close too, so the run it
/// stopped held a `[core.run] max_open` slot for good.
///
/// NO EVENT. `item.parked` is the controller's own line and its latch: the
/// pass writes it once this answers, so a park that stopped half way here is
/// asked for again on the next poll rather than announced.
///
/// A FAILURE AFTER THE GATE WITHDRAWS IT. The next poll raises a gate of its
/// own, and one left behind with no note naming it is exactly the unanswerable
/// gate this park exists not to leave.
pub fn park_at_the_cap(capped: &Capped, store: &dyn Store, packs: &Packs) -> Result<String, Stop> {
    let record = store.show(capped.run)?;
    let question = cap_question(capped);
    let gate = store.gate(capped.run, &question, capped.by).map_err(|e| {
        Stop::could_not_tell(format!(
            "the gate was not raised: {e}\n  {} carries no park",
            capped.run
        ))
    })?;
    let noted = park_note(
        packs,
        capped.run,
        CAPPED,
        RUN_BRANCH,
        &run_hash(&record),
        &gate,
        &question,
    )
    .and_then(|note| {
        store
            .note(capped.run, &note, capped.by)
            .map_err(|e| Stop::could_not_tell(format!("the park note did not land: {e}")))?;
        read_back(capped.run, &note, store)
    });
    noted.map(|()| gate.clone()).map_err(|stop| {
        let withdrawn = match store.resolve_gate(&gate, capped.by) {
            Ok(()) => format!("the gate {gate} is withdrawn and the next poll parks it again"),
            Err(e) => format!(
                "the gate {gate} STANDS on {} with no park naming it, and withdrawing it failed: \
                 {e}",
                capped.run
            ),
        };
        Stop {
            code: stop.code,
            message: format!("{}\n  {withdrawn}", stop.message),
        }
    })
}

/// The question a crash-cap park asks, in the question grammar: the pass's own
/// reading on the marker line, where the logs are, and one lettered option per
/// thing a person can do about it.
///
/// THE OPTIONS SAY WHAT EACH ONE DOES. An answer resolves the gate and does
/// nothing else — nothing executes a parked run again — so the letter records
/// the decision and the cancel verb is what acts on the first of them.
fn cap_question(capped: &Capped) -> String {
    let run = capped.run;
    format!(
        "{} {} — nothing executes it again.\n\
         Its stdout.log and stderr.log are in {}.\n\
         A. cancel it: `fleet cancel {run}` closes its record and resolves this gate, with or \
         without an answer\n\
         B. keep it for now: this answer resolves the gate, and the record stays open, holding a \
         `[core.run] max_open` slot, until it is cancelled\n",
        QUESTION_MARKERS[0],
        capped.reason,
        capped.directory.display()
    )
}

// ---- the answer --------------------------------------------------------------

/// A person's reply, as its arguments.
pub struct Reply<'a> {
    pub item: &'a str,
    /// The option's letter, as the person typed it.
    pub letter: &'a str,
    /// What they said beyond the letter, where the options did not carry it.
    pub text: Option<&'a str>,
    /// Who answered.
    pub by: &'a str,
}

/// The answer written, for a caller that wants to say what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replied {
    pub item: String,
    pub gate: String,
    pub letter: String,
    pub note: String,
}

pub fn answer(out: &mut dyn Write, reply: &Reply, wiring: &Wiring) -> Result<Replied, Stop> {
    // Resolved once: from here on the reply names the id the store answered,
    // so the note, the event and every refusal carry the full one.
    let read = wiring.store.show(reply.item)?;
    let reply = &Reply {
        item: &read.id,
        ..*reply
    };
    let notes = read.notes.clone().unwrap_or_default();
    let Some(park) = last_park(&notes) else {
        return Err(Stop::refused(format!(
            "{} carries no park — an answer settles a question somebody asked, and this item has \
             none",
            reply.item
        )));
    };
    let Some(gate) = label_value(&park, GATE) else {
        return Err(Stop::refused(format!(
            "{}'s last park names no `{GATE}:` — there is no object for an answer to resolve",
            reply.item
        )));
    };

    // THE OPEN LIST IS FILTERED BY THE PARK'S OWN GATE ID and by nothing else:
    // the listing answers which gates are open and never which item each one
    // blocks, so the item's own record is what ties the two together.
    let open = wiring.store.open_gates()?;
    if !open.contains(&gate) {
        return Err(Stop::refused(format!(
            "{}'s gate {gate} is not one the store lists open — it has been answered already, or \
             resolved by hand",
            reply.item
        )));
    }

    let letter = one_letter(reply.letter)?;
    let options = options_in(&park);
    let named = options
        .iter()
        .find(|(carried, _)| *carried == letter)
        .map(|(_, said)| said.clone());
    if named.is_none() && reply.text.is_none() {
        return Err(Stop::usage(format!(
            "the question on {} names no option `{letter}` — its options are {}, and a letter \
             outside them needs `--text <text>` saying what was decided",
            reply.item,
            if options.is_empty() {
                "none".to_string()
            } else {
                options
                    .iter()
                    .map(|(carried, _)| carried.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )));
    }

    let note = answer_note(wiring.packs, &gate, reply.by, letter, reply.text)?;
    wiring.store.note(reply.item, &note, reply.by)?;
    let seen = wiring
        .store
        .show(reply.item)?
        .notes
        .as_deref()
        .and_then(last_answer);
    if seen.as_deref().map(normalised) != Some(normalised(&note)) {
        return Err(Stop::could_not_tell(format!(
            "{} read back with its last answer ==\n{}\n  wanted:\n{note}",
            reply.item,
            seen.as_deref().unwrap_or("(absent)")
        )));
    }

    wiring.store.resolve_gate(&gate, reply.by)?;
    let still = wiring.store.open_gates()?;
    if still.contains(&gate) {
        return Err(Stop::could_not_tell(format!(
            "{gate} is still on the store's open list after it was resolved — the answer on {} \
             STANDS and the item is still blocked",
            reply.item
        )));
    }

    wiring
        .events
        .append(
            GATE_RESOLVED,
            reply.by,
            serde_json::json!({
                "item": reply.item,
                "gate": gate,
                "letter": letter.to_string(),
            }),
        )
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{GATE_RESOLVED} did not reach the stream: {e}\n  the answer on {} STANDS and \
                 {gate} is resolved",
                reply.item
            ))
        })?;

    let _ = writeln!(out, "{} answered {letter} — {gate} resolved", reply.item);
    Ok(Replied {
        item: reply.item.to_string(),
        gate,
        letter: letter.to_string(),
        note,
    })
}

fn one_letter(given: &str) -> Result<char, Stop> {
    let trimmed = given.trim();
    let mut chars = trimmed.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), None) if letter.is_ascii_alphabetic() => Ok(letter.to_ascii_uppercase()),
        _ => Err(Stop::usage(format!(
            "`{given}` is not a letter — an answer names one of the question's options by the \
             letter it carries"
        ))),
    }
}

fn answer_note(
    packs: &Packs,
    gate: &str,
    by: &str,
    letter: char,
    text: Option<&str>,
) -> Result<String, Stop> {
    let template = packs.read(ANSWER_NOTE)?;
    let block = marker_block(&template, ANSWER_MARKERS[0]).ok_or_else(|| {
        Stop::could_not_tell(format!(
            "`{ANSWER_NOTE}` carries no `{}` block — the pack's answer grammar names one",
            ANSWER_MARKERS[0]
        ))
    })?;
    render(
        &block,
        &[
            ("gate", gate),
            ("by", by),
            ("letter", &letter.to_string()),
            ("text", text.unwrap_or(UNREAD)),
        ],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{ANSWER_NOTE}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
        ))
    })
}

// ---- the stops ---------------------------------------------------------------

/// Whitespace normalised to single spaces, because the store keeps a note with
/// the wrapping it was written with and the comparison is about the words.
fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A failure after the commit and before the gate. The commit is real and the
/// message says so, because a caller that read this as "nothing happened" would
/// ask twice.
fn parked(item: &str, commit: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{why}\n  the commit {commit} STANDS on the work branch and {item} carries no park"
    ))
}

/// Every gate a failed create left open, withdrawn, and one line each saying
/// so — or saying that it stands and what resolves it.
///
/// WHAT IS NEW ON THE OPEN LIST IS THIS PARK'S, because the listing never names
/// the item a gate blocks. A gate open before the create is somebody else's and
/// is left alone.
fn left_behind(store: &dyn Store, before: &[String], by: &str) -> String {
    let after = match store.open_gates() {
        Ok(after) => after,
        Err(e) => {
            return format!(
                "\n  the open gates could not be read again: {e} — a gate the store raised all \
                 the same is not known, and `{BD} gate list` lists every open one"
            )
        }
    };
    after
        .iter()
        .filter(|gate| !before.contains(gate))
        .map(|gate| match store.resolve_gate(gate, by) {
            Ok(()) => {
                format!("\n  the store raised the gate {gate} all the same, and it is withdrawn")
            }
            Err(e) => format!(
                "\n  the store raised the gate {gate} all the same, and it STANDS with no park \
                 naming it — withdrawing it failed: {e}; `{BD} gate resolve {gate}` resolves it"
            ),
        })
        .collect()
}

/// A failure after the gate. The gate is on the store's list and a person will
/// meet it there, so the message names it rather than leaving one nobody can
/// tie to an item.
fn gated(item: &str, commit: &str, gate: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{why}\n  the commit {commit} STANDS on the work branch and the gate {gate} STANDS on \
         {item}"
    ))
}
