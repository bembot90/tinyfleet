# Getting started

This area takes you from a checkout of fleet to a fleet that runs: building
the binary, writing a project's fleet with `fleet create`, which installs
the packs its store and its agent run from, adding the tiny pack, and
bringing the controller up with `fleet start`. Every step here is one you
take once per machine or once per project.

## Terms

- **Machine directory**: the one directory where fleet keeps this machine's
  state: the seat list, the stream of events, the installed packs and the
  defaults. `FLEET_DIR` names it outright. Otherwise it is `.fleet` under
  `FLEET_HOME`, or under your home when `FLEET_HOME` is unset. On Linux, when
  `XDG_STATE_HOME` is set and `FLEET_DIR` is not, it is
  `XDG_STATE_HOME/fleet`, whatever `FLEET_HOME` says. The examples below
  write it as `<machine>`.
- **Embedded fleet**: a fleet whose policy file, `fleet.toml`, sits at the root
  of the one project it works on.
- **Standalone fleet**: a fleet whose `fleet.toml` lives in its own directory.
  Each project it works on declares itself to it with a `.fleet/project.toml`
  and is registered on the machine.
- **Defaults**: the record templates and health checks built into the
  binary. `fleet create` and `fleet start` write them into
  `<machine>/defaults`. See [Packs](packs.md).
- **Agent pack**: the pack that carries the agent adapter a fleet's seats
  run through (see [The agent contract](agent.md)). The one `fleet create`
  installs is the claude-code pack, whose adapter runs Claude Code. The
  examples below write the claude-code pack's name, `claude-code`, which is
  also its adapter's, as `<agent>`.

## Building fleet

You build fleet from a checkout with cargo. The workspace builds one binary,
named `fleet`:

```sh
$ cargo build --release
```

