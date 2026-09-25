//! An item's record: the typed entries a store keeps on it, the text each one
//! is, and the fold every reader asks.
//!
//! THE STORE'S TIMELINE IS THE ONLY RECORD. What happened to an item — the
//! order, the delivery, the verdict, the hold and its clearance, the landing —
//! is the list of entries its store keeps, in the store's own order, and
//! nothing beside it: no reader re-derives a fact from a note's prose, and no
//! writer keeps a second copy anywhere else.
//!
//! ONE JSON OBJECT PER ENTRY. The text a store keeps is
//! `{"fleet.entry": 1, "kind": …, fields…}`: the key says the text is fleet's
//! and at which version, `kind` names one of [`KINDS`], and the fields are that
//! kind's and no others. The store's own id, time and author are the entry's
//! id, time and actor, so the text carries none of them.
//!
//! A TEXT WITHOUT THE KEY IS A PERSON'S, AND IS SKIPPED. A board is shared, and
//! a comment somebody wrote by hand — prose, or JSON of their own — is theirs:
//! [`decode`] answers [`Read::NotAnEntry`] and the fold never sees it.
//!
//! A MALFORMED ENTRY IS COULD-NOT-TELL, AND IS NEVER SKIPPED. A text carrying
//! the key at another version, with a field its kind does not have, missing
//! one it needs, or breaking a rule [`Body::validate`] keeps, is fleet's own
//! record gone wrong. Skipping it would answer from a record with a hole in it,
//! so [`read_row`] refuses the whole read and names the comment.
//!
//! THE ORDER IS THE STORE'S LISTING ORDER. [`Timeline`] answers by position in
//! the slice it is handed, and ids are never sorted: an id is the store's to
//! mint, and nothing promises it counts up.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::seat::actor::Actor;
use crate::seat::identity::SeatId;
use crate::store::OrderKind;

/// The key an entry's text carries, and the version it carries it at.
pub const KEY: &str = "fleet.entry";
pub const VERSION: u64 = 1;

/// Every kind an entry is, spelled as its text spells it and in the order an
/// item usually meets them.
pub const KINDS: [&str; 7] = [
    "ordered",
    "order_withdrawn",
    "delivered",
    "reviewed",
    "held",
    "cleared",
    "landed",
];

// ---- the entry ------------------------------------------------------------------

/// One entry as a reader holds it: the store's id and time, who wrote it, and
/// what it says. There is no serde here — the id, the time and the author are
/// the store's and never the text's, so the reader builds this from a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub at: String,
    pub by: Actor,
    pub body: Body,
}

/// What an entry says, one variant per kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    Ordered(Ordered),
    OrderWithdrawn(OrderWithdrawn),
    Delivered(Delivered),
    Reviewed(Reviewed),
    Held(Held),
    Cleared(Cleared),
    Landed(Landed),
}

impl Body {
    /// The kind, as [`KINDS`] spells it.
    pub fn kind(&self) -> &'static str {
        match self {
            Body::Ordered(_) => KINDS[0],
            Body::OrderWithdrawn(_) => KINDS[1],
            Body::Delivered(_) => KINDS[2],
            Body::Reviewed(_) => KINDS[3],
            Body::Held(_) => KINDS[4],
            Body::Cleared(_) => KINDS[5],
            Body::Landed(_) => KINDS[6],
        }
    }
}

// ---- ordered and order_withdrawn ------------------------------------------------

/// An order given: to a named seat, or to whichever transient seat the spawner
/// opens, which is `None` until one is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ordered {
    pub order: OrderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<SeatId>,
}

/// An order taken back, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderWithdrawn {
    pub why: Withdrawal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<SeatId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// The two acts that withdraw an order: the seat holding it retired, or the
/// spawner refused the seat it was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Withdrawal {
    Retire,
    SpawnRefused,
}

// ---- delivered ------------------------------------------------------------------

