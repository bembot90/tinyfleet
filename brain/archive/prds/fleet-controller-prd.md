**History, not maintained.** Archived on 2026-09-24: this PRD is the record of what was decided, and nobody updates it. The code and `docs/` are the source of truth for what fleet does.

# PRD: fleet-controller

**One sentence:** one boring daemon per machine that keeps a fleet of coding-agent
seats alive through the night — brings them up, brings them back when they rest,
hands a seat's thread to its successor, fires the standing routines on its own
clock, publishes what it saw and never what it believes — over any number of
projects, through an adapter seam that makes the agent a setting.

Drafted 2026-09-05, from the vision page (`fleet-vision.md`), the Gas City
engine trial of 2026-09-05, and the **reference controller**: the daemon that
has run the first fleet, behind a live product, since August 2026. The
controller described here is written new. Every requirement below says whether
it distills the reference (**R**), was measured in the trial (**T**), or is new
with fleet (**N**), so the builder knows which claims already survived a night
shift and which have not. The reference's own record — the incidents that
bought each rule — is kept with the reference and cited from the work items,
not from this page.

---

## Problem statement

Coding agents finish real work unattended; nothing around them keeps them
working. A seat that rests needs a successor that wakes oriented. A seat that
crashes needs its context back, not a fresh wake. A machine that upgrades its
agent binary stops every session at once and needs one operator who reads that
as one event. Duties that run on a schedule need one clock and one ledger. A
person asleep needs the night to be a record in the morning.

The reference controller does all of this today: a headless Rust daemon that
observes sessions, decides from a pure table, carries effects out through one
layer, publishes a projection, and writes no issue — plus seven tools around it
(spawn, feed, retire, courier, orders, runway, wake-cost) and twenty-five living
documents that say why. It works, and it is bound to one repository root, one
issue prefix, one worktree per seat, one launchd label, one machine directory
and one agent. It is also 9,000 lines whose every rule was bought by an
incident — which is the asset, and the reason the controller is written from
its record rather than from a blank page.

The Gas City trial measured the one engine that already does this job
generically. Its session layer is real (adoption on restart, restart in a tick,
nudge under two seconds, live config reload), and the same trial measured what a
user meets around it, on gc 1.4.1 on 2026-09-05 (lessons gas-city G1, G2, G4,
G11, G13, G15): an init that installs a launchd service and starts an agent at
effort max with permissions skipped, telemetry on with no disclosure shown, a
forced re-init that deleted a project's export, crashes that leave no event, an
idle timeout that did not fire for an interactive session. The recommendation
on the trial's report: **an independent controller that borrows the shapes** —
adoption on restart, the flat routine file, the event stream's transport — and
owns everything a person meets.

## Goals

1. **Lights on.** A named seat that is absent, rested or crashed is brought
   back within one poll, oriented, with its context or its diary — and never
   twice into one worktree.
2. **Projects, plural.** One controller runs seats across any number of registered
   projects; an issue's id says which project's worktree the session opens in.
3. **The agent is a setting.** Every act on a session goes through one adapter
   with seven verbs — start, stop, adopt, nudge, status, transcript, version —
   so the second agent is an adapter and not a rewrite. Claude Code is the
   first adapter.
4. **Routines on the controller's clock.** Standing duties are flat files the
   controller's own tick evaluates and fires; no second daemon, no per-routine
   service job.
5. **The record, not the belief.** One event stream and one projection say
   what the controller observed and did; neither ever claims liveness; a
   failure is a named absence, never silence.
6. **Safe to leave alone.** Rest is suggested and never enforced; work is
   given and never taken; the guards that cost incidents to learn are on by
   default and opted out of, never remembered.
7. **A known substrate.** Each agent the fleet runs is pinned to a measured
   version; a live version that differs is a flag to re-measure, and the
   controller is what notices.
8. **One binary, two operating systems.** The controller installs, runs as a
   user service and behaves the same on macOS and Linux. Everything that
   differs between them — the service manager, the machine directory, the
   file-access permission model, the process table — sits behind one platform
   layer, and the suite runs on both. Windows is not a target. The one binary is
   built by a workspace of three crates — the controller and the core as
   libraries, the cli crate holding the single `[[bin]]` named `fleet` — so the
   boundary between the process table and pack-and-project work is a dependency
   edge and not a convention (see `../../README.md`).

## Non-goals

- **The manager.** No UI, no HTTP surface, no dashboard. `fleet-manager` reads
  the projection and the events; this daemon writes them.
- **How work is done.** Dispatch, deliver, review and land are the core
  pack's verbs; how a spec is written, how a decision is surfaced, the rituals
  are the tiny pack's. The controller runs seats; it does not run flights.
- **Composing flights.** Composition is a workflow's — tiny's preboard and
  takeoff over `fleet run` (workflows-formula-fate, 2026-09-17); the controller
  fires the routine that starts one and re-runs what waits.
- **A second agent's adapter.** The seam ships; only Claude Code's adapter
  ships first.
- **Hosted or remote projects.** One machine, one user, one controller.
- **Replacing Beads.** The work graph is bd's; the controller writes an issue
  only where a routine's declared `[action.item]` says so (the cli PRD's Q2
  ruling of 2026-09-08 names that table).

## Recorded directions

Rulings this PRD is built on, each with its date:

- **Independent controller; borrow the shapes.** The Gas City trial's
  recommendation, 2026-09-05; the trial was closed on one sitting's evidence
  without the planned seven-day re-measure.
