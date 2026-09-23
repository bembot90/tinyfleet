//! `fleet pack add`, against local git repositories the tests build.
//!
//! Every source here is a folder this test created and committed into, so the
//! suite reaches no network and installs into a temporary machine directory it
//! owns.

mod common;

use common::{a_pack, manifest, repo, source_of, Machine, WHEN};
use fleet_core::add::{self, Refusal, Source, Version};
use fleet_core::lock;
use fleet_core::resolve;

fn lines(refusals: &[Refusal]) -> String {
    refusals
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------- AC1: install

#[test]
fn a_pack_installs_at_a_tag_and_the_lock_pins_the_commit() {
    let (source_repo, sha) = a_pack("tag-src", "neighborly");
    let machine = Machine::new("tag-machine");
    let source = source_of(&source_repo);

    let installed = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source,
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the pack installs: {}", lines(&r)));

    assert_eq!(installed.name, "neighborly");
    assert_eq!(installed.root, machine.packs().join("neighborly"));
    assert!(machine
        .packs()
        .join("neighborly/skills/greet/SKILL.md")
        .is_file());
    assert!(
        !machine.packs().join("neighborly/.git").exists(),
        "the clone's own .git is not part of the pack"
    );
    assert_eq!(
        machine.installed(),
        vec!["neighborly"],
        "the scratch directory of the fetch is gone"
    );

    assert_eq!(
        lock::read(&machine.lock()).expect("the lock parses"),
        vec![lock::Entry {
            source,
            name: Some("neighborly".into()),
            version: "v1".into(),
            commit: sha,
            fetched: WHEN.into(),
            tree: None,
        }]
    );
}

#[test]
fn a_pack_installs_at_a_sha_and_pins_the_version_as_it_was_given() {
    let (source_repo, sha) = a_pack("sha-src", "neighborly");
    let machine = Machine::new("sha-machine");
    let asked = format!("sha:{sha}");

    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        &asked,
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the pack installs: {}", lines(&r)));

    let pinned = lock::read(&machine.lock()).expect("the lock parses");
    assert_eq!(pinned.len(), 1);
    assert_eq!(
        pinned[0].version, asked,
        "the version is recorded as the string that was given"
    );
    assert_eq!(pinned[0].commit, sha, "and the commit it resolved to");
}

#[test]
fn a_branch_name_is_a_version_and_resolves_to_its_head() {
    let (source_repo, sha) = a_pack("branch-src", "neighborly");
    let machine = Machine::new("branch-machine");

    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        "main",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the pack installs: {}", lines(&r)));

    let pinned = lock::read(&machine.lock()).expect("the lock parses");
    assert_eq!(pinned[0].version, "main");
    assert_eq!(pinned[0].commit, sha);
}

/// A lock that is a symlink is replaced, never written through: the write renames
/// a whole document over the path, so a symlink to `/dev/null` becomes a regular
/// file holding the pin, and the pack installs.
#[test]
fn a_lock_symlinked_to_dev_null_is_replaced_by_the_pin_and_the_pack_installs() {
    let (source_repo, sha) = a_pack("void-src", "neighborly");
    let machine = Machine::new("void-machine");
    let source = source_of(&source_repo);
    std::os::unix::fs::symlink("/dev/null", machine.lock()).expect("the lock points at /dev/null");

    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source,
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the pack installs: {}", lines(&r)));

    let kind = std::fs::symlink_metadata(machine.lock())
        .expect("the lock stands")
        .file_type();
    assert!(!kind.is_symlink(), "the symlink is replaced");
    assert!(kind.is_file(), "by a regular file");
    let pinned = lock::read(&machine.lock()).expect("the lock parses");
    assert_eq!(pinned.len(), 1, "the lock holds the one pin: {pinned:?}");
    assert_eq!(pinned[0].source, source);
    assert_eq!(pinned[0].commit, sha);
    assert_eq!(machine.installed(), vec!["neighborly"]);
}

/// A lock the filesystem refuses outright, forced by a directory standing at the
/// temp path the write renames from, which no uid can open for writing. The write
/// reports its own failure, so this arm reads the refusal and not the read-back.
#[test]
fn an_unwritable_lock_refuses_and_leaves_no_pack() {
    let (source_repo, _) = a_pack("readonly-src", "neighborly");
    let machine = Machine::new("readonly-machine");
    std::fs::write(machine.lock(), "schema = 1\n").expect("a lock stands there");
    let tmp = machine
        .lock()
        .with_file_name(format!(".{}.tmp-{}", lock::LOCK, std::process::id()));
    std::fs::create_dir(&tmp).expect("a directory stands at the temp path");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        "v1",
        WHEN,
    )
    .expect_err("an unwritable lock is a refusal");

    assert!(
        lines(&refused).contains("packs.lock cannot be written"),
        "{}",
        lines(&refused)
    );
    assert_eq!(
        machine.installed(),
        Vec::<String>::new(),
        "no pack is installed against a pin that could not be written"
    );
}

