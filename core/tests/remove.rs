//! `fleet pack remove`, against packs the tests install with `add` first.
//!
//! A removal is asserted about two things on disk — the directory and the lock
//! line — so every arm reads both, and a refusal arm reads both again to say
//! nothing moved.

mod common;

use std::path::Path;

use common::{a_pack, manifest, repo, source_of, Machine, WHEN};
use fleet_core::add;
use fleet_core::lock;
use fleet_core::remove::{self, Notice, Refusal};

fn lines(refusals: &[Refusal]) -> String {
    refusals
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A pack whose manifest declares one import, by the name and source given.
fn a_pack_importing(label: &str, name: &str, import: &str, source: &str) -> common::Fixture {
    let (fixture, _) = repo(
        label,
        &[
            (
                "pack.toml",
                &format!(
                    "{}\n[imports.{import}]\nsource = \"{source}\"\nversion = \"^1\"\n",
                    manifest(name)
                ),
            ),
            ("skills/greet/SKILL.md", "# greet\n"),
        ],
    );
    fixture
}

/// One pack installed into a machine, answered as the source it was added by.
fn install(machine: &Machine, fixture: &common::Fixture) -> String {
    let source = source_of(fixture);
    add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source,
        "v1",
        WHEN,
    )
    .unwrap_or_else(|r| panic!("the fixture installs: {r:?}"));
    source
}

fn sources(machine: &Machine) -> Vec<String> {
    lock::read(&machine.lock())
        .expect("the lock parses")
        .into_iter()
        .map(|e| e.source)
        .collect()
}

// ------------------------------------------------------- AC1: the round trip

#[test]
fn a_pack_added_is_removed_and_its_line_dropped() {
    let (source_repo, _) = a_pack("rm-src", "neighborly");
    let machine = Machine::new("rm-machine");
    let source = install(&machine, &source_repo);
    assert_eq!(machine.installed(), vec!["neighborly"]);

    let removed = remove::remove(&machine.packs(), &machine.lock(), &source)
        .unwrap_or_else(|r| panic!("the pack is removed: {}", lines(&r)));

    assert_eq!(removed.name, "neighborly");
    assert_eq!(removed.root, machine.packs().join("neighborly"));
    assert_eq!(removed.entry.source, source);
    assert_eq!(removed.entry.version, "v1");
    assert_eq!(removed.notices, Vec::new());
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert_eq!(lock::read(&machine.lock()), Ok(Vec::new()));
}

/// The source is matched as `add` keyed it, so the same string with surrounding
/// space is the same pack: `add` trims and this trims the same way.
#[test]
fn the_source_is_matched_trimmed_as_add_keyed_it() {
    let (source_repo, _) = a_pack("trim-src", "neighborly");
    let machine = Machine::new("trim-machine");
    let source = install(&machine, &source_repo);

    let padded = format!("  {source}\n");
    remove::remove(&machine.packs(), &machine.lock(), &padded)
        .unwrap_or_else(|r| panic!("a padded source is the same key: {}", lines(&r)));
    assert_eq!(machine.installed(), Vec::<String>::new());
}

/// One pack out of two leaves the other where it was, directory and line both.
#[test]
fn removing_one_of_two_leaves_the_other_installed() {
    let (first_repo, _) = a_pack("two-a", "neighborly");
    let (second_repo, _) = a_pack("two-b", "gastown");
    let machine = Machine::new("two-machine");
    let first = install(&machine, &first_repo);
    let second = install(&machine, &second_repo);

    remove::remove(&machine.packs(), &machine.lock(), &first)
        .unwrap_or_else(|r| panic!("the first is removed: {}", lines(&r)));

    assert_eq!(machine.installed(), vec!["gastown"]);
    assert_eq!(sources(&machine), vec![second]);
}

// ---------------------------------------------------------- AC2: the refusals

#[test]
fn a_source_the_lock_does_not_hold_is_refused_naming_it_and_the_lock() {
    let (source_repo, _) = a_pack("absent-src", "neighborly");
    let machine = Machine::new("absent-machine");
    let installed = install(&machine, &source_repo);

    let refused = remove::remove(&machine.packs(), &machine.lock(), "/nowhere/at/all")
        .expect_err("a source the lock does not hold is a refusal");

    let text = lines(&refused);
    assert!(text.contains("/nowhere/at/all"), "{text}");
    assert!(
        text.contains(&machine.lock().display().to_string()),
        "the refusal names the lock: {text}"
    );
    assert_eq!(machine.installed(), vec!["neighborly"]);
    assert_eq!(sources(&machine), vec![installed]);
}

