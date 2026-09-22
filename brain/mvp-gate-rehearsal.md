# MVP gate rehearsal

The runbook a person follows to prove the fleet can fly one real board flight
end to end, with the controller killed once mid-flight and brought back. Run it
once, top to bottom; a step with no read-back is not done.

## 1. What it proves

One real flight — a run of the takeoff workflow over `fleet run` — flown by the
workflow's own steps: a seat spawned per item, each delivery waited on, each
verdict read at a gate, each accepted item landed on the trunk, the report
written, the run closed and its seats retired. And that it survives a SIGKILL of
its controller after an item's spawn and before that item's delivery: the record
must then show the re-run replayed to the same waiting step, no seat spawned
twice, no closed step restarted, every landed sha an ancestor of the trunk.

## 2. Preconditions

### A machine with no fleet yet: three acts

The fleet and the project it flies, in the order a person types them. The
second act names the fleet with `--fleet` because nothing has written the seat
list yet; **the seats are rendered by the start**, so registration comes first
and one start is enough.

```sh
cd <the fleet's own directory>       # act 1
fleet create --embedded --agent claude_code
# then add one [seats.<name>] table per seat to its fleet.toml

cd <the project>                     # act 2
fleet create --standalone --agent claude_code --fleet <the fleet's own directory>

fleet start                          # act 3 — renders the registered seats
```

A machine whose seat list already names a live `fleet.toml` — `config.json`
under the machine directory, `~/.fleet` unless `FLEET_DIR`, `FLEET_HOME` or
`XDG_STATE_HOME` moves it — skips act 1 and drops `--fleet` from act 2.

### Then, every run

```sh
fleet --version   # the binary under test
fleet pack list   # the packs the run resolves through, pinned; an empty lock refuses
fleet status      # the policy in force; no STALE, no GRANT PENDING, no exit 5
git fetch origin  # the trunk, so RB4 has something to read
```

And a flight a person composed: an ordered list of item ids, each ready in the
store. That list is the run's `items` input, and nothing here invents it.

## 3. The run

### S1 — install or refresh the packs

`fleet pack` carries `check`, `add`, `remove` and `list`. Read back one row per
pack with its commit and fetched stamp; an empty lock prints the header alone.
```sh
fleet pack add <source> --version <tag-or-branch-or-sha:...>
fleet pack list
```

### S2 — confirm the policy the verbs read

Read back the projection's age without `STALE`, the roster, and the rules table
of the policy in force; `fleet status` writes nothing.
```sh
fleet status
```

### S3 — bring the controller up in the foreground

From the project root, for the sitting only. Read back one fresh
`controller.started`: `fleet start` confirms off the stream, not by the load
command's exit.
```sh
nohup fleet start --foreground > /tmp/fleet-controller.log 2>&1 &
fleet event tail --json --type controller.started --since 1 | tail -1
```

If the host runs another supervisor over the same seat sessions, unload it for
the controller's window and reload it at the end (S9). Two supervisors over one
set of sessions is the revive trap: an idle session reads pid-less to the second
and gets revived, one wake per idle session per poll window.

### S4 — launch the run

`--input` is `KEY=VALUE`, repeatable, splitting on the first `=` only, which is
why `policy=review=gate,width=1` arrives whole; the pairs takeoff reads are
`review=gate` (default) or `review=accept`, and `width=<n>`. Read back
`run.started` with the run id — called `<run-id>` below — then the first
`session.spawned`.
```sh
fleet run takeoff --input items=<item-a>,<item-b>,<item-c> \
  --input policy=review=gate,width=1 --by <reviewer>
fleet event tail --json --since 1 \
  | jq -c 'select(.data.kind|test("^(run.started|session.spawned)$")) | [.data.seq,.data.ts,.data.kind,.data.actor]'
```

### S5 — the kill

After `session.spawned` for an item and before that item's `item.delivered`:
```sh
date -u
pgrep -f 'fleet start --foreground'
kill -9 <controller-pid>
```

A SIGKILL writes no `controller.stopped` — the loop owes that line only on a
deliberate stop. The witness is the `date -u` stamp plus the **absence** of a
`controller.stopped` between the two `controller.started` lines around it, which
is what RB3 reads.

### S6 — restart

Read back, in order: `controller.started`; `session.adopted` for the seat
spawned before the kill, adopted by session id and not respawned, so its pid is
unchanged; `run.started` for `<run-id>` again; `step.started` for the same
`until delivered <item-id>` step; `run.waiting` again. The poll re-runs the
bundle already in the run directory once the stream has moved past the position
the last `run.waiting` recorded — once per poll at most, never a fresh bundle.
```sh
nohup fleet start --foreground > /tmp/fleet-controller.log 2>&1 &
fleet event tail --json --since 1 \
  | jq -c 'select(.data.kind|test("^(controller|session|run|step)\\.")) | [.data.seq,.data.kind,(.data.payload.step // .data.actor)]' | tail -30
```

### S7 — the delivery, the gate, the landing

