//! The check rows, printed as each is read and kept for the landed entry.

use std::io::Write;

use super::{Wiring, CRITERIA};
use crate::entry::CheckRow;

// ---- the rows ----------------------------------------------------------------

/// The check rows, printed as each is read and kept for the landed entry.
///
/// A row carries its own criterion NAME rather than taking it from its
/// position, because the rerun adds a row in the middle and a positional name
/// would then print every row after it under its neighbour's heading. The
/// positional count is kept apart for that reason: [`Rows::read`] consumes the
/// next name in [`CRITERIA`], [`Rows::read_named`] consumes none.
pub(super) struct Rows {
    read: Vec<CheckRow>,
    /// How many of [`CRITERIA`] have been used.
    positional: usize,
}

impl Rows {
    pub(super) fn new() -> Rows {
        Rows {
            read: Vec::new(),
            positional: 0,
        }
    }

    /// One criterion read: its row on stdout, one step of the bar, and the
    /// reading kept for the entry.
    pub(super) fn read(
        &mut self,
        out: &mut dyn Write,
        wiring: &Wiring,
        verdict: &str,
        evidence: impl Into<String>,
    ) {
        let named = criterion(self.positional);
        self.positional += 1;
        self.read_named(out, wiring, named, verdict, evidence);
    }

    /// A row whose criterion is not one of [`CRITERIA`]'s and does not consume
    /// one: the suite's second reading is the only one there is.
    pub(super) fn read_named(
        &mut self,
        out: &mut dyn Write,
        wiring: &Wiring,
        named: &'static str,
        verdict: &str,
        evidence: impl Into<String>,
    ) {
        let evidence = evidence.into();
        let n = self.read.len();
        let _ = writeln!(out, "{}", row(n + 1, named, verdict, &evidence));
        self.read.push(CheckRow {
            check: named.to_string(),
            verdict: verdict.to_string(),
            evidence,
        });
        wiring.progress.row();
    }

    /// The rows the landed entry carries: one per check read, in the order
    /// they were read.
    pub(super) fn rows(&self) -> Vec<CheckRow> {
        self.read.clone()
    }
}

/// One criterion row: its number, the criterion, the verdict and the evidence.
/// `fleet item show` prints a landing entry's check rows through this same
/// line, so the page a person watched and the record read afterwards are one
/// text.
pub(crate) fn row(n: usize, criterion: &str, verdict: &str, evidence: &str) -> String {
    format!("{n}. {criterion:<16} {verdict:<10} {evidence}")
}

/// The name of the nth criterion. A row past the list is named rather than
/// panicked on: the land module states the count and a mismatch is a defect
/// in it, not a reason to take a landing down mid-push.
fn criterion(n: usize) -> &'static str {
    CRITERIA.get(n).copied().unwrap_or("(unnamed)")
}
