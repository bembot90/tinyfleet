//! What bd's CLI prints that beads' OpenAPI spec does not define, written here
//! by hand beside the generated [`super::bd_wire`].
//!
//! Both shapes are the CLI's own `--json` contract, which beads defines only in
//! prose — docs/reference/json-schema.md, pinned by its
//! cmd/bd/protocol/json_contract_test.go — and not as a component: the spec's
//! `Problem` is the HTTP API's error, not the CLI's.
//!
//! THE ENVELOPE, `{"schema_version": N, "data": …}`, has no type here.
//! [`super::opened`] reads it in one place for every answer, enveloped or bare,
//! and warns once on a `schema_version` above [`super::SCHEMA_VERSION`]; a
//! typed envelope would be a second reading of the same two keys, and one that
//! could not take the bare answer a bd predating the envelope prints.

use serde::{Deserialize, Deserializer};

/// The object a `--json` call answers in place of its data when it fails:
/// `{schema_version, error, code?, hint?}`, with `schema_version` beside the
/// rest only when the answer is bare — inside the envelope it is the
/// envelope's. [`super::shown`] reads it.
///
/// `error` and `code` are the fields the adapter classifies by. `hint` is
/// advice to a person, and a refusal quotes `error` and not it.
///
/// AN ERROR IS TOLD BY ITS `error` TEXT ALONE. A `code` of any other shape
/// than text reads as no code: a strict `code` would let `{"error": …,
/// "code": 404}` fail to decode as an error and pass on as a row, and the
/// verb would act on an item the store just said is not there.
#[derive(Debug, Clone, Deserialize)]
pub struct CliError {
    pub error: String,
    /// Absent on the answers measured so far — bd 1.3.0's missing id carries
    /// none — so an error with no code is read by its key alone.
    #[serde(default, deserialize_with = "text_or_none")]
    pub code: Option<String>,
}

/// A value that is text, else none, whatever else it holds.
fn text_or_none<'de, D: Deserializer<'de>>(value: D) -> Result<Option<String>, D::Error> {
    Ok(match serde_json::Value::deserialize(value)? {
        serde_json::Value::String(text) => Some(text),
        _ => None,
    })
}

