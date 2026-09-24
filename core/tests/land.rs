//! `fleet land` against a real work graph and a git seam that answers.
//!
//! One store for the whole binary and one item per arm, as the deliver suite
//! has it: `bd` serialises against itself on this box. The git seam is a stub
//! with recorded calls rather than a repository — what land does with a
//! conflicted squash, a rejected push and a push that printed no range line is
//! one seam value away here, and each of those is a state a real repository
//! will not enter on demand. The live path is proven against a real bare remote
//! in the cli's own suite.
//!
//! THE STORE IS REAL. The note, the close and their read-backs go through `bd`,
//! because the one failure this verb has to survive — a write whose read-back
//! disagrees — is asserted against the store the verb actually talks to, and
//! forced through a wrapper over it.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{keys_agree, shared_store, Rooted, Scratch, StubEvents};
use fleet_core::item::brief::{Packs, DELIVERY_NOTE};
use fleet_core::item::land::{
    self, LandGit, Landed, Landing, Progress, Pushed, Squashed, Wiring, CRITERIA, LANDING_NOTE,
    REBASE_NEEDED, SUITE_RERUN,
};
use fleet_core::item::lane;
use fleet_core::item::review::VERDICT;
use fleet_core::item::{
    control_token, last_delivery, last_landing, marker_block, render, review::last_verdict, Change,
    Git, Project, Stop, CHECK_READ, ITEM_LANDED, LANDING_MARKERS, TRUNK, TRUNK_BRANCH,
    VERDICT_MARKERS,
};
use fleet_core::store::{AssignedItem, Bd, Item, Store, StoreError};
use fleet_core::test_support::Board;

const REVIEWER: &str = "a-reviewer";
const BUILDER: &str = "a-builder";
const WORK: &str = "a-builder/feat/the-work";
const SHA: &str = "1111111111111111111111111111111111111111";
const OTHER: &str = "3333333333333333333333333333333333333333";
const LANDED: &str = "4444444444444444444444444444444444444444";
const OLD: &str = "5555555555555555555555555555555555555555";
/// What the LOCAL commit answers. A real push makes the two equal, so the stub
/// is deliberately built as the case where they must diverge: a verb that read
/// the landed sha off a rev-parse instead of off the push's own range line
/// reports this one, and every arm below would still be green if they agreed.
const COMMITTED: &str = "6666666666666666666666666666666666666666";
const FILE: &str = "a/file.rs";
/// The clock the caller hands in. It stamps the lane's lock, so an arm that
/// reads the lock file reads this back.
const AT: &str = "2026-09-13T00:00:00Z";

/// A project with a marker command that prints one word over whatever it is
/// handed, and the reviewer a landing handed the primary resolves a worktree
/// for. The reviewer is [`REVIEWER`], spelled out because a const carries no
/// format, and the arm that reads it says so.
const POLICY: &str =
    "[landing]\nci_marker = \"printf '[skip ci]'\"\n\n[core]\nreviewer = \"a-reviewer\"\n";

/// The test command every landing below is handed unless its arm hands its
/// own, as `fleet land --test` would be: one that answers at once.
const TEST: &str = "exit 0";

fn push_out(old: &str, new: &str) -> String {
    format!("To an-example\n   {old}..{new}  HEAD -> {TRUNK_BRANCH}\n")
}

// ---- the seams ---------------------------------------------------------------

/// The git a landing reads and writes through, as recorded calls and canned
/// answers. Everything an arm varies is a field.
struct StubGit {
    linked: bool,
    /// One answer per `status` call, in order, then [`StubGit::status`] for
    /// every call after: the verb reads the porcelain three times and at three
    /// different moments — the tree gate, the store's own changes after the
    /// export, and the tree after the push — and an arm that answered the same
    /// thing to all three could not tell the three apart.
    statuses: Mutex<std::collections::VecDeque<Vec<String>>>,
    status: Vec<String>,
    staged: Vec<String>,
    delivered: Vec<String>,
    squash: Mutex<Option<Squashed>>,
    behind: u64,
    push: Pushed,
    /// The branch tip, where an arm moves it off the reviewed commit.
    tip: String,
    /// What `diff_paths` finds between the reviewed commit and what landed.
    carried: Vec<String>,
    /// A revision that answers nothing, for the could-not-tell arm.
    blind: Option<String>,
    /// The branch whose LOCAL delete refuses the way git refuses one a worktree
    /// still has checked out — the transient flow's ordinary shape, where the
    /// seat that delivered is still up at land time.
    held: Option<String>,
    /// A SECOND ref that resolves to [`StubGit::tip`], so an arm can hang a
    /// dangerous NAME on a branch the classifier would otherwise call SAFE.
    /// Without it every unusual name reaches could-not-tell through the
    /// resolution and an arm asserting no delete proves nothing about the name.
    reviewed_too: Option<String>,
    /// Whether the checkout refuses to detach, for the arm about what a
    /// landing already on the trunk does with a git that will not answer.
    detach_fails: bool,
    /// What the tree this stub is DRIVEN INTO answers about being linked, for
    /// the arm where the seat table names a tree that is a primary too.
    driven_linked: bool,
    /// SHARED with every stub this one is driven into: a landing resolved
    /// elsewhere records its acts on the list the arm holds, in one order.
    calls: Arc<Mutex<Vec<String>>>,
}

impl StubGit {
    /// A linked worktree with a clean tree, the delivered file staged beside the
    /// store's export, a squash that applies and a push that landed.
    fn clean() -> StubGit {
        StubGit {
            linked: true,
            // The tree gate sees a clean tree; the read after the export sees
            // the one file the export rewrote, in a project that versions it.
            statuses: Mutex::new([Vec::new(), vec![" M .beads/issues.jsonl".to_string()]].into()),
            status: Vec::new(),
            staged: vec![".beads/issues.jsonl".to_string(), FILE.to_string()],
            delivered: vec![FILE.to_string()],
            squash: Mutex::new(Some(Squashed::Done)),
            behind: 0,
            push: Pushed {
                output: push_out(OLD, LANDED),
                code: Some(0),
            },
            tip: SHA.to_string(),
            carried: Vec::new(),
            blind: None,
            held: None,
            reviewed_too: None,
            detach_fails: false,
            driven_linked: true,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("not poisoned").clone()
    }

    /// This stub as a landing drives it at another tree: the same canned
    /// answers, the same call list, and whatever [`StubGit::driven_linked`]
    /// says that tree is. Both trees' acts reach one list in the order they
    /// happened, split by the `at` that handed the landing over, so an arm
    /// reads which tree each act ran in without a second recorder.
    fn driven(&self) -> StubGit {
        StubGit {
            linked: self.driven_linked,
            statuses: Mutex::new(self.statuses.lock().expect("not poisoned").clone()),
            status: self.status.clone(),
            staged: self.staged.clone(),
            delivered: self.delivered.clone(),
            squash: Mutex::new(self.squash.lock().expect("not poisoned").clone()),
            behind: self.behind,
            push: self.push.clone(),
            tip: self.tip.clone(),
            carried: self.carried.clone(),
            blind: self.blind.clone(),
            held: self.held.clone(),
            reviewed_too: self.reviewed_too.clone(),
            detach_fails: self.detach_fails,
            driven_linked: self.driven_linked,
            calls: Arc::clone(&self.calls),
        }
    }

    fn record(&self, call: &str) {
        self.calls
            .lock()
            .expect("not poisoned")
            .push(call.to_string());
    }
}

impl Git for StubGit {
    fn current_branch(&self) -> Result<String, String> {
        self.record("current_branch");
        Ok(WORK.to_string())
    }

    fn head(&self) -> Result<String, String> {
        self.record("head");
        Ok(COMMITTED.to_string())
    }

    fn trunk_tip(&self) -> Result<String, String> {
        self.record("trunk_tip");
        Ok(OLD.to_string())
    }

    fn staged(&self) -> Result<Vec<String>, String> {
        self.record("staged");
        Ok(self.staged.clone())
    }

    fn status(&self) -> Result<Vec<String>, String> {
        self.record("status");
        Ok(self
            .statuses
            .lock()
            .expect("not poisoned")
            .pop_front()
            .unwrap_or_else(|| self.status.clone()))
    }

    fn add_all(&self) -> Result<(), String> {
        self.record("add_all");
        Ok(())
    }

    fn commit(&self, message: &str) -> Result<String, String> {
        self.record(&format!("commit {message}"));
        Ok(COMMITTED.to_string())
    }

    fn numstat(&self, from: &str, to: &str) -> Result<Vec<Change>, String> {
        self.record(&format!("numstat {from} {to}"));
        Ok(Vec::new())
    }
}

impl LandGit for StubGit {
    fn at(&self, root: &Path) -> Box<dyn LandGit + '_> {
        self.record(&format!("at {}", root.display()));
        Box::new(self.driven())
    }

    fn is_linked_worktree(&self) -> Result<bool, String> {
        self.record("is_linked_worktree");
        Ok(self.linked)
    }

    fn fetch(&self, remote: &str) -> Result<(), String> {
        self.record(&format!("fetch {remote}"));
        Ok(())
    }

    fn branch_at(&self, branch: &str, at: &str) -> Result<(), String> {
        self.record(&format!("branch_at {branch} {at}"));
        Ok(())
    }

    fn squash_merge(&self, commit: &str) -> Result<Squashed, String> {
        self.record(&format!("squash_merge {commit}"));
        Ok(self
            .squash
            .lock()
            .expect("not poisoned")
            .clone()
            .unwrap_or(Squashed::Done))
    }

    fn add(&self, paths: &[String]) -> Result<(), String> {
        self.record(&format!("add {}", paths.join(" ")));
        Ok(())
    }

    fn changed_since_merge_base(&self, base: &str, commit: &str) -> Result<Vec<String>, String> {
        self.record(&format!("changed_since_merge_base {base} {commit}"));
        Ok(self.delivered.clone())
    }

    fn diff_paths(&self, from: &str, to: &str, paths: &[String]) -> Result<Vec<String>, String> {
        self.record(&format!("diff_paths {from} {to} {}", paths.join(" ")));
        Ok(self.carried.clone())
    }

    fn commit_message_file(&self, message: &Path) -> Result<String, String> {
        let body = std::fs::read_to_string(message).expect("the message file is written first");
        self.record(&format!("commit_message_file {}", one_line(&body)));
        Ok(COMMITTED.to_string())
    }

    fn behind(&self, what: &str, of: &str) -> Result<u64, String> {
        self.record(&format!("behind {what} {of}"));
        // The classification asks the same question of the work branch, and its
        // answer is how far the tip has moved past the reviewed commit.
        if what == SHA {
            let ahead = u64::from(self.tip != SHA);
            return Ok(ahead);
        }
        Ok(self.behind)
    }

    fn push_head(&self, remote: &str, branch: &str) -> Result<Pushed, String> {
        self.record(&format!("push_head {remote} {branch}"));
        Ok(self.push.clone())
    }

    fn rev(&self, rev: &str) -> Result<Option<String>, String> {
        self.record(&format!("rev {rev}"));
        if self.blind.as_deref() == Some(rev) {
            return Ok(None);
        }
        if rev == WORK || self.reviewed_too.as_deref() == Some(rev) {
            return Ok(Some(self.tip.clone()));
        }
        Ok(Some(rev.to_string()))
    }

    fn delete_branch(&self, branch: &str) -> Result<(), String> {
        self.record(&format!("delete_branch {branch}"));
        if self.held.as_deref() == Some(branch) {
            return Err(format!(
                "error: cannot delete branch '{branch}' used by worktree at /a/worktree"
            ));
        }
        Ok(())
    }

    fn delete_remote_branch(&self, remote: &str, branch: &str) -> Result<(), String> {
        self.record(&format!("delete_remote_branch {remote} {branch}"));
        Ok(())
    }

    fn detach(&self, at: &str) -> Result<(), String> {
        self.record(&format!("detach {at}"));
        if self.detach_fails {
            return Err(String::from("the checkout is held by something else"));
        }
        Ok(())
    }

    fn reset_hard(&self, at: &str) -> Result<(), String> {
        self.record(&format!("reset_hard {at}"));
        Ok(())
    }
}

fn one_line(text: &str) -> String {
    text.lines().collect::<Vec<_>>().join(" / ")
}

/// Everything a landing note carries after its own first line: what the
/// template writes as {checks}.
fn checks_of(note: &str) -> String {
    note.lines().skip(1).collect::<Vec<_>>().join("\n")
}

/// The progress surface, counted rather than drawn.
#[derive(Default)]
struct Steps {
    rows: Mutex<u64>,
    messages: Mutex<Vec<String>>,
    finished: Mutex<bool>,
}

impl Progress for Steps {
    fn row(&self) {
        *self.rows.lock().expect("not poisoned") += 1;
    }

    fn message(&self, text: &str) {
        self.messages
            .lock()
            .expect("not poisoned")
            .push(text.to_string());
    }

    fn finish(&self) {
        *self.finished.lock().expect("not poisoned") = true;
    }
}

/// The real store with one reading bent, so an arm can force the disagreement
/// the read-back exists to catch.
struct Doctored<'a> {
    inner: &'a dyn Store,
    /// Dropped from every note read back, which is what a truncated write looks
    /// like from here.
    drop_landing: bool,
    append: Option<String>,
}

impl Store for Doctored<'_> {
    fn ready(&self) -> Result<Vec<String>, StoreError> {
        self.inner.ready()
    }

    fn open_labelled(&self, label: &str) -> Result<Vec<String>, StoreError> {
        self.inner.open_labelled(label)
    }

    fn create(&self, item: &fleet_core::store::NewItem, by: &str) -> Result<String, StoreError> {
        self.inner.create(item, by)
    }

    fn set_title(&self, item: &str, title: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_title(item, title, by)
    }

    fn show(&self, item: &str) -> Result<Item, StoreError> {
        let mut read = self.inner.show(item)?;
        if self.drop_landing {
            read.notes = read.notes.map(|notes| {
                notes
                    .lines()
                    .filter(|line| !line.starts_with(LANDING_MARKERS[0]))
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        }
        if let Some(extra) = &self.append {
            read.document.push_str(extra);
        }
        Ok(read)
    }

    fn show_text(&self, item: &str) -> Result<String, StoreError> {
        self.inner.show_text(item)
    }

    fn assigned_to(&self, seat: &str) -> Result<Vec<AssignedItem>, StoreError> {
        self.inner.assigned_to(seat)
    }

    fn assign(&self, item: &str, seat: &str, by: &str) -> Result<(), StoreError> {
        self.inner.assign(item, seat, by)
    }

    fn note(&self, item: &str, text: &str, by: &str) -> Result<(), StoreError> {
        self.inner.note(item, text, by)
    }

    fn set_orders(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_orders(item, payload, by)
    }

    fn set_metadata(&self, item: &str, payload: &str, by: &str) -> Result<(), StoreError> {
        self.inner.set_metadata(item, payload, by)
    }

    fn unset_orders(&self, item: &str, by: &str) -> Result<(), StoreError> {
        self.inner.unset_orders(item, by)
    }

    fn hold(&self, item: &str, reason: &str, by: &str) -> Result<String, StoreError> {
        self.inner.hold(item, reason, by)
    }

    fn open_holds(&self) -> Result<Vec<String>, StoreError> {
        self.inner.open_holds()
    }

    fn clear_hold(&self, hold: &str, by: &str) -> Result<(), StoreError> {
        self.inner.clear_hold(hold, by)
    }

    fn close(&self, item: &str, reason: &str, by: &str) -> Result<(), StoreError> {
        self.inner.close(item, reason, by)
    }

    fn export(&self, into: &std::path::Path) -> Result<(), StoreError> {
        self.inner.export(into)
    }
}

// ---- the rig -----------------------------------------------------------------

/// One board per arm, held in memory. The landing's export gate reads a file
/// this store rewrites, so the board is per arm and not shared: two arms
/// exporting to one path would each be reading the other's write.
fn store() -> Board {
    let board = Board::new("land");
    board.fleet_toml(POLICY);
    board
}

/// Serialises the two arms that resolve a name on the process's own `PATH` —
/// the ring, which wants `bd` there, and the arm below that takes it away.
/// `PATH` is process-wide and there is one of it.
static PATH_LOCK: Mutex<()> = Mutex::new(());

/// Runs `body` with `PATH` holding `dir` and nothing else, and puts `PATH` back
/// before returning — including when `body` panics is NOT covered: a red arm
/// here leaves the lock poisoned, which stops the ring rather than misleading
/// it.
fn with_only_on_path<T>(dir: &Path, body: impl FnOnce() -> T) -> T {
    let original = std::env::var_os("PATH");
    std::env::set_var("PATH", dir);
    let answer = body();
    match original {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }
    answer
}

/// The integration ring: the one arm of this suite that lands through `bd`.
fn ring() -> &'static Scratch {
    let scratch = shared_store("land");
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        scratch.fleet_toml(POLICY);
    });
    scratch
}