// ------------------------------------------------- AC2: the layering refusals

#[test]
fn a_second_pack_carrying_an_installed_agent_name_refuses_and_leaves_nothing() {
    let (first_src, _) = repo(
        "collide-first",
        &[
            ("pack.toml", &manifest("first")),
            ("agents/architect/agent.toml", "name = \"architect\"\n"),
        ],
    );
    let (second_src, _) = repo(
        "collide-second",
        &[
            ("pack.toml", &manifest("second")),
            ("agents/architect/agent.toml", "name = \"architect\"\n"),
        ],
    );
    let machine = Machine::new("collide-machine");

    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&first_src),
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the first pack installs: {}", lines(&r)));

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&second_src),
        "v1",
        WHEN,
    )
    .expect_err("a duplicated agent name is a refusal");

    // The two are named in the layering's order — by name, since neither
    // imports the other — and not in the order they were added.
    assert!(
        lines(&refused).contains("the agent name `architect` is in both `first` and `second`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), vec!["first"], "no directory is left");
    let pinned = lock::read(&machine.lock()).expect("the lock parses");
    assert_eq!(pinned.len(), 1, "and no lock entry: {pinned:?}");
    assert_eq!(pinned[0].source, source_of(&first_src));
}

/// The shadow rule only means anything with a bottom layer to read it from, so
/// a machine whose defaults were never written REFUSES the add rather than
/// installing over an empty one: an absent directory walks to no files and
/// publishes no registry, and the pack that would have been refused installs
/// cleanly instead, to fail at whichever reader meets it first.
#[test]
fn an_add_on_a_machine_whose_defaults_were_never_written_is_refused_by_name() {
    let (over_src, _) = repo(
        "no-defaults-over",
        &[
            ("pack.toml", &manifest("over")),
            ("skills/greet/SKILL.md", "# greet\n"),
        ],
    );
    let machine = Machine::new("no-defaults-machine");
    std::fs::remove_dir_all(machine.defaults()).expect("the machine has not started yet");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&over_src),
        "v1",
        WHEN,
    )
    .expect_err("an unwritten bottom layer is not an empty one");

    assert!(
        lines(&refused).contains("`fleet start` writes them"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(!machine.lock().exists(), "a refusal wrote the lock anyway");

    // The control: the same add with the set written installs, so what refused
    // is the absence and not the pack.
    machine.fixture.materialize_defaults();
    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&over_src),
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("a written set installs: {}", lines(&r)));
    assert_eq!(machine.installed(), vec!["over"]);
}

/// The bottom layer is the binary's own defaults, whatever is installed — so
/// this plants the pair into the machine's copy of them rather than installing
/// a pack to stand underneath.
#[test]
fn a_pack_shadowing_a_defaults_path_outside_the_registry_refuses_and_leaves_nothing() {
    let (over_src, _) = repo(
        "shadow-over",
        &[
            ("pack.toml", &manifest("over")),
            ("skills/greet/SKILL.md", "# greet, replaced\n"),
        ],
    );
    let machine = Machine::new("shadow-machine");
    machine
        .plant(
            "assets/shadow-registry.toml",
            "schema = 1\n\n[[shadow]]\npath = \"overlay/hello.md\"\npurpose = \"the greeting a fleet may replace\"\n",
        )
        .plant("overlay/hello.md", "hello from the defaults\n")
        .plant("skills/greet/SKILL.md", "# greet, the defaults' own\n");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&over_src),
        "v1",
        WHEN,
    )
    .expect_err("an unlisted shadow is a refusal");

    assert!(
        lines(&refused).contains("`skills/greet/SKILL.md` shadows `defaults`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(
        !machine.lock().exists(),
        "a refusal before the pin wrote no lock"
    );
}

