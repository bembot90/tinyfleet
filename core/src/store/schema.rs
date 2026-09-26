//! The store contract as one JSON Schema document, generated from the types its
//! JSON is: what `fleet store schema` prints, and what `store.schema.json`
//! beside this file holds, byte for byte.
//!
//! ONE DOCUMENT, ONE SET OF DEFINITIONS. `verbs` holds each verb's `request`
//! and `response`, `refusal` and `error` hold the answers of exits 1 and 3, and
//! every `$ref` points into the document's own `$defs`: a tool loads the whole
//! document and reaches a verb's schema by its pointer,
//! `#/verbs/<verb>/request`.
//!
//! A REQUEST IS WHAT [`super::exec::Exec`] SENDS: the envelope's
//! `schema_version` and `root`, and the verb's fields by the names exec.rs
//! writes them, which an arm there holds to these. A RESPONSE IS THE BODY
//! `Exec` READS, with `schema_version` beside its fields.
//!
//! OBJECTS ARE OPEN WHERE THE TYPES ARE. A type that refuses a key it does not
//! name says `additionalProperties: false`, and every other object takes one:
//! a request above all, because an addition to a request is optional within
//! schema version 1, and an adapter refusing a key it does not know would
//! refuse the next fleet.
//!
//! NO PROSE FROM THE TYPES. The derive would carry each doc comment as a
//! `description`, and those are written for this code's readers — an adapter
//! author's prose is docs/store.md. So the generated schemas are stripped of
//! them, and the only descriptions are the ones [`FENCE`] and [`ASSIGNEE`]
//! write here, for the keys whose absence and whose `null` are two answers.

use schemars::generate::SchemaSettings;
use serde_json::{json, Map, Value};

use super::types::{
    Answered, Appended, Capabilities, Created, Exported, Listed, OpenHolds, Raised, Refusal,
    Resolved, Scratched, Shown, Update, Version, WithdrawFence, CONTRACT_VERSION,
};
use super::{Filter, HoldId, ItemId, NewItem, Order, RunRecord};
use crate::entry::{self, Entry};
use crate::schema::{self, describe, fields, generated, refer, strip};
use crate::seat::actor::Actor;

/// What an update's `assignee` says by being there, and by being `null`.
pub const ASSIGNEE: &str = "Absent, the assignee is left alone. A seat id hands the item to that \
                            seat, and null hands it to nobody.";

/// What an `if_assignee` says by being there, and by being `null`.
pub const FENCE: &str = "The fence, which changes nothing itself. Absent, the write is not \
                         fenced. A seat id, it lands only while that seat holds the item; null, \
                         only while nobody does. An item that does not meet it is refused as \
                         moved, with nothing written.";

