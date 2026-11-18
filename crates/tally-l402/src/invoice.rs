//! BOLT11 invoice facts, used to check an invoice before it is presented.
//!
//! Every error here is a sanitized kind: no invoice string, no payment hash and
//! no amount from the input is echoed.

use std::fmt;
use std::str::FromStr;

use lightning_invoice::{Bolt11Invoice, Currency};

/// Why an invoice was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvoiceErrorKind {
    /// The string is not a BOLT11 invoice.
    Unparseable,
    /// The invoice network differs from the network the wallet reports.
    NetworkMismatch,
    /// The wallet-reported network string is not one of the known names.
    UnknownWalletNetwork,
    /// The invoice carries no amount.
    AmountMissing,
    /// The invoice amount differs from the expected one.
    AmountMismatch,
    /// The invoice has expired.
    Expired,
}

impl InvoiceErrorKind {
    /// A stable short name, for logs and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unparseable => "invoice_unparseable",
            Self::NetworkMismatch => "invoice_network_mismatch",
            Self::UnknownWalletNetwork => "wallet_network_unknown",
            Self::AmountMissing => "invoice_amount_missing",
            Self::AmountMismatch => "invoice_amount_mismatch",
            Self::Expired => "invoice_expired",
        }
    }
}

/// A sanitized invoice error. It never holds a copy of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvoiceError {
    kind: InvoiceErrorKind,
}

impl InvoiceError {
    const fn new(kind: InvoiceErrorKind) -> Self {
        Self { kind }
    }

    /// What went wrong, as a kind.
    #[must_use]
    pub const fn kind(&self) -> InvoiceErrorKind {
        self.kind
    }
}

impl fmt::Display for InvoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.as_str())
    }
}

impl std::error::Error for InvoiceError {}

/// What an invoice says, without its string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceFacts {
    /// Network name: `bitcoin`, `testnet`, `signet`, `regtest` or `simnet`.
    pub network: &'static str,
    /// Amount in millisatoshis, when the invoice carries one.
    pub amount_msat: Option<u64>,
    /// The payment hash, 32 bytes.
    pub payment_hash: [u8; 32],
    /// Seconds left before expiry at the time it was parsed.
    pub seconds_until_expiry: u64,
    /// Whether it had already expired at the time it was parsed.
    pub expired: bool,
    /// The parsed currency, kept for the network check.
    currency: Currency,
}

impl InvoiceFacts {
    /// Checks the network against a wallet-reported name.
    ///
    /// The wallet strings are NIP-47 `get_info` network names: `mainnet` for
    /// Bitcoin, then `testnet`, `signet` and `regtest`.
    ///
    /// # Errors
    ///
    /// [`InvoiceErrorKind::UnknownWalletNetwork`] or
    /// [`InvoiceErrorKind::NetworkMismatch`].
    pub fn check_network(&self, wallet_network: &str) -> Result<(), InvoiceError> {
        let expected = wallet_currency(wallet_network)?;

        if expected == self.currency {
            Ok(())
        } else {
            Err(InvoiceError::new(InvoiceErrorKind::NetworkMismatch))
        }
    }

    /// Checks the amount exactly, in millisatoshis.
    ///
    /// # Errors
    ///
    /// [`InvoiceErrorKind::AmountMissing`] when the invoice carries no amount,
    /// [`InvoiceErrorKind::AmountMismatch`] when it differs.
    pub fn check_amount(&self, expected_msat: u64) -> Result<(), InvoiceError> {
        match self.amount_msat {
            None => Err(InvoiceError::new(InvoiceErrorKind::AmountMissing)),
            Some(amount) if amount == expected_msat => Ok(()),
            Some(_) => Err(InvoiceError::new(InvoiceErrorKind::AmountMismatch)),
        }
    }

    /// Checks that the invoice has not expired.
    ///
    /// # Errors
    ///
    /// [`InvoiceErrorKind::Expired`].
    pub fn check_not_expired(&self) -> Result<(), InvoiceError> {
        if self.expired {
            Err(InvoiceError::new(InvoiceErrorKind::Expired))
        } else {
            Ok(())
        }
    }

    /// Runs all three checks and returns the facts.
    ///
    /// # Errors
    ///
    /// Any of the errors of [`Self::check_network`], [`Self::check_amount`] and
    /// [`Self::check_not_expired`].
    pub fn check(&self, wallet_network: &str, expected_msat: u64) -> Result<(), InvoiceError> {
        self.check_network(wallet_network)?;
        self.check_amount(expected_msat)?;
        self.check_not_expired()
    }
}

/// Parses a BOLT11 invoice into facts.
///
/// # Errors
///
/// [`InvoiceErrorKind::Unparseable`] when the string is not a BOLT11 invoice.
/// The error never contains the string.
pub fn parse(invoice: &str) -> Result<InvoiceFacts, InvoiceError> {
    let parsed = Bolt11Invoice::from_str(invoice)
        .map_err(|_| InvoiceError::new(InvoiceErrorKind::Unparseable))?;

    let mut payment_hash = [0u8; 32];
    payment_hash.copy_from_slice(parsed.payment_hash().as_ref());

    Ok(InvoiceFacts {
        network: network_name(parsed.currency()),
        amount_msat: parsed.amount_milli_satoshis(),
        payment_hash,
        seconds_until_expiry: parsed.duration_until_expiry().as_secs(),
        expired: parsed.is_expired(),
        currency: parsed.currency(),
    })
}

/// Parses an invoice and checks it against the wallet network and an amount.
///
/// # Errors
///
/// Any error of [`parse`] or [`InvoiceFacts::check`].
pub fn check(
    invoice: &str,
    wallet_network: &str,
    expected_msat: u64,
) -> Result<InvoiceFacts, InvoiceError> {
    let facts = parse(invoice)?;
    facts.check(wallet_network, expected_msat)?;
    Ok(facts)
}

