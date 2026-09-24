---
name: docs
description: Write, correct, reconcile and drift-check fleet's user-facing docs under docs/ — one area file per domain, every behaviour claim verified against the code or a command run when written, the citations in a review note on the bead and never in the doc. Use when an item asks for an area file to be drafted or corrected, at a release's docs pass, or to ask read-only whether an area has drifted from the code.
---

# docs

`docs/` answers one question: **what does fleet do, as a user sees it?** A user
is someone running a fleet: installing it, typing its commands, writing the
files it reads, reading what it prints and the codes it exits with, and writing
a pack for it. Every sentence in an area file is a claim about that, and every
claim was checked against the code by the session that wrote it.

`docs/` is canonical. A site rendered from it later is a rendering, and the
rendering is never edited (§ 10).

Everything you need is here, so this file runs in a session that has read
nothing else.

## 1. What goes in docs/, and what goes elsewhere

| Where | What it answers | Evidence for a docs claim? |
| --- | --- | --- |
| `docs/` | what fleet does, as a user sees it | — |
| `brain/` | why fleet is shaped this way and where it is going: the vision, the layers, the naming table, the lessons, the PRDs for work not built, and under `brain/archive/` the PRDs for built work and the flights ADR, kept as history | no |
| the walkthrough skill | how the code does it, taught line by line in sittings | no |
| `README.md` at the root | working on this repository: the crates, the make targets, driving the plugin from a checkout | no |
| the code under `cli/`, `core/`, `controller/` and `packs/`, and the tests beside it | what actually happens | **yes** |
| a command you ran against a built binary | what actually happens | **yes** |

**Behaviour, not implementation.** A claim in `docs/` is something a user can
observe: a command and its flags, what it prints, what it writes where a user
will read it, what it refuses and the exit it refuses with. How the code gets
there — crates, modules, types, functions, the order of internal passes — is
the walkthrough's subject and never `docs/`'s.

**Shipped only.** `brain/` holds design, including design nobody has built.
`docs/` holds none. A PRD requirement, an ADR decision or a README paragraph
is what somebody intended; the code is what happens, and only the code or a
run is evidence. Where the two disagree, `docs/` says what the code does.

**Odd behaviour is described, then filed.** When fleet does something
surprising, awkward or wrong, the area file says plainly what it *does*, and a
bead labelled `docs` records what it *should* do — a product finding, not a
doc edit. The doc never describes the behaviour somebody wishes it had, and
never apologises for the behaviour it has.

## 2. The rules

1. **Agent-written only.** A session writes `docs/`, through this skill. A
   person never edits a file under it.
2. **Corrections are beads.** A person who spots an error files a bead
   labelled `docs`, quoting the sentence and naming the file. The session that
   takes it verifies the claim against the code and fixes the file (§ 7).
3. **Every claim is verified when it is written.** Against the code, or by a
   command run against a built binary, at a named commit. A claim that could
   not be verified is left out, not softened.
4. **Citations live on the bead, never in the doc.** The draft's review note
   on its bead carries `file:line` for every claim (§ 6). The doc carries no
   `file:line`, no source path, no bead id.
5. **One file per area**, and the list is `docs/README.md` (§ 3).
6. **Reconciled at release.** A release pass walks the items landed since the
   last release and updates the areas they touched (§ 8).
7. **docs/ is canonical; a site is a rendering** (§ 10).

`docs/` changes in exactly four ways: an item that drafts an area file, a
correction bead, the release pass, and an item that adds, renames or removes
an area. A landing that changes behaviour does not edit `docs/` on its own
initiative; the release pass is what catches it up. That keeps one writer per
change and one place to look for why a sentence says what it says.

## 3. The area files

One file per area, named for what a user touches, lowercase with hyphens:
`docs/packs.md`, `docs/guards.md`. Filenames stay stable once written, so a
file diffs cleanly release over release and a published address does not move.

`docs/README.md` holds the list, and this skill maintains it:

- It is scaffolding: the rules in brief and the table of areas. It is not
  product documentation and is excluded from any site.
- A new area file lands with its row in the same commit. An area earns a file
  through a bead, never ad hoc.
