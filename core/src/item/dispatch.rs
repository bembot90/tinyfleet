//! `fleet dispatch <item> [--to <seat>]` — the one record that says a seat may
//! begin.
//!
//! Three writes make an order: the assignee, the note and the index. The
//! failure a multi-writer shape produces is an order written to one surface and
//! not the other, which reads as clean on both — so the three are one act here,
//! and the act ends by reading them back.
//!
//! WRITE ORDER is the record before the index. A crash between them leaves an
//! item ordered on the record and merely unindexed, never indexed with no order
//! behind it.
//!
//! THE READ-BACK COMPARES AGAINST THE ARGUMENTS, never against the payload:
//! an expectation taken from the object we just built agrees with whatever that
//! object happens to hold, so a writer that stopped setting a field would
//! satisfy a check that had stopped asking for it in the same edit.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::item::brief::{self, Packs, Subject, ORDER_MARK, TRANSIENT};
use crate::item::deliver::holds;
use crate::item::{
    control_token, render, Events, Project, Ring, RingOutcome, Spawn, SpawnOutcome, Spawner, Stop,
    ITEM_DISPATCHED, NO_SESSION, REFUSED,
};
use crate::seat::identity::{Directory, Kind, SeatId, SeatRef};
use crate::store::{keys, Item, Orders, Store, StoreError};

/// The kind of order this verb writes. The reference's other two grammars —
/// a run's feed, a spawn's own line — are that repository's; here there is one
/// form for giving work.
pub const KIND: &str = "dispatch";

/// What the ring carries. It says where to look and never what to do: the
/// record is the item, and a message that summarised it would be a second copy
/// of the order that could disagree with the first.
pub const RING: &str = "{item} is yours. The order is on the item; your brief is at {path}. \
     Read the item and act on the record.";

/// The store's spelling of the one type no order is given for: an epic's
/// children are the work, and each of them is dispatched on its own.
pub const EPIC: &str = "epic";

/// The line a withdrawal leaves, so an item a spawn refused reads as one
/// nobody was ever given rather than as one whose seat vanished.
pub const WITHDRAWN: &str = "DISPATCH WITHDRAWN — spawn refused";

/// The line a spawn nobody could observe leaves. It is NOT a withdrawal: the
/// order is still on the record under it, and the cause is what a retry acts on.
pub const NOT_TOLD: &str = "DISPATCH COULD NOT TELL — the spawn could not be observed";

/// The order, as its arguments.
pub struct Order<'a> {
    pub item: &'a str,
    pub to: Option<&'a str>,
    pub by: &'a str,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
    /// A brief already written, handed to the spawn as the first turn instead
    /// of one rendered here.
    ///
    /// A caller that pinned a brief needs the seat to read that file and not a
    /// re-render of an item that has moved since. The order itself is
    /// unchanged: the note and the index are written here either way, and the
    /// pinned brief says so rather than standing in for one.
    pub brief: Option<&'a Path>,
    /// The commit the spawned seat's worktree is cut from, where the caller
    /// names one. It rides BESIDE the pinned brief because both reach the
    /// spawn in one act.
    pub base: Option<&'a str>,
    /// The model the spawned seat runs on, where the caller names one; `None`
    /// leaves the fleet's policy default.
    pub model: Option<&'a str>,
    /// The builder's checks — the command the seat runs over its own diff before
    /// it delivers — as the caller hands it. It reaches the brief's `{touched}`
    /// and the spawned seat's permission rules; `None` renders the brief's
    /// named absence ([`brief::DERIVE_TOUCHED`]).
    pub touched: Option<&'a str>,
}

/// A dispatch that stopped, with the one outcome a caller has to tell from the
/// others carried as a value.
///
/// A spawn the belt refused is a HOLD — the order withdrawn, the item retried
/// on the next call — and every other stop is not. A caller that read the two
/// apart by matching the refusal's prose would go quiet the day the sentence
/// changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub stop: Stop,
    /// What the spawner said, where a spawn refused and the order was withdrawn.
    pub withdrawn: Option<String>,
}

