//! Server configuration, wallet loading and the facts the dashboard shows.
//!
//! Connection strings are read the same way the CLI reads them: the
//! `TALLY_PAYER_NWC_URI` and `TALLY_RECEIVER_NWC_URI` environment variables win
//! over the dotenv files. No flag accepts a connection string, and nothing here
//! prints or serializes one.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use secrecy::SecretString;
use serde::Serialize;
use tally_nwc::WalletSession;

/// Port the server binds unless `--port` says otherwise.
pub const DEFAULT_PORT: u16 = 8787;

/// Size of the sandbox payment the dashboard may approve, in millisatoshis.
pub const PAYMENT_AMOUNT_MSAT: u64 = tally_runner::DEFAULT_AMOUNT_MSAT;

/// Everything the server process needs that is not a wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Port to bind on 127.0.0.1; 0 asks the operating system for one.
    pub port: u16,
    /// Dotenv file of the paying wallet.
    pub payer_file: PathBuf,
    /// Dotenv file of the receiving wallet.
    pub receiver_file: PathBuf,
    /// Where reports are written.
    pub out_dir: PathBuf,
    /// Where run-guard state files live.
    pub guard_dir: PathBuf,
    /// Amount asked of the receiver, in millisatoshis.
    pub amount_msat: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            payer_file: PathBuf::from(".env.payer"),
            receiver_file: PathBuf::from(".env.receiver"),
            out_dir: PathBuf::from("reports"),
            guard_dir: PathBuf::from("runs"),
            amount_msat: PAYMENT_AMOUNT_MSAT,
        }
    }
}

/// One resolved connection string and the name of where it came from.
///
/// The source is safe to print: it is `env` or `file <path>`. The secret is
/// reachable only through [`SecretRef::secret`] and never through `Debug`.
#[derive(Clone)]
pub struct SecretRef {
    secret: Arc<SecretString>,
    source: String,
}

impl SecretRef {
    /// Wraps a value and the name of its source.
    #[must_use]
    pub fn new(value: String, source: impl Into<String>) -> Self {
        Self {
            secret: Arc::new(SecretString::from(value)),
            source: source.into(),
        }
    }

    /// The connection string itself.
    #[must_use]
    pub fn secret(&self) -> &SecretString {
        &self.secret
    }

    /// `env` or `file <path>`.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl std::fmt::Debug for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretRef")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// The two wallets the server runs with, if they are configured.
#[derive(Clone, Default)]
pub struct WalletSet {
    /// The paying wallet.
    pub payer: Option<SecretRef>,
    /// The receiving wallet.
    pub receiver: Option<SecretRef>,
}

impl WalletSet {
    /// Builds a set from already resolved wallets.
    #[must_use]
    pub fn new(payer: Option<SecretRef>, receiver: Option<SecretRef>) -> Self {
        Self { payer, receiver }
    }
}

/// Reads one connection string: the environment first, then the dotenv file.
fn resolve(variable: &str, file: &Path, key: &str) -> Option<SecretRef> {
    if let Some(value) = env::var_os(variable) {
        let value = value.to_string_lossy().trim().to_owned();
        if !value.is_empty() {
            return Some(SecretRef::new(value, "env"));
        }
    }

    let text = std::fs::read_to_string(file).ok()?;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim().to_owned();
        if value.is_empty() {
            return None;
        }
        return Some(SecretRef::new(value, format!("file {}", file.display())));
    }

    None
}

/// Resolves both wallets from the environment and the configured files.
#[must_use]
pub fn load_wallets(config: &ServerConfig) -> WalletSet {
    WalletSet::new(
        resolve(
            "TALLY_PAYER_NWC_URI",
            &config.payer_file,
            "TALLY_PAYER_NWC_URI",
        ),
        resolve(
            "TALLY_RECEIVER_NWC_URI",
            &config.receiver_file,
            "TALLY_RECEIVER_NWC_URI",
        ),
    )
}

/// One wallet as the doctor reports it: where the string came from, and whether
/// it parses. Never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WalletFacts {
    /// `env`, `file <path>` or `none`.
    pub source: String,
    /// Whether the string parses as a connection URI.
    pub parses: bool,
}

/// The facts `tally doctor` prints, as one value the API can serialize.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorFacts {
    /// The paying wallet.
    pub payer: WalletFacts,
    /// The receiving wallet.
    pub receiver: WalletFacts,
    /// Whether the report directory can be created and written.
    pub out_dir_writable: bool,
    /// Whether the guard directory can be created and written.
    pub guard_dir_writable: bool,
    /// How many PAYING or AMBIGUOUS guard files would block a payment.
    pub blocking_guard_files: usize,
}

impl DoctorFacts {
    /// True when both wallets parse and both directories are writable.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.payer.parses
            && self.receiver.parses
            && self.out_dir_writable
            && self.guard_dir_writable
    }
}

fn wallet_facts(wallet: Option<&SecretRef>) -> WalletFacts {
    WalletFacts {
        source: wallet.map_or_else(|| String::from("none"), |wallet| wallet.source().to_owned()),
        parses: wallet.is_some_and(|wallet| WalletSession::new(wallet.secret()).is_ok()),
    }
}

fn dir_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }

    let probe = dir.join(".tally-write-probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Collects the doctor facts for one configuration.
#[must_use]
pub fn doctor(config: &ServerConfig, wallets: &WalletSet) -> DoctorFacts {
    DoctorFacts {
        payer: wallet_facts(wallets.payer.as_ref()),
        receiver: wallet_facts(wallets.receiver.as_ref()),
        out_dir_writable: dir_writable(&config.out_dir),
        guard_dir_writable: dir_writable(&config.guard_dir),
        blocking_guard_files: tally_runner::guard::blocking_files(&config.guard_dir)
            .unwrap_or_default()
            .len(),
    }
}
