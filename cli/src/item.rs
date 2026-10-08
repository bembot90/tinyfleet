//! The item verbs' family module: the verbs' arguments, what only a process
//! knows, and the seams core defines and this crate implements.
//!
//! The arguments are clap's, one struct per verb, and each verb answers in the
//! exit table's own words.
//!
//! core never depends on the controller — the workspace test refuses that edge
//! — so the ring, which reaches the provider adapter, and the spawner, which
//! will reach the controller's transient-seat primitives, meet here. git meets
//! here too: core states the operations, and this crate answers them over
//! `fleet_core::process::git`. Every git the shipped code runs by bare name
//! is built by `fleet_core::process::git_command`; the one git it runs
//! otherwise is the agent conformance's live `git init`, a resolved path on
//! the constructed `PATH`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fleet_controller::effect::{TurnTarget, Typed};
use fleet_controller::project::stream::StreamEvents;
use fleet_controller::project::{open_store, resolve_at, Here};
use fleet_controller::{clock, config, effect, platform, policy as controller, sessions};
use fleet_core::agent;
use fleet_core::item::brief::{self, Packs, TRANSIENT};
use fleet_core::item::dispatch::{self, Order, Wiring};
use fleet_core::item::hold;
use fleet_core::item::land::{self, LandGit, Landed, Landing, Progress, Pushed, Squashed};
use fleet_core::item::run as workflow_run;
use fleet_core::item::{
    deliver, numstat_line, review, Change, Git, Ring, RingOutcome, Stop, TRUNK,
};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::{identity_or_mint, roster, IDENTITY};

use crate::envelope;
use crate::exit::Exit;
use crate::ui::{Ui, Wait};

/// Where a rendered brief is written, under the machine directory.
const BRIEFS: &str = "briefs";
/// The fleet's event stream, under the machine directory.
pub(crate) const EVENTS: &str = "events.jsonl";

/// What `dispatch` takes. An order names who gave it, so `--by` is here and
/// not on `brief`.
#[derive(clap::Args)]
pub struct DispatchArgs {
    /// the item to act on
    pub item: String,
    /// the seat it goes to; without it, a transient one
    #[arg(long, value_name = "SEAT")]
    pub to: Option<String>,
    /// who dispatches; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// the builder's checks, named in its brief
    #[arg(long, value_name = "COMMAND")]
    pub touched: Option<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs the brief renders from are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `brief` takes. It writes nothing, so it names no one who wrote it.
#[derive(clap::Args)]
pub struct BriefArgs {
    /// the item to act on
    pub item: String,
    /// the seat it goes to; without it, a transient one
    #[arg(long, value_name = "SEAT")]
    pub to: Option<String>,
    /// the builder's checks, as its dispatch had them
    #[arg(long, value_name = "COMMAND")]
    pub touched: Option<String>,
    /// where the packs the brief renders from are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `deliver` takes. No item: the one the seat holds is the one it is
/// delivering, and `--item` is for the case a seat legitimately holds two.
#[derive(clap::Args)]
pub struct DeliverArgs {
    // The help is one sentence broken in two, because the page is measured
    // under eighty columns and the flag column eats a third of it.
    #[arg(
        long,
        value_name = "FILE",
        help = "the delivery, a JSON file of the shape\nassets/delivery.schema.json"
    )]
    pub delivery: PathBuf,
    /// the item, where the seat holds more than one
    #[arg(long, value_name = "ID")]
    pub item: Option<String>,
    /// who is delivering; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `hold` takes. No item, for the reason `deliver` takes none: the one the
/// seat holds is the one it is asking about.
#[derive(clap::Args)]
pub struct HoldArgs {
    // Broken in two for the reason `--delivery`'s help is.
    #[arg(
        long,
        value_name = "FILE",
        help = "the question, a JSON file of the shape\nassets/question.schema.json"
    )]
    pub question: PathBuf,
    /// the item, where the seat holds more than one
    #[arg(long, value_name = "ID")]
    pub item: Option<String>,
    /// who is asking; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `clear` takes: the item, the letter, and what was said beyond it.
