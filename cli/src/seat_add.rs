//! `fleet seat add` — a seat listed in the fleet's own fleet.toml, as one
//! `[seats.<id>]` table.
//!
//! The file is the one the verbs read their policy from, `Here::policy_file`:
//! the embedded fleet's own, or the one a standalone fleet's machine config
//! names. The seat is core's (`fleet_core::seat::identity`) — its id, its kind,
//! the text its table takes and the reader that reads it back — and what is
//! here is only what a process knows: which file, which machine, and the lock
//! the edit is made under.
//!
//! THE TABLE'S TEXT IS APPENDED, never the document re-serialized: a rewrite
//! through a type drops every comment a person wrote and every key the type
//! does not model. The new document is read as a roster before it is written
//! and the written one read again before the verb answers 0, the discipline
//! `fleet_controller::lifecycle::register` is under.
//!
//! A PERSON'S SEAT HAS ONE WRITER, [`list_human`], and `fleet create` lists
//! whoever ran it through the same one.

use std::path::Path;

use fleet_controller::platform;
use fleet_core::item::Stop;
use fleet_core::seat::identity::{self, Kind, Seat, SeatId, SeatRef};

use crate::envelope;
use crate::exit::Exit;
use crate::transient::{resolved, stopped};

/// The verb name the envelope's documents carry, which is also the word the
/// refusal's stderr line names itself by.
const ADD: &str = "seat add";

/// What `seat add` takes.
#[derive(clap::Args)]
pub struct AddArgs {
    /// a seat the controller runs, under a freshly minted id
    #[arg(long, conflicts_with = "human", required_unless_present = "human")]
    pub agent: bool,
    /// this machine's identity, minted where there is none
    #[arg(long)]
    pub human: bool,
    /// what a person calls the seat; the id stays the key
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// the model an agent seat runs on
    //
    // `requires` ALONE DOES NOT REFUSE `--human`: clap waives a required
    // argument that conflicts with one present, and `--agent` conflicts with
    // `--human`. The conflict is what makes `--model m --human` a usage error.
    #[arg(
        long,
        value_name = "MODEL",
        requires = "agent",
        conflicts_with = "human"
    )]
    pub model: Option<String>,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// print the envelope document instead of the id
    #[arg(long)]
    pub json: bool,
}

/// What an add did, for the lines and the document that say so.
struct Added {
    seat: SeatRef,
    file: String,
    /// The identity file THIS call minted, where it minted one.
    minted: Option<String>,
    /// The directory the agent's worktree goes in, for the next-step line.
    worktrees: Option<String>,
}

pub fn command(args: &AddArgs) -> Exit {
    let added = match add(args) {
        Ok(added) => added,
        Err(stop) => return stopped(ADD, &stop, args.json),
    };
    let seat = &added.seat;
    eprintln!(
        "added: {} {} — [seats.{}] in {}",
        seat.kind.as_str(),
        seat.machine_name(),
        seat.id,
        added.file
    );
    if let Some(minted) = &added.minted {
        eprintln!("identity: minted at {minted}");
    }
    if let Some(worktrees) = &added.worktrees {
        eprintln!(
            "next: git worktree add {worktrees}/{} <a branch>, then fleet start renders it",
            seat.machine_name()
        );
    }
    if args.json {
        // THE DOCUMENT REPLACES THE ID on stdout and never joins it, as it does
        // for every verb under the flag (`envelope.rs`).
        let mut listed = serde_json::json!({
            "id": seat.id.to_string(),
            "kind": seat.kind.as_str(),
        });
        if let Some(name) = &seat.name {
            listed["name"] = serde_json::Value::String(name.clone());
        }
        println!(
            "{}",
            envelope::ok(
                ADD,
                &serde_json::json!({
                    "seat": listed,
                    "file": added.file,
                    "minted_identity": added.minted.is_some(),
                })
            )
        );
    } else {
        // The full id, alone, on stdout: it is what a script assigns to.
        println!("{}", seat.id);
    }
    Exit::Done
}

fn add(args: &AddArgs) -> Result<Added, Stop> {
    let here = resolved(args.project.as_deref())?;
    let name = match &args.name {
        Some(given) => Some(named(given)?),
        None => None,
    };
    let path = &here.policy_file;
    let file = path.display().to_string();

    if args.human {
        return match list_human(path, &here.machine_dir, name)? {
            Listed::Added { seat, minted } => Ok(Added {
                seat,
                file,
                minted: minted.then(|| {
                    here.machine_dir
                        .join(identity::IDENTITY)
                        .display()
                        .to_string()
                }),
                worktrees: None,
            }),
            // [ASSUMES D11] Asked for by name, a listing that is already there
            // is refused: this verb's one act is the table it did not write.
            Listed::AlreadyListed { seat } => Err(Stop::refused(format!(
                "this machine's identity {} ({}) is already [seats.{}] in {file}",
                seat.machine_name(),
                seat.id,
                seat.id
            ))),
        };
    }

    // A blank is no value, exactly as the roster reads one: a `model = ""`
    // written here would read back as no model, and the read-back would refuse
    // the row it had just written.
    let model = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());
    // Asked before the lock, so a `[project] worktrees` the verb cannot read is
    // refused while nothing is written, rather than after the seat is listed.
    let worktrees = here.worktrees_dir()?.display().to_string();

    let _held = platform::lock_beside(path).map_err(Stop::could_not_tell)?;
    let (body, seats) = read_roster(path)?;
    if let Some(name) = &name {
        not_held(&seats, name)?;
    }
    let seat = SeatRef {
        id: SeatId::mint(),
        name,
        kind: Kind::Agent,
    };
    append(path, body, &seat, model)?;

    Ok(Added {
        seat,
        file,
        minted: None,
        worktrees: Some(worktrees),
    })
}

