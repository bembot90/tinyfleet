//! The census, which every key a verb reads has to be in. Two arms: that every
//! table it names is one of the eight, and that every reader call site in this
//! workspace's source names a pair the census carries.

mod common;

use common::workspace;
use fleet_core::policy;

#[test]
fn the_census_names_only_the_eight_tables() {
    for (table, key) in policy::CENSUS {
        assert!(
            policy::TABLES.contains(&table),
            "the census names [{table}] for `{key}`, which is not one of {:?}",
            policy::TABLES
        );
    }
    assert_eq!(
        policy::CENSUS.len(),
        18,
        "the census is read, not empty — a shrunk list would satisfy the arm above saying nothing"
    );
    assert_eq!(
        policy::TABLES.len(),
        8,
        "every table a pair names is listed, and no table nothing reads"
    );
    assert!(
        !policy::TABLES.contains(&"gates"),
        "[gates] is a table nothing reads — its keys moved by purpose"
    );
}

/// The two TEST commands are the workflow's and not the project's: neither is
/// a pair any verb may read, and each is one [`policy::MOVED`] names with the
/// pack setting that replaces it — so a file still setting one is refused by
/// name rather than read as absent. The table they were set in is refused
/// beside them, on its own line.
#[test]
fn the_test_commands_are_not_in_the_census_and_each_names_where_it_moved() {
    for (key, setting) in [("suite", "takeoff.test"), ("touched", "takeoff.touched")] {
        assert!(
            !policy::in_census("gates", key),
            "[gates] {key} is not a pair a verb may read"
        );
        let config: toml::Table = format!("[gates]\n{key} = \"make check\"\n")
            .parse()
            .expect("the fixture config parses");
        let found = policy::moved(&config);
        assert_eq!(
            found.len(),
            2,
            "[gates] {key} and the table are found set: {found:?}"
        );
        let said = found[0].to_string();
        assert!(
            said.contains(&format!("[gates] {key}"))
                && said.contains(&format!("`{setting}` under [packs.tiny]")),
            "the line names the key and the pack setting that replaces it: {said}"
        );
        assert_eq!(found[1].key, None, "the second line is the table's own");
    }
}

/// `[gates]` is refused BY NAME, whatever it carries — an empty table
/// included — and the line names where each of its keys is set now.
#[test]
fn a_gates_table_is_refused_by_name_and_names_the_new_homes() {
    for text in [
        "[gates]\n",
        "[gates]\nci_marker = \"printf x\"\n",
        "[gates]\ntool_commands = [\"cargo\"]\n",
        "[gates]\nrelease_ref_glob = \"refs/heads/release/*\"\n",
    ] {
        let config: toml::Table = text.parse().expect("the fixture config parses");
        let found = policy::moved(&config);
        assert_eq!(found.len(), 1, "{text:?} is refused once: {found:?}");
        assert_eq!(
            found[0].to_string(),
            "[gates] is not a policy table, and nothing reads it — its keys are set by \
             purpose: `ci_marker` under [landing], `tool_commands` under [permissions], and \
             `release_ref_glob` and the `prod_*` lists under [guards.targets]; move each one \
             there, and delete the table",
            "the line names the table and every new home"
        );
    }

    // The control: the three new homes, each carrying its key, are refused
    // nothing — so the answers above are about the table's name and not about
    // any key found under any table.
    let moved_home: toml::Table = "[landing]\nci_marker = \"printf x\"\n\n\
                                   [permissions]\ntool_commands = [\"cargo\"]\n\n\
                                   [guards.targets]\nrelease_ref_glob = \"refs/heads/r/*\"\n"
        .parse()
        .expect("the fixture config parses");
    assert_eq!(policy::moved(&moved_home), Vec::new());
}

/// Each key the old table held is a census pair under its new home and under
/// no other, and reads its value from there.
#[test]
fn each_key_the_gates_table_held_is_read_from_its_new_home() {
    let config: toml::Table = "[landing]\nci_marker = \"printf m\"\n\n\
                               [permissions]\ntool_commands = [\"cargo\"]\n\n\
                               [guards.targets]\nrelease_ref_glob = \"refs/heads/r/*\"\n"
        .parse()
        .expect("the fixture config parses");
    for (table, key, value) in [
        ("landing", "ci_marker", "\"printf m\""),
        ("permissions", "tool_commands", "[\"cargo\"]"),
        ("guards.targets", "release_ref_glob", "\"refs/heads/r/*\""),
    ] {
        let read = policy::read(table, key, &config)
            .expect("a census pair is readable")
            .expect("the value is there");
        assert_eq!(read.to_string(), value, "[{table}] {key}");
        policy::read("gates", key, &config)
            .expect_err("the old home is not a pair any verb may read");
    }
}

