//! `fleet deliver` — the seat's own handoff, and the only writer of a delivery
//! (packs PRD R6; cli PRD § `fleet deliver`).
//!
//! THE REFUSALS COME BEFORE THE COMMIT. Every question this verb can answer
//! from the record and the working tree — the branch, the unstaged file, the
//! empty index, the missing reviewer, a note the grammar cannot anchor on — is
//! asked while nothing has been written, so a refusal leaves a seat exactly
//! where it stood. The one check that cannot is the commit line's, which has no
//! subject until the commit exists. The tree's three questions come FIRST,
//! before the store is called at all.
//!
//! A CLEAN TREE AHEAD OF THE BASE IS ITSELF THE DELIVERY. Nothing staged and
//! HEAD ahead of the trunk ref is a seat resuming after `fleet hold`: the
//! delivery is the commit HEAD already names and this verb commits nothing.
//! Nothing staged and HEAD AT the trunk ref is a seat that built nothing, and
//! is refused.
//!
//! THE NOTE IS THE SEAT'S, THE MACHINE LINES ARE THIS VERB'S. A builder writes
//! the note in the pack's delivery-note grammar and hands it in; deliver fills
//! the three lines only a process knows — the commit, the branch, the base —
//! and the marker line, and touches nothing else. What the note says about the
//! work is the seat's word and is carried through verbatim.

use std::io::Write;
use std::path::Path;

use crate::item::brief::{Packs, DELIVERY_NOTE};
use crate::item::dispatch::EPIC;
use crate::item::{
    control_token, label_value, last_delivery, opens_with, Events, Git, Project, Ring, RingOutcome,
    Stop, DELIVERY_MARKERS, ITEM_DELIVERED, TRUNK, TRUNK_BRANCH,
};
use crate::policy;
use crate::store::{AssignedItem, Item, Store};

/// The three lines the verb fills. Everything else in the grammar is the
/// seat's.
pub const COMMIT: &str = "commit";
pub const BRANCH: &str = "branch";
pub const BASE: &str = "base";

/// What the ring carries: where to look and what to look at. The record is the
/// item, as it is for every other ring this crate sends.
pub const RING: &str = "{item} is delivered at {commit} and is yours to review. \
     Read the item and act on the record.";

/// The line an absent reviewer's delivery prints. The delivery stands: the
/// reassignment recorded the handoff, and the reviewer's successor reads the
/// item at wake (packs PRD § What every verb refuses to guess, rule 4).
pub const STANDS: &str = "DELIVERED, NOT RUNG";

/// The line a clean tree's delivery prints. The seat staged nothing and nothing
/// was committed: the delivery is the commit HEAD already named, and a seat
/// that read this as "committed for me" would look for a commit that is not
/// there.
pub const AS_IS: &str = "DELIVERED AS-IS";

/// The delivery, as its arguments.
pub struct Delivery<'a> {
    /// The item, where the seat holds more than one and named it.
    pub item: Option<&'a str>,
    /// The seat delivering.
    pub by: &'a str,
    /// The note the seat wrote, in the pack's delivery-note grammar.
    pub note: &'a Path,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
}

/// Everything the verb acts through.
pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub git: &'a dyn Git,
    pub packs: &'a Packs,
    pub project: &'a Project,
    pub ring: &'a dyn Ring,
    pub events: &'a dyn Events,
}

/// The delivery made, for a caller that wants to say what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub item: String,
    pub commit: String,
    pub reviewer: String,
    pub note: String,
}

