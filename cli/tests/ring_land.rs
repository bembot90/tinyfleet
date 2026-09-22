//! `fleet land` through the shipped binary, against a REAL remote (packs PRD
//! R8, R10; cli PRD § `fleet land`).
//!
//! This is where the live git path is proven. The rig is three repositories: a
//! bare one that is `origin`, a second one beside it that is the primary and
//! pushes to it, and a LINKED WORKTREE of that primary which is the reviewer's
//! checkout — because the verb refuses to land from a primary, and a worktree
//! is the only way to give it something that is genuinely not one.
//!
//! UNIX WITH `script(1)`. The bar can only be read on a real pseudo-terminal,
//! which this file allocates the way `tests/ui.rs` does; the arm that reads it
//! probes for the tool and says so rather than running if it is not there.
//!
//! The bare is asked what it holds after every arm. A landing is a claim about
//! a remote, and only the remote can answer it: `main` on the bare moved, or it
//! did not.
//!
//! Every rc below is read from the child's own status and never off anything it
//! printed (R10).

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant};

use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// Every child this file spawns — the shipped binary, `bd` and `git` —
/// counted rather than estimated. `FLEET_TEST_SPAWNS=1` prints each one as it
/// is taken, numbered and stamped; nextest runs one arm per process, so the
/// highest number a process prints is that arm's own count.
///
/// The kind leads the label, so a reading can separate the binary's spawns
/// from the store's: `fleet` is the shipped binary, `bd` the work graph, `git`
/// the repositories.
static SPAWNS: AtomicUsize = AtomicUsize::new(0);

static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);

fn spawned(what: &str) {
    let n = SPAWNS.fetch_add(1, Ordering::SeqCst) + 1;
    if std::env::var_os("FLEET_TEST_SPAWNS").is_some() {
        eprintln!("SPAWN {n} {:.2}s {what}", STARTED.elapsed().as_secs_f64());
    }
}

const REVIEWER: &str = "a-reviewer";
const BUILDER: &str = "a-builder";
const WORK: &str = "a-builder/feat/the-work";
/// The second item's branch, for the arm that asks for two landings at once.
const OTHER: &str = "a-builder/feat/the-other";

/// The suite is a script whose exit a seam file sets, so an arm chooses green
/// or red without changing the command the note reports.
const POLICY: &str = "[gates]\nsuite = \"sh the-suite.sh\"\nci_marker = \"sh the-marker.sh\"\n\n\
                      [core]\nreviewer = \"a-reviewer\"\n";

/// Where every stub script below reads its seam files from: the rig's own root,
/// handed to the landing and inherited by the children it spawns.
///
/// NOT A PATH IN THE SCRIPT. The scripts are COMMITTED, and each arm's rig is a
/// copy of one template, so a path written into one would be the template's in
/// every copy — one arm's timing and one arm's verdict then read by all of
/// them, silently and in the direction that looks like a pass.
const SEAM: &str = "FLEET_TEST_SEAM";

/// The marker: one word over whatever pathset it is handed, exiting what
/// `the-marker-rc` says. Its refusal is the only one a test can aim at the
/// window BETWEEN the squash and the commit, which is where a squash is staged
/// and a put-back has something to undo.
fn marker_script() -> String {
    format!(
        "#!/bin/sh\n\
         cat >/dev/null\n\
         printf '[skip ci]'\n\
         exit \"$(cat \"${SEAM}/the-marker-rc\")\"\n"
    )
}

/// The suite: it writes a line a second for as long as `the-sleep` says and
/// exits what `the-rc` says. Both seam files sit OUTSIDE the repository, so an
/// arm chooses its timing and its verdict without leaving the tree dirty —
/// which this verb refuses.
///
/// `the-hold` is the third seam and the one the lane's arm uses: the suite
/// holds until `the-release` appears. A HOLD AND NOT A SLEEP, because the arm
/// around it is about a lock and a budget in seconds would be a clock racing
/// the box rather than a reading of the lock.
///
/// It also runs `the-during`, when an arm wrote one. That is the only place a
/// test can reach INSIDE the landing act — between the fetch the land branch
/// was cut at and the fetch the push counts against — which is the one window
/// the current-trunk gate exists for.
fn suite_script() -> String {
    format!(
        "#!/bin/sh\n\
         if [ -f \"${SEAM}/the-during\" ]; then sh \"${SEAM}/the-during\"; fi\n\
         if [ -f \"${SEAM}/the-hold\" ]; then\n\
         \x20 while [ ! -f \"${SEAM}/the-release\" ]; do sleep 0.05; done\n\
         fi\n\
         i=0\n\
         while [ \"$i\" -lt \"$(cat \"${SEAM}/the-sleep\")\" ]; do\n\
         \x20 printf 'the suite is running\\n'\n\
         \x20 sleep 1\n\
         \x20 i=$((i+1))\n\
         done\n\
         printf 'the suite ran\\n'\n\
         exit \"$(cat \"${SEAM}/the-rc\")\"\n"
    )
}

fn defaults_into(machine: &Path) -> PathBuf {
    let root = machine.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&root).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
    root
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination directory is created");
    for entry in std::fs::read_dir(from).expect("the source directory is readable") {
        let entry = entry.expect("an entry is readable");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file is copied");
        }
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The terminal these arms state for themselves rather than inherit, as the ui
/// suite's own pty helper states it and for the same reason: `indicatif` hands
/// back a HIDDEN draw target whenever `TERM` is unset or `dumb`
/// (`ProgressDrawTarget::term`, over `console::is_dumb`, whose Unix default for
/// an unset `TERM` is dumb), so a process carrying no `TERM` draws no bar on a
/// real tty. The environments this suite is run under include cleared ones — a
/// workflow's gate builds its suite one — so an arm about what a terminal shows
/// names the terminal it needs.
const TERM: &str = "xterm-256color";

/// `script` with the child's streams on a pseudo-terminal, as the ui suite
/// allocates one: nothing here mocks a tty, because the reading being taken IS
/// whether the process believes it has one.
#[cfg(target_os = "macos")]
fn pty_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new("script");
    command.arg("-q").arg("/dev/null").arg(program).args(args);
    command.env("TERM", TERM);
    command
}

