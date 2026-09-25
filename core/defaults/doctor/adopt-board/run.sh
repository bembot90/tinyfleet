#!/bin/sh
#
# The board a project brought to fleet, read through fleet, in three classes:
#
#   1. names fleet owns that fail fleet's schema — a `fleet.orders` or a
#      `fleet.run` at a version or shape this binary does not read, `fleet:run`
#      on an item that is not a run's record, and any other `fleet.` key or
#      `fleet:` label, which fleet never writes;
#   2. the board's own conventions a person may want mapped onto fleet's — a
#      metadata key that is order-like, run-like or assignee-like and not
#      fleet's, and a run label that is not `fleet:run` — with the types and
#      labels the items carry listed for `[[core.flight.rules]]` to match;
#   3. fleet's old marker words in a person's notes or comments, which this
#      check CANNOT READ: fleet reads no notes, and no comment it did not write
#      (fleet-zlk), so the class is named and not counted. Fleet parses none of
#      those words any more; what a person wrote stays theirs.
#
# Each row gives a count and up to five example ids.
#
# THREE EXITS, the doctor's own: 0 when the items read carry nothing in the
# first two classes, 1 when they do and the rows above say where, 3 when the
# board could not be read. The type and label listing is a reading for the
# rules and never a finding by itself: every board has types.
#
# WHAT IS READ is what the store's set reads reach today: the ready set and the
# open items labelled `fleet:run`, through `fleet item list --json` — the
# transitional list, until the store's contract lists every item
# (fleet-0q4.5). An item neither ready nor carrying the run label is not read,
# and the first line says how many were.
#
# READ-ONLY. It writes nothing, anywhere.
#
# The JSON is read by the awk below and nothing else on PATH: a tokenizer over
# the document, because a pattern search over one line of JSON cannot tell a
# key from a string holding the same words. No apostrophe appears in the awk
# program, because the program is one single-quoted word of this script.

set -u

NAME=adopt-board
FLEET=${FLEET_BIN:-fleet}

# Each read from its own command and never through a pipe, so its exit is its
# own; its refusal is on this check's stderr, where the doctor prints it.
ready=$("$FLEET" item list --ready --json)
rc=$?
if [ "$rc" -ne 0 ]; then
	echo "$NAME: could not read the board — \`fleet item list --ready --json\` exited $rc, so nothing was scanned"
	exit 3
fi
runs=$("$FLEET" item list --label fleet:run --json)
rc=$?
if [ "$rc" -ne 0 ]; then
	echo "$NAME: could not read the board — \`fleet item list --label fleet:run --json\` exited $rc, so nothing was scanned"
	exit 3
fi

printf '%s\n%s\n' "$ready" "$runs" | awk -v name="$NAME" '
BEGIN { SEP = "\034"; bad = 0; nitems = 0; docs = 0 }

{
	docs++
	text = $0; len = length(text); pos = 1; ok = ""
	skip()
	if (substr(text, pos, 1) != "{") { bad = 1; next }
	value(0)
	if (ok != "true") bad = 1
}

function skip(   c) {
	while (pos <= len) {
		c = substr(text, pos, 1)
		if (c == " " || c == "\t" || c == "\r" || c == "\n") pos++
		else break
	}
}

function unescape(e) {
	if (e == "n" || e == "t" || e == "r" || e == "b" || e == "f") return " "
	if (e == "u") return "?"
	return e
}

# One JSON string, the cursor on its opening quote; answered decoded, the
# cursor past its closing one.
function str(   out, start, c) {
	pos++; out = ""; start = pos
	while (pos <= len) {
		c = substr(text, pos, 1)
		if (c == "\\") {
			out = out substr(text, start, pos - start) unescape(substr(text, pos + 1, 1))
			if (substr(text, pos + 1, 1) == "u") pos += 6
			else pos += 2
			start = pos
			continue
		}
		if (c == "\"") {
			out = out substr(text, start, pos - start)
			pos++
			return out
		}
		pos++
	}
	bad = 1
	return out
}

