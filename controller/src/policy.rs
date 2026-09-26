//! `fleet.toml` — policy, re-read on mtime.
//!
//! Startup refuses without it, because there is no last-good before the first
//! read. A running loop that meets a file it cannot parse keeps last-good and
//! says so once per change, never once per poll.

use crate::adapter::{Capabilities, Posture};
use serde::Deserialize;
use std::path::{Path, PathBuf};

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
/// agent's listing shows the pane's own process with a status. Measured on
/// Claude Code 2.1.280 and tmux 3.7b (fleet-rge6.2, 2026-09-26), the row was
/// listed 0.5–0.75 s after the session was made and carried its status at
/// 1.0–1.3 s, so five seconds is four times the slowest reading; short enough
/// that one poll cannot be lost to a start that will never list.
pub const DEFAULT_START_WATCH_SECONDS: u64 = 5;

/// How long a turn typed into a seat's session is given to be taken: the row
/// must read busy inside it (`crate::effect::type_turn`). On Claude Code
/// 2.1.280 a typed turn read busy on the first read after its submit, 0.14 s
/// on (fleet-rge6.5, 2026-09-26), so ten seconds is far past any turn that
/// will be taken, and short enough that a poll is not held long by one that
/// will not.
pub const DEFAULT_NUDGE_TIMEOUT_SECONDS: u64 = 10;

/// The permission posture a named seat's session is started under, and the one a
/// transient row is, in the two words a stored policy carries until fleet-1jr1e
/// makes posture fleet's own word: each converts through [`stored_posture`] as
/// it crosses into a launch.
///
/// The gate below is keyed on the posture and not on the pair: the measured
/// downgrade is of a REQUESTED posture, and the transient posture asks for less
/// than the model's default rather than more.
pub const POSTURE_AUTO: &str = "auto";
pub const DEFAULT_POSTURE: &str = POSTURE_AUTO;
pub const DEFAULT_TRANSIENT_POSTURE: &str = "dontAsk";

/// A stored posture as fleet's own word, through the TWO-WORD READER that
/// stands until fleet-1jr1e stores postures as fleet's words: the two stored
/// defaults, `auto` and `dontAsk`, and nothing else. A word it does not read is
/// `None`, and a launch asked for under it is refused rather than guessed at.
pub fn stored_posture(word: &str) -> Option<Posture> {
    match word.trim() {
        POSTURE_AUTO => Some(Posture::Auto),
        DEFAULT_TRANSIENT_POSTURE => Some(Posture::Unattended),
        _ => None,
    }
}

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
/// number of times from the fleet the policy describes. The figure is pinned
/// against the run lifecycle's own default in the binary's suite, which is the
/// one member that can see both crates.
pub const DEFAULT_RUN_MAX_CRASHES: u64 = 2;

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
    pub posture: String,
    pub transient_posture: String,
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
    /// The release this fleet's own file pins under `[substrate]`, as written.
    /// `None` when the file pins nothing, and the expectation is then the
    /// release fleet supports — read through [`Policy::claude_code_expected`],
    /// never off this field.
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
    /// controller is what counts a run's crashes and parks at the cap.
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

