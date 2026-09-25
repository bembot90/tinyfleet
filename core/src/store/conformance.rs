//! The store contract as checks, one function each, that any [`Store`] is
//! asked: what a write moves, what the read beside it answers, and what the
//! store refuses.
//!
//! IN THE LIBRARY AND NOT IN A SUITE, because a store is asked these where it
//! runs: `fleet store check` runs [`run`] against an adapter on a scratch store
//! it made, and `core/tests/contract.rs` runs every check against the store
//! held in memory, one arm each, and the whole table against the built-in
//! store.
//!
//! EACH CHECK ANSWERS AND NONE PANICS. A check passes, fails with a text naming
//! what the store answered, or is skipped with why — a store that declares no
//! export has none to check. [`run`] asks every check whatever the one before
//! it answered, so one run names every disagreement and not the first.
//!
//! WHAT IS CHECKED IS THE CONTRACT AND NOT THE ROW. A store writes fields no
//! trait method answers — a close reason, an updated stamp — and two stores
//! agreeing byte for byte on a document is not what the verbs need. What they
//! need is that a write moves what the read beside it answers, in the same
//! direction, and that is what each check names.
//!
//! A CHECK READS THE ITEMS IT FILED AND NEVER THE WHOLE STORE, bar the first:
//! [`CHECKS`] runs in order on one store, so a listing read whole would be
//! reading what the checks before it left behind. "empty listings" is first
//! because it is the one check that needs a store nothing has written to.

use std::collections::BTreeSet;
use std::path::Path;

use super::{
    Filter, Item, ItemId, ItemSummary, NewItem, Order, OrderKind, OrderState, RunRecord, Stamp,
    Status, Store, StoreError, Update, WithdrawFence,
};
use crate::entry::{
    Body, Delivered, NotProven, OrderWithdrawn, Ordered, Ran, SuiteRun, Withdrawal,
};
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::SeatId;

/// A check that did not fail: it passed, or it was not asked of this store,
/// and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Passed {
    Pass,
    Skip(String),
}

/// What a check is run against.
///
/// `root` is the directory the export is written under — the destination is
/// the caller's, and two stores keep theirs in two places. `absent` is the
/// same adapter addressed at a directory holding no store, which is the one
/// store that answers nothing.
///
/// `another_writer` plants ANOTHER TOOL'S METADATA on an item: a JSON object
/// merged at the top level, the way a project's own tooling writes beside
/// fleet's keys. The trait writes only the contract's types, so a caller that
/// can plant one some way of its own — the board held in memory through its
/// rig, the built-in store through its binary — hands it in, and the check
/// that needs one is skipped where none was handed.
pub struct Ctx<'a> {
    pub store: &'a dyn Store,
    pub root: &'a Path,
    pub absent: &'a dyn Store,
    pub another_writer: Option<&'a AnotherWriter<'a>>,
}

/// Another writer's metadata onto an item: the item's id, and the object as
/// JSON text.
pub type AnotherWriter<'a> = dyn Fn(&str, &str) -> Result<(), String> + 'a;

/// One check: passed or skipped, or the text of what the store answered
/// instead.
pub type Check = fn(&Ctx) -> Result<Passed, String>;

/// Every check, by name, in the order [`run`] asks them.
///
/// The contract's nineteen first, in the spec's order; then the four the
/// suite had grown beside them before it moved here — an order read back,
/// an update naming nothing, the fenced writes, and another writer's keys —
/// so nothing it asked is lost in the move, with the three that check the
/// contract's own fences before the last of them; then another writer's
/// keys as the read and the listing name them.
pub const CHECKS: &[(&str, Check)] = &[
    ("empty listings", empty_listings),
    ("version", version),
    ("capabilities", capabilities),
    ("create then show", create_then_show),
    ("resolve by fragment", resolve_by_fragment),
    ("missing is refused", missing_is_refused),
    ("ambiguous is refused with candidates", ambiguous_is_refused),
    ("unreadable is could not tell", unreadable_is_could_not_tell),
    ("ready", ready),
    ("label filter", label_filter),
    ("assignee filter", assignee_filter),
    ("update", update),
    ("an order write keeps the run record", order_keeps_the_run),
    ("a run write keeps the order", run_keeps_the_order),
    ("order.withdraw clears both", withdraw_clears_both),
    ("timeline is append-only and ordered", timeline),
    ("holds", holds),
    ("close", close),
    ("export", export),
    ("order.set reads back", order_reads_back),
    ("update naming nothing", update_naming_nothing),
    ("fenced writes", fenced_writes),
    ("fenced update refuses moved", fenced_update),
    ("reopen through update", update_to_open),
    ("fenced withdraw with reopen", fenced_withdraw),
    ("another writer's keys", another_writers_keys),
    (
        "another writer's keys are listed as foreign",
        another_writers_keys_are_foreign,
    ),
];

/// Every check against the one store, in [`CHECKS`]' order, each answer
/// beside its name. A failure never stops the run.
///
/// A CHECK IS ASKED ONLY AS THE NEXT ANSWER IS READ, so a caller prints each
/// answer as it lands: the whole table on a store that forks a process per
/// call takes long enough that a silent run reads as a hung one.
pub fn run<'c>(
    ctx: &'c Ctx<'c>,
) -> impl Iterator<Item = (&'static str, Result<Passed, String>)> + 'c {
    CHECKS.iter().map(move |(name, check)| (*name, check(ctx)))
}

type Answer = Result<Passed, String>;

/// The label every item a check files carries.
const LABEL: &str = "fleet-conformance";

/// An id no check files.
const NOBODY: &str = "fleet-conformance-nobody-filed-this";

/// When the orders and records the checks write were given.
const AT: &str = "2026-09-13T00:00:00Z";
const LATER: &str = "2026-09-13T01:00:00Z";

/// How many items the ambiguity check files before it gives up: one more than
/// the 36 characters a base-36 hash can open with, so two of them always share
/// a first character — the built-in store mints ids that way, measured on its
/// pinned release.
const AMBIGUOUS_WITHIN: usize = 37;

/// Who the checks write as: a run, because the suite is no seat's act.
fn by() -> Actor {
    Actor {
        kind: ActorKind::Run,
        id: String::from("fleet-conformance"),
    }
}

/// Who an order names as its dispatcher.
fn architect() -> Actor {
    Actor {
        kind: ActorKind::Run,
        id: String::from("fleet-conformance-architect"),
    }
}

// ---- reading an answer ---------------------------------------------------------

/// A store call's answer, or its refusal as the check's failure, naming the
/// call.
fn answered<T>(call: &str, answer: Result<T, StoreError>) -> Result<T, String> {
    answer.map_err(|e| format!("{call} answered {}", refusal(&e)))
}

/// A refusal, by its kind and its text.
fn refusal(e: &StoreError) -> String {
    match e {
        StoreError::Refused(why) => format!("Refused ({why})"),
        StoreError::Moved(why) => format!("Moved ({why})"),
        StoreError::Usage(why) => format!("Usage ({why})"),
        StoreError::Unreadable(why) => format!("Unreadable ({why})"),
    }
}

/// An answer as a failure's text names it.
fn outcome<T: std::fmt::Debug>(answer: &Result<T, StoreError>) -> String {
    match answer {
        Ok(value) => format!("Ok({value:?})"),
        Err(e) => refusal(e),
    }
}

/// A fenced write the item did not meet: Moved, naming every one of `names` —
/// the item, and what holds it where the check knows.
fn moved(call: &str, answer: Result<(), StoreError>, names: &[&str]) -> Result<(), String> {
    match answer {
        Err(StoreError::Moved(why)) => ensure(names.iter().all(|name| why.contains(name)), || {
            format!("{call} was Moved without naming {names:?}: {why}")
        }),
        answer => Err(format!(
            "{call} answered {}, and a fenced write that does not hold is Moved",
            outcome(&answer)
        )),
    }
}

/// The contract's word on a reading, or `why` as the failure.
fn ensure(held: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if held {
        Ok(())
    } else {
        Err(why())
    }
}

/// One field against what the contract says it reads.
fn same<T: PartialEq + std::fmt::Debug>(what: &str, got: &T, want: &T) -> Result<(), String> {
    ensure(got == want, || {
        format!("{what}: the store answered {got:?}, and the contract says {want:?}")
    })
}

fn stamp(text: &str) -> Result<Stamp, String> {
    Stamp::parse(text).ok_or_else(|| format!("`{text}` is not a stamp"))
}

/// The part of an id after its last `-`, or the whole id where it has none.
fn suffix(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(_, rest)| rest)
}

