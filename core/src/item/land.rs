//! `fleet land <item> <commit>` — the reviewer's verb, and the only writer of a
//! landing.
//!
//! IT LANDS A REVIEW, NOT A DELIVERY. The check that decides whether anything
//! may be squashed is the item's last verdict: an `ACCEPTED` naming this exact
//! commit. A delivery nobody accepted, an accept naming a different commit, and
//! a return are each refused before the trunk is touched.
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
//! reads that seat, the close and `item.landed` carry it with the run named
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
use std::process::{Command, Stdio};

use crate::entry::Timeline;
use crate::item::brief::Packs;
use crate::item::deliver::{named, reviewer_of};
use crate::item::lane;
use crate::item::review::last_verdict;
use crate::item::run;
use crate::item::{
    control_token, last_answer, last_landing, marker_block, opens_with, render, Events, Git,
    Project, Stop, CHECK_READ, ITEM_LANDED, LANDING_MARKERS, TRUNK, TRUNK_BRANCH, VERDICT_MARKERS,
};
use crate::policy;
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::{Directory, SeatId};
use crate::store::{Item, Store, StoreError, EXPORT};

/// The landing-note grammar, in core's pack and shadowable like every other
/// asset.
pub const LANDING_NOTE: &str = "assets/landing-note.md";

/// The criteria the note carries, in the order the checks are READ — which is
/// the order they are printed in, so the page a person watches and the note a
/// reader finds afterwards carry the same rows in the same places.
pub const CRITERIA: [&str; 7] = [
    "reviewed commit",
    "staged set",
    "CI marker",
    "suite",
    "base current",
    "work branch",
    "tree clean after",
];

/// Which of [`CRITERIA`] names the work branch, so the row [`release`] reads is
/// named by the same value the landing wrote it under.
pub const WORK_BRANCH: usize = 5;

/// The verdict a work-branch row carries when the branch holds nothing the
/// trunk does not. Written once, for the landing that prints it and the retire
/// that reads it back off the note.
pub const SAFE: &str = "SAFE";

/// Which side of the work branch a delete line is about.
const LOCAL: &str = "local";

/// What the local line gains where the ref is still held: the act that can
/// finish the delete, named where a person meets the exit-1.
const RETIRE_DELETES: &str =
    "; `fleet seat retire` deletes it off this note when the seat holding it goes";

/// The criterion the suite's SECOND reading is printed under. It is not one of
/// [`CRITERIA`]: a landing that needed no rerun carries no such row, so the
/// count is not fixed and the name cannot be positional.
pub const SUITE_RERUN: &str = "suite rerun";

/// The line a landing ends on, which is also the one a caller greps for.
pub const LANDED: &str = "LANDED";

/// What a landing handed no test command says, on its suite row and on its
/// note's first line. Loud on purpose: a landing that ran nothing is allowed
/// and is never allowed to read like one that ran something.
pub const NOT_TESTED: &str = "NOT TESTED";

/// What follows [`NOT_TESTED`] on the note's first line and on the row.
const UNTESTED: &str =
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

/// The store's own directory, whose paths the staged-set check does not judge:
/// they are this verb's own bookkeeping and not the delivery's.
const STORE_DIR: &str = ".beads/";

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

/// How often the suite's log is re-read while the suite runs. The verb has no
/// deadline of its own; this is only how often the bar's message can change.
const TICK: std::time::Duration = std::time::Duration::from_millis(200);