/// The bottom layer is not a pack and is not removable: the source is answered
/// before the lock is read, so a machine that has pinned it and one that has
/// not are one answer.
#[test]
fn the_binarys_own_defaults_are_refused_and_the_machine_is_left_alone() {
    let (source_repo, _) = a_pack("defaults-src", "neighborly");
    let machine = Machine::new("defaults-machine");
    let installed = install(&machine, &source_repo);

    let refused = remove::remove(
        &machine.packs(),
        &machine.lock(),
        fleet_core::defaults::SOURCE,
    )
    .expect_err("the defaults are not removable");

    assert!(
        lines(&refused).contains("is not removable"),
        "{}",
        lines(&refused)
    );
    assert!(
        lines(&refused).contains(fleet_core::defaults::SOURCE),
        "{}",
        lines(&refused)
    );
    assert!(
        machine.defaults().join("assets/rules.md").is_file(),
        "the defaults still stand"
    );
    assert_eq!(machine.installed(), vec!["neighborly"]);
    assert_eq!(sources(&machine), vec![installed]);
}

/// The control on the arm above, from the other end: the refusal is keyed on
/// the SOURCE, so an ordinary pack whose own source merely resembles it comes
/// out. A pack that takes the bottom layer's NAME never gets this far — `add`
/// refuses the layering it would make (the arm below).
#[test]
fn a_pack_whose_source_merely_resembles_the_defaults_key_comes_out() {
    let (source_repo, _) = a_pack("near-defaults-src", "neighborly");
    let machine = Machine::new("near-defaults-machine");
    let source = install(&machine, &source_repo);
    assert_ne!(source, fleet_core::defaults::SOURCE);

    let removed = remove::remove(&machine.packs(), &machine.lock(), &source)
        .expect("an ordinary pack comes out");

    assert_eq!(removed.name, "neighborly");
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert_eq!(sources(&machine), Vec::<String>::new());
}

/// A pack cannot take the bottom layer's name: two layers under one name
/// resolve a default to the wrong root, so `add` refuses the install.
#[test]
fn a_pack_calling_itself_defaults_is_refused_at_the_add() {
    let (source_repo, _) = a_pack("named-defaults-src", fleet_core::defaults::LAYER);
    let machine = Machine::new("named-defaults-machine");

    let refused = add::add(
        &machine.packs(),
        &machine.defaults(),
        &machine.lock(),
        &source_of(&source_repo),
        "v1",
        WHEN,
    )
    .expect_err("the bottom layer's name is not a pack's to take");

    let text = refused
        .iter()
        .map(|r| r.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("the binary's own bottom layer"), "{text}");
    assert_eq!(machine.installed(), Vec::<String>::new());
    assert!(!machine.lock().exists(), "a refusal wrote the lock anyway");
}

