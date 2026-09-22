# PRD: fleet-flights

**One sentence:** a flight is a function from pinned inputs to a landed set and
one record, flown by ephemeral seats only — no person and no named seat inside
it — so that two flights of the same inputs are comparable, every difference
between them is attributable, and ways of building software with agents can be
tested against each other for which produces the best code.

Drafted 2026-09-08, from a sitting that began by defining the problem and
refused to reach for a solution until it was written down. The reference fleet —
the factory that has flown every flight of a live product since August 2026 — is
the counter-example the problem statement is measured from. Gas City's
`graph.v2` formulas are one existing answer to part of it and are read as such,
not adopted. The packs PRD (`fleet-packs-prd.md`) and the cli PRD
(`fleet-cli-prd.md`) each describe `fly` and `autopilot` as they stood before
this page; where this page rules differently, those pages are edited in the same
landing, and the decisions and their alternatives are in `../adrs/flights.md`.

**Status: ruled 2026-09-09.** This page is the definition; the decisions, their
alternatives and the stress test that pushed on them are `../adrs/flights.md`.

---

## The solution in one page

The whole of it, for a reader who opens this page cold; the why is
`../adrs/flights.md`. **Re-pointed 2026-09-17 (workflows-formula-fate):**
`plan`, `fly`, `autopilot` and the advance left core on that date, and
composition is tiny's preboard and takeoff workflows over `fleet run`; what
follows is the solution as landed, kept as the record.

**What was landed.** A flight is a record, not a program. A person writes a
list of items down with `fleet plan`. `fleet fly` pins every input that could
make two runs differ into a directory — the list, each item's text, the trunk
commit, the pack lock, the agent version, the model per role, the policy, and
each item's formula and brief — and returns. The controller's tick reads that
record every few seconds and does the one thing owed per item. Nothing about
a flight lives in memory, so a crash or an upgrade resumes by reading.

Each item's life is a formula from the formulas slot, in the format packs
already carry. Core ships a default one whose steps are dispatch, build,
deliver, review and land, so a fleet that never touches it flies the plain
flight. A project or a pack shadows it with its own steps. A step is either a
spawned seat whose brief is the step, or a script with an exit contract:
judgment is a seat, code is a script, both are steps.

Around that: every seat inside a flight is created and retired by the flight.
Review is a policy with a default. Landings queue one at a time per project
in a checkout the fleet owns, and a delivery behind the trunk is rebased by
the lane. Every time a flight needs a person it is the same object, the
store's own gate, raised by `fleet ask` and answered by `fleet answer`; the
flight closes without the parked item and the next flight resumes it from its
commit. Every decision is marked constant, key or pack, and the constants are
only the ones that make a flight a function of its inputs.

---

## Problem statement

A flight on the reference is not a function of its inputs. The same list of
items flown twice would not produce the same landings, and the differences
would not be attributable to anything about the code. Read from the record:

- **The operator's context is the flight's fuel tank.** One flight was flown by
  three sessions of one named seat, each waking into a resume plan and
  re-reading on the order of 120K tokens before its first act. Pulls were
  declined at sixty and seventy percent of a context window because of the
  chair, not the work.
- **A person is in the loop by accident.** A pre-dispatch pass blocks on the
  other architect having a live session. A builder's blocking question routes
  to a person. A landing hold spanning two operators was settled by message.
  Each is a place where the flight's outcome depends on who was awake.
- **Review is a stateful seat's judgment.** What a reviewer catches depends on
  that seat's accumulated lessons that day. Two reviewers of one delivery would
  write different verdicts, and nothing about the flight records which one it
  got.
- **The landing shares one machine and one worktree.** Seven gates for three
  landings, suites colliding under load, a finish refused because the other
  operator landed underneath it. A landing's result depends on what else was
  running.
- **The record of a flight is scattered.** Notes on items, a manifest, two
  diaries, a builder log, messages. Reconstructing one flight costs an
  architect an hour, and no measurement folds over it.
- **Cost is not captured.** The stream carries no token cost, so "which
  approach produces the best code" cannot be priced.

Spawned seats improved the one input they touched: a builder now starts from a
worktree at a known commit with a brief as its first turn, and retires.
Everything else in the loop is still a person or a persistent seat.

## The problem, as a claim

**A flight is a function from fixed inputs to a landed set and a record, with
no persistent seat and no human inside it, such that two flights of the same
inputs are comparable and every difference is attributable.**

The named seats are the company. Architects keep a backlog of ready flights and
write specs to the zero-clarification bar. Named builders review and explain on
request. Back-office seats run the intakes. Nobody on the roster is on a
manifest. Flights are flown by ephemeral seats that wake with the context they
need, do the work, and clock out.

## The seven questions

Each is independent of any solution. Each is ruled in its own section below,
with its letter, as the sitting reaches it.

1. **The inputs.** What are they, and what pins them? Anything not on the list
   is a source of variance.
2. **The roles.** Which roles exist inside a flight, and which are ephemeral?
   Dispatcher, builder, reviewer, lander, and whoever answers a builder's
   question.
3. **The person.** What happens when a step needs one — a question, a third
   return, a red gate nothing in the flight can fix?
4. **The record.** What does a flight write, and where, so that a fold can read
   it and an experiment can compare two?
5. **The machine.** What is the box's contract — concurrency, load, one
   landing at a time or not?
6. **The measurement.** What is measured, and against what, so that "best
   code" has a referee?
7. **The company.** What is the named seats' job once none of them flies, and
   which standing rules that reverses?

---

## Q1 — The inputs, and what pins them

An input is anything a flight reads that could differ between two flights of
the same list. The table names each, what lets it vary on the reference today,
and what pins it. **Two rules follow the table and are the substance of this
question:** a flight does not open with an unpinned input, and no input changes
while a flight is open.

| Input | How it varies today | What pins it |
| --- | --- | --- |
| **The list** | `fly` takes "the next ready items", a query answered differently a minute later | the flight's list is explicit: item ids written on the flight's record at takeoff, never a query |
| **Each item's text** | a description is editable; a spec edited after dispatch is a different spec | a hash of each item's title, description and acceptance at takeoff, on the record; the brief rendered for each item stored beside the record, since the brief is what the seat actually reads |
| **The trunk** | the trunk advances under a flight as its own landings and others' land | the project's trunk strategy, pinned on the record; the trunk commit at takeoff; and for every dispatch, the commit its worktree was cut from, so a landing's base is attributable to the landing before it |
| **The pack** | a pack updated by hand between flights changes the brief, the skills, the guards and the note templates | the lock — every installed pack's source, version and commit — copied onto the record, and the resolved layer order with it |
| **The agent** | the agent's binary can move between flights, and does at every upstream release | the pinned version from policy and the live version the adapter reads at takeoff, both on the record; a live version off the pin is a refusal or a marked run, never silent |
| **The model per role, and per step** | a builder and a reviewer can be run on any model | the model and provider per role from policy, and per step from the item's formula where a step names its own, on the record |
| **The policy** | `fleet.toml` is re-read the moment it changes, so a cap or a return limit can move mid-flight | a snapshot of the effective policy at takeoff, stored beside the record and hashed onto it; the running fleet keeps re-reading, the flight does not |
| **The project's gates** | the suite command lives in `project.toml`; its content lives in the tree | the command on the record; its content is inside the trunk commit |
| **The permission posture** | the adapter's start flags | part of the policy snapshot |
| **The seats' prior state** | a named seat carries a diary, a ledger and yesterday; a spawned seat carries nothing | ephemeral seats only, by the claim — the input is empty by construction |
| **The provider's user-level configuration** | settings, memory, instructions and servers in the person's home directory reach every session the adapter starts | isolated: each spawned seat starts with its own empty configuration directory (Q1e) |
| **The every-turn rules** | the reference's brief pulls the tail of a moving log into every spawn | the brief takes text from the pinned pack and the item only; nothing in it is read from a file that moves between flights |
| **The account** | usage limits decide whether a flight finishes | the account identity on the record, and the usage reading at takeoff |
| **The network** | package registries, fetched dependencies, external calls | not pinnable; recorded as a named source of variance, with the lock files inside the trunk doing what they can |
| **Sampling** | the model is not deterministic at a fixed prompt | not pinnable; the irreducible variance, measured by repetition rather than removed |
| **Time** | the clock, and everything scheduled on it | not pinnable; the takeoff and landing stamps on the record |

