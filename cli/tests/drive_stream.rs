//! What the loop states: the stream it writes, and the announcements that stand.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

mod lessons {
    use super::*;

    /// gas-city G17 — eighty percent of one stream was the controller reporting
    /// on itself. The content rule is that inversion: the events a person cares
    /// about, and NOTHING per poll. Three polls here, one event.
    #[test]
    fn the_stream_carries_what_a_person_asks_about() {
        let rig = Rig::new("stream");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        for _ in 0..3 {
            assert_eq!(rig.observe().status.code(), Some(0));
        }
        let events = rig.events();
        assert_eq!(
            events.len(),
            3,
            "one controller.started per process and nothing per poll: {events:#?}"
        );
        assert_eq!(rig.events_of("controller.started"), 3);
        for kind in [
            "controller.stopped",
            "substrate.moved",
            "session.spawned",
            "session.nudged",
        ] {
            assert_eq!(rig.events_of(kind), 0, "{kind} is not a per-poll event");
        }
        // The sequence is the stream's, not the process's: three separate runs
        // and no line renumbered.
        let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    /// gas-city G20 — an order is one flat file with a history. The file is
    /// dropped into a directory and the tick reads it; what it did is the
    /// stream's own rows, and `fleet routine history` is how a person reads them
    /// back.
    ///
    /// Five consecutive minutes of a STEPPED clock, so the arm measures the
    /// schedule and not this box's own hour: the seam is a file the binary
    /// re-reads on every tick.
    #[test]
    fn an_order_is_one_flat_file_with_a_history() {
        let rig = Rig::new("order-history");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        rig.write_routine(
            "beat",
            "[order]\ndescription = \"a beat every minute\"\ntrigger = \"cron\"\n\
             schedule = \"* * * * *\"\n[action.exec]\ncommand = 'true'\n",
        );

        // A minute boundary, so each step lands on the next whole minute.
        let base = 1_788_600_000;
        for minute in 0..5 {
            rig.set_clock(base + minute * 60);
            assert_eq!(rig.observe().status.code(), Some(0));
        }

        let read_back = rig
            .binary()
            .args(["routine", "history", "beat"])
            .output()
            .expect("the built binary runs");
        assert_eq!(read_back.status.code(), Some(0));
        let rows: Vec<String> = String::from_utf8_lossy(&read_back.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        let fired = rows.iter().filter(|r| r.contains("routine.fired")).count();
        let completed = rows
            .iter()
            .filter(|r| r.contains("routine.completed"))
            .count();
        assert_eq!(fired, 5, "one firing per minute: {rows:#?}");
        assert_eq!(completed, 5, "and one terminal row each: {rows:#?}");
        assert_eq!(rows.len(), 10, "and nothing else: {rows:#?}");
        assert!(rows.iter().all(|row| row.contains("beat")));
        assert!(
            rows.iter()
                .filter(|r| r.contains("routine.completed"))
                .all(|r| r.contains("ran")),
            "{rows:#?}"
        );

        // The history is the STREAM's and not a second file: no ledger is
        // written beside it.
        assert!(
            !rig.machine().join("orders").join("ledger.jsonl").exists(),
            "the stream is the ledger"
        );
    }
}

/// The once-per-change clause (`run.rs`: "Once per change, never once per
/// poll"). It is stated on stderr and nowhere else, so the arm keeps the stream.
#[test]
fn a_policy_that_stops_parsing_is_logged_once_per_change_and_not_once_per_poll() {
    const LINE: &str = "running on last-good policy";

    let rig = Rig::new("log-once");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.driving(|polls| {
        polls.tick();
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");

        rig.write_policy("[controller\npoll_seconds = 1\n");
        polls.tick();
        assert!(
            rig.projection().get("fleet_parse_error").is_some(),
            "the broken file raises the flag"
        );

        // The control for "once": three more polls run while the file stays
        // broken, so one line is a line per CHANGE and not a line that happened
        // once. Each `tick` is the poll itself, so the polls needing no witness
        // beyond the calls is what this form buys the arm.
        polls.tick();
        polls.tick();
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(LINE),
            1,
            "one change is one line, however many polls read it"
        );

        // A second change is a second line — otherwise the count above would
        // pass on a controller that logs the failure once and never again.
        rig.write_policy(
            "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"9.9.9\"\n",
        );
        polls.tick();
        assert!(rig.projection().get("fleet_parse_error").is_none());
        rig.write_policy("[controller\npoll_seconds = 1\nbroken again\n");
        polls.tick();
        assert!(rig.projection().get("fleet_parse_error").is_some());
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(LINE),
            2,
            "a second change is a second line"
        );
    });
}

