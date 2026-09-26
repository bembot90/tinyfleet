//! One call of an adapter executable: `<adapter> <verb>` with the request on
//! stdin, bounded, and its exit read through the contracts' one table.
//!
//! THE ENVELOPE. A request is the verb's fields with `schema_version` and the
//! project's `root` inserted beside them ([`envelope`]); an answer is the
//! FIRST JSON value of stdout, an object at the contract's `schema_version`
//! exactly, read with that key taken out ([`answer`]). The version is the
//! caller's to pass, because each contract states its own and grows it on its
//! own. The working directory is inherited and never relied on: the root is in
//! the request.
//!
//! THE EXIT TABLE, and nowhere else is an exit read: 0 is the answer, 1 the
//! record's refusal, 2 a request the adapter does not speak, 3 could not tell,
//! and any other code or a signal is a row of its own that the caller reads as
//! could not tell too. [`run`] answers the row as an [`Exited`] and words
//! nothing: the sentence a row becomes names its contract ("the store
//! contract", "the agent contract"), so it is the caller's.
//!
//! BOUNDED: the process leads a group of its own, and a call that outruns its
//! bound has the whole group killed and is [`Unrun::Deadline`]. The runner is
//! [`crate::process`]'s, which every bounded child in fleet goes through.
//!
//! WHAT THE ADAPTER SAID LAST — the last line of stderr that is not blank,
//! else of stdout ([`tail`]) — rides on every [`Ran`], because that line is
//! for the person reading the refusal, and [`carrying`] puts it beside
//! whatever the caller refuses with.

use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::process::{deadline_cause, run_bounded_fed};

/// A call that never reached its exit: nothing the adapter answered can be
/// read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrun {
    /// The process could not be started or waited on, with the runner's own
    /// reason, which names the program.
    CouldNotRun(String),
    /// The call outran its bound and its whole group was killed. The text is
    /// the bound named whole — `did not answer within 1s` — as
    /// [`deadline_cause`] words it.
    Deadline(String),
}

/// The row of the exit table the process exited on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exited {
    /// 0: the answer, as the whole of stdout, which the caller reads through
    /// [`answer`] into the verb's response type.
    Answered(String),
    /// 1: the record's refusal, as the whole of stdout, which the caller
    /// reads into its contract's refusal type.
    Refused(String),
    /// 2: a request the adapter does not speak.
    Usage,
    /// 3: could not tell, with the `error` the answer carried where stdout's
    /// first value names one as text.
    CouldNotTell(Option<String>),
    /// A code the table has no row for.
    OffTable(i32),
    /// Ended by a signal, with no code at all.
    Signalled,
}

/// A call that reached its exit.
///
/// Stdout rides on the two rows that have an answer to read, and nowhere
/// else: every other row is worded from what the adapter said last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub exited: Exited,
    /// What the adapter said last ([`tail`]): the line a refusal carries.
    pub said: String,
    /// Whether stderr was blank, in which case [`Ran::said`] is stdout's last
    /// line and [`carrying`] adds nothing: a line off stdout is part of the
    /// answer, which the refusal already reads.
    pub stderr_blank: bool,
}

/// One call: `<adapter> <verb>`, with `request` on stdin as one line of JSON,
/// bounded by `timeout`, and run under `path` as its `PATH` where one is given
/// — the search path an entry resolves its runtime and its binary on, which
/// under a service is the constructed one and never the manager's — or this
/// process's own where it is not.
pub fn run(
    adapter: &Path,
    verb: &str,
    request: &Value,
    timeout: Duration,
    path: Option<&str>,
) -> Result<Ran, Unrun> {
    let mut cmd = Command::new(adapter);
    cmd.arg(verb);
    if let Some(path) = path {
        cmd.env("PATH", path);
    }
    let out = run_bounded_fed(cmd, request.to_string().into_bytes(), timeout).map_err(|why| {
        if why == deadline_cause(timeout) {
            Unrun::Deadline(why)
        } else {
            Unrun::CouldNotRun(why)
        }
    })?;
    let said = tail(&out);
    let stderr_blank = String::from_utf8_lossy(&out.stderr).trim().is_empty();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let exited = match out.status.code() {
        Some(0) => Exited::Answered(stdout),
        Some(1) => Exited::Refused(stdout),
        Some(2) => Exited::Usage,
        Some(3) => Exited::CouldNotTell(
            first_value(&stdout)
                .and_then(|answer| answer.get("error")?.as_str().map(str::to_string)),
        ),
        Some(code) => Exited::OffTable(code),
        None => Exited::Signalled,
    };
    Ok(Ran {
        exited,
        said,
        stderr_blank,
    })
}

