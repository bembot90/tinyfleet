//! `fleet review` — the reviewer's read and the verdict it writes (packs PRD
//! R7, Q1; cli PRD § `fleet review <item>`).
//!
//! Q1 ruled core's review is THE READER: one reviewer seat, the diff against
//! the item, the decisions walk. So the size line here is a measurement and
//! carries no tier — core ships no T0-to-T3 scheme and no per-tier fan-out,
//! which are the tiny pack's opinion plugging in at the packs PRD's P1
//! ("`fleet review` size tiers with a per-tier review a pack defines").
//!
//! IT NEVER LANDS. `--land` writes the ACCEPTED verdict that the landing verb
//! then reads; the squash, the push and the close are that verb's (cli PRD
//! § `fleet land`). A verdict is a note like every other note the verbs write,
//! and it is read back before exit 0.

use std::io::Write;
use std::path::Path;

use crate::item::brief::Packs;
use crate::item::deliver::{BASE, COMMIT};
use crate::item::{
    control_token, label_value, last_delivery, render, Change, Events, Git, Project, Ring,
    RingOutcome, Stop, ITEM_RETURNED, ITEM_REVIEWED, VERDICT_ACCEPTED, VERDICT_MARKERS,
};
use crate::store::{Item, Store, StoreError};

/// The verdict grammar, in core's pack and shadowable like every other asset.
pub const VERDICT: &str = "assets/verdict.md";

/// The label the delivery note lists the builder's calls under.
pub const DECISIONS: &str = "decisions";

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
    /// Write the RETURNED WITH FINDINGS verdict from this findings file.
    Return(&'a Path),
}

pub struct Verdict<'a> {
    pub item: &'a str,
    /// The reviewer.
    pub by: &'a str,
    pub mode: Mode<'a>,
}

pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub git: &'a dyn Git,
    pub packs: &'a Packs,
    pub project: &'a Project,
    pub ring: &'a dyn Ring,
    pub events: &'a dyn Events,
}

/// What the review read and wrote.
pub struct Read {
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
    let item = read(wiring.store, verdict.item)?;
    let Some(delivery) = item.notes.as_deref().and_then(last_delivery) else {
        return Err(Stop::refused(format!(
            "{} carries no delivery — a review reads one and there is none to read",
            verdict.item
        )));
    };
    let Some(commit) = label_value(&delivery, COMMIT).filter(|sha| !sha.is_empty()) else {
        return Err(Stop::refused(format!(
            "the delivery on {} names no `{COMMIT}:` — a review takes a commit, never a branch",
            verdict.item
        )));
    };

    let size = size_line(&commit, &delivery, wiring)?;
    let _ = writeln!(out, "{size}");

    let written = match &verdict.mode {
        Mode::Show => {
            let _ = writeln!(out, "{delivery}");
            let _ = writeln!(out, "{}", decisions_block(&delivery));
            None
        }
        Mode::Land => Some(accept(&item, &delivery, &commit, &size, verdict, wiring)?),
        Mode::Return(findings) => Some(retur(
            out, err, &item, findings, &commit, &size, verdict, wiring,
        )?),
    };

    Ok(Read {
        commit,
        size,
        verdict: written,
    })
}

// ---- the size line -----------------------------------------------------------

/// The measurement, from the base the delivery recorded to the delivery commit,
/// counted from their merge-base; a delivery naming no readable base is measured
/// against `<commit>^` and the line says so.
///
/// `<commit>^` and not a parent read of its own: the seam takes two commits and
/// git resolves the expression, so a root commit answers with git's own refusal
/// rather than with a count taken from nothing.
fn size_line(commit: &str, delivery: &str, wiring: &Wiring) -> Result<String, Stop> {
    size_of(commit, delivery, wiring.git, &wiring.project.root)
}

/// The same measurement, for a caller that has a git and a root and no
/// [`Wiring`] — the flight rendering a reviewer's brief, which must show the
/// line `review` will print and not a second one measured its own way.
pub fn size_of(commit: &str, delivery: &str, git: &dyn Git, root: &Path) -> Result<String, Stop> {
    let base = recorded_base(delivery);
    let from = base.clone().unwrap_or_else(|| format!("{commit}^"));
    let changes = git.numstat(&from, commit).map_err(Stop::could_not_tell)?;
    let size = rendered_size(&changes, root);
    Ok(match base {
        Some(_) => size,
        None => format!("{size} — against {from}: the delivery names no readable base"),
    })
}