- **A rewrite keeps three layers and changes four things.** Settled in the
  trial's closing sitting, 2026-09-05: keep the pure decide layer, the
  preflight and the cost instrument; change to a provider trait between decide
  and effect, **a persisted session table — small and ours, so a restart adopts
  rather than re-hosts, not a Dolt ledger** — the projection served as an API
  with an event stream while the file stays for the tools that read it, and
  routines and the courier's delivery inside the loop.
- **The scheduler is part of the controller.** Ruled on the vision page,
  2026-09-04, after the trial showed routines belong on the controller's tick.
- **Agent-agnostic; Beads the one dependency.** Ruled on the vision page,
  2026-09-04.
- **The pack format is Gas City's, verbatim.** Ruled on the vision page;
  measured holding in the trial (a one-agent pack swapped in with one reload).
  Routines as `orders/<name>.toml` are the same shape (five firings on the
  minute).
- **Written new, the reference controller read and never copied.** Ruled
  2026-09-05 on the migration plan.
- **One fleet repository, the controller as `controller/`.** Ruled 2026-09-05.
- **Permission posture: auto mode, denials surfaced, no bypass, for every
  seat.** The standing posture the reference's spawn honours and never widens,
  ruled 2026-08-31.
- **Starting a named seat is the human's; the daemon is their installed
  delegation.** A seat may start only its own successor when it rests; spawned
  builders are the one exception, started by an architect for one item of
  work.

## Design overview

### The three roots, from the first commit

The controller resolves three things and never conflates them: **FLEET**, its
own install — policy (`fleet.toml`), the adapters, the packs it has
installed (core, and tiny on top of it) with the seat identities they
carry; **PROJECT**, a registered project — its `.fleet/project.toml`,
its work graph, its worktrees, its suites; **MACHINE**, the fleet directory —
`config.json`, the projection, the events, the sessions table, the switch, the
ledger. Its default is `~/.fleet` on both platforms (`$XDG_STATE_HOME/fleet`
on Linux when that is set). Two environment variables move it and mean two
things: `FLEET_DIR` is the directory itself and wins outright; `FLEET_HOME` is
the home it sits under. One resolution order, everywhere.

### Two modes: embedded and standalone

A directory that declares itself a project is one, and failing that, where
`fleet.toml` is found decides the mode. A `.fleet/project.toml` wins at its own
level: the declaration is that directory's own statement about itself, and a
`fleet.toml` beside it may be some other tool's file rather than this project's
fleet.

**Embedded** — the file sits at the root of the one project the fleet runs.
FLEET and PROJECT are the same directory: the project's own work graph is the
board, seat state lives under `.fleet/` beside the code, worktrees go beside
the repository, and every other value derives from the directory — the
project's name from its basename, the issue prefix from its Beads config. This
is the side-project case the product promises minutes for. There is no
`project.toml`; the one project is implied.

**Standalone** — `fleet.toml`, the seats and the packs live in their own
repository, and each project the fleet works on declares itself in
`.fleet/project.toml` and is registered with that fleet. This is the shape a
fleet grows into when it runs more than one project, and the shape the first
fleet migrates to.

**One command creates either.** With fleet installed and the controller
running, `fleet create`, run from inside the project, is the only command
needed to have a working fleet: it asks which mode — embedded or standalone —
and writes the config for it. Embedded: one `fleet.toml` at the project root
whose every key has a default the controller supplies, the smallest working
file naming the seats and nothing else. Standalone: the project's
`.fleet/project.toml`, and the project registered with the standalone fleet the
machine already runs. Then `fleet start` brings the seats up. The command
surface beyond this is the cli's PRD; the controller's contract is what the
two files say and how the mode is read.

One controller, one platform layer, one adapter, one event stream serve both.
A mode is never a setting: the controller reads it off where the file was
found, an embedded fleet becomes standalone by moving `fleet.toml` out and
running `fleet create` again, and every command works the same in either.

### Two platforms, one platform layer

The controller ships as one Rust binary for macOS and Linux, and the reference
— macOS only, launchd only — is read as one implementation of a layer, not as
the design. Everything the operating system supplies goes through one module
with one implementation per platform, and nothing outside it names a platform:

