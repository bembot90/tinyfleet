//! The shell-trap class: five checks over one command's text.
//!
//! Each one produces a PLAUSIBLE WRONG ANSWER rather than an error, which is
//! why a written rule does not stop them: nothing at the prompt distinguishes
//! the trapped command from the correct one, so the seat reads a green that was
//! never earned. The refusal prints the rewrite, because a refusal a seat
//! cannot act on is one it routes around.
//!
//! The class fails OPEN throughout, and each table below says so in its own
//! terms: a command word a table has never heard of allows.

use super::lex::{basename, command_words, head_pair, lex, statements, Token};
use super::{Denial, ESCAPE_TRAP};

/// First hit wins, in this order — ordered by what the trap corrupts: the
/// stored record first, then an argument, then a verdict.
pub const CHECKS: [&str; 5] = [
    "record-backtick",
    "modifier",
    "unsplit-variable",
    "pipe-rc",
    "false-alternative",
];

/// The characters that ACT as a modifier after an unbraced `$VAR:`. The braced
/// form is literal for every one of them, which is what makes the rewrite the
/// refusal prints a command this class then allows.
const MODIFIER_CHARS: [char; 14] = [
    'a', 'c', 'e', 'h', 'l', 'q', 'r', 's', 't', 'u', 'A', 'P', 'Q', '&',
];

/// Pipeline stages whose exit status carries no information: they format what
/// reaches them and exit 0 whether the stage before them lived or died. An
/// ALLOWLIST of stages to refuse behind, never a denylist of stages to permit —
/// a last stage this table has never heard of allows, which is the direction
/// this class fails in.
///
/// grep, diff, cmp and test are NOT here and must not be: each reports a real
/// answer, so reading the status after one is the correct command and not the
/// trap. `cmd | grep -q x; echo $?` reads grep's own match status; `diff a b |
/// head; echo $?` reads head's, and is refused.
const FORMATTERS: [&str; 21] = [
    "head", "tail", "cat", "cut", "sed", "awk", "tr", "sort", "uniq", "fold", "tee", "nl", "rev",
    "column", "wc", "less", "more", "pr", "expand", "unexpand", "tac",
];

/// The commands whose whole job is to report an outcome, so one on the `||` arm
/// of an `A && B ||` chain turns A's ERROR into B's negative ANSWER.
const FALSE_ALTERNATIVES: [&str; 5] = ["echo", "printf", "true", ":", "print"];

/// The A commands of an `A && B || C` chain that HAVE a third answer — a "could
/// not tell" a CORRECT invocation reaches because of the world rather than
/// because of the command line. Each entry names the third status. The
/// two-word keys are read before the bare ones, so `git merge-tree` answers
/// while a bare `git` does not, and a command word absent from the table
/// allows: that is what keeps `test -e p && echo EXISTS || echo MISSING` — a
/// two-valued question whose `||` arm names its answer honestly — allowed by
/// construction rather than by an exception.
const THIRD_EXIT: [(&str, &str); 6] = [
    (
        "git merge-tree",
        "1 for a ref it cannot merge, which is also its conflict status",
    ),
    (
        "git merge-base",
        "a non-zero status that is not 1 for a ref it cannot resolve",
    ),
    ("grep", "above 1 when a file could not be read"),
    ("diff", "2 for trouble, where 1 is merely differs"),
    ("cmp", "2 for trouble, where 1 is merely differs"),
    (
        "curl",
        "a distinct number for each transport and protocol failure",
    ),
];

/// Commands every non-flag argument of which is one item of a LIST, so a bare
/// variable there is passed as a single item and matches or removes nothing.
/// The pathspec position — a word after a bare `--` — is the other arm, and it
/// is read for any command word.
const LIST_COMMANDS: [&str; 2] = ["xargs", "kill"];

/// The status read, in the spellings that reach it.
fn reads_status(body: &str) -> bool {
    body.contains("$?") || body.contains("${?}")
}

/// `cli` is the store's command word, [`super::Policy::cli`].
pub fn judge(command: &str, cli: Option<&str>) -> Option<Denial> {
    // Unreadable text allows: this class has no raw-text fallback, and prose
    // about these traps quotes every one of them by construction.
    let tokens = lex(command).ok()?;
    record_backtick(&tokens, cli)
        .or_else(|| modifier(&tokens))
        .or_else(|| unsplit_variable(&tokens))
        .or_else(|| pipe_rc(&tokens))
        .or_else(|| false_alternative(&tokens))
}

fn deny(
    check: &'static str,
    label: &'static str,
    fragment: String,
    rewrite: String,
    why: &'static str,
) -> Denial {
    Denial {
        class: "shell-trap",
        check,
        label,
        fragment,
        rewrite,
        escape: Some(ESCAPE_TRAP),
        why,
    }
}

