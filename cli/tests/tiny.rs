//! The doctrine pack as it ships: it names only verbs this binary has, it
//! checks clean with every slot it fills, and its doctor entries and rituals
//! are what they say they are.
//!
//! Each sweep carries the control that makes its zero a reading, and the scan
//! set is the pack's DIRECTORY, so a document added later is read without
//! anyone adding it here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use fleet_core::pack;

mod common;
use common::hermetic::Hermetic;

fn fleet_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits under the workspace root")
        .to_path_buf()
}

/// Every file under the doctrine pack, with its path relative to the pack.
fn pack_files() -> Vec<(String, String)> {
    let files = pack_files_in(&fleet_root().join("packs/tiny"));
    assert!(
        files.len() >= 4,
        "the pack was walked and holds its documents: {:?}",
        files.iter().map(|(p, _)| p).collect::<Vec<_>>()
    );
    files
}

/// Every file under `root` the pack check reads, as text. The OS litter the
/// check reads past is read past here too: a Finder `.DS_Store` is none of the
/// pack's documents, and it is not UTF-8.
fn pack_files_in(root: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(root.join(&p))
                .unwrap_or_else(|e| panic!("{p} is readable: {e}"));
            (p, text)
        })
        .collect()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("the pack directory is readable") {
        let entry = entry.expect("the entry is readable");
        if pack::is_os_litter(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        if entry.file_type().expect("the kind is readable").is_dir() {
            walk(root, &path, out);
        } else {
            out.push(
                path.strip_prefix(root)
                    .expect("every walked path is under the pack")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

// ---- the verbs --------------------------------------------------------------

/// The command families the binary lists under its help flag, read from the
/// binary rather than from a list here.
fn families() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .arg("--help")
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    assert!(out.status.success(), "--help exits 0");
    let text = String::from_utf8(out.stdout).expect("the page is utf-8");
    let commands = text
        .split_once("Commands:")
        .expect("the help page lists its families")
        .1;
    let mut names = Vec::new();
    for line in commands.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with("  ") || line.starts_with("   -") {
            break;
        }
        let word = line.split_whitespace().next().unwrap_or("");
        if word.starts_with('-') {
            break;
        }
        if !word.is_empty() {
            names.push(word.to_string());
        }
    }
    assert!(
        names.iter().any(|n| n == "pack"),
        "the families were read off the page: {names:?}"
    );
    names
}

/// Every word following a backticked `fleet ` in `text`. The backtick is what
/// makes the occurrence a command and not the noun in a sentence.
fn invoked_verbs(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("`fleet ") {
        let start = from + at + "`fleet ".len();
        let word: String = text[start..]
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '-')
            .collect();
        if !word.is_empty() {
            found.push(word);
        }
        from = start;
    }
    found
}

#[test]
fn every_verb_the_pack_invokes_is_one_this_binary_has() {
    let families = families();

    // The control: a verb nobody implemented, run through the same extractor
    // and the same membership test the sweep below uses.
    let made_up = invoked_verbs("run `fleet frobnicate` afterwards");
    assert_eq!(made_up, vec!["frobnicate".to_string()]);
    assert!(
        !families.contains(&made_up[0]),
        "and the membership test rejects it"
    );

    let mut seen = 0;
    for (path, text) in pack_files() {
        for verb in invoked_verbs(&text) {
            seen += 1;
            assert!(
                families.contains(&verb),
                "{path} invokes `fleet {verb}`, which this binary does not have: \
                 {families:?}"
            );
        }
    }
    assert!(
        seen > 0,
        "the pack invokes verbs at all — a sweep that found none would pass \
         whatever the documents said"
    );
}

/// THE PACK TEACHES THE DELIVERY AS A JSON FILE: the builder's prompt names the
/// flag and the three fields whose meaning it carries beyond their shape, and
/// no document hands a seat the note flag `fleet deliver` refuses.
#[test]
fn the_pack_teaches_the_delivery_file_and_never_the_note_flag() {
    let files = pack_files();
    let (_, prompt) = files
        .iter()
        .find(|(path, _)| path == "agents/builder/prompt.template.md")
        .expect("the pack ships the builder's prompt");
    for wanted in [
        "`fleet deliver --delivery <file>`",
        "`spec_corrections`",
        "`decisions`",
        "`not_proven`",
    ] {
        assert!(
            prompt.contains(wanted),
            "the builder's prompt teaches {wanted}"
        );
    }
    for (path, text) in &files {
        assert!(
            !text.contains("deliver --note"),
            "{path} hands a seat `fleet deliver --note`, which is gone"
        );
    }
}

// ---- the slots the pack fills -----------------------------------------------

/// The pack's own directory, checked by the verb that validates one.
#[test]
fn the_pack_checks_clean_with_every_slot_it_fills() {
    checks_clean_with_every_slot(&fleet_root().join("packs/tiny"));
}

/// The same pack after a file browser has been through it: Finder's
/// `.DS_Store` at the top level, in the skills slot and inside one skill, and
/// Explorer's two beside them. The verb reports what it reports on the clean
/// copy, and the sweeps above read the same documents off both.
#[test]
fn the_pack_checks_clean_and_reads_the_same_with_os_litter_in_it() {
    let rig = Rig::new("litter");
    let copy = rig.dir("tiny");
    fleet_core::test_support::copy_tree(&fleet_root().join("packs/tiny"), &copy);
    for dir in ["", "skills/", "skills/wake/", "assets/", "doctor/"] {
        for name in pack::OS_LITTER {
            std::fs::write(
                copy.join(format!("{dir}{name}")),
                b"\x00\x00\x00\x01Bud1\x00\x00\x10\x00\xff\xfe",
            )
            .expect("the litter is written");
        }
    }
    assert!(
        copy.join("skills/.DS_Store").is_file(),
        "the litter landed where the check walks"
    );

    checks_clean_with_every_slot(&copy);
    let paths =
        |files: Vec<(String, String)>| files.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
    assert_eq!(
        paths(pack_files_in(&copy)),
        paths(pack_files()),
        "the littered copy holds the pack's documents and nothing more"
    );
}

/// `fleet pack check` over `dir`: exit 0, and one line per slot the doctrine
/// pack fills with the entry count it fills it with.
fn checks_clean_with_every_slot(dir: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .arg("pack")
        .arg("check")
        .arg(dir)
        .current_dir(fleet_root())
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    let text = String::from_utf8(out.stdout).expect("the report is utf-8");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the pack checks clean — {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for line in [
        "slot overlay: 1 entry".to_string(),
        "slot doctor: 3 entries".to_string(),
        "slot agents: 2 entries".to_string(),
        "slot assets: 3 entries".to_string(),
        "slot workflows: 1 entry".to_string(),
        format!("slot skills: {} entries", RITUALS.len()),
    ] {
        assert!(text.contains(&line), "the report names {line} — {text}");
    }
}

// ---- the two doctor entries -------------------------------------------------

/// A scratch tree for one doctor entry, with a PATH of its own so the entry's
/// answer is about what THIS case put on it and not about the host.
struct Rig {
    root: PathBuf,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock reads")
            .subsec_nanos();
        let root =
            std::env::temp_dir().join(format!("fleet-tiny-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch tree is created");
        Rig { root }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(&path).expect("the directory is created");
        path
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().expect("the file has a parent"))
            .expect("the parent is created");
        std::fs::write(&path, contents).expect("the file is written");
        path
    }

    fn script(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.write(relative, body);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("the script is executable");
        }
        path
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn entry(name: &str) -> PathBuf {
    fleet_root()
        .join("packs/tiny/doctor")
        .join(name)
        .join("run.sh")
}

fn run_entry(name: &str, cwd: &Path, path: &str) -> Output {
    Command::new("sh")
        .arg(entry(name))
        .current_dir(cwd)
        .env("PATH", path)
        .output()
        .expect("the doctor entry runs")
}

fn bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fleet"))
        .parent()
        .expect("the binary sits in a directory")
        .to_path_buf()
}

/// The host's own PATH, which is what makes `git` and `sh` reachable, with
/// `extra` ahead of it.
fn path_with(extra: &[&Path]) -> String {
    let mut parts: Vec<String> = extra.iter().map(|p| p.display().to_string()).collect();
    parts.push(std::env::var("PATH").unwrap_or_default());
    parts.join(":")
}

/// The same, with every host directory that holds a `fleet` executable left
/// out: a box with the binary installed on its PATH must not answer for the
/// control that asks what happens when nothing resolves.
fn path_without_fleet(extra: &[&Path]) -> String {
    let mut parts: Vec<String> = extra.iter().map(|p| p.display().to_string()).collect();
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if !dir.join("fleet").is_file() {
            parts.push(dir.display().to_string());
        }
    }
    parts.join(":")
}

