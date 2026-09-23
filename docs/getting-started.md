# Getting started

This area takes you from a checkout of fleet to a fleet that runs: building
the binary, writing a project's fleet with `fleet create`, adding the tiny
pack, bringing the controller up with `fleet start`, and wiring fleet into a
Claude Code session through its plugin. Every step here is one you take once
per machine or once per project.

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
- **Defaults**: the record templates, guard wiring and health checks built into
  the binary. `fleet create` and `fleet start` write them into
  `<machine>/defaults`. See [Packs](packs.md).
- **The plugin**: the fleet checkout is also a Claude Code plugin named
  `fleet`. It runs `fleet prime` when a session starts and the four guards
  before each shell command.

## Building fleet

You build fleet from a checkout with cargo. The workspace builds one binary,
named `fleet`:

```sh
$ cargo build --release
```

The binary lands at `target/release/fleet`, or at `target/debug/fleet` for a
plain `cargo build`. Fleet also calls other tools as you go further: `git` to
fetch a pack, `bd` for the project's items, `claude` for the sessions the
controller starts, and `deno` for the tiny pack's workflows.

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

## Creating a fleet

`fleet create` writes a project's fleet from inside the project's directory.
It asks two questions, embedded or standalone and which agent, then writes the
file for that mode into the directory you ran it in. It also writes the
defaults into the machine directory and prints the edit that adds the first
seat. It installs no pack and does not create or change the project's `bd`
store: run `bd init` yourself. Everything it prints goes to standard error.

On a terminal, each question is a list you pick from, and Enter takes the
first row: `embedded` and `claude_code`. Where standard input is not a
terminal, pass the answers as flags instead.

### An embedded fleet

```sh
$ fleet create --embedded --agent claude_code
created embedded fleet — <project>/fleet.toml
guards: shell-trap on, record on
telemetry: off — nothing leaves this machine
defaults: installed 0.1.0 — <machine>/defaults
first seat: add this to <project>/fleet.toml, then make its worktree

    [seats.a-seat]
    model = "claude-opus-5"

    [core]
    reviewer = "a-seat"

    git worktree add <project>-worktrees/a-seat <a branch>

next: fleet start — it installs the service on its first run and loads it
```

It exits 0. The `fleet.toml` it writes opens with a comment naming the agent
and the command that wrote it, then carries three tables: `[guards]` with
`shell-trap.enabled = true` and `record.enabled = true`, `[telemetry]` with
`enabled = false`, and an empty `[seats]`. A later `create` in a directory
that already holds a `fleet.toml` refuses.

`create` writes into the directory you run it in and does not look above it.
Run inside a subdirectory of an existing fleet, it writes a second
`fleet.toml` there.

### A standalone project

A standalone fleet is an embedded fleet in a directory of its own, with other
projects declared to it. In each project, run `fleet create --standalone`.
Before the fleet has been started on this machine, name its directory with
`--fleet`:

```sh
$ fleet create --standalone --agent claude_code --fleet <fleet>
fleet: registered on this machine — <fleet>/fleet.toml — <machine>/config.json
registered <name> at <project> — <machine>/projects.toml
created standalone fleet — <project>/.fleet/project.toml
defaults: already at 0.1.0 — <machine>/defaults
first seat: add this to <fleet>/fleet.toml, then make its worktree

    [seats.a-seat]
    model = "claude-opus-5"

    [core]
    reviewer = "a-seat"

    git worktree add <project>-worktrees/a-seat <a branch>

next: fleet start — it installs the service on its first run and loads it
```

It exits 0. `<name>` is the project directory's own name. The first line
appears only when this call wrote `<machine>/config.json`. Once a fleet is
registered on the machine, `--fleet` is not needed. Registering a project
also writes a `project.registered` event to the stream.

`.fleet/project.toml` carries `[project]` with `name`, `primary` (the project
directory) and `worktrees` (a sibling directory named after the project with
`-worktrees` on the end). Where the project's `.beads/config.yaml` names an
`issue-prefix`, the file carries it as `item_prefix`; otherwise that line is
left commented out for you to fill in. Where a `.fleet/project.toml` is
already there, `create --standalone` reads every key in it, writes nothing
over it, and registers it.

### The first seat

A fleet has no seat until you add one: `create` prints the edit and does not
make it. Add the `[seats.<name>]` table and `[core] reviewer` it shows to the
fleet's own `fleet.toml`, run `fleet start`, and cut the worktree with the
`git worktree add` line it printed. See
[The controller and seats](seats.md).