/// How much of a red suite's log the row prints.
const TAIL: usize = 20;

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
pub use crate::item::{Progress, Silent};

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
    pub packs: &'a Packs,
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
    /// The sha the push's own range line named.
    pub sha: String,
    pub note: String,
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
            store: wiring.store,
            git: git.as_ref(),
            packs: wiring.packs,
            project: &project,
            progress: wiring.progress,
            events: wiring.events,
            load: wiring.load,
            child_path: wiring.child_path,
            seats: wiring.seats,
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
/// commit, and nothing else: a branch name is refused HERE, by shape, before
/// any instrument is asked what it points at.
fn resolve_commit(landing: &Landing, wiring: &Wiring) -> Result<String, Stop> {
    let given = landing.commit;
    commit_shape(given)?;
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
    let status = wiring.git.status().map_err(Stop::could_not_tell)?;
    // THE EXPORT IS EXEMPT HERE AND NOTHING ELSE UNDER THE STORE IS. (e)
    // regenerates the export in this same root, so a check that refused on a
    // dirty board would refuse every re-run after its own first refusal — but
    // a refusal from (e) on hard-resets the tree, so every store path this check
    // lets past is a store path that check's reset discards. The two spellings
    // are the export by name and the whole directory, which is what an
    // untracked-but-unignored store answers the porcelain with.
    if let Some(loose) = outside_also(&status, landing.also) {
        return Err(Stop::refused(format!(
            "`{loose}` is changed in the working tree — a landing squashes the reviewed commit and \
             nothing else, and `--also <path>` is how a path of the reviewer's own is admitted"
        )));
    }

    // (c) THE RECORD. What the item says about itself, and the verdict that is
    // the whole licence to squash anything.
    //
    // THE ITEM IS RESOLVED HERE, ONCE, and the landing names the id the store
    // answered from this line on: the note, the events, the land branch and
    // the close all carry the full id, whatever part of it was typed.
    let item = read(wiring.store, landing.item)?;
    let resolved = item.id.clone();
    let landing = &Landing {
        item: &resolved,
        ..*landing
    };
    if item.status == "closed" {
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
    // The closer's id, which the holder is compared with, the trailer and the
    // note name and the close is made under; and the actor the note and the
    // events are written by — the closer, in its typed form.
    let closer_id = closer.to_string();
    let acting = Actor::seat(closer);
    let actor = acting.to_string();
    let closer_named = wiring.seats.label(&closer);
    match item.assignee.as_deref() {
        Some(seat) if seat == closer_id => {}
        Some(seat) => {
            return Err(Stop::refused(format!(
                "{} is held by `{}` and not by `{closer_named}` — whoever closes an item lands \
                 its work",
                item.id,
                named(seat, wiring.seats)
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
    // answer to the hold this run raised and is refused where there is none.
    if let Some(record) = &by_run {
        licensed(record, closer, wiring.seats)?;
    }
    // The name the landing is written under wherever one is asked for and two
    // are the fact: the seat a person reads and the id a script can pass.
    let landed_by = match &by_run {
        Some(record) => format!("{closer_named} ({closer_id}) through run {}", record.id),
        None => format!("{closer_named} ({closer_id})"),
    };
    let notes = item.notes.clone().unwrap_or_default();
    let accepted = accepted_commit(&item.id, &notes, commit, wiring)?;
    // THE DELIVERY IS THE TIMELINE'S LAST DELIVERED ENTRY. A prose delivery
    // note is not one: the record has no delivery grammar left to read.
    let entries = wiring.store.timeline(&item.id)?;
    let Some((delivered_by, delivery)) = Timeline(&entries).last_delivery() else {
        return Err(Stop::refused(format!(
            "{} carries a verdict and no delivery — the work branch and the builder are read from \
             one and there is none to read",
            item.id
        )));
    };
    let work_branch = Some(delivery.branch.clone()).filter(|b| !b.is_empty());
    let delivered_base = delivery.base.clone();
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
            "{accepted} — the last ACCEPTED verdict on {} names it",
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

    // (e) THE EXPORT, then the staged set and the check on it.
    let export = wiring.project.root.join(EXPORT);
    let before = fingerprint(&export);
    wiring.store.export(&wiring.project.root)?;
    let after = fingerprint(&export);
    if after.is_none() {
        return Err(Stop::could_not_tell(format!(
            "the store's export left nothing at {} — the landing would carry no board at all",
            export.display()
        )));
    }
    // THREE READINGS AND NOT ONE. An mtime alone says nothing on a filesystem
    // whose stamps are coarser than the act, so only a file whose time, length
    // AND content all agree with what stood there before is one nothing wrote.
    if after == before {
        return Err(Stop::could_not_tell(format!(
            "the store's export did not move {} — same mtime, same length and same bytes, so \
             nothing was written and the landing would carry a stale board",
            export.display()
        )));
    }
    // WHETHER THE STORE IS VERSIONED HERE IS READ, NOT ASSUMED. `bd init`
    // writes the ignore that hides its own directory, so in a project that
    // keeps the work graph out of git there is nothing under `.beads/` for git
    // to take — and `git add .beads` over a directory it has been told to
    // ignore exits 128 rather than staging nothing. The porcelain says which
    // project this is.
    let touched = wiring.git.status().map_err(Stop::could_not_tell)?;
    let store_paths: Vec<String> = touched
        .iter()
        .map(|line| porcelain_path(line))
        .filter(|path| path.starts_with(STORE_DIR))
        .collect();
    // THE EXPORT BY NAME, and never the directory: an untracked-but-unignored
    // store answers the porcelain with `.beads/` whole, and a directory handed
    // to `git add` there stages the database beside the export.
    if !store_paths.is_empty() {
        wiring
            .git
            .add(&[EXPORT.to_string()])
            .map_err(Stop::could_not_tell)?;
    }
    if !landing.also.is_empty() {
        wiring.git.add(landing.also).map_err(Stop::could_not_tell)?;
    }
    let all_staged = wiring.git.staged().map_err(Stop::could_not_tell)?;
    // The export's other read-back, and the polarity trap kept: where the
    // project versions the export, the file this act rewrote has to be in the
    // index, or the landing would carry a board that is a commit behind.
    // An untracked store answers the porcelain with its directory and not with
    // the file inside it, so both spellings say the export changed.
    if store_paths.iter().any(|path| is_store_export(path))
        && !all_staged.iter().any(|path| path == EXPORT)
    {
        return Err(Stop::refused(format!(
            "{EXPORT} is versioned here and did not reach the index — the landing would carry a \
             board older than the item it closes"
        )));
    }
    let staged = outside_store(&all_staged);
    // THE DELIVERY'S OWN PATHS, kept apart from the set the check compares
    // against: the classification below asks whether what landed matches what
    // was reviewed, and an `--also` path is in neither commit's diff by
    // construction, so counting it there would read every landing as CARRIES.
    let delivery_paths = outside_store(
        &wiring
            .git
            .changed_since_merge_base(TRUNK, commit)
            .map_err(Stop::could_not_tell)?,
    );
    let delivered = outside_store(
        &delivery_paths
            .iter()
            .cloned()
            .chain(landing.also.iter().cloned())
            .collect::<Vec<String>>(),
    );
    if staged != delivered {
        let _ = writeln!(out, "STAGED (outside .beads/):");
        for path in &staged {
            let _ = writeln!(out, "  {path}");
        }
        let _ = writeln!(out, "DELIVERED (outside .beads/):");
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
        format!(
            "{} path(s) outside .beads/, {set_description}; {EXPORT} regenerated by the store's \
             own export",
            staged.len()
        ),
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
    let work_dir = landing.machine_dir.join("land").join(&item.id);
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
    let suite_rc = readings.last().map(|reading| reading.rc);

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
            "the push to {}/{TRUNK_BRANCH} exited {} — nothing after it ran: no note, no close, no \
             branch touched; {kept}",
            remote(),
            rc_word(pushed.code)
        )));
    }

    // (j) THE RANGE LINE. The landed sha comes from the push's own output and
    // from nowhere else: a rev-parse answers the local tip, which is what a
    // push that did nothing leaves behind.
    let Some((old, sha)) = range(&pushed.output) else {
        let _ = writeln!(out, "{}", pushed.output.trim_end());
        return Err(Stop::could_not_tell(format!(
            "the push printed no `<old>..<new>  HEAD -> {TRUNK_BRANCH}` line — no record has been \
             written and no ref deleted; {kept}"
        )));
    };

    // The two rows the note carries about the state a landing ends in, read
    // before the note renders them and acted on after it.
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

    // (k) THE LANDING NOTE, written through the store and read back. The
    // landing stands on the trunk whatever this step says.
    let rebased = rebased_from(&delivered_base, &old);
    let tested = match (&suite_command, suite_rc) {
        (Some(command), Some(rc)) => format!("suite: {command}, rc {}", rc_word(rc)),
        _ => format!("{NOT_TESTED}: {UNTESTED}"),
    };
    let note = render(
        &block(&wiring.packs.read(LANDING_NOTE)?)?,
        &[
            ("sha", &sha),
            ("rebased", &rebased),
            ("trunk", TRUNK_BRANCH),
            ("actor", &landed_by),
            ("old", &old),
            ("new", &sha),
            ("commit", commit),
            ("builder", &builder),
            ("tested", &tested),
            (
                "checks",
                &rows.rendered(commit, &sha, work_branch.as_deref()),
            ),
        ],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{LANDING_NOTE}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
        ))
    })?;
    wiring.store.note(&item.id, &note, &actor)?;
    read_back(&item.id, &note, wiring)?;

    // (k2) THE TWO EVENTS, after the note has been written and read back and
    // before anything else — so a crash between them leaves a landing note the
    // stream does not carry, and never a stream that carries a landing no note
    // stands behind. The reading precedes the landing, as it did in time.
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
    announce(
        &item.id,
        ITEM_LANDED,
        &acting,
        wiring,
        serde_json::json!({
            "item": item.id,
            "sha": sha,
            "base": old,
            "squash_of": commit,
            // WHAT CARRIED THIS LANDING, beside the actor whose act it is, and
            // `null` where a seat landed it in its own name: an absent key and
            // a landing no run carried are not the same fact.
            "run": by_run.as_ref().map_or(serde_json::Value::Null, |record| {
                serde_json::Value::String(record.id.clone())
            }),
            // WHAT TESTED IT: the command that ran green on the tree this
            // pushed, or `null` for a landing handed none — the stream's own
            // NOT TESTED, readable without the note.
            "test": suite_command,
        }),
    )?;

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
    // THE CLOSE IS MADE UNDER THE HOLDER'S OWN ASSIGNEE STRING, the closer's
    // bare id, and not its typed form: bd 1.3.0 closes an assigned item only
    // for an actor equal to its assignee — measured, `cannot close X: assignee
    // is "<id>", actor is "seat:<id>"` — and the holder check above has
    // already said the closer is that seat.
    wiring
        .store
        .close(&item.id, &reason, &closer_id)
        .map_err(|e| rerun(&item.id, &sha, &e.to_string()))?;
    let closed = read(wiring.store, &item.id)?;
    if closed.status != "closed" {
        return Err(rerun(
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
        item: item.id,
        sha,
        note,
    })
}

// ---- the suite check and its one rerun ---------------------------------------

/// The reading a landing's own first suite run is.
const FIRST_READING: u64 = 1;
/// The reading the rerun is. There is no third: a second red parks.
const SECOND_READING: u64 = 2;

/// The three words a `check.read` carries under `verdict`, which are the three
/// the landing note's own suite rows print. `RED` reaches the stream only on a
/// reading the landing did NOT stand on — a first red that a green rerun
/// followed, or the pair a second red refuses with.
const GREEN: &str = "green";
const RED: &str = "red";
const NO_READING: &str = "none";

/// One reading of the suite check: which it is, what the child exited, where
/// its log is, and what the wait before it did.
struct Reading {
    n: u64,
    rc: Option<i32>,
    took: String,
    log: PathBuf,
    /// What the rerun's wait for a quiet box ended as. `None` on the first
    /// reading, which waits for nothing.
    waited: Option<String>,
    /// The `PATH` this reading's child ran under. Empty where the caller
    /// constructed none and the child inherited this process's.
    path: String,
}

impl Reading {
    fn green(&self) -> bool {
        self.rc == Some(0)
    }

    fn verdict(&self) -> &'static str {
        if self.green() {
            GREEN
        } else {
            RED
        }
    }

    /// The row's evidence, which is also what the note carries.
    fn evidence(&self, command: &str) -> String {
        let head = format!(
            "`{command}` rc {} in {}, read from the child's own exit; log {}",
            rc_word(self.rc),
            self.took,
            self.log.display()
        );
        match &self.waited {
            None => head,
            Some(waited) => format!("{head}; reading {} — {waited}", self.n),
        }
    }

    fn payload(&self, item: &str, command: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "item": item,
            "suite": command,
            "rc": self.rc,
            "verdict": self.verdict(),
            "reading": self.n,
            "log": self.log.display().to_string(),
            // A suite that failed on the diff and one that failed because it
            // could not find its tools are told apart here and nowhere else.
            "path": if self.path.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::String(self.path.clone())
            },
        })
    }
}

