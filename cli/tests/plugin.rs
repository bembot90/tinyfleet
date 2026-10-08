//! `fleet prime` through the shipped binary: what a session is handed at its
//! start, read off a scratch fleet directory — the version, the pack layers
//! and the guards on line one, the store on line two, then the resolved rules
//! file and the seat's own open work.
//!
//! The plugin that runs it at a session's start is no longer this
//! repository's: the claude-code pack carries it, and its own suite holds the
//! hook wiring and the shim (fleet-jymr.5).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("the output is utf-8")
}

// ---- `fleet prime` -----------------------------------------------------------

/// A scratch tree: a working directory with no `fleet.toml` above it, a fleet
/// directory the binary is pointed at, and whatever else the case writes.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-prime-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch tree is created");
        Scratch { root }
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(&path).expect("the directory is created");
        path
    }

    /// A pack under the packs dir, at a DIRECTORY NAME that differs from the
    /// pack's own name wherever a case can afford it: the resolver reads the manifest
    /// and falls back to the directory when it will not parse, so a fixture
    /// whose two names agree cannot tell the two apart. `schema = 3` is the
    /// format's, and a manifest without it does not parse at all.
    fn pack(&self, dir: &str, name: &str, extra: &str) -> &Scratch {
        self.write(
            &format!("fleet-dir/packs/{dir}/pack.toml"),
            &format!("[pack]\nname = \"{name}\"\nschema = 3\n{extra}"),
        );
        self
    }

    /// The bottom layer this fleet resolves over: the machine directory's
    /// defaults, which an arm either materializes from the binary's own set or
    /// plants a fixture into.
    fn defaults(&self, relative: &str, contents: &str) -> &Scratch {
        self.write(
            &format!("fleet-dir/{}/{relative}", fleet_core::defaults::DIR),
            contents,
        );
        self
    }

    /// The binary's own defaults, materialized as `fleet start` materializes
    /// them, so a layering an arm builds is gated by the real registry.
    fn shipped_defaults(&self) -> PathBuf {
        let root = self.dir(&format!("fleet-dir/{}", fleet_core::defaults::DIR));
        fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
        root
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the parent is created");
        }
        std::fs::write(&path, contents).expect("the file is written");
        path
    }

    /// An executable stub, so a case can name one through a binary seam.
    fn script(&self, relative: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.write(relative, body);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the stub is made executable");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text_of(path: &Path) -> String {
    path.to_str().expect("the temp path is utf-8").to_string()
}

/// `fleet prime` from `cwd`, with the machine directory pointed at `fleet_dir`.
fn prime(cwd: &Path, fleet_dir: &Path, env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fleet"));
    cmd.arg("prime")
        .current_dir(cwd)
        .hermetic(&fleet_dir.join("home"), fleet_dir);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.output().expect("the built binary runs")
}

fn line_one(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(0), "prime always exits 0");
    utf8(out.stdout.clone())
        .lines()
        .next()
        .expect("prime prints at least one line")
        .to_string()
}

/// Line 1, line 2 and what follows them, for an arm run from a directory no
/// project claims: its line 2 is the store's third answer, and no store is
/// asked anything.
fn past_line_two(text: &str) -> (&str, &str, &str) {
    let (first, rest) = text
        .split_once('\n')
        .expect("prime printed more than a line");
    let (second, rest) = rest
        .split_once('\n')
        .expect("prime printed more than two lines");
    assert_eq!(
        second, "store: none (no project here)",
        "line 2 is the store's: {text}"
    );
    (first, second, rest)
}

/// `fleet pack add`, with the machine directory pointed at `fleet_dir` AND the
/// two paths passed explicitly. Either alone would do; both are set because an
/// arm that resolved the machine directory from the home directory would
/// install into the fleet this box actually runs.
fn pack_add(source: &str, fleet_dir: &Path, packs_dir: &Path, lock: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fleet"))
        .args(["pack", "add", source, "--version", "v1"])
        .arg("--packs-dir")
        .arg(packs_dir)
        .arg("--lock")
        .arg(lock)
        .hermetic(&fleet_dir.join("home"), fleet_dir)
        .output()
        .expect("the built binary runs")
}

/// A recursive copy, so a fixture repository can hold the real pack bytes.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination is created");
    for entry in std::fs::read_dir(from).expect("the source directory is readable") {
        let entry = entry.expect("the entry is readable");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("the kind is readable").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("the file is copied");
        }
    }
}