fn ids(rows: &[ItemSummary]) -> Vec<String> {
    rows.iter().map(|row| row.id.to_string()).collect()
}

// ---- the store calls every check makes ---------------------------------------

/// One item filed under [`LABEL`] and any label beside it, answered as the id
/// the store gave it.
fn filed(ctx: &Ctx, title: &str, labels: &[&str]) -> Result<ItemId, String> {
    let mut all = vec![String::from(LABEL)];
    all.extend(labels.iter().map(|label| label.to_string()));
    let id = answered(
        &format!("create `{title}`"),
        ctx.store.create(
            &NewItem {
                title: title.to_string(),
                description: String::from("an item the conformance suite filed"),
                item_type: String::from("task"),
                labels: all,
                priority: None,
            },
            &by(),
        ),
    )?;
    ensure(!id.is_empty(), || {
        format!("create `{title}` answered an empty id")
    })?;
    Ok(id)
}

fn read(ctx: &Ctx, id: &ItemId) -> Result<Item, String> {
    answered(&format!("show {id}"), ctx.store.show(id))
}

fn listed(ctx: &Ctx, filter: &Filter) -> Result<Vec<ItemSummary>, String> {
    answered(&format!("list {filter:?}"), ctx.store.list(filter))
}

/// The item's row in a listing, or `None` where the listing leaves it out.
fn row_in(ctx: &Ctx, filter: &Filter, id: &ItemId) -> Result<Option<ItemSummary>, String> {
    Ok(listed(ctx, filter)?.into_iter().find(|row| row.id == *id))
}

fn assign(ctx: &Ctx, id: &ItemId, seat: SeatId) -> Result<(), String> {
    answered(
        &format!("update {id} assignee {seat}"),
        ctx.store.update(id, &Update::assignee(seat), &by()),
    )
}

fn closed(ctx: &Ctx, id: &ItemId, reason: &str, closer: &Actor) -> Result<(), String> {
    answered(&format!("close {id}"), ctx.store.close(id, reason, closer))
}

fn ordered(ctx: &Ctx, id: &ItemId, order: &Order) -> Result<(), String> {
    answered(
        &format!("order.set {id}"),
        ctx.store.order_set(id, order, &by()),
    )
}

fn recorded(ctx: &Ctx, id: &ItemId, run: &RunRecord) -> Result<(), String> {
    answered(&format!("run.set {id}"), ctx.store.run_set(id, run, &by()))
}

/// A dispatch for `seat`, given by the architect at [`AT`].
fn an_order(seat: Option<SeatId>) -> Result<Order, String> {
    Ok(Order {
        kind: OrderKind::Dispatch,
        by: architect(),
        seat,
        at: stamp(AT)?,
    })
}

/// A second order, which replaces the first: a review, for another seat,
/// later.
fn another_order(seat: SeatId) -> Result<Order, String> {
    Ok(Order {
        kind: OrderKind::Review,
        by: architect(),
        seat: Some(seat),
        at: stamp(LATER)?,
    })
}

/// A run's record, pinned to `hash`.
fn a_record(hash: &str, workflow: &str) -> Result<RunRecord, String> {
    Ok(RunRecord {
        hash: hash.to_string(),
        workflow: workflow.to_string(),
        pack: String::from("ts"),
        entry: String::from("greet.ts"),
        started_at: stamp(AT)?,
    })
}

/// A delivery that keeps every rule its kind has.
fn a_delivery() -> Body {
    Body::Delivered(Delivered {
        commit: String::from("1111111111111111111111111111111111111111"),
        branch: String::from("work/fleet-conformance"),
        base: String::from("0123456789abcdef0123456789abcdef01234567"),
        files: vec![String::from("core/src/store/conformance.rs")],
        checks: Vec::new(),
        suite: SuiteRun::Ran(Ran {
            command: String::from("fleet store check"),
            rc: 0,
        }),
        spec_corrections: Vec::new(),
        not_proven: vec![NotProven {
            surface: String::from("a store under load"),
            command: String::from("fleet store check"),
        }],
        decisions: Vec::new(),
        covers: Vec::new(),
    })
}

// ---- the contract's checks ---------------------------------------------------

/// A store nothing has written to holds no open hold, no ready item and no
/// item under a label.
fn empty_listings(ctx: &Ctx) -> Answer {
    let open = answered("holds.open", ctx.store.holds_open())?;
    ensure(open.is_empty(), || {
        format!("holds.open of a fresh store answered {open:?}, and it holds none")
    })?;
    for filter in [
        Filter::Ready,
        Filter::Label(String::from("fleet-conformance-none")),
    ] {
        let rows = listed(ctx, &filter)?;
        ensure(rows.is_empty(), || {
            format!(
                "list {filter:?} of a fresh store answered {:?}, and it holds no item",
                ids(&rows)
            )
        })?;
    }
    Ok(Passed::Pass)
}

/// The store names itself and the version it is at, neither of them empty.
fn version(ctx: &Ctx) -> Answer {
    let answer = answered("version", ctx.store.version())?;
    ensure(!answer.name.is_empty(), || {
        format!("version answered no name: {answer:?}")
    })?;
    ensure(!answer.version.is_empty(), || {
        format!("version answered no version: {answer:?}")
    })?;
    Ok(Passed::Pass)
}