pub fn deliver(
    out: &mut dyn Write,
    err: &mut dyn Write,
    delivery: &Delivery,
    wiring: &Wiring,
) -> Result<Delivered, Stop> {
    // THE TREE IS READ BEFORE THE STORE. A store call of this verb's own can
    // touch a file the project versions — an append-only log beside the
    // database is the shape that does it — and a gate that read the tree after
    // one would report this verb's own bookkeeping as the seat's unstaged work.
    //
    // (a) the trunk, (b) a file left outside the delivery, (c) an empty one.
    let branch = wiring.git.current_branch().map_err(step)?;
    if branch == TRUNK_BRANCH {
        return Err(Stop::refused(format!(
            "the worktree is on `{branch}` — a delivery is a handoff of a work branch and the \
             reviewer takes a commit, never the trunk"
        )));
    }
    let staged = wiring.git.staged().map_err(step)?;
    if let Some(loose) = outside(&wiring.git.status().map_err(step)?, &staged) {
        return Err(Stop::refused(format!(
            "`{loose}` is changed in the working tree and not staged — the delivery is the staged \
             set, and a file outside it is the seat's to stage or to put back"
        )));
    }
    // A CLEAN TREE IS A DELIVERY ONLY WHERE IT IS AHEAD OF THE BASE. HEAD
    // against the trunk ref is what separates a seat resuming after `fleet hold`
    // — held commit, complete work, nothing left to stage — from a seat that
    // built nothing; only the second is refused.
    let base = wiring.git.trunk_tip().map_err(step)?;
    let standing = if staged.is_empty() {
        let head = wiring.git.head().map_err(step)?;
        if head == base {
            return Err(Stop::refused(format!(
                "nothing is staged in {} and HEAD is {TRUNK} at {head} — a note with no commit is \
                 no delivery.\n  A worktree RESUMED AFTER `fleet hold` delivers its held commit \
                 as it stands, with nothing staged, when HEAD is AHEAD of {TRUNK}. This HEAD is \
                 the base itself: `git log --oneline {TRUNK}..HEAD` prints nothing here.\n  So: \
                 stage the work and run this again, or — if the work is committed on another \
                 branch — put this worktree on the branch that holds it and run this again.",
                wiring.project.root.display()
            )));
        }
        Some(head)
    } else {
        None
    };

    let written = read_note(delivery.note)?;
    grammar_holds(&wiring.packs.read(DELIVERY_NOTE)?, &written)?;

    let item = held_item(wiring.store, delivery.by, delivery.item)?;
    let reviewer = reviewer_of(wiring.project)?;

    let as_is = standing.is_some();
    let commit = match standing {
        Some(head) => head,
        None => wiring
            .git
            .commit(&format!("{item}: delivered by {}", delivery.by))
            .map_err(step)?,
    };

    let note = fill(&written, &commit, &branch, &base, delivery)?;
    // The post-condition of the fill, asked of the text that will be written
    // rather than of the values it was built from.
    if label_value(&note, COMMIT).as_deref() != Some(commit.as_str()) {
        return Err(Stop::could_not_tell(format!(
            "the rendered note's `{COMMIT}:` line reads {} and the commit is {commit}",
            label_value(&note, COMMIT).unwrap_or_else(|| "(absent)".to_string())
        )));
    }

    wiring
        .store
        .assign(&item, &reviewer, delivery.by)
        .map_err(|e| {
            committed(
                &item,
                &commit,
                &format!("the reassignment did not land: {e}"),
            )
        })?;
    wiring.store.note(&item, &note, delivery.by).map_err(|e| {
        committed(
            &item,
            &commit,
            &format!("the delivery note did not land: {e}"),
        )
    })?;
    read_back(&item, &note, &reviewer, wiring)?;
    announce(&item, &commit, &branch, &base, delivery, wiring)?;

    if as_is {
        let _ = writeln!(
            out,
            "{AS_IS}: nothing was staged and HEAD {commit} is ahead of {TRUNK} at {base} — the \
             delivery on {item} is that commit and this verb committed nothing"
        );
    }
    ring(out, err, &item, &commit, &reviewer, wiring);
    Ok(Delivered {
        item,
        commit,
        reviewer,
        note,
    })
}

/// The one event this verb writes (flights PRD Q4a).
///
/// AFTER THE READ-BACK AND BEFORE THE DOORBELL: the note is written and read
/// back first, so a crash between the two leaves a delivery nothing announced
/// and never an announcement with no delivery behind it. The three values are
/// the three machine lines the note carries, taken from the same variables that
/// filled them rather than parsed back out of the text.
fn announce(
    item: &str,
    commit: &str,
    branch: &str,
    base: &str,
    delivery: &Delivery,
    wiring: &Wiring,
) -> Result<(), Stop> {
    wiring
        .events
        .append(
            ITEM_DELIVERED,
            delivery.by,
            serde_json::json!({
                "item": item,
                "commit": commit,
                "branch": branch,
                "base": base,
            }),
        )
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{ITEM_DELIVERED} did not reach the stream: {e}\n  the delivery on {item} STANDS \
                 and the commit {commit} is on the work branch"
            ))
        })
}