/// The command the landing was handed, run once, and once more where the first
/// read red.
///
/// THE RERUN IS UNCONDITIONAL, and that is Alberto's ruling and not this
/// module's economy: the rerun is owed to an arm the diff did not reach, and
/// the command is one opaque line that reports no arms, so the condition has
/// nothing to read. Both readings go on the record either way, which is what
/// lets a person answer the question the condition would have.
///
/// A SECOND RED REFUSES WITH BOTH LOGS ON STDOUT. That is the whole channel the
/// park needs: the flight's landing act carries what this printed into the
/// hold's question, so the person who meets it reads both tails without this
/// verb knowing a flight exists.
#[allow(clippy::too_many_arguments)]
fn suite_check(
    out: &mut dyn Write,
    rows: &mut Rows,
    command: &Option<String>,
    work_dir: &Path,
    landing: &Landing,
    closer: &Actor,
    wiring: &Wiring,
) -> Result<Vec<Reading>, Stop> {
    let Some(command) = command else {
        rows.read(out, wiring, NOT_TESTED, UNTESTED);
        return Ok(Vec::new());
    };

    let first = read_once(command, work_dir, FIRST_READING, None, wiring)?;
    rows.read(
        out,
        wiring,
        if first.green() { "PASS" } else { "RED" },
        first.evidence(command),
    );
    if first.green() {
        return Ok(vec![first]);
    }

    // THE WAIT, then the rerun. A suite that raced the box is rerun beside a
    // quieter one; on a box that never quietens the wait expires and the rerun
    // runs anyway, saying so on its own row.
    let waited = lane::wait_for_a_quiet_box(
        wiring.load,
        lane::rerun_wait(&wiring.project.guards)?,
        wiring.progress,
    );
    let second = read_once(
        command,
        work_dir,
        SECOND_READING,
        Some(waited.clause()),
        wiring,
    )?;
    rows.read_named(
        out,
        wiring,
        SUITE_RERUN,
        if second.green() { "PASS" } else { "RED" },
        second.evidence(command),
    );
    if second.green() {
        return Ok(vec![first, second]);
    }

    // BOTH READINGS REACH THE STREAM BEFORE THE REFUSAL, and this is the one
    // place this verb writes an event on a path that lands nothing: both
    // readings are owed to the record, and a second red never gets as far as the
    // note the other events wait behind.
    for reading in [&first, &second] {
        announce(
            landing.item,
            CHECK_READ,
            closer,
            wiring,
            reading.payload(landing.item, Some(command)),
        )?;
    }
    for reading in [&first, &second] {
        let _ = writeln!(out, "reading {} — {}", reading.n, reading.log.display());
        for line in tail(&reading.log) {
            let _ = writeln!(out, "  {line}");
        }
    }
    Err(Stop::refused(format!(
        "the suite `{command}` exited {} and, rerun once, {} — the logs are at {} and {}",
        rc_word(first.rc),
        rc_word(second.rc),
        first.log.display(),
        second.log.display()
    )))
}