/// The control for the arm above: the refusal is the registry's answer about
/// that path, not a refusal of every path a pack and the defaults share.
#[test]
fn a_pack_shadowing_a_listed_path_installs() {
    let (over_src, _) = repo(
        "listed-over",
        &[
            ("pack.toml", &manifest("over")),
            ("overlay/hello.md", "hello from the fleet\n"),
        ],
    );
    let machine = Machine::new("listed-machine");
    machine
        .plant(
            "assets/shadow-registry.toml",
            "schema = 1\n\n[[shadow]]\npath = \"overlay/hello.md\"\npurpose = \"the greeting a fleet may replace\"\n",
        )
        .plant("overlay/hello.md", "hello from the defaults\n")
        .plant("skills/greet/SKILL.md", "# greet, the defaults' own\n");

    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&over_src),
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("a listed shadow installs: {}", lines(&r)));

    assert_eq!(machine.installed(), vec!["over"]);
    assert_eq!(
        lock::read(&machine.lock()).expect("the lock parses").len(),
        1
    );
}

/// A manifest declaring the imports it names, each by a source the verb never
/// fetches: the layering reads the names alone.
fn manifest_importing(name: &str, imports: &[&str]) -> String {
    let mut text = manifest(name);
    for import in imports {
        text.push_str(&format!(
            "\n[imports.{import}]\nsource = \"../{import}\"\nversion = \"0.1.0\"\n"
        ));
    }
    text
}

/// Three packs shaped as the shipped ones — a `base`, a `ts` nothing imports,
/// and a `tiny` importing both — added in the order a fleet that predates ts
/// meets them: base, tiny, then ts. The add lays ts beneath tiny because tiny
/// imports it, the lock records all three, and the packs directory resolves to
/// [ts, base, tiny] from the bottom up.
#[test]
fn a_pack_an_installed_pack_imports_installs_beneath_its_importer() {
    let (base_src, _) = repo(
        "importer-base",
        &[
            ("pack.toml", &manifest("base")),
            ("skills/greet/SKILL.md", "# greet, the base's own\n"),
        ],
    );
    let (tiny_src, _) = repo(
        "importer-tiny",
        &[
            ("pack.toml", &manifest_importing("tiny", &["base", "ts"])),
            ("skills/review/SKILL.md", "# review\n"),
        ],
    );
    let (ts_src, _) = repo(
        "importer-ts",
        &[
            ("pack.toml", &manifest("ts")),
            (
                "doctor/deno-version/doctor.toml",
                "name = \"deno-version\"\n",
            ),
        ],
    );
    let machine = Machine::new("importer-machine");

    for source in [&base_src, &tiny_src, &ts_src] {
        add::add(
            &machine.packs(),
            &machine.defaults(),
            &machine.lock(),
            &source_of(source),
            "v1",
            WHEN,
        )
        .unwrap_or_else(|r| panic!("{} installs: {}", source_of(source), lines(&r)));
    }

    assert_eq!(machine.installed(), vec!["base", "tiny", "ts"]);
    let pinned = lock::read(&machine.lock()).expect("the lock parses");
    let mut names: Vec<&str> = pinned.iter().filter_map(|e| e.name.as_deref()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["base", "tiny", "ts"], "one lock line per pack");

    let ordered = resolve::ordered(resolve::installed(&machine.packs()))
        .expect("the three order without a cycle");
    let mut bottom_up: Vec<&str> = ordered.iter().map(|l| l.name.as_str()).collect();
    bottom_up.reverse();
    assert_eq!(bottom_up, vec!["ts", "base", "tiny"]);
    let resolution = resolve::resolve(&ordered).expect("the layering resolves");
    assert_eq!(
        resolution
            .files
            .get("doctor/deno-version/doctor.toml")
            .map(String::as_str),
        Some("ts")
    );
}

