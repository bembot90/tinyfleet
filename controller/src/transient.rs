//! The transient-seat primitives (PRD R30–R32; cli PRD § Seat).
//!
//! Three verbs — `fleet seat spawn`, `fleet seat feed`, `fleet seat retire` —
//! as three functions over the machine directory, the adapter, the policy, the
//! seat list and the session table. They are the primitives a pack's `dispatch`
//! calls: the order note, the brief's content and the assignment belong to the
//! caller, and NOTHING HERE TOUCHES THE WORK GRAPH.
//!
//! Everything a verb cannot work out for itself is gathered by the caller into
//! [`Machine`], the way `effect.rs` gathers a seat into its `Target`: the
//! project and its two directories come from the project's own policy file,
//! which this crate cannot read because it takes nothing from core but its
//! bounded runner (`fleet_core::process`) and the release it supports
//! (`fleet_core::supported`). The stream and the session table are opened here
//! from the machine directory the caller named, because a verb
//! is one process and there is no loop holding them across a poll.
//!
//! ## The belt's two environment overrides
//!
//! `FLEET_LOAD_AVERAGE` and `FLEET_CPUS` are read by [`Readings::taken`] and by
//! nothing else in this crate. They exist so a suite can force both readings —
//! the way the drive suite forces the agent binary with `FLEET_CLAUDE_BIN` —
//! because an arm that read the real load average is one whose answer changes
//! with whatever else the machine is running. A BINARY is what calls `taken`:
//! an in-process caller hands the two numbers in on [`Machine`] instead,
//! because setting a variable in a process that is forking children races the
//! fork.

use crate::adapter::{dir_key, Agent, RemoveAnswer, RosterRead};
use crate::clock::Clock;
use crate::config::{self, Seat};
use crate::effect::{self, Outcome, Target};
use crate::events::{self, EventLog};
use crate::platform;
use crate::policy::Policy;
use crate::sessions::{self, Table};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The rows of the exit table a REFUSAL can carry, as the cli PRD names them.
/// Done is not among them: a verb that answers `Ok` carries a value and not a
/// status, and the cli turns it into 0.
pub const REFUSED: u8 = 1;
pub const COULD_NOT_TELL: u8 = 3;
pub const NO_SESSION: u8 = 4;
/// The row is a named seat where a transient one was required. Named seats are
/// rung and rested; transient ones are fed and retired.
pub const NOT_TRANSIENT: u8 = 6;

/// A verb that stopped, with the status a script reads and the sentence a
/// person does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: u8,
    pub message: String,
}

impl Refusal {
    fn at(code: u8, message: impl Into<String>) -> Refusal {
        Refusal {
            code,
            message: message.into(),
        }
    }

    fn refused(message: impl Into<String>) -> Refusal {
        Refusal::at(REFUSED, message)
    }

    /// An instrument the answer needed would not answer. Never rounded into a
    /// clean verdict: a session nobody could ask is a question, not an absence.
    fn could_not_tell(message: impl Into<String>) -> Refusal {
        Refusal::at(COULD_NOT_TELL, message)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Everything the three verbs are told rather than read for themselves.
pub struct Machine<'a> {
    pub machine_dir: &'a Path,
    pub agent: &'a dyn Agent,
    pub policy: &'a Policy,
    /// The project a transient seat's worktree is keyed under in the seat list.
    pub project: &'a str,
    /// The checkout every `git worktree` call is run from.
    pub primary: &'a Path,
    /// Where a transient seat's worktree is made.
    pub worktrees_dir: &'a Path,
    /// The belt's two machine readings, TAKEN BY THE CALLER like everything
    /// else here. [`Readings::taken`] is what a binary passes; an in-process
    /// caller hands the numbers in, which is what keeps the two overrides —
    /// the process's own environment, shared by every thread — out of a suite
    /// that runs arms in parallel and forks children while they run.
    pub readings: Readings,
    /// The clock the one wait here whose subject is a DURATION is spent
    /// against — [`cleared`]'s start-watch window. Every other wait a verb
    /// reaches is on a real child's exit and stays the thread's.
    pub clock: &'a dyn Clock,
}

impl Machine<'_> {
    fn config_path(&self) -> PathBuf {
        self.machine_dir.join("config.json")
    }

    fn stream_path(&self) -> PathBuf {
        self.machine_dir.join("events.jsonl")
    }

    fn table_path(&self) -> PathBuf {
        sessions::path_in(self.machine_dir)
    }

    /// The configuration directory a spawned seat of this name comes up under
    /// (flights PRD R13, Q1e): one per seat, under the machine directory, so
    /// nothing from the person's home directory reaches a flight.
    ///
    /// The LOCATION is the machine directory's and no policy key names it: a
    /// second spelling of it is a second place a stale value can live.
    fn config_dir_for(&self, seat: &str) -> PathBuf {
        self.machine_dir.join(CONFIG_DIRS).join(seat)
    }

    /// The directory a seat's own session row names, where it names one.
    fn recorded_config_dir(&self, seat: &str) -> Option<String> {
        sessions::read(&self.table_path())
            .0?
            .newest_for(seat)?
            .config_dir
            .clone()
    }

    /// The seat list as the verbs read it. An unreadable one is could-not-tell:
    /// a verb that treated it as an empty fleet would call every named seat
    /// transient.
    fn seats(&self) -> Result<Vec<Seat>, Refusal> {
        match config::read(&self.config_path()) {
            Ok(machine) => Ok(machine.seats),
            Err(why) => Err(Refusal::could_not_tell(format!(
                "the seat list could not be read, so this verb cannot tell a transient row \
                 from a named one: {why}"
            ))),
        }
    }

    /// The table this controller wrote, off the disk and under no lock.
    ///
    /// A TABLE THAT WOULD NOT PARSE IS A REFUSAL AND NEVER AN EMPTY ONE.
    /// `sessions::read`'s `None` means nothing came off the disk, which covers
    /// a file that is absent as squarely as one that is corrupt or of a schema
    /// this build refuses — and rounding the second to an empty table renames it
    /// over the real one, taking every other seat's row, the halt latch and the
    /// cursor with it. The two are told apart by the cause beside it: an absent
    /// file carries none and is the empty table it already is.
    ///
    /// Never rebuilt from the stream here: a rebuild is the loop's act at
    /// startup, and a verb that rebuilt would write a table the running
    /// controller did not agree to.
    ///
    /// A COPY READ HERE IS ONLY EVER A READING, never the basis of a write: the
    /// file can move before the write lands, so what goes back to disk is built
    /// on [`Machine::table_under_lock`]'s copy and nothing else.
    fn table_now(&self) -> Result<Table, Refusal> {
        match sessions::read(&self.table_path()) {
            (Some(table), _) => Ok(table),
            (None, None) => Ok(Table::default()),
            (None, Some(cause)) => Err(Refusal::could_not_tell(format!(
                "the session table could not be read, and a table nobody could read is not an \
                 empty fleet — nothing was written: {cause}"
            ))),
        }
    }

    /// The same table read UNDER THE LOCK that guards a read-modify-write of
    /// it, with the lock held for as long as the returned handle lives.
    ///
    /// A rename is atomic and a read-modify-write is not, which is the hazard
    /// the seat list's own lock exists for: two verbs that each read this table
    /// and then rename their own version over it leave one of the two edits.
    ///
    /// THE READ IS INSIDE THE LOCK and is never hoisted out of it. A verb that
    /// edits a copy it read before taking the lock writes back every other
    /// seat's row as that copy carried it, which loses whatever landed in
    /// between — the same lost edit the lock exists to stop, moved one step.
    fn table_under_lock(&self) -> Result<(std::fs::File, Table), Refusal> {
        let path = self.table_path();
        let held = platform::lock_beside(&path).map_err(|e| {
            Refusal::could_not_tell(format!(
                "the session table's lock at {} could not be taken: {e}",
                path.display()
            ))
        })?;
        self.table_now().map(|table| (held, table))
    }

    /// The table written back under a lock the caller is holding. The handle is
    /// taken by reference so a caller cannot drop the lock and still write.
    fn write_table(&self, _held: &std::fs::File, table: &Table) -> Result<(), Refusal> {
        sessions::write(&self.table_path(), table).map_err(|e| {
            Refusal::could_not_tell(format!(
                "the session table at {} could not be written: {e}",
                self.table_path().display()
            ))
        })
    }

