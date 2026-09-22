#!/bin/sh
#
# All four guard classes' --check lines, in the order the hook document runs
# them: the binary's defaults' two, then the two this pack adds.
#
# ALL FOUR RUN whatever the first says, so one unconfigured target never hides
# another; the exit is the FIRST non-zero, so a caller reading the status alone
# learns that something is unconfigured and the lines above say which.

set -u

fleet guard shell-trap --check
first=$?

fleet guard record --check
second=$?

fleet guard release-ref --check
third=$?

fleet guard production-write --check
fourth=$?

for status in "$first" "$second" "$third" "$fourth"; do
	if [ "$status" -ne 0 ]; then
		exit "$status"
	fi
done
exit 0
