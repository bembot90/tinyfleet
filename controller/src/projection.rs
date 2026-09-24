//! `projection.json` — what the controller OBSERVED at `generated_at`.
//!
//! Every field is a report of a reading. There is no pid, no handle and no
//! running belief anywhere in the document, because a truthy pid in a state file
//! reads as a live collector to the next person who opens it. Freshness is the
//! only liveness signal a reader gets, and it is theirs to compute.
//!
//! The document is READ BACK by `fleet status`, so every shape here derives both
//! halves of serde together and each key the writer omits carries a default —
//! a writer that can emit a document its own reader refuses is a shape that
//! parts in one release.

use crate::adapter::dir_key;
use crate::config;
use crate::observe::SeatObservation;
use fleet_core::seat::identity::{Kind, SeatId, SeatRef};
use serde::{Deserialize, Serialize};

/// Bumped by any breaking change to the shape below. A reader that meets a
/// version it does not know refuses the document whole rather than half-reading
/// it.
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct Projection {
    pub version: u32,
    pub generated_at: String,
    pub controller_version: String,
    /// What the agent binary reported THIS poll, and the release its behaviours
    /// were measured against. A spread between them is a flag to re-measure,
    /// never a failure.
    pub agent_version: Option<String>,
    pub agent_version_expected: Option<String>,
    pub fleet: PolicyView,
    /// Present exactly while the loop is running on last-good policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_parse_error: Option<String>,
    /// The seat and the effect a poll is BLOCKED INSIDE, written before the
    /// call and cleared after it. Serialised as null rather than omitted, so a
    /// reader can tell a poll that is between effects from a document written
    /// by a build that did not carry the field: a slow effect and a dead
    /// controller are what this exists to separate.
    pub in_flight: Option<InFlight>,
    /// Whether this loop is issuing effects at all, and why not when it is not.
    pub effects: EffectsView,
    /// The file-access gate (lessons claude-code D4): `ok` once every
    /// configured worktree has answered a listing, `pending` while any has not.
    /// The detail names the path and which of the two answers it was, and is
    /// present exactly while the state is pending.
    ///
    /// While it is pending no effect is issued — `effects` is off with this as
    /// its cause — and answering the dialog needs no restart.
    pub grant: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_detail: Option<String>,
    pub seats: Vec<SeatRow>,
    /// Every order this tick loaded, with what it last did. Rendered from the
    /// routines state before the write, so a failing streak the reference let run
    /// unseen is a number every reader of this document meets.
    pub orders: Vec<crate::routines::RoutineRow>,
}

#[derive(Serialize, Deserialize)]
pub struct InFlight {
    /// The seat, as its row's `seat` carries it.
    pub seat: SeatView,
    pub effect: String,
}

/// A seat as every machine-readable document names it: `{id, name?, kind}`.
///
/// THE ID IS WHAT A READER KEYS ON, whole and hyphenated. The name is the
/// seat's own, absent — never null — where it has none, and free to change;
/// the kind is `agent` or `human`. The reader half is strings rather than the
/// typed id, so a row this build cannot parse is still a row `fleet status`
/// prints rather than a document it refuses whole.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SeatView {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    pub kind: String,
}

impl From<&SeatRef> for SeatView {
    fn from(seat: &SeatRef) -> Self {
        Self {
            id: seat.id.to_string(),
            name: seat.name.clone(),
            kind: seat.kind.as_str().to_string(),
        }
    }
}

impl SeatView {
    /// `<slug>-<short>`, the name a person reads a seat by — or the id as it
    /// was published, where the id or the kind is not one this build reads,
    /// because a line naming the text it was handed beats one naming nothing.
    pub fn machine_name(&self) -> String {
        match (SeatId::parse(&self.id), Kind::parse(&self.kind)) {
            (Ok(id), Some(kind)) => SeatRef {
                id,
                name: self.name.clone(),
                kind,
            }
            .machine_name(),
            _ => self.id.clone(),
        }
    }
}

