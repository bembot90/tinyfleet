---
name: adopt
description: Walk a person through the board their project kept before fleet — run the adopt-board doctor check, say what each thing it found means and what fleet reads in its place, and make only the mapping writes the person confirms, one at a time, each read back through fleet.
---

# adopt

A project that adds fleet brings a board with habits of its own: its own
order-like metadata, its own run labels, its own idea of who holds an item.
Fleet reads only its own namespaced keys and label (`fleet.orders`,
`fleet.run`, `fleet:run`), so none of those habits is an error — they are
invisible to fleet, and a person who flies an item they think is ordered finds
fleet reading it as unordered. This skill finds them, says what each means,
and maps what the person chooses onto fleet's, **with the person present**:
every write is theirs to confirm, so it is never run unattended.

## 1. Run the check and read the exit

`fleet doctor adopt-board`, from inside the project. It reads the board
through `fleet item list --json` and writes nothing. **Read the exit, not the
prose**:

- **0** — nothing to adopt. Say so, with the first line's count of items read,
  and stop.
- **3** — the board could not be read. Quote the lines under the row and stop.
  Never route around it by reading the store's own binary yourself: the check
  reads through fleet so that what it reports is what fleet will read. The one
  exception is a line saying `<id>'s run record is not one this fleet reads`:
  that is the `fleet.run` row below, reached through the refusal. Walk that
  row for the id it names, then run the check again.
- **1** — found. Walk the report below.

Say what was read before anything else: the ready set and the open items
labelled `fleet:run`, and nothing more. An item that is neither is not in the
report; say so rather than let "found" read as "the whole board".

## 2. Walk the report, one row and one question at a time

Take the rows in the report's order and skip the ones that say `none`. For a
row, read each example id through `fleet item show <id> --json` **before**
saying anything about it: the report says where to look, the item says what is
there. A row that says `not counted` is a key fleet does not read, and a list
row carries no raw metadata: say what the row means, and ask the person whether
their board keeps such a key and on which items. Never read the store's own
binary to count them.
Then say what the row means, what fleet reads instead, and the choices; ask
the person to choose, and wait. One question per turn.

**1. Names fleet owns, off the schema fleet reads.** Fleet wrote none of
these, or wrote them at a version this binary does not read.

| Row | What it means, and the choices |
| --- | --- |
| `fleet.orders` at a version or shape fleet does not read | The item reads as `order unreadable`, and `fleet dispatch` answers 3 on it rather than guess whether it is an order. Ask whether another fleet at a newer release flies this board: if so, write nothing — the fix is the release. If not, the index is stale or hand-written, and the write is removing it: `bd update <id> --unset-metadata fleet.orders --actor <person>`. Read back: `order` is `null`. |
| `fleet.run` at a version or shape fleet does not read | It is never counted: the item refuses fleet's read of it, so the check answers 3 with the refusal naming the item. The same question and the same write, `--unset-metadata fleet.run`. Read back: `fleet item list --label fleet:run --json` exits 0 again. |
| `fleet:run` on an item that is not a run record | Every open item under `fleet:run` counts against `[core.run] max_open`, and `fleet cancel` treats it as a run's record. Unless it is one fleet filed, the write is the label off: `bd update <id> --remove-label fleet:run --actor <person>`. Read back: `labels` in `fleet item show <id> --json`. |
| a `fleet.` key or `fleet:` label fleet never writes | The namespace is fleet's. Offer to remove it (`--unset-metadata <key>` or `--remove-label <label>`), or to move it under the board's own name: the new key written first and read back, then the old one removed and read back — two writes, two confirmations. |

**2. Conventions of the board itself.** Fleet reads none of these, so leaving
one as it is costs nothing but the mapping; **leaving it is always a choice**.

| Row | What fleet reads instead |
| --- | --- |
| an order-like metadata key | Fleet's order is written by one act, `fleet dispatch <id> --to <seat>`: the ordered entry on the timeline, the `fleet.orders` index and the assignee, together, and the seat is rung. So "your `orders.owner` is our seat" maps by DISPATCHING the item to the seat the person names for that owner — only when they want it flown now. **Never write `fleet.orders` by hand**: an index with no ordered entry beside it is a record fleet did not write. Once no tool of theirs reads the old key, the person may retire it with `--unset-metadata <key>`. |
| a run-like metadata key, or a run label that is not `fleet:run` | The board's own runs. Fleet's runs are only what `fleet run` files. There is nothing to map: never relabel one `fleet:run`, for the reasons in the table above. |
| an assignee-like metadata key | An owner maps onto the seat a dispatch names, as above. One reviewer for everything maps onto `[core] reviewer` in `fleet.toml`. A reviewer per item has no fleet reading today: say so and write nothing. |
| types and labels on the items read | What `[[core.flight.rules]]` in `fleet.toml` would match, by `type` and `labels`. Offer to draft rules; the write is the file, shown whole before it is saved and read back from disk after. `fleet status` prints the rules as it reads them, once the controller has published its projection (exit 5 until then). |

**3. The marker words fleet once wrote into notes.** The check cannot read
them: fleet reads no notes and no comment it did not write. Fleet parses none
of those words any more, so there is nothing to map and nothing to write —
what a person wrote stays theirs.

## 3. Every write, the same four steps

1. **Say it**: the one item, the exact command, and the field it changes.
2. **Ask**, and wait for a yes to that write. A yes to one write is not a yes
   to the next; items are batched only when the person says so, naming them.
3. **Run it** and read its exit. `--actor` names the person, so the store's
   audit row says who.
4. **Read it back through fleet** — `fleet item show <id> --json`, or the
   item's row in `fleet item list … --json` — and quote the field. A read-back
   that does not show what the write said stops the walk: say so, and write
   nothing more to that item until it has been read.

Never write a comment shaped as a fleet entry, a note, or any fleet key or
label by hand: fleet's own verbs write those, and the record guard refuses a
forged entry.

## 4. Close

Run `fleet doctor adopt-board` again. Report, by id: each write made and its
read-back, each row the person chose to leave, and what the check did not read
— the items outside the ready set and the run label, and the third class.
