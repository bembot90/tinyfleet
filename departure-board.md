# Departure board

Everything that came out of walkthrough sitting `fleet-alt.1` (outline
`fleet-alt`, section 1: `core/src/store.rs`), 2026-09-23. The next flight is
the first `## Flight N` that still has an unticked row. Tick a row (`☑`) when
its item lands. The Blocked-by cell is a note written by hand; the item's own
dependencies are the real blockers.

## Flight 1 — store fixes, specced and independent

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-ah4 | Store helpers `answered`/`listed`/`created_id` over `std::process::Output` (F2+F8) | — | ☑ |
| fleet-fhi | `assigned_to` reads a seat's whole listing: `-n 0` (F12, Required) | — | ☑ |
| fleet-jqh | One `From<StoreError> for Stop` replacing eight mappings (F5) | — | ☑ |
| fleet-nv0 | `fleet brief` decides from the order index, not a notes search | — | ☑ |

## Flight 2 — store cleanup and the bd pin

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-9r6 | Read bd's JSON envelope now, before bd v2.0 makes it the default (F11, Required) | fleet-ah4 | ☑ |
| fleet-16s | Delete dead store fields: `created_at`, `flight`, `Ready` (F1) | lands after fleet-ah4 (same file) | ☑ |
| fleet-5y5 | `Item.blockers` counts only blocking dependency types (F9) | lands after fleet-ah4 (same file) | ☑ |
| fleet-016 | History out of comments, `Row` renamed `AssignedItem`, fix `open_labelled`'s doc (F3+F4+F6) | lands after fleet-16s (same lines) | ☑ |
| fleet-reb | Pin bd at 1.3.0, checked, and re-measure the store's quirks | Alberto upgrades bd (`brew upgrade beads`) | ☑ |
| fleet-ay8 | `fleet ask` on an epic leaves an edgeless gate; `ready` lets epics through | fleet-ah4 (`gate()`); with fleet-16s (`ready`) | ☑ |
| fleet-998 | `fleet dispatch <suffix>` refuses a ready item | fleet-16s (same line, `dispatch.rs:182`) | ☑ |
| fleet-pg7 | dispatch's hold check counts ordered items only (ruled) | fleet-fhi (`assigned_to`) | ☑ |
| fleet-bk115 | A test for the store opener's bd resolution (`cli/src/runs.rs:53`) | — | ☑ |

## Flight 3 — specced fixes, independent of the designs

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-45m | The SDK takes a returned delivery as carried (RETAKEN, `until(delivered)`) (Required) | — | ☑ |
| fleet-d11 | Delete the flight engine's leftovers: dead constants, the resume path, the rule matcher, seven census rows | — | ☑ |
| fleet-rzixz | One reader of the item store: the runner moves into core, bd calls bounded at 60s, one opener, prime's reader folded in | — | ☑ |
| fleet-xie | Generate the bd adapter's wire types from beads' OpenAPI spec at the pin (pin-stamp test) | fleet-reb (bd 1.3.0 installed) | ☑ |
| fleet-2jt | Declare, check and document the supported versions of bd and Claude Code, as Deno's already is | fleet-reb | ☑ |

## Flight 4 — the renames, namespacing and the archive

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-6gr | Gates become holds, clearances and checks; `fleet hold` / `fleet clear`; `[gates]` split; routines evaluate the trigger; split at pickup | — | ☑ |
| fleet-4j6 | Namespace fleet's keys (`fleet.orders`, `fleet.run`, label `fleet:run`), clean break; carries fleet-y3l's RERUN fix | — | ☑ |
| fleet-g68 | Archive cli, controller and packs PRDs, flights as superseded, bench stays live; restate every citation as its constraint | fleet-d11 | ☑ |

