#!/bin/sh
#
# The Claude Code pin, measured: the release this binary's controller adapter
# was measured against beside what `claude --version` answers.
#
# TWO readings, and the check reports which one it got:
#
#   holds   claude answered and its first line carries the supported version
#           as a whole token.
#   broken  claude did not run, or answered another version. The first is a
#           fleet whose controller cannot start a seat; the second one whose
#           controller still polls, on shapes nobody measured. Both exit 1, and
#           both name the line that installs the supported release.
#
# SUPPORTED IS A COPY of `fleet_core::supported::PINNED_CLAUDE_CODE`, because a
# script cannot read a Rust constant. The core suite reads this line and fails
# until the two agree, so a pin move that forgets it is refused at the suite.
#
# BUILT ON SHELL BUILTINS ALONE, as the runtime check is: the absence reading
# is taken with claude off PATH, and a check that needs its own tools on that
# same PATH cannot tell an absent claude from an absent grep.
#
# FLEET_CLAUDE_BIN names the binary where it is set — the seam the controller
# and `fleet start` read the agent through — and otherwise it is the first
# `claude` on PATH.
#
# TWO INSTALL LINES, because the upgrade one is the binary's own verb: a claude
# that answered takes `claude install <version>`, and a missing one takes the
# native installer with the same version.

set -u

SUPPORTED=2.1.280
INSTALL="claude install $SUPPORTED"
FRESH="curl -fsSL https://claude.ai/install.sh | bash -s $SUPPORTED"

bin=${FLEET_CLAUDE_BIN:-claude}

# Read from the command's own status, never through a pipe: a claude that is
# absent exits 127 here and a claude that is broken exits its own code, and
# both are reported as what they are.
reported=$("$bin" --version 2>/dev/null)
rc=$?
if [ "$rc" -ne 0 ] || [ -z "$reported" ]; then
	echo "claude-code-version: supported Claude Code $SUPPORTED"
	echo "claude-code-version: broken — \`$bin --version\` did not answer (exit $rc); no claude answers there, so the controller cannot start a seat. Install the supported release: $FRESH"
	exit 1
fi

# The first line only, which is where claude prints its own version.
IFS='
'
set -- $reported
unset IFS
first=$1

# A whole token, never a substring: 2.1.280 is not a match for 12.1.2800, and
# the leading-v form is the same version written the other way.
found=no
for token in $first; do
	if [ "$token" = "$SUPPORTED" ] || [ "$token" = "v$SUPPORTED" ]; then
		found=yes
	fi
done

echo "claude-code-version: supported Claude Code $SUPPORTED; \`$bin --version\` answers: $first"
if [ "$found" = yes ]; then
	echo "claude-code-version: holds"
	exit 0
fi

echo "claude-code-version: broken — this claude is not the supported $SUPPORTED, so the shapes the controller reads were not measured on it; the controller still runs. Install the supported release: $INSTALL"
exit 1
