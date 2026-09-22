#!/usr/bin/env python3
"""tt-margin-ladder — run one nextest selection N times, recording the verdict
token, the arm's own wall and the box's 1-minute load per run.

    tt-margin-ladder.py <test-binary> <filterset> <N> <label>
    tt-margin-ladder.py --self-test

Written for the wait-margin ladders: a margin is varied in the source, the
binary is rebuilt, and the same selection is run N times at each step so a
step's verdict count can be read against the load it was taken at.

It scopes nextest to ONE test binary (`-p fleet-cli --test <binary>`) rather
than the workspace. Measured on this box: a `--workspace` run of a single
4.3 s arm costs 19 s wall, a `--test drive_causes` run of the same arm costs
4.8 s — the workspace form spends 14.7 s per run resolving 63 binaries, which
is three times the quantity being measured.

VERDICT COMES FROM NEXTEST'S OWN PER-TEST STATUS TOKEN. A TIMEOUT prints no
"failed" in its Summary and a LEAK prints "passed (1 leaky)", so a Summary grep
reports both as clean. LEAK matters here specifically: a wait margin's first
observable effect is a descendant still holding the inherited pipe when the arm
returns, which is a LEAK and never a FAIL.

A selection that matches nothing is nextest failing to run, not a verdict: it
raises rather than tabulating N greens. `n_tests` is recorded per run so a
reader can see the selection kept selecting.

The output of any run whose verdict is not PASS is saved beside the JSON as
`<label>-fail-run<N>.log`, because a red's panic text is what attributes it to
a margin rather than to the load.

Conventions: brain/harness/tools.md.
"""

import json
import os
import re
import statistics
import subprocess
import sys
import time

FLEET = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.environ.get("TT_LADDER_OUT", "/tmp/tt-margin-ladder")

ANSI = re.compile(r"\x1b\[[0-9;]*m")
# `    PASS [   4.302s] (1/1) fleet-cli::drive_causes unreadable_causes::the_arm`
# nextest right-aligns the index to the total's width, so both numbers tolerate
# leading spaces. A TERMINATING line prints `(───────)` and never matches.
TOKEN = re.compile(
    r"^\s*(PASS|FAIL|TIMEOUT|LEAK|SIGKILL)\s+\[\s*([\d.]+)s\]\s+\(\s*\d+/\d+\)\s+(\S+)\s+(.+?)\s*$"
)
SUMMARY = re.compile(r"Summary\s+\[\s*[\d.]+s\]\s+(\d+)\s+tests?\s+run:")


class NextestCouldNotRun(Exception):
    """The selection matched nothing, or no Summary line parsed at all."""


def strip_ansi(text):
    return ANSI.sub("", text)


def parse(text):
    """({arm name: (token, seconds)}, tests_run). Raises when nextest itself
    could not answer, so a zero-match selection never reads as N greens."""
    text = strip_ansi(text)
    m = SUMMARY.search(text)
    if not m:
        raise NextestCouldNotRun("no Summary line: nextest never ran")
    if int(m.group(1)) == 0:
        raise NextestCouldNotRun("the selection matched 0 tests")
    toks = {}
    for line in text.splitlines():
        t = TOKEN.match(line)
        if t:
            toks[t.group(4)] = (t.group(1), float(t.group(2)))
    return toks, int(m.group(1))


def one_run(binary, filt):
    p = subprocess.run(
        ["cargo", "nextest", "run", "-p", "fleet-cli", "--test", binary, "-E", filt],
        cwd=FLEET,
        capture_output=True,
        text=True,
    )
    raw = p.stdout + p.stderr
    toks, tests = parse(raw)
    return toks, tests, raw


