# fleet, in layers

**Status: the layers are ruled.** Drafted from a sitting on 2026-09-17; the fifteen questions it left open were ruled the same day (the rulings section at the bottom carries each one with its date), and three of them were sent to the workflows sitting, which ruled them the same evening (Q6, Q9 and Q10 below; the workflows section is rewritten in the definite).
This page fixes the seams: which pieces are substrate, which are features
built on the substrate, and which are opinions shipped as packs. It is the
map the PRDs under `prds/` will be re-read against once the questions at the
bottom are answered. Where this page and a PRD disagree today, the PRD is
what is built and this page is where it is going.

The vision page says what fleet is. `integrations.md` says where you can
reach in. This page says where each piece lives and why, so that the
product fits the way agentic development is going: a small foundation, a
typed surface on top, and everything with an opinion installable and
removable.

---

## The three layers

| Layer | What it holds | Test for membership |
| --- | --- | --- |
| **Substrate** | The things fleet stands on and abstracts away: the store, the agents | Could a user swap it for another implementation without fleet changing? |
| **Feature** | The capabilities fleet unlocks on the substrate: seats, items, routines, the log, packs, the API, workflows, views | Will everyone running a fleet need it, whatever their process? |
| **Opinion** | How work should be done, shipped as packs: tiny is ours | Could a second fleet leave it out entirely and still fly? |

One clarification the layers need spelled out. The feature layer is not
opinion-free. It holds the **safety opinions**: the record is append-only,
work is given and never taken, a delivery is refused when it cannot be
verified, a run pins its inputs. Packs hold the **process opinions**: how a
spec is written, how a review is read, what a seat does when it wakes. A
pack can replace every process opinion and none of the safety ones. That
line is the product, and it is what "nothing in the substrate or feature
layers dictates how a pack works" has to be read against.

---

## Substrate layer

The pieces underneath fleet. Each is reached through one seam, and the seam
is designed so that fleet supports the contract, never the implementations.

### Store

**What it is.** The place the work graph lives: items, their notes, their
labels, their dependency edges, their gates. Beads is the first store and
the only one shipped.

**What it is not.** It is not a bucket for docs, tools or prompts. Docs and
tools are pack files with a lockfile of their own. Memory, if it ever needs
a store, gets a second contract rather than a widened first one.

**The seam today.** A Rust trait in core with about fifteen methods:
ready, show, create, set title, note, assign, assigned-to, open-labelled,
set-orders, set-metadata, gate, open-gates, resolve-gate, and the text
rendering a brief carries. It is shaped like Beads 1.2.2 rather than like
fleet's needs: the top-level metadata merge quirk is two sibling methods, the
gate is Beads' human-gate object, and the ready list comes back "in the
store's own order", which plan then re-sorts by creation date because the
store's order is priority-then-newest. Nobody with a sqlite table could
implement that trait without reading Beads first.

**Where it is going** (ruled 2026-09-17, Q1 and Q2). A contract written to
fleet's needs, and an adapter shape a user can implement without asking us:

- The contract states what fleet needs. What "ready" means and in what
  order, what an item's fields are, how metadata objects are written, what a
  gate is. Every store-specific quirk lives in that store's adapter.
- The adapter is an executable with a fixed set of verbs, JSON in and out,
  the same shape the agent seam already uses. Beads already shells out per
  call, so an executable adapter costs nothing fleet is not paying today.
- A conformance command, `fleet store check`, runs the contract against an
  adapter and answers with the one exit table. Support is the conformance
  suite. A user who wires up sqlite runs the check and knows.

**The item types the API serves** are fleet's own Rust types over the
store's output. That is already ruled for the cockpit's item family (the
read-paths ruling of 2026-09-08). With a pluggable store, those types become
the store contract's types, which is the same thing said from the other
side.

### Agents

**What it is.** The employees. The coding harness that sits in a seat.
Claude Code is the first and only agent shipped. Codex and pi are the next
candidates, in that order of evidence.

