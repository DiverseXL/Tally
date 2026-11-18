//! Sanitized wallet errors.
//!
//! An [`NwcError`] carries a kind and a fixed detail code only. It never holds
//! the input that failed, so it cannot leak a connection string, an invoice or a
//! preimage through `Display` or `Debug`.

use std::error::Error;
use std::fmt;

use tally_core::outcome::{Observation, Outcome, WalletErrorCode, classify_wallet_outcome};

/// The kind of an [`NwcError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NwcErrorKind {
    /// The wallet answered with a NIP-47 error code.
    Wallet(WalletErrorCode),
    /// Timeout, relay unreachable or connect failure.
    Transport,
    /// The payment may have happened and must not be retried.
    AmbiguousPayment,
    /// The response was malformed, uncorrelated or of the wrong type.
    Protocol,
    /// A local input was refused before any request was sent.
    InvalidInput,
    /// The method is not advertised, so no request was sent.
    UnsupportedMethod,
}

impl NwcErrorKind {
    /// A stable name for output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wallet(code) => code.wire_name(),
            Self::Transport => "TRANSPORT",
            Self::AmbiguousPayment => "AMBIGUOUS_PAYMENT",
            Self::Protocol => "PROTOCOL",
            Self::InvalidInput => "INVALID_INPUT",
            Self::UnsupportedMethod => "UNSUPPORTED_METHOD",
        }
    }
}

impl fmt::Display for NwcErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An adapter error that is safe to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NwcError {
    kind: NwcErrorKind,
    detail: &'static str,
}

impl NwcError {
    /// Builds an error from a kind and a fixed detail code.
    ///
    /// `detail` must be a static code, never user input.
    #[must_use]
    pub const fn new(kind: NwcErrorKind, detail: &'static str) -> Self {
        Self { kind, detail }
    }

    /// The kind.
    #[must_use]
    pub const fn kind(self) -> NwcErrorKind {
        self.kind
    }

    /// The fixed detail code.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        self.detail
    }

    /// The status this error maps to, using the shared table in `tally-core`.
    #[must_use]
    pub fn classify(self) -> Outcome {
        let observation = match self.kind {
            NwcErrorKind::Wallet(code) => Observation::WalletError(code),
            NwcErrorKind::Transport => Observation::TransportProblem,
            NwcErrorKind::AmbiguousPayment => Observation::AmbiguousPayment,
            NwcErrorKind::Protocol => Observation::ExpectedBehaviourNotObserved,
            NwcErrorKind::InvalidInput => Observation::InputRefused,
            NwcErrorKind::UnsupportedMethod => Observation::MethodNotAdvertised,
        };

        classify_wallet_outcome(observation)
    }

    /// Maps an `nwc` crate error to a sanitized adapter error.
    ///
    /// The NIP-47 error code is recovered from the response message: the nostr
    /// crate turns a wallet error into `Error::new(ErrorKind::Other,
    /// code.to_string())` (`nostr-0.45.7/src/nips/nip47.rs`, `error_code`), and
    /// `NIP47Error`'s Display is `"{message} [{code:?}]"`
    /// (`nostr-0.45.7/src/nips/nip47.rs`). Only the bracketed code is read; the
    /// message text is never stored.
    #[must_use]
    pub fn from_nwc(error: &nwc::error::Error) -> Self {
        use nwc::error::ErrorKind;

        match error.kind() {
            ErrorKind::NoResponse => Self::new(NwcErrorKind::Transport, "response_not_received"),
            ErrorKind::UnrelatedResponse => Self::new(NwcErrorKind::Protocol, "unrelated_response"),
            ErrorKind::Sdk => Self::new(NwcErrorKind::Transport, "client_failure"),
            ErrorKind::Protocol => match wallet_code_from_message(&error.to_string()) {
                Some(code) => Self::new(NwcErrorKind::Wallet(code), "wallet_error"),
                None => Self::new(NwcErrorKind::Protocol, "protocol"),
            },
            ErrorKind::Other => Self::new(NwcErrorKind::Protocol, "other"),
            // The enum is non-exhaustive; a new kind is treated as a protocol
            // problem rather than a wallet answer.
            _ => Self::new(NwcErrorKind::Protocol, "unknown_error_kind"),
        }
    }

    /// Maps an `nwc` crate error from `pay_invoice`.
    ///
    /// Any outcome that does not carry a wallet error code may mean the payment
    /// happened, so it becomes [`NwcErrorKind::AmbiguousPayment`] and the caller
    /// must not retry.
    #[must_use]
    pub fn from_nwc_payment(error: &nwc::error::Error) -> Self {
        use nwc::error::ErrorKind;

        let mapped = Self::from_nwc(error);

        match error.kind() {
            ErrorKind::Protocol => match mapped.kind {
                NwcErrorKind::Wallet(_) => mapped,
                _ => Self::new(NwcErrorKind::AmbiguousPayment, "payment_outcome_unknown"),
            },
            // Timeouts, client failures and unknown kinds: the payment may have
            // happened, so it is ambiguous and must not be retried.
            _ => Self::new(NwcErrorKind::AmbiguousPayment, "payment_outcome_unknown"),
        }
    }
}

