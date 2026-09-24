//! The effect half of the loop: the seat verbs, and the directory they run under.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

// ---------------------------------------------------------------- the effects

/// The effect half of the loop, end to end against the stub: what the seat verbs
/// write, what the tick decides, what the child received, and what the two
/// documents say afterwards.
///
/// Every reading of an effect is taken from what the STUB RECORDED — its argv,
/// its cwd, its `PATH`, and an append-only log of the calls in order — and never
/// from the controller's own stderr. A log line says what the loop meant to do;
/// only the child says what it did.
mod effects {
    use super::*;

    /// The threshold an arm about the nudge sets, low enough that a one-line
    /// transcript crosses it. The figure itself is pinned in the controller's
    /// own policy arms; here it is only a line the reading has to be over.
    const LOW_THRESHOLD: u64 = 100;

    fn policy_with(extra: &str) -> String {
        format!(
            "[controller]\npoll_seconds = 1\n{extra}\n[substrate.claude_code]\nversion = \"9.9.9\"\n"
        )
    }

    /// A transcript carrying one main-chain reading.
    fn transcript_of(tokens: u64) -> String {
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"usage\":{{\"input_tokens\":{tokens}}}}}}}\n"
        )
    }

    fn seat_row(rig: &Rig) -> serde_json::Value {
        rig.projection()["seats"][0].clone()
    }

    /// AC1 — the four verbs, and two of the three halves a rest can fail on.
    ///
    /// The projection every refusal reads is a REAL one, published by a poll of
    /// this same rig: a hand-written document would let the freshness check pass
    /// over a shape no controller produces.
    #[test]
    fn the_seat_verbs_write_their_events_and_a_rest_refuses_naming_the_half_that_failed() {
        let rig = Rig::new("seat-verbs");

        // 5 — no collector. Nothing has published, so nothing would read it.
        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("no collector is consuming"),
            "{}",
            stderr(&out)
        );

        // 4 — a seat the projection carries no row for. The poll below publishes
        // one seat, and a second row added to the list after it is a seat the
        // collector has not published.
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["roster_state"], "present");
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
                 {{"id":"{SEAT_ID}","name":"Orla","worktrees":{{"demo":"{}"}}}},
                 {{"id":"01a0d1f1-0aec-765f-9abe-5c21e8a04b17","name":"Pell",
                   "worktrees":{{"demo":"{}"}}}}
               ]}}"#,
            rig.policy_path().display(),
            rig.worktree().display(),
            rig.root.join("wt").join("pell").display()
        ));

        let out = rig
            .binary()
            .args(["event", "rest", "Pell", "--reason", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("pell-e8a04b17 has no live session"),
            "the seat is named by the machine name its argument resolved to: {}",
            stderr(&out)
        );

        // 1 — a seat argument that names no seat at all, refused by the resolver
        // before any stream is read, listing the seats it could have meant.
        let out = rig
            .binary()
            .args(["event", "rest", "builder-2", "--reason", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
        assert!(
            stderr(&out).contains(
                "fleet event rest: builder-2 names no seat — the seats are orla-93b9739a"
            ),
            "{}",
            stderr(&out)
        );

        // 0 — a live named row. The event lands with its reason, and the exit
        // status is taken from the read-back rather than from the append.
        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "a nap"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let last = rig.events().pop().expect("the stream carries the rest");
        assert_eq!(last["type"], "seat.resting");
        assert_eq!(last["actor"], SEAT_ID);
        assert_eq!(last["payload"]["reason"], "a nap");

        // The other three verbs, each exiting 0 and each advancing the sequence
        // by exactly one. The rc of every command is read directly.
        let mut previous = last["seq"].as_u64().expect("the line carries a seq");
        for (verb, kind) in [
            ("woke", "seat.woke"),
            ("handed-off", "seat.handed_off"),
            ("exited", "seat.exited"),
        ] {
            let out = rig
                .binary()
                .args(["event", verb, SEAT])
                .output()
                .expect("the built binary runs");
            assert_eq!(out.status.code(), Some(0), "{verb}: {}", stderr(&out));
            let line = rig.events().pop().expect("the stream carries the record");
            assert_eq!(line["type"], kind);
            assert_eq!(line["actor"], SEAT_ID);
            let seq = line["seq"].as_u64().expect("the line carries a seq");
            assert_eq!(
                seq,
                previous + 1,
                "the sequence advances by one, never by none"
            );
            previous = seq;
        }
    }

    /// `--reason` belongs to `rest` and to no other seat writer: `woke`,
    /// `handed-off` and `exited` refuse it.
    ///
    /// The refusal is clap's, at argv, so it is reached before anything the
    /// controller would read — which is why the stream's length is the reading
    /// that says no row was written, taken before and after the three.
    #[test]
    fn reason_is_a_usage_error_on_every_seat_writer_but_rest() {
        let rig = Rig::new("reason-narrows");
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["roster_state"], "present");

        let before = rig.events().len();
        assert!(before > 0, "the poll wrote a stream to count against");
        let mut rows = before;

        for verb in ["woke", "handed-off", "exited"] {
            let out = rig
                .binary()
                .args(["event", verb, SEAT, "--reason", "x"])
                .output()
                .expect("the built binary runs");
            assert_eq!(out.status.code(), Some(2), "{verb}: {}", stderr(&out));
            assert!(
                stderr(&out).contains("unexpected argument '--reason'"),
                "{verb} answers with clap's unexpected-argument line: {}",
                stderr(&out)
            );
            assert_eq!(
                rig.events().len(),
                rows,
                "{verb} refused at argv writes no row"
            );

            // The present half of the pair: the same verb and the same seat
            // without the flag reaches the controller and lands its record, so
            // the 2 above is the flag and not the verb.
            let out = rig
                .binary()
                .args(["event", verb, SEAT])
                .output()
                .expect("the built binary runs");
            assert_eq!(out.status.code(), Some(0), "{verb}: {}", stderr(&out));
            rows += 1;
            assert_eq!(
                rig.events().len(),
                rows,
                "{verb} unflagged writes exactly one row"
            );
        }
        assert_eq!(rows, before + 3, "three verbs, three rows");

        // The control: the one writer the flag belongs to still carries it into
        // the payload, read back from the stream.
        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "a nap"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let last = rig.events().pop().expect("the stream carries the rest");
        assert_eq!(last["type"], "seat.resting");
        assert_eq!(last["actor"], SEAT_ID);
        assert_eq!(last["payload"]["reason"], "a nap");
    }

    /// The `seat` noun refuses the four writers and names the `event` one.
    ///
    /// The rig publishes nothing, so `event rest` on it exits 5 — no collector.
    /// That 5 is the control the 2 needs: it is the same rig and the same seat,
    /// so a 2 under `seat` is the spelling and not the state, and it is reached
    /// at argv, before anything the controller would read.
    #[test]
    fn the_seat_noun_refuses_the_four_writers_and_names_the_event_one() {
        let rig = Rig::new("seat-noun-refuses");

        for (verb, rewrite) in [
            ("rest", "fleet event rest"),
            ("woke", "fleet event woke"),
            ("handed-off", "fleet event handed-off"),
            ("exited", "fleet event exited"),
        ] {
            let out = rig
                .binary()
                .args(["seat", verb, SEAT])
                .output()
                .expect("the built binary runs");
            assert_eq!(out.status.code(), Some(2), "{verb}: {}", stderr(&out));
            assert!(
                stderr(&out).contains(rewrite),
                "the refusal carries the rewrite ({rewrite}): {}",
                stderr(&out)
            );
        }

        // The control: the same seat under the noun that holds it reaches the
        // controller, and answers with the state rather than with usage.
        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));

        // And the rewrite is keyed on the four writers, not on the noun: a verb
        // the `seat` noun does not hold gets clap's own unrecognised line, which
        // names no `event` spelling. `ring` is the specimen because the naming
        // ruling gave this act the word `nudge` (brain/naming.md), so it is a
        // spelling this noun will not come to hold — which keeps the arm about
        // the four writers rather than about a typo.
        let out = rig
            .binary()
            .args(["seat", "ring", SEAT])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("unrecognized subcommand 'ring'")
                && !stderr(&out).contains("fleet event"),
            "{}",
            stderr(&out)
        );
    }

    /// The rewrite survives the options the verb is actually typed with.
    ///
    /// A rest always carries a reason, so `fleet seat rest s1 --reason x` is the
    /// one invocation a ritual under the old spelling puts on the wire — and an
    /// argument parser that claimed the flag for itself would answer it with a
    /// sentence about `--reason` and never reach the rewrite. What holds the
    /// flag out of clap's hands is each rewrite arm taking its words as a
    /// trailing var-arg with the help flag disabled on it; drop either and this
    /// arm reds — on clap's `unexpected argument '--reason' found` without the
    /// var-arg, and on control two below without the disabled help flag, where
    /// `seat woke --help` would print a hidden verb's page and exit 0. The three
    /// controls fence the cost: the FAMILY's help flag still answers as help, a
    /// verb carrying it still gets the rewrite, and a verb this noun does not
    /// hold still names no `event` spelling.
    #[test]
    fn the_rewrite_survives_the_options_the_verb_carries() {
        let rig = Rig::new("seat-noun-with-options");

        for (args, rewrite) in [
            (
                vec!["seat", "rest", SEAT, "--reason", "a nap"],
                "fleet event rest",
            ),
            (
                vec!["seat", "woke", SEAT, "--reason", "x"],
                "fleet event woke",
            ),
            (
                vec!["seat", "handed-off", SEAT, "--reason", "x"],
                "fleet event handed-off",
            ),
            (
                vec!["seat", "exited", SEAT, "--reason", "x"],
                "fleet event exited",
            ),
        ] {
            let out = rig
                .binary()
                .args(&args)
                .output()
                .expect("the built binary runs");
            assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
            assert!(
                stderr(&out).contains(rewrite),
                "{args:?} carries the rewrite ({rewrite}): {}",
                stderr(&out)
            );
        }

        // Control one: the help flag is still help, on stdout and exiting 0.
        // clap answers it before this family's words are matched at all, so the
        // page below is clap's own and no hand branch stands behind it.
        let out = rig
            .binary()
            .args(["seat", "--help"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let printed = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            printed.contains("Usage: fleet seat") && printed.contains("what is done to a seat"),
            "the family's own page: {printed}"
        );

        // Control two: a verb carrying the help flag is still the rewrite, so
        // the branch above is keyed on the FIRST word and not on the flag.
        let out = rig
            .binary()
            .args(["seat", "woke", "--help"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("fleet event woke"),
            "{}",
            stderr(&out)
        );

        // Control three, the one the arm above this holds unchanged: an option
        // beside a verb the `seat` noun does not hold is still clap's own
        // unrecognised line, and still names no `event` spelling.
        let out = rig
            .binary()
            .args(["seat", "ring", SEAT, "--text", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("unrecognized subcommand 'ring'")
                && !stderr(&out).contains("fleet event"),
            "{}",
            stderr(&out)
        );
    }

    /// AC1's third refusal, and AC3(c): only named seats rest.
    ///
    /// A transient row is refused at the CLI with the verb it wants named, and
    /// the hand-written event a caller could put in the file around that refusal
    /// is dropped by the consumer with a line — so the refusal is not the only
    /// thing standing between a spawned seat and a stop.
    #[test]
    fn a_transient_rows_rest_is_refused_and_a_hand_written_one_is_dropped() {
        let rig = Rig::new("seat-transient");
        rig.write_config(&rig.one_seat_config_carrying_model_and_transient(rig.policy_path()));
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["roster_state"], "present");

        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(6), "{}", stderr(&out));
        assert!(
            stderr(&out).contains(&format!("fleet seat retire {SEAT}")),
            "the refusal names the verb this kind of seat wants: {}",
            stderr(&out)
        );

        // The control on the refusal: the same seat, named rather than
        // transient, is accepted — so the 6 above is the row's kind and not this
        // rig's state.
        let control = Rig::new("seat-transient-control");
        control.write_roster(&live_row(&control.worktree(), "ab12"));
        assert_eq!(control.observe().status.code(), Some(0));
        let out = control
            .binary()
            .args(["event", "rest", SEAT, "--reason", "x"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        // Around the refusal: the event written into the file by hand, by the
        // seat's id as every seat line is. The consumer drops it with one line
        // and issues nothing.
        append_event(
            &rig,
            "seat.resting",
            SEAT_ID,
            serde_json::json!({"reason": "x"}),
        );
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("which is a transient row"),
            "the drop is stated: {}",
            stderr(&out)
        );
        assert!(
            rig.calls().is_empty(),
            "and nothing was issued against it: {:?}",
            rig.calls()
        );
    }

    /// AC3(a) and AC7 — an absent seat is started once, with the argv, the cwd
    /// and the `PATH` the child received asserted, and the arrival window holds
    /// the second poll.
    #[test]
    fn an_absent_seat_is_started_once_and_the_arrival_window_holds_the_next_poll() {
        let rig = Rig::new("effect-spawn");
        rig.write_roster("[]");

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["roster_state"], "absent");
        assert_eq!(seat_row(&rig)["decision"], "spawn-woken");
        assert_eq!(seat_row(&rig)["outcome"], "spawned");

        let argv = rig.start_argv();
        assert_eq!(argv.first().map(String::as_str), Some("--bg"));
        assert_eq!(flag_value(&argv, "--name"), SEAT);
        assert_eq!(flag_value(&argv, "--model"), "claude-opus-5");
        assert_eq!(flag_value(&argv, "--permission-mode"), "auto");
        assert_eq!(argv.last(), Some(&format!("/wake {SEAT}")));
        assert_eq!(
            rig.start_cwd(),
            std::fs::canonicalize(rig.worktree()).expect("the worktree resolves")
        );
        assert_eq!(
            rig.start_path(),
            child_path(&rig.home()),
            "the child got the CONSTRUCTED path and not this test process's"
        );
        assert_ne!(
            rig.start_path(),
            std::env::var("PATH").unwrap_or_default(),
            "and the two differ, so the line above is a reading"
        );
        assert_eq!(rig.events_of("session.spawned"), 1);

        // AC7 — the row the dispatch opened, before anything has sighted it:
        // keyed by the seat's id, and recording the name the session was
        // started under.
        let table = rig.sessions();
        assert_eq!(table["schema"], 2);
        let row = &table["sessions"][0];
        assert_eq!(row["seat"], SEAT_ID);
        assert_eq!(row["name"], SEAT);
        assert_eq!(row["model"], "claude-opus-5");
        assert_eq!(row["posture"], "auto");
        assert_eq!(row["first_turn"], format!("/wake {SEAT}"));
        let spawned = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "session.spawned")
            .expect("the start wrote its event");
        assert_eq!(
            row["dispatch_id"], spawned["id"],
            "the row is keyed on the event that opened it"
        );
        assert!(
            row.get("short_id").is_none() && row.get("session_id").is_none(),
            "a row nothing has sighted carries neither: {row}"
        );

        // The window: a second poll with the row still absent starts nothing.
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "leave-alone");
        assert_eq!(rig.events_of("session.spawned"), 1);
        assert_eq!(
            rig.calls()
                .iter()
                .filter(|c| c.starts_with("start "))
                .count(),
            1,
            "one start, not one per poll: {:?}",
            rig.calls()
        );

        // And a sighting fills the row it opened, which is what the window was
        // waiting for.
        rig.write_roster(&live_row(&rig.worktree(), "the-successor"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let row = &rig.sessions()["sessions"][0];
        assert_eq!(row["session_id"], "the-successor");
        assert_eq!(row["short_id"], "the-successor");
        assert!(row["first_seen_at"].as_u64().is_some());
    }

    /// A seat with no name of its own is `agent-<short>` in every name a start
    /// derives — its session's `--name`, the first turn's argument and its start
    /// log — while the line the start writes carries the seat's full id.
    #[test]
    fn an_unnamed_seats_start_is_named_agent_short_and_its_line_carries_the_id() {
        let rig = Rig::new("effect-unnamed");
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
                 {{"id":"{SEAT_ID}","worktrees":{{"demo":"{}"}}}}
               ]}}"#,
            rig.policy_path().display(),
            rig.worktree().display()
        ));
        rig.write_roster("[]");

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["outcome"], "spawned");
        assert_eq!(
            seat_row(&rig)["seat_dir"],
            SEAT_ID,
            "the row is the seat's id"
        );

        let named = "agent-93b9739a";
        let argv = rig.start_argv();
        assert_eq!(flag_value(&argv, "--name"), named);
        assert_eq!(argv.last(), Some(&format!("/wake {named}")));

        let logs: Vec<String> = std::fs::read_dir(rig.machine().join("starts"))
            .expect("the starts directory is there")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(logs.len(), 1, "one start, one log: {logs:?}");
        assert!(
            logs[0].starts_with(&format!("{named}-")) && logs[0].ends_with(".log"),
            "the start log is named by the session: {logs:?}"
        );

        let spawned = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "session.spawned")
            .expect("the start wrote its event");
        assert_eq!(spawned["actor"], SEAT_ID, "the line's actor is the full id");
        assert_eq!(spawned["payload"]["name"], named);
    }

    /// The plugin root end to end through the BUILT binary: the policy names a
    /// relative directory, the start the loop issued carries it resolved against
    /// the policy file's own directory, and the projection publishes the same
    /// path for a reader of the document.
    ///
    /// The resolution is what only this suite can measure: the controller runs
    /// with a working directory nobody configured, so a root resolved against
    /// the process's cwd and one resolved against the file's differ here.
    #[test]
    fn a_start_carries_the_plugin_root_and_the_projection_reports_it() {
        let rig = Rig::new("effect-plugin-root");
        rig.write_policy(&policy_with("plugin_dir = \"the-overlay\""));
        rig.write_roster("[]");

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["outcome"], "spawned");

        let beside = rig
            .policy_path()
            .parent()
            .expect("the policy file sits in a directory")
            .join("the-overlay")
            .display()
            .to_string();
        let argv = rig.start_argv();
        assert_eq!(flag_value(&argv, "--plugin-dir"), beside);
        let at = argv
            .iter()
            .position(|word| word == "--plugin-dir")
            .expect("the flag is in the argv");
        assert_eq!(
            argv.get(at + 2),
            Some(&format!("/wake {SEAT}")),
            "the element after the root's value is the first turn: {argv:?}"
        );
        assert_eq!(
            rig.projection()["fleet"]["plugin_dir"],
            serde_json::json!(beside),
            "and the document names the root a reader would go looking for"
        );

        // The control: a fleet whose policy names none starts with no such
        // element and publishes null, so the readings above are the key's.
        let bare = Rig::new("effect-plugin-root-control");
        bare.write_roster("[]");
        let out = bare.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let argv = bare.start_argv();
        assert!(
            !argv.iter().any(|word| word == "--plugin-dir"),
            "no root is named, so no element is passed: {argv:?}"
        );
        assert_eq!(
            bare.projection()["fleet"]["plugin_dir"],
            serde_json::Value::Null
        );
    }

    /// A start the BUILT CONTROLLER issues hands the session the controller's own
    /// binary as `FLEET_BIN`, so the plugin's hooks in that session run the
    /// binary that spawned it — and not a build the plugin root may not hold,
    /// which blocks every Bash command the session makes.
    ///
    /// Out of process, because the subject is WHICH EXECUTABLE IS RUNNING: in
    /// this process the running executable is the test binary, and a value read
    /// off an in-process poll would pass against a controller that named any
    /// file at all.
    ///
    /// Asserted against the built binary's own path, canonical on both sides,
    /// which is what tells it from a pass-through: this process's `FLEET_BIN` is
    /// unset under a plain shell and names the seat's binary inside a flight.
    #[test]
    fn a_start_hands_the_session_the_controllers_own_binary_as_fleet_bin() {
        let rig = Rig::new("effect-fleet-bin");
        rig.write_roster("[]");

        let out = rig.observe_out_of_process();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["outcome"], "spawned");

        let handed = rig.start_fleet_bin();
        assert!(!handed.is_empty(), "the start carried a FLEET_BIN at all");
        let handed = std::fs::canonicalize(&handed)
            .unwrap_or_else(|e| panic!("the FLEET_BIN handed over, {handed}, resolves: {e}"));
        let built =
            std::fs::canonicalize(env!("CARGO_BIN_EXE_fleet")).expect("the built binary resolves");
        assert_eq!(
            handed, built,
            "the session is handed the binary the controller is running"
        );
    }

    /// A NAMED seat's start writes no settings into its worktree: that checkout
    /// is a person's, and their own permission rules stay theirs. Only a
    /// transient spawn renders the pack's document, and this loop makes none.
    ///
    /// Read off the worktree the loop actually started a session in, which the
    /// arm above proves the start was issued for.
    #[test]
    fn a_named_seats_start_leaves_the_persons_own_settings_alone() {
        let rig = Rig::new("effect-named-settings");
        rig.write_roster("[]");

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["outcome"], "spawned");

        // The control on the absence below: the checkout the start named is
        // there, so `.claude` is missing from a directory that exists rather
        // than from one nothing made.
        let worktree = rig.worktree();
        assert!(worktree.is_dir(), "{} is there", worktree.display());

        let claude = worktree.join(".claude");
        assert!(
            !claude.exists(),
            "{} was written into a named seat's own checkout",
            claude.display()
        );
    }

    /// AC3(b), AC7 and AC8 — the rest collection, in its fixed order of stop,
    /// start the successor, remove the predecessor, and the same collection
    /// with its stop failing.
    #[test]
    fn a_rest_is_stop_then_start_then_remove_and_a_failed_stop_retries() {
        let rig = Rig::new("effect-rest");
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        assert_eq!(rig.observe().status.code(), Some(0));

        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "a nap"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let calls = rig.calls();
        assert_eq!(calls.len(), 3, "three calls, no more: {calls:?}");
        assert_eq!(calls[0], "stop ab12", "{calls:?}");
        assert!(calls[1].starts_with("start "), "{calls:?}");
        assert_eq!(calls[2], "rm ab12", "{calls:?}");
        assert_eq!(rig.events_of("session.rested"), 1);
        assert_eq!(seat_row(&rig)["decision"], "rest");
        assert_eq!(seat_row(&rig)["outcome"], "rested");

        // AC7 — the cursor advanced past the rest it consumed, and the successor
        // has a row of its own.
        let table = rig.sessions();
        assert!(
            table["consumed_seq"].as_u64().unwrap_or(0) > 0,
            "the cursor moved past the event this tick consumed: {table}"
        );
        assert_eq!(table["sessions"].as_array().map(Vec::len), Some(1));
        assert_eq!(table["sessions"][0]["seat"], SEAT_ID);

        // AC8 — the in-flight field is CLEARED once the effect returned.
        assert!(
            rig.projection()["in_flight"].is_null(),
            "the poll is no longer inside an effect: {}",
            rig.projection()
        );
        assert_eq!(rig.projection()["effects"]["state"], "on");

        // The other half: a stop that does not exit 0.
        let rig = Rig::new("effect-rest-failed-stop");
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        assert_eq!(rig.observe().status.code(), Some(0));
        rig.set_seam(STOP_EXIT, Some(1));
        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "a nap"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.calls(), vec!["stop ab12".to_string()]);
        assert_eq!(rig.events_of("session.rested"), 0);
        assert_eq!(rig.events_of("session.spawned"), 0);
        assert_eq!(seat_row(&rig)["outcome"], "failed");
        assert_eq!(
            stderr(&out)
                .lines()
                .filter(|l| l.contains("stays pending"))
                .count(),
            1,
            "one line, and it names what did not happen: {}",
            stderr(&out)
        );

        // The retry: the same event is still standing, so the next poll takes it
        // again — and with the stop landing this time, the collection completes.
        rig.set_seam(STOP_EXIT, None);
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let calls = rig.calls();
        assert_eq!(
            calls.len(),
            4,
            "the retry is a second collection: {calls:?}"
        );
        assert_eq!(calls[1], "stop ab12", "{calls:?}");
        assert!(calls[2].starts_with("start "), "{calls:?}");
        assert_eq!(calls[3], "rm ab12", "{calls:?}");
        assert_eq!(rig.events_of("session.rested"), 1);
    }

    /// A rest whose stop landed and whose successor's start failed says the
    /// predecessor was stopped, and never that nothing was started.
    #[test]
    fn a_rest_whose_start_failed_says_its_predecessor_was_stopped() {
        let rig = Rig::new("effect-rest-failed-start");
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        assert_eq!(rig.observe().status.code(), Some(0));
        rig.set_seam(START_EXIT, Some(1));
        let out = rig
            .binary()
            .args(["event", "rest", SEAT, "--reason", "a nap"])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let calls = rig.calls();
        assert_eq!(calls.len(), 2, "a stop and a start, no removal: {calls:?}");
        assert_eq!(calls[0], "stop ab12", "{calls:?}");
        assert!(calls[1].starts_with("start "), "{calls:?}");
        assert_eq!(seat_row(&rig)["outcome"], "failed");
        let lines = stderr(&out);
        assert_eq!(
            lines.lines().filter(|l| l.contains("was stopped")).count(),
            1,
            "one line, and it says the stop landed: {lines}"
        );
        assert_eq!(
            lines
                .lines()
                .filter(|l| l.contains("nothing was started"))
                .count(),
            0,
            "{lines}"
        );
    }

    /// AC3(d) and AC8 — one nudge per session, and a new session id re-arms it.
    #[test]
    fn a_heavy_seat_is_nudged_once_per_session_and_never_twice() {
        let rig = Rig::new("effect-nudge");
        rig.write_policy(&policy_with(&format!(
            "rest_threshold_tokens = {LOW_THRESHOLD}\n"
        )));
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        rig.write_transcript("ab12", &transcript_of(LOW_THRESHOLD + 5));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "suggest-rest");
        assert_eq!(seat_row(&rig)["outcome"], "nudged");
        assert_eq!(rig.events_of("session.nudged"), 1);
        assert_eq!(
            rig.calls()
                .iter()
                .filter(|c| c.starts_with("nudge "))
                .count(),
            1
        );
        let nudged = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "session.nudged")
            .expect("the nudge wrote its event");
        assert_eq!(nudged["payload"]["session"], "ab12");
        assert_eq!(nudged["payload"]["threshold"], LOW_THRESHOLD);
        assert_eq!(nudged["payload"]["outcome"], "sent");

        // Two more polls, still one.
        for _ in 0..2 {
            assert_eq!(rig.observe().status.code(), Some(0));
        }
        assert_eq!(rig.events_of("session.nudged"), 1);
        assert_eq!(
            rig.calls()
                .iter()
                .filter(|c| c.starts_with("nudge "))
                .count(),
            1,
            "the budget is per session, not per poll: {:?}",
            rig.calls()
        );

        // A new session for the same seat re-arms it, with no bookkeeping of its
        // own — the map is keyed on the session id.
        rig.write_roster(&live_row(&rig.worktree(), "cd34"));
        rig.write_transcript("cd34", &transcript_of(LOW_THRESHOLD + 5));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.nudged"), 2);
        assert_eq!(rig.sessions()["nudged"][SEAT_ID], "cd34");
    }

    /// AC3(e) — a start that exits non-zero inside the watch window is a failure
    /// with a cause, and it leaves no row for the arrival window to wait on.
    #[test]
    fn a_start_that_fails_inside_the_window_is_a_crash_with_a_cause_and_no_row() {
        let rig = Rig::new("effect-failed-start");
        rig.write_roster("[]");
        rig.set_seam(START_EXIT, Some(1));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.crashed"), 1);
        assert_eq!(rig.events_of("session.spawned"), 0);
        assert_eq!(seat_row(&rig)["outcome"], "failed");
        let crashed = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "session.crashed")
            .expect("the failure is an event");
        assert_eq!(crashed["payload"]["phase"], "start");
        assert!(
            crashed["payload"]["cause"]
                .as_str()
                .unwrap_or_default()
                .contains("exited 1"),
            "the cause carries the child's own status: {crashed}"
        );
        let output = crashed["payload"]["output"].as_str().unwrap_or_default();
        assert!(
            std::fs::read_to_string(output)
                .expect("the cause names the file the output went to")
                .contains("the start spoke"),
            "and the file holds what the child printed"
        );
        assert_eq!(
            rig.try_sessions()
                .map(|t| t["sessions"].as_array().map(Vec::len).unwrap_or(0)),
            Some(0),
            "a start that failed opens no row"
        );

        // The control: the same poll with the stub exiting 0 opens one.
        rig.set_seam(START_EXIT, None);
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.spawned"), 1);
        assert_eq!(rig.sessions()["sessions"].as_array().map(Vec::len), Some(1));
    }

    /// AC3(f) — a row whose model cannot honour the posture its start would ask
    /// for is dropped at config read, and nothing is started for it.
    #[test]
    fn a_row_whose_model_cannot_honour_the_posture_is_dropped_before_any_start() {
        let rig = Rig::new("effect-model-gate");
        rig.write_roster("[]");
        rig.write_config(&format!(
            r#"{{"fleet_toml": "{}", "children": [
                 {{"id":"{SEAT_ID}","name":"Orla",
                   "model":"claude-haiku-4-5-20251001",
                   "worktrees":{{"demo":"{}"}}}}
               ]}}"#,
            rig.policy_path().display(),
            rig.worktree().display()
        ));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let said = stderr(&out);
        assert!(
            said.contains("claude-haiku-4-5-20251001") && said.contains("claude-opus-5"),
            "the drop names the model and the list it is outside of: {said}"
        );
        assert!(
            rig.calls().is_empty(),
            "no start was attempted for it: {:?}",
            rig.calls()
        );
        assert_eq!(
            rig.projection()["seats"].as_array().map(Vec::len),
            Some(0),
            "the row is dropped at config read, so it is not published either"
        );

        // The control: the same row on a model that CAN honour the posture is
        // started, so the drop is the model's and not this rig's.
        rig.write_config(&rig.one_seat_config(rig.policy_path()));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.spawned"), 1);
    }

    /// AC8 — the projection's four new fields, and the effects gate with its
    /// cause.
    #[test]
    fn the_projection_carries_the_decision_the_outcome_and_the_effects_gate() {
        let rig = Rig::new("effect-projection");
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let published = rig.projection();
        assert_eq!(published["seats"][0]["decision"], "leave-alone");
        assert_eq!(published["seats"][0]["outcome"], "none");
        assert!(published["in_flight"].is_null());
        assert_eq!(published["effects"]["state"], "on");
        assert!(
            published["effects"].get("cause").is_none(),
            "a gate that is open carries no cause: {published}"
        );

        // With the agent binary unresolvable the loop still observes and
        // publishes, and says why it is issuing nothing.
        let out = rig.observe_with_env(&[(
            common::hermetic::CLAUDE_BIN,
            Some(rig.root.join("no-such-agent").as_os_str()),
        )]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let published = rig.projection();
        assert_eq!(published["effects"]["state"], "off");
        let cause = published["effects"]["cause"].as_str().unwrap_or_default();
        assert!(
            cause.contains("no-such-agent"),
            "the gate names what it could not resolve: {cause}"
        );
        assert_eq!(
            published["seats"][0]["outcome"], "none",
            "and nothing was done: {published}"
        );
    }

    /// AC2 — a revive DISPATCHES an attach of the row's short id, emits one
    /// `session.revived`, and publishes `revived` as its outcome.
    ///
    /// The attach is addressed by the SHORT id and never by the session id: the
    /// two are different values, and a call issued against the identity reaches
    /// no row (lessons claude-code A6, A9).
    ///
    /// So the row carries an `id` and a `sessionId` that DIFFER, and the
    /// transcript is keyed under the identity — the arm cannot measure its own
    /// claim from a row that writes one value into both.
    #[test]
    fn a_revive_attaches_the_rows_short_id_and_says_so_once() {
        let rig = Rig::new("effect-revive");
        // A pid-less row with no deliberate end and a reading under the
        // threshold is a revive.
        rig.write_roster(&ended_row_addressed(
            &rig.worktree(),
            "ab12",
            "a-session",
            now_ms(),
        ));
        rig.write_transcript("a-session", &transcript_of(10));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "revive");
        assert_eq!(seat_row(&rig)["outcome"], "revived");
        assert_eq!(
            rig.calls(),
            vec!["attach ab12".to_string()],
            "the attach takes the row's ADDRESS and the poll issues nothing else"
        );
        assert_eq!(rig.events_of("session.revived"), 1);
        let revived = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "session.revived")
            .expect("the event is in the stream");
        assert_eq!(revived["payload"]["session"], "a-session");
        assert_eq!(revived["payload"]["address"], "ab12");
        assert_eq!(revived["payload"]["outcome"], "dispatched");

        // The row is RE-OPENED as this dispatch's, so the arrival window is
        // keyed to the attach and answered by a sighting. Without it the same
        // row reads sighted and is revived again on the next poll.
        let row = &rig.sessions()["sessions"][0];
        assert!(
            row["session_id"].is_null(),
            "the revive's row waits on a sighting: {row}"
        );
        assert_eq!(row["dispatch_id"], revived["id"]);
    }

    /// AC2 — the revive counts toward the blind counter EVEN WHEN THE ATTACH
    /// FAILS, because the call's own exit is not a witness either way (lessons
    /// claude-code A7).
    #[test]
    fn a_revive_whose_attach_failed_still_counts_as_a_dispatch() {
        let rig = Rig::new("effect-revive-failed");
        rig.write_roster(&ended_row(&rig.worktree(), "ab12", now_ms()));
        rig.write_transcript("ab12", &transcript_of(10));
        rig.set_seam(ATTACH_EXIT, Some(1));

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "revive");
        assert_eq!(seat_row(&rig)["outcome"], "failed");
        assert_eq!(
            seat_row(&rig)["blind"],
            1,
            "the dispatch went out and nothing sighted it: {}",
            rig.projection()
        );
        assert_eq!(rig.events_of("dispatch.blind"), 1);
        assert_eq!(rig.sessions()["seats"][SEAT_ID]["blind"], 1);

        // And the failed attach opened no window: the row still carries its
        // session, so the next poll decides about the same pid-less row again.
        assert_eq!(rig.events_of("session.revived"), 1);
    }

    /// AC3 — three consecutive blind dispatches halt the seat: one
    /// `session.halted`, the halt published, the row carrying its count and its
    /// flag, and a clear-halt that lifts it exactly once.
    #[test]
    fn three_blind_dispatches_halt_the_seat_and_a_clear_halt_lifts_it() {
        let rig = Rig::new("effect-halt");
        // A seat with no row at all: every poll spawns, and the stub's start
        // never puts a row on the roster, so no sighting ever answers.
        rig.write_roster("[]");
        rig.set_seam(START_EXIT, Some(1));

        for poll in 1..=3 {
            let out = rig.observe();
            assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
            assert_eq!(
                seat_row(&rig)["decision"],
                "spawn-woken",
                "poll {poll} dispatched"
            );
        }
        assert_eq!(rig.events_of("dispatch.blind"), 3);
        assert_eq!(
            rig.events_of("session.halted"),
            1,
            "once, at the transition"
        );

        // The fourth poll is the halt itself.
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let row = seat_row(&rig);
        assert_eq!(row["decision"], "halt");
        assert_eq!(row["outcome"], "halted");
        assert_eq!(row["blind"], 3, "the seat row carries its count: {row}");
        assert_eq!(row["halted"], true);
        assert_eq!(
            rig.events_of("session.halted"),
            1,
            "and the announcement is not repeated once per poll"
        );

        // The remedy: a person's request, consumed on the next tick.
        let out = rig
            .binary()
            .args(["event", "clear-halt", SEAT])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("seat.clear_halt"), 1);

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("halt lifted by request"),
            "the reset is announced: {}",
            stderr(&out)
        );
        let row = seat_row(&rig);
        assert_eq!(row["halted"], false);
        assert_eq!(
            row["decision"], "spawn-woken",
            "and the poll after the reset dispatches again: {row}"
        );
    }

    /// AC3 — the cli's clear-halt refusals, each red-proved against a fleet in
    /// the state that produces it.
    #[test]
    fn clear_halt_refuses_with_no_collector_and_for_a_seat_that_is_not_halted() {
        let rig = Rig::new("clear-halt-refusals");

        // 5 — nothing has published, so nothing would consume the request.
        let out = rig
            .binary()
            .args(["event", "clear-halt", SEAT])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("no collector is consuming"),
            "{}",
            stderr(&out)
        );

        // 1 — a published fleet whose seat is not halted, printing the state it
        // read. The projection is a REAL one, published by a poll of this rig.
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["halted"], false);

        let out = rig
            .binary()
            .args(["event", "clear-halt", SEAT])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("is not halted") && stderr(&out).contains("present"),
            "the refusal prints the state it read: {}",
            stderr(&out)
        );
        assert_eq!(
            rig.events_of("seat.clear_halt"),
            0,
            "and a refused request writes nothing"
        );
    }

    /// AC4 — a pid-less row is held with the replacement-held reason while the
    /// daemon is young, and dispatched against once the window closes.
    #[test]
    fn a_pidless_row_is_held_while_the_stub_daemon_is_young() {
        let rig = Rig::new("effect-replacement");
        rig.write_roster(&ended_row(&rig.worktree(), "ab12", now_ms()));
        rig.write_transcript("ab12", &transcript_of(10));
        // The arrival window is 45s by default, so a two-second daemon is inside
        // it and a one-hour one is not.
        rig.write_daemon(4242, "2s");

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "leave-alone");
        assert!(
            stderr(&out).contains("replacement window: held"),
            "the hold names itself: {}",
            stderr(&out)
        );
        assert!(
            rig.calls().is_empty(),
            "and nothing was dispatched: {:?}",
            rig.calls()
        );

        // The window closes and the same row is revived.
        rig.write_daemon(4242, "1h");
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "revive");
        assert_eq!(rig.calls(), vec!["attach ab12".to_string()]);

        // And an unreadable daemon read opens NO window on its own: the same row
        // with the status call failing is dispatched against.
        let rig = Rig::new("effect-replacement-unreadable");
        rig.write_roster(&ended_row(&rig.worktree(), "cd34", now_ms()));
        rig.write_transcript("cd34", &transcript_of(10));
        rig.set_seam(DAEMON_EXIT, Some(1));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(seat_row(&rig)["decision"], "revive");
    }

    /// AC2, AC7 — a restart ADOPTS the live sessions its table names, and a
    /// table that will not parse is rebuilt from the stream rather than lost.
    #[test]
    fn a_restart_adopts_the_live_sessions_and_a_lost_table_is_rebuilt() {
        let rig = Rig::new("effect-adopt");
        // A seat with no row: the first poll spawns and opens a table row.
        rig.write_roster("[]");
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.spawned"), 1);

        // The session then appears live, and a poll sights it onto that row.
        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.sessions()["sessions"][0]["session_id"], "a-session");

        // A restart: the loop reads the table, finds the session still live and
        // CLAIMS it — one event, no second start.
        let before = rig.calls().len();
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.adopted"), 1);
        let adopted = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "session.adopted")
            .expect("the event is in the stream");
        assert_eq!(adopted["payload"]["session"], "a-session");
        assert_eq!(adopted["actor"], SEAT_ID);
        assert_eq!(
            rig.calls().len(),
            before,
            "adoption issues no start: {:?}",
            rig.calls()
        );

        // AC7 — the table is destroyed and the next restart REBUILDS it from
        // the stream, cursor and all, rather than starting from empty.
        let seq = rig.events().last().expect("the stream has lines")["seq"]
            .as_u64()
            .expect("every line carries a sequence");
        write(&rig.machine().join("sessions.json"), "{not json at all");
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("rebuilt from the event stream"),
            "the rebuild says so: {}",
            stderr(&out)
        );
        let rebuilt = rig.sessions();
        assert!(
            rebuilt["consumed_seq"].as_u64().unwrap_or(0) >= seq,
            "the cursor is the last sequence folded: {rebuilt}"
        );
        assert!(
            rebuilt["sessions"]
                .as_array()
                .map(|rows| !rows.is_empty())
                .unwrap_or(false),
            "and the rows came back: {rebuilt}"
        );
    }

    /// Adoption is recorded once per SESSION, not once per process: a `--once`
    /// poll after the restart that claimed the session claims nothing again.
    #[test]
    fn a_second_restart_does_not_adopt_the_session_again() {
        let rig = Rig::new("effect-adopt-once");
        rig.write_roster("[]");
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(rig.events_of("session.spawned"), 1);

        rig.write_roster(&live_row(&rig.worktree(), "a-session"));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(
            rig.events_of("session.adopted"),
            1,
            "the first restart claims the session"
        );

        let before = rig.calls();
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(
            rig.events_of("session.adopted"),
            1,
            "a second restart claims nothing: {}",
            stderr(&out)
        );
        assert_eq!(rig.calls(), before, "and issues nothing");
    }

    /// AC7 — the rebuild folds a STANDING halt out of the stream: a
    /// `session.halted` with no clear after it is a hold that survives the table
    /// being lost, and the daemon pid starts at none.
    #[test]
    fn a_rebuilt_table_carries_a_standing_halt_and_no_daemon_pid() {
        let rig = Rig::new("effect-rebuild-halt");
        rig.write_roster("[]");
        rig.set_seam(START_EXIT, Some(1));
        rig.write_daemon(4242, "1h");
        for _ in 0..3 {
            let out = rig.observe();
            assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        }
        assert_eq!(rig.events_of("session.halted"), 1);
        assert_eq!(rig.sessions()["seats"][SEAT_ID]["halted"], true);
        assert_eq!(rig.sessions()["daemon_pid"], 4242);

        write(&rig.machine().join("sessions.json"), "{not json at all");
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        let row = seat_row(&rig);
        assert_eq!(
            row["halted"], true,
            "the hold survived the table's loss: {row}"
        );
        assert_eq!(row["decision"], "halt");
        assert_eq!(
            rig.events_of("session.halted"),
            1,
            "and the transition is not announced a second time"
        );

        // The control: the same stream with a clear-halt after the halt folds to
        // no hold at all.
        let out = rig
            .binary()
            .args(["event", "clear-halt", SEAT])
            .output()
            .expect("the built binary runs");
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        write(&rig.machine().join("sessions.json"), "{not json at all");
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(
            seat_row(&rig)["halted"],
            false,
            "a halt with a clear after it is not a standing one"
        );
    }

    /// AC2, AC7 — a session table that is GONE is rebuilt from the stream, the
    /// same as one that will not parse.
    ///
    /// Its own arm because the two reach the rebuild by different paths: an
    /// unparseable file is a read that FAILED, and a deleted one is a read that
    /// found nothing — and a reader that treats the second as an empty fleet
    /// rather than as a table to rebuild forgets a standing halt, which is the
    /// failure the hold exists to prevent.
    #[test]
    fn a_deleted_table_is_rebuilt_from_the_stream_and_the_halt_survives() {
        let rig = Rig::new("effect-deleted-table");
        rig.write_roster("[]");
        rig.set_seam(START_EXIT, Some(1));
        rig.write_daemon(4242, "1h");
        for _ in 0..3 {
            let out = rig.observe();
            assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        }
        assert_eq!(rig.events_of("session.halted"), 1);
        assert_eq!(rig.sessions()["seats"][SEAT_ID]["halted"], true);

        // The control that the file was there to lose: the poll below reads a
        // path that resolves to nothing.
        let table = rig.machine().join("sessions.json");
        assert!(table.exists(), "the table is there before it is deleted");
        std::fs::remove_file(&table).expect("the table is deleted");
        assert!(!table.exists());

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            stderr(&out).contains("rebuilt from the event stream"),
            "a table that is GONE is rebuilt, not started from empty: {}",
            stderr(&out)
        );
        let row = seat_row(&rig);
        assert_eq!(
            row["halted"], true,
            "the hold survived the table being deleted: {row}"
        );
        assert_eq!(row["decision"], "halt");
        assert_eq!(row["blind"], 3);
        assert_eq!(
            rig.events_of("session.halted"),
            1,
            "and the transition is not announced a second time"
        );

        // The cursor came back with the rows, so the tick does not re-fold the
        // whole stream and re-announce what it already consumed.
        let rebuilt = rig.sessions();
        assert!(
            rebuilt["consumed_seq"].as_u64().unwrap_or(0) > 0,
            "the cursor is the last sequence folded: {rebuilt}"
        );
    }

    /// A seat event whose actor names no configured row is dropped with a line.
    ///
    /// The actor is matched on the seat's id: another seat's id is no row of
    /// this list, and neither is this seat's own machine name — the shape a line
    /// an older build wrote carries.
    #[test]
    fn a_seat_event_for_a_row_the_seat_list_does_not_carry_is_dropped() {
        const ANOTHER: &str = "01a0d1f1-0aec-765f-9abe-0000000000ff";
        for actor in [ANOTHER, SEAT] {
            let rig = Rig::new("effect-unknown-actor");
            rig.write_roster(&live_row(&rig.worktree(), "ab12"));
            assert_eq!(rig.observe().status.code(), Some(0));

            append_event(
                &rig,
                "seat.resting",
                actor,
                serde_json::json!({"reason": "x"}),
            );
            let out = rig.observe();
            assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
            assert!(
                stderr(&out).contains(&format!(
                    "dropping a seat.resting whose actor `{actor}` names no seat row"
                )),
                "the drop is stated: {}",
                stderr(&out)
            );
            assert!(rig.calls().is_empty(), "{actor}: {:?}", rig.calls());
        }

        // The control: the same line by this seat's id is taken, and asks for a
        // rest the next poll collects.
        let rig = Rig::new("effect-known-actor");
        rig.write_roster(&live_row(&rig.worktree(), "ab12"));
        assert_eq!(rig.observe().status.code(), Some(0));
        append_event(
            &rig,
            "seat.resting",
            SEAT_ID,
            serde_json::json!({"reason": "x"}),
        );
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(
            !stderr(&out).contains("names no seat row"),
            "{}",
            stderr(&out)
        );
        assert_eq!(seat_row(&rig)["decision"], "rest", "{}", seat_row(&rig));
    }

    /// An effect execs the binary the GATE resolved, on the constructed path,
    /// and not whatever `claude` this process's own search path finds first: a
    /// child's PATH is constructed, never inherited.
    ///
    /// The two are separated by putting a DIFFERENT `claude` first on the
    /// controller's `PATH` from the one the constructed path resolves, with the
    /// seam unset so both questions fall to the bare default. Every other arm in
    /// this file sets `FLEET_CLAUDE_BIN` to one absolute path, which makes the
    /// two resolutions one value and hides the difference — this arm is the only
    /// one that can see it.
    ///
    /// The reading is the child's own `$0`, not the argv: the argv says what the
    /// call passed and is byte-identical either way.
    ///
    /// THE PRECONDITION IS ASSERTED BEFORE THE POLL, and that ordering is the
    /// safety as much as the reading. If some box carries a `claude` earlier on
    /// the constructed path than the home's `.local/bin` — a package manager's
    /// prefix holds one on a fleet member — this arm would otherwise EXEC A REAL
    /// AGENT. So it resolves the constructed path itself first and reds naming
    /// the shadow instead.
    #[test]
    fn an_effect_execs_the_resolved_binary_and_not_the_first_claude_on_this_processs_path() {
        let rig = Rig::new("effect-binary");
        rig.write_roster("[]");

        let constructed = rig.plant_stub_on_the_constructed_path();
        let resolved = fleet_controller::platform::resolve_on_path(
            &child_path(&rig.home()),
            fleet_controller::adapter::claude_code::DEFAULT_BIN,
        );
        assert_eq!(
            resolved.as_deref(),
            Some(constructed.as_path()),
            "a `claude` earlier on the constructed path than this rig's own shadows it; \
             this arm will not exec a binary it did not plant"
        );

        // The decoy: the same recording stub under the same name, in a directory
        // that is FIRST on the controller's own `PATH` and on no constructed
        // one. It answers the listing and the version, so the poll gets as far
        // as deciding — which is the point: the reads may run here, the effect
        // may not.
        let decoy_dir = rig.root.join("decoy-bin");
        std::fs::create_dir_all(&decoy_dir).unwrap();
        let decoy = decoy_dir.join(fleet_controller::adapter::claude_code::DEFAULT_BIN);
        std::fs::copy(rig.stub_path(), &decoy).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o755)).unwrap();

        let out = rig.observe_with_env(&[
            (common::hermetic::CLAUDE_BIN, None),
            ("PATH", Some(decoy_dir.as_os_str())),
        ]);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        // The poll reached the effect at all, so the assertion below is about
        // which binary ran and not about a start that never happened.
        assert_eq!(seat_row(&rig)["decision"], "spawn-woken");
        assert_eq!(seat_row(&rig)["outcome"], "spawned");
        assert_eq!(rig.projection()["effects"]["state"], "on");

        assert_eq!(
            rig.start_bin(),
            constructed,
            "the effect exec'd the binary the gate resolved on the constructed path"
        );
        assert_ne!(
            rig.start_bin(),
            decoy,
            "and not the first `claude` on this process's own search path"
        );

        // The control that makes the decoy a decoy: it IS what the reads run,
        // because observe resolves its bare name on the process path. Without
        // this, the arm would pass against a controller that could not see the
        // decoy at all — and would then be measuring nothing.
        assert_eq!(
            fleet_controller::platform::resolve_on_path(
                &decoy_dir.display().to_string(),
                fleet_controller::adapter::claude_code::DEFAULT_BIN,
            )
            .as_deref(),
            Some(decoy.as_path()),
            "the decoy is on the path the reads resolve"
        );
        assert_eq!(
            rig.projection()["agent_version"],
            "9.9.9",
            "and the version read answered through it: {}",
            rig.projection()
        );
    }

    /// One line into the stream, written the way something other than the CLI
    /// would write it — which is the case the consumer's drops exist for.
    fn append_event(rig: &Rig, kind: &str, actor: &str, payload: serde_json::Value) {
        let path = rig.events_path();
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        let seq = body
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|l| l["seq"].as_u64())
            .max()
            .unwrap_or(0)
            + 1;
        let line = serde_json::json!({
            "id": format!("by-hand-{seq}"), "seq": seq, "ts": "2026-09-08T00:00:00Z",
            "type": kind, "actor": actor, "payload": payload
        });
        write(&path, &format!("{body}{line}\n"));
    }
}

