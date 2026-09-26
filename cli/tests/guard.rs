//! `fleet guard` through the shipped binary: the payload on stdin, the policy
//! resolved off the filesystem, and the two exits `--check` has.
//!
//! THE JUDGING PATH HAS ONE EXIT AND IT IS 0. A non-zero status from a pre-tool
//! hook is read as non-blocking, so a guard that signalled a refusal by status
//! would fail open exactly when it broke; every case below reads the status as
//! well as the output for that reason.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch project: a directory the payload's `cwd` points at, holding
/// whatever policy the case is about. The walk up from it reaches only the
/// temp directory's own parents, so no file outside the case decides anything.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-guard-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch project is created");
        Scratch { root }
    }

    fn write(&self, relative: &str, contents: &str) -> &Scratch {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the parent is created");
        }
        std::fs::write(path, contents).expect("the file is written");
        self
    }

    fn path(&self) -> &str {
        self.root.to_str().expect("the temp path is utf-8")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn payload(command: &str, cwd: &str) -> String {
    let body = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "cwd": cwd,
    });
    body.to_string()
}

/// One judging run: the payload on stdin, the class as the argument.
fn judge(class: &str, body: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["guard", class])
        .hermetic_nowhere()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built binary runs");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(body.as_bytes())
        .expect("the payload is written");
    child.wait_with_output().expect("the binary exits")
}

fn refused(class: &str, command: &str, scratch: &Scratch) -> Option<String> {
    let out = judge(class, &payload(command, scratch.path()));
    assert!(
        out.status.success(),
        "the judging path exits 0 on every route, and this one exited {:?}",
        out.status.code()
    );
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

const TRAP: &str = "git show \"$S:tools/land\"";
/// `rest` after the command word a guard polices where the project's store
/// declares none it could read: the default store adapter's name, which is
/// also the command the bd pack's adapter declares.
fn word(rest: &str) -> &'static str {
    Box::leak(format!("{} {rest}", fleet_core::store::DEFAULT_ADAPTER).into_boxed_str())
}

fn replace() -> &'static str {
    word("update x-1 --notes n")
}

fn sql() -> &'static str {
    word("sql \"UPDATE issues SET a = 1\"")
}

fn bare() -> &'static str {
    word("note x-1 \"see a1b2\"")
}

/// The refusal as the hook contract carries it, checked as a document rather
/// than as a substring: a decision the agent cannot parse refuses nothing.
fn decision_of(text: &str) -> serde_json::Value {
    let parsed: serde_json::Value =
        serde_json::from_str(text.trim()).expect("a refusal is one JSON object");
    parsed["hookSpecificOutput"].clone()
}

#[test]
fn a_refusal_is_one_json_object_on_stdout_and_the_exit_is_zero() {
    let scratch = Scratch::new("shape");
    scratch.write("fleet.toml", "[project]\nitem_prefix = \"acme\"\n");

    let text = refused("shell-trap", TRAP, &scratch).expect("the trap is refused");
    let decision = decision_of(&text);
    assert_eq!(decision["hookEventName"], "PreToolUse");
    assert_eq!(decision["permissionDecision"], "deny");
    let reason = decision["permissionDecisionReason"]
        .as_str()
        .expect("the reason is a string");
    assert!(
        reason.contains("MODIFIER"),
        "the reason names the class — {reason}"
    );
    assert!(
        reason.contains("FLEET_TRAP_OK=1"),
        "the reason names the escape — {reason}"
    );
    assert!(
        reason.contains("brace the name"),
        "the reason prints the rewrite — {reason}"
    );
}

#[test]
fn a_guard_switched_off_in_the_policy_refuses_nothing_and_its_sibling_still_does() {
    let scratch = Scratch::new("optout");
    scratch.write(
        "fleet.toml",
        "[project]\nitem_prefix = \"acme\"\n\n[guards]\nshell-trap = { enabled = false }\nrecord = { enabled = true }\n",
    );
    assert!(
        refused("shell-trap", TRAP, &scratch).is_none(),
        "the class switched off refuses nothing"
    );
    assert!(
        refused("record", replace(), &scratch).is_some(),
        "its sibling, left on, still refuses"
    );

    scratch.write(
        "fleet.toml",
        "[project]\nitem_prefix = \"acme\"\n\n[guards]\nshell-trap = { enabled = true }\nrecord = { enabled = false }\n",
    );
    assert!(
        refused("shell-trap", TRAP, &scratch).is_some(),
        "and the reverse"
    );
    assert!(
        refused("record", replace(), &scratch).is_none(),
        "and the reverse"
    );

    // Opt-out, never opt-in: with the table absent, the two classes this arm
    // names both run.
    scratch.write("fleet.toml", "[project]\nitem_prefix = \"acme\"\n");
    assert!(
        refused("shell-trap", TRAP, &scratch).is_some(),
        "no table, still on"
    );
    assert!(
        refused("record", replace(), &scratch).is_some(),
        "no table, still on"
    );
}