/// The production-write class's six target lists, all in `[guards.targets]`.
/// A key the census does not name is a key `policy::read` refuses, so the guard
/// that wanted it reads an empty list and refuses nothing — silently, which is
/// the direction this arm exists to catch.
#[test]
fn the_targets_table_names_every_list_the_production_write_class_reads() {
    for key in [
        "prod_buckets",
        "prod_projects",
        "prod_apps",
        // The three a PRODUCT declares rather than a cloud: the build-tool
        // goals that deploy, the module functions declared as production
        // writes, and the workflow-and-ref pairs a dispatch reaches production
        // through.
        "prod_make_goals",
        "prod_dagger_functions",
        "prod_workflow_refs",
    ] {
        assert!(
            policy::in_census("guards.targets", key),
            "[guards.targets] {key} is a list the production-write class reads"
        );
    }
    // The control, in the same read: a neighbouring spelling the reader would
    // not accept, so the six answers above are about the census rather than
    // about a matcher that says yes to anything.
    assert!(!policy::in_census("guards.targets", "prod_make_goal"));
    assert!(!policy::in_census("guards.targets", "prod_workflows"));
}

/// The flight table's three keys something still reads: the rules `fleet
/// status` prints, and the landing lane's two — where the fleet's own worktree
/// for a project goes, and how long a rerun waits for the box.
#[test]
fn the_flight_table_carries_only_the_keys_something_reads() {
    for key in ["rules", "lanes", "rerun_wait_seconds"] {
        assert!(
            policy::in_census("core.flight", key),
            "[core.flight] {key} is read and the census does not name it"
        );
    }
    assert!(
        !policy::in_census("core.flight", "max_items"),
        "no verb reads a cap on how many items a flight may take"
    );
    // The run's own cap sits in its own table, and neither spelling reaches the
    // other's.
    assert!(policy::in_census("core.run", "max_open"));
    assert!(
        !policy::in_census("core.flight", "max_runs"),
        "the run's cap is `[core.run] max_open` and not a key under the flight's table"
    );
}

/// The keys the flight engine read, left behind when it moved out of core:
/// none is a pair any verb may read, and each is one [`policy::RETIRED`] names
/// — so a file still setting one is refused by name rather than read as absent
/// by a person who believes it is in force.
#[test]
fn the_keys_nothing_reads_are_not_in_the_census_and_each_is_refused_by_name() {
    for (table, key) in [
        ("core", "max_returns"),
        ("core.flight", "max_open"),
        ("core.flight", "max_seats"),
        ("core.flight", "review"),
        ("core.flight", "escape_window_days"),
        ("core.flight", "max_crashes"),
        ("project", "trunk"),
    ] {
        assert!(
            !policy::in_census(table, key),
            "[{table}] {key} is not a pair a verb may read"
        );
        let config: toml::Table = format!("[{table}]\n{key} = 3\n")
            .parse()
            .expect("the fixture config parses");
        let found = policy::moved(&config);
        assert_eq!(found.len(), 1, "[{table}] {key} is found set: {found:?}");
        assert_eq!(
            found[0].to_string(),
            format!("[{table}] {key} is no longer read — delete it"),
            "the line names the key and says to delete it"
        );
    }
    assert_eq!(policy::RETIRED.len(), 7);

    // The control: the keys beside them that ARE read are not refused, in the
    // same tables — so the seven answers above are about the list rather than
    // about a lookup that refuses any key under a table it names.
    let read: toml::Table = "[core]\nreviewer = \"a-reviewer\"\n\n[core.flight]\n\
                             lanes = \"/l\"\n\n[project]\nprimary = \"/p\"\n"
        .parse()
        .expect("the fixture config parses");
    assert_eq!(policy::moved(&read), Vec::new());
}

