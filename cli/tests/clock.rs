//! The controller's fake clock, reached from THIS crate's suite.
//!
//! A test binary of its own, and the arm's subject is the wiring rather than the
//! clock: `fleet-controller`'s `test_support` module is behind a feature that
//! only this crate's dev-dependency turns on, so an arm that compiles here is
//! the proof the in-process loop arms can be driven under fake time.

use fleet_controller::clock::Clock;
use fleet_controller::test_support::FakeClock;
use std::time::Duration;

#[test]
fn the_controllers_fake_clock_is_reachable_from_this_crates_tests() {
    let clock = FakeClock::new();
    let start = clock.now();
    clock.sleep(Duration::from_secs(90));
    assert_eq!(clock.now() - start, Duration::from_secs(90));
}