## Adding the tiny pack

A fresh fleet runs on the defaults alone. The tiny pack is the doctrine layered
over them, and its workflows run under the TypeScript runtime the `ts` pack
declares. Adding tiny does not add ts, so add both, from a checkout of fleet,
after `fleet create` has run on this machine:

```sh
$ fleet pack add <checkout>//packs/tiny --version <version>
added tiny <version> at <sha> — <machine>/packs/tiny
pinned in <machine>/packs.lock
$ fleet pack add <checkout>//packs/ts --version <version>
added ts <version> at <sha> — <machine>/packs/ts
pinned in <machine>/packs.lock
```

Each exits 0. `<version>` is a tag, a branch, or `sha:` followed by a full
40-character commit. The pack is fetched with git, so what installs is the
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
the controller is already running, and when it cannot find the `claude`
binary. It looks for `claude` on a fixed search path, not your shell's `PATH`:
`/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`, `/opt/homebrew/bin`,
`/usr/local/bin` and `~/.local/bin` on macOS, and `~/.local/bin`,
`/usr/local/bin`, `/usr/bin` and `/bin` on Linux. `FLEET_CLAUDE_BIN`, set to
an absolute path, names the binary instead.

Then it does the first-run work, one line each on standard error, each line
starting `first run:`. It makes the machine directory, and writes the seat
list (`<machine>/config.json`) naming this fleet's `fleet.toml` where no seat
list is there yet. When the directory resolves to a project, it renders the
seats in `[seats]` into the seat list, keyed on that project. It writes the
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

## Using fleet in a Claude Code session

The checkout is a Claude Code plugin. To load it for one session:

```sh
$ claude --plugin-dir <checkout>
```

To install it into a project's Claude Code settings, add the checkout as a
marketplace, then install the plugin from it. The marketplace and the plugin
are both named `fleet`:

```sh
$ claude plugin marketplace add <checkout>
$ claude plugin install fleet@fleet --scope project
```

A session with the plugin loaded runs `fleet prime` when it starts, and
before every shell command it runs the four guards in order: `shell-trap`,
`record`, `release-ref` and `production-write`. See [Guards](guards.md).

`fleet prime` names the fleet's installed packs and its guards on its first
line, then prints the resolved rules:

```sh
$ fleet prime
fleet 0.1.0 — packs: tiny, ts; guards: shell-trap on, record on, release-ref on, production-write on
Five things no verb guesses, each one a lesson somebody already paid for:
...
```

It exits 0, always. With no pack installed, the first line says
`packs: none installed`. Outside every fleet it prints one line,
`fleet 0.1.0 — no fleet config found above <directory>`. When the directory
is a seat's worktree, it also lists the items assigned to that seat.

The plugin carries a `version` skill that runs `fleet --version`, and the
tiny pack's rituals as skills: `wake`, `handoff`, `rest`, `clock-out`,
`morning`, `corrections-review`, `praise`, `preboard`, `takeoff`, `report`
and `runbook`. These skills come from the plugin's own checkout, not from
the tiny pack installed in the machine directory.

### Which fleet binary the plugin runs

The plugin's hooks run its own `bin/fleet`, which runs the first of:

1. `FLEET_BIN`, when it is set to an absolute path of an executable file;
2. `<checkout>/target/release/fleet`;
3. `<checkout>/target/debug/fleet`.

So a checkout loaded with `--plugin-dir` needs a build in it, and a copy of
the plugin with no `target` directory needs `FLEET_BIN` set in the
environment Claude Code runs in. When none of the three answers, `bin/fleet`
prints why on standard error and exits 127.

### The sessions the controller starts

The controller starts each seat's session with the plugin only when the
fleet's `fleet.toml` names the plugin's directory:

```toml
[controller]
plugin_dir = "<checkout>"
```