/// The counter answers for a stream that is there. A rig that captured nothing
/// has no stream, and a zero for it would let a zero-count assertion pass
/// against a controller that never ran.
#[test]
#[should_panic(expected = "the captured stream at")]
fn counting_a_stream_that_was_never_captured_is_a_refusal_and_never_a_zero() {
    let rig = Rig::new("no-capture");
    let _ = rig.stderr_lines_with("fleet observe:");
}

/// The fact that puts the arm above out of every other arm's reach:
/// `spawn_loop` creates the stream before it spawns, so a rig that has spawned
/// always has one and the refusal can only be met by a rig that has not.
///
/// Unpinned, this is the arm above's whole standing: a `spawn_loop` that
/// stopped capturing would make the refusal reachable everywhere, and the
/// zero-count assertions across this suite would start reading a controller
/// that never ran as a controller that said nothing.
#[test]
fn a_spawned_loop_has_its_stream_before_the_child_can_write_to_it() {
    let rig = Rig::new("captures");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));

    // The control: the state the arm above needs, on this same rig, before the
    // spawn — so the existence below is the spawn's doing.
    assert!(
        !rig.stderr_path().exists(),
        "a rig that has not spawned has no stream, which is what that arm refuses on"
    );

    let _controller = rig.spawn_loop();
    assert!(
        rig.stderr_path().exists(),
        "spawn_loop creates the stream as it spawns, so no arm that has spawned can reach that refusal"
    );
    // And the reading the refusal must never be confused with: a stream that IS
    // there and holds nothing matching answers 0.
    assert_eq!(
        rig.stderr_lines_with("a needle no controller ever writes"),
        0,
        "a stream that is there and does not carry the needle counts 0"
    );
}

/// A live seat whose transcript is not there. The row is Present and the reading
/// is a named absence: a seat with no context figure must not read as a seat
/// carrying zero, and must not disturb the state the roster reported.
#[test]
fn a_present_seat_whose_transcript_cannot_be_read_publishes_no_reading() {
    let rig = Rig::new("no-transcript");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));

    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(
        row["roster_state"], "present",
        "an unreadable transcript is not a roster fact"
    );
    assert!(
        row["context_tokens"].is_null(),
        "a reading nobody took is absent, never 0"
    );

    // The control: the same row with the transcript in place reads a figure, so
    // the null above is the missing file's and not this rig's.
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(rig.projection()["seats"][0]["context_tokens"], 18);
}

/// A binary that answers `--version` with success and nothing to read. This is
/// the other half of "no version": the failing read exits non-zero, and this one
/// exits 0 with no token in it, which is the path the parse returns None on.
#[test]
fn an_agent_that_answers_no_version_publishes_a_null_and_announces_no_move() {
    let rig = Rig::new("no-version");
    rig.set_version("SILENT");

    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let published = rig.projection();
    assert!(
        published["agent_version"].is_null(),
        "an empty answer is no version, never an empty-string version: {}",
        published["agent_version"]
    );
    assert_eq!(published["agent_version_expected"], "9.9.9");
    assert_eq!(
        rig.events_of("substrate.moved"),
        0,
        "a poll that read no version knows nothing about the spread"
    );

    // The control: the same stub answering normally reads a version and agrees
    // with the pin, so the null above is the empty answer's.
    rig.set_version("9.9.9");
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(rig.projection()["agent_version"], "9.9.9");
}