    fn log(&self) -> EventLog {
        EventLog::open(&self.stream_path())
    }

    /// One line on the stream, which is the controller's ONE ledger (R23, R25).
    ///
    /// A failed append is reported and never swallowed: the act it journals has
    /// already happened — the marker moved, the seat was reclaimed — so a
    /// silent failure leaves the machine changed and the record saying nothing.
    /// It is could-not-tell rather than a refusal, which is the same shape a
    /// probe that cannot answer takes: the act stands, and the message says
    /// which line did not land.
    fn journal(
        &self,
        log: &mut EventLog,
        kind: &str,
        seat: &str,
        payload: serde_json::Value,
    ) -> Result<(), Refusal> {
        log.append(kind, seat, payload).map_err(|e| {
            Refusal::could_not_tell(format!(
                "{kind} for `{seat}` could not be appended to {}, so the act stands and the \
                 ledger does not carry it: {e}",
                self.stream_path().display()
            ))
        })
    }

    /// One listing, once. `Unreadable` is could-not-tell on every verb that
    /// asks (lessons claude-code B4): a listing that answers with nothing while
    /// sessions are live must not read as an empty fleet.
    ///
    /// `config_dir` is one row's own configuration directory, and `None` the
    /// fleet's. A session started under its own is held by its own daemon and
    /// named by no other listing (flights PRD R13), so a verb about such a seat
    /// reads there or sees nothing at all — which is a probe that cannot fail.
    fn roster_under(
        &self,
        config_dir: Option<&Path>,
    ) -> Result<Vec<crate::adapter::AgentRow>, Refusal> {
        match self.agent.status(config_dir) {
            RosterRead::Readable(rows) => Ok(rows),
            RosterRead::Unreadable { cause } => Err(Refusal::could_not_tell(format!(
                "the roster could not be read, so no session can be named: {cause}"
            ))),
        }
    }

    /// The row of the seat list this name belongs to, with the two refusals a
    /// verb over a transient seat owes: a name nothing carries, and a named row
    /// where a transient one was required.
    fn transient_row(&self, seats: &[Seat], name: &str) -> Result<Seat, Refusal> {
        let Some(row) = seats.iter().find(|seat| seat.name == name) else {
            return Err(Refusal::refused(format!(
                "`{name}` is not a row of this machine's seat list"
            )));
        };
        if !row.transient {
            return Err(Refusal::at(
                NOT_TRANSIENT,
                format!(
                    "`{name}` is a named seat — named seats are rung and rested, and only a \
                     transient row is fed and retired"
                ),
            ));
        }
        Ok(row.clone())
    }

    /// This seat's worktree for this project, falling back to its first as the
    /// ring does: a row may hold worktrees for several projects, and the one
    /// this verb is about is the one to act in.
    fn worktree_of(&self, row: &Seat) -> Result<String, Refusal> {
        row.worktrees
            .iter()
            .find(|(project, _)| project == self.project)
            .or_else(|| row.worktrees.first())
            .map(|(_, path)| path.clone())
            .ok_or_else(|| {
                Refusal::refused(format!("`{}` carries no worktree to act in", row.name))
            })
    }
}

// ---- the load belt ----------------------------------------------------------

/// The variable a suite sets to force the five-minute load average the belt
/// reads (`platform::LOAD_SAMPLE`).
pub const LOAD_OVERRIDE: &str = "FLEET_LOAD_AVERAGE";
/// The variable a suite sets to force the processor count.
pub const CPUS_OVERRIDE: &str = "FLEET_CPUS";

/// The two numbers the load leg is computed from, as the caller took them.
///
/// `None` on either is a reading nobody has, which makes the load leg
/// could-not-tell — never a machine under no load.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Readings {
    pub load: Option<f64>,
    pub cpus: Option<u32>,
}

impl Readings {
    /// The two overrides first, then the platform. This is the ONE function in
    /// this crate that reads them, and a binary is its only caller: an
    /// in-process caller builds a `Readings` instead, because setting a
    /// variable in a process that is forking children races the fork.
    pub fn taken() -> Readings {
        Readings {
            load: override_f64(LOAD_OVERRIDE).or_else(platform::load_average_5m),
            cpus: override_u32(CPUS_OVERRIDE).or_else(platform::cpus),
        }
    }
}

/// Both readings the belt took, whichever of them refuses.
///
/// A person reading a refusal is deciding whether to wait, and they need the
/// other number to decide with — so the two legs are reported together on every
/// path out, including the one that lets the spawn through.
#[derive(Debug, Clone, PartialEq)]
pub struct Belt {
    /// The average, the ceiling it was judged against, and the cpu count that
    /// ceiling was computed from. `None` is a leg nobody could read.
    pub load: Option<(f64, f64, u32)>,
    /// Transient seats mid-turn and the cap. `None` is the cap leg
    /// could-not-tell, which refuses nothing.
    pub busy: Option<(u32, u32)>,
    /// Why the cap leg could not be judged, where it could not.
    pub busy_unreadable: Option<String>,
}

impl Belt {
    /// Both legs, read once.
    ///
    /// An unreadable roster makes the cap leg could-not-tell and REFUSES
    /// NOTHING — a daemon nobody can ask must not wedge every spawn in the
    /// fleet — while the load leg keeps its teeth.
    pub fn read(machine: &Machine, seats: &[Seat]) -> Belt {
        let Readings {
            load: reading,
            cpus,
        } = machine.readings;
        let load = match (reading, cpus) {
            (Some(load), Some(cpus)) if cpus > 0 => Some((
                load,
                cpus as f64 * machine.policy.load_ceiling_per_cpu,
                cpus,
            )),
            _ => None,
        };

        // THE FLEET'S LISTING, and one more per transient seat that came up under
        // its own configuration directory. Such a seat is held by its own daemon
        // and appears in no other listing (flights PRD R13), so a cap counted off
        // the fleet's read alone would be zero however many were mid-turn — a
        // ceiling that refuses nothing, which is what this leg exists to prevent.
        //
        // ONE UNREADABLE LISTING makes the whole leg could-not-tell, as one
        // unreadable roster did: a count short by an unknown number is not a
        // count, and a daemon nobody can ask must not wedge every spawn.
        //
        // The table is read ONCE, not once per seat: the directories all come
        // out of the same file and a second read could answer differently.
        let recorded = sessions::read(&machine.table_path()).0;
        let mut mid_turn = 0u32;
        let mut busy_unreadable = None;
        let mut count_in = |rows: &[crate::adapter::AgentRow], keys: &[String]| {
            mid_turn += rows
                .iter()
                .filter(|row| {
                    row.is_live() && row.is_busy() && keys.iter().any(|key| key == row.cwd_key())
                })
                .count() as u32;
        };

        // The seats whose sessions the fleet's own daemon holds: every transient
        // row that named no directory of its own.
        let shared: Vec<String> = seats
            .iter()
            .filter(|seat| seat.transient)
            .filter(|seat| {
                recorded
                    .as_ref()
                    .and_then(|table| table.newest_for(&seat.name))
                    .and_then(|row| row.config_dir.as_ref())
                    .is_none()
            })
            .flat_map(|seat| seat.worktrees.iter().map(|(_, path)| path.clone()))
            .map(|path| dir_key(&path).to_string())
            .collect();
        match machine.agent.status(None) {
            RosterRead::Readable(rows) => count_in(&rows, &shared),
            RosterRead::Unreadable { cause } => busy_unreadable = Some(cause),
        }

        for seat in seats.iter().filter(|seat| seat.transient) {
            let Some(config_dir) = recorded
                .as_ref()
                .and_then(|table| table.newest_for(&seat.name))
                .and_then(|row| row.config_dir.clone())
            else {
                continue;
            };
            let keys: Vec<String> = seat
                .worktrees
                .iter()
                .map(|(_, path)| dir_key(path).to_string())
                .collect();
            match machine.agent.status(Some(Path::new(&config_dir))) {
                RosterRead::Readable(rows) => count_in(&rows, &keys),
                RosterRead::Unreadable { cause } => busy_unreadable = Some(cause),
            }
        }
        let busy = match &busy_unreadable {
            Some(_) => None,
            None => Some((mid_turn, machine.policy.max_transient_busy)),
        };

        Belt {
            load,
            busy,
            busy_unreadable,
        }
    }

