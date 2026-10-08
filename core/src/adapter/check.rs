//! What every contract's conformance suite answers with and runs by, whichever
//! contract it checks.

/// A check that did not fail: it passed, or it was not asked of this adapter,
/// and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Passed {
    Pass,
    Skip(String),
}

/// What one check answers: passed or skipped, or the text of what the adapter
/// answered instead.
pub type Answer = Result<Passed, String>;

/// The contract's word on a reading, or `why` as the failure.
pub fn ensure(held: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if held {
        Ok(())
    } else {
        Err(why())
    }
}

/// One check by its name, as a suite's table lists it: the contract's own
/// `Check` over its own context.
type Named<C> = (&'static str, fn(&C) -> Answer);

/// Every check of `checks` against `ctx`, in order, each answer beside its
/// name, each check asked only as the next answer is read. A failure never
/// stops the run.
pub fn run<'c, C>(
    checks: &'c [Named<C>],
    ctx: &'c C,
) -> impl Iterator<Item = (&'static str, Answer)> + 'c {
    checks.iter().map(move |(name, check)| (*name, check(ctx)))
}
