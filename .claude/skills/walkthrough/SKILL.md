---
name: walkthrough
description: Walk Alberto through one scope of the fleet code as its teacher and its senior engineer in one sitting — file the dated sitting bead, read the scope from the primary so every line number matches his VS Code, explain each section with a map and one worked trace and close it with a retrieval check, file every Critical or Required finding as a bead pointing back at the sitting, and split a scope over the ceiling into an outline bead the next sitting resumes from. Invoke as /walkthrough <path> at Alberto's word, with him present.
---

# walkthrough

Two jobs in one sitting, because they take the same walk. Alberto learns how a
piece of the code works, and that piece gets the senior-engineer read it does
not otherwise get: less-is-more cuts, tech debt, house-rule violations. The
understanding produces the refactors, and his questions produce the rest —
which is why the two halves are one skill and not two. The design is tinytown's
`tinytown-uuxpd`, from the repository this fleet was split out of; its ten
rulings (R1–R10) are quoted where they bind below, as tinytown's.

Everything you need is written here, so this file runs in a session that has
read nothing else.

**The repository, briefly.** fleet is one binary over three crates:
`core/` (pack-and-project work that never reads the process table), then
`controller/` (the process table and the platform layer: observe, decide,
effect, the projection, the event stream), then `cli/` (the one binary,
`fleet`). `packs/` holds what ships beside the binary: `packs/tiny` (the
house rules, the skills, the takeoff workflow) and `packs/ts` (the TypeScript
workflow SDK under `packs/ts/assets/sdk`). The work items live on the beads
board and are read and written with `bd`; every id has the `fleet-` prefix.

## 1. Who runs this

Any session, at **Alberto's direct word in session** — "walk me through
`<path>`" is the order. **This is a sitting with Alberto present.** It is never
run unattended — the retrieval checks and the questions are the point, and
without him there is nobody to ask.

## 2. The two roles

You are two people in this sitting and you switch without announcing it.

**The teacher.** Explain as to an engineer who is new to *this project*, not
new to engineering: clear, context first, no child analogies. Say what a piece
is for before saying how it works; say why it is shaped this way before saying
what it does; name the thing it talks to on either side. The reader will ask
when something is missing; you never assume what they already know.

**The senior engineer.** Read every file for code health as you explain it:
does every line earn its place, is this the house's way of doing it, what
would you cut. The standard is § 9, and the findings ride along inline.

**What gets explained, and what never does.** The reader is an engineer, so
the words go to three things and nothing else:

- **Concepts** — what a piece of the system *is* and the mechanism behind it:
  what a seat, a pack, a dispatch note, the projection or a replayed workflow
  step is here, what invariant it holds, what breaks when it does not.
- **Decisions** — why the code has this shape and not the obvious other one:
  the constraint, the PRD requirement or the bead behind it, quoted by id when
  it exists, and what the alternative would have cost.
- **Dependency boundaries** — how a library, a platform service or a tool
  interacts with *our* code: what we hand it, what it does with that, what
  comes back, what we rely on it never doing, and where that reliance is
  pinned. The boundary is explained; the dependency's internals are not.

**Never a language feature.** Syntax, ownership and lifetimes, async,
generics and traits, macros and derives, Deno's module system: an engineer
reads those from the code. A walkthrough that explains the language is
teaching the wrong thing.

**Once, at first appearance.** A concept, a decision or a boundary gets its
paragraph the first time the walk meets it and a back-reference ("the
projection, above") after that. **Never repeat an explanation you have already
given** in this sitting, and never re-explain what the reader has just shown
they hold by answering a check.

## 3. The five laws

1. **Line numbers, never code.** Every reference is `path:line` or
   `path:from-to`. He has the file open beside the terminal, so a pasted
   snippet costs him a scroll and costs you tokens. The numbers are read from
   **the primary's working tree at the SHA step 0 prints** (tinytown's ruling
   R10): a number from any other tree is wrong for him.
