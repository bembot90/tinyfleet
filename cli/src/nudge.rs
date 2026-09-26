//! `fleet seat nudge` — the courier verb: one message to a live session from
//! outside any session.
//!
//! It delivers through the ring `fleet dispatch` already has, so the row
//! lookup, the pane and listing read and the turn typed into the seat's own
//! session are one path for both callers (`effect::type_turn`). The event says
//! `sent` only where the listing witnessed the turn taken.
//!
//! The seat argument is resolved first, through the seat list, and a name that
//! names no one seat is refused with the resolver's own exit. The two refusals
//! after it are the PROJECTION's, in a fixed order: the collector first (exit
//! 5), then the seat (exit 4). The projection is what makes a seat one of this
//! fleet's, so a seat with a live session and no published row is refused here
//! on purpose.

use std::path::Path;
use std::time::Duration;

use fleet_controller::effect::Typed;
use fleet_controller::events::{self, ActorRef, EventLog};
use fleet_controller::observe::RosterState;
use fleet_controller::seat::COLLECTOR_STALE_POLLS;
use fleet_controller::{clock, policy as controller};

use crate::exit::Exit;
use crate::item::{acting, stream_actor, SeatRing};
use crate::ui::{Stream, Tone, Ui};

/// The published document this verb refuses without.
const PROJECTION: &str = "projection.json";

/// The stream the event goes to, under the machine directory.
const STREAM: &str = "events.jsonl";

/// What the verb writes into the event's `source`, which is what tells a reader
/// this line is the courier's and not a threshold's.
const SOURCE: &str = "seat nudge";

/// What `seat nudge` takes.
#[derive(clap::Args)]
pub struct NudgeArgs {
    /// the seat to nudge
    pub seat: String,
    /// the message, carried verbatim
    #[arg(long, value_name = "TEXT")]
    pub text: String,
    /// how long the seat has to take it, over the policy's
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<u64>,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
}

pub fn nudge_command(ui: &Ui, args: &NudgeArgs) -> Exit {
    let here = match crate::transient::resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return stopped(&stop.message, stop.code),
    };
    // The argument through the seat list's resolver, and its id from here on:
    // the projection, the ring and the stream are keyed on it. Every sentence
    // names the seat by its machine name.
    let row = match crate::transient::seat_named(&here.machine_dir, &args.seat) {
        Ok(row) => row,
        Err(stop) => return stopped(&stop.message, stop.code),
    };
    let key = row.id.to_string();
    let seat = row.machine_name();
    // Who sent it, resolved as every writing verb's actor is: the verb has no
    // `--by`, so `FLEET_ACTOR`, else this machine's identity. The payload
    // carries it typed, as the stream's `{kind, id}`.
    let by = match acting("seat nudge", None, &here) {
        Ok(by) => stream_actor(&by),
        Err(stop) => return stopped(&stop.message, stop.code),
    };

    let document = match fresh_projection(&here.machine_dir) {
        Ok(document) => document,
        Err(why) => return stopped(&why, Exit::NoCollector.code()),
    };
    if let Err(why) = published_live(&document, &key, &seat) {
        return stopped(&why, Exit::NoSession.code());
    }

    let ring = SeatRing {
        machine_dir: here.machine_dir.clone(),
    };
    let rung = ring.ring_with(&key, &args.text, args.timeout.map(Duration::from_secs));
    // `sent` only where the listing witnessed the turn taken; a turn queued
    // behind the one in hand is typed and exits 0 too, and says it is queued.
    let exit = match &rung.typed {
        Typed::Delivered | Typed::Queued => Exit::Done,
        Typed::Blocked(_) | Typed::Failed(_) => Exit::Refused,
        // The verb's own read is the fresher instrument: no live pane under
        // the seat's session, or no listed row carrying its pid. Nothing was
        // typed, so nothing is written to the stream either.
        Typed::Absent => {
            return stopped(
                &format!(
                    "`{seat}` has no live session — no live pane under its session, or no \
                     listed row carrying the pane's pid, whatever the projection published"
                ),
                Exit::NoSession.code(),
            )
        }
    };
    let outcome = rung.typed.recorded();

    let stream = here.machine_dir.join(STREAM);
    let mut log = EventLog::open(&stream);
    // The line is ABOUT the seat, as every `session.*` line is: its actor is the
    // seat nudged, and who sent it is the payload's `by`.
    if let Err(e) = log.append(
        events::SESSION_NUDGED,
        &ActorRef::seat(&key),
        serde_json::json!({
            "session": rung.session,
            "by": by,
            "source": SOURCE,
            "outcome": outcome,
        }),
    ) {
        return stopped(
            &format!("could not append to {}: {e}", stream.display()),
            Exit::CouldNotTell.code(),
        );
    }

    let session = rung.session.as_deref().unwrap_or("no session id");
    let (verb, tone) = match (&rung.typed, exit) {
        (Typed::Queued, _) => ("queued", Tone::Good),
        (_, Exit::Done) => ("nudged", Tone::Good),
        _ => ("not nudged", Tone::Bad),
    };
    ui.status(
        Stream::Err,
        tone,
        verb,
        &seat,
        Some(&format!("{session} — {outcome}")),
    );
    exit
}

