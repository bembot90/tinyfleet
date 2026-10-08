//! `fleet land <item> <commit>` — the reviewer's verb, and the only writer of a
//! landing.
//!
//! IT LANDS A REVIEW, NOT A DELIVERY. The check that decides whether anything
//! may be squashed is the timeline's last `reviewed` entry: an accept of this
//! exact commit, appended by this landing's own reviewer. A delivery nobody
//! accepted, an accept of a different commit, an accept another actor wrote,
//! and a return are each refused before the trunk is touched.
//!
//! IT TAKES A COMMIT AND NEVER A BRANCH NAME. A branch resolves perfectly well
//! — to whatever its tip is at merge time, which is not what the reviewer read
//! — so the shape is refused before anything is read (the pack's rules, rule 1).
//!
//! ONE ACT, NOT TWO PHASES. The suite runs inside it, which is why the check
//! rows reach stdout as they are read rather than at the end: a landing that
//! takes minutes says where it is while it is there.
//!
//! A RUN'S LANDING IS THE REVIEWER'S ACT, CARRIED BY THE RUN. A workflow calls
//! every verb as the run — the actor it hands in is `run:<record id>` — while
//! `deliver` hands every item to the `[core] reviewer`, so on a workflow's
//! landing the holder is always that seat and the caller is always the run. A
//! landing called by a run therefore closes AS the reviewer: the holder check
//! reads that seat, the close and the landed entry carry it with the run named
//! beside it, and what licenses the act is the reviewer's own answer to the
//! hold this run raised. A seat's own landing acts as itself, by its id. Which
//! of the two a landing is, is the actor's KIND; a routine or the controller
//! lands nothing.
//!
//! IT RUNS IN THE REVIEWER'S OWN WORKTREE, WHEREVER IT WAS CALLED FROM. A
//! workflow's verbs all run at the registered project's root, which on a box
//! whose fleet is registered against the trunk checkout is the primary — so a
//! landing handed the primary resolves the `[core] reviewer` seat's worktree
//! for this project out of the machine's seat table and drives the whole act
//! there. The primary holds the trunk and is never the landing tree.
//!
//! WHAT PUTS THE TREE BACK, AND WHERE IT STOPS. Every refusal before the push
//! deletes the land branch, detaches the checkout onto the trunk and restores
//! any `--also` file from the copy taken before the first git write. Nothing is
//! put back after the push: the landing is on the trunk, and a checkout rolled
//! back over it would only hide that.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::item::deliver::reviewer_of;
use crate::item::lane;
use crate::item::{Events, Git, Project, Stop, TRUNK};
use crate::seat::actor::Actor;
use crate::seat::identity::{Directory, SeatId};
use crate::store::Store;

use rows::Rows;
use steps::{
    the_checkout, the_checkout_put_back, the_close, the_commit, the_end_state, the_export,
    the_landed_entry, the_marker, the_push, the_range_line, the_readings_and_signal, the_record,
    the_staged_set, the_suite, the_trunk, the_work_branch, Act,
};
use tree::Tree;

mod branch;
mod licence;
mod porcelain;
mod rows;
mod steps;
mod suite;
mod tree;

pub use branch::{release, Release};
pub(crate) use rows::row;

/// The criteria a landing reads, in the order the checks are READ — which is
/// the order they are printed in, so the page a person watches and the landed
/// entry a reader finds afterwards carry the same rows in the same places.
pub const CRITERIA: [&str; 7] = [
    "reviewed commit",
    "staged set",
    "CI marker",
    "suite",
    "base current",
    "work branch",
    "tree clean after",
];

/// The verdict a work-branch row carries when the branch holds nothing the
/// trunk does not. Written once, for the landing that prints it and the retire
/// that names it when it finishes the delete.
pub const SAFE: &str = "SAFE";

/// The criterion the suite's SECOND reading is printed under. It is not one of
/// [`CRITERIA`]: a landing that needed no rerun carries no such row, so the
/// count is not fixed and the name cannot be positional.
pub const SUITE_RERUN_ROW: &str = "suite rerun";

