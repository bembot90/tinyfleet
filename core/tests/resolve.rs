//! The layers and the shadow surface, over real folders.

mod common;

use common::{fixture_tiny, fixture_ts, Defaults, Fixture};
use fleet_core::resolve::{Layer, Refusal};
use fleet_core::{registry, resolve};

fn layers(packs: &[(&str, &Fixture)]) -> Vec<Layer> {
    packs
        .iter()
        .map(|(name, f)| Layer::new(*name, f.root.clone()))
        .collect()
}

#[test]
fn a_higher_layer_wins_and_the_lower_one_is_shadowed() {
    let top = Fixture::new("top");
    top.manifest("top").file("assets/brief.md", "the top\n");
    let bottom = Fixture::new("bottom");
    bottom
        .manifest("bottom")
        .file("assets/brief.md", "the bottom\n")
        .file("assets/only-below.md", "below\n");

    let resolution = resolve::resolve(&layers(&[("top", &top), ("bottom", &bottom)]))
        .expect("a bottom layer publishing no registry declares no surface to refuse");

    assert_eq!(
        resolution.files.get("assets/brief.md").map(String::as_str),
        Some("top"),
        "the path resolves to the highest layer that carries it"
    );
    assert_eq!(
        resolution
            .files
            .get("assets/only-below.md")
            .map(String::as_str),
        Some("bottom")
    );
    assert_eq!(resolution.shadowed.len(), 1);
    assert_eq!(resolution.shadowed[0].path, "assets/brief.md");
    assert_eq!(resolution.shadowed[0].over, "top");
    assert_eq!(resolution.shadowed[0].under, "bottom");
}

#[test]
fn a_duplicate_agent_name_refuses_naming_both_packs() {
    let top = Fixture::new("agent-top");
    top.manifest("top")
        .file("agents/architect/agent.toml", "name = \"architect\"\n");
    let bottom = Fixture::new("agent-bottom");
    bottom
        .manifest("bottom")
        .file("agents/architect/agent.toml", "name = \"architect\"\n");

    let refusals = resolve::resolve(&layers(&[("top", &top), ("bottom", &bottom)]))
        .expect_err("a collision is refused, never resolved by precedence");
    assert_eq!(
        refusals,
        vec![Refusal::AgentNameCollision {
            agent: "architect".into(),
            upper: "top".into(),
            lower: "bottom".into(),
        }]
    );
    let line = refusals[0].to_string();
    assert!(line.contains("top") && line.contains("bottom"), "{line}");
}

#[test]
fn agent_names_unique_across_layers_resolve() {
    let top = Fixture::new("uniq-top");
    top.manifest("top")
        .file("agents/architect/agent.toml", "name = \"architect\"\n");
    let bottom = Fixture::new("uniq-bottom");
    bottom
        .manifest("bottom")
        .file("agents/builder/agent.toml", "name = \"builder\"\n");

    let resolution = resolve::resolve(&layers(&[("top", &top), ("bottom", &bottom)]))
        .expect("two different names are not a collision");
    assert_eq!(
        resolution.agents.get("architect").map(String::as_str),
        Some("top")
    );
    assert_eq!(
        resolution.agents.get("builder").map(String::as_str),
        Some("bottom")
    );
}

#[test]
fn a_listed_import_declaring_its_own_import_is_refused_by_name() {
    let top = Fixture::new("trans-top");
    top.manifest("top");
    let middle = Fixture::new("trans-middle");
    middle.file(
        "pack.toml",
        "[pack]\nname = \"middle\"\nversion = \"1\"\nschema = 3\n\n\
         [imports.deeper]\nsource = \"git@example:deeper\"\nversion = \"^1\"\n",
    );

    let refusals = resolve::resolve(&layers(&[("top", &top), ("middle", &middle)]))
        .expect_err("imports are one level deep at this slice");
    assert_eq!(
        refusals,
        vec![Refusal::TransitiveImport {
            layer: "middle".into(),
            import: "deeper".into(),
        }]
    );
}

