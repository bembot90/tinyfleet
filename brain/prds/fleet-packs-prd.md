# PRD: fleet-packs

**One sentence:** everything with an opinion is a pack, one pack ships, and
that pack — **core** — moves work with four verbs, runs a flight of them and
the autopilot that pulls the next, and refuses the mistakes every seat makes,
without an opinion about the work itself; a pack on top of it, such as
**tiny**, adds the opinions by shadowing core's files and adding its own.

Drafted 2026-09-05, from the vision page (`fleet-vision.md`), the controller
PRD (`fleet-controller-prd.md`), the naming doc (`naming.md`), the Gas City
pack format as it stands in that project's bundled core pack and its build
packs, and the **reference fleet** — the tools that have moved every work item
of a live product through a fleet of coding-agent seats since August 2026.
Every requirement says whether it distills the reference (**R**), was measured
against Gas City (**T**), or is new with fleet (**N**). The reference's own
record is kept with the reference and cited from the work items, never from
this page; the substrate facts it rests on are in `lessons/`.

---

## Problem statement

The controller keeps seats alive. It does not know what a work item is, how one
reaches a seat, what a finished one looks like, or what makes it safe to put on
`main`. Something has to, and the whole value of an unattended night is that
thing: **a fleet that produces landed commits instead of open branches.**

Two products already answer it, each in its own shape. Gas City answers with a
workflow engine: formulas compile into graphs of steps routed to agent roles,
gated by artifact schemas, looped until a verdict. It is powerful, and it is a
week's learning rather than an afternoon's. The reference fleet answers with
doctrine: two hundred tools, twenty-five living documents, forty-five rule
lines, every one bought by an incident. It works, for the people who wrote it.
fleet's aim is a pack a person reads in an afternoon.

fleet's answer, ruled on the vision page and refined in the sitting this PRD
records, is neither. **Primitives, not features.** The mechanics of moving
work are four verbs a seat runs, an order can fire, and a person can read in
one sitting. The opinions about how to do it well live in a pack you can leave
out. And the mistakes that cost the reference its incidents are refused before
they run, for everyone, by default.

## Goals

1. **Three linked steps, then two words.** Install the cli; `fleet create`
   inside the project asks embedded or standalone and which agent, and writes
   the config; its done message names the command that creates the first seat
   — an architect, on the provider chosen, to help finish the setup; that
   command's done message says the controller is not running and names
   `fleet start`. After that a person needs two words to have a working fleet:
   what a **flight** is and what **autopilot** is. Onboarding never mentions an
   order; those are found on day four, when someone wants a cron job (ruled
   2026-09-05).
2. **Four verbs, one record.** dispatch, deliver, review, land — each a pack
   tool with a written contract, each leaving its note on the work item so a
   successor reads the item and never a message. Above them, **flight** runs a
   batch of ready items through the four to landed, and **autopilot** is the
   switch that pulls the next flight when one lands.
3. **Guards on by default.** The classes of mistake every fleet makes — a
   shell trap that reads as green, a record silently rewritten — are refused
   before they run, in every fleet, unless a person turns one off. The
   classes only some fleets make — a release ref pushed, production
   infrastructure written — are a pack's guards, not core's.
4. **A pack is a folder in a format that already exists.** Gas City's: agents,
   skills, orders, formulas, doctor checks, a per-provider overlay, a manifest
   that imports other packs. No format of fleet's own.
5. **Extensible by shadowing, never by forking.** A pack on top of core
   replaces any file core publishes as its shadow surface, adds its own agents,
   skills, orders and guards, and sets every key core reads. The surface is a
   registry with a test, not a convention.
6. **Opinions in one place, and one bar for core.** core carries no opinion
   about the work, and every piece of it passes one test: *will everyone need
   this?* A side project of fifty commits and one web app is the user to ask
   it of. What fails the test is a pack's — tiny carries ours, and a
   user who wants neither writes a third.