#[test]
fn a_pack_imported_by_name_is_refused_naming_the_importer() {
    let (imported_repo, _) = a_pack("import-name-a", "neighborly");
    let machine = Machine::new("import-name-machine");
    let imported = install(&machine, &imported_repo);
    let importer_repo = a_pack_importing(
        "import-name-b",
        "gastown",
        "neighborly",
        "git@example:some-other-place",
    );
    install(&machine, &importer_repo);

    let refused = remove::remove(&machine.packs(), &machine.lock(), &imported)
        .expect_err("an imported pack is not removable");

    assert_eq!(refused.len(), 1, "{}", lines(&refused));
    assert!(
        lines(&refused).contains("`gastown` imports `neighborly`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), vec!["gastown", "neighborly"]);
    assert!(sources(&machine).contains(&imported));
}

/// The other half of the import check: a manifest that names the source as it
/// was typed rather than the pack's name. Both are declared shapes, so both
/// refuse — and this arm's importer declares an import name that matches no
/// installed pack, so only the source can be what catches it.
#[test]
fn a_pack_imported_by_source_is_refused_naming_the_importer() {
    let (imported_repo, _) = a_pack("import-source-a", "neighborly");
    let machine = Machine::new("import-source-machine");
    let imported = install(&machine, &imported_repo);
    let importer_repo = a_pack_importing("import-source-b", "gastown", "elsewhere", &imported);
    install(&machine, &importer_repo);

    let refused = remove::remove(&machine.packs(), &machine.lock(), &imported)
        .expect_err("a pack imported by source is not removable");

    assert!(
        lines(&refused).contains("`gastown` imports `neighborly`"),
        "{}",
        lines(&refused)
    );
    assert_eq!(machine.installed(), vec!["gastown", "neighborly"]);
    assert!(sources(&machine).contains(&imported));
}

/// A pack directory written straight into the packs dir, with no lock line and
/// no fetch. The resolver refuses a layer that declares an import of its own —
/// imports are one level deep — so a second importer is planted rather than
/// added, and this verb's walk reads the directory either way.
fn plant_importer(machine: &Machine, name: &str, import: &str) {
    let root = machine.packs().join(name);
    std::fs::create_dir_all(root.join("skills/greet")).expect("the planted pack is created");
    std::fs::write(root.join("skills/greet/SKILL.md"), "# greet\n").expect("the skill is written");
    std::fs::write(
        root.join("pack.toml"),
        format!(
            "{}\n[imports.{import}]\nsource = \"git@example:elsewhere\"\nversion = \"^1\"\n",
            manifest(name)
        ),
    )
    .expect("the planted manifest is written");
}

#[test]
fn every_importer_is_named_one_line_each() {
    let (imported_repo, _) = a_pack("many-a", "neighborly");
    let machine = Machine::new("many-machine");
    let imported = install(&machine, &imported_repo);
    plant_importer(&machine, "gastown", "neighborly");
    plant_importer(&machine, "riverside", "neighborly");

    let refused = remove::remove(&machine.packs(), &machine.lock(), &imported)
        .expect_err("two importers are two refusals");

    assert_eq!(refused.len(), 2, "{}", lines(&refused));
    assert!(
        lines(&refused).contains("`gastown` imports"),
        "{}",
        lines(&refused)
    );
    assert!(
        lines(&refused).contains("`riverside` imports"),
        "{}",
        lines(&refused)
    );
}

/// A pack that fails its own check cannot be read for imports, so the walk names
/// it and passes over it rather than refusing every removal on the machine.
#[test]
fn an_installed_pack_that_does_not_check_is_named_and_skipped() {
    let (source_repo, _) = a_pack("skip-a", "neighborly");
    let machine = Machine::new("skip-machine");
    let source = install(&machine, &source_repo);

    // A directory the resolver counts as installed — it has a manifest — whose
    // manifest does not parse, planted after `add` so `add`'s own check cannot
    // refuse it.
    let broken = machine.packs().join("broken");
    std::fs::create_dir_all(&broken).expect("the broken pack's directory is created");
    std::fs::write(broken.join("pack.toml"), "this is not toml = = =\n")
        .expect("the broken manifest is written");

    let removed = remove::remove(&machine.packs(), &machine.lock(), &source)
        .unwrap_or_else(|r| panic!("an unreadable neighbour does not refuse: {}", lines(&r)));

    assert_eq!(removed.notices, vec![Notice::Unreadable("broken".into())]);
    assert!(!machine.packs().join("neighborly").exists());
}

// ------------------------------------------------- AC3: the order and the name

/// The state the spec's write order can leave — a line whose directory is gone —
/// is the one this verb clears, so it is a success and not a refusal.
#[test]
fn a_line_whose_directory_was_deleted_by_hand_is_dropped() {
    let (source_repo, _) = a_pack("gone-src", "neighborly");
    let machine = Machine::new("gone-machine");
    let source = install(&machine, &source_repo);

    let root = machine.packs().join("neighborly");
    std::fs::remove_dir_all(&root).expect("the directory is deleted by hand");

    let removed = remove::remove(&machine.packs(), &machine.lock(), &source)
        .unwrap_or_else(|r| panic!("the line is dropped anyway: {}", lines(&r)));

    assert_eq!(removed.notices, vec![Notice::DirectoryAlreadyGone(root)]);
    assert!(
        removed.notices[0].to_string().contains("was already gone"),
        "{}",
        removed.notices[0]
    );
    assert_eq!(lock::read(&machine.lock()), Ok(Vec::new()));
}

/// A line written before the name key names no directory, so the verb refuses
/// and says what pins it: a re-add of the same source and version, which
/// replaces the line.
#[test]
fn a_line_with_no_name_is_refused_naming_the_re_add() {
    let machine = Machine::new("noname-machine");
    std::fs::create_dir_all(machine.packs()).expect("the packs dir is created");
    std::fs::write(
        machine.lock(),
        "schema = 1\n\n\
         [packs.\"git@example:packs//neighborly\"]\n\
         version = \"v1\"\n\
         commit = \"0123456789abcdef0123456789abcdef01234567\"\n\
         fetched = \"2026-09-08T13:45:00Z\"\n",
    )
    .expect("the pre-name lock is written");

    let refused = remove::remove(
        &machine.packs(),
        &machine.lock(),
        "git@example:packs//neighborly",
    )
    .expect_err("a line with no name is a refusal");

    let text = lines(&refused);
    assert!(text.contains("has no `name` in the lock"), "{text}");
    assert!(
        text.contains("fleet pack add git@example:packs//neighborly --version v1"),
        "the refusal names the re-add that pins it: {text}"
    );
    assert!(text.contains("the directory is untouched"), "{text}");
    assert_eq!(
        sources(&machine),
        vec!["git@example:packs//neighborly".to_string()],
        "the line stands"
    );
}

/// The directory goes first and the lock second, which is what leaves a line
/// with no directory on a failure between them rather than the reverse. The
/// reading: a lock that cannot be written after the directory is gone, forced as
/// `an_unwritable_lock_refuses_and_leaves_no_pack` forces it — a directory at the
/// temp path the write renames from, which no uid can open for writing.
#[test]
fn a_lock_that_cannot_be_written_leaves_a_line_with_no_directory() {
    let (source_repo, _) = a_pack("order-src", "neighborly");
    let machine = Machine::new("order-machine");
    let source = install(&machine, &source_repo);

    let tmp = machine
        .lock()
        .with_file_name(format!(".{}.tmp-{}", lock::LOCK, std::process::id()));
    std::fs::create_dir(&tmp).expect("a directory stands at the temp path");

    let outcome = remove::remove(&machine.packs(), &machine.lock(), &source);
    let installed = machine.installed();
    let still_locked = read_sources(&machine.lock());

    std::fs::remove_dir(&tmp).expect("the temp path is cleared");

    let refused = outcome.expect_err("an unwritable lock is a refusal");
    assert!(
        lines(&refused).contains("cannot be written"),
        "{}",
        lines(&refused)
    );
    assert_eq!(
        installed,
        Vec::<String>::new(),
        "the directory went first, so it is gone"
    );
    assert_eq!(
        still_locked,
        vec![source.clone()],
        "and the line it left is the state a re-run clears"
    );

    // A re-run clears it: the very claim the order is chosen for.
    let removed = remove::remove(&machine.packs(), &machine.lock(), &source)
        .unwrap_or_else(|r| panic!("the re-run clears the line: {}", lines(&r)));
    assert!(matches!(
        removed.notices.as_slice(),
        [Notice::DirectoryAlreadyGone(_)]
    ));
    assert_eq!(lock::read(&machine.lock()), Ok(Vec::new()));
}

/// The other half of the order, and the one the two orders answer differently:
/// a directory that cannot be deleted. Going directory-first, the failure is met
/// before the lock is touched and both stand; going lock-first, the line would be
/// gone and the directory would remain — a directory with no line, which `add`
/// refuses as a collision and this verb refuses as a source it does not hold.
#[test]
fn an_unremovable_directory_refuses_and_leaves_the_line() {
    use std::os::unix::fs::PermissionsExt;
    extern "C" {
        fn geteuid() -> u32;
    }

    let (source_repo, _) = a_pack("locked-src", "neighborly");
    let machine = Machine::new("locked-machine");
    let source = install(&machine, &source_repo);

    // An unlink goes through the holding directory's write bit, so that is where
    // the mode goes.
    std::fs::set_permissions(machine.packs(), std::fs::Permissions::from_mode(0o555))
        .expect("the packs dir is made read-only");

    let outcome = remove::remove(&machine.packs(), &machine.lock(), &source);
    let still_locked = read_sources(&machine.lock());
    let still_there = machine.packs().join("neighborly").is_dir();

    std::fs::set_permissions(machine.packs(), std::fs::Permissions::from_mode(0o755))
        .expect("the packs dir is made writable again");

    // SAFETY: geteuid takes nothing, returns the effective uid and cannot fail.
    if unsafe { geteuid() } == 0 {
        outcome.unwrap_or_else(|r| panic!("uid 0 unlinks through the mode bit: {}", lines(&r)));
        return;
    }

    let refused = outcome.expect_err("a directory that cannot be deleted is a refusal");
    assert!(
        lines(&refused).contains("cannot be removed"),
        "{}",
        lines(&refused)
    );
    assert!(still_there, "the directory stands");
    assert_eq!(still_locked, vec![source], "and so does its line");
}

fn read_sources(path: &Path) -> Vec<String> {
    lock::read(path)
        .expect("the lock parses")
        .into_iter()
        .map(|e| e.source)
        .collect()
}
