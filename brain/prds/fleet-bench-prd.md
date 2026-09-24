**Unbuilt, and requirement 6 rests on what no longer exists.** Requirement 6 makes a bench run a flight, and it rests on `fleet fly` and formulas, which no longer exist; the flights and packs PRDs this page cites are archived under `brain/archive/prds/`.

# PRD: Fleet-Bench

**One sentence:** Fleet-Bench is a benchmark of *model × formula* on a
generated project, run by anyone with `fleet bench`, scored from the diff alone
by a public results repository's CI, and published as one page that shows which
combination produces the best code for the least time and tokens.

Drafted 2026-09-13, from a sitting that started with a different question —
which public benchmarks measure code quality — and ended by refusing every
answer that measured a real project. The reference fleet's own record is a
better benchmark of *its* code than anything published, and that is exactly why
it is not this: a page about fleet has to measure fleet on ground nobody owns.
The flights PRD (`fleet-flights-prd.md`) is the machinery a bench run rides on;
the packs PRD (`fleet-packs-prd.md`) is where a formula comes from.

**Status: ruled 2026-09-13.** Eight decisions, listed at the end with the
alternatives each one refused.

---

## The solution in one page

**What is measured.** A cell is one model and one formula. Fleet flies a task
under that cell inside a project the bench generated from a seed, and the
result is scored nine ways from the delivered diff: did the hidden tests pass,
how much code it took against a reference solution, how far outside the needed
hunks it reached, how much dead code it left, whether the tests it added catch
deliberate breaks, whether it kept the project's own guidelines, what it did to
lint and type strictness, and what it cost in tokens and in wall clock. No
composite. The page shows the columns and a chart of quality against cost.

**What it is measured on.** Not a real project. A generator emits a small, real,
buildable project from a seed — its modules, its tests, its lint config and its
own short guidelines file, which is the only doctrine the run sees — and the
seed varies names, layout and ordering so a published variant is never the one
a model is run on. Three kinds of task, each a language and a size: **shard**
(TypeScript, one file, ten minutes), **chunk** (Python, two modules, thirty
minutes), **slab** (Rust, a change that propagates through a workspace's type
system, ninety minutes). Slab is built so that a lesser model fails to compile,
runs out its limit, or takes the obvious wrong turn and delivers four times the
reference.

**Who runs it.** Anyone with Docker. `fleet bench run` pulls one image that
carries the toolchains, the scorer and the agent runtimes at pinned versions,
regenerates the variant inside it with a hermetic machine directory and board,
times one build and one visible-test pass of it as the machine's **calibration
unit**, flies it under the cell with the limit enforced by a kill, and writes a
result bundle. The contributor's credential enters the container as one
environment variable and nothing from the host is mounted. Every wall clock
and every limit is expressed in that unit, so a slow machine is not a failed
run. `fleet bench score` recomputes
every metric from the bundle's diff. `fleet bench submit` opens a pull request
against a public results repository whose CI regenerates the variant from the
seed, applies the diff, runs the hidden tests and rescores. A contributor can
claim any model wrote a diff; they cannot claim what the diff scores. The page
takes the median of a cell's independent submissions and shows the count.

**What the page proves.** Every cell that reaches three runs, the raw agent
with no formula as its own row, timeouts shown as failures with the tokens they
burned, and a methodology page naming the generator version, the seeds, the
fleet version, the scoring code and the sentence about what a contribution can
and cannot prove.

---

## Problem statement

Fleet's claim is that the *shape* of how agents build — the brief, the gate,
the review, the return loop — changes what lands, not only which model is
behind it. Public benchmarks cannot test that claim: SWE-bench, Terminal-Bench,
LiveCodeBench and their siblings rank models on pass rate and say nothing about
the harness around them or the shape of the diff. Nothing ranks formulas.

Running the comparison on a real project fails for a different reason. A real
repository carries doctrine, history and taste that belong to its owner; a
score against it measures the owner's conventions as much as the cell, and a
reader of the page cannot reproduce it. The one honest ground is a project the
benchmark owns and regenerates.

And the cost of a full comparison — thirty tasks, three runs, a handful of
cells — is a low four-figure sum in tokens per edition. One team cannot keep a
page current on its own. The benchmark has to be runnable by its users, and a
result from a stranger has to be worth showing.

## Goals

1. A page that answers, per cell, *how good, how fast, how much*, and is
   believed by a reader who has run coding agents.
