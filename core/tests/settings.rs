//! A pack's settings, over real pack folders: what `[packs.<name>]` in
//! `fleet.toml` resolves to against what each installed pack declares, and
//! every way the table is refused.

mod common;

use common::Fixture;
use fleet_core::resolve::Layer;
use fleet_core::settings::{self, Refusal};

/// A pack named `name` declaring the given `[config.<key>]` tables.
fn a_pack(name: &str, declarations: &str) -> Fixture {
    let fixture = Fixture::new(&format!("settings-{name}"));
    fixture.file(
        "pack.toml",
        &format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n{declarations}"),
    );
    fixture
}

const TINY: &str = "\n[config.\"takeoff.test\"]\ndescription = \"the suite\"\ntype = \"string\"\n\
     \n[config.takeoff.touched]\ndescription = \"the diff's suite\"\ntype = \"string\"\n\
     default = \"make test-touched\"\n\
     \n[config.width]\ndescription = \"how many at once\"\ntype = \"integer\"\ndefault = 1\n\
     \n[config.note]\ndescription = \"untyped, and with no default\"\n";

fn policy(text: &str) -> toml::Table {
    text.parse().expect("the fixture policy parses")
}

/// The two installed packs every arm reads, over a defaults layer that carries
/// no manifest at all — which is what the binary's own bottom layer is.
struct Installed {
    tiny: Fixture,
    ts: Fixture,
    defaults: Fixture,
}

impl Installed {
    fn new() -> Installed {
        Installed {
            tiny: a_pack("tiny", TINY),
            ts: a_pack("ts", ""),
            defaults: Fixture::new("settings-defaults"),
        }
    }

    fn layers(&self) -> Vec<Layer> {
        vec![
            Layer::new("tiny", self.tiny.root.clone()),
            Layer::new("ts", self.ts.root.clone()),
            Layer::defaults(self.defaults.root.clone()),
        ]
    }
}

#[test]
fn the_fleets_values_lie_over_the_declared_defaults_in_either_spelling() {
    let installed = Installed::new();
    let resolved = settings::resolve(
        &policy(
            "[packs.tiny]\ntakeoff.test = \"cargo nextest run --workspace\"\n\
             \"takeoff.touched\" = \"cargo nextest run -p fleet-core\"\n",
        ),
        &installed.layers(),
    )
    .expect("every key set is declared");

    let tiny = &resolved["tiny"];
    assert_eq!(
        tiny["takeoff.test"].as_str(),
        Some("cargo nextest run --workspace"),
        "a dotted key reads as the setting it spells"
    );
    assert_eq!(
        tiny["takeoff.touched"].as_str(),
        Some("cargo nextest run -p fleet-core"),
        "so does a quoted one, and the value lies over the default"
    );
    assert_eq!(
        tiny["width"].as_integer(),
        Some(1),
        "a key the fleet does not set reads its declared default"
    );
    assert!(
        !tiny.contains_key("note"),
        "a key with no value and no default is absent: {tiny:?}"
    );
    assert!(
        resolved["ts"].is_empty(),
        "a pack declaring nothing resolves to nothing"
    );

    // The control: no [packs] table at all is every default and no refusal.
    let untouched = settings::resolve(&policy("[core]\nreviewer = \"r\"\n"), &installed.layers())
        .expect("a fleet that sets nothing is not refused");
    assert_eq!(
        untouched["tiny"]["takeoff.touched"].as_str(),
        Some("make test-touched")
    );
    assert!(!untouched["tiny"].contains_key("takeoff.test"));
}

