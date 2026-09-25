//! `fleet event tail` and `fleet event show`: the reader half of the event
//! family.
//!
//! STDOUT CARRIES THE STREAM AND NOTHING ELSE — the lines as stored for a tail,
//! the one pretty-printed event for a show — and every human-facing line goes to
//! stderr, the split `routines.rs`'s header states. A script piping this verb wants
//! the stream's own bytes back, and a note about which sequence a stamp resolved
//! to is not one of them.
//!
//! UNDER `--json` STDOUT CARRIES THE ENVELOPE INSTEAD: one document per event
//! for a tail, one for a show, and the refusal document where the flagless verb
//! prints only its stderr line. These are the two stream reads the SDK replays
//! from, so the document is the record whole and not the file's bytes; the
//! exit codes are the same either way, and the stderr line is printed under the
//! flag too, because a person watching a run still reads it.
//!
//! Both verbs are read-only: the controller reads and this module routes.

use crate::envelope;
use crate::exit::Exit;
use crate::item;
use fleet_controller::events::{self, ActorRef, RawLine};
use fleet_controller::routines::load;
use fleet_controller::{config, platform};
use fleet_core::item::table_at;
use fleet_core::seat::actor::Actor;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The stream, under the machine directory the writer verbs resolve.
const STREAM: &str = "events.jsonl";

/// How many lines a tail prints when the caller names no `--since`.
const TAIL_DEFAULT: usize = 50;

/// How often a follower asks the file whether it has grown.
const FOLLOW_INTERVAL: Duration = Duration::from_millis(250);

/// What a tail is filtered and cursored by. One value each, and the three
/// filters combine as an AND.
#[derive(clap::Args)]
pub struct TailArgs {
    /// print new lines as they are appended, until interrupted
    #[arg(long)]
    pub follow: bool,

    /// a sequence, or a stamp for the first line at or after it
    #[arg(long, value_name = "SEQ")]
    pub since: Option<String>,

    /// keep only the lines about this seat (id, name or 8+ hex)
    #[arg(long, value_name = "SEAT")]
    pub seat: Option<String>,

    /// keep only the lines this actor wrote
    #[arg(long, value_name = "KIND:ID")]
    pub actor: Option<String>,

    /// keep only the lines of this type
    #[arg(long = "type", value_name = "TYPE")]
    pub kind: Option<String>,

    /// print each line as the JSON envelope, one per event
    #[arg(long)]
    pub json: bool,
}

pub fn tail(args: &TailArgs) -> Exit {
    let path = machine_stream();
    if !path.is_file() {
        return no_stream("tail", &path, args.json);
    }

    // The two actor filters, resolved ONCE before anything is read: a seat
    // argument through the resolver, and a typed actor as given.
    let filters = match Filters::of(args) {
        Ok(filters) => filters,
        Err((exit, why)) => return refused("event tail", exit, &why, args.json),
    };

    // The cursor is EXCLUSIVE — `--since 150` is every line above 150 — and a
    // stamp is inclusive of the line it resolves to, because an event at the
    // stamp a caller named is one they asked for.
    let after = match args.since.as_deref() {
        None => 0,
        Some(value) => match value.parse::<u64>() {
            Ok(seq) => seq,
            Err(_) => match events::seq_at_or_after(&path, value) {
                Err(_) => {
                    let why = format!(
                        "--since {value} is neither a sequence nor a stamp of the shape {}",
                        events::STAMP_SHAPE
                    );
                    return refused("event tail", Exit::Usage, &why, args.json);
                }
                Ok(Some(seq)) => {
                    eprintln!("--since {value} resolved to {seq}");
                    seq.saturating_sub(1)
                }
                Ok(None) => {
                    eprintln!("--since {value} resolved to none");
                    u64::MAX
                }
            },
        },
    };

    let (lines, mut offset) = events::read_lines_from(&path, 0, after);
    let kept: Vec<&RawLine> = lines.iter().filter(|line| filters.keeps(line)).collect();
    // FILTER FIRST, THEN THE LAST FIFTY that survive, so `--seat` on a busy
    // stream still answers fifty of that seat's lines.
    let from = if args.since.is_some() {
        0
    } else {
        kept.len().saturating_sub(TAIL_DEFAULT)
    };
    for line in &kept[from..] {
        say(line, args.json);
    }

    if !args.follow {
        return Exit::Done;
    }

    // SIGINT ends the follow with exit 0, through the flag the platform layer
    // already arms — the controller's own loop reads the same one, and the
    // handler stores a bool and touches nothing else.
    platform::install_stop_handler();
    while !platform::stop_requested() {
        std::thread::sleep(FOLLOW_INTERVAL);
        // The cursor is a byte offset and the sequence filter is spent: every
        // line appended from here is above it by construction, and a follower
        // that re-applied the `--since` of a stamp resolved to none would print
        // nothing for the life of the follow.
        let (fresh, moved) = events::read_lines_from(&path, offset, 0);
        offset = moved;
        for line in fresh.iter().filter(|line| filters.keeps(line)) {
            say(line, args.json);
        }
    }
    Exit::Done
}