**Rule 1 — no unpinned input opens a flight.** `fly` writes every pin above
onto the flight's record before the first dispatch, and refuses, naming the
input, when one cannot be read: an unreadable lock, a policy that does not
parse, an agent whose version cannot be measured. The two sources of variance
that cannot be pinned, the network and sampling, are written on the record as
such, so a comparison knows what it cannot control.

**Rule 2 — no input changes while a flight is open.** The policy a flight
runs under is its snapshot; a person editing `fleet.toml` mid-flight changes
the next flight. The pack a flight runs under is the lock it recorded; a pack
added mid-flight is the next flight's. The trunk advances, because landings
are the point, and every advance is recorded per dispatch so it is a sequence
and not a surprise.

**Ruled.** Q1a — the pins live in `flights/<id>/` under the machine directory:
the inputs file, the policy snapshot, each item's rendered brief; the
directory's hash and a summary go on the flight's record item, one
`flight.opened` event carries the hash, and the brief a seat actually read is
kept. Q1b — a flight runs under the policy snapshot taken at takeoff; an edit to
`fleet.toml` reaches the next flight, and the running fleet keeps re-reading for
everything else. Q1c — the trunk strategy is a project policy key, declared per
project and pinned on the flight's record beside the commits; its values are
advance per landing with the base recorded per dispatch, one takeoff commit with
one batch landing, and a pull-request shape where a landing opens a request a
person or a rule merges, and `fly` refuses a value it does not implement, naming
it. Q1e — the provider's user-level configuration is isolated: the adapter
starts each spawned seat with its own configuration directory holding only what
the pack's overlay puts there, so nothing from the person's home directory —
settings, memory, instructions, servers — reaches a flight, and the flight's
inputs are the pack and the item and nothing else.

## Q2 — The roles, and which are ephemeral

A role is anything inside a flight that acts between takeoff and the last
landing. The table names each, who holds it on the reference, and what the
claim makes of it. **The rule that follows the table is the substance of this
question:** inside a flight, a role that needs judgment is a spawned seat with
a brief, and a role that does not is code — a verb, or a script step of the
item's formula (S6).

| Role | What it does | On the reference | Under the claim |
| --- | --- | --- | --- |
| **The dispatcher** | opens the flight, pins the inputs, dispatches each item, advances the flight as verdicts land, closes it | a named architect, the operator, whose context is the flight's fuel tank | code: `fly` advancing the flight's record, with no seat in the role |
| **The builder** | reads the brief, builds, delivers | a spawned seat since the reference began spawning | unchanged: one spawned seat per item, retired at delivery or return |
| **The reviewer** | reads the delivery against the item, walks its decisions, writes ACCEPTED or RETURNED | the operator, a named seat with a ledger | a policy (Q2b): none, or a spawned seat per delivery with a review brief — the item, the delivery note, the diff, the pack's review skill — blind to the builder, its verdict on the item |
| **The lander** | gates, squashes, runs the suite, pushes, closes the item | the reviewer, in the reviewer's own worktree | code: `land`, invoked by the flight on an ACCEPTED verdict, in a worktree the flight owns |
| **The answerer** | answers a builder's blocking question | a named architect, or Alberto | nobody — a question parks the item (Q3); a step that names a person as its gate holds until the gate is resolved from outside (Q2d); the zero-clarification bar is held before takeoff, and a park is the measurement of it |
| **The composer** | decides which items make a flight | the operator with the board | the company, before takeoff: a flight is a composed list on the backlog, never a query at takeoff (Q1) |
| **The spec judge** | the worth and readiness passes | the other architect, blocking on a live session | the company, before takeoff: an item without its passes is not an input, and `fly` refuses it by Q1's first rule |
| **The reporter** | the morning's page: what landed, what is owed | the operator, at the end | nobody — the record is the report (Q4), rendered by a fold and shown by the cockpit |

**The rule.** Two roles carry judgment inside a flight, building and
reviewing, and both are spawned seats: born from the pack, the brief and the
pinned trunk, blind to each other, retired when their verdict or delivery is
on the item. Every other role is a verb reading the record. Three roles leave
the flight entirely — composing, judging the spec, answering questions — and
become the company's work before takeoff, which is what makes the flight a
function of its inputs: the judgment that used to happen inside it happens
before it, on the record, where an input can be pinned.

**Ruled.** Q2a — the dispatcher is code: `fly` pins the inputs, dispatches,
reads verdicts off the items and advances the flight's record, and no seat holds
the role. Q2b — review is a fleet policy with a per-item override, the value in
force pinned on the flight's record: `none`, where an accepted delivery lands on
the builder's word; `spawned`, a reviewer seat per delivery, born from the
pack's review skill, the item, the delivery note and the diff, blind to the
builder, its verdict on the item, retired; and `self`, the builder's own read
against the item before it delivers — an item's override is written on the item
before takeoff, so it is an input. Q2c — on an ACCEPTED verdict, or on delivery
when review is `none`, the flight's code runs `land` in a worktree the flight
owns, under the project's trunk strategy, with the reviewer seat already retired
and the landing depending on nothing about it. Q2d — nobody answers inside a
flight: a blocking question parks the item and the flight moves on, and a step
may name a person as its gate, where the flight holds at that step and resumes
when the gate is resolved from outside the flight — through the work graph's
gate, or the cockpit's decisions-as-questions — and never by a person inside it.

## Q3 — When a step needs a person

Three things inside a flight can need a person: a builder's blocking
question, an item that has come back the number of times policy allows, and a
gate that reads red on something the delivery did not touch. Q2d added a
fourth that is not a failure: a step that declares a person as its gate. **The
rule that is the substance of this question:** a flight never waits for a
person. Waiting costs nothing only when nothing is alive, so the seat that
needs the person writes what it needs on the item, delivers what it has, and
retires; the item is parked; the flight goes on and closes without it; the
person answers in the morning, from outside, and the answer makes the item
ready again for the next flight.

| The need | What the reference does | Under the claim |
| --- | --- | --- |
| **A blocking question** | routes to a person or a named seat with a live session; the flight stalls if none is there | the seat commits what it has to its branch, writes the question on the item as a gate carrying its options, and retires; the item is parked |
| **A return at the cap** | a third return parks the item for a person | unchanged: parked, the last findings as the question, the options being another attempt, a re-spec, or a drop |
| **A red gate the delivery did not touch** | the operator reruns by judgment, up to three times, then holds and rings the other architect | one rerun on a red the diff did not reach, then parked with both gate readings as the question |
| **A human gate on a step** | a molecule's human gate, resolved by hand | the item holds at the gate from the moment it is dispatched; the flight does not dispatch it; a resolved gate makes it ready |

