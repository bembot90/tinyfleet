# fleet

One binary, three crates.

## Layout

| Crate | Package | Import path | What it is |
| --- | --- | --- | --- |
| `controller/` | `fleet-controller` | `fleet_controller` | The process table and the platform layer: observe, the projection, the event stream. A library. |
| `core/` | `fleet-core` | `fleet_core` | Pack-and-project work: the verbs that never read the process table. A library. |
| `cli/` | `fleet-cli` | — | The one binary, named `fleet`. Its `[[bin]]` is the only one in the workspace, so the package name carries the `-cli` suffix and no crate shares a name with the binary a user installs. |

**Core never depends on the controller.** The boundary between pack-and-project
work and the process table is kept in the type system rather than in discipline:
`cli/tests/workspace.rs` reads `cargo metadata --no-deps` and refuses the edge.
Core depends on neither of the other members. The dependency that is allowed
runs the other way: the controller depends on core, for the bounded runner
(`fleet_core::process`) and nothing else, and the cli crate is where both meet.

The root manifest lists its members explicitly and never by glob, so a crate
under this directory cannot be absorbed by a manifest that did not name it. That
is the whole of the guard, and it is why no member's own manifest carries an
empty `[workspace]` table.

## Working here

    cargo fmt --all --check                                # formatting, at the workspace root
    cargo clippy --workspace --all-targets -- -D warnings  # every target, warnings as errors; no arm runs
    cargo nextest run --workspace                          # every suite but the ring lane
    cargo nextest run --profile ring                       # the ring_ binaries, serial, judged by nothing

There is no wrapper around these and no time budget: the four commands are the
whole of it, and the third is the suite a landing runs. The ring lane — the
`ring_*` binaries, which spawn the shipped binary against real repositories — is
left out of the default profile by binary name in `.config/nextest.toml` and
runs only under its own profile. That file's per-arm `slow-timeout` is the one
bound on a run's time.

A rig's scratch board runs on bd's embedded engine unless
`FLEET_TEST_DOLT_PORT` names a served one; `tools/dolt-test-server <command>`
runs `<command>` against a throwaway server it starts and stops.
`tools/suite-profile` ranks where a run's seconds went, read from the junit
report the default profile writes.

Tests that drive the built binary live in the crate that builds it — `cli/tests`
— because `CARGO_BIN_EXE_fleet` is defined only for the package whose manifest
declares that binary. Library-level fixture tests live beside the library they
exercise.

## Flying unattended

Core ships **no routine that composes or flies** (flights PRD R26).
Composition is a workflow's — tiny's `preboard` writes a flight's list and its
`takeoff` opens and flies it, both over core's `fleet run`
(`brain/fleet-layers.md` § What moves). A fleet with nobody composing flights
is one file a person writes into their fleet's `orders/` directory, in the
controller's own routine format:

```toml
[order]
description = "fly a nightly flight of the ten oldest ready items"
trigger = "cron"
schedule = "0 2 * * *"

[action.run]
workflow = "takeoff"

[action.run.inputs]
ready = "10"
```

Three lines say when and the rest say what: firing it calls `fleet run takeoff
--input ready=10 --by <routine>` in the routine's project root, so
`run.started` names the routine as its actor and the run's `started` and
`closed` sit inside the routine's `fired` and `completed` on the one stream.
The input names are the workflow's own. `controller/tests/routines.rs` parses
this exact block out of this file through the routine loader, so the example
cannot drift from the format it claims to be in.

The autopilot switch is a machine setting the takeoff routine reads — to be
filed after the MVP — not a verb: on, the routine opens the oldest plan while the open runs are under the
cap; off, nothing opens (`brain/fleet-layers.md` § tiny). The three verbs
`fleet plan`, `fleet fly` and `fleet autopilot on|off` were core's until
2026-09-17 (workflows-formula-fate).

## The plugin shape

This directory is also a Claude Code plugin root: `.claude-plugin/plugin.json`
is the manifest, `.claude-plugin/marketplace.json` publishes this directory as a
one-plugin marketplace, `hooks/hooks.json` wires the guards and `fleet prime`
into a session, `bin/fleet` is the shim those hooks address, and `skills/` holds
the plugin's skills. The hooks reach the shim through `${CLAUDE_PLUGIN_ROOT}`
because a hook's `PATH` does not carry `bin/` and the Bash tool's does
(`brain/lessons/claude-code.md` D5).

`bin/fleet` resolves the real binary by explicit path: `$FLEET_BIN` when it
names an absolute executable, else `target/release/fleet`, else
`target/debug/fleet` under this directory — never a bare `fleet` on `PATH`,
which a service environment does not carry. The controller sets `$FLEET_BIN`
to its own binary on every session it spawns, so a seat's hooks run the binary
that spawned it. A session with no binary to find — started from a checkout
nobody built, or from an installed copy, which carries no `target/` and which
nothing yet points at a binary — fails closed: each guard hook exits 2, which
blocks the Bash command, and says what is missing and how to supply it, while
the session-start hook says the same and lets the session come up.

### The rituals a session gets

`skills/version` is the plugin's own probe. Every other entry under `skills/`
is a **symbolic link** into `packs/tiny/skills/`, so the pack keeps the only
copy of its own opinion and the loader still finds a `SKILL.md` at the path it
reads. The loader follows such a link — measured, with its controls, at
`brain/lessons/claude-code.md` D6 — which is why nothing here is a pinned
duplicate.

