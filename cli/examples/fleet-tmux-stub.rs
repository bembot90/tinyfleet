//! `fleet-tmux-stub` again, built by this crate's own test build, for the
//! reason `fleet-store-stub` is: a test build of one crate builds no other
//! crate's executables, so the controller's copy is beside `fleet` only where
//! an earlier controller build left it — or a stale one. The source is the
//! controller's own file, included rather than copied, and it reaches the fake
//! server through this crate's dev-dependency on fleet-controller with
//! `test-support` on. `tests/common`'s `stub_tmux` runs it.

#[path = "../../controller/src/bin/tmux_stub.rs"]
mod tmux_stub;

fn main() -> std::process::ExitCode {
    tmux_stub::main()
}