/// SIGINT is the other signal the platform layer arms, and it owes the same
/// clean exit and the same one `controller.stopped` as SIGTERM.
#[test]
fn an_interrupt_stops_the_loop_as_cleanly_as_a_termination() {
    let rig = Rig::new("sigint");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    let mut child = rig.spawn_loop();
    assert!(rig.wait_until(|p| p["seats"][0]["roster_state"] == "present"));

    let status = child.signal_and_wait("-INT");
    assert_eq!(
        status.code(),
        Some(0),
        "an interrupted stop is a clean exit"
    );
    assert_eq!(rig.events_of("controller.started"), 1);
    assert_eq!(
        rig.events_of("controller.stopped"),
        1,
        "SIGINT owes the stop event SIGTERM owes"
    );
}

/// `signal_and_wait` sends the signal it is HANDED. Both arms above pass a
/// signal the loop handles, and the handler discards which one it was
/// (`platform::raise_stop` takes `_sig`), so those two cannot tell a helper
/// that forwards its argument from one that hardcodes `-TERM` — and a hardcode
/// silently makes the interrupt arm a second copy of the termination arm.
///
/// SIGKILL is the reading that separates them, because it is the one signal the
/// loop cannot answer: no handler runs, so there is no clean exit code and no
/// `controller.stopped`. Under a `-TERM` hardcode this arm reads the clean stop
/// instead, which is a red on both halves.
#[test]
fn the_stop_helper_sends_the_signal_it_is_given_and_not_a_fixed_one() {
    let rig = Rig::new("forwarded");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    let mut child = rig.spawn_loop();
    assert!(rig.wait_until(|p| p["seats"][0]["roster_state"] == "present"));
    // The control: this loop DOES answer the signals it handles, so the two
    // absences below are SIGKILL's and not a loop that never started.
    assert_eq!(
        rig.events_of("controller.started"),
        1,
        "the loop is up and writing events"
    );

    let status = child.signal_and_wait("-KILL");
    assert_eq!(
        status.code(),
        None,
        "a killed loop has no exit code of its own, where a signalled stop exits 0"
    );
    assert_eq!(
        rig.events_of("controller.stopped"),
        0,
        "and it never reaches the event a handled signal owes"
    );
}

/// The publish cannot land. The poll says so on stderr, naming the path, and
/// still exits 0: a projection that cannot be written is not a reason to stop
/// observing.
#[test]
fn a_publish_that_cannot_land_is_named_and_is_not_a_failure() {
    let rig = Rig::new("no-publish");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    // A non-empty directory standing where the document goes: the rename over it
    // is what fails, which is the write's own failure path and not a missing
    // parent.
    let blocked = rig.machine().join("projection.json");
    write(&blocked.join("occupied"), "in the way");

    let out = rig.observe();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a publish that cannot land is not an exit code"
    );
    assert!(
        stderr(&out).contains("could not publish"),
        "the failure is named: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains(&blocked.display().to_string()),
        "and names the path it could not write: {}",
        stderr(&out)
    );

    // The control: with the way clear the same poll publishes, so the message
    // above is the blocked path's and not a poll that never got that far.
    std::fs::remove_dir_all(&blocked).unwrap();
    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        !stderr(&out).contains("could not publish"),
        "{}",
        stderr(&out)
    );
    assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");
}

