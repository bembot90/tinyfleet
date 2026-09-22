#!/usr/bin/env python3
"""descendant-overshoot-probe — the quantity START_COST_MARGIN_MS covers.

The four sites the margin is added at (fleet/cli/tests/drive_causes.rs) each
wait `DESCENDANT_SECONDS * 1000 + START_COST_MARGIN_MS` after witnessing a
forked descendant's start marker, and then assert its finish marker is ABSENT.
So the margin has to cover exactly one thing: how far PAST its own nominal
sleep a descendant that was left alive writes its marker. Anything less and a
descendant the kill missed is still asleep when the arm reads, which is a green
that means nothing; anything more is suite time spent on nothing.

This probe reproduces that descendant verbatim — the same subshell, the same
`trap '' TERM`, the same two markers — leaves it ALIVE, and reports
`t(finish) - t(start) - sleep`, which is the overshoot. Not the start cost of a
fresh stub: that is the PARENT's cost and the arms pay it before the witness,
not after it.

    fleet/tools/descendant-overshoot-probe.py --runs N [--sleep S] [--label T]

One summary line and one JSON line. The 1-minute load is read before the first
run and after the last.
"""
import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time


def one_run(root, i, secs):
    started = os.path.join(root, f"started-{i}")
    finished = os.path.join(root, f"finished-{i}")
    script = os.path.join(root, f"stub-{i}")
    with open(script, "w") as h:
        h.write(
            "#!/bin/sh\n"
            f"( : > '{started}'; trap '' TERM; sleep {secs}; : > '{finished}' ) &\n"
            "exit 0\n"
        )
    os.chmod(script, 0o755)
    p = subprocess.Popen([script])
    p.wait()
    while not os.path.exists(started):
        pass
    t_start = time.perf_counter()
    while not os.path.exists(finished):
        time.sleep(0.002)
    t_finish = time.perf_counter()
    return (t_finish - t_start - secs) * 1000.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, required=True)
    ap.add_argument("--sleep", type=int, default=5)
    ap.add_argument("--label", default="")
    args = ap.parse_args()

    before = os.getloadavg()[0]
    root = tempfile.mkdtemp(prefix="overshoot-")
    try:
        samples = [one_run(root, i, args.sleep) for i in range(args.runs)]
    finally:
        shutil.rmtree(root, ignore_errors=True)
    after = os.getloadavg()[0]

    s = sorted(samples)
    row = {
        "label": args.label,
        "runs": args.runs,
        "sleep_s": args.sleep,
        "load_before": round(before, 2),
        "load_after": round(after, 2),
        "min_ms": round(s[0], 2),
        "median_ms": round(statistics.median(samples), 2),
        "max_ms": round(s[-1], 2),
        "twice_max_ms": round(2 * s[-1], 2),
    }
    print(
        f"overshoot {args.label}: runs={args.runs} sleep={args.sleep}s "
        f"load {before:.2f} -> {after:.2f} | min {row['min_ms']} ms  "
        f"median {row['median_ms']} ms  MAX {row['max_ms']} ms  "
        f"twice-max {row['twice_max_ms']} ms"
    )
    print(json.dumps(row))
    return 0


if __name__ == "__main__":
    sys.exit(main())