7. **Beads the one dependency.** The work graph is bd's; every note a verb
   writes is a bd note; multi-step rituals are bd formulas; nothing here
   invents a second store.

## Non-goals

- **Gas City's `graph.v2` compiler.** No drains, scopes, teardown or
  output-driven fan-out in core, and no promise that a city's packs run here
  or the reverse (ruled 2026-09-05). The format is shared because it is good
  and known — and since 2026-09-09 a v1-format formula in the slot is an
  item's life inside a flight, run by core's own step runner (the flights
  page, S6); the non-goal that stood here, "a workflow engine", was withdrawn
  there as a ruling that "wasnt meant to be set in stone".
- **The manager.** The verbs write the record; the manager reads it.
- **Composing a flight by theme, the report page, the departure board.** A
  flight in core is the next ready items up to a cap; how the reference
  composes, prices and reports one is tiny's.
- **Any order out of the box.** core ships none; the controller's order format
  is there for whoever writes one, and nothing in onboarding mentions it.
- **A second agent vendor's overlay.** The overlay slot is per-provider by
  format; only Claude Code's ships first.
- **Seat rituals.** Wake, rest, hand-off: the controller's lifecycle events
  (`seat.woke`, `seat.resting`, `seat.handed_off`, `seat.exited`) exist for any
  pack to emit; core emits the two it must and prescribes no ritual.

## Recorded directions

Rulings this PRD is built on, each with its date:

- **Everything with an opinion is a pack, in Gas City's format.** Ruled on the
  vision page, 2026-09-04.
- **fleet forces nothing at this layer and ships one pack, core.** Ruled
  2026-09-05: one pack installed by `fleet create`, named core, holding the
  mechanics; tiny is our own pack, imports core, and is not shipped.
- **Core folds into the binary and ships no pack at all.** Ruled 2026-09-13
  (core-folds-into-the-binary-after-the-rehearsal) and landed after the
  rehearsal: the ruling above stands as what was built until the rehearsal
  passed, and after it core is not a pack. Its files are embedded in
  the executable and materialized into the machine directory's `defaults/` — a
  sibling of `packs/`, never inside it — on `fleet create` and on every `fleet
  start`, pinned by content hash. The resolver appends that directory as the
  bottom layer whatever is installed, `fleet create` installs no pack, an empty
  packs directory is no longer a refusal, and nobody types, imports or reads
  `core` as a pack name on any surface. Where a sentence below says core is the
  pack every fleet gets, read the defaults.
- **Verbs and orders, not an engine.** Ruled 2026-09-05: dispatch, deliver,
  review and land as pack tools, orders to fire them, bd formulas for rituals;
  no graph.v2 runner.
- **The guards ride the pack's per-provider overlay, on by default.** Ruled
  2026-09-05; measured that Gas City's core pack ships provider hook files in
  that slot, so no extension to the format is needed.
- **No interop promise.** Ruled 2026-09-05; the vision page's sentence removed.
- **The controller runs seats; it does not run flights.** The controller PRD's
  boundary, 2026-09-05: it starts, watches, rests and retires seats, and fires
  orders on its tick; what a seat does with a work item is a pack's.
- **Work is given, never taken.** The reference's oldest rule, kept on the
  vision page: a seat begins on an order written on the item, never on its own
  reading of the board.

## Design overview

### What a pack is

A pack is a folder with a manifest and up to eight slots — Gas City's seven,
verbatim, because the format is theirs, and `workflows/`, which is this
format's own:

