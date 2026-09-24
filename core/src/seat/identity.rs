//! What a seat IS: its id, its kind and its name.
//!
//! THE ID IS THE SEAT. A seat is `{id, kind, name?}`, and the id is a UUID v7
//! minted once and stored whole: an id cut down to what is unique inside one
//! fleet stops being unique the day seats carry into a workspace or onto
//! another machine. The name is a person's, optional, and free to change,
//! which is why nothing is keyed by it.
//!
//! THE SHORT ID IS THE TAIL [ASSUMES D1]. A v7's first eight hex digits are the
//! top of its millisecond timestamp — a clock that ticks once every 65,536 ms,
//! so every seat minted in the same minute shares them — and its last eight
//! are random. A seat shows as its last eight, and its machine-facing name is
//! `<slug>-<short>`: the name reduced to `[a-z0-9-]`, or the kind where there
//! is none.
//!
//! Four readers share the one definition: [`roster`] turns fleet.toml's
//! `[seats.<id>]` tables into seats and refuses the shapes the clean break
//! retired, [`resolve`] turns any seat argument into exactly one seat or the
//! reason it cannot, [`read_identity`] and [`identity_or_mint`] read and mint
//! the machine's own `identity.toml`, and [`seat_table`] is the text one
//! appended row takes.

use std::fmt;
use std::io::Write;
use std::path::Path;

use crate::item::Stop;

// ---- the id -------------------------------------------------------------------

/// A seat's id: a UUID, held whole.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct SeatId(uuid::Uuid);

impl SeatId {
    /// A fresh v7, from this machine's clock and its random source.
    pub fn mint() -> SeatId {
        SeatId(uuid::Uuid::now_v7())
    }

    /// The 36-character hyphenated form, in either case and of any UUID
    /// version, and nothing else: the simple, braced and URN forms are refused,
    /// because a seat id a person pastes is the one a record showed them.
    pub fn parse(text: &str) -> Result<SeatId, String> {
        let trimmed = text.trim();
        let refused =
            || format!("{trimmed} is not a seat id — a seat id is 36 characters, 8-4-4-4-12 hex");
        if trimmed.len() != 36 {
            return Err(refused());
        }
        uuid::Uuid::try_parse(trimmed)
            .map(SeatId)
            .map_err(|_| refused())
    }

    /// The last eight characters of the displayed id [ASSUMES D1].
    pub fn short(&self) -> String {
        let shown = self.to_string();
        shown[shown.len() - 8..].to_string()
    }
}

impl fmt::Display for SeatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

impl serde::Serialize for SeatId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for SeatId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<SeatId, D::Error> {
        let text = String::deserialize(deserializer)?;
        SeatId::parse(&text).map_err(serde::de::Error::custom)
    }
}

// ---- the seat -----------------------------------------------------------------

/// Who holds a seat: a person or an agent.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Human,
    Agent,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Human => "human",
            Kind::Agent => "agent",
        }
    }

    /// The two words exactly, in lowercase, after a trim.
    pub fn parse(text: &str) -> Option<Kind> {
        match text.trim() {
            "human" => Some(Kind::Human),
            "agent" => Some(Kind::Agent),
            _ => None,
        }
    }
}

/// One seat, as every reader names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatRef {
    pub id: SeatId,
    pub name: Option<String>,
    pub kind: Kind,
}

impl SeatRef {
    /// The name made safe for a directory and a branch: lowercased, every
    /// character outside `[a-z0-9]` a `-`, runs of `-` collapsed and the ends
    /// trimmed. A seat with no name, or one that reduces to nothing, takes its
    /// kind.
    pub fn slug(&self) -> String {
        let mut slug = String::new();
        for c in self
            .name
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase()
            .chars()
        {
            let c = if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            };
            if c == '-' && slug.ends_with('-') {
                continue;
            }
            slug.push(c);
        }
        let slug = slug.trim_matches('-');
        if slug.is_empty() {
            self.kind.as_str().to_string()
        } else {
            slug.to_string()
        }
    }

    /// `<slug>-<short>`: what a home, a branch and a row are named.
    pub fn machine_name(&self) -> String {
        format!("{}-{}", self.slug(), self.id.short())
    }

    /// How a refusal lists this seat: the name a person reads and the id a
    /// script can pass.
    fn listed(&self) -> String {
        format!("{} ({})", self.machine_name(), self.id)
    }
}

