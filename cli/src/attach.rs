//! `fleet seat attach` — a seat's session, opened in the person's own terminal.
//!
//! ATTACH IS THE HOST'S AND NOT THE AGENT'S (ruling 2). A seat's agent is the
//! pane's own process inside a session on fleet's server, so watching it work
//! is attaching a terminal to that session, whatever agent the pane runs. The
//! verb asks the agent nothing and the adapter has no attach to answer.
//!
//! NO CONTROLLER AND NO PROJECTION. The host holds the sessions across a
//! controller restart (ruling 3), so the verb reads the seat list for the id
//! and the host's own listing for the session, and asks nothing of anything
//! the controller publishes: a seat whose collector is down is still one a
//! person can look at, and the looking is how they find out why.
//!
//! THE ATTACH REPLACES THIS PROCESS. The terminal is the person's and so is how
//! long they stay, so the verb execs the host's attach command in its own place
//! rather than running it under a bound: the host's own refusal — a terminal
//! that is not one, a session gone between the listing and the attach — and its
//! exit reach the person unchanged, and fleet makes no TTY check of its own.
//!
//! A read-only attach writes nothing anywhere. `--write` hands the person's
//! keys to the seat's session, where typing IS the seat acting, so it says so
//! on the terminal and puts one `seat.attached` line on the stream before the
//! exec (reviewer call 2026-09-25 (1) on this row): a keyboard taken over a
//! seat is on the record.

use std::os::unix::process::CommandExt;
use std::path::Path;

use fleet_controller::config;
use fleet_controller::events::{self, ActorRef, EventLog};
use fleet_controller::host::tmux::TmuxHost;
use fleet_controller::host::{session_for, Host, HostRead, PaneState, SOCKET};
use fleet_controller::{clock, platform};

use crate::exit::Exit;

/// The stream the `--write` line goes to, under the machine directory.
const STREAM: &str = "events.jsonl";

/// What `seat attach` takes.
#[derive(clap::Args)]
pub struct AttachArgs {
    /// the seat whose session to open
    pub seat: String,
    /// take the keyboard: typing goes to the seat's session
    #[arg(long)]
    pub write: bool,
    /// the project this directory must resolve to
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
}

pub fn attach_command(args: &AttachArgs) -> Exit {
    // The project first, only so a --project that does not match is refused
    // as a usage error before anything is read: the seat list is the
    // machine's, and nothing below is the project's.
    let here = match crate::transient::resolved(args.project.as_deref()) {
        Ok(here) => here,
        Err(stop) => return stopped(&stop.message, code(stop.code)),
    };
    // The argument through the seat list's resolver, exactly as `seat nudge`
    // takes it: its full id, eight or more of its hex digits, its name or its
    // machine name. Its id names the session; its machine name, every sentence.
    let row = match crate::transient::seat_named(&here.machine_dir, &args.seat) {
        Ok(row) => row,
        Err(stop) => return stopped(&stop.message, code(stop.code)),
    };
    let seat = row.machine_name();
    let host = match TmuxHost::resolve(&platform::child_path(&platform::home_dir())) {
        Ok(host) => host,
        Err(why) => return stopped(&why, Exit::CouldNotTell),
    };
    let session = session_for(&row.id);
    let state = match pane_of(&host, &session, &seat) {
        Ok(state) => state,
        Err((why, exit)) => return stopped(&why, exit),
    };
    if let Some(ended) = ended(state) {
        eprintln!("fleet seat attach: {ended}");
    }
    if args.write {
        // THE LINE BEFORE THE KEYBOARD: a stream that will not take it leaves
        // the keyboard untaken, since a --write nobody can find afterwards is
        // the one thing this line exists to rule out.
        if let Err(why) = record_write(&here.machine_dir, &row) {
            return stopped(&why, Exit::CouldNotTell);
        }
        eprintln!("fleet seat attach: typing into {seat}'s session acts as that seat");
    }
    // `exec` returns only when the program could not be run at all; past it,
    // the process is the host's client, and its exit is this verb's.
    let why = host.attach(&session, args.write).exec();
    stopped(
        &format!("could not run {}: {why}", host.bin().display()),
        Exit::CouldNotTell,
    )
}

