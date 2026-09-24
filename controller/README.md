# fleet/controller

The controller: one boring daemon per machine that keeps a fleet of
coding-agent seats alive, and publishes what it saw. Its contract is
`../brain/prds/fleet-controller-prd.md`; what it knows about the agent it runs
is `../brain/lessons/`.

```
cargo build --release        # target/release/fleet
cargo test                   # the suite, offline, on either platform
make fleet-test              # the same suite through the repository's runner
```

## observe

`fleet observe` **reads** four things and believes none of them for longer than
a poll. The machine directory is `$FLEET_DIR`, else `.fleet` under `$FLEET_HOME`
or the user's home (on Linux, `$XDG_STATE_HOME/fleet` when that is set).
`config.json` there is the seat list — one row per seat with its directory
name, its chosen name, its model, and a `worktrees` map from project to path —
and its `fleet_toml` key names the policy file, which the loop follows if a
later read of the seat list moves it. `fleet.toml` is policy: the poll
interval, and `[substrate.claude_code]`, the agent release this fleet's
measurements are pinned to. Both files are re-read whenever their mtime moves;
a row that names no seat or no worktree is skipped loudly rather than
defaulted, and a policy file that stops parsing leaves the last good one in
force. Startup is the exception that cannot fall back: with neither file
readable, `observe` exits 3 and names the path. Through the agent adapter it
then lists every session, matches rows to seats by working directory and never
by short id, and reads each matched session's context from its transcript's
last main-chain assistant entry — under `$CLAUDE_CONFIG_DIR` when the agent is
scoped to one, and under `$HOME/.claude` otherwise, at the per-project
directory the agent names by replacing every non-alphanumeric character of the
path with a dash. Two live rows in one seat's worktrees, and a listing that
could not be read, are both Unknown — never absence. Every call to the agent
binary — `$FLEET_CLAUDE_BIN`, else `claude` — runs under a deadline of 20
seconds, or of `$FLEET_AGENT_TIMEOUT_MS` when that names a positive whole number
of milliseconds; a call that outruns it is killed and read as Unknown naming the
deadline, because a listing that hangs must not hang the poll.

`fleet observe` **writes** three documents into the machine directory.
`projection.json` is rewritten every
poll through a temp file and a rename, so a reader sees a whole document or the
previous one: it carries the format version, the moment it was generated, the
controller's version, the agent version read this poll beside the release the
policy pins, the policy in force with its mtime and `fleet_parse_error` when
the loop is running on last-good, and one row per seat with its roster state —
`present`, `prompt-blocked`, `starting`, `stopped`, `absent` or `unknown` — its
context tokens, the project and worktree it was found in, and the verdict this
poll reached for it beside what was done about it. Beside the seats it carries
`effects`, which reads `on` or `off` with the cause, and `in_flight`, which names
the seat and the effect a poll is BLOCKED inside and is written before the
blocking call and cleared after it — a slow effect and a dead controller are
what that field exists to separate. A seat stopped in
front of a human is `prompt-blocked` and carries the agent's own cause string;
like the reference controller, it publishes no context reading. There is no
pid, no handle and no claim about what is running now — freshness is the only
liveness signal a reader gets. `events.jsonl` is appended to, one JSON object
per line, each with an id, a sequence, a timestamp, a type, an actor and a
payload: `controller.started` once per process, `controller.stopped` when a
signal ends it, `substrate.moved` when the agent version stops matching the pin,
and `session.spawned`, `session.rested`, `session.nudged` and `session.crashed`
as the effects below take them. The stream runs both ways: `fleet event` appends
`seat.woke`, `seat.resting`, `seat.handed_off` and `seat.exited` from its own
process, and the loop consumes them. Nothing is written per poll, because a
stream that reports the controller's own health drowns the lines a person came
to read. `sessions.json` is the third document and is described below.

## decide

Between the observation and the publish, `fleet observe` reads its own event
stream from the line after the cursor it persisted last tick and folds what it
finds into per-seat state — a `seat.resting` is a pending rest, a `seat.exited`
is a pending deliberate end, a `seat.woke` clears both, and an event whose actor
names no configured row, or a rest for a `transient` one, is dropped with a line
saying so. Then one **pure function** produces one verdict per seat, from terms
every one of which was read this poll: `leave-alone`, `spawn-woken`, `revive`,
`rest`, `suggest-rest` or `halt`. Unknown is always `leave-alone`, because a
roster nobody could read is not an empty fleet; the transient filter turns every
session-creating verdict for a spawned row into `leave-alone` at one chokepoint;
rest outranks spawn outranks suggest, and the halt guard outranks the spawn it
guards. A seat whose newest dispatch is younger than
`controller.arrival_window_seconds` and has no sighting is held whatever the
roster says, because the roster reads the same absence every poll and a start
that has not arrived yet is not a seat that needs another one. A pid-less row
that is not a newborn is read by an EVENT and then by its context — the roster
cannot tell a hibernated session from a deliberately stopped one — so an
unconsumed deliberate end is a successor, a reading under
`controller.rest_threshold_tokens` is a revive, and a reading at or over it, or
one nobody could take, is a successor too.

