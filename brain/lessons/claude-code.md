# Lessons — Claude Code

What fleet knows about the Claude Code substrate, measured rather than read.

The controller is built on measurements taken somewhere else; the PRD it was
built from, archived under `../archive/prds/`, marks each requirement that
rests on one **R**. This file is where those measurements live as fleet's own
knowledge, so a builder changing the behaviour one fences can cite the fact
instead of trusting the letter beside it.

**Every fact here is version-scoped.** Claude Code's session lifecycle is
observed behaviour, not a published contract; a fact without the release it
was measured on is a rumour. Each entry therefore carries **Version** and
**Date**. Where the source of a fact recorded no release or no date, the field
says so — an unrecorded provenance is a fact about the fact, and inventing one
would be worse than carrying it.

**Every fact owes a test.** The **Test** field names a fixture test,
`lessons::<slug>`, in snake_case. That name is the contract between this file
and the slice that lands the code the fact exercises: the slice writes a test
under that exact name, and `## Test inventory` at the foot of this file is what
the scaffold and the slices read. A fact with nothing to exercise says
`none — <why>`.

**Reading the entries.** *Fact* is the measurement in one paragraph. *Implies*
names the archived PRD's requirement the fact fed, by R-number, and says what
the requirement owed it.

---

## A. Session lifecycle

### A1. The pin is a measurement, not a version number

- **Fact:** Every behaviour in this file was observed against one release, and
  the release is part of the fact. The controller therefore records the
  release each behaviour was measured against and publishes it beside the
  version the agent binary reports this poll, so a disagreement is a flag to
  re-measure rather than a failure. A spread between the two is an *unfinished
  move* — someone installed a new binary and the pin has not followed — and not
  drift that arrived on its own, provided the agent's background auto-updater
  is off. The measurement obligation is real work: one move re-measured twelve
  entries on a checksum-verified scratch binary and answered every one.
- **Version:** Claude Code 2.1.233 (the first pin) through 2.1.261, with
  re-measures at 2.1.240, 2.1.247, 2.1.251, 2.1.257 and 2.1.261.
- **Date:** 2026-08-06 through 2026-09-04.
- **Implies:** R29 — `[substrate.<agent>]` pins, and a live version that
  differs is a flag in the projection and a `substrate.moved` event, never a
  failure. R8 — the version is read every poll and published beside the pin,
  never measured once at startup and republished (a controller that cached it
  advertised its boot version for as long as it lived).
- **Test:** `lessons::version_pin_is_published_beside_the_live_version`

### A2. A row with no pid is two different states

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

### A5. `start` must name the model; the default is not the fleet's

- **Fact:** A background session started with no model flag was measured to
  come up on the cheapest available model. The model is therefore mandatory on
  every start, and it is what keeps a seat in the class its config declares.
  The same explicit flag is what makes the environment's own default-model
  variable harmless: an inherited default cannot reach a seat whose start names
  its model. One behaviour rode this shape — a background start on the top
  model could stall at birth asking for usage credit while an interactive
  session on the same account still had allowance — so a seat that never draws
  its first breath is read against that behaviour first.
- **Version:** the default-model measurement carries no release in the source
  that states it; the credit stall was fixed in Claude Code 2.1.251.
- **Date:** not recorded with the default-model measurement; 2026-08-31 for the
  credit stall's fix.
- **Implies:** R18 — `start` passes model, name and the fleet's permission
  posture on every call, and the default is never the agent's.
- **Test:** `lessons::start_names_the_model`

### A6. `stop` takes the short id and refuses the full session id

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

### A9. Only one of three resume shapes continues the session

- **Fact:** Resuming by SHORT id forks a copy. Resuming by FULL id WITH any
  flag forks a copy that inherits the original's name. Resuming by FULL id with
  NO flags continues the session in place — same id, same name, new pid. The
  CLI names each outcome in its own notice, which is why they are quoted rather
  than summarised. The consequence is on the roster: three of four revive
  shapes forked, two of the three gave the copy the original's name, and the
  roster ended with four rows, three carrying one name and one of them alive.
