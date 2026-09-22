---
name: takeoff
description: Fly a list of items with the person absent from the middle — pre-flight while they are present, then `fleet run takeoff` carries the middle as the pack's workflow, then the report is read with them when their answers come back. The architect operates it.
---

# takeoff

A normal session runs at the speed of the person's presence. **A flight is
the mode where the middle is their absence, by design:** they are present at
the start to make the run well-specified, present at the end to rule on what
was decided in their name, and gone in between.

The middle is not a skill any more. It is `workflows/takeoff.ts` in this
pack, run by `fleet run takeoff`: a spawn step per item, an until step per
delivery, review and land as steps, every verdict a gate for the person
under `review=gate`, and the report and the board tick as its last two steps.
A step that cannot close exits waiting and the controller re-runs the bundle
when the stream moves; replay carries the re-run back to the same step, so
nothing is spawned twice and no delivery is reviewed twice. This file holds
the two phases that stay human: **pre-flight** and **reading the report**.

## Who this is for

**The architect, operating.** Review is chartered architect work and the
middle is dominated by it, so there is no dispatcher seat. A builder in a
run just builds, and its questions reach the person through the run's gates.
If you are not the architect and you reached this file, stop and say so.

## Hard rules

- **What never auto-resolves, in any mode:** supervised-lane sign-off on the
  running thing, release refs, and anything the charters reserve to the
  person. The workflow asks; it never answers for them.
- **The review bar does not move.** The verdict at a gate is read to the
  normal standard: the delivery note, the diff at its commit, the suite it
  claims.
- **A decision with no clear recommendation is a gate, not a guess.** The
  workflow parks on it and the run waits; a coin-flip dressed as a
  recommendation is a false record and worse than an interruption.
- **The item trail comes first.** Every act of the run is on the stream and
  on the items before or as it happens; a message is a doorbell.
- **One session, one seat, one day** holds, along with worktrees, squash
  landings and an actor on every write.

## Phase 1 — Pre-flight, with the person present

The start-loaded half: an underspecified item is a question scheduled to
interrupt a run that has no way to be interrupted.

a. **Take the list.** The preboard skill composes it from the departure
   board and prints the two `--input` pairs; the person may hand you ids
   directly instead. Never infer a list from the ready pool. **If the person
   is not actually reachable right now, do not launch:** this phase spends
   their attention once so the middle never needs it.

b. **Read every item on it** — `bd show <id>`: acceptance criteria that exist
   and are checkable, a `## Proof` naming each criterion's command and exit
   code, dependencies satisfied or themselves on this list, a `lane:` label,
   no unresolved decision marker, and **no hold** on the item or its parent
   epic. The three `WORTH` lines and the readiness lines are the judge's,
   answered on the item, and the judge is never the spec's author. A "no"
   drops the item from the list with the reason on it.

c. **Supervised-lane items are accepted only with an explicit flag** that
   they run to the review gate and stop there: the reel, then the gate for
   the person on the running thing, never a landing in their absence.

d. **Resolve every gap now, while they are here** — a question with its
   options and a marked recommendation — or drop the item with their ok. A
   gap carried into the middle is a gate the run will park on for hours.

e. **State the list back and wait for go.** The final list in order, every
   item you dropped and which check dropped it, the policy pair as it will
   be pinned. Only then:

   ```sh
   fleet run takeoff --input items=<id>,<id> --input policy=review=gate,width=<n>
   ```

   The run prints its id and its hash; the run directory under the machine
   directory holds `inputs.toml`, the policy in force and the bundle, and
   **the pinned inputs are the list that flies**. Read them back. Nothing
   else is yours to do until the run closes or parks: `fleet status` shows
   it, and a park is a gate the morning skill reads.

## Phase 5 — The report, then the walk-through

The end-loaded half. The workflow's last two steps wrote `report.md` and
`board-tick.md` into the run directory, and `run.closed` is on the stream.

a. **Read the report** — the decisions the person answered at gates, each
   tagged `<run id>-D<n>`, the flight table with every item's outcome and
   landed sha, and the numbers. Post one line in chat: the run id, the local
   time from `date`, and where the report is. No table, no decision list —
   the page holds them.

b. **Apply the tick.** `board-tick.md` names the landed rows; tick each one
   on the departure board (`☑`) and write the run id in the flight heading,
   and let that edit ride a landing's `--also` so the board is never a commit
   behind the item it closes. A workflow writes its run directory and nothing
   else, which is why the tick is a file for your hand and not an edit.

c. **The walk-through happens when their reading comes back**, whenever that
   is. A gate's letter was already the ruling and needs nothing more; a
   verdict they now want reversed is transcribed verbatim onto the item and
   filed as a follow-up item, because a reversed decision is the mechanism
   working as designed.

d. **The report ends there.** No next batch, no nomination of what to fly
   next; the person holds their own list and asks. A run ending is not a
   session ending: do not hand off, offer one, or mention one because a run
   finished.