#[test]
fn verbs_on_path_prints_the_copy_it_resolved_and_fails_when_nothing_resolves() {
    let rig = Rig::new("verbs");
    let here = rig.dir("here");
    let bin = bin_dir();

    let out = run_entry("verbs-on-path", &here, &path_with(&[&bin]));
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert_eq!(
        out.status.code(),
        Some(0),
        "both answers came back — {text}"
    );
    assert!(
        text.contains(&bin.join("fleet").display().to_string()),
        "the line names the copy it resolved, not just a version — {text}"
    );
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "and the version that copy answers with — {text}"
    );

    // The control: the same entry with nothing of the sort on PATH. A directory
    // of its own rather than an empty PATH, so `sh` still has its tools, and
    // the host's own fleet-holding directories left out, so the difference
    // between the two runs is the one variable on any box.
    let bare = rig.dir("bare");
    let out = run_entry("verbs-on-path", &here, &path_without_fleet(&[&bare]));
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert_eq!(out.status.code(), Some(1), "nothing resolved — {text}");
    assert!(
        text.contains("no fleet on PATH"),
        "and the line says which half failed — {text}"
    );
}

/// The work graph, stubbed: `show <id> --json` answers closed for one id, open
/// for another, and refuses for a third, so the entry's three exits are driven
/// by what the store said rather than by whether it was there.
const STUB_STORE: &str = "#!/bin/sh\n\
                          case \"$2\" in\n\
                          acme-c10s) printf '[{\"id\":\"acme-c10s\",\"status\":\"closed\"}]\\n' ;;\n\
                          acme-op3n) printf '[{\"id\":\"acme-op3n\",\"status\":\"open\"}]\\n' ;;\n\
                          *) exit 1 ;;\n\
                          esac\n";