# One value at depth d, its path in seg[1..d]; seen() is told of each node,
# a container on its way in.
function value(d,   c, key, n, start) {
	if (bad) return
	skip()
	c = substr(text, pos, 1)
	if (c == "{") {
		kind[d] = "object"; seen(d, "object", "")
		pos++; skip()
		if (substr(text, pos, 1) == "}") { pos++; return }
		while (!bad) {
			skip()
			if (substr(text, pos, 1) != "\"") { bad = 1; return }
			key = str(); skip()
			if (substr(text, pos, 1) != ":") { bad = 1; return }
			pos++
			seg[d + 1] = key
			value(d + 1); skip()
			c = substr(text, pos, 1); pos++
			if (c == "}") return
			if (c != ",") { bad = 1; return }
		}
		return
	}
	if (c == "[") {
		kind[d] = "array"; seen(d, "array", "")
		pos++; skip(); n = 0
		if (substr(text, pos, 1) == "]") { pos++; return }
		while (!bad) {
			seg[d + 1] = n++
			value(d + 1); skip()
			c = substr(text, pos, 1); pos++
			if (c == "]") return
			if (c != ",") { bad = 1; return }
		}
		return
	}
	if (c == "\"") { seen(d, "string", str()); return }
	start = pos
	while (pos <= len && index("+-.0123456789Eaeflnrstu", substr(text, pos, 1)) > 0) pos++
	if (pos == start) { bad = 1; return }
	seen(d, "literal", substr(text, start, pos - start))
}

# What one node says about the item it sits in: data.items[i].<field>.
function seen(d, type, v,   it) {
	if (d == 1 && seg[1] == "ok") ok = v
	if (d < 3 || seg[1] != "data" || seg[2] != "items") return
	it = docs SEP seg[3]
	if (d == 3) { order[++nitems] = it; return }
	if (d == 4 && seg[4] == "id") id[it] = v
	if (d == 4 && seg[4] == "type") typ[it] = v
	if (d == 4 && seg[4] == "run" && type == "object") runp[it] = 1
	if (d == 5 && seg[4] == "labels" && type == "string") labels[it] = labels[it] SEP v
	if (d == 5 && seg[4] == "order" && seg[5] == "unreadable" && v == "true") orders_off[it] = 1
	if (d == 5 && seg[4] == "run" && seg[5] == "unreadable" && v == "true") run_off[it] = 1
	if (d == 5 && seg[4] == "metadata") meta[it] = meta[it] SEP seg[5]
	if (d == 6 && seg[4] == "metadata" && kind[5] == "object") held[it SEP seg[5]] = held[it SEP seg[5]] ", " seg[6]
}

# One item counted under row r, and the name it was counted by.
function mark(r, it, what,   k) {
	if (!((r SEP it) in in_row)) {
		in_row[r SEP it] = 1
		count[r]++
		if (count[r] <= 5) ids[r] = ids[r] (count[r] > 1 ? ", " : "") id[it]
		in_class[substr(r, 1, 1) SEP it] = 1
	}
	if (what != "" && !((r SEP what) in named)) {
		named[r SEP what] = 1
		names[r] = names[r] (names[r] == "" ? "" : ", ") what
	}
}

function tally(which, what) {
	if (!((which SEP what) in tallied)) {
		tallied[which SEP what] = 1
		tally_order[which] = tally_order[which] SEP what
	}
	tallies[which SEP what]++
}

# The last word of a key, after its last dot or colon, lowered.
function last_word(key,   n, parts) {
	n = split(key, parts, /[.:]/)
	return tolower(parts[n])
}

function row(r, what, noun,   line) {
	if (count[r] == 0) { print name ":    " what ": none"; return }
	line = name ":    " what ": " count[r] (count[r] == 1 ? " item — " : " items — ") ids[r]
	if (count[r] > 5) line = line " and " (count[r] - 5) " more"
	if (names[r] != "") line = line "; " noun " " names[r]
	print line
}

function tallied_line(which, what,   n, parts, i, line) {
	n = split(tally_order[which], parts, SEP)
	line = ""
	for (i = 2; i <= n; i++) line = line (line == "" ? "" : ", ") parts[i] " " tallies[which SEP parts[i]]
	if (line == "") line = "none"
	print name ":    " what ": " line
}

