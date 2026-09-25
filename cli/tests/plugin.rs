//! The plugin shape, and `fleet prime` through the shipped binary.
//!
//! The manifests, the hook wiring, the shim and the probe skill are read off
//! the tree rather than retyped here — the overlay's command list especially,
//! because the whole claim of the hook file is that it is that list addressed
//! through the plugin root, and a retyped copy would agree with itself.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod common;
use common::hermetic::Hermetic;

/// The address a plugin hook has and the Bash tool's PATH does not: the hook
/// process carries `CLAUDE_PLUGIN_ROOT` and nothing else locates `bin/`.
const HOOK_PREFIX: &str = "\"${CLAUDE_PLUGIN_ROOT}\"/bin/";

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn fleet_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate sits under the workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = fleet_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()))
}

fn json(relative: &str) -> serde_json::Value {
    serde_json::from_str(&read(relative))
        .unwrap_or_else(|e| panic!("{relative} parses as JSON: {e}"))
}

/// The `command` of every hook in one entry, in the file's order, with the
/// type of each asserted on the way past.
fn commands(entry: &serde_json::Value) -> Vec<String> {
    entry["hooks"]
        .as_array()
        .expect("an entry carries a `hooks` array")
        .iter()
        .map(|hook| {
            assert_eq!(
                hook["type"].as_str(),
                Some("command"),
                "every hook here is a command hook"
            );
            hook["command"]
                .as_str()
                .expect("a command hook carries a string command")
                .to_string()
        })
        .collect()
}

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("the output is utf-8")
}

// ---- AC1: the manifests, the hooks, the shim and the skill ------------------

#[test]
fn the_plugin_manifest_names_fleet_at_the_crate_version() {
    let doc = json(".claude-plugin/plugin.json");
    assert_eq!(doc["name"].as_str(), Some("fleet"));
    assert_eq!(
        doc["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "the manifest's version is the cli crate's, so a bump is one edit"
    );
    assert!(
        doc["description"]
            .as_str()
            .is_some_and(|d| !d.trim().is_empty()),
        "the manifest carries a description"
    );
    assert!(
        doc["author"]["name"]
            .as_str()
            .is_some_and(|n| !n.trim().is_empty()),
        "the manifest carries an author with a name"
    );
}

#[test]
fn the_marketplace_lists_one_plugin_at_this_directory() {
    let doc = json(".claude-plugin/marketplace.json");
    assert_eq!(doc["name"].as_str(), Some("fleet"));
    assert!(
        doc["owner"]["name"]
            .as_str()
            .is_some_and(|n| !n.trim().is_empty()),
        "the marketplace carries an owner with a name"
    );
    assert!(
        doc["description"]
            .as_str()
            .is_some_and(|d| !d.trim().is_empty()),
        "the marketplace carries a description"
    );
    let plugins = doc["plugins"]
        .as_array()
        .expect("the marketplace carries a `plugins` array");
    assert_eq!(plugins.len(), 1, "one entry, and it is this directory");
    assert_eq!(plugins[0]["name"].as_str(), Some("fleet"));
    assert_eq!(plugins[0]["source"].as_str(), Some("./"));
}

#[test]
fn the_session_start_hook_is_the_prime_line() {
    let plugin = json("hooks/hooks.json");
    let starts = plugin["hooks"]["SessionStart"]
        .as_array()
        .expect("the file carries a SessionStart array");
    assert_eq!(starts.len(), 1);
    assert_eq!(
        commands(&starts[0]),
        vec![format!("{HOOK_PREFIX}fleet prime")]
    );
}

#[test]
fn the_shim_runs_the_binary_the_seam_names() {
    use std::os::unix::fs::PermissionsExt;

    let shim = fleet_root().join("bin/fleet");
    let mode = std::fs::metadata(&shim)
        .expect("the shim is on the tree")
        .permissions()
        .mode();
    assert!(
        mode & 0o111 != 0,
        "the shim is executable; its mode is {mode:o}"
    );

    let direct = Command::new(env!("CARGO_BIN_EXE_fleet"))
        .arg("--version")
        .hermetic_nowhere()
        .output()
        .expect("the built binary runs");
    let through = Command::new(&shim)
        .env("FLEET_BIN", env!("CARGO_BIN_EXE_fleet"))
        .hermetic_nowhere()
        .arg("--version")
        .output()
        .expect("the shim runs");
    assert_eq!(through.status.code(), Some(0));
    assert_eq!(
        utf8(through.stdout),
        utf8(direct.stdout),
        "the shim's answer is the binary's own"
    );
}

#[test]
fn the_shim_refuses_a_relative_seam() {
    let shim = fleet_root().join("bin/fleet");
    let out = Command::new(&shim)
        .env("FLEET_BIN", "target/debug/fleet")
        .arg("--version")
        .output()
        .expect("the shim runs");
    assert_eq!(
        out.status.code(),
        Some(127),
        "a relative seam is refused, never resolved against whatever directory the caller was left in"
    );
    let text = utf8(out.stderr);
    assert!(
        text.contains("target/debug/fleet"),
        "the refusal names the path it was given: {text}"
    );
    assert!(
        utf8(out.stdout).is_empty(),
        "a refused shim runs nothing and prints nothing on stdout"
    );
}

// ---- the shim with no binary to run ------------------------------------------
//
// A pre-tool hook blocks only on exit 2, and every other non-zero exit is read
// as a hook that failed and let the call through. So a shim that exits 127 for
// a missing binary switches all four guards off without a word to the session,
// and the arms below hold the three answers it gives instead: a guard blocks,
// the session-start line fails open and says what is missing, and an ordinary
// verb fails as a command that could not run.

/// A plugin root holding the shim and NOTHING ELSE: no `target/`, which is the
/// shape of a checkout nobody built and of every copy the plugin cache holds.
/// The shim resolves its root off its own path, so the copy under the scratch
/// tree is what decides, and the real checkout's build is out of reach.
fn unbuilt_root(s: &Scratch) -> PathBuf {
    let root = s.dir("plugin");
    std::fs::create_dir_all(root.join("bin")).expect("the bin directory is created");
    std::fs::copy(fleet_root().join("bin/fleet"), root.join("bin/fleet"))
        .expect("the shim is copied, mode and all");
    assert!(
        !root.join("target").exists(),
        "the fixture root has no build under it"
    );
    // Canonical, because the shim names its root as `pwd -P` resolves it and
    // the temp directory is a link on some hosts.
    std::fs::canonicalize(&root).expect("the fixture root resolves")
}

/// One hook command line run the way the agent runs it: through a shell, with
/// `CLAUDE_PLUGIN_ROOT` naming the root and nothing else inherited — this
/// process's own `FLEET_BIN` least of all, since a suite run inside a seat
/// carries one. The home and the machine directory are the scratch tree's, so
/// a binary the hook does reach reads no fleet this box runs.
fn run_hook(s: &Scratch, command: &str, root: &Path, fleet_bin: Option<&str>) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", command])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CLAUDE_PLUGIN_ROOT", root)
        .env("HOME", s.dir("home"))
        .env("FLEET_DIR", s.dir("fleet-dir"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(bin) = fleet_bin {
        cmd.env("FLEET_BIN", bin);
    }
    let mut child = cmd.spawn().expect("the shell runs");
    // The payload a pre-tool hook is handed. A shim with nothing to run never
    // reads it, so a closed pipe on the far side is not an error here.
    let body = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": "git status" },
        "cwd": root,
    });
    let _ = child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(body.to_string().as_bytes());
    child.wait_with_output().expect("the shell finishes")
}

