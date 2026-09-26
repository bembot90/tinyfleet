//! The host seam, driven through one sequence three ways: against the real
//! tmux on a scratch server, against the `fleet-tmux-stub` binary the cli
//! suites point `FLEET_TMUX_BIN` at, and against the in-process `FakeHost`.
//!
//! THE REAL ARM NEVER TOUCHES `fleet`. It runs on a socket of its own,
//! `fleet-test-<pid>-<nanos>`, and a drop guard kills that server however the
//! arm ends. It is skipped with a printed reason where no tmux resolves on the
//! constructed PATH, so a box without one reads the other two arms and not a
//! red.
//!
//! What it measures that nothing else can: that tmux 3.7b keeps a dead pane's
//! exit status (`pane_dead_status`) under the remain-on-exit the start sets,
//! for a command that ran a while and for one that exited at once.

use fleet_controller::host::tmux::TmuxHost;
use fleet_controller::host::{Host, HostRead, Pane, PaneState};
use fleet_controller::platform;
use fleet_controller::test_support::{FakeHost, Sent};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// How long a pane is given to die once its process has been told to. Far
/// above the milliseconds a shell's exit takes.
const DEATH: Duration = Duration::from_secs(10);

/// A scratch directory under the temp root, named per arm and per run.
fn scratch(arm: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fleet-host-{arm}-{}-{}",
        std::process::id(),
        nanos()
    ));
    std::fs::create_dir_all(&dir).expect("the scratch directory is made");
    dir
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos()
}

