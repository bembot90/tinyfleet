//! The agent contract as one JSON Schema document, generated from the types
//! its JSON is: what `fleet agent schema` prints, and what `agent.schema.json`
//! beside this file holds, byte for byte.
//!
//! THE STORE'S SHAPE. `verbs` holds each of the six verbs' `request` and
//! `response`, `refusal` and `error` hold the answers of exits 1 and 3, and
//! every `$ref` points into the document's own `$defs`, built by the same
//! helpers as the store's document ([`crate::schema`]) so the two an adapter
//! author reads are one shape.
//!
//! A REQUEST IS WHAT FLEET SENDS: the envelope's `schema_version` and `root`
//! ([`super::types::request`]) and the verb's fields. It names no
//! `additionalProperties`, because no request type refuses a key it does not
//! name: an addition to a request is optional within schema version 1. A
//! RESPONSE IS THE BODY fleet reads ([`super::types::answer`]), with
//! `schema_version` beside its fields.
//!
//! THE WORDS ARE CLOSED. A posture, an activity, a blocked reason, an
//! evidence and a refusal's reason are each an `enum` of the words fleet
//! reads, so the types an adapter author generates name them.
//!
//! NO PROSE FROM THE TYPES, as the store's document rules: an adapter
//! author's prose is docs/agent.md, and every generated schema is stripped of
//! the doc comments the derive carries.

use schemars::generate::SchemaSettings;
use serde_json::{json, Map, Value};

use super::types::{
    Activities, Argv, Capabilities, Contexts, Launch, Refusal, Resume, Seats, Version,
    CONTRACT_VERSION,
};
use crate::schema::{self, fields, generated, refer, strip};

/// The document: every verb's request and response, the refusal, the error,
/// and the definitions they share.
pub fn document() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let g = &mut generator;

    let text = json!({"type": "string"});
    let none = Map::new;
    let argv = generated::<Argv>(g);
    let seats = generated::<Seats>(g);

    // Each verb: the fields of its request beyond the envelope, and the body
    // of its answer beyond `schema_version`.
    let each = [
        ("capabilities", none(), generated::<Capabilities>(g)),
        ("version", none(), generated::<Version>(g)),
        ("launch", generated::<Launch>(g), argv.clone()),
        ("resume", generated::<Resume>(g), argv),
        ("read", seats.clone(), generated::<Activities>(g)),
        ("context", seats, generated::<Contexts>(g)),
    ];

    let version = json!({"const": CONTRACT_VERSION});
    let mut verbs = Map::new();
    for (verb, request, response) in each {
        let request = fields(
            request,
            [("schema_version", version.clone()), ("root", text.clone())],
        );
        let response = fields(response, [("schema_version", version.clone())]);
        verbs.insert(
            String::from(verb),
            json!({"request": request, "response": response}),
        );
    }
    let refusal = fields(
        none(),
        [
            ("schema_version", version.clone()),
            ("refused", refer::<Refusal>(g)),
        ],
    );
    let error = fields(none(), [("schema_version", version), ("error", text)]);

    let mut definitions = g.take_definitions(true);
    definitions.values_mut().for_each(strip);
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "schema_version": CONTRACT_VERSION,
        "verbs": verbs,
        "refusal": refusal,
        "error": error,
        "$defs": definitions,
    })
}

