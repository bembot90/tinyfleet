//! fleet pack check, add, remove and list: the pack family's verbs, over core's
//! pack, lock and resolver.

use crate::exit::Exit;
use crate::ui::{Stream, Tone, Ui};
use clap::Subcommand;
use fleet_controller::{clock, platform};
use fleet_core::{add, defaults, lock, pack, remove, resolve};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(crate) enum PackVerb {
    /// validate one pack, and its layering with --over
    #[command(long_about = "\
validate one pack against the format. --over resolves it above the packs
named, in the order given, and refuses a collision or an unlisted shadow.")]
    Check {
        /// the pack directory to validate
        dir: PathBuf,
        /// a pack this one resolves above; repeatable, lowest last
        #[arg(long, value_name = "DIR")]
        over: Vec<PathBuf>,
    },

    /// fetch one pack by git source and version and pin it in packs.lock
    #[command(long_about = "\
fetch one pack by git source and version into the packs dir and pin it in
packs.lock. <source> may end in //<subdirectory>.")]
    Add {
        /// the git URL or local path, optionally ending in //<subdirectory>
        source: String,
        /// a tag, a branch, or sha:<40 hex>
        #[arg(long, value_name = "VERSION")]
        version: String,
        /// where packs are installed
        #[arg(long = "packs-dir", value_name = "DIR")]
        packs_dir: Option<PathBuf>,
        /// the lock file to pin in
        #[arg(long, value_name = "FILE")]
        lock: Option<PathBuf>,
    },

    /// take one pack out of the packs dir and drop its line from packs.lock
    #[command(long_about = "\
take one pack out: its directory, then its line in packs.lock, keyed by the
source as typed. Refuses the binary's own defaults, a pack another installed
pack imports, and a source the lock does not hold.")]
    Remove {
        /// the source as it was typed to `pack add`
        source: String,
        /// where packs are installed
        #[arg(long = "packs-dir", value_name = "DIR")]
        packs_dir: Option<PathBuf>,
        /// the lock file to drop the line from
        #[arg(long, value_name = "FILE")]
        lock: Option<PathBuf>,
    },

    /// print packs.lock as a table
    #[command(long_about = "\
print packs.lock as a table: name, source, version, commit and fetched, one row
per pack, in source order. An empty lock, or a lock path that is not there,
prints the header alone.")]
    List {
        /// the lock file to read
        #[arg(long, value_name = "FILE")]
        lock: Option<PathBuf>,
    },
}

// 0 is a valid pack, 1 is a pack with defects or a layering that refuses, and 2
// is a caller who did not say what to check: the three are distinct because a
// script that cannot tell a bad pack from a bad invocation reports the wrong
// one.
pub(crate) fn command(ui: &Ui, verb: &PackVerb) -> Exit {
    match verb {
        PackVerb::Check { dir, over } => pack_check(ui, dir, over),
        PackVerb::Add {
            source,
            version,
            packs_dir,
            lock,
        } => pack_add(ui, source, version, packs_dir.as_deref(), lock.as_deref()),
        PackVerb::Remove {
            source,
            packs_dir,
            lock,
        } => pack_remove(ui, source, packs_dir.as_deref(), lock.as_deref()),
        PackVerb::List { lock } => pack_list(ui, lock.as_deref()),
    }
}

