//! `fleet brief`: every placeholder resolved against a real pack, and the three
//! ways it refuses — each of them with nothing on stdout, because
//! whole-or-nothing is the whole point of the verb.
//!
//! The store is held in memory here and the pack is a real folder, because the
//! pack is this verb's subject and the store is not. The one arm that drives
//! the real `bd` proves the two agree byte for byte.

mod common;

use common::{shared_store, Fixture, Scratch, StubEvents};
use fleet_core::entry::{Body, Timeline, Withdrawal};
use fleet_core::input::{DELIVERY_SCHEMA, QUESTION_SCHEMA};
use fleet_core::item::brief::{self, Packs, TRANSIENT};
use fleet_core::item::dispatch::{self, Order, Wiring};
use fleet_core::item::{show, table_at, Project, Ring, RingOutcome, Spawn, SpawnOutcome, Spawner};
use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::{Directory, Kind, SeatId, SeatRef};
use fleet_core::seat::retire;
use fleet_core::store::{AssignedItem, Bd, Item, Orders, Store};
use fleet_core::test_support::FakeStore;

/// A policy file with one guard opted out, so the on/off line is read rather
/// than assumed. It names no test command: those are the dispatch's to hand
/// in, and [`TOUCHED`] is the one every arm's render hands over.
const POLICY: &str = "[guards]\nrecord = { enabled = false }\n";

/// The builder's checks [`Rig::render`] hands the brief, as a dispatch would.
const TOUCHED: &str = "make check";

/// The order as the brief prints it, rendered from the index [`ordered`] seeds
/// and every dispatch here writes.
const ORDER: &str = "dispatch ordered by run:lead-1 at 2026-09-23T10:00:00Z";
/// Who gave [`ORDER`], and when, as the order index records it: a run, in the
/// typed form every write carries.
const BY: &str = "run:lead-1";
const AT: &str = "2026-09-23T10:00:00Z";
const ITEM: &str = "fx-1";

/// [`BY`], typed.
fn by() -> Actor {
    Actor::typed(BY).expect("typed").expect("a run")
}

struct Rig {
    fixture: Fixture,
    packs: Packs,
    project: Project,
}

/// One of the binary's own defaults, as text: the arms below read the SHIPPED
/// file and never a copy of it, and the table is where it ships now.
fn shipped(relative: &str) -> String {
    String::from_utf8(
        fleet_core::embedded::bytes(relative)
            .unwrap_or_else(|| panic!("the embedded set carries `{relative}`"))
            .to_vec(),
    )
    .expect("a default is UTF-8")
}

impl Rig {
    fn new(label: &str) -> Rig {
        Rig::declaring(label, POLICY)
    }

    /// The same tree over a policy file the arm writes, for the arms whose
    /// subject is what the project declared.
    fn declaring(label: &str, policy: &str) -> Rig {
        let fixture = Fixture::new(label);
        fixture.file("fleet.toml", policy);
        fixture.materialize_defaults();
        Rig::over(fixture)
    }

    /// The same tree, resolved again: a resolution holds the reading it was
    /// built from, so an arm that plants a defect in a pack file resolves once
    /// more and asks about the pack as it now stands.
    fn over(fixture: Fixture) -> Rig {
        let policy = table_at(&fixture.path("fleet.toml"));
        let packs = Packs::under(
            &fixture.path("packs"),
            &fixture.path(fleet_core::defaults::DIR),
        )
        .expect("the defaults resolve");
        let project = Project {
            root: fixture.root.clone(),
            name: String::from("a-project"),
            guards: policy.clone(),
            policy,
        };
        Rig {
            fixture,
            packs,
            project,
        }
    }

    fn render(&self, store: &dyn Store, seat: &str) -> Rendered {
        self.render_touched(store, seat, Some(TOUCHED))
    }

    /// The same render with the builder's checks in the arm's own hands, `None`
    /// being the dispatch that was handed none.
    fn render_touched(&self, store: &dyn Store, seat: &str, touched: Option<&str>) -> Rendered {
        self.render_as(store, ITEM, seat, touched)
    }

    /// The same render with the item named as the arm types it.
    fn render_as(
        &self,
        store: &dyn Store,
        item: &str,
        seat: &str,
        touched: Option<&str>,
    ) -> Rendered {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let answer = brief::for_item(
            &mut out,
            &mut err,
            &self.packs,
            &self.project,
            store,
            item,
            seat,
            touched,
        );
        Rendered {
            size: answer.as_ref().ok().copied(),
            code: answer.as_ref().err().map(|stop| stop.code),
            why: answer.err().map(|stop| stop.message).unwrap_or_default(),
            body: String::from_utf8(out).expect("the brief is utf-8"),
            err: String::from_utf8(err).expect("stderr is utf-8"),
        }
    }
}

struct Rendered {
    size: Option<usize>,
    code: Option<u8>,
    why: String,
    body: String,
    err: String,
}

/// A store holding one item, with the notes an arm wants it to carry.
fn store_with(notes: Option<&str>) -> FakeStore {
    let store = FakeStore::default();
    store.seed(Item {
        id: ITEM.to_string(),
        title: String::from("a ready item"),
        status: String::from("open"),
        notes: notes.map(str::to_string),
        ..Item::default()
    });
    store
}