/// The per-row configuration directory, driven through the BUILT controller:
/// each spawned seat starts under a configuration directory of its own, and
/// every read of its session has to go through that one.
///
/// The unit arms in `controller/tests/observe.rs` pin the fold over values they
/// build themselves; these drive the whole path — the listing the loop asks for,
/// the transcript it resolves, and the line it writes — which is where a read
/// threaded to the wrong directory shows up.
mod isolation {
    use super::*;

    /// A seat list with one transient row, which is the kind that comes up under
    /// its own configuration directory.
    fn one_transient_seat(rig: &Rig) -> String {
        format!(
            r#"{{"fleet_toml": "{}", "children": [
                 {{"id":"{SEAT_ID}","name":"Orla","transient":true,
                   "worktrees":{{"demo":"{}"}}}}
               ]}}"#,
            rig.policy_path().display(),
            rig.worktree().display()
        )
    }

    /// The session table the controller would have written for a dispatch that
    /// came up under its own directory: one transient row naming the directory
    /// and the item the order index named, sighted by nothing yet.
    fn table_naming(config_dir: &Path, item: &str, worktree: &Path) -> String {
        format!(
            r#"{{"schema":2,"consumed_seq":0,"nudged":{{}},"seats":{{}},"sessions":[
                 {{"seat":"{SEAT_ID}","project":"demo","worktree":"{}",
                   "name":"{SEAT}","model":"a-model","posture":"dontAsk",
                   "first_turn":"a brief","transient":true,
                   "config_dir":"{}","item":"{}",
                   "dispatch_id":"a-dispatch","dispatched_at":1000}}
               ]}}"#,
            worktree.display(),
            config_dir.display(),
            item
        )
    }

    /// The seat's published row.
    fn seat_row(rig: &Rig) -> serde_json::Value {
        rig.projection()["seats"][0].clone()
    }

    /// The provider's logged-out first turn, as it was read off a real
    /// transcript on this box on 2026-09-12 (2.1.261).
    const LOGGED_OUT: &str = concat!(
        r#"{"type":"user","isSidechain":false,"message":{"role":"user"}}"#,
        "\n",
        r#"{"type":"assistant","isSidechain":false,"isApiErrorMessage":true,"#,
        r#""error":"authentication_failed","message":{"model":"<synthetic>","#,
        r#""usage":{"input_tokens":0,"output_tokens":0,"#,
        r#""cache_creation_input_tokens":0,"cache_read_input_tokens":0},"#,
        r#""content":[{"type":"text","text":"Not logged in"}]}}"#,
        "\n",
    );

    /// The transcript the adapter resolves for a session under one configuration
    /// directory, written where the adapter will look for it.
    fn write_transcript(config_dir: &Path, worktree: &Path, session: &str, body: &str) {
        let path = config_dir
            .join("projects")
            .join(encoded(worktree))
            .join(format!("{session}.jsonl"));
        std::fs::create_dir_all(path.parent().expect("the transcript has a parent"))
            .expect("the transcript directory is made");
        write(&path, body);
        assert!(
            path.exists(),
            "the fixture is on disk at {}",
            path.display()
        );
    }

    /// The agent's per-project directory name: every character that is not
    /// alphanumeric becomes a dash (lessons claude-code C1).
    fn encoded(worktree: &Path) -> String {
        worktree
            .display()
            .to_string()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }

    /// AC2 — the controller asks for a transient row's listing UNDER THAT ROW'S
    /// directory, and the row is seen there while the fleet's listing does not
    /// name it.
    ///
    /// The two listings differ in content, so the arm cannot pass by reading
    /// either one twice: the fleet's roster is empty and the per-row one carries
    /// the session.
    #[test]
    fn a_transient_rows_listing_is_read_under_its_own_configuration_directory() {
        let rig = Rig::new("isolation-per-row-listing");
        let config_dir = rig.machine().join("config").join(SEAT);
        std::fs::create_dir_all(&config_dir).expect("the per-row directory is made");
        rig.write_config(&one_transient_seat(&rig));
        // The FLEET's listing names nothing, which is what a per-row daemon
        // leaves it saying.
        rig.write_roster("[]");
        // The row's own listing names the session, served by the stub out of the
        // directory it was asked under.
        write(
            &config_dir.join("roster.json"),
            &live_row(&rig.worktree(), "a-session"),
        );
        write(
            &rig.machine().join("sessions.json"),
            &table_naming(&config_dir, "an-item", &rig.worktree()),
        );

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

        // BOTH directories were asked, and the row's own is among them: this is
        // what a fold reading every row under the fleet's own leaves out.
        let dirs = rig.listing_dirs();
        assert!(
            dirs.iter().any(|d| Path::new(d) == config_dir),
            "the row's own directory was read: {dirs:?}"
        );
        assert!(
            dirs.iter().any(|d| Path::new(d) != config_dir),
            "and the fleet's was read too: {dirs:?}"
        );

        let row = seat_row(&rig);
        assert_eq!(
            row["roster_state"], "present",
            "the row is seen through its own directory: {row}"
        );
    }

    /// AC1's second half and AC3 — a transient row whose first turn answered
    /// LOGGED OUT puts exactly one `dispatch.failed` on the stream, naming the
    /// seat and the item, and a row that answered puts none.
    ///
    /// Measured live on this box on 2026-09-12: the same start one variable apart
    /// — the credential knob unset rather than defined-and-empty — wrote exactly
    /// this transcript shape, while the defined-empty arm answered with a real
    /// turn carrying 36,072 tokens of window.
    #[test]
    fn a_logged_out_first_turn_writes_one_dispatch_failed_naming_the_seat_and_the_item() {
        let rig = Rig::new("isolation-logged-out");
        let config_dir = rig.machine().join("config").join(SEAT);
        std::fs::create_dir_all(&config_dir).expect("the per-row directory is made");
        rig.write_config(&one_transient_seat(&rig));
        rig.write_roster("[]");
        write(
            &config_dir.join("roster.json"),
            &live_row(&rig.worktree(), "a-session"),
        );
        write(
            &rig.machine().join("sessions.json"),
            &table_naming(&config_dir, "an-item", &rig.worktree()),
        );
        write_transcript(&config_dir, &rig.worktree(), "a-session", LOGGED_OUT);

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(
            rig.events_of("dispatch.failed"),
            1,
            "one line for the logged-out dispatch: {:?}",
            rig.events()
        );
        let line = rig
            .events()
            .into_iter()
            .find(|e| e["type"] == "dispatch.failed")
            .expect("the line is on the stream");
        assert_eq!(line["actor"], SEAT_ID);
        assert_eq!(line["payload"]["seat"], SEAT);
        assert_eq!(line["payload"]["item"], "an-item");
        assert_eq!(line["payload"]["cause"], "authentication_failed");

        // ONCE PER ROW. The second poll's row is already sighted, so a reading
        // that still stands writes no second line.
        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert_eq!(
            rig.events_of("dispatch.failed"),
            1,
            "and not a line per poll: {:?}",
            rig.events()
        );
    }

    /// The control on the arm above, one variable apart: the same rig with a
    /// transcript that ANSWERED writes no line at all.
    ///
    /// Without it the assertion above would pass over a controller that wrote
    /// the line for every transient row it sighted.
    #[test]
    fn a_first_turn_that_answered_writes_no_dispatch_failed() {
        let rig = Rig::new("isolation-answered");
        let config_dir = rig.machine().join("config").join(SEAT);
        std::fs::create_dir_all(&config_dir).expect("the per-row directory is made");
        rig.write_config(&one_transient_seat(&rig));
        rig.write_roster("[]");
        write(
            &config_dir.join("roster.json"),
            &live_row(&rig.worktree(), "a-session"),
        );
        write(
            &rig.machine().join("sessions.json"),
            &table_naming(&config_dir, "an-item", &rig.worktree()),
        );
        write_transcript(
            &config_dir,
            &rig.worktree(),
            "a-session",
            "{\"type\":\"assistant\",\"isSidechain\":false,\
             \"message\":{\"usage\":{\"input_tokens\":36072}}}\n",
        );

        let out = rig.observe();
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        // The positive control that the transcript was READ at all: the context
        // reading is the same file, resolved through the same directory.
        let row = seat_row(&rig);
        assert_eq!(
            row["context_tokens"], 36072,
            "the transcript was read under the row's directory: {row}"
        );
        assert_eq!(
            rig.events_of("dispatch.failed"),
            0,
            "a turn that answered is no dispatch failure: {:?}",
            rig.events()
        );
    }
}
