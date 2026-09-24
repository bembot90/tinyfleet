//! The file-access gate AS THE LOOP READS IT (lessons claude-code D4).
//!
//! A test binary of its own, because these arms drive `run::observe_with` and
//! that reads the PROCESS's environment for the machine directory and the agent
//! binary: an arm setting those beside arms that do not would be setting them
//! for every thread in the binary. Inside this one they are serialized on the
//! lock below, which each rig holds for its whole life.
//!
//! What it measures is the thing a pure reading of `projection::effects_of`
//! cannot: that the loop's per-seat effects AND its routines pass are both behind
//! the gate, so a pending grant leaves a seat that would otherwise have been
//! started exactly where it is.

use fleet_controller::platform::{self, Grant, Listing};
use fleet_controller::run::{self, Options};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

mod common;

/// The environment is the process's, so one rig runs at a time. A poisoned lock
/// is taken anyway: the panic that poisoned it already failed its own arm, and
/// refusing it here would fail every other arm for it.
static ENV: Mutex<()> = Mutex::new(());

fn write(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("the fixture directory is made");
    }
    std::fs::write(path, body).expect("the fixture file is written");
}

fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("the stub is executable");
}

fn projection(machine: &Path) -> serde_json::Value {
    let body = std::fs::read_to_string(machine.join("projection.json"))
        .expect("a projection is published");
    serde_json::from_str(&body).expect("the projection parses")
}

/// The rig: a machine directory naming one seat in a worktree that exists, and
/// an agent stub that records every call it is given.
struct Rig {
    root: PathBuf,
    machine: PathBuf,
    worktree: PathBuf,
    /// The second seat's worktree, where the roster's one live row stands.
    live: PathBuf,
    argv: PathBuf,
    _held: MutexGuard<'static, ()>,
}

