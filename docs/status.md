# Status and the event stream

Two things tell you what your fleet is doing. `fleet status` prints the
projection, the document the controller writes on every poll: the seats, what
the controller decided about each, how much context each has used, and the
runs and gates waiting on you. `fleet event tail` and `fleet event show` read
the event stream, the fleet's log of what happened, one line per event. All
three read files and write nothing.

## Terms

- **Machine directory**: where the controller keeps its files, the projection
  and the stream among them. It is the directory `FLEET_DIR` names when that is
  set, and otherwise `.fleet` under your home, where `FLEET_HOME` stands in for
  the home when it is set. On Linux with `XDG_STATE_HOME` set and `FLEET_DIR`
  unset, it is `$XDG_STATE_HOME/fleet`. The paths below are all in it.
- **Projection**: `projection.json`, the document the controller publishes on
  every poll. It reports what the controller observed when it wrote the
  document, and nothing else.
- **Stale**: a projection older than three of the controller's poll intervals,
  using the interval the projection records. With the default five-second
  poll, that is anything older than 15 seconds. A stale projection is one the
  controller has not refreshed inside that window.
- **Event stream**: `events.jsonl`, one JSON object per line, appended to and
  never rewritten. The controller writes to it, and so do the `fleet event`
  writers, the item verbs, runs and routines.
- **Event**: one line of the stream. Every line has six fields:
  - `id`: the event's unique id.
  - `seq`: its sequence number. Sequences rise by one per line and are never
    reused.
  - `ts`: its stamp, in UTC, shaped `YYYY-MM-DDTHH:MM:SSZ`.
  - `type`: what happened, as `<subject>.<what>`, for example
    `controller.started`, `seat.woke` or `run.failed`.
  - `actor`: who the line is from or about. On a seat's lines this is the
    seat's name, on the controller's own lines it is `controller`, and on a
    routine's lines it is the routine's name.
  - `payload`: the event's details, as a JSON object.

## Reading the fleet's status

`fleet status` prints the whole page:

```sh
$ fleet status
projection <stamp> — <age>s old — controller 0.1.0, agent 2.1.261

roster
  builder-1 (orla)  present  decision leave-alone, outcome none  project demo, worktree /work/demo/builder-1
  reviewer  absent  decision leave-alone, outcome none  project demo, worktree /work/demo/reviewer

in flight  nothing
effects  on

context  (rest threshold 400000 tokens)
  builder-1  120000 tokens, 30% of the threshold, 280000 left
  reviewer  —

[[core.flight.rules]]
  type bug → review=none
  labels [docs] → gate=review

routines
  morning  cron  next due 2026-09-24T07:00:00Z, last delivered at 2026-09-23T07:00:02Z, failing streak 0

runs  0 failed in the last 24 hours, 0 parked, 0 could not tell, 0 waiting, 0 open

gates  0 raised by a park and not answered
```

It exits 0.

The page is built from the projection, the policy file that the projection
names, `config.json` in the machine directory, and the event stream. It reads
the policy file as that file is when you run the command. It does not look for running processes, so the
projection's age is the only sign of whether the controller is still running.
It writes nothing, not even to the machine directory.

### The first line

The first line gives the projection's stamp, its age in seconds, the
controller's version and the agent's version. Past the stale window, the age
reads `STALE, <stale-age>s old`. A stamp fleet cannot read as a time reads
`STALE, an age nothing can read it at`. A stale page still exits 0.

When the agent version the controller read differs from the one it expects,
both are printed: `agent 2.1.262 (expected 2.1.261)`. When the controller
could not read the agent's version, the line says `agent not read`.

### A pending grant

When the controller is still waiting for access to a seat's worktree, a
`GRANT PENDING` line follows the first line and says what it is waiting for.
Until that access is granted, the controller carries out no effects, and the
`effects` line gives the same reason:

```sh
$ fleet status
projection <stale-stamp> — STALE, <stale-age>s old — controller 0.1.0, agent 2.1.262 (expected 2.1.261)

GRANT PENDING — the listing of /work/demo/reviewer has not answered within 10s and is still outstanding

roster
  builder-1 (orla)  present  decision leave-alone, outcome none  project demo, worktree /work/demo/builder-1
  reviewer  prompt-blocked, waiting for a permission dialog  decision halt, outcome halted  project demo, worktree /work/demo/reviewer  HALTED, 3 blind dispatch(es)

in flight  builder-1 — revive
effects  off — the listing of /work/demo/reviewer has not answered within 10s and is still outstanding
...
```

### The roster

Each seat gets one row. The row starts with the seat's name, followed in
parentheses by the name you chose for it, if you chose one. Then come its
roster state, the decision the controller reached for it on the last poll,
what the controller did about that decision, and the seat's project and
worktree. A dash stands for a project or worktree the projection does not
carry.

- The roster state is one of `present`, `prompt-blocked`, `starting`,
  `stopped`, `absent` and `unknown`. A `prompt-blocked` seat also shows what
  it is waiting for.