fn project(scratch: &dyn Rooted) -> Project {
    let table = fleet_core::item::table_at(&scratch.root().join("fleet.toml"));
    Project {
        root: scratch.root().to_path_buf(),
        name: "a-project".to_string(),
        guards: table.clone(),
        policy: table,
    }
}

/// The machine's seat table under the rig's own machine directory: the file a
/// landing handed the primary reads to find the tree it belongs in. One row per
/// (seat, project, worktree), written whole each time, because an arm that
/// takes a worktree away is asserting on the file and not on an edit to it.
fn seat_table(scratch: &dyn Rooted, rows: &[(&str, &str, &Path)]) {
    let machine = scratch.root().join("machine");
    std::fs::create_dir_all(&machine).expect("the machine directory is made");
    let children: Vec<serde_json::Value> = rows
        .iter()
        .map(|(seat, project, worktree)| {
            let mut worktrees = serde_json::Map::new();
            worktrees.insert(
                (*project).to_string(),
                serde_json::Value::String(worktree.display().to_string()),
            );
            serde_json::json!({ "name": seat, "worktrees": worktrees })
        })
        .collect();
    std::fs::write(
        machine.join("config.json"),
        serde_json::json!({
            "fleet_toml": scratch.root().join("fleet.toml").display().to_string(),
            "children": children,
        })
        .to_string(),
    )
    .expect("the seat table is written");
}

/// What the arm below's suite leaves behind it, in whichever tree it ran in.
const SUITE_RAN: &str = "the-suite-ran-here";

fn packs(scratch: &dyn Rooted) -> Packs {
    Packs::under(scratch.packs_dir(), scratch.defaults_dir())
        .expect("the defaults resolve under the scratch")
}

fn a_delivery(commit: &str) -> String {
    a_delivery_on(commit, WORK)
}

fn a_delivery_on(commit: &str, branch: &str) -> String {
    format!(
        "DELIVERED {commit} — {BUILDER}\n\
         commit:  {commit}\n\
         branch:  {branch}\n\
         base:    {TRUNK} at {OLD}, fetched at 2026-09-12T00:00:00Z\n\
         files:   {FILE}\n\
         checks:  AC1 green, read from the arm's own status\n\
         suite:   the workspace suite, rc 0\n\
         spec corrections: none\n\
         not proven: what this arm did not run\n\
         decisions: none\n\
         covers: R8"
    )
}

fn a_verdict(marker: &str, commit: &str, item: &str) -> String {
    format!(
        "{marker} {commit} — {REVIEWER}\n\
         item:    {item}\n\
         size: 1 file(s), +1, -0 — tests: no, executable: no\n\
         decisions: 0 accepted, 0 overruled"
    )
}

/// One item held by the reviewer, carrying a delivery and a verdict.
fn an_item(store: &dyn Store, title: &str, verdict: Option<(&str, &str)>) -> String {
    an_item_delivering(store, title, &a_delivery(SHA), verdict)
}

/// One item whose delivery note names this branch, and an accepted verdict.
fn an_item_on_branch(store: &dyn Store, title: &str, branch: &str) -> String {
    an_item_delivering(
        store,
        title,
        &a_delivery_on(SHA, branch),
        Some(("ACCEPTED", SHA)),
    )
}

/// Built through the trait and not through a binary, so one builder fills
/// either board.
fn an_item_delivering(
    store: &dyn Store,
    title: &str,
    delivery: &str,
    verdict: Option<(&str, &str)>,
) -> String {
    let item = store
        .create(
            &fleet_core::store::NewItem {
                title,
                description: "an item to land",
                item_type: "task",
                labels: &[],
            },
            BUILDER,
        )
        .expect("the item is filed");
    store
        .assign(&item, REVIEWER, REVIEWER)
        .expect("the reviewer holds it");
    store
        .note(&item, delivery, BUILDER)
        .expect("the delivery is on it");
    if let Some((marker, commit)) = verdict {
        store
            .note(&item, &a_verdict(marker, commit, &item), REVIEWER)
            .expect("the verdict is on it");
    }
    item
}

struct Ran {
    landed: Result<Landed, Stop>,
    out: String,
    err: String,
    rows: u64,
    finished: bool,
}

impl Ran {
    fn code(&self) -> Option<u8> {
        self.landed.as_ref().err().map(|stop| stop.code)
    }

    fn why(&self) -> String {
        self.landed
            .as_ref()
            .err()
            .map(|stop| stop.message.clone())
            .unwrap_or_default()
    }
}

fn run(scratch: &dyn Rooted, store: &dyn Store, git: &StubGit, item: &str, commit: &str) -> Ran {
    run_with(scratch, store, git, item, commit, &[], None)
}

#[allow(clippy::too_many_arguments)]
fn run_with(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    commit: &str,
    also: &[String],
    reason: Option<&str>,
) -> Ran {
    run_watched(
        scratch,
        store,
        git,
        item,
        commit,
        also,
        reason,
        &StubEvents::default(),
    )
}

/// The same landing with the stream in the arm's own hands.
#[allow(clippy::too_many_arguments)]
fn run_watched(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    commit: &str,
    also: &[String],
    reason: Option<&str>,
    events: &StubEvents,
) -> Ran {
    run_loaded(
        scratch,
        store,
        git,
        item,
        commit,
        also,
        reason,
        events,
        &lane::Unread,
    )
}

/// The same landing with the box's load in the arm's own hands, which is what
/// drives the rerun's wait.
#[allow(clippy::too_many_arguments)]
fn run_loaded(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    commit: &str,
    also: &[String],
    reason: Option<&str>,
    events: &StubEvents,
    load: &dyn lane::Load,
) -> Ran {
    run_against(
        scratch,
        store,
        git,
        item,
        commit,
        also,
        reason,
        &project(scratch),
        Some(TEST),
        events,
        load,
    )
}

/// The landing handed NO test command, over the rig's own project: the call
/// `fleet land` makes without `--test`.
fn run_untested(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    events: &StubEvents,
) -> Ran {
    run_against(
        scratch,
        store,
        git,
        item,
        SHA,
        &[],
        None,
        &project(scratch),
        None,
        events,
        &lane::Unread,
    )
}

/// The landing with the PROJECT and the TEST COMMAND in the arm's own hands
/// too: the rerun's arms each need a command of their own and a wait of their
/// own, and `None` is the landing handed no `--test` at all.
#[allow(clippy::too_many_arguments)]
fn run_against(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    commit: &str,
    also: &[String],
    reason: Option<&str>,
    project: &Project,
    test: Option<&str>,
    events: &StubEvents,
    load: &dyn lane::Load,
) -> Ran {
    run_against_path(
        scratch, store, git, item, commit, also, reason, project, test, events, load, "", REVIEWER,
    )
}

/// The landing with the CONSTRUCTED CHILD PATH in the arm's hands as well —
/// `""` is the caller that constructed none, which is what every arm above
/// hands in — and the CALLER, which is the reviewer everywhere but the run
/// arms, where it is a run's own record id.
#[allow(clippy::too_many_arguments)]
fn run_against_path(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    commit: &str,
    also: &[String],
    reason: Option<&str>,
    project: &Project,
    test: Option<&str>,
    events: &StubEvents,
    load: &dyn lane::Load,
    child_path: &str,
    by: &str,
) -> Ran {
    let packs = packs(scratch);
    let steps = Steps::default();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let landed = land::land(
        &mut out,
        &mut err,
        &Landing {
            item,
            commit,
            also,
            test,
            reason,
            by,
            at: AT,
            machine_dir: &scratch.root().join("machine"),
        },
        &Wiring {
            store,
            git,
            packs: &packs,
            project,
            progress: &steps,
            events,
            load,
            child_path,
        },
    );
    // Read out before the struct literal: a guard taken in a tail expression
    // outlives the local it borrows.
    let rows = *steps.rows.lock().expect("not poisoned");
    let finished = *steps.finished.lock().expect("not poisoned");
    Ran {
        landed,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
        rows,
        finished,
    }
}

// ---- the landing -------------------------------------------------------------

/// The whole act: the gates in the § 4 order, the note the template renders,
/// the close, and both read back.
///
/// THE INTEGRATION RING of this suite, and the one arm here that drives `bd`:
/// the whole landing against the store the verb actually talks to, so the
/// in-memory board the arms below run on cannot drift from it unseen.
#[test]
fn a_clean_landing_runs_the_gates_in_order_and_writes_the_note_and_closes() {
    let _path = PATH_LOCK.lock().expect("not poisoned");
    let scratch = ring();
    let bd = &Bd::at(&scratch.root);
    let item = an_item(bd, "an item to land", Some(("ACCEPTED", SHA)));
    let git = StubGit::clean();

    let events = StubEvents::default();
    let ran = run_watched(scratch, bd, &git, &item, SHA, &[], None, &events);
    let landed = ran.landed.as_ref().unwrap_or_else(|stop| {
        panic!("the landing was refused: {}\n{}", stop.message, ran.out);
    });

    // THE TWO EVENTS, in the order they happened: the reading, then the
    // landing. Both carry the push's own range line and never a rev-parse.
    assert_eq!(events.count(), 2, "one reading and one landing");
    let kinds: Vec<String> = events.all().into_iter().map(|(kind, _, _)| kind).collect();
    assert_eq!(kinds, vec![CHECK_READ.to_string(), ITEM_LANDED.to_string()]);
    let (actor, reading) = events.one(CHECK_READ);
    assert_eq!(actor, REVIEWER);
    keys_agree(CHECK_READ, &reading, &[]);
    assert_eq!(reading["item"], serde_json::json!(item));
    assert_eq!(reading["suite"], serde_json::json!("exit 0"));
    assert_eq!(reading["rc"], serde_json::json!(0));
    assert_eq!(reading["verdict"], serde_json::json!("green"));
    assert_eq!(reading["reading"], serde_json::json!(1));
    let (actor, landing) = events.one(ITEM_LANDED);
    assert_eq!(actor, REVIEWER);
    keys_agree(ITEM_LANDED, &landing, &[]);
    assert_eq!(landing["item"], serde_json::json!(item));
    assert_eq!(landing["sha"], serde_json::json!(LANDED));
    assert_eq!(landing["base"], serde_json::json!(OLD));
    assert_eq!(landing["squash_of"], serde_json::json!(SHA));
    // The stub's own HEAD answers a DIFFERENT sha, so this equality can only
    // have come from the push's own range line.
    assert_ne!(COMMITTED, LANDED);
    assert_eq!(landed.sha, LANDED, "the sha is the push's range line's");

    // THE ORDER, read off the stub's own call list. Every later gate's calls sit
    // after every earlier gate's, which is the § 4 order stated as a sequence.
    let calls = git.calls();
    let at = |needle: &str| {
        calls
            .iter()
            .position(|call| call.starts_with(needle))
            .unwrap_or_else(|| panic!("no `{needle}` among {calls:?}"))
    };
    // BOTH fetches are walked. Correction 4 rests on there being two — the one
    // the land branch is cut at and the one the push counts against — and a
    // walk that skipped them could not tell one fetch from two. The comparison
    // is STRICT, so two steps landing on one call is a failure and not a pass.
    let order = [
        "is_linked_worktree".to_string(),
        "status".to_string(),
        "fetch origin".to_string(),
        format!("branch_at land/{item}"),
        format!("squash_merge {SHA}"),
        format!("add {}", fleet_core::store::EXPORT),
        "staged".to_string(),
        "commit_message_file".to_string(),
        "fetch origin".to_string(),
        "behind HEAD".to_string(),
        "push_head".to_string(),
        "detach".to_string(),
    ];
    let mut walked = 0;
    for step in &order {
        let here = calls
            .iter()
            .enumerate()
            .skip(walked)
            .find(|(_, call)| call.starts_with(step.as_str()))
            .map(|(at, _)| at)
            .unwrap_or_else(|| panic!("no `{step}` at or after {walked} among {calls:?}"));
        assert!(
            here >= walked,
            "`{step}` ran at {here}, before the step at {walked}: {calls:?}"
        );
        walked = here + 1;
    }
    assert_eq!(
        calls
            .iter()
            .filter(|c| c.as_str() == "fetch origin")
            .count(),
        2,
        "the trunk is fetched twice — once for the land branch, once inside the push's own act: \
         {calls:?}"
    );
    let _ = at("is_linked_worktree");

    // THE WHOLE NOTE, rendered here from the shipped template and compared to
    // what the store holds — not the first line, and not by contains.
    let template = std::fs::read_to_string(
        packs(scratch)
            .slot(LANDING_NOTE)
            .expect("core carries the landing note"),
    )
    .expect("the template is readable");
    let block =
        marker_block(&template, LANDING_MARKERS[0]).expect("the template carries a LANDED block");
    let wanted = render(
        &block,
        &[
            ("sha", LANDED),
            ("rebased", ""),
            ("trunk", TRUNK_BRANCH),
            ("actor", REVIEWER),
            ("old", OLD),
            ("new", LANDED),
            ("commit", SHA),
            ("builder", BUILDER),
            ("tested", "suite: exit 0, rc 0"),
            ("checks", &checks_of(&landed.note)),
        ],
    )
    .expect("every placeholder is one this arm offers");
    assert_eq!(
        landed.note, wanted,
        "the note is the template rendered whole — its first line and its gate and nothing the \
         template does not carry"
    );
    // What the check block itself is, asserted where the arm can predict it: the
    // seven rows by number, criterion and verdict, and the commands block
    // verbatim. Only the evidence is left to the rows' own checks, because it
    // names a temporary path and a duration no arm can know in advance.
    let checks = checks_of(&landed.note);
    let verdicts = ["PASS", "PASS", "PASS", "PASS", "PASS", "SAFE", "PASS"];
    for (n, (criterion, verdict)) in CRITERIA.iter().zip(verdicts).enumerate() {
        let head = format!("{}. {criterion:<16} {verdict:<10} ", n + 1);
        assert!(
            checks.lines().any(|line| line.starts_with(&head)),
            "no row opening `{head}` in:\n{checks}"
        );
    }
    let commands: Vec<&str> = checks
        .lines()
        .skip_while(|line| !line.starts_with("## Commands"))
        .collect();
    assert_eq!(
        commands,
        vec![
            "## Commands — every verdict above, re-runnable",
            &format!("git merge-base {TRUNK} {SHA}"),
            &format!("git diff --name-only $(git merge-base {TRUNK} {SHA}) {SHA}"),
            &format!("git show --stat {LANDED}"),
            &format!("git rev-list --count {LANDED}..{TRUNK}"),
            &format!("git rev-parse {WORK}"),
            &format!("git diff {SHA} {LANDED} --"),
            "git status --porcelain",
        ],
        "the commands block is rendered whole"
    );

    let record = scratch.json(&item);
    assert!(
        record.contains(&format!("LANDED {LANDED} on {TRUNK_BRANCH}")),
        "the note is on the item: {record}"
    );
    let read = bd.show(&item).expect("the item reads back");
    assert_eq!(
        last_landing(read.notes.as_deref().unwrap_or_default()).as_deref(),
        Some(landed.note.as_str()),
        "the landing region off the item is the note the verb rendered"
    );
    assert_eq!(read.status, "closed", "the item is closed");
    assert!(
        record.contains(&format!("landed {LANDED}")),
        "the close reason names the landed sha: {record}"
    );

    // Every row the note carries reached stdout as it was read, in the same
    // order, and the bar took one step per row.
    for (n, criterion) in CRITERIA.iter().enumerate() {
        let row = format!("{}. {criterion}", n + 1);
        assert!(ran.out.contains(&row), "no `{row}` on stdout:\n{}", ran.out);
        assert!(
            landed.note.contains(&row),
            "no `{row}` in the note:\n{}",
            landed.note
        );
    }
    assert_eq!(
        ran.rows,
        CRITERIA.len() as u64,
        "the bar took one step per check row"
    );
    assert!(ran.finished, "the bar was taken back off the terminal");
    assert!(
        ran.out.trim_end().ends_with(&format!("LANDED {LANDED}")),
        "the last line is the one a caller greps for:\n{}",
        ran.out
    );
    // The gate table is stdout's alone: a green landing writes nothing to the
    // stream a script does not parse.
    assert_eq!(ran.err, "", "a green landing says nothing on stderr");
}

