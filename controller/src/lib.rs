//! The fleet controller.
//!
//! It observes, decides, acts and publishes: it reads policy and the seat list,
//! reads each seat's presence off its host and asks the agent adapter — opened
//! through `adapter::open`, and spoken to in the agent contract's six verbs —
//! what each live seat is doing and how full its context is, folds the seat
//! events it finds in its own stream, produces one verdict per seat, carries
//! them out, and writes the projection, the session table and the event stream.
//!
//! Beside the loop it holds the three TRANSIENT-SEAT PRIMITIVES a pack's
//! `dispatch` calls, each a one-shot entry to the same machinery and none of
//! them touching the work graph:
//!
//! **`fleet seat spawn --first-turn <file>`** runs the load belt before it
//! creates anything — the five-minute load average against a per-cpu ceiling and
//! transient seats mid-turn against a cap, both readings printed whichever leg
//! refuses — then mints a fresh seat id, cuts a worktree from the project's
//! `origin/main`, appends a `transient: true` row to the seat list, starts the
//! session with the file's text as its first turn, and prints the seat's
//! machine name, `agent-<short>`. A start that fails rolls the worktree and the
//! row back and never a branch.
//!
//! **`--base <commit>`** cuts that worktree from a NAMED COMMIT instead of the
//! trunk: a reviewer reads the delivery it was sent to, and a returned builder
//! resumes from the commit that came back. The base is resolved in the primary
//! BEFORE the name is claimed, so one that is not there refuses with nothing
//! made rather than inside the rollback window — and the commit the spawn
//! answers with is read from the new worktree's own HEAD, which is the tree the
//! fact describes rather than the ref the add was given.
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
//! `transient::delete_branch` only where the item's landed entry already
//! classified that same branch SAFE.
//!
//! ## A spawned seat's own configuration directory
//!
//! A spawned seat comes up under ITS OWN configuration directory — one per seat
//! under the machine directory, made empty at the spawn and taken back at the
//! retire — so nothing from the person's home directory reaches a flight, and
//! the pack's overlay is all it holds. The provider's credential knob is set
//! beside it at the value the operator configured, which is what keeps such a
//! seat logged in.
//!
//! THAT DIRECTORY SCOPES WHAT THE AGENT KNOWS (lessons claude-code A11): a
//! session started under one is known to its agent under that directory and no
//! other. So the directory is on the session-table row and every question put
//! to the agent about that session carries it — its reading, its context, the
//! launch and the resume. How the agent reads under it is the adapter's; a
//! seat it could not read is left Unknown and the rest decided.
//!
//! A logged-out session is LIVE, so the host cannot report the failure: the
//! agent's reading carries it, blocked on `logged_out`, and the first sighting
//! that reads so writes one `dispatch.failed` naming the seat and the item. The
//! controller only reports it — what a workflow does about a held item is the
//! workflow's.
//!
//! **The retire's cost** ([`transient::priced`]) is the retire above with five
//! readings taken first — the main-chain context tokens, the turns, the wall
//! time since the dispatch, and the branch and commit the seat's worktree held.
//! It appends `session.retired` carrying all five beside the reclaim
//! `session.stopped` already carries. A context the agent cannot give leaves
//! the cost fields it prices null and the retire still runs, because a seat
//! nobody can price is still a seat to reclaim.

pub mod adapter;
pub mod clock;
pub mod config;
pub mod decide;
pub mod effect;
pub mod events;
pub mod host;
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