/// The `project` table's two paths, which a spawn resolves the worktrees
/// directory and the primary checkout from.
#[test]
fn the_project_tables_two_paths_are_read_and_its_other_keys_are_not() {
    let config: toml::Table = "[project]\nname = \"a-project\"\nprimary = \"/p\"\n\
                               worktrees = \"/wt\"\n"
        .parse()
        .expect("the fixture config parses");

    assert_eq!(
        policy::read("project", "primary", &config)
            .expect("a census pair is readable")
            .and_then(toml::Value::as_str),
        Some("/p")
    );
    assert_eq!(
        policy::read("project", "worktrees", &config)
            .expect("a census pair is readable")
            .and_then(toml::Value::as_str),
        Some("/wt")
    );
    // The control: `name` is in the same table, is READ by `project_name`
    // through its own function, and is not a pair this reader may reach.
    policy::read("project", "name", &config)
        .expect_err("the value is there and is still not returned");
}

/// The run's cap, which is read the way the flight's is and falls back to its
/// own number.
#[test]
fn the_runs_cap_is_read_from_its_own_table_and_defaults_to_four() {
    use fleet_core::item::run;

    let named: toml::Table = "[core.run]\nmax_open = 2\n"
        .parse()
        .expect("the fixture config parses");
    assert_eq!(run::max_open(&named).expect("the cap reads"), 2);

    // A fleet that capped its FLIGHTS at one has not capped its runs: the two
    // tables are read apart, and the run's default is four.
    let flights_only: toml::Table = "[core.flight]\nmax_open = 1\n"
        .parse()
        .expect("the fixture config parses");
    assert_eq!(
        run::max_open(&flights_only).expect("the cap reads"),
        run::MAX_OPEN
    );
    assert_eq!(run::MAX_OPEN, 4);

    // A cap that is not a whole number is refused rather than defaulted.
    let wrong: toml::Table = "[core.run]\nmax_open = \"two\"\n"
        .parse()
        .expect("the fixture config parses");
    run::max_open(&wrong).expect_err("a cap has to be a whole number");
}

#[test]
fn the_reader_answers_an_unlisted_pair_with_an_error_and_never_a_value() {
    let config: toml::Table = "[core]\nreviewer = \"reviewer\"\nnovel = \"x\"\n"
        .parse()
        .expect("the fixture config parses");

    let listed = policy::read("core", "reviewer", &config).expect("a census pair is readable");
    assert_eq!(listed.and_then(toml::Value::as_str), Some("reviewer"));

    let unlisted = policy::read("core", "novel", &config)
        .expect_err("the value is there and is still not returned");
    assert_eq!(unlisted.table, "core");
    assert_eq!(unlisted.key, "novel");
}

#[test]
fn the_guards_table_is_a_map_and_its_census_entry_is_the_pattern() {
    let config: toml::Table = "[guards]\nshell-trap = { enabled = true }\n"
        .parse()
        .expect("the fixture config parses");
    let value = policy::read("guards", "shell-trap.enabled", &config)
        .expect("every guard's row matches the pattern");
    assert_eq!(value.and_then(toml::Value::as_bool), Some(true));

    policy::read("guards", "shell-trap.mode", &config)
        .expect_err("the pattern covers `enabled` and no other key of the row");
    policy::read("guards", "enabled", &config)
        .expect_err("the pattern wants a guard name before the key");
}

#[test]
fn a_census_key_the_config_omits_reads_as_absent_and_not_as_an_error() {
    let config: toml::Table = "[core]\nreviewer = \"reviewer\"\n"
        .parse()
        .expect("the fixture config parses");
    assert!(policy::read("landing", "ci_marker", &config)
        .expect("the pair is in the census")
        .is_none());
}

#[test]
fn every_reader_call_site_in_the_workspace_names_a_census_pair() {
    let sources = source_files();
    assert!(
        sources.iter().any(|p| p.ends_with("core/src/policy.rs")),
        "the scan read the source tree it claims to: {sources:?}"
    );

    for path in &sources {
        let text = std::fs::read_to_string(path).expect("a source file is readable");
        for site in call_sites(&text) {
            match site {
                Err(snippet) => panic!(
                    "{}: a reader call site whose table and key are not literals: {snippet}",
                    path.display()
                ),
                Ok((table, key)) => assert!(
                    policy::in_census(&table, &key),
                    "{}: reads [{table}] {key}, which the census does not name",
                    path.display()
                ),
            }
        }
    }
}