#[cfg(not(target_os = "macos"))]
fn pty_command(program: &str, args: &[&str]) -> Command {
    let mut line = quoted(program);
    for arg in args {
        line.push(' ');
        line.push_str(&quoted(arg));
    }
    let mut command = Command::new("script");
    command
        .arg("-q")
        .arg("-e")
        .arg("-c")
        .arg(line)
        .arg("/dev/null")
        .env("TERM", TERM);
    command
}

#[cfg(not(target_os = "macos"))]
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

// ---- the template ------------------------------------------------------------

/// The rig every arm copies: the bare, the primary that pushes to it, both work
/// branches, the pack and the scratch board — built once and never mutated
/// afterwards.
///
/// ONE PER PROCESS, the way `tests/dispatch.rs`'s shared project is, and it
/// outlives the arms that copy it: a shared handle has no owner to drop it, so
/// it is left under the system temp directory named by this process's id.
///
/// THE REVIEWER'S CHECKOUT IS NOT IN IT. A linked worktree's `.git` is a file
/// holding an absolute gitdir and the repository points back at it just as
/// absolutely, so a copied one reads the TEMPLATE's repository while every
/// assertion about a remote goes on passing. Each arm adds its own, from its
/// own bare.
struct Template {
    root: PathBuf,
    bare: PathBuf,
    primary: PathBuf,
    /// The pack every arm's binary is pointed at. Read and never written, so
    /// one copy serves them all.
    packs: PathBuf,
    /// The commit the review accepts, on [`WORK`].
    commit: String,
    /// The second item's commit, on [`OTHER`], for the arm that lands two.
    other: String,
}

impl Template {
    fn shared() -> &'static Template {
        static TEMPLATE: OnceLock<Template> = OnceLock::new();
        TEMPLATE.get_or_init(|| Template::at(built_once("with-gates", POLICY)))
    }

    /// The fixture read off the tree, by the process that built it and by every
    /// process that found it already there. The two commits are read from files
    /// the build left rather than from the build, because most processes do not
    /// build.
    fn at(root: PathBuf) -> Template {
        Template {
            bare: root.join("bare"),
            primary: root.join("primary"),
            packs: root.join("packs"),
            commit: a_sha(&root.join("the-commit")),
            other: a_sha(&root.join("the-other")),
            root,
        }
    }

    /// Built in the order the pieces actually depend on each other: the trunk,
    /// the two work branches, then the work graph, whose tracked half rides the
    /// trunk.
    ///
    /// THE STORE IS THE PRIMARY'S, as it is on a real box: `bd init` from a
    /// linked worktree writes into the main worktree, and the database itself
    /// is never versioned. What a reviewer's checkout gets is the tracked half
    /// — the export among it — which is the file a landing rewrites.
    fn build(&mut self, policy: &str, label: &str) {
        std::fs::create_dir_all(self.root.join("hooks")).expect("the fixture directory is made");
        defaults_into(&self.root);

        git(
            &self.root,
            &["init", "--bare", "--quiet", "-b", "main", "bare"],
        );
        git(&self.root, &["init", "--quiet", "-b", "main", "primary"]);
        let primary = self.primary.clone();
        git(
            &primary,
            &[
                "config",
                "core.hooksPath",
                &self.root.join("hooks").display().to_string(),
            ],
        );
        git(
            &primary,
            &["remote", "add", "origin", &self.bare.display().to_string()],
        );

        write(&primary.join("fleet.toml"), policy);
        write(&primary.join("the-suite.sh"), &suite_script());
        write(&primary.join("the-marker.sh"), &marker_script());
        git(
            &primary,
            &["add", "--", "fleet.toml", "the-suite.sh", "the-marker.sh"],
        );
        git(
            &primary,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "the policy"],
        );
        git(&primary, &["push", "--quiet", "origin", "HEAD:main"]);

        self.commit = self.a_branch(WORK, "the-work.txt", "the work");
        self.other = self.a_branch(OTHER, "the-other.txt", "the other work");
        self.a_board(label);
        write(&self.root.join("the-commit"), &self.commit);
        write(&self.root.join("the-other"), &self.other);
    }

    /// One work branch off the trunk, with the one commit a review would have
    /// read.
    fn a_branch(&self, branch: &str, file: &str, what: &str) -> String {
        git(
            &self.primary,
            &["checkout", "--quiet", "-b", branch, "main"],
        );
        write(&self.primary.join(file), &format!("{what}\n"));
        git(&self.primary, &["add", "--", file]);
        git(
            &self.primary,
            &["commit", "--quiet", "--no-gpg-sign", "-m", what],
        );
        git(
            &self.primary,
            &["push", "--quiet", "origin", &format!("HEAD:{branch}")],
        );
        let commit = git(&self.primary, &["rev-parse", branch]);
        assert_eq!(commit.len(), 40, "a commit is 40 hex: {commit}");
        git(&self.primary, &["checkout", "--quiet", "main"]);
        commit
    }

    /// The work graph and its tracked half on the trunk. IT HOLDS NO ITEMS: an
    /// arm's rig creates its own, and what this commit puts on the trunk is the
    /// export file a landing regenerates.
    fn a_board(&self, label: &str) {
        spawned("bd init");
        let init = Command::new("bd")
            .args(["init", "--prefix", "fx", "--quiet"])
            .args(common::bd_init_server_args(label))
            .current_dir(&self.primary)
            .output()
            .expect("bd is on the process PATH");
        assert!(init.status.success(), "bd init: {}", stderr(&init));
        let export = bd_in(
            &self.primary,
            &[
                "export",
                "-o",
                &self
                    .primary
                    .join(".beads/issues.jsonl")
                    .display()
                    .to_string(),
            ],
        );
        assert!(export.status.success(), "bd export: {}", stderr(&export));
        // `bd init` records the git origin it found as `sync.remote`, and this
        // one is the TEMPLATE's bare. It rides the store commit into every
        // rig's checkout, where nothing can rewrite it — the reviewer's tree
        // has to stay clean — so the line is dropped before it is committed.
        let config = self.primary.join(".beads/config.yaml");
        let kept: String = std::fs::read_to_string(&config)
            .expect("bd init wrote a config")
            .lines()
            .filter(|line| !line.contains(&self.root.display().to_string()))
            .map(|line| format!("{line}\n"))
            .collect();
        write(&config, &kept);
        git(&self.primary, &["add", "--", ".beads"]);
        git(
            &self.primary,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "the store"],
        );
        git(&self.primary, &["push", "--quiet", "origin", "HEAD:main"]);
    }

    /// The standing half of the copy's correctness, and the one a look at the
    /// directories only answers once: EVERY file a rig copies is asked whether
    /// it names this template — under the name it was built as AND the name it
    /// is renamed to — and the only ones allowed to are the two files
    /// [`rebased`] rewrites per rig. A path anywhere else is a rig reading the
    /// template's bare, its board or its seams while its own assertions pass.
    fn names_itself_nowhere_a_copy_would_carry(&self, public: &Path) {
        let rewritten = [self.primary.join(".git/config"), repo_state(&self.primary)];
        let mut named: Vec<(PathBuf, String)> = Vec::new();
        for file in files_under(&self.bare)
            .into_iter()
            .chain(files_under(&self.primary))
        {
            if rewritten.contains(&file) {
                continue;
            }
            let Ok(body) = std::fs::read(&file) else {
                continue;
            };
            for name in [&self.root, &public.to_path_buf()] {
                let name = name.display().to_string();
                if holds(&body, &name) {
                    named.push((file.clone(), name));
                }
            }
        }
        assert!(
            named.is_empty(),
            "these copied files name the template: {named:?}"
        );
    }
}

