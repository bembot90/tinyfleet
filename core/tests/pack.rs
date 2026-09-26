//! The pack format. One arm per defect class the check knows, each built as a
//! real folder. The packs fleet's users install live in the fleet-packs
//! repository, and the checks on their own content run there.

mod common;

use common::{fixture_tiny, Defaults, Fixture};
use fleet_core::guard::{self, Class};
use fleet_core::pack::{self, Defect};
use fleet_core::resolve;

fn defects(root: &std::path::Path) -> Vec<Defect> {
    pack::check(root).defects
}

/// The bottom layer is a DIRECTORY and not a pack, so it has no manifest to
/// check — what it owes the format instead is that every file it carries sits
/// under a slot the resolver walks. A default outside one is a file no verb can
/// ever resolve.
#[test]
fn every_embedded_default_sits_under_a_slot_the_resolver_walks() {
    let outside: Vec<&str> = fleet_core::embedded::paths()
        .filter(|path| {
            !pack::SLOTS
                .iter()
                .any(|slot| path.starts_with(&format!("{slot}/")))
        })
        .collect();
    assert_eq!(
        outside,
        Vec::<&str>::new(),
        "a default outside the slots resolves through no layer"
    );

    let mut tops: Vec<&str> = fleet_core::embedded::paths()
        .filter_map(|path| path.split('/').next())
        .collect();
    tops.sort();
    tops.dedup();
    assert_eq!(
        tops,
        vec!["assets", "doctor", "overlay"],
        "the defaults carry the record templates, the guards' wiring and their health checks"
    );

    let defaults = Defaults::new("slots");
    assert!(
        !defaults.path().join(pack::MANIFEST).exists(),
        "the bottom layer carries no manifest: it is not a pack"
    );
}

/// What a seat hands in is read against the binary's own type, and the schema
/// its brief shows is a default like any other: embedded, and listed so a pack
/// on top may reword what a seat reads.
#[test]
fn the_defaults_carry_the_three_input_schemas_and_the_registry_lists_them() {
    use fleet_core::input::{DELIVERY_SCHEMA, FINDINGS_SCHEMA, QUESTION_SCHEMA};
    use fleet_core::registry;

    let registry = registry::parse(
        std::str::from_utf8(
            fleet_core::embedded::bytes(registry::REGISTRY).expect("the set carries the registry"),
        )
        .expect("the registry is UTF-8"),
    )
    .expect("the registry parses");
    for schema in [DELIVERY_SCHEMA, QUESTION_SCHEMA, FINDINGS_SCHEMA] {
        assert!(
            fleet_core::embedded::bytes(schema).is_some(),
            "the defaults carry `{schema}`"
        );
        assert!(
            registry.lists(schema),
            "and the registry lists `{schema}`: {:?}",
            registry.shadows
        );
    }
}

/// Fleet writes no notes, so no note has a template left: the seven the verbs
/// once rendered are in no directory of the embedded tree, and the shadow
/// registry lists none of them for a pack to replace.
#[test]
fn the_defaults_carry_no_note_template_and_the_registry_lists_none() {
    use fleet_core::registry;

    const NOTES: [&str; 7] = [
        "dispatch-note.md",
        "delivery-note.md",
        "verdict.md",
        "landing-note.md",
        "park-note.md",
        "question-note.md",
        "answer-note.md",
    ];
    let named = |path: &str| NOTES.contains(&path.rsplit('/').next().unwrap_or(path));

    let embedded: Vec<&str> = fleet_core::embedded::paths().collect();
    let held: Vec<&&str> = embedded.iter().filter(|path| named(path)).collect();
    assert!(
        held.is_empty(),
        "the embedded tree holds a note template: {held:?}"
    );

    let registry = registry::parse(
        std::str::from_utf8(
            fleet_core::embedded::bytes(registry::REGISTRY).expect("the set carries the registry"),
        )
        .expect("the registry is UTF-8"),
    )
    .expect("the registry parses");
    let listed: Vec<&str> = registry
        .shadows
        .iter()
        .map(|shadow| shadow.path.as_str())
        .filter(|path| named(path))
        .collect();
    assert!(
        listed.is_empty(),
        "the registry lists a note template: {listed:?}"
    );

    // THE CONTROL: the same reading finds the templates that stay, so an empty
    // answer above is the tree's and not the reading's.
    assert!(
        embedded.contains(&"assets/brief.md") && registry.lists("assets/brief.md"),
        "the brief is embedded and listed: {:?}",
        registry.shadows
    );
}

#[test]
fn a_pack_holding_every_slot_correctly_has_no_defects() {
    let fixture = Fixture::new("whole");
    fixture
        .manifest("whole")
        .file("agents/dispatcher/agent.toml", "name = \"dispatcher\"\n")
        .file("agents/writer/prompt.template.md", "# writer\n")
        .file("skills/brief/SKILL.md", "# brief\n")
        .file("orders/nightly.toml", "cron = \"0 22 * * *\"\n")
        .file("doctor/binaries/doctor.toml", "name = \"binaries\"\n")
        .file("doctor/binaries/run.sh", "#!/bin/sh\nexit 0\n")
        .file("overlay/per-provider/claude/hook.py", "print()\n")
        .file("assets/brief.md", "the brief\n");

    let report = pack::check(&fixture.root);
    assert!(report.is_valid(), "unexpected: {:?}", report.defects);
    let mut slots: Vec<&str> = report.slots.iter().map(|s| s.name).collect();
    slots.sort();
    assert_eq!(
        slots,
        vec!["agents", "assets", "doctor", "orders", "overlay", "skills"],
        "one line per slot found, and the six are all the names there are"
    );
}

#[test]
fn a_ninth_top_level_name_is_a_defect() {
    let fixture = Fixture::new("ninth");
    fixture
        .manifest("ninth")
        .file("bin/dispatch", "#!/bin/sh\n");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::UnknownTopLevel("bin".into())]
    );
}

/// The head of a real Finder `.DS_Store`, then a byte no UTF-8 text holds: a
/// check that opened the file as text would fail on it, not skim past it.
const LITTER: &[u8] = b"\x00\x00\x00\x01Bud1\x00\x00\x10\x00\xff\xfe";