/// Every PreToolUse command the plugin's hook file carries, as written there.
fn guard_hook_commands() -> Vec<String> {
    let plugin = json("hooks/hooks.json");
    let pre = plugin["hooks"]["PreToolUse"]
        .as_array()
        .expect("the plugin file carries a PreToolUse array");
    let commands: Vec<String> = pre.iter().flat_map(commands).collect();
    // The control on the loops below: an empty list would pass every one.
    assert_eq!(
        commands.len(),
        4,
        "the four guard hooks were read: {commands:?}"
    );
    commands
}

/// THE GUARDS FAIL CLOSED. With no binary to judge by, every guard hook the
/// plugin wires exits 2 — the one status a pre-tool hook blocks on — and says on
/// stderr, which is what the agent is handed with the block, what is missing
/// and the two ways to supply it.
///
/// Three shapes of missing, because the shim has three refusals and a guard
/// that blocked on one and let the call through on the others would still be a
/// guard switched off by a typo: no build under the root with nothing named,
/// a `FLEET_BIN` that is relative, and one naming a file that is not there.
#[test]
fn a_guard_hook_with_no_binary_to_run_blocks_the_call_and_says_how_to_supply_one() {
    let s = Scratch::new("shim-guard-unbuilt");
    let root = unbuilt_root(&s);
    let absent = s.root.join("no-such-fleet");
    let shapes: [(&str, Option<&str>, &str); 3] = [
        ("no build and no FLEET_BIN", None, "cargo build --release"),
        (
            "a relative FLEET_BIN",
            Some("target/debug/fleet"),
            "target/debug/fleet",
        ),
        (
            "a FLEET_BIN naming nothing",
            Some(absent.to_str().expect("the temp path is utf-8")),
            absent.to_str().expect("the temp path is utf-8"),
        ),
    ];

    for command in guard_hook_commands() {
        let class = command
            .rsplit(' ')
            .next()
            .expect("a guard command ends on its class");
        for (shape, fleet_bin, named) in shapes {
            let out = run_hook(&s, &command, &root, fleet_bin);
            let text = utf8(out.stderr.clone());
            assert_eq!(
                out.status.code(),
                Some(2),
                "{shape}: `{command}` blocks the call rather than letting it run unjudged: {text}"
            );
            assert!(
                utf8(out.stdout.clone()).is_empty(),
                "{shape}: a block is a status and a reason, never a verdict on stdout"
            );
            for owed in ["blocked", class, named, "FLEET_BIN", "absolute path"] {
                assert!(
                    text.contains(owed),
                    "{shape}: the reason names `{owed}`: {text}"
                );
            }
            assert!(
                text.contains(root.to_str().expect("the temp path is utf-8")),
                "{shape}: the reason names the plugin root a build would go under: {text}"
            );
        }
    }
}