- A row whose file does not exist yet says `planned` in its status column; the
  commit that writes the file changes it to `written`.
- Renaming or removing an area file is the person's call, raised as a
  question on the bead: once published, the name is an address somebody
  links to.

An area covers one domain completely and nothing outside it. When a sentence
needs a term another area owns, link to that area rather than explaining the
term twice; two explanations are two copies to keep in agreement.

## 4. The area-file shape

Every area file has the same skeleton, so a reader who has read one knows
where to look in the next:

```markdown
# <Area title, exactly as in docs/README.md's table>

<Lead, no heading: two to four sentences. What this area is from the user's
side, when you meet it, and what you can do with it.>

## Terms

<Only the terms this area introduces, one short paragraph or bullet each, in
the order the reader meets them. A term another area owns is a link to that
area, not a definition.>

## <One section per thing a user does, named for the act:
##  "Installing a pack", "Dispatching an item", "Reading the stream">

<What the act is for, in one or two sentences. The command, as an example
block. What you see. What it changes that you can see afterwards: a file, a
note on the item, a line on the stream. How it refuses, if it can.>

## When it refuses

| Situation | Exit | What you see | What to do |
| --- | --- | --- | --- |
| <the thing a user did or the state they were in> | <n> | <the message, quoted> | <the next act> |

## See also

- [<Other area title>](<other-area>.md): <one line on why you would go there>
```

`Terms` is omitted when the area introduces none. `When it refuses` is
omitted only when nothing in the area can refuse; say so in the lead rather
than dropping the section silently. Sections per act may carry `###`
subsections for flags or variants; nothing goes deeper than `###`.

### Voice

- **Second person, present tense, active.** "You run `fleet status`. It
  prints the roster." Fleet is the subject of what fleet does: "fleet refuses
  the delivery and exits 1."
- **Plain and definite.** No "should", "will", "may", "currently", "now" or
  "as of": each one is either an aspiration or history. What fleet does, it
  does.
- **The product's own words.** Use the names in `brain/naming.md`'s table —
  seat, item, flight, run, pack, routine, gate, park — spelled as the table
  spells them, and the command words exactly as a user types them. Never a
  word the naming table lists as not carried over.
- **Short paragraphs, wrapped at about 79 columns**, so a diff reads.
- **No marketing.** No "simply", "easily", "powerful", "seamless". A sentence
  that cannot be verified cannot be in the file.

### What a claim looks like

A claim is one observable statement: given this, fleet does that, and you can
see it here. Write it so a reader could check it with a command.

> Good: When `fleet pack list` cannot read the lock file, it prints
> `fleet pack list:` and the reason on standard error, and exits 3. A lock
> file that does not exist yet is not an error: you get the table's header
> row, and it exits 0.

> Not this: `pack_list` returns `Ok(Exit::CouldNotTell)` when `lock::read`
> fails (cli/src/main.rs:936), and `read` answers an empty `Vec` for a
> missing path (core/src/lock.rs:83).

The second is true and belongs to the walkthrough: it names functions and
types, and it carries its citations inline. The first says the same thing as
a user meets it, and its citations go in the review note:

```text
§ Listing installed packs
3. "When fleet pack list cannot read the lock file, it prints fleet pack
   list: and the reason on standard error, and exits 3."
   read: cli/src/main.rs:928-936
   ran: fleet pack list --lock <scratch>/dir.lock, a directory -> exit 3;
   stderr "fleet pack list: packs.lock cannot be read: Is a directory
   (os error 21)"
4. "A lock file that does not exist yet is not an error: you get the table's
   header row, and it exits 0."
   read: core/src/lock.rs:83-85
   ran: fleet pack list --lock <scratch>/missing.lock -> exit 0; stdout
   "name  source  version  commit  fetched"
```

### Command examples

A command example is a fenced `sh` block. Command lines start with `$ `;
output lines follow unprefixed, exactly as a real run printed them:

```sh
$ fleet <verb> <item>
<the output, as captured>
```

- Every example was run, against a binary built from the commit the review
  note names, in a scratch fleet (§ 5). An example is a claim like any other.
