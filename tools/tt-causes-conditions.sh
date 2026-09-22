#!/bin/sh
# The conditions the balk arm's flake rate needs, in one instrument, because a
# rate taken under one condition describes the box and not the assertion.
#
#   tools/tt-causes-conditions.sh paired <rounds> <tag>
#       two concurrent copies of the binary -- the condition that reds it
#   tools/tt-causes-conditions.sh solo <runs> <tag>
#       one copy, nothing beside it -- the control that rate is read against
#   tools/tt-causes-conditions.sh interleave <rounds> <tag> <before.rs> <after.rs>
#       both trees ALTERNATED inside one box-state sequence, one paired round
#       each, rebuilding between. A before and an after sampled hours apart
#       confound the tree with the load the box happened to carry; alternating
#       puts the same drift on both sides.
#
# Every mode drives binary(drive_causes) under the dolt test-server wrapper.
# Per run it prints the 1-minute load read at that run's start, the wrapper's
# exit status, whether THE ARM NAMED BELOW failed, and the names of any other
# arms that failed. An rc alone counts a red another arm carried; a verdict
# with no arm named cannot be attributed at all.
set -u
cd "$(dirname "$0")/.."

MODE=${1:-paired}
N=${2:-12}
TAG=${3:-x}
ARM=a_version_call_killed_before_its_fork_once_is_rescued_by_the_patience
FILT='binary(drive_causes)'
SUBJECT=cli/tests/drive_causes.rs

one() {
  log=$1
  load=$(uptime | sed 's/.*load averages: //' | cut -d' ' -f1)
  tools/dolt-test-server cargo nextest run --workspace -E "$FILT" > "$log" 2>&1
  rc=$?
  # FAIL lines only: the PASS line for an arm carries the same name.
  if grep -q "FAIL.*${ARM}" "$log"; then arm=ARM-RED; else arm=arm-green; fi
  others=$(grep '        FAIL' "$log" | sed 's/.*:://' | tr -d '\033' \
    | sed 's/\[[0-9;]*m//g' | grep -v "^${ARM}$" | sort -u | tr '\n' ',')
  echo "rc=$rc $arm load=$load others=[${others}] $log"
}

paired_rounds() {
  rounds=$1
  tag=$2
  side() {
    s=$1
    n=1
    while [ "$n" -le "$rounds" ]; do
      one "/tmp/tt_causes_${tag}_${s}_$n.log"
      n=$((n + 1))
    done
  }
  side A & pa=$!
  side B & pb=$!
  wait $pa
  wait $pb
}

case "$MODE" in
  paired) paired_rounds "$N" "$TAG" ;;
  solo)
    n=1
    while [ "$n" -le "$N" ]; do
      one "/tmp/tt_causes_${TAG}_solo_$n.log"
      n=$((n + 1))
    done
    ;;
  interleave)
    BEFORE=$4
    AFTER=$5
    keep=/tmp/tt_causes_subject_keep.rs
    cp "$SUBJECT" "$keep"
    r=1
    while [ "$r" -le "$N" ]; do
      for side in before after; do
        case "$side" in
          before) cp "$BEFORE" "$SUBJECT" ;;
          after) cp "$AFTER" "$SUBJECT" ;;
        esac
        cargo nextest run --workspace -E "$FILT" --no-run > /dev/null 2>&1
        echo "round $r $side"
        paired_rounds 1 "${TAG}_${side}_r$r"
      done
      r=$((r + 1))
    done
    cp "$keep" "$SUBJECT"
    ;;
  *) echo "mode is paired, solo or interleave" >&2; exit 64;;
esac
echo "done $MODE $TAG"