/// A file browser writes its bookkeeping into every directory it opens, so a
/// pack somebody has looked at in Finder carries a `.DS_Store` at its top
/// level and in every slot they clicked into. The check reads past it: the
/// report on the littered pack is the report on the clean one, down to the
/// entry count of each slot.
#[test]
fn os_litter_anywhere_in_a_pack_is_ignored_and_changes_nothing_the_check_reports() {
    let fixture = Fixture::new("litter");
    fixture
        .manifest("litter")
        .file("agents/dispatcher/agent.toml", "name = \"dispatcher\"\n")
        .file("skills/brief/SKILL.md", "# brief\n")
        .file("orders/nightly.toml", "cron = \"0 22 * * *\"\n")
        .file("doctor/binaries/doctor.toml", "name = \"binaries\"\n")
        .file("assets/brief.md", "the brief\n");
    let clean = pack::check(&fixture.root);
    assert!(
        clean.is_valid(),
        "the pack is clean before the litter lands: {:?}",
        clean.defects
    );

    for dir in [
        "",
        "agents/",
        "skills/",
        "orders/",
        "doctor/",
        "assets/",
        "skills/brief/",
    ] {
        for name in [".DS_Store", "Thumbs.db", "desktop.ini"] {
            std::fs::write(fixture.path(&format!("{dir}{name}")), LITTER)
                .expect("the litter is written");
        }
    }
    let littered = pack::check(&fixture.root);
    assert_eq!(
        littered.defects,
        Vec::<Defect>::new(),
        "the litter is no defect at the top level or in any slot"
    );
    assert_eq!(
        littered.slots, clean.slots,
        "and no slot counts it as an entry"
    );

    // The control: the ignore is those names exactly, not every dotted or
    // unfamiliar one, so a top-level name that merely looks like litter is
    // still the typo the check exists to name.
    fixture.file(".DS_Store.bak", "x\n");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::UnknownTopLevel(".DS_Store.bak".into())]
    );
}

#[test]
fn an_unknown_manifest_key_is_a_defect_and_is_named() {
    let fixture = Fixture::new("key");
    fixture.file(
        "pack.toml",
        "[pack]\nname = \"key\"\nversion = \"1\"\nschema = 3\nauthor = \"somebody\"\n",
    );
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::UnknownManifestKey("author".into())]
    );
}

#[test]
fn an_unknown_top_level_table_is_a_defect_and_is_named() {
    let fixture = Fixture::new("table");
    fixture.file(
        "pack.toml",
        "[pack]\nname = \"t\"\nversion = \"1\"\nschema = 3\n\n[exports]\nx = 1\n",
    );
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::UnknownManifestTable("exports".into())]
    );
}

/// The PREVIOUS number is the case that matters: there is no dual read, so a
/// manifest still declaring 2 is refused by name rather than half-understood.
/// The 4 arm is its control — a refusal that named only the older number would
/// be a dual read spelled differently.
#[test]
fn a_schema_that_is_not_the_current_one_is_refused_naming_the_number() {
    for declared in [2, 4] {
        let fixture = Fixture::new("schema");
        fixture.file(
            "pack.toml",
            &format!("[pack]\nname = \"s\"\nversion = \"1\"\nschema = {declared}\n"),
        );
        let found = defects(&fixture.root);
        assert_eq!(found, vec![Defect::Schema(declared)], "schema = {declared}");
        let line = found[0].to_string();
        assert!(
            line.contains(&declared.to_string()) && line.contains('3'),
            "the line names the number it read and the number it wants: {line}"
        );
    }
}

#[test]
fn a_manifest_without_a_name_is_a_defect() {
    let fixture = Fixture::new("noname");
    fixture.file("pack.toml", "[pack]\nversion = \"1\"\nschema = 3\n");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::MissingManifestKey("pack.name".into())]
    );
}

#[test]
fn a_folder_without_a_manifest_is_not_a_pack() {
    let fixture = Fixture::new("bare");
    fixture.dir("assets");
    assert_eq!(defects(&fixture.root), vec![Defect::NoManifest]);
}

#[test]
fn an_agent_directory_holding_neither_form_is_a_defect() {
    let fixture = Fixture::new("agent");
    fixture
        .manifest("agent")
        .file("agents/dispatcher/notes.md", "nothing\n");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::AgentWithoutDefinition("agents/dispatcher".into())]
    );
}

#[test]
fn both_agent_forms_are_accepted() {
    for form in pack::AGENT_FORMS {
        let fixture = Fixture::new("form");
        fixture
            .manifest("form")
            .file(&format!("agents/one/{form}"), "x\n");
        assert!(
            defects(&fixture.root).is_empty(),
            "`{form}` is one of the two forms measured across the reference packs"
        );
    }
}

#[test]
fn a_skills_directory_without_skill_md_is_a_defect() {
    let fixture = Fixture::new("skill");
    fixture
        .manifest("skill")
        .file("skills/brief/readme.md", "no\n");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::SkillWithoutSkillMd("skills/brief".into())]
    );
}

#[test]
fn a_doctor_directory_without_doctor_toml_is_a_defect() {
    let fixture = Fixture::new("doctor");
    fixture
        .manifest("doctor")
        .file("doctor/binaries/run.sh", "exit 0\n");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::DoctorWithoutDoctorToml("doctor/binaries".into())]
    );
}

#[test]
fn an_orders_file_that_is_not_toml_is_a_defect() {
    let fixture = Fixture::new("toml");
    fixture
        .manifest("toml")
        .file("orders/broken.toml", "this is not = = toml\n");
    let found = defects(&fixture.root);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        matches!(&found[0], Defect::NotToml { path, .. } if path == "orders/broken.toml"),
        "{found:?}"
    );
}

#[test]
fn a_registry_naming_a_path_the_pack_does_not_hold_is_a_defect() {
    let fixture = Fixture::new("registry");
    fixture.manifest("registry").file(
        "assets/shadow-registry.toml",
        "schema = 1\n\n[[shadow]]\npath = \"assets/gone.md\"\npurpose = \"the brief\"\n",
    );
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::RegistryPathMissing("assets/gone.md".into())]
    );
}

#[test]
fn every_further_defect_is_reported_and_not_only_the_first() {
    let fixture = Fixture::new("many");
    fixture
        .file(
            "pack.toml",
            "[pack]\nname = \"many\"\nversion = \"1\"\nschema = 2\nauthor = \"x\"\n",
        )
        .file("bin/dispatch", "#!/bin/sh\n")
        .file("skills/brief/readme.md", "no\n");
    let found = defects(&fixture.root);
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(found.contains(&Defect::UnknownTopLevel("bin".into())));
    assert!(found.contains(&Defect::UnknownManifestKey("author".into())));
    assert!(found.contains(&Defect::Schema(2)));
    assert!(found.contains(&Defect::SkillWithoutSkillMd("skills/brief".into())));
}