    /// The legs that are over their ceiling, in the order they are read.
    pub fn over(&self) -> Vec<&'static str> {
        let mut over = Vec::new();
        if let Some((load, ceiling, _)) = self.load {
            if load > ceiling {
                over.push("load average");
            }
        }
        if let Some((mid_turn, cap)) = self.busy {
            if mid_turn > cap {
                over.push("transient seats mid-turn");
            }
        }
        over
    }

    /// Both readings, one per line, in the shape a refusal prints them.
    pub fn lines(&self) -> String {
        let load = match self.load {
            Some((load, ceiling, cpus)) => format!(
                "{load:.2} (ceiling {ceiling:.2} = {cpus} cpu x {:.2})",
                ceiling / cpus as f64
            ),
            None => "COULD NOT TELL — no load average or cpu count on this host".to_string(),
        };
        let busy = match (&self.busy, &self.busy_unreadable) {
            (Some((mid_turn, cap)), _) => format!("{mid_turn} (cap {cap})"),
            (None, Some(cause)) => {
                format!("COULD NOT TELL — the roster could not be read, which is not zero: {cause}")
            }
            (None, None) => "COULD NOT TELL".to_string(),
        };
        format!("  load average (5m)       : {load}\n  transient seats mid-turn: {busy}")
    }

    /// Both readings as the stream carries them: the numbers this start was let
    /// through on, rather than the sentence a person reads.
    ///
    /// A LEG NOBODY COULD READ IS `null` AND NEVER A ZERO — a count that is not
    /// a count would fold as a quiet machine, which is the rounding this leg
    /// exists to refuse.
    pub fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "load": self.load.map(|(load, _, _)| load),
            "load_ceiling": self.load.map(|(_, ceiling, _)| ceiling),
            "cpus": self.load.map(|(_, _, cpus)| cpus),
            "mid_turn": self.busy.map(|(mid_turn, _)| mid_turn),
            "mid_turn_cap": self.busy.map(|(_, cap)| cap),
        })
    }

    fn refusal(&self) -> Option<Refusal> {
        let over = self.over();
        if over.is_empty() {
            return None;
        }
        Some(Refusal::refused(format!(
            "the machine cannot take another transient seat: {} over its ceiling.\n{}",
            over.join(" and "),
            self.lines()
        )))
    }
}

fn override_f64(key: &str) -> Option<f64> {
    std::env::var(key).ok()?.trim().parse().ok()
}

fn override_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok()?.trim().parse().ok()
}

// ---- spawn (R30) ------------------------------------------------------------

/// What a spawn was asked for. The first turn is the file's TEXT: reading the
/// file is the caller's, because the caller is the one that knows whether a
/// missing file is a usage error or a brief that failed to render.
pub struct Spawn<'a> {
    pub first_turn: &'a str,
    pub model: Option<&'a str>,
    /// The provider's local settings document, with [`WORKTREE`] still in it.
    ///
    /// A spawned session comes up under a posture that refuses every writing
    /// call it holds no rule for, and a permission list cannot ride the plugin
    /// root the pack's overlay is loaded through — so the rules are written into
    /// the worktree instead, before the first turn. `None` writes nothing.
    ///
    /// Everything else in the document is rendered by the caller, which reads
    /// the pack layers and the project's gates. The worktree is not: it is
    /// claimed below, and nothing outside this function knows the path.
    ///
    /// A document the worktree already carries is MERGED INTO rather than
    /// replaced: see [`write_settings`].
    pub settings: Option<&'a str>,
    /// The work item this spawn is being made for, where the caller is giving
    /// one. It is carried only to be RECORDED — on the row and on the stream —
    /// so a report about this seat can name the work it was holding; nothing
    /// here reads the work graph.
    pub item: Option<&'a str>,
    /// The commit this seat's worktree is cut from, where the caller names one.
    /// `None` cuts from [`TRUNK`], which is what every spawn did before a
    /// reviewer had to read a delivery and a returned builder had to resume
    /// from one (flights PRD R11, R14).
    ///
    /// A base the primary cannot resolve REFUSES BEFORE ANYTHING IS MADE: the
    /// `worktree add` would otherwise leave a name claimed and a rollback to
    /// take, for a fact that was readable one call earlier.
    pub base: Option<&'a str>,
    /// The per-provider overlay files that belong in a configuration space,
    /// as `(relative path, content)`.
    ///
    /// They are copied into the per-row directory before the start. The
    /// resolved overlay carries none today — the guards reach a session through
    /// the plugin root and the permissions through the worktree's own settings
    /// file — so the directory's whole content is its emptiness, and this is the
    /// seam that stops being true without a second edit here.
    pub config_files: &'a [(String, String)],
}

/// Where the per-row configuration directories live, under the machine
/// directory: one per spawned seat, named by the seat (flights PRD R13, Q1e).
pub const CONFIG_DIRS: &str = "config";

/// The one placeholder [`spawn`] fills, in the same `{name}` grammar the pack's
/// own templates use.
pub const WORKTREE: &str = "{worktree}";

/// What a spawn left behind. The name is the verb's one answer on stdout,
/// because the caller is `dispatch` and the name is what it assigns to.
#[derive(Debug)]
pub struct Spawned {
    pub seat: String,
    pub worktree: PathBuf,
    pub belt: Belt,
    /// The commit the worktree was cut at, read from ITS OWN HEAD rather than
    /// from the trunk ref the add named: the ref can move between the cut and
    /// the read, and the tree is the thing this fact describes. `None` where the
    /// read would not answer — the spawn stands either way, and a caller that
    /// records this writes it absent rather than guessing the trunk.
    pub base: Option<String>,
}

