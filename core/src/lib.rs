//! Pack-and-project work: the verbs that never read the process table.
//!
//! Nothing here may depend on the controller — the cli crate's workspace test
//! refuses that edge.
//!
//! `dispatch` gives a ready item to a seat. It writes the three parts of an
//! order — the assignee, the note the pack's template renders, and the
//! machine-read index — reads all three back against the arguments it was
//! given, renders the brief to a file, and then either rings a named seat or
//! asks the spawner for a transient one. A spawn that refuses withdraws the
//! order in the same act, so no refusal leaves an ordered item nobody holds.
//!
//! `brief` renders the first thing a dispatched seat reads: the item, its order
//! note, the delivery-note template it will fill, the guards in force, the
//! builder's gate its dispatch was handed and the every-turn rules. Whole or not at all — every value
//! is resolved into memory before a byte reaches stdout, because a seat that
//! read half a contract cannot tell it read half.
//!
//! `deliver` is the seat's handoff and the only writer of a delivery. It
//! refuses first — the trunk, a file left outside the staged set, an empty
//! index, a fleet naming no reviewer, a note the grammar cannot anchor on — and
//! only then commits, fills the three lines a process knows into the note the
//! seat wrote, reassigns to the reviewer, reads both back and rings. A ring
//! nobody answers is still exit 0: the reassignment recorded the handoff.
//!
//! `gate` is the pair that needs a person: `ask` and `answer`. `ask` is the
//! seat's, run from inside its worktree, and it is how work stops on a question
//! without anybody waiting for one — everything the tree holds committed on the
//! work branch, the store's own gate raised carrying the question and its
//! lettered options, the park written on the item with the branch, the commit
//! and the gate, and `item.parked` on the stream. It commits EVERYTHING, staged
//! or not, tracked or not, because a question asked mid-work must lose nothing
//! and the seat is about to be retired; a tree with nothing to commit parks on
//! HEAD. It refuses the trunk, a worktree holding no ordered item and a note
//! the question grammar does not read, all three before the commit. `answer` is
//! a person's reply and the one act that empties the store's gate list: the
//! answer written on the item, the gate resolved and `gate.resolved` written,
//! each read back. A letter the options do not name is still an answer with
//! `--text`, because the person deciding may see a third way the seat did not.
//! Neither verb rings anybody and neither dispatches: what resumes a parked
//! item is whatever dispatches it next.
//!
//! `review` is the reviewer's read and the verdict it writes. It prints a size
//! line that is a measurement and carries no tier — core is one reviewer's read
//! at every size — walks the calls the delivery numbered, and writes ACCEPTED
//! or RETURNED WITH FINDINGS from the pack's verdict grammar. It never lands:
//! the accept verdict is what the landing verb reads.
//!
//! `land` is the reviewer's, and it lands a REVIEW: the licence to squash
//! anything is the item's last verdict, an ACCEPTED naming the exact commit it
//! was given. It takes a commit and never a branch name, runs from a linked
//! worktree and never the primary, squashes onto the trunk, runs the one test
//! command it is handed (or lands on the review alone, NOT TESTED, where it is
//! handed none), counts
//! the trunk's distance in the same act as the push, and reads the landed sha
//! off the push's own range line — never off a rev-parse, which answers the tip
//! a push that did nothing leaves behind. Every gate row reaches stdout as it is
//! read and the note renders the same rows again. A refusal before the push puts
//! the checkout back; nothing is put back after it.
//!
//! `lane` is the queue every landing joins, and `land` is what takes it. A
//! project has one lane: a worktree the fleet owns under the machine directory
//! at `[core.flight] lanes`, cut from the trunk on first use and kept between
//! landings, so its build cache is its own by construction and no person's
//! checkout is ever touched by a flight. Beside it — never inside it, where a
//! landing's own tree gate would meet it — sits one lock file, the standard
//! library's file lock and no crate, blocking with no deadline because a
//! landing is bounded by its own suite and not by a timeout this layer could
//! pick. `land` takes it before its first fetch and holds it until it returns,
//! whatever the exit, so the tick's landings and a person's `fleet land` queue
//! on one object; a landing that finds it held prints the holder and the stamp
//! it wrote there and waits. The module also carries the one wait a gate's
//! rerun takes — the box's five-minute load against the belt's ceiling, read
//! through a seam the caller fills, bounded by `[core.flight]
//! rerun_wait_seconds`, whose expiry reruns anyway and says so on the row.
//!
//! `run` is the whole run lifecycle over a workflow a pack carries: the
//! resolution through the layers, the run directory with every input pinned and
//! hashed onto the record, the pack's bundle and run commands, and the exit
//! table its process answers on.
//!
//! `seat` is the record half of the verbs that end a seat, and `retire` is
//! the one it carries: the orders the seat still holds are withdrawn — the
//! assignee cleared, the order index unset, one note saying so, both writes
//! read back — before the seat list gives its name away. The name is the
//! reason: the next spawn takes the lowest free transient one, and an order
//! left standing against a retired seat is inherited by whoever gets that
//! name next. The session, the worktree and the row are the controller's,
//! and the cli is where the two halves meet.
//!
//! `guard` is the four classes a pre-tool hook judges through, and it is the one
//! module here that no verb calls: `shell-trap` and `record` are core's, because
//! every seat has a shell and every fleet has a record, while `release-ref` and
//! `production-write` are compiled in beside them and belong to a PACK — the
//! class lives here, the wiring and the targets are the pack's and the project's
//! (packs PRD § The guards). Every one of them is a pure function from a
//! command's text and a resolved policy to a verdict, so a second agent runtime
//! adds a reader of its own payload in the caller and reuses all four unchanged.

pub mod add;
pub mod defaults;
pub mod digest;
pub mod embedded;
pub mod guard;
pub mod item;
pub mod lock;
mod os_litter;
pub mod pack;
pub mod policy;
pub mod registry;
pub mod remove;
pub mod resolve;
pub mod seat;
pub mod settings;
pub mod store;

/// What a suite drives these verbs with — the applying fake store and the board
/// over it. Compiled into this crate's own tests always, and into the library
/// only under `test-support`, which a dependent's DEV-dependency turns on.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