/// A configured string, or `None` where it is blank or absent: the key's
/// answer is then another's to give, and a blank never reaches a start.
fn stated(configured: Option<&str>) -> Option<String> {
    configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
    let c = controller.as_ref();
    // A list that is present and empty is refused too: it would gate every
    // model out and leave a fleet that can start nothing under posture `auto`,
    // which is a configuration nobody writes on purpose.
    let auto_capable_models = models(c.and_then(|c| c.auto_capable_models.clone()));
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
        default_model: stated(c.and_then(|c| c.default_model.as_deref())),
        posture: named(c.and_then(|c| c.posture.as_deref()), DEFAULT_POSTURE),
        transient_posture: named(
            c.and_then(|c| c.transient_posture.as_deref()),
            DEFAULT_TRANSIENT_POSTURE,
        ),
        auto_capable_models,
        first_turn: stated(c.and_then(|c| c.first_turn.as_deref())),
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
    pub fn posture_for(&self, transient: bool) -> &str {
        if transient {
            &self.transient_posture
        } else {
            &self.posture
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
        let Some(posture) = stored_posture(self.posture_for(transient)) else {
            return Vec::new();
        };
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

    /// The agent release the live one is compared with: the fleet's own
    /// `[substrate]` pin where the file writes one, and otherwise the release
    /// the agent's adapter declares it was measured against. A fleet that pins
    /// nothing is not a fleet that expects nothing, so a spread there is "not
    /// the release fleet supports" and announced the same way.
    pub fn claude_code_expected(&self, agent: &Capabilities) -> String {
        named(
            self.claude_code_pin.as_deref(),
            agent.measured.first().map(String::as_str).unwrap_or(""),
        )
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
        let Some(c) = over.controller.as_ref() else {
            return self.clone();
        };
        Policy {
            poll_seconds: positive(c.poll_seconds, self.poll_seconds),
            rest_threshold_tokens: positive(c.rest_threshold_tokens, self.rest_threshold_tokens),
            arrival_window_seconds: positive(c.arrival_window_seconds, self.arrival_window_seconds),
            start_watch_seconds: positive(c.start_watch_seconds, self.start_watch_seconds),
            nudge_timeout_seconds: positive(c.nudge_timeout_seconds, self.nudge_timeout_seconds),
            default_model: stated(c.default_model.as_deref()).or(self.default_model.clone()),
            posture: named(c.posture.as_deref(), &self.posture),
            transient_posture: named(c.transient_posture.as_deref(), &self.transient_posture),
            auto_capable_models: models(c.auto_capable_models.clone())
                .or(self.auto_capable_models.clone()),
            first_turn: stated(c.first_turn.as_deref()).or(self.first_turn.clone()),
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
        assert_eq!(policy.posture, "acceptEdits");
        assert_eq!(policy.transient_posture, "bypassPermissions");
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

    /// The two stored words and no other cross as fleet's own (the two-word
    /// reader fleet-1jr1e replaces), and the D3 gate is the file's list for
    /// `auto` where it names one, and the agent's declared prefixes otherwise.
    #[test]
    fn a_stored_posture_crosses_as_fleets_word_and_the_gate_falls_to_the_agent() {
        assert_eq!(stored_posture("auto"), Some(Posture::Auto));
        assert_eq!(stored_posture("dontAsk"), Some(Posture::Unattended));
        for other in [
            "default",
            "acceptEdits",
            "bypassPermissions",
            "plan",
            "ask",
            "",
        ] {
            assert_eq!(stored_posture(other), None, "{other:?}");
        }

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
             posture = \"from-policy\"\n\
             transient_posture = \"from-policy\"\n\
             auto_capable_models = [\"from-policy\"]\n\
             first_turn = \"from-policy {seat}\"\n\
             load_ceiling_per_cpu = 7.0\n\
             max_transient_busy = 7\n\
             \n[substrate.claude_code]\nversion = \"2.1.261\"\n",
        )
        .expect("the policy parses");

        let over = overrides_in(Some(&serde_json::json!({
            "poll_seconds": 11,
            "rest_threshold_tokens": 11,
            "arrival_window_seconds": 11,
            "start_watch_seconds": 11,
            "nudge_timeout_seconds": 11,
            "default_model": "from-machine",
            "posture": "from-machine",
            "transient_posture": "from-machine",
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
        assert_eq!(effective.posture, "from-machine");
        assert_eq!(effective.transient_posture, "from-machine");
        assert_eq!(
            effective.auto_capable_models,
            Some(vec!["from-machine".to_string()])
        );
        assert_eq!(effective.first_turn.as_deref(), Some("from-machine {seat}"));
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
        assert_eq!(untouched.default_model.as_deref(), Some("from-policy"));
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
    fn the_pin_reads_from_a_table_or_from_a_flat_string() {
        let table = parse("[substrate.claude_code]\nversion = \"2.1.261\"\n").unwrap();
        assert_eq!(table.claude_code_pin.as_deref(), Some("2.1.261"));
        let flat = parse("[substrate]\nclaude_code = \"2.1.261\"\n").unwrap();
        assert_eq!(flat.claude_code_pin.as_deref(), Some("2.1.261"));
    }

    /// fleet-2jt: a file that pins nothing expects the release fleet supports,
    /// and a pin of its own wins over it. The pin is a release the constant is
    /// not, so the second half reads the file's and not a coincidence.
    #[test]
    fn a_file_that_pins_nothing_expects_the_release_its_agent_was_measured_against() {
        let policy = parse("[controller]\npoll_seconds = 9\n").unwrap();
        assert_eq!(policy.claude_code_pin, None);
        assert_eq!(policy.claude_code_expected(&declared()), "9.9.9");
        assert_eq!(policy.poll_seconds, 9);

        let pinned = parse("[substrate]\nclaude_code = \"0.0.1\"\n").unwrap();
        assert_ne!(declared().measured[0], "0.0.1");
        assert_eq!(pinned.claude_code_expected(&declared()), "0.0.1");
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