/// What the store declares holds the contract's rules: an export a landing
/// can commit, both paths relative and under the project root and the file
/// in the directory; a command that is one word; and items with at least one
/// type and a priority range inside 0 to 4 that is not upside down.
fn capabilities(ctx: &Ctx) -> Answer {
    let declared = answered("capabilities", ctx.store.capabilities())?;
    declared
        .validate()
        .map_err(|why| format!("what the store declares does not validate: {why}"))?;
    Ok(Passed::Pass)
}

/// A create answers an id, and the next read of it answers the new item's
/// fields: open, nobody holding it, no order and no run's record.
fn create_then_show(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item to read back", &[])?;
    let item = read(ctx, &id)?;
    same("the read's id", &item.id, &id)?;
    same("the title", &item.title.as_str(), &"an item to read back")?;
    same(
        "the description",
        &item.description.as_str(),
        &"an item the conformance suite filed",
    )?;
    same("a fresh item's status", &item.status, &Status::Open)?;
    same("the type", &item.item_type.as_str(), &"task")?;
    ensure(item.labels.iter().any(|label| label == LABEL), || {
        format!(
            "the labels the new item was filed under: the store answered {:?}",
            item.labels
        )
    })?;
    same(
        "the assignee of an item nobody has assigned",
        &item.assignee,
        &None,
    )?;
    same("a fresh item's order", &item.order, &OrderState::None)?;
    same("a fresh item's run record", &item.run, &None)?;
    Ok(Passed::Pass)
}

/// The part of the id after its prefix — what a person types — resolves to the
/// FULL id, and a read of it answers the item under that id.
fn resolve_by_fragment(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item resolved by a fragment", &[])?;
    let fragment = suffix(&id);
    for text in [fragment, id.as_str()] {
        let resolved = answered(&format!("resolve `{text}`"), ctx.store.resolve(text))?;
        same(&format!("resolve `{text}`"), &resolved, &id)?;
        let shown = answered(&format!("show `{text}`"), ctx.store.show(text))?;
        same(&format!("the id show `{text}` answers"), &shown.id, &id)?;
    }
    Ok(Passed::Pass)
}

/// An id no item carries is the record's answer — Refused — and never a store
/// that could not tell: to a read, and to a write, which is the title's
/// `update` because every store takes that one the same way.
fn missing_is_refused(ctx: &Ctx) -> Answer {
    let resolved = ctx.store.resolve(NOBODY);
    ensure(matches!(resolved, Err(StoreError::Refused(_))), || {
        format!(
            "resolve `{NOBODY}` answered {}, and an id no item carries is Refused",
            outcome(&resolved)
        )
    })?;
    let shown = ctx.store.show(NOBODY).map(|item| item.id);
    ensure(matches!(shown, Err(StoreError::Refused(_))), || {
        format!(
            "show `{NOBODY}` answered {}, and an id no item carries is Refused",
            outcome(&shown)
        )
    })?;
    let updated = ctx.store.update(
        &ItemId::from(NOBODY),
        &Update::title(String::from("a title for nobody")),
        &by(),
    );
    ensure(matches!(updated, Err(StoreError::Refused(_))), || {
        format!(
            "an update of `{NOBODY}`'s title answered {}, and a write to an id no item \
             carries is Refused",
            outcome(&updated)
        )
    })?;
    Ok(Passed::Pass)
}

/// Text naming more than one item is Refused, and the refusal names the items
/// it could mean.
///
/// THE FRAGMENT IS A COMMON PREFIX of two filed items' hashes, longest first,
/// and equal to neither. A prefix is what the contract's partial-id rule reads
/// as a match, and the built-in store at its pinned release reads the same —
/// measured on a scratch board, `yc` answered not found beside `fx-0yc` and
/// `fx-cyc` while `0` answered both of `fx-0li` and `fx-0yc`. A fragment some
/// other item's hash IS resolves to that item, which is no ambiguity: the next
/// is tried.
fn ambiguous_is_refused(ctx: &Ctx) -> Answer {
    let mut mine: Vec<ItemId> = Vec::new();
    let mut tried: BTreeSet<String> = BTreeSet::new();
    for n in 1..=AMBIGUOUS_WITHIN {
        let newest = filed(
            ctx,
            &format!("an item an ambiguous fragment names, {n}"),
            &[],
        )?;
        for other in &mine {
            let (a, b) = (suffix(other), suffix(&newest));
            let shared = a
                .char_indices()
                .map(|(at, c)| at + c.len_utf8())
                .map(|end| &a[..end])
                .take_while(|prefix| b.starts_with(prefix))
                .filter(|prefix| *prefix != a && *prefix != b)
                .collect::<Vec<&str>>();
            for fragment in shared.into_iter().rev() {
                if !tried.insert(fragment.to_string()) {
                    continue;
                }
                match ctx.store.resolve(fragment) {
                    Err(StoreError::Refused(why)) => {
                        ensure(
                            why.contains(other.as_str()) && why.contains(newest.as_str()),
                            || {
                                format!(
                                    "resolve `{fragment}`, which both {other} and {newest} \
                                     open with, was Refused without naming both: {why}"
                                )
                            },
                        )?;
                        return Ok(Passed::Pass);
                    }
                    Ok(one) if suffix(&one) == fragment => continue,
                    answer => {
                        return Err(format!(
                            "resolve `{fragment}`, which both {other} and {newest} open with, \
                             answered {}, and text naming two items is Refused",
                            outcome(&answer)
                        ))
                    }
                }
            }
        }
        mine.push(newest);
    }
    Err(format!(
        "could not make an ambiguous fragment in {AMBIGUOUS_WITHIN} creates: {:?}",
        mine.iter().map(|id| id.as_str()).collect::<Vec<&str>>()
    ))
}

/// A store that is not there is a store that could not tell — Unreadable, and
/// never the record's answer that the item is not in it.
fn unreadable_is_could_not_tell(ctx: &Ctx) -> Answer {
    let shown = ctx.absent.show("x").map(|item| item.id);
    ensure(matches!(shown, Err(StoreError::Unreadable(_))), || {
        format!(
            "show `x` of a store that is not there answered {}, and it is Unreadable, never \
             Refused",
            outcome(&shown)
        )
    })?;
    let ready = ctx.absent.list(&Filter::Ready).map(|rows| ids(&rows));
    ensure(matches!(ready, Err(StoreError::Unreadable(_))), || {
        format!(
            "list Ready of a store that is not there answered {}, and it is Unreadable",
            outcome(&ready)
        )
    })?;
    Ok(Passed::Pass)
}

