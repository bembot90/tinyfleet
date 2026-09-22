//! `fleet event step <started|closed>`: the one writer of the step pair
//! (core's `STEP_STARTED` and `STEP_CLOSED`), for a workflow's child process.
//!
//! A workflow runs in a process of its own and reaches the stream only through
//! this binary, so the pair the SDK replays from (fleet-layers § The replay
//! contract) needs a verb that appends it. The reader is the SDK itself, over
//! `fleet event tail --json`; nothing in the controller folds these two.
//!
//! The append is the seat events' own: the machine directory's stream, the
//! controller's `EventLog`, and a read-back of the last line before the exit
//! reads 0 — a workflow that takes a 0 as "recorded" and moves to its next
//! step must be reading the file and not this process's intent.

use crate::exit::Exit;
use crate::item;
use clap::ValueEnum;
use fleet_controller::events::{self, EventLog};
use fleet_controller::platform;
use fleet_core::item::{STEP_CLOSED, STEP_STARTED};

/// The two halves of a step, as the positional word.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Phase {
    Started,
    Closed,
}

impl Phase {
    fn kind(self) -> &'static str {
        match self {
            Phase::Started => STEP_STARTED,
            Phase::Closed => STEP_CLOSED,
        }
    }
}

/// What the verb takes. `--result` and `--sha` are `closed`'s alone and
/// exactly one of them rides it: the result is the step's whole record, and a
/// close carrying neither would be a step the replay reads as returning
/// nothing.
#[derive(clap::Args)]
pub struct StepArgs {
    /// the half this records
    #[arg(value_enum)]
    pub phase: Phase,

    /// the run whose step this is, by its record item's id
    #[arg(long, value_name = "RUN")]
    pub run: String,

    /// the step's number, counted from 1 in call order
    #[arg(long, value_name = "N")]
    pub n: u64,

    /// the step's name, which a re-run checks against
    #[arg(long, value_name = "NAME")]
    pub name: String,

    /// the step's result, one JSON value (closed only)
    #[arg(long, value_name = "JSON", conflicts_with = "sha")]
    pub result: Option<String>,

    /// the sha256, hex, of the result file (closed only)
    #[arg(long, value_name = "HEX", conflicts_with = "result")]
    pub sha: Option<String>,
}

/// Write one step event and confirm it landed.
///
/// The actor is `FLEET_ACTOR` or `BEADS_ACTOR` where one is set, else the run
/// id: the run's child is started with a cleared environment, so the run is
/// the one name every writer under it can be attributed to.
pub fn record(args: &StepArgs) -> Exit {
    let payload = match payload(args) {
        Ok(payload) => payload,
        Err(why) => {
            eprintln!("fleet event step: {why}");
            return Exit::Usage;
        }
    };
    let kind = args.phase.kind();
    let actor = item::actor().unwrap_or_else(|| args.run.clone());
    let stream = platform::machine_dir().join(STREAM);
    let mut log = EventLog::open(&stream);
    if let Err(e) = log.append(kind, &actor, payload) {
        eprintln!(
            "fleet event step: could not append to {}: {e}",
            stream.display()
        );
        return Exit::Refused;
    }
    match events::read_after(&stream, log.seq().saturating_sub(1)).last() {
        Some(record)
            if record.kind == kind
                && record.payload["run"] == args.run
                && record.payload["n"] == args.n =>
        {
            println!(
                "{kind} {} n={} {} — seq {}",
                args.run, args.n, args.name, record.seq
            );
            Exit::Done
        }
        other => {
            eprintln!(
                "fleet event step: the stream's last line reads {} rather than the {kind} for {} \
                 step {} just written; the record is not confirmed",
                other
                    .map(|r| format!("a {} for {}", r.kind, r.actor))
                    .unwrap_or_else(|| "nothing".to_string()),
                args.run,
                args.n
            );
            Exit::Usage
        }
    }
}

const STREAM: &str = "events.jsonl";

/// The payload, or the usage error: `--result` must parse as one JSON value,
/// because a close whose result the replay cannot read is a step it cannot
/// return.
fn payload(args: &StepArgs) -> Result<serde_json::Value, String> {
    let mut payload = serde_json::json!({
        "run": args.run,
        "n": args.n,
        "name": args.name,
    });
    match (args.phase, &args.result, &args.sha) {
        (Phase::Started, None, None) => {}
        (Phase::Started, _, _) => {
            return Err("`started` carries no result — --result and --sha are `closed`'s".into())
        }
        (Phase::Closed, Some(text), None) => {
            let result: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| format!("--result is not one JSON value: {e}"))?;
            payload["result"] = result;
        }
        (Phase::Closed, None, Some(sha)) => {
            payload["sha"] = serde_json::Value::String(sha.clone());
        }
        (Phase::Closed, None, None) => {
            return Err("`closed` carries its result: give --result <json> or --sha <hex>".into())
        }
        (Phase::Closed, Some(_), Some(_)) => {
            return Err("--result and --sha are one or the other".into())
        }
    }
    Ok(payload)
}
