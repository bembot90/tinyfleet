# The controller and seats

The controller is the process that keeps your seats running: it polls the
agent for each seat's session, starts or brings back the ones that are down,
collects the rests seats ask for, and publishes what it saw. You bring it up
with `fleet start` and down with `fleet stop`, or run it in your terminal with
`fleet observe`. Named seats are the ones you declare in `fleet.toml` and the
controller keeps alive; transient seats are made on demand with
`fleet seat spawn`, handed work with `fleet seat feed`, and ended with
`fleet seat retire`.

## Terms

- **controller**: the loop behind `fleet observe`, run as a user service by
  `fleet start`. One poll every `poll_seconds` (5 unless your policy says
  otherwise).
- **machine directory**: where the controller keeps its state and where every
  command in this area looks for it: `FLEET_DIR` where that is set, otherwise
  `~/.fleet` on macOS, and on Linux `$XDG_STATE_HOME/fleet` where that is set
  and `~/.fleet` where it is not.
- **seat list**: `config.json` in the machine directory. It names the policy
  file in force (`fleet_toml`) and carries one row per seat (`children`). The
  controller reads this file, never the `[seats]` table in `fleet.toml`.
- **named seat**: a seat declared under `[seats.<name>]` in `fleet.toml`. The
  controller keeps it running and brings it back when it goes down.
- **transient seat**: a seat made by `fleet seat spawn`, named
  `transient-<n>`. The controller watches it and never brings it back.
- **blind dispatch**: a start or revive the controller issues for a seat that
  has no live session. At three uncancelled blind dispatches the seat is
  **halted**: nothing more is started for it until you lift the halt.
- **rest**: a named seat's request to stop its session and have a fresh one
  started in its place.
- **nudge**: one message carried to a seat's live session.

The projection and the event stream the controller writes are read with
`fleet status` and `fleet event tail`; see
[Status and the event stream](status.md).

## Declaring named seats

A named seat is one table under `[seats]` in the fleet's `fleet.toml`:

```toml
[seats.alpha]
model = "claude-opus-5"
chosen_name = "Orla"

[seats.bravo]

[seats.charlie]
status = "parked"
```

- `model` is the model the seat's session runs on. Without it the seat takes
  `[controller] default_model`, which is `claude-opus-5` unless you set it.
- `chosen_name` is what a person calls the seat. `fleet status` shows it beside
  the seat's name, and the seat's session is started under it, lowercased.
  Without it the session is started under the seat's name.
- `status` is `active` (the same as leaving it out), or `parked`,
  `vacationing` or `chartered`, which all mean the fleet keeps the seat and
  does not run it. Any other value stops `fleet start` with exit 3, naming the
  seat and the value.

Each named seat works in its own git worktree, `<worktrees>/<seat>`, which you
make yourself; `fleet create` prints the `git worktree add` line for its
example seat. `<worktrees>` is the directory `[project] worktrees` names in the
project's file, read against the project root, and `<project>-worktrees`
beside the project where it names none.

`fleet start` is what turns the table into rows of the seat list: every seat
that is not parked becomes a row, a row whose seat left the table or turned
parked is dropped, and transient rows are left alone. The running controller
never reads `[seats]`, so after you change it you run `fleet stop` and then
`fleet start`.

## Starting the controller

```sh
$ fleet start
```

`fleet start` works from inside a project whose fleet it starts, or from
anywhere once the seat list names a fleet. It refuses before anything is
written or loaded when:

- no `fleet.toml` sits above the directory and the seat list names none
  (exit 1);
- the controller's service is already running (exit 1, naming its pid and the
  time of its last published poll);
- no agent binary can be found (exit 3, naming the search path it used).

The agent binary is `FLEET_CLAUDE_BIN` where that names an absolute path, and
otherwise the first `claude` on a search path fleet builds for its children,
not your shell's `PATH`: `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`,
`/opt/homebrew/bin`, `/usr/local/bin` and `~/.local/bin` on macOS, and
`~/.local/bin`, `/usr/local/bin`, `/usr/bin` and `/bin` on Linux.

Then it does the install work, every step of it idempotent, and prints one
line per step on standard error, each starting `first run:`:

- the machine directory, made if it is not there;
- the seat list, written if it is not there and left as it is if it is;
- which project the seat rows are keyed on, and where their worktrees go;
- the named seats rendered into the seat list, with how many rows were added,
  updated and dropped. Run from outside every project of a standalone fleet,
  it renders no row and says to run it from inside a registered project;