/// The marker the project's `[landing] ci_marker` printed rides the commit
/// subject, and the trailers name both seats.
#[test]
fn the_marker_and_the_two_trailers_ride_the_commit_subject() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose subject carries a marker",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());

    let message = git
        .calls()
        .into_iter()
        .find(|call| call.starts_with("commit_message_file"))
        .expect("a commit was made from a message file");
    assert!(
        message.contains(&format!(
            "{item}: an item whose subject carries a marker [skip ci]"
        )),
        "the subject is the item, its title and the marker: {message}"
    );
    assert!(
        message.contains(&format!("Seat: {REVIEWER}")),
        "the Seat trailer names the reviewer: {message}"
    );
    assert!(
        message.contains(&format!("Implemented-by: {BUILDER}")),
        "the Implemented-by trailer names the builder: {message}"
    );
}

/// A landing handed no test command LANDS, on the review alone, and says NOT
/// TESTED where nobody can miss it: the note's first line, its suite row, the
/// row on stdout, and `item.landed`'s own `test` — never a zero borrowed from a
/// run that did not happen.
#[test]
fn a_landing_handed_no_test_lands_and_says_not_tested() {
    let scratch = Board::new("land-no-suite");
    scratch.fleet_toml("[landing]\n");
    let item = an_item(
        &scratch.store,
        "an item landed with no test command",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let events = StubEvents::default();
    let ran = run_untested(&scratch, bd, &git, &item, &events);
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("{}\n{}", stop.message, ran.out));
    assert_eq!(landed.sha, LANDED, "the landing ran through to the push");
    // The reading is a measured absence on the stream too, in the same three
    // words the note's own row prints.
    let (_, reading) = events.one(CHECK_READ);
    assert_eq!(reading["suite"], serde_json::Value::Null);
    assert_eq!(reading["rc"], serde_json::Value::Null);
    assert_eq!(reading["verdict"], serde_json::json!("none"));
    let (_, landing) = events.one(ITEM_LANDED);
    keys_agree(ITEM_LANDED, &landing, &[]);
    assert_eq!(
        landing["test"],
        serde_json::Value::Null,
        "item.landed records that no test ran"
    );
    let first = landed.note.lines().next().unwrap_or_default();
    assert!(
        first.starts_with(&format!("LANDED {LANDED}"))
            && first.contains(&format!("— {}: ", land::NOT_TESTED))
            && first.contains("no test command was handed to this landing"),
        "the first line says NOT TESTED:\n{}",
        landed.note
    );
    assert!(
        !first.contains("rc "),
        "and borrows no rc from a run that did not happen: {first}"
    );
    assert!(
        landed.note.contains("4. suite            NOT TESTED"),
        "and so does the row:\n{}",
        landed.note
    );
    assert!(
        ran.out.contains("4. suite            NOT TESTED"),
        "and the row the person watching reads:\n{}",
        ran.out
    );
}

/// Another writer's bare `orders` and the bare `run` label are not fleet's: the
/// item lands as a seat's landing — the reviewer closing its own — and both
/// ride through the landing and the close byte-identical (fleet-4j6 AC1, the
/// landing).
#[test]
fn another_writers_orders_key_and_run_label_ride_through_a_landing() {
    let scratch = Board::new("land-foreign");
    scratch.fleet_toml("[landing]\n");
    let item = an_item(
        &scratch.store,
        "an item another tool indexes too",
        Some(("ACCEPTED", SHA)),
    );
    scratch
        .store
        .set_metadata(&item, common::FOREIGN_ORDERS, "another-tool")
        .expect("the other writer's key lands");
    scratch.label(&item, common::FOREIGN_LABEL);
    let before = common::foreign_of(&scratch.store, &item);

    let ran = run_untested(
        &scratch,
        &scratch.store,
        &StubGit::clean(),
        &item,
        &StubEvents::default(),
    );
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("{}\n{}", stop.message, ran.out));
    assert_eq!(landed.sha, LANDED, "the landing ran through to the push");
    assert_eq!(
        scratch.store.show(&item).expect("the item reads").status,
        "closed",
        "and closed the item"
    );
    assert_eq!(
        common::foreign_of(&scratch.store, &item),
        before,
        "the other writer's key and label are byte-identical"
    );
}

/// `fleet land --test <command>` runs the command it is handed ON THE LAND
/// BRANCH — after the squash and the commit, before the push — and records the
/// command and its rc 0 on the note's first line, the gate reading and
/// `item.landed`.
#[test]
fn a_landing_handed_a_green_test_lands_and_records_the_command_and_rc_0() {
    let scratch = Board::new("land-test-green");
    scratch.fleet_toml(POLICY);
    let item = an_item(
        &scratch.store,
        "an item landed under a green test",
        Some(("ACCEPTED", SHA)),
    );
    let git = StubGit::clean();
    let events = StubEvents::default();
    let ran = run_against(
        &scratch,
        &scratch.store,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project(&scratch),
        Some("exit 0"),
        &events,
        &lane::Unread,
    );
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("{}\n{}", stop.message, ran.out));
    assert!(
        landed
            .note
            .lines()
            .next()
            .unwrap_or_default()
            .ends_with("— suite: exit 0, rc 0"),
        "the first line records the command and its rc:\n{}",
        landed.note
    );
    assert!(
        !landed.note.contains(land::NOT_TESTED),
        "and a tested landing never reads like an untested one:\n{}",
        landed.note
    );
    let (_, reading) = events.one(CHECK_READ);
    assert_eq!(reading["suite"], serde_json::json!("exit 0"));
    assert_eq!(reading["rc"], serde_json::json!(0));
    let (_, landing) = events.one(ITEM_LANDED);
    assert_eq!(landing["test"], serde_json::json!("exit 0"));

    // The test ran BETWEEN the commit on the land branch and the push: what it
    // read is the tree that landed.
    let calls = git.calls();
    let at = |needle: &str| {
        calls
            .iter()
            .position(|call| call.starts_with(needle))
            .unwrap_or_else(|| panic!("no `{needle}` among {calls:?}"))
    };
    assert!(at("commit_message_file") < at("push_head"), "{calls:?}");
    let row = ran
        .out
        .lines()
        .position(|line| line.starts_with("4. suite"))
        .expect("the suite row is printed");
    let behind = ran
        .out
        .lines()
        .position(|line| line.starts_with("5. base current"))
        .expect("the trunk row is printed");
    assert!(row < behind, "the test is read before the push's own gate");
}

/// `fleet land --test <command>` whose command exits non-zero — twice, the
/// first read and its one rerun — REFUSES, and nothing moved: no push, no
/// landing note, no close, no `item.landed`, and the land branch put back.
#[test]
fn a_landing_handed_a_red_test_refuses_with_nothing_moved() {
    let scratch = Board::new("land-test-red");
    scratch.fleet_toml(POLICY);
    let item = an_item(
        &scratch.store,
        "an item whose test is red",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let git = StubGit::clean();
    let events = StubEvents::default();
    let ran = run_against(
        &scratch,
        &scratch.store,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project(&scratch),
        Some("exit 1"),
        &events,
        &lane::Unread,
    );
    assert_eq!(ran.code(), Some(1), "refused on the record: {}", ran.why());
    assert!(
        ran.why().contains("the suite `exit 1` exited 1"),
        "the refusal names the command and its rc: {}",
        ran.why()
    );
    let calls = git.calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("push_head")),
        "nothing was pushed: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with(&format!("delete_branch land/{item}"))),
        "the land branch is put back: {calls:?}"
    );
    assert_eq!(
        scratch.json(&item),
        before,
        "the item is byte-identical: no note, no close"
    );
    assert!(
        events
            .all()
            .iter()
            .all(|(kind, _, _)| kind.as_str() != ITEM_LANDED),
        "and no landing reached the stream"
    );
}

/// A policy file that still sets `[gates] suite` is REFUSED before anything is
/// read, naming the pack setting that replaces it — never read as absent and
/// landed NOT TESTED while the person who wrote it believes it ran.
#[test]
fn a_policy_file_setting_gates_suite_is_refused_naming_the_pack_setting() {
    let scratch = Board::new("land-moved-suite");
    scratch.fleet_toml(
        "[gates]\nsuite = \"exit 0\"\nci_marker = \"printf '[skip ci]'\"\n\n\
         [core]\nreviewer = \"a-reviewer\"\n",
    );
    let item = an_item(
        &scratch.store,
        "an item in a fleet whose policy still sets a suite",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let git = StubGit::clean();
    let events = StubEvents::default();
    let ran = run_untested(&scratch, &scratch.store, &git, &item, &events);
    assert_eq!(ran.code(), Some(1), "refused on the record: {}", ran.why());
    assert!(
        ran.why().contains("[gates] suite")
            && ran.why().contains("`takeoff.test` under [packs.tiny]")
            && ran.why().contains("fleet land --test <command>"),
        "the line names the key and where it is set instead: {}",
        ran.why()
    );
    assert!(
        git.calls().is_empty(),
        "no git was asked anything: {:?}",
        git.calls()
    );
    assert_eq!(scratch.json(&item), before, "the item is byte-identical");
    assert_eq!(events.count(), 0, "and nothing reached the stream");
}

/// A policy file that still carries a `[gates]` table is REFUSED by the
/// table's name before anything is read, naming where each of its keys is set
/// now — a marker left under `[gates]` is one no landing reads, and the commit
/// it meant to mark would go out unmarked with nobody told.
#[test]
fn a_policy_file_carrying_a_gates_table_is_refused_naming_the_new_homes() {
    let scratch = Board::new("land-moved-gates");
    scratch.fleet_toml(
        "[gates]\nci_marker = \"printf '[skip ci]'\"\n\n[core]\nreviewer = \"a-reviewer\"\n",
    );
    let item = an_item(
        &scratch.store,
        "an item in a fleet whose policy still carries [gates]",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let git = StubGit::clean();
    let events = StubEvents::default();
    let ran = run_untested(&scratch, &scratch.store, &git, &item, &events);
    assert_eq!(ran.code(), Some(1), "refused on the record: {}", ran.why());
    assert!(
        ran.why().contains("[gates] is not a policy table")
            && ran.why().contains("`ci_marker` under [landing]")
            && ran.why().contains("`tool_commands` under [permissions]")
            && ran.why().contains("under [guards.targets]"),
        "the line names the table and every new home: {}",
        ran.why()
    );
    assert!(
        git.calls().is_empty(),
        "no git was asked anything: {:?}",
        git.calls()
    );
    assert_eq!(scratch.json(&item), before, "the item is byte-identical");
    assert_eq!(events.count(), 0, "and nothing reached the stream");
}

/// `--reason` rides the close beside the landed sha, and `--also` widens the
/// set the staged-set gate expects.
#[test]
fn also_widens_the_delivered_set_and_reason_rides_the_close() {
    let scratch = Board::new("land-also");
    scratch.fleet_toml(POLICY);
    let item = an_item(
        &scratch.store,
        "an item landed with a path of the reviewer's own",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.staged = vec![
        ".beads/issues.jsonl".to_string(),
        FILE.to_string(),
        "the-log.md".to_string(),
    ];
    std::fs::write(scratch.root.join("the-log.md"), "the reviewer's own line\n")
        .expect("the admitted path is written");

    let ran = run_with(
        &scratch,
        bd,
        &git,
        &item,
        SHA,
        &["the-log.md".to_string()],
        Some("the gate is the owner's"),
    );
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("{}\n{}", stop.message, ran.out));
    assert!(
        landed.note.contains("plus --also the-log.md"),
        "the staged-set row names what was admitted:\n{}",
        landed.note
    );
    assert!(
        scratch
            .json(&item)
            .contains(&format!("landed {LANDED} — the gate is the owner's")),
        "the close reason carries both halves"
    );
}

/// A project whose store git IGNORES stages nothing of it and lands anyway.
///
/// `bd init` writes that ignore itself, so this is the standalone shape and not
/// an edge: measured on a scratch repository, `git add .beads
/// ':(exclude).beads/hooks'` there exits 128 with `pathspec '.beads' did not
/// match any files`. The verb reads the porcelain to tell the two projects
/// apart rather than staging blind.
#[test]
fn a_project_whose_store_git_ignores_stages_none_of_it() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item in a project whose store git ignores",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    // Nothing under the store's directory appears in the porcelain, ever.
    *git.statuses.lock().expect("not poisoned") = [Vec::new(), Vec::new()].into();
    git.staged = vec![FILE.to_string()];

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    let calls = git.calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("add .beads")),
        "no store path was staged: {calls:?}"
    );
    // The control for the arm above: the clean rig, whose porcelain DOES name
    // the export, stages it — so what was measured is the porcelain and not a
    // verb that never stages the store.
    let versioned = StubGit::clean();
    let other = an_item(
        &scratch.store,
        "an item in a project that versions its store",
        Some(("ACCEPTED", SHA)),
    );
    let ran = run(scratch, bd, &versioned, &other, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    assert!(
        versioned
            .calls()
            .iter()
            .any(|call| call.starts_with("add .beads")),
        "the versioned store IS staged: {:?}",
        versioned.calls()
    );
}

/// A board dirty at the start lands, and a refusal does not wedge the re-run.
///
/// (b) refuses on what the working tree carries and (e) regenerates the export
/// in that same tree, so a gate that judged the store's directory would refuse
/// every second attempt at the item it had just refused.
#[test]
fn a_dirty_board_does_not_wedge_the_gate_or_the_re_run() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item landed over a board that was already dirty",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();
    // The export is already modified when the landing starts, which is what a
    // re-run after any refusal at (e) or later looks like.
    *git.statuses.lock().expect("not poisoned") = [
        vec![" M .beads/issues.jsonl".to_string()],
        vec![" M .beads/issues.jsonl".to_string()],
    ]
    .into();

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);

    // The control: the same line under any OTHER path still refuses, so what
    // was exempted is the store's directory and not the gate.
    let other = an_item(
        &scratch.store,
        "an item landed over a tree with a loose file",
        Some(("ACCEPTED", SHA)),
    );
    let git = StubGit::clean();
    *git.statuses.lock().expect("not poisoned") = [vec![" M a/loose.rs".to_string()]].into();
    let ran = run(scratch, bd, &git, &other, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains("a/loose.rs"), "{}", ran.why());
}

/// The tree gate exempts the STORE'S EXPORT and nothing else beneath it, and
/// the two directions are one arm because they are one boundary.
///
/// TOO WIDE is the bug: a refusal from (e) on hard-resets the tree, so every
/// store path this gate waves past is a store path the reset discards without
/// a word — a tracked `.beads/hooks/*` edit included. TOO NARROW loses the
/// re-run: the export is rewritten by (e) in this same root, so a gate that
/// refused on it would refuse every re-run after its own first refusal.
#[test]
fn the_tree_gate_exempts_the_export_alone_and_refuses_the_rest_of_the_store() {
    let scratch = &store();
    let bd = &scratch.store;

    // TOO WIDE, refused: a tracked file under the store that is not the export.
    let hook = scratch.root().join(".beads/hooks/x");
    std::fs::create_dir_all(hook.parent().expect("it has a parent"))
        .expect("the hooks directory is made");
    std::fs::write(&hook, "a seat's own uncommitted edit\n").expect("the hook is written");
    let item = an_item(
        bd,
        "an item landed over a dirty tracked hook",
        Some(("ACCEPTED", SHA)),
    );
    let git = StubGit::clean();
    *git.statuses.lock().expect("not poisoned") = [vec![" M .beads/hooks/x".to_string()]].into();

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}\n{}", ran.why(), ran.out);
    assert!(
        ran.why().contains(".beads/hooks/x"),
        "the refusal names the path it refused on: {}",
        ran.why()
    );
    // AND IT SURVIVES. The reset is the only call that discards it, and a gate
    // that refuses at (b) reaches no git write at all — so the bytes on disk
    // and the absent calls are the same answer read two ways.
    let calls = git.calls();
    assert!(
        !calls.iter().any(|call| call.starts_with("reset_hard")
            || call.starts_with("branch_at")
            || call.starts_with("squash_merge")),
        "the refusal came before any git write: {calls:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&hook).expect("the hook is still there"),
        "a seat's own uncommitted edit\n",
        "the seat's edit survives the refusal"
    );

    // TOO NARROW, admitted: the export in both spellings the porcelain uses —
    // by name where the store is tracked, and as the whole directory where it
    // is untracked but unignored.
    for spelling in [" M .beads/issues.jsonl", "?? .beads/"] {
        let item = an_item(
            bd,
            &format!("an item landed over `{spelling}`"),
            Some(("ACCEPTED", SHA)),
        );
        let git = StubGit::clean();
        *git.statuses.lock().expect("not poisoned") = [
            vec![spelling.to_string()],
            vec![" M .beads/issues.jsonl".to_string()],
        ]
        .into();
        let ran = run(scratch, bd, &git, &item, SHA);
        assert!(
            ran.landed.is_ok(),
            "`{spelling}` is the verb's own bookkeeping: {}\n{}",
            ran.why(),
            ran.out
        );
    }
}

