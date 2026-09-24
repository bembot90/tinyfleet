//! `fleet seat add` through the shipped binary.
//!
//! One rig per arm: a scratch embedded fleet made by `fleet create`, its own
//! HOME, and its own machine directory named by `FLEET_DIR` — so the
//! `identity.toml` an arm mints is the rig's and never this box's. Every
//! effect is read from the files the child left behind, and the fleet.toml is
//! compared byte for byte, because the verb's whole promise about the file is
//! that it appends and rewrites nothing.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_core::seat::identity::{self, Kind, SeatId, SeatRef};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A seat id written out by hand, for the arms that list a seat before the
/// verb runs.
const SEAT_A: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    home: PathBuf,
    manager: PathBuf,
    agent: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-seat-add-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the fixture root is made");
        // CANONICAL, because the binary prints paths it resolved and this
        // platform's temp directory is reached through a symlink.
        let root = root.canonicalize().expect("the fixture root resolves");
        let rig = Rig {
            project: root.join("a-project"),
            machine: root.join("machine"),
            home: root.join("home"),
            manager: root.join("manager.sh"),
            agent: root.join("agent.sh"),
            root,
        };
        for dir in [&rig.project, &rig.home] {
            std::fs::create_dir_all(dir).expect("the fixture directory is made");
        }
        rig.a_manager();
        write(&rig.agent, "#!/bin/sh\nexit 0\n");
        executable(&rig.agent);

        let out = rig.run(&["create", "--embedded", "--agent", "claude_code"]);
        assert_eq!(out.status.code(), Some(0), "create: {}", stderr(&out));
        rig.as_before_its_creator_was_listed();
        rig
    }

    /// The platform's service manager, stubbed the way `lifecycle.rs` stubs
    /// it: a query answers not-running until a load, and a load writes the
    /// `controller.started` a start confirms by.
    fn a_manager(&self) {
        let pid = self.root.join("manager-pid");
        let script = format!(
            "#!/bin/sh\n\
             stream={machine}/events.jsonl\n\
             verb=$1\n\
             if [ \"$verb\" = \"--user\" ]; then verb=$2; fi\n\
             case \"$verb\" in\n\
             \x20 print|show)\n\
             \x20   if [ -s {pid} ]; then\n\
             \x20     printf '\\tstate = running\\n\\tpid = %s\\n' \"$(cat {pid})\"\n\
             \x20     printf 'MainPID=%s\\n' \"$(cat {pid})\"\n\
             \x20     exit 0\n\
             \x20   fi\n\
             \x20   exit 1 ;;\n\
             \x20 bootout|stop)\n\
             \x20   : > {pid}\n\
             \x20   exit 0 ;;\n\
             \x20 *)\n\
             \x20   echo 4242 > {pid}\n\
             \x20   mkdir -p {machine}\n\
             \x20   if [ -f \"$stream\" ]; then n=$(awk 'END{{print NR+1}}' \"$stream\"); else n=1; fi\n\
             \x20   printf '{{\"id\":\"stub-%s\",\"seq\":%s,\"ts\":\"2026-09-12T00:00:00Z\",\
             \"type\":\"controller.started\",\"actor\":\"controller\",\"payload\":{{}}}}\\n' \
             \"$n\" \"$n\" >> \"$stream\"\n\
             \x20   exit 0 ;;\n\
             esac\n",
            machine = self.machine.display(),
            pid = pid.display(),
        );
        write(&self.manager, &script);
        executable(&self.manager);
        write(&pid, "");
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .current_dir(&self.project)
            .hermetic(&self.home, &self.machine, Some(&self.agent))
            .env("FLEET_SERVICE_BIN", &self.manager)
            .output()
            .expect("the built binary runs")
    }

    fn add(&self, args: &[&str]) -> Output {
        let mut line = vec!["seat", "add"];
        line.extend_from_slice(args);
        self.run(&line)
    }

    fn policy_file(&self) -> PathBuf {
        self.project.join("fleet.toml")
    }

    fn policy(&self) -> String {
        std::fs::read_to_string(self.policy_file()).expect("the policy is there")
    }

    /// The policy file with one more line in it.
    fn policy_says(&self, text: &str) {
        let body = self.policy();
        write(&self.policy_file(), &format!("{body}{text}"));
    }

    fn identity_file(&self) -> PathBuf {
        self.machine.join(identity::IDENTITY)
    }

    /// The fleet as `create` wrote it and the machine as it was before:
    /// `create` lists whoever ran it through this verb's own writer, minting
    /// the identity, and the arms here are about THIS verb doing both. The
    /// creator's table is cut off the end — exactly the text the writer
    /// appends, or the rig refuses — and identity.toml is taken away.
    fn as_before_its_creator_was_listed(&self) {
        let mine = identity::read_identity(&self.machine)
            .expect("the identity reads")
            .expect("create minted one");
        let body = self.policy();
        let table = identity::seat_table(&mine.as_ref(), None);
        let before = body
            .strip_suffix(&table)
            .unwrap_or_else(|| panic!("create's fleet.toml does not end in {table:?}: {body}"));
        write(&self.policy_file(), before);
        std::fs::remove_file(self.identity_file()).expect("the minted identity is removed");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("the stub is executable");
}

