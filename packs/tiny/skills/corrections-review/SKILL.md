---
name: corrections-review
description: The person reviews one landed item with an architect — every question logged on the item verbatim before it is answered, honest argument including against the architect's own spec, and each cut written back as a note plus a follow-up item for any rule that should become a lesson.
---

# corrections-review

Landed work has come back, to be judged again — this time for whether every
line of it earns its place. The person reviews with an **architect**, never
with the seat that wrote it: they ask, refactor and cut; the architect
answers, argues honestly, and turns what was done into rules the fleet will
apply before anyone has to ask again.

It takes one argument: the item whose landing is under review. It runs at the
person's word — do not self-invoke it because a landing looks fat.

## What it is, and is not

Not a correctness review: the landing already had one. The question here is
**does every line earn its place** — what should not exist, what should
collapse into what, what is named wrong, which test is worth its cost, which
document is the code and which is noise.

**Nothing is written into a document during the sitting.** The item carries
the record; documents change afterwards, in the act described at the end.

## 1. Read the record, and start logging

Read the item whole — the spec, the acceptance, the close and the landed
commit it names — then read that commit.

**From this moment, every question the person asks goes on the item verbatim,
before you answer it.** Number them. A question is a rule they applied before
deciding whether to change anything, and it is the richer half of the input —
also the half that evaporates at session end if nobody writes it down.

A supervised item is reviewed the same way and on the running thing rather
than the diff. There is no hunk for a pixel or a line of copy, so **the cut is
whatever they say it is, in their words**, logged verbatim exactly as a
question is.

## 2. Answer honestly, including against yourself

When the item is one you specced, say so, and say whether the spec was the
defect. Give the case for, the case against, and then your position. They
rule; your position is input, and a position you soften to agree with them is
worth nothing to either of you.

## 3. Their changes land in the working tree

They refactor, or say cut it. Reverse-apply the hunks rather than restoring
old copies wholesale when their other changes overlap the same files, then
verify the tree is what it should be — the change they asked for and nothing
else. Run the touched area's suites **in the tree the changes are in**; a
green anywhere else proves nothing. Record each ruling on the item verbatim,
with the mechanics under it.

## 4. Dissect, on the item

When they say they are done, read the working diff hunk by hunk alongside the
question list and the cuts they named, and write **one note per hunk, question
or named cut** on the item. Each names: what it was, in one line; its category
— naming, should-not-exist, collapse, placement, comment, test-worth,
doc-shape, correctness, taste; **the rule**, written as the general sentence
that fires on the *next* item rather than this one; whether it is a first or a
recurrence of a rule already on the record, searched for rather than
remembered, because a recurrence is an enforcement gap; and what should
become of it.

Hunks that move only because a rule moved — callers, renamed references — are
**consequence hunks**: name them together in one note and do not dissect them.

Close with the **measured** count of lines removed against lines added for
this review. Never an estimate: the trend across reviews is what the fleet
reads, and a trend built on estimates is not one.

## 5. File the follow-ups, then stop

**A rule that should become a lesson gets its own item, filed now**, with an
edge back to the item under review and with the red-proof named where the rule
can be made executable. A rule with no item behind it is a sentence nobody
will apply.

Then one closing note on the reviewed item: the verdict, what changed and why,
the note count and the measured metric, and every follow-up you filed by full
id. Read it back before you trust it landed. No status change. The commit is
the person's, and reminding them of it is not your job.
