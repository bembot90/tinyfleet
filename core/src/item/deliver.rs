//! `fleet deliver` — the seat's own handoff, and the only writer of a delivery.
//!
//! THE REFUSALS COME BEFORE THE COMMIT. Every question this verb can answer
//! from the record, the working tree and the file handed in — the branch, the
//! unstaged file, the empty index, a delivery that does not read, an item the
//! actor does not hold, the missing reviewer — is asked while nothing has been
//! written, so a refusal leaves a seat exactly where it stood. The one check
//! that cannot is the commit line's, which has no subject until the commit
//! exists. The tree's three questions come FIRST, before the store is called
//! at all.
//!
//! A CLEAN TREE AHEAD OF THE BASE IS ITSELF THE DELIVERY. Nothing staged and
//! HEAD ahead of the trunk ref is a seat resuming after `fleet hold`: the
//! delivery is the commit HEAD already names and this verb commits nothing.
//! Nothing staged and HEAD AT the trunk ref is a seat that built nothing, and
//! is refused.
//!
//! THE DELIVERY IS THE SEAT'S JSON, THE MACHINE LINES ARE THIS VERB'S. A
//! builder hands in a file of the shape `assets/delivery.schema.json` gives,
//! read against the binary's own type ([`DeliveryInput`]) before anything is
//! written: a file that does not read is exit 2 and the tree, the store and
//! the stream are as they were. The note this verb writes is RENDERED from it
//! ([`transitional_note`]) with the three lines only a process knows — the
//! commit, the branch, the base — and the marker line. The seat writes no
//! prose, so no line of what it says can open a marker at column zero: every
//! newline inside a value renders two spaces in.
//!
//! THE NOTE IS TRANSITIONAL. It is the text the note readers — review and
//! land — still parse, until the delivered entry replaces it (fleet-zlk.7).

use std::io::Write;
use std::path::Path;

use crate::entry::SuiteRun;
use crate::input::{self, DeliveryInput, DELIVERY_SCHEMA};
use crate::item::brief::Packs;
use crate::item::dispatch::EPIC;
use crate::item::{
    control_token, label_value, last_delivery, run, Events, Git, Project, Ring, RingOutcome, Stop,
    DELIVERY_MARKERS, ITEM_DELIVERED, TRUNK, TRUNK_BRANCH,
};
use crate::policy;
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::{Directory, SeatRef};
use crate::store::{AssignedItem, Item, Store};

/// The three lines the verb fills. Everything else the note says is the
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
/// item at wake.
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
    /// Who is delivering: a seat, or a run acting on its own record, which
    /// `item` names.
    pub by: &'a Actor,
    /// The delivery the seat handed in: a JSON file of the shape
    /// [`DELIVERY_SCHEMA`] gives.
    pub delivery: &'a Path,
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
    /// The seats this fleet knows, which `[core] reviewer` is found among.
    pub seats: &'a Directory,
}

