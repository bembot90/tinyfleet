//! What one poll reads about a seat, and what it publishes.
//!
//! One of the `drive_*` binaries, each an integration test of `fleet observe`
//! against a stub agent over one subject. The rig they share — the stub, the
//! seams, the fixture writers and the two poll routes — is `drive/rig.rs`,
//! included at file scope so every arm reads it the way it reads its own
//! neighbours.
#![allow(dead_code, unused_imports)]

include!("drive/rig.rs");

mod common;

/// The window is measured from the session's END, and this is the reading that
/// separates that from the start: a session that RAN for 25 hours and finished
/// a minute ago. Keyed on the start it is a day-old row and its transcript goes
/// with it; keyed on the end it is the freshest stopped row there is.
///
/// The end is the transcript's last write, so the transcript is written here
/// and left at its own mtime — writing it IS the session's last act.
#[test]
fn a_long_session_that_just_ended_reads_stopped_and_not_history() {
    let rig = Rig::new("recency-just-ended");
    rig.write_roster(&ended_row(
        &rig.worktree(),
        "a-session",
        now_ms() - 25 * HOUR_MS,
    ));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );

    assert_eq!(
        rig.observe().status.code(),
        Some(0),
        "{}",
        rig.root.display()
    );
    let row = &rig.projection()["seats"][0];
    assert_eq!(
        row["roster_state"], "stopped",
        "a session that ended a minute ago is stopped however long it ran: {row}"
    );
    assert!(
        row["roster_recency_fallback"].is_null(),
        "and it was ranked on its END, so nothing fell back: {row}"
    );
    // The transcript is the evidence the row keeps by staying stopped: an absent
    // row publishes no reading at all, so this is the same claim from the side a
    // reader of the document cares about.
    assert_eq!(
        row["context_tokens"], 18,
        "the stopped row still answers for context: {row}"
    );
}

/// The window itself is POLICY, read from `fleet.toml` like the poll interval.
/// One row, one end, two files: the same 23-hour-old session is inside a 24 h
/// window and outside a 12 h one.
///
/// Both readings are taken on the SAME rig so nothing but the policy figure
/// moves between them.
#[test]
fn the_stopped_window_is_the_policys_and_not_a_constant() {
    let rig = Rig::new("recency-policy");
    rig.write_roster(&ended_row(
        &rig.worktree(),
        "a-session",
        now_ms() - 23 * HOUR_MS,
    ));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );
    rig.age_transcript("a-session", 23 * HOUR_MS);

    rig.write_policy("[controller]\npoll_seconds = 1\nstopped_recency_hours = 24\n");
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(
        rig.projection()["seats"][0]["roster_state"],
        "stopped",
        "23 hours is inside a 24 hour window: {}",
        rig.projection()["seats"][0]
    );

    rig.write_policy("[controller]\npoll_seconds = 1\nstopped_recency_hours = 12\n");
    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(
        rig.projection()["seats"][0]["roster_state"],
        "absent",
        "and outside a 12 hour one — the figure is the file's: {}",
        rig.projection()["seats"][0]
    );
}

#[test]
fn a_poll_publishes_the_seat_and_the_context_it_is_carrying() {
    let rig = Rig::new("publish");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":5,\
         \"cache_read_input_tokens\":6,\"cache_creation_input_tokens\":7}}}\n",
    );

    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let published = rig.projection();
    assert_eq!(published["version"], 1);
    assert_eq!(published["agent_version"], "9.9.9");
    assert_eq!(published["agent_version_expected"], "9.9.9");
    assert_eq!(published["fleet"]["poll_seconds"], 1);
    assert!(published["fleet"]["mtime"].as_str().is_some());
    assert_eq!(published["seats"][0]["seat_dir"], "builder-1");
    assert_eq!(published["seats"][0]["chosen_name"], "Orla");
    assert_eq!(published["seats"][0]["roster_state"], "present");
    assert_eq!(published["seats"][0]["context_tokens"], 18);
    assert_eq!(published["seats"][0]["project"], "demo");
}

