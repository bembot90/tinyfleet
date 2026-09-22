#!/bin/sh
# tt-margin-load — hold the box at a flight-shaped 1-minute load, for the loaded
# leg of a wait-margin ladder.
#
#     tt-margin-load.sh start   # climbs into a 20-50 1-minute load band
#     tt-margin-load.sh stop
#
# It loops the ALREADY-BUILT fleet test binaries directly and never invokes
# cargo, because a cargo-based generator takes the target-dir build lock that
# the measurement's own `cargo nextest run` needs — the measurement would then
# spend its wall waiting on the lock and the recorded walls would be the lock's,
# not the arm's.
#
# The binaries are resolved once at start from `cargo nextest list`, which is the
# one cargo call here; it finishes before any load is applied.
#
# The two drive binaries are excluded so the generator never runs an arm being
# measured. A caller measuring different arms edits EXCLUDE.
#
# Measured on this box (10 cores): the 1-minute figure reaches 19 at 110 s and
# settles in the 20-50 band once warm, which takes two to three minutes — inside
# the 9-44 a real flight was observed at. It is the same KIND of load as
# a flight — fleet's own test processes, so CPU, process churn and temp-dir I/O
# together — and not a spin loop.

set -e
FLEET=$(cd "$(dirname "$0")/.." && pwd)
FLAG=/tmp/tt-margin-load.on
LIST=/tmp/tt-margin-load.binaries
EXCLUDE='drive_causes|drive_children'

case "$1" in
start)
  cd "$FLEET"
  cargo nextest list --workspace --message-format json 2>/dev/null > /tmp/tt-margin-load.json
  python3 - "$EXCLUDE" <<'PY' > "$LIST"
import json, re, sys
d = json.load(open("/tmp/tt-margin-load.json"))
skip = re.compile(sys.argv[1])
rows = []
for name, suite in d.get("rust-suites", {}).items():
    path = suite.get("binary-path")
    cases = suite.get("testcases") or suite.get("test-cases") or {}
    if path and not skip.search(path):
        rows.append((len(cases), path))
rows.sort(reverse=True)
for _, path in rows[:11]:
    print(path)
PY
  COUNT=$(wc -l < "$LIST" | tr -d ' ')
  if [ "$COUNT" -lt 1 ]; then echo "tt-margin-load: resolved 0 binaries — refusing"; exit 2; fi
  touch "$FLAG"
  while read -r B; do
    ( while [ -f "$FLAG" ]; do "$B" --test-threads 4 >/dev/null 2>&1; done ) &
  done < "$LIST"
  echo "tt-margin-load: $COUNT binaries looping; 1-minute load climbs for ~60s"
  ;;
stop)
  rm -f "$FLAG"
  sleep 10
  [ -f "$LIST" ] && while read -r B; do pkill -f "$B" || true; done < "$LIST"
  echo "tt-margin-load: stopped; 1-minute load decays for ~3 minutes"
  ;;
*)
  echo "usage: tt-margin-load.sh start|stop"
  exit 2
  ;;
esac