#[derive(clap::Args)]
pub struct ClearArgs {
    /// the item whose hold is being cleared
    pub item: String,
    /// the option's letter
    pub letter: String,
    /// what was decided, where the options did not carry it
    #[arg(long, value_name = "TEXT")]
    pub text: Option<String>,
    /// who is answering; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `review` takes: the item, and one of the three modes.
#[derive(clap::Args)]
pub struct ReviewArgs {
    /// the item to review
    pub item: String,
    /// print the delivery and the size line; write nothing
    #[arg(long, conflicts_with_all = ["land", "returned"])]
    pub show: bool,
    /// append the accepted verdict; it lands nothing
    #[arg(long, conflicts_with = "returned")]
    pub land: bool,
    /// append the returned verdict with these findings
    ///
    /// the findings, a JSON file of the shape assets/findings.schema.json
    #[arg(long = "return", value_name = "FILE")]
    pub returned: Option<PathBuf>,
    /// who is reviewing; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `land` takes: the item, the commit the reviewer read, and the three
/// flags that say what else joins the landing.
#[derive(clap::Args)]
pub struct LandArgs {
    /// the item to land
    pub item: String,
    /// the commit reviewed; 7 to 40 hex, never a branch name
    pub commit: String,
    /// a path of the reviewer's own, let into the staged set
    #[arg(long, value_name = "PATH")]
    pub also: Vec<String>,
    /// what the close says beyond the landed sha
    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,
    /// the command run on the rebased tree before the push
    #[arg(long, value_name = "COMMAND")]
    pub test: Option<String>,
    /// who is landing; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// print the outcome as one JSON document
    #[arg(long)]
    pub json: bool,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `run` takes: a workflow name, and the inputs the run is pinned
/// against.
#[derive(clap::Args)]
pub struct RunArgs {
    /// the workflow, resolved as workflows/<name>.* through the layers
    pub workflow: String,
    /// one pinned input, key=value; repeatable
    #[arg(long = "input", value_name = "KEY=VALUE")]
    pub inputs: Vec<String>,
    /// who runs it; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// What `cancel` takes: the run, and who is ending it.
#[derive(clap::Args)]
pub struct CancelArgs {
    /// the run's id, which is its record's
    pub run: String,
    /// who cancels it; else FLEET_ACTOR, else this machine
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
    /// where the packs are installed
    #[arg(long = "packs-dir", value_name = "DIR")]
    pub packs_dir: Option<PathBuf>,
}

/// The cancel verb: the project resolved, then core's cancel over its store and
/// the machine's stream.
pub fn cancel_command(args: &CancelArgs) -> Exit {
    let cancelled = resolve_at(args.packs_dir.clone()).and_then(|here| {
        let by = acting("cancel", args.by.as_deref(), &here)?;
        let store = open_store(&here)?;
        let events = StreamEvents::at(here.machine_dir.join(EVENTS));
        workflow_run::cancel(
            &mut std::io::stdout(),
            &workflow_run::Cancel {
                run: &args.run,
                by: &by,
            },
            store.as_ref(),
            &events,
        )
    });
    match cancelled {
        Ok(_) => Exit::Done,
        Err(stop) => {
            eprintln!("fleet cancel: {}", stop.message);
            stop_exit(stop.code)
        }
    }
}

/// The run verb: the pairs parsed, then core, then the outcome read into this
/// cli's own exit table.
///
/// THE FLEET BINARY IS THIS PROCESS. `{fleet}` in a pack's template is the
/// binary a workflow calls back into, and the one a run started by this process
/// should call back into is this one — never a second `fleet` that happens to
/// be earlier on the caller's PATH.
///
/// THE WORKFLOW'S ROW BECOMES THIS VERB'S EXIT. A caller that scripts `fleet
/// run` has `$?` and nothing else, so a workflow that failed has to be tellable
/// from one that finished without reading the stream; a wait is `Done` because
/// a run that asked to be woken has not gone wrong.
pub fn run_command(args: &RunArgs) -> Exit {
    let mut err = std::io::stderr();
    match run_the_workflow(args, &mut std::io::stdout()) {
        Ok(ended) => match ended {
            workflow_run::Ended::Closed | workflow_run::Ended::Waiting => Exit::Done,
            workflow_run::Ended::Failed => Exit::Refused,
            workflow_run::Ended::CouldNotTell => Exit::CouldNotTell,
        },
        Err(stop) => {
            let _ = writeln!(err, "fleet run: {}", stop.message);
            stop_exit(stop.code)
        }
    }
}

fn run_the_workflow(parsed: &RunArgs, out: &mut dyn Write) -> Result<workflow_run::Ended, Stop> {
    let mut inputs: Vec<(String, String)> = Vec::with_capacity(parsed.inputs.len());
    for given in &parsed.inputs {
        let Some((key, value)) = given.split_once('=') else {
            return Err(Stop::usage(format!(
                "`--input {given}` is not a pair — an input is written key=value"
            )));
        };
        if key.is_empty() {
            return Err(Stop::usage(format!(
                "`--input {given}` names no key — an input is written key=value"
            )));
        }
        inputs.push((key.to_string(), value.to_string()));
    }

    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("run", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));
    let fleet_bin = std::env::current_exe().map_err(|e| {
        Stop::could_not_tell(format!("this process cannot name its own binary: {e}"))
    })?;
    let stamp = clock::now_stamp();
    let child_path = platform::child_path(&platform::home_dir());
    workflow_run::run(
        out,
        &workflow_run::Order {
            workflow: &parsed.workflow,
            inputs: &inputs,
            by: &by,
            at: &stamp,
            machine_dir: &here.machine_dir,
            fleet_bin: &fleet_bin,
        },
        &workflow_run::Wiring {
            store: store.as_ref(),
            project: &here.project,
            packs: &packs,
            policy_file: &here.policy_file,
            events: &events,
            stream: &events,
            child_path: &child_path,
        },
    )
    .map(|ran| ran.ended)
}

pub fn land_command(ui: &Ui, args: &LandArgs) -> Exit {
    let mut err = std::io::stderr();
    let mut human = Human::under(args.json);
    match run_land(ui, args, &mut human, &mut err) {
        Ok(landed) => answered(
            "land",
            serde_json::json!({
                "item": landed.item,
                "state": "landed",
                "sha": landed.sha,
                "entry": landed.entry,
            }),
            args.json,
        ),
        Err(stop) => refused("land", stop_exit(stop.code), &stop.message, args.json),
    }
}

fn run_land(
    ui: &Ui,
    parsed: &LandArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Landed, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("land", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let git = RealGit {
        root: here.project.root.clone(),
    };
    // The bar is bounded by the rows the landing reads, because that count is
    // known before the first check is read and does not change with what they
    // say.
    let progress = Bar::over(ui, land::CRITERIA.len() as u64, "landing");
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));
    let load = BoxLoad::of(&here);
    let stamp = clock::now_stamp();
    // The one constructed search path, the same function the controller's own
    // children are given: the lane's suite never runs under this process's.
    let child_path = platform::child_path(&platform::home_dir());

