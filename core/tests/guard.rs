//! The guard classes, over text alone.
//!
//! Every check owns a table of (label, command, expected), and every table
//! carries BOTH answers: the shapes the check refuses, and the correct commands
//! beside them that it must not. A table of refusals alone would pass against a
//! check that refuses everything, which is the failure mode a guard has to be
//! held away from hardest — a guard seats prefix their escape to by reflex is
//! worse than no guard at all.
//!
//! The must-name tables are the second half: a refusal a seat cannot act on is
//! one it routes around, so every reason is held to naming the fragment it is
//! about, the rewrite, and the one variable that licenses that act.

use fleet_core::guard::{self, Class, Policy, Verdict};

/// (label, command, refused?)
type Case = (&'static str, &'static str, bool);

const PREFIX: &str = "acme";

/// The targets of the two classes a pack wires, written here rather than read
/// from any tree: the lists ARE the target set, so a table that derived them
/// would be measuring a derivation this module deliberately does not carry.
///
/// The glob is the shape a project writes when its releases are cut on a ref
/// path rather than under one namespace, and it is what makes the bare and the
/// nested spellings in the table below answer the same way.
const RELEASE_GLOB: &str = "refs/heads/*release/*";
const BUCKET: &str = "live.example.test";
const PROJECT: &str = "example-production";
const APP: &str = "example-frontdoor";
/// The three product-specific target sets, written here for the reason the
/// three above are: the lists ARE the target set, so a table that derived them
/// would measure a derivation the class deliberately does not carry.
const MAKE_GOAL: &str = "ship-it";
const DAGGER_FUNCTION: &str = "deployExecute";
const WORKFLOW_REF: &str = "pipeline.yml:backend/release/*";

fn policy() -> Policy {
    Policy {
        enabled: true,
        item_prefix: Some(PREFIX.to_string()),
        release_ref_glob: Some(RELEASE_GLOB.to_string()),
        prod_buckets: vec![BUCKET.to_string()],
        prod_projects: vec![PROJECT.to_string()],
        prod_apps: vec![APP.to_string()],
        prod_make_goals: vec![MAKE_GOAL.to_string()],
        prod_dagger_functions: vec![DAGGER_FUNCTION.to_string()],
        prod_workflow_refs: vec![WORKFLOW_REF.to_string()],
        // The two readings only a caller can make are left out of the shared
        // policy on purpose: every table below then measures the flag-named
        // path alone, and the pair gets its own arm with both answers.
        cwd_app: None,
        active_project: None,
    }
}

fn verdict(class: Class, command: &str) -> Verdict {
    guard::judge(class, command, &policy())
}

fn refused(class: Class, command: &str) -> Option<guard::Denial> {
    match verdict(class, command) {
        Verdict::Refused(denial) => Some(denial),
        Verdict::Silent => None,
    }
}

/// Every case of one table, and the check each refusal is expected to come
/// from: a case refused by the wrong check is a green nobody measured.
fn run(class: Class, check: &str, cases: &[Case]) {
    for (label, command, want) in cases {
        match refused(class, command) {
            Some(denial) => {
                assert!(
                    *want,
                    "{label}: refused, and this command is correct — {command}"
                );
                assert_eq!(
                    denial.check, check,
                    "{label}: refused by {} rather than by {check}",
                    denial.check
                );
            }
            None => assert!(
                !*want,
                "{label}: allowed, and this command is the trap — {command}"
            ),
        }
    }
    let refusals = cases.iter().filter(|(_, _, want)| *want).count();
    let controls = cases.len() - refusals;
    assert!(
        refusals >= 6 && controls >= 3,
        "{check}: {refusals} refusals and {controls} controls — a table needs both halves"
    );
}

// ---- the shell-trap class ---------------------------------------------------

#[test]
fn a_backtick_inside_a_stored_string_is_refused_and_the_inert_spellings_are_not() {
    run(
        Class::ShellTrap,
        "record-backtick",
        &[
            ("a note", "bd note x-1 \"the `date` it ran\"", true),
            ("a created title", "bd create \"a `hostname` title\"", true),
            (
                "an appended note",
                "bd update x-1 --append-notes \"see `git log`\"",
                true,
            ),
            (
                "a close reason",
                "bd close x-1 --reason \"fixed by `whoami`\"",
                true,
            ),
            ("a description", "bd update x-1 -d \"a `b` c\"", true),
            // The two surfaces the shared table adds: this class reads the
            // record class's own list, so a title and a comment are guarded
            // here exactly as they are guarded there.
            ("a title", "bd update x-1 --title \"the `b` thing\"", true),
            ("a comment", "bd comment x-1 \"the `date` it ran\"", true),
            (
                "the second positional",
                "bd note x-1 \"one\" \"two `three`\"",
                true,
            ),
            (
                "invoked by path",
                "/usr/local/bin/bd note x-1 \"a `b`\"",
                true,
            ),
            (
                "behind another statement",
                "cd /tmp && bd note x-1 \"a `b`\"",
                true,
            ),
            (
                "an inline flag value",
                "bd update x-1 --design=\"a `b`\"",
                true,
            ),
            (
                "single quotes are inert",
                "bd note x-1 'the `date` it ran'",
                false,
            ),
            (
                "the substitution form the rewrite prescribes",
                "bd note x-1 \"$(cat /tmp/note.txt)\"",
                false,
            ),
            (
                "a subcommand that stores nothing",
                "bd show \"a `b`\"",
                false,
            ),
            ("not the work graph at all", "echo \"a `b`\"", false),
            (
                "an ordinary note",
                "bd note x-1 \"nothing quoted here\"",
                false,
            ),
        ],
    );
}

#[test]
fn an_unbraced_name_before_a_modifier_is_refused_and_the_braced_form_is_not() {
    run(
        Class::ShellTrap,
        "modifier",
        &[
            ("the tail modifier", "git show \"$S:tools/land\"", true),
            (
                "the absolute-path modifier",
                "ls \"$S:app/main.dart\"",
                true,
            ),
            ("unquoted", "git show $S:head", true),
            ("on an assignment's right-hand side", "X=$S:root ls", true),
            ("the root modifier", "printf '%s' \"$VAR:r\"", true),
            ("the extension modifier", "ls \"$D:e\"", true),
            ("the quoting modifier", "ls \"$D:Q\"", true),
            ("the ampersand modifier", "ls \"$D:&\"", true),
            (
                "the braced form the rewrite prescribes",
                "ls \"${S}:tools/land\"",
                false,
            ),
            ("single quotes are inert", "ls '$S:tools/land'", false),
            (
                "a literal character after the colon",
                "ls \"$S:brain/x.md\"",
                false,
            ),
            ("a slash after the colon", "ls \"$S:/absolute\"", false),
            ("a digit after the colon", "ls \"$S:2026\"", false),
        ],
    );
}