A relative path is read from the directory `fleet.toml` is in. With no
`plugin_dir`, the seats' sessions start without fleet's hooks.

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| `fleet create` with no mode flag and no terminal | 2 | `fleet create: fleet: embedded or standalone? — stdin is not a terminal; answer it with --embedded` | Pass `--embedded` or `--standalone`. |
| `fleet create` with no `--agent` and no terminal | 2 | `fleet create: fleet: which agent? — stdin is not a terminal; answer it with --agent` | Pass `--agent claude_code`. |
| `--agent` names an agent fleet has no adapter for | 2 | ``fleet create: no adapter answers to `<name>` — this fleet knows claude_code`` | Pass `--agent claude_code`. |
| `--embedded` together with `--standalone` or `--fleet` | 2 | `error: the argument '--embedded' cannot be used with '--standalone'` | Pass one mode. `--fleet` goes with `--standalone`. |
| The directory already holds a `fleet.toml` | 1 | `fleet create: <project>/fleet.toml is already here, so this directory is already a fleet` | Nothing to do: the fleet exists. |
| The directory holds a `.fleet` with no `project.toml` in it | 1 | `fleet create: <project>/.fleet is here and carries no .fleet/project.toml — a directory that is not this project's own declaration is not one this verb will write into` | Move the `.fleet` directory aside. |
| `--standalone` with no fleet registered on the machine and no `--fleet` | 1 | ``fleet create: no fleet is registered on this machine — <machine>/config.json is not there; name an embedded fleet's directory with --fleet, run `fleet start` in one first, or create this one with --embedded`` | Pass `--fleet <fleet>`. |
| `--fleet` names a directory with no `fleet.toml` | 1 | ``fleet create: --fleet <dir> holds no fleet.toml — name the directory of an embedded fleet, the one `fleet create --embedded` wrote that file in`` | Name the fleet's own directory. |
| `--standalone` again, in a project whose declaration has no `item_prefix` | 1 | ``fleet create: <project>/.fleet/project.toml carries no `[project] item_prefix`, which a project declared to this fleet needs`` | Set `item_prefix` in `[project]`. |
| `fleet pack add` before `fleet create` or `fleet start` has run on this machine | 1 | ``fleet pack add: the defaults this binary carries are not at <machine>/defaults — `fleet start` writes them, and every template resolves through them`` | Run `fleet create` first. |
| `fleet run takeoff` with tiny installed and ts not | 1 | ``fleet run: `tiny` carries `workflows/takeoff.ts` and declares no [runtime] table, and no pack it imports declares one — core knows one thing about a workflow's language and that table is it, so there is no command to bundle this file with`` | Add `<checkout>//packs/ts`. |
| `fleet run` in a project with no `bd` store | 3 | `fleet run: the work graph could not be read: ...` | Run `bd init` in the project. |
| `fleet start` with no `fleet.toml` above the directory and no fleet named by the seat list | 1 | ``fleet start: no fleet.toml above this directory and no fleet named by <machine>/config.json — `fleet create` writes one`` | Run it inside the fleet's project, or `fleet create` first. |
| `fleet start` while the controller is running | 1 | `fleet start: the controller is already running as pid <pid>; its last tick was <stamp>` | Nothing to do, or `fleet stop` first. |
| `fleet start` cannot find `claude` | 3 | ``fleet start: no `claude` on the constructed child PATH (<path>) — nothing was loaded; the search path is <path>`` | Install `claude` into a directory on that path, or set `FLEET_CLAUDE_BIN`. |
| The service loaded and no `controller.started` arrived within 30 seconds | 3 | `fleet start: no controller.started was written within 30s — nothing fresh reached <machine>/events.jsonl; what the service printed is at <machine>/service.err.log` | Read the service's log. |
| The plugin's `bin/fleet` finds no binary | 127 | `fleet: no built binary under <checkout>/target — run cargo build --release in <checkout>, or set FLEET_BIN` | Build the checkout, or set `FLEET_BIN`. |
| `FLEET_BIN` is not an absolute path | 127 | ``fleet: FLEET_BIN names `<value>`, which is not an absolute path`` | Set it to an absolute path. |
| `FLEET_BIN` names no executable file | 127 | ``fleet: FLEET_BIN names `<value>`, which is not an executable file`` | Point it at the built binary. |

## See also

- [Packs](packs.md): what the tiny and ts packs carry, and how packs layer
  over the defaults.
- [The controller and seats](seats.md): adding seats, stopping the
  controller, and what it does once it runs.
- [Guards](guards.md): what the four guards the plugin runs refuse.
- [Runs and workflows](runs.md): `fleet run` and the takeoff workflow.
- [Status and the event stream](status.md): reading the controller once it
  is up.
- [Exit codes and conventions](conventions.md): the exit table every command
  shares.