**The seam today.** The controller's adapter trait with seven verbs: start,
stop, adopt, nudge, status, transcript, version. An eighth, attach, was
pulled into the cockpit's first slice by the roster addendum of 2026-09-08.
The model is a setting, the agent is an adapter, and the controller is the
only piece that knows which one is running.

**What a second agent costs, honestly.** An adapter, and a per-provider
overlay directory in every pack that ships hooks or a session prime. The
overlay slot is the one place a pack knows which agent runs the session, so
the cost is bounded and named, but it is not "one adapter".

---

## Feature layer

The capabilities fleet unlocks on the substrate. Each is a family of
commands on the one binary, a set of events on the stream, and where it
applies, a set of paths on the API. They are the primitives. Everything a
pack does is composed from them.

### Seats

**What it is.** Named, persistent agent identities with their own worktree,
their own history, and their own memory of yesterday, plus the spawned seats
that live for one item and retire. Spawn, feed, retire, nudge, attach.

**Presets and named seats** (ruled 2026-09-17, Q15). `fleet.toml` declares
both. A preset is a
shape a spawn takes: the agent, the model, the permission class, the brief
template, the rest threshold. A named seat is a preset plus an identity and
a worktree that survives sessions. Spawned seats are presets instantiated
per item; named seats rest and are succeeded, spawned seats retire.

**What the controller gives every seat.** Brought up, adopted across a
restart, revived in place after a crash, succeeded when it rests, retired
when it is done. Nobody writes a supervisor.

### Items

**What it is.** The unit of work the verbs move. Called item and never task,
because task is one of the store's types and item survives a second store.
This page briefly considered Tasks as the feature name and returned to item
on that ground.

**The verbs.** Dispatch, deliver, review, land. Each takes what it is given,
infers nothing, refuses what it cannot verify, writes its note on the item,
reads it back, and exits by the one table. Beside them: ask and answer for
a gate, and the item family the cockpit's task view needs, which is list,
query, create, update, claim, close and dep add, through the store.

**Where the lane's mechanics live.** The rebase of a behind delivery sits
inside land. The one land lock per project is a guarantee of the run, below.

### Routines

**What it is.** Scheduled or triggered duties: a trigger and an action in
one small file, fired on the controller's clock. Triggers are cron, cooldown
and condition; actions are nudge a seat, file an item, exec anything, and,
once workflows exist, run a workflow.

**The rename** (ruled 2026-09-17, Q3). These were called orders. "Order"
was overloaded inside fleet: it is also the dispatch record on an item,
"orders given", which is the thing a seat reads to know it may start. The
schedule is renamed routine and the dispatch record keeps its word. The cost
is a collision outside fleet, because Claude Code calls its scheduled cloud
agents routines too; the seam is the same either way.

### Event log

**What it is.** One append-only stream, one JSON object per line,
sequenced, with an id, a timestamp, a type, an actor and a typed payload.
The controller writes what it did; seats and workflows write what they ask
and what they finished. Stats is a fold over it. The cockpit is a screen
over it. A third party plugs into fleet once and sees every piece.

**What it carries.** What a person asks about in the morning: a crash, a
nudge, an adoption, a hold, a landing, a return, a cost, a decision owed,
a run opened and closed, a step started and closed. Never the controller's
own housekeeping.

**Why it matters more once workflows are code.** A workflow that runs for
hours needs to survive the box restarting. The stream is already the
durable log a workflow can replay from. See the workflows section.

### Packs

**What it is.** How a user extends their fleet: agents, skills, routines,
workflows, views, doctor checks, a per-provider overlay, assets, and a
manifest that imports other packs. A pack is a folder in a format that
already exists, Gas City's, and `fleet pack add <git-source>` installs one
and pins it in a lockfile.

**Resolution.** By layer: yours on top, its imports in order, the binary's
own defaults underneath. A file at the same path in a higher layer replaces
the lower one. The defaults publish the list of every file they let you
replace.