/// The control on the arm above: with the importer named to sort BEFORE what
/// it imports, the order the names give and the order the imports give agree,
/// so that arm alone cannot tell the import edge from the alphabet. Here the
/// importer sorts after its import — `zeta` imports `alpha` — and adding zeta
/// first, alpha second still installs.
#[test]
fn the_importer_beneath_by_name_is_still_placed_above_what_it_imports() {
    let (base_src, _) = a_pack("alpha-base", "base");
    let (zeta_src, _) = repo(
        "alpha-zeta",
        &[("pack.toml", &manifest_importing("zeta", &["base", "alpha"]))],
    );
    let (alpha_src, _) = repo("alpha-alpha", &[("pack.toml", &manifest("alpha"))]);
    let machine = Machine::new("alpha-machine");

    for source in [&base_src, &zeta_src, &alpha_src] {
        add::add(
            &machine.packs(),
            &machine.defaults(),
            &machine.lock(),
            &source_of(source),
            "v1",
            WHEN,
        )
        .unwrap_or_else(|r| panic!("{} installs: {}", source_of(source), lines(&r)));
    }
    assert_eq!(machine.installed(), vec!["alpha", "base", "zeta"]);
    let ordered = resolve::ordered(resolve::installed(&machine.packs())).expect("no cycle");
    let names: Vec<&str> = ordered.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["zeta", "alpha", "base"]);
}

/// A candidate nothing installed imports, over a pack that imports: the fleet's
/// own pack is the one layer that declares imports, so a second pack laid
/// beside it is refused as before, naming the importer.
#[test]
fn a_pack_nothing_imports_over_an_importer_still_refuses_and_leaves_nothing() {
    let (base_src, _) = a_pack("stray-base", "base");
    let (tiny_src, _) = repo(
        "stray-tiny",
        &[("pack.toml", &manifest_importing("tiny", &["base"]))],
    );
    let (stray_src, _) = repo("stray-stray", &[("pack.toml", &manifest("stray"))]);
    let machine = Machine::new("stray-machine");

    for source in [&base_src, &tiny_src] {
        add::add(
            &machine.packs(),
            &machine.defaults(),
            &machine.lock(),
            &source_of(source),
            "v1",
            WHEN,
        )
        .unwrap_or_else(|r| panic!("{} installs: {}", source_of(source), lines(&r)));
    }
    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&stray_src),
        "v1",
        WHEN,
    )
    .expect_err("a pack nothing imports cannot sit beside the fleet's own");
    assert!(
        lines(&refused).contains("layer `tiny` declares its own import `base`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), vec!["base", "tiny"]);
    assert_eq!(
        lock::read(&machine.lock()).expect("the lock parses").len(),
        2
    );
}

// ------------------------------- AC3: the caret refusal and the subdirectory

#[test]
fn a_caret_version_refuses_with_one_line_and_writes_nothing() {
    let (source_repo, _) = a_pack("caret-src", "neighborly");
    let machine = Machine::new("caret-machine");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        "^1.2",
        WHEN,
    )
    .expect_err("a caret range is a refusal");

    assert_eq!(refused.len(), 1, "one line: {}", lines(&refused));
    assert!(
        lines(&refused).contains("`^1.2` is a caret range — pin a tag or `sha:<40 hex>`"),
        "{}",
        lines(&refused)
    );
    assert!(
        !machine.packs().exists() && !machine.lock().exists(),
        "a version that never parsed touches neither the packs dir nor the lock"
    );
}

#[test]
fn the_two_slash_form_installs_the_subdirectory_alone() {
    let (source_repo, _) = repo(
        "subdir-src",
        &[
            ("README.md", "the repository, not the pack\n"),
            ("packs/inner/pack.toml", &manifest("inner")),
            ("packs/inner/orders/nightly.toml", "action = \"dispatch\"\n"),
        ],
    );
    let machine = Machine::new("subdir-machine");
    let source = format!("{}//packs/inner", source_of(&source_repo));

    let installed = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source,
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the subdirectory installs: {}", lines(&r)));

    assert_eq!(installed.name, "inner");
    assert!(machine.packs().join("inner/orders/nightly.toml").is_file());
    assert!(
        !machine.packs().join("inner/README.md").exists(),
        "the repository's other files are not part of the pack"
    );
    assert_eq!(machine.installed(), vec!["inner"]);
    assert_eq!(
        lock::read(&machine.lock()).expect("the lock parses")[0].source,
        source,
        "the lock keys on the source as given, subdirectory and all"
    );
}

