# ADR: flights

Accepted 2026-09-09; the definition it decided is
`../prds/fleet-flights-prd.md`.

**Superseded in part, 2026-09-17 (workflows-formula-fate; fleet-layers.md Q10
and § What moves).** S1's engine and S5's three verbs —
`fleet plan`, `fleet fly` and `fleet autopilot` — left core: composition is
tiny's preboard and takeoff workflows over `fleet run`, and the run lifecycle
keeps the pinning, the lock, the cap, resume and retire-all. Every section
below stands as the record of what was decided.

## Context

Drafted 2026-09-08, from a sitting that began by defining the problem and
refused to reach for a solution until it was written down. The reference fleet —
the factory that has flown every flight of a live product since August 2026 — is
the counter-example the problem statement is measured from. Gas City's
`graph.v2` formulas are one existing answer to part of it and are read as such,
not adopted.

**Recorded directions.**

- **Flights are flown by ephemeral seats only; the named seats are the
  company.** Alberto's direction, 2026-09-08, recorded verbatim on the work
  item that carries this sitting. No named seat on a manifest; no message to a
  named seat to keep a flight moving; no question answered by a person inside
  a flight. Architects keep the backlog of ready flights and the specs; named
  builders review and explain on request.
- **The problem before the solution.** The same direction: define the problem
  fully, then judge Gas City's `graph.v2` and every other shape against it.
- **No hard rules that are hard to undo.** The founder's direction at the
  solutions sitting, 2026-09-09, verbatim in § The solution: every decision
  carries its mark — constant, key or pack — and the recommendation preferred
  the key or the pack unless the constant is what makes a flight a function of
  its inputs.
- **Fleet's own engine; Gas City's `graph.v2` read, not adopted.** Ruled
  2026-09-09 after the five solution questions, at the founder's request that
  the ruling live outside the work graph: § Own engine, or Gas City's.
- **No `fleet flight` command.** Ruled 2026-09-09, verbatim in S5a.
- **The workflow-engine ruling was not a keystone.** The founder's direction,
  2026-09-09, verbatim in S6; the question it had answered was put on its own
  and ruled: an item's life is a formula run by fleet's own step runner.

## The seven questions, decided

Gas City's `graph.v2` is one answer to questions 2 and 3 — routing a step by
role, gating on a checked artifact, retrying under a cap. Whether it is the
right shape is a solution question, and it waits on the seven.

### Q1 decisions — ruled 2026-09-08

**Q1a — where the pins live: a flight directory in the machine directory.**
`flights/<id>/` holds the inputs file, the policy snapshot and each item's
rendered brief; the directory's hash and a summary go on the flight's record
item, and one `flight.opened` event carries the hash. Files a fold can read and
a diff can compare, and the brief a seat actually read is kept — so the
question of storing the brief or only its hash is answered by the directory.
Declined: every pin as metadata and notes on the record item; every pin as an
event on the stream.

**Q1b — policy mid-flight: a snapshot at takeoff.** A flight runs under the
policy it recorded, and an edit to `fleet.toml` reaches the next flight. The
running fleet keeps re-reading for everything else. Declined: live policy with
a change event per edit.

**Q1c — the trunk: a project policy key, not a fleet constant.** Ruled in
Alberto's words: "wouldnt different teams have different guidelines for
branching/merging?" The strategy is declared per project and pinned on the
flight's record beside the commits. Its first two values are the two shapes
above — advance per landing with the base recorded per dispatch, and one
takeoff commit with one batch landing — and a pull-request shape, where a
landing opens a request a person or a rule merges, is a third a team may need.
`fly` reads the key and refuses a value it does not implement, naming it.
Declined: one trunk rule for every fleet.

**Q1e — the provider's user-level configuration: isolated.** The adapter starts
each spawned seat with its own configuration directory holding only what the
pack's overlay puts there; nothing from the person's home directory — settings,
memory, instructions, servers — reaches a flight. The flight's inputs are the
pack and the item and nothing else. Declined: inherited with a hash on the
record.

### Q2 decisions — ruled 2026-09-09

**Q2a — the dispatcher: code.** `fly` pins the inputs, dispatches, reads
verdicts off the items and advances the flight's record; no seat holds the
role. Whether it is a process or the controller's tick is Q4 and Q5's.
Declined: a spawned operator seat.

**Q2b — the reviewer: a policy, not a constant.** Ruled in Alberto's words:
"some users might decide to skip review altogether, some items might not
require review at all." Review is a fleet policy with a per-item override,
and the value in force is pinned on the flight's record. Its values: `none` —
an accepted delivery lands on the builder's word; `spawned` — a reviewer seat
per delivery, born from the pack's review skill, the item, the delivery note
and the diff, blind to the builder, its verdict on the item, retired; and a
team may name `self`, the builder's own read against the item before it
delivers. An item's override is written on the item before takeoff, so it is
an input. Declined: a named builder seat reviewing inside a flight.

**Q2c — the lander: the flight invokes `land`.** On an ACCEPTED verdict, or on
delivery when review is `none`, the flight's code runs `land` in a worktree
the flight owns, under the project's trunk strategy (Q1c). The reviewer seat is
already retired, and the landing depends on nothing about it. Declined: the
reviewer seat landing from its own worktree.

**Q2d — the answerer: nobody, and human gates are first-class.** A blocking
question parks the item and the flight moves on. Ruled with one requirement in
Alberto's words: "whatever solution we build needs to account for human
gates." A step may name a person as its gate; the flight holds at that step
and resumes when the gate is resolved from outside the flight — through the
work graph's gate, or the cockpit's decisions-as-questions — and never by a
person inside it. Q3 defines the park and the gate. Declined: a spawned
answerer that answers from the record.

### Q3 decisions — ruled 2026-09-09

**Q3a — a flight never waits.** A park closes the item's place on the flight;
the flight closes when every item on its list is landed or parked, and a
closed flight never reopens; a resolved gate returns the item to the pool and
the next flight takes it with a fresh seat. Every flight is bounded, and a
gate resolved three days later needs nothing to have stayed open. Declined:
the flight staying open to resume the item.

**Q3b — the question is the work graph's own gate.** A human gate on the item,
its text the question with lettered options, listed by the graph's gate
command and answered by resolving it. The cockpit's decisions list is that
command, and a formula's human gate is the same object, so one mechanism
serves a builder's question, a step that names a person, and a parked item's
"what next." Declined: a note in a fixed grammar the cockpit parses.

**Q3c — one rerun, then park with both readings.** A gate red on something
the delivery did not touch gets one rerun, which distinguishes a flake from a
real red at the cost of one suite run; a second red parks the item with both
readings as its question. Declined: parking at once; a policy rerun count.

**Q3d — the parked seat's branch and last commit are kept.** The seat commits
what it has before retiring; the park records the branch and the commit; the
resume cuts its worktree from there, so nothing built before the question is
lost and the resume is attributable to the commit it started from. Declined:
a resume from the trunk.

### Q4 decisions — ruled 2026-09-09

**Q4a — the verbs write events as well as notes.** Each of the four verbs
writes its note on the item and one typed event on the stream carrying the
ids and commits: `item.dispatched`, `item.delivered`, `item.reviewed`,
`item.landed`, `item.returned`, `item.parked`; `fly` writes `flight.opened`
and `flight.closed`; `land` writes `gate.read` per reading. The note is for a
person and the event is for the fold, and each fact still has one writer.
Declined: notes only, with every reader parsing the work graph.

**Q4b — the controller writes a seat's cost at retire.** `session.retired`
carries context tokens, turns and wall time, keyed to the seat and its item,
read through the adapter's transcript verb — a measurement by the layer that
can see it, never a self-report. This answers, for flights, the cost question
the log-and-stats PRD carries. Declined: the seat reporting its own usage; cost
deferred.