/// The CONTROL on the arm above, which says the block is the missing binary's
/// and not the shim's on every path: a guard hook with a binary to run is
/// that binary's own answer, exit 0 with nothing refused for a command no
/// class judges.
#[test]
fn a_guard_hook_with_a_binary_to_run_is_the_binarys_own_answer() {
    let s = Scratch::new("shim-guard-built");
    let root = unbuilt_root(&s);
    for command in guard_hook_commands() {
        let out = run_hook(&s, &command, &root, Some(env!("CARGO_BIN_EXE_fleet")));
        assert_eq!(
            out.status.code(),
            Some(0),
            "`{command}` judged and allowed: {}",
            utf8(out.stderr.clone())
        );
        assert!(
            utf8(out.stdout).is_empty(),
            "`{command}` refused nothing about `git status`"
        );
    }
}

/// THE SESSION-START LINE FAILS OPEN. A session whose start hook blocked would
/// never come up to be told why, so prime's missing binary is a non-zero status
/// that is NOT 2 — a hook that failed, shown to the person — and its words say
/// what the session is missing and what that does to the guards beside it.
#[test]
fn the_session_start_hook_with_no_binary_fails_open_and_says_what_the_session_lacks() {
    let s = Scratch::new("shim-prime-unbuilt");
    let root = unbuilt_root(&s);
    let plugin = json("hooks/hooks.json");
    let start = &commands(&plugin["hooks"]["SessionStart"][0])[0];

    let out = run_hook(&s, start, &root, None);
    let text = utf8(out.stderr.clone());
    assert_eq!(
        out.status.code(),
        Some(127),
        "the start hook fails as a command that could not run, which never blocks: {text}"
    );
    assert!(
        utf8(out.stdout).is_empty(),
        "nothing is handed to the session"
    );
    for owed in [
        "this session starts without",
        "every Bash command",
        "blocked",
        "cargo build --release",
        "FLEET_BIN",
        "absolute path",
    ] {
        assert!(text.contains(owed), "the reason names `{owed}`: {text}");
    }
}

/// An ORDINARY verb with no binary fails as a command that could not run, 127,
/// and its words say nothing of a block: the Bash tool reaches the same file by
/// bare name, and a person reading a refused `fleet status` must not be told a
/// guard judged something. `guard --check` is ordinary in this sense — it is a
/// person asking about configuration, and no hook runs it.
#[test]
fn an_ordinary_verb_with_no_binary_fails_as_could_not_run_and_never_as_a_block() {
    let s = Scratch::new("shim-ordinary-unbuilt");
    let root = unbuilt_root(&s);
    let shim = root.join("bin/fleet");
    let shim = shim.to_str().expect("the temp path is utf-8");

    for verb in ["status", "guard shell-trap --check"] {
        let out = run_hook(&s, &format!("'{shim}' {verb}"), &root, None);
        let text = utf8(out.stderr.clone());
        assert_eq!(out.status.code(), Some(127), "`fleet {verb}`: {text}");
        assert!(
            utf8(out.stdout).is_empty(),
            "`fleet {verb}` printed a verdict"
        );
        assert!(
            !text.contains("blocked"),
            "`fleet {verb}` is not reported as a block: {text}"
        );
        for owed in ["cargo build --release", "FLEET_BIN", "absolute path"] {
            assert!(
                text.contains(owed),
                "`fleet {verb}`: the reason names `{owed}`: {text}"
            );
        }
    }
}

#[test]
fn the_probe_skill_is_named_version() {
    let skill = read("skills/version/SKILL.md");
    assert!(
        skill.starts_with("---\n"),
        "the skill opens on its frontmatter"
    );
    let front = skill
        .split("\n---")
        .next()
        .expect("the frontmatter is delimited");
    assert!(
        front.lines().any(|l| l.trim() == "name: version"),
        "the frontmatter names the skill `version`: {front}"
    );
    assert!(
        front
            .lines()
            .any(|l| l.trim_start().starts_with("description:")),
        "the frontmatter carries a description: {front}"
    );
}

mod lessons {
    //! `claude-code.md` D5's and D6's fixture tests: the hook file is the
    //! overlay's command list addressed through the one variable a hook process
    //! carries, and a pack's skills reach a session through a link the loader
    //! follows rather than through a copy.
    //!
    //! WHICH OVERLAY IS THE DOCTRINE PACK'S. The plugin root is this repository's
    //! and this repository's project is the doctrine pack's, so the list the
    //! plugin must equal is that pack's shadow of the file — which is where the
    //! two classes a pack wires are wired. The second arm holds the
    //! relationship between the two lists, so a pack that dropped one of the
    //! default classes is caught here rather than in a session that stopped
    //! refusing.

