//! [`TmuxHost`]: the host seam answered by tmux, on fleet's own server.
//!
//! EVERY CALL IS `tmux -L <socket> -f /dev/null …`. The socket is
//! [`super::SOCKET`] in production, so fleet's server is never the person's
//! default one; `-f /dev/null` keeps the person's `~/.tmux.conf` from shaping a
//! server whose sessions fleet reads back — a config that renumbers windows,
//! rebinds the prefix or turns remain-on-exit off would change every answer
//! below.
//!
//! Each call is one short-lived client, run with the constructed environment
//! (the constructed `PATH`, lessons claude-code D1) and bounded by
//! [`platform::run_bounded`] at [`CALL_TIMEOUT`]. The server the first call
//! starts daemonizes out of the client's process group and closes the pipes it
//! was handed, so the bound reaches the client and never the server.
//!
//! TWO QUIRKS OF THE ARGUMENT PARSER are answered here and nowhere else, both
//! measured on 3.7b. tmux reads ANY argument ending in `;` as a command
//! separator, so every argument this module did not write itself goes through
//! [`literal`]. And a session's start directory is expanded as a format, so a
//! `#` in it is doubled first ([`format_literal`]) — unescaped, a directory
//! naming one resolved to nothing and tmux started the pane in the home
//! directory without a word.

use super::{Host, HostRead, Pane, PaneState, SUBMIT_GAP};
use crate::platform;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

/// The variable that names the tmux binary, in the shape `FLEET_CLAUDE_BIN`
/// has: an ABSOLUTE path, never a name this module would search for.
pub const TMUX_BIN_VAR: &str = "FLEET_TMUX_BIN";

/// The binary when nothing names one, resolved on the constructed `PATH`.
pub const DEFAULT_BIN: &str = "tmux";

/// The deadline every call runs on. Every call is a local round trip to a
/// server on this machine, so twenty seconds is far past any healthy answer.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(20);

/// The terminal a session's pane advertises to the agent. The environment is
/// set exactly (E2), so the agent sees this and no `TERM` of the controller's.
pub const PANE_TERM: &str = "tmux-256color";

/// What `list` asks for, one pane per line. The path is LAST and the row is
/// split into at most six fields, so a tab inside a directory name stays in the
/// path rather than shifting every column after it.
const LIST_FORMAT: &str = "#{session_name}\t#{pane_pid}\t#{pane_dead}\t#{pane_dead_status}\t\
                           #{session_created}\t#{pane_current_path}";

/// What each client carries from the controller's own environment beside the
/// constructed `PATH`. The locale decides whether the client counts as UTF-8;
/// nothing else of the controller's is the server's business. `TMUX` and
/// `TMUX_TMPDIR` are never carried: the first names the person's own server
/// and the second would move fleet's socket somewhere [`TmuxHost::attach`]
/// does not look.
const PASSED_THROUGH: [&str; 5] = ["HOME", "USER", "LANG", "LC_ALL", "LC_CTYPE"];

/// fleet's server, reached through one binary on one socket.
#[derive(Clone, Debug)]
pub struct TmuxHost {
    bin: PathBuf,
    socket: String,
    child_path: String,
}

impl TmuxHost {
    /// The host on [`super::SOCKET`], its binary resolved once.
    ///
    /// `FLEET_TMUX_BIN` when it names an absolute path to an executable file;
    /// a relative one is refused rather than resolved, for the reason
    /// `resolve_effect_bin` refuses one. Otherwise the first `tmux` on the
    /// constructed `PATH`. Under `FLEET_TEST_HERMETIC` an unnamed binary is
    /// refused, as `configured_bin` refuses one: a suite run inside a flight
    /// inherits a live environment, and an arm that missed its stub would
    /// otherwise start sessions on the operator's own tmux. The refusal is
    /// RETURNED where `configured_bin` exits the process: that one's callers
    /// fall back to a binary on failure, and a caller of this one is left with
    /// no host at all, so no tmux runs either way.
    pub fn resolve(child_path: &str) -> Result<TmuxHost, String> {
        TmuxHost::resolve_from(
            std::env::var(TMUX_BIN_VAR).ok().as_deref(),
            crate::adapter::claude_code::hermetic(),
            child_path,
        )
    }