// ---- the adapters slot --------------------------------------------------------

/// An `adapter.toml` for `adapters/<kind>/<name>/` whose keys agree with where
/// it sits, its entry `main.sh`.
fn adapter_toml(kind: &str, name: &str) -> String {
    format!(
        "[adapter]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
         description = \"an adapter\"\nentry = \"main.sh\"\n"
    )
}

/// `adapters/<kind>/<name>/` in the fixture, its manifest `toml` and its entry
/// `main.sh` executable, answered as the directory.
fn an_adapter(fixture: &Fixture, kind: &str, name: &str, toml: &str) -> std::path::PathBuf {
    let dir = format!("adapters/{kind}/{name}");
    fixture
        .file(&format!("{dir}/adapter.toml"), toml)
        .file(&format!("{dir}/main.sh"), "#!/bin/sh\nexit 0\n");
    executable(&fixture.path(&format!("{dir}/main.sh")), true);
    fixture.path(&dir)
}

fn executable(file: &std::path::Path, on: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        file,
        std::fs::Permissions::from_mode(if on { 0o755 } else { 0o644 }),
    )
    .expect("the mode is set");
}

/// A store adapter and an agent adapter, each where its manifest says: no
/// defect, one `adapters` line counting the two, and the manifest as read.
///
/// RED-PROOF: on the base `adapters` is a ninth top-level name, a defect.
#[test]
fn adapters_under_both_kinds_are_the_eighth_slot_and_counted() {
    let fixture = Fixture::new("adapters");
    fixture.manifest("adapters");
    let store = an_adapter(&fixture, "store", "x", &adapter_toml("store", "x"));
    an_adapter(&fixture, "agent", "y", &adapter_toml("agent", "y"));

    let report = pack::check(&fixture.root);
    assert!(report.is_valid(), "unexpected: {:?}", report.defects);
    assert_eq!(
        report.slots,
        vec![pack::Slot {
            name: "adapters",
            entries: 2
        }],
        "an entry is `adapters/<kind>/<name>/`, one kind directory down"
    );
    assert_eq!(
        pack::adapter_manifest(&store),
        Ok(pack::AdapterManifest {
            name: "x".into(),
            kind: pack::AdapterKind::Store,
            version: "0.1.0".into(),
            description: Some("an adapter".into()),
            entry: "main.sh".into(),
            hook: None,
        })
    );
}

/// A kind directory that names no kind, and a manifest whose `kind` is not the
/// directory it is filed under, are each one defect naming the path.
#[test]
fn an_unknown_kind_directory_or_a_kind_key_that_disagrees_is_a_defect() {
    let fixture = Fixture::new("adapter-kind");
    fixture.manifest("adapter-kind");
    an_adapter(&fixture, "db", "x", &adapter_toml("db", "x"));
    an_adapter(&fixture, "store", "y", &adapter_toml("agent", "y"));

    let found = defects(&fixture.root);
    assert_eq!(
        found,
        vec![
            Defect::AdapterKind("adapters/db".into(), None),
            Defect::AdapterKind("adapters/store/y".into(), Some("agent".into())),
        ]
    );
    assert_eq!(
        found[0].to_string(),
        "`adapters/db` is no adapter kind — an adapter is filed under adapters/store/ or \
         adapters/agent/"
    );
    assert_eq!(
        found[1].to_string(),
        "`adapters/store/y/adapter.toml` says kind `agent`, and it is filed under \
         `adapters/store`"
    );
}

/// An adapter directory holding no manifest, and one whose manifest names
/// another adapter than its directory, are each a defect.
#[test]
fn an_adapter_without_its_manifest_or_under_another_name_is_a_defect() {
    let fixture = Fixture::new("adapter-name");
    fixture
        .manifest("adapter-name")
        .file("adapters/store/bare/main.sh", "#!/bin/sh\n");
    an_adapter(&fixture, "store", "x", &adapter_toml("store", "other"));

    let found = defects(&fixture.root);
    assert_eq!(
        found,
        vec![
            Defect::AdapterWithoutManifest("adapters/store/bare".into()),
            Defect::AdapterNameMismatch {
                path: "adapters/store/x".into(),
                name: "other".into()
            },
        ]
    );
    assert_eq!(
        found[1].to_string(),
        "`adapters/store/x/adapter.toml` names the adapter `other`, and its directory is `x`"
    );
}

/// An entry that is not in the directory is a defect, and one that is there
/// but lost its executable bit — as a checkout that drops modes leaves it —
/// is a defect whose line is the fix.
#[test]
fn an_adapter_entry_missing_or_not_executable_is_a_defect_naming_the_fix() {
    let fixture = Fixture::new("adapter-entry");
    fixture.manifest("adapter-entry");
    let dir = an_adapter(&fixture, "store", "x", &adapter_toml("store", "x"));
    std::fs::remove_file(dir.join("main.sh")).expect("the entry is removed");
    assert_eq!(
        defects(&fixture.root),
        vec![Defect::AdapterEntryMissing {
            path: "adapters/store/x".into(),
            entry: "main.sh".into()
        }]
    );

    fixture.file("adapters/store/x/main.sh", "#!/bin/sh\n");
    let entry = dir.join("main.sh");
    executable(&entry, false);
    let found = defects(&fixture.root);
    assert_eq!(
        found,
        vec![Defect::AdapterEntryNotExecutable(
            entry.display().to_string()
        )]
    );
    assert_eq!(
        found[0].to_string(),
        format!(
            "the adapter entry `{e}` is not executable — `chmod +x {e}` makes it one",
            e = entry.display()
        )
    );

    // The control: the line's own fix clears the defect.
    executable(&entry, true);
    assert_eq!(defects(&fixture.root), Vec::<Defect>::new());
}