#[test]
fn a_source_naming_a_subdirectory_the_repository_does_not_hold_refuses() {
    let (source_repo, _) = a_pack("nosubdir-src", "neighborly");
    let machine = Machine::new("nosubdir-machine");
    let source = format!("{}//packs/inner", source_of(&source_repo));

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source,
        "v1",
        WHEN,
    )
    .expect_err("a subdirectory that is not there is a refusal");
    assert!(
        lines(&refused).contains("the repository holds no `packs/inner`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
}

// ------------------------------------------------------ the two parsers alone

#[test]
fn a_source_splits_on_the_two_slash_form_and_never_on_the_scheme() {
    assert_eq!(
        add::parse_source("https://github.com/gastownhall/gascity-packs.git//gascity"),
        Ok(Source {
            repo: "https://github.com/gastownhall/gascity-packs.git".into(),
            subdir: Some("gascity".into()),
        })
    );
    assert_eq!(
        add::parse_source("https://github.com/gastownhall/gascity-packs.git"),
        Ok(Source {
            repo: "https://github.com/gastownhall/gascity-packs.git".into(),
            subdir: None,
        })
    );
    assert_eq!(
        add::parse_source("git@github.com:gastownhall/packs.git//inner/deeper"),
        Ok(Source {
            repo: "git@github.com:gastownhall/packs.git".into(),
            subdir: Some("inner/deeper".into()),
        })
    );
    assert_eq!(
        add::parse_source("/tmp/a-local-pack"),
        Ok(Source {
            repo: "/tmp/a-local-pack".into(),
            subdir: None,
        })
    );
    assert_eq!(add::parse_source("   "), Err(Refusal::EmptySource));
    assert_eq!(
        add::parse_source("/tmp/repo//../escape"),
        Err(Refusal::EscapingSubdir("/tmp/repo//../escape".into()))
    );
}

#[test]
fn a_version_is_a_name_or_a_full_sha_and_nothing_else() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(
        add::parse_version("v1.2.0"),
        Ok(Version::Named("v1.2.0".into()))
    );
    assert_eq!(
        add::parse_version(&format!("sha:{}", sha.to_uppercase())),
        Ok(Version::Sha(sha.into())),
        "a sha is recorded lowercase whichever way it was typed"
    );
    assert_eq!(
        add::parse_version("sha:0123abc"),
        Err(Refusal::MalformedSha("sha:0123abc".into())),
        "a short sha is not a pin"
    );
    assert_eq!(
        add::parse_version("^1.2"),
        Err(Refusal::CaretVersion("^1.2".into()))
    );
    assert_eq!(add::parse_version(""), Err(Refusal::EmptyVersion));
}

// ------------------------------------------- the fixture the arms here share

/// Every arm in this file is handed the SAME source repository for the same
/// file set, so a write into one would corrupt every later arm in the binary
/// and would not reproduce under a filter that runs that arm alone. The seal in
/// `common::repo` is what forbids the write; this is the arm that goes red if
/// the seal ever comes off — both halves of it, the mode on the file and the
/// mode on the directory that holds it.
#[test]
fn a_write_into_a_shared_fixture_is_refused() {
    let (source_repo, _) = a_pack("sealed-src", "neighborly");

    let overwritten = std::fs::write(source_repo.path("pack.toml"), manifest("hijacked"))
        .expect_err("a write over a file in a shared fixture is refused");
    assert_eq!(overwritten.kind(), std::io::ErrorKind::PermissionDenied);

    let planted = std::fs::write(source_repo.path("planted.md"), "# planted\n")
        .expect_err("a new file in a shared fixture is refused");
    assert_eq!(planted.kind(), std::io::ErrorKind::PermissionDenied);

    assert_eq!(
        std::fs::read_to_string(source_repo.path("pack.toml")).expect("the manifest still reads"),
        manifest("neighborly"),
        "and the fixture the next arm gets is the one it was built as"
    );
}