/// The document: every verb's request and response, the refusal, the error,
/// and the definitions they share.
pub fn document() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let g = &mut generator;

    let id = refer::<ItemId>(g);
    let by = refer::<Actor>(g);
    let text = json!({"type": "string"});
    let none = Map::new;

    let mut update = generated::<Update>(g);
    describe(&mut update, "assignee", ASSIGNEE);
    describe(&mut update, "if_assignee", FENCE);
    let mut withdraw = generated::<WithdrawFence>(g);
    describe(&mut withdraw, "if_assignee", FENCE);

    // `append`'s entry is no type's own serde but `entry::encode`'s object,
    // so it is put in the definitions by hand, under a name of its own.
    let encoded = entry::encoded_schema(g).to_value();
    g.definitions_mut()
        .insert(String::from("NewEntry"), encoded);
    let new_entry = json!({"$ref": "#/$defs/NewEntry"});
    let entries = json!({"type": "array", "items": refer::<Entry>(g)});

    let filter = refer::<Filter>(g);
    let new_item = refer::<NewItem>(g);
    let order = refer::<Order>(g);
    let run = refer::<RunRecord>(g);
    let hold = refer::<HoldId>(g);
    let taken = generated::<Answered>(g);

    // Each verb: the fields of its request beyond the envelope, and the body
    // of its answer beyond `schema_version`.
    let each = [
        ("version", none(), generated::<Version>(g)),
        ("capabilities", none(), generated::<Capabilities>(g)),
        (
            "resolve",
            fields(none(), [("id", text.clone())]),
            generated::<Resolved>(g),
        ),
        (
            "show",
            fields(none(), [("id", text.clone())]),
            generated::<Shown>(g),
        ),
        (
            "list",
            fields(none(), [("filter", filter)]),
            generated::<Listed>(g),
        ),
        (
            "timeline",
            fields(none(), [("id", id.clone())]),
            fields(none(), [("entries", entries)]),
        ),
        (
            "create",
            fields(none(), [("item", new_item), ("by", by.clone())]),
            generated::<Created>(g),
        ),
        (
            "update",
            fields(update, [("id", id.clone()), ("by", by.clone())]),
            taken.clone(),
        ),
        (
            "append",
            fields(
                none(),
                [("id", id.clone()), ("by", by.clone()), ("entry", new_entry)],
            ),
            generated::<Appended>(g),
        ),
        (
            "order.set",
            fields(
                none(),
                [("id", id.clone()), ("by", by.clone()), ("order", order)],
            ),
            taken.clone(),
        ),
        (
            "order.withdraw",
            fields(withdraw, [("id", id.clone()), ("by", by.clone())]),
            taken.clone(),
        ),
        (
            "run.set",
            fields(
                none(),
                [("id", id.clone()), ("by", by.clone()), ("run", run)],
            ),
            taken.clone(),
        ),
        (
            "hold.raise",
            fields(
                none(),
                [
                    ("id", id.clone()),
                    ("by", by.clone()),
                    ("reason", text.clone()),
                ],
            ),
            generated::<Raised>(g),
        ),
        (
            "hold.clear",
            fields(none(), [("hold", hold), ("by", by.clone())]),
            taken.clone(),
        ),
        ("holds.open", none(), generated::<OpenHolds>(g)),
        (
            "close",
            fields(none(), [("id", id), ("by", by), ("reason", text.clone())]),
            taken,
        ),
        (
            "export",
            fields(none(), [("into", text.clone())]),
            generated::<Exported>(g),
        ),
        (
            "scratch",
            fields(none(), [("into", text.clone())]),
            generated::<Scratched>(g),
        ),
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

/// The check every suite holds a store instance to, lifted beside the agent's
/// into [`crate::schema`] and named here where the store's callers reach it.
#[cfg(any(test, feature = "test-support"))]
pub use crate::schema::check;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seat::identity::SeatId;
    use crate::store::types::{
        Answered, ExportSpec, Priorities, RefusalReason, Vocabulary, STAMP_PATTERN,
    };
    use crate::store::{Item, ItemSummary, OrderKind, OrderState, Stamp, Status};

    /// The document as it is committed, and as `fleet store schema` prints it.
    const COMMITTED: &str = include_str!("store.schema.json");

    const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";

    fn seat() -> SeatId {
        SeatId::parse(SEAT).expect("a well-formed seat id")
    }

    fn stamp() -> Stamp {
        Stamp::parse("2026-09-23T10:00:00Z").expect("a well-formed stamp")
    }

    fn value<T: serde::Serialize>(written: &T) -> Value {
        serde_json::to_value(written).expect("a contract type serializes")
    }

    /// The fields of `body` with `schema_version` beside them: an answer.
    fn answered(body: Value) -> Value {
        let mut answer = json!({"schema_version": CONTRACT_VERSION});
        if let (Value::Object(answer), Value::Object(body)) = (&mut answer, body) {
            answer.extend(body);
        }
        answer
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
            "core/src/store/store.schema.json is not the document the types generate (from line \
             {line}) — run `cargo run -p fleet-cli -- store schema > \
             core/src/store/store.schema.json`"
        );
    }

    /// The check refuses a keyword it does not know, but only where an
    /// instance leads it; this walks every schema in the document, so a
    /// keyword the generator starts writing is named here before any check
    /// passes over it.
    #[test]
    fn every_keyword_the_document_uses_is_one_the_check_knows() {
        let unknown = crate::schema::unknown_keywords(&document());
        assert!(
            unknown.is_empty(),
            "keywords the check does not know: {unknown:#?}"
        );
    }

    /// Each pattern the document carries takes exactly the text its type's
    /// reader takes, over the good forms and each way to miss them.
    #[test]
    fn each_pattern_takes_what_its_reader_takes() {
        let pattern = |text: &str| regex_lite::Regex::new(text).expect("the pattern compiles");
        let stamp = pattern(STAMP_PATTERN);
        for text in [
            "2026-09-23T10:00:00Z",
            "0000-01-01T00:00:00Z",
            "9999-12-31T23:59:59Z",
            "2026-02-31T10:00:00Z",
            "then",
            "2026-13-01T00:00:00Z",
            "2026-09-23 10:00:00Z",
            "2026-00-01T00:00:00Z",
            "2026-09-32T00:00:00Z",
            "2026-09-23T24:00:00Z",
            "2026-09-23T10:60:00Z",
            "2026-09-23T10:00:60Z",
            "2026-09-23T10:00:00+",
            "+026-09-23T10:00:00Z",
            "2026-09-23T10:00:00Z ",
            "2026-9-23T10:00:00Z",
        ] {
            assert_eq!(
                stamp.is_match(text),
                Stamp::parse(text).is_some(),
                "the stamp pattern and Stamp::parse disagree on {text:?}"
            );
        }

        let seat_id = document()["$defs"]["SeatId"]["pattern"].clone();
        let seat_id = pattern(seat_id.as_str().expect("a seat id has a pattern"));
        let actor = document()["$defs"]["Actor"]["pattern"].clone();
        let actor = pattern(actor.as_str().expect("an actor has a pattern"));
        for text in [
            SEAT,
            "0199A3C4-7D8E-7F90-A1B2-C3D4E5F60718",
            "0199a3c47d8e7f90a1b2c3d4e5f60718",
            "{0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718}",
            "0199a3c4-7d8e-7f90-a1b2-c3d4e5f6071",
            "0199a3c4-7d8e-7f90-a1b2-c3d4e5f6071g",
            "0199a3c4-7d8e-7f90-a1b2c-3d4e5f60718",
            "",
        ] {
            assert_eq!(
                seat_id.is_match(text),
                SeatId::parse(text).is_ok(),
                "the seat id pattern and SeatId::parse disagree on {text:?}"
            );
        }
        for text in [
            format!("seat:{SEAT}"),
            String::from("run:r-7"),
            String::from("routine:nightly"),
            String::from("controller:main"),
            String::from("seat:orla"),
            String::from("run:"),
            String::from("run:r 7"),
            String::from("routine:\tx"),
            String::from("person:alberto"),
            String::from("orla"),
            format!("seat:{SEAT}x"),
            String::from("controller"),
        ] {
            assert_eq!(
                actor.is_match(&text),
                matches!(Actor::typed(&text), Some(Ok(_))),
                "the actor pattern and Actor::typed disagree on {text:?}"
            );
        }
    }

    /// Every response body and the two failure answers, each written by its
    /// type with every field it can carry set, passes its schema; and the
    /// same shapes with one key the types refuse do not.
    #[test]
    fn each_answer_as_the_types_write_it_passes_its_schema() {
        let order = Order {
            kind: OrderKind::Dispatch,
            by: Actor::seat(seat()),
            seat: Some(seat()),
            at: stamp(),
        };
        let item = Item {
            id: ItemId::from("fx-a1b2"),
            title: String::from("t"),
            description: String::from("d"),
            status: Status::from("deferred"),
            item_type: String::from("task"),
            labels: vec![String::from("fleet")],
            assignee: Some(seat()),
            order: OrderState::Ordered(order.clone()),
            blockers: vec![ItemId::from("fx-c3d4")],
            run: Some(RunRecord {
                hash: String::from("abc"),
                workflow: String::from("build"),
                pack: String::from("ts"),
                entry: String::from("workflows/build.ts"),
                started_at: stamp(),
            }),
            foreign: vec![String::from("sprint")],
            ..Item::default()
        };
        let summary = ItemSummary {
            id: ItemId::from("fx-c3d4"),
            title: String::from("t"),
            status: Status::Closed,
            item_type: String::from("bug"),
            labels: Vec::new(),
            assignee: Some(seat()),
            order: OrderState::Unreadable,
            run: item.run.clone(),
            foreign: Vec::new(),
        };
        let capabilities = Capabilities {
            export: Some(ExportSpec {
                file: String::from("store/export.jsonl"),
                dir: String::from("store/"),
            }),
            scratch: true,
            item_prefix: Some(String::from("fx")),
            cli: Some(String::from("tracker")),
            items: Vocabulary {
                types: vec![String::from("story")],
                priority: Priorities { min: 1, max: 3 },
            },
        };
        for (verb, body) in [
            (
                "version",
                value(&Version {
                    name: String::from("tracker"),
                    version: String::from("0.4.0"),
                }),
            ),
            ("capabilities", value(&capabilities)),
            ("capabilities", value(&Capabilities::default())),
            (
                "resolve",
                value(&Resolved {
                    id: ItemId::from("fx-a1b2"),
                }),
            ),
            ("show", value(&Shown { item: item.clone() })),
            (
                "show",
                value(&Shown {
                    item: Item::default(),
                }),
            ),
            (
                "list",
                value(&Listed {
                    items: vec![summary.clone()],
                }),
            ),
            (
                "create",
                value(&Created {
                    id: ItemId::from("fx-a1b2"),
                }),
            ),
            ("update", value(&Answered {})),
            (
                "append",
                value(&Appended {
                    entry: String::from("e-17"),
                }),
            ),
            (
                "hold.raise",
                value(&Raised {
                    hold: HoldId::from("fx-h9"),
                }),
            ),
            (
                "holds.open",
                value(&OpenHolds {
                    holds: vec![HoldId::from("fx-h9")],
                }),
            ),
            (
                "export",
                value(&Exported {
                    file: String::from("/work/store/export.jsonl"),
                }),
            ),
            (
                "scratch",
                value(&Scratched {
                    root: String::from("/tmp/store"),
                }),
            ),
        ] {
            passes(&format!("/verbs/{verb}/response"), &answered(body));
        }
        for reason in [
            RefusalReason::Missing,
            RefusalReason::Ambiguous,
            RefusalReason::Already,
            RefusalReason::Moved,
        ] {
            let refusal = Refusal {
                reason,
                message: String::from("m"),
                candidates: vec![ItemId::from("fx-a1")],
            };
            passes(
                "/refusal",
                &json!({"schema_version": 1, "refused": value(&refusal)}),
            );
        }
        passes(
            "/refusal",
            &json!({"schema_version": 1, "refused": {"reason": "missing", "message": "m"}}),
        );
        passes("/error", &json!({"schema_version": 1, "error": "locked"}));

        // THE CONTROLS: each one key off what the types read, and refused.
        let mut item_value = value(&item);
        item_value["order"]["order"]["why"] = json!("x");
        refused(
            "/verbs/show/response",
            &answered(json!({"item": item_value})),
            "why",
        );
        let mut item_value = value(&item);
        item_value["run"]["started_at"] = json!("2026-13-01T00:00:00Z");
        refused(
            "/verbs/show/response",
            &answered(json!({"item": item_value})),
            "does not match",
        );
        let mut item_value = value(&item);
        item_value["assignee"] = json!("seat:orla");
        refused(
            "/verbs/show/response",
            &answered(json!({"item": item_value})),
            "does not match",
        );
        let mut summary_value = value(&summary);
        summary_value["order"] = json!({"state": "ordered"});
        refused(
            "/verbs/list/response",
            &answered(json!({"items": [summary_value]})),
            "`order` is required",
        );
        let mut summary_value = value(&summary);
        summary_value.as_object_mut().map(|row| row.remove("type"));
        refused(
            "/verbs/list/response",
            &answered(json!({"items": [summary_value]})),
            "`type` is required",
        );
        let mut capabilities_value = value(&capabilities);
        capabilities_value["items"]["priority"]["max"] = json!(5);
        refused(
            "/verbs/capabilities/response",
            &answered(capabilities_value),
            "maximum",
        );
        refused(
            "/verbs/resolve/response",
            &json!({"id": "fx-a1b2"}),
            "`schema_version` is required",
        );
        refused(
            "/verbs/resolve/response",
            &json!({"schema_version": 2, "id": "fx-a1b2"}),
            "is not 1",
        );
        refused(
            "/refusal",
            &json!({"schema_version": 1, "refused": {"reason": "gone", "message": "m"}}),
            "none of",
        );
    }

    /// The absent key and the `null` one are two answers on an update and a
    /// withdrawal, and the schema takes both as the types write them; a
    /// status other than the reopen's is the usage the contract refuses.
    #[test]
    fn an_absent_and_a_null_fence_both_pass_and_only_open_is_a_status() {
        let envelope = |mut fields: Value| {
            fields["id"] = json!("fx-a1b2");
            fields["by"] = json!(format!("seat:{SEAT}"));
            fields["root"] = json!("/work/project");
            fields["schema_version"] = json!(1);
            fields
        };
        for update in [
            Update::unassigned(),
            Update::assignee(seat()),
            Update::title(String::from("t")),
            Update {
                status: Some(Status::Open),
                ..Update::fenced(None)
            },
            Update {
                assignee: Some(Some(seat())),
                ..Update::fenced(Some(seat()))
            },
        ] {
            passes("/verbs/update/request", &envelope(value(&update)));
        }
        for fence in [
            WithdrawFence::default(),
            WithdrawFence {
                if_assignee: Some(None),
                ..WithdrawFence::default()
            },
            WithdrawFence {
                if_assignee: Some(Some(seat())),
                if_status: Some(Status::InProgress),
                reopen: true,
            },
        ] {
            passes("/verbs/order.withdraw/request", &envelope(value(&fence)));
        }
        let document = document();
        for (verb, key, words) in [
            ("update", "assignee", ASSIGNEE),
            ("update", "if_assignee", FENCE),
            ("order.withdraw", "if_assignee", FENCE),
        ] {
            assert_eq!(
                document["verbs"][verb]["request"]["properties"][key]["description"],
                json!(words),
                "{verb}'s {key} says what its absence and its null are"
            );
        }

        // THE CONTROLS: a status the reopen does not set, and a seat named
        // as an actor.
        let closing = Update {
            status: Some(Status::Closed),
            ..Update::default()
        };
        refused(
            "/verbs/update/request",
            &envelope(value(&closing)),
            "is not \"open\"",
        );
        refused(
            "/verbs/update/request",
            &envelope(json!({"if_assignee": format!("seat:{SEAT}")})),
            "branches pass",
        );
    }
}