#[test]
fn each_escape_licenses_its_own_act_through_the_binary_and_no_other() {
    let scratch = Scratch::new("escapes");
    scratch.write("fleet.toml", "[project]\nitem_prefix = \"acme\"\n");

    let acts = [
        ("shell-trap", TRAP, "FLEET_TRAP_OK"),
        ("record", replace(), "FLEET_NOTES_REPLACE_OK"),
        ("record", sql(), "FLEET_SQL_WRITE_OK"),
        ("record", bare(), "FLEET_BARE_ID_OK"),
    ];
    let escapes = [
        "FLEET_TRAP_OK",
        "FLEET_NOTES_REPLACE_OK",
        "FLEET_SQL_WRITE_OK",
        "FLEET_BARE_ID_OK",
    ];
    for escape in escapes {
        for (class, command, owner) in acts {
            let prefixed = format!("{escape}=1 {command}");
            let licensed = refused(class, &prefixed, &scratch).is_none();
            assert_eq!(
                licensed,
                escape == owner,
                "{escape} against `{command}`: licensed={licensed}, and only {owner} licenses it"
            );
        }
    }
}

#[test]
fn the_bare_id_check_is_silent_without_its_target_and_loud_with_it() {
    let scratch = Scratch::new("prefix");
    scratch.write("fleet.toml", "[guards]\nrecord = { enabled = true }\n");
    assert!(
        refused("record", bare(), &scratch).is_none(),
        "no item prefix, nothing refused"
    );

    scratch.write(
        "fleet.toml",
        "[guards]\nrecord = { enabled = true }\n\n[project]\nitem_prefix = \"acme\"\n",
    );
    let text =
        refused("record", bare(), &scratch).expect("with the target set, the suffix is named");
    let reason = decision_of(&text)["permissionDecisionReason"]
        .as_str()
        .expect("the reason is a string")
        .to_string();
    assert!(
        reason.contains("a1b2 -> acme-a1b2"),
        "the rewrite is the full id — {reason}"
    );
}

/// STANDALONE MODE: no fleet.toml above the project, so the policy is the one
/// the machine directory names and the bare-id check's target comes from the
/// project's own file instead. Both legs are exercised here, because every
/// other case in this file takes the embedded route.
#[test]
fn a_project_with_no_fleet_toml_above_it_reads_the_machine_directory_and_its_own_project_file() {
    let machine = Scratch::new("machine");
    let project = Scratch::new("standalone");
    let policy = machine.root.join("fleet.toml");
    std::fs::write(&policy, "[guards]\nshell-trap = { enabled = false }\n")
        .expect("the machine's policy is written");
    machine.write(
        "config.json",
        &serde_json::json!({ "fleet_toml": policy.to_str().expect("utf-8") }).to_string(),
    );

    let judge_with_machine = |class: &str, command: &str| -> Option<String> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["guard", class])
            .hermetic(&machine.root.join("home"), &machine.root, None)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the built binary runs");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(payload(command, project.path()).as_bytes())
            .expect("the payload is written");
        let out = child.wait_with_output().expect("the binary exits");
        assert!(out.status.success(), "the judging path exits 0");
        let text = String::from_utf8(out.stdout).expect("the output is utf-8");
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    };

    assert!(
        judge_with_machine("shell-trap", TRAP).is_none(),
        "the machine's policy switched this class off"
    );
    assert!(
        judge_with_machine("record", replace()).is_some(),
        "the class the machine's policy leaves on still refuses"
    );
    assert!(
        judge_with_machine("record", bare()).is_none(),
        "no project file, so the bare-id check has no target"
    );

    project.write(".fleet/project.toml", "[project]\nitem_prefix = \"acme\"\n");
    assert!(
        judge_with_machine("record", bare()).is_some(),
        "the project's own file is where the target lives in this mode"
    );
}

/// A DECLARED PROJECT WINS AT ITS OWN LEVEL, for the guards as for the item
/// verbs: a directory holding both files is standalone, so the switches are the
/// FLEET's and the bare-id target is the project's.
///
/// The neighbour disagrees on both — it leaves the class this machine switched
/// off ON, and names a different prefix — which is what makes each answer below
/// a reading of one file rather than two files that happen to agree.
#[test]
fn a_declaration_beside_a_fleet_toml_reads_the_machines_switches_and_its_own_target() {
    let machine = Scratch::new("machine-declared");
    let project = Scratch::new("declared-beside");
    let policy = machine.root.join("fleet.toml");
    std::fs::write(&policy, "[guards]\nshell-trap = { enabled = false }\n")
        .expect("the machine's policy is written");
    machine.write(
        "config.json",
        &serde_json::json!({ "fleet_toml": policy.to_str().expect("utf-8") }).to_string(),
    );
    project.write(
        "fleet.toml",
        "[guards]\nshell-trap = { enabled = true }\n\n[project]\nitem_prefix = \"neighbour\"\n",
    );
    project.write(".fleet/project.toml", "[project]\nitem_prefix = \"acme\"\n");

    let judge_with_machine = |class: &str, command: &str| -> Option<String> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(["guard", class])
            .hermetic(&machine.root.join("home"), &machine.root, None)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the built binary runs");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(payload(command, project.path()).as_bytes())
            .expect("the payload is written");
        let out = child.wait_with_output().expect("the binary exits");
        assert!(out.status.success(), "the judging path exits 0");
        let text = String::from_utf8(out.stdout).expect("the output is utf-8");
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    };

    assert!(
        judge_with_machine("shell-trap", TRAP).is_none(),
        "the switch is the machine's, which has this class off — the neighbour leaves it on"
    );
    let text = judge_with_machine("record", bare()).expect("the target is the declaration's");
    let reason = decision_of(&text)["permissionDecisionReason"]
        .as_str()
        .expect("the reason is a string")
        .to_string();
    assert!(
        reason.contains("a1b2 -> acme-a1b2"),
        "the prefix is the declaration's and not the neighbour's — {reason}"
    );
}

