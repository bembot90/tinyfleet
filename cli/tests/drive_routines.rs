//! The routines on the controller's own tick.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

/// The routines on the controller's own tick (PRD R22–R24, Q2).
///
/// Every arm here drives the BUILT binary against the stub agent, with the
/// routines' clock on a file this rig steps: an arm about a schedule that ran on
/// this box's own hour would measure the hour and not the schedule.
mod routines {
    use super::*;

    /// A minute boundary, so a step of sixty seconds lands on the next whole
    /// minute in every zone whose offset is whole minutes.
    const BASE: u64 = 1_788_600_000;

    /// The `bd` this box carries, resolved on THIS process's own search path.
    /// `None` is a box with no work-graph binary, which the one arm that needs
    /// it says out loud rather than passing quietly.
    fn bd_on_path() -> Option<PathBuf> {
        let path = std::env::var("PATH").ok()?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("bd"))
            .find(|candidate| candidate.is_file())
    }

    fn a_cooldown_nudge(seat: &str, text: &str, authority: &str) -> String {
        format!(
            "[order]\ndescription = \"ring a seat\"\ntrigger = \"cooldown\"\ninterval = \"1m\"\n\
             [action.nudge]\nseat = \"{seat}\"\ntext = \"{text}\"\nauthority = \"{authority}\"\n"
        )
    }

    /// (a) The state is written BEFORE the action, and the command's own
    /// environment carries the fleet and this controller's own directory.
    ///
    /// The state file is read back BY THE COMMAND THE ORDER RUNS: an arm that
    /// read it afterwards could not tell a write before the action from one
    /// after it, which is the whole of the ordering rule — a duty that happened
    /// and was not recorded fires again forever.
    #[test]
    fn an_exec_routine_records_its_firing_before_it_runs_and_completes_with_ran() {
        let rig = Rig::new("routines-exec");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let state_witness = rig.root.join("state-as-the-command-saw-it");
        let path_witness = rig.root.join("path-as-the-command-saw-it");
        rig.write_routine(
            "beat",
            &format!(
                "[order]\ndescription = \"leave a mark\"\ntrigger = \"cooldown\"\n\
                 interval = \"1h\"\n[action.exec]\ncommand = 'cat \
                 $FLEET_DIR/orders/state.json > {}; printf %s \"$PATH\" > {}'\n",
                state_witness.display(),
                path_witness.display()
            ),
        );

        rig.set_clock(BASE);
        // Through the BUILT BINARY: the search path this arm reads back leads
        // with the running executable's own directory, and only the binary's
        // holds a `fleet`.
        assert_eq!(rig.observe_out_of_process().status.code(), Some(0));

        let seen = std::fs::read_to_string(&state_witness)
            .expect("the command the routine ran read the state file back");
        let witness: serde_json::Value =
            serde_json::from_str(&seen).expect("the state the command saw parses as JSON");
        assert_eq!(
            witness["orders"]["beat"]["last_fired"],
            fleet_controller::clock::stamp_secs(BASE),
            "the firing was recorded before the command ran, and it is this firing's own instant: {seen}"
        );

        let events = rig.routine_events();
        assert_eq!(events.len(), 2, "{events:#?}");
        assert_eq!(events[0]["type"], "routine.fired");
        assert_eq!(events[0]["payload"]["order"], "beat");
        assert_eq!(events[0]["payload"]["source"], "fleet");
        assert_eq!(events[0]["payload"]["trigger"], "cooldown");
        assert_eq!(events[1]["type"], "routine.completed");
        assert_eq!(events[1]["payload"]["outcome"], "ran");
        assert!(
            events[1]["payload"]["duration_ms"].is_u64(),
            "{:#?}",
            events[1]
        );
        // The tick's own firing carries no `by`: that half says a person asked
        // for it by hand.
        assert!(events[0]["payload"].get("by").is_none());

        let log = PathBuf::from(
            events[1]["payload"]["log"]
                .as_str()
                .expect("the completed event names the log"),
        );
        assert!(log.is_file(), "the command's log is at {}", log.display());

        // The controller's own directory is FIRST on the child's search path,
        // so a routine that calls a fleet verb by name reaches this binary.
        let own = PathBuf::from(env!("CARGO_BIN_EXE_fleet"))
            .parent()
            .expect("the built binary sits in a directory")
            .to_path_buf();
        let path = std::fs::read_to_string(&path_witness).expect("the command recorded its PATH");
        assert!(
            path.starts_with(&format!("{}:", own.display())),
            "the controller's own directory leads the child PATH: {path}"
        );
        // And the constructed path is behind it, never this process's own.
        assert!(path.contains(&child_path(&rig.home())), "{path}");
    }

    /// (b) A trigger that cannot answer is a third outcome, and the streak it
    /// carries is a number the projection publishes.
    ///
    /// Three polls of a STEPPED clock, then a fourth tick with the clock
    /// standing still: the fourth publishes the state the third left and asks
    /// the routine nothing, so the count of events and the streak in the document
    /// are the same three.
    #[test]
    fn a_failing_streak_is_read_by_the_projection() {
        let rig = Rig::new("routines-streak");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        rig.write_routine(
            "blind",
            "[order]\ndescription = \"the instrument is out\"\ntrigger = \"condition\"\n\
             check = 'exit 3'\ncheck_unknown_exit = [3]\npoll = \"1s\"\n\
             [action.exec]\ncommand = 'true'\n",
        );

        for poll in 0..3 {
            rig.set_clock(BASE + poll * 2);
            assert_eq!(rig.observe().status.code(), Some(0));
        }
        let events = rig.routine_events();
        assert_eq!(
            events.len(),
            3,
            "one per poll and nothing else: {events:#?}"
        );
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event["type"], "routine.could_not_tell");
            assert_eq!(event["payload"]["order"], "blind");
            assert_eq!(event["payload"]["streak"], index as u64 + 1);
            assert!(
                event["payload"]["reason"]
                    .as_str()
                    .is_some_and(|why| why.contains("maps to could-not-tell")),
                "{event:#?}"
            );
        }

        // The tick that publishes what those three left, and asks nothing: the
        // clock has not moved, so the routine is not due an evaluation.
        assert_eq!(rig.observe().status.code(), Some(0));
        assert_eq!(rig.routine_events().len(), 3, "a fourth poll asked nothing");
        let row = rig
            .routine_row("blind")
            .expect("the projection carries the row");
        assert_eq!(row["last_outcome"], "could-not-tell");
        assert_eq!(row["failing_streak"], 3);
        assert_eq!(row["source"], "fleet");
        assert_eq!(row["trigger"], "condition");
        assert!(row["last_fired"].is_null(), "nothing ever fired: {row}");
    }

    /// (c) A ring that lands: the argv the stub received is the policy's model
    /// and a prompt carrying the routine's own sentence.
    #[test]
    fn a_nudge_routine_rings_a_live_seat_with_its_text_and_its_authority() {
        let rig = Rig::new("routines-nudge");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        rig.write_routine(
            "ring",
            &a_cooldown_nudge("builder-1", "look at the board", "the operator"),
        );
        rig.set_clock(BASE);
        assert_eq!(rig.observe().status.code(), Some(0));

        let argv =
            std::fs::read_to_string(rig.nudge_argv_path()).expect("the ring reached the stub");
        assert!(argv.lines().any(|line| line == "-p"), "{argv}");
        assert!(
            argv.lines().any(|line| line == "claude-haiku-4-5-20251001"),
            "the ring runs on the policy's nudge model: {argv}"
        );
        assert!(argv.contains("look at the board"), "{argv}");
        assert!(argv.contains("authority: the operator"), "{argv}");

        let events = rig.routine_events();
        assert_eq!(events.len(), 2, "{events:#?}");
        assert_eq!(events[1]["type"], "routine.completed");
        assert_eq!(events[1]["payload"]["outcome"], "delivered");
    }

    /// The other two halves of the ring, against a scratch work-graph store: a
    /// ring that finds nobody leaves the duty on the graph, and one with
    /// nowhere to leave it is an absence somebody has to read.
    #[test]
    fn a_ring_that_finds_nobody_files_an_item_once_and_dedupes_the_next_firing() {
        let Some(bd) = bd_on_path() else {
            panic!("this box carries no `bd`, so the item action has no store to file into");
        };
        let rig = Rig::new("routines-item");
        // The seat is on the list and absent from the roster, which is the
        // case the fallback exists for.
        rig.write_roster("[]");
        rig.write_routine(
            "leave-it",
            &format!(
                "{}[action.item]\ntitle = \"the ring found nobody\"\nwhen = \"absent\"\n\
                 dedupe = \"open\"\ntype = \"task\"\npriority = 3\nlabels = [\"lane\"]\n",
                a_cooldown_nudge("builder-1", "look at the board", "the operator")
            ),
        );
        rig.write_routine(
            "no-fallback",
            &a_cooldown_nudge("builder-1", "nowhere to leave it", "the operator"),
        );

        common::take_a_board_with(&bd, &rig.root, "drive");

        rig.set_clock(BASE);
        assert_eq!(rig.observe().status.code(), Some(0));

        let filed: Vec<serde_json::Value> = rig
            .routine_events()
            .into_iter()
            .filter(|e| e["payload"]["order"] == "leave-it")
            .collect();
        assert_eq!(filed.len(), 2, "{filed:#?}");
        assert_eq!(filed[1]["type"], "routine.completed");
        assert_eq!(filed[1]["payload"]["outcome"], "filed");
        assert_eq!(filed[1]["payload"]["fallback_from"], "nudge");
        let item = filed[1]["payload"]["item"]
            .as_str()
            .unwrap_or_else(|| panic!("the completed event names the item: {filed:#?}"))
            .to_string();

        // The item carries the routine's own label, which is what makes "did this
        // routine already file one" answerable from outside this process.
        let listed = Command::new(&bd)
            .args([
                "-C",
                &rig.root.display().to_string(),
                "list",
                "--label",
                "routine:leave-it",
                "--status",
                "open",
                "--json",
            ])
            .output()
            .expect("bd runs");
        let rows: serde_json::Value =
            serde_json::from_slice(&listed.stdout).expect("the listing is JSON");
        let ids: Vec<&str> = rows
            .as_array()
            .expect("a list")
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect();
        assert_eq!(ids, vec![item.as_str()], "one item, carrying the label");

        // The ring with nowhere to leave it: absent, and nothing filed.
        let bare: Vec<serde_json::Value> = rig
            .routine_events()
            .into_iter()
            .filter(|e| e["payload"]["order"] == "no-fallback")
            .collect();
        assert_eq!(bare.len(), 2, "{bare:#?}");
        assert_eq!(bare[1]["type"], "routine.failed");
        assert_eq!(bare[1]["payload"]["outcome"], "absent");
        assert_eq!(bare[1]["payload"]["streak"], 1);

        // The next firing, two minutes of the stepped clock later: the open item
        // is already there, so nothing is filed and the event names it.
        rig.set_clock(BASE + 120);
        assert_eq!(rig.observe().status.code(), Some(0));
        let again: Vec<serde_json::Value> = rig
            .routine_events()
            .into_iter()
            .filter(|e| e["payload"]["order"] == "leave-it")
            .collect();
        assert_eq!(again.len(), 4, "{again:#?}");
        assert_eq!(again[3]["type"], "routine.completed");
        assert_eq!(again[3]["payload"]["outcome"], "deduped");
        assert!(
            again[3]["payload"]["existing"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == &serde_json::json!(item))),
            "the dedupe names the item it found: {:#?}",
            again[3]
        );
    }

    /// (d) A not-due routine writes nothing but the fact that it was asked.
    #[test]
    fn a_not_due_cron_routine_leaves_the_stream_alone_across_ten_ticks() {
        let rig = Rig::new("routines-quiet");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        // Midnight on the first of January, from a September instant: not this
        // minute and not any minute these ten ticks reach.
        rig.write_routine(
            "new-year",
            "[order]\ndescription = \"a duty for one minute a year\"\ntrigger = \"cron\"\n\
             schedule = \"0 0 1 1 *\"\n[action.exec]\ncommand = 'true'\n",
        );

        for tick in 0..10 {
            rig.set_clock(BASE + tick * 60);
            assert_eq!(rig.observe().status.code(), Some(0));
        }

        assert!(
            rig.routine_events().is_empty(),
            "a not-due evaluation writes nothing: {:#?}",
            rig.routine_events()
        );
        let state = rig
            .routine_state("new-year")
            .expect("the routine was asked, and that is recorded");
        assert_eq!(
            state["last_evaluated"],
            serde_json::json!(fleet_controller::clock::stamp_secs(BASE + 9 * 60)),
            "the last time it was asked, and nothing else: {state}"
        );
        assert!(state.get("last_fired").is_none(), "{state}");
        assert!(state.get("last_outcome").is_none(), "{state}");
        assert_eq!(state["failing_streak"], 0);

        // And the projection still carries the row, with a next due a reader
        // can act on.
        let row = rig.routine_row("new-year").expect("the row is published");
        assert!(row["last_outcome"].is_null());
        assert!(
            row["next_due"]
                .as_str()
                .is_some_and(|due| due.contains("-01-01T")),
            "{row}"
        );
    }

    /// A lock a `fleet routine run` holds keeps the tick's hands off that routine,
    /// and says so once rather than firing beside it.
    #[test]
    fn a_routine_a_run_holds_is_skipped_by_the_tick() {
        let rig = Rig::new("routines-lock");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        rig.write_routine(
            "beat",
            "[order]\ndescription = \"leave a mark\"\ntrigger = \"cooldown\"\n\
             interval = \"1h\"\n[action.exec]\ncommand = 'true'\n",
        );
        // This process is live by definition, which is what a held lock names.
        write(
            &rig.machine().join("orders").join("beat.lock"),
            &format!("{}\n", std::process::id()),
        );

        rig.set_clock(BASE);
        let polled = rig.observe();
        assert_eq!(polled.status.code(), Some(0));
        assert!(
            rig.routine_events().is_empty(),
            "a held routine fires nothing: {:#?}",
            rig.routine_events()
        );
        assert_eq!(
            String::from_utf8_lossy(&polled.stderr)
                .lines()
                .filter(|l| l.contains("is held by a run at pid"))
                .count(),
            1,
            "the skip is one line: {}",
            String::from_utf8_lossy(&polled.stderr)
        );

        // The control: with the lock gone the same tick fires it, so the
        // silence above is the lock's and not the routine's.
        std::fs::remove_file(rig.machine().join("orders").join("beat.lock")).unwrap();
        assert_eq!(rig.observe().status.code(), Some(0));
        assert_eq!(rig.routine_events().len(), 2);
    }

    /// A file dropped into a routines directory is live on the next evaluation,
    /// and one taken out of it stops being asked — the loader is re-run per
    /// tick and holds no registry of its own.
    #[test]
    fn a_routine_dropped_in_is_live_on_the_next_tick_and_one_taken_out_is_gone() {
        let rig = Rig::new("routines-live");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        rig.set_clock(BASE);
        assert_eq!(rig.observe().status.code(), Some(0));
        assert!(rig.routine_row("beat").is_none(), "no routines yet");

        rig.write_routine(
            "beat",
            "[order]\ndescription = \"leave a mark\"\ntrigger = \"cooldown\"\n\
             interval = \"1h\"\n[action.exec]\ncommand = 'true'\n",
        );
        rig.set_clock(BASE + 60);
        assert_eq!(rig.observe().status.code(), Some(0));
        assert_eq!(rig.routine_events().len(), 2, "no restart was needed");

        std::fs::remove_file(rig.routines_dir().join("beat.toml")).unwrap();
        rig.set_clock(BASE + 7_200);
        assert_eq!(rig.observe().status.code(), Some(0));
        assert_eq!(
            rig.routine_events().len(),
            2,
            "a file that is gone is a routine nobody asks"
        );
        assert!(rig.routine_row("beat").is_none());
    }
}