## Flight 5 — seat identity (fleet-iex, split in build order) and `fleet doctor`

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-iex.1 | Add core's seat identity: the uuid crate, SeatId, Kind, the [seats.<id>] reader, identity.toml, and the one resolver… | — | ☑ |
| fleet-iex.2 | fleet start reads [seats.<id>] through core's roster: agent seats render as <slug>-<short> rows carrying id and kind,… | fleet-iex.1 | ☑ |
| fleet-iex.3 | Add `fleet seat add --agent|--human`: it mints or lists a seat as one [seats.<id>] table in the fleet's own fleet.tom… | fleet-iex.1, fleet-iex.2 | ☐ |
| fleet-iex.4 | fleet create lists its creator as a human seat and stops printing the hand-written first-seat block | fleet-iex.3 | ☐ |
| fleet-iex.7 | A transient spawn mints a fresh seat id that is never reused; transient-N and its lowest-free counter go | fleet-iex.1, fleet-iex.2 | ☐ |
| fleet-iex.5 | Key the machine's seat list (config.json) by the seat id: rows carry id, kind and the seat's own name, and every read… | fleet-iex.2, fleet-iex.7 | ☐ |
| fleet-iex.6 | Key sessions.json, the stream's seat actor and the projection's seat by id, derive every name from <slug>-<short>, an… | fleet-iex.5 | ☐ |
| fleet-iex.11 | The work graph holds seat ids, part 1: dispatch and brief resolve --to to a seat id, the assignee and fleet.orders.se… | fleet-iex.6 | ☐ |
| fleet-iex.12 | The work graph holds seat ids, part 2: [core] reviewer resolves to a seat id, deliver reassigns to it, review returns… | fleet-iex.11 | ☐ |
| fleet-iex.8 | The actor is a typed reference at the cli: --by and FLEET_ACTOR take a seat or kind:id, BEADS_ACTOR is no longer read… | fleet-iex.12 | ☐ |
| fleet-iex.9 | Core's verbs take the typed actor: bd gets its <kind>:<id> string, and every seat-only check refuses a non-seat actor… | fleet-iex.8 | ☐ |
| fleet-iex.10 | The stream carries the actor as a typed reference {kind, id}; tail filters by --seat (resolved) and --actor, and the… | fleet-iex.9 | ☐ |
| fleet-iex.13 | A seat appears as {id, name?, kind} in the projection, the event payloads, every seat-bearing --json document and the… | fleet-iex.6, fleet-iex.7, fleet-iex.11 | ☐ |
| fleet-iex.14 | Rewrite docs/ for seat identity: ids and names, identity.toml, fleet seat add, <slug>-<short> names, the typed actor… | fleet-iex.4, fleet-iex.6, fleet-iex.7, fleet-iex.10, fleet-iex.12, fleet-iex.13 | ☐ |
| fleet-iex.15 | Rewrite the tiny pack's rituals and core's default assets for seat identity: homes under seats/<slug>-<short>/, the n… | fleet-iex.6, fleet-iex.7, fleet-iex.8, fleet-iex.11 | ☐ |
| fleet-2u3.15 | doctor core: one module finds the doctor checks through the layers and runs each one bounded; fleet run's runtime che… | — | ☑ |
| fleet-2u3.1 | fleet doctor: run every installed pack's and the defaults' doctor checks on demand | fleet-2u3.15 | ☑ |

## Flight 6 — the record (fleet-zlk, split verb by verb)

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-zlk.1 | the entry model — an item's record as typed timeline entries, their wire text and the fold, in core | fleet-iex.1 | ☐ |
| fleet-zlk.2 | the store appends and reads an item's timeline — bd comments in the adapter, the fake, the contract, and one append-a… | fleet-zlk.1 | ☐ |
| fleet-zlk.3 | a seat hands in JSON — the delivery, question and findings input types and their JSON Schemas in the defaults | fleet-zlk.1 | ☐ |
| fleet-zlk.4 | `fleet item show <id> [--json]` renders an item and its timeline, and the brief carries that rendering instead of bd's | fleet-zlk.1, fleet-zlk.2 | ☐ |
| fleet-zlk.5 | dispatch, its withdrawal and retire write ordered / order_withdrawn entries, not notes | fleet-iex.9, fleet-iex.11, fleet-zlk.2, fleet-zlk.4 | ☐ |
| fleet-zlk.6 | `fleet deliver --delivery <file.json>` — the seat hands in JSON validated before any write, and `--item` checks the h… | fleet-iex.9, fleet-zlk.3, fleet-zlk.4 | ☐ |
| fleet-zlk.7 | deliver writes the delivered entry, and review and land read the delivery off the timeline | fleet-zlk.2, fleet-zlk.6 | ☐ |
| fleet-zlk.8 | `fleet review --return <file.json>` — findings are JSON, and takeoff's return writes a findings file the verb accepts | fleet-zlk.3, fleet-zlk.4 | ☐ |
| fleet-zlk.9 | review writes the reviewed entry; review refuses a reviewer who is not the item's; land lands only its own reviewer's… | fleet-zlk.7, fleet-zlk.8 | ☐ |
| fleet-zlk.10 | `fleet hold --question <file.json>` — a seat's and a run's question is JSON, and the brief shows its schema | fleet-zlk.3, fleet-zlk.4, fleet-zlk.6 | ☐ |
| fleet-zlk.11 | hold, clear, cancel and the crash cap write held / cleared entries; clear reads the open hold off the timeline; a run… | fleet-zlk.2, fleet-zlk.8, fleet-zlk.10 | ☐ |
| fleet-zlk.12 | land writes the landed entry with full shas, and retire's branch release reads it | fleet-zlk.2, fleet-zlk.7, fleet-zlk.9, fleet-zlk.11 | ☐ |
| fleet-zlk.13 | fleet status counts open holds off every registered project's store, not the stream | fleet-zlk.11 | ☐ |
| fleet-zlk.14 | the SDK reads the store through `fleet item show --json` — until, spawn and hold — and a wake names kinds and a strea… | fleet-zlk.4, fleet-zlk.5, fleet-zlk.7, fleet-zlk.9, fleet-zlk.11, fleet-zlk.12 | ☐ |
| fleet-zlk.15 | item lines on the stream become wake signals {item, entry, kind}; the controller's crash-cap latch and the parked sta… | fleet-zlk.13, fleet-zlk.14 | ☐ |
| fleet-zlk.16 | docs and the record-reading skills say the timeline is the record; preboard reads fleet.orders and runs, never notes | fleet-zlk.15 | ☐ |
| fleet-zlk.17 | delete the marker grammar and the notes plumbing — fleet reads and writes no notes | fleet-zlk.16 | ☐ |

