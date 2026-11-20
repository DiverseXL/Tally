//! The single mapping from a wallet observation to a status and reason code.
//!
//! The table implemented here is the only place where a status is decided; both
//! the wallet adapter and the report read it from here.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::status::Status;

/// Wallet error codes defined by NIP-47.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WalletErrorCode {
    /// The client is sending commands too fast.
    #[serde(rename = "RATE_LIMITED")]
    RateLimited,
    /// The command is unknown or intentionally not implemented.
    #[serde(rename = "NOT_IMPLEMENTED")]
    NotImplemented,
    /// The wallet cannot cover the amount or its fee reserve.
    #[serde(rename = "INSUFFICIENT_BALANCE")]
    InsufficientBalance,
    /// The payment failed.
    #[serde(rename = "PAYMENT_FAILED")]
    PaymentFailed,
    /// The invoice could not be found by the given parameters.
    #[serde(rename = "NOT_FOUND")]
    NotFound,
    /// The wallet has exceeded its spending quota.
    #[serde(rename = "QUOTA_EXCEEDED")]
    QuotaExceeded,
    /// This public key is not allowed to do this operation.
    #[serde(rename = "RESTRICTED")]
    Restricted,
    /// This public key has no wallet connected.
    #[serde(rename = "UNAUTHORIZED")]
    Unauthorized,
    /// An internal wallet error.
    #[serde(rename = "INTERNAL")]
    Internal,
    /// The wallet does not support the selected encryption.
    #[serde(rename = "UNSUPPORTED_ENCRYPTION")]
    UnsupportedEncryption,
    /// Any other wallet error.
    #[serde(rename = "OTHER")]
    Other,
}

impl WalletErrorCode {
    /// The wire name from NIP-47.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::RateLimited => "RATE_LIMITED",
            Self::NotImplemented => "NOT_IMPLEMENTED",
            Self::InsufficientBalance => "INSUFFICIENT_BALANCE",
            Self::PaymentFailed => "PAYMENT_FAILED",
            Self::NotFound => "NOT_FOUND",
            Self::QuotaExceeded => "QUOTA_EXCEEDED",
            Self::Restricted => "RESTRICTED",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Internal => "INTERNAL",
            Self::UnsupportedEncryption => "UNSUPPORTED_ENCRYPTION",
            Self::Other => "OTHER",
        }
    }
}

