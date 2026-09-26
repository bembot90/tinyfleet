//! What the cli rigs share: the store every rig keeps its project on — the
//! store stub, opened through the adapter seam any other adapter is opened by
//! — and the env block the shipped binary runs under.
//!
//! NO RIG HERE NEEDS A STORE INSTALLED. A real store's behaviour is its pack's
//! to test, in the repository the pack is published from.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use fleet_core::seat::actor::Actor;
use fleet_core::seat::identity::SeatId;
use fleet_core::store::exec::Exec;
use fleet_core::store::{ItemId, NewItem, Order, OrderKind, Stamp, Store as _, Update};
use fleet_core::test_support::FakeStore;

// The env block is the controller crate's, included rather than copied: the
// roots it names are that crate's resolution order, and a second copy of the
// list is a second thing to remember to change.
#[path = "../../../controller/tests/common/hermetic.rs"]
pub mod hermetic;

/// The actor a rig's own setup writes under: a run, because the setup is no
/// seat's act.
pub fn the_test() -> Actor {
    fleet_core::test_support::the_test()
}

/// `act` on the stub's store at `root`, under its lock, and the store written
/// back after it: the way in for a rig putting the store in a state no verb of
/// the contract reaches, or reading what no verb answers. Functions and not
/// re-exports, because every rig includes this module and a re-export only
/// some of them read is an unused import in the rest.
pub fn with_state<T>(root: &Path, act: impl FnOnce(&FakeStore) -> T) -> T {
    fleet_core::test_support::stub::with_state(root, act)
        .unwrap_or_else(|why| panic!("the stub's store at {}: {why}", root.display()))
}

/// The store stub this crate's test build made: its example
/// `fleet-store-stub`, in `examples/` beside the `fleet` it builds. Core's own
/// `fleet-store-stub` is not built by a test build of this crate, so the one
/// beside `fleet` is whatever an earlier core build left.
///
/// ABSOLUTE, as `[store] adapter` must name it: a verb whose child `PATH` is
/// one it constructed still opens this file and no other.
pub fn stub_path() -> PathBuf {
    let stub = Path::new(env!("CARGO_BIN_EXE_fleet"))
        .parent()
        .expect("the built binary sits in a directory")
        .join("examples/fleet-store-stub");
    assert!(
        stub.is_file(),
        "{} is built by a test build of fleet-cli — `cargo nextest run -p fleet-cli`, or \
         `cargo build -p fleet-cli --examples`",
        stub.display()
    );
    stub
}

/// A tmux for a rig's `fleet` to run: the path to set `FLEET_TMUX_BIN` to,
/// after the hermetic block, whose own value is the refusing tmux.
///
/// It is a SYMLINK at `<dir>/tmux` to this crate's example `fleet-tmux-stub`
/// (beside `fleet`, in `examples/`, for the reason [`stub_path`]'s store stub
/// is there), and the stub keeps its state beside the link it was run through,
/// at `<dir>/tmux-stub.json`. Named by where the link sits because the host
/// clears its clients' environment, so a variable put on `fleet` never reaches
/// the stub; a link and not a wrapper script, because macOS spends 15 s or more
/// assessing a newly written script's first exec. The rig reads the fake
/// server back with `fleet_controller::test_support::FakeServer::load` on that
/// file, and ends a session's pane with `<link> end <name> <status>`.
pub fn stub_tmux(dir: &Path) -> PathBuf {
    let stub = Path::new(env!("CARGO_BIN_EXE_fleet"))
        .parent()
        .expect("the built binary sits in a directory")
        .join("examples/fleet-tmux-stub");
    assert!(
        stub.is_file(),
        "{} is built by a test build of fleet-cli — `cargo nextest run -p fleet-cli`, or \
         `cargo build -p fleet-cli --examples`",
        stub.display()
    );
    std::fs::create_dir_all(dir).expect("the tmux stub's directory is made");
    let link = dir.join("tmux");
    std::os::unix::fs::symlink(&stub, &link).expect("the link to the tmux stub is made");
    link
}

/// The project at `root` kept on the stub: `[store] adapter` naming it
/// appended to the project's own file — `.fleet/project.toml` where there is
/// one, else `fleet.toml`, made where it is not — and an empty store
/// scratched at the root, through the stub itself. Answers the root.
///
/// THE POLICY COMES FIRST. The file must not already carry a `[store]`
/// table, since a second one does not parse, and a rig that writes the file
/// whole after this call writes the setting away with it — the project then
/// names no store, and a verb opens the default name through the packs.
///
/// A STORE IS ONE ROOT'S. The stub keeps it under `<root>/.store/`, and a
/// verb run in a linked worktree resolves that checkout as its root: a rig
/// whose verbs run there shares the primary's with [`share_store`].
pub fn take_a_store(root: &Path) -> PathBuf {
    let stub = stub_path();
    let file = [root.join(".fleet/project.toml"), root.join("fleet.toml")]
        .into_iter()
        .find(|file| file.is_file())
        .unwrap_or_else(|| root.join("fleet.toml"));
    let mut policy = std::fs::read_to_string(&file).unwrap_or_default();
    let named = serde_json::to_string(&stub.display().to_string()).expect("a path is JSON text");
    policy.push_str(&format!("\n[store]\nadapter = {named}\n"));
    std::fs::write(&file, policy).expect("the project's file names the stub");
    let made = Exec::at(&stub, root)
        .scratch(root)
        .unwrap_or_else(|e| panic!("the stub makes a store in {}: {e}", root.display()));
    assert_eq!(made, root, "the stub's store is the project's root");
    root.to_path_buf()
}

