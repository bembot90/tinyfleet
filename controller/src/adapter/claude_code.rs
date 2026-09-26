//! The Claude Code adapter — the first implementation of the seam.

use super::{
    transcript_path, Agent, AgentRow, DaemonRead, DaemonStatus, Launch, RemoveAnswer, RosterRead,
    StartSpec,
};
use crate::platform::run_bounded;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct ClaudeCode {
    pub bin: String,
    /// Where this agent keeps its own state, which is where the transcripts are.
    pub config_dir: PathBuf,
    pub timeout: Duration,
    /// This machine's fleet directory, under which a nudge's own words are
    /// kept. They go to ONE FILE, because a pipe waits for EOF rather than for
    /// the child and any process still holding the write end keeps the caller
    /// blocked (lessons claude-code D2). A start's words are no longer this
    /// adapter's: the session runs in a pane, which holds them.
    pub machine_dir: PathBuf,
    /// The `PATH` every child this adapter spawns carries, constructed by the
    /// platform layer and never inherited (D1).
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
    /// The ABSOLUTE binary the four effect verbs exec, resolved once by
    /// [`ClaudeCode::resolve_effect_bin`] against the constructed `PATH` and
    /// handed in.
    ///
    /// Separate from `bin`, which is observe's and may be a bare name: the two
    /// answer different questions, and a controller that gated on one and
    /// exec'd the other would act through a binary nothing checked. `None` is a
    /// binary that did not resolve, which is effects OFF — every verb below
    /// refuses rather than falling back to a name it would discover at the
    /// spawn.
    effect_bin: Option<PathBuf>,
    /// This process's own executable, which every child is handed as
    /// [`FLEET_BIN_VAR`] so the plugin's hooks in a session this fleet spawns run
    /// the binary that spawned it. `None` is an executable this process cannot
    /// name, and the child is then handed nothing rather than a guess.
    ///
    /// Not a seam either constructor takes: it is WHICH PROCESS THIS IS, the same
    /// answer for every arm of a suite, and a caller handing in some other file
    /// would be naming a binary the session's hooks were never run against.
    fleet_bin: Option<PathBuf>,
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

/// This agent's pre-tool hook, as the `[hook]` table an agent adapter's
/// `adapter.toml` declares it: what `fleet guard` reads where no installed
/// pack carries [`NAME`]. It lives beside the in-process adapter, and goes
/// with it.
pub const HOOK_MANIFEST: &str = include_str!("claude_code.hook.toml");

impl ClaudeCode {
    pub fn new(home: &Path, machine_dir: &Path) -> Self {
        Self {
            bin: bin_from(configured_bin().as_deref()),
            config_dir: config_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref(), home),
            timeout: timeout_from(std::env::var("FLEET_AGENT_TIMEOUT_MS").ok().as_deref()),
            machine_dir: machine_dir.to_path_buf(),
            child_path: crate::platform::child_path(home),
            credential_dir: credential_dir_from(std::env::var("CLAUDE_CONFIG_DIR").ok().as_deref()),
            // Observe's alone until a caller hands one in: an adapter built for
            // reading issues no effect, and a `None` here refuses every verb
            // rather than letting one fall back to `bin`.
            effect_bin: None,
            fleet_bin: own_executable(),
        }
    }

    /// The adapter with the binary its effects exec.
    ///
    /// Handed in rather than resolved here, so the ONE resolution the loop
    /// already made — the one whose failure turns effects off and whose cause
    /// the projection publishes — is the same value the four verbs run.
    pub fn with_effect_bin(mut self, bin: PathBuf) -> Self {
        self.effect_bin = Some(bin);
        self
    }

    /// An adapter whose seams are all handed in, for a caller that has resolved
    /// them itself. `new` reads the process environment, which a suite running
    /// arms in parallel shares and cannot vary per arm.
    pub fn with_seams(
        bin: String,
        config_dir: PathBuf,
        timeout: Duration,
        machine_dir: PathBuf,
        child_path: String,
        effect_bin: Option<PathBuf>,
        credential_dir: String,
    ) -> Self {
        Self {
            bin,
            config_dir,
            timeout,
            machine_dir,
            child_path,
            credential_dir,
            effect_bin,
            fleet_bin: own_executable(),
        }
    }

    /// The binary an effect execs, resolved once.
    ///
    /// `FLEET_CLAUDE_BIN` when it names an ABSOLUTE path, and a relative one is
    /// refused rather than resolved: a relative path is read against whatever
    /// directory the service manager left this process in, which is a different
    /// file per host. Otherwise the first `claude` on the constructed PATH.
    /// `None` is a binary this controller cannot exec, which turns effects off
    /// and never becomes a bare name it discovers at the spawn.
    pub fn resolve_effect_bin(
        configured: Option<&str>,
        child_path: &str,
    ) -> Result<PathBuf, String> {
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

    /// A command against a program that has already been chosen, with
    /// [`ClaudeCode::environment`] on it and nothing else.
    ///
    /// It chooses no program. Its two callers below do, and they choose
    /// differently on purpose.
    fn with_environment(&self, program: &str, config_dir: Option<&Path>) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for (key, value) in self.environment(config_dir) {
            cmd.env(key, value);
        }
        cmd
    }

    /// The whole environment a child of this adapter carries — a command's and
    /// a session's alike, so the call that reads a listing and the pane that
    /// holds the session it lists agree on every value.
    ///
    /// NOTHING IS INHERITED (lessons claude-code D1). A service-launched process
    /// carries a minimal `PATH`, and a `claude` that inherits it starts a daemon
    /// in that environment — after which every session claimed from that daemon
    /// carries the collapsed search path, long after the start that caused it.
    /// So the environment is cleared and rebuilt: the constructed `PATH`, the
    /// four values a shell needs to be a shell, the agent's own configuration
    /// directory when this adapter is scoped to one, the credential scope that
    /// directory would otherwise move off the operator's own login, and this
    /// process's own executable as the binary the plugin's hooks run.
    ///
    /// `config_dir` is the ONE per-child override: a start that names its own
    /// configuration directory comes up under that one instead of the adapter's:
    /// each spawned seat has its own, holding only the pack's overlay. The
    /// credential knob beside it is unchanged either way — it is the operator's
    /// own configured value, and a child whose two variables agree is the
    /// logged-out child (A11).
    pub fn environment(&self, config_dir: Option<&Path>) -> Vec<(String, String)> {
        let mut env = vec![("PATH".to_string(), self.child_path.clone())];
        for pass in PASSED_THROUGH {
            if let Ok(value) = std::env::var(pass) {
                env.push((pass.to_string(), value));
            }
        }
        env.push((
            "CLAUDE_CONFIG_DIR".to_string(),
            config_dir.unwrap_or(&self.config_dir).display().to_string(),
        ));
        // Set on EVERY child, defined even when empty: unset falls back to the
        // suffixed credential lookup, which is the logged-out child.
        env.push((
            "CLAUDE_SECURESTORAGE_CONFIG_DIR".to_string(),
            self.credential_dir.clone(),
        ));
        // Set on EVERY child and not only the start, for the reason the PATH
        // above is: any call can be the one that starts the agent's background
        // daemon (lessons claude-code D1), and a session holds what its own
        // process was handed. Without it the plugin's shim looks for a build
        // under its own root, and a root holding none blocks every Bash command
        // the session makes.
        if let Some(bin) = &self.fleet_bin {
            env.push((FLEET_BIN_VAR.to_string(), bin.display().to_string()));
        }
        env
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

    /// The four EFFECTS against the absolute binary `resolve_effect_bin`
    /// returned, and against nothing else.
    ///
    /// It asks the process's search path nothing. The binary the gate checked
    /// and the binary an effect execs have to be one value, or a controller
    /// whose own `PATH` carries a different `claude` — which a service-launched
    /// one does, since its environment is not the operator's shell — acts
    /// through the one nothing checked. `Err` is effects off, and it refuses
    /// rather than falling back to `bin`.
    ///
    /// `config_dir` is the child's own configuration directory where the act is
    /// about a session that came up under one, and `None` is the adapter's.
    fn effect_command_under(&self, config_dir: Option<&Path>) -> Result<Command, String> {
        let bin = self.effect_bin.as_ref().ok_or_else(|| {
            "no agent binary is resolved for effects, so this call issues nothing".to_string()
        })?;
        Ok(self.with_environment(&bin.display().to_string(), config_dir))
    }

    /// The binary an effect names in its own causes: the resolved one, or the
    /// seam's spelling when nothing resolved.
    fn effect_bin_name(&self) -> String {
        match &self.effect_bin {
            Some(bin) => bin.display().to_string(),
            None => self.bin.clone(),
        }
    }
}

/// Where a nudge's output goes, under the machine directory. A start's
/// capture is core's (`crate::effect::STARTS_DIR`).
pub const NUDGES_DIR: &str = "nudges";

/// The environment a child keeps, beside the constructed `PATH`. Four values a
/// shell needs to be one, and nothing else: a variable this list does not name
/// cannot reach a session through this controller.
///
/// `CLAUDE_CONFIG_DIR` is not here because it is not passed THROUGH — it is set
/// from the adapter's own resolved directory, which is that variable when the
/// controller was given one and the home default when it was not. Passing it
/// through instead would leave a scoped adapter's child pointing at the default.
///
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` is not here either, and it is set to the
/// CONFIGURED config-directory value rather than the resolved directory: the
/// credential's service name hashes that input, so a directory value — the home
/// default included — names a credential nobody wrote, and only the operator's
/// own input, empty when they configured none, reaches their login.
///
/// `FLEET_BIN` is not here, and passing it through would be wrong twice over: a
/// controller started by a service manager has none to pass, and one started
/// from inside a seat would hand on that seat's binary rather than its own. It
/// is set from this process's own executable instead.
///
/// `FLEET_ACTOR` is not here for the second of those reasons: a controller
/// started from inside a seat would make every session it starts that seat. A
/// start sets it from its own spec, and no other call sets it at all.
pub const PASSED_THROUGH: [&str; 4] = ["HOME", "USER", "TMPDIR", "LANG"];

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

/// The variable the plugin's shim runs its binary from, which every child of
/// this adapter is handed. Spelled here and not taken from the item layer's
/// own constant, because this crate names nothing of the project around it.
pub const FLEET_BIN_VAR: &str = "FLEET_BIN";

/// The variable a started session's verbs read their actor from, set to the
/// start's own `seat:<id>`.
pub const FLEET_ACTOR_VAR: &str = "FLEET_ACTOR";

/// This process's own executable, as the absolute path the shim requires.
///
/// Whatever the operating system answers, and nothing when it answers nothing
/// or something relative: the shim refuses a relative seam, so handing one over
/// would block every Bash command of the session it reached rather than let the
/// shim look under its own root.
fn own_executable() -> Option<PathBuf> {
    std::env::current_exe().ok().filter(|exe| exe.is_absolute())
}

/// Set, `CLAUDE_BIN_VAR` naming nothing is a refusal rather than a fall back.
pub const HERMETIC_VAR: &str = "FLEET_TEST_HERMETIC";

/// The one read of `FLEET_CLAUDE_BIN` in this workspace, and every resolution
/// of the agent binary goes through it.
///
/// Under `FLEET_TEST_HERMETIC` an unnamed binary is a REFUSAL and not the
/// `DEFAULT_BIN` fallback the two resolvers below would take: a suite run
/// inside a flight this fleet is flying inherits a live environment, and an arm
/// that missed its stub would otherwise exec the account's own agent. The stop
/// is taken here rather than returned because both callers' error paths are
/// themselves fallbacks — one turns effects off and carries on — so a returned
/// refusal would be swallowed by the very thing it exists to deny.
pub fn configured_bin() -> Option<String> {
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

/// The agent's configuration directory scopes its daemon and its transcripts
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
    /// The per-project local settings file, which this provider reads out of
    /// the directory the session comes up in.
    fn local_settings(&self) -> &'static str {
        ".claude/settings.local.json"
    }

    /// The resolved binary, the name, the model, the posture and the plugin
    /// root the fleet names, then the first turn as the positional prompt
    /// (lessons claude-code A5, D3, D5) — an INTERACTIVE session, with no
    /// `--bg`: the pane is the session's host now and not the agent's daemon.
    /// The wake rides the start: one act, one channel, so the instruction
    /// cannot be lost without also losing the session. The positional turn
    /// submits on its own (lessons claude-code D8, measured on 2.1.280).
    ///
    /// A start under its own configuration directory is SEEDED first
    /// ([`seed_config`]): that directory begins empty but for the overlay, and
    /// an interactive session under an empty one stops at onboarding and then at
    /// the workspace-trust question before any session exists (D8, A15). A
    /// named seat's start names no directory and is seeded with nothing — its
    /// checkout is a person's, and fleet trusts only worktrees it created
    /// (ruling 13).
    fn launch(&self, spec: &StartSpec) -> Result<Launch, String> {
        let bin = self.effect_bin.as_ref().ok_or_else(|| {
            "no agent binary is resolved for effects, so this call issues nothing".to_string()
        })?;
        let config_dir = spec.config_dir.as_deref().map(Path::new);
        if let Some(dir) = config_dir {
            seed_config(
                dir,
                &operator_file(&self.credential_dir, &self.config_dir),
                Path::new(&spec.worktree),
            )?;
        }
        let mut argv = vec![
            bin.display().to_string(),
            "--name".to_string(),
            spec.name.clone(),
            "--model".to_string(),
            spec.model.clone(),
            "--permission-mode".to_string(),
            spec.posture.clone(),
        ];
        // Only a loaded plugin root gives the session the overlay's hooks and
        // the root's bin on its `PATH` (lessons claude-code D5), and a fleet
        // that names none passes no such element.
        if let Some(plugin_dir) = spec.plugin_dir.as_deref() {
            argv.push("--plugin-dir".to_string());
            argv.push(plugin_dir.to_string());
        }
        argv.push(spec.first_turn.clone());
        let mut env = self.environment(config_dir);
        // WHO THE SESSION ACTS AS, on the start and on nothing else [ASSUMES
        // D7]: its own bare verbs are the seat's. A nudge's print-mode turn
        // writes nothing, so it carries no actor.
        env.push((FLEET_ACTOR_VAR.to_string(), spec.actor.clone()));
        Ok(Launch { argv, env })
    }

    /// Down, then Enter, when the screen is the workspace-trust question: its
    /// default is "No, exit", and the second choice accepts (lessons
    /// claude-code D8, A15).
    fn trust_keys(&self, screen: &str) -> Option<Vec<String>> {
        trust_keys(screen)
    }

    fn stop(&self, config_dir: Option<&Path>, short_id: &str) -> Result<(), String> {
        self.answered(config_dir, &["stop", short_id]).map(|_| ())
    }

    /// `attach <short id>`: the row is reached by its ADDRESS, exactly as a stop
    /// reaches it, and no flag is passed. A resume that carried this fleet's own
    /// flags would fork the session it meant to continue (lessons claude-code
    /// A9), and the table row would then point at a dead twin.
    fn revive(&self, config_dir: Option<&Path>, short_id: &str) -> Result<(), String> {
        self.answered(config_dir, &["attach", short_id]).map(|_| ())
    }

    fn daemon(&self) -> DaemonRead {
        let mut cmd = self.read_command(&self.bin, None);
        cmd.args(["daemon", "status"]);
        let run = match run_bounded(cmd, self.timeout) {
            Ok(run) => run,
            Err(cause) => return DaemonRead::Unreadable { cause },
        };
        // The status is read from the command itself and never through a pipe.
        if !run.status.success() {
            return DaemonRead::Unreadable {
                cause: format!(
                    "`{} daemon status` exited {}: {}",
                    self.bin,
                    run.status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "on a signal".to_string()),
                    String::from_utf8_lossy(&run.stderr).trim()
                ),
            };
        }
        DaemonRead::Readable(parse_daemon_status(&String::from_utf8_lossy(&run.stdout)))
    }

    /// The three answers, read from the exit AND the stdout (lessons claude-code
    /// A8), because two of them share an exit status.
    fn remove(&self, config_dir: Option<&Path>, short_id: &str) -> RemoveAnswer {
        match self.answered(config_dir, &["rm", short_id]) {
            Err(cause) => RemoveAnswer::Refused { cause },
            Ok(stdout) => match worktree_path_in(&stdout) {
                Some(path) => RemoveAnswer::RemovedAWorktree { path },
                None => RemoveAnswer::Removed,
            },
        }
    }

    /// One print-mode turn in the seat's own worktree, on the fleet's cheapest
    /// model, whose whole job is to carry one sentence to one session.
    ///
    /// Its output goes to a file for the same reason a start's does (D2), and it
    /// is bounded: a turn that does not come back must not hold the poll.
    fn nudge(
        &self,
        config_dir: Option<&Path>,
        session_name: &str,
        worktree: &str,
        model: &str,
        prompt: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let log = self.nudge_log_path(session_name);
        let mut cmd = self.effect_command_under(config_dir)?;
        cmd.args(["-p", "--model", model, prompt])
            .current_dir(worktree);
        match self.spawned_to_file(cmd, &log, timeout)? {
            (_, Some(status)) if status.success() => Ok(()),
            (_, Some(status)) => Err(format!(
                "the nudge exited {}; its output is at {}",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "on a signal".to_string()),
                log.display()
            )),
            // A turn that outran its bound is ended WHOLE rather than left
            // running: it holds a session's attention, and the next poll's
            // nudge beside it would be two turns nobody asked for.
            (mut child, None) => {
                crate::platform::kill_process_group(child.id());
                let _ = child.wait();
                Err(format!(
                    "the nudge did not answer within {timeout:?}; its output is at {}",
                    log.display()
                ))
            }
        }
    }

    fn status(&self, config_dir: Option<&Path>) -> RosterRead {
        let mut cmd = self.read_command(&self.bin, config_dir);
        cmd.args(["agents", "--json", "--all"]);
        let run = match run_bounded(cmd, self.timeout) {
            Ok(run) => run,
            Err(cause) => return RosterRead::Unreadable { cause },
        };
        if !run.status.success() {
            return RosterRead::Unreadable {
                cause: format!(
                    "`{} agents --json --all` exited {}: {}",
                    self.bin,
                    run.status
                        .code()
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "on a signal".to_string()),
                    String::from_utf8_lossy(&run.stderr).trim()
                ),
            };
        }
        parse_roster(&String::from_utf8_lossy(&run.stdout))
    }

    fn transcript(
        &self,
        config_dir: Option<&Path>,
        worktree: &str,
        session_id: &str,
    ) -> Option<String> {
        std::fs::read_to_string(transcript_path(
            config_dir.unwrap_or(&self.config_dir),
            worktree,
            session_id,
        ))
        .ok()
    }

    /// The mtime of the same file `transcript` reads, and not a timestamp
    /// parsed out of its last line: a torn or partly written entry still has an
    /// mtime, and a line that does not parse would answer `None` for a session
    /// that plainly ended. The cost is the write cadence — an mtime is the last
    /// WRITE and not the last word — which is far inside any window an operator
    /// would set in hours.
    fn ended_at(&self, config_dir: Option<&Path>, worktree: &str, session_id: &str) -> Option<u64> {
        let written = std::fs::metadata(transcript_path(
            config_dir.unwrap_or(&self.config_dir),
            worktree,
            session_id,
        ))
        .ok()?
        .modified()
        .ok()?;
        Some(
            written
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_millis() as u64,
        )
    }

    fn version(&self) -> Option<String> {
        let mut cmd = self.read_command(&self.bin, None);
        cmd.arg("--version");
        let run = run_bounded(cmd, self.timeout).ok()?;
        if !run.status.success() {
            return None;
        }
        parse_version(&String::from_utf8_lossy(&run.stdout))
    }
}

impl ClaudeCode {
    /// One bounded call whose STDOUT is the answer, or the refusal with why.
    ///
    /// The status is read from the command itself and never through a pipe:
    /// `stop` against the wrong address exits 1 with "No job matching", and a
    /// caller that lost that would believe it had stopped a seat it never
    /// touched (lessons claude-code A6).
    fn answered(&self, config_dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
        let mut cmd = self.effect_command_under(config_dir)?;
        cmd.args(args);
        let run = run_bounded(cmd, self.timeout)?;
        if run.status.success() {
            return Ok(String::from_utf8_lossy(&run.stdout).into_owned());
        }
        // The binary that RAN, which is the effect one and not `bin`: a cause
        // naming a file this call did not exec sends the operator to the wrong
        // one.
        Err(format!(
            "`{} {}` exited {}: {}",
            self.effect_bin_name(),
            args.join(" "),
            run.status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "on a signal".to_string()),
            String::from_utf8_lossy(&run.stderr).trim()
        ))
    }

    /// Where one nudge's words go: named by the session and the moment, so two
    /// nudges for one seat never write over each other and an operator reading
    /// the directory can tell which is which — the naming a start's capture
    /// takes too (`crate::effect::start_capture_path`).
    pub fn nudge_log_path(&self, session_name: &str) -> PathBuf {
        crate::effect::log_path(&self.machine_dir, NUDGES_DIR, session_name)
    }

    /// Spawn a child whose output goes to a file, and wait for it inside
    /// `watch`. `Ok(None)` is a child still running when the window closed.
    ///
    /// The nudge's alone since starts left `--bg` (fleet-rge6.2), and it goes
    /// with the print-mode nudge (fleet-rge6.5). A FILE and never a pipe (D2):
    /// `.output()` waits for EOF rather than for the child, so anything still
    /// holding the write end — the direct child included — keeps this loop
    /// blocked for as long as it lives.
    fn spawned_to_file(
        &self,
        mut cmd: Command,
        log: &Path,
        watch: Duration,
    ) -> Result<(Child, Option<std::process::ExitStatus>), String> {
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                format!("the log directory {} could not be made: {e}", dir.display())
            })?;
        }
        let sink = std::fs::File::create(log)
            .map_err(|e| format!("the log {} could not be opened: {e}", log.display()))?;
        let sink_err = sink
            .try_clone()
            .map_err(|e| format!("the log {} could not be cloned: {e}", log.display()))?;
        // A group of its own, so a call that outruns its window can be ended
        // whole rather than leaving what it forked behind.
        unsafe {
            cmd.pre_exec(crate::platform::own_process_group);
        }
        // The cause names the working directory as well as the binary: a spawn
        // that carries one fails with the same `No such file or directory` for a
        // missing program and for a missing cwd, and a fleet creates and retires
        // worktrees constantly (lessons claude-code A14), so the second is the
        // live case and a cause naming only the binary sends the operator to the
        // wrong file.
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(sink)
            .stderr(sink_err)
            .spawn()
            .map_err(|e| {
                format!(
                    "could not start {} in {}: {e}",
                    self.effect_bin_name(),
                    cmd.get_current_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_else(|| "this process's own directory".to_string())
                )
            })?;
        let status = wait_within(&mut child, watch);
        Ok((child, status))
    }
}

/// `try_wait` until the window closes; `None` is a child that outlived it.
///
/// An unreadable status ends the wait rather than looping to the deadline on a
/// child nobody can ask about any more.
fn wait_within(child: &mut Child, watch: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + watch;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Err(_) => return None,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

/// The worktree path a removal printed, if it printed one.
///
/// A path is what separates the two exit-0 answers, and only one of them is
/// benign (lessons claude-code A8). Read as the first absolute path anywhere in
/// the output, because the sentence around it is the agent's to change and a
/// match on that sentence would read a rewording as "no path printed" — which
/// is the answer that must never be given wrongly.
pub fn worktree_path_in(stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .find(|token| token.starts_with('/') && token.len() > 1)
        .map(|token| token.trim_end_matches(['.', ',']).to_string())
}

/// The listing's answer, or why it is not one.
///
/// Empty output with a success status is `Unreadable`, not an empty fleet
/// (lessons claude-code B4): the force of the rule is that **empty is
/// unreadable**, never a reading of zero. An empty JSON array is a different
/// thing — a listing that answered, and said there is nothing.
pub fn parse_roster(stdout: &str) -> RosterRead {
    if stdout.trim().is_empty() {
        return RosterRead::Unreadable {
            cause: "the listing answered with zero bytes and a success status".to_string(),
        };
    }
    match serde_json::from_str::<Vec<AgentRow>>(stdout) {
        Ok(rows) => RosterRead::Readable(rows),
        Err(e) => RosterRead::Unreadable {
            cause: format!("the listing did not parse as JSON rows: {e}"),
        },
    }
}

/// One `<n><unit>` token of an uptime string, in milliseconds.
fn duration_token_ms(token: &str) -> Option<u64> {
    let at = token.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = token.split_at(at);
    let n: u64 = digits.parse().ok()?;
    let scale = match unit {
        "s" => 1_000,
        "m" => 60 * 1_000,
        "h" => 60 * 60 * 1_000,
        "d" => 24 * 60 * 60 * 1_000,
        _ => return None,
    };
    n.checked_mul(scale)
}

/// Parse the daemon's status. Split from the read so both shapes are provable
/// without a daemon.
///
/// The uptime is SUMMED over `<n>[dhms]` tokens rather than matched against the
/// one spelling a long-lived daemon prints: the humanised spellings a YOUNG
/// daemon might print are exactly the ones the replacement window depends on. A
/// token this does not know makes the whole reading `None`, which closes the
/// window rather than opening it on an invented number.
pub fn parse_daemon_status(body: &str) -> Option<DaemonStatus> {
    let field = |name: &str| -> Option<String> {
        body.lines()
            .find_map(|line| line.trim().strip_prefix(name).map(|v| v.trim().to_string()))
    };
    let pid: u32 = field("pid:")?.parse().ok()?;
    let uptime_ms = field("uptime:").and_then(|v| {
        v.split_whitespace()
            .map(duration_token_ms)
            .try_fold(0u64, |acc, token| Some(acc + token?))
    });
    Some(DaemonStatus { pid, uptime_ms })
}

/// `claude --version` prints the version and then what produced it; the first
/// token is the version.
pub fn parse_version(stdout: &str) -> Option<String> {
    stdout.split_whitespace().next().map(str::to_string)
}

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
        let agent = ClaudeCode::new(home, Path::new("/nowhere-in-particular/.fleet"));
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
            own_executable(),
            "the executable this process is, which every child is handed"
        );
    }

    /// The deadline the controller runs on when nothing sets one. Asserted as
    /// the figure and not as "some default", because the number is the whole
    /// content: widened, a hung listing hangs the poll for as long as it says.
    #[test]
    fn the_deadline_with_no_seam_set_is_twenty_seconds() {
        assert_eq!(timeout_from(None), Duration::from_secs(20));
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(20));
    }

    /// A blank seam is the default and never an empty program, and a setting
    /// that names one is taken trimmed — the same reading its sibling gives a
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

    /// The spawn the guard above exists to prevent, read rather than described:
    /// the reader's docstring quotes this cause as its reason, and a quote no
    /// arm takes is a claim about the OS that nobody has checked.
    ///
    /// The field is set directly, because the guard means no setting of the seam
    /// can reach here — which is the point, and is what the closing pair of
    /// `bin_from` readings says from the other side.
    #[test]
    fn an_empty_program_spawns_as_a_cause_that_names_no_binary() {
        let empty = ClaudeCode {
            bin: String::new(),
            config_dir: PathBuf::from("/nowhere-in-particular/.claude"),
            timeout: Duration::from_secs(5),
            machine_dir: PathBuf::from("/nowhere-in-particular/.fleet"),
            child_path: String::new(),
            credential_dir: String::new(),
            effect_bin: None,
            fleet_bin: None,
        };
        let cause = match empty.status(None) {
            RosterRead::Unreadable { cause } => cause,
            RosterRead::Readable(rows) => panic!("an empty program read {} rows", rows.len()),
        };
        assert!(
            cause.contains("could not start :"),
            "the cause names the empty program as the binary, which is no name at all: {cause}"
        );
        assert!(
            cause.contains("No such file or directory"),
            "and what the OS said about it: {cause}"
        );

        // The control: a program that EXISTS, spawned by this same call. It
        // fails at the CALL and never at the spawn, so the first assertion above
        // reads the empty program rather than a `status()` that answers "could
        // not start" for everything.
        //
        // THE PROGRAM IS ONE WHOSE ARGV CAN NEITHER NAME A FILE NOR ACT ON A
        // PROCESS. `status()` fixes the arguments at `agents --json --all`, so a
        // shell or a reader here would resolve `agents` against whatever
        // directory the test process happens to be in, and a file of that name
        // sitting there would be RUN. Opening nothing is not enough on its own:
        // a `kill` whose `--all` selects by COMMAND NAME, which is what
        // util-linux's does, would send a signal to any process called `agents`
        // on the box. `false` ignores every argument it is given and exits 1, so
        // this control's result is the same in every directory and on every
        // system, and it reaches nothing outside itself either way.
        //
        // It controls the FIRST assertion alone, and it pins that a status came
        // back rather than which one — though `false` is the one program whose
        // status is its whole contract, so the 1 below is not a figure this
        // suite is guessing at.
        let present = ClaudeCode {
            bin: "/usr/bin/false".to_string(),
            config_dir: PathBuf::from("/nowhere-in-particular/.claude"),
            timeout: Duration::from_secs(5),
            machine_dir: PathBuf::from("/nowhere-in-particular/.fleet"),
            child_path: String::new(),
            credential_dir: String::new(),
            effect_bin: None,
            fleet_bin: None,
        };
        let control = match present.status(None) {
            RosterRead::Unreadable { cause } => cause,
            // The binary from the value that ran, never a second copy of the
            // name: a control whose program is renamed leaves this message
            // naming the old one, which is what a reader meets on the red.
            RosterRead::Readable(rows) => {
                panic!("{} read {} rows", present.bin, rows.len())
            }
        };
        assert!(
            !control.contains("could not start"),
            "a program that is there names no failed start: {control}"
        );
        assert!(
            control.contains("exited "),
            "and the failure is the program's own, at the call, carrying the \
             status it chose: {control}"
        );

        // The control for the second assertion, which the one above cannot
        // reach: a program that starts, answers, and fails at the PARSE, so its
        // cause carries neither needle. Without it, "No such file or directory"
        // is a string `status()` might put on anything.
        let answering = ClaudeCode {
            bin: "/bin/echo".to_string(),
            config_dir: PathBuf::from("/nowhere-in-particular/.claude"),
            timeout: Duration::from_secs(5),
            machine_dir: PathBuf::from("/nowhere-in-particular/.fleet"),
            child_path: String::new(),
            credential_dir: String::new(),
            effect_bin: None,
            fleet_bin: None,
        };
        let answered = match answering.status(None) {
            RosterRead::Unreadable { cause } => cause,
            RosterRead::Readable(rows) => panic!("/bin/echo read {} rows", rows.len()),
        };
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

    /// The third seam reader, on the same two readings its siblings are pinned
    /// on: a blank setting is the default under the home it was handed, and a
    /// named one is taken trimmed. The home is a parameter, so the default is
    /// asserted against the argument and never against this box's own.
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
    /// THE TWO DIVERGE ON THE UNCONFIGURED CASE AND THAT DIVERGENCE IS THE
    /// WHOLE POINT. A reader that answered the home default here — which is the
    /// drift a later hand makes, and what a copy of `config_dir_from` would do
    /// — hands the child a directory, and a directory is a credential nobody
    /// wrote. So the blank readings are asserted against the empty string AND
    /// against the sibling's answer for the same input, which no directory
    /// value can satisfy at once.
    #[test]
    fn a_blank_credential_seam_is_empty_and_a_named_one_is_trimmed() {
        let home = Path::new("/nowhere-in-particular");
        for configured in [None, Some(""), Some("   "), Some("\t\n")] {
            assert_eq!(
                credential_dir_from(configured),
                "",
                "{configured:?} names no credential scope, and empty is the value that \
                 restores the operator's own"
            );
            assert_ne!(
                PathBuf::from(credential_dir_from(configured)),
                config_dir_from(configured, home),
                "{configured:?} reads as the resolved directory, which is a different \
                 credential"
            );
        }
        assert_eq!(credential_dir_from(Some("/opt/cfg")), "/opt/cfg");
        assert_eq!(credential_dir_from(Some("  /opt/cfg  ")), "/opt/cfg");
    }

    /// The seam, in milliseconds: the value this reader returns for a setting,
    /// and nothing about elapsed time. The 1 ms case pins that the guard above
    /// rejects a non-deadline without flooring the value, so nothing between 1
    /// and the default is rounded away here; what a 1 ms deadline costs a real
    /// call is `run_bounded`'s 20 ms poll, which this arm never enters.
    ///
    /// THE CLAIM IS ABOUT A RANGE AND THE CASES ARE ITS ENDS AND ITS MIDDLE. A
    /// reader that floors, caps or rounds is a different function at values no
    /// case names, and only a value inside each stretch tells them apart: 1 and
    /// 150 sit inside the two the claim covers, and the case ABOVE the default
    /// is what says the promise reaches past it — a cap at `DEFAULT_TIMEOUT`
    /// satisfies every other line here.
    #[test]
    fn the_seam_sets_the_deadline_in_milliseconds() {
        assert_eq!(timeout_from(Some("1")), Duration::from_millis(1));
        // Inside the range the sentence above claims, and equal to no value any
        // other line asserts: a reader rounding this stretch to its top passes
        // 1, 300 and 5000 unchanged.
        assert_eq!(timeout_from(Some("150")), Duration::from_millis(150));
        assert_eq!(timeout_from(Some("300")), Duration::from_millis(300));
        assert_eq!(timeout_from(Some(" 300 ")), Duration::from_millis(300));
        assert_eq!(timeout_from(Some("5000")), Duration::from_secs(5));

        // Above the default, built FROM the default so the case cannot become a
        // value under it the day that constant moves. The expectation is the
        // same figure the argument was rendered from, so there is no second
        // copy to disagree.
        let above_default = DEFAULT_TIMEOUT + Duration::from_secs(40);
        assert_eq!(
            timeout_from(Some(&above_default.as_millis().to_string())),
            above_default,
            "a seam above the default is honoured, not capped at it"
        );
    }

    /// The operator's state file is where the agent itself keeps it: inside a
    /// configured configuration directory, and in the home — beside the
    /// default directory — where none is configured. The two cases differ in
    /// the one input `credential_dir_from` reads, so each is read through it.
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

    /// The trust question's keys come back only for a screen showing its
    /// accepting choice, wrapped or not — and never for a session at its
    /// prompt, which a start must not type into.
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
    /// default. A zero is named here beside the typos: it parses, and honouring
    /// it would kill every listing before it could answer.
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
}
