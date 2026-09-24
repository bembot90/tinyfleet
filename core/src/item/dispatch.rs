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
use crate::store::{Item, Store, StoreError};

/// The kind of order this verb writes. The reference's other two grammars —
/// a run's feed, a spawn's own line — are that repository's; here there is one
/// form for giving work.
pub const KIND: &str = "dispatch";

/// The kind a flight writes when it hands a DELIVERED item to a reviewer seat
/// (flights PRD R14). It is an order like any other — the seat holds the item
/// and `fleet review` runs against one it holds — and the kind is what tells a
/// reader which of the two the seat was given.
pub const REVIEW_KIND: &str = "review";

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
    /// of one rendered here (flights PRD Q1, § The flight directory).
    ///
    /// A flight's brief is PINNED at takeoff and the directory's hash covers
    /// it, so the file the seat reads has to be that file and not a re-render
    /// of an item that has moved since. The order itself is unchanged: the note
    /// and the index are written here either way, and the pinned brief says so
    /// rather than standing in for one.
    pub brief: Option<&'a Path>,
    /// The commit the spawned seat's worktree is cut from, where the caller
    /// names one (flights PRD R11, R14). It rides BESIDE the pinned brief
    /// because both are the flight's, and both reach the spawn in one act.
    pub base: Option<&'a str>,
    /// The model the spawned seat runs on, where the caller names one; `None`
    /// leaves the fleet's policy default.
    pub model: Option<&'a str>,
    /// The builder's gate — the command the seat runs over its own diff before
    /// it delivers — as the caller hands it. It reaches the brief's `{touched}`
    /// and the spawned seat's permission rules; `None` renders the brief's
    /// named absence ([`brief::DERIVE_TOUCHED`]).
    pub touched: Option<&'a str>,
}

/// A dispatch that stopped, with the one outcome a flight has to tell from the
/// others carried as a value.
///
/// A spawn the belt refused is a HOLD — the order withdrawn, the item retried
/// on the next call — and every other stop is not. A caller that read the two
/// apart by matching the refusal's prose would go quiet the day the sentence
/// changed (flights PRD R9).
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
    /// The seats the machine's config carries. A `--to` naming anything else is
    /// a seat this fleet does not run.
    pub seats: &'a [String],
    pub ring: &'a dyn Ring,
    pub spawner: &'a dyn Spawner,
    pub events: &'a dyn Events,
}

/// The order given, for a caller that wants to say what happened.
pub struct Given {
    pub note: String,
    pub brief_path: PathBuf,
    pub seat: Option<String>,
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
    refuse_unless_dispatchable(order, wiring).map_err(Refused::stopped)?;