- the service file, written or already in place;
- that nothing was started yet;
- telemetry, which is off: where `fleet.toml` does not say
  `[telemetry] enabled`, `fleet start` writes `enabled = false` into it.

A start that changed nothing says `first run: every step was already done, so
this start repeated none of it`. A `defaults:` line follows, saying whether
the defaults this binary carries were installed, refreshed or already in
place; see [Packs](packs.md).

Last, it loads the service and waits up to 30 seconds for a
`controller.started` event newer than any already on the stream. When one
arrives it prints `started dev.fleet.controller — controller.started at
sequence <seq> — <fleet-dir>` and exits 0. When none arrives it exits 3 and
says where the service's own output went.

### The service

The service is a per-user service labelled `dev.fleet.controller` that runs
`fleet observe`, the same binary you typed. The service manager restarts it if
it exits.

| | macOS | Linux |
| --- | --- | --- |
| manager | `launchctl` | `systemctl --user` |
| service file | `~/Library/LaunchAgents/dev.fleet.controller.plist` | `~/.config/systemd/user/dev.fleet.controller.service` |
| what the controller prints | `service.out.log` and `service.err.log` in the machine directory | the user journal: `journalctl --user -u dev.fleet.controller.service` |

When you ran `fleet start` with `FLEET_DIR` set, the service file carries it,
so the service uses the same machine directory.

### `--foreground`

```sh
$ fleet start --foreground
```

It does the same checks and the same first-run steps, loads no service, and
runs the controller in your terminal until you stop it with Ctrl-C.

## Stopping the controller

```sh
$ fleet stop
```

`fleet stop` unloads the service and waits up to 30 seconds for a
`controller.stopped` event newer than any already on the stream. It then
prints `stopped dev.fleet.controller — controller.stopped at sequence <seq>`
and `seats: untouched — their sessions are still running`, and exits 0.

It leaves every seat's session running, and it leaves the service file where
it is. It works from any directory, because it reads only the machine
directory. With no service loaded it refuses with exit 1:
`fleet stop: no service is loaded under dev.fleet.controller — there is
nothing to stop`.

## Running the controller by hand

`fleet observe` is the controller itself, in your terminal. It reads the seat
list in the machine directory and the policy file that list names, whichever
directory you run it from.

`--once` runs one poll and exits 0. Without it the loop polls until it is sent
SIGINT or SIGTERM, then writes `controller.stopped` to the stream and exits 0.
Every run writes `controller.started` when it begins.

What it has to say goes to standard error, each line starting
`fleet observe:`, and is said when something changes rather than on every
poll. For example, two seat rows the controller does not run:

```sh
$ fleet observe --once
fleet observe: skipping a seat row — delta carries no `worktrees` entry
fleet observe: skipping a seat row — echo would start under posture `auto` on model `claude-haiku-4-5`, which matches none of the models measured to honour it (claude-opus-5, claude-fable-5, claude-sonnet-5)
...
```

It exits 3 without polling when the seat list cannot be read
(`fleet observe: cannot read the seat list at ...`) or when the policy file
it names cannot be read (`fleet observe: cannot read policy at ...`).

## What the controller does each poll

On every poll the controller re-reads the seat list and the policy file when
either has changed. A policy file that stops parsing is reported once, and
the controller keeps running on the last one that parsed.

It then reads the agent's list of sessions and matches each seat to the
session running in its worktree. For each seat it decides one of these, which
`fleet status` shows as the seat's decision:

- **spawn-woken**: the seat has no session, so a new one is started in the
  seat's worktree with the first turn `/wake <seat>`. A named seat runs under
  the permission posture `auto` and a transient one under `dontAsk`.
- **revive**: the seat's stopped session is brought back in place, context
  intact. The controller revives only a session whose context it read and
  found under the rest threshold; a stopped session at or over the threshold,
  or one whose context could not be read, gets a new session instead.
