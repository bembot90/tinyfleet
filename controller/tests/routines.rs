//! The routine file, the three roots and the clock.
//!
//! Every arm here is offline: the loader is a pure function over a list of
//! directories, and the trigger takes the instant and the check runner as
//! arguments, so nothing below asks this machine what time it is or runs a
//! command it did not write.

use fleet_controller::routines::file::{self, Action, Loaded, Routine, Source, When};
use fleet_controller::routines::load;
use fleet_controller::routines::trigger::{self, CheckOutcome, Due, LocalMinute, Trigger};
use fleet_core::seat::identity::{Directory, Kind, SeatId, SeatRef};
use std::path::{Path, PathBuf};

/// The fixed ids this suite's seats carry.
const BUILDER: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const ARCHITECT: &str = "01a0d1f1-0aec-765f-9abe-0000000a90e5";
const ORLA: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

fn seat_ref(id: &str, name: &str) -> SeatRef {
    SeatRef {
        id: SeatId::parse(id).expect("a hand-written seat id parses"),
        name: Some(name.to_string()),
        kind: Kind::Agent,
    }
}

/// A person the fleet lists and no machine runs.
const ALBERTO: &str = "01a0d1f1-0aec-765f-9abe-00000a1be270";

/// The seats every arm validates a routine against, unless it says otherwise:
/// the three rows this machine runs, and a person the fleet lists besides. A
/// ring names its seat the way a person would, so every row carries a name.
fn seats() -> Directory {
    let running = vec![
        seat_ref(BUILDER, "builder-1"),
        seat_ref(ARCHITECT, "architect"),
        seat_ref(ORLA, "Orla"),
    ];
    let mut listed = running.clone();
    listed.push(SeatRef {
        kind: Kind::Human,
        ..seat_ref(ALBERTO, "Alberto")
    });
    Directory { listed, running }
}

/// One file's read, with the whole file's text passed in.
fn read(body: &str) -> Loaded {
    file::parse(
        "a-routine",
        Path::new("/orders/a-routine.toml"),
        &Source::Fleet,
        Path::new("/project"),
        body,
        &seats(),
    )
}

fn routine_of(body: &str) -> Routine {
    match read(body) {
        Loaded::Routine(routine) => *routine,
        Loaded::Defective(defect) => panic!("expected a routine: {}", defect.reason()),
    }
}

/// The reasons one file was refused, joined as the printer joins them.
fn refusal(body: &str) -> String {
    match read(body) {
        Loaded::Routine(_) => panic!("expected a defect, and the file read as a routine"),
        Loaded::Defective(defect) => defect.reason(),
    }
}

/// A whole routine with every default in place, for an arm that varies one line.
const CRON_EXEC: &str = r#"
[order]
description = "sweep the branches"
trigger = "cron"
schedule = "0 3 * * *"

[action.exec]
command = "sweep --all"
"#;

// ---- §1, the routine file -----------------------------------------------------

/// Every default the file does not name, read off a file that names none of
/// them. A reader that dropped one would leave a duty with no bound at all.
#[test]
fn a_file_that_names_only_what_it_must_carries_the_defaults() {
    let routine = routine_of(CRON_EXEC);
    assert_eq!(routine.name, "a-routine");
    assert_eq!(routine.trigger, Trigger::Cron);
    assert_eq!(routine.schedule.as_deref(), Some("0 3 * * *"));
    assert_eq!(routine.description, "sweep the branches");
    assert!(routine.enabled, "a file that says nothing is enabled");
    assert_eq!(routine.timeout, file::DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(routine.check_timeout, file::DEFAULT_CHECK_TIMEOUT_SECONDS);
    assert_eq!(routine.poll, file::DEFAULT_POLL_SECONDS);
    assert!(routine.check_unknown_exit.is_empty());
    assert_eq!(routine.action.kind(), "exec");
    assert_eq!(routine.project_root, PathBuf::from("/project"));
    assert_eq!(routine.source.as_string(), "fleet");
}

/// The unattended-fleet example in the workspace README, parsed through this
/// same loader. Core ships no routine that composes or flies, so this file is
/// the one a person writes.
///
/// THE TEXT IS READ OUT OF THE DOCUMENT and never copied here: an example
/// beside a format is a claim about that format, and a copy of it in this file
/// would be a claim about a copy. The block is found by its fence under the
/// section's own heading, and the arm refuses an empty extraction before it
/// parses anything — a selector that matches nothing parses clean and reads as
/// a passing arm.
#[test]
fn the_readmes_unattended_example_parses_as_a_routine() {
    let readme = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the controller crate sits inside the workspace")
        .join("README.md");
    let body = std::fs::read_to_string(&readme).expect("the README is readable");
    let example = toml_block_under(&body, "## Flying unattended");
    assert!(
        example.contains("[action.run]"),
        "the example was extracted, not an empty string: {example:?}"
    );
    assert!(
        example.contains("workflow = \"takeoff\""),
        "the example is the one the section describes — it fires the takeoff workflow: {example}"
    );

    let routine = routine_of(&example);
    assert_eq!(routine.trigger, Trigger::Cron);
    assert_eq!(routine.schedule.as_deref(), Some("0 2 * * *"));
    assert_eq!(routine.action.kind(), "run");
    let run = routine
        .action
        .run
        .as_ref()
        .expect("the run action was read");
    assert_eq!(run.workflow, "takeoff");
    assert_eq!(run.inputs, vec![("ready".to_string(), "10".to_string())]);

    // The control: the same reader over a fence that is NOT the example's
    // answers something else, so the extraction above is reading the section it
    // names rather than the first fence in the file.
    let elsewhere = toml_block_under(&body, "## Layout");
    assert!(
        elsewhere.is_empty(),
        "the extractor is anchored on its own section: {elsewhere:?}"
    );
}

/// The first fenced `toml` block after a heading, or an empty string.
fn toml_block_under(body: &str, heading: &str) -> String {
    let mut lines = body.lines();
    let found = lines.any(|line| line.trim_end() == heading);
    if !found {
        return String::new();
    }
    let mut block = Vec::new();
    let mut inside = false;
    for line in lines {
        if line.starts_with("## ") {
            break;
        }
        if line.trim_end() == "```toml" {
            inside = true;
            continue;
        }
        if inside && line.trim_end() == "```" {
            break;
        }
        if inside {
            block.push(line);
        }
    }
    block.join("\n")
}

/// Each required key, absent, one arm apiece — and the reason names the key,
/// because a refusal a reader cannot act on is one nobody acts on.
#[test]
fn every_required_key_that_is_absent_is_refused_by_name() {
    for (body, needle) in [
        (
            "[order]\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n[action.exec]\ncommand = \"x\"\n",
            "carries no `description`",
        ),
        (
            "[order]\ndescription = \"d\"\n[action.exec]\ncommand = \"x\"\n",
            "carries no `trigger`",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"cron\"\n[action.exec]\ncommand = \"x\"\n",
            "carries no `schedule`",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"cooldown\"\n[action.exec]\ncommand = \"x\"\n",
            "carries no `interval`",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"condition\"\n[action.exec]\ncommand = \"x\"\n",
            "carries no `check`",
        ),
        (
            "[action.exec]\ncommand = \"x\"\n",
            "carries no [order] table",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n",
            "carries no [action.<kind>] table",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n[action.nudge]\ntext = \"t\"\nauthority = \"a\"\n",
            "[action.nudge] carries no `seat`",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n[action.item]\ndescription = \"d\"\n",
            "[action.item] carries no `title`",
        ),
        (
            "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n[action.exec]\n",
            "[action.exec] carries no `command`",
        ),
    ] {
        let why = refusal(body);
        assert!(why.contains(needle), "expected {needle:?}, read {why:?}");
    }
}

/// A trigger parameter under the wrong trigger, one arm per parameter. It is a
/// defect and never an ignored line: an `interval` on a cron routine is a plan its
/// author believes in and nothing reads.
#[test]
fn a_trigger_parameter_under_the_wrong_trigger_is_a_defect() {
    for (key, value, belongs, under) in [
        ("schedule", "\"* * * * *\"", "cron", "cooldown"),
        ("interval", "\"1m\"", "cooldown", "cron"),
        ("check", "\"true\"", "condition", "cron"),
        ("check_timeout", "\"5s\"", "condition", "cron"),
        ("check_unknown_exit", "[3]", "condition", "cron"),
        ("poll", "\"5m\"", "condition", "cron"),
    ] {
        let parameter = match under {
            "cron" => "schedule = \"* * * * *\"",
            _ => "interval = \"1m\"",
        };
        let body = format!(
            "[order]\ndescription = \"d\"\ntrigger = \"{under}\"\n{parameter}\n{key} = {value}\n\
             [action.exec]\ncommand = \"x\"\n"
        );
        let why = refusal(&body);
        assert!(
            why.contains(&format!(
                "carries `{key}`, which belongs to trigger `{belongs}`, under trigger `{under}`"
            )),
            "{key} under {under} read {why:?}"
        );
    }

    // The control: each of the six under its OWN trigger reads as a routine, so
    // the six refusals above are the trigger's and not the key's.
    let cron = routine_of(CRON_EXEC);
    assert_eq!(cron.trigger, Trigger::Cron);
    let cooldown = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"cooldown\"\ninterval = \"90s\"\n\
         [action.exec]\ncommand = \"x\"\n",
    );
    assert_eq!(cooldown.interval, Some(90));
    let condition = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"true\"\n\
         check_timeout = \"5s\"\ncheck_unknown_exit = [3]\npoll = \"5m\"\n\
         [action.exec]\ncommand = \"x\"\n",
    );
    assert_eq!(condition.check_timeout, 5);
    assert_eq!(condition.check_unknown_exit, vec![3]);
    assert_eq!(condition.poll, 300);
}