/// One run of the suite check, into its own log. The second reading's log sits beside
/// the first rather than over it: a reader comparing two reds needs both.
fn read_once(
    command: &str,
    work_dir: &Path,
    n: u64,
    waited: Option<String>,
    wiring: &Wiring,
) -> Result<Reading, Stop> {
    let log = work_dir.join(if n == FIRST_READING {
        "suite.log".to_string()
    } else {
        format!("suite.{n}.log")
    });
    let run = suite(
        command,
        &wiring.project.root,
        wiring.child_path,
        &log,
        wiring.progress,
    )?;
    Ok(Reading {
        n,
        rc: run.code,
        took: run.took,
        log,
        waited,
        path: wiring.child_path.to_string(),
    })
}

/// One event this verb writes. The landing is on the trunk and the note is on
/// the item whatever this says, which is why the failure names both rather than
/// reading as a landing that did not happen.
fn announce(
    item: &str,
    kind: &str,
    closer: &Actor,
    wiring: &Wiring,
    payload: serde_json::Value,
) -> Result<(), Stop> {
    wiring.events.append(kind, closer, payload).map_err(|e| {
        Stop::could_not_tell(format!(
            "{kind} did not reach the stream: {e}\n  the landing on {item} STANDS and its note is \
             on the record"
        ))
    })
}

// ---- putting the tree back ---------------------------------------------------

/// What a refusal before the push has to undo, gathered before the first git
/// write because that is the only moment it can be read.
struct Tree {
    /// Each `--also` path and the bytes it held, `None` where it was not there.
    saved: Vec<(PathBuf, Option<Vec<u8>>)>,
    /// The land branch, once it exists.
    branch: Option<String>,
    /// Whether the push has run. Nothing is put back after it.
    pushed: bool,
}

impl Tree {
    fn of(landing: &Landing, wiring: &Wiring) -> Result<Tree, Stop> {
        let mut saved = Vec::new();
        for path in landing.also {
            let full = wiring.project.root.join(path);
            match std::fs::read(&full) {
                Ok(bytes) => saved.push((full, Some(bytes))),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => saved.push((full, None)),
                // Not usage: the path was named correctly and the INSTRUMENT
                // would not answer, and a refusal that could not read what it
                // has to put back is one that has read nothing.
                Err(e) => {
                    return Err(Stop::could_not_tell(format!(
                        "the --also path {} could not be read: {e}",
                        full.display()
                    )))
                }
            }
        }
        Ok(Tree {
            saved,
            branch: None,
            pushed: false,
        })
    }

    fn put_back(&mut self, err: &mut dyn Write, wiring: &Wiring) {
        if self.pushed {
            return;
        }
        if let Some(branch) = self.branch.take() {
            // THE RESET COMES FIRST. A squash is staged and on disk, and by the
            // time one exists HEAD already names the trunk — so a detach alone
            // succeeds and undoes nothing. The reset is also what puts the
            // store's export back, which is why nothing here restores it by
            // hand.
            if let Err(cause) = wiring.git.reset_hard(TRUNK) {
                let _ = writeln!(err, "the squash could not be put back: {cause}");
            }
            if let Err(cause) = wiring.git.detach(TRUNK) {
                let _ = writeln!(
                    err,
                    "the checkout could not be detached onto {TRUNK}: {cause}"
                );
            }
            if let Err(cause) = wiring.git.delete_branch(&branch) {
                let _ = writeln!(
                    err,
                    "the land branch {branch} could not be removed: {cause}"
                );
            }
        }
        for (path, bytes) in &self.saved {
            let put = match bytes {
                Some(bytes) => std::fs::write(path, bytes),
                // It was not there before, so putting it back is removing it —
                // and a file already gone is the state asked for, not a failure.
                None => match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                    _ => Ok(()),
                },
            };
            if let Err(cause) = put {
                let _ = writeln!(
                    err,
                    "the --also path {} could not be put back: {cause}",
                    path.display()
                );
            }
        }
    }
}

// ---- the rows ----------------------------------------------------------------

/// The check rows, printed as each is read and rendered again into the note.
///
/// A row carries its own criterion NAME rather than taking it from its
/// position, because the rerun adds a row in the middle and a positional name
/// would then print every row after it under its neighbour's heading. The
/// positional count is kept apart for that reason: [`Rows::read`] consumes the
/// next name in [`CRITERIA`], [`Rows::read_named`] consumes none.
struct Rows {
    read: Vec<(&'static str, String, String)>,
    /// How many of [`CRITERIA`] have been used.
    positional: usize,
}

impl Rows {
    fn new() -> Rows {
        Rows {
            read: Vec::new(),
            positional: 0,
        }
    }

    /// One criterion read: its row on stdout, one step of the bar, and the
    /// reading kept for the note.
    fn read(
        &mut self,
        out: &mut dyn Write,
        wiring: &Wiring,
        verdict: &str,
        evidence: impl Into<String>,
    ) {
        let named = criterion(self.positional);
        self.positional += 1;
        self.read_named(out, wiring, named, verdict, evidence);
    }

