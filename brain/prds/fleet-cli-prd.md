# PRD: fleet-cli

**One sentence:** one binary, `fleet`, is the whole surface a person and a seat
touch — nine families of commands over two nouns, where `seat` is what is done
to a seat and `event` is what a seat says, every verb writes its record and
reads it back, and nothing under the surface assumes which agent runs the
session.

Drafted 2026-09-08, from a sitting that walked every built and planned command
against the controller PRD (`fleet-controller-prd.md`), the packs PRD
(`fleet-packs-prd.md`), the naming doc (`naming.md`) and the cli as landed that
day. The two PRDs each name the verbs their side needs; neither owns the
surface, and the spelling had already drifted between them. This page owns
it. Every command below says what it does, what it takes, what it refuses and
with which exit, what it writes, which crate implements it, and where it
stands: **built**, on the **board** at a flight, or **P1** / **P2** as the
owning PRD places it. The reference fleet's tools are cited as the shape a
command distills (**R**), never by name.

---

## Problem statement

A person meets fleet through its commands and nothing else. The controller is
a service, the packs are folders, the stream is a file; the only door to all
three is `fleet <verb>`. When the door is drawn by two documents from two
sides, the same thing gets two spellings and the same word gets two meanings:
the seat lifecycle writers landed under `fleet seat` where the reader
family was going to be `fleet event`, `clear-halt`, `spawn`, `feed` and
`retire` were top-level words with no rule behind them, and `revive` named
both a controller verdict and a candidate verb. Onboarding promises that a
person needs two words after setup — flight and autopilot — and that promise
only holds if every other word sits where a person would guess.

## Goals

1. **One surface, one document.** Every command fleet has or will have is on
   this page, explained in full. A command not here is not yet a command.