2. A benchmark that ranks formulas, which no public benchmark does.
3. Ground nobody owns: a generated, seeded, versioned project, so a variant is
   fresh to every model and a number can be regenerated by anyone.
4. Community-run: `fleet bench` is a verb any fleet user has, and a submission
   costs a pull request.
5. Scores that need no model's opinion: hidden tests and static measures over
   the diff carry every ranking.
6. Hermetic by construction: a bench run can never reach the fleet instance
   that is running it, nor any real board.

## Non-goals

- **Measuring a real project.** Not the reference product, not fleet itself. Whether fleet
  should let a user measure their own setup over time is a separate product
  question and is not this page.
- **A single score.** No composite; the columns and the chart are the answer.
- **Proving who ran a model.** CI proves the diff's score; the model claim is
  the contributor's, shown as such.
- **Judging with a model.** A model-as-judge column may exist as a report; it
  never ranks.
- **Hiding hardware.** Wall clock is reported in the machine's calibration
  unit and in seconds, and the bundle names the machine; the page never shows a
  time without the unit it was measured in.

## Recorded directions

Read from the sitting, verbatim where the words decide something.

- *"the goal is not to measure this against [the reference product] or fleet."* The ground is
  generated.
- *"i would probably want to generate a fake repo to actually run the tests
  in."* The generator, seeded and versioned.
- *"the end-goal for these benchmarks would be to have page on our product
  website with benchmarks showing which combination of models and formulas
  produce the best code, for the least amount of time and tokens."* The three
  axes of the page: quality, time, tokens.
- *"allow other users to contribute to the benchmarks instead of us running all
  of them by ourselves."* The verb, the bundle, the results repository.
- *"the benchmark has three kinds of tests ranging in size of change and
  difficulty: shard (typescript), chunk (python), slab (rust)."* The kinds and
  their languages are fixed; their details are this page's.
- *"the slab should be difficult enough that a lesser model would either never
  be able to complete it, it would take a really long time, or it would write a
  lot of code to do."* Slab's three properties below.
- *"the run has to have a time limit, if a model cant complete it before it
  expires it fails the test."* A kill, not an honor system.

---

## Design overview

### The verb

`fleet bench` is a subcommand of the cli, beside `plan` and `fly`, and a bench
run *is* a flight: what is measured is exactly what a user runs, because the
bench composes the same plan, the same formula slot and the same tick.

```
fleet bench list                                  the kinds, their tasks and limits
fleet bench run --kind K --model M --formula F    one cell, one task set; writes a bundle
                [--task T] [--seed N] [--runs R]
fleet bench score <bundle>                        recompute every metric from the diff
fleet bench submit <bundle>                       open the results-repo pull request
```

`run` regenerates the variant from its seed into a temporary directory, points
`FLEET_DIR` at a temporary machine directory and the project at a temporary
board on its own store, flies the task under the cell, enforces the kind's
limit, and writes the bundle whatever the outcome. It never reads the machine's
own fleet directory, its own board, or any project outside the temporary root:
the hermetic rig every fleet suite runs under is the same one the bench runs
under, and a run that finds itself outside it refuses before the first spawn.

### The container

The contributor path is one image, published per edition and named by digest:
the three toolchains, the linters and analysers, the scoring code, and each
supported agent runtime, all at pinned versions, with a non-root user for the
agent. `fleet bench run` pulls it and runs the whole of a run inside it —
generation, calibration, the flight, scoring — so a number computed on a laptop
and a number computed by the results repository's CI come from the same bytes,
and the host's machine directory, board, process table and filesystem are out
of reach by construction.

Nothing from the host is mounted. The credential enters as an environment
variable and the network reaches the model's API and nothing else. For the
Claude runtime the documented paths are the two the bench supports: a
subscription token, minted once on the host with `claude setup-token` and
passed as `CLAUDE_CODE_OAUTH_TOKEN`, or an API key as `ANTHROPIC_API_KEY`. The
runtime's bare mode does not read the subscription token, so the bench launches
the agent without it on a subscription run and with it on an API run; every
launch is non-interactive with prompts disabled, and the agent runs as the
image's non-root user because the runtime refuses the prompt-free flag as root.
Other runtimes carry their own headless credential; the rule is the same
variable-in, nothing-mounted shape for each.

A native run — outside the image — is allowed for maintainers and shows on the
page as provisional; only container-run bundles publish, so every published
number came from the same image. The bundle carries the image digest.