/// A new item is ready, as one row carrying its title, status, type, own
/// labels, order, nobody holding it and no run's record — and the record once
/// one is written, off the row and not a second read; a hold takes it out,
/// the hold's clear brings it back and a close takes it out again.
fn ready(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item the ready set answers", &[])?;
    let row = row_in(ctx, &Filter::Ready, &id)?
        .ok_or_else(|| format!("a new item, {id}, is not in the ready set"))?;
    same(
        "the ready row's title",
        &row.title.as_str(),
        &"an item the ready set answers",
    )?;
    same("the ready row's status", &row.status, &Status::Open)?;
    same("the ready row's type", &row.item_type.as_str(), &"task")?;
    same(
        "the ready row's own labels",
        &row.labels,
        &vec![String::from(LABEL)],
    )?;
    same("the ready row's order", &row.order, &OrderState::None)?;
    same(
        "the ready row's assignee, of an item nobody has assigned",
        &row.assignee,
        &None,
    )?;
    same("the ready row's run record", &row.run, &None)?;

    let record = a_record("h1", "greet")?;
    recorded(ctx, &id, &record)?;
    let row = row_in(ctx, &Filter::Ready, &id)?
        .ok_or_else(|| format!("{id} is not in the ready set once a run's record is on it"))?;
    same(
        "the ready row's run record once run.set wrote one",
        &row.run,
        &Some(record),
    )?;

    let hold = answered(
        &format!("hold.raise on {id}"),
        ctx.store
            .hold_raise(&id, "a question the conformance suite asks", &by()),
    )?;
    ensure(row_in(ctx, &Filter::Ready, &id)?.is_none(), || {
        format!("{id} is still in the ready set with hold {hold} raised on it")
    })?;
    answered(
        &format!("hold.clear {hold}"),
        ctx.store.hold_clear(&hold, &by()),
    )?;
    ensure(row_in(ctx, &Filter::Ready, &id)?.is_some(), || {
        format!("{id} is not back in the ready set once its hold {hold} is cleared")
    })?;
    closed(ctx, &id, "closed by the conformance suite", &by())?;
    ensure(row_in(ctx, &Filter::Ready, &id)?.is_none(), || {
        format!("{id} is still in the ready set once it is closed")
    })?;
    Ok(Passed::Pass)
}

/// A label nothing else carries lists the one item filed under it, and a close
/// takes it off: a label's listing is of open items.
fn label_filter(ctx: &Ctx) -> Answer {
    let label = format!("fleet-conformance-{}", SeatId::mint().short());
    let id = filed(ctx, "an item under a label of its own", &[&label])?;
    let rows = listed(ctx, &Filter::Label(label.clone()))?;
    same(
        &format!("the items list Label({label}) answers"),
        &ids(&rows),
        &vec![id.to_string()],
    )?;
    closed(ctx, &id, "closed by the conformance suite", &by())?;
    let rows = listed(ctx, &Filter::Label(label.clone()))?;
    ensure(!rows.iter().any(|row| row.id == id), || {
        format!("{id} is still in list Label({label}) once it is closed")
    })?;
    Ok(Passed::Pass)
}

/// An item assigned to a seat is in that seat's listing, its row naming the
/// seat, and still is once it is closed, reading closed: a seat's listing is
/// of every item it holds, whatever its status.
///
/// The close is by the seat itself, because a store may close an assigned
/// item only for its assignee — the built-in store at its pinned release
/// does, and its adapter matches the seat actor to the assignee it wrote.
fn assignee_filter(ctx: &Ctx) -> Answer {
    let seat = SeatId::mint();
    let id = filed(ctx, "an item a seat holds", &[])?;
    let rows = listed(ctx, &Filter::Assignee(seat))?;
    ensure(rows.is_empty(), || {
        format!("a seat nothing was assigned to lists {:?}", ids(&rows))
    })?;
    assign(ctx, &id, seat)?;
    let rows = listed(ctx, &Filter::Assignee(seat))?;
    same(
        &format!("the items list Assignee({seat}) answers"),
        &ids(&rows),
        &vec![id.to_string()],
    )?;
    same(
        &format!("the assignee on {id}'s row in its seat's listing"),
        &rows[0].assignee,
        &Some(seat),
    )?;
    closed(ctx, &id, "closed by its holder", &Actor::seat(seat))?;
    let row = row_in(ctx, &Filter::Assignee(seat), &id)?
        .ok_or_else(|| format!("{id} is not in list Assignee({seat}) once it is closed"))?;
    same(
        "the closed item's status in its seat's listing",
        &row.status,
        &Status::Closed,
    )?;
    Ok(Passed::Pass)
}

/// The title alone, the assignee alone and both in one update each move the
/// field they name and leave the other; an assignee handed to nobody reads
/// ABSENT, as an item nobody ever held does, and never as an empty holder.
fn update(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "the title it was filed under", &[])?;
    answered(
        &format!("update {id} title"),
        ctx.store.update(
            &id,
            &Update::title(String::from("the title it carries now")),
            &by(),
        ),
    )?;
    let now = read(ctx, &id)?;
    same(
        "the title moved",
        &now.title.as_str(),
        &"the title it carries now",
    )?;
    same("a title alone leaves the assignee", &now.assignee, &None)?;

    let seat = SeatId::mint();
    assign(ctx, &id, seat)?;
    let now = read(ctx, &id)?;
    same("the assignee set", &now.assignee, &Some(seat))?;
    same(
        "an assignee alone leaves the title",
        &now.title.as_str(),
        &"the title it carries now",
    )?;

    let another = SeatId::mint();
    answered(
        &format!("update {id} title and assignee"),
        ctx.store.update(
            &id,
            &Update {
                title: Some(String::from("the title and the holder both moved")),
                assignee: Some(Some(another)),
                ..Update::default()
            },
            &by(),
        ),
    )?;
    let now = read(ctx, &id)?;
    same(
        "the title one update moved",
        &now.title.as_str(),
        &"the title and the holder both moved",
    )?;
    same(
        "the assignee the same update moved",
        &now.assignee,
        &Some(another),
    )?;

    answered(
        &format!("update {id} unassigned"),
        ctx.store.update(&id, &Update::unassigned(), &by()),
    )?;
    let now = read(ctx, &id)?;
    same("an assignee handed to nobody", &now.assignee, &None)?;
    same(
        "and the title stands",
        &now.title.as_str(),
        &"the title and the holder both moved",
    )?;
    Ok(Passed::Pass)
}

/// run.set R, order.set O1, order.set O2, and the item answers O2 and R: an
/// order write replaces the order whole and keeps the run's record — a store
/// nesting both under one object loses the record to the first order written
/// over it, measured on the built-in store at its pinned release (fleet-4j6).
fn order_keeps_the_run(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item whose order is given twice", &[])?;
    let record = a_record("h1", "greet")?;
    let second = another_order(SeatId::mint())?;
    recorded(ctx, &id, &record)?;
    ordered(ctx, &id, &an_order(Some(SeatId::mint()))?)?;
    ordered(ctx, &id, &second)?;
    let now = read(ctx, &id)?;
    same(
        "the order after a second order.set",
        &now.order,
        &OrderState::Ordered(second),
    )
    .map_err(|why| format!("{why} — {}", now.proof.as_str()))?;
    same("and the run's record beside it", &now.run, &Some(record))
        .map_err(|why| format!("{why} — {}", now.proof.as_str()))?;
    Ok(Passed::Pass)
}

