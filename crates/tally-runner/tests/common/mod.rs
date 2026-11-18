//! TEST ONLY: a fake wallet pair and helpers for the offline scenarios.
//!
//! TEST ONLY: this module is compiled only into the tally-runner test binary.
//! Only the two wallets are faked. The fixture, the run guard, the payment
//! permit, the preimage check and both report renderers are the real code.
//! Every item here has a doc comment starting with "TEST ONLY".

#![allow(dead_code)]

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use secrecy::SecretString;
use tally_core::WalletErrorCode;
use tally_core::capabilities::content_kind;
use tally_nwc::{
    ConnectReport, InfoEventRecord, InvoiceLookup, MadeInvoice, NwcError, NwcErrorKind,
    PaidInvoice, PaymentPermit, WalletApi, WalletInfo,
};

use tally_l402::test_support::{make_test_invoice, make_test_invoice_aged};

/// TEST ONLY: how the fake receiver issues its invoice.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InvoiceMode {
    /// TEST ONLY: the requested amount and expiry.
    Normal,
    /// TEST ONLY: the requested amount plus one thousand millisatoshis.
    WrongAmount,
    /// TEST ONLY: an invoice that is already expired.
    Expired,
}

/// TEST ONLY: how the fake payer answers pay_invoice.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PayMode {
    /// TEST ONLY: the correct preimage.
    Correct,
    /// TEST ONLY: an empty preimage.
    NoPreimage,
    /// TEST ONLY: a preimage that does not match.
    WrongPreimage,
    /// TEST ONLY: the NIP-47 INSUFFICIENT_BALANCE error.
    InsufficientBalance,
    /// TEST ONLY: no answer at all, which the adapter reports as ambiguous.
    Ambiguous,
}

/// TEST ONLY: settings of the fake pair.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FakeConfig {
    /// TEST ONLY: whether connect returns a transport error.
    pub connect_fails: bool,
    /// TEST ONLY: info-event content of the payer, or None for no info event.
    pub payer_info: Option<&'static str>,
    /// TEST ONLY: info-event content of the receiver, or None for no info event.
    pub receiver_info: Option<&'static str>,
    /// TEST ONLY: get_info method list of the payer.
    pub payer_get_info: &'static str,
    /// TEST ONLY: get_info method list of the receiver.
    pub receiver_get_info: &'static str,
    /// TEST ONLY: the network both wallets report.
    pub network: &'static str,
    /// TEST ONLY: invoice behaviour of the receiver.
    pub invoice: InvoiceMode,
    /// TEST ONLY: payment behaviour of the payer.
    pub pay: PayMode,
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self {
            connect_fails: false,
            payer_info: Some("pay_invoice get_info get_balance make_invoice lookup_invoice"),
            receiver_info: Some("pay_invoice get_info get_balance make_invoice lookup_invoice"),
            payer_get_info: "pay_invoice get_info get_balance make_invoice lookup_invoice",
            receiver_get_info: "pay_invoice get_info get_balance make_invoice lookup_invoice",
            network: "signet",
            invoice: InvoiceMode::Normal,
            pay: PayMode::Correct,
        }
    }
}

/// TEST ONLY: state the two fake wallets share.
pub struct Shared {
    config: FakeConfig,
    preimages: Mutex<HashMap<[u8; 32], String>>,
    paid: Mutex<Vec<[u8; 32]>>,
    drain_pay_calls: AtomicUsize,
    balances_msat: Mutex<(u64, u64)>,
}

/// TEST ONLY: one fake wallet, either the payer or the receiver.
#[derive(Clone)]
pub struct FakeWallet {
    pubkey: String,
    role_payer: bool,
    shared: Arc<Shared>,
}

/// TEST ONLY: the pair of fake wallets.
pub struct FakePair {
    /// TEST ONLY: the paying wallet.
    pub payer: FakeWallet,
    /// TEST ONLY: the receiving wallet.
    pub receiver: FakeWallet,
    /// TEST ONLY: shared state.
    pub shared: Arc<Shared>,
}

impl FakePair {
    /// TEST ONLY: builds a pair from one configuration.
    #[must_use]
    pub fn new(config: FakeConfig) -> Self {
        let shared = Arc::new(Shared {
            config,
            preimages: Mutex::new(HashMap::new()),
            paid: Mutex::new(Vec::new()),
            drain_pay_calls: AtomicUsize::new(0),
            balances_msat: Mutex::new((1_000_000, 0)),
        });

        Self {
            payer: FakeWallet {
                pubkey: "11".repeat(32),
                role_payer: true,
                shared: Arc::clone(&shared),
            },
            receiver: FakeWallet {
                pubkey: "22".repeat(32),
                role_payer: false,
                shared: Arc::clone(&shared),
            },
            shared,
        }
    }

    /// TEST ONLY: how many times the fake payer was asked to pay.
    #[must_use]
    pub fn pay_calls(&self) -> usize {
        self.shared.drain_pay_calls.load(Ordering::SeqCst)
    }
}

impl FakeWallet {
    fn config(&self) -> FakeConfig {
        self.shared.config
    }

    fn info_record(&self, content: &'static str) -> InfoEventRecord {
        let methods = content
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<String>>();

        InfoEventRecord {
            id: String::from("00"),
            created_at: 0,
            tags: Vec::new(),
            raw_content: String::from(content),
            methods,
            encryption_tokens: Vec::new(),
            extensions: Vec::new(),
            content_kind: content_kind(content),
        }
    }