/// The seat's pane as the host lists it, or why there is none to attach.
///
/// A session with no pane alive is still attached — remain-on-exit keeps the
/// dead pane's last screen, which is how a person reads why a start failed —
/// so the state is answered and not judged. One with any pane alive reads
/// alive: a seat's session holds one pane, and a second a person opened by
/// hand does not make the seat's agent any less running.
fn pane_of(host: &dyn Host, session: &str, seat: &str) -> Result<PaneState, (String, Exit)> {
    let panes = match host.list() {
        HostRead::Readable(panes) => panes,
        // A listing that failed is not a seat with no session: said as
        // could-not-tell, with the host's own cause.
        HostRead::Unreadable { cause } => {
            return Err((
                format!("the sessions on {SOCKET} could not be listed: {cause}"),
                Exit::CouldNotTell,
            ))
        }
    };
    let states: Vec<PaneState> = panes
        .into_iter()
        .filter(|pane| pane.session == session)
        .map(|pane| pane.state)
        .collect();
    if states.contains(&PaneState::Alive) {
        return Ok(PaneState::Alive);
    }
    states.first().copied().ok_or_else(|| {
        (
            format!("{seat} has no session on {SOCKET}"),
            Exit::NoSession,
        )
    })
}

/// The line a dead pane is announced with before its last screen is shown.
fn ended(state: PaneState) -> Option<String> {
    match state {
        PaneState::Alive => None,
        PaneState::Dead {
            status: Some(status),
        } => Some(format!("the session ended with status {status}")),
        // A process a signal ended carries a signal and no status.
        PaneState::Dead { status: None } => {
            Some("the session ended with no exit status — a signal ended it".to_string())
        }
    }
}

/// The `seat.attached` line a `--write` owes: about the seat, by its id, with
/// the seat's `{id, name?, kind}` object, the mode and the instant.
fn record_write(machine_dir: &Path, row: &config::Seat) -> Result<(), String> {
    let stream = machine_dir.join(STREAM);
    EventLog::open(&stream)
        .append(
            events::SEAT_ATTACHED,
            &ActorRef::seat(row.id),
            serde_json::json!({
                "seat": row.as_ref(),
                "mode": "write",
                "at": clock::now_stamp(),
            }),
        )
        .map_err(|e| {
            format!(
                "could not append to {}, so the keyboard was not taken: {e}",
                stream.display()
            )
        })
}

/// A resolver's status as a row of the table; a status outside it is
/// could-not-tell, never a guessed row.
fn code(status: u8) -> Exit {
    Exit::from_status(status).unwrap_or(Exit::CouldNotTell)
}

fn stopped(message: &str, exit: Exit) -> Exit {
    eprintln!("fleet seat attach: {message}");
    exit
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_controller::test_support::FakeHost;

    const SEAT: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

    fn started(host: &FakeHost, name: &str) {
        host.new_session(name, Path::new("/"), &["/bin/agent".to_string()], &[])
            .expect("the fake starts it");
    }

    /// A live pane is attached with nothing said; a dead one is attached too,
    /// under the line naming its status.
    #[test]
    fn a_live_pane_says_nothing_and_a_dead_one_says_its_status() {
        let host = FakeHost::new();
        started(&host, SEAT);
        let state = pane_of(&host, SEAT, "orla-93b9739a").expect("a session");
        assert_eq!(state, PaneState::Alive);
        assert_eq!(ended(state), None);

        host.end(SEAT, Some(3));
        let state = pane_of(&host, SEAT, "orla-93b9739a").expect("a dead session still is one");
        assert_eq!(
            ended(state).as_deref(),
            Some("the session ended with status 3")
        );
        assert_eq!(
            ended(PaneState::Dead { status: None }).as_deref(),
            Some("the session ended with no exit status — a signal ended it")
        );
    }

    /// No session of the seat's name is NoSession, whatever else the server
    /// holds; a listing that failed is CouldNotTell with its cause, never a
    /// seat with no session.
    #[test]
    fn no_session_is_four_and_an_unreadable_host_is_three() {
        let host = FakeHost::new();
        started(&host, "01a0d1f1-0aec-765f-9abe-000000000000");
        let (why, exit) = pane_of(&host, SEAT, "orla-93b9739a").expect_err("not this seat's");
        assert_eq!(exit, Exit::NoSession);
        assert_eq!(why, "orla-93b9739a has no session on fleet");

        host.fail(FakeHost::LIST, Some("server exploded"));
        let (why, exit) = pane_of(&host, SEAT, "orla-93b9739a").expect_err("unreadable");
        assert_eq!(exit, Exit::CouldNotTell);
        assert!(why.contains("server exploded"), "{why}");
    }
}