/// One commit tagged `v1`. The box's own git configuration is kept out, so a
/// global hooks path or a missing identity cannot decide whether the fixture
/// builds.
fn git_fixture(root: &Path) {
    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["add", "--all"],
        vec!["commit", "--quiet", "--no-gpg-sign", "-m", "the packs"],
        vec!["tag", "v1"],
    ] {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(&args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "fleet tests")
            .env("GIT_AUTHOR_EMAIL", "fleet@example.invalid")
            .env("GIT_COMMITTER_NAME", "fleet tests")
            .env("GIT_COMMITTER_EMAIL", "fleet@example.invalid")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// A machine `config.json` naming `fleet_toml`, with the seat rows given.
fn config_json(fleet_toml: &Path, children: &str) -> String {
    format!(
        "{{\"fleet_toml\": {}, \"children\": [{children}]}}",
        serde_json::Value::String(text_of(fleet_toml))
    )
}

#[test]
fn with_no_config_anywhere_prime_says_so_in_one_line() {
    let s = Scratch::new("no-config");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    assert_eq!(text.lines().count(), 1, "line 1 alone: {text}");
    let here = std::fs::canonicalize(&cwd).expect("the scratch cwd resolves");
    assert_eq!(
        text.trim_end(),
        format!(
            "fleet {} — no fleet config found above {}",
            env!("CARGO_PKG_VERSION"),
            here.display()
        )
    );
}

#[test]
fn line_one_reads_the_guards_off_the_machine_policy() {
    let s = Scratch::new("guards");
    s.shipped_defaults();
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write(
        "fleet-root/fleet.toml",
        "[guards]\nrecord.enabled = false\n",
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));

    let line = line_one(&prime(&cwd, &fleet_dir, &[]));
    assert_eq!(
        line,
        format!(
            "fleet {} — packs: none installed; guards: shell-trap on, record off",
            env!("CARGO_PKG_VERSION")
        )
    );

    // The control that makes the `off` above a reading rather than a default:
    // the same rig with nothing turned off answers the other way.
    let t = Scratch::new("guards-on");
    t.shipped_defaults();
    let on_cwd = t.dir("cwd");
    let on_dir = t.dir("fleet-dir");
    let on_toml = t.write("fleet-root/fleet.toml", "[guards]\n");
    t.write("fleet-dir/config.json", &config_json(&on_toml, ""));
    assert!(
        line_one(&prime(&on_cwd, &on_dir, &[])).ends_with("guards: shell-trap on, record on"),
        "an untouched policy leaves every class on"
    );
}

/// The guards half names the classes the layers declare: core's two, then
/// every class an installed pack's `guard_classes` turns on, in the order the
/// classes run — and one the fleet switches off still prints, as off. The
/// doctrine-shaped pack declares both of the other two, and the line names all
/// four.
///
/// RED-PROOF: on the base the line names all four whatever is installed, so
/// release-ref sits between record and production-write in the first half.
#[test]
fn line_one_names_the_classes_an_installed_pack_declares() {
    let s = Scratch::new("declared");
    s.shipped_defaults();
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write(
        "fleet-root/fleet.toml",
        "[guards]\nproduction-write.enabled = false\n",
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));
    s.pack(
        "the-opinion",
        "opinion",
        "guard_classes = [\"production-write\"]\n",
    );

    let line = line_one(&prime(&cwd, &fleet_dir, &[]));
    assert!(
        line.ends_with("guards: shell-trap on, record on, production-write off"),
        "{line}"
    );

    std::fs::remove_dir_all(fleet_dir.join("packs/the-opinion")).expect("the pack is removed");
    for name in ["tiny", "ts"] {
        copy_dir(
            &fleet_core::test_support::fixture_pack(name),
            &fleet_dir.join("packs").join(name),
        );
    }
    let line = line_one(&prime(&cwd, &fleet_dir, &[]));
    assert!(
        line.ends_with("guards: shell-trap on, record on, release-ref on, production-write off"),
        "{line}"
    );
}

#[test]
fn the_rules_file_follows_the_first_two_lines_verbatim() {
    let s = Scratch::new("rules");
    s.shipped_defaults();
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));
    s.pack("the-only-dir", "sole", "");
    let rules = "  RULE ONE, indented and unwrapped\n\nRULE TWO\n";
    s.write("fleet-dir/packs/the-only-dir/assets/rules.md", rules);

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let (first, _, rest) = past_line_two(&text);
    assert!(
        first.contains("packs: sole"),
        "line 1 names the installed pack: {first}"
    );
    assert_eq!(rest, rules, "the rules follow line 2 byte for byte");
}