    land::land(
        out,
        err,
        &Landing {
            item: &parsed.item,
            commit: &parsed.commit,
            also: &parsed.also,
            test: parsed.test.as_deref(),
            reason: parsed.reason.as_deref(),
            by: &by,
            at: &stamp,
            machine_dir: &here.machine_dir,
        },
        &land::Wiring {
            store: store.as_ref(),
            git: &git,
            project: &here.project,
            progress: &progress,
            events: &events,
            load: &load,
            child_path: &child_path,
            seats: &here.seats,
        },
    )
}

pub fn deliver_command(args: &DeliverArgs) -> Exit {
    let mut err = std::io::stderr();
    let mut human = Human::under(args.json);
    match run_deliver(args, &mut human, &mut err) {
        Ok(made) => answered(
            "deliver",
            serde_json::json!({
                "item": made.item,
                "state": "delivered",
                "commit": made.commit,
                "entry": made.entry,
            }),
            args.json,
        ),
        Err(stop) => refused("deliver", stop_exit(stop.code), &stop.message, args.json),
    }
}

pub fn hold_command(args: &HoldArgs) -> Exit {
    let mut human = Human::under(args.json);
    match run_hold(args, &mut human) {
        Ok(held) => answered(
            "hold",
            serde_json::json!({
                "item": held.item,
                "state": "held",
                "hold": held.hold,
                "entry": held.entry,
            }),
            args.json,
        ),
        Err(stop) => refused("hold", stop_exit(stop.code), &stop.message, args.json),
    }
}

pub fn clear_command(args: &ClearArgs) -> Exit {
    let mut human = Human::under(args.json);
    match run_clear(args, &mut human) {
        Ok(cleared) => answered(
            "clear",
            serde_json::json!({
                "item": cleared.item,
                "state": "cleared",
                "hold": cleared.hold,
                "entry": cleared.entry,
            }),
            args.json,
        ),
        Err(stop) => refused("clear", stop_exit(stop.code), &stop.message, args.json),
    }
}

fn run_hold(parsed: &HoldArgs, out: &mut dyn Write) -> Result<hold::Held, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("hold", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let git = RealGit {
        root: here.project.root.clone(),
    };
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));