#[test]
fn the_top_layers_own_imports_are_the_callers_and_are_not_refused() {
    let top = Fixture::new("own-top");
    top.file(
        "pack.toml",
        "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n\n\
         [imports.middle]\nsource = \"git@example:middle\"\nversion = \"^1\"\n",
    );
    let middle = Fixture::new("own-middle");
    middle.manifest("middle");

    resolve::resolve(&layers(&[("top", &top), ("middle", &middle)]))
        .expect("the caller assembled the list from exactly these imports");
}

#[test]
fn a_shadow_outside_the_registry_refuses_naming_the_path_and_the_pack() {
    let top = Fixture::new("shadow-top");
    top.manifest("top").file("assets/brief.md", "mine\n");
    let base = Fixture::new("shadow-base");
    base.manifest("base")
        .file("assets/brief.md", "the base's\n")
        .file(
            "assets/shadow-registry.toml",
            "schema = 1\n\n[[shadow]]\npath = \"assets/note.md\"\npurpose = \"a note template\"\n",
        )
        .file("assets/note.md", "a note\n");

    let refusals = resolve::resolve(&layers(&[("top", &top), ("base", &base)]))
        .expect_err("a file outside the registry is the bottom layer's own implementation");
    assert_eq!(
        refusals,
        vec![Refusal::UnlistedShadow {
            path: "assets/brief.md".into(),
            pack: "base".into(),
        }]
    );
    let line = refusals[0].to_string();
    assert!(
        line.contains("assets/brief.md") && line.contains("base"),
        "{line}"
    );
}

#[test]
fn the_same_file_listed_in_the_registry_resolves() {
    let top = Fixture::new("listed-top");
    top.manifest("top").file("assets/brief.md", "mine\n");
    let base = Fixture::new("listed-base");
    base.manifest("base")
        .file("assets/brief.md", "the base's\n")
        .file(
        "assets/shadow-registry.toml",
        "schema = 1\n\n[[shadow]]\npath = \"assets/brief.md\"\npurpose = \"the brief template\"\n",
    );

    let resolution = resolve::resolve(&layers(&[("top", &top), ("base", &base)]))
        .expect("a listed path is exactly what a pack on top may replace");
    assert_eq!(
        resolution.files.get("assets/brief.md").map(String::as_str),
        Some("top")
    );
}

// The control for the two arms above: the registry is only consulted for a path
// the bottom layer actually holds, so a higher-layer file that shadows nothing
// needs no row.
#[test]
fn a_higher_layer_file_the_bottom_does_not_hold_needs_no_row() {
    let top = Fixture::new("new-top");
    top.manifest("top").file("assets/extra.md", "mine\n");
    let base = Fixture::new("new-base");
    base.manifest("base")
        .file("assets/shadow-registry.toml", "schema = 1\n");

    resolve::resolve(&layers(&[("top", &top), ("base", &base)]))
        .expect("adding a file is not shadowing one");
}

// Every pack that publishes a surface carries the registry at this one path, so
// the rule would refuse the normal case if the file were read as content.
#[test]
fn a_higher_layers_own_registry_is_not_a_shadow_of_the_bottoms() {
    let top = Fixture::new("reg-top");
    top.manifest("top")
        .file("assets/shadow-registry.toml", "schema = 1\n");
    let base = Fixture::new("reg-base");
    base.manifest("base")
        .file("assets/shadow-registry.toml", "schema = 1\n");

    resolve::resolve(&layers(&[("top", &top), ("base", &base)]))
        .expect("a pack declaring its own surface is not replacing the one below");
}

#[test]
fn the_registry_lists_only_paths_the_bottom_layer_holds() {
    let held = Defaults::new("bottom");
    let bottom = held.path();
    let registry = registry::read(bottom)
        .expect("the binary's defaults publish the registry")
        .expect("and it parses");
    assert!(
        registry::missing(bottom, &registry).is_empty(),
        "every listed path exists in the layer that publishes it"
    );

    // The control: a fixture whose registry names a file it does not hold is
    // the case the rule exists for.
    let fixture = Fixture::new("existence");
    fixture.manifest("base").file(
        "assets/shadow-registry.toml",
        "schema = 1\n\n[[shadow]]\npath = \"assets/brief.md\"\npurpose = \"the brief\"\n",
    );
    let planted = registry::read(&fixture.root)
        .expect("the fixture publishes a registry")
        .expect("and it parses");
    assert_eq!(
        registry::missing(&fixture.root, &planted),
        vec!["assets/brief.md".to_string()]
    );
}