#[test]
fn a_bare_variable_where_a_list_is_meant_is_refused_and_the_split_forms_are_not() {
    run(
        Class::ShellTrap,
        "unsplit-variable",
        &[
            (
                "a loop over one variable",
                "for x in $LIST; do echo hi; done",
                true,
            ),
            (
                "the braced spelling",
                "for f in ${FILES}; do echo hi; done",
                true,
            ),
            ("a loop over ids", "for id in $IDS; do echo hi; done", true),
            ("a list handed to xargs", "xargs rm $PATHS", true),
            ("a list of process ids", "kill $PIDS", true),
            ("a pathspec", "git diff -- $FILES", true),
            ("a pathspec on another verb", "git add -- $PATHS", true),
            (
                "a pathspec after a flag",
                "git log --oneline -- $FILES",
                true,
            ),
            (
                "explicit arguments, which is the rewrite",
                "for b in $A $B $C; do echo hi; done",
                false,
            ),
            ("a literal list", "for x in a b c; do echo hi; done", false),
            (
                "quoted, so one argument is meant",
                "for x in \"$LIST\"; do echo hi; done",
                false,
            ),
            (
                "the shell's own explicit split",
                "for x in ${=LIST}; do echo hi; done",
                false,
            ),
            ("one path, one argument", "rm -rf $SCRATCH", false),
            ("no bare variable at all", "xargs -0 echo hi", false),
        ],
    );
}

#[test]
fn a_status_read_behind_a_formatting_pipe_is_refused_and_a_deciding_stage_is_not() {
    run(
        Class::ShellTrap,
        "pipe-rc",
        &[
            (
                "behind tail",
                "bd update x-1 --claim | tail -1; echo $?",
                true,
            ),
            ("behind head", "make dev | head -5; echo $?", true),
            (
                "behind head, whose own first stage decides",
                "diff a b | head; echo $?",
                true,
            ),
            ("behind uniq", "cargo test | uniq; echo $?", true),
            ("behind sed", "cargo test | sed -n 1p; echo $?", true),
            ("behind awk", "cargo test | awk '{print}'; echo $?", true),
            ("behind cut", "cargo test | cut -f1; echo $?", true),
            ("behind wc", "cargo test | wc -l; echo $?", true),
            (
                "the braced spelling",
                "cargo test | cat; printf '%s' ${?}",
                true,
            ),
            (
                "two stages, the last a formatter",
                "a | b | tr -d x; echo $?",
                true,
            ),
            (
                "behind diff, whose status is the answer asked for",
                "git show HEAD:f | diff - f; echo $?",
                false,
            ),
            (
                "behind cmp, the same",
                "git show HEAD:f | cmp - f; echo $?",
                false,
            ),
            (
                "behind grep, whose status is its own match answer",
                "cargo test | grep -q FAILED; echo $?",
                false,
            ),
            (
                "behind test, the same",
                "wc -l < f | test 3 -lt 4; echo $?",
                false,
            ),
            ("no pipe at all", "cargo test > out.txt; echo $?", false),
            ("no status read", "cargo test | tail -1", false),
            (
                "a status read of its own command",
                "cargo test; echo $?",
                false,
            ),
        ],
    );
}

#[test]
fn a_chain_that_swallows_a_third_answer_is_refused_and_a_two_valued_question_is_not() {
    run(
        Class::ShellTrap,
        "false-alternative",
        &[
            (
                "an ancestor check",
                "git merge-base --is-ancestor abc origin/main && echo YES || echo NO",
                true,
            ),
            (
                "a merge check",
                "git merge-tree --write-tree origin/main abc && echo SAFE || echo CARRIES",
                true,
            ),
            (
                "a grep whose file may be absent",
                "grep -q x f && echo FOUND || echo MISSING",
                true,
            ),
            (
                "a diff whose file may be absent",
                "diff a b && echo SAME || echo DIFFERS",
                true,
            ),
            (
                "a cmp restore check",
                "cmp -s a b && echo SAME || echo DIFFERS",
                true,
            ),
            (
                "a transfer",
                "curl -f https://example.invalid && echo UP || echo DOWN",
                true,
            ),
            (
                "printf on the alternative arm",
                "grep x f && true || printf FAILED",
                true,
            ),
            (
                "the colon builtin on it",
                "diff a b && echo SAME || :",
                true,
            ),
            (
                "a two-valued question named honestly",
                "test -e p && echo EXISTS || echo MISSING",
                false,
            ),
            (
                "the same shape on ls",
                "ls p && echo exists || echo absent",
                false,
            ),
            ("no alternative arm", "grep -q x f && echo FOUND", false),
            (
                "a semicolon breaks the chain",
                "grep -q x f; echo A || echo B",
                false,
            ),
            (
                "an arm that does not name an answer",
                "grep -q x f && echo FOUND || exit 1",
                false,
            ),
        ],
    );
}

// ---- the record class -------------------------------------------------------

#[test]
fn the_flag_that_replaces_the_notes_field_is_refused_and_the_appending_forms_are_not() {
    run(
        Class::Record,
        "notes-replace",
        &[
            ("the flag", "bd update x-1 --notes \"new\"", true),
            ("the inline spelling", "bd update x-1 --notes=new", true),
            (
                "behind another statement",
                "cd /tmp && bd update x-1 --notes n",
                true,
            ),
            (
                "invoked by path",
                "/usr/local/bin/bd update x-1 --notes n",
                true,
            ),
            ("the flag before the id", "bd update --notes n x-1", true),
            (
                "behind an assignment",
                "FOO=1 bd update x-1 --notes n",
                true,
            ),
            (
                "the appending twin",
                "bd update x-1 --append-notes \"new\"",
                false,
            ),
            ("the append verb", "bd note x-1 \"new\"", false),
            (
                "another field entirely",
                "bd update x-1 --status open",
                false,
            ),
            (
                "its own escape",
                "FLEET_NOTES_REPLACE_OK=1 bd update x-1 --notes n",
                false,
            ),
            (
                "prose naming the rule",
                "echo 'bd update --notes is refused'",
                false,
            ),
        ],
    );
}

#[test]
fn a_write_through_the_sql_route_is_refused_and_a_read_is_not() {
    run(
        Class::Record,
        "sql-write",
        &[
            (
                "an update",
                "bd sql \"UPDATE issues SET status = 'open'\"",
                true,
            ),
            ("a delete", "bd sql \"DELETE FROM issues\"", true),
            (
                "behind a leading comment",
                "bd sql \"/* why */ UPDATE issues SET a = 1\"",
                true,
            ),
            (
                "behind a CTE",
                "bd sql \"WITH t AS (SELECT 1) UPDATE issues SET a = 1\"",
                true,
            ),
            (
                "an insert",
                "bd sql \"INSERT INTO issues VALUES (1)\"",
                true,
            ),
            ("a drop", "bd sql \"DROP TABLE issues\"", true),
            ("a plain read", "bd sql \"SELECT * FROM issues\"", false),
            (
                "a write keyword inside a literal",
                "bd sql \"SELECT 'UPDATE' FROM issues\"",
                false,
            ),
            (
                "a write keyword inside a comment",
                "bd sql \"SELECT 1 -- UPDATE later\"",
                false,
            ),
            (
                "its own escape",
                "FLEET_SQL_WRITE_OK=1 bd sql \"UPDATE issues SET a = 1\"",
                false,
            ),
            ("another subcommand", "bd update x-1 --status open", false),
        ],
    );
}

