//! Pure pre-payment checks.
//!
//! Nothing here performs I/O: the caller passes plain numbers and gets a typed
//! refusal or the approved amount.

use std::fmt;

/// Milliseconds in one satoshi.
pub const MSAT_PER_SAT: u64 = 1_000;

/// Why a payment must not proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardError {
    /// No explicit approval was given for this run.
    NoExplicitApproval,
    /// The approved amount and the invoice amount differ.
    AmountMismatch {
        /// Sats approved by the operator.
        approved_sats: u64,
        /// Invoice amount converted to sats (0 when it is not a whole number).
        invoice_sats: u64,
    },
    /// The invoice has already expired.
    InvoiceExpired,
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoExplicitApproval => f.write_str("no explicit approval for this run"),
            Self::AmountMismatch {
                approved_sats,
                invoice_sats,
            } => write!(
                f,
                "approved amount {approved_sats} sats does not equal invoice amount {invoice_sats} sats"
            ),
            Self::InvoiceExpired => f.write_str("invoice is expired"),
        }
    }
}

impl std::error::Error for GuardError {}

/// Decides whether the payment may proceed.
///
/// Returns the approved amount in sats on success, or the first failing check.
pub fn check_payment(
    approved_sats: Option<u64>,
    invoice_amount_msat: u64,
    invoice_expired: bool,
) -> Result<u64, GuardError> {
    let approved_sats = approved_sats.ok_or(GuardError::NoExplicitApproval)?;

    let invoice_sats = invoice_amount_msat / MSAT_PER_SAT;
    if !invoice_amount_msat.is_multiple_of(MSAT_PER_SAT) || approved_sats != invoice_sats {
        return Err(GuardError::AmountMismatch {
            approved_sats,
            invoice_sats,
        });
    }

    if invoice_expired {
        return Err(GuardError::InvoiceExpired);
    }

    Ok(approved_sats)
}

#[cfg(test)]
mod tests {
    use super::{GuardError, MSAT_PER_SAT, check_payment};

    #[test]
    fn accepts_matching_unexpired_approval() {
        let approved = check_payment(Some(21), 21 * MSAT_PER_SAT, false).unwrap();
        assert_eq!(approved, 21);
    }

    #[test]
    fn refuses_wrong_amount() {
        let error = check_payment(Some(20), 21 * MSAT_PER_SAT, false).unwrap_err();
        assert_eq!(
            error,
            GuardError::AmountMismatch {
                approved_sats: 20,
                invoice_sats: 21
            }
        );
    }

    #[test]
    fn refuses_sub_satoshi_amounts() {
        let error = check_payment(Some(21), 21_500, false).unwrap_err();
        assert_eq!(
            error,
            GuardError::AmountMismatch {
                approved_sats: 21,
                invoice_sats: 21
            }
        );
    }

    #[test]
    fn refuses_expired_invoice() {
        let error = check_payment(Some(21), 21 * MSAT_PER_SAT, true).unwrap_err();
        assert_eq!(error, GuardError::InvoiceExpired);
    }

    #[test]
    fn refuses_missing_approval() {
        let error = check_payment(None, 21 * MSAT_PER_SAT, false).unwrap_err();
        assert_eq!(error, GuardError::NoExplicitApproval);
    }
}
