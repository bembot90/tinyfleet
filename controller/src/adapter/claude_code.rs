//! The Claude Code adapter — the in-process answer to the agent contract's six
//! verbs, and the one file in the controller that knows Claude Code: its
//! flags, its listing, its transcripts, its configuration directory and its
//! permission file. Everything it answers crosses as the contract's own types.

use super::{
    dir_key, Activity, Agent, AgentError, Argv, BlockedOn, Capabilities, Evidence, Launch, Opened,
    Opening, Permissions, Posture, Resume, SeatActivity, SeatContext, SeatRef, Version,
    FLEET_BIN_VAR, PASSED_THROUGH,
};
use crate::platform::run_bounded;
use fleet_core::store::types::Stamp;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

#[derive(Clone)]
pub struct ClaudeCode {
    pub bin: String,
    /// Where this agent keeps its own state, which is where the transcripts are.
    pub config_dir: PathBuf,
    pub timeout: Duration,
    /// The `PATH` every child this adapter spawns for its own reads carries,
    /// constructed by the platform layer and never inherited (D1).
    pub child_path: String,
    /// The credential scope every child carries, which is the CONFIGURED
    /// config-directory value — trimmed, or empty when nothing configured one —
    /// and never the resolved directory `config_dir` holds.
    ///
    /// The credential item's service name takes a hash of the config-directory
    /// input, so a child pointed at any directory, the home default included,
    /// looks up a credential nobody wrote (lessons claude-code A11). This value
    /// is that input as the operator's own login left it: empty restores the
    /// unsuffixed name, and a directory here is a third, different credential.
    pub credential_dir: String,
    /// The ABSOLUTE binary a launch and a resume name as the pane's process,
    /// resolved once by [`open`] against the constructed `PATH` and handed in.
    ///
    /// Separate from `bin`, which is the reads' and may be a bare name: the two
    /// answer different questions, and a controller that gated on one and
    /// exec'd the other would act through a binary nothing checked. `None` is a
    /// binary that did not resolve, which is effects OFF — `launch` and
    /// `resume` refuse rather than falling back to a name the host would
    /// discover at the spawn.
    effect_bin: Option<PathBuf>,
    /// This process's own executable, which every child of this adapter's own
    /// reads is handed as [`FLEET_BIN_VAR`], as every seat's session is.
    fleet_bin: Option<PathBuf>,
    /// The plugin root every session this adapter launches or resumes loads,
    /// or `None` for a fleet that names none — only a loaded plugin root gives
    /// the session the overlay's hooks and the root's bin on its `PATH`
    /// (lessons claude-code D5). The adapter's own (reviewer call 2026-09-25,
    /// E8): handed in at [`open`] and never a field of a request.
    plugin_dir: Option<PathBuf>,
    /// The template a launch renders the request's permissions into, handed in
    /// at [`open`], or `None` for an adapter whose launches write none.
    permissions: Option<String>,
}

/// The deadline every call to the agent binary runs on when nothing sets one.
/// Far above any healthy answer, and far below the interval at which a person
/// would call the poll hung.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// The agent binary when nothing names one: a bare name, resolved on `PATH`.
pub const DEFAULT_BIN: &str = "claude";

/// The name this adapter answers to: the one fleet-packs' pack carries it
/// under, `adapters/agent/claude-code/`.
pub const NAME: &str = "claude-code";

/// The agent this adapter drives, as `version` names it: the `claude` program.
pub const AGENT: &str = "claude";

/// This agent's pre-tool hook, as the `[hook]` table an agent adapter's
/// `adapter.toml` declares it: what `fleet guard` reads where no installed
/// pack carries [`NAME`]. It lives beside the in-process adapter, and goes
/// with it.
pub const HOOK_MANIFEST: &str = include_str!("claude_code.hook.toml");

/// The model a seat whose policy names none is launched on. A launch always
/// names one, because a start with no model flag comes up on the cheapest
/// available one (lessons claude-code A5).
pub const DEFAULT_MODEL: &str = "claude-opus-5";

/// A session's first turn, with `{seat}` the seat's session name. The wake
/// rides the launch: one act, one channel, so the instruction cannot be lost
/// without also losing the session.
pub const DEFAULT_FIRST_TURN: &str = "/wake {seat}";

/// The models measured to honour a requested `auto`, matched BY PREFIX: live
/// model ids carry suffixes that name the same model, one dated and one
/// windowed (lessons claude-code D3). A model outside them is started in the
/// agent's default mode and says so only on screen.
pub const AUTO_CAPABLE_MODELS: [&str; 3] = ["claude-opus-5", "claude-fable-5", "claude-sonnet-5"];

/// What this adapter declares about Claude Code: the three postures, the
/// model and first turn a seat that names none starts with, `context`
/// answered from the transcript, the release it was measured against — the
/// one fleet supports — and `auto` held to the models measured to honour it.
pub fn capabilities() -> Capabilities {
    Capabilities {
        postures: vec![Posture::Ask, Posture::Auto, Posture::Unattended],
        default_model: DEFAULT_MODEL.to_string(),
        first_turn: DEFAULT_FIRST_TURN.to_string(),
        context: true,
        measured: vec![fleet_core::supported::PINNED_CLAUDE_CODE.to_string()],
        posture_models: BTreeMap::from([(
            Posture::Auto,
            AUTO_CAPABLE_MODELS.iter().map(|m| m.to_string()).collect(),
        )]),
    }
}

/// A posture as Claude Code's `--permission-mode` spells it (ruling 14): `ask`
/// is its default mode, `auto` its own, and `unattended` the mode that never
/// stops to ask.
pub fn permission_mode(posture: Posture) -> &'static str {
    match posture {
        Posture::Ask => "default",
        Posture::Auto => "auto",
        Posture::Unattended => "dontAsk",
    }
}

