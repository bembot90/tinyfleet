//! `fleet.toml` — policy, re-read on mtime (PRD R28).
//!
//! Startup refuses without it, because there is no last-good before the first
//! read. A running loop that meets a file it cannot parse keeps last-good and
//! says so once per change, never once per poll.

use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_POLL_SECONDS: u64 = 5;

/// How far back an ended session is still reported rather than treated as
/// history, when the file names no window. The figure the constant carried
/// before it was policy, so a fleet that says nothing keeps what it had.
pub const DEFAULT_STOPPED_RECENCY_HOURS: u64 = 24;

/// Where a seat is told it is heavy. A fraction of the agent's context window,
/// which is the agent's to change (lessons claude-code C5), so it is policy and
/// never a constant this code owns.
pub const DEFAULT_REST_THRESHOLD_TOKENS: u64 = 700_000;

/// How long a dispatch is given to be answered by a sighting before the seat is
/// eligible again. A start that has not shown up yet is not a seat that needs
/// another one.
pub const DEFAULT_ARRIVAL_WINDOW_SECONDS: u64 = 45;

/// How long a start is watched for an immediate failure. Long enough that a
/// child dying on its own argv reports the cause (lessons claude-code A14, whose
/// measured exit lands in 0.01 s), short enough that one poll cannot be lost to
/// a slow one.
pub const DEFAULT_START_WATCH_SECONDS: u64 = 5;

/// The deadline one nudge turn runs on.
pub const DEFAULT_NUDGE_TIMEOUT_SECONDS: u64 = 90;

/// The model a start names when the seat's row does not. The model is mandatory
/// on every start, because a start with no model flag comes up on the cheapest
/// available one (lessons claude-code A5).
pub const DEFAULT_MODEL: &str = "claude-opus-5";

/// The permission posture a named seat's session is started under, and the one a
/// transient row is. Both are values the pinned CLI's `--permission-mode` lists.
///
/// The gate below is keyed on this one value and not on the pair: the measured
/// downgrade is of a REQUESTED posture, and the transient posture asks for less
/// than the model's default rather than more.
pub const POSTURE_AUTO: &str = "auto";
pub const DEFAULT_POSTURE: &str = POSTURE_AUTO;
pub const DEFAULT_TRANSIENT_POSTURE: &str = "dontAsk";

/// The models measured to honour a requested posture, matched BY PREFIX: live
/// model ids carry suffixes that name the same model, one dated and one windowed
/// (lessons claude-code D3).
pub const DEFAULT_AUTO_CAPABLE_MODELS: [&str; 3] =
    ["claude-opus-5", "claude-fable-5", "claude-sonnet-5"];

/// The first turn a woken session is started with, with `{seat}` the seat
/// directory. The wake rides the spawn: one act, one channel, so the instruction
/// cannot be lost without also losing the session.
pub const DEFAULT_FIRST_TURN: &str = "/wake {seat}";

/// The model one nudge turn runs on. A nudge is one sentence to one session, so
/// it is the cheapest model the fleet keeps rather than the seat's own.
pub const DEFAULT_NUDGE_MODEL: &str = "claude-haiku-4-5-20251001";

/// The load belt's first leg: how much five-minute load average this fleet will
/// carry per processor before a spawn is refused (PRD R30). One load unit per
/// cpu is a machine with every core busy and nothing queued behind them.
pub const DEFAULT_LOAD_CEILING_PER_CPU: f64 = 1.0;

/// How many times a run that nothing could classify is executed again before
/// the controller parks it, where `[core.run] max_crashes` names no number.
///
/// THE RUN LIFECYCLE'S KEY AND NOT THIS CONTROLLER'S, which is why it sits under
/// `[core.run]` and is deliberately outside [`CONTROLLER_KEYS`]: a machine-local
/// answer in `config.json` would let one box run a crashing workflow a different
/// number of times from the fleet the policy describes. The figure is pinned
/// against the run lifecycle's own default in the binary's suite, which is the
/// one member that can see both crates.
pub const DEFAULT_RUN_MAX_CRASHES: u64 = 2;

/// The belt's second leg: how many transient seats may be mid-turn at once.
/// Every one of them runs a full gate, so this bounds the work the machine has
/// already accepted rather than the work it is being asked for.
pub const DEFAULT_MAX_TRANSIENT_BUSY: u32 = 3;

/// `Eq` is deliberately absent: `load_ceiling_per_cpu` is a float, which the
/// loop's "did policy move?" test compares the same way it compares the rest and
/// which no total ordering is claimed over.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub poll_seconds: u64,
    /// Where `suggest-rest` fires, in tokens.
    pub rest_threshold_tokens: u64,
    pub arrival_window_seconds: u64,
    pub start_watch_seconds: u64,
    pub nudge_timeout_seconds: u64,
    pub default_model: String,
    pub posture: String,
    pub transient_posture: String,
    /// Prefixes, not ids: membership is by family plus major (D3).
    pub auto_capable_models: Vec<String>,
    /// The template, with `{seat}` still in it — rendered per seat at the start.
    pub first_turn: String,
    pub nudge_model: String,
    /// The stopped-row window, in hours as the file writes it. A fleet whose
    /// sessions are long-lived wants a different figure from one whose seats
    /// turn over hourly, and neither is a number this code can choose.
    pub stopped_recency_hours: u64,
    /// The load belt's two ceilings (R30), read here and nowhere else.
    pub load_ceiling_per_cpu: f64,
    pub max_transient_busy: u32,
    /// The release the Claude Code adapter's behaviours were measured against.
    /// `None` when the file pins nothing, which publishes as an absent
    /// expectation rather than as agreement.
    pub claude_code_pin: Option<String>,
    /// The plugin root every start this fleet makes loads, or `None` for a fleet
    /// that names none — a session loads the overlay's hooks and finds its bin
    /// on the Bash tool's `PATH` only under a loaded plugin root (lessons
    /// claude-code D5).
    ///
    /// ABSOLUTE as [`load`] answers it and only there: a relative path is
    /// resolved against the policy FILE's own directory, which [`parse`] is
    /// handed a body and not a location for.
    pub plugin_dir: Option<PathBuf>,
    /// `[core.run] max_crashes` — the run lifecycle's cap, read here because the
    /// controller is what counts a run's crashes and parks at the cap
    /// (controller PRD R36).
    pub run_max_crashes: u64,
}

