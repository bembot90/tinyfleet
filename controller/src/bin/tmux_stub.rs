//! `fleet-tmux-stub`: tmux's client, answered from a fake server kept in a
//! file, for the suites that drive the built `fleet` and so cannot hand it the
//! in-process `FakeHost` (reviewer call 2026-09-25, E7). It mirrors
//! `fleet-store-stub`: built only under `test-support`, so no release build
//! carries it, and pointed at through `FLEET_TMUX_BIN`.
//!
//! The server is `fleet_controller::test_support::FakeServer`, the model the
//! in-process fake answers from, read from its JSON state file (see
//! `state_path`) and written back after every call — so the two seams answer
//! one set of rules. It answers the client calls `TmuxHost` makes and no others:
//! `-V` (as `tmux 3.7b`), `set-option`, `new-session`, `load-buffer`,
//! `paste-buffer`, `send-keys`, `capture-pane`, `kill-session`, `list-panes`,
//! `attach-session` and `kill-server`, chained with `;` as the real client
//! chains them — and `list-sessions`, which the defaults' `tmux-version`
//! doctor check asks. Every argument list it is run with is recorded on the state,
//! which is how a suite reads an attach's — and an attach's whole environment
//! beside it, the one call run under the person's own.
//!
//! One verb is the stub's own, for a suite to drive what no client call does:
//! `fleet-tmux-stub end <name> <status>` ends that session's pane with the
//! status and keeps it, as remain-on-exit keeps a real one.
//!
//! The cli crate builds this same file as its example `fleet-tmux-stub`,
//! through `#[path]`, because a test build of one crate builds no other crate's
//! executables.

use fleet_controller::test_support::FakeServer;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

/// The variable naming the state file.
pub const STATE_VAR: &str = "FLEET_TMUX_STUB_STATE";

/// What `-V` answers: the release fleet measured.
pub const VERSION_LINE: &str = "tmux 3.7b";

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{why}");
            ExitCode::from(1)
        }
    }
}

/// The state file's name where it sits beside the link the stub was run by.
pub const STATE_BESIDE: &str = "tmux-stub.json";

/// `FLEET_TMUX_STUB_STATE` where it is set, else [`STATE_BESIDE`] in the
/// directory of the SYMLINK the stub was run through.
///
/// The link is how a rig names its state without the variable: `TmuxHost`
/// clears its clients' environment, so a variable put on `fleet` never reaches
/// the stub, and a wrapper SCRIPT carrying it costs its first exec 15–18 s on
/// macOS, where a newly written executable is assessed before it runs
/// (measured 2026-09-26); a link to this already-run binary is not. A run
/// by the binary's own path, with no variable, names no state and refuses.
fn state_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var(STATE_VAR) {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let invoked = PathBuf::from(std::env::args_os().next().unwrap_or_default());
    let is_link = std::fs::symlink_metadata(&invoked)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false);
    match invoked.parent() {
        Some(dir) if is_link => Ok(dir.join(STATE_BESIDE)),
        _ => Err(format!(
            "fleet-tmux-stub: {STATE_VAR} names no state file and this run came through no link"
        )),
    }
}

/// One run: the global flags, then the stub's own `end` or a chain of client
/// commands, against the state file loaded once and saved once.
fn run(args: &[String]) -> Result<String, String> {
    let mut rest = args;
    let mut socket = String::from("default");
    loop {
        match rest {
            [flag, value, tail @ ..] if flag == "-L" => {
                socket = value.clone();
                rest = tail;
            }
            [flag, _, tail @ ..] if flag == "-f" => rest = tail,
            [flag, ..] if flag == "-V" => return Ok(format!("{VERSION_LINE}\n")),
            _ => break,
        }
    }
    let path = state_path()?;
    let mut server = FakeServer::load(&path)?;
    server.invocations.push(args.to_vec());
    let answer = match rest {
        [verb, name, status] if verb == "end" => {
            let status = status
                .parse::<i32>()
                .map_err(|_| format!("fleet-tmux-stub end: `{status}` is not a status"))?;
            server.end(name, Some(status)).map(|()| String::new())
        }
        _ => chain(&mut server, &socket, &commands(rest)),
    };
    // Saved whatever the answer, so the invocation is on record either way —
    // and a command that failed partway has changed only what the real server
    // would have changed before it stopped.
    server.save(&path)?;
    answer
}

/// The argument list split into commands, as tmux splits it: an argument
/// ending in `;` ends a command, and one ending in `\;` is that text with its
/// backslash dropped.
fn commands(args: &[String]) -> Vec<Vec<String>> {
    let mut out = vec![Vec::new()];
    for arg in args {
        match arg.strip_suffix(';') {
            Some(head) => match head.strip_suffix('\\') {
                Some(text) => out.last_mut().unwrap().push(format!("{text};")),
                None => {
                    if !head.is_empty() {
                        out.last_mut().unwrap().push(head.to_string());
                    }
                    out.push(Vec::new());
                }
            },
            None => out.last_mut().unwrap().push(arg.clone()),
        }
    }
    out.retain(|command| !command.is_empty());
    out
}

/// Run the commands in order, stopping at the first that fails, as the real
/// client stops. A chain holding no `new-session` against a server with no
/// sessions is a server that is not running.
fn chain(
    server: &mut FakeServer,
    socket: &str,
    commands: &[Vec<String>],
) -> Result<String, String> {
    let starts = commands.iter().any(|c| c[0] == "new-session");
    if !starts && server.sessions.is_empty() {
        return Err(format!("no server running on /fleet-tmux-stub/{socket}"));
    }
    let mut out = String::new();
    for command in commands {
        out.push_str(&one(server, command)?);
    }
    Ok(out)
}

