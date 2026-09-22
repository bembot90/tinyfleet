//! `fleet brief <item>` — the first thing a dispatched seat reads.
//!
//! WHOLE OR NOT AT ALL. Every value is resolved into memory before a byte is
//! written, because a seat that read half a contract cannot tell it read half:
//! it has no second copy to compare against and no reason to suspect one.
//!
//! The four files it renders are slot paths resolved through the installed
//! layers over the binary's own defaults, so a pack above them replaces any of
//! the four whole.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::guard;
use crate::item::{render, Project, Stop};
use crate::policy;
use crate::resolve::{self, Layer, Resolution};
use crate::store::{Store, StoreError};

/// The four files this verb reads, all of them shadowable.
pub const BRIEF: &str = "assets/brief.md";
pub const DISPATCH_NOTE: &str = "assets/dispatch-note.md";
pub const DELIVERY_NOTE: &str = "assets/delivery-note.md";
pub const RULES: &str = "assets/rules.md";

/// The fifth, read by nothing this verb does: the first turn a SPAWNED REVIEWER
/// gets (flights PRD R14). It is a template in the pack rather than prose in the
/// flight's code, and it is in the shadow registry, because the contract a
/// reviewer works under is a pack's opinion the same way a builder's is.
pub const REVIEW_BRIEF: &str = "assets/review-brief.md";

/// What `{seat}` reads as before a seat exists to name.
pub const TRANSIENT: &str = "(transient)";

/// The substring every recorded order form ends on, and how the order note is
/// found among the item's notes.
pub const ORDER_MARK: &str = "orders given";

/// The value `{suite}` takes where the project declares none. `land` runs no
/// suite on such a project and the brief says so rather than leaving the line
/// blank.
pub const NO_SUITE: &str = "none";

/// The value `{touched}` takes where the project declares no builder's gate.
///
/// A SENTENCE and not a command, because there is no command to name and the
/// one thing that must not fill the hole is the project's whole suite: that is
/// the reviewer's, `land` runs it, and a seat running it too pays for it twice.
pub const DERIVE_TOUCHED: &str = concat!(
    "this project declares no `[gates] touched` command — read its own build targets\n",
    "and run only the suites whose sources your diff reaches. The suite below is the\n",
    "reviewer's, and running it here buys the landing nothing.",
);

/// The installed packs, ordered and resolved once.
pub struct Packs {
    pub layers: Vec<Layer>,
    pub resolution: Resolution,
}

impl Packs {
    /// Every installed pack, in layer order, over the binary's defaults, with
    /// the shadowing resolved.
    ///
    /// AN EMPTY PACKS DIRECTORY IS NOT A REFUSAL, and an absent DEFAULTS
    /// directory is: the defaults are the bottom layer whatever is installed,
    /// so a fleet with no pack resolves every path to them, and a fleet that
    /// has not written them resolves nothing and is told which verb writes
    /// them. Both readings are [`resolve::layers`]'s, which is the one place
    /// the bottom is placed.
    pub fn under(packs_dir: &Path, defaults_dir: &Path) -> Result<Packs, Stop> {
        let layers = resolve::layers(packs_dir, defaults_dir).map_err(refusals)?;
        let resolution = resolve::resolve(&layers).map_err(refusals)?;
        Ok(Packs { layers, resolution })
    }

    pub fn slot(&self, relative: &str) -> Result<PathBuf, Stop> {
        resolve::slot_path(&self.resolution, &self.layers, relative).ok_or_else(|| {
            Stop::could_not_tell(format!(
                "no installed pack carries `{relative}` — the layers resolved {} path(s)",
                self.resolution.files.len()
            ))
        })
    }

    pub fn read(&self, relative: &str) -> Result<String, Stop> {
        let path = self.slot(relative)?;
        std::fs::read_to_string(&path).map_err(|e| {
            Stop::could_not_tell(format!(
                "`{relative}` at {} is unreadable: {e}",
                path.display()
            ))
        })
    }

    /// Every resolved file under one directory, as `(path relative to that
    /// directory, content)`, in path order.
    ///
    /// An EMPTY ANSWER is a directory no installed pack carries, which is a
    /// legitimate state and not a refusal: the caller asking is one that has
    /// somewhere to put whatever is there and nothing to put when there is
    /// nothing.
    pub fn read_under(&self, prefix: &str) -> Result<Vec<(String, String)>, Stop> {
        let prefix = prefix.trim_end_matches('/');
        let under = format!("{prefix}/");
        let mut found = Vec::new();
        for relative in self.resolution.files.keys() {
            let Some(tail) = relative.strip_prefix(&under) else {
                continue;
            };
            found.push((tail.to_string(), self.read(relative)?));
        }
        Ok(found)
    }
}

fn refusals(found: Vec<resolve::Refusal>) -> Stop {
    let first = found
        .first()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "the layers refused and said nothing".to_string());
    Stop::could_not_tell(format!("the pack layers do not resolve: {first}"))
}

/// Everything the brief says about one item, gathered by the caller because
/// each half comes from a different instrument.
pub struct Subject<'a> {
    pub id: &'a str,
    /// The item as a person reads it, verbatim.
    pub text: &'a str,
    /// The order note the item carries.
    pub order: &'a str,
    /// The seat the order named, or [`TRANSIENT`].
    pub seat: &'a str,
}

/// The order note among an item's notes: the last line carrying a recorded
/// form.
///
/// An item with none has not been ordered, and a brief for it would tell a seat
/// it may begin when nothing said so.
pub fn order_line(notes: Option<&str>) -> Option<String> {
    notes?
        .lines()
        .map(str::trim)
        .filter(|line| line.contains(ORDER_MARK))
        .next_back()
        .map(str::to_string)
}

