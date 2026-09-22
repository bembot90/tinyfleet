# Lessons — Gas City

What fleet learned from running Gas City, and what fleet does instead.

Gas City is the closest thing to fleet that exists. It was installed on a real
machine for one evening, by one person typing every command as a first-timer
would, with a second pair of eyes reading the box after each step — twelve arms
across install, adoption, restart, crash, idle, nudge, orders, events and the
stream. That evening is the only reason several requirements in the controller
PRD are marked **T** rather than guessed at, and it is why fleet's install
refuses eight things that evening measured.

**Every fact here is version-scoped**, the same as the agent-substrate
lessons: it was measured against one release of one engine, and the release is
part of the fact. Each entry carries **Version** and **Date**.

**Every entry says what fleet does instead.** That clause lives in *Implies*,
beside the PRD requirement it feeds. Where the finding is a shape fleet copies
outright rather than avoids, *Implies* says that too — four things here worked
exactly as documented, and copying them is the cheapest requirement in the PRD.

**Every fact owes a test**, named `lessons::<slug>` in snake_case, listed once
in `## Test inventory` at the foot of this file. A fact with nothing to
exercise says `none — <why>`.

---

## A. Onboarding — what one person met in one evening

### G1. `init` installed a user service and started an agent, with no question asked

- **Fact:** The wizard ran eight steps. After the prompts — templates, provider
  — it went on to "Registering city with supervisor", "Installed launchd
  service" (run-at-load, keep-alive, the user's whole PATH baked into the plist)
  and "Waiting for supervisor to start city / Adopting sessions…". By the time
  the shell prompt came back there was a supervisor process under the platform's
  service manager listening on a loopback port, a **second** database server for
  the engine's own ledger, and a terminal-multiplexer session running the agent
  as an always-on named seat. The tutorial says `init` then `start`; the wizard
  does both, inside a command named `init`.
- **Version:** gc 1.4.1 (Homebrew, pinned); agent Claude Code 2.1.261.
- **Date:** 2026-09-05.
- **Implies:** R1 — `fleet install` writes the platform's user service, creates
  the machine directory, and **does not start anything**; loading it is a
  second deliberate act. Fleet separates install from start because this
  evening showed what one command that does both feels like to the person who
  runs it.
- **Test:** `lessons::install_does_not_start_anything`

### G2. Command-usage telemetry was on and uploading within minutes, with no disclosure shown

- **Fact:** The metrics command read "State: enabled", notice
  required/accepted 1/1, a third-party endpoint, an installation id present,
  and an upload attempt and success recorded for the hour of the install. The
  person who ran the wizard, asked directly whether it had told him, answered:
  "It never asked". What the envelope carries was not read; the engine's own
  status command says it is redacted.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R2 — telemetry is **off, and asked**: no metric leaves the
  machine without a disclosure shown and accepted. This retires any debate
  about fleet's own default.
- **Test:** `lessons::telemetry_is_off_and_asked`

### G3. What the minimal template carries

- **Fact:** Choosing the smallest of four templates still wrote: an always-on
  named agent; two packs pinned by hash into a cache under the home directory;
  ten formulas of the engine's own shape; seven engine skills written into the
  project's agent-config directory with an ownership file pointing back at the
  cache; two further agents beside the named one; a generated wrapper script
  for the work-graph CLI; and an agent settings file that skips the dangerous-
  mode prompt, enables every project extension server, and installs three
  hooks. Three health orders then ran every thirty seconds and had opened and
  closed fifty-seven tracking issues within a quarter of an hour. The four
  templates were offered with no line saying what a first-timer was choosing
  between, and two agent providers were offered where the docs count seventeen.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R3 — one command creates either mode and **embedded is the
  smallest file that runs**, every value defaulted from the directory. R1 —
  what install writes is what the person asked for and nothing else. Fleet
  names its smallest template for exactly what it writes.
- **Test:** `lessons::a_minimal_template_is_minimal`

### G4. Adopting an existing work-graph store ran a forced init and deleted the project's export before the store's own guard stopped it