/// The line a landing ends on, which is also the one a caller greps for.
pub const LANDED: &str = "LANDED";

/// What a landing handed no test command says, on its suite row. Loud on
/// purpose: a landing that ran nothing is allowed and is never allowed to read
/// like one that ran something.
pub const NOT_TESTED: &str = "NOT TESTED";

/// What follows [`NOT_TESTED`] on the row, and what the landed entry's `test`
/// says where nothing ran.
pub const UNTESTED: &str =
    "no test command was handed to this landing (`fleet land --test <command>`), so nothing ran \
     and it stands on the review alone";

/// The line a landing prints when the trunk moved between the land branch's cut
/// and the push's own check.
///
/// IT IS NOT A PARK. The flight retries on its next call: the lane's lock makes
/// the race rare, and a park would put a person in the middle of a landing
/// nothing is wrong with. A person's `fleet land` reads the refusal under it and
/// runs again.
pub const REBASE_NEEDED: &str = "REBASE NEEDED";

/// The prefix every land branch this verb cuts is named under. A delivery that
/// names one names a landing's branch and not a builder's.
const LAND_PREFIX: &str = "land/";

/// The machine's seat table, under the machine directory this verb is already
/// handed. It is read here and nowhere else in this crate: the process table is
/// the controller's and the one fact a landing wants from it is which tree a
/// seat stands in.
const SEATS: &str = "config.json";

/// The table's own key for its rows, which is not spelled `seats`.
const SEAT_ROWS: &str = "children";

/// The row's map of project name to the worktree that seat holds for it.
const WORKTREES: &str = "worktrees";

/// The remote half of [`TRUNK`], derived and not spelled a second time: two
/// copies of one fact are two things that can disagree.
pub fn remote() -> &'static str {
    match TRUNK.split_once('/') {
        Some((remote, _)) => remote,
        None => TRUNK,
    }
}

// ---- the seam ----------------------------------------------------------------

/// What a squash merge answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Squashed {
    /// The merge is staged in the index and nothing conflicted.
    Done,
    /// The paths git left conflicted. The merge has been put back by the time
    /// this is answered: the reviewer never resolves one by hand.
    Conflicted(Vec<String>),
}

/// A push as it ended, whatever that was.
///
/// The output is answered on every exit and the status beside it, because the
/// two questions a rejected push raises — what did the remote say, and did it
/// land — are answered by different halves of this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    /// stdout and stderr as one text, in the order git wrote them.
    pub output: String,
    /// The child's own status; `None` where a signal ended it.
    pub code: Option<i32>,
}

/// The git operations LAND reads and writes, over the ones every verb shares.
///
/// A second trait and not fifteen more methods on [`Git`]: `deliver` and
/// `review` would otherwise carry stubs for operations they never call, and a
/// stub body nobody reaches is one a later change can start reaching without
/// anything announcing it.
pub trait LandGit: Git {
    /// Whether this checkout is a LINKED worktree — its git dir differs from
    /// its common dir. The primary holds the trunk and is never landed from.
    fn is_linked_worktree(&self) -> Result<bool, String>;

    /// The same git, driven at another working tree.
    ///
    /// A landing that resolves a tree other than the one it was handed acts
    /// through this one and never again through the git it started with, so
    /// the tree it was handed is left exactly as it was found.
    fn at(&self, root: &Path) -> Box<dyn LandGit + '_>;

    /// `git fetch <remote>`.
    fn fetch(&self, remote: &str) -> Result<(), String>;

    /// A branch created or reset at a ref, and checked out.
    fn branch_at(&self, branch: &str, at: &str) -> Result<(), String>;

    /// `git merge --squash <commit>`, its conflict aborted before it answers.
    fn squash_merge(&self, commit: &str) -> Result<Squashed, String>;

    /// The paths staged, by name.
    fn add(&self, paths: &[String]) -> Result<(), String>;

    /// The paths a commit changed since its merge-base with a ref.
    fn changed_since_merge_base(&self, base: &str, commit: &str) -> Result<Vec<String>, String>;