**The two slots fleet adds.** The format has eight slots. Workflows as code
and views are fleet's additions, the way the `run` key on a formula step
was. They are named as fleet's extensions and not passed off as the
borrowed format.

**The defaults.** The feature layer's own: the record templates, the guards'
wiring, the doctor checks, the default lifecycle, with no opinion about the
work. Ruled 2026-09-13 (core-folds-into-the-binary-after-the-rehearsal), and
landed after the rehearsal: they are the binary's embedded defaults, walked
into the executable at build time and materialized into the machine
directory's `defaults/` — a sibling of `packs/`, never inside it — on every
`fleet create` and every `fleet start`, pinned by content hash. Every verb
reads them by PATH unless an installed pack shadows the path, and nobody
types or imports `core` as a pack name. Every fleet runs on them and they are
not optional; `fleet create` installs no pack at all.

### API

**What it is.** The typed surface everything above the binary calls: the
cockpit, the phone, a user's own tools, and workflows.

**What is ruled** (2026-09-08, the manager sitting; this page adds nothing
to it and repeats it so the map is complete):

- Generated from the same Rust types the cli uses, with utoipa or its
  equivalent deriving the schema from the handlers, so the cli and the API
  cannot disagree.
- The handlers live in one library crate. The one binary gains a `serve`
  verb that runs them over HTTP on loopback. The cockpit is a web app in a
  Tauri shell that calls the same handlers as Tauri commands and embeds the
  server. Docker Desktop shape: close the window, the server keeps running,
  a tray icon holds quit server, stop controller, open cockpit.
- Read paths: status, events, stats, config, plus the item family.
- Writes: the full verb surface, each through the handler the cli already
  has, so the API adds no writer of its own.
- Exposure: loopback by default, LAN by a machine-local key with a paired
  token by QR. A remote bridge, if one exists, is a relay of the same server.
- Mobile access is never behind a paid tier.

**What this page adds** (ruled 2026-09-17, Q4). One library crate of
handlers has several fronts. The cli, `serve` and the Tauri commands are
three. A TypeScript SDK is the fourth, and its types are generated from the
same schema, so "generate types" has one answer: from the API's schema,
never from memory. The SDK's transport is the binary: it spawns `fleet`
with `--json` and needs no server listening, so a headless box with nothing
serving still runs a workflow. The SDK carries the schema version it was
generated from and checks it against the binary on first call, so a workflow
written against last month's API fails at start and not halfway through a
night.

**The guards refuse in the verbs and print at the shell** (ruled 2026-09-17,
Q5). A guard was a hook on the agent's shell. Code calling the API bypasses
hooks unless the verbs refuse the same classes themselves, so the
record-rewrite refusal and the release-ref refusal live in the handlers,
where no front can walk around them, and the hooks stay for what a hook does
best: printing the rewrite to a seat at a shell. Neither replaces the other.

### Workflows

**Status: ruled, 2026-09-17, in the workflows sitting.** Every question the
layers sitting sent here is answered below with its date, and the three that
were open on this page (Q6, Q9, Q10) are marked ruled in the rulings section.
What the sitting could not settle without building is listed at the end of
this section, named as such.

**What it is.** A workflow is real code that calls the fleet API, defines
every step, and runs with `fleet run <workflow>`. The reference workflow
every rule below serves is tiny's takeoff (ruled 2026-09-17): hours long,
spawning seats, raising gates, surviving a restart. Either a person or an
agent may write one (ruled 2026-09-17), so the rules an author must keep are
enforced by the runtime and the SDK, never by convention.

**Why code.** An agent writes a TypeScript function far better than it
writes a DSL. Fan-out, retries and conditionals are a loop, a try and an if
rather than three format features. Claude Code's own workflow tool is
JavaScript orchestrating agents; Dagger, Temporal and Inngest are all
code-first. The direction matches where the substrate is going.