/// A store holding the item as a dispatch by `run:lead-1` leaves its index,
/// which is what the brief is gated on and what its order is rendered from.
fn ordered() -> FakeStore {
    let store = store_with(None);
    store.amend(ITEM, |item| {
        item.orders = Some(Orders {
            by: Some(BY.to_string()),
            kind: Some(dispatch::KIND.to_string()),
            at: Some(AT.to_string()),
            ..Orders::default()
        });
    });
    store
}

/// An empty packs directory resolves rather than refusing: the binary's own
/// defaults are the bottom layer whatever is beside them, so a fleet with
/// nothing installed resolves every path to them.
///
/// The control is the packs directory that is not there at all, which is the
/// shape a machine has before `fleet pack add` has ever run.
#[test]
fn a_fleet_with_no_pack_installed_resolves_every_path_to_the_defaults() {
    let fixture = Fixture::new("no-packs");
    let defaults = fixture.materialize_defaults();
    fixture.dir("packs");
    assert!(fixture.path("packs").is_dir() && !fixture.path("not-there").exists());

    for packs in [fixture.path("packs"), fixture.path("not-there")] {
        let resolved = Packs::under(&packs, &defaults)
            .unwrap_or_else(|stop| panic!("{}: {}", packs.display(), stop.message));
        assert_eq!(
            resolved.layers.len(),
            1,
            "the defaults are the only layer: {:?}",
            resolved.layers
        );
        assert!(resolved.layers[0].defaults);
        assert!(
            resolved.resolution.files.len() > 1,
            "and they carry the whole set: {:?}",
            resolved.resolution.files
        );
        assert_eq!(
            resolved.slot(brief::BRIEF).expect("the brief resolves"),
            defaults.join(brief::BRIEF)
        );
        assert!(
            !resolved
                .read(brief::RULES)
                .expect("the rules read")
                .is_empty(),
            "the rules file reads out of the defaults"
        );
    }
}

/// The other side of the arm above, and the reason it is not one reading: a
/// defaults directory that was never materialized walks to no files, publishes
/// no registry and turns the shadow rule off, so an empty bottom layer would
/// resolve NOTHING and refuse nothing — and every reader downstream would then
/// answer "no installed pack carries `assets/brief.md`", which names the symptom
/// and not the cause. It is a refusal naming the verb that writes them.
#[test]
fn a_defaults_directory_that_was_never_written_is_refused_by_name() {
    let fixture = Fixture::new("no-defaults");
    fixture.dir("packs");
    let absent = fixture.path(fleet_core::defaults::DIR);
    assert!(!absent.exists());

    let Err(stop) = Packs::under(&fixture.path("packs"), &absent) else {
        panic!("an unwritten bottom layer is not an empty one");
    };
    assert!(
        stop.message.contains("`fleet start` writes them"),
        "the refusal does not name the verb that writes them: {}",
        stop.message
    );
    assert!(
        stop.message.contains(&absent.display().to_string()),
        "nor where it looked: {}",
        stop.message
    );

    // The control: the same call with the set written resolves, so what refused
    // is the absence and not the fixture.
    let written = fixture.materialize_defaults();
    Packs::under(&fixture.path("packs"), &written).expect("a written set resolves");
}

#[test]
fn every_placeholder_of_the_template_resolves() {
    let rig = Rig::new("whole");
    let rendered = rig.render(&ordered(), SEAT);
    let body = &rendered.body;

    for wanted in [
        ITEM,
        ORDER,
        "fx-1 · a ready item",
        SEAT,
        "a-project",
        TOUCHED,
        // One line per class, iterated rather than named: the two a pack
        // wires reach this list without an edit here, and the one switched off
        // in POLICY is what makes the other three a reading.
        "- shell-trap: on",
        "- record: off",
        "- release-ref: on",
        "- production-write: on",
        // one line out of rules.md, and one out of each schema
        "The commit, never the branch.",
        "\"spec_corrections\"",
        "\"licenses\"",
    ] {
        assert!(
            body.contains(wanted),
            "the brief carries `{wanted}`:\n{body}"
        );
    }
    assert_eq!(rendered.size, Some(body.len()));

    let template = shipped(brief::BRIEF);
    let names = placeholders(&template);
    assert!(
        names.iter().any(|name| name == "item_id"),
        "the guard's list is read off the template: {names:?}"
    );
    for name in &names {
        let token = format!("{{{name}}}");
        assert!(
            !body.contains(&token),
            "`{token}` survives into the rendered brief:\n{body}"
        );
    }
    assert_eq!(
        body.lines().next(),
        template
            .lines()
            .next()
            .map(|line| line.replace("{item_id}", ITEM))
            .as_deref(),
        "the first line is the one only `{{item_id}}` fills"
    );
}

/// Every `{name}` the template carries, by the renderer's own grammar.
fn placeholders(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let name = &after[..close];
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            names.push(name.to_string());
            rest = &after[close + 1..];
        } else {
            rest = after;
        }
    }
    names
}