- **Version:** Claude Code 2.1.257 and 2.1.261, identical on both.
- **Date:** 2026-09-04.
- **Implies:** R17 — startup **adopts** by session id, never by name and never
  by re-issuing a resume: a controller that resumed with its own flags would
  fork the session it meant to reclaim, and then hold a table row pointing at a
  dead twin.
- **Test:** `lessons::resume_continues_only_a_flagless_full_id`

### A10. A newer client replaces the daemon and re-hosts the sessions under it

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

### A11. The config directory scopes the daemon, and a scratch one is logged out

- **Fact:** The daemon's socket directory is derived from the agent's config
  directory, so overriding the config directory starts a *second* daemon with
  its own socket and its own workers while the fleet's keeps its pid, its
  uptime and its binary. This is **not described in the docs we read at these
  releases**, and it is therefore verified at every use rather than trusted:
  two daemons were observed side by side on two binaries, with the fleet's pid
  rising monotonically across every read. A
  scratch config directory is also **logged out**, because the credential
  item's service name carries a hash of the config directory — and the escape
  is a *second*, separate variable that overrides the credential input, which
  when defined-but-empty restores the default service name. The two knobs point
  in opposite directions on purpose: one scopes the daemon, one scopes the
  credential. Re-measured session-free at the pin under a cleared environment:
  the config directory set to the home **default**, explicitly, logs a child out
  exactly as a scratch one does, and the credential knob set to any
  **directory** — the home default included — logs it out too, while
  defined-but-empty, under either config directory and under none, is logged in.
  So a process that sets the config directory on its children carries the
  credential knob as the **configured value**, empty when nothing configured
  one, and never the path it resolved: a resolved directory there is a third
  credential nobody wrote. And the daemon the directory scopes holds its OWN
  roster: a session started under a per-row directory is named by that daemon's
  listing and by **no other**, the fleet's included, so a controller that reads
  one listing per poll publishes every such seat absent and a stop, a removal, a
  nudge or a transcript read made under the wrong directory reaches nothing.
  Measured on two live sessions side by side: the fleet's listing answered nine
  rows and named neither the scratch-directory session nor its short id, whose
  own listing answered exactly one and named only it, `logs <short id>` from the
  fleet's directory exited 1 with `No job matching`, and the fleet daemon's pid
  was unchanged before and after. A logged-out session is **live** in that
  listing — a pid, an idle status — so the roster cannot report the failure and
  the transcript is the only surface that carries it: one user turn, then one
  synthetic assistant entry with `error` `authentication_failed`,
  `isApiErrorMessage` true and a window of zero.
- **Version:** Claude Code 2.1.251 (the scoping), 2.1.261 (re-measured, the
  credential knob read out of the binary, and the per-daemon roster).
- **Date:** 2026-09-01, 2026-09-04 and 2026-09-12.
- **Implies:** R29 — a pin move needs a way to probe a new binary without
  moving the running fleet, and this is the only isolation route there is; a
  release that changes the socket-directory derivation removes it, with nothing
  on fleet's side to report the loss, so the probe gate goes red rather than
  proceeding. R34 — measuring a platform at the pin needs this isolation to be
  safe on a machine that is running.
- **Test:** `lessons::the_config_dir_scopes_the_daemon`

### A12. An MCP tool call backgrounds after 120 s unless a human is being asked

- **Fact:** A tool call still running at 120,000 ms is registered as a
  background task and the model is handed a "still running" result, so the turn
  continues. Three things switch that off and each lets one call hold a session
  open with no bound: a pending elicitation on the call (the timer restarts for
  as long as one is outstanding), a server of an IDE transport type, and the
  auto-background interval set to zero. This is what decides which blocked
  classes can outlive an operator — a hung call that asks nobody anything
  self-heals in two minutes, while one that raises a dialog waits as long as
  the human does. Measured by three arms against a server whose tool never
  returns, with the constants and the three exemptions read out of the
  installed binary rather than inferred from the arms.