/// fleet-4fw: an installed pack whose import is not installed is named on line
/// 1, with the line that adds it read off the importer's own line in the lock —
/// the session that meets it is told before any run is refused.
#[test]
fn line_one_names_an_import_that_is_not_installed_and_the_line_that_adds_it() {
    let s = Scratch::new("absent-import");
    s.shipped_defaults();
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));
    s.pack(
        "tiny",
        "tiny",
        "\n[imports.ts]\nsource = \"../ts\"\nversion = \"0.1.0\"\n",
    );
    fleet_core::lock::write(
        &fleet_dir.join(fleet_core::lock::LOCK),
        &[fleet_core::lock::Entry {
            source: "https://example.invalid/o/fleet//packs/tiny".into(),
            name: Some("tiny".into()),
            version: "v1".into(),
            commit: "0".repeat(40),
            fetched: "2026-09-23T00:00:00Z".into(),
            tree: None,
        }],
    )
    .expect("the lock is written");

    let first = line_one(&prime(&cwd, &fleet_dir, &[]));
    assert!(
        first.contains(
            "packs: tiny (`tiny` imports `ts`, which is not installed — \
             `fleet pack add https://example.invalid/o/fleet//packs/ts --version v1` adds it); \
             guards:"
        ),
        "{first}"
    );
}

/// Three claims in one fixture, each of which a plausible wrong reading gets
/// wrong: the pack NAMES come from the manifests and not the directories (the
/// directory order here is the reverse of the name order, so a listing that
/// sorted directories would print `zeta, alpha`); the bottom layer is the
/// binary's own defaults and is named on no packs line; and the copy printed is
/// the highest layer that CARRIES the path, not the highest layer, which is why
/// `alpha` sits on top holding no rules file. The defaults declare the path
/// shadowable, or the resolution refuses the shadow instead of answering it.
#[test]
fn the_highest_layer_carrying_the_rules_file_is_the_one_printed() {
    let s = Scratch::new("shadow");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));

    s.defaults(
        "assets/shadow-registry.toml",
        "schema = 1\n\n[[shadow]]\npath = \"assets/rules.md\"\npurpose = \"the every-turn rules\"\n",
    )
    .defaults("assets/rules.md", "THE UNDER COPY\n");
    s.pack("1-zeta", "zeta", "");
    s.write("fleet-dir/packs/1-zeta/assets/rules.md", "THE OVER COPY\n");
    s.pack("2-alpha", "alpha", "");

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let (first, _, rest) = past_line_two(&text);
    assert!(
        first.contains("packs: alpha, zeta"),
        "the INSTALLED layers are named top first, sorted: {first}"
    );
    assert!(
        !first.contains(fleet_core::defaults::LAYER),
        "the bottom layer is the binary's and is not a pack anybody installed: {first}"
    );
    assert_eq!(rest, "THE OVER COPY\n");
    assert!(
        !text.contains("THE UNDER COPY"),
        "the shadowed copy is not printed too: {text}"
    );
}

/// `pack add` of a pack and the pack it imports, then `prime` over what it
/// installed.
///
/// The source is a repository holding the two fixture packs — tiny, shaped as
/// the doctrine pack, and ts, the runtime pack it imports — at paths of their
/// own, COPIED from the fixture tree rather than retyped. What the arm is about
/// is fleet's: the subdirectory form, the layering check the DEFAULTS'
/// registry gates (tiny shadows four of the paths it lists), and the bytes
/// that land. Whether the real doctrine pack passes the same gate is its own
/// suite's, in the fleet-packs repository, against a pinned fleet.
///
/// ts first, then tiny — one of the orders `check_layering` accepts, since it
/// lays each add by the imports and not by the install sequence; the core
/// suite's add arms cover the tiny-before-ts order. The lock records both, and
/// the binary's defaults are materialized first so the registry rule the adds
/// pass is the shipped one.
#[test]
fn a_pack_and_its_import_install_from_a_checkout_and_prime_reads_them() {
    let s = Scratch::new("tiny-install");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let source = s.dir("source");
    s.shipped_defaults();

    const PACKS: [&str; 2] = ["ts", "tiny"];
    for name in PACKS {
        copy_dir(
            &fleet_core::test_support::fixture_pack(name),
            &source.join(name),
        );
    }
    git_fixture(&source);

    let packs_dir = fleet_dir.join("packs");
    let lock = fleet_dir.join("packs.lock");
    for name in PACKS {
        let out = pack_add(
            &format!("{}//{name}", text_of(&source)),
            &fleet_dir,
            &packs_dir,
            &lock,
        );
        assert_eq!(
            out.status.code(),
            Some(0),
            "pack add {name}: {}",
            utf8(out.stderr)
        );
        assert!(
            utf8(out.stdout).contains(&format!("added {name} v1 at")),
            "the verb names what it installed"
        );
    }
    assert!(packs_dir.join("tiny/pack.toml").is_file());
    assert!(packs_dir.join("ts/pack.toml").is_file());

    // The lock is the record: one line per pack, the ts one keyed on the source
    // as typed and naming the directory it installed.
    let pinned = fleet_core::lock::read(&lock).expect("the lock reads back");
    let names: Vec<&str> = pinned.iter().filter_map(|e| e.name.as_deref()).collect();
    assert_eq!(
        names,
        vec!["tiny", "ts"],
        "one lock line per pack, in source order"
    );
    let ts = pinned
        .iter()
        .find(|e| e.name.as_deref() == Some("ts"))
        .expect("the lock records the ts pack");
    assert!(ts.source.ends_with("//ts"), "{}", ts.source);
    assert_eq!(ts.version, "v1");

    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let (first, _, rest) = past_line_two(&text);
    assert!(
        first.contains("packs: tiny, ts"),
        "the doctrine-shaped pack sits on top and the runtime pack it imports beneath it: {first}"
    );

    // Read off the fixture, not off the installed copy: a comparison against
    // what `pack add` wrote would only prove that prime read the same file
    // twice.
    let rules_file = fleet_core::test_support::fixture_pack("tiny").join("assets/rules.md");
    let rules = std::fs::read_to_string(&rules_file)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", rules_file.display()));
    assert!(!rules.is_empty(), "the pack's rules file was read");
    assert_eq!(rest, rules, "the pack's rules follow line 2 byte for byte");
}

