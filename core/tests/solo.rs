//! The `SOLO` table names every rig that asserts on the WHOLE board.
//!
//! A rig that asks the board for a SET and asserts on the SIZE or the exact
//! contents of that set cannot share a board: another rig's rows move the
//! answer. A rig that asks for the same set and asserts only that its own row
//! is in it can share, because a neighbour's rows do not change that answer.
//! `common/board.rs` decides which is which by a hand-kept table, and this arm
//! is what says the table still covers the suites.
//!
//! Nothing here is a list of rigs. The three inputs are read from the tree:
//!
//! 1. THE SET READS are the `Store` trait's methods that answer a `Result<Vec>`
//!    — a set the whole board contributes to, as against `show`, which answers
//!    one item the caller named. A list scoped to one named item — its
//!    `timeline`, whose first argument is `item` — is `show`'s kind and not a
//!    set read: no neighbour's rows reach an item the rig filed itself. A
//!    fifth such method added to the trait is picked up here without an edit.
//! 2. THE BOARD-TAKING CONSTRUCTORS are the functions in a crate's test
//!    `common` module that reach `run_board`, to a fixpoint, so a helper added
//!    in front of it is followed. A crate whose `common` reaches `run_board`
//!    through nothing takes no run board and contributes no label.
//! 3. THE TABLE is parsed out of `common/board.rs`.
//!
//! The classifier is pinned from both ends by the tree itself: it must answer
//! WHOLE-BOARD for at least one site and MEMBERSHIP for at least one site, so a
//! classifier that has gone blind and one that flags everything both fail here
//! rather than passing quietly.
//!
//! WHAT IT DOES NOT SEE: a rig whose whole-board dependence lives inside the
//! verb it drives — the cap a takeoff measures its overlap against — reads no
//! set in its own arm, so nothing here finds it. This says an entry is not
//! MISSING from the table; it never says one is spare.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `fleet/`, the workspace this crate sits in.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the core crate sits inside the workspace")
        .to_path_buf()
}

/// The workspace members whose test trees hold rigs.
const CRATES: [&str; 2] = ["core", "cli"];

/// What a rig's arms say about a set the board answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    /// The size or the exact contents of the set — another rig's rows move it.
    WholeBoard,
    /// Whether one named row is in the set — a neighbour's rows do not move it.
    Membership,
    /// The set was read and this arm cannot tell what was asserted about it.
    Unclear,
}

/// Markers that make a claim about the set as a whole.
const WHOLE: [&str; 7] = [
    ".len()",
    ".is_empty()",
    ".count()",
    ".first()",
    ".last()",
    ".as_slice()",
    ".into_iter().collect",
];

/// Markers that make a claim about one row's presence.
const MEMBER: [&str; 4] = [".contains(", ".any(", ".find(", ".position("];

const ASSERTS: [&str; 3] = ["assert!(", "assert_eq!(", "assert_ne!("];

/// One set read, where it is and what its arm says about it.
#[derive(Debug)]
struct Site {
    file: String,
    line: usize,
    read: String,
    labels: BTreeSet<String>,
    verdict: Verdict,
}

