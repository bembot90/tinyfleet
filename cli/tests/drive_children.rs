//! The children a poll leaves behind, and the counters that read them.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

/// The ceiling on a wait for the controller's published document, AND NEVER A
/// READING.
///
/// It exists so a controller that never publishes fails the arm instead of
/// hanging the lane, which is the same job `witness_within`'s bound does. Sixty,
/// under the 80 s period the profile bounds an arm with
/// (`fleet/.config/nextest.toml`), so an arm that reaches this ceiling refuses
/// in its own words rather than being killed by the runner without one. The
/// twenty seconds this replaced was reached by a loaded box twice in one
/// eighteen-run census, and answered a bare `false` that said nothing about how
/// close it had come.
const POLL_BOUND: Duration = Duration::from_secs(60);

/// The deadline the three PATIENT rigs run their agent calls on, AND NEVER A
/// READING either.
///
/// A patient rig exists to sit through its holder's whole life and read what
/// the holder delayed; it is the bounded rig beside it that reads a deadline. By
/// omitting `agent_timeout_ms` these three ran on the controller's 20 s
/// production default, which a loaded box reached — the patient held-pipe rig
/// published `unknown` where its arm asserts `present`, and the two patient
/// escapee rigs spent 27 and 39 seconds on a single discarded attempt. Sixty is
/// an anti-hang ceiling so a holder that never dies fails the arm instead of
/// hanging it, and the guard arm below holds it above every holder it must
/// outlast.
const PATIENT_DEADLINE_MS: u64 = 60_000;

/// The life of the descendant that holds the LISTING call's pipe from inside the
/// group, read at the two sites that spend it.
///
/// At file scope and not in each arm, because the guard arm holds
/// `PATIENT_DEADLINE_MS` above it: a life raised in an arm's own `const` would
/// move the patient rig's ceiling out from under it with the guard still green.
const HELD_PIPE_DESCENDANT_SECONDS: u64 = 10;

/// The shape that separates the two settles: a count that GROWS for the whole
/// window. Settling in both directions answers with the last reading it took —
/// a figure from three seconds after the moment its caller named — and this one
/// answers with the reading at the instant.
///
/// Both halves are asserted, because either alone is satisfied by something
/// else: the VALUE alone passes over a helper that reads once and never settles,
/// and the CALL COUNT alone passes over the helper this one replaced.
#[test]
fn a_count_that_grows_through_the_window_is_answered_at_the_instant() {
    let reads = AtomicUsize::new(0);
    // 1, 2, 3, … — a leak that never stops, which is what the arm reading this
    // helper is written to catch.
    let growing = || 1 + reads.fetch_add(1, Ordering::SeqCst);

    let started = Instant::now();
    let observed = settled_toward_zero(growing);
    let elapsed = started.elapsed();
    let taken = reads.load(Ordering::SeqCst);

    assert_eq!(
        observed, 1,
        "the count read 1 at the instant and grew by one per read to {taken}: \
         the answer is the instant's reading, never the window's last"
    );
    assert!(
        taken > 1,
        "the settle was never entered, so this arm read a helper that answers \
         once rather than one that settles: {taken} read(s)"
    );
    assert!(
        elapsed >= SETTLE_WINDOW,
        "a count that never reaches zero is given the whole window: {elapsed:?}"
    );
}

/// The settle's one purpose, which is a reap already in flight: a count that is
/// nonzero when it is asked for and zero a moment later is a zero, not the
/// nonzero the instant held.
#[test]
fn a_count_that_reaches_zero_inside_the_window_is_answered_zero() {
    let reads = AtomicUsize::new(0);
    // Nonzero for the first two reads and zero after: the reap lands inside the
    // window and not before it.
    let reaping = || {
        if reads.fetch_add(1, Ordering::SeqCst) < 2 {
            1
        } else {
            0
        }
    };

    let observed = settled_toward_zero(reaping);
    let taken = reads.load(Ordering::SeqCst);

    assert_eq!(
        observed, 0,
        "the count reached zero on read three of {taken}: a reap in flight is a \
         zero and not the 1 the instant held"
    );
    assert!(
        taken > 1,
        "the settle was never entered, so the zero above is the first read's \
         and not the window's: {taken} read(s)"
    );
}

/// A sibling's kill-to-wait window is a Z-state child of this process, and no
/// other arm's reading may be broken by it.
///
/// `cargo test` runs every arm of this binary as a thread of ONE process, so a
/// child between its exit and its `wait` is a zombie child of that process for
/// the whole window — and under load that window sits open for seconds with
/// nothing wrong. This arm holds one open longer than any settle here, so a
/// reading taken as a COUNT over this process's children reds against it on a
/// quiet box rather than once a quarter on a loaded one. Every zombie reading
/// in this binary is a pid the reading arm spawned, for that reason.
#[test]
fn a_siblings_kill_to_wait_window_outlives_the_settle_and_is_not_a_leak() {
    let mut held = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("a shell runs");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !is_a_zombie(held.id()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        is_a_zombie(held.id()),
        "the child this arm holds unwaited never reached Z, so the window below \
         is not the one this arm names, at pid {}",
        held.id()
    );

    // Longer than `settled_toward_zero`'s own window, which is the whole span a
    // count over this process would give a sibling to clear. Derived from that
    // constant and not stated beside it: a slice that moves the settle moves
    // this hold with it, where a literal would quietly stop outliving it.
    std::thread::sleep(SETTLE_WINDOW + Duration::from_secs(1));

    let pid = held.id();
    held.wait().expect("the held child is reaped");
    assert!(
        !is_a_zombie(pid),
        "the wait left the child in Z at pid {pid}: this arm's own window never \
         closes, which is a leak and not the sibling window it names"
    );
}