    let stamp = clock::now_stamp();
    hold::hold(
        out,
        &hold::Question {
            item: parsed.item.as_deref(),
            by: &by,
            question: &parsed.question,
            at: &stamp,
        },
        &hold::Wiring {
            store: store.as_ref(),
            git: &git,
            project: &here.project,
            events: &events,
        },
    )
}

fn run_clear(parsed: &ClearArgs, out: &mut dyn Write) -> Result<hold::Cleared, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("clear", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let git = RealGit {
        root: here.project.root.clone(),
    };
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));

    hold::clear(
        out,
        &hold::Clearance {
            item: &parsed.item,
            letter: &parsed.letter,
            text: parsed.text.as_deref(),
            by: &by,
        },
        &hold::Wiring {
            store: store.as_ref(),
            git: &git,
            project: &here.project,
            events: &events,
        },
    )
}

pub fn review_command(args: &ReviewArgs) -> Exit {
    let mut err = std::io::stderr();
    let mut human = Human::under(args.json);
    match run_review(args, &mut human, &mut err) {
        // Either verdict is the one entry kind, `reviewed`, and which verdict
        // it was rides beside it. `--show` writes no verdict and moves the
        // item nowhere, so its state, its verdict and its entry are null: the
        // absent value and not a fourth word for "it did not move".
        Ok(read) => {
            let verdict = match (&args.returned, args.land) {
                (Some(_), _) => Some("returned"),
                (None, true) => Some("accepted"),
                (None, false) => None,
            };
            answered(
                "review",
                serde_json::json!({
                    "item": read.item,
                    "state": verdict.map(|_| "reviewed"),
                    "verdict": verdict,
                    "entry": read.entry,
                }),
                args.json,
            )
        }
        Err(stop) => refused("review", stop_exit(stop.code), &stop.message, args.json),
    }
}

fn run_deliver(
    parsed: &DeliverArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<deliver::Delivered, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("deliver", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    let git = RealGit {
        root: here.project.root.clone(),
    };
    let ring = SeatRing {
        machine_dir: here.machine_dir.clone(),
    };
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));

    let stamp = clock::now_stamp();
    deliver::deliver(
        out,
        err,
        &deliver::Delivery {
            item: parsed.item.as_deref(),
            by: &by,
            delivery: &parsed.delivery,
            at: &stamp,
        },
        &deliver::Wiring {
            store: store.as_ref(),
            git: &git,
            packs: &packs,
            project: &here.project,
            ring: &ring,
            events: &events,
            seats: &here.seats,
        },
    )
}

fn run_review(
    parsed: &ReviewArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<review::Read, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("review", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    let git = RealGit {
        root: here.project.root.clone(),
    };
    let ring = SeatRing {
        machine_dir: here.machine_dir.clone(),
    };
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));

    // --show is the default, so the two writing modes are what a call opts
    // into and nothing here has to tell "asked for --show" from "asked for
    // nothing".
    let mode = match (&parsed.returned, parsed.land) {
        (Some(file), _) => review::Mode::Return(file),
        (None, true) => review::Mode::Land,
        (None, false) => review::Mode::Show,
    };

    review::review(
        out,
        err,
        &review::Verdict {
            item: &parsed.item,
            by: &by,
            mode,
        },
        &review::Wiring {
            store: store.as_ref(),
            git: &git,
            packs: &packs,
            project: &here.project,
            ring: &ring,
            events: &events,
            seats: &here.seats,
        },
    )
}

pub fn dispatch_command(args: &DispatchArgs) -> Exit {
    let mut err = std::io::stderr();
    let mut human = Human::under(args.json);
    match run_dispatch(args, &mut human, &mut err) {
        Ok(given) => answered(
            "dispatch",
            serde_json::json!({
                "item": given.item,
                "state": "ordered",
                "seat": given.seat,
                "entry": given.entry,
            }),
            args.json,
        ),
        Err(stop) => refused("dispatch", stop_exit(stop.code), &stop.message, args.json),
    }
}

