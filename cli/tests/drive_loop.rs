//! The loop itself: what it keeps across polls, and how it ends.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

/// The guard every running-loop arm rests on, on the path a passing arm takes:
/// the scope ends normally and the loop is gone. The failing path is the arm
/// below, which is the one the guard exists for.
#[test]
fn a_loop_that_leaves_scope_is_stopped_and_not_left_polling() {
    let rig = Rig::new("guarded");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));

    let pid = {
        let controller = rig.spawn_loop();
        assert!(rig.wait_until(|p| p["seats"][0]["roster_state"] == "present"));
        // The control, inside the scope: the loop IS running here, so the
        // reading after the drop is the guard's and not a spawn that never
        // started.
        assert_eq!(
            fleet_controller::platform::process_alive(controller.pid()),
            Some(true),
            "the loop is running while its guard is in scope"
        );
        controller.pid()
    };

    assert_eq!(
        fleet_controller::platform::process_alive(pid),
        Some(false),
        "the loop outlived the guard that owns it"
    );
}

/// The path the guard exists for: an arm that FAILS between the spawn and its
/// own kill. The scope is left by an unwind rather than by a return, and a
/// `Drop` that ran only on the returning path would leave that loop polling
/// against a machine directory its rig has removed.
///
/// The panic is this arm's own instrument, raised inside `catch_unwind` so the
/// unwind stops here and the reading below is taken after it. The suite has no
/// other `catch_unwind`, and `fleet/Cargo.toml` sets no `panic`
/// strategy, so the default unwind is what makes the guard run at all.
#[test]
fn a_loop_whose_arm_panics_is_stopped_by_the_unwind_that_leaves_its_scope() {
    let rig = Rig::new("unwound");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));

    // Carried out of the closure because the closure leaves by panicking and
    // returns nothing.
    let escaped = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = std::sync::Arc::clone(&escaped);
    let fell = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let controller = rig.spawn_loop();
        assert!(rig.wait_until(|p| p["seats"][0]["roster_state"] == "present"));
        // The control, inside the scope and before the panic: the loop IS
        // running, so the reading afterwards is the unwind's and not a spawn
        // that never started.
        assert_eq!(
            fleet_controller::platform::process_alive(controller.pid()),
            Some(true),
            "the loop is running when the arm fails"
        );
        seen.store(controller.pid(), std::sync::atomic::Ordering::SeqCst);
        panic!("DELIBERATE: this arm fails on purpose, between the spawn and any kill of its own");
    }));

    assert!(
        fell.is_err(),
        "the probe really did fail; a probe that returned would leave nothing to unwind"
    );
    let pid = escaped.load(std::sync::atomic::Ordering::SeqCst);
    assert_ne!(
        pid, 0,
        "the failure came after the spawn, so there was a loop to leave behind"
    );
    assert_eq!(
        fleet_controller::platform::process_alive(pid),
        Some(false),
        "the unwind ran the guard, and the loop the failing arm left behind is gone"
    );
}

/// A live version that differs from the pin is a flag to re-measure, never a
/// failure: the poll publishes both, appends exactly one `substrate.moved`, and
/// exits 0.
#[test]
fn a_moved_substrate_is_one_event_and_not_a_failure() {
    let rig = Rig::new("moved");
    rig.set_version("9.9.10");
    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0), "a moved pin is not a failure");

    let published = rig.projection();
    assert_eq!(published["agent_version"], "9.9.10");
    assert_eq!(published["agent_version_expected"], "9.9.9");
    assert_eq!(rig.events_of("substrate.moved"), 1);

    let moved = rig
        .events()
        .into_iter()
        .find(|e| e["type"] == "substrate.moved")
        .unwrap();
    assert_eq!(moved["payload"]["observed"], "9.9.10");
    assert_eq!(moved["payload"]["expected"], "9.9.9");
    assert_eq!(moved["actor"], "controller");
}