    use super::*;

    const OVERLAY: &str = "packs/tiny/overlay/per-provider/claude/hooks.json";
    const DEFAULT_OVERLAY: &str = "core/defaults/overlay/per-provider/claude/hooks.json";

    /// The Bash entry's command list out of one hooks document.
    fn pre_tool_commands(relative: &str) -> Vec<String> {
        let document = json(relative);
        let pre = document["hooks"]["PreToolUse"]
            .as_array()
            .unwrap_or_else(|| panic!("{relative} carries a PreToolUse array"));
        assert_eq!(pre.len(), 1, "{relative}: one entry, matching Bash");
        assert_eq!(pre[0]["matcher"].as_str(), Some("Bash"), "{relative}");
        commands(&pre[0])
    }

    #[test]
    fn the_pack_wires_the_default_classes_first_and_its_own_after_them() {
        let defaults = pre_tool_commands(DEFAULT_OVERLAY);
        let pack = pre_tool_commands(OVERLAY);

        // The control on the prefix test below: two empty lists share a prefix,
        // and an overlay this test could not read would pass as agreement.
        assert!(
            !defaults.is_empty() && pack.len() > defaults.len(),
            "both lists were read, and the pack adds to the defaults': {defaults:?} / {pack:?}"
        );
        assert_eq!(
            pack[..defaults.len()],
            defaults[..],
            "the pack's list BEGINS with the defaults', in order — a pack wires its own \
             classes on top and removes none"
        );
        for class in ["release-ref", "production-write"] {
            assert!(
                pack.iter().any(|c| c.ends_with(&format!("guard {class}"))),
                "the pack wires {class}: {pack:?}"
            );
            assert!(
                !defaults
                    .iter()
                    .any(|c| c.ends_with(&format!("guard {class}"))),
                "and the defaults do not, which is why the pack has to: {defaults:?}"
            );
        }
    }

    #[test]
    fn the_plugin_root_addresses_the_hook() {
        let plugin = json("hooks/hooks.json");
        let pre = plugin["hooks"]["PreToolUse"]
            .as_array()
            .expect("the plugin file carries a PreToolUse array");
        assert_eq!(pre.len(), 1, "one entry, matching Bash");
        assert_eq!(pre[0]["matcher"].as_str(), Some("Bash"));

        let stripped: Vec<String> = commands(&pre[0])
            .into_iter()
            .map(|c| {
                c.strip_prefix(HOOK_PREFIX)
                    .unwrap_or_else(|| {
                        panic!("every plugin hook is addressed through the plugin root: {c}")
                    })
                    .to_string()
            })
            .collect();

        let want = pre_tool_commands(OVERLAY);

        // The control on the comparison below: two empty lists are equal, and
        // an overlay this test could not read would pass as agreement.
        assert!(
            !want.is_empty(),
            "the overlay's command list was read and is not empty"
        );
        assert_eq!(
            stripped, want,
            "the plugin's commands are the overlay's, in order, addressed through the plugin root"
        );
    }

    /// D6. The loader follows a symbolic link, so every ritual the doctrine pack
    /// owns is LINKED into the plugin root rather than copied there. What this
    /// pins is the shape that measurement licensed: one copy, in the pack, and
    /// the plugin root pointing at it.
    #[test]
    fn the_plugin_loader_follows_a_skill_link() {
        let root = fleet_root();
        let skills = root.join("skills");

        let mut linked = 0;
        let mut plain = 0;
        for entry in std::fs::read_dir(&skills).expect("the plugin's skills directory is readable")
        {
            let entry = entry.expect("the entry is readable");
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = std::fs::symlink_metadata(entry.path())
                .expect("the entry's own kind is readable")
                .file_type();
            if !kind.is_symlink() {
                // The probe skill is the plugin's own and is a real directory:
                // it is the control that says the test below discriminates,
                // rather than calling everything it finds a link.
                assert_eq!(
                    name, "version",
                    "the only skill living in the plugin root itself is the probe"
                );
                plain += 1;
                continue;
            }

            let target = std::fs::read_link(entry.path()).expect("the link's target is readable");
            let target = target.to_string_lossy().into_owned();
            assert_eq!(
                target,
                format!("../packs/tiny/skills/{name}"),
                "{name} is linked at the pack's own path, relative, so the link \
                 survives a checkout anywhere"
            );

            // The link is LIVE and not a stale copy: the bytes read through the
            // plugin's path are the pack file's own.
            let through = std::fs::read_to_string(entry.path().join("SKILL.md"))
                .unwrap_or_else(|e| panic!("{name} resolves to a SKILL.md: {e}"));
            let direct = read(&format!("packs/tiny/skills/{name}/SKILL.md"));
            assert_eq!(
                through, direct,
                "{name} is read through the link, not copied"
            );
            assert!(
                direct
                    .lines()
                    .any(|line| line.trim() == format!("name: {name}")),
                "{name}'s frontmatter names it for the directory the link points at"
            );
            linked += 1;
        }

        assert_eq!(plain, 1, "the probe skill was found");
        assert!(
            linked >= 8,
            "the pack's rituals reach the plugin root through links: {linked} found"
        );
    }