| Concern | macOS | Linux |
| --- | --- | --- |
| user service: install, load, unload | launchd agent, `~/Library/LaunchAgents`, KeepAlive | `systemd --user` unit, `WantedBy=default.target`, `Restart=always`, lingering enabled so it runs with no terminal open |
| machine directory | `~/.fleet` | `~/.fleet`, or `$XDG_STATE_HOME/fleet` when set |
| atomic write | temp file + `rename(2)`; a reader never sees a torn document | the same |
| file-access permission gate | TCC: the grant requested at startup, `pending` until answered (the reference's incident) | none; the gate reads `ok` |
| process table: pid, uptime, daemon pid | `sysctl`/`proc_pidinfo` behind the layer, never `/bin/ps` | `/proc/<pid>` |
| child PATH construction | constructed, never inherited, from the platform's search dirs | the same |
| the agent's roster and transcript locations | as the adapter finds them on that platform; a location is a measurement per platform and per pin, never a constant | the same |

Windows is not a target and has no documented path; the layer exists so that
the two platforms stay honest with each other, not to hold a door open. The
adapter carries its own platform column: whether Claude Code's background
sessions, its daemon and `claude agents --json` behave the same on Linux as on
macOS is **measured at the pin**, and the substrate table records the answer
beside the version. The reference measured one platform; the first build
measures the second before it is claimed.

### Seats are rows; a row has a worktree per project

`~/.fleet/config.json` is the machine's list of managed seats, written by
`fleet` commands and re-read whenever its mtime moves, with no lock: every
writer renames a temp file over the path, so a reader sees a whole old file or
a whole new one. A row carries the seat directory, its chosen name, its model,
and **`worktrees`: a map from project name to path**. A dispatch names the project by
the issue's prefix, read from the project's `project.toml`; the session opens in that
worktree with cwd there. A row missing a name or a worktree is skipped loudly,
never defaulted — a child spawned into the wrong directory claims work as the
wrong seat. A row carrying `transient: true` is a spawned builder: observed and
published, never revived, re-spawned or rest-succeeded. A re-read that cannot
parse the file leaves the seat list standing, because the empty fleet a corrupt
file parses to is a legitimate configuration nobody asked for.

### The agent adapter: seven verbs

Everything the controller does to a session goes through one interface,
implemented once per agent:

| Verb | Claude Code adapter (from the reference) | What the trial saw in Gas City |
| --- | --- | --- |
| `start(worktree, model, name, first_turn, posture)` | `claude --bg --model … --name … "<first turn>"`, cwd the worktree, the child PATH constructed and never inherited | a tmux pane running `claude --dangerously-skip-permissions --effort max`; the posture is the adapter's to set |
| `stop(row)` | `claude stop <short id>`, the id read off the row | a second stop removed the launchd job (G8) |
| `adopt()` | on startup, for every row in the controller's own session table, ask `status` for that session id and claim it if it is live — no respawn, one `session.adopted` event each; a row the roster no longer carries goes to the discriminator | **adopted, not respawned**: the same pids across a supervisor restart — the one behaviour copied outright, bought there by a session bead in a Dolt ledger |
| `nudge(row, text)` | a throwaway `-p` session whose single act is one cross-session send; it carries the invocation and the authority it names, and no authority of its own | under two seconds to arrival; the seat obeyed its prompt in seven |
| `status()` | `claude agents --json --all`; rows matched by cwd, never by short id; `pid`, `state`, `startedAt`, `sessionId` | `gc session list`; a crash hold recorded in the reconciler trace (G14) |
| `transcript(row)` | the session's JSONL under the projects directory: main-chain context tokens, last turn's usage; sidechains skipped | `gc session logs` reads by working directory; ambiguous for an agent scoped to the config root (G16) |
| `version()` | `claude --version`, this poll, beside the pin | `gc version` |

The adapter is the whole rented half of the design. Every measured caveat of
the first adapter — the newborn's pid-null window, hibernation's
indistinguishability from a deliberate stop, the dead terminal host's two
shapes, the re-host after a daemon replacement — belongs to it and is versioned
there, against the release it was measured on.

### What is the adapter's and what is the fleet's — corrected 2026-09-17

Q4 below rules that the default posture is **the adapter's**. The controller as
built puts it in `[controller]` policy instead, together with four more values
spelled in one provider's vocabulary, and a policy default is what a fleet gets
when it states nothing — so a `fleet.toml` with no `[controller]` table is a
fleet that has silently chosen Claude Code. The one-sentence promise at the top
of this page is an adapter seam that makes the agent a setting; today nothing
reads that setting, and the five keys below are the reason.

| Key | Default in the controller today | What the value is |
| --- | --- | --- |
| `default_model` | `"claude-opus-5"` | a model id |
| `nudge_model` | `"claude-haiku-4-5-20251001"` | a model id |
| `auto_capable_models` | `["claude-opus-5", "claude-fable-5", "claude-sonnet-5"]` | model-id prefixes |
| `posture` | `"auto"` | a `--permission-mode` word |
| `transient_posture` | `"dontAsk"` | a `--permission-mode` word |

A second provider has other model names and may have no permission modes at
all, so none of these five is a value this layer can choose.

**The reader census** — counted on the tree of 2026-09-17, production sites
only, with the policy grammar itself and every test and fixture excluded,
because a knob in `fleet.toml` is a knob for everyone who reads the file:

- `default_model` — **4** readers: the start's model and the grant gate's in
  `run.rs`, a spawn's in `transient.rs`, the rendered seat row in
  `lifecycle.rs`, all four through one accessor. In the reference controller's
  own tools a `default_model` of the same name and meaning has **6** more
  readers, one of them a cross-check against a table of model ids that the
  controller's key has no equivalent of.
- `nudge_model` — **5** readers, in **two** crates: the nudge effect, the
  transient nudge, two routine actions, and one in the CLI.
- `auto_capable_models` — **2** readers: the prefix match behind the grant
  gate, and the skip line that renders the list verbatim to an operator.
- `posture` and `transient_posture` — **3** readers through one accessor, plus
  the gate, which compares its answer to the literal `"auto"`; and then **9**
  carriers downstream — the start spec, the `--permission-mode` flag, the
  persisted session row and its rebuild, and three event payloads.

Two things the census settles, and neither was anticipated:

1. **The posture word is durable.** It is written into three event payloads and
   into the persisted session table, and a session row is rebuilt from the
   payload. Rows already on the stream carry `auto` and `dontAsk` forever. The
   move changes the meaning of a field it cannot rewrite, so it owes a reading
   for an old row and not only a new spelling.
2. **One provider has two spellings here.** The substrate pin and the agent
   list name it `claude_code`; a pack's overlay slot names it `claude`.
   Selection by name needs one spelling ruled before any key is keyed on it.

And one correction of record: the concrete adapter is constructed at **5**
production sites across **two** crates, not at the loop alone.

**The mechanism.**

1. The adapter answers for its own vocabulary. `Agent` gains a defaults verb
   returning the provider's default model, nudge model, auto-capable prefixes
   and two posture words — owned by the adapter module and versioned there
   against the release they were measured on, exactly as every other measured
   caveat of an adapter is.
2. The five keys stay in `[controller]`, stay optional, and **unset means the
   adapter's default** rather than a value this layer names. The resolved
   policy holds an absence, and the accessors take the running adapter's
   defaults as their second argument.
3. The grant gate becomes the adapter's question, not a string comparison here:
   *would this row ask for a posture its model was not measured to honour?* A
   provider with no permission modes answers no for every row, and the gate
   disappears for it instead of being configured away.
4. **The loop selects an adapter by name.** The name is a seat's, with a
   fleet-wide default in `[controller]`, drawn from the agent list the policy
   already carries; the loop resolves it once and hands the trait object down,
   and an unknown name refuses at startup the way an absent `fleet.toml` does
   rather than falling back to the first adapter. One vocabulary then names the
   adapter, the `[substrate.<agent>]` pin and the per-provider overlay
   directory.

**Who owns it.** Not the second adapter: this is its prerequisite, because a
second adapter cannot be written against a policy whose absent table means one
provider, and the five slices that would have carried it are closed. It is
listed in P1 below, ahead of the second adapter in P2.

### Observe, decide, effect, publish

The loop is the reference's, kept because every arm was bought: a poll
(default 5 s, policy) gathers observations through the adapter, a **pure
decision function** produces one verdict per seat, a separate effects layer
carries it out, and the loop publishes a projection and appends events. The
verdicts:

| Verdict | Meaning |
| --- | --- |
| `leave-alone` | the common case; every seat whose observation is **Unknown**; every pid-less row during a **replacement window** |
| `spawn-woken` | seat absent: start a session whose first turn is the wake |
| `revive` | a pid-less row with no deliberate-end event (`seat.exited`, `seat.resting`) and context under threshold: adopt it in place |
| `rest` | an unconsumed `seat.resting` event stands: stop, start a woken successor, then remove the predecessor's row |
| `suggest-rest` | context crossed the threshold: one nudge per session, never a second |
| `halt` | three consecutive blind dispatches: stop dispatching, leave the seat down, say why once |

Three rules outrank the table: rest outranks spawn outranks suggest; the
transient filter turns every session-creating verdict into `leave-alone` and
sits outside the table so a spawn arm added later is covered by construction;
and the halt guard outranks the spawn it guards. **Unknown is not absent**: an
unreadable roster reaches the decision as Unknown for every seat, and two live
rows in one worktree are Unknown too, never a contest.

**The discriminator for a pid-less row that is not a newborn** is a
deliberate-end event plus a context guard, because the roster cannot tell a
hibernated session from a deliberately stopped one: an unconsumed `seat.exited`
or `seat.resting` for that seat → spawn the successor and mark the event
consumed; no such event and context under the rest threshold → revive in place,
same session, same id, context intact; no such event and context at or over the
threshold, or unmeasurable → spawn, because a
revived at-ceiling session reads healthy on every board and can do no work.
Every term is read fresh each poll, so the answer survives a controller
restart.

**New, from a live incident on the reference (2026-09-04):** a daemon whose pid
changed since the last poll, or whose uptime is under the arrival window, opens
a **replacement window** in which no pid-less row is dispatched against. The
reference spawned a duplicate seat into the minute in which the agent daemon
was re-hosting an old row after a binary upgrade, and then froze the seat
blind with two live rows. That is a requirement here, not a fix later.

### The session table: what the controller remembers

The reference remembers nothing between polls but a blind counter, and re-derives
every seat's session from the agent daemon's roster each time — which is why its
restart re-hosts nothing only by accident, and why two live rows in one
worktree are Unknown to it: it has no record of which one it started. The
controller keeps **a persisted session table, small and its own**, at
`~/.fleet/sessions.json`, one row per session it started or adopted: seat,
project, worktree, the agent's session id, the name it was given, the model,
the first turn, `transient`, the pid and daemon pid at the last sighting, the
last context reading, and the timestamps of the dispatch, the first sighting
and the last. Written atomically on every effect and on every sighting that
changes a row; read at startup, when `adopt()` asks the adapter for each
recorded session by id and claims the live ones.

Three things it buys. A restart **adopts by record instead of guessing by cwd**:
a session the controller started is its own on the next boot whatever else
stands in that worktree, so two live rows in one directory are Unknown only when
neither is in the table. The blind counter, the halt latch and the last daemon
pid live here rather than in the projection, so the halt guard and the
replacement window survive a restart without the published file having to carry
private state. And a spawned seat that dies leaves a row that says what it held,
which is what `fleet seat retire --dead` reads.

What it is not. **Not a Dolt ledger and not the work graph**: it holds no work
item and no message, and nothing outside the controller reads it. **Not a
second source of truth**: every row is derivable from the event stream — a
`session.spawned` or `session.adopted` opens it, sightings and `session.stopped`
close it — and a table that is missing or will not parse is rebuilt from the
stream at startup rather than trusted or invented. The projection stays the
published view of what was observed at `generated_at`, and this table stays the
controller's private memory of what it did.

### Two kinds of seat, and only one of them rests

**Named seats** are permanent identities — a charter, a diary, a history —
whose sessions are ephemeral: a named seat that runs low rests, and the
controller brings up a successor that wakes oriented from the record. **Spawned
seats** are started for one item of work and **retire**; they never rest, are
never revived and are never succeeded. The reference measured why the split
pays: a fresh successor's first turn read 117–121K tokens before doing any
work, about a quarter of the rest threshold, and every rest re-spent it — so
the default lifecycle for work is spawn, deliver, retire, and the rest cycle is
the named seats' exception. A row carrying `transient: true` is a spawned seat,
and the transient filter turns every session-creating verdict for it into
`leave-alone`.

### The controller runs on events, not files

The controller has no API of its own. Its inbound surface is **the fleet's
event stream**: a seat's lifecycle is a small set of events any workflow can
emit through the CLI — `seat.woke`, `seat.resting`, `seat.handed_off`,
`seat.exited` — and the controller consumes its own stream on every tick and
acts on what it finds. A rest is `fleet event rest <seat> --reason <text>`,
which appends one `seat.resting` event; the controller reads it, stops the
session, starts the woken successor, then removes the stopped predecessor's row
so the name never points at a corpse. The order is load-bearing and the removal
is only ever after a successful stop, because a remove aimed at a live row
neither refuses nor spares it. There is no marker file and no write to the work
graph: the request, its collection and its outcome are three lines in the same
stream a person reads in the morning.

That is what makes the ritual optional. The tiny pack's wake, sleep and catnap
skills call these commands as their last acts; a fleet with different rituals
emits the same events from different skills, or from a shell script, and the
controller cannot tell the difference. The reference bound its rest cycle to a
skill that wrote a file the daemon polled for, so a fleet without that skill had
no rest cycle at all; here the protocol is the events and the skills are one
emitter of them.

What `fleet event rest` promises: it refuses, naming which half failed, when the
seat has no live session or no collector is consuming the stream, because the
two have different fixes; an accepted event is consumed within one poll; and an
accepted event that is not consumed is the alarm — a named, attributable
absence rather than silence, visible as a `seat.resting` with no
`session.rested` after it.

**Suggest, never enforce**: the strongest verdict a crossed context threshold
produces is one `session.nudged` to the seat, and there is deliberately no path
from the threshold to an automatic rest. Whether to rest is the seat's own
judgment, made inside whatever workflow it runs.

### Routines on the controller's tick

Renamed from orders on 2026-09-18 (fleet-layers Q3, ruled 2026-09-17: the
word was overloaded with the dispatch record's "orders given", which keeps
it). The verb, the events and this page say routine; the file
(`orders/<name>.toml` carrying `[order]`), the state file, the projection's
array and the events' `order` payload key keep the file format's word until
that format is renamed.

`orders/<name>.toml` — the Gas City shape, measured in the trial — one file per
duty with a trigger (`cron`, `cooldown`, `condition`) and an action
(`nudge`, `item`, `exec`, `run` — the cli PRD's Q7 and Q2 rulings of 2026-09-08
name the two actions this page first called `courier` and `bead`; `run` calls
`fleet run <workflow>` with the inputs the file pins, and the routine's `fired`
and `completed` wrap the run's `started` and `closed`, the terminal event
carrying the run id). The reference's
vocabulary is kept: `cron` matches
five fields against local wall-clock to the minute; `cooldown` fires on
`now - last_fired`; `condition` runs a command in the project root and reads its
exit — 0 due, non-zero not, and a command that outran its `check_timeout`,
could not start, or exited a status the routine names in `check_unknown_exit` is
**could not tell**, a third answer never rounded into "not due".
`[action.item] when = "absent"` is the graceful fallback: a ring that finds no
live seat leaves the duty on the work graph instead of nowhere. The controller
evaluates every routine on its tick and fires the due ones, and the **stream's own
routine events are the ledger** — `routine.fired` and one terminal type per firing,
with a not-due evaluation writing nothing, read back by `fleet routine history`
with a sequence cursor (the cli PRD's entry for that verb, ruled after this
page, in place of a `~/.fleet/orders/ledger.jsonl` beside the stream). Packs and
projects both carry `orders/` and the controller reads both. The health patrol the
vision page's controller runs — a work-graph endpoint file re-asserted, the
agent version checked — is itself a routine.

### The event stream is fleet-log's first writer

`~/.fleet/events.jsonl`, append-only, one JSON object per line, monotonically
sequenced, in the envelope the trial found right — an id, a sequence, a
timestamp, a type, a typed payload — with the actor as the seat's name and
never a hash. **The stream runs both ways**: the controller writes what it did,
and seats write what they ask, through the CLI, and the controller consumes
those on its tick. Seat-emitted types: `seat.woke`, `seat.resting`,
`seat.handed_off`, `seat.exited`. Controller-emitted types:
`controller.started`, `controller.stopped`, `session.spawned`,
`session.adopted`, `session.revived`, `session.stopped`, `session.rested`,
`session.crashed`, `session.halted`, `session.nudged`, `dispatch.blind`,
`routine.fired`, `routine.completed`, `routine.failed`, `routine.could_not_tell`,
`project.registered`, `substrate.moved`. The content rule
is the trial's finding inverted: **emit the events a person cares about** — a
crash, a nudge, an adoption, a hold — and never fill the stream with the
controller's own health orders. On an idle Gas City instance, 540 of 676 events in
four hours were the health orders firing and completing.

### The projection never answers liveness

`~/.fleet/projection.json`, versioned, written atomically: every field is what
was observed at `generated_at`; no pid, no handle, no running belief, because a
truthy pid in a state file was once read as a live collector and a seat was
promised a successor that never came. `in_flight` names the seat and effect
while a poll is blocked inside an effect, written before the blocking call, so
a slow effect and a dead controller can be told apart. `grant` is a gate: while
the operating system's file-access grant is pending the loop observes and
publishes and issues no effects, and the blind counter does not move.
`fleet` is the policy in force with `fleet_parse_error` beside it while the
loop runs on last-good. Freshness is the collector's own liveness signal.
`fleet runway`, `fleet status` and the manager are readers.

### Policy, local, pin

`fleet.toml` is policy and is re-read on mtime: startup refuses without it and
exits 3, because there is no last-good before the first read; a running
controller that meets a file that does not parse keeps last-good and logs once
per change, never once per poll. `config.json` is local and beats policy per
key. `[substrate.<agent>]` pins the version each adapter's behaviours were
measured against; the projection publishes measured beside live, and a
difference is a flag to re-measure, surfaced by a routine that files an issue.

Five of `[controller]`'s keys default to one provider's model ids and
permission-mode words, which makes an absent table a choice of provider rather
than a choice of nothing; they move behind the adapter in P1 — § What is the
adapter's and what is the fleet's.

## Requirements

**R** = distilled from the reference controller or its tools; **T** = measured
in the Gas City trial; **N** = new with fleet.

### P0 — the first gate: fleet can build itself

**Install and identity**

1. `fleet install` writes the platform's user service under a fleet label —
   a launchd agent on macOS, a `systemd --user` unit with lingering on Linux —
   creates the machine directory, and **does not start anything**; load is a
   second deliberate act. On macOS the file-access grant is requested at
   startup before the first poll so it is answered in the minute the service
   loads; on Linux the gate reads `ok`. R for the shape; N for Linux, which the
   reference never ran on; T: an init that installs and starts was the trial's
   second finding.
2. Telemetry: **off, and asked**. No metrics leave the machine without a
   disclosure shown and accepted. T: the trial's third finding.
3. Two modes, decided by a directory's own declaration and then by where
   `fleet.toml` is found, and one command that creates either: `fleet create`,
   run from inside the project, asks embedded or standalone and writes the
   config. **Embedded**: the file at the
   project's root, FLEET and PROJECT one directory, every value defaulted from
   the directory, no `project.toml`; the smallest file that runs. **Standalone**:
   the project's `.fleet/project.toml` (name, issue prefix, worktrees
   directory, primary checkout, gates) and the project registered with the
   standalone fleet; a project without the file is refused with the file to
   write. In either mode the controller **never initialises and never rewrites
   a project's work-graph store**. N for the modes and the command; T: the
   trial's first finding for the refusal.