/// The accumulation clause of the reap, read where it is claimed. The in-process
/// arm above reads one call's child inside this test binary; a zombie that costs
/// anything is one entry per outrun poll of a RUNNING controller, so the reading
/// is the loop process's own Z-state children after two polls it outran.
///
/// THE SURFACE IS THIS ARM'S DISTINCT REACH, and no subject mutant separates it
/// from the in-process arm: both run the same `run_bounded`, so every reap
/// defect reachable there reds both. What only this arm reads is a LONG-LIVED
/// process's count ACROSS polls — the cost the claim is about, which one call
/// inside this binary cannot show whatever the reap does. The two readings
/// below are what make that a measurement. Each is the count AT THE INSTANT a
/// poll was observed, settled only toward zero; the first at the first poll this
/// arm sees outrun, and the second at the next poll observed AFTER that first
/// count came back. Neither term claims an ordinal in the controller's own
/// sequence, because this arm cannot see the polls it did not wait for — what it
/// claims is that the second reading follows the first by at least one published
/// poll. What that settle does and does not cover is on `settled_toward_zero`,
/// and the line worth carrying here is that a reap DEFERRED rather than removed
/// is outside this arm's claim.
///
/// NO ASSERT HERE RELATES THE TWO TERMS. Each is judged on its own against zero,
/// and the relation is carried by the messages rather than by an equality: every
/// red prints both figures, so a reader can tell a leak that stopped after one
/// call from one that runs per poll. A second term above the first is that
/// reader's evidence, not a claim this arm holds.
///
/// ONE LIVENESS READ, TAKEN AFTER BOTH COUNTS, AND IT IS THE WHOLE REACH.
/// `Controller::exited` is a `try_wait` and a process does not un-exit, so a
/// `None` there says the loop was running at every instant this arm read a count.
/// A second read placed before the second count can catch no state that one
/// misses, so there is one; the pid control beside it is what separates the
/// counts' subject from this binary's own.
///
/// THE SECOND WAIT IS KEYED ON A STAMP RE-READ AFTER THE FIRST COUNT. The count's
/// own settle runs its whole window under a leak, several polls publish inside
/// it, and a wait keyed on a stamp taken before that count is already satisfied
/// when it starts: it returns without waiting for anything, and the second term
/// is a much later poll's count wearing the label of the next one.
#[test]
fn a_running_controller_holds_no_zombie_children_across_polls() {
    let mut rig = Rig::new("zombie-loop");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.agent_timeout_ms = Some(200);
    rig.hang_seconds = Some(5);

    // The probe's positive control for a parent that is NOT this process, taken
    // before the subject: a shell that execs itself away into `sleep` has
    // nothing left to wait on its background child, and that child IS counted —
    // so the zero below is a reading and not a probe that only answers for the
    // pid it runs in.
    //
    // ITS STREAMS ARE /dev/null AND NOT THIS PROCESS'S. The backgrounded child
    // is unwaited by design and outlives the kill below, and a process holding
    // the harness's own pipes after the arm has returned is read as a leaked
    // test.
    let mut unreaping = Command::new("/bin/sh")
        .args(["-c", "sleep 1 & exec sleep 5"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("a shell runs");
    let deadline = Instant::now() + Duration::from_secs(4);
    while zombie_children_of(unreaping.id()) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        zombie_children_of(unreaping.id()) >= 1,
        "the probe reads another process's child left unwaited on purpose"
    );
    let _ = unreaping.kill();
    let _ = unreaping.wait();

    let mut controller = rig.spawn_loop();
    assert!(
        rig.wait_until(|p| p["seats"][0]["roster_state"] == "unknown"),
        "the loop publishes a poll whose listing it outran"
    );
    // The unknown has to be the DEADLINE's. An unknown from a stub that could
    // not be run at all spawns nothing to reap, and the count below would be a
    // zero about that instead.
    let after_first = rig.projection();
    assert!(
        after_first["seats"][0]["roster_unknown_cause"]
            .as_str()
            .unwrap_or_default()
            .contains("did not answer within"),
        "poll one's unknown names the deadline: {}",
        after_first["seats"][0]["roster_unknown_cause"]
    );
    // The first term of the accumulation, taken before the next poll: one count
    // cannot tell a leak that stops from one that is per poll. Each term carries
    // the pid it was read at, which is the only thing the control below judges.
    let (at_a_poll, first_term_pid) = zombie_children_at_with_pid(controller.pid());

    // The next poll is read from the document's own stamp rather than from a
    // sleep, and the stamp is re-read HERE — after the count above, never from
    // the projection that preceded it. A count that settles for its whole window
    // lets several polls publish inside it, and a wait keyed on the older stamp
    // is satisfied before it begins.
    let before_the_next = rig.projection()["generated_at"].clone();
    assert!(
        rig.wait_until(move |p| p["generated_at"] != before_the_next),
        "the loop publishes another poll after the first count was taken"
    );

    let after_second = rig.projection();
    assert!(
        after_second["seats"][0]["roster_unknown_cause"]
            .as_str()
            .unwrap_or_default()
            .contains("did not answer within"),
        "the next poll's unknown names the deadline: {}",
        after_second["seats"][0]["roster_unknown_cause"]
    );
    // Both terms are read before either is judged, and each message carries the
    // pair: a single figure cannot say whether a leak stopped after one call.
    let (at_a_later_poll, second_term_pid) = zombie_children_at_with_pid(controller.pid());

    // Liveness, once, after both counts and covering both: a controller that
    // dies inside either count's window reparents its children away, so the
    // count reads zero about an absent parent instead of about one that reaps.
    // The handle is what answers it — a dead child this process has not waited
    // on is a zombie, which a pid liveness check reads as alive. The message is
    // worded for a nonzero term too, because the condition reads only the exit.
    assert!(
        controller.exited().is_none(),
        "the controller has exited by this read, so neither term is established \
         as a reading about a running process: a dead parent reparents its \
         children away and a count taken of it answers about nobody \
         ({at_a_poll} at the poll this arm observed, {at_a_later_poll} at the \
         next one it observed)"
    );

    // The control for the pid the counts were taken AT. Both equalities below
    // are satisfied by a term read at this test binary, which holds no zombies
    // of its own, and nothing else here says the arm measured the controller.
    assert_eq!(
        (first_term_pid, second_term_pid),
        (controller.pid(), controller.pid()),
        "both terms are read from the controller's own process, not from this \
         test binary ({at_a_poll} at the poll this arm observed, \
         {at_a_later_poll} at the next one it observed)"
    );

    // Each figure is every Z-state child of the controller's process, which is
    // what `ps` can answer; nothing here reads which child. The outrun listing's
    // is what the messages name because it is the only child the rig gives the
    // loop to hold, and a figure that grows with the polls is the accumulation
    // whatever the entries turn out to be.
    assert_eq!(
        at_a_poll, 0,
        "the running controller holds a Z-state child — an outrun poll's \
         listing, on this rig (unreaped children: {at_a_poll} at the poll this \
         arm observed, {at_a_later_poll} at the next one it observed)"
    );
    assert_eq!(
        at_a_later_poll, 0,
        "the running controller accumulates Z-state children, one per outrun \
         poll — its listings, on this rig (unreaped children: {at_a_poll} at \
         the poll this arm observed, {at_a_later_poll} at the next one it \
         observed)"
    );
}

/// A listing whose direct child answers at once while a descendant it forked
/// still holds the inherited pipe. The exit path runs under the same deadline as
/// the wait, so the seat reads Unknown naming the deadline — a hang published as
/// absence is a listing that answered with an empty fleet, which is the reading
/// `RosterRead` exists to prevent.
///
/// WHAT THE RATIO BOUNDS, exactly: that the bounded LISTING CALL'S SPAN — its
/// start mark to the version call's — ended in a fraction of the DESCENDANT'S
/// LIFE, so the descendant is not what it waited for. It does not tie the call
/// to the seam — the assert tolerates a third of the patient span, about 3.3 s
/// against a 1000 ms seam, so a bound that had slipped to three times the seam
/// would still pass here.
///
/// THE SPAN AND NOT THE WHOLE POLL: a poll's process start-up moves with the box
/// by whole seconds while the listing's bound does not, and a start-up landing
/// on the bounded poll alone would red a bound that held. The two spans still
/// do not move together: the patient one has a floor the arm asserts — it sits
/// through the whole descendant — while the bounded one sits under the seam on
/// a base a tenth the size, so load moves it proportionally far more and the
/// headroom shrinks from one side only. Measured on this box: bounded 0.796 s,
/// patient 10.034 s.
///
/// TIGHTENING IT IS A WALL-CLOCK BOUND IN DISGUISE, and is declined. The patient
/// span is pinned to `HELD_PIPE_DESCENDANT_SECONDS` by the floor assert above, so
/// `bounded * K < unbounded` is arithmetically `bounded < 10s / K` however it is
/// spelled; a K close enough to bite — the seam plus the grace is 1.2 s, so
/// K = 8 — is a hard 1.25 s ceiling that this box's own 0.796 s clears by
/// 450 ms and a loaded one would not. The relative form buys freedom from the
/// DESCENDANT's timing, never from the seam's, and there is nothing else in
/// this arm to measure the seam against.
///
/// The seam sits far above the child's own exit and far below the descendant's
/// life on purpose. A seam the child can outrun puts this arm on the deadline
/// branch, where it would pass without reading the exit path it exists for.
#[test]
fn a_descendant_holding_the_pipe_cannot_hold_the_poll_past_the_deadline() {
    // The unit, measured on this box rather than assumed: the same stub under a
    // deadline long enough to sit through the descendant.
    let mut patient = Rig::new("held-pipe-patient");
    patient.write_roster(&live_row(&patient.worktree(), "a-session"));
    patient.descendant_seconds = Some(HELD_PIPE_DESCENDANT_SECONDS);
    // Named, not omitted: by omission this ran on the controller's 20 s
    // production default, which a loaded box reached — the poll then published
    // `unknown` and the assertion below read it as the descendant not having
    // been what it waited for.
    patient.agent_timeout_ms = Some(PATIENT_DEADLINE_MS);
    // The span below is read from the listing branch's own start mark, and the
    // stub writes that mark a few microseconds after the one `witnessed` reads
    // — a descheduling apart on a loaded box. A kill landing in that gap
    // returns an attempt with no mark, which the span reader answers with a
    // panic. Owed here so such an attempt is DISCARDED instead, which is what
    // this arm's own doc comment says is to happen. The patient rig owes it
    // too: its long deadline makes the kill unlikely, not impossible.
    patient.owes_witness(&patient.listing_started_path());
    patient.clear_call_start_marks();
    assert_eq!(patient.observe().status.code(), Some(0));
    let unbounded = patient.listing_call_lasted();
    assert_eq!(
        patient.projection()["seats"][0]["roster_state"],
        "present",
        "the patient poll reads the listing whose descendant delayed it"
    );
    assert!(
        unbounded >= Duration::from_secs(HELD_PIPE_DESCENDANT_SECONDS),
        "the descendant is what the patient listing call waited for, and the \
         margin below is its lifetime: {unbounded:?}"
    );

    let mut rig = Rig::new("held-pipe-bounded");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.descendant_seconds = Some(HELD_PIPE_DESCENDANT_SECONDS);
    // The hoisted constant at the site that spends it, for the reason the
    // escapee arms give: an alias moves this line while the guard arm below
    // still reads the declaration and passes.
    rig.agent_timeout_ms = Some(HELD_PIPE_SEAM_MS);
    // The witness, for the reason the patient rig's own line gives — and this is
    // the rig the gap was measured on, because a 1000 ms deadline is the
    // shortest in the file.
    rig.owes_witness(&rig.listing_started_path());
    rig.clear_call_start_marks();
    assert_eq!(rig.observe().status.code(), Some(0));
    let bounded = rig.listing_call_lasted();

    let row = &rig.projection()["seats"][0];
    assert_eq!(
        row["roster_state"], "unknown",
        "a listing the deadline cut short is Unknown, never absent"
    );
    assert!(
        row["roster_unknown_cause"]
            .as_str()
            .unwrap_or_default()
            .contains("did not answer within"),
        "and the cause names the deadline: {}",
        row["roster_unknown_cause"]
    );
    assert!(
        bounded * BOUNDED_RATIO_MULTIPLE < unbounded,
        "the bounded listing call lasted {bounded:?}, which is not a fraction \
         of the patient one's {unbounded:?}: the descendant held it"
    );
}

/// The rescue the arm above rests on, INJECTED rather than waited for: the kill
/// landing between the stub's own start mark and the listing branch's, which is
/// what a loaded box produces now and then and what the span reader answers with
/// a panic.
///
/// The shape is the bounded held-pipe rig exactly, plus a one-shot preamble
/// longer than its deadline: the first listing call, after the start mark,
/// sleeps past the seam and is killed before its listing mark, so the attempt
/// leaves none; the next finds the seam consumed and runs the arm's own case.
///
/// THREE CLAIMS AND THE DISCARD'S REASON IS WHAT MAKES THE OTHERS MEAN
/// ANYTHING. Without it a wrapper that ignored the owed witness entirely would
/// pass every other line here on a run where the first attempt happened to reach
/// the mark. The reason and not the COUNT, and not the ORDER either: under load
/// the box discards attempts of its own on either side of the one this arm
/// injects — three runs of an eighteen-run census read a second discard, and one
/// run at load 34.6 read a box discard BEFORE the injected one — so a count
/// pinned at one and an ordinal pinned at first are both readings of the box.
/// That some discard carries the owed-witness reason is the arm's own
/// arrangement and nothing else's.
///
/// WHAT IS NOT ASSERTED IS THE SPAN AGAINST THE PREAMBLE, because the stub
/// sleeps the preamble BEFORE it writes the listing mark (`rig.rs`'s stub body),
/// so no attempt's listing-to-projection span can contain it and the comparison was
/// never the discard it claimed to read. What such a line bounds is the seam
/// plus the drain grace plus a spawn against the wall clock, which is a
/// wall-clock ceiling on a loaded box: it read 1.521527613 s against 1.5 s.
#[test]
fn a_kill_between_the_stubs_two_marks_once_is_rescued_by_the_patience() {
    /// Past `HELD_PIPE_SEAM_MS`, so the deadline lands inside the preamble and
    /// the invocation dies before its branch. Half a second of margin, which is
    /// the same order as the seams around it.
    const PREAMBLE_MS: u64 = 1500;

    let mut rig = Rig::new("held-pipe-preamble");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.descendant_seconds = Some(HELD_PIPE_DESCENDANT_SECONDS);
    rig.agent_timeout_ms = Some(HELD_PIPE_SEAM_MS);
    rig.one_shot_preamble_ms.set(Some(PREAMBLE_MS));
    rig.owes_witness(&rig.listing_started_path());
    rig.clear_call_start_marks();
    assert_eq!(rig.observe().status.code(), Some(0));

    // THE DISCARD IS READ BEFORE THE SPAN, because it is the span's
    // precondition: a wrapper that ignored the owed witness returns the delayed
    // attempt, which never reached a listing mark, and the span reader then
    // panics about a missing file where the defect is the discard that did not
    // happen. First claim first.
    let discards = rig.discard_reasons.borrow().clone();
    assert!(
        !discards.is_empty(),
        "no attempt was discarded, so the preamble never delayed one and the \
         rescue this arm names never happened"
    );
    // AMONG THE DISCARDS AND NOT THE FIRST OF THEM. The box discards attempts of
    // its own on either side of the injected one — measured at load 34.6, where
    // the first discard read "the stub never started" three seconds in and the
    // arm's own kill was the second — so an ordinal here is an assertion about
    // the box. What the arm arranged is that SOME attempt was thrown away for
    // the witness it owes, which is the preamble kill and nothing else: that
    // kill leaves the stub's own start mark and never the listing mark.
    let injected = "the stub never reached the witness this arm owes";
    assert!(
        discards.iter().any(|r| r.starts_with(injected)),
        "no discard was the injected one — the reasons read {discards:?}, and \
         none of them is {injected:?}: the preamble kill leaves the start mark \
         and never the listing mark, so the attempt this arm arranged is \
         discarded for that witness or it never happened"
    );
    // Every OTHER discard is the box's and not this arm's, so it is reported and
    // never asserted over: a count or an order that refused them would red on
    // the box.
    for reason in discards.iter().filter(|r| !r.starts_with(injected)) {
        eprintln!("the box discarded an attempt of its own beside the injected one: {reason}");
    }
    assert!(
        std::fs::read_to_string(rig.seam_path(PREAMBLE))
            .expect("the seam file is there to read")
            .is_empty(),
        "the stub consumed the one-shot preamble, so the kept attempt was not \
         delayed by it"
    );
    let span = rig.listing_call_lasted();
    assert!(
        !span.is_zero(),
        "the span read is a duration and not a pair of marks one instant apart"
    );
    assert_eq!(
        rig.projection()["seats"][0]["roster_state"],
        "unknown",
        "the kept attempt is the bounded held-pipe case this rig was built for"
    );
}

/// The other call. A poll asks the binary twice — once for the listing and once
/// for `--version` — and each call gets a pipe of its own, so a holder is a case
/// PER CALL and the arm above reads only the listing's. Here the `--version`
/// call's pipe is the held one while the listing answers normally, so one half
/// of the poll reads the deadline and the other does not.
///
/// WHAT TIES THE NULL TO THE SEAM is the time, because nothing else can:
/// `version()` answers an `Option` and drops the reason, and no cause for it
/// reaches the document or the log — so the reading that separates this null
/// from a `FAIL`, a `SILENT` or an unparsable answer is that the poll waited a
/// deadline and then gave up.
///
/// It is read on the VERSION CALL'S OWN SPAN — that branch's start mark to the
/// poll's return — and not as a difference between two whole polls: a poll pays
/// a process start-up and a listing call besides the wait, and those two move
/// with the box while the deadline does not, so a difference of two whole polls
/// charges that movement to the wait.
///
/// The control comes FIRST and on the same rig, and a warm-up poll comes before
/// even that: the held poll's LISTING call has to clear the same seam the
/// version call is outrun by, and a cold first exec on this box does not.
#[test]
fn a_descendant_holding_the_version_pipe_publishes_no_agent_version() {
    // The seam has to be clearable by the OTHER call. This arm's whole content is
    // that one call reads and one does not, so a seam the listing call cannot
    // clear turns the pairing into two deadlines and reds. The figure is this
    // arm's own, and the code couples it to nothing: the escapee seams carry the
    // same 3000 for a chain this arm's holder never pays — its descendant is a
    // plain in-group fork, no interpreter and no setsid — so a slice that moves
    // those leaves this one where it is. The holder's life is far above the seam,
    // so the version call is outrun whatever the box is doing.
    const DESCENDANT_SECONDS: u64 = 30;
    const SEAM_MS: u64 = 3000;

    let mut rig = Rig::new("held-version-pipe");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.agent_timeout_ms = Some(SEAM_MS);
    // The span below is the VERSION branch's, so the version mark is the witness
    // owed: a kill landing between the stub's own start mark and it returns an
    // attempt the span reader answers with a panic. `witnessed` removes the mark
    // before every attempt, which is what `clear_version_start_mark` does for
    // the reading below by hand.
    rig.owes_witness(&rig.version_started_path());

    // Two polls with nothing holding the pipe, and the assertion is on the
    // SECOND. The first pays what a first spawn costs here — the built binary and
    // this rig's freshly written stub each reach `exec` for the first time in it
    // — a cost no seam of a few seconds is clearable inside. `witnessed` does not
    // cover it: it re-runs a call whose stub never started, and a stub that
    // started slowly and then answered is a reading it keeps. The second poll is
    // the control on the DESCENDANT, that variable and no other: it reports, so
    // the null below is the holder's and not a rig that publishes no version.
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(
        rig.projection()["agent_version"],
        "9.9.9",
        "the same rig and the same seam with nothing holding the version pipe \
         report a version"
    );

    rig.version_descendant_seconds = Some(DESCENDANT_SECONDS);
    rig.clear_version_start_mark();
    assert_eq!(rig.observe().status.code(), Some(0));
    let bounded = rig.last_call();
    let waited = rig.version_call_lasted(SystemTime::now());

    let published = rig.projection();
    assert!(
        published["agent_version"].is_null(),
        "a version call whose pipe is still held answered nothing, and a \
         reading nobody took is null: {}",
        published["agent_version"]
    );
    // The listing call is the pair's other half, and its pipe was never held:
    // the seat still reads, so the null above is ONE call's and not a poll that
    // failed whole.
    assert_eq!(
        published["seats"][0]["roster_state"], "present",
        "the listing call is untouched by a holder on the version call's pipe"
    );
    assert!(
        bounded < Duration::from_secs(DESCENDANT_SECONDS),
        "the held version pipe held the poll for the descendant's whole life: \
         {bounded:?}"
    );

    // The tie: the span holds the wait and the poll's tail after it, and nothing
    // before the call. Half the seam from below, because a null published before
    // the seam elapsed is not this deadline's; twice it from above, because a
    // seam that had slipped even twofold is a deadline nobody set here.
    assert!(
        waited >= Duration::from_millis(SEAM_MS / 2),
        "the version call lasted {waited:?} inside a poll of {bounded:?}: the \
         null was published without waiting the {SEAM_MS} ms seam, so it is not \
         the deadline's"
    );
    assert!(
        waited < Duration::from_millis(SEAM_MS * 2),
        "the version call lasted {waited:?} inside a poll of {bounded:?}: that \
         is not the {SEAM_MS} ms seam this arm set, so the version call is \
         running on some other deadline"
    );
}

/// The holder the group kill cannot reach, on the EXIT path: a descendant that
/// called `setsid` and so left the group before the child answered. The kill
/// misses it and it keeps the pipe, so the bound that ends the poll is the
/// collect's and not the kill's.
///
/// The seam is well above the child's own exit for the reason D3 named, and the
/// escape is witnessed rather than raced: the stub does not answer until the
/// escapee has left the group, so this arm cannot quietly become the in-group
/// case it was written to sit beside.
///
/// `ESCAPEE_EXIT_SEAM_MS` IS 3000 AND NOT 1000. The escape is a `sh` and a `python3` start
/// plus `setsid` under whatever load the full suite puts on the
/// box, and the seam's clock runs from the spawn: a seam the escape does not
/// clear kills the escapee while it is still in the group and the escaped file
/// is never written. THE SEAM IS NOT WHAT DECIDES THE ARM, though: a call whose
/// escape left no witness took no reading of the case, so `Rig::witnessed`
/// discards it and re-attempts under the stub-start patience. The figure buys
/// the ordinary case its margin — every discarded attempt costs a whole seam —
/// and the arm's correctness rests on the witness instead. The ceiling
/// is the ratio below — `bounded` is the listing call's span, about the seam
/// plus the drain grace, so a seam above about 3.3 s makes the ratio false at
/// `ESCAPEE_SECONDS` = 10, and raising the seam past that means raising the
/// escapee's life with it.
#[test]
fn an_escaped_descendant_cannot_hold_the_poll_on_the_exit_path() {
    // THROUGH THE BUILT BINARY, both rigs. The escapee leaves its group to
    // outlive the poll, so a poll run in this process leaves it holding the test
    // binary's own streams for the rest of its life.
    let mut patient = Rig::new("escapee-exit-patient");
    patient.write_roster(&live_row(&patient.worktree(), "a-session"));
    patient.escapee_seconds = Some(ESCAPEE_SECONDS);
    // Named, not omitted, for the reason the held-pipe patient's own line gives:
    // by omission this ran on the controller's 20 s production default, and a
    // loaded box spent 39 s on one discarded attempt of it.
    patient.agent_timeout_ms = Some(PATIENT_DEADLINE_MS);
    // The listing branch's start mark, owed for the reason the held-pipe arm's
    // own line gives: the span below is read from it, and an attempt killed
    // before it is discarded rather than read as a panic. The escape witness is
    // already owed here by `escapee_seconds`; this is the third.
    patient.owes_witness(&patient.listing_started_path());
    patient.clear_call_start_marks();
    assert_eq!(patient.observe_out_of_process().status.code(), Some(0));
    let unbounded = patient.listing_call_lasted();
    assert!(
        patient.escaped_path().exists(),
        "the escapee left the group and this arm read the case it names"
    );
    assert!(
        unbounded >= Duration::from_secs(ESCAPEE_SECONDS),
        "the escapee is what the patient listing call waited for, and the \
         margin below is its life: {unbounded:?}"
    );

    let mut rig = Rig::new("escapee-exit-bounded");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.escapee_seconds = Some(ESCAPEE_SECONDS);
    // The hoisted constant is read AT THE SITE THAT SPENDS IT. Through a local
    // alias, the guard arm below pins the declaration while this line is free to
    // carry any figure at all.
    rig.agent_timeout_ms = Some(ESCAPEE_EXIT_SEAM_MS);
    // The witness, for the reason the patient rig's own line gives.
    rig.owes_witness(&rig.listing_started_path());
    rig.clear_call_start_marks();
    assert_eq!(rig.observe_out_of_process().status.code(), Some(0));
    let bounded = rig.listing_call_lasted();
    assert!(
        rig.escaped_path().exists(),
        "the escapee left the group here too"
    );

    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "unknown");
    // The FIGURE and not the sentence: the seam this arm set is what the poll
    // has to have run on, and a cause naming any other deadline is the built
    // binary reading some seam that is not this one. Rendered through the same
    // `Duration` formatter the cause is built with, off this arm's own constant.
    let names_the_seam = format!(
        "did not answer within {:?}",
        Duration::from_millis(ESCAPEE_EXIT_SEAM_MS)
    );
    assert!(
        row["roster_unknown_cause"]
            .as_str()
            .unwrap_or_default()
            .contains(&names_the_seam),
        "the cause names this arm's own deadline ({names_the_seam}): {}",
        row["roster_unknown_cause"]
    );
    assert!(
        bounded * BOUNDED_RATIO_MULTIPLE < unbounded,
        "the listing call lasted {bounded:?} against the patient one's \
         {unbounded:?}: a holder outside the group held it"
    );
}