/// `model` and `transient` are the porter's intent, and publishing either would
/// hand a reader that intent as the fleet's state. The unit arm in `observe.rs`
/// pins the same absence over a row it builds ITSELF; this one drives the
/// controller's own publish path, where `run.rs` is the single non-test caller
/// of `SeatRow::from_observation` — so a line added after that call leaks past
/// the unit arm with the whole suite green, and is caught only here.
///
/// The fields go in the CONFIG, which is the one place upstream of the parse:
/// the controller reads them onto the seat and it is the publisher that has to
/// drop them.
///
/// Two assertions, because a leak has two shapes and a needle catches one. The
/// key SET pins the row against any key added under any spelling — a bool
/// published as `is_transient` carries no value a byte scan can anchor on. The
/// byte scan pins the VALUE, which the set cannot see: a model published into
/// an existing field is a leak the shape holds still for. The set is the one
/// this fixture's state produces, which the positive control above fixes: the
/// two `skip_serializing_if` keys are absent exactly because this row is a
/// found seat that is neither Unknown nor prompt-blocked.
#[test]
fn a_published_row_carries_neither_the_seats_model_nor_its_transience() {
    let rig = Rig::new("publish-no-porter-intent");
    rig.write_config(&rig.one_seat_config_carrying_model_and_transient(rig.policy_path()));
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));

    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    // The positive control: the row is THIS seat's and the seat was FOUND. An
    // Unknown row carries neither key whatever the publisher does, so without
    // this the absences below would pass over a document that never reached the
    // shape a leak rides.
    let row = &rig.projection()["seats"][0];
    assert_eq!(row["seat_dir"], "builder-1");
    assert_eq!(row["chosen_name"], "Orla");
    assert_eq!(row["roster_state"], "present");

    let keys: BTreeSet<&str> = row
        .as_object()
        .expect("the published row is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "seat_dir",
            "chosen_name",
            "roster_state",
            "context_tokens",
            "project",
            "worktree",
            "decision",
            "outcome",
            "blind",
            "halted",
        ]),
        "the published row carries a key outside the published shape:\n{row}"
    );

    let body = rig.projection_body();
    // The control for the SCAN: three absences hold over any bytes that do not
    // carry the needles, an empty file included, so the bytes are tied to the
    // document the row above was read from before anything is said about what
    // they lack.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).expect("the body parses as JSON"),
        rig.projection(),
        "the scanned bytes are the published document"
    );
    for forbidden in ["\"model\"", "\"transient\"", "a-model"] {
        assert!(
            !body.contains(forbidden),
            "the published projection must not carry {forbidden}:\n{body}"
        );
    }
}

/// A seat at a permission dialog is a sixth state and not a healthy one, and
/// the reference publishes no context reading for it — which is what the
/// row-for-row parity between the two projections rests on.
#[test]
fn a_seat_at_a_permission_dialog_publishes_prompt_blocked_and_no_reading() {
    let rig = Rig::new("blocked");
    rig.write_roster(&blocked_row(
        &rig.worktree(),
        "a-session",
        "permission prompt",
    ));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );

    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "prompt-blocked");
    assert_eq!(row["waiting_for"], "permission prompt");
    assert!(
        row["context_tokens"].is_null(),
        "the reference reads no context for this state and parity is the acceptance"
    );

    // The control: the same row without the field is a healthy seat, and its
    // transcript IS read — so the null above is the state's, not the rig's.
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    assert_eq!(rig.observe().status.code(), Some(0));
    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "present");
    assert_eq!(row["context_tokens"], 18);
    assert!(row.get("waiting_for").is_none());
}

/// The transcript lives under the agent's configuration directory, which a
/// scoped daemon moves (lessons claude-code A11), at an encoding that touches
/// every non-alphanumeric character in the project path — here a worktree whose
/// last component carries a dot.
#[test]
fn a_scoped_config_directory_and_a_dotted_worktree_still_read_a_context() {
    let mut rig = Rig::with_leaf("scoped", "builder-1.wt");
    let scoped = rig.root.join("elsewhere").join("agent-state");
    rig.scoped_config_dir = Some(scoped.clone());
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.write_transcript_under(
        &scoped,
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":77}}}\n",
    );

    assert_eq!(rig.observe().status.code(), Some(0));
    assert_eq!(
        rig.projection()["seats"][0]["context_tokens"],
        77,
        "the transcript is read from the scoped directory, at the full encoding"
    );

    // The control: with no scoped directory the same transcript is not found,
    // so the reading above came from the scope and not from a default that
    // happened to hold it.
    rig.scoped_config_dir = None;
    assert_eq!(rig.observe().status.code(), Some(0));
    assert!(rig.projection()["seats"][0]["context_tokens"].is_null());
}

