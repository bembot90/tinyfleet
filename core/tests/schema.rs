//! What a seat hands in: the delivery, the question and the findings as JSON,
//! each read against the binary's own type and the schema its brief shows.
//!
//! The schemas are read off the embedded defaults — the bytes the binary
//! carries, and not a copy retyped here — so a schema that drifts from its type
//! reds the agreement arm, and every example a schema carries is read the way
//! a verb will read a seat's file: written to disk and handed to `read`.

mod common;

use common::Fixture;
use fleet_core::entry::{Body, Finding, HoldReason};
use fleet_core::input::{
    agrees, read, Checked, DeliveryInput, FindingsInput, QuestionInput, Standing, DELIVERY_SCHEMA,
    FINDINGS_SCHEMA, QUESTION_SCHEMA,
};
use fleet_core::item::{Stop, USAGE};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const C1: &str = "1111111111111111111111111111111111111111";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

/// A schema as the binary carries it.
fn schema(path: &str) -> &'static str {
    std::str::from_utf8(
        fleet_core::embedded::bytes(path).unwrap_or_else(|| panic!("the defaults carry `{path}`")),
    )
    .expect("a schema is UTF-8")
}

/// The one example a schema carries.
fn example(path: &str) -> Value {
    let schema: Value = serde_json::from_str(schema(path)).expect("the schema is JSON");
    schema["examples"][0].clone()
}

/// `text` written to a file of its own and read as `T` would be by a verb.
fn hand_in<T: DeserializeOwned + Checked>(
    label: &str,
    what: &str,
    schema: &str,
    text: &str,
) -> (Result<T, Stop>, String) {
    let fixture = Fixture::new(&format!("schema-{label}"));
    fixture.file("handed-in.json", text);
    let path = fixture.root.join("handed-in.json");
    (read::<T>(&path, what, schema), path.display().to_string())
}

/// The refusal `text` earns, which must be a usage stop naming the file.
fn refused<T: DeserializeOwned + Checked + std::fmt::Debug>(
    label: &str,
    what: &str,
    schema: &str,
    text: &str,
) -> String {
    let (read, path) = hand_in::<T>(label, what, schema, text);
    let stop = read.expect_err("the file is refused");
    assert_eq!(
        stop.code, USAGE,
        "a file the grammar does not read is exit 2: {stop}"
    );
    assert!(
        stop.message.starts_with(&format!("the {what} at {path} ")),
        "the refusal names what was handed in and where: {stop}"
    );
    stop.message
}

/// The example with `edit` applied, as text.
fn edited(path: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut example = example(path);
    edit(&mut example);
    example.to_string()
}

// ---- AC1: each schema agrees with its type --------------------------------------

#[test]
fn the_delivery_schema_agrees_with_its_type() {
    agrees::<DeliveryInput>(schema(DELIVERY_SCHEMA)).expect("the delivery schema agrees");
}

#[test]
fn the_question_schema_agrees_with_its_type() {
    agrees::<QuestionInput>(schema(QUESTION_SCHEMA)).expect("the question schema agrees");
}

#[test]
fn the_findings_schema_agrees_with_its_type() {
    agrees::<FindingsInput>(schema(FINDINGS_SCHEMA)).expect("the findings schema agrees");
}

/// THE RED-PROOF, kept standing: a type carrying one field more than its
/// schema names is a disagreement, and the answer names the key by its path.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingsAndMore {
    findings: Vec<Finding>,
    #[serde(default)]
    severity: String,
}

#[test]
fn a_type_carrying_a_field_its_schema_does_not_name_disagrees() {
    let why = agrees::<FindingsAndMore>(schema(FINDINGS_SCHEMA))
        .expect_err("the schema does not name `severity`");
    assert!(
        why.contains("$.examples[0].severity"),
        "the disagreement is named by its path: {why}"
    );
}

/// The type the schema's example does not read as at all is the first
/// disagreement, before any key is compared.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingsNeedingMore {
    findings: Vec<Finding>,
    severity: String,
}

#[test]
fn a_type_the_example_does_not_read_as_disagrees() {
    let why = agrees::<FindingsNeedingMore>(schema(FINDINGS_SCHEMA))
        .expect_err("the example carries no `severity`");
    assert!(why.contains("$.examples[0]"), "{why}");
    assert!(why.contains("severity"), "{why}");
}