    let given = match order.to {
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

/// The five refusals, in order, each of them before any write.
fn refuse_unless_dispatchable(order: &Order, wiring: &Wiring) -> Result<(), Stop> {
    let ready = wiring.store.ready()?;
    let item = read(wiring.store, order.item)?;

    if !ready.iter().any(|id| id == order.item) {
        return Err(Stop::refused(format!(
            "{} is not ready — {}",
            order.item,
            why_not_ready(&item)
        )));
    }
    refuse_an_epic(&item)?;
    if item.has_orders_key {
        return Err(Stop::refused(format!(
            "{} already carries an order — {}",
            order.item,
            standing(&item)
        )));
    }

    let Some(seat) = order.to else {
        return Ok(());
    };
    if !wiring.seats.iter().any(|known| known == seat) {
        return Err(Stop::refused(format!(
            "`{seat}` is not a seat this machine runs — the seats it carries are {}",
            if wiring.seats.is_empty() {
                "none".to_string()
            } else {
                wiring.seats.join(", ")
            }
        )));
    }
    // What the seat HOLDS, in deliver's own reading: an item merely assigned
    // to it — an epic, a bug nobody ordered — is not work it was given.
    let held: Vec<String> = holds(wiring.store, seat)?
        .held
        .into_iter()
        .map(|row| format!("{} ({})", row.id, row.status))
        .collect();
    if !held.is_empty() {
        return Err(Stop::refused(format!(
            "`{seat}` already holds {} — one item at a time",
            held.join(", ")
        )));
    }
    Ok(())
}

/// An epic, refused by its TYPE. The store's ready list keeps an open,
/// unblocked epic — it is the store's answer and not a list of what a seat can
/// build — so the ready check alone lets one through. `ask` refuses on the same
/// reading, because a park on an epic is a gate the store will not tie to it.
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
fn to_named_seat(
    err: &mut dyn Write,
    order: &Order,
    wiring: &Wiring,
    note: &str,
    seat: &str,
) -> Result<Given, Stop> {
    wiring
        .store
        .assign(order.item, seat, order.by)
        .map_err(|e| wrote_nothing(order.item, "the assignee", &e))?;
    write_order(order, wiring, note, Some(seat), true)?;
    read_back(order, wiring, note, Some(seat), Some(seat))?;
    // A NAMED SEAT WRITES NO BASE: no worktree was cut for this order, so there
    // is no commit the seat started from that this verb could read.
    announce(order, wiring, seat, None)?;

    let brief_path = match order.brief {
        Some(pinned) => pinned.to_path_buf(),
        None => write_brief(wiring, order, note, seat)?,
    };
    let text = render(
        RING,
        &[
            ("item", order.item),
            ("path", &brief_path.display().to_string()),
        ],
    )
    .map_err(|name| Stop::could_not_tell(format!("the ring names `{{{name}}}`")))?;

    match wiring.ring.ring(seat, &text) {
        RingOutcome::Delivered => Ok(Given {
            note: note.to_string(),
            brief_path,
            seat: Some(seat.to_string()),
            belt: None,
        }),
        RingOutcome::Absent => {
            let _ = writeln!(err, "brief: {}", brief_path.display());
            Err(Stop {
                code: NO_SESSION,
                message: format!(
                    "ORDERED, NOT RUNG: no live session for {seat}; the order stands and the \
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
        SpawnOutcome::Spawned { seat, base, belt } => {
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
            announce(order, wiring, &seat, base.as_deref()).map_err(Refused::stopped)?;
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
                note: note.to_string(),
                brief_path,
                seat: Some(seat),
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

/// The one event this verb writes (flights PRD Q4a).
///
/// AFTER THE READ-BACK AND BEFORE THE EXIT, always in that order: the note is
/// the order and the event is the fold's copy of it, so a crash between them
/// leaves an order nothing announced — which the fold reads as the record says
/// — and never an announcement no order stands behind.
///
/// WHICH DISPATCH THIS IS IS NOT WRITTEN (decision D2). The fold counts the
/// dispatches it reads; a verb that wrote a count would have to read the stream
/// to take one, which is the fold's own answer arriving by a second route.
fn announce(order: &Order, wiring: &Wiring, seat: &str, base: Option<&str>) -> Result<(), Stop> {
    let mut payload = serde_json::Map::new();
    payload.insert("item".into(), order.item.into());
    payload.insert("seat".into(), seat.into());
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
            "the index still carries an orders key after the withdrawal",
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
            "the index carries no orders key after a spawn that could not be told",
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
    index(order.by, KIND, seat, order.at, None)
}

/// The order index, as the one function that writes its shape.
///
/// A flight writes two orders this verb cannot: the reviewer's, whose item
/// already carries an order, and a return's, whose ordinal says which resume it
/// is. Both are the same four fields plus the ordinal, and a second place that
/// built them would be a second shape the read-back could disagree with.
///
/// `ordinal` is ABSENT ON A FIRST DISPATCH rather than 1: the count a reader
/// wants is the fold's, and a key written only where there is something to say
/// cannot go stale against it.
pub fn index(by: &str, kind: &str, seat: Option<&str>, at: &str, ordinal: Option<u64>) -> String {
    let mut index = serde_json::Map::new();
    index.insert("by".into(), by.into());
    index.insert("kind".into(), kind.into());
    if let Some(seat) = seat {
        index.insert("seat".into(), seat.into());
    }
    index.insert("at".into(), at.into());
    if let Some(ordinal) = ordinal {
        index.insert("ordinal".into(), ordinal.into());
    }
    serde_json::Value::Object(
        [("orders".to_string(), serde_json::Value::Object(index))]
            .into_iter()
            .collect(),
    )
    .to_string()
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
                order,
            ));
        }
    }

    let Some(index) = item.orders.as_ref() else {
        return Err(disagrees(
            order.item,
            "metadata.orders",
            "an object",
            None,
            order,
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
                &format!("metadata.orders.{field}"),
                want.unwrap_or("(absent)"),
                got,
                order,
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
                    order,
                ));
            }
        }
        None => return Err(disagrees(order.item, "the notes", note, None, order)),
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

fn standing(item: &Item) -> String {
    match item.orders.as_ref() {
        Some(index) => format!(
            "kind={} by={} at={}",
            index.kind.as_deref().unwrap_or("(none)"),
            index.by.as_deref().unwrap_or("(none)"),
            index.at.as_deref().unwrap_or("(none)")
        ),
        None => "its orders key is not an object".to_string(),
    }
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

fn disagrees(item: &str, field: &str, wanted: &str, got: Option<&str>, order: &Order) -> Stop {
    Stop::could_not_tell(format!(
        "{item} read back with {field} == {}\n  wanted: {wanted} (from the arguments)\n  \
         RERUN: bd update {item} --metadata '{}' --actor {}",
        got.unwrap_or("(absent)"),
        index_payload(order, order.to),
        order.by
    ))
}