/// One spread, many polls, one event — and a poll whose version read FAILED
/// knows nothing about the spread, so it must not close the announcement and
/// let the next healthy poll re-announce the same move.
#[test]
fn a_spread_announces_once_across_polls_and_a_failed_read_does_not_reopen_it() {
    let rig = Rig::new("dedup");
    rig.set_version("9.9.10");
    rig.driving(|polls| {
        polls.tick();
        assert_eq!(
            rig.projection()["agent_version"],
            "9.9.10",
            "the loop publishes the moved version"
        );

        // Three more polls over one unchanging spread.
        polls.tick();
        polls.tick();
        polls.tick();
        assert_eq!(
            rig.events_of("substrate.moved"),
            1,
            "one move is one event however many polls see it"
        );

        rig.set_version("FAIL");
        polls.tick();
        assert!(
            rig.projection()["agent_version"].is_null(),
            "the loop publishes a named absence when the version read fails"
        );
        rig.set_version("9.9.10");
        polls.tick();
        assert_eq!(
            rig.projection()["agent_version"],
            "9.9.10",
            "the version read recovers"
        );
        polls.tick();
        assert_eq!(
            rig.events_of("substrate.moved"),
            1,
            "a failed read is not a version that agrees with the pin"
        );

        // The control: a version that IS read and DOES agree closes it, so a
        // later move announces again — otherwise the arm above would pass on a
        // controller that never announces twice for any reason.
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

/// The seat list re-points `fleet_toml`, under a running controller: the new
/// seat lands AND the new policy file is what the loop reads from then on.
#[test]
fn a_seat_list_that_moves_the_policy_path_moves_the_policy_the_loop_reads() {
    let rig = Rig::new("repoint");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.driving(|polls| {
        polls.tick();
        assert_eq!(rig.projection()["seats"][0]["roster_state"], "present");
        assert_eq!(rig.projection()["fleet"]["poll_seconds"], 1);

        write(
            &rig.second_policy_path(),
            "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"7.7.7\"\n",
        );
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
             {{"id":"{SEAT_ID}","name":"Orla","worktrees":{{"demo":"{}"}}}},
             {{"id":"01a0d1f1-0aec-765f-9abe-5c21e8a04b17","worktrees":{{"demo":"{}"}}}}
           ]}}"#,
            rig.second_policy_path().display(),
            rig.worktree().display(),
            rig.root.join("wt").join("builder-2").display()
        ));
        polls.tick();

        let published = rig.projection();
        assert_eq!(
            published["seats"].as_array().map(Vec::len),
            Some(2),
            "a seat list re-read by a loop that has already polled lands the new seat"
        );
        assert_eq!(
            published["agent_version_expected"], "7.7.7",
            "and the policy the loop reads follows the path the seat list now names"
        );
        assert_eq!(
            published["fleet"]["path"],
            rig.second_policy_path().display().to_string(),
            "the published path is the file in force, not the one startup opened"
        );
        assert!(
            published.get("fleet_parse_error").is_none(),
            "the new file parses, so nothing says otherwise"
        );
    });
}

/// The poll interval is obeyed, asserted as a LOWER bound only: this box is
/// shared, so a poll may be late and may never be assumed prompt.
#[test]
fn the_poll_interval_is_the_policys_and_not_zero() {
    let rig = Rig::new("interval");
    rig.write_policy(
        "[controller]\npoll_seconds = 3\n\n[substrate.claude_code]\nversion = \"9.9.9\"\n",
    );
    let _controller = rig.spawn_loop();
    assert!(rig.wait_until(|p| p["generated_at"].is_string()));

    let first = rig.projection()["generated_at"]
        .as_str()
        .unwrap()
        .to_string();
    let seen_at = Instant::now();
    assert!(
        rig.wait_until(|p| p["generated_at"] != serde_json::json!(first)),
        "a second poll lands"
    );
    let elapsed = seen_at.elapsed();
    assert!(
        elapsed >= Duration::from_secs(2),
        "the next poll came {elapsed:?} after the last one was seen, under a 3s interval"
    );
}

