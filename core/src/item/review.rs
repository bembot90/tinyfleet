//! `fleet review` — the reviewer's read and the verdict it writes.
//!
//! Core's review is THE READER: one reviewer seat, the diff against the item,
//! the decisions walk. So the size line here is a measurement and carries no
//! tier — core ships no T0-to-T3 scheme and no per-tier fan-out, which are the
//! tiny pack's opinion and plug in as size tiers with a per-tier review a pack
//! defines.
//!
//! IT NEVER LANDS. `--land` appends the ACCEPTED `reviewed` entry that the
//! landing verb then reads; the squash, the push and the close are that verb's.
//! A verdict is one entry on the item's timeline, appended by the reviewing
//! actor and read back before exit 0. No note is written.
//!
//! A VERDICT IS THE HOLDER'S (fleet-pl6 (b)). Either writing mode refuses an
//! actor that does not hold the item: a seat holds it by its id, and a
//! delivered item's holder is its reviewer; a run reviews as the `[core]
//! reviewer`, as a run's landing closes as it; no other kind writes one.
//! `--show` writes nothing and is anyone's.
//!
//! THE DELIVERY IS THE TIMELINE'S LAST DELIVERED ENTRY, and nothing else. Its
//! commit is what is reviewed, its base is where the size is measured from,
//! and its decisions are what the walk rules on. Prose on the item is not one,
//! whatever it says: fleet has no grammar to read a delivery out of text.

use std::io::Write;
use std::path::Path;

use crate::entry::{self, Body, Delivered, Finding, Reviewed, Ruling, RulingKind, Size, Timeline};
use crate::input::{self, FindingsInput, FINDINGS_SCHEMA};
use crate::item::brief::Packs;
use crate::item::deliver::reviewer_of;
use crate::item::land::run_record;
use crate::item::show::entry_lines;
use crate::item::{
    control_token, recorded, render, signal, Change, Events, Git, Project, Ring, RingOutcome, Stop,
    Unrecorded, ITEM_ENTRY,
};
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::{Directory, SeatId};
use crate::store::{Item, Order, OrderState, Store};

/// What the ring carries on a return.
pub const RING: &str = "{item} is returned with {findings} finding(s) and is yours again. \
     Read the item and act on the record.";

/// The line a return prints where the builder has no live session.
pub const STANDS: &str = "RETURNED, NOT RUNG";

/// What the reviewer asked for.
pub enum Mode<'a> {
    /// Print what a reviewer needs; write nothing.
    Show,
    /// Append the ACCEPTED verdict, its decisions walk in it.
    Land,
    /// Append the RETURNED verdict with the findings in this file: a JSON
    /// file of the shape [`FINDINGS_SCHEMA`] gives, read against
    /// [`FindingsInput`] before anything is written. Its findings are numbered
    /// by their place in the list, so the file numbers nothing itself.
    Return(&'a Path),
}

pub struct Verdict<'a> {
    pub item: &'a str,
    /// The reviewer: the item's holder, or a run acting as the `[core]
    /// reviewer`.
    pub by: &'a Actor,
    pub mode: Mode<'a>,
}

pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub git: &'a dyn Git,
    pub packs: &'a Packs,
    pub project: &'a Project,
    pub ring: &'a dyn Ring,
    pub events: &'a dyn Events,
    /// The seats this fleet knows, which a return's sentence names the builder
    /// by and `[core] reviewer` is found among.
    pub seats: &'a Directory,
}

/// What the review read and wrote.
pub struct Read {
    /// The item reviewed, by the store's full id, whatever part of it the
    /// caller typed.
    pub item: String,
    pub commit: String,
    pub size: String,
    /// The reviewed entry's id, as the store answered it, where a mode wrote
    /// one.
    pub entry: Option<String>,
}