#[test]
fn every_rig_that_asserts_on_the_whole_board_is_in_the_solo_table() {
    let root = workspace();

    let reads = set_reads(&read(&root.join("core/src/store/mod.rs")));
    assert!(
        reads.contains("ready") && reads.contains("open_holds"),
        "the set reads are parsed off the trait: {reads:?}"
    );
    assert!(
        !reads.contains("show") && !reads.contains("create") && !reads.contains("timeline"),
        "and an item-scoped read is not one of them: {reads:?}"
    );

    let board = read(&root.join("core/tests/common/board.rs"));
    let solo = solo_table(&board);
    assert!(
        !solo.is_empty(),
        "the SOLO table is parsed out of common/board.rs"
    );

    let mut sites: Vec<Site> = Vec::new();
    let mut shadowed: Vec<String> = Vec::new();
    let mut ctors_by_crate: Vec<String> = Vec::new();
    for krate in CRATES {
        let rigs = root.join(krate).join("tests");
        let ctors = board_ctors(&read(&rigs.join("common/mod.rs")));
        ctors_by_crate.push(format!("{krate}: {ctors:?}"));
        for file in rig_files(&rigs) {
            let name = format!(
                "{krate}/tests/{}",
                file.file_name().expect("a file name").to_string_lossy()
            );
            let text = read(&file);
            let live: BTreeSet<String> = ctors
                .iter()
                .filter(|ctor| match ctor.split_once("::") {
                    Some((ty, _)) if text.contains(&format!("impl {ty} ")) => {
                        shadowed.push(format!("{name} defines its own `{ty}`"));
                        false
                    }
                    _ => true,
                })
                .cloned()
                .collect();
            sites.extend(sites_in(&name, &text, &reads, &live));
        }
    }

    let table: Vec<String> = sites
        .iter()
        .map(|s| {
            let labels = if s.labels.is_empty() {
                String::from("(no run board)")
            } else {
                s.labels.iter().cloned().collect::<Vec<_>>().join(", ")
            };
            format!(
                "{:<28} {:>5}  {:<14} {:?}  {labels}",
                s.file, s.line, s.read, s.verdict
            )
        })
        .collect();
    let table = format!(
        "SOLO = {solo:?}\nboard-taking constructors = {ctors_by_crate:?}\n{}\nshadowed: {shadowed:?}",
        table.join("\n")
    );
    println!("{table}");

    // The control, and the reason a green here is not vacuous: both answers are
    // reachable on this tree. A classifier that called everything membership
    // would pass the subject below while seeing nothing, and one that called
    // everything whole-board would report every rig. Neither gets past this.
    let whole = sites
        .iter()
        .filter(|s| s.verdict == Verdict::WholeBoard)
        .count();
    let member = sites
        .iter()
        .filter(|s| s.verdict == Verdict::Membership)
        .count();
    assert!(
        whole > 0 && member > 0,
        "the classifier answers both ways on this tree — whole-board {whole}, membership {member}\n{table}"
    );

    let unclear: Vec<&Site> = sites
        .iter()
        .filter(|s| s.verdict == Verdict::Unclear && !s.labels.is_empty())
        .collect();
    assert!(
        unclear.is_empty(),
        "a rig on a run board reads a set and this arm cannot tell what it asserts — read it and \
         say so, because an unread site is not a safe one: {unclear:#?}\n{table}"
    );

    let missing: Vec<&Site> = sites
        .iter()
        .filter(|s| s.verdict == Verdict::WholeBoard && s.labels.iter().any(|l| !solo.contains(l)))
        .collect();
    assert!(
        missing.is_empty(),
        "these rigs assert on the whole board and are not in `SOLO` in core/tests/common/board.rs, \
         so they share the run's board and their answer moves with a neighbour's rows: \
         {missing:#?}\n{table}"
    );
}

// ---- the tree's three inputs -------------------------------------------------

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|why| panic!("{} is readable: {why}", path.display()))
}

