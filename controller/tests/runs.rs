//! The run lifecycle's controller half (controller PRD R35–R37).
//!
//! Every arm here drives the pass against a real stream file in a temporary
//! directory and a stub for the three acts. The stub WRITES WHAT THE REAL ACT
//! WOULD WRITE — `run.started` and one row of the exit table for a re-run,
//! `session.retired` for a retire — because what these arms measure is the
//! DECISION, and a decision is only visible in what the stream says afterwards.
//!
//! WHY A STUB AND NOT A SCRATCH WORKFLOW. The three acts each resolve a project:
//! its store, its packs, its policy file, its primary checkout. That resolution
//! is the binary's and the controller crate cannot make it — which is why the
//! acts are a seam at all — so an arm here that shelled a real script would be
//! measuring the binary's wiring through a crate that does not have it.

use fleet_controller::events::{self, EventLog};
use fleet_controller::runs::{self, Pass, Runs};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

// ---- the rig -----------------------------------------------------------------

/// A directory of this process's own, removed when the arm ends.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "fleet-runs-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch directory is made");
        Scratch { root }
    }

    fn stream(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// How one scripted execution ends — the row of the exit table the stub's
/// re-run answers on.
#[derive(Clone, Copy)]
enum Ends {
    Closed,
    /// Waiting, recording the stream's position as the child left it.
    Waiting,
    CouldNotTell,
}

/// The three acts, recorded, writing what the real ones write.
struct Stub {
    stream: PathBuf,
    /// One entry per execution the stub will be asked for, in order. An empty
    /// script is a seam that refuses, which is how an arm proves no further
    /// re-run was asked for even where it forgot to count the calls.
    script: RefCell<VecDeque<Ends>>,
    reruns: RefCell<Vec<String>>,
    gates: RefCell<Vec<(String, String)>>,
    retires: RefCell<Vec<(String, String)>>,
}

impl Stub {
    fn with(stream: &Path, script: &[Ends]) -> Stub {
        Stub {
            stream: stream.to_path_buf(),
            script: RefCell::new(script.iter().copied().collect()),
            reruns: RefCell::new(Vec::new()),
            gates: RefCell::new(Vec::new()),
            retires: RefCell::new(Vec::new()),
        }
    }

    fn log(&self) -> EventLog {
        EventLog::open(&self.stream)
    }
}

impl Runs for Stub {
    fn rerun(&self, run: &str) -> Result<(), String> {
        let ends = self
            .script
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| format!("the script has no execution left for {run}"))?;
        self.reruns.borrow_mut().push(run.to_string());
        let mut log = self.log();
        log.append(
            runs::RUN_STARTED,
            "a-runner",
            serde_json::json!({ "run": run, "hash": "abc", "workflow": "w" }),
        )
        .expect("the stream takes the start");
        // THE POSITION IS READ BEFORE THE LINE THAT CARRIES IT, exactly as the
        // back half reads it: `read_the_exit` takes `stream.seq()` and the
        // append that follows lands above it. An arm that recorded the line's
        // own sequence would be measuring a stream core never writes.
        let at_exit = self.log().seq();
        let (kind, payload) = match ends {
            Ends::Closed => (runs::RUN_CLOSED, serde_json::json!({ "run": run })),
            Ends::Waiting => (
                runs::RUN_WAITING,
                serde_json::json!({ "run": run, "wake": { "for": "a line" }, "seq": at_exit }),
            ),
            Ends::CouldNotTell => (
                runs::RUN_COULD_NOT_TELL,
                serde_json::json!({ "run": run, "exit": 7, "read": serde_json::Value::Null }),
            ),
        };
        log.append(kind, "a-runner", payload)
            .expect("the stream takes the outcome");
        Ok(())
    }

    fn gate(&self, run: &str, reason: &str) -> Result<String, String> {
        self.gates
            .borrow_mut()
            .push((run.to_string(), reason.to_string()));
        Ok(format!("gate-for-{run}"))
    }

    fn retire(&self, seat: &str, run: &str) -> Result<(), String> {
        self.retires
            .borrow_mut()
            .push((seat.to_string(), run.to_string()));
        self.log()
            .append(
                events::SESSION_RETIRED,
                seat,
                serde_json::json!({ "seat": seat, "item": run }),
            )
            .expect("the stream takes the retirement");
        Ok(())
    }
}