/// The fail-open contract, at the one place a layering can refuse: line 1
/// still prints, with the refusal in place of the names; the rules are omitted,
/// because there is no resolution to ask; the item lines still print, because
/// the store the seat's worktree names by path does not come from the packs;
/// and the exit is still 0. (A store that IS a pack's adapter is opened
/// through that layering, and its item line says the layering's refusal.)
#[test]
fn a_layering_that_refuses_costs_the_names_and_nothing_else() {
    let s = Scratch::new("cycle");
    s.shipped_defaults();
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));

    // Two packs importing each other: no order puts every pack before the ones
    // it imports, so `ordered` refuses.
    s.pack(
        "ay",
        "ay",
        "\n[imports.bee]\nsource = \"./bee\"\nversion = \"1\"\n",
    );
    s.write(
        "fleet-dir/packs/ay/assets/rules.md",
        "RULES NOBODY REACHES\n",
    );
    s.pack(
        "bee",
        "bee",
        "\n[imports.ay]\nsource = \"./ay\"\nversion = \"1\"\n",
    );
    let adapter = tracker(&s, "tracker", &answers_version("1.3.0"), &answers_rows(&[]));
    keeps_its_store_on(&s, "cwd", &adapter);

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a refused layering still exits 0"
    );
    let text = utf8(out.stdout);
    let first = text.lines().next().expect("line 1 still prints");
    assert!(
        first.contains(
            "packs: could not be resolved — the installed packs import each other in a cycle"
        ),
        "the refusal stands where the names would be: {first}"
    );
    assert!(
        first.ends_with("guards: shell-trap on, record on"),
        "and the guards are still read: core's two, which no layering declares: {first}"
    );
    assert!(
        !text.contains("RULES NOBODY REACHES"),
        "no resolution means no rules file: {text}"
    );
    assert!(
        text.lines().any(|l| l == "item: none"),
        "the item lines do not come from the packs and still print: {text}"
    );
}

/// The SECOND call that can refuse, and it refuses for a different reason at a
/// different place: `ordered` above answers fine here, and `resolve` is what
/// says no, because the upper pack replaces a path the defaults never declared
/// shadowable. Both refusals have to reach line 1, so both have an arm.
#[test]
fn a_resolution_that_refuses_reaches_line_one_too() {
    let s = Scratch::new("unlisted");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));

    // The registry lists the rules file and nothing else, so zeta's copy of the
    // rules is fine and its copy of `assets/other.md` is not.
    s.defaults(
        "assets/shadow-registry.toml",
        "schema = 1\n\n[[shadow]]\npath = \"assets/rules.md\"\npurpose = \"the every-turn rules\"\n",
    )
    .defaults("assets/rules.md", "THE UNDER COPY\n")
    .defaults("assets/other.md", "the defaults' own\n");
    s.pack("1-zeta", "zeta", "");
    s.write("fleet-dir/packs/1-zeta/assets/rules.md", "THE OVER COPY\n");
    s.write(
        "fleet-dir/packs/1-zeta/assets/other.md",
        "an unlisted shadow\n",
    );

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a refused resolution still exits 0"
    );
    let text = utf8(out.stdout);
    let first = text.lines().next().expect("line 1 still prints");
    assert!(
        first.contains(
            "packs: could not be resolved — `assets/other.md` shadows `defaults` \
             and is not listed in assets/shadow-registry.toml"
        ),
        "resolve's refusal stands where the names would be: {first}"
    );
    assert!(
        first.ends_with("guards: shell-trap on, record on"),
        "and the guards are still read: {first}"
    );
    assert!(
        !text.contains("THE OVER COPY") && !text.contains("THE UNDER COPY"),
        "a layering that would not resolve prints no rules file at all: {text}"
    );
}