    /// The paths a diff between two commits touches, restricted to the names
    /// given. An empty answer is an empty diff.
    fn diff_paths(&self, from: &str, to: &str, paths: &[String]) -> Result<Vec<String>, String>;

    /// The staged set committed with the message in this file, answered as the
    /// commit it produced — read from HEAD in the same act.
    fn commit_message_file(&self, message: &Path) -> Result<String, String>;

    /// How many commits `<what>` is behind `<of>`.
    fn behind(&self, what: &str, of: &str) -> Result<u64, String>;

    /// HEAD pushed to a remote branch, answering whatever it printed and
    /// whatever it exited. A rejected push is a value here and not an error:
    /// the caller is the one that has to print what the remote said.
    fn push_head(&self, remote: &str, branch: &str) -> Result<Pushed, String>;

    /// What a revision resolves to, as a full sha, or `None` where it names no
    /// commit. A branch's tip and "is this a commit at all" are one reading.
    fn rev(&self, rev: &str) -> Result<Option<String>, String>;

    /// A local branch deleted.
    fn delete_branch(&self, branch: &str) -> Result<(), String>;

    /// A remote branch deleted.
    fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<(), String>;

    /// The checkout detached onto a ref.
    fn detach(&self, at: &str) -> Result<(), String>;

    /// The index and the working tree put back to a ref.
    ///
    /// A detach alone does NOT undo a squash: between the squash and the commit
    /// HEAD already names the trunk, so detaching onto it succeeds while
    /// leaving the whole merge staged and on disk.
    fn reset_hard(&self, at: &str) -> Result<(), String>;
}

/// The progress surface, which `fly` draws on too: one trait for both, so a
/// caller implements it once. Named here as well because every reader of this
/// verb reaches for `land::Progress`.
pub use crate::item::Progress;

// ---- the arguments -----------------------------------------------------------

/// The landing, as its arguments.
pub struct Landing<'a> {
    pub item: &'a str,
    /// The commit the reviewer read. Never a branch name.
    pub commit: &'a str,
    /// The reviewer's own paths, admitted into the staged set by name.
    pub also: &'a [String],
    /// The command run on the land branch under the lane's lock, after the
    /// squash and before the push, so what is tested is what lands. It is the
    /// CALLER's, never the project's: a workflow hands it in, and `None` runs
    /// nothing and lands [`NOT_TESTED`].
    pub test: Option<&'a str>,
    /// What the close says beyond the landed sha.
    pub reason: Option<&'a str>,
    /// Who lands it: a seat in its own name, or a run as the reviewer.
    pub by: &'a Actor,
    /// The clock, taken by the caller: core reads none. It stamps the lane's
    /// lock, so a landing that waits can say since when.
    pub at: &'a str,
    /// Where the message file and the suite's log go: a process fact, resolved
    /// by the caller like every other one. The lane and its lock are derived
    /// from it too, which is what puts the tick's landings and a person's on
    /// one queue.
    pub machine_dir: &'a Path,
}

/// Everything the verb acts through.
pub struct Wiring<'a> {
    pub store: &'a dyn Store,
    pub git: &'a dyn LandGit,
    pub project: &'a Project,
    pub progress: &'a dyn Progress,
    pub events: &'a dyn Events,
    /// The box's load, for the one wait a rerun takes. A caller with no
    /// controller behind it hands [`lane::Unread`], and the rerun then runs at
    /// once and says it did not wait.
    pub load: &'a dyn lane::Load,
    /// The `PATH` every project-declared child of this verb runs under,
    /// constructed by the caller from `platform::child_path` and never read off
    /// this process. Empty means the caller has none and the children inherit
    /// this process's environment unchanged.
    pub child_path: &'a str,
    /// The seats this fleet knows: what `[core] reviewer` is resolved among,
    /// and what a sentence names a seat by.
    pub seats: &'a Directory,
}

/// The landing made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    pub item: String,
    /// The sha the push's own range line named, resolved whole in the tree
    /// that pushed it.
    pub sha: String,
    /// The landed entry's id, as the store answered it.
    pub entry: String,
}

// ---- the verb ----------------------------------------------------------------