- **Version:** Claude Code 2.1.247.
- **Date:** 2026-08-31.
- **Implies:** R26 — the projection's per-seat roster state is only *complete*
  for stalls this rule does not end; a release that removes the timeout widens
  that blind spot with nothing on fleet's side to report it, because the class
  it currently ends would start persisting.
- **Test:** `lessons::an_mcp_call_backgrounds_at_120s`

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

### A15. A first start in an untrusted directory blocks on the workspace-trust question

- **Fact:** In a directory the agent has not been trusted in, a session started
  there blocks at startup on the agent's own workspace-trust question ("Is this
  a project you created or one you trust?") and the process exits non-zero. The
  trust state is per-directory and recorded in the agent's own configuration —
  a folder reads a trust-accepted flag once the question has been answered —
  and the only way to answer it is interactively, once, in that directory.
  Met under the Gas City trial (gas-city.md G12), where the controller's
  always-on session died this way at startup and its reconciler re-created it
  every few seconds, with the only readable trace a per-session stderr log:
  nothing in the controller's log and nothing in its events. This is the agent's
  behaviour, not the engine's, and it reaches any fleet the same way — a fleet
  that makes a fresh worktree and starts a session in it meets a **new
  directory on every spawn**, so the question is asked mid-spawn unless install
  has already answered it.
- **Version:** Claude Code 2.1.261, met under gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R1 — a first-run gate belongs in install, answered in the minute
  the service loads, beside the file-access grant of D4; a start is not where a
  person should meet a dialog. R30 — a spawn's worktree is a new directory, so
  the trust state has to be established for it or inherited. R14 — three
  consecutive blind dispatches halt the seat and the halt is announced: an
  unbounded re-create loop against a dialog nobody can answer is the failure
  that requirement exists to stop.
- **Test:** `lessons::a_first_run_meets_the_trust_dialog` — shared with
  gas-city.md's G12 deliberately. One behaviour, one fixture, cited from both
  files; a second name would be a second test of the same fact.

---

## B. The roster listing

### B1. The roster is one command, and it tolerates fields it does not know

- **Fact:** The whole observation layer is one listing command asking for JSON
  across all sessions. The fields consumed are `id`, `sessionId`, `cwd`,
  `kind`, `pid`, `status`, `state`, `name` and `startedAt`. The listing carries
  at least one field beyond those, which is harmless because the reader does
  not deny unknown fields. Field presence is kind-dependent — an interactive
  row and a background row do not carry the same set — so a reader that
  requires a field on every row fails on the first mixed listing.
- **Version:** schema measured against Claude Code 2.1.233; an extra field
  observed on 2.1.247.
- **Date:** 2026-08-14 and 2026-08-27.
- **Implies:** R5 — rows are matched to seats by cwd and the short id is never
  compared. R6 — an unreadable roster is Unknown for every seat.
- **Test:** `lessons::the_roster_is_one_command`

### B2. The roster carries no token field of any kind

- **Fact:** There is no context or token figure anywhere in the listing.
  Context accounting cannot come from the roster; it comes from the transcript
  on disk. A release that added token figures to the listing would be an
  **adoption opportunity**, not a break, and should be routed as one.
- **Version:** no release recorded with the measurement.
- **Date:** not recorded with the measurement.
- **Implies:** R7 — context tokens come from the transcript's main chain, and
  the requirement says so explicitly because the cheaper-looking source does
  not exist.
- **Test:** `lessons::the_roster_carries_no_token_field`

### B3. The display name is not an address and not a key