/// The pin is read from the file every time its mtime moves, so editing it is
/// what makes the two agree again — not a restart.
#[test]
fn the_pin_is_reread_when_the_policy_file_moves() {
    let rig = Rig::new("pin");
    rig.set_version("9.9.10");
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(rig.projection()["agent_version_expected"], "9.9.9");

    rig.write_policy(
        "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"9.9.10\"\n",
    );
    assert_eq!(rig.observe().status.code(), Some(0));
    let published = rig.projection();
    assert_eq!(
        published["agent_version_expected"], "9.9.10",
        "the pin is re-read from the file, not cached from startup"
    );
    assert_eq!(published["agent_version"], "9.9.10");
}

/// The running loop's own arms, which `--once` cannot reach: policy that stops
/// parsing under a live controller, the flag it raises, the restore that clears
/// it, and the stop event a signal owes.
#[test]
fn a_running_loop_keeps_last_good_policy_and_stops_on_a_signal() {
    let rig = Rig::new("loop");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );

    let mut child = rig.spawn_loop();
    assert!(
        rig.wait_until(|p| p["seats"][0]["roster_state"] == "present"),
        "the loop publishes a first projection"
    );
    let good_mtime = rig.projection()["fleet"]["mtime"].clone();
    assert!(good_mtime.is_string());

    // No wait on the clock here. `write_policy` puts the file's stamp ahead of
    // the one this loop last read, so the gate in `run.rs` sees the write at
    // once. The moved stamp is asserted rather than assumed: it is what the
    // gate opens on, and this arm's whole subject sits behind it.
    let stamp_before = rig.policy_mtime();
    rig.write_policy("[controller\npoll_seconds = 1\n");
    assert_ne!(
        rig.policy_mtime(),
        stamp_before,
        "the broken policy's write left the stamp where it was, so the re-read \
         below would be a gate that never opened"
    );
    assert!(
        rig.wait_until(|p| p.get("fleet_parse_error").is_some()),
        "a policy that stops parsing raises the flag"
    );
    let on_last_good = rig.projection();
    assert_eq!(
        on_last_good["fleet"]["poll_seconds"], 1,
        "the policy in force is still the last one that parsed"
    );
    assert_eq!(
        on_last_good["fleet"]["mtime"], good_mtime,
        "the stamp is the policy IN FORCE, never the broken file's"
    );
    assert_eq!(
        on_last_good["seats"][0]["roster_state"], "present",
        "a policy failure disturbs no seat row"
    );
    assert_eq!(on_last_good["seats"][0]["context_tokens"], 18);

    rig.write_policy(
        "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"9.9.9\"\n",
    );
    assert!(
        rig.wait_until(|p| p.get("fleet_parse_error").is_none()),
        "restoring the file clears the flag on the following poll"
    );

    let status = child.signal_and_wait("-TERM");
    assert_eq!(status.code(), Some(0), "a signalled stop is a clean exit");
    assert_eq!(rig.events_of("controller.started"), 1);
    assert_eq!(
        rig.events_of("controller.stopped"),
        1,
        "the stop is one event, and the loop is what writes it"
    );
}

