//! `fleet.toml` — policy, re-read on mtime.
//!
//! Startup refuses without it, because there is no last-good before the first
//! read. A running loop that meets a file it cannot parse keeps last-good and
//! says so once per change, never once per poll.

use fleet_core::agent::{Capabilities, Posture};
use serde::Deserialize;
use std::path::Path;

pub const DEFAULT_POLL_SECONDS: u64 = 5;

/// Where a seat is told it is heavy. A fraction of the agent's context window,
/// which is the agent's to change (lessons claude-code C5), so it is policy and
/// never a constant this code owns.
pub const DEFAULT_REST_THRESHOLD_TOKENS: u64 = 700_000;

/// How long a dispatch is given to be answered by a sighting before the seat is
/// eligible again. A start that has not shown up yet is not a seat that needs
/// another one.
pub const DEFAULT_ARRIVAL_WINDOW_SECONDS: u64 = 45;

/// How long a start is watched before it is believed or given up on: until the
/// agent's listing shows the pane's own process with a status. Measured
/// through the claude-code pack on 2.1.280 and tmux 3.7b (fleet-rge6.2,
/// 2026-09-26), the row was listed 0.5–0.75 s after the session was made and
/// carried its status at 1.0–1.3 s, so five seconds is four times the slowest reading; short enough
/// that one poll cannot be lost to a start that will never list.
pub const DEFAULT_START_WATCH_SECONDS: u64 = 5;

/// How long a turn typed into a seat's session is given to be taken: the row
/// must read busy inside it (`crate::effect::type_turn`). Through the
/// claude-code pack, on 2.1.280, a typed turn read busy on the first read
/// after its submit, 0.14 s on (fleet-rge6.5, 2026-09-26), so ten seconds is far past any turn that
/// will be taken, and short enough that a poll is not held long by one that
/// will not.
pub const DEFAULT_NUDGE_TIMEOUT_SECONDS: u64 = 10;

/// The posture a named seat's session is started under, and the one a
/// transient row is, in fleet's own words (ruling 14): a named seat acts
/// within its model's judgement, and a transient one has nobody there, so an
/// act its rules do not allow is refused rather than asked.
///
/// The gate below is keyed on the posture and not on the pair: the measured
/// downgrade is of a REQUESTED posture, and the transient posture asks for less
/// than the model's default rather than more.
pub const DEFAULT_POSTURE: Posture = Posture::Auto;
pub const DEFAULT_TRANSIENT_POSTURE: Posture = Posture::Unattended;

/// The load belt's first leg: how much five-minute load average this fleet will
/// carry per processor before a spawn is refused. One load unit per cpu is a
/// machine with every core busy and nothing queued behind them.
pub const DEFAULT_LOAD_CEILING_PER_CPU: f64 = 1.0;

/// How many times a run that nothing could classify is executed again before
/// the controller parks it, where `[core.run] max_crashes` names no number.
///
/// THE RUN LIFECYCLE'S KEY AND NOT THIS CONTROLLER'S, which is why it sits under
/// `[core.run]` and is deliberately outside [`CONTROLLER_KEYS`]: a machine-local
/// answer in `config.json` would let one box run a crashing workflow a different
/// number of times from the fleet the policy describes. It is the run
/// lifecycle's own default, re-exported, so the two cannot differ.
pub use fleet_core::item::run::MAX_CRASHES as DEFAULT_RUN_MAX_CRASHES;

