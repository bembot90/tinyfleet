//! The doctrine pack's boundary: it is this fleet's own knowledge, and it names
//! only verbs this binary has.
//!
//! Three claims, each with the control that makes its zero a reading. The
//! needles are READ from the surrounding project's own files rather than typed
//! here, because a file under `fleet/` that spelled them would be the very thing
//! it is checking for; and the scan set is a DIRECTORY, so this file is outside
//! it by structure rather than by a pattern that exempts it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod common;
use common::hermetic::Hermetic;

fn fleet_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits under the workspace root")
        .to_path_buf()
}

/// The tree `fleet/` currently sits in. Where it holds neither file this arm
/// reads, the arm says so and fails rather than passing on an empty needle set.
fn surrounding_root() -> PathBuf {
    fleet_root()
        .parent()
        .expect("the workspace sits inside something")
        .to_path_buf()
}

fn read_at(root: &Path, relative: &str) -> String {
    let path = root.join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} is readable — this check cannot run without it, and a pass \
             without it would prove nothing: {e}",
            path.display()
        )
    })
}

/// Every file under the doctrine pack, with its path relative to the pack.
fn pack_files() -> Vec<(String, String)> {
    let root = fleet_root().join("packs/tiny");
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found.sort();
    assert!(
        found.len() >= 4,
        "the pack was walked and holds its documents: {found:?}"
    );
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

// ---- the item-id grammar ----------------------------------------------------

/// The surrounding project's own token, read from the one file under `fleet/`
/// that is allowed to spell it — the boundary checker, whose job is naming it.
fn foreign_token() -> String {
    let text = read_at(&surrounding_root(), "fleet/tools/lessons-check");
    let line = text
        .lines()
        .find(|l| l.starts_with("FOREIGN_TOKEN = \""))
        .expect("the boundary checker declares the token it scans for");
    let token = line
        .trim_start_matches("FOREIGN_TOKEN = \"")
        .trim_end_matches('"')
        .to_string();
    assert!(!token.is_empty(), "the token was read and is not empty");
    token
}

/// Every `<token>-<suffix>` in `text`: the id grammar is the project's item
/// prefix, a hyphen, and a suffix, so a hyphen followed by anything else is
/// prose and not an id.
fn item_ids(text: &str, token: &str) -> Vec<String> {
    let hay = text.to_ascii_lowercase();
    let needle = format!("{}-", token.to_ascii_lowercase());
    let bytes = hay.as_bytes();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let mut end = start + needle.len();
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'.') {
            end += 1;
        }
        if end > start + needle.len() {
            found.push(hay[start..end].to_string());
        }
        from = start + needle.len();
    }
    found
}

#[test]
fn no_file_in_the_pack_carries_an_item_id_from_the_surrounding_project() {
    let token = foreign_token();

    // The control: the scanner is shown an id it must find, built from the same
    // token, so a clean sweep below is a reading rather than a broken matcher.
    assert_eq!(
        item_ids(&format!("filed as {token}-ab1c.2 last week"), &token),
        vec![format!("{}-ab1c.2", token.to_ascii_lowercase())],
        "the scanner finds an id when there is one"
    );
    assert!(
        item_ids(&format!("a {token}-shaped thing"), &token).len() == 1,
        "and it is deliberately generous: anything after the hyphen counts"
    );

    for (path, text) in pack_files() {
        let hits = item_ids(&text, &token);
        assert!(
            hits.is_empty(),
            "{path} names an item of the surrounding project: {hits:?} — the \
             pack's documents cite a PRD section, never an item"
        );
    }
}

// ---- the roster -------------------------------------------------------------

/// The chosen names and seat directories of the rendered roster block, read
/// from the block rather than typed here, so a seat added later is a needle
/// nobody has to remember to add.
fn roster_needles() -> Vec<String> {
    let text = read_at(&surrounding_root(), "seats/README.md");
    let begin = text
        .find("<!-- roster:begin")
        .expect("the roster block opens");
    let after = text[begin..]
        .find("-->")
        .expect("the opening marker closes")
        + begin
        + 3;
    let end = text[after..]
        .find("<!-- roster:end -->")
        .expect("the roster block closes")
        + after;

    let mut needles = Vec::new();
    for line in text[after..end].lines() {
        let cells: Vec<&str> = line.split('|').collect();
        if cells.len() < 3 {
            continue;
        }
        if let Some(name) = between(cells[1], "**", "**") {
            needles.push(name);
        }
        if let Some(seat) = between(cells[2], "`", "/`") {
            needles.push(seat);
        }
    }
    needles.sort();
    needles.dedup();
    assert!(
        needles.len() >= 4,
        "the roster block yielded needles — a scan against an empty set would \
         prove nothing: {needles:?}"
    );
    needles
}

fn between(cell: &str, open: &str, close: &str) -> Option<String> {
    let start = cell.find(open)? + open.len();
    let rest = &cell[start..];
    let end = rest.find(close)?;
    let inner = &rest[..end];
    (!inner.is_empty()).then(|| inner.to_string())
}

/// `needle` as a whole word, case-insensitively: a hit whose neighbour is a
/// letter or a digit is part of a longer word and is not the name.
fn names_word(text: &str, needle: &str) -> bool {
    let hay = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let bytes = hay.as_bytes();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        let before = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

#[test]
fn no_file_in_the_pack_names_a_seat_of_the_surrounding_project() {
    let needles = roster_needles();

    // The control: the same matcher, on a line that does name one.
    let planted = format!("handed to {} on Tuesday", needles[0]);
    assert!(
        names_word(&planted, &needles[0]),
        "the matcher finds a name when there is one"
    );
    assert!(
        !names_word(&format!("x{}x", needles[0]), &needles[0]),
        "and it reads whole words, not substrings"
    );

    for (path, text) in pack_files() {
        for needle in &needles {
            assert!(
                !names_word(&text, needle),
                "{path} names the seat '{needle}' — the pack is written for a \
                 fleet that is not the one it was extracted from"
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

// ---- the slots the pack fills -----------------------------------------------

/// The pack's own directory, checked by the verb that validates one.
#[test]
fn the_pack_checks_clean_with_every_slot_it_fills() {
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["pack", "check", "packs/tiny"])
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
