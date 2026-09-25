//! `fleet dispatch <item> [--to <seat>]` — the one record that says a seat may
//! begin.
//!
//! Three writes make an order: the assignee, the ordered entry on the item's
//! timeline and the index. The failure a multi-writer shape produces is an
//! order written to one surface and not the other, which reads as clean on
//! both — so the three are one act here, and the act ends by reading them back.
//!
//! WRITE ORDER is the entry before the index. A crash between them leaves an
//! item ordered on the record and merely unindexed, never indexed with no order
//! behind it. The index stays the current-order projection, written in the same
//! act, and it is what every verb decides an item's order state on.
//!
//! THE READ-BACK COMPARES AGAINST THE ARGUMENTS, never against the payload:
//! an expectation taken from the object we just built agrees with whatever that
//! object happens to hold, so a writer that stopped setting a field would
//! satisfy a check that had stopped asking for it in the same edit.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::entry::{Body, OrderWithdrawn, Ordered, Withdrawal};
use crate::item::brief::{self, Packs, Subject, TRANSIENT};
use crate::item::deliver::holds;
use crate::item::{
    control_token, recorded, render, show, signal, Events, Project, Ring, RingOutcome, Spawn,
    SpawnOutcome, Spawner, Stop, Unrecorded, ITEM_ENTRY, NO_SESSION, REFUSED,
};
use crate::seat::actor::Actor;
use crate::seat::identity::{Directory, Kind, SeatId, SeatRef};
use crate::store::types::CONTRACT_VERSION;
use crate::store::{
    self, Filter, Item, ItemId, OrderState, Stamp, Status, Store, StoreError, Update, WithdrawFence,
};

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

/// The words a withdrawal says on stderr. The record's own half is the
/// `order_withdrawn` entry, so an item a spawn refused reads as one nobody was
/// ever given rather than as one whose seat vanished.
pub const WITHDRAWN: &str = "DISPATCH WITHDRAWN — spawn refused";

/// The words a spawn nobody could observe says on stderr, and nowhere else. It
/// is NOT a withdrawal: the order is still on the record, and the cause is what
/// a retry acts on.
pub const NOT_TOLD: &str = "DISPATCH COULD NOT TELL — the spawn could not be observed";