// ------------------------------------------------------------ the other exits

#[test]
fn a_fetched_pack_with_a_defect_is_named_and_not_installed() {
    let (source_repo, _) = repo(
        "defect-src",
        &[
            ("pack.toml", &manifest("broken")),
            ("bin/dispatch", "#!/bin/sh\n"),
        ],
    );
    let machine = Machine::new("defect-machine");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        "v1",
        WHEN,
    )
    .expect_err("a pack with a defect is a refusal");

    assert!(
        lines(&refused).contains("broken: unknown top-level name `bin`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(!machine.lock().exists(), "and nothing is pinned");
}

#[test]
fn a_pack_name_already_installed_refuses_rather_than_replacing_it() {
    let (first, _) = a_pack("dup-first", "neighborly");
    let (second, _) = repo(
        "dup-second",
        &[
            ("pack.toml", &manifest("neighborly")),
            ("overlay/x.md", "x\n"),
        ],
    );
    let machine = Machine::new("dup-machine");

    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&first),
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the first installs: {}", lines(&r)));

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&second),
        "v1",
        WHEN,
    )
    .expect_err("a name already installed is a refusal");
    assert!(
        lines(&refused).contains("a pack named `neighborly` is already installed"),
        "{}",
        lines(&refused)
    );
    assert!(
        machine
            .packs()
            .join("neighborly/skills/greet/SKILL.md")
            .is_file(),
        "the installed pack is untouched"
    );
}

/// A source git cannot clone leaves the packs directory as it found it — the
/// scratch directory of the failed fetch included.
#[test]
fn a_source_git_cannot_clone_refuses_naming_the_step() {
    let machine = Machine::new("noclone-machine");
    let missing = machine.fixture.path("not-a-repository");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &missing.display().to_string(),
        "v1",
        WHEN,
    )
    .expect_err("a source that is not a repository is a refusal");

    assert!(lines(&refused).contains("git clone"), "{}", lines(&refused));
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(!machine.lock().exists());
}

#[test]
fn a_version_the_repository_does_not_carry_refuses_naming_the_step() {
    let (source_repo, _) = a_pack("norev-src", "neighborly");
    let machine = Machine::new("norev-machine");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        "v9",
        WHEN,
    )
    .expect_err("a version that is not there is a refusal");

    assert!(
        lines(&refused).contains("git checkout"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
}

// ------------------------------------------ fleet-4fw: the packs a pack imports

/// A checkout shaped as this workspace ships its packs: tiny under `packs/tiny`
/// importing ts from `../ts`, and ts beside it.
fn a_checkout_of_tiny_and_ts(label: &str) -> (common::Fixture, String) {
    repo(
        label,
        &[
            ("packs/tiny/pack.toml", &manifest_importing("tiny", &["ts"])),
            ("packs/tiny/skills/review/SKILL.md", "# review\n"),
            ("packs/ts/pack.toml", &manifest("ts")),
            (
                "packs/ts/doctor/deno-version/doctor.toml",
                "name = \"deno-version\"\n",
            ),
        ],
    )
}

/// The demo's first flight, as fleet-4fw found it: tiny added alone. The import
/// the same checkout holds comes in with it, out of the same clone, so it is
/// pinned at the same commit and keyed on the source a person would have typed
/// for it.
#[test]
fn an_import_the_same_checkout_holds_is_installed_with_its_importer() {
    let (checkout, sha) = a_checkout_of_tiny_and_ts("with-tiny");
    let machine = Machine::new("with-machine");
    let tiny = format!("{}//packs/tiny", source_of(&checkout));

    let added = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &tiny,
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("tiny installs: {}", lines(&r)));

    assert_eq!(added.name, "tiny");
    let imports: Vec<&str> = added.imports.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(imports, vec!["ts"], "the import came in with it");
    assert!(added.missing.is_empty(), "{:?}", added.missing);
    assert_eq!(machine.installed(), vec!["tiny", "ts"]);
    assert!(machine
        .packs()
        .join("ts/doctor/deno-version/doctor.toml")
        .is_file());

    let ts = format!("{}//packs/ts", source_of(&checkout));
    let mut pinned = lock::read(&machine.lock()).expect("the lock parses");
    pinned.sort_by(|a, b| a.source.cmp(&b.source));
    assert_eq!(
        pinned,
        vec![
            lock::Entry {
                source: tiny,
                name: Some("tiny".into()),
                version: "v1".into(),
                commit: sha.clone(),
                fetched: WHEN.into(),
                tree: None,
            },
            lock::Entry {
                source: ts,
                name: Some("ts".into()),
                version: "v1".into(),
                commit: sha,
                fetched: WHEN.into(),
                tree: None,
            },
        ]
    );
    resolve::resolve(
        &resolve::layers(&machine.packs(), &machine.defaults()).expect("the two order"),
    )
    .expect("and the layering resolves");
}

