//! `fleet` — one binary for the fleet, with one arm per family in the dispatch
//! below and one function per family in its own module.
//!
//! `fleet status` is the reading verb of the set: it prints the projection the
//! controller published — the roster with each seat's verdict and outcome, the
//! grant, the halt latches, each seat's context against the rest threshold,
//! and the `[[core.flight.rules]]` table of the policy in force. It reads the
//! published document and the policy, never the process table, and it writes
//! nothing at all.

mod agent;
mod attach;
mod doctor;
mod envelope;
mod exit;
mod item;
mod item_show;
mod lifecycle;
mod nudge;
mod prime;
mod routines;
mod runs;
mod seat_add;
mod status;
mod step;
mod store;
mod stream;
mod transient;
mod ui;

use anyhow::{Context, Result};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use exit::Exit;
use fleet_controller::adapter::claude_code;
use fleet_controller::events;
use fleet_controller::runs::Runs as RunsSeam;
use fleet_controller::{clock, config, platform, run, seat};
use fleet_core::guard::hook::HookMap;
use fleet_core::guard::{self, Class, Policy, Verdict};
use fleet_core::{add, defaults, lock, pack, remove, resolve};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use ui::{Stream, Tone, Ui};

/// The dispatch: one arm per family, each arm a call into the family's own
/// function. Nothing here parses; clap has already done that.
#[derive(Parser)]
#[command(
    name = "fleet",
    about = "one binary for the fleet: the controller, the seats, the packs",
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// print the controller version and exit
    //
    // By hand rather than clap's own flag: the contract is the bare version and
    // nothing else on stdout, and clap's prints the binary's name beside it.
    #[arg(long = "version", short = 'V', action = ArgAction::SetTrue)]
    version: bool,

    #[command(subcommand)]
    family: Option<Family>,
}

#[derive(Subcommand)]
enum Family {
    // The lifecycle family, in the order a person meets it. There is no
    // `install` verb: its work runs on the first `start`, in one function a
    // later installer script calls.
    /// write this project's fleet and materialize its defaults
    #[command(long_about = "\
write this project's fleet, from inside the project. It asks embedded or
standalone, which agent and which store, installs the store's pack from the
fleet-packs source and tag this binary pins (--packs-from names a checkout
instead) and pins it, writes the file for the mode, materializes the defaults
this binary carries and pins them, and lists you as the fleet's first seat, a
human one — nobody is asked for a name. --store none installs no pack and
prints the line that installs one later; with no terminal and no --store, the
store is the bd pack.

It never initialises and never rewrites the project's work-graph store.")]
    Create(lifecycle::CreateArgs),

    /// install on first run, then load the controller's service
    #[command(long_about = "\
bring the controller up. On the first run on a machine it does what an install
verb would have — the machine directory, the seat list, the seats rendered
into it, the service file, the telemetry disclosure — and STARTS NOTHING.
Then it loads the service and confirms by reading a fresh controller.started
off the stream, never by the load command's own exit.")]
    Start(lifecycle::StartArgs),

    /// unload the controller's service
    #[command(long_about = "\
bring the controller down: the service unloaded, confirmed by a fresh
controller.stopped read back off the stream. Seats are not touched — a stopped
controller leaves their sessions running.")]
    Stop,

    /// poll the fleet and publish the projection and the event stream
    Observe {
        /// one poll, then exit
        #[arg(long)]
        once: bool,
    },

    /// what is done to a seat
    #[command(long_about = "\
what is done to a seat, by a person, an order or the controller's own effects.

The four lifecycle writers are not here: a seat has a voice and no hands, so
woke, rest, handed-off and exited are said under `fleet event`.")]
    Seat {
        #[command(subcommand)]
        verb: SeatVerb,
    },

    /// what a seat says, written to the fleet's stream
    Event {
        #[command(subcommand)]
        verb: EventVerb,
    },

    /// the packs installed on this machine
    Pack {
        #[command(subcommand)]
        verb: PackVerb,
    },

    /// the routines this fleet has loaded, and their firing
    Routine {
        #[command(subcommand)]
        verb: routines::Verb,
    },

    /// read items: one with its fields and its timeline, or a list of them
    Item {
        #[command(subcommand)]
        verb: item_show::Verb,
    },

    /// the work-graph store this project reads and writes
    Store {
        #[command(subcommand)]
        verb: store::Verb,
    },

    /// the agent a seat runs, and the adapter it is reached through
    Agent {
        #[command(subcommand)]
        verb: agent::Verb,
    },

    // THE OLD SPELLING OF THE FAMILY, kept for one release and refused: a
    // script still typing `fleet order` reads exit 2 and the one rewrite that
    // helps, never a sentence about the flag. The words are never read.
    #[command(hide = true, disable_help_flag = true)]
    Order(OldWords),

    // The item verbs are TOP-LEVEL and take no noun: they are what a person
    // and a seat type most, so they are the shortest to type. Each is still one
    // arm here and one function in the family's own module.
    /// give a ready item to a seat
    #[command(long_about = "\
give a ready item to a seat: the ordered entry on the item, its fleet.orders
index and the assignee, then the brief and the ring. Without --to, a transient
seat is asked for.")]
    Dispatch(item::DispatchArgs),

    /// render the first turn a dispatched seat reads
    #[command(long_about = "\
render the first turn a dispatched seat reads, whole or not at all, from the
resolved pack layers.")]
    Brief(item::BriefArgs),

    /// hand the work over: commit, delivery, reassign, ring
    #[command(long_about = "\
hand the work over from inside a seat's worktree: the staged set committed on
the work branch, the delivery --delivery names written on the item with the
commit, the branch and the base filled in, the item reassigned to the reviewer
policy names, and the ring.

The delivery is a JSON file of the shape assets/delivery.schema.json, which
the brief shows. It refuses the trunk, a file left outside the staged set and
an empty one, a delivery that does not match its shape, and an --item the
acting seat does not hold, all before anything is committed.")]
    Deliver(item::DeliverArgs),

    /// raise a hold: ask a person a question, and hold the item on it
    #[command(long_about = "\
raise a hold from inside a seat's worktree, asking a person the question
--question names: everything the tree holds is committed on the work branch,
the store's own hold is raised carrying the question and its lettered options,
the park is written on the item with the branch, the commit and the hold, and
its `item.entry` signal reaches the stream.

The question is a JSON file of the shape assets/question.schema.json, which
the brief shows.

It rings nobody and dispatches nothing. The flight retires the seat, the item
leaves the ready set until the hold is cleared, and the next flight that lists
it cuts a fresh seat from the held commit with the question and the answer in
its brief.

It refuses the trunk, a worktree holding no ordered item, and a question that
does not match its shape — all three before anything is committed.")]
    Hold(item::HoldArgs),

    /// give a hold its clearance: answer the question an item is held on
    #[command(long_about = "\
give a hold its clearance, answering the question an item is held on: the
answer written on the item, the store's hold cleared and its `item.entry`
signal on the stream, one act.

The letter names one of the question's options; `--text` says what was decided
where the options did not carry it, and is what makes a letter outside them an
answer rather than a typo. The item is ready again and nothing is dispatched:
the next flight that lists it is what resumes the work.")]
    Clear(item::ClearArgs),

