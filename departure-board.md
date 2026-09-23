# Departure board

Everything that came out of walkthrough sitting `fleet-alt.1` (outline
`fleet-alt`, section 1: `core/src/store.rs`), 2026-09-23. The next flight is
the first `## Flight N` that still has an unticked row. Tick a row (`☑`) when
its item lands. The Blocked-by cell is a note written by hand; the item's own
dependencies are the real blockers.

## Flight 1 — store fixes, specced and independent

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-ah4 | Store helpers `answered`/`listed`/`created_id` over `std::process::Output` (F2+F8) | — | ☐ |
| fleet-fhi | `assigned_to` reads a seat's whole listing: `-n 0` (F12, Required) | — | ☐ |
| fleet-jqh | One `From<StoreError> for Stop` replacing eight mappings (F5) | — | ☐ |
| fleet-nv0 | `fleet brief` decides from the order index, not a notes search | — | ☐ |

## Flight 2 — store cleanup and the bd pin

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-9r6 | Read bd's JSON envelope now, before bd v2.0 makes it the default (F11, Required) | fleet-ah4 | ☐ |
| fleet-16s | Delete dead store fields: `created_at`, `flight`, `Ready` (F1) | lands after fleet-ah4 (same file) | ☐ |
| fleet-5y5 | `Item.blockers` counts only blocking dependency types (F9) | lands after fleet-ah4 (same file) | ☐ |
| fleet-016 | History out of comments, `Row` renamed `AssignedItem`, fix `open_labelled`'s doc (F3+F4+F6) | lands after fleet-16s (same lines) | ☐ |
| fleet-reb | Pin bd at 1.3.0, checked, and re-measure the store's quirks | — | ☐ |

## Flight 3 — generated wire types

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-xie | Generate the bd adapter's wire types from beads' OpenAPI spec at the pin | fleet-ah4, fleet-reb | ☐ |

## Flight 4 — design and spec passes with Alberto

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-zlk | Design: an item's record as a typed timeline the adapter owns; one log or two | — | ☐ |
| fleet-iex | Design: a seat is a UUID v7 identity, name optional, kind human or agent (clean break) | — | ☐ |
| fleet-6gr | Rename gates to hold / clearance / checks; verbs decided in the spec | — | ☐ |
| fleet-g68 | Archive built PRDs to `brain/archive/prds/`; restate the 132 code citations as constraints | — | ☐ |
| fleet-zex | Store calls bounded at 60s; where the helper lives | — | ☐ |

## Flight 5 — namespacing, the store contract, adopting a board

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-4j6 | Namespace fleet's metadata key and run label (clean break) | — | ☐ |
| fleet-0q4 | Executable store contract, bd adapter compiled in, `fleet store check`; split at spec | fleet-ah4; fleet-zlk and fleet-iex designs first | ☐ |
| fleet-0ml | Adopt an existing board: read-only scan verb and mapping skill | fleet-4j6 | ☐ |

## Flight 6 — the walkthrough resumes

| Bead | What | Blocked by | Landed |
| --- | --- | --- | --- |
| fleet-alt.1 | Walkthrough section 1 continues past `store.rs`: re-run step 0 at the new SHA, re-plan, next file | flights 1–3 landed | ☐ |