/// The fixture the whole run copies, built here if this process is the one that
/// finds it missing.
///
/// KEYED ON THE RUN AND NOT ON THE PROCESS. nextest gives every arm a process of
/// its own, so a `OnceLock` alone builds this once per ARM — strictly more work
/// than the single rig it replaced, which is a slower suite and not a faster
/// one. `NEXTEST_RUN_ID` names the run; a bare `cargo test` sets no such
/// variable and falls back to the process, which is the whole run there.
///
/// The build happens under a name private to this process and is `rename`d into
/// place, so a process that loses the race finds a COMPLETE tree and never a
/// half-built one.
fn built_once(name: &str, policy: &str) -> PathBuf {
    let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
    let root = temp.join(format!("fleet-cli-land-template-{name}-{}", a_run()));
    if root.join("bare").is_dir() {
        return root;
    }

    let mine = root.with_file_name(format!(
        "fleet-cli-land-template-{name}-{}-{}",
        a_run(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&mine);
    let mut building = Template {
        bare: mine.join("bare"),
        primary: mine.join("primary"),
        packs: mine.join("packs"),
        root: mine.clone(),
        commit: String::new(),
        other: String::new(),
    };
    building.build(policy, name);
    // The origin was written under the private name, which the rename below
    // takes away. Re-pointing it at the public one leaves the per-rig rewrite
    // one path to find and one to replace.
    rebased(&building.primary.join(".git/config"), &mine, &root);
    rebased_if_there(&repo_state(&building.primary), &mine, &root);
    building.names_itself_nowhere_a_copy_would_carry(&root);

    // A rename onto a directory another arm already put there refuses, which is
    // this arm losing the race and nothing worse.
    if std::fs::rename(&mine, &root).is_err() {
        let _ = std::fs::remove_dir_all(&mine);
    }
    assert!(
        root.join("bare").is_dir(),
        "the run's fixture is at {}",
        root.display()
    );
    root
}

/// What names this run: every arm process of one `cargo nextest run` answers
/// the same string.
fn a_run() -> String {
    std::env::var("NEXTEST_RUN_ID").unwrap_or_else(|_| std::process::id().to_string())
}

/// A commit the template wrote down, read back forty characters long.
fn a_sha(path: &Path) -> String {
    let sha = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
        .trim()
        .to_string();
    assert_eq!(sha.len(), 40, "a commit is 40 hex: {sha}");
    sha
}

/// Every file under a directory, the whole tree of it.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            found.extend(files_under(&entry.path()));
        } else {
            found.push(entry.path());
        }
    }
    found
}

/// Whether a file's bytes hold a phrase, text or not: the path being looked for
/// would be as damaging inside a database page as inside a config line.
fn holds(body: &[u8], phrase: &str) -> bool {
    body.windows(phrase.len())
        .any(|run| run == phrase.as_bytes())
}

// ---- the rig -----------------------------------------------------------------

struct Rig {
    root: PathBuf,
    bare: PathBuf,
    primary: PathBuf,
    /// The reviewer's checkout: a linked worktree of the primary.
    reviewer: PathBuf,
    machine: PathBuf,
    packs: PathBuf,
    item: String,
    /// The commit the review accepted.
    commit: String,
    /// The second item's commit, for the arm that lands two.
    other: String,
}

impl Rig {
    fn new(label: &str) -> Rig {
        Rig::of(Template::shared(), label)
    }

    /// A copy of the template, each piece copied the only way it can be: the
    /// repositories and the board by directory, with every file that names
    /// the template rewritten; the reviewer's checkout ADDED FRESH from this
    /// rig's own bare, because a linked worktree cannot be copied at all.
    fn of(template: &'static Template, label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-cli-land-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut rig = Rig {
            bare: root.join("bare"),
            primary: root.join("primary"),
            reviewer: root.join("reviewer"),
            machine: root.join("machine"),
            packs: template.packs.clone(),
            commit: template.commit.clone(),
            other: template.other.clone(),
            root,
            item: String::new(),
        };
        for dir in [&rig.machine, &rig.root.join("hooks")] {
            std::fs::create_dir_all(dir).expect("the fixture directory is made");
        }
        copy_tree(&template.bare, &rig.bare);
        copy_tree(&template.primary, &rig.primary);
        rebased(&rig.primary.join(".git/config"), &template.root, &rig.root);
        rebased_if_there(&repo_state(&rig.primary), &template.root, &rig.root);
        write(&rig.root.join("the-rc"), "0\n");
        write(&rig.root.join("the-marker-rc"), "0\n");
        write(&rig.root.join("the-sleep"), "0\n");

