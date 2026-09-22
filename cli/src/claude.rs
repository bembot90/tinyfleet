//! The Claude adapter: this provider's pre-tool hook contract, and nothing
//! else.
//!
//! The payload a hook is handed and the document it reads a refusal out of are
//! ONE AGENT RUNTIME'S contract, not the fleet's — so they live here, beside
//! the overlay that wires them, and never in fleet-core. Core's guard module is
//! a function from a command string, a policy and the project keys to a
//! verdict; a second provider's overlay adds a reader of its own payload beside
//! this one and reuses those classes unchanged.
//!
//! THIS IS THE ONE FILE IN THE WORKSPACE THAT STATES A DECISION VALUE, and it
//! states the refusing one. An explicit allowing decision from a pre-tool hook
//! short-circuits the whole permission system, so letting a command through is
//! printing nothing at all.

/// What one payload says, or `None` where there is nothing to judge.
///
/// Four shapes are nothing to judge and each is silence: a body that is not
/// JSON, a tool that is not the shell one — said here as well as in the hook
/// document's matcher, where a mis-wired matcher cannot undo it — a payload
/// with no tool named at all, and a command that is absent, not a string, or
/// blank.
pub struct Payload {
    pub command: String,
    pub cwd: Option<String>,
}

pub fn payload(body: &str) -> Option<Payload> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    if parsed.get("tool_name")?.as_str()? != "Bash" {
        return None;
    }
    let command = parsed.get("tool_input")?.get("command")?.as_str()?;
    if command.trim().is_empty() {
        return None;
    }
    Some(Payload {
        command: command.to_string(),
        cwd: parsed
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// The refusal as this provider's hook contract carries it.
pub fn refusal(reason: &str) -> String {
    let body = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    });
    body.to_string()
}