    /// A row whose criterion is not one of [`CRITERIA`]'s and does not consume
    /// one: the suite's second reading is the only one there is.
    fn read_named(
        &mut self,
        out: &mut dyn Write,
        wiring: &Wiring,
        named: &'static str,
        verdict: &str,
        evidence: impl Into<String>,
    ) {
        let evidence = evidence.into();
        let n = self.read.len();
        let _ = writeln!(out, "{}", row(n + 1, named, verdict, &evidence));
        self.read.push((named, verdict.to_string(), evidence));
        wiring.progress.row();
    }

    /// The rows the note carries, and the commands that re-run each verdict.
    fn rendered(&self, commit: &str, sha: &str, branch: Option<&str>) -> String {
        let mut lines: Vec<String> = self
            .read
            .iter()
            .enumerate()
            .map(|(n, (named, verdict, evidence))| row(n + 1, named, verdict, evidence))
            .collect();
        lines.push(String::new());
        lines.push("## Commands — every verdict above, re-runnable".to_string());
        lines.push(format!("git merge-base {TRUNK} {commit}"));
        lines.push(format!(
            "git diff --name-only $(git merge-base {TRUNK} {commit}) {commit}"
        ));
        lines.push(format!("git show --stat {sha}"));
        lines.push(format!("git rev-list --count {sha}..{TRUNK}"));
        if let Some(branch) = branch {
            lines.push(format!("git rev-parse {branch}"));
            lines.push(format!("git diff {commit} {sha} --"));
        }
        lines.push("git status --porcelain".to_string());
        lines.join("\n")
    }
}

/// One criterion row. The leading `<n>. ` is what a read-back counts, so a row
/// is a number at the start of a line and nothing else is. `fleet item show`
/// prints a landing entry's check rows through this same line.
pub(crate) fn row(n: usize, criterion: &str, verdict: &str, evidence: &str) -> String {
    format!("{n}. {criterion:<16} {verdict:<10} {evidence}")
}

/// The name of the nth criterion. A row past the list is named rather than
/// panicked on: this module states the count and a mismatch is a defect in it,
/// not a reason to take a landing down mid-push.
fn criterion(n: usize) -> &'static str {
    CRITERIA.get(n).copied().unwrap_or("(unnamed)")
}

// ---- the work branch ---------------------------------------------------------

/// What the work branch holds that the trunk does not.
enum Classification {
    /// Its tip is the reviewed commit and the delivered content is on the trunk
    /// byte for byte.
    Safe,
    /// Its tip is past the reviewed commit, by this many commits.
    Ahead(u64),
    /// The delivered content differs from what landed, starting at this path.
    Carries(String),
    /// A read that would not answer.
    CouldNotTell(String),
    /// The delivery named no branch, so there is nothing to classify.
    NotGiven,
}

impl Classification {
    fn verdict(&self) -> &'static str {
        match self {
            Classification::Safe => SAFE,
            Classification::Ahead(_) => "CARRIES UNLANDED WORK",
            Classification::Carries(_) => "CARRIES",
            Classification::CouldNotTell(_) => "COULD NOT TELL",
            Classification::NotGiven => "NOT GIVEN",
        }
    }

    fn evidence(&self, branch: Option<&str>) -> String {
        let named = branch.unwrap_or("(none)");
        match self {
            Classification::Safe => {
                format!("{named} — its tip is the reviewed commit and the landed diff is empty")
            }
            Classification::Ahead(n) => format!(
                "{named} — its tip is {n} commit(s) past the reviewed commit; nothing is deleted"
            ),
            Classification::Carries(path) => format!(
                "{named} — the landed content differs from the delivery at `{path}`; nothing is \
                 deleted"
            ),
            Classification::CouldNotTell(cause) => {
                format!("{named} — {cause}; nothing is deleted")
            }
            Classification::NotGiven => {
                "the delivery names no branch — there is no work branch to classify".to_string()
            }
        }
    }
}

/// The names a delete may never be aimed at, and the reason if this is one.
///
/// The branch comes off the record — a delivery's entry, or the landing note a
/// retire reads — which is text somebody wrote, and SAFE ends in `git branch
/// -D` and `git push --delete`. Three refs would take a trunk or this act's own
/// working branch with them, and a leading `-` is a name git would read as an
/// option wherever a `--` were ever dropped.
fn unsafe_to_delete(branch: &str, land_branch: &str) -> Option<String> {
    let named = |what: &str| Some(format!("the branch is `{branch}`, which is {what}"));
    match branch {
        TRUNK_BRANCH => named("the trunk"),
        "HEAD" => named("HEAD"),
        _ if branch == land_branch => named("this landing's own branch"),
        _ if branch.starts_with(LAND_PREFIX) => {
            named("a landing's own branch and not a delivery's")
        }
        _ if branch == TRUNK => named("the trunk's remote ref"),
        _ if branch.starts_with('-') => named("a name git would read as an option"),
        _ => None,
    }
}

// ---- what a retire reads back off the note ----------------------------------

/// What a landing's own note leaves a later act to do with the work branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    /// The landing classified this branch [`SAFE`] and the seat that is going
    /// is standing on it. Deleting it finishes what the landing could not.
    Delete(String),
    /// Nothing is deleted, and this says why in the words a person reads.
    Keep(String),
}