4. Seats come from `fleet.toml [seats]` (class, model class, status) rendered
   into `config.json` rows with `worktrees` per project. `status` is `active` or
   `parked`, and the harness's own words `vacationing` and `chartered` are read
   as `parked` so one file serves both readers; any other word is refused. R; N:
   the per-project map.

**Observe**

5. Roster rows are matched to seats by cwd; the short id is never compared;
   two live rows in one worktree is Unknown, not a contest. R.
6. An unreadable roster is Unknown for every seat, and Unknown is always
   `leave-alone`. R.
7. Context tokens come from the transcript's main chain, sidechains skipped;
   the roster carries no token field. R.
8. The agent's version is read every poll and published beside the pin. R.

**Decide**

9. The six-verdict table with the three outranking rules, as a pure function
   with fixture tests for every arm. R.
10. The deliberate-end-event-plus-context discriminator for a pid-less
    non-newborn row; an unmeasurable context falls to spawn. R for the shape;
    N for the event in place of the reference's file.
11. The replacement window: no pid-less row is dispatched against while the
    agent daemon's pid has changed or its uptime is under the arrival window;
    the log says `replacement window: held`. N, from the 2026-09-04 incident.
12. Transient rows never receive a session-creating verdict; `halt` and
    `suggest-rest` pass through. R.