**What core owns, language-blind.** `fleet run <workflow>` resolves a file
named `workflows/<name>.*` through the pack layers, overlay first, the way
skills resolve today. It reads the owning pack's runtime table — or, where
that pack declares none, the one pack it imports that does — and refuses
before anything is written when no table is found, two imports each declare
one, or the declaring pack's doctor is red on the pinned version. It opens
the run directory, pins the inputs,
runs the pack's bundle command into the directory and hashes the bundle onto
the record item and the stream. It executes the pack's run command with the
run id and the stream position in `FLEET_*` environment and the pinned
inputs on stdin, and reads one exit table: done; failed, with the reason on
stdout; waiting, with the wake condition on stdout; could not tell. Core
never parses TypeScript. A fleet on core alone has the verbs, `fleet run`
and routines, and no batch automation of its own; automation comes from a
pack.

**The runtime contract** is one table in a pack manifest:

```toml
[runtime]
name    = "deno"
version = "2.4.5"
bundle  = "deno bundle {entry} --output {bundle}"
run     = "deno run --allow-run={fleet} --allow-read={run_dir} --allow-write={run_dir} --allow-env=FLEET_DIR,FLEET_RUN_ID,FLEET_STREAM,FLEET_STREAM_SEQ,FLEET_RUN_DIR,FLEET_BIN,FLEET_PROJECT {bundle}"
```

Core substitutes the placeholders and execs the two lines. A third pack in
Python or shell fills the same table, and core cannot tell the difference.

**The ts pack** (ruled 2026-09-17). The TypeScript SDK, the runtime table
and the doctor check that reads the pinned version live in a feature-layer
pack of their own, which tiny imports; acme or any other opinion pack
imports it the same way. Its runtime is Deno (ruled 2026-09-17, Q6):
TypeScript runs natively, `deno bundle` writes the one-file program that
pinning asks for, and its permission flags are what enforce the boundary
below. Bun was this page's earlier lean and lost on the boundary: it has no
sandbox, so a spawn or a file write outside the SDK goes unseen, which the
either-author ruling cannot afford.

A workflow is one file — the first ten lines of tiny's takeoff, as landed:

```ts
// packs/tiny/workflows/takeoff.ts — a flight's middle as code. Pre-flight,
// the preboard skill with the person present, hands this run its items and
// its policy; the run spawns a builder per item, waits on each delivery,
// reviews it — every verdict a gate when the policy says so — lands the
// accepted ones, and ends on the report and the board tick as its last two
// steps. Every act is a numbered step over the SDK, so a re-run after a
// Waiting exit replays to where it stopped and spawns nothing twice.
import { type Run, workflow } from "../../ts/assets/sdk/mod.ts";

export async function takeoff(run: Run): Promise<void> {
```

**Pinning** (ruled 2026-09-17, Q9). A run is a function of pinned inputs:
the snapshot of items and trunk, the policy in force byte for byte, a brief
per item, the program. A TypeScript workflow is not the file you copy: its
imports resolve when the process starts, from whatever is installed then.
So at run start the pack's bundle command writes the entry and every import
into one file in the run directory, core hashes that file, and that file is
what runs; a re-run of the bundle is a replay. Native dependencies cannot
bundle, a constraint worth having. Two things the bundle does not pin: the
runtime's version, which the pack manifest pins, and the fleet binary the
SDK talks to, which the schema handshake covers.

**The replay contract** (the shape Q8 left to this sitting; ruled
2026-09-17). Every SDK call is a step, numbered in call order. On start the
SDK reads the run's closed steps off the stream. At step n it returns the
recorded result when step n is present under the same name; otherwise it
appends step started, executes the verb through `fleet <verb> --json`,
appends step closed with the result, and returns it. A name mismatch at n
means the code changed under a live run, and the run fails with "replay
diverged at step n" rather than guessing. Time, randomness and inputs are
steps too (`run.now`, `run.random`, `run.input`), so a re-run sees the same
values. A step result lives in the step-closed event; over a size cap it
goes to a file in the run directory with its hash in the event.