/// A seat's handoff: the commit it made, on which branch from which base, the
/// files it touched, what it checked, the suite it ran, and what it could not
/// prove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivered {
    pub commit: String,
    pub branch: String,
    pub base: String,
    pub files: Vec<String>,
    pub checks: Vec<CheckResult>,
    pub suite: SuiteRun,
    pub spec_corrections: Vec<SpecCorrection>,
    pub not_proven: Vec<NotProven>,
    pub decisions: Vec<Decision>,
    pub covers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub check: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecCorrection {
    pub premise: String,
    pub refuted_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotProven {
    pub surface: String,
    pub command: String,
}

/// A call the seat made, numbered by its place in the list: the review's walk
/// rules on `D<k>`, 1-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub call: String,
    pub not_taken: String,
    pub because: String,
}

/// A suite that ran, with the command and its exit, or one that did not, with
/// the reason. The two share no field, so the text itself says which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SuiteRun {
    Ran(Ran),
    NotTested(NotTested),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ran {
    pub command: String,
    pub rc: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotTested {
    pub not_tested: String,
}

// ---- reviewed -------------------------------------------------------------------

/// A reviewer's verdict on one commit, the size it measured, its ruling on each
/// numbered decision and, on a return, what it found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reviewed {
    pub verdict: Verdict,
    pub commit: String,
    pub size: Size,
    pub walk: Vec<Ruling>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accepted,
    Returned,
}

/// The review's size line: a measurement, and no tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub files: u64,
    pub added: u64,
    pub deleted: u64,
    pub binary: u64,
    pub tests: bool,
    pub executable: bool,
    pub base: String,
}

/// The ruling on the delivery's decision `D<decision>`, 1-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruling {
    pub decision: u32,
    pub ruling: RulingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulingKind {
    Accept,
    Overrule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub text: String,
}

// ---- held and cleared -----------------------------------------------------------

/// Work stopped on a question: the store's hold id, why, the lettered options,
/// and what it stopped on — a work branch at a commit, or a run by its hash.
/// `about` is set where the hold licenses something about other items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
    pub hold: String,
    pub reason: HoldReason,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub options: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<About>,
}

/// A seat's own question, or a run the controller stopped at
/// `[core.run] max_crashes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    Ask,
    MaxCrashes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub letter: String,
    pub text: String,
}

/// The items a hold is about, the commit it names where it names one, and the
/// option letter whose clearance licenses them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct About {
    pub items: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub licenses: String,
}

/// A hold cleared: answered with an option's letter (and, where the person
/// saw a third way, their own text), or cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cleared {
    pub hold: String,
    pub how: Clearance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clearance {
    Answer,
    Cancel,
}

// ---- landed ---------------------------------------------------------------------

/// A landing: the trunk's new sha and the one it moved from, the commit it
/// squashed, the suite it ran, every check row it read, and what it made of
/// the work branch left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Landed {
    pub sha: String,
    pub old: String,
    pub squash_of: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub test: SuiteRun,
    pub checks: Vec<CheckRow>,
    pub work_branch: WorkBranch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRow {
    pub check: String,
    pub verdict: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkBranch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub classification: Classification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Safe,
    CarriesUnlandedWork,
    Carries,
    CouldNotTell,
    NotGiven,
}

// ---- the rules ------------------------------------------------------------------

/// Exactly forty characters of lowercase hex: the whole sha git names a commit
/// by, and never an abbreviation, which stops being unique as a history grows.
pub fn full_sha(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha(field: &str, text: &str) -> Result<(), String> {
    if full_sha(text) {
        Ok(())
    } else {
        Err(format!("`{field}` is not a full 40-hex sha: {text}"))
    }
}

fn filled(field: &str, text: &str) -> Result<(), String> {
    if text.is_empty() {
        Err(format!("`{field}` is empty"))
    } else {
        Ok(())
    }
}

/// One capital letter, `^[A-Z]$`: an option's name.
fn a_letter(field: &str, text: &str) -> Result<(), String> {
    if text.len() == 1 && text.as_bytes()[0].is_ascii_uppercase() {
        Ok(())
    } else {
        Err(format!("`{field}` is not one capital letter: {text}"))
    }
}

impl Body {
    /// The rules a kind keeps beyond its shape. The first rule broken is the
    /// answer, and it names its field in backticks.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Body::Ordered(_) => Ok(()),
            Body::OrderWithdrawn(withdrawn) => withdrawn.validate(),
            Body::Delivered(delivered) => delivered.validate(),
            Body::Reviewed(reviewed) => reviewed.validate(),
            Body::Held(held) => held.validate(),
            Body::Cleared(cleared) => cleared.validate(),
            Body::Landed(landed) => landed.validate(),
        }
    }
}

