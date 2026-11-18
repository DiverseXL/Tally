//! The NIP-47 session: one wallet connection through the `nwc` crate.
//!
//! Nothing from `nwc` or `nostr` leaves this crate: every method returns Tally's
//! own types, and no type here prints a connection string, an invoice or a
//! preimage.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use bitcoin::Network;
use lightning_invoice::Bolt11Invoice;
use nostr::nips::nip47::{
    LookupInvoiceRequest, MakeInvoiceRequest, NostrWalletConnectUri, PayInvoiceRequest,
    TransactionState,
};
use nostr::prelude::{Event, Filter, Kind};
use nwc::NostrWalletConnect;
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq;

use tally_core::capabilities::ContentKind;

use crate::error::{NwcError, NwcErrorKind};
use crate::permit::PaymentPermit;

/// Version of the `nwc` crate this adapter is built against.
///
/// Kept in step with the pin in the workspace `Cargo.toml`.
pub const NWC_CLIENT_VERSION: &str = "0.45.2";

/// Default wait for relay connections.
pub const DEFAULT_CONNECT_WAIT: Duration = Duration::from_secs(15);
/// Default upper bound for one request.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Default upper bound for fetching the info event.
pub const DEFAULT_INFO_EVENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Timeouts of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionOptions {
    /// How long to wait for relays to connect.
    pub connect_wait: Duration,
    /// Upper bound for one wallet request.
    pub request_timeout: Duration,
    /// Upper bound for fetching the info event.
    pub info_event_timeout: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            connect_wait: DEFAULT_CONNECT_WAIT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            info_event_timeout: DEFAULT_INFO_EVENT_TIMEOUT,
        }
    }
}

/// One connected wallet.
pub struct WalletSession {
    uri: NostrWalletConnectUri,
    client: NostrWalletConnect,
    options: SessionOptions,
    encryption_mode: Option<EncryptionMode>,
}

/// The NIP-47 encryption mode the client is told to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionMode {
    /// NIP-44 version 2.
    Nip44V2,
    /// The legacy NIP-04.
    Nip04,
}

impl EncryptionMode {
    /// The field value used in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nip44V2 => "nip44_v2",
            Self::Nip04 => "nip04",
        }
    }
}

/// Applies the NIP-47 ciphers rule to the info event's `encryption` tokens.
///
/// `nip44_v2` when the tag lists it; `nip04` otherwise, including when there is
/// no `encryption` tag at all.
#[must_use]
pub fn select_encryption_mode(tokens: &[String]) -> EncryptionMode {
    if tokens.iter().any(|token| token == "nip44_v2") {
        EncryptionMode::Nip44V2
    } else {
        EncryptionMode::Nip04
    }
}

impl WalletSession {
    /// Builds a session from a connection string held in a [`SecretString`].
    ///
    /// The string is borrowed, never copied into a plain `String`, and a parse
    /// failure does not echo it.
    ///
    /// # Errors
    ///
    /// Returns [`NwcErrorKind::InvalidInput`] when the string is not a valid
    /// `nostr+walletconnect://` URI.
    pub fn new(connection: &SecretString) -> Result<Self, NwcError> {
        Self::with_options(connection, SessionOptions::default())
    }

    /// Builds a session with explicit timeouts.
    ///
    /// # Errors
    ///
    /// Same as [`WalletSession::new`].
    pub fn with_options(
        connection: &SecretString,
        options: SessionOptions,
    ) -> Result<Self, NwcError> {
        let uri = NostrWalletConnectUri::parse(connection.expose_secret())
            .map_err(|_| NwcError::new(NwcErrorKind::InvalidInput, "connection_unparseable"))?;

        let client = NostrWalletConnect::builder(uri.clone())
            .timeout(options.request_timeout)
            .build();

        Ok(Self {
            uri,
            client,
            options,
            encryption_mode: None,
        })
    }

