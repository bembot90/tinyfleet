//! The one JSON document every verb the SDK calls prints under `--json`.
//!
//! THE DOCUMENT IS THE OUTCOME AND NOT A RENDERING OF IT — the rule `status.rs`
//! states for its own flag. A caller parses one document and learns what
//! happened; the exit code beside it is the exit table's own row, unchanged by
//! the flag.
//!
//! The refusal's `code` names the exit table's ROW rather than repeating its
//! number: the number is on `$?`, and a second spelling of it is a second thing
//! to keep in step.
//!
//! The key order is fixed — `ok`, then `verb`, then the outcome — and the two
//! documents are built as text because this workspace pins `serde_json` with
//! `preserve_order` off (the pin's own comment, `fleet/Cargo.toml`), so a map
//! would answer the keys alphabetically instead. Every value still goes through
//! `serde_json`, so the escaping is the library's and never this module's.

use crate::exit::Exit;

/// The success document: `{"ok":true,"verb":"<verb>","data":<value>}`.
pub fn ok(verb: &str, data: &serde_json::Value) -> String {
    format!(
        "{{\"ok\":true,\"verb\":{},\"data\":{}}}",
        quoted(verb),
        serialized(data)
    )
}

/// The refusal document, whose `code` is [`Exit::class`] and whose `why` is the
/// same sentence the verb says on stderr.
pub fn refusal(verb: &str, exit: Exit, why: &str) -> String {
    format!(
        "{{\"ok\":false,\"verb\":{},\"refusal\":{{\"code\":{},\"why\":{}}}}}",
        quoted(verb),
        quoted(exit.class()),
        quoted(why)
    )
}

/// A string as a JSON string. A `&str` always serializes; the fallback is an
/// empty JSON string so a verb answering a caller cannot end in a panic.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| String::from("\"\""))
}

/// A value as JSON. Same rule as [`quoted`]: `null` rather than a panic, and a
/// document that parses either way.
fn serialized(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::from("null"))
}

#[cfg(test)]
mod tests {
    use super::{ok, refusal};
    use crate::exit::Exit;

    /// The two shapes, byte for byte. The key order is part of what this module
    /// promises a caller, so the pin is the whole string and not a parse and a
    /// look at three fields.
    #[test]
    fn the_two_shapes_are_pinned_byte_for_byte() {
        assert_eq!(
            ok("event show", &serde_json::json!({ "seq": 3 })),
            r#"{"ok":true,"verb":"event show","data":{"seq":3}}"#
        );
        assert_eq!(
            refusal("event show", Exit::Refused, "no event 0a-01"),
            r#"{"ok":false,"verb":"event show","refusal":{"code":"refused","why":"no event 0a-01"}}"#
        );
    }

    /// Every row of the exit table has a code and no two rows share one: a
    /// caller branching on the code would otherwise meet two outcomes under one
    /// name.
    #[test]
    fn every_exit_row_carries_its_own_refusal_code() {
        let rows = [
            Exit::Done,
            Exit::Refused,
            Exit::Usage,
            Exit::CouldNotTell,
            Exit::NoSession,
            Exit::NoCollector,
            Exit::Transient,
        ];
        let mut seen: Vec<&str> = rows.iter().map(|row| row.class()).collect();
        assert!(
            seen.iter().all(|code| !code.is_empty()),
            "every row names itself: {seen:?}"
        );
        seen.sort_unstable();
        let named = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), named, "no two rows share a code: {seen:?}");
    }

    /// The text a refusal carries is escaped by the library: a `why` naming a
    /// path in quotes is still one JSON string, which is the property a
    /// hand-built document is the easiest place to lose.
    #[test]
    fn a_why_carrying_a_quote_stays_one_json_string() {
        let why = "no event stream at \"/tmp/a b\"\nand nothing else";
        let document = refusal("event tail", Exit::NoCollector, why);
        let parsed: serde_json::Value =
            serde_json::from_str(&document).expect("the document parses");
        assert_eq!(parsed["ok"], serde_json::Value::Bool(false));
        assert_eq!(parsed["refusal"]["code"], "no_collector");
        assert_eq!(parsed["refusal"]["why"], why);
    }

    /// The success document's `data` is whatever the verb handed over, and a
    /// value carrying a quote survives it for the same reason.
    #[test]
    fn the_success_document_carries_the_data_through_unchanged() {
        let data = serde_json::json!({ "text": "a \"quoted\" line", "seq": 7 });
        let document = ok("event tail", &data);
        let parsed: serde_json::Value =
            serde_json::from_str(&document).expect("the document parses");
        assert_eq!(parsed["ok"], serde_json::Value::Bool(true));
        assert_eq!(parsed["verb"], "event tail");
        assert_eq!(parsed["data"], data);
    }
}