/// The deadline is on EVERY call to the agent binary and not on the listing
/// alone (`README.md`). The version call runs under the same seam, and a poll
/// whose version read outruns it publishes a null instead of waiting the read
/// out.
///
/// Three readings against ONE hang, on two axes. What each seam PUBLISHES says
/// which figure the call runs on: a seam under the hang publishes no version; a
/// seam whose TEN TIMES would clear the hang publishes no version either; a seam
/// ABOVE the hang publishes the version, at a seam the hanging call FITS rather
/// than with the hang cleared.
///
/// What each poll TOOK says the deadline is enforced, and it is read RELATIVELY
/// — the two seam-bounded polls against the one that sits through the whole
/// hang, no constant anywhere. A call bounded at an hour and discarded after the
/// fact publishes the same three values and waits the hang out every time, so
/// the three elapsed figures converge and only this reading separates them. The
/// margin is the hang itself, which is what keeps it off the shared box's clock:
/// load moves all three figures together. This is the module docstring's
/// relative form, and the absolute form is not wrong elsewhere — it is
/// unavailable HERE, because the seam is what this arm varies and no single
/// figure separates a bounded call from an unbounded one across three of them.
///
/// THE HANG IS 5 s AND THE ARM COSTS ABOUT THAT: the third reading has to sit
/// through the whole hang, which is what makes it the patient one. The hang is
/// also the entire margin of the two assertions below, so shortening it is not
/// free and is declined — a shorter one leaves the patient poll less room than
/// a loaded box moves a poll by, and the arm then reds on load rather than on
/// the deadline.
#[test]
fn a_version_call_that_outruns_the_deadline_publishes_no_version() {
    let mut rig = Rig::new("version-deadline");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.version_hang_seconds = Some(5);

    rig.agent_timeout_ms = Some(300);
    let out = rig.observe();
    let tight = rig.last_call();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        rig.projection()["agent_version"].is_null(),
        "a version call that outran its deadline is no version: {}",
        rig.projection()["agent_version"]
    );

    rig.agent_timeout_ms = Some(1000);
    let out = rig.observe();
    let wider = rig.last_call();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        rig.projection()["agent_version"].is_null(),
        "the version call runs on the configured seam and not on a multiple of \
         it: ten times this one clears the hang and reads a version: {}",
        rig.projection()["agent_version"]
    );

    rig.agent_timeout_ms = Some(10_000);
    let out = rig.observe();
    let patient = rig.last_call();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        rig.projection()["agent_version"],
        "9.9.9",
        "a seam the hanging version call fits reads the version, so the nulls \
         above are the seam's and not a rig that cannot read one, and the seam \
         is the figure the call runs on rather than a constant under it: {}",
        rig.projection()["agent_version"]
    );

    // The enforcement reading. The poll that sits through the hang is the unit,
    // so every figure here is that poll's: a seam-bounded call returns while the
    // hang still runs, and half of the patient poll is the loosest bound that
    // still separates it from one that waited the hang out.
    assert!(
        tight * 2 < patient,
        "the 300ms seam did not bound the version call: {tight:?} against the \
         {patient:?} of the poll that sits through the hang"
    );
    assert!(
        wider * 2 < patient,
        "the 1000ms seam did not bound the version call: {wider:?} against the \
         {patient:?} of the poll that sits through the hang"
    );
}

/// The close the two standing-announcement arms rest on and neither reaches:
/// their control moves to a NEW version, which announces whether or not the
/// announcement was closed. The reading that separates the two is the SAME pair
/// returning after a version that agreed with the pin cleared it.
#[test]
fn a_spread_that_returns_after_it_was_closed_is_announced_again() {
    let rig = Rig::new("reopened");
    rig.set_version("9.9.10");
    rig.driving(|polls| {
        polls.tick();
        assert_eq!(
            rig.projection()["agent_version"],
            "9.9.10",
            "the loop publishes the moved version"
        );

        // The close: a version that IS read and DOES agree with the pin.
        rig.set_version("9.9.9");
        polls.tick();
        assert_eq!(rig.projection()["agent_version"], "9.9.9");

        rig.set_version("9.9.10");
        polls.tick();
        assert_eq!(rig.projection()["agent_version"], "9.9.10");
        polls.tick();
        let moved: Vec<serde_json::Value> = rig
            .events()
            .into_iter()
            .filter(|e| e["type"] == "substrate.moved")
            .collect();
        assert_eq!(
            moved.len(),
            2,
            "the spread that returned after the close is a second event"
        );
        // The PAIR, on both events and not just their number: this arm is about
        // one pair announced, closed and announced again, and a count of two is
        // equally satisfied by two announcements of different pairs — which is
        // the reading the control below moves to a new version to produce.
        for (nth, event) in moved.iter().enumerate() {
            assert_eq!(
                event["payload"]["observed"],
                "9.9.10",
                "announcement {} carries the observed half of the pair that returned",
                nth + 1
            );
            assert_eq!(
                event["payload"]["expected"],
                "9.9.9",
                "announcement {} carries the pinned half",
                nth + 1
            );
            assert_eq!(event["actor"], "controller", "announcement {}", nth + 1);
        }
    });
}