/// The wire types against the bd they were generated for, and bd 1.3.0's own
/// answers against the wire types.
///
/// The FIXTURES were recorded from bd 1.3.0 on a scratch board — `bd init` in
/// a temporary directory, five items, three dependencies (an open `blocks`, a
/// closed `blocks` and a `discovered-from`), metadata holding an order index
/// and a run on one item, a `fleet.orders` string on another and a foreign
/// bare `orders` and `run` on a third, and a gate — with
/// `BD_JSON_ENVELOPE=1`, the way every call the adapter makes is sent. Moving
/// the pin re-records them.
#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::super::{
        bd_wire, first_value, item_from, opened, shown, ItemId, OrderState, Status, StoreError,
        PINNED_BD,
    };

    const GENERATED: &str = include_str!("bd_wire.rs");
    const FRAGMENT: &[u8] = include_bytes!("bd_wire.schema.json");

    const SHOW: &str = include_str!("fixtures/show.json");
    const SHOW_UNREADABLE_ORDERS: &str = include_str!("fixtures/show_unreadable_orders.json");
    const SHOW_MISSING: &str = include_str!("fixtures/show_missing.json");
    const LIST: &str = include_str!("fixtures/list.json");
    const LIST_ASSIGNED: &str = include_str!("fixtures/list_assigned.json");
    const READY: &str = include_str!("fixtures/ready.json");
    const GATE_LIST: &str = include_str!("fixtures/gate_list.json");

    /// One `key: value` line off the generated file's header.
    fn stamped(key: &str) -> Option<&'static str> {
        GENERATED
            .lines()
            .take_while(|line| line.starts_with("//!"))
            .find_map(|line| {
                line.strip_prefix("//! ")?
                    .strip_prefix(key)?
                    .strip_prefix(": ")
            })
    }

    /// A recorded answer as the adapter reads it: its first value, opened.
    fn answer(text: &str) -> serde_json::Value {
        let value = first_value(text).expect("a recorded answer is JSON");
        opened(value, || String::from("a recorded answer"))
    }

    fn rows(text: &str) -> Vec<serde_json::Value> {
        match answer(text) {
            serde_json::Value::Array(rows) => rows,
            other => panic!("a recorded listing is an array: {other}"),
        }
    }

    /// Each row decodes into `Row`, and writing it back gives the row again —
    /// so bd printed no key the generated type lacks, and none of a type it
    /// does not name.
    fn decodes_whole<Row: DeserializeOwned + Serialize>(fixture: &str, text: &str) {
        let rows = rows(text);
        assert!(!rows.is_empty(), "{fixture} records at least one row");
        for row in rows {
            let decoded: Row = serde_json::from_value(row.clone())
                .unwrap_or_else(|why| panic!("{fixture}: a row does not decode ({why}): {row}"));
            assert_eq!(
                serde_json::to_value(&decoded).expect("a decoded row writes back"),
                row,
                "{fixture}: a key bd 1.3.0 printed is not in the generated type"
            );
        }
    }

    /// The id of the dependency the recorded `show` titles so.
    fn dependency_titled(title: &str) -> String {
        rows(SHOW)[0]["dependencies"]
            .as_array()
            .expect("the recorded show holds dependencies")
            .iter()
            .find(|entry| entry["title"] == title)
            .and_then(|entry| entry["id"].as_str())
            .unwrap_or_else(|| panic!("the recorded show depends on {title:?}"))
            .to_string()
    }

    #[test]
    fn the_wire_types_are_generated_at_the_pinned_bd() {
        assert_eq!(
            stamped("bd-tag"),
            Some(format!("v{PINNED_BD}").as_str()),
            "bd_wire.rs was not generated at the pinned bd {PINNED_BD} — rerun \
             `tools/bd-wire-types v{PINNED_BD}`"
        );
    }

    #[test]
    fn the_committed_fragment_is_the_one_the_wire_types_were_generated_from() {
        assert_eq!(
            stamped("fragment-sha256"),
            Some(crate::digest::hex(FRAGMENT).as_str()),
            "bd_wire.schema.json is not the fragment bd_wire.rs was generated from — rerun \
             `tools/bd-wire-types v{PINNED_BD}` rather than edit either by hand"
        );
    }

    #[test]
    fn a_recorded_show_decodes_into_issue_details() {
        decodes_whole::<bd_wire::IssueDetails>("show.json", SHOW);
        decodes_whole::<bd_wire::IssueDetails>(
            "show_unreadable_orders.json",
            SHOW_UNREADABLE_ORDERS,
        );
    }

    #[test]
    fn recorded_list_and_ready_rows_decode_into_issue_with_counts() {
        decodes_whole::<bd_wire::IssueWithCounts>("list.json", LIST);
        decodes_whole::<bd_wire::IssueWithCounts>("list_assigned.json", LIST_ASSIGNED);
        decodes_whole::<bd_wire::IssueWithCounts>("ready.json", READY);
    }

    #[test]
    fn recorded_gate_rows_decode_into_issue() {
        decodes_whole::<bd_wire::Issue>("gate_list.json", GATE_LIST);
    }

    /// Every field a verb asserts on, off bd 1.3.0's own answer: the open
    /// `blocks` is the one blocker, the closed one and the `discovered-from`
    /// are not.
    ///
    /// THE RECORDING PREDATES THE TYPED RECORD AND THE TYPED ORDER. Its
    /// `fleet.run` is no run's record — `{"v":1,"ok":true,"steps":[1,2]}` — so
    /// the row as recorded refuses the read, naming the key and its version;
    /// and its order index names `alberto`, who is no typed actor, and a kind
    /// fleet gives no order of, so with the run taken off the row the index
    /// reads as present and unreadable.
    #[test]
    fn a_recorded_show_reads_into_the_item_a_verb_asserts_on() {
        let row = shown("fx", answer(SHOW), "").expect("the item is there");
        let refusal = item_from("fx", &row).expect_err("the recorded run is no run record");
        assert!(
            matches!(
                &refusal,
                StoreError::Unreadable(why)
                    if why.contains("'s run record is not one this fleet reads (fleet.run, v 1)")
            ),
            "{refusal:?}"
        );

        let mut row = row;
        row["metadata"]
            .as_object_mut()
            .expect("the recorded show holds metadata")
            .remove("fleet.run");
        let item = item_from("fx", &row).expect("the rest of the recorded show decodes");

        assert_eq!(item.title, "downstream");
        assert_eq!(item.status, Status::Open);
        assert_eq!(item.item_type, "task");
        assert_eq!(item.assignee.as_deref(), Some("seat-1"));
        assert_eq!(item.labels, vec!["fleet", "fleet:run"]);
        assert_eq!(
            item.blockers,
            vec![ItemId::from(dependency_titled("upstream open"))]
        );
        assert_eq!(item.order, OrderState::Unreadable);
        assert_eq!(item.run, None);
        assert!(
            item.id.starts_with("fx-"),
            "the answer's id wins: {}",
            item.id
        );
        assert!(
            item.proof.carries("downstream"),
            "the proof is the row read"
        );
    }

    /// The three states of the order index, off bd 1.3.0's answers where it
    /// has them: a key holding no object, and no fleet key at all — which is
    /// what a row holding only another writer's bare `orders` and `run`
    /// answers.
    #[test]
    fn an_orders_key_holding_no_object_is_present_and_unreadable() {
        let row = shown("fx", answer(SHOW_UNREADABLE_ORDERS), "").expect("the item is there");
        let item = item_from("fx", &row).expect("the recorded show decodes");
        assert_eq!(item.order, OrderState::Unreadable);

        let bare = rows(READY)
            .into_iter()
            .find(|row| row["title"] == "discovered source")
            .expect("the recorded ready set holds the discovered source");
        assert!(
            bare["metadata"]["orders"].is_object() && bare["metadata"]["run"].is_string(),
            "the discovered source carries another writer's bare keys: {bare}"
        );
        let item = item_from("fx", &bare).expect("a ready row decodes as a show row");
        assert_eq!((item.order, item.run), (OrderState::None, None));
    }

    /// Metadata that is not an object at all — which beads' spec says a store
    /// can hold — decodes, and holds no order index.
    #[test]
    fn metadata_that_is_not_an_object_holds_no_orders() {
        let row = serde_json::json!({ "id": "fx-1", "metadata": [1, 2] });
        let item = item_from("fx-1", &row).expect("raw metadata decodes whatever it holds");
        assert_eq!((item.order, item.run), (OrderState::None, None));
    }

    /// A code bd did not spell as text still leaves the answer an error, read
    /// by its `error` alone, and never an item.
    #[test]
    fn an_error_whose_code_is_not_text_is_still_missing() {
        for code in [
            serde_json::json!(404),
            serde_json::json!(true),
            serde_json::json!({ "kind": "not_found" }),
            serde_json::Value::Null,
        ] {
            let row = serde_json::json!({ "error": "no issue fx-1", "code": code });
            let answer = shown("fx-1", row, "");
            assert!(
                matches!(&answer, Err(StoreError::Missing(why)) if why == "fx-1: no issue fx-1"),
                "code {code}: {answer:?}"
            );
        }
    }

    /// A row that does not decode is named by where it fails: the row's id
    /// and the path to the key, and not serde's reason alone.
    #[test]
    fn a_row_that_does_not_decode_names_its_id_and_key() {
        let row = serde_json::json!({ "id": "fx-1", "labels": ["fleet", 3] });
        let refusal = item_from("fx-1", &row).expect_err("a number is not a label");
        assert!(
            matches!(&refusal, StoreError::Unreadable(why) if why.contains("fx-1") && why.contains("labels[1]")),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_recorded_missing_id_is_the_records_answer() {
        let answer = shown("fx-nothere", answer(SHOW_MISSING), "");
        assert!(
            matches!(&answer, Err(StoreError::Missing(why)) if why.starts_with("fx-nothere: no issues found")),
            "{answer:?}"
        );
    }
}
