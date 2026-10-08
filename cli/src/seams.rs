//! core never depends on the controller — the workspace test refuses that edge
//! — so the ring, which reaches the provider adapter, meets here, and the
//! spawner, which reaches the controller's transient-seat primitives, meets in
//! `transient.rs`. git meets here too: core states the operations, and this
//! crate answers them over `fleet_core::process::git`. Every git the shipped
//! code runs by bare name is built by `fleet_core::process::git_command`; the
//! one git it runs otherwise is the agent conformance's live `git init`, a
//! resolved path on the constructed `PATH`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fleet_controller::effect::{TurnTarget, Typed};
use fleet_controller::project::Here;
use fleet_controller::{config, effect, platform, policy as controller, sessions};
use fleet_core::agent;
use fleet_core::item::land::{LandGit, Progress, Pushed, Squashed};
use fleet_core::item::{
    numstat_line, status_entries, Change, Git, Ring, RingOutcome, StatusLine, TRUNK,
};

use crate::ui::{Ui, Wait};

// ---- the two seams ----------------------------------------------------------

/// The ring: one turn typed into the seat's own session, through the same path
/// the controller's own nudge takes (`effect::type_turn`).
pub(crate) struct SeatRing {
    pub(crate) machine_dir: PathBuf,
}

/// What the ring's body knows beyond the outcome: the live session it reached.
///
/// Only the roster read inside the body has the session id, and the courier
/// verb's event names it — so the body hands it back rather than leaving a
/// second reader to take the same reading against a roster that has moved.
pub(crate) struct Rung {
    pub(crate) session: Option<String>,
    pub(crate) typed: Typed,
}

fn rang_nobody(cause: String) -> Rung {
    Rung {
        session: None,
        typed: Typed::Failed(cause),
    }
}

impl Ring for SeatRing {
    /// A ring's three answers out of the typed turn's five. A turn QUEUED
    /// behind the one the seat is holding is never called delivered (reviewer
    /// call 2026-09-25, E5), and a refusal at a dialog is a ring that did not
    /// land: each is a failed ring carrying what it was.
    fn ring(&self, seat: &str, text: &str) -> RingOutcome {
        match self.ring_with(seat, text, None).typed {
            Typed::Delivered => RingOutcome::Delivered,
            Typed::Absent => RingOutcome::Absent,
            Typed::Failed(cause) => RingOutcome::Failed(cause),
            other => RingOutcome::Failed(other.recorded()),
        }
    }
}

impl SeatRing {
    /// The ring over the machine directory `here` resolved.
    pub(crate) fn of(here: &Here) -> SeatRing {
        SeatRing {
            machine_dir: here.machine_dir.clone(),
        }
    }

    /// The one path both callers take: the row lookup, the seat's recorded
    /// configuration directory, and the typed turn into its session.
    ///
    /// `timeout` stands in for the policy's bound on this call alone.
    pub(crate) fn ring_with(&self, seat: &str, text: &str, timeout: Option<Duration>) -> Rung {
        let machine = match config::read(&config::path_in(&self.machine_dir)) {
            Ok(machine) => machine,
            Err(cause) => return rang_nobody(cause),
        };
        // THE ROW THROUGH THE RESOLVER, so a ring names its seat the way every
        // other seat argument does. From here the session table is asked by the
        // seat's id, and the host by the session that id names.
        let row = match machine.resolve(seat) {
            Ok(row) => row,
            Err(unresolved) => return rang_nobody(unresolved.to_string()),
        };
        let key = row.id.to_string();
        let policy = match controller::load(&machine.fleet_toml) {
            Ok(policy) => policy,
            Err(cause) => return rang_nobody(cause),
        };

        let home = platform::home_dir();
        // The agent opened the one way every caller opens it. A ring reads
        // and types and starts nothing, so it is asked whatever the effects
        // gate says: the typed turn is the host's act.
        let setting = match agent::Setting::read(&machine.fleet_toml, &self.machine_dir) {
            Ok(setting) => setting,
            Err(cause) => return rang_nobody(cause),
        };
        let search_path = platform::child_path(&home);
        let agent = match agent::open(&setting.opening(&search_path)) {
            Ok(opened) => opened.agent,
            Err(cause) => return rang_nobody(cause),
        };
        let host = fleet_controller::host::resolve(&search_path);
        // A spawned seat's session runs under the configuration directory
        // that seat alone starts with, and is named by no other listing, so
        // the ring reads under the directory that seat's own session row
        // recorded.
        let table = sessions::read(&sessions::path_in(&self.machine_dir)).0;
        let config_dir = table
            .as_ref()
            .and_then(|table| table.newest_for(&key))
            .and_then(|row| row.config_dir.clone());
        let target = TurnTarget {
            seat: &row.id,
            config_dir: config_dir.as_deref().map(Path::new),
        };
        // The session the courier's event names, off the row carrying the
        // seat's pane's pid; the turn itself reads both again, fresh.
        let session = match effect::seat_row(&agent, host.as_ref(), &target) {
            Ok(live) => live.reading.session_id,
            Err(typed) => {
                return Rung {
                    session: None,
                    typed,
                }
            }
        };
        let typed = effect::type_turn(
            &agent,
            host.as_ref(),
            &target,
            text,
            timeout.unwrap_or_else(|| Duration::from_secs(policy.nudge_timeout_seconds)),
        );
        Rung { session, typed }
    }
}

