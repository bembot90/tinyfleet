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

/// A DEAD PANE is a stopped seat (ruling 3): the host keeps the pane with the
/// status its agent exited with, the listing names nothing — the row went with
/// the process (the claude-code pack's lessons B10) — and the session the table
/// recorded still answers for context, because its transcript outlives it (C4).
///
/// Each `observe --once` is a controller's FIRST poll, which did not see the
/// pane die, so the one `session.ended` it writes is dated by the transcript;
/// the second poll finds the end latched and writes none. There is no stopped
/// window left to age the row out: it reads stopped for as long as the pane is
/// kept.
#[test]
fn a_dead_pane_reads_stopped_with_its_status_its_end_and_its_context() {
    let rig = Rig::new("dead-pane");
    rig.a_dead_session("a-session", Some(3));
    // Held down, so the poll decides nothing for the seat and the next one
    // reads the same dead pane: what is measured here is what a poll READS. A
    // revive would replace the pane, and that is the effects suite's.
    rig.hold_the_seat();
    rig.write_transcript(
        "a-session",
        "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":18}}}\n",
    );

    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let row = &rig.projection()["seats"][0];
    assert_eq!(row["roster_state"], "stopped", "{row}");
    assert_eq!(row["exit_status"], 3, "{row}");
    assert_eq!(
        row["context_tokens"], 18,
        "the stopped row still answers for context: {row}"
    );
    let ends: Vec<serde_json::Value> = rig
        .events()
        .into_iter()
        .filter(|e| e["type"] == "session.ended")
        .collect();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0]["payload"]["session"], "a-session");
    assert_eq!(ends[0]["payload"]["status"], 3);
    assert_eq!(ends[0]["payload"]["source"], "transcript");
    assert!(ends[0]["payload"]["at"].is_string(), "{}", ends[0]);
    assert_eq!(row["ended_at"], ends[0]["payload"]["at"], "{row}");

    // The next poll reads the same dead pane and writes no second end.
    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(rig.events_of("session.ended"), 1);
    assert_eq!(rig.projection()["seats"][0]["roster_state"], "stopped");
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
    assert_eq!(
        published["seats"][0]["seat"],
        serde_json::json!({ "id": SEAT_ID, "name": "Orla", "kind": "agent" }),
        "the row names the seat as its id, its own name and its kind: {}",
        published["seats"][0]
    );
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
/// two cause keys are absent exactly because this row is a found seat that is
/// neither Unknown nor prompt-blocked. The seat object's own keys are pinned the
/// same way, so a model riding in beside the name is a red too.
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
    assert_eq!(row["seat"]["id"], SEAT_ID);
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
            "seat",
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
    let seat_keys: BTreeSet<&str> = row["seat"]
        .as_object()
        .expect("the row's seat is an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        seat_keys,
        BTreeSet::from(["id", "name", "kind"]),
        "the seat object carries a key outside its shape:\n{row}"
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

#[test]
fn an_unreadable_seat_list_refuses_at_startup_and_names_the_path() {
    let rig = Rig::new("no-seats");
    rig.write_config("{\"fleet_toml\": \"/somewhere\", \"children\": [");

    // This poll refuses at startup and never reaches the agent.
    let out = rig.observe();
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

    // This poll refuses at startup and never reaches the agent.
    let out = rig.observe();
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
    // THIS call refuses at startup and never reaches the agent; the two polls
    // either side of it run normally.
    let out = rig.observe();
    assert_eq!(
        out.status.code(),
        Some(3),
        "a startup that cannot parse policy has no last-good and refuses"
    );

    rig.write_policy("[controller]\npoll_seconds = 1\n");
    assert_eq!(rig.observe().status.code(), Some(0));
    let restored = rig.projection();
    assert!(restored.get("fleet_parse_error").is_none());
    assert_eq!(restored["seats"][0]["context_tokens"], 18);
}

/// A seat whose pane the host holds alive, beside a listing that answered
/// zero bytes (the claude-code pack's lessons B4): unknown, with the cause,
/// and never present or absent. The pane is what makes the listing the
/// question — a seat the host holds nothing for is absent on the host's own
/// reading, and its agent is not asked about it.
#[test]
fn an_unreadable_listing_is_unknown_for_every_seat_and_never_absent() {
    let rig = Rig::new("silent");
    common::live_pane(&rig.tmux_state_path(), SEAT_ID, &rig.worktree());
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
        r#"{{"fleet_toml": "{}", "children": [
             {{"id":"01a0d1f1-0aec-765f-9abe-5c21e8a04b17","name":"Pell"}}
           ]}}"#,
        rig.policy_path().display()
    ));
    let out = rig.observe();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        stderr(&out).contains("pell-e8a04b17 carries no worktrees entry"),
        "the skipped row is named: {}",
        stderr(&out)
    );
    assert_eq!(
        rig.projection()["seats"].as_array().map(Vec::len),
        Some(0),
        "a skipped row is not published as a seat"
    );
}

/// `common::stub_agent`, the one line a suite outside this family puts a fleet
/// on the agent stub with: the `[agent]` key after the fleet's own policy and
/// the stub's state reset to its defaults. A poll of that fleet reaches the
/// stub through the Exec — the version it publishes is the stub's own, the
/// adapter it names is the stub's path, and the stub logged the poll's read.
#[test]
fn a_fleet_the_common_helper_puts_on_the_agent_stub_polls_through_it() {
    let rig = Rig::new("common-helper");
    write(&rig.policy_path(), "[controller]\npoll_seconds = 1\n");
    let stub = common::stub_agent(&rig.root);
    // A live pane, so the poll asks the agent about the seat.
    common::live_pane(&rig.tmux_state_path(), SEAT_ID, &rig.worktree());

    assert_eq!(rig.observe().status.code(), Some(0));
    let published = rig.projection();
    assert_eq!(
        published["agent_version"],
        StubAgent::VERSION,
        "{published}"
    );
    assert_eq!(
        published["agent"]["adapter"],
        serde_json::json!(stub.display().to_string()),
        "{published}"
    );
    assert!(
        agent_stub::verbs(&rig.root).contains(&StubAgent::READ),
        "the poll's read reached the stub: {:?}",
        agent_stub::verbs(&rig.root)
    );
}
