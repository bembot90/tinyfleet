#!/bin/sh
#
# tmux against fleet's floor: the oldest release the controller's host was
# measured on beside what `tmux -V` answers, and whether fleet's own server is
# running on its socket.
#
# TWO READINGS, and the check reports which one it got:
#
#   holds   tmux answered with a release at or above the minimum — or with a
#           build that carries no release number to compare (`next-3.8`,
#           `master`), which is taken as recent and said so. The line names
#           whether a server is running on socket `fleet`, and with how many
#           sessions; either state passes.
#   broken  tmux did not run, or answered a release older than the minimum.
#           The first is a controller that can start no seat; the second one
#           that reads its seats on calls nobody measured. Both exit 1, and
#           both name the line that installs tmux.
#
# A TRAILING LETTER IS A PATCH LEVEL: 3.7b is above 3.7a, which is above 3.7.
# Anything else after the release number (a `-rc`) counts as no letter.
#
# MINIMUM IS A COPY of `fleet_core::supported::MINIMUM_TMUX`, because a script
# cannot read a Rust constant. The core suite reads this line and fails until
# the two agree, so a floor move that forgets it is refused at the suite.
#
# BUILT ON SHELL BUILTINS ALONE, as the Claude Code check is: the absence
# reading is taken with tmux off PATH, and a check that needs its own tools on
# that same PATH cannot tell an absent tmux from an absent grep.
#
# FLEET_TMUX_BIN names the binary where it is set — the seam the controller
# and `fleet start` read tmux through — and otherwise it is the first `tmux`
# on PATH.

set -u

MINIMUM=3.7b
INSTALL="brew install tmux on macOS, or the distribution's tmux package on Linux"

bin=${FLEET_TMUX_BIN:-tmux}

# Read from the command's own status, never through a pipe: a tmux that is
# absent exits 127 here and one that is broken exits its own code, and both are
# reported as what they are.
reported=$("$bin" -V 2>/dev/null)
rc=$?
if [ "$rc" -ne 0 ] || [ -z "$reported" ]; then
	echo "tmux-version: minimum tmux $MINIMUM"
	echo "tmux-version: broken — \`$bin -V\` did not answer (exit $rc); no tmux answers there, so the controller cannot start a seat's session. Install tmux $MINIMUM or later: $INSTALL"
	exit 1
fi

# The first line only, and its second token: `tmux 3.7b`.
IFS='
'
set -- $reported
unset IFS
first=$1
set -- $first
version=${2:-}

# A release as three numbers: major, minor, and the letter's place in the
# alphabet (0 for none). Sets `major`, `minor`, `letter` and `release`, which
# is `no` for a version that is not <digits>.<digits>[letter].
parse() {
	release=yes
	major=${1%%.*}
	rest=${1#*.}
	if [ "$rest" = "$1" ]; then
		release=no
	fi
	minor=${rest%%[!0-9]*}
	tail=${rest#"$minor"}
	case $major in '' | *[!0-9]*) release=no ;; esac
	case $minor in '' | *[!0-9]*) release=no ;; esac
	letter=0
	place=0
	for c in a b c d e f g h i j k l m n o p q r s t u v w x y z; do
		place=$((place + 1))
		if [ "$c" = "$tail" ]; then
			letter=$place
		fi
	done
}

parse "$MINIMUM"
min_major=$major
min_minor=$minor
min_letter=$letter

echo "tmux-version: minimum tmux $MINIMUM; \`$bin -V\` answers: $first"

parse "$version"
if [ "$release" = no ]; then
	note=" (\`$version\` carries no release number to compare, so it is taken as recent)"
else
	note=""
	at_least=no
	if [ "$major" -gt "$min_major" ]; then
		at_least=yes
	elif [ "$major" -eq "$min_major" ]; then
		if [ "$minor" -gt "$min_minor" ]; then
			at_least=yes
		elif [ "$minor" -eq "$min_minor" ] && [ "$letter" -ge "$min_letter" ]; then
			at_least=yes
		fi
	fi
	if [ "$at_least" = no ]; then
		echo "tmux-version: broken — tmux $version is older than the minimum $MINIMUM, so the calls the controller makes to it were not measured on it. Install tmux $MINIMUM or later: $INSTALL"
		exit 1
	fi
fi

# Fleet's own server, found where the controller finds it: the host runs tmux
# with a cleared environment, so no TMUX_TMPDIR moves the socket, and neither
# may one here.
unset TMUX_TMPDIR
sessions=$("$bin" -L fleet list-sessions -F '#{session_name}' 2>&1)
src=$?
if [ "$src" -eq 0 ]; then
	count=0
	IFS='
'
	for session in $sessions; do
		count=$((count + 1))
	done
	unset IFS
	server="a server is running on socket fleet with $count session(s)"
else
	case $sessions in
	*"no server running"* | *"error connecting to"*)
		server="no server is running on socket fleet"
		;;
	*)
		server="socket fleet did not answer: $sessions"
		;;
	esac
fi

echo "tmux-version: holds$note — $server"
exit 0
