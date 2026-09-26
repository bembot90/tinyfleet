# Packs

A pack is a folder of agents, skills, routines, workflows, health checks,
adapters and templates that a fleet runs with. You install packs on a machine with
`fleet pack add`, and fleet reads its templates, its rules file and its
workflows through the installed packs, laid over the defaults the `fleet`
binary carries. This page covers installing, listing and removing packs, how
they layer and shadow one another, the defaults every fleet gets, the format a
pack is checked against, and the settings a pack takes from `fleet.toml`.

## Terms

- **Machine directory**: where fleet keeps a machine's packs. It is the
  directory `FLEET_DIR` names when that is set, and otherwise `.fleet` in your
  home directory (`FLEET_HOME`, when set, stands in for the home directory; on
  Linux, `$XDG_STATE_HOME/fleet` when `XDG_STATE_HOME` is set). Installed
  packs sit in its `packs/` directory, the defaults in its `defaults/`
  directory, and the record of both in its `packs.lock`.
- **The defaults**: the files the `fleet` binary carries inside itself — the
  brief, the rules, the JSON shapes a seat and a reviewer hand in, the guards'
  wiring and the health checks. They are not a pack and cannot be removed;
  they are the bottom layer every pack sits over.
- **Source**: where a pack is fetched from — a git URL or a local path to a
  repository, optionally followed by `//` and the pack's directory inside it.
- **Layer**: one pack's directory in the ordered list fleet reads through, top
  first, with the defaults at the bottom.
- **Shadow**: a file a higher layer carries at the same path as a lower one.
  The higher file replaces the lower one whole.
- **Shadow registry**: `assets/shadow-registry.toml`, the list of files the
  packs above the bottom layer can shadow in it. The defaults publish one.
- **Import**: a pack named under `[imports]` in another pack's `pack.toml`.
  The importing pack layers above it.
- **Setting**: a key a pack declares in its `pack.toml` and you set in
  `fleet.toml` under `[packs.<name>]`.

## The defaults every fleet gets

`fleet create` writes the defaults into `<machine-dir>/defaults/` and pins them
in `packs.lock`, and every `fleet start` does the same. You get them before you
install any pack, and every verb resolves through them.

```sh
$ fleet create --embedded --agent claude_code
...
defaults: installed 0.1.0 — <machine-dir>/defaults
...
```

The line says which of three things happened:

- `installed <version>`: no copy stands there, and fleet writes the binary's
  set.
- `already at <version>`: the copy on disk is the binary's set, file for file,
  and fleet writes nothing. The version is the one its line in `packs.lock`
  already carries: the binary that wrote the copy.
- `refreshed <version> — the copy that stood here held another set`: the copy
  is the one its line in `packs.lock` pinned, the binary carries a different
  set, and fleet replaces the copy with the binary's set.

The other two lines carry the `fleet` binary's own version. Whether the copy
is current is judged by its content, not by any version.

fleet never writes over a copy it cannot account for. When the files under
`defaults/` were edited after they were pinned, or a `defaults/` directory
stands there with no line in `packs.lock` behind it, fleet names it:

```text
a defaults directory edited since the line that pinned it was written is at <machine-dir>/defaults; this binary will not write over it — remove it, or keep it
```

`fleet create` stops on this and exits 1. With `--store none` it stops after
it has written `fleet.toml`, or `.fleet/project.toml` for `--standalone`;
installing a store's pack writes the defaults first, so there it stops before
any fleet file is written.
`fleet start` prints it on a `defaults: left as it stands — ` line and carries
on. Remove the `defaults/` directory and the next `fleet create` or
`fleet start` writes the binary's set again.

The defaults carry these files, and a pack can shadow every one of them:

- `assets/brief.md` and `assets/rules.md`
- `assets/delivery.schema.json`, `assets/question.schema.json` and
  `assets/findings.schema.json`: the JSON shapes of a seat's delivery, a
  seat's question and a reviewer's findings
- `overlay/per-provider/claude/hooks.json`,
  `overlay/per-provider/claude/permissions.json`
- `doctor/guards-installed/`, `doctor/isolation-pair/`,
  `doctor/runtime-version/`, `doctor/claude-code-version/`,
  `doctor/fleet-packs-version/` and
  `doctor/adopt-board/`, each a `doctor.toml` and a `run.sh`

