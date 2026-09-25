# The store contract

The store is where fleet keeps the project's work graph. This page is the
contract a store's adapter implements: how fleet calls it, the JSON each call
carries and answers, the exit codes fleet reads, and what each verb means. You
read it when you connect fleet to a store of your own.

## What a store is

The store is the work graph fleet reads and writes: each item with its title,
status, type and labels, who holds it, the order it stands under, the open
items that block it and a run's record; the holds raised on items; and each
item's timeline of entries. bd is built in. Any other store is an executable,
named by `[store] adapter`, that answers the verbs on this page.

**Status:** The contract is live.

## Choosing an adapter

The key is `adapter`, in the `[store]` table:

```toml
[store]
adapter = "bd"
```

`"bd"` is the default, and a project whose file has no `adapter` key uses bd.
The other form is the absolute path to an executable:

```toml
[store]
adapter = "/opt/tracker/bin/fleet-store"
```

The key lives in the project's own file: `fleet.toml` for an embedded fleet,
`.fleet/project.toml` for a standalone project.

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
an act that is already done. `message` is the sentence for a person:

```json
{"schema_version":1,"refused":{"reason":"ambiguous","message":"a1 matches more than one item","candidates":["fx-a1b2","fx-a1c9"]}}
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
  "run": null
}
```

`labels` are the item's own. `assignee` is the seat that holds the item, or
`null`. `order` is an order state. `blockers` are the ids of the open items
that block this one. `run` is a run record, or `null`. `id`, `title`,
`status` and `type` are always there; an item that leaves out `labels`,
`assignee`, `order`, `blockers` or `run` reads as having no labels, nobody
assigned, `{"state":"none"}`, no blockers and no run record.

### Item summary

One item as a listing answers it:

```json
{"id":"fx-c3d4","title":"Name the stamp's fields","status":"open","type":"task","labels":["fleet"],"order":{"state":"none"}}
```

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

### Update

A change to one item's title or assignee. A key left out is left alone, and
`null` for the assignee is the item handed to nobody:

```json
{"title":"Name the stamp's fields"}
{"assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"}
{"assignee":null}
```

### Capabilities

What a store does beyond the verbs every store answers: an export file fleet
commits, scratch stores for a suite, and the prefix its ids carry. A store
that exports `.beads/issues.jsonl`, makes scratch stores and mints ids such as
`fx-a1b2` answers:

```json
{"export":{"file":".beads/issues.jsonl","dir":".beads/"},"scratch":true,"item_prefix":"fx"}
```

`export` is `null` for a store with no export. Its `file` and `dir` are
relative to the project root, `/`-separated and without a `..` segment;
`dir` ends in `/`, and `file` is under `dir`. An answer of `{}` reads as a
store that declares nothing:

```json
{"export":null,"scratch":false,"item_prefix":null}
```

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
| `capabilities` | `{}` | `{export, scratch, item_prefix}` | — |
| `resolve` | `{id: text}` | `{id}` | `missing`, `ambiguous` with `candidates` |
| `show` | `{id: text}` | `{item}` | `missing`, `ambiguous` |
| `list` | `{filter}` | `{items: [item summary]}` | — |
| `timeline` | `{id}` | `{entries: [entry]}`, in append order | `missing` |
| `create` | `{item: new item, by}` | `{id}` | — |
| `update` | `{id, by, title?, assignee?}` | `{}` | `missing` |
| `append` | `{id, by, entry}` | `{entry: entry id}` | `missing` |
| `order.set` | `{id, by, order}` | `{}` | `missing` |
| `order.withdraw` | `{id, by}` | `{}` | `missing` |
| `run.set` | `{id, by, run}` | `{}` | `missing` |
| `hold.raise` | `{id, by, reason}` | `{hold}` | `missing` |
| `hold.clear` | `{hold, by}` | `{}` | `missing`, `already` |
| `holds.open` | `{}` | `{holds: [hold id]}` | — |
| `close` | `{id, by, reason}` | `{}` | `missing`, `already` |
| `export`, where declared | `{into: absolute directory}` | `{file: absolute path written}` | — |
| `scratch`, where declared | `{into: absolute empty directory fleet made}` | `{root: absolute root of the new store}` | — |