#[test]
fn an_undeclared_key_is_refused_naming_the_key_the_pack_and_what_it_declares() {
    let installed = Installed::new();
    let refusals = settings::resolve(
        &policy("[packs.tiny]\ntakeoff.tset = \"make\"\nwidht = 2\n"),
        &installed.layers(),
    )
    .expect_err("a key the pack does not declare is refused");

    let declared: Vec<String> = ["note", "takeoff.test", "takeoff.touched", "width"]
        .iter()
        .map(|k| k.to_string())
        .collect();
    assert_eq!(
        refusals,
        vec![
            Refusal::Undeclared {
                pack: "tiny".into(),
                key: "takeoff.tset".into(),
                declared: declared.clone(),
            },
            Refusal::Undeclared {
                pack: "tiny".into(),
                key: "widht".into(),
                declared,
            },
        ],
        "every undeclared key is named, not only the first"
    );
    let said = refusals[0].to_string();
    for needle in ["[packs.tiny]", "`takeoff.tset`", "`tiny`", "takeoff.test"] {
        assert!(
            said.contains(needle),
            "the refusal carries {needle}: {said}"
        );
    }

    // A key under a pack that declares nothing is refused the same way, and
    // says the pack declares none.
    let refusals = settings::resolve(
        &policy("[packs.ts]\nversion = \"2\"\n"),
        &installed.layers(),
    )
    .expect_err("ts declares nothing");
    assert!(
        refusals[0].to_string().ends_with("declares none"),
        "{}",
        refusals[0]
    );
}

#[test]
fn a_section_for_a_pack_not_installed_is_refused_naming_it_and_the_installed_ones() {
    let installed = Installed::new();
    let refusals = settings::resolve(
        &policy("[packs.gone]\ntakeoff.test = \"make\"\n"),
        &installed.layers(),
    )
    .expect_err("a section for a pack that is not installed is refused");
    assert_eq!(
        refusals,
        vec![Refusal::NotInstalled {
            pack: "gone".into(),
            installed: vec!["tiny".into(), "ts".into()],
        }],
        "the defaults layer is not a pack and is not listed as one"
    );
    let said = refusals[0].to_string();
    assert!(
        said.contains("[packs.gone]") && said.contains("tiny, ts"),
        "{said}"
    );
}

#[test]
fn a_value_of_the_wrong_type_is_refused_naming_both_types() {
    let installed = Installed::new();
    let refusals = settings::resolve(
        &policy("[packs.tiny]\nwidth = \"two\"\nnote = [1, 2]\n"),
        &installed.layers(),
    )
    .expect_err("a string where an integer is declared");
    assert_eq!(
        refusals,
        vec![Refusal::WrongType {
            pack: "tiny".into(),
            key: "width".into(),
            kind: "integer".into(),
            found: "string".into(),
        }],
        "the untyped key takes the array; only the typed one is refused"
    );
    assert!(
        refusals[0].to_string().contains("`width` to a string"),
        "{}",
        refusals[0]
    );
}

#[test]
fn a_packs_table_or_a_section_that_is_not_a_table_is_refused() {
    let installed = Installed::new();
    assert_eq!(
        settings::resolve(&policy("packs = 3\n"), &installed.layers()),
        Err(vec![Refusal::NotATable { at: "packs".into() }])
    );
    assert_eq!(
        settings::resolve(&policy("[packs]\ntiny = \"x\"\n"), &installed.layers()),
        Err(vec![Refusal::NotATable {
            at: "packs.tiny".into()
        }])
    );
}

/// A pack whose manifest does not parse is named for that, and its section is
/// not also called uninstalled: the pack is there.
#[test]
fn a_pack_whose_manifest_does_not_parse_is_named_and_its_section_is_not_called_absent() {
    let installed = Installed::new();
    installed.tiny.file(
        "pack.toml",
        "[pack]\nname = \"tiny\"\nversion = \"0.1.0\"\nschema = 3\n\n[config.x]\ntype = \"string\"\n",
    );
    let refusals = settings::resolve(&policy("[packs.tiny]\nx = \"y\"\n"), &installed.layers())
        .expect_err("what tiny declares cannot be read");
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    let Refusal::Manifest { pack, defect } = &refusals[0] else {
        panic!("{refusals:?}");
    };
    assert_eq!(pack, "tiny");
    assert!(defect.contains("config.x.description"), "{defect}");
}