2. **Two nouns, one rule each.** `fleet seat <verb>` is what is done *to* a
   seat by a person, a routine or the controller's own effects. `fleet event
   <verb>` is what a seat *says*, written to the stream for the controller to
   consume, plus the readers over that stream. Nothing else takes a noun
   except `pack` and `routine`, which are objects a person manages.
3. **The item is the unit.** The thing the verbs move is an **item**, and the
   id they take is an item id. The work graph beneath it is bd today; the word
   survives a second store.
4. **The most-typed words are the shortest.** `dispatch`, `deliver`, `review`,
   `land` and `run` are top-level, because they are what a person and a seat
   type most and because they run outside flights too.
5. **Every verb reads back what it wrote.** A command that writes the stream,
   the lock, the projection or a note confirms the write before exiting zero;
   an exit that cannot say what happened is a third answer, never a guess.
6. **The provider appears in one slot.** No verb, template or key outside the
   controller's adapter and the pack's per-provider overlay assumes the
   session is any particular agent's.

## Non-goals

- **A shell of its own, a TUI, a REPL.** The cli is verbs and exits. The
  manager is the screen.
- **Composition, pricing or reporting of flights.** How a fleet composes a
  themed flight, prices a window or writes a report page is tiny's, in its own
  workflows over `fleet run` (workflows-formula-fate, 2026-09-17).
- **A second command name.** There is `fleet`. Symlinks, aliases and short
  forms are a person's own.
- **Interactive prompts anywhere but `create`.** Every other verb takes its
  arguments and refuses with a sentence naming the missing one.
- **Removing the controller's own verdict vocabulary from the seat's view.**
  `revive`, `halt`, `spawn-woken` and their kin are the controller's words for
  what it decided, printed by `status`; they are not verbs a person runs.

## Recorded directions

Rulings this page is built on, each from the 2026-09-08 sitting unless dated
otherwise:

- **The seat writers move under `event`.** The four lifecycle writers are
  `fleet event woke|rest|handed-off|exited`; the landed `fleet seat` spelling
  is a usage error naming the new one.
- **`seat` is done-to.** `spawn`, `feed`, `retire` and `nudge` live under
  `fleet seat`.
- **`clear-halt` is an event.** A person's request to lift a halt is written
  to the stream and consumed on the tick, like a rest. It is not spelled
  `revive`, because `revive` is already a verdict.
- **`start` and `stop` take the controller.** `install` leaves the surface;
  its work runs inside the installer or on the first `start`, decided in the
  install conversation.
- **`runway` folds into `status`.** Remaining context is a section of status,
  not a command (and the word is not carried over: `naming.md`).
- **`item` is the word.** Declined: `task` (bd's own issue type), `work` (no
  count noun), `cargo` (an analogy, and the Rust tool's name).
- **The four verbs stay top-level.** Declined: nesting them under an item
  noun for symmetry with `seat` and `event`.
- **The courier verb is `nudge`.** The naming doc already names waking a live
  seat with a message `nudge`; one word for one mechanism, the controller's
  threshold nudge and a person's message on the same adapter path.
- **`pack` gains `remove` and `list`.** The lock is the one record and a
  directory deleted by hand leaves a line `add` reads as installed.
- **The event family has a reader half.** `tail` with a resumable sequence and
  `show` by id, so one reader serves a person at a shell and the manager's SSE
  transport at P1.
- **Provider-neutral, ruled the same day:** "we will support many llm
  providers." The guards' payload reader is the provider adapter in the cli,
  never in core; the controller's start flags are the adapter's alone.
- **Every command explained in detail** — the standing instruction for this
  page, verbatim.

## Design overview

### The binary and its crates

`fleet` is one binary from a thin cli crate that routes each verb to one of
two library crates and owns nothing itself. The cli resolves what only a
process knows — the machine directory, the current directory's `fleet.toml`
or `.fleet/project.toml`, the clock, stdin — and hands values to pure
functions.

| Crate | Owns |
| --- | --- |
| **fleet-controller** | the tick (observe, decide, effect), the event stream's append and read, the projection, the session table, the platform layer (machine directory, user service, constructed PATH), the provider adapter, routines |
| **fleet-core** | the pack format, the lock, the layer resolver, the shadow registry, the policy reader, the guard classes, the four verbs and `brief`, and `run` |
| **fleet-cli** | argument parsing, usage text, exits, the provider payload adapters for the hooks, routing |

The rule that keeps two slices from editing one function: the cli's dispatch
is one arm per family, each arm a call into a module named for the family,
and the family module is where subcommands are matched.

### The two nouns

```
fleet seat  <spawn|feed|retire|nudge> <seat> …       done to a seat
fleet event <woke|handed-off|exited> <seat>          said by a seat
fleet event <rest|clear-halt> <seat> [--reason <text>]   said by a seat
fleet event step <started|closed> --run <id> --n <n> --name <name> …   said by a workflow
fleet event <tail|show> …                            read from the stream
```

The line between them is who holds the hands. A seat has a voice and no
hands: it writes what it asks and the controller acts. A person, a routine or
an effect has hands: `seat` verbs start, feed, retire and message a session
through the adapter. `clear-halt` sits under `event` because it is a request
the controller consumes, not an act performed on the seat.

### The families

| Family | Commands | Crate |
| --- | --- | --- |
| lifecycle | `create` · `start` · `stop` · `status` | controller |
| seat | `spawn` · `feed` · `retire` · `nudge` | controller |
| event | `woke` · `rest` · `handed-off` · `exited` · `clear-halt` · `tail` · `show` | controller |
| item verbs | `dispatch` · `brief` · `deliver` · `review` · `land` · `ask` · `answer` | core |
| flight | `plan` · `fly` · `autopilot on\|off` — removed 2026-09-17 (workflows-formula-fate) | — |
| hooks | `guard shell-trap\|record` · `prime` | core, behind a cli adapter |
| routines | `routine list\|check\|run\|history` | controller |
| packs | `pack add\|check\|remove\|list\|search` | core |
| controller | `observe` · `--version` | controller |

### Exits

Every command shares one exit vocabulary, so a script reading `$?` learns the
same thing from every verb:

| Exit | Meaning |
| --- | --- |
| 0 | done, and the write (if any) read back |
| 1 | refused on the record: the thing named is absent, held, or already so |
| 2 | usage: a missing or unknown argument, with the usage line printed |
| 3 | could not tell: an instrument the answer needs was unreadable; nothing changed |
| 4 | the seat has no live session |
| 5 | no collector is consuming the stream (the projection is missing or stale) |
| 6 | the row is transient where a named seat was required |

Exits 4, 5 and 6 are the stream's refusals, shared by every verb that writes
to it or acts on a session. Exit 3 is the third answer: a verb that would
have to guess prints what it could not read and stops.

### The JSON envelope

Every verb the SDK calls carries `--json`, and under it stdout is one document
and nothing else:

| Outcome | Document |
| --- | --- |
| success | `{"ok":true,"verb":"<verb>","data":<value>}` |
| refusal | `{"ok":false,"verb":"<verb>","refusal":{"code":"<class>","why":"<text>"}}` |

The document is the outcome and not a rendering of it, which is the rule
`fleet status --json` already states for its own flag. The `code` names the
exit table's row in snake case — `refused`, `usage`, `could_not_tell`,
`no_session`, `no_collector`, `transient` — rather than repeating its number,
because the number is on `$?` and a second spelling of it is a second thing to
keep in step. **The exit codes are unchanged by the flag**: a verb that refuses
exits its own row whether or not a document was asked for, and the line it
prints for a person on stderr is printed under the flag too.

The key order is fixed: `ok`, then `verb`, then `data` or `refusal`. A caller
reading the bytes rather than a parse tree reads the same bytes from every verb.

`fleet event tail` and `fleet event show` carry the flag first, because the
SDK's replay reads the stream through those two; `fleet seat spawn`, `feed` and
`retire` carry it next, because the SDK's spawn and retire steps run through
those. One later row adopts it:

- **Item verbs gain `--json` for the SDK: dispatch, deliver, review, land, ask,
  answer** — each prints the envelope above, its `data` built from the values
  core returns to the cli and never from the human text. Every one carries
  `item` and the `state` it moved the item to, which is the stream kind the verb
  writes without its `item.`/`gate.` prefix; each verb's own section names what
  it carries beside them. Under the flag the human rendering moves to stderr, so
  stdout is the one document. **Built.**

A verb's `data` carries what its own controller value holds and nothing this
binary went and looked up for the document: a document assembled from a second
read is a second thing that can disagree with the verb's own answer.

### What every verb refuses to guess

Inherited from the packs PRD and applied to every family: no verb resolves a
branch name where a commit is meant; no verb infers an item id from context;
no verb writes to a session's files; no verb reads the process's own PATH to
find the agent; no verb emits `allow` from a hook. A verb that needs a value
it was not given exits 2 and names it.

---

## The commands

Each entry: what it does; arguments; refusals and exits; what it writes;
crate; state.

### Lifecycle

### `fleet create`

Creates a fleet from inside a project. The one interactive command: it asks
**embedded or standalone** and **which agent**, then writes the config for the
mode. Embedded: one `fleet.toml` at the project root, the smallest working
file naming the seats and nothing else, every other key defaulted by the
controller. Standalone: the project's `.fleet/project.toml`, and the project
registered with the standalone fleet the machine already runs. Materializes the
binary's own **defaults** into the machine directory's `defaults/` and installs
NO pack — refreshing a copy whose files are not the set this binary carries, and
refusing one no lock line accounts for or one edited since the line that pinned
it, because the set is pinned at the binary's own version and that version
cannot say which bytes are on disk. On a machine a previous binary left, the
bundled core pack it installed is retired with its lock line in the same act,
and one line says so. Writes the `[guards]` table with every guard on and says
so in one line. Its done message names the command
that creates the first seat — an architect on the chosen provider — and that
command's done message says the controller is not running and names `fleet
start`.

- Arguments: none required; `--embedded` / `--standalone` and `--agent
  <name>` skip the questions for scripts.
- Refuses: a directory already holding a `fleet.toml` or `.fleet/` (exit 1);
  standalone with no registered fleet on the machine (exit 1, naming `fleet
  start`); an agent the adapter does not know (exit 2).
- Writes: the config file for the mode, the machine directory's `defaults/`,
  the lock, one `project.registered` event in standalone mode.
- Crate: controller (mode, registration), core (the defaults).
- State: **board**, flight 7, with the install work (below) folded in.

### `fleet start`

Brings the controller up: loads the platform's user service under the fleet
label — a launchd agent on macOS, a `systemd --user` unit with lingering on
Linux — and confirms the service is running by reading a fresh
`controller.started` event, never by the load command's own exit. On the
first run on a machine it performs what an install verb would have: writes the
service file, creates the machine directory, asks the telemetry question
(off, and asked), and on macOS requests the file-access grant so it is
answered in the minute the service loads. EVERY start materializes the
binary's own defaults the same way `create` does, and says in one line which of
the three happened — installed, refreshed, already — because `create` refuses a
root that already has a `fleet.toml` and is therefore no route to the set a
rebuilt binary carries. A refusal there is that line and never a fourth refusal
of `start`: the controller comes up on a machine whose defaults this binary
will not write over. Whether that first run lives here or
in the installer script is decided in the install conversation; either way
there is no `fleet install` on the surface.

- Arguments: `--foreground` runs the loop in this process instead of the
  service (the same thing as `observe` without `--once`, kept for a person
  watching one machine). The foreground loop **is** the service's loop,
  workflows included: it is wired with the same run seam, so a waiting run is
  re-run, gated and cleaned under it exactly as it is under the service.
- Refuses: a service already running (exit 1, printing its pid and the last
  tick); an agent binary unresolvable on the constructed PATH (exit 3, the
  loop then runs with effects off and says so).
- Writes: the service file on first run; the machine directory's `defaults/`
  and its lock line where the set the binary carries is not the one
  materialized; `controller.started`.
- Crate: controller (platform layer).
- State: **board**, flight 7 (the install-and-create frame).

### `fleet stop`

Brings the controller down: unloads the user service and confirms with a
`controller.stopped` event read back. Seats are not touched — a stopped
controller leaves sessions running and the stream unconsumed, which is the
alarm `status` shows on the next start.

- Arguments: none.
- Refuses: no service loaded (exit 1).
- Writes: `controller.stopped`.
- Crate: controller.
- State: **board**, flight 7.

### `fleet status`

Prints the projection: the roster with each seat's state, the controller's
last verdict and outcome per seat, `in_flight` and `effects`, the grant on
macOS (GRANT PENDING above the roster when unanswered), the halt latches, and
a **context** section — each seat's context against the rest threshold and
the window left, the reading the reference called runway. Reads the
projection file and the policy in force, never the process table; a
projection older than three polls is printed with its age in the first line.
The switch and the backlog it printed above the roster left with `autopilot`
and `plan` (removed 2026-09-17, workflows-formula-fate).

- Arguments: `--json` prints the projection document verbatim; `--seat
  <name>` prints one row.
- Refuses: no projection (exit 5, naming `fleet start`).
- Writes: nothing.
- Crate: controller (projection), core (the rules table).
- State: **board**, flight 8.

### Seat

### `fleet seat spawn --first-turn <file> [--json]`

Creates a transient seat. The load belt first: the five-minute load average
against a per-CPU ceiling, and transient seats mid-turn against a cap, from
policy; an unreadable roster makes the cap leg could-not-tell and refuses
nothing while the load leg keeps its teeth. Then a fresh worktree from the
project's `origin/main`, a roster row with `transient: true`, and the file's
text as the session's first turn through the adapter's `start`. A rollback on
a failed start removes the worktree and never a branch. Writes nothing to the
work graph; the caller (`dispatch`) does.

- Arguments: `--first-turn <file>` required; `--project <name>` in
  standalone mode; `--model <id>` overriding policy's default; `--json`
  printing the envelope instead of the name.
- Under `--json`: `data` is `seat`, `worktree`, `belt` — the belt's two
  readings as the stream carries them, not the two sentences — and `base`, the
  commit the cut was read back at, `null` where that read would not answer. The
  document REPLACES the name on stdout; the belt and the worktree stay on
  stderr for a person.
- Refuses: load over the ceiling (exit 1, printing the reading and the
  ceiling); the cap reached (exit 1); an unreadable roster for the cap (a
  could-not-tell line, not a refusal); a failed start inside the watch window
  (exit 1, the output file named). A leg that could not read what it needed is
  exit 3 and the code `could_not_tell`, distinct from the `usage` a call this
  process cannot read answers with, so a caller never reads a question as a
  verdict.
- Writes: `session.spawned`, or `session.crashed` with phase `start`.
- Crate: controller.
- State: **board**, flight 6.

### `fleet seat feed <seat> --first-turn <file> [--json]`

Hands a live transient seat its next first turn and moves the row's occupant
marker, put-back on failure, every row move journaled. Refuses a seat still
marked as holding a turn. The work-graph writes around it are the caller's.

- Arguments: `<seat>` and `--first-turn <file>` required; `--json`.
- Under `--json`: `data` is `seat`, `prior_first_turn` and `first_turn` — the
  two turns as their first lines, the shape the `session.nudged` journal
  carries them in, because the whole text is the session table's.
- Refuses: a seat holding a turn (exit 1); no live session (exit 4); a named
  seat (exit 6, since named seats are rung, not fed).
- Writes: `session.nudged` carrying the delivery outcome, the row move in the
  session table.
- Crate: controller.
- State: **board**, flight 6.

### `fleet seat retire <seat> [--dead] [--by <name>] [--json]`

Ends a transient seat and verifies from outside that nothing of it holds RAM
or disk: the session stopped and removed, the worktree gone, no process under
its name. Prints the reclaim. `--dead` licenses the removal of a seat whose
session is already gone, by a completed roster read that names no live
session, never by silence.

It also withdraws the orders the seat still holds, because the name it frees
is the one the next spawn takes: every OPEN item assigned to the seat that
carries an orders key is cleared of its assignee and its order index and gets
one note saying so, all of it before the seat-list row comes off. A withdrawal
that cannot be written stops the retire with the row — and the name — still
standing. A seat holding nothing ordered retires exactly as it did before.

- Arguments: `<seat>` required; `--dead`; `--by <name>`, who the withdrawal is
  written by (else `FLEET_ACTOR`, `BEADS_ACTOR`, else the fleet itself); `--json`.
- Under `--json`: `data` is `seat`, `worktree`, `bytes` and `pid` — `null` for
  a directory this verb could not walk or a roster row that named no pid, never
  a zero — plus `branch`, what became of the work branch: its `name` (`null`
  where the seat stood on none), a `disposition` of `deleted` or `kept`, and
  the `why` behind that word.
- Refuses: a surviving resource (exit 1, naming it); an unreadable roster
  (exit 3); a named seat (exit 6; named seats rest); a withdrawal that did not
  land (exit 3, naming the item and saying the seat-list row stands).
- Writes: `session.stopped`, the row removed from the table, and on every item
  the seat still held under an order: the assignee cleared, `metadata.orders`
  unset, one `ORDER WITHDRAWN at retire` note.
- Crate: controller.
- State: **board**, flight 6.

### `fleet seat nudge <seat> --text <text>`

Delivers one message to a seat's live session through the provider adapter's
nudge, the same path the controller's threshold nudge uses, and records the
outcome. The text is delivered verbatim. The verb carries no authority and
its usage line says so: a nudge is a doorbell, and the seat acts on its
record, not on the message. Routines use this verb to ring a seat on a
schedule.

- Arguments: `<seat>` and `--text` required; `--timeout <s>` over policy's
  nudge timeout.
- Refuses: no live session (exit 4); no collector (exit 5); a delivery the
  adapter reports failed (exit 1).
- Writes: `session.nudged` with the outcome.
- Crate: controller.
- State: **P1** in the packs PRD; the frame is on the board at flight 8.

### Event

### `fleet event woke <seat>`

Records that a seat started and oriented. A record only: the controller folds
it into the seat's last lifecycle event and acts on nothing. Reads the stream
back and confirms the last line is its own before exiting zero.

- Arguments: `<seat>` required.
- Refuses: a read-back that does not match (exit 2, saying so); an actor
  naming no config row is written and dropped by the consumer with one log
  line, not refused here.
- Writes: `seat.woke`.
- Crate: controller (the events module); cli routes.
- State: **built** under the `seat` spelling; moves to `event` on the board
  at flight 4.

### `fleet event rest <seat> [--reason <text>]`

A seat's request to be rested. The controller, on its next tick, stops the
live session, starts a woken successor in the same worktree with the first
turn, then removes the predecessor's row, and writes `session.rested` once
the successor's start returned. Refuses, naming which half failed: no
collector consuming (exit 5); no live session (exit 4); a transient row
(exit 6, naming `fleet seat retire` instead — only named seats rest). The
alarm is visible in the stream: a `seat.resting` with no `session.rested`
after it.

- Arguments: `<seat>` required; `--reason <text>`.
- Writes: `seat.resting` with the reason in the payload.
- Crate: controller.
- State: **built** under `seat`; moves at flight 4.

### `fleet event handed-off <seat>`

Records that a seat finished its handoff — its record complete for a
successor. A record only, like `woke`.

- Arguments: `<seat>` required.
- Writes: `seat.handed_off`.
- Crate: controller.
- State: **built** under `seat`; moves at flight 4.

### `fleet event exited <seat>`

Records a deliberate end. The point is the next decision: a seat absent with
an `exited` behind it gets a fresh spawn-woken successor; a seat absent with
no such event is a crash and gets `revive`. The event is consumed by the
tick.

- Arguments: `<seat>` required.
- Writes: `seat.exited`.
- Crate: controller.
- State: **built** under `seat`; moves at flight 4.

### `fleet event clear-halt <seat>`

A person's request to lift a halt. Three consecutive blind dispatches halt a
seat: the controller stops dispatching it, leaves it down and says why once;
the counter persists across restarts and decays rather than clearing. This
event is the only remedy — "I looked, try again" — consumed on the next tick,
which resets the latch and announces the transition once. It is written like
any seat event so the stream shows who asked and when.

- Arguments: `<seat>` required; `--reason <text>`.
- Refuses: no collector (exit 5); a seat not halted (exit 1, printing its
  state).
- Writes: `seat.clear_halt`; the controller answers with `session.halted`'s
  counterpart on the transition.
- Crate: controller.
- State: **board**, flight 5 (the halt and adopt frame).

### `fleet event step <started|closed> --run <id> --n <n> --name <name> [--result <json> | --sha <hex>]`

The one writer of the step pair, for a workflow's own process (fleet-layers
§ The replay contract). `started` is written before a step runs and `closed`
after it, both carrying the run, the number and the name; a close carries the
result as one JSON value or, over the SDK's 64 KiB cap, the sha256 of
`steps/<n>.json` in the run directory. The actor is `FLEET_ACTOR` or
`BEADS_ACTOR`, else the run — the child a run starts carries no actor
variable. Reads the stream back and confirms the last line is its own before
exiting zero. Nothing in the controller consumes the pair: its reader is the
SDK, through `fleet event tail --json --type step.closed`.

- Arguments: the phase, `--run`, `--n` and `--name` required; `closed` takes
  exactly one of `--result <json>` and `--sha <hex>`, `started` takes neither.
- Refuses: a result that is not one JSON value, a start carrying a result, a
  close carrying none, and a read-back that does not match (all exit 2).
- Writes: `step.started`, `step.closed`.
- Crate: cli (the step module), over the controller's event log.
- State: **built**.

### `fleet event tail [--follow] [--since <seq>] [--seat <name>] [--type <t>] [--json]`

Prints lines of the stream from the sequence after `--since` (default: the
last fifty), filtered by actor and by type, one JSON object per line exactly
as stored — or, under `--json`, one envelope per line instead, its `data`
carrying the record whole (`id`, `seq`, `ts`, `kind`, `actor`, `payload`),
because the SDK replays the record and not the file's bytes.
`--follow` keeps the file open and prints new lines as they are
appended, ending on interrupt with exit 0. A timestamp given to `--since`
resolves to the first sequence at or after it and the resolved sequence is
printed once on stderr, so a script can pass it back. The sequence is the
resumable id the manager's SSE transport wants at P1: one reader for both.

- Arguments: `--json` prints the envelope, one document per line.
- Refuses: no stream (exit 5); a `--since` past the end prints nothing and
  exits 0. Under `--json` a refusal is the envelope's refusal document.
- Writes: nothing.
- Crate: controller (the events module's reader).
- State: **built**.

### `fleet event show <id> [--json]`

Prints the one event with that id, pretty-printed. An absent id exits 1
naming it; an id matching more than one line is a corrupted stream and exits 2
naming both sequences, because ids are unique by construction and a duplicate
is a fact worth stopping on.

- Arguments: `--json` prints the envelope carrying the record whole, in place
  of the pretty-printed event; a refusal is the envelope's refusal document and
  its exit is unchanged.
- Writes: nothing.
- Crate: controller.
- State: **built**.

### Item verbs

The four verbs are pack tools with written contracts, each leaving its note
on the item so a successor reads the item and never a message. They run
outside any flight; `fly` runs them in a batch.

### `fleet dispatch <item> [--to <seat>] [--json]`

Gives a ready item to a seat. Writes the order note on the item — the one
record that says a seat may begin — and its machine-read index; then either
rings a named seat with the brief, or asks the controller to spawn a
transient seat with the brief as its first turn. Refuses three things and
names each: an item that is not ready (a blocker open, a hold on it or its
parent); an item already carrying an order; a seat already holding one.
Read-back and own-exit are the rule from this verb on: it reads the note back
before exiting.

- Arguments: `<item>` required; `--to <seat>` names a live seat, else a
  transient seat is spawned; `--json` prints the envelope, its `data` carrying
  `item`, `state` (`dispatched`) and `seat` — the seat the order named, or
  `null` where a transient one was spawned.
- Refuses: not ready (exit 1, the blocker named); already ordered (exit 1);
  the seat holds an item (exit 1); load or cap (exit 1 from `spawn`).
- Writes: the order note and index on the item; `session.spawned` or
  `session.nudged` from the controller.
- Crate: core.
- State: **board**, flight 4.

### `fleet brief <item>`

Renders the brief for an item: the text a dispatched seat reads first —
the item, the order note, the delivery-note template, the every-turn rules
from the resolved layers. Printed to stdout so `dispatch` can hand it to
`spawn` as a file and a person can read what a seat will read. The template
is a default and the shadow registry lists it, so a pack on top replaces it
whole.

- Arguments: `<item>` required.
- Refuses: an item with no order note (exit 1); an unresolved pack layer
  (exit 3).
- Writes: nothing.
- Crate: core.
- State: **board**, flight 4.

### `fleet deliver [--json]`

From inside a seat's worktree: commits the work, writes the delivery note on
the item with its three machine-read lines (the commit, the spec corrections,
the decisions), reassigns the item to the reviewer `[core] reviewer` names, and
reads the note back. The seat's own verb; the only writer of a delivery.

The note is the seat's and is handed in with `--note <file>`, in the pack's
delivery-note grammar. The verb fills the three lines only a process knows —
the commit, the branch and the base — and carries every other line through
verbatim, because what the note says about the work is the seat's word.

- Arguments: `--note <file>` required; the item is read from the worktree's
  order, and `--item <id>` names it where a seat holds two; `--json` prints the
  envelope, its `data` carrying `item`, `state` (`delivered`) and `commit` —
  the commit the delivery made.
- Refuses: the trunk (exit 1); an unclean tree beyond the delivery's own files
  (exit 1); an empty staged set (exit 1); no ordered item on the worktree
  (exit 1); a note the grammar cannot anchor on (exit 2); a read-back that
  disagrees (exit 3).
- Writes: the commit on the work branch; the delivery note.
- Crate: core.
- State: **board**, flight 5.

### `fleet review <item> [--json]`

Prints what a reviewer needs and records the verdict: the delivery note,
the diff stat from the base the delivery recorded (its base: line, from the
merge-base; the commit's parent where the note names none), the size tier
and the floor it sets, then takes `--land` or `--return <findings-file>` and
writes the verdict note. Returns carry a findings count as their first line,
because a return with nothing numbered is a question and goes back as one. A
pack defines the per-tier review; core ships one reviewer's read, whose size
line is a measurement and names no tier.

**`--land` lands nothing.** It writes the `ACCEPTED` verdict that `fleet land`
reads; the squash, the push and the close are that verb's. The flag is named
for what the reviewer has decided, not for the act that follows it.

- Arguments: `<item>` required; one of `--show` (default), `--land`,
  `--return <file>`; `--json` prints the envelope, its `data` carrying `item`
  and `state` — `reviewed` under `--land`, `returned` under `--return`, and
  `null` under `--show`, which writes no verdict and moves the item nowhere.
- Refuses: no delivery on the item (exit 1); a return file with no numbered
  findings (exit 2).
- Writes: the verdict note.
- Crate: core.
- State: **board**, flight 5.

### `fleet land <item> <commit> [--json]`

The reviewer's verb. Takes a commit, never a branch name; gates on a current
trunk (`origin/main` fetched, the base not behind), on the staged set equal to
the delivery's own files, and on the suites the pack's gate names; squashes
onto main with the item id in the subject; pushes; deletes the work branch
only when its classification is safe; closes the item with the landed sha.
Every gate row is printed, rendered from what the tool read, so a person can
re-run each one. From a workflow the landing tree is the reviewer's worktree:
a workflow's verbs run at the registered project's root, so a landing handed
the primary resolves the `[core] reviewer` seat's worktree for this project
out of the machine's seat table and runs the whole act there — and refuses,
naming the seat and that table, where the reviewer has no worktree for it.
From a workflow the landing is the reviewer's act carried by the run: the verb
is called as the run's record, so it acts as the `[core] reviewer` — the holder
gate reads that seat, the close and `item.landed` carry it with the run named
beside it, and the reviewer's own answer to this run's gate is what licenses
it.

- Arguments: `<item>` and `<commit>` required; `--also <path>` admits a
  reviewer's own file into the staged set; `--json` prints the envelope, its
  `data` carrying `item`, `state` (`landed`) and `sha` — the sha the push's own
  range line named.
- Refuses: a branch name where a commit is meant (exit 2); a reviewer with no
  worktree for this project (exit 1); a resolved tree that is a primary too
  (exit 1); base behind (exit 1); a staged set beyond the delivery (exit 1); a
  red suite (exit 1, the row printed); a push rejected (exit 1, and nothing
  after the push runs); a run whose gate nobody answered (exit 1).
- Writes: the commit on main, the close on the item.
- Crate: core.
- State: **board**, flight 6.

### `fleet ask --note <file> [--json]`

From inside a seat's worktree: the seat's one way to need a person without
waiting for one. Commits what the seat has to its branch, raises a gate on the
item — the store's own gate object, type human, carrying the note's question
and its lettered options — records the branch and the commit on the item,
writes `item.parked`, and exits. The flight retires the seat; the item leaves
the flight and the ready set until the gate is resolved, and the next flight
that lists it dispatches a fresh seat from the parked commit with the question
and the answer in its brief (the flights page, S4).

- Arguments: `--note <file>` required, in the pack's question grammar: one
  question, lettered options, one line each; `--json` prints the envelope, its
  `data` carrying `item`, `state` (`parked`) and `gate` — the gate the ask
  raised.
- Refuses: the trunk (exit 1); no ordered item on the worktree (exit 1); a
  note with no lettered option (exit 2); a read-back that disagrees (exit 3).
- Writes: the commit on the work branch; the gate; the park on the item;
  `item.parked`.
- Crate: core.
- State: **board**, flight 8.

### `fleet answer <item> <letter> [--text <text>] [--json]`

A person's reply to a gate, from the cli or the cockpit. Writes the answer on
the item — the letter, and the text when one is given — resolves the item's
open gate, writes `gate.resolved`, and reads both back. The item is ready
again; nothing is dispatched by this verb. The store's gate list is the
decisions list, so `fleet answer` is the one act that empties it.

- Arguments: `<item>` and `<letter>` required; `--text` for an answer the
  options did not carry; `--json` prints the envelope, its `data` carrying
  `item`, `state` (`resolved`) and `gate` — the gate the answer resolved.
- Refuses: no open gate on the item (exit 1); a letter the gate's options do
  not name and no `--text` (exit 2).
- Writes: the answer note; the gate's resolution; `gate.resolved`.
- Crate: core.
- State: **board**, flight 8.

### Flight

**Removed 2026-09-17, ruling `workflows-formula-fate`** (fleet-layers.md Q10,
§ What moves). The three verbs below left core: composition is tiny's preboard
and takeoff workflows over `fleet run`, and the run lifecycle keeps what `fly`
pinned — the directory, the hash on the record, the cap, the lock, resume and
retire-all. Each refuses as an unknown subcommand (exit 2). The text is kept as
the record of what was built.

### `fleet plan <items...> | --ready N`

**Removed 2026-09-17 (workflows-formula-fate).** Wrote a planned flight: the record item — type task, label `flight`, titled
by its id — with the list in its metadata, and `flight.planned` on the stream.
Nothing is pinned and nothing starts; a plan waits in the backlog, oldest
first, until `fly` or autopilot opens it. `--ready N` takes the N oldest ready
items at plan time, which is how a fleet with nobody composing flies: one
routine calling this verb nightly. Core plans nothing on its own.

- Arguments: item ids, or `--ready N`; never both.
- Refuses: an item the work graph does not call ready (exit 1, the blocker
  named); an item already on an open flight or another plan (exit 1); an
  empty list (exit 2).
- Writes: the record item; `flight.planned`.
- Crate: core.
- State: **removed**, 2026-09-17, workflows-formula-fate.

### `fleet fly [<flight>] [--seats M]`

**Removed 2026-09-17 (workflows-formula-fate).** Took off with the named flight, or the oldest plan. Pins every input into
`flights/<id>/` under the machine directory — the list and each item's hash
and `flight.*` object, the policy snapshot, the lock, the trunk commit and the
strategy, the agent's pinned and live versions, the model per role, the
review policy, the account, the load, a brief per item — hashes the directory
onto the record item and `flight.opened`, and returns. The controller's tick
advances the flight from there: every tick derives each item's state from the
record and takes at most one act per item, until every item is landed or
parked, when the close writes `flight.closed` and the summary file. With no
controller running, this verb runs the same loop in the foreground until the
flight closes, progress on stderr past two seconds. The verb is `fly`, the
noun is `flight`, and there is no `fleet flight` command.

- Arguments: `<flight>` names a plan, else the oldest; `--seats M` overrides
  `[core.flight] max_seats` for this flight.
- Refuses: no plan (exit 1); an input it cannot read, named (exit 3, nothing
  written); an item no longer ready (exit 1); open flights at
  `[core.flight] max_open` (exit 1); a trunk strategy it does not implement,
  named (exit 1).
- Writes: the flight directory; the pins on the record item; `flight.opened`;
  then, per tick, the acts' notes and events.
- Crate: core; the tick was the controller's, calling core's advance.
- State: **removed**, 2026-09-17, workflows-formula-fate.

### `fleet autopilot on|off`

**Removed 2026-09-17 (workflows-formula-fate).** The switch. On, the tick opens the oldest plan while open flights are under
`[core.flight] max_open`, default one; off, nothing opens. An empty backlog
opens nothing and `status` says so. The state is one file in the machine
directory, and `status` prints it above the roster.

- Refuses: nothing; setting a state already set exits 0 and says so.
- Writes: the switch file.
- Crate: core.
- State: **removed**, 2026-09-17, workflows-formula-fate.

### Hooks

Two verbs a provider's hook file calls, bare, because the pack's overlay puts
the binary on the session's PATH. Both read the provider's payload through an
adapter in the cli — the field names of one agent's hook contract are that
adapter's and never core's — and call pure functions in core.

### `fleet guard <shell-trap|record> [--check]`

Judges one pre-tool payload read from stdin. **shell-trap** refuses the five
shell traps that read as green: a record-writing command with a backtick in
its string argument, a colon modifier after an unbraced variable, a list
command given one bare variable, a status read after a pipeline whose last
stage only formats, and a false-alternative chain over a command with a third
exit. **record** refuses a work-graph write that replaces an append-only
field, a write statement through the graph's SQL route, and a bare item id in
free text on a note-writing surface. Each refusal prints the class, the
fragment, the rewrite and the escape. A guard never emits allow; silence is
allowing. A refusal is one JSON object on stdout with exit 0, because a crash
signalled by exit code fails open. Opt-out per guard in `fleet.toml`'s
`[guards]`; a check whose target is not configured (the bare-id check needs
the project's item prefix) refuses nothing and says so under `--check`.

- Arguments: the class; `--check` prints one line per check, configured or
  not, exit 0 when all are and 1 otherwise, which the doctor entry runs.
- Escapes: one leading assignment per class on the command itself.
- Writes: nothing.
- Crate: core (the lexer and classes); cli (the payload adapter).
- State: **built**.

### `fleet prime`

The session-start hook. Prints the every-turn rules for a seat — the guards
in force, the verbs and their exits, the item the worktree holds if any —
from the resolved pack layers, so a session that has read nothing else knows
the rules on its first turn. The provider's overlay wires it; the text is
a default and the registry lists it.

- Arguments: none; reads the current directory's config.
- Refuses: nothing; with no fleet config it prints one line saying so and
  exits 0, since a hook that fails open is the contract.
- Writes: nothing.
- Crate: core.
- State: **board**, flight 4 (the plugin-shape frame).

### Routines

`orders/<name>.toml` from every pack and every project, evaluated on the
controller's tick: cron or cooldown, a condition with its check timeout and
its unknown-exit reading, and an action — a nudge, a work-graph write, an
exec, or a run. One ledger row per tick with could-not-tell as a third
outcome. core ships no routines.

Renamed from `order` on 2026-09-18 (fleet-layers Q3: the word was overloaded
with the dispatch record's "orders given", which keeps it). The verb, the
events and this page say routine; the file (`orders/<name>.toml` carrying
`[order]`), the state file, the projection's array and the events' `order`
payload key keep the file format's word until that format is renamed. `fleet
order …` is refused with exit 2 and one line on stderr pointing at `fleet
routine` for one release.

`[action.run]` is the fourth action kind: `workflow` names a workflow as
`fleet run` resolves it, and `[action.run.inputs]` is a table of strings, one
`--input key=value` each. Firing it calls `fleet run <workflow> … --by
<routine>` in the routine's project root, so `run.started` names the routine
as its actor and the routine's `fired` and `completed` wrap the run's `started`
and `closed` on the one stream; the terminal event carries the run id as `run`
(the opening one cannot: the store names the run when it opens). The verb's
exit is the outcome — 0 `ran`, 1 `failed`, 3 `could-not-tell` — and both of its
streams go to the routine's `run` log beside an exec's.

### `fleet routine list`

Prints every routine the controller has loaded: name, source pack or project,
trigger, next due, last outcome.

- Writes: nothing. Crate: controller. State: **board**, flight 4.

### `fleet routine check <name>`

Evaluates one routine's condition now and prints the three-valued answer — due,
not due, could not tell — without firing it.

- Refuses: an unknown routine (exit 1). Writes: nothing. State: **board**,
  flight 4.

### `fleet routine run <name>`

Fires one routine now, outside its schedule, and prints the ledger row it
produced. Writes `routine.fired` and `routine.completed`, `routine.failed` or
`routine.could_not_tell`; a run action's terminal row carries the run id.

- Refuses: an unknown routine (exit 1); a condition that reads not due unless
  `--force` (exit 1). State: **board**, flight 4.

### `fleet routine history [<name>] [--since <seq>]`

Prints the ledger rows for one routine or all, from the stream's routine events.

- Writes: nothing. State: **board**, flight 4.

### Packs

### `fleet pack add <source> --version <tag|branch|sha:40-hex>`

Fetches one pack by git source and version into the packs directory and pins
it in `packs.lock`. The source may end in `//<subdirectory>`; the clone goes
through the git binary on PATH. Checks the format, refuses a name collision
with an installed pack, and on any refusal removes what it fetched and leaves
no lock line. The lock is the record: source as typed, name, version, commit,
fetched — the name because it is the pack's directory's key, and the only join
from a source to the directory that source installed.