#[test]
fn an_unreadable_seat_list_refuses_at_startup_and_names_the_path() {
    let rig = Rig::new("no-seats");
    rig.write_config("{\"fleet_toml\": \"/somewhere\", \"children\": [");

    // The bare path: this poll refuses at startup and never reaches the
    // agent call, so no stub is spawned and the marker's absence is this
    // arm's premise.
    let out = rig.observe_unwitnessed();
    assert_eq!(
        out.status.code(),
        Some(3),
        "a seat list that will not parse has no last-good either"
    );
    let seats = rig.machine().join("config.json");
    assert!(
        stderr(&out).contains(&seats.display().to_string()),
        "the refusal names the path it could not read: {}",
        stderr(&out)
    );
    assert!(!rig.machine().join("projection.json").exists());
}

#[test]
fn an_absent_policy_file_refuses_at_startup_and_names_the_path() {
    let rig = Rig::new("no-policy");
    std::fs::remove_file(rig.policy_path()).unwrap();

    // The bare path: this poll refuses at startup and never reaches the
    // agent call, so no stub is spawned and the marker's absence is this
    // arm's premise.
    let out = rig.observe_unwitnessed();
    assert_eq!(out.status.code(), Some(3), "startup refuses without policy");
    assert!(
        stderr(&out).contains(&rig.policy_path().display().to_string()),
        "the refusal names the path it could not read: {}",
        stderr(&out)
    );
    assert!(
        !rig.machine().join("projection.json").exists(),
        "a refused startup publishes nothing"
    );
}

#[test]
fn a_policy_that_stops_parsing_keeps_last_good_and_says_so_in_the_projection() {
    let rig = Rig::new("last-good");
    rig.write_roster(&live_row(&rig.worktree(), "a-session"));
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );
    assert_eq!(rig.observe().status.code(), Some(0));
    let clean = rig.projection();
    assert!(clean.get("fleet_parse_error").is_none());

    // The loop is re-entered rather than continued, which is the harder case:
    // last-good has to come from the file, and a broken file has none.
    rig.write_policy("[controller\npoll_seconds = 1\n");
    // The bare path for THIS call only: it refuses at startup and never reaches
    // the agent, so no stub is spawned. The two polls either side of it run
    // normally and stay witnessed.
    let out = rig.observe_unwitnessed();
    assert_eq!(
        out.status.code(),
        Some(3),
        "a startup that cannot parse policy has no last-good and refuses"
    );

    rig.write_policy(
        "[controller]\npoll_seconds = 1\n\n[substrate.claude_code]\nversion = \"9.9.9\"\n",
    );
    assert_eq!(rig.observe().status.code(), Some(0));
    let restored = rig.projection();
    assert!(restored.get("fleet_parse_error").is_none());
    assert_eq!(restored["seats"][0]["context_tokens"], 18);
}

#[test]
fn an_unreadable_listing_is_unknown_for_every_seat_and_never_absent() {
    let rig = Rig::new("silent");
    rig.write_roster("");
    assert_eq!(rig.observe().status.code(), Some(0));
    let published = rig.projection();
    assert_eq!(published["seats"][0]["roster_state"], "unknown");
    assert!(published["seats"][0]["roster_unknown_cause"]
        .as_str()
        .is_some_and(|c| !c.is_empty()));
}

#[test]
fn a_row_with_no_worktree_is_skipped_loudly_and_never_defaulted() {
    let rig = Rig::new("skipped");
    rig.write_config(&format!(
        r#"{{"fleet_toml": "{}", "children": [{{"name":"builder-9"}}]}}"#,
        rig.policy_path().display()
    ));
    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stderr(&out).contains("builder-9"),
        "the skipped row is named: {}",
        stderr(&out)
    );
    assert_eq!(
        rig.projection()["seats"].as_array().map(Vec::len),
        Some(0),
        "a skipped row is not published as a seat"
    );
}