impl fmt::Display for WalletErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// Reason codes attached to a classified outcome.
pub mod reason {
    /// The wallet answered NOT_IMPLEMENTED.
    pub const WALLET_NOT_IMPLEMENTED: &str = "WALLET_NOT_IMPLEMENTED";
    /// The wallet answered RESTRICTED.
    pub const WALLET_RESTRICTED: &str = "WALLET_RESTRICTED";
    /// The wallet answered UNAUTHORIZED.
    pub const WALLET_UNAUTHORIZED: &str = "WALLET_UNAUTHORIZED";
    /// The wallet answered RATE_LIMITED.
    pub const WALLET_RATE_LIMITED: &str = "WALLET_RATE_LIMITED";
    /// The wallet answered QUOTA_EXCEEDED.
    pub const WALLET_QUOTA_EXCEEDED: &str = "WALLET_QUOTA_EXCEEDED";
    /// The wallet answered INSUFFICIENT_BALANCE.
    pub const WALLET_INSUFFICIENT_BALANCE: &str = "WALLET_INSUFFICIENT_BALANCE";
    /// The wallet answered PAYMENT_FAILED.
    pub const WALLET_PAYMENT_FAILED: &str = "WALLET_PAYMENT_FAILED";
    /// The wallet answered UNSUPPORTED_ENCRYPTION.
    pub const WALLET_UNSUPPORTED_ENCRYPTION: &str = "WALLET_UNSUPPORTED_ENCRYPTION";
    /// The wallet answered INTERNAL.
    pub const WALLET_INTERNAL: &str = "WALLET_INTERNAL";
    /// The wallet answered OTHER.
    pub const WALLET_OTHER: &str = "WALLET_OTHER";
    /// The wallet answered NOT_FOUND where a found invoice was expected.
    pub const WALLET_NOT_FOUND: &str = "WALLET_NOT_FOUND";
    /// Timeout, relay unreachable or connect failure.
    pub const TRANSPORT_PROBLEM: &str = "TRANSPORT_PROBLEM";
    /// The payment may have happened and was not retried.
    pub const AMBIGUOUS_PAYMENT_NOT_RETRIED: &str = "AMBIGUOUS_PAYMENT_NOT_RETRIED";
    /// The method is not advertised, so no request was sent.
    pub const METHOD_NOT_ADVERTISED: &str = "METHOD_NOT_ADVERTISED";
    /// The test was not selected by the profile.
    pub const NOT_SELECTED: &str = "NOT_SELECTED";
    /// The expected behaviour was observed.
    pub const EXPECTED_BEHAVIOUR_OBSERVED: &str = "EXPECTED_BEHAVIOUR_OBSERVED";
    /// The test ran but the expected behaviour was not observed.
    pub const EXPECTED_BEHAVIOUR_NOT_OBSERVED: &str = "EXPECTED_BEHAVIOUR_NOT_OBSERVED";
    /// A local input was refused before any request was sent.
    pub const INPUT_REFUSED: &str = "INPUT_REFUSED";
    /// The run was not approved to pay.
    pub const PAYMENT_NOT_APPROVED: &str = "PAYMENT_NOT_APPROVED";
    /// A live run did not carry the acknowledgement string.
    pub const LIVE_NOT_ACKNOWLEDGED: &str = "LIVE_NOT_ACKNOWLEDGED";
    /// The run guard holds a PAYING or AMBIGUOUS payment for this wallet.
    pub const RUN_GUARD_BLOCKED: &str = "RUN_GUARD_BLOCKED";
    /// An earlier required test did not pass, so this one could not run.
    pub const PREREQUISITE_FAILED: &str = "PREREQUISITE_FAILED";
    /// The wallet returned no preimage.
    pub const NO_PREIMAGE: &str = "NO_PREIMAGE";
    /// The preimage does not hash to the invoice payment hash.
    pub const PREIMAGE_MISMATCH: &str = "PREIMAGE_MISMATCH";
    /// No info event arrived in time.
    pub const INFO_EVENT_NOT_FOUND: &str = "INFO_EVENT_NOT_FOUND";
}

/// What was observed while running one test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// The wallet answered with a NIP-47 error code.
    WalletError(WalletErrorCode),
    /// Timeout, relay unreachable or connect failure.
    TransportProblem,
    /// The payment may or may not have happened.
    AmbiguousPayment,
    /// The method is not advertised, so no request was sent.
    MethodNotAdvertised,
    /// The profile did not select the test.
    NotSelected,
    /// A local input (for example an invoice or a permit) was refused.
    InputRefused,
    /// The run was not approved to pay.
    PaymentNotApproved,
    /// A live run did not carry the acknowledgement string.
    LiveNotAcknowledged,
    /// The run guard holds a PAYING or AMBIGUOUS payment for this wallet.
    RunGuardBlocked,
    /// An earlier required test did not pass.
    PrerequisiteFailed,
    /// The wallet returned no preimage.
    PreimageMissing,
    /// The preimage does not hash to the invoice payment hash.
    PreimageMismatch,
    /// No info event arrived in time.
    InfoEventMissing,
    /// The test ran and showed the expected behaviour.
    ExpectedBehaviourObserved,
    /// The test ran and did not show the expected behaviour.
    ExpectedBehaviourNotObserved,
}

/// A status together with the short reason code that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    status: Status,
    reason_code: &'static str,
}

impl Outcome {
    const fn new(status: Status, reason_code: &'static str) -> Self {
        Self {
            status,
            reason_code,
        }
    }

    /// The status this observation maps to.
    #[must_use]
    pub const fn status(self) -> Status {
        self.status
    }

    /// The short machine-readable reason code.
    #[must_use]
    pub const fn reason_code(self) -> &'static str {
        self.reason_code
    }
}

