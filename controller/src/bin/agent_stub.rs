//! `fleet-agent-stub`: the agent contract answered from the fake, as an
//! adapter executable — [`fleet_controller::test_support::agent_stub`], which
//! holds all of it. Built only under `test-support`, so no release build
//! carries it.

fn main() -> std::process::ExitCode {
    fleet_controller::test_support::agent_stub::main()
}