/// Create a transient seat: the belt, the worktree, the row, the start, the
/// read-back (R30).
///
/// Each step is read back before the next, and the ROLLBACK WINDOW OPENS AT THE
/// WORKTREE: everything refused above it leaves nothing behind, and a start
/// that fails below it takes the worktree and the row with it — never a branch.
pub fn spawn(machine: &Machine, ask: &Spawn, now_ms: u64) -> Result<Spawned, Refusal> {
    let seats = machine.seats()?;

    // (a) The belt, before anything is created.
    let belt = Belt::read(machine, &seats);
    if let Some(refusal) = belt.refusal() {
        return Err(refusal);
    }

    // (a2) The base, resolved in the primary BEFORE the name is claimed: a cut
    // from a commit that is not there refuses here with nothing made.
    let cut_from = match ask.base {
        Some(base) => resolved_base(machine.primary, base)?,
        None => TRUNK.to_string(),
    };

    // (b) and (c) under ONE lock: the name, the worktree cut from the commit
    // above, and the row. The rollback window opens with the `worktree add`
    // inside it, and a claim that answers `Made` is one that got that far.
    let model = machine.policy.model_for(ask.model);
    let claimed = config::claim_transient_seat(
        &machine.config_path(),
        &config::TransientSeat {
            project: machine.project,
            model: &model,
            worktrees_dir: machine.worktrees_dir,
        },
        |_name, worktree| {
            git(
                machine.primary,
                "worktree add",
                &[
                    "worktree",
                    "add",
                    "--detach",
                    &worktree.display().to_string(),
                    &cut_from,
                ],
            )
            .map(|_| ())
        },
    );
    let (name, worktree) = match claimed {
        Ok(claimed) => (claimed.name, claimed.worktree),
        Err(config::ClaimError::Nothing(why)) => return Err(Refusal::refused(why)),
        Err(config::ClaimError::Made { worktree, why }) => {
            return Err(rolled_back(machine, REFUSED, "", &worktree, &why))
        }
    };
    let worktree_arg = worktree.display().to_string();
    // READ HERE, right after the add and inside the rollback window: this is the
    // one moment the tree exists and nothing of the seat's has touched it, so
    // the commit read is the one the seat starts from. A read that will not
    // answer is not a refusal — the seat is real and the base is a fact about
    // it, not a precondition of it.
    let base = base_of(&worktree);

    match machine.seats() {
        Ok(seats) if seats.iter().any(|seat| seat.name == name && seat.transient) => {}
        Ok(_) => {
            return Err(rolled_back(
                machine,
                COULD_NOT_TELL,
                &name,
                &worktree,
                &format!("the seat list does not read back a transient row for {name}"),
            ))
        }
        Err(refusal) => {
            return Err(rolled_back(
                machine,
                refusal.code,
                &name,
                &worktree,
                &refusal.message,
            ))
        }
    }

    // (c2) The permission rules, inside the rollback window and BEFORE the
    // start: a session that came up without them is one that cannot write, and
    // this provider reads the file once at startup.
    let settings = match ask.settings {
        Some(template) => {
            match write_settings(machine.agent.local_settings(), &worktree, template) {
                Ok(did) => Some(did),
                Err(why) => {
                    return Err(rolled_back(machine, COULD_NOT_TELL, &name, &worktree, &why))
                }
            }
        }
        None => None,
    };

    // (c3) The seat's OWN configuration directory, empty but for whatever the
    // overlay puts in a configuration space (R13, Q1e). Made here, inside the
    // rollback window and before the start, because the start is what the
    // directory is for and a session that came up under the person's own
    // directory is the isolation failure this whole slice is against.
    let config_dir = machine.config_dir_for(&name);
    if let Err(why) = make_config_dir(&config_dir, ask.config_files) {
        return Err(rolled_back(machine, COULD_NOT_TELL, &name, &worktree, &why));
    }

    // (d) The start, through the same effect the loop's own spawn takes. The
    // table is READ here, before the start, so an unreadable one refuses before
    // a session is brought up that nothing could record — and it is read WITHOUT
    // the lock, because the start below spends a whole watch window and a lock
    // held across it blocks every other verb on this machine for that long.
    if let Err(refusal) = machine.table_now() {
        return Err(rolled_back(
            machine,
            refusal.code,
            &name,
            &worktree,
            &refusal.message,
        ));
    }
    let mut log = machine.log();
    let before = log.seq();
    let target = Target {
        seat_dir: &name,
        display_name: name.clone(),
        project: machine.project,
        worktree: &worktree_arg,
        model,
        posture: machine.policy.posture_for(true).to_string(),
        first_turn: ask.first_turn.to_string(),
        transient: true,
        config_dir: Some(config_dir.display().to_string()),
        item: ask.item.map(str::to_string),
        settings: settings.map(|did| did.as_str().to_string()),
        belt: Some(belt.payload()),
        // THE SPAWNING PROCESS'S OWN RUN, read here and nowhere else. A
        // workflow's child carries `FLEET_RUN_ID`, and `fleet seat spawn` under
        // one inherits it — so a seat a run started is tagged with it and a seat
        // started from a shell is not.
        run: crate::runs::of_this_process(),
        session_id: None,
        short_id: None,
        context_tokens: None,
    };
    // INTO A TABLE OF ITS OWN: what this call produces is the ROW, and the file
    // it belongs in can be written by a feed or a retire while the start runs.
    // The row is folded below into a copy read under the lock, so the table this
    // spawn hands over carries whatever else landed inside its window.
    let mut opened = Table::default();
    let started = effect::spawn_woken(
        machine.agent,
        machine.policy,
        &target,
        &mut log,
        &mut opened,
        now_ms,
    );
    if started != Outcome::Spawned {
        // A14: the child exited inside the watch window and said why in-band.
        // The output file is named from the line the adapter itself wrote, so
        // the path in the refusal is the one the crash event carries.
        let output = crashed_output(&machine.stream_path(), before, &name);
        return Err(rolled_back(
            machine,
            REFUSED,
            &name,
            &worktree,
            &format!(
                "the start for {name} failed inside its {}s watch window; its output is at {}",
                machine.policy.start_watch_seconds,
                output
                    .as_deref()
                    .unwrap_or("(the stream names no output file)")
            ),
        ));
    }
    // (d2) The row onto the file, under the lock and over a table read inside
    // it. A failure here is not rolled back: the session is up, and a refusal
    // that also deleted its worktree would take a live seat's tree with it.
    let (held, mut table) = machine.table_under_lock()?;
    table.sessions.extend(opened.sessions);
    machine.write_table(&held, &table)?;
    drop(held);

    // (e) The read-back: the row off the seat list, and the session-table row
    // off its own file rather than off the object this process just built.
    let written = sessions::read(&machine.table_path()).0.ok_or_else(|| {
        Refusal::could_not_tell(format!(
            "the session table at {} does not read back after the start",
            machine.table_path().display()
        ))
    })?;
    if written.newest_for(&name).is_none() {
        return Err(Refusal::could_not_tell(format!(
            "the session table carries no row for {name} after its start"
        )));
    }
    Ok(Spawned {
        seat: name,
        worktree,
        belt,
        base,
    })
}

/// The trunk ref a transient worktree is cut from where a spawn names no base,
/// as the local ref names it.
const TRUNK: &str = "origin/main";

/// One named base as the primary resolves it, or a refusal naming it.
///
/// `rev-parse --verify <base>^{commit}` and not a bare `rev-parse`: the bare
/// form validates SYNTAX and echoes any 40-hex string back at exit 0, so a
/// commit that is not in this repository would reach the `worktree add` and
/// fail there — inside the rollback window, with a name already claimed.
fn resolved_base(primary: &Path, base: &str) -> Result<String, Refusal> {
    git(
        primary,
        "rev-parse",
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .map(|read| read.trim().to_string())
    .map_err(|why| {
        Refusal::refused(format!(
            "`{base}` does not resolve to a commit in {} — nothing was made: {why}",
            primary.display()
        ))
    })
    .and_then(|sha| {
        if sha.is_empty() {
            return Err(Refusal::refused(format!(
                "`{base}` does not resolve to a commit in {} — nothing was made",
                primary.display()
            )));
        }
        Ok(sha)
    })
}

/// The commit a fresh worktree's HEAD names, or nothing.
fn base_of(worktree: &Path) -> Option<String> {
    let read = git(worktree, "rev-parse HEAD", &["rev-parse", "HEAD"]).ok()?;
    let sha = read.trim().to_string();
    (sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())).then_some(sha)
}

/// The seat's own configuration directory, made and read back (R13, Q1e).
///
/// A directory that already stands is EMPTIED first: a spawn re-using a name a
/// retire left behind would otherwise hand the new session the old one's state,
/// which is the isolation this exists to give, lost to a name collision.
///
/// The read-back is the same gate every other step here takes, and it is the
/// LISTING and not the existence: a directory the process could not write into
/// is one the agent will populate from the person's own instead.
fn make_config_dir(dir: &Path, files: &[(String, String)]) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .map_err(|e| format!("{} could not be emptied: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("{} could not be made: {e}", dir.display()))?;
    for (relative, content) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .map_err(|e| format!("{} was not written: {e}", path.display()))?;
    }
    let back = std::fs::read_dir(dir)
        .map_err(|e| format!("{} does not read back: {e}", dir.display()))?
        .count();
    if back != files.len() {
        return Err(format!(
            "{} reads back {back} entries and {} were written",
            dir.display(),
            files.len()
        ));
    }
    Ok(())
}

/// What a settings write DID, which is what the seat's `session.spawned` line
/// carries so a person reading the stream can tell the two apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsWrite {
    /// The worktree carried no such document, so the pack's is the whole file.
    Written,
    /// The worktree carried one — a project may TRACK this path on its trunk —
    /// so the pack's lists were folded into it and every rule of the project's
    /// own kept.
    Merged,
}

impl SettingsWrite {
    pub fn as_str(self) -> &'static str {
        match self {
            SettingsWrite::Written => "written",
            SettingsWrite::Merged => "merged",
        }
    }
}

/// The permission lists a merge folds. Everything else in either document is
/// the project's to keep: a key only the pack carries is not a rule, and a key
/// only the project carries is not this slice's to touch.
const MERGED_LISTS: [&str; 2] = ["allow", "deny"];