- **rest**: the seat asked to rest and its session is live (see
  [Asking for a rest](#asking-for-a-rest)).
- **suggest-rest**: the live session's context is at or over the rest
  threshold, 700000 tokens unless you set it, so the seat is nudged once for
  that session: `<name>: context at <tokens> tokens, over the rest threshold
  <threshold> — rest when your work allows: fleet event rest <seat> --reason
  <why>`. It is never rested for you.
- **halt**: the seat is halted and nothing is started for it.
- **leave-alone**: everything else.

A seat that wrote `fleet event exited` or `fleet event rest` and whose
session then stopped gets a new session rather than the old one back.

Transient seats are decided the same way, except that a decision that would
start or bring back a session is left alone: a transient seat whose session
ends stays down until you retire it.

After starting or reviving a session the controller gives it
`arrival_window_seconds` (45) to appear before it starts or revives anything
for that seat again. Each start or revive for a seat with no live session
counts one blind dispatch, and each poll that sees the seat live takes one
off. At three the seat is halted: the stream carries `session.halted`, and
the controller prints ``<seat> has gone blind on 3 consecutive dispatches and
is HELD DOWN; nothing further is dispatched for it until `fleet event
clear-halt <seat>` ``. Only a clear-halt lifts it. A named seat whose worktree
does not exist fails every start this way and is halted on its third poll.

A seat row is skipped, and said so on every read of the seat list, when it
names no worktree, or when the seat would start under posture `auto` on a
model outside `auto_capable_models`. A skipped seat is not in `fleet status`.

The controller starts, stops and nudges nothing, and says why, when no agent
binary can be found (`fleet observe: effects are off — <why>`). On macOS it
also holds every such act while the permission to read the seats' worktrees
is pending. In both cases it keeps polling and publishing.

### The Claude Code version

On every poll the controller asks `claude` for its version and compares it
with the version it expects. That is the version the fleet's `fleet.toml`
pins under `[substrate]`, in either of two forms:

```toml
[substrate.claude_code]
version = "<version>"
```

```toml
[substrate]
claude_code = "<version>"
```

With no pin, it expects the version fleet supports, 2.1.280 (see
[What fleet runs on](getting-started.md#what-fleet-runs-on)). A blank pin, or
a `claude_code` entry in any other shape, pins nothing.

When the version it reads differs from the one it expects, the controller
writes a `substrate.moved` event naming the agent, the version it observed
and the version it expected, and carries on: it refuses, stops and holds
nothing for it.

```sh
$ fleet event tail --type substrate.moved
{"id":"<id>","seq":2,"ts":"<event-stamp>","type":"substrate.moved","actor":"controller","payload":{"agent":"claude_code","expected":"2.1.280","observed":"<version>"}}
```

A running controller writes one event per difference, not one per poll. A
poll that reads the expected version again ends the difference, so the next
one is a new event. A poll that cannot read the version writes nothing and
does not end it. A controller that starts again writes the event again on
its first poll. `fleet status` prints both versions on its first line while
they differ (see
[Status and the event stream](status.md#the-first-line)).

### Tuning the controller

These keys go under `[controller]` in `fleet.toml`. A zero or a blank where a
number or a name is expected is read as the default.

| Key | Default | What it sets |
| --- | --- | --- |
| `poll_seconds` | `5` | seconds between polls |
| `rest_threshold_tokens` | `700000` | context at or over which a live seat is nudged to rest, and a stopped session gets a new one instead of coming back |
| `arrival_window_seconds` | `45` | how long a start is given to appear before the seat is eligible again |
| `stopped_recency_hours` | `24` | how long an ended session still counts as the seat's stopped session |
| `start_watch_seconds` | `5` | how long a start is watched for an immediate failure |
| `default_model` | `claude-opus-5` | the model of a seat that names none |
| `posture` | `auto` | the permission posture of a named seat's session |
| `transient_posture` | `dontAsk` | the permission posture of a transient seat's session |
| `auto_capable_models` | `claude-opus-5`, `claude-fable-5`, `claude-sonnet-5` | model prefixes allowed to run under posture `auto` |
| `first_turn` | `/wake {seat}` | the first turn of a session the controller starts; `{seat}` is the seat's name |
| `nudge_model` | `claude-haiku-4-5-20251001` | the model that carries a nudge |
| `nudge_timeout_seconds` | `90` | how long a nudge is given |
| `load_ceiling_per_cpu` | `1.0` | `fleet seat spawn` refuses when the five-minute load average is above this times the CPU count |
| `max_transient_busy` | `3` | `fleet seat spawn` refuses when more than this many transient seats are mid-turn; `0` is kept |
| `plugin_dir` | none | a plugin directory every session the fleet starts loads; relative to `fleet.toml` |

Any of these except `plugin_dir` can also be set for one machine, over the
policy file, in a `controller` object in the seat list:

```json
{
  "fleet_toml": "<project>/fleet.toml",
  "children": [],
  "controller": { "poll_seconds": 7 }
}
```

A key there that is not one of these is ignored, and `fleet observe` says so
once.

## What a seat says about itself

Four `fleet event` verbs are how a seat's own session reports on its life.
Each writes one event to the stream, reads it back, prints the event and its
sequence number on standard output, and exits 0:

```sh
$ fleet event woke alpha
seat.woke alpha — seq <seq>
```

- `fleet event woke <seat>`: the seat started and oriented. It cancels a
  rest the seat asked for that the controller has not yet collected.
- `fleet event rest <seat> [--reason <text>]`: a request, below.
- `fleet event handed-off <seat>`: the seat finished its handoff.
- `fleet event exited <seat>`: the seat ended on purpose. When its session
  stops, the controller starts a new one instead of bringing the old one
  back.

`woke`, `handed-off` and `exited` are records: they write whatever state the
fleet is in, and take no `--reason` (exit 2). They are not checked against the
seat list when written; an event for a seat the seat list does not carry is
dropped by the controller, which prints ``dropping a seat.exited whose actor
`<seat>` names no seat row``.

These verbs live under `fleet event` and not `fleet seat`. Typing
`fleet seat rest` (or `woke`, `handed-off`, `exited`) exits 2 with the
rewrite:

```sh
$ fleet seat rest alpha
fleet seat rest: the seat noun is what is done to a seat — say fleet event rest
Usage: fleet [COMMAND]
```

## Asking for a rest

A named seat whose context is heavy asks for a fresh session:

```sh
$ fleet event rest alpha --reason "context heavy"
seat.resting alpha — seq <seq>
```

It exits 0 once the request is on the stream. It refuses, writing nothing,
when:

- no controller is running: the projection is missing, does not parse, or is
  older than three poll intervals (exit 5);
- the seat has no live session in the projection (exit 4);
- the seat is transient (exit 6): ``transient-1 is a transient row, and only
  named seats rest — use `fleet seat retire transient-1` instead``.

On its next poll the controller collects the rest in this order: it stops the
seat's session, starts a new one in the same worktree with the first turn,
removes the old session, and writes `session.rested`. If the stop or the
start fails, the rest stays pending and the next poll tries again.

`fleet event rest` reads only the machine directory, so it works from any
directory.

## Lifting a halt

```sh
$ fleet event clear-halt bravo --reason "made its worktree"
seat.clear_halt bravo — seq <seq>
```

The request is written to the stream and taken on the controller's next poll,
which resets the seat's blind count to zero and lifts the halt. It refuses with
exit 5 when no controller is running, and with exit 1 when the seat is not
halted, printing what it read:

```sh
$ fleet event clear-halt alpha
fleet event clear-halt: alpha is not halted — its row reads present with 0 blind dispatch(es), and there is no hold to lift
```

## Nudging a seat

`fleet seat nudge` carries one message to any seat's live session, named or
transient:

```sh
$ fleet seat nudge alpha --text "check your mail"
nudged alpha — <session> — sent
```

The nudge runs as one turn on `nudge_model` in the seat's worktree, told to
send your text verbatim to the seat's session, bounded by
`nudge_timeout_seconds` or by `--timeout <seconds>` when you give it. Every
nudge that finds a live session writes `session.nudged` to the stream, with
the outcome.

You run it from inside the project; `--project <name>` makes it refuse (exit
2) when the directory resolves to a different project. It refuses before
sending anything when:

- no controller is running (exit 5; the message gives the projection's age
  and ends ``run `fleet start` ``);
- the projection carries no row for the seat, or its row is anything but
  `present`, so a session stopped at a prompt is not nudged (exit 4);
- the agent's own list shows no live session in the seat's worktree (exit 4).

A nudge the agent could not deliver prints `not nudged <seat> — <session> —
failed: <why>` and exits 1.

## Spawning a transient seat

```sh
$ fleet seat spawn --first-turn brief.md
  load average (5m)       : <load> (ceiling <ceiling> = <cpus> cpu x 1.00)
  transient seats mid-turn: 0 (cap 3)
worktree: <worktrees>/transient-1
transient-1
```

The seat's name is the one line on standard output; the rest is on standard
error. `fleet seat spawn`:

1. reads the machine's five-minute load average and how many transient seats
   are mid-turn, and refuses (exit 1, printing both readings) when either is
   over its ceiling;
2. takes the lowest free `transient-<n>`, reusing a number a retire freed,
   cuts a detached worktree for it at `<worktrees>/transient-<n>` from the
   project's `origin/main` as your clone has it, or from `--base <commit>`,
   and adds its row to the seat list;
3. writes the seat's permission rules into `.claude/settings.local.json` in
   the worktree, from the pack layers (see [Packs](packs.md));
4. starts a session in the worktree with the file's text as its first turn,
   on `--model <id>` or the default model, under `transient_posture`, with a
   configuration directory of its own in the machine directory.

The seat's configuration directory is `config/<seat>` in the machine
directory. The spawn empties it, then fills it with the files the installed
packs and the defaults carry under `overlay/per-provider/claude/config/`;
the packs fleet ships carry none there, so it starts empty. The settings,
memory, instructions and servers in your own agent configuration therefore
do not reach the seat. The session starts with `CLAUDE_CONFIG_DIR` set to
that directory, and with `CLAUDE_SECURESTORAGE_CONFIG_DIR` set to the
`CLAUDE_CONFIG_DIR` the spawning command ran with, or empty where it had
none, so the seat finds the login your own configuration stored. A named
seat has no directory of its own: it runs under the `CLAUDE_CONFIG_DIR` the
controller runs with, or `~/.claude` where that is not set.

When a transient seat's first turn answers that it is not logged in, the
controller writes one `dispatch.failed` on the stream, naming the seat, the
item it was given, and the cause `authentication_failed`. It does nothing
else about it.

A start that fails within `start_watch_seconds` exits 1 and undoes the
worktree, the row and the configuration directory, naming each and the file
the start's output went to; it never deletes a branch.

`--touched <command>` names one more command the seat's permission rules let
it run. `--json` prints one document on standard output instead of the name:

```json
{"ok":true,"verb":"seat spawn","data":{"base":"<sha>","belt":{"cpus":<cpus>,"load":<load>,"load_ceiling":<ceiling>,"mid_turn":0,"mid_turn_cap":3},"seat":"transient-1","worktree":"<worktrees>/transient-1"}}
```

`fleet dispatch` without `--to` spawns a transient seat this same way; see
[Items and the record](items.md).

## Feeding a transient seat

`fleet seat feed` hands a transient seat whose session is waiting its next
first turn, from a file:

```sh
$ fleet seat feed transient-1 --first-turn next.md
fed transient-1 — the turn in the seat is now the new one
```

The file's text is run as one turn on `nudge_model` in the seat's worktree,
and `session.nudged` records the first line of the old turn and of the new
one. It refuses when the seat is not in the seat list (exit 1), is a named
seat (exit 6), has no live session (exit 4), or is mid-turn (exit 1:
`` `transient-1` is still holding a turn — the agent reports its session
busy ``). A delivery that fails exits 1, and a second `session.nudged` records
the old turn put back.

## Retiring a transient seat

```sh
$ fleet seat retire transient-1
retired transient-1 — reclaimed <bytes> bytes from <worktrees>/transient-1, pid <pid> confirmed gone
```

`fleet seat retire` stops the seat's session and waits until the agent no
longer lists it as live, removes the session from the agent's list, removes
the worktree, drops the seat's
row, checks from outside that nothing of the seat is still running or on
disk, removes its configuration directory, and writes `session.stopped`. The
name is free for the next spawn.

Before it drops the row, it takes back every open item on the project's board
assigned to the seat with a dispatch on it: the item stays open, unassigned,
with a note saying `ORDER WITHDRAWN at retire`, written by `--by <name>`,
`FLEET_ACTOR` or `BEADS_ACTOR`, or `fleet`. When the board cannot be read and
the seat was dispatched nothing, it says so on standard error and retires the
seat anyway.

It deletes the seat's work branch only when the last landing on the seat's
item marks that same branch `SAFE`; otherwise it prints `work branch kept —
<why>`, or nothing when the worktree stood on no branch.

A seat whose session is already gone retires the same way, with `no live
session to stop` in place of the pid. `--dead` says you expect that: it adds
`(--dead: the roster named no live session)` to the line, and refuses (exit 1)
when the session turns out to be live.

It refuses when the seat is not in the seat list (exit 1) or is a named seat
(exit 6). `--json` prints one document with the seat, the worktree, the bytes,
the pid and what became of the branch.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `fleet start` with no fleet above the directory and none in the seat list | 1 | ``fleet start: no fleet.toml above this directory and no fleet named by <fleet-dir>/config.json — `fleet create` writes one`` | run it inside the project, or run `fleet create` |
| `fleet start` while the controller runs | 1 | `fleet start: the controller is already running as pid <pid>; its last tick was <time>` | nothing, or `fleet stop` first |
| `fleet start` with no agent binary | 3 | `fleet start: <why> — nothing was loaded; the search path is <path>` | install `claude` on that path, or set `FLEET_CLAUDE_BIN` |
| `fleet start` with an unknown seat `status` | 3 | `fleet start: [seats.<seat>] status = "<value>" is none of ...` | use `active`, `parked`, `vacationing` or `chartered` |
| `fleet start` or `fleet stop` sees no fresh event within 30 seconds | 3 | `no controller.started was written within 30s — ...` (or `controller.stopped`), and where the service's output is | read the service's output |
| `fleet stop` with nothing loaded | 1 | `fleet stop: no service is loaded under dev.fleet.controller — there is nothing to stop` | nothing |
| `fleet observe` with no seat list, or no policy file | 3 | `fleet observe: cannot read the seat list at ...` or `cannot read policy at ...` | run `fleet start` once, or fix `fleet_toml` in the seat list |
| `fleet event rest`, `clear-halt` or `fleet seat nudge` with no controller running | 5 | `no collector is consuming — ...` | `fleet start` |
| `fleet event rest` for a seat with no live session | 4 | `fleet event rest: <seat> has no live session — ...` | nothing to rest |
| `fleet event rest` for a transient seat | 6 | ``... only named seats rest — use `fleet seat retire <seat>` instead`` | `fleet seat retire` |
| `fleet event clear-halt` for a seat not halted | 1 | `fleet event clear-halt: <seat> is not halted — ...` | check the seat's name |
| `fleet event woke`, `handed-off` or `exited` with `--reason` | 2 | `error: unexpected argument '--reason' found` | drop `--reason` |
| a lifecycle word under `fleet seat` | 2 | `fleet seat <word>: the seat noun is what is done to a seat — say fleet event <word>` | `fleet event <word>` |
| `fleet seat nudge` for a seat not `present` | 4 | ``fleet seat nudge: `<seat>` has no live session — its row reads <state> and not present`` | wait for the seat, or answer its prompt |
| `fleet seat nudge` that the agent could not deliver | 1 | `not nudged <seat> — <session> — failed: <why>` | read the file it names |
| `fleet seat nudge`, `spawn`, `feed` or `retire` outside every project | 3 | ``no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one`` | run it inside the project |
| `--project` naming another project | 2 | `--project <name> names a project this directory does not resolve to — ...` | run it in that project |
| `fleet seat spawn` with a first-turn file that cannot be read | 2 | `fleet seat spawn: the first turn at <file> could not be read: ...` | fix the path |
| `fleet seat spawn` over the load or mid-turn ceiling | 1 | `the machine cannot take another transient seat: <leg> over its ceiling.` and both readings | wait, or raise `load_ceiling_per_cpu` or `max_transient_busy` |
| `fleet seat spawn --base` naming no commit | 1 | `` `<base>` does not resolve to a commit in <project> — nothing was made `` | name a commit |
| `fleet seat spawn` in a clone with no `origin/main` | 1 | ``fleet seat spawn: `git worktree add` exited 128: fatal: invalid reference: origin/main`` | fetch `origin`, or pass `--base` |
| `fleet seat spawn` whose session fails to start | 1 | `the start for <seat> failed inside its 5s watch window; its output is at <file>` and what was rolled back | read the file |
| `fleet seat feed` or `retire` for a seat not in the seat list | 1 | `` `<seat>` is not a row of this machine's seat list `` | check the name |
| `fleet seat feed` or `retire` for a named seat | 6 | `` `<seat>` is a named seat — named seats are rung and rested, and only a transient row is fed and retired `` | `fleet seat nudge` or `fleet event rest` |
| `fleet seat feed` with no live session | 4 | `` `<seat>` has no live session in <worktree>, so there is nothing to feed `` | retire it, and spawn again |
| `fleet seat feed` while the seat is mid-turn | 1 | `` `<seat>` is still holding a turn — the agent reports its session busy `` | wait for the turn to end |
| `fleet seat retire --dead` for a live seat | 1 | `` `<seat>` is not dead — the roster names a live session ... `` | retire without `--dead` |

## See also

- [Getting started](getting-started.md): `fleet create`, and your first
  `fleet start`.
- [Status and the event stream](status.md): reading the roster, the halts and
  the events this area writes.
- [Items and the record](items.md): `fleet dispatch`, which spawns a
  transient seat when given no `--to`.
- [Runs and workflows](runs.md): workflows that spawn, feed and retire seats.
- [Packs](packs.md): the defaults, and the permission rules a spawned seat
  gets.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
