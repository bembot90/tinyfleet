#!/bin/sh
#
# The bd pin, measured: the release this binary's store was measured against
# beside what `bd version` answers.
#
# TWO readings, and the check reports which one it got:
#
#   holds   bd answered and its first line carries the pinned version as a
#           whole token.
#   broken  bd did not run, or answered another version. The first is a fleet
#           whose every store read fails at the exec; the second one whose
#           verbs still run, on answers nobody measured. Both exit 1, and both
#           point at beads' own installation docs, naming the pin to install.
#
# PINNED IS A COPY of `fleet_core::store::bd::PINNED_BD`, because a script cannot
# read a Rust constant. The core suite reads this line and fails until the two
# agree, so a pin move that forgets it is refused at the suite.
#
# BUILT ON SHELL BUILTINS ALONE, as the runtime check is: the absence reading
# is taken with bd off PATH, and a check that needs its own tools on that same
# PATH cannot tell an absent bd from an absent grep.
#
# FLEET_BD_BIN names the binary where it is set — the seam `fleet prime` reads
# the tracker through — and otherwise it is the first `bd` on PATH.
#
# THE INSTALL IS BEADS' OWN PAGE, not a command: bd installs several ways
# (Homebrew, npm, a script, go install with or without cgo) and which one fits
# is the machine's. The page is read at the pin's tag, so it describes the
# release named, and a pin move checks the page is still at that path there.

set -u

PINNED=1.3.0
DOCS="https://github.com/gastownhall/beads/blob/v$PINNED/docs/getting-started/installation.md"
INSTALL="Install the pinned bd $PINNED by beads' own instructions: $DOCS"

bin=${FLEET_BD_BIN:-bd}

# Read from the command's own status, never through a pipe: a bd that is
# absent exits 127 here and a bd that is broken exits its own code, and both
# are reported as what they are.
reported=$("$bin" version 2>/dev/null)
rc=$?
if [ "$rc" -ne 0 ] || [ -z "$reported" ]; then
	echo "bd-version: pinned bd $PINNED"
	echo "bd-version: broken — \`$bin version\` did not answer (exit $rc); no bd answers there, so every verb that reads the work graph fails at the exec. $INSTALL"
	exit 1
fi

# The first line only, which is where bd prints its own version.
IFS='
'
set -- $reported
unset IFS
first=$1

# A whole token, never a substring: 1.3.0 is not a match for 11.3.01, and the
# leading-v form is the same version written the other way.
found=no
for token in $first; do
	if [ "$token" = "$PINNED" ] || [ "$token" = "v$PINNED" ]; then
		found=yes
	fi
done

echo "bd-version: pinned bd $PINNED; \`$bin version\` answers: $first"
if [ "$found" = yes ]; then
	echo "bd-version: holds"
	exit 0
fi

echo "bd-version: broken — this bd is not the pinned $PINNED, so the answers fleet's store reads were not measured on it; the verbs still run. $INSTALL"
exit 1