    /// The converse of the arm above, which holds every link it finds and so
    /// cannot see one that is missing. A skill the pack ships and the plugin
    /// root does not link is a ritual no `--plugin-dir` session can invoke, and
    /// nothing else fails for it: the pack installs, and every link that is
    /// there resolves.
    #[test]
    fn every_pack_skill_is_linked_into_the_plugin_root() {
        let root = fleet_root();
        let pack = root.join("packs/tiny/skills");

        let mut owned: Vec<String> = std::fs::read_dir(&pack)
            .expect("the pack's skills directory is readable")
            .map(|entry| entry.expect("the entry is readable"))
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        owned.sort();

        // The control on the loop below: a pack directory this test could not
        // read would own nothing, and nothing would pass as all linked.
        assert!(
            owned.iter().any(|name| name == "wake"),
            "the pack's skills were read: {owned:?}"
        );

        let unlinked: Vec<&String> = owned
            .iter()
            .filter(|name| {
                std::fs::symlink_metadata(root.join("skills").join(name.as_str()))
                    .map(|meta| !meta.file_type().is_symlink())
                    .unwrap_or(true)
            })
            .collect();
        assert!(
            unlinked.is_empty(),
            "every skill the pack ships has a link under skills/, or a session \
             loaded with the plugin has no fleet: ritual for it: {unlinked:?} unlinked"
        );
    }
}

// ---- AC2: `fleet prime` -----------------------------------------------------

/// A scratch tree: a working directory with no `fleet.toml` above it, a fleet
/// directory the binary is pointed at, and whatever else the case writes.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("fleet-plugin-{label}-{}-{n}", std::process::id()));
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
        .hermetic(&fleet_dir.join("home"), fleet_dir, None)
        // The suite shares one process environment across arms run in
        // parallel, so every seam a case does not set is removed rather than
        // inherited.
        .env_remove("FLEET_BD_BIN");
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