/// What is to be done with `held` — the branch the retiring seat's worktree is
/// on — given that seat's item's notes.
///
/// A landing runs while the seat that delivered still holds its worktree, so a
/// SAFE branch's LOCAL delete exits 1 there and the ref outlives the landing.
/// The retire that takes that worktree is the act that can finish it, and this
/// is what it reads: the classification the landing ALREADY MADE, off the note
/// it wrote. Nothing here re-derives one — a second classifier would be a second
/// thing that can say SAFE where the first said CARRIES.
///
/// EVERY ANSWER BUT ONE IS `Keep`, and that is the shape rather than the mood.
/// The single delete needs all of: a landing on the record, a work-branch row
/// inside it, [`SAFE`] as that row's verdict, a branch name in that row's
/// evidence, that name passing the same refusal list the landing's own delete
/// passes, and the going seat's worktree standing on exactly that name. A
/// reading that is absent, unparsable or disagrees keeps the branch, because an
/// over-eager delete here takes work nobody can get back.
pub fn release(notes: &str, held: Option<&str>) -> Release {
    let Some(held) = held.map(str::trim).filter(|name| !name.is_empty()) else {
        return Release::Keep("the retiring seat's worktree is on no branch".to_string());
    };
    let Some(landing) = last_landing(notes) else {
        return Release::Keep(format!("`{held}` — the item carries no landing"));
    };
    let Some(row) = work_branch_row(&landing) else {
        return Release::Keep(format!(
            "`{held}` — the landing carries no `{}` row",
            CRITERIA[WORK_BRANCH]
        ));
    };
    // THE VERDICT IS THE ROW'S FIRST FIELD and [`SAFE`] is the only one this
    // acts on, so the strip is the check: what follows it is the evidence, which
    // opens with the branch the landing classified. A row that does not open
    // with SAFE is reported back whole, verdict and all — the words a person
    // needs here are the ones the landing itself wrote.
    let Some(evidence) = row.strip_prefix(SAFE).filter(|rest| rest.starts_with(' ')) else {
        return Release::Keep(format!("`{held}` — the landing reads `{}`", clause(&row)));
    };
    let named = clause(evidence);
    if named != held {
        return Release::Keep(format!(
            "`{held}` — the landing's {SAFE} row names `{named}`, which is another branch"
        ));
    }
    // THE SAME REFUSAL LIST THE LANDING'S OWN DELETE PASSES, asked again here
    // because this reads a note rather than the value the landing classified:
    // there is no land branch at a retire, and no ordinary work branch is named
    // by the empty string.
    if let Some(why) = unsafe_to_delete(named, "") {
        return Release::Keep(format!("`{held}` — {why}"));
    }
    Release::Delete(named.to_string())
}

/// The landing's own work-branch row, from its verdict on: a row is a number at
/// the start of a line and nothing else is, which is what [`row`] writes.
///
/// NOTHING IS SPLIT ON WHITESPACE PAST THE CRITERION. The verdicts run to one,
/// two, three and four words — `SAFE`, `COULD NOT TELL`, `CARRIES UNLANDED
/// WORK` — so a reader that took a field here would need a second copy of the
/// list [`Classification::verdict`] writes, and a list that fell behind would
/// read the wrong text as a branch name.
fn work_branch_row(landing: &str) -> Option<String> {
    let criterion = CRITERIA[WORK_BRANCH];
    landing.lines().find_map(|line| {
        let (n, rest) = line.split_once(". ")?;
        if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let rest = rest.strip_prefix(criterion)?;
        if !rest.starts_with(' ') {
            return None;
        }
        Some(rest.trim_start().to_string())
    })
}

/// What a row says before its first em dash: the evidence's own subject, which
/// on a SAFE row is the branch and on every other is the verdict and the branch
/// together.
fn clause(text: &str) -> &str {
    text.split(" — ").next().unwrap_or_default().trim()
}

/// SAFE is two readings and not one: the tip has not moved past what was
/// reviewed, AND the delivered paths are byte-identical between the reviewed
/// commit and what landed. Either alone would delete a branch holding work.
fn classify(
    commit: &str,
    landed: &str,
    delivered: &[String],
    branch: Option<&str>,
    land_branch: &str,
    wiring: &Wiring,
) -> Classification {
    let Some(branch) = branch else {
        return Classification::NotGiven;
    };
    // THE NAME IS NOTE TEXT, and SAFE ends in two destructive git calls. A
    // value that names the trunk, HEAD or this act's own land branch is
    // refused here rather than classified, and one that could parse as an
    // option is refused for the same reason the calls below pass `--`.
    if let Some(why) = unsafe_to_delete(branch, land_branch) {
        return Classification::CouldNotTell(why);
    }
    let tip = match wiring.git.rev(branch) {
        Ok(Some(tip)) => tip,
        Ok(None) => {
            return Classification::CouldNotTell(format!("`{branch}` resolves to no commit"))
        }
        Err(cause) => return Classification::CouldNotTell(cause),
    };
    if tip != commit {
        return match wiring.git.behind(commit, branch) {
            Ok(0) => Classification::CouldNotTell(format!(
                "its tip {tip} is not the reviewed commit and is not ahead of it"
            )),
            Ok(n) => Classification::Ahead(n),
            Err(cause) => Classification::CouldNotTell(cause),
        };
    }
    match wiring.git.diff_paths(commit, landed, delivered) {
        Ok(paths) => match paths.first() {
            None => Classification::Safe,
            Some(path) => Classification::Carries(path.clone()),
        },
        Err(cause) => Classification::CouldNotTell(cause),
    }
}

// ---- the record ---------------------------------------------------------------

/// The commit the last verdict accepted, as a full sha, against the one this
/// landing was given.
fn accepted_commit(item: &str, notes: &str, commit: &str, wiring: &Wiring) -> Result<String, Stop> {
    let Some(verdict) = last_verdict(notes) else {
        return Err(Stop::refused(format!(
            "{item} carries no verdict — a landing lands a review and there is none to land"
        )));
    };
    let first = verdict.lines().next().unwrap_or_default();
    if opens_with(first, &[VERDICT_MARKERS[1]]) {
        return Err(Stop::refused(format!(
            "the last verdict on {item} is `{}` — the work went back and nothing has accepted it \
             since",
            VERDICT_MARKERS[1]
        )));
    }
    let named = first
        .strip_prefix(VERDICT_MARKERS[0])
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default();
    let resolved = match wiring.git.rev(named).map_err(Stop::could_not_tell)? {
        Some(sha) => sha,
        None => {
            return Err(Stop::could_not_tell(format!(
                "the last verdict on {item} names `{named}`, which resolves to no commit here"
            )))
        }
    };
    if resolved != commit {
        return Err(Stop::refused(format!(
            "the last verdict on {item} accepts {resolved} and this landing was given {commit} — a \
             landing lands the commit the review read"
        )));
    }
    Ok(resolved)
}

/// The clause the landing note's first line gains when the delivery was BEHIND
/// the trunk it landed on.
///
/// Under `advance` the squash onto a land branch cut at the fresh trunk tip IS
/// the rebase — the landed sha is the rebased commit — so what is owed
/// beyond what already happens is the RECORD: the base the delivery was cut
/// from, beside the base it landed on, whenever the two differ. Equal bases are
/// a delivery that was current, and the clause is empty there rather than
/// saying so twice.
///
/// The delivery's base is a whole sha, which the delivered entry cannot be
/// written without. The comparison is still by prefix either way, because the
/// sha it is compared with is read off the push's own range line, which git
/// abbreviates — until the landing records that one whole too.
fn rebased_from(base: &str, landed_on: &str) -> String {
    if landed_on.starts_with(base) || base.starts_with(landed_on) {
        return String::new();
    }
    format!("; rebased from {base}")
}

