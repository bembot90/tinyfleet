---
name: report
description: Build a findings-report artifact — a published page that presents an audit, a review, a measurement write-up or a study as ranked findings with evidence, so a reader can judge each one and rule on it. Use when an item's deliverable is a report rather than code, and someone other than the author will read it. Carries the house style so every report in the fleet reads as the same document.
---

# report

A **report** is one published artifact per body of findings: what was looked
at, what was found, how each finding was classified and measured, what it would
change, and — appended once they exist — the rulings made on it. It is read
*to decide*, usually once, usually on a laptop, by someone who did not do the
work — so the summary comes before the detail, every claim carries its
evidence, and the honest bounds are set beside the findings rather than in a
footnote.

It exists because the alternative is an item's notes: a wall of monospace that
the author can navigate and nobody else can, and that cannot be handed to
someone on a phone.

**The style is settled.** The person whose product this is approved it on
sight, extracted whole from a feature audit that used it. Do not redesign it
per-report. If it needs to change, that is an item and their call — the same
licence the runbook style carries: reuse, not revise.

## How

1. Copy `template.html` beside this file to your scratch directory.
2. Set `<title>` to the report's own name — a short noun phrase, two to four
   words, no appended explainer.
3. Replace everything below the `REPORT TEMPLATE` comment. **Leave the
   `<style>` block untouched.**
4. Publish it, and record the URL on the item in a note saying the notes
   remain the record and the page is a rendering of them.

The CSS is inline and that is deliberate: the artifact CSP blocks external
stylesheets, so a separate `.css` file could never be linked — only copied. One
file means there is no second copy to drift out of agreement with the first.
The two Google Fonts links are the one external asset the CSP admits; every
face has a real fallback stack, so the page holds if they never load.

**This pack ships no test over the template**, so nothing will tell you if
you edit the `<style>` block by accident. Copy it; do not modify it in place.

## What the devices mean

The style encodes information. A device used for decoration is worse than no
device, because a reader who has learned the vocabulary will trust it.

**Semantic colour is separate from the accent.** The teal accent is identity —
links, rank numerals, the verdict strip — and carries no meaning. The
classification chips do:

| Chip | Means | Class |
|---|---|---|
| Amber `a` | The problem is **unfixed** — nothing here addresses it | `.chip.a` |
| Blue `b` | The problem is **half-fixed** — addressed partially or by ritual | `.chip.b` |
| Teal `c` | The problem is **fixed by our own code** — the finding names what that code could shrink or retire | `.chip.c` |
| Amber `up` | A caveat that gates the finding — needs an upgrade, untested, depends on another row | `.chip.up` |
| Plain `ok` | No caveat; measured present | `.chip.ok` |

A report about something other than "what we underuse" keeps the *shape* — a
small set of chips whose colours mean one thing each, declared once at the top
of the section — and rewrites the legend. Never reuse the amber/blue/teal
triplet for a taxonomy that is not (unfixed, half-fixed, ours), because a reader
who has seen one report will read the colours before the words.

**Rank numerals are a claim that order is load-bearing.** The cards in the
findings section are numbered because they are *ranked* — by what adopting each
would retire, or by severity, or by cost — and the report says which, once,
above the first card. The ruled-out list and the misuse list deliberately are
*not* numbered: they are sets. Do not number a list that is not a sequence.

**The verdict strip is three numbers, not a paragraph.** The count that
answers the reader's first question, the count that answers the gate they
were worried about, and the count they would otherwise miss. If the report
cannot be reduced to three numbers, it has not finished thinking.

**The summary table precedes the detail.** The top-N table restates the ranked
cards in one row each — feature, problem, what it retires, the gate — so a
reader who stops after the table has the report's whole argument. The cards
below are the evidence for it, not a second telling.

**Every card carries a doc pointer and a local evidence pointer.** `Doc` is
where the claim about the *outside* thing comes from; `Evidence` is where the
claim about *us* comes from — an item id, a doctrine section, a tool path, a
diary lesson. A row with one and not the other is an opinion. `Bound` is where
the author states what the finding does not show; it is the most trusted
field on the page precisely because it argues against the row.

**Corrections the author owes go in the misuse section, named as
corrections.** A measurement the author got wrong and then fixed is more
valuable published than hidden — it tells the reader which instrument to
distrust — and the house style gives it a place so it is never a footnote.

**The coverage statement is the footer, and it names what was *not* read.**
A report that says only what it covered reads as complete. Name the deliberate
remainder, the partial reads, and the point in the author's window at which
they stopped.

**Rulings are appended, not woven in.** When the report has been read and
decisions made, the rulings and the items filed off them go in their own
section at the end, in a table. The findings above stay as they were written
— the record of what was found is not edited to agree with what was decided.

## What the sections are for

- **Header** — eyebrow (item id · author · date), title, one-sentence lede
  saying what was looked at and how it was ranked, a meta row of the facts the
  reader needs before anything else (versions, counts, coverage in one clause).
- **Verdict strip** — the three numbers.
- **The top N** — the summary table. Recommendations only; say so in the line
  above it.
- **Findings** — ranked cards with `Doc`, `Cmd`, `Measured`, `Problem`,
  `Evidence`, `Retires`, `Bound` as the row needs them; the chip legend
  declared once above the first card.
- **Used wrongly or partially** — the `.ruled` list: things in use whose use
  deviates from what the source says, and the author's own corrections.
- **Read and ruled out** — the `.ruled` list, one line each with the reason,
  so the next reader knows each was considered rather than missed.
- **Rulings and items filed** — appended after the sitting; a table of item,
  what, shape. A `.note` line under it names what was *not* adopted.
- **Footer** — coverage, method, and where the full record lives.

## Writing the rows

**Measure before you write, and say how.** A count comes from a query the
reader could re-run; an "upgrade needed" answer comes from running the verb on
the pinned version, not from the docs. State the instrument in the footer's
method line.

**Refute the item's own hypotheses when the evidence does.** A report that
confirms every candidate it was handed has not measured anything. The
strongest row in the audit this style came from was the one that refuted its
own dispatch.

**Rank by what adoption would retire, and say when a row ranks above its
size.** The cheapest finding on the page may deserve second place because it
costs nothing to try; say so in the card rather than letting the reader
wonder why a one-liner outranks a redesign.

## When not to use this

- A procedure someone follows while executing. That is a `runbook`.
- A design proposal or a PRD. Those argue for a choice; this reports what was
  found. Reach for `artifact-design` and build something suited to them.
- A one-paragraph answer. Say it in the session.
- A report only its author will read. The item's notes are enough.
