//! `fleet doctor` — every doctor check the pack layers carry, run on demand.
//!
//! THE CHECKS ARE CORE'S AND THE ROWS ARE THIS VERB'S. Which `doctor/<name>/`
//! entries the layers resolve, how one is run and what its exit means are all
//! `fleet_core::item::doctor`'s; this module resolves the project, chooses
//! which entries run, hands each one its environment and prints what it said.
//!
//! runtime-version IS RUN ONCE PER PINNED PACK, on the `PATH` `fleet run` gives
//! that pack's own doctor, bundle and run lines — through the same function,
//! so a row here and a run's refusal cannot measure two different binaries.
//! The defaults' copy read with no pack named reads the defaults as a pack,
//! finds no manifest and says broken; run per pin, it measures what a run
//! would.
//!
//! THE VERB WRITES NOTHING. What a check's own script does is that check's
//! business, and one runs at a time: there is no lock to hold and no store to
//! open, and nothing here starts a session.
//!
//! THE REPORT IS THE OUTCOME. Under `--json` the document is a success
//! whatever the checks said, carrying every row and the aggregate; the exit is
//! the aggregate either way. `ok` is false only for the verb's own refusals,
//! said before any check runs.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

use fleet_controller::platform;
use fleet_core::item::brief::Packs;
use fleet_core::item::doctor::{self, Checked, Entry, Invocation, Verdict, RUNTIME_VERSION};
use fleet_core::item::run::{child_path_for, ENV_BIN, ENV_PROJECT};
use fleet_core::item::Stop;

use crate::envelope;
use crate::exit::Exit;
use crate::item::{self, Here, Human};
use crate::ui::{Tone, Ui};

const VERB: &str = "doctor";

/// What `doctor` takes: the checks to run, or none for every one.
#[derive(clap::Args)]
pub struct DoctorArgs {
    /// the checks to run, by name; every check when none is named
    #[arg(value_name = "CHECK")]
    pub checks: Vec<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

pub fn command(ui: &Ui, args: &DoctorArgs) -> Exit {
    let ready = match prepare(args) {
        Ok(ready) => ready,
        Err(stop) => {
            return item::refused(VERB, item::stop_exit(stop.code), &stop.message, args.json)
        }
    };
    let Ready {
        here,
        packs,
        chosen,
        fleet_bin,
    } = ready;

    // THE CALLER'S PATH for an ordinary check, which is what the person typing
    // `fleet doctor` has: a check finds `fleet` and its own tools where they
    // would. runtime-version alone is handed a constructed one, below.
    let caller_path = std::env::var_os("PATH")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let run_base = platform::child_path(&platform::home_dir());
    let root = here.project.root.as_path();
    let env: [(&str, &OsStr); 2] = [
        (ENV_PROJECT, root.as_os_str()),
        (ENV_BIN, fleet_bin.as_os_str()),
    ];

    let mut report = Report {
        ui,
        human: Human::under(args.json),
        rows: Vec::new(),
    };
    for entry in &chosen {
        if entry.name != RUNTIME_VERSION {
            let checked = doctor::run(entry, &how(&entry.root, &caller_path, root, &env));
            report.said(Row::of(entry, None, checked));
            continue;
        }
        let pins = doctor::pinned(&packs);
        if pins.is_empty() {
            report.said(Row::of(
                entry,
                None,
                Checked {
                    verdict: Verdict::Pass,
                    exit: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    line: String::from(
                        "nothing pinned: no installed pack declares a [runtime] table",
                    ),
                },
            ));
            continue;
        }
        for (layer, pin) in pins {
            let checked = match pin {
                Ok(runtime) => {
                    let path = child_path_for(&runtime.name, &run_base);
                    doctor::run(entry, &how(&layer.root, &path, root, &env))
                }
                Err(why) => Checked {
                    verdict: Verdict::CouldNotTell,
                    exit: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    line: why,
                },
            };
            report.said(Row::of(entry, Some(layer.name), checked));
        }
    }
    report.close(args.json)
}

/// How every check runs: from the project root, with the project and this
/// binary named, bounded. Only the pack it measures and its `PATH` differ.
fn how<'a>(
    pack_dir: &'a Path,
    path: &'a str,
    root: &'a Path,
    env: &'a [(&'a str, &'a OsStr)],
) -> Invocation<'a> {
    Invocation {
        pack_dir,
        path,
        cwd: Some(root),
        env,
        timeout: doctor::TIMEOUT,
    }
}

/// Everything resolved before a check runs.
struct Ready {
    here: Here,
    packs: Packs,
    /// The entries to run, in name order.
    chosen: Vec<Entry>,
    fleet_bin: PathBuf,
}