/// A row another writer pushed under the lock, between the poll that read the
/// table and the poll that writes it.
///
/// The loop reads the session table once at startup and carries the copy across
/// polls. A verb — a flight's spawn, `fleet seat` spawn, feed or retire — takes
/// the table's own lock and renames its own version over the file in between,
/// and a poll that renamed its carried copy back would take that row with it:
/// the seat is then polled against the fleet's daemon rather than its own
/// configuration directory, and reads absent while it works.
///
/// THE MERGE IS THE SUBJECT, so the second poll must be one that WRITES: the
/// sighting of the live row moves the table on both polls, which is what makes
/// the second write happen at all. An arm whose second poll wrote nothing would
/// read green against the carried-copy rename as squarely as against the merge.
#[test]
fn a_row_pushed_under_the_lock_between_two_polls_survives_the_second_poll() {
    let rig = Rig::new("lost-update");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    let table_path = rig.machine().join("sessions.json");

    // The row the loop is carrying: written BEFORE the loop starts, which is the
    // one read it takes of this file.
    let mut opening = fleet_controller::sessions::Table::default();
    opening.push(table_row(
        SEAT_ID,
        SEAT,
        &rig.worktree(),
        "dispatch-carried",
        1_000,
    ));
    fleet_controller::sessions::write(&table_path, &opening).expect("the opening table is written");

    rig.driving(|ticks| {
        ticks.tick();
        let after_one = rig.sessions();
        assert_eq!(
            after_one["sessions"].as_array().map(Vec::len),
            Some(1),
            "the first poll leaves the one row it started with: {after_one}"
        );
        assert_eq!(
            after_one["sessions"][0]["session_id"], "a-session",
            "the first poll SIGHTED that row, which is what makes the second poll a writing one"
        );

        // The other writer, taking the same lock every writer of this table
        // takes and renaming its own version over the file.
        {
            let (held, theirs, why) = fleet_controller::sessions::read_under_lock(&table_path)
                .expect("the other writer takes the lock");
            let mut theirs =
                theirs.unwrap_or_else(|| panic!("the other writer's read of the table: {why:?}"));
            theirs.push(table_row(
                "01a0d1f1-0aec-765f-9abe-00001b7e4c09",
                "agent-1b7e4c09",
                &rig.worktree(),
                "dispatch-under-the-lock",
                2_000,
            ));
            fleet_controller::sessions::write_under_lock(&held, &table_path, &theirs)
                .expect("the other writer's table is written");
        }
        let between = rig.sessions();
        assert_eq!(
            between["sessions"].as_array().map(Vec::len),
            Some(2),
            "the control: the other writer's row IS on the file when the second poll starts: \
             {between}"
        );

        ticks.tick();
    });

    let after_two = rig.sessions();
    let rows = after_two["sessions"]
        .as_array()
        .expect("the table carries an array of rows");
    let ids: Vec<&str> = rows
        .iter()
        .map(|row| row["dispatch_id"].as_str().unwrap_or("(none)"))
        .collect();
    assert_eq!(
        ids,
        vec!["dispatch-carried", "dispatch-under-the-lock"],
        "the second poll put back BOTH rows — its own, moved, and the one it never read"
    );
    assert_eq!(
        rows[0]["session_id"], "a-session",
        "and the loop's own move of its own row stands"
    );
    assert_eq!(
        rows[1]["config_dir"],
        rig.worktree().join("config").display().to_string(),
        "the pushed row keeps the configuration directory the poll reads a seat's own listing \
         under"
    );
}

/// One row, shaped the way a spawn's own `open_row` shapes it: keyed by the
/// seat's id, named by the session's name, unsighted, and carrying the
/// configuration directory the dispatch chose.
fn table_row(
    seat: &str,
    name: &str,
    worktree: &Path,
    dispatch_id: &str,
    dispatched_at: u64,
) -> fleet_controller::sessions::SessionRow {
    fleet_controller::sessions::SessionRow {
        seat: seat.to_string(),
        project: "demo".to_string(),
        worktree: worktree.display().to_string(),
        name: name.to_string(),
        model: "a-model".to_string(),
        posture: "auto".to_string(),
        first_turn: "/wake".to_string(),
        transient: false,
        config_dir: Some(worktree.join("config").display().to_string()),
        item: None,
        dispatch_id: dispatch_id.to_string(),
        dispatched_at,
        session_id: None,
        short_id: None,
        first_seen_at: None,
        last_seen_at: None,
        adopted: None,
    }
}