- **Fact:** Four attempts to register a fresh clone of an existing project.
  Three refusals: the directory already contains a store, use adopt; adopt
  requires an issue-prefix key in the store's config — a key the store itself
  has never required; and then a two-letter prefix the engine derived from the
  directory name, reported as a conflict, the refusal naming a flag the person
  had not passed. The fourth attempt, with adopt and an explicit prefix,
  ran a forced re-initialise of the store; that command's help text reads "skip
  init" and "never destructively reinitializes". Before it stopped it had
  **deleted the project's 2,122-line export**, written thirteen keys into the
  project's config (including turning the project's own export and backup off,
  and registering thirteen engine-specific issue types), and created a database
  named after the project on the engine's own server. What ended it was the
  work-graph tool's remote-history guard, exit 1, the project not registered —
  **the deletion happened before the refusal that stopped it**, and the
  refusal came from the store and not from the engine.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R3 — in either mode the controller **never initialises and never
  rewrites a project's work-graph store**; a project without the declaration
  file is refused with the file to write, and that is the whole of fleet's
  interaction with a project's store. This is the row that decided the engine
  question: fleet owns registration entirely rather than delegating it, so a
  first-timer's store is never written by the controller at all.
- **Test:** `lessons::an_existing_store_is_never_reinitialised`

### G5. Registering a project made a local commit in the user's repository, unasked

- **Fact:** Registering a project committed to the user's clone — a commit
  titled for the work-graph initialisation — and left two files modified and
  one untracked beside it. Unpushed, and therefore the user's to undo, but the
  engine wrote history into a repository it had been pointed at, without asking.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R3 — the controller never writes to a project's history. Fleet's
  registration writes one declaration file the person is shown first, and
  commits nothing.
- **Test:** `lessons::the_controller_never_commits_to_a_project`

### G6. Where the CLI's answers and its documentation differed at 1.4.1

- **Fact:** Three small ones, each costing a step. A config-validate command
  exists in the engine's HTTP API and not in the 1.4.1 CLI, so it was handed
  over as a command and refused; the doctor is the check. An empty template
  refused to combine with provider flags, with the refusal naming the
  combination rather than the alternative. And on a city nobody had touched,
  the first doctor a newcomer runs passed 81 checks with two warnings — one of
  them telling them to reconcile a split store the wizard's own init had just
  created, and a structure check reporting "0 agents, 0 rigs" beside a listing
  command that named five agents.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R1 and P1's `fleet doctor` — fleet's doctor reports on the
  person's project rather than on the installer's own work. It answers three
  ways (ok, not ok, could not tell) and every warning it raises names
  something the person can act on.
- **Test:** `lessons::the_first_doctor_reports_on_the_person_not_the_installer`

---

## B. The controller's own lifecycle

### G7. A restart adopts rather than respawns, and says so

- **Fact:** Across a stop-and-start of the supervisor, both live agent sessions
  kept their process ids and their multiplexer server; only the supervisor's
  own pid moved. Its log records the shutdown, the restart, "Adopted 1 running
  session(s) into bead store", and a startup ready in 1.47 s. Two details
  beside it: a forced shutdown reads as `previous_exit=crash` to its successor,
  and **adoption emits no session event** — the stream shows the controller
  starting and nothing about what it reclaimed. This is the one behaviour of the
  engine's that is better than the reference controller's, where a restart
  re-hosts nothing *by accident* and says nothing about it either.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R17 — the controller persists a session table, adopts every live
  session it names by session id at startup, and emits `session.adopted` for
  each. Copied outright, with the event the engine does not emit added. R25 —
  an event is written by the layer that did the thing, so adoption writes one.
- **Test:** `lessons::a_restart_adopts_and_says_so`

### G8. `stop` was a teardown of the service registration, not a pause