        rig.in_primary(&[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            &rig.reviewer.display().to_string(),
            "origin/main",
        ]);
        rig.item =
            rig.a_delivered_item("an item to land", WORK, "the-work.txt", &rig.commit.clone());
        rig
    }

    fn hooks(&self) -> String {
        self.root.join("hooks").display().to_string()
    }

    /// An item held by the reviewer, carrying the orders and the delivery note
    /// the verb reads. ONE `bd create`: every field it needs is one that call
    /// already takes, and a bd call on a served board is the cost this rig
    /// pays most of.
    fn a_delivered_item(&self, title: &str, branch: &str, file: &str, commit: &str) -> String {
        let delivery = format!(
            "DELIVERED {commit} — {BUILDER}\n\
             commit:  {commit}\n\
             branch:  {branch}\n\
             base:    origin/main at {commit}, fetched at 2026-09-12T00:00:00Z\n\
             files:   {file}\n\
             gate:    AC2 green, each rc read from its own command\n\
             suite:   the workspace suite, rc 0\n\
             spec corrections: none\n\
             not proven: what this arm did not run\n\
             decisions: none\n\
             covers: R8"
        );
        let made = bd_in(
            &self.primary,
            &[
                "create",
                "--title",
                title,
                "--description",
                "a scratch item",
                "--type",
                "task",
                "--assignee",
                REVIEWER,
                "--metadata",
                &format!(
                    r#"{{"orders": {{"by": "an-architect", "kind": "dispatch", "seat": "{BUILDER}", "at": "2026-09-12T00:00:00Z"}}}}"#
                ),
                "--notes",
                &delivery,
                "--actor",
                BUILDER,
                "--json",
            ],
        );
        assert!(made.status.success(), "bd create: {}", stderr(&made));
        let value: serde_json::Value =
            serde_json::from_str(stdout(&made).trim()).expect("bd create answers JSON");
        value["id"].as_str().expect("an id").to_string()
    }

    /// The ACCEPTED verdict, written by the SHIPPED verb rather than typed
    /// here: what land gates on is what `fleet review --land` actually writes.
    fn accepted(&self) {
        let reviewed = self.run(&["review", &self.item, "--land"]);
        assert_eq!(reviewed.status.code(), Some(0), "{}", stderr(&reviewed));
    }

    fn in_primary(&self, args: &[&str]) -> String {
        git(&self.primary, args)
    }

    fn in_reviewer(&self, args: &[&str]) -> String {
        git(&self.reviewer, args)
    }

    fn in_bare(&self, args: &[&str]) -> String {
        git(&self.bare, args)
    }

    fn bd(&self, args: &[&str]) -> Output {
        bd_in(&self.reviewer, args)
    }

    fn item_json(&self) -> serde_json::Value {
        let out = self.bd(&["-q", "show", &self.item, "--json"]);
        let value: serde_json::Value =
            serde_json::from_str(stdout(&out).trim()).expect("bd show answers JSON");
        value[0].clone()
    }

    fn notes(&self) -> String {
        self.item_json()["notes"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    /// Every event the machine directory's stream holds, newest last.
    fn events(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("an event is one JSON object"))
            .collect()
    }

    fn args<'a>(&'a self, args: &[&'a str], packs: &'a str) -> Vec<&'a str> {
        let mut all = args.to_vec();
        all.push("--packs-dir");
        all.push(packs);
        all
    }

    fn run(&self, args: &[&str]) -> Output {
        spawned(&format!("fleet {args:?}"));
        let packs = self.packs.display().to_string();
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(self.args(args, &packs))
            .current_dir(&self.reviewer)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env(SEAM, &self.root)
            .env("BEADS_ACTOR", REVIEWER)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .output()
            .expect("the built binary runs")
    }

    /// The same call on a pseudo-terminal, answered as the whole Output: the
    /// two streams are one there, which is the point — what a person sees is
    /// the combined page — and the STATUS is still the child's own, which
    /// `script` propagates (measured: a child exiting 7 answers 7).
    fn on_a_pty(&self, args: &[&str]) -> Output {
        spawned(&format!("fleet {args:?} (on a pty)"));
        let packs = self.packs.display().to_string();
        let all = self.args(args, &packs);
        let mut command = pty_command(env!("CARGO_BIN_EXE_fleet"), &all);
        let out = command
            .current_dir(&self.reviewer)
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env(SEAM, &self.root)
            .env("BEADS_ACTOR", REVIEWER)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .output()
            .expect("`script` allocates the pseudo-terminal these arms are about");
        out
    }

    fn land(&self) -> Output {
        self.run(&["land", &self.item, &self.commit])
    }

    /// A landing started and not waited on, its two streams into one file the
    /// arm can read WHILE it runs — which is the only way to watch a landing
    /// say it is waiting rather than to read it afterwards.
    fn spawn_land(&self, item: &str, commit: &str, into: &Path) -> std::process::Child {
        spawned(&format!(
            "fleet [\"land\", {item:?}, {commit:?}] (not waited on)"
        ));
        let packs = self.packs.display().to_string();
        let page = std::fs::File::create(into)
            .unwrap_or_else(|e| panic!("{} is opened: {e}", into.display()));
        let both = page.try_clone().expect("the page is shared with stderr");
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(self.args(&["land", item, commit], &packs))
            .current_dir(&self.reviewer)
            .stdout(std::process::Stdio::from(page))
            .stderr(std::process::Stdio::from(both))
            .hermetic(&self.root.join("home"), &self.machine, None)
            .env(SEAM, &self.root)
            .env("BEADS_ACTOR", REVIEWER)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .spawn()
            .expect("the built binary runs")
    }

    /// What the BARE holds on `main`. A landing is a claim about a remote, and
    /// this is the only thing that can answer it.
    fn bare_main(&self) -> String {
        self.in_bare(&["rev-parse", "main"])
    }

    fn bare_has(&self, reference: &str) -> bool {
        git_may_fail(&self.bare, &["rev-parse", "--verify", "--quiet", reference])
            .status
            .success()
    }

    fn has(&self, reference: &str) -> bool {
        git_may_fail(
            &self.reviewer,
            &["rev-parse", "--verify", "--quiet", reference],
        )
        .status
        .success()
    }

    /// The seam files live outside the repository, so setting either leaves the
    /// reviewer's tree exactly as clean as it was.
    fn suite_exits(&self, rc: &str) {
        write(&self.root.join("the-rc"), &format!("{rc}\n"));
    }

    fn suite_sleeps(&self, seconds: &str) {
        write(&self.root.join("the-sleep"), &format!("{seconds}\n"));
    }

    fn marker_exits(&self, rc: &str) {
        write(&self.root.join("the-marker-rc"), &format!("{rc}\n"));
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A copied repository's config, made to name this rig instead of the template
/// it came from: its origin and its hooks path are the only absolute paths a
/// copy carries, and a copy that kept them would push at the template's bare
/// while every assertion here went on passing.
fn rebased(config: &Path, from: &Path, to: &Path) {
    let text = std::fs::read_to_string(config)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", config.display()));
    let moved = text.replace(&from.display().to_string(), &to.display().to_string());
    assert_ne!(
        moved,
        text,
        "{} names no path of the template's, so a copy of it pushes at the template's own bare",
        config.display()
    );
    write(config, &moved);
}

/// The board's own record of where it fetches from, and the second absolute
/// path a copy carries — written only where the run named no dolt server and
/// `bd init` fell back to the embedded engine, which records the git origin it
/// found as a dolt remote. That origin is the TEMPLATE's bare. A served board
/// writes no such directory at all, so this file is present under one engine
/// and absent under the other, and the same tree answers differently under
/// `fleet/tools/dolt-test-server` than under a bare `cargo nextest run`.
fn repo_state(primary: &Path) -> PathBuf {
    primary.join(".beads/embeddeddolt/fx/.dolt/repo_state.json")
}

/// [`rebased`] for a file only one of the two board engines writes: absent
/// under a served board, and nothing to repoint when it is.
fn rebased_if_there(config: &Path, from: &Path, to: &Path) {
    if config.exists() {
        rebased(config, from, to);
    }
}

fn bd_in(root: &Path, args: &[&str]) -> Output {
    spawned(&format!("bd {args:?}"));
    Command::new("bd")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("bd runs")
}

fn write(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap_or_else(|e| panic!("{} is written: {e}", path.display()));
}

fn git_may_fail(dir: &Path, args: &[&str]) -> Output {
    spawned(&format!("git {args:?}"));
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "fleet tests")
        .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
        .env("GIT_COMMITTER_NAME", "fleet tests")
        .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
        .output()
        .expect("git runs")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_may_fail(dir, args);
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        stderr(&out)
    );
    stdout(&out).trim().to_string()
}