/// The published document, refused unless a collector is plainly consuming.
///
/// The age is printed on the stale answer because the number is what tells a
/// reader whether the collector stopped a moment ago or yesterday, and the two
/// have different answers.
fn fresh_projection(machine_dir: &Path) -> Result<serde_json::Value, String> {
    let path = machine_dir.join(PROJECTION);
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Err(format!(
            "no collector is consuming — there is no projection at {}; run `fleet start`",
            path.display()
        ));
    };
    let Ok(document) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Err(format!(
            "no collector is consuming — the projection at {} does not parse; run `fleet start`",
            path.display()
        ));
    };
    let poll_seconds = document["fleet"]["poll_seconds"]
        .as_u64()
        .unwrap_or(controller::DEFAULT_POLL_SECONDS);
    let generated_at = document["generated_at"].as_str().unwrap_or_default();
    // A stamp that does not parse, and one in the future, are both `None`, and
    // neither is rounded into "fresh": a document nobody can date is not one
    // anybody can call current.
    match clock::seconds_since_stamp(generated_at) {
        Some(age) if age <= poll_seconds * COLLECTOR_STALE_POLLS => Ok(document),
        Some(age) => Err(format!(
            "no collector is consuming — the projection at {} was generated at {generated_at}, \
             {age}s ago, outside {COLLECTOR_STALE_POLLS} poll intervals of {poll_seconds}s; run \
             `fleet start`",
            path.display()
        )),
        None => Err(format!(
            "no collector is consuming — the projection at {} carries no readable \
             `generated_at`; run `fleet start`",
            path.display()
        )),
    }
}

/// The seat's published row, found by its id, and whether the collector saw a
/// live session on it. `seat` is the machine name the sentences say.
fn published_live(document: &serde_json::Value, key: &str, seat: &str) -> Result<(), String> {
    let seats = document["seats"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let Some(row) = seats.iter().find(|row| row["seat"]["id"] == key) else {
        return Err(format!(
            "the projection carries no row for `{seat}` — the collector is what makes a seat one \
             of this fleet's, so a session it has not published is not one this verb rings"
        ));
    };
    // `present` and nothing else: `prompt-blocked` is split from it exactly so
    // every rule says which side it falls on, and a session stopped in front of
    // a person takes no turn.
    let live = RosterState::Present.as_str();
    match row["roster_state"].as_str() {
        Some(state) if state == live => Ok(()),
        Some(state) => Err(format!(
            "`{seat}` has no live session — its row reads {state} and not {live}"
        )),
        None => Err(format!(
            "`{seat}`'s row carries no `roster_state`, so it cannot be read as {live}"
        )),
    }
}

fn stopped(message: &str, code: u8) -> Exit {
    eprintln!("fleet seat nudge: {message}");
    Exit::from_status(code).unwrap_or(Exit::CouldNotTell)
}