- **Fact:** The second stop of the evening stopped the city's sessions, its
  database server, and **removed the service-manager job** — after it, the
  service listing carried no row at all, so keep-alive had nothing to keep
  alive. Because the stop took longer than five seconds, a start issued five
  seconds later answered "supervisor already running" and started nothing; a
  minute later there was no supervisor and no service. The eventual start
  brought the supervisor back as a plain daemonised process with no parent and
  no service row, and said "Supervisor started (PID …)" with nothing about the
  service. So after one stop/start cycle the "machine-wide supervisor" is
  outside the service manager and a crash of it stays down until a human runs
  start. A first-timer who ran stop, then start, passed through three states,
  and the CLI's own output did not distinguish them.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R1 — install writes the service and load is a separate act, so
  stop and start are the *load* pair and neither touches the registration.
  Fleet's stop is a pause; uninstalling is a different verb, and it is the only
  one that removes the service.
- **Test:** `lessons::stop_and_start_are_inverses`

### G9. A stop returned success while the session's process stayed alive

- **Fact:** The always-on session sat in `draining` for an hour. Its interrupt
  was refused as an "illegal transition: state draining does not accept command
  suspend", and its stop was repeatedly skipped on "instance token mismatch
  (session was replaced)" — while its process stayed alive in its pane the whole
  time. The engine's account of that session and the process table disagreed for
  forty minutes and nothing said so. Separately, the two stops of the evening
  differed in exactly one input — one taken while the seat was awake and idle,
  one while it was held asleep — and took different exit paths; the docs
  describe one.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R16 — the rest collection order is stop → start successor →
  remove predecessor, **and the removal only after a successful stop**. R32 —
  `fleet seat retire` verifies from **outside** that nothing of the seat's holds RAM
  or disk, and refuses on a surviving resource; a controller's own account of a
  session is checked against the process table before the seat is called gone.
- **Test:** `lessons::a_stop_that_cannot_stop_says_so`

### G10. Config is re-read without a restart, and it worked exactly as documented

- **Fact:** Replacing the always-on agent with a one-agent pack was one file
  edit and one reload: the new agent came up, the old one went to draining, no
  supervisor restart, and a per-agent key written by hand read back through the
  engine's config-explain command with the file it came from named beside it.
  The pack format held up exactly as its schema documents. A second reload on
  an unchanged tree answered "No config changes detected".
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R28 — `fleet.toml` re-read on mtime with a last-good copy, and
  per-key overrides beneath it. Copied, along with the config-explain shape:
  a key read back with the file it came from is what made a hand-written key
  provable in one command.
- **Test:** `lessons::config_is_reread_without_a_restart`

---

## C. Sessions under it

### G11. The provider defined no permission mode, so every agent ran with permissions skipped at maximum effort

- **Fact:** The agent provider's option schema accepts a model and **does not
  define a permission mode at all**: both candidate values were refused as "not
  a valid choice", and the binary carries the string "session provider does not
  define permission_mode in options_schema". The consequence, measured on both
  sessions of the evening, is that every agent launches with permissions
  skipped and effort at maximum, whatever the agent is for, under a settings
  file that also skips the dangerous-mode prompt and enables every project
  extension server. Fleet's own defaults are chosen for a solo builder running
  on their own subscription, which is why R18 makes the posture a parameter of
  the start verb.
- **Version:** gc 1.4.1; agent Claude Code 2.1.261.
- **Date:** 2026-09-05.
- **Implies:** R18 — `start` passes model, name **and the fleet's permission
  posture** on every call, and the default is never the agent's. The posture is
  the adapter's to set, which is why it is a parameter of the verb rather than
  a setting somewhere.
- **Test:** `lessons::the_posture_is_the_adapters_to_set`

### G12. The first session died on the agent's workspace-trust dialog and was re-created every few seconds

- **Fact:** On a fresh install in a fresh directory, the always-on session died
  at startup on the agent's own workspace-trust question ("Is this a project you
  created or one you trust?"), the pane exiting non-zero, and the reconciler
  re-created it every few seconds. The only readable trace was a per-session
  stderr log file — nothing in the controller's log, nothing in the events. The
  remedy was to trust each folder once by hand. This is the same class as a
  session stopped in front of any dialog: what the controller reads is that the
  session exited, and **why** it exited sits in a dialog inside the pane, so
  the re-create runs on every tick.
