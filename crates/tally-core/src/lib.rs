//! Core types shared by the Tally crates.
//!
//! This crate is pure: no network access and no wallet SDK types. It holds the
//! five test statuses, the single status mapping, the capability model, the run
//! report with its exit codes, secret redaction and small time helpers.

pub mod capabilities;
pub mod outcome;
pub mod redact;
pub mod report;
pub mod status;
pub mod time;

pub use capabilities::{
    Advertisement, Capabilities, CapabilitySource, ContentKind, Discrepancy, EffectiveCapabilities,
    GetInfoCapabilities, InfoEventCapabilities,
};
pub use outcome::{Observation, Outcome, WalletErrorCode, classify_wallet_outcome, reason};
pub use report::{
    ChallengeEvidence, EndpointClass, EnvClass, Environment, InvoiceEvidence, NwcClientIdentity,
    PaymentEvidence, PaymentOutcome, ProtectedResourceEvidence, RoleCapabilities, RunReport,
    SCHEMA_VERSION, TestResult, WalletRole, WalletSummary, WorkflowEvidence, exit_code,
};
pub use status::Status;
pub use time::{new_run_id, now_rfc3339, rfc3339_utc_from_unix_secs, unix_now_millis};