| Slot | Holds | core ships |
| --- | --- | --- |
| `pack.toml` | name, version, schema, imports of other packs by source and version, always-on agents, the optional `[runtime]` table below, and the `[config.<key>]` settings a fleet may set for the pack | its own manifest, no imports, no runtime, no settings |
| `agents/<name>/` | an agent definition and its prompt template | `architect` (the first seat: reviews, lands, helps finish setup), `builder` (transient) |
| `skills/<name>/SKILL.md` | what a seat can be asked to do | the verbs' skills, `brief` |
| `orders/<name>.toml` | routines — scheduled and triggered duties, the controller's format | none |
| `formulas/` | bd formulas: poured as molecules for multi-step rituals, and an item's life inside a flight run by core's step runner (the flights page, S6) | the default item formula: dispatch, build, deliver, review, land |
| `doctor/<name>/` | a health check the doctor command runs | `verbs-on-path`, `guards-installed`, `stale-branches` |
| `overlay/per-provider/<agent>/` | files dropped into the agent's config space | `claude/` — the guards as hooks, and a `claude/permissions.json` that sets the provider's own allow/deny rules |
| `assets/` | scripts and templates the above read | the brief template, the verbs' note templates |
| `workflows/<name>.<ext>` | the programs `fleet run` resolves by name, overlay first; the pack's `[runtime]` table says what language they are in | none — the defaults carry no runtime |

**What a transient seat may never do:** core's `claude/permissions.json`
denies five trunk-push shapes — `git push origin HEAD:main*`,
`git push origin main*`, `git push --force*`, `git push -f *`,
`git push * --delete*` — the same spellings `tools/spawn-builder`'s
`SPAWN_DENY` denies its own builders. The refusal is the provider's own
permission denial, not a hook, so `fleet land` stays the only path onto the
trunk (`core-deny-trunk-push`, 2026-09-17).

**What every transient seat may do, and what its project adds:** core's
`claude/permissions.json` carries the plain read verbs — `ls`, `cat`, `head`,
`tail`, `wc`, `grep`, `rg`, `sed`, `awk`, `cut`, `sort`, `uniq`, `tr`, `find`,
`mkdir`, `cd`, `pwd`, `which`, `command`, `test`, `date`, `env`, `dirname`,
`basename`, `xargs`, `diff`, `cmp`, `true`, `false` — one `Bash(<verb>:*)` rule
each and no wildcard over `Bash`, because every seat on any project needs them
and a project that forgets to declare them is a seat that cannot `ls`. Not
`rm`, `mv`, `cp`, `chmod`, `curl`, `ssh`, `scp`, `sudo` or `kill`: a seat
deletes and moves through `git` and the fleet's own verbs, and a list that
granted those would be a posture no pack ever saw. The **toolchain** is the
project's, not the pack's, so it is a list the project declares —
`[gates] tool_commands`, an array of command words, each a bare name or a
repository-relative path — and the spawn renders one `Bash(<word>:*)` rule per
entry after the pack's own list, deduplicated against it. An entry carrying a
space, a glob character or a leading dash is refused at the render, naming the
entry, so the list cannot smuggle a wildcard into a posture (a pack reads a
list; measured at the rehearsal, where four transient seats spent 11, 14, 8 and
6 refused calls between them on verbs and toolchains no rule reached).

A pack is resolved by **layer**: the fleet's own pack sits on top, then its
imports in declaration order, then core. The layering is the imports', never
the installs': `fleet pack add` places a pack an installed pack imports beneath
that importer, so a fleet that took tiny before ts existed adds ts afterwards.
A file at the same relative path in a higher layer **shadows** the lower one.
An agent name must be unique across
every installed pack; a collision is refused at install, never resolved by
precedence (T: the rule Gas City adopted after its own experience composing).

### The runtime table

A pack that carries workflows declares **one optional table** in its manifest,
and it is the only thing core knows about a workflow's language:

```toml
[runtime]
name    = "deno"
version = "2.4.5"
bundle  = "deno bundle {entry} --output {bundle}"
run     = "deno run --allow-run={fleet} --allow-read={run_dir} --allow-write={run_dir} --allow-env=FLEET_DIR,FLEET_RUN_ID,FLEET_STREAM,FLEET_STREAM_SEQ,FLEET_RUN_DIR,FLEET_BIN,FLEET_PROJECT {bundle}"
```