- The decision is one of `leave-alone`, `spawn-woken`, `revive`, `rest`,
  `suggest-rest` and `halt`, or `pending` on a row the controller publishes
  before it reaches a decision.
- The outcome is one of `none`, `spawned`, `rested`, `nudged`, `revived`,
  `halted`, `deferred` and `failed`.
- A seat the controller is holding ends with `HALTED` and its count of blind
  dispatches.

With no seats configured, the section says `no seat is configured`. The
states and decisions are explained in
[The controller and seats](seats.md).

### In flight and effects

`in flight` names the seat and the decision the controller is in the middle of
carrying out, and says `nothing` between effects. The controller writes it
into the projection before it acts on a decision and clears it afterwards. So
on a stale page, a seat named here is the one the controller's last published
poll is inside.

`effects` reads `on`, or `off` followed by the reason. While effects are off,
the controller keeps observing and publishing, but it acts on nothing.

### Context

Each seat's context is measured against the rest threshold, the number of
tokens at which the controller suggests a rest. The row gives the tokens the
seat has used, that figure as a whole-number percentage of the threshold
(rounded down), and how many tokens are left. A seat with no reading shows a
dash rather than a zero.

The threshold is the one the controller uses: `rest_threshold_tokens` under
`[controller]` in the policy file, overridden by `rest_threshold_tokens` in
the `controller` object of `config.json` in the machine directory, and 700000
tokens when neither sets it. A `config.json` that cannot be read is passed
over, and the policy file's threshold stands.

### The rules

The `[[core.flight.rules]]` section lists each rule in the policy file on its
own line, in order: what the rule matches, an arrow, then the values it sets.
A rule matches on `type <type>`, `labels [<label>, ...]`, both joined by
`and`, or `every item` when it names neither. A rule that sets no values says
`sets nothing`. A policy file with no rules gives `no rules are set`.

### Routines

Each loaded routine gets one line: its name, its trigger (`cron`, `cooldown` or
`condition`), when it is next due, its last outcome and when it last fired,
and its failing streak. A dash stands for a value the routine does not have
yet. With no routines loaded, the section says `no routine is loaded`.

### Runs and gates

The `runs` line counts runs by state: failed in the last 24 hours, parked,
could not tell, waiting and open. Every run counted there gets its own row
below the line. The `gates` line counts gates that a park raised and
nobody has answered yet. Both come from the event stream, so a machine with no
stream shows every count at zero. The rows are explained in
[Runs and workflows](runs.md).

### When a section cannot be read

If the policy file or the event stream cannot be read, the page still prints.
The section that needed the file shows the reason in place of its contents,
and the same reason appears on standard error, prefixed `fleet status:`. The
command then exits 3. Without the policy file, the context rows show only the
token count.

## Reading one seat

`--seat <name>` prints two lines for one seat: its roster row and its context
row. The name can be the seat's own name or the name you chose for it, and it
must match exactly, including case:

```sh
$ fleet status --seat orla
builder-1 (orla)  present  decision leave-alone, outcome none  project demo, worktree /work/demo/builder-1
builder-1  120000 tokens, 30% of the threshold, 280000 left
```

It exits 0. It reads the same files as the whole page, so a policy file or
stream it cannot read still makes it exit 3, even though it prints neither the
rules nor the runs.

## Printing the projection document

`--json` prints the projection exactly as stored, byte for byte, and exits 0.
Before printing, fleet parses the document, so a projection that the page
would refuse is refused here too. `--json` and `--seat` cannot be used
together.

## Reading the stream

`fleet event tail` prints the last 50 lines of the stream, exactly as they
are stored, one per line on standard output:

```sh
$ fleet event tail
{"id":"<id-1>","seq":1,"ts":"<event-stamp>","type":"seat.woke","actor":"builder-1","payload":{}}
{"id":"<id-2>","seq":2,"ts":"<event-stamp>","type":"seat.woke","actor":"reviewer","payload":{}}
{"id":"<id-3>","seq":3,"ts":"<event-stamp>","type":"seat.handed_off","actor":"builder-1","payload":{}}
{"id":"<id-4>","seq":4,"ts":"<event-stamp>","type":"seat.exited","actor":"builder-1","payload":{}}
```

It exits 0. It skips any line that does not parse as JSON or has no `seq`.
If the stream exists but fleet cannot read it, the tail prints nothing and
still exits 0.

### Filtering

`--seat <name>` keeps only the lines whose `actor` is that name, and
`--type <type>` keeps only the lines of that type. When you give both, a line
must match both. The filters are applied before the last 50 lines are taken,
so `--seat` on a busy stream still gives you 50 of that seat's lines:

```sh
$ fleet event tail --seat builder-1 --type seat.woke
{"id":"<id-1>","seq":1,"ts":"<event-stamp>","type":"seat.woke","actor":"builder-1","payload":{}}
```

Both filters need an exact match. A filter that matches nothing prints
nothing and exits 0. Types are spelled with underscores: the line
`fleet event handed-off` writes has the type `seat.handed_off`, and
`--type seat.handed-off` matches nothing.