/// A path git printed QUOTED reads back to the name a reviewer types, octal and
/// all: under the default `core.quotePath` a non-ASCII byte reaches the
/// porcelain only as `\303\251`, so a decoder that undoes the backslash form
/// alone hands back a name no `--also` value can ever equal.
#[test]
fn a_quoted_octal_path_decodes_to_the_name_also_admits() {
    let scratch = &store();
    let bd = &scratch.store;
    let quoted = " M \"a/caf\\303\\251.rs\"";
    let plain = "a/café.rs";

    // Refused, and the refusal names the DECODED path: a reviewer reads this
    // line and types what it says into `--also`.
    let item = an_item(bd, "an item over a quoted path", Some(("ACCEPTED", SHA)));
    let git = StubGit::clean();
    *git.statuses.lock().expect("not poisoned") = [vec![quoted.to_string()]].into();
    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.why().contains(plain),
        "the refusal names `{plain}` and not the escape: {}",
        ran.why()
    );

    // And admitted by exactly that value, which is the half the decoding is for.
    let item = an_item(
        bd,
        "an item admitting a quoted path",
        Some(("ACCEPTED", SHA)),
    );
    std::fs::create_dir_all(scratch.root().join("a")).expect("the directory is made");
    std::fs::write(scratch.root().join(plain), "the reviewer's own line\n")
        .expect("the admitted path is written");
    let mut git = StubGit::clean();
    git.staged = vec![
        ".beads/issues.jsonl".to_string(),
        FILE.to_string(),
        plain.to_string(),
    ];
    *git.statuses.lock().expect("not poisoned") = [
        vec![quoted.to_string()],
        vec![" M .beads/issues.jsonl".to_string()],
    ]
    .into();
    let ran = run_with(scratch, bd, &git, &item, SHA, &[plain.to_string()], None);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
}

/// The push's output is kept beside the suite's log and BOTH refusals below it
/// point a reader at that file — so a write that failed is said, not implied.
/// A refusal naming a file that is not there sends a reader looking for a run
/// that never happened.
#[test]
fn a_push_output_that_could_not_be_kept_says_so_rather_than_naming_the_file() {
    let scratch = &store();
    let bd = &scratch.store;
    let item = an_item(
        bd,
        "an item whose push output could not be kept",
        Some(("ACCEPTED", SHA)),
    );
    // A DIRECTORY WHERE THE FILE GOES: the write fails for a reason the verb
    // did not cause and cannot fix, which is the state the message is for.
    let blocked = scratch
        .root()
        .join("machine")
        .join("land")
        .join(&item)
        .join("push.out");
    std::fs::create_dir_all(&blocked).expect("the blocking directory is made");
    let mut git = StubGit::clean();
    git.push = Pushed {
        output: "remote: a pre-receive hook refused it\n".to_string(),
        code: Some(1),
    };

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}\n{}", ran.why(), ran.out);
    assert!(
        ran.why().contains("could not be kept"),
        "the refusal says the write failed: {}",
        ran.why()
    );
    assert!(
        ran.out.contains("pre-receive hook refused it"),
        "and the output is on the page instead:\n{}",
        ran.out
    );

    // The control: the same refusal with the write working names the file, so
    // what changed above is the WRITE and not the sentence.
    let item = an_item(bd, "an item whose push was kept", Some(("ACCEPTED", SHA)));
    let mut git = StubGit::clean();
    git.push = Pushed {
        output: "remote: a pre-receive hook refused it\n".to_string(),
        code: Some(1),
    };
    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.why().contains("its output is at") && ran.why().contains("push.out"),
        "a write that worked names the file: {}",
        ran.why()
    );
}

/// A refusal after the squash puts the tree back, and a detach alone does not:
/// by the time a squash exists HEAD already names the trunk, so the reset is
/// the call that undoes anything.
#[test]
fn a_refusal_after_the_squash_resets_before_it_detaches() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item refused with a squash already staged",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.staged = vec![
        ".beads/issues.jsonl".to_string(),
        FILE.to_string(),
        "a/leftover.rs".to_string(),
    ];

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    let calls = git.calls();
    let at = |needle: &str| calls.iter().position(|c| c.starts_with(needle));
    let reset = at(&format!("reset_hard {TRUNK}")).unwrap_or_else(|| {
        panic!("the squash was never put back: {calls:?}");
    });
    let detach = at(&format!("detach {TRUNK}")).expect("and the checkout detached");
    assert!(
        reset < detach,
        "the reset comes first — a detach onto a trunk HEAD already names undoes nothing: \
         {calls:?}"
    );
}

/// An --also path never makes the classification read CARRIES.
///
/// It is absent from the reviewed commit by construction and present in what
/// landed, so a classification restricted to the delivery's set PLUS the --also
/// set reads every landing as carrying work and deletes no branch ever.
#[test]
fn an_also_path_does_not_make_every_landing_carry() {
    let scratch = Board::new("land-also-classify");
    scratch.fleet_toml(POLICY);
    let item = an_item(
        &scratch.store,
        "an item landed with a path of the reviewer's own",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.staged = vec![
        ".beads/issues.jsonl".to_string(),
        FILE.to_string(),
        "the-log.md".to_string(),
    ];
    // The diff between the reviewed commit and what landed is asked about the
    // DELIVERY's paths; the stub answers empty for those, and would answer the
    // --also path if it were ever asked about it.
    std::fs::write(scratch.root.join("the-log.md"), "the reviewer's own line\n")
        .expect("the admitted path is written");

    let ran = run_with(
        &scratch,
        bd,
        &git,
        &item,
        SHA,
        &["the-log.md".to_string()],
        None,
    );
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    assert!(
        ran.out.contains("6. work branch      SAFE"),
        "an admitted path is not unlanded work:\n{}",
        ran.out
    );
    let asked = git
        .calls()
        .into_iter()
        .find(|call| call.starts_with("diff_paths"))
        .expect("the diff was taken");
    assert!(
        !asked.contains("the-log.md"),
        "the classification asks about the delivery's own paths alone: {asked}"
    );
    assert!(
        git.calls()
            .contains(&format!("delete_remote_branch origin {WORK}")),
        "and the branch is deleted: {:?}",
        git.calls()
    );
}

/// A detach that will not run is a warning, not an exit: the work is on the
/// trunk and the item is closed, and a landing reported as could-not-tell is
/// one the next reader tries to make again.
#[test]
fn a_detach_that_fails_after_the_landing_still_exits_zero() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose checkout would not detach",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.detach_fails = true;

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    assert!(
        ran.out.trim_end().ends_with(&format!("LANDED {LANDED}")),
        "the line a caller greps for is still printed:\n{}",
        ran.out
    );
    assert!(
        ran.err.contains("the landing STANDS"),
        "and the failure is said where a script does not parse it: {}",
        ran.err
    );
    assert_eq!(
        bd.show(&item).expect("it reads back").status,
        "closed",
        "the item is closed either way"
    );
}

/// An untracked store stages its export and NOT the directory it sits in: a
/// directory handed to git add there takes the database with it.
#[test]
fn an_untracked_store_stages_its_export_and_not_the_database() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item in a project whose store is untracked",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();
    // An untracked store answers with the DIRECTORY, which is the shape that
    // would have swept the database in.
    *git.statuses.lock().expect("not poisoned") =
        [Vec::new(), vec!["?? .beads/".to_string()]].into();

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    let adds: Vec<String> = git
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("add "))
        .collect();
    assert!(
        adds.contains(&format!("add {}", fleet_core::store::EXPORT)),
        "the export is staged by name: {adds:?}"
    );
    assert!(
        !adds.iter().any(|add| add == "add .beads"
            || add.starts_with("add .beads ")
            || add.contains("exclude")),
        "and the directory never is: {adds:?}"
    );
}

/// A LANDING HANDED THE PRIMARY runs in the `[core] reviewer` seat's own
/// worktree, and the tree it was handed is left alone.
///
/// This is the workflow's shape and the whole defect behind it: every verb a
/// workflow runs is pointed at the REGISTERED project's root, which on a box
/// whose fleet is registered against the trunk checkout is the primary — so
/// each landing a flight reached refused on (b) with nothing read. The
/// resolution is what this arm drives: the seat table names the reviewer's
/// worktree, the whole act runs there, and the git the verb was handed is
/// asked one question and then handed over.
#[test]
fn a_landing_handed_the_primary_runs_in_the_reviewers_own_worktree() {
    assert!(
        POLICY.contains(REVIEWER),
        "this rig's policy names the reviewer its seat table carries a row for"
    );
    let scratch = &store();
    let bd = &scratch.store;
    let item = an_item(
        bd,
        "an item landed from a workflow",
        Some(("ACCEPTED", SHA)),
    );

    // The tree the table names, and the suite's own witness of where it ran:
    // `touch` leaves its mark in the child's working directory, so the file
    // says which of the two trees the gate was read in — a `$PWD` compared as
    // text would answer about a path's spelling and not about a directory.
    let worktree = scratch.root().join("the-reviewers-worktree");
    std::fs::create_dir_all(&worktree).expect("the reviewer's worktree is made");
    seat_table(scratch, &[(REVIEWER, "a-project", &worktree)]);
    let policy = scratch.root().join("a-suite-that-says-where.toml");
    std::fs::write(
        &policy,
        format!(
            "[landing]\nci_marker = \"printf '[skip ci]'\"\n\n[core]\nreviewer = \"{REVIEWER}\"\n"
        ),
    )
    .expect("the policy is written");
    let says_where = format!("touch {SUITE_RAN}");
    let table = fleet_core::item::table_at(&policy);
    let project = Project {
        root: scratch.root().to_path_buf(),
        name: "a-project".to_string(),
        guards: table.clone(),
        policy: table,
    };

    let mut git = StubGit::clean();
    git.linked = false;
    let events = StubEvents::default();
    let ran = run_against(
        scratch,
        bd,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project,
        Some(&says_where),
        &events,
        &lane::Unread,
    );
    let landed = ran.landed.as_ref().unwrap_or_else(|stop| {
        panic!("the landing was refused: {}\n{}", stop.message, ran.out);
    });
    assert_eq!(landed.sha, LANDED, "the landing ran through to the push");

    // THE TREE IT WAS HANDED WAS ASKED ONE QUESTION AND THEN HANDED OVER.
    // Every call after the `at` is the driven tree's, so this pair is the whole
    // of what ran against the primary.
    let calls = git.calls();
    assert_eq!(
        calls.first().map(String::as_str),
        Some("is_linked_worktree"),
        "{calls:?}"
    );
    assert_eq!(
        calls.get(1),
        Some(&format!("at {}", worktree.display())),
        "the second act is the handover to the reviewer's worktree: {calls:?}"
    );

    // THE SUITE RAN IN THE REVIEWER'S WORKTREE, and the primary has no mark of
    // it. The pair is the arm: a gate read in the wrong tree still exits 0.
    assert!(
        worktree.join(SUITE_RAN).is_file(),
        "the suite ran in the reviewer's worktree: {:?}",
        std::fs::read_dir(&worktree).map(|d| d.count())
    );
    assert!(
        !scratch.root().join(SUITE_RAN).exists(),
        "and left nothing in the primary"
    );

    // AND SO DID THE EXPORT, which is the file the landing stages: a board
    // regenerated in the primary would be staged from a tree that holds a
    // commit behind.
    assert!(
        worktree.join(fleet_core::store::EXPORT).is_file(),
        "the export was written into the tree the landing committed from"
    );
    assert!(
        !scratch.root().join(fleet_core::store::EXPORT).exists(),
        "and the primary's board was not touched"
    );

    // THE REVIEWER WITH NO WORKTREE FOR THIS PROJECT refuses, naming the seat
    // and the table. The resolution never falls back to the tree it was handed,
    // because that tree is the one it exists to keep a landing out of.
    let orphan = an_item(
        bd,
        "an item whose reviewer holds no worktree here",
        Some(("ACCEPTED", SHA)),
    );
    seat_table(scratch, &[(REVIEWER, "some-other-project", &worktree)]);
    let mut git = StubGit::clean();
    git.linked = false;
    let ran = run_against(
        scratch,
        bd,
        &git,
        &orphan,
        SHA,
        &[],
        None,
        &project,
        Some(&says_where),
        &StubEvents::default(),
        &lane::Unread,
    );
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains(REVIEWER), "{}", ran.why());
    assert!(ran.why().contains("config.json"), "{}", ran.why());
    assert!(
        git.calls().iter().all(|call| call == "is_linked_worktree"),
        "nothing was driven anywhere: {:?}",
        git.calls()
    );
}

// ---- the refusals ------------------------------------------------------------

/// Every refusal that can be reached before the push, each with its own exit
/// and each leaving the item byte-identical.
#[test]
fn every_refusal_before_the_push_leaves_the_item_untouched() {
    let scratch = &store();
    let bd = &scratch.store;

    // A branch name where a commit is meant, refused by SHAPE before anything
    // is read (rule 1 of the pack's rules).
    let item = an_item(
        &scratch.store,
        "an item landed by branch name",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &item, WORK);
    assert_eq!(ran.code(), Some(2), "{}", ran.why());
    assert!(ran.why().contains(WORK), "{}", ran.why());
    assert!(
        git.calls().is_empty(),
        "nothing was read before the shape was judged: {:?}",
        git.calls()
    );
    assert_eq!(scratch.json(&item), before);

    // The primary checkout, reached now only through a SEAT TABLE that names
    // one: a landing handed the primary resolves the reviewer's own worktree
    // and refuses here on what that resolution answered, so the tree the table
    // names is the tree this gate judges.
    seat_table(
        scratch,
        &[(
            REVIEWER,
            "a-project",
            &scratch.root().join("also-a-primary"),
        )],
    );
    let mut git = StubGit::clean();
    git.linked = false;
    git.driven_linked = false;
    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains("primary"), "{}", ran.why());
    assert_eq!(scratch.json(&item), before);

    // A working-tree change nothing admitted, seen by the FIRST status read.
    let git = StubGit::clean();
    *git.statuses.lock().expect("not poisoned") = [vec!["?? forgotten.txt".to_string()]].into();
    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains("forgotten.txt"), "{}", ran.why());
    assert_eq!(scratch.json(&item), before);

    // An item held by somebody else.
    let other = an_item(
        &scratch.store,
        "an item held by another seat",
        Some(("ACCEPTED", SHA)),
    );
    scratch.assign(&other, BUILDER);
    let before_other = scratch.json(&other);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &other, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains(BUILDER), "{}", ran.why());
    assert_eq!(scratch.json(&other), before_other);

    // No verdict at all.
    let bare = an_item(&scratch.store, "an item nobody reviewed", None);
    let before_bare = scratch.json(&bare);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &bare, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains("no verdict"), "{}", ran.why());
    assert_eq!(scratch.json(&bare), before_bare);

    // A return as the last verdict.
    let returned = an_item(
        &scratch.store,
        "an item whose last verdict sent it back",
        Some(("RETURNED WITH FINDINGS", SHA)),
    );
    let before_returned = scratch.json(&returned);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &returned, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains("went back"), "{}", ran.why());
    assert_eq!(scratch.json(&returned), before_returned);

    // An accept naming a different commit.
    let elsewhere = an_item(
        &scratch.store,
        "an item accepted at another commit",
        Some(("ACCEPTED", OTHER)),
    );
    let before_elsewhere = scratch.json(&elsewhere);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &elsewhere, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.why().contains(OTHER) && ran.why().contains(SHA),
        "{}",
        ran.why()
    );
    assert_eq!(scratch.json(&elsewhere), before_elsewhere);

    // A closed item.
    scratch
        .store
        .close(&elsewhere, "closed by hand", REVIEWER)
        .expect("it closes");
    let before_closed = scratch.json(&elsewhere);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &elsewhere, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(ran.why().contains("closed"), "{}", ran.why());
    assert_eq!(scratch.json(&elsewhere), before_closed);
}

/// A branch name where a commit is meant: exit 2, and nothing was written at
/// all — no git call was made and the item is byte-identical.
///
/// The two readings the cli arm took against a real repository — the bare's
/// trunk unmoved, and no `land/<item>` branch — are one reading here and a
/// stricter one: the seam recorded no call, so there was nothing to move the
/// trunk with and nothing to make the branch with.
#[test]
fn a_branch_name_is_refused_before_any_git_write() {
    let scratch = &store();
    let bd = &scratch.store;
    let item = an_item(
        &scratch.store,
        "an item landed by branch name",
        Some(("ACCEPTED", SHA)),
    );
    let before_item = scratch.json(&item);
    let git = StubGit::clean();

    let ran = run(scratch, bd, &git, &item, WORK);
    assert_eq!(ran.code(), Some(2), "{}", ran.why());
    assert!(ran.why().contains(WORK), "{}", ran.why());
    assert!(
        git.calls().is_empty(),
        "nothing was read or written before the shape was judged: {:?}",
        git.calls()
    );
    assert_eq!(
        scratch.json(&item),
        before_item,
        "the item is byte-identical"
    );
}