#[test]
fn a_registry_whose_schema_is_not_one_is_refused() {
    assert_eq!(
        registry::parse("schema = 2\n"),
        Err(registry::RegistryError::Schema(2))
    );
}

// ---- the layers a verb resolves for itself ----------------------------------

/// One pack inside a packs directory, declaring the imports it names.
fn pack_in(dir: &Fixture, name: &str, imports: &[&str]) {
    let mut manifest = format!("[pack]\nname = \"{name}\"\nversion = \"1\"\nschema = 3\n");
    for import in imports {
        manifest.push_str(&format!(
            "\n[imports.{import}]\nsource = \"git@example:packs//{import}\"\nversion = \"^1\"\n"
        ));
    }
    dir.file(&format!("{name}/pack.toml"), &manifest);
}

fn names_of(layers: Vec<Layer>) -> Vec<String> {
    layers.into_iter().map(|layer| layer.name).collect()
}

#[test]
fn installed_reads_every_pack_by_its_manifest_and_skips_what_is_not_one() {
    let dir = Fixture::new("installed");
    pack_in(&dir, "alpha", &[]);
    pack_in(&dir, "gamma", &[]);
    // A dotted entry is scratch from an add in flight; a directory with no
    // manifest is not a pack at all.
    dir.file(".scratch-1234/pack.toml", "[pack]\nname = \"ghost\"\n");
    dir.dir("not-a-pack");

    let mut names = names_of(resolve::installed(&dir.root));
    names.sort();
    assert_eq!(names, vec!["alpha".to_string(), "gamma".to_string()]);
}

#[test]
fn ordered_puts_every_pack_before_what_it_imports() {
    let dir = Fixture::new("ordered");
    // `beta` imports `alpha`, so beta precedes it — the opposite of the order
    // the tie-break by name would give.
    pack_in(&dir, "alpha", &[]);
    pack_in(&dir, "beta", &["alpha"]);

    assert_eq!(
        names_of(resolve::ordered(resolve::installed(&dir.root)).expect("no cycle")),
        vec!["beta".to_string(), "alpha".to_string()]
    );

    // The control: with the import gone, the tie-break by name decides, so the
    // order above is the import edge and not the sort.
    let plain = Fixture::new("ordered-control");
    pack_in(&plain, "alpha", &[]);
    pack_in(&plain, "beta", &[]);
    assert_eq!(
        names_of(resolve::ordered(resolve::installed(&plain.root)).expect("no cycle")),
        vec!["alpha".to_string(), "beta".to_string()]
    );
}

/// The bottom layer is the caller's to append and never `ordered`'s to sort —
/// and a pack that takes its NAME is refused by `resolve`, because a resolution
/// names the layer carrying each path by name and two layers under one name
/// resolve a default to the wrong root.
#[test]
fn a_pack_calling_itself_defaults_orders_as_a_pack_and_the_resolution_refuses_it() {
    let dir = Fixture::new("ordered-defaults");
    pack_in(&dir, fleet_core::defaults::LAYER, &[]);
    pack_in(&dir, "alpha", &[]);

    let mut ordered = resolve::ordered(resolve::installed(&dir.root)).expect("no cycle");
    assert_eq!(
        names_of(ordered.clone()),
        vec!["alpha".to_string(), fleet_core::defaults::LAYER.to_string()],
        "the tie-break by name decides, and `ordered` holds no special name"
    );
    assert!(
        ordered.iter().all(|layer| !layer.defaults),
        "nothing the packs directory holds is the defaults layer"
    );

    let held = Defaults::new("bottom");
    ordered.push(Layer::defaults(held.path()));
    assert!(
        ordered.last().expect("the list is not empty").defaults,
        "the bottom is the one the caller appended"
    );

    let refusals =
        resolve::resolve(&ordered).expect_err("a pack cannot take the bottom layer's name");
    assert_eq!(
        refusals,
        vec![Refusal::DefaultsName {
            root: dir.path(fleet_core::defaults::LAYER).display().to_string(),
        }]
    );
    let line = refusals[0].to_string();
    assert!(line.contains("rename it or take it out"), "{line}");

    // The control: with that pack renamed, the same layering resolves, so what
    // refused above is the name and not the fixture.
    let clean = Fixture::new("ordered-defaults-control");
    pack_in(&clean, "omega", &[]);
    pack_in(&clean, "alpha", &[]);
    let mut ordered = resolve::ordered(resolve::installed(&clean.root)).expect("no cycle");
    let held = Defaults::new("bottom");
    ordered.push(Layer::defaults(held.path()));
    resolve::resolve(&ordered).expect("two ordinary packs over the defaults resolve");
}