    // THE OLD SPELLINGS OF THE PAIR, refused: a seat or a script still typing
    // `fleet ask` or `fleet answer` reads exit 2 and the one rewrite that
    // helps, never a sentence about a flag. The words are never read.
    #[command(hide = true, disable_help_flag = true)]
    Ask(OldWords),
    #[command(hide = true, disable_help_flag = true)]
    Answer(OldWords),

    /// read a delivery and write the verdict
    #[command(long_about = "\
read a delivery and write the verdict: the size line and the delivered entry on
--show; the accepted reviewed entry, every decision walked, on --land; the
returned reviewed entry with its findings on --return <file>, which reassigns
the item to the seat the order named and rings them.

The findings file is JSON of the shape assets/findings.schema.json, one entry
per finding, and the verdict keeps them in that order. A file that does not
read is refused with exit 2, before anything is written.

A verdict is the holder's: --land and --return refuse a seat that does not hold
the item, and a run writes one as the [core] reviewer.

--land lands nothing. It appends the accept that `fleet land` reads.")]
    Review(item::ReviewArgs),

    /// squash a reviewed commit onto the trunk and close the item
    #[command(long_about = "\
squash a reviewed commit onto the trunk and close the item, from the reviewer's
own worktree. It takes a commit and never a branch name. Its checks are the
last verdict, an accept of that commit by the landing's own reviewer, a staged
set equal to the delivery's, the --test command it is handed, run on the land
branch before the push, and a trunk nobody has moved — the last of those in the
same act as the push, so the two cannot race.

Every check row is printed as it is read, and the landed sha is taken from the
push's own range line and from nowhere else. A landing handed no --test runs
nothing, lands on the review alone, and says NOT TESTED on its note.")]
    Land(item::LandArgs),

    /// run a workflow: resolve it, pin the inputs, bundle, run, record
    #[command(long_about = "\
run a workflow. The name resolves to workflows/<name>.* through the pack
layers, overlay first; the pack that carries it, or else the one pack it
imports that has one, declares the [runtime] table that says how to bundle
and run it.

It refuses before anything is written: no such workflow, no [runtime] table
in the pack or its imports, two imports each declaring one, a red runtime
doctor, and the open runs at [core.run] max_open, default four.

Then it files the run's record item, opens runs/<id>/ under the machine
directory, pins the inputs and the policy in force, runs the pack's bundle
command into the directory, and hashes all three onto the record and onto
run.started. A refusal there — a bundle command that exits non-zero — closes
the record it filed as failed, with run.failed naming it.

Then it runs the pack's run command with the pinned inputs as JSON on
stdin and FLEET_RUN_ID, FLEET_STREAM, FLEET_STREAM_SEQ, FLEET_RUN_DIR,
FLEET_BIN, FLEET_PROJECT (the project root the run was started inside, which
the SDK runs every verb from — the child's own cwd is the run directory) and
FLEET_DIR in its environment, keeping stdout.log and stderr.log in the run
directory. Six names pass through from fleet's own environment when set
there and are not names of the run: PATH, HOME, FLEET_CLAUDE_BIN, USER,
TMPDIR and LANG — the dispatch a workflow calls resolves the agent binary on
a child PATH built from HOME, or from FLEET_CLAUDE_BIN as written, and the
seat it starts reads its keychain login off USER. Nothing else is inherited.

The exit table is the workflow's answer: 0 is run.closed, 1 is run.failed
with the reason read as JSON off stdout's last line, 2 is run.waiting
with the wake condition read the same way and the stream sequence at
exit. Any other exit, a signal, or a last line that is not JSON is
run.could_not_tell with what was read. A closed or failed run is closed
on the record; a waiting one stays open.")]
    Run(item::RunArgs),

    /// cancel a run: clear its holds, close its record, let its seats go
    #[command(long_about = "\
cancel a run: every hold standing on its record cleared, the record closed as
cancelled, and run.cancelled on the stream, with one item.entry per hold —
one act, for a run nothing else will end: held at [core.run] max_crashes,
waiting on a wake that will not come, or gone without a row of the exit
table. The record is what [core.run] max_open counts, so a cancel frees its
slot.

The controller's next poll retires the seats the run spawned, as it does for
a run that closed, and never executes the run again. It stops no process: an
execution under way when the run is cancelled runs to its end, and nothing
acts on what it says.

It refuses an id that is not a run's record and a run already closed.")]
    Cancel(item::CancelArgs),

    /// print the projection: the roster, the context and the rules
    #[command(long_about = "\
print the projection the controller published: its age with the word STALE
past three poll intervals, GRANT PENDING where the grant is unanswered,
the roster with each seat's verdict, outcome and halt latch, in_flight and
effects, each seat's context against the rest threshold, the
[[core.flight.rules]] table of the policy in force, and the routines.

It reads the published document and the policy file — never the process
table — and it writes nothing. --json prints the document verbatim and
--seat <name> prints one seat's two rows; no projection is exit 5.")]
    Status(status::StatusArgs),

    /// run the doctor checks the pack layers carry
    #[command(long_about = "\
run the doctor checks the pack layers carry: every doctor/<name>/ entry
resolved through the layers, the defaults at the bottom and each installed
pack above, a pack's entry replacing a lower one of the same name. Name
checks to run those alone.

Each check runs its doctor.toml's `run` script with sh from the project
root, bounded at 60 seconds, with FLEET_PACK_DIR naming the pack that
carries it. runtime-version runs once for each pack that declares a
[runtime] table, on the PATH `fleet run` gives that pack's workflows.

One row per check: pass, finding or could not tell, its name, its layer
and its last line; a row that did not pass is followed by all it printed.
It writes nothing.

Exit 0 when every check passed, 1 when one reported a finding, 3 when one
could not tell — it exited 3 or anything but 0 and 1, was killed, or did
not answer in time. 3 wins over 1. --json prints one document whose data
carries every row, with the same exit.")]
    Doctor(doctor::DoctorArgs),

    /// judge one pre-tool payload read from stdin
    #[command(long_about = "\
judge one pre-tool payload read from stdin, read through an agent adapter's
[hook] mapping: the built-in claude-code one, or the one --adapter names —
a name the installed packs carry, or the absolute path to its directory. A
refusal is the mapping's deny template with the reason filled in, on stdout,
and the exit is 0 either way, because a non-zero exit from a pre-tool hook is
read as non-blocking. An --adapter that resolves nowhere, or declares no
[hook], exits 2 with one line on stderr. --check reports whether each check's
target is configured.")]
    Guard {
        /// the guard class: shell-trap, record, release-ref or production-write
        #[arg(value_name = "CLASS", value_parser = parse_class)]
        class: Class,

        /// the agent adapter whose [hook] mapping is read
        #[arg(long, value_name = "NAME|PATH")]
        adapter: Option<String>,

        /// report whether each check's target is configured
        #[arg(long)]
        check: bool,
    },

    /// what a session is handed at its start
    #[command(long_about = "\
what a session is handed at its start: the version, this fleet's pack layers
and its guards on one line, then the resolved rules file, then one line per
item assigned to the seat whose row names this directory.

It reads nothing from stdin and always exits 0. A session-start hook that fails
closed takes the session down with it, so a part that cannot be read says so in
its own line and the next part still prints.")]
    Prime,
}

/// The verbs done TO a seat — the add that lists one, three to a transient one,
/// the courier to any and the attach that opens any one's session — plus the
/// four lifecycle words kept as hidden arms.
///
/// The four are HIDDEN and not absent: they are what a seat's ritual types, and
/// a bare "unrecognized subcommand" would leave the person to guess the `event`
/// spelling. Hidden keeps them off this family's help page, where listing them
/// would advertise verbs this noun does not answer.
#[derive(Subcommand)]
enum SeatVerb {
    /// add a seat to this fleet: a fresh id, its kind, a name if given
    #[command(long_about = "\
add a seat to the fleet's own fleet.toml: one [seats.<id>] table, appended
and read back before it answers 0. --agent mints a fresh id for a seat the
controller runs; --human lists this machine's identity, minting it into
identity.toml in the machine directory where there is none. Nobody is asked
for a name. It prints the seat's id.")]
    Add(seat_add::AddArgs),

