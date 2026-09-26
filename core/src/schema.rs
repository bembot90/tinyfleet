//! What a contract's JSON Schema document is built with and checked by,
//! whichever contract it states.
//!
//! Two contracts print themselves as a document: the store's
//! ([`crate::store::schema`], `fleet store schema`) and the agent's
//! ([`crate::agent::schema`], `fleet agent schema`). Each document is its own
//! — its verbs, its types, its version — and both are built by the helpers
//! here, so that the two shapes an adapter author reads cannot drift apart:
//! `verbs` holding each verb's `request` and `response`, `refusal` and `error`
//! the answers of exits 1 and 3, every `$ref` into the document's own `$defs`,
//! and no prose carried over from the types.
//!
//! THE CHECK IS HERE TOO, under `test-support`: the one validator every suite
//! holds an instance to either document with ([`check`]), and the walk that
//! makes it a whole one for a document ([`unknown_keywords`]).

use schemars::{JsonSchema, SchemaGenerator};
use serde_json::{Map, Value};

/// `document` as a committed file spells it: two-space indented, keys in
/// order, one newline at the end.
pub(crate) fn text(document: &Value) -> String {
    let mut text =
        serde_json::to_string_pretty(document).expect("a document of JSON values serializes");
    text.push('\n');
    text
}

/// The `$ref` to `T`'s schema, which puts that schema in the definitions.
pub(crate) fn refer<T: JsonSchema>(g: &mut SchemaGenerator) -> Value {
    g.subschema_for::<T>().to_value()
}

/// `T`'s own schema, written out rather than referred to, and stripped.
pub(crate) fn generated<T: JsonSchema>(g: &mut SchemaGenerator) -> Map<String, Value> {
    let mut schema = T::json_schema(g).to_value();
    strip(&mut schema);
    match schema {
        Value::Object(schema) => schema,
        other => unreachable!("every body and change is an object schema, and this is {other}"),
    }
}

/// An object schema with each of `named` among its properties and demanded,
/// after whatever it already holds.
pub(crate) fn fields<const N: usize>(
    mut schema: Map<String, Value>,
    named: [(&str, Value); N],
) -> Map<String, Value> {
    schema.insert(String::from("type"), Value::from("object"));
    let mut required = match schema.remove("required") {
        Some(Value::Array(names)) => names,
        _ => Vec::new(),
    };
    let properties = schema
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(properties) = properties {
        for (name, field) in named {
            properties.insert(String::from(name), field);
            required.push(Value::from(name));
        }
    }
    if !required.is_empty() {
        schema.insert(String::from("required"), Value::Array(required));
    }
    schema
}

/// `words` as the description of the property `name`.
pub(crate) fn describe(schema: &mut Map<String, Value>, name: &str, words: &str) {
    if let Some(Value::Object(property)) = schema
        .get_mut("properties")
        .and_then(|properties| properties.get_mut(name))
    {
        property.insert(String::from("description"), Value::from(words));
    }
}

/// The schema without the prose the derive carried: every `description` and
/// `title` keyword, and never a property that shares the name — an item's
/// `description` is one.
pub(crate) fn strip(schema: &mut Value) {
    let Value::Object(object) = schema else {
        return;
    };
    object.remove("description");
    object.remove("title");
    for (keyword, value) in object.iter_mut() {
        match (keyword.as_str(), value) {
            ("properties" | "$defs", Value::Object(named)) => named.values_mut().for_each(strip),
            ("anyOf" | "oneOf" | "allOf", Value::Array(each)) => each.iter_mut().for_each(strip),
            ("items" | "additionalProperties", one) => strip(one),
            _ => {}
        }
    }
}

// ---- the check ------------------------------------------------------------------

/// Every keyword [`check`] knows. The first thirteen constrain, and it applies
/// each as JSON Schema 2020-12 says; the rest are annotations and places,
/// which constrain nothing.
#[cfg(any(test, feature = "test-support"))]
pub const KEYWORDS: [&str; 20] = [
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "pattern",
    "anyOf",
    "oneOf",
    "allOf",
    "minimum",
    "maximum",
    "$ref",
    "$schema",
    "$defs",
    "default",
    "format",
    "description",
    "title",
];