/// The brief, assembled whole.
pub fn text(packs: &Packs, project: &Project, subject: &Subject) -> Result<String, Stop> {
    let template = packs.read(BRIEF)?;
    let rules = packs.read(RULES)?;
    let delivery_note = packs.read(DELIVERY_NOTE)?;
    let suite = suite_of(project)?;
    let touched = touched_of(project)?;
    let guards = guards_of(project);

    render(
        &template,
        &[
            ("item_id", subject.id),
            ("item", subject.text),
            ("order", subject.order),
            ("seat", subject.seat),
            ("project", &project.name),
            ("suite", &suite),
            ("touched", &touched),
            ("guards", &guards),
            ("rules", &rules),
            ("delivery_note", &delivery_note),
        ],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{BRIEF}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
        ))
    })
}

/// What a reviewer's brief says about one delivery.
///
/// IT NAMES NO SEAT (decision D1). The reviewer's blindness in this slice is
/// what the mechanism gives — a fresh seat, no session shared with the builder,
/// rung by nobody — and the delivery region below carries the builder's own
/// first line, which is a fact of the record the reviewer reads either way. What
/// is NOT copied here is the order note: a second rendering of it would be a
/// second copy of a fact the item already holds.
pub struct Delivery<'a> {
    pub id: &'a str,
    /// The item as a person reads it, verbatim.
    pub text: &'a str,
    /// The last delivery region of the item's notes.
    pub delivery: &'a str,
    /// The size line `review` prints, measured by the caller.
    pub size: &'a str,
}

/// The reviewer's brief, assembled whole, from the pack's own template.
pub fn review_text(packs: &Packs, project: &Project, subject: &Delivery) -> Result<String, Stop> {
    let template = packs.read(REVIEW_BRIEF)?;
    let rules = packs.read(RULES)?;
    let suite = suite_of(project)?;

    render(
        &template,
        &[
            ("item_id", subject.id),
            ("item", subject.text),
            ("delivery", subject.delivery),
            ("size", subject.size),
            ("project", &project.name),
            ("suite", &suite),
            ("rules", &rules),
        ],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{REVIEW_BRIEF}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
        ))
    })
}

/// The brief on stdout, written once, with its size beside it.
///
/// The size line is stderr's because stdout is the brief and nothing else — and
/// it is printed because a first turn's byte count is the one number that says
/// what every dispatched seat pays before it has done anything
/// (gas-city G21).
pub fn print(
    out: &mut dyn Write,
    err: &mut dyn Write,
    packs: &Packs,
    project: &Project,
    subject: &Subject,
) -> Result<usize, Stop> {
    let body = text(packs, project, subject)?;
    out.write_all(body.as_bytes())
        .map_err(|e| Stop::could_not_tell(format!("the brief could not be written: {e}")))?;
    let _ = writeln!(err, "brief: {} bytes", body.len());
    Ok(body.len())
}

/// The brief for one item, read out of the store and printed.
///
/// The order note is the gate: an item carrying none has not been given to
/// anybody, and a brief for it would tell a seat it may begin when nothing said
/// so.
pub fn for_item(
    out: &mut dyn Write,
    err: &mut dyn Write,
    packs: &Packs,
    project: &Project,
    store: &dyn Store,
    item: &str,
    seat: &str,
) -> Result<usize, Stop> {
    let record = store.show(item).map_err(stop_of)?;
    let Some(order) = order_line(record.notes.as_deref()) else {
        return Err(Stop::refused(format!(
            "{item} carries no order note — a brief for an unordered item would tell a seat it \
             may begin when nothing said so"
        )));
    };
    let text = store.show_text(item).map_err(stop_of)?;
    print(
        out,
        err,
        packs,
        project,
        &Subject {
            id: item,
            text: &text,
            order: &order,
            seat,
        },
    )
}

fn stop_of(e: StoreError) -> Stop {
    match e {
        StoreError::Missing(why) => Stop::refused(why),
        StoreError::Unreadable(why) => Stop::could_not_tell(why),
    }
}

/// `[gates] suite`, through the census reader.
fn suite_of(project: &Project) -> Result<String, Stop> {
    match policy::read("gates", "suite", &project.gates) {
        Ok(Some(value)) => Ok(value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| NO_SUITE.to_string())),
        Ok(None) => Ok(NO_SUITE.to_string()),
        Err(unlisted) => Err(Stop::could_not_tell(unlisted.to_string())),
    }
}

/// `[gates] touched`, through the same reader: the gate a dispatched SEAT runs
/// over its own diff, which is not the gate the landing runs over the tree.
fn touched_of(project: &Project) -> Result<String, Stop> {
    match policy::read("gates", "touched", &project.gates) {
        Ok(Some(value)) => Ok(value
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| DERIVE_TOUCHED.to_string())),
        Ok(None) => Ok(DERIVE_TOUCHED.to_string()),
        Err(unlisted) => Err(Stop::could_not_tell(unlisted.to_string())),
    }
}

/// One line per guard class, on or off, read through the same function the
/// hook path reads.
fn guards_of(project: &Project) -> String {
    guard::CLASSES
        .iter()
        .map(|class| {
            let state = if guard::enabled(*class, &project.guards) {
                "on"
            } else {
                "off"
            };
            format!("- {}: {state}", class.name())
        })
        .collect::<Vec<_>>()
        .join("\n")
}