#[test]
fn an_item_named_by_its_bare_suffix_is_refused_and_a_full_id_is_not() {
    run(
        Class::Record,
        "bare-id",
        &[
            ("in a note", "bd note x-1 \"see a1b2 for the reason\"", true),
            ("in a comment", "bd comment x-1 \"a1b2 says so\"", true),
            ("a child id", "bd note x-1 \"a1b2.3 says so\"", true),
            (
                "in a close reason",
                "bd close x-1 --reason \"fixed by a1b2\"",
                true,
            ),
            ("in a title", "bd create \"a1b2 follow-up\"", true),
            (
                "in a design field",
                "bd update x-1 --design \"per a1b2\"",
                true,
            ),
            (
                "two of them",
                "bd update x-1 --append-notes \"a1b2 and z9y8\"",
                true,
            ),
            (
                "the full id",
                "bd note x-1 \"see acme-a1b2 for the reason\"",
                false,
            ),
            ("a filename", "bd note x-1 \"the file a1b2.txt\"", false),
            (
                "a branch name",
                "bd note x-1 \"on branch seat/fix/a1b2\"",
                false,
            ),
            (
                "ordinary prose",
                "bd note x-1 \"nothing that looks like an id\"",
                false,
            ),
            ("a year", "bd note x-1 \"measured 2026 on the box\"", false),
            (
                "its own escape",
                "FLEET_BARE_ID_OK=1 bd note x-1 \"see a1b2\"",
                false,
            ),
            ("not a guarded surface", "bd show a1b2", false),
        ],
    );
}

/// An entry is written by the verb that owns its act, never by hand: a comment
/// whose text carries the entry key, or whose text the guard cannot read
/// because it comes from a file or the input, is refused, and a person's own
/// words pass.
#[test]
fn an_entry_written_by_hand_is_refused_and_a_persons_comment_is_not() {
    run(
        Class::Record,
        "entry-forge",
        &[
            (
                "a forged entry",
                r#"bd comments add fx-1 '{"fleet.entry":1,"kind":"landed"}'"#,
                true,
            ),
            (
                "double-quoted",
                r#"bd comments add fx-1 "{\"fleet.entry\":1,\"kind\":\"held\"}""#,
                true,
            ),
            (
                "the key anywhere in the text",
                "bd comments add fx-1 'see fleet.entry'",
                true,
            ),
            (
                "the text behind a flag",
                r#"bd comments add fx-1 --author a-seat '{"fleet.entry":1}'"#,
                true,
            ),
            ("from a file", "bd comments add fx-1 -f entry.json", true),
            (
                "from a file, the long flag",
                "bd comments add fx-1 --file entry.json",
                true,
            ),
            (
                "from a file, the inline spelling",
                "bd comments add fx-1 --file=entry.json",
                true,
            ),
            (
                "from a file, the value attached",
                "bd comments add fx-1 -fentry.json",
                true,
            ),
            (
                "the shorthand verb",
                r#"bd comment fx-1 '{"fleet.entry":1,"kind":"landed"}'"#,
                true,
            ),
            (
                "the shorthand from a file",
                "bd comment fx-1 --file entry.json",
                true,
            ),
            (
                "the shorthand from its input",
                "bd comment fx-1 --stdin < entry.json",
                true,
            ),
            (
                "behind another statement",
                r#"cd /tmp && bd comments add fx-1 '{"fleet.entry":1}'"#,
                true,
            ),
            (
                "invoked by path",
                r#"/usr/local/bin/bd comments add fx-1 '{"fleet.entry":1}'"#,
                true,
            ),
            (
                "a person's comment",
                r#"bd comments add fx-1 "looks good""#,
                false,
            ),
            (
                "a person's comment, unquoted",
                "bd comments add fx-1 looks good",
                false,
            ),
            (
                "a person's comment with an author",
                r#"bd comments add fx-1 "looks good" --author alberto"#,
                false,
            ),
            (
                "the shorthand's own",
                r#"bd comment fx-1 "looks good""#,
                false,
            ),
            ("the listing", "bd comments fx-1 --json", false),
            (
                "the listing, read for entries",
                "bd comments fx-1 --json | grep fleet.entry",
                false,
            ),
            (
                "a note naming the key",
                r#"bd note fx-1 "the fleet.entry key""#,
                false,
            ),
            (
                "its own escape",
                r#"FLEET_ENTRY_FORGE_OK=1 bd comments add fx-1 '{"fleet.entry":1}'"#,
                false,
            ),
            (
                "prose naming the rule",
                r#"echo 'bd comments add fx-1 {"fleet.entry":1}'"#,
                false,
            ),
        ],
    );
}

