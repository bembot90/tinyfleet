//! The record class: four checks over one command's text.
//!
//! The record is the work item, so what this class refuses is a write that
//! destroys one, reaches one with no audit row, puts an entry on one that no
//! verb wrote, or leaves one a later reader cannot resolve. Two of the four
//! FAIL CLOSED — a command the reader cannot read still reaches an
//! invocation-shaped text match, and it denies — because a destroyed field is
//! unrecoverable and a direct SQL write leaves nothing behind to find it by.
//! The other two have no fallback. The entry check reads the text of a
//! comment, and a command whose quoting the reader cannot close is one the
//! shell will not run either. The bare-id check is a legibility layer over the
//! record rather than its survival, and over-refusing there would block every
//! note in the fleet.

use super::lex::{basename, command_words, is_assignment, lex, statements, unquote, Token};
use super::{
    leading_escape, subcommand_of, text_arguments, Denial, Policy, ESCAPE_BARE_ID,
    ESCAPE_ENTRY_FORGE, ESCAPE_NOTES_REPLACE, ESCAPE_SQL_WRITE,
};
use crate::entry;

/// In the order the class runs them. The one legibility check is last, so an
/// entry written by hand is refused as that before any id in it is read.
pub const CHECKS: [&str; 4] = ["notes-replace", "sql-write", "entry-forge", "bare-id"];

/// The statement's first keyword decides, so a mention of one inside a literal,
/// a backtick identifier or a comment is not a write. A denylist: a statement
/// whose keyword is not here is a read and passes.
const SQL_WRITE_KEYWORDS: [&str; 7] = [
    "UPDATE", "DELETE", "INSERT", "REPLACE", "DROP", "ALTER", "TRUNCATE",
];

pub fn judge(command: &str, policy: &Policy) -> Option<Denial> {
    // Every check reads a call of the store's own command, so a store that
    // declares none has no call here to refuse.
    let cli = policy.cli.as_deref()?;
    let tokens = lex(command);

    if !leading_escape(command, ESCAPE_NOTES_REPLACE) {
        match &tokens {
            Ok(tokens) => {
                if let Some(flag) = notes_replace(tokens, cli) {
                    return Some(notes_denial(flag, false, cli));
                }
            }
            Err(_) => {
                if raw_invocation(command, cli, "update", &["--notes"]) {
                    return Some(notes_denial("--notes".to_string(), true, cli));
                }
            }
        }
    }

    if !leading_escape(command, ESCAPE_SQL_WRITE) {
        match &tokens {
            Ok(tokens) => {
                if let Some(keyword) = sql_write(tokens, cli) {
                    return Some(sql_denial(keyword, false, cli));
                }
            }
            Err(_) => {
                if raw_invocation(command, cli, "sql", &SQL_WRITE_KEYWORDS) {
                    return Some(sql_denial("a write keyword".to_string(), true, cli));
                }
            }
        }
    }

    // No fallback of its own either: a command the reader cannot read passes
    // this check.
    if !leading_escape(command, ESCAPE_ENTRY_FORGE) {
        if let Ok(tokens) = &tokens {
            if let Some(fragment) = entry_forge(tokens, cli) {
                return Some(forge_denial(fragment));
            }
        }
    }

    // No fallback of its own: a command the reader cannot read passes this
    // check, which is the direction it fails in.
    if !leading_escape(command, ESCAPE_BARE_ID) {
        if let (Ok(tokens), Some(prefix)) = (&tokens, policy.item_prefix.as_deref()) {
            if let Some(hits) = bare_ids(tokens, cli) {
                return Some(bare_denial(&hits, prefix));
            }
        }
    }

    None
}

// ---- 1. the flag that replaces the notes field ------------------------------

fn notes_replace(tokens: &[Token], cli: &str) -> Option<String> {
    for (words, _) in statements(tokens) {
        let current = command_words(&words);
        if subcommand_of(&current, Some(cli)) != Some("update".to_string()) {
            continue;
        }
        for word in &current[1..] {
            if word.text == "--notes" || word.text.starts_with("--notes=") {
                return Some(word.text.clone());
            }
        }
    }
    None
}

