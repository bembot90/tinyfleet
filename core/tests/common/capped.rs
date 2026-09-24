//! A `bd` that caps a listing the way the real one does, for the reads whose
//! whole argument is that they lift the cap.
//!
//! `bd list` answers its first 50 rows unless `-n 0` is passed — bd 1.3.0's own
//! help: "-n, --limit int … (default 50, use 0 for unlimited)", which binds a
//! terminal and, measured, a piped call only where the board sets `list.limit`
//! — and a truncated list reads exactly like a whole one. A real board holding 51 rows
//! against one seat costs a `create` per row to build; this answers the same
//! listing off one file per row, so the row past the cap is cheap to hold.

use std::path::{Path, PathBuf};

use super::Fixture;

/// One row the fake holds against the seat, filed under `id` and ordered or
/// not.
pub struct Held<'a> {
    pub id: &'a str,
    pub ordered: bool,
}

/// Writes the fake under `dir` holding `rows` against `seat`, and answers its
/// absolute path. Every call it is handed goes on one line of `log`, its
/// arguments tab-separated.
///
/// It answers the four verbs a retire makes and no other:
/// - `list` every row it holds, in id order, capped at 50 unless `-n` names
///   another limit — the seat filter is not applied, because every row it holds
///   is that seat's;
/// - `show <id>` that row;
/// - `update <id>` only as a withdrawal fenced on `seat` and the row's open
///   status (`--if-assignee <seat> --if-status open`, then `--assignee ''`
///   with `--unset-metadata fleet.orders` and `--status open`), which leaves
///   the row open, unassigned and unordered;
/// - `note` as a write it accepts and does not keep.
pub fn capped_bd(dir: &Fixture, seat: &str, rows: &[Held], log: &Path) -> PathBuf {
    let items = dir.path("items");
    std::fs::create_dir_all(&items).expect("the fake's rows directory is created");
    for row in rows {
        let orders = if row.ordered {
            format!(
                ",\"metadata\":{{\"fleet.orders\":{{\"v\":1,\"by\":\"an-architect\",\"kind\":\"dispatch\",\
                 \"seat\":\"{seat}\",\"at\":\"2026-09-14T10:40:39Z\"}}}}"
            )
        } else {
            String::new()
        };
        std::fs::write(
            items.join(format!("{}.json", row.id)),
            format!(
                "{{\"id\":\"{id}\",\"status\":\"open\",\"assignee\":\"{seat}\"{orders}}}",
                id = row.id
            ),
        )
        .expect("the fake's row is written");
    }

    let bin = dir.path("bd");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n\
             ( IFS='\t'; printf '%s\\n' \"$*\" ) >> '{log}'\n\
             shift 2\n\
             items='{items}'\n\
             case \"$1\" in\n\
             list)\n\
             \x20 cap=50; prev=\n\
             \x20 for a in \"$@\"; do [ \"$prev\" = -n ] && cap=$a; prev=$a; done\n\
             \x20 n=0; sep=\n\
             \x20 printf '['\n\
             \x20 for f in $(LC_ALL=C ls \"$items\"); do\n\
             \x20   [ \"$cap\" -ne 0 ] && [ \"$n\" -ge \"$cap\" ] && break\n\
             \x20   printf '%s' \"$sep\"; cat \"$items/$f\"; sep=,\n\
             \x20   n=$((n + 1))\n\
             \x20 done\n\
             \x20 printf ']\\n' ;;\n\
             show) printf '['; cat \"$items/$2.json\"; printf ']\\n' ;;\n\
             update)\n\
             \x20 case \" $* \" in *' --if-assignee {seat} --if-status open --assignee  --unset-metadata {orders_key} --status open '*) ;; *) exit 1 ;; esac\n\
             \x20 printf '{{\"id\":\"%s\",\"status\":\"open\"}}' \"$2\" > \"$items/$2.json\" ;;\n\
             note) ;;\n\
             *) echo 'the fake answers list, show, update and note only' >&2; exit 1 ;;\n\
             esac\n",
            log = log.display(),
            items = items.display(),
            orders_key = fleet_core::store::keys::ORDERS,
        ),
    )
    .expect("the fake is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the fake is executable");
    bin
}

/// The calls the fake was handed, one per entry, each split back into its
/// arguments.
pub fn calls(log: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(log)
        .expect("the fake recorded its calls")
        .lines()
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect()
}
