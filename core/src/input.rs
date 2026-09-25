//! What a seat hands in: its delivery, its question and a reviewer's findings,
//! each a JSON file read against the binary's own type before any verb writes.
//!
//! THE TYPE IS THE CONTRACT, AND THE SCHEMA SHOWS IT. Each input is a struct
//! here that denies unknown fields, and each has a JSON Schema in the defaults
//! a brief shows the seat. The file is read against the TYPE, never against the
//! schema text — a pack may reword what a seat reads, and no rewording changes
//! what the verb takes. [`agrees`] is what keeps the two saying one thing.
//!
//! THE FILE IS THE SEAT'S HALF. A delivery carries what only the seat knows —
//! the files, the checks, the suite, what it could not prove — and none of the
//! commit, the branch, the base, the time or the actor: the verb knows those
//! and fills them. The nested types are the entry's own ([`crate::entry`]), so
//! what a seat hands in and what the record keeps cannot drift apart.
//!
//! A FILE THAT DOES NOT READ IS A USAGE STOP. Every refusal [`read`] gives is
//! exit 2 — a file handed in that the grammar does not read — and nothing
//! about the record is judged, because nothing has been written.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entry::{
    full_sha, About, CheckResult, Choice, Decision, Delivered, Finding, Held, HoldReason,
    NotProven, SpecCorrection, SuiteRun,
};
use crate::item::Stop;

/// Each input's schema, as a path in the defaults' layers.
pub const DELIVERY_SCHEMA: &str = "assets/delivery.schema.json";
pub const QUESTION_SCHEMA: &str = "assets/question.schema.json";
pub const FINDINGS_SCHEMA: &str = "assets/findings.schema.json";

// ---- the three inputs -----------------------------------------------------------

/// A seat's delivery, before the verb gives it a commit. Every field is
/// required: a list with nothing to say is `[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryInput {
    pub files: Vec<String>,
    pub checks: Vec<CheckResult>,
    pub suite: SuiteRun,
    pub spec_corrections: Vec<SpecCorrection>,
    pub not_proven: Vec<NotProven>,
    pub decisions: Vec<Decision>,
    pub covers: Vec<String>,
}

/// A question a seat or a run stops on, before the store gives it a hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionInput {
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub options: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<About>,
}

/// What a reviewer found, before the verb writes the return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingsInput {
    pub findings: Vec<Finding>,
}

/// What a hold stopped on: a seat's work branch at a commit, or a run by its
/// hash. One of the two and never both, so the entry cannot carry both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    Work { branch: String, commit: String },
    Run { hash: String },
}

impl DeliveryInput {
    /// The entry this delivery is, with the three facts only the verb knows.
    pub fn into_delivered(self, commit: String, branch: String, base: String) -> Delivered {
        Delivered {
            commit,
            branch,
            base,
            files: self.files,
            checks: self.checks,
            suite: self.suite,
            spec_corrections: self.spec_corrections,
            not_proven: self.not_proven,
            decisions: self.decisions,
            covers: self.covers,
        }
    }
}

impl QuestionInput {
    /// The entry this question is, under the store's hold id, for the reason
    /// the verb raises it and on what it stopped.
    pub fn into_held(self, hold: String, reason: HoldReason, at: Standing) -> Held {
        let (branch, commit, run_hash) = match at {
            Standing::Work { branch, commit } => (Some(branch), Some(commit), None),
            Standing::Run { hash } => (None, None, Some(hash)),
        };
        Held {
            hold,
            reason,
            question: self.question,
            context: self.context,
            options: self.options,
            branch,
            commit,
            run_hash,
            about: self.about,
        }
    }
}

impl FindingsInput {
    pub fn findings(self) -> Vec<Finding> {
        self.findings
    }
}

// ---- the rules beyond the shape -------------------------------------------------

/// The rules an input keeps beyond what serde reads. The answer is the rest of
/// a sentence that opens on "the <what> at <path> (read against <schema>)", and
/// the first rule broken is the one it gives.
///
/// Whatever passes here, the entry its constructor makes passes
/// [`crate::entry::Body::validate`], given the verb's own facts are whole: a
/// full commit and base, a branch and a hold id that are not empty.
pub trait Checked {
    fn check(&self) -> Result<(), String>;
}

