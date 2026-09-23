//! `fleet routine list | check | run | history` against the shipped binary
//! (PRD R24).
//!
//! Every arm runs the built `fleet` with a scratch machine directory and a
//! scratch fleet root, and reads each exit from the child's own status. The
//! agent is never asked for anything here: every fixture routine carries an exec
//! action, which is the half of the surface that needs no session.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod common;
use common::hermetic::Hermetic;

struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(name: &str) -> Rig {
        let root =
            std::env::temp_dir().join(format!("fleet-routine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig { root };
        std::fs::create_dir_all(rig.machine()).unwrap();
        std::fs::create_dir_all(rig.routines_dir()).unwrap();
        write(
            &rig.fleet_root().join("fleet.toml"),
            "[controller]\npoll_seconds = 1\n",
        );
        write(
            &rig.machine().join("config.json"),
            &format!(
                r#"{{"fleet_toml": "{}", "children": [
                     {{"name":"builder-1","chosen_name":"Orla",
                       "worktrees":{{"demo":"{}"}}}}
                   ]}}"#,
                rig.fleet_root().join("fleet.toml").display(),
                rig.fleet_root().display()
            ),
        );
        rig
    }

    fn machine(&self) -> PathBuf {
        self.root.join("machine")
    }
    fn fleet_root(&self) -> PathBuf {
        self.root.join("fleet")
    }
    fn routines_dir(&self) -> PathBuf {
        self.fleet_root().join(ROUTINES)
    }
    fn state_path(&self) -> PathBuf {
        self.machine().join("orders").join("state.json")
    }
    fn events_path(&self) -> PathBuf {
        self.machine().join("events.jsonl")
    }
    fn lock_path(&self, name: &str) -> PathBuf {
        self.machine().join("orders").join(format!("{name}.lock"))
    }

    fn routine(&self, name: &str, body: &str) {
        write(&self.routines_dir().join(format!("{name}.toml")), body);
    }

    /// The built binary, with the fleet root as its working directory — which
    /// is what the walk-up for `fleet.toml` reads.
    fn fleet(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .current_dir(self.fleet_root())
            .hermetic(&self.root.join("home"), &self.machine(), None)
            .env_remove("FLEET_ORDERS_CLOCK")
            .output()
            .expect("the built binary runs")
    }

    fn state(&self) -> String {
        std::fs::read_to_string(self.state_path()).unwrap_or_default()
    }

    fn events(&self) -> String {
        std::fs::read_to_string(self.events_path()).unwrap_or_default()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const ROUTINES: &str = "orders";

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A routine that always fires, whose command leaves a witness in the fleet
/// root — which is the working directory an exec runs in.
fn ticking(witness: &str) -> String {
    format!(
        "[order]\ndescription = \"leave a mark\"\ntrigger = \"cooldown\"\ninterval = \"1h\"\n\
         [action.exec]\ncommand = \"echo ran >> {witness}\"\n"
    )
}

#[test]
fn list_prints_one_row_per_routine_with_its_source_and_a_defect_row_for_a_broken_file() {
    let rig = Rig::new("list");
    rig.routine("beat", &ticking("beat.txt"));
    rig.routine(
        "watch",
        "[order]\ndescription = \"watch a thing\"\ntrigger = \"condition\"\ncheck = \"exit 1\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    rig.routine("broken", "[order]\ntrigger = \"cron\"\ninterval = \"1m\"\n");

    let listed = rig.fleet(&["routine", "list"]);
    assert_eq!(listed.status.code(), Some(0), "{}", err(&listed));
    let body = out(&listed);
    let rows: Vec<&str> = body.lines().collect();
    assert_eq!(rows.len(), 3, "one row per file: {body}");
    assert!(
        rows[0].starts_with("beat  fleet  cooldown  next "),
        "{body}"
    );
    assert!(rows[0].ends_with("last none  streak 0"), "{body}");
    assert!(
        rows[1].starts_with("watch  fleet  condition  next "),
        "the condition routine is listed and list never runs its check: {body}"
    );
    assert!(rows[2].starts_with("DEFECT  broken  fleet  "), "{body}");
    assert!(rows[2].contains("carries no `schedule`"), "{body}");
    assert!(
        !rig.fleet_root().join("beat.txt").exists(),
        "listing a routine does not fire it"
    );
}

#[test]
fn check_answers_due_not_due_and_could_not_tell_with_its_own_exit() {
    let rig = Rig::new("check");
    rig.routine("beat", &ticking("beat.txt"));
    rig.routine(
        "quiet",
        "[order]\ndescription = \"nothing to do\"\ntrigger = \"condition\"\ncheck = \"exit 1\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    rig.routine(
        "blind",
        "[order]\ndescription = \"the instrument is out\"\ntrigger = \"condition\"\n\
         check = \"exit 3\"\ncheck_unknown_exit = [3]\n[action.exec]\ncommand = \"true\"\n",
    );

    let due = rig.fleet(&["routine", "check", "beat"]);
    assert_eq!(due.status.code(), Some(0), "{}", err(&due));
    assert!(out(&due).starts_with("due — "), "{}", out(&due));

    let not_due = rig.fleet(&["routine", "check", "quiet"]);
    assert_eq!(not_due.status.code(), Some(1));
    assert!(out(&not_due).starts_with("not-due — "), "{}", out(&not_due));

    let unknown = rig.fleet(&["routine", "check", "blind"]);
    assert_eq!(unknown.status.code(), Some(3));
    assert!(
        out(&unknown).starts_with("could-not-tell — "),
        "{}",
        out(&unknown)
    );

    // check writes nothing: no state, no stream, whatever it answered.
    assert!(!rig.state_path().exists(), "check wrote a state file");
    assert!(!rig.events_path().exists(), "check wrote to the stream");

    // A routine nobody named, and a file that is not a routine: both are exit 1
    // on the record, and each says which.
    let missing = rig.fleet(&["routine", "check", "nowhere"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(err(&missing).contains("no routine is named `nowhere`"));
}

#[test]
fn run_fires_a_routine_prints_its_event_row_and_refuses_one_that_is_not_due() {
    let rig = Rig::new("run");
    rig.routine("beat", &ticking("beat.txt"));

    let fired = rig.fleet(&["routine", "run", "beat"]);
    assert_eq!(fired.status.code(), Some(0), "{}", err(&fired));
    let row = out(&fired);
    assert!(row.contains("routine.completed"), "{row}");
    assert!(row.contains("ran — the command exited 0"), "{row}");
    assert_eq!(row.lines().count(), 1, "one row on stdout: {row}");
    assert_eq!(
        std::fs::read_to_string(rig.fleet_root().join("beat.txt")).unwrap(),
        "ran\n",
        "the command ran in the order's project root"
    );

    // The same routine, straight away: the cooldown has not passed.
    let again = rig.fleet(&["routine", "run", "beat"]);
    assert_eq!(again.status.code(), Some(1), "{}", out(&again));
    assert!(err(&again).contains("is not due"), "{}", err(&again));
    assert!(out(&again).is_empty(), "a refusal prints no row");

    // --force bypasses the gate.
    let forced = rig.fleet(&["routine", "run", "beat", "--force"]);
    assert_eq!(forced.status.code(), Some(0), "{}", err(&forced));
    assert!(
        out(&forced).contains("routine.completed"),
        "{}",
        out(&forced)
    );
    assert_eq!(
        std::fs::read_to_string(rig.fleet_root().join("beat.txt")).unwrap(),
        "ran\nran\n"
    );

    // An unknown name is exit 1 on run too.
    let missing = rig.fleet(&["routine", "run", "nowhere"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(err(&missing).contains("no routine is named `nowhere`"));
}

#[test]
fn a_dry_run_prints_the_gate_and_the_argv_and_writes_nothing_at_all() {
    let rig = Rig::new("dry");
    rig.routine("beat", &ticking("beat.txt"));
    // A firing first, so there IS a state file and a stream to compare against:
    // an arm run against nothing would pass on a verb that wrote both.
    assert_eq!(
        rig.fleet(&["routine", "run", "beat"]).status.code(),
        Some(0)
    );
    let state_before = rig.state();
    let events_before = rig.events();
    assert!(!state_before.is_empty() && !events_before.is_empty());

    let dry = rig.fleet(&["routine", "run", "beat", "--dry-run", "--force"]);
    assert_eq!(dry.status.code(), Some(0), "{}", err(&dry));
    let body = out(&dry);
    assert!(body.contains("beat fleet cooldown"), "{body}");
    assert!(body.contains("due — --force"), "{body}");
    // The shell is named by the ABSOLUTE path the constructed search path
    // resolved, never by the bare name this process would resolve for itself.
    let shell = body
        .lines()
        .find(|line| line.starts_with("argv /"))
        .unwrap_or_else(|| panic!("the argv names an absolute shell: {body}"));
    assert!(shell.ends_with("/sh"), "{shell}");
    assert!(body.contains("argv -c"), "{body}");
    assert!(body.contains("argv echo ran >> beat.txt"), "{body}");

    assert_eq!(rig.state(), state_before, "a dry run wrote state");
    assert_eq!(rig.events(), events_before, "a dry run wrote to the stream");
    assert!(
        !rig.lock_path("beat").exists(),
        "a dry run left a lock behind"
    );
    assert_eq!(
        std::fs::read_to_string(rig.fleet_root().join("beat.txt")).unwrap(),
        "ran\n",
        "and it never ran the command"
    );
}

#[test]
fn a_lock_a_live_run_holds_is_skipped_and_one_a_dead_process_left_is_taken_over() {
    let rig = Rig::new("lock");
    rig.routine("beat", &ticking("beat.txt"));

    // A live pid: this test process, which is running by definition.
    write(&rig.lock_path("beat"), &format!("{}\n", std::process::id()));
    let held = rig.fleet(&["routine", "run", "beat"]);
    assert_eq!(held.status.code(), Some(1), "{}", err(&held));
    assert!(err(&held).contains("held by a live run"), "{}", err(&held));
    assert!(out(&held).is_empty());
    assert!(
        !rig.fleet_root().join("beat.txt").exists(),
        "a skipped run never reached the command"
    );

    // A dead pid: a child of this process, reaped before the lock is written.
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .expect("a child spawns");
    let gone = child.id();
    child.wait().expect("the child is reaped");
    write(&rig.lock_path("beat"), &format!("{gone}\n"));

    let taken = rig.fleet(&["routine", "run", "beat"]);
    assert_eq!(taken.status.code(), Some(0), "{}", err(&taken));
    assert_eq!(
        err(&taken)
            .lines()
            .filter(|l| l.contains(&format!("from pid {gone}, which is gone")))
            .count(),
        1,
        "the take-over is one line on stderr: {}",
        err(&taken)
    );
    assert!(out(&taken).contains("routine.completed"));
    assert!(
        !rig.lock_path("beat").exists(),
        "a run that finished gave the lock back"
    );
}

#[test]
fn history_prints_the_rows_a_run_wrote_and_since_cuts_them() {
    let rig = Rig::new("history");
    rig.routine("beat", &ticking("beat.txt"));

    // Before anything: an absent stream is one line on stderr and exit 0, not a
    // refusal — a fleet nobody has asked anything of yet has no history.
    let empty = rig.fleet(&["routine", "history"]);
    assert_eq!(empty.status.code(), Some(0), "{}", err(&empty));
    assert!(out(&empty).is_empty());
    assert_eq!(err(&empty).lines().count(), 1, "one line: {}", err(&empty));
    assert!(err(&empty).contains("no event stream"));

    assert_eq!(
        rig.fleet(&["routine", "run", "beat"]).status.code(),
        Some(0)
    );
    assert_eq!(
        rig.fleet(&["routine", "run", "beat", "--force"])
            .status
            .code(),
        Some(0)
    );

    let all = rig.fleet(&["routine", "history"]);
    assert_eq!(all.status.code(), Some(0), "{}", err(&all));
    let rows: Vec<String> = out(&all).lines().map(str::to_string).collect();
    assert_eq!(rows.len(), 4, "two firings, two events each: {rows:?}");
    assert!(rows[0].contains("routine.fired"));
    assert!(rows[1].contains("routine.completed"));
    assert!(rows.iter().all(|row| row.contains("beat")));

    // --since cuts at the sequence: the two rows of the second firing remain.
    let since = rig.fleet(&["routine", "history", "--since", "2"]);
    assert_eq!(since.status.code(), Some(0));
    let cut: Vec<String> = out(&since).lines().map(str::to_string).collect();
    assert_eq!(cut.len(), 2, "{cut:?}");
    assert_eq!(cut, rows[2..].to_vec());

    // -n cuts from the other end: the LAST N rows.
    let last = rig.fleet(&["routine", "history", "-n", "1"]);
    assert_eq!(last.status.code(), Some(0));
    assert_eq!(out(&last).lines().count(), 1);
    assert_eq!(out(&last).trim(), rows[3]);

    // And a name filters to one routine's rows, with a routine that has none
    // answering nothing rather than everything.
    rig.routine("other", &ticking("other.txt"));
    let named = rig.fleet(&["routine", "history", "other"]);
    assert_eq!(named.status.code(), Some(0));
    assert!(out(&named).is_empty(), "{}", out(&named));
}

/// A caller who did not say what to do is exit 2, which is the one code a
/// script can tell a bad invocation by.
#[test]
fn a_caller_who_named_no_routine_or_no_subcommand_reads_usage() {
    let rig = Rig::new("usage");
    for args in [
        vec!["routine"],
        vec!["routine", "sing"],
        vec!["routine", "check"],
        vec!["routine", "run"],
        vec!["routine", "run", "--nowhere"],
        vec!["routine", "list", "extra"],
        vec!["routine", "history", "-n", "many"],
    ] {
        let refused = rig.fleet(&args);
        assert_eq!(
            refused.status.code(),
            Some(2),
            "{args:?} read {}",
            err(&refused)
        );
    }
}

/// `--now` and `--last-fired` are the two seams `check` takes, and both are
/// refused unless they are stamps in the one shape this fleet writes.
#[test]
fn check_takes_the_instant_and_the_last_firing_from_the_caller() {
    let rig = Rig::new("stamps");
    rig.routine(
        "nightly",
        "[order]\ndescription = \"the nightly sweep\"\ntrigger = \"cron\"\n\
         schedule = \"* * * * *\"\n[action.exec]\ncommand = \"true\"\n",
    );

    let at = "2026-09-05T09:20:00Z";
    let fresh = rig.fleet(&["routine", "check", "nightly", "--now", at]);
    assert_eq!(fresh.status.code(), Some(0), "{}", err(&fresh));

    // The same instant, with the routine already fired in that minute.
    let fired = rig.fleet(&[
        "routine",
        "check",
        "nightly",
        "--now",
        at,
        "--last-fired",
        "2026-09-05T09:20:30Z",
    ]);
    assert_eq!(fired.status.code(), Some(1), "{}", out(&fired));
    assert!(
        out(&fired).contains("already fired in that minute"),
        "{}",
        out(&fired)
    );

    let bad = rig.fleet(&["routine", "check", "nightly", "--now", "yesterday"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(err(&bad).contains("is not a UTC stamp"));
}

/// The family's old name, for one release: usage, with the rewrite on stderr
/// as one line, whatever followed it — a script that still types `fleet order`
/// reads exit 2 and the one line that helps.
#[test]
fn the_old_family_name_is_refused_with_the_pointer_to_routine() {
    let rig = Rig::new("old-name");
    rig.routine("beat", &ticking("beat.txt"));
    for args in [
        vec!["order"],
        vec!["order", "list"],
        vec!["order", "run", "beat", "--force"],
        vec!["order", "--help"],
    ] {
        let refused = rig.fleet(&args);
        assert_eq!(
            refused.status.code(),
            Some(2),
            "{args:?} read {}",
            err(&refused)
        );
        assert_eq!(
            err(&refused).lines().count(),
            1,
            "one line: {}",
            err(&refused)
        );
        assert!(
            err(&refused).contains("use `fleet routine list | check | run | history`"),
            "{args:?}: {}",
            err(&refused)
        );
        assert!(out(&refused).is_empty(), "{args:?} printed a row");
    }
    assert!(
        !rig.fleet_root().join("beat.txt").exists(),
        "the old name never fired anything"
    );
    // The control: the same words under the new name are the verb.
    let listed = rig.fleet(&["routine", "list"]);
    assert_eq!(listed.status.code(), Some(0), "{}", err(&listed));
    assert!(
        out(&listed).starts_with("beat  fleet  cooldown"),
        "{}",
        out(&listed)
    );
}

// ---- the run action, through the shipped binary ------------------------------

fn executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A scratch pack with one workflow, on the rig's machine: the binary's own
/// defaults materialized beside it for the runtime check, a runtime stub where
/// the CONSTRUCTED child
/// path looks (`~/.local/bin`, never this process's `PATH`), and a board in
/// the fleet root for the run's record.
fn scratch_pack(rig: &Rig, workflow: &str, script: &str) {
    let defaults = rig.machine().join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&defaults).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&defaults).expect("the embedded defaults are written");

    let runtime = rig.root.join("home/.local/bin/rt-runtime");
    write(&runtime, "#!/bin/sh\necho \"rt-runtime 1.0.0\"\n");
    executable(&runtime);

    let scratch = rig.machine().join("packs/scratch");
    let bundler = scratch.join("assets/bundle.sh");
    write(&bundler, "#!/bin/sh\nset -eu\ncp \"$1\" \"$2\"\n");
    executable(&bundler);
    write(&scratch.join(format!("workflows/{workflow}.sh")), script);
    write(
        &scratch.join("pack.toml"),
        &format!(
            "[pack]\nname = \"scratch\"\nversion = \"0.1.0\"\nschema = 3\n\
             description = \"a scratch pack\"\n\n[runtime]\nname = \"rt-runtime\"\n\
             version = \"1.0.0\"\nbundle = \"sh {} {{entry}} {{bundle}}\"\nrun = \"sh {{bundle}}\"\n",
            bundler.display()
        ),
    );
    common::take_a_board(&rig.fleet_root(), "routines");
}

fn stream_of(rig: &Rig) -> Vec<serde_json::Value> {
    rig.events()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
        .collect()
}

/// AC2 through the real verb: `fleet routine run` on a routine whose action
/// is `[action.run]` opens a run of the scratch pack's workflow, with the
/// routine's events wrapping the run's on the one stream — `routine.fired`
/// first, then `run.started` naming the routine as its runner, `run.closed`,
/// and `routine.completed` last carrying the run id.
#[test]
fn a_run_action_fires_the_run_verb_against_a_scratch_pack_and_the_events_wrap() {
    let rig = Rig::new("run-action");
    scratch_pack(
        &rig,
        "rt-hello",
        "#!/bin/sh\necho ran > \"$FLEET_DIR/witness.txt\"\ncat > \"$FLEET_DIR/stdin.txt\"\n",
    );
    rig.routine(
        "takeoff",
        "[order]\ndescription = \"run the scratch workflow\"\ntrigger = \"cooldown\"\n\
         interval = \"1h\"\n[action.run]\nworkflow = \"rt-hello\"\n\
         [action.run.inputs]\nwho = \"the-arm\"\n",
    );

    let fired = rig.fleet(&["routine", "run", "takeoff"]);
    assert_eq!(
        fired.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        out(&fired),
        err(&fired)
    );
    let row = out(&fired);
    assert!(row.contains("routine.completed"), "{row}");
    assert!(row.contains("ran — "), "{row}");
    assert_eq!(
        std::fs::read_to_string(rig.machine().join("witness.txt")).unwrap(),
        "ran\n",
        "the workflow ran"
    );

    let stream = stream_of(&rig);
    let kinds: Vec<&str> = stream
        .iter()
        .map(|event| event["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "routine.fired",
            "run.started",
            "run.closed",
            "routine.completed"
        ],
        "{stream:?}"
    );
    let run_id = stream[1]["payload"]["run"]
        .as_str()
        .expect("run.started names the run")
        .to_string();
    assert!(run_id.starts_with("fx-"), "{run_id}");
    assert_eq!(stream[1]["actor"], "takeoff", "the routine is the runner");
    assert_eq!(stream[1]["payload"]["workflow"], "rt-hello");
    assert_eq!(stream[2]["payload"]["run"], run_id.as_str());
    assert!(
        stream[0]["payload"].get("run").is_none(),
        "routine.fired carries no run id: {}",
        stream[0]
    );
    assert_eq!(stream[3]["payload"]["order"], "takeoff");
    assert_eq!(stream[3]["payload"]["outcome"], "ran");
    assert_eq!(stream[3]["payload"]["run"], run_id.as_str());
    assert_eq!(stream[3]["payload"]["by"], "run");

    // The inputs reached the run: the pinned file under the run directory.
    let pinned =
        std::fs::read_to_string(rig.machine().join("runs").join(&run_id).join("inputs.toml"))
            .unwrap();
    assert!(pinned.contains("who = \"the-arm\""), "{pinned}");
    let stdin = std::fs::read_to_string(rig.machine().join("stdin.txt")).unwrap();
    assert!(stdin.contains("the-arm"), "{stdin}");

    // The history reads the routine's two rows and neither of the run's.
    let history = rig.fleet(&["routine", "history", "takeoff"]);
    assert_eq!(history.status.code(), Some(0));
    let printed = out(&history);
    let rows: Vec<&str> = printed.lines().collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[1].contains(&run_id), "{rows:?}");

    // A dry run prints the verb's argv and opens nothing.
    let before = stream.len();
    let dry = rig.fleet(&["routine", "run", "takeoff", "--dry-run", "--force"]);
    assert_eq!(dry.status.code(), Some(0), "{}", err(&dry));
    let body = out(&dry);
    let verb = body
        .lines()
        .find(|line| line.starts_with("argv /"))
        .unwrap_or_else(|| panic!("the argv names an absolute fleet: {body}"));
    assert!(verb.ends_with("/fleet"), "{verb}");
    assert!(
        body.contains(
            "argv run\nargv rt-hello\nargv --input\nargv who=the-arm\nargv --by\nargv takeoff\n"
        ),
        "{body}"
    );
    assert_eq!(stream_of(&rig).len(), before, "a dry run opened a run");
}