#[test]
fn packs_that_import_each_other_refuse_and_name_both() {
    let dir = Fixture::new("cycle");
    pack_in(&dir, "alpha", &["beta"]);
    pack_in(&dir, "beta", &["alpha"]);

    let refusals = resolve::ordered(resolve::installed(&dir.root))
        .expect_err("no order puts each pack above the one it imports");
    assert_eq!(
        refusals,
        vec![Refusal::ImportCycle {
            packs: vec!["alpha".to_string(), "beta".to_string()]
        }]
    );
    assert!(refusals[0].to_string().contains("alpha, beta"));
}

#[test]
fn a_slot_path_resolves_to_the_file_the_highest_layer_carries() {
    let top = Fixture::new("slot-top");
    top.manifest("top").file("assets/brief.md", "the top\n");
    let bottom = Fixture::new("slot-bottom");
    bottom
        .manifest("bottom")
        .file("assets/brief.md", "the bottom\n")
        .file("assets/rules.md", "the rules\n");

    let layers = layers(&[("top", &top), ("bottom", &bottom)]);
    let resolution = resolve::resolve(&layers).expect("two clean layers resolve");

    assert_eq!(
        resolve::slot_path(&resolution, &layers, "assets/brief.md"),
        Some(top.path("assets/brief.md")),
        "the shadowing layer's file is the one answered with"
    );
    assert_eq!(
        resolve::slot_path(&resolution, &layers, "assets/rules.md"),
        Some(bottom.path("assets/rules.md")),
        "and a path only the lower layer holds resolves to it"
    );
    assert_eq!(
        resolve::slot_path(&resolution, &layers, "assets/nobody.md"),
        None,
        "a path no layer carries has no answer"
    );
}

/// Two packs a file browser has opened both carry a `.DS_Store` in the same
/// slot. Neither file is part of either pack, so neither resolves and neither
/// shadows the other.
#[test]
fn os_litter_in_two_layers_neither_resolves_nor_shadows() {
    let top = Fixture::new("litter-top");
    top.manifest("top").file("assets/brief.md", "the top\n");
    let bottom = Fixture::new("litter-bottom");
    bottom
        .manifest("bottom")
        .file("assets/rules.md", "the rules\n");
    for fixture in [&top, &bottom] {
        std::fs::write(
            fixture.path("assets/.DS_Store"),
            b"\x00\x00\x00\x01Bud1\xff",
        )
        .expect("the litter is written");
    }

    let layers = layers(&[("top", &top), ("bottom", &bottom)]);
    let resolution = resolve::resolve(&layers).expect("two littered layers resolve");
    assert_eq!(
        resolution.files.keys().collect::<Vec<_>>(),
        vec!["assets/brief.md", "assets/rules.md"],
        "the resolution carries each pack's own files and nothing else"
    );
    assert!(
        resolution.shadowed.is_empty(),
        "and the two copies of the litter are no shadow: {:?}",
        resolution.shadowed
    );
}