/// One kept line on stdout: the bytes as stored, or the envelope carrying the
/// record whole.
fn say(line: &RawLine, json: bool) {
    if json {
        println!(
            "{}",
            envelope::ok("event tail", &record(line.seq, &stored(line)))
        );
    } else {
        println!("{}", line.text);
    }
}

/// The stored line parsed back. A [`RawLine`] carries the two filter fields and
/// the bytes and neither the id nor the payload, so the document path reads the
/// line again — where the flagless tail's contract is those bytes exactly and
/// never a re-serialization.
///
/// A line that does not parse never reaches here: the reader that built the
/// [`RawLine`] parsed it to find the sequence. The fallback is `null`, whose
/// fields [`record`] reads as absent.
fn stored(line: &RawLine) -> serde_json::Value {
    serde_json::from_str(&line.text).unwrap_or(serde_json::Value::Null)
}

/// One stream line as the envelope's `data`: every field the fold's record
/// carries, under that record's own names — `kind` for the line's stored
/// `type`, and the sequence from the reader that numbered it.
///
/// A field the line does not carry is null and not dropped, so a caller folding
/// the document meets a key it can read as absent rather than a shape that
/// varies line to line.
fn record(seq: u64, stored: &serde_json::Value) -> serde_json::Value {
    let field = |name: &str| stored.get(name).cloned().unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "id": field("id"),
        "seq": seq,
        "ts": field("ts"),
        "kind": field("type"),
        "actor": field("actor"),
        "payload": field("payload"),
    })
}

/// The tail's three filters, each resolved to the value a stored line is
/// compared with.
struct Filters<'a> {
    /// `--seat`, resolved to the seat it names: `{seat, <id>}`.
    seat: Option<ActorRef>,
    /// `--actor`, read as a typed actor.
    actor: Option<ActorRef>,
    kind: Option<&'a str>,
}