/// Render the settings document into the worktree and read it back.
///
/// A document already standing at the path is MERGED INTO, never replaced: a
/// project that tracks this file on its trunk hands every transient worktree a
/// copy of it, and a spawn that wrote over it would take the project's own
/// rules out of the seat's session and leave a tracked file modified in a
/// checkout nobody edited.
///
/// The read-back is the same gate every other step here takes: a write that
/// silently landed short leaves a seat that can neither edit nor commit, and
/// the refusal a person would get instead is one from the agent, hours later.
fn write_settings(
    relative: &str,
    worktree: &Path,
    template: &str,
) -> Result<SettingsWrite, String> {
    let rendered = template.replace(WORKTREE, &worktree.display().to_string());
    let path = worktree.join(relative);
    let standing = match std::fs::read_to_string(&path) {
        Ok(standing) => Some(standing),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(format!(
                "the settings document at {} could not be read, and one that cannot be read is \
                 never written over: {e}",
                path.display()
            ))
        }
    };
    let (document, did) = match standing {
        None => (rendered, SettingsWrite::Written),
        Some(standing) => (
            merged_settings(&standing, &rendered).map_err(|why| {
                format!(
                    "the pack's permission rules were not merged into the document at {}, and \
                     nothing was written over: {why}",
                    path.display()
                )
            })?,
            SettingsWrite::Merged,
        ),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    std::fs::write(&path, &document).map_err(|e| {
        format!(
            "the permission rules at {} were not written: {e}",
            path.display()
        )
    })?;
    match std::fs::read_to_string(&path) {
        Ok(back) if back == document => Ok(did),
        Ok(_) => Err(format!(
            "the permission rules at {} do not read back as they were written",
            path.display()
        )),
        Err(e) => Err(format!(
            "the permission rules at {} do not read back: {e}",
            path.display()
        )),
    }
}

/// The pack's permission lists folded into the document a project already
/// carries: the project's document is the base, its rules stand FIRST and in
/// their own order, the pack's are appended, and a rule the project already
/// carries is not appended twice.
///
/// A document either side offers that this cannot read as JSON is an error
/// here, which the caller turns into a refusal — the alternative is to fall
/// back on the write, and the write is the damage.
fn merged_settings(project: &str, pack: &str) -> Result<String, String> {
    let mut merged: serde_json::Value = serde_json::from_str(project)
        .map_err(|e| format!("the project's document is not JSON: {e}"))?;
    let rules: serde_json::Value =
        serde_json::from_str(pack).map_err(|e| format!("the pack's document is not JSON: {e}"))?;
    let Some(root) = merged.as_object_mut() else {
        return Err("the project's document is not a JSON object".to_string());
    };
    let Some(permissions) = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
    else {
        return Err("the project's `permissions` is not a JSON object".to_string());
    };
    for list in MERGED_LISTS {
        let Some(theirs) = rules
            .get("permissions")
            .and_then(|held| held.get(list))
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        let Some(ours) = permissions
            .entry(list)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
        else {
            return Err(format!(
                "the project's `permissions.{list}` is not an array"
            ));
        };
        for rule in theirs {
            if !ours.contains(rule) {
                ours.push(rule.clone());
            }
        }
    }
    serde_json::to_string_pretty(&merged)
        .map_err(|e| format!("the merged document could not be written out: {e}"))
}

/// Undo everything the window covers, and say what the undo did. An empty
/// `name` is a claim that made the worktree and never wrote the row, where the
/// drop below finds nothing and says so.
///
/// NEVER A BRANCH. `git worktree remove` leaves refs alone, so a branch made
/// inside the worktree survives — which is the point: it is exactly what a
/// re-dispatch resumes from.
///
/// `code` is the status the cause CARRIED. A rollback does not change what kind
/// of answer this is: a table nobody could read is still could-not-tell after
/// the worktree has been taken back, and rounding it to a refusal would tell the
/// caller the machine said no when it said it could not say.
fn rolled_back(machine: &Machine, code: u8, name: &str, worktree: &Path, why: &str) -> Refusal {
    let removed = match git(
        machine.primary,
        "worktree remove",
        &[
            "worktree",
            "remove",
            "--force",
            &worktree.display().to_string(),
        ],
    ) {
        Ok(_) => format!("the worktree at {} was removed", worktree.display()),
        Err(cause) => format!("THE WORKTREE AT {} SURVIVES: {cause}", worktree.display()),
    };
    let held_config = machine.config_dir_for(name);
    let unconfigured = if name.is_empty() || !held_config.exists() {
        "no configuration directory was left to remove".to_string()
    } else {
        match std::fs::remove_dir_all(&held_config) {
            Ok(()) => format!(
                "the configuration directory at {} was removed",
                held_config.display()
            ),
            Err(e) => format!(
                "THE CONFIGURATION DIRECTORY AT {} SURVIVES: {e}",
                held_config.display()
            ),
        }
    };
    // An empty name is a claim that made the worktree and never reached the row,
    // so there is nothing to look for and the line says that rather than naming
    // a seat with no name.
    let dropped = if name.is_empty() {
        "no seat-list row was written to drop".to_string()
    } else {
        match config::drop_seat(&machine.config_path(), name) {
            Ok(true) => format!("the seat-list row for {name} was dropped"),
            Ok(false) => format!("the seat list carried no row for {name} to drop"),
            Err(cause) => format!("THE SEAT-LIST ROW FOR {name} SURVIVES: {cause}"),
        }
    };
    Refusal::at(
        code,
        format!(
            "{why}\n  rolled back: {removed}; {dropped}; {unconfigured}; no branch was deleted"
        ),
    )
}