/// The belt's second leg: how many transient seats may be mid-turn at once.
/// Every one of them runs its own checks, so this bounds the work the machine has
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
    /// The model a seat that names none starts on, where the file names one;
    /// `None` falls to the agent's own declared default
    /// ([`Policy::model_for`]).
    pub default_model: Option<String>,
    pub posture: Posture,
    pub transient_posture: Posture,
    /// The models `auto` is held to, where the file names them: prefixes, not
    /// ids, since membership is by family plus major (D3). `None` falls to the
    /// agent's own declared gate ([`Policy::gate_for`]).
    pub auto_capable_models: Option<Vec<String>>,
    /// The first-turn template, with `{seat}` still in it, where the file names
    /// one; `None` falls to the agent's own declared template.
    pub first_turn: Option<String>,
    /// The load belt's two ceilings, read here and nowhere else.
    pub load_ceiling_per_cpu: f64,
    pub max_transient_busy: u32,
    /// `[core.run] max_crashes` — the run lifecycle's cap, read here because the
    /// controller is what counts a run's crashes and parks at the cap.
    pub run_max_crashes: u64,
}

#[derive(Deserialize)]
struct RawPolicy {
    #[serde(default)]
    controller: Option<RawController>,
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

#[derive(Deserialize, Debug)]
struct RawController {
    #[serde(default)]
    poll_seconds: Option<u64>,
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
    load_ceiling_per_cpu: Option<f64>,
    #[serde(default)]
    max_transient_busy: Option<u32>,
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

/// A configured string, or `None` where it is blank or absent: the key's
/// answer is then another's to give, and a blank never reaches a start.
fn stated(configured: Option<&str>) -> Option<String> {
    configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// A configured posture through the one reader ([`Posture::read`]), trimmed,
/// with a blank taken as the key left out: `None` where nothing is written,
/// and the reader's refusal where a word is and it is not a posture.
fn posture(configured: Option<&str>) -> Result<Option<Posture>, String> {
    match configured.map(str::trim) {
        Some(word) if !word.is_empty() => Posture::read(word).map(Some),
        _ => Ok(None),
    }
}

/// A configured list of model prefixes, trimmed, with blanks dropped, and
/// `None` where nothing is left.
fn models(configured: Option<Vec<String>>) -> Option<Vec<String>> {
    configured
        .map(|models| {
            models
                .into_iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|models| !models.is_empty())
}

pub fn load(path: &Path) -> Result<Policy, String> {
    load_with_table(path).map(|(policy, _)| policy)
}

/// One read of the policy file: the policy it parses to and the whole file as
/// a table, both from the same bytes, so a reader holding one never holds the
/// other from a different moment.
pub fn load_with_table(path: &Path) -> Result<(Policy, toml::Table), String> {
    let body = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let policy = parse(&body).map_err(|e| format!("{}: {e}", path.display()))?;
    let table = body
        .parse::<toml::Table>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((policy, table))
}

pub fn parse(body: &str) -> Result<Policy, String> {
    let raw: RawPolicy = toml::from_str(body).map_err(|e| e.to_string())?;
    let c = raw.controller.as_ref();
    // A posture that is no posture is REFUSED, not defaulted: a fleet that
    // asked its seats to run unattended and was started asking would stop at
    // the first dialog with nobody there, and one that asked to be asked and
    // ran unattended would act unasked.
    let posture_key = |key: &str, configured: Option<&String>| {
        posture(configured.map(String::as_str))
            .map_err(|why| format!("[controller] `{key}`: {why}"))
    };
    posture_key("posture", c.and_then(|c| c.posture.as_ref()))?;
    posture_key(
        "transient_posture",
        c.and_then(|c| c.transient_posture.as_ref()),
    )?;
    let defaults = Policy {
        poll_seconds: DEFAULT_POLL_SECONDS,
        rest_threshold_tokens: DEFAULT_REST_THRESHOLD_TOKENS,
        arrival_window_seconds: DEFAULT_ARRIVAL_WINDOW_SECONDS,
        start_watch_seconds: DEFAULT_START_WATCH_SECONDS,
        nudge_timeout_seconds: DEFAULT_NUDGE_TIMEOUT_SECONDS,
        default_model: None,
        posture: DEFAULT_POSTURE,
        transient_posture: DEFAULT_TRANSIENT_POSTURE,
        auto_capable_models: None,
        first_turn: None,
        load_ceiling_per_cpu: DEFAULT_LOAD_CEILING_PER_CPU,
        max_transient_busy: DEFAULT_MAX_TRANSIENT_BUSY,
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
    };
    Ok(match c {
        Some(c) => apply(&defaults, c),
        None => defaults,
    })
}

/// The `[controller]` keys read over a base, key by key, through the one set
/// of readers: `base` is the defaults for the file, and the policy for an
/// overlay.
fn apply(base: &Policy, c: &RawController) -> Policy {
    Policy {
        poll_seconds: positive(c.poll_seconds, base.poll_seconds),
        rest_threshold_tokens: positive(c.rest_threshold_tokens, base.rest_threshold_tokens),
        arrival_window_seconds: positive(c.arrival_window_seconds, base.arrival_window_seconds),
        start_watch_seconds: positive(c.start_watch_seconds, base.start_watch_seconds),
        nudge_timeout_seconds: positive(c.nudge_timeout_seconds, base.nudge_timeout_seconds),
        default_model: stated(c.default_model.as_deref()).or(base.default_model.clone()),
        // A refused word never reaches here ([`overrides_in`] keeps only
        // the keys that read, and [`parse`] refuses one before it calls
        // this), so a posture left is one of the three or blank, and a
        // blank falls to the base's.
        posture: posture(c.posture.as_deref())
            .ok()
            .flatten()
            .unwrap_or(base.posture),
        transient_posture: posture(c.transient_posture.as_deref())
            .ok()
            .flatten()
            .unwrap_or(base.transient_posture),
        // A list that is present and empty is refused too: it would gate every
        // model out and leave a fleet that can start nothing under posture `auto`,
        // which is a configuration nobody writes on purpose.
        auto_capable_models: models(c.auto_capable_models.clone())
            .or(base.auto_capable_models.clone()),
        first_turn: stated(c.first_turn.as_deref()).or(base.first_turn.clone()),
        // A ceiling of zero or less refuses every spawn, and one that is not a
        // number at all is no reading: both fall to the base's, the way a zero
        // interval does.
        load_ceiling_per_cpu: c
            .load_ceiling_per_cpu
            .filter(|n| n.is_finite() && *n > 0.0)
            .unwrap_or(base.load_ceiling_per_cpu),
        // Zero is kept here and is NOT the default: a fleet that wants no
        // transient seat mid-turn beside a new one can say so, and the cap is a
        // count rather than an interval.
        //
        // The one key whose zero is a reading rather than a gap, here as in
        // the file: a fleet may want no transient seat mid-turn beside a new
        // one, and the cap is a count.
        max_transient_busy: c.max_transient_busy.unwrap_or(base.max_transient_busy),
        // `[core.run]`'s and not `[controller]`'s, so no machine-local
        // answer reaches it: the fleet's policy is where a run's caps live.
        run_max_crashes: base.run_max_crashes,
    }
}

impl Policy {
    /// The first turn a seat's session is started with, with `{seat}` filled
    /// by the session's name: the file's template, else the one the agent
    /// declares.
    pub fn first_turn_for(&self, session_name: &str, agent: &Capabilities) -> String {
        self.first_turn
            .as_deref()
            .unwrap_or(&agent.first_turn)
            .replace("{seat}", session_name)
    }

    /// The posture a row of this kind is started under.
    pub fn posture_for(&self, transient: bool) -> Posture {
        if transient {
            self.transient_posture
        } else {
            self.posture
        }
    }

    /// The model a start for this row names. Mandatory on every call (A5), so a
    /// row that configures none takes the fleet's, and a fleet that configures
    /// none takes the model its agent declares — never the agent's own
    /// unflagged choice.
    pub fn model_for(&self, configured: Option<&str>, agent: &Capabilities) -> String {
        named(
            configured,
            self.default_model
                .as_deref()
                .unwrap_or(&agent.default_model),
        )
    }

    /// The models a row of this kind is held to (the D3 gate): the file's own
    /// list where the row asks for `auto` and the file names one, and
    /// otherwise the prefixes the agent declares for the row's posture. Empty
    /// is no gate at all.
    pub fn gate_for(&self, transient: bool, agent: &Capabilities) -> Vec<String> {
        let posture = self.posture_for(transient);
        match (&self.auto_capable_models, posture) {
            (Some(models), Posture::Auto) => models.clone(),
            _ => agent
                .posture_models
                .get(&posture)
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Whether this row would be started asking for a posture its model was not
    /// measured to honour — the downgrade nothing reports (D3). BY PREFIX: a
    /// live id carries a suffix that names the same model. A row that answers
    /// `true` is dropped at config read, before any start is attempted.
    pub fn posture_is_ungranted(&self, transient: bool, model: &str, agent: &Capabilities) -> bool {
        let gate = self.gate_for(transient, agent);
        !gate.is_empty()
            && !gate
                .iter()
                .any(|capable| model.starts_with(capable.as_str()))
    }
}

// ---- what `config.json` overrides, per key ----------------------------------

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
    /// Posture keys whose word is no posture, each with the reader's refusal:
    /// refused the way a wrong shape is — the key loses itself and no other,
    /// and the caller names it once with the word and the three it can use.
    pub refused: Vec<(String, String)>,
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
            .chain(
                self.refused
                    .iter()
                    .map(|(key, why)| format!("`controller.{key}` is refused: {why}")),
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
    let mut refused = Vec::new();
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
        match serde_json::from_value::<RawController>(alone) {
            // The same reader the file's posture keys go through, so the
            // machine's answer is refused in the file's own sentence.
            Ok(read) => match posture(read.posture.or(read.transient_posture).as_deref()) {
                Ok(_) => {
                    kept.insert(key.clone(), value.clone());
                }
                Err(why) => refused.push((key.clone(), why)),
            },
            Err(_) => malformed.push(key.clone()),
        }
    }
    Overrides {
        // Every key that survived alone survives together: the fields are
        // independent options and nothing here can fail a second time.
        controller: serde_json::from_value(serde_json::Value::Object(kept)).ok(),
        unknown,
        malformed,
        refused,
    }
}

/// The keys the object above may carry — `[controller]`'s own, listed once so a
/// key the reader does not wire is named rather than silently kept.
pub const CONTROLLER_KEYS: [&str; 12] = [
    "poll_seconds",
    "rest_threshold_tokens",
    "arrival_window_seconds",
    "start_watch_seconds",
    "nudge_timeout_seconds",
    "default_model",
    "posture",
    "transient_posture",
    "auto_capable_models",
    "first_turn",
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
        over.controller
            .as_ref()
            .map_or_else(|| self.clone(), |c| apply(self, c))
    }
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
             posture = \"ask\"\n\
             transient_posture = \"unattended\"\n\
             auto_capable_models = [\"a-model\", \"b-model\"]\n\
             first_turn = \"/hello {seat}\"\n\
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
        assert_eq!(policy.default_model.as_deref(), Some("a-model"));
        assert_eq!(policy.posture, Posture::Ask);
        assert_eq!(policy.transient_posture, Posture::Unattended);
        assert_eq!(
            policy.auto_capable_models,
            Some(vec!["a-model".to_string(), "b-model".to_string()])
        );
        assert_eq!(policy.first_turn.as_deref(), Some("/hello {seat}"));
    }

    /// The same keys, absent — each at the figure or the name the constant
    /// above it carries, read from that constant rather than from a second copy
    /// of the number; and the three the agent declares instead — the model,
    /// the first turn and the gate — left for its declaration to answer.
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
        assert_eq!(policy.default_model, None);
        assert_eq!(policy.posture, DEFAULT_POSTURE);
        assert_eq!(policy.transient_posture, DEFAULT_TRANSIENT_POSTURE);
        assert_eq!(policy.auto_capable_models, None);
        assert_eq!(policy.first_turn, None);
        assert_eq!(policy.load_ceiling_per_cpu, DEFAULT_LOAD_CEILING_PER_CPU);
        assert_eq!(policy.max_transient_busy, DEFAULT_MAX_TRANSIENT_BUSY);
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
             first_turn = \"\"\n",
        )
        .expect("the file parses");
        assert_eq!(zeroed.rest_threshold_tokens, DEFAULT_REST_THRESHOLD_TOKENS);
        assert_eq!(
            zeroed.arrival_window_seconds,
            DEFAULT_ARRIVAL_WINDOW_SECONDS
        );
        assert_eq!(zeroed.start_watch_seconds, DEFAULT_START_WATCH_SECONDS);
        assert_eq!(zeroed.nudge_timeout_seconds, DEFAULT_NUDGE_TIMEOUT_SECONDS);
        assert_eq!(zeroed.default_model, None);
        assert_eq!(zeroed.posture, DEFAULT_POSTURE);
        assert_eq!(zeroed.transient_posture, DEFAULT_TRANSIENT_POSTURE);
        assert_eq!(
            zeroed.auto_capable_models, None,
            "a present but empty list would keep every model out"
        );
        assert_eq!(zeroed.first_turn, None);
    }