function count_class(c,   i, n) {
	n = 0
	for (i = 1; i <= unique; i++) if ((c SEP kept[i]) in in_class) n++
	return n
}

function items(n) { return n == 1 ? "1 item" : n " items" }

END {
	if (bad || docs != 2) {
		print name ": could not read the board — `fleet item list` answered a document this check does not read, so nothing was scanned"
		exit 3
	}

	# One row per item, the first time its id is read: an open run record is
	# in both reads.
	unique = 0
	for (i = 1; i <= nitems; i++) {
		it = order[i]
		if (id[it] == "" || (id[it] in read_already)) continue
		read_already[id[it]] = 1
		kept[++unique] = it
	}

	for (i = 1; i <= unique; i++) {
		it = kept[i]
		tally("type", typ[it] == "" ? "(none)" : typ[it])
		if (orders_off[it]) mark("1a", it, "")
		if (run_off[it]) mark("1b", it, "")

		n = split(labels[it], label, SEP)
		fleet_run = 0
		for (j = 2; j <= n; j++) {
			tally("label", label[j])
			if (label[j] == "fleet:run") fleet_run = 1
			else if (substr(label[j], 1, 6) == "fleet:") mark("1d", it, label[j])
			else if (label[j] == "run" || (length(label[j]) > 4 && substr(label[j], length(label[j]) - 3) == ":run")) mark("2d", it, label[j])
		}
		if (fleet_run && (!runp[it] || typ[it] != "task")) mark("1c", it, "")

		n = split(meta[it], key, SEP)
		for (j = 2; j <= n; j++) {
			if (key[j] == "fleet.orders" || key[j] == "fleet.run") continue
			if (substr(key[j], 1, 6) == "fleet.") { mark("1d", it, key[j]); continue }
			word = last_word(key[j])
			holds = held[it SEP key[j]]
			shown = key[j] (holds == "" ? "" : " (holding " substr(holds, 3) ")")
			if (word == "orders" || word == "order") mark("2a", it, shown)
			else if (word == "run" || word == "runs") mark("2b", it, shown)
			else if (word == "assignee" || word == "owner" || word == "reviewer" || word == "seat" || word == "assigned" || word == "assigned_to") mark("2c", it, shown)
		}
	}

	first = count_class("1"); second = count_class("2")
	for (i = 1; i <= unique; i++) if ((("1" SEP kept[i]) in in_class) || (("2" SEP kept[i]) in in_class)) found++

	print name ": read " items(unique) " through `fleet item list --json`: the ready set, and the open items labelled fleet:run"
	print name ": 1. names fleet owns, off the schema fleet reads — " items(first)
	row("1a", "fleet.orders at a version or shape fleet does not read", "")
	row("1b", "fleet.run at a version or shape fleet does not read", "")
	row("1c", "fleet:run on an item that is not a run record (a task carrying fleet.run)", "")
	row("1d", "a fleet. key or fleet: label fleet never writes", "names")
	print name ": 2. conventions of the board itself, to map onto the ones fleet reads — " items(second)
	row("2a", "an order-like metadata key that is not fleet.orders", "keys")
	row("2b", "a run-like metadata key that is not fleet.run", "keys")
	row("2c", "an assignee-like metadata key", "keys")
	row("2d", "a run label that is not fleet:run", "labels")
	tallied_line("type", "types on the items read, for [[core.flight.rules]] to match")
	tallied_line("label", "labels on the items read, for [[core.flight.rules]] to match")
	print name ": 3. the marker words fleet once wrote into notes (DELIVERED, RE-DELIVERED, ACCEPTED, RETURNED WITH FINDINGS, LANDED, PARKED, ANSWERED, QUESTION, orders given) — not readable through fleet: fleet reads no notes and no comment it did not write, so this class is not counted"

	if (found > 0) {
		print name ": found — " items(found) " to adopt; `fleet item show <id> --json` reads each, and the adopt skill walks the mapping"
		exit 1
	}
	if (unique == 0) print name ": nothing to adopt — no items read"
	else print name ": nothing to adopt — none of the " items(unique) " read carries a name fleet owns off its schema, or a convention to map"
	exit 0
}
'