    /// Rebuilds the client with an explicit cipher and reconnects.
    ///
    /// The choice is passed to the builder through `force_nip44_v2` or
    /// `force_nip04`, so the mode the client uses is known rather than left to
    /// the library's own detection. Rebuilding drops the previous relay
    /// connections, so this reconnects and returns the new report.
    ///
    /// # Errors
    ///
    /// Returns [`NwcErrorKind::Transport`] when a relay cannot be added.
    pub async fn set_encryption_mode(
        &mut self,
        mode: EncryptionMode,
    ) -> Result<ConnectReport, NwcError> {
        let builder =
            NostrWalletConnect::builder(self.uri.clone()).timeout(self.options.request_timeout);

        self.client = match mode {
            EncryptionMode::Nip44V2 => builder.force_nip44_v2().build(),
            EncryptionMode::Nip04 => builder.force_nip04().build(),
        };
        self.encryption_mode = Some(mode);

        self.connect().await
    }

    /// The cipher the session was told to use, if one was chosen.
    #[must_use]
    pub const fn encryption_mode(&self) -> Option<EncryptionMode> {
        self.encryption_mode
    }

    /// The wallet service public key, hex.
    #[must_use]
    pub fn service_pubkey_hex(&self) -> String {
        self.client.uri().public_key.to_hex()
    }

    /// The relay hosts of this connection, without scheme, path or credentials.
    #[must_use]
    pub fn relay_hosts(&self) -> Vec<String> {
        self.client
            .uri()
            .relays
            .iter()
            .map(|relay| match relay.domain() {
                Some(domain) => domain.to_owned(),
                None => host_only(relay.as_str_without_trailing_slash()),
            })
            .collect()
    }

    /// Connects to every relay of the connection and waits for the handshake.
    ///
    /// # Errors
    ///
    /// Returns [`NwcErrorKind::Transport`] when a relay cannot be added.
    pub async fn connect(&self) -> Result<ConnectReport, NwcError> {
        for relay in self.client.uri().relays.iter() {
            self.client
                .client()
                .add_relay(relay.clone())
                .await
                .map_err(|_| NwcError::new(NwcErrorKind::Transport, "relay_add_failed"))?;
        }

        self.client
            .client()
            .connect()
            .and_wait(self.options.connect_wait)
            .await;

        Ok(ConnectReport {
            relays_requested: self.client.uri().relays.len(),
            relays_connected: self.client.client().relays().await.len(),
        })
    }

    /// Fetches the kind 13194 info event, if the wallet publishes one.
    ///
    /// # Errors
    ///
    /// Returns [`NwcErrorKind::Transport`] when the fetch times out or fails.
    pub async fn fetch_info_event(&self) -> Result<Option<InfoEventRecord>, NwcError> {
        let filter = Filter::new()
            .kind(Kind::WalletConnectInfo)
            .author(self.client.uri().public_key)
            .limit(1);

        let events = self
            .client
            .client()
            .fetch_events(filter)
            .timeout(self.options.info_event_timeout)
            .max_events(1)
            .await
            // The nostr client error type: a fetch failure, i.e. transport.
            .map_err(|_| NwcError::new(NwcErrorKind::Transport, "info_event_fetch_failed"))?;

        Ok(events.into_iter().next().map(InfoEventRecord::from_event))
    }

    /// Sends `get_info`.
    ///
    /// # Errors
    ///
    /// Returns a sanitized [`NwcError`] on transport or protocol failure.
    pub async fn get_info(&self) -> Result<WalletInfo, NwcError> {
        let info = self
            .client
            .get_info()
            .timeout(self.options.request_timeout)
            .await
            .map_err(|error| NwcError::from_nwc(&error))?;

        Ok(WalletInfo {
            alias: info.alias,
            color: info.color,
            pubkey: info.pubkey,
            network: info.network,
            block_height: info.block_height,
            block_hash: info.block_hash,
            methods: info
                .methods
                .iter()
                .map(|method| method.as_str().to_owned())
                .collect(),
            notifications: info.notifications,
            // The nostr 0.45.7 GetInfoResponse has no `extensions` field, so
            // the list is not exposed rather than empty. See docs/STATUS.md.
            extensions: None,
        })
    }

    /// Sends `get_balance` and returns millisatoshis.
    ///
    /// # Errors
    ///
    /// Returns a sanitized [`NwcError`] on transport or protocol failure.
    pub async fn get_balance(&self) -> Result<u64, NwcError> {
        let response = self
            .client
            .get_balance()
            .timeout(self.options.request_timeout)
            .await
            .map_err(|error| NwcError::from_nwc(&error))?;

        Ok(response.balance)
    }