**What a park is.** The item leaves the flight's active set with its state on
the record: the branch and its last commit, the seat that held it, the reason,
and the question with its options. The flight's record marks it parked. A
flight closes when every item on its list is landed or parked, and a closed
flight never reopens. **What a resume is.** A person resolves the gate from
outside — the work graph's own gate, or the cockpit's decisions as questions —
and the item is ready again. The next flight that takes it dispatches a fresh
seat whose brief carries the question and the answer and whose worktree is cut
from the parked branch, so nothing built before the question is lost and no
seat ever waited for it.

**Ruled.** Q3a — a flight never waits: a park closes the item's place on the
flight, the flight closes when every item on its list is landed or parked, a
closed flight never reopens, and a resolved gate returns the item to the pool
for the next flight to take with a fresh seat. Q3b — the question is the work
graph's own gate: a human gate on the item, its text the question with lettered
options, listed by the graph's gate command and answered by resolving it; the
cockpit's decisions list is that command, and a formula's human gate is the same
object, so one mechanism serves a builder's question, a step that names a
person, and a parked item's "what next." Q3c — a gate red on something the
delivery did not touch gets one rerun, and a second red parks the item with both
readings as its question. Q3d — the seat commits what it has before retiring,
the park records the branch and the commit, and the resume cuts its worktree
from there, so nothing built before the question is lost and the resume is
attributable to the commit it started from.

## Q4 — What a flight writes, and where

Three stores already exist and each holds one kind of thing: the **work
graph** holds the notes a person reads on an item — the order, the delivery,
the verdict, the landing's gate table — in the grammars the packs PRD fixes;
the **stream** holds typed events with ids, sequences and stamps, and is what
stats fold over; the **flight directory** (Q1a) holds files — the inputs, the
policy snapshot, every brief. **The rule that is the substance of this
question:** every fact a flight produces is written once, by the layer that
did the thing, into the store that kind of fact belongs to, and the report is
a fold over those three and never a fourth thing written by hand.

| Fact | Written by | Where |
| --- | --- | --- |
| the flight opened, with its inputs' hash | `fly` | `flight.opened` on the stream; the flight's record item; the flight directory |
| an item dispatched, to which seat, from which commit | `dispatch` | the order note on the item; `item.dispatched` on the stream, carrying the seat and the base commit |
| a delivery, its commit, its three machine-read lines | `deliver` | the delivery note on the item; `item.delivered` on the stream, carrying the commit |
| a verdict and its findings count | `review` | the verdict note on the item; `item.reviewed` on the stream, carrying the verdict and the count |
| a landing, its commit on the trunk, its gate table | `land` | the landing note on the item; `item.landed` on the stream, carrying the trunk commit |
| a return, and which return it is | `review` | the verdict note; `item.returned` on the stream, carrying the count |
| a park, its reason, its branch and commit, its gate | the verb that parked it | the gate on the item; `item.parked` on the stream |
| a seat's cost: context tokens at retire, turns, wall time | the controller, at retire, from the adapter's transcript | `session.retired` on the stream, carrying the seat, the item and the numbers |
| a gate's reading, red or green, and a rerun | `land` | the landing note's gate table; `gate.read` on the stream, one per reading |
| the flight closed: landed, parked, returned, cost, wall time | `fly` | `flight.closed` on the stream; the flight's record item; a summary file in the flight directory, derived and marked so |

Two things follow. **Cost becomes a fact** for the first time: the adapter's
transcript verb already reads a session's context tokens and last-turn usage,
and reading it at retire, by the controller and never by the seat reporting on
itself, gives every item a price. **The report is a fold.** What landed, what
is owed, what it cost and how long it took are computed from the stream
between `flight.opened` and `flight.closed`, rendered by the cockpit's Flights
view and printed by a shell verb; nobody writes a page.

**Ruled.** Q4a — each of the four verbs writes its note on the item and one
typed event on the stream carrying the ids and commits: `item.dispatched`,
`item.delivered`, `item.reviewed`, `item.landed`, `item.returned`,
`item.parked`; `fly` writes `flight.opened` and `flight.closed`; `land` writes
`gate.read` per reading; the note is for a person, the event is for the fold,
and each fact has one writer. Q4b — the controller writes `session.retired` at
retire, carrying context tokens, turns and wall time keyed to the seat and its
item, read through the adapter's transcript verb and never self-reported. Q4c —
at close `fly` writes one summary file into the flight directory — the landed
set with commits, the parks, the returns, the gate readings, the cost and the
timings, plus the stream sequence range it folded — derived and marked so, so it
is re-derivable and never a second source of truth, and two flights compare with
one diff. Q4d — there is no report page: the cockpit's Flights view renders the
fold, a shell verb prints it, and the decisions owed are the open gates.

## Q5 — The machine's contract

One machine runs the controller, every seat's worktree, every landing's suite,
and whatever else the person is doing. On the reference the machine is the
largest unpinned input: two landing suites ran beside each other at load
thirty, a builder's synthetic load stalled another operator's suite for eleven
minutes, a finish was refused because the other operator landed underneath it,
and seven gates were spent on three landings. **The rule that is the substance
of this question:** the machine's share of a flight's outcome is either
scheduled by the fleet or recorded as variance, and never left to whoever ran
first.

| Contract | On the reference | Under the claim |
| --- | --- | --- |
| **Dispatch under load** | the load belt: the five-minute average against a per-cpu ceiling, and transient seats mid-turn against a cap; a refusal at spawn | unchanged as the gate; the numbers are policy and pinned on the flight (Q1); a refused dispatch is held and retried on the tick in the list's order, and the hold is recorded |
| **Landings** | each operator lands from their own worktree; two can run the suite at once | one landing lane per project on the machine: landings queue and run one at a time, in arrival order, in a worktree the fleet owns |
| **Suites and build caches** | a scratch tree once shared a build cache with the landing worktree and the landing gate ran a mutant | every worktree — a seat's, the landing lane's — builds in its own cache; nothing shares one |
| **Flights beside each other** | class-scoped: regional and national fly concurrently, overnight is exclusive | ruled below: what shares a machine, and what the record says about it |
| **The person's own load** | a virtual machine and a desktop app once put the box at load twenty-two with no suite running | not schedulable; the five-minute load at every dispatch and every landing is on the record, so a slow flight can be told from a jammed one |

**Ruled.** Q5a — one landing lane per project: landings queue and run one at a
time in arrival order, in a worktree the fleet owns with its own build cache, so
two suites never run together, a finish is never refused by another landing, and
the trunk advances in a recorded sequence. Q5b — any number of flights share the
load belt and the landing lane, and every flight's record names the flights that
overlapped it as a source of variance. Q5c — a dispatch the belt refuses is held
and retried on the tick in list order, the hold and its load reading recorded;
the flight does not park it, and load shapes timing, never the outcome.

## Q6 — What is measured, and against what