/// The THIRD way to have no version, at the announcement clause. A read that
/// FAILED and a read that was SILENT each have an arm holding this clause; a
/// read the deadline ended had none, and it is the one that arrives on a healthy
/// fleet whose agent binary is merely slow.
///
/// The clause: only a version that WAS READ and agrees with the pin closes a
/// standing announcement. A poll whose version call was outrun knows nothing
/// about the spread, so clearing on it would re-announce the same move on the
/// next healthy poll — one event per hang, on a fleet that has not moved.
///
/// WHAT SEPARATES "NOT CLOSED" FROM "CLOSED AND NOT RE-ANNOUNCED" is the last
/// reading and not the middle one: a count that stays at 1 through the hang is
/// equally satisfied by an announcement that was closed and by one that stands,
/// because neither announces during a poll with no version to compare. So the
/// hang is lifted and the SAME pair returns: a closed announcement announces it
/// again, which is what `a_spread_that_returns_after_it_was_closed_is_announced_again`
/// measures, and a standing one does not.
#[test]
fn an_outrun_version_call_does_not_close_a_standing_announcement() {
    let mut rig = Rig::new("outrun-close");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.agent_timeout_ms = Some(300);
    rig.set_version("9.9.10");

    let rig = rig;
    rig.driving(|polls| {
        // The one place this arm waits: its deadline is 300 ms by design, so the
        // healthy poll the hang runs against is polled for rather than assumed.
        polls.until(20, |p| p["agent_version"] == "9.9.10");
        assert_eq!(
            rig.events_of("substrate.moved"),
            1,
            "the move is announced once, which is what the hang below runs against"
        );

        // The version call now outruns its deadline on every poll: no version at
        // all, by the third path. The hang is a file the stub re-reads per call,
        // so it is armed between two polls of one loop.
        rig.set_version_hang(5);
        polls.tick();
        assert!(
            rig.projection()["agent_version"].is_null(),
            "a version call that outran its deadline publishes no version"
        );
        // A second poll of it, so a per-poll announcement would show as more
        // than one.
        polls.tick();
        assert_eq!(
            rig.events_of("substrate.moved"),
            1,
            "a poll that read no version neither announces nor re-announces"
        );

        rig.clear_version_hang();
        // The same wait, for the same reason. Every poll it spends is one more
        // with no version, which leaves a standing announcement exactly where
        // the two above left it.
        polls.until(20, |p| p["agent_version"] == "9.9.10");
        polls.tick();
        assert_eq!(
            rig.events_of("substrate.moved"),
            1,
            "the same pair returning is not a second event, so the polls with no \
             version never closed the announcement — a closed one announces again"
        );
    });
}

