//! The item verbs' family module: the verbs' arguments and what only a process
//! knows.
//!
//! The arguments are clap's, one struct per verb, and each verb answers in the
//! exit table's own words.

use std::io::Write;
use std::path::PathBuf;

use fleet_controller::project::{open_store, resolve_at, Here};
use fleet_controller::{clock, platform};
use fleet_core::item::brief::{self, Packs, TRANSIENT};
use fleet_core::item::dispatch::{self, Order, Wiring};
use fleet_core::item::hold;
use fleet_core::item::land::{self, Landed, Landing};
use fleet_core::item::run as workflow_run;
use fleet_core::item::{deliver, review, Stop};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::{identity_or_mint, roster, IDENTITY};

use crate::envelope;
use crate::exit::{refuse_stop, Exit};
use crate::seams::{Bar, BoxLoad, RealGit, SeatRing};
use crate::ui::Ui;

/// Where a rendered brief is written, under the machine directory.
const BRIEFS: &str = "briefs";

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
        let events = here.events();
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
        Err(stop) => refuse_stop("cancel", &stop, false),
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
            Exit::of(stop.code)
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
    let events = here.events();
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
    enveloped(
        "land",
        args.json,
        |human| run_land(ui, args, human, &mut std::io::stderr()),
        |landed| {
            serde_json::json!({
                "item": landed.item,
                "state": "landed",
                "sha": landed.sha,
                "entry": landed.entry,
            })
        },
    )
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
    let git = RealGit::of(&here);
    // The bar is bounded by the rows the landing reads, because that count is
    // known before the first check is read and does not change with what they
    // say.
    let progress = Bar::over(ui, land::CRITERIA.len() as u64, "landing");
    let events = here.events();
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
    enveloped(
        "deliver",
        args.json,
        |human| run_deliver(args, human, &mut std::io::stderr()),
        |made| {
            serde_json::json!({
                "item": made.item,
                "state": "delivered",
                "commit": made.commit,
                "entry": made.entry,
            })
        },
    )
}

pub fn hold_command(args: &HoldArgs) -> Exit {
    enveloped(
        "hold",
        args.json,
        |human| run_hold(args, human),
        |held| {
            serde_json::json!({
                "item": held.item,
                "state": "held",
                "hold": held.hold,
                "entry": held.entry,
            })
        },
    )
}

pub fn clear_command(args: &ClearArgs) -> Exit {
    enveloped(
        "clear",
        args.json,
        |human| run_clear(args, human),
        |cleared| {
            serde_json::json!({
                "item": cleared.item,
                "state": "cleared",
                "hold": cleared.hold,
                "entry": cleared.entry,
            })
        },
    )
}

fn run_hold(parsed: &HoldArgs, out: &mut dyn Write) -> Result<hold::Held, Stop> {
    let here = resolve_at(parsed.packs_dir.clone())?;
    let by = acting("hold", parsed.by.as_deref(), &here)?;
    let store = open_store(&here)?;
    let git = RealGit::of(&here);
    let events = here.events();

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
    let git = RealGit::of(&here);
    let events = here.events();

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
    enveloped(
        "review",
        args.json,
        |human| run_review(args, human, &mut std::io::stderr()),
        // Either verdict is the one entry kind, `reviewed`, and which verdict
        // it was rides beside it. `--show` writes no verdict and moves the
        // item nowhere, so its state, its verdict and its entry are null: the
        // absent value and not a fourth word for "it did not move".
        |read| {
            let verdict = match (&args.returned, args.land) {
                (Some(_), _) => Some("returned"),
                (None, true) => Some("accepted"),
                (None, false) => None,
            };
            serde_json::json!({
                "item": read.item,
                "state": verdict.map(|_| "reviewed"),
                "verdict": verdict,
                "entry": read.entry,
            })
        },
    )
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
    let git = RealGit::of(&here);
    let ring = SeatRing::of(&here);
    let events = here.events();

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
    let git = RealGit::of(&here);
    let ring = SeatRing::of(&here);
    let events = here.events();

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
    enveloped(
        "dispatch",
        args.json,
        |human| run_dispatch(args, human, &mut std::io::stderr()),
        |given| {
            serde_json::json!({
                "item": given.item,
                "state": "ordered",
                "seat": given.seat,
                "entry": given.entry,
            })
        },
    )
}

pub fn brief_command(args: &BriefArgs) -> Exit {
    let mut err = std::io::stderr();
    match run_brief(args, &mut std::io::stdout(), &mut err) {
        Ok(()) => Exit::Done,
        Err(stop) => {
            let _ = writeln!(err, "fleet brief: {}", stop.message);
            Exit::of(stop.code)
        }
    }
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

/// One enveloped verb: its human rendering where `Human` puts it, then the
/// outcome document or the refusal, each under the verb's own name.
fn enveloped<T>(
    verb: &str,
    json: bool,
    run: impl FnOnce(&mut Human) -> Result<T, Stop>,
    to_json: impl FnOnce(T) -> serde_json::Value,
) -> Exit {
    let mut human = Human::under(json);
    match run(&mut human) {
        Ok(done) => answered(verb, to_json(done), json),
        Err(stop) => refuse_stop(verb, &stop, json),
    }
}

/// The outcome document, printed only where the caller asked for one.
fn answered(verb: &str, data: serde_json::Value, json: bool) -> Exit {
    if json {
        println!("{}", envelope::ok(verb, &data));
    }
    Exit::Done
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
    let ring = SeatRing::of(&here);
    // The real thing: a dispatch with no `--to` runs the controller's spawn,
    // belt and all, and a refusal from it withdraws the order in the same act.
    let spawner = crate::transient::TransientSpawner {
        here: &here,
        home: platform::home_dir(),
    };
    let events = here.events();

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
