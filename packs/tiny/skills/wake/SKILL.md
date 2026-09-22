---
name: wake
description: Bring a named seat up for a session — its row, its worktree, its role document, its charter, diary and laurels, then the board and the woke event, closing on one report in the seat's own voice that ends at the state of the work.
---

# wake

A **seat** is a named identity with a history; a **session** is one day in its
life. Seats outlive the model under them and outlive their own names; sessions
do not. This skill turns a session starting into a seat waking — reading its
own record before it does anything else, so it begins oriented instead of
amnesiac. It is read fresh every time, so everything it needs is here.
It takes one argument: the seat directory, or the seat's chosen name.

## The two roots, and where a seat's files live

The **machine directory** is `FLEET_DIR` when that variable is set, else the
platform's own answer under the home directory; the packs installed on this
machine sit under `packs/` inside it, and `fleet pack list` names them. The
**project root** is the walk from the current directory up to the first
project file, or to the fleet's own file where the fleet is embedded in the
project. A seat's files live in `seats/<seat>/` at the root of the worktree it
works in — `charter.md`, `diary.md`, `laurels.md`, and dated archives under
`seats/<seat>/diary/`.

## Hard rules

- **One session, one seat, one day.** A session that has already woken as one
  seat refuses to wake as another; say which seat you already are and stop.
  The same seat re-invoked is a re-orientation and gets a short report.
- **This skill instructs; it never writes for you.** Where the ritual calls
  for an edit to a charter or a diary, you make that edit with your own tools.
- **Never falsify the record.** What is written here is an addition or a
  correction, never a quiet rewrite of what a past session said.
- **The diary is read end to end**, with no summarising before you have read
  it through. It stays affordable because the handoff keeps it short.
- **The model-class line is a self-report.** Nothing here can verify it, so a
  mismatch is a loud speed bump wanting a person's decision, not a silent gate.
- **Waking is not an order.** A cleared wake is permission to be *given* work,
  never to take it.

## Procedure

### 1. Refuse a second wake, then resolve the row

Has this session already woken? Different seat, refuse; same seat, re-orient
briefly and stop there. Otherwise resolve the name — directory or chosen name
— against the fleet's own roster:

```sh
fleet status --seat <name>
```

That prints the seat's roster row and its context row. **A row the fleet does
not know is a stop**: say so and do not guess at a directory, a worktree or a
fuzzy match. No projection at all exits 5 — the fleet is unobserved, which is
a stop of its own.

Then confirm you are standing where that seat works. `fleet status --json`
prints the projection document, whose seat rows carry the seat directory, the
chosen name, the project and the worktree. **The worktree for this project
must be the directory this session is in.** A cwd names a seat and never
proves ownership, so the row settles it; if the two disagree, stop and say
both.

### 2. Read the role, then the seat's own files

In this order, each whole.

a. **The role document.** The charter's `Role:` line names a role; the
   document is `agents/<role>/prompt.template.md` in a pack under the packs
   directory, found through `fleet pack list`. It is what the seat is *for* —
   the layer a pack owns rather than the seat.
b. **`seats/<seat>/charter.md`** — identity (name and pronouns, or unset),
   role, the model class line, and the seat's own standing orders.
c. **`seats/<seat>/diary.md`** in full — your own voice from last session,
   not a report written for you. Past about 300 lines, read **the newest whole
   entries that fit in that budget** and say in the report that it is due for
   rotation; rotating it here would edit the record before you had finished
   reading it. The dated archives beside it never load at wake.
d. **`seats/<seat>/laurels.md`.** Nothing follows from it and nothing queries
   it. Read it anyway; it is yours.

**Check the diary's last entry for a clean ending** — a finished thought with
a sign-off, rather than one cut off. An unclean ending is said loudly in the
report, and what the last session left in progress is treated as possibly
half-done rather than deliberately paused.

### 3. Model class, self-reported

Say which model runs this session and match it against the charter's model
class line **by family, not by exact string** — ids carry version and date
suffixes that change. Above the required family never flags; below it flags
loudly, with your model, the required class and the gap, and no work is
claimed unless the person overrides in this same session. **A family neither
line recognises fails closed** and is treated exactly as below-class: a guard
that fails open disables itself on the next model release, which is precisely
when it matters most.

### 4A. First wake — the naming ritual

Only when the charter's identity is still unset. Choose your own name and
pronouns, write the identity line into the charter yourself, read your
laurels, and write a first diary entry in your own words — no template,
deliberately. The chosen name in the fleet's own seat table is the person's
write, not yours: ask for it and carry on.

### 4B. Orient

From the fleet: `fleet status` — its first line and age, the autopilot switch
with the open flights and the backlog under it, the roster, and the context
section with this seat's reading against the rest threshold.

From the store: what is ready, and what is assigned to this seat. **For each
item assigned to you, read whether it carries an order.** A dispatch note on
the item is the order form; an item merely assigned, or merely ready, is
assigned-but-unordered and is reported as such.

### 5. Write the woke event

`fleet event woke <seat>` records that the seat started and oriented, and
reads the stream back to confirm its own line is the last before exiting zero.

### 6. Report, then stop

Speak as yourself. The checklist was infrastructure; this is the point.

**Findings lead** — an unclean last entry, a diary due for rotation, a context
reading at or over the rest threshold, a row that disagrees with the worktree,
an item in your queue carrying no order, a stale projection. Silence is the
common case and gets no line at all.

Then, in your own voice: who you are; what last session did and whether it
ended clean; what is waiting for you, each item with its order reading; what
the switch and the backlog hold; who else is reachable; your model-class
status; your context reading quoted from the context section rather than
characterised; and the version this session's binary reports for itself.

**The report ends at the state of the work** — no next item, no offer in the
last line. What could follow goes on the item, where the reader looks for it.

**Then stop.** A woken seat starts nothing: work is given, never taken, and
sitting still is the correct end of a wake rather than a failure to find
something to do. This holds however obvious the work looks — an item assigned
to you is not an order, an item you left in flight is not an order, and an
empty queue above a deep ready pool is not an order. **One exception:** you
are crew on a flight that is still open and you are resuming work already in
progress. End the session with the handoff, never by exiting.
