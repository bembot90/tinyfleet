# The agent contract

The agent is the program a seat runs. This page is the contract an agent's
adapter implements: the verbs it is asked, the JSON each call carries and
answers, the exit codes its answer is read by, and what each verb means. You
read it when you connect fleet to an agent of your own.

## What an agent is

A seat is fleet's worker, and the agent is the program its session runs. The
adapter is the one thing that knows the agent: what it is and which version
of it is installed, the command a session launches or resumes with, what
each seat's agent is doing, and how much of its context a session has used.
It answers those six questions, and it never starts, types into or ends a
session itself: it answers the command a session starts with.

**Status:** fleet speaks this contract to the adapter executable
`[agent] adapter` names. Where the key names none and no installed pack
carries `claude-code`, the agent its seats run is Claude Code, through the
adapter fleet has built in. The JSON on this page is fleet's own, and every
example here reads and writes back through it.

## Choosing an adapter

The key is `adapter`, in the `[agent]` table of the fleet's own
`fleet.toml`. It holds for every seat the fleet runs:

```toml
[agent]
adapter = "quill"
```

The key takes two forms. A name with no `/` in it is the agent adapter an
installed pack carries under `adapters/agent/<name>/`. fleet reads the
installed packs over the defaults, and the highest layer carrying
`adapters/agent/<name>/adapter.toml` is the adapter: fleet runs the `entry`
that file names, the file of that name beside it. An adapter whose
`adapter.toml` fails the pack format is not run. How a pack carries an
adapter is on [Packs](packs.md).

A fleet whose file has no `adapter` key opens `claude-code` the same way.
Where no installed pack carries `claude-code`, that name is the Claude Code
adapter fleet has built in, and so it is on a machine where `fleet start`
has not yet written the defaults. Any other name no installed pack carries
is refused before anything runs, naming the `fleet pack add` line that
installs the one fleet-packs carries. Where the installed packs do not
resolve, every name is refused, `claude-code` among them, naming why.