/// THE ONE MACHINE-READABLE SEAT: `{"id", "name", "kind"}`, in that order, the
/// name absent — never null — where the seat has none. Every event payload and
/// `--json` document that names a seat carries this object, so a reader keys
/// on the id and never has to parse a machine name back apart.
impl serde::Serialize for SeatRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let fields = if self.name.is_some() { 3 } else { 2 };
        let mut object = serializer.serialize_struct("Seat", fields)?;
        object.serialize_field("id", &self.id)?;
        match &self.name {
            Some(name) => object.serialize_field("name", name)?,
            None => object.skip_field("name")?,
        }
        object.serialize_field("kind", self.kind.as_str())?;
        object.end()
    }
}

/// One `[seats.<id>]` table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub seat: SeatRef,
    pub model: Option<String>,
    /// True for `parked` and its aliases: a seat the fleet keeps and does not
    /// run.
    pub parked: bool,
}

// ---- the roster, as fleet.toml states it --------------------------------------

/// What a `[seats.<id>]` table says. `status` is `active`, `parked` or one of
/// [`SEAT_PARKED_ALIASES`], and a row that says nothing is active.
pub const SEAT_ACTIVE: &str = "active";
pub const SEAT_PARKED: &str = "parked";

/// The harness's own words for a parked seat, spelled once. One file serves
/// both readers, so a seat on vacation (kept, no worktree) and one chartered
/// (on paper, never yet started) are `parked` here: kept and not run.
pub const SEAT_PARKED_ALIASES: [&str; 2] = ["vacationing", "chartered"];

/// Every `[seats.<id>]` table in a policy already parsed, in id order — which
/// for v7 ids is the order they were minted in.
///
/// THE CLEAN BREAK IS REFUSED, NOT READ AROUND. A table keyed by a name and a
/// row carrying `chosen_name` are the shapes seat identity retired, and each
/// is refused naming what replaced it: a reader that skipped them would start
/// a fleet short of the seats its person wrote down. A key this reader does
/// not know is ignored.
pub fn roster(policy: &toml::Table) -> Result<Vec<Seat>, String> {
    let seats = match policy.get("seats") {
        None => return Ok(Vec::new()),
        Some(toml::Value::Table(seats)) => seats,
        Some(_) => return Err(String::from("seats is not a table")),
    };
    seats.iter().map(|(key, row)| seat_row(key, row)).collect()
}

/// [`roster`] over a file's text.
pub fn roster_in(body: &str) -> Result<Vec<Seat>, String> {
    let policy: toml::Table = toml::from_str(body).map_err(|e| e.to_string())?;
    roster(&policy)
}

fn seat_row(key: &str, row: &toml::Value) -> Result<Seat, String> {
    let id = SeatId::parse(key).map_err(|_| {
        format!(
            "[seats.{key}] is keyed by a name — a seat is keyed by its id now; \
             fleet seat add --agent --name {key} mints one"
        )
    })?;
    let row = row
        .as_table()
        .ok_or_else(|| format!("[seats.{id}] is not a table"))?;
    let text = |key: &str| string_at(row, key, &format!("[seats.{id}]"));

    let kind = match text("kind")? {
        None => {
            return Err(format!(
                "[seats.{id}] carries no kind — say kind = \"agent\" or kind = \"human\""
            ))
        }
        Some(said) => Kind::parse(&said)
            .ok_or_else(|| format!("[seats.{id}] kind = \"{said}\" is neither agent nor human"))?,
    };
    let name = blank_is_none(text("name")?);
    if row.contains_key("chosen_name") {
        return Err(format!(
            "[seats.{id}] carries chosen_name, which is name now"
        ));
    }
    // A blank is no value, exactly as it is in `[controller]`: an empty model
    // reaches the agent as a flag whose argument is the next flag.
    let model = blank_is_none(text("model")?);
    let status = blank_is_none(text("status")?);
    if kind == Kind::Human {
        // [ASSUMES D15] A person is not started on a model and is not parked:
        // either key on a human row is a row written for somebody else.
        if model.is_some() {
            return Err(format!(
                "[seats.{id}] is a human seat and carries model — only an agent seat runs on a model"
            ));
        }
        if status.is_some() {
            return Err(format!(
                "[seats.{id}] is a human seat and carries status — only an agent seat is parked"
            ));
        }
    }
    Ok(Seat {
        parked: parked_status(&id.to_string(), status.as_deref())?,
        seat: SeatRef { id, name, kind },
        model,
    })
}