/// The record of the run that called this landing.
///
/// A workflow calls every verb as `run:<id>`, and the `fleet:run` label on the
/// item the store answers for that id is the only mark that tells a run's
/// record from every other item — the same discriminator `hold` reads to tell
/// a run's park from a seat's. An id the store holds no item for, or one whose
/// item carries no label, is refused: the actor said it was a run, and it is
/// not one. A store that could not answer is a could-not-tell.
fn run_record(store: &dyn Store, by: &Actor) -> Result<Item, Stop> {
    let named_no_run = || Stop::refused(format!("{by} names no run record"));
    match store.show(&by.id) {
        Ok(record) if record.labels.iter().any(|label| label == run::LABEL) => Ok(record),
        Ok(_) | Err(StoreError::Missing(_)) => Err(named_no_run()),
        // A read never answers `Moved`, which only a fenced write does.
        Err(StoreError::Unreadable(why) | StoreError::Moved(why)) => {
            Err(Stop::could_not_tell(format!(
                "the store could not say whether `{}` is a run's record: {why} — a run's landing \
                 acts as the reviewer, and this is where its record is read",
                by.id
            )))
        }
    }
}

/// The refusal of an actor that is neither a seat nor a run.
fn neither(by: &Actor) -> Stop {
    Stop::refused(format!(
        "fleet land acts as a seat or as a run — {by} is neither"
    ))
}

/// The reviewer's own answer to the hold this run raised, which is the whole
/// licence for a run to land anything.
///
/// The hold is raised on the RUN's record and cleared there, so that record is
/// where the answer is read; the landing the answer licenses is on the item.
/// Whoever answered is read as the typed actor `clear` signed with, and
/// licenses the landing only where it is a seat and that seat is the reviewer:
/// an answer signed by any other seat, by another kind, or with text that is
/// no typed actor at all is refused, naming what it says.
fn licensed(record: &Item, reviewer: SeatId, seats: &Directory) -> Result<(), Stop> {
    let wanted = seats.label(&reviewer);
    let notes = record.notes.clone().unwrap_or_default();
    let Some(answer) = last_answer(&notes) else {
        return Err(Stop::refused(format!(
            "run {} carries no cleared hold — a run lands what `{wanted}` answered for, and \
             nobody has answered this run anything",
            record.id
        )));
    };
    let Some(who) = after_dash(&answer) else {
        return Err(Stop::could_not_tell(format!(
            "run {}'s last answer names nobody after its em dash — who answered is what licenses \
             the landing",
            record.id
        )));
    };
    let answered = Actor::typed(&who).and_then(Result::ok);
    if answered.as_ref().and_then(Actor::seat_id) == Some(reviewer) {
        return Ok(());
    }
    let who = answered.map_or(who, |actor| actor.to_string());
    Err(Stop::refused(format!(
        "run {}'s last hold was cleared by `{who}` and not by `{wanted}` — a run lands as the \
         `[core] reviewer` and on that seat's own answer",
        record.id
    )))
}