fleet runs a pack's adapter with `PATH` set to the search path it builds for
the processes it starts, not the one fleet itself was started with. Where
that path does not hold the runtime the pack runs under, fleet puts the
runtime's directory in front, as it does for a
[store adapter](store.md#choosing-an-adapter).

The other form is the absolute path to an executable:

```toml
[agent]
adapter = "/opt/quill/bin/fleet-agent"
```

An adapter named by path runs on the `PATH` fleet itself was started with.
Any other value is refused.

When fleet opens an adapter, it asks it `capabilities` and `version`. Where
either does not answer, the capabilities break the contract, or `version`
answers `null`, fleet starts no session through that adapter: `fleet start`
refuses, names the adapter and why, exits 3, and loads nothing.
`fleet doctor agent-adapter` opens the adapter the same way and prints what
it answered, without starting anything (see
[Running a doctor check](getting-started.md#running-a-doctor-check)).

## The call

An adapter is an executable called as `<adapter> <verb>`, one process per
call, the same call a [store adapter](store.md#the-call) answers.

- The request is one JSON object on standard input.
- The answer is one JSON object on standard output. The first JSON value
  there is the answer, and anything after it is ignored.
- Standard error is for people. Its last non-blank line, cut to 160
  characters, is carried into every refusal printed about the call.
- Each call is bounded at 20 seconds. `FLEET_AGENT_TIMEOUT_MS`, set to a
  positive whole number of milliseconds in fleet's environment, is the bound
  instead; any other value is the 20 seconds. A call that outruns the bound
  has the adapter's process group killed, and reads as could not tell; what
  fleet prints about it names the bound and `FLEET_AGENT_TIMEOUT_MS`.
- `read` and `context` are asked about every seat at once, so a poll runs
  one process per verb and never one per seat.

## The envelope

Every request carries two keys beside the verb's own fields:
`"schema_version": 1`, and `"root"`, as a
[store request](store.md#the-envelope) does. An agent adapter is the
fleet's and not a project's, so its `root` is the directory the fleet's own
`fleet.toml` is in: for an embedded fleet, the project's root. A `resume`
request:

```json
{"config_dir":"/work/seats/builder-e5f60718/agent","model":"quill-large-2","posture":"auto","root":"/work/project","schema_version":1,"session_id":"5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d","worktree":"/work/lanes/builder-e5f60718"}
```

Every answer carries `"schema_version": 1` beside the verb's response
fields. The answer to that request:

```json
{"schema_version":1,"argv":["quill","--model","quill-large-2","--mode","auto","--resume","5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d"],"env":{"QUILL_HOME":"/work/seats/builder-e5f60718/agent"}}
```

The `schema_version` is the agent contract's own: a store adapter and an
agent adapter each answer at their own contract's version. An answer whose
`schema_version` is any other value, or missing, does not read. An answer
that is not a JSON object, or whose fields do not read as the verb's
response, does not read either, and what is printed about it names the
field that did not read, such as `seats[0].activity`.

The contract grows by adding. A request can carry keys this page does not
name, and an adapter reads past them; an answer can carry keys the request's
verb does not name, and they are read past too.

## The exit table

The adapter's exit code says how the call went. The four codes are the store
contract's, and the first four rows of fleet's own
[exit table](conventions.md#reading-an-exit-code).

| Exit | Meaning | Standard output |
| --- | --- | --- |
| 0 | answered | the verb's response |
| 1 | refused | `{"schema_version":1,"refused":{"reason":…,"message":…}}` |
| 2 | usage: an unknown verb, a malformed request, a `schema_version` the adapter does not speak, or `context` asked of an adapter whose capabilities do not declare it | — |
| 3 | could not tell | optionally `{"schema_version":1,"error":…}` |

Any other code, or death by a signal, reads as 3.

A refusal names its reason. `unsupported` is a posture or a model this agent
will not take. `missing` is a resume of a session the agent does not have.
`message` is the sentence for a person:

```json
{"schema_version":1,"refused":{"reason":"unsupported","message":"quill takes no posture unattended"}}
{"schema_version":1,"refused":{"reason":"missing","message":"quill has no session 5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d"}}
```

An exit 3 names what went wrong in `error`:

```json
{"schema_version":1,"error":"quill's session index is locked"}
```

## Types

Each type is shown as its JSON. The examples on this page are one adapter's,
for an agent named `quill`: its models, flags and variables are its own, and
fleet carries each of them without reading it.

### Seat

A seat is named by its full id, as fleet minted it:
`"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"`. What a seat id is, and a seat's
name and machine name, are on [The controller and seats](seats.md#terms).

### Posture

How much a seat's agent does without asking, in fleet's own words:

```json
"ask"
"auto"
"unattended"
```

`ask` asks before it acts, `auto` acts on what its agent judges safe, and
`unattended` never stops to ask. The adapter maps each onto its agent's own
modes. A posture of any other word does not read.

### Capabilities

What an adapter declares about its agent: the postures it takes, the model a
seat that names none launches on, the template of a session's first turn,
whether it answers `context`, the versions of its agent it was measured
against, and the models a posture is held to. An adapter whose agent takes
all three postures, answers `context`, was measured against version `2.4.0`,
and takes `auto` only on its large models answers:

```json
{"schema_version":1,"postures":["ask","auto","unattended"],"default_model":"quill-large-2","first_turn":"/wake {seat}","context":true,"measured":["2.4.0"],"posture_models":{"auto":["quill-large"]}}
```

- `postures` names at least one posture.
- `default_model` is the agent's own model string.
- `first_turn` is not blank. `{seat}` in it stands for the seat's session
  name, which is filled in before the launch is sent.
- `context` left out is `false`: the adapter does not answer `context`.
- `measured` names at least one version, as `version` names them.
- `posture_models` holds a posture to the models whose names start with one
  of its prefixes. A posture it does not key is held to no model, and a
  `posture_models` left out, or `{}`, holds none.

A declaration with no postures, no measured version or a blank first turn
breaks the contract. A declaration naming a posture of any other word does
not read.

### Version

Which agent the adapter drives, and at which version of itself:

```json
{"schema_version":1,"name":"quill","version":"2.4.0"}
```

`version` is `null` where no binary of the agent is installed:

```json
{"schema_version":1,"name":"quill","version":null}
```

### Permissions

What a seat runs without asking, in fleet's words, which the adapter renders
into its agent's own permission format. It rides on `launch`:

- `commands` are the project's `[permissions] tool_commands`, each one
  command word: a bare name or a path relative to the repository, with no
  space, no `*`, `?`, `[` or `]`, and no leading `-`.
- `touched` is the builder's checks, the one command a dispatch was handed
  with `--touched` (see [Items and the record](items.md#--touched)). It is
  left out where none was handed.

### Argv

What `launch` and `resume` answer: `argv`, the command the session runs as
its own process, first word first, and `env`, the variables it runs with.

### Seat asked about

One seat, as `read` and `context` ask about it: `seat`, the seat's id;
`worktree`, the absolute path its session runs in; and, where fleet holds
them, `session_id`, the session's id as a `read` last answered it; `pid`,
the process id of the pane the seat's agent runs as; `config_dir`, the
configuration directory the session is scoped to; and `screen`, the pane's
text. Each of the last four is left out where fleet has none.

### Activity reading

One seat, as `read` answers it:

- `activity` is `starting`, `busy`, `idle`, `blocked` or `unknown`.
- `blocked_on` is `permission`, `question`, `logged_out` or `usage_limit`:
  what a blocked seat waits on. It is left out where the adapter cannot
  name it; `blocked` without it is still a blocked seat.
- `evidence` is `typed`, a reading of the agent's own state, or `screen`, a
  reading of the pane's text by the adapter's rules.
- `session_id` is the session's id, where the adapter found it.
- `cause` is one sentence for a person, such as what left a reading
  `unknown`.

A blocked seat whose wait the adapter cannot name, and a seat the adapter
cannot read:

```json
{"seat":"0199a3c4-8f01-7a23-b456-c789d0e1f234","activity":"blocked","evidence":"screen","cause":"a dialog no rule names"}
{"seat":"0199a3c4-8f01-7a23-b456-c789d0e1f234","activity":"unknown","evidence":"screen","cause":"the screen matches no rule"}
```

An activity, a `blocked_on` or an `evidence` of any other word does not
read.

### Seat context

One seat, as `context` answers it: `tokens`, how much of its context the
session has used; `window`, how much it has; `turns`, how many turns it has
taken; and `last_write`, when it last wrote, a
[stamp](store.md#stamp). Each is left out where the adapter cannot say.

## Verbs

Each verb's request fields are the ones beyond the envelope, and its response
fields the ones beyond `schema_version`. A `?` marks a field that is left out
where it has no value.

| Verb | Request | Response | Exit 1 |
| --- | --- | --- | --- |
| `capabilities` | `{}` | `{postures, default_model, first_turn, context, measured, posture_models}` | — |
| `version` | `{}` | `{name, version}` | — |
| `launch` | `{seat, worktree, name, model, posture, first_turn, config_dir?, env, permissions: {commands, touched?}}` | `{argv, env}` | `unsupported` |
| `resume` | `{session_id, worktree, config_dir?, model, posture}` | `{argv, env}` | `unsupported`, `missing` |
| `read` | `{seats: [seat asked about]}` | `{seats: [activity reading]}` | — |
| `context`, where declared | `{seats: [seat asked about]}` | `{seats: [seat context]}` | — |

- `launch`'s `name` is the name the session answers to, `first_turn` the
  declared template with `{seat}` filled in, and `env` the variables fleet
  sets for the seat. `config_dir` is left out for a named seat, which runs
  under its agent's own configuration.
- `resume`'s `session_id` is the session's full id.
- `read` and `context` answer one row per seat they were asked about.
- `context` is answered only by an adapter whose capabilities declare it.

A `launch` for the seat `builder-e5f60718`:

```json
{
  "config_dir": "/work/seats/builder-e5f60718/agent",
  "env": {
    "FLEET_ACTOR": "seat:0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"
  },
  "first_turn": "/wake builder-e5f60718",
  "model": "quill-large-2",
  "name": "builder-e5f60718",
  "permissions": {
    "commands": [
      "cargo",
      "make"
    ],
    "touched": "cargo nextest run -p fleet-core"
  },
  "posture": "auto",
  "root": "/work/project",
  "schema_version": 1,
  "seat": "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
  "worktree": "/work/lanes/builder-e5f60718"
}
```

and its answer:

```json
{"schema_version":1,"argv":["quill","--model","quill-large-2","--mode","auto","/wake builder-e5f60718"],"env":{"FLEET_ACTOR":"seat:0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","QUILL_HOME":"/work/seats/builder-e5f60718/agent"}}
```

A `read` of two seats: one whose session fleet knows, and one it knows only
by its pane:

```json
{
  "root": "/work/project",
  "schema_version": 1,
  "seats": [
    {
      "config_dir": "/work/seats/builder-e5f60718/agent",
      "pid": 48213,
      "seat": "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
      "session_id": "5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d",
      "worktree": "/work/lanes/builder-e5f60718"
    },
    {
      "pid": 48307,
      "screen": "Run make lint? (y/n)",
      "seat": "0199a3c4-8f01-7a23-b456-c789d0e1f234",
      "worktree": "/work/project"
    }
  ]
}
```

and its answer:

```json
{"schema_version":1,"seats":[{"seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","activity":"busy","evidence":"typed","session_id":"5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d"},{"seat":"0199a3c4-8f01-7a23-b456-c789d0e1f234","activity":"blocked","blocked_on":"permission","evidence":"screen"}]}
```

A `context` answer for the same two seats, the second of which the adapter
can say nothing about:

```json
{"schema_version":1,"seats":[{"seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","tokens":48210,"window":200000,"turns":12,"last_write":"2026-09-23T10:00:00Z"},{"seat":"0199a3c4-8f01-7a23-b456-c789d0e1f234"}]}
```

## Semantics

### Presence and activity

Whether a seat's session exists and its process is alive is fleet's to read,
and never the adapter's: an activity reading has no field for either. The
adapter answers only what the agent in a live session is doing.

### Liveness

No activity reading ends a seat's work. `idle` says the agent waits at its
prompt, never that its work is done: whether an item's work is done is read
off the item's record, and a run waits on the record with `run.until` (see
[Runs and workflows](runs.md)), never on an adapter's reading.

### Finding a seat's session

The adapter finds a seat's session among its agent's by `session_id`. Where
the request carries none, or none of its agent's sessions carries that id, it
finds it by `pid`, and answers the `session_id` of the session it found
there: an agent can give the same process a new session id, and fleet learns
the new one from the answer. It never matches a seat by its `worktree`.

### Launching

A launch writes inside the seat's `config_dir` and inside its `worktree`,
and nowhere else: the agent's scoped configuration, among it the acceptance
of trust for that worktree, and whatever file its agent reads the rendered
permissions from. The adapter's own plugin or extension directory is the
adapter's to know, and is never in the request.

### Resuming

A resume's `argv` carries the flags the launch's did, its model and posture
among them, with the session's full id.

## The schema

`fleet agent schema` prints this contract as one JSON Schema document, draft
2020-12, on standard output: the contract of the fleet binary you run it
with. You read it to validate what your adapter receives and answers, or to
generate your adapter's types from it. It reads no project and runs no
adapter, so it runs anywhere.

```sh
$ fleet agent schema > contract.json
$ jq '.verbs | keys' contract.json
[
  "capabilities",
  "context",
  "launch",
  "read",
  "resume",
  "version"
]
```

It exits 0.

The document's top-level keys:

- `schema_version` is `1`, the agent contract's version.
- `verbs` holds one entry per row of the [verbs table](#verbs), each with a
  `request` and a `response` schema. A request is the envelope's
  `schema_version` and `root` with the verb's own fields; a response is
  `schema_version` with the verb's response fields.
- `refusal` is the answer of exit 1, `{"schema_version":1,"refused":…}`, and
  `error` the answer of exit 3, `{"schema_version":1,"error":…}`.
- `$defs` holds the types. Every `$ref` in the document points into it, so a
  tool loads the whole document and reaches one verb's schema by its pointer:
  `#/verbs/launch/request`.

A posture, an activity, a `blocked_on`, an `evidence` and a refusal's
`reason` are each a closed list of words, an `enum`, so the types you
generate name them:

```sh
$ jq '."$defs".Posture.enum' contract.json
[
  "ask",
  "auto",
  "unattended"
]
```

Every object accepts keys it does not name, a request among them, except
`posture_models`, whose keys are the three postures and no other.

## Checking an adapter

`fleet agent check [--adapter <path|name>] [--fixtures <dir>] [--live]
[--model <model>]` runs the checks of this contract against an adapter and
prints what each one answered.

Without `--adapter` it checks the adapter the fleet you are in selects: the
one `[agent] adapter` names in the fleet's own `fleet.toml`, else
`claude-code`, opened as [Choosing an adapter](#choosing-an-adapter) says.
Outside a fleet it checks `claude-code`, through this machine's installed
packs. `--adapter` takes what `[agent] adapter` takes, an absolute path to an
adapter executable or the name of one an installed pack carries, and checks
that one instead.

To ask only whether the adapter your fleet names answers at all, run
`fleet doctor agent-adapter`, which asks it `version` and `capabilities` and
starts nothing.

Everything the checks make is under a temporary directory fleet makes and
removes when it finishes, whatever the checks answered: every request's
`root`, and the `config_dir` and `worktree` each `launch` is asked for.

The checks, in the order they print:

- `version` answers a name that is not blank.
- `capabilities` keeps the contract's rules for a declaration, its
  `default_model` is not blank, and `posture_models` keys only postures
  `postures` names.
- Each posture the capabilities name launches: `launch` answers an `argv`
  whose first word is not blank, and writes nothing outside the `config_dir`
  and `worktree` it was asked for. A failure names each path it made,
  changed or removed outside them.
- A posture the capabilities do not name is refused `unsupported`. Where
  they name all three, this check is skipped.
- A `resume` of a session id no agent has answers an `argv` or is refused
  `missing`.
- A `read` of no seats answers no rows.
- `read`, then `context`, answers each of the adapter's recorded cases (see
  [Recorded cases](#recorded-cases)). The `context` cases are skipped for an
  adapter whose capabilities do not declare `context`.
- A verb the contract does not name exits 2, and so does a request at a
  `schema_version` of 2.

Every answer a check reads is also held to the document `fleet agent schema`
prints, at its verb's response, and a check whose answer does not pass it
fails.

The recorded cases and the two exit checks are asked of an adapter
executable. For the adapter fleet has built in they are skipped, saying so.

Each check prints one line on standard output, in the same order every
run: `PASS` and the check's name, `SKIP` with why the check does not apply,
or `FAIL` with what the adapter answered instead. A line is printed as soon
as its check answers, and every check runs whatever the one before it
answered. The last line names the agent, by the name `version` answers,
else by the adapter's name or path, and counts the checks that passed,
failed and were skipped.

It exits 0 when no check failed, and 1 when one did.

### Recorded cases

An adapter carried by a pack ships its recorded cases in a `fixtures/`
directory beside its `adapter.toml`, and so does an adapter executable named
by path that sits beside one. `--fixtures <dir>` names another directory
instead. An adapter with neither has the two case checks skipped.

Each case is a directory of its own under the verb it replays:

```text
fixtures/
  read/
    <case>/
      request.json
      answer.json
      env.json
  context/
    <case>/
      ...
```

- `request.json` is the whole request the case sends, envelope included.
- `answer.json` is the answer the adapter owes, without its
  `schema_version`.
- `env.json`, where there is one, is a JSON object of variables, each a
  string, set for this case's call alone, on top of the environment fleet
  runs the adapter with. An adapter replays a recording through it, such as
  a variable naming a program that prints what its agent printed once.

`{fixture}` in any string of `request.json` or `env.json` is replaced by the
case's own absolute directory before the call, so a recording can sit
beside the case that reads it.

A case passes when the adapter exits 0 and its answer, `schema_version` taken
out, equals `answer.json`. A `last_write` is compared by whether it is there,
not by its value: it is the time a recorded file was last written, which a
checkout does not keep. A case that fails is named with the first field
where the two part, such as `idle: seats[0].activity: answered "busy", and
answer.json holds "idle"`. Every case runs, and the check's line names each
one that failed.

### Checking against the agent itself

`--live` also starts the agent, which costs a model turn, so it is never the
default. Without it, the five live steps print `SKIP`.

The session runs in a git repository fleet makes inside the temporary
directory, under a `config_dir` beside it, on a tmux server of its own on
the socket `fleet-check-<pid>`, with `<pid>` the process id of the
`fleet agent check` you ran. It is never the server your fleet's seats run
on. The session starts the way a seat's does: the pane's command is
`launch`'s `argv`, and its environment is exactly the variables fleet sets
for a seat with `launch`'s `env` over them.

The session launches under the first posture the capabilities name, on
`default_model`, or on the model `--model` names; `--model` names the model
of every `launch` and `resume` the checks ask for, the offline ones too.
The steps, each one line:

1. The launched session reads `idle` within 60 seconds, and `read` names
   its `session_id`.
2. One word, typed into the session as a bracketed paste and then a
   separate submit, reads `busy` within 10 seconds, and then `idle` within
   60.
3. `context`, asked while the session still runs, answers `turns` of at
   least 1 and a `last_write` no earlier than the second the word was typed.
   It is skipped for an adapter that does not declare `context`.
4. Two interrupts, typed a moment apart, end the session, and the pane reads
   dead within 5 seconds.
5. A `resume` of the `session_id` from step 1 reads `idle` within 60
   seconds, as that same session: a `read` naming any other `session_id`
   fails.

A step that reads `blocked` fails at once, naming what it waits on where
`read` names it. Where
the session never comes up, the steps after the first are skipped.

When the run ends, however it ends, fleet ends the `fleet-check-<pid>` server
with everything on it, removes its socket, and removes the temporary
directory. Under `--live`, the summary also names the version of the agent
`version` answered, since the live steps are a reading of that version.

With the Claude Code adapter fleet has built in, on a machine whose Claude
Code is logged in:

```sh
$ fleet agent check --live --model haiku
PASS  version
PASS  capabilities
PASS  each declared posture launches
SKIP  an undeclared posture is refused unsupported: every posture is declared, so there is none to refuse
PASS  resume of a session nobody has
PASS  read of no seats
SKIP  read answers each fixture: no fixtures: the adapter ships no fixtures/ beside an adapter.toml, and --fixtures names none
SKIP  context answers each fixture: no fixtures: the adapter ships no fixtures/ beside an adapter.toml, and --fixtures names none
SKIP  an unknown verb exits 2: the adapter is the one built into fleet, and this is asked of an adapter executable
SKIP  a later schema_version exits 2: the adapter is the one built into fleet, and this is asked of an adapter executable
PASS  live: a launched session comes up idle
PASS  live: a typed turn reads busy, then idle
PASS  live: context counts the turn
PASS  live: the ended session leaves its pane dead
PASS  live: a resume comes back idle as the same session
agent check: claude 2.1.280 — 10 passed, 0 failed, 5 skipped
```

It exits 0.

### When it refuses

| Situation | Exit | What you see |
| --- | --- | --- |
| a check failed | 1 | its `FAIL` line, and the summary's count of failures |
| `--adapter` names a relative path, or nothing | 2 | ``fleet agent check: --adapter takes an absolute path to an executable or the name of an agent adapter an installed pack carries, and `<value>` is neither`` |
| `--fixtures` names no directory | 2 | ``fleet agent check: --fixtures names `<dir>`, which is not a directory`` |
| nothing executable is at the path `--adapter` names | 3 | ``fleet agent check: --adapter names `<path>`, which is not an executable file`` |
| no installed pack carries the name | 3 | ``fleet agent check: no agent adapter named `<name>` in the installed packs — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/agent/<name> --version v0.2.0` installs the one fleet-packs carries`` |
| the installed packs do not resolve | 3 | ``fleet agent check: no agent adapter named `<name>` resolves:`` and the reason |
| the fleet's own `fleet.toml` does not read, or names an adapter that cannot be opened | 3 | `fleet agent check:` and the reason |
| `--live` finds no tmux | 3 | `fleet agent check: --live starts the agent on tmux, and there is none:` and the reason |

Only a failed check prints check lines; every other refusal comes before
any check runs, and prints none.

## See also

- [The store contract](store.md): the store adapter's contract, whose call,
  envelope and exit table this one shares.
- [The controller and seats](seats.md): seat ids, names and machine names,
  named and transient seats, and the policy a seat's session starts under.
- [Runs and workflows](runs.md): `run.until`, which waits on the record.
