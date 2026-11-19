//! The wallet operations the runner needs, as one trait.
//!
//! The trait is used as a generic bound, never as `dyn`: the implementation is
//! chosen at compile time and every method returns a `Send` future. Payments
//! still take a [`PaymentPermit`](crate::PaymentPermit), so an implementation
//! cannot pay without one.

use std::future::Future;

use crate::error::NwcError;
use crate::permit::PaymentPermit;
use crate::session::{
    ConnectReport, EncryptionMode, InfoEventRecord, InvoiceLookup, MadeInvoice, PaidInvoice,
    WalletInfo, WalletSession,
};

/// One wallet, addressed by the operations Tally tests.
///
/// `WalletSession` is the real implementation; the offline tests use a fake one.
pub trait WalletApi {
    /// The wallet service public key, hex.
    fn service_pubkey_hex(&self) -> String;

    /// The relay hosts of this connection, without scheme, path or credentials.
    fn relay_hosts(&self) -> Vec<String>;

    /// Connects to the relays and waits for the handshake.
    fn connect(&self) -> impl Future<Output = Result<ConnectReport, NwcError>> + Send;

    /// The cipher this wallet was told to use, or `None` until one is chosen.
    fn encryption_mode(&self) -> Option<EncryptionMode>;

    /// Tells the wallet which NIP-47 cipher to use and reconnects.
    fn set_encryption_mode(
        &mut self,
        mode: EncryptionMode,
    ) -> impl Future<Output = Result<ConnectReport, NwcError>> + Send;

    /// Fetches the kind 13194 info event, if the wallet publishes one.
    fn fetch_info_event(
        &self,
    ) -> impl Future<Output = Result<Option<InfoEventRecord>, NwcError>> + Send;

    /// Sends `get_info`.
    fn get_info(&self) -> impl Future<Output = Result<WalletInfo, NwcError>> + Send;

    /// Sends `get_balance`, in millisatoshis.
    fn get_balance(&self) -> impl Future<Output = Result<u64, NwcError>> + Send;

    /// Asks the wallet for an invoice.
    fn make_invoice(
        &self,
        amount_msat: u64,
        description: &str,
        expiry_seconds: u64,
    ) -> impl Future<Output = Result<MadeInvoice, NwcError>> + Send;

    /// Sends `lookup_invoice` for a payment hash.
    fn lookup_invoice(
        &self,
        payment_hash_hex: &str,
    ) -> impl Future<Output = Result<InvoiceLookup, NwcError>> + Send;

    /// Pays exactly the invoice bound by `permit`, once.
    fn pay_invoice(
        &self,
        permit: PaymentPermit,
    ) -> impl Future<Output = Result<PaidInvoice, NwcError>> + Send;
}

impl WalletApi for WalletSession {
    fn service_pubkey_hex(&self) -> String {
        WalletSession::service_pubkey_hex(self)
    }

    fn relay_hosts(&self) -> Vec<String> {
        WalletSession::relay_hosts(self)
    }

    fn connect(&self) -> impl Future<Output = Result<ConnectReport, NwcError>> + Send {
        WalletSession::connect(self)
    }

    fn encryption_mode(&self) -> Option<EncryptionMode> {
        WalletSession::encryption_mode(self)
    }

    fn set_encryption_mode(
        &mut self,
        mode: EncryptionMode,
    ) -> impl Future<Output = Result<ConnectReport, NwcError>> + Send {
        WalletSession::set_encryption_mode(self, mode)
    }

    fn fetch_info_event(
        &self,
    ) -> impl Future<Output = Result<Option<InfoEventRecord>, NwcError>> + Send {
        WalletSession::fetch_info_event(self)
    }

    fn get_info(&self) -> impl Future<Output = Result<WalletInfo, NwcError>> + Send {
        WalletSession::get_info(self)
    }

    fn get_balance(&self) -> impl Future<Output = Result<u64, NwcError>> + Send {
        WalletSession::get_balance(self)
    }

    fn make_invoice(
        &self,
        amount_msat: u64,
        description: &str,
        expiry_seconds: u64,
    ) -> impl Future<Output = Result<MadeInvoice, NwcError>> + Send {
        WalletSession::make_invoice(self, amount_msat, description, expiry_seconds)
    }

    fn lookup_invoice(
        &self,
        payment_hash_hex: &str,
    ) -> impl Future<Output = Result<InvoiceLookup, NwcError>> + Send {
        WalletSession::lookup_invoice(self, payment_hash_hex)
    }

    fn pay_invoice(
        &self,
        permit: PaymentPermit,
    ) -> impl Future<Output = Result<PaidInvoice, NwcError>> + Send {
        WalletSession::pay_invoice(self, permit)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bitcoin::hashes::Hash;
    use bitcoin::hashes::sha256;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use lightning_invoice::{Currency, InvoiceBuilder};
    use nostr::key::Keys;
    use secrecy::SecretString;

    use super::WalletApi;
    use crate::permit::PaymentPermit;
    use crate::session::{SessionOptions, WalletSession};
    use tally_core::report::EnvClass;

    /// A syntactically valid connection string built from a fresh throwaway
    /// key, returned with the pubkey it names. Parsing it opens no socket; no
    /// test here connects.
    fn dummy_session() -> (WalletSession, String) {
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        let secret = keys.secret_key().to_secret_hex();
        let connection = SecretString::from(format!(
            "nostr+walletconnect://{pubkey}?secret={secret}&relay=wss%3A%2F%2Frelay.example.com"
        ));
        let session = WalletSession::with_options(&connection, SessionOptions::default()).unwrap();

        (session, pubkey)
    }

    /// Builds a signed invoice offline. No network is used.
    fn test_invoice() -> SecretString {
        let private_key = SecretKey::from_slice(&[7u8; 32]).unwrap();
        let payment_hash = sha256::Hash::from_slice(&[9u8; 32]).unwrap();
        let payment_secret = lightning_invoice::PaymentSecret([11u8; 32]);

        let invoice = InvoiceBuilder::new(Currency::Signet)
            .description(String::from("tally wallet api test"))
            .payment_hash(payment_hash)
            .payment_secret(payment_secret)
            .amount_milli_satoshis(21_000)
            .min_final_cltv_expiry_delta(144)
            .expiry_time(Duration::from_secs(3_600))
            .timestamp(std::time::SystemTime::now())
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
            .unwrap();

        SecretString::from(invoice.to_string())
    }

    #[test]
    fn session_implements_the_trait_without_opening_a_socket() {
        let (session, pubkey) = dummy_session();

        assert_eq!(WalletApi::service_pubkey_hex(&session), pubkey);
        assert_eq!(WalletApi::relay_hosts(&session), vec!["relay.example.com"]);
    }

    #[test]
    fn the_permit_is_still_the_only_way_to_reach_pay_invoice() {
        let invoice = test_invoice();
        let permit = PaymentPermit::issue(&invoice, Some(21), EnvClass::Sandbox, None).unwrap();

        assert_eq!(permit.amount_msat(), 21_000);
        assert_eq!(permit.env_class(), EnvClass::Sandbox);
    }

    /// The trait is usable as a generic bound, which is the only way the
    /// runner takes a wallet.
    fn accepts_generic<W: WalletApi>(wallet: &W) -> usize {
        WalletApi::relay_hosts(wallet).len()
    }

    #[test]
    fn the_trait_works_as_a_generic_bound() {
        let (session, _) = dummy_session();
        assert_eq!(accepts_generic(&session), 1);
    }
}