/// Line 1, line 2 and what follows them. Line 2 is the tracker's version, read
/// off whichever `bd` the arm resolves — the box's own where it sets no seam —
/// so an arm about what follows it asserts its shape and not its words.
fn past_line_two(text: &str) -> (&str, &str, &str) {
    let (first, rest) = text
        .split_once('\n')
        .expect("prime printed more than a line");
    let (second, rest) = rest
        .split_once('\n')
        .expect("prime printed more than two lines");
    assert!(
        second.starts_with("bd: "),
        "line 2 is the tracker's: {text}"
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
        .hermetic(&fleet_dir.join("home"), fleet_dir, None)
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
        vec!["add", "--", "fleet"],
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
            "fleet {} — packs: none installed; guards: shell-trap on, record off, \
             release-ref on, production-write on",
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
        line_one(&prime(&on_cwd, &on_dir, &[]))
            .ends_with("shell-trap on, record on, release-ref on, production-write on"),
        "an untouched policy leaves every class on"
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

/// `pack add` of the two shipped packs, then `prime` over what it installed.
///
/// The source is a repository holding this tree's two pack directories at their
/// own paths, and not a clone of the whole checkout: the verb clones what it is
/// pointed at, and a local clone of this repository copied 1.6 GB in 8 seconds
/// on the box this was written on, per run, growing with the history. What the
/// arm is about — the subdirectory form, the layering check the DEFAULTS'
/// registry gates, and the bytes that land — is carried by the pack
/// directories, which are COPIED here rather than retyped.
///
/// ts first, then tiny — one of the orders `check_layering` accepts, since it
/// lays each add by the imports and not by the install sequence; the core
/// suite's add arms cover the tiny-before-ts order. The lock records both, and
/// the binary's defaults are materialized first so the registry rule the adds
/// pass is the shipped one.
#[test]
fn the_shipped_packs_install_from_a_checkout_and_prime_reads_them() {
    let s = Scratch::new("tiny-install");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let source = s.dir("source");
    s.shipped_defaults();

    const SHIPPED: [&str; 2] = ["ts", "tiny"];
    for name in SHIPPED {
        copy_dir(
            &fleet_root().join("packs").join(name),
            &source.join("fleet/packs").join(name),
        );
    }
    git_fixture(&source);

    let packs_dir = fleet_dir.join("packs");
    let lock = fleet_dir.join("packs.lock");
    for name in SHIPPED {
        let out = pack_add(
            &format!("{}//fleet/packs/{name}", text_of(&source)),
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
        "one lock line per shipped pack, in source order"
    );
    let ts = pinned
        .iter()
        .find(|e| e.name.as_deref() == Some("ts"))
        .expect("the lock records the ts pack");
    assert!(ts.source.ends_with("//fleet/packs/ts"), "{}", ts.source);
    assert_eq!(ts.version, "v1");

    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, ""));

    let out = prime(&cwd, &fleet_dir, &[]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let (first, _, rest) = past_line_two(&text);
    assert!(
        first.contains("packs: tiny, ts"),
        "the doctrine pack sits on top and the runtime pack it imports beneath it: {first}"
    );

    // Read off the tree, not off the installed copy: a comparison against what
    // `pack add` wrote would only prove that prime read the same file twice.
    let rules = read("packs/tiny/assets/rules.md");
    assert!(!rules.is_empty(), "the pack's rules file was read");
    assert_eq!(rest, rules, "the pack's rules follow line 2 byte for byte");
}

/// The fail-open contract, at the one place a layering can refuse: line 1
/// still prints, with the refusal in place of the names; the rules are omitted,
/// because there is no resolution to ask; the item lines still print, because
/// they do not come from the packs; and the exit is still 0.
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
    let stub = s.script("bd", "#!/bin/sh\necho '[]'\n");

    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
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
        first.ends_with("guards: shell-trap on, record on, release-ref on, production-write on"),
        "and the guards, which the packs say nothing about, are still read: {first}"
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
        first.ends_with("guards: shell-trap on, record on, release-ref on, production-write on"),
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

    let argv = s.root.join("argv");
    let stub = s.script(
        "bd",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > {argv}\ncat <<'JSON'\n[\
             {{\"id\": \"x-1\", \"title\": \"the open one\", \"status\": \"open\"}},\
             {{\"id\": \"x-2\", \"title\": \"the moving one\", \"status\": \"in_progress\"}},\
             {{\"id\": \"x-3\", \"title\": \"the done one\", \"status\": \"closed\"}}]\nJSON\n",
            argv = text_of(&argv)
        ),
    );

    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let items: Vec<&str> = text.lines().filter(|l| l.starts_with("item:")).collect();
    assert_eq!(
        items,
        vec!["item: x-1 — the open one", "item: x-2 — the moving one"],
        "open and in-progress only, in the order the tracker listed them: {text}"
    );

    let asked = std::fs::read_to_string(&argv).expect("the stub recorded its arguments");
    assert_eq!(
        asked.trim(),
        format!(
            "-C {} list -a 01a0d1f1-0aec-765f-9abe-5c21e8a04b17 --all --json -n 0",
            text_of(&cwd)
        ),
        "the listing is asked of the row's own project root, for the row's full id"
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
    let stub = s.script("bd", "#!/bin/sh\necho '[]'\n");

    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
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
    let stub = s.script("bd", "#!/bin/sh\necho '[]'\n");

    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
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
    let stub = s.script("bd", "#!/bin/sh\necho 'the store is locked' >&2\nexit 4\n");

    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
    assert_eq!(out.status.code(), Some(0), "prime still exits 0");
    let text = utf8(out.stdout);
    assert!(
        text.contains(&format!(
            "item: could not be read — `{} list -a 01a0d1f1-0aec-765f-9abe-5c21e8a04b17 --all --json -n 0` exit status: 4: the store \
             is locked",
            text_of(&stub)
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
/// would hold the session with it. The stub answers line 2's `version` at once,
/// so the bound measured is the listing's alone.
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
    let stub = s.script(
        "bd",
        &format!(
            "#!/bin/sh\n[ \"$1\" = version ] && {{ echo 'bd version {}'; exit 0; }}\nsleep 30\n",
            fleet_core::store::bd::PINNED_BD
        ),
    );

    let started = std::time::Instant::now();
    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
    let took = started.elapsed();
    assert_eq!(out.status.code(), Some(0), "prime still exits 0");
    let text = utf8(out.stdout);
    assert!(
        text.contains(&format!(
            "item: could not be read — `{} list -a 01a0d1f1-0aec-765f-9abe-5c21e8a04b17 --all --json -n 0` did not answer within 5s",
            text_of(&stub)
        )),
        "the third answer names the bound the listing outran: {text}"
    );
    assert!(
        took < std::time::Duration::from_secs(7),
        "prime answered about five seconds in, not when the tracker gave up: {took:?}"
    );
}

/// A seat's rig whose tracker answers `version` with `version` and every other
/// call with one open item, so an arm reads line 2 and the item line off one
/// prime.
fn tracker_rig(label: &str, version: &str) -> (Scratch, PathBuf, PathBuf, PathBuf) {
    let s = Scratch::new(label);
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));
    let stub = s.script(
        "bd",
        &format!(
            "#!/bin/sh\n[ \"$1\" = version ] && {{ {version}; exit 0; }}\ncat <<'JSON'\n[\
             {{\"id\": \"x-1\", \"title\": \"the open one\", \"status\": \"open\"}}]\nJSON\n"
        ),
    );
    (s, cwd, fleet_dir, stub)
}

/// Where to install the pin, as line 2 prints it: beads' own installation
/// page at the pin's tag, naming the pinned version. The doctor check points
/// at the same page.
fn install_pointer() -> String {
    let pin = fleet_core::store::bd::PINNED_BD;
    format!(
        "install the pinned bd {pin} by beads' own instructions: \
         https://github.com/gastownhall/beads/blob/v{pin}/docs/getting-started/installation.md"
    )
}

/// fleet-reb: line 2 is the tracker's version against `store::bd::PINNED_BD`. The
/// pin reads as itself; another version is NAMED with where to install the
/// pin, and the item line still prints beneath it, because the verbs still
/// run on it. The mismatch arm is what makes the match arm worth anything: a
/// line that called any answer the pin would pass the first alone.
#[test]
fn line_two_names_the_tracker_s_version_against_the_pin() {
    let pin = fleet_core::store::bd::PINNED_BD;
    for (label, version, line) in [
        (
            "bd-pinned",
            format!("echo 'bd version {pin} (Homebrew)'"),
            format!("bd: {pin}, the pinned version"),
        ),
        (
            "bd-other",
            String::from("echo 'bd version 1.2.2 (Homebrew)'"),
            format!(
                "bd: 1.2.2, not the pinned {pin} — the verbs still run, on answers fleet was not \
                 measured against; {}",
                install_pointer()
            ),
        ),
    ] {
        let (_s, cwd, fleet_dir, stub) = tracker_rig(label, &version);
        let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
        assert_eq!(out.status.code(), Some(0), "prime always exits 0");
        let text = utf8(out.stdout);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.get(1), Some(&line.as_str()), "{label}: {text}");
        assert!(
            lines.contains(&"item: x-1 — the open one"),
            "{label}: the item line still prints: {text}"
        );
    }
}

/// A tracker nothing resolves, and one that never answers its version, are
/// line 2's third answer — each naming why and where to install the pin —
/// and cost nothing else: the hung one is cut at line 2's own two-second
/// bound, and the item line after it still prints.
#[test]
fn a_tracker_that_does_not_answer_its_version_is_line_two_s_own_answer() {
    let (s, cwd, fleet_dir, _) = tracker_rig("bd-absent", "true");
    let absent = s.root.join("no-such-dir/bd");
    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&absent))]);
    assert_eq!(out.status.code(), Some(0), "prime always exits 0");
    let text = utf8(out.stdout);
    assert_eq!(
        text.lines().nth(1),
        Some(
            format!(
                "bd: could not be read — the item-tracker seam names `{}`, which is not an \
                 executable file; {}",
                text_of(&absent),
                install_pointer()
            )
            .as_str()
        ),
        "{text}"
    );

    let (_s, cwd, fleet_dir, stub) = tracker_rig("bd-hung", "sleep 30");
    let started = std::time::Instant::now();
    let out = prime(&cwd, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
    let took = started.elapsed();
    assert_eq!(out.status.code(), Some(0), "prime still exits 0");
    let text = utf8(out.stdout);
    assert_eq!(
        text.lines().nth(1),
        Some(
            format!(
                "bd: could not be read — `{} version` did not answer within 2s; {}",
                text_of(&stub),
                install_pointer()
            )
            .as_str()
        ),
        "{text}"
    );
    assert!(
        text.lines().any(|l| l == "item: x-1 — the open one"),
        "the item line still prints: {text}"
    );
    assert!(
        took < std::time::Duration::from_secs(4),
        "prime answered about two seconds in, not when the tracker gave up: {took:?}"
    );
}

/// `store::bd::resolve`'s DEFAULT branch: with no `FLEET_BD_BIN`, the tracker
/// is the first `bd` on the CONSTRUCTED child PATH — the platform's own
/// directory list over the home — and the `PATH` this process inherited
/// contributes nothing.
///
/// The decoy is the control: it is first on the child's inherited `PATH` and on
/// no entry of the constructed one, so it is reachable by a bare name and by
/// nothing else.
///
/// macOS orders `/opt/homebrew/bin` and `/usr/local/bin` ahead of the home's
/// `.local/bin`, and the home is the only entry a test may write into — so a
/// `bd` installed in either of those genuinely wins, and this arm asserts
/// against whichever file the constructed path names rather than against its
/// own stub.
#[test]
fn with_no_seam_the_tracker_comes_off_the_constructed_child_path() {
    let s = Scratch::new("bd-default");
    let cwd = s.dir("cwd");
    let fleet_dir = s.dir("fleet-dir");
    let fleet_toml = s.write("fleet-root/fleet.toml", "");
    let row = format!(
        "{{\"id\": \"01a0d1f1-0aec-765f-9abe-5c21e8a04b17\", \"worktrees\": {{\"demo\": {}}}}}",
        serde_json::Value::String(text_of(&cwd))
    );
    s.write("fleet-dir/config.json", &config_json(&fleet_toml, &row));

    let home = s.dir("home");
    let stub_argv0 = s.root.join("stub-argv0");
    let stub = s.script(
        "home/.local/bin/bd",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$0\" > {log}\ncat <<'JSON'\n[\
             {{\"id\": \"fxitem-stub9\", \"title\": \"the constructed path\", \
             \"status\": \"open\"}}]\nJSON\n",
            log = text_of(&stub_argv0)
        ),
    );
    let decoy_argv0 = s.root.join("decoy-argv0");
    let decoy = s.script(
        "decoy/bd",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$0\" > {log}\ncat <<'JSON'\n[\
             {{\"id\": \"fxitem-decoy9\", \"title\": \"the inherited PATH\", \
             \"status\": \"open\"}}]\nJSON\n",
            log = text_of(&decoy_argv0)
        ),
    );
    let decoy_dir = decoy.parent().expect("the decoy sits in a directory");

    let child_path = fleet_controller::platform::child_path(&home);
    assert!(
        !std::env::split_paths(&child_path).any(|dir| dir == decoy_dir),
        "the decoy is on no entry of the constructed child PATH {child_path}"
    );
    let named = fleet_controller::platform::resolve_on_path(&child_path, "bd")
        .expect("the constructed child PATH names a `bd` — the stub is on it");

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
    let text = utf8(out.stdout.clone());
    let item = text
        .lines()
        .find(|l| l.starts_with("item:"))
        .unwrap_or_else(|| panic!("prime prints an item line: {text}"));

    assert!(
        !text.contains("fxitem-decoy9"),
        "the decoy answers only a bare name resolved on the inherited PATH: {item}"
    );
    assert!(
        !decoy_argv0.exists(),
        "the decoy was never run, and it recorded: {}",
        std::fs::read_to_string(&decoy_argv0).unwrap_or_default()
    );

    if named == stub {
        let argv0 = std::fs::read_to_string(&stub_argv0).expect("the stub recorded its own $0");
        assert_eq!(
            argv0.trim(),
            text_of(&stub),
            "the stub was reached at the absolute path the constructed child PATH names"
        );
    } else {
        assert!(
            !stub_argv0.exists(),
            "{} is earlier on the constructed child PATH than the stub, which ran anyway",
            named.display()
        );
    }

    // The positive claim, in one shape whichever file won: prime's item line
    // carries that file's own answer.
    let direct = Command::new(&named)
        .arg("-C")
        .arg(&cwd)
        .args([
            "list",
            "-a",
            "01a0d1f1-0aec-765f-9abe-5c21e8a04b17",
            "--json",
            "-n",
            "0",
        ])
        .output()
        .expect("the resolved tracker runs");
    let fingerprint = if direct.status.success() {
        let rows: serde_json::Value =
            serde_json::from_str(&utf8(direct.stdout.clone())).expect("the answer is JSON");
        rows.as_array()
            .and_then(|rows| rows.first())
            .and_then(|first| first["id"].as_str())
            .expect("the answer lists a row")
            .to_string()
    } else {
        utf8(direct.stderr.clone())
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .expect("a refusing tracker says why")
            .to_string()
    };
    assert!(
        item.contains(&fingerprint),
        "the item line carries the answer of {}, the file the constructed child PATH names: \
         {item}",
        named.display()
    );
    println!(
        "the constructed child PATH names {} — the arm asserted against that file",
        named.display()
    );
}

