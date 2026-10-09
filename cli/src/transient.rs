//! `fleet seat spawn`, `fleet seat feed`, `fleet seat retire` — the cli half of
//! the controller's transient-seat primitives.
//!
//! Everything here is what only a process knows: the project the cwd resolves
//! to, the clock and the file a turn is read from; the two directories, the
//! agent and the host it is wired with are
//! `fleet_controller::project::wiring`'s. The refusals, the exits and the
//! writes are `fleet_controller::transient`'s, so the belt that would refuse a
//! spawn lives beside the loop that starts the session.

use std::path::{Path, PathBuf};

use fleet_controller::host::Host;
use fleet_controller::project::wiring::{permissions_of, seat_named, spawn_host, verb_host, Wired};
use fleet_controller::project::{open_store, resolve_at, Here};
use fleet_controller::transient::{self, withdrawn_from, Machine};
use fleet_controller::{clock, platform, sessions};
use fleet_core::entry::Entry;

use fleet_core::item::land::{self, Release};
use fleet_core::item::{Spawn, SpawnOutcome, Spawner, Stop, COULD_NOT_TELL};
use fleet_core::seat;
use fleet_core::seat::identity::{Kind, SeatId, SeatRef};
use fleet_core::store::ItemId;

use crate::envelope;
use crate::exit::{refuse_stop, Exit};
use crate::item::acting;

/// The three verb names the envelope's documents carry, which are also the
/// words each verb's own stderr line names itself by.
const SPAWN: &str = "seat spawn";
const FEED: &str = "seat feed";
const RETIRE: &str = "seat retire";

/// What `seat spawn` takes.
#[derive(clap::Args)]
pub struct SpawnArgs {
    /// the file whose text is the session's first turn
    #[arg(long = "first-turn", value_name = "FILE")]
    pub first_turn: PathBuf,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// the model, over the policy's default
    #[arg(long, value_name = "ID")]
    pub model: Option<String>,
    /// the commit to cut the worktree at; else the trunk
    #[arg(long, value_name = "COMMIT")]
    pub base: Option<String>,
    /// the builder's checks the seat's rules let it run
    #[arg(long, value_name = "COMMAND")]
    pub touched: Option<String>,
    /// print the envelope document instead of the name
    #[arg(long)]
    pub json: bool,
}

/// What `seat feed` takes.
#[derive(clap::Args)]
pub struct FeedArgs {
    /// the transient seat to feed
    pub seat: String,
    /// the file whose text is the seat's next first turn
    #[arg(long = "first-turn", value_name = "FILE")]
    pub first_turn: PathBuf,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// print the envelope document instead of the line
    #[arg(long)]
    pub json: bool,
}

/// What `seat retire` takes.
#[derive(clap::Args)]
pub struct RetireArgs {
    /// the transient seat to retire
    pub seat: String,
    /// license a seat whose session is already gone
    #[arg(long)]
    pub dead: bool,
    /// who is retiring it; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// print the envelope document instead of the reclaim
    #[arg(long)]
    pub json: bool,
}

pub fn spawn_command(args: &SpawnArgs) -> Exit {
    let here = match resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return refuse_stop(SPAWN, &stop, args.json),
    };
    let first_turn = match turn_text(&args.first_turn) {
        Ok(text) => text,
        Err(stop) => return refuse_stop(SPAWN, &stop, args.json),
    };
    let home = platform::home_dir();
    // A spawn IS a session started on the host, so a host that does not
    // resolve refuses here, before a name is claimed or a worktree made.
    let wired = match Wired::of(&here, &home, |home| {
        spawn_host(home).map(|host| Box::new(host) as Box<dyn Host>)
    }) {
        Ok(wired) => wired,
        Err(stop) => return refuse_stop(SPAWN, &stop, args.json),
    };
    let permissions = match permissions_of(&here, args.touched.as_deref()) {
        Ok(permissions) => permissions,
        Err(stop) => return refuse_stop(SPAWN, &stop, args.json),
    };
    let machine = wired.machine(&here);

    match transient::spawn(
        &machine,
        &transient::Spawn {
            first_turn: &first_turn,
            model: args.model.as_deref(),
            permissions,
            item: None,
            base: args.base.as_deref(),
        },
        clock::now_ms(),
    ) {
        Ok(spawned) => {
            // Both belt readings on the way out too, because the number a
            // person wants when the next spawn refuses is the one this one saw.
            eprintln!("{}", spawned.belt.lines());
            eprintln!("worktree: {}", spawned.worktree.display());
            if args.json {
                // THE DOCUMENT REPLACES THE NAME on stdout and never joins it:
                // under the flag stdout is one document and nothing else
                // (`envelope.rs`). The belt is its payload rather than its two
                // sentences, for the same reason.
                println!(
                    "{}",
                    envelope::ok(
                        SPAWN,
                        &serde_json::json!({
                            // The seat as its object: the id the spawn
                            // minted, an agent's, and nameless.
                            "seat": SeatRef {
                                id: spawned.id,
                                name: None,
                                kind: Kind::Agent,
                            },
                            "worktree": spawned.worktree.display().to_string(),
                            "belt": spawned.belt.payload(),
                            "base": spawned.base,
                        })
                    )
                );
            } else {
                // The seat's machine name, `agent-<short>`, alone on stdout:
                // what a person reads and types back. `dispatch` assigns the
                // seat's id, which the spawner seam hands it directly.
                println!("{}", spawned.seat);
            }
            Exit::Done
        }
        Err(refusal) => refuse_stop(SPAWN, &refusal, args.json),
    }
}

