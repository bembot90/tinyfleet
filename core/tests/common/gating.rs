//! A `bd` whose `gate create` writes the gate and then exits 1, for the park
//! that has to find what a failed create left behind.
//!
//! bd 1.2.2 did exactly this when the item the gate blocks is an epic: the
//! gate was filed, the blocking edge refused, and the call exited 1 — so an
//! open gate that blocks nothing stood on the list with nobody's park naming
//! it. bd 1.3.0 files the gate AND the edge on an epic and exits 0, measured,
//! so a real board at the pin produces it on demand for no item at all, and
//! this answers the same calls off one file per gate.

use std::path::{Path, PathBuf};

use super::Fixture;

/// The id the failing `gate create` files its gate under.
pub const LEFT_BEHIND: &str = "g-left-behind";

/// Writes the fake under `dir` and answers its absolute path. Every call it is
/// handed goes on one line of `log`, its arguments tab-separated, which is the
/// shape [`super::capped::calls`] reads back.
///
/// It answers the calls a park makes up to its gate and no other:
/// - `show <id>` the one row `item` holds, whatever the id;
/// - `gate list` every gate it holds, in id order — `open` at the start, plus
///   whatever a create has filed since and a resolve has not removed;
/// - `gate create` files [`LEFT_BEHIND`] and then exits 1;
/// - `gate resolve <id>` removes that gate, or exits 1 and keeps it when
///   `resolves` is false.
pub fn gating_bd(dir: &Fixture, item: &str, open: &[&str], resolves: bool, log: &Path) -> PathBuf {
    let gates = dir.path("gates");
    std::fs::create_dir_all(&gates).expect("the fake's gates directory is created");
    for id in open {
        write_gate(&gates, id);
    }
    let row = dir.path("row.json");
    std::fs::write(&row, item).expect("the fake's row is written");
    let resolve = if resolves {
        "rm \"$gates/$3.json\""
    } else {
        "echo 'Error: the fake refuses to resolve' >&2; exit 1"
    };

    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             ( IFS='\t'; printf '%s\\n' \"$*\" ) >> '{log}'\n\
             shift 2\n\
             gates='{gates}'\n\
             case \"$1 $2\" in\n\
             'gate list')\n\
             \x20 sep=\n\
             \x20 printf '['\n\
             \x20 for f in $(LC_ALL=C ls \"$gates\"); do\n\
             \x20   printf '%s' \"$sep\"; cat \"$gates/$f\"; sep=,\n\
             \x20 done\n\
             \x20 printf ']\\n' ;;\n\
             'gate create')\n\
             \x20 printf '{{\"id\":\"{LEFT_BEHIND}\",\"status\":\"open\",\"issue_type\":\"gate\"}}' \
             > \"$gates/{LEFT_BEHIND}.json\"\n\
             \x20 echo 'Error: failed to add the blocking dependency' >&2\n\
             \x20 exit 1 ;;\n\
             'gate resolve') {resolve} ;;\n\
             show\\ *) printf '['; cat '{row}'; printf ']\\n' ;;\n\
             *) echo 'the fake answers show and gate list, create and resolve only' >&2; exit 1 ;;\n\
             esac\n",
            log = log.display(),
            gates = gates.display(),
            row = row.display(),
        ),
    )
    .expect("the fake is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the fake is executable");
    bin
}

/// The gates the fake still holds open, in id order.
pub fn standing(dir: &Fixture) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir.path("gates"))
        .expect("the fake's gates directory reads")
        .map(|entry| {
            entry
                .expect("the entry reads")
                .path()
                .file_stem()
                .expect("a gate file has a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    ids.sort();
    ids
}

fn write_gate(gates: &Path, id: &str) {
    std::fs::write(
        gates.join(format!("{id}.json")),
        format!("{{\"id\":\"{id}\",\"status\":\"open\",\"issue_type\":\"gate\"}}"),
    )
    .expect("the fake's gate is written");
}