pub fn review(
    out: &mut dyn Write,
    err: &mut dyn Write,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<Read, Stop> {
    // Resolved once: everything below names `item.id`, the store's full id,
    // and never `verdict.item`, the part of it that was typed.
    let item = read(wiring.store, verdict.item)?;
    // THE HOLDER BEFORE ANYTHING IS READ OR MEASURED: an actor that may not
    // write a verdict is refused with the record and the stream as they were.
    if !matches!(verdict.mode, Mode::Show) {
        holds(&item, verdict.by, wiring)?;
    }
    let entries = wiring.store.timeline(&item.id)?;
    let Some((entry, delivered)) = Timeline(&entries).last_delivery() else {
        return Err(Stop::refused(format!(
            "{} carries no delivery — a review reads one and there is none to read",
            item.id
        )));
    };
    let commit = delivered.commit.clone();

    // ONE MEASUREMENT: the line printed here and the size a verdict carries
    // are read off the same numstat.
    let changes = wiring
        .git
        .numstat(&delivered.base, &commit)
        .map_err(Stop::could_not_tell)?;
    let size = rendered_size(&changes, &wiring.project.root);
    let _ = writeln!(out, "{size}");
    let measured = sized(&changes, &wiring.project.root, &delivered.base);

    let written = match &verdict.mode {
        Mode::Show => {
            // The entry as `fleet item show` renders it, so a reviewer and a
            // person reading the item read one rendering of it.
            let _ = writeln!(out);
            for line in entry_lines(entry) {
                let _ = writeln!(out, "{line}");
            }
            None
        }
        Mode::Land => Some(accept(
            &item, delivered, &commit, measured, verdict, wiring,
        )?),
        Mode::Return(findings) => Some(retur(
            out, err, &item, findings, &commit, measured, verdict, wiring,
        )?),
    };

    Ok(Read {
        item: item.id.to_string(),
        commit,
        size,
        entry: written,
    })
}

// ---- the holder --------------------------------------------------------------

/// Whether `by` may write a verdict on this item (fleet-pl6 (b)).
///
/// A SEAT holds the item by its id: deliver hands every delivery to the
/// reviewer, so a delivered item's holder is its reviewer. A RUN answers for
/// nothing itself and reviews as the `[core] reviewer`, as land already treats
/// it: its id must name a run's record, and the item must be that seat's. No
/// other kind writes a verdict.
fn holds(item: &Item, by: &Actor, wiring: &Wiring) -> Result<(), Stop> {
    let id = &item.id;
    let assignee = item
        .assignee
        .map_or_else(|| String::from("nobody"), |held| held.to_string());
    match by.kind {
        ActorKind::Seat => {
            if by.seat_id().is_some_and(|seat| item.assignee == Some(seat)) {
                return Ok(());
            }
            Err(Stop::refused(format!(
                "{id} is held by {assignee} and not by {by} — a verdict is the holder's, and a \
                 delivered item's holder is its reviewer"
            )))
        }
        ActorKind::Run => {
            let record = run_record(wiring.store, by)?;
            let reviewer = reviewer_of(wiring.project, wiring.seats)?.id;
            if item.assignee == Some(reviewer) {
                return Ok(());
            }
            Err(Stop::refused(format!(
                "run {} reviews as the [core] reviewer {reviewer}, and {id} is held by {assignee}",
                record.id
            )))
        }
        ActorKind::Routine | ActorKind::Controller => Err(Stop::refused(format!(
            "a {} writes no verdict",
            by.kind.as_str()
        ))),
    }
}

// ---- the size line -----------------------------------------------------------

/// The measurement, from the base the delivery was cut from to the delivery
/// commit, counted from their merge-base, for a caller that has a git and a
/// root and no [`Wiring`] — the flight rendering a reviewer's brief, which must
/// show the line `review` will print and not a second one measured its own way.
/// The base is always a whole sha: the delivered entry cannot be written with
/// anything else.
pub fn size_of(commit: &str, base: &str, git: &dyn Git, root: &Path) -> Result<String, Stop> {
    let changes = git.numstat(base, commit).map_err(Stop::could_not_tell)?;
    Ok(rendered_size(&changes, root))
}

/// The size line a person reads. It names no base: the line is printed beside
/// the delivery that says where it was cut from.
pub fn rendered_size(changes: &[Change], root: &Path) -> String {
    let size = sized(changes, root, "");
    format!(
        "size: {} file(s), +{}, -{}{} — tests: {}, executable: {}",
        size.files,
        size.added,
        size.deleted,
        if size.binary == 0 {
            String::new()
        } else {
            format!(" ({} binary)", size.binary)
        },
        yes(size.tests),
        yes(size.executable),
    )
}

/// The same measurement as the verdict carries it, beside the base it was
/// measured from.
pub fn sized(changes: &[Change], root: &Path, base: &str) -> Size {
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    Size {
        files: count(changes.len()),
        added: changes.iter().filter_map(|c| c.added).sum(),
        deleted: changes.iter().filter_map(|c| c.deleted).sum(),
        binary: count(
            changes
                .iter()
                .filter(|c| c.added.is_none() || c.deleted.is_none())
                .count(),
        ),
        tests: changes.iter().any(|c| is_test(&c.path)),
        executable: changes.iter().any(|c| is_executable(root, &c.path)),
        base: base.to_string(),
    }
}

fn yes(answer: bool) -> &'static str {
    if answer {
        "yes"
    } else {
        "no"
    }
}

