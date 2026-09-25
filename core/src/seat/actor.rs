//! Who acts: a typed reference, `<kind>:<id>`, that every writing verb carries.
//!
//! FOUR KINDS AND NO BARE NAMES. A seat acts as its full id; a run, a routine
//! and the controller each act under an id of their own. The string form is
//! what the store is handed and what a record shows, and it is one fact: the
//! text a writer wrote is the text [`Actor::typed`] reads back, so a check
//! decides by kind and never by comparing a name somebody chose.
//!
//! Only the types live here. Resolving `--by` and `FLEET_ACTOR` into one of
//! these is the cli's, over the roster [`super::identity`] reads.

use std::fmt;

use super::identity::{SeatId, SEAT_ID_PATTERN};

/// What an actor is, spelled once. The word is the string form's prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActorKind {
    Seat,
    Run,
    Routine,
    Controller,
}

impl ActorKind {
    const ALL: [ActorKind; 4] = [
        ActorKind::Seat,
        ActorKind::Run,
        ActorKind::Routine,
        ActorKind::Controller,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ActorKind::Seat => "seat",
            ActorKind::Run => "run",
            ActorKind::Routine => "routine",
            ActorKind::Controller => "controller",
        }
    }
}

/// One actor. A seat's id is its full [`SeatId`] in canonical lowercase; every
/// other kind's is the id it was given.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Actor {
    pub kind: ActorKind,
    pub id: String,
}

impl Actor {
    /// The text read as a typed actor.
    ///
    /// `None` is text that does not open with one of the four `<kind>:`
    /// prefixes — it is not a typed actor at all, and the caller reads it as
    /// a seat argument instead. `Some(Err)` is text that does, with an id the
    /// kind does not take: a seat's must be a whole seat id, and every other
    /// kind's non-empty with no whitespace in it.
    pub fn typed(text: &str) -> Option<Result<Actor, String>> {
        let (kind, id) = ActorKind::ALL.iter().find_map(|kind| {
            text.strip_prefix(kind.as_str())
                .and_then(|rest| rest.strip_prefix(':'))
                .map(|id| (*kind, id))
        })?;
        let checked = match kind {
            ActorKind::Seat => SeatId::parse(id).map(|seat| seat.to_string()),
            _ if id.is_empty() => Err(String::from("the id is empty")),
            _ if id.chars().any(char::is_whitespace) => {
                Err(format!("the id `{id}` carries whitespace"))
            }
            _ => Ok(id.to_string()),
        };
        Some(
            checked
                .map(|id| Actor { kind, id })
                .map_err(|why| format!("`{text}` is a typed actor with a bad id — {why}")),
        )
    }

    pub fn seat(id: SeatId) -> Actor {
        Actor {
            kind: ActorKind::Seat,
            id: id.to_string(),
        }
    }

    /// The seat this actor is, and `None` for every other kind.
    pub fn seat_id(&self) -> Option<SeatId> {
        match self.kind {
            ActorKind::Seat => SeatId::parse(&self.id).ok(),
            _ => None,
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.id)
    }
}

impl serde::Serialize for Actor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Actor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Actor, D::Error> {
        let text = String::deserialize(deserializer)?;
        match Actor::typed(&text) {
            Some(read) => read.map_err(serde::de::Error::custom),
            None => Err(serde::de::Error::custom(format!(
                "`{text}` is not a typed actor — say seat:, run:, routine: or controller: and its id"
            ))),
        }
    }
}

/// An actor on the wire is its one string, which the derive cannot see
/// through the hand-written serde above: a seat's id whole, and any other
/// kind's non-empty with no whitespace, as [`Actor::typed`] reads them.
impl schemars::JsonSchema for Actor {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Actor".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": format!("^(seat:{SEAT_ID_PATTERN}|(run|routine|controller):\\S+)$"),
        })
    }
}
