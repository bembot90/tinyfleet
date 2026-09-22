//! The shell reader both guard classes judge through.
//!
//! It reads words and operators, and it keeps each word's QUOTING, because that
//! is the whole reason the classes lex rather than search: most of the checks
//! turn on whether a fragment sits inside double quotes, inside single quotes,
//! or outside both, and a scan over raw text cannot tell those apart.
//!
//! IT FAILS OPEN, and the caller is the half that makes that true: a text this
//! reader cannot read answers `Err`, the shell-trap class allows on it, and
//! there is no raw-text fallback for that class. The asymmetry is the damage —
//! a shell trap costs one wrong measurement a seat can re-take, while a guard
//! that refuses correct commands is one a seat prefixes its escape to by
//! reflex, which is worse than no guard. The record class's two fail-closed
//! checks carry their own raw-text fallback for the same `Err`, and so does the
//! production-write class, over the literal entries of the lists its project
//! declares.
//!
//! A `$(…)` region is one opaque piece: its interior is a command of its own,
//! and prose about these very traps is written through a quoted heredoc inside
//! one. A heredoc body is skipped rather than read as commands, because the
//! record form every seat is told to use writes its text through one.

/// How a piece of a word was quoted. The classes read `Bare` and `Double`
/// together as what the shell expands, and read `Double` alone where the check
/// is about a stored string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    Bare,
    Single,
    Double,
    Subst,
    Backtick,
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Word,
    Op,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    /// The raw text, exactly as it was typed, quotes included.
    pub text: String,
    pub pieces: Vec<(Quote, String)>,
}

impl Token {
    fn op(text: &str) -> Token {
        Token {
            kind: Kind::Op,
            text: text.to_string(),
            pieces: Vec::new(),
        }
    }

    /// The word's unquoted body alone.
    pub fn unquoted(&self) -> String {
        self.pieces
            .iter()
            .filter(|(q, _)| *q == Quote::Bare)
            .map(|(_, b)| b.as_str())
            .collect()
    }

    /// Each double-quoted body, separately: a stored string is one of these.
    pub fn double_quoted(&self) -> Vec<&str> {
        self.pieces
            .iter()
            .filter(|(q, _)| *q == Quote::Double)
            .map(|(_, b)| b.as_str())
            .collect()
    }

    /// The word's text with its quoting removed — what a call actually stores
    /// when the word is an argument. A `$(…)` region and a backtick are left
    /// out: the shell has not produced their text yet, so this reader cannot
    /// see it at all.
    pub fn value(&self) -> String {
        self.pieces
            .iter()
            .filter(|(q, _)| !matches!(q, Quote::Subst | Quote::Backtick))
            .map(|(_, b)| b.as_str())
            .collect()
    }

    /// The bodies the shell expands in this word's own context, with every
    /// `$(…)` region blanked. Single quotes and escapes are out because the
    /// fragments the checks look for are inert there; a substitution's interior
    /// is out because its own `$VAR:` and `$?` belong to it, and prose
    /// explaining these traps is written inside one.
    pub fn expandable(&self) -> Vec<String> {
        self.pieces
            .iter()
            .filter(|(q, _)| matches!(q, Quote::Bare | Quote::Double))
            .map(|(_, b)| blank_substitutions(b))
            .collect()
    }

    /// Whether the word holds a command substitution anywhere in its raw text.
    /// Such an argument is skipped WHOLE rather than having the region cut out:
    /// the record form is a quoted heredoc inside `"$(cat <file>)"`, whose body
    /// routinely carries an unmatched paren and an unmatched quote.
    pub fn carries_substitution(&self) -> bool {
        self.text.contains("$(")
    }

    /// The word as a refusal quotes it: raw text, whitespace collapsed, elided.
    pub fn fragment(&self) -> String {
        let text: Vec<&str> = self.text.split_whitespace().collect();
        let text = text.join(" ");
        if text.chars().count() > 60 {
            let head: String = text.chars().take(57).collect();
            format!("{head}...")
        } else {
            text
        }
    }
}

/// A text the reader could not read. Each class decides what it means, and they
/// do not all decide the same way: three of them allow, and the record class is
/// the one that falls back to a conservative text match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    UnterminatedQuote(char),
    UnterminatedSubstitution,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexError::UnterminatedQuote(c) => write!(f, "a {c} quote that never closes"),
            LexError::UnterminatedSubstitution => write!(f, "a $( that never closes"),
        }
    }
}

/// Longest first, so `&&` never reads as two `&`.
const OPERATORS: [&str; 9] = ["&&", "||", "|&", ";;", ";", "|", "&", "(", ")"];