### The generator

One generator per kind, deterministic from `(generator version, seed)`. It
emits a project that builds and passes its own visible tests before any task is
applied: modules with a real dependency between them, a test suite, a lint and
type configuration at the strictest setting the language has, and a
`GUIDELINES.md` of under forty lines that is the whole of the doctrine a run
sees — how to name, where tests go, what is forbidden. The seed varies
identifiers, module layout, function order and the incidental data, never the
task's substance, so every variant is the same problem and none is a published
one.

The generator's version is part of every bundle and every published number. A
change to a kind's generator is a new edition of that kind; old results stay
under their version and are never compared across it.

### The task

A task is a directory, portable on its own:

```
<kind>/<task-id>/
  task.toml         kind, difficulty, limit, the constraint list, the reference's line count
  instruction.md    what the run is told: a behaviour, never a design
  hidden/           the acceptance tests the run never sees, applied at scoring
  reference/        the reference solution as a diff against the variant, written to GUIDELINES.md
  mutants/          deliberate breaks the run's own tests are expected to catch
```

The instruction states a behaviour and the constraints; it never names the
seam. Hidden tests are not mounted in the project during the run; they are
applied by `score` and by CI. The reference is the economy baseline: it is the
smallest diff the task's author could land under the guidelines, and every
run's size is read against it.

### The three kinds

| Kind | Project | The task | Reference | Limit |
|---|---|---|---|---|
| **shard** | TypeScript, one package, about 500 lines, vitest, eslint, strict types | One-file change: a feature in an existing module, or a planted defect with a misleading symptom. Constraint: no new dependency | about 30 lines | 10 min |
| **chunk** | Python, a package of four modules, about 2,500 lines, pytest, mypy strict, ruff | A feature that crosses two modules and needs a new test; a data shape changes at one boundary. Constraint: standard library only | about 120 lines | 30 min |
| **slab** | Rust workspace, three crates, about 7,000 lines, cargo test with property tests, clippy with warnings denied | A change that propagates through the type system: a new variant in a core state machine every crate must honour, with serialisation and a cancellation path, under an invariant only a hidden property test checks | about 200 lines | 90 min |

**Why slab separates models.** Three properties, by design and not by size.
The change propagates through types, so partial work does not compile and
there is no partial credit. The instruction states a behaviour, so the run has
to find the right seam; the wrong seam compiles, passes the visible tests and
fails the hidden invariant. And there is a deliberate trap — duplicating the
state machine — that passes everything visible while multiplying the overhead
ratio and failing the mutant set. A lesser model fails to compile and thrashes
to the limit, or takes the trap and delivers four times the reference.

**The calibration run.** Before the flight, on the regenerated variant and on
the same machine, the runner builds the project from clean and runs its visible
tests once, and records the seconds as this run's **unit**. Everything timed
afterwards is reported in units as well as seconds: a slab flight that took
5,400 seconds on a machine whose unit is 90 seconds reads *60 units*, and the
same flight on a machine whose unit is 180 reads *30 units* at 5,400 seconds.
The unit is measured, never entered, and a bundle without one does not score.
The calibration run is not the flight: it starts no agent and spends no tokens.

**The limit** is wall clock from the flight's dispatch to its delivery, held in
the task manifest **in units** and converted to seconds on the machine at hand
from its calibration run — the manifest's value is chosen so that it reads as
the kind's minutes on the reference machine (ten, thirty and ninety) — enforced
by the runner killing the flight. Expiry is a fail: pass is zero, and the
tokens, lines and time are still recorded, so a timeout reads *failed at N
tokens* and never vanishes. A machine's unit is capped at a manifest ceiling
above which the run is refused rather than stretched, because a limit of a
whole day is no limit.

### The score

Nine rows per run, every one computed from the bundle by `score`, none from
any model's opinion:

1. **Pass** — the hidden tests, binary; a timeout is 0.
2. **Overhead ratio** — the run's effective lines (no blanks, no comments)
   over the reference's.
3. **Parsimony** — lines changed outside the files and hunks the reference
   touched.
4. **Dead code** — functions, variables and branches the run added that
   nothing reaches, from the language's own analyser.
5. **Mutation score** — the share of the task's mutants the run's *added*
   tests catch; a run that added none scores 0 here, not blank.
6. **Guideline adherence** — the machine-checkable rules of the variant's
   `GUIDELINES.md` and the task's constraints, as a pass count over the list.