const DECLARATION: &str = "[project]\nname = \"a-project\"\nitem_prefix = \"acme\"\n";

fn vcs(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "a")
        .env("GIT_AUTHOR_EMAIL", "a@example.test")
        .env("GIT_COMMITTER_NAME", "a")
        .env("GIT_COMMITTER_EMAIL", "a@example.test")
        .output()
        .expect("the version control system runs");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// A scratch project with a scratch remote: one commit on the trunk, and one
/// branch per case pushed to it.
fn a_project_with_a_remote(rig: &Rig, branches: &[&str]) -> PathBuf {
    let remote = rig.dir("remote.git");
    vcs(
        &rig.root,
        &["init", "--bare", "--initial-branch=main", "remote.git"],
    );

    let work = rig.dir("work");
    vcs(&work, &["init", "--initial-branch=main"]);
    rig.write("work/.fleet/project.toml", DECLARATION);
    rig.write("work/a-file", "one\n");
    vcs(&work, &["add", "a-file"]);
    vcs(&work, &["commit", "-m", "the first commit"]);
    vcs(
        &work,
        &["remote", "add", "origin", &remote.display().to_string()],
    );
    vcs(&work, &["push", "origin", "main"]);
    for branch in branches {
        vcs(
            &work,
            &["push", "origin", &format!("main:refs/heads/{branch}")],
        );
    }
    work
}