## Flight 7 — the store contract (fleet-0q4, split) and adopting a board

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-0q4.1 | Store contract types in core: ids, Status, Order, OrderState, Item, ItemSummary, Filter, Update, Capabilities and the… | fleet-iex.1, fleet-zlk.1 | ☐ |
| fleet-0q4.2 | docs/store.md: the store contract an adapter implements | fleet-0q4.1 | ☐ |
| fleet-0q4.3 | The bd adapter becomes its own module, and land reads the export file and the store's directory from the adapter | fleet-0q4.1 | ☐ |
| fleet-0q4.4 | Item speaks the contract's domain types: Status, OrderState, ItemId, RunRecord and a read proof in place of document | fleet-0q4.3, fleet-iex.12, fleet-zlk.17 | ☐ |
| fleet-0q4.5 | The store's reads become resolve and list(filter); ready, open_labelled and assigned_to go | fleet-0q4.4 | ☐ |
| fleet-0q4.6 | The store's plain writes become create(NewItem), update(Update), close and version, and StoreError::Missing becomes R… | fleet-0q4.5, fleet-iex.9 | ☐ |
| fleet-0q4.7 | Order, run and hold writes speak the contract, and their storage shapes move behind the bd adapter | fleet-0q4.6, fleet-zlk.2, fleet-zlk.17 | ☐ |
| fleet-0q4.8 | The conformance suite as a library, run against the fake and the bd adapter, and the bd adapter's scratch verb | fleet-0q4.7 | ☐ |
| fleet-0q4.9 | The external store client: one process per call, JSON on stdin and stdout, fleet's exit table, the 60s bound | fleet-0q4.2, fleet-0q4.7 | ☐ |
| fleet-0q4.10 | [store] adapter selects the store, one opener serves every caller, and bd's resolution moves behind the adapter | fleet-0q4.9 | ☐ |
| fleet-0q4.11 | fleet store check [--adapter <path>]: the conformance suite against an adapter on a scratch store it makes | fleet-0q4.10, fleet-0q4.8 | ☐ |
| fleet-0q4.12 | Repair lines print a fleet command, not a bd one, and core names bd only inside the adapter | fleet-0q4.7, fleet-zlk.4 | ☐ |
| fleet-0q4.13 | Routines file items through the store, fleet create reads the item prefix from the adapter, and no file outside the a… | fleet-0q4.10, fleet-0q4.12, fleet-iex.9 | ☐ |
| fleet-0ml | Adopt an existing board: a doctor check that reads the board through fleet, and a mapping skill | fleet-2u3.1, fleet-zlk.4 | ☐ |

## The walkthrough

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-alt.1 | Walkthrough section 1 continues past `store.rs`: re-run step 0 at the new SHA, re-plan, next file | — (flights 2–3 have landed) | ☐ |

## Later — unbuilt requirements from the archived PRDs

Epic fleet-2u3 holds one bead per requirement the archived cli, controller and
packs PRDs carried and nothing built (fleet-2u3.1 to .14: `fleet doctor`, run
rest grants, review size tiers, a pack registry, signed packs, a second agent
runtime, CI on macOS and Linux, per-platform measurements, SSE, the row-moves
journal, incidents, idle reclaim, attach, remote seats). None is specced, and
none is on a flight until someone boards it.

## Held for fleet-zlk

These are notes-as-state bugs that the typed timeline retires. They aren't
fixed in the prose grammar and aren't on a flight; each closes when fleet-zlk
is built, or is judged again there.

- fleet-4rl: deliver accepts a second column-zero marker, then fails its read-back after committing
- fleet-20s: preboard's filter reads an `On flight` note that nothing writes
- fleet-56e: landing records carry 7-character shas
- fleet-z4w: the SDK's gate trips the crash-cap park latch through `item.parked`