- **Fact:** The name on a roster row is model-generated when nothing set it,
  and unstable in several directions at once. Repeat-backgrounding a named
  session **numbers** the row (`my-session (2)`) rather than listing it twice.
  A rename warns that other sessions may still show the old name when the
  registry could not be updated, so a rename is not guaranteed to propagate
  across rows. And forks inherit the name they copied, so a roster does not
  guarantee one row per name: four rows were observed with three of them
  carrying one name and one of them alive. The reversion-to-session-id shape
  the rule was first written from has now gone two releases without an
  observation — an explicit name survived four revivals — and should be read as
  history rather than current behaviour. The rule it protects is unchanged and
  is vindicated by the forks instead.
- **Version:** Claude Code 2.1.246 (numbering), 2.1.247 (the rename warning and
  the name-survives-revive reading), 2.1.261 (the forks).
- **Date:** 2026-08-27 and 2026-09-04.
- **Implies:** R5 — the short id is never compared and rows are matched by cwd;
  a name-keyed reader meets a collision the moment a fork exists.
- **Test:** `lessons::the_name_is_not_an_address`

### B4. The listing answered zero bytes and exit 0 while sessions were live

- **Fact:** The listing has been observed answering with zero bytes and exit 0
  while seventeen peers were reachable from inside a live session, and while
  the controller's own collector had published three seconds earlier — so the
  empty answer came from the listing and not from the reader. The founding
  incident's own cause was later attributed to a collapsed environment rather
  than to the listing, and the exit-0/zero-byte read itself has no attributed
  cause on either side, so fleet keeps the **class** open. A reader that treats
  an empty listing as "no sessions" turns this into every seat reading absent
  at once.
- **Version:** no release recorded with the measurement.
- **Date:** 2026-08-18.
- **Implies:** R6 — an unreadable roster is Unknown for every seat, and Unknown
  is always leave-alone. The requirement's force is that **empty is
  unreadable**, not a reading of zero.
- **Test:** `lessons::the_roster_read_can_go_silently_dead`

### B5. `cwd` names a seat and proves nothing

- **Fact:** One worktree per seat makes the map from working directory to seat
  a function, which is what the whole matching rests on. But the working
  directory is where a process was launched, never who dispatched it. A session
  that no dispatch record accounts for is **unattributed** — it is standing in
  a seat's worktree and it is not the fleet's. Nothing the substrate reports is
  an owner field, so the working directory is safe for acts that are harmless
  against a session the controller did not start, and never for acts that are
  not.
- **Version:** no release recorded with the measurement.
- **Date:** 2026-08-14.
- **Implies:** R5 — two live rows in one worktree is Unknown, not a contest.
  R30 and R32 — a spawn's load belt and a retire's resource check both read
  rows the controller may not own.
- **Test:** `lessons::cwd_names_a_seat_and_proves_nothing`

### B6. A pre-warmed worker was a claimless row, and the fix is why an unattributed row is now worth investigating

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

### B7. A truncated listing is a third state, never absence

- **Fact:** Since 2.1.234 a roster read can report that the account's session
  list was too long to check completely, instead of presenting the unsearched
  remainder as absent. Any rule that rests on "no live row bears this seat's
  name" therefore has three answers, not two: present, absent, and
  **unjudgeable**. The same wording also reaches a caller on the message-send
  path, appended to a name that did not resolve — which is the surface where it
  is most likely to be misread as "they are gone". The strings were confirmed
  present in the installed bundle with a positive and a negative control over
  the same search, because a plain search reports a false absence on a bundle
  that large. No truncation line has been observed in a real read (32 rows).
- **Version:** Claude Code 2.1.234 for the behaviour, strings confirmed in the
  2.1.240 bundle.
- **Date:** 2026-08-22.
- **Implies:** R6 — Unknown for every seat, and Unknown is always leave-alone.
  R32 — `--dead` is licensed by a **completed** roster read that names no live
  session, never by silence, and this entry is the shape that makes silence and
  completion different things.
- **Test:** `lessons::a_truncated_listing_is_not_absence`