- Arguments: `<source>` and `--version` required; `--packs-dir`, `--lock`.
- Refuses: a format violation (exit 1, each violation listed); a collision
  (exit 1); a ref the remote lacks (exit 1); a tree URL as source (exit 2,
  naming the two-slash form).
- Writes: the pack directory, the lock line.
- Crate: core.
- State: **built**.

### `fleet pack check <dir> [--over <dir>]…`

Validates one pack against the format: the manifest's four keys and three
tables, the eight slots and nothing else at the top level, a `doctor.toml` in
every doctor entry. Prints the pack line, one `runtime <name> <version>` line
when the manifest carries a `[runtime]` table — the pin as parsed, which is
what the pack's doctor entry measures against — and one line per slot. With
`--over`, resolves it above the packs named, in order, and refuses a name
collision or a file shadowing a path the registry does not list.

- Refuses: each violation (exit 1, listed). Writes: nothing. Crate: core.
- State: **built**.

### `fleet pack remove <source>`

The inverse of `add`, keyed by the source as typed: deletes the directory the
lock line names, then drops that line and reads the drop back — in that order,
so a failure leaves a lock line with no directory rather than a directory with
no line. A line with no directory is what a re-run of this verb and a re-add of
the same source both clear; a directory with no line is what `add` refuses as a
collision and this verb refuses as a source it does not hold, which no verb
clears. Refuses the binary's own defaults, a pack another installed pack
imports (naming the importer), a source not in the lock, and a line written
before the lock carried a name, which names no directory.

