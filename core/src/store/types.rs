//! The store contract's domain: what an adapter is asked, and what it answers,
//! as fleet's own types.
//!
//! THE JSON IS THE CONTRACT. Every type here derives its serde form, and that
//! form is the one the store's documentation prints and an external adapter
//! sends: a request is a verb's fields with the contract's version and the
//! project's root beside them ([`request`]), and an answer is one JSON object
//! at that version holding the verb's response body ([`answer`]).
//!
//! NOTHING READS THESE YET. The trait still speaks the older shapes beside
//! them in this module; the verbs move onto these one at a time, and until
//! they do only this file's own tests hold them.
//!
//! AN ABSENT KEY AND A NULL ONE ARE TWO ANSWERS WHERE A WRITE SAYS SO. An
//! [`Update`] that leaves `assignee` out touches nothing, and one that says
//! `"assignee": null` clears it — so that field is read and written by hand,
//! and never by the derive that would fold the two together.

use std::fmt;
use std::ops::Deref;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;

/// The version of the contract this fleet speaks, carried as `schema_version`
/// on every request and demanded on every answer.
pub const CONTRACT_VERSION: u64 = 1;

// ---- ids ------------------------------------------------------------------------

/// One store-minted id, held as the text the store gave it: fleet never
/// parses, shortens or sorts it.
macro_rules! store_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(id: impl Into<String>) -> $name {
                $name(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> $name {
                $name(id.to_string())
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> $name {
                $name(id)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }
    };
}

store_id! {
    /// An item's id, whole, as the store minted it.
    ItemId
}

store_id! {
    /// A hold's id, whole, as the store minted it when the hold was raised.
    HoldId
}

// ---- status ---------------------------------------------------------------------

/// Where an item stands: the three states fleet acts on by name, and any other
/// the store keeps, carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Status {
    Open,
    InProgress,
    Closed,
    Other(String),
}

impl Status {
    pub fn as_str(&self) -> &str {
        match self {
            Status::Open => "open",
            Status::InProgress => "in_progress",
            Status::Closed => "closed",
            Status::Other(text) => text,
        }
    }
}

/// The empty text an item read without a status carries, as `Other("")`.
impl Default for Status {
    fn default() -> Status {
        Status::Other(String::new())
    }
}

impl From<&str> for Status {
    fn from(text: &str) -> Status {
        match text {
            "open" => Status::Open,
            "in_progress" => Status::InProgress,
            "closed" => Status::Closed,
            other => Status::Other(other.to_string()),
        }
    }
}

impl From<String> for Status {
    fn from(text: String) -> Status {
        match text.as_str() {
            "open" | "in_progress" | "closed" => Status::from(text.as_str()),
            _ => Status::Other(text),
        }
    }
}