### B8. One field says a session is stopped in front of a human, and it is keyed on presence

- **Fact:** A roster row carries a "waiting for" field **only** while the
  session is stopped waiting on a human, and its value names the cause. The
  whole vocabulary is six causes, read out of the installed binary rather than
  enumerated by measurement: a sandbox request, input needed, a worker request,
  a dialog being open, the top dialog's own label, and **a permission-prompt
  default that any dialog kind with no label of its own falls back to**. That
  default is why keying the detector on the field's PRESENCE holds: a dialog
  nobody has seen still arrives named rather than absent. Two causes are
  measured live. The neighbouring `status` and `state` fields move alongside it
  but neither is safe alone — a refusal that lets the turn continue also reads
  blocked. Measured across 72 snapshots: the field appeared on only the two
  prompt-blocked arms, replicated across two permission postures, and on none
  of four negative controls (a long foreground tool call, a finished turn with
  a live background shell, a refused turn, an idle session). **Two gaps.** A
  session stopped at an operating-system permission dialog is expected to carry
  nothing and read busy — reasoned, not measured, because forcing that dialog
  puts a modal on a shared machine. A session inside a tool call that raises no
  dialog is invisible too, and that one is measured: identical to an ordinary
  long tool call on every stable field. The second gap is the lesser one,
  because A12 ends that class at 120 s.
- **Version:** Claude Code 2.1.247.
- **Date:** 2026-08-28.
- **Implies:** R26 — the projection's per-seat rows carry the roster state, and
  this field is the only thing on any surface that distinguishes *stopped
  waiting for a human* from *working*. R27 — which is why the status command
  prints the grant and the block above the roster rather than below it. A
  rename or a new cause value is a **silent break** of any detector keyed on
  the field, since a renamed field reads as absent, which is
  indistinguishable from a healthy working session.
- **Test:** `lessons::waiting_for_names_the_block`

### B9. Resuming a running background session adds a row rather than continuing one

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

### C1. The transcript path is an encoding, and one variable blinds every reader at once

- **Fact:** The agent encodes a project directory by replacing NON-ALPHANUMERIC
  characters with `-` — not the separators alone — and writes the session's
  JSONL under a per-project directory keyed by that encoding. The census, on
  this machine: of 235 per-project directories, zero carry any character
  outside `[A-Za-z0-9-]`, and a path under a dot-directory resolves with the
  dot as a dash too, so a dot following a separator is two dashes — 103 of the
  235 carry that doubled dash. The separator-only rule is refuted by specimen
  and not by argument. WHAT THE CENSUS REACHES IS `/` AND `.` AND NO OTHER
  non-alphanumeric character, and that was searched for rather than assumed: of
  the 235 names, 27 resolve back to a directory that still exists and not one
  of those paths carries an underscore or a space, while 208 name a path that
  is gone. So the rule stated above is what the controller implements, held
  against a census that contradicts no part of it and exercises two characters
  of it. Two further parts are unmeasured for the same want of a specimen — a
  path past roughly 200 characters, which the agent is said to truncate with a
  hash suffix and whose longest local specimen is 136 characters, and a
  non-ASCII character. All three yield a path that does not exist, which every
  reader renders as a seat with no context reading. Every
  context instrument resolves the same way, which means they all go blind
  together: a change to the encoding, or the environment variable that renames
  the per-project directory, takes out the context read, the cost read and the
  rest trigger in one act, and none of them reports anything — they find no
  file and read no context.
- **Version:** Claude Code 2.1.261 for the census; the directory-renaming
  variable arrived in 2.1.234, and no release was recorded with the
  separator-only claim this entry replaces.
- **Date:** 2026-09-06, re-taken 2026-09-08 with every figure unchanged.
- **Implies:** R7 — context tokens come from the transcript, so the path is a
  hard dependency of the observe layer. R34 — the transcript location is one of
  the named behaviours measured per platform at the pin, because a path
  encoding is exactly the kind of thing that differs across operating systems.
