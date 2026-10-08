//! The agent as an adapter executable answers it: `docs/agent.md`'s contract
//! spoken to a process, one per call.
//!
//! ONE PROCESS PER CALL. Each verb runs `<adapter> <verb>` with the verb's
//! request on stdin — its fields, `schema_version` and the fleet's root, built
//! by [`types::request`] — and reads the answer off stdout through
//! [`types::answer`], which takes the first JSON value, demands the agent
//! contract's `schema_version` exactly and names the field that did not read.
//! `read` and `context` send every seat in one call, so a poll runs one process
//! per verb and never one per seat.
//!
//! THE CALL ITSELF IS EVERY ADAPTER'S: the spawn, the bound and its group
//! kill, the row an exit is and the line the adapter said last are
//! [`fleet_core::adapter::exec`]'s, which the store's caller shares. What is
//! the agent's here is the words: each row becomes a refusal naming the agent
//! contract, and exit 1 is the agent's own refusal, read by its reason.
//!
//! BOUNDED like every agent call: [`AGENT_TIMEOUT`], or what
//! [`TIMEOUT_VAR`] sets, and then the adapter's whole process group is killed
//! and the call is could not tell.
//!
//! SELECTED BY `[agent] adapter` naming an executable by absolute path, or by a
//! name an installed pack carries, which [`super::open`] reads out of the
//! fleet's own file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fleet_core::adapter::exec::{self, Exited, Ran, Unrun};
use fleet_core::agent::types::{self, Activities, Contexts, Seats, AGENT_TIMEOUT, TIMEOUT_VAR};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{
    Agent, AgentError, Argv, Capabilities, Launch, Refusal, Resume, SeatActivity, SeatContext,
    SeatRef, Version,
};

/// The agent as an adapter executable, its requests carrying one root.
#[derive(Clone, Debug)]
pub struct AgentExec {
    adapter: PathBuf,
    root: PathBuf,
    timeout: Duration,
    /// The `PATH` every call runs under, or `None` for this process's own.
    path: Option<String>,
}

/// Exit 1's answer: the refusal, under its one key.
#[derive(Deserialize)]
struct Refused {
    refused: Refusal,
}

impl AgentExec {
    /// The agent the executable at `adapter` answers for, every request
    /// carrying `root`, bounded by [`AGENT_TIMEOUT`].
    pub fn at(adapter: &Path, root: &Path) -> AgentExec {
        AgentExec {
            adapter: adapter.to_path_buf(),
            root: root.to_path_buf(),
            timeout: AGENT_TIMEOUT,
            path: None,
        }
    }

    /// The same agent under another bound.
    pub fn with_timeout(self, timeout: Duration) -> AgentExec {
        AgentExec { timeout, ..self }
    }

    /// The same agent with every call run under `path` as its `PATH`, in place
    /// of this process's own: the search path an entry resolves its runtime
    /// and its agent's binary on, which under a service is the constructed one
    /// and never the manager's.
    pub fn on_path(self, path: String) -> AgentExec {
        AgentExec {
            path: Some(path),
            ..self
        }
    }

    /// The executable every call runs.
    #[cfg(test)]
    pub fn entry(&self) -> &Path {
        &self.adapter
    }

    /// The root every request carries.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// One call as the adapter answers it, past the verbs' own types: the
    /// request sent exactly as given, `env` set on this call alone, and the
    /// row of the exit table it exited on, unread — what `fleet agent check`
    /// asks where a check is about the exit itself or replays a recorded
    /// case. Every call a fleet makes goes through [`Agent`]'s verbs instead.
    pub fn ran(&self, verb: &str, request: &Value, env: &[(String, String)]) -> Result<Ran, Unrun> {
        exec::run_with(
            &self.adapter,
            verb,
            request,
            self.timeout,
            self.path.as_deref(),
            env,
        )
    }