All four keys are required of a table that is present at all, and a fifth key
refuses the manifest by name: `version` is what the pack's doctor check measures
the installed binary against, and a bundle line without a run line is half a
contract. A pack that carries no workflows declares no table and is not
refused for it, which is the side the doctrine pack is on.

`bundle` and `run` are **templates over exactly five placeholders**, single
brace: `{entry}` the workflow file resolved through the layers, `{bundle}` the
one-file program written into the run directory, `{run_dir}` the run directory
itself, `{fleet}` the fleet binary a workflow calls back into, and `{inputs}`
the pinned inputs. Core substitutes them and execs the two lines; it parses no
TypeScript and reads no runtime's output beyond its exit. **A sixth name refuses
the manifest when it is read, not when the run fails** — an exec that dies on an
unsubstituted argument costs a run directory and a record item to discover, and
the same defect costs a line at `pack check`.

The version pin earns a **doctor entry**: a check under the pack's `doctor/`
slot whose `run.sh` asks the pinned binary for its version and reads red when
the answer is another version or there is no binary at all. Core ships the shape
(`doctor/runtime-version/`) against no table of its own; the feature-layer pack
that pins a runtime ships the instance by shadowing it.

### A pack's settings

A pack exposes settings through `fleet.toml`, and **the pack declares every
one** (Alberto's ruling, 2026-09-23: a test command is not project policy, it
belongs to the workflow that runs it). The declaration is a table per key in
the manifest:

```toml
[config."takeoff.test"]
description = "The command takeoff hands fleet land to run on the rebased tree before each landing."
type = "string"          # optional: string, integer, float, boolean or array
# default = "…"          # optional, and of the declared type where one is named
```

`description` is required and nothing else is: a setting nobody can read the
purpose of is one a person sets by guessing. A dotted name may be written
quoted, as above, or as a table path — `[config.takeoff.test]` — and both are
the one setting `takeoff.test`. A fourth key, a type outside the five, a
default of another type, a name part that is not letters, digits, `_` or `-`,
and a name that is the start of another (`takeoff` beside `takeoff.test`) each
refuse the manifest by name at `pack check`, the way a malformed `[runtime]`
does.

A person sets them under the pack's own name:

```toml
[packs.tiny]
takeoff.test = "cargo nextest run --workspace"
takeoff.touched = "cargo nextest run -p fleet-core"
```

**The declaration is the census.** `fleet run` judges the whole `[packs]` table
against every installed pack before it writes anything: a key the pack does not
declare, a value of another type than the declared one, and a section for a
pack that is not installed each refuse the run naming the key and the pack —
with the keys the pack does declare, or the packs that are installed — for the
reason core's own policy reader answers an unlisted key with an error: a value
nothing reads is a setting a person believes is in force and is not.

A workflow reads one with `run.config("takeoff.test")` beside `run.input()`: the
value `fleet.toml` sets, else the declared default, else `undefined`. It reads
the settings of the pack that CARRIES the workflow. They are resolved once, when
the run is opened, and **pinned under `config` in the run directory's
`inputs.toml`** — the file the hash already covers and the document the
workflow is handed on stdin — so a re-run reads the values the run was opened
with and never what `fleet.toml` says since.

### The four verbs

Each verb is one executable under the pack's `bin/`, on every seat's `PATH`
through the adapter, with one skill beside it that says when a seat runs it and
one note template that says what it writes. **The record is the work item.** A
verb that cannot write its note has not happened, and every verb reads the
item back after writing (R: the reference lost fifteen notes in one night to a
write that printed the same checkmark either way).