impl Refused {
    fn stopped(stop: Stop) -> Refused {
        Refused {
            stop,
            withdrawn: None,
        }
    }
}

impl From<Refused> for Stop {
    fn from(refused: Refused) -> Stop {
        refused.stop
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.stop.message)
    }
}

/// Everything the verb acts through.
pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub project: &'a Project,
    pub packs: &'a Packs,
    pub briefs_dir: &'a Path,
    /// The seats this fleet knows and this machine runs. A `--to` that resolves
    /// to no running seat — or to more than one — is a seat this fleet does not
    /// run.
    pub seats: &'a Directory,
    pub ring: &'a dyn Ring,
    pub spawner: &'a dyn Spawner,
    pub events: &'a dyn Events,
}

/// The order given, for a caller that wants to say what happened.
pub struct Given {
    /// The id the order was given under: the store's full one, whatever part
    /// of it the caller typed.
    pub item: String,
    pub note: String,
    pub brief_path: PathBuf,
    /// The seat the order was given to: its full id, as the assignee and the
    /// index carry it, with its name and its kind.
    pub seat: Option<SeatRef>,
    /// Both load-belt readings the spawn was let through on, as the spawner
    /// rendered them. `None` on the named path, which starts nothing and so
    /// runs no belt.
    pub belt: Option<String>,
}

pub fn dispatch(
    out: &mut dyn Write,
    err: &mut dyn Write,
    order: &Order,
    wiring: &Wiring,
) -> Result<Given, Refused> {
    let note = note_text(wiring.packs, order.by).map_err(Refused::stopped)?;

    // Before the first write: the brief refuses on the same reading, and an
    // order written ahead of a brief that cannot render is one nobody reads.
    wiring.project.refuse_moved().map_err(Refused::stopped)?;

    // THE ITEM IS RESOLVED ONCE, HERE, and the order names the id the store
    // answered from this line on. The store resolves a partial id itself, so
    // the ready list compared against the typed text refused a ready item,
    // and every write under it would be a second spelling of the item.
    let item = read(wiring.store, order.item).map_err(Refused::stopped)?;
    let order = &Order {
        item: &item.id,
        ..*order
    };
    let named = refuse_unless_dispatchable(order, &item, wiring).map_err(Refused::stopped)?;

    let given = match named {
        Some(seat) => to_named_seat(err, order, wiring, &note, seat).map_err(Refused::stopped)?,
        None => to_a_transient_seat(err, order, wiring, &note)?,
    };

    // THE ORDER LINE FIRST AND THE BELT UNDER IT, on the same stream: a person
    // reading a dispatch's own output reads what the machine was measured at
    // beside the order it gave, in the words `fleet seat spawn` prints. A
    // dispatch to a named seat starts nothing and prints nothing here.
    let said = match &given.belt {
        Some(belt) => format!("{}\n{belt}\n", given.note),
        None => format!("{}\n", given.note),
    };
    out.write_all(said.as_bytes()).map_err(|e| {
        Refused::stopped(Stop::could_not_tell(format!(
            "the note line could not be written: {e}"
        )))
    })?;
    Ok(given)
}