// ---- the green landing -------------------------------------------------------

/// The whole live path: the range line read from a real push, the bare's own
/// `main` moved to it, the note and the close on the item, and the work branch
/// gone from both sides.
#[test]
fn a_green_landing_moves_the_bare_and_closes_the_item() {
    let rig = Rig::new("green");
    rig.accepted();
    let before = rig.bare_main();

    let out = rig.land();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    // The landed sha, read off the verb's own last line, against what the BARE
    // says its trunk is now.
    let landed = stdout(&out)
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("LANDED ").map(str::to_string))
        .expect("the last line names the landed sha");
    // git's own push line prints the range ABBREVIATED, and this sha is read
    // off that line and nowhere else — so it is asked to resolve rather than
    // asserted forty characters long.
    assert!(
        (7..=40).contains(&landed.len()),
        "the landed sha is git's own abbreviation: {landed}"
    );
    assert_ne!(rig.bare_main(), before, "the trunk moved");
    assert_eq!(
        git(&rig.bare, &["rev-parse", &landed]),
        rig.bare_main(),
        "and it resolves, on the bare, to what the bare now calls main"
    );

    // The squash: one commit on the trunk, carrying the delivery's file and the
    // subject, the marker and the two trailers.
    // The delivered file, and beside it the ONE file this verb wrote itself:
    // the store's export, regenerated in the act and riding the same commit.
    assert_eq!(
        rig.in_bare(&["show", "--name-only", "--format=", "main"]),
        ".beads/issues.jsonl\nthe-work.txt",
        "the delivered file and the regenerated export, and nothing beside them"
    );
    assert!(
        rig.in_bare(&["show", "main:.beads/issues.jsonl"])
            .contains(&rig.item),
        "and the export that landed is one taken after the close"
    );
    let subject = rig.in_bare(&["log", "-1", "--format=%s", "main"]);
    assert!(
        subject.starts_with(&format!("{}: an item to land", rig.item))
            && subject.ends_with("[skip ci]"),
        "the subject is the item, its title and the marker: {subject}"
    );
    let body = rig.in_bare(&["log", "-1", "--format=%b", "main"]);
    assert!(
        body.contains(&format!("Seat: {REVIEWER}"))
            && body.contains(&format!("Implemented-by: {BUILDER}")),
        "both trailers: {body}"
    );

    // The record.
    let notes = rig.notes();
    assert!(
        notes.contains(&format!("LANDED {landed} on main by {REVIEWER}")),
        "the note's first line is on the item:\n{notes}"
    );
    assert!(
        notes.contains("suite: sh the-suite.sh, rc 0"),
        "and it names the suite the project declared:\n{notes}"
    );
    assert_eq!(rig.item_json()["status"], serde_json::json!("closed"));
    assert!(
        notes.contains(&format!("landed {landed}"))
            || rig.item_json()["close_reason"] == serde_json::json!(format!("landed {landed}")),
        "the close names the landed sha"
    );

    // THE EVENTS, off the stream the binary wrote. The accept this rig made in
    // `accepted()` is the first; this act adds the reading and then the landing,
    // in the order they happened.
    let events = rig.events();
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|event| event["type"].as_str())
        .collect();
    assert_eq!(
        kinds,
        vec!["item.reviewed", "gate.read", "item.landed"],
        "the accept, the reading, the landing: {events:?}"
    );
    let reading = &events[1]["payload"];
    let landing = &events[2]["payload"];
    assert_eq!(reading["item"].as_str(), Some(rig.item.as_str()));
    assert_eq!(reading["suite"].as_str(), Some("sh the-suite.sh"));
    assert_eq!(reading["rc"], serde_json::json!(0));
    assert_eq!(reading["verdict"].as_str(), Some("green"));
    assert_eq!(reading["reading"], serde_json::json!(1));
    assert_eq!(events[2]["actor"].as_str(), Some(REVIEWER));
    assert_eq!(landing["sha"].as_str(), Some(landed.as_str()));
    assert_eq!(
        landing["squash_of"].as_str(),
        Some(rig.commit.as_str()),
        "the reviewed commit, and not the one the trunk now carries: {events:?}"
    );
    // The old side is the range line's own, which git prints ABBREVIATED — so it
    // is asked to resolve rather than compared forty characters long.
    assert_eq!(
        rig.in_bare(&["rev-parse", landing["base"].as_str().expect("a base")]),
        before,
        "and the old side of the push's own range line: {events:?}"
    );

    // The work branch, gone from both sides on SAFE.
    assert!(
        !rig.bare_has(&format!("refs/heads/{WORK}")),
        "the work branch is gone from the bare"
    );
    assert!(!rig.has(WORK), "and locally");
    // The checkout is put back: detached on the trunk, with no land branch.
    assert!(
        !rig.has(&format!("land/{}", rig.item)),
        "the land branch is gone"
    );
    assert_eq!(
        rig.in_reviewer(&["status", "--porcelain"]),
        "",
        "and the tree is clean"
    );
}

