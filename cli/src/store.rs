//! `fleet store check [--adapter <path>]` and `fleet store schema`.
//!
//! One family module and one arm in the dispatch. `check` runs every check of
//! the store contract ([`conformance::run`]) against an adapter, and prints one
//! line per check as it is answered, then a summary: the checks are the
//! library's, and what this module owns is which adapter, the store they run
//! on, and the exit. `schema` prints the contract's JSON Schema document
//! ([`schema::text`]), which the core suite holds byte for byte to the
//! committed `core/src/store/store.schema.json`.
//!
//! THE CHECKS WRITE, so they never run on a project's own store. They run on a
//! scratch store the adapter makes through its `scratch` verb, inside a temp
//! dir this module makes and removes in every branch — a panic's too — by a
//! drop guard. An adapter that declares no scratch is refused with nothing run.
//!
//! NO OTHER WRITER IS HANDED IN. Planting another tool's keys on an item takes
//! a way in beside the contract, which only the suites own (the board held in
//! memory through its rig, bd through the binary), so the two checks that
//! plant them, "another writer's keys" and "another writer's keys are listed
//! as foreign", are printed as SKIP here, saying why.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::platform;
use fleet_core::policy::{self, Value};
use fleet_core::store::conformance::{self, Ctx, Passed};
use fleet_core::store::schema;
use fleet_core::store::{self, AdapterSource, Opening, PackDirs, STORE_TIMEOUT};

use crate::exit::Exit;
use crate::item;

/// The family's verbs.
#[derive(clap::Subcommand)]
pub enum Verb {
    /// run the store contract's conformance suite against an adapter
    #[command(long_about = "\
run every check of the store contract (docs/store.md) against an adapter, on a
scratch store the adapter makes through its `scratch` verb and fleet removes
after. Without --adapter it checks the adapter this project selects ([store]
adapter, else the built-in bd). Exits 0 when every check passes, 1 when one
fails or the adapter declares no scratch, 2 on usage, 3 when the adapter could
not be run.")]
    Check {
        // The help is broken by hand where clap would run it past 80 columns:
        // this build of clap wraps nothing.
        #[arg(
            long,
            value_name = "PATH",
            help = "an absolute path to an adapter executable,\nchecked instead of this project's"
        )]
        adapter: Option<PathBuf>,
    },
    /// print the store contract as a JSON Schema document
    #[command(long_about = "\
print the store contract (docs/store.md) as one JSON Schema document: each
verb's request and response, the refusal and the error, generated from the
types this fleet reads and writes. Exits 0.")]
    Schema,
}

pub fn command(verb: &Verb) -> Exit {
    match verb {
        Verb::Check { adapter } => check(adapter.as_deref()),
        Verb::Schema => {
            print!("{}", schema::text());
            Exit::Done
        }
    }
}

/// Which dir this process makes next, beside its pid.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The temp dir the checks run in, removed whole when this is dropped: on
/// every return after it is made, and on a panic's unwind.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A line on stderr under the verb's name, and the row it answers.
fn said(exit: Exit, why: impl std::fmt::Display) -> Exit {
    eprintln!("fleet store check: {why}");
    exit
}