pub fn feed_command(args: &FeedArgs) -> Exit {
    let here = match resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return refuse_stop(FEED, &stop, args.json),
    };
    let row = match seat_named(&here.machine_dir, &args.seat) {
        Ok(row) => row,
        Err(stop) => return refuse_stop(FEED, &stop, args.json),
    };
    let seat = row.machine_name();
    let first_turn = match turn_text(&args.first_turn) {
        Ok(text) => text,
        Err(stop) => return refuse_stop(FEED, &stop, args.json),
    };
    let home = platform::home_dir();
    let wired = match Wired::of(&here, &home, |home| Ok(verb_host(home))) {
        Ok(wired) => wired,
        Err(stop) => return refuse_stop(FEED, &stop, args.json),
    };
    let machine = wired.machine(&here);

    match transient::feed(&machine, &seat, &first_turn) {
        Ok(fed) => {
            if args.json {
                // THE TURNS ARE THEIR FIRST LINES, which is the shape the
                // controller's own journal carries them in: the whole text is
                // the session table's, and a second copy of it could disagree
                // with the first.
                println!(
                    "{}",
                    envelope::ok(
                        FEED,
                        &serde_json::json!({
                            "seat": row.as_ref(),
                            "prior_first_turn": first_line(&fed.prior),
                            "first_turn": first_line(&fed.next),
                        })
                    )
                );
            } else {
                println!("fed {} — the turn in the seat is now the new one", fed.seat);
            }
            Exit::Done
        }
        Err(refusal) => refuse_stop(FEED, &refusal, args.json),
    }
}

pub fn retire_command(args: &RetireArgs) -> Exit {
    let here = match resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return refuse_stop(RETIRE, &stop, args.json),
    };
    // Who the withdrawal is written by, resolved before anything moves: the
    // retire is somebody's act, and a verb always has an actor.
    let by = match acting(RETIRE, args.by.as_deref(), &here) {
        Ok(by) => by,
        Err(stop) => return refuse_stop(RETIRE, &stop, args.json),
    };
    let row = match seat_named(&here.machine_dir, &args.seat) {
        Ok(row) => row,
        Err(stop) => return refuse_stop(RETIRE, &stop, args.json),
    };
    let seat = row.machine_name();
    let home = platform::home_dir();
    let wired = match Wired::of(&here, &home, |home| Ok(verb_host(home))) {
        Ok(wired) => wired,
        Err(stop) => return refuse_stop(RETIRE, &stop, args.json),
    };
    let machine = wired.machine(&here);

    // THE ITEM THIS SEAT WAS DISPATCHED, and the timeline that answers for it,
    // read BEFORE the retire drops the row that names it. Neither is a reading
    // this verb refuses over: a seat nobody can name an item for retires
    // exactly as it always did and keeps its branch.
    let dispatched = held_item(&here, &row.id);
    let timeline = dispatched
        .as_deref()
        .and_then(|item| timeline_of(&here, item))
        .unwrap_or_default();

    // THE RECORD'S HALF OF THE RETIRE. It runs inside the retire, at the last moment the
    // verb can still stop: the name this frees is the one the next spawn takes,
    // and an order left standing against it is one that seat would inherit.
    let store = match open_store(&here) {
        Ok(store) => store,
        Err(stop) => return refuse_stop(RETIRE, &stop, args.json),
    };
    // THE ROW'S ID, which is what the order was assigned to; the note and the
    // sentences name the seat by its machine name. The retire hands its
    // withdrawal the name it resolved, which is this same row.
    let withdrawal = |seat: &str| {
        withdrawn_from(
            store.as_ref(),
            &row.id,
            &here.seats.label(&row.id),
            &by,
            dispatched.is_none().then_some(seat),
        )
    };

    match transient::retire_with(&machine, &seat, args.dead, &withdrawal) {
        Ok(reclaimed) => {
            if !args.json {
                println!(
                    "retired {} — reclaimed {} from {}{}{}",
                    reclaimed.seat,
                    match reclaimed.bytes {
                        Some(bytes) => format!("{bytes} bytes"),
                        None => "an unmeasured number of bytes".to_string(),
                    },
                    reclaimed.worktree,
                    match reclaimed.pid {
                        Some(pid) => format!(", pid {pid} confirmed gone"),
                        None => ", no live session to stop".to_string(),
                    },
                    if reclaimed.dead {
                        " (--dead: the host held no live session)"
                    } else {
                        ""
                    }
                );
            }
            if let Some(removal) = &reclaimed.removal {
                eprintln!("session: {removal}");
            }
            for item in &reclaimed.withdrawn {
                eprintln!(
                    "{}: {item} — it is open and unassigned",
                    seat::retire::WITHDRAWN
                );
            }
            let branch =
                release_the_branch(&machine, &timeline, reclaimed.branch.as_deref(), args.json);
            if args.json {
                println!(
                    "{}",
                    envelope::ok(
                        RETIRE,
                        &serde_json::json!({
                            "seat": row.as_ref(),
                            "worktree": reclaimed.worktree,
                            "bytes": reclaimed.bytes,
                            "pid": reclaimed.pid,
                            "branch": branch,
                        })
                    )
                );
            }
            Exit::Done
        }
        Err(refusal) => refuse_stop(RETIRE, &refusal, args.json),
    }
}