/// The one id stdout carries, and nothing else beside it.
fn the_id(out: &Output) -> SeatId {
    let printed = stdout(out);
    let line = printed
        .strip_suffix('\n')
        .unwrap_or_else(|| panic!("stdout ends in one newline: {printed:?}"));
    assert!(!line.contains('\n'), "stdout is one line: {printed:?}");
    assert_eq!(line.len(), 36, "stdout is one full id: {printed:?}");
    SeatId::parse(line).expect("stdout is a seat id")
}

/// An agent seat is appended as seat_table's text and nothing else moves, and
/// the start after it renders the seat under its machine name.
#[test]
fn an_agent_seat_is_appended_and_start_renders_it() {
    let rig = Rig::new("agent");
    // A comment and a key no reader knows, both the person's: the append
    // keeps them because it never re-serializes the file.
    rig.policy_says("# the person's own note\n[not_a_table_fleet_knows]\nkey = 1\n");
    let before = rig.policy();

    let out = rig.add(&["--agent", "--name", "Orla", "--model", "m"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let id = the_id(&out);

    let seat = SeatRef {
        id,
        name: Some("Orla".to_string()),
        kind: Kind::Agent,
    };
    let table = identity::seat_table(&seat, Some("m"));
    let after = rig.policy();
    assert!(
        after.ends_with(&table),
        "the file ends with the table: {after}"
    );
    assert_eq!(
        &after[..after.len() - table.len()],
        before,
        "every byte before the table is unchanged"
    );

    let short = id.short();
    let said = stderr(&out);
    assert!(
        said.contains(&format!(
            "added: agent orla-{short} — [seats.{id}] in {}",
            rig.policy_file().display()
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "next: git worktree add {}/orla-{short} <a branch>, then fleet start renders it",
            rig.root.join("a-project-worktrees").display()
        )),
        "{said}"
    );

    let out = rig.run(&["start"]);
    assert_eq!(out.status.code(), Some(0), "start: {}", stderr(&out));
    let list: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(rig.machine.join("config.json")).expect("the seat list"),
    )
    .expect("the seat list parses");
    let rows = list["children"].as_array().expect("children is an array");
    let row = rows
        .iter()
        .find(|row| row["name"] == format!("orla-{short}"))
        .unwrap_or_else(|| panic!("start renders orla-{short}: {list}"));
    assert_eq!(row["id"], id.to_string());
    assert_eq!(row["model"], "m");
}

/// A person's seat is this machine's identity, minted where there is none,
/// listed once and never twice.
#[test]
fn a_human_seat_mints_the_identity_once_and_is_listed_once() {
    let rig = Rig::new("human");
    assert!(!rig.identity_file().exists(), "the rig starts with none");
    let before = rig.policy();

    let out = rig.add(&["--human"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let id = the_id(&out);

    assert_eq!(
        std::fs::read_to_string(rig.identity_file()).expect("the identity was minted"),
        format!(
            "# This machine's identity: who acts when a fleet verb runs here with no\n\
             # --by and no FLEET_ACTOR. The id is the key; add name = \"...\" if you\n\
             # want one. Written by fleet.\n\
             id = \"{id}\"\n\
             kind = \"human\"\n"
        ),
        "the file is identity.toml's own text, carrying the printed id"
    );
    let seat = SeatRef {
        id,
        name: None,
        kind: Kind::Human,
    };
    assert_eq!(
        rig.policy(),
        format!("{before}{}", identity::seat_table(&seat, None))
    );
    assert!(
        stderr(&out).contains(&format!(
            "identity: minted at {}",
            rig.identity_file().display()
        )),
        "{}",
        stderr(&out)
    );

    // A second listing of the same person is refused, and nothing moves.
    let listed = rig.policy();
    let out = rig.add(&["--human"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!(
            "fleet seat add: this machine's identity human-{} ({id}) is already [seats.{id}] in {}",
            id.short(),
            rig.policy_file().display()
        )),
        "{}",
        stderr(&out)
    );
    assert_eq!(rig.policy(), listed, "the file is unchanged");
    assert!(
        stdout(&out).is_empty(),
        "no id on a refusal: {}",
        stdout(&out)
    );
}

/// `--name` names the row and never the identity: the verb does not edit
/// identity.toml.
#[test]
fn a_human_seats_name_is_the_rows_and_not_the_identitys() {
    let rig = Rig::new("human-named");
    let out = rig.add(&["--human", "--name", "Orla"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let id = the_id(&out);

    let seats = identity::roster_in(&rig.policy()).expect("the roster reads");
    let row = seats
        .iter()
        .find(|s| s.seat.id == id)
        .expect("the row is listed");
    assert_eq!(row.seat.name.as_deref(), Some("Orla"));
    assert_eq!(row.seat.kind, Kind::Human);

    let mine = identity::read_identity(&rig.machine)
        .expect("the identity reads")
        .expect("the identity is there");
    assert_eq!(mine.id, id);
    assert_eq!(mine.name, None, "identity.toml carries no name");
}

/// The usage refusals: two names that read as ids, an empty one, a model on a
/// person, and neither kind. Each exits 2 and writes nothing.
#[test]
fn a_name_that_reads_as_an_id_and_a_malformed_call_are_usage_errors() {
    let rig = Rig::new("usage");
    let before = rig.policy();

    for (args, said) in [
        (
            vec!["--agent", "--name", "01a0d1f1"],
            "fleet seat add: --name 01a0d1f1 reads as a seat id — pick a name that is not 8 or \
             more hex digits",
        ),
        (
            vec!["--agent", "--name", "kite-5c2e9f31"],
            "fleet seat add: --name kite-5c2e9f31 ends the way a seat's machine name does \
             (-<8 hex>) — pick another",
        ),
        (
            vec!["--agent", "--name", "  "],
            "fleet seat add: --name is empty",
        ),
    ] {
        let out = rig.add(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(said), "{args:?}: {}", stderr(&out));
        assert_eq!(rig.policy(), before, "{args:?} left the file untouched");
    }

    // clap's own usage errors: a model is an agent's, and a seat has a kind.
    for args in [vec!["--model", "m", "--human"], vec!["--name", "Orla"]] {
        let out = rig.add(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("Usage: fleet seat add"),
            "{args:?} is clap's usage error: {}",
            stderr(&out)
        );
        assert_eq!(rig.policy(), before, "{args:?} left the file untouched");
    }
    assert!(
        !rig.identity_file().exists(),
        "no refusal minted an identity"
    );
}

/// Two seats cannot answer to one name, in any case.
#[test]
fn a_name_another_seat_carries_is_refused_naming_the_holder() {
    let rig = Rig::new("duplicate");
    rig.policy_says(&format!(
        "\n[seats.{SEAT_A}]\nkind = \"agent\"\nname = \"Orla\"\n"
    ));
    let before = rig.policy();

    let out = rig.add(&["--agent", "--name", "orla"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!(
            "fleet seat add: orla already names orla-93b9739a ({SEAT_A}) — two seats cannot \
             answer to one name"
        )),
        "{}",
        stderr(&out)
    );
    assert_eq!(rig.policy(), before, "the file is unchanged");
}

/// A policy the roster refuses is refused with the roster's own words, before
/// anything is written.
#[test]
fn a_seat_table_keyed_by_a_name_is_refused_and_nothing_is_written() {
    let rig = Rig::new("by-name");
    rig.policy_says("\n[seats.alpha]\nkind = \"agent\"\n");
    let before = rig.policy();

    let out = rig.add(&["--agent", "--name", "Kite"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&format!(
            "fleet seat add: {}: [seats.alpha] is keyed by a name — a seat is keyed by its id \
             now; fleet seat add --agent --name alpha mints one",
            rig.policy_file().display()
        )),
        "{}",
        stderr(&out)
    );
    assert_eq!(rig.policy(), before, "nothing is written");
}

/// Under `--json` stdout is one envelope, and a refusal is one too.
#[test]
fn the_json_flag_prints_one_envelope_for_an_add_and_for_a_refusal() {
    let rig = Rig::new("json");
    let out = rig.add(&["--agent", "--name", "Orla", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let printed = stdout(&out);
    assert_eq!(printed.lines().count(), 1, "one document: {printed}");
    let document: serde_json::Value = serde_json::from_str(&printed).expect("the document parses");
    assert_eq!(document["ok"], true);
    assert_eq!(document["verb"], "seat add");
    let data = &document["data"];
    let id = SeatId::parse(data["seat"]["id"].as_str().expect("the id is a string"))
        .expect("the id is a seat id");
    assert_eq!(data["seat"]["name"], "Orla");
    assert_eq!(data["seat"]["kind"], "agent");
    assert_eq!(data["file"], rig.policy_file().display().to_string());
    assert_eq!(data["minted_identity"], false);
    assert!(rig.policy().contains(&format!("[seats.{id}]")));

    // A person with no name: the key is left out, and the mint is said.
    let out = rig.add(&["--human", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("the document parses");
    assert_eq!(document["data"]["seat"]["kind"], "human");
    assert!(
        document["data"]["seat"].get("name").is_none(),
        "no name key on a nameless seat: {document}"
    );
    assert_eq!(document["data"]["minted_identity"], true);

    let out = rig.add(&["--agent", "--name", "orla", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let document: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("the refusal parses");
    assert_eq!(document["ok"], false);
    assert_eq!(document["verb"], "seat add");
    assert_eq!(document["refusal"]["code"], "refused");
}