/// `check_unknown_exit` refuses the two values that mean nothing: 0, which is
/// the exit that means DUE, and an empty list, which is a mapping its author
/// wrote and nothing carries.
#[test]
fn the_unknown_exit_field_refuses_zero_and_an_empty_list() {
    let condition = |value: &str| {
        format!(
            "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"true\"\n\
             check_unknown_exit = {value}\n[action.exec]\ncommand = \"x\"\n"
        )
    };
    assert!(refusal(&condition("0")).contains("names 0, which is the exit that means due"));
    assert!(refusal(&condition("[3, 0]")).contains("names 0, which is the exit that means due"));
    assert!(refusal(&condition("[]")).contains("is an empty list"));
    assert!(refusal(&condition("\"3\"")).contains("not an integer or a list of integers"));

    // The control: the same field with a status that is not 0 reads.
    let routine = routine_of(&condition("[3, 4]"));
    assert_eq!(routine.check_unknown_exit, vec![3, 4]);
}

/// A RANGE in a cron field is refused with the list that replaces it, because a
/// schedule read leniently fires at a minute nobody wrote.
#[test]
fn a_range_in_a_cron_field_is_refused_with_the_rewrite() {
    let why = refusal(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"0 9-12 * * *\"\n\
         [action.exec]\ncommand = \"x\"\n",
    );
    assert!(why.contains("is a range in the hour field"), "{why}");
    assert!(
        why.contains("write it as `9,10,11,12`"),
        "the rewrite is in the defect: {why}"
    );

    // And the rewrite itself reads, which is what makes it a rewrite rather
    // than advice.
    let routine = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"0 9,10,11,12 * * *\"\n\
         [action.exec]\ncommand = \"x\"\n",
    );
    assert_eq!(routine.schedule.as_deref(), Some("0 9,10,11,12 * * *"));
}

/// Everything else a schedule can be that is not a schedule.
#[test]
fn a_schedule_that_is_not_five_readable_fields_is_refused() {
    let with = |schedule: &str| {
        format!(
            "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"{schedule}\"\n\
             [action.exec]\ncommand = \"x\"\n"
        )
    };
    assert!(refusal(&with("* * * *")).contains("carries 4 fields"));
    assert!(refusal(&with("* * * * * *")).contains("carries 6 fields"));
    assert!(refusal(&with("60 * * * *")).contains("is not a minute field"));
    assert!(refusal(&with("* 24 * * *")).contains("is not a hour field"));
    assert!(refusal(&with("* * 0 * *")).contains("is not a day-of-month field"));
    assert!(refusal(&with("* * * 13 *")).contains("is not a month field"));
    assert!(refusal(&with("* * * * 8")).contains("is not a day-of-week field"));
    assert!(refusal(&with("*/0 * * * *")).contains("is not a step"));
    assert!(refusal(&with("mon * * * *")).contains("is not a minute field"));
}

/// The action table: an unknown kind, and two actions that are not the one pair
/// allowed.
#[test]
fn an_unknown_action_kind_and_a_second_action_are_both_refused() {
    let head = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n";
    let why = refusal(&format!("{head}[action.courier]\nseat = \"builder-1\"\n"));
    assert!(
        why.contains("[action.courier] is not an action kind"),
        "{why}"
    );

    let both = refusal(&format!(
        "{head}[action.exec]\ncommand = \"x\"\n[action.item]\ntitle = \"t\"\n"
    ));
    assert!(both.contains("carries more than one action"), "{both}");

    let pair_without_absent = refusal(&format!(
        "{head}[action.nudge]\nseat = \"builder-1\"\ntext = \"t\"\nauthority = \"a\"\n\
         [action.item]\ntitle = \"t\"\n"
    ));
    assert!(
        pair_without_absent.contains("carries more than one action"),
        "an item that fires ALWAYS is not the fallback pair: {pair_without_absent}"
    );

    // The one pair allowed, which is the control for the three above.
    let pair = routine_of(&format!(
        "{head}[action.nudge]\nseat = \"builder-1\"\ntext = \"t\"\nauthority = \"a\"\n\
         [action.item]\ntitle = \"t\"\nwhen = \"absent\"\n"
    ));
    assert_eq!(pair.action.kind(), "nudge+item");
    assert_eq!(
        pair.action.item.as_ref().map(|item| item.when),
        Some(When::Absent)
    );
}