"Which approach produces the best code" needs a referee. Everything below is
a fold over the record Q4 defines — the stream between `flight.opened` and
`flight.closed`, the items' notes, the trunk — and none of it is a number a
seat reports about itself. The reference's scoreboard is the shape: a key, a
stated derivation, a delta per landing rather than a standing total, and a
drift alarm over a week. **The rule that is the substance of this question:**
a flight is compared to another flight with the same pinned inputs, and an
experiment is a set of such flights that differ in exactly one input, repeated
enough times to see past sampling.

| Measure | Derivation | What it says about |
| --- | --- | --- |
| **escapes** | defects found after landing — an item filed against a landed item or its commit within a window, or the commit reverted — per landed item | the code |
| **returns per delivery** | `item.returned` over `item.delivered`, and findings per return | the builder against the spec |
| **spec corrections per delivery** | the delivery note's count, per item | the spec, which is the company's work |
| **decisions overruled** | the verdict's walk line, per delivery | the spec's gaps and the reviewer's read |
| **parks per flight, by reason** | `item.parked` grouped by question, cap, red gate, human gate | where the flight needed a person |
| **gate reds on untouched arms** | `gate.read` red where the diff did not reach, and how many parked after the rerun | the suite and the machine |
| **cost per landed item** | `session.retired` tokens summed per item, over landed items; and per flight | the price |
| **time per item** | dispatch to landed, wall clock; and the flight's | the schedule, read beside the overlap and load columns |
| **size** | the review's size line per delivery | the normaliser for every ratio above |

**Ruled.** Q6a — the referee is escapes per landed item: defects found after
landing, per landed item, is the primary measure of "best code", and cost and
returns are the price paid for it. Q6b — the record does not know what an
experiment is: `fly` writes the per-flight summary file, a person groups and
compares them by hand, and the flight carries no experiment or arm field. Q6c —
an escape is an item of type bug that traces to the landed item or its commit
within a policy window, fourteen days by default, or the commit reverted; the
trace runs through the work graph and never through a diff over a file, so
follow-up commits that tidy around a feature and the task and chore items a
review files off a landing are not escapes, a follow-up commit with no item
behind it is invisible to the fold, and a project that does not hold the rule
that every commit references its item under-counts its escapes. Q6d — the
log-and-stats PRD owns the mechanism: this page lists the flight measures and
their derivations, and how and where they are computed — the tick, a verb, or
the cockpit — is that page's.

## Q7 — The company

Once no named seat flies, the named seats are the company, and their job is
everything a flight cannot do because it is a function of its inputs: making
the inputs. **The rule that is the substance of this question:** the company
works before takeoff and after landing, never in between, and a named seat is
given work by a person or an order and never by a flight.

| Seat | Before takeoff | After landing |
| --- | --- | --- |
| **Architects** | design sittings and PRDs; specs to the zero-clarification bar; the worth and readiness passes on each other's specs; composing flights onto the backlog | reading the morning: the open gates, the parks, the escapes; the corrections review with the person; filing what the record shows |
| **Named builders** | on request: pairing on a spec, explaining a part of the tree | on request: a code review beside the person, a reading of what landed |
| **Back office** | the intakes and the syncs, fired by orders, walked as formulas | the same |
| **The person** | rules; orders; the taste gate on anything a user would see | answers gates; reverses rulings; reads the summary |

**What this reverses.** The reference's standing rules that a flight under
the claim no longer needs, each named so the reversal is deliberate:

- *whoever closes an item lands its work* — the flight lands (Q2c);
- *an architect operates the run and the two alternate* — there is no
  operator (Q2a);
- *the operator reviews everything on the list* — review is a policy and a
  spawned seat (Q2b);
- *the other architect's pass blocks dispatch on a live session* — the pass
  is an input, written before takeoff, and `fly` refuses an item without it
  (Q1, Q7c);
- *a builder rings its reviewer; a blocking question routes to a live
  session* — events and gates (Q3, Q4);
- *a resume plan mid-flight, a successor woken into it* — nothing in a flight
  survives past its seat's retire (Q2, Q3);
- *the flight report page* — the record is the report (Q4d);
- *the supervised lane: the person reviews on device before merge* — ruled
  below as a human gate.

**What it keeps.** Work is given, never taken: `fly` gives. A release and a
production write stay the person's: the guards. The record is the record.

**Ruled.** Q7a — the supervised lane is a human gate after delivery: the builder
delivers, the item parks at a gate carrying its review artefacts, the person
resolves it from the cockpit or the graph, and the next flight lands it, the
on-device review staying the taste gate and using the same gate object as every
other question. Q7b — the pre-takeoff pass is a pack's opinion, never core's:
core's `fly` refuses only an item the work graph does not call ready, and a pack
adds a pass — worth, readiness, a cross-read by an architect who is not the
author — as a readiness condition through policy, the fleet's own pack carrying
ours. Q7c — the morning is a pack's ritual: core offers gates and their listing,
and who reads them first — an architect resolving from the record and leaving
the person the rest, or the person alone — is an order and a skill in a pack.
Together: the company's rules are opinions about how a person wants to work and
every one of them lives in a pack, while core ships the seams they hang on —
readiness the graph computes, gates the graph holds, orders the controller
fires, and a flight that is a function of its inputs.

---

## The solution

Every decision below, and every requirement after them, carries one of three
marks: **constant** — core's, and hard to undo, kept only where it is what makes
a flight a function of its inputs; **key** — a policy value with a default,
changed in `fleet.toml` or `project.toml`; **pack** — an opinion a team installs
or leaves out.

### The claim, as a mechanism