/// The sha of the `base:` line: the token after ` at `, up to a comma or a
/// space, and only as 7 to 40 lowercase hex.
fn recorded_base(delivery: &str) -> Option<String> {
    let value = label_value(delivery, BASE)?;
    let (_, after) = value.split_once(" at ")?;
    let sha: String = after
        .chars()
        .take_while(|c| *c != ',' && !c.is_whitespace())
        .collect();
    let hex = sha
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    ((7..=40).contains(&sha.len()) && hex).then_some(sha)
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

/// The `decisions:` line of a delivery and the lines under it, which is where
/// the numbered calls sit.
pub fn decisions_block(delivery: &str) -> String {
    let head = format!("{DECISIONS}:");
    let lines: Vec<&str> = delivery.lines().collect();
    let Some(at) = lines.iter().position(|line| line.starts_with(&head)) else {
        return format!("{head} (absent)");
    };
    let end = lines[at + 1..]
        .iter()
        .position(|line| !line.starts_with(char::is_whitespace) || line.trim().is_empty())
        .map(|offset| at + 1 + offset)
        .unwrap_or(lines.len());
    lines[at..end].join("\n")
}

/// The calls the delivery numbered, by their `D<k>` names, in the order the
/// note lists them.
pub fn decisions(delivery: &str) -> Vec<String> {
    decisions_block(delivery)
        .lines()
        .filter_map(|line| numbered(line.trim(), 'D'))
        .collect()
}

/// A `D1`/`F2` name at the start of a line: the letter, digits, and then a
/// boundary. `D1 the call` is one and `Dispatch` is not.
fn numbered(line: &str, letter: char) -> Option<String> {
    let rest = line.strip_prefix(letter)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    match rest[digits.len()..].chars().next() {
        None | Some(' ') | Some('.') | Some(':') | Some(')') => Some(format!("{letter}{digits}")),
        _ => None,
    }
}

/// The walk: one line per call the delivery numbered, then the count.
///
/// `--land` is the accept, so every call it walks is accepted; a call the
/// reviewer will not take is a finding and the delivery goes back with it.
fn walk(delivery: &str) -> String {
    let calls = decisions(delivery);
    let mut lines: Vec<String> = calls.iter().map(|name| format!("{name} ACCEPT")).collect();
    lines.push(format!("{} accepted, 0 overruled", calls.len()));
    lines.join("\n  ")
}

/// The header's count against the calls the walk finds. The `decisions:` line
/// at column zero reads `N` or `none`; an absent, unreadable or disagreeing
/// header is refused, because a call the walk does not find is one nobody
/// reviewed.
fn counted(item: &str, delivery: &str) -> Result<(), Stop> {
    let Some(header) = label_value(delivery, DECISIONS) else {
        return Err(Stop::refused(format!(
            "{item}'s delivery carries no `{DECISIONS}:` line at column zero — the header \
             counts the calls and the walk is checked against it"
        )));
    };
    let token = header.split_whitespace().next().unwrap_or("");
    let said = match token {
        "none" => Some(0),
        count => count.parse::<usize>().ok(),
    };
    let found = decisions(delivery).len();
    match said {
        None => Err(Stop::refused(format!(
            "{item}'s delivery says `{DECISIONS}: {header}`, which is not a count — the header \
             reads `{DECISIONS}: <N>` or `{DECISIONS}: none`"
        ))),
        Some(n) if n != found => Err(Stop::refused(format!(
            "{item}'s delivery says `{DECISIONS}: {n}` and the walk found {found} — the calls \
             under the header are indented, one `D<k>` per line"
        ))),
        Some(_) => Ok(()),
    }
}

// ---- the two verdicts --------------------------------------------------------

fn accept(
    item: &Item,
    delivery: &str,
    commit: &str,
    size: &str,
    verdict: &Verdict,
    wiring: &Wiring,
) -> Result<String, Stop> {
    counted(&item.id, delivery)?;
    let note = render(
        &block(&wiring.packs.read(VERDICT)?, VERDICT_MARKERS[0])?,
        &[
            ("commit", commit),
            ("reviewer", verdict.by),
            ("item", &item.id),
            ("size", size),
            ("decisions", &walk(delivery)),
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
            "accepted": decisions(delivery).len(),
            "overruled": 0,
        }),
    )?;
    Ok(note)
}

/// The one event either writing mode appends (flights PRD Q4a).
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
    let body = std::fs::read_to_string(findings).map_err(|e| {
        Stop::usage(format!(
            "the findings at {} could not be read: {e}",
            findings.display()
        ))
    })?;
    let count = body
        .lines()
        .filter_map(|line| numbered(line.trim(), 'F'))
        .count();
    if count == 0 {
        return Err(Stop::usage(format!(
            "{} numbers no finding — a return that numbers nothing is a question and goes back as \
             one",
            findings.display()
        )));
    }
    // The builder is the order index's seat: the record of who was given this
    // item, which is the one place that says where a return goes.
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
            ("reviewer", verdict.by),
            ("item", &item.id),
            ("size", size),
            ("findings", &count.to_string()),
            ("body", &off_column_zero(body.trim_end())),
        ],
    )
    .map_err(|name| unresolved(&name))?;

    wiring
        .store
        .assign(&item.id, &builder, verdict.by)
        .map_err(unreadable)?;
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
                "{STANDS}: no live session for {builder}; the return stands and their successor \
                 reads it at wake"
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
    wiring
        .store
        .note(item, note, verdict.by)
        .map_err(unreadable)?;
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

/// The findings body, indented off column zero.
///
/// A region ends at the next marker AT COLUMN ZERO, and a findings body is
/// prose a reviewer wrote — which in this store habitually quotes a delivery's
/// or a landing's own first line. Indented, no line of it can end the verdict
/// it is inside, so the read-back below compares the whole note against the
/// whole note. The count was taken from the body as written and each line is
/// still read with `trim`, so nothing about what a finding IS changes here.
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
    store.show(item).map_err(unreadable)
}

fn unreadable(e: StoreError) -> Stop {
    match e {
        StoreError::Missing(why) => Stop::refused(why),
        StoreError::Unreadable(why) => Stop::could_not_tell(why),
    }
}