/// The five refusals, in order, each of them before any write. `item` is the
/// record the order's id was resolved from. The answer is the running seat a
/// `--to` named, or `None` for a transient dispatch.
fn refuse_unless_dispatchable<'w>(
    order: &Order,
    item: &Item,
    wiring: &Wiring<'w>,
) -> Result<Option<&'w SeatRef>, Stop> {
    let ready = wiring.store.ready()?;

    if !ready.iter().any(|id| id == order.item) {
        return Err(Stop::refused(format!(
            "{} is not ready — {}",
            order.item,
            why_not_ready(item)
        )));
    }
    refuse_an_epic(item)?;
    if item.has_orders_key {
        // A key this binary cannot read is not an order it can weigh: it may
        // be one, at a version a newer fleet wrote, so it is could-not-tell and
        // never taken for absent or overwritten.
        let Some(index) = item.orders.as_ref() else {
            return Err(Stop::could_not_tell(format!(
                "{} carries a `{}` this fleet cannot read — it is not an object at {} {} — and \
                 a dispatch will not guess whether it is an order",
                order.item,
                keys::ORDERS,
                keys::VERSION_FIELD,
                keys::VERSION
            )));
        };
        return Err(Stop::refused(format!(
            "{} already carries an order — {}",
            order.item,
            standing(index)
        )));
    }

    let Some(to) = order.to else {
        return Ok(None);
    };
    // THE MEMBERSHIP IS THE RESOLVER'S, over the seats this machine RUNS: a
    // `--to` is any seat argument — the name, the machine name, eight hex
    // digits of the id or all of it — and a person, who is listed and runs
    // nowhere, is no seat work is given to. What is written onto the item
    // below is the id it resolved to, never the argument as typed.
    let seat = wiring.seats.resolve_running(to).map_err(Stop::from)?;
    // What the seat HOLDS, in deliver's own reading: an item merely assigned
    // to it — an epic, a bug nobody ordered — is not work it was given.
    let held: Vec<String> = holds(wiring.store, &seat.id.to_string())?
        .held
        .into_iter()
        .map(|row| format!("{} ({})", row.id, row.status))
        .collect();
    if !held.is_empty() {
        return Err(Stop::refused(format!(
            "`{}` already holds {} — one item at a time",
            seat.machine_name(),
            held.join(", ")
        )));
    }
    Ok(Some(seat))
}

/// An epic, refused by its TYPE. The store's ready list keeps an open,
/// unblocked epic — it is the store's answer and not a list of what a seat can
/// build — so the ready check alone lets one through. `hold` refuses on the same
/// reading, because a park on an epic is a hold the store will not tie to it.
pub(crate) fn refuse_an_epic(item: &Item) -> Result<(), Stop> {
    if item.item_type == EPIC {
        return Err(Stop::refused(format!(
            "{} is an epic, and an epic is never dispatched — its children are",
            item.id
        )));
    }
    Ok(())
}

/// The named path: assign, note, index, read back, brief, ring.
///
/// Every write, the event and the ring carry the seat's full id; the brief and
/// every sentence name it by its machine name.
fn to_named_seat(
    err: &mut dyn Write,
    order: &Order,
    wiring: &Wiring,
    note: &str,
    named: &SeatRef,
) -> Result<Given, Stop> {
    let id = named.id.to_string();
    let seat = id.as_str();
    let label = wiring.seats.label(&named.id);
    wiring
        .store
        .assign(order.item, seat, order.by)
        .map_err(|e| wrote_nothing(order.item, "the assignee", &e))?;
    write_order(order, wiring, note, Some(seat), true)?;
    read_back(order, wiring, note, Some(seat), Some(seat))?;
    // A NAMED SEAT WRITES NO BASE: no worktree was cut for this order, so there
    // is no commit the seat started from that this verb could read.
    announce(order, wiring, named, None)?;

    let brief_path = match order.brief {
        Some(pinned) => pinned.to_path_buf(),
        None => write_brief(wiring, order, note, &named.machine_name())?,
    };
    let text = render(
        RING,
        &[
            ("item", order.item),
            ("path", &brief_path.display().to_string()),
        ],
    )
    .map_err(|name| Stop::could_not_tell(format!("the ring names `{{{name}}}`")))?;

    // THE RING IS ADDRESSED BY THE ID, which the ring resolves exactly: a name
    // could have moved to another seat between the resolve above and here.
    match wiring.ring.ring(seat, &text) {
        RingOutcome::Delivered => Ok(Given {
            item: order.item.to_string(),
            note: note.to_string(),
            brief_path,
            seat: Some(named.clone()),
            belt: None,
        }),
        RingOutcome::Absent => {
            let _ = writeln!(err, "brief: {}", brief_path.display());
            Err(Stop {
                code: NO_SESSION,
                message: format!(
                    "ORDERED, NOT RUNG: no live session for {label}; the order stands and the \
                     seat's successor reads it at wake"
                ),
            })
        }
        RingOutcome::Failed(cause) => {
            let _ = writeln!(err, "brief: {}", brief_path.display());
            Err(Stop {
                code: REFUSED,
                message: format!("ORDERED, NOT RUNG: {cause}"),
            })
        }
    }
}