/// The gate the brief hands a dispatched seat is ITS gate — the command the
/// dispatch was handed — and the project's whole suite is named as the
/// REVIEWER's, run once at the landing.
///
/// Three halves, each able to go missing on its own, so each is read: the
/// handed command reaches the seat, the whole suite is attributed rather than
/// handed over, and a dispatch handed no builder's checks gets the named absence
/// — the derivation sentence — in that hole instead.
#[test]
fn the_brief_hands_the_seat_the_touched_gate_and_the_suite_to_the_reviewer() {
    let rig = Rig::new("touched");
    let body = rig
        .render_touched(&ordered(), SEAT, Some("make the-touched-gate"))
        .body;

    // rules.md's own copy of the rule, which every brief carries. It is read
    // over the WHOLE page and every reading below is read over the checks
    // section alone: this line would otherwise answer for the section's
    // sentence, which is how the first red proof of this arm passed against a
    // broken template.
    assert!(
        body.contains("Your checks are the suites your diff touches."),
        "the every-turn rules carry the rule:\n{body}"
    );

    // Which sentence and which command sit on which side, read as two regions:
    // a page naming both commands in the wrong halves would satisfy a `contains`
    // over the body and still hand the seat the landing's suite.
    let (seats, reviewers) = checks_halves(&body);
    for wanted in ["the suites your diff touches", "make the-touched-gate"] {
        assert!(
            seats.contains(wanted),
            "the seat's half of the checks section carries `{wanted}`:\n{seats}"
        );
    }
    assert!(
        !seats.contains(brief::DERIVE_TOUCHED),
        "and never the absence beside the command it was handed:\n{seats}"
    );
    for wanted in [
        "the **reviewer's**",
        "`fleet land` runs the command\nit is handed, once, on the rebased tree",
    ] {
        assert!(
            reviewers.contains(wanted),
            "the reviewer's half carries `{wanted}`:\n{reviewers}"
        );
    }
    assert!(
        !reviewers.contains("make the-touched-gate"),
        "and the seat's gate is never offered as the reviewer's:\n{reviewers}"
    );

    // A dispatch handed no builder's checks: the hole is filled by the named
    // absence, which says what was not handed over and what to run instead.
    let derived = rig.render_touched(&ordered(), SEAT, None).body;
    let (seats, _) = checks_halves(&derived);
    assert!(
        seats.contains(brief::DERIVE_TOUCHED),
        "absent builder's checks render the derivation sentence:\n{seats}"
    );
    assert!(
        seats.contains("no touched command was handed to this dispatch"),
        "and the sentence names the absence:\n{seats}"
    );
    assert!(
        !seats.contains("make the-touched-gate") && !seats.contains(TOUCHED),
        "and names no command:\n{seats}"
    );
    // A blank command is no command.
    let blank = rig.render_touched(&ordered(), SEAT, Some("  ")).body;
    assert!(checks_halves(&blank).0.contains(brief::DERIVE_TOUCHED));

    // The control, observed failing: a brief edited back to a section with no
    // gate in it fails the first reading above, so what is pinned here is the
    // rendered sentence and not a string that happens to sit somewhere in the
    // page.
    let rig = Rig::new("touched-broken");
    rig.fixture.file(
        "defaults/assets/brief.md",
        "# {item_id}\n\n## The suite\n\n```\n{project}\n```\n",
    );
    let broken = Rig::over(rig.fixture).render(&ordered(), SEAT).body;
    assert!(
        !broken.contains("## Your checks"),
        "the readings are of the template, which this one no longer carries:\n{broken}"
    );
}

/// The checks section's two halves — the seat's and the reviewer's — bounded at
/// the next heading.
///
/// BOUNDED, because the page's later sections quote the same rule: a reading
/// that ran to the end of the body would be answered by the every-turn rules
/// whatever the section above them said.
fn checks_halves(body: &str) -> (&str, &str) {
    let after = body
        .split_once("## Your checks")
        .expect("the brief carries the checks section")
        .1;
    let section = after
        .split_once("\n## ")
        .map(|(head, _)| head)
        .unwrap_or(after);
    section
        .split_once("The project's whole suite is")
        .expect("the checks section names the reviewer's half")
}

/// A policy file that still sets `[gates] touched` is REFUSED, by name, with
/// the pack setting that replaces it — and nothing reaches stdout. Read as
/// absent instead, the seat would be handed the derivation sentence while the
/// person who wrote the key believes the seat was handed their command.
#[test]
fn a_policy_file_setting_gates_touched_is_refused_naming_the_pack_setting() {
    let rig = Rig::declaring(
        "moved-touched",
        "[gates]\ntouched = \"make the-touched-gate\"\n\n[guards]\nrecord = { enabled = false }\n",
    );
    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(
        rendered.code,
        Some(1),
        "refused on the record: {}",
        rendered.why
    );
    assert!(
        rendered.why.contains("[gates] touched")
            && rendered
                .why
                .contains("`takeoff.touched` under [packs.tiny]")
            && rendered.why.contains("fleet dispatch --touched <command>"),
        "the line names the key and where it is set instead: {}",
        rendered.why
    );
    assert!(
        rendered.body.is_empty(),
        "nothing on stdout: {}",
        rendered.body
    );

    // The control: the same policy with the key taken out renders.
    let rig = Rig::new("moved-touched-control");
    assert_eq!(rig.render(&ordered(), SEAT).code, None);
}