    /// create a transient seat and start its session
    #[command(long_about = "\
create a transient seat: the load belt first, then a worktree cut from the
project's origin/main, a seat-list row and a session started with the file's
text as its first turn. A failed start rolls the worktree and the row back and
never a branch. It prints the seat's machine name, agent-<short id>, and
writes nothing to the work graph.")]
    Spawn(transient::SpawnArgs),

    /// hand a live transient seat its next first turn
    #[command(long_about = "\
hand a live transient seat its next first turn and move the row's occupant
marker, with a put-back when the delivery fails. It refuses a seat the agent
reports still mid-turn. The work-graph writes around it are the caller's.")]
    Feed(transient::FeedArgs),

    /// end a transient seat and reclaim the machine
    #[command(long_about = "\
end a transient seat: the session stopped and removed, the worktree gone, both
rows dropped, and then a check from OUTSIDE that nothing of the seat holds RAM
or disk. It prints the reclaim and never deletes a branch. --dead licenses a
seat whose session is already gone, by a completed roster read.")]
    Retire(transient::RetireArgs),

    /// carry one message to a seat's live session
    #[command(long_about = "\
carry one message to a seat's live session, through the same provider path
the controller's own nudge takes. THE MESSAGE CARRIES NO AUTHORITY — a nudge
is a doorbell, and the seat acts on its record and never on what the message
said.

It refuses from the published projection before it delivers anything: no
projection, or one older than three poll intervals, is 5; a seat the
projection does not carry, or carries without a live session, is 4. A
delivery the provider refuses writes its event and exits 1.")]
    Nudge(nudge::NudgeArgs),

    /// open a seat's session in this terminal, read-only unless --write
    #[command(long_about = "\
open a seat's session in this terminal, read-only unless --write. Detach with
the tmux prefix and d. Typing under --write is typing as the seat: the
session's verbs act as it.

It needs no running controller: the sessions are tmux's, on fleet's own
socket, and outlive one. A seat with no session there is 4; a dead session is
still opened, under a line naming its exit status, because its last screen is
what says why it ended. --write writes one seat.attached line to the stream
first, so a keyboard taken over a seat is on the record.")]
    Attach(attach::AttachArgs),

    // THE HELP FLAG IS DISABLED ON ALL FOUR, and the words are taken as a
    // trailing var-arg: a lifecycle verb is typed with whatever the ritual typed
    // it with — `--reason x`, or `--help` by a person looking for the page — and
    // a parser that claimed either for itself would answer with a sentence about
    // the flag instead of with the one rewrite that helps.
    #[command(hide = true, disable_help_flag = true)]
    Woke(SeatWord),
    #[command(hide = true, disable_help_flag = true)]
    Rest(SeatWord),
    #[command(hide = true, disable_help_flag = true, name = "handed-off")]
    HandedOff(SeatWord),
    #[command(hide = true, disable_help_flag = true)]
    Exited(SeatWord),
}

// Whatever a lifecycle word was given. It is never read: the arm's whole answer
// is the rewrite that names the `event` spelling. A `///` here would become the
// struct's about text, which the help-width arm renders even for a hidden verb.
#[derive(clap::Args)]
struct SeatWord {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "WORDS"
    )]
    words: Vec<String>,
}

// Whatever followed the retired family name. Never read, for the same reason:
// the arm's whole answer is the rewrite that names `routine`.
#[derive(clap::Args)]
struct OldWords {
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "WORDS"
    )]
    words: Vec<String>,
}

#[derive(Subcommand)]
enum EventVerb {
    /// record that a seat started and oriented
    Woke(SeatOnly),
    /// ask for a rest: the session stops and a successor wakes
    Rest(SeatEvent),
    /// record that a seat finished its handoff
    HandedOff(SeatOnly),
    /// record a deliberate end
    Exited(SeatOnly),
    /// ask that a halted seat's hold be lifted
    #[command(
        name = "clear-halt",
        long_about = "\
lift the hold on one seat. It is a REQUEST, like a rest: it is written to the
stream and the controller consumes it on its next tick, resetting the seat's
blind counter and its halt latch. It refuses when no collector is consuming
(5), because a request nothing reads would leave the seat held; and when the
seat is not halted (1), printing the state it read from the projection."
    )]
    ClearHalt(SeatEvent),

    /// record one half of a workflow's step: what the SDK replays from
    #[command(long_about = "\
record one half of a numbered step of a run, for a workflow's own process to
call: `started` before the step runs, `closed` after it, carrying the result
as one JSON value or, over the SDK's size cap, the sha256 of the result file
in the run directory. The actor is FLEET_ACTOR, else the run (run:<id>).
The SDK reads the pair back through `fleet event tail --json`; nothing in the
controller consumes it.")]
    Step(step::StepArgs),

    // The reader half of the family, beside the writers: what a seat or a
    // workflow says is read back where it is written. Each option's help is on
    // the line under it: `--actor <KIND:ID>` widens the option column past
    // where the longest help line fits in eighty columns beside it.
    /// print lines of the stream, as stored
    #[command(next_line_help = true)]
    Tail(stream::TailArgs),

    /// print one event by id, pretty-printed
    Show {
        /// the id of the event to print
        id: String,

        /// print the envelope document instead of the pretty-printed event
        #[arg(long)]
        json: bool,
    },
}

/// What `rest` and `clear-halt` take: the two seat writers whose payload
/// carries a reason, so the two that parse `--reason`.
#[derive(clap::Args)]
struct SeatEvent {
    /// the seat the event is about
    seat: String,
    /// the reason, carried in the event's payload
    #[arg(long)]
    reason: Option<String>,
}

