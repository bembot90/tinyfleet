//! `fleet item show <id>` — an item and its timeline, as fleet renders them.
//!
//! PRESENTATION IS THE VIEWER'S. The store keeps fields and entries; how a
//! person reads them is decided here, once, and the brief's `## The item` block
//! is this same text — so a seat and a person read one rendering, and neither
//! reads the JSON an entry is kept as.
//!
//! Two shapes of one reading: [`render`] for a person and [`document`] for a
//! caller that parses. Neither reads the store; the caller hands both the item
//! and its timeline, read once.
//!
//! A person's comment on the item is not in either. The timeline a store
//! answers has already left it out, and a rendering that went back for it
//! would be a second reader of the store beside [`Store::timeline`].
//!
//! [`Store::timeline`]: crate::store::Store::timeline

use serde::Serialize;
use serde_json::Value;

use crate::entry::{
    self, Body, Clearance, Cleared, Delivered, Entry, Held, Landed, OrderWithdrawn, Ordered,
    Reviewed, RulingKind, SuiteRun, Verdict,
};
use crate::item::{land, TRUNK};
use crate::seat::identity::SeatId;
use crate::store::{Item, OrderState};

/// How far an entry's detail lines sit under its summary.
const DETAIL: usize = 4;

/// How far a list's rows sit under the field that heads them, and a
/// multi-line value's continuation lines under the line it starts on.
const UNDER: usize = 2;

/// The item and its timeline, as a person reads them: the fields, the
/// description, then one block per entry in the store's order. No trailing
/// newline — the caller prints it as a line or puts it in a fence.
pub fn render(item: &Item, timeline: &[Entry]) -> String {
    let mut lines = vec![
        format!("{} · {}  [{}]", item.id, item.title, item.status),
        format!(
            "type {} · labels {} · assignee {}",
            item.item_type,
            joined_or_none(&item.labels),
            holder_or_none(item.assignee)
        ),
        order_line(item),
    ];
    if !item.blockers.is_empty() {
        let blockers: Vec<&str> = item.blockers.iter().map(|id| id.as_str()).collect();
        lines.push(format!("blocked by {}", blockers.join(", ")));
    }
    lines.push(String::new());
    lines.push(if item.description.is_empty() {
        String::from("(no description)")
    } else {
        item.description.clone()
    });
    lines.push(String::new());
    lines.push(if timeline.is_empty() {
        String::from("timeline (no entries)")
    } else {
        format!("timeline ({} entries)", timeline.len())
    });
    for entry in timeline {
        lines.extend(entry_lines(entry));
    }
    lines.join("\n")
}

/// The item and its timeline, as a caller parses them. Each entry is
/// [`entry::to_json`]'s, the contract's own entry, so this document, an
/// adapter's `timeline` answer and the entry model cannot disagree about what
/// an entry carries — its `by` included, as the actor's one string.
pub fn document(item: &Item, timeline: &[Entry]) -> Value {
    let order = order_json(&item.order);
    serde_json::json!({
        "id": item.id,
        "title": item.title,
        "description": item.description,
        "status": item.status,
        "type": item.item_type,
        "labels": item.labels,
        "assignee": item.assignee,
        "order": order,
        "blockers": item.blockers,
        "timeline": timeline.iter().map(entry::to_json).collect::<Vec<_>>(),
    })
}

/// One entry's block: `{at}  {by}  {summary}`, then its detail lines indented
/// under it.
pub fn entry_lines(entry: &Entry) -> Vec<String> {
    let mut lines = Vec::new();
    push(
        &mut lines,
        0,
        &format!("{}  {}  {}", entry.at, entry.by, summary(&entry.body)),
    );
    match &entry.body {
        Body::Ordered(_) | Body::OrderWithdrawn(_) | Body::Cleared(_) => {}
        Body::Delivered(delivered) => delivery(&mut lines, delivered),
        Body::Reviewed(reviewed) => review(&mut lines, reviewed),
        Body::Held(held) => hold(&mut lines, held),
        Body::Landed(landed) => landing(&mut lines, landed),
    }
    lines
}

// ---- the fields -------------------------------------------------------------

/// The order index, read as the verbs read it: absent is `none`, and a key
/// that holds no order this fleet can read is `unreadable`.
fn order_line(item: &Item) -> String {
    match &item.order {
        OrderState::None => String::from("order none"),
        OrderState::Unreadable => String::from("order unreadable"),
        OrderState::Ordered(order) => format!(
            "order {} by {} at {}, seat {}",
            order.kind.as_str(),
            order.by,
            order.at,
            order
                .seat
                .map(|seat| seat.to_string())
                .unwrap_or_else(|| String::from("not yet named"))
        ),
    }
}