/// One pass, with the cap the arm names.
fn pass(stub: &Stub, stream: &Path, max_crashes: u64) -> Result<(), String> {
    let mut log = EventLog::open(stream);
    let mut pass = Pass {
        runs: stub,
        events: &mut log,
        stream,
        max_crashes,
    };
    runs::tick(&mut pass)
}

fn count(stream: &Path, kind: &str) -> usize {
    events::read_after(stream, 0)
        .iter()
        .filter(|record| record.kind == kind)
        .count()
}

fn of_kind(stream: &Path, kind: &str) -> Vec<events::Record> {
    events::read_after(stream, 0)
        .into_iter()
        .filter(|record| record.kind == kind)
        .collect()
}

/// The stream as one run that has been opened and has stopped waiting.
///
/// ITS WAKE NAMES NO ITEMS, which is the shape a workflow that threw its own
/// condition leaves — so every arm built on this fixture drives the fallback:
/// any move wakes it.
fn a_waiting_run(stream: &Path, run: &str) {
    let mut log = EventLog::open(stream);
    log.append(
        runs::RUN_STARTED,
        "a-runner",
        serde_json::json!({ "run": run, "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    let at_exit = EventLog::open(stream).seq();
    log.append(
        runs::RUN_WAITING,
        "a-runner",
        serde_json::json!({ "run": run, "wake": { "for": "a line" }, "seq": at_exit }),
    )
    .expect("the wait lands");
}

/// Somebody else's line on the stream — the move a waiting run is woken by.
fn a_line_from_elsewhere(stream: &Path) {
    EventLog::open(stream)
        .append(events::SEAT_WOKE, "s1", serde_json::json!({}))
        .expect("the line lands");
}

/// The stream as one run that has been opened and has stopped on the wake it is
/// handed — the wrapper's `{"waiting": <condition>}`, whole, as the back half
/// stores it. A `null` stands for the payload that carries no wake key at all:
/// `get` answers the same for both.
fn a_run_waiting_with(stream: &Path, run: &str, wake: serde_json::Value) {
    let mut log = EventLog::open(stream);
    log.append(
        runs::RUN_STARTED,
        "a-runner",
        serde_json::json!({ "run": run, "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    let at_exit = EventLog::open(stream).seq();
    log.append(
        runs::RUN_WAITING,
        "a-runner",
        serde_json::json!({ "run": run, "wake": wake, "seq": at_exit }),
    )
    .expect("the wait lands");
}

/// One item's state, announced by whoever moved it.
fn an_item_moved(stream: &Path, kind: &str, item: &str) {
    EventLog::open(stream)
        .append(kind, "s1", serde_json::json!({ "item": item }))
        .expect("the line lands");
}

// ---- AC1: the waiting run re-runs, and not before ------------------------------

/// A waiting run is re-run only once the stream has moved past where it
/// stopped — one `run.started` before the appended line and two after.
///
/// THE FIRST HALF IS THE ARM. The position `run.waiting` records is the stream
/// as the child left it, and the line carrying it is appended ABOVE that
/// position — so a pass comparing the head against the payload alone finds it
/// already exceeded and re-runs every waiting run on its very next poll. The
/// "not before" assertion is what catches that, and it is the reason the
/// comparison takes the higher of the payload's position and the line's own.
#[test]
fn a_waiting_run_re_runs_only_after_the_stream_moves_past_it() {
    let scratch = Scratch::new("ac1");
    let stream = scratch.stream();
    a_waiting_run(&stream, "r1");
    let stub = Stub::with(&stream, &[Ends::Closed]);

    assert_eq!(count(&stream, runs::RUN_STARTED), 1);
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        count(&stream, runs::RUN_STARTED),
        1,
        "nothing moved on the stream, so the run has not been executed again"
    );
    assert!(
        stub.reruns.borrow().is_empty(),
        "and the seam was not asked"
    );

    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        count(&stream, runs::RUN_STARTED),
        2,
        "the stream moved past the recorded position, so the run ran again"
    );
    assert_eq!(*stub.reruns.borrow(), vec!["r1".to_string()]);
}

/// A re-run that waits again waits again — its own new `run.waiting` does not
/// wake it on the very next pass.
///
/// The second reading of the defect above, and the one a single-cycle arm
/// cannot take: the first `run.waiting` is written by a fixture and the second
/// by the act under test, so a comparison that was right once and wrong for the
/// line the pass itself caused would pass the arm above and fail here.
#[test]
fn a_re_run_that_waits_again_is_not_woken_by_its_own_line() {
    let scratch = Scratch::new("ac1-cycle");
    let stream = scratch.stream();
    a_waiting_run(&stream, "r1");
    let stub = Stub::with(&stream, &[Ends::Waiting, Ends::Closed]);

    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(count(&stream, runs::RUN_STARTED), 2);

    // Nothing else has been written: the run is waiting again, at a position it
    // recorded itself.
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        count(&stream, runs::RUN_STARTED),
        2,
        "the run's own second wait is not a move it is woken by"
    );

    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(count(&stream, runs::RUN_STARTED), 3);
    assert_eq!(stub.reruns.borrow().len(), 2);
}

/// A run stopped on `until` is woken by a line for an item its wake names, and
/// by nothing else — not by a seat's line, and not by another item's delivery.
///
/// THE UNRELATED ITEM IS THE ARM. Every line any seat writes while a run waits
/// is past the position the wait recorded, so a pass reading only the position
/// executes the whole workflow again for a delivery the run is not waiting on —
/// one deno child per event, each ending waiting in the same place.
#[test]
fn a_waiting_run_is_woken_only_by_a_line_for_an_item_its_wake_names() {
    let scratch = Scratch::new("wake-match");
    let stream = scratch.stream();
    a_run_waiting_with(&stream, "r1", serde_json::json!({ "waiting": ["x"] }));
    // One execution in the script: a second re-run refuses, so an arm that
    // over-woke would be reported as well as counted.
    let stub = Stub::with(&stream, &[Ends::Closed]);

    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass runs");
    an_item_moved(&stream, "item.delivered", "y");
    pass(&stub, &stream, 2).expect("the pass runs");
    assert!(
        stub.reruns.borrow().is_empty(),
        "a seat's line and another item's delivery are not what it is waiting for"
    );
    assert_eq!(count(&stream, runs::RUN_STARTED), 1);

    an_item_moved(&stream, "item.delivered", "x");
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["r1".to_string()],
        "the item it named moved, so it ran again"
    );
    assert_eq!(count(&stream, runs::RUN_STARTED), 2);
}

/// A wake that names no items re-runs on any move: the behaviour that stood
/// before the match, kept for every shape the match cannot read.
///
/// THE THREE SHAPES ARE THE POINT — a payload with no wake, a gate's id, and a
/// list with nothing in it. A match that refused any of them would hold a run
/// waiting for a line that is never coming.
#[test]
fn a_waiting_run_whose_wake_names_no_items_is_woken_by_any_line() {
    let scratch = Scratch::new("wake-fallback");
    let stream = scratch.stream();
    a_run_waiting_with(&stream, "r-none", serde_json::Value::Null);
    a_run_waiting_with(&stream, "r-gate", serde_json::json!({ "waiting": "g1" }));
    a_run_waiting_with(&stream, "r-empty", serde_json::json!({ "waiting": [] }));
    let stub = Stub::with(&stream, &[Ends::Closed, Ends::Closed, Ends::Closed]);

    // A line for an item none of the three ever named.
    an_item_moved(&stream, "item.delivered", "z");
    pass(&stub, &stream, 2).expect("the pass runs");

    let mut woken = stub.reruns.borrow().clone();
    woken.sort();
    assert_eq!(
        woken,
        vec![
            "r-empty".to_string(),
            "r-gate".to_string(),
            "r-none".to_string()
        ]
    );
}

/// The re-run is at most once per pass, and a run that has ended is not
/// re-run at all.
///
/// The control for the arm above: without it, a pass that re-ran on every kind
/// would satisfy "two after" and say nothing about which condition did it.
#[test]
fn a_closed_run_is_never_re_run_however_far_the_stream_moves() {
    let scratch = Scratch::new("ac1-control");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    log.append(
        runs::RUN_STARTED,
        "a-runner",
        serde_json::json!({ "run": "r1", "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    log.append(
        runs::RUN_CLOSED,
        "a-runner",
        serde_json::json!({ "run": "r1" }),
    )
    .expect("the close lands");
    let stub = Stub::with(&stream, &[]);

    for _ in 0..3 {
        a_line_from_elsewhere(&stream);
        pass(&stub, &stream, 2).expect("the pass runs");
    }
    assert!(
        stub.reruns.borrow().is_empty(),
        "a closed run is not re-run"
    );
    assert_eq!(count(&stream, runs::RUN_STARTED), 1);
    assert_eq!(
        count(&stream, runs::RUN_CLEANED),
        1,
        "it was cleaned once, and the latch held for the other two passes"
    );
}

// ---- AC2: the crash cap, the gate and the park --------------------------------

/// A run nothing can classify is executed cap-plus-one times, then gated and
/// parked, and never executed again.
#[test]
fn a_run_nothing_can_classify_runs_to_the_cap_then_parks() {
    let scratch = Scratch::new("ac2");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    log.append(
        runs::RUN_STARTED,
        "a-runner",
        serde_json::json!({ "run": "r2", "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    log.append(
        runs::RUN_COULD_NOT_TELL,
        "a-runner",
        serde_json::json!({ "run": "r2", "exit": 7, "read": serde_json::Value::Null }),
    )
    .expect("the reading lands");

    let stub = Stub::with(&stream, &[Ends::CouldNotTell, Ends::CouldNotTell]);
    for _ in 0..4 {
        pass(&stub, &stream, 2).expect("the pass runs");
    }

    assert_eq!(
        count(&stream, runs::RUN_STARTED),
        3,
        "the cap is 2 re-runs, so the run was executed three times"
    );
    assert_eq!(stub.reruns.borrow().len(), 2);
    let gates = stub.gates.borrow();
    assert_eq!(gates.len(), 1, "one gate, raised once at the cap");
    assert!(
        gates[0].1.contains("max_crashes") && gates[0].1.contains('3'),
        "the gate's reason carries the last reading: {}",
        gates[0].1
    );
    let parked = of_kind(&stream, runs::ITEM_PARKED);
    assert_eq!(parked.len(), 1, "one park, and not one per poll");
    assert_eq!(parked[0].payload["item"], "r2");
    assert_eq!(parked[0].payload["gate"], "gate-for-r2");

    // And no further execution, however far the stream then moves.
    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(count(&stream, runs::RUN_STARTED), 3);
    assert_eq!(count(&stream, runs::ITEM_PARKED), 1);
}

/// The cap is READ and not a constant this pass carries: a fleet that names one
/// re-run gets two executions.
#[test]
fn the_cap_the_pass_is_handed_is_the_one_it_counts_against() {
    let scratch = Scratch::new("ac2-cap");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    log.append(
        runs::RUN_COULD_NOT_TELL,
        "a-runner",
        serde_json::json!({ "run": "r2", "exit": 7, "read": serde_json::Value::Null }),
    )
    .expect("the reading lands");

    let stub = Stub::with(&stream, &[Ends::CouldNotTell, Ends::CouldNotTell]);
    for _ in 0..4 {
        pass(&stub, &stream, 1).expect("the pass runs");
    }
    assert_eq!(stub.reruns.borrow().len(), 1, "one re-run under a cap of 1");
    assert_eq!(count(&stream, runs::ITEM_PARKED), 1);
}

// ---- AC3: the cleanup ----------------------------------------------------------

/// The seats one run spawned are retired when it closes, a seat naming another
/// run is untouched, and one `run.cleaned` carries the count.
#[test]
fn a_closed_run_retires_the_seats_it_spawned_and_no_others() {
    let scratch = Scratch::new("ac3");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    for (seat, run) in [
        ("s1", Some("r3")),
        ("s2", Some("r3")),
        ("s9", Some("other")),
    ] {
        log.append(
            events::SESSION_SPAWNED,
            seat,
            serde_json::json!({ "worktree": "/w", "run": run }),
        )
        .expect("the spawn lands");
    }
    // A seat spawned outside every run: its payload carries no run key at all,
    // which is the shape a spawn from a shell writes.
    log.append(
        events::SESSION_SPAWNED,
        "s7",
        serde_json::json!({ "worktree": "/w", "run": serde_json::Value::Null }),
    )
    .expect("the spawn lands");
    log.append(
        runs::RUN_STARTED,
        "a-runner",
        serde_json::json!({ "run": "r3", "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    log.append(
        runs::RUN_CLOSED,
        "a-runner",
        serde_json::json!({ "run": "r3" }),
    )
    .expect("the close lands");

    let stub = Stub::with(&stream, &[]);
    pass(&stub, &stream, 2).expect("the pass runs");

    assert_eq!(
        *stub.retires.borrow(),
        vec![
            ("s1".to_string(), "r3".to_string()),
            ("s2".to_string(), "r3".to_string())
        ],
        "the run's two seats, and neither the other run's nor the runless one"
    );
    let retired: Vec<String> = of_kind(&stream, events::SESSION_RETIRED)
        .into_iter()
        .map(|record| record.actor)
        .collect();
    assert_eq!(retired, vec!["s1".to_string(), "s2".to_string()]);
    let cleaned = of_kind(&stream, runs::RUN_CLEANED);
    assert_eq!(cleaned.len(), 1);
    assert_eq!(cleaned[0].payload["run"], "r3");
    assert_eq!(cleaned[0].payload["count"], 2);

    // The latch: a second pass asks for nothing.
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(stub.retires.borrow().len(), 2);
    assert_eq!(count(&stream, runs::RUN_CLEANED), 1);
}

/// A seat already retired is not retired again, and the count says so.
///
/// The subtraction is taken IN STREAM ORDER: a seat name spawned, retired and
/// spawned again is live, and a set subtracted at the end of the fold would call
/// it gone.
#[test]
fn a_seat_retired_before_the_cleanup_is_not_asked_for_twice() {
    let scratch = Scratch::new("ac3-order");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    let spawn = |log: &mut EventLog, seat: &str| {
        log.append(
            events::SESSION_SPAWNED,
            seat,
            serde_json::json!({ "worktree": "/w", "run": "r4" }),
        )
        .expect("the spawn lands");
    };
    spawn(&mut log, "s1");
    spawn(&mut log, "s2");
    // s1 goes early, by somebody else's hand.
    log.append(
        events::SESSION_STOPPED,
        "s1",
        serde_json::json!({ "worktree": "/w" }),
    )
    .expect("the stop lands");
    // …and a seat of the same name is spawned again under the same run.
    spawn(&mut log, "s1");
    log.append(
        runs::RUN_FAILED,
        "a-runner",
        serde_json::json!({ "run": "r4", "reason": { "said": "no" } }),
    )
    .expect("the failure lands");

    let stub = Stub::with(&stream, &[]);
    pass(&stub, &stream, 2).expect("the pass runs");

    let asked: Vec<String> = stub
        .retires
        .borrow()
        .iter()
        .map(|(seat, _)| seat.clone())
        .collect();
    assert_eq!(
        asked,
        vec!["s1".to_string(), "s2".to_string()],
        "the re-spawned s1 is live and is asked for; the dead one is not asked for twice"
    );
    assert_eq!(of_kind(&stream, runs::RUN_CLEANED)[0].payload["count"], 2);
}

/// A run that spawned nothing is still cleaned, once, with a measured zero.
#[test]
fn a_run_that_spawned_no_seat_is_cleaned_with_a_count_of_zero() {
    let scratch = Scratch::new("ac3-zero");
    let stream = scratch.stream();
    EventLog::open(&stream)
        .append(
            runs::RUN_CLOSED,
            "a-runner",
            serde_json::json!({ "run": "r5" }),
        )
        .expect("the close lands");

    let stub = Stub::with(&stream, &[]);
    pass(&stub, &stream, 2).expect("the pass runs");
    pass(&stub, &stream, 2).expect("the pass runs");

    let cleaned = of_kind(&stream, runs::RUN_CLEANED);
    assert_eq!(cleaned.len(), 1, "written once, and the latch held");
    assert_eq!(cleaned[0].payload["count"], 0);
}

/// The park cleans too: a person looking at a parked run is not also holding
/// its seats.
#[test]
fn the_park_retires_the_runs_seats_as_an_ending_does() {
    let scratch = Scratch::new("ac2-clean");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    log.append(
        events::SESSION_SPAWNED,
        "s3",
        serde_json::json!({ "worktree": "/w", "run": "r6" }),
    )
    .expect("the spawn lands");
    log.append(
        runs::RUN_COULD_NOT_TELL,
        "a-runner",
        serde_json::json!({ "run": "r6", "exit": 7, "read": serde_json::Value::Null }),
    )
    .expect("the reading lands");

    let stub = Stub::with(&stream, &[]);
    // A cap of zero parks on the first reading nothing could classify.
    pass(&stub, &stream, 0).expect("the pass runs");

    assert_eq!(count(&stream, runs::ITEM_PARKED), 1);
    assert_eq!(
        *stub.retires.borrow(),
        vec![("s3".to_string(), "r6".to_string())]
    );
    assert_eq!(of_kind(&stream, runs::RUN_CLEANED)[0].payload["count"], 1);
}

// ---- the refusals --------------------------------------------------------------

/// A seam that refuses is reported and nothing else in the pass is stopped.
#[test]
fn one_run_that_will_not_move_does_not_stop_the_pass() {
    let scratch = Scratch::new("refusal");
    let stream = scratch.stream();
    a_waiting_run(&stream, "r7");
    EventLog::open(&stream)
        .append(
            runs::RUN_CLOSED,
            "a-runner",
            serde_json::json!({ "run": "r8" }),
        )
        .expect("the close lands");
    a_line_from_elsewhere(&stream);

    // An empty script: the seam refuses every re-run it is asked for.
    let stub = Stub::with(&stream, &[]);
    let answered = pass(&stub, &stream, 2).expect_err("the pass reports the refusal");
    assert!(
        answered.contains("r7"),
        "the refusal names the run: {answered}"
    );
    assert_eq!(
        count(&stream, runs::RUN_CLEANED),
        1,
        "and r8 was cleaned in the same pass"
    );
}

// ---- the environment read ------------------------------------------------------

/// The run key is the spawning process's own, and a blank is no run.
///
/// A variable exported and never set reaches a child as the empty string, and a
/// seat tagged with the empty run would be swept up by a cleanup naming no run
/// at all.
#[test]
fn a_blank_run_id_in_the_environment_is_no_run() {
    // SAFETY: the environment is process-wide, so this arm sets and clears the
    // one variable it reads and never runs beside another that reads it.
    std::env::set_var(runs::ENV_RUN_ID, "   ");
    assert_eq!(runs::of_this_process(), None);
    std::env::set_var(runs::ENV_RUN_ID, "fx-a-run");
    assert_eq!(
        runs::of_this_process(),
        Some("fx-a-run".to_string()),
        "a value that is there is read whole"
    );
    std::env::remove_var(runs::ENV_RUN_ID);
    assert_eq!(runs::of_this_process(), None);
}