/// A ring names a row of THIS machine's seat list, or it is a ring nobody would
/// ever answer. The seat is RESOLVED AT LOAD, through the resolver every seat
/// argument takes: a name nobody holds refuses the file with the resolver's own
/// reason, and one that resolves is stored as the id it found.
#[test]
fn a_nudge_to_a_seat_this_machine_does_not_carry_is_refused() {
    let body = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
                [action.nudge]\nseat = \"nobody\"\ntext = \"t\"\nauthority = \"a\"\n";
    let why = refusal(body);
    assert!(
        why.contains("[action.nudge] seat is nobody, which nobody names no seat — the seats are "),
        "{why}"
    );
    assert!(why.contains("orla-93b9739a"), "the seats are listed: {why}");

    // The control: the same file naming a row that IS on the list reads, and
    // carries the id the name resolved to.
    let known = body.replace("nobody", "Orla");
    let routine = routine_of(&known);
    let nudge = routine.action.nudge.as_ref().expect("the nudge is read");
    assert_eq!(nudge.seat, "Orla");
    assert_eq!(
        nudge.seat_id.map(|id| id.to_string()).as_deref(),
        Some(ORLA)
    );
}

/// The ring goes to the ROW THE LOAD RESOLVED, found by its id: a nudge naming
/// `Orla` rings Orla's session, under the machine name her directory and her
/// session are keyed on, and never a neighbour's — the control is a second live
/// seat on the same machine that the ring passes by.
#[test]
fn a_nudge_to_a_seat_by_its_name_fires_at_that_seats_row() {
    use fleet_controller::observe::RosterState;
    use fleet_controller::policy;
    use fleet_controller::routines::{action, action::Machine, Outcome, SeatView};
    use fleet_controller::test_support::StubAgent;

    let routine = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
         [action.nudge]\nseat = \"Orla\"\ntext = \"t\"\nauthority = \"a\"\n",
    );
    let view = |id: &str, session_name: &str, worktree: &str| SeatView {
        id: SeatId::parse(id).unwrap(),
        session_name: session_name.to_string(),
        worktree: worktree.to_string(),
        state: RosterState::Present,
        config_dir: None,
    };
    let views = vec![
        view(BUILDER, "builder-1-e8a04b17", "/wt/builder"),
        view(ORLA, "orla-93b9739a", "/wt/orla"),
    ];
    let policy = policy::parse("").expect("the empty policy is the defaults");
    let agent = StubAgent::new();
    let root = scratch("nudge-by-name");
    let machine = Machine {
        machine_dir: &root,
        child_path: "/usr/bin:/bin",
        policy: &policy,
        seats: &views,
        agent: Some(&agent),
        effects_off: None,
    };

    let done = action::run(&routine, &machine, "2026-09-18T00:00:00Z");
    assert_eq!(done.outcome, Outcome::Delivered, "{}", done.detail);
    assert!(done.detail.contains("/wt/orla"), "{}", done.detail);
    let rung: Vec<String> = agent
        .calls_of(StubAgent::NUDGE)
        .into_iter()
        .map(|call| call.about)
        .collect();
    assert_eq!(rung, vec!["orla-93b9739a".to_string()], "only Orla is rung");

    // The dry run reads the same row.
    let argv = action::argv_of(&routine, &machine);
    assert_eq!(argv.last().map(String::as_str), Some("(in /wt/orla)"));
    let _ = std::fs::remove_dir_all(&root);
}