    /// What an agent declares, for the arms below: a model, a first turn and
    /// a gate of its own, none of them any constant of this file's.
    fn declared() -> Capabilities {
        Capabilities {
            postures: vec![Posture::Ask, Posture::Auto, Posture::Unattended],
            default_model: "declared-model".to_string(),
            first_turn: "/declared {seat}".to_string(),
            context: true,
            measured: vec!["9.9.9".to_string()],
            posture_models: std::collections::BTreeMap::from([(
                Posture::Auto,
                vec!["declared-".to_string()],
            )]),
        }
    }

    /// The four readings a start is composed from, each with the case that is
    /// not the default beside it — and where the file names none, the agent's
    /// own declaration and never a constant of fleet's.
    #[test]
    fn a_start_is_composed_from_the_seats_row_and_the_fleets_policy() {
        let agent = declared();
        let policy = parse("[controller]\nfirst_turn = \"/wake {seat}\"\n").expect("it parses");
        assert_eq!(
            policy.first_turn_for("builder-9", &agent),
            "/wake builder-9"
        );
        // A template naming the seat twice renders it twice, and one naming it
        // not at all renders as itself — the substitution is textual and says so.
        let twice = parse("[controller]\nfirst_turn = \"{seat}: /wake {seat}\"\n").unwrap();
        assert_eq!(twice.first_turn_for("s1", &agent), "s1: /wake s1");
        let fixed = parse("[controller]\nfirst_turn = \"/orient\"\n").unwrap();
        assert_eq!(fixed.first_turn_for("s1", &agent), "/orient");
        let unnamed = parse("").unwrap();
        assert_eq!(
            unnamed.first_turn_for("s1", &agent),
            "/declared s1",
            "a file that names no template takes the agent's"
        );

        assert_eq!(policy.posture_for(false), DEFAULT_POSTURE);
        assert_eq!(policy.posture_for(true), DEFAULT_TRANSIENT_POSTURE);

        assert_eq!(policy.model_for(Some("a-model"), &agent), "a-model");
        assert_eq!(policy.model_for(None, &agent), "declared-model");
        assert_eq!(
            policy.model_for(Some("   "), &agent),
            "declared-model",
            "a blank row-level model is no model, not an empty one"
        );
        let named = parse("[controller]\ndefault_model = \"the-fleets\"\n").unwrap();
        assert_eq!(
            named.model_for(None, &agent),
            "the-fleets",
            "the fleet's own default outranks the agent's"
        );
    }