- Refuses: as listed (exit 1). Writes: the directory removed, the lock.
- Crate: core. State: **built**.

### `fleet pack list`

Prints `packs.lock` as a table — name, source, version, commit, fetched — in
source order, which is the order `add` writes. An empty lock prints the header.

- Writes: nothing. Crate: core. State: **built**.

### `fleet pack search <text>`

Searches a pack registry. Needs the registry, which does not exist; listed so
the word is reserved.

- State: **P2**.

### Controller

### `fleet observe [--once]`

Runs the controller's loop in the foreground: poll the roster through the
adapter, publish the projection and the stream, act on verdicts. `--once` is
one poll then exit, the instrument a person or a test uses to see one tick.
`start` is this loop under the service; `observe` is the same loop in this
terminal.

- Refuses: nothing; an unresolvable agent binary runs with effects off and
  says so.
- Writes: the projection, the stream.
- Crate: controller.
- State: **built**.

### `fleet --version`

Prints the version and exits 0.

- State: **built**.

---

## Requirements

### P0 — the surface as ruled

1. One binary named `fleet`; every command on this page routes through one
   dispatch, one arm per family, subcommands matched in the family's module.
   N.
2. The two nouns hold: no verb under `seat` writes a seat event; no verb
   under `event` performs an effect on a session. N.