7. **Strictness delta** — lint and type warnings per hundred lines added,
   against the variant's own baseline.
8. **Tokens** — input and output, read from the run's own usage record.
9. **Wall clock** — dispatch to delivery, from the runner, in seconds and in
   the run's calibration units; the page ranks on units.

### The bundle

What `run` writes and `submit` sends: the cell (model id as the provider names
it, formula name and hash), the task and seed, the generator and fleet
versions, the image digest or the word `native`, the diff, a hash of the
transcript, the usage record, the test log,
the timings in seconds and in units with the calibration run's own seconds
beside them, the outcome (delivered, timed out, crashed), and one hardware
line — CPU, cores, memory, OS. No transcript, no credentials, no path from the
contributor's machine.

### The results repository and the page

One public repository, results only, open from day one. A submission is a
pull request adding one bundle under `results/<edition>/<kind>/<cell>/`. Its
CI regenerates the variant from the bundle's seed and generator version,
applies the diff, runs the hidden tests and the mutants, and rescores; the
recomputed rows are what merge, beside the contributor's. A bundle whose diff
does not apply, or whose claimed generator version is unknown, is refused with
the reason.

The page is a static build from the repository's main. Per kind: every cell
with three or more independent runs, showing pass rate with the run count,
timeouts, median tokens, median wall clock, median overhead ratio, the spread of
each, and the raw agent — the model with no formula — as its own row. One chart
per kind of pass rate against tokens. One methodology page: the generator
versions and seeds, the fleet version, the scoring code, and the sentence: *the
score is ours; the model is the contributor's word.*

---

## Requirements

**N** = new with the bench; **F** = a property the flights machinery already
has and the bench relies on. Each line ends with its mark: **constant**,
**key** (a policy value in configuration) or **pack**.

### P0 — the first edition: one kind runnable, scorable and submittable end to end

**The verb**

1. `fleet bench run --kind K --model M --formula F` flies one task set under
   one cell inside a regenerated variant and writes a bundle whatever the
   outcome. N. constant.
2. A run is hermetic: a temporary machine directory, a temporary board on its
   own store, a temporary project root; the runner refuses before the first
   spawn when any of the three resolves outside the temporary root. N.
   constant.
3. The limit is enforced by the runner killing the flight at the kind's
   value, read from the task manifest in units and converted through the run's
   calibration; expiry writes a bundle with outcome `timed-out`, pass 0, and
   the tokens, lines and time to that point. N. key.
3a. Before the flight the runner performs the calibration run — a clean build
   and one visible-test pass of the variant, no agent, no tokens — and writes
   its seconds to the bundle as the unit; a bundle without a unit does not
   score, and a unit above the manifest's ceiling refuses the run. N. key.
4. `fleet bench score <bundle>` recomputes the nine rows from the bundle
   alone, with no network and no model. N. constant.
5. `fleet bench submit <bundle>` opens the pull request; it refuses a bundle
   carrying a transcript, a credential or an absolute path. N. constant.
6. A bench run is a flight: the same plan, formula slot and tick as `fleet
   fly`, so the formula measured is the formula a user runs. F. constant.
6a. The image: one per edition, named by digest, carrying the toolchains, the
   analysers, the scorer and each supported agent runtime at pinned versions,
   with a non-root user for the agent; `fleet bench run` pulls it and runs
   generation, calibration, the flight and scoring inside it. N. constant.
6b. The credential enters the container as an environment variable and
   nothing from the host is mounted; the runner refuses a run that would
   mount a host path. For the Claude runtime, a subscription token in
   `CLAUDE_CODE_OAUTH_TOKEN` launches the agent without bare mode and an API
   key in `ANTHROPIC_API_KEY` launches it with bare mode; every launch is
   non-interactive with prompts disabled. N. constant.
6c. The container's network reaches the model's API and nothing else. N.
   key.

**The generator and the tasks**

7. One generator per kind, deterministic from generator version and seed; the
   emitted project builds and passes its visible tests before any task. N.
   constant.
8. The seed varies identifiers, layout, order and incidental data and never
   the task's substance. N. constant.
9. A task is the directory above; hidden tests are absent from the project
   during the run and applied at scoring. N. constant.
10. The reference is the smallest diff its author could land under the
    variant's guidelines; its line count is in the manifest. N. constant.