/// The same holder on the DEADLINE path, where the base detached and a join
/// would be a regression against it: the child hangs past the seam AND an
/// escapee holds the pipe, so the kill lands and misses, and only the grace
/// ends the collect.
///
/// `ESCAPEE_DEADLINE_SEAM_MS` is 3000 for the reason the exit path's docstring gives, and the
/// same ceiling applies: an escape that does not clear the seam costs this arm
/// a discarded attempt, and the ratio below, over the listing call's span, caps
/// how far the seam can be raised while the escapee lives `ESCAPEE_SECONDS`.
#[test]
fn an_escaped_descendant_cannot_hold_the_poll_on_the_deadline_path() {
    // THROUGH THE BUILT BINARY, both rigs. The escapee leaves its group to
    // outlive the poll, so a poll run in this process leaves it holding the test
    // binary's own streams for the rest of its life.
    let mut patient = Rig::new("escapee-deadline-patient");
    patient.write_roster(&live_row(&patient.worktree(), "a-session"));
    patient.escapee_seconds = Some(ESCAPEE_SECONDS);
    patient.hang_seconds = Some(ESCAPEE_SECONDS);
    // Named, not omitted, for the reason the exit path's twin gives.
    patient.agent_timeout_ms = Some(PATIENT_DEADLINE_MS);
    // The witness, for the reason the exit path's twin gives.
    patient.owes_witness(&patient.listing_started_path());
    patient.clear_call_start_marks();
    assert_eq!(patient.observe_out_of_process().status.code(), Some(0));
    let unbounded = patient.listing_call_lasted();
    assert!(
        patient.escaped_path().exists(),
        "the escapee left the group"
    );
    assert!(
        unbounded >= Duration::from_secs(ESCAPEE_SECONDS),
        "the hang and the escapee are what the patient listing call waited \
         for: {unbounded:?}"
    );

    let mut rig = Rig::new("escapee-deadline-bounded");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.escapee_seconds = Some(ESCAPEE_SECONDS);
    rig.hang_seconds = Some(ESCAPEE_SECONDS);
    // Read at the site that spends it, for the reason the exit path's twin
    // gives.
    rig.agent_timeout_ms = Some(ESCAPEE_DEADLINE_SEAM_MS);
    // The witness, for the reason the exit path's twin gives.
    rig.owes_witness(&rig.listing_started_path());
    rig.clear_call_start_marks();
    assert_eq!(rig.observe_out_of_process().status.code(), Some(0));
    let bounded = rig.listing_call_lasted();
    assert!(
        rig.escaped_path().exists(),
        "the escapee left the group here too"
    );

    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "unknown");
    // The figure, for the reason the exit path's twin gives.
    let names_the_seam = format!(
        "did not answer within {:?}",
        Duration::from_millis(ESCAPEE_DEADLINE_SEAM_MS)
    );
    assert!(
        row["roster_unknown_cause"]
            .as_str()
            .unwrap_or_default()
            .contains(&names_the_seam),
        "the cause names this arm's own deadline ({names_the_seam}): {}",
        row["roster_unknown_cause"]
    );
    assert!(
        bounded * BOUNDED_RATIO_MULTIPLE < unbounded,
        "the listing call lasted {bounded:?} against the patient one's \
         {unbounded:?}: the kill missed the holder and nothing else bounded the \
         drain"
    );
}

