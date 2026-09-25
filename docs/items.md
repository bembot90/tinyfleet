# Items and the record

An item is one piece of work in the project's work graph, the bd store beside
the project. Seven verbs move it: `fleet dispatch` gives it to a seat,
`fleet brief` renders what that seat reads first, `fleet deliver` hands the
work over, `fleet hold` stops it on a question and `fleet clear` answers the
question, `fleet review` reads the delivery and writes a verdict, and
`fleet land` puts the reviewed commit on the trunk and closes the item. Every
verb that writes appends a typed entry to the item's timeline and reads it
back before it exits 0, so the item's timeline, in the store's order, is its
record. `fleet item show` renders it.

## Terms

- **Item**: one entry in the project's bd store, named by its id.
- **Entry**: one typed fact on an item: who wrote it, when, and what it
  says. Its kind is one of `ordered`, `order_withdrawn`, `delivered`,
  `reviewed`, `held`, `cleared` and `landed`. The store keeps each entry as
  one JSON comment on the item.
- **Timeline**: the item's entries in the store's order. A comment a person
  writes on the item by hand is theirs and is not on the timeline.
- **The record**: the item's timeline, plus its assignee and its order index.
- **Order**: the `ordered` entry `dispatch` appends, together with the
  item's order index: an object in the item's metadata under `fleet.orders`,
  carrying `"v": 1`. An item with no order index has not been given to
  anybody. fleet reads no other key for the index: an `orders` key another
  tool wrote is not an order, whatever it holds, and fleet leaves it as it
  is.
- **Brief**: the first turn a dispatched seat reads, rendered from the item.
- **Ring**: one message sent to a seat's live session, naming the item and,
  from `dispatch`, where its brief is.
- **Work branch**: the branch a seat builds on. The trunk is `main`, and fleet
  reads it as `origin/main`.
- **Reviewer**: the seat named by `[core] reviewer` in the fleet's policy
  file. The value is any seat argument — the seat's full id, eight or more
  hex digits of it, its name or its machine name — and names one seat the
  fleet lists, an agent's or a person's. A delivery is assigned to its id.