/// The output file the `session.crashed` line this start wrote names.
fn crashed_output(stream: &Path, after: u64, seat: &str) -> Option<String> {
    events::read_after(stream, after)
        .into_iter()
        .rev()
        .find(|record| record.kind == events::SESSION_CRASHED && record.actor == seat)
        .and_then(|record| {
            record
                .payload
                .get("output")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

// ---- feed (R31) -------------------------------------------------------------

/// What a feed left behind: the turn that was in the seat and the one that is
/// now, so a caller can say what moved.
#[derive(Debug)]
pub struct Fed {
    pub seat: String,
    pub prior: String,
    pub next: String,
}

/// Hand a live transient seat its next first turn, moving the occupant marker
/// with a put-back on a delivery that failed (R31).
///
/// THE MARKER IS THE SESSION-TABLE ROW'S `first_turn`, which the spawn set and
/// this verb moves; the move is journaled on the event stream, which is the
/// controller's one ledger (R23, R25), and never in a file beside it.
pub fn feed(machine: &Machine, seat: &str, first_turn: &str) -> Result<Fed, Refusal> {
    let seats = machine.seats()?;
    let row = machine.transient_row(&seats, seat)?;
    let worktree = machine.worktree_of(&row)?;
    let key = dir_key(&worktree);

    // Under the seat's own configuration directory, for the same reason the
    // retire reads there (flights PRD R13): the fleet's listing does not name a
    // spawned seat's session, and a feed that read it would refuse every live
    // seat as having none.
    let config_dir = machine.recorded_config_dir(seat);
    let under = config_dir.as_deref().map(Path::new);
    let rows = machine.roster_under(under)?;
    let live = rows
        .iter()
        .find(|row| row.is_live() && row.cwd_key() == key);
    let Some(live) = live else {
        return Err(Refusal::at(
            NO_SESSION,
            format!("`{seat}` has no live session in {worktree}, so there is nothing to feed"),
        ));
    };
    // The primitive knows only what the AGENT says. Whether the seat is holding
    // a work item is the caller's half and is not asked here.
    if live.is_busy() {
        return Err(Refusal::refused(format!(
            "`{seat}` is still holding a turn — the agent reports its session {}",
            crate::adapter::BUSY
        )));
    }

    // THE NEWEST ROW FOR THE SEAT, which is the one this seat is sitting in: a
    // seat is carried through as many rows as the loop opened, and the first in
    // file order is a dispatch two successors ago. The lock is held across the
    // read, the move and the write-back, and DELIBERATELY ON across the
    // delivery to the put-back below: a second feed landing between the move
    // and its put-back would have its own move undone by this one.
    let (held, mut table) = machine.table_under_lock()?;
    let Some(marker) = table.newest_for_mut(seat) else {
        return Err(Refusal::refused(format!(
            "the session table carries no row for `{seat}`, so there is no occupant marker to \
             move"
        )));
    };
    let prior = marker.first_turn.clone();
    marker.first_turn = first_turn.to_string();
    machine.write_table(&held, &table)?;

    let mut log = machine.log();
    let delivered = machine.agent.nudge(
        under,
        seat,
        &worktree,
        &machine.policy.nudge_model,
        first_turn,
        Duration::from_secs(machine.policy.nudge_timeout_seconds),
    );
    let outcome = match &delivered {
        Ok(()) => "delivered".to_string(),
        Err(cause) => format!("failed: {cause}"),
    };
    // The journal BEFORE the put-back, so a line that cannot land is met with
    // the marker still where this verb moved it and the message saying so.
    let journalled = machine.journal(
        &mut log,
        events::SESSION_NUDGED,
        seat,
        serde_json::json!({
            "worktree": worktree,
            "prior_first_turn": first_line(&prior),
            "first_turn": first_line(first_turn),
            "outcome": outcome,
            "put_back": false,
        }),
    );

    if let Err(cause) = delivered {
        // The row goes back to the turn that was in the seat, and the put-back
        // is a SECOND line rather than a rewrite of the first: the stream is
        // append-only and the attempt is part of what happened.
        if let Some(marker) = table.newest_for_mut(seat) {
            marker.first_turn = prior.clone();
        }
        machine.write_table(&held, &table)?;
        journalled?;
        machine.journal(
            &mut log,
            events::SESSION_NUDGED,
            seat,
            serde_json::json!({
                "worktree": worktree,
                "prior_first_turn": first_line(first_turn),
                "first_turn": first_line(&prior),
                "outcome": format!("put back: {cause}"),
                "put_back": true,
            }),
        )?;
        return Err(Refusal::refused(format!(
            "`{seat}` was not fed — {cause}; the occupant marker was put back"
        )));
    }
    journalled?;

    Ok(Fed {
        seat: seat.to_string(),
        prior,
        next: first_turn.to_string(),
    })
}

/// A turn's first line, which is what the journal carries: the whole text is
/// the table's, and a stream line that repeated it would be a second copy that
/// could disagree with the first.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

// ---- retire (R32) -----------------------------------------------------------

/// What a retire reclaimed, as numbers a person reads.
#[derive(Debug)]
pub struct Reclaimed {
    pub seat: String,
    pub worktree: String,
    /// The worktree's bytes, measured BEFORE the removal. `None` is a directory
    /// this verb could not walk, which is stated rather than reported as zero.
    pub bytes: Option<u64>,
    /// The pid the roster carried before the stop, and `None` where no live row
    /// named one.
    pub pid: Option<u32>,
    pub dead: bool,
    /// What the adapter's removal answered, including the alarm.
    pub removal: Option<String>,
    /// The branch the seat's worktree stood on, read BEFORE the removal took
    /// the worktree with it. `None` is a detached HEAD or a tree git would not
    /// answer about — never a branch called nothing.
    ///
    /// Nothing here acts on it. It is the second key a caller's release of that
    /// branch turns on: the landing's note says which branch it classified SAFE,
    /// and this says which one the seat that is going actually held.
    pub branch: Option<String>,
    /// What the caller's [`Withdrawal`] answered: the record's hold on this
    /// seat, released before the name was freed. Empty is a seat that held
    /// nothing ordered AND a caller that withdraws nothing — the two are one
    /// answer here, because this verb asks no work graph of its own.
    pub withdrawn: Vec<String>,
}

/// The record's half of a retire, filled by the caller.
///
/// A SEAM AND NOT A CALL, because this crate reaches no work graph: the store
/// is core's, and the cli is the one crate holding both. It is handed the seat
/// going and answers what it released, or the refusal the retire stops on.
///
/// The retire runs it at the LAST moment it can still stop — after the session
/// is stopped and verified gone, before the seat-list row comes off — so a
/// withdrawal that cannot be written leaves the name held rather than freed for
/// the next spawn to take with somebody else's order still on it.
pub type Withdrawal<'a> = &'a dyn Fn(&str) -> Result<Vec<String>, Refusal>;

/// The caller that withdraws nothing, which is what every retire did before the
/// seam existed: a controller with no work graph in reach passes it.
pub fn withdraws_nothing(_seat: &str) -> Result<Vec<String>, Refusal> {
    Ok(Vec::new())
}

/// End a transient seat and verify from OUTSIDE that nothing of it holds RAM or
/// disk (R32).
///
/// The roster is read once, whole, at the top: an unreadable one is
/// could-not-tell, because a session nobody could ask is a question and not an
/// absence. Every probe at the end is its own reading and none of them is this
/// verb's own earlier report.
pub fn retire(machine: &Machine, seat: &str, dead: bool) -> Result<Reclaimed, Refusal> {
    retire_with(machine, seat, dead, &withdraws_nothing)
}

/// The same retire, with the record's half of it filled (see [`Withdrawal`]).
pub fn retire_with(
    machine: &Machine,
    seat: &str,
    dead: bool,
    withdrawal: Withdrawal,
) -> Result<Reclaimed, Refusal> {
    let seats = machine.seats()?;
    let row = machine.transient_row(&seats, seat)?;
    let worktree = machine.worktree_of(&row)?;
    let key = dir_key(&worktree).to_string();

    // THE DIRECTORY THIS SEAT'S SESSION IS HELD UNDER, read off its own row
    // (flights PRD R13). Every listing, stop and removal below is made under it:
    // a spawned seat is named by its own daemon's listing and by no other, so
    // the fleet's read would find no live row, take the no-session branch, and
    // delete the worktree out from under a session still running in it.
    let config_dir = machine.recorded_config_dir(seat);
    let under = config_dir.as_deref().map(Path::new);

    let rows = machine.roster_under(under)?;
    let live = rows
        .iter()
        .find(|row| row.is_live() && row.cwd_key() == key)
        .cloned();

    // Measured before anything is removed, because both readings are gone the
    // instant the removal lands.
    let bytes = bytes_under(Path::new(&worktree));
    let branch = branch_of(Path::new(&worktree));
    let pid = live.as_ref().and_then(|row| row.pid);

    // THE ADDRESS A REMOVAL TAKES, from the table where the roster names no
    // live row. A seat whose session is already gone still has a session row to
    // delete, and a retire that skipped it would leave the agent holding one
    // per dead seat.
    //
    // READ WITHOUT THE LOCK, and an unreadable table refuses here, before the
    // stop. What the lock guards is the read-modify-write at the end; held from
    // here it would span the stop, the roster poll bounded by the start-watch
    // window, the removal and two git calls, and every other verb on this
    // machine would wait out a whole retire.
    let recorded_short_id = {
        let table = machine.table_now()?;
        table.newest_for(seat).and_then(|row| row.short_id.clone())
    };

    let short_id = match (&live, dead) {
        // `--dead` licenses the removal by a COMPLETED roster read that names no
        // live row — never by silence, which is what the could-not-tell above
        // keeps out of this branch.
        (Some(row), true) => {
            return Err(Refusal::refused(format!(
                "`{seat}` is not dead — the roster names a live session {} in {worktree}, so \
                 --dead was the wrong flag; retire it without --dead to stop it first",
                row.session_id
            )))
        }
        // No live row: nothing to stop, and the removal is addressed by what the
        // table remembers.
        (None, _) => recorded_short_id,
        (Some(row), false) => {
            let Some(short_id) = row.id.clone() else {
                return Err(Refusal::could_not_tell(format!(
                    "the roster's live row for `{seat}` carries no short id, which is the \
                     address a stop takes (lessons claude-code A6); nothing was removed"
                )));
            };
            machine.agent.stop(under, &short_id).map_err(|cause| {
                Refusal::refused(format!("`{seat}` could not be stopped: {cause}"))
            })?;
            // THE STOP'S OWN EXIT IS NEVER THE WITNESS (A6, A7). The roster is
            // re-read until it names no row in that directory.
            if !cleared(machine, under, &key)? {
                return Err(Refusal::refused(format!(
                    "the roster still names a session in {worktree} after the stop, so nothing \
                     was removed: the worktree, the seat-list row and the session table all \
                     stand"
                )));
            }
            Some(short_id)
        }
    };

    // In order: the session row, the worktree, the seat-list row, the table.
    //
    // `None` is not "nothing happened": it is a seat whose roster row is gone
    // AND whose table row carries no address, so there is no session row
    // anywhere for a removal to delete.
    let removal = short_id.as_ref().map(|short_id| {
        match machine.agent.remove(under, short_id) {
            RemoveAnswer::Removed => format!("removed {short_id}"),
            RemoveAnswer::Refused { cause } => format!("removing {short_id} refused: {cause}"),
            // A8's alarm: no discard flag is ever passed, so this arm must not
            // be reachable — and a reader meets the path in the report rather
            // than meeting the missing directory.
            RemoveAnswer::RemovedAWorktree { path } => {
                format!("ALARM — the removal DELETED a worktree at {path}")
            }
        }
    });

    if Path::new(&worktree).exists() {
        git(
            machine.primary,
            "worktree remove",
            &["worktree", "remove", "--force", &worktree],
        )
        .map_err(|cause| {
            // WHAT THIS REFUSAL SAYS IS GONE IS WHAT THE TWO ACTS ABOVE DID:
            // the stop ran only where the roster named a live row, and the
            // session row is already removed by the time a worktree can be
            // found stuck.
            let stopped = match &live {
                Some(_) => "the session is stopped",
                None => "there was no live session to stop",
            };
            let session_row = match &removal {
                Some(answer) => format!("its session row is already gone ({answer})"),
                None => "there was no session row to remove".to_string(),
            };
            Refusal::refused(format!(
                "{worktree} could not be removed: {cause}\n  {stopped} and {session_row}; the \
                 seat-list row and the session table both stand — so re-running this after \
                 clearing the cause is safe"
            ))
        })?;
    }
    let _ = git(machine.primary, "worktree prune", &["worktree", "prune"]);

    // THE RECORD BEFORE THE NAME. The seat list is what hands `transient-N`
    // out, so the line below is the moment this name becomes takeable — and an
    // order still standing against it would be inherited by whoever takes it
    // next. Everything above has already happened and a refusal here says so:
    // the session is stopped and gone, and the row is the one thing left.
    let withdrawn = withdrawal(seat).map_err(|refusal| Refusal {
        code: refusal.code,
        message: format!(
            "{}\n  the session and the worktree for `{seat}` ARE ALREADY GONE and the seat-list \
             row STANDS: the name is not free, so clear the item and re-run this",
            refusal.message
        ),
    })?;

    config::drop_seat(&machine.config_path(), seat).map_err(|cause| {
        Refusal::could_not_tell(format!(
            "the seat-list row for `{seat}` could not be dropped: {cause}"
        ))
    })?;
    // The row comes off under the lock, over a table read inside it: every
    // other seat's row on the copy this edits is the one on disk now, and not
    // the one this verb read before the stop.
    let (held, mut table) = machine.table_under_lock()?;
    table.sessions.retain(|row| row.seat != seat);
    machine.write_table(&held, &table)?;
    drop(held);

    // FROM HERE THE TWO ROWS ARE ALREADY GONE, so every refusal below says so:
    // a re-run would be refused at the seat list with "is not a row", which
    // tells the person nothing about what is still standing.
    verify_from_outside(machine, seat, under, &worktree, &key, pid).map_err(|refusal| Refusal {
        code: refusal.code,
        message: format!(
            "{}\n  the seat-list row and the session-table row for `{seat}` ARE ALREADY DROPPED, \
             so a re-run will refuse at the seat list: finish by hand from {worktree}{}",
            refusal.message,
            match pid {
                Some(pid) => format!(" and pid {pid}"),
                None => String::new(),
            }
        ),
    })?;

    // LAST, and only once the probes above have answered: the seat's
    // configuration directory, which holds that one session's whole
    // configuration space and which the roster probe reads THROUGH — so a
    // removal before it would leave that probe with nothing to ask and passing
    // vacuously. A refused verification leaves the directory standing, which is
    // right: something is still running under it.
    let held_config = machine.config_dir_for(seat);
    if held_config.exists() {
        std::fs::remove_dir_all(&held_config).map_err(|e| {
            Refusal::could_not_tell(format!(
                "the configuration directory at {} could not be removed, and the seat is \
                 otherwise fully retired: {e}",
                held_config.display()
            ))
        })?;
    }

    // Retire NEVER deletes a branch: refs are the landing verb's, and a branch a
    // dead seat left is exactly what a re-dispatch resumes from. A caller
    // holding the landing that classified this one releases it through
    // [`delete_branch`], off that note and never off this verb's own reading.
    let mut log = machine.log();
    machine.journal(
        &mut log,
        events::SESSION_STOPPED,
        seat,
        serde_json::json!({
            "worktree": worktree,
            "bytes": bytes,
            "pid": pid,
            "dead": dead,
            "removal": removal,
        }),
    )?;

    Ok(Reclaimed {
        seat: seat.to_string(),
        worktree,
        bytes,
        pid,
        dead,
        removal,
        branch,
        withdrawn,
    })
}

/// One work branch deleted in the primary, for a caller that has read a landing
/// saying so.
///
/// THE MUZZLE IS GUARDED AND NOT ONLY THE TRIGGER. This is the one call in this
/// crate that destroys a ref, so a name git would read as an option or as a
/// second argument is refused here whatever decided to pass it — and `--` is
/// passed for the same reason. What a name MEANS is the caller's to judge: this
/// knows nothing of trunks or of which branch was classified.
///
/// A delete that fails is a `String` and not a panic: the retire it follows has
/// already landed, and a ref that outlives it is a line to print.
pub fn delete_branch(machine: &Machine, branch: &str) -> Result<(), String> {
    if branch.is_empty() || branch.starts_with('-') || branch.split_whitespace().count() != 1 {
        return Err(format!(
            "`{branch}` is not a name this deletes — it is empty, reads as an option, or is not \
             one word"
        ));
    }
    git(
        machine.primary,
        "branch -D",
        &["branch", "-D", "--", branch],
    )
    .map(|_| ())
}

// ---- the priced retire (flights PRD R10, R12) -------------------------------

/// What one seat cost, and where its work got to.
///
/// EVERY FIELD IS A READING and every one is an `Option`. A transcript that
/// would not open prices no seat and a worktree git cannot answer about names no
/// branch; a zero here would read as free and an empty string as a branch called
/// nothing, and both are the lie this shape exists to refuse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cost {
    /// The main-chain context the session's last turn carried.
    pub context_tokens: Option<u64>,
    /// Main-chain assistant entries carrying a usage block.
    pub turns: Option<u64>,
    /// The clock less the row's `dispatched_at`.
    pub wall_ms: Option<u64>,
    /// The branch the worktree was on, read before the removal.
    pub branch: Option<String>,
    /// The commit its HEAD named, read in the same act.
    pub commit: Option<String>,
}

/// What a priced retire left behind: the reclaim and the cost beside it.
#[derive(Debug)]
pub struct Priced {
    pub reclaimed: Reclaimed,
    pub cost: Cost,
}

/// Retire a seat a flight spawned, reading what it cost before it goes (R10,
/// R12).
///
/// THE READINGS COME FIRST, ALL OF THEM, because each is gone the instant the
/// removal lands: the transcript goes with the session row, the worktree with
/// the `worktree remove`, and the session table's `dispatched_at` with the row.
///
/// A READING NOBODY CAN TAKE DOES NOT STOP THE RETIRE. A seat nobody can price
/// is still a seat to reclaim, so every failure above is a `None` and the event
/// says so; only the retire itself refuses.
///
/// `--dead` IS NOT PASSED, and that is a reading rather than an omission: the
/// flag REFUSES when the roster names a live row and changes nothing when it
/// names none, so a flagless call reclaims a seat whose session is already gone
/// exactly as `--dead` would and stops one that is still up. A flight retiring a
/// crashed seat and a flight retiring a delivered one therefore take the same
/// call.
pub fn priced(machine: &Machine, seat: &str, item: &str, now_ms: u64) -> Result<Priced, Refusal> {
    priced_with(machine, seat, item, now_ms, &withdraws_nothing)
}

/// The same priced retire, with the record's half of it filled (see
/// [`Withdrawal`]).
///
/// THE SEAM IS THE ONE THE HAND VERB RUNS. A run's cleanup retires a seat whose
/// item can still be open — parked, or never delivered — and the name it frees
/// is the one the next spawn takes, so it owes the record exactly what
/// `fleet seat retire` owes it: the order withdrawn before the name goes back
/// on the pile. The withdrawal runs inside the reclaim below, at the last
/// moment that can still stop it.
pub fn priced_with(
    machine: &Machine,
    seat: &str,
    item: &str,
    now_ms: u64,
    withdrawal: Withdrawal,
) -> Result<Priced, Refusal> {
    let seats = machine.seats()?;
    let row = machine.transient_row(&seats, seat)?;
    let worktree = machine.worktree_of(&row)?;

    // The session row, for the id the transcript is keyed by and the stamp the
    // wall time is measured from.
    let (session_id, dispatched_at) = {
        let path = machine.table_path();
        let (table, _) = sessions::read(&path);
        match table.as_ref().and_then(|table| table.newest_for(seat)) {
            Some(row) => (row.session_id.clone(), Some(row.dispatched_at)),
            None => (None, None),
        }
    };

    // UNDER THE SEAT'S OWN CONFIGURATION DIRECTORY, which is where a spawned
    // seat's transcript is: a read under the default resolves no file at all,
    // which would price every seat this fleet spawned as unread.
    let config_dir = machine.recorded_config_dir(seat);
    let under = config_dir.as_deref().map(Path::new);
    let transcript = session_id
        .as_deref()
        .and_then(|session| machine.agent.transcript(under, &worktree, session));
    let commit = head_of(Path::new(&worktree));

    let reclaimed = retire_with(machine, seat, false, withdrawal)?;

    // The branch is the RETIRE's own reading, taken before the same removal
    // every reading above is taken before, and read off what it answered rather
    // than a second time here: two readings of one fact are two things that can
    // disagree.
    let cost = Cost {
        context_tokens: transcript
            .as_deref()
            .and_then(crate::observe::context_tokens_in),
        // A transcript that opened and states no turn is a MEASURED ZERO, which
        // is why the count is taken inside the `map` and not filtered after it:
        // a session that took none and a transcript nobody could open answer
        // differently here, and they are different facts.
        turns: transcript.as_deref().map(crate::observe::turns_in),
        wall_ms: dispatched_at.map(|from| now_ms.saturating_sub(from)),
        branch: reclaimed.branch.clone(),
        commit,
    };

    let mut log = machine.log();
    machine.journal(
        &mut log,
        events::SESSION_RETIRED,
        seat,
        serde_json::json!({
            "seat": seat,
            "item": item,
            "worktree": reclaimed.worktree,
            "context_tokens": cost.context_tokens,
            "turns": cost.turns,
            "wall_ms": cost.wall_ms,
            "branch": cost.branch,
            "commit": cost.commit,
            "transcript": transcript.is_some(),
            "bytes": reclaimed.bytes,
            "pid": reclaimed.pid,
            "dead": reclaimed.dead,
            "removal": reclaimed.removal,
            // THIS PATH HAS NO TERMINAL. The hand verb prints a line per item
            // it took off the seat; a run's cleanup runs inside a service, so
            // the stream is the only place the withdrawal can be read back.
            "withdrawn": reclaimed.withdrawn,
        }),
    )?;

    Ok(Priced { reclaimed, cost })
}

/// The branch a worktree is on, or `None` for a detached HEAD or a directory
/// git will not answer about.
fn branch_of(worktree: &Path) -> Option<String> {
    let name = git(
        worktree,
        "rev-parse --abbrev-ref HEAD",
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )
    .ok()?;
    let name = name.trim().to_string();
    (!name.is_empty() && name != "HEAD").then_some(name)
}

/// The commit that worktree's HEAD names. A tree with no commit on it answers
/// `None` rather than an empty string.
fn head_of(worktree: &Path) -> Option<String> {
    let sha = git(worktree, "rev-parse HEAD", &["rev-parse", "HEAD"]).ok()?;
    let sha = sha.trim().to_string();
    (!sha.is_empty()).then_some(sha)
}

/// Re-read the roster until it names no session in this directory, bounded by
/// the policy's start-watch window.
///
/// The window and the slice are both [`Machine::clock`]'s, so the number of
/// listings this takes is the same under any clock and only the waiting between
/// them is the box's.
fn cleared(machine: &Machine, under: Option<&Path>, key: &str) -> Result<bool, Refusal> {
    let deadline =
        machine.clock.now() + Duration::from_secs(machine.policy.start_watch_seconds.max(1));
    loop {
        let rows = machine.roster_under(under)?;
        if !rows.iter().any(|row| row.is_live() && row.cwd_key() == key) {
            return Ok(true);
        }
        if machine.clock.now() >= deadline {
            return Ok(false);
        }
        machine.clock.sleep(Duration::from_millis(200));
    }
}

/// The three probes, each its own reading and none of them this verb's own
/// earlier report.
///
/// A probe that CANNOT ANSWER is could-not-tell and refuses at 3, never rounded
/// to clean: the whole point of verifying from outside is that a retire which
/// says "done" has looked.
fn verify_from_outside(
    machine: &Machine,
    seat: &str,
    under: Option<&Path>,
    worktree: &str,
    key: &str,
    pid: Option<u32>,
) -> Result<(), Refusal> {
    // UNDER THE SEAT'S OWN DIRECTORY, which is the only listing that could name
    // its session (flights PRD R13): the fleet's would answer "no live row" for a
    // spawned seat whatever was running, which is a probe that cannot fail.
    let rows = machine.roster_under(under)?;
    if let Some(row) = rows
        .iter()
        .find(|row| row.is_live() && row.cwd_key() == key)
    {
        return Err(Refusal::refused(format!(
            "a live session {} still names {worktree} after the retire",
            row.session_id
        )));
    }

    if Path::new(worktree).exists() {
        return Err(Refusal::refused(format!(
            "{worktree} still exists after the removal"
        )));
    }
    let listed = git(
        machine.primary,
        "worktree list --porcelain",
        &["worktree", "list", "--porcelain"],
    )
    .map_err(|cause| {
        Refusal::could_not_tell(format!(
            "`git worktree list` could not be read, so whether {worktree} is still registered \
             is unknown: {cause}"
        ))
    })?;
    if listed
        .lines()
        .any(|line| line.strip_prefix("worktree ").map(dir_key) == Some(key))
    {
        return Err(Refusal::refused(format!(
            "`git worktree list` in {} still names {worktree}",
            machine.primary.display()
        )));
    }

    if let Some(pid) = pid {
        match platform::process_alive(pid) {
            Some(false) => {}
            Some(true) => {
                return Err(Refusal::refused(format!(
                    "pid {pid}, which the roster gave for `{seat}` before the stop, is still a \
                     live process"
                )))
            }
            None => {
                return Err(Refusal::could_not_tell(format!(
                    "whether pid {pid} is still alive could not be read, so this retire is not \
                     verified — everything else is done"
                )))
            }
        }
    }
    Ok(())
}

/// The bytes under a directory, following no symlink. `None` is a directory
/// this verb could not walk, which is reported as unknown rather than as zero.
///
/// Regular-file lengths and not disk blocks: the number answers "how much did
/// this seat's checkout hold", which is what a person reclaiming a machine is
/// asking, and it is the same figure on both filesystems this fleet runs on.
fn bytes_under(dir: &Path) -> Option<u64> {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(here) = stack.pop() {
        for entry in std::fs::read_dir(&here).ok()? {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Some(total)
}

// ---- git --------------------------------------------------------------------

/// One git call in the primary, the binary through the standard library with
/// the terminal prompt disabled.
///
/// A non-zero exit is a refusal naming the step and what git said, never a
/// value rounded to a default: a verb that read a failed `worktree add` as a
/// worktree would start a session in a directory that is not there.
fn git(primary: &Path, step: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(primary)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("`git {step}` could not be run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`git {step}` {}: {}",
            match out.status.code() {
                Some(code) => format!("exited {code}"),
                None => String::from("was killed by a signal"),
            },
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