    /// One call: the verb's response body.
    ///
    /// Every row of the exit table but the answer and the adapter's own
    /// refusal is could not tell, worded once here and naming the agent
    /// contract; each carries what the adapter said last on stderr.
    fn call<T: DeserializeOwned>(
        &self,
        verb: &str,
        fields: Map<String, Value>,
    ) -> Result<T, AgentError> {
        let adapter = self.adapter.display();
        let named = format!("{adapter} {verb}");
        let request = types::request(fields, &self.root);
        let ran = exec::run(
            &self.adapter,
            verb,
            &request,
            self.timeout,
            self.path.as_deref(),
        )
        .map_err(|unrun| {
            AgentError::Unreadable(match unrun {
                Unrun::CouldNotRun(why) => format!("{adapter} could not be run ({why})"),
                // The variable is named because the bound is the one thing a
                // person can change about a call that outran it — an adapter
                // whose first exec the platform holds, among them.
                Unrun::Deadline(why) => format!("{named} {why} ({TIMEOUT_VAR} sets the bound)"),
            })
        })?;
        let said = &ran.said;
        let read = match &ran.exited {
            Exited::Answered(stdout) => types::answer::<T>(stdout).map_err(|why| {
                AgentError::Unreadable(format!("{named} answered no readable response: {why}"))
            }),
            Exited::Refused(stdout) => Err(match types::answer::<Refused>(stdout) {
                Ok(Refused { refused }) => AgentError::Refused(refused),
                Err(_) => AgentError::Unreadable(format!(
                    "{named} refused with no readable refusal: {said}"
                )),
            }),
            Exited::Usage => Err(AgentError::Unreadable(format!(
                "{named} refused the request as usage (exit 2) — this fleet and the adapter do \
                 not speak the same agent contract: {said}"
            ))),
            Exited::CouldNotTell(error) => Err(AgentError::Unreadable(format!(
                "{named} could not tell: {}",
                error.as_deref().unwrap_or(said)
            ))),
            Exited::OffTable(code) => Err(AgentError::Unreadable(format!(
                "{named} exited {code}, which is not a row of the agent contract's exit table: \
                 {said}"
            ))),
            Exited::Signalled => Err(AgentError::Unreadable(format!(
                "{named} was ended by a signal: {said}"
            ))),
        };
        read.map_err(|refused| carrying_stderr(refused, &ran))
    }
}

/// The refusal with what the adapter said last on stderr beside it, where it
/// said anything there and the refusal does not already carry it
/// ([`exec::carrying`]) — the adapter's own refusal's message included.
fn carrying_stderr(refused: AgentError, ran: &Ran) -> AgentError {
    match refused {
        AgentError::Refused(Refusal { reason, message }) => AgentError::Refused(Refusal {
            reason,
            message: exec::carrying(message, ran),
        }),
        AgentError::Unreadable(text) => AgentError::Unreadable(exec::carrying(text, ran)),
    }
}

/// A request type's own fields, as the verb's fields of a request.
fn fields_of(request: &impl Serialize) -> Map<String, Value> {
    match serde_json::to_value(request) {
        Ok(Value::Object(fields)) => fields,
        _ => Map::new(),
    }
}

impl Agent for AgentExec {
    /// What the adapter declares is held to the contract's rules for it
    /// before anything uses it, because a launch is built from its default
    /// model and first turn, and a posture it names is one a seat is started
    /// under.
    fn capabilities(&self) -> Result<Capabilities, AgentError> {
        let declared = self.declared()?;
        declared.validate().map_err(|why| {
            AgentError::Unreadable(format!(
                "{} capabilities answered no readable response: {why}",
                self.adapter.display()
            ))
        })?;
        Ok(declared)
    }

    fn declared(&self) -> Result<Capabilities, AgentError> {
        self.call("capabilities", Map::new())
    }

    fn version(&self) -> Result<Version, AgentError> {
        self.call("version", Map::new())
    }

    fn launch(&self, launch: &Launch) -> Result<Argv, AgentError> {
        self.call("launch", fields_of(launch))
    }

    fn resume(&self, resume: &Resume) -> Result<Argv, AgentError> {
        self.call("resume", fields_of(resume))
    }

    /// Every seat in one call.
    fn read(&self, seats: &[SeatRef]) -> Result<Vec<SeatActivity>, AgentError> {
        let asked = Seats {
            seats: seats.to_vec(),
        };
        self.call::<Activities>("read", fields_of(&asked))
            .map(|answered| answered.seats)
    }

    /// Every seat in one call, asked only of an adapter whose capabilities
    /// declare it: one that does not is never run for it.
    fn context(&self, seats: &[SeatRef]) -> Result<Vec<SeatContext>, AgentError> {
        if !self.capabilities()?.context {
            return Err(AgentError::Unreadable(String::from(
                "the adapter declares no context",
            )));
        }
        let asked = Seats {
            seats: seats.to_vec(),
        };
        self.call::<Contexts>("context", fields_of(&asked))
            .map(|answered| answered.seats)
    }
}

