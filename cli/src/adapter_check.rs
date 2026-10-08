//! What `fleet store check` and `fleet agent check` share: the form
//! `--adapter` takes, the scratch dir's name, and the tally of what the checks
//! answered.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use fleet_core::adapter::check::{Answer, Passed};
use fleet_core::pack::AdapterKind;

use crate::exit::Exit;

/// Why `--adapter` names neither form an adapter setting of `kind` takes, or
/// `None` where it is given none or one of them.
pub(crate) fn neither_form(adapter: Option<&str>, kind: AdapterKind) -> Option<String> {
    // Either form `[store] adapter` and `[agent] adapter` take, and nothing
    // else: a path that is not absolute is a separator in a name, which no
    // pack's adapter carries.
    adapter
        .filter(|given| !given.starts_with('/') && (given.is_empty() || given.contains('/')))
        .map(|given| {
            format!(
                "--adapter takes an absolute path to an executable or the name of {} an \
                 installed pack carries, and `{given}` is neither",
                kind.indefinite()
            )
        })
}

/// Which dir this process makes next, beside its pid.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The temp dir `fleet <verb> check` runs its checks in, unique to this
/// process and this call, with whatever an earlier process of the same pid
/// left there removed. The caller makes it, and its guard removes it.
pub(crate) fn scratch_dir(verb: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("fleet-{verb}-check-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// How many checks passed, failed and were skipped.
pub(crate) struct Tally {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// Every answer printed as it lands — `PASS`, `SKIP` with why, or `FAIL` with
/// what the adapter answered — and counted.
pub(crate) fn tally(answers: impl Iterator<Item = (&'static str, Answer)>) -> Tally {
    let mut tally = Tally {
        passed: 0,
        failed: 0,
        skipped: 0,
    };
    for (name, answer) in answers {
        match answer {
            Ok(Passed::Pass) => {
                tally.passed += 1;
                println!("PASS  {name}");
            }
            Ok(Passed::Skip(why)) => {
                tally.skipped += 1;
                println!("SKIP  {name}: {why}");
            }
            Err(why) => {
                tally.failed += 1;
                println!("FAIL  {name}: {why}");
            }
        }
        // Each line is owed to a pipe as it lands, before the next check is
        // asked, whatever buffering std picks for a stdout that is no terminal.
        let _ = std::io::stdout().flush();
    }
    tally
}

impl Tally {
    /// The counts as the summary line names them.
    pub(crate) fn counts(&self) -> String {
        format!(
            "{} passed, {} failed, {} skipped",
            self.passed, self.failed, self.skipped
        )
    }

    /// The exit the run answers: done when no check failed, else refused.
    pub(crate) fn exit(&self) -> Exit {
        if self.failed == 0 {
            Exit::Done
        } else {
            Exit::Refused
        }
    }
}