fn notes_denial(flag: String, unreadable: bool, cli: &str) -> Denial {
    Denial {
        class: "record",
        check: "notes-replace",
        label: if unreadable {
            "NOTES REPLACED (this command could not be read, so the conservative text match applied)"
        } else {
            "NOTES REPLACED"
        },
        fragment: flag,
        rewrite: format!(
            "append instead: `{cli} note <id> <text>`, or `{cli} update <id> --append-notes \
             <text>`"
        ),
        escape: Some(ESCAPE_NOTES_REPLACE),
        why: "this flag REPLACES the whole notes field, which is a person's own words on the \
              item and not recoverable from the write itself",
    }
}

// ---- 2. the SQL route around the audit row ---------------------------------

fn sql_write(tokens: &[Token], cli: &str) -> Option<String> {
    for (words, _) in statements(tokens) {
        let current = command_words(&words);
        if subcommand_of(&current, Some(cli)) != Some("sql".to_string()) {
            continue;
        }
        for word in &current[1..] {
            if word.text.starts_with('-') {
                continue;
            }
            for piece in &word.pieces {
                if let Some(keyword) = effective_keyword(&piece.1) {
                    return Some(keyword);
                }
            }
        }
    }
    None
}

/// The keyword that decides a statement, or `None` where it has no bare word.
///
/// Two shapes reach a write while opening with a keyword the rule calls a read,
/// and both are read here rather than left to pass: a LEADING COMMENT, whose
/// first keyword sits behind it, and a CTE, which opens on `WITH` and is
/// decided by the first write keyword anywhere after it. Paren depth is
/// deliberately not consulted — the only statement depth would exclude is a
/// data-modifying CTE, which is a write.
fn effective_keyword(statement: &str) -> Option<String> {
    let words: Vec<String> = sql_words(statement);
    let first = words.first()?;
    if SQL_WRITE_KEYWORDS.contains(&first.as_str()) {
        return Some(first.clone());
    }
    if first == "WITH" {
        return words
            .iter()
            .find(|w| SQL_WRITE_KEYWORDS.contains(&w.as_str()))
            .cloned();
    }
    None
}

/// Each bare word of the statement, upper-cased, in order. Stepped over rather
/// than read: quoted literals, backtick identifiers, `--` and `#` line comments
/// and `/* */` block comments. That is the whole of what keeps this from
/// refusing a read — a write keyword a statement merely MENTIONS lives in one
/// of those four, and a write keyword it PERFORMS cannot.
fn sql_words(statement: &str) -> Vec<String> {
    let chars: Vec<char> = statement.chars().collect();
    let length = chars.len();
    let mut out = Vec::new();
    let mut index = 0;
    while index < length {
        let char = chars[index];
        if char == '\'' || char == '"' || char == '`' {
            index += 1;
            while index < length && chars[index] != char {
                index += 1;
            }
            index += 1;
            continue;
        }
        if char == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < length && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(length);
            continue;
        }
        if char == '#' || (char == '-' && chars.get(index + 1) == Some(&'-')) {
            while index < length && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if char.is_ascii_alphabetic() || char == '_' {
            let start = index;
            while index < length && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            out.push(word.to_uppercase());
            continue;
        }
        index += 1;
    }
    out
}

fn sql_denial(keyword: String, unreadable: bool, cli: &str) -> Denial {
    Denial {
        class: "record",
        check: "sql-write",
        label: if unreadable {
            "SQL WRITE (this command could not be read, so the conservative text match applied)"
        } else {
            "SQL WRITE"
        },
        fragment: keyword,
        rewrite: format!(
            "make the change through the verb that owns it — `{cli} update`, `{cli} note`, \
             `{cli} close` — so the write carries its audit row"
        ),
        escape: Some(ESCAPE_SQL_WRITE),
        why: "the SQL route reaches the same fields with no confirmation and no audit row, so a \
              change made this way leaves nothing behind that a later reader could find it by",
    }
}

