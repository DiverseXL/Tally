//! Nostr Wallet Connect adapter for Tally.
//!
//! The adapter is the only crate in the workspace that uses the `nwc` and
//! `nostr` crates; nothing from those SDKs appears in its public API. Payments
//! require a [`PaymentPermit`], which is consumed on use and never retried.

pub mod connection;
pub mod error;
pub mod guard;
pub mod permit;
pub mod session;

pub use error::{NwcError, NwcErrorKind};
pub use permit::{LIVE_ACKNOWLEDGEMENT, PaymentPermit};
pub use session::{
    ConnectReport, EncryptionMode, InfoEventRecord, InvoiceLookup, MadeInvoice, NWC_CLIENT_VERSION,
    PaidInvoice, SessionOptions, WalletInfo, WalletSession, select_encryption_mode,
    verify_preimage_sha256,
};
