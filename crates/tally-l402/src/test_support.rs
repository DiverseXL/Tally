//! TEST ONLY: offline invoice generator and simulated payer for tests.
//!
//! TEST ONLY: this module is compiled only with the off-by-default
//! `test-support` cargo feature, which only a dev-dependency turns on. It signs
//! real BOLT11 invoices with the `bitcoin` crate and keeps the matching
//! preimage so a test can act as the payer. No runtime path (CLI, server,
//! example) can reach it, and it is not part of a normal build of this crate.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bitcoin::hashes::Hash;
use bitcoin::hashes::sha256;
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use secrecy::SecretString;
use sha2::{Digest, Sha256};

use crate::fixture::{InvoiceSource, InvoiceSourceError, SourcedInvoice};

/// TEST ONLY: one invoice and the preimage that unlocks it.
#[derive(Clone, Debug)]
pub struct TestInvoice {
    /// TEST ONLY: the invoice string.
    pub invoice: String,
    /// TEST ONLY: the preimage, hex, 32 bytes.
    pub preimage_hex: String,
    /// TEST ONLY: the payment hash, 32 bytes.
    pub payment_hash: [u8; 32],
}

/// TEST ONLY: counter that makes every generated preimage different.
static COUNTER: AtomicU64 = AtomicU64::new(1);

/// TEST ONLY: signs a real BOLT11 invoice offline.
///
/// TEST ONLY: the preimage is derived from a counter, so each call returns a
/// distinct invoice and preimage. Nothing here touches the network.
#[must_use]
pub fn make_test_invoice(amount_msat: u64, expiry_seconds: u64) -> TestInvoice {
    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);

    let mut preimage = [0u8; 32];
    preimage[..8].copy_from_slice(&counter.to_le_bytes());
    preimage[8..].copy_from_slice(&[0x5au8; 24]);

    let digest = Sha256::digest(preimage);
    let mut payment_hash = [0u8; 32];
    payment_hash.copy_from_slice(digest.as_ref());

    sign_invoice(
        &preimage,
        payment_hash,
        amount_msat,
        expiry_seconds,
        counter,
    )
}

/// TEST ONLY: signs an invoice for an exact payment hash and expiry.
///
/// TEST ONLY: used when a test needs an invoice that is already expired, or one
/// whose amount differs from the configured amount. Nothing touches the network.
#[must_use]
pub fn sign_test_invoice(
    preimage_hex: &str,
    payment_hash: [u8; 32],
    amount_msat: u64,
    expiry_seconds: u64,
) -> TestInvoice {
    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);

    let mut preimage = [0u8; 32];
    for (index, byte) in preimage.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&preimage_hex[index * 2..index * 2 + 2], 16).unwrap_or(0);
    }

    sign_invoice(
        &preimage,
        payment_hash,
        amount_msat,
        expiry_seconds,
        counter,
    )
}

fn sign_invoice(
    preimage: &[u8; 32],
    payment_hash: [u8; 32],
    amount_msat: u64,
    expiry_seconds: u64,
    counter: u64,
) -> TestInvoice {
    let private_key = SecretKey::from_slice(&[3u8; 32]).expect("static key");
    let payment_hash_hash = sha256::Hash::from_slice(&payment_hash).expect("32 bytes");
    let payment_secret = PaymentSecret([counter as u8; 32]);

    let invoice = InvoiceBuilder::new(Currency::Signet)
        .description(String::from("Tally TEST ONLY fixture invoice"))
        .payment_hash(payment_hash_hash)
        .payment_secret(payment_secret)
        .amount_milli_satoshis(amount_msat)
        .min_final_cltv_expiry_delta(144)
        .expiry_time(Duration::from_secs(expiry_seconds))
        .timestamp(SystemTime::now())
        .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &private_key))
        .expect("signable invoice")
        .to_string();

    TestInvoice {
        invoice,
        preimage_hex: preimage
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        payment_hash,
    }
}

/// TEST ONLY: an invoice source that issues offline test invoices and remembers
/// their preimages, so a test can fetch the preimage for a challenge.
#[derive(Clone, Default)]
pub struct TestInvoiceSource {
    issued: Arc<Mutex<HashMap<[u8; 32], TestInvoice>>>,
}

impl TestInvoiceSource {
    /// TEST ONLY: an empty source.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// TEST ONLY: the preimage for a payment hash this source issued.
    #[must_use]
    pub fn preimage_for(&self, payment_hash: [u8; 32]) -> Option<String> {
        let issued = self.issued.lock().expect("test source lock");
        issued
            .get(&payment_hash)
            .map(|invoice| invoice.preimage_hex.clone())
    }

    /// TEST ONLY: the invoice for a payment hash this source issued.
    #[must_use]
    pub fn invoice_for(&self, payment_hash: [u8; 32]) -> Option<TestInvoice> {
        let issued = self.issued.lock().expect("test source lock");
        issued.get(&payment_hash).cloned()
    }
}

impl InvoiceSource for TestInvoiceSource {
    fn make_invoice(
        &self,
        amount_msat: u64,
        expiry_seconds: u64,
    ) -> impl Future<Output = Result<SourcedInvoice, InvoiceSourceError>> + Send {
        let issued = Arc::clone(&self.issued);

        async move {
            let invoice = make_test_invoice(amount_msat, expiry_seconds);
            let payment_hash = invoice.payment_hash;
            let sourced =
                SourcedInvoice::new(SecretString::from(invoice.invoice.clone()), payment_hash);

            issued
                .lock()
                .expect("test source lock")
                .insert(payment_hash, invoice);

            Ok(sourced)
        }
    }
}