/// The box's five-minute load against the belt's own ceiling, for the one wait a
/// suite check's rerun takes.
///
/// The ceiling is `[dispatch] load_ceiling_per_cpu` times the processor count,
/// which is the same arithmetic the spawn belt does — one number, read twice,
/// rather than two that can disagree. A fleet whose policy would not read
/// answers `None` on every reading, which is a leg nobody could judge and makes
/// the rerun run without waiting.
pub(crate) struct BoxLoad {
    ceiling_per_cpu: Option<f64>,
}

impl BoxLoad {
    pub(crate) fn of(here: &Here) -> BoxLoad {
        BoxLoad {
            ceiling_per_cpu: fleet_controller::project::wiring::policy_of(here)
                .ok()
                .map(|policy| policy.load_ceiling_per_cpu),
        }
    }
}

impl fleet_core::item::lane::Load for BoxLoad {
    fn read(&self) -> Option<(f64, f64)> {
        let per_cpu = self.ceiling_per_cpu?;
        let readings = fleet_controller::transient::Readings::taken();
        let cpus = readings.cpus.filter(|n| *n > 0)?;
        Some((readings.load?, f64::from(cpus) * per_cpu))
    }
}

/// git, run in the project root, one call per operation.
///
/// A non-zero exit is a refusal naming the step and what git said, never a
/// value rounded to a default: a verb that read "no branch" as the trunk would
/// refuse a delivery for a reason that was never true.
pub(crate) struct RealGit {
    root: PathBuf,
}

impl RealGit {
    /// git in the root `here` resolved to, where every verb's git runs.
    pub(crate) fn of(here: &Here) -> RealGit {
        RealGit {
            root: here.project.root.clone(),
        }
    }

    fn run(&self, step: &str, args: &[&str]) -> Result<String, String> {
        fleet_core::process::git(&self.root, step, args)
    }

    /// One call whose STATUS is part of the answer rather than a failure: the
    /// caller reads both halves. Only a git that could not be RUN is an error.
    fn attempt(&self, args: &[&str]) -> Result<std::process::Output, String> {
        fleet_core::process::git_attempt(&self.root, args)
    }

    fn lines(&self, step: &str, args: &[&str]) -> Result<Vec<String>, String> {
        Ok(self
            .run(step, args)?
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect())
    }

    /// One `-z` read: every NUL-ended path, as git holds it.
    fn names(&self, step: &str, args: &[&str]) -> Result<Vec<String>, String> {
        Ok(self
            .run(step, args)?
            .split('\0')
            .filter(|field| !field.is_empty())
            .map(str::to_string)
            .collect())
    }
}

impl Git for RealGit {
    fn current_branch(&self) -> Result<String, String> {
        let name = self
            .run(
                "rev-parse --abbrev-ref HEAD",
                &["rev-parse", "--abbrev-ref", "HEAD"],
            )?
            .trim()
            .to_string();
        if name == "HEAD" {
            return Err(String::from(
                "the worktree is on a detached HEAD and not on a branch — a delivery is a handoff \
                 of a work branch",
            ));
        }
        Ok(name)
    }

