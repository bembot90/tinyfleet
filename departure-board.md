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
| fleet-reb | Pin bd at 1.3.0, checked, and re-measure the store's quirks | Alberto upgrades bd (`brew upgrade beads`) | ☐ |
| fleet-ay8 | `fleet ask` on an epic leaves an edgeless gate; `ready` lets epics through | fleet-ah4 (`gate()`); with fleet-16s (`ready`) | ☑ |
| fleet-998 | `fleet dispatch <suffix>` refuses a ready item | fleet-16s (same line, `dispatch.rs:182`) | ☐ |
| fleet-pg7 | dispatch's hold check counts ordered items only (ruled) | fleet-fhi (`assigned_to`) | ☑ |
| fleet-bk115 | A test for the store opener's bd resolution (`cli/src/runs.rs:53`) | — | ☑ |

## Flight 3 — specced fixes, independent of the designs

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-45m | The SDK takes a returned delivery as carried (RETAKEN, `until(delivered)`) (Required) | — | ☐ |
| fleet-d11 | Delete the flight engine's leftovers: dead constants, the resume path, the rule matcher, seven census rows | — | ☐ |
| fleet-rzixz | One reader of the item store: the runner moves into core, bd calls bounded at 60s, one opener, prime's reader folded in | — | ☐ |
| fleet-xie | Generate the bd adapter's wire types from beads' OpenAPI spec at the pin (pin-stamp test) | fleet-reb (bd 1.3.0 installed) | ☐ |

## Flight 4 — the renames, namespacing and the archive

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-6gr | Gates become holds, clearances and checks; `fleet hold` / `fleet clear`; `[gates]` split; routines evaluate the trigger; split at pickup | — | ☐ |
| fleet-4j6 | Namespace fleet's keys (`fleet.orders`, `fleet.run`, label `fleet:run`), clean break; carries fleet-y3l's RERUN fix | — | ☐ |
| fleet-g68 | Archive cli, controller and packs PRDs, flights as superseded, bench stays live; restate every citation as its constraint | fleet-d11 | ☐ |

## Flight 5 — the record and seat identity (designs ruled; implementation beads split at the flight's start)

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-zlk | The record: typed timeline entries as bd comments, the store is the record, the stream carries wake signals, seats hand in JSON | fleet-iex (types), fleet-6gr (names), fleet-4j6 | ☐ |
| fleet-iex | Seat identity: UUID v7, `<slug>-<id>` names, `fleet seat add`, the actor as a typed reference | — | ☐ |

## Flight 6 — the store contract and adopting a board

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-0q4 | The store contract (`docs/store.md`), bd compiled in, external adapters one process per call, `fleet store check`; split (a)–(e) | fleet-rzixz, fleet-xie, fleet-4j6, fleet-zlk, fleet-iex | ☐ |
| fleet-0ml | Adopt an existing board: a doctor check that reads the board through fleet, and a mapping skill | fleet-4j6, fleet-2u3.1, fleet-zlk | ☐ |

## Flight 7 — the walkthrough resumes

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-alt.1 | Walkthrough section 1 continues past `store.rs`: re-run step 0 at the new SHA, re-plan, next file | flights 2–3 landed | ☐ |

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