`<machine-dir>/defaults/assets/shadow-registry.toml` lists the same files with
one line each on what they are for.

A schema a pack shadows changes what the brief shows a seat, never what the
verb accepts: `fleet deliver`, `fleet hold` and `fleet review --return` read
their file against the shape built into the binary. A brief whose delivery or
question schema, as the layers resolve it, does not describe that shape is not
rendered: `fleet brief` exits 3, naming the schema and what it gets wrong,
and `fleet dispatch` exits 3 with the order it wrote standing.

## Installing a pack

`fleet pack add` fetches a pack from a git source at a version, checks it,
moves it into `<machine-dir>/packs/<name>` and pins it in `packs.lock`, with
the packs it imports that the same checkout holds (see
[Imports](#imports-from-the-same-checkout)).

```sh
$ fleet pack add <fleet-packs>//runtimes/ts --version v0.1.0
added ts v0.1.0 at <sha> — <machine-dir>/packs/ts
pinned in <machine-dir>/packs.lock
$ fleet pack add <fleet-packs>//tiny --version v0.1.0
added tiny v0.1.0 at <sha> — <machine-dir>/packs/tiny
pinned in <machine-dir>/packs.lock
```

`<fleet-packs>` is the fleet-packs repository,
`https://github.com/bembot90/fleet-packs`, which holds the tiny pack at `tiny`
and the ts pack it imports at `runtimes/ts`. It also holds one pack per store
adapter, under `adapters/store/<name>`: `fleet create` installs the bd pack
from there (see [Getting started](getting-started.md#the-stores-pack)), and
the bd pack imports ts too, so a machine holds tiny or the bd pack, not both
(see [Imports are one level deep](#imports-are-one-level-deep)). `v0.1.0` is
the tag this binary supports.

Each exits 0. The directory is named by the `name` in the pack's own
`pack.toml`, not by the source.

The source is a git URL or a local path to a repository. When the pack is not
at the repository's root, add `//` and its directory inside the repository:
`<fleet-packs>//runtimes/ts`. The part after `//` stays inside the repository; a `..`
in it is refused.

`--version` is required, and takes one of three forms:

- a tag, such as `v0.1.0`;
- a branch name, such as `main`;
- `sha:` and a full 40-character commit.

fleet records the version as you typed it and the commit it resolved to on the
day. A caret range such as `^1.0` is refused: there is no registry to resolve
it against.

Before anything lands, fleet checks the fetched pack against the format (see
[Checking a pack](#checking-a-pack)) and against every pack already installed
(see [How packs layer](#how-packs-layer)). A refusal at any step leaves the
installed packs and `packs.lock` as they were, and exits 1. Git runs with its
terminal prompt turned off, so a source that needs credentials this machine
does not have is refused instead of waiting at a prompt.

### Imports from the same checkout

A pack's imports that are not installed yet, and that its own checkout holds,
are installed with it, out of the same clone and pinned at the same commit,
one `added` line each:

```sh
$ fleet pack add <fleet-packs>//tiny --version v0.1.0
added tiny v0.1.0 at <sha> — <machine-dir>/packs/tiny
added ts v0.1.0 at <sha>, which tiny imports — <machine-dir>/packs/ts
pinned in <machine-dir>/packs.lock
```

An import already installed is left as it is. An import the checkout does not
hold is not fetched and does not refuse the pack: it is named on standard
error, with the add that installs it where the manifest's source says where it
lives:

```text
fleet pack add: `<pack>` imports `<import>`, which is not installed — `fleet pack add <source> --version <version>` adds it
```

### Moving a pack to another version

A pack whose name is already installed is refused:

```text
fleet pack add: a pack named `ts` is already installed at `<machine-dir>/packs/ts` — remove it before adding another
```

To move a pack to another version, remove it and add it again. A pack another
installed pack imports cannot be removed while the importer is installed, so
you remove the importer first.

### Where it installs

`--packs-dir <dir>` installs into another packs directory, and fleet then reads
the defaults from `defaults/` beside that directory. `--lock <file>` pins in
another lock file. Without them, both are the machine directory's.

## Listing installed packs

`fleet pack list` prints `packs.lock` as a table, one row per line, in order of
source.

```sh
$ fleet pack list
name      source                      version  commit    fetched
defaults  embedded:defaults           0.1.0    embedded  <fetched>
ts        <fleet-packs>//runtimes/ts  v0.1.0   <sha>     <fetched>
tiny      <fleet-packs>//tiny         v0.1.0   <sha>     <fetched>
```

It exits 0. The defaults have their own row, with the source
`embedded:defaults` and the commit `embedded`. `fetched` is a UTC time, such as
`2026-09-23T14:07:08Z`. A line that carries no name prints `-` in the name
column. A lock file that does not exist yet, or an empty one, prints the header
row alone and exits 0. A lock fleet cannot read or parse prints
`fleet pack list:` and the reason on standard error, and exits 3. `--lock
<file>` reads another lock file.

`packs.lock` is TOML, one table per source:

```toml
schema = 1

[packs."<fleet-packs>//tiny"]
name = "tiny"
version = "v0.1.0"
commit = "<sha>"
fetched = "<fetched>"
```

## Removing a pack

`fleet pack remove` takes the source exactly as you typed it to
`fleet pack add` — the `source` column of `fleet pack list`, not the name.

```sh
$ fleet pack remove <fleet-packs>//tiny
removed tiny v0.1.0 — <machine-dir>/packs/tiny
dropped <fleet-packs>//tiny v0.1.0 from <machine-dir>/packs.lock
```

It exits 0. It deletes the pack's directory first and then drops its line from
`packs.lock`. The `dropped` line carries the source and the version, which is
the `fleet pack add` that puts the pack back.

When the directory is already gone, fleet still drops the line, and says so
on standard error:

```text
fleet pack remove: `<machine-dir>/packs/<name>` was already gone — the lock line is dropped
```

`fleet pack remove` refuses, exits 1 and changes nothing when:

- the source is not in the lock, including when you give the pack's name;
- another installed pack imports it: `` `tiny` imports `ts` ``;
- the source is `embedded:defaults`, the binary's own defaults.

It does not touch `fleet.toml`: a `[packs.<name>]` section for the pack you
removed stays there, and `fleet run` refuses it (see
[Pack settings in fleet.toml](#pack-settings-in-fleettoml)).

## How packs layer

A verb that reads a template, the rules file or a workflow reads it through
the layers: the installed packs, then the defaults at the bottom. For each
file path, the highest layer that carries the path answers.

### The order

fleet orders the installed packs so that each pack sits above the packs it
imports, and otherwise by name, a before z. The defaults are always last. An
import is matched by its name — the key under `[imports]` — against each
installed pack's own name.

`fleet prime` prints the order on its first line, top first, and never names
the defaults:

```sh
$ fleet prime
fleet 0.1.0 — packs: tiny, ts; guards: shell-trap on, record on, release-ref on, production-write on
...
```

With no pack installed, the line reads `packs: none installed`.

Every directory under `<machine-dir>/packs/` that holds a `pack.toml` is a
layer, whether `packs.lock` names it or not.

### Imports are one level deep

Only the top layer can declare imports. A pack below it that declares one is
refused:

```text
fleet pack add: layer `tiny` declares its own import `ts` — imports are one level deep
```

The top layer is the pack that sorts first by name among the packs no other
installed pack imports. So a machine holds at most one pack that declares
imports, and that pack's name sorts before every other pack nothing imports.
With `tiny` and `ts` installed, a pack named `zed` installs and layers below
`ts`; a pack named `abc` is refused with the message above, because it would
take the top.

When the layers cannot be resolved, `fleet prime` prints
`packs: could not be resolved — ` and the first reason.

### Shadowing

A file a pack carries at the same path as a lower layer replaces that file
whole. The `pack.toml` is never shadowed. With `tiny` installed, `fleet prime`
prints `tiny`'s `assets/rules.md`, not the defaults'.

Between installed packs, any file can shadow any other. Over the defaults, a
pack can shadow only the files the defaults' shadow registry lists, which is
every file the defaults carry (see
[The defaults every fleet gets](#the-defaults-every-fleet-gets)).

Agent names are the exception: an agent directory name is unique across every
layer. Two layers that both carry `agents/<name>` are refused, whatever their
order:

```text
the agent name `scout` is in both `top` and `base` — a collision is refused, never resolved by precedence
```

An installed pack whose `pack.toml` names it `defaults` is refused too: that is
the bottom layer's name.

Routine files are not layered. fleet reads the `orders/` directory of every
installed pack, and two routines with one name, in any two packs, are both
refused rather than one shadowing the other.

## Checking a pack

`fleet pack check` validates one pack directory against the format, and
writes nothing.

```sh
$ fleet pack check <machine-dir>/packs/tiny --over <machine-dir>/packs/ts
pack tiny 0.1.0 — schema 3
slot agents: 2 entries
slot assets: 3 entries
slot doctor: 3 entries
slot overlay: 1 entry
slot skills: 12 entries
slot workflows: 1 entry
resolved 43 paths and 2 agents across 2 layers, 0 shadowed
```

It prints the pack's name, version and schema, a `runtime` line when the pack
declares one, and one line per slot it carries with the count of entries
directly under it; for `adapters`, the count of `adapters/<kind>/<name>/`
directories. It exits 0 for a valid pack. For a pack with defects it
prints each one on standard error, prefixed with the pack's name, and exits 1:

```text
bad: unknown top-level name `notes` — a pack holds pack.toml and the eight slots
bad: the slot `workflows` is not a directory
bad: [pack] schema is 2, not 3
```

`--over <dir>` lays the pack above the directories you name, in the order you
give them, lowest last, and applies the layering rules above: it prints the
`resolved` line, or each refusal on standard error and exits 1. The last
`--over` directory is the bottom layer, and its shadow registry, when it
publishes one, decides what the layers above can shadow. `--over` takes pack
directories only: the defaults directory has no `pack.toml` and is refused as
a layer.

### The format

A pack's top level holds `pack.toml` and any of eight slot directories:
`agents`, `skills`, `orders`, `doctor`, `overlay`, `assets`, `workflows` and
`adapters`. Any other name at the top level is a defect, and so is a slot that
is a file. The file browser's own files — `.DS_Store`, `Thumbs.db` and
`desktop.ini` — are read past everywhere.

Each slot's entries:

- `agents/<name>/` holds `agent.toml` or `prompt.template.md`. An entry here
  is not an agent adapter: an agent adapter is an `adapters/agent/<name>/`
  entry, and its contract is [The agent contract](agent.md).
- `skills/<name>/` holds `SKILL.md`.
- `doctor/<name>/` holds `doctor.toml`.
- every file directly under `orders/` parses as TOML.
- `adapters/<kind>/<name>/` holds `adapter.toml`, where `<kind>` is `store` or
  `agent`. Any other name directly under `adapters/` is a defect.
- `overlay/`, `assets/` and `workflows/` hold whatever the pack puts there.

`adapter.toml` holds one table:

```toml
[adapter]
name = "x"                  # required; the adapter's directory name
kind = "store"              # required; the kind directory it sits in
version = "0.1.0"           # required
description = "..."         # optional
entry = "main.sh"           # required; an executable file in the directory
```

Every value is a string, and any other key or table is a defect. An entry
that is in the directory but not executable is a defect whose line is the
fix:

```text
p: the adapter entry `p/adapters/store/x/main.sh` is not executable — `chmod +x p/adapters/store/x/main.sh` makes it one
```

A store adapter a pack carries is one a project can name in `[store]
adapter`; see [The store contract](store.md).

`pack.toml` holds up to five tables, and any other top-level table is a defect:

```toml
[pack]
name = "tiny"          # required; the installed directory's name
version = "0.1.0"      # optional
schema = 3             # required, and 3
description = "..."    # optional

[imports.ts]           # one table per import, keyed by the import's name
source = "../ts"       # required
version = "0.1.0"      # required

[runtime]              # the workflow runtime; all four keys required
name = "deno"
version = "2.9.7"
bundle = "deno bundle -o {bundle} {entry}"
run = "deno run ... {bundle}"

[config."takeoff.test"]  # one table per setting; see below
description = "..."
type = "string"
```

`[[named_session]]` tables are the fifth: each needs `template` and `mode`, and
takes an optional `scope`. No verb acts on them.

`[pack]` takes no keys beyond those four. `[runtime]` takes no keys beyond its
four, and its `bundle` and `run` lines use only the placeholders `{entry}`,
`{bundle}`, `{run_dir}`, `{fleet}` and `{inputs}`. What fleet does with the
runtime is on [Runs and workflows](runs.md).

A pack that publishes a shadow registry at `assets/shadow-registry.toml` holds
every path it lists. The registry is `schema = 1` and one `[[shadow]]` table
per file, each with a `path` and a `purpose`.

## Doctor checks

A doctor check is a `doctor/<name>/` entry in a pack or in the defaults: a
script that looks at one thing on this machine and says whether it holds.
`fleet doctor` runs them, from inside a project, and writes nothing itself.

```sh
$ fleet doctor
pass adopt-board (defaults) — adopt-board: nothing to adopt — no items read
pass claude-code-version (defaults) — claude-code-version: holds
pass fleet-packs-version (defaults) — fleet-packs-version: nothing installed from fleet-packs — no line of the lock names https://github.com/bembot90/fleet-packs
pass guards-installed (defaults) — record bare-id: configured — [project] item_prefix
pass isolation-pair (defaults) — isolation-pair: holds
pass runtime-version (defaults) — nothing pinned: no installed pack declares a [runtime] table
doctor 6 checks — 6 pass, 0 finding, 0 could not tell
```

It exits 0. Each row is the verdict, the check's name, the layer that
carries it in parentheses, and the last line the check printed. The rows come
in name order, each as soon as its check finishes, and the summary line comes
last. Name checks to run only those:

```sh
$ fleet doctor guards-installed
finding guards-installed (defaults) — record bare-id: not configured — [project] item_prefix
  shell-trap record-backtick: configured
  shell-trap modifier: configured
  shell-trap unsplit-variable: configured
  shell-trap pipe-rc: configured
  shell-trap false-alternative: configured
  record notes-replace: configured
  record sql-write: configured
  record bare-id: not configured — [project] item_prefix
doctor 1 check — 0 pass, 1 finding, 0 could not tell
```

It exits 1. A row that did not pass is followed by every line the check
printed, standard output first, each indented by two spaces.

`--json` prints one document on standard output instead, whose `data` holds
a `checks` array with one object per row, the `counts` of each verdict, and
the `verdict` over them all; the rows and the summary move to standard error.
See
[Exit codes and conventions](conventions.md#verbs-whose-exit-means-something-narrower).

### What a check is

`doctor/<name>/doctor.toml` takes two keys: `description`, one line on what
the check is for, and `run`, the script to run, named relative to the entry.
fleet reads no other key. The directory's name is the check's name.

Each file resolves through the layers on its own, like any other pack file:
a pack can shadow a check's `doctor.toml`, its script, or both, and a pack
can carry checks of its own under new names. The layer a row names is the
one that carries the `doctor.toml`. The defaults' shadow registry lists each
of their checks' two files.

### How a check runs

fleet runs the script with `sh`, from the project root, with three variables
set beside everything else in your environment:

- `FLEET_PACK_DIR`: the directory of the layer that carries the check's
  `doctor.toml`. `runtime-version` is the exception, below.
- `FLEET_PROJECT`: the project root.
- `FLEET_BIN`: the `fleet` binary that is running.

The check runs on your `PATH`. It is given 60 seconds; a check still running
then is killed, together with its process group.

The check's exit is its verdict:

| The check | Verdict |
| --- | --- |
| exits 0 | pass |
| exits 1 | finding |
| exits any other code, 3 among them; is killed; runs past 60 seconds; or cannot be started | could not tell |

A check whose entry has no `doctor.toml`, whose `doctor.toml` does not parse
or names no `run`, or whose script no layer carries, could not tell, and
nothing runs.

`fleet doctor` exits 0 when every check passed, 1 when one reported a
finding, and 3 when one could not tell. 3 wins over 1.

### runtime-version

`runtime-version` runs once for each installed pack that declares a
`[runtime]` table, with `FLEET_PACK_DIR` naming that pack, on the `PATH`
`fleet run` gives that pack's workflows (see [Runs and workflows](runs.md)).
Its row names the pack it measured:

```sh
$ fleet doctor runtime-version
pass runtime-version for ts (defaults) — runtime-version: holds
doctor 1 check — 1 pass, 0 finding, 0 could not tell
```

A pack whose `pack.toml` cannot be read or does not parse gets a
could-not-tell row saying why. When no installed pack declares a `[runtime]`
table, `runtime-version` is one passing row, `nothing pinned: no installed
pack declares a [runtime] table`, and nothing runs.

### When fleet doctor refuses

Outside a project, `fleet doctor` runs nothing:

```sh
$ fleet doctor
fleet doctor: no `fleet.toml` and no `.fleet/project.toml` above <dir> — `fleet create` writes one
```

It exits 3. A name no layer carries is refused before any check runs, with
the names the layers do carry:

```sh
$ fleet doctor nosuch
fleet doctor: no doctor check named nosuch — the layers carry: adopt-board, claude-code-version, fleet-packs-version, guards-installed, isolation-pair, runtime-version
```

It exits 2.

## Pack settings in fleet.toml

A pack declares the settings it takes in its `pack.toml`; you set them in
`fleet.toml`; a workflow the pack carries reads them.

### Declaring a setting

Each setting is a `[config.<key>]` table. A key with a dot in it can be written
quoted or as nested tables — `[config."retry.limit"]` and
`[config.retry.limit]` declare the same setting. The examples below are a pack
named `demo` that carries a workflow named `hello`:

```toml
[config.greeting]
description = "What the hello workflow says."
type = "string"
default = "hello"

[config."retry.limit"]
description = "How many times hello retries."
type = "integer"

[config.loud]
description = "Whether hello shouts."
```

- `description` is required.
- `type` is optional, and one of `string`, `integer`, `float`, `boolean` or
  `array`. A setting with no type takes a value of any type.
- `default` is optional, and of the declared type.

Each part of a key is letters, digits, `_` or `-`. One setting's key is never
the start of another's: `retry` and `retry.limit` in one pack is a defect.
`fleet pack check` names each broken declaration:

```text
cfg: pack.toml is missing `config.nodesc.description`
cfg: [config.badtype]'s type `text` is not one of string, integer, float, boolean, array
cfg: [config.retry] and [config.retry.limit] overlap — one setting's name is never the start of another's, or a `fleet.toml` value could be read as either
```

The `tiny` pack declares two settings, `takeoff.test` and `takeoff.touched`,
both strings with no default. What its takeoff workflow does with them is on
[Runs and workflows](runs.md).

### Setting a value

You set a pack's settings in `fleet.toml`, in a section named for the pack.
Dotted and quoted keys both work:

```toml
[packs.demo]
greeting = "hey"
retry.limit = 3
loud = true
```

### When fleet reads them

`fleet run` reads the `[packs]` table when it opens a run, and no other verb
reads it. It checks the whole table against every installed pack, whichever
workflow you run, before anything is written. Then it pins the settings of the
pack that carries the workflow under `[config]` in the run's `inputs.toml`:
each declared default, with every value `fleet.toml` sets laid over it. A
setting with no default that `fleet.toml` does not set is left out.

```sh
$ cat <machine-dir>/runs/<run>/inputs.toml
by = "seat:<you>"
entry = "workflows/hello.ts"
pack = "demo"
started_at = "<started>"
workflow = "hello"

[config]
greeting = "hey"
loud = true
"retry.limit" = 3

[inputs]
```

A re-run of that run reads the settings from this file, not from `fleet.toml`.

`fleet run` refuses, exits 1 and opens nothing when `[packs]` sets a key the
pack does not declare, a value of the wrong type, or a section for a pack that
is not installed. It names every problem at once:

```sh
$ fleet run hello --by <you>
fleet run: `hello` is not opened — [packs.demo] sets `colour`, which the pack `demo` does not declare — its pack.toml declares greeting, loud, retry.limit
  [packs.demo] sets `greeting` to an integer, and the pack `demo` declares it string
  fleet.toml has a [packs.nope] section and no pack named `nope` is installed — the installed packs are demo, ts
```

A `fleet.toml` that does not parse stops `fleet run` with exit 3.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `fleet pack add` with no `--version`, or no source | 2 | `error: the following required arguments were not provided:` and the usage line | Give both. |
| An empty source | 1 | `fleet pack add: no source — a source is a git URL or a local path` | Name the repository. |
| A source ending in `//` with nothing after it | 1 | ``fleet pack add: `<source>` is not a source — a git URL or a local path, optionally followed by `//` and a subdirectory inside it`` | Name the directory after `//`, or drop the `//`. |
| A `..` after `//` | 1 | ``fleet pack add: `<source>` names a subdirectory outside the repository — the part after `//` is a path inside it`` | Name a directory inside the repository. |
| A caret range as the version | 1 | ``fleet pack add: `^1.0` is a caret range — pin a tag or `sha:<40 hex>`; ranges resolve against a registry this verb does not have`` | Give a tag, a branch or `sha:<40 hex>`. |
| `sha:` with anything but 40 hex characters | 1 | ``fleet pack add: `sha:abc` is not a commit — `sha:` takes a full 40-character hex sha`` | Give the full commit. |
| Git cannot clone the source or check out the version | 1 | `fleet pack add: git clone exited 128: ` or `git checkout exited 1: ` and git's first line | Run the same git command by hand to see the rest. |
| The repository has no directory at the path after `//` | 1 | ``fleet pack add: `<source>`: the repository holds no `<path>` `` | Check the path inside the repository at that version. |
| The fetched pack fails the format | 1 | `fleet pack add: <name>: ` and each defect | Fix the pack; `fleet pack check` shows every defect. |
| A pack of that name is already installed | 1 | ``fleet pack add: a pack named `<name>` is already installed at `<dir>` — remove it before adding another`` | `fleet pack remove` it first. |
| The defaults are not in the machine directory | 1 | ``fleet pack add: the defaults this binary carries are not at <dir> — `fleet start` writes them, and every template resolves through them`` | Run `fleet create` or `fleet start`. |
| A pack below the top layer declares an import | 1 | ``fleet pack add: layer `<pack>` declares its own import `<import>` — imports are one level deep`` | The message names the pack pushed down, which is not always the one you were adding. Install at most one pack that declares imports, and no other pack nothing imports whose name sorts before it. |
| Two layers carry the same agent name | 1 | ``the agent name `<agent>` is in both `<upper>` and `<lower>` — a collision is refused, never resolved by precedence`` | Rename one pack's agent. |
| A pack names itself `defaults` | 1 | ``the pack at <dir> calls itself `defaults`, which is the binary's own bottom layer — rename it or take it out`` | Rename the pack in its `pack.toml`. |
| The lock cannot be read or written, on add or remove | 1 | `fleet pack add: ` or `fleet pack remove: `, then `packs.lock cannot be read: ` and the reason | Fix or move the lock file. |
| `fleet pack remove` of a source the lock does not hold | 1 | ``fleet pack remove: `<source>` is not in `<lock>` `` | Copy the source from `fleet pack list`. |
| `fleet pack remove` of a pack another pack imports | 1 | ``fleet pack remove: `<importer>` imports `<name>` `` | Remove the importer first. |
| `fleet pack remove embedded:defaults` | 1 | ``fleet pack remove: `embedded:defaults` is the binary's own defaults, the bottom layer every pack resolves over — it is not removable`` | Nothing: the defaults stay. |
| `fleet pack remove` of a lock line with no `name` | 1 | ``fleet pack remove: `<source>` has no `name` in the lock, so the directory it installed cannot be found — re-add it with ...`` | The `fleet pack add` the message names is refused while the pack's directory stands. Delete the pack's directory under `<machine-dir>/packs/` by hand, then run it. |
| `fleet pack list` on a lock it cannot read or parse | 3 | `fleet pack list: ` and the reason | Fix or move the lock file. |
| `fleet pack check` with no directory | 2 | `error: the following required arguments were not provided:` and the usage line | Name the pack directory. |
| `fleet pack check` on a pack with defects, or a layering that refuses | 1 | each defect or refusal on standard error | Fix what it names. |
| `fleet create` over an edited or unrecorded `defaults/` | 1 | `fleet create: ` and ``... is at <dir>; this binary will not write over it — remove it, or keep it`` | Remove `<machine-dir>/defaults/`. |
| `fleet run` with a `[packs]` key, value or section the installed packs do not declare | 1 | ``fleet run: `<workflow>` is not opened — `` and each problem | Fix the `[packs]` section it names. |
| `fleet run` with a `fleet.toml` that does not parse | 3 | `fleet run: the policy in force at <file> does not parse, so the settings its [packs] table sets cannot be read: ` and the parse error | Fix `fleet.toml`. |

## See also

- [Getting started](getting-started.md): `fleet create` and `fleet start`,
  which write the defaults.
- [Runs and workflows](runs.md): how `fleet run` resolves a workflow through
  the layers and what a workflow does with its settings.
- [Items and the record](items.md): the verbs that render the brief and read
  the files the schemas describe.
- [Guards](guards.md): what the guard wiring in the defaults' overlay turns on.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