- **Test:** `lessons::the_transcript_path_encoding`

### C2. The transcript entry shape

- **Fact:** Entries carry a `type` (user or assistant), a sidechain flag, a
  meta flag, and a usage object holding the input tokens and both cache
  figures. Context for a session is the arithmetic over those fields on the
  main chain; there is no single "context" number to read.
- **Version:** no release recorded with the measurement.
- **Date:** not recorded with the measurement.
- **Implies:** R7 — the requirement names the main chain and the sidechain skip
  because the shape forces both.
- **Test:** `lessons::the_transcript_entry_shape`

### C3. A sidechain entry carries a different context window

- **Fact:** A sidechain entry is a subagent's turn and carries the subagent's
  window, not the session's. Counting one publishes a number from a different
  context and feeds it to whatever decides when a seat should rest. The flag is
  present on every entry, so the skip is a filter and not an inference.
- **Version:** no release recorded with the measurement.
- **Date:** 2026-08-14.
- **Implies:** R7 — sidechains skipped, stated in the requirement rather than
  left to the implementation.
- **Test:** `lessons::sidechains_carry_another_window`

### C4. A hibernated session's transcript outlives the process that wrote it

- **Fact:** The transcript stays readable after the process that wrote it is
  gone, which is what makes a pid-less row answerable at all: the question
  "how much context is this hibernated session carrying" has an answer, and it
  is read the same way as for a live row. Without it, a revive would be a guess
  and the only safe verdict on a pid-less row would be to leave it alone
  forever.
- **Version:** Claude Code 2.1.234, confirmed running at the measurement.
- **Date:** 2026-08-18.
- **Implies:** R10 — the discriminator for a pid-less non-newborn row is the
  deliberate-end event **plus context**, and the context half is only available
  because of this.
- **Test:** `lessons::the_transcript_outlives_the_process`

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

### D3. The permission posture is model-gated, and the downgrade is reported only on screen

- **Fact:** A session's permission mode is not honoured by every model. Across
  four models measured on one release, three came up in the requested mode and
  one came up in the default and said so **only on screen** — nothing any
  instrument reads reports the downgrade. A seat on such a model renders an
  approval dialog at the first call needing one and stops with nobody there to
  answer. Membership in the capable set is by prefix (family plus major),
  because live model ids carry suffixes that name the same model: one is dated
  and another is windowed.
- **Version:** Claude Code 2.1.257.
- **Date:** 2026-09-01.
- **Implies:** R18 — `start` passes the fleet's permission posture on every
  call, and the fleet checks that the model can honour it rather than assuming
  the call was enough.
- **Test:** `lessons::the_permission_posture_is_model_gated`

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

### D5. A plugin's hook and its Bash tool reach `bin/` by different addresses

- **Fact:** A plugin root loaded into a session gives the agent's Bash tool a
  `PATH` that carries the root's `bin/`, so a bare `fleet` in a shell command
  resolves to the plugin's own copy — measured with a stub root whose
  `bin/fleet` recorded every invocation, while `command -v fleet` outside the
  session exited 1. **A hook process gets neither.** Its `PATH` does not carry
  `bin/`, and what it does carry is `CLAUDE_PLUGIN_ROOT` in its environment,
  set to the root the session loaded. So the two wirings are not
  interchangeable: a hook command written as a bare name finds nothing, and the
  same command addressed through `"${CLAUDE_PLUGIN_ROOT}"/bin/` finds the copy
  the session is running. The per-provider overlay keeps the bare form, because
  that is the pack's own wiring for a session that already resolves the binary;
  the plugin's hook file is that same command list with the root prefixed.
- **Version:** Claude Code 2.1.261.
- **Date:** 2026-09-08.
- **Implies:** The plan's § 4.4 — the plugin shape is how the per-provider
  overlay reaches a session at all, and the address a hook uses is the only
  part of it the overlay cannot state for itself.