/// The fixture shaped as the doctrine pack over the binary's defaults,
/// resolved as two layers: nothing refuses, the pack carries the every-turn
/// rules it shadows, and the defaults still carry the brief template nobody
/// has shadowed. The bottom layer is the binary's own set and not a fixture,
/// so a registry entry withdrawn from the defaults for any of the four paths
/// the pack shadows reds this rather than passing against a copy. The runtime
/// pack beneath it, and the layering a fleet installs the two as, is the arm
/// after this one.
#[test]
fn the_doctrine_shaped_pack_over_the_defaults_alone_resolves() {
    let tiny = fixture_tiny();
    let held = Defaults::new("bottom");
    let bottom = held.path();
    let layers = vec![Layer::new("tiny", tiny.clone()), Layer::defaults(bottom)];

    let resolution = resolve::resolve(&layers)
        .expect("the layering resolves: the registry permits what tiny shadows");

    assert_eq!(
        resolution.files.get("assets/rules.md").map(String::as_str),
        Some("tiny"),
        "the pack carries the every-turn rules"
    );
    assert_eq!(
        resolution.files.get("assets/brief.md").map(String::as_str),
        Some(fleet_core::defaults::LAYER),
        "and the defaults still carry the brief template, which nothing shadows"
    );
    assert_eq!(
        resolve::slot_path(&resolution, &layers, "assets/rules.md"),
        Some(tiny.join("assets/rules.md"))
    );
    assert_eq!(
        resolve::slot_path(&resolution, &layers, "assets/brief.md"),
        Some(bottom.join("assets/brief.md"))
    );

    // The control on "refusing nothing": a resolution that saw no shadow at all
    // would satisfy every line above and prove none of them.
    assert_eq!(
        resolution
            .shadowed
            .iter()
            .map(|s| s.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "assets/rules.md",
            "doctor/guards-installed/doctor.toml",
            "doctor/guards-installed/run.sh",
            "overlay/per-provider/claude/hooks.json",
        ],
        "every shadowed path is one the pack means to shadow, and no other: the \
         rules file, and the three that wire and report on the two guard classes \
         this pack adds"
    );
}

/// The two fixture packs, as `installed` reads them off a packs directory and
/// `ordered` lays them, over the binary's own defaults: tiny over ts, because
/// tiny imports ts and ts imports nothing, and the defaults appended under both
/// by the caller. tiny's import resolves to the ts folder beside it, the
/// layering resolves with nothing refused, the doctor instance ts adds is
/// carried by ts alone, and the shadow list is the doctrine-shaped pack's
/// unchanged — so the middle layer adds a file and shadows none.
#[test]
fn a_pack_and_its_import_order_tiny_over_ts_over_the_defaults_and_resolve() {
    let packs_dir = fixture_tiny()
        .parent()
        .expect("the fixtures sit in one packs directory")
        .to_path_buf();
    let installed = resolve::installed(&packs_dir);
    let mut ordered = resolve::ordered(installed).expect("the two packs order without a cycle");
    let held = Defaults::new("bottom");
    ordered.push(Layer::defaults(held.path()));
    let names: Vec<&str> = ordered.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["tiny", "ts", fleet_core::defaults::LAYER]);

    let tiny = std::fs::read_to_string(fixture_tiny().join(fleet_core::pack::MANIFEST))
        .expect("tiny's manifest is readable");
    let manifest = fleet_core::pack::parse_manifest(&tiny).expect("tiny's manifest parses");
    let import = manifest
        .imports
        .iter()
        .find(|i| i.name == "ts")
        .expect("tiny imports ts");
    assert_eq!(
        fixture_tiny()
            .join(&import.source)
            .canonicalize()
            .expect("the import's source is a folder beside tiny"),
        fixture_ts()
            .canonicalize()
            .expect("the ts pack is a folder"),
        "the import's source names the ts folder beside it"
    );
    let ts = std::fs::read_to_string(fixture_ts().join(fleet_core::pack::MANIFEST))
        .expect("ts's manifest is readable");
    let ts = fleet_core::pack::parse_manifest(&ts).expect("ts's manifest parses");
    assert_eq!(
        import.version, ts.version,
        "the import pins the version ts declares"
    );

    let resolution = resolve::resolve(&ordered).expect("the three-layer layering resolves");
    assert_eq!(
        resolution
            .files
            .get("doctor/deno-version/run.sh")
            .map(String::as_str),
        Some("ts"),
        "the doctor instance is ts's own"
    );
    assert_eq!(
        resolution.files.get("assets/rules.md").map(String::as_str),
        Some("tiny")
    );
    assert_eq!(
        resolution.files.get("assets/brief.md").map(String::as_str),
        Some(fleet_core::defaults::LAYER)
    );
    assert_eq!(
        resolution
            .shadowed
            .iter()
            .map(|s| s.path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "assets/rules.md",
            "doctor/guards-installed/doctor.toml",
            "doctor/guards-installed/run.sh",
            "overlay/per-provider/claude/hooks.json",
        ],
        "the middle layer shadows nothing: the list is the doctrine-shaped pack's alone"
    );
}
