//! The production-write class: three checks over one command's text, one per
//! list the project declares.
//!
//! THE LISTS ARE THE WHOLE TARGET SET. A bucket host, a cloud project and an
//! application name are refused because the project wrote them down, never
//! because they are spelled a certain way — a naming rule is a derivation, and a
//! derivation belongs to the tree it was derived from rather than to a pack. A
//! list that is absent or empty leaves its check refusing nothing.
//!
//! READS ARE NEVER REFUSED. A verb that names its own direction — a listing, a
//! read of a policy, a `get` sub-verb — passes on that name. The verbs whose
//! direction depends on argument ORDER are the exception and they are stated
//! rather than guessed: for `cp`, `mv` and `rsync` a listed bucket ANYWHERE in
//! the arguments refuses, so a download FROM a listed bucket refuses too. That
//! is deliberate, and the escape carries the case it costs: guessing direction
//! from argument position fails toward letting a write through.
//!
//! TWO READINGS ARE THE CALLER'S. Nothing here touches the filesystem or runs a
//! process, so the application a `fly.toml` beside the command names and the
//! cloud project a machine is currently configured for arrive through
//! [`Policy`] or do not arrive at all. Where one is absent the check that
//! wanted it has no target and refuses nothing, which is the same shape an
//! absent list has.
//!
//! Residuals, stated so a refusal is legible and a silence is not mistaken for
//! cover: the reader's own (an alias, a command assembled in a variable, a
//! nested shell), a mutation reached through a build tool rather than named on
//! the command line, and an application named through a configuration file this
//! class was not handed.
//!
//! A TEXT THIS READER CANNOT LEX FALLS BACK rather than allowing, which is this
//! class alone among the three a pack wires: the chunk that LEADS with one of
//! this class's own tools is scanned for a LITERAL entry of the three lists, and
//! a hit refuses. The lists are the whole target set there too — the fallback
//! matches what the project wrote down and derives nothing from a spelling, so
//! it reaches no target a parsed judgment could not have named.

use super::lex::{basename, command_words, is_assignment, lex, statements, unquote};
use super::release_ref::matches_glob;
use super::{leading_escape, Denial, Policy, ESCAPE_PROD_WRITE};

pub const CHECKS: [&str; 6] = [
    "bucket",
    "project",
    "app",
    "make-goal",
    "module-function",
    "workflow-ref",
];

/// The tools this class reads. Both paths refuse on this list — the parsed walk
/// below and the raw-text fallback at the foot of the file — so a tool that
/// leaves here leaves both at once.
const GUARDED_TOOLS: [&str; 9] = [
    "gsutil", "gcloud", "firebase", "fly", "flyctl", "make", "gmake", "dagger", "gh",
];

/// The storage verbs that write. A verb outside this set is a read or is
/// something this class has never heard of, and both allow.
const BUCKET_VERBS: [&str; 15] = [
    "cp",
    "mv",
    "rm",
    "rsync",
    "rb",
    "setmeta",
    "acl",
    "iam",
    "defacl",
    "notification",
    "retention",
    "versioning",
    "web",
    "cors",
    "lifecycle",
];

/// The verbs above that take a sub-verb, and the sub-verbs that read. A policy
/// is READ through `get` and `list` before every legitimate change, so refusing
/// the measurement is how a guard gets routed around. The sub-verb is consulted
/// only for the verbs that have one: asking it of `cp` would read a local file
/// named `get` as a direction.
const BUCKET_SUBVERB_VERBS: [&str; 9] = [
    "acl",
    "iam",
    "defacl",
    "notification",
    "retention",
    "versioning",
    "web",
    "cors",
    "lifecycle",
];
const READING_SUBVERBS: [&str; 3] = ["get", "list", "describe"];

/// The verbs whose direction is not on the command line, so a listed bucket
/// anywhere in the arguments refuses.
const ORDER_DEPENDENT_VERBS: [&str; 3] = ["cp", "mv", "rsync"];

/// The storage tool's own flags that consume a following word.
const STORAGE_VALUE_FLAGS: [&str; 4] = ["-o", "-h", "-u", "-i"];

/// `gcloud storage`, the same shape one word in: the flat verbs, and the groups
/// whose write is named by a sub-verb.
const STORAGE_VERBS: [&str; 4] = ["cp", "mv", "rm", "rsync"];
const STORAGE_GROUPS: [(&str, &[&str]); 2] = [
    ("buckets", &["create", "delete", "update"]),
    ("objects", &["delete", "update"]),
];

