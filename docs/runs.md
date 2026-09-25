# Runs and workflows

A workflow is a program a pack carries, and a run is one execution of it that
fleet records: `fleet run <workflow>` files a record item, pins what the run
is given into a run directory, runs the program, and writes how it ended to the
item and to the event stream. You meet runs when you fly a list of items with
the tiny pack's takeoff workflow, and whenever a pack of yours carries a
workflow. A run that asks a person a question, or waits on work, stops and is
carried on by the controller when the answer or the work arrives.

## Terms

- **workflow**: a file `workflows/<name>.<ext>` in an installed pack. The name
  is the file name without its extension, whatever language the file is in.
- **run**: one execution of a workflow, known by the id of its record item.
- **run record**: the item `fleet run` files for the run in the project's
  work graph. It carries the label `fleet:run`, its title is its own id, and
  it is open while the run can still move. An item labelled `run` and not
  `fleet:run` is not a run record.
- **run directory**: `runs/<run>/` under the machine directory, holding what
  the run is pinned to and what its process printed.
- **pinned inputs**: the `--input` pairs and the pack's settings, written into
  the run directory before the workflow starts. A run reads these and nothing
  else, however often it is executed again.
- **runtime table**: the `[runtime]` table in a pack's `pack.toml` that says
  which runtime a workflow is bundled and run with.
- **step**: one numbered unit of a workflow's work, recorded on the event
  stream as a `step.started` and `step.closed` pair.
- **wait**: a run that cannot go on yet exits waiting and names what it is
  waiting for. The controller executes it again when that could have arrived.
- **hold**: a question on the run's record for a person to answer, with a
  `held` entry on the record's timeline. The record is **held** while the
  hold stands, and a person's clearance clears it. The same object a seat
  raises with `fleet hold`; see [Items and the record](items.md).

## Running a workflow

You run a workflow from inside the project it works on:

```sh
$ fleet run takeoff
<run> — <hash>
<run> — failed
```

The first line is printed once the run is opened: its id, and the sha256 over
what it is pinned to. The second is printed when the workflow's process exits,
and ends on `closed`, `failed`, `waiting` or `could not tell`. `fleet run`
stays in the foreground until the process exits.

That run failed because takeoff was handed no items. A workflow of your own
that finishes reads:

```sh
$ fleet run hello --input name=fleet
<run> — <hash>
<run> — closed
```

`fleet run` exits 0 on `closed` and on `waiting`, 1 on `failed`, and 3 on
`could not tell`.

### Inputs

Each `--input key=value` pins one input; repeat the flag for more. Every value
is pinned as a string. A value can itself contain `=`: the first `=` splits
the key from the value. A key given twice is refused, as is a pair with no
`=` or no key, all with exit 2 and before anything is written.

### Who ran it

