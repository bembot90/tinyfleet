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

/// The accumulation clause of the reap, read where it is claimed. A zombie that
/// costs anything is one entry per outrun poll of a RUNNING controller, so the
/// reading is the loop process's own Z-state children after two polls whose
/// agent calls it outran: an adapter that answers `capabilities` at once — the
/// loop starts on it — and sleeps past the deadline on every other verb, so
/// each poll's calls are killed with their group and have to be reaped.
///
/// THE SURFACE IS THIS ARM'S DISTINCT REACH. Every agent call runs through
/// core's one bounded runner, whose own arms read one call's child; what only
/// this arm reads is a LONG-LIVED process's count ACROSS polls — the cost the
/// claim is about, which one call cannot show whatever the reap does. The two readings
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
    rig.name_the_agent(&rig.slow_adapter(&["version", "read", "context"], 5));

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
        "the loop publishes a poll whose agent calls it outran"
    );
    // The unknown has to be the DEADLINE's. An unknown from an adapter that
    // could not be run at all spawns nothing to reap, and the count below would
    // be a zero about that instead.
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
    // what `ps` can answer; nothing here reads which child. The outrun agent
    // call's is what the messages name because it is the only child the rig
    // gives the loop to hold, and a figure that grows with the polls is the
    // accumulation whatever the entries turn out to be.
    assert_eq!(
        at_a_poll, 0,
        "the running controller holds a Z-state child — an outrun poll's \
         agent call, on this rig (unreaped children: {at_a_poll} at the poll \
         this arm observed, {at_a_later_poll} at the next one it observed)"
    );
    assert_eq!(
        at_a_later_poll, 0,
        "the running controller accumulates Z-state children, one per outrun \
         poll — its agent calls, on this rig (unreaped children: {at_a_poll} \
         at the poll this arm observed, {at_a_later_poll} at the next one it \
         observed)"
    );
}