/// The cloud groups whose mutations reach a project, and the exact deploy
/// commands, which are a group plus a verb rather than a group alone.
const PROJECT_GROUPS: [&str; 3] = ["sql", "compute", "iam"];
const DEPLOY_COMMANDS: [(&str, &str); 3] = [
    ("run", "deploy"),
    ("functions", "deploy"),
    ("app", "deploy"),
];

/// The grammar is `<tool> <group> [<subgroup>...] <verb>`, and a subgroup is a
/// noun while a verb is one of a known vocabulary — so the verb is the first
/// positional past the group that one of these two sets recognises. A positional
/// recognised by neither leaves the verb UNKNOWN, which refuses: an unknown verb
/// inside a group that reaches production is the direction this check fails in.
const READING_VERBS: [&str; 11] = [
    "list",
    "describe",
    "get",
    "get-value",
    "get-iam-policy",
    "get-config",
    "info",
    "version",
    "help",
    "print-access-token",
    "print-identity-token",
];
const WRITING_VERBS: [&str; 31] = [
    "create",
    "delete",
    "update",
    "patch",
    "set",
    "add",
    "remove",
    "deploy",
    "import",
    "restore",
    "reset",
    "resize",
    "start",
    "stop",
    "restart",
    "attach",
    "detach",
    "enable",
    "disable",
    "clone",
    "failover",
    "rollback",
    "add-iam-policy-binding",
    "remove-iam-policy-binding",
    "set-iam-policy",
    "promote",
    "migrate",
    "move",
    "rename",
    "undelete",
    "set-traffic",
];

const CLOUD_PROJECT_FLAGS: [&str; 2] = ["--project", "-p"];
const DEPLOY_TOOL_PROJECT_FLAGS: [&str; 2] = ["--project", "-P"];
const DEPLOY_TOOL_COMMANDS: [&str; 3] = ["deploy", "hosting:channel:deploy", "functions:delete"];

/// The application commands that write. `machines` is the whole group; the other
/// three are exact. A read inside the group still passes on its own name, for
/// the reason every read does.
const APP_COMMANDS: [(&str, &str); 3] = [("deploy", ""), ("scale", ""), ("secrets", "set")];
const APP_GROUPS: [&str; 1] = ["machines"];
const APP_READING_SUBVERBS: [&str; 3] = ["list", "status", "show"];
const APP_FLAGS: [&str; 2] = ["-a", "--app"];