impl ClaudeCode {
    pub fn new(home: &Path) -> Self {
        Self {
            bin: bin_from(configured_bin().as_deref()),
            config_dir: config_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref(), home),
            timeout: timeout_from(std::env::var("FLEET_AGENT_TIMEOUT_MS").ok().as_deref()),
            child_path: crate::platform::child_path(home),
            credential_dir: credential_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref()),
            // Reads alone until [`open`] hands one in: an adapter built for
            // reading issues no effect, and a `None` here refuses both effect
            // verbs rather than letting one fall back to `bin`.
            effect_bin: None,
            fleet_bin: super::own_executable(),
            plugin_dir: None,
            permissions: None,
        }
    }

    /// The adapter with the binary its launches and resumes name.
    ///
    /// Handed in rather than resolved here, so the ONE resolution [`open`]
    /// made — the one whose failure turns effects off and whose cause the
    /// projection publishes — is the same value the two verbs name.
    pub fn with_effect_bin(mut self, bin: PathBuf) -> Self {
        self.effect_bin = Some(bin);
        self
    }

    /// The adapter with the plugin root every session it launches or resumes
    /// loads.
    pub fn with_plugin_dir(mut self, plugin_dir: Option<PathBuf>) -> Self {
        self.plugin_dir = plugin_dir;
        self
    }

    /// The adapter with the template its launches render a request's
    /// permissions into.
    pub fn with_permissions(mut self, template: Option<String>) -> Self {
        self.permissions = template;
        self
    }

    /// An adapter whose seams are all handed in, for a caller that has resolved
    /// them itself. `new` reads the process environment, which a suite running
    /// arms in parallel shares and cannot vary per arm.
    pub fn with_seams(
        bin: String,
        config_dir: PathBuf,
        timeout: Duration,
        child_path: String,
        effect_bin: Option<PathBuf>,
        credential_dir: String,
    ) -> Self {
        Self {
            bin,
            config_dir,
            timeout,
            child_path,
            credential_dir,
            effect_bin,
            fleet_bin: super::own_executable(),
            plugin_dir: None,
            permissions: None,
        }
    }

    /// A command against a program that has already been chosen, with
    /// [`ClaudeCode::environment`] on it and nothing else.
    fn with_environment(&self, program: &str, config_dir: Option<&Path>) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for (key, value) in self.environment(config_dir) {
            cmd.env(key, value);
        }
        cmd
    }

    /// The whole environment a child of this adapter's own reads carries — the
    /// listing's and the version's — and the three variables this adapter
    /// answers into a session's own: the agent's configuration directory, the
    /// credential scope beside it and the auto-updater off. So the call that
    /// reads a listing and the pane that holds the session it lists agree on
    /// every value this adapter sets.
    ///
    /// NOTHING IS INHERITED (lessons claude-code D1): the constructed `PATH`,
    /// the four values a shell needs, the agent's own variables, and this
    /// process's own executable as the binary the plugin's hooks run.
    ///
    /// `config_dir` is the ONE per-child override: a read about a session that
    /// came up under its own configuration directory is made under that one.
    pub fn environment(&self, config_dir: Option<&Path>) -> Vec<(String, String)> {
        let mut env = vec![("PATH".to_string(), self.child_path.clone())];
        for pass in PASSED_THROUGH {
            if let Ok(value) = std::env::var(pass) {
                env.push((pass.to_string(), value));
            }
        }
        env.extend(self.agent_environment(config_dir.map(|dir| dir.display().to_string())));
        if let Some(bin) = &self.fleet_bin {
            env.push((FLEET_BIN_VAR.to_string(), bin.display().to_string()));
        }
        env
    }

    /// The variables this adapter adds for its own agent, in a session's pane
    /// and in its own reads alike.
    ///
    /// `CLAUDE_CONFIG_DIR` is the start's own directory where it names one and
    /// the adapter's otherwise: a directory here is the session's whole
    /// configuration space, and it scopes the agent's listing with it (A11).
    ///
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR` is set on EVERY child, defined even
    /// when empty: unset falls back to the suffixed credential lookup, which is
    /// the logged-out child.
    ///
    /// `DISABLE_AUTOUPDATER` is set on EVERY child, the pane's process above
    /// all: an interactive session runs the agent's auto-updater, which
    /// installed a newer release and re-pointed the operator's own `claude`
    /// under them during fleet-rge6.2's measurement (2026-09-26). A seat must
    /// not move the pin under the person (lessons claude-code A1).
    fn agent_environment(&self, config_dir: Option<String>) -> Vec<(String, String)> {
        vec![
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                config_dir.unwrap_or_else(|| self.config_dir.display().to_string()),
            ),
            (
                "CLAUDE_SECURESTORAGE_CONFIG_DIR".to_string(),
                self.credential_dir.clone(),
            ),
            (AUTOUPDATER_VAR.to_string(), "1".to_string()),
        ]
    }

    /// The two READS — the listing and the version — against `bin`, which may be
    /// a bare name, RESOLVED ON THIS PROCESS'S OWN SEARCH PATH.
    ///
    /// Which file to read from is the operator's question, stated by the seam or
    /// by the path this controller was started under; what the child then hands
    /// its own descendants is the fleet's, and that is the constructed `PATH`
    /// the environment above carries either way. A name that resolves nowhere is
    /// passed through unchanged, so the spawn's own cause still names it.
    fn read_command(&self, program: &str, config_dir: Option<&Path>) -> Command {
        let resolved = std::env::var("PATH")
            .ok()
            .and_then(|path| crate::platform::resolve_on_path(&path, program))
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| program.to_string());
        self.with_environment(&resolved, config_dir)
    }

    /// The listing's own output under one configuration directory — the one
    /// command [`Agent::read`] and [`ClaudeCode::daemon_hosted`] both read —
    /// or why it gave none.
    fn listing(&self, config_dir: Option<&Path>) -> Result<String, String> {
        let mut cmd = self.read_command(&self.bin, config_dir);
        cmd.args(["agents", "--json", "--all"]);
        let run = run_bounded(cmd, self.timeout)?;
        if !run.status.success() {
            return Err(format!(
                "`{} agents --json --all` exited {}: {}",
                self.bin,
                run.status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "on a signal".to_string()),
                String::from_utf8_lossy(&run.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&run.stdout).into_owned())
    }

    /// The live sessions Claude Code's background daemon still hosts under one
    /// configuration directory, or the adapter's own where `config_dir` is
    /// `None` — what the upgrade refusal reads before a controller's first
    /// poll (ruling 10: the upgrade adopts nothing).
    ///
    /// ON THE CONCRETE TYPE AND NOT ON [`Agent`] (reviewer call 2026-09-25,
    /// E6): the contract carries no daemon verb (ruling 1), and this goes with
    /// the file that holds it. The rows are read through this adapter's own
    /// private row type ([`parse_hosted`]), so the daemon's address never
    /// reaches a caller as a field: a caller gets the sentence naming it and
    /// the command that stops it, from [`Hosted`].
    pub fn daemon_hosted(&self, config_dir: Option<&Path>) -> Result<Vec<Hosted>, String> {
        self.listing(config_dir)
            .and_then(|stdout| parse_hosted(&stdout))
    }

    /// The binary a session's pane runs, or the refusal a verb with none
    /// resolved answers: effects off, and never a bare name the host would
    /// discover at the spawn.
    fn session_bin(&self) -> Result<String, AgentError> {
        self.effect_bin
            .as_ref()
            .map(|bin| bin.display().to_string())
            .ok_or_else(|| {
                AgentError::Unreadable(
                    "no agent binary is resolved for effects, so this call issues nothing"
                        .to_string(),
                )
            })
    }

    /// The model, the posture and the plugin root, as the flags a launch and a
    /// resume both carry (lessons claude-code A5, D3, D5): only a loaded plugin
    /// root gives the session the overlay's hooks and the root's bin on its
    /// `PATH`, and a fleet that names none passes no such element.
    fn start_flags(&self, model: &str, posture: Posture) -> Vec<String> {
        let mut flags = vec![
            "--model".to_string(),
            model.to_string(),
            "--permission-mode".to_string(),
            permission_mode(posture).to_string(),
        ];
        if let Some(plugin_dir) = &self.plugin_dir {
            flags.push("--plugin-dir".to_string());
            flags.push(plugin_dir.display().to_string());
        }
        flags
    }

    /// The directory a seat's reads are made under: its own, where the request
    /// names one, and the adapter's otherwise.
    fn dir_of(&self, config_dir: Option<&str>) -> PathBuf {
        config_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| self.config_dir.clone())
    }
}

/// The in-process adapter opened for a caller: built from this process's
/// environment, with the binary its effects exec resolved ONCE against the
/// constructed `PATH`. Unresolvable is not an error: the opened agent reads
/// all the same, and its gate carries the cause.
pub(super) fn open(opening: &Opening) -> Opened {
    let mut agent = ClaudeCode::new(opening.home)
        .with_plugin_dir(opening.plugin_dir.clone())
        .with_permissions(opening.permissions.clone());
    // The binary an EFFECT execs, resolved once and never by bare name.
    let effects_off = match resolve_effect_bin(configured_bin().as_deref(), &agent.child_path) {
        Ok(bin) => {
            agent = agent.with_effect_bin(bin);
            None
        }
        Err(cause) => Some(cause),
    };
    Opened {
        name: NAME.to_string(),
        daemon: Some(Box::new(agent.clone())),
        agent: Box::new(agent),
        effects_off,
    }
}

/// The binary an effect execs, resolved once.
///
/// `FLEET_CLAUDE_BIN` when it names an ABSOLUTE path, and a relative one is
/// refused rather than resolved: a relative path is read against whatever
/// directory the service manager left this process in, which is a different
/// file per host. Otherwise the first `claude` on the constructed PATH.
/// `Err` is a binary this controller cannot exec, which turns effects off and
/// never becomes a bare name it discovers at the spawn.
fn resolve_effect_bin(configured: Option<&str>, child_path: &str) -> Result<PathBuf, String> {
    match configured.map(str::trim) {
        Some(bin) if !bin.is_empty() => {
            let named = PathBuf::from(bin);
            if !named.is_absolute() {
                return Err(format!(
                    "the agent binary seam names `{bin}`, which is not an absolute path"
                ));
            }
            match crate::platform::resolve_on_path(child_path, bin) {
                Some(path) => Ok(path),
                None => Err(format!(
                    "the agent binary seam names `{bin}`, which is not an executable file"
                )),
            }
        }
        _ => crate::platform::resolve_on_path(child_path, DEFAULT_BIN).ok_or_else(|| {
            format!("no `{DEFAULT_BIN}` on the constructed child PATH ({child_path})")
        }),
    }
}

/// The binary the adapter runs, with the environment passed in, so the order is
/// tested without touching it.
///
/// A blank setting is the default, like its two siblings below: an empty program
/// spawns as `could not start : No such file or directory`, a cause that names
/// no binary to the operator who has to act on it.
///
/// IT TRIMS, AND THAT IS A REWRITE, not only a blank test: a configured path
/// with a space at either edge is run without it, so a file genuinely named
/// with an edge space is unreachable through this seam and the cause names a
/// path nobody configured. Admitted rather than refused because the blank
/// spellings this normalises are the common case and an edge-spaced binary path
/// is not one anybody here has met; a reading of one would be the reason to
/// refuse instead.
pub fn bin_from(configured: Option<&str>) -> String {
    match configured {
        Some(bin) if !bin.trim().is_empty() => bin.trim().to_string(),
        _ => DEFAULT_BIN.to_string(),
    }
}

/// The variable naming the agent binary.
pub const CLAUDE_BIN_VAR: &str = "FLEET_CLAUDE_BIN";

/// The variable that turns the agent's own auto-updater off, set to `1` on
/// every child this adapter makes.
pub const AUTOUPDATER_VAR: &str = "DISABLE_AUTOUPDATER";

/// Set, `CLAUDE_BIN_VAR` naming nothing is a refusal rather than a fall back.
pub const HERMETIC_VAR: &str = "FLEET_TEST_HERMETIC";

/// The one read of `FLEET_CLAUDE_BIN` in this workspace, and every resolution
/// of the agent binary goes through it.
///
/// Under `FLEET_TEST_HERMETIC` an unnamed binary is a REFUSAL and not the
/// `DEFAULT_BIN` fallback the two resolvers would take: a suite run inside a
/// flight this fleet is flying inherits a live environment, and an arm that
/// missed its stub would otherwise exec the account's own agent. The stop is
/// taken here rather than returned because both callers' error paths are
/// themselves fallbacks — one turns effects off and carries on — so a returned
/// refusal would be swallowed by the very thing it exists to deny.
pub(crate) fn configured_bin() -> Option<String> {
    let configured = std::env::var(CLAUDE_BIN_VAR).ok();
    let names_nothing = configured.as_deref().unwrap_or("").trim().is_empty();
    if names_nothing && hermetic() {
        eprintln!(
            "fleet: {HERMETIC_VAR} is set and {CLAUDE_BIN_VAR} names no agent binary — \
             refusing to fall back to a `{DEFAULT_BIN}` on PATH"
        );
        std::process::exit(2);
    }
    configured
}

pub(crate) fn hermetic() -> bool {
    match std::env::var(HERMETIC_VAR) {
        Ok(value) => !matches!(value.trim(), "" | "0"),
        Err(_) => false,
    }
}

/// The deadline the binary actually runs on, with the environment passed in, so
/// a suite can shorten what the built binary waits for and the order is tested
/// without touching it.
///
/// Anything that is not a positive whole number of milliseconds is the default.
/// The guard is against a reading that is no deadline at all — a typo, a
/// negative, a zero, which would kill every listing before it could answer and
/// publish a whole fleet as Unknown. It is not a floor on the value read back:
/// a seam set to 1 ms returns 1 ms. What that deadline then costs in elapsed
/// time is `run_bounded`'s, which reads `try_wait` every 20 ms and so cuts a
/// call off no finer than that poll; a deadline too short to answer inside is
/// the operator's either way.
pub fn timeout_from(configured: Option<&str>) -> Duration {
    match configured
        .map(str::trim)
        .and_then(|ms| ms.parse::<u64>().ok())
    {
        Some(ms) if ms > 0 => Duration::from_millis(ms),
        _ => DEFAULT_TIMEOUT,
    }
}

/// The agent's configuration directory scopes its listing and its transcripts
/// (lessons claude-code A11), so a fleet whose agent runs under a named one and
/// a reader that assumes the default read different machines.
///
/// Pure, with the environment passed in, so the order is tested without
/// touching it.
pub fn config_dir_from(configured: Option<&str>, home: &Path) -> PathBuf {
    match configured {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir.trim()),
        _ => home.join(".claude"),
    }
}

/// The credential scope a child carries: the same setting its sibling above
/// reads, taken as the operator gave it and NOT resolved to a directory.
///
/// The empty string is a value and not an absence — defined-but-empty is what
/// restores the unsuffixed service name a person's own login wrote, while an
/// unset variable leaves the lookup suffixed by whatever the config directory
/// is (lessons claude-code A11). So the unconfigured reading here is empty
/// rather than the home default, which is the one place this reader and
/// `config_dir_from` answer differently on the same input.
///
/// Pure, with the environment passed in, so the order is tested without
/// touching it.
pub fn credential_dir_from(configured: Option<&str>) -> String {
    match configured {
        Some(dir) if !dir.trim().is_empty() => dir.trim().to_string(),
        _ => String::new(),
    }
}

impl Agent for ClaudeCode {
    fn capabilities(&self) -> Result<Capabilities, AgentError> {
        Ok(capabilities())
    }

    /// `claude --version`, read this call. A binary that will not answer is a
    /// version nobody has — `null`, the agent not there to say — and never an
    /// error: the adapter itself answered.
    fn version(&self) -> Result<Version, AgentError> {
        let mut cmd = self.read_command(&self.bin, None);
        cmd.arg("--version");
        let version = run_bounded(cmd, self.timeout)
            .ok()
            .filter(|run| run.status.success())
            .and_then(|run| parse_version(&String::from_utf8_lossy(&run.stdout)));
        Ok(Version {
            name: AGENT.to_string(),
            version,
        })
    }

    /// The resolved binary, the name, the model, the posture and the plugin
    /// root, then the first turn as the positional prompt (lessons claude-code
    /// A5, D3, D5) — an INTERACTIVE session, with no `--bg`: the pane is the
    /// session's host, and the agent hosts nothing. The positional turn submits
    /// on its own (lessons claude-code D8, measured on 2.1.280).
    ///
    /// Two writes come first, each inside what the request names and nowhere
    /// else (E8). A launch under its own configuration directory SEEDS it
    /// ([`seed_config`]): that directory begins empty but for the overlay, and
    /// an interactive session under an empty one stops at onboarding and then
    /// at the workspace-trust question before any session exists (D8, A15). A
    /// named seat's launch names no directory and is seeded with nothing — its
    /// checkout is a person's, and fleet trusts only worktrees it created
    /// (ruling 13). And where this adapter was opened with a permissions
    /// template, the request's permissions are rendered into the worktree's
    /// own settings file ([`write_settings`]) — before the start, because this
    /// agent reads the file once, as it comes up.
    ///
    /// The environment answered is the request's with this agent's own three
    /// beside it ([`ClaudeCode::agent_environment`]).
    fn launch(&self, launch: &Launch) -> Result<Argv, AgentError> {
        let bin = self.session_bin()?;
        if let Some(dir) = launch.config_dir.as_deref().map(Path::new) {
            seed_config(
                dir,
                &operator_file(&self.credential_dir, &self.config_dir),
                Path::new(&launch.worktree),
            )
            .map_err(AgentError::Unreadable)?;
        }
        if let Some(template) = &self.permissions {
            write_settings(Path::new(&launch.worktree), template, &launch.permissions)
                .map_err(AgentError::Unreadable)?;
        }
        let mut argv = vec![bin, "--name".to_string(), launch.name.clone()];
        argv.extend(self.start_flags(&launch.model, launch.posture));
        argv.push(launch.first_turn.clone());
        let mut env = launch.env.clone();
        env.extend(self.agent_environment(launch.config_dir.clone()));
        Ok(Argv { argv, env })
    }

    /// `--resume <full id>` with the start's own model, posture and plugin
    /// root — and no name and no first turn, because both are the session's
    /// already — under this agent's own variables; fleet sets its own beside
    /// them.
    ///
    /// MEASURED ON 2.1.280 AND TMUX 3.7b (fleet-rge6.4, 2026-09-26): a session
    /// killed after a turn and resumed this way, as a new tmux session, was
    /// listed under the SAME session id on the new pane's pid, under the name
    /// its start gave it, on the model and in the posture the flags named; the
    /// plugin's session-start hook fired with source `resume`, and the next turn
    /// went to the same transcript and remembered the first. A9's fork was a
    /// background session's reading and not this one's, so the flags ride, and
    /// the watch still refuses a session under any other id as a fork.
    ///
    /// The configuration directory is not seeded again: the start seeded it,
    /// and the session resumes where it ran.
    fn resume(&self, resume: &Resume) -> Result<Argv, AgentError> {
        let bin = self.session_bin()?;
        let mut argv = vec![bin, "--resume".to_string(), resume.session_id.clone()];
        argv.extend(self.start_flags(&resume.model, resume.posture));
        Ok(Argv {
            argv,
            env: self
                .agent_environment(resume.config_dir.clone())
                .into_iter()
                .collect(),
        })
    }

    /// One `agents --json --all` per distinct configuration directory the
    /// seats name, each seat found in the listing of its own directory
    /// ([`readings_from`]), and an idle session's first turn read off its
    /// transcript for the logged-out answer (lessons claude-code A11).
    fn read(&self, seats: &[SeatRef]) -> Result<Vec<SeatActivity>, AgentError> {
        Ok(readings_from(
            seats,
            &|dir: Option<&str>| self.listing(dir.map(Path::new)),
            &|seat: &SeatRef, session: &str| {
                std::fs::read_to_string(transcript_path(
                    &self.dir_of(seat.config_dir.as_deref()),
                    dir_key(&seat.worktree),
                    session,
                ))
                .ok()
            },
        ))
    }

    /// Each seat's transcript, under the directory its session was started
    /// with: the window its last main-chain turn carried, the turns it took,
    /// and its last write ([`context_of`]). A seat with no session id, or
    /// whose transcript does not resolve, answers its id alone.
    fn context(&self, seats: &[SeatRef]) -> Result<Vec<SeatContext>, AgentError> {
        Ok(seats
            .iter()
            .map(|seat| {
                let path = seat.session_id.as_deref().map(|session| {
                    transcript_path(
                        &self.dir_of(seat.config_dir.as_deref()),
                        dir_key(&seat.worktree),
                        session,
                    )
                });
                let body = path
                    .as_ref()
                    .and_then(|path| std::fs::read_to_string(path).ok());
                // After the read, so the stamp is no older than the last
                // entry read.
                let written = path
                    .as_ref()
                    .and_then(|path| std::fs::metadata(path).ok())
                    .and_then(|meta| meta.modified().ok());
                context_of(seat, body.as_deref(), written)
            })
            .collect())
    }

    /// Down, then Enter, when the screen is the workspace-trust question: its
    /// default is "No, exit", and the second choice accepts (lessons
    /// claude-code D8, A15).
    fn trust_keys(&self, screen: &str) -> Option<Vec<String>> {
        trust_keys(screen)
    }
}

// ---- the listing --------------------------------------------------------------

/// One session as the listing reports it, as THIS adapter reads it: private,
/// so no row, no short id and no listing word leaves the adapter (reviewer call
/// 2026-09-25, E6). Every field of the agent's row it does not name is read
/// past (lessons claude-code B1).
///
/// There is deliberately no token field: the listing carries none (lessons
/// claude-code B2), and context comes from the transcript.
#[derive(Clone, Debug, Deserialize)]
struct Row {
    #[serde(rename = "sessionId")]
    session_id: String,
    /// The session's process. An interactive row always carries it, and it is
    /// what a row is attributed to a seat by where its session id does not:
    /// the pane's own pid (E2).
    #[serde(default)]
    pid: Option<u32>,
    /// What the agent says this session is DOING right now, in its own
    /// vocabulary: `idle`, `busy` or `waiting` on an interactive row (B10), and
    /// absent in the half second after a row is first listed.
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "startedAt", default)]
    started_at: Option<u64>,
    /// Present ONLY while the session is stopped in front of a human, and its
    /// value names the cause (lessons claude-code B8). Read for PRESENCE: a
    /// cause this adapter does not recognise still blocks the seat.
    #[serde(rename = "waitingFor", default)]
    waiting_for: Option<String>,
}

/// The agent's own word for a session that is mid-turn.
const BUSY: &str = "busy";

/// The agent's own word for a session at its prompt.
const IDLE: &str = "idle";

/// The agent's word for a session stopped in front of a human, which an
/// interactive row carries beside its `waitingFor` (lessons claude-code B10).
/// Read as a block on its own too, so a row that names no cause still blocks.
const WAITING: &str = "waiting";

/// The one `waitingFor` value recorded (on 2.1.280, 2026-09-26), and the
/// block it names.
const PERMISSION_PROMPT: &str = "permission prompt";

/// The listing's answer, or why it is not one.
///
/// Empty output with a success status is unreadable, not an empty fleet
/// (lessons claude-code B4): the force of the rule is that **empty is
/// unreadable**, never a reading of zero. An empty JSON array is a different
/// thing — a listing that answered, and said there is nothing.
fn parse_listing(stdout: &str) -> Result<Vec<Row>, String> {
    if stdout.trim().is_empty() {
        return Err("the listing answered with zero bytes and a success status".to_string());
    }
    serde_json::from_str::<Vec<Row>>(stdout)
        .map_err(|e| format!("the listing did not parse as JSON rows: {e}"))
}

/// Every seat's reading off the listings `listing` answers — one per distinct
/// configuration directory the seats name, `None` for the adapter's own —
/// in the order the seats were asked, one each.
///
/// Pure but for its two seams, so the rules are the same whoever answers
/// them: [`ClaudeCode`] runs the listing and opens the transcript, and a
/// suite's stub hands in its own.
///
/// ONE LISTING PER DIRECTORY, because a directory scopes the listing: a
/// session started under its own is named by that directory's listing and by
/// no other (lessons claude-code A11; interactive rows too, B10). AN
/// UNREADABLE LISTING IS ITS OWN DIRECTORY'S: its seats read `unknown`
/// carrying why, and every other directory's are read as usual.
///
/// A seat's row is the one carrying the seat's `session_id`, and where none
/// does — no id yet, or one the agent no longer has — the one whose pid is
/// the seat's pane's: on 2.1.280 `/clear` gives the same pid a new session
/// id, and the seat learns the new one from this answer (fleet-jymr.3).
/// NEVER BY THE WORKING DIRECTORY, which names a seat and proves nothing about
/// who started the session in it (lessons claude-code B5). Two rows under one
/// key: the newest started.
///
/// A seat with no row is `starting`: whether a start that never lists has
/// failed is core's to judge, off the pane's age, and never an end this
/// answers.
pub fn readings_from(
    seats: &[SeatRef],
    listing: &dyn Fn(Option<&str>) -> Result<String, String>,
    transcript: &dyn Fn(&SeatRef, &str) -> Option<String>,
) -> Vec<SeatActivity> {
    let mut read: BTreeMap<Option<String>, Result<Vec<Row>, String>> = BTreeMap::new();
    for seat in seats {
        let dir = seat
            .config_dir
            .as_deref()
            .map(|dir| dir_key(dir).to_string());
        if let std::collections::btree_map::Entry::Vacant(unread) = read.entry(dir) {
            let rows = listing(unread.key().as_deref()).and_then(|stdout| parse_listing(&stdout));
            unread.insert(rows);
        }
    }
    seats
        .iter()
        .map(|seat| {
            let dir = seat
                .config_dir
                .as_deref()
                .map(|dir| dir_key(dir).to_string());
            match &read[&dir] {
                Err(cause) => SeatActivity {
                    seat: seat.seat,
                    activity: Activity::Unknown,
                    blocked_on: None,
                    evidence: Evidence::Typed,
                    session_id: None,
                    cause: Some(format!("the listing could not be read: {cause}")),
                },
                Ok(rows) => match matched(seat, rows) {
                    None => SeatActivity {
                        seat: seat.seat,
                        activity: Activity::Starting,
                        blocked_on: None,
                        evidence: Evidence::Typed,
                        session_id: None,
                        cause: Some(match seat.pid {
                            Some(pid) => format!("the listing names no row with pid {pid}"),
                            None => "the listing can name no row for it".to_string(),
                        }),
                    },
                    Some(row) => {
                        let reading = reading_of(seat, row);
                        let logged_out = reading.activity == Activity::Idle
                            && transcript(seat, &row.session_id)
                                .is_some_and(|body| logged_out_first_turn(&body));
                        if logged_out {
                            SeatActivity {
                                activity: Activity::Blocked,
                                blocked_on: Some(BlockedOn::LoggedOut),
                                ..reading
                            }
                        } else {
                            reading
                        }
                    }
                },
            }
        })
        .collect()
}

/// The seat's row among `rows`: by its session id, else by its pane's pid,
/// the newest started where two carry the same.
fn matched<'r>(seat: &SeatRef, rows: &'r [Row]) -> Option<&'r Row> {
    // The first listed, unless a later one started strictly after it: a row
    // with no start stamp is the oldest.
    let newest = |found: Vec<&'r Row>| {
        found.into_iter().reduce(|best, row| {
            if row.started_at.unwrap_or(0) > best.started_at.unwrap_or(0) {
                row
            } else {
                best
            }
        })
    };
    let id = seat.session_id.as_deref().map(str::trim).unwrap_or("");
    if !id.is_empty() {
        if let Some(row) = newest(rows.iter().filter(|row| row.session_id == id).collect()) {
            return Some(row);
        }
    }
    let pid = seat.pid?;
    newest(rows.iter().filter(|row| row.pid == Some(pid)).collect())
}

/// A found row's reading, all of it TYPED — the agent's own state and no
/// screen (lessons claude-code B8, B10; re-measured on 2.1.280).
///
/// `waitingFor` is keyed on PRESENCE: present, the seat is blocked whatever
/// the status beside it, and the cause is carried VERBATIM — `permission
/// prompt`, the one value recorded, names the block. Otherwise the status is
/// the activity: `idle`, `busy`, or `waiting`, which is blocked on nothing it
/// names. A row listed before it carries a status is `starting` — on 2.1.280
/// a row was listed half a second before its status. A status word this
/// adapter does not read is `unknown`, naming it, and never a fifth state.
fn reading_of(seat: &SeatRef, row: &Row) -> SeatActivity {
    let typed =
        |activity: Activity, blocked_on: Option<BlockedOn>, cause: Option<String>| SeatActivity {
            seat: seat.seat,
            activity,
            blocked_on,
            evidence: Evidence::Typed,
            session_id: Some(row.session_id.clone()),
            cause,
        };
    if let Some(cause) = &row.waiting_for {
        let on = (cause == PERMISSION_PROMPT).then_some(BlockedOn::Permission);
        return typed(Activity::Blocked, on, Some(cause.clone()));
    }
    match row.status.as_deref() {
        Some(IDLE) => typed(Activity::Idle, None, None),
        Some(BUSY) => typed(Activity::Busy, None, None),
        Some(WAITING) => typed(Activity::Blocked, None, Some(format!("status {WAITING}"))),
        None => typed(
            Activity::Starting,
            None,
            Some("the listing's row carries no status yet".to_string()),
        ),
        Some(other) => typed(
            Activity::Unknown,
            None,
            Some(format!(
                "the listing's status is `{other}`, a word this adapter does not read"
            )),
        ),
    }
}

