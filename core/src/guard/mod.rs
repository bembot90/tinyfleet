//! The guard classes, as a pure function from a command's text and the policy
//! that governs it to a verdict.
//!
//! FOUR CLASSES, COMPILED INTO ONE BINARY, AND THEY ARE NOT ALL CORE'S. Two of
//! them — shell-trap and record — pass core's bar and every fleet gets them:
//! every seat has a shell and every fleet has a record. The other two —
//! release-ref and production-write — fail it, because a fifty-commit side
//! project has no release refs and no production cloud, so they are A PACK'S
//! guards on the same rules (packs PRD § The guards): the class lives here,
//! which classes a provider runs is the pack's overlay to wire, and the
//! `[guards]` table switches any of them off the same way.
//!
//! Three rules hold for every class:
//!
//!   * A GUARD NEVER EMITS AN ALLOWING DECISION. An explicit one from a
//!     pre-tool hook short-circuits the agent's whole permission system, so
//!     letting a command through means printing nothing at all — which is why
//!     the silent verdict below carries no wording of its own.
//!   * A REFUSAL IS DATA, delivered on stdout with exit 0. A crash that
//!     signalled by exit status would be read as non-blocking, so a guard that
//!     reported by exit code would fail open exactly when it broke.
//!   * A REFUSAL PRINTS THE REWRITE. A refusal a seat cannot act on is one it
//!     routes around.
//!
//! NO PROVIDER REACHES THIS MODULE. The payload one agent runtime hands a
//! pre-tool hook, and the document it reads a refusal out of, are that
//! runtime's contract and live in the caller's adapter; a second runtime adds
//! a reader of its own and reuses these classes unchanged. Nothing here states
//! a decision value, touches the filesystem or reads the process table: the
//! caller resolves the policy and hands it in, which is what keeps the
//! judgment testable over text alone.

pub mod lex;
pub mod production_write;
pub mod record;
pub mod release_ref;
pub mod shell_trap;

use crate::policy;

/// The escape for each guarded act THAT HAS ONE, as a leading assignment on the
/// command itself. They are NOT interchangeable: each licenses the act it names and
/// nothing else, so a session that meant to replace one field cannot also be
/// issuing SQL writes on the strength of the same prefix. Read off the command
/// rather than the environment, because an ambient variable would license every
/// later call in the session and the point is per-call intent.
pub const ESCAPE_TRAP: &str = "FLEET_TRAP_OK";
pub const ESCAPE_NOTES_REPLACE: &str = "FLEET_NOTES_REPLACE_OK";
pub const ESCAPE_SQL_WRITE: &str = "FLEET_SQL_WRITE_OK";
pub const ESCAPE_BARE_ID: &str = "FLEET_BARE_ID_OK";
pub const ESCAPE_PROD_WRITE: &str = "FLEET_PROD_WRITE_OK";

/// The release-ref class has no constant above and cannot have one: the packs
/// PRD's row reads "none at this layer; a hook beneath it holds the override".
/// A leading assignment would put a release one prefix away from a seat, and the
/// person who cuts one does it from their own shell, where no pre-tool hook runs
/// at all. This is the sentence a refusal with no escape prints in its place.
pub const NO_ESCAPE: &str =
    "no escape at this layer — a release is cut by a person from their own shell";

/// The key the bare-id check needs before it refuses anything.
pub const ITEM_PREFIX_KEY: &str = "[project] item_prefix";

/// The keys the two pack classes need, each in the project's own
/// `[guards.targets]` table, beside the fleet's `[guards]` switches. A check
/// whose key is absent or empty refuses nothing and says so under the check
/// flag (packs PRD R16).
pub const RELEASE_REF_GLOB_KEY: &str = "[guards.targets] release_ref_glob";
pub const PROD_BUCKETS_KEY: &str = "[guards.targets] prod_buckets";
pub const PROD_PROJECTS_KEY: &str = "[guards.targets] prod_projects";
pub const PROD_APPS_KEY: &str = "[guards.targets] prod_apps";
pub const PROD_MAKE_GOALS_KEY: &str = "[guards.targets] prod_make_goals";
pub const PROD_DAGGER_FUNCTIONS_KEY: &str = "[guards.targets] prod_dagger_functions";
pub const PROD_WORKFLOW_REFS_KEY: &str = "[guards.targets] prod_workflow_refs";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    ShellTrap,
    Record,
    ReleaseRef,
    ProductionWrite,
}