    /// [`TmuxHost::resolve`] with the environment passed in, so the order is
    /// tested without touching it.
    pub fn resolve_from(
        configured: Option<&str>,
        hermetic: bool,
        child_path: &str,
    ) -> Result<TmuxHost, String> {
        let bin = match configured.map(str::trim).filter(|v| !v.is_empty()) {
            Some(named) => {
                if !Path::new(named).is_absolute() {
                    return Err(format!(
                        "{TMUX_BIN_VAR} names `{named}`, which is not an absolute path"
                    ));
                }
                platform::resolve_on_path(child_path, named).ok_or_else(|| {
                    format!("{TMUX_BIN_VAR} names `{named}`, which is not an executable file")
                })?
            }
            None if hermetic => {
                return Err(format!(
                    "{} is set and {TMUX_BIN_VAR} names no tmux binary — refusing to fall back \
                     to a `{DEFAULT_BIN}` on PATH",
                    crate::adapter::claude_code::HERMETIC_VAR
                ))
            }
            None => platform::resolve_on_path(child_path, DEFAULT_BIN).ok_or_else(|| {
                format!("no `{DEFAULT_BIN}` on the constructed child PATH ({child_path})")
            })?,
        };
        Ok(TmuxHost {
            bin,
            socket: super::SOCKET.to_string(),
            child_path: child_path.to_string(),
        })
    }

    /// The same host on another socket: a suite's scratch server, which is
    /// never [`super::SOCKET`].
    pub fn on_socket(mut self, socket: &str) -> TmuxHost {
        self.socket = socket.to_string();
        self
    }

    /// The binary every call runs.
    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// The socket every call names.
    pub fn socket(&self) -> &str {
        &self.socket
    }

    /// A client command on this host's server, with the constructed
    /// environment and nothing else of the controller's.
    fn client(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.env_clear().env("PATH", &self.child_path);
        for pass in PASSED_THROUGH {
            if let Ok(value) = std::env::var(pass) {
                cmd.env(pass, value);
            }
        }
        cmd.args(["-L", &self.socket, "-f", "/dev/null"]).args(args);
        cmd
    }

    /// Run a client call and answer its stdout, or the cause it failed with.
    fn run(&self, args: &[&str]) -> Result<Output, String> {
        platform::run_bounded(self.client(args), CALL_TIMEOUT)
    }

    fn ran(&self, args: &[&str]) -> Result<String, String> {
        answered(self.run(args)?)
    }

    /// The first half of [`Host::send`] alone: `text` pasted into the session
    /// and NOT submitted. Public so a suite can read the pane between the paste
    /// and the submit, which `send` itself leaves no window to do.
    ///
    /// The text goes in on the client's stdin (`load-buffer -`), so no byte of
    /// it is an argument tmux could read as syntax, and the load and the paste
    /// are ONE client, so the buffer never outlives the call that named it.
    /// `-p` asks for a bracketed paste, which tmux sends only where the program
    /// asked for one; `-d` deletes the buffer. A pane that has died refuses
    /// the paste (`target pane has exited`, measured on 3.7b).
    pub fn paste(&self, name: &str, text: &str) -> Result<(), String> {
        checked(name)?;
        let buffer = buffer_for(name);
        let target = pane_target(name);
        let cmd = self.client(&[
            "load-buffer",
            "-b",
            &buffer,
            "-",
            ";",
            "paste-buffer",
            "-p",
            "-d",
            "-b",
            &buffer,
            "-t",
            &target,
        ]);
        answered(platform::run_bounded_fed(
            cmd,
            text.as_bytes().to_vec(),
            CALL_TIMEOUT,
        )?)
        .map(|_| ())
    }
}

/// A call's stdout, or its stderr's last line as the cause.
fn answered(out: Output) -> Result<String, String> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(cause_of(&out))
    }
}

/// Why a call failed, in tmux's own words where it printed any: the last line
/// of stderr, else the exit.
fn cause_of(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    match stderr.lines().rev().map(str::trim).find(|l| !l.is_empty()) {
        Some(line) => line.to_string(),
        None => format!("tmux exited with {}", out.status),
    }
}