/// The rig every arm here runs an adapter on, and the opener's arms beside it
/// (`super::tests`): an adapter written as a `#!/bin/sh` stub.
#[cfg(test)]
pub(crate) mod tests {
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use fleet_core::agent::types::{Activity, Evidence, Permissions, Posture, RefusalReason};
    use fleet_core::seat::identity::SeatId;
    use serde_json::json;

    use super::*;

    const SEATS: [&str; 3] = [
        "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
        "0199a3c4-8f01-7a23-b456-c789d0e1f234",
        "0199a3c4-9a12-7b34-8c56-d789e0f1a2b3",
    ];

    /// A declaration that keeps the contract, with `context` as given.
    pub(crate) fn declared(context: bool) -> String {
        format!(
            r#"{{"schema_version":1,"postures":["ask","auto"],"default_model":"quill-large-2","first_turn":"/wake {{seat}}","context":{context},"measured":["2.4.0"]}}"#
        )
    }

    /// An adapter written as a `#!/bin/sh` stub in a directory of its own,
    /// which is also the root its requests carry. The stub writes its pid to
    /// `pid`, appends its verb to `argv`, copies its stdin to `<verb>.json`,
    /// and then runs `answer`, which reads its verb as `$1`.
    pub(crate) struct Stub {
        pub(crate) dir: PathBuf,
        pub(crate) bin: PathBuf,
    }

    impl Stub {
        pub(crate) fn new(label: &str, answer: &str) -> Stub {
            let dir = scratch(label);
            let bin = dir.join("adapter");
            write_stub(&bin, &dir, answer);
            Stub { dir, bin }
        }

        /// The agent over the stub, rooted in the stub's own directory, under
        /// a bound of a minute, which no arm here meets unless it means to:
        /// the first exec of a script just written can take tens of seconds on
        /// a busy macOS box, and an arm reading a row must not read the
        /// bound's.
        pub(crate) fn agent(&self) -> AgentExec {
            AgentExec::at(&self.bin, &self.dir).with_timeout(Duration::from_secs(60))
        }

        /// The last request the stub was handed for `verb`, as JSON.
        pub(crate) fn request(&self, verb: &str) -> Value {
            recorded(&self.dir, verb)
        }

        /// Every verb the stub was called with, in order: one per process.
        pub(crate) fn verbs(&self) -> Vec<String> {
            verbs_in(&self.dir)
        }

