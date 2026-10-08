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

use crate::entry::{self, Body, SuiteRun, Timeline};
use crate::item::deliver::reviewer_of;
use crate::item::lane;
use crate::item::run::run_record;
use crate::item::{
    recorded, signal, Events, Git, Project, Stop, Unrecorded, CHECK_READ, ITEM_ENTRY, TRUNK,
    TRUNK_BRANCH,
};
use crate::policy;
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::{Directory, SeatId};
use crate::store::{Item, Status, Store};

use branch::{classify, Classification};
use licence::{accepted, licensed, neither};
use porcelain::{
    fingerprint, full, is_store_export, outside_also, outside_store, porcelain_path, range,
};
use rows::Rows;
use suite::{announce, stdin_command, suite_check, FIRST_READING, NO_READING};
use tree::Tree;

mod branch;
mod licence;
mod porcelain;
mod rows;
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

/// Which side of the work branch a delete line is about.
const LOCAL: &str = "local";

/// What the local line gains where the ref is still held: the act that can
/// finish the delete, named where a person meets the exit-1.
const RETIRE_DELETES: &str =
    "; `fleet seat retire` deletes it off the landed entry when the seat holding it goes";

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

#[allow(clippy::too_many_lines)]
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

    // (b) THE CHECKOUT, read once by [`land`] and carried here: the tree this
    // acts in is either the one the caller stood in or the reviewer's own, and
    // either way the answer is the one that chose it. The primary holds the
    // trunk and is the owner's; a landing runs from a linked worktree or not at
    // all, and a seat table naming the primary lands here too.
    if !linked {
        return Err(Stop::refused(
            "this is the primary checkout and not a linked worktree — the primary holds the trunk \
             and a landing runs from the reviewer's own",
        ));
    }
    // THE STORE'S OWN PATHS ARE THE ONES IT DECLARES: its export file and the
    // directory that holds it, and none at all for a store that declares no
    // export. Nothing here spells either.
    let export = wiring.store.capabilities()?.export;
    let status = wiring.git.status().map_err(Stop::could_not_tell)?;
    // THE EXPORT IS EXEMPT HERE AND NOTHING ELSE UNDER THE STORE IS. (e)
    // regenerates the export in this same root, so a check that refused on a
    // dirty board would refuse every re-run after its own first refusal — but
    // a refusal from (e) on hard-resets the tree, so every store path this check
    // lets past is a store path that check's reset discards. The two spellings
    // are the export by name and the whole directory, which is what an
    // untracked-but-unignored store answers the porcelain with.
    if let Some(loose) = outside_also(&status, landing.also, export.as_ref()) {
        return Err(Stop::refused(format!(
            "`{loose}` is changed in the working tree — a landing squashes the reviewed commit and \
             nothing else, and `--also <path>` is how a path of the reviewer's own is admitted"
        )));
    }

    // (c) THE RECORD. What the item says about itself, and the verdict that is
    // the whole licence to squash anything.
    //
    // THE ITEM IS RESOLVED HERE, ONCE, and the landing names the id the store
    // answered from this line on: the landed entry, the events, the land
    // branch and the close all carry the full id, whatever part of it was
    // typed.
    let item = wiring.store.show(landing.item)?;
    let resolved = item.id.clone();
    let landing = &Landing {
        item: &resolved,
        ..*landing
    };
    if item.status == Status::Closed {
        return Err(Stop::refused(format!(
            "{} is closed — a landing closes an item and cannot close one twice",
            item.id
        )));
    }
    // WHO CLOSES THIS ITEM, as a seat id, decided by the actor's KIND and never
    // by reading its text. A seat closes as itself. A run closes as the `[core]
    // reviewer`: the landing is that seat's act, on that seat's worktree, under
    // a hold that seat cleared, and the run is only what carried it. A routine
    // or the controller is nobody an item can be held by.
    let (closer, by_run): (SeatId, Option<Item>) = match landing.by.kind {
        ActorKind::Run => {
            let record = run_record(wiring.store, landing.by)?;
            (reviewer_of(wiring.project, wiring.seats)?.id, Some(record))
        }
        ActorKind::Seat => match landing.by.seat_id() {
            Some(seat) => (seat, None),
            None => return Err(neither(landing.by)),
        },
        ActorKind::Routine | ActorKind::Controller => return Err(neither(landing.by)),
    };
    // The closer's id, which the holder is compared with and the trailer
    // names; and the actor the landed entry, the events and the close are
    // written by — the closer, in its typed form.
    let closer_id = closer.to_string();
    let acting = Actor::seat(closer);
    let closer_named = wiring.seats.label(&closer);
    match item.assignee {
        Some(seat) if seat == closer => {}
        Some(seat) => {
            return Err(Stop::refused(format!(
                "{} is held by `{}` and not by `{closer_named}` — whoever closes an item lands \
                 its work",
                item.id,
                wiring.seats.label(&seat)
            )))
        }
        None => {
            return Err(Stop::refused(format!(
                "{} is held by nobody — whoever closes an item lands its work",
                item.id
            )))
        }
    }
    // THE RUN'S LICENCE. A seat answers for its own landing by making it; a run
    // answers for nothing, so a run's landing stands on the reviewer's own
    // clearance of the hold this run raised about the item, and is refused
    // where there is none.
    if let Some(record) = &by_run {
        let entries = wiring.store.timeline(&record.id)?;
        licensed(
            &record.id,
            &Timeline(&entries),
            &item.id,
            commit,
            closer,
            wiring.seats,
        )?;
    }
    // THE VERDICT IS THE TIMELINE'S LAST REVIEWED ENTRY, and the delivery its
    // last delivered one. A prose note is neither: the record has no grammar
    // left to read one by.
    let entries = wiring.store.timeline(&item.id)?;
    let accepted = accepted(
        &item.id,
        &Timeline(&entries),
        commit,
        &acting,
        by_run.as_ref(),
    )?;
    let Some((delivered_by, delivery)) = Timeline(&entries).last_delivery() else {
        return Err(Stop::refused(format!(
            "{} carries a verdict and no delivery — the work branch and the builder are read from \
             one and there is none to read",
            item.id
        )));
    };
    let work_branch = Some(delivery.branch.clone()).filter(|b| !b.is_empty());
    // The builder is the actor the delivered entry is by: a seat by its full
    // id, and any other kind — a run's delivery is the run's — in its string
    // form.
    let builder = delivered_by
        .by
        .seat_id()
        .map_or_else(|| delivered_by.by.to_string(), |seat| seat.to_string());
    rows.read(
        out,
        wiring,
        "PASS",
        format!(
            "{accepted} — the last verdict on {} accepts it, by this landing's own reviewer",
            item.id
        ),
    );

    // (d) THE TRUNK. A fetch that will not run is an instrument that could not
    // answer, not a refusal on the record.
    wiring.git.fetch(remote()).map_err(Stop::could_not_tell)?;
    let land_branch = format!("{LAND_PREFIX}{}", item.id);
    wiring
        .git
        .branch_at(&land_branch, TRUNK)
        .map_err(Stop::could_not_tell)?;
    tree.branch = Some(land_branch.clone());
    match wiring
        .git
        .squash_merge(commit)
        .map_err(Stop::could_not_tell)?
    {
        Squashed::Done => {}
        Squashed::Conflicted(paths) => {
            let _ = writeln!(out, "RETURN FOR REBASE");
            for path in &paths {
                let _ = writeln!(out, "  {path}");
            }
            return Err(Stop::refused(format!(
                "the squash of {commit} conflicts with {TRUNK} — return the item: a hand-merged \
                 conflict is a builder with no suite over their edit"
            )));
        }
    }

    // (e) THE EXPORT, then the staged set and the check on it. A store that
    // declares no export has none to take: no export and no fingerprint, and
    // no path of the store's own in the porcelain.
    let root = &wiring.project.root;
    let store_paths: Vec<String> = match &export {
        Some(spec) => {
            let file = root.join(&spec.file);
            let before = fingerprint(&file);
            wiring.store.export(root)?;
            let after = fingerprint(&file);
            if after.is_none() {
                return Err(Stop::could_not_tell(format!(
                    "the store's export left nothing at {} — the landing would carry no board at \
                     all",
                    file.display()
                )));
            }
            // THREE READINGS AND NOT ONE. An mtime alone says nothing on a
            // filesystem whose stamps are coarser than the act, so only a file
            // whose time, length AND content all agree with what stood there
            // before is one nothing wrote.
            if after == before {
                return Err(Stop::could_not_tell(format!(
                    "the store's export did not move {} — same mtime, same length and same \
                     bytes, so nothing was written and the landing would carry a stale board",
                    file.display()
                )));
            }
            // WHETHER THE STORE IS VERSIONED HERE IS READ, NOT ASSUMED. A
            // project may keep its store's directory out of git, and git add
            // over an ignored path exits 128 rather than staging nothing. The
            // porcelain says which project this is.
            let touched = wiring.git.status().map_err(Stop::could_not_tell)?;
            let store_paths: Vec<String> = touched
                .iter()
                .map(|line| porcelain_path(line))
                .filter(|path| path.starts_with(&spec.dir))
                .collect();
            // THE EXPORT BY NAME, and never the directory: an
            // untracked-but-unignored store answers the porcelain with its
            // directory whole, and a directory handed to `git add` there stages
            // the database beside the export.
            if !store_paths.is_empty() {
                wiring
                    .git
                    .add(std::slice::from_ref(&spec.file))
                    .map_err(Stop::could_not_tell)?;
            }
            store_paths
        }
        None => Vec::new(),
    };
    if !landing.also.is_empty() {
        wiring.git.add(landing.also).map_err(Stop::could_not_tell)?;
    }
    let all_staged = wiring.git.staged().map_err(Stop::could_not_tell)?;
    // The export's other read-back, and the polarity trap kept: where the
    // project versions the export, the file this act rewrote has to be in the
    // index, or the landing would carry a board that is a commit behind.
    // An untracked store answers the porcelain with its directory and not with
    // the file inside it, so both spellings say the export changed.
    if let Some(spec) = &export {
        if store_paths
            .iter()
            .any(|path| is_store_export(path, Some(spec)))
            && !all_staged.contains(&spec.file)
        {
            return Err(Stop::refused(format!(
                "{} is versioned here and did not reach the index — the landing would carry a \
                 board older than the item it closes",
                spec.file
            )));
        }
    }
    let staged = outside_store(&all_staged, export.as_ref());
    // THE DELIVERY'S OWN PATHS, kept apart from the set the check compares
    // against: the classification below asks whether what landed matches what
    // was reviewed, and an `--also` path is in neither commit's diff by
    // construction, so counting it there would read every landing as CARRIES.
    let delivery_paths = outside_store(
        &wiring
            .git
            .changed_since_merge_base(TRUNK, commit)
            .map_err(Stop::could_not_tell)?,
        export.as_ref(),
    );
    let delivered = outside_store(
        &delivery_paths
            .iter()
            .cloned()
            .chain(landing.also.iter().cloned())
            .collect::<Vec<String>>(),
        export.as_ref(),
    );
    if staged != delivered {
        let (staged_heading, delivered_heading) = match &export {
            Some(spec) => (
                format!("STAGED (outside {}):", spec.dir),
                format!("DELIVERED (outside {}):", spec.dir),
            ),
            None => (String::from("STAGED:"), String::from("DELIVERED:")),
        };
        let _ = writeln!(out, "{staged_heading}");
        for path in &staged {
            let _ = writeln!(out, "  {path}");
        }
        let _ = writeln!(out, "{delivered_heading}");
        for path in &delivered {
            let _ = writeln!(out, "  {path}");
        }
        return Err(Stop::refused(
            "the staged set is not the delivered set — both sets are printed above, and `--also \
             <path>` is the one way a path of the reviewer's own joins the second",
        ));
    }
    let set_description = if landing.also.is_empty() {
        "equal to the delivery's own set".to_string()
    } else {
        format!(
            "equal to the delivery's own set plus --also {}",
            landing.also.join(" ")
        )
    };
    rows.read(
        out,
        wiring,
        "PASS",
        match &export {
            Some(spec) => format!(
                "{} path(s) outside {}, {set_description}; {} regenerated by the store's own \
                 export",
                staged.len(),
                spec.dir,
                spec.file
            ),
            None => format!(
                "{} path(s), {set_description}; the store declares no export, so the landing \
                 carries no board file",
                staged.len()
            ),
        },
    );

    // (f) THE MARKER, read through the census reader.
    let marker_command = marker_of(wiring.project)?;
    let marker = match &marker_command {
        // THE STAGED PATHS, all of them: §4(f) says the staged paths, and a
        // marker command that decides whether a pipeline may be skipped is one
        // the store's own files are as much a part of as the delivery's.
        Some(command) => stdin_command(
            command,
            &wiring.project.root,
            wiring.child_path,
            &all_staged,
        )?,
        None => String::new(),
    };
    rows.read(
        out,
        wiring,
        if marker.is_empty() { "NONE" } else { "PASS" },
        match &marker_command {
            Some(command) if marker.is_empty() => {
                format!("`{command}` printed nothing over the staged pathset")
            }
            Some(command) => format!("{marker} — `{command}` over the staged pathset"),
            None => "no [landing] ci_marker in this project — no marker is appended".to_string(),
        },
    );

    // (g) THE COMMIT, from a message file, its sha read in the same act.
    let work_dir = landing.machine_dir.join("land").join(item.id.as_str());
    std::fs::create_dir_all(&work_dir).map_err(|e| {
        Stop::could_not_tell(format!(
            "the landing's own directory {} could not be made: {e}",
            work_dir.display()
        ))
    })?;
    let message_path = work_dir.join("message");
    std::fs::write(&message_path, message(&item, &marker, &closer_id, &builder)).map_err(|e| {
        Stop::could_not_tell(format!(
            "the commit message at {} could not be written: {e}",
            message_path.display()
        ))
    })?;
    wiring
        .git
        .commit_message_file(&message_path)
        .map_err(Stop::could_not_tell)?;

    // (h) THE SUITE, the command the caller handed in, or none at all and the
    // landing says NOT TESTED, with the one rerun a red gets before it refuses.
    let suite_command = landing
        .test
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_string);
    let readings = suite_check(
        out,
        &mut rows,
        &suite_command,
        &work_dir,
        landing,
        &acting,
        wiring,
    )?;

    // (i) THE CURRENT-TRUNK CHECK AND THE PUSH, in one act. Split into two they
    // are a race: the trunk can move between the count and the push, and the
    // push then lands on a trunk nobody read.
    wiring.git.fetch(remote()).map_err(Stop::could_not_tell)?;
    let behind = wiring
        .git
        .behind("HEAD", TRUNK)
        .map_err(Stop::could_not_tell)?;
    // The row is printed HERE, where it was read, and not after the push: a
    // rejected push is the case where a reader most wants to know what the
    // trunk was at, and a row held back until the push succeeded is one that
    // only ever appears on a landing that needed it least.
    rows.read(
        out,
        wiring,
        if behind == 0 { "PASS" } else { "BEHIND" },
        format!("behind={behind} against {TRUNK}, counted in the same act as the push"),
    );
    if behind > 0 {
        let _ = writeln!(out, "{REBASE_NEEDED}: {TRUNK} moved ({behind})");
        return Err(Stop::refused(format!(
            "{TRUNK} is {behind} commit(s) ahead of this land branch — rebase the work and land it \
             again; this run's suite is never carried over a rebuilt branch"
        )));
    }
    let pushed = wiring
        .git
        .push_head(remote(), TRUNK_BRANCH)
        .map_err(Stop::could_not_tell)?;
    // From here the trunk may hold the work, so nothing is put back.
    tree.pushed = true;
    // The push's whole output is kept beside the suite's log, because the two
    // refusals below both name a file rather than asking a reader to scroll.
    let push_path = work_dir.join("push.out");
    // THE WRITE IS READ. Both refusals below point a reader at this file, and a
    // refusal naming a file that is not there sends them looking for a run that
    // never happened.
    let kept = match std::fs::write(&push_path, &pushed.output) {
        Ok(()) => format!("its output is at {}", push_path.display()),
        Err(cause) => format!(
            "its output could not be kept at {}: {cause}, so it is above and nowhere else",
            push_path.display()
        ),
    };
    if pushed.code != Some(0) {
        let _ = writeln!(out, "{}", pushed.output.trim_end());
        return Err(Stop::refused(format!(
            "the push to {}/{TRUNK_BRANCH} exited {} — nothing after it ran: no entry, no close, \
             no branch touched; {kept}",
            remote(),
            rc_word(pushed.code)
        )));
    }

    // (j) THE RANGE LINE. The landed sha comes from the push's own output and
    // from nowhere else: a rev-parse of HEAD answers the local tip, which is
    // what a push that did nothing leaves behind.
    let Some((old, sha)) = range(&pushed.output) else {
        let _ = writeln!(out, "{}", pushed.output.trim_end());
        return Err(Stop::could_not_tell(format!(
            "the push printed no `<old>..<new>  HEAD -> {TRUNK_BRANCH}` line — no record has been \
             written and no ref deleted; {kept}"
        )));
    };
    // THE RANGE'S ENDS, WHOLE. The push prints both abbreviated, and an
    // abbreviation stops being unique as a history grows, so every sha this
    // landing writes — the entry, the event, the close and the LANDED line —
    // is the one each end resolves to here, where the push was made.
    let old = full(&old, &item.id, wiring)?;
    let sha = full(&sha, &item.id, wiring)?;

    // The two rows the landed entry carries about the state a landing ends in,
    // read before the entry is written and acted on after it.
    let classification = classify(
        commit,
        &sha,
        &delivery_paths,
        work_branch.as_deref(),
        &land_branch,
        wiring,
    );
    rows.read(
        out,
        wiring,
        classification.verdict(),
        classification.evidence(work_branch.as_deref()),
    );
    let after = wiring.git.status().map_err(Stop::could_not_tell)?;
    rows.read(
        out,
        wiring,
        if after.is_empty() { "PASS" } else { "DIRTY" },
        if after.is_empty() {
            format!("git status is empty in {}", wiring.project.root.display())
        } else {
            format!("git status: {}", after.join("; "))
        },
    );

    // (k) THE LANDED ENTRY, appended by the closer and read back. The landing
    // stands on the trunk whatever this step says. What tested it is the last
    // reading's command and exit — the one the landing stood on — or the
    // sentence saying nothing ran.
    let test = match (
        &suite_command,
        readings.last().and_then(|reading| reading.rc),
    ) {
        (Some(command), Some(rc)) => SuiteRun::Ran(entry::Ran {
            command: command.clone(),
            rc,
        }),
        _ => SuiteRun::NotTested(entry::NotTested {
            not_tested: UNTESTED.to_string(),
        }),
    };
    let landed = Body::Landed(entry::Landed {
        sha: sha.clone(),
        old: old.clone(),
        squash_of: commit.to_string(),
        run: by_run.as_ref().map(|record| record.id.to_string()),
        test,
        checks: rows.rows(),
        work_branch: entry::WorkBranch {
            branch: work_branch.clone(),
            classification: classification.recorded(),
        },
    });
    let entry = recorded(wiring.store, &item.id, &landed, &acting).map_err(|unrecorded| {
        let why = match unrecorded {
            Unrecorded::NotWritten(e) => format!("the landed entry was not written: {e}"),
            Unrecorded::Unconfirmed(why) => why,
        };
        Stop::could_not_tell(format!(
            "{why}\n  the landing {sha} STANDS on {TRUNK_BRANCH}"
        ))
    })?;

    // (k2) THE READINGS AND THE SIGNAL, after the entry has been written and
    // read back and before anything else — so a crash between them leaves a
    // landed entry the stream does not carry, and never a stream that signals a
    // landing no entry stands behind. The reading precedes the landing's signal,
    // as it did in time.
    // ONE `check.read` PER READING, in the order they were taken. A landing
    // that needed no rerun writes the one it always did.
    if readings.is_empty() {
        announce(
            &item.id,
            CHECK_READ,
            &acting,
            wiring,
            serde_json::json!({
                "item": item.id,
                "suite": suite_command,
                "rc": serde_json::Value::Null,
                "verdict": NO_READING,
                "reading": FIRST_READING,
                // A reading nobody took has no log, and an absent key and an
                // absent reading are not the same fact. Its `path` is null for
                // the same reason: no child ran under one.
                "log": serde_json::Value::Null,
                "path": serde_json::Value::Null,
            }),
        )?;
    }
    for reading in &readings {
        announce(
            &item.id,
            CHECK_READ,
            &acting,
            wiring,
            reading.payload(&item.id, suite_command.as_deref()),
        )?;
    }
    // The landed entry's signal. What carried the landing, what tested it and
    // the sha it pushed are the entry's, and a reader reads them there.
    signal(wiring.events, &acting, &item.id, &entry, "landed").map_err(|e| {
        Stop::could_not_tell(format!(
            "{ITEM_ENTRY} did not reach the stream: {e}\n  the landing on {} STANDS and its \
             landed entry is on the record",
            item.id
        ))
    })?;

    // (l) THE CLOSE, with the landed sha in its reason and the run that carried
    // it beside the sha, where one did.
    let landed = match &by_run {
        Some(record) => format!("landed {sha} through run {}", record.id),
        None => format!("landed {sha}"),
    };
    let reason = match landing.reason {
        Some(text) if !text.trim().is_empty() => format!("{landed} — {}", text.trim()),
        _ => landed,
    };
    // THE CLOSE IS THE HOLDER'S: a store may close an assigned item only for
    // its assignee — the adapter's `close` says how it matches the seat to
    // one — and the holder check above has already said the closer is that
    // seat.
    wiring
        .store
        .close(&item.id, &reason, &acting)
        .map_err(|e| unclosed(&item.id, &sha, &e.to_string()))?;
    let closed = wiring.store.show(&item.id)?;
    if closed.status != Status::Closed {
        return Err(unclosed(
            &item.id,
            &sha,
            &format!("it read back with status `{}`", closed.status),
        ));
    }

    // (m) THE WORK BRANCH, deleted on SAFE alone and on the origin side only
    // here, after the range line was read.
    if let (Some(branch), Classification::Safe) = (work_branch.as_deref(), &classification) {
        for (what, deleted) in [
            (LOCAL, wiring.git.delete_branch(branch)),
            ("origin", wiring.git.delete_remote_branch(remote(), branch)),
        ] {
            match deleted {
                Ok(()) => {
                    let _ = writeln!(out, "work branch {branch}: {SAFE}, {what} deleted");
                }
                // The landing succeeded and a ref that outlived it is its
                // holder's to remove, so each side says what happened to it and
                // the exit stays 0.
                //
                // The LOCAL side is held by construction wherever the seat that
                // delivered is still up — its worktree has the branch checked
                // out — so that line names the act that can finish it.
                Err(cause) => {
                    let next = if what == LOCAL { RETIRE_DELETES } else { "" };
                    let _ = writeln!(
                        err,
                        "work branch {branch}: {SAFE}, {what} kept — {cause}{next}"
                    );
                }
            }
        }
    } else if let Some(branch) = work_branch.as_deref() {
        let _ = writeln!(out, "work branch {branch}: {}", classification.verdict());
        let _ = writeln!(
            out,
            "  kept. Delete only on SAFE; a COULD NOT TELL is a question, never a no."
        );
    }

    // (n) THE CHECKOUT PUT BACK, and the one line a caller greps for.
    //
    // A detach that will not run WARNS and does not take the exit with it: the
    // work is on the trunk and the item is closed, and a landing reported as
    // could-not-tell over a checkout left on a land branch is a landing the
    // next reader would try to make again.
    if let Err(cause) = wiring.git.detach(TRUNK) {
        let _ = writeln!(
            err,
            "the checkout is still on the land branch — {cause}; the landing STANDS"
        );
    }
    if let Some(branch) = tree.branch.take() {
        if let Err(cause) = wiring.git.delete_branch(&branch) {
            let _ = writeln!(
                err,
                "the land branch {branch} outlived the landing — {cause}"
            );
        }
    }
    let _ = writeln!(out, "{LANDED} {sha}");

    Ok(Landed {
        item: item.id.to_string(),
        sha,
        entry,
    })
}