/// A landing handed no test command lands on the review alone, and says NOT
/// TESTED on the one line a script reads — read back off the RECORD, which is
/// where a script finds it.
#[test]
fn a_landing_handed_no_test_says_not_tested_on_the_record() {
    let scratch = Board::new("land-suite-none");
    scratch.fleet_toml("[landing]\n");
    let item = an_item(
        &scratch.store,
        "an item landed with no test command",
        Some(("ACCEPTED", SHA)),
    );
    let git = StubGit::clean();

    let ran = run_untested(
        &scratch,
        &scratch.store,
        &git,
        &item,
        &StubEvents::default(),
    );
    assert_eq!(ran.code(), None, "{}", ran.why());
    let notes = scratch.json(&item);
    assert!(
        notes.contains(&format!(
            "— {}: no test command was handed to this landing",
            land::NOT_TESTED
        )),
        "the first line says NOT TESTED:\n{notes}"
    );
}

/// On a pipe, stdout is the check rows and the LANDED line and nothing else —
/// and it is the same page whether the suite took a second or none, but for the
/// one row that measures the suite.
#[test]
fn stdout_is_the_same_page_whichever_way_the_suite_was_timed() {
    // One second on the slow side, which is the smallest span the suite can be
    // timed at and still differ from the fast one — which is all this asserts.
    // The COMMAND is one word in both, because the row prints it: a landing
    // handed `sleep 1` in one run and `exit 0` in the other would differ on the
    // row this arm is asserting is the same.
    let command = format!("sh {SUITE_SCRIPT}");
    let pages: Vec<String> = ["1", "0"]
        .iter()
        .map(|sleep| {
            let scratch = Board::new(if *sleep == "1" {
                "land-page-slow"
            } else {
                "land-page-fast"
            });
            scratch.fleet_toml("[landing]\nci_marker = \"printf '[skip ci]'\"\n");
            a_sleeping_suite(&scratch, sleep);
            let item = an_item(
                &scratch.store,
                "an item whose page is read",
                Some(("ACCEPTED", SHA)),
            );
            let git = StubGit::clean();

            let ran = run_against(
                &scratch,
                &scratch.store,
                &git,
                &item,
                SHA,
                &[],
                None,
                &project(&scratch),
                Some(&command),
                &StubEvents::default(),
                &lane::Unread,
            );
            assert_eq!(ran.code(), None, "{}", ran.why());
            assert_eq!(ran.err, "", "a pipe gets no bar and no chatter");
            generalised(&ran.out, &item, scratch.root())
        })
        .collect();

    for page in &pages {
        for line in page.lines() {
            assert!(
                line.starts_with("LANDED <sha>")
                    || line.starts_with("work branch ")
                    || line.chars().next().is_some_and(|c| c.is_ascii_digit()),
                "stdout carries only check rows and the LANDED line: {line:?}"
            );
        }
    }
    assert_eq!(
        pages[0], pages[1],
        "the same page but for the suite row's own duration"
    );
}

/// The suite the arm above times, as one script whose sleep is a number in it —
/// so the command string the check row prints is the same in both runs.
const SUITE_SCRIPT: &str = "the-suite.sh";

fn a_sleeping_suite(scratch: &dyn Rooted, seconds: &str) {
    std::fs::write(
        scratch.root().join(SUITE_SCRIPT),
        format!("#!/bin/sh\nsleep {seconds}\nexit 0\n"),
    )
    .expect("the suite script is written");
}

/// One page with everything that cannot be equal between two runs taken out:
/// the shas, the item, the board's directory and the suite's duration.
fn generalised(page: &str, item: &str, root: &Path) -> String {
    let mut out = String::new();
    for line in page.lines() {
        let mut line = redact_hex(
            &line
                .replace(item, "<item>")
                .replace(&root.display().to_string(), "<root>"),
        );
        if let (Some(open), Some(close)) = (line.find("rc 0 in "), line.find("s, read from")) {
            line.replace_range(open..close, "rc 0 in <took>");
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Every run of seven or more hex characters replaced, which is every sha a
/// page can carry.
fn redact_hex(line: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for c in line.chars() {
        if c.is_ascii_hexdigit() {
            run.push(c);
            continue;
        }
        out.push_str(&flushed(&run));
        run.clear();
        out.push(c);
    }
    out.push_str(&flushed(&run));
    out
}

fn flushed(run: &str) -> String {
    if run.len() >= 7 {
        String::from("<sha>")
    } else {
        run.to_string()
    }
}

/// A conflicted squash prints every conflicted path under RETURN FOR REBASE,
/// puts the tree back and refuses. The reviewer never resolves one by hand.
#[test]
fn a_conflicted_squash_returns_for_rebase_and_puts_the_tree_back() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose squash conflicts",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let bd = &scratch.store;
    let git = StubGit::clean();
    *git.squash.lock().expect("not poisoned") = Some(Squashed::Conflicted(vec![
        FILE.to_string(),
        "b/other.rs".to_string(),
    ]));

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.out.contains("RETURN FOR REBASE")
            && ran.out.contains(FILE)
            && ran.out.contains("b/other.rs"),
        "every conflicted path is printed:\n{}",
        ran.out
    );
    assert_eq!(scratch.json(&item), before);
    let calls = git.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with(&format!("detach {TRUNK}")))
            && calls
                .iter()
                .any(|c| c == &format!("delete_branch land/{item}")),
        "the tree was put back: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c.starts_with("push_head")),
        "and nothing was pushed: {calls:?}"
    );
}

/// A staged set beyond the delivery prints BOTH sets, because the useful
/// question is which side holds the extra path.
#[test]
fn a_staged_set_beyond_the_delivery_prints_both_sets() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose index carries a leftover",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.staged = vec![
        ".beads/issues.jsonl".to_string(),
        FILE.to_string(),
        "a/leftover.rs".to_string(),
    ];

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.out.contains("STAGED (outside .beads/):")
            && ran.out.contains("DELIVERED (outside .beads/):")
            && ran.out.contains("a/leftover.rs"),
        "both sets and the extra path:\n{}",
        ran.out
    );
    assert_eq!(scratch.json(&item), before);
}

/// A trunk that moved is REBASE NEEDED with its count, and this run's suite is
/// never carried over the rebuilt branch.
#[test]
fn a_trunk_that_moved_is_rebase_needed_with_its_count() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item landed onto a trunk that moved",
        Some(("ACCEPTED", SHA)),
    );
    let before = scratch.json(&item);
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.behind = 3;

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.out.contains("REBASE NEEDED") && ran.out.contains("(3)"),
        "the count is printed:\n{}",
        ran.out
    );
    assert_eq!(scratch.json(&item), before);
    assert!(
        !git.calls().iter().any(|c| c.starts_with("push_head")),
        "the count and the push are one act, and this one stopped at the count"
    );
}

/// A rejected push prints what the remote said and NOTHING after it runs: no
/// note, no close, no branch touched.
#[test]
fn a_rejected_push_writes_no_note_and_no_close() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose push was rejected",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.push = Pushed {
        output: "remote: a pre-receive hook refused it\n! [remote rejected] HEAD -> main\n"
            .to_string(),
        code: Some(1),
    };

    let before = scratch.json(&item);
    let events = StubEvents::default();
    let ran = run_watched(scratch, bd, &git, &item, SHA, &[], None, &events);
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert_eq!(
        events.count(),
        0,
        "the events follow the note, so a refusal before it announces nothing"
    );
    assert!(
        ran.out.contains("pre-receive hook refused it"),
        "the remote's own words:\n{}",
        ran.out
    );
    let read = bd.show(&item).expect("the item reads back");
    assert_eq!(read.status, "open", "nothing after the push ran");
    assert!(
        last_landing(read.notes.as_deref().unwrap_or_default()).is_none(),
        "and no landing note was written"
    );
    assert_eq!(
        scratch.json(&item),
        before,
        "the item is byte-identical: nothing after the push ran at all"
    );
    let calls = git.calls();
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("delete_branch") || c.starts_with("delete_remote_branch")),
        "no branch was touched: {calls:?}"
    );
}

/// A push that printed no range line is an instrument that could not answer:
/// exit 3, the output printed, and nothing written.
#[test]
fn a_push_with_no_range_line_could_not_tell_and_wrote_nothing() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose push printed no range",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.push = Pushed {
        output: "Everything up-to-date\n".to_string(),
        code: Some(0),
    };

    let before = scratch.json(&item);
    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(3), "{}", ran.why());
    assert!(ran.out.contains("Everything up-to-date"), "{}", ran.out);
    assert!(
        ran.why().contains("push.out"),
        "the refusal names where the output was kept: {}",
        ran.why()
    );
    let read = bd.show(&item).expect("the item reads back");
    assert_eq!(read.status, "open");
    assert!(last_landing(read.notes.as_deref().unwrap_or_default()).is_none());
    assert_eq!(
        scratch.json(&item),
        before,
        "the item is byte-identical: no record was written"
    );
}

/// The read-back is the only witness: a note the store gives back without its
/// landing region is exit 3 printing both readings, and the landing STANDS.
#[test]
fn a_note_that_reads_back_wrong_is_could_not_tell_and_says_the_landing_stands() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose note reads back short",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let doctored = Doctored {
        inner: bd,
        drop_landing: true,
        append: None,
    };
    let git = StubGit::clean();

    let ran = run(scratch, &doctored, &git, &item, SHA);
    assert_eq!(ran.code(), Some(3), "{}", ran.why());
    assert!(
        ran.why().contains("(absent)") && ran.why().contains(&format!("LANDED {LANDED}")),
        "both readings are printed: {}",
        ran.why()
    );
    assert!(ran.why().contains("STANDS"), "{}", ran.why());
    assert!(
        ran.why().contains(&format!("bd note {item}")),
        "and the one idempotent command that repairs it: {}",
        ran.why()
    );
    // The control for the arm above: the note DID reach the item; only the
    // reading was bent.
    assert!(scratch.json(&item).contains(&format!("LANDED {LANDED}")));
}

/// The negative control: a read that answers yes to a token nothing wrote is
/// not reading this item.
#[test]
fn the_read_back_catches_a_planted_token() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose read-back is not its own",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let doctored = Doctored {
        inner: bd,
        drop_landing: false,
        append: Some(control_token().to_string()),
    };
    let git = StubGit::clean();

    let ran = run(scratch, &doctored, &git, &item, SHA);
    assert_eq!(ran.code(), Some(3), "{}", ran.why());
    assert!(ran.why().contains(control_token()), "{}", ran.why());
}

// ---- the work branch ---------------------------------------------------------

/// The classification's four answers, and the deletes present only under SAFE.
#[test]
fn the_branch_is_deleted_on_safe_alone() {
    let scratch = &store();
    let bd = &scratch.store;

    // SAFE: the tip IS the reviewed commit and the delivered paths are
    // byte-identical on the trunk.
    let item = an_item(
        &scratch.store,
        "an item whose branch is safe",
        Some(("ACCEPTED", SHA)),
    );
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());
    assert!(ran.out.contains("6. work branch      SAFE"), "{}", ran.out);
    let calls = git.calls();
    assert!(
        calls.contains(&format!("delete_branch {WORK}"))
            && calls.contains(&format!("delete_remote_branch origin {WORK}")),
        "both sides deleted: {calls:?}"
    );

    // A tip past the reviewed commit.
    let item = an_item(
        &scratch.store,
        "an item whose branch moved on",
        Some(("ACCEPTED", SHA)),
    );
    let mut git = StubGit::clean();
    git.tip = OTHER.to_string();
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());
    assert!(
        ran.out.contains("CARRIES UNLANDED WORK"),
        "the moved tip is named:\n{}",
        ran.out
    );
    assert!(no_delete(&git), "and nothing is deleted: {:?}", git.calls());

    // A non-empty diff between the reviewed commit and what landed.
    let item = an_item(
        &scratch.store,
        "an item whose landed content differs",
        Some(("ACCEPTED", SHA)),
    );
    let mut git = StubGit::clean();
    git.carried = vec![FILE.to_string()];
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());
    assert!(
        ran.out.contains("6. work branch      CARRIES ") && ran.out.contains(FILE),
        "the first path is named:\n{}",
        ran.out
    );
    assert!(no_delete(&git), "{:?}", git.calls());

    // A read that would not answer is a question, never a no.
    let item = an_item(
        &scratch.store,
        "an item whose branch cannot be read",
        Some(("ACCEPTED", SHA)),
    );
    let mut git = StubGit::clean();
    git.blind = Some(WORK.to_string());
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());
    assert!(ran.out.contains("COULD NOT TELL"), "{}", ran.out);
    assert!(no_delete(&git), "{:?}", git.calls());
}

/// Neither side of the work branch was touched. BOTH are asked, because the
/// origin delete is the one that cannot be undone.
fn no_delete(git: &StubGit) -> bool {
    let calls = git.calls();
    !calls.iter().any(|call| {
        call.starts_with(&format!("delete_branch {WORK}"))
            || call.starts_with(&format!("delete_remote_branch origin {WORK}"))
    })
}

/// A branch name the delivery note could carry that names the trunk, HEAD or
/// this act's own branch is a question, never a delete: the name is text
/// somebody wrote and SAFE ends in two destructive calls.
#[test]
fn a_branch_name_that_names_a_trunk_is_never_deleted() {
    let scratch = &store();
    let bd = &scratch.store;

    for dangerous in [TRUNK_BRANCH, "HEAD", TRUNK, "--force"] {
        let item = an_item_on_branch(
            &scratch.store,
            &format!("an item whose delivery names {dangerous}"),
            dangerous,
        );
        let mut git = StubGit::clean();
        // The tip and the diff both say SAFE, so only the name refuses.
        git.tip = SHA.to_string();
        let ran = run(scratch, bd, &git, &item, SHA);
        assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
        assert!(
            ran.out.contains("COULD NOT TELL"),
            "`{dangerous}` is classified, not deleted:\n{}",
            ran.out
        );
        let calls = git.calls();
        assert!(
            !calls
                .iter()
                .any(|call| call.starts_with("delete_branch") && call.contains(dangerous)),
            "`{dangerous}` reached a delete: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|call| call.starts_with("delete_remote_branch")),
            "`{dangerous}` reached the origin delete: {calls:?}"
        );
    }

    // The control: the same rig with an ordinary branch name deletes both
    // sides, so what refused above is the NAME and not a classifier that never
    // says SAFE.
    let item = an_item_on_branch(&scratch.store, "an item whose branch is ordinary", WORK);
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());
    assert!(
        git.calls()
            .contains(&format!("delete_remote_branch origin {WORK}")),
        "{:?}",
        git.calls()
    );
}

// ---- the release a retire reads back ---------------------------------------

/// The held case end to end: the local delete refuses because a worktree still
/// has the branch, the landing says where the delete finishes, and the note it
/// WROTE is the one the release READS — so the two halves cannot drift apart
/// over a row's grammar.
#[test]
fn a_held_local_delete_names_the_retire_and_its_own_note_reads_back_as_a_delete() {
    let scratch = &store();
    let bd = &scratch.store;
    let item = an_item(
        &scratch.store,
        "an item whose branch a worktree still holds",
        Some(("ACCEPTED", SHA)),
    );
    let mut git = StubGit::clean();
    git.held = Some(WORK.to_string());

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}", ran.why());
    assert!(
        ran.err
            .contains(&format!("work branch {WORK}: SAFE, local kept — ")),
        "the held local side says so:\n{}",
        ran.err
    );
    assert!(
        ran.err.contains("`fleet seat retire` deletes it"),
        "and names where the delete finishes:\n{}",
        ran.err
    );
    // The ORIGIN side is not held by a worktree and still goes, which is what
    // makes the line above about the local ref alone.
    assert!(
        git.calls()
            .contains(&format!("delete_remote_branch origin {WORK}")),
        "{:?}",
        git.calls()
    );

    // THE NOTE THE LANDING JUST WROTE, read back off the store the way the
    // retire reads it — never a literal typed here.
    let notes = bd
        .show(&item)
        .expect("the item reads back")
        .notes
        .unwrap_or_default();
    assert_eq!(
        land::release(&notes, Some(WORK)),
        land::Release::Delete(WORK.to_string()),
        "the landing's own SAFE row releases the branch it named:\n{notes}"
    );

    // The control on the second key: a seat standing on some OTHER branch is
    // kept, however safe this item's landing was.
    let elsewhere = "a-builder/feat/something-else";
    assert!(
        matches!(
            land::release(&notes, Some(elsewhere)),
            land::Release::Keep(why) if why.contains(elsewhere) && why.contains(WORK)
        ),
        "a branch the landing did not name is kept:\n{notes}"
    );
}