- **Test:** `lessons::the_plugin_root_addresses_the_hook`

### D6. The plugin loader follows a symbolic link into a pack

- **Fact:** A plugin root's `skills/` entry may be a **symbolic link** to a
  directory elsewhere in the checkout, and the loader follows it: measured
  three ways in one sitting on one root. A skill whose entry was a link to a
  pack's own skill directory resolved under the plugin's namespace and answered
  with the token only the linked file carried. A real directory beside it, the
  positive control, answered with its own token, so the session was loading
  plugin skills at all. And with the link removed and the target file left
  exactly where it was, the same invocation answered `Unknown command` — which
  is what says the link was the route rather than some other path to the same
  bytes. So a pack's skills reach a session by being linked from the plugin
  root, and the pack keeps the only copy; a pinned duplicate under the plugin
  root, which is what a loader that did not follow links would have forced, is
  not needed.
- **Version:** Claude Code 2.1.261.
- **Date:** 2026-09-13.
- **Implies:** The plan's § 4.4 — the plugin shape is how a pack's opinion
  reaches a session, and a link is what puts a pack's skills into that shape
  without a second copy to drift out of agreement with the first.
- **Test:** `lessons::the_plugin_loader_follows_a_skill_link`

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

Every fixture test named above, once. The scaffold reads this table; each
slice that lands the code a fact exercises writes the test under this exact
name.

| Test | Entry |
| --- | --- |
| `lessons::version_pin_is_published_beside_the_live_version` | A1 |
| `lessons::pid_null_is_two_states` | A2 |
| `lessons::hibernation_reads_as_a_deliberate_stop` | A3 |
| `lessons::a_dead_host_has_two_shapes` | A4 |
| `lessons::start_names_the_model` | A5 |
| `lessons::stop_takes_the_short_id` | A6 |
| `lessons::attach_exit_is_not_a_witness` | A7 |
| `lessons::remove_answers_three_ways` | A8 |
| `lessons::resume_continues_only_a_flagless_full_id` | A9 |
| `lessons::a_newer_client_replaces_the_daemon_and_rehosts` | A10 |
| `lessons::the_config_dir_scopes_the_daemon` | A11 |
| `lessons::an_mcp_call_backgrounds_at_120s` | A12 |
| `lessons::a_session_locks_only_a_worktree_it_created` | A13 |
| `lessons::a_failed_start_exits_inside_the_watch_window` | A14 |
| `lessons::a_first_run_meets_the_trust_dialog` | A15 (shared with gas-city.md G12) |
| `lessons::the_roster_is_one_command` | B1 |
| `lessons::the_roster_carries_no_token_field` | B2 |
| `lessons::the_name_is_not_an_address` | B3 |
| `lessons::the_roster_read_can_go_silently_dead` | B4 |
| `lessons::cwd_names_a_seat_and_proves_nothing` | B5 |
| `lessons::a_prewarmed_worker_is_not_a_seat` | B6 |
| `lessons::a_truncated_listing_is_not_absence` | B7 |
| `lessons::waiting_for_names_the_block` | B8 |
| `lessons::a_live_session_is_reached_without_a_resume` | B9 |
| `lessons::the_transcript_path_encoding` | C1 |
| `lessons::the_transcript_entry_shape` | C2 |
| `lessons::sidechains_carry_another_window` | C3 |
| `lessons::the_transcript_outlives_the_process` | C4 |
| `lessons::the_context_threshold_is_a_fraction_of_a_moving_window` | C5 |
| `lessons::the_child_path_is_constructed` | D1 |
| `lessons::start_output_goes_to_a_file` | D2 |
| `lessons::the_permission_posture_is_model_gated` | D3 |
| `lessons::a_blocked_grant_read_is_pending` | D4 |
| `lessons::the_plugin_root_addresses_the_hook` | D5 |
| `lessons::the_plugin_loader_follows_a_skill_link` | D6 |