// ---- the transcript -----------------------------------------------------------

/// The transcript path encoding (lessons claude-code C1): the agent keys a
/// per-project directory on the project path with every non-alphanumeric
/// character replaced by a dash — the separators, and the dots, underscores and
/// spaces beside them.
///
/// Censused on this fleet's own machine: of 235 per-project directories, zero
/// carry any character outside `[A-Za-z0-9-]`, and a path under `.claude`
/// resolves to `--claude`, so the dash is not the separator's alone.
///
/// TWO PARTS OF THE ENCODING ARE NOT HANDLED HERE, because no specimen on this
/// machine exercises them: a project path past roughly 200 characters, which
/// the agent truncates and gives a hash suffix (the longest local directory is
/// 136), and a non-ASCII character, which this maps to a dash without a
/// measurement saying it should. Either yields a path that does not exist,
/// which every reader renders as a seat with no context reading.
///
/// Pure, and separate from the read, because the encoding is what can be wrong
/// and a test must reach it without a filesystem.
pub fn encode_project_dir(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The session's transcript under the agent's configuration directory.
pub fn transcript_path(config_dir: &Path, worktree: &str, session_id: &str) -> PathBuf {
    config_dir
        .join("projects")
        .join(encode_project_dir(worktree))
        .join(format!("{session_id}.jsonl"))
}

#[derive(Deserialize)]
struct TranscriptEntry {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default, rename = "isSidechain")]
    is_sidechain: bool,
    #[serde(default)]
    message: Option<TranscriptMessage>,
    /// The provider's own machine-readable cause on an entry it wrote in place
    /// of a model turn. Absent on every ordinary entry.
    #[serde(default)]
    error: Option<String>,
    /// Whether the provider wrote this entry itself instead of the model
    /// answering.
    #[serde(default, rename = "isApiErrorMessage")]
    is_api_error: bool,
}

#[derive(Deserialize)]
struct TranscriptMessage {
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

/// Context for a session: the last main-chain assistant entry's input tokens
/// plus both cache figures.
///
/// There is no single context number in the file — the arithmetic over those
/// three fields is the reading (lessons claude-code C2). Sidechain entries are
/// skipped because a sidechain is a subagent's turn carrying the subagent's
/// window (C3); the flag is on every entry, so the skip is a filter and not an
/// inference.
///
/// An entry stating no window is skipped: no usage block, or a usage block
/// summing to zero. The agent writes a zero-usage entry for a turn that made no
/// model call, which is main-chain and does carry usage, so a plain last-entry
/// reader publishes 0 for a loaded session. The zero is the test rather than the
/// marker the agent puts on those entries, because a marker string that changes
/// republishes the 0 while a usage schema that moves yields no reading at all —
/// which every reader renders as blind instead.
///
/// A malformed line is skipped rather than fatal: transcripts are read while
/// they are being appended to, so a torn final line is an expected transient.
pub fn context_tokens_in(body: &str) -> Option<u64> {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<TranscriptEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
        .filter_map(|e| e.message)
        .filter_map(|m| m.usage)
        .map(|u| {
            u.input_tokens.unwrap_or(0)
                + u.cache_read_input_tokens.unwrap_or(0)
                + u.cache_creation_input_tokens.unwrap_or(0)
        })
        .filter(|&tokens| tokens != 0)
        .next_back()
}

/// The provider's own cause on the entry it writes in place of a first model
/// turn when the session has no credential (lessons claude-code A11).
pub const AUTHENTICATION_FAILED: &str = "authentication_failed";

/// Whether this transcript's first main-chain assistant entry is the provider's
/// LOGGED-OUT answer.
///
/// A seat started under its own configuration directory is logged out unless the
/// credential knob is defined-but-empty beside it, and the listing cannot say
/// so: such a session is LIVE and idle, with a pid and a status, exactly like
/// one waiting for work. The transcript is the only surface that carries the
/// reading, and [`readings_from`] answers it as `blocked_on: logged_out`.
///
/// THREE TERMS, ALL REQUIRED: the entry is one the provider wrote itself, its
/// cause is [`AUTHENTICATION_FAILED`], and its window is zero. The conjunction
/// is the safe direction — a term that moves in a later release yields NO
/// reading, and a reading nobody has costs one uncaught logged-out seat, where a
/// looser match would fail a dispatch that was fine.
///
/// It reads the FIRST such entry and not the last: what is being asked is how
/// the session ANSWERED ITS FIRST TURN, and an entry further down belongs to a
/// session that was already working.
pub fn logged_out_first_turn(body: &str) -> bool {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<TranscriptEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
        .map(|e| {
            let window = e
                .message
                .and_then(|m| m.usage)
                .map(|u| {
                    u.input_tokens.unwrap_or(0)
                        + u.cache_read_input_tokens.unwrap_or(0)
                        + u.cache_creation_input_tokens.unwrap_or(0)
                })
                .unwrap_or(0);
            e.is_api_error && e.error.as_deref() == Some(AUTHENTICATION_FAILED) && window == 0
        })
        .next()
        .unwrap_or(false)
}

/// How many turns a session took: main-chain assistant entries carrying a usage
/// block.
///
/// THE SAME FILTER AS [`context_tokens_in`], ONE STEP SHORTER. A turn that made
/// a model call is a turn whatever the call cost, so the zero-usage entry that
/// reader skips — the agent's line for a turn that called no model — is COUNTED
/// here: a session's turns and the window its last turn carried are two
/// different questions, and the second one is why that skip exists.
///
/// An entry with no usage block at all is not a turn the agent made a call for
/// and is not counted, which is the one thing the two readers agree to drop.
/// Sidechains are skipped for C3's reason: a sidechain is a subagent's turn and
/// the seat did not take it.
///
/// A malformed line is skipped rather than fatal, as it is there: a transcript
/// is read while it is being appended to, so a torn final line is expected.
pub fn turns_in(body: &str) -> u64 {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<TranscriptEntry>(l).ok())
        .filter(|e| e.kind == "assistant" && !e.is_sidechain)
        .filter_map(|e| e.message)
        .filter(|m| m.usage.is_some())
        .count() as u64
}