/// Every `*.rs` directly under a crate's `tests/`, excluding this file and the
/// `common` module. The set is the directory's, never a list kept here.
fn rig_files(rigs: &Path) -> Vec<PathBuf> {
    let me = Path::new(file!())
        .file_name()
        .expect("this file has a name")
        .to_owned();
    let mut files: Vec<PathBuf> = std::fs::read_dir(rigs)
        .unwrap_or_else(|why| panic!("{} is readable: {why}", rigs.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .filter(|path| path.file_name() != Some(&me))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "{} holds rigs", rigs.display());
    files
}

/// The `Store` trait's methods that answer a set: a `Result<Vec<..>>` return,
/// from a method whose first argument is not the one `item` it is scoped to.
fn set_reads(store: &str) -> BTreeSet<String> {
    let code = code_only(store);
    let Some(open) = code.find("pub trait Store {") else {
        panic!("core/src/store/mod.rs declares `pub trait Store`");
    };
    let body = &code[open..];
    let end = body
        .find("\n}")
        .expect("the trait block closes at column 0");
    let mut names = BTreeSet::new();
    for piece in body[..end].split("    fn ").skip(1) {
        let head = piece.split(';').next().unwrap_or(piece);
        let item_scoped = head.contains("(&self, item: &str");
        if head.contains("-> Result<Vec<") && !item_scoped {
            names.insert(head[..head.find('(').unwrap_or(0)].to_string());
        }
    }
    names
}

/// The labels in `const SOLO: [&str; N] = [..];`, with N read as its own check.
fn solo_table(board: &str) -> BTreeSet<String> {
    let code = code_only(board);
    let at = code
        .find("const SOLO:")
        .expect("common/board.rs declares SOLO");
    let declared: usize = board[at..]
        .split_once("; ")
        .and_then(|(_, rest)| rest.split(']').next())
        .and_then(|n| n.trim().parse().ok())
        .expect("the SOLO table declares its length");
    let open = at + code[at..].find("= [").expect("the table opens") + 2;
    let close = open + code[open..].find(']').expect("the table closes");
    let labels: BTreeSet<String> = string_literals(board, open, close);
    assert_eq!(
        labels.len(),
        declared,
        "the SOLO table's declared length is the number of labels in it: {labels:?}"
    );
    labels
}

/// `run_board` and the functions of a crate's test `common` module that reach
/// it, to a fixpoint — so a rig that calls it straight and a rig that calls a
/// helper put in front of it are both followed.
fn board_ctors(common: &str) -> BTreeSet<String> {
    let mut found: BTreeSet<String> = BTreeSet::from([String::from("run_board")]);
    let fns = functions(common);
    loop {
        let mut grew = false;
        for (name, body) in &fns {
            if found.contains(name) {
                continue;
            }
            let reached = found.iter().any(|f| {
                let bare = f.rsplit("::").next().unwrap_or(f);
                body.contains(&format!("{f}(")) || body.contains(&format!("Self::{bare}("))
            });
            if reached {
                found.insert(name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    found
}

// ---- the rigs ----------------------------------------------------------------

/// Every set read in one rig file, with the labels its arm takes and what the
/// arm asserts about the set.
fn sites_in(
    name: &str,
    text: &str,
    reads: &BTreeSet<String>,
    ctors: &BTreeSet<String>,
) -> Vec<Site> {
    let code = code_only(text);
    let spans = fn_spans(&code);
    let arms: Vec<&(String, usize, usize, bool)> = spans.iter().filter(|s| s.3).collect();
    assert_eq!(
        arms.len(),
        code.matches("#[test]").count(),
        "{name}: every `#[test]` in the file is one parsed arm — a mismatch means the item spans \
         were misread and nothing below can be trusted"
    );

    // A label named outside every arm belongs to the file's helpers, and so to
    // any arm that names none of its own.
    let outside: BTreeSet<String> = labels_in(&code, text, ctors, 0, code.len())
        .difference(
            &arms
                .iter()
                .flat_map(|(_, from, to, _)| labels_in(&code, text, ctors, *from, *to))
                .collect(),
        )
        .cloned()
        .collect();

    let mut sites = Vec::new();
    for read in reads {
        let needle = format!(".{read}(");
        for (at, _) in code.match_indices(&needle) {
            let Some((owner, from, to, is_arm)) = innermost(&spans, at) else {
                continue;
            };
            // A delegating or stubbed implementation of the read itself is not
            // an assertion about a board.
            if reads.contains(owner.rsplit("::").next().unwrap_or(owner)) {
                continue;
            }
            // A helper is called from any arm in the file, so its reads run on
            // every board the file takes. An arm's own label wins for itself,
            // and an arm that names none takes what its file's helpers made.
            let own = labels_in(&code, text, ctors, from, to);
            let labels = match (is_arm, own.is_empty()) {
                (true, false) => own,
                (true, true) => outside.clone(),
                (false, _) => labels_in(&code, text, ctors, 0, code.len()),
            };
            sites.push(Site {
                file: name.to_string(),
                line: code[..at].matches('\n').count() + 1,
                read: read.clone(),
                labels,
                verdict: verdict(&code[from..to], at - from),
            });
        }
    }
    sites
}

/// The labels a board-taking constructor is called with inside `[from, to)`.
///
/// The call sites are found in the blanked copy and the label is read off the
/// source, which the blanking leaves byte-for-byte the same length.
fn labels_in(
    code: &str,
    text: &str,
    ctors: &BTreeSet<String>,
    from: usize,
    to: usize,
) -> BTreeSet<String> {
    let mut labels = BTreeSet::new();
    for ctor in ctors {
        let needle = format!("{ctor}(");
        for (at, _) in code[from..to].match_indices(&needle) {
            let open = from + at + needle.len() - 1;
            let close = open + code[open..].find(')').unwrap_or(0);
            labels.extend(string_literals(text, open, close));
        }
    }
    labels
}

/// What the arm asserts about the set the read at `at` answered.
fn verdict(body: &str, at: usize) -> Verdict {
    let stmt = statement(body, at);
    if ASSERTS.iter().any(|a| body[stmt.0..stmt.1].contains(a)) {
        return judge(&body[stmt.0..stmt.1], None);
    }
    let Some(bound) = binding(&body[stmt.0..stmt.1]) else {
        return Verdict::Unclear;
    };
    let claims: String = statements(body)
        .filter(|(from, to)| {
            mentions(&body[*from..*to], &bound)
                && ASSERTS.iter().any(|a| body[*from..*to].contains(a))
        })
        .map(|(from, to)| &body[from..to])
        .collect::<Vec<_>>()
        .join("\n");
    if claims.is_empty() {
        return Verdict::Unclear;
    }
    judge(&claims, Some(&bound))
}

/// An equality against the set itself claims its exact contents, which is the
/// same claim as its size; a claim about one row's presence is not.
fn judge(text: &str, subject: Option<&str>) -> Verdict {
    if WHOLE.iter().any(|m| text.contains(m)) {
        return Verdict::WholeBoard;
    }
    if subject.is_some_and(|name| equated_whole(text, name)) {
        return Verdict::WholeBoard;
    }
    if MEMBER.iter().any(|m| text.contains(m)) {
        Verdict::Membership
    } else {
        Verdict::Unclear
    }
}

/// `assert_eq!(<set>, ..)`, reading through the unwrap the read needs.
fn equated_whole(text: &str, name: &str) -> bool {
    text.split("assert_eq!(").skip(1).any(|rest| {
        let left = rest.split(',').next().unwrap_or("");
        let left = left.trim().trim_start_matches('&');
        left.strip_prefix(name).is_some_and(|tail| {
            tail.is_empty() || tail.starts_with(".expect(") || tail.starts_with(".unwrap(")
        })
    })
}

/// The name a `let` statement binds, where the statement is one.
fn binding(stmt: &str) -> Option<String> {
    let rest = stmt.trim_start().strip_prefix("let ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

fn mentions(text: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back().is_none_or(|c| !word(c));
        let after = text[at + name.len()..]
            .chars()
            .next()
            .is_none_or(|c| !word(c));
        before && after
    })
}

// ---- reading Rust without a parser -------------------------------------------

/// The source with every comment, string and character literal blanked out and
/// every other byte where it was, so a `}` in a message is never read as code.
fn code_only(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with("//") {
            let end = rest.find('\n').map_or(bytes.len(), |n| i + n);
            blank_keeping_lines(&mut out, bytes, i, end);
            i = end;
        } else if rest.starts_with("/*") {
            let mut depth = 1;
            let mut j = i + 2;
            while j < bytes.len() && depth > 0 {
                if text[j..].starts_with("/*") {
                    depth += 1;
                    j += 2;
                } else if text[j..].starts_with("*/") {
                    depth -= 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            blank_keeping_lines(&mut out, bytes, i, j);
            i = j;
        } else if let Some(hashes) = raw_string(rest) {
            let close = format!("\"{}", "#".repeat(hashes));
            let from = i + rest.find('"').expect("a raw string opens") + 1;
            let end = text[from..]
                .find(&close)
                .map_or(bytes.len(), |n| from + n + close.len());
            blank_keeping_lines(&mut out, bytes, i, end);
            i = end;
        } else if rest.starts_with('"') || rest.starts_with("b\"") {
            let mut j = i + rest.find('"').expect("a string opens") + 1;
            while j < bytes.len() {
                match bytes[j] {
                    b'\\' => j += 2,
                    b'"' => {
                        j += 1;
                        break;
                    }
                    _ => j += 1,
                }
            }
            blank_keeping_lines(&mut out, bytes, i, j.min(bytes.len()));
            i = j.min(bytes.len());
        } else if let Some(end) = char_literal(rest) {
            blank_keeping_lines(&mut out, bytes, i, i + end);
            i += end;
        } else {
            out[i] = bytes[i];
            i += 1;
        }
    }
    String::from_utf8(out).expect("blanking keeps the source valid utf-8")
}

/// Newlines survive the blanking, so a byte offset still names its own line.
fn blank_keeping_lines(out: &mut [u8], bytes: &[u8], from: usize, to: usize) {
    for k in from..to.min(bytes.len()) {
        out[k] = if bytes[k] == b'\n' { b'\n' } else { b' ' };
    }
}

/// The number of `#` in a raw string's opener, where one starts here.
fn raw_string(rest: &str) -> Option<usize> {
    let body = rest.strip_prefix("br").or_else(|| rest.strip_prefix('r'))?;
    let hashes = body.chars().take_while(|c| *c == '#').count();
    body[hashes..].starts_with('"').then_some(hashes)
}

/// The length of a character literal starting here — never a lifetime, which
/// carries no closing quote.
fn char_literal(rest: &str) -> Option<usize> {
    let body = rest.strip_prefix('\'')?;
    if let Some(escaped) = body.strip_prefix('\\') {
        let end = escaped.find('\'')?;
        return (end < 6).then_some(end + 3);
    }
    let mut chars = body.chars();
    let first = chars.next()?;
    (chars.next() == Some('\'')).then_some(1 + first.len_utf8() + 1)
}

/// Every function in the file as (qualified name, body), for an inherent `impl`
/// method as `Type::name`. Top-level items and their methods are found by
/// column, which the formatter guarantees.
fn functions(text: &str) -> Vec<(String, String)> {
    let code = code_only(text);
    fn_spans(&code)
        .into_iter()
        .map(|(name, from, to, _)| (name, code[from..to].to_string()))
        .collect()
}

/// Every function's (qualified name, body start, body end, is a `#[test]` arm).
fn fn_spans(code: &str) -> Vec<(String, usize, usize, bool)> {
    let mut spans = Vec::new();
    let mut ty: Option<String> = None;
    let mut line_at = 0usize;
    for line in code.split_inclusive('\n') {
        let at = line_at;
        line_at += line.len();
        if let Some(head) = line.strip_prefix("impl ") {
            ty = Some(inherent_type(head));
            continue;
        }
        let (indent, close) = if line.starts_with("fn ") || line.starts_with("pub fn ") {
            (0usize, "\n}")
        } else if line.trim_start().starts_with("fn ") || line.trim_start().starts_with("pub fn ") {
            (line.len() - line.trim_start().len(), "\n    }")
        } else {
            continue;
        };
        let Some(name) = fn_name(line.trim_start()) else {
            continue;
        };
        let end = code[at..].find(close).map_or(code.len(), |n| at + n + 1);
        let qualified = match (indent, &ty) {
            (0, _) => name,
            (_, Some(ty)) => format!("{ty}::{name}"),
            (_, None) => name,
        };
        spans.push((qualified, at, end, is_arm(&code[..at])));
    }
    spans
}

/// The head of an `impl` block. For `impl Trait for Type` this is the TRAIT's
/// name, which no call site writes, so a trait method never matches a label.
fn inherent_type(head: &str) -> String {
    head.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches('{')
        .split('<')
        .next()
        .unwrap_or("")
        .to_string()
}

/// A `#[test]` on the item that starts here, at whatever depth it sits. The
/// attributes immediately above the `fn` are read and anything else ends the
/// search, so this never reaches past its own item.
fn is_arm(before: &str) -> bool {
    for line in before.lines().rev() {
        let line = line.trim();
        if line == "#[test]" {
            return true;
        }
        if !line.starts_with("#[") && !line.starts_with("//") {
            return false;
        }
    }
    false
}

fn fn_name(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("pub fn ")
        .or_else(|| line.strip_prefix("fn "))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The statement containing `at`, as offsets into `body`.
fn statement(body: &str, at: usize) -> (usize, usize) {
    statements(body)
        .find(|(from, to)| *from <= at && at < *to)
        .unwrap_or((0, body.len()))
}

/// The body's statements, delimited by the semicolons the blanking left.
fn statements(body: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut from = 0usize;
    body.match_indices(';')
        .map(move |(at, _)| {
            let span = (from, at + 1);
            from = at + 1;
            span
        })
        .filter(|(from, to)| from < to)
}

/// The string literals between two offsets, read off the ORIGINAL source: the
/// blanked copy holds no literal to read.
fn string_literals(text: &str, from: usize, to: usize) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let slice = &text[from..to.min(text.len())];
    let mut rest = slice;
    while let Some(open) = rest.find('"') {
        let body = &rest[open + 1..];
        let Some(close) = body.find('"') else { break };
        found.insert(body[..close].to_string());
        rest = &body[close + 1..];
    }
    found
}

/// The innermost function span holding `at`.
fn innermost(
    spans: &[(String, usize, usize, bool)],
    at: usize,
) -> Option<(&str, usize, usize, bool)> {
    spans
        .iter()
        .filter(|(_, from, to, _)| *from <= at && at < *to)
        .min_by_key(|(_, from, to, _)| to - from)
        .map(|(name, from, to, is_arm)| (name.as_str(), *from, *to, *is_arm))
}