pub fn brief_command(args: &BriefArgs) -> Exit {
    let mut err = std::io::stderr();
    match run_brief(args, &mut std::io::stdout(), &mut err) {
        Ok(()) => Exit::Done,
        Err(stop) => {
            let _ = writeln!(err, "fleet brief: {}", stop.message);
            stop_exit(stop.code)
        }
    }
}

/// core states a stop's exit as a number from the same table this enum holds.
/// A number outside it is core's own defect and reads as could-not-tell here
/// rather than as a status this binary invented.
pub(crate) fn stop_exit(code: u8) -> Exit {
    Exit::from_status(code).unwrap_or(Exit::CouldNotTell)
}

// ---- the JSON envelope, for the six verbs the SDK drives ---------------------

/// Where a verb's human rendering goes: stdout ordinarily, and stderr under
/// `--json`, because stdout then carries the one document and nothing else
/// (`envelope.rs`). The exit code is the exit table's own row either way.
pub(crate) enum Human {
    Out(std::io::Stdout),
    Aside(std::io::Stderr),
}

impl Human {
    pub(crate) fn under(json: bool) -> Human {
        if json {
            Human::Aside(std::io::stderr())
        } else {
            Human::Out(std::io::stdout())
        }
    }
}

impl Write for Human {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Human::Out(out) => out.write(buf),
            Human::Aside(aside) => aside.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Human::Out(out) => out.flush(),
            Human::Aside(aside) => aside.flush(),
        }
    }
}

/// The outcome document, printed only where the caller asked for one.
fn answered(verb: &str, data: serde_json::Value, json: bool) -> Exit {
    if json {
        println!("{}", envelope::ok(verb, &data));
    }
    Exit::Done
}

/// A refusal said once: the human's line on stderr always, the envelope on
/// stdout when the caller asked for the document, and the exit table's own row
/// either way. `stream.rs` says the same for the event verbs.
pub(crate) fn refused(verb: &str, exit: Exit, why: &str, json: bool) -> Exit {
    eprintln!("fleet {verb}: {why}");
    if json {
        println!("{}", envelope::refusal(verb, exit, why));
    }
    exit
}

fn run_dispatch(
    parsed: &DispatchArgs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<dispatch::Given, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("dispatch", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    let ring = SeatRing {
        machine_dir: here.machine_dir.clone(),
    };
    // The real thing: a dispatch with no `--to` runs the controller's spawn,
    // belt and all, and a refusal from it withdraws the order in the same act.
    let spawner = crate::transient::TransientSpawner {
        here: &here,
        home: platform::home_dir(),
    };
    let events = StreamEvents::at(here.machine_dir.join(EVENTS));

    let stamp = clock::now_stamp();
    dispatch::dispatch(
        out,
        err,
        &Order {
            item: &parsed.item,
            to: parsed.to.as_deref(),
            by: &by,
            at: &stamp,
            // A HAND-RUN DISPATCH PINS NOTHING: the brief is rendered from the
            // item as it stands now, which is what a person running this verb
            // means by it. A flight's own dispatch hands over the file the
            // takeoff pinned instead, and the base and the model with it.
            brief: None,
            base: None,
            model: None,
            touched: parsed.touched.as_deref(),
        },
        &Wiring {
            store: store.as_ref(),
            project: &here.project,
            packs: &packs,
            briefs_dir: &here.machine_dir.join(BRIEFS),
            seats: &here.seats,
            ring: &ring,
            spawner: &spawner,
            events: &events,
        },
    )
    .map_err(Stop::from)
}

fn run_brief(parsed: &BriefArgs, out: &mut dyn Write, err: &mut dyn Write) -> Result<(), Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let store = open_store(&here)?;
    let packs = Packs::under(&here.packs_dir, &here.defaults_dir)?;
    // `--to` names a seat this machine runs, as dispatch's does, and the brief
    // says who it is for by the seat's machine name; no `--to` is the
    // transient seat that does not exist yet.
    let seat = match parsed.to.as_deref() {
        Some(to) => here.seats.resolve_running(to)?.machine_name(),
        None => TRANSIENT.to_string(),
    };

    brief::for_item(
        out,
        err,
        &packs,
        &here.project,
        store.as_ref(),
        &parsed.item,
        &seat,
        parsed.touched.as_deref(),
    )
    .map(|_| ())
}