### Starting from a point

`--since <seq>` prints every line with a sequence above `<seq>`, and lifts the
50-line limit:

```sh
$ fleet event tail --since 2
{"id":"<id-3>","seq":3,"ts":"<event-stamp>","type":"seat.handed_off","actor":"builder-1","payload":{}}
{"id":"<id-4>","seq":4,"ts":"<event-stamp>","type":"seat.exited","actor":"builder-1","payload":{}}
```

`--since` also takes a stamp of the form `YYYY-MM-DDTHH:MM:SSZ`. Fleet looks
for the first line, in file order, stamped at or after that time, prints from
that line on, and tells you on standard error which sequence the stamp
resolved to:

```sh
$ fleet event tail --since <event-stamp>
--since <event-stamp> resolved to 1
{"id":"<id-1>","seq":1,"ts":"<event-stamp>","type":"seat.woke","actor":"builder-1","payload":{}}
...
```

A stamp later than every line resolves to `none` and prints no lines. A
`--since` value that is neither a number nor a stamp of that form is refused
with exit 2.

### Following

`--follow` prints the tail and then keeps watching, printing each new line as
it is appended, with the same filters. A line appears only once it is
complete. Interrupting the follow (Ctrl-C) ends it with exit 0. With both
`--follow` and `--since`, the `--since` value chooses only the lines printed
at the start.

### As JSON

`--json` prints each line as a JSON document instead of the stored bytes:

```sh
$ fleet event tail --json --since 3
{"ok":true,"verb":"event tail","data":{"actor":"builder-1","id":"<id-4>","kind":"seat.exited","payload":{},"seq":4,"ts":"<event-stamp>"}}
```

`data` carries the event's six fields. The stored `type` is named `kind`
here, and a field the stored line does not have is `null`. When the tail is
refused under `--json`, it prints a refusal document on standard output, and
still prints its message on standard error:

```sh
$ fleet event tail --json --since yesterday
{"ok":false,"verb":"event tail","refusal":{"code":"usage","why":"--since yesterday is neither a sequence nor a stamp of the shape YYYY-MM-DDTHH:MM:SSZ"}}
```

The exit is the same as without `--json`, and `code` names that exit's row in
[Exit codes and conventions](conventions.md).

## Reading one event

`fleet event show <id>` prints the event with that id, laid out over several
lines with its fields in alphabetical order:

```sh
$ fleet event show <id-3>
{
  "actor": "builder-1",
  "id": "<id-3>",
  "payload": {},
  "seq": 3,
  "ts": "<event-stamp>",
  "type": "seat.handed_off"
}
```

It exits 0. The id must be given whole; fleet does not match a shortened id.
`--json` prints the same document `fleet event tail --json` prints for the
line, with `"verb":"event show"`, and refusals take the same form.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `fleet status` finds no projection | 5 | ``fleet status: there is no projection at <machine>/projection.json to print (No such file or directory (os error 2)) — run `fleet start` `` | Start the controller; see [Getting started](getting-started.md). |
| The projection is not valid JSON | 3 | `fleet status: the projection at <machine>/projection.json does not parse:` and the parser's reason | Start the controller, or let its next poll rewrite the file. |
| The projection is a version this binary does not read | 3 | `fleet status: the projection at <machine>/projection.json is version 2, and this binary reads version 1 — a document half-read is worse than one refused` | Use the `fleet` binary that matches the running controller. |
| The policy file or the stream cannot be read | 3 | The page, with the reason in the affected section, and the reason again on standard error | Fix the file the message names. |
| `--seat` names no seat in the projection | 1 | ``fleet status: the projection carries no row for `<name>` — the collector is what makes a seat one of this fleet's`` | Check the name against the roster, including its case. |
| `--seat` and `--json` given together | 2 | `error: the argument '--seat <NAME>' cannot be used with '--json'` and the usage line | Give one of the two. |
| `fleet event tail` or `fleet event show` finds no stream | 5 | `fleet event tail: no event stream at <machine>/events.jsonl` (or `fleet event show:`) | Nothing has written to this machine's stream yet; start the controller. |
| `--since` is neither a sequence nor a stamp | 2 | `fleet event tail: --since <value> is neither a sequence nor a stamp of the shape YYYY-MM-DDTHH:MM:SSZ` | Give a sequence number or a stamp of that shape. |
| `fleet event show` finds no line with that id | 1 | `fleet event show: no event <id>` | Copy the whole id from `fleet event tail`. |
| Two lines of the stream carry the same id | 2 | `fleet event show: <id> is on more than one line — sequences <seq-a>, <seq-b>` | The stream is damaged; read both lines with `fleet event tail --since`. |

## See also

- [The controller and seats](seats.md): the controller that publishes the
  projection, and the seats on its roster.
- [Runs and workflows](runs.md): the rows of the runs section.
- [Items and the record](items.md): the verbs that write item events to the
  stream, and the questions a seat parks on.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
- [Getting started](getting-started.md): starting the controller that
  publishes the projection and writes the stream.
