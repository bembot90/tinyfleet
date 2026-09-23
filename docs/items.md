# Items and the record

An item is one piece of work in the project's work graph, the bd store beside
the project. Seven verbs move it: `fleet dispatch` gives it to a seat,
`fleet brief` renders what that seat reads first, `fleet deliver` hands the
work over, `fleet ask` and `fleet answer` stop it on a question and settle
the question, `fleet review` reads the delivery and writes a verdict, and
`fleet land` puts the reviewed commit on the trunk and closes the item. Every
verb that writes, writes a note on the item and reads it back before it exits
0, so the item's notes, in order, are its record.

## Terms

- **Item**: one entry in the project's bd store, named by its id.
- **Note**: one entry in an item's notes. The order note ends in
  `orders given`; every other note opens on a marker word at column zero —
  `DELIVERED`, `ACCEPTED`, `LANDED` and the rest — and runs to the next
  marker of another kind.
- **The record**: the item's notes in order, plus its assignee and its order
  index.
- **Order**: the note `dispatch` writes, ending in `orders given`, together
  with a machine-read copy of it in the item's metadata under `orders`. An
  item with no order note has not been given to anybody.
- **Brief**: the first turn a dispatched seat reads, rendered from the item.
- **Ring**: one message sent to a seat's live session, naming the item and,
  from `dispatch`, where its brief is.
- **Work branch**: the branch a seat builds on. The trunk is `main`, and fleet
  reads it as `origin/main`.
- **Reviewer**: the seat named by `[core] reviewer` in the fleet's policy
  file. A delivery goes to it.
- **Verdict**: the note `review` writes: `ACCEPTED`, or
  `RETURNED WITH FINDINGS`.
- **Park**: the state `ask` puts an item in, with the store's own gate raised
  on it carrying the question. A **gate** blocks the item until someone
  answers it. [Runs and workflows](runs.md) covers how a workflow resumes a
  parked item.
- **Lane**: the queue a project's landings take one at a time.

## Saying who acts

Every note names who wrote it. Each writing verb takes `--by <name>`, and
without it reads `FLEET_ACTOR`, then `BEADS_ACTOR`. With none of the three,
the verb writes nothing and exits 2:

```sh
$ fleet dispatch <item> --to <seat>
fleet dispatch: no dispatcher — pass --by <name>, or set FLEET_ACTOR or BEADS_ACTOR. An order names who gave it.
```

`fleet review` asks for a name in every mode, `--show` included, although
`--show` writes nothing. `fleet brief` writes nothing and takes no `--by`.

The seat verbs (`deliver`, `ask`) use the name to find the item: it is the
one item assigned to that name, open or in progress, that carries an order.
A seat holding two names one with `--item <id>`. Given `--item`, the verb
acts on that item without checking who holds it.

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
store's ready set (open, and blocked by nothing) and must carry no order yet.

```sh
$ fleet dispatch <item> --to <seat> --by <you>
dispatched by <you> — orders given
```

It exits 0. In one act it writes three things and reads all three back:

- the assignee: `<seat>`;
- the order note on the item: `dispatched by <you> — orders given`;
- the order index in the item's metadata, `orders`: `by`, `kind` (`dispatch`),
  `seat` and `at`.

Then it writes `item.dispatched` to the event stream, naming the item and the
seat, and the brief to `<fleet-dir>/briefs/<item>.md`. Last, it rings the
seat: one message to the seat's live session saying the item is theirs and
where the brief is.

### When the seat has no live session

The order stands and nothing is undone. The order line is not printed; you
get the brief's path and the reason on standard error, and the exit is 4:

```sh
$ fleet dispatch <item> --to <seat> --by <you>
brief: <fleet-dir>/briefs/<item>.md
fleet dispatch: ORDERED, NOT RUNG: no live session for <seat>; the order stands and the seat's successor reads it at wake
```

A ring that fails for any other reason is exit 1, with
`ORDERED, NOT RUNG:` and the cause. In both cases the item is ordered;
dispatching it again refuses, because it carries an order.

### Without `--to`