The binary lands at `target/release/fleet`, or at `target/debug/fleet` for a
plain `cargo build`. Fleet also calls other tools as you go further: `git` to
fetch a pack, `tmux` to host the sessions the controller starts, and `deno`
for the tiny pack's workflows. The agent's pack and the store's pack run
what they need: the claude-code pack runs `deno` and Claude Code, `claude`,
and the bd pack runs `deno` and `bd`. The version of each that fleet
supports is in [What fleet runs on](#what-fleet-runs-on).

`--version` prints the version and nothing else:

```sh
$ fleet --version
0.1.0
```

It exits 0. `fleet` with no command prints the command list on standard error
and exits 2.

### Putting fleet on your PATH

Fleet has no install command. Putting the built binary on your shell's `PATH`
is your own step: put `target/release/fleet`, or a copy of it, on it.
The controller's service runs the file that ran `fleet start`, named by its
full path, so start the controller from the copy you mean to keep.

## What fleet runs on

Fleet runs on macOS and Linux. Windows is not supported: fleet does not
build there.

Fleet runs three other tools itself, and installs packs from one
repository, fleet-packs, at `https://github.com/bembot90/fleet-packs`.
Every one but `git` has a supported version, or for tmux a minimum, and a
doctor check measures the one you have installed against it.

| Tool | Supported version | What measures it |
| --- | --- | --- |
| tmux (`tmux`) | 3.7b or later | the `tmux-version` doctor check |
| Deno (`deno`) | 2.9.7, pinned by the `ts` pack | the `runtime-version` doctor check, which `fleet run` runs before it opens a run, and the `ts` pack's `deno-version` |
| fleet-packs | the tag `v0.2.0`, which `fleet create` installs the agent's pack and the store's pack at | the `fleet-packs-version` doctor check, over every pack `packs.lock` pins from that repository |
| `git` | none: fleet pins no version | nothing |

The agent is its pack's, and so is the store.
The claude-code pack pins the Claude Code it runs, declares the versions its
adapter was measured against, and carries the doctor checks that measure
them. The bd pack pins the versions its store runs on and carries the check
that measures them. Each pack's README says what the pack needs, how to
install it, and which check measures what. It sits beside the adapter's
entry: installed, at
`<machine>/packs/<name>/adapters/<kind>/<name>/README.md`, and in
fleet-packs at `adapters/<kind>/<name>/adapters/<kind>/<name>/README.md`,
where `<kind>` is `agent` or `store`.

Another version of the agent, or of a pack from fleet-packs, is named and not
refused: the verbs and the controller still run on it. Deno is
different: while the `runtime-version` check is red, `fleet run` refuses to
open a run. See [Runs and workflows](runs.md).

The controller runs every agent seat's session inside tmux, on a tmux server
of fleet's own (see [The sessions the controller starts](#the-sessions-the-controller-starts)).
`fleet start` refuses to start without a tmux. An older tmux is named by the
`tmux-version` check and not refused.

The controller compares the agent's version with the versions its adapter
declares it was measured against, on every poll; see
[The controller and seats](seats.md#the-agents-version).

### Running a doctor check

`tmux-version`, `fleet-packs-version` and `runtime-version` come with the
defaults every fleet gets, which `fleet create` and `fleet start` write
under `<machine>/defaults`. A pack carries checks of its own, under its
directory in `<machine>/packs`: the ts pack's `deno-version` (below), the bd
pack's check on what its store runs, and the claude-code pack's two checks
on Claude Code, which the claude-code pack's README describes. Each is a
shell script you run with `sh`, or through `fleet doctor`. On another
version, a check says `broken`, says how to install the supported one, and
exits 1.

`fleet-packs-version` reads `packs.lock` through `fleet pack list` and
compares every pack installed from
`https://github.com/bembot90/fleet-packs` with `v0.2.0`, so run it through
`fleet doctor`, inside the project:

```sh
$ fleet doctor fleet-packs-version
pass fleet-packs-version (defaults) — fleet-packs-version: holds — <agent>, bd, ts at v0.2.0
doctor 1 check — 1 pass, 0 finding, 0 could not tell
```

It exits 0. A lock with no line from that repository passes too, with
`nothing installed from fleet-packs`; a pack installed from a checkout you
named with `--packs-from` is not read. A pack from the repository at another
tag is a finding and exits 1, naming it with
`` `fleet pack remove <source>` and then `fleet pack add <source> --version v0.2.0` ``.

The bd pack's README says which `bd` its check and its store run, and
the claude-code pack's README says which Claude Code its checks and its
adapter run.

`tmux-version` holds when `tmux -V` answers
3.7b or later, reading a trailing letter as a later release (3.7b is after
3.7a, which is after 3.7), and says whether fleet's own tmux server is
running and with how many sessions; either way it passes:

```sh
$ fleet doctor tmux-version
pass tmux-version (defaults) — tmux-version: holds — no server is running on socket fleet
doctor 1 check — 1 pass, 0 finding, 0 could not tell
```

It exits 0. A running server reads `a server is running on socket fleet with
<n> session(s)`. A tmux that answers no release number, such as a build from
source, holds too, and the line says it was taken as recent. An older tmux,
or none, is `broken` and exits 1, naming the minimum and how to install it:

```sh
$ sh <machine>/defaults/doctor/tmux-version/run.sh
tmux-version: minimum tmux 3.7b; `tmux -V` answers: tmux 3.4
tmux-version: broken — tmux 3.4 is older than the minimum 3.7b, so the calls the controller makes to it were not measured on it. Install tmux 3.7b or later: brew install tmux on macOS, or the distribution's tmux package on Linux
```

`tmux-version` asks the binary `FLEET_TMUX_BIN` names, or else the first
`tmux` on your `PATH`.

The `ts` pack's own Deno check runs the same way, from the pack:

```sh
$ sh <machine>/packs/ts/doctor/deno-version/run.sh
deno-version: pinned deno 2.9.7
deno-version: deno resolved from PATH (<path>)
deno-version: `deno --version` answers: deno 2.9.7 (<build>)
deno-version: holds
```

It exits 0.

Two rows of `fleet doctor` are built in rather than carried by a pack, and
run after every check the layers carry. `store-adapter` opens the store
adapter the project's `[store] adapter` names, and `agent-adapter` opens the
agent adapter the fleet's `[agent] adapter` names, each the way every other
command opens it. Each adapter is asked its `version` and its
`capabilities` and nothing else, so neither row writes to the store, makes
a store of its own or starts a session. A pack's `doctor/store-adapter/` or
`doctor/agent-adapter/` entry is not run, and the row stays the built-in
one.

```sh
$ fleet doctor store-adapter agent-adapter
pass store-adapter (built-in) — <store> <version> via <adapter> — schema 1, capabilities valid
pass agent-adapter (built-in) — <name> <version> via <agent> — measured <version>
doctor 2 checks — 2 pass, 0 finding, 0 could not tell
```

It exits 0. The agent row names the agent the adapter drives, as its
`version` names it, the version the adapter answered, and the versions it
was measured against. An installed version that is not among them still
passes, and the row says to re-measure:

```sh
$ fleet doctor agent-adapter
pass agent-adapter (built-in) — <name> <installed> via <agent> — installed <installed> is not among the measured <version>: re-measure
doctor 1 check — 1 pass, 0 finding, 0 could not tell
```

It exits 0. Where an adapter cannot be opened, does not answer in time, or
answers something fleet cannot read, its row says could not tell and
carries the reason fleet gives. A name no installed pack carries names the
`fleet pack add` line that installs it:

```sh
$ fleet doctor store-adapter
could not tell store-adapter (built-in) — no store adapter named `nowhere` in the installed packs — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/store/nowhere --version v0.2.0` installs the one fleet-packs carries
doctor 1 check — 0 pass, 0 finding, 1 could not tell
```

It exits 3. Capabilities that break the contract are a finding naming the
field, and so is an agent adapter whose `version` answers `null`, which
means no agent is installed for it to drive:

```sh
$ fleet doctor agent-adapter
finding agent-adapter (built-in) — <agent> answers that no <name> is installed (its version is null)
doctor 1 check — 0 pass, 1 finding, 0 could not tell
```

It exits 1. Whether an adapter keeps the rest of its contract is
`fleet store check`'s question; see
[Checking an adapter](store.md#checking-an-adapter).

## Creating a fleet

`fleet create` writes a project's fleet from inside the project's directory.
It asks three questions: embedded or standalone, which store, and which
agent. It writes the defaults into the machine directory, installs the
store's pack and the agent's pack, then writes the file for the mode into the
directory you ran it in. It also lists you, whoever ran it, as the fleet's
first seat: a human one, under this machine's identity. It makes nothing of
the store's own in the project (see [The store's pack](#the-stores-pack)).
Everything it prints goes to standard error.

On a terminal, each question is a list you pick from, and Enter takes the
first row: `embedded`; `bd`, the bd pack's store; and
`claude-code`, the claude-code pack's agent. Where standard input is not a
terminal, pass the mode as a flag. The store and the agent have defaults:
with no terminal, no `--store` means `bd`, and no `--agent` means the
claude-code pack's `claude-code`.

### The store's pack

The store is where the fleet's items live, and a pack carries it. The answer
`bd` installs the bd store's pack from fleet-packs at the tag this binary
supports, the same install as:

```sh
$ fleet pack add https://github.com/bembot90/fleet-packs//adapters/store/bd --version v0.2.0
```

It fetches with git, so `create` needs the network. The bd pack imports the
`ts` pack from the same repository, and both are installed and pinned in
`packs.lock` at that tag. The file `create` writes names the store:

```toml
[store]
adapter = "bd"
```

Where a pack named `bd` is already installed on the machine, `create` leaves
it as it stands and says where it came from.

The bd pack's store keeps the project's items in a beads board inside the
project, which `create` does not make: the bd pack's README says how to make
one. Once the board is there, `fleet item list --ready` reads the items that
are ready:

```sh
$ fleet item list --ready
<item> · Name the stamp's fields  [open]  type task · labels none · assignee none
```

It exits 0. Before the board is there, it refuses and exits 3, with the
store's reason (see [When it refuses](#when-it-refuses)).

`--store none` installs no store pack, writes no `[store]` table, and prints
the line that installs the pack later:

```text
store: none installed — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/store/bd --version v0.2.0` installs one
```

`--packs-from <dir>` installs the packs from a checkout of fleet-packs
instead, at the same tag: the checkout's git history is what is read, not its
working tree. The lock records the checkout's path as the source, and the
`store: none installed` line names the checkout.

When the fetch fails, `create` refuses and exits 1 with no fleet file
written, naming git's reason and `fleet create --store none` as the way to
create the fleet without the pack. The defaults are already written by then.

### The agent's pack

The agent is what the fleet's agent seats run on, and a pack carries it too.
The one answer is `claude-code`, which installs the claude-code pack from
fleet-packs at the same tag, the same install as:

```sh
$ fleet pack add https://github.com/bembot90/fleet-packs//adapters/agent/<agent> --version v0.2.0
```

The claude-code pack imports the `ts` pack as well: where the store's pack
has not already installed ts, it is installed and pinned with the agent's.
The file `create` writes names the agent:

```toml
[agent]
adapter = "<agent>"
```

`--agent` takes that name, spelled that way; any other name is refused
before anything is asked or written. Where a pack of that name is already
installed, `create` leaves it as it stands, as it does the store's.

The bd pack and the claude-code pack each import `ts`, and both install
beside the one ts. With both defaults, `create` installs the bd pack with
ts, then the agent's pack over them. Here `<checkout>` is a checkout of
fleet-packs, named with `--packs-from` (below):

```sh
$ fleet create --embedded --packs-from <checkout>
store: bd v0.2.0 at <sha>, installed and pinned — <machine>/packs/bd
store: ts v0.2.0 at <sha>, which bd imports — <machine>/packs/ts
agent: <agent> v0.2.0 at <sha>, installed and pinned — <machine>/packs/<agent>
created embedded fleet — <project>/fleet.toml
...
```

It exits 0. With `--store none`, `create` installs the agent's pack and the
ts it imports:

```sh
$ fleet create --embedded --store none --packs-from <checkout>
store: none installed — `fleet pack add <checkout>//adapters/store/bd --version v0.2.0` installs one
agent: <agent> v0.2.0 at <sha>, installed and pinned — <machine>/packs/<agent>
agent: ts v0.2.0 at <sha>, which <agent> imports — <machine>/packs/ts
created embedded fleet — <project>/fleet.toml
...
```

It exits 0. See [Packs](packs.md#imports-are-one-level-deep).

When the agent's pack cannot be fetched, `create` refuses and exits 1 with
no fleet file written, naming git's reason and the `fleet pack add` line
that installs the pack.

A standalone project is not asked which agent: it runs on the agent of the
fleet it is declared to, and `create --standalone` installs no agent's pack.

### An embedded fleet

```sh
$ fleet create --embedded --store none --packs-from <checkout>
store: none installed — `fleet pack add <checkout>//adapters/store/bd --version v0.2.0` installs one
agent: <agent> v0.2.0 at <sha>, installed and pinned — <machine>/packs/<agent>
agent: ts v0.2.0 at <sha>, which <agent> imports — <machine>/packs/ts
created embedded fleet — <project>/fleet.toml
guards: shell-trap on, record on
telemetry: off — nothing leaves this machine
defaults: installed 0.1.0 — <machine>/defaults
seat: you — human human-4d9ad4b3, listed as [seats.01a0e17f-8e64-76a3-ba77-c80c4d9ad4b3] — <project>/fleet.toml
identity: minted — who acts here when no --by is given — <machine>/identity.toml
next: fleet start — it installs the service on its first run and loads it
```

It exits 0. `<checkout>` is a checkout of fleet-packs; without
`--packs-from`, the packs come from `https://github.com/bembot90/fleet-packs`.
The `fleet.toml` it writes opens with a comment naming the command that wrote
it, then carries `[guards]` with `shell-trap.enabled = true` and
`record.enabled = true`, `[telemetry]` with `enabled = false`, `[agent]` with
`adapter = "<agent>"`, `[store]` with `adapter = "bd"` (none with
`--store none`), and `[seats]`, under a comment showing what a seat's table
looks like, with one table in it: yours. The example row in that comment
carries the model the agent's adapter names as its default, and no model
line where the adapter does not answer.

```toml
[seats.01a0dc3e-c99c-77f0-af71-8fe28bdbe55a]
kind = "human"
```

The id is this machine's identity, kept in `<machine>/identity.toml`. The
`identity:` line appears only when this call minted it; on a machine that
already had one, `create` lists that one. A verb you run on this machine
without `--by` acts as it. See
[The controller and seats](seats.md#who-you-are-identitytoml).

A later `create` in a directory that already holds a `fleet.toml` refuses.

`create` writes into the directory you run it in and does not look above it.
Run inside a subdirectory of an existing fleet, it writes a second
`fleet.toml` there.

### A standalone project

A standalone fleet is an embedded fleet in a directory of its own, with other
projects declared to it. In each project, run `fleet create --standalone`.
Before the fleet has been started on this machine, name its directory with
`--fleet`:

```sh
$ fleet create --standalone --store none --fleet <fleet>
store: none installed — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/store/bd --version v0.2.0` installs one
fleet: registered on this machine — <fleet>/fleet.toml — <machine>/config.json
registered <name> at <project> — <machine>/projects.toml
created standalone fleet — <project>/.fleet/project.toml
defaults: already at 0.1.0 — <machine>/defaults
seat: you — human human-4d9ad4b3, already listed — <fleet>/fleet.toml
next: fleet start — it installs the service on its first run and loads it
```

It exits 0. Here the embedded fleet's `create` had already listed you. With
no `--store`, the store's pack is installed as for an embedded fleet, and
where the embedded fleet's `create` installed it already, the `store:` line
says so and leaves it as it stands. `<name>` is the project directory's own
name. The project runs on the fleet's agent: `--agent` naming another than
the one the fleet's `fleet.toml` names, or than `<agent>` where it names
none, is refused. The
`fleet:` line appears only when this call wrote `<machine>/config.json`. Once a fleet is
registered on the machine, `--fleet` is not needed. Registering a project
also writes a `project.registered` event to the stream, and that event mints
this machine's identity where it has none, so a standalone `create` prints
no `identity:` line.

The `seat:` line lists you in the fleet's own `fleet.toml`, the one `--fleet`
names. When you are already listed there it says `already listed`. When that
file cannot take the table, the project is still declared and registered,
the line says `not listed —` and why, and names `fleet seat add --human`,
which lists you once the file is put right.

`.fleet/project.toml` carries `[project]` with `name`, `primary` (the project
directory) and `worktrees` (a sibling directory named after the project with
`-worktrees` on the end). Where the project's store answers a prefix for its
ids, the file carries it as `item_prefix`; otherwise that line is left
commented out for you to fill in. It carries the `[store]` table too,
as the embedded file does. Where a `.fleet/project.toml` is already there,
`create --standalone` reads every key in it, writes nothing over it, and
registers it.

### The first agent seat

A fleet has no agent seat until you add one. Run `fleet seat add --agent`
inside the project; it writes the seat's table into the fleet's own
`fleet.toml` and prints the `git worktree add` line for its worktree:

```sh
$ fleet seat add --agent --name Orla --model <model>
added: agent orla-10b55fd3 — [seats.01a0d5ff-b143-7781-9967-5ccd10b55fd3] in <project>/fleet.toml
next: git worktree add <project>-worktrees/orla-10b55fd3 <a branch>, then fleet start renders it
01a0d5ff-b143-7781-9967-5ccd10b55fd3
```

`<model>` is a model the agent takes, in its own words. Leave `--model` out
and the seat runs on the default model the agent's adapter declares, the one
the example row in `fleet.toml`'s comment carries, unless
`[controller] default_model` names another.

Cut the worktree with that line, set `[core] reviewer` in `fleet.toml` to
the seat deliveries go to (any seat argument: its name, its machine name or
its id), and run `fleet start`. See
[The controller and seats](seats.md#adding-seats).

## Adding the tiny pack

A fresh fleet runs on the defaults, its agent's pack and its store's pack.
The tiny pack is the doctrine layered over them, and its workflows run under
the TypeScript runtime the `ts` pack declares. Both live in the fleet-packs
repository, `https://github.com/bembot90/fleet-packs`, written
`<fleet-packs>` below: tiny at `tiny` and ts at `runtimes/ts`. tiny imports
ts, and so do the claude-code pack and the bd pack, so on a machine where
`fleet create` has run, ts is already installed and the add installs tiny
beside them:

```sh
$ fleet pack add <fleet-packs>//tiny --version <version>
added tiny <version> at <sha> — <machine>/packs/tiny
pinned in <machine>/packs.lock
```

It exits 0. `<version>` is a tag, a branch such as `main`, or `sha:` followed
by a full 40-character commit.

On a machine where ts is not installed, the repository holds both, so one
add installs the two, and ts is pinned at the same commit as tiny:

```sh
$ fleet pack add <fleet-packs>//tiny --version <version>
added tiny <version> at <sha> — <machine>/packs/tiny
added ts <version> at <sha>, which tiny imports — <machine>/packs/ts
pinned in <machine>/packs.lock
```

It exits 0. See [Packs](packs.md#imports-are-one-level-deep).
The pack is fetched with git, so what installs is the
committed pack at that version, not the working tree. The `ts` pack pins
`deno` 2.9.7 as its runtime. With tiny installed and ts missing,
`fleet run takeoff` refuses. How packs layer is in [Packs](packs.md).

## Starting the controller

`fleet start` brings the controller up. It is shown here without output: it
loads a user service, which a scratch fleet does not do.

```sh
$ fleet start
```

Run it inside the embedded fleet's project, or inside any directory once the
machine's seat list names a fleet. Before it does anything, it refuses when
the controller is already running, when no installed pack carries the
fleet's agent adapter, when that adapter answers that its agent is not
installed, and when it cannot find `tmux` (see
[Starting the controller](seats.md#starting-the-controller)). It runs the
agent's adapter, and looks for `tmux`, on a fixed search path, not your
shell's `PATH`: `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`,
`/opt/homebrew/bin`, `/usr/local/bin` and `~/.local/bin` on macOS, and
`~/.local/bin`, `/usr/local/bin`, `/usr/bin` and `/bin` on Linux.
`FLEET_TMUX_BIN`, set to an absolute path, names the tmux binary instead.
How the agent is found on that path is its pack's to say:
the claude-code pack's README says how it finds Claude Code.

Then it does the first-run work, one line each on standard error, each line
starting `first run:`. It makes the machine directory, and writes the seat
list (`<machine>/config.json`) naming this fleet's `fleet.toml` where no seat
list is there yet. When the directory resolves to a project, it renders the
agent seats in `[seats]` into the seat list, keyed on that project; human
seats, yours among them, are listed and never rendered. It writes the
service file. It writes `[telemetry] enabled = false` into `fleet.toml` where
the file does not say. The service file is:

- on macOS, `~/Library/LaunchAgents/dev.fleet.controller.plist`, which runs
  this binary with `observe`, restarts it when it exits, and sends its output
  to `<machine>/service.out.log` and `<machine>/service.err.log`;
- on Linux, `~/.config/systemd/user/dev.fleet.controller.service`, whose
  output goes to the user journal.

When `FLEET_DIR` is set, the service file carries it. Then `start` writes the
defaults, and a `defaults:` line says whether it installed them, refreshed
them, or found them already there. Last, it loads the service and waits up to
30 seconds for the controller's own `controller.started` event on the stream.
When the event arrives, it prints
`started dev.fleet.controller — controller.started at sequence <n> — <machine>`
and exits 0.

A second `fleet start` after `fleet stop` repeats none of the first-run
work, and says
`first run: every step was already done, so this start repeated none of it`.

### Running in the foreground

`fleet start --foreground` does the same first-run work, including writing
the service file, then runs the controller's loop in your terminal and loads
nothing. It prints `running in this process — nothing was loaded`.

## Fleet inside a session

What wires fleet into a seat's session is its agent's pack.
The claude-code pack carries a plugin, and its adapter names it on every
launch and resume, so every seat's session loads it and no setting of yours
names it. It runs `fleet prime` when a session starts, and before every Bash
command it runs `fleet guard --adapter <agent>`, which runs the declared
guard classes (see [Guards](guards.md)).

A session of your own loads the same plugin from fleet-packs, and
the claude-code pack's README says how for Claude Code: under "Loading it in
a session of your own" it gives the two lines that install it, and the one
that loads it for a single session.

`fleet prime` names the fleet's installed packs and its guards on its first
line, the project's store on its second, then prints the resolved rules. In
the fleet `fleet create` made with both defaults:

```sh
$ fleet prime
fleet 0.1.0 — packs: bd, <agent>, ts; guards: shell-trap on, record on
store: bd 1.3.0 (adapter bd)
Five things no verb guesses, each one a lesson somebody already paid for:
...
```

It exits 0, always. With no pack installed, the first line says
`packs: none installed`. The second line names the store by the name and
version it answers with, then the adapter that answered: the name the
project's [`[store] adapter`](store.md#choosing-an-adapter) gives, `bd`, the
bd pack's adapter, where it names none, or the file name of the adapter it
names by path. The project
is the seat's worktree when the directory is one, and otherwise the nearest
directory, at or above it, that holds a `fleet.toml` or a
`.fleet/project.toml`. The line does not compare the version with the
supported one; for the bd pack's store, the bd pack's own doctor check does
(see [Running a doctor check](#running-a-doctor-check)). When the store cannot be
opened, or does not answer within two seconds, the line reads
`store: could not be read — ` and the reason, so a machine where no installed
pack carries the store's adapter gets that line too, naming the
`fleet pack add` line that installs it.
With no project, it reads `store: none (no project here)`. Outside every fleet it
prints one line, `fleet 0.1.0 — no fleet config found above <directory>`. When the directory
is a seat's worktree, it also lists the items assigned to that seat.

### Which fleet binary the plugin runs

fleet sets `FLEET_BIN` to the absolute path of its own binary in every
seat's session it starts (see
[What the controller does each poll](seats.md#what-the-controller-does-each-poll)).
The claude-code pack's plugin runs the binary `FLEET_BIN` names, and no
other. A session of your own needs `FLEET_BIN` set once, in the environment
the session starts in, as the claude-code pack's README shows; without it,
the plugin blocks every Bash command in the session.

### The sessions the controller starts

Each agent seat's session is its agent run interactively, as the process of
its own tmux session on a tmux server that is fleet's alone: the
server on the socket `fleet`, which `tmux -L fleet` reaches, started with no
configuration file, so your own `~/.tmux.conf` does not shape it. The tmux
session is named by the seat's full id, and `fleet seat attach <seat>`
opens it in your terminal (see
[The controller and seats](seats.md#attaching-to-a-seat)).

A transient seat's session runs under a configuration directory of its own,
which the agent's adapter fills as it launches the session.
The claude-code pack's launch seeds it with your onboarding answers and the
seat's worktree marked trusted, so the session starts without asking either
(see [Spawning a transient seat](seats.md#spawning-a-transient-seat)). A
named seat's session runs under your own configuration of its agent.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `fleet create` with no mode flag and no terminal | 2 | `fleet create: fleet: embedded or standalone? — stdin is not a terminal; answer it with --embedded` | Pass `--embedded` or `--standalone`. |
| `--agent` names an agent fleet installs no pack for | 2 | ``fleet create: no agent pack answers to `<name>` — this fleet installs <agent>`` | Pass `--agent <agent>`, or leave it out. |
| `--standalone` with an `--agent` that is not the fleet's | 2 | ``fleet create: --agent <name> is not this fleet's agent — <fleet>/fleet.toml runs `<adapter>`, and a project declared to it runs on that one; drop --agent`` | Drop `--agent`. |
| `--store` names a store fleet installs no pack for | 2 | ``fleet create: no store pack answers to `<name>` — this fleet installs bd, or none`` | Pass `--store bd` or `--store none`. |
| `--standalone --store none` together with `--packs-from` | 2 | `fleet create: --packs-from names where the packs come from, and a standalone project with --store none installs none — drop one of them` | Drop one of the two. |
| `--packs-from` names no directory | 2 | `fleet create: --packs-from <dir> is not a directory — name a checkout of fleet-packs` | Name a checkout of fleet-packs. |
| The store's pack cannot be fetched: no network, or a checkout without the tag | 1 | ``fleet create: the store's pack was not installed, so no fleet file was written: git <step> exited <n>: <git's reason> — `fleet create --store none` creates the fleet without one`` | Fix what git names, or pass `--store none` and install the pack later. |
| The agent's pack cannot be fetched, or does not layer over what is installed | 1 | ``fleet create: the agent's pack was not installed, so no fleet file was written: <the reason> — `fleet pack add <fleet-packs>//adapters/agent/<agent> --version v0.2.0` installs it`` | Fix what the reason names. |
| `--embedded` together with `--standalone` or `--fleet` | 2 | `error: the argument '--embedded' cannot be used with '--standalone'` | Pass one mode. `--fleet` goes with `--standalone`. |
| The directory already holds a `fleet.toml` | 1 | `fleet create: <project>/fleet.toml is already here, so this directory is already a fleet` | Nothing to do: the fleet exists. |
| The directory holds a `.fleet` with no `project.toml` in it | 1 | `fleet create: <project>/.fleet is here and carries no .fleet/project.toml — a directory that is not this project's own declaration is not one this verb will write into` | Move the `.fleet` directory aside. |
| `--standalone` with no fleet registered on the machine and no `--fleet` | 1 | ``fleet create: no fleet is registered on this machine — <machine>/config.json is not there; name an embedded fleet's directory with --fleet, run `fleet start` in one first, or create this one with --embedded`` | Pass `--fleet <fleet>`. |
| `--fleet` names a directory with no `fleet.toml` | 1 | ``fleet create: --fleet <dir> holds no fleet.toml — name the directory of an embedded fleet, the one `fleet create --embedded` wrote that file in`` | Name the fleet's own directory. |
| `--standalone` again, in a project whose declaration has no `item_prefix` | 1 | ``fleet create: <project>/.fleet/project.toml carries no `[project] item_prefix`, which a project declared to this fleet needs`` | Set `item_prefix` in `[project]`. |
| `fleet pack add` before `fleet create` or `fleet start` has run on this machine | 1 | ``fleet pack add: the defaults this binary carries are not at <machine>/defaults — `fleet start` writes them, and every template resolves through them`` | Run `fleet create` first. |
| `fleet run takeoff` with tiny installed and ts not | 1 | ``fleet run: `tiny` carries `workflows/takeoff.ts` and declares no [runtime] table, and no installed pack it imports declares one: `tiny` imports `ts`, which is not installed — `fleet pack add <fleet-packs>//runtimes/ts --version <version>` adds it`` | Run the `fleet pack add` it names. |
| A verb that reads the store where no installed pack carries the store's adapter, as after `fleet create --store none` | 3 | ``fleet item list: no store adapter named `bd` in the installed packs — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/store/bd --version v0.2.0` installs the one fleet-packs carries`` | Run the `fleet pack add` it names. |
| `fleet item list --ready` or `fleet run` where the store cannot read the project's items, as with the bd pack's store before the project has a board | 3 | `fleet item list: the store's ready set could not be read: ` or `fleet run: the work graph could not be read: `, then the adapter's reason | Make the board, as the bd pack's README says. |
| `fleet start` with no `fleet.toml` above the directory and no fleet named by the seat list | 1 | ``fleet start: no fleet.toml above this directory and no fleet named by <machine>/config.json — `fleet create` writes one`` | Run it inside the fleet's project, or `fleet create` first. |
| `fleet start` while the controller is running | 1 | `fleet start: the controller is already running as pid <pid>; its last tick was <stamp>` | Nothing to do, or `fleet stop` first. |
| `fleet start` where no installed pack carries the fleet's agent adapter | 3 | ``fleet start: no agent adapter named `<agent>` in the installed packs — `fleet pack add https://github.com/bembot90/fleet-packs//adapters/agent/<agent> --version v0.2.0` installs the one fleet-packs carries — nothing was loaded`` | Run the `fleet pack add` it names. |
| `fleet start` where the agent's adapter answers that its agent is not installed | 3 | ``fleet start: <agent> answers that no <name> is installed (its version is null), so no session can be started through it — nothing was loaded; the search path is <path>`` | Install the agent where the search path finds it, as its pack's README says. |
| `fleet start` cannot find `tmux` | 3 | ``fleet start: no `tmux` on the constructed child PATH (<path>) — nothing was loaded; the search path is <path>`` | Install tmux into a directory on that path (`brew install tmux` on macOS, the distribution's `tmux` package on Linux), or set `FLEET_TMUX_BIN`. |
| The service loaded and no `controller.started` arrived within 30 seconds | 3 | `fleet start: no controller.started was written within 30s — nothing fresh reached <machine>/events.jsonl; what the service printed is at <machine>/service.err.log` | Read the service's log. |

## See also

- [Packs](packs.md): what the tiny and ts packs carry, and how packs layer
  over the defaults.
- [The controller and seats](seats.md): adding seats, stopping the
  controller, and what it does once it runs.
- [Guards](guards.md): what the guards a seat's hook runs refuse.
- [The agent contract](agent.md): the agent adapter an agent pack carries,
  and how to write one.
- [Runs and workflows](runs.md): `fleet run` and the takeoff workflow.
- [Status and the event stream](status.md): reading the controller once it
  is up.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