/// The order index as a caller parses it, the one shape `item show` and `item
/// list` both answer: `null` for none, `{"unreadable": true}` for an index
/// this fleet cannot read, and the order's four fields — the seat `null` where
/// the order names none yet.
pub fn order_json(order: &OrderState) -> Value {
    match order {
        OrderState::None => Value::Null,
        OrderState::Unreadable => serde_json::json!({ "unreadable": true }),
        OrderState::Ordered(order) => serde_json::json!({
            "by": order.by,
            "kind": order.kind,
            "seat": order.seat,
            "at": order.at,
        }),
    }
}

/// The seat holding an item, by its full id, or `none`.
pub(crate) fn holder_or_none(assignee: Option<SeatId>) -> String {
    assignee.map_or_else(|| String::from("none"), |seat| seat.to_string())
}

fn joined_or_none(list: &[String]) -> String {
    if list.is_empty() {
        String::from("none")
    } else {
        list.join(", ")
    }
}

// ---- the summaries ----------------------------------------------------------

fn summary(body: &Body) -> String {
    match body {
        Body::Ordered(ordered) => order(ordered),
        Body::OrderWithdrawn(withdrawn) => withdrawal(withdrawn),
        Body::Delivered(delivered) => format!(
            "delivered {} on {}, base {}",
            delivered.commit, delivered.branch, delivered.base
        ),
        Body::Reviewed(reviewed) => match reviewed.verdict {
            Verdict::Accepted => format!("accepted {}", reviewed.commit),
            Verdict::Returned => format!(
                "returned {} with {} finding(s)",
                reviewed.commit,
                reviewed.findings.len()
            ),
        },
        Body::Held(held) => format!(
            "held {} — {}: {}",
            held.hold,
            word(&held.reason),
            held.question
        ),
        Body::Cleared(cleared) => clearance(cleared),
        Body::Landed(landed) => {
            let through = landed
                .run
                .as_ref()
                .map(|run| format!("; through run {run}"))
                .unwrap_or_default();
            format!(
                "landed {sha} (range {old}..{sha}; squash of {squash}{through})",
                sha = landed.sha,
                old = landed.old,
                squash = landed.squash_of,
            )
        }
    }
}

fn order(ordered: &Ordered) -> String {
    let seat = ordered
        .seat
        .as_ref()
        .map(|seat| seat.to_string())
        .unwrap_or_else(|| String::from("a transient seat, not yet named"));
    format!("ordered {} → {seat}", word(&ordered.order))
}

fn withdrawal(withdrawn: &OrderWithdrawn) -> String {
    let mut line = format!("order withdrawn ({})", word(&withdrawn.why));
    if let Some(seat) = &withdrawn.seat {
        line.push_str(&format!(", seat {seat}"));
    }
    if let Some(cause) = &withdrawn.cause {
        line.push_str(&format!(": {cause}"));
    }
    line
}

fn clearance(cleared: &Cleared) -> String {
    match cleared.how {
        Clearance::Cancel => format!("cleared {} — cancelled", cleared.hold),
        Clearance::Answer => {
            let letter = cleared.letter.as_deref().unwrap_or_default();
            match &cleared.text {
                Some(text) => format!("cleared {} — answered {letter}: {text}", cleared.hold),
                None => format!("cleared {} — answered {letter}", cleared.hold),
            }
        }
    }
}

// ---- the details ------------------------------------------------------------

fn delivery(lines: &mut Vec<String>, delivered: &Delivered) {
    push(
        lines,
        DETAIL,
        &format!("files: {}", delivered.files.join(", ")),
    );
    list(
        lines,
        "checks",
        delivered
            .checks
            .iter()
            .map(|row| format!("- {}: {}", row.check, row.result)),
    );
    push(lines, DETAIL, &suite("suite", &delivered.suite));
    list(
        lines,
        "spec corrections",
        delivered.spec_corrections.iter().map(|correction| {
            format!(
                "- {} — refuted by {}",
                correction.premise, correction.refuted_by
            )
        }),
    );
    list(
        lines,
        "not proven",
        delivered
            .not_proven
            .iter()
            .map(|gap| format!("- {} — {}", gap.surface, gap.command)),
    );
    list(
        lines,
        "decisions",
        delivered.decisions.iter().enumerate().map(|(n, decision)| {
            format!(
                "D{} {}; not taken: {}; because {}",
                n + 1,
                decision.call,
                decision.not_taken,
                decision.because
            )
        }),
    );
    push(
        lines,
        DETAIL,
        &format!("covers: {}", joined_or_none(&delivered.covers)),
    );
}