/// Maps one observation to a status and reason code.
///
/// This is the table from the Tally specification. NOT_FOUND is treated as
/// "a found invoice was expected", which is the only case the specification
/// defines.
#[must_use]
pub fn classify_wallet_outcome(observation: Observation) -> Outcome {
    match observation {
        Observation::WalletError(WalletErrorCode::NotImplemented) => {
            Outcome::new(Status::Unsupported, reason::WALLET_NOT_IMPLEMENTED)
        }
        Observation::WalletError(WalletErrorCode::Restricted) => {
            Outcome::new(Status::Blocked, reason::WALLET_RESTRICTED)
        }
        Observation::WalletError(WalletErrorCode::Unauthorized) => {
            Outcome::new(Status::Blocked, reason::WALLET_UNAUTHORIZED)
        }
        Observation::WalletError(WalletErrorCode::RateLimited) => {
            Outcome::new(Status::Blocked, reason::WALLET_RATE_LIMITED)
        }
        Observation::WalletError(WalletErrorCode::QuotaExceeded) => {
            Outcome::new(Status::Blocked, reason::WALLET_QUOTA_EXCEEDED)
        }
        Observation::WalletError(WalletErrorCode::InsufficientBalance) => {
            Outcome::new(Status::Fail, reason::WALLET_INSUFFICIENT_BALANCE)
        }
        Observation::WalletError(WalletErrorCode::PaymentFailed) => {
            Outcome::new(Status::Fail, reason::WALLET_PAYMENT_FAILED)
        }
        Observation::WalletError(WalletErrorCode::UnsupportedEncryption) => {
            Outcome::new(Status::Fail, reason::WALLET_UNSUPPORTED_ENCRYPTION)
        }
        Observation::WalletError(WalletErrorCode::Internal) => {
            Outcome::new(Status::Fail, reason::WALLET_INTERNAL)
        }
        Observation::WalletError(WalletErrorCode::Other) => {
            Outcome::new(Status::Fail, reason::WALLET_OTHER)
        }
        Observation::WalletError(WalletErrorCode::NotFound) => {
            Outcome::new(Status::Fail, reason::WALLET_NOT_FOUND)
        }
        Observation::TransportProblem => Outcome::new(Status::Blocked, reason::TRANSPORT_PROBLEM),
        Observation::AmbiguousPayment => {
            Outcome::new(Status::Blocked, reason::AMBIGUOUS_PAYMENT_NOT_RETRIED)
        }
        Observation::MethodNotAdvertised => {
            Outcome::new(Status::Unsupported, reason::METHOD_NOT_ADVERTISED)
        }
        Observation::NotSelected => Outcome::new(Status::NotTested, reason::NOT_SELECTED),
        Observation::InputRefused => Outcome::new(Status::Fail, reason::INPUT_REFUSED),
        Observation::PaymentNotApproved => {
            Outcome::new(Status::NotTested, reason::PAYMENT_NOT_APPROVED)
        }
        Observation::LiveNotAcknowledged => {
            Outcome::new(Status::Blocked, reason::LIVE_NOT_ACKNOWLEDGED)
        }
        Observation::RunGuardBlocked => Outcome::new(Status::Blocked, reason::RUN_GUARD_BLOCKED),
        Observation::PrerequisiteFailed => {
            Outcome::new(Status::Blocked, reason::PREREQUISITE_FAILED)
        }
        Observation::PreimageMissing => Outcome::new(Status::Fail, reason::NO_PREIMAGE),
        Observation::PreimageMismatch => Outcome::new(Status::Fail, reason::PREIMAGE_MISMATCH),
        Observation::InfoEventMissing => Outcome::new(Status::Fail, reason::INFO_EVENT_NOT_FOUND),
        Observation::ExpectedBehaviourObserved => {
            Outcome::new(Status::Pass, reason::EXPECTED_BEHAVIOUR_OBSERVED)
        }
        Observation::ExpectedBehaviourNotObserved => {
            Outcome::new(Status::Fail, reason::EXPECTED_BEHAVIOUR_NOT_OBSERVED)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Observation, WalletErrorCode, classify_wallet_outcome, reason};
    use crate::status::Status;

    const ALL_CODES: [WalletErrorCode; 11] = [
        WalletErrorCode::RateLimited,
        WalletErrorCode::NotImplemented,
        WalletErrorCode::InsufficientBalance,
        WalletErrorCode::PaymentFailed,
        WalletErrorCode::NotFound,
        WalletErrorCode::QuotaExceeded,
        WalletErrorCode::Restricted,
        WalletErrorCode::Unauthorized,
        WalletErrorCode::Internal,
        WalletErrorCode::UnsupportedEncryption,
        WalletErrorCode::Other,
    ];

    #[test]
    fn every_wallet_error_row_of_the_table() {
        let expected = [
            (
                WalletErrorCode::NotImplemented,
                Status::Unsupported,
                reason::WALLET_NOT_IMPLEMENTED,
            ),
            (
                WalletErrorCode::Restricted,
                Status::Blocked,
                reason::WALLET_RESTRICTED,
            ),
            (
                WalletErrorCode::Unauthorized,
                Status::Blocked,
                reason::WALLET_UNAUTHORIZED,
            ),
            (
                WalletErrorCode::RateLimited,
                Status::Blocked,
                reason::WALLET_RATE_LIMITED,
            ),
            (
                WalletErrorCode::QuotaExceeded,
                Status::Blocked,
                reason::WALLET_QUOTA_EXCEEDED,
            ),
            (
                WalletErrorCode::InsufficientBalance,
                Status::Fail,
                reason::WALLET_INSUFFICIENT_BALANCE,
            ),
            (
                WalletErrorCode::PaymentFailed,
                Status::Fail,
                reason::WALLET_PAYMENT_FAILED,
            ),
            (
                WalletErrorCode::UnsupportedEncryption,
                Status::Fail,
                reason::WALLET_UNSUPPORTED_ENCRYPTION,
            ),
            (
                WalletErrorCode::Internal,
                Status::Fail,
                reason::WALLET_INTERNAL,
            ),
            (WalletErrorCode::Other, Status::Fail, reason::WALLET_OTHER),
            (
                WalletErrorCode::NotFound,
                Status::Fail,
                reason::WALLET_NOT_FOUND,
            ),
        ];

        assert_eq!(expected.len(), ALL_CODES.len());
        for (code, status, reason_code) in expected {
            let outcome = classify_wallet_outcome(Observation::WalletError(code));
            assert_eq!(outcome.status(), status, "code {code}");
            assert_eq!(outcome.reason_code(), reason_code, "code {code}");
        }
    }

    #[test]
    fn remaining_rows_of_the_table() {
        let cases = [
            (
                Observation::TransportProblem,
                Status::Blocked,
                reason::TRANSPORT_PROBLEM,
            ),
            (
                Observation::AmbiguousPayment,
                Status::Blocked,
                reason::AMBIGUOUS_PAYMENT_NOT_RETRIED,
            ),
            (
                Observation::MethodNotAdvertised,
                Status::Unsupported,
                reason::METHOD_NOT_ADVERTISED,
            ),
            (
                Observation::NotSelected,
                Status::NotTested,
                reason::NOT_SELECTED,
            ),
            (
                Observation::PaymentNotApproved,
                Status::NotTested,
                reason::PAYMENT_NOT_APPROVED,
            ),
            (
                Observation::LiveNotAcknowledged,
                Status::Blocked,
                reason::LIVE_NOT_ACKNOWLEDGED,
            ),
            (
                Observation::RunGuardBlocked,
                Status::Blocked,
                reason::RUN_GUARD_BLOCKED,
            ),
            (
                Observation::PrerequisiteFailed,
                Status::Blocked,
                reason::PREREQUISITE_FAILED,
            ),
            (
                Observation::PreimageMissing,
                Status::Fail,
                reason::NO_PREIMAGE,
            ),
            (
                Observation::PreimageMismatch,
                Status::Fail,
                reason::PREIMAGE_MISMATCH,
            ),
            (
                Observation::InfoEventMissing,
                Status::Fail,
                reason::INFO_EVENT_NOT_FOUND,
            ),
            (
                Observation::ExpectedBehaviourObserved,
                Status::Pass,
                reason::EXPECTED_BEHAVIOUR_OBSERVED,
            ),
            (
                Observation::ExpectedBehaviourNotObserved,
                Status::Fail,
                reason::EXPECTED_BEHAVIOUR_NOT_OBSERVED,
            ),
        ];

        for (observation, status, reason_code) in cases {
            let outcome = classify_wallet_outcome(observation);
            assert_eq!(outcome.status(), status, "observation {observation:?}");
            assert_eq!(outcome.reason_code(), reason_code);
        }
    }

    #[test]
    fn wire_names_match_the_specification() {
        for code in ALL_CODES {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.wire_name()));
        }
    }
}
