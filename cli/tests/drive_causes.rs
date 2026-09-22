//! Why a listing is unreadable, at the adapter and end to end.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

/// `RosterRead::Unreadable` carries six distinct causes, at three construction
/// sites in `adapter::claude_code`: a binary that cannot be spawned, a wait on
/// the child that errors and a call that outruns its deadline (`run_bounded`);
/// a listing that exits non-zero, naming its status or "on a signal" when it has
/// none (`status`); and a listing that answers with zero bytes and success, or
/// with bytes that do not parse as JSON rows (`parse_roster`).
///
/// THREE of those six are driven here, in four readings between them: the spawn
/// failure, the deadline, and the exit-non-zero cause both ways — naming a
/// status, and naming none when the listing died on a signal. The list is
/// exhaustive, because a module that promises six and takes four is a coverage
/// claim a reader cannot check without counting the arms themselves.
///
/// The other three are elsewhere or nowhere, and the count is a claim about the
/// tree this file ships in — take it again after any arm moves, by putting a
/// `panic!` in the arm of `parse_roster` in question and reading who trips it.
/// The zero-bytes branch has THREE drivers, none in this module and none reading
/// which cause it is, only that one is there:
/// `an_unreadable_listing_is_unknown_for_every_seat_and_never_absent` in
/// `drive_seats.rs`, and
/// `lessons::the_roster_read_can_go_silently_dead` and
/// `an_unknown_seat_publishes_its_cause_and_no_reading` in `observe.rs`. The
/// JSON-parse cause has none: every listing every arm hands the reader is valid
/// JSON, an empty array, or zero bytes. And the wait error needs a `try_wait`
/// that fails, which no seam in this suite can produce.
///
/// Each arm below names the cause it expects, because "unreadable" with the
/// wrong reason is a reading nobody can act on — and each carries a control, so
/// a reading is the case's own and not this rig's.
mod unreadable_causes {
    use super::*;

    const OK_STUB: &str = "#!/bin/sh\necho '[]'\n";

    /// The anti-hang ceiling the kill arms wait for their stub's in-group fork
    /// under, and nothing else: no arm reads it as a figure, and a fork that
    /// lands one millisecond inside it is as good as one that lands at once.
    ///
    /// THE WAIT UNDER IT IS NOT WHERE THE TIME GOES. Measured over 18 runs of
    /// this binary at 1-minute load 7.2 to 53.2 on a 10-core box — 54 witness
    /// readings, one per kill arm per run — every reading was in MICROSECONDS
    /// and the slowest was 93.459 µs. The call ahead of this wait has already
    /// spent its whole seam plus the drain grace, so the fork it is looking for
    /// happened long before the first poll: this ceiling separates "the fork
    /// happened" from "this attempt never forked at all", and no value of it
    /// turns the second into the first.
    ///
    /// Retake it the way it was taken: run the selection under load with
    /// `--success-output immediate` and read the `witness:` lines.
    const WITNESS_BOUND: Duration = Duration::from_secs(5);

    /// The control an arm takes when what it has to rule out is this rig: the
    /// same rig and the same call, against a stub that answers. It shares the
    /// rig's directories and nothing
    /// else — its stub and its deadline are its own, deliberately, because a
    /// control taking the case's own deadline would assert that a healthy spawn
    /// finishes inside a figure the case chose to be short, which is a timing
    /// assertion on a shared box.
    fn readable(rig: &Rig) -> RosterRead {
        rig.read_with(&rig.stub_adapter("ok-stub", OK_STUB, Duration::from_secs(5)))
    }

    fn cause_of(read: RosterRead) -> String {
        match read {
            RosterRead::Unreadable { cause } => cause,
            RosterRead::Readable(rows) => {
                panic!(
                    "the listing was read as {} rows, not as unreadable",
                    rows.len()
                )
            }
        }
    }

