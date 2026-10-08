//! The resolver's empty entry, in a process of its own: the arm moves this
//! process's working directory, which no other arm may share.

use std::os::unix::fs::PermissionsExt;

use fleet_core::process::resolve_on_path;

/// An empty entry is the current directory on a shell's search path, and the
/// resolver never searches it.
///
/// The program is planted in the working directory, so a resolver that read
/// an empty entry as `.` would find it there. Both ends are read: an empty
/// entry leading the path and one trailing it.
#[test]
fn an_empty_entry_on_the_path_never_resolves_the_working_directory() {
    let dir = std::env::temp_dir().join(format!("fleet-resolve-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let planted = dir.join("fx-here");
    std::fs::write(&planted, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    assert_eq!(
        resolve_on_path(":/nowhere", "fx-here"),
        None,
        "an empty entry leading the path was searched as the working directory"
    );
    assert_eq!(
        resolve_on_path("/nowhere:", "fx-here"),
        None,
        "an empty entry trailing the path was searched as the working directory"
    );

    // The control: the same program on a path that names its directory
    // resolves, so the two `None`s above are the empty entry's and not a
    // planted file the resolver could not see.
    assert_eq!(
        resolve_on_path(&format!("/nowhere:{}", dir.display()), "fx-here"),
        Some(planted)
    );

    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