**Q4c — a summary file at close, derived and marked so.** `fly` writes one
file into the flight directory: the landed set with commits, the parks, the
returns, the gate readings, the cost and the timings, plus the stream sequence
range it folded, so it is re-derivable and never a second source of truth, and
two flights compare with one diff. Declined: fold on demand only.

**Q4d — no report page.** The record is the report: the cockpit's Flights view
renders the fold, a shell verb prints it, and the decisions owed are the open
gates. Nothing is written by hand and nothing can drift from the record.
Declined: a rendered page in the directory as well.

### Q5 decisions — ruled 2026-09-09

**Q5a — one landing lane per project.** Landings queue and run one at a time
in arrival order, in a worktree the fleet owns with its own build cache. Two
suites never run together, a finish is never refused by another landing, and
the trunk advances in a recorded sequence. Declined: a landing worktree per
flight, landing in parallel.

**Q5b — any number of flights, overlap recorded.** Flights share the load belt
and the landing lane, and every flight's record names the flights that
overlapped it as a source of variance, so the fleet keeps flying and an
experiment compares like with like by that column. Declined: one flight per
machine at a time; one flight per project at a time.

**Q5c — a refused dispatch is held and retried on the tick, in order.** The
item waits its turn; the flight does not park it; the hold and its load
reading are recorded. Load shapes timing, never the outcome. Declined: parking
a refused dispatch for a person.

### Q6 decisions — ruled 2026-09-09

**Q6a — the referee is escapes per landed item.** Defects found after landing,
per landed item, is the primary measure of "best code"; cost and returns are
the price paid for it. Best code is code that does not come back, and this is
the one measure a seat cannot game inside the flight. Declined: returns per
delivery, cost per landed item, or a composite as the referee.

**Q6b — the record does not know what an experiment is.** `fly` writes the
per-flight summary file (Q4c) and a person groups and compares them by hand;
the flight carries no experiment or arm field. Declined: naming the experiment
and its arm on the flight so the fold groups by them.

**Q6c — an escape is bounded by a policy window, fourteen days by default.**
An item **of type bug** that traces to the landed item or its commit within the
window, or the commit reverted. The window is a policy key, so a team with a
slower cadence widens it. The trace runs through the work graph and never
through a diff over a file: five follow-up commits that tidy comments around a
feature are not escapes, and neither are the task and chore items a review
files off a landing — the luggage of a return-on-defects-only review is the
review doing its job. A follow-up commit with no item behind it is invisible
to the fold by construction; a real defect fixed with no item filed is the one
gap, closed by a project holding the rule that every commit references its
item, and a project that does not hold it under-counts its escapes. Declined:
no window; counting every item that traces back regardless of type.

**Q6d — the log-and-stats PRD owns the mechanism.** This page lists the flight
measures and their derivations; how and where they are computed — the tick, a
verb, or the cockpit — is that sitting's own question. Declined: a
flight-specific mechanism ruled here.

### Q7 decisions — ruled 2026-09-09

**Q7a — the supervised lane is a human gate after delivery.** The builder
delivers, the item parks at a gate carrying its review artefacts, the person
resolves it from the cockpit or the graph, and the next flight lands it. The
on-device review stays the taste gate, and it is the same gate object as every
other question (Q3b). Declined: supervised items hand-flown outside flights.

**Q7b — the pre-takeoff pass is a pack's opinion, never core's.** Ruled in
Alberto's words: "all of this should be configurable and not something we
should bake into fleet by default. some users might want it other not." Core's
`fly` refuses only an item the work graph does not call ready. A pack adds a
pass — worth, readiness, a cross-read by an architect who is not the author —
as a readiness condition through policy, and the fleet's own pack carries ours.
Declined: the pass baked into core; the mechanical check as core's only gate
being anything more than what readiness already is.

**Q7c — the morning is a pack's ritual.** Core offers gates and their listing
(Q3b). Who reads them first — an architect resolving from the record and
leaving the person the rest, or the person alone — is an order and a skill in
a pack. Declined: a triage baked into core.

**What the two rulings say together.** The company's rules are opinions about
how a person wants to work, and every one of them lives in a pack. Core ships
the seams they hang on: readiness the graph computes, gates the graph holds,
orders the controller fires, and a flight that is a function of its inputs.

## The solution, decided

Ruled 2026-09-09 in five questions, each read against the seven above, and
opened under one direction recorded on the way, in the founder's words: "i
dont want to bake any hard rules into fleet that would be hard for us to undo
later when we find out it makes it impossible for an end user to set up fleet
in the way they want." So every decision below, and every requirement after
them, carries one of three marks: **constant** — core's, and hard to undo, kept
only where it is what makes a flight a function of its inputs; **key** — a
policy value with a default, changed in `fleet.toml` or `project.toml`;
**pack** — an opinion a team installs or leaves out.

### Own engine, or Gas City's

Asked last, because the sitting had skipped it: whether the engine that flies
a flight is fleet's own or Gas City's `graph.v2` formulas. The seven questions
were written partly to judge that, and the judgment is this table.

| Question | What `graph.v2` gives | What fleet's question still needs |
| --- | --- | --- |
| Q1 inputs | a formula hash and a variables snapshot on the root | the agent version, the model per role, the pack lock, the trunk commit, the policy: the orchestrator's environment, unpinned |
| Q2 roles | steps route to a role at dispatch; control steps run outside agents | the roles are its own agents; a reviewer blind to the builder is not a concept |
| Q3 person | a human gate on a step, as a gate bead that blocks it | the gate has no runtime consumer in the current release — nothing acts on it, nothing resumes until a hand closes it — where Q3a closes the flight and resumes the item on the next one |
| Q4 record | steps are items; its own event stream | cost is not on any event the docs or one measurement show — 676 rows of one idle city on gc 1.4.1, mostly its own health orders — and the transcript was unreadable for a session in the default directory |
| Q5 machine | fan-out caps | no landing lane, no load contract |
| Q6 measurement | nothing | the referee |
| Q7 company | nothing | opinions and core are one thing there |

**Ruled: fleet's own engine — the tick over fleet's own record — and
`graph.v2` read, not adopted.** It answers Q3's gate shape and part of Q2, and
none of the other five; adopting it would bring its agents, a gate with no
runtime consumer, and its stream. The founder asked for this ruling to live
outside the work graph, which is why it is a section here. Declined: flights
written as `graph.v2` formulas and run by a `graph.v2` runner in core; a
formula-shaped flight as a later pack; Gas City itself as the flight runner,
which the controller sitting closed. Read beside S6, ruled the same night:
an item's life *is* a formula, in the v1 format the slot already holds, run
by fleet's own step runner — what is declined here is `graph.v2`'s compiler
and its constructs as core's, not the idea of steps.

### S1 — The engine

**Removed 2026-09-17 (workflows-formula-fate).** The engine — the tick's advance
and `fly`'s foreground loop — left core with the three verbs; the run lifecycle
is what re-runs a run (controller PRD R35–R37). Kept as the record.

| Engine | How a flight advances | What survives a crash | What it costs |
| --- | --- | --- | --- |
| **the tick over the record** | the controller's tick calls core's advance for every open flight | everything: the state is a fold over the stream and the notes | the tick gains one duty; core gains an event seam the cli wires |
| a long-lived `fly` process | `fly` loops until close | nothing without a resume plan; two `fly` on one flight race | a process per flight: the reference's operator with the person removed and the fuel tank kept |
| a poured molecule per flight | `fly` pours a formula, steps per item; the tick runs steps | the molecule, every transition a store write | a step runner in core |

**S1a — the tick over the record.** Constant. `fly` pins and opens, then
returns; each tick derives every open flight's state and takes one act per
item. Declined: the process; the molecule.

