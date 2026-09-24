---
name: morning
description: Read the open holds and the runs that stopped first thing, answer only the ones the record already settles, and hand the person one list of the rest with each question and its options.
---

# morning

Overnight the flights parked on holds, and some runs stopped. This is the
first read of both: an architect walks them, answers what the record already
settles, and leaves the person a single list of what genuinely needs them —
with the question and its options beside each one, so answering is a press
and not an investigation. Core offers the holds and the stream; **who reads
them first is this pack's opinion**, and this skill is it.

## The rule for what you may answer

**Answer only what the record already settles.** That is a ruling written on
the item itself, a ruling in a design document the item cites, or a premise a
later landing has since answered — read, quoted, and named in your answer.

**Never answer for the person:**

- anything a user would see or feel — copy, layout, timing, tone, the shape of
  a flow;
- anything a ruling reserves to them, however obvious the answer looks;
- anything where the record is silent, ambiguous, or says only what somebody
  intended rather than what was decided.

When you are reaching for a reason why an answer is *probably* fine, that is
the signal it is theirs. The list costs them a minute; a wrong answer costs a
flight.

## Procedure

### 1. The open holds, and the runs that stopped

A hold is the store's own object: `bd gate list --json` on this fleet's store.
**A failed run raises no hold**, and one nothing could classify re-runs until
it parks: read both with `fleet event tail --json --since <stamp> --type
run.failed` and `--type run.could_not_tell`, and list each unanswered with
its `reason`, or its `exit` and `read`. `fleet status` shows them under `runs`.

Each hold names the item it blocks. Read that item's **last `PARKED` region**
— the marker at column zero, then its `branch:`, `commit:` and `hold:` lines,
with the question and its lettered options under them. The whole question is
carried there and in the hold's own reason, so you need no other source. Take
the **last** region: an item parked, answered and parked again carries more
than one, and only the last is open.

### 2. Answer what the record settles

```sh
fleet clear <item> <letter> --text "<what the record says, and where>"
```

The letter names one of the question's own options. `--text` is what was
decided where the options did not carry it, and it is what makes a letter
outside them an answer rather than a typo. The verb writes the answer on the
item, clears the hold and writes `hold.cleared`; the item leaves the
blocked set and **nothing is dispatched** — the next flight that lists it is
what resumes the work.

Quote the ruling you answered from, in the `--text`, every time. An answer
whose grounds are not on the record is indistinguishable from a guess.

### 3. The parks at the return cap, and the red suites

Same reading, same rule. An item parked at the return cap asks whether to keep
going, and the record rarely settles that — it usually belongs on the person's
list with the returns quoted. A hold a red suite raised carries its reading and
its log; answering it means reading the log, not the summary.

### 4. Escapes

An escape is a defect found after its item landed. **No verb lists them** — you
read them off the store's own graph: items filed since the last morning,
traced back to the landing they came off. They are not holds and nothing
parks on them, but they are what the person most wants to see first thing, so
each goes in the list beside that landing.

### 5. Hand over one list, and stop

One list, in the order you would want them answered. Each row: the item by its
full id, the question in its own words, the options, and — where you have one
— which option the record leans toward and why, marked as a lean and never as
an answer. Above it, one line for what you answered and one for what you did
not touch.

Then say where the work stands and stop. The list is the deliverable; nothing
here dispatches, plans or flies.
