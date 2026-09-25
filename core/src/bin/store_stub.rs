//! `fleet-store-stub`: the store contract answered from the fake, as an
//! adapter executable — [`fleet_core::test_support::stub`], which holds all of
//! it. Built only under `test-support`, so no release build carries it.

fn main() -> std::process::ExitCode {
    fleet_core::test_support::stub::main()
}
