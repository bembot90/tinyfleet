//! `fleet agent check [--adapter <path|name>] [--fixtures <dir>] [--live]
//! [--model <model>]` and `fleet agent schema`.
//!
//! One family module and one arm in the dispatch, shaped as `fleet store` is:
//! the agent adapter's family, as that is the store adapter's (ruling 18,
//! 2026-09-25).
//!
//! `check` runs every check of the agent contract ([`conformance::run`])
//! against an adapter, and prints one line per check as it is answered, then a
//! summary, as `fleet store check` does: the checks are the controller's, and
//! what this module owns is which adapter, the fixtures it is replayed
//! against, the scratch they all run in, and the exit.
//!
//! EVERYTHING A CHECK MAKES IS IN ONE TEMP DIR, which this module makes and
//! removes in every branch — a panic's too — by a drop guard: the requests'
//! root, each launch's configuration directory and worktree, and under
//! `--live` the tmux server the session runs on, on a socket of its own,
//! `fleet-check-<pid>`, and never fleet's own `fleet` (E1). The guard ends that
//! server before it removes the dir.
//!
//! `--live` COSTS A MODEL TURN, so it is never the default: without it the
//! live steps are printed as SKIP, saying so.
//!
//! `schema` prints the agent contract's JSON Schema document
//! ([`schema::text`]), which the core suite holds byte for byte to the
//! committed `core/src/agent/agent.schema.json`. It reads no project and runs
//! no adapter.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_controller::adapter::conformance::{self, Ctx, Live, Passed};
use fleet_controller::adapter::{self, AdapterSource, Version};
use fleet_controller::host::TmuxHost;
use fleet_controller::platform;
use fleet_core::agent::schema;
use fleet_core::policy::Value;

use crate::exit::Exit;
use crate::item;

/// The family's verbs.
#[derive(clap::Subcommand)]
pub enum Verb {
    /// run the agent contract's conformance suite against an adapter
    #[command(long_about = "\
run every check of the agent contract (docs/agent.md) against an adapter: what
it declares, what each launch answers and writes, the recorded cases it ships
in fixtures/ beside its adapter.toml, and the exits it owes. --adapter names
the adapter as [agent] adapter does: an absolute path to an executable, or the
name of an agent adapter an installed pack carries. Without it, it checks the
adapter the fleet's own file selects, else the built-in one. --live also starts
the agent in a scratch worktree on a scratch tmux socket and types one turn,
which costs a model turn. Exits 0 when every check passes, 1 when one fails, 2
on usage, 3 when the adapter could not be opened or --live has no tmux.")]
    Check {
        // The help is broken by hand where clap would run it past 80 columns:
        // this build of clap wraps nothing.
        #[arg(
            long,
            value_name = "PATH|NAME",
            help = "an absolute path to an adapter executable, or\nthe name of one an installed \
                    pack carries,\nchecked instead of the fleet's"
        )]
        adapter: Option<String>,
        #[arg(
            long,
            value_name = "DIR",
            help = "the recorded cases to replay, in place of the\nfixtures/ beside the \
                    adapter's adapter.toml"
        )]
        fixtures: Option<PathBuf>,
        #[arg(
            long,
            help = "also start the agent in a scratch worktree and\ntype one turn into it \
                    (costs a model turn)"
        )]
        live: bool,
        #[arg(
            long,
            value_name = "MODEL",
            help = "the model every launch names, in place of the\nadapter's declared \
                    default_model"
        )]
        model: Option<String>,
    },
    /// print the agent contract as a JSON Schema document
    #[command(long_about = "\
print the agent contract (docs/agent.md) as one JSON Schema document: each
verb's request and response, the refusal and the error, generated from the
types this fleet reads and writes. Exits 0.")]
    Schema,
}

pub fn command(verb: &Verb) -> Exit {
    match verb {
        Verb::Check {
            adapter,
            fixtures,
            live,
            model,
        } => check(
            adapter.as_deref(),
            fixtures.as_deref(),
            *live,
            model.clone(),
        ),
        Verb::Schema => {
            print!("{}", schema::text());
            Exit::Done
        }
    }
}