/// The store the stub keeps at `root`, for a rig's own setup and its own
/// reading: an item filed, handed over or closed by the rig and not by the
/// verb under test, written as [`the_test`] wherever the rig names no actor.
pub fn store_at(root: &Path) -> Exec {
    Exec::at(&stub_path(), root)
}

/// One open task filed at `root` under `labels`, answered as its full id.
pub fn filed(root: &Path, title: &str, labels: &[&str]) -> String {
    store_at(root)
        .create(
            &NewItem {
                title: title.to_string(),
                description: String::from("a scratch item"),
                item_type: String::from("task"),
                labels: labels.iter().map(|label| label.to_string()).collect(),
                priority: None,
            },
            &the_test(),
        )
        .unwrap_or_else(|e| panic!("the stub files `{title}` at {}: {e}", root.display()))
        .to_string()
}

/// `item` handed to the seat whose full id is `seat`.
pub fn hand_to(root: &Path, item: &str, seat: &str) {
    let seat = SeatId::parse(seat).unwrap_or_else(|why| panic!("{why}"));
    store_at(root)
        .update(&ItemId::from(item), &Update::assignee(seat), &the_test())
        .unwrap_or_else(|e| panic!("{item} is handed to {seat}: {e}"));
}

/// The order index a dispatch writes, set on `item` by the rig: `kind`, given
/// by the typed actor `by`, to the seat whose full id is `seat` — or to none
/// yet — at a fixed instant.
pub fn ordered(root: &Path, item: &str, kind: OrderKind, by: &str, seat: Option<&str>) {
    let order = Order {
        kind,
        by: Actor::typed(by)
            .and_then(Result::ok)
            .unwrap_or_else(|| panic!("`{by}` is a typed actor")),
        seat: seat.map(|seat| SeatId::parse(seat).unwrap_or_else(|why| panic!("{why}"))),
        at: Stamp::parse("2026-09-09T00:00:00Z").expect("a stamp"),
    };
    store_at(root)
        .order_set(&ItemId::from(item), &order, &the_test())
        .unwrap_or_else(|e| panic!("{item} is ordered: {e}"));
}

/// `item` as the store answers it, in the contract's own JSON: its `assignee`
/// a seat's full id or absent, its `order` and its `run` the contract's
/// shapes.
pub fn shown(root: &Path, item: &str) -> serde_json::Value {
    let item = store_at(root)
        .show(item)
        .unwrap_or_else(|e| panic!("{item} reads at {}: {e}", root.display()));
    serde_json::to_value(item).expect("an item is JSON")
}

/// The order index `shown` carries, as the contract spells an order — its
/// `kind`, `by`, `seat` and `at` — or `null` where it carries none. An index
/// the store could not read is answered as the store put it.
pub fn order_in(shown: &serde_json::Value) -> serde_json::Value {
    match shown["order"]["state"].as_str() {
        Some("ordered") => shown["order"]["order"].clone(),
        Some("none") => serde_json::Value::Null,
        _ => shown["order"].clone(),
    }
}

/// `item` closed by the rig, done.
pub fn closed(root: &Path, item: &str) {
    store_at(root)
        .close(&ItemId::from(item), "done", &the_test())
        .unwrap_or_else(|e| panic!("{item} is closed: {e}"));
}

/// The linked worktree at `checkout` reading and writing the store of the
/// primary at `primary`, as a verb run there needs: its `.store` is a link to
/// the primary's, and [`store_outside_git`] keeps the link out of its status.
pub fn share_store(primary: &Path, checkout: &Path) {
    std::os::unix::fs::symlink(primary.join(".store"), checkout.join(".store"))
        .expect("the checkout's store links to the primary's");
    store_outside_git(primary);
}

/// `.store` kept out of the status of the repository at `repo` and of every
/// worktree it has, as a project keeps a database it does not version: a
/// line in `info/exclude`, which the COMMON DIR holds and every linked
/// worktree reads.
pub fn store_outside_git(repo: &Path) {
    let exclude = repo.join(".git/info/exclude");
    let mut lines = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !lines.lines().any(|line| line == ".store") {
        lines.push_str("\n.store\n");
        std::fs::create_dir_all(exclude.parent().expect("info/ sits in the git dir"))
            .expect("the git dir's info/ is made");
        std::fs::write(&exclude, lines).expect("the exclude file is written");
    }
}

/// The full id a rig's seat is keyed by, derived from its name alone: FNV-1a
/// over the name, in the node's twelve digits. A seat's items are assigned to
/// its id, so a rig that names its seats under its own prefix gets ids no
/// other rig's seats share.
pub fn seat_id_of(name: &str) -> String {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("01a0d1f1-0aec-765f-9abe-{:012x}", hash & 0xffff_ffff_ffff)
}

/// The `[seats.<id>]` table that lists an agent seat by that id and name.
pub fn seat_table_of(name: &str) -> String {
    format!(
        "\n[seats.{}]\nkind = \"agent\"\nname = \"{name}\"\n",
        seat_id_of(name)
    )
}