**Waiting is an exit, not a sleep.** A step that cannot close yet, a seat
still building or a gate unanswered, writes its wake condition to stdout
and exits waiting. The controller records the stream position and re-runs
the bundle the next time the stream moves past it; replay carries the re-run
to the same step in milliseconds and it closes or exits waiting again. No
process lives across a restart, so the box restarting at three in the
morning is the ordinary case. The proof of this section (ruled 2026-09-17)
is one board flight flown by takeoff as code, the controller killed once
mid-flight, the run resuming from the stream with no step repeated and no
seat spawned twice.

**Gates.** A gate is `fleet ask` on the run's item as one step and then
waiting; a resolved gate closes the step with the answer letter on re-run.
One object and one event, the same as a seat's question today.

**What a workflow may do outside the verbs** (the remainder of Q5).
Everything pure, and SDK calls. Deno's flags reach the fleet binary, the run
directory and `FLEET_*` environment and nothing else; a workflow that opens
the network, writes elsewhere or spawns anything else is refused by the
runtime before the SDK is involved, and the refusal exits failed with the
permission error as the reason. The handlers refuse too (Q5), so a workflow
driving the verbs meets the same guards a shell does.

**How a run starts.** Three doors onto one verb. A person runs
`fleet run <name>` with inputs. A routine's action carries a `run` kind
beside `exec`, naming a workflow and its inputs; the routine's fired and
completed events wrap the run's started and closed. A parent workflow calls
`run.start(name, inputs)` as a step and waits on the child's closing event.
Autopilot is a routine on a schedule whose guard reads the switch (Q12), and
no other line of core knows the word.

**The run lifecycle** (ruled Q11 and Q13; the mechanics ruled 2026-09-17).
The batch primitive in core is `run`: `fleet run <workflow>` and a run
directory; flight, takeoff and the board are tiny's words. Every run gets
the following, without being asked and without an opt-out:

- A run directory with the pinned inputs and the bundle, hashed onto the
  record item and the stream.
- A record item, `run.started` and `run.closed`, a step event pair per
  step, and `run.waiting` carrying the stream position on each waiting exit.
- One land lock per project, held by the land verb and never by the
  workflow.
- A cap on open runs, refused at `fleet run` before the directory is
  written.
- Resume after a crash. A process that dies with no exit row is could not
  tell, re-run up to a crash cap the way a formula's seat is today, then
  parked with a gate on the run's item.
- Cleanup at close. Every seat whose spawned event names the run is retired,
  whichever way the run ended.

A flight is a run of tiny's takeoff workflow. The word, the composition, the
board and the report are tiny's; the lifecycle is core's. Takeoff splits at
the human line: pre-flight with Alberto stays a skill whose output is the
input list the run pins, and everything after it is the workflow: a spawn
step per item, an until step per delivery, review and land as steps, each
decision that would have gone to him as a gate step, the report and the
board tick as the last two steps.

**The TOML formula's fate** (ruled 2026-09-17, Q10). Retired now, ahead of
the proof: the formula parser, the flight advance's step walking and the
`plan`, `fly` and `autopilot` verbs leave core, and the board's remaining
TOML flights are flown by hand or wait. This reverses the ruling of
2026-09-09, "a step model in core over the formulas slot", on the record.
The item lifecycle's states, listed through landed, are the verbs' and stay.
Chosen over retiring after the proof, with the risk stated and accepted: the
proof flight has no runner to fall back to.

**What waits on a build.** Named as such, per the sitting's charge:

- The step-event payloads and the size cap over which a result moves to a
  file: written with the SDK's first arm.
- Whether a waiting run re-runs on every stream move or on a filtered wake
  condition: the first takeoff run measures the unfiltered form, and a
  filter is added only if the number says so.
- The run's crash cap, and whether it reads the item's max-crashes key:
  decided when the run's record item is written.
- Which verbs gain `--json` first: every verb the SDK calls, in the order
  takeoff calls them; `fleet status` is the only one that has it today.
- The wake condition's grammar on stdout: fixed when the controller's re-run
  loop is written.
- Deno's npm compatibility edges: the SDK depends on nothing from npm until
  one is measured.

