//! The run lifecycle's controller half.
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

use fleet_controller::events::{self, ActorRef, EventLog};
use fleet_controller::runs::{self, Pass, Runs, Standing};
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
    /// Waiting on the id given — a hold or a child the stream may or may not
    /// know — as `hold` and `start` throw it.
    WaitingOn(&'static str),
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
    holds: RefCell<Vec<(String, String)>>,
    retires: RefCell<Vec<(String, String)>>,
}

impl Stub {
    fn with(stream: &Path, script: &[Ends]) -> Stub {
        Stub {
            stream: stream.to_path_buf(),
            script: RefCell::new(script.iter().copied().collect()),
            reruns: RefCell::new(Vec::new()),
            holds: RefCell::new(Vec::new()),
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
            &the_runner(),
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
            Ends::WaitingOn(id) => (
                runs::RUN_WAITING,
                serde_json::json!({ "run": run, "wake": id, "seq": at_exit }),
            ),
            Ends::CouldNotTell => (
                runs::RUN_COULD_NOT_TELL,
                serde_json::json!({ "run": run, "exit": 7, "read": serde_json::Value::Null }),
            ),
        };
        log.append(kind, &the_runner(), payload)
            .expect("the stream takes the outcome");
        Ok(())
    }

    fn hold(&self, run: &str, reason: &str) -> Result<String, String> {
        self.holds
            .borrow_mut()
            .push((run.to_string(), reason.to_string()));
        Ok(format!("hold-for-{run}"))
    }

    fn retire(&self, seat: &str, run: &str) -> Result<(), String> {
        self.retires
            .borrow_mut()
            .push((seat.to_string(), run.to_string()));
        self.log()
            .append(
                events::SESSION_RETIRED,
                &a_seat(seat),
                serde_json::json!({ "seat": seat, "item": run }),
            )
            .expect("the stream takes the retirement");
        Ok(())
    }
}

/// The runner every run-lifecycle line of these arms is written by.
fn the_runner() -> ActorRef {
    ActorRef::new(events::RUN, "a-runner")
}

/// A seat, as the line about it names it.
fn a_seat(id: &str) -> ActorRef {
    ActorRef::seat(id)
}