/// What ends a pipeline rather than continuing it. The empty string is the end
/// of the command text.
pub const PIPELINE_TERMINATORS: [&str; 5] = [";", "\n", "&&", "||", ""];

pub fn lex(command: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = command.chars().collect();
    let length = chars.len();
    let mut tokens: Vec<Token> = Vec::new();
    let mut pieces: Vec<(Quote, String)> = Vec::new();
    let mut raw = String::new();
    let mut heredocs: Vec<String> = Vec::new();
    let mut expect_delimiter = false;
    let mut index = 0;

    while index < length {
        let char = chars[index];

        if char == '\\' && index + 1 < length {
            let following = chars[index + 1];
            if following == '\n' {
                index += 2;
                continue;
            }
            add(
                &mut pieces,
                &mut raw,
                Quote::Escape,
                &following.to_string(),
                &slice(&chars, index, index + 2),
            );
            index += 2;
            continue;
        }

        if char == '\'' {
            let Some(end) = find(&chars, '\'', index + 1) else {
                return Err(LexError::UnterminatedQuote('\''));
            };
            add(
                &mut pieces,
                &mut raw,
                Quote::Single,
                &slice(&chars, index + 1, end),
                &slice(&chars, index, end + 1),
            );
            index = end + 1;
            continue;
        }

        if char == '"' {
            let mut body = String::new();
            let mut cursor = index + 1;
            let mut closed = false;
            while cursor < length {
                if chars[cursor] == '\\' && cursor + 1 < length {
                    // An escaped character is INERT, so it is replaced by a
                    // space rather than kept: a backslash-escaped backtick
                    // inside double quotes does not run, and a body that kept
                    // the backtick would read as the trap.
                    body.push(' ');
                    cursor += 2;
                    continue;
                }
                if starts_with(&chars, cursor, "$(") {
                    let end = skip_substitution(&chars, cursor)?;
                    body.push_str(&slice(&chars, cursor, end));
                    cursor = end;
                    continue;
                }
                if chars[cursor] == '"' {
                    closed = true;
                    break;
                }
                body.push(chars[cursor]);
                cursor += 1;
            }
            if !closed {
                return Err(LexError::UnterminatedQuote('"'));
            }
            add(
                &mut pieces,
                &mut raw,
                Quote::Double,
                &body,
                &slice(&chars, index, cursor + 1),
            );
            index = cursor + 1;
            continue;
        }

        if starts_with(&chars, index, "$(") {
            let end = skip_substitution(&chars, index)?;
            let text = slice(&chars, index, end);
            add(&mut pieces, &mut raw, Quote::Subst, &text, &text);
            index = end;
            continue;
        }

        if char == '`' {
            let Some(end) = find(&chars, '`', index + 1) else {
                return Err(LexError::UnterminatedQuote('`'));
            };
            let text = slice(&chars, index, end + 1);
            add(&mut pieces, &mut raw, Quote::Backtick, &text, &text);
            index = end + 1;
            continue;
        }

        if char == '\n' {
            flush(
                &mut tokens,
                &mut pieces,
                &mut raw,
                &mut heredocs,
                &mut expect_delimiter,
            );
            tokens.push(Token::op("\n"));
            index = skip_heredoc_bodies(&chars, index + 1, &mut heredocs);
            continue;
        }

        if char == ' ' || char == '\t' {
            flush(
                &mut tokens,
                &mut pieces,
                &mut raw,
                &mut heredocs,
                &mut expect_delimiter,
            );
            index += 1;
            continue;
        }

        // A brace is grouping only where it stands alone as a word. Reading
        // every brace as an operator would split `${VAR}` down the middle,
        // which is the one expansion three of these checks are about.
        if (char == '{' || char == '}') && raw.is_empty() && standalone(&chars, index) {
            tokens.push(Token::op(&char.to_string()));
            index += 1;
            continue;
        }

        // `2>&1`, `>&2` and `>|` carry an operator character inside a
        // REDIRECTION, where it separates nothing. Reading the `&` of `2>&1` as
        // a background operator splits one statement into three and moves every
        // later statement's index, which is what two of the checks count with.
        if (char == '&' || char == '|')
            && !starts_with(&chars, index, "&&")
            && !starts_with(&chars, index, "||")
            && ((index > 0 && (chars[index - 1] == '>' || chars[index - 1] == '<'))
                || chars.get(index + 1) == Some(&'>'))
        {
            add(
                &mut pieces,
                &mut raw,
                Quote::Bare,
                &char.to_string(),
                &char.to_string(),
            );
            index += 1;
            continue;
        }

        let mut operator = None;
        for candidate in OPERATORS {
            if starts_with(&chars, index, candidate) {
                operator = Some(candidate);
                break;
            }
        }
        if let Some(operator) = operator {
            flush(
                &mut tokens,
                &mut pieces,
                &mut raw,
                &mut heredocs,
                &mut expect_delimiter,
            );
            tokens.push(Token::op(operator));
            index += operator.chars().count();
            continue;
        }

        add(
            &mut pieces,
            &mut raw,
            Quote::Bare,
            &char.to_string(),
            &char.to_string(),
        );
        index += 1;
    }

    flush(
        &mut tokens,
        &mut pieces,
        &mut raw,
        &mut heredocs,
        &mut expect_delimiter,
    );
    Ok(tokens)
}