### Views

**What it is.** The UI layer inside the cockpit: fleet provides the
primitives, the tokens and the data, users and their agents build the rest.
A game engine for agent orchestration.

**What is ruled** (2026-09-18, the one-tier ruling, superseding the two-tier
line of 2026-09-08). One tier and no iframe. A view is a web component
composed from the cockpit's primitives, chips, buttons, fields and panes, and
its semantic theme tokens; it is fed by a view data client over the API's
typed reads and stream subscriptions, and loaded in-page into a slot it never
reaches outside of. A check verb refuses a component carrying a raw color,
font or size, as a lint over a pack's views and as an audit of computed
styles in the page. Domain components live in the pack that needs them and
never in the kit: tiny's flight track is tiny's, and a third pack's game is
that pack's. The block vocabulary the earlier line proposed, declarative
documents over list, table, board and the rest, is not built, because a
constrained tier stays honest only if it grows with demand and the team that
would grow it does not exist; the two views tiny needs first, a board drawn
as a canvas of flights and tracks, and luggage shown as comments beside code
with actions, fit no vocabulary and fit a component exactly.

**Why no frame.** Packs are installed by the user onto their own machine and
carry the same trust as their skills and doctor scripts, so an iframe would
buy isolation the cockpit does not need and cost the shared look, which is
the failure every free-tier host on the record shows. The slot rule is what
keeps the door open: a component that never reaches outside its slot can be
wrapped in a frame the day a marketplace or a hosted cockpit moves the trust
boundary, without rewriting a view.

**What this page adds** (ruled 2026-09-17, Q14). Views are a ninth pack
slot, `views/`, fleet's own extension of the format. The cockpit at its most
basic is four pieces: the roster, the empty page, the status bar, the
settings screen. Everything else, the departure board included, is a view a
pack ships. What fleet maintains under the one-tier ruling is the kit of
primitives, the tokens, the data client, the loader and the check.

### Memory

**Status: deliberately not designed now.** Markdown files are fine for the
present, ruled in this sitting. The paragraph below exists so the piece is
named and not forgotten.

Three things with different lifecycles hide under the word. Doctrine and
rules: versioned, reviewed, owned, in packs, where git is the right
substrate. Episodic memory: diaries, lessons, ledgers, the part that goes
stale and contradicts itself; the problem there is not the format but the
absence of a source, a scope, an expiry and a check that can retire an
entry. Prompt fragments injected at steps: the brief and note templates,
which already exist as shadowable pack assets. The litmus for anything later
called Memory: does an entry have an owner, a date, a scope and a way to be
proven wrong.

---

## Opinion layer

Everything with an opinion about how software should be built. Shipped as
packs, on the feature layer's primitives, removable.

### tiny

Our pack, everything the way we like it. Imports the runtime pack, layers
over the binary's own defaults, and changes no verb.
What it carries, once the layers above land:

- **Flights**, as workflows. `fleet run preboard` writes a flight's list
  (today's `fleet plan`). `fleet run takeoff` opens and flies it (today's
  `fleet fly`). The composition, the cap, the board and the report are in
  the workflow.
