#!/bin/sh
#
# The runtime contract this pack pins, measured against the deno this box
# resolves.
#
# WHERE THE BINARY IS LOOKED FOR, in order, and the check prints which one it
# used: PATH first, then the installer's own bin — DENO_INSTALL/bin when the
# installer's root variable is set, ~/.deno/bin otherwise — which no session's
# PATH carries on its own. A session that evals the toolchain export sees it
# appended and a bare session does not, and a reading that named the version
# without the directory could not tell those two apart.
#
# TWO readings, exit 0 and exit 1:
#
#   holds    the resolved binary answered and its first line carries the
#            pinned version as a whole token.
#   broken   no binary resolved anywhere, or the one that did answered another
#            version, or the manifest pins nothing — which is broken here and
#            not "nothing pinned", because pinning the runtime is what this
#            pack is for. Every reading exits 1 the same way: core execs the
#            bundle and run lines this manifest declares, and none of them was
#            measured against what is here.
#
# BUILT ON SHELL BUILTINS ALONE: the absence reading is taken with the runtime
# off PATH, and a check that needs grep on that same PATH cannot tell an absent
# runtime from an absent grep.
#
# FLEET_PACK_DIR names the pack whose manifest is read; without it the pack is
# this check's own, two directories up.

set -u

here=${0%/*}
PACK=${FLEET_PACK_DIR:-$here/../..}
MANIFEST=$PACK/pack.toml
INSTALLER_BIN=${DENO_INSTALL:-${HOME:-}/.deno}/bin

if [ ! -f "$MANIFEST" ]; then
	echo "deno-version: broken — no pack.toml at $PACK, so nothing was measured"
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
	echo "deno-version: broken — $MANIFEST declares no [runtime] table, and pinning the runtime is what this pack is for"
	exit 1
fi

echo "deno-version: pinned $pinned_name $pinned_version"

if resolved=$(command -v "$pinned_name" 2>/dev/null) && [ -n "$resolved" ]; then
	where="PATH"
elif [ -x "$INSTALLER_BIN/$pinned_name" ]; then
	resolved=$INSTALLER_BIN/$pinned_name
	where="the installer's bin, which no session PATH carries on its own"
else
	echo "deno-version: broken — $pinned_name is absent: not on PATH and not in the installer's bin $INSTALLER_BIN, so every bundle and run line this pack declares fails at the exec"
	exit 1
fi
echo "deno-version: $pinned_name resolved from $where ($resolved)"

# Read from the command's own status, never through a pipe: a binary that is
# broken exits its own code and is reported as what it is.
reported=$("$resolved" --version 2>/dev/null)
rc=$?
if [ "$rc" -ne 0 ] || [ -z "$reported" ]; then
	echo "deno-version: broken — \`$resolved --version\` did not answer (exit $rc), so every bundle and run line this pack declares fails at the exec"
	exit 1
fi

# The first line only: deno prints its own version there and v8's and
# typescript's below, and a dependency at the pinned number would otherwise
# read as a match.
IFS='
'
set -- $reported
unset IFS
first=$1

# A whole token, never a substring: 2.9.7 is not a match for 12.9.71, and the
# leading-v form is the same version written the other way.
found=no
for token in $first; do
	if [ "$token" = "$pinned_version" ] || [ "$token" = "v$pinned_version" ]; then
		found=yes
	fi
done

echo "deno-version: \`$pinned_name --version\` answers: $first"
if [ "$found" = yes ]; then
	echo "deno-version: holds"
	exit 0
fi

echo "deno-version: broken — the $pinned_name resolved is not the pinned $pinned_version, so the bundle and run lines core execs are not the ones this pack was measured against"
exit 1
