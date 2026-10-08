//! `fleet create`, `fleet start` and `fleet stop` through the shipped binary.
//!
//! One rig per arm: its own scratch project, its own HOME, its own machine
//! directory, and its own stub for the platform's service manager. NOTHING HERE
//! TOUCHES THE REAL MACHINE — the service file is written under the scratch
//! HOME, no label is loaded on this box, and the fleet directory is the rig's.
//!
//! The manager stub records every call it is given, CLASSIFIED — `query`, `load`
//! or `unload` — because the two platforms spell those three differently and an
//! arm that counted a platform's own word would be an arm for one of them. The
//! argv is recorded beside the class, so what was run is still readable.

use fleet_controller::events::EventLog;
use fleet_controller::runs;
use fleet_core::seat::identity::{self, MachineIdentity, SeatId};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The agent `fleet create` installs a pack for by default, and the name the
/// file it writes gives `[agent] adapter`: fleet-packs'
/// `adapters/agent/<this>`, carried here by the rig's own checkout.
const AGENT: &str = fleet_controller::adapter::DEFAULT_AGENT_ADAPTER;

/// The store pack `fleet create` installs by default, and the name the file
/// it writes gives `[store] adapter`: fleet-packs' `adapters/store/<this>`.
const STORE: &str = fleet_core::store::DEFAULT_ADAPTER;

/// Seat ids written out by hand, so a row's `<slug>-<short>` name is read
/// against a spelling this suite fixed and not one it computed. They share
/// their first eight characters the way ids minted in one minute do, and each
/// ends in its own eight — the short id.
const SEAT_A: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const SEAT_B: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
const SEAT_C: &str = "01a0d1f1-0aec-765f-9abe-0f3b6d2c8e44";
const SEAT_H: &str = "01a0d1f1-0aec-765f-9abe-7a9e1c4f05d2";
/// A transient seat's row, and a row whose seat has left the table.
const SPAWNED: &str = "01a0d1f1-0aec-765f-9abe-00002f6d1a93";
const GONE: &str = "01a0d1f1-0aec-765f-9abe-0000000a90e5";

/// The model the rig's agent pack names as its default: the agent stub's own
/// answer, read after the install and carried by the example seat row.
fn model() -> String {
    fleet_controller::test_support::Answers::default()
        .capabilities
        .default_model
}

/// What `fleet create --embedded` writes, byte for byte, with the model the
/// installed adapter names in the example row, or no model line at all.
fn embedded_with(model: Option<&str>) -> String {
    let model = model
        .map(|model| format!("#   model = \"{model}\"\n"))
        .unwrap_or_default();
    format!(
        "\
# This fleet is EMBEDDED: this file is its policy and it sits at the
# project's root, so the fleet and the project are one directory.
# Written by `fleet create --embedded --agent {AGENT} --store none`.
#
# Every key the controller defaults is left out on purpose. Add one here
# to override it for this fleet.

# Opt-out, never opt-in: a guard is on unless this file turns it off.
[guards]
shell-trap.enabled = true
record.enabled = true

# Off, and said. No metric leaves this machine.
[telemetry]
enabled = false

# The agent every seat of this fleet runs on: the agent adapter an
# installed pack carries under this name.
[agent]
adapter = \"{AGENT}\"

# One table per seat, keyed by the seat's id. fleet seat add writes them
# and fleet start renders the agent seats into the machine's seat list.
# A row looks like this:
#
#   [seats.01a0d1f1-0aec-765f-9abe-d4f993b9739a]
#   kind = \"agent\"
#   name = \"what a person calls it\"
{model}#   status = \"active\"
[seats]
"
    )
}

/// What `fleet create --embedded` writes, byte for byte.
fn embedded() -> String {
    embedded_with(Some(&model()))
}

/// The embedded file once `create` has listed its creator: [`embedded`] and
/// one human table after it, keyed by the machine's identity.
fn embedded_listing(id: &SeatId) -> String {
    format!("{}\n[seats.{id}]\nkind = \"human\"\n", embedded())
}

/// What `fleet create --standalone` writes, byte for byte, for one project.
/// The header names no `--agent` though the call carried one: the agent is
/// the fleet's, and a project declared to it records none.
fn standalone_text(name: &str, item_prefix: Option<&str>, root: &Path, worktrees: &Path) -> String {
    let prefix = match item_prefix {
        Some(prefix) => format!("item_prefix = \"{prefix}\"\n"),
        None => "# item_prefix = \"the prefix this project's items carry\"\n".to_string(),
    };
    format!(
        "# This project is declared to the STANDALONE fleet this machine runs.\n\
         # Written by `fleet create --standalone --store none`.\n\
         \n\
         [project]\n\
         name = \"{name}\"\n\
         {prefix}\
         primary = \"{root}\"\n\
         worktrees = \"{worktrees}\"\n\
         \n\
         # Owed, and left for the person: a marker this file guessed would\n\
         # skip a pipeline nobody chose to skip. The test commands are not set\n\
         # here — a workflow hands them to the landing; for takeoff they are\n\
         # takeoff.test and takeoff.touched under [packs.tiny] in fleet.toml.\n\
         [landing]\n\
         # ci_marker = \"the marker a landing's commit carries\"\n",
        root = root.display(),
        worktrees = worktrees.display(),
    )
}

/// `script` with the child's three streams on a pseudo-terminal. The two
/// spellings are not interchangeable: BSD takes the command as its own
/// arguments, util-linux takes one `-c` string and names the typescript last.
#[cfg(target_os = "macos")]
fn script_command(line: &[String]) -> Command {
    let mut command = Command::new("script");
    command.arg("-q").arg("/dev/null").args(line);
    command
}

#[cfg(not(target_os = "macos"))]
fn script_command(line: &[String]) -> Command {
    let joined = line
        .iter()
        .map(|word| format!("'{}'", word.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = Command::new("script");
    command
        .arg("-q")
        .arg("-e")
        .arg("-c")
        .arg(joined)
        .arg("/dev/null");
    command
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("the binary exited rather than died")
}

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

/// The two paths `kill` lives at on the two targets.
fn kill_bin() -> PathBuf {
    ["/bin/kill", "/usr/bin/kill"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .expect("a kill binary is on this box")
}

struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    home: PathBuf,
    manager: PathBuf,
    argv: PathBuf,
    pid: PathBuf,
    /// The rig's own fleet-packs checkout, which every embedded `create` here
    /// installs the agent's pack from.
    packs: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-lifecycle-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the fixture root is made");
        // CANONICAL, because the binary resolves the directory it was run in and
        // this platform's temp directory is reached through a symlink: an arm
        // comparing a path it composed against one the binary printed would be
        // comparing two spellings of the same directory.
        let root = root.canonicalize().expect("the fixture root resolves");
        let rig = Rig {
            project: root.join("a-project"),
            machine: root.join("machine"),
            home: root.join("home"),
            manager: root.join("manager.sh"),
            argv: root.join("manager-argv"),
            pid: root.join("manager-pid"),
            packs: common::packs_checkout(&root),
            root,
        };
        for dir in [&rig.project, &rig.home] {
            std::fs::create_dir_all(dir).expect("the fixture directory is made");
        }
        rig.a_repository();
        // A store that is already there, so the arm that says this verb never
        // touches one has a subject to read.
        write(
            &rig.project.join(".tracker/config.yaml"),
            "# the project's own store config\nissue-prefix: ap\n",
        );
        rig.a_manager();
        rig
    }

    /// The scratch project: a repository with a bare origin, which is the shape
    /// a fleet is created inside.
    fn a_repository(&self) {
        git(
            &self.root,
            &["init", "--bare", "--quiet", "-b", "main", "bare"],
        );
        git(&self.project, &["init", "--quiet", "-b", "main"]);
        git(
            &self.project,
            &[
                "remote",
                "add",
                "origin",
                &self.root.join("bare").display().to_string(),
            ],
        );
        write(&self.project.join("the-work.txt"), "the work\n");
        git(&self.project, &["add", "--", "the-work.txt"]);
        git(
            &self.project,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "the work"],
        );
    }

    /// The platform's service manager, stubbed.
    fn a_manager(&self) {
        let script = format!(
            "#!/bin/sh\n\
             stream={machine}/events.jsonl\n\
             say() {{\n\
             \x20 if [ -f \"$stream\" ]; then n=$(awk 'END{{print NR+1}}' \"$stream\"); else n=1; fi\n\
             \x20 mkdir -p {machine}\n\
             \x20 printf '{{\"id\":\"stub-%s\",\"seq\":%s,\"ts\":\"2026-09-12T00:00:00Z\",\
             \"type\":\"%s\",\"actor\":{{\"kind\":\"controller\",\"id\":\"a-machine\"}},\"payload\":{{}}}}\\n' \"$n\" \"$n\" \"$1\" \
             >> \"$stream\"\n\
             }}\n\
             verb=$1\n\
             if [ \"$verb\" = \"--user\" ]; then verb=$2; fi\n\
             case \"$verb\" in\n\
             \x20 print|show)\n\
             \x20   printf 'query|%s\\n' \"$*\" >> {argv}\n\
             \x20   if [ -s {pid} ]; then\n\
             \x20     printf '\\tstate = running\\n\\tpid = %s\\n' \"$(cat {pid})\"\n\
             \x20     printf 'MainPID=%s\\n' \"$(cat {pid})\"\n\
             \x20     exit 0\n\
             \x20   fi\n\
             \x20   exit 1 ;;\n\
             \x20 bootout|stop)\n\
             \x20   printf 'unload|%s\\n' \"$*\" >> {argv}\n\
             \x20   : > {pid}\n\
             \x20   say controller.stopped\n\
             \x20   exit 0 ;;\n\
             \x20 *)\n\
             \x20   printf 'load|%s\\n' \"$*\" >> {argv}\n\
             \x20   echo 4242 > {pid}\n\
             \x20   say controller.started\n\
             \x20   exit 0 ;;\n\
             esac\n",
            machine = self.machine.display(),
            argv = self.argv.display(),
            pid = self.pid.display(),
        );
        write(&self.manager, &script);
        executable(&self.manager);
        write(&self.pid, "");
    }

    /// The rig's checkout laid out as fleet-packs is, for `--packs-from`:
    /// [`common::packs_checkout`]'s, carrying the default store pack, the
    /// runtime pack it imports and the default agent pack.
    fn a_packs_checkout(&self) -> String {
        self.packs.clone()
    }

    /// The call every arm not about the questions creates its embedded fleet
    /// with: every answer scripted, no store pack, and the agent's pack from
    /// the rig's own checkout, so no call reaches the published source.
    fn embedded(&self) -> [&str; 8] {
        [
            "create",
            "--embedded",
            "--agent",
            AGENT,
            "--store",
            "none",
            "--packs-from",
            &self.packs,
        ]
    }

    /// The agent's pack `create` installed, taken back out: the fleet's file
    /// still names it, and no pack carries the name.
    fn without_the_agent_pack(&self) -> &Rig {
        std::fs::remove_dir_all(self.machine.join("packs").join(AGENT))
            .expect("the agent's pack was installed by create");
        self
    }

    /// The fleet's file with `[agent] adapter` naming `value` in place of the
    /// name `create` wrote, or with no `[agent]` table at all for `None`.
    fn agent_is(&self, value: Option<&str>) -> &Rig {
        let path = self.project.join("fleet.toml");
        let body = std::fs::read_to_string(&path).expect("the policy is there");
        let written = format!("[agent]\nadapter = \"{AGENT}\"\n");
        assert!(body.contains(&written), "create wrote the key: {body}");
        let now = match value {
            Some(value) => format!("[agent]\nadapter = {value:?}\n"),
            None => String::new(),
        };
        write(&path, &body.replace(&written, &now));
        self
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fleet"));
        command.args(args);
        self.envs(&mut command);
        command
    }

    fn envs(&self, command: &mut Command) {
        command
            .current_dir(&self.project)
            .hermetic(&self.home, &self.machine)
            .env("FLEET_SERVICE_BIN", &self.manager)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            // NO NETWORK: `create` fetches the agent's pack and a store's
            // with git, and an arm that forgot `--packs-from` would reach the
            // pinned source over https. Local paths are git's `file`
            // transport, and nothing else is let through.
            .env("GIT_ALLOW_PROTOCOL", "file");
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the built binary runs")
    }

    /// The verb on a pseudo-terminal, with `keys` typed at it.
    ///
    /// The terminal is a real one, allocated by `script`, because the reading
    /// being taken IS whether the prompt accepts the keystrokes a person makes
    ///. `script` copies its own stdin into the terminal it
    /// allocated, so the keys reach dialoguer through a tty and not through a
    /// pipe the verb refuses before it asks anything.
    ///
    /// DEADLINED: the failure this arm is about is a prompt that accepts
    /// nothing, and a child still waiting once its keys have run out would hang
    /// the suite rather than fail it. A `None` status is that timeout.
    fn on_a_pty(&self, args: &[&str], keys: &str) -> (Option<i32>, String) {
        let mut line = vec![env!("CARGO_BIN_EXE_fleet").to_string()];
        line.extend(args.iter().map(|arg| arg.to_string()));
        let mut command = script_command(&line);
        self.envs(&mut command);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("`script` allocates the terminal");
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(keys.as_bytes())
            .expect("the keys are typed");

        let end = Instant::now() + Duration::from_secs(30);
        loop {
            match child.try_wait().expect("the child is waitable") {
                Some(_) => break,
                None if Instant::now() >= end => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (None, String::new());
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let out = child.wait_with_output().expect("the child is reaped");
        // The two streams are one on a terminal, which is the point: what a
        // person sees is this.
        (out.status.code(), stdout(&out))
    }

    /// The classified calls the manager stub was given, in order.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.argv)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn of_class(&self, class: &str) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|line| line.starts_with(&format!("{class}|")))
            .collect()
    }

    fn service_file(&self) -> PathBuf {
        let agents = self.home.join("Library/LaunchAgents");
        let units = self.home.join(".config/systemd/user");
        for dir in [agents, units] {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                if let Some(one) = entries.filter_map(Result::ok).next() {
                    return one.path();
                }
            }
        }
        panic!("no service file was written under {}", self.home.display());
    }

    fn seat_list(&self) -> serde_json::Value {
        let body = std::fs::read_to_string(self.machine.join("config.json"))
            .expect("the seat list is there");
        serde_json::from_str(&body).expect("the seat list parses")
    }

    fn stream(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The fleet created, so an arm about `start` does not re-assert `create`.
    fn created(&self) -> &Rig {
        let out = self.run(&self.embedded());
        assert_eq!(code(&out), 0, "create: {}", stderr(&out));
        self
    }

    /// The identity `create` minted in this rig's machine directory.
    fn identity(&self) -> MachineIdentity {
        identity::read_identity(&self.machine)
            .expect("identity.toml reads")
            .expect("identity.toml is there")
    }

    /// The policy file with one more line in it.
    fn policy_says(&self, text: &str) -> &Rig {
        let path = self.project.join("fleet.toml");
        let mut body = std::fs::read_to_string(&path).expect("the policy is there");
        body.push_str(text);
        write(&path, &body);
        self
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("the stub is executable");
}

fn git(dir: &Path, args: &[&str]) {
    std::fs::create_dir_all(dir).expect("the directory is there");
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "fleet tests")
        .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
        .env("GIT_COMMITTER_NAME", "fleet tests")
        .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Wait for a condition, or say what was never true.
fn until(what: &str, deadline: Duration, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("{what} did not happen within {deadline:?}");
}

// ---- fleet create -----------------------------------------------------------

/// Every refusal `create` owes, each before anything is written.
#[test]
fn create_refuses_a_fleet_that_is_already_here_and_a_dot_fleet_that_is_not_a_project() {
    let rig = Rig::new("refusals");
    write(&rig.project.join("fleet.toml"), "[controller]\n");
    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&rig.project.join("fleet.toml").display().to_string()),
        "the path is named: {}",
        stderr(&out)
    );
    std::fs::remove_file(rig.project.join("fleet.toml")).unwrap();

    // A `.fleet/` with no declaration in it is somebody else's directory.
    std::fs::create_dir_all(rig.project.join(".fleet/elsewhere")).unwrap();
    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&rig.project.join(".fleet").display().to_string()),
        "{}",
        stderr(&out)
    );
    assert!(
        !rig.project.join("fleet.toml").exists(),
        "nothing was written"
    );

    // The control: with that directory carrying a declaration, the same call is
    // not refused for this reason — a declared project is one `--standalone`
    // registers rather than collides with, and `--embedded` still writes.
    std::fs::remove_dir_all(rig.project.join(".fleet")).unwrap();
    write(
        &rig.project.join(".fleet/project.toml"),
        "[project]\nname = \"a-project\"\n",
    );
    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