/// Each of the manifest's keys is held to its shape: the four it requires,
/// strings all, no key beyond the five, and an entry inside the directory.
#[test]
fn an_adapter_manifest_key_out_of_shape_is_a_defect_naming_the_key() {
    let cases = [
        (
            "[adapter]\nname = \"x\"\nkind = \"store\"\nentry = \"main.sh\"\n",
            "version",
            "is missing",
        ),
        (
            "[adapter]\nname = \"x\"\nkind = \"store\"\nversion = 1\nentry = \"main.sh\"\n",
            "version",
            "is not a string",
        ),
        (
            "[adapter]\nname = \"x\"\nkind = \"store\"\nversion = \"1\"\nentry = \"main.sh\"\n\
             runtime = \"deno\"\n",
            "runtime",
            "is not a key [adapter] holds — it holds name, kind, version, description and entry",
        ),
        (
            "[adapter]\nname = \"x\"\nkind = \"store\"\nversion = \"1\"\n\
             entry = \"../main.sh\"\n",
            "entry",
            "is not a path inside the adapter's directory",
        ),
    ];
    for (toml, key, why) in cases {
        let fixture = Fixture::new("adapter-key");
        fixture.manifest("adapter-key");
        an_adapter(&fixture, "store", "x", toml);
        assert_eq!(
            defects(&fixture.root),
            vec![Defect::AdapterKey(
                "adapters/store/x".into(),
                key.into(),
                why
            )],
            "{toml}"
        );
    }
}

// ---- an agent adapter's hook mapping ----------------------------------------

/// A `[hook]` table no real agent speaks, in the shape the guards read.
const HOOK: &str = "\n[hook]\nshell_tool = \"sh\"\ntool = \"/call\"\ncommand = \"/line\"\n\
     deny = '{\"why\":{reason}}'\n";

/// An agent adapter carries its `[hook]` table beside `[adapter]`, read into
/// the manifest as written.
///
/// RED-PROOF: on the base `hook` is a table the file does not hold.
#[test]
fn an_agent_adapter_carries_its_hook_table() {
    let fixture = Fixture::new("adapter-hook");
    fixture.manifest("adapter-hook");
    let dir = an_adapter(
        &fixture,
        "agent",
        "y",
        &format!("{}{HOOK}", adapter_toml("agent", "y")),
    );
    assert_eq!(defects(&fixture.root), Vec::<Defect>::new());
    let hook = pack::adapter_manifest(&dir)
        .expect("the manifest reads")
        .hook
        .expect("the table is read, not merely tolerated");
    assert_eq!(hook["shell_tool"].as_str(), Some("sh"));
    assert_eq!(hook["deny"].as_str(), Some("{\"why\":{reason}}"));
}

/// A `[hook]` on a store adapter, a `deny` with no `{reason}`, and a pointer
/// that is not a JSON pointer are each one defect naming the adapter.
#[test]
fn a_hook_out_of_place_or_out_of_shape_is_a_defect() {
    let fixture = Fixture::new("adapter-hook-defects");
    fixture.manifest("adapter-hook-defects");
    an_adapter(
        &fixture,
        "store",
        "s",
        &format!("{}{HOOK}", adapter_toml("store", "s")),
    );
    an_adapter(
        &fixture,
        "agent",
        "a",
        &format!(
            "{}{}",
            adapter_toml("agent", "a"),
            HOOK.replace("{reason}", "\"no\"")
        ),
    );
    an_adapter(
        &fixture,
        "agent",
        "b",
        &format!(
            "{}{}",
            adapter_toml("agent", "b"),
            HOOK.replace("\"/line\"", "\"line\"")
        ),
    );

    let found = defects(&fixture.root);
    assert_eq!(found.len(), 3, "{found:?}");
    let lines: Vec<String> = found.iter().map(Defect::to_string).collect();
    assert_eq!(
        lines,
        vec![
            "`adapters/agent/a/adapter.toml`'s [hook] `deny` holds no {reason} — a refusal \
             that does not carry the reason is one the agent cannot act on",
            "`adapters/agent/b/adapter.toml`'s [hook] `command` is `line`, which is not a \
             JSON pointer — one starts with `/` and writes `~` as `~0`",
            "`adapters/store/s/adapter.toml`'s `[hook]` is a table only an agent adapter \
             holds — a store adapter holds [adapter] alone",
        ]
    );
}

// ---- the runtime table ------------------------------------------------------

/// The example the workflows page carries, verbatim down to the flag order, so
/// an arm here is a claim about a line a real pack would ship.
const RUNTIME: &str = "\n[runtime]\nname    = \"deno\"\nversion = \"2.4.5\"\n\
     bundle  = \"deno bundle {entry} --output {bundle}\"\n\
     run     = \"deno run --allow-run={fleet} --allow-read={run_dir} \
     --allow-write={run_dir} --allow-env=FLEET_ {bundle}\"\n";

fn with_runtime(body: &str) -> Result<pack::Manifest, Vec<Defect>> {
    pack::parse_manifest(&format!(
        "[pack]\nname = \"ts\"\nversion = \"0.1.0\"\nschema = 3\n{body}"
    ))
}

#[test]
fn a_manifest_carrying_the_runtime_table_parses_and_one_without_it_parses_too() {
    let with = with_runtime(RUNTIME).expect("the workflows page's example is a real manifest");
    let runtime = with
        .runtime
        .expect("the table is read, not merely tolerated");
    assert_eq!(runtime.name, "deno");
    assert_eq!(runtime.version, "2.4.5");
    assert_eq!(runtime.bundle, "deno bundle {entry} --output {bundle}");
    assert!(
        runtime.run.starts_with("deno run --allow-run={fleet}"),
        "{}",
        runtime.run
    );

    // The control: the table is optional, and a whole pack that carries no
    // workflows — the fixture shaped as the doctrine pack — is on this side of it.
    let without = with_runtime("").expect("a pack that carries no workflows declares no runtime");
    assert_eq!(without.runtime, None);
    let tiny = fixture_tiny();
    let manifest = pack::check(&tiny).manifest.expect("the fixture's manifest");
    assert_eq!(manifest.runtime, None, "{}", tiny.display());
}

/// The four-key rule, both ways round: a fifth key is named and so is a missing
/// one of the four. Each key gets its own arm of the loop, because a rule that
/// only ever refuses `bundle` would pass a one-key check.
#[test]
fn the_runtime_table_holds_exactly_four_keys_and_names_the_one_that_is_wrong() {
    let found = with_runtime(&format!("{RUNTIME}engine  = \"v8\"\n"))
        .expect_err("a fifth key in [runtime] is a defect");
    assert_eq!(found, vec![Defect::UnknownRuntimeKey("engine".into())]);
    assert!(found[0].to_string().contains("engine"), "{}", found[0]);

    for missing in ["name", "version", "bundle", "run"] {
        let body: String = RUNTIME
            .lines()
            .filter(|l| !l.starts_with(missing))
            .collect::<Vec<_>>()
            .join("\n");
        let found = with_runtime(&format!("{body}\n"))
            .expect_err(&format!("[runtime] without `{missing}` is a defect"));
        assert_eq!(
            found,
            vec![Defect::MissingManifestKey(format!("runtime.{missing}"))],
            "the manifest is refused BY THE NAME of the key it is missing"
        );
    }
}