Above those two arms and below the arrival hold sits the **replacement window**.
The controller reads the agent daemon's own account of itself once per poll — a
whole-fleet read like the roster, so two seats cannot decide against different
beliefs about the same moment — and a pid-less row is HELD while that daemon's
pid has moved since the last poll or its uptime is under
`controller.arrival_window_seconds`. Upgrading the agent replaces the daemon,
which ends every hosted process at once and re-hosts the sessions within about a
minute: a row in that window is in transit rather than gone, and spawning over it
is what puts two live rows in one worktree. The hold's line names itself
(`replacement window: held`) and is printed once per transition into it. A daemon
read that FAILED opens no window — a hold on a reading nobody took would stop
every dispatch on the fleet for as long as the read stayed broken. A poll in
which at least half the seats, and never fewer than two, stand on pid-less rows
logs ONE upgrade line rather than N independent absences; it changes no seat's
verdict, because the arrival window already bounds each dispatch.

The **blind counter** is the other pure function beside the table. A sighting —
a present or prompt-blocked row — decays it by one and never clears it, because a
flapping roster that cleared the count on each good poll would let a guard built
for a stuck seat never fire; an unreadable roster holds it, so a broken read
cannot launder a runaway back to zero; a starting row moves it in no direction;
and a pid-less row under a session-creating verdict increments it. A REVIVE
counts as a dispatch: an attach exits 0 whether it revived the row or silently
did nothing, so without that a failing revive would be retried every poll forever
and never reach the guard. Three consecutive blind dispatches — the limit is a
constant, because the number is a property of the arrival-window design rather
than a per-fleet knob — latch the seat **halted**: the verdict becomes `halt`,
one `session.halted` and one line are written at the transition and never once
per poll, and at the limit a later sighting does not drop the count below it, or
the hold would lift itself with no operator act. The remedy is a person's:
`fleet event clear-halt <seat>`, consumed on the controller's next tick before
any verdict is reached.

## effect

Four of the six verdicts are carried out here, each writing its own event once.
`spawn-woken` starts a session in the seat's worktree with `--bg`, the seat's
name, its model, the fleet's permission posture and the rendered first turn as
the prompt — the model and the posture on every call, never the agent's own
defaults — with the child's output going to a **file** under the machine
directory's `starts/` and the child watched for `controller.start_watch_seconds`:
one that exits inside that window is a `session.crashed` carrying its status and
the file, and one still running when it closes is a `session.spawned` and a row
in the session table, because arrival is the roster's answer and never the
start's own exit. `rest` is stop, then start the successor, then remove the
predecessor's row, in that order and by the **short id** from the roster, which
is the address a stop takes and is not the session id; a stop that does not exit
0 starts nothing, removes nothing, leaves the rest pending for the next poll, and
is the alarm the contract names — a `seat.resting` with no `session.rested` after
it. `suggest-rest` sends one nudge, once per session, and never a second.
`revive` dispatches an **attach** of the pid-less row's short id, so the session
continues in place with its id and its context intact — only a flagless full-id
resume continues a session and a flagged one forks it — and it emits
`session.revived`; the row is then re-opened as this dispatch's, waiting on a
sighting, because the attach's own exit is not a witness that it took. A pid-less
row with no short id cannot be revived, says so, and is left to the next poll.
`halt` is the verdict whose act is to dispatch nothing: it publishes `halted` as
its outcome, and its line and its event were written once at the transition.
Every process the controller starts carries
a **constructed** `PATH` from the platform layer and an environment built rather
than inherited, and the binary an effect execs is resolved once at startup —
`$FLEET_CLAUDE_BIN` when it names an absolute path, else the first `claude` on
that constructed `PATH`. Unresolvable is not fatal: the loop observes and
publishes with the projection's `effects` field reading `off` and the cause
beside it.

`sessions.json` in the machine directory is the controller's own memory of what
it started: the stream cursor, the map of sessions already nudged, the blind
counter and the halt latch per seat, the daemon's pid at the last poll, and one
row per session with its seat, project, worktree, name, model, posture, first
turn, the id of the `session.spawned` that opened it, and — filled at the first
sighting, matched by worktree — the agent's session id, the short id and the
stamps. It is private and holds no work item.

It is also **not a second source of truth**. At startup the controller ADOPTS:
for every session the table names, it asks the roster this poll already read
whether that session id is live and claims the live ones by id — no respawn, one
`session.adopted` each — so a restart re-hosts nothing, and a session the roster
no longer carries falls to the discriminator on that same poll. A table that is
missing, unparseable or of a schema this build refuses is REBUILT from the event
stream rather than trusted or invented: a `session.spawned` or a
`session.adopted` opens a row, a `session.rested` closes the predecessor it
names, a `dispatch.blind` carries the counter, a `session.halted` with no
`seat.clear_halt` after it is a standing hold, and the cursor is the last
sequence folded. The daemon pid is not folded and starts at none, which reads as
no replacement rather than a false one.