// ---- 3. an entry written by hand --------------------------------------------

/// The word that makes a statement an entry written by hand, or `None` where
/// no statement writes a comment or each one written is a person's own words.
///
/// A comment is where every entry lives, so the comment verbs are the one
/// route by which an entry can reach the record without the verb that owns its
/// act: `comments add`, and `comment`, its shorthand in the store's CLI. The text
/// is refused where it carries the entry key anywhere in it, and so is a text
/// the reader cannot see — one read from a file, or from the input.
fn entry_forge(tokens: &[Token], cli: &str) -> Option<String> {
    for (words, _) in statements(tokens) {
        let current = command_words(&words);
        let Some(arguments) = comment_arguments(&current, cli) else {
            continue;
        };
        for word in arguments {
            let text = word.text.as_str();
            if text.starts_with('-') {
                let unseen = (text.starts_with("-f") && !text.starts_with("--"))
                    || text == "--file"
                    || text.starts_with("--file=")
                    || text == "--stdin";
                if unseen {
                    return Some(word.text.clone());
                }
                continue;
            }
            if word.value().contains(entry::KEY) {
                return Some(word.text.clone());
            }
        }
    }
    None
}

/// Every word after the comment verb's own path, or `None` where the statement
/// is not a comment being written. `comments` with no `add` after it is the
/// listing, which writes nothing.
fn comment_arguments<'t>(current: &[&'t Token], cli: &str) -> Option<Vec<&'t Token>> {
    let verb = subcommand_of(current, Some(cli))?;
    let at = current[1..].iter().position(|w| !w.text.starts_with('-'))? + 1;
    match verb.as_str() {
        "comment" => Some(current[at + 1..].to_vec()),
        "comments" => {
            let add = current[at + 1..]
                .iter()
                .position(|w| !w.text.starts_with('-'))
                .map(|p| at + 1 + p)?;
            (current[add].text == "add").then(|| current[add + 1..].to_vec())
        }
        _ => None,
    }
}

fn forge_denial(fragment: String) -> Denial {
    Denial {
        class: "record",
        check: "entry-forge",
        label: "ENTRY FORGED",
        fragment,
        rewrite: String::from(
            "record through the verb that owns the act: fleet \
             dispatch|deliver|review|hold|clear|land|cancel",
        ),
        escape: Some(ESCAPE_ENTRY_FORGE),
        why: "an entry is the record every verb decides from; one written by hand skips the \
              checks its verb makes before it writes",
    }
}

// ---- 4. the item named by a bare suffix -------------------------------------

fn bare_ids(tokens: &[Token], cli: &str) -> Option<Vec<String>> {
    let mut found: Vec<String> = Vec::new();
    for (words, _) in statements(tokens) {
        let current = command_words(&words);
        for argument in text_arguments(&current, Some(cli)) {
            for token in bare_hits(&argument.value()) {
                if !found.contains(&token) {
                    found.push(token);
                }
            }
        }
    }
    if found.is_empty() {
        None
    } else {
        Some(found)
    }
}