/// What `woke`, `handed-off` and `exited` take. They are records, and a record
/// writes no reason — so `--reason` is a usage error here (exit 2) rather than a
/// flag they accept and drop.
#[derive(clap::Args)]
struct SeatOnly {
    /// the seat the event is about
    seat: String,
}

#[derive(Subcommand)]
enum PackVerb {
    /// validate one pack, and its layering with --over
    #[command(long_about = "\
validate one pack against the format. --over resolves it above the packs
named, in the order given, and refuses a collision or an unlisted shadow.")]
    Check {
        /// the pack directory to validate
        dir: PathBuf,
        /// a pack this one resolves above; repeatable, lowest last
        #[arg(long, value_name = "DIR")]
        over: Vec<PathBuf>,
    },

    /// fetch one pack by git source and version and pin it in packs.lock
    #[command(long_about = "\
fetch one pack by git source and version into the packs dir and pin it in
packs.lock. <source> may end in //<subdirectory>.")]
    Add {
        /// the git URL or local path, optionally ending in //<subdirectory>
        source: String,
        /// a tag, a branch, or sha:<40 hex>
        #[arg(long, value_name = "VERSION")]
        version: String,
        /// where packs are installed
        #[arg(long = "packs-dir", value_name = "DIR")]
        packs_dir: Option<PathBuf>,
        /// the lock file to pin in
        #[arg(long, value_name = "FILE")]
        lock: Option<PathBuf>,
    },

    /// take one pack out of the packs dir and drop its line from packs.lock
    #[command(long_about = "\
take one pack out: its directory, then its line in packs.lock, keyed by the
source as typed. Refuses the binary's own defaults, a pack another installed
pack imports, and a source the lock does not hold.")]
    Remove {
        /// the source as it was typed to `pack add`
        source: String,
        /// where packs are installed
        #[arg(long = "packs-dir", value_name = "DIR")]
        packs_dir: Option<PathBuf>,
        /// the lock file to drop the line from
        #[arg(long, value_name = "FILE")]
        lock: Option<PathBuf>,
    },

    /// print packs.lock as a table
    #[command(long_about = "\
print packs.lock as a table: name, source, version, commit and fetched, one row
per pack, in source order. An empty lock, or a lock path that is not there,
prints the header alone.")]
    List {
        /// the lock file to read
        #[arg(long, value_name = "FILE")]
        lock: Option<PathBuf>,
    },
}

fn parse_class(name: &str) -> Result<Class, String> {
    Class::parse(name).ok_or_else(|| {
        let names: Vec<&str> = guard::CLASSES.iter().map(|c| c.name()).collect();
        format!("unknown class `{name}` — one of {}", names.join(", "))
    })
}

// The one place a status is chosen. Every other function in this binary answers
// in the exit table's own words, and an Err is could-not-tell: the chain says
// which instrument could not be read and the status is 3, never 0.
fn main() -> ExitCode {
    match dispatch() {
        Ok(exit) => ExitCode::from(exit.code()),
        Err(err) => {
            eprintln!("fleet: {err}");
            for cause in err.chain().skip(1) {
                eprintln!("  caused by: {cause}");
            }
            ExitCode::from(Exit::CouldNotTell.code())
        }
    }
}

fn dispatch() -> Result<Exit> {
    let cli = Cli::parse();

    if cli.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(Exit::Done);
    }

    // No command is a usage error, not a no-op: a caller that reached here meant
    // something and the exit status has to say so. Clap would print the help and
    // exit 0, so the help goes to stderr by hand and the status is the table's.
    let Some(family) = cli.family else {
        eprint!("{}", Cli::command().render_help());
        return Ok(Exit::Usage);
    };

    // The terminal is read once, here, and handed to the verbs that print
    // through it: a per-call reading would let one page change shape halfway
    // down (src/ui.rs).
    let ui = Ui::from_env();

    match family {
        Family::Create(args) => Ok(lifecycle::create_command(&ui, &args)),
        Family::Start(args) => Ok(lifecycle::start_command(&ui, &args)),
        Family::Stop => Ok(lifecycle::stop_command(&ui)),
        Family::Observe { once } => observe(once),
        Family::Seat { verb } => Ok(seat_command(&ui, &verb)),
        Family::Event { verb } => event_command(&verb),
        Family::Pack { verb } => pack_command(&ui, &verb),
        Family::Routine { verb } => Ok(routines::command(&verb)),
        Family::Item { verb } => Ok(item_show::command(&verb)),
        Family::Store { verb } => Ok(store::command(&verb)),
        Family::Agent { verb } => Ok(agent::command(&verb)),
        Family::Order(_) => Ok(routines::old_name()),
        Family::Dispatch(args) => Ok(item::dispatch_command(&args)),
        Family::Brief(args) => Ok(item::brief_command(&args)),
        Family::Deliver(args) => Ok(item::deliver_command(&args)),
        Family::Hold(args) => Ok(item::hold_command(&args)),
        Family::Clear(args) => Ok(item::clear_command(&args)),
        Family::Ask(_) => Ok(old_verb(
            "ask",
            "a question is a hold now",
            "fleet hold --question <file>",
        )),
        Family::Answer(_) => Ok(old_verb(
            "answer",
            "an answer is a hold's clearance now",
            "fleet clear <item> <letter> [--text <text>]",
        )),
        Family::Review(args) => Ok(item::review_command(&args)),
        Family::Land(args) => Ok(item::land_command(&ui, &args)),
        Family::Run(args) => Ok(item::run_command(&args)),
        Family::Cancel(args) => Ok(item::cancel_command(&args)),
        Family::Status(args) => Ok(status::status_command(&args)),
        Family::Doctor(args) => Ok(doctor::command(&ui, &args)),
        Family::Guard {
            class,
            adapter,
            check,
        } => guard_command(&ui, class, adapter.as_deref(), check),
        Family::Prime => Ok(prime::command()),
    }
}

/// The loop, wired — the ONE place it is built, so `observe` and `start
/// --foreground` cannot differ in what a poll advances.
///
/// THE RUN SEAM IS FILLED. It is built on the MACHINE DIRECTORY and on no
/// project: a service-started controller has no working directory to resolve
/// one from, so the engine looks each run's record up over the projects the
/// machine registers. `None` here is a loop that never re-runs, holds or cleans
/// a run.
pub fn observe_loop(once: bool) -> u8 {
    let engine = runs::Engine::on(platform::machine_dir());
    run::observe_runs(
        &run::Options { once },
        platform::Grant::new(platform::directory_listing(), platform::GRANT_PROBE_TIMEOUT),
        Some(&engine as &dyn RunsSeam),
    )
}

fn observe(once: bool) -> Result<Exit> {
    let status = observe_loop(once);
    // The loop's own could-not-tell: it printed which instrument it could not
    // read, and the chain names the verb that could not run because of it.
    if status == run::EXIT_NO_POLICY {
        return Err(anyhow::anyhow!(
            "the seat list could not be read, so no poll ran"
        ))
        .context("fleet observe");
    }
    Exit::from_status(status).context("fleet observe")
}