/// The project, the layers and the names, each a refusal of the verb's own
/// said before any check runs: a name no layer carries is a typo, and running
/// the rest would answer a question nobody asked.
fn prepare(args: &DoctorArgs) -> Result<Ready, Stop> {
    let here = item::resolve_at(args.packs_dir.clone())?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    let all = doctor::entries(&packs);

    let mut named: Vec<&str> = Vec::new();
    for name in &args.checks {
        if !named.contains(&name.as_str()) {
            named.push(name);
        }
    }
    if let Some(unknown) = named
        .iter()
        .find(|name| !all.iter().any(|entry| entry.name == **name))
    {
        let carried: Vec<&str> = all.iter().map(|entry| entry.name.as_str()).collect();
        return Err(Stop::usage(format!(
            "no doctor check named {unknown} — the layers carry: {}",
            carried.join(", ")
        )));
    }
    let chosen = all
        .into_iter()
        .filter(|entry| named.is_empty() || named.contains(&entry.name.as_str()))
        .collect();

    // THE FLEET BINARY IS THIS PROCESS, as it is for a run: a check calling
    // back into fleet reaches the one the person ran.
    let fleet_bin = std::env::current_exe().map_err(|e| {
        Stop::could_not_tell(format!("this process cannot name its own binary: {e}"))
    })?;
    Ok(Ready {
        here,
        packs,
        chosen,
        fleet_bin,
    })
}

/// One row: the check that ran, which pack it measured where it measured one,
/// and what it said.
struct Row {
    name: String,
    layer: String,
    description: Option<String>,
    measures: Option<String>,
    checked: Checked,
}

impl Row {
    fn of(entry: &Entry, measures: Option<String>, checked: Checked) -> Row {
        Row {
            name: entry.name.clone(),
            layer: entry.layer.clone(),
            description: entry.description.clone(),
            measures,
            checked,
        }
    }

    fn subject(&self) -> String {
        match &self.measures {
            Some(measures) => format!("{} for {measures} ({})", self.name, self.layer),
            None => format!("{} ({})", self.name, self.layer),
        }
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "description": self.description,
            "exit": self.checked.exit,
            "layer": self.layer,
            "line": self.checked.line,
            "measures": self.measures,
            "name": self.name,
            "stderr": self.checked.stderr,
            "stdout": self.checked.stdout,
            "verdict": self.checked.verdict.code(),
        })
    }
}

/// The rows as they come, and the summary and the document after them.
struct Report<'a> {
    ui: &'a Ui,
    human: Human,
    rows: Vec<Row>,
}

impl Report<'_> {
    /// One row, printed as soon as its check returns. A row that did not pass
    /// is followed by all the check printed, because those are the lines a
    /// person acts on and the row's own line is only the last of them.
    fn said(&mut self, row: Row) {
        let verdict = row.checked.verdict;
        let line = self.ui.line(
            tone(verdict),
            verdict.word(),
            &row.subject(),
            Some(&row.checked.line),
        );
        let _ = writeln!(self.human, "{line}");
        if verdict != Verdict::Pass {
            for said in row.checked.stdout.lines().chain(row.checked.stderr.lines()) {
                if !said.trim().is_empty() {
                    let _ = writeln!(self.human, "  {said}");
                }
            }
        }
        let _ = self.human.flush();
        self.rows.push(row);
    }

    /// The summary, the document where one was asked for, and the aggregate
    /// as the exit.
    fn close(mut self, json: bool) -> Exit {
        let count = |verdict| {
            self.rows
                .iter()
                .filter(|row| row.checked.verdict == verdict)
                .count()
        };
        let (pass, finding, unknown) = (
            count(Verdict::Pass),
            count(Verdict::Finding),
            count(Verdict::CouldNotTell),
        );
        let verdict = Verdict::aggregate(self.rows.iter().map(|row| row.checked.verdict));
        let checks = match self.rows.len() {
            1 => String::from("1 check"),
            n => format!("{n} checks"),
        };
        let summary = self.ui.line(
            tone(verdict),
            VERB,
            &checks,
            Some(&format!(
                "{pass} pass, {finding} finding, {unknown} could not tell"
            )),
        );
        let _ = writeln!(self.human, "{summary}");
        let _ = self.human.flush();

        if json {
            let data = serde_json::json!({
                "checks": self.rows.iter().map(Row::json).collect::<Vec<_>>(),
                "counts": {
                    "could_not_tell": unknown,
                    "finding": finding,
                    "pass": pass,
                },
                "verdict": verdict.code(),
            });
            println!("{}", envelope::ok(VERB, &data));
        }
        item::stop_exit(verdict.exit())
    }
}

fn tone(verdict: Verdict) -> Tone {
    match verdict {
        Verdict::Pass => Tone::Good,
        Verdict::Finding | Verdict::CouldNotTell => Tone::Bad,
    }
}
