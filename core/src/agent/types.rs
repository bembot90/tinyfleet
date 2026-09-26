//! The agent contract's domain: what an agent adapter is asked, and what it
//! answers, as fleet's own types.
//!
//! THE JSON IS THE CONTRACT. Every type here derives its serde form, and that
//! form is the one `docs/agent.md` prints and an external adapter sends: a
//! request is a verb's fields with the contract's version and the project's
//! root beside them ([`request`]), and an answer is one JSON object at that
//! version holding the verb's response body ([`answer`]). The envelope, the
//! exit table and the bound are the store contract's own, through
//! [`crate::adapter::exec`]; what differs is the version this contract states,
//! its verbs, and its refusal's reasons.
//!
//! NOTHING CALLS THESE YET. The controller's trait still speaks its own
//! shapes; it moves onto these verb by verb, and until it does only this
//! file's own tests hold them.
//!
//! IT GROWS ADDITIVELY. `schema_version` stays 1 while every field a later
//! fleet adds to a request is optional and every field it adds to an answer is
//! defaulted, so no request type here refuses a key it does not name: an
//! adapter built against a later fleet's requests still reads an earlier
//! one's, and the reverse. An answer's enumerations are the exception, and on
//! purpose — an activity, a blocked reason or a posture this fleet has no word
//! for is a reading it cannot act on, so it does not read, and [`answer`]
//! names the field that carried it.
//!
//! NO FIELD NAMES A VENDOR. A model is the agent's own string, carried and
//! never parsed; a posture is fleet's word, which each adapter maps to its
//! agent's own; a permission is a command word, which each adapter renders
//! into its agent's own format. What an adapter knows about its agent stays
//! in the adapter.

use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::seat::identity::SeatId;
use crate::store::types::Stamp;

/// The version of the agent contract this fleet speaks, carried as
/// `schema_version` on every request and demanded on every answer. The
/// store's contract states its own; the two grow apart.
pub const CONTRACT_VERSION: u64 = 1;

// ---- the posture ----------------------------------------------------------------

/// How much a seat's agent may do without asking, as fleet's own word (ruling
/// 14, 2026-09-25): `ask` asks before acting, `auto` acts on what its agent
/// judges safe, and `unattended` never stops to ask. Each adapter maps the
/// three onto its agent's own modes; no other word reads.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Posture {
    Ask,
    Auto,
    Unattended,
}

// ---- what an agent is -----------------------------------------------------------

/// What an agent adapter declares about its agent: the postures it takes, the
/// model a seat that names none launches on, the template of a session's first turn,
/// whether it answers `context`, the versions of its agent it was measured
/// against, and the models a posture is held to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Capabilities {
    /// The postures this agent takes. A launch or a resume naming another is
    /// refused `unsupported`.
    pub postures: Vec<Posture>,
    /// The model a seat whose policy names none is launched on, as the
    /// agent's own string.
    pub default_model: String,
    /// A session's first turn as a template: `{seat}` in it stands for the
    /// seat's session name, filled in per seat before the launch is sent.
    pub first_turn: String,
    /// Whether this adapter answers `context`. Absent, it does not, and fleet
    /// never asks it.
    #[serde(default)]
    pub context: bool,
    /// The versions of its agent this adapter was measured against, as the
    /// agent's `version` names them: what an installed agent is compared to.
    pub measured: Vec<String>,
    /// The D3 gate (reviewer call 2026-09-25, E9): a posture keyed here is
    /// honoured only by a model whose name starts with one of its prefixes.
    /// A posture not keyed, and a map left out or empty, is no gate at all.
    #[serde(default)]
    pub posture_models: BTreeMap<Posture, Vec<String>>,
}

impl Capabilities {
    /// The declaration, or the field that breaks it: at least one posture,
    /// at least one measured version, and a first turn that is not blank —
    /// a session started on nothing would sit at its prompt with no
    /// instruction, which is a seat that never began.
    pub fn validate(&self) -> Result<(), String> {
        if self.postures.is_empty() {
            return Err(String::from(
                "postures is empty — an agent takes at least one of ask, auto and unattended",
            ));
        }
        if self.measured.is_empty() {
            return Err(String::from(
                "measured is empty — an adapter names at least one version of its agent it was \
                 measured against",
            ));
        }
        if self.first_turn.trim().is_empty() {
            return Err(String::from(
                "first_turn is blank — a session starts on its first turn",
            ));
        }
        Ok(())
    }
}