    /// Asks the wallet for an invoice and reads it back.
    ///
    /// # Errors
    ///
    /// Returns [`NwcErrorKind::Protocol`] when the returned string is not a
    /// BOLT11 invoice, and a sanitized error on transport or wallet failure.
    pub async fn make_invoice(
        &self,
        amount_msat: u64,
        description: &str,
        expiry_seconds: u64,
    ) -> Result<MadeInvoice, NwcError> {
        let request = MakeInvoiceRequest {
            amount: amount_msat,
            description: Some(description.to_owned()),
            description_hash: None,
            expiry: Some(expiry_seconds),
        };

        let response = self
            .client
            .make_invoice(request)
            .timeout(self.options.request_timeout)
            .await
            .map_err(|error| NwcError::from_nwc(&error))?;

        let invoice = Bolt11Invoice::from_str(&response.invoice)
            .map_err(|_| NwcError::new(NwcErrorKind::Protocol, "bolt11_invoice_unparseable"))?;

        Ok(MadeInvoice {
            amount_msat: invoice.amount_milli_satoshis().unwrap_or(0),
            network: network_name(invoice.network()),
            payment_hash_hex: Some(invoice.payment_hash().to_string()),
            seconds_until_expiry: invoice.duration_until_expiry().as_secs(),
            wallet_payment_hash_hex: response.payment_hash,
            invoice: SecretString::from(response.invoice),
        })
    }

    /// Sends `lookup_invoice` for a payment hash.
    ///
    /// # Errors
    ///
    /// Returns a sanitized [`NwcError`]; NOT_FOUND stays a wallet error.
    pub async fn lookup_invoice(&self, payment_hash_hex: &str) -> Result<InvoiceLookup, NwcError> {
        let request = LookupInvoiceRequest {
            payment_hash: Some(payment_hash_hex.to_owned()),
            invoice: None,
        };

        let response = self
            .client
            .lookup_invoice(request)
            .timeout(self.options.request_timeout)
            .await
            .map_err(|error| NwcError::from_nwc(&error))?;

        Ok(InvoiceLookup {
            state: response.state.map(state_name).map(str::to_owned),
            amount_msat: response.amount,
            fees_paid_msat: response.fees_paid,
            settled_at: response.settled_at.map(|timestamp| timestamp.as_secs()),
        })
    }

    /// Pays exactly the invoice bound by `permit`, once.
    ///
    /// The permit is consumed. There is no retry: a timeout or any other
    /// ambiguous outcome becomes [`NwcErrorKind::AmbiguousPayment`] and the
    /// caller must stop.
    ///
    /// # Errors
    ///
    /// Returns a wallet error, an ambiguous-payment error or a protocol error.
    pub async fn pay_invoice(&self, permit: PaymentPermit) -> Result<PaidInvoice, NwcError> {
        let request = PayInvoiceRequest::new(permit.invoice_str());

        let response = self
            .client
            .pay_invoice(request)
            .timeout(self.options.request_timeout)
            .await
            .map_err(|error| NwcError::from_nwc_payment(&error))?;

        Ok(PaidInvoice {
            preimage: SecretString::from(response.preimage),
            fees_paid_msat: response.fees_paid,
        })
    }
}

impl fmt::Debug for WalletSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WalletSession")
            .field("service_pubkey_hex", &self.service_pubkey_hex())
            .field("relays", &self.relay_hosts().len())
            .finish()
    }
}

/// Result of [`WalletSession::connect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectReport {
    /// Relays named in the connection string.
    pub relays_requested: usize,
    /// Relays the client reports after the wait.
    pub relays_connected: usize,
}

/// Raw copy of the wallet info event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfoEventRecord {
    /// Event id, hex.
    pub id: String,
    /// Creation timestamp, seconds since the epoch.
    pub created_at: u64,
    /// Every tag, name and values, exactly as received.
    pub tags: Vec<Vec<String>>,
    /// The content string exactly as received.
    pub raw_content: String,
    /// Content split on whitespace.
    pub methods: Vec<String>,
    /// Tokens of every `encryption` tag, split on whitespace.
    pub encryption_tokens: Vec<String>,
    /// Tokens of every `extensions` tag, split on whitespace.
    pub extensions: Vec<String>,
    /// How the content looks.
    pub content_kind: ContentKind,
}