- Values that differ per machine or per run — paths, ids, times, shas — are
  replaced by angle-bracket placeholders (`<project>`, `<item>`, `<sha>`), and
  the same placeholder means the same value throughout the file.
- Cut long output with a line reading `...`; never reword a line of output.
- The exit is stated in the prose after the block ("It exits 0."), never shown
  as `echo $?`.
- A command a user should not run from a scratch fleet — one that loads a
  service, touches a real project's trunk, or writes to the user's own
  configuration — is shown without output and verified by reading the code.

### What never goes in

- **Internals**: crate, module, function, type or trait names; source paths;
  the order of internal passes; anything only the code can see.
- **Citations**: no `file:line`, in or under the prose, not even in a
  comment. They go in the review note.
- **Bead ids, PRD requirement numbers, ADR numbers, ruling dates, who
  decided.** The record is where a reader goes for why.
- **History**: "was", "used to", "renamed from", "no longer". The doc
  describes the product at one commit.
- **Design not built**, and work planned or filed.
- **Anything about the machine that wrote it**: seat names, worktree paths,
  this repository's own layout beyond what a user installs.
- **Links outside `docs/`.** A site renders `docs/` alone, so a link to
  `brain/` or to source breaks there. Link to sibling area files only.

## 5. Writing or changing an area file

1. **Name the commit.** Draft against one commit of main and record its sha.
   Build that commit (`cargo build`) and use `target/debug/fleet`.
2. **Read the code before the prose.** Start at the command's entry in
   `cli/src/main.rs` and follow it to what decides the behaviour; read the
   tests beside it, which state what somebody made sure of. Doc comments and
   README.md tell you where to look, never what is true; the archived PRDs
   under `brain/archive/prds/` are history, and say only what was intended.
3. **Run what can be run, in a scratch fleet.** Point every run at scratch
   directories so nothing touches the machine's fleet or the person's home:
   the command's own path flags where it has them (`--lock`, `--packs-dir`),
   and otherwise `FLEET_DIR=<a scratch directory>` on the command line, which
   is where every verb looks for the machine directory first. Never run
   `fleet start` or `fleet stop`, anything that loads a service, or anything
   against a real project or the machine's own fleet directory; read the code
   for those. Read each command's exit directly, never through a pipe.
4. **Write the claims**, in the shape of § 4, keeping a running list of each
   claim and its evidence as you go. Evidence gathered after the prose is a
   search for support, not a verification.
5. **Drop what did not verify.** A claim you could not check is left out and
   listed under `LEFT OUT` in the review note with the reason. Never soften it
   into "usually" or "may".
6. **File the findings.** Anything that was hard to explain, or that behaves
   surprisingly, is a bead labelled `docs` describing the product behaviour
   and what it should be instead. The doc says what fleet does; the bead says
   what it should do.
7. **Update `docs/README.md`** if the area's row changes status.
8. **Write the review note** (§ 6) and commit the doc and its README row
   together, with the item's full id leading the subject.

## 6. The review note

The review note is where a claim's evidence lives. It is appended to the bead
that carries the work (`bd note <id> --file <note>`). A drafter that does not
hold the record — a subagent, or a seat told not to write to the store — hands
the note back in its delivery, and whoever holds the record appends it
verbatim.

One note per area file per change, in this form:

```text
DOCS REVIEW NOTE: docs/<area>.md at <sha>
<n> claims, <k> of them also run. <m> left out.

§ <Section heading, as in the file>
1. "<the claim, quoted or closely paraphrased>"
   read: <path>:<line>[-<line>], <path>:<line>
2. "<the claim>"
   ran: <the exact command> in a scratch FLEET_DIR and HOME -> exit <n>;
   output as shown in the doc
   read: <path>:<line>

LEFT OUT
- "<the claim>": <why it could not be verified>

FINDINGS
- <the surprising behaviour, one line> -> <bead id>, labelled docs
```

- **`read:`** cites the line that decides the behaviour, not the line that
  describes it: the branch that refuses, the format string that prints, the
  constant that is the default. A test asserting the behaviour is cited too
  when there is one.