#[test]
fn a_dispatch_with_no_seat_yet_says_so_where_the_seat_goes() {
    let rig = Rig::new("transient");
    let rendered = rig.render(&ordered(), TRANSIENT);
    assert!(rendered.body.contains(TRANSIENT), "{}", rendered.body);
}

#[test]
fn an_unknown_placeholder_exits_three_and_writes_nothing() {
    let rig = Rig::new("unknown");
    rig.fixture
        .file("defaults/assets/brief.md", "you are {nobody}\n");
    let rig = Rig::over(rig.fixture);

    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(rendered.code, Some(3), "{}", rendered.why);
    assert!(rendered.why.contains("{nobody}"), "{}", rendered.why);
    assert_eq!(rendered.body.len(), 0, "nothing reaches stdout");
}

#[test]
fn a_missing_rules_file_exits_three_and_writes_nothing() {
    let rig = Rig::new("no-rules");
    std::fs::remove_file(rig.fixture.path("defaults/assets/rules.md"))
        .expect("the rules file is removed");
    let rig = Rig::over(rig.fixture);

    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(rendered.code, Some(3), "{}", rendered.why);
    assert!(rendered.why.contains("assets/rules.md"), "{}", rendered.why);
    assert_eq!(rendered.body.len(), 0, "nothing reaches stdout");
}

#[test]
fn an_item_with_no_order_index_is_refused_and_writes_nothing() {
    let rig = Rig::new("unordered");
    let rendered = rig.render(&store_with(None), SEAT);
    assert_eq!(rendered.code, Some(1), "{}", rendered.why);
    assert!(rendered.why.contains("no order index"), "{}", rendered.why);
    assert_eq!(rendered.body.len(), 0, "nothing reaches stdout");

    // An index that is there and cannot be read is refused by its own line, and
    // so is one naming nobody: the order the brief prints is rendered from who
    // gave it. Each is the ordered store with one reading taken away, so what
    // refuses is that reading and not the rest of the record.
    let unreadable = ordered();
    unreadable.amend(ITEM, |item| {
        item.orders = None;
        item.has_orders_key = true;
    });
    let nobody = ordered();
    nobody.amend(ITEM, |item| {
        if let Some(index) = item.orders.as_mut() {
            index.by = None;
        }
    });
    for (store, wanted) in [
        (&unreadable, "order index is not an object"),
        (&nobody, "order index names no dispatcher"),
    ] {
        let rendered = rig.render(store, SEAT);
        assert_eq!(rendered.code, Some(1), "{}", rendered.why);
        assert!(rendered.why.contains(wanted), "{}", rendered.why);
        assert_eq!(rendered.body.len(), 0, "nothing reaches stdout");
    }
    assert_eq!(
        rig.render(&ordered(), SEAT).code,
        None,
        "the control renders"
    );
}

// ---- the gate is the index ---------------------------------------------------

/// Orla's id: the seat [`Rig::dispatch`] names, and the one `retire` releases.
const ORLA: &str = "01a0d1f1-0aec-765f-9abe-d4f993b9739a";

/// Orla's machine name — `{seat}` in every brief this suite renders for a
/// named seat. `her_machine_name_is_the_seat_every_arm_renders` holds it to
/// the id above.
const SEAT: &str = "orla-93b9739a";

/// Orla, a seat this machine runs.
fn orla() -> SeatRef {
    SeatRef {
        id: SeatId::parse(ORLA).expect("the rig's seat id parses"),
        name: Some(String::from("Orla")),
        kind: Kind::Agent,
    }
}

/// The fleet the dispatches below are given in: Orla, listed and running.
fn directory() -> Directory {
    Directory {
        listed: vec![orla()],
        running: vec![orla()],
    }
}

#[test]
fn her_machine_name_is_the_seat_every_arm_renders() {
    assert_eq!(orla().machine_name(), SEAT);
}

/// `--to orla` renders the seat's machine name where the brief says who it
/// is for — `fleet brief` resolves the argument among the running seats and
/// hands the renderer that — and no `--to` renders the transient placeholder.
#[test]
fn a_named_brief_says_the_machine_name_and_an_unnamed_one_says_transient() {
    let rig = Rig::new("you-are");
    let to = directory()
        .resolve_running("orla")
        .expect("Orla runs here")
        .machine_name();
    let named = rig.render(&ordered(), &to);
    assert_eq!(named.code, None, "{}", named.why);
    assert!(
        named.body.contains("You are `orla-93b9739a`"),
        "{}",
        named.body
    );

    let unnamed = rig.render(&ordered(), TRANSIENT);
    assert_eq!(unnamed.code, None, "{}", unnamed.why);
    assert!(
        unnamed.body.contains("You are `(transient)`"),
        "{}",
        unnamed.body
    );
}

/// A ring that is always heard, so a dispatch to a named seat completes.
struct Heard;

impl Ring for Heard {
    fn ring(&self, _seat: &str, _text: &str) -> RingOutcome {
        RingOutcome::Delivered
    }
}

/// A spawner answering one outcome, for the dispatch that names no seat.
struct Spawns(SpawnOutcome);

impl Spawner for Spawns {
    fn spawn(&self, _ask: &Spawn) -> SpawnOutcome {
        self.0.clone()
    }
}