/// In Orla's worktree, the item lines are the items assigned to HER ID: every
/// dispatch assigns the full id, so a listing by her name or her machine name
/// would find nothing she was given.
#[test]
fn the_item_lines_are_this_seat_s_open_work() {
    let s = Scratch::new("items");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"name\": \"Orla\", \
         \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let adapter = tracker(
        &s,
        "tracker",
        &answers_version("1.3.0"),
        &answers_rows(&[
            ("x-1", "the open one", "open"),
            ("x-2", "the moving one", "in_progress"),
            ("x-3", "the done one", "closed"),
        ]),
    );
    keeps_its_store_on(&s, "cwd", &adapter);

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let items: Vec<&str> = text.lines().filter(|l| l.starts_with("item:")).collect();
    assert_eq!(
        items,
        vec!["item: x-1 — the open one", "item: x-2 — the moving one"],
        "open and in-progress only, in the order the tracker listed them: {text}"
    );

    let listed: Vec<serde_json::Value> = calls_of(&s)
        .into_iter()
        .filter(|(verb, _)| verb == "list")
        .map(|(_, request)| request)
        .collect();
    assert_eq!(listed.len(), 1, "one listing: {listed:?}");
    assert_eq!(
        listed[0]["filter"],
        serde_json::json!({ "assignee": "01a0d1f1-0aec-765f-9abe-5c21e8a04b17" }),
        "the listing is asked for the row's full id: {}",
        listed[0]
    );
    assert_eq!(
        listed[0]["root"],
        serde_json::json!(text_of(&cwd)),
        "of the row's own project root: {}",
        listed[0]
    );
}

#[test]
fn a_seat_with_nothing_open_prints_none() {
    let s = Scratch::new("items-none");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let adapter = tracker(&s, "tracker", &answers_version("1.3.0"), &answers_rows(&[]));
    keeps_its_store_on(&s, "cwd", &adapter);

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        utf8(out.stdout).lines().any(|l| l == "item: none"),
        "an empty listing is a measured none"
    );
}

#[test]
fn a_directory_no_row_names_gets_no_item_line() {
    let s = Scratch::new("items-absent");
    let cwd = s.dir("cwd");
    let elsewhere = s.dir("elsewhere");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&elsewhere))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    assert!(
        !text.contains("item:"),
        "a row naming another directory says nothing about this one: {text}"
    );
}

#[test]
fn a_listing_that_cannot_be_read_is_its_own_answer() {
    let s = Scratch::new("items-unreadable");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let adapter = tracker(
        &s,
        "tracker",
        &answers_version("1.3.0"),
        "echo 'the store is locked' >&2; exit 4",
    );
    keeps_its_store_on(&s, "cwd", &adapter);

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0), "prime still exits 0");
    let text = utf8(out.stdout);
    assert!(
        text.contains(&format!(
            "item: could not be read — {} list exited 4, which is not a row of the store \
             contract's exit table: the store is locked",
            text_of(&adapter)
        )),
        "the third answer names what happened rather than reading as `none`: {text}"
    );
    assert!(
        !text.contains("item: none"),
        "and it is not `none`, which would tell a seat it is free: {text}"
    );
}

/// A tracker that never answers costs the item line and nothing else: prime
/// prints its third answer inside the listing's five-second bound, names that
/// bound, and still exits 0 — a session-start hook that waited on a hung store
/// would hold the session with it. The stub answers line 2's `version` at
/// once, so the bound measured is the listing's alone.
#[test]
fn a_listing_that_never_answers_is_its_own_answer_inside_the_bound() {
    let s = Scratch::new("items-hung");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let adapter = tracker(&s, "tracker", &answers_version("1.3.0"), "sleep 30");
    keeps_its_store_on(&s, "cwd", &adapter);

    let started = std::time::Instant::now();
    let out = prime(&cwd, &fleet_dir, &[]);
    let took = started.elapsed();
    assert_eq!(out.status.code(), Some(0), "prime still exits 0");
    let text = utf8(out.stdout);
    assert!(
        text.contains(&format!(
            "item: could not be read — {} list did not answer within 5s",
            text_of(&adapter)
        )),
        "the third answer names the bound the listing outran: {text}"
    );
    assert!(
        took < std::time::Duration::from_secs(7),
        "prime answered about five seconds in, not when the tracker gave up: {took:?}"
    );
}

/// A store adapter answering the contract from a `#!/bin/sh` stub at
/// `relative`: it appends its verb and its request, one line per call, to
/// [`calls_of`]'s log, answers `version` by running `version` and `list` by
/// running `list`, and every other verb as usage.
fn tracker(s: &Scratch, relative: &str, version: &str, list: &str) -> PathBuf {
    let log = s.root.join("tracker-calls");
    s.script(
        relative,
        &format!(
            "#!/bin/sh\nrequest=$(cat)\nprintf '%s %s\\n' \"$1\" \"$request\" >> '{log}'\n\
             case \"$1\" in\nversion) {version} ;;\nlist) {list} ;;\n*) exit 2 ;;\nesac\n",
            log = text_of(&log)
        ),
    )
}

