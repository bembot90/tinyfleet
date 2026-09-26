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
//! `"assignee": null` clears it; a write that leaves `if_assignee` out is
//! fenced on nobody, and one that says `"if_assignee": null` lands only on an
//! item nobody holds — so those fields are read and written by hand, and never
//! by the derive that would fold the two together.

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::path::Path;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
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
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize, JsonSchema,
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

/// Any string, with the three fleet acts on named: the derive would describe
/// the enum's variants, and the wire carries the text `from` and `into` make.
impl JsonSchema for Status {
    fn schema_name() -> Cow<'static, str> {
        "Status".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "anyOf": [
                {"enum": ["open", "in_progress", "closed"]},
                {"type": "string"},
            ],
        })
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

/// The text [`Stamp::parse`] takes, as one pattern: each field its digits,
/// and a month, day, hour, minute and second each in the range it holds.
pub(crate) const STAMP_PATTERN: &str =
    "^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9]Z$";

impl JsonSchema for Stamp {
    fn schema_name() -> Cow<'static, str> {
        "Stamp".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": STAMP_PATTERN,
        })
    }
}

// ---- the order ------------------------------------------------------------------

/// What an order asks of the seat it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    Dispatch,
    Review,
}

impl OrderKind {
    /// The word the JSON spells it with, for a line that names the kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderKind::Dispatch => "dispatch",
            OrderKind::Review => "review",
        }
    }
}

/// The order an item stands under: what it asks, who gave it, the seat it is
/// for where one is named, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

    /// The text itself, for a suite comparing one read with the next and for
    /// a message quoting it — never for a verb to decide on, which reads the
    /// item's own fields.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ---- items ----------------------------------------------------------------------

/// One item as a read answers it: its id, title, description, status and
/// type, its own labels, who holds it, its order, what blocks it, a run's
/// record and the names of the keys another writer keeps on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub id: ItemId,
    /// What a person calls this item. `land` writes it into the commit subject,
    /// so a trunk's log reads as a list of what was done and not of ids.
    pub title: String,
    /// What the item says: `""` where it says nothing, which an answer spells
    /// by omitting the key. `item show` renders it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub status: Status,
    /// The type, as the store spells it: a rule matches on this value.
    #[serde(rename = "type")]
    pub item_type: String,
    /// The item's OWN labels and no parent's, which is what the store answers.
    #[serde(default)]
    pub labels: Vec<String>,
    /// The seat holding the item, or `None` where nobody does: the store OMITS
    /// a key it has no value for, and an absent field must not read as a
    /// disagreement.
    ///
    /// A SEAT AND NEVER A PERSON. A board a project brought may name a person
    /// here, and that read refuses, naming the item and its holder: an item a
    /// person holds that read as `None` would be dispatched as held by nobody.
    #[serde(default)]
    pub assignee: Option<SeatId>,
    /// The order index: none, one the store holds and this fleet cannot read,
    /// or the order — one value, so an order that is there and unread cannot
    /// be built as absent.
    #[serde(default)]
    pub order: OrderState,
    /// The open items that block this one by a type the store's ready set
    /// honours.
    #[serde(default)]
    pub blockers: Vec<ItemId>,
    /// A run's record, on the item that records it. One the store holds at a
    /// shape this fleet does not read is no item at all: the read refuses.
    #[serde(default)]
    pub run: Option<RunRecord>,
    /// The names of the keys the store holds on the item that are not
    /// fleet's: another tool's, named so a person adopting the board can map
    /// them. Never what they hold, which fleet neither reads nor writes.
    #[serde(default)]
    pub foreign: Vec<String>,
    /// The whole text the read answered. The negative control asks it, so the
    /// control asks the SAME answer for a token nothing wrote.
    #[serde(skip)]
    pub proof: ReadProof,
}

/// One item as a listing answers it: enough to choose from without reading
/// each one, its holder and its run's record among it — so a listing is one
/// call, and never one more per row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ItemSummary {
    pub id: ItemId,
    pub title: String,
    pub status: Status,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub labels: Vec<String>,
    /// The seat holding the item, or `None` where nobody does, read as
    /// [`Item::assignee`] is: a row a person holds refuses the listing, which
    /// never answers it as held by nobody.
    #[serde(default)]
    pub assignee: Option<SeatId>,
    #[serde(default)]
    pub order: OrderState,
    /// A run's record, read as [`Item::run`] is: a record this fleet does not
    /// read refuses the listing, which never answers it as carrying none.
    #[serde(default)]
    pub run: Option<RunRecord>,
    /// The store's keys on the item that are not fleet's, as [`Item`] names
    /// them.
    #[serde(default)]
    pub foreign: Vec<String>,
}

