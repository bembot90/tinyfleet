//! The host seam: where a seat's session RUNS, as distinct from what runs in
//! it.
//!
//! A seat's agent is the pane's own process inside a terminal multiplexer
//! session this controller starts, on a server of its own. Everything the
//! controller asks of that server — start a session, type into it, read its
//! screen, end it, list what is there — goes through one trait, [`Host`], so
//! the in-process suites drive a fake ([`crate::test_support::FakeHost`]) and
//! the one real implementation is [`tmux::TmuxHost`]. That module is the only
//! code in the workspace that spells the multiplexer's name outside prose: an
//! adapter never touches the host (ruling 2), and nothing above this seam
//! learns which program answers it.
//!
//! ONE SERVER PER CONTROLLER (reviewer call 2026-09-25, E1). The controller
//! runs per machine and its seats span projects, so there is one socket,
//! [`SOCKET`], and never the person's own default server. A seat's session is
//! named by the seat's id ([`session_for`]), which survives a rename and holds
//! nothing a target string reads as syntax.
//!
//! THE AGENT IS THE PANE'S OWN PROCESS (E2). A session is started with the
//! agent's argv as its command and the environment set exactly — no shell
//! between the host and the agent — so the pane dies when the agent does, and
//! the host keeps the dead pane with its exit status: that is the presence
//! reading ruling 3 rests on.

use fleet_core::seat::identity::SeatId;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub mod tmux;

pub use tmux::TmuxHost;

/// The socket fleet's server listens on: one per controller, per machine.
pub const SOCKET: &str = "fleet";

/// The pause between a paste and the separate keystroke that submits it.
///
/// A burst of text followed by Enter in the same write did not submit (lessons
/// claude-code D8, on 2.1.282), so the submit is its own call after this gap —
/// herdr's spacing. On the supported 2.1.280 a bracketed paste and a `C-m`
/// this far apart submitted one two-line turn (measured 2026-09-26).
pub const SUBMIT_GAP: Duration = Duration::from_millis(300);

/// A seat's session name: the seat's id, hyphenated.
///
/// The id and not the name, because a name is a person's and free to change
/// while the session outlives it; and the hyphenated id is only `[0-9a-f-]`, so
/// no target the host parses reads any of it as a separator.
pub fn session_for(seat: &SeatId) -> String {
    seat.to_string()
}

/// One pane the host listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    /// The session the pane belongs to, which for a seat is [`session_for`].
    pub session: String,
    /// The pane's process — the agent itself, since nothing sits between. A
    /// dead pane still reports the pid it HAD, so a reader keys presence on
    /// [`Pane::state`] and never on this.
    pub pid: Option<u32>,
    pub state: PaneState,
    /// The pane's working directory as the host reads it off the live process.
    /// Empty on a dead pane (measured on tmux 3.7b): there is no process left
    /// to read it from.
    pub path: String,
    /// When the SESSION was created, in epoch milliseconds. The host counts in
    /// whole seconds, so the last three digits are always zero.
    pub created_ms: Option<u64>,
}

/// Whether the pane's process is still running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneState {
    Alive,
    /// The process exited and the pane was kept (remain-on-exit). `status` is
    /// its exit code, and `None` is a pane with none to report — a process
    /// ended by a signal carries a signal rather than a status.
    Dead {
        status: Option<i32>,
    },
}

/// A whole-server read, or the reason there is none.
///
/// The same distinction [`crate::adapter::RosterRead`] draws: a listing that
/// failed must not reach a seat as "no sessions", because that is every seat
/// reading absent at once. A server that is not running IS readable — it holds
/// no sessions, which is the fleet after a reboot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostRead {
    Readable(Vec<Pane>),
    Unreadable { cause: String },
}

/// Everything the controller asks of the host, each verb about one session by
/// name.
pub trait Host {
    /// Start a detached session running `argv` in `cwd`, with exactly `env` as
    /// its environment and nothing inherited. The argv IS the pane's process
    /// (E2): no shell runs it, so the pane dies with the agent and keeps its
    /// exit status.
    fn new_session(
        &self,
        name: &str,
        cwd: &Path,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<(), String>;

    /// Type `text` into the session as ONE bracketed paste, then submit it with
    /// a separate keystroke [`SUBMIT_GAP`] later (E4). A paste and not keys, so
    /// a newline inside the text is part of the text rather than a submit of
    /// its first line.
    fn send(&self, name: &str, text: &str) -> Result<(), String>;

    /// Press named keys in the session, in order.
    fn keys(&self, name: &str, keys: &[&str]) -> Result<(), String>;

    /// The pane's text: its history and then its screen. A pane that has
    /// died keeps both, under the host's own line saying so.
    fn capture(&self, name: &str) -> Result<String, String>;

    /// End the session. A session already gone is `Ok`: the ask is that it not
    /// be there, and it is not.
    fn kill(&self, name: &str) -> Result<(), String>;

    /// Every pane on the server.
    fn list(&self) -> HostRead;

    /// The command that attaches a person's terminal to the session, read-only
    /// unless `write`. BUILT here and never run or bounded here: the terminal
    /// is the person's, and so is how long they stay.
    fn attach(&self, name: &str, write: bool) -> Command;

    /// The host program's own version, or `None` when it will not say.
    fn version(&self) -> Option<String>;
}

/// The host where none resolved: every verb refuses with the resolution's own
/// cause, and the listing is unreadable with it.
///
/// A caller that could not resolve its host still holds one, so the seams it
/// threads carry a host either way; a loop publishes the same cause as effects
/// off and never reaches a verb, and a verb that does reach one fails naming
/// why there is no host rather than naming a program nobody ran.
#[derive(Clone, Debug)]
pub struct Unresolved {
    pub cause: String,
}

impl Host for Unresolved {
    fn new_session(
        &self,
        _name: &str,
        _cwd: &Path,
        _argv: &[String],
        _env: &[(String, String)],
    ) -> Result<(), String> {
        Err(self.cause.clone())
    }

    fn send(&self, _name: &str, _text: &str) -> Result<(), String> {
        Err(self.cause.clone())
    }

    fn keys(&self, _name: &str, _keys: &[&str]) -> Result<(), String> {
        Err(self.cause.clone())
    }

    fn capture(&self, _name: &str) -> Result<String, String> {
        Err(self.cause.clone())
    }

    fn kill(&self, _name: &str) -> Result<(), String> {
        Err(self.cause.clone())
    }

    fn list(&self) -> HostRead {
        HostRead::Unreadable {
            cause: self.cause.clone(),
        }
    }

    /// A command that fails at once and touches no terminal: there is no
    /// session to attach to.
    fn attach(&self, _name: &str, _write: bool) -> Command {
        Command::new("/usr/bin/false")
    }

    fn version(&self) -> Option<String> {
        None
    }
}

/// The host a process resolves for itself: fleet's own server through the
/// resolved binary, or [`Unresolved`] carrying why there is none.
pub fn resolve(child_path: &str) -> Box<dyn Host> {
    match tmux::TmuxHost::resolve(child_path) {
        Ok(host) => Box::new(host),
        Err(cause) => Box::new(Unresolved { cause }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seat's session is its hyphenated id: the same string the seat list
    /// stores, and nothing in it a target could read as syntax.
    #[test]
    fn a_seats_session_is_its_hyphenated_id() {
        let id = SeatId::parse("0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b").expect("a seat id");
        let name = session_for(&id);
        assert_eq!(name, "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase() || c == '-'),
            "only [0-9a-f-]: {name}"
        );
    }
}
