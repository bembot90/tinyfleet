#!/bin/sh
#
# Both guard classes' --check lines, in the order the hook document runs them.
#
# BOTH RUN whatever the first says, so one unconfigured target never hides
# another; the exit is the FIRST non-zero, so a caller reading the status alone
# learns that something is unconfigured and the lines above say which.

set -u

fleet guard shell-trap --check
first=$?

fleet guard record --check
second=$?

if [ "$first" -ne 0 ]; then
	exit "$first"
fi
exit "$second"