/// One seat's context off its transcript's body and its last write, where
/// either could be read.
///
/// A BODY THAT OPENED AND STATES NO TURN IS A MEASURED ZERO, which is why the
/// turns are counted whenever there is a body: a session that took none and a
/// transcript nobody could open answer differently, and they are different
/// facts. No body is the seat's id alone — every reader renders a missing
/// reading as blind, where a 0 would read as an empty session.
///
/// The last write is the file's MTIME and not a timestamp parsed out of its
/// last line: a torn or partly written entry still has an mtime, and a line
/// that does not parse would answer nothing for a session that plainly ended.
/// It is the session's own final act and it outlives the process (lessons
/// claude-code C4). The cost is the write cadence — an mtime is the last WRITE
/// and not the last word — which is far inside any window an operator would
/// set in hours. `window` is never answered: the transcript states none.
pub fn context_of(seat: &SeatRef, body: Option<&str>, written: Option<SystemTime>) -> SeatContext {
    SeatContext {
        seat: seat.seat,
        tokens: body.and_then(context_tokens_in),
        window: None,
        turns: body.map(turns_in),
        last_write: written
            .and_then(crate::clock::stamp_of)
            .and_then(|stamp| Stamp::parse(&stamp)),
    }
}

// ---- the permission file ------------------------------------------------------

/// Where Claude Code reads a session's project-local settings from, relative
/// to the working directory the session comes up in — which is where a
/// transient seat's permission rules are written, since a permission list
/// cannot ride the plugin root the overlay is loaded through.
pub const LOCAL_SETTINGS: &str = ".claude/settings.local.json";

/// The placeholder a permissions template writes the seat's worktree under.
pub const WORKTREE: &str = "{worktree}";

/// The placeholder a permissions template writes the builder's checks under.
pub const TOUCHED: &str = "{touched}";

/// The permission lists a merge folds. Everything else in either document is
/// the project's to keep: a key only the pack carries is not a rule, and a key
/// only the project carries is not this adapter's to touch.
const MERGED_LISTS: [&str; 2] = ["allow", "deny"];

/// The request's neutral permissions rendered into the template this adapter
/// was opened with, as Claude Code's own settings document for `worktree`.
///
/// `{touched}` is the builder's checks, escaped as JSON because it is written
/// inside one of the document's strings. A request that carries none gets NO
/// RULE for one: every allow entry naming the placeholder is taken out before
/// the render, so the seat's rules never name a command nobody gave.
///
/// The commands are the project's `[permissions] tool_commands`, each one
/// command word, appended to the allow list as `Bash(<word>:*)` after the
/// pack's own and deduplicated against it. A request carrying none gets the
/// rendered document back UNTOUCHED rather than round-tripped through a parse,
/// so the bytes a seat comes up under are then the pack's document with two
/// values in it.
///
/// A placeholder the template writes that this render has no value for is an
/// error, never a literal a seat would come up under.
pub fn render_permissions(
    template: &str,
    permissions: &Permissions,
    worktree: &Path,
) -> Result<String, String> {
    let (template, touched) = match permissions
        .touched
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
    {
        Some(command) => (template.to_string(), inside_a_json_string(command)),
        None => (without_the_touched_rule(template)?, String::new()),
    };
    let rendered = fleet_core::item::render(
        &template,
        &[
            ("touched", &touched),
            ("worktree", &worktree.display().to_string()),
        ],
    )
    .map_err(|name| {
        format!("the permissions template names `{{{name}}}`, which a launch has no value for")
    })?;
    with_tool_commands(rendered, &permissions.commands)
}

/// A command as the characters that stand for it between a JSON string's
/// quotes.
fn inside_a_json_string(command: &str) -> String {
    let quoted = serde_json::Value::String(command.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

/// The template with every allow entry that names `{touched}` taken out.
///
/// A template that names none is handed back UNTOUCHED rather than round-tripped
/// through a parse, so a pack whose rules carry no builder's checks comes up under
/// its own bytes whatever the launch was handed.
fn without_the_touched_rule(template: &str) -> Result<String, String> {
    if !template.contains(TOUCHED) {
        return Ok(template.to_string());
    }
    let mut doc: serde_json::Value = serde_json::from_str(template).map_err(|why| {
        format!(
            "the permissions template names {TOUCHED} and is not the JSON its rule can be taken \
             out of: {why}"
        )
    })?;
    if let Some(allow) = doc
        .get_mut("permissions")
        .and_then(|permissions| permissions.get_mut("allow"))
        .and_then(serde_json::Value::as_array_mut)
    {
        allow.retain(|rule| !rule.as_str().is_some_and(|rule| rule.contains(TOUCHED)));
    }
    let mut out = serde_json::to_string_pretty(&doc)
        .map_err(|why| format!("the seat's own settings could not be written: {why}"))?;
    out.push('\n');
    Ok(out)
}

/// The project's words as rules after the pack's own allow list, deduplicated
/// against it.
fn with_tool_commands(rendered: String, words: &[String]) -> Result<String, String> {
    if words.is_empty() {
        return Ok(rendered);
    }
    let mut doc: serde_json::Value = serde_json::from_str(&rendered).map_err(|why| {
        format!("the permissions template is not the JSON a rule can be added to: {why}")
    })?;
    let allow = doc
        .get_mut("permissions")
        .and_then(|permissions| permissions.get_mut("allow"))
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| {
            "the permissions template carries no `permissions.allow` list for [permissions] \
             tool_commands to be added to"
                .to_string()
        })?;
    for word in words {
        let rule = serde_json::Value::String(format!("Bash({word}:*)"));
        if !allow.contains(&rule) {
            allow.push(rule);
        }
    }
    let mut out = serde_json::to_string_pretty(&doc)
        .map_err(|why| format!("the seat's own settings could not be written: {why}"))?;
    out.push('\n');
    Ok(out)
}

/// Render the permissions into the worktree's settings document and read it
/// back.
///
/// A document already standing at the path is MERGED INTO, never replaced: a
/// project that tracks this file on its trunk hands every transient worktree a
/// copy of it, and a launch that wrote over it would take the project's own
/// rules out of the seat's session and leave a tracked file modified in a
/// checkout nobody edited.
///
/// The read-back is the same check every other step of a spawn takes: a write
/// that silently landed short leaves a seat that can neither edit nor commit,
/// and the refusal a person would get instead is one from the agent, hours
/// later.
fn write_settings(
    worktree: &Path,
    template: &str,
    permissions: &Permissions,
) -> Result<(), String> {
    let rendered = render_permissions(template, permissions, worktree)?;
    let path = worktree.join(LOCAL_SETTINGS);
    let standing = match std::fs::read_to_string(&path) {
        Ok(standing) => Some(standing),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(format!(
                "the settings document at {} could not be read, and one that cannot be read is \
                 never written over: {e}",
                path.display()
            ))
        }
    };
    let document = match standing {
        None => rendered,
        Some(standing) => merged_settings(&standing, &rendered).map_err(|why| {
            format!(
                "the pack's permission rules were not merged into the document at {}, and \
                 nothing was written over: {why}",
                path.display()
            )
        })?,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    std::fs::write(&path, &document).map_err(|e| {
        format!(
            "the permission rules at {} were not written: {e}",
            path.display()
        )
    })?;
    match std::fs::read_to_string(&path) {
        Ok(back) if back == document => Ok(()),
        Ok(_) => Err(format!(
            "the permission rules at {} do not read back as they were written",
            path.display()
        )),
        Err(e) => Err(format!(
            "the permission rules at {} do not read back: {e}",
            path.display()
        )),
    }
}

/// The pack's permission lists folded into the document a project already
/// carries: the project's document is the base, its rules stand FIRST and in
/// their own order, the pack's are appended, and a rule the project already
/// carries is not appended twice.
///
/// A document either side offers that this cannot read as JSON is an error
/// here, which the caller turns into a refusal — the alternative is to fall
/// back on the write, and the write is the damage.
fn merged_settings(project: &str, pack: &str) -> Result<String, String> {
    let mut merged: serde_json::Value = serde_json::from_str(project)
        .map_err(|e| format!("the project's document is not JSON: {e}"))?;
    let rules: serde_json::Value =
        serde_json::from_str(pack).map_err(|e| format!("the pack's document is not JSON: {e}"))?;
    let Some(root) = merged.as_object_mut() else {
        return Err("the project's document is not a JSON object".to_string());
    };
    let Some(permissions) = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
    else {
        return Err("the project's `permissions` is not a JSON object".to_string());
    };
    for list in MERGED_LISTS {
        let Some(theirs) = rules
            .get("permissions")
            .and_then(|held| held.get(list))
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let Some(ours) = permissions
            .entry(list)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
        else {
            return Err(format!(
                "the project's `permissions.{list}` is not an array"
            ));
        };
        for rule in theirs {
            if !ours.contains(rule) {
                ours.push(rule.clone());
            }
        }
    }
    serde_json::to_string_pretty(&merged)
        .map_err(|e| format!("the merged document could not be written out: {e}"))
}

// ---- the daemon ---------------------------------------------------------------

/// One session the daemon hosts, as [`ClaudeCode::daemon_hosted`] reads it:
/// what a person needs to find it and to stop it, and nothing a caller could
/// issue an act against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hosted {
    pub session_id: String,
    /// The directory the session stands in, as the listing reports it.
    pub cwd: String,
    short_id: String,
}

impl Hosted {
    /// The row's working directory in the form seat matching compares.
    pub fn cwd_key(&self) -> &str {
        dir_key(&self.cwd)
    }

    /// The refusal's line for this session, standing in `worktree` of the
    /// seat `seat` names.
    pub fn line(&self, seat: &str, worktree: &str) -> String {
        format!(
            "{seat} is hosted by the Claude Code daemon: session {}, short id {}, in {worktree}",
            self.session_id, self.short_id
        )
    }

    /// The command a person runs to stop this session, under the
    /// configuration directory its listing was read under: the daemon a
    /// directory scopes is that directory's own (lessons claude-code A11).
    pub fn stop_command(&self, config_dir: Option<&str>) -> String {
        match config_dir {
            Some(dir) => format!(
                "CLAUDE_CONFIG_DIR={dir} {DEFAULT_BIN} stop {}",
                self.short_id
            ),
            None => format!("{DEFAULT_BIN} stop {}", self.short_id),
        }
    }
}

/// The listing's row as the daemon check reads it: this adapter's own shape,
/// private, so the short id it carries never becomes a field anything outside
/// this file holds (reviewer call 2026-09-25, E6).
#[derive(serde::Deserialize)]
struct ListedRow {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    #[serde(default)]
    pid: Option<u32>,
    /// The daemon's short id: a background row always carries one and an
    /// interactive row never does (lessons claude-code A6, B10), which is the
    /// whole of the test.
    #[serde(default)]
    id: Option<String>,
}

/// The rows of a listing the daemon hosts that are LIVE: a short id and a
/// pid.
///
/// A row with a short id and no pid is left out. A background session stopped
/// from idle reads pid-less with its state `done` and stays listed (lessons
/// claude-code A3), so a check that counted it would go on refusing after the
/// person ran the very stop it named; a live one is a process the daemon keeps
/// running in a seat's worktree, which is what a start must not go on beside.
///
/// Empty output is unreadable, never a listing of nothing (lessons claude-code
/// B4), exactly as the listing's own reader reads it.
pub fn parse_hosted(stdout: &str) -> Result<Vec<Hosted>, String> {
    if stdout.trim().is_empty() {
        return Err("the listing answered with zero bytes and a success status".to_string());
    }
    let rows: Vec<ListedRow> = serde_json::from_str(stdout)
        .map_err(|e| format!("the listing did not parse as JSON rows: {e}"))?;
    Ok(rows
        .into_iter()
        .filter(|row| row.pid.is_some())
        .filter_map(|row| {
            let short_id = row.id.filter(|id| !id.trim().is_empty())?;
            Some(Hosted {
                session_id: row.session_id,
                cwd: row.cwd,
                short_id,
            })
        })
        .collect())
}

/// Whoever answers the upgrade refusal's reading, handed to the loop beside
/// the agent (`crate::run::Seams`): this adapter, whose
/// [`ClaudeCode::daemon_hosted`] it is, or a suite's own answer.
///
/// A seam of its own and never a verb on [`Agent`], for the reason
/// [`ClaudeCode::daemon_hosted`] gives; it goes with this file.
pub trait DaemonListing {
    fn daemon_hosted(&self, config_dir: Option<&Path>) -> Result<Vec<Hosted>, String>;
}

impl DaemonListing for ClaudeCode {
    fn daemon_hosted(&self, config_dir: Option<&Path>) -> Result<Vec<Hosted>, String> {
        ClaudeCode::daemon_hosted(self, config_dir)
    }
}

impl<F> DaemonListing for F
where
    F: Fn(Option<&Path>) -> Result<Vec<Hosted>, String>,
{
    fn daemon_hosted(&self, config_dir: Option<&Path>) -> Result<Vec<Hosted>, String> {
        self(config_dir)
    }
}

/// `claude --version` prints the version and then what produced it; the first
/// token is the version.
pub fn parse_version(stdout: &str) -> Option<String> {
    stdout.split_whitespace().next().map(str::to_string)
}

// ---- the configuration directory ----------------------------------------------

/// The agent's own state file inside a configuration directory, where the
/// onboarding stamp and the per-directory trust are kept.
pub const STATE_FILE: &str = ".claude.json";

/// The keys a seat's configuration directory is seeded with from the
/// operator's own state file, every one REQUIRED: an interactive session under
/// a directory missing them stops at the theme picker and then at the
/// login-method menu before any session exists, and with them it came up
/// logged in under the operator's subscription (lessons claude-code D8; the
/// three measured sufficient on 2.1.280, 2026-09-26).
pub const ONBOARDING_KEYS: [&str; 3] = [
    "hasCompletedOnboarding",
    "lastOnboardingVersion",
    "oauthAccount",
];