/// The three verdicts that are not SAFE, each produced by a REAL landing and
/// each read back as a keep — the control half of the arm above.
#[test]
fn a_landing_that_did_not_read_safe_releases_nothing() {
    let scratch = &store();
    let bd = &scratch.store;

    // Every rig here also holds the branch, so what keeps it is the verdict and
    // never a delete that happened to succeed at land time.
    for what in [
        "whose branch moved on",
        "whose landed content differs",
        "whose branch cannot be read",
    ] {
        let item = an_item(
            &scratch.store,
            &format!("an item {what}"),
            Some(("ACCEPTED", SHA)),
        );
        let mut git = StubGit::clean();
        git.held = Some(WORK.to_string());
        match what {
            "whose branch moved on" => git.tip = OTHER.to_string(),
            "whose landed content differs" => git.carried = vec![FILE.to_string()],
            _ => git.blind = Some(WORK.to_string()),
        }
        let ran = run(scratch, bd, &git, &item, SHA);
        assert!(ran.landed.is_ok(), "{}", ran.why());
        let notes = bd
            .show(&item)
            .expect("the item reads back")
            .notes
            .unwrap_or_default();
        assert!(
            matches!(land::release(&notes, Some(WORK)), land::Release::Keep(_)),
            "an item {what} is kept:\n{notes}"
        );
    }
}

/// Every reading that is absent, unparsable or dangerous keeps the branch.
///
/// The notes here are FORGED on purpose: a note is text somebody wrote, and a
/// SAFE row naming the trunk is exactly the shape a hand could put on an item.
#[test]
fn a_release_keeps_the_branch_on_every_reading_but_one() {
    assert_eq!(
        CRITERIA[land::WORK_BRANCH],
        "work branch",
        "the row the release reads is the one the landing writes"
    );

    // The one delete there is, as the control: everything below is this note
    // with one thing changed.
    let safe = |branch: &str| {
        format!(
            "LANDED {LANDED} on {TRUNK_BRANCH} by {REVIEWER}\n\
             5. base current     PASS       nothing\n\
             6. work branch      SAFE       {branch} — its tip is the reviewed commit\n\
             7. tree clean after PASS       nothing\n"
        )
    };
    assert_eq!(
        land::release(&safe(WORK), Some(WORK)),
        land::Release::Delete(WORK.to_string()),
        "the control deletes"
    );

    let kept: [(&str, String, Option<&str>); 9] = [
        ("a seat on no branch", safe(WORK), None),
        ("a seat on an empty branch", safe(WORK), Some("   ")),
        (
            "no landing at all",
            "DELIVERED abc — a-builder\n".to_string(),
            Some(WORK),
        ),
        (
            "a landing with no work-branch row",
            format!("LANDED {LANDED} on {TRUNK_BRANCH} by {REVIEWER}\n4. suite  PASS  green\n"),
            Some(WORK),
        ),
        (
            "a row whose verdict is not SAFE",
            safe(WORK).replace("SAFE", "CARRIES UNLANDED WORK"),
            Some(WORK),
        ),
        (
            "a SAFE row naming the trunk",
            safe(TRUNK_BRANCH),
            Some(TRUNK_BRANCH),
        ),
        (
            "a SAFE row naming the remote trunk",
            safe(TRUNK),
            Some(TRUNK),
        ),
        (
            "a SAFE row naming an option",
            safe("--force"),
            Some("--force"),
        ),
        // A RETIRE HAS NO LAND BRANCH, so the `""` handed to the refusal list
        // above cannot equal any name and the prefix is what answers here.
        (
            "a SAFE row naming another landing's branch",
            safe("land/an-other-item"),
            Some("land/an-other-item"),
        ),
    ];
    for (what, notes, held) in kept {
        assert!(
            matches!(land::release(&notes, held), land::Release::Keep(_)),
            "{what} keeps the branch:\n{notes}"
        );
    }
}

/// A delivery note naming ANOTHER landing's branch is a question too. The
/// prefix and not the item is what makes a ref a landing's, so `land/<other>`
/// is refused where only this act's own `land/<item>` was.
///
/// THE RIG PUTS THE NAME ON A BRANCH THAT WOULD OTHERWISE CLASSIFY SAFE — the
/// ref resolves to the reviewed commit and carries nothing — because an
/// unusual name that reaches could-not-tell through its RESOLUTION says
/// nothing about the guard.
#[test]
fn a_delivery_naming_another_landings_branch_is_never_deleted() {
    let scratch = &store();
    let bd = &scratch.store;
    let other = "land/an-other-item";

    let item = an_item_on_branch(bd, "an item whose delivery names another landing", other);
    let mut git = StubGit::clean();
    git.reviewed_too = Some(other.to_string());
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    assert!(
        ran.out.contains("COULD NOT TELL"),
        "`{other}` is classified, not deleted:\n{}",
        ran.out
    );
    let calls = git.calls();
    assert!(
        !calls.iter().any(|call| call.contains(other)),
        "`{other}` reached a git write: {calls:?}"
    );

    // THE CONTROL, and it is the whole arm: the same rig with an ordinary name
    // on the same seam reaches SAFE and deletes both sides, so what refused
    // above is the PREFIX.
    let ordinary = "a-builder/feat/another-hand";
    let item = an_item_on_branch(bd, "an item on an ordinary second branch", ordinary);
    let mut git = StubGit::clean();
    git.reviewed_too = Some(ordinary.to_string());
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "{}\n{}", ran.why(), ran.out);
    assert!(
        git.calls()
            .contains(&format!("delete_remote_branch origin {ordinary}")),
        "the same seam reaches SAFE for an ordinary name: {:?}",
        git.calls()
    );
}

// ---- the template and its marker ---------------------------------------------

/// The landing marker is distinct from every delivery and verdict marker: one
/// of each in one note, and each reader finds only its own.
#[test]
fn each_reader_finds_only_its_own_region() {
    // THE THREE SHIPPED TEMPLATES, rendered — not three literals typed here,
    // which would prove the arm's own strings and not the pack's markers.
    let scratch = &store();
    let packs = packs(scratch);
    let delivery = filled(
        &packs.read(DELIVERY_NOTE).expect("core carries it"),
        &[("sha", SHA), ("seat", BUILDER)],
    );
    let verdict = render(
        &marker_block(
            &packs.read(VERDICT).expect("core carries it"),
            VERDICT_MARKERS[0],
        )
        .expect("the verdict grammar names ACCEPTED"),
        &[
            ("commit", SHA),
            ("reviewer", REVIEWER),
            ("item", "fx-1"),
            ("size", "size: 1 file(s), +1, -0"),
            ("decisions", "0 accepted, 0 overruled"),
        ],
    )
    .expect("every placeholder is one this arm offers");
    let landing = render(
        &marker_block(
            &packs.read(LANDING_NOTE).expect("core carries it"),
            LANDING_MARKERS[0],
        )
        .expect("the landing grammar names LANDED"),
        &[
            ("sha", LANDED),
            ("rebased", ""),
            ("trunk", TRUNK_BRANCH),
            ("actor", REVIEWER),
            ("old", OLD),
            ("new", LANDED),
            ("commit", SHA),
            ("builder", BUILDER),
            ("tested", "suite: exit 0, rc 0"),
            ("checks", "1. reviewed commit PASS  read here"),
        ],
    )
    .expect("every placeholder is one this arm offers");
    let notes = format!("{delivery}\n{verdict}\n{landing}");

    assert_eq!(
        last_delivery(&notes).as_deref(),
        Some(delivery.as_str()),
        "the delivery region stops at the verdict"
    );
    assert_eq!(
        last_verdict(&notes).as_deref(),
        Some(verdict.as_str()),
        "the verdict region stops at the landing"
    );
    assert_eq!(
        last_landing(&notes).as_deref(),
        Some(landing.as_str()),
        "and the landing region is the landing"
    );
    // The control: with the landing taken away the verdict runs to the end, so
    // the boundary above is the landing marker's doing and not an artefact.
    let without = format!("{delivery}\n{verdict}");
    assert_eq!(last_verdict(&without).as_deref(), Some(verdict.as_str()));
    assert!(last_landing(&without).is_none());
}

/// A REGION IS ENDED BY WHAT FOLLOWS IT, NEVER BY ITS OWN KIND.
///
/// The two halves are separate: a body may not open a marker at column zero,
/// which the verdict grammar's own indentation keeps (the review suite proves
/// that end), and a reader may not stop at a marker of the kind it is reading,
/// which is this arm. Both are needed — bounding at every marker cut a verdict
/// off inside itself, and bounding at none let a landing extend one.
#[test]
fn a_region_is_ended_by_what_follows_it_and_never_by_its_own_kind() {
    let verdict = format!(
        "RETURNED WITH FINDINGS {SHA} — {REVIEWER}\n\
         findings: 1\n\
         item:    fx-1\n\
         F1 the note quotes a verdict, indented as the grammar writes it:\n\
         \x20 ACCEPTED abc — someone"
    );
    let landed = format!(
        "LANDED {LANDED} on {TRUNK_BRANCH} by {REVIEWER} — NOT TESTED: no test command was \
         handed to this landing"
    );

    assert_eq!(
        last_verdict(&verdict).as_deref(),
        Some(verdict.as_str()),
        "a verdict alone is the whole verdict"
    );
    assert_eq!(
        last_verdict(&format!("{verdict}\n{landed}")).as_deref(),
        Some(verdict.as_str()),
        "and a landing after it ends it"
    );
    assert_eq!(
        last_landing(&format!("{verdict}\n{landed}")).as_deref(),
        Some(landed.as_str())
    );
    // The half a naive bound gets wrong: a SECOND verdict does not end the
    // first — a reader that stopped at its own kind would cut a verdict off at
    // any line of its body that opened one.
    let accepted = format!("ACCEPTED {SHA} — {REVIEWER}");
    assert_eq!(
        last_verdict(&format!("{accepted}\n{verdict}")).as_deref(),
        Some(verdict.as_str()),
        "the LAST verdict is the one read, and it runs to the end"
    );
    // The control on the other side: a delivery DOES end a verdict, so the
    // bound is a rule about kinds and not a bound that never fires.
    let redelivered = format!("RE-DELIVERED {SHA} — {BUILDER}");
    assert_eq!(
        last_verdict(&format!("{verdict}\n{redelivered}")).as_deref(),
        Some(verdict.as_str())
    );
}

/// The delivery-note template with its two marker-line placeholders filled and
/// every other line left as the template wrote it: enough to anchor on.
fn filled(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template
        .lines()
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    for (name, value) in values {
        out = out.replace(&format!("<{name}>"), value);
    }
    out
}

/// The template is one of the binary's own defaults, resolves through the
/// layers, and the registry lists it — which is the registry existence test: a
/// row naming a path the set does not hold is a defect.
#[test]
fn the_landing_template_is_a_default_and_the_registry_still_resolves() {
    let scratch = Board::new("land-template");
    let template: PathBuf = packs(&scratch)
        .slot(LANDING_NOTE)
        .expect("the defaults carry the landing note");
    assert!(
        template.starts_with(&scratch.defaults_dir),
        "the template resolves out of the materialized defaults: {}",
        template.display()
    );
    let body = std::fs::read_to_string(&template).expect("the template is readable");
    assert!(
        body.starts_with(LANDING_MARKERS[0]),
        "it opens on the landing marker:\n{body}"
    );

    let registry =
        std::fs::read_to_string(scratch.defaults_dir.join("assets/shadow-registry.toml"))
            .expect("the registry is readable");
    assert!(
        registry.contains("assets/landing-note.md"),
        "and the registry names it:\n{registry}"
    );
}

// ---- the lane and its lock ---------------------------------------------------

/// Where this rig's lane and its lock sit: the machine directory the arms hand
/// in, the default `lanes/` under it, and the project's own name under the
/// lane's prefix.
fn lane_of(scratch: &dyn Rooted) -> PathBuf {
    scratch
        .root()
        .join("machine")
        .join(lane::LANES)
        .join(format!("{}a-project", lane::LANE_PREFIX))
}

/// The same lane under the name a machine cut before the prefix: what an
/// adoption starts from.
fn old_lane_of(scratch: &dyn Rooted) -> PathBuf {
    scratch
        .root()
        .join("machine")
        .join(lane::LANES)
        .join("a-project")
}

/// A project whose `[landing]` and `[core.flight]` tables the arm writes.
fn project_under(scratch: &dyn Rooted, policy: &str, flight: &str) -> Project {
    Project {
        root: scratch.root().to_path_buf(),
        name: "a-project".to_string(),
        policy: policy.parse().expect("the fixture policy parses"),
        guards: flight.parse().expect("the fixture flight table parses"),
    }
}

/// `land` TAKES THE LANE, and the file it leaves says who held it.
///
/// The two readings are separate on purpose: that a lock file exists says only
/// that something made one, and the arm that matters is that its holder line
/// names THIS item and the stamp the caller handed in — a landing that opened
/// the file and never claimed it would pass the first and fail this.
#[test]
fn a_landing_takes_the_lane_and_leaves_the_holder_on_the_lock() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose landing takes the lane",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "the landing was refused: {}", ran.why());

    let lock = lane::lock_path(&lane_of(scratch));
    let held = std::fs::read_to_string(&lock)
        .unwrap_or_else(|e| panic!("the lock at {} is left in place: {e}", lock.display()));
    assert_eq!(
        held.trim(),
        format!("{item} {AT}"),
        "the holder's item and the caller's stamp, written after the lock was taken"
    );
    // The control the reading above cannot give itself: the lane is a directory
    // the LANDING resolves, and a lock beside a path nothing named would satisfy
    // the assertion just as well.
    assert_eq!(
        lock,
        scratch
            .root
            .join("machine")
            .join("lanes")
            .join("lane-a-project.lock"),
        "beside the lane, never inside it, and under the lane's own prefixed name"
    );
}

/// A LANE CUT UNDER THE PROJECT'S BARE NAME IS TAKEN OVER, NEVER ORPHANED
///. The reading that makes this an adoption and not a fresh
/// cut is the FILE: a machine's lane is worth keeping for what is inside it, so
/// the arm writes a byte into the old directory and reads it back out of the new
/// one. An adoption that removed the old lane and cut a new one would leave the
/// directory in place and this line empty.
#[test]
fn a_lane_under_the_projects_bare_name_is_adopted_under_the_prefixed_one() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose landing adopts the older lane",
        Some(("ACCEPTED", SHA)),
    );

    // The machine as it stands before the prefix: the lane cut under the
    // project's own name, with something in it, and its lock beside it.
    let old = old_lane_of(scratch);
    std::fs::create_dir_all(&old).expect("the older lane is made");
    std::fs::write(old.join("the-cache"), "a build nobody wants to lose\n")
        .expect("the older lane's contents");
    let old_lock = lane::lock_path(&old);
    std::fs::write(&old_lock, "fx-before 2026-09-13T00:00:00Z\n").expect("the older lock");

    let bd = &scratch.store;
    let git = StubGit::clean();
    let ran = run(scratch, bd, &git, &item, SHA);
    assert!(ran.landed.is_ok(), "the landing was refused: {}", ran.why());

    let lane = lane_of(scratch);
    assert_eq!(
        std::fs::read_to_string(lane.join("the-cache")).ok(),
        Some(String::from("a build nobody wants to lose\n")),
        "the older lane's contents are under the prefixed name at {}",
        lane.display()
    );
    assert!(
        !old.exists(),
        "and nothing is left behind at {}",
        old.display()
    );
    // The lock came across on the same act, so the name a landing queues on is
    // the one the older landing's hold was taken on.
    assert!(
        !old_lock.exists(),
        "the older lock is not left beside the lane it no longer guards: {}",
        old_lock.display()
    );
    let lock = lane::lock_path(&lane);
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap_or_default().trim(),
        format!("{item} {AT}"),
        "and the landing claimed the lock under the prefixed name at {}",
        lock.display()
    );
}

