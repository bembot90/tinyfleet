//! The release-ref class: one check over one command's text.
//!
//! A release ref deploys, so the act it refuses is a push whose parsed
//! destination matches the glob the project declares. The glob is the whole of
//! the target: this class reads no branch naming convention and evidences
//! nothing from the tree, so a project that declares no glob is a project where
//! this class refuses nothing.
//!
//! IT HAS NO ESCAPE, and that is the row's own wording — "none at this layer; a
//! git hook beneath it holds the override". A leading assignment would put a
//! release one prefix away from a seat, which is the thing the rule is for; a
//! person cutting a release does it from their own shell, where no pre-tool
//! hook runs at all.
//!
//! It FAILS OPEN, like the shell-trap class and for the same reason: the class
//! refuses on a destination it can name, and a text the reader cannot lex names
//! none. The residuals it inherits from the reader — an alias, a command
//! assembled in a variable, a nested shell — are the reader's; the residuals of
//! its own reading are two, and both name no ref at all: `--all` and `--mirror`
//! push every matching ref without spelling one, so this check has nothing to
//! match and allows.

use super::lex::{basename, command_words, lex, statements, Token};
use super::{Denial, Policy};

pub const CHECKS: [&str; 1] = ["push-target"];

/// git's own global flags that consume a following word, so the walk to the
/// subcommand steps over the value rather than reading it as `push`.
const GIT_VALUE_FLAGS: [&str; 6] = [
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
];

/// The flags whose VALUE is a ref rather than an option: a deletion names its
/// ref through one, and a lease names the ref it is leasing.
const REF_VALUE_FLAGS: [&str; 4] = [
    "-d",
    "--delete",
    "--force-with-lease",
    "--force-if-includes",
];

pub fn judge(command: &str, policy: &Policy) -> Option<Denial> {
    let glob = policy.release_ref_glob.as_deref()?;
    if glob.is_empty() {
        return None;
    }
    let tokens = lex(command).ok()?;
    for (words, _) in statements(&tokens) {
        let current = command_words(&words);
        let Some(arguments) = push_arguments(&current) else {
            continue;
        };
        for (typed, destination) in destinations(&arguments) {
            if matches_glob(glob, &destination) {
                return Some(denial(&typed, &destination, glob));
            }
        }
    }
    None
}

/// The argument words of a `git push` statement, or `None` where the statement
/// invokes something else. An absolute path answers like the bare name.
fn push_arguments<'t>(current: &[&'t Token]) -> Option<Vec<&'t Token>> {
    let head = current.first()?;
    if basename(&head.text) != "git" {
        return None;
    }
    let mut index = 1;
    while index < current.len() {
        let word = &current[index].text;
        if GIT_VALUE_FLAGS.contains(&word.as_str()) {
            index += 2;
        } else if word.starts_with('-') {
            index += 1;
        } else {
            break;
        }
    }
    if current.get(index)?.text != "push" {
        return None;
    }
    Some(current[index + 1..].to_vec())
}

/// Every ref this push writes, as (the word it was typed as, the destination it
/// resolves to). The FIRST positional is the remote and names no ref; every
/// positional after it is a refspec, and a ref-valued flag carries one too.
fn destinations(arguments: &[&Token]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut seen_remote = false;
    let mut index = 0;
    while index < arguments.len() {
        let word = arguments[index].value();
        if let Some((name, value)) = flag_pair(&word) {
            if REF_VALUE_FLAGS.contains(&name) {
                match value {
                    Some(value) => {
                        push_destination(&mut found, value);
                        index += 1;
                        continue;
                    }
                    // `--delete <ref>...` takes its refs as the positionals
                    // after it, which the remote rule below already reads: the
                    // remote still comes first on a deletion.
                    None => {
                        index += 1;
                        continue;
                    }
                }
            }
            index += 1;
            continue;
        }
        if !seen_remote {
            seen_remote = true;
            index += 1;
            continue;
        }
        push_destination(&mut found, &word);
        index += 1;
    }
    found
}

/// A word's flag name and its attached value, or `None` where the word is a
/// positional. `--flag=value` yields the value; a bare `--flag` yields `None`.
fn flag_pair(word: &str) -> Option<(&str, Option<&str>)> {
    if !word.starts_with('-') {
        return None;
    }
    match word.split_once('=') {
        Some((name, value)) => Some((name, Some(value))),
        None => Some((word, None)),
    }
}

/// The destination of one refspec, appended where it has one.
///
/// A leading `+` forces and names no ref of its own. A colon splits source from
/// destination, and a deletion is the form whose source is empty; a lease's
/// value is `<ref>` or `<ref>:<expected>`, which this same split reads. A bare
/// name pushes to the ref of the same name.
fn push_destination(found: &mut Vec<(String, String)>, typed: &str) {
    let body = typed.strip_prefix('+').unwrap_or(typed);
    let name = match body.split_once(':') {
        Some((_source, destination)) => destination,
        None => body,
    };
    if name.is_empty() {
        return;
    }
    found.push((typed.to_string(), qualify(name)));
}

/// A destination that does not name its ref namespace is a branch, which is
/// what git does with it.
fn qualify(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_string()
    } else {
        format!("refs/heads/{name}")
    }
}

/// Whether `text` matches `pattern`, where `*` stands for any run of characters
/// — A SLASH INCLUDED — and `?` for exactly one. Every other character is
/// literal.
///
/// That is the shell's `case` semantics, which is what the git hook beneath
/// this layer matches its own refs with, so one glob string written in the
/// project's file refuses the same refs at both layers. The one `case` form not
/// read here is a bracket class, which no ref glob has yet needed; a pattern
/// carrying one matches its brackets literally and so refuses less, which is
/// the direction this class fails in.
pub fn matches_glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut p = 0;
    let mut t = 0;
    let mut star: Option<usize> = None;
    let mut resume = 0;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            resume = t;
        } else if let Some(at) = star {
            p = at + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn denial(typed: &str, destination: &str, glob: &str) -> Denial {
    Denial {
        class: "release-ref",
        check: "push-target",
        label: "A RELEASE REF",
        fragment: if typed == destination {
            destination.to_string()
        } else {
            format!("{typed} -> {destination}")
        },
        rewrite: format!(
            "push the same commit to a work branch — `git push <remote> \
             <local>:refs/heads/<your-branch>` — and leave `{glob}` to the person cutting the \
             release"
        ),
        escape: None,
        why: "a release ref deploys, so this push is a deployment wearing the clothes of a \
              branch update — and it is one act, with nothing between it and production",
    }
}