/// The transient path: the order first, then the seat that will hold it.
///
/// The note precedes the spawn because the brief IS the first turn and a seat
/// started before the order exists would read an item nobody had given it. A
/// spawn that refuses therefore withdraws the order in the same act, so a
/// refusal never leaves an ordered item nobody holds.
///
/// A SPAWN THAT COULD NOT BE OBSERVED IS NOT A REFUSAL and withdraws nothing:
/// the order stands with one note naming the cause, so the retry that could
/// still succeed has an order to succeed under.
fn to_a_transient_seat(
    err: &mut dyn Write,
    order: &Order,
    wiring: &Wiring,
    note: &str,
) -> Result<Given, Refused> {
    write_order(order, wiring, note, None, true).map_err(Refused::stopped)?;
    read_back(order, wiring, note, None, None).map_err(Refused::stopped)?;

    let pinned = order.brief.map(Path::to_path_buf);
    let brief_path = match &pinned {
        Some(path) => path.clone(),
        None => write_brief(wiring, order, note, TRANSIENT).map_err(Refused::stopped)?,
    };

    match wiring.spawner.spawn(&Spawn {
        first_turn: &brief_path,
        item: order.item,
        base: order.base,
        model: order.model,
        touched: order.touched,
    }) {
        // `seat` is the spawned seat's full id, which is what the assignee and
        // the index carry: a transient seat has no name to be found by.
        SpawnOutcome::Spawned { seat, base, belt } => {
            // THE SEAT THE SPAWN MADE: an agent, and nameless. An answer that
            // is no seat id is one no record can key on, so it is a question
            // asked before the first write that would carry it.
            let spawned = SeatId::parse(&seat)
                .map(|id| SeatRef {
                    id,
                    name: None,
                    kind: Kind::Agent,
                })
                .map_err(|e| {
                    Refused::stopped(Stop::could_not_tell(format!(
                        "{} was ordered and the spawner answered `{seat}`, which no record can \
                         key on: {e}",
                        order.item
                    )))
                })?;
            wiring
                .store
                .assign(order.item, &seat, order.by)
                .map_err(|e| {
                    Refused::stopped(Stop::could_not_tell(format!(
                        "{} was ordered and `{seat}` was spawned, and the assignment did not \
                         land: {e}",
                        order.item
                    )))
                })?;
            write_order(order, wiring, note, Some(&seat), false).map_err(Refused::stopped)?;
            read_back(order, wiring, note, Some(&seat), Some(&seat)).map_err(Refused::stopped)?;
            announce(order, wiring, &spawned, base.as_deref()).map_err(Refused::stopped)?;
            // The item's own rendering moved under the brief: the assignment
            // and the seat in the index are both in it. Rendered again over the
            // same path, so the file a reader opens is the item as it stands
            // rather than as it was two writes ago. `{seat}` stays what a
            // transient dispatch named — nothing — because that is what this
            // order said and what `fleet brief` prints for it.
            //
            // A PINNED BRIEF IS NEVER RE-RENDERED: the flight directory's hash
            // covers those bytes, so a second write over them would be a pinned
            // input this verb moved.
            if pinned.is_none() {
                write_brief(wiring, order, note, TRANSIENT).map_err(Refused::stopped)?;
            }
            Ok(Given {
                item: order.item.to_string(),
                note: note.to_string(),
                brief_path,
                seat: Some(spawned),
                belt,
            })
        }
        SpawnOutcome::Refused(cause) => {
            withdraw(err, order, wiring, &cause).map_err(Refused::stopped)?;
            Err(Refused {
                stop: Stop::refused(format!(
                    "{} was not dispatched — {cause}; the order was withdrawn",
                    order.item
                )),
                withdrawn: Some(cause),
            })
        }
        // `withdrawn` stays `None`, which is the field's whole job: a flight
        // reads a hold off it, and a hold here would re-dispatch an item whose
        // order was never taken away.
        SpawnOutcome::CouldNotTell(cause) => {
            not_told(err, order, wiring, &cause).map_err(Refused::stopped)?;
            Err(Refused {
                stop: Stop::could_not_tell(format!(
                    "{} may or may not have been dispatched — {cause}; the order stands",
                    order.item
                )),
                withdrawn: None,
            })
        }
    }
}