impl Rig {
    /// The item ordered through `fleet dispatch` itself, and not seeded: the
    /// three writes this arm's brief reads are the ones the verb makes. `to`
    /// names the seat, or `None` hands the order to the spawner.
    fn dispatch(
        &self,
        store: &dyn Store,
        to: Option<&str>,
        spawner: &dyn Spawner,
    ) -> Result<(), String> {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        dispatch::dispatch(
            &mut out,
            &mut err,
            &Order {
                item: ITEM,
                to,
                by: &by(),
                at: AT,
                brief: None,
                base: None,
                model: None,
                touched: Some(TOUCHED),
            },
            &Wiring {
                store,
                project: &self.project,
                packs: &self.packs,
                briefs_dir: &self.fixture.path("briefs"),
                seats: &directory(),
                ring: &Heard,
                spawner,
                events: &StubEvents::default(),
            },
        )
        .map(|_| ())
        .map_err(|refused| refused.stop.message)
    }
}

/// A WITHDRAWN order is refused (fleet-nv0). Both withdrawals leave the ordered
/// entry standing — a timeline is append-only — and unset the index, so a gate
/// that took the last ordered entry for an order would hand a seat a brief for
/// an item nobody holds.
///
/// The two withdrawals are the retire's and the refused spawn's, each written by
/// its own verb rather than seeded, and each read first for the entries it left
/// behind: the arm is only a proof while the ordered entry is still there to
/// mislead.
#[test]
fn a_withdrawn_order_is_refused_and_writes_nothing() {
    let rig = Rig::new("withdrawn");

    // The retire's: dispatched to a named seat, then released by `retire`.
    let retired = store_with(None);
    rig.dispatch(
        &retired,
        Some("orla"),
        &Spawns(SpawnOutcome::Refused(String::new())),
    )
    .expect("the dispatch lands");
    let row = AssignedItem {
        id: ITEM.to_string(),
        status: String::from("open"),
        ..AssignedItem::default()
    };
    let orla = SeatId::parse(ORLA).expect("Orla's id parses");
    retire::withdraw(&retired, &[row], &orla, SEAT, &by()).expect("the retire withdraws");

    // The refused spawn's: dispatched to no seat, withdrawn in the same act.
    let refused = store_with(None);
    let why = rig
        .dispatch(
            &refused,
            None,
            &Spawns(SpawnOutcome::Refused(String::from(
                "no seat can be spawned",
            ))),
        )
        .expect_err("a refused spawn refuses the dispatch");
    assert!(why.contains("withdrawn"), "{why}");

    for (store, line, why) in [
        (&retired, "retire", Withdrawal::Retire),
        (&refused, "spawn refused", Withdrawal::SpawnRefused),
    ] {
        let read = store.show(ITEM).expect("the item reads back");
        let entries = store.timeline(ITEM).expect("the timeline reads back");
        assert!(
            matches!(
                entries.first().map(|entry| &entry.body),
                Some(Body::Ordered(_))
            ),
            "{line}: the ordered entry stands under the withdrawal: {entries:?}"
        );
        assert!(
            matches!(
                entries.last().map(|entry| &entry.body),
                Some(Body::OrderWithdrawn(withdrawn)) if withdrawn.why == why
            ),
            "{line}: and the withdrawal is the last entry: {entries:?}"
        );
        assert!(Timeline(&entries).current_order().is_none(), "{line}");
        assert!(
            !read.has_orders_key,
            "and the index is gone: {:?}",
            read.orders
        );

        let rendered = rig.render(store, SEAT);
        assert_eq!(
            rendered.code,
            Some(1),
            "{line}: refused on the record: {}",
            rendered.why
        );
        assert!(
            rendered.why.contains("no order index"),
            "{line}: {}",
            rendered.why
        );
        assert!(
            rendered.body.is_empty(),
            "{line}: nothing on stdout:\n{}",
            rendered.body
        );
    }
}

/// A person's note that happens to carry the mark is not an order: an item
/// with no index is refused whatever its notes say.
#[test]
fn a_note_saying_orders_given_on_an_unindexed_item_is_refused() {
    let rig = Rig::new("human-note");
    let store = store_with(Some(
        "asked on the call whether the orders given last week still stand",
    ));
    let rendered = rig.render(&store, SEAT);
    assert_eq!(rendered.code, Some(1), "{}", rendered.why);
    assert!(rendered.why.contains("no order index"), "{}", rendered.why);
    assert!(
        rendered.body.is_empty(),
        "nothing on stdout:\n{}",
        rendered.body
    );
}

/// The control on the two arms above: the same dispatch, not withdrawn, still
/// gets its brief — and the order it carries is the one the dispatch wrote.
#[test]
fn a_dispatched_item_still_gets_its_brief() {
    let rig = Rig::new("dispatched");
    let store = store_with(None);
    rig.dispatch(
        &store,
        Some("orla"),
        &Spawns(SpawnOutcome::Refused(String::new())),
    )
    .expect("the dispatch lands");

    let rendered = rig.render(&store, SEAT);
    assert_eq!(rendered.code, None, "{}", rendered.why);
    assert!(
        rendered.body.contains(&format!("```\n{ORDER}\n```")),
        "the brief carries the order the dispatch wrote:\n{}",
        rendered.body
    );
    assert_eq!(
        std::fs::read_to_string(rig.fixture.path("briefs").join(format!("{ITEM}.md")))
            .expect("the dispatch wrote its brief"),
        rendered.body,
        "and it is the brief the dispatch handed its seat, byte for byte"
    );
}