/// An import already installed is the import answered: the add fetches only the
/// pack it was given, whichever order the two came in.
#[test]
fn an_import_already_installed_is_not_installed_again() {
    let (checkout, _) = a_checkout_of_tiny_and_ts("already-tiny");
    let machine = Machine::new("already-machine");

    for subdir in ["ts", "tiny"] {
        let added = add::add(
            &machine.packs(),
            &machine.defaults(),
            &machine.lock(),
            &format!("{}//packs/{subdir}", source_of(&checkout)),
            "v1",
            WHEN,
        )
        .unwrap_or_else(|r| panic!("{subdir} installs: {}", lines(&r)));
        assert!(added.imports.is_empty(), "{:?}", added.imports);
        assert!(added.missing.is_empty(), "{:?}", added.missing);
    }
    assert_eq!(machine.installed(), vec!["tiny", "ts"]);
    assert_eq!(lock::read(&machine.lock()).expect("parses").len(), 2);
}

/// An import outside the checkout is not fetched — its version is the
/// manifest's, which no add has checked is a ref — so the pack installs without
/// it, and the add names it with the exact line that adds it, read against the
/// importer's own source.
#[test]
fn an_import_outside_the_checkout_is_named_with_the_line_that_adds_it() {
    let (tiny_src, _) = repo(
        "outside-tiny",
        &[("pack.toml", &manifest_importing("tiny", &["ts"]))],
    );
    let machine = Machine::new("outside-machine");

    let added = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&tiny_src),
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("tiny installs: {}", lines(&r)));

    assert!(added.imports.is_empty());
    let sibling = tiny_src.root.parent().expect("a parent").join("ts");
    let line = format!("fleet pack add {} --version 0.1.0", sibling.display());
    assert_eq!(
        added.missing,
        vec![add::Missing {
            importer: "tiny".into(),
            import: "ts".into(),
            source: "../ts".into(),
            line: Some(line.clone()),
        }]
    );
    let said = added.missing[0].to_string();
    assert!(
        said.contains("`tiny` imports `ts`, which is not installed") && said.contains(&line),
        "{said}"
    );
    assert_eq!(machine.installed(), vec!["tiny"]);

    // What run and prime read later: the same line, from the installed packs
    // and the lock alone.
    assert_eq!(
        add::missing_imports(&resolve::installed(&machine.packs()), &machine.lock()),
        added.missing
    );
}

/// A checkout that does not hold the import it declares: the pack still
/// installs, and the import is named without a line, because the only one there
/// is to give would name a directory that is not there.
#[test]
fn an_import_the_checkout_does_not_hold_is_named_without_a_line() {
    let (checkout, _) = repo(
        "nothere-tiny",
        &[("packs/tiny/pack.toml", &manifest_importing("tiny", &["ts"]))],
    );
    let machine = Machine::new("nothere-machine");

    let added = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &format!("{}//packs/tiny", source_of(&checkout)),
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("tiny installs: {}", lines(&r)));

    assert_eq!(added.missing.len(), 1, "{:?}", added.missing);
    assert_eq!(added.missing[0].line, None);
    assert!(
        added.missing[0]
            .to_string()
            .contains("`tiny` imports `ts`, which is not installed"),
        "{}",
        added.missing[0]
    );
    assert_eq!(machine.installed(), vec!["tiny"]);
}