/// Whether stderr says the socket has no server behind it. Two spellings,
/// both measured on 3.7b: `no server running on <socket>` where the socket
/// file outlived its server (3.7b leaves the file behind when its last session
/// ends), and `error connecting to <socket> (No such file or directory)` where
/// there is no file at all, which is the fleet after a reboot. A connect the
/// OS refuses (`Connection refused`) is taken as the same, unmeasured. Any
/// other connect failure — a socket this user may not open — is NOT a server
/// that is gone, and is left unreadable.
fn no_server(stderr: &str) -> bool {
    stderr.contains("no server running on")
        || (stderr.contains("error connecting to")
            && (stderr.contains("No such file or directory")
                || stderr.contains("Connection refused")))
}

/// Whether stderr says the named session or pane is not on the server.
fn not_found(stderr: &str) -> bool {
    stderr.contains("can't find session") || stderr.contains("can't find pane")
}

/// An argument tmux will read back as itself. tmux takes any argument that
/// ENDS in `;` for a command separator, and one ending in `\;` for the text
/// with its backslash dropped — so a backslash before the last `;` restores
/// exactly the argument given, whatever came before it (measured on 3.7b: `a\;`
/// passed as `a\\;` arrives as `a\;`, and `;` passed as `\;` arrives as `;`).
pub fn literal(arg: &str) -> String {
    match arg.strip_suffix(';') {
        Some(head) => format!("{head}\\;"),
        None => arg.to_string(),
    }
}

/// An argument tmux expands as a format, read back as itself: every `#`
/// doubled, then [`literal`].
pub fn format_literal(arg: &str) -> String {
    literal(&arg.replace('#', "##"))
}

/// The target naming a session exactly: `=` refuses a prefix match, so seat
/// `ab` never answers for seat `abc`.
fn session_target(name: &str) -> String {
    format!("={name}")
}

/// The target naming a session's active pane exactly. A bare `=name` is not a
/// pane target — tmux answers `can't find pane: =name` (measured on 3.7b) —
/// so the session is closed with its `:`.
fn pane_target(name: &str) -> String {
    format!("={name}:")
}

/// A session name tmux keeps as given. tmux rewrites `:` and `.` in a name and
/// reads others as target syntax, so anything outside `[A-Za-z0-9_-]` is
/// refused rather than started under a name nobody finds again. A seat's name
/// ([`super::session_for`]) is always inside it.
fn checked(name: &str) -> Result<(), String> {
    if !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(format!(
            "`{name}` is not a session name — a session name is [A-Za-z0-9_-] and not empty"
        ))
    }
}

/// The paste buffer a send loads into, named per session so two sends to two
/// sessions never share one.
fn buffer_for(name: &str) -> String {
    format!("fleet-{name}")
}

/// The whole `new-session` call, as the client's argument list after the
/// socket and config flags: remain-on-exit set globally FIRST and in the SAME
/// client, so a command that exits at once still leaves a dead pane holding
/// its status, then the session, whose command is `/usr/bin/env -i` with the
/// environment and the argv — no shell between tmux and the agent, and nothing
/// of the server's own environment reaching the pane. The pane's `TERM` is
/// [`PANE_TERM`], and a `TERM` in `env` is dropped for it.
pub fn new_session_args(
    name: &str,
    cwd: &Path,
    argv: &[String],
    env: &[(String, String)],
) -> Vec<String> {
    let mut args: Vec<String> = [
        "set-option",
        "-g",
        "remain-on-exit",
        "on",
        ";",
        "new-session",
        "-d",
        "-s",
    ]
    .iter()
    .map(|a| a.to_string())
    .collect();
    args.push(name.to_string());
    args.push("-c".to_string());
    args.push(format_literal(&cwd.to_string_lossy()));
    args.push("--".to_string());
    args.push("/usr/bin/env".to_string());
    args.push("-i".to_string());
    for (key, value) in env.iter().filter(|(key, _)| key != "TERM") {
        args.push(literal(&format!("{key}={value}")));
    }
    args.push(format!("TERM={PANE_TERM}"));
    args.extend(argv.iter().map(|a| literal(a)));
    args
}

/// A listing's answer as a read: rows when it answered, no panes when no
/// server runs, and the cause otherwise.
pub fn read_listing(ok: bool, stdout: &str, stderr: &str) -> HostRead {
    if !ok {
        if no_server(stderr) {
            return HostRead::Readable(Vec::new());
        }
        let cause = stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("the listing failed and printed nothing");
        return HostRead::Unreadable {
            cause: cause.to_string(),
        };
    }
    let mut panes = Vec::new();
    for line in stdout.lines().filter(|l| !l.is_empty()) {
        match parse_row(line) {
            Some(pane) => panes.push(pane),
            // A row that will not read makes the whole listing unreadable: a
            // listing read around it is a pane reading absent.
            None => {
                return HostRead::Unreadable {
                    cause: format!("a listing row did not read: {line:?}"),
                }
            }
        }
    }
    HostRead::Readable(panes)
}