/// THE ORDER IS THE INDEX'S, rendered: who gave it and when, off
/// `fleet.orders`, with no template and no note behind it. The index here names
/// a dispatcher and a time no other arm's does, so the block is read off this
/// record and not off a constant.
#[test]
fn the_order_block_reads_who_gave_it_and_when_off_the_index() {
    let rig = Rig::new("order-text");
    let store = ordered();
    store.amend(ITEM, |item| {
        item.orders = Some(Orders {
            by: Some(String::from("seat:01a0d1f1-0aec-765f-9abe-0000000000b1")),
            kind: Some(dispatch::KIND.to_string()),
            at: Some(String::from("2026-09-24T08:15:00Z")),
            ..Orders::default()
        });
    });

    let rendered = rig.render(&store, SEAT);
    assert_eq!(rendered.code, None, "{}", rendered.why);
    assert!(
        rendered.body.contains(
            "## Your order\n\n```\ndispatch ordered by \
             seat:01a0d1f1-0aec-765f-9abe-0000000000b1 at 2026-09-24T08:15:00Z\n```"
        ),
        "the order block is the index's dispatcher and time:\n{}",
        rendered.body
    );
    let index = store
        .show(ITEM)
        .expect("the item reads")
        .orders
        .expect("the index is an object");
    assert_eq!(
        brief::order_text(&index),
        "dispatch ordered by seat:01a0d1f1-0aec-765f-9abe-0000000000b1 at 2026-09-24T08:15:00Z"
    );
}

/// The dispatch note's template is retired with the note: the binary's own
/// defaults carry no `assets/dispatch-note.md`, and the shadow registry — the
/// list of paths a pack may replace — names no such slot.
#[test]
fn the_defaults_carry_no_dispatch_note_and_the_registry_no_row_for_one() {
    assert!(
        fleet_core::embedded::bytes("assets/dispatch-note.md").is_none(),
        "the embedded defaults carry no dispatch note"
    );
    let registry = shipped("assets/shadow-registry.toml");
    assert!(
        !registry.contains("assets/dispatch-note.md"),
        "the registry names no dispatch note:\n{registry}"
    );
    // The control: the registry is the one read, and it names its neighbours.
    assert!(
        registry.contains("path = \"assets/brief.md\""),
        "{registry}"
    );
}

/// An item named by its suffix gets the brief its full id gets, byte for byte:
/// the verb resolves the argument once and reads the rendering, the order and
/// the brief's own id off the id the store answered.
#[test]
fn a_suffix_gets_the_brief_of_the_full_id_it_resolves_to() {
    let rig = Rig::new("suffix");
    let store = ordered();
    let whole = rig.render(&store, SEAT);
    assert_eq!(whole.code, None, "{}", whole.why);

    let suffix = ITEM
        .strip_prefix("fx-")
        .expect("the item is filed under fx-");
    let named = rig.render_as(&store, suffix, SEAT, Some(TOUCHED));
    assert_eq!(named.code, None, "{}", named.why);
    assert_eq!(named.body, whole.body, "the brief of the full id");
}

#[test]
fn a_pack_on_top_shadows_the_brief_whole() {
    let rig = Rig::new("shadow");
    rig.fixture
        .file(
            "packs/top/pack.toml",
            "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
        )
        .file(
            "packs/top/assets/brief.md",
            "THE SHADOW SPEAKS for {item_id}\n",
        );
    let rig = Rig::over(rig.fixture);

    let rendered = rig.render(&ordered(), SEAT);
    assert!(
        rendered.body.starts_with("THE SHADOW SPEAKS"),
        "{}",
        rendered.body
    );
    assert!(
        !rendered.body.contains("The commit, never the branch."),
        "the shadow replaces the file WHOLE, so core's sections are gone:\n{}",
        rendered.body
    );
    assert_eq!(rendered.size, Some(rendered.body.len()));
}

/// THE BRIEF SHOWS THE SCHEMA A DELIVERY IS READ AGAINST, and no note grammar:
/// the seat hands in JSON, so the page carries the resolved schema's text
/// whole, in a `json` block, and never the prose note's marker line.
#[test]
fn the_brief_shows_the_delivery_schema_and_no_note_grammar() {
    let rig = Rig::new("schema");
    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(rendered.code, None, "{}", rendered.why);
    let body = &rendered.body;

    let schema = shipped(DELIVERY_SCHEMA);
    assert!(
        body.contains("## The delivery you will hand in\n\n`fleet deliver --delivery <file>`"),
        "the section names the verb and its flag:\n{body}"
    );
    assert!(
        body.contains(&format!("```json\n{}\n```", schema.trim_end())),
        "the schema's text, whole, in a json block:\n{body}"
    );
    assert!(
        !body.contains("DELIVERED <sha>"),
        "no delivery-note grammar line:\n{body}"
    );
}

