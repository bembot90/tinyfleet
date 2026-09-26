#!/bin/sh
#
# The fleet-packs pin, measured: the tag this binary was checked against
# beside the version the lock pins each pack installed from fleet-packs at.
#
# THREE readings, and the check reports which one it got:
#
#   nothing installed  no line of the lock names the pinned source, so there is
#                      nothing to compare. A fleet created with --store none,
#                      or whose packs came from a checkout, is this one.
#   holds              every pack installed from the source is pinned at the
#                      supported tag.
#   broken             one or more is pinned at another version. Each is named
#                      with its version, and the check exits 1: fleet still
#                      runs them, on a contract nobody checked against this
#                      binary.
#
# A lock `fleet pack list` cannot read is none of the three: exit 3.
#
# SOURCE AND PINNED ARE COPIES of `fleet_core::supported::PINNED_PACKS_SOURCE`
# and `PINNED_PACKS`, because a script cannot read a Rust constant. The core
# suite reads these two lines and fails until the three agree, so a pin move
# that forgets them is refused at the suite.
#
# It READS THROUGH FLEET: `fleet pack list` is the one read, the lock as the
# binary parses it, and it writes nothing. The rows are split by the shell
# alone — a pack list's columns are words, and none of the three read here
# (name, source, version) holds a space — with globbing off, so a row is split
# and never expanded as a path.

set -u
set -f

SOURCE=https://github.com/bembot90/fleet-packs
PINNED=v0.1.0
FLEET=${FLEET_BIN:-fleet}

# Read from the command's own status, never through a pipe, so its exit is its
# own; its refusal is on this check's stderr, where the doctor prints it.
listed=$("$FLEET" pack list)
rc=$?
if [ "$rc" -ne 0 ]; then
	echo "fleet-packs-version: could not read the lock — \`fleet pack list\` exited $rc, so nothing was compared"
	exit 3
fi

echo "fleet-packs-version: supported fleet-packs $PINNED, from $SOURCE"

held=
moved=0
IFS='
'
set -- $listed
unset IFS
for row in "$@"; do
	set -- $row
	[ "$#" -ge 3 ] || continue
	name=$1
	source=$2
	version=$3
	case $source in
	"$SOURCE" | "$SOURCE"//*) ;;
	*) continue ;;
	esac
	if [ "$version" = "$PINNED" ]; then
		held="${held:+$held, }$name"
		continue
	fi
	moved=$((moved + 1))
	echo "fleet-packs-version: $name is pinned at $version, not the supported $PINNED — \`fleet pack remove $source\` and then \`fleet pack add $source --version $PINNED\` put the supported tag in its place"
done

if [ "$moved" -gt 0 ]; then
	echo "fleet-packs-version: broken — $moved pack(s) from fleet-packs are not at the supported $PINNED, so the contract they answer was not checked against this binary; fleet still runs them. Remove an importer before the pack it imports."
	exit 1
fi
if [ -z "$held" ]; then
	echo "fleet-packs-version: nothing installed from fleet-packs — no line of the lock names $SOURCE"
	exit 0
fi
echo "fleet-packs-version: holds — $held at $PINNED"
exit 0
