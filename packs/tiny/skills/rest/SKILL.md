---
name: rest
description: A named seat asks to be rested mid-day — the whole handoff first, then the rest event, which stops the session and brings a woken successor up in the same worktree.
---

# rest

A seat sheds context without a person in the middle of it. It takes one
argument, the reason, and is invoked at the person's word, or when the context
section of `fleet status` says this seat is at or past the rest threshold.
**Never advertised**: do not offer a rest, mention one is available, or end a
report by suggesting one. Take a rest you need; never propose one you do not.

## 1. Run the whole handoff, by its own skill text

Start to finish — not a summary, not the parts that seem to apply. **A rest
that quietly does less than a handoff is how state rots between sessions.**

The diary carries one extra duty here: the items in flight and what state you
actually left them in, the flight you are on if any, and what the next session
should pick up **once it is told to** — a successor that is not crew on an
open flight wakes, reports and stops like any other seat, so your pointer says
what to resume when ordered and never authorises resuming unasked. Write it
for someone with none of your context: that is who reads it, minutes from now,
with nobody to ask.

## 2. Ask, read the exit, and stop

`fleet event rest <seat> --reason "<one line>"`. The controller stops the live
session on its next tick, starts a woken successor in the same worktree, and
removes the predecessor's row. **Read the exit, not the prose**: 0 is the
request written; 4 no live session; 5 no collector consuming the stream, so
nothing would pick the request up; 6 a transient row, which never rests and is
retired instead. **A refusal is a correct ending, not an error** — everything
is staged, so say which refusal you got in the command's own words and stop.
Never route around one: a request nothing collects leaves the seat staged and
silent, which is worse than an honest stop. Accepted, say the handoff is
complete, the seat staged and the request posted; then end the turn and go
quiet. Do not try to stop your own process — the controller does that within a
poll, and your part ended when the request did.
