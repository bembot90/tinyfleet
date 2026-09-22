#!/bin/sh
#
# Which copy of the binary this shell resolves, and whether it answers.
#
# TWO ANSWERS, AND BOTH ARE PRINTED. The path alone does not say the file runs,
# and a version alone does not say which file produced it — so the exit is 0 only
# when the name resolves AND the resolved file answers, and 1 when either does
# not. A seat that read a version out of one checkout and reported it against
# another has measured nothing at all.

set -u

resolved=$(command -v fleet 2>/dev/null) || resolved=''
if [ -z "$resolved" ]; then
	echo "verbs-on-path: no fleet on PATH — nothing in this shell resolves the verbs"
	exit 1
fi
echo "verbs-on-path: $resolved"

version=$(fleet --version 2>/dev/null)
status=$?
if [ "$status" -ne 0 ] || [ -z "$version" ]; then
	echo "verbs-on-path: $resolved does not answer its version flag (exit $status)"
	exit 1
fi
echo "verbs-on-path: $version"
exit 0