/// One row of [`LIST_FORMAT`].
fn parse_row(line: &str) -> Option<Pane> {
    let fields: Vec<&str> = line.splitn(6, '\t').collect();
    let [session, pid, dead, status, created, path] = fields[..] else {
        return None;
    };
    if session.is_empty() {
        return None;
    }
    let state = match dead {
        "0" => PaneState::Alive,
        "1" => PaneState::Dead {
            status: status.parse().ok(),
        },
        _ => return None,
    };
    Some(Pane {
        session: session.to_string(),
        pid: pid.parse().ok(),
        state,
        path: path.to_string(),
        created_ms: created.parse::<u64>().ok().map(|secs| secs * 1000),
    })
}

impl Host for TmuxHost {
    fn new_session(
        &self,
        name: &str,
        cwd: &Path,
        argv: &[String],
        env: &[(String, String)],
    ) -> Result<(), String> {
        checked(name)?;
        // `env` reads every leading `K=V` as an assignment, so a key that is
        // empty or holds `=`, or a program whose path holds one, would hand
        // the pane a different environment or a different program.
        match argv.first() {
            None => return Err(format!("session {name}: no command to run")),
            Some(program) if program.contains('=') => {
                return Err(format!(
                    "session {name}: the program `{program}` holds `=`, which env reads as an \
                     assignment"
                ))
            }
            Some(_) => {}
        }
        if let Some((key, _)) = env
            .iter()
            .find(|(key, _)| key.is_empty() || key.contains('='))
        {
            return Err(format!(
                "session {name}: `{key}` is not an environment variable name"
            ));
        }
        // tmux starts a pane whose directory does not exist in the home
        // directory instead, and says nothing (measured on 3.7b): an agent in
        // the wrong tree is worse than no agent.
        if !cwd.is_dir() {
            return Err(format!(
                "session {name}: {} is not a directory",
                cwd.display()
            ));
        }
        let args = new_session_args(name, cwd, argv, env);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.ran(&args).map(|_| ())
    }

    fn send(&self, name: &str, text: &str) -> Result<(), String> {
        self.paste(name, text)?;
        std::thread::sleep(SUBMIT_GAP);
        self.ran(&["send-keys", "-t", &pane_target(name), "C-m"])
            .map(|_| ())
    }

    fn keys(&self, name: &str, keys: &[&str]) -> Result<(), String> {
        checked(name)?;
        let target = pane_target(name);
        let keys: Vec<String> = keys.iter().map(|k| literal(k)).collect();
        let mut args = vec!["send-keys", "-t", &target];
        args.extend(keys.iter().map(String::as_str));
        self.ran(&args).map(|_| ())
    }

    fn capture(&self, name: &str) -> Result<String, String> {
        checked(name)?;
        // `-S -`: the history as well as the screen. When a pane dies, tmux
        // writes its `Pane is dead (status …)` line at the foot of the screen
        // and what the process printed goes above it into the history
        // (measured on 3.7b), so the visible screen alone of a pane that has
        // just died holds none of its last words.
        self.ran(&["capture-pane", "-p", "-S", "-", "-t", &pane_target(name)])
    }

    fn kill(&self, name: &str) -> Result<(), String> {
        checked(name)?;
        let out = self.run(&["kill-session", "-t", &session_target(name)])?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        if out.status.success() || not_found(&stderr) || no_server(&stderr) {
            Ok(())
        } else {
            Err(cause_of(&out))
        }
    }

    fn list(&self) -> HostRead {
        match self.run(&["list-panes", "-a", "-F", LIST_FORMAT]) {
            Ok(out) => read_listing(
                out.status.success(),
                &String::from_utf8_lossy(&out.stdout),
                &String::from_utf8_lossy(&out.stderr),
            ),
            Err(cause) => HostRead::Unreadable { cause },
        }
    }