/// The commit's message: the subject, the marker where there is one, and the
/// two trailers that say who landed it and who wrote it.
fn message(item: &Item, marker: &str, closer: &str, builder: &str) -> String {
    let subject = if marker.is_empty() {
        format!("{}: {}", item.id, item.title)
    } else {
        format!("{}: {} {marker}", item.id, item.title)
    };
    format!("{subject}\n\nSeat: {closer}\nImplemented-by: {builder}\n")
}

/// A close that did not happen after the push. The landing is real and the
/// message says so, and names the read that shows what the item holds.
fn unclosed(item: &str, sha: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{item} did not close: {why}\n  the landing {sha} STANDS on {TRUNK_BRANCH}\n  READ: \
         fleet item show {item}"
    ))
}

// ---- the marker key ----------------------------------------------------------

/// `[landing] ci_marker`, through the census reader. The table and the key are
/// LITERALS at the call site, as they are at every other reader in this
/// workspace: the pair a verb reads has to be readable out of the source
/// without running it. This verb is its first reader.
fn marker_of(project: &Project) -> Result<Option<String>, Stop> {
    command(policy::read("landing", "ci_marker", &project.policy))
}

/// A key that is there but is not a string reads as absent: a marker is a
/// command line or it is nothing.
fn command(
    read: Result<Option<&toml::Value>, crate::policy::Unlisted>,
) -> Result<Option<String>, Stop> {
    match read {
        Ok(Some(value)) => Ok(value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)),
        Ok(None) => Ok(None),
        Err(unlisted) => Err(Stop::could_not_tell(unlisted.to_string())),
    }
}

fn rc_word(code: Option<i32>) -> String {
    match code {
        Some(code) => code.to_string(),
        None => "on a signal".to_string(),
    }
}