3. The exit table above is the only exit vocabulary; a verb that cannot say
   what happened exits 3 and names what it could not read. R.
4. Every writing verb reads its write back before exiting 0. R.
5. `item` is the word in every usage line, note and error; the store's own
   name appears only where the store itself is meant. N.
6. The provider's hook payload is read in the cli's adapter for that
   provider; core's guard functions take command text, a policy table and
   project keys, and nothing else. N (ruled 2026-09-08).
7. The four lifecycle writers under the old `fleet seat` spelling exit 2
   naming the `event` spelling, from the flight that moves it until the first
   release. N.

### P1 — after the first gate

- `fleet seat nudge` as core's, so routines can ring a seat.
- `fleet doctor`: the control lines — the work-graph endpoint file, the
  daemon pid, the grant, the projection's age — and every verb's
  preconditions, as one command with a third answer, running each pack's
  doctor entries.
- Rest grants and roster releases for runs, read off the run's record, as
  `fleet` verbs (the controller PRD's P1).
- `fleet review` size tiers with a per-tier review a pack defines.

### P2 — designed for, not built

- `fleet pack search` over a registry; signed packs.
- A second provider's payload adapter for the hooks, proving the slot.

## Success metrics

- A person who has read `fleet --help` once guesses the spelling of any verb
  on this page without opening it: measured at the rehearsal by the operator's
  own first-try rate, target every verb.
- Zero verbs whose name appears in two families or two meanings, measured by
  the naming doc's table against this page's headings at every landing.
- Every command's usage line under 80 columns and its refusals under three,
  measured by the cli's own usage test.

## Decisions — ruled 2026-09-08

**Q1 — Where do the seat writers live?** Under `event`. Ruled: `seat` is
done-to, `event` is said-by.

**Q2 — What is the work unit called?** `item`. Ruled over task, work and
cargo.

**Q3 — Do the four verbs nest?** No. Ruled: top-level, shortest to type.

**Q4 — `install` as a verb?** No. Ruled: its work runs in the installer or on
the first `start`; which one is the install conversation's.

**Q5 — `runway`?** A section of `status`. Ruled.

**Q6 — `clear-halt` or `revive`?** `clear-halt`, under `event`. Ruled;
`revive` is a verdict.

**Q7 — The courier verb's name?** `nudge`. Ruled against ring and ping on the
naming doc's rule: one word for one mechanism.

**Q8 — Does `pack` need `remove` and `list`?** Yes. Ruled.

**Q9 — Is a pack a Claude Code plugin?** No. Ruled the same day: a pack is
fleet's own format, the provider appears in the overlay slot and the adapter
and nowhere else, and the plugin shape is that overlay's wiring for one
provider.