/// [ASSUMES D14] An item's assignee is RESOLVED AT LOAD among every seat the
/// fleet lists and every seat this machine runs, and the item is filed to the
/// FULL ID it found: `Orla` files to Orla's id, a person the fleet lists is a
/// seat an item may go to, and a name nobody holds refuses the file with the
/// resolver's own reason.
#[test]
fn an_item_assignee_is_filed_with_the_full_id_it_resolves_to() {
    use fleet_controller::policy;
    use fleet_controller::routines::{action, action::Machine};

    let head = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
                [action.item]\ntitle = \"t\"\n";
    let why = refusal(&format!("{head}assignee = \"nobody\"\n"));
    assert!(
        why.contains(
            "[action.item] assignee is nobody, which nobody names no seat — the seats are "
        ),
        "{why}"
    );

    let policy = policy::parse("").expect("the empty policy is the defaults");
    let root = scratch("item-assignee");
    let machine = Machine {
        machine_dir: &root,
        child_path: "/usr/bin:/bin",
        policy: &policy,
        seats: &[],
        agent: None,
        effects_off: None,
    };
    for (named, id) in [
        ("Orla", ORLA),
        ("orla-93b9739a", ORLA),
        ("alberto", ALBERTO),
    ] {
        let routine = routine_of(&format!("{head}assignee = \"{named}\"\n"));
        let item = routine.action.item.as_ref().expect("the item reads");
        assert_eq!(item.assignee.as_deref(), Some(named), "the file's own word");
        assert_eq!(
            item.assignee_id.map(|found| found.to_string()).as_deref(),
            Some(id),
            "{named} resolves"
        );
        let argv = action::argv_of(&routine, &machine);
        let at = argv
            .iter()
            .position(|arg| arg == "--assignee")
            .unwrap_or_else(|| panic!("{named}: the create names an assignee: {argv:?}"));
        assert_eq!(
            argv[at + 1],
            id,
            "{named}: filed with the full id: {argv:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// The item table's own vocabulary, each value refused by name.
#[test]
fn an_item_refuses_a_priority_a_type_a_when_and_a_dedupe_it_does_not_know() {
    let head = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
                [action.item]\ntitle = \"t\"\n";
    assert!(refusal(&format!("{head}priority = 5\n")).contains("priority is `5`"));
    assert!(refusal(&format!("{head}type = \"story\"\n")).contains("type is `story`"));
    assert!(refusal(&format!("{head}when = \"sometimes\"\n")).contains("when is `sometimes`"));
    assert!(refusal(&format!("{head}dedupe = \"all\"\n")).contains("dedupe is `all`"));
    assert!(refusal(&format!("{head}labels = \"one\"\n")).contains("labels is a string"));

    let routine = routine_of(&format!(
        "{head}priority = 3\ntype = \"task\"\nwhen = \"always\"\ndedupe = \"open\"\n\
         labels = [\"lane\", \"surface\"]\nassignee = \"builder-1\"\ndescription = \"why\"\n"
    ));
    let item = routine.action.item.expect("the item reads");
    assert_eq!(item.priority, Some(3));
    assert_eq!(item.kind.as_deref(), Some("task"));
    assert_eq!(item.when, When::Always);
    assert_eq!(item.dedupe.as_deref(), Some("open"));
    assert_eq!(item.labels, vec!["lane", "surface"]);
    assert_eq!(item.assignee.as_deref(), Some("builder-1"));
    assert_eq!(item.description.as_deref(), Some("why"));
}

/// Two tables and no third, and no key inside them this reader does not
/// understand. A misspelled key is a duty that silently never does what its
/// author wrote.
#[test]
fn a_third_table_and_an_unknown_key_are_both_defects() {
    let head = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n";
    assert!(refusal(&format!(
        "{head}[action.exec]\ncommand = \"x\"\n[owner]\nname = \"a-seat\"\n"
    ))
    .contains("carries a `owner` table"));
    assert!(refusal(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\nowner = \"a\"\n\
         [action.exec]\ncommand = \"x\"\n"
    )
    .contains("[order] carries an unknown key `owner`"));
    assert!(refusal(&format!(
        "{head}[action.exec]\ncommand = \"x\"\nshell = \"bash\"\n"
    ))
    .contains("[action.exec] carries an unknown key `shell`"));
    assert!(refusal("this is not TOML at all\n").contains("is not valid TOML"));
}

/// The description is one line and never a paragraph: it is what `list` prints
/// beside a name.
#[test]
fn a_description_is_one_non_empty_line() {
    let with = |description: &str| {
        format!(
            "[order]\ndescription = {description}\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
             [action.exec]\ncommand = \"x\"\n"
        )
    };
    assert!(refusal(&with("\"\"")).contains("is empty or spans more than one line"));
    assert!(
        refusal(&with("\"\"\"\none\ntwo\n\"\"\"")).contains("is empty or spans more than one line")
    );
    assert!(refusal(&with("3")).contains("description is an integer"));
}

/// The duration form, and the shapes that are not it.
#[test]
fn a_duration_is_an_integer_and_one_unit() {
    assert_eq!(file::parse_duration("30s"), Ok(30));
    assert_eq!(file::parse_duration("15m"), Ok(900));
    assert_eq!(file::parse_duration("6h"), Ok(21_600));
    assert_eq!(file::parse_duration("1d"), Ok(86_400));
    for bad in ["", "s", "30", "30x", "-1s", "1.5h", "30 s", "thirty"] {
        assert!(file::parse_duration(bad).is_err(), "{bad:?} is no duration");
    }
}

/// The file's own name is the routine's name, and a name this registry cannot
/// print is a defect rather than a name it invents.
#[test]
fn a_routine_is_named_by_its_file_and_the_name_is_lower_case() {
    assert_eq!(
        file::routine_name_of(Path::new("/o/stale-branches.toml")),
        Ok("stale-branches".to_string())
    );
    for bad in [
        "/o/Stale.toml",
        "/o/one_two.toml",
        "/o/.toml",
        "/o/a b.toml",
    ] {
        assert!(file::routine_name_of(Path::new(bad)).is_err(), "{bad}");
    }
}

// ---- §2, the three roots ----------------------------------------------------

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleet-routines-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn an_exec_routine(what: &str) -> String {
    format!(
        "[order]\ndescription = \"{what}\"\ntrigger = \"cooldown\"\ninterval = \"1h\"\n\
         [action.exec]\ncommand = \"true\"\n"
    )
}

/// One arm per source label, over three scratch roots: the fleet install, an
/// installed pack, and a project.
#[test]
fn every_routine_carries_the_root_it_was_loaded_from() {
    let dir = scratch("sources");
    let fleet_root = dir.join("fleet");
    let machine = dir.join("machine");
    let project = dir.join("app");
    write(
        &fleet_root.join("orders").join("from-fleet.toml"),
        &an_exec_routine("the fleet's own"),
    );
    write(
        &machine
            .join("packs")
            .join("tiny")
            .join("orders")
            .join("from-pack.toml"),
        &an_exec_routine("a pack's"),
    );
    write(
        &project.join("orders").join("from-project.toml"),
        &an_exec_routine("a project's"),
    );

    let roots = load::roots(
        &fleet_root,
        &machine,
        &[("app".to_string(), project.clone())],
    );
    let registry = load::load(&roots, &seats());
    let sources: Vec<String> = registry
        .routines
        .iter()
        .map(|routine| format!("{} {}", routine.name, routine.source.as_string()))
        .collect();
    assert_eq!(
        sources,
        vec![
            "from-fleet fleet".to_string(),
            "from-pack pack:tiny".to_string(),
            "from-project project:app".to_string(),
        ],
        "each routine names the root it came from"
    );
    assert!(registry.defects.is_empty(), "{:?}", registry.defects);

    // A project's routines run in the project, and a pack's run where the fleet
    // does: a pack is a library of duties and carries no checkout.
    let root_of = |name: &str| registry.get(name).expect(name).project_root.clone();
    assert_eq!(root_of("from-project"), project);
    assert_eq!(root_of("from-pack"), fleet_root);
    assert_eq!(root_of("from-fleet"), fleet_root);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Two files with one name refuse BOTH, and the defect names both paths.
/// Picking one silently is how a duty runs from a file nobody is looking at.
#[test]
fn a_name_in_two_roots_refuses_both_files() {
    let dir = scratch("duplicate");
    let fleet_root = dir.join("fleet");
    let machine = dir.join("machine");
    let first = fleet_root.join("orders").join("sweep.toml");
    let second = machine
        .join("packs")
        .join("tiny")
        .join("orders")
        .join("sweep.toml");
    write(&first, &an_exec_routine("the fleet's"));
    write(&second, &an_exec_routine("the pack's"));

    let registry = load::load(&load::roots(&fleet_root, &machine, &[]), &seats());
    assert!(
        registry.routines.is_empty(),
        "neither file is a routine this fleet acts on: {:?}",
        registry
            .routines
            .iter()
            .map(|o| &o.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(registry.defects.len(), 2, "both files are refused");
    for defect in &registry.defects {
        assert!(defect.reason().contains(&first.display().to_string()));
        assert!(defect.reason().contains(&second.display().to_string()));
    }

    // The control: with the second file gone the first one is a routine, so the
    // refusal above is the duplicate's and not the file's.
    std::fs::remove_file(&second).unwrap();
    let alone = load::load(&load::roots(&fleet_root, &machine, &[]), &seats());
    assert_eq!(alone.routines.len(), 1);
    assert!(alone.defects.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

/// An embedded fleet's root and its one project are the same directory, and
/// listing it twice would make every routine in it a duplicate of itself.
#[test]
fn one_directory_is_listed_once_however_many_roots_name_it() {
    let dir = scratch("embedded");
    let fleet_root = dir.join("fleet");
    write(
        &fleet_root.join("orders").join("sweep.toml"),
        &an_exec_routine("the embedded project's"),
    );
    let roots = load::roots(
        &fleet_root,
        &dir.join("machine"),
        &[("fleet".to_string(), fleet_root.clone())],
    );
    assert_eq!(roots.len(), 1, "one directory, one root");
    let registry = load::load(&roots, &seats());
    assert_eq!(registry.routines.len(), 1);
    assert!(registry.defects.is_empty(), "{:?}", registry.defects);
    let _ = std::fs::remove_dir_all(&dir);
}

/// One broken file does not hide the rest of the registry, and a directory
/// that is not there is an empty read rather than a failure.
#[test]
fn a_defective_file_stands_beside_the_routines_that_read() {
    let dir = scratch("mixed");
    let fleet_root = dir.join("fleet");
    write(
        &fleet_root.join("orders").join("good.toml"),
        &an_exec_routine("fine"),
    );
    write(
        &fleet_root.join("orders").join("bad.toml"),
        "[order]\ntrigger = \"cron\"\n",
    );
    write(
        &fleet_root.join("orders").join("notes.md"),
        "not a routine\n",
    );
    let registry = load::load(
        &load::roots(&fleet_root, &dir.join("nowhere"), &[]),
        &seats(),
    );
    assert_eq!(registry.routines.len(), 1);
    assert_eq!(registry.routines[0].name, "good");
    assert_eq!(registry.defects.len(), 1, "only the toml files are read");
    assert_eq!(registry.defects[0].name, "bad");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- §3, the clock ----------------------------------------------------------

fn at(minute: i8, hour: i8, day: i8, month: i8, weekday: i8) -> LocalMinute {
    LocalMinute {
        minute,
        hour,
        day,
        month,
        weekday,
    }
}

/// Each field form, over an injected local minute: the star, the step, one
/// integer and a comma list.
#[test]
fn each_cron_field_form_matches_the_minute_it_names() {
    let noon = at(30, 12, 15, 6, 1);
    assert_eq!(trigger::cron_matches("* * * * *", &noon), Ok(true));
    assert_eq!(trigger::cron_matches("30 12 15 6 1", &noon), Ok(true));
    assert_eq!(trigger::cron_matches("31 12 15 6 1", &noon), Ok(false));
    assert_eq!(trigger::cron_matches("0,30 12 15 6 1", &noon), Ok(true));
    assert_eq!(trigger::cron_matches("0,29 * * * *", &noon), Ok(false));
    assert_eq!(trigger::cron_matches("*/15 * * * *", &noon), Ok(true));
    assert_eq!(trigger::cron_matches("*/7 * * * *", &noon), Ok(false));
    assert_eq!(trigger::cron_matches("* */6 * * *", &noon), Ok(true));
    assert_eq!(trigger::cron_matches("* * * 6 *", &noon), Ok(true));
    assert_eq!(trigger::cron_matches("* * * 7 *", &noon), Ok(false));
    assert!(trigger::cron_matches("1-5 * * * *", &noon).is_err());
}

/// Sunday is 0 and 7 alike, and no other day answers to both.
#[test]
fn sunday_is_zero_and_seven_and_no_other_day_is_two_numbers() {
    let sunday = at(0, 0, 1, 1, 0);
    assert_eq!(trigger::cron_matches("* * * * 0", &sunday), Ok(true));
    assert_eq!(trigger::cron_matches("* * * * 7", &sunday), Ok(true));
    assert_eq!(trigger::cron_matches("* * * * 1", &sunday), Ok(false));

    let monday = at(0, 0, 2, 1, 1);
    assert_eq!(trigger::cron_matches("* * * * 1", &monday), Ok(true));
    assert_eq!(trigger::cron_matches("* * * * 0", &monday), Ok(false));
    assert_eq!(trigger::cron_matches("* * * * 7", &monday), Ok(false));
}

/// The instant is read AS ITS MINUTE: every second inside one minute is the
/// same reading, and the second after it is a different one.
#[test]
fn an_instant_is_read_as_the_minute_it_falls_in() {
    // A minute boundary in UTC is one in every zone whose offset is whole
    // minutes, which is every zone this reads.
    let boundary = 1_788_600_000;
    assert_eq!(boundary % 60, 0, "the fixture starts on a minute");
    let first = trigger::local_minute_of(boundary).expect("this machine has a zone");
    for inside in [1, 30, 59] {
        assert_eq!(
            trigger::local_minute_of(boundary + inside),
            Ok(first),
            "second {inside} of the minute reads as the same minute"
        );
    }
    let next = trigger::local_minute_of(boundary + 60).expect("this machine has a zone");
    assert_ne!(next, first, "the minute after is a different reading");
    assert_eq!(next.minute, (first.minute + 1) % 60);
}

/// A routine with no action to run, for an arm about the trigger alone.
fn trigger_routine(body: &str) -> Routine {
    routine_of(body)
}

const EVERY_MINUTE: &str = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\n\
                            schedule = \"* * * * *\"\n[action.exec]\ncommand = \"true\"\n";

fn never_run(_: &Routine) -> CheckOutcome {
    panic!("the evaluation ran a check for a trigger that has none")
}

/// A cron routine that matched and already fired in that minute does not fire
/// twice, and the same routine in the NEXT minute does.
#[test]
fn a_cron_routine_fires_once_in_the_minute_it_matches() {
    let routine = trigger_routine(EVERY_MINUTE);
    let now = 1_788_600_030;
    assert!(matches!(
        trigger::evaluate(&routine, now, None, &never_run),
        Due::Due(_)
    ));
    let inside = trigger::evaluate(&routine, now, Some(now - 20), &never_run);
    assert!(
        matches!(inside, Due::NotDue(ref why) if why.contains("already fired in that minute")),
        "{inside:?}"
    );
    // The same last_fired, one minute later: the suppression is the MINUTE's
    // and not a fixed span.
    assert!(matches!(
        trigger::evaluate(&routine, now + 60, Some(now - 20), &never_run),
        Due::Due(_)
    ));
}

/// A cooldown fires when it has never fired, and again once its interval has
/// passed.
#[test]
fn a_cooldown_fires_when_it_never_has_and_when_its_interval_is_up() {
    let routine = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"cooldown\"\ninterval = \"1h\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    let now = 1_788_600_000;
    assert!(matches!(
        trigger::evaluate(&routine, now, None, &never_run),
        Due::Due(ref why) if why.contains("never fired")
    ));
    assert!(matches!(
        trigger::evaluate(&routine, now, Some(now - 3_599), &never_run),
        Due::NotDue(_)
    ));
    assert!(matches!(
        trigger::evaluate(&routine, now, Some(now - 3_600), &never_run),
        Due::Due(_)
    ));
}

/// The seven arms of the condition trigger, through an injected runner. Three
/// answers and never two: the fourth arm is the one that says a status the
/// routine names is read BEFORE the 0 test.
#[test]
fn the_condition_trigger_answers_three_ways_and_reads_the_named_status_first() {
    let routine = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"probe\"\n\
         check_unknown_exit = [3]\n[action.exec]\ncommand = \"true\"\n",
    );
    let now = 1_788_600_000;
    let answer = |outcome: CheckOutcome| {
        let run = move |_: &Routine| outcome.clone();
        trigger::evaluate(&routine, now, None, &run)
    };

    assert!(matches!(answer(CheckOutcome::Exited(0)), Due::Due(_)));
    assert!(matches!(answer(CheckOutcome::Exited(1)), Due::NotDue(_)));
    assert!(matches!(
        answer(CheckOutcome::Exited(3)),
        Due::CouldNotTell(ref why) if why.contains("maps to could-not-tell")
    ));
    assert!(matches!(
        answer(CheckOutcome::Timeout("5s".to_string())),
        Due::CouldNotTell(ref why) if why.contains("did not finish within")
    ));
    assert!(matches!(
        answer(CheckOutcome::NotStarted("no such file".to_string())),
        Due::CouldNotTell(ref why) if why.contains("could not be run")
    ));

    // Disabled is NOT-DUE and never could-not-tell: nothing was unreadable.
    let off = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"probe\"\n\
         enabled = false\n[action.exec]\ncommand = \"true\"\n",
    );
    assert!(matches!(
        trigger::evaluate(&off, now, None, &never_run),
        Due::NotDue(ref why) if why.contains("disabled")
    ));

    // The ordering, on the one input that separates the two readings: a status
    // the routine maps to could-not-tell that is ALSO the due status. The file
    // reader refuses a 0 there, so this routine is built rather than parsed —
    // which is what makes the clause a rule of the evaluation and not of the parser.
    let ambiguous = Routine {
        check_unknown_exit: vec![0],
        ..trigger_routine(
            "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"probe\"\n\
             [action.exec]\ncommand = \"true\"\n",
        )
    };
    let zero = |_: &Routine| CheckOutcome::Exited(0);
    assert!(
        matches!(
            trigger::evaluate(&ambiguous, now, None, &zero),
            Due::CouldNotTell(_)
        ),
        "a named status is read before the 0 test"
    );
    assert!(
        refusal(
            "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"probe\"\n\
             check_unknown_exit = [0]\n[action.exec]\ncommand = \"true\"\n"
        )
        .contains("names 0"),
        "and the file reader is what keeps that routine out of a registry"
    );
}

/// `next_due` for each trigger, and the cron search's own end.
#[test]
fn the_next_due_of_each_trigger_is_the_instant_a_reader_can_act_on() {
    let now = 1_788_600_000;
    let every_minute = trigger_routine(EVERY_MINUTE);
    assert_eq!(trigger::next_due(&every_minute, now, None, None), Some(now));
    assert_eq!(
        trigger::next_due(&every_minute, now, Some(now), None),
        Some(now + 60),
        "a minute this routine already fired in is behind it"
    );

    let cooldown = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"cooldown\"\ninterval = \"1h\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    assert_eq!(trigger::next_due(&cooldown, now, None, None), Some(now));
    assert_eq!(
        trigger::next_due(&cooldown, now, Some(now - 100), None),
        Some(now - 100 + 3_600)
    );

    let condition = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"probe\"\n\
         poll = \"5m\"\n[action.exec]\ncommand = \"true\"\n",
    );
    assert_eq!(trigger::next_due(&condition, now, None, None), Some(now));
    assert_eq!(
        trigger::next_due(&condition, now, None, Some(now - 60)),
        Some(now - 60 + 300)
    );

    // A schedule with no matching minute at all answers none rather than a date
    // nobody meant. The thirtieth of February is the case, and the search stops
    // at the limit rather than running forever.
    let never = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"0 0 30 2 *\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    assert_eq!(trigger::next_due(&never, now, None, None), None);

    // The control that the search reaches far without being unbounded: a
    // schedule that matches once a year is found.
    let yearly = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"0 0 1 1 *\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    let next = trigger::next_due(&yearly, now, None, None).expect("a yearly schedule has a next");
    assert!(next > now, "the next new year is ahead of {now}");
    assert!(
        next - now < trigger::SEARCH_LIMIT_MINUTES * 60,
        "and inside the search limit"
    );
}

/// How often each trigger is asked at all. A cron routine is asked once per LOCAL
/// MINUTE and not once per sixty seconds, because a span drifts and skips the
/// minute its author wrote.
#[test]
fn each_trigger_is_asked_no_more_often_than_it_can_answer() {
    let now = 1_788_600_000;
    let cron = trigger_routine(EVERY_MINUTE);
    assert!(trigger::is_due_an_evaluation(&cron, now, None));
    assert!(!trigger::is_due_an_evaluation(&cron, now + 59, Some(now)));
    assert!(trigger::is_due_an_evaluation(&cron, now + 60, Some(now)));
    // The drift a span would have: evaluated one second before the boundary,
    // the next minute is still asked about.
    assert!(trigger::is_due_an_evaluation(
        &cron,
        now + 60,
        Some(now + 59)
    ));

    let cooldown = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"cooldown\"\ninterval = \"1h\"\n\
         [action.exec]\ncommand = \"true\"\n",
    );
    assert!(!trigger::is_due_an_evaluation(
        &cooldown,
        now + 59,
        Some(now)
    ));
    assert!(trigger::is_due_an_evaluation(
        &cooldown,
        now + 60,
        Some(now)
    ));

    let condition = trigger_routine(
        "[order]\ndescription = \"d\"\ntrigger = \"condition\"\ncheck = \"probe\"\n\
         poll = \"5m\"\n[action.exec]\ncommand = \"true\"\n",
    );
    assert!(!trigger::is_due_an_evaluation(
        &condition,
        now + 299,
        Some(now)
    ));
    assert!(trigger::is_due_an_evaluation(
        &condition,
        now + 300,
        Some(now)
    ));
}

/// A disabled routine is not-due whatever its trigger says, which is the first
/// line of the evaluation and not a branch inside each one.
#[test]
fn a_disabled_routine_is_not_due_under_every_trigger() {
    for body in [
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
         enabled = false\n[action.exec]\ncommand = \"true\"\n",
        "[order]\ndescription = \"d\"\ntrigger = \"cooldown\"\ninterval = \"1s\"\n\
         enabled = false\n[action.exec]\ncommand = \"true\"\n",
    ] {
        let routine = trigger_routine(body);
        assert!(matches!(
            trigger::evaluate(&routine, 1_788_600_000, None, &never_run),
            Due::NotDue(ref why) if why.contains("disabled")
        ));
    }
}

/// The action kind, as `list` and a dry run print it — including the empty one,
/// which no loaded routine carries and which the printer still has to answer for.
#[test]
fn the_action_kind_names_what_the_routine_will_do() {
    assert_eq!(Action::default().kind(), "exec");
    assert_eq!(routine_of(CRON_EXEC).action.kind(), "exec");
    let item = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
         [action.item]\ntitle = \"t\"\n",
    );
    assert_eq!(item.action.kind(), "item");
    let nudge = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
         [action.nudge]\nseat = \"builder-1\"\ntext = \"t\"\nauthority = \"a\"\n",
    );
    assert_eq!(nudge.action.kind(), "nudge");
}

/// The fourth kind: a workflow by name and its inputs, each a string. The
/// inputs come out in key order, which is the order the argv carries them.
#[test]
fn a_run_action_names_a_workflow_and_pins_its_inputs_as_strings() {
    let head = "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n";
    let run = routine_of(&format!(
        "{head}[action.run]\nworkflow = \"takeoff\"\n[action.run.inputs]\nwho = \"the-arm\"\n\
         board = \"main\"\n"
    ));
    assert_eq!(run.action.kind(), "run");
    let action = run.action.run.as_ref().expect("the run action is read");
    assert_eq!(action.workflow, "takeoff");
    assert_eq!(
        action.inputs,
        vec![
            ("board".to_string(), "main".to_string()),
            ("who".to_string(), "the-arm".to_string())
        ]
    );

    // No inputs at all is a run with none, not a defect.
    let bare = routine_of(&format!("{head}[action.run]\nworkflow = \"takeoff\"\n"));
    assert!(bare
        .action
        .run
        .as_ref()
        .is_some_and(|run| run.inputs.is_empty()));

    let no_workflow = refusal(&format!(
        "{head}[action.run]\n[action.run.inputs]\nwho = \"x\"\n"
    ));
    assert!(
        no_workflow.contains("[action.run] carries no `workflow`"),
        "{no_workflow}"
    );

    let bare_number = refusal(&format!(
        "{head}[action.run]\nworkflow = \"takeoff\"\n[action.run.inputs]\ncount = 3\n"
    ));
    assert!(
        bare_number.contains("[action.run.inputs] count is an integer, not a string"),
        "{bare_number}"
    );

    let not_a_table = refusal(&format!(
        "{head}[action.run]\nworkflow = \"takeoff\"\ninputs = \"who=x\"\n"
    ));
    assert!(
        not_a_table.contains("[action.run] inputs is a string, not a table of strings"),
        "{not_a_table}"
    );

    let unknown = refusal(&format!(
        "{head}[action.run]\nworkflow = \"takeoff\"\nformula = \"f\"\n"
    ));
    assert!(
        unknown.contains("[action.run] carries an unknown key `formula`"),
        "{unknown}"
    );

    // A run beside an exec is two actions, the same as any other pair.
    let two = refusal(&format!(
        "{head}[action.run]\nworkflow = \"takeoff\"\n[action.exec]\ncommand = \"x\"\n"
    ));
    assert!(two.contains("carries more than one action"), "{two}");
}

// ---- §4, the run action -----------------------------------------------------

/// A `fleet` on the child path that answers `run` the way the verb does: the
/// two lines on stdout, `run.started` and one terminal line on the stream
/// under the actor it was given, and the exit the terminal line maps to.
///
/// THE VERB IS A STUB HERE because this crate carries no fleet binary: the
/// contract measured is the action's — what it types, where, and what it reads
/// back — and the real verb behind the same argv is the cli suite's arm.
fn stub_fleet(dir: &Path) -> String {
    let script = dir.join("fleet");
    std::fs::write(
        &script,
        r#"#!/bin/sh
set -u
stream="$FLEET_DIR/events.jsonl"
printf '%s\n' "$@" > "$FLEET_DIR/fleet-argv.txt"
pwd > "$FLEET_DIR/fleet-cwd.txt"
for word; do by=$word; done
workflow=$2
last=$(tail -n 1 "$stream" 2>/dev/null | sed 's/.*"seq":\([0-9]*\).*/\1/')
[ -n "$last" ] || last=0
one=$((last + 1))
two=$((last + 2))
echo "{\"id\":\"e-$one\",\"seq\":$one,\"ts\":\"2026-09-18T00:00:00Z\",\"type\":\"run.started\",\"actor\":\"$by\",\"payload\":{\"run\":\"fx-7\",\"hash\":\"h\",\"workflow\":\"$workflow\"}}" >> "$stream"
echo "fx-7 — h"
case $workflow in
  falls)
    echo "{\"id\":\"e-$two\",\"seq\":$two,\"ts\":\"2026-09-18T00:00:00Z\",\"type\":\"run.failed\",\"actor\":\"$by\",\"payload\":{\"run\":\"fx-7\",\"exit\":1}}" >> "$stream"
    echo "fx-7 — failed"
    exit 1
    ;;
  *)
    echo "{\"id\":\"e-$two\",\"seq\":$two,\"ts\":\"2026-09-18T00:00:00Z\",\"type\":\"run.closed\",\"actor\":\"$by\",\"payload\":{\"run\":\"fx-7\"}}" >> "$stream"
    echo "fx-7 — closed"
    exit 0
    ;;
esac
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir.display().to_string()
}

/// One firing of a run routine against the stub, answering the stream it wrote
/// and the outcome the action reported.
fn fire_run_routine(
    label: &str,
    workflow: &str,
) -> (
    fleet_controller::routines::Outcome,
    Vec<fleet_controller::events::Record>,
) {
    use fleet_controller::routines::{self, action::Machine, state::State, Pass};
    use fleet_controller::{events, policy};

    let root = scratch(label);
    let machine_dir = root.join("machine");
    let project = root.join("project");
    std::fs::create_dir_all(&machine_dir).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let stubs = root.join("stubs");
    std::fs::create_dir_all(&stubs).unwrap();
    // The stub's own tools sit behind it on the child path; the stub is first.
    let child_path = format!("{}:/usr/bin:/bin", stub_fleet(&stubs));

    let body = format!(
        "[order]\ndescription = \"run one\"\ntrigger = \"cooldown\"\ninterval = \"1h\"\n\
         [action.run]\nworkflow = \"{workflow}\"\n[action.run.inputs]\nwho = \"the-arm\"\n"
    );
    let routine = match file::parse(
        "nightly",
        &project.join("orders/nightly.toml"),
        &Source::Fleet,
        &project,
        &body,
        &seats(),
    ) {
        Loaded::Routine(routine) => *routine,
        Loaded::Defective(defect) => panic!("{}", defect.reason()),
    };
    let policy = policy::parse("").expect("the empty policy is the defaults");
    let mut log = events::EventLog::open(&machine_dir.join("events.jsonl"));
    let mut state = State::default();
    let outcome = {
        let mut pass = Pass {
            machine: Machine {
                machine_dir: &machine_dir,
                child_path: &child_path,
                policy: &policy,
                seats: &[],
                agent: None,
                effects_off: None,
            },
            events: &mut log,
            state: &mut state,
        };
        routines::fire(&routine, &mut pass, 1_788_600_000, "the arm", Some("run"))
    };
    let argv = std::fs::read_to_string(machine_dir.join("fleet-argv.txt")).unwrap();
    assert_eq!(
        argv,
        format!("run\n{workflow}\n--input\nwho=the-arm\n--by\nnightly\n"),
        "the verb, the workflow, each input as a pair and the routine as the runner"
    );
    let cwd = std::fs::read_to_string(machine_dir.join("fleet-cwd.txt")).unwrap();
    assert_eq!(
        Path::new(cwd.trim()).canonicalize().unwrap(),
        project.canonicalize().unwrap(),
        "the verb runs in the routine's project root"
    );
    let stream = events::read_after(&machine_dir.join("events.jsonl"), 0);
    let _ = std::fs::remove_dir_all(&root);
    (outcome, stream)
}

fn seq_of(stream: &[fleet_controller::events::Record], kind: &str) -> u64 {
    let found: Vec<&fleet_controller::events::Record> =
        stream.iter().filter(|r| r.kind == kind).collect();
    assert_eq!(found.len(), 1, "exactly one {kind}: {:?}", kinds(stream));
    found[0].seq
}

fn kinds(stream: &[fleet_controller::events::Record]) -> Vec<String> {
    stream.iter().map(|r| r.kind.clone()).collect()
}

/// AC2. The routine's events wrap the run's: `routine.fired` opens before the
/// verb is called, so it cannot carry an id the store has not named yet;
/// `run.started` carries the routine as its actor; `routine.completed` comes
/// after `run.closed` and carries the run id.
#[test]
fn a_run_action_calls_the_run_verb_and_the_run_id_rides_the_terminal_event() {
    use fleet_controller::events;
    use fleet_controller::routines::Outcome;

    let (outcome, stream) = fire_run_routine("run-closed", "takeoff");
    assert_eq!(outcome, Outcome::Ran);
    assert_eq!(
        kinds(&stream),
        vec![
            events::ROUTINE_FIRED,
            "run.started",
            "run.closed",
            events::ROUTINE_COMPLETED
        ]
    );
    let fired = seq_of(&stream, events::ROUTINE_FIRED);
    let started = seq_of(&stream, "run.started");
    let closed = seq_of(&stream, "run.closed");
    let completed = seq_of(&stream, events::ROUTINE_COMPLETED);
    assert!(fired < started && started < closed && closed < completed);

    let opening = &stream[0];
    assert_eq!(opening.actor, "nightly");
    assert_eq!(opening.payload["order"], "nightly");
    assert!(
        opening.payload.get("run").is_none(),
        "the opening event carries no run id, because none exists yet: {}",
        opening.payload
    );
    assert_eq!(
        stream[1].actor, "nightly",
        "the run names the routine as its runner"
    );
    let run_id = stream[1].payload["run"].as_str().unwrap().to_string();

    let terminal = &stream[3];
    assert_eq!(terminal.actor, "nightly");
    assert_eq!(terminal.payload["order"], "nightly");
    assert_eq!(terminal.payload["outcome"], "ran");
    assert_eq!(terminal.payload["run"], run_id.as_str());
    assert_eq!(terminal.payload["by"], "run");
    let detail = terminal.payload["detail"].as_str().unwrap();
    assert!(detail.starts_with("fx-7 closed; "), "{detail}");
    assert!(
        terminal.payload["log"]
            .as_str()
            .is_some_and(|log| log.ends_with("nightly-run.log")),
        "{}",
        terminal.payload
    );
}

/// The verb's exit 1 is `routine.failed`, and the run id rides that line too:
/// a failed run is the one a reader most needs to find.
#[test]
fn a_run_the_verb_reports_failed_is_routine_failed_with_the_run_id() {
    use fleet_controller::events;
    use fleet_controller::routines::Outcome;

    let (outcome, stream) = fire_run_routine("run-failed", "falls");
    assert_eq!(outcome, Outcome::Failed);
    assert_eq!(
        kinds(&stream),
        vec![
            events::ROUTINE_FIRED,
            "run.started",
            "run.failed",
            events::ROUTINE_FAILED
        ]
    );
    let terminal = &stream[3];
    assert_eq!(terminal.payload["outcome"], "failed");
    assert_eq!(terminal.payload["run"], "fx-7");
    let detail = terminal.payload["detail"].as_str().unwrap();
    assert!(detail.starts_with("fx-7 exited 1: failed; "), "{detail}");
}

/// A machine whose child path carries no `fleet` cannot open a run, and says
/// so as could-not-tell rather than as a run that failed.
#[test]
fn a_run_action_with_no_fleet_on_the_child_path_is_could_not_tell() {
    use fleet_controller::policy;
    use fleet_controller::routines::{action, action::Machine, Outcome};

    let root = scratch("run-no-fleet");
    let routine = routine_of(
        "[order]\ndescription = \"d\"\ntrigger = \"cron\"\nschedule = \"* * * * *\"\n\
         [action.run]\nworkflow = \"takeoff\"\n",
    );
    let policy = policy::parse("").expect("the empty policy is the defaults");
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let child_path = empty.display().to_string();
    let machine = Machine {
        machine_dir: &root,
        child_path: &child_path,
        policy: &policy,
        seats: &[],
        agent: None,
        effects_off: None,
    };
    let done = action::run(&routine, &machine, "2026-09-18T00:00:00Z");
    assert_eq!(done.outcome, Outcome::CouldNotTell);
    assert!(
        done.detail
            .contains("no `fleet` on the constructed child PATH"),
        "{}",
        done.detail
    );

    // The dry run's argv names the verb by its bare name where nothing resolves.
    let argv = action::argv_of(&routine, &machine);
    assert_eq!(argv, vec!["fleet", "run", "takeoff", "--by", "a-routine"]);
    let _ = std::fs::remove_dir_all(&root);
}