/// The document as the committed file spells it: two-space indented, keys in
/// order, one newline at the end.
pub fn text() -> String {
    schema::text(&document())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{check, unknown_keywords};

    /// The document as it is committed, and as `fleet agent schema` prints it.
    const COMMITTED: &str = include_str!("agent.schema.json");

    /// The page every example below is read from.
    const AGENT_DOC: &str = include_str!("../../../docs/agent.md");

    const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";

    /// Where each JSON example docs/agent.md prints is checked, in the order
    /// the page prints them: one row per example.
    const EXAMPLES: [&str; 18] = [
        "/verbs/resume/request",
        "/verbs/resume/response",
        "/refusal",
        "/refusal",
        "/error",
        "/$defs/Posture",
        "/$defs/Posture",
        "/$defs/Posture",
        "/verbs/capabilities/response",
        "/verbs/version/response",
        "/verbs/version/response",
        "/$defs/SeatActivity",
        "/$defs/SeatActivity",
        "/verbs/launch/request",
        "/verbs/launch/response",
        "/verbs/read/request",
        "/verbs/read/response",
        "/verbs/context/response",
    ];

    /// Every JSON value of every ```json block on the page, in order: a block
    /// holds one value, or one per line.
    fn documented() -> Vec<Value> {
        AGENT_DOC
            .split("```json\n")
            .skip(1)
            .flat_map(|block| {
                let block = block.split("```").next().unwrap_or_default();
                serde_json::Deserializer::from_str(block)
                    .into_iter::<Value>()
                    .map(|value| value.expect("each example on the page is JSON"))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn passes(pointer: &str, instance: &Value) {
        let document = document();
        if let Err(why) = check(&document, pointer, instance) {
            panic!("{instance} does not pass {pointer}: {why}");
        }
    }

    fn refused(pointer: &str, instance: &Value, naming: &str) {
        let document = document();
        match check(&document, pointer, instance) {
            Ok(()) => panic!("{instance} passes {pointer}, and the types refuse it"),
            Err(why) => assert!(why.contains(naming), "{instance} at {pointer}: {why}"),
        }
    }

    /// RED-PROOF: a field added to a contract type without the file
    /// regenerated fails here, naming the command that regenerates it.
    #[test]
    fn the_committed_document_is_the_one_the_types_generate() {
        let generated = text();
        let line = generated
            .lines()
            .zip(COMMITTED.lines())
            .take_while(|(made, kept)| made == kept)
            .count()
            + 1;
        assert!(
            generated == COMMITTED,
            "core/src/agent/agent.schema.json is not the document the types generate (from line \
             {line}) — run `cargo run -p fleet-cli -- agent schema > \
             core/src/agent/agent.schema.json`"
        );
    }

    #[test]
    fn every_keyword_the_document_uses_is_one_the_check_knows() {
        let unknown = unknown_keywords(&document());
        assert!(
            unknown.is_empty(),
            "keywords the check does not know: {unknown:#?}"
        );
    }

    #[test]
    fn the_verbs_are_the_contract_s_six() {
        let document = document();
        let verbs: Vec<&String> = document["verbs"]
            .as_object()
            .expect("verbs is an object")
            .keys()
            .collect();
        assert_eq!(
            verbs,
            [
                "capabilities",
                "context",
                "launch",
                "read",
                "resume",
                "version"
            ]
        );
        assert_eq!(document["schema_version"], json!(CONTRACT_VERSION));
        assert_eq!(
            document["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }

    /// Each word an answer carries is one of a closed list, so an adapter
    /// author's generated types name the words fleet reads.
    #[test]
    fn each_word_is_a_closed_list() {
        let document = document();
        for (name, words) in [
            ("Posture", json!(["ask", "auto", "unattended"])),
            (
                "Activity",
                json!(["starting", "busy", "idle", "blocked", "unknown"]),
            ),
            (
                "BlockedOn",
                json!(["permission", "question", "logged_out", "usage_limit"]),
            ),
            ("Evidence", json!(["typed", "screen"])),
            ("RefusalReason", json!(["unsupported", "missing"])),
        ] {
            assert_eq!(document["$defs"][name]["enum"], words, "{name}");
        }
    }

    /// The definitions are the agent contract's types, each under its own
    /// name. The agent's `SeatRef` is not the seat identity's type of that
    /// name, which the generator never reaches: were two types of one name
    /// ever reached, one would be named with a number after it, and this
    /// list would say so.
    #[test]
    fn the_definitions_are_the_contract_s_types_by_their_own_names() {
        let document = document();
        let names: Vec<&String> = document["$defs"]
            .as_object()
            .expect("$defs is an object")
            .keys()
            .collect();
        assert_eq!(
            names,
            [
                "Activity",
                "BlockedOn",
                "Evidence",
                "Permissions",
                "Posture",
                "Refusal",
                "RefusalReason",
                "SeatActivity",
                "SeatContext",
                "SeatId",
                "SeatRef",
                "Stamp",
            ]
        );
        assert!(document["$defs"]["SeatRef"]["properties"]["worktree"].is_object());
    }

    /// Every JSON example docs/agent.md prints passes the schema of what it
    /// shows: a request, an answer, a refusal, the error, or one type.
    ///
    /// RED-PROOF: an example added to the page without its row in
    /// [`EXAMPLES`] fails here on the count, and one that its schema refuses
    /// fails naming the example and the place it breaks.
    #[test]
    fn every_example_the_agent_doc_prints_passes_its_schema() {
        let examples = documented();
        assert_eq!(
            examples.len(),
            EXAMPLES.len(),
            "docs/agent.md prints {} JSON examples and EXAMPLES checks {}",
            examples.len(),
            EXAMPLES.len()
        );
        let document = document();
        let failing: Vec<String> = examples
            .iter()
            .zip(EXAMPLES)
            .enumerate()
            .filter_map(|(n, (example, pointer))| {
                check(&document, pointer, example)
                    .err()
                    .map(|why| format!("example {} at {pointer}: {example}: {why}", n + 1))
            })
            .collect();
        assert!(failing.is_empty(), "{failing:#?}");
    }

    /// A request reads past a key it does not name, and an answer is held to
    /// the words and the version fleet reads.
    #[test]
    fn a_request_stays_open_and_an_answer_s_words_are_held() {
        let seats = json!({
            "schema_version": 1,
            "root": "/work/project",
            "seats": [{"seat": SEAT, "worktree": "/work/lanes/b", "later": 1}],
            "later": {"added": true},
        });
        passes("/verbs/read/request", &seats);
        passes("/verbs/context/request", &seats);
        passes(
            "/verbs/version/request",
            &json!({"schema_version": 1, "root": "/work/project", "later": 1}),
        );

        // THE CONTROLS: each one key off what the types read, and refused.
        refused(
            "/verbs/read/response",
            &json!({"schema_version": 1, "seats": [
                {"seat": SEAT, "activity": "napping", "evidence": "typed"}
            ]}),
            "none of",
        );
        refused(
            "/verbs/read/response",
            &json!({"schema_version": 1, "seats": [
                {"seat": SEAT, "activity": "blocked", "blocked_on": "lunch", "evidence": "screen"}
            ]}),
            "none of",
        );
        refused(
            "/verbs/capabilities/response",
            &json!({"schema_version": 1, "postures": ["dontAsk"], "default_model": "m",
                    "first_turn": "/wake {seat}", "measured": ["2.4.0"]}),
            "none of",
        );
        refused(
            "/verbs/capabilities/response",
            &json!({"schema_version": 1, "default_model": "m",
                    "first_turn": "/wake {seat}", "measured": ["2.4.0"]}),
            "`postures` is required",
        );
        // A posture the gate keys is one of the three, in the schema as in
        // the types.
        let gated = json!({"postures": ["ask"], "default_model": "m",
                           "first_turn": "/wake {seat}", "measured": ["2.4.0"],
                           "posture_models": {"plan": ["m"]}});
        assert!(serde_json::from_value::<Capabilities>(gated.clone()).is_err());
        let mut answered = gated;
        answered["schema_version"] = json!(1);
        refused(
            "/verbs/capabilities/response",
            &answered,
            "`plan` is not a named key",
        );
        refused(
            "/verbs/context/response",
            &json!({"schema_version": 1, "seats": [{"seat": SEAT, "last_write": "yesterday"}]}),
            "does not match",
        );
        refused(
            "/verbs/launch/request",
            &json!({"schema_version": 1, "root": "/work/project"}),
            "is required",
        );
        refused(
            "/verbs/version/response",
            &json!({"schema_version": 2, "name": "quill", "version": "2.4.0"}),
            "is not 1",
        );
        refused(
            "/refusal",
            &json!({"schema_version": 1, "refused": {"reason": "gone", "message": "m"}}),
            "none of",
        );
    }
}
