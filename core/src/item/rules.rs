//! The `flight.*` object an item declares, as one `[[core.flight.rules]]` entry
//! sets it: the shape `fleet status` prints each rule by.
//!
//! No verb applies a rule to an item: the page prints the array off the policy
//! in force, and that is the whole of what reads it.

/// The one key an item's object carries.
pub const KEYS: [&str; 1] = ["review"];

/// The object as one rule sets it: the same key, present only where the rule
/// filled it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effective {
    pub review: Option<String>,
}

impl Effective {
    /// The keys that were filled, in [`KEYS`] order, for a writer that renders
    /// them.
    pub fn rows(&self) -> Vec<(&'static str, &str)> {
        [("review", self.review.as_deref())]
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .collect()
    }
}
