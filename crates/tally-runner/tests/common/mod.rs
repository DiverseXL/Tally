//! TEST ONLY: the fake wallets used by the runner and server tests.
//!
//! TEST ONLY: the implementation lives in `tally_runner::test_support`, behind
//! the off-by-default `test-support` feature. This module only re-exports it so
//! both integration test files share one import path.

#[cfg(feature = "test-support")]
pub use tally_runner::test_support::*;

// A package cannot enable its own cargo feature for its tests, so a standalone
// `cargo test -p tally-runner` compiles the same TEST ONLY file straight into
// the test binary instead. Both paths are the one implementation.
#[cfg(not(feature = "test-support"))]
#[path = "../../src/test_support.rs"]
mod inline;

#[cfg(not(feature = "test-support"))]
pub use inline::*;
