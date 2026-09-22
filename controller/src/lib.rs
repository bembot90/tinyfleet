//! The fleet controller.
//!
//! It observes, decides, acts and publishes: it reads policy and the seat list,
//! lists sessions through the agent adapter, matches rows to seats by working
//! directory, reads context from the transcripts, folds the seat events it finds
//! in its own stream, produces one verdict per seat, carries three of them out,
//! and writes the projection, the session table and the event stream.
//!
//! Beside the loop it holds the three TRANSIENT-SEAT PRIMITIVES a pack's
//! `dispatch` calls (PRD R30–R32), each a one-shot entry to the same machinery
//! and none of them touching the work graph:
//!
//! **`fleet seat spawn --first-turn <file>`** runs the load belt before it
//! creates anything — the five-minute load average against a per-cpu ceiling and
//! transient seats mid-turn against a cap, both readings printed whichever leg
//! refuses — then cuts a worktree from the project's `origin/main`, appends a
//! `transient: true` row to the seat list, starts the session with the file's
//! text as its first turn, and prints the seat's name. A start that fails rolls
//! the worktree and the row back and never a branch.
//!
//! **`--base <commit>`** cuts that worktree from a NAMED COMMIT instead of the
//! trunk (flights PRD R11, R14): a reviewer reads the delivery it was sent to,
//! and a returned builder resumes from the commit that came back. The base is
//! resolved in the primary BEFORE the name is claimed, so one that is not there
//! refuses with nothing made rather than inside the rollback window — and the
//! commit the spawn answers with is read from the new worktree's own HEAD, which
//! is the tree the fact describes rather than the ref the add was given.
//!
//! **`fleet seat feed <seat> --first-turn <file>`** moves a live transient
//! seat's occupant marker — the session-table row's first turn — to the new
//! text, delivers the turn through the adapter, and journals the move on the
//! stream. A delivery the adapter reports failed puts the marker back and says
//! so in a second line.
//!
//! **`fleet seat retire <seat> [--dead]`** stops the session, removes it, takes
//! the worktree and the two rows, and then verifies from OUTSIDE that nothing of
//! the seat holds RAM or disk: the roster, the filesystem and git's own worktree
//! list, and the pid it read before the stop. It prints the reclaim and writes
//! `session.stopped`. It deletes no branch of its own accord: it answers which
//! one that worktree stood on, and the cli releases it through
//! `transient::delete_branch` only where the item's landing note already
//! classified that same branch SAFE.
//!
//! ## A spawned seat's own configuration directory
//!
//! A spawned seat comes up under ITS OWN configuration directory — one per seat
//! under the machine directory, made empty at the spawn and taken back at the
//! retire — so nothing from the person's home directory reaches a flight
//! (flights PRD R13, Q1e). The provider's credential knob is set beside it at
//! the value the operator configured, which is what keeps such a seat logged in.
//!
//! THAT DIRECTORY SCOPES THE PROVIDER'S DAEMON, and a per-seat daemon holds a
//! per-seat roster (lessons claude-code A11): the fleet's listing does not name
//! a session started under one. So the directory is on the session-table row and
//! every act about that session is made under it — the listing, the transcript,
//! the end stamp, the stop, the removal, the revive and the nudge. A poll reads
//! one listing per distinct directory and decides each seat against the listing
//! that could see it; a per-row listing nobody could read leaves that row Unknown
//! and the rest decided.
//!
//! A logged-out session is LIVE in that listing, so the roster cannot report the
//! failure: the transcript is the only surface that carries it, and the first
//! sighting whose transcript reads logged out writes one `dispatch.failed`
//! naming the seat and the item. The controller only reports it — what a
//! workflow does about a held item is the workflow's.
//!
//! **The retire's cost** ([`transient::priced`], flights PRD R10, R12) is the
//! retire above with five readings taken first — the main-chain context tokens,
//! the turns, the wall time since the dispatch, and the branch and commit the
//! seat's worktree held. It appends `session.retired` carrying all five beside
//! the reclaim `session.stopped` already carries. A transcript that will not
//! open leaves the three cost fields null and the retire still runs, because a
//! seat nobody can price is still a seat to reclaim.

pub mod adapter;
pub mod clock;
pub mod config;
pub mod decide;
pub mod effect;
pub mod events;
pub mod lifecycle;
pub mod observe;
pub mod platform;
pub mod policy;
pub mod projection;
pub mod routines;
pub mod run;
pub mod runs;
pub mod seat;
pub mod sessions;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod transient;