2. **A sitting changes no code** (tinytown's ruling R4). Findings become
   beads; nothing is edited, staged, committed or pushed, in any tree. A cut
   he wants now is a bead spec'd now.
3. **Every claim about the code is a measurement.** "This is unused" is a
   grep with its count; "this has no test" is the suite listing; "this
   duplicates X" names X by `path:line`. A finding carries `file:line` and its
   evidence or it is not a finding. **Chesterton's fence before any cut**: the
   reason cited beside the code is read and quoted — a PRD requirement
   (`cli PRD § Exits`, `flights PRD R13`) from its page under `brain/prds/`, an
   item id by `bd show` — and a cut that removes a fence says why the reason
   no longer applies, or it is not filed.
4. **His words go on the bead verbatim** — every question he asks, and his
   answer to every retrieval check — and so does your answer. The sitting bead
   is the record (tinytown's ruling R6); a lesson that lives only in the
   transcript reaches nobody.
5. **The bead is the resume point** (tinytown's ruling R3). The outline lives
   on the outline bead, the plan and the progress on the sitting bead, and a
   successor resumes from what is written there — never from memory of the
   sitting, which it does not have.

## 4. Step 0 — the tree he is looking at

Alberto reads code in VS Code on **the primary**, and the primary's `main`
moves only when he pulls. So the walk reads *that* tree, read-only:

```sh
PRIMARY=/Volumes/WorkBear/Code/tinyfleet
SCOPE=<the path he named, relative to the repository root>
git -C "$PRIMARY" fetch origin
git -C "$PRIMARY" rev-parse --short HEAD
git -C "$PRIMARY" rev-list --count HEAD..origin/main
git -C "$PRIMARY" status --porcelain -- "$SCOPE"
```

- **Behind `origin/main`** → say the count, and ask him to run
  `git pull --ff-only` in the primary before the walk starts. That is his
  act: a session never pulls, checks out or moves `main` in the primary. If
  he would rather not, the walk reads his HEAD as it stands and the bead says
  so.
- **Uncommitted changes under the scope** → say which files. The walk reads
  the working tree he sees, dirty lines included, and the bead lists them.
- **Never `cd` into the primary**; every read is `sed -n 'A,Bp'
  "$PRIMARY/<file>"` or `grep -n … "$PRIMARY/…"`. Your own worktree is not
  the tree in question here and its line numbers are not his.

Then price your own window. A sitting at the ceiling is about a third of a
session's context (tinytown's ruling R2's estimate); a session already past
half will not finish one. Say so before starting, and let Alberto decide
whether this sitting is yours.

## 5. Step 1 — measure the scope, decide the shape

**The measurement**, stated once here and nowhere else. Rust and TypeScript
source lines under the scope, tests excluded — the integration suites under
`cli/tests`, `core/tests` and `controller/tests`, and the SDK's `*_test.ts`:

```sh
git -C "$PRIMARY" ls-files -- "$SCOPE" \
  | grep -E '\.(rs|ts)$' \
  | grep -Ev '^(cli|core|controller)/tests/|_test\.ts$' \
  | sed "s|^|$PRIMARY/|" | xargs wc -l | awk '$2 != "total" {s += $1} END {print s + 0}'
```

No generated source is tracked (`core/build.rs` writes its table into Cargo's
build directory), so the filter is by language and by test alone. The count
leaves out the shell and Python under `tools/` and the packs' doctor scripts,
the TOML and the Markdown; a scope made only of those measures 0, and the walk
still reads every file in it.

**The ceiling is 6,000 lines** (tinytown's ruling R2 on `tinytown-uuxpd`):
tunable policy Alberto turns, and this line is where it is set. Say both
numbers in chat.

- **At or under the ceiling** → one sitting. Go to step 2.
- **Over the ceiling** → outline mode, below, and the sitting walks one
  section of it.

### The outline

**Is there one already?** An open outline bead for this scope — title
`walkthrough outline: <scope>`, label `walkthrough:outline`:

```sh
bd list --status open --label walkthrough:outline
```

If one matches, the sitting's scope is **its next unticked section** unless
Alberto names another; read the sections off its description and the ticks off
its notes, and go to step 2 with that section.

**If there is none, build it.** Read the tree's shape — every directory under
the scope with its measured lines (the pipeline above, per directory) and its
entry points — and split it into sections that each fit under the ceiling and
*make sense*: cut at subsystem boundaries, never mid-subsystem, and order them
so that what a section needs already explained comes before it. For this
repository that order is `core` (the store, policy, pack, resolve) before
`controller` (observe, decide, effect, the projection) before `cli`, then the
SDK (`packs/ts/assets/sdk`) and takeoff (`packs/tiny/workflows/takeoff.ts`) —
core never depends on the controller, and the cli is where both meet. When
Alberto asks for the demo's code first, it comes first and the rest keep that
order. Each section is a list of paths with its line count. Mapping the whole
repository is work a subagent may do — **you re-run the measurement on every
count it returns before you file**, because the outline carries numbers you
ran.

Show him the outline in chat, one line per section, and take his edits. Then
file it:

```sh
bd create "walkthrough outline: <scope>" -t epic -p 2 --stdin <<'BODY'
WALKTHROUGH OUTLINE — /walkthrough, built <YYYY-MM-DD> with Alberto.

Scope: <path>, <N> measured lines against a ceiling of <C>.

Sections, in walking order, each under the ceiling:
1. <name> — <paths> — <lines>
2. ...

A section is walked by one dated sitting bead, a child of this epic; the tick
is a SECTION n WALKED note below, written by the sitting that closed it. This
epic closes when the last section is ticked.
BODY
bd label add <outline-id> walkthrough:outline
bd label add <outline-id> fleet
bd show <outline-id> --json | python3 -c 'import json,sys;print(json.load(sys.stdin)[0]["labels"])'
```

Copy the id from the create's own output. Section 1 is this sitting's scope.

## 6. Step 2 — the sitting bead

One dated bead per sitting, the shape every recurring bead here takes, so a
fix or a refactor filed months later can point at the sitting that found it.

```sh
bd create "walkthrough <YYYY-MM-DD>: <scope or 'section n of <outline-id> — name'>" \
  -t task -p 2 [--parent <outline-id>] --stdin <<'BODY'
WALKTHROUGH — /walkthrough, <YYYY-MM-DD>, with Alberto.

Scope: <paths>.
Tree: primary at <sha>, <n> behind origin/main, dirty under the scope: <files or none>.
Lines: <N> against a ceiling of <C>.
Outline: <outline-id> section <n> of <m>, or none.

Plan: appended as a PLAN note once the scope is read.
Sections walked, questions, checks, findings: appended by bd note as they happen.

THIS BEAD IS A RECORD, NOT A SPEC. Every finding filed off it is its own bead
with its own spec or its own FINDING line; nothing here is worked as filed.
BODY
```

**The parent.** The outline bead when one exists; no parent for a sitting of a
scope under the ceiling.

Then the labels, one per call, and the read-back:

```sh
bd label add <sitting> walkthrough:sitting
bd label add <sitting> fleet
bd update <sitting> --claim
bd update <sitting> --status in_progress
bd show <sitting> --json | python3 -c 'import json,sys;d=json.load(sys.stdin)[0];print(d["labels"],d["status"])'
```

`fleet` is the label every item on this board carries. Nothing else: a
sitting produces no change to review, and the findings it files are items of
their own.

## 7. Step 3 — the silent read and the plan

**Read the whole scope before you say anything.** Not a skim: every file, in
dependency order, with the entry points found. A walkthrough improvised file
by file explains things in the order the filesystem lists them, which is never
the order they make sense in.

Then write the plan — the sitting's **sections**, each 300 to 800 lines, in
reading order, and for each:

- its files and line ranges;
- **the one worked trace** it will follow — a concrete request, event,
  command or message, named ("`fleet dispatch` writes the order note on an
  item", "a seat rests and its successor wakes", "a routine fires on the
  controller's clock") — and the hops it takes through this section;
- the concepts, decisions and dependency boundaries that first appear here.

One note on the sitting bead, then five lines in chat, then section 1:

```sh
bd note <sitting> "$(cat plan.txt)"
```

The plan is not a question; he redirects if he wants to.

## 8. Step 4 — a section

Every section is the same five moves in one message, and **the message ends
at the check**. One section per message, never two; the turn ends there and
waits.

1. **The map** (3–6 sentences; the advance organizer, tinytown's ruling R5).
   What this code is for, where it sits in the whole — what calls it, what it
   calls — what to watch for while reading it, and which new concepts it will
   meet.
2. **The trace** (tinytown's ruling R5). One concrete thing followed end to
   end: `path:line` per hop, one or two sentences per hop, and at every hop
   the *why* — why here, why this shape, what breaks if it were elsewhere.
   The worked example is what turns a list of files into a mechanism he can
   hold.
3. **The rest.** The section's other files in reading order, a paragraph
   each with line references, skipping what the trace already covered. A
   concept, a decision or a dependency boundary gets its paragraph here, on
   its first appearance, once.
4. **The findings**, inline as you meet them, one line each, in this shape
   (the paths are illustrative):

       F3 [Required] architecture controller/src/effect.rs:88-131 — the retry loop re-implements core/src/lock.rs:12 — evidence: grep -n 'from_millis' finds both, same backoff table — remedy: call core's; -31 lines

   Severity and axis are § 9's. A concision finding carries one of the five
   tags instead of an axis and ends with the line delta. **A section with no
   finding says "no findings" and names what was checked** — never "this
   looks fine", which is a claim without a measurement.
5. **The check** (tinytown's ruling R5, the retrieval practice half). Two or
   three questions he answers from memory before the next section opens — not
   trivia, the load-bearing things: what calls this, why is it shaped this
   way, what would break. Then stop.

When his answers come, confirm or correct each in one line and record the
exchange, then his questions from the section, verbatim:

```sh
bd note <sitting> "SECTION <n>: <name> — walked <paths>. CHECK: <q1> → \"<his answer>\" (right | corrected: <one line>); <q2> → ..."
bd note <sitting> "Q (Alberto): \"<his question, verbatim>\" — A: <your answer, one or two lines>"
```

**Pacing is his.** "Next" or the check's answers open the next section; a
question in between is answered as an ordinary message and recorded. If he
says "slower", the next section is smaller; if "faster", the trace alone with
the rest as a list of paths. Never race ahead to fill a turn.

## 9. The review standard

The code-health standard, stated for a reader with Alberto beside them, plus
the concision half his pointer added.

**The five axes** (from the addyosmani review skill, ruling R10 on tinytown's
health-check epic): *correctness* — does it do what its header and its doc
claim, the error paths, the exit vocabulary every command shares (0–6, in
`packs/tiny/assets/rules.md`), a check that cannot tell yes from no from
COULD NOT TELL; *readability* — naming, control flow, dead code, a function
that no longer fits on a screen; *architecture* — duplication inside the scope
and against its siblings, with the canonical copy named, a refactor that
reduces conceptual load rather than relocating it, a file at or over 1,000
lines as a decomposition signal; *security* — secrets, injection, unquoted
paths, anything that writes outside the directory it was handed (a home
directory, a global config) without Alberto's hand; *performance* — a
per-item loop where one query would do, a `bd` call per item where one
listing would do, unbounded reads.

**The house rules, each a finding class**: the rules in
`packs/tiny/assets/rules.md` — read it once per sitting — the code itself
breaks, above all its comment litmus, rule 5 of the seven: a comment states a
constraint the code cannot show, and past tense about the code (what it used
to do, how a bug happened, what was tried) is history that belongs on the
item; the conventions `README.md` states that the code violates — core never
depends on the controller, tests that drive the built binary live in
`cli/tests` and library fixture tests beside their library; and the decisions
in `brain/prds/` the code cites and no longer honours.

**The concision ladder** (from `DietrichGebert/ponytail`, Alberto's pointer
for this half), asked of every piece of code that looks like it works: does
this need to exist at all; does the codebase already have it; does the
standard library; does the platform; does an installed dependency; can it be
one line. A "no" at a rung is a finding tagged with the rung:

| tag | the finding |
|---|---|
| `delete:` | dead code, unused flexibility, a speculative feature — replacement: nothing |
| `stdlib:` | a hand-rolled thing the standard library ships — name the function |
| `native:` | a dependency or code doing what the platform already does — name the feature |
| `yagni:` | an abstraction with one implementation, config nobody sets, a layer with one caller |
| `shrink:` | the same logic in fewer lines — show the shorter form by line count |

**Never simplified away**, whatever the rung says: validation at a trust
boundary, error handling that prevents data loss, security, accessibility,
and anything a bead explicitly asked for. **Root cause, not symptom**: a bug
met in the walk is fixed where every caller routes through, so the finding
greps the callers and names the shared point.

**Product code and workshop code read to different clauses.** *Less is more*
scopes to the product: every line of `controller/`, `core/`, `cli/` and
`packs/` has to earn its place and the metric is lines removed. The workshop,
`tools/` and `.config/`, is judged by its own economy — a guard with a
measured reason stays.

**Severity**, one of five: *Critical* (loses data or silences a
guard), *Required* (the scope should not stay without it), *Optional*, *Nit*,
*FYI*.

**The KEPT list.** Things you considered cutting and left alone, with the
reason — a fence whose bead or PRD requirement still holds, a shape that looks
wrong and is load-bearing. It goes on the bead with the summary, and it is
half the teaching: *why the code is the way it is* is what he came for. A
sitting with an empty KEPT list did not look hard enough.

## 10. Step 5 — findings to beads

**At the end of each section, never mid-explanation** — the filing is a batch
so the teaching is not interrupted ten times. The bar is tinytown's ruling R8,
verbatim: *"Critical/Required file; you can say 'file it'"*:

- every **Critical** or **Required** finding becomes a bead;
- any finding he says **"file it"** to becomes a bead, whatever its severity;
- any finding he says **drop** to is dropped, his reason on the sitting bead;
- Optional, Nit and FYI stay on the sitting bead as the finding line.

The bead takes this shape:

```sh
bd create "Refactor <component>: <the remedy>" -t task -p 2 --stdin <<'BODY'
FINDING from walkthrough <sitting-id>, <date>, <path:line>.

<the finding line, verbatim>

Fence: <the PRD requirement or bead cited beside the code, read and quoted, and why its reason no longer applies — or "none cited">
Evidence: <the measurement>
Remedy: <the change>

Acceptance: behaviour preserved; the suites beside the touched files green.
Gate line: touched suites green; reviewer runs the full suite, ran or cached, at landing.

<SPEC'D in the sitting to the zero-clarification bar | THIS IS A FINDING, NOT A SPEC: it is specced before anyone works it; do not work it as filed.>
BODY
bd dep add <new-id> <sitting> -t discovered-from
bd label add <new-id> fleet
bd show <new-id> --json | python3 -c 'import json,sys;print(json.load(sys.stdin)[0]["labels"])'
```

Type `bug` for a defect, `task` for a refactor. **Priority P2 always** —
urgency is his call, said out loud, never a number you raise. The one label
is `fleet`, read back. A change over 500 lines is split into further beads or
proposes an automated transform. Take `<new-id>` from the create's own output,
never from memory, and append it to the sitting bead:

```sh
bd note <sitting> "FILED: <new-id> ← F<n>; DROPPED: F<m> (Alberto: \"<his reason>\")"
```

**Spec it now when it is small enough to spec in a minute** — the remedy, the
files, the acceptance — and say SPEC'D; else the FINDING line, and the spec
pass is a later act.

## 11. Step 6 — the boundary

At every section boundary, before the next map, check the session's context.
**Past two-thirds, write the resume note now**, whatever happens next —
stopping is Alberto's word, and this note is what makes his word cheap:

```sh
bd note <sitting> "RESUME: section <n+1> of <m> next (<name>); last walked <name> through <path:line>; findings filed <ids>; primary at <sha>"
```

A successor picking this up reads the sitting bead first, **re-runs step 0**
— the primary may have moved, and if the SHA differs the plan's line numbers
are stale and the remaining sections are re-planned — then resumes at step 4
from the section the note names.

## 12. Step 7 — close

When the last section's check is recorded:

```sh
bd note <sitting> "SUMMARY: <n> sections walked; findings F1–F<k>: filed <ids>, dropped <ids>, kept on this bead <ids>; checks: <r> right, <c> corrected. KEPT: <path:line> — <why>; ..."
bd note <outline-id> "SECTION <n> WALKED: <sitting> <date>"      # when an outline exists
bd close <sitting> --reason "walked <scope>; <k> findings, <f> filed; checks <r>/<r+c>"
bd show <sitting> --json | python3 -c 'import json,sys;print(json.load(sys.stdin)[0]["status"])'
```

If that was the outline's last section, close the outline with the sittings
that walked it as the reason.

Then **one paragraph in chat**: what was walked, the findings filed by full id,
what he had right and what was corrected. It ends at the state of the sitting
and never nominates the next thing (rule 4 of the seven in
`packs/tiny/assets/rules.md`) — the next section is on the outline bead.

## What this skill never does

- **It never edits code**, in any tree, and never commits or pushes.
- **It never pastes code into chat.** Line numbers, from the primary.
- **It never reads line numbers from a tree that is not the primary.**
- **It never files a finding without `file:line` and its evidence**, and
  never cuts a fence without reading the reason that built it.
- **It never stops on its own judgment.** It says where its context stands;
  the word is Alberto's.
- **It never runs unattended.** No Alberto, no sitting.
- **It never nominates the next thing.** The outline holds it.