/// Kills the scratch server however the arm ends, and takes the scratch
/// directory with it.
struct Guard {
    bin: PathBuf,
    socket: String,
    dir: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = Command::new(&self.bin)
            .args(["-L", &self.socket, "-f", "/dev/null", "kill-server"])
            .output();
        // tmux 3.7b leaves the socket FILE behind its server, so the file goes
        // too: `/tmp/tmux-<uid>/`, where a client with no TMUX_TMPDIR — which
        // is every client the host runs — puts it. Absent for the stub arm.
        extern "C" {
            fn getuid() -> u32;
        }
        let uid = unsafe { getuid() };
        let _ = std::fs::remove_file(format!("/tmp/tmux-{uid}/{}", self.socket));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The one pane of `name` in a listing that read.
fn pane_of(host: &dyn Host, name: &str) -> Option<Pane> {
    match host.list() {
        HostRead::Readable(panes) => panes.into_iter().find(|p| p.session == name),
        HostRead::Unreadable { cause } => panic!("the listing did not read: {cause}"),
    }
}

/// The pane's state once it has died, polled for up to [`DEATH`].
fn dead(host: &dyn Host, name: &str) -> PaneState {
    let until = Instant::now() + DEATH;
    loop {
        let pane = pane_of(host, name).expect("a dead pane is kept, not dropped");
        if matches!(pane.state, PaneState::Dead { .. }) {
            return pane.state;
        }
        assert!(Instant::now() < until, "{name} still alive after {DEATH:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn canonical(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The sequence, whichever host answers it. `paste` is the host's paste with
/// no submit; `end` stands in for a process that ends, and does nothing where
/// the process is real. Answers what it observed, for the arm to print.
fn sequence(
    host: &dyn Host,
    paste: &dyn Fn(&str, &str) -> Result<(), String>,
    end: &dyn Fn(&str, i32),
    dir: &Path,
) -> Vec<String> {
    let mut seen = Vec::new();
    let sh = |s: &str| s.to_string();

    // 1. A command that waits on its input is listed Alive, with a pid, in the
    //    directory it was started in.
    host.new_session(
        "reader",
        dir,
        &[sh("/bin/sh"), sh("-c"), sh("read x; exit 3")],
        &[(sh("HOME"), dir.display().to_string())],
    )
    .expect("the reader starts");
    let pane = pane_of(host, "reader").expect("the reader is listed");
    assert_eq!(pane.state, PaneState::Alive, "{pane:?}");
    assert!(pane.pid.is_some(), "a live pane has a pid: {pane:?}");
    assert_eq!(canonical(&pane.path), canonical(&dir.display().to_string()));
    assert!(pane.created_ms.is_some(), "{pane:?}");
    seen.push(format!("1: {pane:?}"));

    // A second start of the same name is refused, not a second session.
    let twice = host
        .new_session("reader", dir, &[sh("/bin/sh")], &[])
        .expect_err("a name is one session");
    assert!(twice.contains("duplicate session"), "{twice}");

    // 2. A send types the text and submits it; the capture reads it back.
    host.send("reader", "hi").expect("the send is taken");
    end("reader", 3);
    let screen = host.capture("reader").expect("the reader's pane captures");
    assert!(
        screen.lines().any(|l| l.trim() == "hi"),
        "the sent text is on the pane: {screen:?}"
    );

    // 3. The submit ended the read, and the pane is kept dead with the status.
    let state = dead(host, "reader");
    assert_eq!(state, PaneState::Dead { status: Some(3) });
    seen.push(format!("3: reader {state:?}"));
    let refused = paste("reader", "late").expect_err("a dead pane takes no paste");
    assert!(refused.contains("target pane has exited"), "{refused}");

    // 4. A command that exits at once still leaves its pane, and its status:
    //    remain-on-exit was on before the pane existed.
    host.new_session("quitter", dir, &[sh("/usr/bin/false")], &[])
        .expect("the quitter starts");
    end("quitter", 1);
    let state = dead(host, "quitter");
    assert_eq!(state, PaneState::Dead { status: Some(1) });
    seen.push(format!("4: quitter {state:?}"));

    // 5. Two lines pasted into cat are both on the pane before any submit.
    host.new_session("echo", dir, &[sh("/bin/cat")], &[])
        .expect("cat starts");
    paste("echo", "first line\nsecond line").expect("the paste is taken");
    std::thread::sleep(Duration::from_millis(300));
    let screen = host.capture("echo").expect("cat's pane captures");
    assert!(
        screen.contains("first line") && screen.contains("second line"),
        "both pasted lines, before the submit: {screen:?}"
    );
    host.keys("echo", &["C-m"]).expect("the submit is taken");

    // A kill ends a session, and a kill of one already gone is Ok.
    host.kill("echo").expect("cat's session is killed");
    host.kill("echo").expect("a session already gone is killed");
    assert!(
        pane_of(host, "echo").is_none(),
        "a killed session is not listed"
    );
    seen.push(format!("version: {:?}", host.version()));
    seen
}

/// The real arm: tmux itself, on a scratch server.
#[test]
fn the_real_host_keeps_a_dead_panes_status_on_a_scratch_server() {
    let child_path = platform::child_path(&platform::home_dir());
    // The real binary on purpose — this arm's subject is tmux itself — so the
    // seam and the hermetic flag are passed as absent rather than read.
    let host = match TmuxHost::resolve_from(None, false, &child_path) {
        Ok(host) => host,
        Err(why) => {
            println!("SKIPPED: no tmux resolves on the constructed PATH: {why}");
            return;
        }
    };
    let socket = format!("fleet-test-{}-{}", std::process::id(), nanos());
    assert_ne!(socket, fleet_controller::host::SOCKET);
    let host = host.on_socket(&socket);
    let dir = scratch("real");
    let _guard = Guard {
        bin: host.bin().to_path_buf(),
        socket: socket.clone(),
        dir: dir.clone(),
    };

    // No server behind the socket yet: a fleet after a reboot, readable and
    // empty.
    assert_eq!(host.list(), HostRead::Readable(Vec::new()));

    let seen = sequence(
        &host,
        &|name, text| host.paste(name, text),
        &|_, _| {},
        &dir,
    );
    for line in &seen {
        println!("measured on {}: {line}", host.bin().display());
    }
    assert!(host.version().is_some(), "tmux -V answers");

    // The last sessions killed, the server exits, and the socket reads as no
    // server again.
    host.kill("reader").expect("the reader is killed");
    host.kill("quitter").expect("the quitter is killed");
    assert_eq!(host.list(), HostRead::Readable(Vec::new()));
}

/// The stub arm: the same sequence through `fleet-tmux-stub`, as the cli
/// suites will drive it.
#[test]
fn the_stub_binary_answers_the_same_sequence() {
    let dir = scratch("stub");
    let state = dir.join("tmux-stub.json");
    // The host clears its clients' environment, so the state file is named by
    // where the link to the stub sits, not by a variable on this process — and
    // a link, not a wrapper script, whose first exec macOS spends 15 s or more
    // assessing.
    let wrapper = dir.join("tmux");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fleet-tmux-stub"), &wrapper)
        .expect("the link to the stub is made");
    let host = TmuxHost::resolve_from(Some(&wrapper.display().to_string()), true, "")
        .expect("the stub is named")
        .on_socket("fleet-test-stub");
    let _guard = Guard {
        bin: wrapper.clone(),
        socket: "fleet-test-stub".into(),
        dir: dir.clone(),
    };
    let end = |name: &str, status: i32| {
        let out = Command::new(&wrapper)
            .args(["end", name, &status.to_string()])
            .output()
            .expect("the stub runs");
        assert!(out.status.success(), "{out:?}");
    };

    assert_eq!(host.list(), HostRead::Readable(Vec::new()));
    let seen = sequence(&host, &|name, text| host.paste(name, text), &end, &dir);
    assert!(
        seen.iter().any(|l| l == "version: Some(\"3.7b\")"),
        "{seen:?}"
    );

    // The stub kept what the start handed it and what was typed.
    let server = fleet_controller::test_support::FakeServer::load(&state).expect("the state");
    let reader = &server.sessions["reader"];
    assert_eq!(reader.argv, ["/bin/sh", "-c", "read x; exit 3"]);
    assert!(reader
        .env
        .contains(&("TERM".into(), "tmux-256color".into())));
    assert_eq!(reader.sent, [Sent::Paste("hi".into()), Sent::Submit]);

    // An attach's argv is on the record, read-only unless asked.
    let status = host
        .attach("reader", false)
        .status()
        .expect("the attach runs against the stub");
    assert!(status.success());
    let server = fleet_controller::test_support::FakeServer::load(&state).expect("the state");
    let last = server.invocations.last().expect("the attach was recorded");
    assert!(
        last.contains(&"attach-session".to_string()) && last.contains(&"-r".to_string()),
        "{last:?}"
    );
}

/// The fake arm: the same sequence through the in-process `FakeHost`.
#[test]
fn the_fake_host_answers_the_same_sequence() {
    let dir = scratch("fake");
    let host = FakeHost::new();
    let seen = sequence(
        &host,
        &|name, text| host.paste(name, text),
        &|name, status| host.end(name, Some(status)),
        &dir,
    );
    assert!(
        seen.iter().any(|l| l == "version: Some(\"3.7b\")"),
        "{seen:?}"
    );
    assert_eq!(
        host.sends("reader"),
        [Sent::Paste("hi".into()), Sent::Submit]
    );
    let reader = host.session("reader").expect("the reader is kept dead");
    assert_eq!(reader.argv, ["/bin/sh", "-c", "read x; exit 3"]);

    // A listing set to fail reads Unreadable with its cause.
    host.fail(FakeHost::LIST, Some("server exploded"));
    assert_eq!(
        host.list(),
        HostRead::Unreadable {
            cause: "server exploded".into()
        }
    );
    let _ = std::fs::remove_dir_all(&dir);
}