/// Whether `instance` passes the schema at `pointer` in `document`, or the
/// first place it does not and why.
///
/// A CHECK OF THE KEYWORDS THE DOCUMENTS USE AND NO OTHERS: each of
/// [`KEYWORDS`] is applied in full, a `$ref` is followed only into the same
/// document, a `pattern` is matched by regex-lite, whose classes are ASCII,
/// and a schema carrying any other keyword is refused rather than passed. It
/// is not a general validator; the arm that walks a document for a keyword
/// outside [`KEYWORDS`] ([`unknown_keywords`]) is what makes it a whole one
/// for that document.
#[cfg(any(test, feature = "test-support"))]
pub fn check(document: &Value, pointer: &str, instance: &Value) -> Result<(), String> {
    let schema = document
        .pointer(pointer)
        .ok_or_else(|| format!("the document holds nothing at {pointer}"))?;
    passes(document, schema, instance, "")
}

/// Every place in `document` a keyword outside [`KEYWORDS`] is written, by
/// its pointer: each verb's request and response, the refusal, the error and
/// every definition, walked through every schema they hold.
///
/// [`check`] refuses a keyword it does not know, but only where an instance
/// leads it; this walks all of a document, so a keyword the generator starts
/// writing is named before any check passes over it.
#[cfg(any(test, feature = "test-support"))]
pub fn unknown_keywords(document: &Value) -> Vec<String> {
    fn walk(schema: &Value, at: String, unknown: &mut Vec<String>) {
        let Value::Object(object) = schema else {
            return;
        };
        for (keyword, value) in object {
            if !KEYWORDS.contains(&keyword.as_str()) {
                unknown.push(format!("{at}/{keyword}"));
            }
            match (keyword.as_str(), value) {
                ("properties" | "$defs", Value::Object(named)) => {
                    for (name, schema) in named {
                        walk(schema, format!("{at}/{keyword}/{name}"), unknown);
                    }
                }
                ("anyOf" | "oneOf" | "allOf", Value::Array(each)) => {
                    for (n, schema) in each.iter().enumerate() {
                        walk(schema, format!("{at}/{keyword}/{n}"), unknown);
                    }
                }
                ("items" | "additionalProperties", schema) => {
                    walk(schema, format!("{at}/{keyword}"), unknown);
                }
                _ => {}
            }
        }
    }
    let mut unknown = Vec::new();
    for (verb, both) in document["verbs"].as_object().expect("verbs is an object") {
        for side in ["request", "response"] {
            walk(&both[side], format!("/verbs/{verb}/{side}"), &mut unknown);
        }
    }
    walk(&document["refusal"], String::from("/refusal"), &mut unknown);
    walk(&document["error"], String::from("/error"), &mut unknown);
    for (name, schema) in document["$defs"].as_object().expect("$defs is an object") {
        walk(schema, format!("/$defs/{name}"), &mut unknown);
    }
    unknown
}