/// A path under a `test`/`tests` directory, or a file whose stem (the name up
/// to its last `.`) is `test` or `tests`, ends `_test`, `_tests` or `.test`, or
/// begins `test_`.
fn is_test(path: &str) -> bool {
    let (dirs, name) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    dirs.split('/').any(|dir| dir == "test" || dir == "tests")
        || stem == "test"
        || stem == "tests"
        || ["_test", "_tests", ".test"]
            .iter()
            .any(|end| stem.ends_with(end))
        || stem.starts_with("test_")
}

/// The mode as the working tree holds it. A path the delivery deleted or moved
/// away is not there to read and answers no, which the size line's own words
/// say it is measuring: what is executable in the tree the reviewer has.
fn is_executable(root: &Path, path: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(root.join(path))
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, path);
        false
    }
}

// ---- the decisions walk ------------------------------------------------------

/// The calls the delivery lists, by their `D<k>` names: the k-th decision in
/// the entry's list is `D<k>`, 1-based. The list is the count, so there is no
/// header for it to disagree with.
pub fn decisions(delivered: &Delivered) -> Vec<String> {
    (1..=delivered.decisions.len())
        .map(|k| format!("D{k}"))
        .collect()
}

/// The walk: one ruling per call the delivery lists, by its number.
///
/// `--land` is the accept, so every call it walks is accepted; a call the
/// reviewer will not take is a finding and the delivery goes back with it.
fn walk(delivered: &Delivered) -> Vec<Ruling> {
    (1..=delivered.decisions.len())
        .map(|k| Ruling {
            decision: u32::try_from(k).unwrap_or(u32::MAX),
            ruling: RulingKind::Accept,
        })
        .collect()
}

// ---- the two verdicts --------------------------------------------------------