/// Every bare suffix in one free text, in order.
///
/// WITHOUT AN ID ORACLE THE SHAPE IS THE WHOLE OF THE PRECISION, so the shape
/// is narrow: four characters of lowercase alphanumerics carrying at least one
/// digit and at least one letter, optionally followed by a dotted child number.
/// An all-digit run is a date or a count; an all-letter run is an ordinary
/// word; a longer run is neither this shape nor refusable without knowing the
/// items that exist. A live suffix outside that shape is missed, which is the
/// direction this check fails in.
fn bare_hits(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let length = chars.len();
    let mut found = Vec::new();
    let mut index = 0;
    while index < length {
        if !opens_a_token(&chars, index) {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < length && (chars[end].is_ascii_lowercase() || chars[end].is_ascii_digit()) {
            end += 1;
        }
        let stem: String = chars[index..end].iter().collect();
        let mut child_end = end;
        while chars.get(child_end) == Some(&'.')
            && chars.get(child_end + 1).is_some_and(char::is_ascii_digit)
        {
            child_end += 1;
            while chars.get(child_end).is_some_and(char::is_ascii_digit) {
                child_end += 1;
            }
        }
        if stem_is_a_suffix(&stem) && closes_a_token(&chars, child_end) {
            found.push(chars[index..child_end].iter().collect());
        }
        index = child_end.max(index + 1);
    }
    found
}

fn stem_is_a_suffix(stem: &str) -> bool {
    stem.chars().count() == 4
        && stem.chars().any(|c| c.is_ascii_digit())
        && stem.chars().any(|c| c.is_ascii_lowercase())
}

/// A token may not open behind an alphanumeric, an underscore, a dot, a slash
/// or a hyphen: the hyphen is what an id already written in full ends with, and
/// the slash is what a path or a branch name carries.
fn opens_a_token(chars: &[char], index: usize) -> bool {
    if !(chars[index].is_ascii_lowercase() || chars[index].is_ascii_digit()) {
        return false;
    }
    match index.checked_sub(1).map(|i| chars[i]) {
        None => true,
        Some(c) => !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '/' || c == '-'),
    }
}

/// A token may not close on an alphanumeric, an underscore, a slash, a hyphen,
/// or a dot followed by any of those: the last is a file extension, which is
/// what makes the note idiom's own filename pass.
fn closes_a_token(chars: &[char], end: usize) -> bool {
    match chars.get(end) {
        None => true,
        Some(c) => {
            if c.is_ascii_alphanumeric() || *c == '_' || *c == '/' || *c == '-' {
                return false;
            }
            if *c == '.' {
                return !chars
                    .get(end + 1)
                    .is_some_and(|n| n.is_ascii_alphanumeric() || *n == '_');
            }
            true
        }
    }
}

fn bare_denial(hits: &[String], prefix: &str) -> Denial {
    let named: Vec<String> = hits
        .iter()
        .map(|t| format!("{t} -> {prefix}-{t}"))
        .collect();
    Denial {
        class: "record",
        check: "bare-id",
        label: "AN ITEM NAMED BY ITS BARE SUFFIX",
        fragment: named.join(", "),
        rewrite: format!(
            "write the full id — {prefix}-<suffix> — so a reader can resolve it without knowing \
             which record it was written on"
        ),
        escape: Some(ESCAPE_BARE_ID),
        why: "a stored record is read years later and out of its own context, where a bare suffix \
              names nothing a reader can look up",
    }
}

// ---- the fallback's own reader ----------------------------------------------

/// The conservative text match, reached only for a command the reader could not
/// read. It is INVOCATION-SHAPED — a chunk whose own command word is the
/// store's `cli` — so prose that merely names the rule is not refused.
fn raw_invocation(command: &str, cli: &str, subcommand: &str, needles: &[&str]) -> bool {
    for line in command.lines() {
        for chunk in line.split(['(', ')', ';', '&', '|']) {
            let words: Vec<&str> = chunk.split_whitespace().collect();
            let mut index = 0;
            while index < words.len() && is_assignment(words[index]) {
                index += 1;
            }
            let Some(head) = words.get(index) else {
                continue;
            };
            if basename(head) != cli {
                continue;
            }
            // The quoting is what could not be read, so a word is compared with
            // its quotes off: the write keyword of an unterminated statement
            // arrives carrying the quote that never closed.
            let rest: Vec<&str> = words[index + 1..].iter().map(|w| unquote(w)).collect();
            if !rest.contains(&subcommand) {
                continue;
            }
            if rest.iter().any(|w| {
                needles
                    .iter()
                    .any(|n| w.eq_ignore_ascii_case(n) || w.starts_with(&format!("{n}=")))
            }) {
                return true;
            }
        }
    }
    false
}