/// What a note's own first line names after its em dash: the seat on a
/// delivery, the reviewer on a verdict.
fn after_dash(region: &str) -> Option<String> {
    let first = region.lines().next()?;
    let (_, who) = first.split_once(" — ")?;
    let who = who.trim();
    (!who.is_empty()).then(|| who.to_string())
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

/// One read, asserting the landing region against the text this verb rendered —
/// plus a token nothing wrote.
fn read_back(item: &str, note: &str, wiring: &Wiring) -> Result<(), Stop> {
    let read = read(wiring.store, item)?;
    let seen = read.notes.as_deref().and_then(last_landing);
    if seen.as_deref().map(normalised) != Some(normalised(note)) {
        return Err(Stop::could_not_tell(format!(
            "{item} read back with its last landing ==\n{}\n  wanted:\n{note}\n  the landing \
             STANDS on {TRUNK_BRANCH}\n  RERUN: bd note {item} \"<the note, as it is printed \
             above>\"",
            seen.as_deref().unwrap_or("(absent)")
        )));
    }
    let control = control_token();
    if read.document.contains(control) {
        return Err(Stop::could_not_tell(format!(
            "the read-back on {item} carries {control}, which nothing wrote — the read is not \
             reading this item"
        )));
    }
    Ok(())
}

fn rerun(item: &str, sha: &str, why: &str) -> Stop {
    Stop::could_not_tell(format!(
        "{item} did not close: {why}\n  the landing {sha} STANDS on {TRUNK_BRANCH}\n  RERUN: bd \
         close {item} --reason \"landed {sha}\""
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

// ---- the two children ---------------------------------------------------------

/// A command in the project root, with these lines on its stdin. Its trimmed
/// stdout is the answer; a non-zero exit is an instrument that would not answer.
fn stdin_command(
    command: &str,
    root: &Path,
    child_path: &str,
    lines: &[String],
) -> Result<String, Stop> {
    let mut child = shell(command, root, child_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Stop::could_not_tell(format!("`{command}` could not be run: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let mut body = lines.join("\n");
        body.push('\n');
        let _ = stdin.write_all(body.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| Stop::could_not_tell(format!("`{command}` could not be read: {e}")))?;
    if !out.status.success() {
        return Err(Stop::could_not_tell(format!(
            "`{command}` exited {}: {}",
            rc_word(out.status.code()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// What the suite did.
struct Ran {
    code: Option<i32>,
    took: String,
}

/// The suite as a child of this act, with no deadline of the verb's own: the
/// project decides how long its own suite takes. Its rc is read from the child's
/// own exit and never from anything it printed.
fn suite(
    command: &str,
    root: &Path,
    child_path: &str,
    log: &Path,
    progress: &dyn Progress,
) -> Result<Ran, Stop> {
    let file = std::fs::File::create(log).map_err(|e| {
        Stop::could_not_tell(format!(
            "the suite's log at {} could not be opened: {e}",
            log.display()
        ))
    })?;
    let both = file.try_clone().map_err(|e| {
        Stop::could_not_tell(format!(
            "the suite's log could not be shared with stderr: {e}"
        ))
    })?;
    let started = std::time::Instant::now();
    let mut child = shell(command, root, child_path)
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(both))
        .spawn()
        .map_err(|e| Stop::could_not_tell(format!("`{command}` could not be run: {e}")))?;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                progress.message(&format!("{command} — {} line(s)", lines_in(log)));
                std::thread::sleep(TICK);
            }
            Err(e) => {
                return Err(Stop::could_not_tell(format!(
                    "`{command}` could not be waited on: {e}"
                )))
            }
        }
    };
    Ok(Ran {
        code: status.code(),
        took: format!("{:.1}s", started.elapsed().as_secs_f64()),
    })
}

fn shell(command: &str, root: &Path, child_path: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(root);
    if !child_path.is_empty() {
        cmd.env("PATH", child_path);
    }
    cmd
}

fn lines_in(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

fn tail(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL)..]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

// ---- the readings -------------------------------------------------------------

/// The one reading of the landed sha, and of the range's other end: the line
/// git's own push prints, and nothing else.
fn range(output: &str) -> Option<(String, String)> {
    let tail = format!("HEAD -> {TRUNK_BRANCH}");
    output.lines().find_map(|line| {
        let line = line.trim();
        if !line.ends_with(&tail) {
            return None;
        }
        let (span, _) = line.split_once(char::is_whitespace)?;
        let (old, new) = span.split_once("..")?;
        (is_hex(old) && is_hex(new)).then(|| (old.to_string(), new.to_string()))
    })
}

fn is_hex(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The first path `git status --porcelain` reports that `--also` did not admit.
/// A landing squashes a commit, so every working-tree change is outside it but
/// the ones the reviewer named — and the store's export, which this verb
/// rewrites itself between its own first refusal and the re-run after it.
///
/// THE EXPORT AND NOT THE DIRECTORY: a refusal from (e) on hard-resets the
/// tree, so a path this check lets past is a path that reset discards without a
/// word. A tracked `.beads/hooks/*` or `.beads/config.json` edit is a seat's
/// work and refuses here like any other.
fn outside_also(status: &[String], also: &[String]) -> Option<String> {
    status
        .iter()
        .map(|line| porcelain_path(line))
        .find(|path| !also.contains(path) && !is_store_export(path))
}

/// The store paths a landing rewrites itself, in both spellings the porcelain
/// uses for them: the export by name, and the store's whole directory, which is
/// what an untracked-but-unignored store answers with.
fn is_store_export(path: &str) -> bool {
    path == EXPORT || path == STORE_DIR
}

/// The path a porcelain line names. A rename prints `old -> new`, and a path
/// holding a space, a quote or a backslash is printed QUOTED — so a quoted one
/// is read back to the bytes it names, or a reviewer could never admit it with
/// `--also` and every landing in that tree would refuse.
fn porcelain_path(line: &str) -> String {
    let rest = line.get(3..).unwrap_or_default().trim();
    let named = match rest.rsplit_once(" -> ") {
        Some((_, new)) => new,
        None => rest,
    };
    unquoted(named)
}

/// git's own C-style quoting, undone: the surrounding quotes, the named escapes
/// it writes inside them, and the three-digit OCTAL it writes for every byte
/// outside printable ASCII. A value that is not quoted is its own answer.
///
/// The octal is the half a reviewer needs: under the default `core.quotePath`,
/// a path holding one non-ASCII character reaches the porcelain as `\303\251`
/// and nothing else, so a decoder that only undoes `\\x` hands back a name no
/// `--also` value can ever equal.
fn unquoted(name: &str) -> String {
    let Some(inner) = name
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return name.to_string();
    };
    // BYTES AND NOT CHARS: an octal run spells one byte of a multi-byte
    // character, and only the whole run is text again.
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut rest = inner.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte != b'\\' {
            out.push(byte);
            rest = tail;
            continue;
        }
        let Some((&escaped, after)) = tail.split_first() else {
            rest = tail;
            continue;
        };
        rest = after;
        match escaped {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'0'..=b'7' => match octal(tail) {
                Some(byte) => {
                    out.push(byte);
                    rest = &tail[3..];
                }
                None => out.push(escaped),
            },
            other => out.push(other),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The byte a three-digit octal run spells, or `None` where the run is short,
/// not octal, or names a value no byte holds.
fn octal(digits: &[u8]) -> Option<u8> {
    let run = digits.get(..3)?;
    if !run.iter().all(|d| (b'0'..=b'7').contains(d)) {
        return None;
    }
    u8::try_from(
        run.iter()
            .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0')),
    )
    .ok()
}

/// A path set with the store's own directory taken out, sorted and deduplicated
/// so the two sides of the staged-set check are compared as sets.
fn outside_store(paths: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = paths
        .iter()
        .filter(|path| !path.starts_with(STORE_DIR))
        .cloned()
        .collect();
    kept.sort();
    kept.dedup();
    kept
}

/// A file as three independent readings — when it was written, how long it is,
/// and what is in it. `None` is a file that is not there, which is a fourth
/// answer and not a fourth reading.
///
/// Three because one is not enough: a filesystem whose mtime is coarser than
/// this act reads two writes in a tick as one, and a rewrite that happens to
/// keep the length is not a rewrite this verb may miss. The content is hashed
/// rather than kept, because the question is only whether two readings of one
/// path differ.
fn fingerprint(path: &Path) -> Option<(std::time::SystemTime, u64, u64)> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((meta.modified().ok()?, meta.len(), hasher.finish()))
}

fn rc_word(code: Option<i32>) -> String {
    match code {
        Some(code) => code.to_string(),
        None => "on a signal".to_string(),
    }
}

/// The template's note block, in this verb's own words.
fn block(template: &str) -> Result<String, Stop> {
    marker_block(template, LANDING_MARKERS[0]).ok_or_else(|| {
        Stop::could_not_tell(format!(
            "`{LANDING_NOTE}` carries no `{}` block — the pack's landing grammar names one",
            LANDING_MARKERS[0]
        ))
    })
}

fn normalised(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn read(store: &dyn Store, item: &str) -> Result<Item, Stop> {
    store.show(item).map_err(Stop::from)
}