// The four seat events. This is argv and nothing else: which event a
// verb names, which seat it is about — resolved through the seat list, as every
// seat argument is — and the reason a rest carries. Every other refusal, every
// status and the read-back are `seat::record`'s, in the controller, so the
// check that a rest is answerable lives beside the loop that would answer it.
fn event_command(verb: &EventVerb) -> Result<Exit> {
    let (name, kind, seat, reason) = match verb {
        EventVerb::Woke(e) => ("woke", events::SEAT_WOKE, &e.seat, None),
        EventVerb::Rest(e) => ("rest", events::SEAT_RESTING, &e.seat, e.reason.as_deref()),
        EventVerb::HandedOff(e) => ("handed-off", events::SEAT_HANDED_OFF, &e.seat, None),
        EventVerb::Exited(e) => ("exited", events::SEAT_EXITED, &e.seat, None),
        EventVerb::ClearHalt(e) => (
            "clear-halt",
            events::SEAT_CLEAR_HALT,
            &e.seat,
            e.reason.as_deref(),
        ),
        // The one writer whose subject is a run and not a seat: its append and
        // its read-back are the step module's.
        EventVerb::Step(args) => return Ok(step::record(args)),
        // The readers write nothing and take no seat, so they answer before the
        // writers' one shared call below.
        EventVerb::Tail(args) => return Ok(stream::tail(args)),
        EventVerb::Show { id, json } => return Ok(stream::show(id, *json)),
    };

    // The seat argument through the seat list's resolver first, and its id
    // from here on: the stream and the projection are keyed on it.
    let machine_dir = platform::machine_dir();
    let seat = match transient::seat_named(&machine_dir, seat) {
        Ok(row) => row.id,
        Err(stop) => {
            eprintln!("fleet event {name}: {}", stop.message);
            return Exit::from_status(stop.code).with_context(|| format!("fleet event {name}"));
        }
    };
    match seat::record(&machine_dir, kind, &seat, reason) {
        Ok(line) => {
            println!("{line}");
            Ok(Exit::Done)
        }
        Err((status, why)) => {
            eprintln!("fleet event {name}: {why}");
            Exit::from_status(status).with_context(|| format!("fleet event {name}"))
        }
    }
}

// The verbs done to a seat, and the four lifecycle words that are not done to
// one.
fn seat_command(ui: &Ui, verb: &SeatVerb) -> Exit {
    match verb {
        SeatVerb::Add(args) => seat_add::command(args),
        SeatVerb::Spawn(args) => transient::spawn_command(args),
        SeatVerb::Feed(args) => transient::feed_command(args),
        SeatVerb::Retire(args) => transient::retire_command(args),
        SeatVerb::Nudge(args) => nudge::nudge_command(ui, args),
        SeatVerb::Attach(args) => attach::attach_command(args),
        SeatVerb::Woke(_) => seat_usage_error("woke"),
        SeatVerb::Rest(_) => seat_usage_error("rest"),
        SeatVerb::HandedOff(_) => seat_usage_error("handed-off"),
        SeatVerb::Exited(_) => seat_usage_error("exited"),
    }
}

// `seat` is what is done TO a seat and `event` is what a seat SAYS, so the four
// lifecycle writers answer under `event` alone. The four verbs are named back
// with their rewrite rather than met by a bare unknown subcommand, because they
// are the four a seat's ritual types.
fn seat_usage_error(verb: &str) -> Exit {
    eprintln!(
        "fleet seat {verb}: the seat noun is what is done to a seat — say fleet event {verb}"
    );
    // The root's usage line and not this family's help: the refusal above is
    // the whole answer, and the rewrite is the only text a reader of it needs.
    // `fleet seat --help` is where the family's own page lives.
    eprint!("{}", Cli::command().render_usage());
    eprintln!();
    Exit::Usage
}

// `ask` and `answer` are `hold` and `clear` now (fleet-6gr, V1). Named back with
// their rewrite, as the lifecycle words under `seat` are, rather than met by a
// bare unknown subcommand: they are the two a seat's rules and a person's habit
// type.
fn old_verb(verb: &str, why: &str, rewrite: &str) -> Exit {
    eprintln!("fleet {verb}: {why} — say {rewrite}");
    eprint!("{}", Cli::command().render_usage());
    eprintln!();
    Exit::Usage
}

// 0 is a valid pack, 1 is a pack with defects or a layering that refuses, and 2
// is a caller who did not say what to check: the three are distinct because a
// script that cannot tell a bad pack from a bad invocation reports the wrong
// one.
fn pack_command(ui: &Ui, verb: &PackVerb) -> Result<Exit> {
    match verb {
        PackVerb::Check { dir, over } => pack_check(ui, dir, over),
        PackVerb::Add {
            source,
            version,
            packs_dir,
            lock,
        } => pack_add(ui, source, version, packs_dir.as_deref(), lock.as_deref()),
        PackVerb::Remove {
            source,
            packs_dir,
            lock,
        } => pack_remove(ui, source, packs_dir.as_deref(), lock.as_deref()),
        PackVerb::List { lock } => pack_list(ui, lock.as_deref()),
    }
}

fn pack_check(ui: &Ui, dir: &Path, over: &[PathBuf]) -> Result<Exit> {
    let report = pack::check(dir);
    let name = report
        .manifest
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| pack_name_of(dir));

    for defect in &report.defects {
        ui.status(
            Stream::Err,
            Tone::Bad,
            &format!("{name}:"),
            &defect.to_string(),
            None,
        );
    }
    if !report.is_valid() {
        return Ok(Exit::Refused);
    }

    if let Some(manifest) = &report.manifest {
        ui.status(
            Stream::Out,
            Tone::Good,
            "pack",
            &format!("{} {}", manifest.name, manifest.version),
            Some(&format!("schema {}", manifest.schema)),
        );
        // The pin as parsed, so a reader sees the version the doctor entry
        // measures against without opening the manifest.
        if let Some(runtime) = &manifest.runtime {
            ui.status(
                Stream::Out,
                Tone::Flat,
                "runtime",
                &format!("{} {}", runtime.name, runtime.version),
                None,
            );
        }
    }
    for slot in &report.slots {
        let plural = if slot.entries == 1 {
            "entry"
        } else {
            "entries"
        };
        ui.status(
            Stream::Out,
            Tone::Flat,
            "slot",
            &format!("{}: {} {plural}", slot.name, slot.entries),
            None,
        );
    }

    if over.is_empty() {
        return Ok(Exit::Done);
    }

    let mut layers = vec![resolve::Layer::new(name, dir.to_path_buf())];
    for lower in over {
        layers.push(resolve::Layer::new(pack_name_of(lower), lower.clone()));
    }
    match resolve::resolve(&layers) {
        Ok(resolution) => {
            ui.status(
                Stream::Out,
                Tone::Good,
                "resolved",
                &format!(
                    "{} paths and {} agents across {} layers, {} shadowed",
                    resolution.files.len(),
                    resolution.agents.len(),
                    layers.len(),
                    resolution.shadowed.len()
                ),
                None,
            );
            Ok(Exit::Done)
        }
        Err(refusals) => {
            for refusal in refusals {
                // The refusal is its own whole sentence, so it is the verb and
                // there is no subject to set beside it.
                ui.status(Stream::Err, Tone::Bad, &refusal.to_string(), "", None);
            }
            Ok(Exit::Refused)
        }
    }
}