impl From<Status> for String {
    fn from(status: Status) -> String {
        match status {
            Status::Other(text) => text,
            named => named.as_str().to_string(),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for Status {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Status {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

// ---- stamp ----------------------------------------------------------------------

/// A moment as fleet writes one: `YYYY-MM-DDTHH:MM:SSZ`, in UTC, to the second.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Stamp(String);

impl Stamp {
    /// The text as a stamp, when it is exactly the 20-character form with a
    /// month, day, hour, minute and second each in its range.
    pub fn parse(text: &str) -> Option<Stamp> {
        let bytes = text.as_bytes();
        if bytes.len() != 20 {
            return None;
        }
        for (at, separator) in [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'Z'),
        ] {
            if bytes[at] != separator {
                return None;
            }
        }
        // Each field is its digits and nothing else — a sign or a space is not
        // one — and a year is any four of them.
        let field = |from: usize, to: usize| -> Option<u32> {
            let digits = &bytes[from..to];
            if !digits.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(digits.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0')))
        };
        field(0, 4)?;
        let in_range = [
            (field(5, 7)?, 1..=12),
            (field(8, 10)?, 1..=31),
            (field(11, 13)?, 0..=23),
            (field(14, 16)?, 0..=59),
            (field(17, 19)?, 0..=59),
        ];
        if in_range.iter().all(|(value, range)| range.contains(value)) {
            Some(Stamp(text.to_string()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Stamp {
    type Error = String;

    fn try_from(text: String) -> Result<Stamp, String> {
        Stamp::parse(&text)
            .ok_or_else(|| format!("{text} is not a stamp — the form is YYYY-MM-DDTHH:MM:SSZ"))
    }
}

impl From<Stamp> for String {
    fn from(stamp: Stamp) -> String {
        stamp.0
    }
}

// ---- the order ------------------------------------------------------------------

/// What an order asks of the seat it names.
///
/// Not [`crate::entry::OrderKind`], which is the `ordered` entry's and spells
/// only the dispatch its writers make today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    Dispatch,
    Review,
}

/// The order an item stands under: what it asks, who gave it, the seat it is
/// for where one is named, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub kind: OrderKind,
    pub by: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<SeatId>,
    pub at: Stamp,
}

/// Whether an item is ordered: no order, one the store holds and fleet cannot
/// read, or the order itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "order", rename_all = "snake_case")]
pub enum OrderState {
    #[default]
    None,
    Unreadable,
    Ordered(Order),
}

// ---- the run's record -----------------------------------------------------------

/// A run's record, on the item that records it: the workflow's pinned hash,
/// the workflow, its pack and entry, and when the run started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub hash: String,
    pub workflow: String,
    pub pack: String,
    pub entry: String,
    pub started_at: Stamp,
}

// ---- the read's proof -----------------------------------------------------------

/// The whole text a read answered, kept beside the item so a check can ask
/// whether a token it wrote came back — never serialized, because it is the
/// reading's and not the item's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadProof(String);

impl ReadProof {
    pub fn of(text: impl Into<String>) -> ReadProof {
        ReadProof(text.into())
    }

    /// Whether the read's text carries the token anywhere.
    pub fn carries(&self, token: &str) -> bool {
        self.0.contains(token)
    }
}

// ---- items ----------------------------------------------------------------------

/// One item as a read answers it: its id, title, status and type, its own
/// labels, who holds it, its order, what blocks it and a run's record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    pub title: String,
    pub status: Status,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignee: Option<SeatId>,
    #[serde(default)]
    pub order: OrderState,
    #[serde(default)]
    pub blockers: Vec<ItemId>,
    #[serde(default)]
    pub run: Option<RunRecord>,
    #[serde(skip)]
    pub proof: ReadProof,
}

/// One item as a listing answers it: enough to choose from without reading
/// each one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSummary {
    pub id: ItemId,
    pub title: String,
    pub status: Status,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub order: OrderState,
}

/// Which items a listing asks for: the ready ones, those carrying a label, or
/// those a seat holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Ready,
    Label(String),
    Assignee(SeatId),
}

/// An item to file: the store names it, so there is no id here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewItem {
    pub title: String,
    pub description: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
}

impl NewItem {
    /// The item, or the reason a store must not file it: a priority outside
    /// 0 to 4.
    pub fn validate(&self) -> Result<(), String> {
        match self.priority {
            Some(n) if n > 4 => Err(format!("priority is {n}; the range is 0 to 4")),
            _ => Ok(()),
        }
    }
}

/// A change to one item: each field is left alone where it is absent, and
/// `"assignee": null` is the item handed to nobody.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "assignee_out",
        deserialize_with = "assignee_in"
    )]
    pub assignee: Option<Option<SeatId>>,
}

/// A present `assignee` as its JSON: the seat's id, or `null` for nobody. An
/// absent one never reaches here, because the key is skipped.
fn assignee_out<S: Serializer>(
    assignee: &Option<Option<SeatId>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match assignee {
        Some(Some(seat)) => serializer.serialize_some(seat),
        Some(None) | None => serializer.serialize_none(),
    }
}

/// A present `assignee` key, `null` included, as `Some`: only a key that is
/// not there at all is left to `default` as `None`.
fn assignee_in<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<SeatId>>, D::Error> {
    Option::<SeatId>::deserialize(deserializer).map(Some)
}