/// One pass, with the cap the arm names.
fn pass(stub: &Stub, stream: &Path, max_crashes: u64) -> Result<(), String> {
    let mut log = EventLog::open(stream);
    let controller = ActorRef::new(events::CONTROLLER, "a-machine");
    let mut pass = Pass {
        runs: stub,
        events: &mut log,
        controller: &controller,
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
        &the_runner(),
        serde_json::json!({ "run": run, "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    let at_exit = EventLog::open(stream).seq();
    log.append(
        runs::RUN_WAITING,
        &the_runner(),
        serde_json::json!({ "run": run, "wake": { "for": "a line" }, "seq": at_exit }),
    )
    .expect("the wait lands");
}

/// Somebody else's line on the stream — the move a waiting run is woken by.
fn a_line_from_elsewhere(stream: &Path) {
    EventLog::open(stream)
        .append(events::SEAT_WOKE, &a_seat("s1"), serde_json::json!({}))
        .expect("the line lands");
}

/// The stream as one run that has been opened and has stopped on the wake it is
/// handed — the condition the wrapper printed, as the back half stores it. A
/// `null` stands for the payload that carries no wake key at all: `get` answers
/// the same for both.
fn a_run_waiting_with(stream: &Path, run: &str, wake: serde_json::Value) {
    let mut log = EventLog::open(stream);
    log.append(
        runs::RUN_STARTED,
        &the_runner(),
        serde_json::json!({ "run": run, "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    let at_exit = EventLog::open(stream).seq();
    log.append(
        runs::RUN_WAITING,
        &the_runner(),
        serde_json::json!({ "run": run, "wake": wake, "seq": at_exit }),
    )
    .expect("the wait lands");
}

/// One item's state, announced by whoever moved it.
fn an_item_moved(stream: &Path, kind: &str, item: &str) {
    EventLog::open(stream)
        .append(kind, &a_seat("s1"), serde_json::json!({ "item": item }))
        .expect("the line lands");
}

/// A hold raised on a run's own record, announced as `fleet hold` announces it:
/// `item.held` naming the run as the item and the hold it raised. The SDK's
/// `hold` raises first and waits after, so the raise is on the stream below the
/// wait that names the hold.
fn a_hold_raised(stream: &Path, run: &str, hold: &str) {
    EventLog::open(stream)
        .append(
            runs::ITEM_HELD,
            &ActorRef::new(events::RUN, run),
            serde_json::json!({
                "item": run, "reason": "ask", "branch": null, "commit": null, "hold": hold,
            }),
        )
        .expect("the hold lands");
}

/// A person's clearance of a hold.
fn a_hold_cleared(stream: &Path, item: &str, hold: &str) {
    EventLog::open(stream)
        .append(
            runs::HOLD_CLEARED,
            &a_seat("alberto"),
            serde_json::json!({ "item": item, "hold": hold, "letter": "a" }),
        )
        .expect("the clearance lands");
}

/// One lifecycle line of a run nobody is waiting on, or of a child.
fn a_run_line(stream: &Path, kind: &str, run: &str) {
    EventLog::open(stream)
        .append(kind, &the_runner(), serde_json::json!({ "run": run }))
        .expect("the line lands");
}

/// The stream as one run that stopped on `wake`, with what `between` writes
/// landing after its start and before its exit — so those lines sit at or
/// below the position the wait records, as a line that lands while the
/// process is still on its way out does. The position is read at exit, as
/// `read_the_exit` reads it.
fn a_run_waiting_after(
    stream: &Path,
    run: &str,
    wake: serde_json::Value,
    between: impl FnOnce(&Path),
) {
    a_run_line(stream, runs::RUN_STARTED, run);
    between(stream);
    let at_exit = EventLog::open(stream).seq();
    EventLog::open(stream)
        .append(
            runs::RUN_WAITING,
            &the_runner(),
            serde_json::json!({ "run": run, "wake": wake, "seq": at_exit }),
        )
        .expect("the wait lands");
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
    a_run_waiting_with(&stream, "r1", serde_json::json!(["x"]));
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

/// A wake the pass cannot read re-runs on any move: the behaviour that stood
/// before the match, kept for every shape the match does not know.
///
/// THE FOUR SHAPES ARE THE POINT — a payload with no wake, a list with nothing
/// in it, an object a workflow threw itself, and an id no hold and no run on
/// the stream carries. A match that refused any of them would hold a run
/// waiting for a line that is never coming.
#[test]
fn a_waiting_run_whose_wake_the_pass_cannot_read_is_woken_by_any_line() {
    let scratch = Scratch::new("wake-fallback");
    let stream = scratch.stream();
    a_run_waiting_with(&stream, "r-none", serde_json::Value::Null);
    a_run_waiting_with(&stream, "r-empty", serde_json::json!([]));
    a_run_waiting_with(&stream, "r-own", serde_json::json!({ "for": "a line" }));
    a_run_waiting_with(&stream, "r-word", serde_json::json!("tomorrow"));
    let stub = Stub::with(
        &stream,
        &[Ends::Closed, Ends::Closed, Ends::Closed, Ends::Closed],
    );

    // A line for an item none of the four ever named.
    an_item_moved(&stream, "item.delivered", "z");
    pass(&stub, &stream, 2).expect("the pass runs");

    let mut woken = stub.reruns.borrow().clone();
    woken.sort();
    assert_eq!(
        woken,
        vec![
            "r-empty".to_string(),
            "r-none".to_string(),
            "r-own".to_string(),
            "r-word".to_string()
        ]
    );
}

/// A run stopped on a hold is woken by that hold's clearance and by nothing
/// else — not by a seat's line, not by an item's, not by another hold's
/// clearance, and
/// not by the lines another waiting run's re-run writes.
///
/// THE LAST OF THOSE IS THE ARM. Two runs waiting on holds, each re-run on any
/// move, keep each other running: the first one's re-run writes `run.started`,
/// a `step.started` and a `run.waiting`, which is a move for the second, whose
/// re-run is a move for the first — one child execution per run per poll, and
/// another `step.started` on the stream each time, for as long as nobody
/// clears. A kill-and-resume on a takeoff stopped at its hold is exactly that
/// run.
#[test]
fn a_run_waiting_on_a_hold_is_woken_only_by_that_hold_s_clearance() {
    let scratch = Scratch::new("wake-hold");
    let stream = scratch.stream();
    a_hold_raised(&stream, "r1", "g1");
    a_run_waiting_with(&stream, "r1", serde_json::json!("g1"));
    a_hold_raised(&stream, "r2", "g2");
    a_run_waiting_with(&stream, "r2", serde_json::json!("g2"));
    // One execution in the script: a second re-run refuses, so a pass that
    // over-woke is reported as well as counted.
    let stub = Stub::with(&stream, &[Ends::Closed]);

    a_line_from_elsewhere(&stream);
    an_item_moved(&stream, "item.delivered", "g1");
    a_hold_cleared(&stream, "it-9", "g9");
    let passed = pass(&stub, &stream, 2);
    assert!(
        stub.reruns.borrow().is_empty(),
        "nothing it is waiting for has moved, and it was executed: {:?}",
        stub.reruns.borrow()
    );
    passed.expect("the pass runs");

    a_hold_cleared(&stream, "r1", "g1");
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["r1".to_string()],
        "its hold was cleared, so it ran again, and the other did not"
    );

    // The re-run's own lines are on the stream now, above where r2 stopped.
    let passed = pass(&stub, &stream, 2);
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["r1".to_string()],
        "another run's execution is not a clearance of r2's hold"
    );
    passed.expect("the pass runs");
}

/// A run stopped on a child it started is woken by that child's end — its
/// `run.closed` or its `run.failed` — and not by the child's own start, not by
/// another run's end, and not by anybody else's line.
///
/// BOTH ENDS WAKE IT: the SDK's `start` returns on the child's close and fails
/// the parent on the child's failure, and a parent that slept through the
/// failure would wait on a child that is never running again.
#[test]
fn a_run_waiting_on_a_child_run_is_woken_only_by_that_child_s_end() {
    let scratch = Scratch::new("wake-child");
    let stream = scratch.stream();
    a_run_line(&stream, runs::RUN_STARTED, "c1");
    a_run_waiting_with(&stream, "p1", serde_json::json!("c1"));
    a_run_line(&stream, runs::RUN_STARTED, "c2");
    a_run_waiting_with(&stream, "p2", serde_json::json!("c2"));
    let stub = Stub::with(&stream, &[Ends::Closed, Ends::Closed]);

    a_line_from_elsewhere(&stream);
    a_run_line(&stream, runs::RUN_STARTED, "c1");
    a_run_line(&stream, runs::RUN_STARTED, "other");
    a_run_line(&stream, runs::RUN_CLOSED, "other");
    let passed = pass(&stub, &stream, 2);
    assert!(
        stub.reruns.borrow().is_empty(),
        "neither child has ended, and a parent was executed: {:?}",
        stub.reruns.borrow()
    );
    passed.expect("the pass runs");

    a_run_line(&stream, runs::RUN_CLOSED, "c1");
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["p1".to_string()],
        "c1 closed, so p1 ran again, and p2 did not"
    );

    a_run_line(&stream, runs::RUN_FAILED, "c2");
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["p1".to_string(), "p2".to_string()],
        "c2 failed, so p2 ran again"
    );
}

/// A CANCEL IS A CHILD'S END TOO: a run stopped on a child that a person
/// cancelled is woken by the child's `run.cancelled`, and a cancel of some
/// other run does not wake it.
///
/// Nothing executes a cancelled run again, so a parent that slept through the
/// cancel would wait for good on a child that is never ending any other way —
/// holding its own record, and a `[core.run] max_open` slot, as it did.
#[test]
fn a_run_waiting_on_a_child_is_woken_by_that_child_s_cancel_and_no_other() {
    let scratch = Scratch::new("wake-child-cancel");
    let stream = scratch.stream();
    a_run_line(&stream, runs::RUN_STARTED, "c1");
    a_run_waiting_with(&stream, "p1", serde_json::json!("c1"));
    let stub = Stub::with(&stream, &[Ends::Closed]);

    a_run_line(&stream, runs::RUN_STARTED, "other");
    a_run_line(&stream, runs::RUN_CANCELLED, "other");
    let passed = pass(&stub, &stream, 2);
    assert!(
        stub.reruns.borrow().is_empty(),
        "another run's cancel woke the parent: {:?}",
        stub.reruns.borrow()
    );
    passed.expect("the pass runs");

    a_run_line(&stream, runs::RUN_CANCELLED, "c1");
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["p1".to_string()],
        "c1 was cancelled, so p1 ran again"
    );
}

/// A run whose hold was cleared after the raise and before its process exited
/// is woken by that clearance, though it sits at or below the position
/// the wait recorded — and once the re-run has moved it on, it is not woken
/// again.
///
/// THE POSITION IS READ AT EXIT. `hold` raises, finds no clearance and throws;
/// the wrapper prints and the process exits, and only then does the back half
/// read the stream's position for `run.waiting`. A clearance that lands in that
/// gap is below the position, so a match that looked only above it would keep
/// the run on a hold that is already cleared — and with nothing else
/// written, the stream would never move past it for any match to look at.
///
/// ONCE, AND NOT EVERY POLL: the clearance stays on the stream, and what stops it
/// waking the run again is that the fold reads the run's latest `run.waiting`,
/// which after the re-run names another hold.
#[test]
fn a_hold_cleared_before_the_run_exited_still_wakes_it_and_only_once() {
    let scratch = Scratch::new("wake-hold-early");
    let stream = scratch.stream();
    a_run_waiting_after(&stream, "r1", serde_json::json!("g1"), |stream| {
        a_hold_raised(stream, "r1", "g1");
        a_hold_cleared(stream, "r1", "g1");
    });
    a_run_waiting_after(&stream, "r2", serde_json::json!("g2"), |stream| {
        a_hold_raised(stream, "r2", "g2");
        a_hold_cleared(stream, "it-9", "g3");
    });
    // The re-run replays past the cleared hold and stops on the next one.
    let stub = Stub::with(&stream, &[Ends::WaitingOn("g4")]);

    let passed = pass(&stub, &stream, 2);
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["r1".to_string()],
        "r1's hold was cleared before it exited, and r2's never was"
    );
    passed.expect("the pass runs");

    a_hold_raised(&stream, "r1", "g4");
    a_line_from_elsewhere(&stream);
    for _ in 0..2 {
        let passed = pass(&stub, &stream, 2);
        assert_eq!(
            *stub.reruns.borrow(),
            vec!["r1".to_string()],
            "g1's clearance woke r1 once; r1 now waits on g4, which nobody cleared"
        );
        passed.expect("the pass runs");
    }
}

/// A run whose child ended after the start and before the parent's process
/// exited is woken by that end, though it sits at or below the position the
/// wait recorded — and another run's end, in the same gap, wakes nothing.
///
/// The child's reading of the arm above: `start` reads no `run.closed` and no
/// `run.failed` for its child, throws, and the child ends while the parent is
/// still on its way out.
#[test]
fn a_child_that_ended_before_its_parent_exited_still_wakes_the_parent() {
    let scratch = Scratch::new("wake-child-early");
    let stream = scratch.stream();
    a_run_waiting_after(&stream, "p1", serde_json::json!("c1"), |stream| {
        a_run_line(stream, runs::RUN_STARTED, "c1");
        a_run_line(stream, runs::RUN_CLOSED, "c1");
    });
    a_run_waiting_after(&stream, "p2", serde_json::json!("c2"), |stream| {
        a_run_line(stream, runs::RUN_STARTED, "c2");
        a_run_line(stream, runs::RUN_STARTED, "other");
        a_run_line(stream, runs::RUN_FAILED, "other");
    });
    let stub = Stub::with(&stream, &[Ends::Closed]);

    let passed = pass(&stub, &stream, 2);
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["p1".to_string()],
        "c1 closed before p1 exited, and c2 has not ended"
    );
    passed.expect("the pass runs");

    let passed = pass(&stub, &stream, 2);
    assert_eq!(
        *stub.reruns.borrow(),
        vec!["p1".to_string()],
        "p1 closed on its re-run, and nothing else moved"
    );
    passed.expect("the pass runs");
}