- **Actor**: who a verb acts as, written `<kind>:<id>`. See
  [Saying who acts](#saying-who-acts).
- **Verdict**: the `reviewed` entry `review` appends: an accept, or a return
  with findings.
- **Hold**: the store's own object `fleet hold` raises on an item, carrying
  the question. A hold blocks the item until someone clears it with an
  answer. An item with an open hold is **held**, and the `held` entry
  `fleet hold` appends is its **park**. [Runs and workflows](runs.md) covers
  how a workflow resumes a held item.
- **Checks**: the automated pass/fail readings a verb takes. The **builder's
  checks** are the command a dispatch names for the seat to run over its own
  diff; a landing's **check rows** are the rows `fleet land` prints, one per
  condition it reads.
- **Lane**: the queue a project's landings take one at a time.

## Saying who acts

Every entry names who wrote it, and so does every line a verb writes to the
event stream. That is the verb's actor, one of four kinds, written
`<kind>:<id>`:

- `seat:<id>`: a seat, by its full id (see
  [The controller and seats](seats.md));
- `run:<id>`: a run, by its record's id; a workflow's verbs act this way
  (see [Runs and workflows](runs.md));
- `routine:<name>`: a routine, which starts the runs it fires this way;
- `controller:<id>`: the controller, under this machine's identity.

Each writing verb takes `--by`. Its value is a typed actor as above, or a
seat argument: the seat's full id, eight or more hex digits of it, its name
in any case, or its machine name. A seat argument is resolved over the seats
the fleet lists — the `[seats]` table in `fleet.toml`, this machine's
transient seats and this machine's identity — and the verb acts as
`seat:<id>` for the one seat it names. Without `--by` the verb reads
`FLEET_ACTOR`, which takes the same two forms. With neither, it acts as this
machine's identity, the human seat in `identity.toml`, minting it where the
machine has none, and says so on standard error while `fleet.toml` does not
list it (see [Who you are](seats.md#who-you-are-identitytoml)). The
controller sets `FLEET_ACTOR=seat:<id>` on every session it starts, so a
seat's own verbs act as that seat. No other variable names the actor.

A `--by` or `FLEET_ACTOR` that names no seat, or more than one, is refused
with exit 1 before the item is read or anything is written, and the message
lists every seat by machine name and id:
`--by nobody names no seat — the seats are <machine-name> (<id>), …`.
An empty `--by` is exit 2: `--by names no seat — the argument is empty`. A
value that opens on one of the four kinds and carries an id that kind does
not take — a seat id that is not a whole id, or an empty or spaced id for
the others — is exit 1:
`` `seat:nope` is a typed actor with a bad id — nope is not a seat id — a seat id is 36 characters, 8-4-4-4-12 hex ``.
A `FLEET_ACTOR` of nothing but spaces is read as unset.

`fleet review` acts in every mode, `--show` included, although `--show`
writes nothing. `fleet brief` and `fleet item show` write nothing and take
no `--by`.

The examples below act as three seats: you, a person, as `seat:<you>`; the
builder, an agent seat named `<seat-name>`, as `seat:<seat>`; and the
reviewer, named `<reviewer-name>`, as `seat:<reviewer>`. Each `<…>` in
`seat:<…>` is a full id.

The actor is what the store records too: every write a verb makes to the
item is made under `<kind>:<id>`. The one exception is the close
`fleet land` makes, which is made under the closer's bare id, because `bd`
closes an assigned item only for an actor equal to its assignee.

The seat verbs (`deliver`, `hold`) use the actor to find the item: it is the
one item assigned to that seat's id, open or in progress, that carries an
order index and is not an epic. A seat holding two names one with
`--item <id>`, which names an item the acting seat holds and is refused
otherwise: `<item> is held by <assignee> and not by seat:<seat> — --item names
an item the acting seat holds`. A run's record is its own run's to name. An
actor that is not a seat holds nothing, and without `--item` is refused with
exit 1: `run:<run> is not a seat, so it holds nothing — pass --item <id>`.

Run the verbs from inside the project or a seat's worktree of it. They find
the fleet by walking up from the current directory to the nearest
`.fleet/project.toml` or `fleet.toml`. Where the walk finds neither, a
directory inside a git checkout uses the policy file the machine's
`config.json` names. Anywhere else they exit 3:

```sh
$ fleet review <item>
fleet review: no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one
```

In an embedded fleet, the project's name is the name of the directory the
walk stopped in. Run from a seat's worktree, that is the worktree's own
directory name, and the brief and the lane carry that name.

## Dispatching an item

`fleet dispatch` gives a ready item to a seat. The item must be in the
store's ready set (open, and blocked by nothing), must not be an epic, and
must carry no order yet. `--to` takes a seat argument, resolved over the
seats this machine runs: the agent seats in its seat list, and never a
person.

```sh
$ fleet dispatch <item> --to <seat-name>
ordered <item> to <seat-name> — entry <entry>
```

It exits 0. In one act it writes three things and reads all three back:

- the assignee: the seat's full id, `<seat>`;
- the `ordered` entry on the item's timeline, naming the seat by its full
  id; `<entry>` is the entry's id;
- the order index in the item's metadata, `fleet.orders`: `by` (the actor,
  `seat:<you>`), `kind` (`dispatch`), `seat` (the full id), `at` and `v`
  (`1`).

Then it writes one `item.entry` line to the event stream for the `ordered`
entry, and the brief to `<fleet-dir>/briefs/<item>.md`. Last, it rings the
seat: one message to the seat's live session saying the item is theirs and
where the brief is.

### When the seat has no live session

The order stands and nothing is undone. The order line is not printed; you
get the brief's path and the reason on standard error, and the exit is 4:

```sh
$ fleet dispatch <item> --to <seat-name>
brief: <fleet-dir>/briefs/<item>.md
fleet dispatch: ORDERED, NOT RUNG: no live session for <seat-name>; the order stands and the seat's successor reads it at wake
```

A ring that fails for any other reason is exit 1, with
`ORDERED, NOT RUNG:` and the cause. In both cases the item is ordered;
dispatching it again refuses, because it carries an order.

### Without `--to`

With no `--to`, fleet appends an `ordered` entry naming no seat and writes
the order index first, and then asks for a transient seat to hold it. The
brief names the seat `(transient)`. When the seat starts, the item is
assigned to its id, a second `ordered` entry names it, the id joins the
order index, and the output is `ordered <item> to a transient seat — entry
<entry>` followed by the two lines `fleet seat spawn` prints: the machine's
load average and the transient seats mid-turn. When the spawn is refused,
fleet withdraws the order: it removes the order index, appends an
`order_withdrawn` entry carrying the cause, prints
`withdrawn: DISPATCH WITHDRAWN — spawn refused: <cause>` on standard error,
and exits 1. When fleet cannot tell whether the seat started, the order
stands with nothing written after it, standard error carries
`could not tell: DISPATCH COULD NOT TELL — the spawn could not be observed: <cause>`,
and the exit is 3. [The controller and seats](seats.md) covers transient
seats.

### `--touched`

`--touched <command>` names the builder's checks: the command the seat runs
over its own diff before it delivers. It appears in the brief's "Your checks"
section. Without it, that section says the dispatch named no command and
tells the seat to run only the suites its diff reaches.

## Rendering a brief

`fleet brief` prints the brief for an ordered item on standard output and its
size on standard error. It writes nothing.

```sh
$ fleet brief <item> --to <seat-name>
# <item> — your first turn

You are `<seat-name>`, working on `<project>`. This page is everything you were
given. Read it once, in full, before your first act.
...
```

Standard error carries one line, `brief: <n> bytes`. It exits 0. The brief
names the seat by its machine name. Without `--to`, the seat reads
`(transient)`. `--touched <command>` fills the checks
section as it does on `dispatch`.

The brief carries the order, the item as `fleet item show` renders it, the
builder's checks, one line per guard class saying `on` or `off`, the rules
every seat works under, and the two JSON shapes a seat hands in: the
delivery's and the question's. It is rendered whole or not at all: a template
that cannot render prints nothing and exits 3. An item with no order index is
refused with exit 1.

The brief, the rules and the three JSON shapes come from files in the pack
layers, so an installed pack can replace them; see [Packs](packs.md). The
verbs read a delivery and a question against their own shape whatever the
layers say, so a replaced delivery or question schema that does not describe
it makes the brief exit 3: `assets/delivery.schema.json as the layers resolve
it does not describe what fleet deliver reads: <why>`. The seven verbs
take `--packs-dir <dir>` to read the packs from another directory, and then
read the defaults from the `defaults` directory beside it.

## Delivering work

`fleet deliver` hands the work over from inside the seat's worktree. The
delivery is the staged set: stage what you are handing over, write the
delivery file, and run it.

```sh
$ fleet deliver --delivery <delivery-file>
DELIVERED, NOT RUNG: no live session for <reviewer-name>; <item> is theirs and their successor reads it at wake
```

It exits 0. The line above is what you see when the reviewer has no live
session; when the ring reaches the reviewer, nothing is printed, and when the
ring fails, standard error carries `DELIVERED, NOT RUNG: <cause>; the
delivery stands`. The exit is 0 in all three cases.

In order, it:

1. commits the staged set on the work branch, with the message
   `<item>: delivered by seat:<seat>`;
2. reassigns the item to the reviewer's full id;
3. appends the `delivered` entry — your file's fields, with the commit, the
   branch and the base filled in — and reads it back, and the assignee;
4. writes one `item.entry` line to the event stream for it;
5. rings the reviewer.

`fleet item show` renders the entry:

```text
<time>  seat:<seat>  delivered <commit> on <work-branch>, base <base>
    files: GREETING
    checks:
      - GREETING holds one line: PASS: wc -l GREETING printed 1
    suite: test -f GREETING, rc 0
    spec corrections: none
    not proven:
      - the greeting's wording — a person reading it
    decisions:
      D1 plain text; not taken: markdown; because the item says a line
    covers: none
```

The commit and the base are whole 40-character shas. The base is
`origin/main` as your checkout last fetched it. `deliver` does not fetch.

### The delivery file

The delivery is a JSON file of the shape `assets/delivery.schema.json` gives,
which the brief shows whole. It carries what only the seat knows; the
commit, the branch, the base and the time are the verb's. Every key is
required, and no string is empty:

```json
{
  "files": ["GREETING"],
  "checks": [{"check": "GREETING holds one line", "result": "PASS: wc -l GREETING printed 1"}],
  "suite": {"command": "test -f GREETING", "rc": 0},
  "spec_corrections": [],
  "not_proven": [{"surface": "the greeting's wording", "command": "a person reading it"}],
  "decisions": [{"call": "plain text", "not_taken": "markdown", "because": "the item says a line"}],
  "covers": []
}
```

- `files`: every path the delivery touched; never empty.
- `checks`: each acceptance check, with its result.
- `suite`: the suite that ran, as `command` and `rc`, or
  `{"not_tested": "<why>"}` where none ran.
- `spec_corrections`: each premise the item got wrong, as `premise` and
  `refuted_by`.
- `not_proven`: what the delivery does not establish, as `surface` and the
  `command` that would measure it; never empty.
- `decisions`: the calls the item left to the seat, as `call`, `not_taken`
  and `because`. The first is `D1`, and the review walks them by number.
- `covers`: the requirements the delivery covers.

A file that does not read, or is not that shape, is refused with exit 2 before
anything is committed, naming the first thing wrong:

```sh
$ fleet deliver --delivery <bad-file>
fleet deliver: the delivery at <bad-file> is not the shape assets/delivery.schema.json gives: missing field `checks` at line 1 column 13 — the brief shows that schema
```

`--note` is refused with exit 2 whatever it names:
`--note is gone: a delivery is a JSON file — fleet deliver --delivery <file>; its shape is assets/delivery.schema.json, which the brief shows`.

### A held commit

A worktree resumed after `fleet hold` holds its work in the held commit and
has nothing to stage. With nothing staged and HEAD at any commit other than
the tip of `origin/main`, `deliver` delivers HEAD as it stands and commits
nothing:

```sh
$ fleet deliver --delivery <delivery-file>
DELIVERED AS-IS: nothing was staged and HEAD <held-commit> is ahead of origin/main at <trunk-tip> — the delivery on <held-item> is that commit and this verb committed nothing
DELIVERED, NOT RUNG: no live session for <reviewer-name>; <held-item> is theirs and their successor reads it at wake
```

The line says "ahead" whatever HEAD's relation to `origin/main`: a HEAD
behind it, or on another line of history, is delivered the same way. With
nothing staged and HEAD at the tip of `origin/main`, there is no work to hand
over and it refuses.

### What it refuses before committing

These are all read before anything is written, so a refusal leaves the
worktree as it stood: the trunk branch, a changed or untracked file outside
the staged set, nothing staged at the base, a delivery file that does not
read or is not its shape, a seat holding no ordered item or more than one, an
`--item` the actor does not hold, and a fleet with no `[core] reviewer`. The
table under [When it refuses](#when-it-refuses) gives each message.

## Holding an item on a question

`fleet hold` stops the work on a question for a person. Write the question as
a JSON file of the shape `assets/question.schema.json` gives, which the brief
shows whole: `question`, one line; `context`, optional; and `options`, each a
capital `letter` and its `text`, no letter twice.

```json
{
  "question": "Should the second line print to stdout or to a file?",
  "context": "The item does not say.",
  "options": [
    {"letter": "A", "text": "stdout, one line"},
    {"letter": "B", "text": "a file named OUT"}
  ]
}
```

An optional `about` names the items the answer licenses — `items`, a
`commit` where the question names one, and `licenses`, the letter that
licenses them. A workflow's hold uses it; see [Runs and workflows](runs.md).

Run it from the seat's worktree:

```sh
$ fleet hold --question <question-file>
<hold>
```

It prints the hold's id and exits 0. In order, it:

1. commits everything the worktree holds on the work branch — staged,
   modified and untracked alike — with the message
   `<held-item>: held — seat:<seat> asked a question at <time>`; a tree with
   nothing to commit parks on HEAD;
2. raises a hold in the store blocking the item, carrying the question, its
   context and one `<letter>. <text>` line per option as its reason;
3. appends the `held` entry and reads it back;
4. writes one `item.entry` line to the event stream for it.

`fleet item show` renders the entry:

```text
<time>  seat:<seat>  held <hold> — ask: Should the second line print to stdout or to a file?
    The item does not say.
    A. stdout, one line
    B. a file named OUT
    on <work-branch> at <held-commit>
```

The item leaves the store's ready set while the hold is open. `hold` rings
nobody and dispatches nothing; the item stays assigned to the seat and keeps
its order.

It refuses the trunk branch, a seat holding no ordered item, and a question
file that does not read or is not its shape, all before it commits.
`--note` is refused with exit 2:
`--note is gone: a question is a JSON file — fleet hold --question <file>; its shape is assets/question.schema.json, which the brief shows`.

## Clearing a hold

`fleet clear` answers a held item's question and clears its hold: the item,
then the letter of the option chosen.

```sh
$ fleet clear <held-item> b --text "a file, but name it OUT.txt"
<held-item> answered B — <hold> cleared
```

It exits 0. The letter is read without regard to case. The hold it clears is
the item's open one: the last `held` entry with no `cleared` entry naming its
hold after it. It appends the `cleared` entry and reads it back, clears the
hold, checks that the store's list of open holds does not carry it, and
writes one `item.entry` line to the event stream:

```text
<time>  seat:<you>  cleared <hold> — answered B: a file, but name it OUT.txt
```

`--text` says what you decided beyond the option. A letter the question does
not offer is an answer only with `--text`; without it, `clear` exits 2 and
lists the letters on offer.

Clearing returns the item to the store's ready set and dispatches nothing.
The item still carries its order and its assignee, so `fleet dispatch`
refuses it; the seat that resumes it delivers it (see
[A held commit](#a-held-commit)).

It refuses an item with no open hold, a hold the store does not list open,
and an argument that is not a single letter.

## Reviewing a delivery

`fleet review <item>` reads the item's last delivery: the last `delivered`
entry on its timeline. It takes one of three modes: `--show` (the default),
`--land`, or `--return <file>`. Every mode first prints the size line,
measured from the delivery's base to its commit:

```text
size: 1 file(s), +1, -0 — tests: no, executable: no
```

`tests:` is `yes` when a changed path sits under a `test` or `tests`
directory or is named as a test file. `executable:` is `yes` when a changed
file is executable in your working tree.

`--land` and `--return` write a verdict, and a verdict is the holder's: they
refuse a seat that does not hold the item before the delivery is read. A
delivered item's holder is its reviewer. A run writes one as the
`[core] reviewer`; a routine or the controller writes none. `--show` writes
nothing and anyone can run it.

### Reading it

```sh
$ fleet review <item> --show --by <reviewer-name>
size: 1 file(s), +1, -0 — tests: no, executable: no

<time>  seat:<seat>  delivered <commit> on <work-branch>, base <base>
    files: GREETING
...
    decisions:
      D1 plain text; not taken: markdown; because the item says a line
    covers: none
```

After the size line and a blank line comes the `delivered` entry, as
`fleet item show` renders it. It writes nothing and exits 0.

### Returning it

Write the findings as a JSON file of the shape
`assets/findings.schema.json` gives: `{"findings": [{"text": "…"}]}`, one
object per finding, never empty. The verb numbers them `F1`, `F2` and so on
in the file's order. Then:

```sh
$ fleet review <item> --return <findings-file> --by <reviewer-name>
size: 1 file(s), +1, -0 — tests: no, executable: no
RETURNED, NOT RUNG: no live session for <seat-name>; the return stands and their successor reads it at wake
```

It exits 0. It reassigns the item to the seat its order index names, by that
seat's full id, appends the `reviewed` entry with the findings and reads both
back, writes one `item.entry` line to the event stream, and rings that seat.
The second line above appears only when the seat has no live session; a ring
that fails puts `RETURNED, NOT RUNG: <cause>; the return stands` on standard
error. The entry reads:

```text
<time>  seat:<reviewer>  returned <commit> with 1 finding(s)
    size: 1 file(s), +1, -0 — tests: no, executable: no, against <base>
    F1 the greeting ends without a period
```

A findings file that does not read, is not its shape, or lists no finding
exits 2, after the size line is printed and before anything is written.

### Accepting it

```sh
$ fleet review <item> --land --by <reviewer-name>
size: 1 file(s), +1, -0 — tests: no, executable: no
```

It exits 0. `--land` lands nothing: it appends the accepting `reviewed`
entry that `fleet land` reads, and one `item.entry` line on the event
stream. It walks the delivery's decisions and accepts each one; a decision
the reviewer will not take is a finding, and the delivery goes back with it.

```text
<time>  seat:<reviewer>  accepted <redelivered>
    size: 1 file(s), +1, -0 — tests: no, executable: no, against <base>
    walk: D1 accept
```

`--show` with either of the others, and `--land` with `--return`, is a usage
error, exit 2.

## Landing an item

`fleet land <item> <commit>` squashes the accepted commit onto the trunk,
pushes it, and closes the item. It takes a commit, 7 to 40 hex characters,
and never a branch name. It runs as the seat that holds the item, which after
a delivery is the reviewer, from a linked worktree: the actor's seat id must
be the item's assignee. A run's landing acts as the `[core] reviewer` seat
(see [Runs and workflows](runs.md)); a routine or the controller cannot
land. Run from the project's primary checkout, it does its work in the
reviewer's worktree for this project, as the machine's seat list names it.

```sh
$ fleet land <item> <redelivered> --test "test -f GREETING" --reason "greeting added" --by <reviewer-name>
1. reviewed commit  PASS       <redelivered> — the last verdict on <item> accepts it, by this landing's own reviewer
2. staged set       PASS       1 path(s) outside .beads/, equal to the delivery's own set; .beads/issues.jsonl regenerated by the store's own export
3. CI marker        NONE       no [landing] ci_marker in this project — no marker is appended
4. suite            PASS       `test -f GREETING` rc 0 in <took>, read from the child's own exit; log <fleet-dir>/land/<item>/suite.log
5. base current     PASS       behind=0 against origin/main, counted in the same act as the push
6. work branch      SAFE       <work-branch> — its tip is the reviewed commit and the landed diff is empty
7. tree clean after PASS       git status is empty in <reviewer-worktree>
LANDED <landed>
```

It exits 0. Each row prints as it is read. The last line is `LANDED` and the
landed sha, whole.

In order, it:

1. takes the project's lane, and prints `waiting on the lane:` with the
   holder's item and time when another landing holds it;
2. checks the worktree holds no changed file but the ones `--also` names;
3. checks the item is open and held by you; for a run, that the
   `[core] reviewer` cleared a hold about the item (see
   [Runs and workflows](runs.md)); and that the last `reviewed` entry is an
   accept of this commit, written by the landing's own reviewer;
4. fetches `origin`, cuts `land/<item>` at `origin/main`, and squashes the
   commit onto it;
5. regenerates the store's export and checks the staged set equals the
   delivery's own paths, outside `.beads/`;
6. runs `[landing] ci_marker`, where the project sets one, with the staged
   paths on its input, and appends what it prints to the commit subject;
7. commits, with the subject `<item>: <title>` and two trailers:
   `Seat: <reviewer>`, the full id of the seat that landed it, and
   `Implemented-by: <seat>`, whoever the last delivery names — a seat by its
   full id, and any other actor as `<kind>:<id>`;
8. runs the `--test` command on the land branch;
9. fetches again, counts how far `origin/main` has moved, and pushes to
   `main` only where it has not;
10. appends the `landed` entry and reads it back, writes one `check.read`
    per suite reading and then one `item.entry` line to the event stream,
    and closes the item with the reason `landed <landed>`, followed by
    ` — ` and `--reason` where you gave one;
11. deletes the work branch, locally and on `origin`, only where the row
    says `SAFE`, puts the worktree back on `origin/main`, and deletes
    `land/<item>`.

The landing's own files sit in `<fleet-dir>/land/<item>/`: the commit
message, `suite.log` where a suite ran, `suite.2.log` on a rerun, and
`push.out` once the push has run.

The `landed` entry carries the landed sha and the trunk commit it moved
from, the squashed commit, what tested it, every check row, the work
branch's classification, and a block of commands that re-run each verdict:

```text
<time>  seat:<reviewer>  landed <landed> (range <old>..<landed>; squash of <redelivered>)
    test: test -f GREETING, rc 0
    1. reviewed commit  PASS       <redelivered> — the last verdict on <item> accepts it, by this landing's own reviewer
...
    work branch: <work-branch> — safe
    commands:
      git merge-base origin/main <redelivered>
...
```

Every sha on it is whole. The entry is by the seat that landed; the builder
is the actor on the `delivered` entry. A landing a run made reads
`; through run <run>` after the squashed commit, and its close reason
`landed <landed> through run <run>`.

### The lane

Every landing in a project queues on one lock, the project's lane, whether
you run `fleet land` yourself or a workflow runs it. The lock is the file
`lanes/lane-<project>.lock` in the machine directory. `[core.flight] lanes`
in the policy file names another directory to keep it in, read against the
machine directory when the path is relative.

A landing takes the lane before its first fetch and holds it until it exits,
whatever the exit. While another landing holds it, the landing prints the
holder's item and the time it took the lane, then waits for it with no
deadline:

```text
waiting on the lane: <other-item> since <time>
```

Where `lanes/<project>` exists in that directory and `lanes/lane-<project>`
does not, a landing moves the first to the second, lock and all, before it
takes the lane; one holding a `.git` is moved with `git worktree move`.

### The suite

`--test <command>` is the only suite a landing runs; the project's policy
names none. The command runs with `sh -c` in the worktree, under fleet's own
constructed `PATH`, with its output in the log. A red first reading waits for
the machine's load to fall under its ceiling, for at most
`[core.flight] rerun_wait_seconds` (300 unless set), and runs once more. A
second red refuses, prints the tail of both logs, and writes both readings
to the event stream:

```sh
$ fleet land <item> <redelivered> --test "echo failing; exit 3" --by <reviewer-name>
...
4. suite            RED        `echo failing; exit 3` rc 3 in <took>, read from the child's own exit; log <fleet-dir>/land/<item>/suite.log
5. suite rerun      RED        `echo failing; exit 3` rc 3 in <took>, read from the child's own exit; log <fleet-dir>/land/<item>/suite.2.log; reading 2 — the box was already quiet — load <load> against a ceiling of <ceiling>, no wait
reading 1 — <fleet-dir>/land/<item>/suite.log
  failing
reading 2 — <fleet-dir>/land/<item>/suite.2.log
  failing
fleet land: the suite `echo failing; exit 3` exited 3 and, rerun once, 3 — the logs are at <fleet-dir>/land/<item>/suite.log and <fleet-dir>/land/<item>/suite.2.log
```

It exits 1. A landing handed no `--test` runs nothing and lands on the review
alone. Its suite row reads `NOT TESTED`, and so does the `landed` entry's
`test:` line:

```text
    test: NOT TESTED — no test command was handed to this landing (`fleet land --test <command>`), so nothing ran and it stands on the review alone
```

### `--also`

`--also <path>` admits a file of your own into the landing. Without it, any
changed file in the worktree refuses the landing, and a staged set that
differs from the delivery's refuses it with both sets printed.

### The work branch

Row 6 classifies the work branch the delivery names. `SAFE` means its tip is
the reviewed commit and the delivered paths landed unchanged; only then are
the branches deleted. Each side reports on standard error when it cannot be
deleted, and the exit stays 0:

```text
work branch <work-branch>: SAFE, local kept — `git branch -D` exited 1: error: cannot delete branch '<work-branch>' used by worktree at '<seat-worktree>'; `fleet seat retire` deletes it off the landed entry when the seat holding it goes
work branch <work-branch>: SAFE, origin kept — origin carries no refs/heads/<work-branch>
```

Any other classification keeps the branch and says why.

### When a landing stops

A refusal after `land/<item>` is cut and before the push resets the
worktree, detaches it onto `origin/main` and deletes `land/<item>`; any
`--also` file goes back to what it held. A squash that conflicts prints
`RETURN FOR REBASE` and the conflicted paths. A trunk that moved prints
`REBASE NEEDED: origin/main moved (<n>)`. Both exit 1.

A landing that refuses before the push appends no entry and puts no hold on
the item: the item stays open, held by you, with its verdict standing. You
run `fleet land` again, or return the item, as the table under
[When it refuses](#when-it-refuses) says.

Once the push has run, nothing is put back, whether or not the push
succeeded. A rejected push prints what the remote said, exits 1, and leaves
the worktree on `land/<item>`; the remote's output is kept in `push.out`. A
failure after a push that landed — the landed entry, a line on the stream,
or the close — names the step that failed and says the landing stands on
`main`.

## Reading the record

`fleet item show <item>` prints the item and its timeline: the item's
fields, its description, then one block per entry in the store's order, each
opening on its time, its actor and a summary. After a full pass it reads:

```sh
$ fleet item show <item>
<item> · Add a greeting  [closed]
type task · labels none · assignee <reviewer>
order dispatch by seat:<you> at <time>, seat <seat>

Write GREETING with one line.

timeline (6 entries)
<time>  seat:<you>  ordered dispatch → <seat>
<time>  seat:<seat>  delivered <commit> on <work-branch>, base <base>
...
<time>  seat:<reviewer>  returned <commit> with 1 finding(s)
...
<time>  seat:<seat>  delivered <redelivered> on <work-branch>, base <base>
...
<time>  seat:<reviewer>  accepted <redelivered>
...
<time>  seat:<reviewer>  landed <landed> (range <old>..<landed>; squash of <redelivered>)
...
```

It writes nothing and exits 0. `order` reads `none` on an item with no order
index, and `unreadable` where `fleet.orders` is not an object at `v` 1. The
item's assignee is always a seat's full id. `held` and `cleared` entries sit
where a question stopped the work; an `order_withdrawn` entry follows an
order a refused spawn or a retired seat took back.

`--json` prints one document: `id`, `title`, `description`, `status`,
`type`, `labels`, `assignee`, `order` (the index's `by`, `kind`, `seat` and
`at`, `{"unreadable": true}`, or `null`), `blockers`, and `timeline`, one
object per entry carrying its `id`, `at`, `by` (as `{"kind", "id"}`), `kind`
and the kind's own fields.

An item the store does not hold is exit 1:
`fleet item show: <item>: no issues found matching the provided IDs`. A
comment on the item that carries fleet's entry key and does not read as an
entry makes the whole read exit 3, naming the comment — every verb that
reads the timeline refuses the same way:
`fleet item show: <item>'s comment <comment> carries fleet.entry and does not read: missing field `hold``.

### What each verb writes to the stream

Each verb signals the entry it wrote on the event stream, after the entry
has been read back, by one line of one kind, `item.entry`, which
[Status and the event stream](status.md) covers. Its `actor` is the entry's,
as an object: `seat:<you>` is `{"kind":"seat","id":"<you>"}`, and on a line
the controller writes it is `{"kind":"controller","id":"<identity>"}`,
`<identity>` being this machine's identity. Its payload is `item`, `entry`
(the entry's id) and `kind`, and nothing else; what the entry says is read
off the record.

| Verb | Lines |
| --- | --- |
| `dispatch` | `item.entry` for the `ordered` entry that names the seat; nothing for a refused spawn's `order_withdrawn` |
| `deliver` | `item.entry` for `delivered` |
| `review --land`, `review --return` | `item.entry` for `reviewed` |
| `hold` | `item.entry` for `held` |
| `clear` | `item.entry` for `cleared` |
| `land` | `check.read` per suite reading, or one reading `none` without `--test`, then `item.entry` for `landed` |

`check.read` carries `item`; `suite`, the `--test` command; `rc`, what it
exited; `verdict`, `green` or `red`; `reading`, 1 or 2; `log`, the reading's
log file; and `path`, the search path the command ran under. A landing
handed no `--test` writes one `check.read` with `suite`, `rc`, `log` and
`path` null and `verdict` `none`. `rc` is null where the command was killed
by a signal.

### `--json`

`dispatch`, `deliver`, `hold`, `clear`, `review` and `land` take `--json`.
Standard output then carries one document and nothing else; the human lines
move to standard error, and the exit is unchanged:

```sh
$ fleet deliver --delivery <delivery-file> --json
{"ok":true,"verb":"deliver","data":{"commit":"<redelivered>","entry":"<entry>","item":"<item>","state":"delivered"}}
```

`data` carries the item, its `state` — the kind of the entry the verb wrote:
`ordered`, `delivered`, `held`, `cleared`, `reviewed` or `landed` — and
`entry`, that entry's id. `review` adds `verdict`, `accepted` or `returned`;
`review --show` writes no entry, and its `state`, `verdict` and `entry` are
`null`. Beside them, `dispatch` gives the `seat` as its object, `deliver`
the `commit`, `hold` and `clear` the `hold`, and `land` the `sha`. A refusal
is
`{"ok":false,"verb":"<verb>","refusal":{"code":"<code>","why":"<message>"}}`,
where the code names the exit. An order that stands unrung is a refusal
document too: with the code `no_session` when the seat has no live session,
and `refused` when the ring failed.

## When it refuses

Exits follow the table every command shares; see
[Exit codes and conventions](conventions.md).

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `--by` or `FLEET_ACTOR` names no seat | 1 | `fleet <verb>: --by <arg> names no seat — the seats are <machine-name> (<id>), …` (or `FLEET_ACTOR <arg> …`) | Name a seat from the list, or a typed actor |
| `--by` or `FLEET_ACTOR` names more than one seat | 1 | `fleet <verb>: --by <arg> names <n> seats — <machine-name> (<id>), … — say more of the id` | Give more of the id |
| `--by` is empty | 2 | `fleet <verb>: --by names no seat — the argument is empty` | Name a seat |
| `--by` or `FLEET_ACTOR` is a typed actor with a bad id | 1 | ``fleet <verb>: `<value>` is a typed actor with a bad id — …`` | Give the whole id |
| No `--by` or `FLEET_ACTOR`, and an `identity.toml` that does not read | 3 | `fleet <verb>: could not tell who acts: <fleet-dir>/identity.toml: <why>` | Fix the file, or pass `--by` |
| No fleet above the current directory | 3 | ``no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one`` | Run from inside the project |
| The policy file sets `[gates] suite` or `[gates] touched` | 1 | `[gates] suite is not project policy, and nothing reads it — a test command is the workflow's: set …, and delete the key`, and the `[gates]` line below | Delete the key and the `[gates]` table; pass `--test` or `--touched` |
| The policy file carries a `[gates]` table, even an empty one | 1 | ``[gates] is not a policy table, and nothing reads it — its keys are set by purpose: `ci_marker` under [landing], `tool_commands` under [permissions], and `release_ref_glob` and the `prod_*` lists under [guards.targets]; move each one there, and delete the table`` | Move each key to the table named, and delete `[gates]` |
| A comment on the item carries `fleet.entry` and does not read | 3 | `<item>'s comment <comment> carries fleet.entry and does not read: <why>` | Read the comment; fleet will not answer from a record with a hole in it |
| `item show` of an item not in the store | 1 | `<item>: no issues found matching the provided IDs` | Check the id |
| `dispatch` of an item that is blocked | 1 | `<item> is not ready — it is blocked by <other>` | Finish the blocker |
| `dispatch` of an item that is not open | 1 | ``<item> is not ready — its status is `<status>` `` | Pick a ready item |
| `dispatch` of an epic | 1 | `<item> is an epic, and an epic is never dispatched — its children are` | Dispatch its children |
| `dispatch` of an item not in the store | 1 | `<item>: no issues found matching the provided IDs` | Check the id |
| `dispatch` of an item already ordered | 1 | `<item> already carries an order — kind=dispatch by=seat:<you> at=<time>` | Nothing: it is given |
| `dispatch` of an item whose `fleet.orders` is not an object at `v` 1 | 3 | ``<item> carries a `fleet.orders` this fleet cannot read — it is not an object at v 1 — and a dispatch will not guess whether it is an order`` | Read the key; dispatch with the fleet that wrote it |
| `dispatch --to` a seat the machine does not run | 1 | `<arg> names no seat — the seats are <machine-name> (<id>), …`, listing the seats this machine runs | Name a seat from the list |
| `dispatch --to` an argument more than one running seat answers to | 1 | `<arg> names <n> seats — <machine-name> (<id>), … — say more of the id` | Give more of the id |
| `dispatch --to` a seat holding open work | 1 | `` `<seat-name>` already holds <item> (open) — one item at a time `` | Wait, or pick another seat |
| `dispatch` rang no live session | 4 | `ORDERED, NOT RUNG: no live session for <seat-name>; the order stands …` | Nothing: the order stands |
| `dispatch` ring failed | 1 | `ORDERED, NOT RUNG: <cause>` | Nothing: the order stands |
| `dispatch` spawn refused | 1 | `<item> was not dispatched — <cause>; the order was withdrawn` | Retry later |
| `dispatch` spawn could not be observed | 3 | `<item> may or may not have been dispatched — <cause>; the order stands` | Check the seat, then retry |
| `brief` of an item with no order | 1 | `<item> carries no order index — a brief for an unordered item would tell a seat it may begin when nothing said so` | Dispatch it first |
| `deliver` on `main` | 1 | `` the worktree is on `main` — a delivery is a handoff of a work branch … `` | Work on a work branch |
| `deliver` with a file outside the staged set | 1 | `` `<path>` is changed in the working tree and not staged — … `` | Stage it or put it back |
| `deliver` with nothing staged at the base | 1 | `nothing is staged in <worktree> and HEAD is origin/main at <sha> — there is no commit to deliver. …` | Stage the work |
| `deliver --note` or `hold --note` | 2 | `--note is gone: a delivery is a JSON file — fleet deliver --delivery <file>; …` (or `a question is a JSON file — fleet hold --question <file>; …`) | Write the JSON file the brief shows |
| `deliver`, `hold` or `review --return` with a file that cannot be read | 2 | `the <delivery, question or findings> at <file> could not be read: …` | Fix the path |
| `deliver`, `hold` or `review --return` with a file that is not its shape | 2 | `the <delivery, question or findings> at <file> is not the shape assets/<name>.schema.json gives: <what> — the brief shows that schema` | Match the schema |
| `deliver` or `hold` by a seat holding no ordered item | 1 | `` `seat:<seat>` holds no open ordered item — … `` | Check `--by` or `FLEET_ACTOR` |
| `deliver` or `hold` by a seat holding two | 1 | `` `seat:<seat>` holds 2 ordered items — <ids> — and `--item <id>` says which one this is `` | Pass `--item` |
| `deliver` or `hold` by an actor that is not a seat, with no `--item` | 1 | `<kind>:<id> is not a seat, so it holds nothing — pass --item <id>` | Pass `--item` |
| `deliver` or `hold` with an `--item` the actor does not hold | 1 | `<item> is held by <assignee> and not by <actor> — --item names an item the acting seat holds` | Name your own item |
| `deliver` with no reviewer in the policy | 1 | ``no `[core] reviewer` in this fleet's policy — a delivery has nowhere to go without one`` | Set `[core] reviewer` |
| `deliver` or `land` with a `[core] reviewer` that names no listed seat, or more than one | 1 | `[core] reviewer = "<value>" <value> names no seat — the seats are …` (or `names <n> seats — …`) | Set it to one seat's name, machine name or id |
| `hold` on `main` | 1 | `` the worktree at <dir> is on `main` — a park records the branch the work is on … `` | Work on a work branch |
| `clear` of an item with no open hold | 1 | `<item> carries no open hold — a clearance settles a question somebody asked, and this item has none` | Check the id |
| `clear` of a hold already cleared | 1 | `<item>'s hold <hold> is not one the store lists open — it has been cleared already, or by hand` | Nothing to do |
| `clear` with a letter not offered | 2 | `` the question on <item> names no option `<L>` — its options are A, B, and a letter outside them needs `--text <text>` saying what was decided `` | Pick a letter, or add `--text` |
| `clear` with more than one letter | 2 | `` `<arg>` is not a letter — … `` | Give one letter |
| `review` of an item with no delivery | 1 | `<item> carries no delivery — a review reads one and there is none to read` | Wait for the delivery |
| `review --land` or `--return` by a seat not holding the item | 1 | `<item> is held by <assignee> and not by seat:<you> — a verdict is the holder's, and a delivered item's holder is its reviewer` | Review as the holder |
| `review --land` or `--return` by a routine or the controller | 1 | `a <kind> writes no verdict` | Review as the holder |
| `review --return` with a findings file that lists none | 2 | `the findings at <file> (read against assets/findings.schema.json) numbers no finding — a return that numbers nothing is a question` | Name a finding, or ask the question |
| `review --return` with no seat in the order | 1 | `<item>'s order index names no seat — …` | Reassign by hand |
| `review` with two modes | 2 | ``the argument '--land' cannot be used with '--return <FILE>'`` | Pick one mode |
| `land` given a branch name | 2 | `` `<name>` is not a commit — a commit is 7 to 40 hex characters, … `` | Pass the commit |
| `land` given a commit this checkout lacks | 2 | `` `<sha>` resolves to no commit in this checkout `` | Fetch, then retry |
| `land` with a changed file in the worktree | 1 | `` `<path>` is changed in the working tree — … `--also <path>` is how a path of the reviewer's own is admitted `` | Clean it, or pass `--also` |
| `land` of a closed item | 1 | `<item> is closed — a landing closes an item and cannot close one twice` | Nothing to do |
| `land` by a seat not holding the item | 1 | `` <item> is held by `<holder-name>` and not by `<your-name>` — whoever closes an item lands its work ``, each seat by its machine name | Land as the holder |
| `land` by a routine or the controller | 1 | `fleet land acts as a seat or as a run — <kind>:<id> is neither` | Land as the holder |
| `land` by a run with no licence | 1 | `run <run> raised no hold about <item> — …`, or why the hold does not license it | See [Runs and workflows](runs.md) |
| `land` with no verdict, or a return last | 1 | `<item> carries no verdict — …` or `the last verdict on <item> is a return — …` | Review it |
| `land` of a commit the verdict does not name | 1 | `the last verdict on <item> accepts <sha> and this landing was given <sha> — a landing lands the commit the review read` | Land the accepted commit |
| `land` of an accept another actor wrote | 1 | `the last verdict on <item> was written by <actor>, and this landing closes as <actor> — a landing lands its own reviewer's accept` | Review it as the lander |
| `land` from the primary with no reviewer row | 1 | `` [core] reviewer is `<reviewer-name>` (<reviewer>), and <fleet-dir>/config.json carries no row for it — … `` | Run from the reviewer's worktree |
| `land` from the primary with no reviewer worktree | 1 | `` `<reviewer-name>` carries no `<project>` worktree in <fleet-dir>/config.json — … `` | Run from the reviewer's worktree |
| `land` squash conflicts | 1 | `RETURN FOR REBASE`, the paths, and `the squash of <sha> conflicts with origin/main — return the item: …` | Return the item |
| `land` staged set differs | 1 | `STAGED` and `DELIVERED` lists, and `the staged set is not the delivered set — …` | Return, or pass `--also` |
| `land` suite red twice | 1 | both log tails, and ``the suite `<command>` exited <rc> and, rerun once, <rc> — …`` | Read the logs |
| `land` trunk moved | 1 | `REBASE NEEDED: origin/main moved (<n>)` | Rebase the work, then land |
| `land` push rejected | 1 | the push's output, and `the push to origin/main exited <rc> — nothing after it ran: …` | Read `push.out` |
| A write that did not read back | 3 | what the record holds against what the verb wrote, or which write did not land and what STANDS; for an assignee, an order index or a close, a `READ:` line | Read the item with the `READ:` line, then re-run the verb or report it |

## See also

- [Runs and workflows](runs.md): how a workflow calls these verbs, how a
  held item resumes, and what licenses a run's landing.
- [The controller and seats](seats.md): named and transient seats, and the
  sessions a ring reaches.
- [Packs](packs.md): the files behind the brief, the rules and the three
  JSON shapes.
- [Status and the event stream](status.md): reading the lines these verbs
  write.
- [Exit codes and conventions](conventions.md): the exit table and full item
  ids.
