//! `packs.lock`: the format, and what a reader gets back from what was written.

mod common;

use common::Fixture;
use fleet_core::lock::{self, Entry, LockError};

/// An entry with no `name`, which is the shape of every line written before the
/// key existed and the shape most of these arms are about.
fn entry(source: &str, version: &str, commit: &str) -> Entry {
    Entry {
        source: source.to_string(),
        name: None,
        version: version.to_string(),
        commit: commit.to_string(),
        fetched: String::from("2026-09-08T13:45:00Z"),
        tree: None,
    }
}

fn named(name: &str, source: &str, version: &str, commit: &str) -> Entry {
    Entry {
        name: Some(name.to_string()),
        ..entry(source, version, commit)
    }
}

const A_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const ANOTHER_COMMIT: &str = "89abcdef0123456789abcdef0123456789abcdef";

#[test]
fn the_rendered_document_is_the_reference_shape() {
    let rendered = lock::render(&[named(
        "gascity",
        "https://github.com/gastownhall/gascity-packs.git//gascity",
        "0.4.0",
        A_COMMIT,
    )]);
    assert_eq!(
        rendered,
        format!(
            "schema = 1\n\n\
             [packs.\"https://github.com/gastownhall/gascity-packs.git//gascity\"]\n\
             name = \"gascity\"\n\
             version = \"0.4.0\"\n\
             commit = \"{A_COMMIT}\"\n\
             fetched = \"2026-09-08T13:45:00Z\"\n"
        )
    );
}

/// The key is optional at both ends: a document written before it existed parses
/// to entries with no name, and rendering such an entry writes no key at all —
/// which is what keeps the schema at 1.
#[test]
fn a_document_written_before_the_name_key_parses_and_renders_without_it() {
    let pre_bead = format!(
        "schema = 1\n\n\
         [packs.a]\n\
         version = \"v1\"\n\
         commit = \"{A_COMMIT}\"\n\
         fetched = \"2026-09-08T13:45:00Z\"\n"
    );
    let read = lock::parse(&pre_bead).expect("a pre-name document parses");
    assert_eq!(read, vec![entry("a", "v1", A_COMMIT)]);
    assert_eq!(read[0].name, None);

    let rendered = lock::render(&read);
    assert!(!rendered.contains("name"), "{rendered}");
    assert_eq!(rendered, pre_bead, "the document round-trips byte for byte");
}

/// A drop leaves the rest of the document exactly as a render of it alone: the
/// whole file is re-written, so "the other line is untouched" is a claim about
/// bytes and is read as one.
#[test]
fn removing_one_entry_leaves_the_other_byte_identical() {
    let fixture = Fixture::new("lock-remove");
    let path = fixture.path("packs.lock");
    let kept = named("beta", "b", "v2", ANOTHER_COMMIT);

    lock::append(&path, &named("alpha", "a", "v1", A_COMMIT)).expect("the first pin lands");
    lock::append(&path, &kept).expect("the second pin lands");

    lock::remove(&path, "a").expect("the drop lands");

    assert_eq!(lock::read(&path), Ok(vec![kept.clone()]));
    assert_eq!(
        std::fs::read_to_string(&path).expect("the lock is readable"),
        lock::render(std::slice::from_ref(&kept))
    );

    // A source the lock does not hold is not this function's refusal to make: it
    // writes the document back and the caller decides.
    lock::remove(&path, "a").expect("a second drop of the same source is not an error");
    assert_eq!(lock::read(&path), Ok(vec![kept]));
}

#[test]
fn what_the_renderer_writes_is_what_the_parser_reads_back() {
    let entries = vec![
        entry("https://example.invalid/a.git//inner", "v1", A_COMMIT),
        entry("/tmp/local-pack", "main", ANOTHER_COMMIT),
    ];
    let read = lock::parse(&lock::render(&entries)).expect("the rendered document parses");
    // Sorted by source, which is why the read-back is compared as a set of the
    // same members rather than in the order they were handed over.
    assert_eq!(read.len(), 2);
    for one in &entries {
        assert!(read.contains(one), "{one:?} is missing from {read:?}");
    }
}

#[test]
fn an_absent_lock_and_an_empty_one_are_both_an_empty_lock() {
    let fixture = Fixture::new("lock-absent");
    let path = fixture.path("packs.lock");
    assert_eq!(lock::read(&path), Ok(Vec::new()), "absent");
    std::fs::write(&path, "").expect("the empty lock is written");
    assert_eq!(lock::read(&path), Ok(Vec::new()), "empty");
    assert_eq!(lock::parse("   \n\n"), Ok(Vec::new()), "blank");
}

#[test]
fn a_second_entry_joins_the_first_and_a_re_add_moves_the_pin() {
    let fixture = Fixture::new("lock-append");
    let path = fixture.path("packs.lock");

    lock::append(&path, &entry("a", "v1", A_COMMIT)).expect("the first pin lands");
    lock::append(&path, &entry("b", "v2", ANOTHER_COMMIT)).expect("the second pin lands");
    assert_eq!(lock::read(&path).expect("the lock parses").len(), 2);

    lock::append(&path, &entry("a", "v3", ANOTHER_COMMIT)).expect("the re-pin lands");
    let read = lock::read(&path).expect("the lock parses");
    assert_eq!(read.len(), 2, "a re-add moves the pin, never adds a second");
    assert!(read.contains(&entry("a", "v3", ANOTHER_COMMIT)));
    assert!(!read.contains(&entry("a", "v1", A_COMMIT)));
}

