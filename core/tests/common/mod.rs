#![allow(dead_code)]
//! Fixture packs, built on disk and removed when the test ends.
//!
//! A pack is a folder, so every rule about one is asserted against a real
//! folder rather than against a model of it.

pub mod board;
pub mod capped;
pub mod holding;

pub use board::bd_init_server_args;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_core::store::Store;
// The applying fake store and the board over it are the CRATE's now, under its
// `test-support` feature, so a dependent's suite can drive core's verbs on them
// too. Every arm that wants them takes them from there.
use fleet_core::test_support::{copy_tree, Board};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Fixture {
    pub root: PathBuf,
    /// A shared fixture is a template other arms are reading right now, so its
    /// drop leaves the directory standing.
    owned: bool,
}

impl Fixture {
    /// The directory name carries the process id, so two test binaries running
    /// at once never share one.
    pub fn new(label: &str) -> Fixture {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-pack-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the fixture root is created");
        Fixture { root, owned: true }
    }

    fn shared(root: PathBuf) -> Fixture {
        Fixture { root, owned: false }
    }

    pub fn file(&self, relative: &str, contents: &str) -> &Fixture {
        write_at(&self.root, relative, contents);
        self
    }

    pub fn dir(&self, relative: &str) -> &Fixture {
        std::fs::create_dir_all(self.root.join(relative)).expect("the fixture dir is created");
        self
    }

    pub fn manifest(&self, name: &str) -> &Fixture {
        self.file(
            "pack.toml",
            &format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n"),
        )
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// The binary's own defaults written under this fixture, as `fleet start`
    /// writes them into a machine directory: the same function, so an arm
    /// resolves through the set the verbs resolve through.
    pub fn materialize_defaults(&self) -> PathBuf {
        materialize(&self.root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// The stamp the pack verbs are handed, so an arm reads no clock.
pub const WHEN: &str = "2026-09-08T13:45:00Z";

/// git inside a fixture, with the box's own configuration out of the way: a
/// global `hooksPath`, a signing key or a missing identity would otherwise
/// decide whether these repositories can be built at all.
pub fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
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
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn write_at(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the fixture's parent is created");
    }
    std::fs::write(&path, contents).expect("the fixture file is written");
}

/// FNV-1a, which is what both halves of a template's address are hashed with:
/// an address has to name the same directory in the next process and in the
/// next run, and `DefaultHasher` promises neither.
fn fnv1a(chunks: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for chunk in chunks {
        // Each chunk is closed with a NUL, so no two chunk boundaries collide.
        for b in chunk.iter().chain(std::iter::once(&0u8)) {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Half a template's address: the inputs the repository is built FROM.
fn template_key(files: &[(&str, &str)]) -> String {
    let mut chunks: Vec<&[u8]> = Vec::with_capacity(files.len() * 2);
    for (relative, body) in files {
        chunks.push(relative.as_bytes());
        chunks.push(body.as_bytes());
    }
    fnv1a(&chunks)
}

/// The recipe's own source: every step `template` takes lives in this file, so
/// this is the text that decides what a built template looks like.
pub const RECIPE: &str = include_str!("mod.rs");

/// The other half of a template's address: the STEPS the repository is built
/// by. A file set alone cannot tell a template built with one initial branch,
/// tag or commit message from one built with another, so the store a template
/// stands in is named by this, and an edit to any step names a different store.
///
/// Derived rather than typed, which is the whole point: a hand-kept number is
/// a guard nothing forces to move. Over-broad by construction — an edit
/// anywhere in this file re-keys the store, not only an edit to a build step —
/// and that is the safe direction for a cache key, at the price of one rebuild.
pub fn recipe_key(source: &str) -> String {
    fnv1a(&[source.as_bytes()])
}

/// The templates stand beside the fixtures, on the temp volume: a source the
/// verbs clone out of is read far more often than it is built, and the volume
/// this checkout sits on is a different disk.
pub fn templates() -> PathBuf {
    std::env::temp_dir().join(format!("fleet-repo-templates-{}", recipe_key(RECIPE)))
}

/// How long a store beside ours may go untouched before it is cold.
const COLD_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Once per process: the store is made, its stamp refreshed, and the cold
/// stores of superseded recipes removed. Without this a box collects one store
/// per recipe the tree has ever held, for ever, because nothing else ever
/// removes a template.
fn open_store() -> &'static PathBuf {
    static STORE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        let store = templates();
        let _ = std::fs::create_dir_all(&store);
        let _ = std::fs::write(store.join("used"), b"");
        sweep_cold_templates(&std::env::temp_dir(), &store);
        store
    })
}

/// Removes every `fleet-repo-templates-<key>` directory beside `keep` whose
/// `used` stamp is older than `COLD_AFTER`. A store with NO stamp was written
/// by code that predates it and cannot be shown unused, so it is kept — the
/// same law as `alive`: nothing is removed blind. A stamp is refreshed once per
/// process, so a store any run has touched today stands.
fn sweep_cold_templates(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path == keep
            || !entry
                .file_name()
                .to_string_lossy()
                .starts_with("fleet-repo-templates-")
            || !entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
        {
            continue;
        }
        let cold = std::fs::metadata(path.join("used"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|age| age > COLD_AFTER)
            .unwrap_or(false);
        if cold {
            remove_store(&path);
        }
    }
}

/// A template's tree is sealed read-only and a sealed directory's entries
/// cannot be unlinked, so the modes come off first. Nothing here panics: a
/// sweep that cannot read a neighbour leaves it standing.
fn remove_store(store: &Path) {
    fn open_up(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                open_up(&entry.path());
            } else {
                let _ =
                    std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o644));
            }
        }
    }
    open_up(store);
    let _ = std::fs::remove_dir_all(store);
}