/// order.set O, run.set R1, run.set R2, and the item answers O and R2: a run
/// write replaces the record whole and keeps the order.
fn run_keeps_the_order(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item whose run record is written twice", &[])?;
    let order = an_order(Some(SeatId::mint()))?;
    let second = a_record("h2", "another-workflow")?;
    ordered(ctx, &id, &order)?;
    recorded(ctx, &id, &a_record("h1", "greet")?)?;
    recorded(ctx, &id, &second)?;
    let now = read(ctx, &id)?;
    same("the record after a second run.set", &now.run, &Some(second))
        .map_err(|why| format!("{why} — {}", now.proof.as_str()))?;
    same(
        "and the order beside it",
        &now.order,
        &OrderState::Ordered(order),
    )
    .map_err(|why| format!("{why} — {}", now.proof.as_str()))?;
    Ok(Passed::Pass)
}

/// order.withdraw clears the assignee and the order in one act, and touches
/// nothing else: the run's record and the status stand.
fn withdraw_clears_both(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item whose order is withdrawn", &[])?;
    let seat = SeatId::mint();
    let record = a_record("h1", "greet")?;
    assign(ctx, &id, seat)?;
    recorded(ctx, &id, &record)?;
    ordered(ctx, &id, &an_order(Some(seat))?)?;
    answered(
        &format!("order.withdraw {id}"),
        ctx.store
            .order_withdraw(&id, &WithdrawFence::default(), &by()),
    )?;
    let now = read(ctx, &id)?;
    same("the assignee after a withdrawal", &now.assignee, &None)?;
    same(
        "the order after a withdrawal",
        &now.order,
        &OrderState::None,
    )?;
    same(
        "the run's record after a withdrawal",
        &now.run,
        &Some(record),
    )?;
    same("the status after a withdrawal", &now.status, &Status::Open)?;
    Ok(Passed::Pass)
}

/// A filed item's timeline is empty; three appends come back as its last three
/// entries in the order they were appended, with ids of their own, the kinds
/// and bodies sent, the actor who appended them and the store's time; and the
/// timeline of an item nobody filed is Refused.
fn timeline(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item with a timeline", &[])?;
    let before = answered(&format!("timeline {id}"), ctx.store.timeline(&id))?;
    ensure(before.is_empty(), || {
        format!("a filed item's timeline answered {before:?}, and nothing was appended")
    })?;

    let seat = SeatId::mint();
    let author = Actor::seat(seat);
    let bodies = [
        Body::Ordered(Ordered {
            order: OrderKind::Dispatch,
            seat: Some(seat),
        }),
        Body::OrderWithdrawn(OrderWithdrawn {
            why: Withdrawal::Retire,
            seat: Some(seat),
            cause: None,
        }),
        a_delivery(),
    ];
    let mut sent = Vec::new();
    for body in &bodies {
        sent.push(answered(
            &format!("append {} to {id}", body.kind()),
            ctx.store.append(&id, body, &author),
        )?);
    }
    let distinct: BTreeSet<&String> = sent.iter().collect();
    ensure(distinct.len() == sent.len(), || {
        format!("three appends answered {sent:?}, and each entry has an id of its own")
    })?;

    let entries = answered(&format!("timeline {id}"), ctx.store.timeline(&id))?;
    let last = entries
        .len()
        .checked_sub(bodies.len())
        .and_then(|from| entries.get(from..))
        .ok_or_else(|| {
            format!(
                "the timeline answered {} entries after three appends",
                entries.len()
            )
        })?;
    same(
        "the last three entries' ids, in the order they were appended",
        &last
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>(),
        &sent,
    )?;
    same(
        "their kinds",
        &last
            .iter()
            .map(|entry| entry.body.kind())
            .collect::<Vec<_>>(),
        &bodies.iter().map(Body::kind).collect::<Vec<_>>(),
    )?;
    for (entry, body) in last.iter().zip(&bodies) {
        same(
            &format!("the body of entry {}", entry.id),
            &entry.body,
            body,
        )?;
        same(
            &format!("the author of entry {}", entry.id),
            &entry.by,
            &author,
        )?;
        ensure(!entry.at.is_empty(), || {
            format!("entry {} carries no time", entry.id)
        })?;
    }

    let absent = ctx.store.timeline(&ItemId::from(NOBODY)).map(|e| e.len());
    ensure(matches!(absent, Err(StoreError::Refused(_))), || {
        format!(
            "timeline {NOBODY} answered {}, and an item nobody filed is Refused",
            outcome(&absent)
        )
    })?;
    Ok(Passed::Pass)
}

/// A raised hold is open and takes its item out of the ready set; a cleared one
/// is not open; and a second clear is Refused, naming the hold — the act is
/// already done — and leaves it closed.
fn holds(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item to hold", &[])?;
    let hold = answered(
        &format!("hold.raise on {id}"),
        ctx.store
            .hold_raise(&id, "the question this hold asks", &by()),
    )?;
    ensure(!hold.is_empty(), || {
        String::from("hold.raise answered an empty id")
    })?;
    let open = answered("holds.open", ctx.store.holds_open())?;
    ensure(open.contains(&hold), || {
        format!("a raised hold, {hold}, is not in holds.open: {open:?}")
    })?;
    ensure(row_in(ctx, &Filter::Ready, &id)?.is_none(), || {
        format!("{id} is still in the ready set with hold {hold} raised on it")
    })?;

    answered(
        &format!("hold.clear {hold}"),
        ctx.store.hold_clear(&hold, &by()),
    )?;
    let open = answered("holds.open", ctx.store.holds_open())?;
    ensure(!open.contains(&hold), || {
        format!("a cleared hold, {hold}, is still in holds.open")
    })?;

    match ctx.store.hold_clear(&hold, &by()) {
        Err(StoreError::Refused(why)) => ensure(why.contains(hold.as_str()), || {
            format!("a second clear of {hold} was Refused without naming it: {why}")
        })?,
        answer => {
            return Err(format!(
                "a second clear of {hold} answered {}, and a hold already cleared is Refused",
                outcome(&answer)
            ))
        }
    }
    let open = answered("holds.open", ctx.store.holds_open())?;
    ensure(!open.contains(&hold), || {
        format!("{hold} is open again after a second clear")
    })?;
    Ok(Passed::Pass)
}

/// A close moves the status the next read answers; a second close is Refused,
/// naming the item — the act is already done — and the item never carries the
/// second close's reason.
fn close(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item to close", &[])?;
    same(
        "a fresh item's status",
        &read(ctx, &id)?.status,
        &Status::Open,
    )?;
    let closer = by();
    closed(ctx, &id, "the first close's reason", &closer)?;
    same(
        "the status after a close",
        &read(ctx, &id)?.status,
        &Status::Closed,
    )?;
    match ctx.store.close(&id, "the second close's reason", &closer) {
        Err(StoreError::Refused(why)) => ensure(why.contains(id.as_str()), || {
            format!("a second close of {id} was Refused without naming it: {why}")
        })?,
        answer => {
            return Err(format!(
                "a second close of {id} answered {}, and an item already closed is Refused",
                outcome(&answer)
            ))
        }
    }
    let now = read(ctx, &id)?;
    same(
        "the status after a second close",
        &now.status,
        &Status::Closed,
    )?;
    ensure(!now.proof.carries("the second close's reason"), || {
        format!(
            "{id} carries the refused close's reason: {}",
            now.proof.as_str()
        )
    })?;
    Ok(Passed::Pass)
}