// A seat, where an order or a withdrawal names one, is a SeatId: the type
// cannot hold an empty one, so neither kind has a rule of its own for it.

impl OrderWithdrawn {
    fn validate(&self) -> Result<(), String> {
        if self.why == Withdrawal::SpawnRefused && self.cause.is_none() {
            return Err(String::from(
                "`cause` is missing — a refused spawn says what refused it",
            ));
        }
        Ok(())
    }
}

impl Delivered {
    fn validate(&self) -> Result<(), String> {
        sha("commit", &self.commit)?;
        sha("base", &self.base)?;
        filled("branch", &self.branch)?;
        if self.files.is_empty() {
            return Err(String::from(
                "`files` names no file — `files` lists every path the delivery touched",
            ));
        }
        if self.not_proven.is_empty() {
            return Err(String::from(
                "`not_proven` names nothing not proven — `not_proven` is never empty",
            ));
        }
        for (n, file) in self.files.iter().enumerate() {
            filled(&format!("files[{n}]"), file)?;
        }
        for (n, row) in self.checks.iter().enumerate() {
            filled(&format!("checks[{n}].check"), &row.check)?;
            filled(&format!("checks[{n}].result"), &row.result)?;
        }
        suite("suite", &self.suite)?;
        for (n, correction) in self.spec_corrections.iter().enumerate() {
            filled(
                &format!("spec_corrections[{n}].premise"),
                &correction.premise,
            )?;
            filled(
                &format!("spec_corrections[{n}].refuted_by"),
                &correction.refuted_by,
            )?;
        }
        for (n, gap) in self.not_proven.iter().enumerate() {
            filled(&format!("not_proven[{n}].surface"), &gap.surface)?;
            filled(&format!("not_proven[{n}].command"), &gap.command)?;
        }
        for (n, decision) in self.decisions.iter().enumerate() {
            filled(&format!("decisions[{n}].call"), &decision.call)?;
            filled(&format!("decisions[{n}].not_taken"), &decision.not_taken)?;
            filled(&format!("decisions[{n}].because"), &decision.because)?;
        }
        for (n, covered) in self.covers.iter().enumerate() {
            filled(&format!("covers[{n}]"), covered)?;
        }
        Ok(())
    }
}

fn suite(field: &str, run: &SuiteRun) -> Result<(), String> {
    match run {
        SuiteRun::Ran(ran) => filled(&format!("{field}.command"), &ran.command),
        SuiteRun::NotTested(not) => filled(&format!("{field}.not_tested"), &not.not_tested),
    }
}

impl Reviewed {
    fn validate(&self) -> Result<(), String> {
        sha("commit", &self.commit)?;
        sha("size.base", &self.size.base)?;
        match self.verdict {
            Verdict::Returned if self.findings.is_empty() => {
                return Err(String::from(
                    "`findings` is empty — a return carries at least one finding",
                ));
            }
            Verdict::Accepted if !self.findings.is_empty() => {
                return Err(format!(
                    "`findings` carries {} — an acceptance carries none",
                    self.findings.len()
                ));
            }
            _ => {}
        }
        let mut last = 0;
        for ruling in &self.walk {
            let decision = ruling.decision;
            if decision == 0 {
                return Err(String::from(
                    "`walk` rules on D0 — decisions are numbered from D1",
                ));
            }
            if decision <= last {
                return Err(format!(
                    "`walk` rules on D{decision} after D{last} — its decision numbers strictly \
                     increase"
                ));
            }
            last = decision;
        }
        Ok(())
    }
}

