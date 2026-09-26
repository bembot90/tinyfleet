//! `fleet seat nudge` through the shipped binary, with the ring TYPED into the
//! seat's own session on `fleet-tmux-stub` and the agent's listing answered by
//! a stub script.
//!
//! The stub's `agents` serves a roster file until the seat's pane has taken a
//! submit, and the "taken" roster after it, where the arm wrote one — a session
//! that takes a typed turn reads busy on its next listing. What the projection
//! says is a file each arm writes, because the two refusals in front of the
//! delivery are readings of that document and of nothing else.
//!
//! No work graph anywhere: this verb writes to the event stream and never to the
//! store, so the project here is a directory with a policy file in it.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::test_support::{FakeServer, Sent};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const POLL_SECONDS: u64 = 5;
const POLICY: &str = "[controller]\nnudge_timeout_seconds = 20\n";

/// One arm's project, machine directory, seat worktree, provider stub and tmux
/// stub.
struct Rig {
    root: PathBuf,
    project: PathBuf,
    machine: PathBuf,
    worktree: PathBuf,
    stub: PathBuf,
    roster: PathBuf,
    /// What the listing reads once the seat's pane has taken a submit. Absent,
    /// the listing never moves: a session that leaves a typed turn untaken.
    roster_taken: PathBuf,
    /// The link `FLEET_TMUX_BIN` names, and the fake server's state beside it.
    tmux: PathBuf,
    state: PathBuf,
}

/// The seat's id and its own name. Its row is keyed by the id, its session on
/// the host is named by the id, and a person names it by the name.
const SEAT_ID: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";
const NAME: &str = "Orla";

impl Rig {
    fn new(label: &str) -> Rig {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "fleet-cli-nudge-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("a-project");
        let machine = root.join("machine");
        let worktree = root.join("worktree");
        for dir in [&project, &machine, &worktree] {
            std::fs::create_dir_all(dir).expect("the directory is created");
        }
        std::fs::write(project.join("fleet.toml"), POLICY).expect("the policy file is written");

        let tmux = common::stub_tmux(&root.join("tmux"));
        let rig = Rig {
            stub: root.join("agent.sh"),
            roster: root.join("roster.json"),
            roster_taken: root.join("roster-taken.json"),
            state: root.join("tmux").join("tmux-stub.json"),
            tmux,
            root,
            project,
            machine,
            worktree,
        };
        rig.named(NAME);
        rig.write_stub();
        rig.roster("[]");
        rig
    }