pub fn judge(command: &str, policy: &Policy) -> Option<Denial> {
    if leading_escape(command, ESCAPE_PROD_WRITE) {
        return None;
    }
    let tokens = match lex(command) {
        Ok(tokens) => tokens,
        Err(_) => return raw_listed_target(command, policy),
    };
    for (words, _) in statements(&tokens) {
        let current = command_words(&words);
        let Some(head) = current.first() else {
            continue;
        };
        let tool = basename(&head.text);
        if !GUARDED_TOOLS.contains(&tool) {
            continue;
        }
        let rest: Vec<String> = current[1..].iter().map(|w| w.value()).collect();
        let found = match tool {
            "gsutil" => storage_tool(&rest, policy),
            "gcloud" => cloud_tool(&rest, policy),
            "firebase" => deploy_tool(&rest, policy),
            "fly" | "flyctl" => app_tool(&rest, policy),
            "make" | "gmake" => make_tool(&rest, policy),
            "dagger" => dagger_tool(&rest, policy),
            "gh" => gh_tool(&rest, policy),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

// ---- 1. the bucket a storage write reaches ----------------------------------

fn storage_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let mut index = 0;
    while index < rest.len() {
        if STORAGE_VALUE_FLAGS.contains(&rest[index].as_str()) {
            index += 2;
        } else if rest[index].starts_with('-') {
            index += 1;
        } else {
            break;
        }
    }
    let verb = rest.get(index)?;
    if !BUCKET_VERBS.contains(&verb.as_str()) {
        return None;
    }
    let arguments = &rest[index + 1..];
    let mut named = verb.clone();
    if BUCKET_SUBVERB_VERBS.contains(&verb.as_str()) {
        let subverb = arguments.first()?;
        if READING_SUBVERBS.contains(&subverb.as_str()) {
            return None;
        }
        named = format!("{verb} {subverb}");
    }
    let target = listed_bucket(arguments, policy)?;
    Some(bucket_denial(&named, &target, verb))
}

fn storage_group(positionals: &[String], arguments: &[String], policy: &Policy) -> Option<Denial> {
    let verb = positionals.get(1)?;
    if verb == "service-agent" {
        // The one listed storage verb that names a project rather than a
        // bucket, so the project rule judges it.
        return project_denial(
            "storage service-agent",
            arguments,
            &CLOUD_PROJECT_FLAGS,
            policy,
        );
    }
    if STORAGE_VERBS.contains(&verb.as_str()) {
        let target = listed_bucket(arguments, policy)?;
        return Some(bucket_denial(verb, &target, verb));
    }
    let (_, subverbs) = STORAGE_GROUPS.iter().find(|(g, _)| g == verb)?;
    let subverb = positionals.get(2)?;
    if !subverbs.contains(&subverb.as_str()) {
        return None;
    }
    let target = listed_bucket(arguments, policy)?;
    Some(bucket_denial(&format!("{verb} {subverb}"), &target, verb))
}

/// The first argument naming a listed bucket, or `None`.
///
/// For a verb that names its direction the destination is the LAST argument, and
/// this reads every one of them; the over-refusal that buys is the
/// order-dependent rule stated in the header, and it is why the caller passes
/// the verb rather than this function reading it.
fn listed_bucket(arguments: &[String], policy: &Policy) -> Option<String> {
    for argument in arguments {
        let Some(path) = argument.strip_prefix("gs://") else {
            continue;
        };
        let host = path.split('/').next().unwrap_or(path);
        if policy.prod_buckets.iter().any(|b| b == host) {
            return Some(argument.clone());
        }
    }
    None
}

fn bucket_denial(named: &str, target: &str, verb: &str) -> Denial {
    let both_ways = ORDER_DEPENDENT_VERBS.contains(&verb);
    Denial {
        class: "production-write",
        check: "bucket",
        label: if both_ways {
            "A LISTED BUCKET, AND THIS VERB READS ITS DIRECTION FROM ARGUMENT ORDER"
        } else {
            "A WRITE TO A LISTED BUCKET"
        },
        fragment: format!("{named} {target}"),
        rewrite: "read it instead — a listing, a `cat`, a `get` sub-verb — or have a person run \
                  the write from their own shell"
            .to_string(),
        escape: Some(ESCAPE_PROD_WRITE),
        why: "a listed bucket holds live data, and nothing between this command and that bucket \
              asks a second time",
    }
}

// ---- 2. the project a cloud write reaches -----------------------------------

fn cloud_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let positionals = positionals(rest, &CLOUD_PROJECT_FLAGS);
    let group = positionals.first()?;

    if group == "storage" {
        return storage_group(&positionals, rest, policy);
    }

    // The one configuration write that changes which project every later
    // command reaches without naming it.
    if group == "config" && positionals.get(1).map(String::as_str) == Some("set") {
        let key = positionals.get(2)?;
        let value = positionals.get(3)?;
        if key == "project" && policy.prod_projects.iter().any(|p| p == value) {
            return Some(project_refusal("config set project", value));
        }
        return None;
    }

    if let Some(second) = positionals.get(1) {
        if DEPLOY_COMMANDS
            .iter()
            .any(|(g, v)| g == group && v == second)
        {
            return project_denial(
                &format!("{group} {second}"),
                rest,
                &CLOUD_PROJECT_FLAGS,
                policy,
            );
        }
    }

    if PROJECT_GROUPS.contains(&group.as_str()) {
        let verb = verb_of(&positionals);
        if verb.as_deref().is_some_and(|v| READING_VERBS.contains(&v)) {
            return None;
        }
        let named = format!("{group} {}", verb.as_deref().unwrap_or("?"));
        return project_denial(&named, rest, &CLOUD_PROJECT_FLAGS, policy);
    }

    None
}

fn deploy_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let positionals = positionals(rest, &DEPLOY_TOOL_PROJECT_FLAGS);
    let command = positionals.first()?;
    if !DEPLOY_TOOL_COMMANDS.contains(&command.as_str()) {
        return None;
    }
    project_denial(command, rest, &DEPLOY_TOOL_PROJECT_FLAGS, policy)
}

/// The verb of a `<tool> <group> [<subgroup>...] <verb>` invocation, or `None`
/// where neither vocabulary recognised any positional past the group.
fn verb_of(positionals: &[String]) -> Option<String> {
    positionals[1..]
        .iter()
        .find(|w| READING_VERBS.contains(&w.as_str()) || WRITING_VERBS.contains(&w.as_str()))
        .cloned()
}