#[test]
fn a_payload_with_nothing_to_judge_prints_nothing_and_exits_zero() {
    for (label, body) in [
        ("not json at all", "this is not json".to_string()),
        (
            "another tool",
            serde_json::json!({"tool_name": "Read", "tool_input": {"command": TRAP}}).to_string(),
        ),
        (
            "no tool name",
            serde_json::json!({"tool_input": {"command": TRAP}}).to_string(),
        ),
        (
            "a blank command",
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "   "}}).to_string(),
        ),
        (
            "a command that is not a string",
            serde_json::json!({"tool_name": "Bash", "tool_input": {"command": 17}}).to_string(),
        ),
        ("an empty body", String::new()),
    ] {
        for class in ["shell-trap", "record"] {
            let out = judge(class, &body);
            assert!(
                out.status.success(),
                "{label} on {class}: exited {:?}",
                out.status.code()
            );
            assert!(
                out.stdout.is_empty(),
                "{label} on {class}: printed {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}

// ---- the hook mapping an agent adapter declares ------------------------------

/// A machine directory holding the binary's defaults and, under `packs/`,
/// whatever packs the arm installs by hand: the layers `--adapter <name>`
/// resolves through.
fn a_machine(scratch: &Scratch) -> PathBuf {
    let machine = scratch.root.join("machine");
    let defaults = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&defaults).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&defaults).expect("the embedded defaults are written");
    machine
}

/// The pack `name`, installed on `machine`, carrying the agent adapter `name`
/// whose `adapter.toml` ends in `hook` — a `[hook]` table, or nothing.
fn an_agent_pack(machine: &Path, name: &str, hook: &str) {
    let root = machine.join("packs").join(name);
    let dir = root.join(format!("adapters/agent/{name}"));
    std::fs::create_dir_all(&dir).expect("the adapter's directory is created");
    std::fs::write(
        root.join("pack.toml"),
        format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n"),
    )
    .expect("the manifest is written");
    std::fs::write(
        dir.join("adapter.toml"),
        format!(
            "[adapter]\nname = \"{name}\"\nkind = \"agent\"\nversion = \"0.1.0\"\n\
             entry = \"main.sh\"\n{hook}"
        ),
    )
    .expect("the adapter's manifest is written");
    std::fs::write(dir.join("main.sh"), "#!/bin/sh\nexit 3\n").expect("the entry is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.join("main.sh"), std::fs::Permissions::from_mode(0o755))
        .expect("the entry is executable");
}

/// One judging run under `machine`, from inside `scratch`, the class and
/// whatever flags follow it as the arguments.
fn judge_on(machine: &Path, scratch: &Scratch, args: &[&str], body: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .arg("guard")
        .args(args)
        .hermetic(&scratch.root.join("home"), machine, None)
        .current_dir(&scratch.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built binary runs");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(body.as_bytes())
        .expect("the payload is written");
    child.wait_with_output().expect("the binary exits")
}

/// THE BUILT-IN IS CLAUDE CODE'S MAPPING, BYTE FOR BYTE. The payload and the
/// refusal docs/guards.md prints, read off the page: with no `--adapter`, and
/// with `--adapter claude-code` where no pack carries that name, the binary
/// prints exactly the page's line.
///
/// RED-PROOF: on the base `--adapter` is an unknown flag, a usage error.
#[test]
fn the_guards_page_example_prints_the_same_bytes_with_and_without_the_built_in_adapter() {
    let page =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/guards.md"))
            .expect("the guards page is readable");
    let lines: Vec<&str> = page.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.starts_with("$ echo '") && line.ends_with("| fleet guard shell-trap"))
        .expect("the page hands a payload to fleet guard shell-trap");
    let body = lines[at]
        .strip_prefix("$ echo '")
        .and_then(|rest| rest.strip_suffix("' | fleet guard shell-trap"))
        .expect("the payload sits between the quotes");
    let printed = format!("{}\n", lines[at + 1]);

    let scratch = Scratch::new("page");
    let machine = a_machine(&scratch);
    for args in [
        vec!["shell-trap"],
        vec!["shell-trap", "--adapter", "claude-code"],
    ] {
        let out = judge_on(&machine, &scratch, &args, body);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            printed,
            "{args:?} prints the page's line"
        );
    }
}

/// ANOTHER AGENT'S MAPPING IS DATA IN ITS PACK. An agent adapter whose `[hook]`
/// names its shell tool `shell`, keeps the command at `/input/cmd` and writes
/// a refusal as the bare reason: a trapped command in its payload is refused
/// in its template, and a Claude Code payload is nothing it can judge. The
/// same payload through the built-in is silent the other way round.
///
/// RED-PROOF: on the base `--adapter` is an unknown flag, a usage error.
#[test]
fn a_packs_hook_mapping_reads_its_own_payload_and_writes_its_own_refusal() {
    let scratch = Scratch::new("other");
    scratch.write("fleet.toml", "[project]\nitem_prefix = \"acme\"\n");
    let machine = a_machine(&scratch);
    an_agent_pack(
        &machine,
        "other",
        "\n[hook]\nshell_tool = \"shell\"\ntool = \"/tool\"\ncommand = \"/input/cmd\"\n\
         cwd = \"/dir\"\ndeny = '{reason}'\n",
    );
    let theirs =
        serde_json::json!({"tool": "shell", "input": {"cmd": TRAP}, "dir": scratch.path()})
            .to_string();

    let out = judge_on(
        &machine,
        &scratch,
        &["shell-trap", "--adapter", "other"],
        &theirs,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    let reason: String = serde_json::from_str(text.trim())
        .expect("the refusal is the template filled: the reason as one JSON string");
    assert!(
        reason.starts_with("fleet guard shell-trap: MODIFIER")
            && reason.contains("FLEET_TRAP_OK=1"),
        "the reason is core's — {reason}"
    );
    assert!(
        text.starts_with('"') && text.ends_with("\"\n"),
        "no envelope around it: {text}"
    );

    let ours = payload(TRAP, scratch.path());
    let out = judge_on(
        &machine,
        &scratch,
        &["shell-trap", "--adapter", "other"],
        &ours,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "a Claude Code payload is nothing this mapping judges: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let out = judge_on(&machine, &scratch, &["shell-trap"], &theirs);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "the built-in judges no other agent's payload: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A MIS-WIRED GUARD FAILS CLOSED. An `--adapter` that resolves nowhere, one
/// whose manifest has no `[hook]`, and one that is neither form each exit 2
/// with one line on standard error naming what was missing, and print nothing
/// on standard output — the agent reads a hook's exit 2 as blocking.
#[test]
fn an_adapter_that_resolves_nowhere_or_declares_no_hook_exits_two_with_one_line() {
    let scratch = Scratch::new("miswired");
    scratch.write("fleet.toml", "[project]\nitem_prefix = \"acme\"\n");
    let machine = a_machine(&scratch);
    an_agent_pack(&machine, "bare", "");
    let bare = machine.join("packs/bare/adapters/agent/bare");
    let body = payload(TRAP, scratch.path());
    for (adapter, names) in [
        ("nobody", "`nobody`"),
        ("bare", "[hook]"),
        (bare.to_str().expect("a utf-8 path"), "[hook]"),
        ("some/where", "`some/where`"),
    ] {
        let out = judge_on(
            &machine,
            &scratch,
            &["shell-trap", "--adapter", adapter],
            &body,
        );
        let said = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(out.status.code(), Some(2), "{adapter}: {said}");
        assert!(out.stdout.is_empty(), "{adapter}: nothing on stdout");
        assert_eq!(said.lines().count(), 1, "{adapter}: one line — {said}");
        assert!(
            said.starts_with("fleet guard: ") && said.contains(names),
            "{adapter}: the line names {names} — {said}"
        );
    }
}

// ---- the command the project's store declares ---------------------------------

/// The project's `[store] adapter` as an executable that logs each verb it is
/// asked on `asked` and answers `capabilities` with the shell given.
fn a_store_declaring(scratch: &Scratch, capabilities: &str) {
    let adapter = scratch.root.join("adapter");
    std::fs::write(
        &adapter,
        format!(
            "#!/bin/sh\necho \"$1\" >> '{asked}'\ncase \"$1\" in\n\
             capabilities) {capabilities} ;;\n*) exit 2 ;;\nesac\n",
            asked = scratch.root.join("asked").display()
        ),
    )
    .expect("the adapter is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&adapter, std::fs::Permissions::from_mode(0o755))
        .expect("the adapter is executable");
    scratch.write(
        "fleet.toml",
        &format!(
            "[project]\nitem_prefix = \"acme\"\n\n[store]\nadapter = \"{}\"\n",
            adapter.display()
        ),
    );
}

/// THE STORE'S COMMAND IS THE ONE POLICED. A project whose store declares
/// `tk` has `tk`'s calls refused and the default word's let through; one whose
/// store declares no command has neither refused by the checks that read one;
/// and a store that will not answer leaves the default word policed, never
/// nothing. A text no check reads a store call by is judged without asking the
/// store at all.
///
/// RED-PROOF: with the command fixed at the default word, the `tk` project
/// refuses it and lets `tk` through, and the project declaring none refuses
/// it.
#[test]
fn the_command_the_projects_store_declares_is_the_one_the_guards_police() {
    let tk_notes = "tk update x-1 --notes n";
    let tk_backtick = "tk note acme-x1 \"a `b` c\"";
    let default_backtick = word("note acme-x1 \"a `b` c\"");

    let scratch = Scratch::new("store-cli-tk");
    a_store_declaring(&scratch, r#"echo '{"schema_version":1,"cli":"tk"}'"#);
    let text = refused("record", tk_notes, &scratch).expect("tk's notes flag is refused");
    let reason = decision_of(&text)["permissionDecisionReason"]
        .as_str()
        .expect("the reason is text")
        .to_string();
    assert!(
        reason.contains("`tk note <id> <text>`, or `tk update <id> --append-notes <text>`"),
        "{reason}"
    );
    assert!(refused("shell-trap", tk_backtick, &scratch).is_some());
    assert!(
        refused("record", replace(), &scratch).is_none(),
        "the default word is a command like any other here"
    );
    assert!(refused("shell-trap", default_backtick, &scratch).is_none());

    let scratch = Scratch::new("store-cli-none");
    a_store_declaring(&scratch, r#"echo '{"schema_version":1}'"#);
    for (class, command) in [
        ("record", replace()),
        ("record", sql()),
        ("record", bare()),
        ("shell-trap", default_backtick),
    ] {
        assert!(
            refused(class, command, &scratch).is_none(),
            "{command}: a store declaring no command has no call refused"
        );
    }
    assert!(
        refused("shell-trap", TRAP, &scratch).is_some(),
        "a check that reads no store call still refuses"
    );

    let scratch = Scratch::new("store-cli-unanswered");
    a_store_declaring(&scratch, "echo 'the index is locked' >&2; exit 3");
    assert!(
        refused("record", replace(), &scratch).is_some(),
        "a store that does not answer leaves the default adapter's command policed"
    );
    assert!(refused("shell-trap", default_backtick, &scratch).is_some());

    let scratch = Scratch::new("store-cli-unasked");
    a_store_declaring(&scratch, r#"echo '{"schema_version":1,"cli":"tk"}'"#);
    for class in ["shell-trap", "record"] {
        assert!(refused(class, "cargo build --workspace", &scratch).is_none());
    }
    assert!(
        !scratch.root.join("asked").exists(),
        "a text no check reads a store call by never asks the store"
    );
    assert!(refused("record", tk_notes, &scratch).is_some());
    assert_eq!(
        std::fs::read_to_string(scratch.root.join("asked")).expect("the store was asked"),
        "capabilities\n",
        "a text naming a store subcommand asks for the declaration, and nothing else"
    );
}

// ---- --check, and the doctor entry that runs it -----------------------------

fn check(class: &str, scratch: &Scratch) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["guard", class, "--check"])
        .current_dir(&scratch.root)
        .hermetic(
            &scratch.root.join("home"),
            &scratch.root.join("machine"),
            None,
        )
        .output()
        .expect("the built binary runs")
}

#[test]
fn check_reports_each_check_and_exits_on_whether_every_target_is_configured() {
    let scratch = Scratch::new("check");
    scratch.write("fleet.toml", "[guards]\nrecord = { enabled = true }\n");

    let out = check("record", &scratch);
    assert_eq!(out.status.code(), Some(1), "an unconfigured target exits 1");
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert!(
        text.contains("record bare-id: not configured — [project] item_prefix"),
        "the line names the key — {text}"
    );
    assert!(
        text.contains("record entry-forge: configured"),
        "the check with no target is always configured — {text}"
    );
    assert_eq!(text.lines().count(), 4, "one line per check — {text}");

    scratch.write(
        "fleet.toml",
        "[guards]\nrecord = { enabled = true }\n\n[project]\nitem_prefix = \"acme\"\n",
    );
    let out = check("record", &scratch);
    assert_eq!(
        out.status.code(),
        Some(0),
        "every target configured exits 0"
    );
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert!(text.contains("record bare-id: configured"), "— {text}");

    // The shell-trap class's checks have no target, so its line is the same
    // either way and its exit is 0.
    let out = check("shell-trap", &scratch);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert_eq!(text.lines().count(), 5, "one line per check — {text}");
    assert!(
        !text.contains("not configured"),
        "no target, nothing to configure — {text}"
    );
}

#[test]
fn the_doctor_entry_runs_both_check_lines_and_exits_with_the_first_non_zero() {
    let scratch = Scratch::new("doctor");
    scratch.write("fleet.toml", "[guards]\nrecord = { enabled = true }\n");

    let binary = PathBuf::from(env!("CARGO_BIN_EXE_fleet"));
    let bin_dir = binary.parent().expect("the binary sits in a directory");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let run_sh = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits inside the workspace")
        .join("core/defaults/doctor/guards-installed/run.sh");

    let out = Command::new("sh")
        .arg(&run_sh)
        .current_dir(&scratch.root)
        .env("PATH", &path)
        .hermetic(
            &scratch.root.join("home"),
            &scratch.root.join("machine"),
            None,
        )
        .output()
        .expect("the doctor entry runs");
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert!(
        text.contains("shell-trap record-backtick") && text.contains("record bare-id"),
        "both --check lines ran — {text}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "the record class's unconfigured target is the first non-zero"
    );

    scratch.write(
        "fleet.toml",
        "[guards]\nrecord = { enabled = true }\n\n[project]\nitem_prefix = \"acme\"\n",
    );
    let out = Command::new("sh")
        .arg(&run_sh)
        .current_dir(&scratch.root)
        .env("PATH", &path)
        .hermetic(
            &scratch.root.join("home"),
            &scratch.root.join("machine"),
            None,
        )
        .output()
        .expect("the doctor entry runs");
    assert_eq!(
        out.status.code(),
        Some(0),
        "every target configured, nothing to report"
    );
}

// ---- the two classes a pack wires, through the same binary ------------------

/// A declared project carrying all four target keys, which is the shape the
/// pack's own guards read.
const TARGETS: &str = "[project]\nitem_prefix = \"acme\"\n\n[guards.targets]\n\
                       release_ref_glob = \"refs/heads/*release/*\"\n\
                       prod_buckets = [\"live.example.test\"]\n\
                       prod_projects = [\"example-production\"]\n\
                       prod_apps = [\"example-frontdoor\"]\n\
                       prod_make_goals = [\"ship-it\"]\n\
                       prod_dagger_functions = [\"deployExecute\"]\n\
                       prod_workflow_refs = [\"pipeline.yml:backend/release/*\"]\n";

const PUSH: &str = "git push origin backend/release/1.2";
const BUCKET_WRITE: &str = "gsutil rm gs://live.example.test/x";

#[test]
fn the_targets_ride_on_the_project_file_and_an_absent_key_refuses_nothing() {
    let full = Scratch::new("targets");
    full.write(".fleet/project.toml", TARGETS);
    full.write("fleet-dir/config.json", "{}");

    let text = refused("release-ref", PUSH, &full).expect("the release ref is refused");
    let decision = decision_of(&text);
    assert_eq!(decision["permissionDecision"], "deny");
    let reason = decision["permissionDecisionReason"]
        .as_str()
        .expect("the reason is a string");
    assert!(
        reason.starts_with("fleet guard release-ref:"),
        "the refusal names the class that made it — {reason}"
    );
    assert!(
        reason.contains("refs/heads/backend/release/1.2"),
        "and the destination it resolved — {reason}"
    );
    assert!(
        reason.contains(fleet_core::guard::NO_ESCAPE),
        "and that there is no escape at this layer — {reason}"
    );
    for escape in [
        fleet_core::guard::ESCAPE_TRAP,
        fleet_core::guard::ESCAPE_PROD_WRITE,
    ] {
        assert!(
            !reason.contains(escape),
            "no other class's escape is offered here — {reason}"
        );
    }

    let text = refused("production-write", BUCKET_WRITE, &full).expect("the write is refused");
    let reason = decision_of(&text)["permissionDecisionReason"]
        .as_str()
        .expect("the reason is a string")
        .to_string();
    assert!(
        reason.starts_with("fleet guard production-write:"),
        "the refusal names the class — {reason}"
    );
    assert!(
        reason.contains(&format!("{}=1", fleet_core::guard::ESCAPE_PROD_WRITE)),
        "and the escape, in the spelling a seat would type — {reason}"
    );
    assert!(
        refused(
            "production-write",
            "gsutil ls gs://live.example.test",
            &full
        )
        .is_none(),
        "a read of the same bucket is not refused"
    );

    // The same project with the keys absent: nothing is configured, so nothing
    // is refused, and that is the control that makes the two refusals above a
    // reading of the keys rather than of the command words.
    let empty = Scratch::new("no-targets");
    empty.write(".fleet/project.toml", "[project]\nitem_prefix = \"acme\"\n");
    empty.write("fleet-dir/config.json", "{}");
    assert!(
        refused("release-ref", PUSH, &empty).is_none(),
        "no glob, nothing to match a destination against"
    );
    assert!(
        refused("production-write", BUCKET_WRITE, &empty).is_none(),
        "no list, nothing to match a target against"
    );
}

/// The same command with the quote that never closes, which is what sends the
/// judgment down the raw-text fallback inside the class.
const UNREADABLE_BUCKET_WRITE: &str = "gsutil rm \"gs://live.example.test/x";
const UNREADABLE_UNLISTED: &str = "gsutil rm \"gs://unlisted.example.test/x";

#[test]
fn a_text_the_binary_cannot_lex_falls_back_to_the_lists_the_project_declares() {
    let full = Scratch::new("raw-fallback");
    full.write(".fleet/project.toml", TARGETS);
    full.write("fleet-dir/config.json", "{}");

    let text = refused("production-write", UNREADABLE_BUCKET_WRITE, &full)
        .expect("the unreadable command carrying a declared bucket is refused");
    let decision = decision_of(&text);
    assert_eq!(decision["hookEventName"], "PreToolUse");
    assert_eq!(decision["permissionDecision"], "deny");
    let reason = decision["permissionDecisionReason"]
        .as_str()
        .expect("the reason is a string");
    assert!(
        reason.starts_with("fleet guard production-write:"),
        "the refusal names the class that made it — {reason}"
    );
    assert!(
        reason.contains("could not be read"),
        "and says the conservative match applied — {reason}"
    );
    assert!(
        reason.contains("live.example.test"),
        "and names the declared entry it matched — {reason}"
    );
    assert!(
        reason.contains(&format!("{}=1", fleet_core::guard::ESCAPE_PROD_WRITE)),
        "and the escape, in the spelling a seat would type — {reason}"
    );

    // The control: with the quote closed the same command is refused by the
    // parsed walk, so the refusal above is the reader's failure and not a
    // different command.
    assert!(
        refused("production-write", BUCKET_WRITE, &full).is_some(),
        "the lexable form is refused too"
    );

    // The arm that keeps the fallback from being a blanket refuser, read
    // through the binary: unreadable text naming nothing the project declared.
    assert!(
        refused("production-write", UNREADABLE_UNLISTED, &full).is_none(),
        "unreadable text naming no declared target has no target, and allows"
    );

    // And the same three against a project that declares no lists at all: the
    // fallback reads the declaration, never a spelling.
    let empty = Scratch::new("raw-fallback-empty");
    empty.write(".fleet/project.toml", "[project]\nitem_prefix = \"acme\"\n");
    empty.write("fleet-dir/config.json", "{}");
    assert!(
        refused("production-write", UNREADABLE_BUCKET_WRITE, &empty).is_none(),
        "no list, nothing for the fallback to match either"
    );
}

const MAKE_DEPLOY: &str = "make ship-it";
const DAGGER_DEPLOY: &str = "dagger call deployExecute";
const WORKFLOW_DEPLOY: &str = "gh workflow run pipeline.yml --ref backend/release/2.7";

#[test]
fn the_three_product_surfaces_ride_on_the_project_file_and_an_absent_key_refuses_nothing() {
    let full = Scratch::new("product-targets");
    full.write(".fleet/project.toml", TARGETS);
    full.write("fleet-dir/config.json", "{}");

    for (command, needle) in [
        (MAKE_DEPLOY, "ship-it"),
        (DAGGER_DEPLOY, "deployExecute"),
        (WORKFLOW_DEPLOY, "backend/release/2.7"),
    ] {
        let text = refused("production-write", command, &full)
            .unwrap_or_else(|| panic!("the declared surface is refused — {command}"));
        let decision = decision_of(&text);
        assert_eq!(decision["permissionDecision"], "deny");
        let reason = decision["permissionDecisionReason"]
            .as_str()
            .expect("the reason is a string");
        assert!(
            reason.starts_with("fleet guard production-write:"),
            "the refusal names the class — {reason}"
        );
        assert!(
            reason.contains(needle),
            "and the target it matched — {reason}"
        );
        assert!(
            reason.contains(&format!("{}=1", fleet_core::guard::ESCAPE_PROD_WRITE)),
            "and the escape, in the spelling a seat would type — {reason}"
        );
    }

    // The escape the build tool carries itself, which is not this guard's.
    assert!(
        refused("production-write", "make ship-it dry=1", &full).is_none(),
        "the dry run prints the call and runs nothing"
    );

    // The same project with the three keys absent: nothing declared, nothing
    // refused — the control that makes the three refusals above a reading of
    // the keys rather than of the command words.
    let empty = Scratch::new("no-product-targets");
    empty.write(".fleet/project.toml", "[project]\nitem_prefix = \"acme\"\n");
    empty.write("fleet-dir/config.json", "{}");
    for command in [MAKE_DEPLOY, DAGGER_DEPLOY, WORKFLOW_DEPLOY] {
        assert!(
            refused("production-write", command, &empty).is_none(),
            "no list, nothing to match a target against — {command}"
        );
    }
}

#[test]
fn check_on_each_new_class_names_its_keys_and_exits_on_whether_they_are_configured() {
    let empty = Scratch::new("check-empty");
    empty.write(".fleet/project.toml", "[project]\nitem_prefix = \"acme\"\n");

    let out = check("release-ref", &empty);
    assert_eq!(out.status.code(), Some(1), "an unconfigured target exits 1");
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert!(
        text.contains(
            "release-ref push-target: not configured — [guards.targets] release_ref_glob"
        ),
        "the line names the key — {text}"
    );
    assert_eq!(text.lines().count(), 1, "one line per check — {text}");

    let out = check("production-write", &empty);
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    for key in [
        "prod_buckets",
        "prod_projects",
        "prod_apps",
        "prod_make_goals",
        "prod_dagger_functions",
        "prod_workflow_refs",
    ] {
        assert!(
            text.contains(&format!("not configured — [guards.targets] {key}")),
            "every unconfigured key is named, not only the first — {text}"
        );
    }
    assert_eq!(text.lines().count(), 6, "one line per check — {text}");

    let full = Scratch::new("check-full");
    full.write(".fleet/project.toml", TARGETS);
    for class in ["release-ref", "production-write"] {
        let out = check(class, &full);
        assert_eq!(
            out.status.code(),
            Some(0),
            "every target configured exits 0 — {class}"
        );
        let text = String::from_utf8(out.stdout).expect("the output is utf-8");
        assert!(
            !text.contains("not configured"),
            "and says so on every line — {text}"
        );
    }
}

// ---- the provider, and where it is allowed to be ----------------------------

/// The classes are called with a command string and NO PAYLOAD AT ALL, which is
/// what "no provider reaches fleet-core" means when it is measured rather than
/// asserted: a second provider's overlay adds a reader of its own beside the
/// adapter and reuses exactly this call.
#[test]
fn the_classes_are_callable_through_fleet_core_with_no_payload_at_all() {
    let policy = fleet_core::guard::Policy {
        enabled: true,
        item_prefix: Some("acme".to_string()),
        ..Default::default()
    };
    let with_targets = fleet_core::guard::Policy {
        release_ref_glob: Some("refs/heads/*release/*".to_string()),
        prod_buckets: vec!["live.example.test".to_string()],
        ..policy.clone()
    };
    for (class, command, check, policy) in [
        (
            fleet_core::guard::Class::ShellTrap,
            TRAP,
            "modifier",
            &policy,
        ),
        (
            fleet_core::guard::Class::Record,
            replace(),
            "notes-replace",
            &policy,
        ),
        (
            fleet_core::guard::Class::ReleaseRef,
            PUSH,
            "push-target",
            &with_targets,
        ),
        (
            fleet_core::guard::Class::ProductionWrite,
            BUCKET_WRITE,
            "bucket",
            &with_targets,
        ),
    ] {
        match fleet_core::guard::judge(class, command, policy) {
            fleet_core::guard::Verdict::Refused(denial) => {
                assert_eq!(denial.check, check, "{command}");
                let carried = match denial.escape {
                    Some(escape) => denial.reason().contains(escape),
                    None => denial.reason().contains(fleet_core::guard::NO_ESCAPE),
                };
                assert!(
                    carried,
                    "the whole refusal is assembled without a payload — {}",
                    denial.reason()
                );
            }
            fleet_core::guard::Verdict::Silent => panic!("{command}: allowed"),
        }
    }
}

/// A guard never emits an allowing decision: an explicit one from a pre-tool
/// hook short-circuits the agent's whole permission system, so letting a
/// command through means printing nothing. Read off the sources rather than off
/// a run, because the claim is about what the code CAN say — and read in two
/// directions, because the decision value belongs to ONE provider: the
/// adapter's hook mapping states the refusing one and nothing else, and core's
/// guard module states none at all.
#[test]
fn the_adapter_states_the_refusing_decision_alone_and_core_states_none() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits inside the workspace")
        .to_path_buf();
    let allowing = format!("\"{}\"", "allow");

    let adapter = workspace.join("controller/src/adapter/claude_code.hook.toml");
    let text = std::fs::read_to_string(&adapter).expect("the adapter's mapping is readable");
    let mut stated = 0;
    for (offset, _) in text.match_indices("permissionDecision") {
        let after = &text[offset..];
        if after.starts_with("permissionDecisionReason") {
            continue;
        }
        assert!(
            after.contains("\"deny\""),
            "the adapter states a decision value that is not the refusing one"
        );
        stated += 1;
    }
    assert_eq!(stated, 1, "the adapter states the decision exactly once");
    assert!(
        !text.contains(&allowing),
        "the adapter states an allowing decision value"
    );

    let mut core_sources = vec![workspace.join("cli/src/main.rs")];
    let dir = workspace.join("core/src/guard");
    for entry in std::fs::read_dir(&dir).expect("the guard module is a directory") {
        let path = entry.expect("an entry is readable").path();
        if path.extension().is_some_and(|e| e == "rs") {
            core_sources.push(path);
        }
    }
    assert!(
        core_sources.len() >= 5,
        "the scan read the sources it claims to: {core_sources:?}"
    );
    for path in &core_sources {
        let text = std::fs::read_to_string(path).expect("a source is readable");
        assert!(
            !text.contains("permissionDecision"),
            "{}: states a decision value, which is the adapter's alone",
            path.display()
        );
        assert!(
            !text.contains(&allowing),
            "{}: states an allowing decision value",
            path.display()
        );
    }
}

#[test]
fn a_caller_who_did_not_name_a_class_is_a_usage_error() {
    for args in [
        vec!["guard"],
        vec!["guard", "astrologer"],
        vec!["guard", "shell-trap", "record"],
        vec!["guard", "shell-trap", "--wat"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(&args)
            .hermetic_nowhere()
            .output()
            .expect("the built binary runs");
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?}: a usage error is 2, apart from both guard answers"
        );
    }
}
