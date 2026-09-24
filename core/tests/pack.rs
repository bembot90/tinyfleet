//! The pack format. One arm per defect class the check knows, each built as a
//! real folder, plus the shipped pack read as it stands.

mod common;

use common::{bundled_tiny, bundled_ts, Defaults, Fixture};
use fleet_core::pack::{self, Defect};

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

#[test]
fn the_doctrine_pack_is_valid_under_the_same_check() {
    let report = pack::check(&bundled_tiny());
    assert!(
        report.is_valid(),
        "the shipped doctrine pack must pass the format check: {:?}",
        report.defects
    );
    let manifest = report
        .manifest
        .expect("the doctrine pack carries a manifest");
    assert_eq!(manifest.name, "tiny");
    assert_eq!(manifest.schema, pack::SCHEMA);
    assert_eq!(
        manifest.imports.iter().map(|i| &i.name).collect::<Vec<_>>(),
        vec!["ts"],
        "tiny imports the runtime pack and nothing else: the defaults are the \
         binary's and are imported by nobody"
    );
    assert_eq!(
        report.slots.iter().map(|s| s.name).collect::<Vec<_>>(),
        vec!["agents", "assets", "doctor", "overlay", "skills", "workflows"],
        "the doctrine lives in the six slots the format gives it: the role \
         definitions under agents, the documents under assets, the health checks \
         under doctor, under overlay the one file that says which guard classes \
         a provider's sessions run, the seat's rituals under skills, and the workflow files under workflows"
    );
}