impl Update {
    pub fn title(title: String) -> Update {
        Update {
            title: Some(title),
            ..Update::default()
        }
    }

    pub fn assignee(seat: SeatId) -> Update {
        Update {
            assignee: Some(Some(seat)),
            ..Update::default()
        }
    }

    pub fn unassigned() -> Update {
        Update {
            assignee: Some(None),
            ..Update::default()
        }
    }

    /// Whether this changes nothing at all.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.assignee.is_none()
    }
}

// ---- what a store can do --------------------------------------------------------

/// What a store says it can do beyond the verbs every store answers: an
/// export fleet commits, a scratch board for a suite, and the prefix its ids
/// carry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub export: Option<ExportSpec>,
    #[serde(default)]
    pub scratch: bool,
    #[serde(default)]
    pub item_prefix: Option<String>,
}

/// Where a store's export lands, relative to the project root: the file, and
/// the directory it sits in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSpec {
    pub file: String,
    pub dir: String,
}

impl ExportSpec {
    /// The spec, or the field and value that break it: both paths relative,
    /// `/`-separated and without a `..` segment, `dir` ending in `/`, and
    /// `file` under `dir`.
    pub fn validate(&self) -> Result<(), String> {
        for (field, value) in [("file", &self.file), ("dir", &self.dir)] {
            if value.starts_with('/') {
                return Err(format!(
                    "export {field} `{value}` is absolute — it is relative to the project root"
                ));
            }
            if value.split('/').any(|segment| segment == "..") {
                return Err(format!(
                    "export {field} `{value}` has a .. segment — it stays under the project root"
                ));
            }
        }
        if !self.dir.ends_with('/') {
            return Err(format!("export dir `{}` does not end in /", self.dir));
        }
        if !self.file.starts_with(&self.dir) || self.file.len() == self.dir.len() {
            return Err(format!(
                "export file `{}` is not under dir `{}`",
                self.file, self.dir
            ));
        }
        Ok(())
    }
}

/// Which store answered, and at which version of itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub name: String,
    pub version: String,
}

// ---- refusals -------------------------------------------------------------------

/// The store's answer that the act cannot be done as asked, and why — the
/// record's answer, not a store that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub reason: RefusalReason,
    pub message: String,
    #[serde(default)]
    pub candidates: Vec<ItemId>,
}

/// Why a store refused: nothing by that id, more than one item it could mean,
/// or the act already done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Missing,
    Ambiguous,
    Already,
}

// ---- response bodies ------------------------------------------------------------
//
// The timeline's answer is not one of these: its entries are
// [`crate::entry`]'s, read and folded there.

/// The one item a partial id names, whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub id: ItemId,
}

/// One item, read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    pub item: Item,
}

/// The items a filter matched, in the store's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub items: Vec<ItemSummary>,
}

/// The id the store gave an item it filed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created {
    pub id: ItemId,
}

/// The id the store gave an entry it appended to an item's timeline, as text:
/// the same id [`crate::entry::Entry`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Appended {
    pub entry: String,
}

/// The id of a hold the store raised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raised {
    pub hold: HoldId,
}

/// Every hold the store still calls open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenHolds {
    pub holds: Vec<HoldId>,
}

/// The export file the store wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exported {
    pub file: String,
}

/// The root of a scratch board the store made for a suite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scratched {
    pub root: String,
}

/// A write the store took, which answers nothing more.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answered {}

// ---- the envelope ---------------------------------------------------------------

/// A verb's fields as one request: `schema_version` at [`CONTRACT_VERSION`] and
/// the project's `root` inserted beside them.
pub fn request(
    mut verb_fields: serde_json::Map<String, serde_json::Value>,
    root: &Path,
) -> serde_json::Value {
    verb_fields.insert(
        String::from("schema_version"),
        serde_json::Value::from(CONTRACT_VERSION),
    );
    verb_fields.insert(
        String::from("root"),
        serde_json::Value::from(root.display().to_string()),
    );
    serde_json::Value::Object(verb_fields)
}