/// A machine row names a worktree ROOT, and the match is on that directory
/// itself: a session in a subdirectory of it gets no item line at all rather
/// than a wrong seat's (D3 of the prime spec — a cwd names a seat and proves
/// nothing, so the weakest reading is the one taken).
///
/// The tracker stub would answer with one item, and its argv file — holding
/// the LAST call it was handed — is the witness that it was asked its version
/// for line 2 and never for a listing. The same fixture run from the root is
/// the control: there the item line appears and the listing is the last call.
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

    let argv = s.root.join("argv");
    let stub = s.script(
        "bd",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > {argv}\ncat <<'JSON'\n[\
             {{\"id\": \"x-9\", \"title\": \"the descendant one\", \"status\": \"open\"}}]\nJSON\n",
            argv = text_of(&argv)
        ),
    );

    let out = prime(&under, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
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
        std::fs::read_to_string(&argv)
            .expect("the stub recorded line 2's call")
            .trim(),
        "version",
        "the tracker was asked its version and never for a listing"
    );

    let out = prime(&root, &fleet_dir, &[("FLEET_BD_BIN", &text_of(&stub))]);
    assert_eq!(out.status.code(), Some(0));
    let text = utf8(out.stdout);
    let items: Vec<&str> = text.lines().filter(|l| l.starts_with("item:")).collect();
    assert_eq!(
        items,
        vec!["item: x-9 — the descendant one"],
        "the same fixture run from the root itself does get the line: {text}"
    );
    let asked = std::fs::read_to_string(&argv).expect("the stub recorded its arguments");
    assert_eq!(
        asked.trim(),
        format!(
            "-C {} list -a 01a0d1f1-0aec-765f-9abe-5c21e8a04b17 --all --json -n 0",
            text_of(&root)
        ),
        "and it is asked of the row's own root, never of the subdirectory"
    );
}
