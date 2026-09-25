//! `fleet review` — the reviewer's read and the verdict it writes.
//!
//! Core's review is THE READER: one reviewer seat, the diff against the item,
//! the decisions walk. So the size line here is a measurement and carries no
//! tier — core ships no T0-to-T3 scheme and no per-tier fan-out, which are the
//! tiny pack's opinion and plug in as size tiers with a per-tier review a pack
//! defines.
//!
//! IT NEVER LANDS. `--land` writes the ACCEPTED verdict that the landing verb
//! then reads; the squash, the push and the close are that verb's. A verdict
//! is a note like every other note the verbs write, and it is read back before
//! exit 0.
//!
//! THE DELIVERY IS THE TIMELINE'S LAST DELIVERED ENTRY, and nothing else. Its
//! commit is what is reviewed, its base is where the size is measured from,
//! and its decisions are what the walk rules on. A prose delivery note on the
//! item is not one: the record has no delivery grammar left to read it by.

use std::io::Write;
use std::path::Path;

use crate::entry::{Delivered, Finding, Timeline};
use crate::input::{self, FindingsInput, FINDINGS_SCHEMA};
use crate::item::brief::Packs;
use crate::item::deliver::named;
use crate::item::show::entry_lines;
use crate::item::{
    control_token, render, Change, Events, Git, Project, Ring, RingOutcome, Stop, ITEM_RETURNED,
    ITEM_REVIEWED, VERDICT_ACCEPTED, VERDICT_MARKERS,
};
use crate::seat::actor::Actor;
use crate::seat::identity::Directory;
use crate::store::{Item, Store};

/// The verdict grammar, in core's pack and shadowable like every other asset.
pub const VERDICT: &str = "assets/verdict.md";

/// What the ring carries on a return.
pub const RING: &str = "{item} is returned with {findings} finding(s) and is yours again. \
     Read the item and act on the record.";

/// The line a return prints where the builder has no live session.
pub const STANDS: &str = "RETURNED, NOT RUNG";

/// What the reviewer asked for.
pub enum Mode<'a> {
    /// Print what a reviewer needs; write nothing.
    Show,
    /// Write the ACCEPTED verdict, its decisions walk in it.
    Land,
    /// Write the RETURNED WITH FINDINGS verdict from this findings file: a
    /// JSON file of the shape [`FINDINGS_SCHEMA`] gives, read against
    /// [`FindingsInput`] before anything is written. Its findings are numbered
    /// by their place in the list, so the file numbers nothing itself.
    Return(&'a Path),
}

pub struct Verdict<'a> {
    pub item: &'a str,
    /// The reviewer.
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
    /// by.
    pub seats: &'a Directory,
}

/// What the review read and wrote.
pub struct Read {
    /// The item reviewed, by the store's full id, whatever part of it the
    /// caller typed.
    pub item: String,
    pub commit: String,
    pub size: String,
    /// The verdict written, where a mode wrote one.
    pub verdict: Option<String>,
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
    let entries = wiring.store.timeline(&item.id)?;
    let Some((entry, delivered)) = Timeline(&entries).last_delivery() else {
        return Err(Stop::refused(format!(
            "{} carries no delivery — a review reads one and there is none to read",
            item.id
        )));
    };
    let commit = delivered.commit.clone();

    let size = size_line(&commit, &delivered.base, wiring)?;
    let _ = writeln!(out, "{size}");

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
        Mode::Land => Some(accept(&item, delivered, &commit, &size, verdict, wiring)?),
        Mode::Return(findings) => Some(retur(
            out, err, &item, findings, &commit, &size, verdict, wiring,
        )?),
    };

    Ok(Read {
        item: item.id,
        commit,
        size,
        verdict: written,
    })
}

// ---- the size line -----------------------------------------------------------

/// The measurement, from the base the delivery was cut from to the delivery
/// commit, counted from their merge-base. The base is always a whole sha: the
/// delivered entry cannot be written with anything else.
fn size_line(commit: &str, base: &str, wiring: &Wiring) -> Result<String, Stop> {
    size_of(commit, base, wiring.git, &wiring.project.root)
}

/// The same measurement, for a caller that has a git and a root and no
/// [`Wiring`] — the flight rendering a reviewer's brief, which must show the
/// line `review` will print and not a second one measured its own way.
pub fn size_of(commit: &str, base: &str, git: &dyn Git, root: &Path) -> Result<String, Stop> {
    let changes = git.numstat(base, commit).map_err(Stop::could_not_tell)?;
    Ok(rendered_size(&changes, root))
}

