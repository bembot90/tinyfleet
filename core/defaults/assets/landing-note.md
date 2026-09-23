LANDED {sha} on {trunk} by {actor} (range {old}..{new}; squash of {commit}{rebased}; implemented by {builder}) — {tested}
{gate}

The one shape a landing writes, and the landing verb renders it whole. What is
written here under no marker is this note, which teaches it and is never written
to an item.

The first line is ONE line on purpose: a reader that is not human takes the
landed sha, the trunk it is on, the range the push printed, the commit that was
squashed, who wrote the work and the suite's own status off it without parsing
the table below. `{tested}` is `suite: <command>, rc <rc>` — the command `fleet
land --test` was handed, run on the land branch before the push — or, where
the landing was handed none, `NOT TESTED: ` and the sentence saying nothing ran.
An untested landing is allowed and never quiet: the suite row below says NOT
TESTED too, and no rc is borrowed from a run that did not happen.

`{rebased}` writes `; rebased from <base>` and nothing else, and only where the
delivery was cut from a base the landing did not land on: under the `advance`
strategy the squash onto a land branch cut at the fresh trunk tip IS the rebase,
so the landed sha is the rebased commit and this clause is what says the
delivery had been behind. A current delivery writes no clause, because a reader
who sees one twice cannot tell which of them moved.

`LANDED` is chosen so that a reader anchoring on the last delivery marker at
column zero, or on the last verdict, reads neither a landing: it begins with
neither of theirs, and a landing appended after a verdict ends that verdict's
region rather than extending it.

Then the gate as the verb read it — one numbered row per criterion, its verdict
and the evidence that produced it, in the order the gates were read, which is
the order they were printed in while the landing ran. Nothing in a row is typed:
each is rendered from a reading a step above it took. Beneath them a commands
block, so the next reader can re-run every verdict rather than believe it.