/// A shared template loses its write bits before it is ever visible, so an arm
/// that writes to one fails at its own write instead of at some later arm in
/// the same binary, which is a failure no filter can reproduce. Directories go
/// after what they hold, because a directory that cannot be written cannot have
/// its entries chmodded either.
fn seal(root: &Path) {
    for entry in std::fs::read_dir(root).expect("the template is walked") {
        let entry = entry.expect("the template's entry is read");
        if entry
            .file_type()
            .expect("the entry's kind is read")
            .is_dir()
        {
            seal(&entry.path());
        } else {
            mode(&entry.path(), 0o444);
        }
    }
    mode(root, 0o555);
}

/// The other direction, for the loser of a build race: what it sealed it has to
/// be able to delete again. Directories go first here, for the same reason.
fn unseal(root: &Path) {
    mode(root, 0o755);
    for entry in std::fs::read_dir(root).expect("the template is walked") {
        let entry = entry.expect("the template's entry is read");
        if entry
            .file_type()
            .expect("the entry's kind is read")
            .is_dir()
        {
            unseal(&entry.path());
        } else {
            mode(&entry.path(), 0o644);
        }
    }
}

fn mode(path: &Path, bits: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))
        .unwrap_or_else(|e| panic!("the template's mode is set on {}: {e}", path.display()));
}

fn built_template(home: &Path) -> Option<(PathBuf, String)> {
    let sha = std::fs::read_to_string(home.join("sha")).ok()?;
    Some((home.join("repo"), sha))
}

