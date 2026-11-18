//! The only thing that authorises a payment.
//!
//! A [`PaymentPermit`] binds one invoice, its amount, its payment hash and the
//! environment. It is created only by [`PaymentPermit::issue`], after the guard
//! checks, and it is consumed by
//! [`WalletSession::pay_invoice`](crate::session::WalletSession::pay_invoice).

use std::fmt;
use std::str::FromStr;

use lightning_invoice::Bolt11Invoice;
use secrecy::{ExposeSecret, SecretString};

use tally_core::report::EnvClass;

use crate::error::{NwcError, NwcErrorKind};
use crate::guard::{self, GuardError};

/// The exact string a caller must pass to pay a live invoice.
pub const LIVE_ACKNOWLEDGEMENT: &str = "I ACKNOWLEDGE LIVE FUNDS";

/// Authorisation to pay one specific invoice once.
///
/// Not `Clone`, not `Copy`: paying consumes it, so one permit cannot pay twice.
pub struct PaymentPermit {
    invoice: SecretString,
    amount_msat: u64,
    payment_hash_hex: String,
    env: EnvClass,
}

impl PaymentPermit {
    /// Issues a permit for exactly this invoice.
    ///
    /// Checks, in order: the invoice parses, it carries an amount, the approved
    /// sats equal that amount exactly, the invoice has not expired, and a live
    /// environment carries [`LIVE_ACKNOWLEDGEMENT`].
    ///
    /// # Errors
    ///
    /// Returns [`NwcErrorKind::InvalidInput`] for every refusal. No request is
    /// sent and no secret or input text is included in the error.
    pub fn issue(
        invoice: &SecretString,
        approved_sats: Option<u64>,
        env: EnvClass,
        live_acknowledgement: Option<&str>,
    ) -> Result<Self, NwcError> {
        let parsed = Bolt11Invoice::from_str(invoice.expose_secret())
            .map_err(|_| NwcError::new(NwcErrorKind::InvalidInput, "invoice_unparseable"))?;

        let amount_msat = parsed.amount_milli_satoshis().ok_or(NwcError::new(
            NwcErrorKind::InvalidInput,
            "invoice_has_no_amount",
        ))?;

        guard::check_payment(approved_sats, amount_msat, parsed.is_expired())
            .map_err(guard_refusal)?;

        if env == EnvClass::Live && live_acknowledgement != Some(LIVE_ACKNOWLEDGEMENT) {
            return Err(NwcError::new(
                NwcErrorKind::InvalidInput,
                "live_acknowledgement_missing",
            ));
        }

        Ok(Self {
            invoice: SecretString::from(invoice.expose_secret().to_owned()),
            amount_msat,
            payment_hash_hex: parsed.payment_hash().to_string(),
            env,
        })
    }

    /// The bound amount in millisatoshis.
    #[must_use]
    pub const fn amount_msat(&self) -> u64 {
        self.amount_msat
    }

    /// The bound payment hash, hex.
    #[must_use]
    pub fn payment_hash_hex(&self) -> &str {
        &self.payment_hash_hex
    }

    /// The environment the permit was issued for.
    #[must_use]
    pub const fn env_class(&self) -> EnvClass {
        self.env
    }

    /// The bound invoice string. Crate-private on purpose.
    pub(crate) fn invoice_str(&self) -> &str {
        self.invoice.expose_secret()
    }
}

fn guard_refusal(error: GuardError) -> NwcError {
    let detail = match error {
        GuardError::NoExplicitApproval => "no_explicit_approval",
        GuardError::AmountMismatch { .. } => "amount_mismatch",
        GuardError::InvoiceExpired => "invoice_expired",
    };

    NwcError::new(NwcErrorKind::InvalidInput, detail)
}