With no `--to`, fleet writes the order first and then asks for a transient
seat to hold it. The brief names the seat `(transient)`. When the seat
starts, the item is assigned to it, its name joins the order index, and the
output is the order line followed by the two lines `fleet seat spawn`
prints: the machine's load average and the transient seats mid-turn. When
the spawn is refused, fleet withdraws the order: it removes the order index,
adds the note `DISPATCH WITHDRAWN — spawn refused: <cause>`, and exits 1.
When fleet cannot tell whether the seat started, the order stands under the
note `DISPATCH COULD NOT TELL — the spawn could not be observed: <cause>`, and
the exit is 3. [The controller and seats](seats.md) covers transient seats.

### `--touched`

`--touched <command>` names the builder's gate: the command the seat runs
over its own diff before it delivers. It appears in the brief's "Your gate"
section. Without it, that section says the dispatch named no command and
tells the seat to run only the suites its diff reaches.

## Rendering a brief

`fleet brief` prints the brief for an ordered item on standard output and its
size on standard error. It writes nothing.

```sh
$ fleet brief <item> --to <seat>
# <item> — your first turn

You are `<seat>`, working on `<project>`. This page is everything you were
given. Read it once, in full, before your first act.
...
```

Standard error carries one line, `brief: <n> bytes`. It exits 0. Without
`--to`, the seat reads `(transient)`. `--touched <command>` fills the gate
section as it does on `dispatch`.

The brief carries the order note, the item as `bd show` prints it, the gate,
one line per guard class saying `on` or `off`, the rules every seat works
under, and the delivery-note grammar. It is rendered whole or not at all: a
template that cannot render prints nothing and exits 3. An item with no order
note is refused with exit 1.

The brief and every note's shape come from template files in the pack
layers, so an installed pack can replace any of them; see [Packs](packs.md).
Every item verb takes `--packs-dir <dir>` to read the packs from another
directory, and then reads the defaults from the `defaults` directory beside
it.

## Delivering work

`fleet deliver` hands the work over from inside the seat's worktree. The
delivery is the staged set: stage what you are handing over, write the
delivery note, and run it.

```sh
$ fleet deliver --note <note-file>
DELIVERED, NOT RUNG: no live session for <reviewer>; <item> is theirs and their successor reads it at wake
```

It exits 0. The line above is what you see when the reviewer has no live
session; when the ring reaches the reviewer, nothing is printed, and when the
ring fails, standard error carries `DELIVERED, NOT RUNG: <cause>; the
delivery stands`. The exit is 0 in all three cases.

In order, it:

1. commits the staged set on the work branch, with the message
   `<item>: delivered by <seat>`;
2. fills the note's first line and its `commit:`, `branch:` and `base:` lines;
3. reassigns the item to the reviewer, adds the note, and reads both back;
4. writes `item.delivered` to the event stream;
5. rings the reviewer.

The note on the item reads:

```text
DELIVERED <commit> — <seat>
commit: <commit>
branch: <work-branch>
base: origin/main at <base>, read at <time>
files:   GREETING
...
```

`base:` is `origin/main` as your checkout last fetched it. `deliver` does not
fetch.

### The note

The note is a file you write. Its first non-blank line opens on `DELIVERED`,
or on `RE-DELIVERED` for a delivery after a return. It carries every label
the delivery-note template names, each at column zero. The defaults' template
names these:

```text
DELIVERED
commit:
branch:
base:
files:   <the paths this delivery touched>
gate:    <each acceptance check, with its result>
suite:   <the suite that ran, and its exit>
spec corrections: <N, or none>
not proven: <what this delivery does not establish>
decisions: <N, or none>
  D1 <the call made>; not taken: <the alternative>; because <why>
covers: <requirements covered, or none>
```

`deliver` replaces the first line and whatever `commit:`, `branch:` and
`base:` hold, keeping the spaces after each colon, or one space where there
are none. Everything else reaches the item as you wrote it. `fleet review
--land` checks the `decisions:` count against the indented `D<n>` lines under
it.

### A parked commit

A worktree resumed after `fleet ask` holds its work in the parked commit and
has nothing to stage. With nothing staged and HEAD at any commit other than
the tip of `origin/main`, `deliver` delivers HEAD as it stands and commits
nothing:

```sh
$ fleet deliver --note <note-file>
DELIVERED AS-IS: nothing was staged and HEAD <parked-commit> is ahead of origin/main at <trunk-tip> — the delivery on <parked-item> is that commit and this verb committed nothing
```

The line says "ahead" whatever HEAD's relation to `origin/main`: a HEAD
behind it, or on another line of history, is delivered the same way. With
nothing staged and HEAD at the tip of `origin/main`, there is no work to hand
over and it refuses.

### What it refuses before committing

These are all read before anything is written, so a refusal leaves the
worktree as it stood: the trunk branch, a changed or untracked file outside
the staged set, nothing staged at the base, a note it cannot read, a note
that opens on another word or drops a label, a seat holding no ordered item
or more than one, and a fleet with no `[core] reviewer`. The table under
[When it refuses](#when-it-refuses) gives each message.

## Asking a question

`fleet ask` stops the work on a question for a person. Write the question in
a file: `QUESTION` and the question on the first line, then one option per
line as a capital letter, a period and the text. Anything else in the file
travels with the question.

```text
QUESTION Should the second item print to stdout or to a file?
A. stdout, one line
B. a file named OUT

Context: the item does not say.
```

Run it from the seat's worktree:

```sh
$ fleet ask --note <question-file>
<gate>
```

It prints the gate's id and exits 0. In order, it:

1. commits everything the worktree holds on the work branch — staged,
   modified and untracked alike — with the message
   `<parked-item>: parked — <seat> asked a question at <time>`; a tree with
   nothing to commit parks on HEAD;
2. raises a gate in the store blocking the item, with the whole note as its
   reason;
3. writes the park note on the item and reads it back;
4. writes `item.parked` to the event stream.

The park note carries the question moved two spaces in, so no line of it can
end the note:

```text
PARKED <parked-item> — ask
branch:  <work-branch>
commit:  <parked-commit>
gate:    <gate>
  QUESTION Should the second item print to stdout or to a file?
  A. stdout, one line
  B. a file named OUT

  Context: the item does not say.
```

The item leaves the store's ready set while the gate is open. `ask` rings
nobody and dispatches nothing; the item stays assigned to the seat and keeps
its order.

It refuses the trunk branch, a seat holding no ordered item, and a note that
does not open on `QUESTION` or names no option, all before it commits.

## Answering a question

`fleet answer` settles a parked item's question: the item, then the letter
of the option chosen.

```sh
$ fleet answer <parked-item> b --text "a file, but name it OUT.txt" --by <you>
<parked-item> answered B — <gate> resolved
```

It exits 0. The letter is read without regard to case. It writes the answer
note, reads it back, resolves the gate, checks that the store's list of open
gates does not carry it, and writes `gate.resolved` to the event stream:

```text
ANSWERED <gate> — <you>
letter:  B
text:    a file, but name it OUT.txt
```

`--text` says what you decided beyond the option. Without it, `text:` reads
`(none)`. A letter the question does not offer is an answer only with
`--text`; without it, `answer` exits 2 and lists the letters on offer.

Answering returns the item to the store's ready set and dispatches nothing.
The item still carries its order and its assignee, so `fleet dispatch`
refuses it; the seat that resumes it delivers it (see
[A parked commit](#a-parked-commit)).

It refuses an item with no park, a park whose gate the store does not list
open, and an argument that is not a single letter.

## Reviewing a delivery

`fleet review <item>` reads the item's last delivery. It takes one of three
modes: `--show` (the default), `--land`, or `--return <file>`. Every mode
first prints the size line, measured from the delivery's recorded base to
its commit:

```text
size: 1 file(s), +1, -0 — tests: no, executable: no
```

`tests:` is `yes` when a changed path sits under a `test` or `tests`
directory or is named as a test file. `executable:` is `yes` when a changed
file is executable in your working tree. A delivery whose `base:` names no
commit is measured against `<commit>^`, and the line says so.

`fleet review` does not check who holds the item: any name given with
`--by` can write a verdict.

### Reading it

```sh
$ fleet review <item> --show --by <reviewer>
size: 1 file(s), +1, -0 — tests: no, executable: no
DELIVERED <commit> — <seat>
commit: <commit>
...
decisions: 1
  D1 plain text; not taken: markdown; because the item says a line
```

After the size line come the delivery note and then its `decisions:` block
again on its own. It writes nothing and exits 0.

### Returning it

Write the findings in a file, one per line, each starting `F1`, `F2` and so
on. Then:

```sh
$ fleet review <item> --return <findings-file> --by <reviewer>
size: 1 file(s), +1, -0 — tests: no, executable: no
RETURNED, NOT RUNG: no live session for <seat>; the return stands and their successor reads it at wake
```

It exits 0. It reassigns the item to the seat its order names, writes the
verdict and reads both back, writes `item.returned` to the event stream, and
rings that seat. The second line above appears only when the seat has no live
session; a ring that fails puts `RETURNED, NOT RUNG: <cause>; the return
stands` on standard error. The verdict carries the findings moved two spaces
in:

```text
RETURNED WITH FINDINGS <commit> — <reviewer>
findings: 1
item:    <item>
size: 1 file(s), +1, -0 — tests: no, executable: no
  F1 the greeting ends without a period
```

A findings file that numbers no finding exits 2, after the size line is
printed. The builder's next delivery opens its note on `RE-DELIVERED`;
`deliver` takes either marker.

### Accepting it

```sh
$ fleet review <item> --land --by <reviewer>
size: 1 file(s), +1, -0 — tests: no, executable: no
```

It exits 0. `--land` lands nothing: it writes the `ACCEPTED` verdict that
`fleet land` reads, and `item.reviewed` on the event stream. It walks the
delivery's decisions and accepts each one:

```text
ACCEPTED <redelivered> — <reviewer>
item:    <item>
size: 1 file(s), +1, -0 — tests: no, executable: no
decisions: D1 ACCEPT
  1 accepted, 0 overruled
```

`--land` refuses, with exit 1, a delivery with no `decisions:` line at column
zero, a count that is neither a number nor `none`, and a count that differs
from the `D<n>` lines under it. `--land` and `--return` together are a usage
error.

## Landing an item

`fleet land <item> <commit>` squashes the accepted commit onto the trunk,
pushes it, and closes the item. It takes a commit, 7 to 40 hex characters,
and never a branch name. It runs as the seat that holds the item, which after
a delivery is the reviewer, from a linked worktree. Run from the project's
primary checkout, it does its work in the reviewer's worktree for this
project, as the machine's seat list names it.

```sh
$ fleet land <item> <redelivered> --test "test -f GREETING" --reason "greeting added" --by <reviewer>
1. reviewed commit  PASS       <redelivered> — the last ACCEPTED verdict on <item> names it
2. staged set       PASS       1 path(s) outside .beads/, equal to the delivery's own set; .beads/issues.jsonl regenerated by the store's own export
3. CI marker        NONE       no [gates] ci_marker in this project — no marker is appended
4. suite            PASS       `test -f GREETING` rc 0 in <took>, read from the child's own exit; log <fleet-dir>/land/<item>/suite.log
5. base current     PASS       behind=0 against origin/main, counted in the same act as the push
6. work branch      SAFE       <work-branch> — its tip is the reviewed commit and the landed diff is empty
7. tree clean after PASS       git status is empty in <reviewer-worktree>
LANDED <landed>
```

It exits 0. Each row prints as it is read. The last line is `LANDED` and the
landed sha as the push printed it, abbreviated.

In order, it:

1. takes the project's lane, and prints `waiting on the lane:` with the
   holder's item and time when another landing holds it;
2. checks the worktree holds no changed file but the ones `--also` names;
3. checks the item is open and held by you, and that its last verdict is an
   `ACCEPTED` naming this commit;
4. fetches `origin`, cuts `land/<item>` at `origin/main`, and squashes the
   commit onto it;
5. regenerates the store's export and checks the staged set equals the
   delivery's own paths, outside `.beads/`;
6. runs `[gates] ci_marker`, where the project sets one, with the staged
   paths on its input, and appends what it prints to the commit subject;
7. commits, with the subject `<item>: <title>` and the trailers
   `Seat: <reviewer>` and `Implemented-by: <seat>`;
8. runs the `--test` command on the land branch;
9. fetches again, counts how far `origin/main` has moved, and pushes to
   `main` only where it has not;
10. writes the landing note and reads it back, writes one `gate.read` per
    suite reading and then `item.landed` to the event stream, and closes the
    item with the reason `landed <landed>`, followed by ` — ` and `--reason`
    where you gave one;
11. deletes the work branch, locally and on `origin`, only where the row
    says `SAFE`, puts the worktree back on `origin/main`, and deletes
    `land/<item>`.

The landing's own files sit in `<fleet-dir>/land/<item>/`: the commit
message, `suite.log` where a suite ran, `suite.2.log` on a rerun, and
`push.out` once the push has run.

The landing note's first line carries everything a script needs; the rows
and a block of commands that re-run each verdict follow it:

```text
LANDED <landed> on main by <reviewer> (range <old>..<landed>; squash of <redelivered>; implemented by <seat>) — suite: test -f GREETING, rc 0
1. reviewed commit  PASS       <redelivered> — the last ACCEPTED verdict on <item> names it
...
## Commands — every verdict above, re-runnable
git merge-base origin/main <redelivered>
...
```

Where the delivery's base is not the base it lands on, the first line adds
`; rebased from <base>` after the squashed commit.

### The suite

`--test <command>` is the only suite a landing runs; the project's policy
names none. The command runs with `sh -c` in the worktree, under fleet's own
constructed `PATH`, with its output in the log. A red first reading waits for
the machine's load to fall under its ceiling, for at most
`[core.flight] rerun_wait_seconds` (300 unless set), and runs once more. A
second red refuses, prints the tail of both logs, and writes both readings
to the event stream:

```sh
$ fleet land <item> <redelivered> --test "echo failing; exit 3" --by <reviewer>
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
alone. Its suite row reads `NOT TESTED`, and so does the note's first line:

```text
LANDED <landed2> on main by <reviewer> (range <landed>..<landed2>; squash of <parked-commit>; implemented by <seat>) — NOT TESTED: no test command was handed to this landing (`fleet land --test <command>`), so nothing ran and it stands on the review alone
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
work branch <work-branch>: SAFE, local kept — `git branch -D` exited 1: error: cannot delete branch '<work-branch>' used by worktree at '<seat-worktree>'; `fleet seat retire` deletes it off this note when the seat holding it goes
work branch <work-branch>: SAFE, origin kept — origin carries no refs/heads/<work-branch>
```

Any other classification keeps the branch and says why.

### When a landing stops

A refusal after `land/<item>` is cut and before the push resets the
worktree, detaches it onto `origin/main` and deletes `land/<item>`; any
`--also` file goes back to what it held. A squash that conflicts prints
`RETURN FOR REBASE` and the conflicted paths. A trunk that moved prints
`REBASE NEEDED: origin/main moved (<n>)`. Both exit 1.

Once the push has run, nothing is put back, whether or not the push
succeeded. A rejected push prints what the remote said, exits 1, and leaves
the worktree on `land/<item>`; the remote's output is kept in `push.out`. A
failure after a push that landed names the step that failed and says the
landing stands on `main`.

## Reading the record

`bd show <item>` prints the notes. After a full pass they read, top to
bottom:

```text
dispatched by <you> — orders given
DELIVERED <commit> — <seat>
...
RETURNED WITH FINDINGS <commit> — <reviewer>
...
RE-DELIVERED <redelivered> — <seat>
...
ACCEPTED <redelivered> — <reviewer>
...
LANDED <landed> on main by <reviewer> (...)
...
```

`PARKED` and `ANSWERED` notes sit where a question stopped the work. Each
verb also writes to the event stream, which
[Status and the event stream](status.md) covers:

| Verb | Event |
| --- | --- |
| `dispatch` | `item.dispatched` |
| `deliver` | `item.delivered` |
| `review --land` | `item.reviewed` |
| `review --return` | `item.returned` |
| `land` | `gate.read` per suite reading, or one reading `none` without `--test`, then `item.landed` |
| `ask` | `item.parked` |
| `answer` | `gate.resolved` |

### `--json`

`dispatch`, `deliver`, `ask`, `answer`, `review` and `land` take `--json`.
Standard output then carries one document and nothing else; the human lines
move to standard error, and the exit is unchanged:

```sh
$ fleet deliver --note <note-file> --json
{"ok":true,"verb":"deliver","data":{"commit":"<redelivered>","item":"<item>","state":"delivered"}}
```

`data` carries the item and its `state`: `dispatched`, `delivered`,
`parked`, `resolved`, `reviewed`, `returned` or `landed`, and `null` for
`review --show`. Beside them, `dispatch` gives the `seat`, `deliver` the
`commit`, `ask` and `answer` the `gate`, and `land` the `sha`. A refusal is
`{"ok":false,"verb":"<verb>","refusal":{"code":"<code>","why":"<message>"}}`,
where the code names the exit. An order that stands unrung is a refusal
document too, with the code `no_session`.

## When it refuses

Exits follow the table every command shares; see
[Exit codes and conventions](conventions.md).

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| No `--by`, `FLEET_ACTOR` or `BEADS_ACTOR` | 2 | `fleet <verb>: no dispatcher — pass --by <name>, or set FLEET_ACTOR or BEADS_ACTOR. …` (the noun varies by verb) | Name yourself |
| No fleet above the current directory | 3 | ``no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one`` | Run from inside the project |
| The policy file sets `[gates] suite` or `[gates] touched` | 1 | `[gates] suite is not project policy, and nothing reads it — a test command is the workflow's: set …, and delete the key` | Delete the key; pass `--test` or `--touched` |
| `dispatch` of an item that is blocked | 1 | `<item> is not ready — it is blocked by <other>` | Finish the blocker |
| `dispatch` of an item that is not open | 1 | ``<item> is not ready — its status is `<status>` `` | Pick a ready item |
| `dispatch` of an item not in the store | 1 | `<item>: no issues found matching the provided IDs` | Check the id |
| `dispatch` of an item already ordered | 1 | `<item> already carries an order — kind=dispatch by=<you> at=<time>` | Nothing: it is given |
| `dispatch --to` a seat the machine does not run | 1 | `` `<name>` is not a seat this machine runs — the seats it carries are <seats> `` | Name a seat from the list |
| `dispatch --to` a seat holding open work | 1 | `` `<seat>` already holds <item> (open) — one item at a time `` | Wait, or pick another seat |
| `dispatch` rang no live session | 4 | `ORDERED, NOT RUNG: no live session for <seat>; the order stands …` | Nothing: the order stands |
| `dispatch` ring failed | 1 | `ORDERED, NOT RUNG: <cause>` | Nothing: the order stands |
| `dispatch` spawn refused | 1 | `<item> was not dispatched — <cause>; the order was withdrawn` | Retry later |
| `dispatch` spawn could not be observed | 3 | `<item> may or may not have been dispatched — <cause>; the order stands` | Check the seat, then retry |
| `brief` of an item with no order | 1 | `<item> carries no order note — a brief for an unordered item would tell a seat it may begin when nothing said so` | Dispatch it first |
| `deliver` on `main` | 1 | `` the worktree is on `main` — a delivery is a handoff of a work branch … `` | Work on a work branch |
| `deliver` with a file outside the staged set | 1 | `` `<path>` is changed in the working tree and not staged — … `` | Stage it or put it back |
| `deliver` with nothing staged at the base | 1 | `nothing is staged in <worktree> and HEAD is origin/main at <sha> — a note with no commit is no delivery. …` | Stage the work |
| `deliver` or `ask` with an unreadable note | 2 | `the note at <file> could not be read: …` | Fix the path |
| `deliver` note opens on another word | 2 | `` the note opens on `<line>` — it opens on `DELIVERED` at column zero, or no reader can anchor on it `` | Open on `DELIVERED` |
| `deliver` note drops a label | 2 | `` the note carries no `<label>:` line, which `assets/delivery-note.md` names — … `` | Add the line |
| `deliver` or `ask` by a seat holding no ordered item | 1 | `` `<seat>` holds no open ordered item — … `` | Check `--by` |
| `deliver` or `ask` by a seat holding two | 1 | `` `<seat>` holds 2 ordered items — <ids> — and `--item <id>` says which one this is `` | Pass `--item` |
| `deliver` with no reviewer in the policy | 1 | ``no `[core] reviewer` in this fleet's policy — a delivery has nowhere to go without one`` | Set `[core] reviewer` |
| `ask` on `main` | 1 | `` the worktree at <dir> is on `main` — a park records the branch the work is on … `` | Work on a work branch |
| `ask` note with no option | 2 | `the note names no lettered option — …` and the template's example | Add `A.`, `B.` lines |
| `answer` of an item with no park | 1 | `<item> carries no park — an answer settles a question somebody asked, and this item has none` | Check the id |
| `answer` of a resolved gate | 1 | `<item>'s gate <gate> is not one the store lists open — it has been answered already, or resolved by hand` | Nothing to do |
| `answer` with a letter not offered | 2 | `` the question on <item> names no option `<L>` — its options are A, B, … `` | Pick a letter, or add `--text` |
| `answer` with more than one letter | 2 | `` `<arg>` is not a letter — … `` | Give one letter |
| `review` of an item with no delivery | 1 | `<item> carries no delivery — a review reads one and there is none to read` | Wait for the delivery |
| `review --return` with no numbered finding | 2 | `<file> numbers no finding — a return that numbers nothing is a question and goes back as one` | Number them `F1`, `F2` |
| `review --return` with no seat in the order | 1 | `<item>'s order index names no seat — …` | Reassign by hand |
| `review --land` with a mismatched count | 1 | `` <item>'s delivery says `decisions: <n>` and the walk found <m> — … `` | Return it |
| `land` given a branch name | 2 | `` `<name>` is not a commit — a commit is 7 to 40 hex characters, … `` | Pass the commit |
| `land` given a commit this checkout lacks | 2 | `` `<sha>` resolves to no commit in this checkout `` | Fetch, then retry |
| `land` with a changed file in the worktree | 1 | `` `<path>` is changed in the working tree — … `--also <path>` is how a path of the reviewer's own is admitted `` | Clean it, or pass `--also` |
| `land` of a closed item | 1 | `<item> is closed — a landing closes an item and cannot close one twice` | Nothing to do |
| `land` by a seat not holding the item | 1 | `` <item> is held by `<holder>` and not by `<you>` — whoever closes an item lands its work `` | Land as the holder |
| `land` with no verdict, or a return last | 1 | `<item> carries no verdict — …` or `` the last verdict on <item> is `RETURNED WITH FINDINGS` — … `` | Review it |
| `land` of a commit the verdict does not name | 1 | `the last verdict on <item> accepts <sha> and this landing was given <sha> — a landing lands the commit the review read` | Land the accepted commit |
| `land` from the primary with no reviewer worktree | 1 | `` `<reviewer>` carries no `<project>` worktree in <fleet-dir>/config.json — … `` | Run from the reviewer's worktree |
| `land` squash conflicts | 1 | `RETURN FOR REBASE`, the paths, and `the squash of <sha> conflicts with origin/main — return the item: …` | Return the item |
| `land` staged set differs | 1 | `STAGED` and `DELIVERED` lists, and `the staged set is not the delivered set — …` | Return, or pass `--also` |
| `land` suite red twice | 1 | both log tails, and ``the suite `<command>` exited <rc> and, rerun once, <rc> — …`` | Read the logs |
| `land` trunk moved | 1 | `REBASE NEEDED: origin/main moved (<n>)` | Rebase the work, then land |
| `land` push rejected | 1 | the push's output, and `the push to origin/main exited <rc> — nothing after it ran: …` | Read `push.out` |
| A write that did not read back | 3 | what the record holds, what the verb wanted, and, from `dispatch`, `deliver`, `review` and `land`, a `RERUN:` line | Run the `RERUN:` line, or write the note by hand |

## See also

- [Runs and workflows](runs.md): how a workflow calls these verbs, and how a
  parked item resumes.
- [The controller and seats](seats.md): named and transient seats, and the
  sessions a ring reaches.
- [Packs](packs.md): the templates behind the brief and every note.
- [Status and the event stream](status.md): reading the events these verbs
  write.
- [Exit codes and conventions](conventions.md): the exit table and full item
  ids.