**S1b — `fleet fly` with no controller running.** `fly` runs the same advance
loop in the foreground, ticking on the policy interval until the flight
closes, progress on stderr — identical code to the tick, so the two never
disagree, and the embedded fleet flies on day one without a service. Declined:
refusing and naming `fleet start`; opening the record and returning.

**S1c — the flight's record item.** One work item of type task carrying the
label `flight`, titled by the flight id, its metadata carrying the directory
hash and the summary, closed at `flight.closed`; no store configuration is
touched. Declined: a custom item type, which would write a store's config;
no item, which the Flights view could not read.

### S2 — An item's life inside a flight

**S2a — a return dispatches a fresh seat from the delivered commit.**
Constant. The new seat's worktree is cut from the delivery commit and its
brief carries the item, the delivery note and the numbered findings; the
resume commit is on the record. Declined: a fresh seat from the trunk; reviving
the retired seat, which makes the builder persistent inside the flight.

**S2b — review policy values.** Key, `[core.flight] review`, default
`spawned`, per-item override. `none` and `spawned` are built in the first
slice; `self` — the builder's own read against the item before it delivers,
the verdict written by the same seat — is specified here and lands after the
first gate. Declined: all three in the slice; `spawned` only.

**S2c — the per-item override surface.** Constant for the seam, the values
keys. An item's overrides live in one metadata object on the item, `flight.*`,
written before takeoff, copied into the flight's inputs file at takeoff, and
never inherited from a parent, so an epic cannot change a child's review
silently. Declined: labels, which inherit; the flight's inputs file, which
would let the same item fly differently without the item changing.

**S2d — the flight retires every seat.** Constant. The tick sees
`item.delivered` or `item.reviewed`, calls the controller's retire, which
verifies from outside that nothing of the seat's holds memory or disk; the
controller then reads the transcript and writes `session.retired` with the
cost. Declined: the seat retiring itself as the last act of `deliver` or
`review`.

### S3 — The landing lane and the trunk

**S3a — a behind delivery is rebased by the lane.** Inside `advance`: the
lane rebases the delivery commit onto the trunk's tip, runs the suite on the
rebased commit and lands it, recording both the rebased commit and the base; a
conflict parks the item with the conflict as the question. Declined: refuse
and park at once, the reference's rule, which parks every second delivery of a
flight; a merge commit, which drops the squash.

**S3b — one lock per project, taken by `land` itself.** Constant. Every
landing, the tick's or a person's, queues on the same lock and prints that it
is waiting. Declined: the tick alone serializing, with a hand-run `land`
refusing during a flight.

**S3c — `advance` built first.** Key. `batch` and `pull-request` are
specified below with their record shape and each lands as its own slice; until
then `fly` refuses the value by name. Declined: building two or all three now.

**S3d — the lane's worktree under the machine directory.** Key with the
default `lanes/<project>/`: nobody else edits it, a person's checkout is never
touched by a flight, and its build cache is shared with no seat's and no
scratch tree's. Declined: the project's primary checkout; a tree beside the
seats'.

### S4 — Gates and parks

**S4a — the gate is the store's own.** Constant. On the first store, an
ad-hoc gate of type human blocking the item, measured on its current release
in the sitting: the item leaves the ready set until the gate is resolved, and
the gate list shows every open one. Declined: the store's human label, whose
answer closes the item and not a gate; a fleet-owned question item, which
re-implements the listing the store has.

**S4b — `fleet answer <item> <letter> [--text]`.** Constant. Writes the
answer on the item, resolves the gate, writes `gate.resolved`; the cockpit
calls the same verb. Declined: resolving the gate alone with the letter a note
a person may forget; the letter as the gate's close reason, where no brief
renderer looks.

**S4c — `fleet ask`.** The seat's verb, from inside its worktree: commits what
it has, raises the gate carrying the question and its lettered options,
records the branch and the commit on the item, writes `item.parked`. Pairs
with `answer`. Declined: `park`, which names the state and not the act; `gate
raise`, two words on the store's noun.

**S4d — a supervised item's gate sits after review by default.** Key with a
per-item override, the value naming the step: `flight.gate = "review"` parks
the item after an accepted review, so the person's taste read arrives on a
delivery a review already accepted and a return never reaches them; a team
that wants the person first sets `review = "none"` on the item and the gate
sits after delivery. Declined: the taste gate instead of review, or before it.

### S5 — Plans and autopilot

**Removed 2026-09-17 (workflows-formula-fate).** `plan`, `fly` and `autopilot`
left core; the backlog, the switch file and `status`'s listing of them went with
them. What plans and opens work is tiny's preboard and takeoff workflows over
`fleet run`. Kept as the record.

**S5a — `fleet plan <items...>`, then `fleet fly`.** Constant for the split,
the names the naming page's. `plan` writes a planned flight; `fly` takes off
with the oldest plan or a named one; `status` lists the plans waiting. **There
is no `fleet flight` command**, ruled in the founder's words: "fleet flight
sounds dumb to humans and should never be a command" — flight is the noun a
person reads in prose and in `status`, never a command word. Declined:
`queue`; `fly` planning and opening in one act, which leaves autopilot nothing
to open.

**S5b — one flight open at a time, out of the box.** Key, `[core.flight]
max_open`, default 1; raising it records the overlap on every flight's record
(Q5b). Declined: unlimited.

**S5c — an empty backlog opens nothing.** Constant that core never picks:
`fleet plan --ready N` exists for a hand or an order, and the ready query runs
at plan time, never at takeoff; `status` prints that the backlog is empty.
Declined: core planning from ready items under a key; no `--ready` flag.

**S5d — `--seats M` stays, `--items N` goes.** Key, `[core.flight]
max_seats`, with `--seats` as the per-flight override; the plan already says
which items fly. Declined: `--items N` flying the first N of a plan; both flags
dropped.

### S6 — The item's life is a formula, and the steps are the flexibility

**Reversed 2026-09-17** by the workflows sitting's ruling
`workflows-formula-fate` (fleet-layers.md Q10), ahead of the proof and with the
risk accepted on the record: S6a, S6b and S6c below are reversed. The formula
parser, the default formula, `flight.formula`, the instantiation at takeoff and
the flight advance's step walking leave core; a workflow is code under the run
lifecycle, and the `step.started` and `step.closed` pair is the run
lifecycle's. The item lifecycle's states are the verbs' and stay. The
paragraphs below are kept as the record of the ruling they reverse.

Put last, and put twice. The first time this sitting reached the question it
answered it by inheritance: the packs sitting's "no workflow engine" had been
carried through the problem and the solution as a keystone, and it decided
Q2's "no third kind", S1a's third option, S2's four verbs and the non-goal
without ever being asked. The founder named that, in his words: "That workflow
engine decision wasnt meant to be set in stone forever but we keep holding it
as a keystone in everything we have discussed around this which feels like has
biased or warped our decision process". It also contradicted a ruling already
made: formulas are a core slot the cockpit edits. So the question was put on
its own, with an option set from the seven questions and that ruling only,
priced in a table the stress-test page carries.

| Option | What the user writes | What core builds | The record | Hard to undo |
| --- | --- | --- | --- | --- |
| **a step model in core over the formulas slot** | one formula file per item life | a runner: read the formula, walk steps by needs, run a script or spawn a seat per step, one event per step | the item stays the unit; steps are lines on it | the format — and it is bd's, Gas City's v1, already shared |
| the same, every step a bead | the same file | the runner plus pouring molecules | ten beads per item for a ten-step life | the same, plus every graph filling with step beads |
| hooks between four fixed verbs | a script per point, six points | six call sites and a script contract | the item | the six points |
| a fixed life, the four verbs | nothing | nothing | the item | the four verbs |