    fn head(&self) -> Result<String, String> {
        Ok(self
            .run("rev-parse HEAD", &["rev-parse", "HEAD"])?
            .trim()
            .to_string())
    }

    fn trunk_tip(&self) -> Result<String, String> {
        Ok(self
            .run(&format!("rev-parse {TRUNK}"), &["rev-parse", TRUNK])?
            .trim()
            .to_string())
    }

    fn staged(&self) -> Result<Vec<String>, String> {
        self.names(
            "diff --cached --name-only",
            &["diff", "--cached", "--name-only", "-z"],
        )
    }

    fn status(&self) -> Result<Vec<StatusLine>, String> {
        status_entries(&self.run("status --porcelain", &["status", "--porcelain", "-z"])?)
    }

    /// `git add -A`, which is the one spelling that takes the untracked file
    /// and the deletion together — `-u` leaves the first and a bare pathspec
    /// leaves the second.
    fn add_all(&self) -> Result<(), String> {
        self.run("add -A", &["add", "-A", "--"]).map(|_| ())
    }

    fn commit(&self, message: &str) -> Result<String, String> {
        self.run(
            "commit",
            &["commit", "--quiet", "--no-gpg-sign", "-m", message],
        )?;
        self.head()
    }

    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        Ok(self
            .lines(
                "diff --numstat",
                &["diff", "--numstat", &format!("{from}...{to}")],
            )?
            .iter()
            .filter_map(|line| numstat_line(line))
            .collect())
    }
}

