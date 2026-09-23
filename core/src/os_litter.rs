//! The files an operating system's file browser writes into every directory it
//! opens: Finder's `.DS_Store`, and Explorer's thumbnail cache and folder
//! settings.
//!
//! None is ever a pack's own or a default's, and none is under anybody's
//! control once the folder has been looked at, so every walk that decides what
//! a tree HOLDS reads past them: the pack check and the resolver, the defaults'
//! tree hash, and the build's walk of `defaults/` into the executable. The last
//! is why this is a file of its own with no `use` in it — `build.rs` compiles it
//! by path, so the build and the crate read one list and cannot drift.
//!
//! Exact names and nothing broader: a name that merely resembles one of these
//! is still read, and still refused where a format has no place for it.

pub const OS_LITTER: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];

pub fn is_os_litter(name: &str) -> bool {
    OS_LITTER.contains(&name)
}