fn wallet_currency(wallet_network: &str) -> Result<Currency, InvoiceError> {
    match wallet_network {
        "mainnet" | "bitcoin" => Ok(Currency::Bitcoin),
        "testnet" => Ok(Currency::BitcoinTestnet),
        "signet" => Ok(Currency::Signet),
        "regtest" => Ok(Currency::Regtest),
        "simnet" => Ok(Currency::Simnet),
        _ => Err(InvoiceError::new(InvoiceErrorKind::UnknownWalletNetwork)),
    }
}

const fn network_name(currency: Currency) -> &'static str {
    match currency {
        Currency::Bitcoin => "bitcoin",
        Currency::BitcoinTestnet => "testnet",
        Currency::Signet => "signet",
        Currency::Regtest => "regtest",
        Currency::Simnet => "simnet",
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use bitcoin::hashes::Hash;
    use bitcoin::hashes::sha256;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{Currency, InvoiceBuilder};

    use super::{InvoiceErrorKind, check, parse};

    /// Builds a signed invoice offline. No network is used.
    fn invoice(currency: Currency, amount_msat: Option<u64>, age: Duration) -> String {
        let private_key = SecretKey::from_slice(&[3u8; 32]).unwrap();
        let payment_hash = sha256::Hash::from_slice(&[5u8; 32]).unwrap();
        let payment_secret = lightning_invoice::PaymentSecret([13u8; 32]);

        let builder = InvoiceBuilder::new(currency)
            .description(String::from("tally l402 invoice test"))
            .payment_hash(payment_hash)
            .payment_secret(payment_secret)
            .min_final_cltv_expiry_delta(144)
            .expiry_time(Duration::from_secs(3_600))
            .timestamp(SystemTime::now() - age);

        let builder = match amount_msat {
            Some(amount) => builder.amount_milli_satoshis(amount),
            None => builder,
        };

        builder
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
            .unwrap()
            .to_string()
    }

    #[test]
    fn parses_network_amount_hash_and_expiry() {
        let text = invoice(Currency::Signet, Some(21_000), Duration::from_secs(0));
        let facts = parse(&text).unwrap();

        assert_eq!(facts.network, "signet");
        assert_eq!(facts.amount_msat, Some(21_000));
        assert_eq!(facts.payment_hash, [5u8; 32]);
        assert!(!facts.expired);
        assert!(facts.seconds_until_expiry <= 3_600);
    }

    #[test]
    fn invoice_without_an_amount_reports_none() {
        let text = invoice(Currency::Bitcoin, None, Duration::from_secs(0));
        let facts = parse(&text).unwrap();

        assert_eq!(facts.network, "bitcoin");
        assert_eq!(facts.amount_msat, None);
        assert_eq!(
            facts.check_amount(21_000).unwrap_err().kind(),
            InvoiceErrorKind::AmountMissing
        );
    }

    #[test]
    fn wallet_network_names_map_to_currencies() {
        let mainnet = parse(&invoice(Currency::Bitcoin, Some(1_000), Duration::ZERO)).unwrap();
        assert!(mainnet.check_network("mainnet").is_ok());

        let testnet = parse(&invoice(
            Currency::BitcoinTestnet,
            Some(1_000),
            Duration::ZERO,
        ))
        .unwrap();
        assert!(testnet.check_network("testnet").is_ok());
        assert_eq!(
            testnet.check_network("mainnet").unwrap_err().kind(),
            InvoiceErrorKind::NetworkMismatch
        );

        let regtest = parse(&invoice(Currency::Regtest, Some(1_000), Duration::ZERO)).unwrap();
        assert!(regtest.check_network("regtest").is_ok());

        let signet = parse(&invoice(Currency::Signet, Some(1_000), Duration::ZERO)).unwrap();
        assert!(signet.check_network("signet").is_ok());
        assert_eq!(
            signet.check_network("not-a-network").unwrap_err().kind(),
            InvoiceErrorKind::UnknownWalletNetwork
        );
    }

    #[test]
    fn amount_is_compared_exactly() {
        let facts = parse(&invoice(Currency::Signet, Some(21_000), Duration::ZERO)).unwrap();

        assert!(facts.check_amount(21_000).is_ok());
        assert_eq!(
            facts.check_amount(20_999).unwrap_err().kind(),
            InvoiceErrorKind::AmountMismatch
        );
    }

    #[test]
    fn expired_invoice_is_reported() {
        let facts = parse(&invoice(
            Currency::Signet,
            Some(21_000),
            Duration::from_secs(7_200),
        ))
        .unwrap();

        assert!(facts.expired);
        assert_eq!(
            facts.check_not_expired().unwrap_err().kind(),
            InvoiceErrorKind::Expired
        );
    }

    #[test]
    fn unparseable_input_is_refused_without_echoing_it() {
        let error = parse("lnbc-not-a-real-invoice-sentinel").unwrap_err();

        assert_eq!(error.kind(), InvoiceErrorKind::Unparseable);
        let text = format!("{error} {error:?}");
        assert!(!text.contains("sentinel"));
    }

    #[test]
    fn check_combines_network_amount_and_expiry() {
        let text = invoice(Currency::Signet, Some(21_000), Duration::ZERO);
        let facts = check(&text, "signet", 21_000).unwrap();
        assert_eq!(facts.amount_msat, Some(21_000));

        assert_eq!(
            check(&text, "mainnet", 21_000).unwrap_err().kind(),
            InvoiceErrorKind::NetworkMismatch
        );
        assert_eq!(
            check(&text, "signet", 1_000).unwrap_err().kind(),
            InvoiceErrorKind::AmountMismatch
        );
    }
}
