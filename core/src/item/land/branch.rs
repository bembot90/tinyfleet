//! The work branch: what a landing classifies it as, and what a retire reads back off the landed entry.

use super::{Wiring, LAND_PREFIX, SAFE};
use crate::entry::{self, Entry, Timeline};
use crate::item::{show, TRUNK, TRUNK_BRANCH};

// ---- the work branch ---------------------------------------------------------

/// What the work branch holds that the trunk does not.
pub(super) enum Classification {
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
    pub(super) fn verdict(&self) -> &'static str {
        match self {
            Classification::Safe => SAFE,
            Classification::Ahead(_) => "CARRIES UNLANDED WORK",
            Classification::Carries(_) => "CARRIES",
            Classification::CouldNotTell(_) => "COULD NOT TELL",
            Classification::NotGiven => "NOT GIVEN",
        }
    }

    /// The classification as the landed entry records it: the reading's kind,
    /// without the count or the path its row's evidence already names.
    pub(super) fn recorded(&self) -> entry::Classification {
        match self {
            Classification::Safe => entry::Classification::Safe,
            Classification::Ahead(_) => entry::Classification::CarriesUnlandedWork,
            Classification::Carries(_) => entry::Classification::Carries,
            Classification::CouldNotTell(_) => entry::Classification::CouldNotTell,
            Classification::NotGiven => entry::Classification::NotGiven,
        }
    }

    pub(super) fn evidence(&self, branch: Option<&str>) -> String {
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
/// The branch comes off the record — a delivery's entry, or the landed entry a
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

// ---- what a retire reads back off the landed entry ---------------------------

/// What a landing's own entry leaves a later act to do with the work branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    /// The landing classified this branch [`SAFE`] and the seat that is going
    /// is standing on it. Deleting it finishes what the landing could not.
    Delete(String),
    /// Nothing is deleted, and this says why in the words a person reads.
    Keep(String),
}

/// What is to be done with `held` — the branch the retiring seat's worktree is
/// on — given that seat's item's timeline.
///
/// A landing runs while the seat that delivered still holds its worktree, so a
/// SAFE branch's LOCAL delete exits 1 there and the ref outlives the landing.
/// The retire that takes that worktree is the act that can finish it, and this
/// is what it reads: the classification the landing ALREADY MADE, off the
/// timeline's last landed entry. Nothing here re-derives one — a second
/// classifier would be a second thing that can say SAFE where the first said
/// CARRIES.
///
/// EVERY ANSWER BUT ONE IS `Keep`, and that is the shape rather than the mood.
/// The single delete needs all of: a landing on the record, its work branch
/// classified safe, a branch named beside that classification, that name
/// passing the same refusal list the landing's own delete passes, and the
/// going seat's worktree standing on exactly that name. A reading that is
/// absent or disagrees keeps the branch, because an over-eager delete here
/// takes work nobody can get back.
pub fn release(timeline: &[Entry], held: Option<&str>) -> Release {
    let Some(held) = held.map(str::trim).filter(|name| !name.is_empty()) else {
        return Release::Keep("the retiring seat's worktree is on no branch".to_string());
    };
    let Some((_, landing)) = Timeline(timeline).last_landing() else {
        return Release::Keep(format!("`{held}` — the item carries no landing"));
    };
    let classified = &landing.work_branch;
    if classified.classification != entry::Classification::Safe {
        return Release::Keep(format!(
            "`{held}` — the landing reads `{}`",
            show::word(&classified.classification)
        ));
    }
    let named = classified.branch.as_deref().unwrap_or_default();
    if named != held {
        return Release::Keep(format!(
            "`{held}` — the landing's {SAFE} row names `{named}`, which is another branch"
        ));
    }
    // THE SAME REFUSAL LIST THE LANDING'S OWN DELETE PASSES, asked again here
    // because this reads the record rather than the value the landing
    // classified: there is no land branch at a retire, and no ordinary work
    // branch is named by the empty string.
    if let Some(why) = unsafe_to_delete(named, "") {
        return Release::Keep(format!("`{held}` — {why}"));
    }
    Release::Delete(named.to_string())
}

/// SAFE is two readings and not one: the tip has not moved past what was
/// reviewed, AND the delivered paths are byte-identical between the reviewed
/// commit and what landed. Either alone would delete a branch holding work.
pub(super) fn classify(
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
    // THE NAME IS RECORD TEXT, and SAFE ends in two destructive git calls. A
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
