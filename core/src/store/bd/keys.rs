//! The metadata keys fleet owns on a bd board, and the version each one's
//! object carries — named here once, for the adapter's readers and writers
//! and for nothing outside it: a verb hands the store an [`Order`] or a
//! [`RunRecord`], and which key holds it is bd's shape and not the verb's.
//!
//! A BOARD IS SHARED. A project that adds fleet may already keep its own
//! `orders` or `run` key, so fleet's keys carry its name and nothing reads a
//! bare one: an item holding no fleet key reads as no order and no run,
//! whatever else its metadata holds.
//!
//! DOTTED AND TOP-LEVEL, AND NEVER ONE NESTED `fleet` OBJECT. bd merges a
//! metadata write at the top level and replaces one key's object whole —
//! measured on bd 1.3.0, a nested `{"fleet":{"run":…}}` was lost to the next
//! `{"fleet":{"orders":…}}` write, where `fleet.run` and `fleet.orders` kept
//! each other — so [`Store::order_set`] and [`Store::run_set`] each write the
//! one key it owns, and neither ever erases the other.
//!
//! [`Order`]: crate::store::Order
//! [`RunRecord`]: crate::store::RunRecord
//! [`Store::order_set`]: crate::store::Store::order_set
//! [`Store::run_set`]: crate::store::Store::run_set

/// The order index a dispatch writes.
pub const ORDERS: &str = "fleet.orders";

/// A run's own object, on its record item.
pub const RUN: &str = "fleet.run";

/// The field each fleet key's object carries its version in.
pub const VERSION_FIELD: &str = "v";

/// The one version this binary writes and reads. A fleet key at any other, or
/// at none, is a shape this binary does not know, and a reader says so rather
/// than guess at it.
pub const VERSION: u64 = 1;

/// A fleet key's object, written with its version stamped in: the one shape
/// every writer hands the store, `{"<key>": {…, "v": 1}}`.
pub fn stamped(
    key: &str,
    mut object: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    object.insert(String::from(VERSION_FIELD), VERSION.into());
    serde_json::Value::Object(
        [(key.to_string(), serde_json::Value::Object(object))]
            .into_iter()
            .collect(),
    )
}

/// The object a fleet key holds, or the sentence that says why this binary
/// cannot read it: not an object, or at a version it does not know.
pub fn versioned<'a>(
    key: &str,
    held: &'a serde_json::Value,
) -> Result<&'a serde_json::Map<String, serde_json::Value>, String> {
    let Some(object) = held.as_object() else {
        return Err(format!("`{key}` holds something that is not an object"));
    };
    match object.get(VERSION_FIELD) {
        Some(found) if found.as_u64() == Some(VERSION) => Ok(object),
        Some(found) => Err(format!(
            "`{key}` carries {VERSION_FIELD} {found}, and this fleet reads {VERSION_FIELD} \
             {VERSION} and no other"
        )),
        None => Err(format!(
            "`{key}` carries no {VERSION_FIELD}, and this fleet reads {VERSION_FIELD} {VERSION} \
             and no other"
        )),
    }
}