/// Every call the [`tracker`] stubs of `s` were handed, as `(verb, request)`.
fn calls_of(s: &Scratch) -> Vec<(String, serde_json::Value)> {
    std::fs::read_to_string(s.root.join("tracker-calls"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let (verb, request) = line.split_once(' ').expect("a verb and its request");
            let request = serde_json::from_str(request).expect("the request is one JSON value");
            (verb.to_string(), request)
        })
        .collect()
}

/// A `version` answer naming the store `tracker` at `version`.
fn answers_version(version: &str) -> String {
    format!("echo '{{\"schema_version\":1,\"name\":\"tracker\",\"version\":\"{version}\"}}'")
}

/// A `list` answer of one row per `(id, title, status)`.
fn answers_rows(rows: &[(&str, &str, &str)]) -> String {
    let rows: Vec<String> = rows
        .iter()
        .map(|(id, title, status)| {
            format!(
                "{{\"id\":\"{id}\",\"title\":\"{title}\",\"status\":\"{status}\",\
                 \"type\":\"task\",\"labels\":[],\"order\":{{\"state\":\"none\"}}}}"
            )
        })
        .collect();
    format!(
        "echo '{{\"schema_version\":1,\"items\":[{}]}}'",
        rows.join(",")
    )
}

/// The seat's worktree at `cwd` naming `adapter` as its store, in the
/// project's own file there: the file the item line's store is opened by.
fn keeps_its_store_on(s: &Scratch, cwd: &str, adapter: &Path) {
    s.write(
        &format!("{cwd}/fleet.toml"),
        &naming_adapter(&text_of(adapter)),
    );
}

/// A pack on the machine carrying the store adapter `name`, its entry the
/// `#!/bin/sh` script `body`, over the binary's own defaults: the store a
/// project whose file names none, or names `name`, opens by name.
fn a_store_pack(s: &Scratch, name: &str, body: &str) -> PathBuf {
    s.shipped_defaults();
    s.pack("a-store-pack", "a-store-pack", "version = \"0.1.0\"\n");
    s.write(
        &format!("fleet-dir/packs/a-store-pack/adapters/store/{name}/adapter.toml"),
        &format!(
            "[adapter]\nname = \"{name}\"\nkind = \"store\"\nversion = \"0.1.0\"\nentry = \"main\"\n"
        ),
    );
    s.script(
        &format!("fleet-dir/packs/a-store-pack/adapters/store/{name}/main"),
        body,
    )
}

/// A seat's rig whose tracker answers `version` at `version` and every listing
/// with one open item, so an arm reads line 2 and the item line off one prime.
fn tracker_rig(label: &str, version: &str) -> (Scratch, PathBuf, PathBuf) {
    let s = Scratch::new(label);
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let adapter = tracker(
        &s,
        "tracker",
        &answers_version(version),
        &answers_rows(&[("x-1", "the open one", "open")]),
    );
    keeps_its_store_on(&s, "cwd", &adapter);
    (s, cwd, fleet_dir)
}

/// fleet-wpf0.3: line 2 is the store's own answer to the contract's `version`
/// and the adapter that gave it, read off the seat's worktree, the root the
/// item line reads. It COMPARES WITH NO PIN: a store at another release reads
/// the same way as one at the pin, with no pointer and no verdict, and the
/// item line still prints beneath it.
#[test]
fn line_two_is_the_store_s_own_version_and_its_adapter() {
    for (label, version, line) in [
        (
            "store-at-one",
            "1.3.0",
            "store: tracker 1.3.0 (adapter tracker)",
        ),
        (
            "store-at-another",
            "1.2.2",
            "store: tracker 1.2.2 (adapter tracker)",
        ),
    ] {
        let (_s, cwd, fleet_dir) = tracker_rig(label, version);
        let out = prime(&cwd, &fleet_dir, &[]);
        assert_eq!(out.status.code(), Some(0), "prime always exits 0");
        let text = utf8(out.stdout);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.get(1), Some(&line), "{label}: {text}");
        assert!(
            lines.contains(&"item: x-1 — the open one"),
            "{label}: the item line still prints: {text}"
        );
    }
}

/// The contract's stub adapter, at an absolute path whose file name is
/// `fleet-store-stub`: it answers `version` as `stub` at `0`, and every other
/// verb as a usage row.
fn stub_adapter(s: &Scratch, relative: &str) -> PathBuf {
    s.script(
        relative,
        "#!/bin/sh\ncat > /dev/null\n[ \"$1\" = version ] && \
         { echo '{\"schema_version\":1,\"name\":\"stub\",\"version\":\"0\"}'; exit 0; }\nexit 2\n",
    )
}

