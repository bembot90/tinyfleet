//! `fleet brief` (packs PRD R5, R19): every placeholder resolved against a real
//! pack, and the three ways it refuses — each of them with nothing on stdout,
//! because whole-or-nothing is the whole point of the verb.
//!
//! The store is held in memory here and the pack is a real folder, because the
//! pack is this verb's subject and the store is not. The one arm that drives
//! the real `bd` proves the two agree byte for byte.

mod common;

use common::{shared_store, Fixture, Scratch};
use fleet_core::item::brief::{self, Packs, TRANSIENT};
use fleet_core::item::{table_at, Project};
use fleet_core::store::{Bd, Item, Store};
use fleet_core::test_support::FakeStore;

/// A policy file with both readings in it: a suite the brief prints, and one
/// guard opted out so the on/off line is read rather than assumed.
const POLICY: &str = "[gates]\nsuite = \"make check\"\n\n[guards]\nrecord = { enabled = false }\n";

const ORDER: &str = "dispatched by architect-1 — orders given";
const ITEM: &str = "fx-1";

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
        let gates = table_at(&fixture.path("fleet.toml"));
        let packs = Packs::under(
            &fixture.path("packs"),
            &fixture.path(fleet_core::defaults::DIR),
        )
        .expect("the defaults resolve");
        let project = Project {
            root: fixture.root.clone(),
            name: String::from("a-project"),
            guards: gates.clone(),
            gates,
        };
        Rig {
            fixture,
            packs,
            project,
        }
    }

    fn render(&self, store: &dyn Store, seat: &str) -> Rendered {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let answer = brief::for_item(
            &mut out,
            &mut err,
            &self.packs,
            &self.project,
            store,
            ITEM,
            seat,
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
        status: String::from("open"),
        notes: notes.map(str::to_string),
        ..Item::default()
    });
    store.set_text(ITEM, "fx-1 · a ready item\nOPEN\n");
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
    let rendered = rig.render(&store_with(Some(ORDER)), "s1");
    let body = &rendered.body;

    for wanted in [
        ITEM,
        ORDER,
        "fx-1 · a ready item",
        "s1",
        "a-project",
        "make check",
        // One line per class, iterated rather than named: the two a pack
        // wires reach this list without an edit here, and the one switched off
        // in POLICY is what makes the other three a reading.
        "- shell-trap: on",
        "- record: off",
        "- release-ref: on",
        "- production-write: on",
        // one line out of rules.md, and one out of delivery-note.md
        "The commit, never the branch.",
        "spec corrections:",
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
/// The REVIEWER's brief: every placeholder resolves, the pack it sits in still
/// checks out, and the registry names it (flights PRD R14, decision D2).
///
/// The registry half is the existence test: a row naming a path the pack does
/// not hold is a defect of the pack publishing the registry.
#[test]
fn the_review_brief_resolves_whole_and_the_registry_names_it() {
    let rig = Rig::new("review-brief");
    let body = brief::review_text(
        &rig.packs,
        &rig.project,
        &brief::Delivery {
            id: ITEM,
            text: "fx-1 · a ready item\n",
            delivery: "DELIVERED abc1234 — a-builder\ncommit:  abc1234\n",
            size: "size: 3 file(s), +40, -2 — tests: yes, executable: no",
        },
    )
    .expect("the reviewer's brief renders whole");

    for wanted in [
        ITEM,
        "fx-1 · a ready item",
        "a-project",
        "make check",
        "size: 3 file(s), +40, -2",
        "DELIVERED abc1234",
        "fleet review fx-1 --land",
        "fleet review fx-1 --return",
        // one line out of rules.md, which every brief carries
        "The commit, never the branch.",
    ] {
        assert!(
            body.contains(wanted),
            "the reviewer's brief carries `{wanted}`:\n{body}"
        );
    }
    assert!(
        !body.contains("{"),
        "and nothing is left unresolved:\n{body}"
    );

    // The template's own placeholder list, read off the shipped file: a name
    // the renderer does not offer would be a refusal rather than a literal, so
    // this is the guard against a template that grew one.
    let template = shipped(brief::REVIEW_BRIEF);
    let mut names = placeholders(&template);
    names.sort();
    names.dedup();
    assert_eq!(
        names,
        vec!["delivery", "item", "item_id", "project", "rules", "size", "suite"],
        "the template writes exactly the placeholders the renderer resolves"
    );

    // The registry that publishes it, off the same embedded set.
    let registry = shipped(fleet_core::registry::REGISTRY);
    assert!(
        registry.contains("assets/review-brief.md"),
        "and the registry names it:\n{registry}"
    );
}

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

/// The gate the brief hands a dispatched seat is ITS gate — the suites its diff
/// touches — and the project's whole suite is named as the REVIEWER's, run once
/// at the landing.
///
/// Three halves, each able to go missing on its own, so each is read: the
/// declared command reaches the seat, the whole suite is attributed rather than
/// handed over, and a project that declares no builder's gate gets the
/// derivation sentence in that hole instead of the landing's command.
#[test]
fn the_brief_hands_the_seat_the_touched_gate_and_the_suite_to_the_reviewer() {
    const DECLARED: &str =
        "[gates]\nsuite = \"make the-whole-suite\"\ntouched = \"make the-touched-gate\"\n";
    let rig = Rig::declaring("touched", DECLARED);
    let body = rig.render(&store_with(Some(ORDER)), "s1").body;

    // rules.md's own copy of the rule, which every brief carries. It is read
    // over the WHOLE page and every reading below is read over the gate section
    // alone: this line would otherwise answer for the section's sentence, which
    // is how the first red proof of this arm passed against a broken template.
    assert!(
        body.contains("The gate is the suites your diff touches."),
        "the every-turn rules carry the rule:\n{body}"
    );

    // Which sentence and which command sit on which side, read as two regions:
    // a page naming both commands in the wrong halves would satisfy a `contains`
    // over the body and still hand the seat the landing's suite.
    let (seats, reviewers) = gate_halves(&body);
    for wanted in ["the suites your diff touches", "make the-touched-gate"] {
        assert!(
            seats.contains(wanted),
            "the seat's half of the gate section carries `{wanted}`:\n{seats}"
        );
    }
    assert!(
        !seats.contains("make the-whole-suite"),
        "and never the landing's suite:\n{seats}"
    );
    for wanted in [
        "the **reviewer's**",
        "`fleet land` runs it once",
        "make the-whole-suite",
    ] {
        assert!(
            reviewers.contains(wanted),
            "the reviewer's half carries `{wanted}`:\n{reviewers}"
        );
    }

    // A project that declares the landing's suite and no builder's gate: the
    // hole is filled by the derivation sentence, and not by the suite.
    let bare = Rig::declaring("touched-absent", POLICY);
    let derived = bare.render(&store_with(Some(ORDER)), "s1").body;
    let (seats, _) = gate_halves(&derived);
    assert!(
        seats.contains(brief::DERIVE_TOUCHED),
        "an undeclared builder's gate renders the derivation sentence:\n{seats}"
    );
    assert!(
        !seats.contains("make check"),
        "and never the landing's suite:\n{seats}"
    );

    // The control, observed failing: a brief edited back to the suite alone
    // fails the first reading above, so what is pinned here is the rendered
    // sentence and not a string that happens to sit somewhere in the page.
    let rig = Rig::new("touched-broken");
    rig.fixture.file(
        "defaults/assets/brief.md",
        "# {item_id}\n\n## The suite\n\n```\n{suite}\n```\n",
    );
    let broken = Rig::over(rig.fixture)
        .render(&store_with(Some(ORDER)), "s1")
        .body;
    assert!(
        !broken.contains("## Your gate"),
        "the readings are of the template, which this one no longer carries:\n{broken}"
    );
}

/// The gate section's two halves — the seat's and the reviewer's — bounded at
/// the next heading.
///
/// BOUNDED, because the page's later sections quote the same rule: a reading
/// that ran to the end of the body would be answered by the every-turn rules
/// whatever the section above them said.
fn gate_halves(body: &str) -> (&str, &str) {
    let after = body
        .split_once("## Your gate")
        .expect("the brief carries the gate section")
        .1;
    let section = after
        .split_once("\n## ")
        .map(|(head, _)| head)
        .unwrap_or(after);
    section
        .split_once("The project's whole suite is")
        .expect("the gate section names the reviewer's half")
}

#[test]
fn a_dispatch_with_no_seat_yet_says_so_where_the_seat_goes() {
    let rig = Rig::new("transient");
    let rendered = rig.render(&store_with(Some(ORDER)), TRANSIENT);
    assert!(rendered.body.contains(TRANSIENT), "{}", rendered.body);
}

#[test]
fn an_unknown_placeholder_exits_three_and_writes_nothing() {
    let rig = Rig::new("unknown");
    rig.fixture
        .file("defaults/assets/brief.md", "you are {nobody}\n");
    let rig = Rig::over(rig.fixture);

    let rendered = rig.render(&store_with(Some(ORDER)), "s1");
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

    let rendered = rig.render(&store_with(Some(ORDER)), "s1");
    assert_eq!(rendered.code, Some(3), "{}", rendered.why);
    assert!(rendered.why.contains("assets/rules.md"), "{}", rendered.why);
    assert_eq!(rendered.body.len(), 0, "nothing reaches stdout");
}

#[test]
fn an_item_with_no_order_note_is_refused_and_writes_nothing() {
    let rig = Rig::new("unordered");
    let rendered = rig.render(&store_with(None), "s1");
    assert_eq!(rendered.code, Some(1), "{}", rendered.why);
    assert!(rendered.why.contains("no order note"), "{}", rendered.why);
    assert_eq!(rendered.body.len(), 0, "nothing reaches stdout");

    // The control: the same store carrying notes with no recorded form in them
    // is refused too, so the gate is the FORM and not the field's presence.
    let rendered = rig.render(&store_with(Some("a note about something else")), "s1");
    assert_eq!(rendered.code, Some(1), "{}", rendered.why);
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

    let rendered = rig.render(&store_with(Some(ORDER)), "s1");
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

/// The fidelity control on every arm above: the readings the fake answers are
/// the readings `bd` answers, measured by rendering the same brief through both
/// and comparing the bytes.
#[test]
fn the_real_store_renders_the_same_brief_as_the_one_held_in_memory() {
    let scratch = shared_store("brief");
    let item = scratch.item("a ready item");
    let out = scratch.bd(&["note", &item, ORDER, "--actor", "architect-1"]);
    assert!(out.status.success(), "the order note is written");

    let rig = Rig::new("fidelity");
    let real = Bd::at(&scratch.root);
    let record = real.show(&item).expect("bd answers about the item");
    let text = real.show_text(&item).expect("bd renders the item");

    let fake = FakeStore::default();
    fake.seed(record.clone());
    fake.set_text(&item, &text);

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
            "s1",
        )
        .expect("the brief renders");
        String::from_utf8(out).expect("utf-8")
    };

    assert_eq!(through(&real), through(&fake), "byte for byte");
    assert_eq!(record.status, "open");
    assert_eq!(
        brief::order_line(record.notes.as_deref()).as_deref(),
        Some(ORDER),
        "the order note bd stored is the one that was written"
    );
}