**S6a (reversed 2026-09-17) — a step model in core over the formulas slot.**
Constant for the
runner; the default formula a pack's, shadowable. The format is the one the
slot already holds — bd's, which is Gas City's v1: `[[steps]]` with `id`,
`title`, `description`, `needs`, `[vars]`, `[steps.gate]` — plus fleet's one
addition, `run`, which names what executes the step: `seat` with an optional
role (the default: a spawned seat whose brief is the step's text plus the
item), or `script = "<path>"` with the exit contract 0 pass, 1 fail with the
reason on stdout, 3 could not tell; a script step's path is a pack or project
file pinned by the lock. Core ships the default formula in its own `formulas/`:
`dispatch` → `build` → `deliver` → `review` → `land`, which renders exactly
S2's table, so a fleet that never touches it flies today's flight. A project
shadows it in its own `formulas/`; an item names a formula in `flight.formula`
and otherwise gets the default. The formula is instantiated per item at
takeoff — its text with the item's variables — and stored in the flight
directory beside the brief, so it is an input (Q1). Each step writes one
`step.started` and one `step.closed` event carrying the item, the step id, the
seat or the script, and the outcome, and one note line on the item; steps are
never beads. Declined: every step a bead (bd's molecules) — the referee is
per landed item and ten beads per item makes every list and the cockpit ten
times noisier for that one number; hooks between four fixed verbs — cheaper,
and it leaves the formulas slot a promise with nothing behind it; the fixed
life as landed.

**S6b (reversed 2026-09-17) — the first slice reaches linear steps.** `needs`,
a seat or a script
per step, `[steps.check]` as the format has it, a human gate on a step (the
same gate object as S4). Fan-out discovered at run time, a retry policy per
step, and non-human gates are P1, each its own slice, and a formula that uses
one of them is refused at takeoff by name. Declined: fan-out in the slice,
which reopens Q1's pinned list for the expanded set; everything `graph.v2`
has, which makes the slice the largest thing on the board.

**S6c (reversed 2026-09-17) — a model and a provider per step, ruled
2026-09-09 after the stress
test's scenario 8.** The question was whether a team wanting a different model
on one step should get it by naming roles as model aliases — a reviewer per
model, each a policy row — or by an optional key on the step. Ruled: `model`
and `provider` as optional keys on a seat step's `run`, the role's row the
default. What it costs: nothing to pin, because the instantiated formula the
flight directory already holds carries it — which corrects the seat's earlier
reading that a per-step model was one more input to pin, false once S6 stored
the formula per item; nothing on the record, which already carries the seat's
model on the spawn and step events; one read at spawn. The one obligation:
`status` and the flight's record show the effective model per step, never
the role's, or a reader is misled. Declined: the role's model only, with roles
as the workaround, because a role selects a skill and a brief as well as a
model, so roles would multiply as skills times models and stop meaning jobs;
a model per step without a provider per step.

**S2e — defaults by rule, ruled 2026-09-09 after the stress test's scenario
9.** The founder's use case, verbatim: "i would love if product + that
actually change behavior beads would automatically get a more rigorous review
process vs a harness + comment change bead getting almost nothing at all."
Two mechanisms answer it and only one is new. What an item *is* — its type
and labels — is known at takeoff, and the ruling puts an ordered
`[[core.flight.rules]]` list in policy: match on the item's own type and
labels, never a parent's; set the `flight.*` values the item did not write;
first match wins; the item's own object beats any rule; computed into the
inputs file at takeoff; `status` prints the table. What the change *did* — a
comment-only diff, a behaviour change — is known after delivery, and that
stays where the packs page put it: a pack's per-tier review at P1, the review
step reading the diff stat, with core's review one reader's read at every
size. Cost of the map: one key, one matcher, one place in `fly` that already
builds the per-item object. Declined: an exec order on the tick stamping
`flight.*` onto items by label — nothing in core, but stamps go stale when a
label changes, an item filed after the order's last run gets the default, and
the rule lives in a script where the map is a table; deferring until a project
needs two formulas; pulling the size tiers into core's first slice, which is
an opinion about review depth Q7 put in packs.

**Why v1 and not v2 — a consequence, and the right one.** The ruling names
"the format the slot already holds"; the slot holds the store's own format,
and that is Gas City's v1: `[[steps]]` with `id`, `title`, `description`,
`needs`, `[vars]`, `[steps.gate]`. Beyond the accident of what the slot
holds: v2 is not a file format but a compiler — `check`, `retry`, `drain`,
`on_complete`, scopes and teardown exist because Gas City's orchestrator
compiles them into control items it runs outside any agent, which is the
compiler § Own engine declined; v1 is data a runner reads. v1 is the shared
layer the store, Gas City, every pack and the cockpit's formula editing all
hold, and fleet adds one field to it. S6b's linear first slice is v1's shape
plus that field, and every v2 construct is a later slice in fleet's own
runner, refused at takeoff by name until built. The honest cost is the two
setup scenarios where Gas City holds and fleet bends — fan-out discovered at
run time, and a retry policy per step — a known deferral and not a gap. One
thing this page does not claim: that v1's `[steps.check]` is v2's
run-and-verify loop. "A check as the format has it" is measured on the pinned
store before the first slice's spec dictates it.

**What this changes above.** Q2's rule loses "no third kind" and gains the
script step. S2's table becomes the default formula's rendering. S1a's third
option and the own-engine section's second declined option are read as
declining `graph.v2`'s compiler, not steps. The non-goal "a workflow engine"
is withdrawn. The seven questions and every other ruling stand.

## Why this solution, and how it differs from Gas City

**Why it was chosen.**

- The founder's direction: flights flown by ephemeral seats only, no person
  and no named seat inside one, so that runs are comparable and approaches can
  be measured against each other. Everything above follows from pinning the
  inputs and moving judgment before takeoff.
- The record as the engine was the only option where a flight outlives the
  shell that opened it and survives the controller restarting.
- The formula as an item's life was chosen after the sitting caught the
  earlier "no workflow engine" ruling doing the arguing. It is the only option
  consistent with the ruling that formulas are a core slot the cockpit edits;
  it is the least hard to undo, because the format is not fleet's own; and it
  keeps the item as the unit of record, which is what the referee measures.
- The flexibility direction: hard rules only where they buy determinism, keys
  and packs everywhere else.

**How it differs from Gas City.**

| | Gas City | fleet |
| --- | --- | --- |
| the engine | an orchestrator ticking over beads; work survives crashes, but a crash is not an event and a restart is a fresh session with no resume | the tick over a record every fact of which is pinned or written once; crashes, holds and cost are events |
| what is pinned per run | the formula's hash and its variables | every input — list, item text, trunk, lock, agent version, model per role, policy, brief — and the two unpinnable ones named as variance |
| a step's executor | always an agent; a script is only a check after an agent's step | a seat or a script, declared on the step |
| steps in the record | one bead per step | one event and one note line per step; the item stays the unit |
| fan-out, retries, scopes | in the compiler: drains, on_complete, retry, scopes, teardown | linear steps first; fan-out and retry as later slices, refused by name until built |
| landing code | not in core; the reference pack's merge queue is an agent step | one lane per project, rebase by the lane, one rerun, a conflict parks |
| needing a person | a human gate bead with no runtime consumer; the guidance is judgment in the prompt | one gate object for every case, asked and answered by verbs, the flight never waiting |
| cost | not captured | written by the controller at every retire, summed per flight |
| measurement | pass or fail per step | escapes per landed item, over flights of the same pinned inputs |
| opinions | roles, review and rituals are pack configuration, with a compiler underneath | the same split, with the seams in core and the runner reading the shared format |

In one line: Gas City is a general workflow orchestrator whose flexibility
lives in its compiler. fleet is a flight engine whose flexibility lives in a
formula file, whose determinism lives in the pins, and whose record is built
to answer which approach produced the best code.

## The stress test

**Flights Under Stress** — the work item that carries the sitting · 2026-09-09.

Ten scenarios built to push Gas City's orchestrator to its limits — five about
surviving a night, five about setting the thing up the way a user wants — each
run first through Gas City as its docs and the 2026-09-05 trial describe it,
then through the flights PRD as landed at 6d6b5d57c and, for the five setup
scenarios, re-run against the final landed solution with S6; ranked within each part by
how hard the scenario pushes.

Gas City docs read: **14 pages in full**. Trial lessons **G1–G24**, gc 1.4.1.
Our side: **`fleet-flights-prd.md`** at 6d6b5d57c. Verdicts: **20**, two per
scenario.

- **10** — scenarios: five for the night, five for the setup
- **2 · 1** — scenarios that break Gas City · that break ours against the final solution
- **5 · 1** — gaps filed as beads · ruled in the sitting

### The ten, in two tables

The verdict for each side, one row per scenario. The detail below is the
evidence. **The night** — what happens while nobody is watching:

| | Scenario | Gas City | Fleet | What decided it |
| --- | --- | --- | --- | --- |
| 1 | The crash at turn twelve | **bends** | **breaks** | Their beads survive, their record is silent. Our PRD has no rule for a dispatched item whose seat died. |
| 2 | The long night on a small box | **breaks** | **bends** | They cap by count, not load, and restart a rate-limited agent every tick. We hold on load; a mid-turn usage limit has no signal yet. |
| 3 | The moving trunk | **bends** | **holds** | They have no landing; merging is an agent step and a conflict can abort the scope. We have one lane that rebases and parks. |
| 4 | The experiment | **breaks** | **bends** | Their runs differ in inputs nobody recorded and carry no cost. Ours pin and price; the referee needs a window and repetitions. |
| 5 | The question at 3 a.m. | **bends** | **holds** | Their human gate has no consumer and the guidance is "judgment is a sentence in the prompt". Ours is one object, asked and answered. |

**The setup** — what a user can make a flight do. This is where the picture
reverses, and it is the picture Alberto's flexibility direction was about:

| | Scenario | Gas City | Fleet | What decided it |
| --- | --- | --- | --- | --- |
| 6 | The ten-step process | **holds** | **holds** | Steps with needs, a check script per step, retry: the formula's reason to exist. Ours, since S6: an item's formula with seat and script steps, linear first. First run, before S6: breaks. |
| 7 | Fan-out discovered at run time | **holds** | **bends** | A drain scatters a convoy into up to 100 units. Ours pins the list at takeoff and flies the forty on the next plan. |
| 8 | Bring your own reviewer, provider per role | **holds** | **holds** | Harness per agent, model per step, a script as a check. Ours, since S6c: a script review step in the formula, and model and provider per step on `run`. First run: bends. |
| 9 | Different rules per item type | **holds** | **holds** | A formula per type. Ours, since S2e: `[[core.flight.rules]]` fills formula, review and gate from the item's own type and labels. First run: bends. |
| 10 | Wait on something outside | **bends** | **bends** | Both have the store's gate check and neither calls it. Ours has a tick already; one bead makes it hold. |

### 1 · The night

Classification, per side: **holds** — the design answers the scenario as written
· **bends** — it survives by an agent's judgment, a pack author's care, or a
rule that is not written · **breaks** — no answer exists in the docs or the PRD.
Every row carries a doc pointer for Gas City and an evidence pointer for us.

#### 1 · The crash at turn twelve

Gas City **bends** · fleet **breaks**.

**Scenario.** Three builders are mid-turn at 2 a.m. The agent binary is upgraded
under them and the daemon re-hosts; one builder's process dies outright with a
half-built branch in its worktree. Nobody is awake.

**Doc.**
[how-gas-city-works](https://docs.gascity.com/getting-started/how-gas-city-works.md)
· [06-beads](https://docs.gascity.com/tutorials/06-beads.md)

**There.** The supervisor "adopts the live sessions it finds — creating a
session bead for each — rather than respawning them", and the trial measured it:
pids kept across a restart (G7). The dead builder's bead "stays open and a fresh
agent picks up the same work": the reconciler restarts a fresh session within a
tick, new transcript, no resume, and the stream carries the wake and *no crash
event of any kind* (G13). The fourth crash in four minutes trips a hold that
lives only in a trace and clears only on a restart (G14). Whether the fresh
agent finds the half-built branch depends on whether the pack put the agent in a
worktree; core says nothing about worktrees. Outcome: the item survives; the
retry is bounded and is not written to the log the operator reads.

**Here.** The controller half is written: the session table adopts by record,
the replacement window suspends dispatch against pid-less rows, a crash is an
event, the halt latch persists and is announced (controller PRD R10–R17, ruled
off G7, G13, G14). The flight half is not. The PRD's state table has no row for
a dispatched item whose seat is gone: the tick waits for a terminal event that
will never come, and the item is dispatched forever. The parked-commit resume
(S3d) and the return-from-the-delivered-commit rule (S2a) are the two halves of
the answer, and neither is wired to `session.crashed`.

**Evidence.** `fleet-flights-prd.md` § S2 state table (no crash row); controller
PRD R14, R17; `gas-city.md` G7, G13, G14.

**Filed.** A decide bead with the options: a fresh seat from the branch's last
commit counting toward a crash cap, or a park at once with the crash as the
question. Listed under Rulings below.

**Bound.** This is a hole in a page written last night, not a measurement of a
running fleet; the fix is one requirement and one state-table row. Gas City's
"bends" is measured on gc 1.4.1 and may have moved.

#### 2 · The long night on a small box

Gas City **breaks** · fleet **bends**.

**Scenario.** Thirty items planned for one night on a six-core laptop with a
Rust suite that takes six minutes. The person's own virtual machine is running.
At 4 a.m. the account's usage limit is reached and every new turn is refused
until 9.

**Doc.**
[formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md) §
drain ·
[configuring-an-agent](https://docs.gascity.com/guides/configuring-an-agent.md)

**There.** Parallelism is a drain: `context = "separate"` "runs all item roots
in parallel" up to `max_units`, "default and hard cap 100". A pool is sized by
`scale_check` within `max_active_sessions` — a count of sessions, with no
reading of the machine. Nothing in the docs reads CPU load, and nothing
serializes suites. At the usage limit, every session fails at its first prompt;
the reconciler restarts each within a tick (G13) at a prompt floor of roughly
36K tokens per start (G21), until the crash hold fires (G14). Thirty items at
four attempts each is on the order of four million tokens; that cost sits in
each session's transcript and in neither the event stream nor the controller's
log.

**Here.** The load belt refuses a dispatch above a per-cpu ceiling and the
flight holds the item with the reading, retried in list order (R9; Q5c);
landings run one at a time on the lane (R15); `max_seats` caps the flight; the
person's load at every dispatch is on the record as variance (Q5). The usage
reading is pinned at takeoff (Q1) and a logged-out first turn is a dispatch
failure that halts dispatching after three (R13). What is missing: a seat that
hits the limit *mid-turn* stalls or exits, and the PRD names no signal for it,
so the flight reads it as a crash (scenario 1's hole) or as silence.

**Evidence.** `fleet-flights-prd.md` R9, R13, R15, § Q5; controller PRD R30 (the
belt); `gas-city.md` G13, G14, G21.

**Filed.** A task: the adapter reads a limit-reached state from the transcript,
the flight holds every remaining dispatch with one `dispatch.failed` naming the
limit, and resumes on the tick when a probe answers. Listed under Rulings below.

**Bound.** The four-million figure is arithmetic from G21's measured floor and
G13's restart cadence, not a measured night. Whether the agent's transcript
exposes a limit-reached state on the pinned version is unmeasured; the work item
names that measurement first.

#### 3 · The moving trunk

Gas City **bends** · fleet **holds**.

**Scenario.** Five items fly together. Items 1 and 3 touch the same file; 1
lands first. Item 4's suite reds once on an arm its diff never touched. Items 2
and 5 are clean.

**Doc.**
[coming-from-gastown](https://docs.gascity.com/getting-started/coming-from-gastown.md)
· [formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md)
§ check, retry, scope

**There.** Core has no landing. Gas Town's refinery — its merge queue — is "a
configured agent + a formula or order post-processing step. A workflow step, not
a standing role": the merge is an agent step, and what makes it correct is that
agent's prompt. A pack author can wrap it in a `check` loop whose script merges
and tests, with `max_attempts`; exhaustion closes the step failed, and under
`gc.on_fail = "abort_scope"` "skips all remaining open scope members", so item
3's conflict reaches 4 and 5 as well. The flake is a `retry` question, and the
orchestrator "classifies each closed attempt" from the agent's own outcome, so a
flake and a real red are separated by the agent too. Two drains with
`context = "separate"` merge in parallel on one trunk; the only exclusivity is
`member_access = "exclusive"`, per member, not per trunk.

**Here.** One lane per project, a lock `land` takes, in a worktree the fleet
owns (S3b, S3d). Item 3 arrives behind: the lane rebases onto the fresh tip,
runs the suite on the rebased commit, records both commits; the conflict parks
item 3 alone with the conflict as its question (S3a). Item 4's red on an
untouched arm is rerun once, both readings on `gate.read`; a second red parks it
(Q3c, R18). Items 2 and 5 land. Nothing here is an agent's judgment.

**Evidence.** `fleet-flights-prd.md` § S3, R15–R18; the reference's
seven-gates-for-three-landings night in § Problem statement.

**Bound.** Only `advance` is built in the first slice; a team wanting one batch
landing waits for P1. A rebase that merges cleanly and is wrong semantically is
caught by the suite or by nothing, which is true of every merge queue.

#### 4 · The experiment

Gas City **breaks** · fleet **bends**.

**Scenario.** The same four items, flown three times on model A and three times
on model B, a week apart. Which produced better code, and at what price?

**Doc.**
[formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md) §
run recording · [events](https://docs.gascity.com/reference/events.md)

**There.** Pinned per run: the formula's bytes (`gc.formula_hash`), its
variables (`gc.graphv2_vars.v1`), and the model as `opt_model` per step. Not
pinned: the agent binary's version, the pack imports in force (the lock is an
import-time object, not a run's), the trunk commit each unit started from, the
city's thresholds. Cost: the events page names no usage field, and the trial
found "no token cost" on any of 676 rows (G17); the transcript exists but was
unreadable for a session in the default working directory (G16), so the price
half of the question had no source to read on the trial's install. Outcome per
step is `gc.outcome` pass or fail, which answers whether the step passed rather
than which run produced better code. Two runs a week apart differ in inputs
nobody recorded, so a difference is not attributable — which is the problem
statement's first sentence.

**Here.** Every input in Q1's table is pinned into the flight directory and
hashed onto the record, the two unpinnable ones named as variance;
`session.retired` prices every seat and `summary.json` folds the flight; the
referee is escapes per landed item (Q6a). Six summary files compared by hand
answer the question. What bends: the referee needs the fourteen-day window to
pass and a project that files bugs against landings, and three repetitions may
not see past sampling; the stats mechanism is another page's.

**Evidence.** `fleet-flights-prd.md` § Q1 table, § Q6, § The flight directory,
R2, R12, R27; `gas-city.md` G16, G17.

**Bound.** Nothing in fleet groups an experiment (Q6b, ruled); the person does.
Gas City's harness pin may exist in city.toml under a name the five-axes page
does not show; the page read names none.

#### 5 · The question at 3 a.m.

Gas City **bends** · fleet **holds**.

**Scenario.** A builder finds the spec's premise false — the table it names does
not exist — and the right fix depends on a product decision. In the same flight,
a UI item needs the founder's look on a device before it merges.

**Doc.**
[formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md) §
gates ·
[understanding-formulas](https://docs.gascity.com/guides/understanding-formulas.md)
· [04-communication](https://docs.gascity.com/tutorials/04-communication.md)

**There.** A `[steps.gate]` of `type = "human"` compiles to a gate bead that
blocks the step, but gates "have no runtime consumer in the current release" and
"no bundled watcher acts on them"; the step waits until someone closes the gate
by hand, the workflow root open the whole time. The documented answer for
judgment is the other way round: "judgment is a sentence in the prompt, not a
branch in code" — the builder decides, or mails the mayor, an always-on agent,
who decides for it. So a question with options travels as a message rather than
as an object of its own. The half-built work survives only if the pack gave the
agent a worktree it comes back to.

**Here.** `fleet ask` commits what the seat has, raises the store's own gate
carrying the question and its lettered options, records the branch and commit,
and the seat is retired; the flight closes without the item (Q3a, S4). The
founder answers with `fleet answer` from the cockpit or a shell, one act; the
next flight cuts a fresh seat from the parked commit with the question and the
answer in its brief (R22). The UI item carries `flight.gate = "review"` and
parks after an accepted review with its artefacts on the gate (S4d, R23). Same
object, same list, same answer verb.

**Evidence.** `fleet-flights-prd.md` § S4, R19–R23; the gate probe on bd 1.2.2
recorded on the work item that carries the sitting.

**Bound.** A builder can still decide instead of asking; when to ask is the
pack's brief, not core's. The phone-shaped gate list is the cockpit PRD's
promise, and until it lands the list is `bd gate list` in a shell.

### 2 · The setup

Five edge cases a user meets when making a flight do what they want, not what
the fleet's author wanted. Same chips, same rule: a doc pointer for Gas City, an
evidence pointer for us.

#### 6 · The ten-step process

Gas City **holds** · fleet **holds** against the final solution (first run, against 6d6b5d57c: **breaks**).

**Scenario.** A user wants every item to go through ten steps with different
requirements each: write the plan, build, run a migration script, call their
deploy API for a preview, wait for it, screenshot it, review, run a security
scan, land, post to their chat. Two of the steps are a script and an API call,
not an agent.

**Doc.** [05-formulas](https://docs.gascity.com/tutorials/05-formulas.md) ·
[formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md) §
check, retry, routing

**There.** This is what a v2 formula is for: ten `[[steps]]` with `needs`, each
step an agent's work bead, a `[steps.check]` with `mode = "exec"`, `path`,
`timeout` and `max_attempts` after any step that needs a script to say pass,
`[steps.retry]` for the flaky API, a per-step model as `opt_model`, and routing
per step through `gc.run_target` or by slinging a cooked step to a different
agent. Two honest limits: there is no exec step type, so "run a script" is an
agent told to run it with a check behind it, and "wait for the preview" is a
gate the release has no watcher for (scenario 10). Holds, because every one of
the ten has a place and a record: one bead per step.

**Here, against the final solution.** Holds. An item's life is a formula from
the formulas slot (S6): the user writes one file with ten steps, each `run =
"seat"` or `run = { script = "<path>" }`, ordered by `needs`; the migration
script and the deploy API call are script steps with the exit contract; the
review, the scan and the landing are steps too; each step writes one event and
one note line on the item. Two bounds inside the holds: "wait for the preview"
is a human gate on a step until non-human gates land (P1, scenario 10), and a
per-step retry policy is P1, so a flaky API call is a script step that fails and
returns rather than retries.

**Here, as first run.** An item's life inside a flight is four verbs and they are constants
(S2). The ten steps live in the pack's brief, and one seat follows them as text;
the project's suite in `land` is the one script gate; the guards refuse
mistakes. There is no step object, no per-step event, no place for a script or
an API call except inside the builder's own turns or the suite, and the PRD's
non-goal says so: "a workflow engine". Breaks for this user, by a choice made
last night. Alberto's flexibility direction is the reason this row is on the
page.

**Evidence.** `fleet-flights-prd.md` § S2 (the four verbs, constant), §
Non-goals; the packs PRD's four-verb ruling.

**Filed.** A decide bead with three options: as landed; hooks between the verbs
with an rc contract and one event each, files pinned by the lock; a pack formula
per item. Listed under Rulings below.

**Bound.** Gas City's holds is a holds of the formula format; the trial never
ran a ten-step v2 formula, and two shapes the docs describe — an until-loop
that runs once, and a wait with no bundled watcher — are inside the format
too. Ours breaks on the engine and
not on the pack: a skill can carry ten steps as prose today.

#### 7 · Fan-out discovered at run time

Gas City **holds** · fleet **bends**.

**Scenario.** One item says "migrate every screen to the new navigation". Nobody
knows there are forty until an agent has looked. Each screen needs its own build
and review, and one step at the end reads them all.

**Doc.**
[formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md) §
drain, on_complete

**There.** Designed for it twice over. A `drain` "scatters the input convoy into
unit convoys and runs an item formula per unit", `context = "separate"` in
parallel up to `max_units` (default and hard cap 100), `on_item_failure` to
continue or skip the rest; `on_complete` with `for_each = "output.screens"` and
a bonded formula per item, `parallel` or `sequential`, with the step marked
`gc.output_json_required`. The docs call it "the pack's single load-bearing
parallelism pattern".

**Here.** The list is pinned at takeoff and "anything not on the list is a
source of variance" (Q1 rule 1) — a constant, because a list that grows at run
time is a query answered mid-flight, which is the problem statement's first row.
So the discovering seat files forty items with a discovered-from edge and a
forty-first blocked on all of them, delivers the plan, and the next flight flies
them: `fleet plan --ready 40` by hand, or autopilot on with `max_open` raised
and a nightly plan order. The same night if the switch is on and the box allows;
a night later if not. Bends: the user gets the fan-out across flights and never
inside one.

**Evidence.** `fleet-flights-prd.md` § Q1 rule 1, § S5 (plan --ready, max_open),
R1, R24.

**Bound.** Whether "next flight" is minutes or a day is the user's switch and
cap, not the design. A fan-out inside a flight is a deliberate non-answer; if
Alberto wants it, it is the pinned list rule that moves, not a slice.

#### 8 · Bring your own reviewer, provider per role

Gas City **holds** · fleet **holds** against the final solution as of the S6c and S2e rulings (first run, against 6d6b5d57c: **bends**; against S6 alone: **bends**).

**Scenario.** A user wants the review to be a script: post the diff to their
company's policy API and block on its verdict. Another wants the builder on one
provider and the reviewer on a second, so the two never share a blind spot. A
third wants a cheap model for chores and the best one for features.

**Doc.**
[configuring-an-agent](https://docs.gascity.com/guides/configuring-an-agent.md)
· [formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md)
§ check, opt_*

**There.** The five axes make the harness (`provider`: claude, codex, gemini),
the model (`option_defaults.model`), the upstream, the transport and the runtime
each a per-agent setting; a step's `opt_model` overrides per step; a script
verdict is a `check` with `max_attempts` whose exit is the verdict. All three
users are configuration.

**Here, against the final solution.** Holds. The policy-API reviewer is a
formula: a project shadows the default so the `review` step is a script step
that posts the diff and reads the exit — a return on 1, a park on 3 — before
anything lands. The second provider and the cheap model are keys on the step's
`run`, `provider` and `model`, the role's row the default (S6c, ruled after this
scenario was first read); the build is a work item filed off the sitting. Read
against S6 alone this bent on the provider per role; the ruling closed it.

**Here, as first run.** The model per role is pinned (Q1) and review is a policy with three
values, none, spawned and self (S2b); a script is not a value, and the provider
per role is not a key, though the vision promises "mix them in one fleet". The
suite is the one script gate and it runs at landing, after review, so the
policy-API user gets their verdict late and cannot make it a return. Bends: a
pack can wrap the script inside the review skill's prompt and let a seat run it,
which is an LLM relaying an exit code.

**Evidence.** `fleet-flights-prd.md` § Q1 table (the model per role), S2b, R14;
`fleet-vision.md` § Bring your own agent.

**Filed.** A task: `review = "script"` with an rc contract and a pinned script,
and provider and model per role on the record. Listed under Rulings below.

**Bound.** Gas City's per-step model was measured only as an accepted option in
the trial (G11's schema), never as two providers in one convoy. Ours bends on
two missing keys, not on a constant.

#### 9 · Different rules per item type

Gas City **holds** · fleet **holds** against the final solution as of the S6c and S2e rulings (first run, against 6d6b5d57c: **bends**; against S6 alone: **bends**).

**Scenario.** Bugs must start with a failing test. Features need a design note
reviewed before any code. Chores skip review. Anything touching payments parks
for the founder before landing. The user wants to write this once, not per item.

**Doc.**
[understanding-formulas](https://docs.gascity.com/guides/understanding-formulas.md)
· [07-orders](https://docs.gascity.com/tutorials/07-orders.md)

**There.** A formula per type, chosen at `gc sling` or by an order;
`review_mode` as a variable with `enum` values; a step routed to a reviewer role
or not; the payments rule a prompt sentence, because "judgment is a sentence in
the prompt, not a branch in code". Written once, in the pack. Holds, with the
caveat that "anything touching payments" is an agent's judgment and not a rule
the orchestrator enforces.

**Here, against the final solution.** Holds. A formula per type exists by name
on the item, and the "once" is `[[core.flight.rules]]` (S2e, ruled after this
scenario was first read): an ordered list in policy matching the item's own type
and labels and filling formula, review and gate, first match wins, the item's
own object winning, computed at takeoff. A product item gets the rigorous
formula and the founder's gate; a documentation item gets no reviewer; the
failing-test-first rule is the bug formula's first step. Read against S6 alone
this bent on the missing map; the ruling closed it.

**Here, as first run.** The per-item object is right — `flight.review` and `flight.gate`,
pinned at takeoff, never inherited (S2c) — and the "once" is missing: nothing
maps an item's type or label to those defaults, so a person or a pack's order
writes them on every item, and the brief is one template per pack with no branch
on type. The failing-test-first rule is brief text. Bends: everything is
expressible, nothing is declarable.

**Evidence.** `fleet-flights-prd.md` § S2c, R4, R23; core's brief template in
`packs/core/assets/brief.md`, one file.

**Filed.** Rides the steps-between-the-verbs decide bead as its second question:
a `[core.flight.by_type]` map of defaults for review, gate and hooks.

**Bound.** A label-to-policy map is one key; the risk it carries is the one S2c
declined for labels, an epic's label reaching every child, and the map must read
the item's own type and never a parent's.

#### 10 · Wait on something outside

Gas City **bends** · fleet **bends**.

**Scenario.** A step must wait for CI on GitHub to go green, or for a pull
request someone else merges, or for a deploy that takes twenty minutes, or for
another project's item to close.

**Doc.**
[formula-spec-v2](https://docs.gascity.com/reference/specs/formula-spec-v2.md) §
gates · `bd gate check --help` on bd 1.2.2

**There.** The gate vocabulary names `gh:run`, `gh:pr`, `timer` and `mail`, and
the spec says "no bundled watcher acts on them". The store underneath has one:
measured this sitting, `bd gate check` resolves a run on completed-and-success,
a pull request on MERGED, a timer on its timeout and a cross-project item on
closed, and escalates a failed run or a closed request. Gas City does not call
it; a user writes an exec order that runs it every minute, which is one file.
Bends.

**Here.** The gate is the store's own (S4a), so the same check applies, and the
PRD defines human gates only; nothing on the tick evaluates the rest, and `fleet
ask` raises human gates alone. The tick already exists and already reads the
store, so the distance to holds is one bead: run the check per tick, treat a
resolved gate as an answered one and an escalated gate as a park. Bends today
for the same reason as theirs, with a shorter road.

**Evidence.** `fleet-flights-prd.md` § S4, R19–R22; the gate check measured this
sitting on the board of the work item that carries the sitting.

**Filed.** A task: the store's gate check on the tick, `fleet ask --type`,
escalation as a park. Listed under Rulings below.

**Bound.** The GitHub gates shell out to the `gh` CLI and need a logged-in one
on the box; a deploy webhook has no gate type on either side and would be a
timer or a bead gate closed by the webhook's receiver.

### 3 · Corrections the author owes

- **Correction · the own-engine table.** The PRD's § Own engine says Gas City's
  gate "holds the workflow open". The spec we read says it differently: gates
  "have no runtime consumer in the current release", so the step waits until a
  hand closes the gate. The PRD's sentence describes a hold the spec does not
  state; a one-line edit rides the next landing of that page.
- **Correction · cost.** The PRD says Gas City's stream carries no cost, citing
  the trial. The docs agree by omission, not by statement; the claim rests on
  G17's count of 676 rows, which is a measurement of one idle city on one
  version.

### 4 · Read and ruled out as scenarios

- **A pack updated mid-flight.** Both sides pin it: Gas City's formula hash
  detects drift; our lock is recorded at takeoff and the running flight ignores
  the edit (Q1 rule 2). Not a stress.
- **Two flights on one project.** Ours records the overlap and shares the lane
  (Q5b); Gas City runs drains side by side with no record of it. The difference
  is scenario 4's, not a scenario of its own.
- **A formula edited while poured.** Gas City's own gap (drift detected,
  recovery unspecified); we have no formula. Nothing to compare.
- **An agent that never finishes.** Gas City's idle timeout never fired for an
  interactive session (G15); ours reads the transcript and suggests rest for
  named seats only, and a spawned seat's silence is scenario 1's hole under
  another name.

### 5 · The question the keystone had answered for us

After part two, Alberto named the bias: the packs sitting's "no workflow engine"
ruling had been carried as a keystone through the problem and solution sittings
without ever being put as a question, and it did the arguing in six places — the
problem PRD's non-goal, Q2's "no third kind", S1a's third option, the
own-or-theirs question's second option, S2's four verbs, and scenarios 6 to 10
above. It also contradicted a ruling already on the record: formulas are a core
slot the cockpit edits. So the question was put once, with an option set written
from the seven questions and that ruling only, priced as below.

| | Step model in core, over the formulas slot | Same, every step a bead | Hooks between four fixed verbs | Fixed four verbs |
| --- | --- | --- | --- | --- |
| **What the user writes** | one formula file: ten steps, each a seat or a script | the same file | a script per hook point, up to six points | nothing; the steps go in a skill's prose |
| **The ten-step example** | fits: two script steps, eight seat steps, in order | fits | only if the ten collapse onto six fixed points | does not fit |
| **Per-item flexibility** | a project or a pack shadows the default formula; a type can name its own | same | same, at the points only | none |
| **What core builds** | a runner: read the formula, walk steps by needs, run a script or spawn a seat per step, one event per step | the runner plus pouring and reading molecules | six call sites in the four verbs and a script contract | nothing new |
| **Build cost, honest** | the largest: roughly the four verbs put together | larger still | small: a week's slice | zero |
| **Run cost per item** | one seat per judgment step; a script step costs nothing; one event per step | plus one bead per step written to the store, read by every tool | as fixed, plus the hooks | one seat, one review |
| **The record** | the item stays the unit; steps are lines on it | ten beads per item for the ten-step user | the item stays the unit | the item |
| **Fits the seven questions** | yes: the formula is a pinned input; a judgment step is a seat, a code step is a script; a step that needs a person is a gate | yes | yes | yes |
| **Fits the cockpit ruling** | yes: formulas are the slot the cockpit edits, and this is their runner | yes | no: hooks are not formulas, the slot stays empty | no |
| **Hard to undo** | the format, and it is bd's — Gas City's v1 — the one already shared | the same, plus every graph filling with step beads | the six points: a seventh later is a break | the four verbs |
| **Gas City interop** | a v1 formula runs here with one field added | same | none | none |

**Why the seat recommended the first.** Three reasons, in order of weight. It is
the only option that agrees with something already ruled: formulas are a core
slot the cockpit edits, and an empty slot with no runner is a promise with
nothing behind it. It is the least hard to undo of the flexible ones: the format
is not ours, so if the runner is dropped the files still mean something
elsewhere, where six hook points of our own would break every pack that used
them. And the record stays the item: the referee is escapes per landed item, and
ten beads per item would make every list and the cockpit ten times noisier for
the one number that matters. What it costs: the biggest slice on the board
before fan-out and retries; the first slice is linear steps only, and core's
default formula is the four verbs written as a formula, so a user who never
touches it gets exactly today's flight. Why not hooks: the cheap answer, and the
right one if the cockpit ruling did not exist; with it, hooks leave a formulas
slot that does nothing and a second mechanism beside it.

### What Alberto ruled, and the work items filed

| Work item | What | Shape |
| --- | --- | --- |
| a decide item, filed on the fleet epic | The crashed-seat rule for a dispatched item: a fresh seat from the branch's last commit under a crash cap, or a park at once. Scenario 1. | decide, with lettered options on the item; a PRD row and one requirement |
| a task, filed on the fleet epic | A usage limit reached mid-flight: the adapter's signal, the flight's hold, the resume. Scenario 2. | task; the measurement of the signal first |
| a decide item, filed on the fleet epic | Steps between the verbs. **Ruled 2026-09-09 in the sitting, after section 5:** a step model in core over the formulas slot, linear steps in the first slice; the item closes on the PRD landing that carries the ruling. | ruled; the PRD's S6 |
| a task, filed on the fleet epic | review = script with a pinned script, and provider and model per role on the record. Scenario 8. | task; two PRD edits |
| a task, filed on the fleet epic | The tick runs the store's gate check so an item can wait on a run, a pull request, a timer or another item. Scenario 10. | task; small |

Ruled in the sitting: section 5's question, recorded on the epic as
`flights-s6-item-life`. Still open: the crashed-seat rule, the usage limit
reached mid-flight, review = script, and the gate check on the tick. The
findings above stay as written; scenario 6's "breaks" is the verdict on the page
as landed at 6d6b5d57c; the setup cards carry a second reading against the
final solution at 0a0b2e53b, the first kept beside it.

**Coverage.** Read in full: how-gas-city-works, formula-spec-v2, events,
exec-session-provider, pack-spec, 07-orders, understanding-formulas,
configuring-an-agent, 03-sessions, coming-from-gastown, 06-beads, 05-formulas,
04-communication, the docs index; `fleet/brain/lessons/gas-city.md` G1–G24. Not
read (a deliberate remainder): the cli reference, the API and config schemas,
system-packs, the herdr provider, the Gastown config recipes. Every Gas City
claim is the docs' or the trial's on gc 1.4.1; nothing was re-run against a live
city tonight.

Our side is read from `fleet/brain/prds/fleet-flights-prd.md` and the controller
PRD at 6d6b5d57c; the full record is on the work item that carries the sitting
and the epic's rulings.

## Record

The rulings live on the work graph: on the fleet epic, in its notes, each in the
keyed grammar the sittings used — section 5's question is recorded there as
`flights-s6-item-life` — and on the work item that carries the sitting, where
the founder's directions are recorded verbatim. This file is the rendering of
that record, and the definition it decided is `../prds/fleet-flights-prd.md`.