// ---- 1. the backtick inside a stored string --------------------------------
//
// The calls this reads are the record class's own surfaces, out of the one
// table both classes share: the same argument a suffix has to be resolvable in
// is the one a backtick must not run in.

fn record_backtick(tokens: &[Token], cli: Option<&str>) -> Option<Denial> {
    for (words, _) in statements(tokens) {
        let current = command_words(&words);
        for token in super::text_arguments(&current, cli) {
            // An argument holding a substitution is skipped WHOLE: the stored
            // form is a quoted heredoc inside `"$(cat <file>)"`, whose body
            // carries parens and quotes of its own, and reading past them turns
            // every backtick in that prose into a hit.
            if token.carries_substitution() {
                continue;
            }
            for body in token.double_quoted() {
                if body.contains('`') {
                    return Some(deny(
                        "record-backtick",
                        "BACKTICK IN A RECORD STRING",
                        token.fragment(),
                        "write the text to a file through a QUOTED heredoc (<<'EOF') and pass \
                         \"$(cat <file>)\" — command substitution output is not re-scanned, so \
                         backticks inside the file are inert"
                            .to_string(),
                        "the shell runs a backtick inside double quotes as a command \
                         substitution, so the phrase vanishes from the stored record while the \
                         work graph prints its checkmark",
                    ));
                }
            }
        }
    }
    None
}

// ---- 2. the modifier after an unbraced name --------------------------------

fn modifier(tokens: &[Token]) -> Option<Denial> {
    for token in tokens {
        for body in token.expandable() {
            if let Some(name) = modifier_reference(&body) {
                return Some(deny(
                    "modifier",
                    "MODIFIER",
                    token.fragment(),
                    format!("brace the name so the colon is literal: \"${{{name}}}:<rest>\""),
                    "a colon straight after an UNBRACED name introduces a history modifier, even \
                     inside double quotes, so the expansion silently becomes the tail of the \
                     value or a path under the working directory",
                ));
            }
        }
    }
    None
}

/// The name of the first `$NAME:` whose colon is followed by a modifier
/// character. The braced form cannot match: after `$` this requires a name
/// character, and `{` is not one.
fn modifier_reference(body: &str) -> Option<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        if end < chars.len() && (chars[end].is_ascii_alphabetic() || chars[end] == '_') {
            end += 1;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
        }
        if end > start
            && chars.get(end) == Some(&':')
            && chars
                .get(end + 1)
                .is_some_and(|c| MODIFIER_CHARS.contains(c))
        {
            return Some(chars[start..end].iter().collect());
        }
        index = end.max(index + 1);
    }
    None
}

// ---- 3. the bare variable where a list is meant ----------------------------

fn unsplit_variable(tokens: &[Token]) -> Option<Denial> {
    for (words, _) in statements(tokens) {
        let current = command_words(&words);
        if let Some(hit) = for_loop_arm(&current).or_else(|| list_argument_arm(&current)) {
            return Some(hit);
        }
    }
    None
}

/// `for x in $LIST` — the loop that runs ONCE over the whole string.
///
/// The `in` list must be EXACTLY ONE word and that word must be the whole
/// variable reference: `for b in $A $B $C` over three separately-assigned
/// values is the "pass explicit arguments" form the rewrite itself prescribes.
fn for_loop_arm(current: &[&Token]) -> Option<Denial> {
    if current.first()?.text != "for" {
        return None;
    }
    let mut listed = Vec::new();
    let mut seen_in = false;
    for word in &current[1..] {
        if word.text == "in" {
            seen_in = true;
            continue;
        }
        if seen_in {
            listed.push(*word);
        }
    }
    if listed.len() != 1 {
        return None;
    }
    let word = listed[0];
    let name = whole_variable(&word.text)?;
    Some(unsplit_denial(word, &name))
}

/// A bare variable standing where a command takes a LIST of items: an argument
/// to one of the list commands, or a pathspec after a bare `--`.
fn list_argument_arm(current: &[&Token]) -> Option<Denial> {
    let head = basename(&current.first()?.text).to_string();
    let listed = LIST_COMMANDS.contains(&head.as_str());
    let mut after_separator = false;
    for word in &current[1..] {
        if word.text == "--" {
            after_separator = true;
            continue;
        }
        if !(listed || after_separator) {
            continue;
        }
        if word.text.starts_with('-') {
            continue;
        }
        if let Some(name) = whole_variable(&word.text) {
            return Some(unsplit_denial(word, &name));
        }
    }
    None
}

/// The name of a word that is NOTHING BUT `$VAR` or `${VAR}`, unquoted. The
/// shell's own explicit-split spellings, `$?`, `$1`, `$(…)` and a glob are all
/// out by construction, because each of them fails the shape rather than an
/// exception.
fn whole_variable(text: &str) -> Option<String> {
    let body = text.strip_prefix('$')?;
    let body = match body.strip_prefix('{') {
        Some(inner) => inner.strip_suffix('}')?,
        None => body,
    };
    if body.is_empty() {
        return None;
    }
    let mut chars = body.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(body.to_string())
}