/// A configured worktree is a spelling of a directory. The matcher already puts
/// both sides in one form; the transcript lookup consumes the configured path,
/// and a trailing separator there encodes to a project directory the agent never
/// wrote — a seat that reads Present and carries no context, forever.
#[test]
fn a_configured_worktree_with_a_trailing_separator_still_reads_a_context() {
    let rig = Rig::new("slashed");
    rig.write_config(&format!(
        r#"{{"fleet_toml": "{}", "children": [
             {{"name":"builder-1","chosen_name":"Orla","worktrees":{{"demo":"{}/"}}}}
           ]}}"#,
        rig.policy_path().display(),
        rig.worktree().display()
    ));
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );

    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(
        row["roster_state"], "present",
        "a trailing separator is not a different directory"
    );
    assert_eq!(
        row["context_tokens"], 18,
        "and the transcript is found at the directory's encoding, not at the \
         configured spelling's"
    );
    assert_eq!(
        row["worktree"],
        serde_json::json!(rig.worktree().display().to_string()),
        "and the published spelling is the directory's too, so a reader matching \
         it against an agent's cwd needs no normalisation of its own"
    );

    // The control: the same rig with the transcript gone reads no figure, so the
    // 18 above came from the file the agent wrote.
    std::fs::remove_dir_all(rig.home().join(".claude")).unwrap();
    assert_eq!(rig.observe().status.code(), Some(0));
    assert!(rig.projection()["seats"][0]["context_tokens"].is_null());
}

/// The same field's OTHER case, end to end. The arm above pins the spelling a
/// seat that has one directory publishes; this one pins what a seat that has no
/// single answer publishes instead, which is the key present and null.
///
/// Null and an empty string are different answers to a reader matching this
/// against an agent's `cwd`: null is "this seat names no one directory", and ""
/// is a directory — the root's parent, or a path that failed to render, and
/// either way something to go looking for. A publish site that renders the
/// absent case as "" is a live shape, not a hypothetical: the field is built by
/// mapping an `Option`, and any unwrap with a default under that map produces
/// exactly it while every present-case arm stays green.
///
/// The seat is registered on two projects and no row matches it, which is where
/// `observe::unmatched` answers with neither project nor worktree — a seat is
/// one session, and a seat on several projects has no one place it would be
/// found.
#[test]
fn a_seat_with_no_one_worktree_publishes_a_null_and_not_an_empty_string() {
    let rig = Rig::new("ambiguous");
    rig.write_config(&format!(
        r#"{{"fleet_toml": "{}", "children": [
             {{"name":"builder-1","worktrees":{{"demo":"{}","other":"{}"}}}}
           ]}}"#,
        rig.policy_path().display(),
        rig.worktree().display(),
        rig.worktree().join("elsewhere").display()
    ));
    rig.write_roster("[]");

    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(
        row["roster_state"], "absent",
        "no row matched the seat, which is the case this arm is about"
    );
    assert!(
        row["worktree"].is_null(),
        "a seat on two projects that no row matched names no worktree, and a \
         reading nobody took is null: {}",
        row["worktree"]
    );
    assert!(
        row["project"].is_null(),
        "and the project beside it, for the same reason: {}",
        row["project"]
    );

    // The control on the AMBIGUITY, that variable and no other: the same rig and
    // the same empty roster with ONE project configured names both fields, so
    // the nulls above are the two entries' and not an absent seat publishing
    // nothing whatever it was configured with.
    rig.write_config(&format!(
        r#"{{"fleet_toml": "{}", "children": [
             {{"name":"builder-1","worktrees":{{"demo":"{}"}}}}
           ]}}"#,
        rig.policy_path().display(),
        rig.worktree().display()
    ));
    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "absent");
    assert_eq!(
        row["worktree"],
        serde_json::json!(rig.worktree().display().to_string())
    );
    assert_eq!(row["project"], "demo");
}

/// The other input to the same null. A missing file is one way to have no
/// reading; a file that IS read and states no window is the other, and the agent
/// writes exactly that for a turn that made no model call.
#[test]
fn a_transcript_that_states_no_window_publishes_no_reading() {
    const NO_WINDOW: &str = "{\"type\":\"assistant\",\"message\":{\"usage\":{\
                             \"input_tokens\":0,\"cache_read_input_tokens\":0,\
                             \"cache_creation_input_tokens\":0}}}\n";
    let rig = Rig::new("no-window");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.write_transcript("a-session", NO_WINDOW);

    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "present");
    assert!(
        row["context_tokens"].is_null(),
        "an entry stating no window is no reading, never a seat carrying 0: {}",
        row["context_tokens"]
    );

    // The control: the same path, read the same way, with a window in it. The
    // null above is the entry's and not a transcript nobody found.
    rig.write_transcript(
        "a-session",
        &format!(
            "{NO_WINDOW}{}",
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n"
        ),
    );
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(rig.projection()["seats"][0]["context_tokens"], 18);
}

