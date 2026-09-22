//! The `flight.*` object an item flies under, and the `[[core.flight.rules]]`
//! array that fills what the item did not write (flights PRD R4).
//!
//! A pure function over three values the caller read: the item's own type, its
//! own labels, and its own `metadata.flight`. IT IS HANDED THE ITEM'S OWN
//! LABELS AND NOTHING ELSE, which is how "never inherited from a parent" is
//! enforced rather than promised — a parent's labels cannot match a rule the
//! matcher never sees them at.
//!
//! A key no rule fills stays ABSENT. The snapshot writes only what was
//! decided, so the tick applies the policy default to an absent key rather
//! than reading a default this function invented and pinned as a decision.

use crate::policy;

/// The two keys an item's object carries (flights PRD R4).
pub const KEYS: [&str; 2] = ["review", "gate"];

/// The object as it stands after the rules ran: the same two keys, each
/// present only where the item or a rule filled it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effective {
    pub review: Option<String>,
    pub gate: Option<String>,
}

impl Effective {
    /// The keys that were filled, in [`KEYS`] order, for a writer that renders
    /// them.
    pub fn rows(&self) -> Vec<(&'static str, &str)> {
        [
            ("review", self.review.as_deref()),
            ("gate", self.gate.as_deref()),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect()
    }
}

/// One item, as the matcher reads it.
pub struct Subject<'a> {
    pub item_type: &'a str,
    /// The item's OWN labels.
    pub labels: &'a [String],
    /// `metadata.flight`, as the store holds it.
    pub own: Option<&'a serde_json::Value>,
}

/// The effective object for one item under one policy.
///
/// `[[core.flight.rules]]` is read through the census, so a policy that holds
/// the array under some other spelling is not read at all. A value of the
/// wrong shape — the array missing, an entry that is not a table — fills
/// nothing rather than refusing: the caller's own reader of the key decides
/// whether an unreadable policy is a stop, and this function's contract is the
/// object it can build.
pub fn effective(subject: &Subject, config: &toml::Table) -> Effective {
    let mut effective = Effective::default();
    for key in KEYS {
        set(&mut effective, key, own_value(subject.own, key));
    }

    for rule in rules_in(config) {
        if !matches(rule, subject) {
            continue;
        }
        for key in KEYS {
            if filled(&effective, key) {
                continue;
            }
            // FIRST MATCH WINS PER KEY: a rule that matched and says nothing
            // about a key leaves it for the next rule, because the rule the
            // item satisfies first is the one that decides what it declares
            // and not what it omits.
            set(&mut effective, key, string_in(rule, key));
        }
    }
    effective
}

/// The array, or nothing. An entry that is not a table is dropped: it declares
/// no match clause and no value, so there is nothing in it to apply.
fn rules_in(config: &toml::Table) -> Vec<&toml::Table> {
    let Ok(Some(value)) = policy::read("core.flight", "rules", config) else {
        return Vec::new();
    };
    value
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_table())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether one rule's match clause holds for this item.
///
/// Both halves are optional and both are the item's own. A rule with neither
/// matches every item, which is how a policy writes a fleet-wide default.
fn matches(rule: &toml::Table, subject: &Subject) -> bool {
    let Some(clause) = rule.get("match") else {
        return true;
    };
    let Some(clause) = clause.as_table() else {
        return false;
    };
    if let Some(wanted) = clause.get("type") {
        if wanted.as_str() != Some(subject.item_type) {
            return false;
        }
    }
    if let Some(wanted) = clause.get("labels") {
        let Some(wanted) = wanted.as_array() else {
            return false;
        };
        let holds = |label: &toml::Value| {
            label
                .as_str()
                .map(|label| subject.labels.iter().any(|own| own == label))
                .unwrap_or(false)
        };
        if !wanted.iter().all(holds) {
            return false;
        }
    }
    true
}

/// One key off the item's own object, when it holds a string there.
fn own_value(own: Option<&serde_json::Value>, key: &str) -> Option<String> {
    own?.get(key)?.as_str().map(str::to_string)
}

fn string_in(rule: &toml::Table, key: &str) -> Option<String> {
    rule.get(key)?.as_str().map(str::to_string)
}

fn filled(effective: &Effective, key: &str) -> bool {
    match key {
        "review" => effective.review.is_some(),
        _ => effective.gate.is_some(),
    }
}

fn set(effective: &mut Effective, key: &str, value: Option<String>) {
    if value.is_none() {
        return;
    }
    match key {
        "review" => effective.review = value,
        _ => effective.gate = value,
    }
}