/// A wake still wrapped as `{"waiting": <condition>}` is read as the condition
/// inside it, on both shapes the match knows.
///
/// A RUN'S BUNDLE IS PINNED in its directory and every re-run executes that
/// same file, so a run bundled by an SDK that printed the wrapper keeps printing
/// it for as long as it waits. Read whole, both waits below would fall to the
/// fallback and wake on any move.
#[test]
fn a_wake_still_wrapped_by_a_pinned_bundle_is_read_as_the_condition_inside() {
    let scratch = Scratch::new("wake-wrapped");
    let stream = scratch.stream();
    a_run_waiting_with(&stream, "r-items", serde_json::json!({ "waiting": ["x"] }));
    a_hold_raised(&stream, "r-hold", "g1");
    a_run_waiting_with(&stream, "r-hold", serde_json::json!({ "waiting": "g1" }));
    let stub = Stub::with(&stream, &[Ends::Closed, Ends::Closed]);

    a_line_from_elsewhere(&stream);
    let passed = pass(&stub, &stream, 2);
    assert!(
        stub.reruns.borrow().is_empty(),
        "a seat's line is neither wait's condition: {:?}",
        stub.reruns.borrow()
    );
    passed.expect("the pass runs");

    an_item_moved(&stream, "item.delivered", "x");
    a_hold_cleared(&stream, "r-hold", "g1");
    pass(&stub, &stream, 2).expect("the pass runs");
    let mut woken = stub.reruns.borrow().clone();
    woken.sort();
    assert_eq!(woken, vec!["r-hold".to_string(), "r-items".to_string()]);
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
        &the_runner(),
        serde_json::json!({ "run": "r1", "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    log.append(
        runs::RUN_CLOSED,
        &the_runner(),
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

// ---- AC2: the crash cap, the hold and the park --------------------------------

/// A run nothing can classify is executed cap-plus-one times, then held, and
/// never executed again.
#[test]
fn a_run_nothing_can_classify_runs_to_the_cap_then_parks() {
    let scratch = Scratch::new("ac2");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    log.append(
        runs::RUN_STARTED,
        &the_runner(),
        serde_json::json!({ "run": "r2", "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    log.append(
        runs::RUN_COULD_NOT_TELL,
        &the_runner(),
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
    let holds = stub.holds.borrow();
    assert_eq!(holds.len(), 1, "one hold, raised once at the cap");
    assert!(
        holds[0].1.contains("max_crashes") && holds[0].1.contains('3'),
        "the hold's reason carries the last reading: {}",
        holds[0].1
    );
    let held = of_kind(&stream, runs::ITEM_HELD);
    assert_eq!(held.len(), 1, "one park, and not one per poll");
    assert_eq!(held[0].payload["item"], "r2");
    assert_eq!(held[0].payload["hold"], "hold-for-r2");

    // And no further execution, however far the stream then moves.
    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass runs");
    assert_eq!(count(&stream, runs::RUN_STARTED), 3);
    assert_eq!(count(&stream, runs::ITEM_HELD), 1);
}

/// What a reader outside the pass is told is the pass's own fold: a run the
/// pass has not parked reads as could-not-tell, and the same run reads as
/// held, on the hold the pass raised, once the pass has parked it — never on
/// a rule of the reader's own. A failure reads as failed with its reason.
#[test]
fn the_readings_follow_the_pass_from_could_not_tell_to_held() {
    let scratch = Scratch::new("readings");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    for (kind, payload) in [
        (
            runs::RUN_STARTED,
            serde_json::json!({ "run": "r3", "hash": "abc", "workflow": "w" }),
        ),
        (
            runs::RUN_COULD_NOT_TELL,
            serde_json::json!({ "run": "r3", "exit": 7, "read": serde_json::Value::Null }),
        ),
        (
            runs::RUN_STARTED,
            serde_json::json!({ "run": "r4", "hash": "abc", "workflow": "w" }),
        ),
        (
            runs::RUN_FAILED,
            serde_json::json!({ "run": "r4", "reason": { "why": "refused" } }),
        ),
    ] {
        log.append(kind, &the_runner(), payload)
            .expect("the line lands");
    }
    let read = || runs::readings(&events::read_after(&stream, 0));

    let before = read();
    assert_eq!(before.len(), 2, "{before:?}");
    assert_eq!(before[0].run, "r3");
    assert_eq!(before[0].standing, Standing::CouldNotTell);
    assert_eq!(before[0].workflow.as_deref(), Some("w"));
    assert_eq!(before[0].crashes, 1);
    assert_eq!(before[0].hold, None, "no hold before the pass raised one");
    assert_eq!(before[1].run, "r4");
    assert_eq!(before[1].standing, Standing::Failed);
    assert_eq!(before[1].said["reason"]["why"], "refused");
    assert!(!before[1].stamp.is_empty(), "the line's stamp is carried");

    // A cap of zero re-runs: the one could-not-tell already stands at it.
    let stub = Stub::with(&stream, &[]);
    pass(&stub, &stream, 0).expect("the pass runs");
    let after = read();
    assert_eq!(after[0].standing, Standing::Held);
    assert_eq!(after[0].hold.as_deref(), Some("hold-for-r3"));
    assert_eq!(after[1].standing, Standing::Failed);
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
        &the_runner(),
        serde_json::json!({ "run": "r2", "exit": 7, "read": serde_json::Value::Null }),
    )
    .expect("the reading lands");

    let stub = Stub::with(&stream, &[Ends::CouldNotTell, Ends::CouldNotTell]);
    for _ in 0..4 {
        pass(&stub, &stream, 1).expect("the pass runs");
    }
    assert_eq!(stub.reruns.borrow().len(), 1, "one re-run under a cap of 1");
    assert_eq!(count(&stream, runs::ITEM_HELD), 1);
}

// ---- AC3: the cleanup ----------------------------------------------------------

/// The seats one run spawned are retired when it closes, a seat naming another
/// run is untouched, and one `run.cleaned` carries the count.
#[test]
fn a_closed_run_retires_the_seats_it_spawned_and_no_others() {
    // Every actor on a session line is the seat, by its full id.
    const S1: &str = "01a0d1f1-0aec-765f-9abe-1a1a1a1a1a1a";
    const S2: &str = "01a0d1f1-0aec-765f-9abe-2b2b2b2b2b2b";
    const S3: &str = "01a0d1f1-0aec-765f-9abe-3c3c3c3c3c3c";
    const S7: &str = "01a0d1f1-0aec-765f-9abe-7c7c7c7c7c7c";
    const S9: &str = "01a0d1f1-0aec-765f-9abe-9d9d9d9d9d9d";
    let scratch = Scratch::new("ac3");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    for (seat, run) in [(S1, Some("r3")), (S2, Some("r3")), (S9, Some("other"))] {
        log.append(
            events::SESSION_SPAWNED,
            &a_seat(seat),
            serde_json::json!({ "worktree": "/w", "run": run }),
        )
        .expect("the spawn lands");
    }
    // A seat spawned outside every run: its payload carries no run key at all,
    // which is the shape a spawn from a shell writes.
    log.append(
        events::SESSION_SPAWNED,
        &a_seat(S7),
        serde_json::json!({ "worktree": "/w", "run": serde_json::Value::Null }),
    )
    .expect("the spawn lands");
    // A line whose actor is no seat, though its id has a seat id's shape: it
    // names nobody the run spawned, and the cleanup asks for nothing on it.
    log.append(
        events::SESSION_SPAWNED,
        &ActorRef::new(events::RUN, S3),
        serde_json::json!({ "worktree": "/w", "run": "r3" }),
    )
    .expect("the spawn lands");
    log.append(
        runs::RUN_STARTED,
        &the_runner(),
        serde_json::json!({ "run": "r3", "hash": "abc", "workflow": "w" }),
    )
    .expect("the start lands");
    log.append(
        runs::RUN_CLOSED,
        &the_runner(),
        serde_json::json!({ "run": "r3" }),
    )
    .expect("the close lands");

    let stub = Stub::with(&stream, &[]);
    pass(&stub, &stream, 2).expect("the pass runs");

    assert_eq!(
        *stub.retires.borrow(),
        vec![
            (S1.to_string(), "r3".to_string()),
            (S2.to_string(), "r3".to_string())
        ],
        "the run's two seats, by id, and neither the other run's nor the runless one"
    );
    let retired: Vec<String> = of_kind(&stream, events::SESSION_RETIRED)
        .into_iter()
        .map(|record| record.actor.id)
        .collect();
    assert_eq!(retired, vec![S1.to_string(), S2.to_string()]);
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
            &a_seat(seat),
            serde_json::json!({ "worktree": "/w", "run": "r4" }),
        )
        .expect("the spawn lands");
    };
    spawn(&mut log, "s1");
    spawn(&mut log, "s2");
    // s1 goes early, by somebody else's hand.
    log.append(
        events::SESSION_STOPPED,
        &a_seat("s1"),
        serde_json::json!({ "worktree": "/w" }),
    )
    .expect("the stop lands");
    // …and a seat of the same name is spawned again under the same run.
    spawn(&mut log, "s1");
    log.append(
        runs::RUN_FAILED,
        &the_runner(),
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
            &the_runner(),
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

/// The park cleans too: a person looking at a held run is not also holding
/// its seats.
#[test]
fn the_park_retires_the_runs_seats_as_an_ending_does() {
    let scratch = Scratch::new("ac2-clean");
    let stream = scratch.stream();
    let mut log = EventLog::open(&stream);
    log.append(
        events::SESSION_SPAWNED,
        &a_seat("s3"),
        serde_json::json!({ "worktree": "/w", "run": "r6" }),
    )
    .expect("the spawn lands");
    log.append(
        runs::RUN_COULD_NOT_TELL,
        &the_runner(),
        serde_json::json!({ "run": "r6", "exit": 7, "read": serde_json::Value::Null }),
    )
    .expect("the reading lands");

    let stub = Stub::with(&stream, &[]);
    // A cap of zero parks on the first reading nothing could classify.
    pass(&stub, &stream, 0).expect("the pass runs");

    assert_eq!(count(&stream, runs::ITEM_HELD), 1);
    assert_eq!(
        *stub.retires.borrow(),
        vec![("s3".to_string(), "r6".to_string())]
    );
    assert_eq!(of_kind(&stream, runs::RUN_CLEANED)[0].payload["count"], 1);
}

// ---- the cancel ----------------------------------------------------------------

/// A CANCELLED RUN IS ENDED FOR GOOD: the pass never executes it again — not
/// when the stream moves past its wait, and not when an execution that was
/// under way when it was cancelled writes a wait of its own after the cancel —
/// and it retires the seats the run spawned, once, as it does for a run that
/// closed.
#[test]
fn a_cancelled_run_is_never_executed_again_and_its_seats_are_retired() {
    let scratch = Scratch::new("cancel");
    let stream = scratch.stream();
    EventLog::open(&stream)
        .append(
            events::SESSION_SPAWNED,
            &a_seat("s4"),
            serde_json::json!({ "worktree": "/w", "run": "r9" }),
        )
        .expect("the spawn lands");
    a_waiting_run(&stream, "r9");
    EventLog::open(&stream)
        .append(
            runs::RUN_CANCELLED,
            &a_seat("a-person"),
            serde_json::json!({ "run": "r9" }),
        )
        .expect("the cancel lands");
    a_line_from_elsewhere(&stream);

    // An empty script: a re-run asked for is a refusal the pass reports.
    let stub = Stub::with(&stream, &[]);
    pass(&stub, &stream, 2).expect("the pass asks for no re-run");
    assert!(
        stub.reruns.borrow().is_empty(),
        "a cancelled run is not re-run"
    );
    assert_eq!(
        *stub.retires.borrow(),
        vec![("s4".to_string(), "r9".to_string())],
        "its seat is let go"
    );
    let cleaned = of_kind(&stream, runs::RUN_CLEANED);
    assert_eq!(cleaned.len(), 1);
    assert_eq!(cleaned[0].payload["run"], "r9");
    assert_eq!(cleaned[0].payload["count"], 1);

    // The execution that was under way at the cancel ends on a wait, and the
    // stream moves past it: the cancel still stands.
    let at_exit = EventLog::open(&stream).seq();
    EventLog::open(&stream)
        .append(
            runs::RUN_WAITING,
            &the_runner(),
            serde_json::json!({ "run": "r9", "wake": { "for": "a line" }, "seq": at_exit }),
        )
        .expect("the late wait lands");
    a_line_from_elsewhere(&stream);
    pass(&stub, &stream, 2).expect("the pass asks for no re-run");
    assert!(
        stub.reruns.borrow().is_empty(),
        "a line written after the cancel does not reopen the run"
    );
    assert_eq!(count(&stream, runs::RUN_CLEANED), 1, "cleaned once");
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
            &the_runner(),
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