/// The one event this verb writes.
///
/// AFTER THE READ-BACK AND BEFORE THE EXIT, always in that order: the note is
/// the order and the event is the fold's copy of it, so a crash between them
/// leaves an order nothing announced — which the fold reads as the record says
/// — and never an announcement no order stands behind.
///
/// WHICH DISPATCH THIS IS IS NOT WRITTEN (decision D2). The fold counts the
/// dispatches it reads; a verb that wrote a count would have to read the stream
/// to take one, which is the fold's own answer arriving by a second route.
///
/// THE SEAT IS THE OBJECT `{id, name?, kind}`, off the directory entry the
/// order resolved to: a reader keys on the id and reads the name beside it.
fn announce(
    order: &Order,
    wiring: &Wiring,
    seat: &SeatRef,
    base: Option<&str>,
) -> Result<(), Stop> {
    let mut payload = serde_json::Map::new();
    payload.insert("item".into(), order.item.into());
    payload.insert("seat".into(), serde_json::json!(seat));
    if let Some(base) = base {
        payload.insert("base".into(), base.into());
    }
    wiring
        .events
        .append(
            ITEM_DISPATCHED,
            order.by,
            serde_json::Value::Object(payload),
        )
        .map_err(|e| {
            stands(
                order.item,
                &format!("{ITEM_DISPATCHED} did not reach the stream: {e}"),
            )
        })
}

/// The order withdrawn: the index unset, one note saying so, and the
/// withdrawal read back.
fn withdraw(err: &mut dyn Write, order: &Order, wiring: &Wiring, cause: &str) -> Result<(), Stop> {
    let line = format!("{WITHDRAWN}: {cause}");
    wiring
        .store
        .unset_orders(order.item, order.by)
        .map_err(|e| stands(order.item, &format!("the index could not be unset: {e}")))?;
    wiring
        .store
        .note(order.item, &line, order.by)
        .map_err(|e| {
            stands(
                order.item,
                &format!("the withdrawal note did not land: {e}"),
            )
        })?;
    let item = read(wiring.store, order.item)?;
    if item.has_orders_key {
        return Err(stands(
            order.item,
            "the index still carries a fleet.orders key after the withdrawal",
        ));
    }
    let _ = writeln!(err, "withdrawn: {line}");
    Ok(())
}

/// The order LEFT STANDING, with one note saying what could not be observed.
///
/// The mirror of [`withdraw`] and it reads the index back the opposite way: the
/// key has to STILL be there. An order this path quietly took away would be the
/// rounding the third outcome exists to stop, arriving by the other side.
fn not_told(err: &mut dyn Write, order: &Order, wiring: &Wiring, cause: &str) -> Result<(), Stop> {
    let line = format!("{NOT_TOLD}: {cause}");
    wiring
        .store
        .note(order.item, &line, order.by)
        .map_err(|e| {
            stands(
                order.item,
                &format!("the could-not-tell note did not land: {e}"),
            )
        })?;
    let item = read(wiring.store, order.item)?;
    if !item.has_orders_key {
        return Err(stands(
            order.item,
            "the index carries no fleet.orders key after a spawn that could not be told",
        ));
    }
    let _ = writeln!(err, "could not tell: {line}");
    Ok(())
}

/// The index, written as one object that replaces the key whole.
fn write_order(
    order: &Order,
    wiring: &Wiring,
    note: &str,
    seat: Option<&str>,
    first: bool,
) -> Result<(), Stop> {
    // The note before the index, and only on the first pass: the second index
    // write of a transient dispatch adds the seat to an order already on the
    // record, and a second note would say the same thing twice.
    if first {
        wiring
            .store
            .note(order.item, note, order.by)
            .map_err(|e| wrote_nothing(order.item, "the order note", &e))?;
    }
    let payload = index_payload(order, seat);
    wiring
        .store
        .set_orders(order.item, &payload, order.by)
        .map_err(|e| {
            stands(
                order.item,
                &format!("the index did not land: {e}\n  RERUN: bd update {} --metadata '{payload}' --actor {}", order.item, order.by),
            )
        })
}