    /// The scope of `Rig::witnessed`'s retry, which is the whole objection to it
    /// answered.
    ///
    /// A stub that RAN and then misbehaved wrote the marker on its first line,
    /// so the wrapper hands its reading back on attempt 1 and discards nothing.
    /// A retry gated on anything looser would re-run a stub whose bad answer is
    /// the subject, and the suite would be reading the last of three tries
    /// rather than the one it set up — the exact way a tolerance hides a defect.
    ///
    /// Two halves: the reading is the misbehaviour's, and the marker the wrapper
    /// decides on is really there — without the second, the first would also be
    /// what a wrapper that never reads the marker produces.
    ///
    /// The DISCARD COUNT IS NOT ASSERTED HERE, and that is deliberate: a
    /// well-behaved stub on a loaded box can be stalled before its first line
    /// like any other, so a zero here would be an assertion about the box.
    /// `a_call_whose_stub_marked_its_start_is_never_retried` pins the wrapper's
    /// decision instead, with no spawn in it at all.
    #[test]
    fn a_stub_that_ran_and_misbehaved_is_not_retried() {
        let rig = Rig::new("ran-and-misbehaved");
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "misbehaving-stub",
            "#!/bin/sh\nexit 3\n",
            Duration::from_secs(5),
        )));

        assert!(
            cause.contains('3'),
            "the reading is the stub's own misbehaviour, taken on the attempt \
             that ran it: {cause}"
        );
        assert!(
            rig.stub_started_path().exists(),
            "and the misbehaving stub marked its own start, so the reading above \
             came back from an attempt the wrapper KEPT: {}",
            rig.stub_started_path().display()
        );
    }

    /// The wrapper's decision, pinned without a spawn so no box can move it.
    ///
    /// The closure stands where a call would: it marks the start and returns a
    /// value. The wrapper must hand that value straight back and discard
    /// nothing — which is the whole of "a stub that ran is never retried",
    /// stated where a stalled box cannot reach it.
    #[test]
    fn a_call_whose_stub_marked_its_start_is_never_retried() {
        let rig = Rig::new("marked-its-start");
        let seen = rig.witnessed(|| {
            write(&rig.stub_started_path(), "");
            "the first attempt's own answer"
        });

        assert_eq!(seen, "the first attempt's own answer");
        assert_eq!(
            rig.discarded_attempts.get(),
            0,
            "the retry condition is the marker's absence and nothing else"
        );
    }

    /// The wrapper's whole purpose, INJECTED rather than waited for.
    ///
    /// A stub that misses its start exactly once and then behaves: the first
    /// invocation exits before the marker line, every later one marks and
    /// answers. That is the stall this exists for, made to happen on demand
    /// instead of once in ninety spawns on a busy box — so the rescue is pinned
    /// on any box, at any load, in milliseconds.
    ///
    /// The stub is written by hand because `stub_adapter` puts the marker
    /// immediately after the shebang, by design; an arm that has to balk BEFORE
    /// marking cannot say so through that writer.
    ///
    /// Two claims: the reading is the answering attempt's, and exactly one
    /// attempt was discarded. Without the second, an arm that never balked at
    /// all would pass this just as well.
    #[test]
    fn a_stub_that_misses_its_start_once_is_rescued_by_the_patience() {
        let rig = Rig::new("misses-once");
        let adapter = rig.stub_adapter(
            "balking-stub",
            "#!/bin/sh\nexit 0\n",
            Duration::from_secs(5),
        );
        let balked = rig.root.join("the-stub-has-balked");
        write(
            &rig.root.join("balking-stub"),
            &format!(
                "#!/bin/sh\n\
                 if [ ! -f '{balked}' ]; then : > '{balked}'; exit 0; fi\n\
                 : > '{marker}'\n\
                 echo '[]'\n",
                balked = balked.display(),
                marker = rig.stub_started_path().display()
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            rig.root.join("balking-stub"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        let read = rig.read_with(&adapter);

        assert!(
            matches!(read, RosterRead::Readable(_)),
            "the reading is the attempt that RAN, not the one that balked: {read:?}"
        );
        assert!(balked.exists(), "the stub really did balk once");
        assert_eq!(
            rig.discarded_attempts.get(),
            1,
            "exactly one attempt was discarded — the balk, and nothing else"
        );
    }

    /// The other side of that decision, and the end of the patience.
    ///
    /// A call that never marks a start is re-run and then REFUSED: spent
    /// patience is a red on the class and never a reading the suite quietly
    /// accepts. The panic message is the assertion — a wrapper that gave up
    /// silently, or that looped forever, fails here.
    ///
    /// WHICH OF THE THREE PATIENCES RUNS OUT HERE IS THE CALM BUDGET, set to
    /// 300 ms so the pin costs a fraction of a second. This closure's attempts
    /// are instant, so neither the floor in attempts nor the ceiling in attempt
    /// time is what ends it; the waits between them spend the calm budget and
    /// the refusal arrives carrying the wait's clause after this arm's own
    /// words. What is being pinned is that the refusal happens and opens with
    /// the condition that failed, not which budget paid for it.
    #[test]
    #[should_panic(expected = "the stub never started before the call returned")]
    fn a_call_whose_stub_never_starts_is_refused_when_the_patience_runs_out() {
        let rig = Rig::new("never-starts");
        rig.calm_budget.set(Duration::from_millis(300));
        rig.witnessed(|| ());
    }

    /// The third witness, pinned without a spawn for the reason the arm above
    /// gives: what is being stated is the wrapper's decision, and a box that
    /// stalls must not be able to move it.
    ///
    /// The closure marks its start on every attempt and reaches the owed witness
    /// only on the second, which is the stub killed mid-setup and then not
    /// killed. Two claims, and the second is what makes the first mean
    /// anything: the reading is the reaching attempt's, and exactly one attempt
    /// was discarded — without it, a wrapper that ignored the witness entirely
    /// would pass on the first attempt's answer just as well.
    #[test]
    fn a_call_that_misses_the_witness_it_owes_once_is_rescued_by_the_patience() {
        let rig = Rig::new("owes-once");
        let owed = rig.root.join("the-witness-this-arm-owes");
        rig.owes_witness(&owed);
        let reached = std::cell::Cell::new(false);

        let seen = rig.witnessed(|| {
            write(&rig.stub_started_path(), "");
            if reached.get() {
                write(&owed, "");
            }
            reached.set(true);
            "the reaching attempt's own answer"
        });

        assert_eq!(seen, "the reaching attempt's own answer");
        assert!(owed.exists(), "the witness really was reached in the end");
        assert_eq!(
            rig.discarded_attempts.get(),
            1,
            "exactly one attempt was discarded — the one that missed the \
             witness, and nothing else"
        );
    }

    /// THE FLOOR IN ATTEMPTS, pinned where the ceiling in time cannot reach it.
    ///
    /// The attempt budget is set to nothing, so the first attempt exhausts it
    /// and the retry this arm reads can only have come from the floor. That is
    /// the patient rigs' case made cheap: their one attempt costs more than the
    /// whole budget, and under a ceiling alone they were refused before they had
    /// retried once — "1 attempts over 39.442598125s of patience".
    ///
    /// The closure marks its start on the SECOND attempt only, so the answer
    /// returned is the retry's and the discard count says the first was thrown
    /// away. At `STUB_START_ATTEMPTS` of one this arm reds in the start
    /// refusal's words, which is the mutant that proves the floor is read.
    #[test]
    fn a_call_whose_attempt_outspends_the_budget_is_still_given_the_floors_retries() {
        let rig = Rig::new("attempt-floor");
        rig.attempt_budget.set(Duration::ZERO);
        let reached = std::cell::Cell::new(false);

        let seen = rig.witnessed(|| {
            if reached.get() {
                write(&rig.stub_started_path(), "");
            }
            reached.set(true);
            "the retry's own answer"
        });

        assert_eq!(seen, "the retry's own answer");
        assert_eq!(
            rig.discarded_attempts.get(),
            1,
            "exactly one attempt was discarded — the one that outspent the \
             budget, and nothing else"
        );
    }

    /// The other side of that decision. A witness nothing ever writes spends the
    /// patience and then REFUSES, naming the file it waited for — the calm
    /// budget here, for the reason the arm two above gives.
    ///
    /// This is the arm that keeps the third witness from becoming a mask: a
    /// production break that stops every attempt reaching the marker cannot pass
    /// as an endless quiet retry, it lands as a red that names the path and the
    /// class. The start marker is written on every attempt, so the refusal here
    /// is the third condition's and never the first's.
    #[test]
    #[should_panic(expected = "the stub never reached the witness this arm owes")]
    fn a_call_that_never_reaches_the_witness_it_owes_is_refused_when_the_patience_runs_out() {
        let rig = Rig::new("owes-forever");
        rig.calm_budget.set(Duration::from_millis(300));
        rig.owes_witness(&rig.root.join("a-witness-nothing-writes"));
        rig.witnessed(|| write(&rig.stub_started_path(), ""));
    }

    /// The wait inside that patience, pinned on its own bound.
    ///
    /// A probe that cannot be run never reads as clear, so this wrapper spends
    /// the CALM budget waiting and then refuses, naming the class. This is the
    /// half the attempt count does not end: the calm budget runs out inside the
    /// first wait, before the second attempt is ever fired. The refusal is the
    /// wrapper's — the wait reports a spent patience rather than raising it —
    /// and the words pinned here are the wait's clause, which that refusal
    /// carries. Without the pin, a wait that counted a failed probe as ready
    /// would be a no-op that nothing notices — and a no-op here would put every
    /// re-attempt straight back into the storm the wait exists to sit out.
    ///
    /// The probe is pointed at a path that is not there, which is the same
    /// injection the binary-seam arms use, and the calm budget is short for the
    /// reason the arm above gives.
    #[test]
    #[should_panic(expected = "no spawn cleared inside")]
    fn a_spawn_probe_that_cannot_run_is_never_clear_and_the_wait_refuses() {
        let rig = Rig::new("no-probe");
        rig.calm_budget.set(Duration::from_millis(300));
        *rig.spawn_probe.borrow_mut() = rig.root.join("no-such-shell");
        rig.witnessed(|| ());
    }

    /// A probe that alternates prompt and slow: the box that hands out one good
    /// spawn between bad ones, which is the shape a single sample cannot tell
    /// from a calm box.
    ///
    /// ITS STATE IS A LINE COUNT AND ITS WORK IS BUILTINS ONLY — `read` and an
    /// appending `echo`, no subshell and no `cat` — because the prompt half has
    /// to land under `SPAWN_IS_CLEAR_WITHIN` or this probe is an always-slow one
    /// wearing an alternating name. Measured on this box: 25 to 28 ms on the
    /// prompt half against the 50 ms bound.
    fn an_alternating_probe(root: &Path) -> PathBuf {
        let path = root.join("a-shell-that-alternates");
        let counter = root.join("a-shell-that-alternates.count");
        write(&counter, "");
        write(
            &path,
            &format!(
                "#!/bin/sh\n\
                 n=0\n\
                 while read -r _; do n=$((n+1)); done < '{counter}'\n\
                 echo x >> '{counter}'\n\
                 [ $((n % 2)) -eq 1 ] && sleep 0.1\n\
                 exit 0\n",
                counter = counter.display()
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// CALM IS CONSECUTIVE, and one good sample is not a reading of it.
    ///
    /// Why the count is three and not one: the fail logs of the suite this came
    /// from show four attempts fired inside 12.3 to 12.5 s, each released by a
    /// single prompt probe, into a box that discarded every one of them. A wait
    /// that clears on one sample is a wait the storm walks straight through.
    ///
    /// THE CLOSURE REACHES ITS MARKER ON THE SECOND ATTEMPT, and that is what
    /// makes this a reading rather than a restatement of the arm two above. A
    /// wait that CLEARS here releases that second attempt and the call returns;
    /// a wait that refuses never lets it run. So at `CALM_PROBES` of one — where
    /// the alternating probe's prompt half clears every wait — this arm returns
    /// normally and the `should_panic` reds, while at three the wait spends the
    /// calm budget and refuses in its own voice. Pinned on the wait's words
    /// because the condition that failed is the box's, not the stub's.
    ///
    /// ITS CALM BUDGET IS SECONDS AND NOT THE 300 ms ITS NEIGHBOURS USE, which
    /// is measured and not taste: the probe's first exec is cold at about
    /// 176 ms and its slow half costs 110, so inside 300 ms exactly two probes
    /// fire and neither is the prompt one. The budget has to hold several full
    /// alternations or the arm refuses for want of a turn, which is the
    /// always-slow arm's reading and not this one's.
    ///
    /// It needs a box whose prompt half is genuinely prompt: on one loaded
    /// enough that every probe is slow, the mutant above refuses too and this
    /// arm stops separating the two.
    #[test]
    #[should_panic(expected = "no spawn cleared inside")]
    fn a_probe_that_is_never_prompt_twice_running_never_reads_calm() {
        let rig = Rig::new("alternating-probe");
        rig.calm_budget.set(Duration::from_secs(2));
        *rig.spawn_probe.borrow_mut() = an_alternating_probe(&rig.root);
        let reached = std::cell::Cell::new(false);
        rig.witnessed(|| {
            if reached.get() {
                write(&rig.stub_started_path(), "");
            }
            reached.set(true);
        });
    }

    /// A probe the box can always run and never runs promptly — the slow spawn
    /// a contended box hands out, so an arm can reach the exhausted wait
    /// without putting load on a shared machine. Everything it needs is its
    /// own: nothing an arm sets by hand is read back out of it.
    fn a_probe_that_never_clears(root: &Path) -> PathBuf {
        let path = root.join("a-shell-that-dawdles");
        write(&path, "#!/bin/sh\nsleep 0.1\nexit 0\n");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The refusal has ONE voice whichever budget ran out.
    ///
    /// The two pins above spend their patience in discarded attempts, on a box
    /// where the probe between them clears at once. This one spends the CALM
    /// budget instead — the probe runs and is never prompt — and the refusal it
    /// owes the reader is still the witness's, because what failed is the same
    /// class: the stub never reached the marker this arm owes. A wrapper whose
    /// refusal changed its words with the way the patience happened to go
    /// leaves this pin reading a panic about spawns.
    #[test]
    #[should_panic(expected = "the stub never reached the witness this arm owes")]
    fn a_witness_refusal_keeps_its_words_when_the_patience_goes_into_the_wait() {
        let rig = Rig::new("owes-forever-slow-probe");
        rig.calm_budget.set(Duration::from_millis(300));
        rig.owes_witness(&rig.root.join("a-witness-nothing-writes"));
        *rig.spawn_probe.borrow_mut() = a_probe_that_never_clears(&rig.root);
        rig.witnessed(|| write(&rig.stub_started_path(), ""));
    }

    /// The sibling of the arm above, on the first of the three conditions.
    ///
    /// A call that never marks a start, on a box whose probe is never prompt,
    /// is refused in the start's words and not the wait's. Both arms are
    /// needed: the wrapper composes the refusal from whichever condition
    /// discarded the attempt, so a fix that pinned only one of them would leave
    /// the other free to speak in the wait's voice.
    #[test]
    #[should_panic(expected = "the stub never started before the call returned")]
    fn a_start_refusal_keeps_its_words_when_the_patience_goes_into_the_wait() {
        let rig = Rig::new("never-starts-slow-probe");
        rig.calm_budget.set(Duration::from_millis(300));
        *rig.spawn_probe.borrow_mut() = a_probe_that_never_clears(&rig.root);
        rig.witnessed(|| ());
    }

    /// What a spawn cause carries after the adapter's own `could not start
    /// <bin>: ` — the `io::Error`'s rendering, whose words are the platform's.
    ///
    /// Stripping the prefix is the assertion: a cause that does not open with
    /// it fails here, naming what it opened with instead, so the prefix and the
    /// binary are pinned by the split itself and an arm can then speak about
    /// the OS's reason without quoting it.
    fn os_reason(cause: &str, bin: &str) -> String {
        let prefix = format!("could not start {bin}: ");
        cause
            .strip_prefix(&prefix)
            .unwrap_or_else(|| {
                panic!("a spawn cause opens with {prefix:?}, and this one reads: {cause:?}")
            })
            .to_string()
    }

    #[test]
    fn a_listing_that_exits_non_zero_is_unreadable_and_names_the_status() {
        let rig = Rig::new("exit-non-zero");
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "refusing-stub",
            "#!/bin/sh\necho 'the account is not signed in' >&2\nexit 3\n",
            Duration::from_secs(5),
        )));
        assert!(
            cause.contains("exited 3"),
            "the cause names the status: {cause}"
        );
        assert!(
            cause.contains("the account is not signed in"),
            "and carries what the binary said about it: {cause}"
        );

        // The control: the same rig, the same call, a stub that answers.
        assert!(
            matches!(readable(&rig), RosterRead::Readable(_)),
            "the rig can read a listing, so the Unreadable above is the stub's"
        );
    }

    /// A spawn fails for more than one reason and the cause is what an operator
    /// acts on, so it names the binary AND what the OS said about it.
    #[test]
    fn a_binary_that_cannot_be_spawned_is_unreadable_and_names_it_and_why() {
        let rig = Rig::new("no-binary");
        let absent = rig.root.join("no-such-agent").display().to_string();
        let missing = rig.adapter_at(absent.clone(), Duration::from_secs(5));
        let cause = cause_of(missing.status(None));
        // Split on the adapter's own contract — `could not start <bin>: ` and
        // then whatever the io::Error rendered — so what is pinned is the
        // adapter's prefix and the binary, never the words libc's strerror
        // chose for the errno. This file's header claims its shapes hold on
        // either platform, and the OS text is the one part of this cause that
        // does not.
        let reason_for_the_missing_binary = os_reason(&cause, &absent);
        assert!(
            !reason_for_the_missing_binary.is_empty(),
            "the cause carries the OS's own reason after the prefix: {cause}"
        );

        // The control on the reason: a binary that IS there and cannot be
        // executed fails differently, so the reason above is this spawn's own
        // failure and not one wording for every spawn. Asserted as a DIFFERENCE
        // between the two renderings rather than as either one's text, which is
        // the same claim without the platform in it.
        let unrunnable = rig.root.join("not-executable");
        write(&unrunnable, "#!/bin/sh\necho '[]'\n");
        let refused = rig.adapter_at(unrunnable.display().to_string(), Duration::from_secs(5));
        let cause = cause_of(refused.status(None));
        let reason_for_the_unrunnable_file = os_reason(&cause, &unrunnable.display().to_string());
        assert!(
            !reason_for_the_unrunnable_file.is_empty(),
            "the second cause carries a reason too: {cause}"
        );
        assert_ne!(
            reason_for_the_missing_binary, reason_for_the_unrunnable_file,
            "a file that is not executable is a DIFFERENT spawn failure from one \
             that is not there, and the cause says which"
        );

        assert!(
            matches!(readable(&rig), RosterRead::Readable(_)),
            "the control reads, so the causes above are the two bad binaries'"
        );
    }

    /// The deadline, measured at the adapter with a short one rather than at the
    /// 20 s `ClaudeCode::new` sets: the field is the seam, and a 20 s arm would
    /// be 20 s of every run of this suite.
    #[test]
    fn a_listing_that_outruns_the_deadline_is_killed_and_read_as_unreadable() {
        let rig = Rig::new("deadline");
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "hanging-stub",
            "#!/bin/sh\nsleep 5\necho '[]'\n",
            Duration::from_millis(200),
        )));
        assert!(
            cause.contains("did not answer within 200ms"),
            "the cause names the deadline it actually ran on, and never rounds a \
             sub-second one down to a deadline nobody set: {cause}"
        );
        // The module docstring's absolute form: the margin is the stub's own 5 s
        // sleep against a 200 ms seam, so the two outcomes are seconds apart.
        assert!(
            rig.last_call() < Duration::from_secs(5),
            "the call returned before the stub would have: {:?}",
            rig.last_call()
        );

        assert!(
            matches!(readable(&rig), RosterRead::Readable(_)),
            "the control reads through the same rig and the same call, at a \
             deadline of its own that it fits"
        );
    }

    /// The kill the arm above is named for. A fast return says the CALLER
    /// stopped waiting; it says nothing about the child, and a listing left
    /// running past its deadline is a process per poll on the operator's box.
    ///
    /// The marker is the measurement: the stub writes it after its sleep, so a
    /// child that was killed never writes one.
    ///
    /// What is measured is the DIRECT child, through a marker only the child
    /// writes. The kill reaches the GROUP the child leads, so a descendant left
    /// in that group dies with it and a descendant that called `setsid` does
    /// not — the blast radius `run_bounded` states, and the pair of arms below
    /// (`an_outrun_listings_in_group_descendant_is_killed_with_the_group`,
    /// `an_escaped_descendant_survives_the_kill_the_in_group_one_dies_on`) are
    /// what read it. The held-pipe arm below reads neither: it measures that
    /// this call RETURNS while a descendant still holds the inherited pipe, and
    /// observes no descendant at all.
    #[test]
    fn an_outrun_listing_is_killed_and_not_left_running_to_finish() {
        const SEAM_MS: u64 = 200;
        /// How late the kill may land and still find the stub asleep. The
        /// stub's sleep is the seam plus this, and below it the stub could
        /// finish HONESTLY before the kill and write the marker — which reads
        /// as a kill that missed. It is the headroom the bare `sleep 1` already
        /// carried against this arm's 200 ms seam, NAMED and not changed: what
        /// rate a smaller one flakes at needs a sample the work item that named
        /// it could not afford, and is on the follow-up filed off it.
        const KILL_MARGIN_MS: u64 = 800;
        const STUB_SLEEP_MS: u64 = SEAM_MS + KILL_MARGIN_MS;
        /// How long the arm waits before reading the marker ABSENT, which is
        /// the one figure here that buys correctness rather than calm: a wait
        /// shorter than the stub's whole life passes on a kill that missed.
        /// Derived, so it cannot fall behind the sleep above it — the stub's
        /// sleep, the grace the adapter gives a killed child to drain, and the
        /// start the stub paid before that sleep began.
        const WAIT_MS: u64 = STUB_SLEEP_MS
            + fleet_controller::platform::DRAIN_GRACE.as_millis() as u64
            + START_COST_MARGIN_MS;

        let rig = Rig::new("kill");
        let marker = rig.root.join("the-stub-finished");
        // Single-quoted into the shell text: a TMPDIR carrying a space otherwise
        // splits the redirection and writes outside the rig.
        let body = format!(
            "#!/bin/sh\nsleep {}\n: > '{}'\necho '[]'\n",
            STUB_SLEEP_MS as f64 / 1000.0,
            marker.display()
        );

        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "slow-stub",
            &body,
            Duration::from_millis(SEAM_MS),
        )));
        assert!(cause.contains("did not answer within"), "{cause}");
        std::thread::sleep(Duration::from_millis(WAIT_MS));
        assert!(
            !marker.exists(),
            "the outrun listing ran on past the deadline and finished its work: {}",
            marker.display()
        );

        // The control: the same stub inside a deadline it fits DOES write the
        // marker, so the absence above is the kill's and not a stub that never
        // writes one.
        assert!(matches!(
            rig.read_with(&rig.stub_adapter("patient-stub", &body, Duration::from_secs(5))),
            RosterRead::Readable(_)
        ));
        assert!(
            marker.exists(),
            "the same stub writes the marker when it is left to finish: {}",
            marker.display()
        );
    }

    /// The kill's BLAST RADIUS, from inside it: a descendant the listing forked
    /// and left in the group dies with the group, deliberately, however hard it
    /// tries to stay.
    ///
    /// `trap '' TERM` is what makes this arm read the signal and not merely the
    /// kill: a descendant that ignores TERM survives a group SIGTERM, finishes
    /// its sleep and writes the marker, so the absence below is SIGKILL's and
    /// nothing weaker's.
    ///
    /// THE ABSENCE NEEDS A WITNESS THAT THERE WAS SOMETHING TO KILL. A descendant
    /// that never started writes no marker either, and under the full suite's
    /// parallel load that is what a short deadline buys: the stub's `/bin/sh` is
    /// killed before it reaches the fork, and the arm reads "nothing existed" as
    /// "the kill reached it" and passes with the group kill deleted. So the
    /// subshell's FIRST act is a start marker the arm polls for and asserts, and
    /// SEAM_MS is far enough above `/bin/sh`'s start under that load for the fork
    /// to clear it — the descendant outlives the seam and the child outlives the
    /// descendant, so the kill lands with the descendant alive and the control
    /// still has something to finish.
    #[test]
    fn an_outrun_listings_in_group_descendant_is_killed_with_the_group() {
        const SEAM_MS: u64 = 3000;
        const DESCENDANT_SECONDS: u64 = 5;
        const CHILD_SECONDS: u64 = 6;

        let rig = Rig::new("group-kill");
        let started = rig.root.join("the-descendant-started");
        let marker = rig.root.join("the-descendant-finished");
        // Both paths are single-quoted into the shell text: a TMPDIR carrying a
        // space otherwise splits the redirection and writes outside the rig.
        let body = format!(
            "#!/bin/sh\n\
             ( : > '{}'; trap '' TERM; sleep {DESCENDANT_SECONDS}; : > '{}' ) &\n\
             sleep {CHILD_SECONDS}\n\
             echo '[]'\n",
            started.display(),
            marker.display()
        );

        // A PRECONDITION BEFORE IT IS AN ASSERTION, the pairing the version arm
        // below already has: the fork is a spawn chain inside a fixed seam, so a
        // box that stretches it past the deadline kills the stub mid-setup and
        // the attempt never reached the case this arm is about. Named here, that
        // attempt is discarded and retried on the stub-start budget instead of
        // asserted on.
        rig.owes_witness(&started);
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "keeper-stub",
            &body,
            Duration::from_millis(SEAM_MS),
        )));
        assert!(cause.contains("did not answer within"), "{cause}");
        // The witness, before the absence and not after it: the fork happened, so
        // the kill had a descendant to reach. Polled rather than read once,
        // because the start is a race the arm is allowed to win late.
        rig.witness_within(
            &started,
            WITNESS_BOUND,
            "the stub never forked its descendant, so the absence below would be \
             a kill that had nothing to reach",
        );
        // Past the descendant's own sleep, so a marker it was going to write has
        // had its whole life to appear.
        std::thread::sleep(Duration::from_millis(
            DESCENDANT_SECONDS * 1000 + START_COST_MARGIN_MS,
        ));
        assert!(
            !marker.exists(),
            "a descendant of the outrun listing outlived the group kill and \
             finished its work: {}",
            marker.display()
        );

        // The control: the same stub inside a deadline it fits, where no kill
        // happens at all. The descendant writes the marker, so the absence above
        // is the kill's and not a stub whose descendant never writes one.
        assert!(matches!(
            rig.read_with(&rig.stub_adapter("keeper-patient-stub", &body, Duration::from_secs(30))),
            RosterRead::Readable(_)
        ));
        assert!(
            marker.exists(),
            "the same descendant writes the marker when the group is left alone: {}",
            marker.display()
        );
    }

    /// The same kill at its OTHER site. `run_bounded` kills the group twice —
    /// once when the wait outran the deadline, and once when the child answered
    /// and the collect after it did not — and only the first has a reader. Here
    /// the child exits 0 at once while a descendant it forked and left in the
    /// group keeps the inherited pipe: the drains cannot finish, the collect
    /// gives up at the deadline, and the kill is what stops the descendant from
    /// holding that pair for its whole life.
    ///
    /// The shape is the arm above's because the claim is the same one on the
    /// other branch: `trap '' TERM`, so the absence reads SIGKILL and nothing
    /// weaker; a start marker asserted BEFORE the absence, so a fork that never
    /// happened cannot read as a kill that landed; and a control at a deadline
    /// the descendant fits.
    ///
    /// THE CHILD ANSWERS AT ONCE, which is what puts the call on this branch. A
    /// child that outran the seam would take the deadline branch and this arm
    /// would pass while reading the site above instead of its own.
    #[test]
    fn an_answered_listings_in_group_descendant_is_killed_when_the_collect_gives_up() {
        const SEAM_MS: u64 = 3000;
        const DESCENDANT_SECONDS: u64 = 5;

        let rig = Rig::new("exit-path-group-kill");
        let started = rig.root.join("the-descendant-started");
        let marker = rig.root.join("the-descendant-finished");
        // Both paths are single-quoted into the shell text: a TMPDIR carrying a
        // space otherwise splits the redirection and writes outside the rig.
        let body = format!(
            "#!/bin/sh\n\
             ( : > '{}'; trap '' TERM; sleep {DESCENDANT_SECONDS}; : > '{}' ) &\n\
             echo '[]'\n",
            started.display(),
            marker.display()
        );

        // The call ends on the collect's own deadline, not on the wait's: the
        // child was reaped long before, and this cause is the collect giving up.
        //
        // The fork is this arm's precondition and not only its assertion: an
        // attempt whose descendant never started sampled no group kill, so it is
        // discarded rather than read.
        rig.owes_witness(&started);
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "answering-keeper-stub",
            &body,
            Duration::from_millis(SEAM_MS),
        )));
        assert!(cause.contains("did not answer within"), "{cause}");
        // The witness, before the absence and not after it: the fork happened,
        // so the kill had a descendant to reach. Polled rather than read once,
        // because the start is a race the arm is allowed to win late.
        rig.witness_within(
            &started,
            WITNESS_BOUND,
            "the stub never forked its descendant, so the absence below would be \
             a kill that had nothing to reach",
        );
        // Past the descendant's own sleep, so a marker it was going to write has
        // had its whole life to appear.
        std::thread::sleep(Duration::from_millis(
            DESCENDANT_SECONDS * 1000 + START_COST_MARGIN_MS,
        ));
        assert!(
            !marker.exists(),
            "a descendant holding the pipe of a listing that ANSWERED outlived \
             the collect's kill and finished its work: {}",
            marker.display()
        );

        // The control: the same stub at a deadline the descendant fits, where
        // the collect returns and no kill happens at all. The listing reads and
        // the descendant writes the marker, so the absence above is the kill's.
        assert!(matches!(
            rig.read_with(&rig.stub_adapter(
                "answering-keeper-patient-stub",
                &body,
                Duration::from_secs(30)
            )),
            RosterRead::Readable(_)
        ));
        assert!(
            marker.exists(),
            "the same descendant writes the marker when the collect is given the \
             time to return: {}",
            marker.display()
        );
    }

    /// The kill on the call the arms above never make. A poll asks the binary
    /// twice and `--version` runs through the same `run_bounded`, so the blast
    /// radius and the deadline are claims about that call too — and every arm
    /// that reads them drives the listing.
    ///
    /// The stub answers only `--version` here: a body that also served the
    /// listing would let a rig reach this arm's descendant down the call it is
    /// not about.
    ///
    /// The fork is this arm's setup, so `started` is owed to `witnessed` rather
    /// than waited for: a kill that lands before it is discarded, not asserted over.
    #[test]
    fn an_outrun_version_calls_in_group_descendant_is_killed_with_the_group() {
        let rig = Rig::new("group-kill-version");
        let started = rig.root.join("the-descendant-started");
        let marker = rig.root.join("the-descendant-finished");
        let body = version_keeper_body(&started, &marker, None);

        let agent = rig.stub_adapter(
            "version-keeper-stub",
            &body,
            Duration::from_millis(VERSION_SEAM_MS),
        );
        rig.owes_witness(&started);
        let version = rig.witnessed(|| agent.version());
        assert!(
            version.is_none(),
            "a version call the deadline cut short reports nothing: {version:?}"
        );
        assert!(
            started.exists(),
            "the stub never forked its descendant, so the absence below would be \
             a kill that had nothing to reach: {}",
            started.display()
        );
        std::thread::sleep(Duration::from_millis(
            VERSION_DESCENDANT_SECONDS * 1000 + START_COST_MARGIN_MS,
        ));
        assert!(
            !marker.exists(),
            "a descendant of the outrun version call outlived the group kill and \
             finished its work: {}",
            marker.display()
        );

        // The control, on the same two readings: the same stub inside a deadline
        // it fits reports its version AND its descendant writes the marker, so
        // the null and the absence above are the deadline's and not a stub that
        // answers nothing.
        assert_eq!(
            rig.stub_adapter(
                "version-keeper-patient-stub",
                &body,
                Duration::from_secs(30)
            )
            .version()
            .as_deref(),
            Some("9.9.9")
        );
        assert!(
            marker.exists(),
            "the same descendant writes the marker when the group is left alone: {}",
            marker.display()
        );
    }

    /// The outrun version call's seam, and the two sleeps past it the stub
    /// below runs: the descendant's, and the child's own.
    const VERSION_SEAM_MS: u64 = 3000;
    const VERSION_DESCENDANT_SECONDS: u64 = 5;
    const VERSION_CHILD_SECONDS: u64 = 6;

    /// The `--version`-only stub both version-kill arms run: a descendant that
    /// writes `started`, ignores TERM, and writes `marker` only if it outlives
    /// the call's group kill.
    ///
    /// With `balked`, the first invocation creates that file and sleeps past the
    /// seam before the fork, so its kill lands mid-setup; every later invocation
    /// finds the file and runs the body unchanged.
    fn version_keeper_body(started: &Path, marker: &Path, balked: Option<&Path>) -> String {
        let balk = balked
            .map(|path| {
                format!(
                    "\x20   if [ ! -f '{p}' ]; then : > '{p}'; sleep {}; fi\n",
                    VERSION_SEAM_MS / 1000 + 1,
                    p = path.display()
                )
            })
            .unwrap_or_default();
        format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 --version)\n\
             {balk}\
             \x20   ( : > '{}'; trap '' TERM; sleep {VERSION_DESCENDANT_SECONDS}; : > '{}' ) &\n\
             \x20   sleep {VERSION_CHILD_SECONDS}\n\
             \x20   echo '9.9.9 (a stub)'\n\
             \x20   ;;\n\
             \x20 *) exit 64;;\n\
             esac\n",
            started.display(),
            marker.display()
        )
    }

    /// The rescue the arm above rests on, INJECTED rather than waited for: its
    /// own stub, balking past the seam before the fork on the first invocation
    /// only — the kill landing mid-setup that a loaded box produces now and then.
    ///
    /// The count is what makes the other claims mean anything: a call that never
    /// enters `witnessed` reads the balked attempt, discards nothing, and has no
    /// `started` to show.
    ///
    /// IT IS A FLOOR AND NOT AN EQUALITY. The balk's own attempt is always
    /// thrown away, so one discard is owed and its absence is the failure this
    /// arm exists to catch; but an attempt beside it can be stalled before its
    /// own first line, or past its fork, by a box running a second copy of these
    /// arms, so an exact count is an assertion about the box and not about the
    /// rescue. `a_stub_that_ran_and_misbehaved_is_not_retried` declines the same
    /// equality one number lower, for the same reason.
    #[test]
    fn a_version_call_killed_before_its_fork_once_is_rescued_by_the_patience() {
        let rig = Rig::new("version-balks-once");
        let started = rig.root.join("the-descendant-started");
        let balked = rig.root.join("the-version-stub-balked");
        let body = version_keeper_body(
            &started,
            &rig.root.join("the-descendant-finished"),
            Some(&balked),
        );

        let agent = rig.stub_adapter(
            "version-balking-stub",
            &body,
            Duration::from_millis(VERSION_SEAM_MS),
        );
        rig.owes_witness(&started);
        let version = rig.witnessed(|| agent.version());

        assert!(
            version.is_none(),
            "the kept attempt was outrun by the seam too, so it reports nothing: {version:?}"
        );
        assert!(
            started.exists(),
            "the attempt the wrapper kept forked its descendant: {}",
            started.display()
        );
        assert!(balked.exists(), "the stub really did balk once");
        assert!(
            rig.discarded_attempts.get() >= 1,
            "the balk was rescued without being discarded, so no retry happened: \
             {} attempts thrown away",
            rig.discarded_attempts.get()
        );
    }

    /// The blast radius from OUTSIDE it. `run_bounded` states that the kill
    /// stops at the group, so a descendant that called `setsid` before the
    /// deadline keeps running when everything still in the group dies — and the
    /// price the same paragraph names, a parked thread and its pipe end, is paid
    /// only because that holder is alive to hold them.
    ///
    /// BOTH HALVES ARE READ IN ONE CALL, because either alone is satisfied by a
    /// kill that never happened: the in-group descendant's missing marker says
    /// the kill landed, and the escapee alive after it says it stopped at the
    /// group boundary. The escapee is named by the pid its own witness carries,
    /// and its liveness is a `ps` state read — it is a child of nothing this
    /// suite waits on, so there is no reap of ours between the kill and the read.
    ///
    /// NOT THROUGH `observe_out_of_process`, which the escapee-forking POLL arms
    /// take. This arm drives the ADAPTER and never a poll, and the streams its
    /// escapee inherits are `run_bounded`'s own: `spawn_in_group` nulls the
    /// child's stdin and pipes both of its output streams, so the escapee holds
    /// the controller's pipe ends and none of this test process's.
    #[test]
    fn an_escaped_descendant_survives_the_kill_the_in_group_one_dies_on() {
        const SEAM_MS: u64 = 3000;
        const DESCENDANT_SECONDS: u64 = 5;
        const CHILD_SECONDS: u64 = 6;
        // Long enough that the escapee is still alive at the read below on any
        // load the seam itself survives: the read comes one grace after the
        // deadline, and the margin is this minus that.
        const ESCAPEE_LIFE_SECONDS: u64 = 30;

        let rig = Rig::new("blast-radius");

        // The reader's control, taken before the subject: a pid this arm reaps
        // itself reads NOT alive, so the aliveness below is a state this reader
        // can tell from the other one and not the only answer it has.
        let mut reaped = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("a shell runs");
        let reaped_pid = reaped.id();
        reaped.wait().expect("the control child is reaped");
        assert!(
            !process_is_alive(reaped_pid),
            "the reader answers alive for a pid this arm waited on: {reaped_pid}"
        );

        let started = rig.root.join("the-descendant-started");
        let marker = rig.root.join("the-descendant-finished");
        // Every interpolated path is single-quoted into the shell text: a TMPDIR
        // carrying a space otherwise splits the redirection and writes outside
        // the rig.
        let body = format!(
            "#!/bin/sh\n\
             '{escape}' {ESCAPEE_LIFE_SECONDS} '{escaped}'\n\
             ( : > '{started}'; trap '' TERM; sleep {DESCENDANT_SECONDS}; : > '{marker}' ) &\n\
             sleep {CHILD_SECONDS}\n\
             echo '[]'\n",
            escape = rig.escape_path().display(),
            escaped = rig.escaped_path().display(),
            started = started.display(),
            marker = marker.display()
        );

        // THE THIRD WITNESS BESIDE THE ESCAPE'S OWN. This arm already owes the
        // escapee's file; the in-group fork is the other half of the same
        // comparison, and an attempt that produced one and not the other cannot
        // tell the two descendants apart. Both are preconditions here.
        rig.owes_witness(&started);
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "blast-radius-stub",
            &body,
            Duration::from_millis(SEAM_MS),
        )));
        assert!(cause.contains("did not answer within"), "{cause}");
        // Read as close to the kill as the arm can: the call returns one drain
        // grace after it, and every line below adds to that distance.
        let escapee = rig.escapee_pid();
        assert!(
            process_is_alive(escapee),
            "the escapee left the group before the deadline and the kill reached \
             it anyway: {escapee}"
        );
        // The witness the absence needs, before the absence: the fork happened,
        // so the kill had a descendant inside the group to reach.
        rig.witness_within(
            &started,
            WITNESS_BOUND,
            "the stub never forked its in-group descendant, so the absence below \
             would be a kill that had nothing to reach",
        );
        std::thread::sleep(Duration::from_millis(
            DESCENDANT_SECONDS * 1000 + START_COST_MARGIN_MS,
        ));
        assert!(
            !marker.exists(),
            "the in-group descendant outlived the kill, so the escapee's life \
             above is not the group boundary: {}",
            marker.display()
        );
    }

    /// `DRAIN_GRACE` itself, read on the exit path, where it is the only thing
    /// that ends the call.
    ///
    /// The child answers at once and an escapee holds both pipe ends, so the
    /// collect runs out the call's own deadline, the kill misses the holder, and
    /// the grace is what the last wait is bounded by. The elapsed is therefore
    /// the seam plus the grace, read against the adapter's own constant: BELOW
    /// that the grace collect is not happening at all, and above it by more than
    /// the spawn the grace has become a wait for an answer — the join the detach
    /// exists to refuse.
    ///
    /// THE ANSWERED MARKER IS THE BRANCH WITNESS. The two branches spend the
    /// same seam and the same grace, so the elapsed cannot tell them apart; the
    /// child writes this before its echo and a child the deadline killed never
    /// reaches the line, so its presence is what says this reading is the exit
    /// path's.
    ///
    /// AND IT IS A PRECONDITION BEFORE IT IS AN ASSERTION, which is why it goes
    /// to `owes_witness` as well as to the assert below — the same pairing the
    /// escape witness beside it has. Everything the stub does before that line
    /// is a spawn chain — a shell, the escape helper, a `python3` that calls
    /// `setsid`, and a poll that notices its file — and the seam is a fixed
    /// wall-clock figure, so a box that stretches the chain past it kills the
    /// child mid-setup and the call lands on the deadline branch. That attempt
    /// sampled no exit path, so the wrapper discards it on the same budget as a
    /// stub that never started, and the bounds below are read on an attempt
    /// that reached the case they are about. What the chain costs against this
    /// 3000 ms seam, measured at the suite's own width over ten rounds: 103 ms
    /// to 150 ms on the nine rounds with no other gate on the box, and 1073 ms
    /// on the one round that shared it with a landing gate.
    #[test]
    fn the_exit_paths_collect_is_bounded_by_the_drain_grace() {
        const SEAM_MS: u64 = 3000;
        // Far above the seam and the grace together: the ceiling below has to
        // red on a grace that waits for the holder, and this is the life it
        // would be waiting out.
        const ESCAPEE_LIFE_SECONDS: u64 = 30;
        // Sized against the JOIN the ceiling has to refuse, never against a
        // quiet run's margin: the wait a lost bound becomes is the holder's
        // life, so anything under `ESCAPEE_LIFE_SECONDS` still reds while no
        // contention this suite's parallelism puts on the call — a spawn, a
        // kill and two channel reads, every one of which load only lengthens —
        // comes near it. An upper bound on elapsed wall-clock is a bound on the
        // box as much as on the code, and this is the width that makes it one
        // about the code.
        const SLACK_MS: u64 = 15_000;

        let rig = Rig::new("exit-path-grace");
        let answered = rig.root.join("the-child-answered");
        // Single-quoted for the reason the group-kill arms give.
        let body = format!(
            "#!/bin/sh\n\
             '{escape}' {ESCAPEE_LIFE_SECONDS} '{escaped}'\n\
             : > '{answered}'\n\
             echo '[]'\n",
            escape = rig.escape_path().display(),
            escaped = rig.escaped_path().display(),
            answered = answered.display()
        );

        let adapter = rig.stub_adapter("grace-stub", &body, Duration::from_millis(SEAM_MS));
        rig.owes_witness(&answered);
        let cause = cause_of(rig.read_with(&adapter));
        // The returning attempt's own span, not a clock around the wrapper: a
        // discarded attempt is time this arm's bounds must not see.
        let elapsed = rig.last_call();
        assert!(cause.contains("did not answer within"), "{cause}");
        assert!(
            rig.escaped_path().exists(),
            "nothing left the group, so the collect below is not the one this arm \
             names"
        );
        assert!(
            answered.exists(),
            "the child never reached its answer, so this call took the deadline \
             branch and the reading below is the other site's: {}",
            answered.display()
        );

        let grace = fleet_controller::platform::DRAIN_GRACE;
        let seam = Duration::from_millis(SEAM_MS);
        assert!(
            elapsed >= seam + grace,
            "the call took {elapsed:?}, under the {SEAM_MS} ms seam plus the \
             {grace:?} grace: the collect after the kill is not being waited on \
             at all"
        );
        assert!(
            elapsed < seam + grace + Duration::from_millis(SLACK_MS),
            "the call took {elapsed:?} against the {SEAM_MS} ms seam plus the \
             {grace:?} grace: the wait after the kill is not bounded by the \
             grace, and a holder outside the group can hold this call"
        );
    }

    /// The other half of the kill above. A killed child that nobody waits on is
    /// held by the OS as a zombie for as long as this process lives, which under
    /// a running controller is one entry per outrun poll.
    ///
    /// NEITHER SPAWN HERE IS READ INSIDE A FIXED WINDOW. The two terms a loaded
    /// box stretches by whole seconds are a spawn's own start cost and the setup
    /// a stub runs before the state an arm is about, and both are witnesses here
    /// rather than bounds: the control child marks its last line, and the
    /// listing's pid is owed to `witnessed`, so a call killed before it recorded
    /// one is discarded and re-attempted rather than asserted over. What is left
    /// inside a bound is only the transition each witness hands over — a shell's
    /// exit path, and a wait the adapter has already issued — which is what
    /// `SETTLE_WINDOW` is the unit of.
    #[test]
    fn an_outrun_listing_is_reaped_and_not_left_a_zombie() {
        let rig = Rig::new("zombie");

        // The probe's positive control, taken before the subject: a child this
        // arm spawns and deliberately does not wait on IS read as a zombie, so
        // the zero below is a reading and not a probe that can only count
        // nothing. It is asserted of THAT CHILD'S OWN PID and never of a count
        // over this process's children, which every other arm in this binary
        // feeds: a sibling's kill-then-wait window would satisfy a count here
        // while this arm's child had never been read at all.
        //
        // ITS MARKER IS THE SPAWN AND THE SETTLE IS THE EXIT. A shell reaching
        // its first line is the term measured in seconds on a contended box, so
        // the control spends the wrapper's own CALM budget waiting for the
        // marker and gives only the exit that follows it the settle. Read from
        // the rig rather than written here so the two move together; a box that
        // cannot start a shell at all inside it is the class `witnessed` refuses
        // on, and the refusal below says so rather than reporting a probe that
        // cannot read a zombie.
        let exited = rig.root.join("the-control-child-marked-its-last-line");
        let mut unwaited = Command::new("/bin/sh")
            .arg("-c")
            .arg(format!(": > '{}'", exited.display()))
            .spawn()
            .expect("a shell runs");
        let patience = Instant::now() + rig.calm_budget.get();
        while !exited.exists() && Instant::now() < patience {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            exited.exists(),
            "the control child never reached its last line inside the start \
             patience: this box is not starting a shell at all, which is the \
             class `Rig::witnessed` refuses on and not a reading of the probe. {}",
            exited.display()
        );
        let deadline = Instant::now() + SETTLE_WINDOW;
        while !is_a_zombie(unwaited.id()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            is_a_zombie(unwaited.id()),
            "the probe reads the child THIS arm left unwaited on purpose, at pid {}",
            unwaited.id()
        );
        unwaited.wait().expect("the control child is reaped");

        // THE PID IS OWED, AND THE WITNESS IS A SECOND FILE. `witnessed` reads
        // an EXISTENCE, while `echo $$ > f` creates the file and writes into it
        // as two steps — so the pid file alone would be reached, and empty, in
        // exactly the window this arm's own kill lands in. The marker is written
        // after it by the same shell in body order, so a marker that is there is
        // a pid that is whole; an attempt killed before either took no reading
        // of the reap and is discarded instead of read. Without it the kill
        // landing between the start marker and the pid write is a red naming a
        // file the box never gave the stub time to write.
        let pid_file = rig.stub_pid_path();
        let wrote_its_pid = rig.root.join("the-stub-recorded-its-pid");
        let adapter = rig.stub_adapter(
            "unreaped-stub",
            &format!(
                "#!/bin/sh\necho $$ > '{}'\n: > '{}'\nsleep 2\necho '[]'\n",
                pid_file.display(),
                wrote_its_pid.display()
            ),
            Duration::from_millis(200),
        );
        rig.owes_witness(&wrote_its_pid);
        let cause = cause_of(rig.read_with(&adapter));
        assert!(cause.contains("did not answer within"), "{cause}");

        let recorded = std::fs::read_to_string(&pid_file).unwrap_or_else(|e| {
            panic!(
                "the listing's child records its pid at {}: {e}",
                pid_file.display()
            )
        });
        let child: u32 = recorded.trim().parse().unwrap_or_else(|e| {
            panic!("the stub writes one pid and nothing else, {recorded:?}: {e}")
        });

        // Settled in the green direction only, as the count helpers are: a reap
        // already in flight is given the window to finish, and a child nobody
        // waits on never clears it. The window is the rig's own unit and not a
        // literal, so a slice that moves the settle moves this hold with it.
        let settle = Instant::now() + SETTLE_WINDOW;
        while is_a_zombie(child) && Instant::now() < settle {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !is_a_zombie(child),
            "the killed listing's own child is held as a zombie at pid {child}: \
             the listing was never reaped. The reading is the pid this call's \
             stub recorded, so no sibling's kill-to-wait window is standing in \
             for it"
        );
    }

    /// The deadline branch answers while a descendant still holds the pipe. The
    /// kill reaches the whole process group the child leads, so the descendant
    /// goes with it and both drains end: the joins that follow are bounded by
    /// the kill, and nothing is left parked on a pipe the deadline cannot close.
    #[test]
    fn an_outrun_listing_answers_while_a_descendant_still_holds_the_pipe() {
        let rig = Rig::new("held-pipe");
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "held-pipe-stub",
            "#!/bin/sh\n( sleep 5 ) &\nsleep 5\necho '[]'\n",
            Duration::from_millis(200),
        )));
        assert!(cause.contains("did not answer within"), "{cause}");
        // The module docstring's absolute form again: the margin is the stub's
        // 5 s sleep and its descendant's, against a 200 ms seam.
        assert!(
            rig.last_call() < Duration::from_secs(3),
            "the call waited on the pipe a descendant of the killed child holds: {:?}",
            rig.last_call()
        );

        assert!(
            matches!(readable(&rig), RosterRead::Readable(_)),
            "the control reads through the same rig and the same call, at a \
             deadline of its own that it fits"
        );
    }

    /// The fourth reading of a listing that did not succeed: one that died on a
    /// signal has no status to name. The cause says which it was, because
    /// "exited " with nothing after it reads as a listing that answered.
    #[test]
    fn a_listing_that_dies_on_a_signal_is_unreadable_and_names_no_status() {
        let rig = Rig::new("signalled");
        let cause = cause_of(rig.read_with(&rig.stub_adapter(
            "signalled-stub",
            "#!/bin/sh\nkill -TERM $$\n",
            Duration::from_secs(5),
        )));
        assert!(
            cause.contains("exited on a signal"),
            "a signalled listing is named as one: {cause}"
        );

        // The control: the same shape with a status names the status, so the
        // branch above is the missing code's and not the cause's only wording.
        let coded = cause_of(rig.read_with(&rig.stub_adapter(
            "coded-stub",
            "#!/bin/sh\nexit 7\n",
            Duration::from_secs(5),
        )));
        assert!(
            coded.contains("exited 7"),
            "a listing that exits names its status: {coded}"
        );
    }
}

