# fleet

**A lights-on software factory for one person.**

You describe the work. Your fleet builds it, reviews it, lands it, and tells you
what it needs from you in the morning. It runs on your computer, with the agents you already use, and it gets out of the way.

---

## The problem

Coding agents got good enough to finish real work without you watching. Nothing
around them did.

Running more than one agent means a pile of scripts nobody else can run,
a terminal you cannot leave, and a morning spent working out what happened
overnight. The tools that exist for this were built for teams and priced for
companies: orchestration platforms, cloud dashboards, API bills. A solo builder
does not need any of that. They need the agents to keep working while they
sleep, and a clear view of what happened when they wake.

That is the whole product.

## What fleet is

fleet is the small set of primitives that turns coding agents into a factory
you can leave running:

- **Seats.** Named, persistent agent identities with their own workspace, their
  own history, and their own memory of yesterday.
- **Flights.** A batch of work handed to the fleet, built by the seats, reviewed
  and landed without you, with every decision you owe collected for the morning.
- **The log.** One event stream of everything the fleet did, so the night is a
  record and not a mystery.
- **The controller.** The thing that keeps the lights on: it restarts seats that
  rest, projects how far each one can go, and holds the fleet to a known,
  measured version of its substrate.
- **The manager.** One screen where it all comes together, on your desk or on
  your phone.

fleet owns the seats, the flights, the log, and the record. It has one
dependency, Beads, the local-first issue tracker the work graph lives in. The
agent that sits in a seat is yours to choose: the first fleet flies on Claude
Code, and the model underneath is a setting and not a dependency, so a
different vendor tomorrow or a local model the day after joins the same fleet.
The doctrine of _how_ work should be done is a pack you can install, adapt,
or leave out entirely.

## Why fleet

**Minutes to first flight. Years of polish after that.**
One command installs it. A short walk-through asks which parts you want. Every
feature is designed to the standard of a product you would pay for, because it
is one.

**Primitives, not features.**
fleet gives you seats, flights, a log, a controller, and a manager, and every
seam between them is a file you edit, a command you call, or a line you read.
What other tools ship as features — a workflow engine, a rules console, a
configuration language of their own — you build on those primitives in a
language you already know, or install as a pack in a format that already
exists. Small enough to understand in an afternoon. Deep enough to run your
whole product. The seams, one by one: `integrations.md`.

**Take the parts. Leave the opinions.**
Every piece works alone and composes with the rest. Keep a few named seats
working overnight with nothing but the controller and a config file. Add
standing orders when you want the fleet to act on a schedule. Add the manager
when you want to see it. The doctrine layer, the part with opinions about how software should be
built, is optional, adaptable, and ships as a worked example rather than a
requirement.

**Runs on your machine, on your accounts.**
No cloud tenancy, no team plan, no second bill. fleet is built for one person
and the computer in front of them. It uses the agent subscriptions you already
pay for, or the model already running on your hardware. The premium tier adds
a bridge to your phone. Nothing else ever leaves your desk unless you send it.

**Bring your own agent.**
A seat is a seat whatever sits in it. Run Claude Code today, add an OpenAI
seat when it earns one, put a local model on the night shift when it is good
enough. Mix them in one fleet, compare them in one log, and switch without
rebuilding anything. fleet is loyal to your work, not to a vendor.

**Safe to leave alone.**
An unattended fleet is only useful if you trust it. fleet ships with guardrails
on by default: the record of what happened cannot be quietly rewritten, and the
classes of mistake that agents make at three in the morning are refused before
they run. A pack adds guardrails of its own — ours keeps the actions that are
structurally yours, a release, a production write, yours. You opt out of a
guardrail; you never have to remember to turn one on.

## The pieces

### fleet-cli

The one entry point. Install, upgrade, create a seat, switch autopilot on,
file a bug against fleet itself from inside any agent session. The installer is
a menu: pick the pieces you want and skip the rest.

### fleet-log

The single event stream every other piece writes to. What each seat did, what
landed, what was refused, what a flight cost, what is waiting on you. One
format, one place, so third-party tools plug into fleet once and every piece
shows up.

### fleet-stats