/// A REAL WORKTREE IS ADOPTED THROUGH GIT AND STAYS REGISTERED. The primary
/// records where each linked worktree is, so a lane moved by rename alone is one
/// the next `git worktree prune` unregisters — a checkout stranded on the disk
/// with nothing pointing at it. The reading is the PRIMARY'S OWN LIST: the old
/// path is gone from it and the new one is in it, which a rename would have got
/// exactly backwards.
#[test]
fn a_lane_that_is_a_worktree_is_adopted_through_git_and_stays_registered() {
    let fixture = common::Fixture::new("lane-adopt");
    let primary = fixture.path("primary");
    std::fs::create_dir_all(&primary).expect("the primary is made");
    common::git(&primary, &["init", "--initial-branch", "main"]);
    std::fs::write(primary.join("a-file"), "one\n").expect("a file to commit");
    common::git(&primary, &["add", "a-file"]);
    common::git(&primary, &["commit", "-m", "one"]);

    let machine = fixture.path("machine");
    let old = machine.join(lane::LANES).join("a-project");
    std::fs::create_dir_all(old.parent().expect("the lanes directory")).expect("lanes/");
    common::git(
        &primary,
        &[
            "worktree",
            "add",
            "--detach",
            &old.display().to_string(),
            "HEAD",
        ],
    );
    assert!(old.join(".git").exists(), "the older lane is a worktree");

    let lane = lane::directory(&machine, &toml::Table::new(), "a-project")
        .expect("the lane resolves and adopts");
    assert_eq!(
        lane,
        machine.join(lane::LANES).join("lane-a-project"),
        "the lane carries the prefix"
    );
    assert!(lane.join(".git").exists(), "and the worktree moved with it");
    assert!(!old.exists(), "leaving nothing at {}", old.display());

    let listed = String::from_utf8_lossy(
        &common::git(&primary, &["worktree", "list", "--porcelain"]).stdout,
    )
    .into_owned();
    // The paths are compared as git prints them — resolved — because the box's
    // own temporary directory is reached through a symlink and a lane named by
    // its unresolved path would match nothing here whatever the adoption did.
    let names = |path: &Path| {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        listed
            .lines()
            .filter(|line| *line == format!("worktree {}", resolved.display()))
            .count()
    };
    assert_eq!(
        (names(&lane), names(&old)),
        (1, 0),
        "the primary's list names the lane where it now is and not where it was:\n{listed}"
    );
}

/// AN ADOPTION THAT WILL NOT RUN LEAVES THE LANE WHERE IT IS AND REFUSES. A
/// directory carrying a `.git` that git will not move is the shape a half-done
/// migration leaves; taking it over by rename anyway is what would strand it.
/// The two readings are separate: the verb refused, AND the older lane is still
/// there to be recovered.
#[test]
fn an_adoption_git_refuses_leaves_the_older_lane_standing() {
    let fixture = common::Fixture::new("lane-adopt-refused");
    let machine = fixture.path("machine");
    let old = machine.join(lane::LANES).join("a-project");
    std::fs::create_dir_all(&old).expect("the older lane is made");
    std::fs::write(
        old.join(".git"),
        "gitdir: /nowhere/a-repo/.git/worktrees/a-project\n",
    )
    .expect("a `.git` pointing at no repository");

    let refused = lane::directory(&machine, &toml::Table::new(), "a-project")
        .expect_err("the lane cannot be adopted");
    let said = format!("{refused:?}");
    assert!(
        said.contains(&old.display().to_string()),
        "the refusal names the lane it could not take over:\n{said}"
    );
    assert!(
        old.join(".git").exists(),
        "and the older lane is still standing at {}",
        old.display()
    );
    assert!(
        !machine.join(lane::LANES).join("lane-a-project").exists(),
        "with nothing cut beside it"
    );
}

/// A MACHINE ALREADY ON THE PREFIXED NAME ADOPTS NOTHING. The older directory is
/// left alone rather than merged or deleted: the two are indistinguishable from
/// here and only one of those is safe.
#[test]
fn a_lane_already_under_the_prefixed_name_leaves_an_older_one_alone() {
    let fixture = common::Fixture::new("lane-adopt-both");
    let machine = fixture.path("machine");
    let lanes = machine.join(lane::LANES);
    let old = lanes.join("a-project");
    let lane = lanes.join("lane-a-project");
    std::fs::create_dir_all(&old).expect("the older lane");
    std::fs::create_dir_all(&lane).expect("the lane in use");
    std::fs::write(old.join("the-cache"), "older\n").expect("the older lane's contents");

    let resolved =
        lane::directory(&machine, &toml::Table::new(), "a-project").expect("the lane resolves");
    assert_eq!(resolved, lane, "the lane in use is the prefixed one");
    assert_eq!(
        std::fs::read_to_string(old.join("the-cache")).ok(),
        Some(String::from("older\n")),
        "and the older directory is untouched at {}",
        old.display()
    );
}

/// A LANDING THAT FINDS THE LANE HELD PRINTS THE HOLDER AND WAITS.
///
/// The hold is a real file lock taken by this arm, and the landing runs on
/// another thread: what is proved is that the verb did not proceed while the
/// lock was held and did proceed once it was released. THE WAIT IS SIZED TO THE
/// LOCK AND NOT TO A CLOCK — the arm releases the lock and then joins with no
/// deadline of its own, so a slow box lengthens the arm and never fails it.
#[test]
fn a_landing_waits_on_a_held_lane_and_lands_after_it_is_released() {
    let board = store();
    let scratch = &board;
    let item = an_item(
        &scratch.store,
        "an item that queues on the lane",
        Some(("ACCEPTED", SHA)),
    );

    // The holder: another landing's lock, claimed the way `land` claims one.
    let lane = lane_of(scratch);
    std::fs::create_dir_all(lane.parent().expect("the lane has a parent"))
        .expect("the lanes directory is made");
    let lock = lane::lock_path(&lane);
    std::fs::write(&lock, "fx-elsewhere 2026-09-13T00:00:00Z\n").expect("the holder's line");
    let holder = std::fs::File::options()
        .read(true)
        .write(true)
        .open(&lock)
        .expect("the lock file opens");
    holder.lock().expect("the arm takes the lock first");

    // THE SAME BOARD ON BOTH THREADS, which is what makes it one lane: a
    // landing given a board of its own would take a lock nobody else holds.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|threads| {
        let landing = threads.spawn(|| {
            let bd = &board.store;
            let git = StubGit::clean();
            let ran = run(&board, bd, &git, &item, SHA);
            let _ = tx.send(());
            let why = ran.why();
            (ran.out, ran.landed.is_ok(), why)
        });

        // It must NOT finish while the lock is held. A landing that took no
        // lock would run straight through, and this is the reading that says
        // it did not.
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_err(),
            "the landing finished while another holder had the lane"
        );
        holder.unlock().expect("the arm releases the lock");
        drop(holder);

        let (out, landed, why) = landing.join().expect("the landing thread does not panic");
        assert!(landed, "it lands once the lane is free: {why}\n{out}");
        assert!(
            out.lines().any(|line| line.starts_with(lane::WAITING)
                && line.contains("fx-elsewhere")
                && line.contains("2026-09-13T00:00:00Z")),
            "the waiting line names the holder and since when:\n{out}"
        );
    });
}

/// A LOCK THAT CANNOT BE MADE IS EXIT 3 AND NOTHING IS WRITTEN.
///
/// `[core.flight] lanes` is pointed at a FILE, so the directory the lock would
/// sit in cannot be created. The item is read back afterwards: an instrument
/// that would not answer must not leave a landing half-written.
#[test]
fn a_lane_whose_lock_cannot_be_made_is_exit_3_with_nothing_written() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose lane will not open",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let blocked = scratch.root.join("a-file-not-a-directory");
    std::fs::write(&blocked, "not a directory\n").expect("the blocking file is written");
    let project = project_under(
        scratch,
        POLICY,
        &format!("[core.flight]\nlanes = \"{}\"\n", blocked.display()),
    );

    let ran = run_against(
        scratch,
        bd,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project,
        Some(TEST),
        &StubEvents::default(),
        &lane::Unread,
    );
    assert_eq!(
        ran.code(),
        Some(3),
        "an instrument that would not answer: {}",
        ran.why()
    );
    assert!(
        ran.why().contains("lane"),
        "and it names the lane: {}",
        ran.why()
    );
    let read = bd.show(&item).expect("the item reads");
    assert_eq!(read.status, "open", "nothing was written");
    assert!(
        last_landing(&read.notes.unwrap_or_default()).is_none(),
        "and no landing note"
    );
    assert!(
        git.calls()
            .iter()
            .all(|call| call.starts_with("rev ") || call == "is_linked_worktree"),
        "the lane is taken before the first git WRITE — the two reads ahead of it resolve the \
         commit and the tree this runs in: {:?}",
        git.calls()
    );
}

// ---- the gate's one rerun ----------------------------------------------------

/// A test command whose exit comes from a file of rc's, one per reading, and
/// which says which reading it is on its own stdout.
fn a_gate_script(scratch: &dyn Rooted, name: &str, rcs: &[i32]) -> String {
    let root = scratch.root().display().to_string();
    let rcs = rcs
        .iter()
        .map(|rc| rc.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        scratch.root().join(format!("{name}.rc")),
        format!("{rcs}\n"),
    )
    .expect("the rc file is written");
    let _ = std::fs::remove_file(scratch.root().join(format!("{name}.n")));
    std::fs::write(
        scratch.root().join(format!("{name}.sh")),
        format!(
            "#!/bin/sh\n\
             n=$(cat '{root}/{name}.n' 2>/dev/null || echo 0)\n\
             n=$((n+1))\n\
             printf '%s\\n' \"$n\" > '{root}/{name}.n'\n\
             printf 'the gate ran, reading %s\\n' \"$n\"\n\
             exit \"$(sed -n \"${{n}}p\" '{root}/{name}.rc')\"\n"
        ),
    )
    .expect("the gate script is written");
    format!("sh {name}.sh")
}

/// The box's load, as the arm sets it: a queue of readings, then the last one
/// for ever. `None` is the reading nobody took.
struct StubLoad {
    readings: Mutex<std::collections::VecDeque<Option<(f64, f64)>>>,
    last: Option<(f64, f64)>,
}

impl StubLoad {
    fn of(readings: &[Option<(f64, f64)>]) -> StubLoad {
        StubLoad {
            readings: Mutex::new(readings.iter().copied().collect()),
            last: readings.last().copied().flatten(),
        }
    }
}

impl lane::Load for StubLoad {
    fn read(&self) -> Option<(f64, f64)> {
        match self.readings.lock().expect("not poisoned").pop_front() {
            Some(reading) => reading,
            None => self.last,
        }
    }
}

/// A RED GATE IS RERUN ONCE AND A GREEN SECOND READING LANDS, with both rows in
/// the note and both readings on the stream.
#[test]
fn a_red_gate_is_rerun_once_and_a_green_second_reading_lands_with_both_rows() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose gate reddens once",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let command = a_gate_script(scratch, "rerun-green", &[1, 0]);
    let project = project_under(
        scratch,
        "[landing]\n",
        "[core.flight]\nrerun_wait_seconds = 1\n",
    );
    let events = StubEvents::default();
    let ran = run_against(
        scratch,
        bd,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project,
        Some(&command),
        &events,
        &lane::Unread,
    );
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("the landing was refused: {}\n{}", stop.message, ran.out));

    let checks = checks_of(&landed.note);
    let suite_rows: Vec<&str> = checks
        .lines()
        .filter(|line| line.contains(&command))
        .collect();
    assert_eq!(suite_rows.len(), 2, "both readings are rows:\n{checks}");
    assert!(
        suite_rows[0].contains("RED"),
        "the first reading is the red one: {}",
        suite_rows[0]
    );
    assert!(
        suite_rows[1].contains(SUITE_RERUN) && suite_rows[1].contains("PASS"),
        "the second is the rerun and it is green: {}",
        suite_rows[1]
    );
    assert!(
        suite_rows[1].contains("suite.2.log"),
        "the rerun's log sits beside the first and not over it: {}",
        suite_rows[1]
    );
    // The rows the note carries after the rerun are still the ones they were
    // named: a row whose criterion came from its POSITION would have slid.
    for criterion in ["base current", "work branch", "tree clean after"] {
        assert!(
            checks.lines().any(|line| line.contains(criterion)),
            "`{criterion}` keeps its own row after the rerun:\n{checks}"
        );
    }

    let readings: Vec<serde_json::Value> = events
        .all()
        .into_iter()
        .filter(|(kind, _, _)| kind == CHECK_READ)
        .map(|(_, _, payload)| payload)
        .collect();
    assert_eq!(
        readings.len(),
        2,
        "one `{CHECK_READ}` per reading: {readings:?}"
    );
    assert_eq!(readings[0]["reading"], 1);
    assert_eq!(readings[0]["verdict"], "red");
    assert_eq!(readings[0]["rc"], 1);
    assert_eq!(readings[1]["reading"], 2);
    assert_eq!(readings[1]["verdict"], "green");
    assert_eq!(readings[1]["rc"], 0);
    assert!(
        readings[1]["log"]
            .as_str()
            .unwrap_or_default()
            .ends_with("suite.2.log"),
        "each reading names its own log: {readings:?}"
    );
}

/// A SECOND RED REFUSES WITH BOTH LOG TAILS ON STDOUT and both readings on the
/// stream. The tails are the whole channel the flight's park needs, so this arm
/// reads the text a person would.
#[test]
fn a_second_red_reading_refuses_with_both_tails_and_writes_both_readings() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose gate stays red",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let command = a_gate_script(scratch, "rerun-red", &[1, 2]);
    let project = project_under(
        scratch,
        "[landing]\n",
        "[core.flight]\nrerun_wait_seconds = 1\n",
    );
    let events = StubEvents::default();
    let ran = run_against(
        scratch,
        bd,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project,
        Some(&command),
        &events,
        &lane::Unread,
    );
    assert_eq!(ran.code(), Some(1), "a second red refuses: {}", ran.why());
    assert!(
        ran.why().contains("rerun once"),
        "and says it reran: {}",
        ran.why()
    );
    assert!(
        ran.out.contains("the gate ran, reading 1") && ran.out.contains("the gate ran, reading 2"),
        "both log tails are printed:\n{}",
        ran.out
    );
    let readings: Vec<serde_json::Value> = events
        .all()
        .into_iter()
        .filter(|(kind, _, _)| kind == CHECK_READ)
        .map(|(_, _, payload)| payload)
        .collect();
    assert_eq!(
        readings.len(),
        2,
        "both readings reach the stream: {readings:?}"
    );
    assert_eq!(readings[0]["rc"], 1);
    assert_eq!(readings[1]["rc"], 2);
    assert!(
        readings.iter().all(|r| r["verdict"] == "red"),
        "both red: {readings:?}"
    );
    let read = bd.show(&item).expect("the item reads");
    assert_eq!(read.status, "open", "and nothing landed");
}

/// THE WAIT: the rerun holds until the box's load falls under the ceiling, and
/// the row says it quietened.
#[test]
fn the_rerun_waits_for_the_box_to_quieten_and_the_row_says_it_did() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose rerun waits",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let command = a_gate_script(scratch, "rerun-wait", &[1, 0]);
    let project = project_under(
        scratch,
        "[landing]\n",
        "[core.flight]\nrerun_wait_seconds = 30\n",
    );
    // Busy, busy, then under the ceiling: the wait has to take more than one
    // reading to end, which is what a value read once could not have shown.
    let load = StubLoad::of(&[
        Some((9.0, 4.0)),
        Some((8.0, 4.0)),
        Some((7.0, 4.0)),
        Some((1.0, 4.0)),
    ]);
    let ran = run_against(
        scratch,
        bd,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project,
        Some(&command),
        &StubEvents::default(),
        &load,
    );
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("the landing was refused: {}\n{}", stop.message, ran.out));
    let checks = checks_of(&landed.note);
    assert!(
        checks
            .lines()
            .any(|line| line.contains(SUITE_RERUN) && line.contains("the box quietened after")),
        "the second row says what the wait ended as:\n{checks}"
    );
    assert!(
        !checks.contains("expired"),
        "and it did not expire:\n{checks}"
    );
}

/// THE EXPIRY RERUNS ANYWAY AND SAYS SO — and on a busy box this is the common
/// path and not the exceptional one, which is why it has an arm of its own
/// rather than riding the wait's.
#[test]
fn a_wait_that_expires_reruns_anyway_and_the_row_says_it_expired() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose rerun waits in vain",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let git = StubGit::clean();

    let command = a_gate_script(scratch, "rerun-expire", &[1, 0]);
    let project = project_under(
        scratch,
        "[landing]\n",
        "[core.flight]\nrerun_wait_seconds = 1\n",
    );
    // A box that never quietens. The wait is a real one — a second of it — so
    // the arm reads the expiry and not a limit of zero that never looped.
    let load = StubLoad::of(&[Some((40.0, 4.0))]);
    let started = std::time::Instant::now();
    let ran = run_against(
        scratch,
        bd,
        &git,
        &item,
        SHA,
        &[],
        None,
        &project,
        Some(&command),
        &StubEvents::default(),
        &load,
    );
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("the landing was refused: {}\n{}", stop.message, ran.out));
    // A LOWER BOUND ONLY: this box runs many suites at once and contention can
    // only lengthen an elapsed reading.
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(1),
        "the wait was waited: {:?}",
        started.elapsed()
    );
    let checks = checks_of(&landed.note);
    assert!(
        checks.lines().any(|line| line.contains(SUITE_RERUN)
            && line.contains("expired")
            && line.contains("ran anyway")),
        "the second row says the wait expired and the rerun ran:\n{checks}"
    );
}