    fn wallet_info(&self, methods: &'static str, network: &'static str) -> WalletInfo {
        WalletInfo {
            alias: Some(String::from("fake-wallet")),
            color: None,
            pubkey: Some(self.pubkey.clone()),
            network: Some(String::from(network)),
            block_height: Some(0),
            block_hash: None,
            methods: methods
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<String>>(),
            notifications: Vec::new(),
            extensions: None,
        }
    }
}

fn hex32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }

    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }

    Some(bytes)
}

impl WalletApi for FakeWallet {
    fn service_pubkey_hex(&self) -> String {
        self.pubkey.clone()
    }

    fn relay_hosts(&self) -> Vec<String> {
        vec![String::from("relay.example.com")]
    }

    fn connect(&self) -> impl Future<Output = Result<ConnectReport, NwcError>> + Send {
        let fails = self.config().connect_fails;

        async move {
            if fails {
                return Err(NwcError::new(
                    NwcErrorKind::Transport,
                    "fake_connect_timeout",
                ));
            }

            Ok(ConnectReport {
                relays_requested: 1,
                relays_connected: 1,
            })
        }
    }

    fn fetch_info_event(
        &self,
    ) -> impl Future<Output = Result<Option<InfoEventRecord>, NwcError>> + Send {
        let content = if self.role_payer {
            self.config().payer_info
        } else {
            self.config().receiver_info
        };
        let record = content.map(|content| self.info_record(content));

        async move { Ok(record) }
    }

    fn get_info(&self) -> impl Future<Output = Result<WalletInfo, NwcError>> + Send {
        let config = self.config();
        let info = if self.role_payer {
            self.wallet_info(config.payer_get_info, config.network)
        } else {
            self.wallet_info(config.receiver_get_info, config.network)
        };

        async move { Ok(info) }
    }

    fn get_balance(&self) -> impl Future<Output = Result<u64, NwcError>> + Send {
        let shared = Arc::clone(&self.shared);
        let payer = self.role_payer;

        async move {
            let balances = shared.balances_msat.lock().expect("fake balances");
            Ok(if payer { balances.0 } else { balances.1 })
        }
    }

    fn make_invoice(
        &self,
        amount_msat: u64,
        _description: &str,
        expiry_seconds: u64,
    ) -> impl Future<Output = Result<MadeInvoice, NwcError>> + Send {
        let shared = Arc::clone(&self.shared);
        let mode = self.config().invoice;

        async move {
            if self.role_payer {
                return Err(NwcError::new(
                    NwcErrorKind::Wallet(WalletErrorCode::NotImplemented),
                    "wallet_error",
                ));
            }

            let test = match mode {
                InvoiceMode::Normal => make_test_invoice(amount_msat, expiry_seconds),
                InvoiceMode::WrongAmount => make_test_invoice(amount_msat + 1_000, expiry_seconds),
                InvoiceMode::Expired => make_test_invoice_aged(amount_msat, 1, 7_200),
            };

            shared
                .preimages
                .lock()
                .expect("fake preimages")
                .insert(test.payment_hash, test.preimage_hex.clone());

            Ok(MadeInvoice::from_parts(
                SecretString::from(test.invoice),
                amount_msat,
                "signet",
                Some(
                    test.payment_hash
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                ),
                None,
                expiry_seconds,
            ))
        }
    }

    fn lookup_invoice(
        &self,
        payment_hash_hex: &str,
    ) -> impl Future<Output = Result<InvoiceLookup, NwcError>> + Send {
        let shared = Arc::clone(&self.shared);
        let hash = hex32(payment_hash_hex);

        async move {
            let settled = hash
                .is_some_and(|hash| shared.paid.lock().expect("fake paid list").contains(&hash));

            Ok(InvoiceLookup {
                state: Some(String::from(if settled { "settled" } else { "pending" })),
                amount_msat: 21_000,
                fees_paid_msat: 0,
                settled_at: None,
            })
        }
    }

    fn pay_invoice(
        &self,
        permit: PaymentPermit,
    ) -> impl Future<Output = Result<PaidInvoice, NwcError>> + Send {
        let shared = Arc::clone(&self.shared);
        let mode = self.config().pay;
        let amount_msat = permit.amount_msat();
        let hash = permit.payment_hash_hex().to_owned();

        async move {
            shared.drain_pay_calls.fetch_add(1, Ordering::SeqCst);

            match mode {
                PayMode::InsufficientBalance => {
                    return Err(NwcError::new(
                        NwcErrorKind::Wallet(WalletErrorCode::InsufficientBalance),
                        "wallet_error",
                    ));
                }
                PayMode::Ambiguous => {
                    return Err(NwcError::new(
                        NwcErrorKind::AmbiguousPayment,
                        "payment_outcome_unknown",
                    ));
                }
                _ => {}
            }

            let preimage = if mode == PayMode::NoPreimage {
                String::new()
            } else if mode == PayMode::WrongPreimage {
                "aa".repeat(32)
            } else {
                let key = hex32(&hash);
                shared
                    .preimages
                    .lock()
                    .expect("fake preimages")
                    .get(&key.unwrap_or([0u8; 32]))
                    .cloned()
                    .unwrap_or_default()
            };

            if let Some(key) = hex32(&hash) {
                shared.paid.lock().expect("fake paid list").push(key);
            }
            {
                let mut balances = shared.balances_msat.lock().expect("fake balances");
                balances.0 = balances.0.saturating_sub(amount_msat);
                balances.1 = balances.1.saturating_add(amount_msat);
            }

            Ok(PaidInvoice::from_preimage(
                SecretString::from(preimage),
                Some(0),
            ))
        }
    }
}