/// All THREE causes the module above drives, taken end to end instead of at the
/// adapter: through the binary the seam resolves to, through the seat match, and
/// out into the published document a reader actually opens. That module takes
/// the three in FOUR readings, and the fourth is not driven here — a listing
/// that died on a signal is the exit-non-zero cause with no status to name, and
/// it is proved at the adapter and nowhere else.
///
/// SIX ARMS CARRY THE THREE. The status has one and the deadline has one; the
/// spawn failure is driven through three spellings of the binary seam — set,
/// blank, absent — and a fourth seam arm resolves the default on a `PATH` that
/// holds it, so that one's subject READS and a cause appears only in its
/// control.
///
/// A cause measured at the adapter proves the string; these prove it ARRIVES —
/// as `roster_unknown_cause` on a seat that is Unknown and never Absent.
///
/// Which binary the cause names is asserted only where the cause carries one:
/// the stub's own path for the two the seam points at it, `claude` for the
/// blank and absent seams, and nothing for the deadline, whose cause names the
/// deadline instead.
mod causes_end_to_end {
    use super::*;

    fn unknown_cause(rig: &Rig) -> String {
        let row = &rig.projection()["seats"][0];
        assert_eq!(
            row["roster_state"], "unknown",
            "a listing that could not be read is Unknown for the seat: {row}"
        );
        assert!(
            row["context_tokens"].is_null(),
            "and carries no context reading: {row}"
        );
        row["roster_unknown_cause"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn a_listing_that_exits_non_zero_reaches_the_document_with_its_status() {
        let rig = Rig::new("e2e-exit");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        // The stub's listing branch is `cat "$FLEET_TEST_ROSTER"`, so a roster
        // that is not there is a listing that exits non-zero end to end.
        std::fs::remove_file(rig.roster_path()).unwrap();

        let out = rig.observe();
        assert_eq!(
            out.status.code(),
            Some(0),
            "an unreadable listing is a reading, not an exit code: {}",
            stderr(&out)
        );
        let cause = unknown_cause(&rig);
        assert!(cause.contains("exited 1"), "the status arrives: {cause}");
        assert!(
            cause.contains(&rig.stub_path().display().to_string()),
            "named against the binary FLEET_CLAUDE_BIN pointed at: {cause}"
        );

        // The control: the same poll with the roster back reads the seat, so the
        // Unknown above is the failing listing's and not this rig's.
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        assert_eq!(rig.observe().status.code(), Some(0));
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");
    }

    #[test]
    fn a_binary_that_cannot_be_spawned_reaches_the_document_and_says_so() {
        let rig = Rig::new("e2e-spawn");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        std::fs::remove_file(rig.stub_path()).unwrap();

        // The bare path: a stub that cannot be spawned is this arm's SUBJECT, so
        // the never-started witness is the reading and not a lost attempt.
        let out = rig.observe_unwitnessed();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let cause = unknown_cause(&rig);
        assert!(
            cause.contains("could not start"),
            "a binary that will not spawn is named as such, and never as an empty fleet: {cause}"
        );
        assert!(
            cause.contains(&rig.stub_path().display().to_string()),
            "and the document names the binary FLEET_CLAUDE_BIN pointed at: {cause}"
        );
        assert!(
            rig.projection()["agent_version"].is_null(),
            "the same absent binary answers no version either"
        );

        // The control: the binary back in place reads the seat and the version.
        rig.write_stub_agent();
        assert_eq!(rig.observe().status.code(), Some(0));
        let published = rig.projection();
        assert_eq!(published["seats"][0]["roster_state"], "present");
        assert_eq!(published["agent_version"], "9.9.9");
    }

    /// The third reading of the binary seam, beside absent and set: a blank one.
    /// It falls to the default, so the cause names a binary — an empty program's
    /// cause reads `could not start : ...`, which sends the operator acting on
    /// an Unknown seat nowhere.
    ///
    /// BLANK HAS TWO SPELLINGS end to end, and both are driven: the empty string
    /// an `export FLEET_CLAUDE_BIN=` puts in the environment, and a value that
    /// is whitespace only, which is what a setting written with a stray space
    /// carries. The trim that makes them one reading is the seam's own, pinned
    /// at the unit level; this arm is where a trim dropped from the built binary
    /// would show as an empty program named to an operator.
    #[test]
    fn a_blank_binary_seam_reaches_the_document_naming_the_default_binary() {
        let rig = Rig::new("e2e-blank-bin");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let broken_path = rig.root.join("no-such-path");

        // The default is a bare name resolved on `PATH`, so a search path with
        // no such file in it makes the spawn fail wherever this runs, and the
        // cause below is the name's rather than this box's.
        for blank in ["", "   "] {
            let out = rig.observe_with_env(&[
                (common::hermetic::CLAUDE_BIN, Some(OsStr::new(blank))),
                ("PATH", Some(broken_path.as_os_str())),
            ]);
            assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

            let cause = unknown_cause(&rig);
            assert!(
                cause.contains("could not start claude:"),
                "a seam of {blank:?} is the default binary, never an empty \
                 program: {cause}"
            );
        }

        // The control on the SEAM, moving that variable and no other: the same
        // broken `PATH`, the same program every other arm drives, named this
        // time instead of left to the default. It reads, so the Unknown above is
        // the blank seam's and not the search path's.
        let out = rig.observe_with_env(&[
            (
                common::hermetic::CLAUDE_BIN,
                Some(rig.stub_path().as_os_str()),
            ),
            ("PATH", Some(broken_path.as_os_str())),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");

        // AND NO CONTROL ON `PATH` HERE. One would have to keep the failing
        // case's own blank seam and give the default bare name a search path
        // that resolves; naming the stub instead moves the subject off `PATH`
        // altogether, so restoring the search path beside it varies a variable
        // nothing in the call reads and cannot fail unless the control above
        // does. That reading is
        // `a_blank_binary_seam_resolves_the_default_on_path_and_reads_it`, an
        // arm of its own because it needs a stub installed under the default's
        // bare name.
    }

    /// The seam's first reading, which every arm above and below sets: ABSENT.
    /// The rig exports it on every call, so nothing else here drives the branch
    /// an operator who never exported it runs on, and the default is the same
    /// bare name the blank seam falls to.
    #[test]
    fn an_absent_binary_seam_reaches_the_document_naming_the_default_binary() {
        let rig = Rig::new("e2e-absent-bin");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let broken_path = rig.root.join("no-such-path");

        let out = rig.observe_with_env(&[
            (common::hermetic::CLAUDE_BIN, None),
            ("PATH", Some(broken_path.as_os_str())),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        let cause = unknown_cause(&rig);
        assert!(
            cause.contains("could not start claude:"),
            "an unset seam is the default binary, exactly as a blank one is: {cause}"
        );

        // The control on the SEAM, and the only one this arm carries: the same
        // broken `PATH`, the same program, set this time instead of absent. The
        // reading that varies `PATH` is
        // `a_blank_binary_seam_resolves_the_default_on_path_and_reads_it`, for
        // the reason the blank-seam twin above states.
        let out = rig.observe_with_env(&[
            (
                common::hermetic::CLAUDE_BIN,
                Some(rig.stub_path().as_os_str()),
            ),
            ("PATH", Some(broken_path.as_os_str())),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");
    }

    /// The reading the two arms above cannot take: the default binary RESOLVED
    /// and READ. Both of them break `PATH` on purpose, so what they pin is the
    /// NAME the fallback carries into a failed spawn — and a controller that
    /// resolved the default and could not run it would pass either of them.
    /// Here the name is on a `PATH` the rig owns, so the poll runs the default
    /// and the document carries what it answered.
    #[test]
    fn a_blank_binary_seam_resolves_the_default_on_path_and_reads_it() {
        let rig = Rig::new("e2e-default-read");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let bin_dir = rig.write_default_named_stub();

        let out = rig.observe_with_env(&[
            (common::hermetic::CLAUDE_BIN, Some(OsStr::new(""))),
            ("PATH", Some(bin_dir.as_os_str())),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        let published = rig.projection();
        assert_eq!(
            published["seats"][0]["roster_state"], "present",
            "the listing the DEFAULT binary answered is what the row reads"
        );
        assert_eq!(
            published["agent_version"], "9.9.9",
            "and the version call resolved the same name: {}",
            published["agent_version"]
        );

        // The control on `PATH`, that variable and no other: the same blank seam
        // with the rig's directory taken off the search path. It goes Unknown
        // naming the default, so the reading above is THIS stub answering under
        // the name `claude` and not some agent installed on this box.
        let out = rig.observe_with_env(&[
            (common::hermetic::CLAUDE_BIN, Some(OsStr::new(""))),
            ("PATH", Some(rig.root.join("no-such-path").as_os_str())),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let cause = unknown_cause(&rig);
        assert!(
            cause.contains("could not start claude:"),
            "with the rig's directory off the search path the same seam resolves \
             nothing: {cause}"
        );
    }

    /// The deadline the BINARY runs on, through the seam, with the figure the
    /// cause carries asserted: at 300 ms a whole-seconds format renders "0s",
    /// which names no deadline at all.
    #[test]
    fn a_listing_that_outruns_the_deadline_reaches_the_document_naming_the_deadline() {
        let mut rig = Rig::new("e2e-deadline");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        rig.agent_timeout_ms = Some(300);
        rig.hang_seconds = Some(3);

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        // The module docstring's absolute form, at the built binary: the margin
        // is the 3 s hang this arm set against a 300 ms seam. The ceiling is the
        // hang and not half of it, because this poll pays the process start-up
        // the adapter-level siblings do not.
        assert!(
            rig.last_call() < Duration::from_secs(3),
            "the poll returned before the listing would have: {:?}",
            rig.last_call()
        );
        let cause = unknown_cause(&rig);
        assert!(
            cause.contains("did not answer within 300ms"),
            "the deadline the seam set is the deadline the cause names: {cause}"
        );

        // The control: the same rig, the same stub, the hang AND the shortened
        // seam both cleared, reads the seat — so the Unknown above is the hang's
        // and not a rig that cannot read one.
        //
        // The seam is cleared and not merely widened: a control that keeps it
        // asserts a healthy spawn finishes inside a stated figure, and that is
        // the second kind of elapsed bound the module docstring names — its
        // margin is this box's own speed, so it reds under this suite's own
        // parallelism while the code is right. What that costs is stated —
        // nothing here says the seam
        // ADMITS a listing that fits it; the arms in `adapter::claude_code` pin
        // the figure the seam maps to, and the half above pins that the binary
        // reads it.
        rig.hang_seconds = None;
        rig.agent_timeout_ms = None;
        assert_eq!(rig.observe().status.code(), Some(0));
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");
    }
}