/// Which items a listing asks for: the ready ones, those carrying a label, or
/// those a seat holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    Ready,
    Label(String),
    Assignee(SeatId),
}

/// An item to file: the store names it, so there is no id here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NewItem {
    pub title: String,
    pub description: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(max = PRIORITY_MAX))]
    pub priority: Option<u8>,
}

/// The highest priority the contract carries. A store declares its own range
/// inside 0 to this, so no store is sent a priority past it.
pub const PRIORITY_MAX: u8 = 4;

impl NewItem {
    /// The item, or the reason no store may be sent it: a priority outside
    /// the contract's 0 to 4. A type, and a priority inside the range one
    /// store takes, are that store's to answer: [`Vocabulary::takes`].
    pub fn validate(&self) -> Result<(), String> {
        match self.priority {
            Some(n) if n > PRIORITY_MAX => {
                Err(format!("priority is {n}; the range is 0 to {PRIORITY_MAX}"))
            }
            _ => Ok(()),
        }
    }
}

/// A change to one item: each field is left alone where it is absent, and
/// `"assignee": null` is the item handed to nobody.
///
/// `if_assignee` is the FENCE, and changes nothing: where it is present the
/// change lands only while that seat holds the item — `null` for nobody — and
/// is otherwise refused as moved, with nothing written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "assignee_out",
        deserialize_with = "assignee_in"
    )]
    pub if_assignee: Option<Option<SeatId>>,
    /// The status the item is set to, which is `open` or nothing: the reopen,
    /// which brings an item nobody is working back to the ready set. Any other
    /// status is the caller's mistake, refused before the store is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "reopened")]
    pub status: Option<Status>,
}

/// An update's `status`, which is `"open"` and nothing else: the schema says
/// what [`super::writable`] lets through to a store, not all a `Status` reads.
fn reopened(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"const": "open"})
}

/// A present `assignee` or `if_assignee` as its JSON: the seat's id, or `null`
/// for nobody. An absent one never reaches here, because the key is skipped.
fn assignee_out<S: Serializer>(
    assignee: &Option<Option<SeatId>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match assignee {
        Some(Some(seat)) => serializer.serialize_some(seat),
        Some(None) | None => serializer.serialize_none(),
    }
}

/// A present `assignee` or `if_assignee` key, `null` included, as `Some`: only
/// a key that is not there at all is left to `default` as `None`.
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

    /// No change yet, fenced on `from` holding the item — `None` for nobody —
    /// for the caller to name what the fenced write moves.
    pub fn fenced(from: Option<SeatId>) -> Update {
        Update {
            if_assignee: Some(from),
            ..Update::default()
        }
    }

    /// Whether this changes nothing at all: a fence alone is no change.
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.assignee.is_none() && self.status.is_none()
    }
}

/// What an order's withdrawal is fenced on, and whether it reopens the item:
/// sent as `order.withdraw`'s own fields, each left out where it is unset, so
/// the default is the plain withdrawal.
///
/// A fence that is present is one the item must meet — held by `if_assignee`
/// (`null` for nobody), reading `if_status` — or the withdrawal is refused as
/// moved, with nothing written. `reopen` sets the status to `open` in the SAME
/// act that clears the assignee and takes the order away: an item left
/// `in_progress` with nobody holding it is out of the ready set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WithdrawFence {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "assignee_out",
        deserialize_with = "assignee_in"
    )]
    pub if_assignee: Option<Option<SeatId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_status: Option<Status>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reopen: bool,
}

// ---- what a store can do --------------------------------------------------------

/// What a store says it can do beyond the verbs every store answers: an
/// export fleet commits, a scratch board for a suite, the prefix its ids
/// carry, the command a seat types against it, and the types and priorities
/// its items take.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Capabilities {
    #[serde(default)]
    pub export: Option<ExportSpec>,
    #[serde(default)]
    pub scratch: bool,
    #[serde(default)]
    pub item_prefix: Option<String>,
    /// The first word of the command a seat types in its shell to reach this
    /// store, which the guards police: `None` for a store no seat reaches
    /// from a shell.
    #[serde(default)]
    pub cli: Option<String>,
    /// Absent, the contract's own [`Vocabulary::default`].
    #[serde(default)]
    pub items: Vocabulary,
}