    /// The seat list, with the seat's row carrying this name.
    fn named(&self, name: &str) -> &Rig {
        std::fs::write(
            self.machine.join("config.json"),
            format!(
                r#"{{"fleet_toml": {fleet_toml}, "children": [
                     {{"id": "{SEAT_ID}", "name": "{name}",
                      "worktrees": {{"a-project": {worktree}}}}}
                   ]}}"#,
                fleet_toml = json_string(&self.project.join("fleet.toml").display().to_string()),
                worktree = json_string(&self.worktree.display().to_string()),
            ),
        )
        .expect("the machine config is written");
        self
    }

    /// A session table holding one row for the seat, keyed by its id and
    /// recording the name its session was started under.
    fn session_row(&self, session_name: &str) -> &Rig {
        std::fs::write(
            self.machine.join("sessions.json"),
            format!(
                r#"{{"schema": 2, "sessions": [{{
                     "seat": "{SEAT_ID}", "project": "a-project", "worktree": {worktree},
                     "name": "{session_name}", "model": "a-model", "posture": "auto",
                     "first_turn": "/wake {session_name}", "transient": false,
                     "dispatch_id": "a-dispatch", "dispatched_at": 1000,
                     "session_id": "abcdef", "short_id": "s0"
                   }}]}}"#,
                worktree = json_string(&self.worktree.display().to_string()),
            ),
        )
        .expect("the session table is written");
        self
    }

    /// The stub: `agents` is the listing, which reads the "taken" roster once
    /// the seat's pane has taken a submit and the arm wrote one
    /// ([`common::listing_branch`]).
    fn write_stub(&self) {
        std::fs::write(
            &self.stub,
            format!(
                "#!/bin/sh\ncase \"$1\" in\n{agents}\x20 *) exit 64 ;;\nesac\n",
                agents = common::listing_branch(&self.roster, &self.roster_taken, &self.state),
            ),
        )
        .expect("the stub is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&self.stub, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
    }

    fn roster(&self, body: &str) -> &Rig {
        std::fs::write(&self.roster, body).expect("the roster is written");
        self
    }

    /// The seat's session on the fake server, its pane alive, as the
    /// controller's start leaves it — named by the seat's id.
    fn pane(&self) -> u32 {
        common::live_pane(&self.state, SEAT_ID, &self.worktree)
    }

    /// A live pane, and a listed row carrying its pid that reads `status` and
    /// `busy` once the pane has taken a submit.
    fn listed(&self, status: &str) -> &Rig {
        let pid = self.pane();
        self.roster(&row(pid, status));
        std::fs::write(&self.roster_taken, row(pid, "busy")).expect("the roster is written");
        self
    }

    /// A live seat that takes what is typed: idle, then busy.
    fn live(&self) -> &Rig {
        self.listed("idle")
    }

    /// A live seat that leaves what is typed at its prompt: idle, and idle.
    fn never_takes(&self) -> &Rig {
        self.live();
        std::fs::remove_file(&self.roster_taken).expect("the taken roster is removed");
        self
    }

    /// The projection, as the collector would have published it `age` seconds
    /// ago, with one row for this seat in `state`, keyed by its id.
    fn projection(&self, state: &str, age: u64) -> &Rig {
        self.publish(&format!(
            r#"{{"version": 1, "generated_at": "{at}", "fleet": {{"poll_seconds": {POLL_SECONDS}}},
                 "seats": [{{"seat": {{"id": "{SEAT_ID}", "kind": "agent"}}, "roster_state": "{state}"}}]}}"#,
            at = stamp_secs_ago(age),
        ))
    }

    /// A projection with no row for this seat at all.
    fn projection_without_the_seat(&self) -> &Rig {
        self.publish(&format!(
            r#"{{"version": 1, "generated_at": "{at}", "fleet": {{"poll_seconds": {POLL_SECONDS}}},
                 "seats": [{{"seat": {{"id": "someone-else", "kind": "agent"}}, "roster_state": "present"}}]}}"#,
            at = stamp_secs_ago(0),
        ))
    }

    fn publish(&self, body: &str) -> &Rig {
        std::fs::write(self.machine.join("projection.json"), body)
            .expect("the projection is written");
        self
    }

    /// The shipped binary, run inside the project the policy file names, its
    /// tmux the stub.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet"))
            .args(args)
            .current_dir(&self.project)
            .hermetic(&self.root.join("home"), &self.machine, Some(&self.stub))
            .env(common::hermetic::TMUX_BIN, &self.tmux)
            .env("FLEET_ACTOR", "run:a-caller")
            .output()
            .expect("the built binary runs")
    }

    fn nudge(&self, extra: &[&str]) -> Output {
        let call = ["seat", "nudge", NAME, "--text", TEXT];
        self.run(&[&call[..], extra].concat())
    }

    /// Everything typed into the seat's session, in order.
    fn sends(&self) -> Vec<Sent> {
        common::typed_into(&self.state, SEAT_ID)
    }

    /// Every `session.nudged` line on the stream, parsed.
    fn nudged_events(&self) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(self.machine.join("events.jsonl")).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["type"] == "session.nudged")
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The message, with a shape no template around it would produce by accident.
const TEXT: &str = "look at the record — item q7 is yours";

/// The listing's one row: the session `abcdef`, its pid the pane's.
fn row(pid: u32, status: &str) -> String {
    format!("[{}]", common::listed_row("abcdef", pid, status))
}

fn json_string(value: &str) -> String {
    serde_json::Value::String(value.to_string()).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A `YYYY-MM-DDTHH:MM:SSZ` stamp `age` seconds in the past, written the way the
/// controller's clock writes one — the reader is strict about the shape.
fn stamp_secs_ago(age: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_secs();
    fleet_controller::clock::stamp_secs(now.saturating_sub(age))
}

/// The text is TYPED into the seat's own session — one paste of it, verbatim,
/// then one submit — and the line says `sent` because the listing turned busy.
#[test]
fn a_live_row_and_a_fresh_projection_carry_the_text_and_say_sent() {
    let rig = Rig::new("sent");
    rig.live().projection("present", 0);

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        out.stdout.is_empty(),
        "nothing goes to stdout: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(
        rig.sends(),
        vec![Sent::Paste(TEXT.to_string()), Sent::Submit],
        "the text is typed verbatim, and nothing wraps it"
    );

    let events = rig.nudged_events();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    let event = &events[0];
    assert_eq!(
        event["actor"],
        serde_json::json!({ "kind": "seat", "id": SEAT_ID }),
        "the line's actor is the seat nudged, by its id"
    );
    assert_eq!(event["payload"]["outcome"], serde_json::json!("sent"));
    assert_eq!(event["payload"]["source"], serde_json::json!("seat nudge"));
    assert_eq!(event["payload"]["session"], serde_json::json!("abcdef"));
    assert_eq!(
        event["payload"]["by"],
        serde_json::json!({ "kind": "run", "id": "a-caller" }),
        "who sent it is the payload's `by`, typed"
    );
    assert_eq!(
        event["payload"]["context_tokens"],
        serde_json::Value::Null,
        "the threshold nudge's keys are what tell the two apart, and this is not one"
    );
}

