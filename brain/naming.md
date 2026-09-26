# Naming

How fleet names things, and the list of names it has chosen. Read before
naming anything a user will see: a command, a config key, a file, an event, a
mode, a piece.

## The rule

**Use the plain, conventional name.** The word an experienced developer would
guess before reading the docs, the one their existing tools already use, the
one that needs no sentence of explanation. `project`, not `rig`. `service`,
not `porter`. `rest`, not `catnap`. `embedded` and `standalone`, not a pair of
aviation words.

**Analogies are a cost, kept to a minimum.** Every metaphor a user meets is a
term they must translate before they can use the tool, and a family of them
becomes a vocabulary lesson that stands between the person and their work.
Gas City's cities, rigs, mayors, polecats, convoys and wisps are its own
vocabulary and internally consistent; fleet chooses plain words so a command
reads without a glossary.

fleet is named fleet, and a few flying words come with that name. They are
allowed where they already carry the product's story and are used everywhere
without variation; they are not a license to coin more. **The test for a new
term:** if it needs explaining in the sentence that introduces it, use the
plain word instead.

## The chosen names

| Thing | Name | Why |
| --- | --- | --- |
| the product | **fleet** | the name; lowercase |
| a persistent agent identity with a worktree and a history | **seat** | plain word for a position someone occupies; in general use |
| the adapter a seat's agent is reached through, and the cli family that checks it | **agent adapter**, `fleet agent` | plain; mirrors the store's, and a seat is the worker while the agent is the program it runs — not the agents/ pack slot, which holds seat templates (Alberto's ruling 18, 2026-09-25) |
| a batch of work run without the person present | **flight** | the one analogy the product story rests on: "a night with fleet", "minutes to first flight"; used everywhere, never varied |
| the mode that runs flights unattended from the board | **autopilot** | conventional English for an automatic mode, not a coinage. *Removed from core 2026-09-17 (workflows-formula-fate): `fleet autopilot` and the switch file left with `plan` and `fly`; unattended composition is tiny's takeoff workflow over `fleet run`* |
| the binary's own bottom layer | **defaults** | plain; what every fleet runs on — the record templates, the guards' wiring, the health checks — with no opinion about the work. *Was the shipped pack **core** until the ruling of 2026-09-13 (core-folds-into-the-binary-after-the-rehearsal) landed: the files are embedded in the executable and materialized into the machine directory's `defaults/`, and `core` names no pack on any surface* |
| the doctrine pack | **tiny** | one short word for one pack; layers over the binary's defaults; the only place opinions live. The bundle — this pack plus the cli — is **tinyfleet**, and naming the pack after the bundle would have made the cli look like it only ran ours, which it does not: it runs anyone's packs |
| the daemon | **controller** | plain; what it does |
| the screen | **manager** | plain |
| the event stream | **log** | plain |
| the numbers | **stats** | plain |
| a registered repository the fleet works on | **project** | plain; replaces the borrowed `rig` |
| the fleet whose config lives inside the one project it runs | **embedded** | plain; the fleet is embedded in the project |
| the fleet whose config lives in its own repository and registers projects | **standalone** | plain; the fleet stands on its own |
| a scheduled or triggered duty | **routine** | renamed from `order` 2026-09-18 (fleet-layers Q3, ruled 2026-09-17): the word was overloaded with the dispatch record on an item, "orders given", which keeps it. `fleet routine list\|check\|run\|history`; the events are `routine.fired`, `routine.completed`, `routine.failed`, `routine.could_not_tell`. The file format is borrowed from Gas City and keeps its word — `orders/<name>.toml` carrying `[order]`, the state file, the projection's array and the events' `order` payload key — until that format is renamed. `fleet order` is refused with exit 2 and the pointer for one release |
| a folder of agents, routines, skills and workflows | **pack** | borrowed with its format; plain enough to keep |
| a seat asking to stop and be succeeded | **rest** | plain; the lifecycle events are `seat.woke`, `seat.resting`, `seat.handed_off`, `seat.exited` |
| making a transient seat: its worktree, its row and its first session | **spawn** | plain, and the word an operating system, a shell and Claude Code all use for starting a process |
| handing a live transient seat its next first turn | **feed** | plain; what is done to a seat that is waiting for work, and distinct from `nudge`, which carries a message to one that is not |
| a spawned seat's end | **retire** | plain |
| how much context a seat has left | **context** or **remaining context** | say the thing; the reference's `runway` is an analogy and is not carried over |
| waking a live seat with a message | **nudge** | plain, and the word Claude Code and Gas City both use |
| giving a ready item to a seat, delivering finished work, reading it, putting it on main | **dispatch**, **deliver**, **review**, **land** | plain verbs; core's four |
| the first thing a dispatched seat reads | **brief** | plain; what a person hands someone starting a job |
| a hook that refuses a class of mistake before it runs | **guard** | plain; the vision page's guardrail, one word |
| a pack file replacing the same path in a lower pack | **shadow** | borrowed from Gas City's layer rule; the word its docs use |
| the policy file | **fleet.toml** | the product name and the format |
| the project's declaration in standalone mode | **project.toml** | plain |
| the unit of work the verbs move | **item** | plain; survives a second store beneath it; the store's own name (bd today) is used only where the store itself is meant |
| one line on the log, and the cli family that writes and reads them | **event** | plain; `fleet event <verb>` is what a seat says, `fleet seat <verb>` is what is done to a seat |
| bringing the controller up and down | **start**, **stop** | plain; the service's own verbs, and the only two that touch it |
| writing down a list of items to fly later | **plan** | plain; a flight plan is what a person expects the word to mean. *Removed from core 2026-09-17 (workflows-formula-fate): `fleet plan` and `fleet fly` are tiny's preboard and takeoff workflows over `fleet run`* |
| the flights planned and not yet open | **backlog** | plain. *Removed from core with `plan` 2026-09-17 (workflows-formula-fate); `status` no longer lists one* |
| a seat's question that needs a person, and the person's reply | **ask**, **answer** | plain; `fleet ask` from the seat, `fleet answer` from the person; one pair, one object |
| the object a question, a cap, a red gate or a declared step becomes on an item | **gate** | the store's own word for it, and its own object; listed by the store, resolved by `answer` |
| an item set aside inside a flight with its branch, commit and question kept | **park** | plain; the state a gate puts an item in; never a command word |
| a dispatch the load belt refused, retried next tick | **held** | plain |
| the one-at-a-time queue a project's landings run through | **lane** | plain; one lane per project, a lock `land` takes |
| a project's rule for how landings reach the trunk | **trunk strategy**: `advance`, `batch`, `pull-request` | plain words for the three shapes; a `project.toml` key |
| a defect found after landing, traced through the work graph | **escape** | the testing vocabulary's word (defect escape); the referee for best code |
| the files a flight pins and writes | **flight directory**, **summary** | plain; `flights/<id>/` and `summary.json`. *Removed from core 2026-09-17 (workflows-formula-fate): what a run pins lives in `runs/<id>/`, the run lifecycle's own directory* |
| one unit of a workflow's run — a spawn, an until, a review, a land, a gate | **step** | plain; the SDK numbers them, and the run lifecycle writes `step.started` and `step.closed` per step |
| the ordered policy list that fills an item's flight defaults from its type and labels | **rules** | plain; `[[core.flight.rules]]`, first match wins |

## Names we are not carrying over

**`flight` is never a command word.** Ruled 2026-09-09: "fleet flight sounds
dumb to humans and should never be a command." The noun lives in prose and on
the record. The verbs were `plan`, `fly` and `autopilot`, and all three left
core on 2026-09-17 (workflows-formula-fate, fleet-layers.md § What moves):
composition is tiny's preboard and takeoff workflows over `fleet run`, and the
run lifecycle keeps what a flight pinned, locked, capped and resumed. With the
flight advance went its two event kinds `flight.halted` and
`flight.halt_cleared`; `fleet event clear-halt` takes a seat alone.

The reference fleet coined freely, and most of its coinages stay behind:
`porter`, `runway`, `catnap`, `wake` and `sleep` as ritual names, `courier`,
`desk`, `tower`, `hangar`, `boomerang`, `laurels`. Where a thing survives it
gets the plain name above; where a ritual survives it lives in the tiny pack
under whatever name that pack chooses for itself, because a pack is where
opinions are allowed.

The tiny pack has taken that licence, and every name it chose is a plain word,
so **none of them is a row above**: `wake`, `handoff`, `rest`, `clock-out`,
`morning`, `corrections-review`, `praise`, `report` and `runbook`. Three are
worth stating because the reference called them something else — a seat's day
ends in a **handoff** and not a `sleep`, it shortens its day with a **rest**
and not a `catnap`, and the second read of a landing is a
**corrections-review** and not a `boomerang`. `wake` is the one ritual name
carried over, because it is the event's own word rather than a coinage.

## How to add a name

1. Write the plain, conventional word first. Check what Claude Code, git, bd
   and systemd call the same thing; if one of them has a word, use it.
2. If you still want an analogy, write the sentence that would introduce it to
   a stranger. If the sentence has to explain the analogy, the plain word wins.
3. Add the row here in the same change that introduces the term, with the why.
   A name that is not in this table is not yet a name.