    fn attach(&self, name: &str, write: bool) -> Command {
        // The person's own environment, because the terminal is theirs and so
        // is its TERM — less the two variables that would point this client at
        // a server other than fleet's: TMUX_TMPDIR moves the socket directory,
        // and TMUX is the person's own session, inside which tmux refuses to
        // attach at all.
        let mut cmd = Command::new(&self.bin);
        cmd.env_remove("TMUX").env_remove("TMUX_TMPDIR");
        cmd.args(["-L", &self.socket, "-f", "/dev/null", "attach-session"]);
        if !write {
            cmd.arg("-r");
        }
        cmd.args(["-t", &session_target(name)]);
        cmd
    }

    fn version(&self) -> Option<String> {
        let out = self.ran(&["-V"]).ok()?;
        let version = out.trim().strip_prefix("tmux ")?.trim();
        (!version.is_empty()).then(|| version.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE: &str = "abc\t4242\t0\t\t1790406337\t/private/tmp/work";

    /// A live pane: its pid, Alive, its path, and the session's creation in
    /// milliseconds from tmux's seconds.
    #[test]
    fn a_live_row_reads_alive_with_its_pid_and_path() {
        let read = read_listing(true, &format!("{LIVE}\n"), "");
        assert_eq!(
            read,
            HostRead::Readable(vec![Pane {
                session: "abc".into(),
                pid: Some(4242),
                state: PaneState::Alive,
                path: "/private/tmp/work".into(),
                created_ms: Some(1_790_406_337_000),
            }])
        );
    }

    /// A dead pane keeps its status; its path reads empty, as 3.7b prints it.
    #[test]
    fn a_dead_row_reads_dead_with_its_status() {
        let read = read_listing(true, "f1\t4243\t1\t3\t1790406337\t\n", "");
        let HostRead::Readable(panes) = read else {
            panic!("a dead pane is a readable row: {read:?}");
        };
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].state, PaneState::Dead { status: Some(3) });
        assert_eq!(panes[0].pid, Some(4243), "the pid it had");
        assert_eq!(panes[0].path, "");
    }

    /// A dead pane with no status — a process a signal ended — is Dead with
    /// none, never Alive and never an unreadable row.
    #[test]
    fn a_dead_row_with_no_status_reads_dead_with_none() {
        let read = read_listing(true, "f2\t4244\t1\t\t1790406337\t\n", "");
        let HostRead::Readable(panes) = read else {
            panic!("{read:?}");
        };
        assert_eq!(panes[0].state, PaneState::Dead { status: None });
    }

    /// No server behind the socket is a server holding nothing, in both of the
    /// spellings 3.7b prints; a socket this user may not open is not.
    #[test]
    fn no_server_reads_as_no_panes_and_a_refused_socket_does_not() {
        for stderr in [
            "no server running on /private/tmp/tmux-501/fleet\n",
            "error connecting to /private/tmp/tmux-501/fleet (No such file or directory)\n",
            "error connecting to /private/tmp/tmux-501/fleet (Connection refused)\n",
        ] {
            assert_eq!(
                read_listing(false, "", stderr),
                HostRead::Readable(Vec::new()),
                "{stderr}"
            );
        }
        assert_eq!(
            read_listing(
                false,
                "",
                "error connecting to /private/tmp/tmux-501/fleet (Permission denied)\n"
            ),
            HostRead::Unreadable {
                cause: "error connecting to /private/tmp/tmux-501/fleet (Permission denied)".into()
            }
        );
    }

    /// Any other failure is unreadable with stderr's last line, and a row that
    /// will not read unreads the whole listing rather than dropping a pane.
    #[test]
    fn a_failure_or_a_bad_row_is_unreadable() {
        assert_eq!(
            read_listing(false, "", "warming up\nserver exploded\n"),
            HostRead::Unreadable {
                cause: "server exploded".into()
            }
        );
        let bad = read_listing(true, &format!("{LIVE}\nnot a row\n"), "");
        assert!(
            matches!(bad, HostRead::Unreadable { ref cause } if cause.contains("not a row")),
            "{bad:?}"
        );
    }

    /// A tab inside the path stays in the path: it is the last field and the
    /// split stops at six.
    #[test]
    fn a_tab_in_the_path_stays_in_the_path() {
        let read = read_listing(true, "abc\t1\t0\t\t1\t/tmp/a\tb\n", "");
        let HostRead::Readable(panes) = read else {
            panic!("{read:?}");
        };
        assert_eq!(panes[0].path, "/tmp/a\tb");
    }