/// fleet-fmver's addressing: the seat is reached at the session ITS ID names on
/// fleet's own server, whatever it is called. No name reaches the address — not
/// the one the policy gives it today, and not the one its session row recorded
/// at start.
///
/// Orla's session came up as `orla-93b9739a`; the policy names her Wren now, so
/// her machine name is `wren-93b9739a`. She is nudged by her new name.
#[test]
fn a_renamed_seat_is_typed_into_the_session_its_id_names() {
    let rig = Rig::new("renamed");
    rig.named("Wren");
    rig.session_row("orla-93b9739a");
    rig.live().projection("present", 0);

    let out = rig.run(&["seat", "nudge", "wren", "--text", TEXT]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        rig.sends(),
        vec![Sent::Paste(TEXT.to_string()), Sent::Submit],
        "typed into the session named {SEAT_ID}"
    );
    let server = FakeServer::load(&rig.state).expect("the stub's state reads");
    assert_eq!(
        server.sessions.keys().collect::<Vec<_>>(),
        vec![SEAT_ID],
        "and no session under any name was asked for"
    );
    let events = rig.nudged_events();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    assert_eq!(
        events[0]["actor"],
        serde_json::json!({ "kind": "seat", "id": SEAT_ID })
    );
}

/// fleet-fmver's outcome: a turn that was typed and NEVER TAKEN — the listing
/// still idle when the bound closes — writes `failed` and exits 1, though every
/// host call returned 0. The send returning is a dispatch and never a witness.
#[test]
fn a_turn_the_session_never_takes_writes_a_failed_event_and_exits_one() {
    let rig = Rig::new("failed");
    rig.never_takes().projection("present", 0);

    let out = rig.nudge(&["--timeout", "1"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(
        rig.sends(),
        vec![Sent::Paste(TEXT.to_string()), Sent::Submit],
        "the turn was typed, and it is the taking that failed"
    );

    let events = rig.nudged_events();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    let outcome = events[0]["payload"]["outcome"]
        .as_str()
        .expect("the outcome is a string")
        .to_string();
    assert!(
        outcome.starts_with("failed: typed and not taken: still idle"),
        "{outcome}"
    );
}

/// A seat already mid-turn is typed into and QUEUED: exit 0, and the line says
/// queued and never sent (E5).
#[test]
fn a_seat_mid_turn_is_queued_and_exits_zero() {
    let rig = Rig::new("queued");
    rig.listed("busy").projection("present", 0);

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stderr(&out).contains("queued"), "{}", stderr(&out));
    assert_eq!(
        rig.sends(),
        vec![Sent::Paste(TEXT.to_string()), Sent::Submit]
    );
    let events = rig.nudged_events();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    assert_eq!(
        events[0]["payload"]["outcome"],
        serde_json::json!("queued: the seat was mid-turn")
    );
}

/// A seat whose row stands in front of a person is refused before any byte:
/// exit 1, and the line names the block.
#[test]
fn a_seat_at_a_dialog_is_refused_before_any_byte_and_exits_one() {
    let rig = Rig::new("blocked");
    let pid = rig.pane();
    rig.roster(&format!(
        r#"[{{"sessionId": "abcdef", "cwd": "/anywhere", "pid": {pid}, "status": "waiting",
             "waitingFor": "permission prompt"}}]"#
    ))
    .projection("present", 0);

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(rig.sends().is_empty(), "no byte: {:?}", rig.sends());
    let events = rig.nudged_events();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    assert_eq!(
        events[0]["payload"]["outcome"],
        serde_json::json!("refused: blocked on permission prompt")
    );
}