- **The departure board**, as a view.
- **Autopilot**, as a machine setting a workflow reads (ruled 2026-09-17,
  Q12: read and written through the API, by `fleet settings` or the
  cockpit's screen). On, the routine that runs takeoff opens the oldest plan
  while the open runs are under the cap; off, nothing opens. The switch is a
  line in the machine directory, and not policy.
- **The rituals**: wake, handoff, rest, clock-out, morning, corrections
  review, praise, the report and runbook house styles.
- **The role documents, the builder manual, the values, the every-turn
  rules**, as the overlay's rules file on top of the default one.
- **Its own guards**, on the same overlay slot as the defaults' own.
- **Bun**, pinned in the manifest, checked by doctor.

### acme

Someone else's setup, completely different from ours. Nothing in the
substrate or feature layers dictates their process. What the feature layer
does dictate is the safety line above: their record is append-only, their
seats do not start unasked, their runs pin their inputs. acme is the test
that every process opinion really is in tiny and not leaking into the
binary's own defaults.

### gascity

A Gas City-shaped setup is probably reproducible on top of code workflows:
graph steps, drains, scopes, fan-out discovered at run time. It would run
inside fleet's safety rules and not around them. It is named here as the
second test of the seams, not as a goal.

---

## What moves, and where

| Today | Where it lives on this map |
| --- | --- |
| `fleet plan`, `fleet fly` | tiny's `preboard` and `takeoff` workflows over core's `fleet run` |
| `fleet autopilot on\|off` | a machine setting, read by tiny's takeoff routine |
| the flight directory, pinning, the land lock, retire-all | the run lifecycle in core |
| formulas, the TOML step runner | workflows; the formula's fate is an open question |
| orders | routines |
| the manager | the cockpit, already ruled |
| the departure board, the report page | tiny views |
| the store trait in core | the store contract and an executable adapter |
| the agent adapter trait in the controller | unchanged |

---

## The MVP

**Status: ruled, 2026-09-17, in the MVP sitting.** The smallest fleet that
flies a night on this map, and the path from what is built to it. Each ruling
below carries its date; the path's rows are beads on tiny's departure board.

**What is real, what is a stub** (ruled 2026-09-17). The substrate stays what
is built: the bd adapter behind the store trait and the Claude adapter behind
the agent trait. Real in the feature layer: seats, items, the event log,
packs, routines, `--json` on every verb the SDK calls, `fleet run` with its
lifecycle, and the ts pack with the SDK. Stubbed: views stay markdown files,
memory stays undesigned, and the executable store contract with its
conformance command waits past the MVP.

**What tiny ships on day one** (ruled 2026-09-17). Takeoff as code, phases
two to four of today's takeoff skill; pre-flight stays a skill whose output is
the input list the run pins; the departure board stays a file read by hand;
the pack's existing skills stay. The cockpit, model-judged reels, the
corrections review and autopilot as a routine come after.

**What the gate proves** (ruled 2026-09-17). Exactly the workflows sitting's
success criterion: one real board flight flown by takeoff as code through
`fleet run` end to end, the controller killed once between a spawn and its
delivery, the run resuming from the stream with no step repeated and no seat
spawned twice, every landing on main. Read from the stream and the run
directory, never from a report.

**The path** (ruled 2026-09-17). Fifteen rows in four flights, each one bead
wide where it extends the one before, so a child can always fetch its
prerequisite from the trunk before it starts. The removals sit after the
extraction so nothing `run` reuses is deleted before it moves.

1. *Core learns to run.* A JSON envelope for verb output, applied first to
   the two stream readers the SDK replays from, landing alone as the shared
   prerequisite; then `--json` on the item verbs and on the seat verbs,
   beside each other; the `[runtime]` table in the pack manifest with its
   doctor check; `fleet run`'s front half, extracted from what `fly` pins
   today; its back half with the exit table; and the controller's re-run of
   a waiting run, the crash cap, the park and cleanup at close.
2. *The ts pack.* The pack skeleton with Deno pinned and tiny importing it;
   the SDK's core with the replay contract; the SDK's seven verbs.
3. *The removals and the rename.* The formula runner leaves core; `plan`,
   `fly` and `autopilot` leave core; the schedule verb renamed routine with
   its `run` action kind, the naming doc and the PRDs following.
4. *tiny and the gate.* Takeoff as code; then the gate above.

**After the MVP**, in the order the board keeps them: the executable store
contract, the cockpit and the log PRDs, the lifecycle bugs held for the
workflows sitting re-read against the line between what left and what
stayed, core folding into the binary, the switch, and the production-write
surfaces.

## The rulings

Every question this page left open on 2026-09-17, ruled by Alberto the
same day in a sitting with an architect, each recorded on the design
session's record under a key of the form `fleet-layers-q<n>-…`. The
recommendation is kept beside each ruling so a reader can see where the
ruling took it and where it did not. Three questions went to the workflows
sitting and were ruled there the same evening, under keys of the form
`workflows-…` on that sitting's record.

**Q1. The store adapter's form. Ruled: A.** An executable with a fixed verb
set, JSON in and out, and a `fleet store check` conformance command. The
agent seam already has this shape, Beads already shells out per call, and
every seam stays a file or a command rather than something compiled against
fleet.

**Q2. Who defines the store's semantics. Ruled: A.** The contract defines
ready, ordering, metadata writes and gates; every adapter conforms and
translates its store's quirks inside. The conformance suite is only possible
this way.

**Q3. Orders become routines. Ruled: A.** The schedule is renamed routine;
the dispatch record keeps "orders", the load-bearing word in every seat's
doctrine.

**Q4. The SDK's transport. Ruled: A.** The SDK spawns the binary with
`--json` and needs no server. The page had recommended A first and C later;
the ruling is A.

**Q5. Where the guards refuse. Ruled: C.** Both: the handlers refuse, so no
front can walk around a guard, and the hooks print the rewrite at a shell.

**Q6. The workflow runtime. Ruled: Deno, 2026-09-17, in the workflows
sitting.** Its permission flags enforce the verb boundary, which the page's
Bun lean could not; it lives in the ts pack, which tiny imports.

**Q7. Where the runtime dependency is declared. Ruled: A.** In the pack
manifest, pinned, checked by the pack's doctor slot. Core stays one binary
with one dependency.

**Q8. Durability of a long-running workflow. Ruled: A.** Replay from the
stream: the workflow is deterministic, effects are events, a re-run skips
every step the stream shows done. Decided here although the page had marked
it for the workflows sitting; the sitting wrote the contract's shape on
2026-09-17 (the replay contract, in the workflows section).

**Q9. Pinning a code workflow. Ruled: A, 2026-09-17, in the workflows
sitting.** The pack's bundle command writes the entry and every import into
the run directory at start; core hashes that file and runs it.

**Q10. The TOML formula's fate. Ruled: C, 2026-09-17, in the workflows
sitting.** Retired now, before the proof, against the recommendation of
after; the risk accepted on the record. Reverses the 2026-09-09 ruling.

**Q11. `fleet run` inherits every guarantee `fly` has. Ruled: A.** The run
directory, pinning, the lock, the cap, resume and retire-all, without
opt-out. These are the safety opinions.

**Q12. Autopilot's home. Ruled: A.** A machine setting, read and written
through the API, by `fleet settings` or the cockpit's screen. It is machine
state, not policy, and not process environment.

**Q13. The batch primitive's name in core. Ruled: A.** `run`: `fleet run
<workflow>` and a run directory. Flight, takeoff and the board are tiny's
words.

**Q14. Views as a pack slot. Ruled: A.** A ninth slot, `views/`, fleet's
extension of the format, with a check verb and a shadow list of its own.

**Q15. Seat presets in `fleet.toml`. Ruled: A.** A preset is a spawn shape
and a named seat is a preset plus an identity and a worktree.

**Q16. Recorded and still open elsewhere.** The premium line, with mobile
never paid. Whether the roster is a tab or the empty page's content, and
what the attach opens. Both belong to the cockpit PRD.

---

## Next steps

1. **Clarifications on this page.** Done 2026-09-17: the rulings above,
   and every ruled section rewritten to the definite.
2. **The workflows sitting.** Done 2026-09-17: Q6, Q9 and Q10 ruled, the
   workflows section rewritten in the definite, and what waits on a build
   named at the end of that section.
3. **The MVP.** Done 2026-09-17: the MVP section above rules what is real,
   what tiny ships on day one and what the gate proves.
4. **The path from here to the MVP.** Done 2026-09-17: fifteen rows in four
   flights, in the MVP section above and on tiny's departure board, each one
   landing in production the moment it is extracted. The renames (routine,
   run, item) are a row on the path, not a cleanup after it.