impl fmt::Debug for PaymentPermit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaymentPermit")
            .field("invoice", &"[REDACTED]")
            .field("amount_msat", &self.amount_msat)
            .field("payment_hash_hex", &"[REDACTED]")
            .field("env", &self.env)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bitcoin::hashes::Hash;
    use bitcoin::hashes::sha256;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{Currency, InvoiceBuilder};
    use secrecy::SecretString;

    use super::{LIVE_ACKNOWLEDGEMENT, PaymentPermit};
    use crate::error::NwcErrorKind;
    use tally_core::report::EnvClass;

    /// Builds a signed invoice for the tests. Nothing here touches the network.
    fn test_invoice(amount_msat: u64, age: Duration) -> SecretString {
        let private_key = SecretKey::from_slice(&[7u8; 32]).unwrap();
        let payment_hash = sha256::Hash::from_slice(&[9u8; 32]).unwrap();
        let payment_secret = lightning_invoice::PaymentSecret([11u8; 32]);

        let invoice = InvoiceBuilder::new(Currency::Signet)
            .description(String::from("tally permit test"))
            .payment_hash(payment_hash)
            .payment_secret(payment_secret)
            .amount_milli_satoshis(amount_msat)
            .min_final_cltv_expiry_delta(144)
            .expiry_time(Duration::from_secs(3_600))
            .timestamp(SystemTime::now() - age)
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
            .unwrap();

        SecretString::from(invoice.to_string())
    }

    #[test]
    fn issues_for_a_matching_unexpired_sandbox_invoice() {
        let invoice = test_invoice(21_000, Duration::from_secs(0));
        let permit = PaymentPermit::issue(&invoice, Some(21), EnvClass::Sandbox, None).unwrap();

        assert_eq!(permit.amount_msat(), 21_000);
        assert_eq!(permit.env_class(), EnvClass::Sandbox);
        assert_eq!(permit.payment_hash_hex().len(), 64);
    }

    #[test]
    fn refuses_without_approval() {
        let invoice = test_invoice(21_000, Duration::from_secs(0));
        let error = PaymentPermit::issue(&invoice, None, EnvClass::Sandbox, None).unwrap_err();

        assert_eq!(error.kind(), NwcErrorKind::InvalidInput);
        assert_eq!(error.detail(), "no_explicit_approval");
    }

    #[test]
    fn refuses_a_different_amount() {
        let invoice = test_invoice(21_000, Duration::from_secs(0));
        let error = PaymentPermit::issue(&invoice, Some(20), EnvClass::Sandbox, None).unwrap_err();

        assert_eq!(error.detail(), "amount_mismatch");
    }

    #[test]
    fn refuses_an_expired_invoice() {
        let invoice = test_invoice(21_000, Duration::from_secs(7_200));
        let error = PaymentPermit::issue(&invoice, Some(21), EnvClass::Sandbox, None).unwrap_err();

        assert_eq!(error.detail(), "invoice_expired");
    }

    #[test]
    fn live_needs_the_acknowledgement_string() {
        let invoice = test_invoice(21_000, Duration::from_secs(0));

        let missing = PaymentPermit::issue(&invoice, Some(21), EnvClass::Live, None).unwrap_err();
        assert_eq!(missing.detail(), "live_acknowledgement_missing");

        let wrong =
            PaymentPermit::issue(&invoice, Some(21), EnvClass::Live, Some("yes")).unwrap_err();
        assert_eq!(wrong.detail(), "live_acknowledgement_missing");

        let accepted = PaymentPermit::issue(
            &invoice,
            Some(21),
            EnvClass::Live,
            Some(LIVE_ACKNOWLEDGEMENT),
        )
        .unwrap();
        assert_eq!(accepted.env_class(), EnvClass::Live);
    }

    #[test]
    fn refuses_unparseable_input_without_echoing_it() {
        let sentinel = SecretString::from(String::from("lnbc21not-an-invoice-sentinel"));
        let error = PaymentPermit::issue(&sentinel, Some(21), EnvClass::Sandbox, None).unwrap_err();

        assert_eq!(error.detail(), "invoice_unparseable");
        let text = format!("{error} {error:?}");
        assert!(!text.contains("sentinel"));
        assert!(!text.contains("lnbc"));
    }

    #[test]
    fn debug_never_reveals_the_invoice_or_hash() {
        let invoice = test_invoice(21_000, Duration::from_secs(0));
        let permit = PaymentPermit::issue(&invoice, Some(21), EnvClass::Sandbox, None).unwrap();

        let debug = format!("{permit:?}");
        assert!(!debug.contains("lnbc"));
        assert!(!debug.contains(permit.payment_hash_hex()));
        assert!(debug.contains("[REDACTED]"));
        assert!(debug.contains("21000"));
    }
}