/// The landing, in the tree it belongs in.
///
/// (a0) THE TREE, READ BEFORE ANYTHING IS TOUCHED. A linked worktree is the
/// reviewer standing where a landing runs and this verb acts through the git
/// it was handed. The primary is a caller that resolved the registered
/// project's root and not a tree of anyone's own — every workflow verb does,
/// which is the whole defect — so the reviewer's own worktree is resolved and
/// the act is driven there instead. The reading is the CHECKOUT's and not the
/// process's: an environment variable naming a run would say a workflow called
/// this, and the fact that decides where a landing runs is which checkout it
/// was pointed at.
pub fn land(
    out: &mut dyn Write,
    err: &mut dyn Write,
    landing: &Landing,
    wiring: &Wiring,
) -> Result<Landed, Stop> {
    // THE SHAPE FIRST, so a branch name where a commit is meant is still
    // refused before any instrument is asked anything — the resolution below
    // reads a checkout and a file, and both are instruments.
    commit_shape(landing.commit)?;
    wiring.project.refuse_moved()?;
    if wiring
        .git
        .is_linked_worktree()
        .map_err(Stop::could_not_tell)?
    {
        return land_in(out, err, landing, wiring, true);
    }
    let root = reviewers_worktree(wiring, landing.machine_dir)?;
    let git = wiring.git.at(&root);
    let linked = git.is_linked_worktree().map_err(Stop::could_not_tell)?;
    let project = Project {
        root,
        name: wiring.project.name.clone(),
        policy: wiring.project.policy.clone(),
        guards: wiring.project.guards.clone(),
    };
    land_in(
        out,
        err,
        landing,
        &Wiring {
            git: git.as_ref(),
            project: &project,
            ..*wiring
        },
        linked,
    )
}

/// The `[core] reviewer` seat's worktree for this project, out of the machine's
/// seat table.
///
/// IT REFUSES RATHER THAN FALLING BACK. Every path out of here that is not a
/// worktree is the primary, and a landing that ran there would squash onto the
/// trunk checkout somebody else is standing in — so a seat the table has no
/// row for, and a seat it names with no worktree for this project, are each a
/// refusal naming the seat and the file.
///
/// THE REVIEWER IS RESOLVED, then found: `[core] reviewer` is any seat
/// argument, resolved among the listed seats to one seat, and its row is the
/// one whose `id` is that seat's. A row with no id that parses is no seat this
/// table can name.
fn reviewers_worktree(wiring: &Wiring, machine_dir: &Path) -> Result<PathBuf, Stop> {
    let reviewer = reviewer_of(wiring.project, wiring.seats)?;
    let seat = reviewer.machine_name();
    let table = machine_dir.join(SEATS);
    let body = std::fs::read_to_string(&table).map_err(|e| {
        Stop::could_not_tell(format!(
            "a landing runs from `{seat}`'s own worktree and {} could not be read: {e} — that \
             table is where a seat's worktree is named",
            table.display()
        ))
    })?;
    let document: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        Stop::could_not_tell(format!("{} is not readable JSON: {e}", table.display()))
    })?;
    let row = document
        .get(SEAT_ROWS)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|row| {
            row.get("id")
                .and_then(serde_json::Value::as_str)
                .and_then(|id| SeatId::parse(id).ok())
                == Some(reviewer.id)
        })
        .ok_or_else(|| {
            Stop::refused(format!(
                "[core] reviewer is `{seat}` ({}), and {} carries no row for it — a landing runs \
                 from the reviewer's own worktree and this machine runs no such seat",
                reviewer.id,
                table.display()
            ))
        })?;
    let named = row
        .get(WORKTREES)
        .and_then(|trees| trees.get(&wiring.project.name))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty());
    match named {
        Some(path) => Ok(PathBuf::from(path)),
        None => Err(Stop::refused(format!(
            "`{seat}` carries no `{}` worktree in {} — a landing runs from the reviewer's own \
             worktree and this reviewer has none for this project",
            wiring.project.name,
            table.display()
        ))),
    }
}