/// "Once per change, never once per poll" is a property of every line the loop
/// states on a change. The three driven here are the seat list re-pointing the
/// policy path, a seat list that will not parse leaving the last-good one
/// standing, and the policy itself being re-read; all three are stated on
/// stderr and nowhere else, so the arm keeps the stream.
#[test]
fn every_change_the_loop_states_is_one_line_and_not_one_line_per_poll() {
    const REPOINT: &str = "the seat list re-points policy to";
    const STANDS: &str = "the seat list stands";
    const REREAD: &str = "policy re-read from";

    let rig = Rig::new("log-siblings");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.driving(|polls| {
        polls.tick();
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");
        assert_eq!(
            rig.stderr_lines_with(REPOINT),
            0,
            "nothing has re-pointed yet"
        );

        // (1) The seat list re-points the policy path.
        write(
            &rig.second_policy_path(),
            "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"7.7.7\"\n",
        );
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
             {{"name":"builder-1","chosen_name":"Orla","worktrees":{{"demo":"{}"}}}}
           ]}}"#,
            rig.second_policy_path().display(),
            rig.worktree().display()
        ));
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(REPOINT),
            1,
            "the re-point is stated: {}",
            std::fs::read_to_string(rig.stderr_path()).unwrap_or_default()
        );

        // (2) The seat list stops parsing. The list stands, and so does the line.
        rig.write_config("{\"fleet_toml\": \"/somewhere\", \"children\": [");
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(STANDS),
            1,
            "the broken re-read is stated"
        );

        // The control for "once": two more polls run while both states stand, so
        // one line each is a line per CHANGE and not a line that happened once.
        polls.tick();
        polls.tick();
        assert_eq!(rig.stderr_lines_with(REPOINT), 1, "one re-point, one line");
        assert_eq!(
            rig.stderr_lines_with(STANDS),
            1,
            "one seat list that will not parse, one line"
        );
        assert_eq!(
            rig.stderr_lines_with(REREAD),
            1,
            "the re-point is also a policy in force that changed, and that is its \
             own one line"
        );

        // (3) The policy file's CONTENT moves under a path that has not. The
        // re-read is stated once; the re-point is not restated, because the path
        // the seat list names has not moved.
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
             {{"name":"builder-1","chosen_name":"Orla","worktrees":{{"demo":"{}"}}}}
           ]}}"#,
            rig.second_policy_path().display(),
            rig.worktree().display()
        ));
        polls.tick();
        assert_eq!(rig.projection()["seats"].as_array().map(Vec::len), Some(1));
        write(
            &rig.second_policy_path(),
            "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"7.7.8\"\n",
        );
        polls.tick();
        assert_eq!(
            rig.projection()["agent_version_expected"],
            "7.7.8",
            "the moved file is re-read"
        );
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(REREAD),
            2,
            "one policy change, one line, however many polls stand on it"
        );
        assert_eq!(
            rig.stderr_lines_with(REPOINT),
            1,
            "a policy whose CONTENT moved is not a policy path that moved"
        );

        // (4) A SECOND re-point, and a SECOND seat list that will not parse.
        // Every count these two carry above is 1, and a line stated once and
        // never again reads as 1 too — so each needs a second change of its own
        // before "once per change" is what has been measured. `REREAD` already
        // has its pair.
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
             {{"name":"builder-1","chosen_name":"Orla","worktrees":{{"demo":"{}"}}}}
           ]}}"#,
            rig.policy_path().display(),
            rig.worktree().display()
        ));
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(REPOINT),
            2,
            "a second path the seat list names is a second re-point line: {}",
            std::fs::read_to_string(rig.stderr_path()).unwrap_or_default()
        );
        rig.write_config("{\"fleet_toml\": \"/somewhere\", \"children\": [");
        polls.tick();
        // Two more polls under both standing states, so each count above is a
        // line per change rather than a line a poll repeats.
        polls.tick();
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(REPOINT),
            2,
            "two re-points, two lines"
        );
        assert_eq!(
            rig.stderr_lines_with(STANDS),
            2,
            "two seat lists that will not parse, two lines"
        );

        // (5) The policy file rewritten with the content it already has. The
        // mtime moves, so the re-read runs; the policy does not, so no line is
        // owed — which is the guard the line is stated behind and not the gate
        // above it.
        let reread_before = rig.stderr_lines_with(REREAD);
        // The witness that the re-read RAN. A count that held is a null reading:
        // it is what a re-read that never happened publishes too. The stamp is
        // set from the file on every successful re-read and not only on one that
        // changed something, so a stamp that moved is the re-read itself, taken
        // on content the loop then found identical.
        let stamp_before = rig.projection()["fleet"]["mtime"].clone();
        assert!(
            stamp_before.is_string(),
            "the policy in force carries a stamp to move: {stamp_before}"
        );
        let unchanged =
            std::fs::read_to_string(rig.policy_path()).expect("the policy file is there");
        write(&rig.policy_path(), &unchanged);
        // The stamp is second-granularity, so the move it has to show is put
        // there rather than left to whichever second this poll lands in. AN
        // HOUR, and not the few seconds that would do on a quiet box: the
        // distance has to be longer than this arm's own slowest run, or a loaded
        // box lands the aged stamp back on the second the previous re-read
        // recorded and the witness reads as a re-read that never ran.
        age_mtime(&rig.policy_path(), Duration::from_secs(3600));
        polls.tick();
        assert_eq!(
            rig.stderr_lines_with(REREAD),
            reread_before,
            "a re-read that read the policy already in force states no line"
        );
        // The witness for the count above, which on its own is a null reading: a
        // re-read that never ran states no line either. The stamp moving is the
        // re-read, and it is asserted after the silence so a run under a loop
        // that skipped the re-read shows both halves — the count still held, and
        // this is what caught it.
        assert_ne!(
            rig.projection()["fleet"]["mtime"],
            stamp_before,
            "the stamp of the policy in force did not move, so no re-read ran on \
             the rewritten file and the silence above is a re-read that never \
             happened"
        );
    });
}

