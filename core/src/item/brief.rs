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
use crate::input::{self, DeliveryInput};
use crate::item::{render, show, Project, Stop};
use crate::resolve::{self, Layer, Resolution};
use crate::store::{Orders, Store};

/// The four files this verb reads, all of them shadowable.
pub const BRIEF: &str = "assets/brief.md";
pub const RULES: &str = "assets/rules.md";

/// What `{seat}` reads as before a seat exists to name.
pub const TRANSIENT: &str = "(transient)";

/// The value `{touched}` takes where the dispatch was handed no builder's checks.
///
/// A SENTENCE and not a command, because there is no command to name and the
/// one thing that must not fill the hole is the project's whole suite: that is
/// the reviewer's, `land` runs it, and a seat running it too pays for it twice.
pub const DERIVE_TOUCHED: &str = concat!(
    "no touched command was handed to this dispatch — read the project's own build\n",
    "targets and run only the suites whose sources your diff reaches. The whole suite\n",
    "is the reviewer's, and running it here buys the landing nothing.",
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
    /// The item as a person reads it: [`show::render`] of the item and its
    /// timeline, verbatim, so the seat reads what `fleet item show` prints and
    /// never the text the store keeps an entry as.
    pub text: &'a str,
    /// The order, as [`order_text`] renders it off the item's order index.
    pub order: &'a str,
    /// The machine name of the seat the order named, or [`TRANSIENT`]: the
    /// brief is read by a seat and a person, and the id the record carries is
    /// neither's name for it.
    pub seat: &'a str,
    /// The builder's checks as the caller handed them, or `None` for
    /// [`DERIVE_TOUCHED`]. It is the CALLER's because it is the workflow's: a
    /// project's policy names no test command.
    pub touched: Option<&'a str>,
}

/// The order as the brief prints it: who gave it and when, off the order
/// index. `fleet brief` and a dispatch's own brief both render it here, so the
/// two are one text.
pub fn order_text(index: &Orders) -> String {
    let field = |held: &Option<String>| held.clone().unwrap_or_else(|| String::from("(absent)"));
    format!(
        "dispatch ordered by {} at {}",
        field(&index.by),
        field(&index.at)
    )
}

/// The brief, assembled whole.
pub fn text(packs: &Packs, project: &Project, subject: &Subject) -> Result<String, Stop> {
    project.refuse_moved()?;
    let template = packs.read(BRIEF)?;
    let rules = packs.read(RULES)?;
    let delivery_schema = packs.read(input::DELIVERY_SCHEMA)?;
    // The schema a seat is shown is the one `fleet deliver` reads it against,
    // or the brief would teach a delivery the verb refuses.
    input::agrees::<DeliveryInput>(&delivery_schema).map_err(|why| {
        Stop::could_not_tell(format!(
            "{} as the layers resolve it does not describe what fleet deliver reads: {why}",
            input::DELIVERY_SCHEMA
        ))
    })?;
    let touched = command_or(subject.touched, DERIVE_TOUCHED);
    let guards = guards_of(project);

    render(
        &template,
        &[
            ("item_id", subject.id),
            ("item", subject.text),
            ("order", subject.order),
            ("seat", subject.seat),
            ("project", &project.name),
            ("touched", touched),
            ("guards", &guards),
            ("rules", &rules),
            ("delivery_schema", delivery_schema.trim_end()),
        ],
    )
    .map_err(|name| {
        Stop::could_not_tell(format!(
            "`{BRIEF}` writes `{{{name}}}`, which is not a placeholder this verb resolves"
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
/// The order INDEX is what decides — `metadata["fleet.orders"]`, the reading
/// dispatch, deliver, review and retire all take: an item carrying none has not
/// been given to anybody, and a brief for it would tell a seat it may begin
/// when nothing said so. The timeline is never searched for it, because a
/// withdrawal unsets the index and leaves the ordered entry standing.
///
/// The order the brief prints is rendered from the index by [`order_text`],
/// which a dispatch renders its own brief's order with, so the brief `fleet
/// brief` prints and the one a dispatch handed its seat are one text.
#[allow(clippy::too_many_arguments)]
pub fn for_item(
    out: &mut dyn Write,
    err: &mut dyn Write,
    packs: &Packs,
    project: &Project,
    store: &dyn Store,
    item: &str,
    seat: &str,
    touched: Option<&str>,
) -> Result<usize, Stop> {
    // Resolved once: the rendering, the refusals and the brief's own id read
    // the id the store answered, never the part of it that was typed.
    let record = store.show(item)?;
    let item = record.id.as_str();
    let Some(index) = record.orders.as_ref() else {
        return Err(Stop::refused(if record.has_orders_key {
            format!(
                "{item}'s order index is not an object — a brief read off an order nobody can \
                 read would tell a seat it may begin when nothing said so"
            )
        } else {
            format!(
                "{item} carries no order index — a brief for an unordered item would tell a seat \
                 it may begin when nothing said so"
            )
        }));
    };
    if index.by.is_none() {
        return Err(Stop::refused(format!(
            "{item}'s order index names no dispatcher — the brief's order says who gave it, and \
             the record does not say who that is"
        )));
    }
    let order = order_text(index);
    let text = show::render(&record, &store.timeline(item)?);
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
            touched,
        },
    )
}

/// A command a caller handed in, or the named absence in its place. A blank
/// command is no command: a brief that printed an empty block would tell a
/// seat it had been given a check.
fn command_or<'a>(given: Option<&'a str>, absent: &'a str) -> &'a str {
    given
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .unwrap_or(absent)
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
