//! TEST ONLY: re-export of the feature-gated offline invoice helpers.
//!
//! TEST ONLY: the code lives in `tally_l402::test_support`, compiled only when
//! the off-by-default `test-support` feature is on. This module only makes it
//! reachable from this crate's integration tests.

pub use tally_l402::test_support::TestInvoiceSource;