/// A key's string value, `None` where the key is absent, and a refusal where
/// it holds anything but a string.
fn string_at(row: &toml::Table, key: &str, table: &str) -> Result<Option<String>, String> {
    match row.get(key) {
        None => Ok(None),
        Some(toml::Value::String(said)) => Ok(Some(said.clone())),
        Some(other) => Err(format!("{table} {key} = {other} is not a string")),
    }
}

/// Trimmed, and a blank is no value.
fn blank_is_none(said: Option<String>) -> Option<String> {
    said.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The status, read from the four admitted words and from absence — which is
/// active.
///
/// A FIFTH VALUE IS REFUSED rather than rounded. Rounding an unrecognised word
/// to active starts a seat the person wrote `status = "park"` to keep still, and
/// that is the one mistake this key exists to prevent; a blank is the same as
/// saying nothing, because a key with no value in it was not an answer either.
fn parked_status(id: &str, status: Option<&str>) -> Result<bool, String> {
    match status.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(false),
        Some(said) if said.eq_ignore_ascii_case(SEAT_ACTIVE) => Ok(false),
        Some(said) if said.eq_ignore_ascii_case(SEAT_PARKED) => Ok(true),
        Some(said)
            if SEAT_PARKED_ALIASES
                .iter()
                .any(|alias| said.eq_ignore_ascii_case(alias)) =>
        {
            Ok(true)
        }
        Some(said) => {
            let aliases = SEAT_PARKED_ALIASES.join("`, `");
            Err(format!(
                "[seats.{id}] status = \"{said}\" is none of `{SEAT_ACTIVE}`, `{SEAT_PARKED}`, \
                 `{aliases}` — a status this reader does not know is refused rather than read as \
                 active, because reading it as active starts a seat somebody meant to keep still"
            ))
        }
    }
}

/// The text one appended `[seats.<id>]` table takes: the kind always, the name
/// and the model only where there is one. It opens on a blank line, so it can
/// be appended to any file that ends in a newline.
pub fn seat_table(seat: &SeatRef, model: Option<&str>) -> String {
    let mut table = format!("\n[seats.{}]\nkind = \"{}\"\n", seat.id, seat.kind.as_str());
    if let Some(name) = &seat.name {
        table.push_str(&format!("name = \"{}\"\n", basic(name)));
    }
    if let Some(model) = model {
        table.push_str(&format!("model = \"{}\"\n", basic(model)));
    }
    table
}

/// One value inside a TOML basic string — a private copy of the controller's
/// `lifecycle::basic`, because this crate may not depend on the controller.
///
/// A name is whatever a person typed, and a quote or a backslash in one writes
/// a file that does not parse. The set is the format's own: the two delimiters
/// and the control characters, each as the short escape where it has one.
fn basic(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

// ---- the resolver -------------------------------------------------------------

/// Why a seat argument names no one seat. Each carries the exit it answers:
/// an empty argument is the call's fault, and a seat nobody holds or two that
/// both answer is a refusal about the fleet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unresolved {
    Blank,
    Missing {
        arg: String,
        known: Vec<String>,
    },
    Ambiguous {
        arg: String,
        candidates: Vec<String>,
    },
}

impl Unresolved {
    pub fn code(&self) -> u8 {
        match self {
            Unresolved::Blank => crate::item::USAGE,
            Unresolved::Missing { .. } | Unresolved::Ambiguous { .. } => crate::item::REFUSED,
        }
    }
}

impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unresolved::Blank => f.write_str("names no seat — the argument is empty"),
            Unresolved::Missing { arg, known } => {
                let known = if known.is_empty() {
                    String::from("none")
                } else {
                    known.join(", ")
                };
                write!(f, "{arg} names no seat — the seats are {known}")
            }
            Unresolved::Ambiguous { arg, candidates } => write!(
                f,
                "{arg} names {} seats — {} — say more of the id",
                candidates.len(),
                candidates.join(", ")
            ),
        }
    }
}

