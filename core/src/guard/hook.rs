//! An agent's pre-tool hook, read as data: where its payload keeps the tool
//! and the command, and the document it reads a refusal out of.
//!
//! THE MAPPING IS THE ADAPTER'S, DECLARED AND NOT RUN. An agent adapter's
//! `adapter.toml` carries a `[hook]` table — its shell tool's name, three JSON
//! pointers into the payload, and the refusal as a template — and `fleet
//! guard` reads that one file and applies core's classes. No process is
//! spawned per tool call, and no provider's field name is spelled here.
//!
//! This module states no decision value. The template is the adapter's, so
//! the rule the classes hold — a guard never emits an allowing decision — is
//! kept by what a template can say: the reason goes into it, and letting a
//! command through prints nothing at all.

use serde_json::Value;

/// The keys a `[hook]` table holds. Any other is a defect.
pub const KEYS: [&str; 5] = ["shell_tool", "tool", "command", "cwd", "deny"];

/// The hole in the refusal template the reason fills, as a JSON string
/// literal: quoted and escaped, so a template is written around a value and
/// never inside a string.
pub const REASON: &str = "{reason}";

/// The reason a template is filled with when it is read, to prove it parses
/// as JSON once filled. It carries every character the literal escapes.
const TEST_FILL: &str = "a \"quoted\" \\ reason\nover two lines";

/// One agent's `[hook]` table, read and held to its shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookMap {
    /// The agent's name for its shell tool: the one tool a guard judges.
    pub shell_tool: String,
    /// JSON pointers into the payload: the tool named, the command it runs,
    /// and, where the agent sends one, the directory it runs in.
    pub tool: String,
    pub command: String,
    pub cwd: Option<String>,
    /// The refusal, holding [`REASON`] once or more.
    pub deny: String,
}

/// What one payload says, where there is something to judge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    pub tool: String,
    pub command: String,
    pub cwd: Option<String>,
}

impl HookMap {
    /// The `[hook]` table's contents, held to their shape: `shell_tool` a
    /// string that is not blank, `tool`, `command` and the optional `cwd` JSON
    /// pointers, and `deny` a template holding [`REASON`] that parses as JSON
    /// once filled. The first thing out of shape is the answer, naming the key.
    pub fn parse(table: &toml::Table) -> Result<HookMap, String> {
        if let Some(unknown) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(format!(
                "`{unknown}` is not a key [hook] holds — it holds shell_tool, tool, command, \
                 cwd and deny"
            ));
        }
        let text = |key: &str| match table.get(key) {
            None => Ok(None),
            Some(toml::Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(format!("`{key}` is not a string")),
        };
        let required = |key: &str| text(key)?.ok_or_else(|| format!("`{key}` is missing"));
        let pointer = |key: &str, value: String| {
            if is_pointer(&value) {
                Ok(value)
            } else {
                Err(format!(
                    "`{key}` is `{value}`, which is not a JSON pointer — one starts with `/` \
                     and writes `~` as `~0`"
                ))
            }
        };

        let shell_tool = required("shell_tool")?;
        if shell_tool.trim().is_empty() {
            return Err(String::from("`shell_tool` is blank"));
        }
        let tool = pointer("tool", required("tool")?)?;
        let command = pointer("command", required("command")?)?;
        let cwd = text("cwd")?
            .map(|value| pointer("cwd", value))
            .transpose()?;
        let deny = required("deny")?;
        if !deny.contains(REASON) {
            return Err(format!(
                "`deny` holds no {REASON} — a refusal that does not carry the reason is one \
                 the agent cannot act on"
            ));
        }
        let map = HookMap {
            shell_tool,
            tool,
            command,
            cwd,
            deny,
        };
        serde_json::from_str::<Value>(&map.deny(TEST_FILL))
            .map_err(|e| format!("`deny` is not JSON once {REASON} is filled: {e}"))?;
        Ok(map)
    }