#[derive(Deserialize)]
struct RawPolicy {
    #[serde(default)]
    controller: Option<RawController>,
    #[serde(default)]
    substrate: Option<toml::Value>,
    /// `[core]`, of which this reader takes ONE key. The rest of that table
    /// belongs to the verbs and is read through core's own census; what is here
    /// is the one cap the controller is the enforcer of.
    #[serde(default)]
    core: Option<RawCore>,
}

#[derive(Deserialize)]
struct RawCore {
    #[serde(default)]
    run: Option<RawCoreRun>,
}

#[derive(Deserialize)]
struct RawCoreRun {
    #[serde(default)]
    max_crashes: Option<u64>,
}

/// The `[seats]` table's keys, in the order [`RawController`] is read for the
/// same reason: a key this list forgets is a value the file states and nothing
/// reads.
///
/// `[classes]` and `[models]` are the reference's own indirection and are NOT
/// read: a row names the model it runs on, or takes the fleet's default.
#[derive(Deserialize)]
struct RawSeatEntry {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    chosen_name: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct RawSeats {
    #[serde(default)]
    seats: std::collections::BTreeMap<String, RawSeatEntry>,
}

#[derive(Deserialize, Debug)]
struct RawController {
    #[serde(default)]
    poll_seconds: Option<u64>,
    #[serde(default)]
    stopped_recency_hours: Option<u64>,
    #[serde(default)]
    rest_threshold_tokens: Option<u64>,
    #[serde(default)]
    arrival_window_seconds: Option<u64>,
    #[serde(default)]
    start_watch_seconds: Option<u64>,
    #[serde(default)]
    nudge_timeout_seconds: Option<u64>,
    #[serde(default)]
    default_model: Option<String>,
    #[serde(default)]
    posture: Option<String>,
    #[serde(default)]
    transient_posture: Option<String>,
    #[serde(default)]
    auto_capable_models: Option<Vec<String>>,
    #[serde(default)]
    first_turn: Option<String>,
    #[serde(default)]
    nudge_model: Option<String>,
    #[serde(default)]
    load_ceiling_per_cpu: Option<f64>,
    #[serde(default)]
    max_transient_busy: Option<u32>,
    /// Deliberately absent from [`CONTROLLER_KEYS`], so no machine-local answer
    /// reaches it: a relative path here is resolved against the policy file's
    /// own directory, and `config.json` is a different directory.
    #[serde(default)]
    plugin_dir: Option<String>,
}

/// A configured whole number, with zero refused the way a zero poll interval is:
/// none of these is a figure an operator can have meant, and honouring one
/// disables the mechanism it belongs to rather than tightening it.
fn positive(configured: Option<u64>, default: u64) -> u64 {
    configured.filter(|&n| n > 0).unwrap_or(default)
}

/// A configured string, with blank refused: an empty model or posture reaches
/// the agent as a flag with no value and makes the next flag its argument.
fn named(configured: Option<&str>, default: &str) -> String {
    match configured.map(str::trim) {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => default.to_string(),
    }
}

pub fn load(path: &Path) -> Result<Policy, String> {
    let body = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut policy = parse(&body).map_err(|e| format!("{}: {e}", path.display()))?;
    policy.plugin_dir = policy
        .plugin_dir
        .take()
        .map(|dir| resolved_beside(path, dir));
    Ok(policy)
}

/// A configured path as the FILE means it: relative to the file's own directory,
/// never to the working directory, which is a service's and nobody configured it.
fn resolved_beside(policy_file: &Path, dir: PathBuf) -> PathBuf {
    if dir.is_absolute() {
        return dir;
    }
    match policy_file.parent() {
        Some(beside) if !beside.as_os_str().is_empty() => beside.join(dir),
        _ => dir,
    }
}

pub fn parse(body: &str) -> Result<Policy, String> {
    let raw: RawPolicy = toml::from_str(body).map_err(|e| e.to_string())?;
    let controller = raw.controller;
    let poll_seconds = controller
        .as_ref()
        .and_then(|c| c.poll_seconds)
        .filter(|&s| s > 0)
        .unwrap_or(DEFAULT_POLL_SECONDS);
    // Zero falls to the default the way a zero interval does, and for the same
    // reason: it is not a window an operator can have meant, and honouring it
    // would treat every ended session as history the instant it ended.
    let stopped_recency_hours = controller
        .as_ref()
        .and_then(|c| c.stopped_recency_hours)
        .filter(|&h| h > 0)
        .unwrap_or(DEFAULT_STOPPED_RECENCY_HOURS);
    let c = controller.as_ref();
    // A list that is present and empty is refused too: it would gate every
    // model out and leave a fleet that can start nothing under posture `auto`,
    // which is a configuration nobody writes on purpose.
    let auto_capable_models = c
        .and_then(|c| c.auto_capable_models.clone())
        .map(|models| {
            models
                .into_iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| {
            DEFAULT_AUTO_CAPABLE_MODELS
                .iter()
                .map(|m| m.to_string())
                .collect()
        });
    Ok(Policy {
        poll_seconds,
        rest_threshold_tokens: positive(
            c.and_then(|c| c.rest_threshold_tokens),
            DEFAULT_REST_THRESHOLD_TOKENS,
        ),
        arrival_window_seconds: positive(
            c.and_then(|c| c.arrival_window_seconds),
            DEFAULT_ARRIVAL_WINDOW_SECONDS,
        ),
        start_watch_seconds: positive(
            c.and_then(|c| c.start_watch_seconds),
            DEFAULT_START_WATCH_SECONDS,
        ),
        nudge_timeout_seconds: positive(
            c.and_then(|c| c.nudge_timeout_seconds),
            DEFAULT_NUDGE_TIMEOUT_SECONDS,
        ),
        default_model: named(c.and_then(|c| c.default_model.as_deref()), DEFAULT_MODEL),
        posture: named(c.and_then(|c| c.posture.as_deref()), DEFAULT_POSTURE),
        transient_posture: named(
            c.and_then(|c| c.transient_posture.as_deref()),
            DEFAULT_TRANSIENT_POSTURE,
        ),
        auto_capable_models,
        first_turn: named(c.and_then(|c| c.first_turn.as_deref()), DEFAULT_FIRST_TURN),
        nudge_model: named(
            c.and_then(|c| c.nudge_model.as_deref()),
            DEFAULT_NUDGE_MODEL,
        ),
        stopped_recency_hours,
        // A ceiling of zero or less refuses every spawn, and one that is not a
        // number at all is no reading: both fall to the default, the way a zero
        // interval does.
        load_ceiling_per_cpu: c
            .and_then(|c| c.load_ceiling_per_cpu)
            .filter(|n| n.is_finite() && *n > 0.0)
            .unwrap_or(DEFAULT_LOAD_CEILING_PER_CPU),
        // Zero is kept here and is NOT the default: a fleet that wants no
        // transient seat mid-turn beside a new one can say so, and the cap is a
        // count rather than an interval.
        max_transient_busy: c
            .and_then(|c| c.max_transient_busy)
            .unwrap_or(DEFAULT_MAX_TRANSIENT_BUSY),
        claude_code_pin: raw.substrate.as_ref().and_then(pin_for_claude_code),
        // A blank is no path, the way a blank model is no model: `--plugin-dir`
        // with an empty value makes the next flag its argument.
        plugin_dir: c
            .and_then(|c| c.plugin_dir.as_deref())
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from),
        // ZERO IS KEPT, and it is the one cap here that means something at zero:
        // a fleet that parks a run the first time nothing can classify it has
        // said exactly that, where a zero poll interval or a zero window is a
        // figure nobody meant.
        run_max_crashes: raw
            .core
            .as_ref()
            .and_then(|core| core.run.as_ref())
            .and_then(|run| run.max_crashes)
            .unwrap_or(DEFAULT_RUN_MAX_CRASHES),
    })
}

impl Policy {
    /// The first turn a seat's session is started with.
    pub fn first_turn_for(&self, seat_dir: &str) -> String {
        self.first_turn.replace("{seat}", seat_dir)
    }