/// The value after `flag` in a command.
fn flag<'a>(command: &'a [String], flag: &str) -> Option<&'a str> {
    command
        .iter()
        .position(|a| a == flag)
        .and_then(|i| command.get(i + 1))
        .map(String::as_str)
}

/// A session name from a target: `=name` or `=name:`.
fn target(command: &[String]) -> Result<String, String> {
    let t = flag(command, "-t").ok_or("no target")?;
    Ok(t.trim_start_matches('=').trim_end_matches(':').to_string())
}

fn one(server: &mut FakeServer, command: &[String]) -> Result<String, String> {
    match command[0].as_str() {
        "set-option" => Ok(String::new()),
        "new-session" => {
            let name = flag(command, "-s").ok_or("new-session: no -s")?;
            // The start directory is a format: `##` is a `#`.
            let cwd = flag(command, "-c").unwrap_or("").replace("##", "#");
            let split = command
                .iter()
                .position(|a| a == "--")
                .ok_or("new-session: no command")?;
            let (env, argv) = env_and_argv(&command[split + 1..]);
            server
                .start(name, &cwd, &argv, &env)
                .map(|()| String::new())
        }
        "load-buffer" => {
            let buffer = flag(command, "-b").ok_or("load-buffer: no -b")?;
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("load-buffer: {e}"))?;
            server.buffers.insert(buffer.to_string(), text);
            Ok(String::new())
        }
        "paste-buffer" => {
            let buffer = flag(command, "-b").ok_or("paste-buffer: no -b")?;
            let name = target(command)?;
            let text = server
                .buffers
                .get(buffer)
                .cloned()
                .ok_or_else(|| format!("no buffer {buffer}"))?;
            server.paste(&name, &text)?;
            if command.iter().any(|a| a == "-d") {
                server.buffers.remove(buffer);
            }
            Ok(String::new())
        }
        "send-keys" => {
            let name = target(command)?;
            let at = command.iter().position(|a| a == "-t").unwrap_or(0) + 2;
            let keys = &command[at.min(command.len())..];
            // A lone submit is what a send's second call is.
            if keys == ["C-m"] {
                server.submit(&name)
            } else {
                server.press(&name, keys)
            }
            .map(|()| String::new())
        }
        "capture-pane" => {
            let name = target(command)?;
            server.capture(&name).map(|screen| format!("{screen}\n"))
        }
        "kill-session" => {
            let name = target(command)?;
            if server.kill(&name) {
                Ok(String::new())
            } else {
                Err(format!("can't find session: {name}"))
            }
        }
        // Not a call `TmuxHost` makes: the defaults' `tmux-version` doctor
        // check asks it, for the server's session count. One name a line,
        // whatever the format asks for.
        "list-sessions" => Ok(server
            .sessions
            .keys()
            .map(|name| format!("{name}\n"))
            .collect()),
        "list-panes" => {
            let format = flag(command, "-F").ok_or("list-panes: no -F")?;
            Ok(server
                .panes()
                .iter()
                .map(|pane| format!("{}\n", render(format, pane)))
                .collect())
        }
        "attach-session" => {
            // Recorded before the target is read, as the argument list is: an
            // attach refused for its session still ran under what it carried.
            server.attach_envs.push(
                std::env::vars_os()
                    .map(|(k, v)| {
                        (
                            k.to_string_lossy().into_owned(),
                            v.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            );
            let name = target(command)?;
            if server.sessions.contains_key(&name) {
                Ok(String::new())
            } else {
                Err(format!("can't find session: {name}"))
            }
        }
        "kill-server" => {
            server.sessions.clear();
            Ok(String::new())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

/// `/usr/bin/env -i K=V… argv…`, taken apart again.
fn env_and_argv(command: &[String]) -> (Env, Vec<String>) {
    let rest = match command {
        [env, flag, rest @ ..] if env == "/usr/bin/env" && flag == "-i" => rest,
        _ => return (Vec::new(), command.to_vec()),
    };
    let split = rest
        .iter()
        .position(|a| !a.contains('='))
        .unwrap_or(rest.len());
    let env = rest[..split]
        .iter()
        .map(|kv| {
            let (k, v) = kv.split_once('=').expect("an assignment");
            (k.to_string(), v.to_string())
        })
        .collect();
    (env, rest[split..].to_vec())
}

/// A pane's environment, as the start handed it.
type Env = Vec<(String, String)>;

/// A list format with the six variables `TmuxHost` asks for filled in, and
/// any other variable empty.
fn render(format: &str, pane: &fleet_controller::host::Pane) -> String {
    use fleet_controller::host::PaneState;
    let mut out = String::new();
    let mut rest = format;
    while let Some(at) = rest.find("#{") {
        out.push_str(&rest[..at]);
        let Some(end) = rest[at..].find('}') else {
            break;
        };
        let var = &rest[at + 2..at + end];
        out.push_str(&match var {
            "session_name" => pane.session.clone(),
            "pane_pid" => pane.pid.map(|p| p.to_string()).unwrap_or_default(),
            "pane_dead" => match pane.state {
                PaneState::Alive => "0".into(),
                PaneState::Dead { .. } => "1".into(),
            },
            "pane_dead_status" => match pane.state {
                PaneState::Dead { status: Some(s) } => s.to_string(),
                _ => String::new(),
            },
            "session_created" => pane
                .created_ms
                .map(|ms| (ms / 1000).to_string())
                .unwrap_or_default(),
            "pane_current_path" => pane.path.clone(),
            _ => String::new(),
        });
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    out
}