fn review(lines: &mut Vec<String>, reviewed: &Reviewed) {
    let size = &reviewed.size;
    let binary = if size.binary > 0 {
        format!(" ({} binary)", size.binary)
    } else {
        String::new()
    };
    push(
        lines,
        DETAIL,
        &format!(
            "size: {} file(s), +{}, -{}{binary} — tests: {}, executable: {}, against {}",
            size.files,
            size.added,
            size.deleted,
            yes_no(size.tests),
            yes_no(size.executable),
            size.base
        ),
    );
    match reviewed.verdict {
        Verdict::Accepted => {
            let walk: Vec<String> = reviewed
                .walk
                .iter()
                .map(|ruling| {
                    let ruled = match ruling.ruling {
                        RulingKind::Accept => "accept",
                        RulingKind::Overrule => "overrule",
                    };
                    format!("D{} {ruled}", ruling.decision)
                })
                .collect();
            push(lines, DETAIL, &format!("walk: {}", joined_or_none(&walk)));
        }
        Verdict::Returned => {
            for (n, finding) in reviewed.findings.iter().enumerate() {
                push(lines, DETAIL, &format!("F{} {}", n + 1, finding.text));
            }
        }
    }
}

fn hold(lines: &mut Vec<String>, held: &Held) {
    for line in held.context.iter().flat_map(|context| context.lines()) {
        push(lines, DETAIL, line);
    }
    for choice in &held.options {
        push(
            lines,
            DETAIL,
            &format!("{}. {}", choice.letter, choice.text),
        );
    }
    let on = match (&held.run_hash, &held.branch, &held.commit) {
        (Some(hash), _, _) => format!("on the run's hash {hash}"),
        (None, branch, commit) => format!(
            "on {} at {}",
            branch.as_deref().unwrap_or("(none)"),
            commit.as_deref().unwrap_or("(none)")
        ),
    };
    push(lines, DETAIL, &on);
    if let Some(about) = &held.about {
        let at = about
            .commit
            .as_ref()
            .map(|commit| format!(" at {commit}"))
            .unwrap_or_default();
        push(
            lines,
            DETAIL,
            &format!(
                "about {}{at}; {} licenses a landing",
                about.items.join(", "),
                about.licenses
            ),
        );
    }
}

fn landing(lines: &mut Vec<String>, landed: &Landed) {
    push(lines, DETAIL, &suite("test", &landed.test));
    for (n, row) in landed.checks.iter().enumerate() {
        push(
            lines,
            DETAIL,
            &land::row(n + 1, &row.check, &row.verdict, &row.evidence),
        );
    }
    push(
        lines,
        DETAIL,
        &format!(
            "work branch: {} — {}",
            landed.work_branch.branch.as_deref().unwrap_or("(none)"),
            word(&landed.work_branch.classification)
        ),
    );
    list(lines, "commands", commands(landed).into_iter());
}

/// The commands that re-run each of a landing's verdicts, over what the entry
/// carries: the commit it squashed, the sha it landed and the work branch it
/// classified. The next reader re-runs a verdict rather than believing it.
fn commands(landed: &Landed) -> Vec<String> {
    let commit = &landed.squash_of;
    let sha = &landed.sha;
    let mut lines = vec![
        format!("git merge-base {TRUNK} {commit}"),
        format!("git diff --name-only $(git merge-base {TRUNK} {commit}) {commit}"),
        format!("git show --stat {sha}"),
        format!("git rev-list --count {sha}..{TRUNK}"),
    ];
    if let Some(branch) = &landed.work_branch.branch {
        lines.push(format!("git rev-parse {branch}"));
        lines.push(format!("git diff {commit} {sha} --"));
    }
    lines.push(String::from("git status --porcelain"));
    lines
}

/// A suite that ran, with its command and exit, or the reason it did not.
fn suite(field: &str, run: &SuiteRun) -> String {
    match run {
        SuiteRun::Ran(ran) => format!("{field}: {}, rc {}", ran.command, ran.rc),
        SuiteRun::NotTested(not) => format!("{field}: NOT TESTED — {}", not.not_tested),
    }
}

/// A list under the field that names it — the field alone and each row under
/// it — or `<field>: none` where it holds nothing.
fn list(lines: &mut Vec<String>, field: &str, rows: impl Iterator<Item = String>) {
    let rows: Vec<String> = rows.collect();
    if rows.is_empty() {
        push(lines, DETAIL, &format!("{field}: none"));
        return;
    }
    push(lines, DETAIL, &format!("{field}:"));
    for row in rows {
        push(lines, DETAIL + UNDER, &row);
    }
}

fn yes_no(held: bool) -> &'static str {
    if held {
        "yes"
    } else {
        "no"
    }
}

/// An enum's value as the entry's text spells it, so the rendering and the
/// JSON say one word for one thing.
fn word(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(word)) => word,
        other => format!("{other:?}"),
    }
}

/// One line at `indent`, its continuation lines — a value that runs over more
/// than one — indented under it.
fn push(lines: &mut Vec<String>, indent: usize, text: &str) {
    let mut parts = text.split('\n');
    let first = parts.next().unwrap_or_default();
    lines.push(format!("{:indent$}{first}", ""));
    for rest in parts {
        lines.push(format!("{:width$}{rest}", "", width = indent + UNDER));
    }
}