    /// The posture a row of this kind is started under.
    pub fn posture_for(&self, transient: bool) -> &str {
        if transient {
            &self.transient_posture
        } else {
            &self.posture
        }
    }

    /// Whether a model can be trusted to honour the posture it is asked for.
    /// BY PREFIX: a live id carries a suffix that names the same model (D3).
    pub fn model_can_honour(&self, model: &str) -> bool {
        self.auto_capable_models
            .iter()
            .any(|capable| model.starts_with(capable.as_str()))
    }

    /// The model a start for this row names. Mandatory on every call (A5), so a
    /// row that configures none takes the fleet's and never the agent's.
    pub fn model_for(&self, configured: Option<&str>) -> String {
        named(configured, &self.default_model)
    }

    /// Whether this row would be started asking for a posture its model was not
    /// measured to honour — the downgrade nothing reports (D3). A row that
    /// answers `true` is dropped at config read, before any start is attempted.
    pub fn posture_is_ungranted(&self, transient: bool, model: &str) -> bool {
        self.posture_for(transient) == POSTURE_AUTO && !self.model_can_honour(model)
    }
}

// ---- the seat list, as `fleet.toml` states it -------------------------------

/// What a `[seats.<name>]` table says. `status` is `active`, `parked` or one of
/// [`SEAT_PARKED_ALIASES`], and a row that says nothing is active.
pub const SEAT_ACTIVE: &str = "active";
pub const SEAT_PARKED: &str = "parked";

/// The harness's own words for a parked seat, spelled once. One file serves
/// both readers, so a seat on vacation (kept, no worktree) and one chartered
/// (on paper, never yet started) are `parked` here: kept and not run.
pub const SEAT_PARKED_ALIASES: [&str; 2] = ["vacationing", "chartered"];

/// One `[seats.<name>]` table (PRD R4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatEntry {
    /// The seat directory, and the identity key `config.json` is joined on.
    pub name: String,
    pub model: Option<String>,
    pub chosen_name: Option<String>,
    /// True for `parked` and its aliases: a seat the fleet keeps and does not
    /// run.
    pub parked: bool,
}

/// Every `[seats.<name>]` table the file carries, in name order.
///
/// RENDERING IS `fleet start`'S ACT and never the loop's: the controller re-reads
/// `config.json` and never this table, so a changed `[seats]` table is followed
/// by a stop and a start.
pub fn seats_in(body: &str) -> Result<Vec<SeatEntry>, String> {
    let raw: RawSeats = toml::from_str(body).map_err(|e| e.to_string())?;
    raw.seats
        .into_iter()
        .filter(|(name, _)| !name.trim().is_empty())
        .map(|(name, entry)| {
            let name = name.trim().to_string();
            Ok(SeatEntry {
                parked: parked_status(&name, entry.status.as_deref())?,
                // A blank is no value, exactly as it is in `[controller]`: an
                // empty model reaches the agent as a flag whose argument is the
                // next flag.
                model: entry
                    .model
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty()),
                chosen_name: entry
                    .chosen_name
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty()),
                name,
            })
        })
        .collect()
}

/// The status, read from the four admitted words and from absence — which is
/// active.
///
/// A FIFTH VALUE IS REFUSED rather than rounded. Rounding an unrecognised word
/// to active starts a seat the person wrote `status = "park"` to keep still, and
/// that is the one mistake this key exists to prevent; a blank is the same as
/// saying nothing, because a key with no value in it was not an answer either.
fn parked_status(seat: &str, status: Option<&str>) -> Result<bool, String> {
    match status.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(false),
        Some(said) if said.eq_ignore_ascii_case(SEAT_ACTIVE) => Ok(false),
        Some(said) if said.eq_ignore_ascii_case(SEAT_PARKED) => Ok(true),
        Some(said)
            if SEAT_PARKED_ALIASES
                .iter()
                .any(|alias| said.eq_ignore_ascii_case(alias)) =>
        {
            Ok(true)
        }
        Some(said) => {
            let aliases = SEAT_PARKED_ALIASES.join("`, `");
            Err(format!(
                "[seats.{seat}] status = \"{said}\" is none of `{SEAT_ACTIVE}`, `{SEAT_PARKED}`, \
                 `{aliases}` — a status this reader does not know is refused rather than read as \
                 active, because reading it as active starts a seat somebody meant to keep still"
            ))
        }
    }
}

// ---- what `config.json` overrides, per key (PRD R28) ------------------------

