# The store contract

The store is where fleet keeps the project's work graph. This page is the
contract a store's adapter implements: how fleet calls it, the JSON each call
carries and answers, the exit codes fleet reads, and what each verb means. You
read it when you connect fleet to a store of your own.

## What a store is

The store is the work graph fleet reads and writes: each item with its title,
description, status, type and labels, who holds it, the order it stands
under, the open items that block it, a run's record and the names of the
keys other tools keep on it; the holds raised on items; and each item's
timeline of entries. A store is an executable, named by `[store] adapter`,
that answers the verbs on this page: the adapter the bd pack carries, or one
of your own.

**Status:** The contract is live.

## Choosing an adapter

The key is `adapter`, in the `[store]` table:

```toml
[store]
adapter = "bd"
```

The key takes two forms. A name with no `/` in it is the store adapter an
installed pack carries under `adapters/store/<name>/`. `bd` is the one the bd
pack carries, and a project whose file has no `adapter` key opens `bd` the
same way. Another pack's adapter is named the same way:

```toml
[store]
adapter = "tracker"
```

fleet reads the installed packs over the defaults, and the highest layer
carrying `adapters/store/<name>/adapter.toml` is the adapter: fleet runs the
`entry` that file names, from the same directory. An adapter whose
`adapter.toml` fails the pack format is not run. How a pack carries an
adapter is on [Packs](packs.md).

fleet runs a pack's adapter with `PATH` set to the search path it builds for
the processes it starts, not the one fleet itself was started with. Where
that path does not hold the runtime the pack runs under (its own `[runtime]`,
or that of a pack it imports), fleet puts the runtime's directory in front:
the one on fleet's own `PATH` that holds it, else the runtime's installer
directory, `$<NAME>_INSTALL/bin` or `~/.<name>/bin`. With no pack installed
that carries the name, fleet refuses before it runs anything, naming the
`fleet pack add` line that installs the one fleet-packs carries.

The other form is the absolute path to an executable:

```toml
[store]
adapter = "/opt/tracker/bin/fleet-store"
```

An adapter named by path runs on the `PATH` fleet itself was started with.
Any other value is refused. The key lives in the project's own file:
`fleet.toml` for an embedded fleet, `.fleet/project.toml` for a standalone
project.

## The call

fleet runs `<adapter> <verb>`, one process per call.

- The request is one JSON object on standard input.
- The answer is one JSON object on standard output. fleet reads the first
  JSON value there and ignores anything after it.
- Standard error is for people. fleet carries its last non-blank line, cut to
  160 characters, into every refusal it prints about the call.
- Each call is bounded at 60 seconds. When a call outruns the bound, fleet
  kills the adapter's process group and reads the call as could not tell. For
  a write, fleet also says that the write's effect cannot be told, so the item
  has to be read before anything is written to it again.

## The envelope

Every request carries two keys beside the verb's own fields:
`"schema_version": 1`, and `"root"`, the absolute path of the project's root.
A `show` request for `a1b2`:

```json
{"id":"a1b2","root":"/work/project","schema_version":1}
```

Every answer carries `"schema_version": 1` beside the verb's response fields.
The answer to that request:

```json
{"schema_version":1,"id":"fx-a1b2"}
```

fleet reads an answer whose `schema_version` is any other value, or missing,
as could not tell. An answer that is not a JSON object, or whose fields do not
read as the verb's response, is could not tell too.

## The exit table

The adapter's exit code says how the call went. Its four codes are the first
four rows of fleet's own [exit table](conventions.md#reading-an-exit-code).

| Exit | Meaning | Standard output |
| --- | --- | --- |
| 0 | answered | the verb's response |
| 1 | refused on the record | `{"schema_version":1,"refused":{"reason":…,"message":…,"candidates":[…]}}` |
| 2 | usage: an unknown verb, a malformed request, a `schema_version` the adapter does not speak, or a capability verb it does not declare | — |
| 3 | could not tell | optionally `{"schema_version":1,"error":…}` |

fleet reads any other code, or death by a signal, as 3.

A refusal names its reason. `missing` is nothing by that id. `ambiguous` is
text that names more than one item, and `candidates` lists them. `already` is
an act that is already done. `moved` is a fenced write whose item no longer
meets its fence: nothing was written, and `message` names who holds the item
or the status it reads. `message` is the sentence for a person:

```json
{"schema_version":1,"refused":{"reason":"ambiguous","message":"a1 matches more than one item","candidates":["fx-a1b2","fx-a1c9"]}}
{"schema_version":1,"refused":{"reason":"moved","message":"fx-a1b2 is held by 0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","candidates":[]}}
```