**Effect**

13. Every dispatch — spawn or revive — opens an arrival window keyed to the
    dispatch the controller recorded, never to the row; a sighting answers the
    window; the blind counter moves once per window. R.
14. Three consecutive blind dispatches halt the seat; the counter persists
    across restarts and decays rather than clears; `fleet clear-halt <seat>` is
    the only remedy, and a halt is announced once per transition. R.
15. A poll in which at least half the seats, never fewer than two, stand on
    pid-less rows logs one upgrade line, not N absences. R.
16. The rest collection order is stop → start successor → remove predecessor,
    and the removal only after a successful stop. R.
17. The controller persists a session table at `~/.fleet/sessions.json` —
    one row per session it started or adopted, written atomically on every
    effect and on every sighting that changes a row; it holds the blind
    counter, the halt latch and the last daemon pid, so both survive a restart.
    A missing or unparseable table is rebuilt from the event stream, never
    trusted or invented. Startup **adopts** every live session the table names,
    by session id, and emits `session.adopted`; a controller restart re-hosts
    nothing. T: the behaviour copied outright; N: the table, small and the
    controller's own, not a Dolt ledger — the reference has adoption by accident
    and no memory of what it started.
18. `start` passes model, name and the fleet's permission posture on every
    call; the default is never the agent's. R; T: the trial's fourth finding.
