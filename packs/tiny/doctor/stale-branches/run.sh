#!/bin/sh
#
# Remote branches whose item is closed, run from the project root.
#
# IT PRINTS THE DELETE LINE AND NEVER RUNS IT. A ref may still hold work that
# never landed, and the only reader who can tell is a person.
#
# Three exits, and the order between them is deliberate: 3 when something could
# not be READ — the project file, the remote, the work graph, or one item's row —
# and it wins over the other two even when the sweep also found branches, because
# a partial sweep is not a clean one and the difference is invisible in a status.
# 1 when every source answered and at least one branch is stale. 0 when every
# source answered and none is.

set -u

project_file=''
for candidate in .fleet/project.toml fleet.toml; do
	if [ -f "$candidate" ]; then
		project_file=$candidate
		break
	fi
done
if [ -z "$project_file" ]; then
	echo >&2 "stale-branches: no project file here — the id grammar cannot be read"
	exit 3
fi

prefix=$(sed -n 's/^[[:space:]]*item_prefix[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$project_file")
if [ -z "$prefix" ]; then
	echo >&2 "stale-branches: $project_file names no item prefix — a branch cannot be matched to an item"
	exit 3
fi

if ! command -v bd >/dev/null 2>&1; then
	echo >&2 "stale-branches: no work graph on PATH — no item's state can be read"
	exit 3
fi

trunk=$(git symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null)
trunk=${trunk#origin/}
if [ -z "$trunk" ]; then
	trunk=main
fi

heads=$(git ls-remote --heads origin 2>/dev/null)
status=$?
if [ "$status" -ne 0 ]; then
	echo >&2 "stale-branches: origin could not be read (exit $status)"
	exit 3
fi

findings=0
unreadable=0

# A here-document rather than a pipe: a loop on the right of a pipe runs in a
# subshell, and the two counters below would be lost when it ended.
while read -r sha ref; do
	[ -n "$sha" ] || continue
	case "$ref" in refs/heads/*) ;; *) continue ;; esac
	branch=${ref#refs/heads/}
	[ "$branch" != "$trunk" ] || continue

	item=$(printf '%s\n' "$branch" | grep -o -E "$prefix-[a-z0-9]+(\.[0-9]+)*" | head -1)
	[ -n "$item" ] || continue

	row=$(bd show "$item" --json 2>/dev/null)
	status=$?
	if [ "$status" -ne 0 ] || [ -z "$row" ]; then
		echo >&2 "stale-branches: the work graph could not answer for $item ($branch)"
		unreadable=1
		continue
	fi

	if printf '%s' "$row" | grep -q '"status"[[:space:]]*:[[:space:]]*"closed"'; then
		findings=$((findings + 1))
		echo "stale-branches: $branch — $item is closed; delete it with: git push origin --delete $branch"
	fi
done <<STALE_BRANCHES_HEADS
$heads
STALE_BRANCHES_HEADS

if [ "$unreadable" -ne 0 ]; then
	exit 3
fi
if [ "$findings" -ne 0 ]; then
	exit 1
fi
exit 0