/// The project this command names, or the one the machine is configured for
/// where it names none.
///
/// AN UNREADABLE ANSWER LEAVES THE CHECK WITHOUT A TARGET and refuses nothing,
/// which is the same shape an absent list has: this class reads no process, so
/// it cannot tell an unreadable configuration from an absent one and must not
/// refuse on the difference.
fn project_denial(
    named: &str,
    words: &[String],
    flags: &[&str],
    policy: &Policy,
) -> Option<Denial> {
    match flag_value(words, flags) {
        Some(value) => policy
            .prod_projects
            .contains(&value)
            .then(|| project_refusal(named, &format!("--project {value}"))),
        None => {
            let active = policy.active_project.as_deref()?;
            policy.prod_projects.iter().any(|p| p == active).then(|| {
                project_refusal(
                    named,
                    &format!("no --project, and the configured project is {active}"),
                )
            })
        }
    }
}

fn project_refusal(named: &str, target: &str) -> Denial {
    Denial {
        class: "production-write",
        check: "project",
        label: "A WRITE TO A LISTED PROJECT",
        fragment: format!("{named} — {target}"),
        rewrite: "name a project that is not listed with `--project <one>`, or have a person run \
                  it from their own shell"
            .to_string(),
        escape: Some(ESCAPE_PROD_WRITE),
        why: "a listed project is live infrastructure, and a mutating command reaches it under \
              whatever credentials this session already holds",
    }
}

// ---- 3. the application a deploy reaches ------------------------------------

fn app_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let positionals = positionals(rest, &APP_FLAGS);
    let first = positionals.first()?;
    let second = positionals.get(1).map(String::as_str);

    let named = if APP_COMMANDS.iter().any(|(a, b)| a == first && b.is_empty()) {
        first.clone()
    } else if let Some(second) =
        second.filter(|s| APP_COMMANDS.iter().any(|(a, b)| a == first && b == s))
    {
        format!("{first} {second}")
    } else if APP_GROUPS.contains(&first.as_str()) {
        if second.is_some_and(|s| APP_READING_SUBVERBS.contains(&s)) {
            return None;
        }
        match second {
            Some(second) => format!("{first} {second}"),
            None => first.clone(),
        }
    } else {
        return None;
    };

    match flag_value(rest, &APP_FLAGS) {
        Some(name) => policy
            .prod_apps
            .contains(&name)
            .then(|| app_refusal(&named, &format!("--app {name}"))),
        None => {
            let here = policy.cwd_app.as_deref()?;
            policy.prod_apps.iter().any(|a| a == here).then(|| {
                app_refusal(
                    &named,
                    &format!("no --app, and the directory's own file names {here}"),
                )
            })
        }
    }
}

fn app_refusal(named: &str, target: &str) -> Denial {
    Denial {
        class: "production-write",
        check: "app",
        label: "A WRITE TO A LISTED APPLICATION",
        fragment: format!("{named} — {target}"),
        rewrite: "name an application that is not listed with `--app <one>`, or have a person run \
                  it from their own shell"
            .to_string(),
        escape: Some(ESCAPE_PROD_WRITE),
        why: "a listed application serves live traffic, and a deploy or a scale reaches it with \
              nothing between this command and the network",
    }
}

// ---- the argument walk both project checks share ----------------------------

/// The words that are neither a flag nor a flag's separate value.
fn positionals(words: &[String], value_flags: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < words.len() {
        if value_flags.contains(&words[index].as_str()) {
            index += 2;
            continue;
        }
        if words[index].starts_with('-') {
            index += 1;
            continue;
        }
        found.push(words[index].clone());
        index += 1;
    }
    found
}

/// The value of the first of `flags` present, in either of its two forms. A flag
/// written last with no value yields `None` rather than swallowing the end of
/// the list.
fn flag_value(words: &[String], flags: &[&str]) -> Option<String> {
    for (index, word) in words.iter().enumerate() {
        for flag in flags {
            if word == flag {
                return words
                    .get(index + 1)
                    .filter(|next| !next.starts_with('-'))
                    .cloned();
            }
            if let Some(value) = word.strip_prefix(&format!("{flag}=")) {
                return Some(value.to_string());
            }
        }
    }
    None
}

// ---- 4. the build-tool goal that deploys ------------------------------------
//
// THE LAST `dry=` ASSIGNMENT DECIDES, because that is what make itself does
// with a repeated variable — reading the first would call a real deploy dry.

/// The assignment that makes the goal print its call and run nothing. The
/// spellings between the name and the `=` are make's own conditional forms.
const MAKE_DRY: &str = "dry=1";

