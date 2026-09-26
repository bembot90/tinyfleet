//! `fleet seat attach` through the shipped binary, with tmux answered by
//! `fleet-tmux-stub` (reviewer call 2026-09-25, E7).
//!
//! The stub keeps a fake server in a file beside the link `FLEET_TMUX_BIN`
//! names, answers the verb's listing out of it, and records every argument
//! list it is run with — the attach's included, and the attach's whole
//! environment beside it. The verb EXECS the attach in its own place, so the
//! run that answers the rig is the stub's: its exit is the process's exit, and
//! what the rig reads afterwards is what the stub recorded.
//!
//! No work graph and no controller anywhere: the verb reads the seat list and
//! the host and nothing else, so the project is a directory with a policy file
//! in it and the machine directory holds only the seat list.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::test_support::FakeServer;

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The seat's id, its own name and its machine name. Its session on the host is
/// named by the id; a person names it by either of the other two.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const NAME: &str = "Orla";
const MACHINE_NAME: &str = "orla-93b9739a";

/// One arm's project, machine directory, seat worktree and tmux stub.
struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    worktree: PathBuf,
    /// The link `FLEET_TMUX_BIN` names, and the state file beside it.
    tmux: PathBuf,
    state: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-attach-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("a-project");
        let machine = root.join("machine");
        let worktree = root.join("worktree");
        for dir in [&project, &machine, &worktree] {
            std::fs::create_dir_all(dir).expect("the directory is created");
        }
        std::fs::write(project.join("fleet.toml"), "[controller]\n")
            .expect("the policy file is written");
        std::fs::write(
            machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"id": "{SEAT_ID}", "name": "{NAME}",
                      "worktrees": {{"a-project": {worktree}}}}}
                   ]}}"#,
                fleet_toml = json_string(&project.join("fleet.toml").display().to_string()),
                worktree = json_string(&worktree.display().to_string()),
            ),
        )
        .expect("the machine config is written");
        let tmux = common::stub_tmux(&root.join("tmux"));
        let state = root.join("tmux").join("tmux-stub.json");
        Rig {
            root,
            project,
            machine,
            worktree,
            tmux,
            state,
        }
    }

    /// The seat's session on the fake server, its pane alive — as the
    /// controller's start leaves it.
    fn session(&self) -> &Rig {
        let mut server = FakeServer::load(&self.state).expect("the stub's state reads");
        server
            .start(
                SEAT_ID,
                &self.worktree.display().to_string(),
                &["/bin/agent".to_string()],
                &[("HOME".to_string(), "/h".to_string())],
            )
            .expect("the session starts on the fake server");
        server
            .save(&self.state)
            .expect("the stub's state is written");
        self
    }

    /// The seat's pane ended with `status` and kept, as remain-on-exit keeps
    /// one: through the stub's own verb, as a rig ends a real one.
    fn ended(&self, status: i32) -> &Rig {
        let out = Command::new(&self.tmux)
            .args(["end", SEAT_ID, &status.to_string()])
            .output()
            .expect("the stub runs");
        assert!(out.status.success(), "{}", stderr(&out));
        self
    }

    /// The shipped binary, run inside the project, its tmux the stub.
    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the built binary runs")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fleet"));
        cmd.args(args)
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env(common::hermetic::TMUX_BIN, &self.tmux);
        cmd
    }

    fn attach(&self, extra: &[&str]) -> Output {
        self.run(&[&["seat", "attach", MACHINE_NAME][..], extra].concat())
    }

    /// The fake server as the stub last wrote it.
    fn server(&self) -> FakeServer {
        FakeServer::load(&self.state).expect("the stub's state reads")
    }

    /// Every attach the stub was run for, as its argument list.
    fn attaches(&self) -> Vec<Vec<String>> {
        self.server()
            .invocations
            .into_iter()
            .filter(|args| args.iter().any(|a| a == "attach-session"))
            .collect()
    }

    /// Every line on the machine's stream, parsed.
    fn events(&self) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(self.machine.join("events.jsonl")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The argument list a read-only attach of the seat's session runs with: fleet's
/// socket, no person's config, and the session named exactly — `=` refuses a
/// prefix match, so no other session answers for this one.
fn read_only_attach() -> Vec<String> {
    [
        "-L",
        "fleet",
        "-f",
        "/dev/null",
        "attach-session",
        "-r",
        "-t",
        &format!("={SEAT_ID}"),
    ]
    .iter()
    .map(|a| a.to_string())
    .collect()
}

#[test]
fn a_machine_name_execs_a_read_only_attach_of_the_seats_session() {
    let rig = Rig::new("read-only");
    rig.session();

    let out = rig.attach(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(rig.attaches(), vec![read_only_attach()]);
    assert!(
        rig.events().is_empty(),
        "a read-only attach writes nothing to the stream: {:?}",
        rig.events()
    );
    assert!(
        !stderr(&out).contains("acts as that seat"),
        "the keyboard warning is --write's alone: {}",
        stderr(&out)
    );
}

#[test]
fn write_drops_read_only_warns_and_puts_the_keyboard_on_the_record() {
    let rig = Rig::new("write");
    rig.session();

    let out = rig.attach(&["--write"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let expected: Vec<String> = read_only_attach()
        .into_iter()
        .filter(|a| a != "-r")
        .collect();
    assert_eq!(rig.attaches(), vec![expected], "--write drops -r");
    assert!(
        stderr(&out).contains(&format!(
            "typing into {MACHINE_NAME}'s session acts as that seat"
        )),
        "{}",
        stderr(&out)
    );

    // Reviewer call 2026-09-25 (1): a keyboard taken over a seat is on the
    // record, as one line about the seat, written before the exec.
    let events = rig.events();
    assert_eq!(events.len(), 1, "one line: {events:?}");
    let event = &events[0];
    assert_eq!(event["type"], "seat.attached");
    assert_eq!(
        event["actor"],
        serde_json::json!({ "kind": "seat", "id": SEAT_ID }),
        "the line is about the seat, by its id"
    );
    assert_eq!(
        event["payload"]["seat"],
        serde_json::json!({ "id": SEAT_ID, "name": NAME, "kind": "agent" })
    );
    assert_eq!(event["payload"]["mode"], "write");
    let at = event["payload"]["at"].as_str().unwrap_or_default();
    assert!(
        fleet_controller::clock::secs_of_stamp(at).is_some(),
        "`at` is a stamp: {event}"
    );
}

#[test]
fn the_full_id_and_the_bare_name_attach_the_same_session() {
    let rig = Rig::new("spellings");
    rig.session();

    for seat in [MACHINE_NAME, SEAT_ID, NAME] {
        let out = rig.run(&["seat", "attach", seat]);
        assert_eq!(out.status.code(), Some(0), "{seat}: {}", stderr(&out));
    }
    assert_eq!(rig.attaches(), vec![read_only_attach(); 3]);
}

#[test]
fn a_seat_with_no_session_is_four_and_nothing_is_execd() {
    let rig = Rig::new("no-session");

    for extra in [&[][..], &["--write"][..]] {
        let out = rig.attach(extra);
        assert_eq!(out.status.code(), Some(4), "{extra:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(&format!("{MACHINE_NAME} has no session on fleet")),
            "{}",
            stderr(&out)
        );
    }
    let invoked = rig.server().invocations;
    assert!(
        invoked
            .iter()
            .all(|args| args.iter().any(|a| a == "list-panes")),
        "the host was listed and never attached: {invoked:?}"
    );
    assert_eq!(invoked.len(), 2, "one listing per run: {invoked:?}");
    assert!(
        rig.events().is_empty(),
        "a refused --write took no keyboard and writes no line"
    );

    // The control: another seat's session on the same server does not answer
    // for this one.
    let mut server = rig.server();
    server
        .start(
            "01a0d1f1-0aec-765f-9abe-000000000000",
            "/w",
            &["/bin/a".into()],
            &[],
        )
        .expect("another session starts");
    server.save(&rig.state).expect("the state is written");
    let out = rig.attach(&[]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(rig.attaches().is_empty());
}

#[test]
fn a_dead_pane_prints_its_status_and_is_still_attached() {
    let rig = Rig::new("dead");
    rig.session().ended(3);

    let out = rig.attach(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("the session ended with status 3"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        rig.attaches(),
        vec![read_only_attach()],
        "remain-on-exit kept the last screen, so the attach still runs"
    );

    // The control: a live pane prints no end.
    let live = Rig::new("dead-control");
    live.session();
    let out = live.attach(&[]);
    assert!(
        !stderr(&out).contains("the session ended"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_unknown_seat_exits_with_the_resolvers_code_before_the_host_is_asked() {
    let rig = Rig::new("unknown");
    rig.session();

    let out = rig.run(&["seat", "attach", "nobody"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("names no seat"), "{}", stderr(&out));
    assert!(
        rig.server().invocations.is_empty(),
        "the seat is resolved before the host is asked anything"
    );
}

#[test]
fn a_project_the_directory_does_not_resolve_to_is_two() {
    let rig = Rig::new("project");
    rig.session();

    let out = rig.attach(&["--project", "other"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--project"), "{}", stderr(&out));
    assert!(rig.server().invocations.is_empty());

    // The control: the name this directory resolves to gets past the gate.
    let out = rig.attach(&["--project", "a-project"]);
    assert_eq!(out.status.code(), Some(0), "the control: {}", stderr(&out));
    assert_eq!(rig.attaches(), vec![read_only_attach()]);
}

/// A person already inside their own tmux gets a nested client rather than
/// tmux's refusal, and the client finds fleet's socket where every other call
/// of the host found it — so neither of the two variables that would point it
/// elsewhere reaches it. The rest of the person's environment does: the
/// terminal is theirs.
#[test]
fn tmux_and_its_socket_directory_are_absent_from_the_attachs_environment() {
    let rig = Rig::new("nested");
    rig.session();

    let out = rig
        .command(&["seat", "attach", MACHINE_NAME])
        .env("TMUX", "/private/tmp/tmux-501/default,4242,0")
        .env("TMUX_TMPDIR", "/somewhere/else")
        .env("FLEET_ATTACH_PROBE", "carried")
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let envs = rig.server().attach_envs;
    assert_eq!(envs.len(), 1, "one attach: {envs:?}");
    let names: Vec<&str> = envs[0].iter().map(|(k, _)| k.as_str()).collect();
    assert!(!names.contains(&"TMUX"), "{names:?}");
    assert!(!names.contains(&"TMUX_TMPDIR"), "{names:?}");
    assert!(
        envs[0]
            .iter()
            .any(|(k, v)| k == "FLEET_ATTACH_PROBE" && v == "carried"),
        "the control: the person's own environment is carried: {names:?}"
    );
}

/// A host that will not answer is could-not-tell, with its own cause: here the
/// hermetic block's refusing tmux, which is what a rig naming no stub runs.
#[test]
fn an_unreadable_host_is_three_with_its_cause() {
    let rig = Rig::new("unreadable");
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["seat", "attach", MACHINE_NAME])
        .current_dir(&rig.project)
        .hermetic(&rig.root.join("home"), &rig.machine, None)
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("no tmux binary: this rig named no FLEET_TMUX_BIN"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn the_verbs_help_says_how_to_detach_and_what_write_means() {
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["seat", "attach", "--help"])
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let help = String::from_utf8_lossy(&out.stdout);
    for said in ["read-only unless", "--write", "prefix and d", "--project"] {
        assert!(help.contains(said), "{said}: {help}");
    }
}