/// The seams an escape has to clear, pinned as RELATIONS rather than as figures.
///
/// Neither escapee arm reads its own seam: they assert the escape happened, the
/// patient floor, the Unknown row and its cause, and a ratio against the
/// holder's life — all of which stay green with the seam back at
/// `HELD_PIPE_SEAM_MS`, on a box quiet enough for the escape to clear it. So the
/// gate could not tell the raised seam from the one it replaced, and this arm is
/// what makes the constants readable at all. It reads the constants, and the
/// arms spend them at their own `agent_timeout_ms` lines: an alias between the
/// two leaves this arm pinning a declaration nothing uses.
///
/// It pins three things and deliberately not a fourth. The FLOOR: an escapee
/// seam sits at least `ESCAPEE_SEAM_FLOOR_MULTIPLE` above the seam of the arm
/// whose descendant needs no escape — read against every seam an escape has to
/// clear, the parked-pair loop's included. The CEILING: the seam plus the drain
/// grace, `BOUNDED_RATIO_MULTIPLE` times over, still fits inside the holder's
/// life — read against the adapter's own `DRAIN_GRACE` and not a copy of it, and
/// against the same multiple the arms assert. It reaches the two seams whose
/// arms assert that ratio and no others. The EQUALITY: the two paths race the
/// same escape, so today they carry the same figure, and a slice that sizes them
/// apart edits this line and says why.
///
/// THE CEILING IS NECESSARY AND NOT SUFFICIENT. It models the bounded listing
/// call as the seam plus the grace, and the two arms assert their ratio over
/// the measured SPAN of that call — its start mark to the version call's —
/// which is that less the stub's own start before its mark, plus the step from
/// the collect to the version call and that call's spawn up to its mark. So a
/// seam passing here can still leave the ratio those arms assert false on a
/// loaded box: what the ceiling rules out is a seam that makes the ratio
/// unsatisfiable arithmetically, and nothing more.
///
/// WHAT IT DOES NOT PIN is that `ESCAPEE_EXIT_SEAM_MS` is the right figure
/// rather than merely a sufficient one. What keeps the escapee arms green is
/// the escape's own witness and not this figure: an attempt whose escape left
/// no witness is discarded and re-attempted, so a seam the race loses costs
/// patience rather than a red, and the figure is a margin and not a correctness
/// boundary. Pinning it here would encode a number the fleet is still measuring.
///
/// NOR ITS OWN PARAMETERS: the floor is `HELD_PIPE_SEAM_MS` times
/// `ESCAPEE_SEAM_FLOOR_MULTIPLE`, and an edit to either moves the floor with
/// this arm green. A guard over those two would rest on parameters of its own,
/// and the regress ends only at a figure something MEASURES — which is the
/// held-pipe arm's `unbounded`, a reading of the descendant's life and not of
/// the cheap seam. So this arm's parameters stay a reviewer's business.
#[test]
fn the_escapee_seams_sit_above_the_cheap_seam_and_under_the_ratios_ceiling() {
    let grace_ms = fleet_controller::platform::DRAIN_GRACE.as_millis() as u64;

    let ratio = u64::from(BOUNDED_RATIO_MULTIPLE);

    for (name, seam) in [
        ("the exit path's", ESCAPEE_EXIT_SEAM_MS),
        ("the deadline path's", ESCAPEE_DEADLINE_SEAM_MS),
        ("the parked-pair loop's", ESCAPEE_PARKED_PAIR_SEAM_MS),
    ] {
        assert!(
            seam >= HELD_PIPE_SEAM_MS * ESCAPEE_SEAM_FLOOR_MULTIPLE,
            "{name} seam is {seam} ms, under the floor of {} ms: an escape is an \
             sh, a python3 and a setsid before the holder exists, so the seam \
             that suffices where nothing has to escape ({HELD_PIPE_SEAM_MS} ms) \
             is not a seam for this arm",
            HELD_PIPE_SEAM_MS * ESCAPEE_SEAM_FLOOR_MULTIPLE
        );
    }

    // The ceiling reaches the two arms that assert the ratio, and the parked-pair
    // loop is not one of them: it reads a descriptor count against a holder that
    // outlives any seam this file sets.
    for (name, seam) in [
        ("the exit path's", ESCAPEE_EXIT_SEAM_MS),
        ("the deadline path's", ESCAPEE_DEADLINE_SEAM_MS),
    ] {
        assert!(
            (seam + grace_ms) * ratio < ESCAPEE_SECONDS * 1000,
            "{name} seam is {seam} ms, and with the {grace_ms} ms drain grace \
             {ratio} times over that is {} ms against a holder living \
             {ESCAPEE_SECONDS} s: the ratio those arms assert is no longer \
             satisfiable, so raising the seam means raising the holder's life",
            (seam + grace_ms) * ratio
        );
    }

    assert_eq!(
        ESCAPEE_EXIT_SEAM_MS, ESCAPEE_DEADLINE_SEAM_MS,
        "the two paths race the same escape and carry one figure between them"
    );

    // The fourth relation, and the only one about a PATIENT rig: the ceiling
    // those three run on sits far enough above every holder they have to outlast
    // that reaching it is the holder never dying, and never the holder living
    // its stated life. Three times, so a slice that raises a holder's life is
    // told here to raise the ceiling with it rather than finding out under load.
    for (name, life_ms) in [
        ("the escapees'", ESCAPEE_SECONDS * 1000),
        (
            "the held-pipe descendant's",
            HELD_PIPE_DESCENDANT_SECONDS * 1000,
        ),
    ] {
        assert!(
            PATIENT_DEADLINE_MS >= life_ms * 3,
            "the patient deadline is {PATIENT_DEADLINE_MS} ms against {name} life of \
             {life_ms} ms: a patient rig exists to sit through its holder, so its \
             ceiling has to stay a multiple of the life and not a figure the life \
             has grown into"
        );
    }
}