/// The reviewer, and the doorbell. Every outcome is exit 0: the writes landed,
/// and a delivery whose writes landed is done whether or not the doorbell rang.
fn ring(
    out: &mut dyn Write,
    err: &mut dyn Write,
    item: &str,
    commit: &str,
    reviewer: &str,
    wiring: &Wiring,
) {
    let text = crate::item::render(RING, &[("item", item), ("commit", commit)])
        .unwrap_or_else(|_| format!("{item} is delivered at {commit}"));
    match wiring.ring.ring(reviewer, &text) {
        RingOutcome::Delivered => {}
        RingOutcome::Absent => {
            let _ = writeln!(
                out,
                "{STANDS}: no live session for {reviewer}; {item} is theirs and their successor \
                 reads it at wake"
            );
        }
        RingOutcome::Failed(cause) => {
            let _ = writeln!(err, "{STANDS}: {cause}; the delivery stands");
        }
    }
}

// ---- the item ----------------------------------------------------------------

/// The item this worktree is acting on: the one item the seat [`holds`].
///
/// `named` says which one directly, for the case a seat legitimately holds two.
///
/// IT IS `ask`'s READ TOO (flights PRD S4c). A question and a delivery ask the
/// same question of the record — which item is this worktree's — and two
/// readers of it would be two answers the day one of them changed.
pub fn held_item(store: &dyn Store, by: &str, named: Option<&str>) -> Result<String, Stop> {
    if let Some(named) = named {
        // Resolved once, here: the caller acts on the store's full id and
        // never on the part of it that was typed.
        let item = read(store, named)?;
        return Ok(item.id);
    }
    let Holds { open, held, .. } = holds(store, by)?;
    let mut held: Vec<String> = held.into_iter().map(|row| row.id).collect();
    match held.len() {
        1 => Ok(held.remove(0)),
        0 => Err(Stop::refused(format!(
            "`{by}` holds no open ordered item{} — work is given, and an act on an item answers \
             an order",
            if open.is_empty() {
                String::new()
            } else {
                format!(
                    " (it holds {}, none of them an ordered item that is not an epic)",
                    open.iter()
                        .map(|row| row.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ))),
        _ => Err(Stop::refused(format!(
            "`{by}` holds {} ordered items — {} — and `--item <id>` says which one this is",
            held.len(),
            held.join(", ")
        ))),
    }
}

/// What a seat is carrying, off ONE listing and no per-row read.
pub(crate) struct Holds {
    /// Every open row assigned to the seat.
    pub(crate) open: Vec<AssignedItem>,
    /// Of those, every one that carries a `fleet.orders` key, whatever its
    /// type: the orders standing against the seat's name.
    pub(crate) ordered: Vec<AssignedItem>,
    /// Of those, every one that is not an epic: what the seat HOLDS.
    pub(crate) held: Vec<AssignedItem>,
}

/// THE ONE READING of what a seat holds: an open item assigned to it that
/// carries a `fleet.orders` key and is not an epic. [`held_item`] and dispatch's
/// one-item-at-a-time refusal both ask it, and an item one of them counted and
/// the other did not is a seat refused a dispatch over work nobody gave it — a
/// bug assigned a month ago and never ordered, an epic still naming an old
/// assignee.
///
/// A retire asks the wider half, `ordered`: an order left standing against a
/// retired name is inherited by the next seat of that name whatever the item's
/// type, so a retire withdraws an ordered epic that no seat holds.
///
/// Whether a row is ordered and whether it is an epic are both read off the
/// row: the listing answers each row's metadata and type, so the whole reading
/// is one call.
pub(crate) fn holds(store: &dyn Store, seat: &str) -> Result<Holds, Stop> {
    let rows = store
        .assigned_to(seat)?
        .into_iter()
        .filter(open)
        .collect::<Vec<AssignedItem>>();
    let ordered = rows
        .iter()
        .filter(|row| row.has_orders_key)
        .cloned()
        .collect::<Vec<AssignedItem>>();
    // An epic is never work a seat holds: it stays open while its children are
    // built, and an assignee left on it names whoever last touched it, not a
    // seat carrying it.
    let held = ordered
        .iter()
        .filter(|row| row.item_type != EPIC)
        .cloned()
        .collect();
    Ok(Holds {
        open: rows,
        ordered,
        held,
    })
}

/// The statuses a seat is working under. `in_progress` is the same holding as
/// `open`: a seat that claimed its item has not stopped holding it.
fn open(row: &AssignedItem) -> bool {
    row.status == "open" || row.status == "in_progress"
}

/// `[core] reviewer`, through the census reader. The fleet's own policy rather
/// than the project's, which is the table `guards` carries.
pub(crate) fn reviewer_of(project: &Project) -> Result<String, Stop> {
    match policy::read("core", "reviewer", &project.guards) {
        Ok(Some(value)) => match value.as_str() {
            Some(name) if !name.trim().is_empty() => Ok(name.trim().to_string()),
            _ => Err(no_reviewer()),
        },
        Ok(None) => Err(no_reviewer()),
        Err(unlisted) => Err(Stop::could_not_tell(unlisted.to_string())),
    }
}

fn no_reviewer() -> Stop {
    Stop::refused(
        "no `[core] reviewer` in this fleet's policy — a delivery has nowhere to go without one",
    )
}

// ---- the working tree --------------------------------------------------------

/// The first path `git status --porcelain` reports changed in the WORKING TREE
/// and outside the staged set.
///
/// The two status columns are the index's and the working tree's: a line whose
/// second column is a space is an index-only change and is part of the
/// delivery, and a path already staged is the delivery whatever else was done
/// to it.
pub fn outside(status: &[String], staged: &[String]) -> Option<String> {
    status.iter().find_map(|line| {
        let mut columns = line.chars();
        let index = columns.next()?;
        let worktree = columns.next()?;
        if worktree == ' ' {
            return None;
        }
        let path = porcelain_path(line);
        if staged.contains(&path) {
            return None;
        }
        // An untracked entry carries `??` and no index status at all; it is
        // still a file the seat has not put in the delivery.
        let _ = index;
        Some(path)
    })
}

/// The path a porcelain line names. A rename prints `old -> new`, and the new
/// name is the one the delivery would carry.
fn porcelain_path(line: &str) -> String {
    let rest = line.get(3..).unwrap_or_default().trim();
    match rest.rsplit_once(" -> ") {
        Some((_, new)) => new.to_string(),
        None => rest.to_string(),
    }
}

// ---- the note ----------------------------------------------------------------

fn read_note(path: &Path) -> Result<String, Stop> {
    std::fs::read_to_string(path).map_err(|e| {
        Stop::usage(format!(
            "the note at {} could not be read: {e} — `--note <file>` names the note the seat wrote",
            path.display()
        ))
    })
}

/// Every label the pack's grammar names, at column zero, in its order.
///
/// The grammar is the template's FIRST block: what follows the blank line is
/// the prose that teaches a seat to write one, and a note is not made of prose.
pub fn grammar_labels(template: &str) -> Vec<String> {
    template
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(|line| line.split_once(':').map(|(label, _)| label.to_string()))
        .collect()
}

/// The note the seat handed in, against the pack's grammar: the marker it opens
/// on and every label the grammar names.
fn grammar_holds(template: &str, written: &str) -> Result<(), Stop> {
    let Some(first) = written.lines().find(|line| !line.trim().is_empty()) else {
        return Err(Stop::usage(
            "the note is empty — a delivery is the note a reviewer reads".to_string(),
        ));
    };
    if !opens_with(first, &DELIVERY_MARKERS) {
        return Err(Stop::usage(format!(
            "the note opens on `{first}` — it opens on `{}` at column zero, or no reader can \
             anchor on it",
            DELIVERY_MARKERS[0]
        )));
    }
    for label in grammar_labels(template) {
        if label_value(written, &label).is_none() {
            return Err(Stop::usage(format!(
                "the note carries no `{label}:` line, which `{DELIVERY_NOTE}` names — a field the \
                 grammar names and the note drops is a field nobody wrote"
            )));
        }
    }
    Ok(())
}

/// The seat's note with the three machine lines and the marker filled in.
fn fill(
    written: &str,
    commit: &str,
    branch: &str,
    base: &str,
    delivery: &Delivery,
) -> Result<String, Stop> {
    let marker = written
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| {
            DELIVERY_MARKERS
                .iter()
                .find(|marker| opens_with(line, &[**marker]))
        })
        .copied()
        .unwrap_or(DELIVERY_MARKERS[0]);

    let mut note = set_first_line(written, &format!("{marker} {commit} — {}", delivery.by));
    for (label, value) in [
        (COMMIT, commit.to_string()),
        (BRANCH, branch.to_string()),
        (BASE, format!("{TRUNK} at {base}, read at {}", delivery.at)),
    ] {
        note = set_label(&note, label, &value).ok_or_else(|| {
            Stop::usage(format!(
                "the note carries no `{label}:` line — deliver fills it and cannot write a line \
                 that is not there"
            ))
        })?;
    }
    Ok(note.trim_end().to_string())
}

fn set_first_line(text: &str, line: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    if let Some(at) = lines.iter().position(|line| !line.trim().is_empty()) {
        lines[at] = line.to_string();
    }
    lines.join("\n")
}

/// The label's value replaced and its own spacing kept: the column a note lines
/// its values up in is the pack's and not this verb's.
fn set_label(text: &str, label: &str, value: &str) -> Option<String> {
    let head = format!("{label}:");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let at = lines.iter().position(|line| line.starts_with(&head))?;
    let spacing: String = lines[at][head.len()..]
        .chars()
        .take_while(|c| *c == ' ')
        .collect();
    let spacing = if spacing.is_empty() {
        " ".to_string()
    } else {
        spacing
    };
    lines[at] = format!("{head}{spacing}{value}");
    Some(lines.join("\n"))
}

// ---- the read-back -----------------------------------------------------------

/// One read, asserting the assignee and the delivery region against the
/// ARGUMENTS — plus a token nothing wrote.
fn read_back(item: &str, note: &str, reviewer: &str, wiring: &Wiring) -> Result<(), Stop> {
    let read = read(wiring.store, item)?;
    if read.assignee.as_deref() != Some(reviewer) {
        return Err(disagrees(
            item,
            "assignee",
            reviewer,
            read.assignee.as_deref(),
            &format!("bd update {item} --assignee {reviewer}"),
        ));
    }
    let seen = read.notes.as_deref().and_then(last_delivery);
    if seen.as_deref().map(normalised) != Some(normalised(note)) {
        return Err(disagrees(
            item,
            "the last delivery note",
            note,
            seen.as_deref(),
            &format!("bd note {item} \"<the note, as it is printed above>\""),
        ));
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

/// Whitespace normalised to single spaces, because the store keeps a note with
/// the wrapping it was written with and the comparison is about the words.
fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn read(store: &dyn Store, item: &str) -> Result<Item, Stop> {
    store.show(item).map_err(Stop::from)
}

/// A git operation that would not answer. The step names itself in the message
/// git handed back, and nothing is rounded to a default.
fn step(cause: String) -> Stop {
    Stop::could_not_tell(cause)
}

/// A failure after the commit. The commit is real and the message says so,
/// because a caller that read this as "nothing happened" would deliver twice.
fn committed(item: &str, commit: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{why}\n  the commit {commit} STANDS on the work branch and {item} carries no delivery"
    ))
}

fn disagrees(item: &str, field: &str, wanted: &str, got: Option<&str>, rerun: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{item} read back with {field} ==\n{}\n  wanted:\n{wanted}\n  RERUN: {rerun}",
        got.unwrap_or("(absent)")
    ))
}