#[test]
fn a_schema_that_is_not_json_disagrees() {
    let why = agrees::<FindingsInput>("{ not json").expect_err("the text does not parse");
    assert!(why.contains("not JSON"), "{why}");
}

/// (c): a key the schema requires that the type reads without is a
/// disagreement, and so is the other way round.
#[test]
fn a_required_list_that_disagrees_with_the_type_is_named() {
    let mut loosened: Value = serde_json::from_str(schema(DELIVERY_SCHEMA)).expect("JSON");
    loosened["required"]
        .as_array_mut()
        .expect("the delivery schema requires its keys")
        .retain(|key| key != "covers");
    let why =
        agrees::<DeliveryInput>(&loosened.to_string()).expect_err("covers is required by the type");
    assert!(why.contains("$.examples[0].covers"), "{why}");

    let mut tightened: Value = serde_json::from_str(schema(QUESTION_SCHEMA)).expect("JSON");
    tightened["required"]
        .as_array_mut()
        .expect("the question schema requires its keys")
        .push(Value::from("context"));
    let why = agrees::<QuestionInput>(&tightened.to_string())
        .expect_err("the type reads a question without context");
    assert!(why.contains("$.examples[0].context"), "{why}");
}

/// (c) reaches nested objects too: `about.commit` is optional, and a schema
/// that requires it disagrees.
#[test]
fn a_nested_required_list_that_disagrees_with_the_type_is_named() {
    let mut tightened: Value = serde_json::from_str(schema(QUESTION_SCHEMA)).expect("JSON");
    tightened["properties"]["about"]["required"]
        .as_array_mut()
        .expect("about requires its keys")
        .push(Value::from("commit"));
    let why = agrees::<QuestionInput>(&tightened.to_string())
        .expect_err("the type reads an about without a commit");
    assert!(why.contains("$.examples[0].about.commit"), "{why}");
}

/// (d): an object schema anywhere in the tree left open is a disagreement,
/// named by the schema's own path.
#[test]
fn an_object_schema_left_open_is_named_by_its_path() {
    let mut opened: Value = serde_json::from_str(schema(DELIVERY_SCHEMA)).expect("JSON");
    opened["properties"]["suite"]["oneOf"][1]
        .as_object_mut()
        .expect("the not-tested branch is an object schema")
        .remove("additionalProperties");
    let why = agrees::<DeliveryInput>(&opened.to_string()).expect_err("the branch is open");
    assert!(why.contains("$.properties.suite.oneOf[1]"), "{why}");
}

// ---- AC2: each example reads, and each refusal is exit 2 ------------------------

#[test]
fn each_schemas_example_reads_as_its_type() {
    let (delivery, _) = hand_in::<DeliveryInput>(
        "delivery",
        "delivery",
        DELIVERY_SCHEMA,
        &example(DELIVERY_SCHEMA).to_string(),
    );
    let delivery = delivery.expect("the delivery example reads");
    assert!(!delivery.files.is_empty());

    let (question, _) = hand_in::<QuestionInput>(
        "question",
        "question",
        QUESTION_SCHEMA,
        &example(QUESTION_SCHEMA).to_string(),
    );
    let question = question.expect("the question example reads");
    assert!(
        question.context.is_some() && question.about.is_some(),
        "the example carries every optional property"
    );

    let (findings, _) = hand_in::<FindingsInput>(
        "findings",
        "findings",
        FINDINGS_SCHEMA,
        &example(FINDINGS_SCHEMA).to_string(),
    );
    assert!(!findings
        .expect("the findings example reads")
        .findings
        .is_empty());
}

#[test]
fn an_unknown_key_is_refused_naming_the_schema() {
    let text = edited(DELIVERY_SCHEMA, |example| {
        example["colour"] = Value::from("blue");
    });
    let message = refused::<DeliveryInput>("unknown", "delivery", DELIVERY_SCHEMA, &text);
    assert!(
        message.contains(&format!("is not the shape {DELIVERY_SCHEMA} gives")),
        "{message}"
    );
    assert!(message.contains("colour"), "the key is named: {message}");
    assert!(
        message.ends_with("— the brief shows that schema"),
        "{message}"
    );
}

#[test]
fn an_unknown_nested_key_is_refused_naming_its_path() {
    let text = edited(DELIVERY_SCHEMA, |example| {
        example["checks"][0]["colour"] = Value::from("blue");
    });
    let message = refused::<DeliveryInput>("nested", "delivery", DELIVERY_SCHEMA, &text);
    assert!(message.contains(DELIVERY_SCHEMA), "{message}");
    assert!(
        message.contains("checks[0]"),
        "the path is named: {message}"
    );
}