fn pack_check(ui: &Ui, dir: &Path, over: &[PathBuf]) -> Exit {
    let report = pack::check(dir);
    let name = report
        .manifest
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| pack_name_of(dir));

    for defect in &report.defects {
        ui.status(
            Stream::Err,
            Tone::Bad,
            &format!("{name}:"),
            &defect.to_string(),
            None,
        );
    }
    if !report.is_valid() {
        return Exit::Refused;
    }

    if let Some(manifest) = &report.manifest {
        ui.status(
            Stream::Out,
            Tone::Good,
            "pack",
            &format!("{} {}", manifest.name, manifest.version),
            Some(&format!("schema {}", manifest.schema)),
        );
        // The pin as parsed, so a reader sees the version the doctor entry
        // measures against without opening the manifest.
        if let Some(runtime) = &manifest.runtime {
            ui.status(
                Stream::Out,
                Tone::Flat,
                "runtime",
                &format!("{} {}", runtime.name, runtime.version),
                None,
            );
        }
    }
    for slot in &report.slots {
        let plural = if slot.entries == 1 {
            "entry"
        } else {
            "entries"
        };
        ui.status(
            Stream::Out,
            Tone::Flat,
            "slot",
            &format!("{}: {} {plural}", slot.name, slot.entries),
            None,
        );
    }

    if over.is_empty() {
        return Exit::Done;
    }

    let mut layers = vec![resolve::Layer::new(name, dir.to_path_buf())];
    for lower in over {
        layers.push(resolve::Layer::new(pack_name_of(lower), lower.clone()));
    }
    match resolve::resolve(&layers) {
        Ok(resolution) => {
            ui.status(
                Stream::Out,
                Tone::Good,
                "resolved",
                &format!(
                    "{} paths and {} agents across {} layers, {} shadowed",
                    resolution.files.len(),
                    resolution.agents.len(),
                    layers.len(),
                    resolution.shadowed.len()
                ),
                None,
            );
            Exit::Done
        }
        Err(refusals) => {
            for refusal in refusals {
                // The refusal is its own whole sentence, so it is the verb and
                // there is no subject to set beside it.
                ui.status(Stream::Err, Tone::Bad, &refusal.to_string(), "", None);
            }
            Exit::Refused
        }
    }
}

// The same three codes as `pack check`: 0 installed and pinned, 1 a refusal —
// the source, the version, the pack's own format, the layering it would enter,
// or a pin that did not land — and 2 a caller who did not say what to add.
fn pack_add(
    ui: &Ui,
    source: &str,
    version: &str,
    packs_dir: Option<&Path>,
    lock_path: Option<&Path>,
) -> Exit {
    // The machine directory is the platform layer's answer and core never asks
    // for it: it is resolved here, and the two paths core derives from it are
    // handed down.
    let machine = platform::machine_dir();
    let (packs_dir, defaults_dir) = defaults::pack_dirs(&machine, packs_dir);
    let lock_path = lock_path.map_or_else(|| machine.join(lock::LOCK), Path::to_path_buf);

    // The clone and the checkout are the unbounded wait in this verb, and the
    // only one it has: the pin that follows is a file write.
    let wait = ui.spinner(&format!("fetching {source} at {version}"));
    let added = add::add(
        &packs_dir,
        &defaults_dir,
        &lock_path,
        source,
        version,
        &clock::now_stamp(),
    );
    wait.done();

    match added {
        Ok(installed) => {
            ui.status(
                Stream::Out,
                Tone::Good,
                "added",
                &format!(
                    "{} {} at {}",
                    installed.name, installed.entry.version, installed.entry.commit
                ),
                Some(&installed.root.display().to_string()),
            );
            for import in &installed.imports {
                ui.status(
                    Stream::Out,
                    Tone::Good,
                    "added",
                    &format!(
                        "{} {} at {}, which {} imports",
                        import.name, import.entry.version, import.entry.commit, installed.name
                    ),
                    Some(&import.root.display().to_string()),
                );
            }
            ui.status(
                Stream::Out,
                Tone::Good,
                "pinned",
                &format!("in {}", lock_path.display()),
                None,
            );
            // Installed without them, and said where the next read of this
            // fleet — a run, a session's prime — would otherwise be the first
            // to find out.
            for missing in &installed.missing {
                ui.status(
                    Stream::Err,
                    Tone::Flat,
                    "fleet pack add:",
                    &missing.to_string(),
                    None,
                );
            }
            Exit::Done
        }
        Err(refusals) => {
            for refusal in refusals {
                ui.status(
                    Stream::Err,
                    Tone::Bad,
                    "fleet pack add:",
                    &refusal.to_string(),
                    None,
                );
            }
            Exit::Refused
        }
    }
}