fn remote_branches(work: &Path) -> String {
    String::from_utf8(vcs(work, &["ls-remote", "--heads", "origin"]).stdout)
        .expect("the listing is utf-8")
}

#[test]
fn stale_branches_reports_a_closed_items_branch_and_leaves_the_branch_where_it_is() {
    let rig = Rig::new("stale");
    let store = rig.dir("store");
    rig.script("store/bd", STUB_STORE);
    let path = path_with(&[&store]);

    // 0: a remote carrying nothing but the trunk.
    let clean = a_project_with_a_remote(&rig, &[]);
    let out = run_entry("stale-branches", &clean, &path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "nothing to report — {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // 1: two branches, one item closed and one open. The open one is the control
    // that makes the finding a reading of the store rather than of the name.
    let rig = Rig::new("stale-one");
    let store = rig.dir("store");
    rig.script("store/bd", STUB_STORE);
    let path = path_with(&[&store]);
    let work = a_project_with_a_remote(&rig, &["ab/feat/acme-c10s", "ab/feat/acme-op3n"]);

    let out = run_entry("stale-branches", &work, &path);
    let text = String::from_utf8(out.stdout).expect("the output is utf-8");
    assert_eq!(
        out.status.code(),
        Some(1),
        "one branch is stale — {text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("ab/feat/acme-c10s") && text.contains("--delete ab/feat/acme-c10s"),
        "the line names the branch and the command that would delete it — {text}"
    );
    assert!(
        !text.contains("acme-op3n"),
        "and says nothing about the branch whose item is open — {text}"
    );
    assert_eq!(text.lines().count(), 1, "one line per finding — {text}");

    // IT PRINTS AND NEVER RUNS. The branch is still on the remote afterwards.
    let listing = remote_branches(&work);
    assert!(
        listing.contains("refs/heads/ab/feat/acme-c10s"),
        "the entry reported and deleted nothing — {listing}"
    );

    // 3, twice: an item the store cannot answer for, and a remote that is not
    // there. Both are could-not-read, which is a different answer from clean.
    let rig = Rig::new("stale-unreadable");
    let store = rig.dir("store");
    rig.script("store/bd", STUB_STORE);
    let path = path_with(&[&store]);
    let work = a_project_with_a_remote(&rig, &["ab/feat/acme-zz9z"]);
    let out = run_entry("stale-branches", &work, &path);
    assert_eq!(
        out.status.code(),
        Some(3),
        "the store could not answer for that item — {}",
        String::from_utf8_lossy(&out.stderr)
    );

    vcs(
        &work,
        &["remote", "set-url", "origin", "/no/such/remote/at/all.git"],
    );
    let out = run_entry("stale-branches", &work, &path);
    assert_eq!(
        out.status.code(),
        Some(3),
        "the remote could not be read — {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---- the ritual skills ------------------------------------------------------

/// The rituals the pack ships, each with the line budget its spec set, and
/// `None` where the spec set none: the two carried from elsewhere whose length
/// is the house style's, and the two flight skills.
const RITUALS: [(&str, Option<usize>); 11] = [
    ("wake", Some(150)),
    ("handoff", Some(130)),
    ("rest", Some(40)),
    ("clock-out", Some(40)),
    ("morning", Some(90)),
    ("corrections-review", Some(90)),
    ("praise", Some(50)),
    ("report", None),
    ("runbook", None),
    ("preboard", None),
    ("takeoff", None),
];

/// Every `SKILL.md` under the pack, by the directory that holds it. The walk is
/// the pack's own, so a skill added later is read without anyone adding it here.
fn pack_skills() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = pack_files()
        .into_iter()
        .filter_map(|(path, text)| {
            let rest = path.strip_prefix("skills/")?;
            let (dir, file) = rest.split_once('/')?;
            (file == "SKILL.md").then(|| (dir.to_string(), text))
        })
        .collect();
    found.sort();
    assert!(
        !found.is_empty(),
        "the pack's skills slot was walked and holds skills — a sweep over an \
         empty set would pass whatever the documents said"
    );
    found
}

/// The frontmatter block's lines: the file opens on `---` and the block closes
/// on the next one. `None` where either delimiter is missing.
fn frontmatter(text: &str) -> Option<Vec<&str>> {
    let body = text.strip_prefix("---\n")?;
    let (block, _) = body.split_once("\n---")?;
    Some(block.lines().collect())
}

#[test]
fn every_ritual_the_pack_names_is_a_skill_with_frontmatter() {
    let skills = pack_skills();
    let names: Vec<&str> = skills.iter().map(|(dir, _)| dir.as_str()).collect();
    for (ritual, _) in RITUALS {
        assert!(
            names.contains(&ritual),
            "the pack ships the `{ritual}` skill: {names:?}"
        );
    }

    // The control on the parser below: frontmatter that is not there reads as
    // absent rather than as an empty block satisfying every assertion under it.
    assert!(
        frontmatter("# no frontmatter here\n").is_none(),
        "the parser reports a file with no frontmatter as having none"
    );

    for (dir, text) in &skills {
        let front = frontmatter(text)
            .unwrap_or_else(|| panic!("{dir}/SKILL.md opens on its frontmatter block"));
        let name = front
            .iter()
            .find_map(|line| line.strip_prefix("name:"))
            .unwrap_or_else(|| panic!("{dir}/SKILL.md carries a `name:` line: {front:?}"))
            .trim();
        assert_eq!(
            name, dir,
            "{dir}/SKILL.md names itself for the directory it lives in — the \
             provider resolves a skill by the frontmatter name"
        );
        let description = front
            .iter()
            .find_map(|line| line.strip_prefix("description:"))
            .unwrap_or_else(|| panic!("{dir}/SKILL.md carries a `description:` line: {front:?}"))
            .trim();
        assert!(
            !description.is_empty(),
            "{dir}/SKILL.md's description says something"
        );
    }
}

#[test]
fn every_verb_a_ritual_invokes_is_one_this_binary_has() {
    let families = families();

    // The control: a verb nobody implemented, through the same extractor and
    // the same membership test the sweep below uses.
    let made_up = invoked_verbs("then run `fleet frobnicate` and read the exit");
    assert_eq!(made_up, vec!["frobnicate".to_string()]);
    assert!(
        !families.contains(&made_up[0]),
        "and the membership test rejects it"
    );

    let mut seen = 0;
    for (dir, text) in pack_skills() {
        for verb in invoked_verbs(&text) {
            seen += 1;
            assert!(
                families.contains(&verb),
                "{dir}/SKILL.md invokes `fleet {verb}`, which this binary does \
                 not have: {families:?}"
            );
        }
    }
    assert!(
        seen > 0,
        "the rituals invoke verbs at all — a sweep that found none would pass \
         whatever they said"
    );
}

#[test]
fn every_ritual_is_under_the_line_budget_its_spec_set() {
    let skills = pack_skills();
    for (ritual, cap) in RITUALS {
        let Some(cap) = cap else { continue };
        let (_, text) = skills
            .iter()
            .find(|(dir, _)| dir == ritual)
            .unwrap_or_else(|| panic!("the pack ships the `{ritual}` skill"));
        let lines = text.lines().count();
        assert!(
            lines <= cap,
            "{ritual}/SKILL.md is {lines} lines, over its budget of {cap} — a \
             ritual read fresh every invocation is paid for every invocation"
        );
    }
}
