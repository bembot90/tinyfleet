# Exit codes and conventions

Every `fleet` command answers with an exit code from one table of seven rows,
so a script reading `$?` learns the same thing from every verb. This page is
that table, how a refusal reads on standard error and under `--json`, the few
verbs whose exit means something narrower, and the rule for naming an item:
by its full id.

## Terms

- **Exit table**: the seven exit codes, 0 to 6, that every verb shares. Each
  row has a number, which is the exit code, and a name, which is what the
  `--json` document carries.
- **Refusal**: a verb that did not do what it was asked, and said why. It
  says why on standard error and exits with the row that fits.
- **Could not tell**: exit 3. Something fleet needed to read could not be
  read, so it cannot say what happened. It is never a success.
- **Full id**: an item's id as the store prints it, prefix and suffix
  together: `ab-2td`, not `2td`.

## Reading an exit code

| Exit | Name | What it means | Where you meet it |
| --- | --- | --- | --- |
| 0 | `done` | The verb did what it was asked. | every verb |
| 1 | `refused` | The thing named is absent, is not in the state the verb needs, or already is so; or a check answered no. | `fleet dispatch` of an item that is not ready, `fleet event show` of an event that is not there, `fleet pack check` of a pack with defects, `fleet guard <class> --check` with a check not configured, `fleet routine check` of a routine that is not due, a `fleet seat nudge` whose seat is stopped at a question or did not take the text, a seat argument that names no seat or more than one |
| 2 | `usage` | The call itself is wrong: a missing or unknown argument, a question fleet would have to ask with no terminal to ask it on, or a verb typed under the wrong noun. | every verb |
| 3 | `could_not_tell` | Something the answer needs could not be read. | `fleet pack list` with an unreadable lock file, a verb that works on a project run outside any fleet, `fleet status` with a section it could not fill, a verb acting as an `identity.toml` that does not read |
| 4 | `no_session` | The seat has no live session. | `fleet event rest`, `fleet seat nudge`, `fleet seat feed`, and `fleet seat attach` for a seat fleet's tmux server holds no session for |
| 5 | `no_collector` | No controller is consuming this fleet: there is no projection, or, for `fleet seat nudge`, `fleet event rest` and `fleet event clear-halt`, the one there is older than three poll intervals. `fleet status` prints a stale projection and marks it `STALE`. `fleet event tail` and `fleet event show` answer 5 when there is no event stream at all. | `fleet status`, `fleet seat nudge`, `fleet event rest`, `fleet event clear-halt`, `fleet event tail`, `fleet event show` |
| 6 | `transient` | The seat is the wrong kind for the verb. | `fleet event rest` on a transient seat, and `fleet seat feed` or `fleet seat retire` on a named seat |