        /// Whether the process whose pid the stub wrote to `file` is still
        /// alive.
        fn alive(&self, file: &str) -> bool {
            let pid = std::fs::read_to_string(self.dir.join(file))
                .unwrap_or_else(|e| panic!("the stub wrote {file}: {e}"));
            Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("kill -0 {} 2>/dev/null", pid.trim()))
                .status()
                .expect("kill -0 ran")
                .success()
        }
    }

    impl Drop for Stub {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A fresh directory under the temp directory, unique to this call.
    pub(crate) fn scratch(label: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "fleet-agent-exec-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory is made");
        dir
    }

    /// The stub's script at `bin`, recording into `dir`, then running
    /// `answer`, made executable.
    pub(crate) fn write_stub(bin: &Path, dir: &Path, answer: &str) {
        std::fs::write(
            bin,
            format!(
                "#!/bin/sh\n\
                 echo $$ > '{dir}/pid'\n\
                 printf '%s\\n' \"$1\" >> '{dir}/argv'\n\
                 cat > \"{dir}/$1.json\"\n\
                 {answer}\n",
                dir = dir.display(),
            ),
        )
        .expect("the stub is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is executable");
    }

    /// The last request a stub recording into `dir` was handed for `verb`.
    pub(crate) fn recorded(dir: &Path, verb: &str) -> Value {
        let text = std::fs::read_to_string(dir.join(format!("{verb}.json")))
            .unwrap_or_else(|e| panic!("the stub recorded a {verb} request: {e}"));
        serde_json::from_str(&text).expect("the request is one JSON value")
    }

    /// Every verb a stub recording into `dir` was called with, in order.
    pub(crate) fn verbs_in(dir: &Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("argv"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// A stub body that prints `json` and exits `code`.
    pub(crate) fn answers(json: &str, code: i32) -> String {
        format!("cat <<'JSON'\n{json}\nJSON\nexit {code}")
    }

    fn seat_ref(id: &str, worktree: &str) -> SeatRef {
        SeatRef {
            seat: SeatId::parse(id).expect("a well-formed seat id"),
            session_id: None,
            pid: Some(4242),
            config_dir: None,
            worktree: worktree.to_string(),
            screen: None,
        }
    }

    fn a_launch(posture: Posture) -> Launch {
        Launch {
            seat: SeatId::parse(SEATS[0]).expect("a seat id"),
            worktree: "/work/lanes/a-seat".to_string(),
            name: "a-seat".to_string(),
            model: "quill-large-2".to_string(),
            posture,
            first_turn: "/wake a-seat".to_string(),
            config_dir: Some("/work/seats/a-seat/agent".to_string()),
            env: [("FLEET_ACTOR".to_string(), format!("seat:{}", SEATS[0]))].into(),
            permissions: Permissions {
                commands: vec!["cargo".to_string()],
                touched: None,
            },
        }
    }

    fn a_resume(posture: Posture) -> Resume {
        Resume {
            session_id: "5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d".to_string(),
            worktree: "/work/lanes/a-seat".to_string(),
            config_dir: None,
            model: "quill-large-2".to_string(),
            posture,
        }
    }

    fn unreadable<T: std::fmt::Debug>(result: Result<T, AgentError>) -> String {
        match result {
            Err(AgentError::Unreadable(why)) => why,
            other => panic!("wanted Unreadable, got {other:?}"),
        }
    }

    /// ONE READ FOR THREE SEATS IS ONE PROCESS: `<adapter> read`, whose stdin
    /// holds all three seats beside `schema_version` 1 and the root, and
    /// whose answer reads as one row per seat.
    ///
    /// RED-PROOF: a read that called the adapter once per seat leaves `argv`
    /// holding three lines, and a request holding one seat.
    #[test]
    fn one_read_of_three_seats_is_one_process_carrying_all_three() {
        let rows: Vec<String> = SEATS
            .iter()
            .map(|seat| format!(r#"{{"seat":"{seat}","activity":"idle","evidence":"typed"}}"#))
            .collect();
        let stub = Stub::new(
            "read",
            &answers(
                &format!(r#"{{"schema_version":1,"seats":[{}]}}"#, rows.join(",")),
                0,
            ),
        );
        let asked: Vec<SeatRef> = SEATS
            .iter()
            .map(|seat| seat_ref(seat, "/work/lanes/a-seat"))
            .collect();

        let read = stub.agent().read(&asked).expect("the stub answered a read");

        assert_eq!(
            stub.verbs(),
            ["read"],
            "argv[1] is the verb, and one process ran"
        );
        let request = stub.request("read");
        assert_eq!(request["schema_version"], 1);
        assert_eq!(request["root"], stub.dir.display().to_string());
        let sent: Vec<&str> = request["seats"]
            .as_array()
            .expect("the request carries the seats")
            .iter()
            .map(|seat| seat["seat"].as_str().expect("each seat is named"))
            .collect();
        assert_eq!(sent, SEATS, "every seat asked about, in one request");
        assert_eq!(request["seats"][0]["pid"], 4242);
        assert_eq!(read.len(), 3);
        assert!(read
            .iter()
            .all(|row| row.activity == Activity::Idle && row.evidence == Evidence::Typed));
    }

    /// A launch carries its request's fields beside the envelope and answers
    /// the argv and environment the adapter gave; a resume the same.
    #[test]
    fn a_launch_and_a_resume_carry_their_fields_and_answer_the_argv() {
        let stub = Stub::new(
            "launch",
            &answers(
                r#"{"schema_version":1,"argv":["quill","--mode","auto"],"env":{"QUILL_HOME":"/q"}}"#,
                0,
            ),
        );
        let agent = stub.agent();
        let argv = agent
            .launch(&a_launch(Posture::Auto))
            .expect("the stub answered a launch");
        assert_eq!(argv.argv, ["quill", "--mode", "auto"]);
        assert_eq!(argv.env.get("QUILL_HOME").map(String::as_str), Some("/q"));
        let request = stub.request("launch");
        assert_eq!(request["schema_version"], 1);
        assert_eq!(request["posture"], "auto");
        assert_eq!(request["name"], "a-seat");
        assert_eq!(request["permissions"]["commands"], json!(["cargo"]));

        agent
            .resume(&a_resume(Posture::Ask))
            .expect("the stub answered a resume");
        let request = stub.request("resume");
        assert_eq!(
            request["session_id"],
            "5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d"
        );
        assert_eq!(request["posture"], "ask");
        assert!(request.get("config_dir").is_none(), "{request}");
        assert_eq!(stub.verbs(), ["launch", "resume"]);
    }

    /// Each exit is read by its row, in the agent contract's words: 1 the
    /// adapter's own refusal by its reason — `unsupported` here — 2 usage, 3
    /// could not tell with the error it names, and 9 no row of the table at
    /// all. An exit 1 whose answer is no refusal is could not tell.
    ///
    /// ONE STUB answers every row, by the verb it is called with: the first
    /// exec of a script just written is the slow one.
    #[test]
    fn exits_1_2_3_and_9_are_read_by_their_rows() {
        let stub = Stub::new(
            "rows",
            "case \"$1\" in\n\
             launch) echo '{\"schema_version\":1,\"refused\":{\"reason\":\"unsupported\",\"message\":\"quill takes no posture unattended\"}}'; exit 1 ;;\n\
             version) exit 2 ;;\n\
             read) echo '{\"schema_version\":1,\"error\":\"the session index is locked\"}'; exit 3 ;;\n\
             resume) exit 9 ;;\n\
             capabilities) echo 'not a refusal'; exit 1 ;;\n\
             esac",
        );
        let agent = stub.agent();
        match agent.launch(&a_launch(Posture::Unattended)) {
            Err(AgentError::Refused(refusal)) => {
                assert_eq!(refusal.reason, RefusalReason::Unsupported);
                assert_eq!(refusal.message, "quill takes no posture unattended");
            }
            other => panic!("wanted the adapter's refusal, got {other:?}"),
        }

        let why = unreadable(agent.version());
        assert!(
            why.starts_with(&format!("{} version", stub.bin.display())),
            "{why}"
        );
        assert!(
            why.contains(
                "refused the request as usage (exit 2) — this fleet and the adapter do not speak \
                 the same agent contract"
            ),
            "{why}"
        );

        let why = unreadable(agent.read(&[seat_ref(SEATS[0], "/w")]));
        assert!(
            why.ends_with("read could not tell: the session index is locked"),
            "{why}"
        );

        let why = unreadable(agent.resume(&a_resume(Posture::Ask)));
        assert!(
            why.contains("resume exited 9, which is not a row of the agent contract's exit table"),
            "{why}"
        );

        let why = unreadable(agent.capabilities());
        assert!(
            why.ends_with("capabilities refused with no readable refusal: not a refusal"),
            "{why}"
        );
    }

    /// A call that outruns its bound — 300 ms here — has the stub's WHOLE
    /// GROUP killed, the stub and the sleep it forked, and is could not tell,
    /// naming the bound and the variable that sets it.
    ///
    /// The stub is RUN ONCE BEFORE THE CALL, answering at once: the first exec
    /// of a script just written can take longer than the whole bound on
    /// macOS, and a stub killed before it wrote its pid proves nothing about
    /// the kill.
    #[test]
    fn a_300ms_bound_kills_a_sleeping_stubs_group() {
        let bound = Duration::from_millis(300);
        let stub = Stub::new(
            "bound",
            "[ \"$1\" = warm ] && exit 0\nsleep 5 &\necho $! > \"${0%/*}/child\"\nwait",
        );
        let warmed = Command::new(&stub.bin)
            .arg("warm")
            .stdin(std::process::Stdio::null())
            .status()
            .expect("the stub runs");
        assert!(warmed.success(), "the warm-up answered");
        let _ = std::fs::remove_file(stub.dir.join("pid"));

        let started = Instant::now();
        let why = unreadable(stub.agent().with_timeout(bound).version());
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the call ended at its bound and not the sleep's: {:?}",
            started.elapsed()
        );
        assert_eq!(
            why,
            format!(
                "{} version did not answer within 300ms ({TIMEOUT_VAR} sets the bound)",
                stub.bin.display()
            )
        );
        assert!(!stub.alive("pid"), "the stub is gone");
        assert!(!stub.alive("child"), "and the sleep it forked in its group");
    }

    /// What the adapter said last on stderr is carried into the refusal: as
    /// the reason where it is the whole of what there is, beside the error an
    /// answer named, and beside the adapter's own refusal.
    #[test]
    fn the_last_line_of_stderr_appears_in_the_refusal() {
        let stub = Stub::new(
            "stderr",
            "echo early >&2\necho boom >&2\n\
             case \"$1\" in\n\
             version) exit 3 ;;\n\
             read) echo '{\"schema_version\":1,\"error\":\"the index is locked\"}'; exit 3 ;;\n\
             resume) echo '{\"schema_version\":1,\"refused\":{\"reason\":\"missing\",\"message\":\"no session s\"}}'; exit 1 ;;\n\
             esac",
        );
        let agent = stub.agent();
        let why = unreadable(agent.version());
        assert!(why.ends_with("version could not tell: boom"), "{why}");

        let why = unreadable(agent.read(&[seat_ref(SEATS[0], "/w")]));
        assert!(
            why.ends_with("could not tell: the index is locked (the adapter said: boom)"),
            "{why}"
        );

        match agent.resume(&a_resume(Posture::Ask)) {
            Err(AgentError::Refused(refusal)) => {
                assert_eq!(refusal.reason, RefusalReason::Missing);
                assert_eq!(refusal.message, "no session s (the adapter said: boom)");
            }
            other => panic!("wanted the adapter's refusal, got {other:?}"),
        }
    }

    /// `context` is asked only of an adapter whose capabilities declare it: a
    /// declaration of `context: false` runs `capabilities` and never
    /// `context`, and one of `true` runs both and reads the rows.
    ///
    /// RED-PROOF: with the declaration never read, `argv` holds `context`.
    #[test]
    fn context_is_never_run_for_an_adapter_declaring_context_false() {
        let row = format!(
            r#"{{"schema_version":1,"seats":[{{"seat":"{}","tokens":48210}}]}}"#,
            SEATS[0]
        );
        for (context, verbs) in [
            (false, vec!["capabilities"]),
            (true, vec!["capabilities", "context"]),
        ] {
            let stub = Stub::new(
                "context",
                &format!(
                    "case \"$1\" in\n\
                     capabilities) cat <<'JSON'\n{}\nJSON\n;;\n\
                     context) cat <<'JSON'\n{row}\nJSON\n;;\n\
                     esac",
                    declared(context)
                ),
            );
            let answered = stub.agent().context(&[seat_ref(SEATS[0], "/w")]);
            assert_eq!(stub.verbs(), verbs, "context: {context}");
            match (context, answered) {
                (false, Err(AgentError::Unreadable(why))) => {
                    assert_eq!(why, "the adapter declares no context")
                }
                (true, Ok(rows)) => assert_eq!(rows[0].tokens, Some(48210)),
                (_, other) => panic!("context: {context} answered {other:?}"),
            }
        }
    }

    /// A declaration that breaks the contract — no postures — is could not
    /// tell, naming the field, and an answer whose fields do not read names
    /// the one that did not.
    #[test]
    fn a_capabilities_with_empty_postures_is_refused_naming_the_field() {
        let stub = Stub::new(
            "postures",
            "case \"$1\" in\n\
             capabilities) echo '{\"schema_version\":1,\"postures\":[],\"default_model\":\"m\",\"first_turn\":\"/wake {seat}\",\"measured\":[\"2.4.0\"]}' ;;\n\
             read) echo '{\"schema_version\":1,\"seats\":[{\"seat\":\"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718\",\"activity\":\"napping\",\"evidence\":\"typed\"}]}' ;;\n\
             esac",
        );
        let why = unreadable(stub.agent().capabilities());
        assert!(
            why.contains("capabilities answered no readable response: postures is empty"),
            "{why}"
        );
        // The same answer as the adapter gave it, for the doctor to judge.
        let declared = stub
            .agent()
            .declared()
            .expect("the declaration reads before it is judged");
        assert!(declared.postures.is_empty(), "{declared:?}");
        let why = unreadable(stub.agent().read(&[seat_ref(SEATS[0], "/w")]));
        assert!(why.contains("seats[0].activity"), "{why}");
    }
}
