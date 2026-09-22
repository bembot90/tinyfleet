#!/usr/bin/env python3
"""start-cost-probe — how long from spawning a rig stub to its first act.

The figure `START_COST_MARGIN_MS` in fleet/cli/tests/drive/rig.rs is sized
against: fork-and-exec of a /bin/sh stub plus the stub's first write. The rig
already builds exactly that shape — `Rig::stub_adapter` inserts
`: > '<started>'` as the line after the shebang — so this probe reproduces it
and measures spawn-to-marker, N times, reporting the largest.

    fleet/tools/start-cost-probe.py --runs N [--label TEXT]

One row per run, then a summary line and one JSON line. The 1-minute load is
read before the first run and after the last, the way fleet-test's own
before-line does, so a row is readable against the box it was taken on.

The poll for the marker is a tight loop and not a sleep: the quantity is tens
of milliseconds on a quiet box and a 1 ms sleep would quantise it. Under load
that loop competes for the CPU it is measuring, so a reading taken there is an
OVER-estimate — which is the safe direction for a margin.
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


def load_1m():
    return os.getloadavg()[0]


def one_run(root, i):
    marker = os.path.join(root, f"started-{i}")
    script = os.path.join(root, f"stub-{i}")
    with open(script, "w") as h:
        h.write(f"#!/bin/sh\n: > '{marker}'\nexit 0\n")
    os.chmod(script, 0o755)
    t0 = time.perf_counter()
    p = subprocess.Popen([script])
    while not os.path.exists(marker):
        pass
    t1 = time.perf_counter()
    p.wait()
    return (t1 - t0) * 1000.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, required=True)
    ap.add_argument("--label", default="")
    args = ap.parse_args()

    before = load_1m()
    root = tempfile.mkdtemp(prefix="start-cost-")
    try:
        samples = [one_run(root, i) for i in range(args.runs)]
    finally:
        shutil.rmtree(root, ignore_errors=True)
    after = load_1m()

    samples_sorted = sorted(samples)
    largest = samples_sorted[-1]
    row = {
        "label": args.label,
        "runs": args.runs,
        "load_before": round(before, 2),
        "load_after": round(after, 2),
        "min_ms": round(samples_sorted[0], 2),
        "median_ms": round(statistics.median(samples), 2),
        "p95_ms": round(samples_sorted[int(0.95 * (args.runs - 1))], 2),
        "max_ms": round(largest, 2),
        "twice_max_ms": round(2 * largest, 2),
    }
    print(
        f"start-cost {args.label}: runs={args.runs} load {before:.2f} -> {after:.2f} | "
        f"min {row['min_ms']} ms  median {row['median_ms']} ms  "
        f"p95 {row['p95_ms']} ms  MAX {row['max_ms']} ms  "
        f"twice-max {row['twice_max_ms']} ms"
    )
    print(json.dumps(row))
    return 0


if __name__ == "__main__":
    sys.exit(main())