impl Held {
    fn validate(&self) -> Result<(), String> {
        filled("hold", &self.hold)?;
        filled("question", &self.question)?;
        if self.question.contains('\n') {
            return Err(String::from(
                "`question` runs over more than one line — a question is one line",
            ));
        }
        if self.options.is_empty() {
            return Err(String::from(
                "`options` offers nothing — a hold carries at least one lettered option",
            ));
        }
        let mut letters: Vec<&str> = Vec::new();
        for (n, choice) in self.options.iter().enumerate() {
            a_letter(&format!("options[{n}].letter"), &choice.letter)?;
            if letters.contains(&choice.letter.as_str()) {
                return Err(format!(
                    "`options[{n}].letter` repeats {} — each option's letter is its own",
                    choice.letter
                ));
            }
            letters.push(&choice.letter);
            filled(&format!("options[{n}].text"), &choice.text)?;
        }
        // WHAT IT STOPPED ON, one of two and never both: a seat's work branch
        // at a commit, or a run's record by the run's hash.
        match (&self.branch, &self.commit, &self.run_hash) {
            (Some(_), Some(_), None) | (None, None, Some(_)) => {}
            (_, _, Some(_)) => {
                return Err(String::from(
                    "`run_hash` sits beside `branch` or `commit` — a hold stops on a work \
                     branch at a commit or on a run, never both",
                ));
            }
            _ => {
                return Err(String::from(
                    "`branch` and `commit` are not both set, and there is no `run_hash` — a \
                     hold stops on a work branch at a commit or on a run",
                ));
            }
        }
        if let Some(commit) = &self.commit {
            sha("commit", commit)?;
        }
        if let Some(about) = &self.about {
            if about.items.is_empty() {
                return Err(String::from(
                    "`about.items` names no item — a hold about items names at least one",
                ));
            }
            if let Some(commit) = &about.commit {
                sha("about.commit", commit)?;
            }
            a_letter("about.licenses", &about.licenses)?;
            if !letters.contains(&about.licenses.as_str()) {
                return Err(format!(
                    "`about.licenses` names {}, and no option carries that letter",
                    about.licenses
                ));
            }
        }
        Ok(())
    }
}

impl Cleared {
    fn validate(&self) -> Result<(), String> {
        match self.how {
            Clearance::Answer => match &self.letter {
                Some(letter) => a_letter("letter", letter),
                None => Err(String::from(
                    "`letter` is missing — an answer names the option it takes",
                )),
            },
            Clearance::Cancel if self.letter.is_some() => Err(String::from(
                "`letter` is set on a cancel — a cancel takes no option",
            )),
            Clearance::Cancel if self.text.is_some() => Err(String::from(
                "`text` is set on a cancel — a cancel answers nothing",
            )),
            Clearance::Cancel => Ok(()),
        }
    }
}

impl Landed {
    fn validate(&self) -> Result<(), String> {
        sha("sha", &self.sha)?;
        sha("old", &self.old)?;
        sha("squash_of", &self.squash_of)?;
        if self.checks.is_empty() {
            return Err(String::from(
                "`checks` is empty — a landing reads at least one check",
            ));
        }
        Ok(())
    }
}

// ---- the wire text --------------------------------------------------------------

/// The body's own object: its fields and its `kind`.
fn object(body: &Body) -> serde_json::Map<String, Value> {
    match serde_json::to_value(body) {
        Ok(Value::Object(object)) => object,
        // Every variant is a struct under an internal tag, and every field a
        // string, a number, a bool, or a list or struct of those: there is no
        // other value and no failure for serde_json to give.
        other => unreachable!("a body serializes to an object, and this one gave {other:?}"),
    }
}

