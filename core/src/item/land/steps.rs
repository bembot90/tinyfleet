//! The landing's steps, (b) to (n), in the order run takes them.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::branch::{classify, Classification};
use super::licence::{accepted, licensed, neither};
use super::porcelain::{fingerprint, full, is_store_export, outside_also, outside_store, range};
use super::rows::Rows;
use super::suite::{announce, stdin_command, suite_check, Reading, FIRST_READING, NO_READING};
use super::tree::Tree;
use super::{
    rc_word, remote, Landing, Pushed, Squashed, Wiring, LANDED, LAND_PREFIX, REBASE_NEEDED, SAFE,
    UNTESTED,
};
use crate::entry::{self, Body, SuiteRun, Timeline};
use crate::item::deliver::reviewer_of;
use crate::item::run::run_record;
use crate::item::{
    recorded, signal, Project, Stop, Unrecorded, CHECK_READ, ITEM_ENTRY, TRUNK, TRUNK_BRANCH,
};
use crate::policy;
use crate::seat::actor::{Actor, ActorKind};
use crate::seat::identity::SeatId;
use crate::store::types::ExportSpec;
use crate::store::{Item, Status};

/// Which side of the work branch a delete line is about.
const LOCAL: &str = "local";

/// What the local line gains where the ref is still held: the act that can
/// finish the delete, named where a person meets the exit-1.
const RETIRE_DELETES: &str =
    "; `fleet seat retire` deletes it off the landed entry when the seat holding it goes";

// ---- what the steps carry ----------------------------------------------------

/// One landing's fixed facts once (c) has resolved the item: the commit, the
/// arguments and the wiring.
pub(super) struct Act<'a> {
    pub(super) commit: &'a str,
    pub(super) landing: &'a Landing<'a>,
    pub(super) wiring: &'a Wiring<'a>,
}

/// What (c) read off the record and every later step names.
pub(super) struct Record {
    pub(super) item: Item,
    pub(super) acting: Actor,
    closer_id: String,
    by_run: Option<Item>,
    pub(super) work_branch: Option<String>,
    builder: String,
}

/// What (e) read: the whole staged set the marker is asked over, and the
/// delivery's own paths the work-branch row compares.
pub(super) struct Staged {
    pub(super) all_staged: Vec<String>,
    pub(super) delivery_paths: Vec<String>,
}

/// What (h) ran: the command handed in, if any, and its readings.
pub(super) struct Suite {
    command: Option<String>,
    readings: Vec<Reading>,
}

// ---- the steps ---------------------------------------------------------------

pub(super) fn the_checkout(
    linked: bool,
    landing: &Landing,
    wiring: &Wiring,
) -> Result<Option<ExportSpec>, Stop> {
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
    Ok(export)
}

pub(super) fn the_record(
    out: &mut dyn Write,
    rows: &mut Rows,
    commit: &str,
    landing: &Landing,
    wiring: &Wiring,
) -> Result<Record, Stop> {
    // (c) THE RECORD. What the item says about itself, and the verdict that is
    // the whole licence to squash anything.
    //
    // THE ITEM IS RESOLVED HERE, ONCE, and the landing names the id the store
    // answered from this line on: the landed entry, the events, the land
    // branch and the close all carry the full id, whatever part of it was
    // typed.
    let item = wiring.store.show(landing.item)?;
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
    Ok(Record {
        item,
        acting,
        closer_id,
        by_run,
        work_branch,
        builder,
    })
}

pub(super) fn the_trunk(
    out: &mut dyn Write,
    act: &Act,
    item: &Item,
    tree: &mut Tree,
) -> Result<String, Stop> {
    let Act { commit, wiring, .. } = *act;
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
    Ok(land_branch)
}

pub(super) fn the_export(
    export: &Option<ExportSpec>,
    wiring: &Wiring,
) -> Result<Vec<String>, Stop> {
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
                .map(|line| line.path.clone())
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
    Ok(store_paths)
}

/// The rest of (e): the staged set and the check on it.
pub(super) fn the_staged_set(
    out: &mut dyn Write,
    rows: &mut Rows,
    act: &Act,
    export: &Option<ExportSpec>,
    store_paths: &[String],
) -> Result<Staged, Stop> {
    let Act {
        commit,
        landing,
        wiring,
    } = *act;
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
    Ok(Staged {
        all_staged,
        delivery_paths,
    })
}

pub(super) fn the_marker(
    out: &mut dyn Write,
    rows: &mut Rows,
    all_staged: &[String],
    wiring: &Wiring,
) -> Result<String, Stop> {
    // (f) THE MARKER, read through the census reader.
    let marker_command = marker_of(wiring.project)?;
    let marker = match &marker_command {
        // THE STAGED PATHS, all of them: §4(f) says the staged paths, and a
        // marker command that decides whether a pipeline may be skipped is one
        // the store's own files are as much a part of as the delivery's.
        Some(command) => {
            stdin_command(command, &wiring.project.root, wiring.child_path, all_staged)?
        }
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
    Ok(marker)
}

pub(super) fn the_commit(act: &Act, record: &Record, marker: &str) -> Result<PathBuf, Stop> {
    let Act {
        landing, wiring, ..
    } = *act;
    let Record {
        item,
        closer_id,
        builder,
        ..
    } = record;
    // (g) THE COMMIT, from a message file, its sha read in the same act.
    let work_dir = landing.machine_dir.join("land").join(item.id.as_str());
    std::fs::create_dir_all(&work_dir).map_err(|e| {
        Stop::could_not_tell(format!(
            "the landing's own directory {} could not be made: {e}",
            work_dir.display()
        ))
    })?;
    let message_path = work_dir.join("message");
    std::fs::write(&message_path, message(item, marker, closer_id, builder)).map_err(|e| {
        Stop::could_not_tell(format!(
            "the commit message at {} could not be written: {e}",
            message_path.display()
        ))
    })?;
    wiring
        .git
        .commit_message_file(&message_path)
        .map_err(Stop::could_not_tell)?;
    Ok(work_dir)
}

pub(super) fn the_suite(
    out: &mut dyn Write,
    rows: &mut Rows,
    act: &Act,
    work_dir: &Path,
    acting: &Actor,
) -> Result<Suite, Stop> {
    let Act {
        landing, wiring, ..
    } = *act;
    // (h) THE SUITE, the command the caller handed in, or none at all and the
    // landing says NOT TESTED, with the one rerun a red gets before it refuses.
    let suite_command = landing
        .test
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_string);
    let readings = suite_check(out, rows, &suite_command, work_dir, landing, acting, wiring)?;
    Ok(Suite {
        command: suite_command,
        readings,
    })
}

