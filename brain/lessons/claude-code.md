# Lessons — Claude Code

What fleet knows about the Claude Code substrate, measured rather than read.

The controller is built on measurements taken somewhere else; the PRD it was
built from, archived under `../archive/prds/`, marks each requirement that
rests on one **R**. This file is where those measurements live as fleet's own
knowledge, so a builder changing the behaviour one fences can cite the fact
instead of trusting the letter beside it.

**The entries the claude-code pack rests on live in the pack**, beside its
fixtures: A1, A5, A9, A11, A12, A15, B1–B5, B7, B8, B10, C1–C4, D3, D5, D6 and
D8, in the adapter's
[`LESSONS.md`](https://github.com/bembot90/fleet-packs/blob/v0.2.0/adapters/agent/claude-code/adapters/agent/claude-code/LESSONS.md)
at fleet-packs v0.2.0, the first tag carrying it and the one fleet pins from
flight 14 on (`core/src/supported.rs` `PINNED_PACKS`). They moved there with
their ids, slugs, Versions and Dates (fleet-jymr.6, ruling 15). What stays here
is two kinds of entry. The daemon-era entries flight 11 retired, each marked
with the row that retired it and otherwise left as written, as history. And
the facts about the host and fleet's own loop — A13, C5, D1, D4 and D7 — which
were measured on a Claude Code fleet but are core's, kept live.

**Every fact here is version-scoped.** Claude Code's session lifecycle is
observed behaviour, not a published contract; a fact without the release it
was measured on is a rumour. Each entry therefore carries **Version** and
**Date**. Where the source of a fact recorded no release or no date, the field
says so — an unrecorded provenance is a fact about the fact, and inventing one
would be worse than carrying it.

**Every fact owes a test.** The **Test** field names a fixture test,
`lessons::<slug>`, in snake_case. A core entry's test is written under that
exact name in a `lessons` module of the crate that lands its code; a retired
entry's test was deleted with the behaviour it held, and its name stays here
as history; a moved entry's test is the pack's. `## Test inventory` at the foot
of this file says, for every name, which of the three it is. A fact with
nothing to exercise says `none — <why>`.

**Reading the entries.** *Fact* is the measurement in one paragraph. *Implies*
names the archived PRD's requirement the fact fed, by R-number, and says what
the requirement owed it.

---

## A. Session lifecycle

### A2. A row with no pid is two different states

- **Retired by:** fleet-rge6.3 (2026-09-26). A seat's session is an
  interactive one in fleet's own tmux pane, and its row always carries the
  pane's pid (B10); a session the host holds with no row yet is starting by
  the pane's age, and one that ended is a dead pane. No pid-less row is
  read any more. Kept here as history.
- **Fact:** A roster row carrying no `pid` means either the session has ENDED
  or it has not finished STARTING, and `state` is the only field that separates
  them. The newborn window was measured at 277–737 ms across 4 spawns of 4: no
  pid, no status, `state` "working". A resting session in the same window reads
  pid-less with `state: done`, 5 of 5, so the split holds at exactly the field
  the design reads. Because the phenomenon is sub-second and a poll is seconds,
  the grace a controller allows a newborn should be far larger than the window —
  the cost of generosity is a few polls of delay before a genuinely stuck row is
  recovered, and the cost of tightness is starting a second session over a slow
  start, which is the defect itself.
- **Version:** Claude Code 2.1.234, re-read on 2.1.247.
- **Date:** 2026-08-14 (first), 2026-08-27 (re-read).
- **Implies:** R10 — the discriminator for a pid-less non-newborn row. R12 —
  transient rows never receive a session-creating verdict.
- **Test:** `lessons::pid_null_is_two_states`

### A3. No end of life can be read off the roster, and `done` least of all

- **Retired by:** fleet-rge6.3 (2026-09-26). The end is the host's: a
  dead pane holding its exit status, dated by the first poll that reads it
  dead or by the transcript (`session.ended`). The state vocabulary is
  background-only and nothing reads it; the deliberate-end event is still
  what tells a goodbye from a crash, because a dead pane cannot. Kept here
  as history.
- **Fact:** Hibernation was absent from the roster on 2.1.233 and returned in
  2.1.234, and a hibernated session and a deliberately stopped one are
  identical across every roster field. Nothing the agent reports separates
  "this seat asked to stop" from "the host put this seat to sleep". A
  controller that wants the difference has to hold it itself, as its own
  record of what it was told, and never try to read it off the roster. **The
  2.1.261 re-read widens it, and the widening is the load-bearing half.** A
  background row's state vocabulary is five words — `working`, `blocked`,
  `done`, `stopped`, `failed` — and its fields are id, sessionId, cwd, kind,
  name, pid, startedAt, state, status, with `status` present only while a pid
  is (`idle`, `busy`). A LIVE IDLE session reads pid present, `state: done`,
  `status: idle` (it can also read `state: blocked` with `status: idle`); a
  busy one reads `state: working`, `status: busy`. Pid-less, `state: done`,
  no status is what a hibernated session reads, what a session stopped from
  idle reads, and what a SIGKILLed one reads within 100 ms — three different
  fates, one reading, none of them separable. `stopped` and `failed` are
  reached only from a NON-IDLE prior state: a stop from `blocked` leaves
  `stopped`, a stop mid-start leaves `failed`, and a SIGTERM from `blocked`
  leaves the row pid-less for about 17 s before the daemon respawns it
  (A4's shape). So `done` is live-idle, hibernated and ended alike — reading
  it as an end marks every idle seat in the fleet as finished — and the only
  ends the roster can NAME are the two that idle never reaches.
- **Version:** Claude Code 2.1.233 (absent), 2.1.234 (returned), 2.1.261
  (re-read, three scratch background sessions over a whole life).
- **Date:** 2026-08-11 (absent), 2026-08-13 (returned), 2026-09-21 (re-read).
- **Implies:** R10 — the deliberate-end **event** plus context is the
  discriminator precisely because the roster cannot be one; R20 — which is why
  the seat lifecycle is four events any workflow emits, and not a state the
  controller infers. R17 — and why adoption claims a session on the PID it can
  see rather than on a state word that means three things: a claim gated on
  `done` takes no idle session at all, and then holds nothing when that session
  hibernates.
- **Test:** `lessons::hibernation_reads_as_a_deliberate_stop`

### A4. A dead terminal host has two shapes and neither carries a status

- **Retired by:** fleet-rge6.3 (2026-09-26). The terminal host is fleet's
  own tmux server, and the agent is its pane's process: a dead one is a
  dead pane carrying its status under remain-on-exit, and nothing
  respawns it. Kept here as history.
- **Fact:** Killing a session's terminal-host process produces a row that is
  field-for-field a deliberately stopped one when the session was idle, and a
  self-respawning pid-null `state: working` row for 10.8–12.1 s (n=4) when it
  was mid-turn. No failure status appears in either shape — the listing's
  `status` field is not a witness of host death — and the session revives with
  its transcript intact. The mid-turn window sits inside a 30 s newborn grace
  with under 3× of margin, and nothing has measured what a slower machine does
  to it: past the grace such a row reads as stopped and is acted on. The
  interactive "press Enter to restart" affordance never competes with a
  controller's revive, because it lives on the attach path and wants a terminal
  a controller has not got.
- **Version:** Claude Code 2.1.247.
- **Date:** 2026-08-27.
- **Implies:** R10 — an unmeasurable context on a pid-less row falls to
  starting a session, and this is the shape that makes the fall dangerous.
  R12 — transient rows get no session-creating verdict.
- **Test:** `lessons::a_dead_host_has_two_shapes`

### A6. `stop` takes the short id and refuses the full session id

- **Retired by:** fleet-rge6.4 (2026-09-26). A seat's session is stopped on
  fleet's own tmux server, by the seat's own session name: one interrupt, a
  grace, then the session killed, and the host's own listing read after the
  kill is the witness. No short id is carried outside the adapter and nothing
  is issued at the daemon's address. Kept here as history.
- **Fact:** Identity and invocation address are different values. Stopping a
  session by its full session id exits 1 with "No job matching"; stopping it by
  the short id on its roster row succeeds. A controller that stores the session
  id as its key — which it should, because the short id and the display name
  are both unstable — has to carry the short id separately as the address it
  issues acts against.
- **Version:** no release recorded with the measurement.
- **Date:** not recorded with the measurement.
- **Implies:** R16 — the rest collection order is stop → start successor →
  remove predecessor, and the removal only after a *successful* stop, which is
  unreadable if the stop was addressed wrongly and exited 1. R17 — the session
  table holds what `stop` needs, not only what `adopt` needs.
- **Test:** `lessons::stop_takes_the_short_id`

### A7. `attach` exits 0 whether or not it revived anything

- **Retired by:** fleet-rge6.4 (2026-09-26). A revive is a new tmux session
  whose command resumes the session's full id (A9), believed only when the
  listing shows the new pane's own process under that same id; nothing is
  attached. The rule this entry taught — an act's own return is not its
  witness — is the one the stop and the revive now keep against the host and
  the listing. Kept here as history.
- **Fact:** An attach that revived a row and an attach that did nothing both
  exit 0. Exit status is not a witness here; only the next roster read
  separates them. The stderr line the tool prints ("Waking session …") is
  narrower than the exit status, not wider: it appeared on 5 of 5 attaches
  aimed at pid-less rows and 0 of 4 aimed at live rows, all nine exiting 0 —
  so it reports what the attach was *aimed at*, not what it *did*. What the
  line does for an attach at a pid-less row that did not take is unmeasured.
  The call is also safe through a pipe: the revived session does not hold the
  inherited output, so it returns in well under a second rather than blocking.
- **Version:** Claude Code 2.1.247 for the stderr/aim split; no release
  recorded for the exit-status measurement.
- **Date:** 2026-08-27 for the split; 2026-08-18 for the founding
  exit-status measurement, which is its record's own date and not a date the
  measurement states.
- **Implies:** R13 — every dispatch opens an arrival window keyed to the
  dispatch the controller recorded, and **a sighting answers the window**. This
  entry is why: the effect's own return cannot.
- **Test:** `lessons::attach_exit_is_not_a_witness`

### A8. Removing a session answers three ways, and one of them deletes a checkout

- **Retired by:** fleet-rge6.4 (2026-09-26). No daemon row is left to remove:
  a seat's session was the host's, and the stop took it. A rest writes no
  removal and a retire removes the seat's worktree with fleet's own
  `git worktree remove --force`, as it always did, then verifies from outside
  that the host holds no session for the seat. Kept here as history.
- **Fact:** Removing a background session **refuses** (rc 1, "worktree has
  commits that are not pushed anywhere", leaving both the row and the
  worktree); or **succeeds and deletes the worktree** (rc 0, printing the
  worktree path, the directory gone afterwards); or **succeeds and keeps it**
  (rc 0). All three were observed on one binary. The trigger was isolated a
  release later, on two arms differing in one variable: **the repository must
  have a remote** — the remote-less arm took the keep-it outcome and the
  remote-bearing arm refused. Since every working checkout a fleet creates has
  a remote, the refusal is the expected answer on a seat's tree rather than a
  lottery. A fourth act exists and cannot be reached by accident: the
  worktree-deleting outcome is only available through a discard flag whose
  token comes out of the refusal itself, so a discard needs a refusal received
  first and a flag typed by hand; a repeat removal refuses byte-identically and
  discards nothing. The two help surfaces describe different outcomes for this
  command, and the top-level one is the current claim. **Not proven:**
  the live-row half of the refusal, whether the token expires or is reusable,
  and whether the keep-it outcome is reachable at all on a repo that has a
  remote.
- **Version:** Claude Code 2.1.233 (first, where removal on a live row exited
  0, removed the row and left the worktree — true then, false now), 2.1.251
  (all three outcomes), 2.1.261 (trigger isolated).
- **Date:** 2026-08-14, 2026-08-31, 2026-09-04.
- **Implies:** R32 — `fleet seat retire` verifies from outside that nothing of the
  seat's holds RAM or disk and refuses on a surviving resource; a remove is an
  act that can FAIL and an act that can DELETE, so its exit status is read and
  it is never treated as a silent row delete. R30 — a spawn's rollback removes
  the worktree and never the branch, which is only safe because the fleet does
  the removal itself rather than asking the agent to. A13 says which worktrees
  the delete outcome can reach.
- **Test:** `lessons::remove_answers_three_ways`

### A10. A newer client replaces the daemon and re-hosts the sessions under it

- **Retired by:** fleet-rge6.3 (2026-09-26). Fleet's seats are no longer
  hosted by the agent's daemon, so no replacement ends them or re-hosts
  them, and the replacement window, the re-host hold and the daemon read
  are gone with it. Kept here as history.
- **Fact:** One background daemon per user hosts every background session.
  Starting a session with a NEWER binary REPLACES that daemon. The replacement
  ends the hosted processes — leaving every row pid-less — and the sessions then
  come back in place, claimed from the new daemon's spare pool, which is why
  session ids survive while the processes under them move. **Inside that
  pid-less window a controller can start over a session that is still there:**
  a row older than a recency bound reads there as no eligible session, gets
  started over, and the re-host then puts the old row back beside the new one.
  The window was 21–61 s on one replacement and had closed within 7 s on
  another, so it is not a constant to code against; the answer is to HOLD a pid-less row while the
  daemon is under a minute old or its pid has just moved. None of this is
  visible to a version-only read: the launcher and the daemon are different
  things, and a fleet can be in a third state — same version, different path —
  that no version comparison distinguishes. A replacement, a controller restart
  and a daemon kill are three different acts: killing the daemon moved nothing,
  one restart re-hosted every session and another moved nothing, and the
  condition separating those two restart readings is not measured.
- **Version:** Claude Code 2.1.251 over 2.1.247, and again 2.1.261 over 2.1.257.
- **Date:** 2026-08-31 and 2026-09-04.
- **Implies:** R11 — the replacement window: no pid-less row is dispatched
  against while the daemon's pid has changed or its uptime is under the arrival
  window, and the log says `replacement window: held`. R17 — adoption by
  session id is what survives the re-host. R34 — this is one of the named
  behaviours measured per platform at the pin.
- **Test:** `lessons::a_newer_client_replaces_the_daemon_and_rehosts`

### A13. A session locks only a worktree the agent created for it

- **Fact:** A background session holds its worktree's version-control lock
  **only** when the agent created that worktree itself, through its own
  worktree flag; the lock file names the session, and removing the worktree
  then exits 128. A session merely running inside a worktree somebody else made
  by hand gets no lock at all, and removing that worktree exits 0 — taking the
  checkout out from under a live session. Two arms, one variable, opposite
  answers. **Every worktree a fleet makes for a seat is the unlocked shape**,
  so a seat's checkout is protected by the fleet's own care and by nothing
  else. This is also the boundary on A8's delete outcome: the worktree a remove
  can delete is one the agent created, which is the same set.
- **Version:** Claude Code 2.1.251.
- **Date:** 2026-08-31.
- **Implies:** R30 — a spawn's rollback removes the worktree and never the
  branch, and the removal is the controller's own act against a tree it made.
  R32 — `fleet seat retire` verifies from outside that nothing of the seat's holds
  disk before it reclaims anything, because no lock will stop it.
- **Test:** `lessons::a_session_locks_only_a_worktree_it_created`

### A14. A start that cannot start says so in-band, fast, and leaves no row

- **Retired by:** fleet-rge6.2 (2026-09-26). Starts are interactive sessions
  in fleet's own tmux panes and no longer `--bg`; a start that cannot start
  is a dead pane carrying its exit status, and a start is believed only when
  the listing shows the pane's process. Kept here as history.
- **Fact:** A background start from a directory that had just been deleted used
  to report "backgrounded" and leave a crashed session row — **a success return
  with no live session behind it**, which the controller reads as a session that
  exists, so it waits for a sighting that does not come. From 2.1.257 it prints
  the reason and exits 1 instead. Re-measured on 2.1.261: the
  exit lands in **0.01 s**, three orders of magnitude inside the five-second
  window a controller watches a start for, with the reason on stderr and no
  roster row left behind. So a controller that reads its child's own exit status
  inside a bounded watch window collects this failure for free and turns it into
  a failed outcome with a logged cause; a child still running when the window
  closes stays OK and is adopted, arrival being the roster's answer. A fleet
  creates and retires worktrees constantly, which is exactly the precondition,
  so this is a live path and not a curiosity.
- **Version:** Claude Code 2.1.257 (the change), re-measured on 2.1.261.
- **Date:** 2026-09-04.
- **Implies:** R13 — every dispatch opens an arrival window keyed to the
  dispatch the controller recorded; a start that exits non-zero inside the watch
  window is a **failure with a cause**, not a blind dispatch to be counted
  against R14's three. R18 — `start` is an effect whose exit status is read.
- **Test:** `lessons::a_failed_start_exits_inside_the_watch_window`

---

## B. The roster listing

### B6. A pre-warmed worker was a claimless row, and the fix is why an unattributed row is now worth investigating

- **Retired by:** fleet-rge6.3 (2026-09-26). The pre-warmed worker was
  the background daemon's, and no interactive session under tmux meets it:
  a seat's session is the pane's own process, and a background row is
  never a seat's (the claude-code pack's B1 and B5). Kept here as history.
- **Fact:** Until 2.1.238 the idle worker the agent pre-warms for the next
  background session appeared on the listing before any task claimed it — a row
  with a working directory and no dispatcher, which is B5's unattributed shape
  with a known and innocent cause. The fix shows that worker only once a task
  claims it. So on releases past that, an unattributed row has some *other*
  origin, and running the alibi test is worth doing rather than shrugging at.
- **Version:** Claude Code 2.1.238.
- **Date:** 2026-08-20.
- **Implies:** R5 — the cwd match is only as good as the assumption that a row
  in a seat's worktree belongs to that seat; this entry is the record of the
  one time it did not, and of that cause being closed.
- **Test:** `lessons::a_prewarmed_worker_is_not_a_seat`

### B9. Resuming a running background session adds a row rather than continuing one

- **Retired by:** fleet-rge6.5 (2026-09-26). A turn for a live seat is typed
  into its own tmux pane and believed when the listing turns busy (D8); no
  print-mode turn carries it and nothing resumes a live session to reach it.
  Kept here as history.
- **Fact:** `--bg --resume` against a background session that is ALREADY
  RUNNING starts a COPY, under a new id, and says so: "session `<id>` is
  already running in the background, so this started a copy as `<new id>`".
  Measured by full id and by short id, with the same outcome either way, which
  is narrower than A9's reading of a resume against a session that is NOT
  running — there the FULL id with no flags continues in place. The consequence
  is on the listing: the original keeps its id, its pid and its own row, and
  the copy appears beside it as a row of its own, so a fleet that resumed a live
  session to reach it would be counting two rows in one worktree and holding an
  address that answers for neither. A verb that wants a turn on a live session
  therefore cannot resume it — it opens a turn of its own.
- **Version:** Claude Code 2.1.261.
- **Date:** 2026-09-21.
- **Implies:** R21 — the rest suggestion is a print-mode turn in the seat's own
  worktree carrying no session address, because the one verb this controller
  aims at a LIVE session is the one that cannot be a resume. R17 — and why
  adoption is a claim recorded against a session rather than anything issued at
  it.
- **Test:** `lessons::a_live_session_is_reached_without_a_resume`

---

## C. The transcript on disk

### C5. The rest threshold is a fraction of a window that moves

- **Fact:** A fleet's rest threshold is chosen as a fraction of the agent's
  context window, and the window is the agent's to change. Two windows were
  measured in one fleet: auto-compaction fires at about 967K on one model and
  at 1,002,002 and 999,061 preTokens on another, taking about 55 s and dropping
  the context to roughly 83K each time. The ordering the threshold exists to
  guarantee — the fleet suggests rest *before* the agent compacts — held across
  both, but the margin differs by model (~267K against ~299K). A window change
  therefore makes the constant wrong with no tool reporting anything: this is a
  **denominator dependency**, not a code one, and a release note that only
  changes context window sizes matches it.
- **Version:** Claude Code 2.1.247 (the first window) and 2.1.261 (the second).
- **Date:** 2026-08-27 and 2026-09-04.
- **Implies:** R21 — one nudge per session on a crossed threshold, keyed on
  session id, with no path from threshold to rest. The threshold is a
  suggestion because the number under it is a fraction of something the fleet
  does not own.
- **Test:** `lessons::the_context_threshold_is_a_fraction_of_a_moving_window`

---

## D. The host the agent runs on

### D1. The child PATH is constructed, never inherited

- **Fact:** A background daemon started from a service environment carries that
  environment's minimal PATH, and every session claimed from it inherits it.
  Worse, each shell call inside a session sources a snapshot whose last line
  re-exports the capturing process's PATH — so one daemon started bare hands
  every later session a PATH that **collapses mid-run**, long after the start
  that caused it. The fix is to construct the child PATH at the point where
  processes are started, once, rather than at each call site: a fifth call site
  cannot forget what it never had to remember. This applies to every process
  the controller starts and not only to the agent, because any of them can be
  the one that starts a daemon. The same minimal PATH is why a controller
  resolves every binary it runs by an explicit path and **never by a bare
  name**: a service environment holds neither a package manager's prefix nor
  the user's local bin, so a bare name finds nothing.
- **Version:** no release recorded with the measurement.
- **Date:** 2026-08-18 for the collapsing-PATH half, 2026-08-14 for the
  bare-name half; both are their records' own dates rather than dates the
  measurements state.
- **Implies:** R19 — the child PATH is constructed, never inherited, on every
  process the controller starts. R33 — the child PATH is one of the six things
  the platform layer owns, because the service environment differs per
  operating system.
- **Test:** `lessons::the_child_path_is_constructed`

### D2. A start's output goes to a file, never to a pipe

- **Retired by:** fleet-rge6.2 (2026-09-26). A start's output is its pane's,
  and a failed start's last screen is captured to a file by core. The
  print-mode nudge sent its output to a file until fleet-rge6.5 removed it
  (2026-09-26): a turn is typed into the seat's pane now, and no child's
  output is collected at all. Kept here as history.
- **Fact:** Collecting a child's output through a pipe waits for EOF on the
  pipe rather than for the child, so ANY process still holding the write end
  keeps the caller blocked — the direct child included, and a grandchild that
  outlives it certainly. This was measured as a whole poll loop lost for hours
  while the service manager reported the process as running. A file has no EOF
  to wait for, so it keeps the child's own words without the wait that loses
  the loop. The related read — an attach — is safe through a pipe,
  because the revived session does not hold the inherited output (A7).
- **Version:** no release recorded with the measurement.
- **Date:** 2026-08-15.
- **Implies:** R18 — `start` is an effect the controller must return from. R13
  — the arrival window only means something if the loop that opened it is still
  running to close it.
- **Test:** `lessons::start_output_goes_to_a_file`

### D4. A blocked permission read is pending, not denied

- **Fact:** The operating system's file-access dialog does not return a
  refusal — it blocks the call until somebody answers. Whether a guarded read
  under it returns a permission error or simply hangs was **never measured**,
  because the measurement needs the service loaded in a desktop session and a
  reset that would target the identifier of the service currently running the
  fleet. So the gate was built so the answer does not matter: a timeout counts
  as PENDING exactly as a refusal does, and the detail says which one was seen.
  A seat whose probe timed out is not re-probed while that probe is still
  outstanding, or a blocked dialog accumulates one parked thread per poll per
  seat. The bound itself is **observational, not a controlled percentile**: it
  is the top of a band from one incident — a read that exceeded 1.5 s and never
  returned, and a recurrence under load that cleared in 5 s — so ten seconds
  calls the recurrence transient and the never-returning read a stopped volume.
- **Version:** not an agent behaviour; the host's permission layer. No agent
  release applies.
- **Date:** 2026-08-28 (the incident the band comes from).
- **Implies:** R1 — on macOS the file-access grant is requested at startup
  before the first poll so it is answered in the minute the service loads, and
  on Linux the gate reads `ok`. R27 — the status command prints the grant above
  the roster. R33 — the permission gate is one of the six things the platform
  layer owns.
- **Test:** `lessons::a_blocked_grant_read_is_pending`

### D7. A process is ended by the pid you started, never by a pattern on its name

- **Fact:** A build tool that names a test binary from a hash of the source
  tree produces the SAME name in every checkout of that tree, and a pattern
  kill matches the whole command line rather than the path — so one typed to
  clear a single hung run on a shared box terminates the identically named
  process in every other checkout at the same instant, under directories the
  person typing it has never seen. Measured once on a box carrying several
  checkouts of one tree: the pattern one worker used to clear its own hang
  matched another worker's copy, under an unrelated temporary path, exactly as
  well as its own. The reach is not the worst of it. The process that did not
  hang dies of SIGTERM, and a SIGTERM in a test log is indistinguishable from
  the out-of-memory kills a loaded box already produces — three runs had
  already been discarded as memory kills that day, so a fourth cause wearing
  the same clothes would have been absorbed without question. It was caught
  only because the worker who typed the kill said so; no instrument saw it.
- **Version:** not release-scoped — the matcher is the operating system's and
  the binary name is the build tool's; no agent release recorded with the
  measurement.
- **Date:** 2026-09-13.
- **Implies:** R17 — the session table is the address book for every act
  against a process, so an effect is aimed at a recorded id or pid and never at
  a name pattern; a name is not an address, which B3 already says of the
  display name. R32 — a retire verifies from outside by reading the roster and
  the process table, never by sweeping for a command-line pattern, which cannot
  tell one checkout's process from another's. R33 — the process table is one of
  the six things the platform layer owns and it answers by pid; a pattern
  matcher is not a portable substitute for it.
- **Test:** `none — a bound on how an act may reach a process, kept by R17's
  recorded address and R32's verification from outside; it names no behaviour
  of its own to exercise.`

---

## Test inventory

Every fixture test named in this file or moved from it, once, and where it
lives: in core, under this exact name in a `lessons` module; in the
claude-code pack, whose own inventory is its `LESSONS.md`'s; or nowhere,
retired with its entry. D7 names no test.

| Test | Entry | Where it lives |
| --- | --- | --- |
| `lessons::version_pin_is_published_beside_the_live_version` | A1 | the claude-code pack, `test/lessons/version_pin_is_published_beside_the_live_version.test.ts` |
| `lessons::pid_null_is_two_states` | A2 | retired with its entry by fleet-rge6.3 |
| `lessons::hibernation_reads_as_a_deliberate_stop` | A3 | retired with its entry by fleet-rge6.3 |
| `lessons::a_dead_host_has_two_shapes` | A4 | retired with its entry by fleet-rge6.3 |
| `lessons::start_names_the_model` | A5 | the claude-code pack, `test/lessons/start_names_the_model.test.ts` |
| `lessons::stop_takes_the_short_id` | A6 | retired with its entry by fleet-rge6.4 |
| `lessons::attach_exit_is_not_a_witness` | A7 | retired with its entry by fleet-rge6.4 |
| `lessons::remove_answers_three_ways` | A8 | retired with its entry by fleet-rge6.4 |
| `lessons::a_resume_by_full_id_keeps_the_session` | A9 | the claude-code pack, `test/lessons/a_resume_by_full_id_keeps_the_session.test.ts` |
| `lessons::a_newer_client_replaces_the_daemon_and_rehosts` | A10 | retired with its entry by fleet-rge6.3 |
| `lessons::the_config_dir_scopes_the_daemon` | A11 | the claude-code pack, `test/lessons/the_config_dir_scopes_the_daemon.test.ts` |
| `lessons::an_mcp_call_backgrounds_at_120s` | A12 | the claude-code pack, `test/lessons/an_mcp_call_backgrounds_at_120s.test.ts` |
| `lessons::a_session_locks_only_a_worktree_it_created` | A13 | core, `cli/tests/seat.rs` |
| `lessons::a_failed_start_exits_inside_the_watch_window` | A14 | retired with its entry by fleet-rge6.2 |
| `lessons::a_first_run_meets_the_trust_dialog` | A15 | the claude-code pack, `test/lessons/a_first_run_meets_the_trust_dialog.test.ts` (shared with gas-city.md G12) |
| `lessons::the_roster_is_one_command` | B1 | the claude-code pack, `test/lessons/the_roster_is_one_command.test.ts` |
| `lessons::the_roster_carries_no_token_field` | B2 | the claude-code pack, `test/lessons/the_roster_carries_no_token_field.test.ts` |
| `lessons::the_name_is_not_an_address` | B3 | the claude-code pack, `test/lessons/the_name_is_not_an_address.test.ts` |
| `lessons::the_roster_read_can_go_silently_dead` | B4 | the claude-code pack, `test/lessons/the_roster_read_can_go_silently_dead.test.ts` |
| `lessons::cwd_names_a_seat_and_proves_nothing` | B5 | the claude-code pack, `test/lessons/cwd_names_a_seat_and_proves_nothing.test.ts` |
| `lessons::a_prewarmed_worker_is_not_a_seat` | B6 | retired with its entry by fleet-rge6.3 |
| `lessons::a_truncated_listing_is_not_absence` | B7 | the claude-code pack, `test/lessons/a_truncated_listing_is_not_absence.test.ts` |
| `lessons::waiting_for_names_the_block` | B8 | the claude-code pack, `test/lessons/waiting_for_names_the_block.test.ts` |
| `lessons::a_live_session_is_reached_without_a_resume` | B9 | retired with its entry by fleet-rge6.5 |
| `lessons::an_interactive_row_is_listed_without_an_address` | B10 | the claude-code pack, `test/lessons/an_interactive_row_is_listed_without_an_address.test.ts` |
| `lessons::the_transcript_path_encoding` | C1 | the claude-code pack, `test/lessons/the_transcript_path_encoding.test.ts` |
| `lessons::the_transcript_entry_shape` | C2 | the claude-code pack, `test/lessons/the_transcript_entry_shape.test.ts` |
| `lessons::sidechains_carry_another_window` | C3 | the claude-code pack, `test/lessons/sidechains_carry_another_window.test.ts` |
| `lessons::the_transcript_outlives_the_process` | C4 | the claude-code pack, `test/lessons/the_transcript_outlives_the_process.test.ts` |
| `lessons::the_context_threshold_is_a_fraction_of_a_moving_window` | C5 | core, `controller/tests/effects.rs` |
| `lessons::the_child_path_is_constructed` | D1 | core, `controller/tests/effects.rs` |
| `lessons::start_output_goes_to_a_file` | D2 | retired with its entry by fleet-rge6.2 |
| `lessons::the_permission_posture_is_model_gated` | D3 | the claude-code pack, `test/lessons/the_permission_posture_is_model_gated.test.ts` |
| `lessons::a_blocked_grant_read_is_pending` | D4 | core, `controller/tests/observe.rs` |
| `lessons::the_plugin_root_addresses_the_hook` | D5 | the claude-code pack, `test/lessons/the_plugin_root_addresses_the_hook.test.ts` |
| `lessons::the_plugin_loader_follows_a_skill_link` | D6 | the claude-code pack, `test/lessons/the_plugin_loader_follows_a_skill_link.test.ts` |
| `lessons::an_interactive_start_under_tmux` | D8 | the claude-code pack, `test/lessons/an_interactive_start_under_tmux.test.ts` |