/// The work branch the landing already classified, deleted now that the
/// worktree holding it is gone.
///
/// IT NEVER TAKES THE EXIT WITH IT, either way. The seat is reclaimed by the
/// time this runs, and a retire reported as failed over a ref would be one a
/// person runs again against a seat that is not there.
fn release_the_branch(
    machine: &Machine,
    timeline: &[Entry],
    held: Option<&str>,
    json: bool,
) -> serde_json::Value {
    match land::release(timeline, held) {
        Release::Delete(branch) => match transient::delete_branch(machine, &branch) {
            Ok(()) => {
                if !json {
                    println!(
                        "work branch {branch}: {} on the landing — deleted",
                        land::SAFE
                    );
                }
                disposition(
                    Some(&branch),
                    "deleted",
                    &format!("{} on the landing", land::SAFE),
                )
            }
            Err(cause) => {
                eprintln!(
                    "work branch {branch}: {} on the landing — kept: {cause}",
                    land::SAFE
                );
                disposition(Some(&branch), "kept", &cause.to_string())
            }
        },
        // A seat that was on no branch at all has nothing to say here; every
        // other keep names the branch and the reading that spared it.
        Release::Keep(why) => {
            if held.is_some() && !json {
                println!("work branch kept — {why}");
            }
            disposition(held, "kept", &why)
        }
    }
}

/// What became of the work branch, as the envelope's reader branches on it: the
/// name, the verdict in one word, and the reading behind it. A seat that stood
/// on no branch has `null` for the name and the same two words for the rest.
fn disposition(branch: Option<&str>, what: &str, why: &str) -> serde_json::Value {
    serde_json::json!({ "name": branch, "disposition": what, "why": why })
}

/// The item this seat's newest session row was dispatched, where one names it.
fn held_item(here: &Here, seat: &SeatId) -> Option<String> {
    sessions::read(&sessions::path_in(&here.machine_dir))
        .0?
        .newest_for(&seat.to_string())?
        .item
        .clone()
}

/// That item's timeline. A store that will not answer reads as no entries,
/// which is the answer that keeps the branch.
fn timeline_of(here: &Here, item: &str) -> Option<Vec<Entry>> {
    open_store(here).ok()?.timeline(&ItemId::from(item)).ok()
}

// ---- the spawner seam -------------------------------------------------------

/// The spawner `dispatch` reaches when it is given no `--to`, which is the one
/// the controller's primitives implement. The row, the worktree, the first
/// turn, the belt and the retire are the controller's; the order note, the
/// brief's content and the assignment stay `dispatch`'s.
///
/// It answers `Refusal` on every path this verb has, including the belt's: a
/// dispatch whose spawn refuses withdraws the order in the same act, so the
/// cause a person reads here is the cause of the withdrawal.
///
/// EVERY STOP ON THESE PATHS ALREADY CARRIES ITS EXIT, and [`outcome_of`] is
/// the one place that reads it, so a leg that learns to tell a question from a
/// verdict is told apart here without a second table.
pub struct TransientSpawner<'a> {
    pub here: &'a Here,
    pub home: PathBuf,
}