/// A SCHEMA THAT DOES NOT SAY WHAT THE VERB READS IS NO BRIEF. A pack layer
/// shadowing the delivery schema with a property taken out would teach a seat
/// a delivery `fleet deliver` refuses, so the brief is exit 3 naming the path
/// and the disagreement, with nothing on stdout.
#[test]
fn a_layer_shadowing_the_schema_with_a_property_removed_is_no_brief() {
    let mut schema: serde_json::Value =
        serde_json::from_str(&shipped(DELIVERY_SCHEMA)).expect("the schema is JSON");
    schema["properties"]
        .as_object_mut()
        .expect("properties")
        .remove("covers");
    let rig = Rig::new("schema-shadow");
    rig.fixture
        .file(
            "packs/top/pack.toml",
            "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
        )
        .file(
            &format!("packs/top/{DELIVERY_SCHEMA}"),
            &serde_json::to_string_pretty(&schema).expect("it writes"),
        );
    let rig = Rig::over(rig.fixture);

    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(rendered.code, Some(3), "{}", rendered.why);
    assert!(
        rendered.why.starts_with(&format!(
            "{DELIVERY_SCHEMA} as the layers resolve it does not describe what fleet deliver \
             reads: "
        )),
        "{}",
        rendered.why
    );
    assert!(rendered.why.contains("covers"), "{}", rendered.why);
    assert!(rendered.body.is_empty(), "nothing on stdout");

    // The control: the shipped schema, through the same layers, renders.
    let whole = Rig::new("schema-whole").render(&ordered(), SEAT);
    assert_eq!(whole.code, None, "{}", whole.why);
}

/// THE BRIEF SHOWS THE SCHEMA A QUESTION IS READ AGAINST, under its own
/// section: the verb and its flag, what a hold commits, that the item is held
/// until a person clears it, and the resolved schema's text whole in a `json`
/// block.
#[test]
fn the_brief_shows_the_question_schema_under_its_own_section() {
    let rig = Rig::new("question-schema");
    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(rendered.code, None, "{}", rendered.why);
    let body = &rendered.body;

    let section = body
        .split_once("## If a question blocks you\n")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| panic!("the brief carries the question's section:\n{body}"));
    // The words, whatever the wrapping.
    let said = section.split_whitespace().collect::<Vec<_>>().join(" ");
    for wanted in [
        "`fleet hold --question <file>`",
        "everything your tree holds is committed on the work branch",
        "The item is held until a person clears it",
        "A guess written into a diff costs more than a question.",
    ] {
        assert!(
            said.contains(wanted),
            "the section says `{wanted}`:\n{section}"
        );
    }
    let schema = shipped(QUESTION_SCHEMA);
    assert!(
        section.contains(&format!("```json\n{}\n```", schema.trim_end())),
        "the schema's text, whole, in a json block:\n{section}"
    );
}

/// The question's schema is checked against the type `fleet hold` reads, as
/// the delivery's is: a layer shadowing it with a property taken out is exit 3
/// naming the path and the disagreement, with nothing on stdout.
#[test]
fn a_layer_shadowing_the_question_schema_with_a_property_removed_is_no_brief() {
    let mut schema: serde_json::Value =
        serde_json::from_str(&shipped(QUESTION_SCHEMA)).expect("the schema is JSON");
    schema["properties"]
        .as_object_mut()
        .expect("properties")
        .remove("context");
    let rig = Rig::new("question-shadow");
    rig.fixture
        .file(
            "packs/top/pack.toml",
            "[pack]\nname = \"top\"\nversion = \"1\"\nschema = 3\n",
        )
        .file(
            &format!("packs/top/{QUESTION_SCHEMA}"),
            &serde_json::to_string_pretty(&schema).expect("it writes"),
        );
    let rig = Rig::over(rig.fixture);

    let rendered = rig.render(&ordered(), SEAT);
    assert_eq!(rendered.code, Some(3), "{}", rendered.why);
    assert!(
        rendered.why.starts_with(&format!(
            "{QUESTION_SCHEMA} as the layers resolve it does not describe what fleet hold reads: "
        )),
        "{}",
        rendered.why
    );
    assert!(rendered.why.contains("context"), "{}", rendered.why);
    assert!(rendered.body.is_empty(), "nothing on stdout");
}

/// The fidelity control on every arm above: the readings the fake answers are
/// the readings `bd` answers, measured by rendering the same brief through both
/// and comparing the bytes.
#[test]
fn the_real_store_renders_the_same_brief_as_the_one_held_in_memory() {
    let scratch = shared_store("brief");
    let item = scratch.item("a ready item");
    let index = dispatch::index(BY, dispatch::KIND, None, AT);
    let out = scratch.bd(&["update", &item, "--metadata", &index, "--actor", BY]);
    assert!(out.status.success(), "the order index is written");

    let rig = Rig::new("fidelity");
    let real = Bd::at(&scratch.root);
    let record = real.show(&item).expect("bd answers about the item");

    let fake = FakeStore::default();
    fake.seed(record.clone());

    let through = |store: &dyn Store| {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        brief::for_item(
            &mut out,
            &mut err,
            &rig.packs,
            &rig.project,
            store,
            &item,
            SEAT,
            Some(TOUCHED),
        )
        .expect("the brief renders");
        String::from_utf8(out).expect("utf-8")
    };

    assert_eq!(through(&real), through(&fake), "byte for byte");
    assert_eq!(record.status, "open");
    assert_eq!(
        record.orders.and_then(|index| index.by).as_deref(),
        Some(BY),
        "the order index bd stored is the one that was written"
    );
}