/// Which agent the adapter drives, and at which version of itself. `version`
/// is `null` where no binary of the agent is installed: the adapter answers,
/// and the agent it would drive is not there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Version {
    pub name: String,
    pub version: Option<String>,
}

// ---- launching and resuming -----------------------------------------------------

/// The `launch` request: everything fleet knows about the session a seat
/// starts, from which the adapter answers the argv and environment the host
/// starts it with.
///
/// A launch MAY WRITE inside `config_dir` and inside the seat's `worktree`,
/// and nowhere else (reviewer call 2026-09-25, E8): the agent's scoped
/// configuration — its onboarding seed and the trust acceptance for this
/// worktree, which fleet created (ruling 13) — and whatever rendering of
/// `permissions` its agent reads from the worktree. The adapter's own plugin
/// or extension directory is its own to know and is never in the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Launch {
    /// The seat whose session this is.
    pub seat: SeatId,
    /// The absolute path of the worktree the session runs in.
    pub worktree: String,
    /// The name the session answers to.
    pub name: String,
    /// The model, as the agent's own string.
    pub model: String,
    pub posture: Posture,
    /// The session's first turn, the declared template with `{seat}` filled.
    pub first_turn: String,
    /// The configuration directory the session is scoped to. Absent for a
    /// named seat, which runs under the agent's own: the controller's session
    /// row carries none for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    /// The variables fleet sets for the seat's session.
    pub env: BTreeMap<String, String>,
    pub permissions: Permissions,
}

/// What the seat may run without asking, stated neutrally: fleet's words,
/// which each adapter renders into its own agent's format.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Permissions {
    /// The project's `[permissions] tool_commands`, each one command word —
    /// a bare name or a path relative to the repository, never carrying a
    /// space, a glob or a leading dash — as the transient spawn checks them
    /// today.
    pub commands: Vec<String>,
    /// The builder's checks: the one command line the dispatch handed in,
    /// whole. Absent where none was handed, and then no rule names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touched: Option<String>,
}

/// The `resume` request: a session the agent already has, by its full id,
/// brought back under the model and posture it was started with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Resume {
    /// The session's id, whole, as `read` answered it.
    pub session_id: String,
    pub worktree: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    pub model: String,
    pub posture: Posture,
}

/// The answer to `launch` and to `resume`: the command the host runs as the
/// session's own process, and the environment it runs with. A resume's argv
/// carries the start's flags with the full session id (reviewer call
/// 2026-09-25, E3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Argv {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

// ---- reading the seats ----------------------------------------------------------

/// One seat as `read` and `context` are asked about it: what fleet holds that
/// lets the adapter find the seat's session among its agent's.
///
/// The adapter matches a seat to its agent's own record by `session_id`, and
/// else by `pid`, the pane's process — never by the working directory, which
/// two sessions can share (CORRECTIONS AT REVIEW, 2026-09-25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeatRef {
    pub seat: SeatId,
    /// The session's id, once one has been read for the seat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The process id of the pane the seat's agent runs as, where the host
    /// reads one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    pub worktree: String,
    /// The pane's text as the host captured it, for an adapter whose agent
    /// keeps no typed state to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
}

/// The request `read` and `context` both take: every seat at once, so one
/// poll runs one process per verb and never one per seat.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Seats {
    pub seats: Vec<SeatRef>,
}

/// What a seat's agent is doing. Presence — whether the session exists and
/// its process lives — is fleet's own reading of the host, and never this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Starting,
    Busy,
    Idle,
    Blocked,
    Unknown,
}

/// What a blocked seat waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockedOn {
    Permission,
    Question,
    LoggedOut,
    UsageLimit,
}

/// Where a reading came from: the agent's own typed state, or rules the
/// adapter read the pane's screen by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Typed,
    Screen,
}

/// One seat as `read` answers it.
///
/// `blocked` without `blocked_on` is a legal reading: a stop keys on the
/// blocked activity being there, not on knowing what it waits on (B8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeatActivity {
    pub seat: SeatId,
    pub activity: Activity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on: Option<BlockedOn>,
    pub evidence: Evidence,
    /// The session's id, where the adapter found it: what the next read and
    /// a resume carry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Why, in one sentence for a person: what made the reading `unknown`,
    /// or what the seat is blocked on where no reason names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// The answer to `read`: one row per seat it was asked about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Activities {
    pub seats: Vec<SeatActivity>,
}