/// A refusal at the staged-set gate leaves a real tree where it found it, and
/// the SAME call run again lands.
///
/// Both halves are one arm because they are one question: a refusal that left
/// the squash staged, or that left the board it rewrote dirty, is one no re-run
/// can get past — and the first thing a reviewer does after a refusal is run it
/// again.
#[test]
fn a_refusal_puts_the_tree_back_and_the_re_run_lands() {
    let rig = Rig::new("wedge");
    rig.accepted();
    let before = rig.bare_main();
    let clean = rig.in_reviewer(&["rev-parse", "HEAD"]);

    // A MARKER THAT WILL NOT ANSWER, which is a refusal at (f): the one window
    // where the squash is STAGED and not yet committed, and the export has
    // already been rewritten. A later refusal has nothing in the index to put
    // back, so it could not tell a reset from a detach.
    rig.marker_exits("3");
    let out = rig.land();
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("the-marker.sh"),
        "it refused at the marker, between the squash and the commit:\n{}",
        stderr(&out)
    );

    // THE TREE IS WHERE IT WAS: nothing staged, nothing left over, back on the
    // trunk, and no land branch. This is the half a detach alone cannot do —
    // by the time a squash exists HEAD already names the trunk.
    assert_eq!(
        rig.in_reviewer(&["status", "--porcelain"]),
        "",
        "the squash and the rewritten board are both put back"
    );
    assert_eq!(
        rig.in_reviewer(&["diff", "--cached", "--name-only"]),
        "",
        "and the index is empty"
    );
    assert_eq!(
        rig.in_reviewer(&["rev-parse", "HEAD"]),
        clean,
        "and the checkout is back where it started"
    );
    assert!(
        !rig.has(&format!("land/{}", rig.item)),
        "with no land branch"
    );
    assert_eq!(rig.bare_main(), before, "and nothing landed");

    // AND THE RE-RUN LANDS. This is the half a wedged gate fails: the export
    // the refused run rewrote is exactly what (b) would refuse next time.
    rig.marker_exits("0");
    let out = rig.land();
    assert_eq!(
        out.status.code(),
        Some(0),
        "the re-run after a refusal lands:\n{}\n{}",
        stdout(&out),
        stderr(&out)
    );
    assert_ne!(rig.bare_main(), before);
    assert_eq!(rig.item_json()["status"], serde_json::json!("closed"));
}

/// An `--also` path rides the landing and does NOT make the classification
/// read CARRIES: the branch is deleted on both sides, which is the shape this
/// fleet's own landings take.
#[test]
fn an_also_path_rides_the_landing_and_the_branch_is_still_deleted() {
    let rig = Rig::new("also");
    rig.accepted();
    write(
        &rig.reviewer.join("the-log.md"),
        "the reviewer's own line\n",
    );

    let out = rig.run(&[
        "land",
        &rig.item,
        &rig.commit,
        "--also",
        "the-log.md",
        "--reason",
        "the gate is the owner's",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("6. work branch      SAFE"),
        "an admitted path is not unlanded work:\n{}",
        stdout(&out)
    );
    assert_eq!(
        rig.in_bare(&["show", "--name-only", "--format=", "main"]),
        ".beads/issues.jsonl\nthe-log.md\nthe-work.txt",
        "the admitted path rode the landing commit"
    );
    assert!(
        !rig.bare_has(&format!("refs/heads/{WORK}")) && !rig.has(WORK),
        "and the branch is gone from both sides"
    );
    assert!(
        rig.notes().contains("plus --also the-log.md"),
        "the staged-set row names what was admitted:\n{}",
        rig.notes()
    );
}

// ---- the refusals ------------------------------------------------------------

/// A red suite is exit 1 with the row and the log's path printed, and the bare
/// is exactly where it was.
#[test]
fn a_red_suite_refuses_and_leaves_the_bare_where_it_was() {
    let rig = Rig::new("red-suite");
    rig.accepted();
    rig.suite_exits("3");
    let before = rig.bare_main();

    let out = rig.land();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let page = stdout(&out);
    assert!(
        page.contains("4. suite            RED"),
        "the suite row is printed red:\n{page}"
    );
    assert!(page.contains("suite.log"), "and the log's path:\n{page}");
    assert!(
        page.contains("the suite ran"),
        "with the log's own tail under it:\n{page}"
    );
    assert!(
        stderr(&out).contains("exited 3"),
        "the rc is the child's own: {}",
        stderr(&out)
    );

    assert_eq!(rig.bare_main(), before, "the trunk did not move");
    assert_eq!(
        rig.item_json()["status"],
        serde_json::json!("open"),
        "and the item is open"
    );
    // The tree is put back: detached on the trunk, no land branch.
    assert!(
        !rig.has(&format!("land/{}", rig.item)),
        "the land branch is gone"
    );
    // Both halves of "detached on the trunk": the commit it points at, and
    // that it points at a commit and not at a branch. A sha equality alone is
    // true of a checkout sitting on a branch at the same place.
    assert!(
        !git_may_fail(&rig.reviewer, &["symbolic-ref", "-q", "HEAD"])
            .status
            .success(),
        "the checkout is detached and not on a branch"
    );
    assert_eq!(
        rig.in_reviewer(&["rev-parse", "HEAD"]),
        rig.in_reviewer(&["rev-parse", "origin/main"]),
        "and the checkout is back on the trunk"
    );
}

/// A trunk somebody else moved DURING the act is REBASE NEEDED.
///
/// During, and not before: the land branch is cut at the trunk this act
/// fetched, so a commit pushed beforehand is one this landing squashes onto and
/// nothing to rebase against. The window the gate exists for is the one between
/// that fetch and the push, and the suite is what holds it open.
#[test]
fn a_trunk_moved_during_the_act_is_rebase_needed() {
    let rig = Rig::new("moved");
    rig.accepted();

    let second = rig.root.join("second");
    git(
        &rig.root,
        &[
            "clone",
            "--quiet",
            &rig.bare.display().to_string(),
            &second.display().to_string(),
        ],
    );
    git(&second, &["config", "core.hooksPath", &rig.hooks()]);
    write(&second.join("elsewhere.txt"), "somebody else's work\n");
    git(&second, &["add", "--", "elsewhere.txt"]);
    write(
        &rig.root.join("the-during"),
        &format!(
            "#!/bin/sh\n\
             cd '{second}'\n\
             git -c user.email=f@example.invalid -c user.name=f commit -q --no-gpg-sign -m elsewhere\n\
             git push -q origin HEAD:main\n",
            second = second.display()
        ),
    );
    let before = rig.bare_main();

    let out = rig.land();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stdout(&out).contains("REBASE NEEDED"), "{}", stdout(&out));

    let moved = rig.bare_main();
    assert_ne!(
        moved, before,
        "somebody else's commit is what moved the trunk"
    );
    assert_eq!(
        rig.in_bare(&["log", "-1", "--format=%s", "main"]),
        "elsewhere",
        "and it stands alone: this landing added nothing"
    );
    assert_eq!(rig.item_json()["status"], serde_json::json!("open"));
    assert!(!rig.notes().contains("LANDED "), "and wrote no note");
}