// The same three codes as `pack check`: 0 installed and pinned, 1 a refusal —
// the source, the version, the pack's own format, the layering it would enter,
// or a pin that did not land — and 2 a caller who did not say what to add.
fn pack_add(
    ui: &Ui,
    source: &str,
    version: &str,
    packs_dir: Option<&Path>,
    lock_path: Option<&Path>,
) -> Result<Exit> {
    // The machine directory is the platform layer's answer and core never asks
    // for it: the two paths are resolved here and handed down.
    let machine = platform::machine_dir();
    let packs_dir = packs_dir.map_or_else(|| machine.join("packs"), Path::to_path_buf);
    let defaults_dir = packs_dir.parent().unwrap_or(&machine).join(defaults::DIR);
    let lock_path = lock_path.map_or_else(|| machine.join(lock::LOCK), Path::to_path_buf);

    // The clone and the checkout are the unbounded wait in this verb, and the
    // only one it has: the pin that follows is a file write.
    let wait = ui.spinner(&format!("fetching {source} at {version}"));
    let added = add::add(
        &packs_dir,
        &defaults_dir,
        &lock_path,
        source,
        version,
        &clock::now_stamp(),
    );
    wait.done();

    match added {
        Ok(installed) => {
            ui.status(
                Stream::Out,
                Tone::Good,
                "added",
                &format!(
                    "{} {} at {}",
                    installed.name, installed.entry.version, installed.entry.commit
                ),
                Some(&installed.root.display().to_string()),
            );
            for import in &installed.imports {
                ui.status(
                    Stream::Out,
                    Tone::Good,
                    "added",
                    &format!(
                        "{} {} at {}, which {} imports",
                        import.name, import.entry.version, import.entry.commit, installed.name
                    ),
                    Some(&import.root.display().to_string()),
                );
            }
            ui.status(
                Stream::Out,
                Tone::Good,
                "pinned",
                &format!("in {}", lock_path.display()),
                None,
            );
            // Installed without them, and said where the next read of this
            // fleet — a run, a session's prime — would otherwise be the first
            // to find out.
            for missing in &installed.missing {
                ui.status(
                    Stream::Err,
                    Tone::Flat,
                    "fleet pack add:",
                    &missing.to_string(),
                    None,
                );
            }
            Ok(Exit::Done)
        }
        Err(refusals) => {
            for refusal in refusals {
                ui.status(
                    Stream::Err,
                    Tone::Bad,
                    "fleet pack add:",
                    &refusal.to_string(),
                    None,
                );
            }
            Ok(Exit::Refused)
        }
    }
}

// 0 removed and the drop read back, 1 a refusal — a source the lock does not
// hold, a line with no name, the binary's own defaults, or a pack another one
// imports — and 2 a caller who did not say what to remove.
fn pack_remove(
    ui: &Ui,
    source: &str,
    packs_dir: Option<&Path>,
    lock_path: Option<&Path>,
) -> Result<Exit> {
    let machine = platform::machine_dir();
    let packs_dir = packs_dir.map_or_else(|| machine.join("packs"), Path::to_path_buf);
    let lock_path = lock_path.map_or_else(|| machine.join(lock::LOCK), Path::to_path_buf);

    match remove::remove(&packs_dir, &lock_path, source) {
        Ok(removed) => {
            // A notice is a fact the verb is reporting beside a success, not a
            // refusal: it is flat, and it goes where nothing parses it.
            for notice in &removed.notices {
                ui.status(
                    Stream::Err,
                    Tone::Flat,
                    "fleet pack remove:",
                    &notice.to_string(),
                    None,
                );
            }
            ui.status(
                Stream::Out,
                Tone::Good,
                "removed",
                &format!("{} {}", removed.name, removed.entry.version),
                Some(&removed.root.display().to_string()),
            );
            // The source and the version, because the rollback is a re-add of
            // exactly this line.
            ui.status(
                Stream::Out,
                Tone::Good,
                "dropped",
                &format!(
                    "{} {} from {}",
                    removed.entry.source,
                    removed.entry.version,
                    lock_path.display()
                ),
                None,
            );
            Ok(Exit::Done)
        }
        Err(refusals) => {
            for refusal in refusals {
                ui.status(
                    Stream::Err,
                    Tone::Bad,
                    "fleet pack remove:",
                    &refusal.to_string(),
                    None,
                );
            }
            Ok(Exit::Refused)
        }
    }
}

// 0 and 3 only: there is nothing here to refuse. A lock that does not parse is
// an instrument the answer needs and could not read, which is could-not-tell and
// not an empty list.
fn pack_list(ui: &Ui, lock_path: Option<&Path>) -> Result<Exit> {
    let lock_path = lock_path.map_or_else(
        || platform::machine_dir().join(lock::LOCK),
        Path::to_path_buf,
    );

    match lock::read(&lock_path) {
        Ok(entries) => {
            // The table is the standard library's and reaches stdout unstyled:
            // a listing verb prints columns, and the ui module holds no table
            // surface for it to reach.
            print!("{}", lock_table(&entries));
            Ok(Exit::Done)
        }
        Err(e) => {
            ui.status(
                Stream::Err,
                Tone::Bad,
                "fleet pack list:",
                &e.to_string(),
                None,
            );
            Ok(Exit::CouldNotTell)
        }
    }
}