/// `text` with what the adapter said last on stderr beside it, where it said
/// anything there and `text` does not already carry it.
pub fn carrying(text: String, ran: &Ran) -> String {
    if ran.stderr_blank || text.contains(&ran.said) {
        text
    } else {
        format!("{text} (the adapter said: {})", ran.said)
    }
}

/// A verb's fields as one request: `schema_version` at the contract's
/// `version` and the project's `root` inserted beside them.
pub fn envelope(mut fields: Map<String, Value>, root: &Path, version: u64) -> Value {
    fields.insert(String::from("schema_version"), Value::from(version));
    fields.insert(
        String::from("root"),
        Value::from(root.display().to_string()),
    );
    Value::Object(fields)
}

/// An answer's response body, read off the FIRST JSON value of the text with
/// whatever trails it ignored: an object at `schema_version` `version`
/// exactly, with that key taken out before the body is decoded.
pub fn answer<T: DeserializeOwned>(text: &str, version: u64) -> Result<T, String> {
    let Some(value) = first_value(text) else {
        return Err(String::from("answered no JSON value"));
    };
    let Value::Object(mut body) = value else {
        return Err(format!("answered {value}, which is not an object"));
    };
    match body.remove("schema_version") {
        None => return Err(String::from("answered no schema_version")),
        Some(answered) if answered.as_u64() != Some(version) => {
            return Err(format!(
                "answered schema_version {answered}; this fleet speaks {version}"
            ));
        }
        Some(_) => {}
    }
    serde_json::from_value(Value::Object(body))
        .map_err(|why| format!("the answer does not read — {why}"))
}

/// What a call said last: the last line of its stderr that is not blank, else
/// of its stdout, cut to 160 characters.
pub fn tail(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let body = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout)
    } else {
        stderr
    };
    body.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("no output")
        .chars()
        .take(160)
        .collect()
}

/// The first JSON value of an answer, with whatever trails it discarded.
///
/// Public beside [`answer`], which reads an answer through it, because an
/// adapter's own reader of a request reads the same way.
pub fn first_value(text: &str) -> Option<Value> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    stream.next().and_then(Result::ok)
}

