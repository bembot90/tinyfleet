#!/usr/bin/env python3
"""Red-prove the controller's decision source against the fixtures that watch it.

Run: python3 fleet/tools/redprove.py   (exits non-zero if any mutation survives).
Every mutation is applied to the shipped file, the named fixture is run, and the
file is restored — so a decide arm whose test does not actually watch it is
reported rather than trusted.

For each rule: break the shipped source in the way that rule forbids, run the
named fixture, and require it to FAIL. A mutation that leaves the suite green
means the fixture does not watch the thing its name claims.

Two modes:

  --anchors   the classifier reads each canned cargo output below to the verdict
              it must get, and every mutation's `old` text still resolves in the
              source, exactly once. No mutation runs and no test runs, which is
              what makes it affordable as a gate: an anchor is exact source text,
              so a reformat orphans it and every mutation below it silently stops
              testing anything.
  (no flag)   the full table, by hand. Minutes, because each row is a cargo test.

WHAT --anchors DOES NOT CHECK: that a replacement still compiles, or that it
reddens its fixture. `new` is Rust inside a Python string literal and nothing
compiles it until this file runs in full, so a replacement naming a field whose
shape has changed goes stale with only the full run to announce it — the row
then reports INCONCLUSIVE rather than passing.
"""
import itertools
import os
import re
import shutil
import subprocess
import sys
import time

# /Volumes/WorkBear has 1-SECOND mtime granularity. Cargo fingerprints on mtime,
# so a mutation written and restored inside one second is invisible and cargo
# re-runs the CACHED binary, reporting a green that means nothing. Every write
# below bumps mtime forward past the granularity.
_tick = itertools.count(1)


def write_visibly(path, body):
    with open(path, "w") as handle:
        handle.write(body)
    future = time.time() + 10 * next(_tick)
    os.utime(path, (future, future))


