# Integrations

**fleet is simple, local, and truly yours.** It ships a small foundation
that does the hard, boring parts of running coding agents unattended, and it
leaves every opinion about *how* to work to you. This page is the long form
of the vision page's "Primitives, not features": every place you can reach
in, what each one lets you do, and why the controller and the log are what
make all of them possible.

The page describes the shapes fleet is built to, and the shapes are settled.
Which commands are built, and what each one does, is the code's business,
and `docs/` describes it; what is not built yet is on the board. The PRDs
the page was first written from are archived under `archive/prds/` as
history.

---

## The shape

fleet is three layers, and every integration point is a seam between them.

**The controller** is the only piece that knows about processes. It talks to
your agent through one adapter, keeps seats alive through the night, delivers
messages, reads your config, and fires your orders on its own clock. Nothing
above it has to know which agent is running, how a session is started, or
where a transcript lives.

**The record** is what the controller and the seats write: one event stream,
one projection of the fleet's state, and the work graph the items live in.
Everything the fleet does is a line you can read, tail, fold, or act on. Stats
is a fold over the stream; the manager is a screen over the same two files.

**Packs** are where opinions live. Agents, skills, orders, formulas, guards,
templates. fleet's own **defaults** are embedded in the binary and carry the
mechanics with no opinion about the work. Everything on top of them is a pack,
and a pack is a folder you can write in an afternoon.

Every hook below is one of three things: **a file you edit**, **a command you
call**, or **a line you read**. There is no plugin API to learn, no
configuration language, and nothing that has to be compiled against fleet.

## What the controller gives you

**Bring your own agent.** Everything the controller does to a session goes
through one adapter with seven verbs: start, stop, adopt, nudge, status,
transcript, version. The model is a setting; the agent is an adapter; a
second vendor or a local model is an adapter and not a rewrite. Mix them in
one fleet and compare them in one log.

**Seats that survive.** A seat is a persistent identity with its own worktree
and its own history. The controller brings it up, adopts it across a restart
instead of respawning it, revives it in place after a crash with its context
intact, and when it rests, starts a successor that wakes oriented from the
record. You never write a supervisor, a restart loop, or a hand-off protocol.
Named seats rest; spawned seats retire; the controller knows the difference.

**Messaging you do not have to build.** A seat has a voice and no hands: it
says what it needs by writing events, and the controller acts. A person, an
order, or a script has hands: `fleet seat nudge` delivers one message to a
live session through the same adapter path the controller uses. A message is
a doorbell and carries no authority; the record on the item is what a seat
acts on. That one rule is what makes agent-to-agent messaging safe to leave
running.

**Config that reloads.** `fleet.toml` is policy: the seats, the reviewer,
the flight caps, the guards, the rest threshold, the load ceiling, the pinned
agent version. `project.toml` is one project: its name, its item prefix, its
worktrees, its suite. `config.json` is this machine's overrides. All three are
plain TOML or JSON, policy and the overrides are re-read the moment they
change, and a file that will not parse keeps the last good one and says so
once.

**Routines: your scripts on the fleet's clock.** A routine is one small file in
`orders/`, in a pack or in a project: a trigger and an action. Triggers are
`cron`, `cooldown`, or `condition`, and a condition is any command whose exit
you choose to read. Actions are `nudge` a seat, file an `item` on the work
graph, `exec` anything, or `run` a workflow. Fly at ten every night. Triage the
inbox at seven. Run your health check on the twenty-fifth. Ring the architect
when a script of yours says so. No second daemon, no per-routine service job, and every
firing is an event with one of three outcomes: completed, failed, could not
tell.

**Primitives you can call yourself.** `fleet seat spawn`, `feed`, `retire`
and `nudge` are the pieces `dispatch` and `fly` are built from, and they are
yours too. Write your own dispatcher in a shell script. Spawn a seat with a
first turn you composed. The load belt, the worktree, the roster row and the
retire's verification come with them.

## What the record gives you

**One stream, one envelope, one cursor.** `events.jsonl` is append-only, one
JSON object per line, sequenced, with an id, a timestamp, a type, and a typed
payload. `fleet event tail --follow --since <seq>` reads it from any point,
and the sequence is the resumable id the manager's live transport uses. A
third-party tool plugs into fleet once and sees every piece.

**The stream runs both ways.** The controller writes what it did; seats write
what they ask. `fleet event woke | rest | handed-off | exited` are the whole
seat lifecycle, and anything can emit them: a skill, a shell script, a cron
job of your own. The controller consumes its own stream on every tick and
cannot tell which one wrote the line. That is what makes the rituals
optional: fleet's wake and rest are one emitter of four events, and yours can
be another.