/// The cost of shadowing a file whole: the copy has to carry the original. The
/// arm reads BOTH files off the tree — neither text is retyped here, because a
/// retyped copy would agree with itself — so a change to the default rules reds
/// this until the doctrine pack's copy is re-synced.
#[test]
fn the_doctrine_pack_s_rules_open_with_the_defaults_byte_for_byte() {
    let defaults = Defaults::new("rules");
    let under = std::fs::read(defaults.path().join("assets/rules.md"))
        .expect("the defaults publish the every-turn rules");
    let over = std::fs::read(bundled_tiny().join("assets/rules.md"))
        .expect("the doctrine pack shadows them");

    // The control on the two assertions below: an empty prefix is a prefix of
    // everything, and two identical files would make `starts_with` say nothing
    // about ordering either.
    assert!(
        !under.is_empty(),
        "the default rules file was read and is not empty"
    );
    assert!(
        over.len() > under.len(),
        "the shadow adds its own section: {} bytes over {}",
        over.len(),
        under.len()
    );
    assert!(
        over.starts_with(&under),
        "the doctrine pack's rules must open with the defaults', byte for byte"
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

    // The control: the table is optional, and every pack this fleet ships today
    // is on this side of it.
    let without = with_runtime("").expect("a pack that carries no workflows declares no runtime");
    assert_eq!(without.runtime, None);
    let shipped = bundled_tiny();
    let manifest = pack::check(&shipped).manifest.expect("a shipped manifest");
    assert_eq!(manifest.runtime, None, "{}", shipped.display());
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

    // The doctrine pack's reading: it pins no runtime, so the entry is the
    // shape and not an instance, and a pack that carries no workflows is not
    // red for it.
    let (code, said) = run(&matching, &bundled_tiny());
    assert_eq!(code, 0, "the doctrine pack pins no runtime: {said}");
    assert!(said.contains("runtime-version: nothing pinned"), "{said}");
}

/// fleet-reb — the bd pin's doctor check the defaults ship, run as a real
/// script against stub trackers, and its copy of the pin held to
/// `store::PINNED_BD`: the script cannot read the constant, so this arm is what
/// refuses a pin move that left the script behind.
///
/// The mismatch arm is a bd answering 1.2.2, the release before the pin, and
/// is what makes the holding arm worth anything; absence is measured with no
/// bd on PATH at all, which is also what proves the check needs no tool of its
/// own there. `FLEET_BD_BIN` is the seam prime reads the tracker through, and
/// names the binary over PATH.
#[test]
fn the_bd_doctor_check_reads_bd_version_against_the_pin() {
    let pin = fleet_core::store::PINNED_BD;
    let install = format!(
        "Install the pinned bd {pin} by beads' own instructions: \
         https://github.com/gastownhall/beads/blob/v{pin}/docs/getting-started/installation.md"
    );
    let defaults = Defaults::new("bd-doctor");
    let check = defaults.path().join("doctor/bd-version/run.sh");
    let script = std::fs::read_to_string(&check)
        .unwrap_or_else(|e| panic!("the defaults ship the check at {}: {e}", check.display()));
    assert_eq!(
        script
            .lines()
            .filter(|line| line.starts_with("PINNED="))
            .collect::<Vec<_>>(),
        vec![format!("PINNED={pin}").as_str()],
        "the check's one copy of the pin is store::PINNED_BD"
    );

    let fixture = Fixture::new("bd-doctor-stubs");
    fixture.dir("nothing");
    let fake = |label: &str, answer: &str| -> std::path::PathBuf {
        let dir = fixture.path(label);
        std::fs::create_dir_all(&dir).expect("the fake tracker's directory");
        let bin = dir.join("bd");
        std::fs::write(&bin, format!("#!/bin/sh\necho \"{answer}\"\n"))
            .expect("the fake tracker is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .expect("the fake tracker is executable");
        dir
    };
    let pinned = fake("pinned", &format!("bd version {pin} (Homebrew)"));
    let other = fake("other", "bd version 1.2.2");

    let run = |path: &std::path::Path, seam: Option<&std::path::Path>| -> (i32, String) {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg(&check).env("PATH", path).env_remove("FLEET_BD_BIN");
        if let Some(bin) = seam {
            cmd.env("FLEET_BD_BIN", bin);
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
    assert_eq!(code, 0, "the pinned bd on PATH: {said}");
    assert!(said.contains("bd-version: holds"), "{said}");

    let (code, said) = run(&other, None);
    assert_eq!(code, 1, "another bd on PATH: {said}");
    assert!(
        said.contains("answers: bd version 1.2.2")
            && said.contains(&format!("is not the pinned {pin}"))
            && said.contains("the verbs still run")
            && said.contains(&install),
        "the mismatch is named, pointed at where to install the pin: {said}"
    );

    let (code, said) = run(&fixture.path("nothing"), None);
    assert_eq!(code, 1, "no bd on PATH: {said}");
    assert!(
        said.contains("did not answer (exit 127)") && said.contains(&install),
        "{said}"
    );

    let (code, said) = run(&other, Some(&pinned.join("bd")));
    assert_eq!(code, 0, "the seam names the pinned bd over PATH's: {said}");
    assert!(said.contains("bd-version: holds"), "{said}");
}

/// fleet-2jt — the Claude Code pin's doctor check the defaults ship, run as a
/// real script against stub agents, and its copy of the pin held to
/// `supported::PINNED_CLAUDE_CODE`, as the bd check's is held to its own.
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

/// The ts pack, read as it stands: the one shipped pack whose manifest carries
/// the runtime table, and the doctor instance that measures it. The instance
/// resolves the binary from PATH first and from the installer's bin second —
/// the root deno's own installer names in `DENO_INSTALL`, which is the seam the
/// arms move instead of HOME — printing which, so the arms move PATH and the
/// root separately. The fifth arm puts a wrong version on PATH beside a right
/// one under the root, because a check that looked in the root first would
/// read green there and the order would be unproved.
#[test]
fn the_ts_pack_pins_deno_and_its_doctor_check_resolves_it_from_path_then_the_installers_directory()
{
    let ts = bundled_ts();
    let report = pack::check(&ts);
    assert!(
        report.is_valid(),
        "the ts pack checks clean: {:?}",
        report.defects
    );
    let manifest = report.manifest.expect("the ts pack ships a manifest");
    assert_eq!(manifest.name, "ts");
    assert_eq!(
        manifest.imports,
        vec![],
        "a middle layer declares no imports"
    );
    let runtime = manifest.runtime.expect("the ts pack pins a runtime");
    assert_eq!(runtime.name, "deno");
    assert_eq!(runtime.version, "2.9.7");
    assert_eq!(runtime.bundle, "deno bundle -o {bundle} {entry}");
    assert_eq!(
        runtime.run,
        "deno run --allow-run={fleet} --allow-read={run_dir} --allow-write={run_dir} \
         --allow-env=FLEET_DIR,FLEET_RUN_ID,FLEET_STREAM,FLEET_STREAM_SEQ,FLEET_RUN_DIR,FLEET_BIN,FLEET_PROJECT {bundle}"
    );

    let check = ts.join("doctor/deno-version/run.sh");
    assert!(
        check.is_file(),
        "the pack ships the instance: {}",
        check.display()
    );

    let fixture = Fixture::new("deno-doctor");
    fixture.dir("nothing");
    fixture.dir("empty-install");
    let empty_install = fixture.path("empty-install");

    let fake = |label: &str, answer: &str| -> std::path::PathBuf {
        let dir = fixture.path(label);
        std::fs::create_dir_all(&dir).expect("the fake runtime's directory");
        let bin = dir.join("deno");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"deno {answer} (stable, release, aarch64-apple-darwin)\"\necho \"v8 15.0.245.2-rusty\"\necho \"typescript 6.0.3\"\n"),
        )
        .expect("the fake runtime is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("the fake runtime is executable");
        }
        dir
    };
    let pinned = fake("pinned", "2.9.7");
    let other = fake("other", "2.9.6");
    fake("install/bin", "2.9.7");
    let install = fixture.path("install");

    let run = |path: &std::path::Path, install: &std::path::Path| -> (i32, String) {
        let out = std::process::Command::new("/bin/sh")
            .arg(&check)
            .env("PATH", path)
            .env("DENO_INSTALL", install)
            .env("FLEET_PACK_DIR", &ts)
            .output()
            .expect("the check runs");
        (
            out.status
                .code()
                .expect("the check exits rather than signals"),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };

    let (code, said) = run(&pinned, &empty_install);
    assert_eq!(code, 0, "the pinned version on PATH: {said}");
    assert!(said.contains("resolved from PATH"), "{said}");
    assert!(said.contains("deno-version: holds"), "{said}");

    let (code, said) = run(&fixture.path("nothing"), &install);
    assert_eq!(code, 0, "off PATH, in the installer's bin: {said}");
    assert!(said.contains("resolved from the installer's bin"), "{said}");
    assert!(said.contains("deno-version: holds"), "{said}");

    let (code, said) = run(&other, &empty_install);
    assert_eq!(code, 1, "another version on PATH: {said}");
    assert!(said.contains("is not the pinned 2.9.7"), "{said}");

    // Absence: neither place holds it, and the reading says so in the word a
    // reader greps for.
    let (code, said) = run(&fixture.path("nothing"), &empty_install);
    assert_eq!(code, 1, "no runtime anywhere: {said}");
    assert!(said.contains("deno is absent"), "{said}");

    // The order: PATH wins over the installer's bin, so a wrong version on PATH
    // is reported even when the right one sits under the root.
    let (code, said) = run(&other, &install);
    assert_eq!(
        code, 1,
        "PATH is read before the installer's directory: {said}"
    );
    assert!(said.contains("resolved from PATH"), "{said}");
    assert!(said.contains("is not the pinned 2.9.7"), "{said}");
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

/// The doctrine pack declares the two commands its takeoff reads, each with a
/// one-line description a person reads before setting it.
#[test]
fn the_doctrine_pack_declares_the_two_takeoff_settings() {
    let manifest = pack::check(&bundled_tiny())
        .manifest
        .expect("the doctrine pack carries a manifest");
    let keys: Vec<&str> = manifest.config.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, vec!["takeoff.test", "takeoff.touched"]);
    for setting in &manifest.config {
        assert!(
            !setting.description.trim().is_empty() && !setting.description.contains('\n'),
            "{} carries a one-line description: {:?}",
            setting.key,
            setting.description
        );
        assert_eq!(
            setting.kind.as_deref(),
            Some("string"),
            "{} is a command",
            setting.key
        );
    }
}

mod lessons {
    use super::*;
    use fleet_core::pack::Manifest;

    /// gas-city G24 — the format is the reference's verbatim, plus the one slot
    /// it does not carry: nine names at a pack's top level and no tenth, either
    /// agent form, and `scope` optional on an always-on agent, because the
    /// reference's own fleet manifest omits it where its imported pack carries
    /// it. `workflows` is this format's own, and is what `fleet run` resolves a
    /// name through.
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
