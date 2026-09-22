ACCEPTED {commit} — {reviewer}
item:    {item}
{size}
decisions: {decisions}

RETURNED WITH FINDINGS {commit} — {reviewer}
findings: {findings}
item:    {item}
{size}
{body}

The two shapes a review writes, and the review verb renders one of them whole.
A block is the paragraph its marker opens; what is written here under no marker
is this note, which teaches them and is never written to an item.

Both markers are chosen so that a reader anchoring on the last `DELIVERED` line
at column zero never reads a verdict as a delivery: neither begins with a
delivery's marker, and a verdict appended after one ends that delivery's region
rather than extending it.

`decisions:` is the walk — one `D<k> ACCEPT` or `D<k> OVERRULE` line per call
the delivery numbered, then the count accepted and overruled. A call the
delivery listed and the walk skipped is a call nobody reviewed.

`findings:` is the first line after the marker, and it counts the numbered
findings the return carries. A return that numbers nothing is a question, and
it goes back as one rather than as a verdict.