fn make_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let goal = rest
        .iter()
        .find(|word| policy.prod_make_goals.iter().any(|g| g == *word))?;
    let dry = rest
        .iter()
        .filter(|word| is_dry_assignment(word))
        .next_back();
    if dry.map(String::as_str) == Some(MAKE_DRY) {
        return None;
    }
    Some(Denial {
        class: "production-write",
        check: CHECKS[3],
        label: "A BUILD-TOOL GOAL THE PROJECT LISTS AS A DEPLOY",
        fragment: goal.clone(),
        rewrite: format!(
            "run it dry — `{MAKE_DRY}` among the goal's words prints the call and runs nothing \
             — or have a person run the deploy from their own shell"
        ),
        escape: Some(ESCAPE_PROD_WRITE),
        why:
            "what a goal runs is inside the makefile, where the command text cannot show it, so a \
              listed goal is refused wherever it is invoked from",
    })
}

fn is_dry_assignment(word: &str) -> bool {
    let Some(rest) = word.strip_prefix("dry") else {
        return false;
    };
    rest.trim_start_matches([':', '?', '+', '!'])
        .starts_with('=')
}

// ---- 5. the module function declared a production write ---------------------
//
// The tool's own value-taking flags are CARRIED rather than guessed: a flag
// whose value this reader stepped past as a positional would read that value as
// the function name, and the function word would then never be reached.

const DAGGER_VALUE_FLAGS: [&str; 9] = [
    "--mod",
    "--command",
    "--progress",
    "--model",
    "--allow-llm",
    "--interactive-command",
    "--lock",
    "--x-release",
    "--cleanup-timeout",
];
/// The shorthands that take a value, as `(letter, the long flag it is)`.
const DAGGER_VALUE_SHORTHANDS: [char; 2] = ['m', 'c'];

fn dagger_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let (start, scripts) = dagger_arguments(rest);
    // A script handed to the tool is words it will run, so the function named
    // inside one is named by this invocation. The lexer has already taken the
    // quoting off each word; what is left is to split the script the way the
    // tool's own shell does.
    let in_script = scripts
        .iter()
        .flat_map(|script| script.split([' ', '\t', '\n', '|', ';', '&', '(', ')']))
        .find(|word| policy.prod_dagger_functions.iter().any(|f| f == word))
        .map(str::to_string);
    let named = match &in_script {
        Some(word) => word,
        None => rest[start..]
            .iter()
            .find(|word| policy.prod_dagger_functions.iter().any(|f| f == *word))?,
    };
    Some(Denial {
        class: "production-write",
        check: CHECKS[4],
        label: "A MODULE FUNCTION THE PROJECT LISTS AS A PRODUCTION WRITE",
        fragment: named.clone(),
        rewrite: "call a function that is not listed, or have a person run this one from their \
                  own shell"
            .to_string(),
        escape: Some(ESCAPE_PROD_WRITE),
        why: "the project declared this function as one that writes production, and what it \
              reaches is inside the module rather than on the command line",
    })
}

/// `(the index of the first word past the tool's flags, the scripts its
/// command flag was handed)`, read as its own flag library reads them:
/// `--flag value`, `--flag=value`, and a shorthand group whose value letter
/// takes the rest of the group or else the next word.
fn dagger_arguments(words: &[String]) -> (usize, Vec<String>) {
    let mut scripts = Vec::new();
    let mut index = 0;
    while index < words.len() && words[index].starts_with('-') && words[index] != "-" {
        let word = &words[index];
        index += 1;
        if let Some(name) = word.strip_prefix("--") {
            let (name, value) = match name.split_once('=') {
                Some((head, value)) => (head, Some(value.to_string())),
                None => (name, None),
            };
            let long = format!("--{name}");
            let value = match (value, DAGGER_VALUE_FLAGS.contains(&long.as_str())) {
                (Some(value), _) => Some(value),
                (None, true) => {
                    let value = words.get(index).cloned();
                    index += 1;
                    value
                }
                (None, false) => None,
            };
            if long == "--command" {
                scripts.extend(value);
            }
            continue;
        }
        for (position, letter) in word.char_indices().skip(1) {
            if DAGGER_VALUE_SHORTHANDS.contains(&letter) {
                // The value is the rest of the group, or the next word when the
                // group ends at the value letter.
                let mut value = word[position + letter.len_utf8()..].to_string();
                if value.is_empty() {
                    value = words.get(index).cloned().unwrap_or_default();
                    index += 1;
                } else if let Some(stripped) = value.strip_prefix('=') {
                    value = stripped.to_string();
                }
                if letter == 'c' {
                    scripts.push(value);
                }
                break;
            }
        }
    }
    (index, scripts)
}