**Re-pointed 2026-09-17 (workflows-formula-fate, fleet-layers.md Q10 and § What
moves).** The flight-as-a-function-of-its-inputs claim now stands on the run
lifecycle: `fleet run` pins every input into `runs/<id>/`, hashes the directory
onto the record and onto `run.started`, and a re-run executes the pinned bundle
and nothing else — a directory whose hash moved is refused rather than run.
`fleet plan`, `fleet fly`, `fleet autopilot` and the advance below left core on
that date; composition is tiny's preboard and takeoff workflows over `fleet
run`. The mechanism as it was built is kept below as the record.

A flight is a record the controller's tick advances. `fly` pins the inputs
into the flight directory and the flight's record item, writes
`flight.opened`, and returns. Every tick reads the record — the stream since
`flight.opened` and the items' notes — derives each item's state, and takes at
most one act per item: dispatch, retire, review, land, return, park, close.
No fact about a flight lives in a process's memory, so a crash, a controller
restart or a binary upgrade resumes by reading.

### S1 — The engine

**Removed 2026-09-17 (workflows-formula-fate).** The engine — the tick's advance
and `fly`'s foreground loop — left core with the three verbs; the run lifecycle
is what re-runs a run (controller PRD R35–R37). Kept as the record.

**The rule.** The flight's state is derived from the record on every tick and
held nowhere else.

**Ruled.** S1a — the engine is the tick over the record, a constant: `fly` pins
and opens, then returns, and each tick derives every open flight's state and
takes one act per item. S1b — with no controller running, `fly` runs the same
advance loop in the foreground, ticking on the policy interval until the flight
closes, progress on stderr, identical code to the tick so the two never disagree
and the embedded fleet flies on day one without a service. S1c — the flight's
record item is one work item of type task carrying the label `flight`, titled by
the flight id, its metadata carrying the directory hash and the summary, closed
at `flight.closed`, with no store configuration touched.

### S2 — An item's life inside a flight

The table below is the life core's **default formula** gives an item — the
four verbs as steps — and it is the life every item has until a pack or a
project shadows that formula (S6). Under a shadowed formula the states between
*dispatched* and *delivered* are the formula's own steps, each one event and
one note line; the states from *delivered* on are unchanged, because `deliver`,
`review` and `land` are the steps every formula ends with.

| State | Entered by | The tick's next act | Mark |
| --- | --- | --- | --- |
| **listed** | `flight.opened` | dispatch a fresh seat when the load belt and the flight's seat cap allow; refused → **held**, retried next tick in list order | cap: key `[core.flight] max_seats`; belt: key |
| **dispatched** | `item.dispatched` | wait for the seat's terminal event: `item.delivered`, or a gate the seat raised | constant |
| **delivered** | `item.delivered` | retire the builder; then by review policy: `spawned` → spawn a reviewer at the delivered commit; `none` → enqueue the landing | key `[core.flight] review`, per-item override |
| **reviewing** | `session.spawned` for the reviewer | wait for `item.reviewed` or `item.returned` | constant |
| **accepted** | `item.reviewed` ACCEPTED | retire the reviewer; enqueue the landing on the project's lane | constant |
| **returned** | `item.returned` | retire the reviewer; under the cap, dispatch a fresh builder from the delivered commit with the findings in its brief; at the cap, park with the last findings as the question | cap: key `[core] max_returns` |
| **landing** | the lane took it | `land`; green → **landed**; red on an arm the diff did not reach → one rerun, then park with both readings | rerun count: constant, one (Q3c) |
| **gated** | the item declares a human gate at this step | park at once: gate on the item, `item.parked`; the flight moves on | per-item input |
| **crashed** | `session.crashed` for the item's seat, after `item.dispatched` and before a terminal item event | retire the dead seat; under `[core.flight] max_crashes`, dispatch a fresh builder whose worktree is cut from the branch's last commit (the base when the branch holds none), its brief carrying the crash and what the branch holds; at the cap, park with the crash readings as the question (ruled 2026-09-12) | cap: key `[core.flight] max_crashes`, default 2 |
| **landed / parked** | `item.landed` / `item.parked` | nothing; the flight closes when every item is one of these | constant |

**The rule.** One act per item per tick, chosen from the record and never
from memory; a returned item goes to a fresh seat, and every seat inside a
flight is retired by the flight, never by itself.

**Ruled.** S2a — a return dispatches a fresh seat from the delivered commit, a
constant: the new seat's worktree is cut from the delivery commit, its brief
carries the item, the delivery note and the numbered findings, and the resume
commit is on the record. S2b — review policy is the key `[core.flight] review`,
default `spawned`, with a per-item override; `none` and `spawned` are built in
the first slice, and `self` — the builder's own read against the item before it
delivers, the verdict written by the same seat — lands after the first gate. S2c
— an item's overrides live in one metadata object on the item, `flight.*`,
written before takeoff, copied into the flight's inputs file at takeoff and
never inherited from a parent, so an epic cannot change a child's review
silently; the seam is constant, the values keys. S2d — the flight retires every
seat, a constant: the tick sees `item.delivered` or `item.reviewed`, calls the
controller's retire, which verifies from outside that nothing of the seat's
holds memory or disk, and the controller then reads the transcript and writes
`session.retired` with the cost.

**Defaults by rule.** `[[core.flight.rules]]` in policy is an ordered list;
each rule matches on the item's own type and labels — never a parent's — and
sets the `flight.*` values an item did not write (`formula`, `review`,
`gate`); the first match wins, the item's own object wins over any rule, and
the result is computed into the inputs file at takeoff, so it is pinned and a
rule edited mid-flight reaches the next flight. `status` prints the table. The
other half of "how hard is this item reviewed" is read from the delivery, not
the item: the review step's depth by the diff — size tiers, a comment-only
change as the floor — is a pack's per-tier review at P1, the review step
reading the diff stat.

### S3 — The landing lane and the trunk

| Piece | Under the claim | Mark |
| --- | --- | --- |
| **the lane** | one queue per project; the tick serves one landing at a time, in arrival order, in a worktree the fleet owns with its own build cache | one at a time: constant (Q5a); the worktree's place: key |
| **the base** | the lane fetches first; a delivery behind the trunk is rebased by the lane onto the fresh tip, the suite runs on the rebased commit, and the base it landed on is recorded; a conflict parks the item with the conflict as the question | inside the `advance` strategy |
| **the suite** | the project's `[gates] suite`, or `suite: none` on the landing note | key |
| **a red on an untouched arm** | one rerun, then park with both readings | constant (Q3c) |
| **the strategy** | `advance`: rebase and land per delivery; `batch`: one takeoff commit, one landing at close; `pull-request`: a landing opens a request a person or a rule merges | key `[project] trunk`, default `advance`; an unbuilt value refused by name (Q1c) |
| **a hand-run `land`** | a person's `fleet land` queues on the same lane | constant |

**The rule.** One landing at a time per project, in a tree the fleet owns; a
delivery behind the trunk is moved by the lane and never by a person, and a
conflict is a park, not a wait.

**Ruled.** S3a — inside `advance`, a delivery behind the trunk is rebased by the
lane onto the trunk's tip, the suite runs on the rebased commit and the lane
lands it, recording both the rebased commit and the base, and a conflict parks
the item with the conflict as the question. S3b — one lock per project, taken by
`land` itself, a constant: every landing, the tick's or a person's, queues on
the same lock and prints that it is waiting. S3c — `advance` is built first, a
key; `batch` and `pull-request` are specified below with their record shape and
each lands as its own slice, and until then `fly` refuses the value by name. S3d
— the lane's worktree lives under the machine directory, a key with the default
`lanes/<project>/`: nobody else edits it, a person's checkout is never touched
by a flight, and its build cache is shared with no seat's and no scratch tree's.

**The two strategies not yet built, as the record will show them.** Under
`batch`, `fly` records one takeoff commit; every delivery is reviewed against
it; at close the lane squashes the accepted deliveries in list order onto the
trunk as one landing, runs the suite once, and a conflict inside the squash
parks the conflicting item and lands the rest. Under `pull-request`, a landing
opens a request on the project's host with the delivery's diff and the review's
verdict, `item.landed` fires when the host reports the merge, and the flight
closes on the request opened, not merged, so a flight never waits (Q3a). Both
need what this page does not design: the batch's conflict order and a host
adapter.

### S4 — Gates and parks

| Need | The object | Who writes it | Mark |
| --- | --- | --- | --- |
| **a builder's blocking question** | the seat commits what it has and runs `fleet ask`: the question with lettered options becomes a gate on the item, the branch and commit go on the item, `item.parked` on the stream | the seat | constant |
| **a return at the cap** | the same gate, the last findings as the question | the tick | cap: key |
| **a red the diff did not reach, twice** | the same gate, both readings as the question | `land` | constant |
| **a human gate at a step** | declared in the item's `flight.gate` before takeoff; the tick raises the gate when the step is reached | the tick | per-item input |
| **the listing** | the store's own gate list is the cockpit's decisions list and the morning's | the store | constant |
| **the answer** | `fleet answer`: the gate resolved and the letter written on the item, one act | a person, from the cli or the cockpit | constant |
| **the resume** | the item is ready again; the next flight's takeoff reads the park on it and cuts the seat's worktree from the parked commit, the brief carrying question and answer; an item parked with an accepted verdict goes straight to the lane | `fly` | constant |

**The rule.** A park is one gate on the item plus one event, whoever raised
it, and every kind of "needs a person" is that same object, so one listing
shows everything owed and one act answers any of them.

**Ruled.** S4a — the gate is the store's own, a constant: on the first store, an
ad-hoc gate of type human blocking the item, so the item leaves the ready set
until the gate is resolved and the gate list shows every open one. S4b — `fleet
answer <item> <letter> [--text]`, a constant, writes the answer on the item,
resolves the gate and writes `gate.resolved`, and the cockpit calls the same
verb. S4c — `fleet ask` is the seat's verb, run from inside its worktree: it
commits what the seat has, raises the gate carrying the question and its
lettered options, records the branch and the commit on the item and writes
`item.parked`, and it pairs with `answer`. S4d — a supervised item's gate sits
after review by default, a key with a per-item override whose value names the
step: `flight.gate = "review"` parks the item after an accepted review, so the
person's taste read arrives on a delivery a review already accepted and a return
never reaches them, and a team that wants the person first sets `review =
"none"` on the item so the gate sits after delivery.

### S5 — Plans and autopilot

**Removed 2026-09-17 (workflows-formula-fate).** `plan`, `fly` and `autopilot`
left core; the backlog, the switch file and `status`'s listing of them went with
them. What plans and opens work is tiny's preboard and takeoff workflows over
`fleet run`. Kept as the record.

| Piece | Under the claim | Mark |
| --- | --- | --- |
| **planning** | a flight is a list someone writes before takeoff: `fleet plan` files the record item with its item ids, nothing pinned yet; the backlog is the planned flights not yet open, oldest first | constant: core plans nothing |
| **opening** | `fleet fly [<flight>] [--seats M]` opens the named flight, or the oldest plan: pins every input into the directory and the record, refuses on an unpinnable input or an item the graph does not call ready, writes `flight.opened` | constant (Q1 rule 1) |
| **autopilot** | on: the tick opens the oldest plan while open flights are under the cap; off: nothing opens; the switch is one file | cap: key `[core.flight] max_open` |
| **the departure board** | a pack's view over the backlog, and its order that plans flights from the board | pack |
| **a fleet with no architect** | `fleet plan --ready N` writes a plan from the N oldest ready items at plan time; a hands-off fleet schedules it with one order | a person's order, or a pack's |

**The rule.** Core plans nothing and opens only what was planned. Every flight
was a list someone wrote before takeoff, which is what makes the list an input
rather than a query.

**Ruled.** S5a — `fleet plan <items...>`, then `fleet fly`, the split constant
and the names the naming page's: `plan` writes a planned flight, `fly` takes off
with the oldest plan or a named one, `status` lists the plans waiting, and there
is no `fleet flight` command — flight is the noun a person reads in prose and in
`status`, never a command word. S5b — one flight is open at a time out of the
box: the key `[core.flight] max_open`, default 1, and raising it records the
overlap on every flight's record. S5c — an empty backlog opens nothing, a
constant that core never picks: `fleet plan --ready N` exists for a hand or an
order, the ready query runs at plan time and never at takeoff, and `status`
prints that the backlog is empty. S5d — `--seats M` stays and `--items N` goes:
the key `[core.flight] max_seats`, with `--seats` as the per-flight override,
since the plan already says which items fly.

### S6 — The item's life is a formula, and the steps are the flexibility

**Reversed 2026-09-17.** S6a to S6c below are reversed on the record by the
workflows sitting's ruling `workflows-formula-fate` (fleet-layers.md Q10): the
formula parser, the default formula, `flight.formula`, the instantiation at
takeoff and the flight advance's step walking leave core, and a workflow is
code that runs under the run lifecycle. The `step.started` and `step.closed`
pair stays, re-homed on the run lifecycle; the item lifecycle's states in S2
are the verbs' and stay. The text below is kept as the record of what was
ruled and reversed.

**The rule.** An item's life inside a flight is a formula from the formulas
slot, and a step is one of two things: a spawned seat whose brief is the step,
or a script with an exit contract. Judgment is a seat, code is a script, and
both are steps.

**Ruled, then reversed (see above).** S6a — a step model in core over the formulas slot: the runner is
constant and the default formula a pack's, shadowable; core ships the default
formula in its own `formulas/` — `dispatch` → `build` → `deliver` → `review` →
`land`, which renders exactly S2's table, so a fleet that never touches it flies
today's flight — a project shadows it in its own `formulas/`, an item names a
formula in `flight.formula` and otherwise gets the default, the formula is
instantiated per item at takeoff — its text with the item's variables — and
stored in the flight directory beside the brief so it is an input (Q1), each
step writes one `step.started` and one `step.closed` event carrying the item,
the step id, the seat or the script and the outcome, plus one note line on the
item, and steps are never beads. S6b — the first slice reaches linear steps:
`needs`, a seat or a script per step, `[steps.check]` as the format has it, and
a human gate on a step as the same gate object as S4; fan-out discovered at run
time, a retry policy per step and non-human gates are P1, each its own slice,
and a formula that uses one of them is refused at takeoff by name.

The format is the one the slot already holds — bd's, which is Gas City's v1:
`[[steps]]` with `id`, `title`, `description`, `needs`, `[vars]`, `[steps.gate]`
— plus fleet's one addition, `run`, which names what executes the step. `run` is
`seat` with an optional role (the default: a spawned seat whose brief is the
step's text plus the item), or `script = "<path>"` with the exit contract 0
pass, 1 fail with the reason on stdout, 3 could not tell; a script step's path
is a pack or project file pinned by the lock.
S6c, reversed with the two above — a seat step's `run` may also carry `model`
and `provider`, optional, the role's
policy row the default: pinned by the instantiated formula the flight
directory already holds, read once at spawn, and shown as the effective model
per step on `status` and on the flight's record, never the role's.

### The flight directory

```
flights/<id>/
  inputs.toml         the list; per item its title-description-acceptance hash and its flight.* object;
                      the trunk commit and the strategy; the lock's rows; the agent's pinned and live
                      versions; the model per role; the review policy in force; the account and its
                      usage reading; the load at takeoff; the takeoff stamp; the two named variances
  policy.toml         the effective policy at takeoff
  formulas/<item>.toml         the item's formula as instantiated at takeoff (S6)
  briefs/<item>.md    the brief the first seat read
  briefs/<item>.<n>.md         a re-dispatch's brief, findings included
  briefs/<item>.review.md      the reviewer's brief
  summary.json        at close: derived, marked so, carrying the stream sequence range it folded