fn unsplit_denial(word: &Token, name: &str) -> Denial {
    deny(
        "unsplit-variable",
        "UNSPLIT VARIABLE",
        word.fragment(),
        format!(
            "pass the items as explicit arguments, or read them one line at a time: \
             printf '%s\\n' \"${{{name}}}\" | while read -r item; do ...; done — and where ONE \
             argument really is intended, quote it: \"${{{name}}}\""
        ),
        "the shell passes an unquoted variable as ONE argument, so this iterates or matches once \
         over the whole newline-joined string and reads exactly like absence",
    )
}

// ---- 4. the status read behind a formatting pipe ---------------------------

fn pipe_rc(tokens: &[Token]) -> Option<Denial> {
    let parts = statements(tokens);
    for position in 0..parts.len() {
        if parts[position].1 != "|" {
            continue;
        }
        let mut end = position;
        while end < parts.len() && parts[end].1 == "|" {
            end += 1;
        }
        if end >= parts.len() {
            continue;
        }
        if !super::lex::PIPELINE_TERMINATORS.contains(&parts[end].1.as_str()) {
            continue;
        }
        if end + 1 >= parts.len() {
            continue;
        }
        if !swallows_status(&parts[end].0) {
            continue;
        }
        for token in &parts[end + 1].0 {
            for body in token.expandable() {
                if reads_status(&body) {
                    return Some(deny(
                        "pipe-rc",
                        "PIPE RC",
                        token.fragment(),
                        "run the command WITHOUT the pipe, redirecting to a file; read the status \
                         from that command's own $? ; then filter the file. Where any-stage \
                         failure is what you mean, `set -o pipefail` says so — second best, \
                         because it changes every pipeline in the shell"
                            .to_string(),
                        "the status after a pipeline is the LAST stage's, and a stage that only \
                         formats exits 0 whether the stage before it lived or died",
                    ));
                }
            }
        }
    }
    None
}

fn swallows_status(words: &[&Token]) -> bool {
    let current = command_words(words);
    match current.first() {
        Some(first) => FORMATTERS.contains(&basename(&first.text)),
        None => false,
    }
}

// ---- 5. the chain that reports a third answer as the negative one ----------

fn false_alternative(tokens: &[Token]) -> Option<Denial> {
    let parts = statements(tokens);
    for position in 0..parts.len() {
        if parts[position].1 != "||" {
            continue;
        }
        let Some(arming) = arming_and(&parts, position) else {
            continue;
        };
        if position + 1 >= parts.len() {
            continue;
        }
        let alternative = command_words(&parts[position + 1].0);
        let Some(first) = alternative.first() else {
            continue;
        };
        if !FALSE_ALTERNATIVES.contains(&first.text.as_str()) {
            continue;
        }
        let Some((name, third)) = third_exit_of(&parts[arming].0) else {
            continue;
        };
        let quoted: Vec<&str> = alternative.iter().map(|w| w.text.as_str()).collect();
        let quoted: String = quoted.join(" ").chars().take(40).collect();
        return Some(deny(
            "false-alternative",
            "FALSE-ALTERNATIVE CHAIN",
            format!("... || {quoted}"),
            format!(
                "split the three answers apart: if A; then B; else echo \"FAILED: <what could \
                 not be told>\"; exit 1; fi — because {name} exits {third}, and this chain \
                 reports that third answer as the negative one"
            ),
            "A && B || C runs C when A FAILED, so a command that errored is reported as the \
             negative outcome and the third answer is lost — and this A is one of the few that \
             HAS a third answer to lose",
        ));
    }
    None
}

/// The index of the statement whose `&&` opens the chain this `||` sits in.
/// Walks back over further `||`s only: a `;`, a newline or a paren ends the
/// chain, which is what keeps `A; B || C` — an ordinary fallback — allowed.
fn arming_and(parts: &[(Vec<&Token>, String)], position: usize) -> Option<usize> {
    let mut index = position;
    while index > 0 {
        index -= 1;
        match parts[index].1.as_str() {
            "&&" => return Some(index),
            "||" => continue,
            _ => return None,
        }
    }
    None
}

fn third_exit_of(words: &[&Token]) -> Option<(&'static str, &'static str)> {
    let current = command_words(words);
    let (head, argument) = head_pair(&current);
    let head = basename(&head?).to_string();
    if let Some(argument) = argument {
        let pair = format!("{head} {argument}");
        if let Some((name, third)) = THIRD_EXIT.iter().find(|(n, _)| *n == pair) {
            return Some((name, third));
        }
    }
    THIRD_EXIT
        .iter()
        .find(|(n, _)| *n == head)
        .map(|(n, t)| (*n, *t))
}