/// The lock as a table: a header, then one row per entry in the order the lock
/// holds them, which is source order. Every column but the last is padded to its
/// widest cell, the header's own width counted, and a line with no name prints
/// `-` in that column.
fn lock_table(entries: &[lock::Entry]) -> String {
    const HEADER: [&str; 5] = ["name", "source", "version", "commit", "fetched"];
    let rows: Vec<[&str; 5]> = std::iter::once(HEADER)
        .chain(entries.iter().map(|e| {
            [
                e.name.as_deref().unwrap_or("-"),
                e.source.as_str(),
                e.version.as_str(),
                e.commit.as_str(),
                e.fetched.as_str(),
            ]
        }))
        .collect();

    let widths: Vec<usize> = (0..HEADER.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut out = String::new();
    for row in &rows {
        for column in 0..HEADER.len() {
            let cell = row[column];
            out.push_str(cell);
            if column + 1 < HEADER.len() {
                out.push_str(&" ".repeat(widths[column] - cell.chars().count() + 2));
            }
        }
        out.push('\n');
    }
    out
}

// ---- the guards -------------------------------------------------------------
//
// 2 is a caller who did not name a class, the same usage code the pack verbs
// use, and a guard wired to an adapter whose mapping cannot be read. The
// JUDGING path has no other code: a refusal is data on stdout and the exit is 0
// on every path, because a non-zero exit from a pre-tool hook is read as
// non-blocking and a guard that signalled by status would fail open exactly
// when it broke. --check is the one guard path with a verdict in its exit.
//
// The mis-wired guard is the exception, and the reason is the same table read
// the other way: the agent reads a hook's 2 as blocking, so a guard that cannot
// read the payload it was wired for blocks every call until it is wired right,
// rather than letting each one through unjudged.

fn guard_command(ui: &Ui, class: Class, adapter: Option<&str>, check: bool) -> Result<Exit> {
    if check {
        return Ok(guard_check(ui, class));
    }
    let map = match hook_map(adapter) {
        Ok(map) => map,
        Err(missing) => {
            eprintln!("fleet guard: {missing}");
            return Ok(Exit::Usage);
        }
    };
    Ok(guard_run(class, &map))
}

/// The mapping a payload is read and a refusal written through: the built-in
/// claude-code manifest where no `--adapter` is given, and otherwise the
/// `[hook]` table of the agent adapter it names — by an absolute path to the
/// adapter's directory, or by a bare name the machine's packs carry over its
/// defaults, as a store adapter's name is looked up. `claude-code` where no
/// pack carries it is the built-in.
///
/// Each `Err` is one line naming what was missing. The manifest is read
/// through the pack format's own reader, so a mapping `fleet pack check`
/// refuses is never applied.
fn hook_map(adapter: Option<&str>) -> Result<HookMap, String> {
    let dir = match adapter {
        None => return built_in_hook(),
        Some(path) if path.starts_with('/') => PathBuf::from(path),
        Some(name) if !name.is_empty() && !name.contains('/') => {
            let machine_dir = platform::machine_dir();
            let packs = fleet_core::item::brief::Packs::under(
                &machine_dir.join("packs"),
                &machine_dir.join(defaults::DIR),
            )
            .map_err(|stop| {
                format!(
                    "the agent adapter `{name}` is looked up in the installed packs: {}",
                    stop.message
                )
            })?;
            match pack::adapter_dir(&packs, pack::AdapterKind::Agent, name) {
                Some((_, dir)) => dir,
                None if name == claude_code::NAME => return built_in_hook(),
                None => {
                    return Err(format!(
                        "no installed pack carries the agent adapter `{name}` — `{}/{}/{name}/{}` \
                         resolves nowhere",
                        pack::ADAPTERS,
                        pack::AdapterKind::Agent.as_str(),
                        pack::ADAPTER_MANIFEST
                    ))
                }
            }
        }
        Some(other) => {
            return Err(format!(
                "--adapter `{other}` is neither an agent adapter's name nor an absolute path to \
                 its directory"
            ))
        }
    };
    let manifest = pack::adapter_manifest(&dir).map_err(|defect| defect.to_string())?;
    let Some(table) = manifest.hook else {
        return Err(format!(
            "the agent adapter at {} declares no [{}] — a guard has no mapping to read its \
             payload through",
            dir.display(),
            pack::HOOK_TABLE
        ));
    };
    HookMap::parse(&table)
}

/// Claude Code's mapping, compiled in: the built-in's `[hook]` table.
fn built_in_hook() -> Result<HookMap, String> {
    HookMap::from_manifest(claude_code::HOOK_MANIFEST)
        .map_err(|why| format!("the built-in {} hook mapping: {why}", claude_code::NAME))
}

fn guard_run(class: Class, map: &HookMap) -> Exit {
    let mut body = String::new();
    if std::io::stdin().read_to_string(&mut body).is_err() {
        return Exit::Done;
    }
    let Some(payload) = map.payload(&body) else {
        return Exit::Done;
    };
    let cwd = payload.cwd.as_deref().map(Path::new);
    let mut policy = resolve_policy(class, cwd);
    caller_readings(class, &payload.command, cwd, &mut policy);
    // A defect in the reader allows, which is the contract the shell-trap class
    // states and the reason it has no raw-text fallback.
    let verdict = std::panic::catch_unwind(|| guard::judge(class, &payload.command, &policy));
    if let Ok(Verdict::Refused(denial)) = verdict {
        println!("{}", map.deny(&denial.reason()));
    }
    Exit::Done
}

fn guard_check(ui: &Ui, class: Class) -> Exit {
    let policy = resolve_policy(class, None);
    let mut all = true;
    for check in class.checks() {
        let name = class.name();
        match class.target_of(check) {
            None => ui.status(
                Stream::Out,
                Tone::Good,
                name,
                &format!("{check}: configured"),
                None,
            ),
            Some(key) => {
                if guard::configured(class, check, &policy) {
                    ui.status(
                        Stream::Out,
                        Tone::Good,
                        name,
                        &format!("{check}: configured"),
                        Some(key),
                    );
                } else {
                    ui.status(
                        Stream::Out,
                        Tone::Bad,
                        name,
                        &format!("{check}: not configured"),
                        Some(key),
                    );
                    all = false;
                }
            }
        }
    }
    if all {
        Exit::Done
    } else {
        Exit::Refused
    }
}

// A DECLARED PROJECT FIRST, then the embedded file, then neither. The walk is
// the item verbs' (`crate::item::resolve_at`), and the two answers differ in
// where the guards come from: an embedded fleet keeps its policy beside the
// work, so its own fleet.toml carries both the switches and the target; a
// standalone project declares only itself, so the switches are the FLEET's —
// the file the machine directory names — and only the target is the project's.
// Failing both, every check runs and the bare-id check has no target, so it
// refuses nothing.
fn resolve_policy(class: Class, cwd: Option<&Path>) -> Policy {
    let start = cwd
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let found = start.as_deref().and_then(walk_up_config);
    // An unreadable or unparsable file reads as an empty table, which leaves
    // every check on and prints nothing: opt-out, never opt-in. That holds for
    // the machine's file as much as for the project's, so an absent machine
    // config below leaves the class on rather than switching it off.
    let machine_policy = || {
        config::read(&platform::machine_dir().join("config.json"))
            .map(|machine| guard::enabled_in(class, &read_text(&machine.fleet_toml)))
            .unwrap_or(true)
    };

    match found {
        Some(Found::Embedded(path)) => {
            let text = read_text(&path);
            with_targets(
                Policy {
                    enabled: guard::enabled_in(class, &text),
                    item_prefix: guard::item_prefix_in(&text),
                    ..Policy::default()
                },
                guard::targets_in(&text),
            )
        }
        Some(Found::Declared(path)) => {
            let text = read_text(&path);
            with_targets(
                Policy {
                    enabled: machine_policy(),
                    item_prefix: guard::item_prefix_in(&text),
                    ..Policy::default()
                },
                guard::targets_in(&text),
            )
        }
        None => Policy {
            enabled: machine_policy(),
            ..Policy::default()
        },
    }
}

/// The project's own `[guards.targets]`, onto the policy the walk built.
fn with_targets(policy: Policy, targets: guard::Targets) -> Policy {
    Policy {
        release_ref_glob: targets.release_ref_glob,
        prod_buckets: targets.prod_buckets,
        prod_projects: targets.prod_projects,
        prod_apps: targets.prod_apps,
        prod_make_goals: targets.prod_make_goals,
        prod_dagger_functions: targets.prod_dagger_functions,
        prod_workflow_refs: targets.prod_workflow_refs,
        ..policy
    }
}

/// The two readings core cannot make, because it opens no path and runs no
/// process: the application the deployment file beside the command names, and
/// the project this machine is currently configured for. Both are targets the
/// production-write class NARROWS with — a command that names neither an
/// application nor a project reaches one anyway — so both are resolved here and
/// handed in, and an unreadable answer leaves the check without that target
/// rather than refusing on it.
///
/// The configured project costs a subprocess, so it is read only when the text
/// could possibly need it. That filter is deliberately crude and one-directional:
/// it can only leave the reading UNTAKEN, which is the silent direction the
/// class already fails in.
///
/// The shell-trap and record classes read a third: the command the project's
/// store declares, which an adapter answers as a process. It is asked only for
/// a text [`guard::reads_the_cli`] says a declaration could change the verdict
/// on, and a store that does not answer leaves the default adapter's command
/// policed rather than none.
fn caller_readings(class: Class, command: &str, cwd: Option<&Path>, policy: &mut Policy) {
    if matches!(class, Class::ShellTrap | Class::Record) {
        if guard::reads_the_cli(command) {
            if let Some(declared) = store_cli(cwd) {
                policy.cli = declared;
            }
        }
        return;
    }
    if class != Class::ProductionWrite {
        return;
    }
    if let Some(dir) = cwd {
        policy.cwd_app = guard::app_in(&read_text(&dir.join(FLY_CONFIG)));
    }
    if !policy.prod_projects.is_empty() && command.contains(CLOUD_TOOL) {
        policy.active_project = configured_project();
    }
}

/// The deployment file an application's own directory carries, and the command
/// word whose project this machine holds a default for. Both are one provider's
/// spelling and neither is core's, which is why they sit in the caller.
const FLY_CONFIG: &str = "fly.toml";
const CLOUD_TOOL: &str = "gcloud";

/// How long the configured-project read is given before it is abandoned. A
/// pre-tool hook runs before EVERY shell call, so an unbounded read here would
/// hang the session rather than fail it.
const PROJECT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The project this machine is configured for, or `None` where it could not be
/// read at all — an unset value, a non-zero exit, a missing binary and a read
/// that ran past its bound are one answer, because the class treats every one of
/// them the same way.
fn configured_project() -> Option<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new(CLOUD_TOOL)
            .args(["config", "get-value", "project"])
            .output();
        let _ = sender.send(out);
    });
    let out = receiver.recv_timeout(PROJECT_READ_TIMEOUT).ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if value.is_empty() || value == "(unset)" {
        None
    } else {
        Some(value)
    }
}