#[test]
fn no_projection_is_five_and_names_the_path_and_fleet_start() {
    let rig = Rig::new("no-projection");
    rig.live();

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let said = stderr(&out);
    assert!(
        said.contains(&rig.machine.join("projection.json").display().to_string()),
        "{said}"
    );
    assert!(said.contains("fleet start"), "{said}");
    assert!(
        rig.sends().is_empty(),
        "the refusal is in front of the delivery"
    );
}

#[test]
fn a_projection_older_than_three_polls_is_five_with_its_age() {
    let rig = Rig::new("stale");
    let age = POLL_SECONDS * 3 + 7;
    rig.live().projection("present", age);

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let said = stderr(&out);
    // A LOWER BOUND: the document was stamped `age` seconds back and the clock
    // keeps running between that write and the read, so the number printed is
    // at least `age` and pinning it exactly would flake on a loaded machine.
    let reported = said
        .split("s ago")
        .next()
        .and_then(|head| head.rsplit(", ").next())
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or_else(|| panic!("the refusal prints an age: {said}"));
    assert!(reported >= age, "the age is the document's own: {said}");
    assert!(
        rig.sends().is_empty(),
        "the refusal is in front of the delivery"
    );

    // The control: the same document inside the window delivers, so the age and
    // not the shape of the file is what the arm above read.
    rig.projection("present", POLL_SECONDS);
    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(0), "the control: {}", stderr(&out));
}

#[test]
fn a_seat_the_projection_does_not_carry_is_four() {
    let rig = Rig::new("no-row");
    rig.live().projection_without_the_seat();

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(stderr(&out).contains("no row"), "{}", stderr(&out));
    assert!(
        rig.sends().is_empty(),
        "a seat the collector has not published is not rung"
    );
}

#[test]
fn a_published_row_that_is_not_present_is_four() {
    let rig = Rig::new("not-present");
    rig.live().projection("prompt-blocked", 0);

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(4), "{}", stderr(&out));
    assert!(stderr(&out).contains("prompt-blocked"), "{}", stderr(&out));
}

/// The verb's own reading is the fresher one: a published live row over a pane
/// the listing carries no row for, and over no pane at all, is 4 — and nothing
/// was typed, so nothing is on the stream.
#[test]
fn a_live_published_row_over_an_empty_roster_is_four() {
    let rig = Rig::new("empty-roster");
    rig.projection("present", 0);

    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(4), "no pane: {}", stderr(&out));

    rig.pane();
    let out = rig.nudge(&[]);
    assert_eq!(out.status.code(), Some(4), "no row: {}", stderr(&out));
    assert!(
        rig.nudged_events().is_empty(),
        "nothing was delivered, so nothing is on the stream"
    );
    assert!(rig.sends().is_empty(), "{:?}", rig.sends());
}

#[test]
fn a_timeout_flag_bounds_the_turn_and_the_event_says_failed() {
    let rig = Rig::new("timeout");
    rig.never_takes().projection("present", 0);

    let started = std::time::Instant::now();
    let out = rig.nudge(&["--timeout", "1"]);
    let elapsed = started.elapsed();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));

    let events = rig.nudged_events();
    assert_eq!(events.len(), 1, "one event: {events:?}");
    let outcome = events[0]["payload"]["outcome"]
        .as_str()
        .expect("the outcome is a string")
        .to_string();
    assert!(outcome.contains("after 1s"), "{outcome}");

    // A LOWER BOUND ONLY. That the bound was honoured is what this asserts —
    // the turn was not refused instantly — and an upper bound here would be a
    // claim about how loaded the machine running it is.
    assert!(
        elapsed >= std::time::Duration::from_secs(1),
        "the turn was watched until its bound: {elapsed:?}"
    );
}

#[test]
fn the_verbs_help_says_the_message_carries_no_authority() {
    let out = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["seat", "nudge", "--help"])
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("CARRIES NO AUTHORITY"), "{help}");
    assert!(help.contains("doorbell"), "{help}");
    assert!(help.contains("--timeout"), "{help}");
    assert!(help.contains("--project"), "{help}");
}

#[test]
fn a_project_the_directory_does_not_resolve_to_is_a_usage_error() {
    let rig = Rig::new("project");
    rig.live().projection("present", 0);

    let out = rig.nudge(&["--project", "somewhere-else"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--project"), "{}", stderr(&out));

    // The control: the name this directory does resolve to gets past the gate.
    let out = rig.nudge(&["--project", "a-project"]);
    assert_eq!(out.status.code(), Some(0), "the control: {}", stderr(&out));
}