/// Every class, for a reader that reports on all of them. A further class added
/// to the enum and not to this list is a class the brief would not name.
pub const CLASSES: [Class; 4] = [
    Class::ShellTrap,
    Class::Record,
    Class::ReleaseRef,
    Class::ProductionWrite,
];

impl Class {
    pub fn parse(name: &str) -> Option<Class> {
        match name {
            "shell-trap" => Some(Class::ShellTrap),
            "record" => Some(Class::Record),
            "release-ref" => Some(Class::ReleaseRef),
            "production-write" => Some(Class::ProductionWrite),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::ShellTrap => "shell-trap",
            Class::Record => "record",
            Class::ReleaseRef => "release-ref",
            Class::ProductionWrite => "production-write",
        }
    }

    /// The checks the class runs, in the order it runs them.
    pub fn checks(self) -> &'static [&'static str] {
        match self {
            Class::ShellTrap => &shell_trap::CHECKS,
            Class::Record => &record::CHECKS,
            Class::ReleaseRef => &release_ref::CHECKS,
            Class::ProductionWrite => &production_write::CHECKS,
        }
    }

    /// The configuration key a check needs before it can refuse anything, or
    /// `None` where the check has no target and is always configured.
    pub fn target_of(self, check: &str) -> Option<&'static str> {
        match (self, check) {
            (Class::Record, "bare-id") => Some(ITEM_PREFIX_KEY),
            (Class::ReleaseRef, "push-target") => Some(RELEASE_REF_GLOB_KEY),
            (Class::ProductionWrite, "bucket") => Some(PROD_BUCKETS_KEY),
            (Class::ProductionWrite, "project") => Some(PROD_PROJECTS_KEY),
            (Class::ProductionWrite, "app") => Some(PROD_APPS_KEY),
            (Class::ProductionWrite, "make-goal") => Some(PROD_MAKE_GOALS_KEY),
            (Class::ProductionWrite, "module-function") => Some(PROD_DAGGER_FUNCTIONS_KEY),
            (Class::ProductionWrite, "workflow-ref") => Some(PROD_WORKFLOW_REFS_KEY),
            _ => None,
        }
    }
}

/// Everything the judgment reads that is not the command text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// `[guards].<class>.enabled`. Opt-out, never opt-in: an absent table, an
    /// absent key and an unreadable file all leave the class on.
    pub enabled: bool,
    /// The target the bare-id check needs. A check whose target is not
    /// configured refuses nothing.
    pub item_prefix: Option<String>,
    /// The glob the push-target check matches a push's destination against.
    pub release_ref_glob: Option<String>,
    /// The three lists the production-write class reads, one per check. THE LIST
    /// IS THE WHOLE TARGET SET: this module evidences nothing from a tree and
    /// derives nothing from a name, so an empty one refuses nothing.
    pub prod_buckets: Vec<String>,
    pub prod_projects: Vec<String>,
    pub prod_apps: Vec<String>,
    /// The three lists that name a PRODUCT's own production surfaces rather
    /// than a cloud target: the build-tool goals that deploy, the module
    /// functions declared as production writes, and the workflow-and-ref pairs
    /// written `<workflow>:<ref glob>`. Same rule as the three above — the list
    /// is the whole target set, and an empty one refuses nothing.
    pub prod_make_goals: Vec<String>,
    pub prod_dagger_functions: Vec<String>,
    pub prod_workflow_refs: Vec<String>,
    /// The two readings this module cannot make for itself, because it touches
    /// no filesystem and runs no process: the application the configuration file
    /// beside the command names, and the project the machine is currently
    /// configured for. Either absent leaves the check that wanted it without
    /// that target, which refuses nothing — an unreadable answer and an absent
    /// one are the same answer here, and refusing on the difference would make
    /// the judgment depend on something the text cannot show.
    pub cwd_app: Option<String>,
    pub active_project: Option<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            enabled: true,
            item_prefix: None,
            release_ref_glob: None,
            prod_buckets: Vec::new(),
            prod_projects: Vec::new(),
            prod_apps: Vec::new(),
            prod_make_goals: Vec::new(),
            prod_dagger_functions: Vec::new(),
            prod_workflow_refs: Vec::new(),
            cwd_app: None,
            active_project: None,
        }
    }
}