/// The text a store keeps for one entry: the body's object with the key
/// stamped in, written compact.
pub fn encode(body: &Body) -> String {
    let mut object = object(body);
    object.insert(String::from(KEY), VERSION.into());
    Value::Object(object).to_string()
}

/// What one stored text is.
///
/// The body is held unboxed though it dwarfs the other two: a `Read` lives
/// for one match and its body moves straight into an [`Entry`], so a box
/// would buy one allocation per row and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum Read {
    /// A person's text: not a JSON object, or one without the key.
    NotAnEntry,
    Entry(Body),
    /// Fleet's key, and a text this binary cannot take as an entry.
    Unreadable(String),
}

pub fn decode(text: &str) -> Read {
    let Ok(Value::Object(mut object)) = serde_json::from_str::<Value>(text.trim()) else {
        return Read::NotAnEntry;
    };
    let Some(version) = object.remove(KEY) else {
        return Read::NotAnEntry;
    };
    if version.as_u64() != Some(VERSION) {
        return Read::Unreadable(format!(
            "carries {KEY} {version}, and this binary reads version {VERSION}"
        ));
    }
    let body = match serde_json::from_value::<Body>(Value::Object(object)) {
        Ok(body) => body,
        Err(error) => return Read::Unreadable(error.to_string()),
    };
    match body.validate() {
        Ok(()) => Read::Entry(body),
        Err(why) => Read::Unreadable(why),
    }
}

/// One of a store's rows on `item` read as an entry: `None` for a person's
/// text, and a refusal naming the comment for fleet's text that does not read
/// or an author that is not a typed actor.
pub fn read_row(
    item: &str,
    id: &str,
    author: &str,
    text: &str,
    at: &str,
) -> Result<Option<Entry>, String> {
    let body = match decode(text) {
        Read::NotAnEntry => return Ok(None),
        Read::Unreadable(why) => {
            return Err(format!(
                "{item}'s comment {id} carries {KEY} and does not read: {why}"
            ))
        }
        Read::Entry(body) => body,
    };
    let Some(Ok(by)) = Actor::typed(author) else {
        return Err(format!(
            "{item}'s comment {id} names its author {author}, which is not an actor reference \
             (<kind>:<id>)"
        ));
    };
    Ok(Some(Entry {
        id: id.to_string(),
        at: at.to_string(),
        by,
        body,
    }))
}

/// An entry as a reader's JSON shows it: the body's object with its `kind` and
/// without the key, beside the store's `id` and `at` and the actor as its one
/// string, `<kind>:<id>`.
///
/// THIS IS THE CONTRACT'S ENTRY: what an adapter's `timeline` answers for
/// each entry, so `fleet item show --json` and an adapter print one document,
/// and the actor is spelled as every write's `by` is.
pub fn to_json(entry: &Entry) -> Value {
    let mut object = object(&entry.body);
    object.insert(String::from("id"), entry.id.clone().into());
    object.insert(String::from("at"), entry.at.clone().into());
    object.insert(String::from("by"), entry.by.to_string().into());
    Value::Object(object)
}

// ---- the fold -------------------------------------------------------------------

/// An item's entries in the store's order, and every question a reader asks of
/// them. Each answer is by position in the slice.
pub struct Timeline<'a>(pub &'a [Entry]);