#[test]
fn holds_answers_field_for_field_and_not_by_source_alone() {
    let fixture = Fixture::new("lock-holds");
    let path = fixture.path("packs.lock");
    let pinned = entry("a", "v1", A_COMMIT);
    lock::append(&path, &pinned).expect("the pin lands");

    assert_eq!(lock::holds(&path, &pinned), Ok(true));
    assert_eq!(
        lock::holds(&path, &entry("a", "v1", ANOTHER_COMMIT)),
        Ok(false),
        "the same source at another commit is not this pin"
    );
}

#[test]
fn a_schema_that_is_not_one_is_refused_naming_the_number() {
    assert_eq!(lock::parse("schema = 2\n"), Err(LockError::Schema(2)));
    assert_eq!(lock::parse("packs = {}\n"), Err(LockError::NoSchema));
}

#[test]
fn an_entry_missing_a_field_is_refused_naming_the_field() {
    let text = "schema = 1\n\n[packs.\"a\"]\nversion = \"v1\"\ncommit = \"abc\"\n";
    assert_eq!(
        lock::parse(text),
        Err(LockError::Key {
            source: "a".into(),
            key: "fetched"
        })
    );
}

/// A source is whatever a person typed, so the key it becomes is escaped rather
/// than trusted: an unescaped quote renders a document that does not parse, and
/// the round trip is the reading that says so.
#[test]
fn a_source_carrying_a_quote_round_trips() {
    let awkward = entry("https://example.invalid/a\"b\\c.git", "v1", A_COMMIT);
    let read =
        lock::parse(&lock::render(std::slice::from_ref(&awkward))).expect("the document parses");
    assert_eq!(read, vec![awkward]);
}

/// Which of the two spellings TOML allows the renderer reaches for, pinned
/// rather than left to the serializer's judgement, because the answer decides
/// the bytes on disk: a key needing no quotes is written bare, and a string
/// carrying a quote or a backslash is written as a literal rather than escaped.
/// Both parse to the value that went in, which is what the round trip holds.
#[test]
fn a_bare_key_stays_bare_and_an_awkward_value_becomes_a_literal() {
    let bare = entry("mypack", "v1", A_COMMIT);
    let rendered = lock::render(std::slice::from_ref(&bare));
    assert!(
        rendered.contains("[packs.mypack]"),
        "a key needing no quotes is written bare: {rendered}"
    );
    assert_eq!(lock::parse(&rendered), Ok(vec![bare]));

    let awkward = entry("s", "a \"quoted\" tag", "c\\d");
    let rendered = lock::render(std::slice::from_ref(&awkward));
    assert!(
        rendered.contains("version = 'a \"quoted\" tag'"),
        "a value carrying a quote is written as a literal: {rendered}"
    );
    assert_eq!(lock::parse(&rendered), Ok(vec![awkward]));
}

/// A rewrite that fails leaves the lock it meant to replace byte for byte, and no
/// temp file beside it. The failure is a read-only file planted at the temp path,
/// spelled as the write spells it; uid 0 opens it anyway, so that branch reads
/// the landing instead. Neither branch returns without asserting.
#[test]
fn a_rewrite_that_cannot_open_its_temp_file_leaves_the_old_lock_byte_identical() {
    use std::os::unix::fs::PermissionsExt;
    extern "C" {
        fn geteuid() -> u32;
    }

    let fixture = Fixture::new("lock-atomic");
    let path = fixture.path(lock::LOCK);
    let dir = path.parent().expect("the lock has a directory");
    lock::write(
        &path,
        &[entry("a", "v1", A_COMMIT), entry("b", "v2", ANOTHER_COMMIT)],
    )
    .expect("the two-entry lock lands");
    let before = std::fs::read(&path).expect("the lock is readable");

    let prefix = format!(".{}.tmp-", lock::LOCK);
    let tmp = dir.join(format!("{prefix}{}", std::process::id()));
    std::fs::write(&tmp, b"debris from a previous write").expect("the temp path is planted");
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))
        .expect("the planted file is made read-only");

    let written = lock::append(&path, &entry("c", "v3", A_COMMIT));

    // SAFETY: geteuid takes nothing, returns the effective uid and cannot fail.
    if unsafe { geteuid() } == 0 {
        written.expect("uid 0 opens the read-only temp file, so the write lands");
        assert_eq!(lock::read(&path).expect("the lock parses").len(), 3);
    } else {
        assert!(
            matches!(written, Err(LockError::Unwritable(_))),
            "the temp file cannot be opened for writing, so the write fails: {written:?}"
        );
        assert_eq!(
            std::fs::read(&path).expect("the lock is readable"),
            before,
            "the lock is the whole old document"
        );
    }

    let debris: Vec<String> = std::fs::read_dir(dir)
        .expect("the lock's directory is readable")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix))
        .collect();
    assert!(
        debris.is_empty(),
        "a temp file outlived the write: {debris:?}"
    );
}
