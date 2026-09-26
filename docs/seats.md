# The controller and seats

A seat is whoever does a fleet's work: an agent the controller runs, or a
person. Every seat is known by an id minted once, and may carry a name you
choose. The controller is the process that keeps the agent seats running: it
polls the agent for each seat's session, starts or brings back the ones that
are down, collects the rests seats ask for, and publishes what it saw. You
list seats in `fleet.toml` with `fleet seat add`, bring the controller up
with `fleet start` and down with `fleet stop`, or run it in your terminal
with `fleet observe`. Transient seats are made on demand with
`fleet seat spawn`, handed work with `fleet seat feed`, and ended with
`fleet seat retire`.

## Terms

- **seat**: one who does work, with an id, a kind and, where you gave one, a
  name. An **agent seat** works in sessions the controller starts. A
  **human seat** is a person: it is listed in `fleet.toml`, acts on items,
  and is never run.
- **id**: the seat's key, a UUID minted once when the seat is added and shown
  whole, for example `01a0d5ff-b143-7781-9967-5ccd10b55fd3`. The id is what
  `fleet.toml`, the seat list, the event stream and an item's assignee carry.
  A name can change; the id never does.
- **short id**: the id's last eight hex digits, `10b55fd3` for the id above.
- **name**: what a person calls the seat, such as `Orla`. It is optional and
  free to change.
- **machine name**: `<slug>-<short id>`, the name fleet prints for a seat and
  gives its worktree. The slug is the name lowercased, with every run of
  characters outside `a`–`z` and `0`–`9` turned into one `-` and the ends
  trimmed; a seat with no name, or a name that leaves nothing, takes its kind.
  `Orla` above is `orla-10b55fd3`; an agent seat with no name is
  `agent-<short id>`, and a person with none is `human-<short id>`.
- **seat argument**: what every verb that names a seat takes. It is the full
  id; eight or more hex digits from the front or the back of the id; the
  name, in any case; or a machine name, matched on its last eight hex digits
  alone. It must name exactly one seat, or the verb refuses.