/// What listing this machine's person in a fleet's file came to.
pub(crate) enum Listed {
    /// The table was appended and read back. `minted` is true where THIS call
    /// minted the machine's identity.
    Added { seat: SeatRef, minted: bool },
    /// The identity already has a table there — this one, as the file states
    /// it — and the file is left as it was.
    AlreadyListed { seat: SeatRef },
}

/// This machine's identity listed in `policy_file` as a human seat, with
/// `identity.toml` minted under `machine_dir` where there is none.
///
/// THE ONE WRITER A PERSON'S SEAT HAS. `fleet seat add --human` calls it and
/// refuses [`Listed::AlreadyListed`]; `fleet create` calls it for whoever ran
/// it, with no name, and says the listing and moves on. The row's name is
/// `name` where one is given, else the identity's own, and `identity.toml` is
/// never written a name.
pub(crate) fn list_human(
    policy_file: &Path,
    machine_dir: &Path,
    name: Option<String>,
) -> Result<Listed, Stop> {
    let _held = platform::lock_beside(policy_file).map_err(Stop::could_not_tell)?;
    let (body, seats) = read_roster(policy_file)?;
    if let Some(name) = &name {
        not_held(&seats, name)?;
    }
    let (mine, minted) = identity::identity_or_mint(machine_dir).map_err(Stop::could_not_tell)?;
    // [ASSUMES D11] A person is listed once: a second listing of the same id
    // would be a second table under one key, which is not TOML.
    if let Some(listed) = seats.iter().find(|s| s.seat.id == mine.id) {
        return Ok(Listed::AlreadyListed {
            seat: listed.seat.clone(),
        });
    }
    let name = match name {
        Some(given) => Some(given),
        None => {
            if let Some(own) = &mine.name {
                not_held(&seats, own)?;
            }
            mine.name
        }
    };
    let seat = SeatRef {
        id: mine.id,
        name,
        kind: Kind::Human,
    };
    append(policy_file, body, &seat, None)?;
    Ok(Listed::Added { seat, minted })
}

/// The file's text and the seats it lists.
///
/// READ UNDER THE LOCK the caller holds (`platform::lock_beside`), because an
/// add is a read-modify-write and the atomic rename under it is not: two adds
/// that each read the same file would leave one row.
fn read_roster(path: &Path) -> Result<(String, Vec<Seat>), Stop> {
    let file = path.display();
    let body =
        std::fs::read_to_string(path).map_err(|e| Stop::could_not_tell(format!("{file}: {e}")))?;
    let seats =
        identity::roster_in(&body).map_err(|why| Stop::could_not_tell(format!("{file}: {why}")))?;
    Ok((body, seats))
}

/// `seat`'s table appended to `body`, written over `path` and read back, under
/// the same lock [`read_roster`] was read under.
fn append(path: &Path, body: String, seat: &SeatRef, model: Option<&str>) -> Result<(), Stop> {
    let file = path.display();
    let mut whole = body;
    if !whole.is_empty() && !whole.ends_with('\n') {
        whole.push('\n');
    }
    whole.push_str(&identity::seat_table(seat, model));
    // Read as a roster BEFORE it is written: a file this verb made unparsable is
    // a fleet that cannot start again.
    identity::roster_in(&whole)
        .map_err(|e| Stop::could_not_tell(format!("{file} would not parse after the edit: {e}")))?;
    platform::write_atomic(path, whole.as_bytes())
        .map_err(|e| Stop::could_not_tell(format!("{file}: {e}")))?;

    // The read-back this write owes its own record, from the file and not from
    // the text this call built.
    let back = std::fs::read_to_string(path)
        .ok()
        .and_then(|body| identity::roster_in(&body).ok())
        .is_some_and(|seats| {
            seats
                .iter()
                .any(|s| s.seat == *seat && s.model.as_deref() == model)
        });
    if !back {
        return Err(Stop::could_not_tell(format!(
            "{file} was written and does not read [seats.{}] back",
            seat.id
        )));
    }
    Ok(())
}

/// The name, trimmed, or the usage error that says why it cannot be one.
///
/// [ASSUMES D10] A NAME MAY NOT READ AS AN ID. The resolver reads eight or
/// more hex digits as a piece of an id and a `-<8 hex>` tail as a machine
/// name, beside reading the argument as a name, so a seat named either way
/// would answer for somebody else's id as well as its own.
fn named(given: &str) -> Result<String, Stop> {
    let name = given.trim();
    if name.is_empty() {
        return Err(Stop::usage("--name is empty"));
    }
    if name.len() >= 8 && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(Stop::usage(format!(
            "--name {name} reads as a seat id — pick a name that is not 8 or more hex digits"
        )));
    }
    let machine_tail = name
        .rsplit_once('-')
        .is_some_and(|(_, tail)| tail.len() == 8 && tail.chars().all(|c| c.is_ascii_hexdigit()));
    if machine_tail {
        return Err(Stop::usage(format!(
            "--name {name} ends the way a seat's machine name does (-<8 hex>) — pick another"
        )));
    }
    Ok(name.to_string())
}

/// [ASSUMES D10] Two seats cannot answer to one name: the resolver would
/// refuse either as ambiguous, so the second is refused here instead.
fn not_held(seats: &[Seat], name: &str) -> Result<(), Stop> {
    match seats.iter().find(|s| {
        s.seat
            .name
            .as_deref()
            .is_some_and(|held| held.eq_ignore_ascii_case(name))
    }) {
        Some(holder) => Err(Stop::refused(format!(
            "{name} already names {} ({}) — two seats cannot answer to one name",
            holder.seat.machine_name(),
            holder.seat.id
        ))),
        None => Ok(()),
    }
}