fn accept(
    item: &Item,
    delivered: &Delivered,
    commit: &str,
    size: Size,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<String, Stop> {
    let reviewed = Body::Reviewed(Reviewed {
        verdict: entry::Verdict::Accepted,
        commit: commit.to_string(),
        size,
        walk: walk(delivered),
        findings: Vec::new(),
    });
    let id = write_verdict(&item.id, &reviewed, None, verdict, wiring)?;
    announce(&item.id, &id, verdict, wiring)?;
    Ok(id)
}

/// The one event either writing mode appends: the reviewed entry's signal, an
/// accept's and a return's alike. Which verdict it was is the entry's.
///
/// AFTER THE VERDICT IS WRITTEN AND READ BACK, and before the exit: a crash
/// between the two leaves a verdict nothing signalled, which a reader of the
/// record reads as the record says, and never the reverse.
fn announce(item: &str, entry: &str, verdict: &Verdict, wiring: &Wiring) -> Result<(), Stop> {
    signal(wiring.events, verdict.by, item, entry, "reviewed").map_err(|e| {
        Stop::could_not_tell(format!(
            "{ITEM_ENTRY} did not reach the stream: {e}\n  the verdict on {item} STANDS"
        ))
    })
}

#[allow(clippy::too_many_arguments)]
fn retur(
    out: &mut dyn Write,
    err: &mut dyn Write,
    item: &Item,
    findings: &Path,
    commit: &str,
    size: Size,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<String, Stop> {
    // Read whole before anything is written: a file that does not parse, or
    // parses and numbers nothing, stops here with the item still the
    // reviewer's.
    let findings: Vec<Finding> =
        input::read::<FindingsInput>(findings, "findings", FINDINGS_SCHEMA)?.findings();
    let count = findings.len();
    // The builder is the order index's seat: the record of who was given this
    // item, which is the one place that says where a return goes. It is the
    // seat's full id, which is what the return assigns and rings.
    let builder = match &item.order {
        OrderState::Ordered(Order {
            seat: Some(seat), ..
        }) => *seat,
        OrderState::Unreadable => {
            return Err(Stop::refused(format!(
                "{}'s order index is not one this fleet can read — a return goes to the seat the \
                 order named and the record does not say who that is",
                item.id
            )))
        }
        OrderState::Ordered(_) | OrderState::None => {
            return Err(Stop::refused(format!(
                "{}'s order index names no seat — a return goes to the seat the order named and \
                 the record does not say who that is",
                item.id
            )))
        }
    };

    let reviewed = Body::Reviewed(Reviewed {
        verdict: entry::Verdict::Returned,
        commit: commit.to_string(),
        size,
        walk: Vec::new(),
        findings,
    });

    // HANDED OVER FROM THE HOLDER THIS REVIEW READ, and only while it still
    // holds the item: under a run the holder is the `[core] reviewer` and not
    // the `--by` of the call, and a store may refuse a plain reassignment of an
    // `in_progress` item by anyone but its holder — which the builder's claim
    // leaves it through delivery.
    wiring.store.hand_over(
        &item.id,
        &item
            .assignee
            .map(|held| held.to_string())
            .unwrap_or_default(),
        &builder.to_string(),
        &verdict.by.to_string(),
    )?;
    let id = write_verdict(&item.id, &reviewed, Some(builder), verdict, wiring)?;
    announce(&item.id, &id, verdict, wiring)?;

    let text = render(
        RING,
        &[("item", &item.id), ("findings", &count.to_string())],
    )
    .unwrap_or_else(|_| format!("{} is returned", item.id));
    match wiring.ring.ring(&builder.to_string(), &text) {
        RingOutcome::Delivered => {}
        RingOutcome::Absent => {
            let _ = writeln!(
                out,
                "{STANDS}: no live session for {}; the return stands and their successor reads \
                 it at wake",
                wiring.seats.label(&builder)
            );
        }
        RingOutcome::Failed(cause) => {
            let _ = writeln!(err, "{STANDS}: {cause}; the return stands");
        }
    }
    Ok(id)
}

/// The verdict appended and read back, answered as the entry's id: the entry
/// through [`recorded`], which reads it off the timeline by the id the store
/// answered, then — for a return — the assignee it reassigned to, plus a token
/// nothing wrote.
fn write_verdict(
    item: &str,
    reviewed: &Body,
    assignee: Option<SeatId>,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<String, Stop> {
    let id =
        recorded(wiring.store, item, reviewed, verdict.by).map_err(
            |unrecorded| match unrecorded {
                Unrecorded::NotWritten(e) => Stop::from(e),
                Unrecorded::Unconfirmed(why) => {
                    Stop::could_not_tell(format!("{why}\n  the verdict on {item} STANDS"))
                }
            },
        )?;
    if let Some(wanted) = assignee {
        let read = read(wiring.store, item)?;
        if read.assignee != Some(wanted) {
            return Err(Stop::could_not_tell(format!(
                "{item} read back with assignee ==\n{}\n  wanted:\n{wanted}\n  READ: fleet item \
                 show {item}",
                read.assignee
                    .map_or_else(|| String::from("(absent)"), |held| held.to_string())
            )));
        }
        let control = control_token();
        if read.proof.carries(control) {
            return Err(Stop::could_not_tell(format!(
                "the read-back on {item} carries {control}, which nothing wrote — the read is not \
                 reading this item"
            )));
        }
    }
    Ok(id)
}

fn read(store: &dyn Store, item: &str) -> Result<Item, Stop> {
    store.show(item).map_err(Stop::from)
}