/// A name `create` installs no agent pack for is a usage refusal naming the
/// ones it does, before any question and before anything is written — and
/// [`AGENT`] with an underscore for its dash is such a name: there is one
/// spelling (ruling 17).
#[test]
fn create_refuses_an_agent_no_adapter_answers_to() {
    let rig = Rig::new("agent");
    let underscored = AGENT.replace('-', "_");
    for named in ["not-an-agent", underscored.as_str()] {
        let out = rig.run(&[
            "create",
            "--embedded",
            "--agent",
            named,
            "--packs-from",
            &rig.packs,
        ]);
        assert_eq!(code(&out), 2, "{}", stderr(&out));
        assert!(
            stderr(&out).contains(&format!(
                "no agent pack answers to `{named}` — this fleet installs {AGENT}"
            )),
            "it names what is known: {}",
            stderr(&out)
        );
    }
    // Before any question: a pipe with no mode answered meets the agent's
    // refusal, not the mode's.
    let out = rig.run(&["create", "--agent", &underscored]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).contains(&underscored), "{}", stderr(&out));
    assert!(
        !rig.project.join("fleet.toml").exists(),
        "nothing was written"
    );
    assert!(
        !rig.machine.join("packs.lock").exists(),
        "nothing was pinned"
    );
}

/// NEITHER of the two ways of naming a fleet, so the refusal names both: the
/// flag that answers it here, and the start that writes the seat list.
#[test]
fn create_refuses_standalone_with_no_fleet_on_the_machine_and_names_start() {
    let rig = Rig::new("standalone-alone");
    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--store",
        "none",
    ]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("fleet start"), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("--fleet"),
        "the flag that answers this refusal is named in it: {}",
        stderr(&out)
    );
    assert!(!rig.project.join(".fleet/project.toml").exists());
    assert!(!rig.machine.join("config.json").exists());
}

/// The flag given and the directory it names carrying no fleet — a refusal of
/// its own, so a mistyped path is not read as no flag at all.
#[test]
fn create_refuses_a_fleet_flag_whose_directory_holds_no_policy_file() {
    let rig = Rig::new("standalone-mistyped");
    let nowhere = rig.root.join("not-a-fleet");
    std::fs::create_dir_all(&nowhere).expect("the directory is made");
    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--store",
        "none",
        "--fleet",
        &nowhere.display().to_string(),
    ]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!("--fleet {}", nowhere.display())),
        "the refusal names the directory it was given: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("holds no fleet.toml"),
        "and what it looked for there: {}",
        stderr(&out)
    );
    assert!(!rig.project.join(".fleet/project.toml").exists());
    assert!(!rig.machine.join("config.json").exists());
}

