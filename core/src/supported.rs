//! The releases of the tools fleet runs on that this binary supports, in one
//! place: what each was measured against, and so what the defaults' doctor
//! checks and the controller compare the installed one with.
//!
//! ONE IS DECLARED HERE: the fleet-packs repository's — the source and the
//! tag `fleet create` installs the agent's pack and a store pack from —
//! beside tmux's MINIMUM, a floor rather than a pin. An agent's pin is its
//! pack's and not this binary's, as a store's is: the pack at that tag pins
//! the program it drives, declares the releases its adapter was measured
//! against, and carries the doctor check that measures it (ruling 8). The
//! workflow runtime is NOT the binary's either: a pack pins it in its own
//! `[runtime]` table (the ts pack pins Deno), and the defaults'
//! `runtime-version` check measures whichever pack declares one. git carries
//! no pin.
//!
//! Another version is NAMED AND NOT REFUSED: the verbs and the controller
//! still run on it, and the doctor check that reports the spread says so
//! beside the `fleet pack add` line at the tag.

/// The oldest tmux fleet runs on: every seat's session is a tmux session on
/// fleet's own socket, and the controller's host reads each pane's liveness,
/// exit status and screen through it. The defaults' `tmux-version` doctor
/// check compares `tmux -V` with it.
///
/// A FLOOR, not a pin: a later release is expected to keep the few client
/// calls the host makes. It is the one release measured (3.7b, 2026-09-26),
/// and it moves down only when an older one is measured the same way. The
/// doctor check carries its own copy, because a shell script cannot read this,
/// and a suite arm fails until the two agree.
pub const MINIMUM_TMUX: &str = "3.7b";

/// The repository fleet's packs are published from: the store adapters, the
/// runtimes, the agent adapters and the tiny pack, one directory each. `fleet
/// create` installs the agent's pack and the store pack it is asked for from
/// here, `<this>//adapters/agent/<name>` and `<this>//adapters/store/<name>`,
/// and a refusal naming an adapter no installed pack carries names the same
/// line. `fleet create --packs-from <dir>` replaces it for one call, with a
/// checkout of the same repository.
pub const PINNED_PACKS_SOURCE: &str = "https://github.com/bembot90/fleet-packs";

/// The tag of [`PINNED_PACKS_SOURCE`] this release of fleet was checked
/// against: the version `fleet create` installs the agent's pack and a store
/// pack at, and the one
/// the defaults' `fleet-packs-version` doctor check compares every pack the
/// lock pins from that source with.
///
/// A move is THIS LINE PLUS THE RE-CHECK: the packs at the new tag run against
/// this binary — `fleet store check --adapter` over each store adapter, `fleet
/// agent check --adapter` over each agent adapter, and the suites beside `fleet
/// create` — before the line changes. v0.2.0 is the first tag that carries the
/// claude-code pack. The doctor check
/// carries its own copy of this and of the source, because a shell script
/// cannot read either, and a suite arm fails until the three agree.
pub const PINNED_PACKS: &str = "v0.2.0";