The numbers that matter for a factory: how far each seat can go before it
needs to rest, what a landing costs, how often work comes back, how long a
flight takes, and how each agent compares on all of it. A default set ships
with fleet. Track your own beside them. If the manager is installed, stats is
a tab in it.

### fleet-controller

Keeps the fleet alive through the night. It brings seats up, brings them back
when they rest, hands work from a seat to its successor without losing the
thread, and projects how much runway each seat has left. It pins each agent
the fleet runs to a measured version and tells you when a new one is worth
moving to, so an upstream release never surprises you at two in the morning.
It is the piece that knows how to talk to each agent, so nothing else has to.

It also keeps the fleet's standing orders. Run a flight every night at ten.
Triage the inbox every morning. Run the health check on the twenty-fifth. An
order is a small file, in a format that already exists, and a pack can carry
its own; the controller fires them on its own clock, so recurring work is
something the fleet does without being asked and without a second daemon.

### fleet-manager

The part nothing else has. A single view of the fleet: what is in the air,
what landed, what is blocked, and above all what needs you. Decisions arrive
as questions with options, not as pages to read. Answer them from your desk or
from your phone at breakfast. This is where fleet stops being a set of scripts
and becomes a product, and it is the door to the premium tier.

### fleet-packs

Everything with an opinion lives in a pack: the agents, the skills, the
formulas, the standing orders, the guardrails, the rituals. A pack is a
folder in an open format, the same one Gas City packs use. Install one, import
several, or write your own on the same primitives.

fleet's own **defaults** ship inside the binary: the mechanics of moving work,
without an opinion about the work. Four verbs — dispatch, deliver, review,
land — the orders that fire them, and the guardrails, on by default. They are
what every fleet runs on, and `fleet create` writes them out; no pack is
installed to get them.

On top of them sits **tiny**: our doctrine, the way we run our own product.
How work is specified so an agent needs no clarification. How a delivery is
reviewed and landed. How decisions are surfaced and rulings recorded. How a
seat wakes, works, rests, and hands off. It layers over the defaults and adds
the opinions. It is offered as a starting point and a worked example, it is the
only part of
fleet with an opinion about how software should be built, and it is the part
you can leave out.

## A night with fleet

At ten, a standing order opens a flight from the work you marked ready. The
controller brings up the seats. They build, deliver, review each other's work,
and land what passes. One seat runs low and hands off to its successor, which
picks up mid-thread. A delivery raises a question only you can answer; it goes
on the morning's list instead of stalling the night. By six, the flight is
closed, the log has every event, and the manager has one page for you: five
things landed, one decision owed, two options, a recommendation.

You answer it from your phone before you get up.

## Who it is for

Solo builders with a real product and no team: the person shipping an app to
live users on nights and weekends, who wants leverage without a payroll.
fleet is not for enterprises, not for platform teams, and not for anyone who
wants an orchestration console with a seat-based license. If you have ever
wished your side project had a night shift, this is for you.

## Principles

- **The record is the record.** Everything the fleet does is written down once,
  by the thing that did it, and never rewritten.
- **Work is given, never taken.** A seat does not start anything on its own.
  You, or the schedule you set, give the orders.
- **Decisions are questions.** Anything that needs you is surfaced as a
  question with options, never buried in a report.
- **Less is more.** Every feature earns its place. The product stays small on
  purpose.
- **Your machine, your accounts, your call.** Local-first is a promise, not a
  deployment option.
- **The agent is a choice.** fleet owns the seats, the flights, and the
  record. What sits in a seat is yours to pick, and to change.

## Where it is going

fleet is running today, in its first form, as the factory behind a live
product with real users. The pieces above are being separated from that
factory one at a time, each replaced in production the moment it is
extracted, so every release has already run a night shift before it ships.

The first fleet flies on Claude Code, because that is what runs the factory
now. The seam between fleet and the agent is drawn on day one, so the second
agent is an addition and not a rewrite.

The free core is the factory. The premium tier is the manager on your phone
and what grows from it. The goal is not a large company. It is a tool good
enough that solo builders reach for it, and a business small enough to fund
the next passion project.

---

_fleet. Lights on._
