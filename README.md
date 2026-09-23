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
The dependency that is allowed runs the other way, and the cli crate is where
both meet.

The root manifest lists its members explicitly and never by glob, so a crate
under this directory cannot be absorbed by a manifest that did not name it. That
is the whole of the guard, and it is why no member's own manifest carries an
empty `[workspace]` table.

## Working here

    make fleet-fmt     # cargo fmt --all --check at the workspace root
    make fleet-check   # clippy over every target and the boundary checks; no arm runs
    make fleet-test    # fleet-check, then cargo nextest run --workspace, then the budget verdict
    make fleet-ring    # the ring_ binaries, serial, outside the budget and judged by nothing

`fleet-test` is the gate a landing runs. Its test phase is held to the budget in
the surrounding project's own policy file, judged only when the box's 1-minute
load before the run is under that file's ceiling — over it the run prints NOT
JUDGED and exits 0, because contention can only lengthen a run and so neither
side of the budget says anything about the code. The ring lane is a separate
population by binary name and no ring second is inside the budgeted number.

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
`target/debug/fleet` under this directory. An installed copy carries no
`target/`, so `$FLEET_BIN` is the seam the install sets.

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

    fleet pack add <checkout>//fleet/packs/tiny --version <tag, branch or sha:…>

`fleet prime` then names the installed packs on its first line, top first —
`none installed` until one is added, and never the defaults, which are the
binary's — and prints the resolved rules file beneath it.

### Driving it from a checkout

    cargo build                       # in this directory, so target/debug/fleet exists
    claude --plugin-dir <checkout>/fleet

`--plugin-dir` loads the plugin for that session only and shadows an installed
plugin of the same name. Four probes say it worked:

1. the session's `SessionStart` hook output carries `fleet prime`'s first line;
2. a `Bash` call on `git show "$S:tools/land"` is denied, and the reason names
   `fleet guard shell-trap` and prints the rewrite;
3. a `Bash` call on `fleet --version` answers this crate's version — the bare
   name resolved to `bin/`, which nothing outside the session resolves;
4. `claude -p '/fleet:version'` answers with the version, which is the skill
   namespace.

`claude plugin validate <checkout>/fleet` checks the manifests.

A project INSTALLS the plugin instead, with
`claude plugin marketplace add <path to this directory>` followed by
`claude plugin install fleet@fleet --scope project`. That is a decision about a
project's settings and is not run from here.