- `by` is an actor, `order` an order and `run` a run record.
- `show` resolves its `id` exactly as `resolve` does.
- `update` with neither `title` nor `assignee` is usage, exit 2.
- `append`'s `entry` is the entry fleet appends: one JSON object carrying
  `"fleet.entry": 1` and a `kind`. Each entry `timeline` answers carries the
  id and time the store gave it, the actor who appended it, and the entry fleet
  appended. The kinds are in [Items and the record](items.md#terms).
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
3. else the ids whose part after the prefix contains the text.

More than one match at the first rule that matches anything is `ambiguous`,
and no match at all is `missing`. An adapter whose ids carry no prefix treats
the whole id as that part.

### Orders and runs

- `order.set` replaces the item's order whole, and touches neither the
  assignee nor the run record. `run.set` likewise never touches the order.
- `order.withdraw` clears the assignee and the order in one act. After any
  exit, an item never has its assignee cleared while its order stands.

### The timeline

The timeline is append-only. Entries come back in append order with distinct
ids, and the store sets each entry's id and time.

### Holds

- `hold.raise` takes the item out of ready until the hold is cleared.
- `hold.clear` of a hold already cleared is exit 1, `already`.
- `holds.open` answers hold ids only.

### Closing

`close` of an item already closed is exit 1, `already`.

### Export and scratch

- `export` regenerates the one export file under `into`, and makes the
  directories it needs.
- `scratch` makes a new, empty store inside `into`, and never touches a store
  outside it.

## Checking an adapter

`fleet store check [--adapter <path>]` runs every check of this contract
against an adapter and prints what each one answered.

Without `--adapter` it checks the adapter the project you are in selects:
the one `[store] adapter` names in the project's own file, else bd. Outside
a project it checks bd. `--adapter` takes the absolute path to an adapter
executable and checks that one instead.

The checks write, so they never run on your project's store. fleet makes a
temporary directory, asks the adapter's `scratch` verb for a new store
inside it, runs every check on that store, and removes the directory when it
finishes, whatever the checks answered. An adapter whose capabilities do not
declare `scratch` is asked for nothing beyond its capabilities, and no check
runs.

In a project whose file names no adapter:

```sh
$ fleet store check
PASS  empty listings
PASS  version
PASS  capabilities
...
PASS  fenced writes
SKIP  another writer's keys: no other writer was handed to this run, so nothing plants another tool's keys
store check: bd — 22 passed, 0 failed, 1 skipped
```

It exits 0.

Each check prints one line on standard output, in the same order every
run: `PASS` and the check's name, `SKIP` with why the check does not apply,
or `FAIL` with what the store answered instead. Every check runs whatever
the one before it answered, so one run names every disagreement. The last
line names the adapter, by the name its `version` answers, else by its path,
and counts the checks that passed, failed and were skipped.

Two checks can be skipped. `export` is skipped for a store whose
capabilities declare no export. `another writer's keys` is always skipped
here, because `fleet store check` has no way to put another tool's keys on
an item.

### When it refuses

| Situation | Exit | What you see |
| --- | --- | --- |
| a check failed | 1 | its `FAIL` line, and the summary's count of failures |
| the adapter declares no `scratch` | 1 | `fleet store check: <adapter> declares no scratch capability, and the check runs only on a store it makes for the purpose — nothing was run` |
| `--adapter` names a relative path | 2 | `fleet store check: --adapter takes an absolute path to an executable, and <path> is not one` |
| nothing executable is at the path | 3 | ``fleet store check: [store] adapter names `<path>`, which is not an executable file`` |
| the adapter cannot be run, or does not answer `capabilities` or `scratch` | 3 | `fleet store check:` and the reason |

Only a failed check prints check lines; every other refusal comes before
any check runs, and prints none.

## See also

- [Exit codes and conventions](conventions.md): the exit table every fleet
  command shares, and naming an item by its full id.
- [Items and the record](items.md): the verbs that write an item's timeline,
  and the entries each one appends.
- [Runs and workflows](runs.md): the run whose record `run.set` writes.