## event

`fleet event woke | handed-off | exited <seat>` records a lifecycle event on the
stream and always writes: a record that refused because the fleet looked quiet is
one nobody can go back and take. `fleet event rest <seat> --reason <text>` is the
one that asks for something, so it is the one that can refuse — exit 5 when no
collector is consuming (there is no projection, or its `generated_at` is older
than three poll intervals), exit 4 when the seat has no live session, exit 6 when
the row is a spawned one, which never rests and wants `fleet seat retire`
instead.
`fleet event clear-halt <seat>` is the other one that asks: it refuses with exit
5 on the same collector check, because a request nothing would consume leaves the
seat held, and with exit 1 when the seat is not halted, printing the state and
the blind count it read from the published row. All of them read the stream back
before exiting 0, and a read-back that does not match exits 2: a caller acting on
a 0 stops working and waits for a successor, so "it was written" has to be a
reading of the file.

## routine

Renamed from `order` on 2026-09-18 (fleet-layers Q3): the verb, the events and
the prose say routine; the file (`orders/<name>.toml` carrying `[order]`), the
state file, the projection's array and the events' `order` payload key keep the
file format's word until that format is renamed, and `fleet order` is refused
with exit 2 and the pointer for one release.

Standing duties are flat files this same tick evaluates — no second daemon and
no per-routine service job — and a controller that is down fires nothing and says
so by the absence of its own events. `orders/<name>.toml` is read from three
roots on every tick that evaluates: the fleet root's `orders/`, every installed
pack's under the machine directory's `packs/`, and every project's. Each loaded
routine carries the root it came from as `fleet`, `pack:<name>` or
`project:<name>`, and one name in two roots refuses **both** files naming both
paths, because picking one silently is how a duty runs from a file nobody is
looking at. The loader is a pure function over the list of roots and is re-run
per tick, so a file dropped into a routines directory is live on the next
evaluation with no restart.

A file carries `[order]` and one `[action.<kind>]` and no third table, and every
value this reader does not understand is a **defect** with the file and the key
named — a misspelled key would be a duty that silently never runs, and a trigger
parameter under the wrong trigger is a plan its author believes in and nothing
reads. A defective file never fires and is a `DEFECT` row in `fleet routine list`.
The three triggers are `cron` (five fields against LOCAL wall-clock truncated to
the minute, `*`, `*/N`, an integer or a comma list, 0 and 7 both Sunday, a range
refused with the list that replaces it), `cooldown` (`now - last_fired` against
an interval) and `condition` (a command under `sh -c` in the routine's project
root). The trigger's answer is **three-valued and never rounded**: a check that
outran `check_timeout`, one that could not be started, and one that exited a
status the routine names in `check_unknown_exit` are all *could-not-tell*, which
is what keeps a duty whose instrument is out from reading as a quiet one. The
four actions are `nudge` (ring a live seat, with an `[action.item] when =
"absent"` beside it as the graceful fallback), `item` (file one on the project's
work graph, carrying `routine:<name>` so a `dedupe` can read it back), `exec`
and `run` (`[action.run]` names a workflow and an `inputs` table; firing it
calls `fleet run <workflow> --input key=value… --by <routine>` in the routine's
project root, so the routine's `fired` and `completed` wrap the run's `started`
and `closed` on the one stream, and the terminal event carries the run id as
`run`).
Every child of an action carries the constructed search path with this
controller's own directory in front of it and `FLEET_DIR` naming the machine
directory.

The **stream is the ledger**: a firing writes `routine.fired` and then one of
`routine.completed`, `routine.failed` or `routine.could_not_tell`, and a not-due
evaluation writes nothing at all. `orders/state.json` under the machine
directory holds, per routine, when it was last asked, when it last fired, what it
last did and the consecutive failing outcomes since its last success — and that
streak is published in the projection's `orders` array every tick, so a duty
that has been failing for a week is a number a reader meets rather than a
silence. The state is written BEFORE the action on every firing: a duty that
happened and was not recorded fires again forever, and a double firing costs one
turn.

`fleet routine list | check <name> | run <name> | history [<name>]` are the four
verbs. `check` evaluates the trigger now, running a condition for real, and writes
nothing; `run` takes the routine's lock, fires it outside its schedule and prints
the event row it produced, with `--force` overriding the trigger and never the lock
and `--dry-run` writing nothing at all; `history` reads the routine events back
off the stream. A `run` holding a routine's lock makes the tick skip that routine
with one line and no event.

`fleet.example.toml` beside this file is the smallest policy that runs, and is
what a scratch fleet directory can be pointed at. `routines.example.toml` beside
it is one routine with every other trigger and action it could have taken in
comments.