/// The index as JSON, with the seat present only where one was named. A
/// dispatch to a transient seat carries no seat until the spawn answers with
/// one.
pub fn index_payload(order: &Order, seat: Option<&str>) -> String {
    index(order.by, KIND, seat, order.at)
}

/// The order index, as the one function that writes its shape: a second place
/// that built it would be a second shape the read-back could disagree with.
pub fn index(by: &str, kind: &str, seat: Option<&str>, at: &str) -> String {
    let mut index = serde_json::Map::new();
    index.insert("by".into(), by.into());
    index.insert("kind".into(), kind.into());
    if let Some(seat) = seat {
        index.insert("seat".into(), seat.into());
    }
    index.insert("at".into(), at.into());
    keys::stamped(keys::ORDERS, index).to_string()
}

/// The order note the pack's template renders, for a caller that writes its own
/// order rather than calling [`dispatch`] — and for `fleet brief`, which renders
/// the order it prints from the index's `by` rather than reading the note back.
pub fn note_for(packs: &Packs, by: &str) -> Result<String, Stop> {
    note_text(packs, by)
}

/// One read, asserting the assignee, the note and the four index fields against
/// the ARGUMENTS — plus a token nothing wrote.
fn read_back(
    order: &Order,
    wiring: &Wiring,
    note: &str,
    assignee: Option<&str>,
    seat: Option<&str>,
) -> Result<(), Stop> {
    let item = read(wiring.store, order.item)?;

    if let Some(wanted) = assignee {
        if item.assignee.as_deref() != Some(wanted) {
            return Err(disagrees(
                order.item,
                "assignee",
                wanted,
                item.assignee.as_deref(),
                &assignee_repair(order, wanted),
            ));
        }
    }

    let Some(index) = item.orders.as_ref() else {
        return Err(disagrees(
            order.item,
            keys::ORDERS,
            &format!("an object at {} {}", keys::VERSION_FIELD, keys::VERSION),
            None,
            &index_repair(order, seat),
        ));
    };
    let wanted: [(&str, Option<&str>, Option<&str>); 4] = [
        ("by", Some(order.by), index.by.as_deref()),
        ("kind", Some(KIND), index.kind.as_deref()),
        ("seat", seat, index.seat.as_deref()),
        ("at", Some(order.at), index.at.as_deref()),
    ];
    for (field, want, got) in wanted {
        if want != got {
            return Err(disagrees(
                order.item,
                &format!("{}.{field}", keys::ORDERS),
                want.unwrap_or("(absent)"),
                got,
                &index_repair(order, seat),
            ));
        }
    }

    // The note half of the same read. A record carrying no notes field at all
    // is a third answer and not a disagreement: it is said, and not judged.
    match item.notes.as_deref() {
        Some(notes) => {
            let seen = brief::order_line(Some(notes));
            if seen.as_deref().map(normalised) != Some(normalised(note)) {
                return Err(disagrees(
                    order.item,
                    "the last order note",
                    note,
                    seen.as_deref(),
                    &index_repair(order, seat),
                ));
            }
        }
        None => {
            return Err(disagrees(
                order.item,
                "the notes",
                note,
                None,
                &index_repair(order, seat),
            ))
        }
    }

    let control = control_token();
    if item.document.contains(control) {
        return Err(Stop::could_not_tell(format!(
            "the read-back on {} carries {control}, which nothing wrote — the read is not \
             reading this item",
            order.item
        )));
    }
    Ok(())
}