fn add(
    pieces: &mut Vec<(Quote, String)>,
    raw: &mut String,
    quote: Quote,
    body: &str,
    raw_text: &str,
) {
    match pieces.last_mut() {
        Some((Quote::Bare, held)) if quote == Quote::Bare => held.push_str(body),
        _ => pieces.push((quote, body.to_string())),
    }
    raw.push_str(raw_text);
}

fn flush(
    tokens: &mut Vec<Token>,
    pieces: &mut Vec<(Quote, String)>,
    raw: &mut String,
    heredocs: &mut Vec<String>,
    expect_delimiter: &mut bool,
) {
    if raw.is_empty() && pieces.is_empty() {
        return;
    }
    let text = std::mem::take(raw);
    note_heredoc(&text, heredocs, expect_delimiter);
    tokens.push(Token {
        kind: Kind::Word,
        text,
        pieces: std::mem::take(pieces),
    });
}

/// A heredoc redirection names a delimiter, in the word itself (`<<'EOF'`) or
/// in the word after it (`<< EOF`).
fn note_heredoc(text: &str, heredocs: &mut Vec<String>, expect_delimiter: &mut bool) {
    if *expect_delimiter {
        heredocs.push(text.trim_matches(|c| c == '\'' || c == '"').to_string());
        *expect_delimiter = false;
        return;
    }
    let Some(rest) = text.strip_prefix("<<") else {
        return;
    };
    // `<<<` is a here-STRING, which has no body to skip.
    if rest.starts_with('<') {
        return;
    }
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    if rest.is_empty() {
        *expect_delimiter = true;
    } else {
        heredocs.push(rest.trim_matches(|c| c == '\'' || c == '"').to_string());
    }
}

fn skip_heredoc_bodies(chars: &[char], mut index: usize, heredocs: &mut Vec<String>) -> usize {
    let length = chars.len();
    while !heredocs.is_empty() {
        let delimiter = heredocs.remove(0);
        while index < length {
            let newline = find(chars, '\n', index);
            let line = slice(chars, index, newline.unwrap_or(length));
            index = match newline {
                Some(n) => n + 1,
                None => length,
            };
            if line.trim() == delimiter {
                break;
            }
        }
    }
    index
}

/// The index just past the `$(…)` opening at `index`.
///
/// Paren depth, but not paren depth ALONE: the form every seat is told to write
/// stored text through is `"$(cat <<'EOF' … EOF )"`, and a heredoc body
/// routinely carries an unmatched paren and an unmatched quote. A bare depth
/// counter closes the substitution somewhere inside the prose and the rest of
/// the note then reads as commands.
fn skip_substitution(chars: &[char], index: usize) -> Result<usize, LexError> {
    let length = chars.len();
    let mut cursor = index + 2;
    let mut depth = 1;
    let mut heredocs: Vec<String> = Vec::new();
    while cursor < length && depth > 0 {
        let char = chars[cursor];
        if char == '\\' && cursor + 1 < length {
            cursor += 2;
            continue;
        }
        if char == '\'' {
            let Some(end) = find(chars, '\'', cursor + 1) else {
                return Err(LexError::UnterminatedQuote('\''));
            };
            cursor = end + 1;
            continue;
        }
        if char == '"' {
            cursor += 1;
            let mut closed = false;
            while cursor < length {
                if chars[cursor] == '\\' && cursor + 1 < length {
                    cursor += 2;
                    continue;
                }
                if chars[cursor] == '"' {
                    closed = true;
                    break;
                }
                cursor += 1;
            }
            if !closed {
                return Err(LexError::UnterminatedQuote('"'));
            }
            cursor += 1;
            continue;
        }
        if let Some((delimiter, end)) = heredoc_redirect(chars, cursor) {
            heredocs.push(delimiter);
            cursor = end;
            continue;
        }
        if char == '\n' && !heredocs.is_empty() {
            cursor = skip_heredoc_bodies(chars, cursor + 1, &mut heredocs);
            continue;
        }
        if char == '(' {
            depth += 1;
        } else if char == ')' {
            depth -= 1;
        }
        cursor += 1;
    }
    if depth > 0 {
        return Err(LexError::UnterminatedSubstitution);
    }
    Ok(cursor)
}