/// `[store] adapter = <path>` as a project's own file carries it.
fn naming_adapter(adapter: &str) -> String {
    format!(
        "[store]\nadapter = {}\n",
        serde_json::Value::String(adapter.to_string())
    )
}

/// fleet-wpf0.3: an adapter the project's own file names answers line 2
/// through the contract — its `version`, and the adapter by the file name of
/// its path. Both files a project names its store in are read, an embedded
/// fleet's `fleet.toml` and a standalone project's `.fleet/project.toml`, from
/// a directory under the project the walk finds it from, and no seat row is
/// needed for it: line 2 is the project's, and the item line the seat's.
#[test]
fn line_two_reads_the_adapter_the_project_names() {
    for (label, file) in [
        ("adapter-embedded", "fleet.toml"),
        ("adapter-declared", ".fleet/project.toml"),
    ] {
        let s = Scratch::new(label);
        let under = s.dir("project/src");
        let fleet_dir = s.dir("fleet-dir");
        let fleet_toml = s.write("fleet-root/fleet.toml", "");
        s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));
        let adapter = stub_adapter(&s, "adapters/fleet-store-stub");
        s.write(
            &format!("project/{file}"),
            &naming_adapter(&text_of(&adapter)),
        );

        let out = prime(&under, &fleet_dir, &[]);
        assert_eq!(out.status.code(), Some(0), "prime always exits 0");
        let text = utf8(out.stdout);
        assert_eq!(
            text.lines().nth(1),
            Some("store: stub 0 (adapter fleet-store-stub)"),
            "{label}: {text}"
        );
        assert!(
            !text.contains("item:"),
            "{label}: no row names this directory: {text}"
        );
    }
}

/// fleet-wpf0.3: a store that cannot be opened, or will not answer, is line
/// 2's own answer, and costs nothing else: prime exits 0 and the rules still
/// follow. An adapter path that is not executable and a bare name that is
/// neither form are refused by the opener before anything runs; an adapter
/// that never answers `version` is cut at line 2's two-second bound.
#[test]
fn a_store_that_cannot_be_read_is_line_two_s_own_answer() {
    let adapters = Scratch::new("adapter-inert");
    let inert = adapters.write("fleet-store-stub", "#!/bin/sh\nexit 0\n");
    let hung = adapters.script("hung", "#!/bin/sh\nsleep 30\n");
    for (label, named, line) in [
        (
            "not-executable",
            text_of(&inert),
            format!(
                "store: could not be read — [store] adapter names `{}`, which is not an \
                 executable file",
                text_of(&inert)
            ),
        ),
        (
            "neither-form",
            String::from("tools/sqlite"),
            String::from(
                "store: could not be read — [store] adapter is `tools/sqlite` — it is the name \
                 of a store adapter an installed pack carries, or an absolute path to an \
                 adapter executable",
            ),
        ),
        (
            "no-pack-carries-it",
            String::from("sqlite"),
            format!(
                "store: could not be read — no store adapter named `sqlite` in the installed \
                 packs — `fleet pack add {}//adapters/store/sqlite --version {}` installs the \
                 one fleet-packs carries",
                fleet_core::supported::PINNED_PACKS_SOURCE,
                fleet_core::supported::PINNED_PACKS
            ),
        ),
        (
            "never-answers",
            text_of(&hung),
            format!(
                "store: could not be read — {} version did not answer within 2s",
                text_of(&hung)
            ),
        ),
    ] {
        let s = Scratch::new(&format!("adapter-{label}"));
        s.shipped_defaults();
        let project = s.dir("project");
        let fleet_dir = s.dir("fleet-dir");
        s.write("project/fleet.toml", &naming_adapter(&named));

        let started = std::time::Instant::now();
        let out = prime(&project, &fleet_dir, &[]);
        let took = started.elapsed();
        assert_eq!(out.status.code(), Some(0), "{label}: prime always exits 0");
        let text = utf8(out.stdout);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.get(1), Some(&line.as_str()), "{label}: {text}");
        assert!(
            lines.len() > 2,
            "{label}: the rules still follow line 2: {text}"
        );
        assert!(
            took < std::time::Duration::from_secs(4),
            "{label}: prime answered inside line 2's bound, not when the store gave up: \
             {took:?}"
        );
    }
}

