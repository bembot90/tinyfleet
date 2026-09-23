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
| 1 | `refused` | The thing named is absent, is not in the state the verb needs, or already is so; or a check answered no. | `fleet dispatch` of an item that is not ready, `fleet event show` of an event that is not there, `fleet pack check` of a pack with defects, `fleet guard <class> --check` with a check not configured, `fleet routine check` of a routine that is not due, a `fleet seat nudge` the agent did not deliver |
| 2 | `usage` | The call itself is wrong: a missing or unknown argument, a question fleet would have to ask with no terminal to ask it on, or a verb typed under the wrong noun. | every verb |
| 3 | `could_not_tell` | Something the answer needs could not be read. | `fleet pack list` with an unreadable lock file, a verb that works on a project run outside any fleet, `fleet status` with a section it could not fill |
| 4 | `no_session` | The seat has no live session. | `fleet event rest`, `fleet seat nudge`, `fleet seat feed` |
| 5 | `no_collector` | No controller is consuming this fleet: there is no projection, or the one there is older than three poll intervals. `fleet event tail` and `fleet event show` answer 5 when there is no event stream at all. | `fleet status`, `fleet seat nudge`, `fleet event rest`, `fleet event clear-halt`, `fleet event tail`, `fleet event show` |
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

Exit 2 is about the call. You meet it in the shapes below. One exit 2 is not
about the call: `fleet event woke`, `rest`, `handed-off`, `exited` and
`clear-halt` read their line back off the event stream after writing it, and
exit 2 when the stream's last line is not the one they wrote.

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

It exits 2. An unknown command, an unknown flag, a guard class fleet does not
know and two flags that cannot go together (`fleet status --json --seat
<name>`) read the same way and exit 2.

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

It exits 2. A prompt never waits on a script.

A verb under the wrong noun. `fleet seat woke`, `fleet seat rest`, `fleet seat
handed-off` and `fleet seat exited` name the spelling that works, whatever
follows them:

```sh
$ fleet seat rest alpha --reason done
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

A missing actor. `fleet dispatch`, `deliver`, `ask`, `answer`, `review`,
`land` and `run` each name who acted, `fleet review --show` included. Given no
`--by` and neither `FLEET_ACTOR` nor `BEADS_ACTOR` set, each refuses with
exit 2 before it reads anything:

```sh
$ fleet dispatch ab-2td
fleet dispatch: no dispatcher — pass --by <name>, or set FLEET_ACTOR or BEADS_ACTOR. An order names who gave it.
```

It exits 2.

## Reading the answer as JSON

`--json` asks for the outcome as one JSON document on standard output. These
verbs take it: `fleet dispatch`, `deliver`, `ask`, `answer`, `review` and
`land`; `fleet seat spawn`, `feed` and `retire`; and `fleet event tail` and
`fleet event show`.

A success is `ok`, then `verb`, then `data`:

```sh
$ fleet event tail --json
{"ok":true,"verb":"event tail","data":{"actor":"alpha","id":"<event-id>","kind":"seat.woke","payload":{},"seq":1,"ts":"<time>"}}
```

A refusal is `ok`, then `verb`, then `refusal`, whose `code` is the exit
table's row name and whose `why` is the sentence the verb also prints on
standard error:

```sh
$ fleet dispatch 2td --to ghost --by me --json
{"ok":false,"verb":"dispatch","refusal":{"code":"refused","why":"2td is not ready — the store does not list it among the ready"}}
fleet dispatch: 2td is not ready — the store does not list it among the ready
```

It exits 1, the same as without the flag. The first line is standard output,
the second standard error. Under `--json` the item verbs move the lines they
would otherwise print on standard output to standard error, so standard output
carries the document and nothing else.

`fleet event tail --json` prints one success document per event, one per
line.

Two things under `--json` print no envelope:

- An error in the call itself, such as a missing argument, prints only the
  `error:` block on standard error and exits 2.
- `fleet status --json` prints the projection as it is stored. When there is
  no projection it prints nothing on standard output and exits 5. It reads
  nothing beyond the projection, so where `fleet status` exits 3 over a
  section it could not fill, `fleet status --json` prints the projection and
  exits 0.

## Verbs whose exit means something narrower

- **`fleet guard <class>`**, judging a payload on standard input, exits 0
  every time. A refusal is a JSON object on standard output; letting a
  command through prints nothing. A hook reads the object, not the exit.
  `fleet guard <class> --check` is the one guard form with a verdict in its
  exit: 0 when every check is configured, 1 when one is not.
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

Type an item's full id wherever a verb takes one. `fleet dispatch` compares the
id you type with the full ids in the store's ready list, so a bare suffix is
refused as not ready even when the item is ready:

```sh
$ fleet dispatch 2td --to ghost --by me
fleet dispatch: 2td is not ready — the store does not list it among the ready
```

It exits 1. The same item, by its full id, passes that check:

```sh
$ fleet dispatch ab-2td --to ghost --by me
fleet dispatch: `ghost` is not a seat this machine runs — the seats it carries are <seats>
```

That refusal is about the seat, not the item.

The commits fleet makes name the item by its full id. The commit
`fleet deliver` makes has the subject `<item>: delivered by <seat>`. When the
worktree holds changes, `fleet ask` commits them with the subject
`<item>: parked — <seat> asked a question at <time>`. Both carry the full id
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
| No `--by` and no `FLEET_ACTOR` or `BEADS_ACTOR` on a verb that names its actor | 2 | `fleet <verb>: no <who> — pass --by <name>, or set FLEET_ACTOR or BEADS_ACTOR. …` | Pass `--by`, or set one of the two. |
| A line written by `fleet event woke`, `rest`, `handed-off`, `exited` or `clear-halt` does not read back | 2 | `fleet event <verb>: the stream's last line reads … rather than the <kind> for <seat> just written; the record is not confirmed` | Read the stream with `fleet event tail` before writing it again. |
| A verb that works on a project, such as `fleet dispatch`, `fleet brief` or `fleet seat nudge`, run outside any fleet | 3 | ``fleet <verb>: no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one`` | Run it inside the project, or run `fleet create` there. |
| The event stream cannot be appended to | 1 | `fleet event <verb>: could not append to <machine-dir>/events.jsonl: <reason>` | Fix the stream file's path or permissions. |
| No projection, or one older than three poll intervals | 5 | `fleet <verb>: … no projection at <machine-dir>/projection.json …` | Start the controller with `fleet start`. |
| `fleet event rest` for a seat with no live session | 4 | `fleet event rest: <seat> has no live session — its row reads <state>` | Nothing to stop; read the seat's row with `fleet status --seat <seat>`. |
| `fleet event rest` on a transient seat | 6 | ``… is a transient row, and only named seats rest — use `fleet seat retire <seat>` instead`` | Retire it. |
| `fleet seat feed` or `fleet seat retire` on a named seat | 6 | `` `<seat>` is a named seat — named seats are rung and rested, and only a transient row is fed and retired`` | Use `fleet seat nudge` or `fleet event rest`. |
| `fleet dispatch` given a bare suffix | 1 | `fleet dispatch: <suffix> is not ready — the store does not list it among the ready` | Type the full id. |

## See also

- [Items and the record](items.md): the verbs that take an item id, and what
  each writes on it.
- [Guards](guards.md): the record guard's bare-id check in full.
- [Runs and workflows](runs.md): how a workflow's exit becomes `fleet run`'s.
- [Status and the event stream](status.md): `fleet status` and the event
  verbs whose refusals are the 5s above.
- [The controller and seats](seats.md): named and transient seats, and the
  rest and nudge that answer 4, 5 and 6.
