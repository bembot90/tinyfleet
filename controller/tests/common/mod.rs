//! What the controller rigs share.
//!
//! One thing so far: the env block that keeps an arm off the running machine
//! directory and the real agent binary. It lives in `hermetic.rs` beside this
//! file and is included from the cli crate's `tests/common` too, so the four
//! roots are one list for both suites.
#![allow(dead_code)]

pub mod hermetic;