// The positive control for the arm above: with no call site in the tree yet, a
// scan that read nothing would pass it in silence. This drives the same scanner
// over text that HOLDS one and asserts both halves — that it finds the pair,
// and that an unlisted pair is what the census check would reject.
#[test]
fn the_scanner_finds_a_call_site_and_the_census_rejects_an_unlisted_pair() {
    let source = "fn f(c: &toml::Table) {\n    \
                  let _ = policy::read(\"core\", \"reviewer\", c);\n    \
                  let _ = crate::policy::read(\n        \"core\",\n        \"novel\",\n        c);\n}\n";
    let sites = call_sites(source);
    assert_eq!(
        sites,
        vec![
            Ok(("core".to_string(), "reviewer".to_string())),
            Ok(("core".to_string(), "novel".to_string())),
        ],
        "both call sites are read, across one line and across four"
    );
    assert!(policy::in_census("core", "reviewer"));
    assert!(!policy::in_census("core", "novel"));

    let malformed =
        "let pair = (\"core\", \"reviewer\");\nlet _ = policy::read(pair.0, pair.1, c);\n";
    assert!(
        call_sites(malformed)[0].is_err(),
        "a call site whose pair is not literal is reported, never skipped"
    );
}

#[test]
fn no_second_policy_module_in_the_workspace_defines_a_read() {
    let modules = policy_modules();
    assert!(
        modules
            .iter()
            .any(|p| p.ends_with("controller/src/policy.rs")),
        "the scan read the policy modules it claims to: {modules:?}"
    );
    assert!(
        defines_read("pub fn read(path: &Path) -> Result<Policy, String> {"),
        "the check recognises a `read` definition"
    );

    for path in &modules {
        let text = std::fs::read_to_string(path).expect("a source file is readable");
        assert!(
            !defines_read(&text),
            "{}: defines a function named `read` — the census arm matches the text \
             `{CALL}`, so a call to this one reads as the core reader's call site; give it \
             another name",
            path.display()
        );
    }
}

const CALL: &str = "policy::read(";

/// Every reader call site in one source file, as the literal pair it names.
/// `Err` is a call site whose first two arguments are not string literals: it
/// cannot be read out of the source, which is the whole point of the form.
fn call_sites(source: &str) -> Vec<Result<(String, String), String>> {
    let mut sites = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find(CALL) {
        rest = &rest[at + CALL.len()..];
        let snippet: String = rest.chars().take(60).collect();
        let Some((table, after)) = literal(rest) else {
            sites.push(Err(snippet));
            continue;
        };
        let after = after.trim_start();
        let Some(after) = after.strip_prefix(',') else {
            sites.push(Err(snippet));
            continue;
        };
        let Some((key, _)) = literal(after) else {
            sites.push(Err(snippet));
            continue;
        };
        sites.push(Ok((table, key)));
    }
    sites
}

/// The next argument, when it is a string literal written where the argument
/// starts. Anything else answers `None` rather than searching ahead, so a
/// non-literal call site is never read as the literal belonging to the next.
fn literal(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let body = s.strip_prefix('"')?;
    let close = body.find('"')?;
    Some((body[..close].to_string(), &body[close + 1..]))
}

fn source_files() -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for crate_dir in ["core", "cli"] {
        collect(&workspace().join(crate_dir).join("src"), &mut found);
    }
    found.sort();
    found
}

/// Every `policy.rs` and `policy/mod.rs` under a workspace crate's `src`, the
/// core crate's own reader excepted.
fn policy_modules() -> Vec<std::path::PathBuf> {
    let mut sources = Vec::new();
    let crates = std::fs::read_dir(workspace()).expect("the workspace is readable");
    for entry in crates {
        let src = entry
            .expect("a directory entry is readable")
            .path()
            .join("src");
        if src.is_dir() {
            collect(&src, &mut sources);
        }
    }
    let mut modules: Vec<_> = sources
        .into_iter()
        .filter(|p| p.ends_with("policy.rs") || p.ends_with("policy/mod.rs"))
        .filter(|p| !p.ends_with("core/src/policy.rs"))
        .collect();
    modules.sort();
    modules
}

fn defines_read(source: &str) -> bool {
    source.match_indices("fn read").any(|(at, found)| {
        source[at + found.len()..]
            .trim_start()
            .starts_with(['(', '<'])
    })
}

fn collect(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("a crate's src directory is readable");
    for entry in entries {
        let path = entry.expect("a directory entry is readable").path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            into.push(path);
        }
    }
}