/// The store's human rendering sometimes carries a tip line that NAMES A
/// PROVIDER, which a pack template may not do — and which would make one item
/// render two ways depending on whether anybody had looked at it before.
///
/// The tip is not asserted here, in either direction. Its budget is not the
/// store's: three fresh stores measured on this box tipped on the second and
/// third and not the first, and a run under a private HOME did not tip at all.
/// An arm that demanded it would be red on somebody else's morning. What IS
/// deterministic is measured instead — the quiet read is stable, and the
/// template passes the rendering through verbatim, which is the reason the
/// quiet flag has to be on the read rather than on a filter afterwards.
#[test]
fn the_item_rendering_is_stable_and_a_tip_in_it_would_reach_the_seat() {
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

    // The control, observed failing: a store whose rendering DOES carry the tip
    // puts it in the seat's first turn, because `{item}` is verbatim. That is
    // the hazard the quiet flag removes at the read.
    let tipped = store_with(Some(ORDER));
    tipped.set_text(
        ITEM,
        &format!("{first}\n💡 Tip: run 'bd setup claude' for CLI-only mode\n"),
    );
    let rig = Rig::new("tip-render");
    let rendered = rig.render(&tipped, "s1");
    assert!(
        rendered.body.contains("bd setup claude"),
        "a tip in the rendering reaches the brief:\n{}",
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
        let rendered = rig.render(&store_with(Some(ORDER)), "s1");

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
        let store = store_with(Some(ORDER));
        store.set_text(
            ITEM,
            "fx-1 · an item with a very much longer rendering than the one above\n",
        );
        let longer = rig.render(&store, "s1");
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