/// REGISTRATION BEFORE THE FIRST START: three acts and not five. The embedded
/// fleet, the project declared to it by `--fleet` with no start in between, and
/// ONE start that renders the seat into the row.
///
/// The seat list is read at each act, because the whole change is WHEN that
/// file comes to exist: a run of this sequence before it said "1 row(s) already
/// rendered" on a second start, and the row only ever appeared on that second.
#[test]
fn create_standalone_names_the_fleet_and_one_start_renders_the_seat() {
    let rig = Rig::new("standalone-first");
    // The fleet's own directory BESIDE the project, never above it, so the row's
    // worktree can only have come from the registered project.
    let fleet = rig.root.join("the-fleet");
    std::fs::create_dir_all(&fleet).expect("the fleet's own directory is made");
    let out = rig
        .command(&rig.embedded())
        .current_dir(&fleet)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let policy = fleet.join("fleet.toml");
    let body = std::fs::read_to_string(&policy).expect("the policy is there");
    write(
        &policy,
        &format!("{body}\n[seats.{SEAT_A}]\nkind = \"agent\"\n"),
    );
    // ACT ONE LEAVES NO SEAT LIST. It is `start` that writes one, and the whole
    // point of this arm is that no start has run.
    assert!(
        !rig.machine.join("config.json").exists(),
        "`create --embedded` writes no seat list"
    );

    // ACT TWO: the project declared to that fleet, with the flag naming it.
    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--store",
        "none",
        "--fleet",
        &fleet.display().to_string(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let before = rig.seat_list();
    assert_eq!(
        before["fleet_toml"],
        serde_json::json!(policy.display().to_string()),
        "the seat list names the fleet the flag named: {before}"
    );
    assert_eq!(
        before["children"].as_array().map(Vec::len),
        Some(0),
        "and holds no row — rendering is the start's: {before}"
    );
    assert!(
        stderr(&out).contains(&format!(
            "registered on this machine — {}",
            policy.display()
        )),
        "the verb says it wrote the record: {}",
        stderr(&out)
    );
    // The project is in the register too, which is what the start renders from.
    let register =
        std::fs::read_to_string(rig.machine.join("projects.toml")).expect("the register is there");
    assert!(
        register.contains(&format!("root = \"{}\"", rig.project.display())),
        "{register}"
    );

    // ACT THREE: ONE start, and the seat is rendered against the project.
    let out = rig.run(&["start"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let after = rig.seat_list();
    let rows = after["children"].as_array().expect("children is an array");
    assert_eq!(rows.len(), 1, "one seat, one row: {after}");
    let map = rows[0]["worktrees"]
        .as_object()
        .expect("the row carries a worktrees map");
    assert_eq!(
        map.get("a-project").map(ToString::to_string),
        Some(format!(
            "\"{}\"",
            rig.root
                .join("a-project-worktrees/agent-93b9739a")
                .display()
        )),
        "the row is keyed on the registered project: {after}"
    );
    assert!(
        stderr(&out).contains("1 row(s) rendered"),
        "THIS start rendered it: {}",
        stderr(&out)
    );
    assert!(
        !stderr(&out).contains("already rendered"),
        "and did not find it already there: {}",
        stderr(&out)
    );
    // THE COUNT THAT SAYS THREE ACTS: one load, so no second start happened.
    assert_eq!(
        rig.of_class("load").len(),
        1,
        "one start, not two: {:?}",
        rig.calls()
    );

    // WHAT `fleet status` WOULD SAY IS NOT READABLE HERE: its one instrument is
    // the projection a running controller publishes, and this rig's manager is a
    // stub that starts none — so the row above is read from the seat list, which
    // is the file `start` actually renders into.
    let status = rig.run(&["status"]);
    assert_eq!(
        code(&status),
        5,
        "no projection, so no page: {}",
        stderr(&status)
    );
}

/// A question put to a pipe is a usage error naming the flag that answers it,
/// never a wait for an answer that cannot come.
#[test]
fn create_refuses_a_question_it_cannot_ask_and_names_the_flag() {
    let rig = Rig::new("piped");
    let out = rig.run(&["create"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).contains("--embedded"), "{}", stderr(&out));
    assert!(stderr(&out).contains("not a terminal"), "{}", stderr(&out));

    assert!(
        !rig.project.join("fleet.toml").exists(),
        "nothing was written"
    );
}

/// THE AGENT IS A QUESTION WITH A DEFAULT, as the store is: no terminal and no
/// `--agent` takes the first agent rather than being refused. Its pack is
/// installed from the source `--packs-from` names at the tag this binary
/// pins, pinned in the lock, and named in the file as `[agent] adapter`; the
/// header names no `--agent`, because the call carried none and nothing was
/// asked.
#[test]
fn create_with_no_terminal_and_no_agent_takes_the_default_and_installs_its_pack() {
    let rig = Rig::new("agent-default");
    let out = rig.run(&[
        "create",
        "--embedded",
        "--store",
        "none",
        "--packs-from",
        &rig.packs,
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");

    let pin = fleet_core::supported::PINNED_PACKS;
    let source = format!("{}//adapters/agent/{AGENT}", rig.packs);
    let lines = locked(&rig.machine);
    let (_, version, commit) = lines
        .iter()
        .find(|(held, _, _)| *held == source)
        .unwrap_or_else(|| panic!("the lock pins {source}: {lines:?}"))
        .clone();
    assert_eq!(version, pin, "pinned at the tag the binary pins");
    assert!(rig
        .machine
        .join("packs")
        .join(AGENT)
        .join(format!("adapters/agent/{AGENT}/adapter.toml"))
        .is_file());
    assert!(
        said.contains(&format!(
            "agent: {AGENT} {pin} at {commit}, installed and pinned — {}",
            rig.machine.join("packs").join(AGENT).display()
        )),
        "{said}"
    );

    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert!(
        written.contains(&format!("[agent]\nadapter = \"{AGENT}\"\n")),
        "{written}"
    );
    assert!(
        written.contains("# Written by `fleet create --embedded --store none`.\n"),
        "{written}"
    );
}

/// THE EXAMPLE ROW'S MODEL IS THE ADAPTER'S: read off the installed agent
/// adapter's capabilities after the install, never a constant of this binary.
/// The stub's state names another default model here, and that one is written.
///
/// RED-PROOF: with the row's model read from anything but the adapter, the
/// file carries the stub's usual default and not this one.
#[test]
fn create_writes_the_model_the_installed_adapter_names_as_its_default() {
    let rig = Rig::new("agent-model");
    fleet_controller::test_support::agent_stub::script(&rig.project, |answers| {
        answers.capabilities.default_model = "quill-large-2".to_string();
    });
    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert!(
        written.contains("\n#   model = \"quill-large-2\"\n"),
        "{written}"
    );
    assert_ne!(model(), "quill-large-2", "the arm reads a different model");
}

/// An agent pack already on the machine is left as it stands, and one whose
/// adapter does not answer leaves the example row's model OUT rather than
/// guessing one: the rest of the file is written as ever.
#[test]
fn create_leaves_the_example_model_out_where_the_adapter_does_not_answer() {
    let rig = Rig::new("agent-mute");
    let pack = rig.machine.join("packs").join(AGENT);
    let dir = pack.join(format!("adapters/agent/{AGENT}"));
    write(
        &pack.join("pack.toml"),
        &format!("[pack]\nname = \"{AGENT}\"\nversion = \"0.1.0\"\nschema = 3\n"),
    );
    write(
        &dir.join("adapter.toml"),
        &format!(
            "[adapter]\nname = \"{AGENT}\"\nkind = \"agent\"\nversion = \"0.1.0\"\n\
             entry = \"main\"\n"
        ),
    );
    write(&dir.join("main"), "#!/bin/sh\ncat > /dev/null\nexit 3\n");
    executable(&dir.join("main"));

    let out = rig.run(&rig.embedded());
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");
    assert!(
        said.contains(&format!(
            "agent: {AGENT}, already installed on this machine — left as it stands"
        )),
        "{said}"
    );
    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert_eq!(
        written,
        format!(
            "{}\n[seats.{}]\nkind = \"human\"\n",
            embedded_with(None),
            rig.identity().id
        )
    );
}

/// A fetch of the agent's pack that fails — a checkout that carries no tag the
/// binary pins — refuses with no fleet file written, naming git's reason and
/// the `fleet pack add` line that installs the pack.
#[test]
fn create_refuses_with_no_fleet_written_when_the_agent_pack_cannot_be_fetched() {
    let rig = Rig::new("agent-unfetched");
    let untagged = rig.root.join("untagged");
    write(&untagged.join("README"), "no packs here\n");
    git(&untagged, &["init", "--quiet", "-b", "main"]);
    git(&untagged, &["add", "--all"]);
    git(
        &untagged,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "nothing"],
    );
    let untagged = untagged.display().to_string();

    let out = rig.run(&[
        "create",
        "--embedded",
        "--agent",
        AGENT,
        "--store",
        "none",
        "--packs-from",
        &untagged,
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 1, "{said}");
    assert!(
        said.contains(
            "the agent's pack was not installed, so no fleet file was written: git checkout"
        ),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "`fleet pack add {untagged}//adapters/agent/{AGENT} --version {}` installs it",
            fleet_core::supported::PINNED_PACKS
        )),
        "{said}"
    );
    assert!(!rig.project.join("fleet.toml").exists(), "no fleet file");
    assert!(!rig.machine.join("packs").join(AGENT).exists(), "no pack");
}

/// The three questions answered by three Enters, on a real terminal.
///
/// Every row is the first row, which is what Enter alone takes: a prompt that
/// starts with nothing selected refuses Enter and redraws, and the arm's red is
/// the timeout below rather than a wrong answer. The store's first row is the
/// default store pack and the agent's the default agent's pack, both installed
/// from the rig's own fleet-packs checkout.
#[test]
fn create_takes_the_first_row_of_each_prompt_on_enter_alone() {
    let rig = Rig::new("pty-enter");
    let packs = rig.a_packs_checkout();
    let (status, seen) = rig.on_a_pty(&["create", "--packs-from", &packs], "\n\n\n");
    assert_eq!(
        status,
        Some(0),
        "three Enters answer the three questions; the terminal showed:\n{seen}"
    );
    assert!(
        seen.contains("embedded or standalone?"),
        "every question was asked: {seen}"
    );
    // The agent is asked AFTER the store, and its row is the pack's name.
    let store_at = seen.find("which store?").expect("the store was asked");
    let agent_at = seen.find("which agent?").expect("the agent was asked");
    assert!(store_at < agent_at, "the store first: {seen}");
    assert!(
        seen.contains(&format!("{AGENT} — installed from fleet-packs")),
        "{seen}"
    );

    // The first row of the first question is `embedded`, so the embedded file is
    // the one written — the answer is read out of what landed, not out of what
    // was drawn.
    let written = std::fs::read_to_string(rig.project.join("fleet.toml"))
        .expect("the first row is `embedded`, so the embedded file is the one written");
    assert!(written.contains("EMBEDDED"), "{written}");
    assert!(
        !rig.project.join(".fleet/project.toml").exists(),
        "the second row was not taken"
    );

    // The first row of the second question is the default store pack:
    // installed, and the file naming it.
    assert!(
        written.contains(&format!("[store]\nadapter = \"{STORE}\"\n")),
        "{written}"
    );
    assert!(
        rig.machine
            .join("packs")
            .join(STORE)
            .join("pack.toml")
            .is_file(),
        "the store's pack is installed"
    );

    // The first row of the third is the default agent: its pack installed,
    // and the file naming it.
    assert!(
        written.contains(&format!("[agent]\nadapter = \"{AGENT}\"\n")),
        "{written}"
    );
    assert!(
        rig.machine
            .join("packs")
            .join(AGENT)
            .join("pack.toml")
            .is_file(),
        "the agent's pack is installed"
    );
}

/// A file nobody passed a flag to does not claim one.
#[test]
fn create_writes_a_header_naming_only_the_flags_the_call_carried() {
    let rig = Rig::new("pty-header");
    let packs = rig.a_packs_checkout();
    let (status, seen) = rig.on_a_pty(&["create", "--packs-from", &packs], "\n\n\n");
    assert_eq!(status, Some(0), "{seen}");

    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert!(
        written.contains("# Written by `fleet create`, answered at its prompts."),
        "{written}"
    );
    for flag in [
        "--embedded",
        "--standalone",
        "--agent",
        "--store",
        "--packs-from",
    ] {
        assert!(
            !written.contains(flag),
            "no flag was passed, so none is named: {flag} in\n{written}"
        );
    }

    // The pair: a call that DID carry its flags names them, on the same
    // assertion the arm above makes about their absence.
    let flagged = Rig::new("pty-header-flagged");
    let out = flagged.run(&flagged.embedded());
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let written = std::fs::read_to_string(flagged.project.join("fleet.toml")).expect("it landed");
    assert!(
        written.contains(&format!(
            "# Written by `fleet create --embedded --agent {AGENT} --store none`."
        )),
        "{written}"
    );
}

// ---- fleet create: the store's pack (fleet-3krx.1) --------------------------

/// The lines the machine's lock holds, as `(source, version, commit)`.
fn locked(machine: &Path) -> Vec<(String, String, String)> {
    fleet_core::lock::read(&machine.join(fleet_core::lock::LOCK))
        .expect("the lock reads")
        .into_iter()
        .map(|line| (line.source, line.version, line.commit))
        .collect()
}

/// The `[store] adapter` the project at `root` names in its own file, read the
/// way the store is opened by.
fn store_named(root: &Path) -> Option<String> {
    let policy = fleet_core::store::project_policy(root).expect("the project's file parses");
    policy
        .get("store")
        .and_then(|store| store.get("adapter"))
        .and_then(|adapter| adapter.as_str())
        .map(str::to_string)
}

/// No terminal and no `--store`: the default is taken rather than refused, and
/// the default is the bd pack — fetched from the source `--packs-from` names at
/// the tag this binary pins, installed with the runtime pack it imports out of
/// the same checkout, both pinned in the lock at that tag and commit, and the
/// file naming the adapter. The header names no `--store`, because the call
/// carried none.
#[test]
fn create_with_no_terminal_installs_the_default_store_pack_and_pins_it() {
    let rig = Rig::new("store-default");
    let packs = rig.a_packs_checkout();
    let out = rig.run(&[
        "create",
        "--embedded",
        "--agent",
        AGENT,
        "--packs-from",
        &packs,
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");

    let pin = fleet_core::supported::PINNED_PACKS;
    let store = format!("{packs}//adapters/store/{STORE}");
    let ts = format!("{packs}//runtimes/ts");
    let lines = locked(&rig.machine);
    let line = |source: &str| {
        lines
            .iter()
            .find(|(held, _, _)| held == source)
            .unwrap_or_else(|| panic!("the lock pins {source}: {lines:?}"))
            .clone()
    };
    let (_, version, commit) = line(&store);
    assert_eq!(version, pin, "pinned at the tag the binary pins");
    assert_eq!(commit.len(), 40, "and the commit it resolved to: {commit}");
    assert_eq!(line(&ts).1, pin, "the import at the same tag");
    assert_eq!(line(&ts).2, commit, "out of the same checkout");
    let installed = rig.machine.join("packs").join(STORE);
    assert!(installed.join("pack.toml").is_file());
    assert!(installed
        .join(format!("adapters/store/{STORE}/main.sh"))
        .is_file());
    assert!(rig.machine.join("packs/ts/pack.toml").is_file());
    assert!(
        rig.machine.join("defaults").is_dir(),
        "the defaults beneath it, as ever"
    );

    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert_eq!(
        store_named(&rig.project).as_deref(),
        Some(STORE),
        "the file names the store its pack carries: {written}"
    );
    assert!(
        written.contains(&format!(
            "# Written by `fleet create --embedded --agent {AGENT}`."
        )),
        "{written}"
    );
    assert!(
        said.contains(&format!(
            "store: {STORE} {pin} at {commit}, installed and pinned"
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "store: ts {pin} at {commit}, which {STORE} imports"
        )),
        "{said}"
    );

    // --packs-from SERVES BOTH PACKS: the agent's comes out of the same
    // checkout, at the same tag and commit, beside the store's.
    let agent = format!("{packs}//adapters/agent/{AGENT}");
    assert_eq!(line(&agent).1, pin, "the agent's pack at the same tag");
    assert_eq!(line(&agent).2, commit, "out of the same checkout");
    assert!(
        said.contains(&format!(
            "agent: {AGENT} {pin} at {commit}, installed and pinned"
        )),
        "{said}"
    );
    assert!(
        written.contains(&format!("[agent]\nadapter = \"{AGENT}\"\n")),
        "{written}"
    );
}

/// fleet-6oc: a DEFAULT embedded create over a checkout whose store pack and
/// agent pack both import ts, as the published two do. Both install beside the
/// one ts, the file is written naming both, and tiny — a third pack importing
/// ts — then adds beside them; `prime` reads the four in one layering, the three
/// importers above the runtime they share. Before, the second importer was
/// refused for the import it declares, and no file was written.
#[test]
fn a_default_create_installs_two_packs_importing_ts_and_tiny_adds_beside_them() {
    let rig = Rig::new("importers");
    let packs = rig.a_packs_checkout();
    let out = rig.run(&["create", "--embedded", "--packs-from", &packs]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert_eq!(
        store_named(&rig.project).as_deref(),
        Some(STORE),
        "{written}"
    );
    assert!(
        written.contains(&format!("[agent]\nadapter = \"{AGENT}\"\n")),
        "{written}"
    );
    let mut installed: Vec<String> = locked(&rig.machine)
        .into_iter()
        .map(|(source, _, _)| source)
        .collect();
    installed.sort();
    assert_eq!(
        installed,
        vec![
            format!("{packs}//adapters/agent/{AGENT}"),
            format!("{packs}//adapters/store/{STORE}"),
            format!("{packs}//runtimes/ts"),
            String::from("embedded:defaults"),
        ],
        "both importers and the one runtime they share"
    );

    let added = rig.run(&[
        "pack",
        "add",
        &format!("{packs}//tiny"),
        "--version",
        fleet_core::supported::PINNED_PACKS,
    ]);
    assert_eq!(code(&added), 0, "{}", stderr(&added));

    let primed = rig.run(&["prime"]);
    assert_eq!(code(&primed), 0, "{}", stderr(&primed));
    let page = stdout(&primed);
    let line_one = page.lines().next().expect("prime printed a line");
    assert!(
        line_one.contains(&format!("packs: {STORE}, {AGENT}, tiny, ts;")),
        "line 1 names the four, the runtime last: {line_one}"
    );
}

/// `--store none`: no store pack fetched, nothing pinned but the defaults, the
/// agent's pack and the ts it imports, no `[store]` table, and the line that installs the
/// default store pack later printed with the source this call named and the
/// tag this binary pins.
#[test]
fn create_with_store_none_installs_nothing_and_prints_the_line_that_does() {
    let rig = Rig::new("store-none");
    let out = rig.run(&rig.embedded());
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");

    let line = format!(
        "fleet pack add {}//adapters/store/{STORE} --version {}",
        rig.packs,
        fleet_core::supported::PINNED_PACKS
    );
    assert!(
        said.contains(&format!("store: none installed — `{line}` installs one")),
        "{said}"
    );
    let mut sources: Vec<String> = locked(&rig.machine)
        .into_iter()
        .map(|(source, _, _)| source)
        .collect();
    sources.sort();
    let mut expected = vec![
        format!("{}//adapters/agent/{AGENT}", rig.packs),
        format!("{}//runtimes/ts", rig.packs),
        fleet_core::defaults::SOURCE.to_string(),
    ];
    expected.sort();
    assert_eq!(
        sources, expected,
        "only the defaults, the agent's pack and the ts it imports"
    );
    assert!(
        !rig.machine.join("packs").join(STORE).exists(),
        "no pack installed"
    );
    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert!(!written.contains("[store]"), "{written}");
}

/// The store's scripted answer is refused before any question and before
/// anything is written: a name no pack is installed for, and `--packs-from`
/// beside `--standalone --store none`, which would name a source for nothing —
/// a standalone project installs no agent's pack, the fleet's being the one it
/// runs on. Beside `--embedded --store none` it serves the agent's pack.
#[test]
fn create_refuses_a_store_it_does_not_install_before_anything_is_written() {
    let rig = Rig::new("store-refused");
    let out = rig.run(&[
        "create",
        "--embedded",
        "--agent",
        AGENT,
        "--store",
        "sqlite",
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 2, "{said}");
    assert!(
        said.contains(&format!(
            "no store pack answers to `sqlite` — this fleet installs {STORE}, or none"
        )),
        "{said}"
    );

    // A pipe with no mode answered: the store is refused, not the question.
    let out = rig.run(&["create", "--store", "sqlite"]);
    assert_eq!(code(&out), 2, "{}", stderr(&out));
    assert!(stderr(&out).contains("sqlite"), "{}", stderr(&out));

    let packs = rig.a_packs_checkout();
    let out = rig.run(&[
        "create",
        "--standalone",
        "--store",
        "none",
        "--packs-from",
        &packs,
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 2, "{said}");
    assert!(
        said.contains("--packs-from") && said.contains("a standalone project with --store none"),
        "{said}"
    );

    assert!(
        !rig.project.join("fleet.toml").exists(),
        "nothing was written"
    );
    assert!(
        !rig.machine.join("packs.lock").exists(),
        "nothing was pinned"
    );
}

/// A fetch that fails — here a checkout that carries no tag the binary pins,
/// the offline case's stand-in — refuses with no fleet file written, naming
/// git's reason and `--store none` as the way to create the fleet without it.
#[test]
fn create_refuses_with_no_fleet_written_when_the_store_pack_cannot_be_fetched() {
    let rig = Rig::new("store-unfetched");
    let untagged = rig.root.join("untagged");
    write(&untagged.join("README"), "no packs here\n");
    git(&untagged, &["init", "--quiet", "-b", "main"]);
    git(&untagged, &["add", "--all"]);
    git(
        &untagged,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "nothing"],
    );

    let out = rig.run(&[
        "create",
        "--embedded",
        "--agent",
        AGENT,
        "--packs-from",
        &untagged.display().to_string(),
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 1, "{said}");
    assert!(
        said.contains(
            "the store's pack was not installed, so no fleet file was written: git checkout"
        ) && said.contains("`fleet create --store none` creates the fleet without one"),
        "{said}"
    );
    assert!(!rig.project.join("fleet.toml").exists(), "no fleet file");
    assert!(!rig.machine.join("packs").join(STORE).exists(), "no pack");
}

/// A second fleet on the machine reads the pack the first installed: the
/// standalone project declared to the first finds the store pack already
/// there, leaves it as it stands, says where it came from, and names it in its
/// own file — with the prefix the store's capabilities answer, read through
/// that pack's adapter.
#[test]
fn create_leaves_a_store_pack_already_on_the_machine_as_it_stands() {
    let rig = Rig::new("store-already");
    let packs = rig.a_packs_checkout();
    let fleet = rig.root.join("the-fleet");
    std::fs::create_dir_all(&fleet).expect("the fleet's own directory is made");
    let out = rig
        .command(&[
            "create",
            "--embedded",
            "--agent",
            AGENT,
            "--packs-from",
            &packs,
        ])
        .current_dir(&fleet)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let pinned = locked(&rig.machine);

    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--packs-from",
        &packs,
        "--fleet",
        &fleet.display().to_string(),
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");
    assert!(
        said.contains(&format!(
            "store: {STORE}, already installed on this machine at {} from \
             {packs}//adapters/store/{STORE} — left as it stands",
            fleet_core::supported::PINNED_PACKS
        )),
        "{said}"
    );
    assert_eq!(locked(&rig.machine), pinned, "the lock is as it was");
    let declared = std::fs::read_to_string(rig.project.join(".fleet/project.toml"))
        .expect("the declaration landed");
    assert_eq!(
        store_named(&rig.project).as_deref(),
        Some(STORE),
        "the project's own file names its store: {declared}"
    );
    assert!(
        declared.contains("\nitem_prefix = \"zz\"\n"),
        "the prefix the pack's adapter answers: {declared}"
    );
}

/// OPT-IN, and the one arm that reaches the network: `fleet create` with no
/// `--packs-from` and every default installs the bd pack and the agent's pack
/// from the source and tag this binary pins, the real fleet-packs on GitHub,
/// and the file names both. Skipped, with a line saying so, unless
/// `FLEET_TEST_NETWORK` is set, and when it is set and the source does not
/// answer for the tag. The published bd and claude-code packs both import
/// `ts`, and install beside the one ts they share.
#[test]
fn create_installs_the_pinned_packs_from_the_published_source() {
    let arm = "create_installs_the_pinned_packs_from_the_published_source";
    if std::env::var_os("FLEET_TEST_NETWORK").is_none() {
        eprintln!("SKIP {arm}: FLEET_TEST_NETWORK is unset, and this arm reaches GitHub");
        return;
    }
    let source = fleet_core::supported::PINNED_PACKS_SOURCE;
    let pin = fleet_core::supported::PINNED_PACKS;
    let reached = Command::new("git")
        .args(["ls-remote", "--tags", source, pin])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false);
    if !reached {
        eprintln!("SKIP {arm}: {source} did not answer for {pin} — offline, or the tag is gone");
        return;
    }

    let rig = Rig::new("packs-network");
    let out = rig
        .command(&["create", "--embedded"])
        .env_remove("GIT_ALLOW_PROTOCOL")
        .output()
        .expect("the built binary runs");
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");
    for (kind, name) in [("store", STORE), ("agent", AGENT)] {
        let pack = format!("{source}//adapters/{kind}/{name}");
        assert!(
            locked(&rig.machine)
                .iter()
                .any(|(held, version, _)| *held == pack && version == pin),
            "the lock pins {pack} at {pin}"
        );
        assert!(rig
            .machine
            .join("packs")
            .join(name)
            .join(format!("adapters/{kind}/{name}/adapter.toml"))
            .is_file());
    }
    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).expect("the file landed");
    assert!(
        written.contains(&format!("[agent]\nadapter = \"{AGENT}\"\n")),
        "{written}"
    );
}

/// The embedded file, byte for byte, with the binary's own defaults
/// materialized beside it and NO PACK INSTALLED but the agent's and the ts it
/// imports (AC1).
#[test]
fn create_embedded_writes_the_smallest_file_that_runs_and_materializes_the_defaults() {
    let rig = Rig::new("embedded");
    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let written = std::fs::read_to_string(rig.project.join("fleet.toml")).unwrap();
    assert_eq!(
        written,
        embedded_listing(&rig.identity().id),
        "the file is not the expected text"
    );

    // NOTHING under packs but the agent's and the ts it imports: `create
    // --store none` installs no store pack, and the word core names no
    // directory a person could meet.
    let packs = rig.machine.join("packs");
    let mut installed: Vec<String> = std::fs::read_dir(&packs)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    installed.sort();
    assert_eq!(installed, [AGENT, "ts"], "another pack was installed");

    // The defaults: the whole embedded set, file for file, and the lock line
    // that says which binary put it there.
    let root = rig.machine.join(fleet_core::defaults::DIR);
    for file in fleet_core::embedded::FILES {
        assert_eq!(
            std::fs::read(root.join(file.path)).unwrap_or_else(|e| panic!("{}: {e}", file.path)),
            file.bytes,
            "{} is not the byte for byte default",
            file.path
        );
    }
    assert_eq!(
        fleet_core::defaults::tree_hash(&root).expect("what landed hashes"),
        fleet_core::defaults::embedded_hash(),
        "the materialized set is not the one this binary carries"
    );
    let lock = std::fs::read_to_string(rig.machine.join("packs.lock")).unwrap();
    assert!(lock.contains("embedded:defaults"), "{lock}");
    assert!(lock.contains("commit = \"embedded\""), "{lock}");
    assert!(lock.contains("name = \"defaults\""), "{lock}");
    assert!(
        stderr(&out).contains("defaults: installed"),
        "the done message does not name the defaults: {}",
        stderr(&out)
    );

    // And what a person reads after it: `prime` names the installed packs, the
    // agent's over the ts it imports, and never the defaults — which resolve
    // all the same, or
    // the rules file below line 1 would be missing.
    // Line 2 is the store's own version, and the file `create` wrote names
    // none: the project is kept on the stub before it is asked.
    common::take_a_store(&rig.project);
    let primed = rig.run(&["prime"]);
    assert_eq!(code(&primed), 0, "{}", stderr(&primed));
    let page = stdout(&primed);
    let line_one = page.lines().next().expect("prime printed a line");
    assert!(
        line_one.contains(&format!("packs: {AGENT}, ts;")),
        "line 1 names the agent's pack over its ts alone: {line_one}"
    );
    assert!(
        !line_one.contains(fleet_core::defaults::LAYER),
        "the defaults are the binary's and are on no packs line: {line_one}"
    );
    assert!(
        page.lines().count() > 2,
        "the rules file resolved through the defaults and follows line 2, the \
         store's: {page}"
    );

    // The done message.
    let said = stderr(&out);
    for line in [
        "embedded fleet",
        "shell-trap on, record on",
        "off — nothing leaves this machine",
        "seat: you — human",
        "fleet start",
    ] {
        assert!(
            said.contains(line),
            "the done message omits {line:?}: {said}"
        );
    }
    assert!(
        stdout(&out).is_empty(),
        "stdout is a script's: {}",
        stdout(&out)
    );
}

/// The person who ran `create` is the fleet's first seat, a human one, listed
/// under the identity the call minted — and nobody is handed a table to write.
#[test]
fn create_embedded_lists_its_creator_as_a_human_seat_under_a_minted_identity() {
    let rig = Rig::new("creator");
    let identity_file = rig.machine.join(identity::IDENTITY);
    assert!(!identity_file.exists(), "the rig starts with no identity");

    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // The id is READ from the minted file, so the table is checked against the
    // identity this machine now answers to and not one this arm made up.
    let mine = rig.identity();
    let fleet_toml = rig.project.join("fleet.toml");
    assert_eq!(
        std::fs::read_to_string(&fleet_toml).unwrap(),
        embedded_listing(&mine.id),
        "the file is EMBEDDED followed by one human table"
    );

    let said = stderr(&out);
    assert!(
        said.contains(&format!(
            "seat: you — human human-{}, listed as [seats.{}] — {}",
            mine.id.short(),
            mine.id,
            fleet_toml.display()
        )),
        "the seat line names the table and the file: {said}"
    );
    assert!(
        said.contains(&format!(
            "identity: minted — who acts here when no --by is given — {}",
            identity_file.display()
        )),
        "the mint is said, with its path: {said}"
    );
    for gone in ["first seat:", "[seats.a-seat]", "git worktree add"] {
        assert!(
            !said.contains(gone),
            "the hand-written first-seat block is gone, and {gone:?} is in: {said}"
        );
    }

    // A SECOND create on the same machine, in another project, lists the same
    // person: the identity is the machine's, and it is minted once.
    let second = rig.root.join("b-project");
    std::fs::create_dir_all(&second).unwrap();
    let out = rig
        .command(&rig.embedded())
        .current_dir(&second)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(second.join("fleet.toml")).unwrap(),
        embedded_listing(&mine.id),
        "the second fleet lists the same identity"
    );
    assert_eq!(rig.identity(), mine, "the identity is not re-minted");
    let said = stderr(&out);
    assert!(
        said.contains(&format!("listed as [seats.{}]", mine.id)),
        "{said}"
    );
    assert!(
        !said.contains("identity:") && !said.contains("minted"),
        "nothing was minted, so nothing says so: {said}"
    );
}

/// A standalone project declared to a fleet that already lists this machine's
/// person: the listing is said and not refused, and the fleet's file is left
/// byte for byte as it was.
#[test]
fn create_standalone_into_a_fleet_that_already_lists_the_identity_says_so_and_writes_nothing() {
    let rig = Rig::new("standalone-listed");
    let fleet = rig.root.join("the-fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    let out = rig
        .command(&rig.embedded())
        .current_dir(&fleet)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let policy = fleet.join("fleet.toml");
    let before = std::fs::read(&policy).unwrap();
    let mine = rig.identity();

    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--store",
        "none",
        "--fleet",
        &fleet.display().to_string(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let said = stderr(&out);
    assert!(
        said.contains(&format!(
            "seat: you — human human-{}, already listed",
            mine.id.short()
        )),
        "{said}"
    );
    assert_eq!(
        std::fs::read(&policy).unwrap(),
        before,
        "the fleet's file is byte-unchanged"
    );
    assert!(!said.contains("identity:"), "nothing was minted: {said}");
}

/// A fleet's file this verb cannot read as a roster is not a refusal of the
/// declaration: the project is registered, the seat line says why nobody was
/// listed and which verb lists them, and the call answers 0.
#[test]
fn create_standalone_into_a_fleet_whose_seats_do_not_read_says_so_and_answers_0() {
    let rig = Rig::new("standalone-unread");
    let fleet = rig.root.join("the-fleet");
    let policy = fleet.join("fleet.toml");
    write(&policy, "[seats.alpha]\nkind = \"agent\"\n");
    let before = std::fs::read(&policy).unwrap();

    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--store",
        "none",
        "--fleet",
        &fleet.display().to_string(),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let said = stderr(&out);
    assert!(said.contains("seat: not listed — "), "{said}");
    assert!(
        said.contains("[seats.alpha] is keyed by a name"),
        "the why is the roster's: {said}"
    );
    assert!(
        said.contains("; fleet seat add --human lists you"),
        "{said}"
    );
    assert!(said.contains("registered"), "{said}");
    assert_eq!(
        std::fs::read(&policy).unwrap(),
        before,
        "nothing was written"
    );
}

/// An identity that cannot be minted — a directory standing where its file
/// goes — is a fleet that lists nobody: exit 3, said, and the fleet.toml the
/// call wrote stays written without a human table.
///
/// A machine directory that cannot be written in at all is refused BEFORE any
/// fleet file now: the agent's pack goes in first, over the defaults, and
/// neither can land there.
#[test]
fn create_embedded_with_an_unmintable_identity_exits_3_and_lists_nobody() {
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::new("unmintable");
    std::fs::create_dir_all(rig.machine.join(identity::IDENTITY)).unwrap();

    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    let said = stderr(&out);
    assert!(
        said.contains("fleet create: the fleet was written and lists nobody: "),
        "{said}"
    );
    assert!(
        said.contains(" — fleet seat add --human finishes it"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(rig.project.join("fleet.toml")).unwrap(),
        embedded(),
        "the fleet.toml is there, without a human table"
    );
    assert!(rig.machine.join(identity::IDENTITY).is_dir());

    let unwritable = Rig::new("unwritable");
    std::fs::create_dir_all(&unwritable.machine).unwrap();
    std::fs::set_permissions(&unwritable.machine, std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = unwritable.run(&unwritable.embedded());
    // Writable again BEFORE any assertion, so a failing arm still cleans up.
    std::fs::set_permissions(&unwritable.machine, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        !unwritable.project.join("fleet.toml").exists(),
        "no fleet file: {}",
        stderr(&out)
    );
}

/// The store refusal, from the other end: the controller never initialises and
/// never rewrites a project's work-graph store.
#[test]
fn create_never_touches_the_projects_work_graph_store() {
    let rig = Rig::new("store");
    let store = rig.project.join(".tracker");
    let before = std::fs::metadata(&store).unwrap().modified().unwrap();
    let listed = |dir: &Path| {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let names_before = listed(&store);

    let out = rig.run(&rig.embedded());
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    assert_eq!(
        std::fs::metadata(&store).unwrap().modified().unwrap(),
        before,
        "the store directory was written into"
    );
    assert_eq!(listed(&store), names_before, "the store gained a file");
    assert_eq!(
        std::fs::read_to_string(store.join("config.yaml")).unwrap(),
        "# the project's own store config\nissue-prefix: ap\n",
        "the store's own config was rewritten"
    );

    // The control the three readings above need: this call DID write, so an
    // unchanged store is a store this verb left alone and not a call that did
    // nothing.
    assert!(rig.project.join("fleet.toml").is_file());
}

/// A standalone project runs on its fleet's agent (ruling 11): nothing asks
/// which, nothing is installed for it, and an `--agent` naming another adapter
/// than the fleet's is a usage refusal naming the fleet's, before anything is
/// written. Leaving the flag out is no refusal.
#[test]
fn create_standalone_refuses_an_agent_that_is_not_the_fleets() {
    let rig = Rig::new("standalone-agent");
    let fleet = rig.root.join("the-fleet");
    let fleet_toml = fleet.join("fleet.toml");
    write(&fleet_toml, "[agent]\nadapter = \"quill\"\n");
    let named = fleet.display().to_string();
    let out = rig.run(&[
        "create",
        "--standalone",
        "--agent",
        AGENT,
        "--store",
        "none",
        "--fleet",
        &named,
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 2, "{said}");
    assert!(
        said.contains(&format!(
            "--agent {AGENT} is not this fleet's agent — {} runs `quill`, and a project \
             declared to it runs on that one; drop --agent",
            fleet_toml.display()
        )),
        "{said}"
    );
    assert!(!rig.project.join(".fleet/project.toml").exists());
    assert!(!rig.machine.join("config.json").exists());

    let out = rig.run(&[
        "create",
        "--standalone",
        "--store",
        "none",
        "--fleet",
        &named,
    ]);
    let said = stderr(&out);
    assert_eq!(code(&out), 0, "{said}");
    assert!(!said.contains("agent:"), "nothing installed for it: {said}");
    assert!(!rig.machine.join("packs").join(AGENT).exists());
}

/// The standalone file, byte for byte, the register, and the event.
#[test]
fn create_standalone_declares_the_project_registers_it_and_says_so_on_the_stream() {
    // A fleet on the machine first: `--standalone` declares a project TO one.
    let host = Rig::new("standalone-host");
    host.created();
    // A fleet is registered on a machine once its first start has written the
    // seat list that names its policy file — which is exactly what
    // `--standalone` looks for, and what its refusal names.
    assert_eq!(code(&host.run(&["start"])), 0);
    assert_eq!(code(&host.run(&["stop"])), 0);
    // The host's own fleet.toml is embedded, so a second create in the same
    // directory would refuse. The standalone project is a second directory that
    // shares the machine.
    //
    // The default store's pack is on the machine, added by hand: `--store
    // none` writes no `[store]` table, so the project names no store and the
    // default name opens that pack's adapter, whose capabilities name the
    // prefix the declaration carries.
    let packs = host.a_packs_checkout();
    let added = host.run(&[
        "pack",
        "add",
        &format!("{packs}//adapters/store/{STORE}"),
        "--version",
        fleet_core::supported::PINNED_PACKS,
    ]);
    assert_eq!(code(&added), 0, "{}", stderr(&added));
    let second = host.root.join("b-project");
    std::fs::create_dir_all(&second).unwrap();
    let out = host
        .command(&[
            "create",
            "--standalone",
            "--agent",
            AGENT,
            "--store",
            "none",
        ])
        .current_dir(&second)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // BYTE FOR BYTE against the expected text, exactly as the embedded file is
    // compared: a handful of `contains` checks all pass on a file that dropped
    // a line.
    let written = std::fs::read_to_string(second.join(".fleet/project.toml")).unwrap();
    assert_eq!(
        written,
        standalone_text(
            "b-project",
            Some("zz"),
            &second,
            &host.root.join("b-project-worktrees")
        ),
        "the declaration is not the expected text"
    );

    // THE DONE MESSAGE IN STANDALONE MODE reports only what this call did. The
    // guards and telemetry lines belong to a policy file this mode does not
    // write, and the seat belongs to the FLEET's own file: a `[seats]` table in
    // a project's declaration does nothing at all. The host's own create listed
    // this machine's person there already, so this one says so.
    let said = stderr(&out);
    let fleet_toml = host.project.join("fleet.toml").display().to_string();
    assert!(said.contains("standalone fleet"), "{said}");
    assert!(
        said.contains(&format!("already listed — {fleet_toml}")),
        "the seat line names the fleet's own file, not the declaration: {said}"
    );
    assert!(
        !said.contains(&format!(
            "listed — {}",
            second.join(".fleet/project.toml").display()
        )),
        "{said}"
    );
    assert!(!said.contains("first seat:"), "{said}");
    assert!(
        !said.contains("shell-trap on"),
        "this mode wrote no guards table to report: {said}"
    );
    assert!(
        !said.contains("telemetry:"),
        "this mode wrote no telemetry key to report: {said}"
    );

    // THE CONTROL: the embedded mode's own done message DOES carry both lines,
    // so the two absences above are this mode's and not lines the verb never
    // prints. A third directory, so neither of the two above is disturbed.
    let third = host.root.join("d-project");
    std::fs::create_dir_all(&third).unwrap();
    let embedded = host
        .command(&host.embedded())
        .current_dir(&third)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&embedded), 0, "{}", stderr(&embedded));
    let embedded = stderr(&embedded);
    assert!(embedded.contains("shell-trap on, record on"), "{embedded}");
    assert!(embedded.contains("telemetry:"), "{embedded}");
    assert!(
        embedded.contains(&format!(
            "listed as [seats.{}] — {}",
            host.identity().id,
            third.join("fleet.toml").display()
        )),
        "an embedded fleet's seat is listed in its own file: {embedded}"
    );

    let register = std::fs::read_to_string(host.machine.join("projects.toml")).unwrap();
    assert!(register.contains("[[project]]"), "{register}");
    assert!(
        register.contains(&second.display().to_string()),
        "{register}"
    );
    assert!(register.contains("name = \"b-project\""), "{register}");

    let registered: Vec<_> = host
        .stream()
        .into_iter()
        .filter(|e| e["type"] == "project.registered")
        .collect();
    assert_eq!(registered.len(), 1, "one event per registration");
    assert_eq!(registered[0]["payload"]["name"], "b-project");
    assert_eq!(
        registered[0]["payload"]["root"],
        second.display().to_string()
    );

    // A DECLARED project is registered, not collided with: a second call reads
    // the file's keys, writes nothing over it, and leaves the register a set
    // rather than a log.
    let out = host
        .command(&[
            "create",
            "--standalone",
            "--agent",
            AGENT,
            "--store",
            "none",
        ])
        .current_dir(&second)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stderr(&out).contains("already"), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(second.join(".fleet/project.toml")).unwrap(),
        written,
        "a declaration somebody wrote is never written over"
    );
    assert_eq!(
        std::fs::read_to_string(host.machine.join("projects.toml")).unwrap(),
        register
    );
    assert_eq!(
        host.stream()
            .into_iter()
            .filter(|e| e["type"] == "project.registered")
            .count(),
        1,
        "one event per registration, and this was not one"
    );

    // And a declaration that names no project is refused with what is missing.
    let third = host.root.join("c-project");
    write(
        &third.join(".fleet/project.toml"),
        "[landing]\nci_marker = \"printf '[skip ci]'\"\n",
    );
    let out = host
        .command(&[
            "create",
            "--standalone",
            "--agent",
            AGENT,
            "--store",
            "none",
        ])
        .current_dir(&third)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("[project] name"), "{}", stderr(&out));

    // A project whose store names no prefix — here no store at all, the pack
    // taken back off the machine — is declared with the commented line, and
    // never with a prefix guessed.
    let removed = host.run(&[
        "pack",
        "remove",
        &format!("{packs}//adapters/store/{STORE}"),
    ]);
    assert_eq!(code(&removed), 0, "{}", stderr(&removed));
    let bare = host.root.join("e-project");
    std::fs::create_dir_all(&bare).unwrap();
    let out = host
        .command(&[
            "create",
            "--standalone",
            "--agent",
            AGENT,
            "--store",
            "none",
        ])
        .current_dir(&bare)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(bare.join(".fleet/project.toml")).unwrap(),
        standalone_text(
            "e-project",
            None,
            &bare,
            &host.root.join("e-project-worktrees")
        ),
        "the declaration is not the expected text"
    );
}

/// A DECLARED PROJECT WINS AT ITS OWN LEVEL: `--standalone` registers the
/// declaration in a directory that also carries a `fleet.toml`, because the
/// resolver reads that directory as a standalone project and the neighbour is
/// some other tool's file rather than this project's fleet.
/// `--embedded` still refuses there — the refusal is one mode's now, not both —
/// and the keys of a file somebody wrote are ALL read, not only the one the
/// register is keyed on.
#[test]
fn a_declaration_beside_a_fleet_toml_is_registered_and_every_key_is_read() {
    let host = Rig::new("declared-beside");
    host.created();
    assert_eq!(code(&host.run(&["start"])), 0);
    assert_eq!(code(&host.run(&["stop"])), 0);

    let both = host.root.join("both");
    let worktrees = host.root.join("both-worktrees");
    write(
        &both.join("fleet.toml"),
        "# somebody else's file, at this project's root\n[reference]\nharness = true\n",
    );
    let declaration = format!(
        "[project]\nname = \"both\"\nitem_prefix = \"bo\"\n\
         primary = \"{primary}\"\nworktrees = \"{worktrees}\"\n",
        primary = both.display(),
        worktrees = worktrees.display(),
    );
    write(&both.join(".fleet/project.toml"), &declaration);

    let out = host
        .command(&[
            "create",
            "--standalone",
            "--agent",
            AGENT,
            "--store",
            "none",
        ])
        .current_dir(&both)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(both.join(".fleet/project.toml")).unwrap(),
        declaration,
        "a declaration somebody wrote is never written over"
    );
    let register = std::fs::read_to_string(host.machine.join("projects.toml")).unwrap();
    assert!(
        register.contains(&both.display().to_string()) && register.contains("name = \"both\""),
        "the project is registered: {register}"
    );
    // AND the worktrees directory it names is not there. A registration is not
    // the moment a seat's checkout exists — which is why that key is read as a
    // place rather than as a directory that has to be on the machine already.
    assert!(
        !worktrees.exists(),
        "{} was registered with its worktrees directory still unmade",
        worktrees.display()
    );

    // `--embedded` in the same directory still refuses, naming the neighbour:
    // the declaration excuses the standalone mode and nothing else.
    let out = host
        .command(&host.embedded())
        .current_dir(&both)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&both.join("fleet.toml").display().to_string()),
        "the refusal names the file that is already here: {}",
        stderr(&out)
    );

    // Every key is read: the same declaration with its prefix taken out is
    // refused, with the key it is missing spelled.
    let short = host.root.join("short");
    write(
        &short.join(".fleet/project.toml"),
        &format!(
            "[project]\nname = \"short\"\nprimary = \"{primary}\"\nworktrees = \"{worktrees}\"\n",
            primary = short.display(),
            worktrees = host.root.join("short-worktrees").display(),
        ),
    );
    let out = host
        .command(&[
            "create",
            "--standalone",
            "--agent",
            AGENT,
            "--store",
            "none",
        ])
        .current_dir(&short)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("[project] item_prefix"),
        "the refusal names the key: {}",
        stderr(&out)
    );
    assert!(
        !std::fs::read_to_string(host.machine.join("projects.toml"))
            .unwrap()
            .contains(&short.display().to_string()),
        "and a refused declaration is not registered"
    );
}

// ---- fleet start and fleet stop ---------------------------------------------

/// The first run's own idempotence: a second start repeats none of it and says
/// so, line by line.
#[test]
fn a_second_start_writes_nothing_and_says_so() {
    let rig = Rig::new("idempotent");
    rig.created();

    let first = rig.run(&["start"]);
    assert_eq!(code(&first), 0, "{}", stderr(&first));
    assert!(
        stderr(&first).contains("seat list: written"),
        "{}",
        stderr(&first)
    );
    assert!(
        stderr(&first).contains("service file: written"),
        "{}",
        stderr(&first)
    );

    let service = rig.service_file();
    let text = std::fs::read_to_string(&service).unwrap();
    let stamp = std::fs::metadata(&service).unwrap().modified().unwrap();
    let seats = std::fs::read_to_string(rig.machine.join("config.json")).unwrap();

    // The stub leaves a pid behind, so a second start would refuse as already
    // running; the stop in between is the fleet's own verb.
    let stopped = rig.run(&["stop"]);
    assert_eq!(code(&stopped), 0, "{}", stderr(&stopped));

    let second = rig.run(&["start"]);
    assert_eq!(code(&second), 0, "{}", stderr(&second));
    assert!(
        stderr(&second).contains("seat list: already"),
        "{}",
        stderr(&second)
    );
    assert!(
        stderr(&second).contains("service file: already"),
        "{}",
        stderr(&second)
    );
    assert!(
        stderr(&second).contains("repeated none of it"),
        "{}",
        stderr(&second)
    );
    assert_eq!(std::fs::read_to_string(&service).unwrap(), text);
    assert_eq!(
        std::fs::metadata(&service).unwrap().modified().unwrap(),
        stamp
    );
    assert_eq!(
        std::fs::read_to_string(rig.machine.join("config.json")).unwrap(),
        seats
    );
}

/// AC5 — the defaults a REBUILT BINARY carries, on a machine that already holds
/// an older set.
///
/// The set this binary carries cannot be moved from a test, so the OTHER side
/// is: the materialized directory with one file withheld, re-pinned at its own
/// hash, which is exactly what an earlier binary left behind. The version is
/// the same string on both sides, because it is the crate's and a rebuild does
/// not move it.
#[test]
fn start_refreshes_a_defaults_set_this_binary_no_longer_carries() {
    let rig = Rig::new("defaults-refresh");
    assert_eq!(code(&rig.run(&rig.embedded())), 0);

    let root = rig.machine.join(fleet_core::defaults::DIR);
    let dropped = root.join("assets/brief.md");
    let body = std::fs::read(&dropped).expect("the default reads");
    std::fs::remove_file(&dropped).expect("the older set is one file short");
    let older = fleet_core::defaults::tree_hash(&root).expect("the older set hashes");
    assert_ne!(
        older,
        fleet_core::defaults::embedded_hash(),
        "the fixture moved nothing, so the reading below would pass on a no-op"
    );
    let lock_path = rig.machine.join(fleet_core::lock::LOCK);
    let mut lines = fleet_core::lock::read(&lock_path).expect("the lock reads");
    for line in &mut lines {
        if line.source == fleet_core::defaults::SOURCE {
            line.tree = Some(older.clone());
        }
    }
    fleet_core::lock::write(&lock_path, &lines).expect("the older line is pinned");

    let started = rig.run(&["start"]);
    assert_eq!(code(&started), 0, "{}", stderr(&started));
    assert!(
        stderr(&started).contains("defaults: refreshed"),
        "the start does not say the copy was replaced: {}",
        stderr(&started)
    );
    assert_eq!(
        std::fs::read(&dropped).expect("the default is back"),
        body,
        "the copy on the machine is not the set the binary carries"
    );

    // A start over the same set writes nothing and says so.
    assert_eq!(code(&rig.run(&["stop"])), 0);
    let again = rig.run(&["start"]);
    assert_eq!(code(&again), 0, "{}", stderr(&again));
    assert!(
        stderr(&again).contains("defaults: already at"),
        "{}",
        stderr(&again)
    );
}

/// The already-running refusal reads its pid from the manager and its tick FROM
/// THE FILE — never from the process table.
#[test]
fn start_refuses_a_controller_that_is_already_running_with_its_pid_and_its_last_tick() {
    let rig = Rig::new("running");
    rig.created();
    assert_eq!(code(&rig.run(&["start"])), 0);

    // A published projection, which is what the last tick is read out of.
    write(
        &rig.machine.join("projection.json"),
        "{\"generated_at\": \"2026-09-12T01:02:03Z\"}\n",
    );
    let out = rig.run(&["start"]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("4242"), "the pid: {}", stderr(&out));
    assert!(
        stderr(&out).contains("2026-09-12T01:02:03Z"),
        "the last tick: {}",
        stderr(&out)
    );

    // Nothing was loaded by the refusal: the only load on the record is the
    // first start's.
    assert_eq!(rig.of_class("load").len(), 1, "{:?}", rig.calls());
}

/// A fleet whose agent's pack was taken out is could-not-tell, naming the line
/// that installs it, and NOTHING IS LOADED — the refusal comes before the
/// first-run work. The default's name is a name like any other: no pack
/// carrying it is no adapter at all, whether the fleet's file writes the key
/// as `create` does or leaves it out.
///
/// RED-PROOF: with the default's name opening an adapter of its own where no
/// pack carries it, the start goes past the opener and the line is never said.
#[test]
fn start_refuses_a_fleet_whose_agent_pack_is_removed_naming_the_pack_add_line() {
    for (label, written) in [("agent-gone-named", true), ("agent-gone", false)] {
        let rig = Rig::new(label);
        rig.created().without_the_agent_pack();
        if !written {
            rig.agent_is(None);
        }
        let out = rig.run(&["start"]);
        let said = stderr(&out);
        assert_eq!(code(&out), 3, "{label}: {said}");
        assert!(
            said.contains(&format!(
                "no agent adapter named `{AGENT}` in the installed packs — `{}` installs the one \
                 fleet-packs carries — nothing was loaded",
                fleet_core::pack::AdapterKind::Agent.pack_line(
                    fleet_core::supported::PINNED_PACKS_SOURCE,
                    AGENT,
                    fleet_core::supported::PINNED_PACKS
                )
            )),
            "{label}: {said}"
        );
        assert!(rig.of_class("load").is_empty(), "{:?}", rig.calls());
        assert!(
            !rig.machine.join("config.json").exists(),
            "{label}: no first-run work ran"
        );
    }
}

/// An agent adapter named by path whose capabilities do not answer is a
/// controller that could start no seat: could-not-tell, naming the adapter,
/// and NOTHING IS LOADED.
#[test]
fn start_refuses_an_agent_adapter_whose_capabilities_exit_3_and_loads_nothing() {
    let rig = Rig::new("agent-exec-3");
    rig.created();
    let adapter = rig.root.join("agent-adapter");
    write(
        &adapter,
        "#!/bin/sh\ncat > /dev/null\necho 'the agent is not set up' >&2\nexit 3\n",
    );
    executable(&adapter);
    rig.agent_is(Some(&adapter.display().to_string()));
    let out = rig
        .command(&["start"])
        // The stub is a script written a moment ago, whose first exec this
        // platform can hold past the agent's own bound.
        .env("FLEET_AGENT_TIMEOUT_MS", "300000")
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    let said = stderr(&out);
    assert!(
        said.contains(&format!(
            "{} capabilities could not tell: the agent is not set up",
            adapter.display()
        )),
        "{said}"
    );
    assert!(said.contains("nothing was loaded"), "{said}");
    assert!(rig.of_class("load").is_empty(), "{:?}", rig.calls());
    assert!(
        !rig.machine.join("config.json").exists(),
        "no first-run work ran"
    );
}

/// A fleet whose file names no agent adapter, with the default's pack
/// installed, starts THROUGH THAT PACK: the gate asks the pack's adapter its
/// capabilities and its version, and the service is loaded. The same fleet
/// naming the adapter, as `create` writes it, starts the same way.
///
/// The pack is the rig's agent stub, which logs every call to the state under
/// the fleet's root once one is scripted there.
#[test]
fn start_loads_a_fleet_whose_agent_is_the_installed_default_pack() {
    for (label, written) in [("agent-pack-named", true), ("agent-pack", false)] {
        let rig = Rig::new(label);
        rig.created();
        if !written {
            rig.agent_is(None);
        }
        fleet_controller::test_support::agent_stub::script(&rig.project, |_| {});
        let out = rig.run(&["start"]);
        assert_eq!(code(&out), 0, "{label}: {}", stderr(&out));
        assert_eq!(rig.of_class("load").len(), 1, "{:?}", rig.calls());
        let asked: Vec<&str> = fleet_controller::test_support::agent_stub::calls(&rig.project)
            .iter()
            .map(|call| call.verb)
            .collect();
        assert!(
            asked.starts_with(&["capabilities", "version"]),
            "{label}: the pack's adapter answered the gate: {asked:?}"
        );
    }
}

/// A tmux that does not resolve is could-not-tell beside the agent binary, and
/// NOTHING IS LOADED: every seat's session runs on it, so a controller without
/// one would start no seat at all.
#[test]
fn start_refuses_an_unresolvable_tmux_and_loads_nothing() {
    let rig = Rig::new("no-tmux");
    rig.created();
    let out = rig
        .command(&["start"])
        .env(common::hermetic::TMUX_BIN, rig.root.join("not-a-tmux"))
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("nothing was loaded"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains(&rig.root.join("not-a-tmux").display().to_string()),
        "{}",
        stderr(&out)
    );
    assert!(rig.of_class("load").is_empty(), "{:?}", rig.calls());
    assert!(
        !rig.machine.join("config.json").exists(),
        "no first-run work ran"
    );
}

/// `--foreground` IS THE SERVICE'S LOOP, workflows included: a waiting run
/// whose stream has moved is advanced under it.
///
/// WHY THE READING IS A REFUSAL. The seam a foreground start wires is the real
/// engine, and the engine resolves a run by asking each registered project's
/// store for its record. The run named here is in no store, so the advance the
/// fold decided on is visible as the run pass's own refusal, naming that run —
/// a reading that needs no workflow to execute and nothing in the project's
/// store, which is an empty one on the stub. A
/// foreground loop wired with no seam never folds the stream at all: it prints
/// nothing about any run, which is the failure this arm answers with.
#[test]
fn the_foreground_loop_advances_a_waiting_run() {
    /// In no store, on this box or any other: the resolution this arm wants is
    /// the one that finds nothing.
    const RUN: &str = "no-such-run-9f3c";

    let rig = Rig::new("foreground-runs");
    rig.created();
    common::take_a_store(&rig.project);

    // `run.waiting` recording position 0, so the controller's own
    // `controller.started` is a line above what the run left behind — which is
    // the fold's test for "somebody else wrote something".
    let stream = rig.machine.join("events.jsonl");
    EventLog::open(&stream)
        .append(
            runs::RUN_WAITING,
            &fleet_controller::events::ActorRef::new(fleet_controller::events::RUN, "a-runner"),
            serde_json::json!({ "run": RUN, "wake": { "for": "a line" }, "seq": 0 }),
        )
        .expect("the stream takes the waiting line");

    let said = rig.root.join("foreground.err");
    let mut child = rig
        .command(&["start", "--foreground"])
        .stderr(Stdio::from(
            std::fs::File::create(&said).expect("the log file is made"),
        ))
        .spawn()
        .expect("the built binary runs");
    // The child is killed on the way out whatever was read, so a deadline this
    // arm misses does not leave a poll loop running for the rest of the suite.
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if std::fs::read_to_string(&said)
            .unwrap_or_default()
            .contains(RUN)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let out = Command::new(kill_bin())
        .args(["-TERM", &child.id().to_string()])
        .output()
        .expect("kill runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    child.wait().expect("the child is reaped");

    let said = std::fs::read_to_string(&said).unwrap_or_default();
    assert!(
        said.contains(fleet_controller::runs::REFUSED),
        "the foreground loop's run pass never ran: {said}"
    );
    assert!(
        said.contains(RUN),
        "the run pass ran and did not reach this run: {said}"
    );
}

/// The `[seats.<id>]` tables, rendered: every active agent seat a row keyed by
/// its id, carrying its kind and the seat's own name where it has one, and no
/// row for a parked seat or for a person's.
#[test]
fn the_seats_table_is_rendered_into_rows_and_a_transient_row_survives_it() {
    let rig = Rig::new("seats");
    rig.created().policy_says(&format!(
        "\n[seats.{SEAT_A}]\nkind = \"agent\"\nname = \"Kite\"\nmodel = \"a-model\"\n\
         \n[seats.{SEAT_B}]\nkind = \"agent\"\n\
         \n[seats.{SEAT_C}]\nkind = \"agent\"\nstatus = \"parked\"\n\
         \n[seats.{SEAT_H}]\nkind = \"human\"\nname = \"Orla\"\n",
    ));
    // A transient row, an unknown key and a row keyed by its name alone,
    // written before the render: all three are what the document-edit
    // discipline exists for, and the name-keyed row is the shape the clean
    // break retired — not a seat this render reconciles, and not migrated. B's
    // row is already there under its id and still carries the machine name and
    // the `chosen_name` a render before this one wrote, both of which this
    // render owns and takes off: B has no name of its own.
    write(
        &rig.machine.join("config.json"),
        &format!(
            "{{\n  \"fleet_toml\": \"{}\",\n  \"autopilot\": {{\"on\": true}},\n  \
             \"children\": [\n    {{\"id\": \"{SPAWNED}\", \"kind\": \"agent\", \
             \"transient\": true, \"spawned_by\": \"somebody\", \
             \"worktrees\": {{\"a-project\": \"/wt/t1\"}}}},\n    \
             {{\"id\": \"{SEAT_B}\", \"name\": \"agent-e8a04b17\", \"chosen_name\": \"Pell\", \
             \"worktrees\": {{\"a-project\": \"/wt/b\"}}}},\n    \
             {{\"id\": \"{GONE}\", \"name\": \"Gone\", \
             \"worktrees\": {{\"a-project\": \"/wt/gone\"}}}},\n    \
             {{\"name\": \"a-row-keyed-by-its-name\", \
             \"worktrees\": {{\"a-project\": \"/wt/old\"}}}}\n  ]\n}}\n",
            rig.project.join("fleet.toml").display()
        ),
    );
    let before = rig.seat_list();

    let out = rig.run(&["start"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let after = rig.seat_list();
    let rows = after["children"].as_array().expect("children is an array");

    let row = |id: &str| {
        rows.iter()
            .find(|r| r["id"] == id)
            .unwrap_or_else(|| panic!("no row for {id}: {after}"))
    };
    assert_eq!(
        row(SEAT_A)["name"],
        "Kite",
        "the row carries the seat's own name, not a slug"
    );
    assert_eq!(row(SEAT_A)["kind"], "agent");
    assert_eq!(row(SEAT_A)["model"], "a-model");
    assert_eq!(
        row(SEAT_A)["worktrees"]["a-project"],
        rig.root
            .join("a-project-worktrees/kite-93b9739a")
            .display()
            .to_string(),
        "with no directory ending in its short id, a seat's worktree is its machine name"
    );
    assert!(
        row(SEAT_B).get("name").is_none(),
        "a seat with no name carries none on its row: {after}"
    );
    assert_eq!(row(SEAT_B)["kind"], "agent");
    assert_eq!(
        row(SEAT_B)["model"],
        model(),
        "a row naming no model takes the default the fleet's agent declares"
    );
    assert_eq!(
        row(SEAT_B)["worktrees"]["a-project"],
        rig.root
            .join("a-project-worktrees/agent-e8a04b17")
            .display()
            .to_string(),
        "a seat with no name takes its kind as its slug"
    );
    for (id, who) in [(SEAT_C, "the parked seat"), (SEAT_H, "the human seat")] {
        assert!(
            !rows.iter().any(|r| r["id"] == id),
            "{who} is not rendered: {after}"
        );
    }
    assert!(
        !rows.iter().any(|r| r["id"] == GONE),
        "a row whose seat left the table is dropped: {after}"
    );
    assert_eq!(
        rows.len(),
        4,
        "the two agent rows, the transient and the name-keyed row: {after}"
    );
    assert!(
        rows.iter().all(|r| r.get("chosen_name").is_none()),
        "no row carries chosen_name: {after}"
    );
    assert!(
        stderr(&out).contains("seats: 2 row(s) rendered — 1 added, 1 updated, 1 dropped"),
        "{}",
        stderr(&out)
    );
    // Two people: Orla, and the one `create` listed for the machine it ran on.
    assert!(
        stderr(&out).contains("2 human seat(s) listed and not rendered"),
        "{}",
        stderr(&out)
    );

    // BYTE-IDENTICAL: the transient row, the name-keyed row and the unknown key
    // survive, field for field, because the document is edited and never
    // re-serialized.
    assert_eq!(*row(SPAWNED), before["children"][0], "{after}");
    let keyless: Vec<&serde_json::Value> = rows.iter().filter(|r| r["id"].is_null()).collect();
    assert_eq!(keyless, vec![&before["children"][3]], "{after}");
    assert_eq!(after["autopilot"], before["autopilot"]);
    assert_eq!(after["fleet_toml"], before["fleet_toml"]);

    // And the render is a fixed point: a second one moves no byte at all.
    let bytes = std::fs::read_to_string(rig.machine.join("config.json")).unwrap();
    assert_eq!(code(&rig.run(&["stop"])), 0);
    assert_eq!(code(&rig.run(&["start"])), 0);
    assert_eq!(
        std::fs::read_to_string(rig.machine.join("config.json")).unwrap(),
        bytes,
        "a second render rewrote the document"
    );

    // The worktree directory is NOT created: starting a named seat is the
    // person's, and `fleet seat add --agent` names the git command.
    assert!(!rig.root.join("a-project-worktrees").exists());
}

/// THE CLEAN BREAK: a seat table keyed by a name is refused, and nothing
/// migrates it. A start that read around it would come up short of the seat
/// its person wrote down, so it stops before the render with core's refusal.
#[test]
fn a_seat_table_keyed_by_a_name_is_refused_by_start() {
    let rig = Rig::new("seats-by-name");
    rig.created()
        .policy_says("\n[seats.alpha]\nkind = \"agent\"\n");

    let out = rig.run(&["start"]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("[seats.alpha] is keyed by a name"),
        "{}",
        stderr(&out)
    );
    assert!(
        !rig.seat_list()["children"]
            .as_array()
            .expect("children is an array")
            .iter()
            .any(|row| row["name"] == "alpha"),
        "no row is rendered for a table keyed by a name: {}",
        rig.seat_list()
    );
    assert!(
        rig.of_class("load").is_empty(),
        "nothing is loaded after the refusal: {:?}",
        rig.calls()
    );
}

/// `config.json` is local and beats policy per key, through the same readers
/// the file goes through.
#[test]
fn the_seat_list_overrides_policy_per_key_and_a_zero_falls_back_to_it() {
    let rig = Rig::new("override");
    rig.created()
        .policy_says("\n[controller]\npoll_seconds = 7\ndefault_model = \"from-policy\"\n");
    assert_eq!(code(&rig.run(&["start"])), 0);
    assert_eq!(code(&rig.run(&["stop"])), 0);

    // The machine's own answer for one key, a zero for a second, and a key no
    // policy key answers to.
    let mut seats = rig.seat_list();
    seats["controller"] = serde_json::json!({
        "poll_seconds": 11,
        "rest_threshold_tokens": 0,
        "not_a_policy_key": "ignored",
    });
    write(
        &rig.machine.join("config.json"),
        &format!("{}\n", serde_json::to_string_pretty(&seats).unwrap()),
    );

    let out = rig.run(&["observe", "--once"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("projection.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        published["fleet"]["poll_seconds"], 11,
        "the machine's answer is the effective value"
    );
    assert!(
        stderr(&out).contains("not_a_policy_key"),
        "an unknown key is named once: {}",
        stderr(&out)
    );

    // A KNOWN KEY WITH A VALUE OF THE WRONG SHAPE LOSES ITSELF AND NO OTHER.
    // Read as one object it would fail the whole deserialization and drop
    // every override on the machine, silently, because a known key never
    // reaches the unknown list.
    let mut seats = rig.seat_list();
    seats["controller"] = serde_json::json!({
        "poll_seconds": 11,
        "rest_threshold_tokens": "a string where a count goes",
    });
    write(
        &rig.machine.join("config.json"),
        &format!("{}\n", serde_json::to_string_pretty(&seats).unwrap()),
    );
    let out = rig.run(&["observe", "--once"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("projection.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        published["fleet"]["poll_seconds"], 11,
        "the good key beside the bad one still lands: {published}"
    );
    assert!(
        stderr(&out).contains("rest_threshold_tokens"),
        "the bad key is named: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("wrong shape"),
        "and why: {}",
        stderr(&out)
    );

    // THE ZERO CONTROL, on the one key the projection publishes: a zero in the
    // machine's file falls back to POLICY's figure and not to the compiled
    // default, which is the same non-reading a zero in the policy file is. The
    // three numbers are distinct on purpose — 11 the machine's, 7 the policy's,
    // 5 the default — so the fallback has somewhere wrong to land.
    let mut seats = rig.seat_list();
    seats["controller"] = serde_json::json!({ "poll_seconds": 0 });
    write(
        &rig.machine.join("config.json"),
        &format!("{}\n", serde_json::to_string_pretty(&seats).unwrap()),
    );
    assert_eq!(code(&rig.run(&["observe", "--once"])), 0);
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("projection.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        published["fleet"]["poll_seconds"], 7,
        "a zero override falls to the policy's figure, never to the default"
    );

    // And the control for the override itself: with the object gone, the
    // policy's own figure is published again.
    let mut seats = rig.seat_list();
    seats.as_object_mut().unwrap().remove("controller");
    write(
        &rig.machine.join("config.json"),
        &format!("{}\n", serde_json::to_string_pretty(&seats).unwrap()),
    );
    assert_eq!(code(&rig.run(&["observe", "--once"])), 0);
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("projection.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(published["fleet"]["poll_seconds"], 7);
}

/// `fleet stop`: the query first, then the unload, then the event read back.
#[test]
fn stop_refuses_when_nothing_is_loaded_and_otherwise_reads_the_stopped_event_back() {
    let rig = Rig::new("stop");
    rig.created();

    let out = rig.run(&["stop"]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("nothing to stop"), "{}", stderr(&out));
    assert!(rig.of_class("unload").is_empty(), "{:?}", rig.calls());

    assert_eq!(code(&rig.run(&["start"])), 0);
    let out = rig.run(&["stop"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(rig.of_class("unload").len(), 1, "{:?}", rig.calls());
    assert!(
        stderr(&out).contains("controller.stopped"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("untouched"),
        "seats are not touched, and it says so: {}",
        stderr(&out)
    );
    assert!(
        rig.stream()
            .iter()
            .any(|e| e["type"] == "controller.stopped"),
        "the event the confirmation read"
    );
}

/// THE SPEC'S OWN FIRST-RUN SEQUENCE, end to end: add a seat to the policy
/// file, `fleet start`, and only then make the worktree. The render does not
/// create the directory (§5), so between the second act and the third the
/// fleet's one configured worktree does not exist — and the grant must read ok
/// through it, or every effect is off on the sequence the spec prescribes.
#[test]
fn the_first_run_sequence_leaves_the_grant_ok_with_no_worktree_made_yet() {
    let rig = Rig::new("sequence");
    // NO MODEL on the row, so it takes the fleet's default — which is one the
    // posture gate was measured on. A row naming an unmeasured model is dropped
    // from the seat list before the loop reads it (the claude-code pack's
    // lessons D3), and a dropped row has no worktree to probe, which would
    // leave this arm measuring nothing.
    rig.created()
        .policy_says(&format!("\n[seats.{SEAT_A}]\nkind = \"agent\"\n"));

    assert_eq!(code(&rig.run(&["start"])), 0);
    let worktree = rig.root.join("a-project-worktrees/agent-93b9739a");
    assert!(
        !worktree.exists(),
        "the render does not create the worktree: {}",
        worktree.display()
    );
    assert_eq!(
        rig.seat_list()["children"][0]["worktrees"]["a-project"],
        worktree.display().to_string()
    );

    let out = rig.run(&["observe", "--once"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("projection.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        published["grant"], "ok",
        "a worktree nobody has made yet is not a grant question: {published}"
    );
    assert!(published.get("grant_detail").is_none(), "{published}");
    assert_eq!(
        published["effects"]["state"], "on",
        "so the fleet is not held: {published}"
    );

    // The control: with the worktree made, the same poll still reads ok — so
    // the reading above is the gate's answer for both states and not a gate
    // that never asks anything.
    std::fs::create_dir_all(&worktree).unwrap();
    assert_eq!(code(&rig.run(&["observe", "--once"])), 0);
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("projection.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(published["grant"], "ok");
    assert_eq!(published["effects"]["state"], "on");
}

/// `fleet stop` resolves NO FLEET: it queries the manager first, from wherever
/// it was run. A stop that resolved a project first would refuse from a bare
/// directory while the service was loaded, leaving a controller running and
/// `controller.stopped` never read.
#[test]
fn stop_queries_the_manager_from_a_directory_that_resolves_to_no_project() {
    let rig = Rig::new("stop-bare");
    rig.created();
    assert_eq!(code(&rig.run(&["start"])), 0);

    // A directory with no policy file above it, and the fleet's own policy file
    // GONE — somebody moved or deleted the project while its controller was
    // still loaded. Every resolution of a fleet fails here, and the stop must
    // still unload the service that is running.
    let bare = rig.root.join("elsewhere");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::remove_file(rig.project.join("fleet.toml")).unwrap();
    let out = rig
        .command(&["stop"])
        .current_dir(&bare)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(rig.of_class("unload").len(), 1, "{:?}", rig.calls());
    assert!(
        stderr(&out).contains("controller.stopped"),
        "{}",
        stderr(&out)
    );

    // The control: from that same bare directory, with nothing loaded, the
    // refusal is the RUNNING QUERY's and not the resolver's.
    let out = rig
        .command(&["stop"])
        .current_dir(&bare)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("nothing to stop"), "{}", stderr(&out));
    assert!(
        !stderr(&out).contains("fleet create"),
        "the refusal is the query's, not the resolver's: {}",
        stderr(&out)
    );
}

/// `fleet start`'s own refusal is a CONJUNCTION: no policy file above the cwd
/// AND no seat list naming one. A directory that resolves to no project is not
/// by itself a fleetless one — that is the standalone shape.
#[test]
fn start_runs_outside_every_project_when_the_seat_list_names_a_fleet() {
    let rig = Rig::new("start-bare");
    rig.created();
    assert_eq!(code(&rig.run(&["start"])), 0);
    assert_eq!(code(&rig.run(&["stop"])), 0);

    let bare = rig.root.join("elsewhere");
    std::fs::create_dir_all(&bare).unwrap();
    let out = rig
        .command(&["start"])
        .current_dir(&bare)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(
        stderr(&out).contains("no project resolves from this directory"),
        "the render says why it rendered nothing: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("controller.started"),
        "{}",
        stderr(&out)
    );

    // The control: with the seat list taken away, the SAME directory refuses —
    // so the run above is the conjunction's second half and not a verb that
    // never refuses.
    assert_eq!(code(&rig.run(&["stop"])), 0);
    std::fs::remove_file(rig.machine.join("config.json")).unwrap();
    let out = rig
        .command(&["start"])
        .current_dir(&bare)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("fleet create"), "{}", stderr(&out));
}

/// An embedded fleet INSIDE a registered project renders its seat rows under
/// THAT project and not under the fleet's own directory.
///
/// The shape: a fleet's own directory, embedded one level inside a checkout the
/// machine has registered. The control is the same start with the register taken
/// away, which keys on the fleet directory's basename — so what moves the key is
/// the register and not something incidental to the fixture.
#[test]
fn an_embedded_fleet_inside_a_registered_project_keys_its_rows_on_that_project() {
    let rig = Rig::new("embedded-inside");
    // The containing project: declared, with a worktrees directory that is
    // neither directory's derived sibling, so a row keyed on either one is
    // visible as a different path.
    let worktrees = rig.root.join("somewhere-else");
    write(
        &rig.project.join(".fleet/project.toml"),
        &standalone_text("outer", Some("ou"), &rig.project, &worktrees),
    );
    let inside = rig.project.join("fleet");
    std::fs::create_dir_all(&inside).expect("the fleet's own directory is made");
    let out = rig
        .command(&rig.embedded())
        .current_dir(&inside)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    write(
        &rig.machine.join("projects.toml"),
        &format!(
            "[[project]]\nroot = \"{}\"\nname = \"outer\"\n",
            rig.project.display()
        ),
    );
    let policy = inside.join("fleet.toml");
    let body = std::fs::read_to_string(&policy).expect("the policy is there");
    write(
        &policy,
        &format!("{body}\n[seats.{SEAT_A}]\nkind = \"agent\"\n"),
    );

    let out = rig
        .command(&["start"])
        .current_dir(&inside)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let after = rig.seat_list();
    let rows = after["children"].as_array().expect("children is an array");
    assert_eq!(rows.len(), 1, "one seat, one row: {after}");
    let map = rows[0]["worktrees"]
        .as_object()
        .expect("the row carries a worktrees map");
    assert_eq!(map.len(), 1, "one project key: {after}");
    assert_eq!(
        map.get("outer").map(ToString::to_string),
        Some(format!(
            "\"{}\"",
            worktrees.join("agent-93b9739a").display()
        )),
        "the row is keyed on the containing project: {after}"
    );
    assert!(
        !map.contains_key("fleet"),
        "and not on the fleet's own directory: {after}"
    );
    assert!(
        stderr(&out).contains(&format!("rows keyed on outer at {}", worktrees.display())),
        "the first run says which project the rows were keyed on: {}",
        stderr(&out)
    );

    // The control: the same directory, the same policy, the register taken
    // away — and the row keys on the fleet directory's own basename.
    assert_eq!(
        code(
            &rig.command(&["stop"])
                .current_dir(&inside)
                .output()
                .unwrap()
        ),
        0
    );
    std::fs::remove_file(rig.machine.join("projects.toml")).expect("the register is taken away");
    std::fs::remove_file(rig.machine.join("config.json")).expect("the seat list is taken away");
    let out = rig
        .command(&["start"])
        .current_dir(&inside)
        .output()
        .expect("the built binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let after = rig.seat_list();
    let map = after["children"][0]["worktrees"]
        .as_object()
        .expect("the row carries a worktrees map");
    assert_eq!(map.len(), 1, "one project key: {after}");
    assert_eq!(
        map.get("fleet").map(ToString::to_string),
        Some(format!(
            "\"{}\"",
            rig.project.join("fleet-worktrees/agent-93b9739a").display()
        )),
        "with no register above it, the fleet's own directory is the project: {after}"
    );
}

// ---- the lessons ------------------------------------------------------------

mod lessons {
    use super::*;

    /// gas-city G1 — an install writes the service and STARTS NOTHING; load is a
    /// second deliberate act.
    ///
    /// The first-run work is run on its own through `--foreground`, which loads
    /// nothing by contract, and the manager's record is read for a load call.
    /// Then the ordinary start is run, and the record carries exactly one.
    #[test]
    fn install_does_not_start_anything() {
        let rig = Rig::new("g1");
        rig.created();

        let mut child = rig
            .command(&["start", "--foreground"])
            .spawn()
            .expect("the built binary runs");
        until("the first-run work", Duration::from_secs(30), || {
            rig.machine.join("projection.json").is_file()
        });
        let out = Command::new(kill_bin())
            .args(["-TERM", &child.id().to_string()])
            .output()
            .expect("kill runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        child.wait().expect("the child is reaped");

        // The service file is on disk and names this binary and the loop.
        let service = rig.service_file();
        let text = std::fs::read_to_string(&service).unwrap();
        assert!(text.contains("observe"), "{text}");
        assert!(text.contains(env!("CARGO_BIN_EXE_fleet")), "{text}");

        // And NOTHING WAS LOADED: the only calls the manager saw are the query
        // the refusal path makes.
        assert!(
            rig.of_class("load").is_empty(),
            "the first run loaded something: {:?}",
            rig.calls()
        );
        assert!(!rig.calls().is_empty(), "the stub was reached at all");

        // The second deliberate act: exactly one load, and the controller's own
        // first line read back off the stream.
        let out = rig.run(&["start"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        assert_eq!(rig.of_class("load").len(), 1, "{:?}", rig.calls());
        assert!(
            stderr(&out).contains("controller.started"),
            "{}",
            stderr(&out)
        );
    }

    /// gas-city G2 — telemetry is off, and asked: the key is written false and
    /// the disclosure names it, and nothing ever writes it true.
    #[test]
    fn telemetry_is_off_and_asked() {
        let rig = Rig::new("g2");
        let out = rig.run(&rig.embedded());
        assert_eq!(code(&out), 0, "{}", stderr(&out));

        let policy = rig.project.join("fleet.toml");
        let body = std::fs::read_to_string(&policy).unwrap();
        assert!(body.contains("[telemetry]\nenabled = false"), "{body}");
        assert!(
            stderr(&out).contains("nothing leaves this machine"),
            "{}",
            stderr(&out)
        );

        let out = rig.run(&["start"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        assert!(
            stderr(&out).contains("[telemetry] enabled"),
            "the first start names the key: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains("nothing leaves this machine"),
            "{}",
            stderr(&out)
        );
        assert_eq!(
            std::fs::read_to_string(&policy).unwrap(),
            body,
            "the start rewrote a key that was already there"
        );
        assert!(
            !body.contains("[telemetry]\nenabled = true"),
            "the key is never written true: {body}"
        );

        // The other half: a policy file that says nothing about telemetry has
        // the key WRITTEN false by the first start, so the answer is on the
        // record and not merely in a sentence.
        let bare = Rig::new("g2-bare");
        write(
            &bare.project.join("fleet.toml"),
            "[controller]\npoll_seconds = 5\n",
        );
        // Written by hand, so no pack carries its agent: the agent stub, by
        // path.
        common::stub_agent(&bare.project);
        let out = bare.run(&["start"]);
        assert_eq!(code(&out), 0, "{}", stderr(&out));
        let body = std::fs::read_to_string(bare.project.join("fleet.toml")).unwrap();
        assert!(body.contains("[telemetry]\nenabled = false"), "{body}");
        assert!(!body.contains("[telemetry]\nenabled = true"), "{body}");
        assert!(
            body.contains("poll_seconds = 5"),
            "the file it was added to: {body}"
        );
    }
}