A refusal without `candidates` reads as one with none. An exit 3 names what
went wrong in `error`:

```json
{"schema_version":1,"error":"the database is locked"}
```

## Types

Each type is shown as its JSON. The verbs below send and answer these.

### Ids

An item id and a hold id are strings, whole, as the store minted them:
`"fx-a1b2"`, `"fx-h9"`. fleet never parses, shortens or sorts one.

### Status

`"open"`, `"in_progress"` and `"closed"` are the statuses fleet acts on by
name. Any other status the store keeps, such as `"deferred"`, is carried as
the store wrote it.

### Stamp

A moment is `YYYY-MM-DDTHH:MM:SSZ`, in UTC, to the second:
`"2026-09-23T10:00:00Z"`. A month, day, hour, minute or second outside its
range is not a stamp.

### Actor

Who acted, as one string, `<kind>:<id>`, where the kind is `seat`, `run`,
`routine` or `controller`. A seat acts as its full seat id:
`"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d"`. The `by` of every write is an
actor.

### Order

The order an item stands under: what it asks, who gave it, the seat it is for
and when.

```json
{"kind":"dispatch","by":"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d","seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","at":"2026-09-23T10:00:00Z"}
```

`kind` is `dispatch` or `review`. `seat` is left out where the order names no
seat:

```json
{"kind":"review","by":"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d","at":"2026-09-23T10:00:00Z"}
```

An order carrying any other key does not read.

### Order state

On an item, the order is exactly one of three objects: no order, an order the
store holds in a form that does not read as one, or the order itself.

```json
{"state":"none"}
{"state":"unreadable"}
{"state":"ordered","order":{"kind":"dispatch","by":"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d","seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","at":"2026-09-23T10:00:00Z"}}
```

### Run record

A run's record, on the item that records the run: the run's hash, the
workflow, its pack and entry file, and when the run started.

```json
{"hash":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","workflow":"build","pack":"ts","entry":"workflows/build.ts","started_at":"2026-09-23T10:00:00Z"}
```

A run record carrying any other key does not read.

### Item

One item, as `show` answers it: here, an item ordered to the seat that holds
it, and blocked by one open item.

```json
{
  "id": "fx-a1b2",
  "title": "Teach the parser the new stamp",
  "description": "The stamp gains a seconds field.",
  "status": "in_progress",
  "type": "task",
  "labels": [
    "fleet"
  ],
  "assignee": "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
  "order": {
    "state": "ordered",
    "order": {
      "kind": "dispatch",
      "by": "seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
      "seat": "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
      "at": "2026-09-23T10:00:00Z"
    }
  },
  "blockers": [
    "fx-c3d4"
  ],
  "run": null,
  "foreign": [
    "sprint"
  ]
}
```

`description` is what the item says; `fleet item show` prints it. `labels`
are the item's own. `assignee` is the seat that holds the item, or `null`.
`order` is an order state. `blockers` are the ids of the open items that
block this one. `run` is a run record, or `null`. `foreign` names the keys
the store keeps on the item beyond its order and its run record: another
tool's. It carries their names only, never what they hold. `id`, `title`,
`status` and `type` are always there; an item that leaves out
`description`, `labels`, `assignee`, `order`, `blockers`, `run` or `foreign`
reads as having no description, no labels, nobody assigned,
`{"state":"none"}`, no blockers, no run record and no foreign keys.

### Item summary

One item as a listing answers it: here, a ready item nobody holds, carrying
one key of another tool's.

```json
{"id":"fx-c3d4","title":"Name the stamp's fields","status":"open","type":"task","labels":["fleet"],"assignee":null,"order":{"state":"none"},"run":null,"foreign":["sprint"]}
```

The fields mean what they mean on an item: `assignee` is the seat that holds
the item, or `null`, `run` is a run record, or `null`, and `foreign` is the
item's, as `show` answers it. `id`, `title`, `status` and `type` are always
there; a summary that leaves out `labels`, `assignee`, `order`, `run` or
`foreign` reads as having no labels, nobody assigned, `{"state":"none"}`, no
run record and no foreign keys. fleet reads each row's holder and run record
off the listing itself and asks `show` for neither. `fleet item list --json`
prints each row's `foreign`.

### Entry

One entry on an item's timeline, as `timeline` answers it: the `id` and `at`
the store gave it, `by`, the actor who appended it, and beside them the fields
of the entry fleet appended, its `kind` among them:

```json
{"id":"e-17","at":"2026-09-23T10:00:00Z","by":"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d","kind":"ordered","order":"dispatch","seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"}
```