**`fleet dispatch <item> [--seat <name>]`** — gives a ready item to a seat.
With a named seat, the item is assigned to it, the order is written on the item
(`dispatched by <who> — orders given`, plus the machine-read index the
controller's projection carries), and the seat is rung. Without one, the
controller's `spawn` starts a fresh transient seat in a worktree at the
project's `origin/main`, the brief is its first turn, and the same order is
written. Refuses when the item is not ready, is already assigned, or the seat
already holds an item (R: the reference's spawn, feed and load belt; the
controller's R30–R31 are the primitives it calls).

**`fleet brief <item>`** — renders the first thing a dispatched seat reads:
the item verbatim, the contract (deliver to a work branch, record the commit,
reassign, ring; never land), the guards' names, the project's suite command,
and the pack's rule lines. Rendered whole or not at all (R: a partial brief is
a builder who cannot tell it read half).

**`fleet deliver`** — the builder's handoff. Commits the work branch, writes
the delivery note on the item — the commit, a `spec corrections:` count, a
`decisions:` block or `none`, what was not proven — reassigns the item to the
reviewer named in policy, and rings them. Refuses on the trunk, on an unclean
tree beyond the delivery's own files, or on a note that names no commit
(R: committed handoffs; the delivery note's three machine-read lines). The
unclean-tree wording is the cli PRD's, which is the newer text.

**`fleet review <item> <commit>`** — the reviewer's read. Measures the
delivery (paths, executable change, tests changed, returns) and prints a size
line on the item; runs the review the pack defines at that size — core's is
one reviewer seat reading the diff against the item and running the project's
suite; walks the delivery's `decisions:` block; writes the verdict on the item:
`ACCEPTED` or `RETURNED WITH FINDINGS` with `findings: <N>` as its first line
and each finding numbered. A return reassigns the item to the builder and rings
them; nothing lands from a return (R: the sizer, the verdict grammar, the
decisions walk).

**`fleet land <item> <commit>`** — the reviewer's landing, never the builder's.
Fetches, refuses unless `origin/main` is current, squashes the reviewed commit
onto it as one commit referencing the item, runs the project's suite from
`project.toml` on the land branch, exports the board into the commit, pushes,
reads the push's own range line before writing anything, writes the landing
note with the gate table, classifies the work branch and deletes it only when
safe, and closes the item. Exit codes are the contract: landed, refused, could
not tell, conflict, trunk moved, suite red (R: the reference's landing tool,
whose every gate is an incident it prevents).

### What every verb refuses to guess

Four rules shared by all four, each a lesson the reference paid for:

1. **The commit, never the branch.** review and land take a commit; a branch
   tip that moved after review is excluded, not swept in.
2. **Status read directly.** Every gate reads its command's own exit; nothing
   is read through a pipe.
3. **The record before the act's end.** No verb reports done until the note it
   wrote is read back from the item.
4. **Absence is graceful.** A ring that finds no live session sends nothing and
   moves on; the reassignment already recorded the handoff.

### Flight and autopilot

**Removed 2026-09-17, ruling `workflows-formula-fate`** (fleet-layers.md Q10,
§ What moves). The three verbs below left core: composition is tiny's preboard
and takeoff workflows over `fleet run`, and the run lifecycle keeps what `fly`
pinned — the directory, the hash on the record, the cap, the lock, resume and
retire-all. The flights page (`fleet-flights-prd.md`) holds the rulings; the
text below is kept as the record of what this page said.

**`fleet plan <items...>`** — writes a planned flight: the record item with
its list, nothing pinned. `--ready N` takes the N oldest ready items at plan
time. Core plans nothing on its own; the departure board that composes flights
from an opinion about the week is a pack's (R: the reference's board, kept out
of core).

**`fleet fly [<flight>] [--seats M]`** — takes off with the named flight or
the oldest plan: pins every input into the flight directory and the record,
refuses on any it cannot read, writes `flight.opened`, and returns; the
controller's tick advances the flight from there — dispatch, retire, review,
land, return, park, close — reading the record every tick and holding nothing
in memory. With no controller running, `fly` runs the same loop in the
foreground until the flight closes. The verb is `fly`, the noun is `flight`,
and there is no `fleet flight` command (R: the reference's manifest and its
operator, reduced to a record and a tick).

**`fleet autopilot on|off`** — the switch. On, the tick opens the oldest plan
while open flights are under `[core.flight] max_open`, default one; off,
nothing opens; an empty backlog opens nothing. The state is one file in the
machine directory, and `fleet status` prints it above the roster with the
plans waiting (R: the reference's switch, without its composition).

### Routines as the trigger, for whoever writes one

core ships no routines. The controller's format (cron, condition, action) is
there, and a routine's action calls a verb by name or fires a workflow —
`[action.run]` naming tiny's `takeoff` nightly — so scheduling is one small
file a person writes on the day they want it. A pack on top may ship routines.

### The guards

Two guards ship in core as Claude Code hooks under
`overlay/per-provider/claude/`, one class each, every one a parsing hook on the
command text that prints its rewrite when it refuses (R: a refusal a seat
cannot act on is one it routes around). Two pass core's bar — *will everyone
need this?* — because every seat has a shell and every fleet has a record:

| Guard | Refuses | Escape |
| --- | --- | --- |
| **shell-trap** | the four shell traps that read as green: an unsplit variable, a colon modifier after an unbraced variable, a status read through a pipe, a backtick in a stored record string | a per-command prefix |
| **record** | a work-graph write that replaces an append-only field, or a write statement through the graph's SQL route, or a bare item id in free text | a per-command prefix for the first two; the full id for the third |

Two more are the reference's and fail the bar — a fifty-commit side project
has no release refs and no production cloud — so they ship in **tiny**'s
overlay, on the same rules, reading their targets from that pack's own keys
(ruled 2026-09-05):

| Guard | Refuses | Escape |
| --- | --- | --- |
| **release-ref** | a push whose parsed target matches the pack's `release_ref_glob` | none at this layer; a git hook beneath it holds the override |
| **production-write** | a command that mutates a target the pack lists | a per-command prefix, by design |

Two rules the guards share: **a guard never emits allow** (an explicit allow
short-circuits the agent's whole permission system; allowing is silence), and
**a guard delivers its refusal as data on stdout with exit 0**, because a crash
that signalled by exit code would fail open. A guard whose targets are absent
from `project.toml` refuses nothing and says so in the doctor check.

**Opt-out, never opt-in.** `fleet.toml`'s `[guards]` table lists each
installed guard, core's and any pack's, with `enabled = true`; a person sets
one false; a pack cannot remove one. `fleet create` writes the table with
every guard on and says so in one line.

### Policy core reads

`fleet.toml`:

```toml
[core]
reviewer = "reviewer"          # the seat deliver reassigns to
max_returns = 3                # a third return parks the item for a person

[core.flight]
max_items = 6
max_seats = 3

[guards]
shell-trap = { enabled = true }
record = { enabled = true }
# a pack's guards add their own lines here when it is installed
```

`project.toml` (standalone) or the same keys at the project root (embedded):

```toml
[project]
name = "…"
item_prefix = "…"               # the work graph's id prefix
worktrees = "../…-worktrees"
primary = "/path/to/checkout"

[gates]
suite = "make test"             # what land runs; absent means land runs none and says so
touched = "make test-touched"   # the BUILDER's gate, over its own diff; absent means the
                                # brief tells a seat to derive it and never hands over `suite`
ci_marker = ""                  # a tool that prints the skip marker, or absent
```

The two gate keys are two gates, not one. `suite` is the reviewer's, run once
at the landing; `touched` is what a dispatched seat runs before it delivers.
The MAPPING from a changed path to a suite stays in the project — a runner that
already computes its own blast radius needs a command named here and nothing
else — so this key is a command and never a table.

Keys a pack's guards read — a release-ref glob, a list of production targets —
are that pack's, declared in its own table, never core's.

A pack's own settings sit in the same file under `[packs.<name>]`, each one a key
the pack's manifest declares (§ A pack's settings above).

### The shadow surface

core publishes `assets/shadow-registry.toml`: every file a pack on top may
replace, with one line saying what it is for. At filing: the brief template,
each verb's note template, each verb's skill, each guard's hook file, each
order. A doctor check reads the registry and refuses a pack whose shadowing
file names a path that is not in it (T: Gas City's build pack keeps such a
registry with a test that every listed path exists; the rule carries over
whole). Anything not in the registry is core's implementation, and a pack that
needs it changed files an issue against core rather than a copy.

### tiny, as the worked example

tiny imports core and touches it only through the surface above: it
shadows `review`'s skill to run its panel of lenses instead of one reviewer's
read, shadows the every-turn rules file to carry its doctrine — which is where
the brief's rules placeholder and every session's first line both read from, so
the brief template itself stays core's — adds its agents (the roles), its
skills (the rituals), its orders (the night's flights, the intakes, the health
check), its formulas (the rituals as molecules), and its own two guards,
release-ref and production-write. It changes no verb. If it
cannot do something through the surface, the surface is what changes.

## Requirements

**R** = distilled from the reference; **T** = measured against Gas City;
**N** = new with fleet.

### P0 — the first gate: one item, dispatched to landed, by core alone

**The format and the layers**

1. A pack is a folder in Gas City's format with the nine names above; the
   manifest's `schema` is theirs; `fleet pack check` validates one. T.
2. Packs resolve by layer — fleet's own, its imports in order, core — and a
   file in a higher layer shadows the same path below. Agent names are unique
   across installed packs; a collision refuses the install. T.
3. `fleet create` installs core and no other pack; `fleet pack add <source>`
   adds one by git source and version, pinned in a lock file. N; T for the
   lock.

**The verbs**

4. `fleet dispatch` assigns, writes the order note and its index, rings; or
   calls the controller's spawn with the brief as the first turn; refuses on
   not-ready, already-assigned, or a seat holding an item. R.
5. `fleet brief` renders whole or not at all, from the pack's template. R.
6. `fleet deliver` commits, writes the delivery note with its three
   machine-read lines, reassigns, rings; refuses on the trunk, on an unclean
   tree beyond the delivery's own files, or on a note with no commit. R.
7. `fleet review` prints the size line, runs the pack's review at that size,
   walks the decisions block, writes `ACCEPTED` or `RETURNED WITH FINDINGS`
   with a numbered count, returns to the builder on findings. R.
8. `fleet land` is the reviewer's, takes a commit, gates on a current trunk,
   squashes, runs the project's suite, exports the board, pushes, reads the
   range line, classifies and deletes the branch only when safe, closes the
   item, and exits by the contract. R.
9. Every verb reads its note back before exiting 0; a note that did not land
   is the verb's failure. R.
10. Every gate reads its command's own exit and never a pipeline's. R.

**Orders**

11. **Removed 2026-09-17 (workflows-formula-fate)** — composition is tiny's
    preboard and takeoff workflows over `fleet run`; kept as the record:
    `fleet plan` writes a flight's list; `fleet fly` pins its inputs and
    opens it; the controller's tick advances every open flight from its record
    and closes it when every item is landed or parked; `fleet autopilot
    on|off` is the switch the tick reads to open the oldest plan. The flights
    page holds the rulings and the requirements. R, reduced; N for the tick.
12. core ships no orders; an order's action, when a person or a pack writes
    one, calls a verb by name and never re-implements one. N.

**The guards**

13. Two guard classes — shell-trap and record — ship in core under
    `overlay/per-provider/claude/`, on by default, opt-out per guard in
    `fleet.toml [guards]`; a pack's guards install into the same table on the
    same rules. R for the classes; T for the slot; N for the split by core's
    bar.
14. A guard never emits allow and delivers refusals as data with exit 0. R.
15. A guard prints the rewrite when it refuses. R.
16. A guard whose targets are not configured refuses nothing and reports that
    in the doctor check. N.

**The surface**

17. `assets/shadow-registry.toml` lists every file a pack may shadow; a doctor
    check refuses a shadowing file outside it; a test asserts every listed
    path exists. T.
18. Every key the verbs read is in `[core]`, `[core.flight]`, `[guards]` or
    `project.toml [gates]`; a verb that reads a key not listed is a defect. N.
    Onboarding writes them all with `fleet create` and asks about none of them
    beyond the mode and the agent. N.

**The record**

19. Every note a verb writes has a grammar a reader can grep: the order form,
    the delivery's three lines, the verdict's first line, the landing's gate
    table. The grammars are in `assets/` as the templates the verbs render. R.
20. A verb writes to the work graph through `bd` only; no verb reaches the
    store another way. R.

### P1 — after the first gate

- `fleet review` size tiers with a per-tier review a pack defines, so
  tiny's panel plugs in by tier rather than by shadowing the whole skill.
- A `courier` verb — ring a seat from outside any session — as core's, since
  orders need it; until then the controller's nudge carries it.
- A second provider's overlay, proving the slot.
- Doctor checks for every verb's preconditions, run by `fleet doctor`.

### P2 — designed for, not built

- A pack registry and `fleet pack search`.
- Signed packs.

## The first slice — the first build

**"One item, dispatched to landed, by core alone."** In an embedded fleet
with one project, `fleet dispatch` on a ready item starts a transient seat with
the brief; the seat builds and runs `fleet deliver`; a reviewer seat runs
`fleet review` and, on accept, `fleet land`; the item is closed with all four
notes on it, the branch deleted, one commit on `main`, and every guard
installed in the seats' overlay. The four verbs, the brief, the record
templates, the two guards and the registry with its test are the slice;
`fleet fly`, `fleet autopilot` and `fleet pack add` were slice 2; the first two
left core 2026-09-17 (workflows-formula-fate).

## Success metrics

| Metric | The reference today | Target at the first gate |
| --- | --- | --- |
| items landed per night without a person | five to seven on a flight | the same, from core alone |
| deliveries returned for a guard-class mistake | a rule line each, still re-firing | zero: refused before the delivery |
| files a pack on top had to fork | tiny's skills fork nothing yet — not measured | zero outside the registry |
| minutes from `fleet create` to the first landed item | — | under sixty, with a person answering two questions |

## Decisions — open, for the sitting

**Q1 — core's review: one reviewer's read, or the panel?** *Ruled 2026-09-05:*
the reader — one reviewer seat, the diff against the item, the project's suite,
the decisions walk. The panel's lenses are tiny's opinion and plug in at
P1 by tier. Declined: the panel in core (six agents per landing for every
user); no review in core (land on the builder's word).

**Q2 — does `land` require a suite?** *Ruled 2026-09-05:* it runs the one
`project.toml` names, and a project that names none lands on the review alone
with `suite: none` in the landing note. Declined: refusing to land without a
suite (most side projects have none on day one).

**Q3 — how a guard is opted out.** *Ruled 2026-09-05:* `[guards]` in
`fleet.toml`, one line per installed guard, written on by `fleet create`; a
pack cannot remove one. Declined: a pack overlay that deletes the hook; a
`project.toml` key. *Ruled the same day, on the classes:* release-ref and
production-write are tiny's, not core's — "way too prescriptive for
core; the bar for core should be: will everyone need this?"


**Q4 — the controller's spawn, feed and retire against core's dispatch.**
The controller PRD keeps R30–R32 as controller verbs that write the order note
and hand the brief. *Ruled 2026-09-05:* the controller keeps the primitives — a
transient row, a worktree, a first turn, the load belt, the retire — and the
order note, the brief's content and the assignment move to `dispatch`, which
calls them; the controller PRD's three requirements are edited to say so.
Declined: leaving both layers writing the order (two writers of one record).

**Q5 — the nightly flight order.** *Ruled 2026-09-05, verbatim:* "this
shouldnt exist out of the box at all. people will be able to schedule these if
they want but its not something we should promote during the oob experience."
core ships no orders; flight and autopilot are verbs a person runs, and the
out-of-the-box path is three linked steps — create, the first seat, start —
followed by two words. Declined: shipped disabled with a question; enabled by
default.