19. The child PATH is constructed, never inherited, on every process the
    controller starts — a daemon started bare hands every later session a PATH
    that collapses mid-run. R.

**Rest**

20. The seat lifecycle is four events any workflow emits through the CLI —
    `fleet event woke | rest | handed-off | exited` writing `seat.woke`,
    `seat.resting`, `seat.handed_off`, `seat.exited` — and the controller
    consumes its own stream each tick; there is no marker file and no
    controller-side write to the work graph. `fleet event rest <seat> --reason`
    refuses, naming which half failed, when the seat has no live session or no
    collector is consuming; an accepted `seat.resting` is acted on within one
    poll, and one with no `session.rested` after it is the alarm. R for the
    checks and the collection order; N for the seam.
21. Only named seats rest: a `seat.resting` for a spawned (`transient`) seat is
    refused at the CLI with `fleet seat retire` named instead. One
    `session.nudged` per session on a crossed threshold, keyed on session id; no
    path from threshold to rest. R.

**Routines**

22. `orders/<name>.toml` from the fleet install, every pack and every project,
    evaluated each tick: `cron`, `cooldown`, `condition` with `check_timeout`
    and `check_unknown_exit`, the condition's command run in the project root. R;
    T.
23. Actions: `nudge` (ring a seat with an invocation and its authority), `item`
    (file one on the work graph, with `when = "absent"` and `dedupe`), `exec`
    (the two nouns are the cli PRD's Q7 and Q2 rulings of 2026-09-08, which are
    newer than this line's `courier` and `bead`), `run` (call `fleet run
    <workflow>` with the pinned inputs, the routine as the runner; the terminal
    event carries the run id). Every firing writes its own
    rows to the event stream — `routine.fired` and one terminal type — and there
    is no ledger file beside it (the cli PRD's **fleet routine history** reads a
    history from the stream's routine events with a sequence cursor);
    `could-not-tell` is a third outcome. R.
24. `fleet routine list | check | run | history`. R.

**Events and projection**

25. The event stream, append-only JSONL, sequenced, in the envelope above with
    the vocabulary above; an event is written by the layer that did the thing,
    once. N; T for the envelope and the content rule.
26. The projection, versioned and atomic, with `in_flight`, `grant`, `fleet`,
    `fleet_parse_error`, and per-seat rows carrying the roster state, context
    tokens, decision, outcome and blind count; a projection of an unknown
    version is refused whole. R.
27. `fleet status` prints the projection with GRANT PENDING above the roster,
    never below it. R.

**Policy and substrate**

28. `fleet.toml` re-read on mtime with last-good; startup exits 3 without it;
    `config.json` overrides policy per key. R.
29. `[substrate.<agent>]` pins; a live version that differs is a flag in the
    projection and a `substrate.moved` event, never a failure. R.

**Transient seats (the primitives the pack's `dispatch` calls; ruled
2026-09-05 on the packs PRD: the controller keeps the row, the worktree, the
first turn, the load belt and the retire — the order on the item, the
assignment and the brief's content are the pack's)**

30. `fleet seat spawn --first-turn <file>` — the load belt first (the 5-minute
    load average against a per-cpu ceiling, and transient seats mid-turn
    against a cap; an unreadable roster makes the second leg UNJUDGEABLE and
    refuses nothing, while the load leg keeps its teeth), then the worktree
    from the project's `origin/main`, the row with `transient: true`, the given
    text as the session's first turn; a rollback removes the worktree and never
    the branch. It writes nothing to the work graph; the caller does. R.
31. `fleet seat feed <seat> --first-turn <file>` — hands a live transient seat
    its next first turn and moves the row's occupant marker, put-back on
    failure, every row move journaled; refuses a seat still marked as holding
    one. The work-graph writes around it are the caller's. R.
32. `fleet seat retire <seat> [--dead]` — verifies from outside that nothing of
    the seat's holds RAM or disk, refuses on a surviving resource or an
    unreadable roster, prints the reclaim; `--dead` licenses by a completed
    roster read that names no live session, never by silence. R.

**Platforms**

33. One platform layer, one implementation per operating system, and no
    platform name outside it: the service manager, the machine directory, the
    atomic write, the permission gate, the process table and the child PATH
    (the table in § Two platforms). The suite runs on macOS and Linux in CI,
    and a change that passes on one and fails on the other does not land.
    Windows is not a target. N; the reference is macOS only.
34. The Claude Code adapter's behaviours are measured per platform at the pin
    — background sessions, the daemon, the roster listing, the transcript
    location, the newborn's pid-null window, the re-host after a daemon
    replacement — and `[substrate.claude_code]` records the answer beside the
    version for both platforms; an unmeasured platform is `pending` in the
    projection, never assumed to match. N.

**The run lifecycle's controller half (the MVP path's row 7; the last three
guarantees of fleet-layers Q11 are the controller's and not the process's — a
workflow process is short-lived, so what re-runs it, what stops re-running it
and what lets go of what it started all live on this side)**

The `[runtime]` table a run — and every re-run of it — bundles and executes
under resolves from the pack that carries the workflow file where that pack
declares one, else from the one pack it imports that declares one (two
declaring imports refuse, naming both), and the runtime's doctor check runs
against the pack that declares the pin.

35. The poll's run pass, after the routines pass: for
    every run whose last lifecycle event is `run.waiting`, the stream's head
    against the position that event recorded, and a re-run of the bundle already
    in the run directory once the stream has moved past it — at most once per
    poll, and never a fresh bundle. The run's OWN `run.waiting` line does not
    wake it: that line is appended above the position it carries, so the
    comparison takes the higher of the two. N.
36. A run whose last event is `run.could_not_tell` is re-run the same way up to
    `[core.run] max_crashes` (default 2, so the run is executed cap-plus-one
    times); at the cap the controller raises a gate on the run's record through
    the store's gate call with the last reading as its reason and emits
    `item.parked`. The park is latched on that event, not on the run's last
    lifecycle event, which does not move once nobody is executing it. N.
37. On `run.closed`, on `run.failed` and at the park, every seat whose
    `session.spawned` payload carries this run's id is retired through the
    transient retire path and one `run.cleaned` carries the count. The key is
    written at the spawn from `FLEET_RUN_ID` in the spawning process's own
    environment, so a seat spawned outside a run carries none and is untouched;
    the cleanup is latched on `run.cleaned`, and a run that spawned nothing is
    still cleaned, with a measured zero. N.

**The three acts above are a seam this crate does not fill** — a re-run needs
the project's store, packs and policy file, a park needs the store's gate and a
retire needs the project's primary checkout — all of which are wired in the
binary. The flights pass that once sat beside this one left with the advance
(workflows-formula-fate, 2026-09-17): the run pass is the poll's one call into
a lifecycle. The DECISION is the controller's, because it is a fold of the
machine's own stream against one cap.

### P1 — after the first gate

- The SSE surface over the event stream for the manager, heartbeats every
  fifteen seconds, resumable ids — the transport the trial measured.
- `fleet doctor`: the control lines (the work-graph endpoint file, the daemon
  pid, the grant, the projection's age) as one command with a third answer.
- Rest grants and roster releases for runs read off the run's record as
  `fleet` verbs.
- The row-moves journal and an incidents collector reading the event stream
  instead of the log.
- An idle reclaim of fleet's own, on the source G15 names: a seat idle past a
  per-class bound is suggested rest, still never enforced.
- **The provider-neutral policy** (added 2026-09-17): the five provider-named
  policy defaults move behind the adapter, the grant gate becomes the adapter's
  question, and the loop selects an adapter by name instead of constructing one
  — § What is the adapter's and what is the fleet's, which carries the keys,
  the reader census and the mechanism. It is listed ahead of the second adapter
  because it is that work's prerequisite and not part of it.

### P2 — designed for, not built

- A second adapter (Codex, a local model) proving the seam — on the
  provider-neutral policy of P1, never before it.
- A per-seat `attach` for a human to sit in a session from the manager.
- Remote projects and a hosted controller.

## The first slice — the first build

**"The controller observes and publishes."** A `fleet` binary that reads one
embedded `fleet.toml` at a project's root, lists sessions through the Claude Code
adapter's `status`, `transcript` and `version`, matches rows to seats by cwd,
and writes the projection and the event stream — **no effects**. It is small
enough for one build run, real enough to run beside the reference controller on
the same machine reading the same roster, and its acceptance is a diff: the
projection it publishes for the named seats against the reference's projection
at the same minute, row for row. Slice 2 adds `spawn-woken`, `rest` and the
seat events (R9–R21); slice 3 the routines (R22–R24); slice 4 revive, the replacement
window, halt and adopt (R10–R17); slice 5 the spawn tools (R30–R32). **The
platform layer (R33) is in slice 1**, because a controller that grows up on one
operating system acquires that system's assumptions in every later slice; the
first build compiles and passes its suite on both, and the Linux adapter
measurements (R34) land when Linux first runs a real seat. The
first gate is slice 5 landed and one fleet work item flown through it with Gas
City stopped.

## Success metrics

| Metric | The reference today | Target at the first gate |
| --- | --- | --- |
| halts, ghost sessions, duplicate rows | measured; two duplicate-row incidents in one week | zero duplicate rows across a replacement window, measured by a rehearsed agent upgrade |
| blind dispatches per week | published in the projection | zero outside a real outage |
| arrival, dispatch to sighting | 3.19–3.92 s on Claude Code 2.1.247 | the same, on the fleet's pin |
| age of an unconsumed `seat.resting` | the alarm (a file, on the reference) | under one poll |
| routine failures a reader consumed | a 57-tick failure streak once went unseen for three days | every failure an event something read |
| events per idle hour | — | under ten, none of them health orders |
| adopt on restart | by accident, unmeasured | 100 percent of managed rows, one `session.adopted` each |

## Decisions — ruled 2026-09-05

**Q1 — Language: Rust**, as the reference is — the decision table's pure
function, its fixture tests and its mutation harness port as designs, and a
boring daemon wants no runtime. Declined: Go (bd's and gc's language, one
static binary the same way, the option of embedding bd later); Python was not
offered, because the daemon runs as a user service on two operating systems
whose Python the fleet does not control. **Dependencies: any crate, each added
with a work item that says why** — re-ruled 2026-09-07, replacing the
stdlib-class-only rule of 2026-09-05. That rule cost nothing at the clock,
whose UTC calendar is eleven lines, and would have cost real code at the
routines, whose `cron` matches local wall-clock, which the standard library
cannot read.

**Q2 — The routines' clock: the controller's tick**, as ruled on the vision page
— one clock, one ledger, routines from packs and projects read together; a controller
that is down fires nothing and says so. Declined: one launchd job per routine,
the reference's shape, a second scheduler that survives a controller crash.

**Q3 — The event envelope: Gas City's shape, the seat named** — an id, a
sequence, a timestamp, a type, a typed payload — so a manager written against
theirs reads ours, and a fleet event never carries a hashed actor. Declined:
starting from the reference's log line, which no tool parses today.

**Q4 — The adapter's default posture for any fleet: ours** — auto mode,
denials surfaced, no bypass, `dontAsk` for spawned seats, the installer stating
it in one line. Declined: the agent's own default, which under Gas City
measured as `--dangerously-skip-permissions --effort max`.