impl InfoEventRecord {
    fn from_event(event: Event) -> Self {
        let tags = event
            .tags
            .iter()
            .map(|tag| tag.as_slice().to_vec())
            .collect::<Vec<Vec<String>>>();

        let mut encryption_tokens = Vec::new();
        let mut extensions = Vec::new();
        for tag in &tags {
            let Some(name) = tag.first() else {
                continue;
            };
            match name.as_str() {
                "encryption" => encryption_tokens.extend(
                    tag.iter()
                        .skip(1)
                        .flat_map(|value| value.split_whitespace())
                        .map(str::to_owned),
                ),
                "extensions" => extensions.extend(
                    tag.iter()
                        .skip(1)
                        .flat_map(|value| value.split_whitespace())
                        .map(str::to_owned),
                ),
                _ => {}
            }
        }

        let methods = event
            .content
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<String>>();

        Self {
            id: event.id.to_hex(),
            created_at: event.created_at.as_secs(),
            tags,
            content_kind: tally_core::capabilities::content_kind(&event.content),
            raw_content: event.content,
            methods,
            encryption_tokens,
            extensions,
        }
    }

    /// The capability record for the report model.
    #[must_use]
    pub fn capabilities(&self) -> tally_core::capabilities::InfoEventCapabilities {
        tally_core::capabilities::InfoEventCapabilities {
            found: true,
            raw_content: self.raw_content.clone(),
            methods: self.methods.clone(),
            encryption_tokens: self.encryption_tokens.clone(),
            extensions: self.extensions.clone(),
        }
    }
}

/// What `get_info` reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletInfo {
    /// Node alias.
    pub alias: Option<String>,
    /// Node color.
    pub color: Option<String>,
    /// Node public key.
    pub pubkey: Option<String>,
    /// Reported network.
    pub network: Option<String>,
    /// Block height.
    pub block_height: Option<u32>,
    /// Block hash.
    pub block_hash: Option<String>,
    /// Methods supported for this connection.
    pub methods: Vec<String>,
    /// Supported notifications.
    pub notifications: Vec<String>,
    /// Extensions; `None` against nostr 0.45.7, whose response has no such
    /// field, so the client library does not expose them at all.
    pub extensions: Option<Vec<String>>,
}

impl WalletInfo {
    /// The `get_info` record for the report model.
    #[must_use]
    pub fn capabilities(&self) -> tally_core::capabilities::GetInfoCapabilities {
        tally_core::capabilities::GetInfoCapabilities {
            methods: self.methods.clone(),
            extensions: self.extensions.clone(),
            notifications: self.notifications.clone(),
            network: self.network.clone(),
            alias: self.alias.clone(),
        }
    }
}

/// An invoice issued by the wallet.
pub struct MadeInvoice {
    invoice: SecretString,
    amount_msat: u64,
    network: &'static str,
    payment_hash_hex: Option<String>,
    wallet_payment_hash_hex: Option<String>,
    seconds_until_expiry: u64,
}

impl MadeInvoice {
    /// The invoice string, held as a secret.
    #[must_use]
    pub fn invoice(&self) -> &SecretString {
        &self.invoice
    }

    /// Hands the invoice string to the caller, for example to scrub it.
    #[must_use]
    pub fn into_invoice(self) -> SecretString {
        self.invoice
    }

    /// Amount in millisatoshis, read back from the invoice.
    #[must_use]
    pub const fn amount_msat(&self) -> u64 {
        self.amount_msat
    }

    /// Network prefix of the invoice.
    #[must_use]
    pub const fn network(&self) -> &'static str {
        self.network
    }

    /// Payment hash read from the invoice, hex.
    #[must_use]
    pub fn payment_hash_hex(&self) -> Option<&str> {
        self.payment_hash_hex.as_deref()
    }

    /// Whether the payment hash the wallet returned equals the invoice's.
    #[must_use]
    pub fn wallet_hash_matches(&self) -> Option<bool> {
        match (&self.wallet_payment_hash_hex, &self.payment_hash_hex) {
            (Some(wallet), Some(invoice)) => Some(wallet.eq_ignore_ascii_case(invoice)),
            _ => None,
        }
    }

    /// Seconds until the invoice expires, at the time it was read.
    #[must_use]
    pub const fn seconds_until_expiry(&self) -> u64 {
        self.seconds_until_expiry
    }
}

impl fmt::Debug for MadeInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MadeInvoice")
            .field("invoice", &"[REDACTED]")
            .field("amount_msat", &self.amount_msat)
            .field("network", &self.network)
            .field("seconds_until_expiry", &self.seconds_until_expiry)
            .finish()
    }
}