`id`, `at` and `by` are strings, and `by` is an actor. An entry that keeps
the `"fleet.entry": 1` that `append` sent reads the same as one that leaves
it out. This is also the shape of each entry `fleet item show --json` prints.

### Filter

Which items a listing asks for: the ready ones, those carrying a label, or
those assigned to a seat.

```json
"ready"
{"label":"fleet"}
{"assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"}
```

### New item

An item to file. The store names it, so there is no id.

```json
{"title":"Name the stamp's fields","description":"Each field has a range.","type":"task","labels":["fleet"],"priority":2}
```

`priority` runs from 0 to 4, and is left out where the item names none.
fleet never sends one outside that range. A routine's item is held to the
types and the range the store declares too (see
[Capabilities](#capabilities)).

### Update

A change to one item's title, assignee or status. A key left out is left
alone, and `null` for the assignee is the item handed to nobody:

```json
{"title":"Name the stamp's fields"}
{"assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"}
{"assignee":null}
```

`status` takes one value, `"open"`: it reopens the item.

`if_assignee` is a fence, and changes nothing itself. With a seat id, the
change lands only while that seat holds the item; with `null`, only while
nobody holds it. An item that does not meet the fence is refused with exit 1,
`moved`, and nothing is written. Left out, the change is not fenced. An
update handing an item nobody holds to a seat, and one reopening an item
nobody holds:

```json
{"assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","if_assignee":null}
{"if_assignee":null,"status":"open"}
```

### Withdrawal fence

`order.withdraw`'s own fields beyond `id` and `by`, each left out where it
is not set. `if_assignee` is a fence as on an update. `if_status` is a status
the item has to read, or the withdrawal is refused as `moved` with nothing
written. `reopen: true` sets the status to `open` in the same act that clears
the assignee and takes the order away; left out, it is `false`. A withdrawal
of an item its seat holds in progress, reopening it:

```json
{"if_assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","if_status":"in_progress","reopen":true}
```

### Capabilities

What a store does beyond the verbs every store answers: an export file fleet
commits, scratch stores for a suite, the prefix its ids carry, the command a
seat types to reach it, and the types and priorities its items take. A store
that exports `.tracker/items.jsonl`, makes scratch stores, mints ids such as
`fx-a1b2`, is reached from a shell as `tracker`, and files items of type
`task` and `bug` at priorities 0 to 4 answers:

```json
{"export":{"file":".tracker/items.jsonl","dir":".tracker/"},"scratch":true,"item_prefix":"fx","cli":"tracker","items":{"types":["task","bug"],"priority":{"min":0,"max":4}}}
```

`export` is `null` for a store with no export. Its `file` and `dir` are
relative to the project root, `/`-separated and without a `..` segment;
`dir` ends in `/`, and `file` is under `dir`.

`cli` is the command a seat types in its shell against the store: one word,
with no `/` and no space. The [guards](guards.md) police that command's
calls. It is `null` for a store no seat reaches from a shell.

`items` names at least one type, none of them empty, and a `priority` range
whose `min` is not above its `max` and whose `max` is not above 4. A
routine files an item only of a type the store names, at a
priority inside its range, and a routine whose item does not fit is failed
without a create being sent. A `types` left out is the six types below, and a
`priority` left out is 0 to 4.

The bd pack's adapter declares the command `bd`, the types `bug`,
`feature`, `task`, `epic`, `chore`, `decision`, `spike`, `story` and
`milestone`, and priorities 0 to 4.

An answer of `{}` reads as a store that declares nothing:

```json
{"export":null,"scratch":false,"item_prefix":null,"cli":null,"items":{"types":["bug","feature","task","epic","chore","decision"],"priority":{"min":0,"max":4}}}
```

A declaration that breaks one of these rules is could not tell.

### Version

Which store answered, and at which version of itself:

```json
{"name":"tracker","version":"0.4.0"}
```

## Verbs

Each verb's request fields are the ones beyond the envelope, and its response
fields the ones beyond `schema_version`.

| Verb | Request | Response | Exit 1 |
| --- | --- | --- | --- |
| `version` | `{}` | `{name, version}` | — |
| `capabilities` | `{}` | `{export, scratch, item_prefix, cli, items}` | — |
| `resolve` | `{id: text}` | `{id}` | `missing`, `ambiguous` with `candidates` |
| `show` | `{id: text}` | `{item}` | `missing`, `ambiguous` |
| `list` | `{filter}` | `{items: [item summary]}` | — |
| `timeline` | `{id}` | `{entries: [entry]}`, in append order | `missing` |
| `create` | `{item: new item, by}` | `{id}` | — |
| `update` | `{id, by, title?, assignee?, status?, if_assignee?}` | `{}` | `missing`, `moved` |
| `append` | `{id, by, entry}` | `{entry: entry id}` | `missing` |
| `order.set` | `{id, by, order}` | `{}` | `missing` |
| `order.withdraw` | `{id, by, if_assignee?, if_status?, reopen?}` | `{}` | `missing`, `moved` |
| `run.set` | `{id, by, run}` | `{}` | `missing` |
| `hold.raise` | `{id, by, reason}` | `{hold}` | `missing` |
| `hold.clear` | `{hold, by}` | `{}` | `missing`, `already` |
| `holds.open` | `{}` | `{holds: [hold id]}` | — |
| `close` | `{id, by, reason}` | `{}` | `missing`, `already` |
| `export`, where declared | `{into: absolute directory}` | `{file: absolute path written}` | — |
| `scratch`, where declared | `{into: absolute empty directory fleet made}` | `{root: absolute root of the new store}` | — |

- `by` is an actor, `order` an order and `run` a run record.
- `show` resolves its `id` exactly as `resolve` does.
- `update` naming none of `title`, `assignee` and `status`, or a `status`
  other than `"open"`, is a malformed request: the adapter answers usage,
  exit 2, and writes nothing. fleet never sends either. It refuses the first
  as could not tell, exit 3, and the second as usage, exit 2, before the
  adapter is run.
- `append`'s `entry` is the entry fleet appends: one JSON object carrying
  `"fleet.entry": 1` and a `kind`. Each entry `timeline` answers is an entry,
  with the `by` the `append` carried. An entry that does not read, a `by`
  that is not an actor's string among them, makes the whole read could not
  tell, naming its row. The kinds are in
  [Items and the record](items.md#terms).
- `export` and `scratch` are answered only by a store whose capabilities
  declare them. A store that does not declare one answers it with exit 2.
- `scratch`'s `root` is the root fleet sends in every request to the new
  store.

Answers, one per shape beyond those above:

```json
{"id":"fx-a1b2"}
{"entry":"e-17"}
{"hold":"fx-h9"}
{"holds":["fx-h9"]}
{"file":"/work/lane/store/export.jsonl"}
{"root":"/tmp/fleet-scratch/store"}
```

## Semantics

### Listing

- `"ready"` answers the items that are open, not blocked by an open item the
  store counts as having to finish first, and not held. The listing is
  unordered.
- `{"label": …}` answers the items that are open and carry the label.
- `{"assignee": …}` answers every item assigned to the seat, whatever its
  status.

### Resolving an id

`resolve` and `show` read the text they are given by three rules, in order:

1. the whole id;
2. else the one id whose part after its prefix is the text: `a1b2` for
   `fx-a1b2`;
3. else the ids whose part after the prefix starts with the text.

More than one match at the first rule that matches anything is `ambiguous`,
and no match at all is `missing`. An adapter whose ids carry no prefix treats
the whole id as that part.

### Orders and runs

- `order.set` replaces the item's order whole, and touches neither the
  assignee nor the run record. `run.set` likewise never touches the order.
- `order.withdraw` clears the assignee and the order in one act. After any
  exit, an item never has its assignee cleared while its order stands.
- With `reopen: true`, the same act sets the status to `open`, so an item its
  seat left in progress is open again, with nobody holding it. The reopen is
  never a second write.

### Fences

A write carrying `if_assignee`, or a withdrawal carrying `if_status`, is
fenced: the store takes it only while the item is held by that seat, or by
nobody for `null`, and reads that status. An item whose `assignee` is `null`
or left out meets a fence of `null`. A write whose fence the item does not
meet is exit 1, `moved`, and nothing of it is written.

### The timeline

The timeline is append-only. Entries come back in append order with distinct
ids, and the store sets each entry's id and time.

### Holds

- `hold.raise` takes the item out of ready until the hold is cleared.
- `hold.clear` of a hold already cleared is exit 1, `already`.
- `holds.open` answers hold ids only.

### Closing

`close` of an item already closed is exit 1, `already`. A landing's `close`
carries `by` as the seat that holds the item, `seat:<its assignee>`.

### Export and scratch

- `export` regenerates the one export file under `into`, and makes the
  directories it needs.
- `scratch` makes a new, empty store inside `into`, and never touches a store
  outside it.

## The schema

`fleet store schema` prints this contract as one JSON Schema document, draft
2020-12, on standard output: the contract of the fleet binary you run it
with. You read it to validate what your adapter receives and answers, or to
generate your adapter's types from it. It reads no project and no store, so
it runs anywhere.

```sh
$ fleet store schema > contract.json
$ jq '.verbs | keys' contract.json
[
  "append",
  "capabilities",
  "close",
...
  "update",
  "version"
]
```

It exits 0.

The document's top-level keys:

- `schema_version` is `1`, the contract's version.
- `verbs` holds one entry per row of the [verbs table](#verbs), each with a
  `request` and a `response` schema. A request is the envelope's
  `schema_version` and `root` with the verb's own fields; a response is
  `schema_version` with the verb's response fields.
- `refusal` is the answer of exit 1, `{"schema_version":1,"refused":…}`, and
  `error` the answer of exit 3, `{"schema_version":1,"error":…}`.
- `$defs` holds the types. Every `$ref` in the document points into it, so a
  tool loads the whole document and reaches one verb's schema by its pointer:
  `#/verbs/show/request`.

A type that does not read with a key it does not name, such as an order, a
run record or an entry, says `"additionalProperties": false`. Every other
object, a request among them, accepts keys it does not name. An update's
`status` is `"open"` and nothing else.

## Checking an adapter

`fleet store check [--adapter <path|name>]` runs every check of this
contract against an adapter and prints what each one answered.

Without `--adapter` it checks the adapter the project you are in selects:
the one `[store] adapter` names in the project's own file, else `bd`, by
name. Outside a project it checks `bd`, through this machine's installed
packs. `--adapter` takes what `[store] adapter` takes, an absolute path to
an adapter executable or the name of one an installed pack carries, and
checks that one instead.

The checks write, so they never run on your project's store. fleet makes a
temporary directory, asks the adapter's `scratch` verb for a new store
inside it, runs every check on that store, and removes the directory when it
finishes, whatever the checks answered. An adapter whose capabilities do not
declare `scratch` is asked for nothing beyond its capabilities, and no check
runs.

In a project whose store is the bd pack's:

```sh
$ fleet store check
PASS  empty listings
PASS  version
PASS  capabilities
...
PASS  fenced withdraw with reopen
SKIP  another writer's keys: no other writer was handed to this run, so nothing plants another tool's keys
SKIP  another writer's keys are listed as foreign: no other writer was handed to this run, so nothing plants another tool's keys
store check: bd — 25 passed, 0 failed, 2 skipped
```

It exits 0.

Each check prints one line on standard output, in the same order every
run: `PASS` and the check's name, `SKIP` with why the check does not apply,
or `FAIL` with what the store answered instead. A check's line is printed
as soon as the check answers, before the next check runs, so a slow adapter
shows its progress line by line. Every check runs whatever the one before
it answered, so one run names every disagreement. The last line names the
adapter, by the name its `version` answers, else by its path, and counts
the checks that passed, failed and were skipped.

Three checks can be skipped. `export` is skipped for a store whose
capabilities declare no export. `another writer's keys` and `another
writer's keys are listed as foreign` are always skipped here, because `fleet
store check` has no way to put another tool's keys on an item.

### When it refuses

| Situation | Exit | What you see |
| --- | --- | --- |
| a check failed | 1 | its `FAIL` line, and the summary's count of failures |
| the adapter declares no `scratch` | 1 | `fleet store check: <adapter> declares no scratch capability, and the check runs only on a store it makes for the purpose — nothing was run` |
| `--adapter` names a relative path, or nothing | 2 | ``fleet store check: --adapter takes an absolute path to an executable or the name of a store adapter an installed pack carries, and `<value>` is neither`` |
| nothing executable is at the path `--adapter` names | 3 | ``fleet store check: --adapter names `<path>`, which is not an executable file`` |
| nothing executable is at the path `[store] adapter` names | 3 | ``fleet store check: [store] adapter names `<path>`, which is not an executable file`` |
| no installed pack carries the name | 3 | ``fleet store check: no store adapter named `<name>` in the installed packs — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/store/<name> --version v0.1.0` installs the one fleet-packs carries`` |
| the installed packs do not resolve | 3 | ``fleet store check: no store adapter named `<name>` resolves:`` and the reason |
| the pack's adapter fails the format | 3 | ``fleet store check: the store adapter `<name>` cannot be opened:`` and the defect |
| the adapter cannot be run, or does not answer `capabilities` or `scratch` | 3 | `fleet store check:` and the reason |

Only a failed check prints check lines; every other refusal comes before
any check runs, and prints none.

## See also

- [Exit codes and conventions](conventions.md): the exit table every fleet
  command shares, and naming an item by its full id.
- [Items and the record](items.md): the verbs that write an item's timeline,
  and the entries each one appends.
- [Runs and workflows](runs.md): the run whose record `run.set` writes.