/// The export lands at the file the store's capabilities declare, under the
/// root the caller named, and the store answers that path; the file carries
/// the items, and its bytes move when an item's timeline does — which is what
/// a landing's gate reads. A store that declares no export has none to check.
fn export(ctx: &Ctx) -> Answer {
    let declared = answered("capabilities", ctx.store.capabilities())?;
    let Some(spec) = declared.export else {
        return Ok(Passed::Skip(String::from("the store declares no export")));
    };
    let id = filed(ctx, "an item the export carries", &[])?;
    let into = ctx.root.join(&spec.file);
    let call = format!("export under {}", ctx.root.display());
    let wrote = answered(&call, ctx.store.export(ctx.root))?;
    same(
        "the path the export answers, the declared file under the root",
        &wrote,
        &into,
    )?;
    let before = std::fs::read(&into)
        .map_err(|e| format!("the export left no file at {}: {e}", into.display()))?;
    ensure(!before.is_empty(), || {
        format!("the export at {} is empty", into.display())
    })?;
    ensure(
        String::from_utf8_lossy(&before).contains(id.as_str()),
        || format!("the export at {} does not carry {id}", into.display()),
    )?;

    answered(
        &format!("append ordered to {id}"),
        ctx.store.append(
            &id,
            &Body::Ordered(Ordered {
                order: OrderKind::Dispatch,
                seat: None,
            }),
            &Actor::seat(SeatId::mint()),
        ),
    )?;
    let again = answered(&call, ctx.store.export(ctx.root))?;
    same("the path a second export answers", &again, &into)?;
    let after = std::fs::read(&into)
        .map_err(|e| format!("the second export left no file at {}: {e}", into.display()))?;
    ensure(before != after, || {
        format!(
            "the export's bytes at {} did not move when {id}'s timeline did",
            into.display()
        )
    })?;
    Ok(Passed::Pass)
}

// ---- the checks the suite had grown beside them ----------------------------

/// An item nothing has ordered reads no order; order.set is the order the
/// next read answers and touches no assignee; and an order naming no seat —
/// a transient dispatch not yet answered — reads back naming none, the seat
/// left out and not written empty.
fn order_reads_back(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item with an order on it", &[])?;
    same(
        "the order of an item nothing has ordered",
        &read(ctx, &id)?.order,
        &OrderState::None,
    )?;
    let order = an_order(Some(SeatId::mint()))?;
    ordered(ctx, &id, &order)?;
    let now = read(ctx, &id)?;
    same("the order written", &now.order, &OrderState::Ordered(order))?;
    same("an order's assignee", &now.assignee, &None)?;

    let transient = an_order(None)?;
    ordered(ctx, &id, &transient)?;
    let now = read(ctx, &id)?;
    same(
        "an order naming no seat",
        &now.order,
        &OrderState::Ordered(transient),
    )
    .map_err(|why| format!("{why} — {}", now.proof.as_str()))?;
    Ok(Passed::Pass)
}

/// An update naming neither a title nor an assignee is a caller's mistake —
/// Unreadable — and nothing moves.
fn update_naming_nothing(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item an empty update reaches", &[])?;
    let seat = SeatId::mint();
    assign(ctx, &id, seat)?;
    let answer = ctx.store.update(&id, &Update::default(), &by());
    ensure(matches!(answer, Err(StoreError::Unreadable(_))), || {
        format!(
            "an update of {id} naming nothing answered {}, and it is Unreadable",
            outcome(&answer)
        )
    })?;
    let now = read(ctx, &id)?;
    same(
        "the title after an update naming nothing",
        &now.title.as_str(),
        &"an item an empty update reaches",
    )?;
    same(
        "the assignee after an update naming nothing",
        &now.assignee,
        &Some(seat),
    )?;
    Ok(Passed::Pass)
}

/// A fenced write lands only while the holder it names still holds the item,
/// and is Moved with NOTHING written where somebody else does. A withdrawal is
/// fenced on the status it names too, so a closed item is never reopened by
/// one, and the one that lands leaves the item open, unheld and unordered. A
/// retire's withdrawal and a return's hand-over are these writes, each one
/// call.
fn fenced_writes(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item a fenced write reaches", &[])?;
    let seat = SeatId::mint();
    assign(ctx, &id, seat)?;
    ordered(ctx, &id, &an_order(Some(seat))?)?;
    let untouched = |status: Status, after: &str| -> Result<(), String> {
        let now = read(ctx, &id)?;
        same(
            &format!("{after}: the holder, as nothing was written"),
            &now.assignee,
            &Some(seat),
        )?;
        ensure(now.order != OrderState::None, || {
            format!("{after}: the order is gone, and nothing was to be written")
        })?;
        same(&format!("{after}: the status"), &now.status, &status)
    };

    // A retire's withdrawal: fenced on the seat it retires and the status it
    // listed, reopening the item.
    let retired = |holder: SeatId, status: Status| WithdrawFence {
        if_assignee: Some(Some(holder)),
        if_status: Some(status),
        reopen: true,
    };
    let another = SeatId::mint();
    let other = another.to_string();
    moved(
        "a withdrawal naming a seat that does not hold the item",
        ctx.store
            .order_withdraw(&id, &retired(another, Status::Open), &by()),
        &[id.as_str(), &other],
    )?;
    untouched(Status::Open, "a withdrawal naming another seat")?;

    moved(
        "a withdrawal naming a status the item is not in",
        ctx.store
            .order_withdraw(&id, &retired(seat, Status::InProgress), &by()),
        &[id.as_str(), "in_progress"],
    )?;
    untouched(Status::Open, "a withdrawal naming another status")?;

    closed(ctx, &id, "landed by its seat", &Actor::seat(seat))?;
    moved(
        "a withdrawal of an item closed since it was read",
        ctx.store
            .order_withdraw(&id, &retired(seat, Status::Open), &by()),
        &[id.as_str(), "closed"],
    )?;
    untouched(Status::Closed, "a withdrawal of a closed item")?;

    let reopened = Update {
        status: Some(Status::Open),
        ..Update::default()
    };
    answered(
        &format!("a reopen of {id}"),
        ctx.store.update(&id, &reopened, &by()),
    )?;
    untouched(Status::Open, "a reopen")?;

    answered(
        &format!("the holder's withdrawal of {id}"),
        ctx.store
            .order_withdraw(&id, &retired(seat, Status::Open), &by()),
    )?;
    let now = read(ctx, &id)?;
    same("the assignee after the withdrawal", &now.assignee, &None)?;
    same(
        "the order after the withdrawal",
        &now.order,
        &OrderState::None,
    )?;
    same(
        "the status after the withdrawal",
        &now.status,
        &Status::Open,
    )?;

    // A return's hand-over: the assignee moved, fenced on the holder the
    // caller read.
    let handed = |from: Option<SeatId>, to: SeatId| Update {
        assignee: Some(Some(to)),
        ..Update::fenced(from)
    };
    let builder = SeatId::mint();
    let reviewer = SeatId::mint();
    moved(
        "a hand-over from a seat that no longer holds the item",
        ctx.store.update(&id, &handed(Some(seat), builder), &by()),
        &[id.as_str()],
    )?;
    same(
        "the holder after a hand-over that was Moved",
        &read(ctx, &id)?.assignee,
        &None,
    )?;
    answered(
        &format!("a hand-over of {id}, which nobody holds, from nobody"),
        ctx.store.update(&id, &handed(None, builder), &by()),
    )?;
    answered(
        &format!("a hand-over of {id} from its holder"),
        ctx.store
            .update(&id, &handed(Some(builder), reviewer), &by()),
    )?;
    same(
        "the holder after two hand-overs",
        &read(ctx, &id)?.assignee,
        &Some(reviewer),
    )?;
    Ok(Passed::Pass)
}