The seat delivers; the `until` step closes; the run parks on its gate
(`item.parked`, the question written to `gates/<k>.md` under the run directory)
and exits waiting. Takeoff raises that gate on the **run's own record item**, so
the item argument to `answer` is the run id; `A` is accept and land, `B` is
return to the builder, `--text` carries what the options did not. Read back
`item.reviewed`, `gate.read` green with rc 0 on the project's suite, and
`item.landed` with its sha, taken off the push's own range line.
```sh
fleet review <item-id> --show
fleet answer <run-id> A --by <reviewer>
fleet event tail --json --since 1 \
  | jq -c 'select(.data.kind|test("^(item|gate)\\.")) | [.data.seq,.data.kind,.data.payload]'
```

### S8 — repeat, then let the run close itself

Repeat S7 once per item. On the last one the workflow runs its own last two
steps, `report` and `tick`, and exits 0. Read back `run.closed` for `<run-id>`,
one `session.retired` per seat the run spawned, one `run.cleaned` with the count
— a run that spawned nothing is still cleaned, with a measured zero.
```sh
fleet event tail --json --since 1 \
  | jq -c 'select(.data.kind|test("^(run.closed|session.retired|run.cleaned)$")) | [.data.seq,.data.kind,.data.payload]'
```

### S9 — stop the controller

A foreground controller arms SIGINT and SIGTERM, finishes its tick and writes
the line; run as the service instead, `fleet stop` unloads it and confirms the
same way. Seats are untouched either way. Reload the host's other supervisor now
if S3 unloaded one.
```sh
kill -TERM <controller-pid>
fleet event tail --json --type controller.stopped --since 1 | tail -1
```

### S10 — tick the board

The run wrote `board-tick.md` and `report.md` into `$FLEET_RUN_DIR`
(`runs/<run-id>/` under the machine directory). Tick the board rows from
`board-tick.md` — one ticked row per landed item with its sha — and write
`<run-id>` in the flight's heading.
```sh
cat runs/<run-id>/board-tick.md runs/<run-id>/report.md
```

## 4. The four read-backs

All four read `fleet event tail --json`, whose envelope is
`{"ok":true,"verb":"event tail","data":{"id","seq","ts","kind","actor","payload"}}`
— the stored line's `type` arrives as `data.kind`. `--since` is a cursor and
**exclusive**, and a tail with no `--since` prints only the last 50 lines, which
is why every block below passes one.

**RB1 — the run's record.** Expect `run.started`, one `run.waiting` per wait,
exactly one `run.closed`, and no `run.failed` or `run.could_not_tell`.
```sh
fleet event tail --json --since 1 \
  | jq -c 'select(.data.payload.run=="<run-id>") | select(.data.kind|test("^run\\.")) | [.data.seq,.data.ts,.data.kind]'
```

**RB2 — exactly one spawn per item.** Counted from a file, never from `$?`, and
compared with the item count. The count is per item across the **whole stream**,
not per run: a delivered item retaken by a later run is not re-spawned, so a
second spawn line for one item is the failure this read-back catches.
```sh
fleet event tail --json --since 1 \
  | jq -c 'select(.data.kind=="session.spawned") | [.data.seq,.data.actor,(.data.payload.first_turn|split("\n")[0])]' > spawned.txt
wc -l < spawned.txt
```

**RB3 — the kill fell between a spawn and its delivery.** In seq order: the
item's `session.spawned` first; the S5 stamp after it and before that item's
`item.delivered`; and no `controller.stopped` between the two
`controller.started` lines bracketing the stamp. That absence is the proof it
died rather than stopped.
```sh
fleet event tail --json --since 1 \
  | jq -c 'select(.data.kind|test("^(session.spawned|item.delivered|controller.started|controller.stopped)$")) | [.data.seq,.data.ts,.data.kind,.data.actor]'
```

**RB4 — every landing is on the trunk.** Each `rc=` is read directly off its own
`merge-base` call, and every one must be 0.
```sh
git fetch origin
for s in <sha-a> <sha-b> <sha-c>; do
  git merge-base --is-ancestor "$s" origin/main
  echo "$s rc=$?"
done
```

## 5. Replay semantics

A waiting step re-emits `step.started` on every re-run because it never closed.
`until delivered <item>` and a gate are both waiting steps, so seeing their
`step.started` twice is what a resumed run looks like. A closed step never
re-starts: the SDK replays up to the first step with no `step.closed` and
resumes there, and `step.failed` is the only thing that puts a closed step back
in play. So the check is **"no closed step restarted"**, not "no step number
started twice": at most one `step.closed` per step, and no `step.started` above
it.

## 6. Known costs

**A foreground controller revives idle sessions.** An idle session reading
pid-less to its poll comes back as a revive, one wake each per poll window and
each a real turn. Run it only for the minutes a step is owed, and unload any
second supervisor first (S3).

**The whole-suite gate reds timing arms under load.** Landing runs the project's
suite while the run's seats are still working, and timing-sensitive arms fail on
that concurrency rather than on their merits, so the landing gate for this
rehearsal is the touched suites.