#[test]
fn a_missing_required_key_is_refused_naming_the_schema() {
    let text = edited(DELIVERY_SCHEMA, |example| {
        example
            .as_object_mut()
            .expect("an object")
            .remove("not_proven");
    });
    let message = refused::<DeliveryInput>("missing", "delivery", DELIVERY_SCHEMA, &text);
    assert!(
        message.contains(&format!("is not the shape {DELIVERY_SCHEMA} gives")),
        "{message}"
    );
    assert!(
        message.contains("not_proven"),
        "the key is named: {message}"
    );
}

#[test]
fn a_file_that_is_not_json_is_refused_naming_the_schema() {
    let message = refused::<FindingsInput>(
        "not-json",
        "findings",
        FINDINGS_SCHEMA,
        "FINDINGS\n1. the arm is missing\n",
    );
    assert!(
        message.contains(&format!("is not the shape {FINDINGS_SCHEMA} gives")),
        "{message}"
    );
}

#[test]
fn a_file_that_is_not_there_is_refused_as_unreadable() {
    let fixture = Fixture::new("schema-absent");
    let path = fixture.root.join("nowhere.json");
    let stop =
        read::<DeliveryInput>(&path, "delivery", DELIVERY_SCHEMA).expect_err("there is no file");
    assert_eq!(stop.code, USAGE);
    assert!(
        stop.message.starts_with(&format!(
            "the delivery at {} could not be read: ",
            path.display()
        )),
        "{stop}"
    );
}

#[test]
fn a_delivery_naming_no_file_is_refused() {
    let text = edited(DELIVERY_SCHEMA, |example| {
        example["files"] = Value::Array(Vec::new());
    });
    let message = refused::<DeliveryInput>("no-files", "delivery", DELIVERY_SCHEMA, &text);
    assert!(message.ends_with(" names no file"), "{message}");
    assert!(
        message.contains(&format!(" (read against {DELIVERY_SCHEMA}) ")),
        "a rule's refusal names the schema too: {message}"
    );
}

#[test]
fn a_delivery_naming_nothing_not_proven_is_refused() {
    let text = edited(DELIVERY_SCHEMA, |example| {
        example["not_proven"] = Value::Array(Vec::new());
    });
    let message = refused::<DeliveryInput>("proven", "delivery", DELIVERY_SCHEMA, &text);
    assert!(
        message.ends_with(" names nothing not proven — it is never empty"),
        "{message}"
    );
}

#[test]
fn a_delivery_carrying_an_empty_string_is_refused_naming_the_field() {
    let text = edited(DELIVERY_SCHEMA, |example| {
        example["decisions"][0]["because"] = Value::from("");
    });
    let message = refused::<DeliveryInput>("empty", "delivery", DELIVERY_SCHEMA, &text);
    assert!(
        message.ends_with(" carries an empty `decisions[0].because`"),
        "{message}"
    );
}

#[test]
fn a_two_line_question_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["question"] = Value::from("Which store?\nThe real one or the fake?");
    });
    let message = refused::<QuestionInput>("two-lines", "question", QUESTION_SCHEMA, &text);
    assert!(
        message.ends_with(" asks its question over more than one line — context carries the rest"),
        "{message}"
    );
}

#[test]
fn a_question_naming_no_option_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["options"] = Value::Array(Vec::new());
        example.as_object_mut().expect("an object").remove("about");
    });
    let message = refused::<QuestionInput>("no-options", "question", QUESTION_SCHEMA, &text);
    assert!(
        message.ends_with(" names no option — a question with none is a conversation"),
        "{message}"
    );
}

#[test]
fn a_question_naming_one_letter_twice_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["options"][1]["letter"] = example["options"][0]["letter"].clone();
    });
    let message = refused::<QuestionInput>("twice", "question", QUESTION_SCHEMA, &text);
    assert!(message.ends_with(" names option A twice"), "{message}");
}

#[test]
fn a_question_whose_letter_is_not_a_capital_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["options"][0]["letter"] = Value::from("a");
    });
    let message = refused::<QuestionInput>("lower", "question", QUESTION_SCHEMA, &text);
    assert!(message.contains("`options[0].letter`"), "{message}");
}