/// The import's own manifest calls it something else: installing it would leave
/// the import still unanswered under a second name, so the add refuses whole.
#[test]
fn an_import_that_calls_itself_another_name_refuses_and_leaves_nothing() {
    let (checkout, _) = repo(
        "misnamed-tiny",
        &[
            ("packs/tiny/pack.toml", &manifest_importing("tiny", &["ts"])),
            ("packs/ts/pack.toml", &manifest("typescript")),
        ],
    );
    let machine = Machine::new("misnamed-machine");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &format!("{}//packs/tiny", source_of(&checkout)),
        "v1",
        WHEN,
    )
    .expect_err("an import under another name is a refusal");

    assert!(
        lines(&refused).contains(
            "`tiny` imports `ts` from `../ts`, and the pack there calls itself `typescript`"
        ),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(!machine.lock().exists());
}

/// The import comes in under the same layering check as its importer, and a
/// refusal of either leaves neither.
#[test]
fn an_import_the_layering_refuses_leaves_neither_pack() {
    let (checkout, _) = repo(
        "deep-tiny",
        &[
            ("packs/tiny/pack.toml", &manifest_importing("tiny", &["ts"])),
            ("packs/ts/pack.toml", &manifest_importing("ts", &["deno"])),
        ],
    );
    let machine = Machine::new("deep-machine");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &format!("{}//packs/tiny", source_of(&checkout)),
        "v1",
        WHEN,
    )
    .expect_err("a transitive import is a refusal");

    assert!(
        lines(&refused).contains("layer `ts` declares its own import `deno`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(!machine.lock().exists());
}

/// A pack placed by hand has no lock line to read a source from: its missing
/// import is still named, without a line.
#[test]
fn a_missing_import_of_a_pack_the_lock_does_not_hold_is_named_without_a_line() {
    let machine = Machine::new("byhand-machine");
    std::fs::create_dir_all(machine.packs().join("tiny")).expect("the pack dir");
    std::fs::write(
        machine.packs().join("tiny/pack.toml"),
        manifest_importing("tiny", &["ts"]),
    )
    .expect("the manifest");

    let missing = add::missing_imports(&resolve::installed(&machine.packs()), &machine.lock());
    assert_eq!(
        missing,
        vec![add::Missing {
            importer: "tiny".into(),
            import: "ts".into(),
            source: "../ts".into(),
            line: None,
        }]
    );
}

#[test]
fn an_import_source_is_read_against_its_importers_source() {
    let at = |repo: &str, subdir: Option<&str>| Source {
        repo: repo.into(),
        subdir: subdir.map(String::from),
    };
    let tiny = at("https://example.invalid/o/fleet", Some("packs/tiny"));
    let root = at("https://example.invalid/o/tiny", None);

    let cases: [(&Source, &str, add::Located); 8] = [
        (
            &tiny,
            "../ts",
            add::Located::Inside(at("https://example.invalid/o/fleet", Some("packs/ts"))),
        ),
        (
            &tiny,
            "./vendor/ts",
            add::Located::Inside(at(
                "https://example.invalid/o/fleet",
                Some("packs/tiny/vendor/ts"),
            )),
        ),
        (
            &tiny,
            "../..",
            add::Located::Inside(at("https://example.invalid/o/fleet", None)),
        ),
        (
            &tiny,
            "../../../ts",
            add::Located::Elsewhere("https://example.invalid/o/ts".into()),
        ),
        (
            &root,
            "../ts",
            add::Located::Elsewhere("https://example.invalid/o/ts".into()),
        ),
        (
            &tiny,
            "https://example.invalid/p/ts//packs/ts",
            add::Located::Elsewhere("https://example.invalid/p/ts//packs/ts".into()),
        ),
        (
            &tiny,
            "/srv/packs/ts",
            add::Located::Elsewhere("/srv/packs/ts".into()),
        ),
        (
            &tiny,
            "git@example.invalid:o/ts.git",
            add::Located::Elsewhere("git@example.invalid:o/ts.git".into()),
        ),
    ];
    for (importer, declared, want) in cases {
        assert_eq!(
            add::locate_import(importer, declared),
            want,
            "{declared} against {importer}"
        );
    }
    assert_eq!(
        tiny.to_string(),
        "https://example.invalid/o/fleet//packs/tiny"
    );
    assert_eq!(root.to_string(), "https://example.invalid/o/tiny");
}