/// The delivery made, for a caller that wants to say what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub item: String,
    pub commit: String,
    /// The reviewer's full id, which the item is now assigned to.
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
    // database is the shape that does it — and a check that read the tree after
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

    // THE FILE IS READ BEFORE THE STORE, and before the commit: a delivery
    // that does not read is the seat's to fix, and nothing is written for it.
    let input = input::read::<DeliveryInput>(delivery.delivery, "delivery", DELIVERY_SCHEMA)?;

    let item = held_item(wiring.store, delivery.by, delivery.item)?;
    let reviewer = reviewer_of(wiring.project, wiring.seats)?;
    let reviewer = reviewer.id.to_string();

    // The string form every write and the event carry.
    let by = delivery.by.to_string();
    let as_is = standing.is_some();
    let commit = match standing {
        Some(head) => head,
        None => wiring
            .git
            .commit(&format!("{item}: delivered by {}", delivery.by))
            .map_err(step)?,
    };

    let note = transitional_note(&input, &commit, &branch, &base, delivery.by, delivery.at);
    // The post-condition of the render, asked of the text that will be
    // written rather than of the values it was built from.
    if label_value(&note, COMMIT).as_deref() != Some(commit.as_str()) {
        return Err(Stop::could_not_tell(format!(
            "the rendered note's `{COMMIT}:` line reads {} and the commit is {commit}",
            label_value(&note, COMMIT).unwrap_or_else(|| "(absent)".to_string())
        )));
    }

    wiring.store.assign(&item, &reviewer, &by).map_err(|e| {
        committed(
            &item,
            &commit,
            &format!("the reassignment did not land: {e}"),
        )
    })?;
    wiring.store.note(&item, &note, &by).map_err(|e| {
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

/// The one event this verb writes.
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
                "{STANDS}: no live session for {}; {item} is theirs and their successor reads it \
                 at wake",
                named(reviewer, wiring.seats)
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
/// IT IS `hold`'s READ TOO. A question and a delivery ask the same question of
/// the record — which item is this worktree's — and two readers of it would be
/// two answers the day one of them changed.
///
/// ONLY A SEAT HOLDS WORK, and whether the actor is one is its KIND: work is
/// assigned to a seat's id, and [`Actor::seat_id`] is that id or nothing. A
/// run, a routine or the controller holds nothing, and is refused rather than
/// read as a seat nobody gave anything — the refusal names the flag that says
/// which item instead.
///
/// `--item` NAMES AN ITEM THE ACTOR HOLDS, and is checked as one
/// ([`holds_named`]): the flag says which of the actor's items, never whose.
pub fn held_item(store: &dyn Store, by: &Actor, named: Option<&str>) -> Result<String, Stop> {
    if let Some(named) = named {
        // Resolved once, here: the caller acts on the store's full id and
        // never on the part of it that was typed.
        let item = read(store, named)?;
        holds_named(&item, by)?;
        return Ok(item.id);
    }
    let Some(seat) = by.seat_id() else {
        return Err(Stop::refused(format!(
            "{by} is not a seat, so it holds nothing — pass --item <id>"
        )));
    };
    let Holds { open, held, .. } = holds(store, &seat.to_string())?;
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

/// Whether `by` holds the item `--item` named (fleet-pl6 (a)).
///
/// (i) A RUN'S RECORD IS ITS OWN RUN'S: the run whose id is the record's holds
/// it, and no other actor of any kind does. (ii) Any other item is a seat's,
/// so every other kind is refused by its kind. (iii) The seat holds it the way
/// [`holds`] reads a holding without the flag — assigned to the seat's id,
/// carrying an order index, and no epic — so naming an item never reaches
/// work the listing would not have found.
fn holds_named(item: &Item, by: &Actor) -> Result<(), Stop> {
    let id = &item.id;
    if item.labels.iter().any(|label| label == run::LABEL) {
        let its_run = Actor {
            kind: ActorKind::Run,
            id: id.clone(),
        };
        if *by == its_run {
            return Ok(());
        }
        return Err(Stop::refused(format!(
            "{id} is run {id}'s record, and {by} is not that run — a run's record is its own \
             run's to hold"
        )));
    }
    let Some(seat) = by.seat_id() else {
        return Err(Stop::refused(format!(
            "a {} holds no item — --item names an item the acting seat holds",
            by.kind.as_str()
        )));
    };
    let assignee = item.assignee.as_deref();
    if assignee == Some(seat.to_string().as_str()) && item.has_orders_key && item.item_type != EPIC
    {
        return Ok(());
    }
    Err(Stop::refused(format!(
        "{id} is held by {} and not by {by} — --item names an item the acting seat holds",
        assignee.unwrap_or("nobody")
    )))
}

/// What a seat is carrying, off ONE listing and no per-row read.
pub(crate) struct Holds {
    /// Every open row assigned to the seat.
    pub(crate) open: Vec<AssignedItem>,
    /// Of those, every one that carries a `fleet.orders` key, whatever its
    /// type: the orders standing against the seat.
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
/// retired seat is one nobody delivers whatever the item's type, so a retire
/// withdraws an ordered epic that no seat holds.
///
/// Whether a row is ordered and whether it is an epic are both read off the
/// row: the listing answers each row's metadata and type, so the whole reading
/// is one call. `seat` is the seat's full id, which is what an assignee is.
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

/// `[core] reviewer`, through the census reader, as the one seat it names. The
/// fleet's own policy rather than the project's, which is the table `guards`
/// carries.
///
/// THE VALUE IS ANY SEAT ARGUMENT — a full id, eight or more of its hex, a name
/// or a machine name — and every reader turns it into ONE LISTED SEAT, of
/// either kind, through core's one resolver: a delivery is assigned to that
/// seat's id, a landing closes as it and runs in its worktree. A value naming
/// no seat, or two, is refused naming the key rather than written to the
/// record as a word nobody holds work under.
pub(crate) fn reviewer_of(project: &Project, seats: &Directory) -> Result<SeatRef, Stop> {
    let value = match policy::read("core", "reviewer", &project.guards) {
        Ok(Some(value)) => match value.as_str() {
            Some(name) if !name.trim().is_empty() => name.trim().to_string(),
            _ => return Err(no_reviewer()),
        },
        Ok(None) => return Err(no_reviewer()),
        Err(unlisted) => return Err(Stop::could_not_tell(unlisted.to_string())),
    };
    seats.resolve_listed(&value).cloned().map_err(|unresolved| {
        let why = format!("[core] reviewer = \"{value}\" {unresolved}");
        match unresolved.code() {
            crate::item::USAGE => Stop::usage(why),
            _ => Stop::refused(why),
        }
    })
}

/// How a sentence names a seat the record holds by id: its label where the
/// text is an id, and the text itself where it is not.
pub(crate) fn named(seat: &str, seats: &Directory) -> String {
    crate::seat::identity::SeatId::parse(seat)
        .map(|id| seats.label(&id))
        .unwrap_or_else(|_| seat.to_string())
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

/// The delivery note, rendered from the seat's delivery and the verb's own
/// facts in the grammar the note readers anchor on: the marker line, the three
/// machine lines, then one line per field the seat filled, and each numbered
/// call two spaces in under `decisions:`.
///
/// EVERY NEWLINE INSIDE A VALUE RENDERS AS "\n  ", so a value quoting a marker
/// never opens one at column zero and never ends the region it sits in
/// (fleet-4rl). A list with nothing in it renders `none`.
///
/// TRANSITIONAL: review's decisions walk and land's label reads parse this text
/// until deliver writes the delivered entry, which deletes this and its
/// callers (fleet-zlk.7).
pub fn transitional_note(
    input: &DeliveryInput,
    commit: &str,
    branch: &str,
    base: &str,
    by: &Actor,
    at: &str,
) -> String {
    let listed = |values: Vec<String>, between: &str| {
        if values.is_empty() {
            String::from("none")
        } else {
            values.join(between)
        }
    };
    let suite = match &input.suite {
        SuiteRun::Ran(ran) => format!("{}, rc {}", off(&ran.command), ran.rc),
        SuiteRun::NotTested(not) => format!("NOT TESTED — {}", off(&not.not_tested)),
    };
    let corrections = match input.spec_corrections.len() {
        0 => String::from("none"),
        n => format!(
            "{n} — {}",
            input
                .spec_corrections
                .iter()
                .map(|c| format!("{} — refuted by {}", off(&c.premise), off(&c.refuted_by)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    };

    let mut lines = vec![
        format!(
            "{} {} — {}",
            DELIVERY_MARKERS[0],
            off(commit),
            off(&by.to_string())
        ),
        format!("{COMMIT}:  {}", off(commit)),
        format!("{BRANCH}:  {}", off(branch)),
        format!("{BASE}:    {TRUNK} at {}, read at {}", off(base), off(at)),
        format!(
            "files:   {}",
            listed(input.files.iter().map(|file| off(file)).collect(), ", ")
        ),
        format!(
            "checks:  {}",
            listed(
                input
                    .checks
                    .iter()
                    .map(|row| format!("{}: {}", off(&row.check), off(&row.result)))
                    .collect(),
                "; "
            )
        ),
        format!("suite:   {suite}"),
        format!("spec corrections: {corrections}"),
        format!(
            "not proven: {}",
            listed(
                input
                    .not_proven
                    .iter()
                    .map(|gap| format!("{} — {}", off(&gap.surface), off(&gap.command)))
                    .collect(),
                "; "
            )
        ),
        match input.decisions.len() {
            0 => String::from("decisions: none"),
            n => format!("decisions: {n}"),
        },
    ];
    // Numbered by position: the first is D1, which is the name the reviewer
    // rules on.
    for (k, decision) in input.decisions.iter().enumerate() {
        lines.push(format!(
            "  D{} {}; not taken: {}; because {}",
            k + 1,
            off(&decision.call),
            off(&decision.not_taken),
            off(&decision.because)
        ));
    }
    lines.push(format!(
        "covers: {}",
        listed(
            input.covers.iter().map(|covered| off(covered)).collect(),
            ", "
        )
    ));
    lines.join("\n").trim_end().to_string()
}

/// A value with every line after its first two spaces in, so no line of it
/// sits at column zero.
fn off(value: &str) -> String {
    value.replace('\n', "\n  ")
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