11. Shard ships first: at least fifteen tasks, half features and half planted
    defects, each with a reference and a mutant set. N. constant.

**The score**

12. The nine rows, computed as the design states, each from a named tool the
    scoring code pins by version. N. constant.
13. No composite is computed anywhere; the page shows rows. N. constant.

**The results repository and the page**

14. The repository is public from day one; CI rescores every submission by
    regenerating the variant, applying the diff and running the hidden tests
    and mutants; the recomputed rows are what merge. N. constant.
15. A cell publishes at three independent runs; the page shows the median,
    the spread and the count; below three it is empty. N. key.
15a. Every time on the page is shown in units with the seconds beside it, and
    ranking on time uses units. N. constant.
15b. Only bundles carrying an image digest publish; a `native` bundle shows as
    provisional and never enters a cell's median. N. constant.
16. The raw agent — the model with no formula — is a row on the public page.
    N. constant.
17. The methodology page names the generator versions and seeds, the fleet
    version, the scoring code and the trust sentence. N. constant.

### P1 — the full grid

18. Chunk and slab generators and task sets, slab carrying its three
    separating properties and a hidden invariant test per task. N. constant.
19. The full model × formula grid per kind, community-filled; every cell that
    reaches three runs publishes. N. constant.
20. A per-kind chart of pass rate against tokens, with the baseline row
    marked. N. constant.
21. Editions: a generator change is a new edition of its kind; results are
    never compared across editions. N. constant.
22. Resume: a run of R repetitions that is interrupted resumes from its
    bundles rather than restarting. N. constant.

### P2 — the report columns

23. A model-as-judge column, reported and never ranked. N. constant.
24. A machine-class column derived from the bundle's hardware line, shown
    beside wall clock. N. constant.

## Success metrics

- The first edition's shard page is up with at least three cells at three
  runs, one of them the raw-agent row, and a reader can regenerate any
  variant from the methodology page.
- A submission from a machine the maintainers never touched merges through CI
  with its rescored rows and no maintainer edit.
- Slab, once shipped, produces a spread: at least one cell times out or lands
  over three times the reference where another lands under 1.5.
- No published number was ever produced by a model's opinion.

## Decisions — ruled 2026-09-13

Each was a question with options; the chosen option is quoted, the refused ones
named.

- **D1 — location.** *"fleet/brain/prds/"* — beside the controller and flights
  PRDs, so the page travels with fleet. Refused: the reference product's own store.
- **D2 — submission trust.** *"Honor system, rescored diff shown."* The model
  and formula are the contributor's claim; the score is CI's; the page shows
  the count per cell and takes the median. Refused: a provider usage record
  required per submission; maintainer-run only at first.
- **D3 — the baseline row.** *"Yes, its own row."* The raw agent is on the
  public page. Refused: internal only.
- **D4 — runs per cell.** *"3."* Median with spread. Refused: five; one to
  publish as provisional.
- **D5 — the limits.** *"Keep 10 / 30 / 90."* Wall clock, a kill, expiry is a
  fail; re-set from the reference flight's median once slab exists. Refused:
  5 / 20 / 60; 15 / 45 / 120.
- **D6 — the grid.** *"The full grid, models × formulas."* Refused: models on
  one fixed formula; formulas on one fixed model.
- **D7 — the results repository.** *"Public from day one."* Refused: private
  until the first edition.
- **D8 — the name.** *"Fleet-Bench."* The verb is `fleet bench`; the kinds are
  shard, chunk and slab.
- **D9 — the runtime.** *"docker it is."* The contributor path is a container
  image; native runs are provisional; only container-run bundles publish.
  Refused: Nix as the contributor path, kept as a way the maintainers may build
  the image; native runs on the page.

## Open questions

- **Hardware and the limit — resolved by the calibration run** (this page's
  design, added the same day): times and limits are in the machine's own
  unit. What stays open is the unit's ceiling per kind, set from the first
  edition's spread.
- **The runtime — resolved, D9.** The container is the contributor path. What
  stays open is whether the maintainers build the image with Nix so one
  definition serves the developer environment and the image, decided when the
  first image is built.
- **Formula identity.** A formula is named and hashed in the bundle; whether
  the page shows a hash or a pack-qualified name is decided when the first
  non-default formula is submitted.
- **Who writes slab's tasks.** A task that separates models needs an author
  who can see the trap; whether that is one person or a reviewed contribution
  path is decided after shard's fifteen.