/// One seat as `context` answers it: how much of its window the session has
/// used, and when it last wrote. Each is absent where the adapter cannot say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SeatContext {
    pub seat: SeatId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_write: Option<Stamp>,
}

/// The answer to `context`: one row per seat it was asked about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Contexts {
    pub seats: Vec<SeatContext>,
}

// ---- refusals -------------------------------------------------------------------

/// The adapter's answer that the act cannot be done as asked, and why: exit
/// 1's `refused`, beside `schema_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Refusal {
    pub reason: RefusalReason,
    pub message: String,
}

/// Why an agent adapter refused: a posture or a model its agent will not
/// take, or a resume of a session its agent does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Unsupported,
    Missing,
}

// ---- the envelope ---------------------------------------------------------------

/// A verb's fields as one request: `schema_version` at [`CONTRACT_VERSION`]
/// and the project's `root` inserted beside them — the envelope every
/// adapter is sent ([`crate::adapter::exec::envelope`]), at this contract's
/// version, with `root` meaning exactly what the store contract's does.
pub fn request(
    verb_fields: serde_json::Map<String, serde_json::Value>,
    root: &Path,
) -> serde_json::Value {
    crate::adapter::exec::envelope(verb_fields, root, CONTRACT_VERSION)
}

/// An answer's response body, read off the FIRST JSON value of the text at
/// `schema_version` [`CONTRACT_VERSION`] exactly
/// ([`crate::adapter::exec::answer`]), and decoded naming the field that
/// failed.
///
/// NAMED BY ITS PATH, where the store's reader names only the type it
/// expected. A `read` answers one row per seat, and a row whose `activity` is
/// a word fleet has none for is one seat of many: `seats[3].activity` is the
/// sentence a person can take to the adapter, `unknown variant` is not. So the
/// envelope is read as every adapter's is, into the body as JSON, and only the
/// body's decode is this contract's own.
pub fn answer<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let body: serde_json::Value = crate::adapter::exec::answer(text, CONTRACT_VERSION)?;
    serde_path_to_error::deserialize(body)
        .map_err(|why| format!("the answer does not read — {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fmt::Debug;

    const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";
    const REVIEWER: &str = "0199a3c4-8f01-7a23-b456-c789d0e1f234";
    const SESSION: &str = "5d1c2a9e-4b3f-4e6a-9c8d-7e6f5a4b3c2d";
    const ROOT: &str = "/work/project";
    const WORKTREE: &str = "/work/lanes/builder-e5f60718";
    const CONFIG_DIR: &str = "/work/seats/builder-e5f60718/agent";
    const NAME: &str = "builder-e5f60718";

    fn seat(id: &str) -> SeatId {
        SeatId::parse(id).expect("a well-formed seat id")
    }

    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("a contract type serializes")
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    // ---- the examples docs/agent.md prints, as the types hold them ----

    fn capabilities() -> Capabilities {
        Capabilities {
            postures: vec![Posture::Ask, Posture::Auto, Posture::Unattended],
            default_model: String::from("quill-large-2"),
            first_turn: String::from("/wake {seat}"),
            context: true,
            measured: words(&["2.4.0"]),
            posture_models: BTreeMap::from([(Posture::Auto, words(&["quill-large"]))]),
        }
    }

    fn launch() -> Launch {
        Launch {
            seat: seat(SEAT),
            worktree: String::from(WORKTREE),
            name: String::from(NAME),
            model: String::from("quill-large-2"),
            posture: Posture::Auto,
            first_turn: format!("/wake {NAME}"),
            config_dir: Some(String::from(CONFIG_DIR)),
            env: vars(&[("FLEET_ACTOR", &format!("seat:{SEAT}"))]),
            permissions: Permissions {
                commands: words(&["cargo", "make"]),
                touched: Some(String::from("cargo nextest run -p fleet-core")),
            },
        }
    }

    fn launched() -> Argv {
        Argv {
            argv: words(&[
                "quill",
                "--model",
                "quill-large-2",
                "--mode",
                "auto",
                &format!("/wake {NAME}"),
            ]),
            env: vars(&[
                ("FLEET_ACTOR", &format!("seat:{SEAT}")),
                ("QUILL_HOME", CONFIG_DIR),
            ]),
        }
    }

    fn resume() -> Resume {
        Resume {
            session_id: String::from(SESSION),
            worktree: String::from(WORKTREE),
            config_dir: Some(String::from(CONFIG_DIR)),
            model: String::from("quill-large-2"),
            posture: Posture::Auto,
        }
    }

    fn resumed() -> Argv {
        Argv {
            argv: words(&[
                "quill",
                "--model",
                "quill-large-2",
                "--mode",
                "auto",
                "--resume",
                SESSION,
            ]),
            env: vars(&[("QUILL_HOME", CONFIG_DIR)]),
        }
    }

    fn seats() -> Seats {
        Seats {
            seats: vec![
                SeatRef {
                    seat: seat(SEAT),
                    session_id: Some(String::from(SESSION)),
                    pid: Some(48213),
                    config_dir: Some(String::from(CONFIG_DIR)),
                    worktree: String::from(WORKTREE),
                    screen: None,
                },
                SeatRef {
                    seat: seat(REVIEWER),
                    session_id: None,
                    pid: Some(48307),
                    config_dir: None,
                    worktree: String::from(ROOT),
                    screen: Some(String::from("Run make lint? (y/n)")),
                },
            ],
        }
    }

    fn activities() -> Activities {
        Activities {
            seats: vec![
                SeatActivity {
                    seat: seat(SEAT),
                    activity: Activity::Busy,
                    blocked_on: None,
                    evidence: Evidence::Typed,
                    session_id: Some(String::from(SESSION)),
                    cause: None,
                },
                SeatActivity {
                    seat: seat(REVIEWER),
                    activity: Activity::Blocked,
                    blocked_on: Some(BlockedOn::Permission),
                    evidence: Evidence::Screen,
                    session_id: None,
                    cause: None,
                },
            ],
        }
    }

    fn blocked_on_nothing_named() -> SeatActivity {
        SeatActivity {
            seat: seat(REVIEWER),
            activity: Activity::Blocked,
            blocked_on: None,
            evidence: Evidence::Screen,
            session_id: None,
            cause: Some(String::from("a dialog no rule names")),
        }
    }

    fn unknown() -> SeatActivity {
        SeatActivity {
            seat: seat(REVIEWER),
            activity: Activity::Unknown,
            blocked_on: None,
            evidence: Evidence::Screen,
            session_id: None,
            cause: Some(String::from("the screen matches no rule")),
        }
    }

    fn contexts() -> Contexts {
        Contexts {
            seats: vec![
                SeatContext {
                    seat: seat(SEAT),
                    tokens: Some(48210),
                    window: Some(200000),
                    turns: Some(12),
                    last_write: Stamp::parse("2026-09-23T10:00:00Z"),
                },
                SeatContext {
                    seat: seat(REVIEWER),
                    tokens: None,
                    window: None,
                    turns: None,
                    last_write: None,
                },
            ],
        }
    }

    fn unsupported() -> Refusal {
        Refusal {
            reason: RefusalReason::Unsupported,
            message: String::from("quill takes no posture unattended"),
        }
    }

    fn missing() -> Refusal {
        Refusal {
            reason: RefusalReason::Missing,
            message: format!("quill has no session {SESSION}"),
        }
    }

    const ERROR_EXAMPLE: &str = r#"{"schema_version":1,"error":"quill's session index is locked"}"#;

    /// Exit 1's answer: the refusal under `refused`.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Refused {
        refused: Refusal,
    }

    /// One JSON example the page prints, and whether it reads back through
    /// the types as the value it was written from.
    struct Example {
        text: String,
        round_trip: Result<(), String>,
    }

    fn verdict<T: PartialEq + Debug>(
        text: &str,
        read: Result<T, String>,
        written: &T,
    ) -> Result<(), String> {
        match read {
            Ok(read) if &read == written => Ok(()),
            Ok(read) => Err(format!("{text} reads back as {read:?}")),
            Err(why) => Err(format!("{text} does not read: {why}")),
        }
    }

    /// An answer as an adapter prints it: `schema_version`, then the body's
    /// fields in the order the type writes them.
    fn answered<T: Serialize + DeserializeOwned + PartialEq + Debug>(body: T) -> Example {
        let fields = json(&body);
        let text = if fields == "{}" {
            String::from(r#"{"schema_version":1}"#)
        } else {
            format!(r#"{{"schema_version":1,{}"#, &fields[1..])
        };
        let round_trip = verdict(&text, answer::<T>(&text), &body);
        Example { text, round_trip }
    }

    /// A request as fleet sends it: the verb's fields inside the envelope,
    /// printed whole, read back through the verb's request type — which
    /// reads past the envelope's two keys as it reads past any it does not
    /// name.
    fn sent<T: Serialize + DeserializeOwned + PartialEq + Debug>(
        fields: T,
        pretty: bool,
    ) -> Example {
        let serde_json::Value::Object(map) = serde_json::to_value(&fields).unwrap() else {
            panic!("a request's fields are an object");
        };
        let whole = request(map, Path::new(ROOT));
        let text = if pretty {
            serde_json::to_string_pretty(&whole).unwrap()
        } else {
            whole.to_string()
        };
        let read = serde_json::from_str::<T>(&text).map_err(|why| why.to_string());
        let round_trip = verdict(&text, read, &fields);
        Example { text, round_trip }
    }

    /// A type on its own, as the page's Types section prints it.
    fn bare<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: T) -> Example {
        let text = json(&value);
        let read = serde_json::from_str::<T>(&text).map_err(|why| why.to_string());
        let round_trip = verdict(&text, read, &value);
        Example { text, round_trip }
    }

    /// Exit 1's answer, as the page prints it.
    fn refused(refusal: Refusal) -> Example {
        let text = format!(r#"{{"schema_version":1,"refused":{}}}"#, json(&refusal));
        let read = answer::<Refused>(&text).map(|read| read.refused);
        let round_trip = verdict(&text, read, &refusal);
        Example { text, round_trip }
    }

    fn documented_examples() -> Vec<Example> {
        vec![
            bare(Posture::Ask),
            bare(Posture::Auto),
            bare(Posture::Unattended),
            answered(capabilities()),
            answered(Version {
                name: String::from("quill"),
                version: Some(String::from("2.4.0")),
            }),
            answered(Version {
                name: String::from("quill"),
                version: None,
            }),
            sent(launch(), true),
            answered(launched()),
            sent(resume(), false),
            answered(resumed()),
            sent(seats(), true),
            answered(activities()),
            bare(blocked_on_nothing_named()),
            bare(unknown()),
            answered(contexts()),
            refused(unsupported()),
            refused(missing()),
        ]
    }

    // ---- 1: the page ----

    /// docs/agent.md prints the contract's JSON, and this arm holds every
    /// example there to the types: each as the types write it, which the arm
    /// below reads back through them as the value it was written from.
    ///
    /// RED-PROOF: one byte of one example changed in the page fails this arm,
    /// naming the example it no longer finds.
    #[test]
    fn the_agent_doc_prints_every_example_as_the_types_write_it() {
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/agent.md"))
            .expect("docs/agent.md reads");
        let mut texts: Vec<String> = documented_examples()
            .into_iter()
            .map(|example| example.text)
            .collect();
        texts.push(String::from(ERROR_EXAMPLE));

        let missing: Vec<&String> = texts
            .iter()
            .filter(|text| !doc.contains(text.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "docs/agent.md does not print these examples verbatim: {missing:#?}"
        );
    }

    #[test]
    fn every_documented_example_round_trips_through_the_types() {
        let failing: Vec<String> = documented_examples()
            .into_iter()
            .filter_map(|example| example.round_trip.err())
            .collect();
        assert!(failing.is_empty(), "{failing:#?}");

        // THE ERROR: exit 3's answer names what went wrong in `error`, read
        // off the first value as every adapter's is.
        let error = crate::adapter::exec::first_value(ERROR_EXAMPLE).unwrap();
        assert_eq!(error["error"], "quill's session index is locked");
    }

    #[test]
    fn the_documented_capabilities_keep_the_rules_the_page_states() {
        assert_eq!(capabilities().validate(), Ok(()));
    }

    // ---- 2: the words ----

    #[test]
    fn a_posture_is_one_of_three_words_and_no_other() {
        for (text, posture) in [
            ("ask", Posture::Ask),
            ("auto", Posture::Auto),
            ("unattended", Posture::Unattended),
        ] {
            assert_eq!(json(&posture), format!("\"{text}\""));
            assert_eq!(
                serde_json::from_str::<Posture>(&json(&posture)).unwrap(),
                posture
            );
        }
        // An agent's own word for a mode is the adapter's to map to, and
        // never fleet's to carry.
        for other in [
            "default",
            "dontAsk",
            "acceptEdits",
            "bypassPermissions",
            "plan",
        ] {
            assert!(
                serde_json::from_str::<Posture>(&format!("\"{other}\"")).is_err(),
                "{other} is not a posture"
            );
        }
    }

    #[test]
    fn the_readings_are_their_words() {
        for (activity, text) in [
            (Activity::Starting, "starting"),
            (Activity::Busy, "busy"),
            (Activity::Idle, "idle"),
            (Activity::Blocked, "blocked"),
            (Activity::Unknown, "unknown"),
        ] {
            assert_eq!(json(&activity), format!("\"{text}\""));
        }
        for (on, text) in [
            (BlockedOn::Permission, "permission"),
            (BlockedOn::Question, "question"),
            (BlockedOn::LoggedOut, "logged_out"),
            (BlockedOn::UsageLimit, "usage_limit"),
        ] {
            assert_eq!(json(&on), format!("\"{text}\""));
        }
        assert_eq!(json(&Evidence::Typed), "\"typed\"");
        assert_eq!(json(&Evidence::Screen), "\"screen\"");
        assert_eq!(json(&RefusalReason::Unsupported), "\"unsupported\"");
        assert_eq!(json(&RefusalReason::Missing), "\"missing\"");
    }

    /// An activity fleet has no word for is a reading it cannot act on, and
    /// the refusal says which field of which row carried it, so a person
    /// reading it knows where the adapter and fleet part.
    #[test]
    fn an_activity_with_no_word_does_not_read_and_names_its_field() {
        let text = format!(
            r#"{{"schema_version":1,"seats":[{{"seat":"{SEAT}","activity":"napping","evidence":"typed"}}]}}"#
        );
        let refused = answer::<Activities>(&text).unwrap_err();
        assert!(refused.contains("seats[0].activity"), "{refused}");
        assert!(refused.contains("napping"), "{refused}");

        // THE CONTROL: the same row at a word fleet has reads.
        let text = text.replace("napping", "idle");
        let read = answer::<Activities>(&text).unwrap();
        assert_eq!(read.seats[0].activity, Activity::Idle);
    }

    #[test]
    fn blocked_without_what_it_waits_on_is_a_reading() {
        let text = format!(r#"{{"seat":"{SEAT}","activity":"blocked","evidence":"screen"}}"#);
        let read: SeatActivity = serde_json::from_str(&text).unwrap();
        assert_eq!(read.activity, Activity::Blocked);
        assert_eq!(read.blocked_on, None);
        assert_eq!(json(&read), text);
    }

    // ---- 3: capabilities ----

    #[test]
    fn capabilities_with_no_postures_fail_validate_naming_the_field() {
        let none = Capabilities {
            postures: Vec::new(),
            ..capabilities()
        };
        let refused = none.validate().unwrap_err();
        assert!(refused.starts_with("postures"), "{refused}");
    }

    #[test]
    fn capabilities_measured_against_nothing_fail_validate() {
        let none = Capabilities {
            measured: Vec::new(),
            ..capabilities()
        };
        let refused = none.validate().unwrap_err();
        assert!(refused.starts_with("measured"), "{refused}");
    }

    #[test]
    fn capabilities_whose_first_turn_is_blank_fail_validate() {
        for blank in ["", "  \n"] {
            let none = Capabilities {
                first_turn: String::from(blank),
                ..capabilities()
            };
            let refused = none.validate().unwrap_err();
            assert!(refused.starts_with("first_turn"), "{refused}");
        }
    }

    #[test]
    fn capabilities_leaving_out_context_and_the_gate_read_as_neither() {
        let text = r#"{"postures":["ask"],"default_model":"quill-large-2","first_turn":"/wake {seat}","measured":["2.4.0"]}"#;
        let read: Capabilities = serde_json::from_str(text).unwrap();
        assert!(!read.context);
        assert!(read.posture_models.is_empty());
        assert_eq!(read.validate(), Ok(()));
    }

    #[test]
    fn capabilities_leaving_out_the_postures_do_not_read() {
        let text = r#"{"schema_version":1,"default_model":"quill-large-2","first_turn":"/wake {seat}","measured":["2.4.0"]}"#;
        let refused = answer::<Capabilities>(text).unwrap_err();
        assert!(refused.contains("postures"), "{refused}");
    }

    #[test]
    fn a_version_of_null_is_an_agent_not_installed() {
        let read: Version =
            answer(r#"{"schema_version":1,"name":"quill","version":null}"#).unwrap();
        assert_eq!(read.version, None);
    }

    // ---- 4: requests stay open ----

    #[test]
    fn a_request_with_a_key_it_does_not_name_reads() {
        let later = |example: String| -> String {
            let mut value: serde_json::Value = serde_json::from_str(&example).unwrap();
            value["later"] = serde_json::json!({"added": true});
            value.to_string()
        };
        let text = later(sent(launch(), false).text);
        assert_eq!(serde_json::from_str::<Launch>(&text).unwrap(), launch());
        let text = later(sent(resume(), false).text);
        assert_eq!(serde_json::from_str::<Resume>(&text).unwrap(), resume());
        let text = later(sent(seats(), false).text);
        assert_eq!(serde_json::from_str::<Seats>(&text).unwrap(), seats());

        // A key a later fleet adds inside a seat, or inside permissions,
        // reads the same way.
        let text =
            format!(r#"{{"seats":[{{"seat":"{SEAT}","worktree":"{WORKTREE}","later":1}}]}}"#);
        assert_eq!(serde_json::from_str::<Seats>(&text).unwrap().seats.len(), 1);
        let text = r#"{"commands":["cargo"],"later":1}"#;
        assert_eq!(
            serde_json::from_str::<Permissions>(text).unwrap().commands,
            words(&["cargo"])
        );
    }

    #[test]
    fn a_launch_for_a_named_seat_carries_no_config_dir_and_reads() {
        let named = Launch {
            config_dir: None,
            permissions: Permissions {
                commands: Vec::new(),
                touched: None,
            },
            ..launch()
        };
        let text = sent(named.clone(), false).text;
        assert!(!text.contains("config_dir"), "{text}");
        assert!(!text.contains("touched"), "{text}");
        assert_eq!(serde_json::from_str::<Launch>(&text).unwrap(), named);
    }

    #[test]
    fn a_seat_asked_about_needs_only_its_id_and_worktree() {
        let text = format!(r#"{{"seats":[{{"seat":"{SEAT}","worktree":"{WORKTREE}"}}]}}"#);
        let read: Seats = serde_json::from_str(&text).unwrap();
        assert_eq!(read.seats[0].session_id, None);
        assert_eq!(read.seats[0].pid, None);
        assert_eq!(json(&read), text);
    }

    /// An answer grows the way a request does: a key a later adapter adds is
    /// read past by a fleet that does not know it.
    #[test]
    fn an_answer_with_a_key_it_does_not_name_reads() {
        let text = r#"{"schema_version":1,"name":"quill","version":"2.4.0","build":"a1b2"}"#;
        let read: Version = answer(text).unwrap();
        assert_eq!(read.version.as_deref(), Some("2.4.0"));
    }

    // ---- 5: context ----

    #[test]
    fn a_seat_s_context_is_each_reading_the_adapter_can_give() {
        let text = format!(r#"{{"seat":"{SEAT}"}}"#);
        let read: SeatContext = serde_json::from_str(&text).unwrap();
        assert_eq!(read.tokens, None);
        assert_eq!(read.last_write, None);

        let text = format!(r#"{{"seat":"{SEAT}","last_write":"yesterday"}}"#);
        assert!(serde_json::from_str::<SeatContext>(&text).is_err());
    }

    // ---- 6: the envelope ----

    #[test]
    fn a_request_carries_this_contract_s_version_and_the_root() {
        let sent = request(serde_json::Map::new(), Path::new(ROOT));
        assert_eq!(
            sent,
            serde_json::json!({"schema_version": 1, "root": "/work/project"})
        );
    }

    #[test]
    fn an_answer_at_another_version_or_none_does_not_read() {
        assert_eq!(
            answer::<Version>(r#"{"schema_version":2,"name":"quill","version":"2.4.0"}"#),
            Err(String::from(
                "answered schema_version 2; this fleet speaks 1"
            ))
        );
        assert_eq!(
            answer::<Version>(r#"{"name":"quill","version":"2.4.0"}"#),
            Err(String::from("answered no schema_version"))
        );
    }
}