- **identity**: this machine's own seat, a human one, kept in
  `identity.toml` in the machine directory. A verb you run with no `--by`
  acts as it (see [Who you are](#who-you-are-identitytoml)).
- **named seat**: an agent seat listed in the `[seats]` table of
  `fleet.toml`. The controller keeps it running and brings it back when it
  goes down. Fleet's messages call it a named seat whether or not it has a
  name.
- **transient seat**: a seat made by `fleet seat spawn`, under an id minted
  for it and never listed in `fleet.toml`. It has no name, so its machine
  name is `agent-<short id>`. The controller watches it and never brings it
  back.
- **controller**: the loop behind `fleet observe`, run as a user service by
  `fleet start`. One poll every `poll_seconds` (5 unless your policy says
  otherwise).
- **machine directory**: where the controller keeps its state and where every
  command in this area looks for it: `FLEET_DIR` where that is set, otherwise
  `~/.fleet` on macOS, and on Linux `$XDG_STATE_HOME/fleet` where that is set
  and `~/.fleet` where it is not.
- **seat list**: `config.json` in the machine directory. It names the policy
  file in force (`fleet_toml`) and carries one row per agent seat this
  machine runs (`children`), keyed by the seat's id. The controller runs the
  seats this file carries, and never starts one from the `[seats]` table in
  `fleet.toml`.
- **blind dispatch**: a start or revive the controller issues for a seat that
  has no live session. At three uncancelled blind dispatches the seat is
  **halted**: nothing more is started for it until you lift the halt.
- **rest**: a named seat's request to stop its session and have a fresh one
  started in its place.
- **nudge**: one message carried to a seat's live session.

The projection and the event stream the controller writes are read with
`fleet status` and `fleet event tail`; see
[Status and the event stream](status.md).

## Adding seats

`fleet seat add` lists one seat in the fleet's own `fleet.toml`: it appends a
`[seats.<id>]` table, reads the file back, and prints the seat's id on
standard output. Run it from inside the project. It asks nothing; the two
kinds are flags, and you give exactly one.

### An agent seat

`--agent` mints a fresh id for a seat the controller runs. `--name` gives it
a name and `--model` the model its sessions run on; both are optional:

```sh
$ fleet seat add --agent --name Orla --model claude-opus-5
added: agent orla-10b55fd3 — [seats.01a0d5ff-b143-7781-9967-5ccd10b55fd3] in <project>/fleet.toml
next: git worktree add <worktrees>/orla-10b55fd3 <a branch>, then fleet start renders it
01a0d5ff-b143-7781-9967-5ccd10b55fd3
```

It exits 0. The first two lines are on standard error, and the id alone is
on standard output. The table it appends reads:

```toml
[seats.01a0d5ff-b143-7781-9967-5ccd10b55fd3]
kind = "agent"
name = "Orla"
model = "claude-opus-5"
```

The `next:` line names the worktree the seat works in, which you make
yourself (see [The `[seats]` table](#the-seats-table)). The seat runs once
`fleet start` renders it into the seat list.

### A human seat

`--human` lists this machine's identity as a human seat, minting the
identity first where the machine has none. `--name` gives the row a name;
without it the row takes the name in `identity.toml`, if that file carries
one. On a machine whose identity was already there:

```sh
$ fleet seat add --human --name Bea
added: human bea-8a397d42 — [seats.01a0d602-37ef-72f3-b718-a4a98a397d42] in <project>/fleet.toml
01a0d602-37ef-72f3-b718-a4a98a397d42
```

It exits 0. Where this call minted the identity, a second line says so:

```sh
$ fleet seat add --human
added: human human-a033413d — [seats.01a0d602-4063-7370-b769-f515a033413d] in <project>/fleet.toml
identity: minted at <fleet-dir>/identity.toml
01a0d602-4063-7370-b769-f515a033413d
```

A person is listed once: when the identity already has a table in the file,
`--human` writes nothing and exits 1. `fleet create` lists whoever ran it
this same way (see [Getting started](getting-started.md)).

### `--json`

`--json` prints one document on standard output in place of the id:

```sh
$ fleet seat add --agent --name Bex --json
added: agent bex-d31e482f — [seats.01a0d5fa-6d86-7721-af36-0c28d31e482f] in <project>/fleet.toml
next: git worktree add <worktrees>/bex-d31e482f <a branch>, then fleet start renders it
{"ok":true,"verb":"seat add","data":{"file":"<project>/fleet.toml","minted_identity":false,"seat":{"id":"01a0d5fa-6d86-7721-af36-0c28d31e482f","kind":"agent","name":"Bex"}}}
```

### What it refuses

- Two seats cannot answer to one name. A name another seat in the file
  already holds, in any case, is refused with exit 1:
  `Orla already names orla-10b55fd3 (<id>) — two seats cannot answer to one
  name`.
- A name the seat argument would read as an id is refused with exit 2: eight
  or more characters that are all hex digits or `-`
  (`--name deadbeef reads as a seat id — pick a name that is not 8 or more hex
  digits`), or one ending in `-` and eight hex digits
  (`--name x-0123abcd ends the way a seat's machine name does (-<8 hex>) —
  pick another`). A blank name is `--name is empty`, exit 2.
- `--agent` with `--human`, neither of them, or `--model` with `--human` is
  a usage error, exit 2.
- A `fleet.toml` whose `[seats]` table does not read (see
  [The `[seats]` table](#the-seats-table)) is exit 3, naming the file, and
  nothing is written.

## Who you are: identity.toml

`identity.toml` in the machine directory says who acts when you run a verb
on this machine without saying who. It is a person's seat id, minted on the
first act that needs one: `fleet create`, `fleet seat add --human`, a verb
that acts with no `--by`, or the controller. Fleet writes it as:

```toml
# This machine's identity: who acts when a fleet verb runs here with no
# --by and no FLEET_ACTOR. The id is the key; add name = "..." if you
# want one. Written by fleet.
id = "01a0d602-37ef-72f3-b718-a4a98a397d42"
kind = "human"
```

`kind` is always `human`. `name` is yours to add; fleet never writes one.

`fleet dispatch`, `deliver`, `hold`, `clear`, `review`, `land`, `run` and
`cancel`, `fleet seat retire` and `fleet seat nudge` each act as somebody.
Given no `--by` and no `FLEET_ACTOR`, they act as this identity. When the fleet's `fleet.toml` does not list it,
the verb says so on standard error, on every run until it is listed, and
goes on. The first run on a machine with no identity mints it and says that
too:

```sh
$ fleet review <item> --show
fleet review: this machine had no identity, so one was minted at <fleet-dir>/identity.toml; acting as this machine's identity human-8a397d42 (01a0d602-37ef-72f3-b718-a4a98a397d42), which <project>/fleet.toml does not list — fleet seat add --human lists it
size: 1 file(s), +1, -0 — tests: no, executable: no
...
```

It exits 0. `fleet seat add --human` lists the identity, and from then on
the verb says nothing about who acts. [Items and the record](items.md)
covers `--by` and `FLEET_ACTOR`.

An `identity.toml` that is there and does not read as a person's id is never
replaced, because the id in it may already be on the record. A verb that
needs it exits 3 naming the file and what is wrong, for example
`could not tell who acts: <fleet-dir>/identity.toml: kind = "agent" — a
machine's identity is a person, kind = "human"`.

## The `[seats]` table

Each seat is one table under `[seats]` in the fleet's `fleet.toml`, keyed by
its id. `fleet seat add` writes them, and you can edit them by hand:

```toml
[seats.01a0d5ff-b115-7312-95cf-2472898452ce]
kind = "human"

[seats.01a0d5ff-b143-7781-9967-5ccd10b55fd3]
kind = "agent"
name = "Orla"
model = "claude-opus-5"

[seats.01a0d5ff-b14e-7d43-809e-43b851df4f54]
kind = "agent"
name = "Rex"
status = "parked"
```

- `kind` is `agent` or `human`, and every table carries one.
- `name` is what a person calls the seat. Change it or remove it at will: the
  id stays the key, so a rename moves no worktree and no work.
- `model` is the model an agent seat's sessions run on. Without it the seat
  takes `[controller] default_model`, which is `claude-opus-5` unless you set
  it.
- `status` is `active` (the same as leaving it out), or `parked`,
  `vacationing` or `chartered`, which all mean the fleet keeps the agent seat
  and does not run it.

A human seat takes neither `model` nor `status`. `fleet start` reads the
table and refuses with exit 3, naming the seat, a table keyed by anything but
an id, a table with no `kind` or another kind, a table carrying the one
retired key the refusal names in place of `name`, a `model` or `status` on a
human seat, and a `status` it does not know. The
refusals are in [When it refuses](#when-it-refuses).

Each agent seat works in its own git worktree, which you make yourself. For
a seat `fleet start` renders, the worktree is the one entry in `<worktrees>`
whose name ends in `-<short id>`, and `<worktrees>/<machine name>` where
there is none or more than one. A worktree made under a seat's old name is
still found after a rename, by the short id it kept. `<worktrees>` is the
directory `[project] worktrees` names in the project's file, read against
the project root, and `<project>-worktrees` beside the project where it
names none.

`fleet start` is what turns the table into rows of the seat list: every agent
seat that is not parked becomes a row, a row whose seat left the table or
turned parked is dropped, and transient rows are left alone. Human seats are
listed and never rendered: the controller runs agent seats only. The running
controller runs only the rows of the seat list, so after you add or change an
agent seat you run `fleet stop` and then `fleet start`.

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
  updated and dropped, and how many human seats are listed and not rendered.
  Run from outside every project of a standalone fleet, it renders no row and
  says to run it from inside a registered project. A `[seats]` table that
  does not read stops the start here with exit 3, before the service file is
  written;
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
poll. For example, three seat rows the controller does not run:

```sh
$ fleet observe --once
fleet observe: skipping a seat row — delta-91e8402f carries no worktrees entry
fleet observe: skipping a seat row — a row carries no id
fleet observe: skipping a seat row — echo-fe0d9831 would start under posture `auto` on model `claude-sonnet-4-5`, which matches none of the models measured to honour it (claude-opus-5, claude-fable-5, claude-sonnet-5)
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
  seat's worktree with the first turn `/wake <session-name>`. A named seat
  runs under the permission posture `auto` and a transient one under
  `dontAsk`.
- **revive**: the seat's stopped session is brought back in place, context
  intact. The controller revives only a session whose context it read and
  found under the rest threshold; a stopped session at or over the threshold,
  or one whose context could not be read, gets a new session instead.
- **rest**: the seat asked to rest and its session is live (see
  [Asking for a rest](#asking-for-a-rest)).
- **suggest-rest**: the live session's context is at or over the rest
  threshold, 700000 tokens unless you set it, so the seat is nudged once for
  that session: `<session-name>: context at <tokens> tokens, over the rest
  threshold <threshold> — rest when your work allows: fleet event rest
  <session-name> --reason <why>`. It is never rested for you.
- **halt**: the seat is halted and nothing is started for it.
- **leave-alone**: everything else.

`<session-name>` is the name the seat's sessions are started under: the
seat's machine name when the controller first starts it, such as
`orla-10b55fd3`. A seat renamed since keeps the name its sessions were
started under, and because a machine name is matched on its short id, that
name still names the seat in every verb.

Every session the controller starts, named or transient, carries
`FLEET_ACTOR=seat:<id>` in its environment, so the verbs the seat runs act as
that seat (see [Items and the record](items.md)).

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
the controller prints ``<machine-name> has gone blind on 3 consecutive
dispatches and is HELD DOWN; nothing further is dispatched for it until
`fleet event clear-halt <machine-name>` ``. Only a clear-halt lifts it. A
named seat whose worktree does not exist fails every start this way and is
halted on its third poll.

A seat row is skipped, and said so on every read of the seat list, when it
carries no id or an id that is not a seat id, names no worktree, or would
start under posture `auto` on a model outside `auto_capable_models`. A
skipped seat is not in `fleet status`.

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
{"id":"<id>","seq":2,"ts":"<event-stamp>","type":"substrate.moved","actor":{"kind":"controller","id":"<identity>"},"payload":{"agent":"claude_code","expected":"2.1.280","observed":"<version>"}}
```

`<identity>` is the id in this machine's `identity.toml`: the controller's
own lines are written as the controller under that id.

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
| `first_turn` | `/wake {seat}` | the first turn of a session the controller starts; `{seat}` is the name the session is started under |
| `nudge_timeout_seconds` | `10` | how long a nudge or a feed typed into a seat's session is given to be taken |
| `load_ceiling_per_cpu` | `1.0` | `fleet seat spawn` refuses when the five-minute load average is above this times the CPU count |
| `max_transient_busy` | `3` | `fleet seat spawn` refuses when more than this many transient seats are mid-turn; `0` is kept |
| `plugin_dir` | none | a plugin directory every session the fleet starts loads; relative to `fleet.toml` |

Any of these except `plugin_dir` can also be set for one machine, over the
policy file, in a `controller` object in the seat list. The running
controller and `fleet status` read it; the verbs you run — `fleet seat
spawn`, `feed`, `retire` and `nudge`, and `fleet dispatch` without `--to` —
read the policy file alone, so a `load_ceiling_per_cpu`,
`max_transient_busy` or `nudge_timeout_seconds` set only here does not reach
them:

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
Each takes a seat argument, writes one event to the stream as that seat,
reads it back, prints the event with the seat's machine name and its
sequence number on standard output, and exits 0:

```sh
$ fleet event woke orla
seat.woke orla-10b55fd3 — seq <seq>
```

- `fleet event woke <seat>`: the seat started and oriented. It cancels a
  rest the seat asked for that the controller has not yet collected.
- `fleet event rest <seat> [--reason <text>]`: a request, below.
- `fleet event handed-off <seat>`: the seat finished its handoff.
- `fleet event exited <seat>`: the seat ended on purpose. When its session
  stops, the controller starts a new one instead of bringing the old one
  back.

`woke`, `handed-off` and `exited` are records: they write whatever state the
fleet is in, and take no `--reason` (exit 2). The seat argument is resolved
over the seat list before anything is written: one that names no row there,
or more than one, is refused with exit 1 and the rows listed:

```sh
$ fleet event woke 01a0d5ff
fleet event woke: 01a0d5ff names 2 seats — orla-10b55fd3 (01a0d5ff-b143-7781-9967-5ccd10b55fd3), rex-51df4f54 (01a0d5ff-b14e-7d43-809e-43b851df4f54) — say more of the id
```

A line for a seat whose row has since left the seat list is dropped by the
controller, which prints ``dropping a seat.exited whose actor `<id>` names no
seat row``.

These verbs live under `fleet event` and not `fleet seat`. Typing
`fleet seat rest` (or `woke`, `handed-off`, `exited`) exits 2 with the
rewrite:

```sh
$ fleet seat rest orla
fleet seat rest: the seat noun is what is done to a seat — say fleet event rest
Usage: fleet [COMMAND]
```

## Asking for a rest

A named seat whose context is heavy asks for a fresh session:

```sh
$ fleet event rest orla --reason "context heavy"
seat.resting orla-10b55fd3 — seq <seq>
```

It exits 0 once the request is on the stream. It refuses, writing nothing,
when:

- no controller is running: the projection is missing, does not parse, or is
  older than three poll intervals (exit 5);
- the seat has no live session in the projection (exit 4);
- the seat is transient (exit 6): ``agent-fe0d9831 is a transient row, and
  only named seats rest — use `fleet seat retire agent-fe0d9831` instead``.

On its next poll the controller collects the rest in this order: it stops the
seat's session, starts a new one in the same worktree with the first turn,
removes the old session, and writes `session.rested`. If the stop or the
start fails, the rest stays pending and the next poll tries again.

`fleet event rest` reads only the machine directory, so it works from any
directory.

## Lifting a halt

```sh
$ fleet event clear-halt rex --reason "made its worktree"
seat.clear_halt rex-51df4f54 — seq <seq>
```

The request is written to the stream and taken on the controller's next poll,
which resets the seat's blind count to zero and lifts the halt. It refuses with
exit 5 when no controller is running, and with exit 1 when the seat is not
halted, printing what it read:

```sh
$ fleet event clear-halt orla
fleet event clear-halt: orla-10b55fd3 is not halted — its row reads present with 0 blind dispatch(es), and there is no hold to lift
```

## Nudging a seat

`fleet seat nudge` carries one message to any seat's live session, named or
transient:

```sh
$ fleet seat nudge orla --text "check your mail"
nudged orla-10b55fd3 — <session> — sent
```

Your text is typed, verbatim, into the seat's own session: pasted whole,
then submitted. The nudge is `sent` only when the agent's own list shows the
seat's session busy afterwards, within `nudge_timeout_seconds` or
`--timeout <seconds>` when you give it. A seat already mid-turn takes the
text after the turn it is on: fleet types it, prints `queued <seat> —
<session> — queued: the seat was mid-turn`, and exits 0. Every nudge that
finds a live session writes `session.nudged` to the stream, with the outcome
and who sent it: `FLEET_ACTOR` where it is set, and otherwise this machine's
identity (see [Who you are](#who-you-are-identitytoml)).

You run it from inside the project; `--project <name>` makes it refuse (exit
2) when the directory resolves to a different project. It refuses before
sending anything when:

- no controller is running (exit 5; the message says whether the
  projection is missing, does not parse, carries no readable stamp or is
  too old, giving its age, and ends ``run `fleet start` ``);
- the projection carries no row for the seat, or its row is anything but
  `present`, so a session stopped at a prompt is not nudged (exit 4);
- the seat has no live session on fleet's host, or the agent's own list
  carries no row for that session (exit 4).

A seat whose session is stopped at a question, such as a permission prompt,
is refused before anything is typed: it prints `not nudged <seat> —
<session> — refused: blocked on <cause>` and exits 1. A nudge the session
did not take within the bound prints `not nudged <seat> — <session> —
failed: typed and not taken: still <status> after <n>s` and exits 1.

## Spawning a transient seat

```sh
$ fleet seat spawn --first-turn brief.md
  load average (5m)       : <load> (ceiling <ceiling> = <cpus> cpu x 1.00)
  transient seats mid-turn: 0 (cap 3)
worktree: <worktrees>/agent-fe0d9831
agent-fe0d9831
```

The seat's machine name is the one line on standard output; the rest is on
standard error. `fleet seat spawn`:

1. reads the machine's five-minute load average and how many transient seats
   are mid-turn, and refuses (exit 1, printing both readings) when either is
   over its ceiling;
2. mints a fresh id for the seat, which no other seat ever had and no later
   spawn takes, cuts a detached worktree for it at
   `<worktrees>/agent-<short id>` from the project's `origin/main` as your
   clone has it, or from `--base <commit>`, and adds its row to the seat
   list, marked transient;
3. writes the seat's permission rules into `.claude/settings.local.json` in
   the worktree, from the pack layers (see [Packs](packs.md));
4. starts a session in the worktree with the file's text as its first turn,
   on `--model <id>` or the default model, under `transient_posture`, with a
   configuration directory of its own in the machine directory.

The seat's configuration directory is `config/agent-<short id>` in the
machine directory. The spawn empties it, then fills it with the files the installed
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
it run. `--json` prints one document on standard output instead of the name,
with the seat as its id and its kind:

```json
{"ok":true,"verb":"seat spawn","data":{"base":"<sha>","belt":{"cpus":<cpus>,"load":<load>,"load_ceiling":<ceiling>,"mid_turn":0,"mid_turn_cap":3},"seat":{"id":"01a0d5fa-3312-7be3-99eb-cec4fe0d9831","kind":"agent"},"worktree":"<worktrees>/agent-fe0d9831"}}
```

`fleet dispatch` without `--to` spawns a transient seat this same way; see
[Items and the record](items.md).

## Feeding a transient seat

`fleet seat feed` hands a transient seat whose session is waiting its next
first turn, from a file:

```sh
$ fleet seat feed agent-fe0d9831 --first-turn next.md
fed agent-fe0d9831 — the turn in the seat is now the new one
```

The file's text is typed into the seat's own session, pasted whole and then
submitted, and `session.nudged` records the first line of the old turn and
of the new one. It refuses when the seat argument names no row of the seat
list, or more than one (exit 1), is a named seat (exit 6), has no live
session (exit 4), is mid-turn (exit 1: `` `agent-fe0d9831` is still holding
a turn — the agent reports its session busy ``), or is stopped at a question
(exit 1, naming what it is blocked on). The feed counts only when the agent's
own list shows the session busy within `nudge_timeout_seconds`; otherwise it
exits 1, and a second `session.nudged` records the old turn put back.

## Retiring a transient seat

```sh
$ fleet seat retire agent-fe0d9831
retired agent-fe0d9831 — reclaimed <bytes> bytes from <worktrees>/agent-fe0d9831, pid <pid> confirmed gone
```

`fleet seat retire` stops the seat's session and waits until the agent no
longer lists it as live, removes the session from the agent's list, removes
the worktree, drops the seat's
row, checks from outside that nothing of the seat is still running or on
disk, removes its configuration directory, and writes `session.stopped`. The
seat's id goes with it: the next spawn mints a new one.

Before it drops the row, it takes back every `open` or `in_progress` item on
the project's board assigned to the seat with a dispatch on it: the item goes
back to `open`, unassigned and with its order index removed, and an
`order_withdrawn` entry naming the seat is appended to its timeline (see
[Items and the record](items.md#reading-the-record)). Standard error says
`ORDER WITHDRAWN at retire: <item> — it is open and unassigned` for each one.
The entry is written by whoever retires the seat: `--by`, else
`FLEET_ACTOR`, else this machine's identity. An item nothing blocks is back
in the store's ready set, so `fleet dispatch` can give it to another seat.
A closed item is never reopened: when an item was closed, or taken by another
seat, after the retire read the board, the retire writes nothing to it and
refuses (exit 1). The session and the worktree are gone by then and the seat's
row stands, so read the item and retire again. When the board cannot be read
and the seat was dispatched nothing, it says so on standard error and retires
the seat anyway.

It deletes the seat's work branch only when the last `landed` entry on the
seat's item classifies that same branch `SAFE`, and then prints
`work branch <branch>: SAFE on the landing — deleted`. Otherwise it prints
`work branch kept — <why>`, or nothing when the worktree stood on no branch.

A seat whose session is already gone retires the same way, with `no live
session to stop` in place of the pid. `--dead` says you expect that: it adds
`(--dead: the roster named no live session)` to the line, and refuses (exit 1)
when the session turns out to be live.

It refuses when the seat argument names no row of the seat list, or more
than one (exit 1), or names a named seat (exit 6). A `--by` that names no
seat is refused (exit 1) before anything moves. `--json` prints one document
with the seat, the worktree, the bytes, the pid and what became of the
branch.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `fleet start` with no fleet above the directory and none in the seat list | 1 | ``fleet start: no fleet.toml above this directory and no fleet named by <fleet-dir>/config.json — `fleet create` writes one`` | run it inside the project, or run `fleet create` |
| `fleet start` while the controller runs | 1 | `fleet start: the controller is already running as pid <pid>; its last tick was <time>` | nothing, or `fleet stop` first |
| `fleet start` with no agent binary | 3 | `fleet start: <why> — nothing was loaded; the search path is <path>` | install `claude` on that path, or set `FLEET_CLAUDE_BIN` |
| `fleet start` with a `[seats]` table keyed by anything but an id | 3 | `fleet start: [seats.<key>] is keyed by a name — a seat is keyed by its id now; fleet seat add --agent --name <key> mints one` | run the `fleet seat add` it names, and delete the old table |
| `fleet start` with a seat table carrying the retired name key | 3 | `fleet start: [seats.<id>] carries <retired-key>, which is name now` | rename the key to `name` |
| `fleet start` with a seat table carrying no `kind`, or another kind | 3 | `fleet start: [seats.<id>] carries no kind — say kind = "agent" or kind = "human"`, or `[seats.<id>] kind = "<value>" is neither agent nor human` | set `kind` |
| `fleet start` with `model` or `status` on a human seat | 3 | `fleet start: [seats.<id>] is a human seat and carries model — only an agent seat runs on a model` (or `carries status — only an agent seat is parked`) | remove the key |
| `fleet start` with an unknown seat `status` | 3 | `fleet start: [seats.<id>] status = "<value>" is none of ...` | use `active`, `parked`, `vacationing` or `chartered` |
| `fleet seat add` with a name another seat holds | 1 | `fleet seat add: <name> already names <machine-name> (<id>) — two seats cannot answer to one name` | pick another name |
| `fleet seat add --human` when this machine's identity is already listed | 1 | `fleet seat add: this machine's identity <machine-name> (<id>) is already [seats.<id>] in <project>/fleet.toml` | nothing: you are listed |
| `fleet seat add --name` that reads as an id or a machine name | 2 | `fleet seat add: --name <name> reads as a seat id — pick a name that is not 8 or more hex digits`, or `--name <name> ends the way a seat's machine name does (-<8 hex>) — pick another` | pick another name |
| `fleet seat add` with neither `--agent` nor `--human`, or both, or `--model` with `--human` | 2 | `error: the following required arguments were not provided:`, `error: the argument '--agent' cannot be used with '--human'` or `error: the argument '--human' cannot be used with '--model <MODEL>'` | give one kind |
| `fleet seat add` over a `fleet.toml` whose `[seats]` table does not read | 3 | `fleet seat add: <project>/fleet.toml: ` and the table's refusal | fix the table |
| an `identity.toml` that does not read as a person's id | 3 | `could not tell who acts: <fleet-dir>/identity.toml: <why>` | fix the file, or move it aside if its id is on no record |
| a seat argument that names no seat | 1 | `<arg> names no seat — the seats are <machine-name> (<id>), ...` | pick one of the seats listed |
| a seat argument that names more than one seat | 1 | `<arg> names <n> seats — <machine-name> (<id>), ... — say more of the id` | give more of the id |
| an empty seat argument | 2 | `names no seat — the argument is empty` | name a seat |
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
| `fleet seat nudge` for a seat stopped at a question | 1 | `not nudged <seat> — <session> — refused: blocked on <cause>` | answer the seat's question, then nudge again |
| `fleet seat nudge` the session did not take | 1 | `not nudged <seat> — <session> — failed: typed and not taken: still <status> after <n>s` | look at it with `fleet seat attach <seat>`, or nudge again |
| `fleet seat add`, `nudge`, `spawn`, `feed` or `retire` outside every project | 3 | ``no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one`` | run it inside the project |
| `--project` naming another project | 2 | `--project <name> names a project this directory does not resolve to — ...` | run it in that project |
| `fleet seat spawn` with a first-turn file that cannot be read | 2 | `fleet seat spawn: the first turn at <file> could not be read: ...` | fix the path |
| `fleet seat spawn` over the load or mid-turn ceiling | 1 | `the machine cannot take another transient seat: <leg> over its ceiling.` and both readings | wait, or raise `load_ceiling_per_cpu` or `max_transient_busy` |
| `fleet seat spawn --base` naming no commit | 1 | `` `<base>` does not resolve to a commit in <project> — nothing was made: `git rev-parse` exited 1: `` | name a commit |
| `fleet seat spawn` in a clone with no `origin/main` | 1 | ``fleet seat spawn: `git worktree add` exited 128: fatal: invalid reference: origin/main`` | fetch `origin`, or pass `--base` |
| `fleet seat spawn` whose session fails to start | 1 | `the start for <seat> failed inside its 5s watch window; its output is at <file>` and what was rolled back | read the file |
| `fleet seat feed` or `retire` for a named seat | 6 | `<machine-name> is a named seat — named seats are rung and rested, and only a transient row is fed and retired` | `fleet seat nudge` or `fleet event rest` |
| `fleet seat feed` with no live session | 4 | `` `<seat>` has no live session in <worktree>, so there is nothing to feed `` | retire it, and spawn again |
| `fleet seat feed` while the seat is mid-turn | 1 | `` `<seat>` is still holding a turn — the agent reports its session busy `` | wait for the turn to end |
| `fleet seat feed` while the seat is stopped at a question | 1 | `` `<seat>` is stopped in front of a person — blocked on <cause> — and nothing is typed at a dialog `` | answer the seat's question |
| `fleet seat feed` the session did not take | 1 | `` `<seat>` was not fed — failed: typed and not taken: still <status> after <n>s; the occupant marker was put back `` | feed it again |
| `fleet seat retire --dead` for a live seat | 1 | `` `<seat>` is not dead — the roster names a live session ... `` | retire without `--dead` |

## See also

- [Getting started](getting-started.md): `fleet create`, and your first
  `fleet start`.
- [Status and the event stream](status.md): reading the roster, the halts and
  the events this area writes.
- [Items and the record](items.md): `fleet dispatch`, which spawns a
  transient seat when given no `--to`, and `--by`, `FLEET_ACTOR` and the
  actor every entry names.
- [Runs and workflows](runs.md): workflows that spawn, feed and retire seats.
- [Packs](packs.md): the defaults, and the permission rules a spawned seat
  gets.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