/// `<<EOF`, `<<'EOF'` and `<<"EOF"` matched mid-text, where there are no lexed
/// words to read a redirection off.
fn heredoc_redirect(chars: &[char], index: usize) -> Option<(String, usize)> {
    if !starts_with(chars, index, "<<") || chars.get(index + 2) == Some(&'<') {
        return None;
    }
    let mut cursor = index + 2;
    if chars.get(cursor) == Some(&'-') {
        cursor += 1;
    }
    while chars.get(cursor) == Some(&' ') || chars.get(cursor) == Some(&'\t') {
        cursor += 1;
    }
    let quote = match chars.get(cursor) {
        Some('\'') => Some('\''),
        Some('"') => Some('"'),
        _ => None,
    };
    if quote.is_some() {
        cursor += 1;
    }
    let start = cursor;
    while cursor < chars.len() && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_') {
        cursor += 1;
    }
    if cursor == start {
        return None;
    }
    let delimiter = slice(chars, start, cursor);
    if quote.is_some() {
        cursor += 1;
    }
    Some((delimiter, cursor))
}

/// The body with every `$(…)` region replaced by one space.
fn blank_substitutions(body: &str) -> String {
    if !body.contains("$(") {
        return body.to_string();
    }
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if starts_with(&chars, index, "$(") {
            match skip_substitution(&chars, index) {
                Ok(end) => {
                    out.push(' ');
                    index = end;
                    continue;
                }
                // The word already lexed, so an unclosed region here is the
                // tail of a piece rather than unreadable text: blank the rest.
                Err(_) => {
                    out.push(' ');
                    break;
                }
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Whether the brace at `index` stands alone as a word.
fn standalone(chars: &[char], index: usize) -> bool {
    match chars.get(index + 1) {
        None => true,
        Some(c) => matches!(c, ' ' | '\t' | '\n' | ';'),
    }
}

fn starts_with(chars: &[char], index: usize, needle: &str) -> bool {
    let needle: Vec<char> = needle.chars().collect();
    if index + needle.len() > chars.len() {
        return false;
    }
    chars[index..index + needle.len()] == needle[..]
}

fn find(chars: &[char], needle: char, from: usize) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == needle)
}

fn slice(chars: &[char], from: usize, to: usize) -> String {
    let to = to.min(chars.len());
    if from >= to {
        return String::new();
    }
    chars[from..to].iter().collect()
}

/// One entry per statement: its words, and the operator that ended it. The
/// trailing entry's operator is empty — the command simply ended. Both halves
/// are read: the false-alternative check reads the operators to see an
/// `&& … ||` chain, and the pipe-rc check reads them to find where a pipeline
/// stops.
pub fn statements(tokens: &[Token]) -> Vec<(Vec<&Token>, String)> {
    let mut out = Vec::new();
    let mut current: Vec<&Token> = Vec::new();
    for token in tokens {
        if token.kind == Kind::Op {
            out.push((std::mem::take(&mut current), token.text.clone()));
        } else {
            current.push(token);
        }
    }
    out.push((current, String::new()));
    out
}

/// A `VAR=value` prefix, stepped over when deciding what a statement's command
/// word is.
pub fn is_assignment(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for (offset, c) in text.char_indices().skip(1) {
        if c == '=' {
            return offset > 0;
        }
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    false
}

const WRAPPERS: [&str; 10] = [
    "env", "command", "builtin", "exec", "sudo", "nohup", "time", "then", "do", "else",
];

/// The words of a statement from its command word on, or empty.
pub fn command_words<'t>(words: &[&'t Token]) -> Vec<&'t Token> {
    let mut index = 0;
    while index < words.len()
        && (is_assignment(&words[index].text) || WRAPPERS.contains(&words[index].text.as_str()))
    {
        index += 1;
    }
    words[index..].to_vec()
}

/// (command word, first non-flag argument) for a statement.
pub fn head_pair(words: &[&Token]) -> (Option<String>, Option<String>) {
    let Some(head) = words.first() else {
        return (None, None);
    };
    for word in &words[1..] {
        if word.text.starts_with('-') {
            continue;
        }
        return (Some(head.text.clone()), Some(word.text.clone()));
    }
    (Some(head.text.clone()), None)
}

/// A word with the quoting a fallback could not read trimmed off it. The
/// quoting is exactly what failed, so the word arrives carrying the quote that
/// never closed.
pub fn unquote(word: &str) -> &str {
    word.trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == ',')
}

/// The last path segment of a command word, so an absolute path answers like
/// the bare name.
pub fn basename(text: &str) -> &str {
    text.rsplit('/').next().unwrap_or(text)
}