impl Rig {
    fn new(label: &str) -> Rig {
        let held = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!("fleet-grant-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rig = Rig {
            machine: root.join("machine"),
            worktree: root.join("wt/a-seat"),
            live: root.join("wt/b-seat"),
            argv: root.join("agent-argv"),
            root,
            _held: held,
        };
        std::fs::create_dir_all(&rig.worktree).expect("the worktree is made");
        std::fs::create_dir_all(&rig.live).expect("the live seat's worktree is made");

        write(
            &rig.root.join("fleet.toml"),
            "[controller]\npoll_seconds = 1\n",
        );
        // A routine that rings the seat and is due on every tick. The routines pass
        // is the SECOND place the loop issues an effect, and it reads its own
        // agent handle: an arm with no routine in the fleet root would say
        // nothing about whether that half is behind the gate.
        write(
            &rig.root.join("orders/a-ring.toml"),
            // A CONDITION that is always true, so the routine is due on every
            // tick: a cooldown would make the second poll's silence the
            // interval's and not the gate's.
            "[order]\ndescription = \"ring the live seat\"\ntrigger = \"condition\"\n\
             check = \"exit 0\"\npoll = \"0s\"\n\
             [action.nudge]\nseat = \"b-seat\"\ntext = \"a sentence\"\n\
             authority = \"an arm\"\n",
        );
        write(
            &rig.machine.join("config.json"),
            &format!(
                "{{\"fleet_toml\": \"{}\", \"children\": [\
                 {{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"name\": \"a-seat\", \
                   \"worktrees\": {{\"a-project\": \"{}\"}}}}, \
                 {{\"id\": \"01a0d1f1-0aec-765f-9abe-d4f993b9739a\", \"name\": \"b-seat\", \
                   \"worktrees\": {{\"a-project\": \"{}\"}}}}]}}\n",
                rig.root.join("fleet.toml").display(),
                rig.worktree.display(),
                rig.live.display()
            ),
        );

        // The agent: a roster carrying ONE live row, in `b-seat`'s worktree.
        //
        // TWO SEATS, because the loop's two effect paths want opposite states.
        // `a-seat` is absent, so its verdict is a spawn — the per-seat effect.
        // `b-seat` is live, so the routine's ring has somebody to reach — the
        // routines pass's effect. A ring at an absent seat fails for its own
        // reason and would say nothing about the gate.
        let stub = rig.root.join("agent.sh");
        let roster = format!(
            "[{{\"id\":\"bb\",\"sessionId\":\"b-session\",\"cwd\":\"{}\",\
             \"kind\":\"background\",\"pid\":4242,\"status\":\"idle\",\"startedAt\":1000}}]",
            rig.live.display()
        );
        write(
            &stub,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {argv}\ncase \"$*\" in\n  \
                 *--version*) echo 2.1.261 ;;\n  *) printf '%s' '{roster}' ;;\nesac\nexit 0\n",
                argv = rig.argv.display(),
            ),
        );
        executable(&stub);
        for (key, value) in common::hermetic::vars(&rig.root, &rig.machine, Some(&stub)) {
            std::env::set_var(key, value);
        }
        rig
    }

    /// The agent calls that are EFFECTS, told from the observation calls the
    /// loop makes every poll whatever the gate says: a start names the model
    /// and the permission posture, and a roster read names neither.
    fn effect_calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.argv)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains("--model") || line.contains("--permission-mode"))
            .map(str::to_string)
            .collect()
    }

    fn stream(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.machine.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Every event the loop wrote ABOUT a session, which is what the per-seat
    /// effect path leaves behind.
    fn session_events(&self) -> Vec<String> {
        self.kinds("session.")
    }

    /// And the routines pass's own ledger, which is the other path's.
    fn routine_events(&self) -> Vec<String> {
        self.kinds("routine.")
    }

    fn kinds(&self, prefix: &str) -> Vec<String> {
        self.stream()
            .into_iter()
            .filter_map(|event| event["type"].as_str().map(str::to_string))
            .filter(|kind| kind.starts_with(prefix))
            .collect()
    }

    /// Every string a routine's firing put in a payload, which is where the
    /// reason it gives travels.
    fn routine_reasons(&self) -> Vec<String> {
        self.stream()
            .into_iter()
            .filter(|event| {
                event["type"]
                    .as_str()
                    .map(|kind| kind.starts_with("routine."))
                    .unwrap_or(false)
            })
            .flat_map(|event| {
                event["payload"]
                    .as_object()
                    .map(|payload| {
                        payload
                            .values()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A WORKTREE THAT IS NOT THERE IS NOT A GRANT QUESTION.
///
/// The spec's own first-run sequence is: add `[seats.x]` to the policy file,
/// run `fleet start`, and THEN `git worktree add` — §5 says the render does not
/// create the directory, because starting a named seat is the person's. So the
/// ordinary state between those two acts is a configured worktree that does not
/// exist yet, and a gate that read an absent directory as a refusal would
/// publish the whole fleet as pending with effects off, fleet-wide, on the one
/// sequence the spec prescribes.
#[test]
fn a_worktree_the_person_has_not_made_yet_leaves_the_grant_ok_and_the_effects_on() {
    let rig = Rig::new("absent");
    // The seat's row names a worktree, and the directory is taken away — which
    // is exactly the state after `fleet start` and before `git worktree add`.
    std::fs::remove_dir_all(&rig.worktree).expect("the worktree is taken away");
    assert!(!rig.worktree.exists());

    let mut gate = Grant::new(platform::directory_listing(), Duration::from_secs(5));
    let read = gate.poll(&[rig.worktree.clone(), rig.live.clone()]);
    assert!(read.is_ok(), "an absent directory is not pending: {read:?}");
    assert_eq!(read.detail, None);

    assert_eq!(run::observe_with(&Options { once: true }, gate), 0);
    let document = projection(&rig.machine);
    assert_eq!(document["grant"], platform::GRANT_OK, "{document}");
    assert!(document.get("grant_detail").is_none(), "{document}");
    assert_eq!(document["effects"]["state"], "on", "{document}");
    // The seat was DECIDED about and ACTED ON — the loop reached its effect
    // rather than skipping the whole arm. What the start then made of a cwd
    // that is not there is the effect layer's business and not the gate's; the
    // gate's claim is that it did not hold it.
    assert_ne!(
        document["seats"][0]["decision"], "pending",
        "the seat was decided about: {document}"
    );
    assert_ne!(
        document["seats"][0]["outcome"], "none",
        "the loop carried the verdict out rather than holding it: {document}"
    );
}

/// While the grant is pending the loop issues NO effect — not a per-seat one and
/// not a routine's ring — and publishes effects off with the grant as the cause.
/// Once the listing answers, the same loop starts the seat it was holding.
///
/// The control is the second half: without it a loop that never starts anything
/// would satisfy the first half and say nothing.
#[test]
fn a_pending_grant_holds_every_effect_and_an_answered_one_releases_them() {
    let rig = Rig::new("pending");

    let blocked = Arc::new(AtomicBool::new(true));
    let held = Arc::clone(&blocked);
    let listing: Listing = Arc::new(move |_: &Path| {
        while held.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    });
    let mut gate = Grant::new(Arc::clone(&listing), Duration::from_millis(50));

    // The gate is polled here exactly as the loop polls it, so the first tick
    // meets a probe that is already outstanding — which is the state D4 is
    // about, and the one a fresh probe inside the poll would not reach.
    let read = gate.poll(&[rig.worktree.clone(), rig.live.clone()]);
    if !platform::grant_is_gated() {
        // On a platform with no dialog in front of a read the gate is ok by
        // construction; the arm asserts THAT rather than returning unmeasured,
        // and the loop below is then the control's half alone.
        assert!(read.is_ok());
        blocked.store(false, Ordering::SeqCst);
        assert_eq!(run::observe_with(&Options { once: true }, gate), 0);
        let document = projection(&rig.machine);
        assert_eq!(document["grant"], platform::GRANT_OK);
        assert_eq!(document["effects"]["state"], "on");
        assert!(
            !rig.effect_calls().is_empty(),
            "the loop issued the seat's effect"
        );
        return;
    }
    assert_eq!(read.state, platform::GRANT_PENDING);

    assert_eq!(run::observe_with(&Options { once: true }, gate), 0);

    let document = projection(&rig.machine);
    assert_eq!(document["grant"], platform::GRANT_PENDING);
    assert!(
        document["grant_detail"]
            .as_str()
            .unwrap_or_default()
            .contains(&rig.worktree.display().to_string()),
        "the detail names the worktree: {document}"
    );
    assert_eq!(document["effects"]["state"], "off");
    assert_eq!(
        document["effects"]["cause"], document["grant_detail"],
        "the cause names the grant: {document}"
    );
    assert_eq!(
        document["seats"][0]["outcome"], "none",
        "no effect was issued for the seat: {document}"
    );
    assert!(
        rig.effect_calls().is_empty(),
        "the loop issued an effect while the grant was pending: {:?}",
        rig.effect_calls()
    );
    assert!(
        rig.session_events().is_empty(),
        "a per-seat effect left an event behind while the grant was pending: {:?}",
        rig.session_events()
    );

    // THE ORDERS PASS IS THE SECOND EFFECT PATH and it is behind the same gate.
    // A due routine that rings the LIVE seat reaches no agent: it is answered
    // could-not-tell with the grant as the cause, exactly as it is answered
    // when the agent binary cannot be resolved.
    assert!(
        rig.routine_events()
            .iter()
            .any(|kind| kind == "routine.could_not_tell"),
        "the ring was answered could-not-tell: {:?}",
        rig.routine_events()
    );
    assert!(
        !rig.routine_events()
            .iter()
            .any(|kind| kind == "routine.completed"),
        "the routine rang somebody while the grant was pending: {:?}",
        rig.routine_events()
    );
    assert!(
        rig.routine_reasons()
            .iter()
            .any(|why| Some(why.as_str()) == document["grant_detail"].as_str()),
        "and the reason it gives is the grant: {:?}",
        rig.routine_reasons()
    );

    // THE CONTROL: the dialog is answered, the same seat is still absent, and
    // this poll issues the effect the one above held. Without this half the
    // assertions above would hold for a loop that starts nothing ever.
    blocked.store(false, Ordering::SeqCst);
    let mut gate = Grant::new(listing, Duration::from_secs(5));
    assert!(gate.poll(&[rig.worktree.clone(), rig.live.clone()]).is_ok());
    assert_eq!(run::observe_with(&Options { once: true }, gate), 0);

    let document = projection(&rig.machine);
    assert_eq!(document["grant"], platform::GRANT_OK);
    assert!(document.get("grant_detail").is_none(), "{document}");
    assert_eq!(document["effects"]["state"], "on");
    assert_ne!(
        document["seats"][0]["outcome"], "none",
        "the held effect is issued once the grant reads ok: {document}"
    );
    assert!(
        !rig.effect_calls().is_empty(),
        "the loop issued the seat's effect this time"
    );
    assert!(
        !rig.session_events().is_empty(),
        "and left the event behind: {:?}",
        rig.session_events()
    );
    // And the ring reached the agent this time, which is what says the
    // could-not-tell above was the gate's and not a routine that never rings.
    assert!(
        rig.routine_events()
            .iter()
            .any(|kind| kind == "routine.completed"),
        "the routine rang the live seat once the grant read ok: {:?}",
        rig.routine_events()
    );
}