/// The accumulation clause of the drain, read where it is claimed. A parked
/// drain thread and the pipe end it holds cost one poll nothing; they are a
/// RUNNING controller's, one pair per outrun call, forever — so the reading is
/// the loop process's own open descriptors across polls it outran.
///
/// The descendant outlives the whole window BY CONSTRUCTION, and the arm
/// asserts that rather than assuming it. This is an absence assertion — no
/// growth — and what it can see is only the pipes still held at the second
/// reading: a descendant that dies of its own accord inside the window gives
/// its pipe back whether or not the kill reached it. At a descendant's life of
/// about one window the count is a FRACTION of the price, set by the poll
/// cadence and not by the claim; outliving the window it is the whole of it —
/// four outrun polls that never give a pair back read eight above the first.
#[test]
fn a_running_controller_accumulates_no_drain_pipes_across_polls() {
    let mut rig = Rig::new("drain-loop");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.agent_timeout_ms = Some(200);
    rig.hang_seconds = Some(DRAIN_LOOP_DESCENDANT_SECONDS);
    rig.descendant_seconds = Some(DRAIN_LOOP_DESCENDANT_SECONDS);

    let controller = rig.spawn_loop();
    rig.wait_until_within(
        |p| p["seats"][0]["roster_state"] == "unknown",
        POLL_BOUND,
        "the loop publishes a poll whose listing it outran",
    );
    let first =
        open_fds_of(controller.pid()).expect("lsof reads a running controller's open descriptors");
    let opened_at = Instant::now();

    // Four more polls, each of them outrun, read from the document's own stamp:
    // the claim is per poll, so the polls have to have happened.
    for _ in 0..4 {
        let seen = rig.projection()["generated_at"].clone();
        rig.wait_until_within(
            move |p| p["generated_at"] != seen,
            POLL_BOUND,
            "the loop polls again",
        );
    }

    let last = open_fds_of(controller.pid()).expect("the probe still reads the controller");
    let window = opened_at.elapsed();
    // The witness for the absence below: every descendant these five polls
    // forked is still inside its sleep at the second reading, so a pipe end the
    // group kill did not take back is one this count can see.
    assert!(
        window < Duration::from_secs(DRAIN_LOOP_DESCENDANT_SECONDS),
        "the five polls took {window:?}, which is the {DRAIN_LOOP_DESCENDANT_SECONDS} s \
         descendant's own life: a descendant that exits inside the window gives its \
         pipe back whether or not the kill reached it, and the count below reads \
         nothing"
    );
    // Three, and each unit is named: two for the one call that may be in flight
    // at either reading, which holds its own pipe pair legitimately, and one for
    // the temp file the controller has open when a reading lands mid-rewrite of
    // the projection. Four outrun polls that never gave a pair back read eight
    // above the first, so the slack does not reach the case.
    assert!(
        last <= first + 3,
        "the controller held {first} descriptors after its first outrun poll \
         and {last} after four more: the drains do not give their pipes back"
    );
}

