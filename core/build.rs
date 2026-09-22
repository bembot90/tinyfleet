//! The crate's `defaults/` tree, walked at build time into the table
//! `src/embedded.rs` includes.
//!
//! STD ONLY, by the epic's dependency rule: no walker crate, no codegen crate.
//! The generated file is a literal slice of [`File`] rows in path order, each
//! carrying an `include_bytes!` of the source and the executable bit the SOURCE
//! FILE holds in the working tree — a mode `include_bytes!` cannot carry. Every
//! default is 0644 today, so the column reads false throughout; it is there so
//! a default that becomes executable survives the embed.

use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("cargo names the manifest directory"),
    )
    .join("defaults");

    let mut files = Vec::new();
    walk(&root, &root, &mut files).expect("the defaults tree is readable");
    files.sort();

    // The directory itself, so a file added or dropped rebuilds; each file, so
    // an edit does.
    println!("cargo:rerun-if-changed=defaults");
    println!("cargo:rerun-if-changed=build.rs");

    let mut out = String::from("pub static FILES: &[File] = &[\n");
    for relative in &files {
        let source = root.join(relative);
        println!("cargo:rerun-if-changed=defaults/{relative}");
        out.push_str(&format!(
            "    File {{ path: {:?}, bytes: include_bytes!({:?}), executable: {} }},\n",
            relative,
            source,
            is_executable(&source)
        ));
    }
    out.push_str("];\n");

    let generated = PathBuf::from(std::env::var("OUT_DIR").expect("cargo names the out directory"))
        .join("embedded.rs");
    std::fs::write(&generated, out).expect("the embedded table is written");
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("the walk stays under the root")
                .to_str()
                .expect("a default's name is UTF-8")
                .to_string();
            out.push(relative);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}