/// The other no-version path against a STANDING announcement. `SILENT` answers
/// with success and nothing to read, where the base arm's `FAIL` exits non-zero,
/// and neither knows anything about the spread: closing on one re-announces the
/// same move on the next healthy poll.
#[test]
fn a_poll_that_reads_no_version_leaves_a_standing_announcement_standing() {
    let rig = Rig::new("silent-standing");
    rig.set_version("9.9.10");
    rig.driving(|polls| {
        polls.tick();
        assert_eq!(
            rig.projection()["agent_version"],
            "9.9.10",
            "the loop publishes the moved version"
        );

        rig.set_version("SILENT");
        polls.tick();
        assert!(
            rig.projection()["agent_version"].is_null(),
            "an answer with no version in it publishes a null"
        );
        rig.set_version("9.9.10");
        polls.tick();
        assert_eq!(rig.projection()["agent_version"], "9.9.10");
        polls.tick();
        assert_eq!(
            rig.events_of("substrate.moved"),
            1,
            "a poll that read no version is not a version that agrees with the pin"
        );

        // The control: a version that IS read and DOES agree closes the
        // announcement, so a later move announces again — otherwise the count
        // above would pass on a controller that never announces twice for any
        // reason.
        rig.set_version("9.9.9");
        polls.tick();
        assert_eq!(rig.projection()["agent_version"], "9.9.9");
        rig.set_version("9.9.11");
        polls.tick();
        assert_eq!(rig.projection()["agent_version"], "9.9.11");
        polls.tick();
        assert_eq!(
            rig.events_of("substrate.moved"),
            2,
            "a second move is a second event"
        );
    });
}
