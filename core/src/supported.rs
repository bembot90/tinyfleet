//! The releases of the tools fleet runs on that this binary supports, in one
//! place: what each was measured against, and so what the defaults' doctor
//! checks and the controller compare the installed one with.
//!
//! TWO ARE THE BINARY'S, and both are here. bd's is the store's own constant,
//! named again below rather than copied, because every "measured on" claim it
//! rests on sits in that file beside it. Claude Code's is declared here. The
//! workflow runtime is NOT the binary's: a pack pins it in its own `[runtime]`
//! table (the ts pack pins Deno), and the defaults' `runtime-version` check
//! measures whichever pack declares one. git carries no pin at all.
//!
//! Another version is NAMED AND NOT REFUSED, for every tool here: the verbs and
//! the controller still run on it, and what reports the spread says so beside
//! the line that installs the supported release.

/// The bd release the store adapter was measured against, and the one the
/// defaults' `bd-version` doctor check and `fleet prime`'s second line compare
/// `bd version` with. Declared in the store, whose claims it dates.
pub use crate::store::PINNED_BD;

/// The Claude Code release the controller's adapter was last measured against:
/// the one its behaviours were re-read on (lessons claude-code A1), and the one
/// the defaults' `claude-code-version` doctor check compares `claude --version`
/// with.
///
/// It is also the controller's DEFAULT EXPECTATION. A fleet whose policy pins
/// nothing under `[substrate]` expects this release, so a `substrate.moved`
/// there means "not the release fleet supports"; a fleet's own pin still wins.
///
/// A move is THIS LINE PLUS THE RE-MEASURE: every version-scoped behaviour the
/// adapter reads re-run on the new release and restated, or its code changed
/// where the behaviour moved. The doctor check carries its own copy, because a
/// shell script cannot read this, and a suite arm fails until the two agree.
pub const PINNED_CLAUDE_CODE: &str = "2.1.261";