/// The machine's local answer for any `[controller]` key.
///
/// `config.json` is LOCAL and beats policy per key: a machine that needs a
/// slower poll or a different model says so beside its seat list, and every
/// other key still comes from the fleet's own file.
#[derive(Default, Debug)]
pub struct Overrides {
    controller: Option<RawController>,
    /// Keys under the object that name no `[controller]` key. Ignored, and named
    /// once by the caller: a machine-local key nobody reads is worth a line and
    /// is not worth refusing the file over.
    pub unknown: Vec<String>,
    /// Keys that name a `[controller]` key and carry a value of the wrong shape.
    /// Each loses ITSELF and no other, and is named once by the caller.
    pub malformed: Vec<String>,
}

impl Overrides {
    /// Every key this machine's file wrote that no policy value took, with why.
    /// One line each, said once by the caller.
    pub fn ignored(&self) -> Vec<String> {
        self.unknown
            .iter()
            .map(|key| format!("`controller.{key}` names no policy key"))
            .chain(
                self.malformed
                    .iter()
                    .map(|key| format!("`controller.{key}` carries a value of the wrong shape")),
            )
            .collect()
    }
}

/// The overrides a machine's `config.json` carries under its top-level
/// `controller` object, or none where it carries no such object.
///
/// An object that does not read as `[controller]`'s keys is reported as an
/// unknown key rather than refused: the seat list is the file the loop cannot
/// run without.
pub fn overrides_in(value: Option<&serde_json::Value>) -> Overrides {
    let Some(object) = value.and_then(serde_json::Value::as_object) else {
        return Overrides::default();
    };
    let mut unknown = Vec::new();
    let mut malformed = Vec::new();
    let mut kept = serde_json::Map::new();
    for (key, value) in object {
        if !CONTROLLER_KEYS.contains(&key.as_str()) {
            unknown.push(key.clone());
            continue;
        }
        // ONE KEY AT A TIME, so a value of the wrong shape loses ITSELF. Read
        // as a whole object, a single `"poll_seconds": "30"` fails the
        // deserialization and takes every other override on the machine with
        // it, silently — the machine's answers are per key and their failures
        // are too.
        let alone = serde_json::Value::Object(
            std::iter::once((key.clone(), value.clone())).collect::<serde_json::Map<_, _>>(),
        );
        if serde_json::from_value::<RawController>(alone).is_ok() {
            kept.insert(key.clone(), value.clone());
        } else {
            malformed.push(key.clone());
        }
    }
    Overrides {
        // Every key that survived alone survives together: the fields are
        // independent options and nothing here can fail a second time.
        controller: serde_json::from_value(serde_json::Value::Object(kept)).ok(),
        unknown,
        malformed,
    }
}

/// The keys the object above may carry — `[controller]`'s own, listed once so a
/// key the reader does not wire is named rather than silently kept.
pub const CONTROLLER_KEYS: [&str; 14] = [
    "poll_seconds",
    "stopped_recency_hours",
    "rest_threshold_tokens",
    "arrival_window_seconds",
    "start_watch_seconds",
    "nudge_timeout_seconds",
    "default_model",
    "posture",
    "transient_posture",
    "auto_capable_models",
    "first_turn",
    "nudge_model",
    "load_ceiling_per_cpu",
    "max_transient_busy",
];

impl Policy {
    /// This policy with the machine's local answers over it, key by key.
    ///
    /// THROUGH THE SAME READERS the file goes through, so a zero or a blank in
    /// `config.json` falls back to the policy's value exactly as it falls back
    /// to the default: an override is a second source for the same key and not a
    /// second grammar for it.
    pub fn overlaid(&self, over: &Overrides) -> Policy {
        let Some(c) = over.controller.as_ref() else {
            return self.clone();
        };
        Policy {
            poll_seconds: positive(c.poll_seconds, self.poll_seconds),
            rest_threshold_tokens: positive(c.rest_threshold_tokens, self.rest_threshold_tokens),
            arrival_window_seconds: positive(c.arrival_window_seconds, self.arrival_window_seconds),
            start_watch_seconds: positive(c.start_watch_seconds, self.start_watch_seconds),
            nudge_timeout_seconds: positive(c.nudge_timeout_seconds, self.nudge_timeout_seconds),
            default_model: named(c.default_model.as_deref(), &self.default_model),
            posture: named(c.posture.as_deref(), &self.posture),
            transient_posture: named(c.transient_posture.as_deref(), &self.transient_posture),
            auto_capable_models: c
                .auto_capable_models
                .clone()
                .map(|models| {
                    models
                        .into_iter()
                        .map(|m| m.trim().to_string())
                        .filter(|m| !m.is_empty())
                        .collect::<Vec<_>>()
                })
                .filter(|models| !models.is_empty())
                .unwrap_or_else(|| self.auto_capable_models.clone()),
            first_turn: named(c.first_turn.as_deref(), &self.first_turn),
            nudge_model: named(c.nudge_model.as_deref(), &self.nudge_model),
            stopped_recency_hours: positive(c.stopped_recency_hours, self.stopped_recency_hours),
            load_ceiling_per_cpu: c
                .load_ceiling_per_cpu
                .filter(|n| n.is_finite() && *n > 0.0)
                .unwrap_or(self.load_ceiling_per_cpu),
            // The one key whose zero is a reading rather than a gap, here as in
            // the file: a fleet may want no transient seat mid-turn beside a new
            // one, and the cap is a count.
            max_transient_busy: c.max_transient_busy.unwrap_or(self.max_transient_busy),
            // `[core.run]`'s and not `[controller]`'s, so no machine-local
            // answer reaches it: the fleet's policy is where a run's caps live.
            run_max_crashes: self.run_max_crashes,
            // The pin is `[substrate]`'s and not `[controller]`'s, so no
            // machine-local key reaches it. The plugin root is `[controller]`'s
            // and still does not: its relative form is resolved against the
            // policy file's directory, and this document sits in another one.
            claude_code_pin: self.claude_code_pin.clone(),
            plugin_dir: self.plugin_dir.clone(),
        }
    }
}

/// The one agent this fleet has an adapter for, spelled as `[substrate.<agent>]`
/// spells it. ONE SPELLING: `create` checks the name it is given against this
/// list, and the pin below reads the same key off the file.
pub const AGENT_CLAUDE_CODE: &str = "claude_code";

/// Every agent name the adapters answer to.
pub const AGENTS: [&str; 1] = [AGENT_CLAUDE_CODE];