#[test]
fn a_question_licensing_a_letter_no_option_carries_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["about"]["licenses"] = Value::from("Z");
    });
    let message = refused::<QuestionInput>("licenses", "question", QUESTION_SCHEMA, &text);
    assert!(message.contains("`about.licenses`"), "{message}");
}

#[test]
fn a_question_about_no_item_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["about"]["items"] = Value::Array(Vec::new());
    });
    let message = refused::<QuestionInput>("no-items", "question", QUESTION_SCHEMA, &text);
    assert!(message.contains("`about.items`"), "{message}");
}

#[test]
fn a_question_about_an_abbreviated_commit_is_refused() {
    let text = edited(QUESTION_SCHEMA, |example| {
        example["about"]["commit"] = Value::from("abc1234");
    });
    let message = refused::<QuestionInput>("short", "question", QUESTION_SCHEMA, &text);
    assert!(message.contains("`about.commit`"), "{message}");
}

#[test]
fn findings_numbering_nothing_are_refused() {
    let message = refused::<FindingsInput>(
        "no-findings",
        "findings",
        FINDINGS_SCHEMA,
        r#"{"findings": []}"#,
    );
    assert!(
        message.ends_with(" numbers no finding — a return that numbers nothing is a question"),
        "{message}"
    );
}

#[test]
fn a_finding_with_no_text_is_refused_naming_it() {
    let message = refused::<FindingsInput>(
        "empty-finding",
        "findings",
        FINDINGS_SCHEMA,
        r#"{"findings": [{"text": "the arm is missing"}, {"text": ""}]}"#,
    );
    assert!(
        message.ends_with(" carries an empty `findings[1].text`"),
        "{message}"
    );
}

// ---- what the verbs make of them ------------------------------------------------

#[test]
fn a_delivery_that_reads_becomes_a_delivered_entry_that_validates() {
    let (input, _) = hand_in::<DeliveryInput>(
        "into-delivered",
        "delivery",
        DELIVERY_SCHEMA,
        &example(DELIVERY_SCHEMA).to_string(),
    );
    let input = input.expect("the example reads");
    let files = input.files.clone();
    let delivered = input.into_delivered(C1.into(), "work/orla-4a5b".into(), BASE.into());
    assert_eq!(delivered.commit, C1);
    assert_eq!(delivered.branch, "work/orla-4a5b");
    assert_eq!(delivered.base, BASE);
    assert_eq!(delivered.files, files, "the files are the seat's");
    Body::Delivered(delivered)
        .validate()
        .expect("a delivery that read is an entry that validates");
}

#[test]
fn a_question_that_reads_becomes_a_held_entry_that_validates_on_either_standing() {
    let read_one = |label: &str| {
        let (input, _) = hand_in::<QuestionInput>(
            label,
            "question",
            QUESTION_SCHEMA,
            &example(QUESTION_SCHEMA).to_string(),
        );
        input.expect("the example reads")
    };

    let held = read_one("into-held-work").into_held(
        "hold-1".into(),
        HoldReason::Ask,
        Standing::Work {
            branch: "work/orla-4a5b".into(),
            commit: C1.into(),
        },
    );
    assert_eq!(
        (
            held.branch.as_deref(),
            held.commit.as_deref(),
            held.run_hash.as_deref()
        ),
        (Some("work/orla-4a5b"), Some(C1), None),
        "a seat's hold stops on its work branch at a commit"
    );
    assert!(held.about.is_some() && held.context.is_some());
    Body::Held(held)
        .validate()
        .expect("a question that read is a hold that validates");

    let held = read_one("into-held-run").into_held(
        "hold-2".into(),
        HoldReason::MaxCrashes,
        Standing::Run {
            hash: "sha256:abc".into(),
        },
    );
    assert_eq!(
        (
            held.branch.as_deref(),
            held.commit.as_deref(),
            held.run_hash.as_deref()
        ),
        (None, None, Some("sha256:abc")),
        "a run's hold stops on the run"
    );
    Body::Held(held)
        .validate()
        .expect("a run's question that read is a hold that validates");
}

#[test]
fn findings_that_read_are_the_findings_a_return_carries() {
    let (input, _) = hand_in::<FindingsInput>(
        "into-findings",
        "findings",
        FINDINGS_SCHEMA,
        r#"{"findings": [{"text": "the arm is missing"}]}"#,
    );
    assert_eq!(
        input.expect("the findings read").findings(),
        vec![Finding {
            text: "the arm is missing".into()
        }]
    );
}
