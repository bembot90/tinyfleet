//! `fleet-store-stub` again, built by this crate's own test build: a test
//! build of one crate builds no other crate's executables, so the copy core
//! builds is only beside `fleet` where an earlier core build left it — or a
//! stale one. An example is built by every `cargo test` of this crate, from
//! this crate's dev-dependency on fleet-core with `test-support` on, and is
//! what `tests/common`'s `stub_path` runs.

fn main() -> std::process::ExitCode {
    fleet_core::test_support::stub::main()
}