/// An update fenced on a seat lands only while that seat holds the item, and
/// one fenced on nobody only while nobody does — an item never held, or one
/// whose holder was cleared. A fence the item does not meet is Moved, naming
/// the item and the seat that holds it, with NOTHING written.
fn fenced_update(ctx: &Ctx) -> Answer {
    let title = "an item a fenced update reaches";
    let id = filed(ctx, title, &[])?;
    let seat = SeatId::mint();
    let holder = seat.to_string();
    answered(
        &format!("update {id} assignee {seat}, fenced on nobody"),
        ctx.store.update(
            &id,
            &Update {
                assignee: Some(Some(seat)),
                ..Update::fenced(None)
            },
            &by(),
        ),
    )?;
    same(
        "the holder an update fenced on nobody wrote on an item never held",
        &read(ctx, &id)?.assignee,
        &Some(seat),
    )?;

    let another = SeatId::mint();
    for (fence, call) in [
        (Some(another), "fenced on a seat that does not hold it"),
        (None, "fenced on nobody"),
    ] {
        moved(
            &format!("an update of {id} {call}"),
            ctx.store.update(
                &id,
                &Update {
                    title: Some(String::from("a title nothing writes")),
                    assignee: Some(Some(another)),
                    ..Update::fenced(fence)
                },
                &by(),
            ),
            &[id.as_str(), &holder],
        )?;
        let now = read(ctx, &id)?;
        same(
            &format!("{call}: the holder, as nothing was written"),
            &now.assignee,
            &Some(seat),
        )?;
        same(
            &format!("{call}: the title, as nothing was written"),
            &now.title.as_str(),
            &title,
        )?;
    }

    answered(
        &format!("update {id} title, fenced on its holder"),
        ctx.store.update(
            &id,
            &Update {
                title: Some(String::from("the title its holder's fence let through")),
                ..Update::fenced(Some(seat))
            },
            &by(),
        ),
    )?;
    let now = read(ctx, &id)?;
    same(
        "the title an update fenced on its holder moved",
        &now.title.as_str(),
        &"the title its holder's fence let through",
    )?;
    same("and the holder it left", &now.assignee, &Some(seat))?;

    answered(
        &format!("update {id} unassigned, fenced on its holder"),
        ctx.store.update(
            &id,
            &Update {
                assignee: Some(None),
                ..Update::fenced(Some(seat))
            },
            &by(),
        ),
    )?;
    same(
        "the holder an update fenced on its holder cleared",
        &read(ctx, &id)?.assignee,
        &None,
    )?;
    answered(
        &format!("update {id} assignee {another}, fenced on nobody"),
        ctx.store.update(
            &id,
            &Update {
                assignee: Some(Some(another)),
                ..Update::fenced(None)
            },
            &by(),
        ),
    )?;
    same(
        "the holder an update fenced on nobody wrote on an item whose holder was cleared",
        &read(ctx, &id)?.assignee,
        &Some(another),
    )?;
    Ok(Passed::Pass)
}

/// An update's status is `open` or nothing. Fenced on its holder, it reopens
/// an item that holder closed and hands it to nobody in the same write, which
/// brings it back to the ready set; fenced on nobody while a seat holds it, it
/// is Moved with nothing written; and a status other than `open` is Usage,
/// with nothing written.
fn update_to_open(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item an update reopens", &[])?;
    let seat = SeatId::mint();
    let holder = seat.to_string();
    assign(ctx, &id, seat)?;
    closed(ctx, &id, "closed by its holder", &Actor::seat(seat))?;
    let reopened = |fence: Option<SeatId>| Update {
        assignee: Some(None),
        status: Some(Status::Open),
        ..Update::fenced(fence)
    };
    let untouched = |after: &str| -> Result<(), String> {
        let now = read(ctx, &id)?;
        same(
            &format!("{after}: the holder, as nothing was written"),
            &now.assignee,
            &Some(seat),
        )?;
        same(
            &format!("{after}: the status, as nothing was written"),
            &now.status,
            &Status::Closed,
        )
    };

    moved(
        &format!("a reopen of {id} fenced on nobody"),
        ctx.store.update(&id, &reopened(None), &by()),
        &[id.as_str(), &holder],
    )?;
    untouched("a reopen fenced on nobody")?;

    let usage = ctx.store.update(
        &id,
        &Update {
            status: Some(Status::InProgress),
            ..Update::fenced(Some(seat))
        },
        &by(),
    );
    ensure(matches!(usage, Err(StoreError::Usage(_))), || {
        format!(
            "an update of {id} to in_progress answered {}, and a status other than open is \
             Usage",
            outcome(&usage)
        )
    })?;
    untouched("an update to in_progress")?;

    answered(
        &format!("a reopen of {id} fenced on its holder"),
        ctx.store.update(&id, &reopened(Some(seat)), &by()),
    )?;
    let now = read(ctx, &id)?;
    same("the status a reopen wrote", &now.status, &Status::Open)?;
    same("the holder the same write cleared", &now.assignee, &None)?;
    ensure(row_in(ctx, &Filter::Ready, &id)?.is_some(), || {
        format!("{id} is not back in the ready set once it is reopened and held by nobody")
    })?;
    Ok(Passed::Pass)
}