// ---- what only a process knows ----------------------------------------------

/// The variable a verb reads its actor from where `--by` did not carry one.
/// The controller sets it to `seat:<id>` on every session it starts.
pub(crate) const FLEET_ACTOR: &str = "FLEET_ACTOR";

/// `FLEET_ACTOR`, trimmed, where it holds anything at all.
pub(crate) fn fleet_actor() -> Option<String> {
    std::env::var(FLEET_ACTOR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Who acts, resolved once at this boundary: `--by`, else `FLEET_ACTOR`, else
/// this machine's identity.
///
/// A VERB ALWAYS HAS AN ACTOR. Typed text — `<kind>:<id>` — is taken as given,
/// and anything else is a seat argument, resolved over the fleet's listed
/// seats: the roster, the machine's transient rows and this machine's
/// identity. With neither source the verb acts as the machine's identity,
/// minted where it has none, and says so on one line when the roster does not
/// list it: a person the fleet does not know is still somebody, and the line
/// names the verb that makes them known.
pub(crate) fn acting(verb: &str, by: Option<&str>, here: &Here) -> Result<Actor, Stop> {
    let given = match by {
        Some(by) => Some(("--by", by.to_string())),
        None => fleet_actor().map(|value| (FLEET_ACTOR, value)),
    };
    if let Some((source, text)) = given {
        return match Actor::typed(&text) {
            Some(typed) => typed.map_err(Stop::refused),
            None => here
                .seats
                .resolve_listed(&text)
                .map(|seat| Actor::seat(seat.id))
                .map_err(|unresolved| {
                    let stop = Stop::from(unresolved);
                    Stop {
                        message: format!("{source} {}", stop.message),
                        ..stop
                    }
                }),
        };
    }

    let (identity, minted) = identity_or_mint(&here.machine_dir)
        .map_err(|why| Stop::could_not_tell(format!("could not tell who acts: {why}")))?;
    let listed = roster(&here.project.guards)
        .unwrap_or_default()
        .iter()
        .any(|seat| seat.seat.id == identity.id);
    if !listed {
        let minted = if minted {
            format!(
                "this machine had no identity, so one was minted at {}; ",
                here.machine_dir.join(IDENTITY).display()
            )
        } else {
            String::new()
        };
        eprintln!(
            "fleet {verb}: {minted}acting as this machine's identity {} ({}), which {} does not \
             list — fleet seat add --human lists it",
            identity.as_ref().machine_name(),
            identity.id,
            here.policy_file.display()
        );
    }
    Ok(Actor::seat(identity.id))
}

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
    /// The one path both callers take: the row lookup, the seat's recorded
    /// configuration directory, and the typed turn into its session.
    ///
    /// `timeout` stands in for the policy's bound on this call alone.
    pub(crate) fn ring_with(&self, seat: &str, text: &str, timeout: Option<Duration>) -> Rung {
        let machine = match config::read(&self.machine_dir.join("config.json")) {
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
            ceiling_per_cpu: crate::transient::policy_of(here)
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
        self.lines(
            "diff --cached --name-only",
            &["diff", "--cached", "--name-only"],
        )
    }

    fn status(&self) -> Result<Vec<String>, String> {
        self.lines("status --porcelain", &["status", "--porcelain"])
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
        let conflicted = self.lines(
            "diff --diff-filter=U",
            &["diff", "--name-only", "--diff-filter=U"],
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
        self.lines(
            "diff --name-only <base>...<commit>",
            &["diff", "--name-only", &format!("{base}...{commit}")],
        )
    }

    /// An empty restriction is an empty diff, and never the whole tree: a
    /// pathspec-less `git diff` answers about everything, which is the opposite
    /// of what "restricted to no path" means.
    fn diff_paths(&self, from: &str, to: &str, paths: &[String]) -> Result<Vec<String>, String> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec!["diff", "--name-only", from, to, "--"];
        args.extend(paths.iter().map(String::as_str));
        self.lines("diff --name-only <from> <to> -- <paths>", &args)
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
struct Bar {
    wait: std::cell::RefCell<Option<Wait>>,
}

impl Bar {
    fn over(ui: &Ui, rows: u64, message: &str) -> Bar {
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