// 0 removed and the drop read back, 1 a refusal — a source the lock does not
// hold, a line with no name, the binary's own defaults, or a pack another one
// imports — and 2 a caller who did not say what to remove.
fn pack_remove(ui: &Ui, source: &str, packs_dir: Option<&Path>, lock_path: Option<&Path>) -> Exit {
    let machine = platform::machine_dir();
    let packs_dir = defaults::pack_dirs(&machine, packs_dir).0;
    let lock_path = lock_path.map_or_else(|| machine.join(lock::LOCK), Path::to_path_buf);

    match remove::remove(&packs_dir, &lock_path, source) {
        Ok(removed) => {
            // A notice is a fact the verb is reporting beside a success, not a
            // refusal: it is flat, and it goes where nothing parses it.
            for notice in &removed.notices {
                ui.status(
                    Stream::Err,
                    Tone::Flat,
                    "fleet pack remove:",
                    &notice.to_string(),
                    None,
                );
            }
            ui.status(
                Stream::Out,
                Tone::Good,
                "removed",
                &format!("{} {}", removed.name, removed.entry.version),
                Some(&removed.root.display().to_string()),
            );
            // The source and the version, because the rollback is a re-add of
            // exactly this line.
            ui.status(
                Stream::Out,
                Tone::Good,
                "dropped",
                &format!(
                    "{} {} from {}",
                    removed.entry.source,
                    removed.entry.version,
                    lock_path.display()
                ),
                None,
            );
            Exit::Done
        }
        Err(refusals) => {
            for refusal in refusals {
                ui.status(
                    Stream::Err,
                    Tone::Bad,
                    "fleet pack remove:",
                    &refusal.to_string(),
                    None,
                );
            }
            Exit::Refused
        }
    }
}

// 0 and 3 only: there is nothing here to refuse. A lock that does not parse is
// an instrument the answer needs and could not read, which is could-not-tell and
// not an empty list.
fn pack_list(ui: &Ui, lock_path: Option<&Path>) -> Exit {
    let lock_path = lock_path.map_or_else(
        || platform::machine_dir().join(lock::LOCK),
        Path::to_path_buf,
    );

    match lock::read(&lock_path) {
        Ok(entries) => {
            // The table is the standard library's and reaches stdout unstyled:
            // a listing verb prints columns, and the ui module holds no table
            // surface for it to reach.
            print!("{}", lock_table(&entries));
            Exit::Done
        }
        Err(e) => {
            ui.status(
                Stream::Err,
                Tone::Bad,
                "fleet pack list:",
                &e.to_string(),
                None,
            );
            Exit::CouldNotTell
        }
    }
}

/// The lock as a table: a header, then one row per entry in the order the lock
/// holds them, which is source order. Every column but the last is padded to its
/// widest cell, the header's own width counted, and a line with no name prints
/// `-` in that column.
fn lock_table(entries: &[lock::Entry]) -> String {
    const HEADER: [&str; 5] = ["name", "source", "version", "commit", "fetched"];
    let rows: Vec<[&str; 5]> = std::iter::once(HEADER)
        .chain(entries.iter().map(|e| {
            [
                e.name.as_deref().unwrap_or("-"),
                e.source.as_str(),
                e.version.as_str(),
                e.commit.as_str(),
                e.fetched.as_str(),
            ]
        }))
        .collect();

    let widths: Vec<usize> = (0..HEADER.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut out = String::new();
    for row in &rows {
        for column in 0..HEADER.len() {
            let cell = row[column];
            out.push_str(cell);
            if column + 1 < HEADER.len() {
                out.push_str(&" ".repeat(widths[column] - cell.chars().count() + 2));
            }
        }
        out.push('\n');
    }
    out
}

// A layer is named by its manifest where it has a readable one, and by its
// directory otherwise: a refusal that names neither is one a reader cannot act
// on.
fn pack_name_of(dir: &std::path::Path) -> String {
    pack::read_manifest(dir)
        .ok()
        .map(|manifest| manifest.name)
        .unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.display().to_string())
        })
}