/// An answer's response body, read off the FIRST JSON value of the text with
/// whatever trails it ignored: an object at `schema_version`
/// [`CONTRACT_VERSION`] exactly, with that key taken out before the body is
/// decoded.
pub fn answer<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let Some(value) = super::first_value(text) else {
        return Err(String::from("answered no JSON value"));
    };
    let serde_json::Value::Object(mut body) = value else {
        return Err(format!("answered {value}, which is not an object"));
    };
    match body.remove("schema_version") {
        None => return Err(String::from("answered no schema_version")),
        Some(version) if version.as_u64() != Some(CONTRACT_VERSION) => {
            return Err(format!(
                "answered schema_version {version}; this fleet speaks {CONTRACT_VERSION}"
            ));
        }
        Some(_) => {}
    }
    serde_json::from_value(serde_json::Value::Object(body))
        .map_err(|why| format!("the answer does not read — {why}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";
    const PERSON: &str = "0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d";

    /// The item the store's documentation prints, byte for byte: a read of
    /// one ordered item, held by the seat it was ordered to.
    const ITEM_EXAMPLE: &str = r#"{
  "id": "fx-a1b2",
  "title": "Teach the parser the new stamp",
  "status": "in_progress",
  "type": "task",
  "labels": [
    "fleet"
  ],
  "assignee": "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
  "order": {
    "state": "ordered",
    "order": {
      "kind": "dispatch",
      "by": "seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d",
      "seat": "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718",
      "at": "2026-09-23T10:00:00Z"
    }
  },
  "blockers": [
    "fx-c3d4"
  ],
  "run": null
}"#;

    fn seat(id: &str) -> SeatId {
        SeatId::parse(id).expect("a well-formed seat id")
    }

    fn stamp(text: &str) -> Stamp {
        Stamp::parse(text).expect("a well-formed stamp")
    }

    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).expect("a contract type serializes")
    }

    fn an_order(seat_named: Option<SeatId>) -> Order {
        Order {
            kind: OrderKind::Dispatch,
            by: Actor::seat(seat(PERSON)),
            seat: seat_named,
            at: stamp("2026-09-23T10:00:00Z"),
        }
    }

    // ---- 1: status ----

    #[test]
    fn the_three_named_statuses_round_trip_to_their_variants() {
        for (text, status) in [
            ("open", Status::Open),
            ("in_progress", Status::InProgress),
            ("closed", Status::Closed),
        ] {
            let read: Status = serde_json::from_str(&format!("\"{text}\"")).unwrap();
            assert_eq!(read, status);
            assert_eq!(json(&read), format!("\"{text}\""));
        }
    }

    #[test]
    fn any_other_status_is_carried_verbatim() {
        let read: Status = serde_json::from_str("\"deferred\"").unwrap();
        assert_eq!(read, Status::Other(String::from("deferred")));
        assert_eq!(json(&read), "\"deferred\"");
    }

    #[test]
    fn a_status_compares_equal_to_its_text() {
        let (open, deferred) = (Status::from("open"), Status::from("deferred"));
        assert!(open == "open");
        assert!(deferred == "deferred");
        assert!(Status::Closed != "open");
    }

    #[test]
    fn a_default_status_is_the_empty_text() {
        assert_eq!(Status::default(), Status::Other(String::new()));
        assert_eq!(Item::default().status, "");
    }

    // ---- 2: the order ----

    #[test]
    fn an_order_state_is_exactly_one_of_three_objects() {
        assert_eq!(json(&OrderState::None), r#"{"state":"none"}"#);
        assert_eq!(json(&OrderState::Unreadable), r#"{"state":"unreadable"}"#);
        assert_eq!(
            json(&OrderState::Ordered(an_order(Some(seat(SEAT))))),
            format!(
                r#"{{"state":"ordered","order":{{"kind":"dispatch","by":"seat:{PERSON}","seat":"{SEAT}","at":"2026-09-23T10:00:00Z"}}}}"#
            )
        );
        for state in [
            OrderState::None,
            OrderState::Unreadable,
            OrderState::Ordered(an_order(Some(seat(SEAT)))),
        ] {
            let read: OrderState = serde_json::from_str(&json(&state)).unwrap();
            assert_eq!(read, state);
        }
    }

    #[test]
    fn an_order_without_a_seat_omits_the_key() {
        let written = json(&an_order(None));
        assert!(!written.contains("seat\":"), "{written}");
        let read: Order = serde_json::from_str(&written).unwrap();
        assert_eq!(read, an_order(None));
    }

    #[test]
    fn an_order_with_a_key_it_does_not_have_does_not_decode() {
        let text = format!(
            r#"{{"kind":"review","by":"seat:{PERSON}","at":"2026-09-23T10:00:00Z","why":"x"}}"#
        );
        let refused = serde_json::from_str::<Order>(&text).unwrap_err();
        assert!(refused.to_string().contains("why"), "{refused}");

        // THE CONTROL: the same text without the extra key reads.
        let text =
            format!(r#"{{"kind":"review","by":"seat:{PERSON}","at":"2026-09-23T10:00:00Z"}}"#);
        let read: Order = serde_json::from_str(&text).unwrap();
        assert_eq!(read.kind, OrderKind::Review);
    }

    // ---- 3: the stamp ----

    #[test]
    fn a_stamp_is_the_twenty_character_form() {
        assert_eq!(
            Stamp::parse("2026-09-23T10:00:00Z").map(|s| s.to_string()),
            Some(String::from("2026-09-23T10:00:00Z"))
        );
    }

    /// RED-PROOF: a parse that checks only the length passes the month-13
    /// stamp, which is the same length as a good one.
    #[test]
    fn a_stamp_off_the_form_or_out_of_range_is_refused() {
        for text in [
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
        ] {
            assert_eq!(Stamp::parse(text), None, "{text} is not a stamp");
        }
    }

    #[test]
    fn a_stamp_that_does_not_parse_does_not_decode() {
        let refused = serde_json::from_str::<Stamp>("\"then\"").unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("then is not a stamp — the form is YYYY-MM-DDTHH:MM:SSZ"),
            "{refused}"
        );
    }

    // ---- 4: the filter ----

    #[test]
    fn a_filter_is_a_word_or_a_one_key_object_and_reads_back() {
        for (filter, text) in [
            (Filter::Ready, String::from("\"ready\"")),
            (
                Filter::Label(String::from("x")),
                String::from(r#"{"label":"x"}"#),
            ),
            (
                Filter::Assignee(seat(SEAT)),
                format!(r#"{{"assignee":"{SEAT}"}}"#),
            ),
        ] {
            assert_eq!(json(&filter), text);
            let read: Filter = serde_json::from_str(&text).unwrap();
            assert_eq!(read, filter);
        }
    }

    // ---- 5: the update ----

    #[test]
    fn an_empty_update_changes_nothing() {
        let read: Update = serde_json::from_str("{}").unwrap();
        assert!(read.is_empty());
        assert_eq!(read, Update::default());
        assert_eq!(json(&read), "{}");
        assert!(!Update::title(String::from("t")).is_empty());
        assert!(!Update::unassigned().is_empty());
    }

    /// RED-PROOF: a derived `Option<Option<_>>` under `serde(default)` reads
    /// `null` as `None`, which is the untouched assignee and not the cleared one.
    #[test]
    fn a_null_assignee_is_cleared_and_an_absent_one_untouched() {
        let cleared: Update = serde_json::from_str(r#"{"assignee":null}"#).unwrap();
        assert_eq!(cleared.assignee, Some(None));
        assert_eq!(cleared, Update::unassigned());

        let untouched: Update = serde_json::from_str(r#"{"title":"t"}"#).unwrap();
        assert_eq!(untouched.assignee, None);
        assert_eq!(untouched, Update::title(String::from("t")));

        let assigned: Update =
            serde_json::from_str(&format!(r#"{{"assignee":"{SEAT}"}}"#)).unwrap();
        assert_eq!(assigned, Update::assignee(seat(SEAT)));
    }

    #[test]
    fn a_cleared_and_an_untouched_assignee_round_trip_distinctly() {
        let cleared = json(&Update::unassigned());
        let untouched = json(&Update::title(String::from("t")));
        let assigned = json(&Update::assignee(seat(SEAT)));
        assert_eq!(cleared, r#"{"assignee":null}"#);
        assert_eq!(untouched, r#"{"title":"t"}"#);
        assert_eq!(assigned, format!(r#"{{"assignee":"{SEAT}"}}"#));
        for (text, update) in [
            (cleared, Update::unassigned()),
            (untouched, Update::title(String::from("t"))),
            (assigned, Update::assignee(seat(SEAT))),
        ] {
            assert_eq!(serde_json::from_str::<Update>(&text).unwrap(), update);
        }
    }

    // ---- 6: the export ----

    #[test]
    fn an_export_spec_off_the_root_or_outside_its_dir_is_refused() {
        let spec = |file: &str, dir: &str| ExportSpec {
            file: file.to_string(),
            dir: dir.to_string(),
        };
        for (refused, names) in [
            (spec("/abs/x", "/abs/"), "export file `/abs/x` is absolute"),
            (spec("../x", "../"), "export file `../x` has a .. segment"),
            (spec("a/x", "b/"), "export file `a/x` is not under dir `b/`"),
            (spec("a/x", "/a/"), "export dir `/a/` is absolute"),
            (
                spec("a/../x", "a/"),
                "export file `a/../x` has a .. segment",
            ),
            (spec("a/x", "a"), "export dir `a` does not end in /"),
            (spec("a/", "a/"), "export file `a/` is not under dir `a/`"),
        ] {
            let why = refused.validate().unwrap_err();
            assert!(why.starts_with(names), "{why}");
        }

        // THE CONTROL: a relative file under its dir is kept.
        assert_eq!(spec("store/export.jsonl", "store/").validate(), Ok(()));
    }

    // ---- 7: the envelope ----

    #[test]
    fn an_answer_without_the_contract_version_is_refused() {
        assert_eq!(
            answer::<Answered>(r#"{}"#),
            Err(String::from("answered no schema_version"))
        );
        assert_eq!(
            answer::<Answered>(r#"{"schema_version":2}"#),
            Err(String::from(
                "answered schema_version 2; this fleet speaks 1"
            ))
        );
        assert!(answer::<Answered>("[]").is_err());
        assert!(answer::<Answered>("").is_err());
    }

    #[test]
    fn an_answer_is_its_first_value_and_ignores_what_trails_it() {
        let read: Created =
            answer("{\"schema_version\":1,\"id\":\"fx-a1b2\"}\ntrailing bytes {").unwrap();
        assert_eq!(read.id, "fx-a1b2");
        let read: Answered = answer("{\"schema_version\":1}\n").unwrap();
        assert_eq!(read, Answered {});
    }

    #[test]
    fn a_request_carries_the_contract_version_and_the_root() {
        let mut fields = serde_json::Map::new();
        fields.insert(String::from("verb"), serde_json::Value::from("show"));
        let sent = request(fields, Path::new("/work/project"));
        assert_eq!(
            sent,
            serde_json::json!({"verb": "show", "schema_version": 1, "root": "/work/project"})
        );
    }

    // ---- 8: the item the documentation prints ----

    #[test]
    fn the_documented_item_reads_and_writes_back_byte_for_byte() {
        let read: Item = serde_json::from_str(ITEM_EXAMPLE).unwrap();
        assert_eq!(read.id, "fx-a1b2");
        assert_eq!(read.status, Status::InProgress);
        assert_eq!(read.assignee, Some(seat(SEAT)));
        assert_eq!(read.order, OrderState::Ordered(an_order(Some(seat(SEAT)))));
        assert_eq!(read.blockers, vec![ItemId::from("fx-c3d4")]);
        assert_eq!(read.proof, ReadProof::default());
        assert_eq!(serde_json::to_string_pretty(&read).unwrap(), ITEM_EXAMPLE);
    }

    // ---- the rest of the vocabulary ----

    #[test]
    fn an_item_read_without_its_optional_keys_takes_their_defaults() {
        let read: Item =
            serde_json::from_str(r#"{"id":"fx-1","title":"t","status":"open","type":"task"}"#)
                .unwrap();
        assert_eq!(
            read,
            Item {
                id: ItemId::from("fx-1"),
                title: String::from("t"),
                status: Status::Open,
                item_type: String::from("task"),
                ..Item::default()
            }
        );
    }

    #[test]
    fn a_run_record_round_trips_and_refuses_a_key_it_does_not_have() {
        let record = RunRecord {
            hash: String::from("abc"),
            workflow: String::from("build"),
            pack: String::from("ts"),
            entry: String::from("workflows/build.ts"),
            started_at: stamp("2026-09-23T10:00:00Z"),
        };
        let written = json(&record);
        assert_eq!(serde_json::from_str::<RunRecord>(&written).unwrap(), record);
        let extra = written.replacen('{', r#"{"v":1,"#, 1);
        assert!(
            serde_json::from_str::<RunRecord>(&extra).is_err(),
            "{extra}"
        );
    }

    #[test]
    fn an_id_is_its_text() {
        let id = ItemId::new("fx-1");
        assert_eq!(id.as_str(), "fx-1");
        assert_eq!(id.to_string(), "fx-1");
        let text = String::from("fx-1");
        assert!(id == "fx-1" && id == *"fx-1" && id == text);
        assert!(id.starts_with("fx-"));
        assert_eq!(json(&HoldId::from(String::from("fx-h9"))), "\"fx-h9\"");
    }

    #[test]
    fn a_read_proof_says_whether_a_token_came_back() {
        let proof = ReadProof::of("{\"token\":\"t-1\"}");
        assert!(proof.carries("t-1"));
        assert!(!proof.carries("t-2"));
    }

    #[test]
    fn a_priority_above_four_is_refused() {
        let item = |priority| NewItem {
            title: String::from("t"),
            item_type: String::from("task"),
            priority,
            ..NewItem::default()
        };
        assert_eq!(
            item(Some(5)).validate(),
            Err(String::from("priority is 5; the range is 0 to 4"))
        );
        assert_eq!(item(Some(4)).validate(), Ok(()));
        assert_eq!(item(None).validate(), Ok(()));
        assert!(!json(&item(None)).contains("priority"));
    }

    #[test]
    fn a_refusal_names_its_reason_in_snake_case() {
        let refusal = Refusal {
            reason: RefusalReason::Ambiguous,
            message: String::from("`a` matches more than one item"),
            candidates: vec![ItemId::from("fx-a1"), ItemId::from("fx-a2")],
        };
        let written = json(&refusal);
        assert!(written.contains(r#""reason":"ambiguous""#), "{written}");
        assert_eq!(serde_json::from_str::<Refusal>(&written).unwrap(), refusal);
    }

    #[test]
    fn capabilities_read_as_none_where_a_store_says_nothing() {
        let read: Capabilities = serde_json::from_str("{}").unwrap();
        assert_eq!(read, Capabilities::default());
    }

    // ---- 9: the store's documentation ----

    /// docs/store.md prints the contract's JSON, and this arm holds every
    /// example there to the types: the Item byte for byte, as the arm above
    /// holds it, each other shape as the types write it, and the envelope's
    /// answer as [`answer`] reads it.
    ///
    /// RED-PROOF: one byte of one example changed in the doc fails this arm,
    /// naming the example it no longer finds.
    #[test]
    fn the_store_doc_prints_every_example_as_the_types_write_it() {
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/store.md"))
            .expect("docs/store.md reads");

        let mut show = serde_json::Map::new();
        show.insert(String::from("id"), serde_json::Value::from("a1b2"));
        let refusal = Refusal {
            reason: RefusalReason::Ambiguous,
            message: String::from("a1 matches more than one item"),
            candidates: vec![ItemId::from("fx-a1b2"), ItemId::from("fx-a1c9")],
        };
        let summary = ItemSummary {
            id: ItemId::from("fx-c3d4"),
            title: String::from("Name the stamp's fields"),
            status: Status::Open,
            item_type: String::from("task"),
            labels: vec![String::from("fleet")],
            order: OrderState::None,
        };
        let new_item = NewItem {
            title: String::from("Name the stamp's fields"),
            description: String::from("Each field has a range."),
            item_type: String::from("task"),
            labels: vec![String::from("fleet")],
            priority: Some(2),
        };
        let run = RunRecord {
            hash: String::from("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
            workflow: String::from("build"),
            pack: String::from("ts"),
            entry: String::from("workflows/build.ts"),
            started_at: stamp("2026-09-23T10:00:00Z"),
        };
        let exporting = Capabilities {
            export: Some(ExportSpec {
                file: String::from(".beads/issues.jsonl"),
                dir: String::from(".beads/"),
            }),
            scratch: true,
            item_prefix: Some(String::from("fx")),
        };
        let review = Order {
            kind: OrderKind::Review,
            ..an_order(None)
        };

        let mut examples = vec![
            ITEM_EXAMPLE.to_string(),
            json(&request(show, Path::new("/work/project"))),
            format!(r#"{{"schema_version":1,"refused":{}}}"#, json(&refusal)),
            json(&HoldId::from("fx-h9")),
            json(&Status::from("deferred")),
            json(&stamp("2026-09-23T10:00:00Z")),
            json(&Actor::seat(seat(PERSON))),
            json(&an_order(Some(seat(SEAT)))),
            json(&review),
            json(&OrderState::None),
            json(&OrderState::Unreadable),
            json(&OrderState::Ordered(an_order(Some(seat(SEAT))))),
            json(&run),
            json(&summary),
            json(&Filter::Ready),
            json(&Filter::Label(String::from("fleet"))),
            json(&Filter::Assignee(seat(SEAT))),
            json(&new_item),
            json(&Update::title(String::from("Name the stamp's fields"))),
            json(&Update::assignee(seat(SEAT))),
            json(&Update::unassigned()),
            json(&exporting),
            json(&Capabilities::default()),
            json(&Version {
                name: String::from("tracker"),
                version: String::from("0.4.0"),
            }),
            json(&Resolved {
                id: ItemId::from("fx-a1b2"),
            }),
            json(&Appended {
                entry: String::from("e-17"),
            }),
            json(&Raised {
                hold: HoldId::from("fx-h9"),
            }),
            json(&OpenHolds {
                holds: vec![HoldId::from("fx-h9")],
            }),
            json(&Exported {
                file: String::from("/work/lane/store/export.jsonl"),
            }),
            json(&Scratched {
                root: String::from("/tmp/fleet-scratch/store"),
            }),
        ];

        // The whole answer the envelope section prints, read as `resolve`'s.
        let answered = r#"{"schema_version":1,"id":"fx-a1b2"}"#;
        assert_eq!(
            answer::<Resolved>(answered),
            Ok(Resolved {
                id: ItemId::from("fx-a1b2")
            })
        );
        examples.push(answered.to_string());

        // THE CONTROLS: the capability example's export keeps the rules the
        // doc states, and `{}` reads as the store that declares nothing.
        assert_eq!(
            exporting.export.as_ref().map(ExportSpec::validate),
            Some(Ok(()))
        );
        assert_eq!(
            serde_json::from_str::<Capabilities>("{}").unwrap(),
            Capabilities::default()
        );

        let missing: Vec<&String> = examples
            .iter()
            .filter(|example| !doc.contains(example.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "docs/store.md does not print these examples verbatim: {missing:#?}"
        );
    }
}