/// The rig every adapter's arms run a call on: this module's own, and each
/// contract's caller through `pub(crate)`, which adds the one method that
/// opens its contract over the stub.
#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    /// An adapter written as a `#!/bin/sh` stub in a directory of its own,
    /// which is also the project root it is handed. The stub writes its pid to
    /// `pid`, appends its verb to `argv`, copies its stdin to `request.json`,
    /// and then runs `answer`.
    pub(crate) struct Stub {
        pub(crate) dir: PathBuf,
        pub(crate) bin: PathBuf,
    }

    impl Stub {
        pub(crate) fn new(label: &str, answer: &str) -> Stub {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("fleet-exec-{label}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the stub's directory is made");
            let bin = dir.join("adapter");
            std::fs::write(
                &bin,
                format!(
                    "#!/bin/sh\n\
                     echo $$ > '{dir}/pid'\n\
                     printf '%s\\n' \"$1\" >> '{dir}/argv'\n\
                     cat > '{dir}/request.json'\n\
                     {answer}\n",
                    dir = dir.display(),
                ),
            )
            .expect("the stub is written");
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("the stub is executable");
            Stub { dir, bin }
        }

        /// The last request the stub was handed, as JSON.
        pub(crate) fn request(&self) -> Value {
            let text = std::fs::read_to_string(self.dir.join("request.json"))
                .expect("the stub recorded its request");
            serde_json::from_str(&text).expect("the request is one JSON value")
        }

        /// Every verb the stub was called with, in order.
        pub(crate) fn verbs(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("argv"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        /// One call of `verb` carrying `request`, under the store's own bound
        /// of a minute, which no arm here meets unless it means to: the first
        /// exec of a script just written can take tens of seconds on a busy
        /// macOS box, and an arm reading a row must not read the bound's.
        fn run(&self, verb: &str, request: &Value) -> Result<Ran, Unrun> {
            run(&self.bin, verb, request, Duration::from_secs(60), None)
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

    /// A stub body that prints `json` and exits `code`.
    pub(crate) fn answers(json: &str, code: i32) -> String {
        format!("cat <<'JSON'\n{json}\nJSON\nexit {code}")
    }

    /// A call knows no verb and no answer's shape: a verb no store speaks,
    /// answered with an object no contract here types, is argv[1] and comes
    /// back Answered with the whole of stdout — the trailing line included —
    /// and the request on stdin is the value it was handed.
    #[test]
    fn any_verb_is_argv_1_and_its_answer_comes_back_whole() {
        let answer = r#"{"schema_version":1,"screen":{"rows":["> _"],"blocked":null}}"#;
        let stub = Stub::new(
            "read",
            &format!("cat <<'JSON'\n{answer}\nJSON\necho 'and a line after'"),
        );
        let request = envelope(
            match json!({ "seat": "orla", "lines": 40 }) {
                Value::Object(fields) => fields,
                _ => unreachable!("an object"),
            },
            &stub.dir,
            1,
        );
        let ran = stub.run("read", &request).expect("the stub ran");

        assert_eq!(
            stub.verbs(),
            ["read"],
            "argv[1] is the verb, and one call ran"
        );
        assert_eq!(stub.request(), request, "the request went in whole");
        assert_eq!(
            ran.exited,
            Exited::Answered(format!("{answer}\nand a line after\n")),
            "the whole of stdout, and nothing read out of it"
        );
        assert!(ran.stderr_blank);
        assert_eq!(
            ran.said, "and a line after",
            "stdout's last line, stderr being blank"
        );
    }

    /// Each exit is the row of the table it names: 1 carries its stdout for
    /// the caller's refusal type, 3 the `error` its answer names and none
    /// where it names none, and 7 and a signal are rows of their own.
    ///
    /// ONE STUB answers every row, by the verb it is called with: the first
    /// exec of a script just written is the slow one, and six scripts are six
    /// of them.
    #[test]
    fn each_exit_is_its_row_of_the_table() {
        let refusal = r#"{"schema_version":1,"refused":{"reason":"missing","message":"no"}}"#;
        let untold = r#"{"schema_version":1,"error":"the database is locked"}"#;
        let stub = Stub::new(
            "rows",
            &format!(
                "case \"$1\" in\n\
                 refused) echo '{refusal}'; exit 1 ;;\n\
                 usage) exit 2 ;;\n\
                 untold) echo '{untold}'; exit 3 ;;\n\
                 untold-bare) echo 'not json'; exit 3 ;;\n\
                 seven) exit 7 ;;\n\
                 signal) kill -9 $$ ;;\n\
                 esac"
            ),
        );
        for (verb, row) in [
            ("refused", Exited::Refused(format!("{refusal}\n"))),
            ("usage", Exited::Usage),
            (
                "untold",
                Exited::CouldNotTell(Some(String::from("the database is locked"))),
            ),
            ("untold-bare", Exited::CouldNotTell(None)),
            ("seven", Exited::OffTable(7)),
            ("signal", Exited::Signalled),
        ] {
            let ran = stub.run(verb, &json!({})).expect("the stub ran");
            assert_eq!(ran.exited, row, "{verb}");
        }
    }

    /// A call that outruns its bound is Deadline, naming the bound whole, and
    /// the stub's WHOLE GROUP is gone: the stub itself and the sleep it forked
    /// and waits on.
    ///
    /// The stub is RUN ONCE BEFORE THE CALL, answering at once: the first exec
    /// of a script just written can take longer than the whole bound on
    /// macOS, and a stub killed before it wrote its pid proves nothing about
    /// the kill.
    #[test]
    fn a_call_past_its_bound_kills_the_whole_group() {
        let bound = Duration::from_millis(200);
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

        let unrun =
            run(&stub.bin, "read", &json!({}), bound, None).expect_err("the call outran its bound");
        assert_eq!(
            unrun,
            Unrun::Deadline(String::from("did not answer within 200ms"))
        );
        assert!(!stub.alive("pid"), "the stub is gone");
        assert!(!stub.alive("child"), "and the sleep it forked in its group");
    }

    /// A program that cannot be started is CouldNotRun, in the runner's words,
    /// which name it.
    #[test]
    fn a_program_that_cannot_start_could_not_run() {
        let stub = Stub::new("missing", "exit 0");
        let nothing = stub.dir.join("nothing-here");
        match run(
            &nothing,
            "version",
            &json!({}),
            Duration::from_secs(5),
            None,
        ) {
            Err(Unrun::CouldNotRun(why)) => {
                assert!(why.contains("could not start"), "{why}");
                assert!(why.contains("nothing-here"), "{why}");
            }
            other => panic!("wanted CouldNotRun, got {other:?}"),
        }
    }

    /// The last line of stderr is what the adapter said, and is carried
    /// beside a text that does not already hold it — and never where stderr
    /// was blank, because stdout's line is part of the answer.
    #[test]
    fn what_the_adapter_said_on_stderr_is_carried_once() {
        let stub = Stub::new("stderr", "echo early >&2\necho boom >&2\necho out\nexit 3");
        let ran = stub.run("version", &json!({})).expect("the stub ran");
        assert!(!ran.stderr_blank);
        assert_eq!(ran.said, "boom", "stderr's last line, over stdout's");
        assert_eq!(
            carrying(String::from("could not tell"), &ran),
            "could not tell (the adapter said: boom)"
        );
        assert_eq!(
            carrying(String::from("could not tell: boom"), &ran),
            "could not tell: boom",
            "a text already carrying it is left alone"
        );

        let stub = Stub::new("stdout-only", "echo out\nexit 3");
        let ran = stub.run("version", &json!({})).expect("the stub ran");
        assert!(ran.stderr_blank);
        assert_eq!(
            carrying(String::from("could not tell"), &ran),
            "could not tell"
        );
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Named {
        name: String,
    }

    /// An answer is read at the version its caller speaks and no other, and
    /// a refusal names both numbers.
    #[test]
    fn an_answer_is_read_at_the_callers_version_and_no_other() {
        assert_eq!(
            answer::<Named>(r#"{"schema_version":1,"name":"stub"}"#, 2),
            Err(String::from(
                "answered schema_version 1; this fleet speaks 2"
            ))
        );
        assert_eq!(
            answer::<Named>("{\"schema_version\":2,\"name\":\"stub\"}\ntrailing {", 2),
            Ok(Named {
                name: String::from("stub")
            })
        );
        assert_eq!(
            answer::<Named>(r#"{"name":"stub"}"#, 2),
            Err(String::from("answered no schema_version"))
        );
    }

    /// A request is its fields with the caller's version and the root beside
    /// them.
    #[test]
    fn an_envelope_carries_the_callers_version_and_the_root() {
        let mut fields = Map::new();
        fields.insert(String::from("seat"), Value::from("orla"));
        assert_eq!(
            envelope(fields, Path::new("/work/project"), 7),
            json!({"seat": "orla", "schema_version": 7, "root": "/work/project"})
        );
    }
}