# The fleet workspace, whose members the rows below name with -p.
WORKSPACE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# (rule, file, old, new, cargo args, test filter)
#
# One row per decide arm, one per outranking rule, one per blind-counter
# transition, and one per WIRING reading that has to reach the decision — a pure
# rule that is right and never read is a rule the fleet does not have.
MUTATIONS = [
    # ------------------------------------------------- the table's outranking
    ("R6   cannot-see is never treated as empty", "controller/src/decide.rs",
     "    if input.state == RosterState::Unknown {\n        return Verdict::LeaveAlone;\n    }",
     "    if input.state == RosterState::Unknown {\n        return Verdict::SpawnWoken;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "unknown_is_leave_alone_and_the_same_input_absent_is_a_spawn"),

    ("R12  the transient filter over every session-creating verdict",
     "controller/src/decide.rs",
     "    let verdict = decide_table(input);\n    if input.transient && verdict.creates_a_session() {\n        return Verdict::LeaveAlone;\n    }\n    verdict",
     "    decide_table(input)",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_transient_row_takes_no_session_creating_verdict_and_the_other_two_pass"),

    ("R12  revive is one of the verdicts that creates a session",
     "controller/src/decide.rs",
     "        matches!(self, Verdict::SpawnWoken | Verdict::Revive | Verdict::Rest)",
     "        matches!(self, Verdict::SpawnWoken | Verdict::Rest)",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_transient_row_takes_no_session_creating_verdict_and_the_other_two_pass"),

    ("R14  the halt guard outranks the spawn it guards",
     "controller/src/decide.rs",
     "    if input.halted {\n        return Verdict::Halt;\n    }",
     "    if input.halted && input.blind == u32::MAX {\n        return Verdict::Halt;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_halt_guard_outranks_the_spawn_it_guards"),

    ("R16  rest outranks spawn", "controller/src/decide.rs",
     "    if input.pending_rest && is_live(input.state) {\n        return Verdict::Rest;\n    }",
     "    if input.pending_rest && is_live(input.state) && input.blind == u32::MAX {\n        return Verdict::Rest;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "rest_outranks_spawn_and_spawn_outranks_suggest"),

    # The ORDER is what makes spawn outrank suggest — the spawn arms return
    # above the suggest one — so the mutation that inverts the rule is the
    # verdict on the arm that gets there first, not a guard inside the arm
    # below it.
    ("R21  spawn outranks suggest-rest", "controller/src/decide.rs",
     "    if input.state == RosterState::Absent {\n        return Verdict::SpawnWoken;\n    }",
     "    if input.state == RosterState::Absent {\n        return Verdict::SuggestRest;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "rest_outranks_spawn_and_spawn_outranks_suggest"),

    # And the guard inside that arm holds one reading of its own: every state
    # that reaches it is live except a STARTING row, which a guard-less arm
    # would nudge before it had finished coming up.
    ("R21  a suggestion needs a live row", "controller/src/decide.rs",
     "    if is_live(input.state) && !input.already_nudged {",
     "    if !input.already_nudged {",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_starting_row_is_left_alone_and_the_same_seat_absent_is_spawned"),

    # ------------------------------------------------------- the arrival hold
    ("R13  a dispatch inside its window is not re-issued",
     "controller/src/decide.rs",
     "    if !input.sighted && arrival_window_open(input) {",
     "    if input.sighted && arrival_window_open(input) {",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_dispatch_inside_its_window_with_no_sighting_holds_the_seat"),

    ("R13  the window is keyed to the DISPATCH and closes at its edge",
     "controller/src/decide.rs",
     "        Some(age) => age < input.arrival_window_ms,",
     "        Some(age) => age < input.arrival_window_ms * 2,",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_dispatch_inside_its_window_with_no_sighting_holds_the_seat"),

    # -------------------------------------------------- the replacement window
    ("R11  a pid-less row is held while the daemon is replaced",
     "controller/src/decide.rs",
     "    if hold(input).is_some() {\n        return Verdict::LeaveAlone;\n    }",
     "    if hold(input).is_some() && input.blind == u32::MAX {\n        return Verdict::LeaveAlone;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_pidless_row_is_held_while_the_daemon_is_being_replaced"),

    ("R11  either reading opens the window on its own",
     "controller/src/decide.rs",
     "    input.daemon_pid_changed\n        || matches!(input.daemon_uptime_ms, Some(up) if up < input.arrival_window_ms)",
     "    input.daemon_pid_changed\n        && matches!(input.daemon_uptime_ms, Some(up) if up < input.arrival_window_ms)",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_pidless_row_is_held_while_the_daemon_is_being_replaced"),

    # The state guard is spelled the same in both holds, so each anchor carries
    # the comment line above its own: an anchor that matches twice pins neither.
    ("R11  the hold needs a ROW, and an Absent seat with one is held",
     "controller/src/decide.rs",
     "    // here would put a replacement's name on a newborn nothing is waiting on.\n    if !matches!(input.state, RosterState::Stopped | RosterState::Absent) {\n        return None;\n    }",
     "    // here would put a replacement's name on a newborn nothing is waiting on.\n    if !matches!(input.state, RosterState::Stopped) {\n        return None;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_replacement_hold_sits_under_rest_and_over_the_spawn_arms"),

    ("R11  a seat standing on NO row is not held",
     "controller/src/decide.rs",
     "    if !input.pidless_row || !replacement_window_open(input) {\n        return None;\n    }",
     "    if !replacement_window_open(input) {\n        return None;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_replacement_hold_sits_under_rest_and_over_the_spawn_arms"),

    # ------------------------------------------------------ the re-host window
    # The daemon stands unchanged while ONE session's host is replaced, so the
    # hold below reads this controller's own earlier sighting and nothing the
    # daemon can answer for.
    ("R11  a row seen live under this controller is held while its host moves",
     "controller/src/decide.rs",
     "    let since = input.since_pidless_ms?;\n    if since >= input.arrival_window_ms {\n        return None;\n    }",
     "    let since = input.since_pidless_ms?;\n    if since >= u64::MAX {\n        return None;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_row_seen_live_under_this_controller_is_held_while_its_host_is_replaced"),

    ("R11  a seat this controller never saw live is not held",
     "controller/src/decide.rs",
     "    if !input.seen_live {\n        return None;\n    }",
     "    if input.seen_live {\n        return None;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_row_seen_live_under_this_controller_is_held_while_its_host_is_replaced"),

    # The window is spent on the PID-LESS RUN and not on the last sighting: the
    # polls a run's land step separates are minutes apart, so the two readings
    # are different numbers on exactly the poll the hold exists for
    # — the poll after a land step.
    ("R11  the re-host window is measured from the row and not from the sighting",
     "controller/src/run.rs",
     "                since_pidless_ms: self\n                    .pidless_since",
     "                since_pidless_ms: self\n                    .live_seen",
     ["-p", "fleet-controller", "--test", "blocked_poll"],
     "a_fleet_that_went_pid_less_inside_a_runs_land_step_is_held_on_the_poll_after_it"),

    ("R11  a deliberate end is not held for a re-host",
     "controller/src/decide.rs",
     "    if input.pending_deliberate_end {\n        return None;\n    }",
     "    if !input.pending_deliberate_end {\n        return None;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_row_seen_live_under_this_controller_is_held_while_its_host_is_replaced"),

    ("R11  the daemon's hold is the one a seat inside both windows reports",
     "controller/src/decide.rs",
     "    replacement_hold(input).or_else(|| rehost_hold(input))",
     "    rehost_hold(input).or_else(|| replacement_hold(input))",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_held_seats_line_names_which_hold_and_the_daemons_is_the_one_read_first"),

    # ---------------------------------------------------- the discriminator
    ("R10  a deliberate end takes the successor arm",
     "controller/src/decide.rs",
     "        if input.pending_deliberate_end {\n            return Verdict::SpawnWoken;\n        }",
     "        if !input.pending_deliberate_end {\n            return Verdict::SpawnWoken;\n        }",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_stopped_row_is_read_by_its_end_event_and_then_by_its_context"),

    ("R10  the context guard splits at the threshold",
     "controller/src/decide.rs",
     "            Some(tokens) if tokens < input.rest_threshold_tokens => Verdict::Revive,",
     "            Some(tokens) if tokens < u64::MAX => Verdict::Revive,",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_stopped_row_is_read_by_its_end_event_and_then_by_its_context"),

    ("R10  an unmeasurable context is not a licence to revive",
     "controller/src/decide.rs",
     "            Some(tokens) if tokens < input.rest_threshold_tokens => Verdict::Revive,\n            _ => Verdict::SpawnWoken,",
     "            Some(tokens) if tokens < input.rest_threshold_tokens => Verdict::Revive,\n            _ => Verdict::Revive,",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_stopped_row_is_read_by_its_end_event_and_then_by_its_context"),

    ("R10  a row with no session at all is spawned for",
     "controller/src/decide.rs",
     "    if input.state == RosterState::Absent {\n        return Verdict::SpawnWoken;\n    }",
     "    if input.state == RosterState::Absent {\n        return Verdict::LeaveAlone;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "a_starting_row_is_left_alone_and_the_same_seat_absent_is_spawned"),

    # ------------------------------------------------------ the blind counter
    ("R14  a sighting DECAYS the counter and never clears it",
     "controller/src/decide.rs",
     "        RosterState::Present | RosterState::PromptBlocked => previous.saturating_sub(1),",
     "        RosterState::Present | RosterState::PromptBlocked => 0,",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_blind_counter_decays_on_a_sighting_and_climbs_on_a_dispatch"),

    ("R14  an unreadable roster holds the counter",
     "controller/src/decide.rs",
     "        RosterState::Unknown => previous,",
     "        RosterState::Unknown => previous.saturating_sub(1),",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_blind_counter_decays_on_a_sighting_and_climbs_on_a_dispatch"),

    ("R14  a starting row moves the counter in no direction",
     "controller/src/decide.rs",
     "        RosterState::Starting => previous,",
     "        RosterState::Starting => previous + 1,",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_blind_counter_decays_on_a_sighting_and_climbs_on_a_dispatch"),

    ("R14  the counter is LATCHED at the limit",
     "controller/src/decide.rs",
     "    if previous >= BLIND_LIMIT {\n        return previous;\n    }",
     "    if previous > u32::MAX - 1 {\n        return previous;\n    }",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_blind_counter_decays_on_a_sighting_and_climbs_on_a_dispatch"),

    ("R14  only a session-creating verdict counts as a dispatch",
     "controller/src/decide.rs",
     "            if verdict.creates_a_session() {\n                previous + 1",
     "            if !verdict.creates_a_session() {\n                previous + 1",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_blind_counter_decays_on_a_sighting_and_climbs_on_a_dispatch"),

    # ------------------------------------------------------ the upgrade shape
    ("R15  the upgrade shape has a floor of two",
     "controller/src/decide.rs",
     "    if pidless >= UPGRADE_SHAPE_FLOOR && pidless * 2 >= total {",
     "    if pidless * 2 >= total {",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_upgrade_shape_is_half_the_fleet_and_never_fewer_than_two"),

    ("R15  and it is half the fleet, not any of it",
     "controller/src/decide.rs",
     "    if pidless >= UPGRADE_SHAPE_FLOOR && pidless * 2 >= total {",
     "    if pidless >= UPGRADE_SHAPE_FLOOR {",
     ["-p", "fleet-controller", "--test", "decide"],
     "the_upgrade_shape_is_half_the_fleet_and_never_fewer_than_two"),

    # ------------------------------------------------------------ the wiring
    #
    # A rule that is right and never READ is a rule the fleet does not have, so
    # each reading that has to reach the decision gets a mutant that empties it.
    ("wiring: the daemon reading reaches the verdict",
     "controller/src/run.rs",
     "                daemon_uptime_ms: daemon.uptime_ms(),",
     "                daemon_uptime_ms: None,",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::a_pidless_row_is_held_while_the_stub_daemon_is_young"),

    ("wiring: the blind counter is persisted across polls",
     "controller/src/run.rs",
     "                    self.table\n"
     "                        .set_seat_state(&seat.name, SeatState { blind, halted });",
     "                    self.table.set_seat_state(&seat.name, SeatState::default());",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::three_blind_dispatches_halt_the_seat_and_a_clear_halt_lifts_it"),

    ("wiring: the clear-halt request is consumed on the tick",
     "controller/src/run.rs",
     "                .map(|asked| asked.clear_halt)",
     "                .map(|asked| asked.clear_halt && asked.rest)",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::three_blind_dispatches_halt_the_seat_and_a_clear_halt_lifts_it"),

    ("wiring: revive's effect is carried out",
     "controller/src/run.rs",
     "                Verdict::Revive => effect::revive(agent, &target, events_log, table, now_ms),",
     "                Verdict::Revive => Outcome::None,",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::a_revive_attaches_the_rows_short_id_and_says_so_once"),

    ("wiring: adoption runs at startup",
     "controller/src/run.rs",
     "                let claimed = effect::adopt(&rows, &mut self.table, &mut self.events_log, now_ms);",
     "                let claimed: Vec<String> = { let _ = rows; Vec::new() };",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::a_restart_adopts_the_live_sessions_and_a_lost_table_is_rebuilt"),

    ("wiring: a lost table is rebuilt from the stream",
     "controller/src/run.rs",
     "            let rebuilt = sessions::rebuild(&machine_dir.join(\"events.jsonl\"));",
     "            let rebuilt = Table::default();",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::a_rebuilt_table_carries_a_standing_halt_and_no_daemon_pid"),

    # An ABSENT table reaches the rebuild by a different path from an unreadable
    # one — a read that found nothing rather than one that failed — so it gets
    # its own mutant and its own fixture. Putting the old `(Table::default(),
    # None)` back is the defect itself: the controller starts empty on exactly
    # the loss the halt latch exists to survive.
    ("wiring: an ABSENT table is a table to rebuild, not an empty fleet",
     "controller/src/sessions.rs",
     "        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None),",
     "        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {\n            return (Some(Table::default()), None)\n        }",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::a_deleted_table_is_rebuilt_from_the_stream_and_the_halt_survives"),

    ("wiring: the halt latch reaches the verdict",
     "controller/src/run.rs",
     "                halted: carried.halted,",
     "                halted: false,",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::three_blind_dispatches_halt_the_seat_and_a_clear_halt_lifts_it"),

    # --------------------------------------------------- adoption, once each
    #
    # A spawned line carries no session id, so an adoption folded as a new row
    # is a second row for one session; and adoption is once per session, which
    # a `--once` poll — a process per poll — is the reading that shows.
    ("R17  a rebuilt adoption is a sighting, not a new row",
     "controller/src/sessions.rs",
     "            crate::events::SESSION_SPAWNED => table.push(opened()),",
     "            crate::events::SESSION_SPAWNED | crate::events::SESSION_ADOPTED => table.push(opened()),",
     ["-p", "fleet-controller", "--lib"],
     "sessions::tests::a_spawned_then_adopted_session_rebuilds_to_one_row"),

    ("R25  a session is adopted once, not once per call",
     "controller/src/effect.rs",
     "        if row.adopted.as_ref() == Some(&session_id) {\n            continue;\n        }\n",
     "",
     ["-p", "fleet-controller", "--test", "effects"],
     "a_second_adopt_over_the_same_table_claims_nothing"),

    ("R25  and a second --once poll adopts nothing",
     "controller/src/effect.rs",
     "        if row.adopted.as_ref() == Some(&session_id) {\n            continue;\n        }\n",
     "",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::a_second_restart_does_not_adopt_the_session_again"),

    # ------------------------------------------------------------ the stream
    ("R25  a sequence is taken under the stream's lock",
     "controller/src/events.rs",
     "        file.lock()?;\n",
     "",
     ["-p", "fleet-controller", "--lib"],
     "events::tests::two_writers_racing_on_one_stream_never_share_a_sequence"),

    # ----------------------------------------------------------- the effects
    #
    # The drive fixture's roster row carries one value as its id and its session
    # id, so only the lesson fixture, whose two differ, can tell them apart.
    ("R10  a revive attaches the ADDRESS, never the identity",
     "controller/src/effect.rs",
     "    let attached = agent.revive(target.config_dir(), short_id);",
     "    let attached = agent.revive(target.config_dir(), session_id);",
     ["-p", "fleet-controller", "--test", "effects"],
     "lessons::resume_continues_only_a_flagless_full_id"),

    ("R17  an adoption writes one line per claimed session",
     "controller/src/effect.rs",
     "        append(\n            events_log,\n            events::SESSION_ADOPTED,",
     "        append(events_log, events::SESSION_ADOPTED, &row.seat, serde_json::json!({ \"session\": session_id }));\n        append(\n            events_log,\n            events::SESSION_ADOPTED,",
     ["-p", "fleet-controller", "--test", "effects"],
     "lessons::a_restart_adopts_and_says_so"),

    ("R14  a halt transition writes one event",
     "controller/src/effect.rs",
     "    append(\n        events_log,\n        events::SESSION_HALTED,",
     "    append(events_log, events::SESSION_HALTED, seat_dir, serde_json::json!({ \"blind\": blind }));\n    append(\n        events_log,\n        events::SESSION_HALTED,",
     ["-p", "fleet-controller", "--test", "effects"],
     "lessons::a_hold_persists_and_is_announced"),

    # ------------------------------------------------------- the cli's exits
    ("R14  clear-halt into a fleet nobody collects exits 5",
     "controller/src/seat.rs",
     "pub const EXIT_NO_COLLECTOR: u8 = 5;",
     "pub const EXIT_NO_COLLECTOR: u8 = 1;",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::clear_halt_refuses_with_no_collector_and_for_a_seat_that_is_not_halted"),

    ("R14  clear-halt for a seat that is not halted exits 1",
     "controller/src/seat.rs",
     "pub const EXIT_NOT_HALTED: u8 = 1;",
     "pub const EXIT_NOT_HALTED: u8 = 5;",
     ["-p", "fleet-cli", "--test", "drive"],
     "effects::clear_halt_refuses_with_no_collector_and_for_a_seat_that_is_not_halted"),

    # ------------------------------------------------------- the landing lane
    ("R15  land takes the lane before it touches the trunk",
     "core/src/item/land.rs",
     "    let _lane = lane::take(\n        out,\n        wiring.progress,\n"
     "        &lane::directory(\n            landing.machine_dir,\n"
     "            &wiring.project.guards,\n            &wiring.project.name,\n"
     "        )?,\n        landing.item,\n        landing.at,\n    )?;\n",
     "",
     ["-p", "fleet-core", "--test", "land"],
     "a_landing_waits_on_a_held_lane_and_lands_after_it_is_released"),

    ("R18  a red gate is rerun once before it refuses",
     "core/src/item/land.rs",
     "    if first.green() {\n        return Ok(vec![first]);\n    }",
     "    if !first.green() {\n        return Ok(vec![first]);\n    }",
     ["-p", "fleet-core", "--test", "land"],
     "a_red_gate_is_rerun_once_and_a_green_second_reading_lands_with_both_rows"),

    ("R18  the rerun's wait expires and reruns anyway, saying so",
     "core/src/item/lane.rs",
     "            return Waited::Expired(format!(",
     "            return Waited::Quiet(format!(",
     ["-p", "fleet-core", "--test", "land"],
     "a_wait_that_expires_reruns_anyway_and_the_row_says_it_expired"),
]


# The verdicts one cargo run can reach. Only RED starts with "RED", which is the
# prefix the full table's tally counts as a proof.
RED = "RED (the named fixture failed)"
GREEN = "STAYED GREEN -- the fixture does not watch this"
NO_TEST_RAN = "NO TEST RAN (the filter matched none)"
FIXTURE_MISSING = "FIXTURE MISSING (tests ran, not this one)"
OTHER_FAILED = "NOT RED (a test other than the named one failed)"
CARGO_FAILED = "CARGO FAILED (no test result reported)"
INCONCLUSIVE = "INCONCLUSIVE (did not compile)"

RESULT_LINE = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed;", re.M)


def verdict(code, out, test_filter):
    """What one cargo run says about one row.

    RED only when cargo's own `test result:` line reports a failure AND the
    failure is the named fixture's; every other outcome is its own verdict.
    """
    results = [(int(p), int(f)) for p, f in RESULT_LINE.findall(out)]
    lines = out.splitlines()
    if not results:
        # NOT red. A mutation that does not build never reached the fixture, so
        # the run says nothing about whether the fixture watches the rule.
        if "could not compile" in out or "error[E" in out:
            return INCONCLUSIVE
        return CARGO_FAILED
    passed = sum(p for p, _ in results)
    failed = sum(f for _, f in results)
    if failed:
        return RED if f"test {test_filter} ... FAILED" in lines else OTHER_FAILED
    if code != 0:
        return CARGO_FAILED
    if passed == 0:
        return NO_TEST_RAN
    if f"test {test_filter} ... ok" not in lines:
        return FIXTURE_MISSING
    return GREEN


# (case, exit code, cargo output, the verdict it must get), read by --anchors so
# the gate that resolves the anchors holds the classifier to these too.
CANNED_FILTER = "effects::the_named_fixture"
CANNED = [
    ("the named fixture failed", 101,
     "running 1 test\ntest effects::the_named_fixture ... FAILED\n\nfailures:\n\n"
     "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 9 filtered out\n",
     RED),
    ("a cargo-level failure", 101,
     "error: no test target named `absent` in `fleet-cli` package\n",
     CARGO_FAILED),
    ("no test ran", 0,
     "running 0 tests\n\n"
     "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out\n",
     NO_TEST_RAN),
    ("a compile failure", 101,
     "error[E0425]: cannot find value `short` in this scope\n"
     "error: could not compile `fleet-controller` (lib) due to 1 previous error\n",
     INCONCLUSIVE),
    ("a different test failed", 101,
     "running 1 test\ntest effects::another_fixture ... FAILED\n\nfailures:\n\n"
     "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 9 filtered out\n",
     OTHER_FAILED),
    ("the named fixture passed", 0,
     "running 1 test\ntest effects::the_named_fixture ... ok\n\n"
     "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n",
     GREEN),
    ("a different test passed", 0,
     "running 1 test\ntest effects::another_fixture ... ok\n\n"
     "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out\n",
     FIXTURE_MISSING),
]


def classifier_arm():
    wrong = 0
    for case, code, out, want in CANNED:
        got = verdict(code, out, CANNED_FILTER)
        if got != want:
            wrong += 1
            print(f"classifier: {case} reads {got!r}, not {want!r}")
    print(f"{len(CANNED) - wrong}/{len(CANNED)} canned cargo outputs classified")
    return 1 if wrong else 0


def anchor_state(relpath, old):
    """Whether this row's `old` resolves in the source, exactly once.

    An anchor is exact source text and formatting is a gated step, so a reformat
    orphans anchors and every mutation below them stops testing anything. This
    resolver runs no mutation, which is what lets a gate afford it.
    """
    path = os.path.join(WORKSPACE, relpath)
    try:
        with open(path) as handle:
            src = handle.read()
    except OSError as e:
        return f"FILE-UNREADABLE ({e.strerror})"
    found = src.count(old)
    if found == 0:
        return "ANCHOR-MISSING"
    if found > 1:
        return f"ANCHOR-AMBIGUOUS x{found}"
    return "anchored"


def anchors_arm():
    rows = [(rule, anchor_state(path, old), path)
            for rule, path, old, _new, _args, _filt in MUTATIONS]
    bad = [row for row in rows if row[1] != "anchored"]
    for rule, state, path in bad:
        print(f"{rule:55} {state:25} {path}")
    print(f"{len(rows) - len(bad)}/{len(rows)} anchors resolve")
    return 1 if bad else 0


def cargo():
    """The build tool, by an absolute path when this process's own PATH lacks it.

    $CARGO first, then the search path, then the default install location: a
    restricted or service-launched process carries a PATH that is not the
    operator's shell, and a table that reported every row INCONCLUSIVE because it
    could not find cargo would read as a source nobody had proved.
    """
    named = os.environ.get("CARGO")
    if named:
        return named
    found = shutil.which("cargo")
    if found:
        return found
    return os.path.join(os.path.expanduser("~"), ".cargo", "bin", "cargo")


def run(args, test_filter):
    cmd = [cargo(), "test"] + args + ["--", test_filter, "--exact"]
    try:
        done = subprocess.run(cmd, cwd=WORKSPACE, capture_output=True, text=True)
    except OSError as e:
        print(f"CANNOT JUDGE — cargo could not be run: {e}", file=sys.stderr)
        return None
    return done.returncode, done.stdout + done.stderr


def full_table():
    results = []
    for rule, relpath, old, new, args, test_filter in MUTATIONS:
        state = anchor_state(relpath, old)
        if state != "anchored":
            results.append((rule, state, relpath))
            continue
        path = os.path.join(WORKSPACE, relpath)
        with open(path) as handle:
            original = handle.read()
        try:
            write_visibly(path, original.replace(old, new, 1))
            answered = run(args, test_filter)
            if answered is None:
                # A gate arm has three answers, not two: a missing toolchain and
                # a mutation that survived must not reach the same row.
                write_visibly(path, original)
                return 2
            code, out = answered
            results.append((rule, verdict(code, out, test_filter), relpath))
        finally:
            write_visibly(path, original)

    print(f"{'RULE':55} {'RESULT':45} FILE")
    bad = 0
    for rule, result, path in results:
        print(f"{rule:55} {result:45} {path}")
        if not result.startswith("RED"):
            bad += 1
    print(f"\n{len(results) - bad}/{len(results)} mutations went red")
    return 1 if bad else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--anchors"]:
        sys.exit(max(classifier_arm(), anchors_arm()))
    # 2, never 1: a gate must not read "you called me wrong" as "an anchor is
    # orphaned".
    if sys.argv[1:]:
        print(f"usage: {os.path.basename(__file__)} [--anchors]", file=sys.stderr)
        sys.exit(2)
    sys.exit(full_table())