    /// The two escapes, as 3.7b reads them back: a trailing `;` gains a
    /// backslash, and a format argument doubles every `#` first.
    #[test]
    fn arguments_are_escaped_so_tmux_reads_them_back_as_given() {
        assert_eq!(literal("plain"), "plain");
        assert_eq!(literal("a;"), "a\\;");
        assert_eq!(literal(";"), "\\;");
        assert_eq!(literal("a\\;"), "a\\\\;");
        assert_eq!(literal("a;b"), "a;b", "only a TRAILING `;` separates");
        assert_eq!(format_literal("/w/a#b#{x}"), "/w/a##b##{x}");
    }

    /// The start is one client: remain-on-exit before the session, the
    /// command under `env -i` with the environment and TERM, then the argv.
    #[test]
    fn a_start_is_one_client_setting_remain_on_exit_first() {
        let args = new_session_args(
            "s1",
            Path::new("/w/tree"),
            &["/bin/sh".into(), "-c".into(), "exit 3;".into()],
            &[("HOME".into(), "/h".into()), ("TERM".into(), "dumb".into())],
        );
        assert_eq!(
            args,
            [
                "set-option",
                "-g",
                "remain-on-exit",
                "on",
                ";",
                "new-session",
                "-d",
                "-s",
                "s1",
                "-c",
                "/w/tree",
                "--",
                "/usr/bin/env",
                "-i",
                "HOME=/h",
                "TERM=tmux-256color",
                "/bin/sh",
                "-c",
                "exit 3\\;",
            ]
        );
    }

    /// The resolution order: a named absolute binary, a relative one refused,
    /// the hermetic refusal, and the constructed PATH.
    #[test]
    fn the_binary_resolves_from_the_seam_or_the_constructed_path() {
        let named = TmuxHost::resolve_from(Some("/bin/sh"), true, "")
            .expect("an absolute executable is taken, hermetic or not");
        assert_eq!(named.bin(), Path::new("/bin/sh"));
        assert_eq!(named.socket(), crate::host::SOCKET);

        let relative = TmuxHost::resolve_from(Some("bin/tmux"), false, "/bin")
            .expect_err("a relative seam is refused");
        assert!(relative.contains("not an absolute path"), "{relative}");

        let missing = TmuxHost::resolve_from(Some("/nowhere/tmux"), false, "/bin")
            .expect_err("a seam naming no file is refused");
        assert!(missing.contains("not an executable file"), "{missing}");

        let hermetic = TmuxHost::resolve_from(Some("  "), true, "/bin:/usr/bin")
            .expect_err("hermetic with no binary named is a refusal");
        assert!(hermetic.contains("FLEET_TEST_HERMETIC"), "{hermetic}");

        let absent = TmuxHost::resolve_from(None, false, "/nowhere")
            .expect_err("nothing on the path is a refusal");
        assert!(absent.contains("no `tmux`"), "{absent}");
    }

    /// Names outside `[A-Za-z0-9_-]` are refused before any call.
    #[test]
    fn a_name_tmux_would_rewrite_is_refused() {
        let host = TmuxHost::resolve_from(Some("/bin/sh"), false, "")
            .expect("a host")
            .on_socket("fleet-test-never-called");
        for bad in ["", "a:b", "a.b", "=a", "a b"] {
            assert!(host.capture(bad).is_err(), "{bad:?}");
        }
        assert!(checked("0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b").is_ok());
    }

    /// The attach is read-only unless asked for the keyboard, names the
    /// session exactly, and is never pointed at the person's own server.
    #[test]
    fn the_attach_is_read_only_unless_write_and_names_the_session_exactly() {
        let host = TmuxHost::resolve_from(Some("/bin/sh"), false, "")
            .expect("a host")
            .on_socket("fleet-test-x");
        let args = |cmd: &Command| -> Vec<String> {
            cmd.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let read = host.attach("s1", false);
        assert_eq!(
            args(&read),
            [
                "-L",
                "fleet-test-x",
                "-f",
                "/dev/null",
                "attach-session",
                "-r",
                "-t",
                "=s1"
            ]
        );
        let write = host.attach("s1", true);
        assert!(!args(&write).contains(&"-r".to_string()));
        let removed: Vec<_> = read
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(
            removed.contains(&"TMUX".into()) && removed.contains(&"TMUX_TMPDIR".into()),
            "{removed:?}"
        );
    }
}