- **Version:** gc 1.4.1; agent Claude Code 2.1.261.
- **Date:** 2026-09-05.
- **Implies:** R26 — the projection's per-seat rows carry the roster state, and
  the agent adapter reports the block's cause where the substrate names it.
  R14 — three consecutive blind dispatches halt the seat, the counter persists
  and decays, and a halt is announced once per transition; a retry loop with no
  ceiling is the failure this requirement exists to prevent. R1 — a
  first-run gate belongs in install, answered in the minute the service loads,
  not discovered by a seat that cannot start.
- **Test:** `lessons::a_first_run_meets_the_trust_dialog` — shared with
  claude-code.md's A15, which states the same behaviour as the agent's rather
  than the engine's. One behaviour, one fixture.

### G13. Crashes restarted within a tick and left no event at all

- **Fact:** Six kills were attempted 45 s apart; four landed, because the pane
  keeps a dead process id until the reconciler's next tick, so a loop that
  kills by pane pid lands every other round. Restarts came in 12, 57 and 55
  seconds, each a **fresh** session with a new transcript and no resume, first
  tool call 6–7 s after the restart. The events stream carried the three wakes
  and **no crash event of any kind** — a crash is not an event here, only the
  wake that follows it — and the controller's log carried three success lines
  and no cause.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R25 — the event stream is written by the layer that did the
  thing, once, and a crash is one of the events fleet emits that the engine
  does not. R14 — fleet writes a record for every restart, because a restart
  with no record is one its own counter cannot see.
- **Test:** `lessons::a_crash_is_an_event`

### G14. The fourth crash in four minutes triggered a hold the published behaviour does not describe, and a restart cleared it

- **Fact:** After the fourth landed kill the seat went to "asleep, reason
  context-churn" and stayed there — no restart then or in the minutes after.
  The published behaviour describes a different guard (five restarts in a
  one-hour window, then quarantine), so this one fires earlier than documented
  or is a different guard entirely. The verdict lived **only** in an always-on
  reconciler trace under the runtime directory (1.48 MB and 3,913 records after
  thirty-five minutes of an idle one-agent city), restated on every tick with
  the label and no rule and no reason text; it was in neither the log nor the
  events. A controller restart cleared it and the seat woke three seconds
  later, which matches the documented in-memory crash tracker.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R14 — the halt latch **persists across restarts and decays
  rather than clears**, one explicit command is the only remedy, and the halt
  is announced once per transition. Fleet's hold is written to the log and to
  the events and survives a controller restart, so a person can see it and
  clear it deliberately.
- **Test:** `lessons::a_hold_persists_and_is_announced`

### G15. The idle timeout never fired for an interactive agent session