/// An item carrying an entry gets fleet's rendering of it in its brief, and
/// never bd's: bd's own `show` text prints every comment under a `COMMENTS`
/// header — measured on 1.3.0 — so the first entry a verb appended would reach
/// every brief as the raw JSON the store keeps.
///
/// The person's comment beside the entry is the control on "never bd's": it is
/// on the item, and fleet's rendering leaves it out.
#[test]
fn an_item_carrying_an_entry_is_briefed_through_fleets_rendering() {
    let scratch = shared_store("brief");
    let item = scratch.item("an item with a record");
    let index = dispatch::index(BY, dispatch::KIND, None, AT);
    let out = scratch.bd(&["update", &item, "--metadata", &index, "--actor", BY]);
    assert!(out.status.success(), "the order index is written");
    let real = Bd::at(&scratch.root);
    real.append(&item, &common::a_delivery(common::A_COMMIT), &by())
        .expect("the entry is appended");
    let out = scratch.bd(&["comments", "add", &item, "a person's own words"]);
    assert!(out.status.success(), "the person's comment is written");

    let rig = Rig::new("entry");
    let rendered = rig.render_as(&real, &item, SEAT, Some(TOUCHED));
    assert_eq!(rendered.code, None, "{}", rendered.why);
    let body = &rendered.body;
    assert!(!body.contains("COMMENTS"), "no bd header:\n{body}");
    assert!(!body.contains("{\"fleet.entry\""), "no raw entry:\n{body}");
    assert!(!body.contains("a person's own words"), "{body}");

    // The block is `fleet item show`'s text, whole, over the one entry.
    let record = real.show(&item).expect("bd answers about the item");
    let timeline = real.timeline(&item).expect("bd answers the timeline");
    assert_eq!(timeline.len(), 1, "the entry, and not the person's comment");
    let block = format!(
        "## The item\n\n```\n{}\n```\n",
        show::render(&record, &timeline)
    );
    assert!(
        body.contains(&block),
        "the item block is show::render:\n{block}\n--- in ---\n{body}"
    );
    assert!(block.contains(&format!("delivered {}", common::A_COMMIT)));
}

/// The store's human rendering sometimes carries a tip line that NAMES A
/// PROVIDER, which a pack template may not do — and which would make one item
/// render two ways depending on whether anybody had looked at it before.
///
/// The tip is not asserted here, in either direction. Its budget is not the
/// store's: three fresh stores measured on this box tipped on the second and
/// third and not the first, and a run under a private HOME did not tip at all.
/// An arm that demanded it would be red on somebody else's morning. What IS
/// deterministic is measured instead — the quiet read is stable, and the brief
/// no longer carries the store's rendering at all: `{item}` is fleet's own
/// (`show::render`), so a tip in bd's text cannot reach a seat.
#[test]
fn the_stores_own_rendering_is_stable_and_never_reaches_the_seat() {
    let scratch = Scratch::new("tip");
    let item = scratch.item("a ready item");
    let store = Bd::at(&scratch.root);

    let first = store.show_text(&item).expect("bd renders the item");
    assert!(!first.contains("Tip:"), "the quiet read is clean:\n{first}");
    for _ in 0..3 {
        assert_eq!(
            store.show_text(&item).expect("a further read"),
            first,
            "the rendering is stable across reads, which is what a brief needs"
        );
    }

    // A store whose own rendering DOES carry the tip leaves the seat's first
    // turn clean, because the brief never reads that rendering. Before
    // fleet-zlk.4 `{item}` was the store's text verbatim, and this arm was the
    // control that watched the tip arrive.
    let tipped = ordered();
    tipped.set_text(
        ITEM,
        &format!("{first}\n💡 Tip: run 'bd setup claude' for CLI-only mode\n"),
    );
    let rig = Rig::new("tip-render");
    let rendered = rig.render(&tipped, SEAT);
    assert_eq!(rendered.code, None, "{}", rendered.why);
    assert!(
        !rendered.body.contains("bd setup claude"),
        "the store's rendering stays out of the brief:\n{}",
        rendered.body
    );
}

/// The gas-city lesson this slice owes by name: the first turn's size is a
/// number worth watching, so the verb prints it and the number is the body's
/// own length rather than a second count of something else.
mod lessons {
    use super::*;

    #[test]
    fn the_prompt_floor_is_paid_on_every_restart() {
        let rig = Rig::new("floor");
        let rendered = rig.render(&ordered(), SEAT);

        assert_eq!(
            rendered.size,
            Some(rendered.body.len()),
            "the answer is the body's own length"
        );
        assert_eq!(
            rendered.err.trim(),
            format!("brief: {} bytes", rendered.body.len()),
            "the size line is stderr's, and it counts the bytes stdout got"
        );

        // The control: a longer body reports a different number, so the
        // assertion above reads the render and not a constant.
        let store = ordered();
        store.amend(ITEM, |item| {
            item.title =
                String::from("an item with a very much longer rendering than the one above");
        });
        let longer = rig.render(&store, SEAT);
        assert!(longer.body.len() > rendered.body.len());
        assert!(
            longer
                .err
                .contains(&format!("brief: {} bytes", longer.body.len())),
            "{}",
            longer.err
        );
    }
}
