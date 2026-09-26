//! The releases of the tools fleet runs on that this binary supports, in one
//! place: what each was measured against, and so what the defaults' doctor
//! checks and the controller compare the installed one with.
//!
//! TWO ARE DECLARED HERE: Claude Code's, and the fleet-packs repository's —
//! the source and the tag `fleet create` installs a store pack from. A store's
//! own pin is its pack's and not this binary's: the pack at that tag pins the
//! store it drives and carries the doctor check that measures it. The workflow
//! runtime is NOT the binary's either: a pack pins it in its own `[runtime]`
//! table (the ts pack pins Deno), and the defaults' `runtime-version` check
//! measures whichever pack declares one. git carries no pin.
//!
//! Another version is NAMED AND NOT REFUSED, for every pin: the verbs and the
//! controller still run on it, and the doctor check that reports the spread
//! says so beside how to install the supported release: Claude Code's own
//! install command, and for fleet-packs the `fleet pack add` line at the tag.

/// The Claude Code release fleet supports: the one the defaults'
/// `claude-code-version` doctor check compares `claude --version` with, and
/// the one fleet is developed on and its suites run beside. The adapter's
/// version-scoped behaviours were last re-read on an earlier release (lessons
/// claude-code A1 names it), so the spread between the two is a re-measure
/// owed.
///
/// It is also the controller's DEFAULT EXPECTATION. A fleet whose policy pins
/// nothing under `[substrate]` expects this release, so a `substrate.moved`
/// there means "not the release fleet supports"; a fleet's own pin still wins.
///
/// A move is THIS LINE PLUS THE RE-MEASURE: every version-scoped behaviour the
/// adapter reads re-run on the new release and restated, or its code changed
/// where the behaviour moved. The doctor check carries its own copy, because a
/// shell script cannot read this, and a suite arm fails until the two agree.
pub const PINNED_CLAUDE_CODE: &str = "2.1.280";

/// The repository fleet's packs are published from: the store adapters, the
/// runtimes and the tiny pack, one directory each. `fleet create` installs the
/// store pack it is asked for from here, `<this>//adapters/store/<name>`, and a
/// refusal naming a store adapter no installed pack carries names the same
/// line. `fleet create --packs-from <dir>` replaces it for one call, with a
/// checkout of the same repository.
pub const PINNED_PACKS_SOURCE: &str = "https://github.com/bembot90/fleet-packs";

/// The tag of [`PINNED_PACKS_SOURCE`] this release of fleet was checked
/// against: the version `fleet create` installs a store pack at, and the one
/// the defaults' `fleet-packs-version` doctor check compares every pack the
/// lock pins from that source with.
///
/// A move is THIS LINE PLUS THE RE-CHECK: the packs at the new tag run against
/// this binary — `fleet store check --adapter` over each store adapter, and the
/// suites beside `fleet create` — before the line changes. The doctor check
/// carries its own copy of this and of the source, because a shell script
/// cannot read either, and a suite arm fails until the three agree.
pub const PINNED_PACKS: &str = "v0.1.0";
