# Departure board

Flights 1–7 (the walkthrough's findings, seat identity, the record, the store
contract and adopting a board) landed on `release/0.1.0` on 2026-09-23 to
2026-09-25 and were cleared from this board on 2026-09-25; their record is on
the beads and in `git log`. Flights 8–10 are the store side of the adapters
design (`fleet-notes/adapters-and-sessions.md`, brainstormed and ruled
2026-09-25): epics `fleet-wpf0`, `fleet-rsia` and `fleet-3krx`, whose
descriptions carry the rulings and the global constraints every row builds
under. The next flight is the first `## Flight N` that still has an unticked
row. Tick a row (`☑`) when its item lands. The Blocked-by cell is a note
written by hand; the item's own dependencies are the real blockers.

## Flight 8 — the store contract complete for an external adapter, the suite hermetic, a bare adapter name resolved through packs (`fleet-wpf0`)

| Landed | Bead | What | Blocked by |
| --- | --- | --- | --- |
| ☑ | fleet-run | A non-seat-id assignee reads Unreadable at exit 3, and store::Item.assignee becomes Option<SeatId> | — (first: every seam sits on it) |
| ☐ | fleet-wpf0.1 | The contract's fences: update takes if_assignee and status open, order.withdraw takes if_assignee, if_status and reopen, moved joins the refusals, show carries description (folds fleet-j7q) | fleet-run |
| ☐ | fleet-pjl | The contract's actor is one string everywhere: the timeline entry's wire shape is pinned, and close takes an actor | — |
| ☐ | fleet-b47 | Adapter and doc agree on missing writes, the empty update and NewItem::validate | — |
| ☐ | fleet-urp | The contract's Item and list rows carry the store's foreign keys, and the adopt-board check reads class 2 off them | fleet-run |
| ☐ | fleet-2tp | ItemSummary carries the assignee and the run record, so a listing needs no show per row | fleet-run |
| ☐ | fleet-xpc | Capabilities declares the CLI the guard polices and the item vocabulary routines check | — |
| ☐ | fleet-z5m | The store contract's fragment rule is a prefix rule, as bd 1.3.0 resolves | — |
| ☑ | fleet-vqre | Two OrderKind types exist: entry.rs's {Dispatch} and the contract's {Dispatch, Review} | — |
| ☐ | fleet-4qpq | fleet store check names [store] adapter for a bad --adapter and prints nothing for ~50 s | — |
| ☐ | fleet-wpf0.4 | The trait slims onto the fences: hand_over, reopen and order_withdraw_from go, order_withdraw takes a WithdrawFence | fleet-wpf0.1 |
| ☐ | fleet-wpf0.5 | fleet store schema prints the contract as JSON Schema generated from the Rust types, committed with a drift test | fleet-wpf0.1, fleet-pjl |
| ☐ | fleet-wpf0.6 | The fake store decodes the contract's own JSON and nothing of bd's | fleet-run, fleet-urp |
| ☐ | fleet-wpf0.7 | fleet-store-stub: a test-only adapter executable answering the contract from the fake, its state in a file | fleet-wpf0.4, fleet-wpf0.6 |
| ☐ | fleet-wpf0.8 | core's verb suites run on the stub through Exec | fleet-wpf0.7 |
| ☐ | fleet-wpf0.9 | cli's suites run on the stub through Exec; store_check keeps one bd arm | fleet-wpf0.7 |
| ☑ | fleet-wpf0.2 | adapters is the eighth pack slot, and a bare name in [store] adapter resolves through the installed packs | — |
| ☑ | fleet-wpf0.3 | fleet prime's second line reads the store's version verb through the opener | — |

## Flight 9 — the bd store adapter as a TypeScript pack in fleet-packs (`fleet-rsia`)

Builds in the fleet-packs repository; the beads stay here.

| Landed | Bead | What | Blocked by |
| --- | --- | --- | --- |
| ☐ | fleet-rsia.1 | ASSUMED: fleet-packs exists with runtimes/ts, tiny, adapters/store, adapters/agent; the ts and tiny packs move out of this repo (Alberto's call) | — |
| ☐ | fleet-rsia.2 | The bd pack's skeleton, generated contract types, and the read verbs over bd 1.3.0 with their quirks and fixtures | fleet-wpf0.5, fleet-wpf0.2, fleet-rsia.1 |
| ☐ | fleet-rsia.3 | The bd pack's plain writes: create, update with the fences, append, close with the actor stripped | fleet-rsia.2 |
| ☐ | fleet-rsia.4 | The bd pack's orders, runs, holds, export and scratch | fleet-rsia.2 |
| ☐ | fleet-rsia.5 | The bd pack's doctor check and CI: fleet store check and a smoke ring on a pinned fleet | fleet-rsia.2–4 |

## Flight 10 — bd leaves the repository (`fleet-3krx`)

| Landed | Bead | What | Blocked by |
| --- | --- | --- | --- |
| ☐ | fleet-3krx.1 | fleet create asks which store the fleet uses and installs its pack from the pinned fleet-packs source and tag | flight 9 |
| ☐ | fleet-3krx.2 | Delete core/src/store/bd: the built-in branch, the defaults' bd-version check, the bd arms and tools go | fleet-3krx.1 |
| ☐ | fleet-3krx.3 | Docs pass: store.md, packs.md, getting-started, README, conventions and CONTRIBUTING | fleet-3krx.1, fleet-3krx.2 |

## The walkthrough

| Landed | Bead | What | Blocked by |
| --- | --- | --- | --- |
| ☐ | fleet-alt.1 | Walkthrough section 1 continues past `store.rs`: re-run step 0 at the new SHA, re-plan, next file | — (flights 2–3 have landed) |

## Later — unbuilt requirements from the archived PRDs

Epic fleet-2u3 holds one bead per requirement the archived cli, controller and
packs PRDs carried and nothing built (fleet-2u3.1 to .14: `fleet doctor`, run
rest grants, review size tiers, a pack registry, signed packs, a second agent
runtime, CI on macOS and Linux, per-platform measurements, SSE, the row-moves
journal, incidents, idle reclaim, attach, remote seats). None is specced, and
none is on a flight until someone boards it. The agent side of the adapters
design (fleet-2u3.6, .8 and .13 are the beads it absorbs) is brainstormed
after flight 10.

## Held for fleet-zlk

These are notes-as-state bugs that the typed timeline retires. They aren't
fixed in the prose grammar and aren't on a flight; each closes when fleet-zlk
is built, or is judged again there.

- fleet-4rl: deliver accepts a second column-zero marker, then fails its read-back after committing
- fleet-20s: preboard's filter reads an `On flight` note that nothing writes
- fleet-56e: landing records carry 7-character shas
- fleet-z4w: the SDK's gate trips the crash-cap park latch through `item.parked`
