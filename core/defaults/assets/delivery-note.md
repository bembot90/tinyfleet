DELIVERED <sha> — <seat>
commit:  <sha, read from the commit's own output — never from the trunk>
branch:  <the work branch this sits on>
base:    <trunk> at <sha>, fetched at <time>
files:   <the paths this delivery touched>
checks:  <each acceptance check the item named, with its observed result>
suite:   <the suite that ran, its exit status read from the command itself>
spec corrections: <N> — <one clause per correction: the premise, and the
         measurement that refuted it; or "none">
not proven: <what this delivery does NOT establish, as a list of surfaces and
         the command that would have measured each>
decisions: <N> — or "none"
  D1 <the call made>; not taken: <the alternative>; because <one clause>
covers: <the requirement numbers this slice covered, or "none">

The first line is the marker every reader anchors on: upper case, at the start
of the line. A note that opens on `commit:` reads as no delivery at all.

`spec corrections:` counts what the item got WRONG about the world — a premise
it stated about the tree that you proved false. Not a design call, not a
residual, not a judgment. `decisions:` is the other half: the calls the item
left to you, numbered so a reviewer can accept or overrule each one. A call
nobody listed is a call nobody reviewed.

`not proven:` is never empty. The reviewer re-measures on the commit, so name
what you could not measure rather than letting a green imply it.