SELF_TEST_PASS = (
    "\x1b[32;1m    Starting\x1b[0m \x1b[1m1\x1b[0m test across \x1b[1m1\x1b[0m binary\n"
    "\x1b[32;1m        PASS\x1b[0m [   4.302s] (1/1) \x1b[35;1mfleet-cli::drive_causes\x1b[0m \x1b[34;1mm::an_arm\x1b[0m\n"
    "\x1b[32;1m     Summary\x1b[0m [   4.305s] \x1b[1m1\x1b[0m test run: \x1b[1m1\x1b[0m passed\n"
)
SELF_TEST_LEAK = (
    "\x1b[33;1m        LEAK\x1b[0m [  16.846s] (1/5) \x1b[35;1mfleet-cli::drive_causes\x1b[0m \x1b[34;1mm::an_arm\x1b[0m\n"
    "\x1b[32;1m     Summary\x1b[0m [  16.9s] \x1b[1m5\x1b[0m tests run: \x1b[1m5\x1b[0m passed (\x1b[1m1\x1b[0m leaky)\n"
)
SELF_TEST_NOMATCH = "     Summary [   0.001s] 0 tests run: 0 passed, 925 skipped\n"


def self_test():
    ok = True
    toks, tests = parse(SELF_TEST_PASS)
    if toks != {"m::an_arm": ("PASS", 4.302)} or tests != 1:
        print(f"self-test: PASS fixture read as {toks} / {tests}")
        ok = False
    toks, tests = parse(SELF_TEST_LEAK)
    if toks.get("m::an_arm", (None,))[0] != "LEAK" or tests != 5:
        print(f"self-test: LEAK fixture read as {toks} / {tests}")
        ok = False
    try:
        parse(SELF_TEST_NOMATCH)
        print("self-test: a 0-match Summary parsed as a verdict")
        ok = False
    except NextestCouldNotRun:
        pass
    print("self-test: " + ("agreed on 3 fixtures" if ok else "DISAGREED"))
    return 0 if ok else 2


def main(argv):
    if argv[:1] == ["--self-test"]:
        return self_test()
    if len(argv) != 4:
        print(__doc__.splitlines()[2].strip())
        return 2
    binary, filt, n, label = argv[0], argv[1], int(argv[2]), argv[3]
    os.makedirs(OUT_DIR, exist_ok=True)
    rows = []
    for i in range(n):
        load = os.getloadavg()[0]
        started = time.time()
        toks, tests, raw = one_run(binary, filt)
        wall = time.time() - started
        bad = sorted(name for name, (tok, _) in toks.items() if tok != "PASS")
        verdict = toks[bad[0]][0] if bad else "PASS"
        arm_s = max(secs for _, secs in toks.values())
        if bad:
            with open(f"{OUT_DIR}/{label}-fail-run{i + 1}.log", "w") as f:
                f.write(raw)
        rows.append(
            {
                "run": i + 1,
                "load": round(load, 2),
                "verdict": verdict,
                "arm_s": arm_s,
                "wall_s": round(wall, 2),
                "failed": bad,
                "n_tests": tests,
            }
        )
        print(
            f"  {i + 1:>3}  {verdict:<8} arm={arm_s:>7.3f}s wall={wall:>6.2f}s "
            f"load={load:>6.2f}" + (" ARMS=" + ",".join(bad) if bad else ""),
            flush=True,
        )
    loads = [r["load"] for r in rows]
    arms = [r["arm_s"] for r in rows]
    counts = {}
    for r in rows:
        counts[r["verdict"]] = counts.get(r["verdict"], 0) + 1
    out = {
        "label": label,
        "binary": binary,
        "filter": filt,
        "n": n,
        "verdicts": counts,
        "load_min": min(loads),
        "load_median": round(statistics.median(loads), 2),
        "load_max": max(loads),
        "arm_min": round(min(arms), 3),
        "arm_median": round(statistics.median(arms), 3),
        "arm_max": round(max(arms), 3),
        "rows": rows,
    }
    print(f"{label}: {counts}  load {min(loads)}-{max(loads)} "
          f"(median {out['load_median']})  N={n}")
    with open(f"{OUT_DIR}/{label}.json", "w") as f:
        json.dump(out, f, indent=1)
    return 0 if counts.get("PASS", 0) == n else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