/// Result of `lookup_invoice`, without the invoice or preimage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceLookup {
    /// One of `pending`, `settled`, `accepted`, `expired`, `failed`.
    pub state: Option<String>,
    /// Amount in millisatoshis.
    pub amount_msat: u64,
    /// Fees paid in millisatoshis.
    pub fees_paid_msat: u64,
    /// Settlement timestamp, seconds since the epoch.
    pub settled_at: Option<u64>,
}

/// Result of a successful `pay_invoice`.
pub struct PaidInvoice {
    preimage: SecretString,
    fees_paid_msat: Option<u64>,
}

impl PaidInvoice {
    /// Whether the wallet returned a non-empty preimage.
    #[must_use]
    pub fn preimage_present(&self) -> bool {
        !self.preimage.expose_secret().is_empty()
    }

    /// Fees the wallet reported, in millisatoshis.
    #[must_use]
    pub const fn fees_paid_msat(&self) -> Option<u64> {
        self.fees_paid_msat
    }

    /// Checks SHA-256 of the preimage against the invoice payment hash.
    #[must_use]
    pub fn preimage_matches(&self, payment_hash_hex: &str) -> bool {
        verify_preimage_sha256(self.preimage.expose_secret(), payment_hash_hex)
    }
}

impl fmt::Debug for PaidInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaidInvoice")
            .field("preimage", &"[REDACTED]")
            .field("fees_paid_msat", &self.fees_paid_msat)
            .finish()
    }
}

/// True when SHA-256 of the preimage equals the payment hash, compared in
/// constant time.
#[must_use]
pub fn verify_preimage_sha256(preimage_hex: &str, payment_hash_hex: &str) -> bool {
    use sha2::{Digest, Sha256};

    let (Some(preimage), Some(expected)) = (hex_to_32(preimage_hex), hex_to_32(payment_hash_hex))
    else {
        return false;
    };

    let digest = Sha256::digest(preimage);
    let digest_bytes: &[u8] = digest.as_ref();

    bool::from(digest_bytes.ct_eq(expected.as_slice()))
}

fn hex_to_32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }

    let mut bytes = [0u8; 32];
    for (index, chunk) in bytes.iter_mut().enumerate() {
        *chunk = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }

    Some(bytes)
}

fn host_only(url: &str) -> String {
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);

    without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .to_owned()
}

fn network_name(network: Network) -> &'static str {
    match network {
        Network::Bitcoin => "bitcoin",
        Network::Testnet => "testnet",
        Network::Signet => "signet",
        Network::Regtest => "regtest",
        _ => "unknown",
    }
}