```

The flight id is the record item's id. The directory's hash — over
`inputs.toml`, `policy.toml` and the briefs present at takeoff — goes on the
record item's metadata and on `flight.opened`; briefs a re-dispatch adds after
takeoff are derived from pinned inputs plus findings on the record, and the
summary hashes them separately.

### The events

Writers as Q4a ruled: each verb writes its note on the item and one typed
event, and each fact has one writer.

| Event | Written by | Carries |
| --- | --- | --- |
| `flight.planned` | `plan` — removed 2026-09-17 (workflows-formula-fate); nothing writes it | the flight id and its list |
| `flight.opened` | `fly` — removed 2026-09-17 (workflows-formula-fate); nothing writes it | the id, the directory hash, the overlapping open flights |
| `run.started` | `run` | the run id, the run directory's hash, the workflow's name |
| `run.closed` | `run` | the run id |
| `run.failed` | `run` | the run id, the reason the workflow's last line gave |
| `run.waiting` | `run` | the run id, the wake condition, the stream sequence at exit |
| `run.could_not_tell` | `run` | the run id, the exit code or null on a signal, the last line as read |
| `run.cleaned` | the tick | the run id and how many seats it spawned were retired |
| `item.dispatched` | `dispatch` | the item, the seat, the base commit, which dispatch this is |
| `item.held` | the tick | the item and the belt's reading |
| `item.delivered` | `deliver` | the item and the commit |
| `item.reviewed` | `review` | the item, the verdict, the findings count |
| `item.returned` | `review` | the item and which return this is |
| `item.landed` | `land` | the item, the trunk commit, the base, the rebased commit when one was made |
| `item.parked` | `ask`, the tick or `land` | the item, the reason, the branch, the commit, the gate |
| `gate.read` | `land` | the item, the suite, red or green, which reading |
| `gate.resolved` | `answer` | the item, the gate, the letter |
| `session.retired` | the controller | the seat, the item, context tokens, turns, wall time |
| `step.started`, `step.closed` | the run lifecycle | the item, the step id, the run, the outcome — re-homed on the run lifecycle when S6 was reversed |
| `flight.closed` | the advance — removed 2026-09-17 (workflows-formula-fate); nothing writes it | the id, landed, parked, returned, cost, wall time |

Core never opens the stream file: it writes through an event seam the cli
wires to the controller's writer, the same way `dispatch` reaches the
controller's spawn. In the foreground loop `fly` wires the same writer.

### The seat's isolation, as measured

Q1e ruled each spawned seat starts with its own empty configuration directory.
On the first provider that directory is `CLAUDE_CONFIG_DIR`, and it carries a
measured trap: the credential store's service name takes a suffix derived from
that directory, so a scratch directory is logged out and every first turn under
it fails. The escape is a second knob, `CLAUDE_SECURESTORAGE_CONFIG_DIR`
defined but empty, which restores the default credential while the daemon
stays scoped. The adapter sets both on every spawned start; the substrate pin
records whether the pair still isolates, measured at every upgrade rather than
trusted; a first turn that answers logged out is a dispatch failure — the seat
retired, the item held, `dispatch.failed` on the stream — and three in a row
halt the flight's dispatching as the controller's blind counter halts a seat's.

## Goals

1. **A flight is a function of its inputs.** Every input pinned before the
   first dispatch, none changed while the flight is open, and the two that
   cannot be pinned named as variance (Q1). *Re-pointed 2026-09-17
   (workflows-formula-fate): the run lifecycle gives this — `fleet run` pins
   and hashes, and a re-run executes only the pinned bundle.*
2. **No person and no persistent seat inside a flight.** Two spawned roles,
   building and reviewing; every other role a verb; composing, judging and
   answering moved before takeoff (Q2).
3. **A flight never waits.** A park, a close, a gate resolved from outside, a
   resume in the next flight from the parked branch (Q3).
4. **One record, three stores, one writer per fact,** and the report a fold
   over it, with cost a fact for the first time (Q4).
5. **The machine scheduled or recorded.** One landing lane per project, the
   load belt pinned, overlap on the record (Q5).
6. **A referee.** Escapes per landed item, over flights of the same pinned
   inputs (Q6).
7. **Opinions in packs.** Review policy, the pre-takeoff pass, the morning's
   triage and the supervised lane's gate are configurable, and core ships the
   seams (Q2b, Q7).

## Non-goals

- **Gas City's `graph.v2` compiler.** Its constructs — drains, scopes,
  teardown, output-driven fan-out — are not core's; fleet runs an item's
  formula with its own step runner (S6).
- **The cockpit's Flights view.** It reads the record this page defines and is
  the cockpit PRD's.
- **The reference's flights.** They are the problem statement's evidence and
  are not migrated; they are replaced.

Directions and rulings: `../adrs/flights.md`.

## Requirements

**R** = distilled from the reference; **T** = measured against Gas City;
**N** = new with fleet. Each line ends with its mark: **constant**, **key**
or **pack**.

### P0 — the first gate: one planned flight, flown to its landings by the tick alone

**The record and the inputs**

1. `fleet plan <items...>` files the flight's record item — type task, label
   `flight`, titled by its id — with the list in its metadata, writes
   `flight.planned`, and pins nothing; `--ready N` takes the N oldest ready
   items at plan time. N. constant.
2. `fleet fly [<flight>] [--seats M]` opens the named flight or the oldest
   plan: writes `flights/<id>/inputs.toml`, `policy.toml` and a brief per item,
   hashes the directory onto the record item and `flight.opened`, and refuses
   before the first dispatch on any input it cannot read, naming it, and on any
   item the work graph does not call ready (Q1 rule 1). R for the pins; N for
   the refusal. constant.
3. No input changes while a flight is open: the policy in force is the
   snapshot, the pack is the lock recorded, the trunk advances per landing with
   every base recorded (Q1 rule 2). N. constant.
4. An item's overrides live in one metadata object `flight.*` — `formula`,
   `review`, `gate` — written before takeoff, copied into `inputs.toml`, never
   inherited from a parent; `[[core.flight.rules]]` fills the values an item
   did not write, matched on the item's own type and labels, first match wins,
   computed at takeoff. N. constant for the seam; the values and the rules
   keys.
5. The trunk strategy is `[project] trunk`, default `advance`; `fly` refuses a
   value it does not implement by name. N. key.

**The engine**

6. The controller's tick calls core's advance for every open flight; the
   advance derives each item's state from the stream since `flight.opened` and
   the items' notes, takes at most one act per item, and writes the act's
   event before returning. N. constant.
7. With no controller running, `fleet fly` runs the same advance in the
   foreground on the policy interval until the flight closes, progress on
   stderr. N. constant.
8. A flight closes when every item on its list is landed or parked; `fly`
   writes `flight.closed`, closes the record item with the summary line, and
   writes `summary.json` into the directory, derived, marked so, carrying the
   sequence range it folded. R for the shape; N for the fold. constant.
9. A dispatch the load belt refuses is held, `item.held` written with the
   reading, and retried on the next tick in list order (Q5c). R. key for the
   belt's numbers.

**The seats**

10. Every seat inside a flight is spawned by the flight through the
    controller's spawn and retired by the flight through the controller's
    retire after its terminal event; a seat never retires itself. R. constant.
11. A return dispatches a fresh builder whose worktree is cut from the
    delivered commit and whose brief carries the item, the delivery note and
    the numbered findings, counting against `[core] max_returns`; at the cap
    the item parks with the last findings as the question. R. constant;
    the cap a key.
12. The controller writes `session.retired` at retire with context tokens,
    turns and wall time, read through the adapter's transcript verb, keyed to
    the seat and its item. N. constant.
13. Each spawned seat starts with its own configuration directory holding only
    the pack's overlay; on the first provider the adapter sets the two knobs
    the isolation section names, the pin records whether the pair still
    isolates, and a logged-out first turn is a dispatch failure that holds
    the item and halts dispatching after three in a row. N. constant.

**Review**

14. `[core.flight] review` — `none`: an accepted delivery goes to the lane on
    the builder's word; `spawned`: a reviewer seat per delivery, born from the
    pack's review skill, the item, the delivery note and the diff, in a
    worktree cut at the delivered commit, blind to the builder, its verdict on
    the item, retired by the flight. `self` is P1. R for the reader; N for the
    policy. key.

**The lane**

15. One landing lane per project: a lock under the machine directory that
    `land` itself takes, so the tick's landings and a person's queue alike; a
    waiting `land` prints so. R for the gates; N for the lane. constant.
16. The lane's worktree is `lanes/<project>/` under the machine directory by
    default, cut from the trunk, with its own build cache. N. key.
17. Under `advance`, a delivery behind the trunk is rebased by the lane onto
    the fresh tip, the suite runs on the rebased commit, both commits go on
    the landing note and `item.landed`; a conflict parks the item with the
    conflict as the question. N. constant inside the strategy.
18. A gate red on an arm the diff did not reach is rerun once, both readings
    written as `gate.read`; a second red parks the item with both readings as
    the question. R. constant.

**Gates**

19. A park is one gate on the item — the store's own gate object, type human,
    blocking the item — plus `item.parked` carrying the reason, the branch,
    the commit and the gate; the flight goes on and never reopens for it.
    R for the park; T for the gate object. constant.
20. `fleet ask --note <file>`, from inside a seat's worktree, commits what the
    seat has, raises the gate with the note's question and lettered options,
    records the branch and commit on the item, writes `item.parked`, and
    exits; the flight retires the seat. N. constant.
21. `fleet answer <item> <letter> [--text <text>]` writes the answer on the
    item, resolves the gate and writes `gate.resolved`; the store's gate list
    is the decisions list. N. constant.
22. A parked item that is ready again is taken by the next flight that lists
    it: its seat's worktree is cut from the parked commit and its brief carries
    the question and the answer; a parked item carrying an accepted verdict
    goes to the lane without a new seat. N. constant.
23. `flight.gate` on an item names the step after which the item parks for a
    person — `review` by default for a supervised item, `delivery` when the
    item's review is `none` — and the gate raised there carries the review
    artefacts the pack's template names. R for the taste gate; N for the
    object. key with the per-item override.

**Autopilot**

24. `fleet autopilot on|off` is one file; on, the tick opens the oldest plan
    while open flights are under `[core.flight] max_open`, default 1; off,
    nothing opens; an empty backlog opens nothing and `status` says so.
    R, reduced. key for the cap.
25. Every flight's record names the open flights that overlapped it. N.
    constant.
26. Core ships no order that plans or flies; a person or a pack writes one
    whose action calls `plan --ready` or `fly` by name. N. pack.

**Measurement**

27. The flight measures of Q6 are derivations over the stream and the notes,
    listed on this page and computed where the log-and-stats page rules;
    `summary.json` is the per-flight fold a person compares by hand. N. key
    for the escape window, `[core.flight] escape_window_days`, default 14.

**The item's formula (S6)**

28. An item's life is a formula from the formulas slot in the format the slot
    holds, plus `run` — `seat` with an optional role, or `script = "<path>"`
    with the exit contract 0 pass, 1 fail with the reason on stdout, 3 could
    not tell; core ships the default formula whose steps are the four verbs,
    a project's `formulas/` shadows it, and `flight.formula` on an item names
    one. T for the format; N for `run`. constant for the runner; the default
    a pack's.
29. `fly` instantiates each item's formula at takeoff into
    `flights/<id>/formulas/<item>.toml` and hashes it with the rest; a formula
    naming a construct the slice does not implement is refused at takeoff by
    name. N. constant.
30. The advance walks an item's steps by `needs`: a seat step spawns a seat
    whose brief is the step and the item and retires it at its close; a script
    step runs the script in the item's worktree with the item and the flight on
    its environment and reads its exit; each step writes `step.started` and
    `step.closed` and one note line; a step is never a bead. N. constant.
31. A `[steps.check]` runs after its step as the format defines, up to its
    attempts; exhaustion is a return finding on the item. T. constant.
32. A `[steps.gate]` of type human on a step parks the item at that step with
    the gate object of S4; the resume continues from the step after it. T for
    the field; N for the park. constant.
33. Fan-out at run time, a per-step retry policy and non-human gate types are
    P1; a formula using them is refused by R29 until each lands. N.

**The crashed seat (S2, ruled 2026-09-12)**

34. On `session.crashed` for a dispatched item's seat, the tick retires the
    dead seat and, under `[core.flight] max_crashes` (default 2), dispatches a
    fresh builder whose worktree is cut from the branch's last commit — the
    base when the branch holds none — with the crash and the branch's state in
    its brief, each attempt on the record as its own `item.dispatched`; at the
    cap the item parks with the crash readings as the question. R for the
    resume shape; N for the cap. constant for the rule; the cap a key.

### P1 — after the first gate

- `review = "self"`: the builder's own read against the item before it
  delivers, the verdict written by the same seat.
- The formula's next constructs, each a slice: fan-out discovered at run time
  (the pinned list reopened for the expanded set), a retry policy per step,
  non-human gates through the store's own gate check on the tick.
- The `batch` and `pull-request` trunk strategies, each its own slice, with
  the batch's conflict order and a host adapter designed first.
- The escape fold and the drift alarm, in the log-and-stats page's mechanism.
- A pack's readiness condition on `fly` — the worth and readiness passes as
  policy (Q7b).

### P2 — designed for, not built

- A second provider's isolation knobs, proving the seam.
- Experiments as a person's grouping of summary files; nothing on the flight.

## The first slice — the first build

**"One planned flight of two items, flown to two landings by the tick
alone, under core's default formula; then one item under a shadowed formula
with a script step."** On a scratch project with a suite: `fleet plan a b` files the
record; `fleet fly` pins the inputs into the directory and opens; the tick
dispatches two builders, retires each at delivery, spawns two reviewers at the
delivered commits, retires each at its verdict, lands both on the lane in
order with the second rebased onto the first, closes the flight and writes
`summary.json`; `session.retired` carries a cost for all four seats. Then the
same on a third item whose builder runs `fleet ask`: the item parks, the
flight closes without it, `fleet answer` resolves it, and a second planned
flight lands it from the parked commit with the answer in its brief. Every
number in the success metrics is read from that run's stream.

## Success metrics

| Metric | The reference today | Target at the first gate |
| --- | --- | --- |
| named seats on a flight's roster | one operator, sometimes two | zero |
| messages to a person or a named seat during a flight | the delivery ring, the pass ring, the hold | zero; every need is a gate |
| tokens read by anyone before a flight's first act | 117–121K per operator session | zero: the tick reads the record |
| a flight's record | notes, a manifest, two diaries, a log, messages | three stores and one summary file |
| cost per landed item | not captured | on every `session.retired`, summed in `summary.json` |
| landings that ran beside another suite | seven gates for three landings on one night | zero: one lane |
| minutes from `fleet plan` to the first landed item, no person present | — | under sixty on the scratch project |

## What this page changes elsewhere

- The packs page's § Flight and autopilot and its requirement 11 are
  rewritten to this page's `plan` and `fly`.
- The cli page's `fleet fly` and `fleet autopilot` entries are rewritten;
  `fleet plan`, `fleet ask` and `fleet answer` are added; the families table
  gains the three; `status` gains the plans waiting.
- The naming page gains a row for every name this page introduces, and a row
  banning `flight` as a command word.