/// `on`, or `off` with the cause. A loop that cannot exec the agent still
/// observes and publishes: refusing to publish would take the fleet's only
/// readable surface away over a binary nobody could find.
#[derive(Serialize, Deserialize)]
pub struct EffectsView {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

pub const EFFECTS_ON: &str = "on";
pub const EFFECTS_OFF: &str = "off";

impl EffectsView {
    pub fn on() -> Self {
        Self {
            state: EFFECTS_ON.to_string(),
            cause: None,
        }
    }

    pub fn off(cause: String) -> Self {
        Self {
            state: EFFECTS_OFF.to_string(),
            cause: Some(cause),
        }
    }
}

/// What a poll does about effects, and why.
pub struct EffectsDecision {
    pub view: EffectsView,
    /// Whether this poll carries any verdict out at all.
    pub acting: bool,
}

/// The two things that hold a poll's effects, read in one place.
///
/// THE GRANT OUTRANKS THE BINARY in the cause: a pending grant is a question a
/// person can answer and an unresolvable binary is not. And a pending grant
/// holds every effect, so the blind counter does not move either — a dispatch
/// nobody issued is not a dispatch nobody answered.
pub fn effects_of(grant_detail: Option<&str>, bin_error: Option<&str>) -> EffectsDecision {
    match (grant_detail, bin_error) {
        (Some(detail), _) => EffectsDecision {
            view: EffectsView::off(detail.to_string()),
            acting: false,
        },
        (None, Some(why)) => EffectsDecision {
            view: EffectsView::off(why.to_string()),
            acting: false,
        },
        (None, None) => EffectsDecision {
            view: EffectsView::on(),
            acting: true,
        },
    }
}

#[derive(Serialize, Deserialize)]
pub struct PolicyView {
    pub path: String,
    /// The stamp of the policy IN FORCE — the last read that parsed, not the
    /// file on disk.
    pub mtime: Option<String>,
    pub poll_seconds: u64,
    pub claude_code: Option<String>,
    /// The plugin root every start this fleet makes loads, absolute, or null for
    /// a fleet that names none. Null is a reading and not a gap, so it is
    /// serialised either way.
    pub plugin_dir: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SeatRow {
    /// The seat: a reader finds the row by its `id`.
    pub seat: SeatView,
    pub roster_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roster_unknown_cause: Option<String>,
    /// The agent's own cause string, present exactly when `roster_state` is
    /// `prompt-blocked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<String>,
    /// Why this stopped row was ranked on its START rather than on its end:
    /// present exactly when the recency window fell back, and absent on every
    /// row it did not — including every row that is not stopped. A reader
    /// deciding whether a `stopped` row is fresh needs to know which reading it
    /// is, because the two answer different questions about a long session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roster_recency_fallback: Option<String>,
    /// Null is a measured absence — nothing was read — and never a zero.
    pub context_tokens: Option<u64>,
    pub project: Option<String>,
    /// The directory, in the one spelling the seat match and the transcript
    /// lookup use — never the configured one, which a reader comparing this to
    /// an agent's `cwd` would have to normalise for itself.
    pub worktree: Option<String>,
    /// The verdict this poll reached for the seat, and what was done about it.
    /// A verdict that was reached and deliberately not carried out publishes its
    /// own outcome rather than a silence a reader would take for a quiet seat.
    pub decision: String,
    pub outcome: String,
    /// Consecutive blind dispatches for this seat, and whether the guard is
    /// holding it down. A number and a flag, both read from the controller's
    /// own record: a person answers a halted row with `fleet event clear-halt`,
    /// and the count is what says how close an unhalted one is.
    pub blind: u32,
    pub halted: bool,
}

impl SeatRow {
    pub fn from_observation(
        seat: &config::Seat,
        observation: &SeatObservation,
        context_tokens: Option<u64>,
    ) -> Self {
        Self {
            seat: SeatView::from(&seat.as_ref()),
            roster_state: observation.state.as_str().to_string(),
            roster_unknown_cause: observation.unknown_cause.clone(),
            waiting_for: observation.waiting_for.clone(),
            roster_recency_fallback: observation.recency_fallback.clone(),
            context_tokens,
            project: observation.project.clone(),
            worktree: observation
                .worktree
                .as_deref()
                .map(|worktree| dir_key(worktree).to_string()),
            // Filled by the loop once the verdict is reached. A row published
            // before that says so rather than guessing at leave-alone, which a
            // reader could not tell from a decision that was actually made.
            decision: UNDECIDED.to_string(),
            outcome: crate::effect::Outcome::None.as_str().to_string(),
            // The same: filled from the session table once the seat's verdict is
            // reached. A published zero here is a row nobody has decided about
            // yet, and the decision beside it says so.
            blind: 0,
            halted: false,
        }
    }
}

/// The `decision` a row carries before one has been reached.
pub const UNDECIDED: &str = "pending";

pub fn render(projection: &Projection) -> Result<String, String> {
    let mut body = serde_json::to_string_pretty(projection).map_err(|e| e.to_string())?;
    body.push('\n');
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observe::RosterState;

    const BUILDER_1: &str = "01a0d1f1-0aec-765f-9abe-5c21e8a04b17";
    const BUILDER_2: &str = "01a0d1f1-0aec-765f-9abe-2b7c1d0e4f58";

    fn configured(id: &str, name: Option<&str>) -> config::Seat {
        config::Seat {
            id: SeatId::parse(id).expect("a hand-written seat id parses"),
            name: name.map(str::to_string),
            model: None,
            transient: false,
            worktrees: Vec::new(),
        }
    }

    fn one_seat() -> Projection {
        Projection {
            version: VERSION,
            generated_at: "2026-09-06T00:00:00Z".to_string(),
            controller_version: "0.1.0".to_string(),
            agent_version: Some("2.1.261".to_string()),
            agent_version_expected: Some("2.1.261".to_string()),
            fleet: PolicyView {
                path: "/fleet/fleet.toml".to_string(),
                mtime: Some("2026-09-06T00:00:00Z".to_string()),
                poll_seconds: 5,
                claude_code: Some("2.1.261".to_string()),
                plugin_dir: Some("/fleet".to_string()),
            },
            fleet_parse_error: None,
            in_flight: None,
            effects: EffectsView::on(),
            grant: crate::platform::GRANT_OK.to_string(),
            grant_detail: None,
            seats: vec![SeatRow::from_observation(
                &configured(BUILDER_1, Some("Orla")),
                &SeatObservation {
                    state: RosterState::Present,
                    unknown_cause: None,
                    waiting_for: None,
                    recency_fallback: None,
                    session_id: Some("a-session".to_string()),
                    short_id: Some("aa".to_string()),
                    project: Some("demo".to_string()),
                    worktree: Some("/wt/builder-1".to_string()),
                    pidless_row: false,
                    state_names_an_end: false,
                },
                Some(42),
            )],
            orders: Vec::new(),
        }
    }

    #[test]
    fn a_clean_projection_carries_no_parse_error_key() {
        let body = render(&one_seat()).unwrap();
        assert!(!body.contains("fleet_parse_error"));
        assert!(body.ends_with("}\n"));
    }

    /// The one spelling the field's contract promises, read at the site that
    /// applies it and against an input that is NOT already in it.
    ///
    /// A fixture whose configured path is already normalised leaves this arm
    /// green under a publish site that copies the observation through, because
    /// the two sides are the same string before `dir_key` is ever asked. So the
    /// input here carries the separator and the expectation does not, and the
    /// None case is beside it: a seat with no unambiguous worktree publishes the
    /// key as null, which a reader can tell from a directory, and never as an
    /// empty string, which it cannot.
    #[test]
    fn the_row_publishes_the_directory_s_own_spelling_and_never_a_guess() {
        let row = SeatRow::from_observation(
            &configured(BUILDER_1, None),
            &SeatObservation {
                state: RosterState::Absent,
                unknown_cause: None,
                waiting_for: None,
                recency_fallback: None,
                session_id: None,
                short_id: None,
                project: Some("demo".to_string()),
                worktree: Some("/wt/builder-1/".to_string()),
                pidless_row: false,
                state_names_an_end: false,
            },
            None,
        );
        assert_eq!(row.worktree.as_deref(), Some("/wt/builder-1"));

        let nowhere = SeatRow::from_observation(
            &configured(BUILDER_2, None),
            &SeatObservation {
                state: RosterState::Absent,
                unknown_cause: None,
                waiting_for: None,
                recency_fallback: None,
                session_id: None,
                short_id: None,
                project: None,
                worktree: None,
                pidless_row: false,
                state_names_an_end: false,
            },
            None,
        );
        assert_eq!(nowhere.worktree, None);
    }

    /// The writer's own output, read back through the reader `fleet status`
    /// uses.
    ///
    /// The chain is elision then default, and each link is measured on a key
    /// `one_seat()` leaves `None`: the writer OMITS the key entirely rather
    /// than rendering it as null, and the reader supplies a default for the
    /// very keys just proved absent. The absence is asserted on keys the
    /// fixture leaves `None` and never on one it gives a value: that spelling
    /// would be an assertion no input can falsify.
    #[test]
    fn the_document_the_writer_emits_is_one_the_reader_accepts() {
        let body = render(&one_seat()).unwrap();
        for elided in [
            "roster_unknown_cause",
            "waiting_for",
            "roster_recency_fallback",
            "grant_detail",
        ] {
            assert!(
                !body.contains(elided),
                "{elided} is None here, so the writer omits the key: {body}"
            );
        }
        // The control for the loop above: a key the same row DOES carry is
        // found by the same search, so the four absences are the attribute's
        // doing and not a `contains` that matches nothing.
        assert!(body.contains("\"name\""), "{body}");

        let read: Projection = serde_json::from_str(&body).expect("the writer's own document");
        assert_eq!(read.version, VERSION);
        assert_eq!(read.fleet.poll_seconds, 5);
        assert_eq!(read.seats.len(), 1);
        assert_eq!(read.seats[0].seat.id, BUILDER_1);
        assert_eq!(read.seats[0].seat.name.as_deref(), Some("Orla"));
        assert_eq!(read.seats[0].context_tokens, Some(42));
        assert_eq!(read.fleet_parse_error, None);

        // The control: a document missing a key the writer always emits is
        // refused, so the acceptance above is the defaults' doing and not a
        // reader that accepts anything.
        let short = serde_json::to_string(&serde_json::json!({ "version": VERSION })).unwrap();
        assert!(serde_json::from_str::<Projection>(&short).is_err());
    }

    #[test]
    fn the_row_publishes_the_state_the_observation_carries() {
        let body = render(&one_seat()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["seats"][0]["roster_state"], "present");
        assert_eq!(parsed["seats"][0]["context_tokens"], 42);
        assert_eq!(parsed["seats"][0]["project"], "demo");
        assert_eq!(parsed["version"], VERSION);
    }

    /// The row's object and the one core serializes a seat as are the same
    /// document, key for key and in the same order: the projection and the
    /// stream name a seat one way, whichever half wrote it.
    #[test]
    fn a_seat_view_is_the_object_core_writes_for_the_same_seat() {
        for name in [Some("Orla"), None] {
            let seat = configured(BUILDER_1, name).as_ref();
            assert_eq!(
                serde_json::to_string(&SeatView::from(&seat)).unwrap(),
                serde_json::to_string(&seat).unwrap(),
                "{name:?}"
            );
        }
    }

    /// The name a person reads is the machine name, and a row whose id this
    /// build cannot parse still prints as the text it carries.
    #[test]
    fn a_seat_view_is_read_by_its_machine_name_or_else_its_id() {
        let orla = SeatView::from(&configured(BUILDER_1, Some("Orla")).as_ref());
        assert_eq!(orla.machine_name(), "orla-e8a04b17");
        let nameless = SeatView::from(&configured(BUILDER_2, None).as_ref());
        assert_eq!(nameless.machine_name(), "agent-1d0e4f58");
        let unread = SeatView {
            id: "builder-1".to_string(),
            name: None,
            kind: "agent".to_string(),
        };
        assert_eq!(unread.machine_name(), "builder-1");
    }
}