/// The pin under `[substrate.claude_code]`, in either shape a person writes it:
/// a table carrying `version`, and the flat `claude_code = "…"` string a
/// one-agent fleet reaches for. A shape this does not recognise pins nothing,
/// which publishes as no expectation rather than as a wrong one.
fn pin_for_claude_code(substrate: &toml::Value) -> Option<String> {
    let entry = substrate.get(AGENT_CLAUDE_CODE)?;
    let pin = match entry {
        toml::Value::String(s) => s.as_str(),
        table => table.get("version")?.as_str()?,
    };
    let pin = pin.trim();
    (!pin.is_empty()).then(|| pin.to_string())
}

pub fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key §8 adds, read from a file that names all of them — so a key the
    /// reader forgot to wire is a value that stays at its default here.
    #[test]
    fn every_effect_key_is_read_from_the_file() {
        let policy = parse(
            "[controller]\n\
             rest_threshold_tokens = 111\n\
             arrival_window_seconds = 222\n\
             start_watch_seconds = 333\n\
             nudge_timeout_seconds = 444\n\
             default_model = \"a-model\"\n\
             posture = \"acceptEdits\"\n\
             transient_posture = \"bypassPermissions\"\n\
             auto_capable_models = [\"a-model\", \"b-model\"]\n\
             first_turn = \"/hello {seat}\"\n\
             nudge_model = \"a-cheap-model\"\n\
             load_ceiling_per_cpu = 2.5\n\
             max_transient_busy = 7\n",
        )
        .expect("the file parses");
        assert_eq!(policy.load_ceiling_per_cpu, 2.5);
        assert_eq!(policy.max_transient_busy, 7);
        assert_eq!(policy.rest_threshold_tokens, 111);
        assert_eq!(policy.arrival_window_seconds, 222);
        assert_eq!(policy.start_watch_seconds, 333);
        assert_eq!(policy.nudge_timeout_seconds, 444);
        assert_eq!(policy.default_model, "a-model");
        assert_eq!(policy.posture, "acceptEdits");
        assert_eq!(policy.transient_posture, "bypassPermissions");
        assert_eq!(policy.auto_capable_models, vec!["a-model", "b-model"]);
        assert_eq!(policy.first_turn, "/hello {seat}");
        assert_eq!(policy.nudge_model, "a-cheap-model");
    }

    /// The same keys, absent — each at the figure or the name the constant
    /// above it carries, read from that constant rather than from a second copy
    /// of the number.
    #[test]
    fn a_file_that_names_no_effect_key_takes_every_default() {
        let policy = parse("").expect("an empty file parses");
        assert_eq!(policy.rest_threshold_tokens, DEFAULT_REST_THRESHOLD_TOKENS);
        assert_eq!(
            policy.arrival_window_seconds,
            DEFAULT_ARRIVAL_WINDOW_SECONDS
        );
        assert_eq!(policy.start_watch_seconds, DEFAULT_START_WATCH_SECONDS);
        assert_eq!(policy.nudge_timeout_seconds, DEFAULT_NUDGE_TIMEOUT_SECONDS);
        assert_eq!(policy.default_model, DEFAULT_MODEL);
        assert_eq!(policy.posture, DEFAULT_POSTURE);
        assert_eq!(policy.transient_posture, DEFAULT_TRANSIENT_POSTURE);
        assert_eq!(
            policy.auto_capable_models,
            DEFAULT_AUTO_CAPABLE_MODELS.to_vec()
        );
        assert_eq!(policy.first_turn, DEFAULT_FIRST_TURN);
        assert_eq!(policy.nudge_model, DEFAULT_NUDGE_MODEL);
        assert_eq!(policy.load_ceiling_per_cpu, DEFAULT_LOAD_CEILING_PER_CPU);
        assert_eq!(policy.max_transient_busy, DEFAULT_MAX_TRANSIENT_BUSY);
    }

    /// The plugin root, in the four states a file can leave it: absent, blank,
    /// relative and absolute.
    ///
    /// The relative one is read through [`load`], because the base is the FILE's
    /// own directory and `parse` is handed a body: the working directory is a
    /// service's and nobody configured it, so the arm asserts against the file's
    /// directory and states the other answer it must not be.
    #[test]
    fn the_plugin_root_is_resolved_against_the_policy_files_own_directory() {
        assert_eq!(
            parse("").expect("an empty file parses").plugin_dir,
            None,
            "a file that names none names none"
        );
        assert_eq!(
            parse("[controller]\nplugin_dir = \"   \"\n")
                .expect("the file parses")
                .plugin_dir,
            None,
            "a blank is no path, the way a blank model is no model"
        );

        let dir = std::env::temp_dir().join(format!("fleet-policy-plugin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory is made");
        let file = dir.join("fleet.toml");

        std::fs::write(&file, "[controller]\nplugin_dir = \"the-overlay\"\n")
            .expect("the file is written");
        let relative = load(&file).expect("the file parses");
        assert_eq!(relative.plugin_dir, Some(dir.join("the-overlay")));
        let here = std::env::current_dir().expect("this process has a working directory");
        assert_ne!(
            relative.plugin_dir,
            Some(here.join("the-overlay")),
            "and not the working directory's, which is the other answer available"
        );

        // The control on the base: an ABSOLUTE path is taken as written, so the
        // join above is the relative case's and not applied to every reading.
        let absolute = dir.join("elsewhere");
        std::fs::write(
            &file,
            format!("[controller]\nplugin_dir = \"{}\"\n", absolute.display()),
        )
        .expect("the file is written");
        assert_eq!(
            load(&file).expect("the file parses").plugin_dir,
            Some(absolute)
        );

        std::fs::write(&file, "[controller]\nplugin_dir = \"\"\n").expect("the file is written");
        assert_eq!(
            load(&file).expect("the file parses").plugin_dir,
            None,
            "a blank is no path through the loader too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The belt's two ceilings part company on zero, which is why they are read
    /// by two rules rather than one: a ceiling of zero refuses every spawn on a
    /// machine with no load at all, and a cap of zero is a fleet that will start
    /// a transient seat only when no other one is mid-turn.
    #[test]
    fn a_zero_load_ceiling_is_the_default_and_a_zero_transient_cap_is_a_reading() {
        let zeroed = parse("[controller]\nload_ceiling_per_cpu = 0.0\nmax_transient_busy = 0\n")
            .expect("the file parses");
        assert_eq!(zeroed.load_ceiling_per_cpu, DEFAULT_LOAD_CEILING_PER_CPU);
        assert_eq!(
            zeroed.max_transient_busy, 0,
            "zero busy is a cap, not a gap"
        );

        // A negative and a non-finite ceiling are the same non-reading as zero.
        for body in [
            "[controller]\nload_ceiling_per_cpu = -1.0\n",
            "[controller]\nload_ceiling_per_cpu = nan\n",
            "[controller]\nload_ceiling_per_cpu = inf\n",
        ] {
            assert_eq!(
                parse(body).expect("the file parses").load_ceiling_per_cpu,
                DEFAULT_LOAD_CEILING_PER_CPU,
                "{body:?}"
            );
        }
    }

    /// A zero and a blank are not readings. Honouring either disables the
    /// mechanism it belongs to: a zero watch window collects no failed start, a
    /// zero arrival window dispatches once per poll, and a blank model reaches
    /// the agent as a flag whose argument is the next flag.
    #[test]
    fn a_zero_or_blank_setting_is_the_default_and_never_a_disabled_mechanism() {
        let zeroed = parse(
            "[controller]\n\
             rest_threshold_tokens = 0\n\
             arrival_window_seconds = 0\n\
             start_watch_seconds = 0\n\
             nudge_timeout_seconds = 0\n\
             default_model = \"  \"\n\
             posture = \"\"\n\
             transient_posture = \"\"\n\
             auto_capable_models = []\n\
             first_turn = \"\"\n\
             nudge_model = \"   \"\n",
        )
        .expect("the file parses");
        assert_eq!(zeroed.rest_threshold_tokens, DEFAULT_REST_THRESHOLD_TOKENS);
        assert_eq!(
            zeroed.arrival_window_seconds,
            DEFAULT_ARRIVAL_WINDOW_SECONDS
        );
        assert_eq!(zeroed.start_watch_seconds, DEFAULT_START_WATCH_SECONDS);
        assert_eq!(zeroed.nudge_timeout_seconds, DEFAULT_NUDGE_TIMEOUT_SECONDS);
        assert_eq!(zeroed.default_model, DEFAULT_MODEL);
        assert_eq!(zeroed.posture, DEFAULT_POSTURE);
        assert_eq!(zeroed.transient_posture, DEFAULT_TRANSIENT_POSTURE);
        assert_eq!(
            zeroed.auto_capable_models,
            DEFAULT_AUTO_CAPABLE_MODELS.to_vec(),
            "a present but empty list would gate every model out"
        );
        assert_eq!(zeroed.first_turn, DEFAULT_FIRST_TURN);
        assert_eq!(zeroed.nudge_model, DEFAULT_NUDGE_MODEL);
    }

    /// The four readings a start is composed from, each with the case that is
    /// not the default beside it.
    #[test]
    fn a_start_is_composed_from_the_seats_row_and_the_fleets_policy() {
        let policy = parse("[controller]\nfirst_turn = \"/wake {seat}\"\n").expect("it parses");
        assert_eq!(policy.first_turn_for("builder-9"), "/wake builder-9");
        // A template naming the seat twice renders it twice, and one naming it
        // not at all renders as itself — the substitution is textual and says so.
        let twice = parse("[controller]\nfirst_turn = \"{seat}: /wake {seat}\"\n").unwrap();
        assert_eq!(twice.first_turn_for("s1"), "s1: /wake s1");
        let fixed = parse("[controller]\nfirst_turn = \"/orient\"\n").unwrap();
        assert_eq!(fixed.first_turn_for("s1"), "/orient");

        assert_eq!(policy.posture_for(false), DEFAULT_POSTURE);
        assert_eq!(policy.posture_for(true), DEFAULT_TRANSIENT_POSTURE);

        assert_eq!(policy.model_for(Some("a-model")), "a-model");
        assert_eq!(policy.model_for(None), DEFAULT_MODEL);
        assert_eq!(
            policy.model_for(Some("   ")),
            DEFAULT_MODEL,
            "a blank row-level model is no model, not an empty one"
        );
    }

    /// Every key the machine may answer for, over a policy that answers
    /// differently — so a key the overlay forgot to wire is a value that stays
    /// at the policy's here.
    #[test]
    fn every_controller_key_can_be_answered_by_the_machines_own_file() {
        let file = parse(
            "[controller]\n\
             poll_seconds = 7\n\
             stopped_recency_hours = 7\n\
             rest_threshold_tokens = 7\n\
             arrival_window_seconds = 7\n\
             start_watch_seconds = 7\n\
             nudge_timeout_seconds = 7\n\
             default_model = \"from-policy\"\n\
             posture = \"from-policy\"\n\
             transient_posture = \"from-policy\"\n\
             auto_capable_models = [\"from-policy\"]\n\
             first_turn = \"from-policy {seat}\"\n\
             nudge_model = \"from-policy\"\n\
             load_ceiling_per_cpu = 7.0\n\
             max_transient_busy = 7\n\
             \n[substrate.claude_code]\nversion = \"2.1.261\"\n",
        )
        .expect("the policy parses");

        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": 11,
            "stopped_recency_hours": 11,
            "rest_threshold_tokens": 11,
            "arrival_window_seconds": 11,
            "start_watch_seconds": 11,
            "nudge_timeout_seconds": 11,
            "default_model": "from-machine",
            "posture": "from-machine",
            "transient_posture": "from-machine",
            "auto_capable_models": ["from-machine"],
            "first_turn": "from-machine {seat}",
            "nudge_model": "from-machine",
            "load_ceiling_per_cpu": 11.0,
            "max_transient_busy": 11,
        })));
        assert!(over.unknown.is_empty(), "{:?}", over.unknown);

        let effective = file.overlaid(&over);
        assert_eq!(effective.poll_seconds, 11);
        assert_eq!(effective.stopped_recency_hours, 11);
        assert_eq!(effective.rest_threshold_tokens, 11);
        assert_eq!(effective.arrival_window_seconds, 11);
        assert_eq!(effective.start_watch_seconds, 11);
        assert_eq!(effective.nudge_timeout_seconds, 11);
        assert_eq!(effective.default_model, "from-machine");
        assert_eq!(effective.posture, "from-machine");
        assert_eq!(effective.transient_posture, "from-machine");
        assert_eq!(effective.auto_capable_models, vec!["from-machine"]);
        assert_eq!(effective.first_turn, "from-machine {seat}");
        assert_eq!(effective.nudge_model, "from-machine");
        assert_eq!(effective.load_ceiling_per_cpu, 11.0);
        assert_eq!(effective.max_transient_busy, 11);
        assert_eq!(
            effective.claude_code_pin.as_deref(),
            Some("2.1.261"),
            "the pin is `[substrate]`'s, so no machine-local key reaches it"
        );

        // THE CONTROL every assertion above needs: with no object, every one of
        // those keys is the policy's — so the fourteen readings are the
        // machine's answers and not a function that answers 11 regardless.
        let untouched = file.overlaid(&Overrides::default());
        assert_eq!(untouched, file);
        assert_eq!(untouched.poll_seconds, 7);
        assert_eq!(untouched.default_model, "from-policy");
        assert_eq!(untouched.auto_capable_models, vec!["from-policy"]);

        // Every key in the census is one the object above named, so a key added
        // to `[controller]` and forgotten here is a failure and not a silence.
        assert_eq!(CONTROLLER_KEYS.len(), 14);
        for key in CONTROLLER_KEYS {
            let named = overrides_in(Some(&serde_json::json!({ key: 1 })));
            assert!(
                named.unknown.is_empty(),
                "{key} is in the census and reads as unknown"
            );
        }
    }

    /// A zero, a blank and an empty list fall to the POLICY's value — the same
    /// non-readings the file's own reader refuses, so an override is a second
    /// source for a key and never a second grammar for it.
    #[test]
    fn a_zero_or_blank_override_falls_back_to_policy_and_not_to_the_default() {
        let file = parse(
            "[controller]\n\
             poll_seconds = 7\n\
             rest_threshold_tokens = 77\n\
             default_model = \"from-policy\"\n\
             auto_capable_models = [\"from-policy\"]\n\
             load_ceiling_per_cpu = 7.0\n\
             max_transient_busy = 7\n",
        )
        .expect("the policy parses");

        let effective = file.overlaid(&overrides_in(Some(&serde_json::json!({
            "poll_seconds": 0,
            "rest_threshold_tokens": 0,
            "default_model": "   ",
            "auto_capable_models": [],
            "load_ceiling_per_cpu": 0.0,
        }))));
        assert_eq!(effective.poll_seconds, 7, "and not {DEFAULT_POLL_SECONDS}");
        assert_eq!(effective.rest_threshold_tokens, 77);
        assert_eq!(effective.default_model, "from-policy");
        assert_eq!(effective.auto_capable_models, vec!["from-policy"]);
        assert_eq!(effective.load_ceiling_per_cpu, 7.0);
        assert_ne!(
            effective.poll_seconds, DEFAULT_POLL_SECONDS,
            "the policy's figure and the compiled default must differ, or this arm \
             could not tell them apart"
        );

        // The one key whose zero is a READING and not a gap, here as in the
        // file: a fleet may want no transient seat mid-turn beside a new one.
        let capped = file.overlaid(&overrides_in(Some(
            &serde_json::json!({ "max_transient_busy": 0 }),
        )));
        assert_eq!(capped.max_transient_busy, 0);
    }

    /// A KEY THAT IS KNOWN AND WRONGLY TYPED LOSES ITSELF AND NO OTHER.
    ///
    /// Read as one object, a single `"poll_seconds": "30"` fails the whole
    /// deserialization: every other override the machine wrote is dropped, and
    /// silently, because a known key never reaches the unknown list. The
    /// control is the good key beside it, which must still land.
    #[test]
    fn one_wrongly_typed_override_loses_itself_and_leaves_every_other_one_standing() {
        let file = parse(
            "[controller]\npoll_seconds = 7\nnudge_model = \"from-policy\"\n\
             rest_threshold_tokens = 77\n",
        )
        .expect("the policy parses");

        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": "30",
            "nudge_model": "from-machine",
            "rest_threshold_tokens": 111,
        })));
        assert_eq!(over.malformed, vec!["poll_seconds"]);
        assert!(over.unknown.is_empty(), "a known key is not an unknown one");

        let effective = file.overlaid(&over);
        assert_eq!(
            effective.poll_seconds, 7,
            "the wrongly-typed key falls back to policy"
        );
        assert_eq!(
            effective.nudge_model, "from-machine",
            "and the other overrides on the same machine still land"
        );
        assert_eq!(effective.rest_threshold_tokens, 111);

        // It is NAMED, once, beside the unknown keys and with why.
        let said = over.ignored();
        assert_eq!(said.len(), 1);
        assert!(said[0].contains("poll_seconds"), "{said:?}");
        assert!(said[0].contains("wrong shape"), "{said:?}");

        // The control: the same object with the key typed right overrides it,
        // so the fallback above is the VALUE's and not a key this reader drops.
        let good = file.overlaid(&overrides_in(Some(&serde_json::json!({
            "poll_seconds": 30,
            "nudge_model": "from-machine",
        }))));
        assert_eq!(good.poll_seconds, 30);
        assert_eq!(good.nudge_model, "from-machine");
    }

    /// A key the object carries that names no `[controller]` key is IGNORED and
    /// named, and an object that is not one at all overrides nothing.
    #[test]
    fn an_unknown_override_key_is_named_and_a_shape_that_is_not_an_object_is_no_override() {
        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": 11,
            "not_a_key": true,
            "another": 1,
        })));
        assert_eq!(over.unknown, vec!["another", "not_a_key"]);
        assert!(over.malformed.is_empty());
        let file = parse("[controller]\npoll_seconds = 7\n").unwrap();
        assert_eq!(
            file.overlaid(&over).poll_seconds,
            11,
            "the known key still lands"
        );

        for shape in [
            serde_json::json!("a string"),
            serde_json::json!([1, 2]),
            serde_json::json!(null),
        ] {
            let none = overrides_in(Some(&shape));
            assert!(none.unknown.is_empty());
            assert_eq!(file.overlaid(&none), file, "{shape}");
        }
        assert_eq!(file.overlaid(&overrides_in(None)), file);
    }

    /// The `[seats]` table (R4): the three keys a row may carry, the defaults a
    /// row that says nothing takes, and the blanks that are no value.
    #[test]
    fn a_seats_table_reads_its_rows_and_a_row_that_says_nothing_is_active() {
        let seats = seats_in(
            "[seats.one]\nmodel = \"a-model\"\nchosen_name = \"Kite\"\nstatus = \"active\"\n\
             \n[seats.two]\n\
             \n[seats.three]\nstatus = \"PARKED\"\n\
             \n[seats.four]\nmodel = \"  \"\nchosen_name = \"\"\n",
        )
        .expect("the table parses");

        assert_eq!(
            seats.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["four", "one", "three", "two"],
            "in name order, whatever order the file wrote them in"
        );
        let row = |name: &str| seats.iter().find(|s| s.name == name).expect(name);
        assert_eq!(row("one").model.as_deref(), Some("a-model"));
        assert_eq!(row("one").chosen_name.as_deref(), Some("Kite"));
        assert!(!row("one").parked);
        assert!(
            row("two").model.is_none(),
            "a row that names no model takes the fleet's"
        );
        assert!(!row("two").parked, "a row that says nothing is active");
        assert!(
            row("three").parked,
            "the status is read without regard to case"
        );
        assert!(
            row("four").model.is_none() && row("four").chosen_name.is_none(),
            "a blank is no value, exactly as it is in `[controller]`"
        );

        // The reference's own indirection is NOT read: a file carrying those
        // tables yields the rows and nothing from them.
        let ignored = seats_in(
            "[classes.architect]\nmodel = \"from-a-class\"\n\
             [models.big]\nid = \"from-a-model\"\n\
             [seats.one]\nclass = \"architect\"\n",
        )
        .expect("the table parses");
        assert_eq!(ignored.len(), 1);
        assert!(ignored[0].model.is_none());

        assert!(seats_in("[controller]\npoll_seconds = 5\n")
            .unwrap()
            .is_empty());
        assert!(
            seats_in("[seats\n").is_err(),
            "a file that does not parse is an error"
        );
    }

    /// THE STATUS IS READ FROM THE ADMITTED WORDS AND NO OTHERS. A fifth value
    /// is refused rather than rounded to active: rounding starts the seat
    /// somebody wrote it to keep still, which is the one mistake this key
    /// exists to prevent.
    #[test]
    fn a_status_that_is_neither_word_is_refused_and_never_read_as_active() {
        for said in ["park", "parked.", "inactive", "off", "Active!"] {
            let refused = seats_in(&format!("[seats.one]\nstatus = \"{said}\"\n"))
                .expect_err("an unrecognised status is refused");
            assert!(refused.contains(said), "{refused}");
            assert!(refused.contains("[seats.one]"), "{refused}");
            assert!(
                refused.contains(SEAT_ACTIVE)
                    && refused.contains(SEAT_PARKED)
                    && SEAT_PARKED_ALIASES.iter().all(|a| refused.contains(a)),
                "the refusal names all four it knows: {refused}"
            );
        }

        // The control: the readings it DOES take, so the refusals above are
        // about an unadmitted word and not about a reader that refuses every
        // status. `active` is compared as well as `parked` — the constant is
        // declared and this is what reads it.
        assert!(!seats_in("[seats.one]\nstatus = \"active\"\n").unwrap()[0].parked);
        assert!(!seats_in("[seats.one]\nstatus = \"ACTIVE\"\n").unwrap()[0].parked);
        assert!(seats_in("[seats.one]\nstatus = \"parked\"\n").unwrap()[0].parked);
        assert!(!seats_in("[seats.one]\n").unwrap()[0].parked);
        assert!(
            !seats_in("[seats.one]\nstatus = \"  \"\n").unwrap()[0].parked,
            "a blank was not an answer either, so it is the absent reading"
        );
    }

    /// `vacationing` is the harness's word for a seat it keeps and does not
    /// run — no worktree, no porter row — which is this reader's `parked`.
    #[test]
    fn seats_in_reads_a_vacationing_seat_as_parked() {
        assert!(seats_in("[seats.one]\nstatus = \"vacationing\"\n").unwrap()[0].parked);
        assert!(
            seats_in("[seats.one]\nstatus = \"Vacationing\"\n").unwrap()[0].parked,
            "read without regard to case, like the other words"
        );
        refusal_for("park");
    }

    /// `chartered` is a seat that exists on paper and has never been started,
    /// which the fleet also keeps and does not run.
    #[test]
    fn seats_in_reads_a_chartered_seat_as_parked() {
        assert!(seats_in("[seats.one]\nstatus = \"chartered\"\n").unwrap()[0].parked);
        assert!(
            seats_in("[seats.one]\nstatus = \"CHARTERED\"\n").unwrap()[0].parked,
            "read without regard to case, like the other words"
        );
        refusal_for("park");
    }

    /// The control both alias arms lean on: an unadmitted word still refuses,
    /// and the sentence names all four the reader admits.
    fn refusal_for(said: &str) -> String {
        let refused = seats_in(&format!("[seats.one]\nstatus = \"{said}\"\n"))
            .expect_err("a word outside the admitted four is refused");
        assert!(
            refused.contains(SEAT_ACTIVE)
                && refused.contains(SEAT_PARKED)
                && SEAT_PARKED_ALIASES.iter().all(|a| refused.contains(a)),
            "the refusal names all four it knows: {refused}"
        );
        refused
    }

    #[test]
    fn the_pin_reads_from_a_table_or_from_a_flat_string() {
        let table = parse("[substrate.claude_code]\nversion = \"2.1.261\"\n").unwrap();
        assert_eq!(table.claude_code_pin.as_deref(), Some("2.1.261"));
        let flat = parse("[substrate]\nclaude_code = \"2.1.261\"\n").unwrap();
        assert_eq!(flat.claude_code_pin.as_deref(), Some("2.1.261"));
    }

    #[test]
    fn a_file_that_pins_nothing_expects_nothing() {
        let policy = parse("[controller]\npoll_seconds = 9\n").unwrap();
        assert_eq!(policy.claude_code_pin, None);
        assert_eq!(policy.poll_seconds, 9);
    }

    #[test]
    fn an_absent_or_zero_poll_interval_falls_to_the_default() {
        assert_eq!(parse("").unwrap().poll_seconds, DEFAULT_POLL_SECONDS);
        assert_eq!(
            parse("[controller]\npoll_seconds = 0\n")
                .unwrap()
                .poll_seconds,
            DEFAULT_POLL_SECONDS
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error_carrying_why() {
        let err = parse("[controller\npoll_seconds = 5\n").expect_err("broken TOML is refused");
        assert!(!err.is_empty(), "the parse error travels with the refusal");
    }
}
