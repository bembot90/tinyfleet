# docs/

What fleet does, as a user sees it: the commands, what they print, the files
you write and read, what fleet refuses and the codes it exits with. One file
per area.

This README is scaffolding, not product documentation. It states the rules the
area files are held to and lists them, and it is excluded from any site
rendered from this directory. The procedure behind every rule is the docs
skill, `.claude/skills/docs/SKILL.md`.

## Rules

1. **Behaviour, not implementation.** An area file says what a user observes.
   How the code does it belongs to the code walkthrough, and why fleet is
   shaped this way belongs to `brain/`. Shipped behaviour only: design nobody
   has built is not here.
2. **Agent-written only.** A session writes these files through the docs
   skill. A person never edits them.
3. **Corrections are beads.** Spot an error, file a bead labelled `docs`
   quoting the sentence and naming the file. The session that takes it
   verifies the claim against the code and fixes the file.
4. **Every claim verified when written**, against the code or by a command
   run against a built binary. The citations, `file:line` per claim, go in a
   review note on the bead that carried the change, never in the file.
5. **Reconciled at release.** A release pass walks the items landed since the
   last release and updates the areas they touched.
6. **This directory is canonical.** A published site is a rendering of it,
   built one way from here and never edited. The renderer is not chosen yet.

## Areas

| File | Area | Covers | Status |
| --- | --- | --- | --- |
| `getting-started.md` | Getting started | building and installing fleet, `fleet create`, `fleet start`, the plugin in a Claude Code session | planned |
| `packs.md` | Packs | installing and removing a pack, how packs layer and shadow, the defaults every fleet gets, pack settings in `fleet.toml` | planned |
| `items.md` | Items and the record | `dispatch`, `brief`, `deliver`, `ask`, `answer`, `review`, `land`, and the note each one writes on the item | planned |
| `runs.md` | Runs and workflows | `fleet run`, the workflow SDK, the takeoff and preboard workflows, gates and parks, the runs section of `fleet status` | planned |
| `seats.md` | The controller and seats | starting and stopping the controller, `observe`, named and transient seats, rest, `nudge` | planned |
| `guards.md` | Guards | the four guard classes: shell-trap, record, release-ref, production-write | planned |
| `status.md` | Status and the event stream | `fleet status`, `fleet event tail`, `fleet event show` | planned |
| `conventions.md` | Exit codes and conventions | the exit table every command shares, full item ids | planned |