impl From<Unresolved> for Stop {
    fn from(unresolved: Unresolved) -> Stop {
        match unresolved.code() {
            crate::item::USAGE => Stop::usage(unresolved.to_string()),
            _ => Stop::refused(unresolved.to_string()),
        }
    }
}

/// The index of the one seat an argument names.
///
/// A WHOLE ID IS EXACT. An argument that parses as a full seat id is that seat
/// or no seat, and no fragment rule runs, so an id nobody holds is never read
/// as a prefix of one somebody does.
///
/// Anything else names every seat it matches, by any of three rules: the name,
/// without regard to ASCII case; a machine name, matched on its id part alone
/// so a rename moves nothing; and eight or more characters of the id, from its
/// front or its back. One seat is the answer, none is missing and more than one
/// is ambiguous — a resolver that guessed would act as a seat nobody named.
pub fn resolve(seats: &[SeatRef], arg: &str) -> Result<usize, Unresolved> {
    let arg = arg.trim();
    if arg.is_empty() {
        return Err(Unresolved::Blank);
    }
    let missing = || Unresolved::Missing {
        arg: arg.to_string(),
        known: seats.iter().map(SeatRef::listed).collect(),
    };

    if let Ok(whole) = SeatId::parse(arg) {
        return seats.iter().position(|s| s.id == whole).ok_or_else(missing);
    }

    let machine_short = arg
        .rsplit_once('-')
        .map(|(_, tail)| tail)
        .filter(|tail| tail.len() == 8 && tail.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase);
    let fragment = (arg.len() >= 8 && arg.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| arg.to_ascii_lowercase());

    let mut found: Vec<usize> = Vec::new();
    for (index, seat) in seats.iter().enumerate() {
        let full = seat.id.to_string();
        let matched = seat
            .name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(arg))
            || machine_short.as_deref() == Some(seat.id.short().as_str())
            || fragment
                .as_deref()
                .is_some_and(|f| full.starts_with(f) || full.ends_with(f));
        if matched && !found.iter().any(|&i| seats[i].id == seat.id) {
            found.push(index);
        }
    }

    match found.as_slice() {
        [] => Err(missing()),
        [one] => Ok(*one),
        many => Err(Unresolved::Ambiguous {
            arg: arg.to_string(),
            candidates: many.iter().map(|&i| seats[i].listed()).collect(),
        }),
    }
}

// ---- the directory ------------------------------------------------------------

/// Every seat a verb on this machine can name, in the two lists a verb asks.
///
/// LISTED IS WHO THE FLEET KNOWS: fleet.toml's roster, the machine's transient
/// rows — a spawned seat is on no roster — and this machine's own identity
/// where `identity.toml` exists. RUNNING IS WHAT THIS MACHINE RUNS: its
/// config.json rows, every one an agent's. Work is GIVEN only to a running
/// seat, and anybody listed may act on the record. The caller fills both;
/// reading them never mints.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Directory {
    pub listed: Vec<SeatRef>,
    pub running: Vec<SeatRef>,
}

impl Directory {
    /// The one running seat an argument names: an agent seat this machine
    /// runs, and never a person, who is listed and not run.
    pub fn resolve_running(&self, arg: &str) -> Result<&SeatRef, Unresolved> {
        resolve(&self.running, arg).map(|index| &self.running[index])
    }

    /// The one listed seat an argument names.
    pub fn resolve_listed(&self, arg: &str) -> Result<&SeatRef, Unresolved> {
        resolve(&self.listed, arg).map(|index| &self.listed[index])
    }

    /// How a sentence names a seat: the machine name of a listed or running
    /// seat, else the id itself — a seat since dropped is still somebody.
    pub fn label(&self, id: &SeatId) -> String {
        self.listed
            .iter()
            .chain(&self.running)
            .find(|seat| seat.id == *id)
            .map(SeatRef::machine_name)
            .unwrap_or_else(|| id.to_string())
    }