/// The twin of the arm above, over the holder the kill CANNOT reach. An
/// in-group descendant closes its pipe end when the group kill lands, so the
/// drain returns and gives the pair back; an escapee left the group, keeps the
/// pipe for its whole life, and its drain is detached — which is the price
/// `run_bounded` names, charged once per outrun call and never given back.
///
/// So this arm reads the price rather than a bound: it asserts the accumulation
/// IS a pair per outrun poll, within the one call that may be in flight at
/// either reading.
///
/// The reading is PIPE rows, which is what the pair is made of. Counted off
/// every descriptor instead, both bounds are satisfied by a build that gives the
/// pair back and leaks two files or sockets in its place. A reading that stopped growing would mean the detach had
/// become a join or the kill had started reaching the escapee, and both are
/// changes the paragraph has to be re-read for.
#[test]
fn a_running_controller_parks_one_drain_pair_per_escaped_poll() {
    // The seam is an escapee seam and not the in-group twin's 200 ms: the escape
    // is an interpreter start plus `setsid`, and a seam it cannot clear kills the
    // holder inside the group, which is the other arm's case. Read from the const
    // at the site that spends it, so the floor guard below reaches this line and
    // not a declaration beside it.
    let mut rig = Rig::new("escaped-drain-loop");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.agent_timeout_ms = Some(ESCAPEE_PARKED_PAIR_SEAM_MS);
    rig.hang_seconds = Some(60);
    rig.escapee_seconds = Some(60);

    let controller = rig.spawn_loop();
    rig.wait_until_within(
        |p| p["seats"][0]["roster_state"] == "unknown",
        POLL_BOUND,
        "the loop publishes a poll whose listing it outran",
    );
    let escapes_before = rig.escapes_so_far();
    let (all_first, first) = open_descriptors_of(controller.pid())
        .expect("lsof reads a running controller's open descriptors");
    // The filter is a filter: the controller holds descriptors that are not
    // pipes — its binary, its cwd, the document it writes — so a reading where
    // the two counts are equal is a TYPE column this box spells differently and
    // a count that is silently the old type-blind one.
    assert!(
        all_first > first,
        "the controller held {all_first} descriptors and {first} of them read as \
         pipes: nothing here is distinguishing the type, so the pair below would \
         be counted off any descriptor at all"
    );

    // Polls until the EVENT this arm reads has happened twice, and not a fixed
    // three of them. The subject is escapes and not polls: an escape that loses
    // its race to the seam is an in-group holder whose pair comes back, so three
    // polls on a loaded box can carry one escape or none and the lower bound
    // below then reds on the box. Each poll is read from the document's own
    // stamp, as before, under the anti-hang ceiling. The cap is what keeps a
    // controller that escapes nothing from polling here forever, and its refusal
    // names both figures so the reading is the box's and not a bare absence.
    const POLL_CAP: usize = 8;
    let mut polls = 0_usize;
    while rig.escapes_so_far().saturating_sub(escapes_before) < 2 {
        assert!(
            polls < POLL_CAP,
            "{polls} poll(s) between the two readings produced only {} escape(s): \
             this controller is not escaping the group at all, which is the \
             in-group case and not the parked drain this arm reads",
            rig.escapes_so_far().saturating_sub(escapes_before)
        );
        let seen = rig.projection()["generated_at"].clone();
        rig.wait_until_within(
            move |p| p["generated_at"] != seen,
            POLL_BOUND,
            "the loop polls again",
        );
        polls += 1;
    }

    let (all_last, last) =
        open_descriptors_of(controller.pid()).expect("the probe still reads the controller");
    // Escapes are COUNTED and not assumed: an escape that loses its race to the
    // seam is an in-group holder the kill reaches, whose pair comes back, and a
    // poll count would charge this arm for it.
    let escaped = rig.escapes_so_far().saturating_sub(escapes_before);
    // TWO, not one: at one escape the lower bound below is `grew + 2 >= 2`,
    // which every reading satisfies, so a run that escaped once reads nothing
    // about the accumulation it is here for. THE PRECONDITION THE WAIT ABOVE
    // MEETS, kept as the statement of what the bounds need: the wait ends on
    // this count reaching two or refuses naming its own figures, so a red here
    // says the wait is not keyed on this event.
    assert!(
        escaped >= 2,
        "only {escaped} poll(s) between the two readings escaped the group: at \
         one escape the lower bound below degenerates to grew >= 0 and this arm \
         reads neither the in-group case nor the parked drain's"
    );
    let grew = last.saturating_sub(first);
    // Both bounds carry every figure: a growth BELOW the price is the detach
    // gone, and one above it is a second leak beside the parked pair. The slack
    // of two is the one call that may be in flight at either reading.
    assert!(
        grew + 2 >= 2 * escaped,
        "the controller held {first} pipes and then {last} across {escaped} \
         escaped poll(s) ({all_first} then {all_last} descriptors of every \
         type): the parked pairs are not accumulating, so the price run_bounded \
         states is not what this box charges"
    );
    assert!(
        grew <= 2 * escaped + 2,
        "the controller held {first} pipes and then {last} across {escaped} \
         escaped poll(s) ({all_first} then {all_last} descriptors of every \
         type): that is more than a pair per poll, so something beside the \
         parked drain is being kept"
    );
}