// ---- 6. the workflow-and-ref pair a dispatch deploys through ----------------
//
// An entry is `<workflow>:<ref glob>` and BOTH halves must match: the same
// workflow on another ref deploys nothing, and another workflow on a release
// ref deploys nothing either.

const GH_REF_FLAGS: [&str; 2] = ["--ref", "-r"];
/// The tool's value-taking flags on this subcommand, carried for the reason the
/// tool above carries its own: a value read as a positional is read as the
/// workflow name, and a ref written before it is then a silent allow.
const GH_VALUE_FLAGS: [&str; 14] = [
    "--ref",
    "-r",
    "--repo",
    "-R",
    "--field",
    "-f",
    "--raw-field",
    "-F",
    "--json",
    "--jq",
    "--template",
    "--job",
    "-j",
    "--input",
];

fn gh_tool(rest: &[String], policy: &Policy) -> Option<Denial> {
    let positionals = positionals(rest, &GH_VALUE_FLAGS);
    if positionals.first().map(String::as_str) != Some("workflow")
        || positionals.get(1).map(String::as_str) != Some("run")
    {
        return None;
    }
    // A dispatch naming no workflow, or naming one with no ref, reaches no
    // pair — and that is the direction the reference allows in too.
    let workflow = basename(&positionals.get(2)?.replace('\\', "/")).to_string();
    let reference = flag_value(rest, &GH_REF_FLAGS)?;
    let entry = policy.prod_workflow_refs.iter().find(|entry| {
        entry
            .split_once(':')
            .is_some_and(|(name, glob)| name == workflow && matches_glob(glob, &reference))
    })?;
    Some(Denial {
        class: "production-write",
        check: CHECKS[5],
        label: "A WORKFLOW DISPATCH ON A REF THE PROJECT LISTS AS PRODUCTION",
        fragment: format!("workflow run {workflow} — {reference}, listed as {entry}"),
        rewrite: "dispatch it on a ref that is not listed, or have a person start the production \
                  run from their own shell"
            .to_string(),
        escape: Some(ESCAPE_PROD_WRITE),
        why: "a dispatch on a listed ref deploys production through the forge, where nothing \
              between this command and the deploy asks a second time",
    })
}

// ---- the fallback for a text the reader could not read ----------------------

/// The conservative text match, reached only when the lexer answers `Err`.
///
/// It is INVOCATION-SHAPED, like the record class's: a chunk is judged only when
/// its own command word is one of this class's tools, so unreadable prose that
/// merely names a listed target — a note quoting a refused command, this file's
/// own header — is an argument to something that writes nothing. There is no
/// parsed verb here, so a READ of a listed target refuses too; that is the
/// over-refusal this direction buys, and the escape is what bounds it.
fn raw_listed_target(command: &str, policy: &Policy) -> Option<Denial> {
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
            let tool = basename(unquote(head));
            if !GUARDED_TOOLS.contains(&tool) {
                continue;
            }
            // The quoting is what could not be read, so each argument is
            // compared with its quotes off: the target of an unterminated word
            // arrives carrying the quote that never closed.
            for argument in &words[index + 1..] {
                let argument = unquote(argument);
                for (check, list) in [
                    (CHECKS[0], &policy.prod_buckets),
                    (CHECKS[1], &policy.prod_projects),
                    (CHECKS[2], &policy.prod_apps),
                ] {
                    let hit = list
                        .iter()
                        .find(|entry| !entry.is_empty() && argument.contains(entry.as_str()));
                    if let Some(entry) = hit {
                        return Some(raw_refusal(check, tool, entry));
                    }
                }
            }
        }
    }
    None
}

fn raw_refusal(check: &'static str, tool: &str, entry: &str) -> Denial {
    Denial {
        class: "production-write",
        check,
        label: "A LISTED TARGET (this command could not be read, so the conservative text match \
                applied)",
        fragment: format!("{tool} — {entry}"),
        rewrite: "close the quoting so the command can be read, and it is judged on what it \
                  actually reaches — or have a person run it from their own shell"
            .to_string(),
        escape: Some(ESCAPE_PROD_WRITE),
        why: "this text names a target the project listed as live and could not be read well \
              enough to say what it does to it, and a wrong quote is not a reason to let a \
              production write through",
    }
}