/// AC3 as it is worded: the forged comment is denied with the rewrite, a
/// person's comment passes, and the flag that replaces the notes field is
/// still refused, its reason restated — the field is a person's words, and
/// fleet writes none.
#[test]
fn a_forged_entry_is_denied_with_the_rewrite_and_the_notes_flag_names_a_persons_words() {
    let denial = refused(
        Class::Record,
        r#"bd comments add fx-1 '{"fleet.entry":1,"kind":"landed"}'"#,
    )
    .expect("the forged entry is denied");
    assert_eq!(denial.class, "record");
    assert_eq!(denial.check, "entry-forge");
    assert_eq!(denial.label, "ENTRY FORGED");
    assert_eq!(
        denial.rewrite,
        "record through the verb that owns the act: fleet \
         dispatch|deliver|review|hold|clear|land|cancel"
    );
    assert_eq!(
        denial.why,
        "an entry is the record every verb decides from; one written by hand skips the checks \
         its verb makes before it writes"
    );

    assert_eq!(
        verdict(Class::Record, r#"bd comments add fx-1 "looks good""#),
        Verdict::Silent,
        "a person's comment passes"
    );

    let denial =
        refused(Class::Record, "bd update fx-1 --notes x").expect("the replacement is denied");
    assert_eq!(denial.check, "notes-replace");
    assert_eq!(
        denial.why,
        "this flag REPLACES the whole notes field, which is a person's own words on the item and \
         not recoverable from the write itself"
    );
}

// ---- the release-ref class --------------------------------------------------

#[test]
fn a_push_whose_destination_matches_the_glob_is_refused_and_every_other_push_is_not() {
    run(
        Class::ReleaseRef,
        "push-target",
        &[
            ("a bare release name", "git push origin release/1.2", true),
            ("a nested one", "git push origin backend/release/1.2", true),
            (
                "a fully qualified one",
                "git push origin refs/heads/app/release/2.0",
                true,
            ),
            (
                "a source-colon-destination",
                "git push origin main:backend/release/1.2",
                true,
            ),
            (
                "its plus-prefixed form",
                "git push origin +main:backend/release/1.2",
                true,
            ),
            ("a deletion", "git push origin :backend/release/1.2", true),
            (
                "the deletion flag's value",
                "git push --delete origin backend/release/1.2",
                true,
            ),
            (
                "the force-with-lease flag's value",
                "git push origin --force-with-lease=backend/release/1.2 main",
                true,
            ),
            (
                "a wrapper before the command word",
                "env git push origin release/1.2",
                true,
            ),
            (
                "a compound with a directory change first",
                "cd /tmp && git push origin release/1.2",
                true,
            ),
            (
                "a work branch",
                "git push origin HEAD:refs/heads/ab/feat/the-thing",
                false,
            ),
            ("not a push at all", "git fetch origin release/1.2", false),
            (
                "a branch merely named like one",
                "git push origin release-notes",
                false,
            ),
            ("the flag that names no ref", "git push --all origin", false),
            (
                "the other flag that names no ref",
                "git push --mirror origin",
                false,
            ),
            (
                "a release ref inside a heredoc body",
                "bd note x-1 \"$(cat <<'EOF'\ngit push origin release/1.2\nEOF\n)\"",
                false,
            ),
            (
                "a release ref inside a stored note",
                "bd note x-1 \"never git push origin release/1.2 from a seat\"",
                false,
            ),
        ],
    );
}

/// The glob's own semantics, stated where they can be read rather than inferred
/// from the table above: a star crosses a slash, which is what makes one glob
/// string mean the same thing here and in the hook beneath this layer.
#[test]
fn a_star_in_the_glob_crosses_a_slash_and_a_question_mark_takes_exactly_one_character() {
    use fleet_core::guard::release_ref::matches_glob;

    assert!(
        matches_glob("refs/heads/*", "refs/heads/a/b/c"),
        "a star crosses slashes"
    );
    assert!(matches_glob(
        "refs/heads/*/release/*",
        "refs/heads/app/release/1.2"
    ));
    assert!(
        matches_glob("refs/heads/*/release/*", "refs/heads/a/b/release/1.2"),
        "and it crosses more than one"
    );
    assert!(!matches_glob(
        "refs/heads/*/release/*",
        "refs/heads/release/1.2"
    ));
    assert!(matches_glob("refs/heads/release/?", "refs/heads/release/9"));
    assert!(
        !matches_glob("refs/heads/release/?", "refs/heads/release/10"),
        "a question mark takes exactly one character"
    );
    assert!(matches_glob("*", ""), "a star matches the empty run");
    assert!(
        !matches_glob("", "x"),
        "and an empty pattern matches nothing else"
    );
    assert!(
        !matches_glob("refs/heads/[a-z]*", "refs/heads/abc"),
        "a bracket class is literal here, so a pattern carrying one refuses less"
    );
}

// ---- the production-write class ---------------------------------------------

#[test]
fn a_write_to_a_listed_bucket_is_refused_and_a_read_and_an_unlisted_one_are_not() {
    run(
        Class::ProductionWrite,
        "bucket",
        &[
            ("a removal", "gsutil rm gs://live.example.test/x", true),
            (
                "a copy to it",
                "gsutil cp ./a gs://live.example.test/a",
                true,
            ),
            (
                "a copy FROM it, because this verb reads its direction from argument order",
                "gsutil cp gs://live.example.test/a ./a",
                true,
            ),
            (
                "a sync",
                "gsutil rsync -r ./a gs://live.example.test/a",
                true,
            ),
            (
                "a policy written through a sub-verb",
                "gsutil iam set policy.json gs://live.example.test",
                true,
            ),
            (
                "the bucket itself",
                "gsutil rb gs://live.example.test",
                true,
            ),
            (
                "the cloud tool's storage group",
                "gcloud storage rm gs://live.example.test/x",
                true,
            ),
            (
                "its group-and-sub-verb form",
                "gcloud storage buckets delete gs://live.example.test",
                true,
            ),
            (
                "a wrapper before the command word",
                "env gsutil rm gs://live.example.test/x",
                true,
            ),
            ("a listing", "gsutil ls gs://live.example.test", false),
            (
                "a read of the same policy",
                "gsutil iam get gs://live.example.test",
                false,
            ),
            (
                "a bucket nobody listed",
                "gsutil rm gs://other.example.test/x",
                false,
            ),
            (
                "a listing through the cloud tool",
                "gcloud storage ls gs://live.example.test",
                false,
            ),
            (
                "its own escape",
                "FLEET_PROD_WRITE_OK=1 gsutil rm gs://live.example.test/x",
                false,
            ),
            (
                "a command that merely names the host",
                "echo gs://live.example.test",
                false,
            ),
        ],
    );
}

#[test]
fn a_write_naming_a_listed_project_is_refused_and_a_read_and_an_unlisted_one_are_not() {
    run(
        Class::ProductionWrite,
        "project",
        &[
            (
                "a deploy naming it",
                "gcloud run deploy api --project example-production",
                true,
            ),
            (
                "the short flag",
                "gcloud sql instances create db -p example-production",
                true,
            ),
            (
                "the attached form",
                "gcloud functions deploy fn --project=example-production",
                true,
            ),
            (
                "the third deploy command",
                "gcloud app deploy --project example-production",
                true,
            ),
            (
                "a write in a listed group",
                "gcloud compute instances delete vm --project example-production",
                true,
            ),
            (
                "a verb neither vocabulary knows, inside a listed group",
                "gcloud iam frobnicate --project example-production",
                true,
            ),
            (
                "the deployment tool",
                "firebase deploy --project example-production",
                true,
            ),
            (
                "the configuration write that changes every later command",
                "gcloud config set project example-production",
                true,
            ),
            (
                "a read",
                "gcloud sql instances list --project example-production",
                false,
            ),
            (
                "a project nobody listed",
                "gcloud run deploy api --project other-example",
                false,
            ),
            (
                "a group that reaches no project",
                "gcloud auth login --project example-production",
                false,
            ),
            (
                "the deployment tool reading",
                "firebase projects:list --project example-production",
                false,
            ),
            (
                "its own escape",
                "FLEET_PROD_WRITE_OK=1 gcloud run deploy api --project example-production",
                false,
            ),
            (
                "no flag, and nothing handed in to resolve it against",
                "gcloud run deploy api",
                false,
            ),
        ],
    );
}

#[test]
fn a_write_naming_a_listed_application_is_refused_and_a_read_and_an_unlisted_one_are_not() {
    run(
        Class::ProductionWrite,
        "app",
        &[
            ("a deploy", "fly deploy -a example-frontdoor", true),
            (
                "the long flag, and the other command word",
                "flyctl deploy --app example-frontdoor",
                true,
            ),
            ("a scale", "fly scale count 3 -a example-frontdoor", true),
            (
                "a secret set",
                "fly secrets set A=1 -a example-frontdoor",
                true,
            ),
            (
                "the machines group",
                "fly machines restart 1234 -a example-frontdoor",
                true,
            ),
            (
                "the group with no sub-verb at all",
                "fly machines -a example-frontdoor",
                true,
            ),
            (
                "a wrapper before the command word",
                "env fly deploy -a example-frontdoor",
                true,
            ),
            (
                "a read inside the group",
                "fly machines list -a example-frontdoor",
                false,
            ),
            (
                "an application nobody listed",
                "fly deploy -a other-frontdoor",
                false,
            ),
            ("a status", "fly status -a example-frontdoor", false),
            (
                "its own escape",
                "FLEET_PROD_WRITE_OK=1 fly deploy -a example-frontdoor",
                false,
            ),
            (
                "no flag, and nothing handed in to resolve it against",
                "fly deploy",
                false,
            ),
        ],
    );
}

/// The two readings core cannot make, handed in through the policy: a deploy in
/// an application's own directory, and a write under a configured project. Both
/// arms carry both answers, because a target handed in that refused everything
/// would read exactly like one that worked.
#[test]
fn the_two_readings_the_caller_hands_in_refuse_only_when_what_they_name_is_listed() {
    let here = |app: &str| Policy {
        cwd_app: Some(app.to_string()),
        ..policy()
    };
    assert!(
        refused(Class::ProductionWrite, "fly deploy").is_none(),
        "with nothing handed in the check has no target and refuses nothing"
    );
    let denial = match guard::judge(Class::ProductionWrite, "fly deploy", &here(APP)) {
        Verdict::Refused(denial) => denial,
        Verdict::Silent => panic!("a deploy with no flag, in a listed application's own directory"),
    };
    assert!(
        denial.fragment.contains(APP) && denial.fragment.contains("no --app"),
        "the refusal says which reading it refused on — {}",
        denial.fragment
    );
    assert_eq!(
        guard::judge(
            Class::ProductionWrite,
            "fly deploy",
            &here("other-frontdoor")
        ),
        Verdict::Silent,
        "and the same deploy where the directory names something nobody listed"
    );

    let configured = |project: &str| Policy {
        active_project: Some(project.to_string()),
        ..policy()
    };
    assert!(
        refused(Class::ProductionWrite, "gcloud run deploy api").is_none(),
        "with nothing handed in the check has no target and refuses nothing"
    );
    let denial = match guard::judge(
        Class::ProductionWrite,
        "gcloud run deploy api",
        &configured(PROJECT),
    ) {
        Verdict::Refused(denial) => denial,
        Verdict::Silent => panic!("a deploy naming no project, under a listed configured one"),
    };
    assert!(
        denial.fragment.contains(PROJECT) && denial.fragment.contains("no --project"),
        "the refusal says which reading it refused on — {}",
        denial.fragment
    );
    assert_eq!(
        guard::judge(
            Class::ProductionWrite,
            "gcloud run deploy api",
            &configured("other-example")
        ),
        Verdict::Silent,
        "and the same deploy under a configured project nobody listed"
    );
}

/// An unconfigured target, at the level the class itself answers it: a target
/// that is absent or empty leaves its check refusing nothing, and the reader
/// that the check flag prints from agrees with the judgment rather than
/// restating it.
#[test]
fn an_unconfigured_target_refuses_nothing_and_the_reader_says_so() {
    let empty = Policy {
        release_ref_glob: None,
        prod_buckets: Vec::new(),
        prod_projects: Vec::new(),
        prod_apps: Vec::new(),
        ..policy()
    };
    for command in [
        "git push origin release/1.2",
        "gsutil rm gs://live.example.test/x",
        "gcloud run deploy api --project example-production",
        "fly deploy -a example-frontdoor",
    ] {
        let class = if command.starts_with("git") {
            Class::ReleaseRef
        } else {
            Class::ProductionWrite
        };
        assert_eq!(
            guard::judge(class, command, &empty),
            Verdict::Silent,
            "no target, nothing to match against, nothing refused — {command}"
        );
    }

    // A glob written and left blank is a key nobody filled in, not a pattern
    // that matches the empty ref.
    let blank = Policy {
        release_ref_glob: Some(String::new()),
        ..policy()
    };
    assert_eq!(
        guard::judge(Class::ReleaseRef, "git push origin release/1.2", &blank),
        Verdict::Silent,
        "an empty glob is not configured"
    );

    for (class, check) in [
        (Class::ReleaseRef, "push-target"),
        (Class::ProductionWrite, "bucket"),
        (Class::ProductionWrite, "project"),
        (Class::ProductionWrite, "app"),
    ] {
        assert!(
            class.target_of(check).is_some(),
            "{check} names the key it needs"
        );
        assert!(
            guard::configured(class, check, &policy()),
            "{check} reads as configured off a policy that carries its target"
        );
        assert!(
            !guard::configured(class, check, &empty),
            "{check} reads as unconfigured off a policy that does not"
        );
    }

    // The targets, off a file's text, through the census.
    let text =
        "[guards.targets]\nrelease_ref_glob = \"refs/heads/*\"\nprod_buckets = [\"a\", \"b\"]\n";
    let targets = guard::targets_in(text);
    assert_eq!(targets.release_ref_glob.as_deref(), Some("refs/heads/*"));
    assert_eq!(targets.prod_buckets, vec!["a".to_string(), "b".to_string()]);
    assert!(
        targets.prod_projects.is_empty() && targets.prod_apps.is_empty(),
        "a key the file omits reads as the empty list"
    );
    assert_eq!(
        guard::targets_in("[guards.targets]\nprod_apps = \"one\"\n").prod_apps,
        Vec::<String>::new(),
        "a value of the wrong type reads as no target rather than as one"
    );
    assert_eq!(
        guard::app_in("app = \"the-frontdoor\"\nprimary_region = \"iad\"\n").as_deref(),
        Some("the-frontdoor"),
        "the caller's own file is parsed here and opened there"
    );
}

// ---- the two halves of the refusal ------------------------------------------

/// One specimen per check, which the tables above have already proved refused.
const SPECIMENS: [(Class, &str, &str); 13] = [
    (
        Class::ShellTrap,
        "record-backtick",
        "bd note x-1 \"the `date` it ran\"",
    ),
    (Class::ShellTrap, "modifier", "git show \"$S:tools/land\""),
    (
        Class::ShellTrap,
        "unsplit-variable",
        "for x in $LIST; do echo hi; done",
    ),
    (Class::ShellTrap, "pipe-rc", "make dev | tail -1; echo $?"),
    (
        Class::ShellTrap,
        "false-alternative",
        "grep -q x f && echo FOUND || echo MISSING",
    ),
    (Class::Record, "notes-replace", "bd update x-1 --notes n"),
    (
        Class::Record,
        "sql-write",
        "bd sql \"UPDATE issues SET a = 1\"",
    ),
    (Class::Record, "bare-id", "bd note x-1 \"see a1b2\""),
    (
        Class::Record,
        "entry-forge",
        "bd comments add x-1 '{\"fleet.entry\":1,\"kind\":\"landed\"}'",
    ),
    (
        Class::ReleaseRef,
        "push-target",
        "git push origin release/1.2",
    ),
    (
        Class::ProductionWrite,
        "bucket",
        "gsutil rm gs://live.example.test/x",
    ),
    (
        Class::ProductionWrite,
        "project",
        "gcloud run deploy api --project example-production",
    ),
    (
        Class::ProductionWrite,
        "app",
        "fly deploy -a example-frontdoor",
    ),
];

#[test]
fn every_refusal_names_the_fragment_the_rewrite_and_the_escape() {
    for (class, check, command) in SPECIMENS {
        let denial = refused(class, command).unwrap_or_else(|| panic!("{check}: {command}"));
        assert_eq!(denial.check, check, "{check}: refused by another check");
        let reason = denial.reason();
        assert!(
            reason.contains(&denial.fragment),
            "{check}: the reason does not carry the fragment — {reason}"
        );
        assert!(
            reason.contains(&denial.rewrite),
            "{check}: the reason does not carry the rewrite — {reason}"
        );
        match denial.escape {
            Some(escape) => assert!(
                reason.contains(escape),
                "{check}: the reason does not carry the escape — {reason}"
            ),
            None => assert!(
                reason.contains(guard::NO_ESCAPE),
                "{check}: the reason does not say there is no escape — {reason}"
            ),
        }
        assert!(
            reason.contains(denial.why),
            "{check}: the reason does not say why — {reason}"
        );
    }
}

/// The six escapes against the six acts that have one. Each licenses the act
/// it names and nothing else, so a session that meant to replace one field is
/// not also issuing SQL writes on the strength of the same prefix.
#[test]
fn each_escape_licenses_its_own_act_and_no_other() {
    let acts: [(Class, &str, &str, &str); 6] = [
        (
            Class::ShellTrap,
            "a shell trap",
            "git show \"$S:tools/land\"",
            guard::ESCAPE_TRAP,
        ),
        (
            Class::Record,
            "a notes replacement",
            "bd update x-1 --notes n",
            guard::ESCAPE_NOTES_REPLACE,
        ),
        (
            Class::Record,
            "a SQL write",
            "bd sql \"UPDATE issues SET a = 1\"",
            guard::ESCAPE_SQL_WRITE,
        ),
        (
            Class::Record,
            "a bare id",
            "bd note x-1 \"see a1b2\"",
            guard::ESCAPE_BARE_ID,
        ),
        (
            Class::Record,
            "an entry written by hand",
            "bd comments add x-1 '{\"fleet.entry\":1,\"kind\":\"landed\"}'",
            guard::ESCAPE_ENTRY_FORGE,
        ),
        (
            Class::ProductionWrite,
            "a production write",
            "gsutil rm gs://live.example.test/x",
            guard::ESCAPE_PROD_WRITE,
        ),
    ];
    let escapes = [
        guard::ESCAPE_TRAP,
        guard::ESCAPE_NOTES_REPLACE,
        guard::ESCAPE_SQL_WRITE,
        guard::ESCAPE_BARE_ID,
        guard::ESCAPE_ENTRY_FORGE,
        guard::ESCAPE_PROD_WRITE,
    ];

    for escape in escapes {
        for (class, act, command, owner) in acts {
            let prefixed = format!("{escape}=1 {command}");
            let licensed = refused(class, &prefixed).is_none();
            assert_eq!(
                licensed,
                escape == owner,
                "{escape} against {act}: licensed={licensed}, and only {owner} licenses it"
            );
        }

        // The seventh act has no escape at all, so NO leading assignment
        // licenses it — not the six above, and not a variable named for the act
        // itself.
        let push = format!("{escape}=1 git push origin release/1.2");
        assert!(
            refused(Class::ReleaseRef, &push).is_some(),
            "{escape} licenses no release push — the class has no escape at this layer"
        );
    }
    for invented in ["FLEET_RELEASE_OK", "FLEET_RELEASE_REF_OK", "TT_RELEASE_OK"] {
        let push = format!("{invented}=1 git push origin release/1.2");
        assert!(
            refused(Class::ReleaseRef, &push).is_some(),
            "{invented} is not an escape this layer has, and inventing one licenses nothing"
        );
    }
}

/// The spellings that leave the escape written and switched off, so a seat can
/// keep the prefix in a script without it licensing anything.
#[test]
fn the_off_spellings_of_an_escape_license_nothing() {
    for value in ["0", "false", "no", ""] {
        let command = format!("{}={value} git show \"$S:tools/land\"", guard::ESCAPE_TRAP);
        assert!(
            refused(Class::ShellTrap, &command).is_some(),
            "={value} is the switched-off spelling and licenses nothing"
        );
    }
}

// ---- the policy -------------------------------------------------------------

#[test]
fn a_class_switched_off_refuses_nothing_and_an_absent_table_leaves_it_on() {
    let off = Policy {
        enabled: false,
        ..policy()
    };
    for (class, command) in [
        (Class::ShellTrap, "git show \"$S:tools/land\""),
        (Class::Record, "bd update x-1 --notes n"),
        (Class::ReleaseRef, "git push origin release/1.2"),
        (Class::ProductionWrite, "gsutil rm gs://live.example.test/x"),
    ] {
        assert!(
            refused(class, command).is_some(),
            "{}: the case is one this class refuses while it is on",
            class.name()
        );
        assert_eq!(
            guard::judge(class, command, &off),
            Verdict::Silent,
            "{}: a class switched off refuses nothing",
            class.name()
        );
    }

    // Opt-out, never opt-in: only `false` switches a class off.
    assert!(
        guard::enabled_in(Class::ShellTrap, ""),
        "an empty policy leaves it on"
    );
    assert!(
        guard::enabled_in(
            Class::Record,
            "[guards]\nshell-trap = { enabled = false }\n"
        ),
        "another guard's row leaves this one on"
    );
    assert!(
        !guard::enabled_in(
            Class::ShellTrap,
            "[guards]\nshell-trap = { enabled = false }\n"
        ),
        "its own row switches it off"
    );
    // The two classes a pack wires read the same table on the same rules, which
    // is the whole of what "install into the same table" means here.
    for class in [Class::ReleaseRef, Class::ProductionWrite] {
        let row = format!("[guards]\n{} = {{ enabled = false }}\n", class.name());
        assert!(
            !guard::enabled_in(class, &row),
            "its own row switches it off"
        );
        assert!(
            guard::enabled_in(class, "[guards]\n"),
            "an absent row leaves it on"
        );
        assert!(
            guard::enabled_in(Class::ShellTrap, &row),
            "and switching it off leaves core's own classes alone"
        );
    }
    assert!(
        guard::enabled_in(Class::ShellTrap, "this is not toml at all ["),
        "an unreadable file leaves it on"
    );
}

#[test]
fn the_bare_id_check_refuses_nothing_until_its_target_is_configured() {
    let unconfigured = Policy {
        item_prefix: None,
        ..policy()
    };
    assert_eq!(
        guard::judge(Class::Record, "bd note x-1 \"see a1b2\"", &unconfigured),
        Verdict::Silent,
        "no item prefix, nothing to name an id by, nothing refused"
    );
    // The other two checks of the same class have no target and still refuse.
    assert!(
        matches!(
            guard::judge(Class::Record, "bd update x-1 --notes n", &unconfigured),
            Verdict::Refused(_)
        ),
        "a check with no target is always configured"
    );
    assert_eq!(
        guard::item_prefix_in("[project]\nitem_prefix = \"acme\"\n"),
        Some("acme".to_string())
    );
    assert_eq!(guard::item_prefix_in("[project]\nname = \"x\"\n"), None);

    assert_eq!(
        Class::Record.target_of("bare-id"),
        Some(guard::ITEM_PREFIX_KEY)
    );
    assert_eq!(Class::Record.target_of("sql-write"), None);
    for check in Class::ShellTrap.checks() {
        assert_eq!(
            Class::ShellTrap.target_of(check),
            None,
            "{check} has no target and is always configured"
        );
    }
}

// ---- the three surfaces a product declares ----------------------------------

#[test]
fn a_listed_build_goal_is_refused_and_a_dry_run_and_an_unlisted_goal_are_not() {
    run(
        Class::ProductionWrite,
        "make-goal",
        &[
            ("the listed goal", "make ship-it", true),
            ("wherever it is invoked from", "make -C app ship-it", true),
            ("under another spelling of the tool", "gmake ship-it", true),
            (
                "with an absolute path to the tool",
                "/usr/bin/make ship-it",
                true,
            ),
            (
                "with the goal after other goals",
                "make build ship-it",
                true,
            ),
            (
                "with a dry assignment that is not the escape",
                "make ship-it dry=0",
                true,
            ),
            // THE ESCAPE, and it is the tool's own rather than this guard's: the
            // goal prints its call and runs nothing.
            ("the dry run", "make ship-it dry=1", false),
            ("the dry run written first", "make dry=1 ship-it", false),
            // make reads the LAST assignment of a repeated variable, so this is
            // a real dry run and reading the first would refuse it.
            (
                "a dry run that overrides an earlier value",
                "make ship-it dry=0 dry=1",
                false,
            ),
            // A conditional spelling IS an assignment, so it becomes the last
            // one — but only the exact escape is the escape, because `dry?=1`
            // sets the variable just when it is unset and the goal may still
            // run for real. The reference reads it the same way, at
            // .claude/hooks/prod-write-guard.py MAKE_DRY_ASSIGNMENT.
            (
                "a conditional dry assignment is not the escape",
                "make ship-it dry?=1",
                true,
            ),
            (
                "a conditional assignment after the escape takes the last word",
                "make ship-it dry=1 dry?=0",
                true,
            ),
            ("an unlisted goal", "make build", false),
            (
                "a goal that merely contains the name",
                "make ship-it-later",
                false,
            ),
        ],
    );
}

#[test]
fn a_listed_module_function_is_refused_and_an_unlisted_one_is_not() {
    run(
        Class::ProductionWrite,
        "module-function",
        &[
            ("the listed function", "dagger call deployExecute", true),
            ("through the run verb", "dagger run deployExecute", true),
            ("named with no verb at all", "dagger deployExecute", true),
            // The tool's value-taking flags are stepped over, so the function
            // word is reached rather than read as a flag's value.
            (
                "behind a value-taking flag",
                "dagger --mod ./ci call deployExecute",
                true,
            ),
            (
                "behind the same flag written with =",
                "dagger --mod=./ci call deployExecute",
                true,
            ),
            (
                "behind a shorthand that takes a value",
                "dagger -m ./ci call deployExecute",
                true,
            ),
            // A script the tool is handed is words it will run, so a listed
            // function named inside one is named by this invocation. The
            // reference reads its own command flag the same way.
            ("named inside a script", "dagger -c 'deployExecute'", true),
            (
                "named inside a longer script",
                "dagger -c 'ci | deployExecute'",
                true,
            ),
            (
                "through the long spelling of that flag",
                "dagger --command 'deployExecute'",
                true,
            ),
            (
                "through the long spelling written with =",
                "dagger --command='deployExecute'",
                true,
            ),
            ("an unlisted function", "dagger call deployPlan", false),
            (
                "a script naming no listed function",
                "dagger -c 'ci | test'",
                false,
            ),
            (
                "a read verb naming no listed function",
                "dagger functions",
                false,
            ),
            // The flag's VALUE is not the function, which is the direction a
            // reader that failed to step over it would get wrong.
            (
                "the name appearing only as a flag value",
                "dagger --mod deployExecute functions",
                false,
            ),
        ],
    );
}

#[test]
fn a_dispatch_matching_both_halves_of_a_listed_pair_is_refused_and_the_near_misses_are_not() {
    run(
        Class::ProductionWrite,
        "workflow-ref",
        &[
            (
                "the listed pair",
                "gh workflow run pipeline.yml --ref backend/release/2.7",
                true,
            ),
            (
                "the ref written with the short flag",
                "gh workflow run pipeline.yml -r backend/release/2.7",
                true,
            ),
            (
                "the ref written with =",
                "gh workflow run pipeline.yml --ref=backend/release/2.7",
                true,
            ),
            (
                "the workflow named by path",
                "gh workflow run .github/workflows/pipeline.yml --ref backend/release/2.7",
                true,
            ),
            (
                "with a value-taking flag before the workflow",
                "gh workflow run --repo o/r pipeline.yml --ref backend/release/2.7",
                true,
            ),
            (
                "deeper under the ref glob",
                "gh workflow run pipeline.yml --ref backend/release/2.7/hotfix",
                true,
            ),
            // BOTH HALVES MATCH OR NEITHER DOES: the same workflow on another
            // ref deploys nothing, and another workflow on the release ref
            // deploys nothing either.
            (
                "the listed workflow on another ref",
                "gh workflow run pipeline.yml --ref main",
                false,
            ),
            (
                "an unlisted workflow on the listed ref",
                "gh workflow run other.yml --ref backend/release/2.7",
                false,
            ),
            (
                "a dispatch naming no ref",
                "gh workflow run pipeline.yml",
                false,
            ),
            ("a dispatch naming no workflow", "gh workflow run", false),
            (
                "a read of the same workflow",
                "gh workflow view pipeline.yml",
                false,
            ),
        ],
    );
}

/// (the check, a command it refuses, the clearing of the list it reads).
type ListCase = (&'static str, &'static str, fn(&mut Policy));

/// AC2: each of the three keys disables its own check when the project declares
/// nothing, exactly as the three cloud lists do — and the OTHER checks keep
/// refusing, which is what says the empty list disabled one check rather than
/// the class.
#[test]
fn an_undeclared_product_list_disables_its_own_check_and_no_other() {
    let cases: [ListCase; 3] = [
        ("make-goal", "make ship-it", |p| p.prod_make_goals.clear()),
        ("module-function", "dagger call deployExecute", |p| {
            p.prod_dagger_functions.clear()
        }),
        (
            "workflow-ref",
            "gh workflow run pipeline.yml --ref backend/release/2.7",
            |p| p.prod_workflow_refs.clear(),
        ),
    ];
    for (check, command, clear) in cases {
        // Declared, it refuses — the control that makes the silence below a
        // reading of the list rather than of the command.
        assert_eq!(
            refused(Class::ProductionWrite, command).map(|d| d.check),
            Some(check),
            "{check}: declared, this command is the trap — {command}"
        );

        let mut empty = policy();
        clear(&mut empty);
        assert_eq!(
            guard::judge(Class::ProductionWrite, command, &empty),
            Verdict::Silent,
            "{check}: an empty list is a target set with nothing in it"
        );
        assert!(
            !guard::configured(Class::ProductionWrite, check, &empty),
            "{check}: and the reader says so under the check flag"
        );
        // The siblings are untouched: one cleared list disables one check.
        assert!(
            refused(Class::ProductionWrite, "gsutil rm gs://live.example.test/x").is_some(),
            "{check}: clearing one list left the bucket check refusing"
        );
    }
}

// ---- the reader's own two directions ----------------------------------------

#[test]
fn text_the_reader_cannot_read_allows_the_shell_trap_class_and_denies_the_two_that_fail_closed() {
    // An unterminated quote: the reader answers that it could not read this,
    // and the classes answer that differently on purpose.
    let unreadable = "bd update x-1 --notes \"a quote that never closes";
    assert_eq!(
        guard::judge(Class::ShellTrap, unreadable, &policy()),
        Verdict::Silent,
        "the shell-trap class has no raw-text fallback and allows"
    );
    let denial =
        refused(Class::Record, unreadable).expect("the record class falls back and denies");
    assert_eq!(denial.check, "notes-replace");
    assert!(
        denial.label.contains("could not be read"),
        "the refusal says the conservative match applied — {}",
        denial.label
    );

    let sql = "bd sql \"UPDATE issues SET a = 'unterminated";
    let denial = refused(Class::Record, sql).expect("the SQL route falls back too");
    assert_eq!(denial.check, "sql-write");

    // The fallback is INVOCATION-SHAPED, so unreadable prose that merely names
    // the rule is not refused.
    assert_eq!(
        guard::judge(
            Class::Record,
            "echo \"never run bd update --notes on a record",
            &policy()
        ),
        Verdict::Silent,
        "prose naming the rule is not an invocation of it"
    );

    // The bare-id check has no fallback of its own, which is the direction it
    // fails in: with the two fail-closed checks escaped, unreadable text with a
    // bare suffix in it allows.
    assert_eq!(
        guard::judge(
            Class::Record,
            "FLEET_NOTES_REPLACE_OK=1 bd note x-1 \"see a1b2 and an unterminated quote",
            &policy()
        ),
        Verdict::Silent,
        "the bare-id check reads nothing it could not lex"
    );
    // Nor has the entry-forge check: a forged entry whose quote never closes
    // is a command the shell will not run either, and it allows.
    assert_eq!(
        guard::judge(
            Class::Record,
            "bd comments add x-1 '{\"fleet.entry\":1,\"kind\":\"landed\"}",
            &policy()
        ),
        Verdict::Silent,
        "the entry-forge check reads nothing it could not lex"
    );
}

/// The other direction of the same reading, for the release-ref class: it
/// refuses on a destination it can NAME, and a text the reader could not lex
/// names none, so it allows. It is the residual that module's header states,
/// measured rather than asserted — and with the shell-trap arm above it, this is
/// the pair that keeps the production-write fallback below from moving either of
/// them with it.
#[test]
fn text_the_reader_cannot_read_allows_the_release_ref_class() {
    let unreadable = "git push origin \"release/1.2";
    assert_eq!(
        guard::judge(Class::ReleaseRef, unreadable, &policy()),
        Verdict::Silent,
        "release-ref has no raw-text fallback, so unreadable text allows"
    );
    // The control: the same command with its quote closed is one this class
    // refuses, so the silence above is the reader's and not the case's.
    let readable = unreadable.replace('"', "");
    assert!(
        refused(Class::ReleaseRef, &readable).is_some(),
        "the closed form is the trap — {readable}"
    );
}

/// The production-write class is the one a pack wires that DOES fall back, over
/// the literal entries of the three lists. Three answers are measured here and
/// the third is the load-bearing one: a lex failure that names nothing listed
/// still allows, which is what keeps a fallback from becoming a blanket refuser.
#[test]
fn text_the_reader_cannot_read_falls_back_to_the_listed_entries_on_production_write() {
    let unreadable = format!("gsutil rm \"gs://{BUCKET}/a.png");
    let denial = refused(Class::ProductionWrite, &unreadable)
        .expect("a lex failure carrying a listed entry is refused");
    assert_eq!(denial.check, "bucket");
    assert!(
        denial.label.contains("could not be read"),
        "the refusal says the conservative match applied — {}",
        denial.label
    );
    assert!(
        denial.fragment.contains(BUCKET),
        "and names the entry it matched — {}",
        denial.fragment
    );

    // The control, one character away: with the quote closed the parsed walk
    // refuses the same command, so the fallback is not the only reading that
    // condemns it and the silence it replaced was the reader's.
    let readable = unreadable.replace('"', "");
    assert_eq!(
        refused(Class::ProductionWrite, &readable).map(|d| d.check),
        Some("bucket"),
        "the lexable form is refused by the parsed walk — {readable}"
    );

    // THE ARM THAT KEEPS THIS FROM BEING A BLANKET REFUSER.
    assert_eq!(
        guard::judge(
            Class::ProductionWrite,
            "gsutil rm \"gs://unlisted.example.test/a.png",
            &policy()
        ),
        Verdict::Silent,
        "a lex failure naming nothing on any list has no target, and allows"
    );

    // The other two lists reach the same fallback.
    for (command, check) in [
        (
            format!("gcloud sql databases delete d --project {PROJECT} \"unterminated"),
            "project",
        ),
        (format!("fly deploy --app {APP} \"unterminated"), "app"),
    ] {
        let denial = refused(Class::ProductionWrite, &command)
            .unwrap_or_else(|| panic!("the {check} list reaches the fallback — {command}"));
        assert_eq!(denial.check, check, "matched by the wrong list — {command}");
    }

    // It is invocation-shaped, so unreadable prose that merely names a listed
    // target is an argument to a command that writes nothing.
    assert_eq!(
        guard::judge(
            Class::ProductionWrite,
            &format!("echo \"never run gsutil rm gs://{BUCKET}/a.png"),
            &policy()
        ),
        Verdict::Silent,
        "prose naming the rule is not an invocation of it"
    );

    // And the escape licenses this path exactly as it licenses the parsed one:
    // a refusal a seat cannot act on is one it routes around.
    assert_eq!(
        guard::judge(
            Class::ProductionWrite,
            &format!("FLEET_PROD_WRITE_OK=1 gsutil rm \"gs://{BUCKET}/a.png"),
            &policy()
        ),
        Verdict::Silent,
        "the escape reaches the fallback too"
    );
}

#[test]
fn a_heredoc_body_is_skipped_rather_than_read_as_commands() {
    // The form every seat is told to write stored text through. Its body names
    // three of the traps and none of them is this command's.
    let command = "bd note x-1 \"$(cat <<'EOF'\n\
                   for x in $LIST; do echo hi; done\n\
                   git show \"$S:tools/land\"\n\
                   EOF\n\
                   )\"";
    assert_eq!(
        guard::judge(Class::ShellTrap, command, &policy()),
        Verdict::Silent,
        "the body of a quoted heredoc is text, not commands"
    );
}

// The decision value belongs to a provider, so the test that reads for one is
// the cli's: fleet/cli/tests/guard.rs greps the adapter module for the refusing
// value and this module for none at all.