impl<'a> Filters<'a> {
    /// The filters, or the refusal and its row of the exit table.
    ///
    /// `--seat` is resolved as every seat argument is — its id, eight or more
    /// of its hex digits, its name or its machine name — over the seats this
    /// fleet lists: the policy's roster, the machine's rows and this machine's
    /// identity. A miss or an ambiguity is the resolver's own refusal, exit 1,
    /// and an empty argument is exit 2. `--actor` must be typed, `<kind>:<id>`,
    /// and anything else is exit 2.
    fn of(args: &'a TailArgs) -> Result<Filters<'a>, (Exit, String)> {
        let seat = match args.seat.as_deref() {
            None => None,
            Some(arg) => Some(seat_named(arg)?),
        };
        let actor = match args.actor.as_deref() {
            None => None,
            Some(text) => match Actor::typed(text) {
                Some(Ok(actor)) => Some(item::stream_actor(&actor)),
                Some(Err(why)) => return Err((Exit::Usage, format!("--actor {why}"))),
                None => {
                    return Err((Exit::Usage, format!("--actor {text} is not kind:id")));
                }
            },
        };
        Ok(Filters {
            seat,
            actor,
            kind: args.kind.as_deref(),
        })
    }

    /// Whether a stored line survives the filters.
    ///
    /// A line the stream carries with no actor or no type is EXCLUDED by a
    /// filter it cannot answer and printed when no filter is given: it is a line
    /// of the stream either way, and a filter is a claim about a field it has.
    /// An actor matches exactly, kind and id both, so a run whose id is a
    /// seat's own is never a line about that seat.
    fn keeps(&self, line: &RawLine) -> bool {
        matches(self.seat.as_ref(), line.actor.as_ref())
            && matches(self.actor.as_ref(), line.actor.as_ref())
            && matches(self.kind, line.kind.as_deref())
    }
}

fn matches<T: PartialEq + ?Sized>(wanted: Option<&T>, stored: Option<&T>) -> bool {
    match wanted {
        None => true,
        Some(wanted) => stored == Some(wanted),
    }
}

/// The one seat `--seat` names, as the actor a line about it carries.
fn seat_named(arg: &str) -> Result<ActorRef, (Exit, String)> {
    let machine_dir = platform::machine_dir();
    let rows = config::read(&machine_dir.join("config.json"))
        .map(|machine| machine.seats)
        .unwrap_or_default();
    let policy = load::fleet_root(None, &machine_dir)
        .map(|root| table_at(&root.join("fleet.toml")))
        .unwrap_or_default();
    config::directory(&rows, &policy, &machine_dir)
        .resolve_listed(arg)
        .map(|seat| ActorRef::seat(seat.id))
        .map_err(|unresolved| {
            (
                Exit::from_status(unresolved.code()).unwrap_or(Exit::Refused),
                format!("--seat {unresolved}"),
            )
        })
}

pub fn show(id: &str, json: bool) -> Exit {
    let path = machine_stream();
    if !path.is_file() {
        return no_stream("show", &path, json);
    }

    match events::find_by_id(&path, id).as_slice() {
        [] => refused("event show", Exit::Refused, &format!("no event {id}"), json),
        // The one place a re-serialization is the contract: a person reading one
        // event wants it laid out, where a tail wants the bytes as stored.
        [one] if !json => match serde_json::to_string_pretty(&one.value) {
            Ok(text) => {
                println!("{text}");
                Exit::Done
            }
            Err(e) => {
                eprintln!("fleet event show: {id} is on the stream and cannot be printed: {e}");
                Exit::CouldNotTell
            }
        },
        [one] => {
            println!(
                "{}",
                envelope::ok("event show", &record(one.seq, &one.value))
            );
            Exit::Done
        }
        // Ids are unique by construction, so a duplicate is a corrupted stream
        // and a fact worth stopping on. It answers in the usage row, exit 2,
        // naming both sequences — the exit table has no corruption row and this
        // verb does not add one.
        many => {
            let seqs: Vec<String> = many.iter().map(|m| m.seq.to_string()).collect();
            let why = format!(
                "{id} is on more than one line — sequences {}",
                seqs.join(", ")
            );
            refused("event show", Exit::Usage, &why, json)
        }
    }
}

fn machine_stream() -> PathBuf {
    platform::machine_dir().join(STREAM)
}

/// The event readers' stream refusal: exit 5, with the path a caller would have
/// to look at. It is deliberately not `routine history`'s answer to the same
/// absence, which is a stderr line and exit 0; that verb keeps its own answer.
fn no_stream(verb: &str, path: &Path, json: bool) -> Exit {
    let why = format!("no event stream at {}", path.display());
    refused(&format!("event {verb}"), Exit::NoCollector, &why, json)
}

/// A refusal said once: the human's line on stderr always, the envelope on
/// stdout when the caller asked for the document, and the exit table's own row
/// either way.
fn refused(verb: &str, exit: Exit, why: &str, json: bool) -> Exit {
    eprintln!("fleet {verb}: {why}");
    if json {
        println!("{}", envelope::refusal(verb, exit, why));
    }
    exit
}