/// The git LAND reads and writes through, beside the operations every verb
/// shares. Same binary, same root, same disabled prompt.
impl LandGit for RealGit {
    /// A linked worktree's git dir is not its common dir. The primary's are the
    /// same path, which is what tells the two apart without naming either.
    fn at(&self, root: &Path) -> Box<dyn LandGit + '_> {
        Box::new(RealGit {
            root: root.to_path_buf(),
        })
    }

    fn is_linked_worktree(&self) -> Result<bool, String> {
        let git_dir = self.run(
            "rev-parse --git-dir",
            &["rev-parse", "--path-format=absolute", "--git-dir"],
        )?;
        let common = self.run(
            "rev-parse --git-common-dir",
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        Ok(git_dir.trim() != common.trim())
    }

    fn fetch(&self, remote: &str) -> Result<(), String> {
        self.run("fetch", &["fetch", remote]).map(|_| ())
    }

    fn branch_at(&self, branch: &str, at: &str) -> Result<(), String> {
        self.run("checkout -B", &["checkout", "--quiet", "-B", branch, at])
            .map(|_| ())
    }

    /// The conflict is PUT BACK before this answers. `--squash` sets no
    /// MERGE_HEAD, so `merge --abort` has nothing to abort and the hard reset
    /// behind it is what actually empties the index — which is why the caller
    /// restores its `--also` paths from the copies it took beforehand.
    fn squash_merge(&self, commit: &str) -> Result<Squashed, String> {
        let merged = self.attempt(&["merge", "--squash", commit])?;
        if merged.status.success() {
            return Ok(Squashed::Done);
        }
        let conflicted = self.names(
            "diff --diff-filter=U",
            &["diff", "--name-only", "-z", "--diff-filter=U"],
        )?;
        let _ = self.attempt(&["merge", "--abort"]);
        let _ = self.attempt(&["reset", "--quiet", "--hard", "HEAD"]);
        Ok(Squashed::Conflicted(conflicted))
    }

    fn add(&self, paths: &[String]) -> Result<(), String> {
        let mut args = vec!["add", "--"];
        args.extend(paths.iter().map(String::as_str));
        self.run("add", &args).map(|_| ())
    }

    fn changed_since_merge_base(&self, base: &str, commit: &str) -> Result<Vec<String>, String> {
        self.names(
            "diff --name-only <base>...<commit>",
            &["diff", "--name-only", "-z", &format!("{base}...{commit}")],
        )
    }

    /// An empty restriction is an empty diff, and never the whole tree: a
    /// pathspec-less `git diff` answers about everything, which is the opposite
    /// of what "restricted to no path" means.
    fn diff_paths(&self, from: &str, to: &str, paths: &[String]) -> Result<Vec<String>, String> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["diff", "--name-only", "-z", from, to, "--"];
        args.extend(paths.iter().map(String::as_str));
        self.names("diff --name-only <from> <to> -- <paths>", &args)
    }

    fn commit_message_file(&self, message: &Path) -> Result<String, String> {
        self.run(
            "commit -F",
            &[
                "commit",
                "--quiet",
                "--no-gpg-sign",
                "-F",
                &message.to_string_lossy(),
            ],
        )?;
        self.head()
    }

    fn behind(&self, what: &str, of: &str) -> Result<u64, String> {
        let span = format!("{what}..{of}");
        let counted = self.run("rev-list --count", &["rev-list", "--count", &span])?;
        counted.trim().parse::<u64>().map_err(|e| {
            format!(
                "`git rev-list --count {span}` answered `{}`, which is not a count: {e}",
                counted.trim()
            )
        })
    }

    /// A rejected push is a VALUE here and not an error: the caller prints what
    /// the remote said, and a push that ran is a push that may have landed.
    /// Both streams are carried because git writes the range line to stderr.
    fn push_head(&self, remote: &str, branch: &str) -> Result<Pushed, String> {
        let pushed = self.attempt(&["push", remote, &format!("HEAD:{branch}")])?;
        let mut output = String::from_utf8_lossy(&pushed.stdout).into_owned();
        output.push_str(&String::from_utf8_lossy(&pushed.stderr));
        Ok(Pushed {
            output,
            code: pushed.status.code(),
        })
    }

    fn rev(&self, rev: &str) -> Result<Option<String>, String> {
        let asked = self.attempt(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ])?;
        if !asked.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&asked.stdout).trim().to_string(),
        ))
    }

    /// `--` before the name, because the name comes off the record: a value
    /// beginning with `-` would otherwise be read as an option by a call whose
    /// whole job is to delete something.
    fn delete_branch(&self, branch: &str) -> Result<(), String> {
        self.run("branch -D", &["branch", "--quiet", "-D", "--", branch])
            .map(|_| ())
    }

    /// ASK BEFORE DELETING, and read the answer as a NUMBER: `ls-remote
    /// --exit-code` exits 2 for a ref the remote does not have and 128 for a
    /// remote it could not reach, so "never pushed" is told from "could not
    /// look" without parsing git's prose.
    fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<(), String> {
        let looked = self.attempt(&[
            "ls-remote",
            "--exit-code",
            remote,
            &format!("refs/heads/{branch}"),
        ])?;
        match looked.status.code() {
            Some(0) => self
                .run("push --delete", &["push", remote, "--delete", "--", branch])
                .map(|_| ()),
            Some(2) => Err(format!("{remote} carries no refs/heads/{branch}")),
            Some(code) => Err(format!(
                "`git ls-remote` exited {code} — {remote} was not read"
            )),
            None => Err(String::from("`git ls-remote` was killed by a signal")),
        }
    }

    fn detach(&self, at: &str) -> Result<(), String> {
        self.run(
            "checkout --detach",
            &["checkout", "--quiet", "--detach", at],
        )
        .map(|_| ())
    }

    fn reset_hard(&self, at: &str) -> Result<(), String> {
        self.run("reset --hard", &["reset", "--quiet", "--hard", at])
            .map(|_| ())
    }
}

/// A long verb's progress, as the ui module draws it: one bar bounded by the
/// rows the act will read, on stderr, silent until the act has already
/// outlasted the threshold. Nothing here changes an exit or a row.
pub(crate) struct Bar {
    wait: std::cell::RefCell<Option<Wait>>,
}

impl Bar {
    pub(crate) fn over(ui: &Ui, rows: u64, message: &str) -> Bar {
        Bar {
            wait: std::cell::RefCell::new(Some(ui.bar(rows, message))),
        }
    }
}

impl Progress for Bar {
    fn row(&self) {
        if let Some(wait) = self.wait.borrow().as_ref() {
            wait.inc(1);
        }
    }

    fn message(&self, text: &str) {
        if let Some(wait) = self.wait.borrow().as_ref() {
            wait.say(text);
        }
    }

    fn finish(&self) {
        if let Some(wait) = self.wait.borrow_mut().take() {
            wait.done();
        }
    }
}