/// A withdrawal fenced on the item's holder and the status it reads, and
/// reopening it, clears the assignee, takes the order away and sets the
/// status to `open` in ONE act, the run's record standing. One whose fence
/// the item does not meet — another holder, nobody, another status — is Moved
/// with nothing written, naming what the item is held by or reads.
fn fenced_withdraw(ctx: &Ctx) -> Answer {
    let id = filed(ctx, "an item a fenced withdrawal reopens", &[])?;
    let seat = SeatId::mint();
    let holder = seat.to_string();
    let record = a_record("h1", "greet")?;
    assign(ctx, &id, seat)?;
    recorded(ctx, &id, &record)?;
    ordered(ctx, &id, &an_order(Some(seat))?)?;
    closed(ctx, &id, "closed by its holder", &Actor::seat(seat))?;
    let fence = |held: Option<SeatId>, status: Status| WithdrawFence {
        if_assignee: Some(held),
        if_status: Some(status),
        reopen: true,
    };

    let another = SeatId::mint();
    for (missed, call, named) in [
        (
            fence(Some(another), Status::Closed),
            "fenced on a seat that does not hold it",
            holder.as_str(),
        ),
        (
            fence(None, Status::Closed),
            "fenced on nobody",
            holder.as_str(),
        ),
        (
            fence(Some(seat), Status::Open),
            "fenced on a status it does not read",
            Status::Closed.as_str(),
        ),
    ] {
        moved(
            &format!("a withdrawal of {id} {call}"),
            ctx.store.order_withdraw(&id, &missed, &by()),
            &[id.as_str(), named],
        )?;
        let now = read(ctx, &id)?;
        same(
            &format!("{call}: the holder, as nothing was written"),
            &now.assignee,
            &Some(seat),
        )?;
        ensure(now.order != OrderState::None, || {
            format!("{call}: the order is gone, and nothing was to be written")
        })?;
        same(
            &format!("{call}: the status, as nothing was written"),
            &now.status,
            &Status::Closed,
        )?;
    }

    answered(
        &format!("the withdrawal of {id} fenced on its holder and its status, reopening it"),
        ctx.store
            .order_withdraw(&id, &fence(Some(seat), Status::Closed), &by()),
    )?;
    let now = read(ctx, &id)?;
    same("the assignee after the withdrawal", &now.assignee, &None)?;
    same(
        "the order after the withdrawal",
        &now.order,
        &OrderState::None,
    )?;
    same(
        "the status the withdrawal reopened",
        &now.status,
        &Status::Open,
    )?;
    same(
        "the run's record after the withdrawal",
        &now.run,
        &Some(record),
    )?;
    ensure(row_in(ctx, &Filter::Ready, &id)?.is_some(), || {
        format!("{id} is not back in the ready set once its withdrawal reopened it")
    })?;
    Ok(Passed::Pass)
}

/// Fleet's writes merge BESIDE another writer's keys (fleet-4j6): a bare
/// `orders` and a key of its own that another tool keeps on the item are
/// neither read nor moved by an order, a run's record or a withdrawal, and
/// the reads of fleet's own two keys answer through them.
fn another_writers_keys(ctx: &Ctx) -> Answer {
    let Some(plant) = ctx.another_writer else {
        return Ok(Passed::Skip(String::from(
            "no other writer was handed to this run, so nothing plants another tool's keys",
        )));
    };
    let id = filed(ctx, "an item carrying another writer's keys", &[])?;
    plant(
        &id,
        r#"{"orders":{"seat":"another-tools-seat","by":7},"a_prior_key":{"kept":true}}"#,
    )
    .map_err(|why| format!("another writer's metadata on {id}: {why}"))?;
    let theirs = serde_json::json!([
        { "seat": "another-tools-seat", "by": 7 },
        { "kept": true },
    ]);
    let unmoved = |item: &Item, after: &str| -> Result<(), String> {
        let document = super::first_value(item.proof.as_str())
            .ok_or_else(|| format!("the read of {id} is not JSON: {}", item.proof.as_str()))?;
        let document = match document {
            serde_json::Value::Array(rows) => rows.into_iter().next().unwrap_or_default(),
            row => row,
        };
        let held = serde_json::json!([
            document["metadata"]["orders"],
            document["metadata"]["a_prior_key"],
        ]);
        ensure(held == theirs, || {
            format!(
                "after {after}, the other writer's keys moved: {}",
                item.proof.as_str()
            )
        })
    };
    let now = read(ctx, &id)?;
    same(
        "a bare `orders`, which is not fleet's",
        &now.order,
        &OrderState::None,
    )?;
    same("the run's record beside it", &now.run, &None)?;

    let record = a_record("h1", "greet")?;
    recorded(ctx, &id, &record)?;
    let now = read(ctx, &id)?;
    same("the run's record written", &now.run, &Some(record.clone()))?;
    unmoved(&now, "a run's record")?;

    let seat = SeatId::mint();
    let order = an_order(Some(seat))?;
    ordered(ctx, &id, &order)?;
    let now = read(ctx, &id)?;
    same("the order written", &now.order, &OrderState::Ordered(order))?;
    unmoved(&now, "an order")?;

    assign(ctx, &id, seat)?;
    answered(
        &format!("order.withdraw {id}"),
        ctx.store
            .order_withdraw(&id, &WithdrawFence::default(), &by()),
    )?;
    let now = read(ctx, &id)?;
    same(
        "the order after a withdrawal",
        &now.order,
        &OrderState::None,
    )?;
    same(
        "the run's record after a withdrawal",
        &now.run,
        &Some(record),
    )?;
    unmoved(&now, "a withdrawal")?;
    Ok(Passed::Pass)
}

/// The keys another writer keeps on an item are named as `foreign`, by the
/// read and by the listing's row alike, and fleet's own are never among them:
/// with an order and a run's record written, an item carrying nobody else's
/// key names none, and once another writer's two are planted beside fleet's it
/// names exactly those two.
///
/// Compared as a set: the contract fixes which keys and not their order.
fn another_writers_keys_are_foreign(ctx: &Ctx) -> Answer {
    let Some(plant) = ctx.another_writer else {
        return Ok(Passed::Skip(String::from(
            "no other writer was handed to this run, so nothing plants another tool's keys",
        )));
    };
    let id = filed(ctx, "an item whose other keys are named", &[])?;
    ordered(ctx, &id, &an_order(Some(SeatId::mint()))?)?;
    recorded(ctx, &id, &a_record("h1", "greet")?)?;
    let named = |foreign: &[String]| -> BTreeSet<String> { foreign.iter().cloned().collect() };
    same(
        "the keys named foreign where fleet's alone are written",
        &named(&read(ctx, &id)?.foreign),
        &BTreeSet::new(),
    )?;

    plant(
        &id,
        r#"{"orders":{"seat":"another-tools-seat"},"a_prior_key":7}"#,
    )
    .map_err(|why| format!("another writer's metadata on {id}: {why}"))?;
    let theirs: BTreeSet<String> = ["a_prior_key", "orders"]
        .into_iter()
        .map(String::from)
        .collect();
    same(
        "the keys the read names foreign",
        &named(&read(ctx, &id)?.foreign),
        &theirs,
    )?;
    let row = row_in(ctx, &Filter::Label(String::from(LABEL)), &id)?.ok_or_else(|| {
        format!("{id} is open under {LABEL}, and its label's listing left it out")
    })?;
    same(
        "the keys the listing's row names foreign",
        &named(&row.foreign),
        &theirs,
    )?;
    Ok(Passed::Pass)
}