/// One refusal, with every part a reader acts on kept apart from the sentence
/// they are assembled into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub class: &'static str,
    pub check: &'static str,
    pub label: &'static str,
    /// The text of the command this refusal is about.
    pub fragment: String,
    /// What to write instead.
    pub rewrite: String,
    /// The variable that licenses this one act, or `None` where the class has
    /// no escape at this layer and [`NO_ESCAPE`] is the sentence printed instead.
    pub escape: Option<&'static str>,
    /// Why the shape is a trap rather than a style.
    pub why: &'static str,
}

impl Denial {
    pub fn reason(&self) -> String {
        let escape = match self.escape {
            Some(name) => format!("{name}=1 <the same command> runs it anyway"),
            None => NO_ESCAPE.to_string(),
        };
        format!(
            "fleet guard {}: {} — {} → {}; {escape}. The reason: {}.",
            self.class, self.label, self.fragment, self.rewrite, self.why
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing is refused, and the caller prints nothing at all.
    Silent,
    Refused(Denial),
}

/// The whole judgment, over the command text and the policy alone.
pub fn judge(class: Class, command: &str, policy: &Policy) -> Verdict {
    if !policy.enabled {
        return Verdict::Silent;
    }
    if command.trim().is_empty() {
        return Verdict::Silent;
    }
    let found = match class {
        Class::ShellTrap => {
            if leading_escape(command, ESCAPE_TRAP) {
                None
            } else {
                shell_trap::judge(command)
            }
        }
        Class::Record => record::judge(command, policy),
        Class::ReleaseRef => release_ref::judge(command, policy),
        // Its own escape is read inside the class, beside the checks it
        // licenses; the release-ref arm above has none to read.
        Class::ProductionWrite => production_write::judge(command, policy),
    };
    match found {
        Some(denial) => Verdict::Refused(denial),
        None => Verdict::Silent,
    }
}

/// Whether one of the command's LEADING assignments sets `name` to a value that
/// is on. `=0`, `=false`, `=no` and an empty value are the spellings that are
/// off, so the variable can be written and left switched off in place.
pub fn leading_escape(command: &str, name: &str) -> bool {
    for word in command.split_whitespace() {
        let Some((key, value)) = word.split_once('=') else {
            return false;
        };
        if !lex::is_assignment(word) {
            return false;
        }
        if key == name && !matches!(value, "" | "0" | "false" | "no") {
            return true;
        }
    }
    false
}

/// `[guards].<class>.enabled`, through the census reader. The pair is written
/// as literals at each call site, which is what lets the census scanner read
/// what this module reads without running it.
pub fn enabled(class: Class, config: &toml::Table) -> bool {
    let read = match class {
        Class::ShellTrap => policy::read("guards", "shell-trap.enabled", config),
        Class::Record => policy::read("guards", "record.enabled", config),
        Class::ReleaseRef => policy::read("guards", "release-ref.enabled", config),
        Class::ProductionWrite => policy::read("guards", "production-write.enabled", config),
    };
    match read {
        Ok(Some(toml::Value::Boolean(false))) => false,
        // Opt-out, never opt-in: an absent key, a value of another type, and a
        // pair the census somehow refused all leave the class on.
        _ => true,
    }
}

/// `[project] item_prefix`, read straight off the table it lives in. It is a
/// target rather than a policy key — a check reads it to know what an id looks
/// like — so it is not one of the census pairs.
pub fn item_prefix(config: &toml::Table) -> Option<String> {
    config
        .get("project")?
        .as_table()?
        .get("item_prefix")?
        .as_str()
        .map(str::to_string)
}

/// The targets the two pack classes read, all in the project's own
/// `[guards.targets]` table and all through the census, so the pairs a guard
/// reads can be read out of this source without running it (packs PRD R18).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    pub release_ref_glob: Option<String>,
    pub prod_buckets: Vec<String>,
    pub prod_projects: Vec<String>,
    pub prod_apps: Vec<String>,
    pub prod_make_goals: Vec<String>,
    pub prod_dagger_functions: Vec<String>,
    pub prod_workflow_refs: Vec<String>,
}

pub fn targets(config: &toml::Table) -> Targets {
    Targets {
        release_ref_glob: string_target(policy::read("guards.targets", "release_ref_glob", config)),
        prod_buckets: list_target(policy::read("guards.targets", "prod_buckets", config)),
        prod_projects: list_target(policy::read("guards.targets", "prod_projects", config)),
        prod_apps: list_target(policy::read("guards.targets", "prod_apps", config)),
        prod_make_goals: list_target(policy::read("guards.targets", "prod_make_goals", config)),
        prod_dagger_functions: list_target(policy::read(
            "guards.targets",
            "prod_dagger_functions",
            config,
        )),
        prod_workflow_refs: list_target(policy::read(
            "guards.targets",
            "prod_workflow_refs",
            config,
        )),
    }
}

/// A string target, or `None`. AN EMPTY STRING IS NOT CONFIGURED: a key written
/// and left blank is a key nobody filled in, and a glob matching nothing is what
/// it would otherwise mean.
fn string_target(read: Result<Option<&toml::Value>, policy::Unlisted>) -> Option<String> {
    match read {
        Ok(Some(toml::Value::String(value))) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

/// A list target. A value of another type, a pair the census refused and an
/// absent key all read as the empty list, which refuses nothing; non-string
/// entries are dropped rather than stringified, because a target nobody can type
/// at a prompt is one no command can name.
fn list_target(read: Result<Option<&toml::Value>, policy::Unlisted>) -> Vec<String> {
    match read {
        Ok(Some(toml::Value::Array(values))) => values
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the target `check` needs is configured in this policy. A check with
/// no target is always configured; every other answer is read off the policy the
/// caller resolved, so the check flag and the judgment cannot disagree.
pub fn configured(class: Class, check: &str, policy: &Policy) -> bool {
    match class.target_of(check) {
        None => true,
        Some(ITEM_PREFIX_KEY) => policy.item_prefix.is_some(),
        Some(RELEASE_REF_GLOB_KEY) => policy.release_ref_glob.is_some(),
        Some(PROD_BUCKETS_KEY) => !policy.prod_buckets.is_empty(),
        Some(PROD_PROJECTS_KEY) => !policy.prod_projects.is_empty(),
        Some(PROD_APPS_KEY) => !policy.prod_apps.is_empty(),
        Some(PROD_MAKE_GOALS_KEY) => !policy.prod_make_goals.is_empty(),
        Some(PROD_DAGGER_FUNCTIONS_KEY) => !policy.prod_dagger_functions.is_empty(),
        Some(PROD_WORKFLOW_REFS_KEY) => !policy.prod_workflow_refs.is_empty(),
        Some(_) => false,
    }
}

/// The same readings over a file's TEXT, so the caller resolves paths and this
/// module does every parse. An unreadable or unparsable file reads as an empty
/// table: opt-out, never opt-in, and nothing printed.
pub fn enabled_in(class: Class, text: &str) -> bool {
    enabled(class, &text.parse::<toml::Table>().unwrap_or_default())
}

pub fn item_prefix_in(text: &str) -> Option<String> {
    item_prefix(&text.parse::<toml::Table>().unwrap_or_default())
}

pub fn targets_in(text: &str) -> Targets {
    targets(&text.parse::<toml::Table>().unwrap_or_default())
}

/// The application a deployment configuration file names, read off that file's
/// text. It is the caller's reading handed to this module to PARSE rather than
/// to find: nothing here opens a path.
pub fn app_in(text: &str) -> Option<String> {
    text.parse::<toml::Table>()
        .ok()?
        .get("app")?
        .as_str()
        .map(str::to_string)
}

// ---- the surface table both classes read ------------------------------------
//
// The free text of each guarded call: how many leading positionals to step over
// before the text starts, and which flag values carry more. `None` reads no
// positional at all.
//
// ONE TABLE, TWO CLASSES. The record class reads these arguments to find a
// suffix nobody can resolve; the shell-trap class reads the same ones to find a
// backtick that will run. A second table would agree with the first until the
// day one of them gained a surface, and the pair whose whole point is that both
// classes guard the same writes is the pair that must not drift.
//
// Positionals are read only up to the first token beginning with `-`, which is
// exactly what the work graph's own usage lines make text and removes any need
// to guess which of its flags consume a following value.

pub struct Surface {
    pub subcommand: &'static str,
    pub positionals: Option<usize>,
    pub flags: &'static [&'static str],
}

/// `update`'s replacing fields have no additive twin, so their text is written
/// once and read for years. `--title` has no short form — the work graph's `-t`
/// is `--type`.
pub const SURFACES: [Surface; 5] = [
    Surface {
        subcommand: "note",
        positionals: Some(1),
        flags: &[],
    },
    Surface {
        subcommand: "comment",
        positionals: Some(1),
        flags: &[],
    },
    Surface {
        subcommand: "create",
        positionals: Some(0),
        flags: &["--description", "-d", "--notes", "--append-notes"],
    },
    Surface {
        subcommand: "close",
        positionals: None,
        flags: &["--reason", "-r"],
    },
    Surface {
        subcommand: "update",
        positionals: None,
        flags: &[
            "--title",
            "--description",
            "-d",
            "--acceptance",
            "--design",
            "--append-notes",
        ],
    },
];

/// The command word every guarded call is made through: the work graph's.
pub const WORK_GRAPH: &str = "bd";

/// The work graph's subcommand for this statement, or `None` where the
/// statement invokes something else.
pub fn subcommand_of(current: &[&lex::Token]) -> Option<String> {
    let head = current.first()?;
    if lex::basename(&head.text) != WORK_GRAPH {
        return None;
    }
    current[1..]
        .iter()
        .find(|w| !w.text.starts_with('-'))
        .map(|w| w.text.clone())
}

/// The stored-text arguments of one statement, as the words they were typed
/// as: the record class reads their value, the shell-trap class their quoting.
pub fn text_arguments<'t>(current: &[&'t lex::Token]) -> Vec<&'t lex::Token> {
    let Some(subcommand) = subcommand_of(current) else {
        return Vec::new();
    };
    let Some(surface) = SURFACES.iter().find(|s| s.subcommand == subcommand) else {
        return Vec::new();
    };

    // Everything after the subcommand word itself, which is the statement's
    // first non-flag argument and may sit behind flags of its own.
    let after = current[1..]
        .iter()
        .position(|w| !w.text.starts_with('-'))
        .map(|p| p + 1)
        .unwrap_or(0);
    let arguments: Vec<&lex::Token> = current[after + 1..].to_vec();

    let mut found = Vec::new();
    if let Some(skip) = surface.positionals {
        for (seen, word) in arguments.iter().enumerate() {
            if word.text.starts_with('-') {
                break;
            }
            if seen >= skip {
                found.push(*word);
            }
        }
    }

    let mut index = 0;
    while index < arguments.len() {
        let (name, inline) = match arguments[index].text.split_once('=') {
            Some((name, _)) => (name.to_string(), true),
            None => (arguments[index].text.clone(), false),
        };
        if surface.flags.contains(&name.as_str()) {
            if inline {
                found.push(arguments[index]);
                index += 1;
            } else {
                if index + 1 < arguments.len() {
                    found.push(arguments[index + 1]);
                }
                index += 2;
            }
            continue;
        }
        index += 1;
    }
    found
}