- **Fact:** With a ten-minute idle timeout applied and readable through the
  config-explain command, the seat was judged "awake" on every tick twenty-one
  minutes after its last turn: no kill, no idle event, the process id
  unchanged. The multiplexer's own session-level activity read 816 seconds
  stale while its **window**-level activity had moved with nothing typed and
  nothing in the transcript. Hypothesis, not proven: the provider reads window
  activity for its last-activity probe, and the agent's terminal UI repaints —
  its status line, its clock — keep that fresh, so an interactive agent session
  can never read idle. The published caveat describes the shape of the failure
  ("if the provider doesn't support activity tracking, idle detection silently
  does nothing") without naming this case.
- **Version:** gc 1.4.1; agent Claude Code 2.1.261.
- **Date:** 2026-09-05.
- **Implies:** P1's idle reclaim — a seat idle past a per-class bound is
  *suggested* rest, never killed, and the bound is read from something that can
  actually be idle: the seat's own transcript, which is the same source R7
  already reads for context. Which clock a timeout probe reads is named in the
  requirement and verified at every use.
- **Test:** `lessons::idle_detection_needs_a_probe_that_can_be_idle`

### G16. A session's logs could not be read at all when its working directory was the config root

- **Fact:** Asking for one session's logs was refused: "session has no
  session_key and workdir fallback is ambiguous" — for an agent whose working
  directory is the city itself, which is the default the wizard wrote. So for
  the working directory the wizard writes by default, that command is not the
  path to a session's logs.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R26 and R7 — fleet reads a session's transcript by session id
  through the adapter's `transcript` verb, and the session id comes from the
  session table (R17), so there is no fallback to be ambiguous about.
- **Test:** `lessons::a_sessions_log_is_readable_by_id`

---

## D. Events, the stream, and what a manager can see

### G17. Eighty percent of the event stream was the controller reporting on itself

- **Fact:** The city's stream held 676 rows after four hours: 270 order-fired,
  270 order-completed, 49 issues created, 42 closed, 35 updated, 5 session
  wakes, 1 stop, 3 controller starts, 1 identity stamp. Every row carried a
  sequence number, a type, a timestamp and an actor; a subject on 672, a
  payload on 128, a session id on 6. What a person actually asks about was
  **absent**: no crash (none of four), no nudge (none of two), no adoption
  (none), no churn or idle verdict (those live only in the reconciler trace),
  no token cost. A separate machine-level stream held 655 rows, 647 of them one
  per HTTP call to its own loopback API. The public export envelope strips the
  subject and the payload and hashes the actor, so a manager fed by it would
  see that something of type `session.woke` happened and not to whom.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R25 — the event stream is fleet's first-class surface, in one
  envelope with one vocabulary, and an event is written by the layer that did
  the thing, once. The lesson is about **content**, not shape: fleet emits the
  events a person cares about — a seat woke after a crash, a nudge landed, a
  session was adopted, a seat was held — and keeps the controller's own
  housekeeping out of the same channel or out of the way.
- **Test:** `lessons::the_stream_carries_what_a_person_asks_about`

### G18. The stream's transport is exactly what a manager wants

- **Fact:** Four minutes of the server-sent-events endpoint on loopback, with
  no authentication asked: 15 heartbeat frames with a UTC timestamp every 15
  seconds, and 48 event frames each carrying an id equal to its sequence number
  and a JSON data line of the same shape as the stored event. Same origin as
  the dashboard. The service also publishes an OpenAPI 3.1.0 document — 127
  paths, 700 KB — with a documentation page beside it.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** P1's SSE surface over the event stream — heartbeats every
  fifteen seconds, resumable ids. Copied: the transport is right and there is
  no reason to invent another. The content is R25's problem, not this one's.
- **Test:** `lessons::the_transport_is_sse_with_resumable_ids`

### G19. A nudge arrived in under two seconds and left no trace

- **Fact:** Clocked against a timestamp printed after the command returned, the
  message reached the receiving session's transcript in **under two seconds**.
  The gap after arrival — 5.4 s and 6.9 s to the first tool call across two
  sends — is the agent's own safe-boundary queueing of a deferred reminder and
  not the transport: the message is delivered into the interactive session's
  input, and the agent holds it until the current turn ends. Against a direct
  cross-session transport measured at 10–16 ms receiver-append, the engine's
  delivery is one to two orders slower and still well inside anything a seat
  notices; both then wait on the same queue. The engine recorded nothing: no
  event in either stream, and its own nudge-status command read zero pending,
  zero in flight, zero dead.
- **Version:** gc 1.4.1; agent Claude Code 2.1.261.
- **Date:** 2026-09-05.
- **Implies:** R25 — a nudge is one of the events fleet emits and the engine
  does not. The adapter's `nudge` verb is a throwaway session whose single act
  is one cross-session send, carrying the invocation and the authority it
  names and no authority of its own; the delivery latency is not the design
  constraint, the record of it is.
- **Test:** `lessons::a_nudge_is_an_event`

---

## E. Orders

### G20. An order is one flat file, and it came with a history

- **Fact:** A hand-written order — a cron schedule and one exec action —
  fired five times, exactly two minutes apart, seventeen seconds past each
  boundary, which is the reconciliation tick's own phase rather than a
  scheduler's drift. The history command listed all five with one tracking
  issue each, and the event stream carried a fired and a completed for the
  subject. That is a scheduler's job done in one file and one reload, with a
  per-run record where a plain ledger line would otherwise be. One correction
  worth carrying: the working format is a flat `orders/<name>.toml`, not the
  `orders/<name>/order.toml` the glossary describes — the format was read off
  the engine's own built-in order files, not off its documentation. The cost
  side is that each fire creates and closes an issue in the engine's store,
  which a later reaper order prunes.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R22 — `orders/<name>.toml` from the install, every pack and
  every project, evaluated each tick, with cron, cooldown and condition. R23 —
  the actions, one ledger row per tick, and `could-not-tell` as a third
  outcome. R24 — `fleet routine list | check | run | history`. The flat format is
  adopted; the history command is the half the reference fleet did not have.
- **Test:** `lessons::an_order_is_one_flat_file_with_a_history`

---

## F. Cost

### G21. The prompt floor was paid on every restart

- **Fact:** A session start on the minimal template cost about 36,360
  cache-read tokens before the agent did anything, because the rendered prompt
  carries an injected skills section on top of the template's own. Four crashes
  therefore cost about 146K tokens of prompt before any work. Across six
  transcripts of a seat whose only work was to print its working directory four
  times: 2,350,565 cache-read, 220,925 cache-create, 55,715 output — on one
  person's own subscription. At rest with one idle agent and one orphan, the
  resident memory was about 127 MB for the supervisor, 188 MB for its database
  server, 4 MB for the multiplexer and roughly 290 MB per agent session. These
  figures are the sum of every assistant turn's usage field per transcript;
  they double-count nothing and they are not a billing figure.
- **Version:** gc 1.4.1; agent Claude Code 2.1.261.
- **Date:** 2026-09-05.
- **Implies:** R14 — a restart is not free, so a halt that stops a restart loop
  is a cost control as much as a safety one. R21 — the rest suggestion exists
  because a session's context is money, and the same arithmetic is what makes
  a crash-restart loop expensive rather than merely untidy. Fleet's own first
  turn is a brief, and its size is a number worth watching for the same reason.
- **Test:** `lessons::the_prompt_floor_is_paid_on_every_restart`

---

## G. Recorded as design input

### G22. An external store endpoint names a directory, not a database

- **Fact:** Pinning a project's store to a database server outside the engine
  points it at a **running server whose data directory is somebody else's**:
  the layout is `<data-dir>/<database>/.dolt`, so an external endpoint pointed
  at an existing fleet's server would create the new database *inside that
  fleet's own store directory*. The alternatives are to leave the project on
  the engine's managed server and migrate later by export and import, or to
  install a second server on its own port and data directory first.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R3 — the controller never initialises and never rewrites a
  project's work-graph store, and a project's store endpoint is the project's
  declaration to make. R1 — fleet's own store belongs to fleet's own install,
  which is where a fleet-scoped server is created if one is wanted at all.
- **Test:** `lessons::a_store_endpoint_names_a_directory`

### G23. Where the config lives is a mode, not a default

- **Fact:** Gas City separates its own repository — config, packs, agents,
  formulas and orders committed, runtime and ledger ignored — from the project,
  which is registered by path and carries only a small scoped footprint. The
  reference fleet does the opposite and ships the whole factory inside the
  product repository. Neither is wrong; they answer different questions, and
  the question was put in one sentence by somebody looking at their own repo:
  *if I onboard a contributor, they see all this harness material that is not
  part of the product itself.* Recorded as an input, not a decision — the cost
  of the split is two repositories kept in step and every tool that assumed one
  root.
- **Version:** gc 1.4.1.
- **Date:** 2026-09-05.
- **Implies:** R3 — fleet answers it as a **mode** rather than a default:
  embedded (the config at the project's root, fleet and project one directory)
  and standalone (the fleet's own repository, projects registered), decided by
  where `fleet.toml` is found, with one command that creates either. The
  person chooses; fleet does not choose for them.
- **Test:** `none — a recorded direction, not a behaviour; R3's two modes are
  tested by the install and create slices, not by this entry.`

### G24. A pack is a folder in one format, and the format is small enough to take whole

- **Fact:** A pack is a folder holding a manifest and up to seven slot
  directories, and the bundled core pack's top level is exactly those eight
  names and nothing else: `pack.toml`, `agents/`, `skills/`, `orders/`,
  `formulas/`, `doctor/`, `overlay/`, `assets/`. The manifest's `[pack]` table
  carries `name`, `version`, `schema` and `description`, `schema` being `2`
  today. `[imports.<name>]` sub-tables each carry `source` and `version` as
  strings — a `<git url>//<subdir>` source and a caret range such as `^0.4`.
  `[[named_session]]` entries carry `template` and `mode`, and `scope` is
  present on some and absent on others: the imported pack's five entries carry
  it and the fleet's own entry does not, so a parser that requires it refuses a
  real file. An agent directory holds `agent.toml` in the bundled pack and
  `prompt.template.md` in the imported one, and both forms are live. A skill is
  `skills/<name>/SKILL.md`; a health check is `doctor/<name>/doctor.toml` with
  an optional `run.sh`; orders and formulas are `<name>.toml`; the overlay is
  `overlay/per-provider/<provider>/`, nine providers in the bundled pack;
  `assets/` holds whatever the above read. The lock file beside the fleet's own
  manifest is `schema = 1` and one `[packs."<name>"]` table per installed pack,
  each with `version`, a forty-hex `commit` and an RFC 3339 `fetched`. A
  cross-pack agent name collision is a hard error rather than a precedence
  question — stated in the imported pack's own manifest comment and repeated in
  its README. The eight-name rule is core's alone, and fleet keeps it by
  Alberto's ruling on the decide bead filed off the pack-check landing: the
  `gastown` pack in the same packs repository carries more at its top level
  (`README.md`, `REQUIREMENTS.md`, `commands/`, `roles/`, `schemas/`,
  `template-fragments/`, `tests/`), a `global` table in its manifest and a
  doctor entry with no `doctor.toml`, so `fleet pack check` refuses it and
  every pack written to that wider practice — by design, not by defect, because
  fleet's own packs are written new and a typo'd slot has to be a defect.
- **Version:** the bundled core pack at gascity commit f895c0ff; the packs
  repository at 0.4.0, commit f69ec02b.
- **Date:** 2026-09-08.
- **Implies:** R1 and R2 — the format is theirs verbatim, so `fleet pack check`
  validates against these eight names and this schema number rather than
  against a shape of fleet's own, and the resolver refuses a duplicate agent
  name across layers instead of letting the higher one win. R3 — the lock
  file's keys are the ones above, so a pack installed by git source pins the
  same three fields they pin.
- **Test:** `lessons::a_pack_is_a_folder_with_eight_slots`

---

## Test inventory

Every fixture test named above, once. The scaffold reads this table; each
slice that lands the code a fact exercises writes the test under this exact
name.

| Test | Entry |
| --- | --- |
| `lessons::install_does_not_start_anything` | G1 |
| `lessons::telemetry_is_off_and_asked` | G2 |
| `lessons::a_minimal_template_is_minimal` | G3 |
| `lessons::an_existing_store_is_never_reinitialised` | G4 |
| `lessons::the_controller_never_commits_to_a_project` | G5 |
| `lessons::the_first_doctor_reports_on_the_person_not_the_installer` | G6 |
| `lessons::a_restart_adopts_and_says_so` | G7 |
| `lessons::stop_and_start_are_inverses` | G8 |
| `lessons::a_stop_that_cannot_stop_says_so` | G9 |
| `lessons::config_is_reread_without_a_restart` | G10 |
| `lessons::the_posture_is_the_adapters_to_set` | G11 |
| `lessons::a_first_run_meets_the_trust_dialog` | G12 |
| `lessons::a_crash_is_an_event` | G13 |
| `lessons::a_hold_persists_and_is_announced` | G14 |
| `lessons::idle_detection_needs_a_probe_that_can_be_idle` | G15 |
| `lessons::a_sessions_log_is_readable_by_id` | G16 |
| `lessons::the_stream_carries_what_a_person_asks_about` | G17 |
| `lessons::the_transport_is_sse_with_resumable_ids` | G18 |
| `lessons::a_nudge_is_an_event` | G19 |
| `lessons::an_order_is_one_flat_file_with_a_history` | G20 |
| `lessons::the_prompt_floor_is_paid_on_every_restart` | G21 |
| `lessons::a_store_endpoint_names_a_directory` | G22 |
| `lessons::a_pack_is_a_folder_with_eight_slots` | G24 |

G23 names no test; its five-field entry says why.