/// fleet-3krx.2: a store adapter a pack carries runs on the CONSTRUCTED child
/// PATH — the platform's own directory list over the home — and the `PATH`
/// this process inherited contributes nothing. The pack's entry execs its
/// tracker by bare name, as the bd pack's execs its runtime and its store's
/// binary, so what the entry finds is the proof of which `PATH` it ran on.
///
/// The decoy is the control: it is first on the child's inherited `PATH` and on
/// no entry of the constructed one, so an entry run on the inherited `PATH`
/// reaches it and nothing else does. The tracker's name is this arm's own, so
/// no directory the platform lists ahead of the home's `.local/bin` holds one.
///
/// RED-PROOF: with the adapter run on this process's own `PATH`, the entry
/// execs the decoy and the item line is the decoy's.
#[test]
fn with_no_seam_the_tracker_comes_off_the_constructed_child_path() {
    let s = Scratch::new("store-constructed");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    // The worktree names no store: the default name, through the packs.
    a_store_pack(
        &s,
        fleet_core::store::DEFAULT_ADAPTER,
        "#!/bin/sh\nexec fx-probe-tracker \"$@\"\n",
    );

    let home = s.dir("home");
    let stub = tracker(
        &s,
        "home/.local/bin/fx-probe-tracker",
        &answers_version("1.3.0"),
        &answers_rows(&[("fxitem-stub9", "the constructed path", "open")]),
    );
    let decoy = tracker(
        &s,
        "decoy/fx-probe-tracker",
        &answers_version("1.3.0"),
        &answers_rows(&[("fxitem-decoy9", "the inherited PATH", "open")]),
    );
    let decoy_dir = decoy.parent().expect("the decoy sits in a directory");

    let child_path = fleet_controller::platform::child_path(&home);
    assert!(
        !std::env::split_paths(&child_path).any(|dir| dir == decoy_dir),
        "the decoy is on no entry of the constructed child PATH {child_path}"
    );
    assert_eq!(
        fleet_core::process::resolve_on_path(&child_path, "fx-probe-tracker"),
        Some(stub.clone()),
        "the constructed child PATH names the stub"
    );

    // `HOME` and `PATH` are set on the CHILD, so no arm in this binary shares
    // them and no lock is owed.
    let ahead = match std::env::var("PATH") {
        Ok(rest) if !rest.is_empty() => format!("{}:{rest}", text_of(decoy_dir)),
        _ => text_of(decoy_dir),
    };
    let out = prime(
        &cwd,
        &fleet_dir,
        &[(common::hermetic::HOME, &text_of(&home)), ("PATH", &ahead)],
    );
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let items: Vec<&str> = text.lines().filter(|l| l.starts_with("item:")).collect();
    assert_eq!(
        items,
        vec!["item: fxitem-stub9 — the constructed path"],
        "the entry found the tracker the constructed child PATH names, and never the decoy \
         the inherited PATH puts first: {text}"
    );
}

/// A machine row names a worktree ROOT, and the match is on that directory
/// itself: a session in a subdirectory of it gets no item line at all rather
/// than a wrong seat's (D3 of the prime spec — a cwd names a seat and proves
/// nothing, so the weakest reading is the one taken).
///
/// The store is the default name's, a pack's adapter the worktree's file need
/// not name, and its call log is the witness that it was asked nothing: no row
/// names the subdirectory and no project file sits above it, so line 2 has no
/// store to ask either. The same fixture run from the root is the control:
/// there the item line appears and the listing is asked of the root.
#[test]
fn a_session_under_a_seat_s_worktree_gets_no_item_line() {
    let s = Scratch::new("items-descendant");
    let root = s.dir("worktree");
    let under = s.dir("worktree/backend/src");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&root))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let tracker_bin = tracker(
        &s,
        "the-tracker",
        &answers_version("1.3.0"),
        &answers_rows(&[("x-9", "the descendant one", "open")]),
    );
    a_store_pack(
        &s,
        fleet_core::store::DEFAULT_ADAPTER,
        &format!("#!/bin/sh\nexec '{}' \"$@\"\n", text_of(&tracker_bin)),
    );

    let out = prime(&under, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0), "prime always exits 0");
    let text = utf8(out.stdout);
    let items: Vec<&str> = text.lines().filter(|l| l.starts_with("item:")).collect();
    assert!(
        items.is_empty(),
        "a row naming the worktree root says nothing about a directory under it, \
         and it printed {} line(s): {text}",
        items.len()
    );
    assert_eq!(
        text.lines().nth(1),
        Some("store: none (no project here)"),
        "{text}"
    );
    assert!(
        calls_of(&s).is_empty(),
        "the tracker was asked nothing, and it recorded: {:?}",
        calls_of(&s)
    );

    let out = prime(&root, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let items: Vec<&str> = text.lines().filter(|l| l.starts_with("item:")).collect();
    assert_eq!(
        items,
        vec!["item: x-9 — the descendant one"],
        "the same fixture run from the root itself does get the line: {text}"
    );
    let listed: Vec<serde_json::Value> = calls_of(&s)
        .into_iter()
        .filter(|(verb, _)| verb == "list")
        .map(|(_, request)| request)
        .collect();
    assert_eq!(
        listed
            .iter()
            .map(|request| request["root"].clone())
            .collect::<Vec<_>>(),
        vec![serde_json::json!(text_of(&root))],
        "and it is asked of the row's own root, never of the subdirectory"
    );
}