impl Spawner for TransientSpawner<'_> {
    fn spawn(&self, ask: &Spawn) -> SpawnOutcome {
        let text = match turn_text(ask.first_turn) {
            Ok(text) => text,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let wired = match Wired::of(self.here, &self.home, |home| {
            spawn_host(home).map(|host| Box::new(host) as Box<dyn Host>)
        }) {
            Ok(wired) => wired,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let permissions = match permissions_of(self.here, ask.touched) {
            Ok(permissions) => permissions,
            Err(stop) => return outcome_of(stop.code, stop.message),
        };
        let machine = wired.machine(self.here);
        match transient::spawn(
            &machine,
            &transient::Spawn {
                first_turn: &text,
                model: ask.model,
                permissions,
                item: Some(ask.item),
                base: ask.base,
            },
            clock::now_ms(),
        ) {
            // The seat's FULL ID, which is what dispatch assigns the item to
            // and writes into the index: the record is keyed by the seat.
            Ok(spawned) => SpawnOutcome::Spawned {
                seat: spawned.id.to_string(),
                // The same two lines `seat spawn` prints, from the same render:
                // the two verbs run one belt and a person reading either reads
                // the same words about it.
                belt: Some(spawned.belt.lines()),
            },
            Err(refusal) => outcome_of(refusal.code, refusal.message),
        }
    }
}

/// One leg's stop, as the outcome dispatch acts on.
///
/// ONLY THE COULD-NOT-TELL EXIT LEAVES THE ORDER STANDING. A usage error is
/// dispatch's own defect — it built a call this process cannot read — and is
/// deterministic, so an order left standing for it would wait on a retry that
/// cannot succeed; it reads as the refusal it is (decision A of that spec).
fn outcome_of(code: u8, message: String) -> SpawnOutcome {
    match code {
        COULD_NOT_TELL => SpawnOutcome::CouldNotTell(message),
        _ => SpawnOutcome::Refused(message),
    }
}

// ---- what only a process knows ----------------------------------------------

/// The project this directory resolves to, with `--project` checked against it.
///
/// `--project` SELECTS NOTHING here: choosing among registered projects is the
/// standalone registry's, which `fleet create` brings. What it does is catch a
/// caller who thinks they are somewhere else, which is exactly the mistake that
/// spawns a seat against the wrong checkout.
pub(crate) fn resolved(project: Option<&str>) -> Result<Here, Stop> {
    let here = resolve_at(None)?;
    if let Some(named) = project {
        if named != here.project.name {
            return Err(Stop::usage(format!(
                "--project {named} names a project this directory does not resolve to — here is \
                 `{}`, at {}",
                here.project.name,
                here.project.root.display()
            )));
        }
    }
    Ok(here)
}

/// The first turn's TEXT. A file that is not there is a usage error and not a
/// refusal: nothing about the fleet is wrong, the call named a path this
/// process cannot read.
fn turn_text(path: &Path) -> Result<String, Stop> {
    std::fs::read_to_string(path).map_err(|e| {
        Stop::usage(format!(
            "the first turn at {} could not be read: {e}",
            path.display()
        ))
    })
}

/// A turn's first line, which is what the envelope carries of it.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// The spawner's exit-to-outcome table, which nothing outside this binary can
/// reach: the cli is a binary crate, so a `cfg(test)` module here is the one
/// route to [`outcome_of`] from an arm.
#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::item::{REFUSED, USAGE};

    /// AC3 of the could-not-tell spec, the half no end-to-end arm can reach: `turn_text` is
    /// the only usage-coded leg the spawner has, and `dispatch` writes the file
    /// it then reads, so a usage error cannot be staged through the binary. The
    /// withdrawal the other half of AC3 names is measured against a REFUSED leg
    /// end-to-end in `fleet/cli/tests/dispatch.rs`.
    #[test]
    fn only_the_could_not_tell_exit_leaves_the_order_standing() {
        assert_eq!(
            outcome_of(USAGE, String::from("the first turn could not be read")),
            SpawnOutcome::Refused(String::from("the first turn could not be read")),
            "a usage error is dispatch's own defect and withdraws the order"
        );
        assert_eq!(
            outcome_of(REFUSED, String::from("the belt is full")),
            SpawnOutcome::Refused(String::from("the belt is full")),
            "a refusal is a verdict"
        );
        assert_eq!(
            outcome_of(
                COULD_NOT_TELL,
                String::from("the agent binary is unresolvable")
            ),
            SpawnOutcome::CouldNotTell(String::from("the agent binary is unresolvable")),
            "a could-not-tell is a question, and the order stands under it"
        );
    }
}