/// Whitespace normalised to single spaces, because the store keeps a note with
/// the wrapping it was written with and the comparison is about the words.
fn normalised(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The dispatch note, rendered from the pack's template rather than composed.
fn note_text(packs: &Packs, by: &str) -> Result<String, Stop> {
    let template = packs.read(brief::DISPATCH_NOTE)?;
    let line = render(&template, &[("by", by)]).map_err(|name| {
        Stop::could_not_tell(format!(
            "`{}` writes `{{{name}}}`, which is not a placeholder this verb resolves",
            brief::DISPATCH_NOTE
        ))
    })?;
    let line = line.trim().to_string();
    if !line.contains(ORDER_MARK) {
        return Err(Stop::could_not_tell(format!(
            "`{}` renders `{line}`, which carries no `{ORDER_MARK}` — a note no reader can find \
             is not an order",
            brief::DISPATCH_NOTE
        )));
    }
    Ok(line)
}

/// The brief, rendered from the item as it now reads and written to the briefs
/// directory as the file a spawn hands its seat.
fn write_brief(wiring: &Wiring, order: &Order, note: &str, seat: &str) -> Result<PathBuf, Stop> {
    let text = wiring
        .store
        .show_text(order.item)
        .map_err(|e| stands(order.item, &format!("the item could not be read: {e}")))?;
    let body = brief::text(
        wiring.packs,
        wiring.project,
        &Subject {
            id: order.item,
            text: &text,
            order: note,
            seat,
            touched: order.touched,
        },
    )
    .map_err(|stop| stands(order.item, &stop.message))?;

    std::fs::create_dir_all(wiring.briefs_dir).map_err(|e| {
        stands(
            order.item,
            &format!(
                "the briefs directory {} could not be made: {e}",
                wiring.briefs_dir.display()
            ),
        )
    })?;
    // Renamed over rather than written in place: the second render of a
    // transient dispatch lands while a seat may already be reading the first,
    // and a reader must see one whole file or the other and never half of each.
    let path = wiring.briefs_dir.join(format!("{}.md", order.item));
    let scratch = wiring
        .briefs_dir
        .join(format!(".{}.{}.md", order.item, std::process::id()));
    let written = std::fs::write(&scratch, &body).and_then(|()| std::fs::rename(&scratch, &path));
    written.map_err(|e| {
        let _ = std::fs::remove_file(&scratch);
        stands(
            order.item,
            &format!("the brief could not be written to {}: {e}", path.display()),
        )
    })?;
    Ok(path)
}

fn read(store: &dyn Store, item: &str) -> Result<Item, Stop> {
    store.show(item).map_err(Stop::from)
}

fn why_not_ready(item: &Item) -> String {
    if item.status != "open" {
        return format!("its status is `{}`", item.status);
    }
    if item.blockers.is_empty() {
        return "the store does not list it among the ready".to_string();
    }
    format!("it is blocked by {}", item.blockers.join(", "))
}

fn standing(index: &Orders) -> String {
    format!(
        "kind={} by={} at={}",
        index.kind.as_deref().unwrap_or("(none)"),
        index.by.as_deref().unwrap_or("(none)"),
        index.at.as_deref().unwrap_or("(none)")
    )
}

fn wrote_nothing(item: &str, what: &str, e: &StoreError) -> Stop {
    Stop::could_not_tell(format!(
        "{what} on {item} did not land: {e}\n  NOTHING else was written — the item carries no order"
    ))
}

/// A failure after the record was written. The order is real and the message
/// says which part of it stands, because a caller that read this as "nothing
/// happened" would dispatch the item twice.
fn stands(item: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{why}\n  the order note on {item} STANDS and the order is real"
    ))
}

/// A read-back that disagrees, with the repair for THE FIELD THAT DISAGREED:
/// the caller names it, because an assignee handed the index's write would
/// read back lost again.
fn disagrees(item: &str, field: &str, wanted: &str, got: Option<&str>, repair: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{item} read back with {field} == {}\n  wanted: {wanted} (from the arguments)\n  \
         RERUN: {repair}",
        got.unwrap_or("(absent)"),
    ))
}

/// The assignee written again, as the store's own command.
fn assignee_repair(order: &Order, seat: &str) -> String {
    format!(
        "bd update {} --assignee {seat} --actor {}",
        order.item, order.by
    )
}

/// The index written again whole, as the store's own command, naming the seat
/// the read-back wanted: the id the argument resolved to, never the argument.
fn index_repair(order: &Order, seat: Option<&str>) -> String {
    format!(
        "bd update {} --metadata '{}' --actor {}",
        order.item,
        index_payload(order, seat),
        order.by
    )
}