/// The repository for one file set, built once and left standing for whoever
/// asks next. It is built under a staging name and renamed into place, so a
/// process that loses the race meets a directory that is already whole; a
/// rename onto a directory that holds something fails, which is how the loss
/// is read.
fn template(label: &str, files: &[(&str, &str)]) -> (PathBuf, String) {
    let store = open_store();
    let home = store.join(template_key(files));
    if let Some(built) = built_template(&home) {
        return built;
    }

    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let staging = store.join(format!(".building-{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let root = staging.join("repo");
    std::fs::create_dir_all(&root).expect("the template root is created");
    for (relative, body) in files {
        write_at(&root, relative, body);
    }
    git(&root, &["init", "--quiet", "-b", "main"]);
    let mut staged = vec!["add", "--"];
    staged.extend(files.iter().map(|(relative, _)| *relative));
    git(&root, &staged);
    git(
        &root,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "the pack"],
    );
    git(&root, &["tag", "v1"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    let sha = String::from_utf8_lossy(&head.stdout).trim().to_string();
    assert_eq!(sha.len(), 40, "a commit is 40 hex: {sha}");
    std::fs::write(staging.join("sha"), &sha).expect("the template's commit is recorded");
    seal(&root);

    if std::fs::rename(&staging, &home).is_err() {
        unseal(&root);
        let _ = std::fs::remove_dir_all(&staging);
    }
    built_template(&home).expect("a template stands at the key after the rename")
}

/// A repository holding the files given, committed on `main` and tagged `v1`.
/// The paths are staged one by one — the fixture knows every file it wrote, so
/// nothing else can be swept in.
///
/// Every arm asking for the same files is handed the SAME repository, and the
/// fixture that carries it does not delete it: nextest runs one process per
/// arm, so a cache held in this process would be a cache of one. Sharing is
/// safe because no arm writes to a source repository — each hands its path to
/// a verb that clones out of it — and the template is sealed read-only, so an
/// arm that ever does write to one is refused at the write and takes
/// `Fixture::new` to build its own.
pub fn repo(label: &str, files: &[(&str, &str)]) -> (Fixture, String) {
    let (root, sha) = template(label, files);
    (Fixture::shared(root), sha)
}

pub fn manifest(name: &str) -> String {
    format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n")
}

/// A pack whose root is the repository root: the manifest and one skill.
pub fn a_pack(label: &str, name: &str) -> (Fixture, String) {
    repo(
        label,
        &[
            ("pack.toml", &manifest(name)),
            ("skills/greet/SKILL.md", "# greet\n"),
        ],
    )
}

/// A temporary machine directory: the packs dir and the lock the verbs write.
pub struct Machine {
    pub fixture: Fixture,
}

impl Machine {
    pub fn new(label: &str) -> Machine {
        let machine = Machine {
            fixture: Fixture::new(label),
        };
        machine.fixture.materialize_defaults();
        machine
    }
    pub fn packs(&self) -> PathBuf {
        self.fixture.path("packs")
    }
    /// The bottom layer every add resolves over: this machine's own copy of the
    /// binary's defaults, which an arm may plant into.
    pub fn defaults(&self) -> PathBuf {
        self.fixture.path(fleet_core::defaults::DIR)
    }
    /// One file written into the bottom layer, for an arm whose subject is what
    /// a pack above it may replace.
    pub fn plant(&self, relative: &str, body: &str) -> &Machine {
        self.fixture
            .file(&format!("{}/{relative}", fleet_core::defaults::DIR), body);
        self
    }
    pub fn lock(&self) -> PathBuf {
        self.fixture.path(fleet_core::lock::LOCK)
    }
    /// What the packs directory holds, so an arm can say that a refusal left
    /// nothing behind — the scratch directory of a fetch in flight included.
    pub fn installed(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.packs())
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

pub fn source_of(fixture: &Fixture) -> String {
    fixture.root.display().to_string()
}

/// The binary's own defaults, written under `machine_dir` as `fleet start`
/// writes them, and answered as the path the resolver's bottom layer takes.
///
/// It goes through [`fleet_core::embedded::write_all`] — the function the
/// lifecycle's install calls — so an arm reads the set this binary carries and
/// never a folder in the worktree.
pub fn materialize(machine_dir: &Path) -> PathBuf {
    let root = machine_dir.join(fleet_core::defaults::DIR);
    std::fs::create_dir_all(&root).expect("the defaults dir is created");
    fleet_core::embedded::write_all(&root).expect("the embedded defaults are written");
    root
}

/// The binary's defaults on disk for one arm, for a subject that is the SET
/// rather than a machine directory holding it.
///
/// OWNED AND DROPPED, like [`Fixture`], and not a process-wide `OnceLock`: a
/// static holding a `PathBuf` never runs a destructor, so a shared tree is one
/// 19-file directory left under the temp directory per test process, for ever.
/// Nothing writes to it, so per-arm costs a copy and no correctness.
pub struct Defaults {
    fixture: Fixture,
    root: PathBuf,
}

impl Defaults {
    pub fn new(label: &str) -> Defaults {
        let fixture = Fixture::new(label);
        let root = fixture.materialize_defaults();
        Defaults { fixture, root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// The fixture the set sits under, for an arm that wants a packs directory
    /// beside it.
    pub fn beside(&self) -> &Fixture {
        &self.fixture
    }
}

impl AsRef<Path> for Defaults {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

/// The doctrine pack, read from the shipped folder the same way.
pub fn bundled_tiny() -> PathBuf {
    workspace().join("packs/tiny")
}

/// The TypeScript layer, the one shipped pack that pins a runtime.
pub fn bundled_ts() -> PathBuf {
    workspace().join("packs/ts")
}

pub fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the core crate sits inside the workspace")
        .to_path_buf()
}

/// A real work graph in a temporary directory.
///
/// The store is `bd` on this box against its own database, so every rule about
/// what a read answers is asserted against the store the verbs actually talk
/// to. It is a REAL init and not a hand-built directory: `bd init` writes a
/// database, a git repository and its own files, and a lookalike would agree
/// with it until the day one of them changed.
///
/// UNDER A RUN the init is the run's, made once by
/// `fleet/tools/dolt-test-server` and copied in here — the same files, and the
/// rows on the run's one database. The rigs that cannot share those rows are
/// the `SOLO` table in `common/board.rs`. With no run, or with a board the
/// wrapper did not make, this runs the init itself, so one binary under a bare
/// `cargo test` still comes up.
pub struct Scratch {
    pub root: PathBuf,
    pub packs_dir: PathBuf,
    pub defaults_dir: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-store-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch root is created");
        match board::run_board(label) {
            Some(made) => copy_tree(&made, &root),
            None => bd_init(&root, label),
        }
        Scratch::around(root)
    }

    /// A board of its own and never the run's shared one: a `bd init` here,
    /// on the run's server where there is one.
    ///
    /// For the reading only an EMPTY store answers — the contract's first
    /// check lists a store nothing has written to — which a copy of the run's
    /// shared board, holding every other rig's rows, cannot give.
    pub fn fresh(label: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-store-{label}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the scratch root is created");
        bd_init(&root, label);
        Scratch::around(root)
    }

    /// The packs and defaults directories beside an initialised store.
    fn around(root: PathBuf) -> Scratch {
        let packs_dir = root.join("packs");
        std::fs::create_dir_all(&packs_dir).expect("the packs dir is created");
        let defaults_dir = materialize(&root);
        Scratch {
            root,
            packs_dir,
            defaults_dir,
        }
    }

    /// A copy of the shipped pack under this scratch's packs directory, so a
    /// shadow is planted over something that resolved a moment ago.
    pub fn install(&self, name: &str, from: &Path) -> PathBuf {
        let into = self.packs_dir.join(name);
        copy_tree(from, &into);
        into
    }

    pub fn fleet_toml(&self, body: &str) -> &Scratch {
        std::fs::write(self.root.join("fleet.toml"), body).expect("the policy file is written");
        self
    }

    pub fn bd(&self, args: &[&str]) -> std::process::Output {
        std::process::Command::new("bd")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .expect("bd runs")
    }

    /// One item, by title, answered as its id.
    pub fn item(&self, title: &str) -> String {
        let out = self.bd(&[
            "create",
            "--title",
            title,
            "--description",
            "a scratch item",
            "--type",
            "task",
            "--json",
        ]);
        assert!(
            out.status.success(),
            "bd create: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout);
        let value: serde_json::Value =
            serde_json::from_str(text.trim()).expect("bd create answers JSON");
        value
            .get("id")
            .and_then(|id| id.as_str())
            .expect("the created item has an id")
            .to_string()
    }

    /// The whole document, as text: what an arm compares before and after. A
    /// failed read panics, so two failures never compare equal.
    pub fn json(&self, item: &str) -> String {
        let out = self.bd(&["show", item, "--json"]);
        assert!(
            out.status.success(),
            "bd show {item}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// One `bd init` in `root`, on the run's server where there is one.
fn bd_init(root: &Path, label: &str) {
    let out = std::process::Command::new("bd")
        .args(["init", "--prefix", "fx", "--quiet"])
        .args(bd_init_server_args(label))
        .current_dir(root)
        .output()
        .expect("bd is on the process PATH");
    assert!(
        out.status.success(),
        "bd init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The one store a whole test binary shares.
///
/// `bd` serialises against itself on this box — eight arms driving eight
/// SEPARATE stores in parallel measured slower than eight driving one, a
/// reading taken 2026-09-08 and not retaken since — so a store per arm buys
/// isolation at a price and no speed. Each arm takes its own item out of this
/// one instead.
///
/// It outlives the process: a shared handle has no owner to drop it, so the
/// directory is left under the system temp directory. The name carries the
/// pid, so a second run never reads this one's, and every binary's first call
/// sweeps the stores whose pid is no longer a live process.
pub fn shared_store(label: &'static str) -> &'static Scratch {
    static STORE: std::sync::OnceLock<Scratch> = std::sync::OnceLock::new();
    STORE.get_or_init(|| {
        sweep_dead_stores(&std::env::temp_dir());
        Scratch::new(label)
    })
}

/// Removes every `fleet-store-<label>-<pid>-<n>` directory directly under
/// `root` whose pid is not a live process. A live run's store, another suite's
/// on this box included, is kept; no other name is touched.
pub fn sweep_dead_stores(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let Some(pid) = store_pid(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && !alive(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn store_pid(name: &str) -> Option<u32> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut parts = name.strip_prefix("fleet-store-")?.rsplitn(3, '-');
    let (n, pid, label) = (parts.next()?, parts.next()?, parts.next()?);
    if label.is_empty() || !digits(n) || !digits(pid) {
        return None;
    }
    pid.parse().ok()
}

/// `kill -0` sends no signal: its exit status alone says whether the pid is
/// live. A kill that cannot run answers alive, so nothing is removed blind.
///
/// Both streams are nulled: a child left holding the test's own output pipe
/// past the test's exit is what nextest reports as LEAK.
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(true)
}

/// The stream, recorded rather than written.
///
/// The four verbs' suites share this one: an arm asserts what was appended, and
/// every arm about a refusal asserts the COUNT, because "appended nothing" is
/// the half of the write order that a stub answering only the last event could
/// not tell from "appended something else".
#[derive(Default)]
pub struct StubEvents {
    appended: std::sync::Mutex<Vec<(String, String, serde_json::Value)>>,
    /// Answered instead of an append, where an arm is about the stream refusing.
    pub refuse: Option<String>,
}

impl StubEvents {
    pub fn refusing(why: &str) -> StubEvents {
        StubEvents {
            appended: std::sync::Mutex::new(Vec::new()),
            refuse: Some(why.to_string()),
        }
    }

    pub fn last(&self) -> Option<(String, String, serde_json::Value)> {
        self.appended.lock().expect("not poisoned").last().cloned()
    }

    pub fn all(&self) -> Vec<(String, String, serde_json::Value)> {
        self.appended.lock().expect("not poisoned").clone()
    }

    pub fn count(&self) -> usize {
        self.appended.lock().expect("not poisoned").len()
    }

    /// The one event of this kind, and a panic where there is not exactly one.
    pub fn one(&self, kind: &str) -> (String, serde_json::Value) {
        let mut found: Vec<(String, serde_json::Value)> = self
            .all()
            .into_iter()
            .filter(|(seen, _, _)| seen == kind)
            .map(|(_, actor, payload)| (actor, payload))
            .collect();
        assert_eq!(found.len(), 1, "exactly one {kind} was appended");
        found.remove(0)
    }
}

impl fleet_core::item::Events for StubEvents {
    fn append(
        &self,
        kind: &str,
        actor: &fleet_core::seat::actor::Actor,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        if let Some(why) = &self.refuse {
            return Err(why.clone());
        }
        self.appended.lock().expect("not poisoned").push((
            kind.to_string(),
            actor.to_string(),
            payload,
        ));
        Ok(())
    }
}

/// Another writer's order index, of the kind a board fleet is added to may
/// already carry: a bare `orders` in a shape fleet never wrote, whose `seat` is
/// a seat name a misreading would route work to.
pub const FOREIGN_ORDERS: &str =
    r#"{"orders":{"seat":"another-tools-seat","by":7,"kind":["theirs",{"nested":true}]}}"#;

/// Another writer's label, the bare word fleet's run label once was.
pub const FOREIGN_LABEL: &str = "run";

/// The item's bare `orders` key and its labels, as the store answers them: what
/// an arm holds byte-identical across a verb that must neither read nor move
/// them.
pub fn foreign_of(store: &dyn Store, item: &str) -> (String, Vec<String>) {
    let read = store.show(item).expect("the item reads");
    let document: serde_json::Value =
        serde_json::from_str(read.proof.as_str()).expect("the document is JSON");
    (document["metadata"]["orders"].to_string(), read.labels)
}

/// The payload's keys, sorted, against the table core declares for that kind.
///
/// The assertion every verb's arm makes about its own event: a key the verb
/// invented and a key the table names and the verb dropped are the same defect,
/// and only a set comparison catches both.
pub fn keys_agree(kind: &str, payload: &serde_json::Value, absent: &[&str]) {
    let declared: Vec<&str> = fleet_core::item::payload_keys(kind)
        .unwrap_or_else(|| panic!("{kind} is one of the item kinds"))
        .iter()
        .copied()
        .filter(|key| !absent.contains(key))
        .collect();
    let mut written: Vec<&str> = payload
        .as_object()
        .expect("the payload is an object")
        .keys()
        .map(String::as_str)
        .collect();
    let mut wanted = declared;
    written.sort_unstable();
    wanted.sort_unstable();
    assert_eq!(written, wanted, "{kind}'s payload keys");
}

/// The entry signals the verb appended, in order, as `(actor, payload)`: every
/// one of them `item.entry`, carrying the table's keys and nothing else.
pub fn signals(events: &StubEvents) -> Vec<(String, serde_json::Value)> {
    let signals: Vec<(String, serde_json::Value)> = events
        .all()
        .into_iter()
        .filter(|(kind, _, _)| kind == fleet_core::item::ITEM_ENTRY)
        .map(|(_, actor, payload)| (actor, payload))
        .collect();
    for (_, payload) in &signals {
        keys_agree(fleet_core::item::ITEM_ENTRY, payload, &[]);
    }
    signals
}

/// The one signal an entry's writer appends: `item.entry` by `by`, naming the
/// item, the entry the timeline holds and its kind.
pub fn signal(item: &str, entry: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({ "item": item, "entry": entry, "kind": kind })
}

// ---- seats --------------------------------------------------------------------

/// The id an arm's seat is keyed by, derived from its name alone: FNV-1a over
/// the name, in the node's twelve digits.
///
/// A seat-holds-an-item read is a query across a whole store, and one store is
/// shared across a binary's arms, so two arms' seats may never share an id —
/// and each arm already takes a name of its own.
pub fn seat_id(name: &str) -> fleet_core::seat::identity::SeatId {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    fleet_core::seat::identity::SeatId::parse(&format!(
        "01a0d1f1-0aec-765f-9abe-{:012x}",
        hash & 0xffff_ffff_ffff
    ))
    .expect("an arm's seat id parses")
}

/// That id as the record writes it: the assignee and `fleet.orders.seat`.
pub fn full(name: &str) -> String {
    seat_id(name).to_string()
}

/// The agent seat an arm names.
pub fn agent(name: &str) -> fleet_core::seat::identity::SeatRef {
    fleet_core::seat::identity::SeatRef {
        id: seat_id(name),
        name: Some(name.to_string()),
        kind: fleet_core::seat::identity::Kind::Agent,
    }
}

/// The typed actor an arm's seat acts as: `seat:<its full id>`, which is what
/// every write and every event the seat makes carries.
pub fn seat_actor(name: &str) -> fleet_core::seat::actor::Actor {
    fleet_core::seat::actor::Actor::seat(seat_id(name))
}

/// The order a dispatch gives: by the actor, to the seat — a seat's full id,
/// or none for a transient dispatch not yet answered — at the stamp, each as
/// its text.
pub fn a_dispatch(by: &str, seat: Option<&str>, at: &str) -> fleet_core::store::Order {
    fleet_core::store::Order {
        kind: fleet_core::store::OrderKind::Dispatch,
        by: fleet_core::seat::actor::Actor::typed(by)
            .expect("a typed actor")
            .unwrap_or_else(|e| panic!("{by} is an actor: {e}")),
        seat: seat.map(|seat| {
            fleet_core::seat::identity::SeatId::parse(seat)
                .unwrap_or_else(|e| panic!("{seat} is a seat id: {e}"))
        }),
        at: fleet_core::store::Stamp::parse(at).unwrap_or_else(|| panic!("{at} is a stamp")),
    }
}

/// A fleet of agent seats by name, each listed and running here.
pub fn fleet_of(names: &[&str]) -> fleet_core::seat::identity::Directory {
    let running: Vec<_> = names.iter().map(|name| agent(name)).collect();
    fleet_core::seat::identity::Directory {
        listed: running.clone(),
        running,
    }
}

// ---- entries ------------------------------------------------------------------

/// A delivery that keeps every rule its kind has, at `commit`: the one sample
/// the store suites append, and a short `commit` is the one that does not.
pub fn a_delivery(commit: &str) -> fleet_core::entry::Body {
    use fleet_core::entry::{Body, Delivered, NotProven, Ran, SuiteRun};
    Body::Delivered(Delivered {
        commit: commit.to_string(),
        branch: String::from("work/a-seat"),
        base: String::from("0123456789abcdef0123456789abcdef01234567"),
        files: vec![String::from("core/src/store.rs")],
        checks: Vec::new(),
        suite: SuiteRun::Ran(Ran {
            command: String::from("cargo nextest run -p fleet-core"),
            rc: 0,
        }),
        spec_corrections: Vec::new(),
        not_proven: vec![NotProven {
            surface: String::from("a store under load"),
            command: String::from("fleet item show fx-1"),
        }],
        decisions: Vec::new(),
        covers: Vec::new(),
    })
}

/// A whole sha, for [`a_delivery`] to name.
pub const A_COMMIT: &str = "1111111111111111111111111111111111111111";

/// The work graph a rig runs against: held in memory, or `bd` on a scratch
/// board.
///
/// ONE TYPE FOR BOTH, so the one arm per suite that drives `bd` — the
/// integration ring — runs through the same rig as every arm around it, and
/// the two cannot drift apart unseen.
pub enum Graph {
    /// Boxed because a board carries a whole store, and an enum is as big as
    /// its widest variant wherever it is held.
    Memory(Box<Board>),
    Real(&'static Scratch, fleet_core::store::bd::Bd),
}

impl Graph {
    pub fn memory(label: &str) -> Graph {
        Graph::Memory(Box::new(Board::new(label)))
    }

    /// The one `bd` board this binary drives, taken by its ring arm.
    pub fn real(label: &'static str) -> Graph {
        let scratch = shared_store(label);
        Graph::Real(scratch, fleet_core::store::bd::Bd::at(&scratch.root))
    }

    pub fn store(&self) -> &dyn Store {
        match self {
            Graph::Memory(board) => &board.store,
            Graph::Real(_, bd) => bd,
        }
    }

    pub fn item(&self, title: &str) -> String {
        match self {
            Graph::Memory(board) => board.item(title),
            Graph::Real(scratch, _) => scratch.item(title),
        }
    }

    /// The whole document, as text: what an arm compares before and after.
    pub fn json(&self, item: &str) -> String {
        match self {
            Graph::Memory(board) => board.json(item),
            Graph::Real(scratch, _) => scratch.json(item),
        }
    }

    pub fn install(&self, name: &str, from: &Path) -> PathBuf {
        match self {
            Graph::Memory(board) => board.install(name, from),
            Graph::Real(scratch, _) => scratch.install(name, from),
        }
    }

    pub fn fleet_toml(&self, body: &str) {
        match self {
            Graph::Memory(board) => {
                board.fleet_toml(body);
            }
            Graph::Real(scratch, _) => {
                scratch.fleet_toml(body);
            }
        }
    }

    /// The item handed to a seat, by its full id, as a rig's own setup.
    pub fn hand_to(&self, item: &str, seat: &str) {
        let seat = fleet_core::seat::identity::SeatId::parse(seat)
            .unwrap_or_else(|e| panic!("a rig hands {item} to a seat: {e}"));
        self.store()
            .update(
                &fleet_core::store::ItemId::from(item),
                &fleet_core::store::Update::assignee(seat),
                &fleet_core::test_support::the_test(),
            )
            .unwrap_or_else(|e| panic!("hand {item} to {seat}: {e}"));
    }

    /// The item's order, written as a dispatch writes one, as a rig's own
    /// setup.
    pub fn order(&self, item: &str, order: &fleet_core::store::Order) {
        self.store()
            .order_set(
                &fleet_core::store::ItemId::from(item),
                order,
                &fleet_core::test_support::the_test(),
            )
            .unwrap_or_else(|e| panic!("the order on {item}: {e}"));
    }

    /// A metadata object merged onto the item as ANOTHER WRITER leaves one —
    /// a key fleet does not own, or one of fleet's own at a shape no fleet
    /// writer here makes. The trait writes only the contract's types, so the
    /// real half writes it through the binary, under a writer that is no
    /// fleet actor.
    pub fn set_metadata(&self, item: &str, payload: &str) {
        match self {
            Graph::Memory(board) => board.set_metadata(item, payload),
            Graph::Real(_, _) => self.wrote(&[
                "update",
                item,
                "--metadata",
                payload,
                "--actor",
                "another-tool",
            ]),
        }
    }

    /// The label the open-flight read finds a record by. The trait carries no
    /// label verb, so the real half writes it through the binary.
    pub fn label(&self, item: &str, label: &str) {
        match self {
            Graph::Memory(board) => board.label(item, label),
            Graph::Real(_, _) => self.wrote(&["label", "add", item, label]),
        }
    }

    /// One open dependency between two items, which is what takes the first out
    /// of the ready set.
    pub fn blocked_by(&self, item: &str, blocker: &str) {
        match self {
            Graph::Memory(board) => board.blocked_by(item, blocker),
            Graph::Real(_, _) => self.wrote(&["dep", "add", item, blocker]),
        }
    }

    pub fn status(&self, item: &str, status: &str) {
        match self {
            Graph::Memory(board) => board.status(item, status),
            Graph::Real(_, _) => {
                self.wrote(&["update", item, "--status", status, "--actor", "the-test"])
            }
        }
    }

    /// The type, as the store spells it. The trait carries no verb for it, so
    /// the real half writes it through the binary.
    pub fn item_type(&self, item: &str, kind: &str) {
        match self {
            Graph::Memory(board) => board.amend(item, |held| held.item_type = kind.to_string()),
            Graph::Real(_, _) => {
                self.wrote(&["update", item, "--type", kind, "--actor", "the-test"])
            }
        }
    }

    /// A call to the binary itself, which a ring arm's own readings take: the
    /// store's gate listing carries fields no trait method answers.
    pub fn bd(&self, args: &[&str]) -> std::process::Output {
        match self {
            Graph::Real(scratch, _) => scratch.bd(args),
            Graph::Memory(_) => panic!("only a ring's graph drives the binary"),
        }
    }

    fn wrote(&self, args: &[&str]) {
        let out = self.bd(args);
        assert!(
            out.status.success(),
            "bd {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

impl Rooted for Graph {
    fn root(&self) -> &Path {
        match self {
            Graph::Memory(board) => &board.root,
            Graph::Real(scratch, _) => &scratch.root,
        }
    }
    fn packs_dir(&self) -> &Path {
        match self {
            Graph::Memory(board) => &board.packs_dir,
            Graph::Real(scratch, _) => &scratch.packs_dir,
        }
    }
    fn defaults_dir(&self) -> &Path {
        match self {
            Graph::Memory(board) => &board.defaults_dir,
            Graph::Real(scratch, _) => &scratch.defaults_dir,
        }
    }
}

/// What a suite's helpers need of a board, whichever kind it is: somewhere to
/// run and somewhere to render from. Both boards answer it, so the one arm per
/// suite that takes a [`Scratch`] runs through the same helpers as the rest.
pub trait Rooted {
    fn root(&self) -> &Path;
    fn packs_dir(&self) -> &Path;
    fn defaults_dir(&self) -> &Path;
}

impl Rooted for Board {
    fn root(&self) -> &Path {
        &self.root
    }
    fn packs_dir(&self) -> &Path {
        &self.packs_dir
    }
    fn defaults_dir(&self) -> &Path {
        &self.defaults_dir
    }
}

impl Rooted for Scratch {
    fn root(&self) -> &Path {
        &self.root
    }
    fn packs_dir(&self) -> &Path {
        &self.packs_dir
    }
    fn defaults_dir(&self) -> &Path {
        &self.defaults_dir
    }
}
