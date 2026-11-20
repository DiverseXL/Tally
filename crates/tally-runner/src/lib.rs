//! The runner: profiles, orchestration, the run guard and the two report
//! renderers.
//!
//! One [`RunReport`](tally_core::report::RunReport) value feeds both the JSON
//! and the HTML output, so the two can never disagree.

pub mod config;
pub mod guard;
pub mod profile;
pub mod render;
pub mod run;
pub mod summary;
pub mod wallet;

#[cfg(feature = "test-support")]
pub mod test_support;

pub use config::{
    DEFAULT_AMOUNT_MSAT, DEFAULT_INVOICE_EXPIRY_SECONDS, DEFAULT_WALLET_REQUIRED, RunConfig,
};
pub use profile::{Profile, TestSpec};
pub use render::{STATUSES, WrittenReports, escape, to_html, to_json, write_report_files};
pub use run::{
    FIXTURE_LIMITATION, NoopObserver, RunError, RunOutcome, SANDBOX_LIMITATION, StepEvent,
    StepObserver, StepPhase, TALLY_VERSION, run, run_with_observer,
};
pub use summary::{reason_of, summary_lines};
pub use wallet::{
    ADVERTISED_ONLY, NO_PAYMENT_LIMITATION, run_wallet, run_wallet_with_observer, sanitize_label,
};