/// The defect names the line it judged, so `bundle` and `run` are each built
/// wrong on their own: a check that only ever read the first string would pass
/// the `bundle` arm and prove nothing about the one core execs.
#[test]
fn a_runtime_template_s_unknown_placeholder_is_refused_when_the_manifest_is_read() {
    for key in ["bundle", "run"] {
        let body = if key == "bundle" {
            RUNTIME.replace("{entry}", "{source}")
        } else {
            RUNTIME.replace("--allow-read={run_dir}", "--allow-read={source}")
        };
        let found = with_runtime(&body).expect_err("a sixth placeholder name is a defect");
        assert_eq!(found.len(), 1, "{key}: {found:?}");
        let Defect::RuntimeTemplate { key: named, why } = &found[0] else {
            panic!("{key}: {found:?}");
        };
        assert_eq!(named, &format!("runtime.{key}"));
        assert!(
            why.contains("{source}") && why.contains("{run_dir}"),
            "the reason names the placeholder it read and the five core fills: {why}"
        );
    }
}

/// An unterminated brace is the other half of the same rule: a template core
/// cannot read a name out of is refused at the same point, not exec'd.
#[test]
fn a_runtime_template_s_brace_with_no_close_is_refused_too() {
    let body = RUNTIME.replace(
        "deno bundle {entry} --output {bundle}",
        "deno bundle {entry --output",
    );
    let found = with_runtime(&body).expect_err("a `{` with no `}` is a template core cannot fill");
    let Defect::RuntimeTemplate { key, why } = &found[0] else {
        panic!("{found:?}");
    };
    assert_eq!(key, "runtime.bundle");
    assert!(why.contains("no `}`"), "{why}");
}

#[test]
fn runtime_substitution_fills_the_five_names_and_leaves_an_unsupplied_one_standing() {
    let filled = pack::substitute(
        "deno run --allow-run={fleet} --allow-read={run_dir} {bundle}",
        &[("fleet", "/usr/local/bin/fleet"), ("run_dir", "/runs/17")],
    );
    assert_eq!(
        filled,
        "deno run --allow-run=/usr/local/bin/fleet --allow-read=/runs/17 {bundle}",
        "a name with no value is left standing: an argument silently blanked is a different command"
    );
    assert_eq!(
        pack::placeholders("deno bundle {entry} --output {bundle}").unwrap(),
        vec!["entry", "bundle"]
    );
    assert_eq!(pack::PLACEHOLDERS.len(), 5);
}