/// The order, as its arguments.
pub struct Order<'a> {
    pub item: &'a str,
    pub to: Option<&'a str>,
    /// Who gives the order: the ordered entry's author, and its string form is
    /// what the index and every other write carry.
    pub by: &'a Actor,
    /// The clock, taken by the caller: core reads none.
    pub at: &'a str,
    /// A brief already written, handed to the spawn as the first turn instead
    /// of one rendered here.
    ///
    /// A caller that pinned a brief needs the seat to read that file and not a
    /// re-render of an item that has moved since. The order itself is
    /// unchanged: the entry and the index are written here either way, and the
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
    /// The id the store gave the ordered entry that seated the order: the one
    /// naming the seat, on either path.
    pub entry: String,
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
    // Before the first write: the brief refuses on the same reading, and an
    // order written ahead of a brief that cannot render is one nobody reads.
    wiring.project.refuse_moved().map_err(Refused::stopped)?;
    // And the clock: an order is written with its stamp, so one given at a
    // time that is no stamp is a question asked before anything is written.
    stamp(order).map_err(Refused::stopped)?;

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
        Some(seat) => to_named_seat(err, order, wiring, seat).map_err(Refused::stopped)?,
        None => to_a_transient_seat(err, order, wiring)?,
    };

    // THE ORDER LINE FIRST AND THE BELT UNDER IT, on the same stream: a person
    // reading a dispatch's own output reads what the machine was measured at
    // beside the order it gave, in the words `fleet seat spawn` prints. A
    // dispatch to a named seat starts nothing and prints nothing here. The seat
    // is named as a sentence names it, and the entry by the store's id.
    let line = match named {
        Some(seat) => format!(
            "ordered {} to {} — entry {}",
            given.item,
            seat.machine_name(),
            given.entry
        ),
        None => format!(
            "ordered {} to a transient seat — entry {}",
            given.item, given.entry
        ),
    };
    let said = match &given.belt {
        Some(belt) => format!("{line}\n{belt}\n"),
        None => format!("{line}\n"),
    };
    out.write_all(said.as_bytes()).map_err(|e| {
        Refused::stopped(Stop::could_not_tell(format!(
            "the order line could not be written: {e}"
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
    let ready: Vec<store::ItemId> = wiring
        .store
        .list(&Filter::Ready)?
        .into_iter()
        .map(|row| row.id)
        .collect();

    if !ready.iter().any(|id| id == order.item) {
        return Err(Stop::refused(format!(
            "{} is not ready — {}",
            order.item,
            why_not_ready(item)
        )));
    }
    refuse_an_epic(item)?;
    match &item.order {
        OrderState::None => {}
        // An index this binary cannot read is not an order it can weigh: it may
        // be one, at a version a newer fleet wrote, so it is could-not-tell and
        // never taken for absent or overwritten.
        OrderState::Unreadable => {
            return Err(Stop::could_not_tell(format!(
                "{}'s order index is not one this fleet can read — this fleet reads an order \
                 index at the store contract's v {CONTRACT_VERSION} — and a dispatch will not \
                 guess whether it is an order",
                order.item
            )));
        }
        OrderState::Ordered(index) => {
            return Err(Stop::refused(format!(
                "{} already carries an order — {}",
                order.item,
                standing(index)
            )));
        }
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
    let held: Vec<String> = holds(wiring.store, &seat.id)?
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

/// The named path: assign, entry, index, read back, brief, ring.
///
/// Every write, the event and the ring carry the seat's full id; the brief and
/// every sentence name it by its machine name.
fn to_named_seat(
    err: &mut dyn Write,
    order: &Order,
    wiring: &Wiring,
    named: &SeatRef,
) -> Result<Given, Stop> {
    let id = named.id.to_string();
    let seat = id.as_str();
    let label = wiring.seats.label(&named.id);
    wiring
        .store
        .update(
            &ItemId::from(order.item),
            &Update::assignee(named.id),
            order.by,
        )
        .map_err(|e| wrote_nothing(order.item, "the assignee", &e))?;
    let entry = write_order(order, wiring, Some(&named.id), true)?;
    read_back(order, wiring, Some(&named.id), Some(&named.id))?;
    announce(order, wiring, &entry)?;

    let brief_path = match order.brief {
        Some(pinned) => pinned.to_path_buf(),
        None => write_brief(wiring, order, &named.machine_name())?,
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
            entry,
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
/// The ordered entry precedes the spawn, naming no seat, because the brief IS
/// the first turn and a seat started before the order exists would read an
/// item nobody had given it. A second ordered entry names the seat once the
/// spawn has made one, so the timeline's current order is the seated one. A
/// spawn that refuses therefore withdraws the order in the same act, so a
/// refusal never leaves an ordered item nobody holds.
///
/// A SPAWN THAT COULD NOT BE OBSERVED IS NOT A REFUSAL and withdraws nothing:
/// the order stands with nothing written after it, and the cause is the exit's
/// message, so the retry that could still succeed has an order to succeed
/// under.
fn to_a_transient_seat(
    err: &mut dyn Write,
    order: &Order,
    wiring: &Wiring,
) -> Result<Given, Refused> {
    write_order(order, wiring, None, true).map_err(Refused::stopped)?;
    read_back(order, wiring, None, None).map_err(Refused::stopped)?;

    let pinned = order.brief.map(Path::to_path_buf);
    let brief_path = match &pinned {
        Some(path) => path.clone(),
        None => write_brief(wiring, order, TRANSIENT).map_err(Refused::stopped)?,
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
        SpawnOutcome::Spawned { seat, belt } => {
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
                .update(
                    &ItemId::from(order.item),
                    &Update::assignee(spawned.id),
                    order.by,
                )
                .map_err(|e| {
                    Refused::stopped(Stop::could_not_tell(format!(
                        "{} was ordered and `{seat}` was spawned, and the assignment did not \
                         land: {e}",
                        order.item
                    )))
                })?;
            let entry =
                write_order(order, wiring, Some(&spawned.id), false).map_err(Refused::stopped)?;
            read_back(order, wiring, Some(&spawned.id), Some(&spawned.id))
                .map_err(Refused::stopped)?;
            announce(order, wiring, &entry).map_err(Refused::stopped)?;
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
                write_brief(wiring, order, TRANSIENT).map_err(Refused::stopped)?;
            }
            Ok(Given {
                item: order.item.to_string(),
                entry,
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

/// The one event this verb writes: the signal of the ordered entry that
/// seated the order.
///
/// AFTER THE READ-BACK AND BEFORE THE EXIT, always in that order: the entry is
/// the order and the line only says it was written, so a crash between them
/// leaves an order nothing signalled — which a reader of the record reads as
/// the record says — and never a signal no order stands behind.
///
/// THE SEAT AND THE BASE ARE NOT ON IT [ASSUMES D2]. The seat is the entry's
/// and the index's; a reader that wants either reads the record the signal
/// names.
fn announce(order: &Order, wiring: &Wiring, entry: &str) -> Result<(), Stop> {
    signal(wiring.events, order.by, order.item, entry, "ordered").map_err(|e| {
        stands(
            order.item,
            &format!("{ITEM_ENTRY} did not reach the stream: {e}"),
        )
    })
}

/// The order withdrawn: the assignee cleared and the order taken away in one
/// act, one `order_withdrawn` entry naming the spawner's cause, and the
/// withdrawal read back. The transient path assigns nobody before its spawn
/// answers, so the assignee the act clears is one nothing here wrote.
fn withdraw(err: &mut dyn Write, order: &Order, wiring: &Wiring, cause: &str) -> Result<(), Stop> {
    wiring
        .store
        .order_withdraw(
            &ItemId::from(order.item),
            &WithdrawFence::default(),
            order.by,
        )
        .map_err(|e| {
            stands(
                order.item,
                &format!("the order could not be withdrawn: {e}"),
            )
        })?;
    let withdrawn = Body::OrderWithdrawn(OrderWithdrawn {
        why: Withdrawal::SpawnRefused,
        seat: None,
        cause: Some(cause.to_string()),
    });
    recorded(wiring.store, order.item, &withdrawn, order.by).map_err(
        |unrecorded| match unrecorded {
            Unrecorded::NotWritten(e) => stands(
                order.item,
                &format!("the order_withdrawn entry did not land: {e}"),
            ),
            Unrecorded::Unconfirmed(why) => stands(order.item, &why),
        },
    )?;
    let item = read(wiring.store, order.item)?;
    if !matches!(item.order, OrderState::None) {
        return Err(stands(
            order.item,
            "the item still carries its order index after the withdrawal",
        ));
    }
    let _ = writeln!(err, "withdrawn: {WITHDRAWN}: {cause}");
    Ok(())
}

/// The order LEFT STANDING, with nothing written: what could not be observed is
/// the exit's message and stderr's line, and no entry on the record.
///
/// The mirror of [`withdraw`] and it reads the index back the opposite way: the
/// key has to STILL be there. An order this path quietly took away would be the
/// rounding the third outcome exists to stop, arriving by the other side.
fn not_told(err: &mut dyn Write, order: &Order, wiring: &Wiring, cause: &str) -> Result<(), Stop> {
    let item = read(wiring.store, order.item)?;
    if matches!(item.order, OrderState::None) {
        return Err(stands(
            order.item,
            "the item carries no order index after a spawn that could not be told",
        ));
    }
    let _ = writeln!(err, "could not tell: {NOT_TOLD}: {cause}");
    Ok(())
}

/// The ordered entry, appended and read back, then the order, which replaces
/// the one the item carried whole. Answers the entry's id.
///
/// Both passes append: the first names the seat where one was named and no
/// seat on the transient path, and the transient path's second names the seat
/// the spawn made, so the timeline's current order is the seated one. What the
/// pass changes is what a failed append leaves: before the first nothing is
/// ordered, and before the second the first order already stands.
fn write_order(
    order: &Order,
    wiring: &Wiring,
    seat: Option<&SeatId>,
    first: bool,
) -> Result<String, Stop> {
    let ordered = Body::Ordered(Ordered {
        order: store::OrderKind::Dispatch,
        seat: seat.copied(),
    });
    let entry = recorded(wiring.store, order.item, &ordered, order.by).map_err(|unrecorded| {
        match unrecorded {
            Unrecorded::NotWritten(e) if first => {
                wrote_nothing(order.item, "the ordered entry", &e)
            }
            Unrecorded::NotWritten(e) => stands(
                order.item,
                &format!("the ordered entry naming the seat did not land: {e}"),
            ),
            Unrecorded::Unconfirmed(why) => stands(order.item, &why),
        }
    })?;
    let given = the_order(order, seat.copied())?;
    wiring
        .store
        .order_set(&ItemId::from(order.item), &given, order.by)
        .map_err(|e| {
            stands(
                order.item,
                &format!(
                    "the order did not land: {e}\n  READ: fleet item show {}",
                    order.item
                ),
            )
        })?;
    Ok(entry)
}

/// The order this dispatch gives, with the seat present only where one was
/// named: a dispatch to a transient seat names no seat until the spawn answers
/// with one. The one place it is built, so the order written and the order the
/// read-back wants are the same value.
fn the_order(order: &Order, seat: Option<SeatId>) -> Result<store::Order, Stop> {
    Ok(store::Order {
        kind: store::OrderKind::Dispatch,
        by: order.by.clone(),
        seat,
        at: stamp(order)?,
    })
}

/// The order's clock as the stamp it is written with.
fn stamp(order: &Order) -> Result<Stamp, Stop> {
    Stamp::parse(order.at).ok_or_else(|| {
        Stop::could_not_tell(format!(
            "{} was ordered at `{}`, which is not a stamp — the form is YYYY-MM-DDTHH:MM:SSZ — so \
             no order this fleet can read could be written with it",
            order.item, order.at
        ))
    })
}

/// One read, asserting the assignee and the four fields of the order index
/// against the ARGUMENTS — plus a token nothing wrote. The entry is not asked
/// about here: [`recorded`] read it back when it was appended.
///
/// The fields are compared as the order's own types: who gave it as an actor,
/// its kind, the seat as a seat id and when as a stamp. A wanted `at` that is
/// no stamp is a question and not a disagreement: the index it was written
/// into cannot read as an order, and the read-back would blame the store.
fn read_back(
    order: &Order,
    wiring: &Wiring,
    assignee: Option<&SeatId>,
    seat: Option<&SeatId>,
) -> Result<(), Stop> {
    let item = read(wiring.store, order.item)?;
    let seat_text = seat.map(SeatId::to_string);
    let given = the_order(order, seat.copied())?;

    if let Some(wanted) = assignee {
        if item.assignee.as_ref() != Some(wanted) {
            return Err(disagrees(
                order.item,
                "assignee",
                &wanted.to_string(),
                item.assignee.map(|held| held.to_string()).as_deref(),
            ));
        }
    }

    let at = given.at.clone();
    let index = match &item.order {
        OrderState::Ordered(index) => index,
        OrderState::Unreadable => {
            return Err(disagrees(
                order.item,
                "the order index",
                "a readable order index",
                Some("one this fleet cannot read"),
            ))
        }
        OrderState::None => {
            return Err(disagrees(
                order.item,
                "the order index",
                "a readable order index",
                None,
            ))
        }
    };
    let field = |name: &str, wanted: Option<String>, got: Option<String>| {
        disagrees(
            order.item,
            &format!("order.{name}"),
            wanted.as_deref().unwrap_or("(absent)"),
            got.as_deref(),
        )
    };
    if index.by != *order.by {
        return Err(field(
            "by",
            Some(order.by.to_string()),
            Some(index.by.to_string()),
        ));
    }
    if index.kind != store::OrderKind::Dispatch {
        return Err(field(
            "kind",
            Some(KIND.to_string()),
            Some(index.kind.as_str().to_string()),
        ));
    }
    if index.seat.as_ref() != seat {
        return Err(field(
            "seat",
            seat_text.clone(),
            index.seat.map(|held| held.to_string()),
        ));
    }
    if index.at != at {
        return Err(field(
            "at",
            Some(at.to_string()),
            Some(index.at.to_string()),
        ));
    }

    let control = control_token();
    if item.proof.carries(control) {
        return Err(Stop::could_not_tell(format!(
            "the read-back on {} carries {control}, which nothing wrote — the read is not \
             reading this item",
            order.item
        )));
    }
    Ok(())
}

/// The brief, rendered from the item as it now reads and written to the briefs
/// directory as the file a spawn hands its seat. The item is read with its
/// timeline and rendered by [`show::render`], and the order by
/// [`brief::order_text`] off the index just written: the text `fleet brief`
/// prints.
fn write_brief(wiring: &Wiring, order: &Order, seat: &str) -> Result<PathBuf, Stop> {
    let unread = |e: StoreError| stands(order.item, &format!("the item could not be read: {e}"));
    let record = wiring.store.show(order.item).map_err(unread)?;
    let timeline = wiring.store.timeline(&record.id).map_err(unread)?;
    let text = show::render(&record, &timeline);
    let OrderState::Ordered(index) = &record.order else {
        return Err(stands(
            order.item,
            "the order index read back and then was gone from under the brief",
        ));
    };
    let body = brief::text(
        wiring.packs,
        wiring.project,
        &Subject {
            id: order.item,
            text: &text,
            order: &brief::order_text(index),
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
    if item.status != Status::Open {
        return format!("its status is `{}`", item.status);
    }
    if item.blockers.is_empty() {
        return "the store does not list it among the ready".to_string();
    }
    let blockers: Vec<&str> = item.blockers.iter().map(|id| id.as_str()).collect();
    format!("it is blocked by {}", blockers.join(", "))
}

fn standing(index: &store::Order) -> String {
    format!(
        "kind={} by={} at={}",
        index.kind.as_str(),
        index.by,
        index.at
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
        "{why}\n  the ordered entry on {item} STANDS and the order is real"
    ))
}

/// A read-back that disagrees, naming THE FIELD THAT DISAGREED and the read
/// that shows the item. The repair is a read and never a write to make again:
/// the command that writes a field is the store's, and not fleet's to print.
fn disagrees(item: &str, field: &str, wanted: &str, got: Option<&str>) -> Stop {
    Stop::could_not_tell(format!(
        "{item} read back with {field} == {}\n  wanted: {wanted} (from the arguments)\n  \
         READ: fleet item show {item}",
        got.unwrap_or("(absent)"),
    ))
}