/// The fourth key D8 copied, carried over WHERE THE OPERATOR'S FILE HAS IT and
/// never required: on 2.1.280 a directory seeded without it met no theme
/// picker (measured 2026-09-26), and this machine's own state file carries none
/// — the setting lives in the operator's settings instead — so requiring it
/// would refuse every spawn here over a key the start does not need.
pub const THEME_KEY: &str = "theme";

/// The per-directory table in [`STATE_FILE`], and the flag in an entry that
/// says the workspace-trust question was answered yes (lessons claude-code
/// A15). A directory whose entry carries it true started with no question on
/// 2.1.280 (measured 2026-09-26).
pub const PROJECTS_KEY: &str = "projects";
pub const TRUST_KEY: &str = "hasTrustDialogAccepted";

/// The operator's own state file, which the seed copies from: inside the
/// configured configuration directory where the operator configured one, and
/// beside the default directory — in the home — where they did not, which is
/// where the agent itself keeps it in each case.
///
/// `credential_dir` is the configured value (empty for none) and `config_dir`
/// the resolved one, so the unconfigured case is `config_dir`'s parent: the
/// resolved directory is then `<home>/.claude` by `config_dir_from`'s own rule.
pub fn operator_file(credential_dir: &str, config_dir: &Path) -> PathBuf {
    if credential_dir.is_empty() {
        config_dir.parent().unwrap_or(config_dir).join(STATE_FILE)
    } else {
        Path::new(credential_dir).join(STATE_FILE)
    }
}

/// Seed a seat's configuration directory so an interactive session under it
/// comes up with no question in front of it: the onboarding keys copied from
/// `operator`'s state file, and the workspace-trust acceptance for `worktree`
/// and for no other directory (ruling 13).
///
/// MERGED into whatever [`STATE_FILE`] the overlay already put in `dir`, so a
/// pack's own keys survive: a seeded key is set and nothing else is touched.
/// Nothing but the three keys and the theme is copied — the operator's own
/// `projects` table least of all, which would trust every directory they have
/// ever trusted.
///
/// The trust entry is keyed on the worktree RESOLVED, because the session
/// reads its directory off its own process, which the operating system hands
/// back resolved: a start given `/tmp/x` on this machine is listed at
/// `/private/tmp/x`, and an entry keyed on the resolved path held for it
/// (measured on 2.1.280, 2026-09-26). A path that will not resolve is keyed as
/// given.
pub fn seed_config(dir: &Path, operator: &Path, worktree: &Path) -> Result<(), String> {
    let theirs = read_object(operator)?.ok_or_else(|| {
        format!(
            "{} is not there to copy the onboarding keys from",
            operator.display()
        )
    })?;
    let path = dir.join(STATE_FILE);
    let mut seeded = read_object(&path)?.unwrap_or_default();
    for key in ONBOARDING_KEYS {
        let value = theirs.get(key).ok_or_else(|| {
            format!(
                "{} carries no `{key}`, which a seat's configuration directory is seeded with \
                 so its session meets no onboarding",
                operator.display()
            )
        })?;
        seeded.insert(key.to_string(), value.clone());
    }
    if let Some(theme) = theirs.get(THEME_KEY) {
        seeded.insert(THEME_KEY.to_string(), theme.clone());
    }
    let trusted = std::fs::canonicalize(worktree)
        .unwrap_or_else(|_| worktree.to_path_buf())
        .display()
        .to_string();
    let projects = seeded
        .entry(PROJECTS_KEY)
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| format!("{}: `{PROJECTS_KEY}` is not an object", path.display()))?;
    let entry = projects
        .entry(trusted)
        .or_insert_with(|| serde_json::Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| format!("{}: the worktree's entry is not an object", path.display()))?;
    entry.insert(TRUST_KEY.to_string(), serde_json::Value::Bool(true));
    let body = serde_json::to_vec_pretty(&serde_json::Value::Object(seeded))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    crate::platform::write_atomic(&path, &body)
        .map_err(|e| format!("{} was not written: {e}", path.display()))
}

/// A JSON object read off `path`, `None` where there is no file, and a refusal
/// naming the file where there is one that is not an object.
fn read_object(path: &Path) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{} could not be read: {e}", path.display())),
    };
    match serde_json::from_slice(&bytes) {
        Ok(serde_json::Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(format!("{} is not a JSON object", path.display())),
        Err(e) => Err(format!("{} is not JSON: {e}", path.display())),
    }
}

/// The workspace-trust question's accepting choice, as the screen spells it on
/// 2.1.280 (measured 2026-09-26): the dialog lists "No, exit" first, selected,
/// and this second.
pub const TRUST_CHOICE: &str = "Yes, I trust this folder";