fn check(adapter: Option<&Path>) -> Exit {
    if let Some(path) = adapter.filter(|path| !path.is_absolute()) {
        return said(
            Exit::Usage,
            format!(
                "--adapter takes an absolute path to an executable, and {} is not one",
                path.display()
            ),
        );
    }
    // The policy the store is opened with: `[store] adapter` naming the path
    // handed in; else the project's own file, where one resolves; else
    // nothing, which is the built-in bd.
    let here = match adapter {
        Some(_) => None,
        None => item::resolve_at(None).ok(),
    };
    let mut policy = here
        .as_ref()
        .map(|here| here.project.policy.clone())
        .unwrap_or_default();
    // A name resolves through the packs the project's machine installs, and a
    // path handed in names no packs at all.
    let packs = here.as_ref().map(|here| PackDirs {
        packs_dir: &here.packs_dir,
        defaults_dir: &here.defaults_dir,
    });
    let mut source = AdapterSource::Setting;
    if let Some(path) = adapter {
        let named = Value::from(path.display().to_string());
        let table = Value::from(BTreeMap::from([("adapter", named)]));
        policy.insert(String::from("store"), table);
        source = AdapterSource::Flag;
    }
    let policy = &policy;
    // The adapter by what selects it: the path, or the built-in store's name.
    let named = match policy::read("store", "adapter", policy) {
        Ok(Some(Value::String(named))) => named.clone(),
        _ => String::from(store::bd::NAME),
    };

    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("fleet-store-check-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let scratch = Scratch(dir);
    let absent_dir = scratch.0.join("absent");
    let into = scratch.0.join("store");
    for made in [&scratch.0, &absent_dir, &into] {
        if let Err(e) = std::fs::create_dir_all(made) {
            return said(
                Exit::CouldNotTell,
                format!("{} could not be made: {e}", made.display()),
            );
        }
    }

    let search_path = platform::child_path(&platform::home_dir());
    let open = |root: &Path| {
        store::open(&Opening {
            root,
            policy,
            source,
            search_path: &search_path,
            strict: true,
            timeout: STORE_TIMEOUT,
            packs,
        })
    };
    let adapter = match open(&scratch.0) {
        Ok(adapter) => adapter,
        Err(e) => return said(Exit::CouldNotTell, e),
    };
    match adapter.capabilities() {
        Ok(declared) if declared.scratch => {}
        Ok(_) => {
            return said(
                Exit::Refused,
                format!(
                    "{named} declares no scratch capability, and the check runs only on a \
                     store it makes for the purpose — nothing was run"
                ),
            )
        }
        Err(e) => return said(Exit::CouldNotTell, e),
    }
    let root = match adapter.scratch(&into) {
        Ok(root) => root,
        Err(e) => return said(Exit::CouldNotTell, e),
    };
    let (made, absent) = match (open(&root), open(&absent_dir)) {
        (Ok(made), Ok(absent)) => (made, absent),
        (Err(e), _) | (_, Err(e)) => return said(Exit::CouldNotTell, e),
    };

    let ctx = Ctx {
        store: made.as_ref(),
        root: &root,
        absent: absent.as_ref(),
        another_writer: None,
    };
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    for (name, answer) in conformance::run(&ctx) {
        match answer {
            Ok(Passed::Pass) => {
                passed += 1;
                println!("PASS  {name}");
            }
            Ok(Passed::Skip(why)) => {
                skipped += 1;
                println!("SKIP  {name}: {why}");
            }
            Err(why) => {
                failed += 1;
                println!("FAIL  {name}: {why}");
            }
        }
        // Each line is owed to a pipe as it lands, before the next check is
        // asked, whatever buffering std picks for a stdout that is no terminal.
        let _ = std::io::stdout().flush();
    }
    let name = made.version().map(|v| v.name).unwrap_or(named);
    println!("store check: {name} — {passed} passed, {failed} failed, {skipped} skipped");
    if failed == 0 {
        Exit::Done
    } else {
        Exit::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::Scratch;

    /// The guard removes the temp dir on a panic's unwind, whole, as it does
    /// on a return.
    #[test]
    fn the_temp_dir_is_removed_on_a_panic() {
        let dir =
            std::env::temp_dir().join(format!("fleet-store-check-guard-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("store/.beads")).expect("the dir is made");
        let held = dir.clone();
        let unwound = std::panic::catch_unwind(move || {
            let _scratch = Scratch(held);
            panic!("a check panicked");
        });
        assert!(unwound.is_err(), "the closure panicked");
        assert!(!dir.exists(), "the temp dir is gone: {}", dir.display());
    }
}