The rituals, each resolving under the `fleet:` namespace: **wake**, a seat
coming up for a session; **handoff**, the seat's day ending; **rest**, a
mid-day handoff that asks the controller for a woken successor; **clock-out**,
a spawned seat's last acts; **morning**, the first read of the open gates;
**corrections-review**, a landing judged a second time for whether every line
earns its place; **praise**, a laurel written into another seat's file;
**preboard**, the next flight's list composed off the departure board;
**takeoff**, a flight's two human phases, pre-flight and the reading of its
report; and **report** and **runbook**, the two published-page house styles,
each carrying its own `template.html`.

### The defaults and the packs

`core/defaults/` is what every fleet gets: the record templates, the guards'
wiring and their health checks, with no opinion about the work. It is not a
pack — `core/build.rs` walks it into the executable, and `fleet create` and
every `fleet start` materialize it into the machine directory's `defaults/`,
a sibling of `packs/`, pinned by content hash. Every verb reads those files
by path unless an installed pack shadows the path. `packs/tiny` is the
opinion — the role documents, the builder manual, the values and the
every-turn rules — and it shadows
`assets/rules.md` to add its own. The tiny pack and this binary together are
the bundle **tinyfleet**; the binary on its own stays usable with anyone's
packs, which is why the pack is not named after the bundle.

Tiny installs from a checkout with the subdirectory form, which takes the
repository and the path to the pack inside it:

    fleet pack add <checkout>//packs/tiny --version <tag, branch or sha:…>

Tiny imports `ts`, the pack that pins Deno as the workflow runtime, and the
same checkout holds it at `packs/ts`, so that one line installs both: ts comes
out of the same clone, pinned at the same commit and keyed in `packs.lock` on
`<checkout>//packs/ts`. An import that lives in another repository is not
fetched. `pack add` names it on stderr with the `fleet pack add` line that
installs it, and still exits 0. Until it is added, `fleet prime`'s first line
and `fleet run`'s refusal for a workflow with no runtime both name it with
that same line.

`fleet prime` then names the installed packs on its first line, top first —
`none installed` until one is added, and never the defaults, which are the
binary's — and prints the resolved rules file beneath it.

A pack's settings are set in `fleet.toml`, under the pack's name, and only the
keys the pack declares. A pack declares each one in its `pack.toml` with a
one-line description, and optionally a type and a default:

```toml
# pack.toml
[config."takeoff.test"]
description = "The command takeoff hands fleet land to run on the rebased tree before each landing."
type = "string"

# fleet.toml
[packs.tiny]
takeoff.test = "cargo nextest run --workspace"
```

`fleet run` refuses a key the installed pack does not declare, a value of the
wrong type, and a section for a pack that is not installed, naming the key and
the pack. A workflow reads a setting with `run.config("takeoff.test")`: the value
set here, else the declared default, else `undefined`, as pinned when the run
was opened. tiny declares `takeoff.test` and `takeoff.touched`, and `fleet pack
check` refuses a malformed declaration by name.

### Where the test commands live

A test command is the workflow's, not the project's. There is no `[gates]
suite` and no `[gates] touched`: a `fleet.toml` or `.fleet/project.toml` that
still sets either is refused by name — by `land`, `dispatch`, `brief`, `seat
spawn` and `run` — with the setting that replaces it. The `[gates]` table
itself is refused the same way, naming where its keys live now: the landing's
marker is `[landing] ci_marker`, a seat's command words are `[permissions]
tool_commands`, and the guard targets (`release_ref_glob` and the `prod_*`
lists) sit under `[guards.targets]`.

- **The landing's suite** is what `fleet land <item> <commit> --test
  <command>` is handed. The landing runs it on the rebased land branch, under
  the lane's lock and before the push, so what is tested is what lands; a red
  reading is rerun once and a second red refuses with nothing pushed. Without
  `--test` the landing runs nothing, and says so: its note's first line and its
  suite row read `NOT TESTED`, and `item.landed` carries `test: null`.
- **The builder's gate** is what `fleet dispatch <item> --touched <command>`
  is handed; the brief names it where the seat reads its gate, or names the
  absence where none was handed.
- **takeoff** hands both: `test` from `--input test=<command>`, else
  `takeoff.test` under `[packs.tiny]`; `touched` the same way from
  `--input touched=` or `takeoff.touched`. The input wins. With neither test
  command the flight still flies, and its report's first line says
  `NOT TESTED`.

```toml
# fleet.toml
[packs.tiny]
takeoff.test = "cargo nextest run --workspace"
takeoff.touched = "make test-touched"
```

### Driving it from a checkout

    cargo build                       # in this directory, so target/debug/fleet exists
    claude --plugin-dir <checkout>

`--plugin-dir` loads the plugin for that session only and shadows an installed
plugin of the same name. Four probes say it worked:

1. the session's `SessionStart` hook output carries `fleet prime`'s first line;
2. a `Bash` call on `git show "$S:tools/land"` is denied, and the reason names
   `fleet guard shell-trap` and prints the rewrite;
3. a `Bash` call on `fleet --version` answers this crate's version — the bare
   name resolved to `bin/`, which nothing outside the session resolves;
4. `claude -p '/fleet:version'` answers with the version, which is the skill
   namespace.

`claude plugin validate <checkout>` checks the manifests.

A project INSTALLS the plugin instead, with
`claude plugin marketplace add <path to this directory>` followed by
`claude plugin install fleet@fleet --scope project`. That is a decision about a
project's settings and is not run from here.
