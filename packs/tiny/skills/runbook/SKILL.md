---
name: runbook
description: Build an operational runbook artifact — a published page someone follows step by step while executing against real infrastructure. Use when handing the person (or another seat) a procedure to run: a deploy, a cutover, a migration, a rotation, a recovery. Carries the house style so every runbook in the fleet reads as the same document.
---

# runbook

A **runbook** is one published artifact per procedure: the steps someone runs,
in order, with the verifications between them and the rollback at the end. It is
read *while executing*, usually against production, often on a second screen — so
it is scanned, not read through, and everything about the house style serves
that.

It exists because the alternative is a wall of commands in terminal scrollback,
which cannot be re-read on a phone, cannot be handed to someone else, and loses
the ordering constraints that are the entire risk of the procedure.

**The style is settled.** The person whose product this is approved it on
sight, extracted whole from a cutover page that used it. Do not redesign it
per-runbook. If it needs to change, that is an item and their call — the
licence is to reuse, not to revise.

## How

1. Copy `template.html` beside this file to your scratch directory.
2. Set `<title>` to the runbook's own name — a short noun phrase, two to four
   words, no appended explainer.
3. Replace everything below the `RUNBOOK TEMPLATE` comment. **Leave the
   `<style>` block untouched.**
4. Publish with the `Artifact` tool. Hand the URL over in your report.

The CSS is inline and that is deliberate: the artifact CSP blocks external
stylesheets, so a separate `.css` file could never be linked — only copied. One
file means there is no second copy to drift out of agreement with the first.

**This pack ships no test over the template**, so nothing will tell you if
you edit the `<style>` block by accident. Copy it; do not modify it in place.

## What the devices mean

The style encodes information. A device used for decoration is worse than no
device, because a reader who has learned the vocabulary will trust it.

**Semantic colour is separate from the accent.** The teal accent is identity and
carries no meaning. The other three do:

| | Means | Where |
|---|---|---|
| Amber | This changes what real users hit, spends money, or is hard to undo | `.step-n.is-act`, `.note.act` |
| Rose | A blind spot — what an instrument cannot perceive, or a way this bites | `.note.stop` |
| Green | This is what right looks like | `.note.ok`, `.cell dd.is-ok` |
| Teal | This step is a verification | `.step-n.is-check` |
| Plain | Safe, local, reversible | `.step-n` |

**Step numbers are coloured by act type, so the left gutter is scannable on its
own.** A reader should be able to run their eye down the numbers and see where
the danger is without reading a word. This is the single most load-bearing thing
in the design — if you colour steps by anything else, the column starts lying.

**Numbering is a claim that order is load-bearing.** Steps are numbered because
in a procedure the sequence carries information the reader needs. The `.fixes`
block deliberately is *not* numbered: corrections are a set. Do not number a list
that is not a sequence.

**Monospace is the display face, not just code.** Headings, eyebrows and labels
are mono because operational documents are overwhelmingly identifiers — app
names, ports, env keys, service paths. It is the subject's own vernacular. A page
whose subject is *not* identifiers should not inherit this reflexively.

**Every verification names its instrument, and its blind spot.** `.note.stop`
under a check is not optional garnish: a green whose client cannot see the
failure mode is worth nothing, and the reader has no way to know that unless the
runbook says so. `curl` cannot see CORS; an access log written at stream close
cannot witness an open stream. Name it.

## What the sections are for

- **Status strip** — the state of the world before the reader starts, in four or
  five chips. What has landed, what is blocked, what still needs them.
- **Read this first** — anything that changes how they execute, *above* the
  sequence. A correction to a stale instruction goes here, never buried at the
  step it contradicts.
- **Values, pinned against the tree** — the table's `Source` column is its whole
  point. A value with a file and line beside it can be re-checked when the tree
  moves; a value without one is a claim.
- **The sequence** — numbered steps, verification steps interleaved rather than
  batched at the end.
- **Rollback** — always present. When there is nothing to undo, say that
  explicitly rather than omitting the section.
- **What I could not verify** — unmeasured concerns stated as unmeasured, and an
  explicit line naming what was read versus what was actually executed, at which
  commit.

## Writing the commands

**Every command carries its own tree.** A runbook handed to the person runs
from their own checkout, not from your worktree, and unlanded work exists only
in seat worktrees. Put the `cd` inline in the same copy-paste block, and say
in one clause which tree and why whenever it is not theirs.

**Capture rollback values before overwriting them**, in the same block as the
overwrite, so a reader following top-to-bottom cannot skip it.

**Never chain a status-bearing command into a pipe** in a runbook any more than
in your own session — `cmd | head` reports `head`'s status. If the reader needs
an exit code, give them the bare command.

## When not to use this

- A one-line command. Say it in the session.
- Something you are going to run yourself. This is for procedures handed over.
- A review or a report of findings. That is a `report` — the sibling skill
  beside this one carries its house style.
- A design proposal or a PRD. Those argue for a choice and want their own
  treatment — reach for `artifact-design` and build something suited to them
  rather than forcing this shape onto them.