**What a person cares about, and nothing else.** The stream carries a crash,
a nudge, an adoption, a hold, a landing, a return, a cost. It never carries
the controller's own housekeeping, so a month of events is a month you can
read.

**Stats are a fold.** Every number fleet reports is derived from the stream
and nothing else: context before rest, what a landing costs, how often work
comes back, how long a flight takes, how each agent compares. A default set
ships. Track your own beside them, with a stated derivation, over the same
events.

**The work graph is open.** Items live in Beads, a local-first issue tracker
with its own command line and its own database. Every note a verb writes is a
note on the item: the order, the delivery, the verdict, the landing's gate
table, each in a grammar a reader can grep. Multi-step rituals are Beads
formulas. Your queries, your reports and your dashboards run against the
store fleet already writes to, not a copy of it.

**Build from the past.** An order whose condition reads the stream fires on
a pattern: three returns on one item, a seat that has rested twice tonight, a
flight that ran long. A script that tails the stream posts landings to your
chat. A stat you declare watches the number you care about and alarms when it
drifts. The night is a record, and a record is something you can act on.

## What a pack gives you

**Eight slots, one format.** A pack is a folder: a manifest that imports
other packs; `agents/`, `skills/`, `orders/`, `formulas/`, `doctor/`, a
per-provider `overlay/`, and `assets/`. The format is Gas City's, verbatim,
because it is good and known. `fleet pack add <git-source>` installs one and
pins it in a lock file; a pack is shared the way code is shared.

**Shadow, never fork.** Packs resolve by layer: yours on top, its imports in
order, the binary's own defaults underneath. A file at the same path in a
higher layer replaces the lower one. The defaults publish the list of every
file they let you replace: the brief a dispatched seat reads first, each
verb's note template, each verb's
skill, each guard, each order. Replace the review skill and every landing
runs your review. Replace the brief and every seat starts its day your way.
Nothing outside the list is yours to change, and nothing inside it needs a
fork.

**Guards, yours and ours.** A guard is a hook that refuses a class of mistake
before it runs and prints the rewrite. The defaults wire two, on by default,
that every fleet needs: the shell traps that read as green, and the writes that
would quietly rewrite the record. A pack adds its own in the overlay, on the
same rules, reading its targets from its own keys. Every guard, the defaults'
and yours, is one line in `fleet.toml [guards]`: you opt out, you never opt in.

**Your rituals, your agents, your doctrine.** Everything with an opinion
about how software should be built belongs in a pack. How a spec is written.
How a delivery is reviewed. What a seat does when it wakes. Which agents
exist and what each one is for. fleet's own doctrine pack, tiny, is a
worked example that layers over the defaults and changes no verb; a fleet that
wants none of it writes a third pack on the same slots.

**The provider appears in one place.** The overlay slot is per-provider, and
it is the only place a pack knows which agent runs the session. Hooks, a
session-start prime, a plugin manifest: all of it lives there, so a pack
written for one agent adds a second by adding one directory.

## What the verbs give you

The four verbs are commands, not a framework: `dispatch`, `deliver`,
`review`, `land`. Each takes what it is given and infers nothing, refuses
what it cannot verify, writes its note on the item, reads it back, and exits
by one shared table: done, refused, usage, could not tell. `fly` runs a batch of them and
`autopilot` pulls the next batch when one lands. They run outside flights
too, from your shell, from an order, from a script, so a workflow of your
own composes them the way `fly` does.

## What the manager adds

One screen over the two files the record already is: what is in the air, what
landed, what is blocked, what needs you. Decisions arrive as questions with
options and you answer them from your phone. If the manager publishes an API,
it is generated from the same types the cli uses, a handful of paths,
loopback by default, so the screen and the command line cannot disagree and
nothing leaves your desk unless you open the door.

## Built for agents to run agents

Most of what a person changes about their fleet, they will change by asking a
seat to change it. The first seat `fleet create` names is an architect whose
first job is to help finish the setup, and from then on "add an order that
flies at ten", "write a guard that refuses pushes to release branches" and
"replace the review with a two-reader one" are sentences said to an agent.
fleet is built for agents to run agents, so every seam on this page is a
shape an agent writes well — a TOML file, a markdown skill, a folder — and
every command on this page passes the tests an agent holds a tool to. We
learned both halves from Beads, the tool our own seats wield hardest: what
makes it easy to hold, and the handful of behaviours we learned by
measurement. The tests, in the agent's own terms:

**The exit code is my eyes.** I cannot see the screen. Zero means done and
read back; a refusal is non-zero and names the thing; *could not tell* is a
third exit and never rounds to either answer. One command that prints a
checkmark and did not land teaches me to spend a second call verifying every
write, forever. Every fleet verb shares one exit table, and every writing verb
reads its own write back before it exits zero.

**Every read has `--json`, whole and never truncated.** A reader that trims a
long field without saying so is lying to me in the one way I cannot catch.
`status --json`, `event tail`, `event show`: the same shape every time, the
whole record every time.

**Bodies come from a file, never from an argument.** A brief, a findings list,
a first turn: long prose on a shell line is where a backtick runs and a quote
truncates. fleet takes `--first-turn <file>` and `--return <file>`, and the
guard refuses the shapes that bite.

**No prompt, ever.** An interactive question is a hang I cannot answer.
`create` asks two questions and takes two flags to skip them; nothing else
asks.

**Additive verbs never replace.** A note appends. A field that can be
overwritten has a different word, and the record guard refuses the write that
would rewrite an append-only one.

**Refuse loudly, and print the rewrite.** A refusal I cannot act on is one I
route around. Every guard prints the class, the fragment, the fix and the
escape.

**The record is on the thing, not in my head and not in a message.** I wake
with no memory of yesterday. The item holds the order, the delivery, the
verdict and the landing in a grammar I can grep, so one read puts the day
back in my hands. A message is a doorbell; the record is what I act on.

**Tell me the rules before I have read anything.** `fleet prime` runs at
session start and prints the guards in force, the verbs and their exits, and
the item this worktree holds. `--help` is the contract, and the naming doc
keeps one spelling per thing.

**Identity is a setting, never inherited.** Every seat shares one git user.
The actor on every write is the seat and never that user, so the record says
who did it without a variable I have to remember.

**Check before act.** `order check`, `pack check`, `guard --check`, `observe
--once`: a way to ask what would happen, with the same three answers, before
anything moves.

**Everything is a file and nothing hides.** Policy is TOML, the stream is
JSONL, a pack is a folder, a skill is markdown. I can grep it, diff it, and
edit it with the tools I already hold. There is no state that lives only in a
screen, no second store with a polarity to reason about, and fleet never
regenerates a file into your repository behind you.

**Fast and local.** A verb answers in milliseconds, with no login and no
network, so I can call it a hundred times in a session without budgeting for
it.

Hold a tool to that list and an agent can shape it all night without a person
watching. That is the whole reason the seams are files and commands rather
than a plugin API: an agent can write a file, run a check, read the exit, and
know.

## Where to plug in

| You want to | Do this |
| --- | --- |
| run a different agent, or a local model | write an adapter with the seven verbs; set the model in `fleet.toml` |
| change what a seat reads first | shadow the brief template in your pack |
| change how deliveries are reviewed | shadow the review skill; the default is one reviewer's read |
| add a rule seats must not break | add a guard to your pack's overlay; it appears in `[guards]` |
| run something every night, or when a script says so | one `orders/<name>.toml` with a `cron` or a `condition` and an action |
| ring a seat from outside any session | `fleet seat nudge <seat> --text` |
| give a seat a different wake or rest ritual | emit `fleet event woke` and `fleet event rest` from your own skill or script |
| build your own dispatcher | call `fleet seat spawn` with a first turn you wrote |
| watch the fleet from another tool | `fleet event tail --follow --since`, or the manager's live transport |
| track a number of your own | declare a stat with its derivation over the stream |
| query the work | Beads' own command line and database; every note is on the item |
| share what you built | put it in a pack; `fleet pack add` by git source |
| have any of the above done for you | ask a seat; every seam is a file it can write and a check it can run |

## What we did not build

**A workflow engine.** No graph compiler, no step routing, no artifact
schemas between steps. Four verbs, orders that fire them, formulas for
rituals. If you need a graph, you can write one in a language you already
know and call the verbs from it.

**A configuration language.** Policy is TOML. Orders are TOML. Packs are
folders in a format that already exists. There is nothing of fleet's own to
learn.

**A cloud.** One machine, one person, one controller. Your agent
subscriptions, your hardware, your record. The manager's phone bridge is the
one thing that crosses the desk, and only when you send it.

**Opinions in the core.** The binary passes one test for every piece: *will
everyone need this?* Everything that fails it is a pack, including ours.

---

_fleet. Lights on._