pub fn rendered_size(changes: &[Change], root: &Path) -> String {
    let added: u64 = changes.iter().filter_map(|c| c.added).sum();
    let deleted: u64 = changes.iter().filter_map(|c| c.deleted).sum();
    let binary = changes
        .iter()
        .filter(|c| c.added.is_none() || c.deleted.is_none())
        .count();
    format!(
        "size: {} file(s), +{added}, -{deleted}{} — tests: {}, executable: {}",
        changes.len(),
        if binary == 0 {
            String::new()
        } else {
            format!(" ({binary} binary)")
        },
        yes(changes.iter().any(|c| is_test(&c.path))),
        yes(changes.iter().any(|c| is_executable(root, &c.path))),
    )
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

/// The walk: one line per call the delivery lists, then the count.
///
/// `--land` is the accept, so every call it walks is accepted; a call the
/// reviewer will not take is a finding and the delivery goes back with it.
fn walk(delivered: &Delivered) -> String {
    let calls = decisions(delivered);
    let mut lines: Vec<String> = calls.iter().map(|name| format!("{name} ACCEPT")).collect();
    lines.push(format!("{} accepted, 0 overruled", calls.len()));
    lines.join("\n  ")
}

// ---- the two verdicts --------------------------------------------------------

fn accept(
    item: &Item,
    delivered: &Delivered,
    commit: &str,
    size: &str,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<String, Stop> {
    let note = render(
        &block(&wiring.packs.read(VERDICT)?, VERDICT_MARKERS[0])?,
        &[
            ("commit", commit),
            ("reviewer", &verdict.by.to_string()),
            ("item", &item.id),
            ("size", size),
            ("decisions", &walk(delivered)),
        ],
    )
    .map_err(|name| unresolved(&name))?;
    write_verdict(&item.id, &note, None, verdict, wiring)?;
    // The walk is the accept, so every call it found is accepted and none is
    // overruled: a `--land` that would overrule one is a return instead.
    announce(
        &item.id,
        ITEM_REVIEWED,
        verdict,
        wiring,
        serde_json::json!({
            "item": item.id,
            "commit": commit,
            "verdict": VERDICT_ACCEPTED,
            "accepted": delivered.decisions.len(),
            "overruled": 0,
        }),
    )?;
    Ok(note)
}

/// The one event either writing mode appends.
///
/// AFTER THE VERDICT IS WRITTEN AND READ BACK, and before the exit: a crash
/// between the two leaves a verdict nothing announced, which the fold reads as
/// the record says, and never the reverse.
///
/// WHICH RETURN THIS IS IS NOT WRITTEN (decision D2): the fold counts the
/// returns it reads.
fn announce(
    item: &str,
    kind: &str,
    verdict: &Verdict,
    wiring: &Wiring,
    payload: serde_json::Value,
) -> Result<(), Stop> {
    wiring
        .events
        .append(kind, verdict.by, payload)
        .map_err(|e| {
            Stop::could_not_tell(format!(
                "{kind} did not reach the stream: {e}\n  the verdict on {item} STANDS"
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
    size: &str,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<String, Stop> {
    // Read whole before anything is written: a file that does not parse, or
    // parses and numbers nothing, stops here with the item still the
    // reviewer's.
    let findings = input::read::<FindingsInput>(findings, "findings", FINDINGS_SCHEMA)?.findings();
    let count = findings.len();
    // The builder is the order index's seat: the record of who was given this
    // item, which is the one place that says where a return goes. It is the
    // seat's full id, which is what the return assigns and rings.
    let Some(builder) = item.orders.as_ref().and_then(|o| o.seat.clone()) else {
        return Err(Stop::refused(format!(
            "{}'s order index names no seat — a return goes to the seat the order named and the \
             record does not say who that is",
            item.id
        )));
    };

    let note = render(
        &block(&wiring.packs.read(VERDICT)?, VERDICT_MARKERS[1])?,
        &[
            ("commit", commit),
            ("reviewer", &verdict.by.to_string()),
            ("item", &item.id),
            ("size", size),
            ("findings", &count.to_string()),
            ("body", &off_column_zero(&numbered_findings(&findings))),
        ],
    )
    .map_err(|name| unresolved(&name))?;

    // HANDED OVER FROM THE HOLDER THIS REVIEW READ, and only while it still
    // holds the item: the reviewer need not be the `--by` of the call, and bd
    // 1.3.0 refuses a plain reassignment of an `in_progress` item by anyone
    // but its holder — which the builder's claim leaves it through delivery.
    wiring.store.hand_over(
        &item.id,
        item.assignee.as_deref().unwrap_or_default(),
        &builder,
        &verdict.by.to_string(),
    )?;
    write_verdict(&item.id, &note, Some(&builder), verdict, wiring)?;
    announce(
        &item.id,
        ITEM_RETURNED,
        verdict,
        wiring,
        serde_json::json!({
            "item": item.id,
            "commit": commit,
            "findings": count,
        }),
    )?;

    let text = render(
        RING,
        &[("item", &item.id), ("findings", &count.to_string())],
    )
    .unwrap_or_else(|_| format!("{} is returned", item.id));
    match wiring.ring.ring(&builder, &text) {
        RingOutcome::Delivered => {}
        RingOutcome::Absent => {
            let _ = writeln!(
                out,
                "{STANDS}: no live session for {}; the return stands and their successor reads \
                 it at wake",
                named(&builder, wiring.seats)
            );
        }
        RingOutcome::Failed(cause) => {
            let _ = writeln!(err, "{STANDS}: {cause}; the return stands");
        }
    }
    Ok(note)
}

/// The verdict written and read back: the assignee a return reassigned to, the
/// last verdict region of the item's notes against the text this verb
/// rendered, plus a token nothing wrote.
fn write_verdict(
    item: &str,
    note: &str,
    assignee: Option<&str>,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<(), Stop> {
    wiring.store.note(item, note, &verdict.by.to_string())?;
    let read = read(wiring.store, item)?;
    if let Some(wanted) = assignee {
        if read.assignee.as_deref() != Some(wanted) {
            return Err(Stop::could_not_tell(format!(
                "{item} read back with assignee ==\n{}\n  wanted:\n{wanted}\n  RERUN: bd update \
                 {item} --assignee {wanted}",
                read.assignee.as_deref().unwrap_or("(absent)")
            )));
        }
    }
    let seen = read.notes.as_deref().and_then(last_verdict);
    if seen.as_deref().map(normalised) != Some(normalised(note)) {
        return Err(Stop::could_not_tell(format!(
            "{item} read back with its last verdict ==\n{}\n  wanted:\n{note}\n  RERUN: bd note \
             {item} \"<the verdict, as it is printed above>\"",
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

/// The last verdict region of an item's notes, ended by the delivery or the
/// landing that follows it — and by NEITHER verdict marker, because a return's
/// findings body is a reviewer's own text and habitually quotes one.
pub fn last_verdict(notes: &str) -> Option<String> {
    crate::item::last_region(
        notes,
        &VERDICT_MARKERS,
        &[
            &crate::item::DELIVERY_MARKERS,
            &crate::item::LANDING_MARKERS,
            &crate::item::PARK_MARKERS,
            &crate::item::ANSWER_MARKERS,
        ],
    )
}

/// The template's block for one marker, in this verb's own words.
fn block(template: &str, marker: &str) -> Result<String, Stop> {
    crate::item::marker_block(template, marker).ok_or_else(|| {
        Stop::could_not_tell(format!(
            "`{VERDICT}` carries no `{marker}` block — the pack's verdict grammar names both"
        ))
    })
}

fn unresolved(name: &str) -> Stop {
    Stop::could_not_tell(format!(
        "`{VERDICT}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
    ))
}

/// The findings as the verdict's body: `F<k> <text>` for the k-th finding,
/// each line that continues its text two spaces in, so a finding that runs
/// over lines still reads as one.
fn numbered_findings(findings: &[Finding]) -> String {
    findings
        .iter()
        .enumerate()
        .map(|(k, finding)| {
            format!(
                "F{} {}",
                k + 1,
                finding.text.trim_end().replace('\n', "\n  ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The findings body, indented off column zero.
///
/// A region ends at the next marker AT COLUMN ZERO, and a finding is prose a
/// reviewer wrote — which in this store habitually quotes a delivery's or a
/// landing's own first line. Indented, no line of it can end the verdict it
/// is inside, so the read-back below compares the whole note against the
/// whole note. The count was taken from the findings file, so nothing about
/// how many findings there are changes here.
fn off_column_zero(body: &str) -> String {
    body.lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("  {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn read(store: &dyn Store, item: &str) -> Result<Item, Stop> {
    store.show(item).map_err(Stop::from)
}