// ---- the advance strategy's record -------------------------------------------

/// A DELIVERY THAT WAS BEHIND SAYS SO ON THE NOTE'S FIRST LINE, and one that was
/// current does not. The two are one arm because the reading that matters is
/// the DIFFERENCE: a clause printed on every landing would say nothing.
#[test]
fn a_behind_delivery_names_its_own_base_and_a_current_one_does_not() {
    let scratch = &store();
    let bd = &scratch.store;

    // The push lands on OLD, and this delivery was cut from OTHER.
    let behind = an_item_delivering(
        &scratch.store,
        "an item delivered behind the trunk",
        &format!(
            "DELIVERED {SHA} — {BUILDER}\n\
             commit:  {SHA}\n\
             branch:  {WORK}\n\
             base:    {TRUNK} at {OTHER}, fetched at 2026-09-12T00:00:00Z\n\
             files:   {FILE}\n\
             checks:  AC1 green, read from the arm's own status\n\
             suite:   the workspace suite, rc 0\n\
             spec corrections: none\n\
             not proven: what this arm did not run\n\
             decisions: none\n\
             covers: R8"
        ),
        Some(("ACCEPTED", SHA)),
    );
    let ran = run(scratch, bd, &StubGit::clean(), &behind, SHA);
    let note = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("refused: {}\n{}", stop.message, ran.out))
        .note
        .clone();
    let first = note.lines().next().unwrap_or_default();
    assert!(
        first.contains(&format!("rebased from {OTHER}")),
        "the delivery's own base is named beside the one it landed on: {first}"
    );

    // The control: the rig's own delivery names the base the push landed on.
    let current = an_item(
        &scratch.store,
        "an item delivered on the tip",
        Some(("ACCEPTED", SHA)),
    );
    let ran = run(scratch, bd, &StubGit::clean(), &current, SHA);
    let note = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("refused: {}\n{}", stop.message, ran.out))
        .note
        .clone();
    let first = note.lines().next().unwrap_or_default();
    assert!(
        !first.contains("rebased from"),
        "a delivery that was current says nothing: {first}"
    );
}

/// `REBASE NEEDED` is the line the flight reads to tell a trunk that moved from
/// a landing that is wrong, so it is a constant and printed at column zero.
#[test]
fn a_trunk_that_moved_prints_the_rebase_needed_line_at_column_zero() {
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item whose trunk moved",
        Some(("ACCEPTED", SHA)),
    );
    let bd = &scratch.store;
    let mut git = StubGit::clean();
    git.behind = 2;

    let ran = run(scratch, bd, &git, &item, SHA);
    assert_eq!(ran.code(), Some(1), "it refuses: {}", ran.why());
    assert!(
        ran.out
            .lines()
            .any(|line| line.starts_with(REBASE_NEEDED) && line.contains("(2)")),
        "the line the flight reads, at column zero:\n{}",
        ran.out
    );
}

// ---- the store the landing does not shell out to -------------------------------

/// THE WHOLE LANDING RUNS WITH NO `bd` ON THE `PATH`, export gate and all.
///
/// `store::BD` is a bare name resolved on the process's own `PATH` — the seam
/// the store suite's shim enters through — so a `PATH` holding one directory
/// that is not the store's resolves nothing, and a landing that still lands is
/// one that made no `bd` subprocess. That is what puts the arms of this file in
/// memory rather than in a subprocess, and it is the reading that says so.
///
/// `sh` IS STILL THERE, linked into that directory, because the gate suite is a
/// real shell child and stays one: what is taken away is the store and nothing
/// else.
#[test]
fn a_landing_on_the_board_held_in_memory_runs_with_no_bd_on_the_path() {
    let _path = PATH_LOCK.lock().expect("not poisoned");
    let scratch = &store();
    let item = an_item(
        &scratch.store,
        "an item landed with no store on the path",
        Some(("ACCEPTED", SHA)),
    );
    let bin = scratch.root.join("only-sh");
    std::fs::create_dir_all(&bin).expect("the one PATH directory is made");
    std::os::unix::fs::symlink("/bin/sh", bin.join("sh")).expect("sh is linked into it");
    assert!(
        !bin.join("bd").exists(),
        "and the store's binary is not: {}",
        bin.display()
    );

    let stub = StubGit::clean();
    let ran = with_only_on_path(&bin, || run(scratch, &scratch.store, &stub, &item, SHA));
    let landed = ran
        .landed
        .as_ref()
        .unwrap_or_else(|stop| panic!("the landing was refused: {}\n{}", stop.message, ran.out));

    // The export gate is the one the recording fake could not pass at all: it
    // fingerprints a file, so it passes only because the store wrote one.
    let export = scratch.root.join(fleet_core::store::EXPORT);
    let written = std::fs::read_to_string(&export).expect("the store's export is a file");
    assert!(
        written.contains(&item),
        "carrying the item that was landed:\n{written}"
    );
    assert!(
        landed.note.contains(&format!("LANDED {LANDED}")),
        "and the landing note was written:\n{}",
        landed.note
    );
    assert_eq!(
        scratch.store.show(&item).expect("the item reads").status,
        "closed",
        "and the close landed"
    );
}

// ---- the search path the project's children run under -------------------------

/// A directory holding one executable `tt-probe` and nothing else. The name is
/// this suite's own, so prepending the directory to the process `PATH` shadows
/// nothing another arm resolves.
fn a_probe_dir(scratch: &dyn Rooted, name: &str, rc: i32) -> PathBuf {
    let dir = scratch.root().join(name);
    std::fs::create_dir_all(&dir).expect("the probe directory is made");
    let probe = dir.join("tt-probe");
    std::fs::write(
        &probe,
        format!("#!/bin/sh\nprintf '{name}\\n'\nexit {rc}\n"),
    )
    .expect("the probe is written");
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755))
        .expect("the probe is executable");
    dir
}

/// THE SUITE RUNS UNDER THE CONSTRUCTED PATH, NOT THE AMBIENT ONE, AND THE
/// READING NAMES THE PATH IT RAN UNDER.
///
/// Both halves are read off ONE gate command, `tt-probe`, with one ambient
/// `PATH` in force for both: the reading handed a constructed path resolves the
/// probe that exits 0 and the landing lands, and the reading handed none
/// resolves the probe first on the ambient path — the broken tool the service
/// was started with — and the gate refuses. A change that stopped setting the
/// child's `PATH` turns the first half red; one that set it always turns the
/// second half green.
#[test]
fn the_suite_runs_under_the_constructed_path_and_the_reading_names_it() {
    let _path = PATH_LOCK.lock().expect("not poisoned");
    let scratch = &store();
    let bd = &scratch.store;

    // The ambient path: the broken probe, and the `sh` every gate child is.
    let ambient = a_probe_dir(scratch, "ambient", 1);
    std::os::unix::fs::symlink("/bin/sh", ambient.join("sh")).expect("sh is linked into it");
    // The constructed path: the working probe, and the system directories. It
    // holds nothing of the ambient one.
    let good = a_probe_dir(scratch, "constructed", 0);
    let constructed = format!("{}:/usr/bin:/bin", good.display());
    assert!(
        !constructed.contains(&ambient.display().to_string()),
        "the constructed path holds none of the ambient one: {constructed}"
    );

    let project = project_under(
        scratch,
        "[landing]\n",
        "[core.flight]\nrerun_wait_seconds = 0\n",
    );

    let under_the_constructed_path = an_item(
        bd,
        "an item landed under the constructed path",
        Some(("ACCEPTED", SHA)),
    );
    let under_the_ambient_path = an_item(
        bd,
        "an item landed under the ambient path",
        Some(("ACCEPTED", SHA)),
    );

    let green = StubEvents::default();
    let red = StubEvents::default();
    let (green_ran, red_ran) = with_only_on_path(&ambient, || {
        let green_ran = run_against_path(
            scratch,
            bd,
            &StubGit::clean(),
            &under_the_constructed_path,
            SHA,
            &[],
            None,
            &project,
            Some("tt-probe"),
            &green,
            &lane::Unread,
            &constructed,
            REVIEWER,
        );
        let red_ran = run_against_path(
            scratch,
            bd,
            &StubGit::clean(),
            &under_the_ambient_path,
            SHA,
            &[],
            None,
            &project,
            Some("tt-probe"),
            &red,
            &lane::Unread,
            "",
            REVIEWER,
        );
        (green_ran, red_ran)
    });

    let landed = green_ran.landed.as_ref().unwrap_or_else(|stop| {
        panic!(
            "the constructed path resolves the working probe: {}\n{}",
            stop.message, green_ran.out
        )
    });
    assert!(
        landed.note.contains("4. suite            PASS"),
        "and the suite row is green:\n{}",
        landed.note
    );
    let (_, reading) = green.one(CHECK_READ);
    keys_agree(CHECK_READ, &reading, &[]);
    assert_eq!(
        reading["path"],
        serde_json::json!(constructed),
        "the reading names the path it ran under"
    );
    assert_eq!(reading["rc"], serde_json::json!(0));

    // THE OTHER HALF, on the same command and the same ambient path: with no
    // constructed path the child inherits, and the broken probe is what it
    // finds.
    let stop = red_ran
        .landed
        .as_ref()
        .expect_err("the ambient path resolves the broken probe");
    assert_eq!(
        stop.code, 1,
        "a refusal, not an instrument: {}",
        stop.message
    );
    let readings: Vec<serde_json::Value> = red
        .all()
        .into_iter()
        .filter(|(kind, _, _)| kind == CHECK_READ)
        .map(|(_, _, payload)| payload)
        .collect();
    assert_eq!(readings.len(), 2, "both readings reached the stream");
    for reading in &readings {
        assert_eq!(reading["rc"], serde_json::json!(1));
        assert_eq!(
            reading["path"],
            serde_json::Value::Null,
            "a child that inherited names no constructed path"
        );
    }
}

// ---- a run's landing ---------------------------------------------------------

/// The hold a run raises on its own record, as `hold` writes it and `clear`
/// clears it. `answered_by` is `None` for the run nobody answered.
fn a_run(store: &dyn Store, answered_by: Option<&str>) -> String {
    let run = store
        .create(
            &fleet_core::store::NewItem {
                title: "a run of takeoff",
                description: "a run's record",
                item_type: "task",
                labels: &[fleet_core::item::run::LABEL],
            },
            REVIEWER,
        )
        .expect("the run's record is filed");
    store
        .note(
            &run,
            &format!(
                "PARKED {run} — ask\n\
                 branch:  (run)\n\
                 commit:  {OLD}\n\
                 hold:    a-hold\n\
                 QUESTION Accept the delivery?\n\
                 A. accept and land\n\
                 B. return to the builder"
            ),
            REVIEWER,
        )
        .expect("the park is on the run");
    if let Some(who) = answered_by {
        store
            .note(
                &run,
                &format!("ANSWERED a-hold — {who}\nletter:  A\ntext:    (none)"),
                who,
            )
            .expect("the answer is on the run");
    }
    run
}

/// The landing as a WORKFLOW calls it: the `by` is the run's record id, which
/// is the whole of what tells one from a seat's own landing.
fn run_as(
    scratch: &dyn Rooted,
    store: &dyn Store,
    git: &StubGit,
    item: &str,
    by: &str,
    events: &StubEvents,
) -> Ran {
    run_against_path(
        scratch,
        store,
        git,
        item,
        SHA,
        &[],
        None,
        &project(scratch),
        Some(TEST),
        events,
        &lane::Unread,
        "",
        by,
    )
}

/// A RUN LANDS AS THE REVIEWER, WITH THE RUN NAMED BESIDE IT. The caller is the
/// run's record, the item is held by the `[core] reviewer` — `deliver` hands
/// every item to that seat, so on a workflow's landing the holder is never the
/// caller — and the run carries that reviewer's own answer. The landing stands,
/// and each of the three places one name is asked for carries both.
#[test]
fn a_run_lands_as_the_reviewer_and_names_the_run_beside_it() {
    let board = store();
    let scratch = &board;
    let item = an_item(
        &scratch.store,
        "an item a run lands",
        Some(("ACCEPTED", SHA)),
    );
    let run = a_run(&scratch.store, Some(REVIEWER));
    let events = StubEvents::default();

    let git = StubGit::clean();
    let ran = run_as(scratch, &scratch.store, &git, &item, &run, &events);
    let landed = ran.landed.as_ref().unwrap_or_else(|stop| {
        panic!("a run's landing was refused: {}\n{}", stop.message, ran.out);
    });
    assert_eq!(landed.sha, LANDED, "the push's own range line");

    // `item.landed`: the reviewer is the actor whose act it is, and `run` is
    // what carried it. The stub's caller is neither name by accident — the run
    // id is what was handed in, and the reviewer is what came back.
    assert_ne!(run, REVIEWER);
    let (actor, landing) = events.one(ITEM_LANDED);
    assert_eq!(actor, REVIEWER, "the landing is the reviewer's act");
    keys_agree(ITEM_LANDED, &landing, &[]);
    assert_eq!(landing["run"], serde_json::json!(run));
    assert_eq!(landing["sha"], serde_json::json!(LANDED));

    // The close: written by the reviewer, with the run named beside the sha.
    let wrote = scratch.store.wrote();
    let closed = wrote
        .iter()
        .find(|line| line.starts_with(&format!("close {item} ")))
        .unwrap_or_else(|| panic!("the item was closed: {wrote:?}"));
    assert_eq!(
        closed,
        &format!("close {item} landed {LANDED} through run {run} {REVIEWER}")
    );

    // The note a person reads afterwards names both on its own first line.
    assert!(
        landed
            .note
            .contains(&format!("by {REVIEWER} through run {run}")),
        "{}",
        landed.note
    );
}

/// THE HOLDER GATE STILL HOLDS UNDER A RUN. It compares the item's assignee to
/// the reviewer the run acts as, so an item some other seat holds is refused
/// exactly as a seat's own landing of it would be.
#[test]
fn a_runs_landing_is_refused_an_item_another_seat_holds() {
    let board = store();
    let scratch = &board;
    let item = an_item(
        &scratch.store,
        "an item another seat holds",
        Some(("ACCEPTED", SHA)),
    );
    scratch
        .store
        .assign(&item, BUILDER, REVIEWER)
        .expect("the builder holds it");
    let run = a_run(&scratch.store, Some(REVIEWER));

    let git = StubGit::clean();
    let ran = run_as(
        scratch,
        &scratch.store,
        &git,
        &item,
        &run,
        &StubEvents::default(),
    );
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert_eq!(
        ran.why(),
        format!(
            "{item} is held by `{BUILDER}` and not by `{REVIEWER}` — whoever closes an item lands \
             its work"
        )
    );
    assert!(
        git.calls()
            .iter()
            .all(|call| !call.starts_with("push_head")),
        "nothing was pushed: {:?}",
        git.calls()
    );
}

/// A RUN LANDS ON AN ANSWER OR NOT AT ALL. A seat answers for its own landing
/// by making it; a run answers for nothing, so a run whose hold nobody cleared
/// is refused — with the run named, because the answer is on its record.
#[test]
fn a_runs_landing_is_refused_where_nobody_cleared_its_hold() {
    let board = store();
    let scratch = &board;
    let item = an_item(
        &scratch.store,
        "an item nobody cleared a hold for",
        Some(("ACCEPTED", SHA)),
    );
    let unanswered = a_run(&scratch.store, None);

    let git = StubGit::clean();
    let ran = run_as(
        scratch,
        &scratch.store,
        &git,
        &item,
        &unanswered,
        &StubEvents::default(),
    );
    assert_eq!(ran.code(), Some(1), "{}", ran.why());
    assert!(
        ran.why()
            .contains(&format!("run {unanswered} carries no cleared hold")),
        "{}",
        ran.why()
    );
    assert!(
        git.calls()
            .iter()
            .all(|call| !call.starts_with("push_head")),
        "nothing was pushed: {:?}",
        git.calls()
    );

    // THE CONTROL, on the same board and the same item: the one thing that
    // changed is that somebody answered, and the landing stands.
    let answered = a_run(&scratch.store, Some(REVIEWER));
    let git = StubGit::clean();
    let ran = run_as(
        scratch,
        &scratch.store,
        &git,
        &item,
        &answered,
        &StubEvents::default(),
    );
    assert!(
        ran.landed.is_ok(),
        "the same landing with an answer behind it: {}",
        ran.why()
    );
}
