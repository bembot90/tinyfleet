//! What the cli rigs share: which dolt engine their scratch boards run on, and
//! how each of them comes by a board — or by the store stub in its place.
//!
//! The rule lives once, in the core crate's test module, and is included here
//! rather than copied — the port this refuses is the fleet's live board, and a
//! second copy is a second thing to remember to change.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../../../core/tests/common/board.rs"]
mod board;

// The env block is the controller crate's, included rather than copied: the
// roots it names are that crate's resolution order, and a second copy of the
// list is a second thing to remember to change.
#[path = "../../../controller/tests/common/hermetic.rs"]
pub mod hermetic;

pub use board::{bd_init_server_args, run_board};

/// One line for one `bd init` a rig does not run itself — `fleet store
/// check`'s, the bd adapter's own scratch — counted where the run reads its
/// count. A function and not a re-export, because every rig includes this
/// module and a re-export only one of them reads is an unused import in the
/// rest.
pub fn note_bd_init(label: &str) {
    board::note_bd_init(label);
}

/// The store stub this crate's test build made: its example
/// `fleet-store-stub`, in `examples/` beside the `fleet` it builds. Core's own
/// `fleet-store-stub` is not built by a test build of this crate, so the one
/// beside `fleet` is whatever an earlier core build left.
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

/// The project at `root` kept on the stub: `[store] adapter` naming it
/// appended to the project's own file — `.fleet/project.toml` where there is
/// one, else `fleet.toml`, made where it is not — and an empty store
/// scratched at the root, through the stub itself. Answers the stub's path.
///
/// The file must not already carry a `[store]` table: a second one does not
/// parse.
pub fn stub_store(root: &Path) -> PathBuf {
    use fleet_core::store::Store as _;
    let stub = stub_path();
    let file = [root.join(".fleet/project.toml"), root.join("fleet.toml")]
        .into_iter()
        .find(|file| file.is_file())
        .unwrap_or_else(|| root.join("fleet.toml"));
    let mut policy = std::fs::read_to_string(&file).unwrap_or_default();
    let named = serde_json::to_string(&stub.display().to_string()).expect("a path is JSON text");
    policy.push_str(&format!("\n[store]\nadapter = {named}\n"));
    std::fs::write(&file, policy).expect("the project's file names the stub");
    let made = fleet_core::store::exec::Exec::at(&stub, root)
        .scratch(root)
        .unwrap_or_else(|e| panic!("the stub makes a store in {}: {e}", root.display()));
    assert_eq!(made, root, "the stub's store is the project's root");
    stub
}

/// A work graph at `root`: the run's board copied in, or a `bd init` of this
/// rig's own where no run made one.
///
/// A rig whose subject is a WHOLE-BOARD reading cannot take the run's shared
/// board — the size of the ready set, the open runs `[core.run] max_open` is
/// measured against — because a neighbour's rows move the answer it asserts
/// on. Such a rig runs its own `bd init` and says so where it does.
/// `fleet/core/tests/solo.rs` catches the half of that class an arm reads for
/// itself; the half that lives inside the verb the rig drives is nobody's to
/// catch but the reader's.
///
/// ONE CLI RIG KEEPS A BOARD OF ITS OWN: `land.rs` already initialises one
/// board for the whole run and copies it per arm, so there is nothing left for
/// this to save.
///
/// ONE MORE IS ONE ARM AND NOT A WHOLE RIG: `run.rs` measures `[core.run]
/// max_open` against every open run on the board, so the arm that does it runs
/// its own `bd init` while every other arm in that file shares.
///
/// A SEAT NAME IS BOARD-WIDE. `held_item` and the dispatch refusal both ask
/// "which open item does this seat hold", across everything on the board, so a
/// rig here names its seats under a prefix no other crate's rigs use.
pub fn take_a_board(root: &Path, label: &str) {
    take_a_board_with(Path::new("bd"), root, label);
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

/// A board of this rig's OWN, whatever the run made: an init every time.
///
/// For a rig whose arms make a BOARD-WIDE WRITE keyed on a seat. `fleet seat
/// retire` withdraws every open ordered item the retiring seat holds, across
/// the whole board, so two arms that retired one seat name on one board would
/// have one's retire clear the order the other's dispatch had just written. A
/// spawn mints its seat fresh, so the arms in `seat.rs` can no longer meet on a
/// name; what a board of their own still buys them is one no neighbour's rows
/// reach, and moving them onto the run's shared board is a change of its own.
pub fn take_a_board_alone(root: &Path, label: &str) {
    bd_init(Path::new("bd"), root, label);
}

/// The same, for a rig that resolved `bd` on its own search path rather than
/// leaving it to the process PATH.
pub fn take_a_board_with(bd: &Path, root: &Path, label: &str) {
    if let Some(made) = run_board(label) {
        copy_board(&made, root);
        return;
    }
    bd_init(bd, root, label);
}

/// One `bd init` under this rig's root, on the run's server where there is one.
fn bd_init(bd: &Path, root: &Path, label: &str) {
    let out = Command::new(bd)
        .args(["init", "--prefix", "fx", "--quiet"])
        .args(bd_init_server_args(label))
        .current_dir(root)
        .output()
        .expect("bd is on the process PATH");
    assert!(
        out.status.success(),
        "bd init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The board's own tree, copied in — EXCEPT a `.git` where the rig already has
/// one. `bd init` makes a repository beside the store, so a rig that made its
/// own first would otherwise have its refs and its index written over.
fn copy_board(from: &Path, to: &Path) {
    let keeps_its_own_repository = to.join(".git").exists();
    for entry in std::fs::read_dir(from).expect("the run's board is readable") {
        let entry = entry.expect("an entry is readable");
        if keeps_its_own_repository && entry.file_name() == ".git" {
            continue;
        }
        copy_tree(&entry.path(), &to.join(entry.file_name()));
    }
}

fn copy_tree(from: &Path, to: &Path) {
    if from.is_dir() {
        std::fs::create_dir_all(to).expect("the destination directory is created");
        for entry in std::fs::read_dir(from).expect("the source directory is readable") {
            let entry = entry.expect("an entry is readable");
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        std::fs::copy(from, to).expect("the file is copied");
    }
}