fn state_name(state: TransactionState) -> &'static str {
    match state {
        TransactionState::Pending => "pending",
        TransactionState::Settled => "settled",
        TransactionState::Accepted => "accepted",
        TransactionState::Expired => "expired",
        TransactionState::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::{
        EncryptionMode, InvoiceLookup, MadeInvoice, PaidInvoice, SessionOptions, WalletSession,
        host_only, select_encryption_mode, verify_preimage_sha256,
    };
    use crate::error::NwcErrorKind;
    use crate::permit::PaymentPermit;
    use tally_core::report::EnvClass;

    const SENTINEL_INVOICE: &str = "lnbc21sentinelinvoicevalue";

    fn dummy_session() -> WalletSession {
        // A syntactically valid connection string with a throwaway key. Parsing
        // it opens no socket; no test here connects.
        let connection = SecretString::from(String::from(
            "nostr+walletconnect://b889ff5b1513b641e2a139f661a661364979c5beee91842f8f0ef42ab558e9d4?secret=71a8c14c1407c113601079c4302dab36460f0ccd0ad506f1f2dc73b5100e4f3c&relay=wss%3A%2F%2Frelay.example.com",
        ));

        WalletSession::with_options(&connection, SessionOptions::default()).unwrap()
    }

    #[test]
    fn session_reports_pubkey_and_hosts_only() {
        let session = dummy_session();

        assert_eq!(
            session.service_pubkey_hex(),
            "b889ff5b1513b641e2a139f661a661364979c5beee91842f8f0ef42ab558e9d4"
        );
        assert_eq!(session.relay_hosts(), vec!["relay.example.com"]);
    }

    #[test]
    fn session_debug_has_no_secret() {
        let session = dummy_session();
        let debug = format!("{session:?}");

        assert!(
            !debug.contains("71a8c14c1407c113601079c4302dab36460f0ccd0ad506f1f2dc73b5100e4f3c")
        );
        assert!(!debug.contains("nostr+walletconnect"));
        assert!(!debug.contains("secret="));
        assert!(!debug.contains("relay.example.com"));
    }

    #[test]
    fn invalid_connection_string_is_refused_without_echoing_it() {
        let connection = SecretString::from(String::from("not-a-uri-with-a-sentinel-secret"));
        let error = WalletSession::new(&connection).unwrap_err();

        assert_eq!(error.kind(), NwcErrorKind::InvalidInput);
        let text = format!("{error} {error:?}");
        assert!(!text.contains("sentinel"));
    }

    #[test]
    fn preimage_check_matches_sha256_of_the_preimage() {
        use sha2::{Digest, Sha256};

        let preimage = "11".repeat(32);
        let expected = Sha256::digest(hex_to_bytes_for_test(&preimage));
        let expected_hex = expected
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        assert!(verify_preimage_sha256(&preimage, &expected_hex));
        assert!(!verify_preimage_sha256(&preimage, &"00".repeat(32)));
        assert!(!verify_preimage_sha256("not-hex", &expected_hex));
    }

    fn hex_to_bytes_for_test(hex: &str) -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|index| u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn invoice_debug_never_reveals_the_invoice() {
        let invoice = MadeInvoice {
            invoice: SecretString::from(String::from(SENTINEL_INVOICE)),
            amount_msat: 21_000,
            network: "signet",
            payment_hash_hex: Some(String::from("aa")),
            wallet_payment_hash_hex: Some(String::from("aa")),
            seconds_until_expiry: 300,
        };

        let debug = format!("{invoice:?}");
        assert!(!debug.contains(SENTINEL_INVOICE));
        assert!(debug.contains("[REDACTED]"));
        assert_eq!(invoice.wallet_hash_matches(), Some(true));
    }

    #[test]
    fn paid_invoice_debug_never_reveals_the_preimage() {
        let paid = PaidInvoice {
            preimage: SecretString::from(String::from("sentinel-preimage-value")),
            fees_paid_msat: Some(0),
        };

        let debug = format!("{paid:?}");
        assert!(!debug.contains("sentinel-preimage-value"));
        assert!(debug.contains("[REDACTED]"));
        assert!(paid.preimage_present());
    }

    #[test]
    fn lookup_and_permit_debug_are_safe() {
        let lookup = InvoiceLookup {
            state: Some(String::from("pending")),
            amount_msat: 21_000,
            fees_paid_msat: 0,
            settled_at: None,
        };
        assert!(format!("{lookup:?}").contains("pending"));

        let connection = SecretString::from(String::from(
            "nostr+walletconnect://b889ff5b1513b641e2a139f661a661364979c5beee91842f8f0ef42ab558e9d4?secret=71a8c14c1407c113601079c4302dab36460f0ccd0ad506f1f2dc73b5100e4f3c&relay=wss%3A%2F%2Frelay.example.com",
        ));
        assert!(WalletSession::new(&connection).is_ok());

        let _ = PaymentPermit::issue(
            &SecretString::from(String::from(SENTINEL_INVOICE)),
            Some(21),
            EnvClass::Sandbox,
            None,
        );
    }

    #[test]
    fn encryption_mode_follows_the_info_event_tag() {
        assert_eq!(
            select_encryption_mode(&[String::from("nip44_v2"), String::from("nip04")]),
            EncryptionMode::Nip44V2
        );
        assert_eq!(
            select_encryption_mode(&[String::from("nip04")]),
            EncryptionMode::Nip04
        );
        // No encryption tag at all.
        assert_eq!(select_encryption_mode(&[]), EncryptionMode::Nip04);
    }

    #[test]
    fn host_only_strips_scheme_path_and_port_is_kept() {
        assert_eq!(host_only("wss://relay.example.com/"), "relay.example.com");
        assert_eq!(
            host_only("wss://relay.example.com:7000"),
            "relay.example.com:7000"
        );
        assert_eq!(
            host_only("wss://relay.example.com/path?x=1"),
            "relay.example.com"
        );
    }
}