impl Capabilities {
    /// The declaration, or the field and value that break it: the export
    /// [`ExportSpec::validate`] holds, a `cli` that is one word with no path
    /// in it, and the items [`Vocabulary::validate`] holds.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(export) = &self.export {
            export.validate()?;
        }
        if let Some(cli) = &self.cli {
            if cli.is_empty() || cli.contains('/') || cli.chars().any(char::is_whitespace) {
                return Err(format!(
                    "cli `{cli}` is not one word — it is the name a seat types, with no path"
                ));
            }
        }
        self.items.validate()
    }
}

/// The types a store files an item under and the priorities it takes: what a
/// routine's item is held to before the store is sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Vocabulary {
    #[serde(default = "default_types")]
    pub types: Vec<String>,
    #[serde(default)]
    pub priority: Priorities,
}

/// The lowest and the highest priority a store takes, both inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Priorities {
    #[schemars(range(max = PRIORITY_MAX))]
    pub min: u8,
    #[schemars(range(max = PRIORITY_MAX))]
    pub max: u8,
}

/// The types of a store that declares none: an adapter that answers no
/// `items` is sent a routine's item of one of these six and of no other.
pub const DEFAULT_TYPES: [&str; 6] = ["bug", "feature", "task", "epic", "chore", "decision"];

fn default_types() -> Vec<String> {
    DEFAULT_TYPES.iter().map(|word| word.to_string()).collect()
}

impl Default for Vocabulary {
    fn default() -> Self {
        Vocabulary {
            types: default_types(),
            priority: Priorities::default(),
        }
    }
}

/// The contract's whole range, 0 to [`PRIORITY_MAX`].
impl Default for Priorities {
    fn default() -> Self {
        Priorities {
            min: 0,
            max: PRIORITY_MAX,
        }
    }
}

impl Vocabulary {
    /// The vocabulary, or the field and value that break it: at least one
    /// type and none empty, and a priority range that is not upside down and
    /// ends inside the contract's 0 to 4.
    pub fn validate(&self) -> Result<(), String> {
        if self.types.is_empty() {
            return Err(String::from(
                "items types is empty — a store takes at least one",
            ));
        }
        if self.types.iter().any(String::is_empty) {
            return Err(String::from("items types carries an empty type"));
        }
        let Priorities { min, max } = self.priority;
        if min > max {
            return Err(format!("items priority min {min} is above max {max}"));
        }
        if max > PRIORITY_MAX {
            return Err(format!(
                "items priority max {max} is past {PRIORITY_MAX}, the contract's highest"
            ));
        }
        Ok(())
    }

    /// The item, or why this store must not be sent it: a type it does not
    /// declare, or a priority outside its range.
    pub fn takes(&self, item: &NewItem) -> Result<(), String> {
        if !self.types.contains(&item.item_type) {
            return Err(format!(
                "type is `{}`; the store takes {}",
                item.item_type,
                self.types.join(", ")
            ));
        }
        match item.priority {
            Some(n) if n < self.priority.min || n > self.priority.max => Err(format!(
                "priority is {n}; the store takes {} to {}",
                self.priority.min, self.priority.max
            )),
            _ => Ok(()),
        }
    }
}

/// Where a store's export lands, relative to the project root: the file, and
/// the directory it sits in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Version {
    pub name: String,
    pub version: String,
}

// ---- refusals -------------------------------------------------------------------

/// The store's answer that the act cannot be done as asked, and why — the
/// record's answer, not a store that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Refusal {
    pub reason: RefusalReason,
    pub message: String,
    #[serde(default)]
    pub candidates: Vec<ItemId>,
}

/// Why a store refused: nothing by that id, more than one item it could mean,
/// the act already done, or a fence the item no longer meets — whose message
/// names what holds the item instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Missing,
    Ambiguous,
    Already,
    Moved,
}

// ---- response bodies ------------------------------------------------------------
//
// The timeline's answer is not one of these: its entries are
// [`crate::entry`]'s, read and folded there.

/// The one item a partial id names, whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Resolved {
    pub id: ItemId,
}

/// One item, read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Shown {
    pub item: Item,
}

/// The items a filter matched, in the store's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Listed {
    pub items: Vec<ItemSummary>,
}

/// The id the store gave an item it filed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Created {
    pub id: ItemId,
}