/// Which dir this process makes next, beside its pid.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The temp dir the checks run in, and the scratch tmux server `--live` runs
/// its session on: the server ended and the dir removed whole when this is
/// dropped — on every return after it is made, and on a panic's unwind.
struct Scratch {
    dir: PathBuf,
    host: Option<TmuxHost>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(host) = &self.host {
            // The whole server, whatever the steps left on it — a session the
            // check started and every pane it kept — and its socket. The
            // server runs as whoever made the dir, which is this process.
            if let Ok(made) = std::fs::metadata(&self.dir) {
                let _ = host.end_server(made.uid());
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A line on stderr under the verb's name, and the row it answers.
fn said(exit: Exit, why: impl std::fmt::Display) -> Exit {
    eprintln!("fleet agent check: {why}");
    exit
}

fn check(
    adapter: Option<&str>,
    fixtures: Option<&Path>,
    live: bool,
    model: Option<String>,
) -> Exit {
    // Either form `[agent] adapter` takes, and nothing else: a path that is
    // not absolute is a separator in a name, which no pack's adapter carries.
    if let Some(given) =
        adapter.filter(|given| !given.starts_with('/') && (given.is_empty() || given.contains('/')))
    {
        return said(
            Exit::Usage,
            format!(
                "--adapter takes an absolute path to an executable or the name of an agent \
                 adapter an installed pack carries, and `{given}` is neither"
            ),
        );
    }
    // A relative --fixtures is the caller's directory's, and made absolute
    // here: `{fixture}` is filled with a case's absolute path.
    let fixtures = match fixtures {
        None => None,
        Some(given) => {
            let absolute = std::env::current_dir()
                .map(|cwd| cwd.join(given))
                .unwrap_or_else(|_| given.to_path_buf());
            if !absolute.is_dir() {
                return said(
                    Exit::Usage,
                    format!(
                        "--fixtures names `{}`, which is not a directory",
                        given.display()
                    ),
                );
            }
            Some(absolute)
        }
    };

    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("fleet-agent-check-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut scratch = Scratch { dir, host: None };
    if let Err(e) = std::fs::create_dir_all(&scratch.dir) {
        return said(
            Exit::CouldNotTell,
            format!("{} could not be made: {e}", scratch.dir.display()),
        );
    }
    // RESOLVED, because an agent may key what it keeps on the directory it
    // runs in as the system resolves it, and the temp dir is a symlink away on
    // macOS: a worktree named through the link is one the agent never wrote
    // under.
    if let Ok(resolved) = std::fs::canonicalize(&scratch.dir) {
        scratch.dir = resolved;
    }

    // The fleet's own file where one resolves, as every opener reads it, over
    // the packs that project's machine installs; else this machine's, and no
    // setting, which is the default name.
    let machine_dir = platform::machine_dir();
    let mut setting = match item::resolve_at(None) {
        Ok(here) => match adapter::Setting::read(&here.policy_file, &here.machine_dir) {
            Ok(setting) => adapter::Setting {
                packs_dir: here.packs_dir.clone(),
                defaults_dir: here.defaults_dir.clone(),
                ..setting
            },
            Err(why) => return said(Exit::CouldNotTell, why),
        },
        Err(_) => adapter::Setting::of(
            Default::default(),
            &scratch.dir.join("fleet.toml"),
            &machine_dir,
        ),
    };
    // Every request the checks send carries the scratch dir as its root, so
    // what an adapter keeps beside its root stays inside what is removed.
    setting.root = scratch.dir.clone();
    let mut source = AdapterSource::Setting;
    if let Some(given) = adapter {
        let named = Value::from(given.to_string());
        let table = Value::from(BTreeMap::from([("adapter", named)]));
        setting.policy = Default::default();
        setting.policy.insert(String::from("agent"), table);
        source = AdapterSource::Flag;
    }
    let home = platform::home_dir();
    let mut opening = setting.opening(&home, None, None);
    opening.source = source;
    let opened = match adapter::open(&opening) {
        Ok(opened) => opened,
        Err(why) => return said(Exit::CouldNotTell, why),
    };
    // The cases the adapter ships beside its adapter.toml, where --fixtures
    // names none.
    let fixtures = fixtures.or_else(|| {
        opened
            .dir
            .as_ref()
            .map(|dir| dir.join("fixtures"))
            .filter(|dir| dir.is_dir())
    });

    let host = if live {
        match TmuxHost::resolve(&platform::child_path(&home)) {
            Ok(host) => {
                let host = host.on_socket(&format!("fleet-check-{}", std::process::id()));
                scratch.host = Some(host.clone());
                Some(host)
            }
            Err(cause) => {
                return said(
                    Exit::CouldNotTell,
                    format!("--live starts the agent on tmux, and there is none: {cause}"),
                )
            }
        }
    } else {
        None
    };
    let live = host.as_ref().map(|host| Live::on(host));

    let ctx = Ctx {
        agent: opened.agent.as_ref(),
        exec: opened.exec.as_ref(),
        fixtures: fixtures.as_deref(),
        scratch: &scratch.dir,
        model: model.as_deref(),
        live: live.as_ref(),
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
    // The agent by the name its version answers, else the adapter's; under
    // --live with the version too, because the live steps rest on what that
    // version of the agent was measured to do (E14).
    let version = opened.agent.version().ok();
    let name = version
        .as_ref()
        .map(|version| version.name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| opened.name.clone());
    let named = match (&live, version) {
        (None, _) => name,
        (
            Some(_),
            Some(Version {
                version: Some(installed),
                ..
            }),
        ) => format!("{name} {installed}"),
        (Some(_), Some(_)) => format!("{name}, with no version installed"),
        (Some(_), None) => format!("{name}, whose version did not answer"),
    };
    println!("agent check: {named} — {passed} passed, {failed} failed, {skipped} skipped");
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
            std::env::temp_dir().join(format!("fleet-agent-check-guard-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("live/config")).expect("the dir is made");
        let held = dir.clone();
        let unwound = std::panic::catch_unwind(move || {
            let _scratch = Scratch {
                dir: held,
                host: None,
            };
            panic!("a check panicked");
        });
        assert!(unwound.is_err(), "the closure panicked");
        assert!(!dir.exists(), "the temp dir is gone: {}", dir.display());
    }
}