    /// The seat an actor string names: a typed `seat:<full id>` as given, and
    /// otherwise the one listed seat the text resolves to.
    ///
    /// A BRIDGE until the verbs take the typed actor. The cli hands a verb the
    /// typed form, and a seat's is taken as given, as the cli took it — the
    /// identity this very call minted is in no directory yet. Every other kind
    /// is no seat, and so is a missing or an ambiguous answer: the caller says
    /// so rather than guessing whose work it holds.
    pub fn seat_of(&self, by: &str) -> Option<SeatId> {
        match super::actor::Actor::typed(by) {
            Some(typed) => typed.ok().and_then(|actor| actor.seat_id()),
            None => self.resolve_listed(by).ok().map(|seat| seat.id),
        }
    }
}

// ---- the machine's own identity -----------------------------------------------

/// The file under the machine directory that says who this machine is.
pub const IDENTITY: &str = "identity.toml";

/// Who acts when a verb runs here with no `--by` and no `FLEET_ACTOR`: always a
/// person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineIdentity {
    pub id: SeatId,
    pub name: Option<String>,
}

impl MachineIdentity {
    pub fn as_ref(&self) -> SeatRef {
        SeatRef {
            id: self.id,
            name: self.name.clone(),
            kind: Kind::Human,
        }
    }
}

/// The machine's identity, or `None` where it has none yet.
///
/// A file that is there and cannot be read as a person's id is refused naming
/// its path, never re-minted: the id in it may already be on the record, and a
/// fresh one would make whoever acted before a stranger.
pub fn read_identity(machine_dir: &Path) -> Result<Option<MachineIdentity>, String> {
    let path = machine_dir.join(IDENTITY);
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    identity_in(&body)
        .map(Some)
        .map_err(|why| format!("{}: {why}", path.display()))
}

fn identity_in(body: &str) -> Result<MachineIdentity, String> {
    let file: toml::Table = toml::from_str(body).map_err(|e| e.to_string())?;
    let text = |key: &str| string_at(&file, key, "the identity's");
    let id = match text("id")? {
        None => return Err(String::from("carries no id")),
        Some(said) => SeatId::parse(&said)?,
    };
    match text("kind")? {
        Some(said) if Kind::parse(&said) == Some(Kind::Human) => {}
        Some(said) => {
            return Err(format!(
                "kind = \"{said}\" — a machine's identity is a person, kind = \"human\""
            ))
        }
        None => {
            return Err(String::from(
                "carries no kind — a machine's identity is a person, kind = \"human\"",
            ))
        }
    }
    Ok(MachineIdentity {
        id,
        name: blank_is_none(text("name")?),
    })
}

/// The machine's identity, minted where it has none; the flag is true when THIS
/// call minted it.
///
/// THE MINT NEVER CLOBBERS. The file is written whole and synced under a name
/// of its own, then hard-linked into place — a link to a name that exists
/// fails rather than replaces, so two processes minting at once leave exactly
/// one id, and the one that lost reads the winner's.
pub fn identity_or_mint(machine_dir: &Path) -> Result<(MachineIdentity, bool), String> {
    if let Some(known) = read_identity(machine_dir)? {
        return Ok((known, false));
    }
    let path = machine_dir.join(IDENTITY);
    let failed = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(machine_dir).map_err(failed)?;

    let minted = SeatId::mint();
    // The pid says which process left a stray one; the short id keeps two
    // threads of one process off each other's file.
    let tmp = machine_dir.join(format!(
        "{IDENTITY}.{}.{}.tmp",
        std::process::id(),
        minted.short()
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(identity_text(&minted).as_bytes())?;
            file.sync_all()
        });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", tmp.display()));
    }
    let linked = std::fs::hard_link(&tmp, &path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok((
            MachineIdentity {
                id: minted,
                name: None,
            },
            true,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            match read_identity(machine_dir)? {
                Some(theirs) => Ok((theirs, false)),
                None => Err(format!(
                    "{}: another process wrote it and it is gone again",
                    path.display()
                )),
            }
        }
        Err(e) => Err(failed(e)),
    }
}

fn identity_text(id: &SeatId) -> String {
    format!(
        "# This machine's identity: who acts when a fleet verb runs here with no\n\
         # --by and no FLEET_ACTOR. The id is the key; add name = \"...\" if you\n\
         # want one. Written by fleet.\n\
         id = \"{id}\"\n\
         kind = \"human\"\n"
    )
}
