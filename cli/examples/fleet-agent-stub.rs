//! `fleet-agent-stub` again, built by this crate's own test build, for the
//! reason `fleet-store-stub` is: a test build of one crate builds no other
//! crate's executables, so the controller's copy is beside `fleet` only where
//! an earlier controller build left it — or a stale one. It reaches the fake
//! through this crate's dev-dependency on fleet-controller with `test-support`
//! on. `tests/common`'s `stub_agent` names it.

fn main() -> std::process::ExitCode {
    fleet_controller::test_support::agent_stub::main()
}
