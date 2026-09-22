#!/bin/sh
#
# The runtime contract, measured: what the pack pins in [runtime] against what
# answers on PATH.
#
# THREE readings, and the check reports which one it got:
#
#   nothing pinned  the pack declares no [runtime] table, so there is nothing to
#                   measure and the entry is green. This is every pack that
#                   carries no workflows.
#   holds           the pinned binary answered and its first line carries the
#                   pinned version as a whole token.
#   broken          the binary did not run, or answered another version. Both
#                   are the same failure downstream — core execs a command line
#                   it was never measured against — so both exit 1.
#
# BUILT ON SHELL BUILTINS ALONE: no grep, no sed, no cut, no head. The absence
# reading is taken with the runtime off PATH, and a check that needs its own
# tools on that same PATH cannot tell an absent runtime from an absent grep.
#
# FLEET_PACK_DIR names the pack whose manifest is read; without it the pack is
# this check's own, two directories up.

set -u

here=${0%/*}
PACK=${FLEET_PACK_DIR:-$here/../..}
MANIFEST=$PACK/pack.toml

if [ ! -f "$MANIFEST" ]; then
	echo "runtime-version: broken — no pack.toml at $PACK, so nothing was measured"
	exit 1
fi

# A hand-rolled reader for two keys of one table, because the alternative is a
# TOML parser on PATH and PATH is the thing under test.
trim() {
	value=$1
	while :; do
		case $value in
		' '*) value=${value# } ;;
		*' ') value=${value% } ;;
		'	'*) value=${value#	} ;;
		*) break ;;
		esac
	done
	case $value in
	'"'*'"') value=${value#\"}; value=${value%\"} ;;
	esac
}

table=
pinned_name=
pinned_version=
while IFS= read -r line || [ -n "$line" ]; do
	case $line in
	'['*']') table=$line; continue ;;
	esac
	[ "$table" = "[runtime]" ] || continue
	case $line in
	*=*) ;;
	*) continue ;;
	esac
	trim "${line%%=*}"
	key=$value
	trim "${line#*=}"
	case $key in
	name) pinned_name=$value ;;
	version) pinned_version=$value ;;
	esac
done <"$MANIFEST"

if [ -z "$pinned_name" ] || [ -z "$pinned_version" ]; then
	echo "runtime-version: nothing pinned — $MANIFEST declares no [runtime] table, so this pack carries no workflows for core to bundle or run"
	exit 0
fi

# Read from the command's own status, never through a pipe: a runtime that is
# absent exits 127 here and a runtime that is broken exits its own code, and
# both are reported as what they are.
reported=$("$pinned_name" --version 2>/dev/null)
rc=$?
if [ "$rc" -ne 0 ] || [ -z "$reported" ]; then
	echo "runtime-version: pinned $pinned_name $pinned_version"
	echo "runtime-version: broken — \`$pinned_name --version\` did not answer (exit $rc); the pinned runtime is not on PATH, so every bundle and run line this pack declares fails at the exec"
	exit 1
fi

# The first line only: every runtime measured prints its own version there and
# its dependencies' versions below, and a dependency at the pinned number would
# otherwise read as a match.
IFS='
'
set -- $reported
unset IFS
first=$1

# A whole token, never a substring: 2.4.5 is not a match for 12.4.51, and the
# leading-v form is the same version written the other way.
found=no
for token in $first; do
	if [ "$token" = "$pinned_version" ] || [ "$token" = "v$pinned_version" ]; then
		found=yes
	fi
done

echo "runtime-version: pinned $pinned_name $pinned_version; \`$pinned_name --version\` answers: $first"
if [ "$found" = yes ]; then
	echo "runtime-version: holds"
	exit 0
fi

echo "runtime-version: broken — the $pinned_name on PATH is not the pinned $pinned_version, so the bundle and run lines core execs are not the ones this pack was measured against"
exit 1