    /// The `[hook]` table of a manifest's whole text, held to its shape — for
    /// a mapping compiled into a caller rather than read off a pack's
    /// directory, which [`crate::pack::adapter_manifest`] reads.
    pub fn from_manifest(text: &str) -> Result<HookMap, String> {
        let manifest: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| format!("the manifest is not TOML: {}", e.message()))?;
        match manifest.get(crate::pack::HOOK_TABLE) {
            Some(toml::Value::Table(table)) => HookMap::parse(table),
            _ => Err(format!(
                "the manifest declares no [{}]",
                crate::pack::HOOK_TABLE
            )),
        }
    }

    /// What `body` says, or `None` where there is nothing to judge.
    ///
    /// Four shapes are nothing to judge and each is silence: a body that is not
    /// JSON, a tool that is not the shell one — said here as well as in the
    /// hook's own matcher, where a mis-wired matcher cannot undo it — a payload
    /// with no tool named at all, and a command that is absent, not a string,
    /// or blank.
    pub fn payload(&self, body: &str) -> Option<Payload> {
        let parsed: Value = serde_json::from_str(body).ok()?;
        let tool = parsed.pointer(&self.tool)?.as_str()?;
        if tool != self.shell_tool {
            return None;
        }
        let command = parsed.pointer(&self.command)?.as_str()?;
        if command.trim().is_empty() {
            return None;
        }
        Some(Payload {
            tool: tool.to_string(),
            command: command.to_string(),
            cwd: self
                .cwd
                .as_deref()
                .and_then(|at| parsed.pointer(at))
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    /// The refusal as the agent reads it: the template with every [`REASON`]
    /// replaced by the reason as a JSON string literal.
    pub fn deny(&self, reason: &str) -> String {
        let literal = Value::String(reason.to_string()).to_string();
        self.deny.replace(REASON, &literal)
    }
}

/// RFC 6901's grammar: empty, or `/`-led tokens in which `~` is only ever
/// `~0` or `~1`.
fn is_pointer(text: &str) -> bool {
    if !(text.is_empty() || text.starts_with('/')) {
        return false;
    }
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mapping no real agent speaks, so the reader is shown to hold no
    /// agent's field names of its own.
    const MAPPING: &str = r#"
shell_tool = "sh"
tool = "/call/name"
command = "/call/args/line"
cwd = "/where"
deny = '{"verdict":"refused","why":{reason}}'
"#;

    fn mapping() -> HookMap {
        HookMap::parse(&MAPPING.parse().expect("the mapping is TOML")).expect("the mapping reads")
    }

    fn body(tool: Value, line: Value) -> String {
        serde_json::json!({"call": {"name": tool, "args": {"line": line}}, "where": "/w"})
            .to_string()
    }

    #[test]
    fn a_payload_is_read_through_the_pointers() {
        let read = mapping().payload(&body("sh".into(), "ls -l".into()));
        assert_eq!(
            read,
            Some(Payload {
                tool: "sh".into(),
                command: "ls -l".into(),
                cwd: Some("/w".into()),
            })
        );
        let unplaced = HookMap {
            cwd: None,
            ..mapping()
        };
        assert_eq!(
            unplaced
                .payload(&body("sh".into(), "ls".into()))
                .and_then(|p| p.cwd),
            None,
            "a mapping with no cwd pointer reads no directory"
        );
    }

    /// The four silences, each on its own: RED-PROOF — a reader that skipped
    /// the tool check would judge the second, and one that took any command
    /// would judge the last three.
    #[test]
    fn nothing_to_judge_is_none() {
        let map = mapping();
        for (label, text) in [
            ("not json", String::from("this is not json")),
            ("another tool", body("read".into(), "ls".into())),
            (
                "no tool",
                serde_json::json!({"call": {"args": {"line": "ls"}}}).to_string(),
            ),
            ("a tool that is not a string", body(7.into(), "ls".into())),
            ("no command", body("sh".into(), Value::Null)),
            (
                "a command that is not a string",
                body("sh".into(), 17.into()),
            ),
            ("a blank command", body("sh".into(), "   ".into())),
            ("an empty body", String::new()),
        ] {
            assert_eq!(map.payload(&text), None, "{label}");
        }
    }

    #[test]
    fn a_refusal_fills_the_template_with_the_reason_as_a_json_string() {
        let reason = "no — \"that\"\nand \\ this";
        let written = mapping().deny(reason);
        let read: Value = serde_json::from_str(&written).expect("the filled template is JSON");
        assert_eq!(read["why"], reason);
        assert_eq!(read["verdict"], "refused");
        assert_eq!(
            written,
            format!(
                "{{\"verdict\":\"refused\",\"why\":{}}}",
                serde_json::to_string(reason).expect("a string serializes")
            ),
            "the template's own bytes are kept, and only the hole is filled"
        );
    }

    #[test]
    fn each_key_out_of_shape_is_named() {
        let with = |key: &str, value: &str| {
            let mut table: toml::Table = MAPPING.parse().expect("the mapping is TOML");
            if value.is_empty() {
                table.remove(key);
            } else {
                table.insert(
                    key.to_string(),
                    value.parse::<toml::Table>().expect("a value")["v"].clone(),
                );
            }
            HookMap::parse(&table).expect_err(key)
        };
        for (key, value, said) in [
            ("paths", "v = \"/p\"", "`paths` is not a key [hook] holds"),
            ("shell_tool", "", "`shell_tool` is missing"),
            ("shell_tool", "v = \" \"", "`shell_tool` is blank"),
            ("tool", "v = 1", "`tool` is not a string"),
            ("command", "", "`command` is missing"),
            (
                "command",
                "v = \"call/args/line\"",
                "`command` is `call/args/line`, which is not a JSON pointer",
            ),
            (
                "cwd",
                "v = \"/a~b\"",
                "`cwd` is `/a~b`, which is not a JSON pointer",
            ),
            ("deny", "", "`deny` is missing"),
            (
                "deny",
                "v = '{\"why\":\"refused\"}'",
                "`deny` holds no {reason}",
            ),
            (
                "deny",
                "v = '{\"why\":\"refused: {reason}\"}'",
                "`deny` is not JSON once {reason} is filled",
            ),
        ] {
            let error = with(key, value);
            assert!(error.starts_with(said), "{key} = {value}: {error}");
        }
        let optional = {
            let mut table: toml::Table = MAPPING.parse().expect("the mapping is TOML");
            table.remove("cwd");
            HookMap::parse(&table)
        };
        assert_eq!(optional.map(|map| map.cwd), Ok(None), "cwd is optional");
    }

    #[test]
    fn a_pointer_is_rfc_6901s() {
        for good in ["", "/", "/a/b", "/a~0b/~1c"] {
            assert!(is_pointer(good), "{good}");
        }
        for bad in ["a", "a/b", "/a~", "/a~2"] {
            assert!(!is_pointer(bad), "{bad}");
        }
    }
}