    /// A posture key reads fleet's three words, trimmed, and a blank is the
    /// default the way a blank model is.
    #[test]
    fn a_posture_key_reads_fleets_three_words() {
        for posture in Posture::ALL {
            let policy = parse(&format!(
                "[controller]\nposture = \" {0} \"\ntransient_posture = \"{0}\"\n",
                posture.word()
            ))
            .expect("the file parses");
            assert_eq!(
                (policy.posture, policy.transient_posture),
                (posture, posture)
            );
        }
    }

    /// EVERY OTHER WORD IS REFUSED AT LOAD (ruling 14, amended by 17): an
    /// agent's own mode names, the two this file once defaulted to among
    /// them, read as nothing, and no alias turns one into a posture. The refusal names the
    /// key, the word and the three it can use, and it comes back through
    /// [`load`], which is what keeps a running loop on its last-good policy
    /// and refuses a start.
    #[test]
    fn a_posture_key_naming_any_other_word_is_refused_at_load_naming_the_three() {
        for word in [
            "default",
            "dontAsk",
            "acceptEdits",
            "bypassPermissions",
            "plan",
            "manual",
        ] {
            for key in ["posture", "transient_posture"] {
                let refused = parse(&format!("[controller]\n{key} = \"{word}\"\n"))
                    .expect_err("a word that is no posture is refused");
                assert!(refused.contains(&format!("`{key}`")), "{refused}");
                assert!(refused.contains(&format!("`{word}`")), "{refused}");
                for three in ["`ask`", "`auto`", "`unattended`"] {
                    assert!(refused.contains(three), "{refused}");
                }
            }
        }

        let dir = std::env::temp_dir().join(format!("fleet-policy-posture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory is made");
        let file = dir.join("fleet.toml");
        std::fs::write(&file, "[controller]\nposture = \"bypassPermissions\"\n")
            .expect("the file is written");
        let refused = load(&file).expect_err("the loader refuses it too");
        assert!(refused.contains("bypassPermissions"), "{refused}");
        assert!(
            refused.contains("`ask`, `auto` or `unattended`"),
            "{refused}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One read answers the policy `parse` answers and the whole file as a
    /// table from the same bytes; a file that will not parse answers the very
    /// text `load` answers.
    #[test]
    fn one_read_answers_the_policy_and_the_table_from_the_same_bytes() {
        let dir = std::env::temp_dir().join(format!("fleet-policy-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory is made");
        let file = dir.join("fleet.toml");

        let body =
            "[controller]\npoll_seconds = 7\n\n[seats.pell]\nkind = \"agent\"\nname = \"Pell\"\n";
        std::fs::write(&file, body).expect("the file is written");
        let (policy, table) = load_with_table(&file).expect("the file reads");
        assert_eq!(policy, parse(body).expect("the body parses"));
        assert_eq!(policy.poll_seconds, 7);
        let seats = table
            .get("seats")
            .and_then(toml::Value::as_table)
            .expect("the table carries the seats");
        assert!(seats.contains_key("pell"), "{seats:?}");

        std::fs::write(&file, "[controller\n").expect("the file is written");
        let refused = load_with_table(&file).expect_err("a broken file does not read");
        assert_eq!(refused, load(&file).expect_err("nor does it load"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The machine's own file answers a posture per key, as every other key:
    /// a word that is no posture is refused and named with the three, and
    /// loses itself alone — the policy's posture stands and the machine's
    /// other answers still land.
    #[test]
    fn a_machines_posture_naming_any_other_word_is_refused_and_named_and_the_policys_stands() {
        let file = parse("[controller]\nposture = \"ask\"\n").expect("the policy parses");
        let over = overrides_in(Some(&serde_json::json!({
            "posture": "dontAsk",
            "transient_posture": "acceptEdits",
            "poll_seconds": 11,
        })));
        assert_eq!(over.refused.len(), 2, "{:?}", over.refused);
        let said = over.ignored();
        for (key, word) in [("posture", "dontAsk"), ("transient_posture", "acceptEdits")] {
            let line = said
                .iter()
                .find(|line| line.contains(&format!("`controller.{key}`")))
                .unwrap_or_else(|| panic!("{key} is named: {said:?}"));
            assert!(line.contains(&format!("`{word}`")), "{line}");
            assert!(line.contains("`ask`, `auto` or `unattended`"), "{line}");
        }
        let effective = file.overlaid(&over);
        assert_eq!(effective.posture, Posture::Ask);
        assert_eq!(effective.transient_posture, DEFAULT_TRANSIENT_POSTURE);
        assert_eq!(effective.poll_seconds, 11, "the other answer still lands");

        // The control: a posture that is one of the three is the machine's.
        let good = overrides_in(Some(&serde_json::json!({ "posture": "unattended" })));
        assert!(good.refused.is_empty());
        assert_eq!(file.overlaid(&good).posture, Posture::Unattended);
    }

    /// The D3 gate is the file's list for `auto` where it names one, and the
    /// agent's declared prefixes otherwise.
    #[test]
    fn the_gate_is_the_files_list_for_auto_and_falls_to_the_agent() {
        let agent = declared();
        let unnamed = parse("").unwrap();
        assert_eq!(
            unnamed.gate_for(false, &agent),
            vec!["declared-".to_string()]
        );
        assert!(unnamed.posture_is_ungranted(false, "another-model", &agent));
        assert!(!unnamed.posture_is_ungranted(false, "declared-model-2", &agent));
        assert!(
            !unnamed.posture_is_ungranted(true, "another-model", &agent),
            "the transient posture is held to nothing the agent declares"
        );
        let named = parse("[controller]\nauto_capable_models = [\"the-fleets\"]\n").unwrap();
        assert!(named.posture_is_ungranted(false, "declared-model", &agent));
        assert!(!named.posture_is_ungranted(false, "the-fleets-1", &agent));
    }

    /// Every key the machine may answer for, over a policy that answers
    /// differently — so a key the overlay forgot to wire is a value that stays
    /// at the policy's here.
    #[test]
    fn every_controller_key_can_be_answered_by_the_machines_own_file() {
        let file = parse(
            "[controller]\n\
             poll_seconds = 7\n\
             rest_threshold_tokens = 7\n\
             arrival_window_seconds = 7\n\
             start_watch_seconds = 7\n\
             nudge_timeout_seconds = 7\n\
             default_model = \"from-policy\"\n\
             posture = \"ask\"\n\
             transient_posture = \"auto\"\n\
             auto_capable_models = [\"from-policy\"]\n\
             first_turn = \"from-policy {seat}\"\n\
             load_ceiling_per_cpu = 7.0\n\
             max_transient_busy = 7\n",
        )
        .expect("the policy parses");

        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": 11,
            "rest_threshold_tokens": 11,
            "arrival_window_seconds": 11,
            "start_watch_seconds": 11,
            "nudge_timeout_seconds": 11,
            "default_model": "from-machine",
            "posture": "unattended",
            "transient_posture": "ask",
            "auto_capable_models": ["from-machine"],
            "first_turn": "from-machine {seat}",
            "load_ceiling_per_cpu": 11.0,
            "max_transient_busy": 11,
        })));
        assert!(over.unknown.is_empty(), "{:?}", over.unknown);

        let effective = file.overlaid(&over);
        assert_eq!(effective.poll_seconds, 11);
        assert_eq!(effective.rest_threshold_tokens, 11);
        assert_eq!(effective.arrival_window_seconds, 11);
        assert_eq!(effective.start_watch_seconds, 11);
        assert_eq!(effective.nudge_timeout_seconds, 11);
        assert_eq!(effective.default_model.as_deref(), Some("from-machine"));
        assert_eq!(effective.posture, Posture::Unattended);
        assert_eq!(effective.transient_posture, Posture::Ask);
        assert_eq!(
            effective.auto_capable_models,
            Some(vec!["from-machine".to_string()])
        );
        assert_eq!(effective.first_turn.as_deref(), Some("from-machine {seat}"));
        assert_eq!(effective.load_ceiling_per_cpu, 11.0);
        assert_eq!(effective.max_transient_busy, 11);

        // THE CONTROL every assertion above needs: with no object, every one of
        // those keys is the policy's — so the fourteen readings are the
        // machine's answers and not a function that answers 11 regardless.
        let untouched = file.overlaid(&Overrides::default());
        assert_eq!(untouched, file);
        assert_eq!(untouched.poll_seconds, 7);
        assert_eq!(untouched.default_model.as_deref(), Some("from-policy"));
        assert_eq!(
            (untouched.posture, untouched.transient_posture),
            (Posture::Ask, Posture::Auto)
        );
        assert_eq!(
            untouched.auto_capable_models,
            Some(vec!["from-policy".to_string()])
        );

        // Every key in the census is one the object above named, so a key added
        // to `[controller]` and forgotten here is a failure and not a silence.
        assert_eq!(CONTROLLER_KEYS.len(), 12);
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
        assert_eq!(effective.default_model.as_deref(), Some("from-policy"));
        assert_eq!(
            effective.auto_capable_models,
            Some(vec!["from-policy".to_string()])
        );
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
            "[controller]\npoll_seconds = 7\ndefault_model = \"from-policy\"\n\
             rest_threshold_tokens = 77\n",
        )
        .expect("the policy parses");

        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": "30",
            "default_model": "from-machine",
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
            effective.default_model.as_deref(),
            Some("from-machine"),
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
            "default_model": "from-machine",
        }))));
        assert_eq!(good.poll_seconds, 30);
        assert_eq!(good.default_model.as_deref(), Some("from-machine"));
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

    /// ONE GRAMMAR FOR BOTH SOURCES: a file that sets every `[controller]` key
    /// reads to the same policy as the defaults with the same twelve answers
    /// laid over them from `config.json` — so the file's reading and the
    /// overlay's cannot drift apart key by key.
    #[test]
    fn every_controller_key_reads_the_same_from_the_file_and_from_an_overlay() {
        let file = parse(
            "[controller]\n\
             poll_seconds = 9\n\
             rest_threshold_tokens = 111\n\
             arrival_window_seconds = 222\n\
             start_watch_seconds = 333\n\
             nudge_timeout_seconds = 444\n\
             default_model = \" a-model \"\n\
             posture = \"ask\"\n\
             transient_posture = \"auto\"\n\
             auto_capable_models = [\" a-model \", \"\", \"b-model\"]\n\
             first_turn = \"/hello {seat}\"\n\
             load_ceiling_per_cpu = 2.5\n\
             max_transient_busy = 0\n",
        )
        .expect("the file parses");
        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": 9,
            "rest_threshold_tokens": 111,
            "arrival_window_seconds": 222,
            "start_watch_seconds": 333,
            "nudge_timeout_seconds": 444,
            "default_model": " a-model ",
            "posture": "ask",
            "transient_posture": "auto",
            "auto_capable_models": [" a-model ", "", "b-model"],
            "first_turn": "/hello {seat}",
            "load_ceiling_per_cpu": 2.5,
            "max_transient_busy": 0,
        })));
        assert!(over.ignored().is_empty(), "{:?}", over.ignored());
        let defaults = parse("").expect("an empty file parses");
        assert_ne!(file, defaults, "the file moves off the defaults");
        assert_eq!(defaults.overlaid(&over), file);
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error_carrying_why() {
        let err = parse("[controller\npoll_seconds = 5\n").expect_err("broken TOML is refused");
        assert!(!err.is_empty(), "the parse error travels with the refusal");
    }
}
