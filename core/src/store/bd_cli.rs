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

use serde::Deserialize;

/// The object a `--json` call answers in place of its data when it fails:
/// `{schema_version, error, code?, hint?}`, with `schema_version` beside the
/// rest only when the answer is bare — inside the envelope it is the
/// envelope's. [`super::shown`] reads it.
///
/// `error` and `code` are the fields the adapter classifies by. `hint` is
/// advice to a person, and a refusal quotes `error` and not it.
#[derive(Debug, Clone, Deserialize)]
pub struct CliError {
    pub error: String,
    /// Absent on the answers measured so far — bd 1.3.0's missing id carries
    /// none — so an error with no code is read by its key alone.
    pub code: Option<String>,
}

/// The wire types against the bd they were generated for, and bd 1.3.0's own
/// answers against the wire types.
///
/// The FIXTURES were recorded from bd 1.3.0 on a scratch board — `bd init` in
/// a temporary directory, five items, three dependencies (an open `blocks`, a
/// closed `blocks` and a `discovered-from`), metadata holding an order index
/// and a run on one item and an `orders` string on another, and a gate — with
/// `BD_JSON_ENVELOPE=1`, the way every call the adapter makes is sent. Moving
/// the pin re-records them.
#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::super::{bd_wire, first_value, item_from, opened, shown, StoreError, PINNED_BD};

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
    #[test]
    fn a_recorded_show_reads_into_the_item_a_verb_asserts_on() {
        let item = item_from(
            "fx",
            &shown("fx", answer(SHOW), "").expect("the item is there"),
        )
        .expect("the recorded show decodes");

        assert_eq!(item.title, "downstream");
        assert_eq!(item.status, "open");
        assert_eq!(item.item_type, "task");
        assert_eq!(item.assignee.as_deref(), Some("seat-1"));
        assert_eq!(item.notes.as_deref(), Some("a note on downstream"));
        assert_eq!(item.labels, vec!["fleet", "run"]);
        assert_eq!(item.blockers, vec![dependency_titled("upstream open")]);
        assert!(item.has_orders_key);
        assert_eq!(
            item.orders,
            Some(super::super::Orders {
                by: Some(String::from("alberto")),
                kind: Some(String::from("build")),
                seat: Some(String::from("seat-1")),
                at: Some(String::from("2026-09-23T10:00:00Z")),
            })
        );
        assert_eq!(
            item.run,
            Some(serde_json::json!({ "ok": true, "steps": [1, 2] }))
        );
        assert!(
            item.id.starts_with("fx-"),
            "the answer's id wins: {}",
            item.id
        );
    }

    /// The three states of the order index, off bd 1.3.0's answers where it
    /// has them: an object, a key holding no object, and no key at all.
    #[test]
    fn an_orders_key_holding_no_object_is_present_and_unreadable() {
        let row = shown("fx", answer(SHOW_UNREADABLE_ORDERS), "").expect("the item is there");
        let item = item_from("fx", &row).expect("the recorded show decodes");
        assert_eq!((item.orders, item.has_orders_key), (None, true));

        let bare = rows(READY)
            .into_iter()
            .find(|row| row["title"] == "discovered source")
            .expect("the recorded ready set holds the discovered source");
        let item = item_from("fx", &bare).expect("a ready row decodes as a show row");
        assert_eq!(
            (item.orders, item.has_orders_key, item.run),
            (None, false, None)
        );
    }

    /// Metadata that is not an object at all — which beads' spec says a store
    /// can hold — decodes, and holds no order index.
    #[test]
    fn metadata_that_is_not_an_object_holds_no_orders() {
        let row = serde_json::json!({ "id": "fx-1", "metadata": [1, 2] });
        let item = item_from("fx-1", &row).expect("raw metadata decodes whatever it holds");
        assert_eq!(
            (item.orders, item.has_orders_key, item.run),
            (None, false, None)
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