A run names who started it, as an actor. `--by` names them, as a seat
argument or a typed actor; without it fleet takes `FLEET_ACTOR`, and without
that, this machine's identity. The actor is pinned into the run as `by`, and
`run.started` carries it. A routine that fires a run starts it as
`routine:<name>`, and a run a workflow starts from inside another run is
started as `run:<id>` of the run that started it. How the actor is read is in
[Items and the record](items.md#saying-who-acts).

### Which file runs

The name resolves to `workflows/<name>.<ext>` through the installed packs'
layers (see [Packs](packs.md)). One file answers or the run is refused: no
file is exit 1, and two files under one name, such as `hello.sh` and
`hello.ts`, is exit 1 naming both.

The pack that carries the file says how to run it with its own `[runtime]`
table. A pack with no table of its own uses the table of the one pack it
imports that declares one; two such imports, or none, are refused with
exit 1. The tiny pack declares no table and takes the ts pack's, which pins
Deno.

Before it opens the run, fleet runs the `runtime-version` doctor check
against the pack that declares the table: the same check
`fleet doctor runtime-version` runs for that pack (see
[Packs](packs.md#doctor-checks)), bounded at 60 seconds. It asks the pinned
runtime for its version on the `PATH` the workflow gets. A check that exits
non-zero refuses the run with exit 1 and the check's own lines; one that is
killed, runs past 60 seconds or cannot be started refuses it with exit 3.

That `PATH` is built, not copied from yours. On macOS it is `/usr/bin`,
`/bin`, `/usr/sbin`, `/sbin`, `/opt/homebrew/bin`, `/usr/local/bin` and
`$HOME/.local/bin`; on Linux, `$HOME/.local/bin`, `/usr/local/bin`,
`/usr/bin` and `/bin`. When none of those holds the runtime, fleet puts the
directory it is found in at the front: the first directory on your own `PATH`
that holds it, else the runtime installer's `bin` — for Deno,
`$DENO_INSTALL/bin`, or `~/.deno/bin` when `DENO_INSTALL` is not set.

### What a run writes

In this order, once every check has passed:

1. The run record, filed in the project's work graph with the label
   `fleet:run` and a description naming the workflow, the pack and the
   project.
2. The run directory `runs/<run>/` under the machine directory, holding
   `inputs.toml` (the workflow, the pack, the file, who ran it, when, the
   `[inputs]` and the `[config]` settings), `policy.toml` (a byte-for-byte copy
   of the `fleet.toml` in force) and `bundle` (what the pack's bundle command
   wrote).
3. The hash, a sha256 over those three files, and the workflow, the pack and
   the file, written to the run record's `fleet.run` metadata, an object
   carrying `"v": 1`.
4. `run.started` on the event stream, carrying the run, the hash and the
   workflow.

Then the workflow's process runs in the run directory, and its standard output
and standard error go to `stdout.log` and `stderr.log` there. When it exits,
the run record is closed if the run closed or failed, with the reason
`the run closed` or `the run failed`, and one event is written: `run.closed`,
`run.failed`, `run.waiting` or `run.could_not_tell`.

A refused run files no record and makes no directory: every refusal comes
before step 1, except a bundle command that fails. The bundle command runs
after steps 1 and 2, and when it fails `fleet run` exits 1 with the command
and its error output, closes the record with the reason
`the run failed before it started`, and writes `run.failed` carrying the
refusal as its `reason`. The directory stays in place.

```sh
$ fleet run brittle
fleet run: the bundle command of `broken` exited 7 — `echo the bundler would not bundle >&2; exit 7`
  the bundler would not bundle
  the record <run> filed for this run is closed as failed
```

### Settings a run reads

A pack declares settings in its `pack.toml`, and you set them in `fleet.toml`
under `[packs.<pack>]` (see [Packs](packs.md)). A run pins the settings of the
pack that carries its workflow into `inputs.toml` under `[config]`: each
declared default, with every value you set laid over it. A setting with no
default that you did not set is absent.

```toml
[packs.tiny]
takeoff.test = "make check"
```

The pin is what the run reads, so editing `fleet.toml` later does not change
what a run already opened sees, however often it is executed again.

`fleet run` judges the whole `[packs]` table first, against every installed
pack, and refuses with exit 1 before anything is written when a section names
a pack that is not installed, a key the pack does not declare, or a value of
another type than the pack declares.

### Test commands belong to the workflow

A project's `fleet.toml` does not set test commands. `[gates] suite` and
`[gates] touched` are refused wherever they appear: `fleet run` refuses with
exit 1, naming where the command is set instead, before anything is written.
For the takeoff workflow, set them as `takeoff.test` and `takeoff.touched`
under `[packs.tiny]`, or pass `--input test=<command>` and
`--input touched=<command>`.

A `[gates]` table is refused the same way, whatever it holds, even when it is
empty. The refusal names where each key it could hold is set: `ci_marker`
under `[landing]`, `tool_commands` under `[permissions]`, and
`release_ref_glob` and the `prod_*` lists under `[guards.targets]`.

### How many runs are open

A run counts as open while its record is open: while it is running, waiting,
could not be read, or is held. `[core.run] max_open` in `fleet.toml` caps
them, four when it is not set. At the cap `fleet run` refuses with exit 1 and
lists the open runs. A value that is not a whole number is refused too, with
exit 1.

```toml
[core.run]
max_open = 2
```

## How a run ends

The workflow's process tells fleet how it ended by its exit code and the last
line with content on its standard output:

| The process exits | Last line | The run | `fleet run` exits |
| --- | --- | --- | --- |
| 0 | anything | closed; record closed; `run.closed` | 0 |
| 1 | one JSON value | failed; record closed; `run.failed` carrying that value as `reason` | 1 |
| 2 | one JSON value | waiting; record open; `run.waiting` carrying that value as `wake` and the stream's position | 0 |
| any other code, or a signal | anything | could not tell; record open; `run.could_not_tell` carrying the exit and the last line | 3 |
| 1 or 2 | not JSON, or nothing | could not tell, as above | 3 |

A failed or closed run is finished: nothing executes it again.

### What the workflow is handed

The process starts with a cleared environment. It gets:

- the pinned inputs as one JSON document on standard input: `workflow`,
  `entry`, `pack`, `by` (who ran it, as `<kind>:<id>`), `started_at`,
  `inputs` and `config`;
- `FLEET_RUN_ID`, the run's id;
- `FLEET_RUN_DIR`, the run directory, which is also its working directory;
- `FLEET_STREAM`, the event stream's path, and `FLEET_STREAM_SEQ`, the
  stream's position when the process started;
- `FLEET_BIN`, the `fleet` binary that started it;
- `FLEET_PROJECT`, the project the run was started in;
- `FLEET_DIR`, the machine directory;
- `PATH`, built as described under *Which file runs*;
- `HOME`, `FLEET_CLAUDE_BIN`, `USER`, `TMPDIR` and `LANG`, each copied from
  your environment when it is set there.

Nothing else from your environment reaches it.

## Waiting runs and the controller

A run that exits waiting stays open, and the controller carries it on. On
each poll it reads the `wake` the run's `run.waiting` carries and executes the
run again when that could have arrived:

- a wake of the form `{"items": [...], "kinds": [...], "since": <n>}` — what
  the SDK's `until` and `hold` wait on — when the stream carries an
  `item.entry` line above position `n` for one of those items, signalling an
  entry of one of those kinds (see
  [Items and the record](items.md#reading-the-record)). `n` is the stream's
  position when the waiting execution started, so an entry written while it
  ran wakes it too;
- a wake that is one run's id — what the SDK's `start` waits on — when that
  run's `run.closed`, `run.failed` or `run.cancelled` is on the stream;
- any other wake, or none, when any line has arrived after the run's
  `run.waiting`.

A cancelled run is not executed again.

Executing a run again uses the bundle already in its run directory; the
pack's bundle command does not run again. The controller first recomputes the
hash over the three pinned files, and if it differs from the one on the
record it refuses to execute the run and logs:

```text
fleet observe: the run pass refused: <run>: <run directory> hashes <sha> and <run> is pinned to <hash> — what is in the directory is not what this run was opened against
```

Each execution writes its own `run.started` and its own ending event, with the
controller as the actor: `controller:<id>`, under this machine's identity.

### A run nothing could read

A run that ends `could not tell` is also executed again by the controller, up
to `[core.run] max_crashes` more times, two when it is not set. Past that, the
controller holds the run: it raises a hold on the run's record, appends a
`held` entry to the record's timeline as `controller:<id>`, and writes the
entry's `item.entry` line. The question reads:

```text
<run> has been executed 3 time(s) and nothing could classify the last one — `[core.run] max_crashes` is 2 — nothing executes it again.
Its stdout.log and stderr.log are in <run directory>.
A. cancel it: fleet cancel <run> closes its record and clears this hold, with or without an answer
B. keep it for now: this answer clears the hold, and the record stays open, holding a [core.run] max_open slot, until it is cancelled
```

With `max_crashes = 0` the first `could not tell` holds the run. A run held
this way is not executed again, whatever the hold's answer. `fleet clear`
clears its hold like any other (see *Questions a run asks*); the record stays
open, counting against `max_open`, until `fleet cancel` closes it (see
*Cancelling a run*).

### Seats a run spawned

When a run closes, fails, is cancelled or is held at the crash cap, the
controller retires every transient seat the run spawned, releases the items
those seats still held, and writes `run.cleaned` with the count, once per run
and also when the count is zero. A run waiting on its own question keeps its
seats.

Each seat it retires gets a `session.retired` line on the stream beside the
retire's own `session.stopped`, carrying what the seat cost:

- `context_tokens`: the size of the context the session's last turn carried;
- `turns`: how many turns the session took;
- `wall_ms`: milliseconds from the session's start to the retire;
- `branch` and `commit`: the branch and the commit the seat's worktree
  stood on;
- `transcript`: whether the session's transcript could be read.

Its `item` is the run, and it also carries the seat, the worktree and what
the retire reclaimed. A reading fleet could not take is null, never zero,
and the seat is retired all the same.

## Questions a run asks

A workflow asks a person a question with a hold on its own run record (the
SDK's `hold`). The record is held the way a seat's item is: a hold in the
work graph carrying the question and its lettered options, a `held` entry on
the record's timeline, and that entry's `item.entry` line on the stream. The
run exits waiting on the record's `cleared` entries. `fleet item show <run>`
prints the question:

```sh
$ fleet item show <run>
<run> · <run>  [open]
type task · labels fleet:run · assignee none
order none
blocked by <hold>

A run of `takeoff` from the pack `tiny` on <project>.

timeline (1 entries)
<time>  run:<run>  held <hold> — ask: Land what this flight's review accepts? <item>
    A. land each accepted delivery
    B. land nothing — end the flight
    on the run's hash <hash>
    about <item>; A licenses a landing
```

You clear it with `fleet clear`, naming the run as the item:

```sh
$ fleet clear <run> A --by <reviewer-name>
<run> answered A — <hold> cleared
```

The controller's next poll executes the run again, and the workflow reads the
letter you gave. See [Items and the record](items.md) for `fleet clear`.

### What licenses a run's landing

A run lands an item only on a cleared hold about it. A hold is about an item
when the question names it among the items it licenses, as the line
`about <item>; A licenses a landing` above says; a hold about an item may
name one commit, and then it is about that commit alone. When a workflow
calls `fleet land` as the run, the landing acts as the seat `[core] reviewer`
names, and reads the run's record: the last hold about the item must be
cleared, by that seat, with the letter the hold says licenses a landing. Clear
a run's hold as the reviewer — `--by` with any seat argument that names it,
or with no `--by` on the reviewer's own machine when the reviewer is that
machine's identity — when its answer is what lets it land.

Otherwise the landing refuses with exit 1, naming what is missing:

- `run <run> raised no hold about <item> — a run lands what <reviewer-name> cleared, and nobody was asked about this item`
- `run <run>'s hold <hold> about <item> is not cleared yet`
- `run <run>'s hold <hold> about <item> was cancelled, and a cancel licenses nothing`
- `run <run>'s hold <hold> was cleared by seat:<you> and not by <reviewer-name> — a run lands as the [core] reviewer and on that seat's own clearance`
- `run <run>'s hold <hold> was cleared B, and A is the letter that licenses a landing`

A landing a run made names the run on its `landed` entry and in the item's
close reason, `landed <sha> through run <run>`.

## Cancelling a run

`fleet cancel` ends a run nothing else ends: one held at
`[core.run] max_crashes`, one waiting on a wake that does not come, or one
whose process ended without writing how.

```sh
$ fleet cancel <run>
<run> — cancelled, hold <hold> cleared
```

It exits 0. For each hold standing on the run's record it appends a
`cleared` entry that says the hold was cancelled, and clears the hold; then it
closes the record with the reason `the run cancelled`, and writes
`run.cancelled` and one `item.entry` line per `cleared` entry. A run with no
hold standing prints `<run> — cancelled`. The record no longer counts against
`max_open`, and the controller's next poll retires the seats the run spawned
and never executes it again. A cancel stops no process: an execution under way
runs to its end, and nothing acts on what it writes.

## Watching runs

`fleet status` prints a runs section after the routines, read off the event
stream. Like the rest of the page, it prints only once the controller has
published its projection; without one, `fleet status` exits 5 (see
[Status and the event stream](status.md)).

In this example each row is a different run, so the ids, holds and times are
placeholders per row:

```sh
$ fleet status
...
runs  4 failed in the last 24 hours, 0 held, 1 could not tell, 2 waiting, 0 open
  <run-a>  fails  FAILED at <time-a> — fails: this workflow fails on purpose
...
  <run-b>  takeoff  FAILED at <time-b> — takeoff: no `items` input — the flight has nothing to fly
  <run-c>  takeoff  FAILED at <time-c> — {"code":"refused","verb":"dispatch","why":"<item> is not ready — its status is `closed`"}
  <run-d>  crashes  could not tell at <time-d>, 1 execution(s) so far — exit 5, read "oops"
  <run-e>  takeoff  waiting since <time-e> for {"items":["<run-e>"],"kinds":["cleared"],"since":<n-e>}
  <run-f>  takeoff  waiting since <time-f> for {"items":["<run-f>"],"kinds":["cleared"],"since":<n-f>}

holds  1 open
```

It exits 0.

The first line counts the runs in each standing; the rows follow in the same
order, and inside each standing by run id. Each row is the run, its workflow,
and:

- `FAILED at` the time it failed, with the reason its workflow gave;
- `HELD at` the time of its last execution, for a run held at
  `[core.run] max_crashes`, `on hold <hold>`, with `, cleared` after the hold
  once the store no longer lists it open, and how many executions nothing
  could classify: `HELD at <time> on hold <hold>, cleared — nothing could
  classify 3 execution(s)`;
- `could not tell at` the time, the executions so far, the exit and the line
  that could not be read;
- `waiting since` the time, and what it is waiting for;
- `open since` the time, for a run that is executing, or whose process ended
  without writing how.

Closed runs are not listed. A run that failed more than 24 hours ago is left
out of the first line's count and of the rows; a line under the rows says how
many were left out, and `fleet event tail --type run.failed` lists them. The
last line counts the holds the store of every project this machine registers
lists open, a run's and an item's alike. A store that does not answer leaves
them uncounted: the line reads `holds  not counted — ` and the reason, the
reason is also on standard error, and `fleet status` exits 3. A run at
`could not tell` whose record cannot be asked whether it is held leaves
`the runs were not all read — whether <run> is held could not be read:` and
the reason under the rows and on standard error, with the same exit.

## Writing a workflow

A workflow is a file under `workflows/` in a pack, run by the runtime the
pack's `[runtime]` table pins:

```toml
[runtime]
name    = "deno"
version = "2.9.7"
bundle  = "deno bundle -o {bundle} {entry}"
run     = "deno run --allow-run={fleet} --allow-read={run_dir} --allow-write={run_dir} --allow-env=FLEET_DIR,FLEET_RUN_ID,FLEET_STREAM,FLEET_STREAM_SEQ,FLEET_RUN_DIR,FLEET_BIN,FLEET_PROJECT {bundle}"
```

`bundle` and `run` are shell command lines. fleet substitutes five
placeholders into them — `{entry}` the workflow file, `{bundle}` the bundle
path in the run directory, `{run_dir}` the run directory, `{fleet}` the fleet
binary, `{inputs}` the pinned `inputs.toml` — and runs each in the run
directory. A bundle command that exits non-zero, or exits 0 and writes no
`bundle`, refuses the run with exit 1; the record is filed by then.

That table is the ts pack's. A pack that imports ts and declares no table of
its own runs its workflows with it, as tiny does.

### The TypeScript SDK

The ts pack carries an SDK at `assets/sdk/mod.ts`. A workflow in a pack
installed beside ts imports it by that relative path, and hands its function
to `workflow`:

```ts
import { type Run, workflow } from "../../ts/assets/sdk/mod.ts";

export async function hello(run: Run): Promise<void> {
  const name = await run.input("name");
  const greeting = run.config("greeting");
  await run.step("say", () => `${greeting}, ${name ?? "nobody"}`);
}

if (import.meta.main) await workflow(hello);
```

`workflow` reads the environment and the pinned document, runs the function,
and exits by the table in *How a run ends*: 0 when the function returns, 2
with the step's condition as the last line when a step is waiting, and 1 with
the reason as the last line when anything else is thrown — the error's
message, or, for a verb that refused, `{"code", "verb", "why"}`. That last
line is what the event carries whole, as the status rows above show.

### Steps and replay

Every call on the run handle is a numbered step. A step writes
`step.started`, does its work, and writes `step.closed` with its result,
through `fleet event step`. When the run is executed again, the SDK reads the
run's closed steps off the stream first, and a step already closed under the
same number and name returns the recorded result without doing its work again.
That is what lets a waiting run be executed from the top: it replays to the
step it stopped at.

If step `n` was recorded under another name, the code changed under a live
run, and the run fails with `replay diverged at step n: expected <recorded>
got <name>`. A result over 64 KiB is written to `steps/<n>.json` in the run
directory, and the event carries its sha256 instead.

The handle:

| Call | What it does |
| --- | --- |
| `run.step(name, fn)` | runs `fn` once, as a step, and returns its result |
| `run.input(key)` | one pinned input, or `null` where none was pinned; a step |
| `run.config(key)` | one pinned setting of the pack that carries the workflow, or `undefined`; not a step |
| `run.now()`, `run.random()` | the time and a random number, as steps, so a replay sees the same values |
| `run.spawn({ role: "builder", item, touched? })` | `fleet dispatch <item>`, with `--touched` when given, which gives the item to a transient seat; an item whose last delivery no return and no landing followed is not dispatched again, and the step answers `already delivered at <commit>` |
| `run.review(item, "accepted")` or `run.review(item, { returned: file })` | `fleet review <item> --land`, or `--return <file>` |
| `run.land(item, sha, { test? })` | `fleet land <item> <sha>`, with `--test` when given |
| `run.hold(question, options, about?)` | `fleet hold --item <run> --question <file>` on the run's own record, then waits for the hold's clearance; returns the letter it was cleared with, or `cancelled` where it was cancelled |
| `run.until(items, state)` | waits until each item's timeline answers `state`; returns each item's answering entry as `fleet item show --json` prints it |
| `run.start(name, inputs?)` | `fleet run <name>` as a child run, and waits for it to close |

The verbs run the fleet binary from the project with `--by run:<run>`, and
all but `run.start`'s `fleet run` with `--json`, so every act a run takes is
on the record under the run's id. A verb that refuses fails the run, with
`{"code", "verb", "why"}` as its reason. A hold is raised once and a child run
is started once, however often the run is executed again. `spawn` takes the
one role `builder`, and refuses a `model`.

`spawn`, `until` and `hold` read an item through `fleet item show <item>
--json` and never off the stream (see
[Items and the record](items.md#reading-the-record)). `hold` takes each option
as `<letter>. <text>`, writes the question as JSON to `holds/<n>.json` in the
run directory, and passes `about` — `{ items, commit?, licenses }`, the items
the answer licenses, the one commit where it names one, and the licensing
letter — into the question (see *What licenses a run's landing*). `until`
takes six states, each answered by an entry on the item's timeline:

| State | Answered by |
| --- | --- |
| `dispatched` | the last `ordered` entry, where no `order_withdrawn` followed it |
| `delivered` | the last `delivered` entry no return followed |
| `reviewed` | the last `reviewed` entry, where it accepts and no delivery followed it |
| `returned` | the last `reviewed` entry, where it returns and no delivery followed it |
| `held` | the last `held` entry no `cleared` entry for its hold followed |
| `landed` | the last `landed` entry |

A step that cannot answer yet waits on
`{"items": [...], "kinds": [...], "since": <n>}`: the items still
outstanding, the entry kinds that could answer it, and the stream's position
when the execution started (see *Waiting runs and the controller*).

## The takeoff workflow

The tiny pack carries `workflows/takeoff.ts`: a flight's middle, run with the
person away. It spawns a builder per item, waits for each delivery, reviews
it, lands the accepted ones, and ends by writing a report.

```sh
$ fleet run takeoff --input items=<item-1>,<item-2> --input policy=review=hold,width=2 --input test="make check"
```

Its inputs:

- `items`: the items to fly, in order, comma- or space-separated or as a JSON
  array. Required, and an item listed twice fails the run.
- `policy`: comma-separated pairs. `review=hold` (the default) asks the person
  for every verdict at a hold; `review=accept` asks once, before anything is
  spawned, whether to land every delivery the review accepts. `width=<n>` is
  how many items are in the air at once, 1 by default. Any other pair fails
  the run.
- `test`: the command each landing runs on its rebased tree before the push,
  handed to `fleet land --test`. Without it, `takeoff.test` under
  `[packs.tiny]` in `fleet.toml`; the input wins.
- `touched`: the command each builder's brief names for it to run over its own
  diff, handed to `fleet dispatch --touched`. Without it, `takeoff.touched`
  under `[packs.tiny]`; the input wins.

A blank test command counts as none. With no test command, the flight still
flies: each landing runs nothing and says NOT TESTED on its `landed` entry
(see [Items and the record](items.md)), and the report's first line reads:

```text
NOT TESTED — this flight was handed no test command, so every landing ran nothing and stands on the review alone. Set `takeoff.test` under [packs.tiny] in fleet.toml, or pass `--input test=<command>`.
```

Each builder is a transient seat, given its item by `fleet dispatch`.

Under `review=hold` each delivered item raises a hold on the run,
`Accept <item> at <commit>?` with `A. accept and land` and
`B. return to the builder`, about that item at that commit, with `A` as the
letter that licenses its landing. `A` accepts and lands it; any other letter
returns it to the builder with a findings file under `findings/` in the run
directory.

Under `review=accept` the run raises one hold before it spawns anything,
about every item it flies:
`Land what this flight's review accepts? <item-1>, <item-2>` with
`A. land each accepted delivery` and `B. land nothing — end the flight`. `A`
licenses the landing of each delivery, and every delivery is then accepted
and landed unasked. Any other letter fails the run with
`takeoff: the person declined to license this flight's landings (answered B)`,
and a cancel fails it with
`takeoff: the flight's licence hold was cancelled`, both with nothing
reviewed.

Either hold licenses a landing only when the `[core] reviewer` clears it (see
*What licenses a run's landing*), so clear them as that seat.

When every item is settled, the run writes two files to its run directory and
closes: `report.md`, with the decisions answered at holds, each item's outcome
and landed sha, and the counts; and `board-tick.md`, the landed items to tick
on the departure board. A workflow writes only its run directory, so the tick
is left for a person or a seat to apply.

### Composing a flight

The tiny pack carries no preboard workflow: `fleet run preboard` is refused
with exit 1 like any unknown name. `preboard` is one of tiny's skills. A
session running it reads the departure board, filters the rows, and prints the
`--input` lines for `fleet run takeoff`, printing `NOT TESTED — no test
command; every landing in this flight will run nothing` in place of the
`test` line where neither the person nor `takeoff.test` names one. tiny's
`takeoff` skill is the one that runs the command, after reading the list back
to the person.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `--by` or `FLEET_ACTOR` names no seat, or more than one | 1 | `fleet run: --by <arg> names no seat — the seats are <machine-name> (<id>), …` (or `names <n> seats — …`) | Name one seat, or give a typed actor. |
| `--by` is empty | 2 | `fleet run: --by names no seat — the argument is empty` | Name a seat, or leave `--by` out. |
| An input without `=` | 2 | ``fleet run: `--input name` is not a pair — an input is written key=value`` | Write `key=value`. |
| An input with no key | 2 | ``fleet run: `--input =x` names no key — an input is written key=value`` | Name the key. |
| One key given twice | 2 | ``fleet run: `--input name=` is given twice — a run pins one value per key`` | Give each key once. |
| `fleet.toml` sets `[gates] suite` or `[gates] touched` | 1 | ``fleet run: [gates] suite is not project policy, and nothing reads it — a test command is the workflow's: set `takeoff.test` under [packs.tiny] in fleet.toml, or `--input test=<command>` on `fleet run takeoff`; a landing run by hand takes `fleet land --test <command>`, and delete the key``, and on the next line the `[gates]` refusal below | Move the command where it says, and delete the key and the `[gates]` table. |
| `fleet.toml` carries a `[gates]` table | 1 | ``fleet run: [gates] is not a policy table, and nothing reads it — its keys are set by purpose: `ci_marker` under [landing], `tool_commands` under [permissions], and `release_ref_glob` and the `prod_*` lists under [guards.targets]; move each one there, and delete the table`` | Move each key to the table named, and delete `[gates]`. |
| The open runs are at `[core.run] max_open` | 1 | ``fleet run: 2 run(s) are open and `[core.run] max_open` is 2 — the open runs are <run-1>, <run-2>, and one has to close or the cap has to be raised`` | Wait for a run to close, or raise the cap. |
| `max_open` is not a whole number | 1 | ``fleet run: `[core.run] max_open` is string, and a cap has to be a whole number — this fleet will not fall back to a cap it did not name`` | Write a whole number. |
| No pack carries the workflow | 1 | ``fleet run: no workflow named `nosuch` — no installed pack carries `workflows/nosuch.<ext>` `` | Check the name, or install the pack. |
| Two files answer to one name | 1 | ``fleet run: `hello` names 2 files and a run takes one — workflows/hello.sh, workflows/hello.ts`` | Remove or rename one. |
| The workflow is `preboard` | 1 | ``fleet run: no workflow named `preboard` — no installed pack carries `workflows/preboard.<ext>` `` | Use tiny's preboard skill; it is not a workflow. |
| Neither the carrying pack nor its imports declare `[runtime]` | 1 | ``fleet run: `tiny` carries `workflows/takeoff.ts` and declares no [runtime] table, and no pack it imports declares one — that table is the one thing fleet reads about a workflow's language, so there is no command to bundle this file with`` | Install the pack that declares it (ts, for tiny). |
| The carrying pack imports a pack that is not installed, and declares no `[runtime]` | 1 | ``fleet run: `tiny` carries `workflows/takeoff.ts` and declares no [runtime] table, and no installed pack it imports declares one: `tiny` imports `ts`, which is not installed — `` and either the `fleet pack add` line that adds it, read off `packs.lock`, or `its manifest names the source <source>` | Install the pack it names. |
| Two imports each declare `[runtime]` | 1 | the two packs named | Declare `[runtime]` in the carrying pack. |
| The runtime check is red | 1 | ``fleet run: `<pack>`'s runtime check is red, so `<workflow>` is not opened — `` and the check's lines | Install the pinned runtime version. |
| `[packs.<pack>]` names a pack that is not installed | 1 | ``fleet run: `<workflow>` is not opened — fleet.toml has a [packs.nosuch] section and no pack named `nosuch` is installed — the installed packs are tiny, ts, <pack>`` | Remove the section, or install the pack. |
| `[packs.<pack>]` sets a key the pack does not declare | 1 | ``fleet run: `<workflow>` is not opened — [packs.<pack>] sets `farewell`, which the pack `<pack>` does not declare — its pack.toml declares greeting`` | Remove the key. |
| `[packs.<pack>]` sets a value of the wrong type | 1 | ``fleet run: `<workflow>` is not opened — [packs.<pack>] sets `greeting` to an integer, and the pack `<pack>` declares it string`` | Write the declared type. |
| `fleet.toml` does not parse | 3 | ``fleet run: the policy in force at <path> does not parse, so the settings its [packs] table sets cannot be read:`` and the parse error | Fix the file. |
| The work graph cannot be read | 3 | ``fleet run: the work graph could not be read:`` and the reason | Make `bd` reachable. |
| The pack's bundle command fails | 1 | ``fleet run: the bundle command of `<pack>` exited <rc> — `` the command, its error output, and `the record <run> filed for this run is closed as failed` | Fix the workflow, and run it again. |
| `fleet cancel` of an item that is not a run's record | 1 | ``fleet cancel: <item> is not a run's record — it carries no `fleet:run` label, and `fleet cancel` ends runs and nothing else`` | Name the run's id. |
| `fleet cancel` of a run already closed | 1 | `fleet cancel: <run> is closed already — the run has ended and there is nothing to cancel` | Nothing to do. |
| A run's landing, with no hold on the run about the item | 1 | `fleet land: run <run> raised no hold about <item> — a run lands what <reviewer-name> cleared, and nobody was asked about this item` | Ask about the item in the workflow, and clear it as the reviewer. |
| A run's landing, with the hold about the item not cleared | 1 | `fleet land: run <run>'s hold <hold> about <item> is not cleared yet` | Clear the hold as the reviewer. |
| A run's landing, with the hold about the item cancelled | 1 | `fleet land: run <run>'s hold <hold> about <item> was cancelled, and a cancel licenses nothing` | The run cannot land it; review and land it by hand as the reviewer. |
| A run's landing, with the hold cleared by another seat | 1 | `fleet land: run <run>'s hold <hold> was cleared by seat:<id> and not by <reviewer-name> — a run lands as the [core] reviewer and on that seat's own clearance` | Clear the run's holds as the reviewer. |
| A run's landing, with the hold cleared on another letter | 1 | `fleet land: run <run>'s hold <hold> was cleared B, and A is the letter that licenses a landing` | Nothing licenses it; the answer said not to land. |

Every `fleet run` refusal above is made before the run record is filed, except
a failed bundle, which closes the record it filed as failed.

## See also

- [Items and the record](items.md): `dispatch`, `deliver`, `review`, `land`,
  `hold` and `clear`, the verbs a workflow calls, the entries they append, and
  `fleet item show`, which the SDK reads an item through.
- [Packs](packs.md): installing the tiny and ts packs, how layers resolve a
  workflow's file, and pack settings in `fleet.toml`.
- [Status and the event stream](status.md): the rest of `fleet status`, and
  `fleet event tail` for reading a run's events.
- [The controller and seats](seats.md): the controller that carries waiting
  runs on, and the transient seats a run spawns.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
