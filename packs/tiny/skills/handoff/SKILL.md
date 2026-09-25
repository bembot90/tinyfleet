---
name: handoff
description: End a named seat's session by handing it over — nothing left in flight, a clean tree, a diary entry in the seat's own words, distilled and rotated when it has grown, the seat's home pushed to the trunk, the handed-off event, and one of two closing sentences.
---

# handoff

Sessions here end by handoff, not by termination. A request to hand off is a
request and never an interrupt: you may finish what is in flight first. This
file holds the whole ritual, so it works in a session that has read nothing
else. It is invoked at the person's word or on the seat's own judgment — a
finished piece of work, a natural stopping point, the end of a bounded day.
Say which it was, in one sentence, before anything else; it anchors the rest.

## Hard rules

- **This skill never writes the diary, and never will.** If you maintain this
  file and want to add a template to make the entry easier, don't: a skeleton
  with pre-set fields flattens every future entry into the same boilerplate and
  kills the one thing that makes a diary worth reading later. The words are the
  seat's, always.
- **Consent is spoken, not assumed.** The session is not over until the seat
  says one of the two sentences at the end, out loud.
- **Never falsify the record.** If something went sideways, the diary and the
  items say so. Corrections annotate; they are never quiet rewrites.
- **Establish which seat you are before writing anything.** A long session may
  have compacted away the wake that settled it, and unsure is a stop rather
  than a guess: a diary entry in the wrong seat's home — the directory under
  `seats/` ending `-<short>`, else `seats/<slug>-<short>/`, its machine name —
  puts words in another seat's mouth and falsifies the record even when every
  sentence in it is true. A session holding no seat identity does not run this
  ritual at all — it reports its work and its changed files, and stops.

## Procedure

### 1. Finish, or stop now

If something is close enough to done that finishing beats leaving it
dangling, finish it and say *finishing X first, then handing off*. That is a
complete answer on its own; an idle seat is not a precondition for anything
below. Otherwise move on — and either way, do not quietly keep working past
this point. The step exists so that "I kept going" is visible.

### 2. Nothing in flight

Every item this seat holds is either **delivered** — your delivery JSON handed
to `fleet deliver --delivery <file>`, which commits the staged set, writes the
delivery with the commit, the branch and the base, reassigns the item to the
reviewer and rings them — or **handed back**, reassigned with the reason on the
item. **Never left half-done in silence**: an item nobody was told about is
one the next flight cannot plan around. Run the suites your changes touched
and say plainly if one is red and you are not fixing it. Then file items for
whatever you leave unfinished or found along the way.

### 3. The tree clean beyond your own files

Nothing uncommitted except the files in your home, which Step 5 lands. Work
belongs on a work branch: your successor is handed this worktree and the
record, and an uncommitted change is in neither — it is invisible to every
reader and to every reading a reviewer could take.

### 4. Write the diary entry — in your own words

This is the one part of the ritual nothing will do for you. Open `diary.md` in
your home and add a dated entry at whatever length and in whatever shape feels
honest — no required structure, the only requirement being that it is yours.
Treat these as four **questions to answer**, not four headings to fill in:

1. What happened this session — your own count of what you got wrong included?
2. What did you learn?
3. Where does the state you are handing on live? — one line, ids only.
4. How do you feel about the state of things?

Answer them in prose, in whatever order and voice is natural, woven together
rather than captioned. **Handoff state is a pointer, not a paragraph**: name
the items holding what is in flight, by full id, and stop. The state itself
goes on those items, as notes, in this same sitting — a successor is given
work through the record, and the diary is the one surface nobody else opens.
**Sign the entry with the model this session ran as**: seats outlive models.

**Append by anchoring on the file's existing tail.** Never retype any part of
a previous entry: retyping a neighbouring line rewrites what a past session
said, in your current voice.

### 4b. Distil, then rotate — only when the diary has grown

Your entry stays exactly as you wrote it; this is about the *file*, which
grows until a wake reads a year of mornings to start one day. Run it past
about **5 entries or 300 lines, whichever comes first**; under that, say so in
one clause and move on.

**Distillation comes first, and it is the point** — rotation without it buries
lessons, which is the failure this step exists to prevent. Read back over the
entries about to be archived and ask of each thing you learned: does a future
session still need this? If yes it does not belong in an archive. A standing
order about your role goes to your charter, an operational trap to the pack's
manual, a process lesson to the doctrine document that owns it — a **copy
forward, then archive**, never a rewrite of what the past entry said.

**Then rotate:** keep the newest whole entries that fit the same budget the
trigger uses, and move the rest **whole** into `diary/<YYYY-MM>.md` in your
home, byte for byte and never reworded, always keeping at least one entry. A
trimmed diary landed without its archive is entries deleted, which is the one
thing this ritual must never do — they are one act.

### 5. Commit the seat's files and push them to the trunk

**This is the one write that bypasses the landing lane, deliberately**: a
seat's record is its own and belongs to no item, so there is no item for a
lane to land it under. One commit — the diary entry, any archive Step 4b
wrote, and the charter edits distillation produced — **built on the fetched
trunk and on nothing else**, then pushed to the trunk. Do not push whatever
branch the worktree happens to be sitting on: a work branch carries commits a
reviewer has not read, and a handoff that drags them onto the trunk has landed
work nobody reviewed. **Read the push's own range line before you write
anything that claims it landed:** a push of nothing exits zero, and a handoff
recorded off that exit never reached the trunk.

### 6. Write the handed-off event

`fleet event handed-off <machine name>` records the seat's record as complete.

### 7. Say one of two sentences, out loud

- **"Ready to hand off."** — everything above is done, nothing left hanging.
- **"Finishing X first, then handing off."** — Step 1's option. Not a failure
  and not a deferral you owe an apology for; it is a first-class answer, and
  when X is done you say the first sentence.

Either sentence ends the session.