impl fmt::Display for NwcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nwc error: {} ({})", self.kind, self.detail)
    }
}

impl Error for NwcError {}

/// Debug names of the NIP-47 error codes, as printed by `NIP47Error`.
const CODE_NAMES: [(&str, WalletErrorCode); 10] = [
    ("[RateLimited]", WalletErrorCode::RateLimited),
    ("[NotImplemented]", WalletErrorCode::NotImplemented),
    (
        "[InsufficientBalance]",
        WalletErrorCode::InsufficientBalance,
    ),
    ("[PaymentFailed]", WalletErrorCode::PaymentFailed),
    ("[NotFound]", WalletErrorCode::NotFound),
    ("[QuotaExceeded]", WalletErrorCode::QuotaExceeded),
    ("[Restricted]", WalletErrorCode::Restricted),
    ("[Unauthorized]", WalletErrorCode::Unauthorized),
    ("[Internal]", WalletErrorCode::Internal),
    ("[Other]", WalletErrorCode::Other),
];

/// Extracts the NIP-47 error code from a wallet error message.
///
/// Returns `None` for messages that carry no code. The message text is not kept.
#[must_use]
pub fn wallet_code_from_message(message: &str) -> Option<WalletErrorCode> {
    CODE_NAMES
        .iter()
        .find(|(needle, _)| message.contains(needle))
        .map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::{NwcError, NwcErrorKind, wallet_code_from_message};
    use tally_core::WalletErrorCode;
    use tally_core::status::Status;

    const SENTINEL: &str = "lnbc21n1sentinelinvoicevalue";

    #[test]
    fn reads_every_code_name_from_a_message() {
        let cases = [
            ("boom [RateLimited]", WalletErrorCode::RateLimited),
            ("boom [NotImplemented]", WalletErrorCode::NotImplemented),
            (
                "boom [InsufficientBalance]",
                WalletErrorCode::InsufficientBalance,
            ),
            ("boom [PaymentFailed]", WalletErrorCode::PaymentFailed),
            ("boom [NotFound]", WalletErrorCode::NotFound),
            ("boom [QuotaExceeded]", WalletErrorCode::QuotaExceeded),
            ("boom [Restricted]", WalletErrorCode::Restricted),
            ("boom [Unauthorized]", WalletErrorCode::Unauthorized),
            ("boom [Internal]", WalletErrorCode::Internal),
            ("boom [Other]", WalletErrorCode::Other),
        ];

        for (message, code) in cases {
            assert_eq!(wallet_code_from_message(message), Some(code));
        }

        assert_eq!(wallet_code_from_message("no code here"), None);
    }

    #[test]
    fn errors_never_contain_input() {
        let error = NwcError::new(NwcErrorKind::Protocol, "protocol");
        let text = format!("{error} {:?}", error);

        assert!(!text.contains(SENTINEL));
        assert!(text.contains("protocol"));
        assert!(!text.contains("nostr+walletconnect"));
    }

    #[test]
    fn classify_uses_the_shared_table() {
        let unsupported = NwcError::new(
            NwcErrorKind::Wallet(WalletErrorCode::NotImplemented),
            "wallet_error",
        );
        assert_eq!(unsupported.classify().status(), Status::Unsupported);

        let ambiguous = NwcError::new(NwcErrorKind::AmbiguousPayment, "payment_outcome_unknown");
        assert_eq!(ambiguous.classify().status(), Status::Blocked);
        assert_eq!(
            ambiguous.classify().reason_code(),
            "AMBIGUOUS_PAYMENT_NOT_RETRIED"
        );

        let refused = NwcError::new(NwcErrorKind::InvalidInput, "amount_mismatch");
        assert_eq!(refused.classify().status(), Status::Fail);

        let not_advertised = NwcError::new(NwcErrorKind::UnsupportedMethod, "not_advertised");
        assert_eq!(not_advertised.classify().status(), Status::Unsupported);
    }
}