- **`ran:`** gives the command exactly as typed and the exit as read. A
  command example in the doc always has a `ran:` line.
- Every claim the change wrote or re-verified appears in the note — for a
  new file, every claim in it — numbered within its section in the file's
  order, so a reader auditing one sentence finds its evidence by section and
  number.
- The note names the sha, because line numbers move; a later reader resolves
  `file:line` against that commit, not against main.

## 7. A correction

A correction bead quotes the sentence it disputes and names the file.

1. Re-verify the claim against current main, from the code, as if writing it
   fresh; the bead's reporter may be right, wrong, or right about something
   else.
2. If the doc is wrong, fix the sentence, and re-verify the claims around it
   that share its evidence.
3. If the doc is right and the product surprised the reporter, the doc stays,
   and the finding is a `docs`-labelled product bead, linked from the
   correction.
4. The review note on the correction bead covers every claim the change
   touched, in § 6's form, with the verdict first: `CORRECTED`,
   `DOC STANDS`, or both, per claim.

## 8. The release pass

At a release, the docs are brought to the release's commit in one pass. This
pass is a step of the release workflow planned for 0.1.0's cut; until that
workflow exists, the person starts the pass by naming the two refs.

1. **List the items landed since the last release.** Every landing's subject
   leads with its item id, so `git log --format='%h %s' <last>..<release>`
   on the trunk is the list; before the first release, `<last>` is the commit
   the bootstrap drafted from.
2. **Judge each item**: read its bead and its diff and decide whether it
   changed anything a user sees. Record the judgment either way.
3. **For each area an item touched**, re-verify every affected claim and
   write the new ones, per § 5, against the release's commit.
4. **Run the drift check (§ 9) on every area no item touched.** An untouched
   area can still go stale through a change nobody tagged as user-visible.
5. **One review note on the release's bead**, carrying § 6's note for each
   changed file, and a `NO DOCS CHANGE` list naming each item judged not
   user-visible with one line on why. A judgment nobody wrote down is one
   nobody can check.

## 9. The drift check (read-only)

The question: *has this area drifted from the code?* Asked for one named area
at a time, at any commit, by anyone. It writes nothing: no edit under
`docs/`, no note, no bead. Its output is a report to whoever asked.

1. Read `docs/<area>.md` and take every claim in it, section by section, in
   order.
2. Find the area's most recent review note, if there is one; its citations
   say where to start reading. Lines move, so re-find each one rather than
   trusting the number.
3. Re-verify each claim against the current code, reading and running as in
   § 5, and sort it:
   - **current**: the code still does what the sentence says;
   - **stale**: it does not — say what the code does now, with `file:line`;
   - **could not tell**: the evidence could not be read or the command could
     not run — say which.
4. Report per claim, stale ones first, with the sha the check ran at:

```text
DRIFT CHECK: docs/<area>.md at <sha>
<n> claims: <c> current, <s> stale, <u> could not tell.

STALE
- § <Section> #<n>: "<the claim>"
  now: <what the code does>, <path>:<line>

COULD NOT TELL
- § <Section> #<n>: "<the claim>": <why>
```

A stale claim becomes a correction only through a bead; the check itself
fixes nothing.

## 10. Publish

`docs/` is canonical and a site is a rendering of it. **The renderer is not
chosen yet** (the person's decision, deferred to when publishing is taken up):
until it is, nothing publishes, and no session picks a renderer, adds a site
configuration or writes a publishing script.

Whatever renderer is chosen, these hold:

- **One way.** `docs/` to the site, never back. If the two disagree, the site
  is wrong and is rebuilt from `docs/`.
- **Verbatim.** The site renders the area files as they are; no redaction
  pass and no rewording, because a reworded copy is a second copy to keep in
  agreement.
- **`docs/README.md` is excluded**, always.
- **A renamed or removed area file stops the publish** for the person's call,
  because it takes down a published address.
- **Publishing rides the change.** A change to `docs/` is published as part of
  the work that made it, not on a schedule.

When the renderer is chosen, this section gets: the renderer, the build
command, where the site publishes, how a publish is verified, and what the
site's navigation is built from.