pub(super) fn the_push(
    out: &mut dyn Write,
    rows: &mut Rows,
    act: &Act,
    work_dir: &Path,
    tree: &mut Tree,
) -> Result<(Pushed, String), Stop> {
    let Act { wiring, .. } = *act;
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
    Ok((pushed, kept))
}

pub(super) fn the_range_line(
    out: &mut dyn Write,
    pushed: &Pushed,
    kept: &str,
    item: &Item,
    wiring: &Wiring,
) -> Result<(String, String), Stop> {
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
    Ok((old, sha))
}

pub(super) fn the_end_state(
    out: &mut dyn Write,
    rows: &mut Rows,
    act: &Act,
    sha: &str,
    delivery_paths: &[String],
    work_branch: &Option<String>,
    land_branch: &str,
) -> Result<Classification, Stop> {
    let Act { commit, wiring, .. } = *act;
    // The two rows the landed entry carries about the state a landing ends in,
    // read before the entry is written and acted on after it.
    let classification = classify(
        commit,
        sha,
        delivery_paths,
        work_branch.as_deref(),
        land_branch,
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
            format!(
                "git status: {}",
                after
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        },
    );
    Ok(classification)
}

pub(super) fn the_landed_entry(
    act: &Act,
    record: &Record,
    rows: &Rows,
    suite: &Suite,
    old: &str,
    sha: &str,
    classification: &Classification,
) -> Result<String, Stop> {
    let Act { commit, wiring, .. } = *act;
    let Record {
        item,
        acting,
        by_run,
        work_branch,
        ..
    } = record;
    let Suite {
        command: suite_command,
        readings,
    } = suite;
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
        sha: sha.to_string(),
        old: old.to_string(),
        squash_of: commit.to_string(),
        run: by_run.as_ref().map(|record| record.id.to_string()),
        test,
        checks: rows.rows(),
        work_branch: entry::WorkBranch {
            branch: work_branch.clone(),
            classification: classification.recorded(),
        },
    });
    let entry = recorded(wiring.store, &item.id, &landed, acting).map_err(|unrecorded| {
        let why = match unrecorded {
            Unrecorded::NotWritten(e) => format!("the landed entry was not written: {e}"),
            Unrecorded::Unconfirmed(why) => why,
        };
        Stop::could_not_tell(format!(
            "{why}\n  the landing {sha} STANDS on {TRUNK_BRANCH}"
        ))
    })?;
    Ok(entry)
}

pub(super) fn the_readings_and_signal(
    act: &Act,
    record: &Record,
    suite: &Suite,
    entry: &str,
) -> Result<(), Stop> {
    let Act { wiring, .. } = *act;
    let Record { item, acting, .. } = record;
    let Suite {
        command: suite_command,
        readings,
    } = suite;
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
            acting,
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
    for reading in readings {
        announce(
            &item.id,
            CHECK_READ,
            acting,
            wiring,
            reading.payload(&item.id, suite_command.as_deref()),
        )?;
    }
    // The landed entry's signal. What carried the landing, what tested it and
    // the sha it pushed are the entry's, and a reader reads them there.
    signal(wiring.events, acting, &item.id, entry, "landed").map_err(|e| {
        Stop::could_not_tell(format!(
            "{ITEM_ENTRY} did not reach the stream: {e}\n  the landing on {} STANDS and its \
             landed entry is on the record",
            item.id
        ))
    })?;
    Ok(())
}

pub(super) fn the_close(act: &Act, record: &Record, sha: &str) -> Result<(), Stop> {
    let Act {
        landing, wiring, ..
    } = *act;
    let Record {
        item,
        acting,
        by_run,
        ..
    } = record;
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
        .close(&item.id, &reason, acting)
        .map_err(|e| unclosed(&item.id, sha, &e.to_string()))?;
    let closed = wiring.store.show(&item.id)?;
    if closed.status != Status::Closed {
        return Err(unclosed(
            &item.id,
            sha,
            &format!("it read back with status `{}`", closed.status),
        ));
    }
    Ok(())
}

pub(super) fn the_work_branch(
    out: &mut dyn Write,
    err: &mut dyn Write,
    wiring: &Wiring,
    work_branch: &Option<String>,
    classification: &Classification,
) {
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
}

pub(super) fn the_checkout_put_back(
    out: &mut dyn Write,
    err: &mut dyn Write,
    tree: &mut Tree,
    sha: &str,
    wiring: &Wiring,
) {
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