/// AC3 — the doctor shape the binary's defaults ship, run as a real script
/// against a fake runtime. No doctor runner exists in core, cli or controller
/// yet (the pack check validates the slot and nothing runs the entries), so
/// this arm is the only thing that executes it.
///
/// Four readings, because three of them are green-or-red and the fourth is the
/// one a pack pinning no runtime is in. The mismatch arm is what makes the
/// match arm worth anything: a check that answered `holds` on any output at all
/// would pass the first arm alone.
#[test]
fn the_runtime_doctor_shape_reads_the_pinned_version_against_the_binary_on_path() {
    let defaults = Defaults::new("doctor");
    let check = defaults.path().join("doctor/runtime-version/run.sh");
    assert!(
        check.is_file(),
        "the defaults ship the shape: {}",
        check.display()
    );

    let fixture = Fixture::new("doctor");
    fixture.file(
        "pack.toml",
        "[pack]\nname = \"ts\"\nversion = \"0.1.0\"\nschema = 3\n\n\
         [runtime]\nname    = \"deno\"\nversion = \"2.4.5\"\n\
         bundle  = \"deno bundle {entry} --output {bundle}\"\n\
         run     = \"deno run {bundle}\"\n",
    );
    fixture.dir("nothing");

    let fake = |label: &str, body: &str| -> std::path::PathBuf {
        let dir = fixture.path(label);
        std::fs::create_dir_all(&dir).expect("the fake runtime's directory");
        let bin = dir.join("deno");
        std::fs::write(&bin, body).expect("the fake runtime is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("the fake runtime is executable");
        }
        dir
    };

    let matching = fake(
        "pinned",
        "#!/bin/sh\necho \"deno 2.4.5 (stable, release, aarch64-apple-darwin)\"\necho \"v8 13.7.1\"\n",
    );
    let other = fake(
        "other",
        "#!/bin/sh\necho \"deno 2.4.4 (stable, release, aarch64-apple-darwin)\"\n",
    );

    let run = |path: &std::path::Path, pack: &std::path::Path| -> (i32, String) {
        let out = std::process::Command::new("/bin/sh")
            .arg(&check)
            .env("PATH", path)
            .env("FLEET_PACK_DIR", pack)
            .output()
            .expect("the check runs");
        (
            out.status
                .code()
                .expect("the check exits rather than signals"),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };

    let (code, said) = run(&matching, &fixture.root);
    assert_eq!(code, 0, "the pinned version on PATH: {said}");
    assert!(said.contains("runtime-version: holds"), "{said}");

    let (code, said) = run(&other, &fixture.root);
    assert_eq!(code, 1, "another version on PATH: {said}");
    assert!(said.contains("is not the pinned 2.4.5"), "{said}");

    // Absence, measured with the runtime's directory off PATH — which is also
    // what proves the check needs no tool of its own on PATH to answer.
    let (code, said) = run(&fixture.path("nothing"), &fixture.root);
    assert_eq!(code, 1, "no runtime on PATH: {said}");
    assert!(said.contains("did not answer (exit 127)"), "{said}");

    // A pack that pins no runtime — the fixture shaped as the doctrine pack:
    // the entry is the shape and not an instance, and a pack that carries no
    // workflows is not red for it.
    let (code, said) = run(&matching, &fixture_tiny());
    assert_eq!(code, 0, "a pack that pins no runtime: {said}");
    assert!(said.contains("runtime-version: nothing pinned"), "{said}");
}

/// fleet-2jt — the Claude Code pin's doctor check the defaults ship, run as a
/// real script against stub agents, and its copy of the pin held to
/// `supported::PINNED_CLAUDE_CODE`.
///
/// The mismatch arm is a claude answering 2.1.261, a release before the pin,
/// in the shape `claude --version` prints; absence is measured with no claude
/// on PATH at all. `FLEET_CLAUDE_BIN` is the seam the controller reads the
/// agent through, and names the binary over PATH.
#[test]
fn the_claude_code_doctor_check_reads_claude_version_against_the_pin() {
    let pin = fleet_core::supported::PINNED_CLAUDE_CODE;
    let defaults = Defaults::new("claude-doctor");
    let check = defaults.path().join("doctor/claude-code-version/run.sh");
    let script = std::fs::read_to_string(&check)
        .unwrap_or_else(|e| panic!("the defaults ship the check at {}: {e}", check.display()));
    assert_eq!(
        script
            .lines()
            .filter(|line| line.starts_with("SUPPORTED="))
            .collect::<Vec<_>>(),
        vec![format!("SUPPORTED={pin}").as_str()],
        "the check's one copy of the pin is supported::PINNED_CLAUDE_CODE"
    );

    let fixture = Fixture::new("claude-doctor-stubs");
    fixture.dir("nothing");
    let fake = |label: &str, answer: &str| -> std::path::PathBuf {
        let dir = fixture.path(label);
        std::fs::create_dir_all(&dir).expect("the fake agent's directory");
        let bin = dir.join("claude");
        std::fs::write(&bin, format!("#!/bin/sh\necho \"{answer}\"\n"))
            .expect("the fake agent is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("the fake agent is executable");
        dir
    };
    let pinned = fake("pinned", &format!("{pin} (Claude Code)"));
    let other = fake("other", "2.1.261 (Claude Code)");

    let run = |path: &std::path::Path, seam: Option<&std::path::Path>| -> (i32, String) {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg(&check)
            .env("PATH", path)
            .env_remove("FLEET_CLAUDE_BIN");
        if let Some(bin) = seam {
            cmd.env("FLEET_CLAUDE_BIN", bin);
        }
        let out = cmd.output().expect("the check runs");
        (
            out.status
                .code()
                .expect("the check exits rather than signals"),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };

    let (code, said) = run(&pinned, None);
    assert_eq!(code, 0, "the pinned claude on PATH: {said}");
    assert!(said.contains("claude-code-version: holds"), "{said}");

    let (code, said) = run(&other, None);
    assert_eq!(code, 1, "another claude on PATH: {said}");
    assert!(
        said.contains("answers: 2.1.261 (Claude Code)")
            && said.contains(&format!("is not the supported {pin}"))
            && said.contains("the controller still runs")
            && said.contains(&format!("claude install {pin}")),
        "the mismatch is named, with the line that installs the pin: {said}"
    );

    let (code, said) = run(&fixture.path("nothing"), None);
    assert_eq!(code, 1, "no claude on PATH: {said}");
    assert!(
        said.contains("did not answer (exit 127)")
            && said.contains(&format!(
                "curl -fsSL https://claude.ai/install.sh | bash -s {pin}"
            )),
        "{said}"
    );

    let (code, said) = run(&other, Some(&pinned.join("claude")));
    assert_eq!(
        code, 0,
        "the seam names the pinned claude over PATH's: {said}"
    );
    assert!(said.contains("claude-code-version: holds"), "{said}");
}

/// fleet-3krx.1 — the fleet-packs pin's doctor check the defaults ship, run as
/// a real script against a stub `fleet` whose `pack list` prints the lock, and
/// its two copies held to `supported::PINNED_PACKS_SOURCE` and `PINNED_PACKS`.
///
/// The table the stub prints is `fleet pack list`'s own shape: a header, then
/// one padded row per line of the lock. A line from any other source — a
/// checkout `fleet create --packs-from` named — is not read, whatever its
/// version.
#[test]
fn the_fleet_packs_doctor_check_reads_the_lock_against_the_pin() {
    let source = fleet_core::supported::PINNED_PACKS_SOURCE;
    let pin = fleet_core::supported::PINNED_PACKS;
    let defaults = Defaults::new("packs-doctor");
    let check = defaults.path().join("doctor/fleet-packs-version/run.sh");
    let script = std::fs::read_to_string(&check)
        .unwrap_or_else(|e| panic!("the defaults ship the check at {}: {e}", check.display()));
    let copies = |key: &str| -> Vec<String> {
        script
            .lines()
            .filter(|line| line.starts_with(&format!("{key}=")))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(
        copies("SOURCE"),
        vec![format!("SOURCE={source}")],
        "the check's one copy of the source is supported::PINNED_PACKS_SOURCE"
    );
    assert_eq!(
        copies("PINNED"),
        vec![format!("PINNED={pin}")],
        "the check's one copy of the tag is supported::PINNED_PACKS"
    );

    let fixture = Fixture::new("packs-doctor-stubs");
    let header = "name      source                                 version  commit    fetched";
    let row = |name: &str, from: &str, version: &str| {
        format!("{name}  {from}  {version}  0123abcd  2026-09-25T00:00:00Z")
    };
    let defaults_row = row("defaults", "embedded:defaults", "0.1.0");
    let tk = format!("{source}//adapters/store/tk");
    let ts = format!("{source}//runtimes/ts");
    let fleet = |label: &str, table: &str, exit: i32| -> std::path::PathBuf {
        let dir = fixture.path(label);
        std::fs::create_dir_all(&dir).expect("the fake fleet's directory");
        let bin = dir.join("fleet");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\ncat <<'EOF'\n{table}\nEOF\nexit {exit}\n"),
        )
        .expect("the fake fleet is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("the fake fleet is executable");
        bin
    };
    let run = |bin: &std::path::Path| -> (i32, String) {
        let out = std::process::Command::new("/bin/sh")
            .arg(&check)
            .env("FLEET_BIN", bin)
            .output()
            .expect("the check runs");
        (
            out.status
                .code()
                .expect("the check exits rather than signals"),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };

    // Nothing from the source: the defaults' line, and a checkout's pack at a
    // version that is not the pin, which is not read.
    let (code, said) = run(&fleet(
        "nothing",
        &[
            header.to_string(),
            row("tk", "/a/checkout//adapters/store/tk", "main"),
            defaults_row.clone(),
        ]
        .join("\n"),
        0,
    ));
    assert_eq!(code, 0, "nothing installed from the source: {said}");
    assert!(
        said.contains(&format!(
            "nothing installed from fleet-packs — no line of the lock names {source}"
        )),
        "{said}"
    );

    let (code, said) = run(&fleet(
        "holds",
        &[
            header.to_string(),
            row("tk", &tk, pin),
            defaults_row.clone(),
            row("ts", &ts, pin),
        ]
        .join("\n"),
        0,
    ));
    assert_eq!(code, 0, "every pack from the source at the pin: {said}");
    assert!(
        said.contains(&format!("fleet-packs-version: holds — tk, ts at {pin}")),
        "{said}"
    );

    let (code, said) = run(&fleet(
        "moved",
        &[
            header.to_string(),
            row("tk", &tk, "v0.0.9"),
            defaults_row.clone(),
            row("ts", &ts, pin),
        ]
        .join("\n"),
        0,
    ));
    assert_eq!(code, 1, "a pack from the source at another tag: {said}");
    assert!(
        said.contains(&format!(
            "tk is pinned at v0.0.9, not the supported {pin} — `fleet pack remove {tk}` and \
             then `fleet pack add {tk} --version {pin}`"
        )) && said.contains("broken — 1 pack(s) from fleet-packs")
            && said.contains("fleet still runs them")
            && !said.contains("ts is pinned"),
        "the one that moved is named with the lines that replace it: {said}"
    );

    let (code, said) = run(&fleet("unread", "", 3));
    assert_eq!(code, 3, "a lock fleet could not read: {said}");
    assert!(
        said.contains("could not read the lock — `fleet pack list` exited 3"),
        "{said}"
    );
}

// ---- the config table ---------------------------------------------------------

fn with_config(body: &str) -> Result<pack::Manifest, Vec<Defect>> {
    pack::parse_manifest(&format!(
        "[pack]\nname = \"tiny\"\nversion = \"0.1.0\"\nschema = 3\n{body}"
    ))
}

/// Both spellings of a dotted name read as the one setting, a default and a
/// type are kept, and a declaration may carry neither — only the description
/// is required.
#[test]
fn a_config_declaration_parses_in_either_spelling_with_its_default_and_type() {
    let manifest = with_config(
        "\n[config.\"takeoff.test\"]\ndescription = \"the builder's suite\"\n\
         \n[config.takeoff.touched]\ndescription = \"the diff's suite\"\ntype = \"string\"\n\
         default = \"make test-touched\"\n\
         \n[config.retries]\ndescription = \"how many\"\ntype = \"integer\"\ndefault = 3\n",
    )
    .expect("a well-formed declaration is a real manifest");

    let keys: Vec<&str> = manifest.config.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["retries", "takeoff.test", "takeoff.touched"],
        "the quoted name and the dotted table path are one name each, sorted"
    );
    let touched = &manifest.config[2];
    assert_eq!(touched.description, "the diff's suite");
    assert_eq!(touched.kind.as_deref(), Some("string"));
    assert_eq!(
        touched.default.as_ref().and_then(toml::Value::as_str),
        Some("make test-touched")
    );
    let test = &manifest.config[1];
    assert_eq!(test.kind, None, "the type is optional");
    assert_eq!(test.default, None, "and so is the default");
    assert!(
        test.admits(&toml::Value::Integer(1)),
        "an untyped declaration admits any value"
    );
    assert!(!touched.admits(&toml::Value::Integer(1)));
    assert!(touched.admits(&toml::Value::String("x".into())));
    assert_eq!(
        manifest.config[0]
            .default
            .as_ref()
            .and_then(toml::Value::as_integer),
        Some(3)
    );

    // The control: the table is optional, and a manifest without it declares
    // nothing.
    assert!(with_config("").expect("no table").config.is_empty());
}

/// Every malformed declaration refuses the manifest naming the setting and what
/// is wrong with it — one case per rule, each on its own, so a rule that never
/// fires cannot hide behind another that does.
#[test]
fn a_malformed_config_declaration_is_refused_naming_the_setting() {
    let cases: Vec<(&str, Defect)> = vec![
        (
            "[config.\"takeoff.test\"]\ntype = \"string\"\n",
            Defect::MissingManifestKey("config.takeoff.test.description".into()),
        ),
        (
            "[config.empty]\n",
            Defect::MissingManifestKey("config.empty.description".into()),
        ),
        (
            "[config.test]\ndescription = \"x\"\nrequired = true\n",
            Defect::UnknownSettingKey {
                key: "test".into(),
                field: "required".into(),
            },
        ),
        (
            "[config.test]\ndescription = \"x\"\ntype = \"path\"\n",
            Defect::SettingType {
                key: "test".into(),
                kind: "path".into(),
            },
        ),
        (
            "[config.test]\ndescription = \"x\"\ntype = \"string\"\ndefault = 3\n",
            Defect::SettingDefault {
                key: "test".into(),
                kind: "string".into(),
            },
        ),
        (
            "[config.test]\ndescription = 7\n",
            Defect::ManifestKeyType {
                key: "config.test.description".into(),
                want: "a string",
            },
        ),
        (
            "[config]\ntest = \"a bare value\"\n",
            Defect::ManifestKeyType {
                key: "config.test".into(),
                want: "a table",
            },
        ),
        (
            "[config.\"take off\"]\ndescription = \"x\"\n",
            Defect::SettingName("take off".into()),
        ),
        (
            "[config.takeoff]\ndescription = \"x\"\n\
             [config.\"takeoff.test\"]\ndescription = \"y\"\n",
            Defect::SettingOverlap {
                key: "takeoff".into(),
                other: "takeoff.test".into(),
            },
        ),
        (
            "[config.\"takeoff.test\"]\ndescription = \"x\"\n\
             [config.takeoff.test]\ndescription = \"y\"\n",
            Defect::SettingOverlap {
                key: "takeoff.test".into(),
                other: "takeoff.test".into(),
            },
        ),
    ];
    for (body, expected) in cases {
        let found = with_config(&format!("\n{body}"))
            .expect_err(&format!("a malformed declaration is a defect: {body}"));
        assert_eq!(found, vec![expected.clone()], "{body}");
        assert!(
            !found[0].to_string().is_empty(),
            "every defect prints as one line: {expected:?}"
        );
    }

    // The table itself as a bare value, which has to sit above `[pack]` to be
    // a top-level key at all.
    assert_eq!(
        pack::parse_manifest(
            "config = 3\n[pack]\nname = \"tiny\"\nversion = \"0.1.0\"\nschema = 3\n"
        ),
        Err(vec![Defect::ManifestKeyType {
            key: "config".into(),
            want: "a table",
        }])
    );

    // The overlap and the duplicate print as themselves, and both name the
    // settings they are about.
    let overlap = Defect::SettingOverlap {
        key: "takeoff".into(),
        other: "takeoff.test".into(),
    }
    .to_string();
    assert!(
        overlap.contains("[config.takeoff]") && overlap.contains("[config.takeoff.test]"),
        "{overlap}"
    );
    let twice = Defect::SettingOverlap {
        key: "takeoff.test".into(),
        other: "takeoff.test".into(),
    }
    .to_string();
    assert_eq!(twice, "[config.takeoff.test] is declared twice");
}

// ---- the guard classes a pack turns on ----------------------------------------

fn with_guards(line: &str) -> Result<pack::Manifest, Vec<Defect>> {
    pack::parse_manifest(&format!(
        "[pack]\nname = \"tiny\"\nversion = \"0.1.0\"\nschema = 3\n{line}"
    ))
}

/// The doctrine pack's declaration, read as the two classes it names, in the
/// order it names them; and a manifest without the key turns nothing on.
///
/// RED-PROOF: on the base `guard_classes` is an unknown `[pack]` key.
#[test]
fn a_pack_names_the_guard_classes_it_turns_on_in_its_pack_table() {
    let manifest = with_guards("guard_classes = [\"release-ref\", \"production-write\"]\n")
        .expect("a declaration of two of the four classes is a real manifest");
    assert_eq!(
        manifest.guards,
        vec![Class::ReleaseRef, Class::ProductionWrite]
    );
    assert!(
        with_guards("").expect("no key").guards.is_empty(),
        "a pack that names none turns none on"
    );
    assert!(
        with_guards("guard_classes = []\n")
            .expect("an empty list")
            .guards
            .is_empty(),
        "and an empty list is the same answer"
    );
}

/// A name outside the four is a defect naming it and the four, and a value
/// that is not a list of strings is one naming the key — one case each.
#[test]
fn a_guard_class_outside_the_four_or_a_value_not_a_list_of_strings_is_a_defect() {
    let not_a_list = Defect::ManifestKeyType {
        key: "pack.guard_classes".into(),
        want: "a list of strings",
    };
    for (line, expected) in [
        (
            "guard_classes = [\"shell\"]\n",
            Defect::UnknownGuardClass("shell".into()),
        ),
        ("guard_classes = \"release-ref\"\n", not_a_list.clone()),
        ("guard_classes = [\"record\", 1]\n", not_a_list.clone()),
    ] {
        assert_eq!(with_guards(line), Err(vec![expected]), "{line}");
    }
    let said = Defect::UnknownGuardClass("shell".into()).to_string();
    for name in [
        "`shell`",
        "shell-trap",
        "record",
        "release-ref",
        "production-write",
    ] {
        assert!(said.contains(name), "the line names {name}: {said}");
    }
}

/// THE SET `fleet guard` RUNS WITH NO CLASS. Core's two classes with nothing
/// installed, because the defaults carry no manifest to declare them; every
/// class an installed pack declares on top of them, once, in the order the
/// classes run whatever order the packs wrote them in.
#[test]
fn the_declared_set_is_core_s_two_and_every_class_an_installed_pack_names() {
    let defaults = Defaults::new("declared");
    let packs = defaults.beside().path("packs");
    std::fs::create_dir_all(&packs).expect("the packs dir is created");
    let layers = || resolve::layers(&packs, defaults.path()).expect("the layers order");

    assert_eq!(
        guard::declared(&layers()),
        Ok(vec![Class::ShellTrap, Class::Record]),
        "the defaults alone run core's two"
    );

    let write = |dir: &str, name: &str, line: &str| {
        let root = packs.join(dir);
        std::fs::create_dir_all(&root).expect("the pack dir is created");
        std::fs::write(
            root.join("pack.toml"),
            format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n{line}"),
        )
        .expect("the manifest is written");
    };
    write(
        "one",
        "one",
        "guard_classes = [\"production-write\", \"record\"]\n",
    );
    write("two", "two", "guard_classes = [\"release-ref\"]\n");
    assert_eq!(
        guard::declared(&layers()),
        Ok(guard::CLASSES.to_vec()),
        "the union, in class order"
    );

    // A layer whose manifest does not parse cannot say what it turns on, and
    // a set read without it would run fewer classes than the fleet declared.
    write("two", "two", "guard_classes = [\"shell\"]\n");
    let why = guard::declared(&layers()).expect_err("a defective declaration is not skipped");
    assert!(
        why.contains("`two`") && why.contains("`shell`"),
        "the line names the layer and the defect: {why}"
    );
}

mod lessons {
    use super::*;
    use fleet_core::pack::Manifest;

    /// gas-city G24 — the format is the reference's verbatim, plus the two
    /// slots it does not carry: nine names at a pack's top level and no tenth,
    /// either agent form, and `scope` optional on an always-on agent, because
    /// the reference's own fleet manifest omits it where its imported pack
    /// carries it. `workflows` and `adapters` are this format's own: what
    /// `fleet run` resolves a workflow's name through, and the store's opener
    /// an adapter's.
    ///
    /// The schema number is the one place this format now stands AHEAD of the
    /// reference's: G24 measures theirs at 2, and the runtime table is a table
    /// they do not carry, so a pack written for either parser would be read by
    /// the wrong one unless the number moves.
    #[test]
    fn a_pack_is_a_folder_with_nine_slots() {
        let mut names = vec![pack::MANIFEST];
        names.extend(pack::SLOTS);
        names.sort();
        assert_eq!(
            names,
            vec![
                "adapters",
                "agents",
                "assets",
                "doctor",
                "orders",
                "overlay",
                "pack.toml",
                "skills",
                "workflows",
            ]
        );
        assert_eq!(pack::SCHEMA, 3);
        assert_eq!(pack::AGENT_FORMS, ["agent.toml", "prompt.template.md"]);

        let without_scope = pack::parse_manifest(
            "[pack]\nname = \"c\"\nversion = \"\"\nschema = 3\n\n\
             [imports.gastown]\nsource = \"git@example:packs//gastown\"\nversion = \"^0.4\"\n\n\
             [[named_session]]\ntemplate = \"boot\"\nmode = \"once\"\n",
        )
        .expect("a manifest whose named_session omits scope is a real file");
        assert_eq!(without_scope.version, "");
        assert_eq!(without_scope.imports.len(), 1);
        assert_eq!(without_scope.named_sessions[0].scope, None);

        let with_scope: Manifest = pack::parse_manifest(
            "[pack]\nname = \"g\"\nversion = \"0.4.0\"\nschema = 3\n\n\
             [[named_session]]\ntemplate = \"polecat\"\nmode = \"loop\"\nscope = \"city\"\n",
        )
        .expect("and one that carries scope is a real file too");
        assert_eq!(with_scope.named_sessions[0].scope.as_deref(), Some("city"));
    }
}