fn land_in(
    out: &mut dyn Write,
    err: &mut dyn Write,
    landing: &Landing,
    wiring: &Wiring,
    linked: bool,
) -> Result<Landed, Stop> {
    // (a) THE ARGUMENTS, and the copies of everything a refusal has to put
    // back, taken while nothing has been written.
    let commit = resolve_commit(landing, wiring)?;
    let mut tree = Tree::of(landing, wiring)?;

    // (a2) THE LANE, taken before the fetch and held until this verb returns
    // whatever its exit. It is `land` that takes it and not the flight, so a
    // person's landing and the tick's queue on one object — and a landing that
    // never reaches the trunk still holds it while it could have.
    let _lane = lane::take(
        out,
        wiring.progress,
        &lane::directory(
            landing.machine_dir,
            &wiring.project.guards,
            &wiring.project.name,
        )?,
        landing.item,
        landing.at,
    )?;

    let landed = run(out, err, &commit, &mut tree, landing, wiring, linked);
    if landed.is_err() {
        tree.put_back(err, wiring);
    }
    wiring.progress.finish();
    landed
}

/// The shape alone, asking no instrument anything — which is why it is read
/// before the tree this landing runs in is resolved, let alone opened.
fn commit_shape(given: &str) -> Result<(), Stop> {
    if !(7..=40).contains(&given.len()) || !given.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Stop::usage(format!(
            "`{given}` is not a commit — a commit is 7 to 40 hex characters, and a branch name is \
             refused here because its tip is not what the reviewer read"
        )));
    }
    Ok(())
}

/// `<commit>` as a full sha. Seven to forty hex characters resolving to a
/// commit, and nothing else: a branch name is refused by shape in [land],
/// before any instrument is asked what it points at.
fn resolve_commit(landing: &Landing, wiring: &Wiring) -> Result<String, Stop> {
    let given = landing.commit;
    match wiring.git.rev(given).map_err(Stop::could_not_tell)? {
        Some(sha) => Ok(sha),
        None => Err(Stop::usage(format!(
            "`{given}` resolves to no commit in this checkout"
        ))),
    }
}

fn run(
    out: &mut dyn Write,
    err: &mut dyn Write,
    commit: &str,
    tree: &mut Tree,
    landing: &Landing,
    wiring: &Wiring,
    linked: bool,
) -> Result<Landed, Stop> {
    let mut rows = Rows::new();

    let export = the_checkout(linked, landing, wiring)?;
    let record = the_record(out, &mut rows, commit, landing, wiring)?;
    let resolved = record.item.id.clone();
    let landing = &Landing {
        item: &resolved,
        ..*landing
    };
    let act = Act {
        commit,
        landing,
        wiring,
    };
    let land_branch = the_trunk(out, &act, &record.item, tree)?;
    let store_paths = the_export(&export, wiring)?;
    let staged = the_staged_set(out, &mut rows, &act, &export, &store_paths)?;
    let marker = the_marker(out, &mut rows, &staged.all_staged, wiring)?;
    let work_dir = the_commit(&act, &record, &marker)?;
    let suite = the_suite(out, &mut rows, &act, &work_dir, &record.acting)?;
    let (pushed, kept) = the_push(out, &mut rows, &act, &work_dir, tree)?;
    let (old, sha) = the_range_line(out, &pushed, &kept, &record.item, wiring)?;
    let classification = the_end_state(
        out,
        &mut rows,
        &act,
        &sha,
        &staged.delivery_paths,
        &record.work_branch,
        &land_branch,
    )?;
    let entry = the_landed_entry(&act, &record, &rows, &suite, &old, &sha, &classification)?;
    the_readings_and_signal(&act, &record, &suite, &entry)?;
    the_close(&act, &record, &sha)?;
    the_work_branch(out, err, wiring, &record.work_branch, &classification);
    the_checkout_put_back(out, err, tree, &sha, wiring);

    Ok(Landed {
        item: record.item.id.to_string(),
        sha,
        entry,
    })
}

fn rc_word(code: Option<i32>) -> String {
    match code {
        Some(code) => code.to_string(),
        None => "on a signal".to_string(),
    }
}