/// A remote that rejects the push: exit 1, the remote's own words printed, and
/// nothing after it runs.
#[test]
fn a_rejected_push_prints_the_remotes_words_and_writes_nothing() {
    let rig = Rig::new("rejected");
    rig.accepted();
    let hook = rig.bare.join("hooks/pre-receive");
    write(
        &hook,
        "#!/bin/sh\nprintf 'this remote is closed for the evening\\n' >&2\nexit 1\n",
    );
    make_executable(&hook);
    let before = rig.bare_main();

    let out = rig.land();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("closed for the evening"),
        "the remote's own words reach the page:\n{}",
        stdout(&out)
    );
    assert_eq!(rig.bare_main(), before, "nothing landed");
    assert_eq!(rig.item_json()["status"], serde_json::json!("open"));
    assert!(
        !rig.notes().contains("LANDED "),
        "and no note was written:\n{}",
        rig.notes()
    );
}

// ---- the ui (the ruling carried) ---------------------------------------------

/// The bar is stderr's and the gate table is stdout's (the ruling carried).
///
/// The reading that separates them is the TERMINAL and not the clock: a
/// landing against a real remote outlasts the ui module's two-second threshold
/// whatever its suite does — measured, a suite exiting at once still drew the
/// bar — so the control here is the same landing on a pipe, where the module's
/// own rule says nothing is drawn. That the threshold itself is a rule rather
/// than a silence is `src/ui.rs`'s own arms and `tests/ui.rs`'s spinner pair.
#[test]
fn the_bar_is_drawn_on_a_terminal_and_never_on_a_pipe() {
    let Some(()) = script_is_here() else {
        eprintln!(
            "the_bar_is_drawn_on_a_terminal_and_never_on_a_pipe: SKIPPED — script(1) is not on \
             this box, and the bar can only be read on a real pseudo-terminal. This run proves \
             nothing about the bar."
        );
        return;
    };

    let rig = Rig::new("bar");
    rig.accepted();
    // Two seconds, and the assertion below is why: the suite writes a line a
    // second, and the bar has to be read on two DIFFERENT counts.
    rig.suite_sleeps("2");

    let out = rig.on_a_pty(&["land", &rig.item, &rig.commit]);
    // The child's own status, which `script` hands back — not inferred from
    // anything the page says (R10).
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    let page = stdout(&out);
    assert!(page.contains("LANDED "), "the landing succeeded:\n{page}");
    assert!(
        page.contains("/7 "),
        "the bar is bounded by the gate rows the note renders:\n{page}"
    );

    // THE COUNT GREW, which is what "as it grows" means and what a single read
    // cannot produce: the suite writes a line a second for two seconds, so the
    // page has to carry at least two DIFFERENT counts, and the last line the
    // suite writes has to be among them or the bar stopped refreshing early.
    let counts: Vec<usize> = (0..=3)
        .filter(|n| page.contains(&format!("— {n} line(s)")))
        .collect();
    assert!(
        counts.len() >= 2,
        "the bar's message was read more than once: counts seen {counts:?} in:\n{page}"
    );
    assert!(
        counts.contains(&2),
        "and it kept reading to the suite's last line: counts seen {counts:?}"
    );

    // The control: the same act, the same suite, on a pipe. Nothing is drawn,
    // so what the arm above measured is the terminal.
    let piped = Rig::new("bar-piped");
    piped.accepted();
    // The control answers the arm above only if its suite ran as long as that
    // one's did, so it is timed the same and for the same reason.
    piped.suite_sleeps("2");
    let out = piped.land();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stderr(&out), "", "a pipe gets no bar at all");
    assert!(!stdout(&out).contains("line(s)"), "{}", stdout(&out));
}

/// Whether this box has the tool the arm above needs. A missing one is said
/// out loud rather than passed over: a green with no subject is worse than a
/// skip somebody can read.
fn script_is_here() -> Option<()> {
    Command::new("sh")
        .arg("-c")
        .arg("command -v script")
        .output()
        .ok()
        .filter(|out| out.status.success() && !stdout(out).trim().is_empty())
        .map(|_| ())
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .expect("the file is there")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("the file is made executable");
}

// ---- the lane (flights PRD R15, S3b) -----------------------------------------

/// A second item on its own branch, its own file and its own accepted verdict,
/// so two landings can be asked for at once.
///
/// Its file is not the first item's: what this arm is about is the QUEUE, and a
/// squash conflict would end both landings before the lock was ever read. The
/// branch is the template's, pushed to this rig's own bare with everything else
/// it copied.
fn a_second_item(rig: &Rig) -> (String, String) {
    let commit = rig.other.clone();
    let item = rig.a_delivered_item("a second item to land", OTHER, "the-other.txt", &commit);
    let reviewed = rig.run(&["review", &item, "--land"]);
    assert_eq!(reviewed.status.code(), Some(0), "{}", stderr(&reviewed));
    (item, commit)
}