impl<'a> Timeline<'a> {
    /// The last entry `pick` takes, with its position.
    fn last<T>(
        &self,
        pick: impl Fn(&'a Body) -> Option<&'a T>,
    ) -> Option<(usize, &'a Entry, &'a T)> {
        self.0
            .iter()
            .enumerate()
            .rev()
            .find_map(|(at, entry)| pick(&entry.body).map(|body| (at, entry, body)))
    }

    /// Whether any entry after position `at` is one `is` names.
    fn after(&self, at: usize, is: impl Fn(&Body) -> bool) -> bool {
        self.0[at + 1..].iter().any(|entry| is(&entry.body))
    }

    /// The last order, where no withdrawal came after it.
    pub fn current_order(&self) -> Option<(&'a Entry, &'a Ordered)> {
        let (at, entry, order) = self.last(|body| match body {
            Body::Ordered(order) => Some(order),
            _ => None,
        })?;
        let withdrawn = self.after(at, |body| matches!(body, Body::OrderWithdrawn(_)));
        (!withdrawn).then_some((entry, order))
    }

    pub fn last_delivery(&self) -> Option<(&'a Entry, &'a Delivered)> {
        self.last(delivery).map(|(_, entry, body)| (entry, body))
    }

    fn standing(&self) -> Option<(usize, &'a Entry, &'a Delivered)> {
        let (at, entry, delivered) = self.last(delivery)?;
        let returned = self.after(
            at,
            |body| matches!(body, Body::Reviewed(review) if review.verdict == Verdict::Returned),
        );
        (!returned).then_some((at, entry, delivered))
    }

    /// The last delivery no return came after. A landing does not clear it
    /// (fleet-45m): what was landed still stood when it was.
    pub fn standing_delivery(&self) -> Option<(&'a Entry, &'a Delivered)> {
        self.standing().map(|(_, entry, body)| (entry, body))
    }

    /// The standing delivery, where no landing came after it: the work the
    /// item still carries.
    pub fn carried_delivery(&self) -> Option<(&'a Entry, &'a Delivered)> {
        let (at, entry, delivered) = self.standing()?;
        let landed = self.after(at, |body| matches!(body, Body::Landed(_)));
        (!landed).then_some((entry, delivered))
    }

    pub fn last_review(&self) -> Option<(&'a Entry, &'a Reviewed)> {
        self.last(|body| match body {
            Body::Reviewed(review) => Some(review),
            _ => None,
        })
        .map(|(_, entry, body)| (entry, body))
    }

    pub fn last_landing(&self) -> Option<(&'a Entry, &'a Landed)> {
        self.last(|body| match body {
            Body::Landed(landing) => Some(landing),
            _ => None,
        })
        .map(|(_, entry, body)| (entry, body))
    }

    /// The last hold raised under the id `hold`.
    pub fn held(&self, hold: &str) -> Option<(&'a Entry, &'a Held)> {
        self.last(|body| match body {
            Body::Held(held) if held.hold == hold => Some(held),
            _ => None,
        })
        .map(|(_, entry, body)| (entry, body))
    }

    /// The first clearance naming `hold`, whether or not a hold came before it:
    /// a clearance answers by the id it names.
    pub fn clearance(&self, hold: &str) -> Option<(&'a Entry, &'a Cleared)> {
        self.0.iter().find_map(|entry| match &entry.body {
            Body::Cleared(cleared) if cleared.hold == hold => Some((entry, cleared)),
            _ => None,
        })
    }

    /// The last hold no clearance naming it came after.
    pub fn open_hold(&self) -> Option<(&'a Entry, &'a Held)> {
        self.0
            .iter()
            .enumerate()
            .rev()
            .find_map(|(at, entry)| match &entry.body {
                Body::Held(held)
                    if !self.after(
                        at,
                        |body| matches!(body, Body::Cleared(cleared) if cleared.hold == held.hold),
                    ) =>
                {
                    Some((entry, held))
                }
                _ => None,
            })
    }

    /// The last hold whose `about` names `item`.
    pub fn holds_about(&self, item: &str) -> Option<(&'a Entry, &'a Held)> {
        self.last(|body| match body {
            Body::Held(held)
                if held
                    .about
                    .as_ref()
                    .is_some_and(|about| about.items.iter().any(|named| named == item)) =>
            {
                Some(held)
            }
            _ => None,
        })
        .map(|(_, entry, body)| (entry, body))
    }

    /// The entry the store keeps under `id`.
    pub fn entry(&self, id: &str) -> Option<&'a Entry> {
        self.0.iter().find(|entry| entry.id == id)
    }
}

fn delivery(body: &Body) -> Option<&Delivered> {
    match body {
        Body::Delivered(delivered) => Some(delivered),
        _ => None,
    }
}