fn filled(field: &str, text: &str) -> Result<(), String> {
    if text.is_empty() {
        Err(format!("carries an empty `{field}`"))
    } else {
        Ok(())
    }
}

/// One capital letter, `^[A-Z]$`: an option's name.
fn a_letter(field: &str, text: &str) -> Result<(), String> {
    if text.len() == 1 && text.as_bytes()[0].is_ascii_uppercase() {
        Ok(())
    } else {
        Err(format!(
            "carries `{field}` {text}, which is not one capital letter"
        ))
    }
}

impl Checked for DeliveryInput {
    fn check(&self) -> Result<(), String> {
        if self.files.is_empty() {
            return Err(String::from("names no file"));
        }
        if self.not_proven.is_empty() {
            return Err(String::from("names nothing not proven — it is never empty"));
        }
        for (n, file) in self.files.iter().enumerate() {
            filled(&format!("files[{n}]"), file)?;
        }
        for (n, row) in self.checks.iter().enumerate() {
            filled(&format!("checks[{n}].check"), &row.check)?;
            filled(&format!("checks[{n}].result"), &row.result)?;
        }
        match &self.suite {
            SuiteRun::Ran(ran) => filled("suite.command", &ran.command)?,
            SuiteRun::NotTested(not) => filled("suite.not_tested", &not.not_tested)?,
        }
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

impl Checked for QuestionInput {
    fn check(&self) -> Result<(), String> {
        filled("question", &self.question)?;
        if self.question.contains('\n') {
            return Err(String::from(
                "asks its question over more than one line — context carries the rest",
            ));
        }
        if let Some(context) = &self.context {
            filled("context", context)?;
        }
        if self.options.is_empty() {
            return Err(String::from(
                "names no option — a question with none is a conversation",
            ));
        }
        let mut letters: Vec<&str> = Vec::new();
        for (n, choice) in self.options.iter().enumerate() {
            a_letter(&format!("options[{n}].letter"), &choice.letter)?;
            if letters.contains(&choice.letter.as_str()) {
                return Err(format!("names option {} twice", choice.letter));
            }
            letters.push(&choice.letter);
            filled(&format!("options[{n}].text"), &choice.text)?;
        }
        // THE HELD RULES on `about`, word for word what the entry keeps, so a
        // question that reads here is a hold the record takes.
        if let Some(about) = &self.about {
            if about.items.is_empty() {
                return Err(String::from(
                    "names no item in `about.items` — a question about items names at least one",
                ));
            }
            for (n, item) in about.items.iter().enumerate() {
                filled(&format!("about.items[{n}]"), item)?;
            }
            if let Some(commit) = &about.commit {
                if !full_sha(commit) {
                    return Err(format!(
                        "carries `about.commit` {commit}, which is not a full 40-hex sha"
                    ));
                }
            }
            a_letter("about.licenses", &about.licenses)?;
            if !letters.contains(&about.licenses.as_str()) {
                return Err(format!(
                    "names {} in `about.licenses`, and no option carries that letter",
                    about.licenses
                ));
            }
        }
        Ok(())
    }
}

impl Checked for FindingsInput {
    fn check(&self) -> Result<(), String> {
        if self.findings.is_empty() {
            return Err(String::from(
                "numbers no finding — a return that numbers nothing is a question",
            ));
        }
        for (n, finding) in self.findings.iter().enumerate() {
            filled(&format!("findings[{n}].text"), &finding.text)?;
        }
        Ok(())
    }
}

// ---- reading one ----------------------------------------------------------------

/// The input at `path`, read as `T` and checked. `what` names it in the
/// refusal ("delivery", "question", "findings") and `schema` is the path of
/// the schema the seat's brief showed it.
///
/// A shape serde refuses names the key it refused at, because a seat fixing
/// its file by hand needs the place and not only the type serde expected. Every
/// refusal of a file that was read names `schema`, the page the seat fixes it
/// against.
pub fn read<T: DeserializeOwned + Checked>(
    path: &Path,
    what: &str,
    schema: &str,
) -> Result<T, Stop> {
    let at = path.display();
    let text = std::fs::read_to_string(path)
        .map_err(|e| Stop::usage(format!("the {what} at {at} could not be read: {e}")))?;
    let not_the_shape = |e: &dyn std::fmt::Display| {
        Stop::usage(format!(
            "the {what} at {at} is not the shape {schema} gives: {e} — the brief shows that schema"
        ))
    };
    let mut reader = serde_json::Deserializer::from_str(&text);
    let input: T = serde_path_to_error::deserialize(&mut reader).map_err(|e| not_the_shape(&e))?;
    reader.end().map_err(|e| not_the_shape(&e))?;
    input
        .check()
        .map_err(|why| Stop::usage(format!("the {what} at {at} (read against {schema}) {why}")))?;
    Ok(input)
}

// ---- the schema agrees with the type --------------------------------------------

/// A place in a JSON document: a key, or an index into an array.
#[derive(Debug, Clone)]
enum Step {
    Key(String),
    Index(usize),
}

fn show(root: &str, at: &[Step]) -> String {
    let mut text = String::from(root);
    for step in at {
        match step {
            Step::Key(key) => {
                text.push('.');
                text.push_str(key);
            }
            Step::Index(n) => text.push_str(&format!("[{n}]")),
        }
    }
    text
}

/// Where a schema's example sits in the schema text.
const EXAMPLE: &str = "$.examples[0]";

/// Whether `schema` says its value is an object: it names the type, or it
/// gives properties. A node that only chooses among branches (`oneOf`) is
/// not one — each branch is — and must not be, because `additionalProperties:
/// false` beside a `oneOf` refuses every key a branch names.
fn object_schema(schema: &Value) -> bool {
    schema.get("type").and_then(Value::as_str) == Some("object")
        || schema.get("properties").is_some()
}

/// The properties an object schema gives, and none where it gives none.
fn properties(schema: &Value) -> impl Iterator<Item = (&String, &Value)> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
}

fn required(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|keys| keys.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Whether `schema_text` says what `T` reads and writes. It checks, and
/// answers the first disagreement named by its JSON path in the schema text:
///
/// (a) the text is JSON;
/// (b) `examples[0]` reads as `T`, and written back its keys are the
///     schema's `properties` keys, at every object the schema describes;
/// (c) the example without any one required key does not read, and without
///     any one key that is not required, still does;
/// (d) every object schema in the tree carries `additionalProperties: false`.
pub fn agrees<T: DeserializeOwned + Serialize>(schema_text: &str) -> Result<(), String> {
    // (a)
    let schema: Value =
        serde_json::from_str(schema_text).map_err(|e| format!("`$` is not JSON: {e}"))?;

    // (b)
    let Some(example) = schema.get("examples").and_then(|all| all.get(0)) else {
        return Err(format!(
            "`{EXAMPLE}` is missing — a schema carries one example with every property"
        ));
    };
    let typed: T = serde_json::from_value(example.clone())
        .map_err(|e| format!("`{EXAMPLE}` does not read as the type: {e}"))?;
    let written = serde_json::to_value(&typed)
        .map_err(|e| format!("`{EXAMPLE}` read, and the type would not write it back: {e}"))?;
    let mut objects = Vec::new();
    same_keys(
        &schema,
        String::from("$"),
        &written,
        &mut Vec::new(),
        &mut objects,
    )?;

    // (c)
    for (at, node) in &objects {
        let needed = required(node);
        for (key, _) in properties(node) {
            let mut without = written.clone();
            remove(&mut without, at, key);
            let place = show(
                EXAMPLE,
                &[at.as_slice(), &[Step::Key(key.clone())]].concat(),
            );
            match (
                needed.contains(&key.as_str()),
                serde_json::from_value::<T>(without),
            ) {
                (true, Ok(_)) => {
                    return Err(format!(
                        "`{place}` is required by the schema, and the type reads the example \
                         without it"
                    ))
                }
                (false, Err(e)) => {
                    return Err(format!(
                        "`{place}` is not required by the schema, and the type does not read \
                         the example without it: {e}"
                    ))
                }
                _ => {}
            }
        }
    }

    // (d)
    closed(&schema, String::from("$"))
}

/// (b)'s walk: `value` is the type's own writing of the example, and `schema`
/// the node describing it. Every object met is kept, with the place it sits
/// and the schema that describes it, for (c).
fn same_keys<'s>(
    schema: &'s Value,
    schema_at: String,
    value: &Value,
    at: &mut Vec<Step>,
    objects: &mut Vec<(Vec<Step>, &'s Value)>,
) -> Result<(), String> {
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
        // The branch the value takes is the one whose keys it carries: the
        // branches of an untagged enum share no key, so at most one fits.
        let keys = value.as_object().map(|object| {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            keys
        });
        for (n, branch) in branches.iter().enumerate() {
            let mut names: Vec<&String> = properties(branch).map(|(key, _)| key).collect();
            names.sort();
            if keys.as_ref() == Some(&names) {
                return same_keys(
                    branch,
                    format!("{schema_at}.oneOf[{n}]"),
                    value,
                    at,
                    objects,
                );
            }
        }
        return Err(format!(
            "`{}` takes no branch of `{schema_at}.oneOf`",
            show(EXAMPLE, at)
        ));
    }

    if object_schema(schema) {
        let Some(object) = value.as_object() else {
            return Err(format!(
                "`{}` is not an object, and `{schema_at}` describes one",
                show(EXAMPLE, at)
            ));
        };
        for key in object.keys() {
            if !properties(schema).any(|(named, _)| named == key) {
                at.push(Step::Key(key.clone()));
                let place = show(EXAMPLE, at);
                return Err(format!(
                    "`{place}` is written by the type, and `{schema_at}.properties` does not \
                     name it"
                ));
            }
        }
        for (key, _) in properties(schema) {
            if !object.contains_key(key) {
                return Err(format!(
                    "`{schema_at}.properties.{key}` is not written by the type, and the example \
                     carries every property"
                ));
            }
        }
        objects.push((at.clone(), schema));
        for (key, child) in properties(schema) {
            at.push(Step::Key(key.clone()));
            same_keys(
                child,
                format!("{schema_at}.properties.{key}"),
                &object[key],
                at,
                objects,
            )?;
            at.pop();
        }
        return Ok(());
    }

    if value.is_object() {
        return Err(format!(
            "`{}` is an object, and `{schema_at}` gives it no properties",
            show(EXAMPLE, at)
        ));
    }

    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (n, element) in values.iter().enumerate() {
            at.push(Step::Index(n));
            same_keys(items, format!("{schema_at}.items"), element, at, objects)?;
            at.pop();
        }
    }
    Ok(())
}

/// `key` taken out of the object at `at`.
fn remove(value: &mut Value, at: &[Step], key: &str) {
    let mut here = value;
    for step in at {
        here = match step {
            Step::Key(name) => &mut here[name.as_str()],
            Step::Index(n) => &mut here[*n],
        };
    }
    if let Some(object) = here.as_object_mut() {
        object.remove(key);
    }
}

/// (d)'s walk, over every place a schema may sit.
fn closed(schema: &Value, schema_at: String) -> Result<(), String> {
    if object_schema(schema) && schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(format!(
            "`{schema_at}` is an object schema without `additionalProperties: false`"
        ));
    }
    for keyword in ["properties", "$defs"] {
        if let Some(children) = schema.get(keyword).and_then(Value::as_object) {
            for (key, child) in children {
                closed(child, format!("{schema_at}.{keyword}.{key}"))?;
            }
        }
    }
    if let Some(items) = schema.get("items") {
        closed(items, format!("{schema_at}.items"))?;
    }
    for keyword in ["oneOf", "anyOf", "allOf"] {
        if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
            for (n, branch) in branches.iter().enumerate() {
                closed(branch, format!("{schema_at}.{keyword}[{n}]"))?;
            }
        }
    }
    Ok(())
}
