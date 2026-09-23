---
name: preboard
description: Compose the next flight's list from the departure board, in board order and through the readiness filters, and print it as the `--input` pairs `fleet run takeoff` pins — prose for a person or an architect, never code.
---

# preboard

A flight is a run of the takeoff workflow, and the workflow flies whatever
list it is pinned. This is where the list comes from: the departure board,
read top to bottom, each row through the same filters a hand has always
applied, and the survivors printed as the `--input` pairs the run takes,
beside the test command its landings will run.
It composes and prints; it starts nothing. The takeoff skill's pre-flight is
where the person hears the list back and says go.

## The board is the list; nothing else joins it

The departure board is the file the fleet's own file names under
`autopilot.board`, holding `## Flight N` headings from 1 up, each a table of
rows, one item per row, with a Landed cell reading `☐` or `☑`. **The next
flight is the first `## Flight N` from 1 up holding an unticked row whose
item has not closed.** A flight whose unticked rows have all closed has
landed and owes its ticks: name them (`LANDED, UNTICKED`) and walk past.
**The board keeps precedence and loses its veto:** a board flight with one
dispatchable row is still the whole list, and no item off the board joins it
or tops it up to the flight size; a board flight with **no** dispatchable row
falls back to the ready pool — every ready item through the same filters,
ordered by how many items each unblocks, then priority, then age — with the
board block and its reasons kept above the list as the cause of it. The
board's own rows never re-enter by that route: they failed the same filters
the pool pass runs, so they fail them again.

## The filters, in order, one verdict per row

Every unticked row of that flight, in board order. The first verdict that
applies is the row's, and every excluded row prints with its verdict and
reason so a dropped row is visible in the list of what will not fly:

1. **No id in the Bead cell** — not dispatchable.
2. **Closed** — `LANDED, UNTICKED`; the row owes a tick, and that is all.
3. **Open but not ready** — `bd ready --json -n 0` does not list it: `WAITS
   ON` its open blockers, read from the item's own dependency edges and
   never from the Blocked-by cell, which is prose a hand keeps. `-n 0` is
   load-bearing: the default answers a hundred rows and a row past that is
   invisible.
4. **An epic** — never fed; its children are rows of their own.
5. **Already in flight** — an `orders` key on the item, an `On flight` note
   with no `LEFT FLIGHT` after it, or an open run whose record names it:
   `ASSIGNED`. Boarding it would feed work someone is carrying.
6. **Held** — a `HOLD` line on the item or on its parent epic, in the title,
   the description or the notes, with no later line lifting it: held. A hold
   lives wherever the person wrote it, never in a title prefix.
7. **The person's own** — assigned to them, or carrying no `lane:` label, or
   a `kind:decide` label: `NEEDS THE PERSON`, because that is a decision they
   owe and not a seat at work.
8. **Not on an allowed surface** — a surface label outside
   `autopilot.allowed_surfaces`: excluded, by the ruling that put the list
   there.
9. **The spec pass** — acceptance criteria that exist and are checkable, a
   `## Proof` naming each criterion's command and exit code, and the three
   `WORTH` lines and the readiness lines on the item. A row missing them is
   `SPEC PASS OWED`; the judge is never the spec's author.

The survivors, in board order, are the flight; the first
`autopilot.flight_size` at the current `economy.level` fly and the rest print
under `NEXT FLIGHTS` so the whole queue is published on every composition.
Two survivors on one flight that edit one file are a spine: keep the board's
order, which is the person's hand, and never sort.

## What it prints

The board block first — every row with its verdict — then the flight, then
the queue, then the three pairs on their own lines, ready to paste:

```
--input items=<id>,<id>,<id>
--input policy=review=gate,width=<n>
--input test=<command>
```

`items` is the flight in board order, comma-separated. `policy` is the
workflow's own grammar: `review=gate` asks the person for every verdict at
a gate and `review=accept` lands every delivery unasked — `gate` unless the
person says otherwise, because every decision that would have gone to them
is theirs to answer; `width` is how many items fly at once, the roster's
width on the box and never more than `dispatch.max_busy_spawned`.

`test` is the command every landing runs on its rebased tree before the
push. Print the one `takeoff.test` sets under `[packs.tiny]` in the fleet's
`fleet.toml`, or the one the person names — theirs wins, the way the run's
input wins over the file. **Where neither names one, print that instead of
the pair, in capitals:** `NOT TESTED — no test command; every landing in this
flight will run nothing`. A flight may fly untested; it may not fly untested
without the person having read that line before they said go. `touched`, the
command each builder runs over its own diff, rides the same way
(`--input touched=<command>`, else `takeoff.touched`) and prints only when
one is named.

The run is then one line, and it is the takeoff skill's pre-flight that
runs it, after the person has heard the list back:

```
fleet run takeoff --input items=… --input policy=… --input test=…
```

## Two measurements minutes apart differ

The board is live: a row lands or is re-filed between a preview and the run
that follows it. **The list that flies is the one the run pins**, read back
from its `inputs.toml` in the run directory; a list printed earlier is a
preview. Say when they differ rather than flying the earlier one.