/// TWO LANDINGS ASKED FOR AT ONCE QUEUE ON THE LANE, and both land in order
/// (R15, S3b).
///
/// THE WAIT IS SIZED TO THE LOCK AND NOT TO A CLOCK. The arm holds the first
/// landing inside its suite until a file appears, watches the LOCK FILE for the
/// first landing's own holder line, and watches the second landing's stdout for
/// the waiting line — then releases. Nothing here is a budget in seconds that a
/// busy box can run past.
///
/// THAT THE LOCK IS WHAT SERIALISED THEM is not a guess: both children run in
/// the SAME reviewer checkout, which is one index and one HEAD. Without the
/// lock the second would cut its land branch under the first's squash, and no
/// ordering of two green landings on the bare would be possible at all.
#[test]
fn two_landings_at_once_queue_on_the_lane_and_land_in_order() {
    let rig = Rig::new("lane-queue");
    rig.accepted();
    let (second, second_commit) = a_second_item(&rig);
    let before = rig.bare_main();

    // The first landing holds inside its suite until the arm says otherwise.
    write(&rig.root.join("the-hold"), "hold\n");
    let first_out = rig.root.join("first.out");
    let second_out = rig.root.join("second.out");
    let mut first = rig.spawn_land(&rig.item, &rig.commit, &first_out);

    // It holds the lane once its lock file names it. This is the reading that
    // says the first landing is the holder, and it is the lock's own.
    let lock = rig
        .machine
        .join("lanes")
        .join(format!("lane-{}.lock", project_name(&rig)));
    await_in(&lock, &rig.item, "the first landing takes the lane");

    let mut queued = rig.spawn_land(&second, &second_commit, &second_out);
    // And the second says it is waiting on it, by name.
    await_in(
        &second_out,
        "waiting on the lane:",
        "the second landing waits",
    );
    let waiting = std::fs::read_to_string(&second_out).unwrap_or_default();
    assert!(
        waiting.contains(&rig.item),
        "the waiting line names the holder:\n{waiting}"
    );

    write(&rig.root.join("the-release"), "go\n");
    let first = first.wait().expect("the first landing ends");
    let queued = queued.wait().expect("the second landing ends");
    let said = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    assert_eq!(
        first.code(),
        Some(0),
        "the first landed:\n{}",
        said(&first_out)
    );
    assert_eq!(
        queued.code(),
        Some(0),
        "the second landed:\n{}",
        said(&second_out)
    );

    // BOTH ON THE BARE'S MAIN, IN ORDER. The remote is the only thing that can
    // answer whether two landings both reached the trunk.
    let subjects = rig.in_bare(&["log", "--format=%s", "main"]);
    let reached: Vec<&str> = subjects.lines().take(2).collect();
    assert!(
        reached[0].starts_with(&second) && reached[1].starts_with(&rig.item),
        "the holder's landing is under the one that queued behind it:\n{subjects}"
    );
    assert_ne!(rig.bare_main(), before, "and the trunk moved");
    assert_eq!(
        rig.in_bare(&["rev-list", "--count", &format!("{before}..main")]),
        "2",
        "twice, once per landing"
    );
}

/// The project this rig resolves to, which is the reviewer checkout's own
/// directory name: the fixture declares no `[project] name`.
fn project_name(rig: &Rig) -> String {
    rig.reviewer
        .file_name()
        .expect("the checkout has a name")
        .to_string_lossy()
        .into_owned()
}

/// Wait until a file holds a phrase. THE DEADLINE IS A HANG DETECTOR AND NOT A
/// BUDGET: what is waited for is a lock taken inside an act already running,
/// and the one arm that waits on it cleared this bound on a box measured at
/// load 11. An arm that needs longer is a finding to report, not a bound to
/// raise.
fn await_in(path: &Path, phrase: &str, what: &str) {
    let started = Instant::now();
    loop {
        if std::fs::read_to_string(path)
            .unwrap_or_default()
            .contains(phrase)
        {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{what}: `{phrase}` never reached {} — thirty seconds is not a budget, it is the \
             point past which this is a hang and not a slow box",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ---- the JSON envelope (cli PRD § The JSON envelope; MVP path row 2) ---------

/// Stdout as the ONE document it is under `--json`: the flag's whole promise to
/// a caller is that the stream can be parsed rather than searched.
fn one_document(out: &Output) -> serde_json::Value {
    let page = stdout(out);
    let mut lines = page.lines();
    let first = lines
        .next()
        .unwrap_or_else(|| panic!("stdout carries a document: {page:?}"));
    assert_eq!(lines.next(), None, "and nothing else on stdout: {page:?}");
    serde_json::from_str(first).unwrap_or_else(|e| panic!("the document parses: {e}: {first}"))
}

/// `land --json`: the sha the push's own range line named, which no other verb
/// produces, and the gate table on the other stream.
#[test]
fn a_landing_under_json_prints_the_sha_the_push_named_and_the_table_on_stderr() {
    let rig = Rig::new("json");
    rig.accepted();

    let out = rig.run(&["land", &rig.item, &rig.commit, "--json"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    let document = one_document(&out);
    assert_eq!(document["ok"], serde_json::json!(true), "{document}");
    assert_eq!(document["verb"], serde_json::json!("land"), "{document}");
    let data = &document["data"];
    assert_eq!(data["item"], serde_json::json!(rig.item), "{data}");
    assert_eq!(data["state"], serde_json::json!("landed"), "{data}");

    // The document's sha against what the BARE says its trunk is now: only the
    // remote can answer the claim a landing makes.
    let sha = data["sha"].as_str().expect("the document names a sha");
    assert_eq!(
        git(&rig.bare, &["rev-parse", sha]),
        rig.bare_main(),
        "the sha the document carries is the trunk the bare now holds: {data}"
    );

    let page = stderr(&out);
    assert!(
        page.contains(&format!("LANDED {sha}")),
        "the line a person reads is still said, on the other stream:\n{page}"
    );
}

/// The refusal shape and the exit code the verb always had: a red suite is
/// exit 1 with the flag and without it, and the trunk does not move either way.
#[test]
fn a_refused_landing_under_json_prints_the_refusal_shape_and_the_same_exit_code() {
    let rig = Rig::new("json-refused");
    rig.accepted();
    rig.suite_exits("3");
    let before = rig.bare_main();

    let out = rig.run(&["land", &rig.item, &rig.commit, "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    let document = one_document(&out);
    assert_eq!(document["ok"], serde_json::json!(false), "{document}");
    assert_eq!(document["verb"], serde_json::json!("land"), "{document}");
    assert_eq!(
        document["refusal"]["code"],
        serde_json::json!("refused"),
        "the exit table's row by name, not its number: {document}"
    );
    let why = document["refusal"]["why"]
        .as_str()
        .expect("the refusal says why");
    assert!(
        stderr(&out).contains(why),
        "and says the same sentence on stderr: {}",
        stderr(&out)
    );

    assert_eq!(rig.bare_main(), before, "the trunk did not move");
}