#[cfg(any(test, feature = "test-support"))]
fn passes(document: &Value, schema: &Value, instance: &Value, at: &str) -> Result<(), String> {
    let object = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err(format!("{at}: nothing is allowed here")),
        Value::Object(object) => object,
        other => return Err(format!("{at}: {other} is not a schema")),
    };
    let here = |why: String| format!("{}: {why}", if at.is_empty() { "/" } else { at });
    for (keyword, value) in object {
        match keyword.as_str() {
            "$ref" => {
                let target = value
                    .as_str()
                    .and_then(|reference| reference.strip_prefix('#'))
                    .and_then(|pointer| document.pointer(pointer))
                    .ok_or_else(|| here(format!("{value} does not resolve in the document")))?;
                passes(document, target, instance, at)?;
            }
            "type" => {
                let named: Vec<&str> = match value {
                    Value::String(one) => vec![one.as_str()],
                    Value::Array(each) => each.iter().filter_map(Value::as_str).collect(),
                    other => return Err(here(format!("type {other} is not a type"))),
                };
                if !named.iter().any(|name| is_a(instance, name)) {
                    return Err(here(format!("{instance} is not {}", named.join(" or "))));
                }
            }
            "properties" => {
                if let (Value::Object(properties), Value::Object(fields)) = (value, instance) {
                    for (name, field) in fields {
                        if let Some(schema) = properties.get(name) {
                            passes(document, schema, field, &format!("{at}/{name}"))?;
                        }
                    }
                }
            }
            "required" => {
                if let Value::Object(fields) = instance {
                    for name in value.as_array().into_iter().flatten() {
                        let name = name.as_str().unwrap_or_default();
                        if !fields.contains_key(name) {
                            return Err(here(format!("`{name}` is required and absent")));
                        }
                    }
                }
            }
            "additionalProperties" => {
                if let Value::Object(fields) = instance {
                    let named = object.get("properties").and_then(Value::as_object);
                    for (name, field) in fields {
                        if !named.is_some_and(|named| named.contains_key(name)) {
                            passes(document, value, field, &format!("{at}/{name}"))
                                .map_err(|why| format!("{why} (`{name}` is not a named key)"))?;
                        }
                    }
                }
            }
            "items" => {
                if let Value::Array(elements) = instance {
                    for (n, element) in elements.iter().enumerate() {
                        passes(document, value, element, &format!("{at}/{n}"))?;
                    }
                }
            }
            "enum" => {
                if !value.as_array().is_some_and(|each| each.contains(instance)) {
                    return Err(here(format!("{instance} is none of {value}")));
                }
            }
            "const" => {
                if value != instance {
                    return Err(here(format!("{instance} is not {value}")));
                }
            }
            "pattern" => {
                if let Value::String(text) = instance {
                    let pattern = value.as_str().unwrap_or_default();
                    let matched = regex_lite::Regex::new(pattern)
                        .map_err(|why| here(format!("pattern {pattern} does not compile: {why}")))?
                        .is_match(text);
                    if !matched {
                        return Err(here(format!("{text:?} does not match {pattern}")));
                    }
                }
            }
            "anyOf" | "oneOf" | "allOf" => {
                let each = value.as_array().map(Vec::as_slice).unwrap_or_default();
                let answers: Vec<Result<(), String>> = each
                    .iter()
                    .map(|schema| passes(document, schema, instance, at))
                    .collect();
                let passed = answers.iter().filter(|answer| answer.is_ok()).count();
                let held = match keyword.as_str() {
                    "anyOf" => passed > 0,
                    "oneOf" => passed == 1,
                    _ => passed == each.len(),
                };
                if !held {
                    let why: Vec<String> = answers.into_iter().filter_map(Result::err).collect();
                    return Err(here(format!(
                        "{passed} of {} {keyword} branches pass: {}",
                        each.len(),
                        why.join("; ")
                    )));
                }
            }
            "minimum" | "maximum" => {
                if let (Some(bound), Some(number)) = (value.as_f64(), instance.as_f64()) {
                    let inside = if keyword == "minimum" {
                        number >= bound
                    } else {
                        number <= bound
                    };
                    if !inside {
                        return Err(here(format!("{number} is past its {keyword} {bound}")));
                    }
                }
            }
            known if KEYWORDS.contains(&known) => {}
            unknown => return Err(here(format!("the check does not know `{unknown}`"))),
        }
    }
    Ok(())
}

/// Whether `instance` is of the JSON Schema type `name`.
#[cfg(any(test, feature = "test-support"))]
fn is_a(instance: &Value, name: &str) -> bool {
    match (name, instance) {
        ("null", Value::Null)
        | ("boolean", Value::Bool(_))
        | ("string", Value::String(_))
        | ("array", Value::Array(_))
        | ("object", Value::Object(_))
        | ("number", Value::Number(_)) => true,
        ("integer", Value::Number(n)) => {
            n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        _ => false,
    }
}
