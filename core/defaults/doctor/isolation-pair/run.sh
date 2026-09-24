#!/bin/sh
#
# The provider's isolation pair, measured SESSION-FREE: the authentication status
# under a scratch configuration directory, twice, one variable apart.
#
# Two arms, because one arm cannot tell a pair that still isolates from a
# provider that has stopped scoping the credential at all:
#
#   the pair    the scratch directory with the credential knob DEFINED AND EMPTY
#               beside it — a spawned seat's own start — must answer logged IN.
#   the control  the same scratch directory with the knob UNSET must answer
#               logged OUT, which is what proves the knob is doing the work and
#               that this probe can see a failure at all.
#
# `holds` is both arms answering as above. Anything else is `broken` and names
# which arm disagrees. A pair that fails to log a seat in is a flight whose every
# first turn fails. A control that is logged in anyway is a provider whose
# credential does not derive from the directory at all, which takes the trap away
# and this probe's teeth with it.
#
# SESSION-FREE and read-only. It starts no session, writes nothing outside its
# own scratch directory and removes that on the way out, so it is safe on a
# machine that is running.

set -u

BIN=${FLEET_CLAUDE_BIN:-claude}

SCRATCH=$(mktemp -d 2>/dev/null) || {
	echo "isolation-pair: broken — no scratch directory could be made, so nothing was measured"
	exit 1
}
mkdir -p "$SCRATCH/pair" "$SCRATCH/control" || {
	echo "isolation-pair: broken — the scratch directory could not be populated"
	rm -rf "$SCRATCH"
	exit 1
}

# Each arm's status is read from ITS OWN command and never through a pipe: the
# body is kept so the logged-in reading is checked as well as the exit, because a
# release that exits 0 while logged out would otherwise read as a pair that holds.
CLAUDE_CONFIG_DIR="$SCRATCH/pair" CLAUDE_SECURESTORAGE_CONFIG_DIR= \
	"$BIN" auth status >"$SCRATCH/pair.out" 2>"$SCRATCH/pair.err"
pair_rc=$?

CLAUDE_CONFIG_DIR="$SCRATCH/control" "$BIN" auth status \
	>"$SCRATCH/control.out" 2>"$SCRATCH/control.err"
control_rc=$?

pair_in=no
grep -q '"loggedIn": *true' "$SCRATCH/pair.out" && pair_in=yes
control_in=no
grep -q '"loggedIn": *true' "$SCRATCH/control.out" && control_in=yes

echo "isolation-pair: the pair    — exit $pair_rc, loggedIn=$pair_in (expected exit 0, loggedIn=yes)"
echo "isolation-pair: the control — exit $control_rc, loggedIn=$control_in (expected non-zero, loggedIn=no)"

verdict=holds
if [ "$pair_rc" -ne 0 ] || [ "$pair_in" != yes ]; then
	verdict=broken
	echo "isolation-pair: the PAIR arm did not answer logged in, so a spawned seat's first turn would fail"
	sed -n '1,5p' "$SCRATCH/pair.err"
fi
if [ "$control_rc" -eq 0 ] || [ "$control_in" != no ]; then
	verdict=broken
	echo "isolation-pair: the CONTROL arm did not answer logged out, so the credential is no longer scoped by the directory and this probe proves nothing"
fi

echo "isolation-pair: $verdict"
rm -rf "$SCRATCH"
[ "$verdict" = holds ] || exit 1
exit 0