/// The keys that accept the workspace-trust question, when `screen` shows it:
/// Down onto the accepting choice, then Enter (lessons claude-code D8).
///
/// The screen is read with its whitespace collapsed, so a choice the pane's
/// width wrapped is still one phrase.
pub fn trust_keys(screen: &str) -> Option<Vec<String>> {
    let flat = screen.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.contains(TRUST_CHOICE)
        .then(|| vec!["Down".to_string(), "C-m".to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_core::seat::identity::SeatId;

    /// `ClaudeCode::new` itself, which the arms below never build: they pin the
    /// three seam readers, and a constructor that asked a different one — or
    /// none — passed every one of them.
    ///
    /// The seams are read from the process environment, which every test in this
    /// binary shares and which a `set_var` here would race — the `getenv` another
    /// test's `temp_dir` makes. So the subject is the COMPOSITION and not the
    /// defaults: each field equals its own reader applied to the live value,
    /// which reads the same on a box exporting a documented seam and on one
    /// exporting none. The defaults are pinned by the reader arms below.
    #[test]
    fn the_adapter_built_carries_each_reader_s_reading_of_the_live_environment() {
        let home = Path::new("/nowhere-in-particular");
        let agent = ClaudeCode::new(home);
        assert_eq!(
            agent.timeout,
            timeout_from(std::env::var("FLEET_AGENT_TIMEOUT_MS").ok().as_deref()),
            "the deadline a real call runs on, and not only the constant"
        );
        assert_eq!(agent.bin, bin_from(configured_bin().as_deref()));
        assert_eq!(
            agent.config_dir,
            config_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref(), home),
            "the home it was handed, and not this process's own"
        );
        assert_eq!(
            agent.credential_dir,
            credential_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref()),
            "the configured value, read by its own reader and not by the directory's"
        );
        assert_eq!(
            agent.fleet_bin,
            crate::adapter::own_executable(),
            "the executable this process is, which every child is handed"
        );
    }

    fn scratch_agent() -> ClaudeCode {
        ClaudeCode::with_seams(
            "claude".to_string(),
            PathBuf::from("/nowhere/.claude"),
            DEFAULT_TIMEOUT,
            "/usr/bin:/bin".to_string(),
            Some(PathBuf::from("/opt/claude/bin/claude")),
            String::new(),
        )
    }

    /// Every child carries the agent's auto-updater OFF — a session's pane and a
    /// read alike, under the adapter's own configuration directory and under a
    /// seat's.
    ///
    /// The incident this answers (fleet-rge6.2's measurement, 2026-09-26): an
    /// interactive session with the updater on installed a newer release and
    /// re-pointed the operator's own binary under them, which is the pin moved by
    /// nobody (lessons claude-code A1). The control is the variable's value:
    /// present and set to anything but `1` is an updater that still runs.
    #[test]
    fn every_child_carries_the_auto_updater_off() {
        let agent = scratch_agent();
        for under in [None, Some(Path::new("/nowhere/seats/a-seat"))] {
            let env = agent.environment(under);
            let set: Vec<&str> = env
                .iter()
                .filter(|(key, _)| key == AUTOUPDATER_VAR)
                .map(|(_, value)| value.as_str())
                .collect();
            assert_eq!(
                set,
                vec!["1"],
                "exactly one {AUTOUPDATER_VAR}=1 under {under:?}: {env:?}"
            );
        }
        let launched = agent.launch(&a_launch()).expect("the launch is built");
        assert_eq!(
            launched.env.get(AUTOUPDATER_VAR).map(String::as_str),
            Some("1")
        );
        let resumed = agent.resume(&a_resume()).expect("the resume is built");
        assert_eq!(
            resumed.env.get(AUTOUPDATER_VAR).map(String::as_str),
            Some("1")
        );
        assert_eq!(AUTOUPDATER_VAR, "DISABLE_AUTOUPDATER");
    }

    /// The deadline the controller runs on when nothing sets one. Asserted as the
    /// figure and not as "some default", because the number is the whole content:
    /// widened, a hung listing hangs the poll for as long as it says.
    #[test]
    fn the_deadline_with_no_seam_set_is_twenty_seconds() {
        assert_eq!(timeout_from(None), Duration::from_secs(20));
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(20));
    }

    /// A blank seam is the default and never an empty program, and a setting that
    /// names one is taken trimmed — the same reading its sibling gives a
    /// configured directory.
    #[test]
    fn a_blank_binary_seam_is_the_default_and_a_named_one_is_trimmed() {
        for configured in [None, Some(""), Some("   "), Some("\t\n")] {
            assert_eq!(
                bin_from(configured),
                DEFAULT_BIN,
                "{configured:?} names no binary"
            );
        }
        assert_eq!(bin_from(Some("/opt/claude")), "/opt/claude");
        assert_eq!(bin_from(Some("  /opt/claude  ")), "/opt/claude");
    }

    /// An adapter reading through `bin`, with nothing else resolved.
    fn reading_through(bin: &str) -> ClaudeCode {
        ClaudeCode::with_seams(
            bin.to_string(),
            PathBuf::from("/nowhere-in-particular/.claude"),
            Duration::from_secs(5),
            String::new(),
            None,
            String::new(),
        )
    }

    /// The listing's cause through the one call `read` makes of it: the run, then
    /// the parse.
    fn listing_cause(agent: &ClaudeCode) -> String {
        match agent
            .listing(None)
            .and_then(|stdout| parse_listing(&stdout))
        {
            Err(cause) => cause,
            Ok(rows) => panic!("{} read {} rows", agent.bin, rows.len()),
        }
    }

    /// The spawn the guard above exists to prevent, read rather than described:
    /// the reader's docstring quotes this cause as its reason, and a quote no arm
    /// takes is a claim about the OS that nobody has checked.
    ///
    /// The field is set directly, because the guard means no setting of the seam
    /// can reach here — which is the point, and is what the closing pair of
    /// `bin_from` readings says from the other side.
    #[test]
    fn an_empty_program_spawns_as_a_cause_that_names_no_binary() {
        let cause = listing_cause(&reading_through(""));
        assert!(
            cause.contains("could not start :"),
            "the cause names the empty program as the binary, which is no name at all: {cause}"
        );
        assert!(
            cause.contains("No such file or directory"),
            "and what the OS said about it: {cause}"
        );

        // The control: a program that EXISTS, spawned by this same call. It fails
        // at the CALL and never at the spawn, so the first assertion above reads
        // the empty program rather than a listing that answers "could not start"
        // for everything.
        //
        // THE PROGRAM IS ONE WHOSE ARGV CAN NEITHER NAME A FILE NOR ACT ON A
        // PROCESS. The listing fixes the arguments at `agents --json --all`, so a
        // shell or a reader here would resolve `agents` against whatever directory
        // the test process happens to be in, and a file of that name sitting there
        // would be RUN. `false` ignores every argument it is given and exits 1, so
        // this control's result is the same in every directory and on every
        // system, and it reaches nothing outside itself either way.
        let control = listing_cause(&reading_through("/usr/bin/false"));
        assert!(
            !control.contains("could not start"),
            "a program that is there names no failed start: {control}"
        );
        assert!(
            control.contains("exited "),
            "and the failure is the program's own, at the call, carrying the status it chose: \
             {control}"
        );

        // The control for the second assertion, which the one above cannot reach:
        // a program that starts, answers, and fails at the PARSE, so its cause
        // carries neither needle.
        let answered = listing_cause(&reading_through("/bin/echo"));
        assert!(
            answered.contains("did not parse as JSON rows"),
            "the second control answers and fails at the parse: {answered}"
        );
        for needle in ["could not start", "No such file or directory"] {
            assert!(
                !answered.contains(needle),
                "a cause from a program that started carries no {needle}: {answered}"
            );
        }

        // The other side: no reading of the seam produces that program, so this
        // spawn is unreachable through `new` and the quote is a reason and not a
        // path.
        assert_eq!(bin_from(Some("")), DEFAULT_BIN);
        assert_eq!(bin_from(None), DEFAULT_BIN);
    }

    /// The third seam reader, on the same two readings its siblings are pinned on:
    /// a blank setting is the default under the home it was handed, and a named
    /// one is taken trimmed. The home is a parameter, so the default is asserted
    /// against the argument and never against this box's own.
    #[test]
    fn a_blank_config_dir_seam_is_the_home_default_and_a_named_one_is_trimmed() {
        let home = Path::new("/nowhere-in-particular");
        for configured in [None, Some(""), Some("   "), Some("\t\n")] {
            assert_eq!(
                config_dir_from(configured, home),
                home.join(".claude"),
                "{configured:?} names no directory"
            );
        }
        assert_eq!(
            config_dir_from(Some("/opt/cfg"), home),
            PathBuf::from("/opt/cfg")
        );
        assert_eq!(
            config_dir_from(Some("  /opt/cfg  "), home),
            PathBuf::from("/opt/cfg")
        );

        // The reading a different home moves, so the default above is the
        // argument's and not a constant that happens to match it.
        assert_eq!(
            config_dir_from(None, Path::new("/elsewhere")),
            PathBuf::from("/elsewhere/.claude")
        );
    }

    /// The fourth seam reader: the same setting its sibling above resolves to a
    /// directory, read as the value the operator gave it.
    ///
    /// THE TWO DIVERGE ON THE UNCONFIGURED CASE AND THAT DIVERGENCE IS THE WHOLE
    /// POINT. A reader that answered the home default here — which is the drift a
    /// later hand makes, and what a copy of `config_dir_from` would do — hands the
    /// child a directory, and a directory is a credential nobody wrote. So the
    /// blank readings are asserted against the empty string AND against the
    /// sibling's answer for the same input, which no directory value can satisfy
    /// at once.
    #[test]
    fn a_blank_credential_seam_is_empty_and_a_named_one_is_trimmed() {
        let home = Path::new("/nowhere-in-particular");
        for configured in [None, Some(""), Some("   "), Some("\t\n")] {
            assert_eq!(
                credential_dir_from(configured),
                "",
                "{configured:?} names no credential scope, and empty is the value that restores \
                 the operator's own"
            );
            assert_ne!(
                PathBuf::from(credential_dir_from(configured)),
                config_dir_from(configured, home),
                "{configured:?} reads as the resolved directory, which is a different credential"
            );
        }
        assert_eq!(credential_dir_from(Some("/opt/cfg")), "/opt/cfg");
        assert_eq!(credential_dir_from(Some("  /opt/cfg  ")), "/opt/cfg");
    }

    /// The seam, in milliseconds: the value this reader returns for a setting, and
    /// nothing about elapsed time. The 1 ms case pins that the guard rejects a
    /// non-deadline without flooring the value; what a 1 ms deadline costs a real
    /// call is `run_bounded`'s 20 ms poll, which this arm never enters.
    ///
    /// THE CLAIM IS ABOUT A RANGE AND THE CASES ARE ITS ENDS AND ITS MIDDLE: 1 and
    /// 150 sit inside the two stretches the claim covers, and the case ABOVE the
    /// default is what says the promise reaches past it — a cap at
    /// `DEFAULT_TIMEOUT` satisfies every other line here.
    #[test]
    fn the_seam_sets_the_deadline_in_milliseconds() {
        assert_eq!(timeout_from(Some("1")), Duration::from_millis(1));
        assert_eq!(timeout_from(Some("150")), Duration::from_millis(150));
        assert_eq!(timeout_from(Some("300")), Duration::from_millis(300));
        assert_eq!(timeout_from(Some(" 300 ")), Duration::from_millis(300));
        assert_eq!(timeout_from(Some("5000")), Duration::from_secs(5));

        // Above the default, built FROM the default so the case cannot become a
        // value under it the day that constant moves.
        let above_default = DEFAULT_TIMEOUT + Duration::from_secs(40);
        assert_eq!(
            timeout_from(Some(&above_default.as_millis().to_string())),
            above_default,
            "a seam above the default is honoured, not capped at it"
        );
    }

    /// The operator's state file is where the agent itself keeps it: inside a
    /// configured configuration directory, and in the home — beside the default
    /// directory — where none is configured.
    #[test]
    fn the_operator_file_is_inside_a_configured_directory_and_in_the_home_otherwise() {
        let home = Path::new("/a-home");
        let unconfigured = operator_file(&credential_dir_from(None), &config_dir_from(None, home));
        assert_eq!(unconfigured, PathBuf::from("/a-home/.claude.json"));
        let configured = operator_file(
            &credential_dir_from(Some("/opt/cfg")),
            &config_dir_from(Some("/opt/cfg"), home),
        );
        assert_eq!(configured, PathBuf::from("/opt/cfg/.claude.json"));
    }

    /// The trust question's keys come back only for a screen showing its accepting
    /// choice, wrapped or not — and never for a session at its prompt, which a
    /// start must not type into.
    #[test]
    fn the_trust_keys_answer_the_trust_question_and_nothing_else() {
        let asked = " Quick safety check: Is this a project you created or one you trust?\n\
                     ❯ No, exit\n   Yes, I trust this folder\n";
        assert_eq!(
            trust_keys(asked),
            Some(vec!["Down".to_string(), "C-m".to_string()])
        );
        let wrapped = "❯ No, exit\n   Yes, I trust\nthis folder\n";
        assert!(
            trust_keys(wrapped).is_some(),
            "a wrapped choice is one phrase"
        );
        let at_the_prompt = " ▐▛███▜▌   Claude Code v2.1.280\n❯ \n  ⏸ manual mode on\n";
        assert_eq!(trust_keys(at_the_prompt), None);
    }

    /// Every reading that is not a positive whole number of milliseconds is the
    /// default. A zero is named here beside the typos: it parses, and honouring it
    /// would kill every listing before it could answer.
    #[test]
    fn an_unreadable_or_zero_seam_is_the_default_and_never_a_zero_deadline() {
        for configured in ["", "   ", "0", "-1", "3.5", "20s", "twenty"] {
            assert_eq!(
                timeout_from(Some(configured)),
                DEFAULT_TIMEOUT,
                "{configured:?} is not a deadline"
            );
        }
    }

    /// The daemon check's one test: a short id on a live row (lessons claude-code
    /// A6, B10). An interactive row carries none, and a background row the daemon
    /// no longer runs carries no pid (A3); an empty answer is unreadable (B4), and
    /// an empty array is a listing of nothing.
    #[test]
    fn a_daemon_hosted_row_is_a_short_id_on_a_live_row() {
        let listing = r#"[
            {"id":"ab12","sessionId":"s-live","cwd":"/wt/a/","kind":"background","pid":4242,"state":"done","status":"idle"},
            {"id":"cd34","sessionId":"s-stopped","cwd":"/wt/b","kind":"background","state":"done"},
            {"sessionId":"s-seat","cwd":"/wt/c","kind":"interactive","pid":4343,"status":"idle"}
        ]"#;
        let hosted = parse_hosted(listing).expect("the listing parses");
        assert_eq!(hosted.len(), 1, "{hosted:?}");
        assert_eq!(hosted[0].session_id, "s-live");
        assert_eq!(hosted[0].cwd_key(), "/wt/a");
        assert_eq!(
            hosted[0].line("agent-e8a04b17", "/wt/a"),
            "agent-e8a04b17 is hosted by the Claude Code daemon: session s-live, short id ab12, in \
             /wt/a"
        );
        assert_eq!(hosted[0].stop_command(None), "claude stop ab12");
        assert_eq!(
            hosted[0].stop_command(Some("/cfg/a")),
            "CLAUDE_CONFIG_DIR=/cfg/a claude stop ab12"
        );
        assert_eq!(parse_hosted("[]"), Ok(Vec::new()));
        assert!(parse_hosted("").is_err());
        assert!(parse_hosted("not json").is_err());
    }

    // ---- the six verbs ------------------------------------------------------------

    const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
    const OTHER_ID: &str = "01a0d1f1-0aec-765f-9abe-00007e3fa2c0";

    fn id(text: &str) -> SeatId {
        SeatId::parse(text).expect("a hand-written seat id parses")
    }

    /// A named seat's launch: no configuration directory of its own, so it seeds
    /// nothing.
    fn a_launch() -> Launch {
        Launch {
            seat: id(SEAT_ID),
            worktree: "/nowhere/wt/builder-1".to_string(),
            name: "builder-9739a".to_string(),
            model: "claude-opus-5".to_string(),
            posture: Posture::Unattended,
            first_turn: "/wake builder-9739a".to_string(),
            config_dir: None,
            env: BTreeMap::from([(
                crate::adapter::FLEET_ACTOR_VAR.to_string(),
                format!("seat:{SEAT_ID}"),
            )]),
            permissions: Permissions::default(),
        }
    }

    fn a_resume() -> Resume {
        Resume {
            session_id: "663267a3-2b6e-44ac-b0f1-cbfd24bbbc5d".to_string(),
            worktree: "/nowhere/wt/builder-1".to_string(),
            config_dir: Some("/nowhere/seats/builder-9739a".to_string()),
            model: "claude-haiku-4-5".to_string(),
            posture: Posture::Auto,
        }
    }

    /// The declaration, whole: every posture, the model and first turn a seat that
    /// names none starts with, `context` answered, the supported release as the
    /// one measured, and `auto` held to the models measured to honour it — and a
    /// declaration the contract's own rules accept.
    #[test]
    fn the_capabilities_are_claude_codes_and_keep_the_contracts_rules() {
        let declared = scratch_agent().capabilities().expect("declared in-process");
        assert_eq!(declared, capabilities());
        assert_eq!(
            declared.postures,
            vec![Posture::Ask, Posture::Auto, Posture::Unattended]
        );
        assert_eq!(declared.default_model, "claude-opus-5");
        assert_eq!(declared.first_turn, "/wake {seat}");
        assert!(declared.context);
        assert_eq!(
            declared.measured,
            vec![fleet_core::supported::PINNED_CLAUDE_CODE.to_string()]
        );
        assert_eq!(
            declared.posture_models.get(&Posture::Auto),
            Some(&vec![
                "claude-opus-5".to_string(),
                "claude-fable-5".to_string(),
                "claude-sonnet-5".to_string()
            ])
        );
        assert_eq!(declared.posture_models.len(), 1, "no other posture is held");
        assert_eq!(declared.validate(), Ok(()));
    }

    /// Fleet's three words onto Claude Code's modes (ruling 14), one each.
    #[test]
    fn a_posture_is_the_permission_mode_ruling_fourteen_maps_it_to() {
        assert_eq!(permission_mode(Posture::Ask), "default");
        assert_eq!(permission_mode(Posture::Auto), "auto");
        assert_eq!(permission_mode(Posture::Unattended), "dontAsk");
    }

    /// The launch's argv is the interactive one — the resolved binary, the name,
    /// the model, the posture as its mode, the plugin root where one is handed,
    /// then the first turn — and its environment is the request's with this
    /// agent's own three beside it.
    #[test]
    fn a_launch_answers_the_interactive_argv_and_the_requests_environment_with_its_own() {
        let agent = scratch_agent().with_plugin_dir(Some(PathBuf::from("/an/overlay")));
        let argv = agent.launch(&a_launch()).expect("the launch is built");
        assert_eq!(
            argv.argv,
            vec![
                "/opt/claude/bin/claude",
                "--name",
                "builder-9739a",
                "--model",
                "claude-opus-5",
                "--permission-mode",
                "dontAsk",
                "--plugin-dir",
                "/an/overlay",
                "/wake builder-9739a",
            ]
        );
        assert_eq!(
            argv.env.get("FLEET_ACTOR").map(String::as_str),
            Some(format!("seat:{SEAT_ID}").as_str()),
            "the request's own variables are carried"
        );
        assert_eq!(
            argv.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/nowhere/.claude"),
            "a named seat comes up under the adapter's own directory"
        );
        assert_eq!(
            argv.env
                .get("CLAUDE_SECURESTORAGE_CONFIG_DIR")
                .map(String::as_str),
            Some(""),
            "the credential scope is defined, and empty"
        );

        // The control: no plugin root, no such element.
        let bare = scratch_agent().launch(&a_launch()).unwrap();
        assert!(
            !bare.argv.iter().any(|word| word == "--plugin-dir"),
            "{bare:?}"
        );

        // Effects off: nothing is built at all.
        assert!(matches!(
            reading_through("claude").launch(&a_launch()),
            Err(AgentError::Unreadable(_))
        ));
    }

    /// A resume is the full id with the start's own flags — the model, the posture
    /// and the plugin root — no name and no first turn, under this agent's own
    /// variables and the request's directory (reviewer call 2026-09-25, E3;
    /// measured on 2.1.280 by fleet-rge6.4). Who the session acts as is fleet's
    /// to set, and a resume request carries none.
    #[test]
    fn a_resume_is_the_full_id_with_the_starts_flags() {
        let agent = scratch_agent().with_plugin_dir(Some(PathBuf::from("/an/overlay")));
        let argv = agent.resume(&a_resume()).expect("the resume is built");
        assert_eq!(
            argv.argv,
            vec![
                "/opt/claude/bin/claude",
                "--resume",
                "663267a3-2b6e-44ac-b0f1-cbfd24bbbc5d",
                "--model",
                "claude-haiku-4-5",
                "--permission-mode",
                "auto",
                "--plugin-dir",
                "/an/overlay",
            ]
        );
        assert_eq!(
            argv.env.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/nowhere/seats/builder-9739a")
        );
        assert!(!argv.env.contains_key("FLEET_ACTOR"), "{argv:?}");
    }

    /// A launch opened with a permissions template renders the request's
    /// permissions into the worktree's own settings file before anything starts,
    /// and a launch opened with none writes nothing there.
    #[test]
    fn a_launch_renders_the_requests_permissions_into_the_worktree() {
        let root = std::env::temp_dir().join(format!("cc-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let worktree = root.join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        let template = r#"{"permissions":{"allow":["Bash({touched})","Edit(/{worktree}/**)"]}}"#;
        let mut launch = a_launch();
        launch.worktree = worktree.display().to_string();
        launch.permissions = Permissions {
            commands: vec!["cargo".to_string()],
            touched: Some("make \"check\"".to_string()),
        };
        scratch_agent()
            .with_permissions(Some(template.to_string()))
            .launch(&launch)
            .expect("the launch is built");
        let written: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(worktree.join(LOCAL_SETTINGS)).expect("the file is written"),
        )
        .expect("the file is JSON");
        assert_eq!(
            written["permissions"]["allow"],
            serde_json::json!([
                "Bash(make \"check\")",
                format!("Edit(/{}/**)", worktree.display()),
                "Bash(cargo:*)",
            ])
        );

        // The control: an adapter opened with no template writes nothing.
        let bare = root.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        launch.worktree = bare.display().to_string();
        scratch_agent()
            .launch(&launch)
            .expect("the launch is built");
        assert!(!bare.join(".claude").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The render's three rules: no `{touched}` rule for a request that carries no
    /// command, the template's own bytes where there are no commands to add, and a
    /// placeholder it has no value for refused, never written.
    #[test]
    fn the_permissions_render_drops_an_unhanded_rule_and_refuses_an_unknown_placeholder() {
        let worktree = Path::new("/wt/a");
        let with_touched = "{\"permissions\":{\"allow\":[\"Bash({touched})\",\"Read\"]}}";
        let none = render_permissions(with_touched, &Permissions::default(), worktree).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&none).unwrap();
        assert_eq!(doc["permissions"]["allow"], serde_json::json!(["Read"]));

        let plain = "{\"allow\":\"Edit(/{worktree}/**)\"}";
        assert_eq!(
            render_permissions(plain, &Permissions::default(), worktree).unwrap(),
            "{\"allow\":\"Edit(//wt/a/**)\"}",
            "no commands: the template's own bytes with the value in"
        );

        let unknown = render_permissions("{\"x\":\"{seat}\"}", &Permissions::default(), worktree);
        assert!(unknown.unwrap_err().contains("`{seat}`"));
    }

    // ---- read: the recorded listing -------------------------------------------------

    /// THE LISTING RECORDED ON THE SUPPORTED RELEASE (fleet-rge6.3's first step,
    /// reviewer call E14): Claude Code 2.1.280, tmux 3.7b, 2026-09-26, a scratch
    /// configuration directory seeded with onboarding and the worktree's trust, a
    /// scratch socket, `DISABLE_AUTOUPDATER=1` in the pane. Byte for byte as the
    /// agent printed them, less the whitespace; the pane listed `pid=31697 dead=0`
    /// beside every one of the three rows.
    const RECORDED_IDLE: &str = r#"[{"pid":31697,"cwd":"/private/tmp/fleet-measure-rge63/wt","kind":"interactive","startedAt":1790414327458,"sessionId":"d0090b9b-6edf-4cfa-8309-d14b2f1e70b4","name":"ma","status":"idle"}]"#;
    const RECORDED_BUSY: &str = r#"[{"pid":31697,"cwd":"/private/tmp/fleet-measure-rge63/wt","kind":"interactive","startedAt":1790414327458,"sessionId":"d0090b9b-6edf-4cfa-8309-d14b2f1e70b4","name":"ma","status":"busy"}]"#;
    const RECORDED_WAITING: &str = r#"[{"pid":31697,"cwd":"/private/tmp/fleet-measure-rge63/wt","kind":"interactive","startedAt":1790414327458,"sessionId":"d0090b9b-6edf-4cfa-8309-d14b2f1e70b4","name":"ma","status":"waiting","waitingFor":"permission prompt"}]"#;
    const RECORDED_GONE: &str = "[]";
    const RECORDED_PID: u32 = 31_697;
    const RECORDED_SESSION: &str = "d0090b9b-6edf-4cfa-8309-d14b2f1e70b4";
    const RECORDED_CWD: &str = "/private/tmp/fleet-measure-rge63/wt";

    /// The seat asked about by its pane's pid alone, as a poll asks about a pane
    /// whose session nobody has read yet.
    fn by_pid(pid: u32) -> SeatRef {
        SeatRef {
            seat: id(SEAT_ID),
            session_id: None,
            pid: Some(pid),
            config_dir: None,
            worktree: RECORDED_CWD.to_string(),
            screen: None,
        }
    }

    /// One seat read off one listing body, with no transcript to read.
    fn read_one(seat: &SeatRef, body: &str) -> SeatActivity {
        let body = body.to_string();
        readings_from(
            std::slice::from_ref(seat),
            &|_: Option<&str>| Ok(body.clone()),
            &|_: &SeatRef, _: &str| None,
        )
        .remove(0)
    }

    mod lessons {
        use super::*;

        /// claude-code B1 — the roster is one command, and the reader tolerates
        /// fields it does not know. Field presence is kind-dependent, so a reader
        /// that requires a field on every row fails on the first mixed listing:
        /// here a background row beside the recorded interactive one.
        #[test]
        fn the_roster_is_one_command() {
            let recorded = RECORDED_IDLE.trim_start_matches('[').trim_end_matches(']');
            let mixed = format!(
                r#"[
                  {{"id":"aa","sessionId":"aa","cwd":"/wt/builder-1","kind":"background",
                   "pid":1,"status":"idle","state":"running","name":"orla","startedAt":10,
                   "someFieldNobodyHasSeen":"harmless"}},
                  {recorded}
                ]"#
            );
            let rows = parse_listing(&mixed).expect("the listing must parse");
            assert_eq!(rows.len(), 2, "both kinds survive one read");
            assert_eq!(rows[1].pid, Some(RECORDED_PID));
            let seen = read_one(&by_pid(RECORDED_PID), &mixed);
            assert_eq!(seen.activity, Activity::Idle);
            assert_eq!(seen.session_id.as_deref(), Some(RECORDED_SESSION));
        }

        /// claude-code B2 — there is no token figure anywhere in the listing, so
        /// context accounting cannot come from it. A number that looks like one on
        /// a row is not the seat's context: the transcript is.
        #[test]
        fn the_roster_carries_no_token_field() {
            let listed = r#"[{"sessionId":"aa","cwd":"/wt/builder-1","kind":"interactive",
                 "pid":4242,"status":"idle","startedAt":10,"tokens":999999,"input_tokens":999999}]"#;
            let seen = read_one(&by_pid(4242), listed);
            assert_eq!(seen.activity, Activity::Idle);
            let context = context_of(&by_pid(4242), None, None);
            assert_eq!(context.tokens, None, "999999 has no path into a reading");

            let from_transcript =
                context_tokens_in(r#"{"type":"assistant","message":{"usage":{"input_tokens":7}}}"#);
            assert_eq!(
                from_transcript,
                Some(7),
                "the reading is the transcript's, and 999999 has no path into it"
            );
        }

        /// claude-code B4 — the listing has been observed answering with zero bytes
        /// and a success status while sessions were live. Empty is UNREADABLE,
        /// never a reading of zero; the control is an empty JSON array, which is a
        /// listing that answered and said there is nothing.
        #[test]
        fn the_roster_read_can_go_silently_dead() {
            let silent = read_one(&by_pid(RECORDED_PID), "");
            assert_eq!(silent.activity, Activity::Unknown);
            assert!(
                silent
                    .cause
                    .as_deref()
                    .is_some_and(|cause| cause.contains("zero bytes")),
                "the cause travels with the unknown: {silent:?}"
            );
            assert_eq!(silent.session_id, None);

            let answered = read_one(&by_pid(RECORDED_PID), RECORDED_GONE);
            assert_eq!(
                answered.activity,
                Activity::Starting,
                "a listing that answered and said nothing is not the same read"
            );
        }

        /// claude-code B5 — `cwd` names a seat and proves nothing. A row is the
        /// seat's by its session id, else by the PANE's pid, whatever directory it
        /// stands in, and a row in the seat's worktree with neither is never the
        /// seat's.
        #[test]
        fn cwd_names_a_seat_and_proves_nothing() {
            let listing = format!(
                r#"[{{"sessionId":"not-mine","cwd":"{RECORDED_CWD}","pid":4242,"status":"busy"}},
                    {{"sessionId":"mine","cwd":"/somewhere/else","pid":{RECORDED_PID},"status":"idle"}}]"#
            );
            let seen = read_one(&by_pid(RECORDED_PID), &listing);
            assert_eq!(
                seen.session_id.as_deref(),
                Some("mine"),
                "the pid attributes the row, and the directory does not"
            );
            assert_eq!(seen.activity, Activity::Idle);

            // The control: a pane whose pid no row carries is found by nothing, the
            // row standing in its worktree included.
            let nobody = read_one(&by_pid(7), &listing);
            assert_eq!(nobody.session_id, None);
            assert_eq!(nobody.activity, Activity::Starting);
        }

        /// claude-code C1 — the transcript path is an encoding, and every context
        /// instrument resolves the same way, so a change to it blinds them all at
        /// once. The rule is EVERY non-alphanumeric character, not the separator
        /// alone: a worktree carrying a dot, an underscore or a space is the case a
        /// separator-only reader publishes a null context for forever.
        #[test]
        fn the_transcript_path_encoding() {
            let path = transcript_path(Path::new("/home/av/.claude"), "/wt/builder-1", "aa-bb");
            assert_eq!(
                path,
                Path::new("/home/av/.claude/projects/-wt-builder-1/aa-bb.jsonl")
            );

            assert_eq!(
                encode_project_dir("/Users/av/.claude/jobs/tmp"),
                "-Users-av--claude-jobs-tmp",
                "a dot is a dash, and a dot after a separator is two"
            );
            assert_eq!(
                encode_project_dir("/wt/my_seat/a b.c"),
                "-wt-my-seat-a-b-c",
                "an underscore, a space and a dot are all dashes"
            );
            assert_eq!(encode_project_dir("plain123"), "plain123");
            // Measured on 2.1.280: the scoped session's transcript landed at
            // `<config dir>/projects/-private-tmp-fleet-measure-rge63-wt/<id>.jsonl`.
            assert_eq!(
                encode_project_dir(RECORDED_CWD),
                "-private-tmp-fleet-measure-rge63-wt"
            );
        }

        /// claude-code B8 — one field says a session is stopped in front of a
        /// human, and it is keyed on PRESENCE. The vocabulary is the agent's, so a
        /// cause this fleet has never seen must still stop the seat rather than
        /// read as a healthy one; the control is the same row without the field.
        ///
        /// On the INTERACTIVE row the recording read (B10): `waiting` and
        /// `permission prompt` at the approval dialog.
        #[test]
        fn waiting_for_names_the_block() {
            let blocked = read_one(&by_pid(RECORDED_PID), RECORDED_WAITING);
            assert_eq!(blocked.activity, Activity::Blocked);
            assert_eq!(blocked.blocked_on, Some(BlockedOn::Permission));
            assert_eq!(blocked.cause.as_deref(), Some("permission prompt"));
            assert_eq!(blocked.evidence, Evidence::Typed);
            assert_eq!(blocked.session_id.as_deref(), Some(RECORDED_SESSION));

            let unrecognised = read_one(
                &by_pid(RECORDED_PID),
                &RECORDED_BUSY.replace(
                    r#""status":"busy""#,
                    r#""status":"busy","waitingFor":"a cause nobody has enumerated""#,
                ),
            );
            assert_eq!(
                unrecognised.activity,
                Activity::Blocked,
                "presence, never the value: an unknown cause still stops the seat"
            );
            assert_eq!(
                unrecognised.blocked_on, None,
                "and names no reason it cannot"
            );
            assert_eq!(
                unrecognised.cause.as_deref(),
                Some("a cause nobody has enumerated")
            );

            // `waiting` with no cause beside it is a block all the same.
            let causeless = read_one(
                &by_pid(RECORDED_PID),
                &RECORDED_WAITING.replace(r#","waitingFor":"permission prompt""#, ""),
            );
            assert_eq!(causeless.activity, Activity::Blocked);
            assert_eq!(causeless.cause.as_deref(), Some("status waiting"));

            let control = read_one(&by_pid(RECORDED_PID), RECORDED_BUSY);
            assert_eq!(control.activity, Activity::Busy);
            assert_eq!(control.cause, None);
        }

        /// claude-code B10 — an interactive session is listed WITHOUT AN ADDRESS,
        /// its pid is the pane's, its activity is a three-word status and its
        /// blocked cause is typed. Re-measured on the supported 2.1.280 (reviewer
        /// call E14; B10 was first read on 2.1.282), and the recording is the
        /// fixture.
        #[test]
        fn an_interactive_row_is_listed_without_an_address() {
            for (body, activity) in [
                (RECORDED_IDLE, Activity::Idle),
                (RECORDED_BUSY, Activity::Busy),
                (RECORDED_WAITING, Activity::Blocked),
            ] {
                assert!(
                    !body.contains("\"id\""),
                    "no address on an interactive row: {body}"
                );
                let seen = read_one(&by_pid(RECORDED_PID), body);
                assert_eq!(seen.activity, activity, "{body}");
                assert_eq!(seen.evidence, Evidence::Typed);
                assert_eq!(seen.session_id.as_deref(), Some(RECORDED_SESSION));
            }
            // `kill-session`: the next read lists nothing, and a pane still asked
            // about by its pid is a pane with no row — starting, never an end,
            // which is the host's reading to make.
            let gone = read_one(&by_pid(RECORDED_PID), RECORDED_GONE);
            assert_eq!(gone.activity, Activity::Starting);
            assert_eq!(gone.session_id, None);
        }

        /// claude-code C2 — the entry shape: there is no single context number, and
        /// the reading is the arithmetic over the input tokens and both cache
        /// figures on the LAST main-chain assistant entry.
        #[test]
        fn the_transcript_entry_shape() {
            let body = r#"
    {"type":"user","message":{"usage":{"input_tokens":900}}}
    {"type":"assistant","message":{"usage":{"input_tokens":1,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}}
    {"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":30}}}
    {"type":"assistant","message":{"usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}
    {"type":"assistant","message":{"usage":{"inp"#;
            assert_eq!(
                context_tokens_in(body),
                Some(60),
                "the last entry stating a window, summed over all three figures"
            );
            assert_eq!(
                context_tokens_in(r#"{"type":"assistant","message":{}}"#),
                None,
                "an entry with no usage block states no window"
            );
            assert_eq!(context_tokens_in(""), None);
        }

        /// claude-code C3 — a sidechain entry is a subagent's turn carrying the
        /// subagent's window. The flag is on every entry, so the skip is a filter
        /// and not an inference; the control below is the same file with the flag
        /// cleared, where the entry IS the reading.
        #[test]
        fn sidechains_carry_another_window() {
            let with_subagent = r#"
    {"type":"assistant","isSidechain":false,"message":{"usage":{"input_tokens":11}}}
    {"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":2600000}}}"#;
            assert_eq!(context_tokens_in(with_subagent), Some(11));

            let control = with_subagent.replace("\"isSidechain\":true", "\"isSidechain\":false");
            assert_eq!(
                context_tokens_in(&control),
                Some(2_600_000),
                "the control must read the entry the skip drops, or the skip proves nothing"
            );

            let absent_flag = r#"{"type":"assistant","message":{"usage":{"input_tokens":42}}}"#;
            assert_eq!(
                context_tokens_in(absent_flag),
                Some(42),
                "an absent flag reads as main chain"
            );
        }

        /// claude-code A11 — the configuration directory scopes the provider's
        /// listing: a session started under a per-row directory is listed under
        /// that directory and under no other, so every read about that session has
        /// to be made under the same directory. It held for interactive rows too
        /// (B10, re-read on 2.1.280: the scratch directory's listing named the
        /// session and its transcript landed under it).
        ///
        /// The fixture is the FOLD: the directories asked for are recorded, and
        /// each seat is read off the listing that could see it.
        #[test]
        fn the_config_dir_scopes_the_daemon() {
            let per_row_dir = "/machine/config/builder-9";
            let named = by_pid(1111);
            let spawned = SeatRef {
                seat: id(OTHER_ID),
                config_dir: Some(format!("{per_row_dir}/")),
                ..by_pid(2222)
            };
            let fleets = r#"[{"sessionId":"named","pid":1111,"status":"idle"}]"#;
            let asked = std::cell::RefCell::new(Vec::new());
            let listing = |dir: Option<&str>| {
                asked.borrow_mut().push(dir.map(str::to_string));
                Ok(match dir {
                    None => fleets.to_string(),
                    Some(_) => {
                        r#"[{"sessionId":"spawned","pid":2222,"status":"busy"}]"#.to_string()
                    }
                })
            };
            let seen = readings_from(
                &[named, spawned.clone(), spawned.clone()],
                &listing,
                &|_: &SeatRef, _: &str| None,
            );

            // ONE READ PER DISTINCT DIRECTORY: a read that listed once could not
            // see the spawned session at all, and one per seat would ask the same
            // directory twice.
            assert_eq!(
                asked.into_inner(),
                vec![None, Some(per_row_dir.to_string())],
                "the fleet's directory and the row's own, once each"
            );
            assert_eq!(seen.len(), 3, "one reading per seat asked");
            assert_eq!(seen[0].session_id.as_deref(), Some("named"));
            assert_eq!(seen[1].session_id.as_deref(), Some("spawned"));
            assert_eq!(seen[1].activity, Activity::Busy);

            // The control: the same spawned seat asked under the fleet's own
            // directory is found nowhere.
            let unscoped = SeatRef {
                config_dir: None,
                ..spawned
            };
            assert_eq!(read_one(&unscoped, fleets).session_id, None);
        }
    }

    /// A session id the agent has is found by it, whatever the pid, and one it no
    /// longer has falls to the pane's pid — measured on 2.1.280, `/clear` gives the
    /// same pid a new session id — and the answer teaches the seat the new one
    /// (fleet-jymr.3).
    #[test]
    fn a_seat_is_found_by_its_session_else_by_its_panes_pid() {
        let listing = r#"[{"sessionId":"old","pid":1,"status":"busy","startedAt":1},
                          {"sessionId":"new","pid":2,"status":"idle","startedAt":2}]"#;
        let by_session = SeatRef {
            session_id: Some("old".to_string()),
            ..by_pid(2)
        };
        let seen = read_one(&by_session, listing);
        assert_eq!(seen.session_id.as_deref(), Some("old"), "the id wins");
        assert_eq!(seen.activity, Activity::Busy);

        let cleared = SeatRef {
            session_id: Some("before-the-clear".to_string()),
            ..by_pid(2)
        };
        let seen = read_one(&cleared, listing);
        assert_eq!(
            seen.session_id.as_deref(),
            Some("new"),
            "an id no row carries falls to the pid, and the answer names the row's"
        );

        // Two rows under one pid: the newest started.
        let twice = r#"[{"sessionId":"older","pid":5,"status":"busy","startedAt":10},
                        {"sessionId":"newer","pid":5,"status":"idle","startedAt":20}]"#;
        assert_eq!(
            read_one(&by_pid(5), twice).session_id.as_deref(),
            Some("newer")
        );
    }

    /// A row listed before it carries a status is starting, and a status this
    /// adapter has no word for is unknown naming it — each carrying the session
    /// the row names.
    #[test]
    fn a_row_with_no_status_is_starting_and_one_with_an_unknown_status_names_it() {
        let listed = read_one(&by_pid(9), r#"[{"sessionId":"s","pid":9}]"#);
        assert_eq!(listed.activity, Activity::Starting);
        assert_eq!(listed.session_id.as_deref(), Some("s"));

        let odd = read_one(
            &by_pid(9),
            r#"[{"sessionId":"s","pid":9,"status":"napping"}]"#,
        );
        assert_eq!(odd.activity, Activity::Unknown);
        assert!(odd.cause.as_deref().is_some_and(|c| c.contains("napping")));
        assert_eq!(odd.session_id.as_deref(), Some("s"));
    }

    /// An unreadable listing is its OWN directory's: the seats under it read
    /// unknown with the cause, and a seat under another directory is read as usual.
    #[test]
    fn an_unreadable_listing_leaves_only_its_own_directorys_seats_unknown() {
        let fine = by_pid(1);
        let broken = SeatRef {
            seat: id(OTHER_ID),
            config_dir: Some("/broken".to_string()),
            ..by_pid(2)
        };
        let seen = readings_from(
            &[fine, broken],
            &|dir: Option<&str>| match dir {
                None => Ok(r#"[{"sessionId":"a","pid":1,"status":"idle"}]"#.to_string()),
                Some(_) => Err("`claude agents --json --all` exited 1: no".to_string()),
            },
            &|_: &SeatRef, _: &str| None,
        );
        assert_eq!(seen[0].activity, Activity::Idle);
        assert_eq!(seen[1].activity, Activity::Unknown);
        assert!(seen[1]
            .cause
            .as_deref()
            .is_some_and(|cause| cause.contains("exited 1")));
    }

    // ---- the logged-out answer ------------------------------------------------------

    /// The provider's logged-out first turn, as it was read off a real transcript on
    /// this box on 2026-09-12 (2.1.261): one synthetic assistant entry carrying the
    /// cause, the flag and a window of zero.
    fn logged_out_body() -> String {
        concat!(
            r#"{"type":"user","isSidechain":false,"message":{"role":"user"}}"#,
            "\n",
            r#"{"type":"assistant","isSidechain":false,"isApiErrorMessage":true,"#,
            r#""error":"authentication_failed","message":{"model":"<synthetic>","#,
            r#""usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"#,
            r#""cache_read_input_tokens":0},"content":[{"type":"text","#,
            r#""text":"Not logged in · Please run /login"}]}}"#,
            "\n",
        )
        .to_string()
    }

    /// A first turn that ANSWERED, from the same probe's other arm: a real
    /// assistant entry with a non-zero window.
    fn answered_body() -> String {
        concat!(
            r#"{"type":"user","isSidechain":false,"message":{"role":"user"}}"#,
            "\n",
            r#"{"type":"assistant","isSidechain":false,"message":{"model":"claude-sonnet-4-5","#,
            r#""usage":{"input_tokens":10,"output_tokens":219,"#,
            r#""cache_creation_input_tokens":36062,"cache_read_input_tokens":0}}}"#,
            "\n",
        )
        .to_string()
    }

    /// AC3 — the reader's three terms, each one alone insufficient. The conjunction
    /// is the safe direction: a term that moves in a later release yields NO
    /// reading, and a reading nobody has costs one uncaught logged-out seat where a
    /// looser match would fail a dispatch that was fine.
    #[test]
    fn the_logged_out_reader_needs_all_three_terms() {
        assert!(logged_out_first_turn(&logged_out_body()));
        // The flag alone: an entry the provider wrote for some other cause.
        let other_cause = logged_out_body().replace("authentication_failed", "overloaded_error");
        assert!(!logged_out_first_turn(&other_cause));
        // The cause alone, on an entry the provider did not write itself.
        let not_flagged = logged_out_body().replace(r#""isApiErrorMessage":true,"#, "");
        assert!(!logged_out_first_turn(&not_flagged));
        // The pair, with a window: not a turn that never reached the model.
        let with_window = logged_out_body().replace(r#""input_tokens":0"#, r#""input_tokens":42"#);
        assert!(!logged_out_first_turn(&with_window));
        // And the FIRST entry is the one read: a session that answered and later
        // met an auth failure was not a failed dispatch.
        let later = format!("{}{}", answered_body(), logged_out_body());
        assert!(!logged_out_first_turn(&later));
    }

    /// An IDLE session whose first turn was the logged-out answer reads BLOCKED ON
    /// LOGGED_OUT: live and idle on the listing, and only its transcript says
    /// otherwise (lessons claude-code A11). The controls are the same row over a
    /// transcript that answered, and over none.
    #[test]
    fn an_idle_session_whose_first_turn_was_logged_out_is_blocked_on_it() {
        let seat = by_pid(RECORDED_PID);
        let read_over = |body: Option<String>| {
            readings_from(
                std::slice::from_ref(&seat),
                &|_: Option<&str>| Ok(RECORDED_IDLE.to_string()),
                &|_: &SeatRef, _: &str| body.clone(),
            )
            .remove(0)
        };
        let logged_out = read_over(Some(logged_out_body()));
        assert_eq!(logged_out.activity, Activity::Blocked);
        assert_eq!(logged_out.blocked_on, Some(BlockedOn::LoggedOut));
        assert_eq!(logged_out.session_id.as_deref(), Some(RECORDED_SESSION));

        assert_eq!(read_over(Some(answered_body())).activity, Activity::Idle);
        assert_eq!(read_over(None).activity, Activity::Idle);
    }

    // ---- context --------------------------------------------------------------------

    /// The turn reader beside the window reader: the SAME filter, one step shorter.
    ///
    /// The two part company on exactly one entry shape, and the fixture is built so
    /// they must answer differently: four main-chain assistant entries carry a
    /// usage block and one of them sums to zero, so the window reader answers the
    /// last non-zero and the turn reader answers 4. A reader that had copied the
    /// window's own filter would answer 3 here.
    #[test]
    fn the_turn_reader_counts_the_entry_the_window_reader_skips() {
        let body = r#"
    {"type":"user","message":{"usage":{"input_tokens":900}}}
    {"type":"assistant","message":{"usage":{"input_tokens":1}}}
    {"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":2600000}}}
    {"type":"assistant","message":{"usage":{"input_tokens":0,"cache_read_input_tokens":0}}}
    {"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":20}}}
    {"type":"assistant","message":{"usage":{"input_tokens":40,"cache_read_input_tokens":20}}}
    {"type":"assistant","message":{"usage":{"inp"#;
        assert_eq!(turns_in(body), 4, "the zero-usage entry is a turn");
        assert_eq!(
            context_tokens_in(body),
            Some(60),
            "and the window reader still skips it, which is what makes the count above a second \
             reading rather than a copy"
        );
        assert_eq!(
            turns_in(r#"{"type":"assistant","message":{}}"#),
            0,
            "no usage block is no turn"
        );
        assert_eq!(
            turns_in(""),
            0,
            "and an empty transcript is a measured zero"
        );
        // The sidechain control: with the flag cleared, the entry IS counted — so
        // the skip is a filter and not an inference.
        let control = body.replace("\"isSidechain\":true", "\"isSidechain\":false");
        assert_eq!(
            turns_in(&control),
            5,
            "the control must count the entry the skip drops, or the skip proves nothing"
        );
    }

    /// AC2 — the transcript is read under the SEAT'S directory. The adapter
    /// resolves the path from the directory the request names, so a body written
    /// under one is unreadable through the other; and a context row carries the
    /// window, the turns and the file's last write, or the seat's id alone where
    /// nothing resolves.
    #[test]
    fn a_seats_context_is_read_under_the_directory_it_is_asked_with() {
        let root = std::env::temp_dir().join(format!("cc-context-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fleet_dir = root.join("fleet-config");
        let row_dir = root.join("row-config");
        let body = "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":48210}}}\n";
        let path = transcript_path(&row_dir, "/wt/builder-1", "a-session");
        std::fs::create_dir_all(path.parent().expect("the transcript has a parent")).unwrap();
        std::fs::write(&path, body).unwrap();

        let agent = ClaudeCode::with_seams(
            "claude".to_string(),
            fleet_dir,
            Duration::from_secs(1),
            String::new(),
            None,
            String::new(),
        );
        let asked = SeatRef {
            seat: id(SEAT_ID),
            session_id: Some("a-session".to_string()),
            pid: None,
            config_dir: Some(row_dir.display().to_string()),
            // A configured trailing separator encodes to one the agent never
            // wrote, so the worktree is put in the one form first.
            worktree: "/wt/builder-1/".to_string(),
            screen: None,
        };
        let unscoped = SeatRef {
            seat: id(OTHER_ID),
            config_dir: None,
            ..asked.clone()
        };
        let read = agent
            .context(&[asked, unscoped])
            .expect("context answers in-process");
        assert_eq!(read[0].tokens, Some(48_210));
        assert_eq!(read[0].turns, Some(1));
        assert_eq!(read[0].window, None, "the transcript states no window");
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(
            read[0].last_write.as_ref().map(Stamp::as_str),
            crate::clock::stamp_of(written).as_deref(),
            "the last write is the file's own mtime"
        );
        // The control: the same session under the adapter's own directory resolves
        // nothing, so the directory is what decided it.
        assert_eq!(read[1].seat, id(OTHER_ID));
        assert_eq!(read[1].tokens, None);
        assert_eq!(read[1].turns, None);
        assert_eq!(read[1].last_write, None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