/// How long the store is given to declare its command. An adapter's answer is
/// a process, and one that will not answer costs the reading and never the
/// shell call behind the hook.
const STORE_CLI_TIMEOUT: Duration = Duration::from_secs(2);

/// The command the project's store declares, through the opener every verb
/// takes: `Some(None)` for a store declaring none, and `None` where it could
/// not be read — no project above the caller, a store that will not open or
/// will not answer — which leaves the policy's default in place.
///
/// The store read is the one a verb run here opens, on the same constructed
/// child PATH.
fn store_cli(cwd: Option<&Path>) -> Option<Option<String>> {
    let start = cwd
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let root = match walk_up_config(&start)? {
        Found::Embedded(file) => file.parent()?.to_path_buf(),
        Found::Declared(file) => file.parent()?.parent()?.to_path_buf(),
    };
    let policy = fleet_core::store::project_policy(&root).ok()?;
    let machine_dir = platform::machine_dir();
    let store = fleet_core::store::open(&fleet_core::store::Opening {
        root: &root,
        policy: &policy,
        source: fleet_core::store::AdapterSource::Setting,
        search_path: &platform::child_path(&platform::home_dir()),
        timeout: STORE_CLI_TIMEOUT,
        packs: Some(fleet_core::store::PackDirs {
            packs_dir: &machine_dir.join("packs"),
            defaults_dir: &machine_dir.join(defaults::DIR),
        }),
    })
    .ok()?;
    Some(store.capabilities().ok()?.cli)
}

/// What the nearest directory above the caller that says anything says it is.
pub enum Found {
    /// A project declaring itself to the fleet this machine runs.
    Declared(PathBuf),
    /// A fleet keeping its own policy beside the work.
    Embedded(PathBuf),
}

/// The one resolution order, level by level: A DECLARED PROJECT WINS AT ITS OWN
/// LEVEL. A directory carrying `.fleet/project.toml` is a standalone project
/// even where a `fleet.toml` sits beside it, because the declaration is that
/// directory's own statement about itself and the neighbour may be some other
/// tool's file.
pub fn walk_up_config(start: &Path) -> Option<Found> {
    let mut here = Some(start);
    while let Some(dir) = here {
        let declared = dir.join(".fleet/project.toml");
        if declared.is_file() {
            return Some(Found::Declared(declared));
        }
        let embedded = dir.join("fleet.toml");
        if embedded.is_file() {
            return Some(Found::Embedded(embedded));
        }
        here = dir.parent();
    }
    None
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

// A layer is named by its manifest where it has a readable one, and by its
// directory otherwise: a refusal that names neither is one a reader cannot act
// on.
fn pack_name_of(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join(pack::MANIFEST))
        .ok()
        .and_then(|text| pack::parse_manifest(&text).ok())
        .map(|manifest| manifest.name)
        .unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.display().to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    /// clap's own audit of the shape above: a conflicting name, a missing value
    /// name or a positional after a trailing one is caught here rather than at
    /// the first call.
    #[test]
    fn the_command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    /// Every usage line under 80 columns. What is measured is the page a person
    /// sees, both forms of it, because an about that fits on its own still runs
    /// over once clap has laid the verb column out to its left.
    #[test]
    fn every_help_line_is_under_eighty_columns() {
        fn walk(command: &mut clap::Command, path: &str) {
            for page in [
                command.render_help().to_string(),
                command.render_long_help().to_string(),
            ] {
                for line in page.lines() {
                    assert!(
                        line.chars().count() < 80,
                        "`{path} --help` runs to {} columns: {line}",
                        line.chars().count()
                    );
                }
            }
            let names: Vec<String> = command
                .get_subcommands()
                .map(|s| s.get_name().to_string())
                .collect();
            for name in names {
                let sub = command
                    .find_subcommand_mut(&name)
                    .expect("the name came from this command");
                walk(sub, &format!("{path} {name}"));
            }
        }
        walk(&mut Cli::command(), "fleet");
    }

    /// The control the arm above needs: a page that renders to nothing would
    /// satisfy it and say nothing, so the root's own page is asserted to hold
    /// every family it dispatches.
    #[test]
    fn the_help_page_measured_above_lists_every_family() {
        let page = Cli::command().render_help().to_string();
        for family in ["observe", "seat", "event", "pack", "guard", "doctor"] {
            assert!(
                page.contains(family),
                "`fleet --help` omits {family}: {page}"
            );
        }
    }
}