On the verbs that print a JSON envelope (see
[Reading the answer as JSON](#reading-the-answer-as-json)), `--json` never
changes the exit code: the same refusal exits with the same number with the
flag and without it. `fleet status --json` is the exception, below.

## Reading a refusal

A refusal is said on standard error, on a line that starts with `fleet` and
the verb:

```sh
$ fleet event show 1
fleet event show: no event 1
```

It exits 1. Standard output stays empty.

A verb that needs a projection and finds none names the path it looked at and
the next act:

```sh
$ fleet status
fleet status: there is no projection at <machine-dir>/projection.json to print (No such file or directory (os error 2)) — run `fleet start`
```

It exits 5.

`fleet pack check` names the pack instead of the command. A defect line
starts with the pack's name, or with its directory's name when the pack has no
readable manifest:

```sh
$ fleet pack check ./nopack
nopack: the pack directory cannot be read: No such file or directory (os error 2)
```

It exits 1.

### When fleet stops on an error of its own

`fleet observe` is the one verb that can stop on an error it did not turn into
a refusal. It prints its own line, then `fleet:` and the verb, then each cause
on a `caused by:` line:

```sh
$ fleet observe --once
fleet observe: cannot read policy at <policy-file>: No such file or directory (os error 2)
fleet: fleet observe
  caused by: the seat list could not be read, so no poll ran
```

It exits 3.

## Getting the call wrong

Exit 2 is about the call. You meet it in the shapes below. Three exits 2 are
not about the call: `fleet event woke`, `rest`, `handed-off`, `exited` and
`clear-halt` read their line back off the event stream after writing it, and
exit 2 when the stream's last line is not the one they wrote;
`fleet event show` exits 2 when the id is on more than one line of the
stream; and `fleet seat spawn`, and `fleet dispatch` with no `--to`, exit 2
when `[permissions] tool_commands` in `fleet.toml` is not a list of single
command words.

A missing, unknown or invalid argument. fleet prints `error:`, what was wrong,
the usage line and a pointer to `--help`:

```sh
$ fleet pack add
error: the following required arguments were not provided:
  --version <VERSION>
  <SOURCE>

Usage: fleet pack add --version <VERSION> <SOURCE>

For more information, try '--help'.
```

It exits 2. An unknown command, an unknown flag and two flags that cannot go
together (`fleet status --json --seat <seat>`) read the same way and exit 2.
A guard class fleet does not know prints `error:` and the reason, then the
pointer to `--help` with no usage line, and exits 2.

No command at all. fleet prints the help page on standard error:

```sh
$ fleet
one binary for the fleet: the controller, the seats, the packs

Usage: fleet [COMMAND]
...
```

It exits 2.

A family with no verb, such as `fleet pack` on its own, prints that family's
help page on standard error and exits 2. `fleet --help`, and `--help` on any
verb, prints the page on standard output and exits 0. `fleet --version`
prints the version number alone and exits 0.

A question with no terminal to ask it on. A verb that asks you something
refuses instead when standard input is not a terminal, and names the flag
that answers the question:

```sh
$ fleet create < /dev/null
fleet create: fleet: embedded or standalone? — stdin is not a terminal; answer it with --embedded
```

It exits 2. A prompt never waits on a script. One question has a default
instead of a refusal: `fleet create`'s store question, which takes `bd`, the
bd pack's store, when there is no terminal and no `--store`.

A verb under the wrong noun. `fleet seat woke`, `fleet seat rest`, `fleet seat
handed-off` and `fleet seat exited` name the spelling that works, whatever
follows them:

```sh
$ fleet seat rest orla --reason done
fleet seat rest: the seat noun is what is done to a seat — say fleet event rest
Usage: fleet [COMMAND]
```

It exits 2.

`fleet order`, with anything after it, does the same and names `fleet
routine`:

```sh
$ fleet order list
fleet order: the family is `fleet routine` now — use `fleet routine list | check | run | history`
```

It exits 2.

An empty actor. `fleet dispatch`, `deliver`, `hold`, `clear`, `review`,
`land`, `run`, `cancel` and `fleet seat retire` each name who acted,
`fleet review --show` included. Inside a fleet, an empty `--by` refuses with
exit 2 before the item is read or anything is written:

```sh
$ fleet review <item> --show --by ""
fleet review: --by names no seat — the argument is empty
```

It exits 2. A verb given no `--by` at all is not refused: it acts as
`FLEET_ACTOR`, or else as this machine's identity (see
[The default actor](#the-default-actor)).

## The default actor

A verb that names who acted and is given neither `--by` nor `FLEET_ACTOR`
acts as this machine's identity, the human seat kept in `identity.toml` in
the machine directory, minted where the machine has none. While the fleet's
`fleet.toml` does not list that seat, the verb says so on standard error and
carries on:

```sh
$ fleet review <item> --show
fleet review: acting as this machine's identity human-8a397d42 (01a0d602-37ef-72f3-b718-a4a98a397d42), which <project>/fleet.toml does not list — fleet seat add --human lists it
size: 1 file(s), +1, -0 — tests: no, executable: no
...
```

It exits 0: the line is not a refusal. No other variable is read. How `--by`
and `FLEET_ACTOR` are read is in
[Items and the record](items.md#saying-who-acts), and the identity in
[The controller and seats](seats.md#who-you-are-identitytoml).

## Reading the answer as JSON

`--json` asks for the outcome as one JSON document on standard output. These
verbs take it: `fleet dispatch`, `deliver`, `hold`, `clear`, `review` and
`land`; `fleet item show`; `fleet seat add`, `spawn`, `feed` and `retire`;
`fleet doctor`; and `fleet event tail` and `fleet event show`.

A success is `ok`, then `verb`, then `data`:

```sh
$ fleet event tail --json
{"ok":true,"verb":"event tail","data":{"actor":{"id":"01a0d5ff-b143-7781-9967-5ccd10b55fd3","kind":"seat"},"id":"<event-id>","kind":"seat.woke","payload":{},"seq":1,"ts":"<time>"}}
```

A seat in a document is an object, `{"id", "kind", "name"}`, with no `name`
where the seat has none; an event's `actor` is `{"id", "kind"}`.

A refusal is `ok`, then `verb`, then `refusal`, whose `code` is the exit
table's row name and whose `why` is the sentence the verb also prints on
standard error:

```sh
$ fleet dispatch dm-ncl --to orla --json
{"ok":false,"verb":"dispatch","refusal":{"code":"refused","why":"dm-ncl is not ready — its status is `closed`"}}
fleet dispatch: dm-ncl is not ready — its status is `closed`
```

It exits 1, the same as without the flag. The first line is standard output,
the second standard error. Under `--json` the item verbs move the lines they
would otherwise print on standard output to standard error, so standard output
carries the document and nothing else.

`fleet event tail --json` prints one success document per event, one per
line.

Two things under `--json` print no envelope:

- An error the argument parser finds, such as a missing argument, prints
  only the `error:` block on standard error and exits 2. A call fleet itself
  refuses, such as an empty `--by`, prints the refusal document with the code
  `usage`.
- `fleet status --json` prints the projection as it is stored. When there is
  no projection it prints nothing on standard output and exits 5. It reads
  nothing beyond the projection, so where `fleet status` exits 3 over a
  section it could not fill, `fleet status --json` prints the projection and
  exits 0.

## Verbs whose exit means something narrower

- **`fleet guard <class>`**, judging a payload on standard input, exits 0
  every time it reads its hook mapping. A refusal is JSON on standard
  output, one object for Claude Code; letting a command through prints
  nothing. A hook reads the output, not the exit. An `--adapter` it cannot read a hook mapping
  from exits 2, which Claude Code reads as blocking the call.
  `fleet guard <class> --check` is the one guard form with a verdict in its
  exit: 0 when every check is configured, 1 when one is not.
- **`fleet doctor`** exits with what its checks said: 0 when every check
  passed, 1 when one reported a finding, and 3 when one could not tell. 3
  wins over 1. Under `--json` the document's `ok` is `true` whatever the
  checks said, and `data.verdict` carries the same answer as `pass`,
  `finding` or `could_not_tell`. `ok` is `false` only when `fleet doctor`
  refuses before any check runs. See [Packs](packs.md#doctor-checks).
- **`fleet prime`** exits 0 every time. A part it cannot read is said in its
  place, and the rest still prints.
- **`fleet run`** exits with the workflow's outcome: 0 for a run that closed
  or is waiting to be woken, 1 for a run that failed, 3 for a run fleet could
  not read the outcome of. See [Runs and workflows](runs.md).
- **`fleet routine check`** exits 0 when the routine is due, 1 when it is
  not, and 3 when it could not tell.
- **`fleet status`** prints every section it can and exits 3 when one of them
  could not be read.

## Naming items by their full id

Type an item's full id wherever a verb takes one. The item verbs look the id
you type up in the store, and act on, write and print the full id the store
answers. The store also answers a suffix that names one item, so a suffix
reaches the item too, and what the verb writes names it in full:

```sh
$ fleet dispatch ncl --to orla
fleet dispatch: dm-ncl is not ready — its status is `closed`
```

It exits 1, naming `dm-ncl`. An id the store matches to no item is refused
as not in the store, with the last line the store's adapter printed on
standard error beside it. With the bd pack's store:

```sh
$ fleet dispatch zzz --to orla
fleet dispatch: zzz is not in the store (the adapter said: `zzz` is not in the store: no issues found matching the provided IDs)
```

It exits 1.

The commits fleet makes name the item by its full id and the actor by its
typed form. The commit `fleet deliver` makes has the subject
`<item>: delivered by seat:<id>`. When the worktree holds changes,
`fleet hold` commits them with the subject
`<item>: held — seat:<id> asked a question at <time>`. Both carry the full id
the store answers, including when you named the item to `--item` by its
suffix.

### The record guard's bare-id check

The record guard refuses a shell command whose text names an item by a bare
suffix, and its refusal names the full id to write instead. It has a prefix to
check against only where `[project] item_prefix` is set: in `fleet.toml` for
an embedded fleet, and in `.fleet/project.toml` for a standalone project.
`fleet create --embedded` does not write that key. To see whether the check
is on, run:

```sh
$ fleet guard record --check
record notes-replace: configured
record sql-write: configured
record bare-id: not configured — [project] item_prefix
```

It exits 1 while the key is missing. Which words the check takes for a bare
suffix, and how to run a command past it, are in [Guards](guards.md).

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| A required argument is missing, or an argument or flag is unknown | 2 | `error:` and the usage line | Run the verb with `--help` and add what it names. |
| No command given | 2 | the help page, on standard error | Name a command. |
| A verb asks a question and standard input is not a terminal | 2 | `fleet <verb>: fleet: <question> — stdin is not a terminal; answer it with <flag>` | Pass the flag it names. |
| A lifecycle word typed under `fleet seat` | 2 | `fleet seat <word>: the seat noun is what is done to a seat — say fleet event <word>` | Run `fleet event <word>`. |
| `fleet order` typed | 2 | ``fleet order: the family is `fleet routine` now — …`` | Run `fleet routine`. |
| An empty `--by` on a verb that names its actor | 2 | `fleet <verb>: --by names no seat — the argument is empty` | Name a seat, or leave `--by` out. |
| A seat argument that names no seat | 1 | `fleet <verb>: <arg> names no seat — the seats are <machine-name> (<id>), …` | Pick a seat from the list. |
| A seat argument that more than one seat answers to | 1 | `fleet <verb>: <arg> names <n> seats — <machine-name> (<id>), … — say more of the id` | Give more of the id. |
| An `identity.toml` that does not read, on a verb acting with no `--by` or `FLEET_ACTOR` | 3 | `fleet <verb>: could not tell who acts: <machine-dir>/identity.toml: <why>` | Fix the file, or pass `--by`. |
| A line written by `fleet event woke`, `rest`, `handed-off`, `exited` or `clear-halt` does not read back | 2 | `fleet event <verb>: the stream's last line reads … rather than the <kind> for <seat> just written; the record is not confirmed` | Read the stream with `fleet event tail` before writing it again. |
| A verb that works on a project, such as `fleet dispatch`, `fleet brief` or `fleet seat nudge`, run outside any fleet | 3 | ``fleet <verb>: no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one`` | Run it inside the project, or run `fleet create` there. |
| The event stream cannot be appended to | 1 | `fleet event <verb>: could not append to <machine-dir>/events.jsonl: <reason>` | Fix the stream file's path or permissions. |
| No projection, or, for `fleet seat nudge`, `fleet event rest` and `fleet event clear-halt`, one older than three poll intervals | 5 | `fleet <verb>: … no projection at <machine-dir>/projection.json …`, or `no collector is consuming — the projection at <machine-dir>/projection.json was generated at <stamp>, …` | Start the controller with `fleet start`. |
| `fleet event rest` for a seat with no live session | 4 | `fleet event rest: <seat> has no live session — its row reads <state>` | Nothing to stop; read the seat's row with `fleet status --seat <seat>`. |
| `fleet event rest` on a transient seat | 6 | ``… is a transient row, and only named seats rest — use `fleet seat retire <seat>` instead`` | Retire it. |
| `fleet seat feed` or `fleet seat retire` on a named seat | 6 | `<machine-name> is a named seat — named seats are rung and rested, and only a transient row is fed and retired` | Use `fleet seat nudge` or `fleet event rest`. |
| An item id the store matches to no item | 1 | `fleet <verb>: <id> is not in the store`, and `(the adapter said: …)` where the adapter printed a line on standard error | Type the full id. |

## See also

- [Items and the record](items.md): the verbs that take an item id, and the
  entry each appends to its timeline.
- [Guards](guards.md): the record guard's bare-id check in full.
- [Runs and workflows](runs.md): how a workflow's exit becomes `fleet run`'s.
- [Status and the event stream](status.md): `fleet status` and the event
  verbs whose refusals are the 5s above.
- [The controller and seats](seats.md): seat ids and names, this machine's
  identity, named and transient seats, and the rest and nudge that answer 4,
  5 and 6.