/// The id the store gave an entry it appended to an item's timeline, as text:
/// the same id [`crate::entry::Entry`] carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Appended {
    pub entry: String,
}

/// The id of a hold the store raised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Raised {
    pub hold: HoldId,
}

/// Every hold the store still calls open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OpenHolds {
    pub holds: Vec<HoldId>,
}

/// The export file the store wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Exported {
    pub file: String,
}

/// The root of a scratch board the store made for a suite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Scratched {
    pub root: String,
}

/// A write the store took, which answers nothing more.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
  "description": "The stamp gains a seconds field.",
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
  "run": null,
  "foreign": [
    "sprint"
  ]
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

    /// RED-PROOF: a derived `if_assignee` reads `null` as no fence, which is a
    /// write landing on an item somebody holds.
    #[test]
    fn a_null_fence_is_nobody_holding_and_an_absent_one_no_fence() {
        let unheld: Update =
            serde_json::from_str(r#"{"if_assignee":null,"status":"open"}"#).unwrap();
        assert_eq!(unheld.if_assignee, Some(None));
        assert_eq!(unheld.status, Some(Status::Open));
        assert_eq!(json(&unheld), r#"{"if_assignee":null,"status":"open"}"#);

        let unfenced: Update = serde_json::from_str(r#"{"status":"open"}"#).unwrap();
        assert_eq!(unfenced.if_assignee, None);

        let held = Update {
            assignee: Some(None),
            ..Update::fenced(Some(seat(SEAT)))
        };
        assert_eq!(
            json(&held),
            format!(r#"{{"assignee":null,"if_assignee":"{SEAT}"}}"#)
        );
        assert_eq!(serde_json::from_str::<Update>(&json(&held)).unwrap(), held);
    }

    #[test]
    fn a_fence_alone_changes_nothing() {
        assert!(Update::fenced(None).is_empty());
        assert!(Update::fenced(Some(seat(SEAT))).is_empty());
        let reopened = Update {
            status: Some(Status::Open),
            ..Update::fenced(None)
        };
        assert!(!reopened.is_empty());
    }

    /// The plain withdrawal sends no field beyond its id and actor, so an
    /// adapter that knows no fence answers it as it always did.
    #[test]
    fn a_withdrawal_fence_sends_only_what_it_sets() {
        assert_eq!(json(&WithdrawFence::default()), "{}");
        assert_eq!(
            serde_json::from_str::<WithdrawFence>("{}").unwrap(),
            WithdrawFence::default()
        );
        let retire = WithdrawFence {
            if_assignee: Some(Some(seat(SEAT))),
            if_status: Some(Status::InProgress),
            reopen: true,
        };
        let written = json(&retire);
        assert_eq!(
            written,
            format!(r#"{{"if_assignee":"{SEAT}","if_status":"in_progress","reopen":true}}"#)
        );
        assert_eq!(
            serde_json::from_str::<WithdrawFence>(&written).unwrap(),
            retire
        );
        let unheld: WithdrawFence = serde_json::from_str(r#"{"if_assignee":null}"#).unwrap();
        assert_eq!(unheld.if_assignee, Some(None));
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
        assert_eq!(read.description, "The stamp gains a seconds field.");
        assert_eq!(read.status, Status::InProgress);
        assert_eq!(read.assignee, Some(seat(SEAT)));
        assert_eq!(read.order, OrderState::Ordered(an_order(Some(seat(SEAT)))));
        assert_eq!(read.blockers, vec![ItemId::from("fx-c3d4")]);
        assert_eq!(read.foreign, vec![String::from("sprint")]);
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

    /// A row with no `assignee` and no `run` — the row an adapter writes
    /// that answers neither — reads as held by nobody and carrying no record,
    /// and a row carrying both reads them.
    #[test]
    fn a_summary_read_without_its_holder_or_run_takes_their_defaults() {
        let bare = r#"{"id":"fx-c3d4","title":"t","status":"open","type":"task","labels":[],"order":{"state":"none"}}"#;
        let read: ItemSummary = serde_json::from_str(bare).unwrap();
        assert_eq!(read.assignee, None);
        assert_eq!(read.run, None);

        let held = bare.replacen(
            r#""order""#,
            &format!(
                r#""assignee":"{SEAT}","run":{},"order""#,
                r#"{"hash":"h1","workflow":"build","pack":"ts","entry":"build.ts","started_at":"2026-09-23T10:00:00Z"}"#
            ),
            1,
        );
        let read: ItemSummary = serde_json::from_str(&held).unwrap();
        assert_eq!(read.assignee, Some(seat(SEAT)));
        assert_eq!(read.run.map(|run| run.hash), Some(String::from("h1")));
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
        assert_eq!(read.cli, None);
        assert_eq!(read.items.types, DEFAULT_TYPES);
        assert_eq!(read.items.priority, Priorities { min: 0, max: 4 });
        assert_eq!(read.validate(), Ok(()));

        // An adapter answering neither `cli` nor `items` reads as the
        // contract's vocabulary, and one naming its types alone
        // takes the contract's whole range.
        let before: Capabilities =
            serde_json::from_str(r#"{"export":null,"scratch":true,"item_prefix":"fx"}"#).unwrap();
        assert_eq!(before.items, Vocabulary::default());
        let typed: Capabilities = serde_json::from_str(r#"{"items":{"types":["story"]}}"#).unwrap();
        assert_eq!(typed.items.types, ["story"]);
        assert_eq!(typed.items.priority, Priorities::default());
    }

    #[test]
    fn a_declaration_with_no_types_an_upside_down_range_or_a_path_for_a_cli_is_refused() {
        let items = |types: &[&str], min, max| Capabilities {
            items: Vocabulary {
                types: types.iter().map(|word| word.to_string()).collect(),
                priority: Priorities { min, max },
            },
            ..Capabilities::default()
        };
        let cli = |word: &str| Capabilities {
            cli: Some(word.to_string()),
            ..Capabilities::default()
        };
        for (refused, names) in [
            (items(&[], 0, 4), "items types is empty"),
            (
                items(&["task", ""], 0, 4),
                "items types carries an empty type",
            ),
            (
                items(&["task"], 3, 1),
                "items priority min 3 is above max 1",
            ),
            (items(&["task"], 0, 5), "items priority max 5 is past 4"),
            (cli(""), "cli `` is not one word"),
            (
                cli("/usr/local/bin/tk"),
                "cli `/usr/local/bin/tk` is not one word",
            ),
            (cli("tk item"), "cli `tk item` is not one word"),
            (
                Capabilities {
                    export: Some(ExportSpec {
                        file: String::from("/abs/x"),
                        dir: String::from("/abs/"),
                    }),
                    ..Capabilities::default()
                },
                "export file `/abs/x` is absolute",
            ),
        ] {
            let why = refused.validate().unwrap_err();
            assert!(why.starts_with(names), "{why}");
        }

        // THE CONTROLS: one type, a range of one, and a bare word are kept.
        assert_eq!(items(&["story"], 2, 2).validate(), Ok(()));
        assert_eq!(cli("tk").validate(), Ok(()));
    }

    #[test]
    fn a_store_takes_the_types_it_declares_and_the_priorities_in_its_range() {
        let declared = Vocabulary {
            types: vec![String::from("story"), String::from("task")],
            priority: Priorities { min: 1, max: 3 },
        };
        let item = |item_type: &str, priority| NewItem {
            title: String::from("t"),
            item_type: item_type.to_string(),
            priority,
            ..NewItem::default()
        };
        assert_eq!(declared.takes(&item("story", Some(1))), Ok(()));
        assert_eq!(declared.takes(&item("task", None)), Ok(()));
        assert_eq!(
            declared.takes(&item("bug", None)),
            Err(String::from("type is `bug`; the store takes story, task"))
        );
        assert_eq!(
            declared.takes(&item("story", Some(0))),
            Err(String::from("priority is 0; the store takes 1 to 3"))
        );
        assert_eq!(
            declared.takes(&item("story", Some(4))),
            Err(String::from("priority is 4; the store takes 1 to 3"))
        );
    }

    // ---- 9: the store's documentation ----

    /// One JSON example docs/store.md prints, and the message of the contract
    /// it sits in: the schema in the contract's document that takes it, and
    /// the whole request or answer it is part of there.
    struct Documented {
        text: String,
        schema: &'static str,
        message: serde_json::Value,
    }

    /// The example `text`, and the message `place` builds around it for the
    /// schema at `schema`.
    fn documented(
        text: String,
        schema: &'static str,
        place: fn(serde_json::Value) -> serde_json::Value,
    ) -> Documented {
        let example = serde_json::from_str(&text).expect("a documented example is JSON");
        Documented {
            message: place(example),
            text,
            schema,
        }
    }

    /// The example as it stands: the whole message, or a type of its own.
    fn whole(example: serde_json::Value) -> serde_json::Value {
        example
    }

    /// `fields` merged into `into`.
    fn merged(mut into: serde_json::Value, fields: serde_json::Value) -> serde_json::Value {
        if let (serde_json::Value::Object(into), serde_json::Value::Object(fields)) =
            (&mut into, fields)
        {
            into.extend(fields);
        }
        into
    }

    /// A request's envelope, with the item and the actor the writes name.
    fn sent(fields: serde_json::Value) -> serde_json::Value {
        let envelope = serde_json::json!({
            "schema_version": 1, "root": "/work/project",
            "id": "fx-a1b2", "by": format!("seat:{PERSON}"),
        });
        merged(envelope, fields)
    }

    /// An answer: the example's fields beside `schema_version`.
    fn answered(fields: serde_json::Value) -> serde_json::Value {
        merged(serde_json::json!({"schema_version": 1}), fields)
    }

    /// Every JSON example docs/store.md prints of the contract's types, each
    /// as the types write it, placed in its verb's message.
    fn documented_examples() -> Vec<Documented> {
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
            assignee: None,
            order: OrderState::None,
            run: None,
            foreign: vec![String::from("sprint")],
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
                file: String::from(".tracker/items.jsonl"),
                dir: String::from(".tracker/"),
            }),
            scratch: true,
            item_prefix: Some(String::from("fx")),
            cli: Some(String::from("tracker")),
            items: Vocabulary {
                types: vec![String::from("task"), String::from("bug")],
                priority: Priorities { min: 0, max: 4 },
            },
        };
        let review = Order {
            kind: OrderKind::Review,
            ..an_order(None)
        };
        let moved = Refusal {
            reason: RefusalReason::Moved,
            message: format!("fx-a1b2 is held by {SEAT}"),
            candidates: Vec::new(),
        };
        let retire = WithdrawFence {
            if_assignee: Some(Some(seat(SEAT))),
            if_status: Some(Status::InProgress),
            reopen: true,
        };

        let item = |item| serde_json::json!({"schema_version": 1, "item": item});
        let row = |row| serde_json::json!({"schema_version": 1, "items": [row]});
        let filter = |filter| serde_json::json!({"schema_version": 1, "root": "/work/project", "filter": filter});
        let filed = |item| sent(serde_json::json!({"item": item}));
        let ordered = |order| sent(serde_json::json!({"order": order}));
        let ran = |run| sent(serde_json::json!({"run": run}));
        let cleared = |hold| sent(serde_json::json!({"hold": hold}));
        vec![
            documented(ITEM_EXAMPLE.to_string(), "/verbs/show/response", item),
            documented(
                json(&request(show, Path::new("/work/project"))),
                "/verbs/show/request",
                whole,
            ),
            documented(
                format!(r#"{{"schema_version":1,"refused":{}}}"#, json(&refusal)),
                "/refusal",
                whole,
            ),
            documented(
                format!(r#"{{"schema_version":1,"refused":{}}}"#, json(&moved)),
                "/refusal",
                whole,
            ),
            documented(
                String::from(r#"{"schema_version":1,"error":"the database is locked"}"#),
                "/error",
                whole,
            ),
            documented(
                json(&HoldId::from("fx-h9")),
                "/verbs/hold.clear/request",
                cleared,
            ),
            documented(json(&Status::from("deferred")), "/$defs/Status", whole),
            documented(json(&stamp("2026-09-23T10:00:00Z")), "/$defs/Stamp", whole),
            documented(json(&Actor::seat(seat(PERSON))), "/$defs/Actor", whole),
            documented(
                json(&an_order(Some(seat(SEAT)))),
                "/verbs/order.set/request",
                ordered,
            ),
            documented(json(&review), "/verbs/order.set/request", ordered),
            documented(json(&OrderState::None), "/$defs/OrderState", whole),
            documented(json(&OrderState::Unreadable), "/$defs/OrderState", whole),
            documented(
                json(&OrderState::Ordered(an_order(Some(seat(SEAT))))),
                "/$defs/OrderState",
                whole,
            ),
            documented(json(&run), "/verbs/run.set/request", ran),
            documented(json(&summary), "/verbs/list/response", row),
            documented(json(&Filter::Ready), "/verbs/list/request", filter),
            documented(
                json(&Filter::Label(String::from("fleet"))),
                "/verbs/list/request",
                filter,
            ),
            documented(
                json(&Filter::Assignee(seat(SEAT))),
                "/verbs/list/request",
                filter,
            ),
            documented(json(&new_item), "/verbs/create/request", filed),
            documented(
                json(&Update::title(String::from("Name the stamp's fields"))),
                "/verbs/update/request",
                sent,
            ),
            documented(
                json(&Update::assignee(seat(SEAT))),
                "/verbs/update/request",
                sent,
            ),
            documented(json(&Update::unassigned()), "/verbs/update/request", sent),
            documented(
                json(&Update {
                    assignee: Some(Some(seat(SEAT))),
                    ..Update::fenced(None)
                }),
                "/verbs/update/request",
                sent,
            ),
            documented(
                json(&Update {
                    status: Some(Status::Open),
                    ..Update::fenced(None)
                }),
                "/verbs/update/request",
                sent,
            ),
            documented(json(&retire), "/verbs/order.withdraw/request", sent),
            documented(json(&exporting), "/verbs/capabilities/response", answered),
            documented(
                json(&Capabilities::default()),
                "/verbs/capabilities/response",
                answered,
            ),
            documented(
                json(&Version {
                    name: String::from("tracker"),
                    version: String::from("0.4.0"),
                }),
                "/verbs/version/response",
                answered,
            ),
            documented(
                json(&Resolved {
                    id: ItemId::from("fx-a1b2"),
                }),
                "/verbs/resolve/response",
                answered,
            ),
            documented(
                json(&Appended {
                    entry: String::from("e-17"),
                }),
                "/verbs/append/response",
                answered,
            ),
            documented(
                json(&Raised {
                    hold: HoldId::from("fx-h9"),
                }),
                "/verbs/hold.raise/response",
                answered,
            ),
            documented(
                json(&OpenHolds {
                    holds: vec![HoldId::from("fx-h9")],
                }),
                "/verbs/holds.open/response",
                answered,
            ),
            documented(
                json(&Exported {
                    file: String::from("/work/lane/store/export.jsonl"),
                }),
                "/verbs/export/response",
                answered,
            ),
            documented(
                json(&Scratched {
                    root: String::from("/tmp/fleet-scratch/store"),
                }),
                "/verbs/scratch/response",
                answered,
            ),
            // The whole answer the envelope section prints, read as `resolve`'s
            // by the arm below.
            documented(
                String::from(r#"{"schema_version":1,"id":"fx-a1b2"}"#),
                "/verbs/resolve/response",
                whole,
            ),
        ]
    }

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

        assert_eq!(
            answer::<Resolved>(r#"{"schema_version":1,"id":"fx-a1b2"}"#),
            Ok(Resolved {
                id: ItemId::from("fx-a1b2")
            })
        );

        // THE CONTROLS: the capability example keeps the rules the doc
        // states, and `{}` reads as the store that declares nothing.
        let exporting: Capabilities = serde_json::from_str(
            r#"{"export":{"file":".tracker/items.jsonl","dir":".tracker/"},"scratch":true,"item_prefix":"fx","cli":"tracker","items":{"types":["task","bug"],"priority":{"min":0,"max":4}}}"#,
        )
        .unwrap();
        assert_eq!(exporting.validate(), Ok(()));
        assert_eq!(
            serde_json::from_str::<Capabilities>("{}").unwrap(),
            Capabilities::default()
        );

        let examples = documented_examples();
        let missing: Vec<&String> = examples
            .iter()
            .map(|example| &example.text)
            .filter(|text| !doc.contains(text.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "docs/store.md does not print these examples verbatim: {missing:#?}"
        );
    }

    /// Each example the arm above finds in the doc, placed in its verb's
    /// request or answer, passes that verb's schema in the contract's
    /// document: what the doc shows an adapter author is what the schema they
    /// generate from takes.
    #[test]
    fn every_documented_example_passes_its_verbs_schema() {
        let document = super::super::schema::document();
        let failing: Vec<String> = documented_examples()
            .into_iter()
            .filter_map(|example| {
                super::super::schema::check(&document, example.schema, &example.message)
                    .err()
                    .map(|why| format!("{} at {}: {why}", example.text, example.schema))
            })
            .collect();
        assert!(
            failing.is_empty(),
            "documented examples the schema does not take: {failing:#?}"
        );
    }
}
