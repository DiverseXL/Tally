//! Everything one run needs that is not a wallet.

use std::path::PathBuf;

use tally_core::report::EnvClass;
use tally_nwc::SessionOptions;

/// The configured amount of every run, in millisatoshis.
pub const DEFAULT_AMOUNT_MSAT: u64 = 21_000;

/// How long an invoice issued for the fixture stays valid, in seconds.
pub const DEFAULT_INVOICE_EXPIRY_SECONDS: u64 = 300;

/// The methods a single-wallet inspection requires when the operator names
/// none. `--require` replaces this list, it never adds to it.
pub const DEFAULT_WALLET_REQUIRED: &[&str] = &[
    "get_info",
    "get_balance",
    "make_invoice",
    "lookup_invoice",
    "pay_invoice",
];

/// Configuration of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    /// The run id to use, when the caller must announce it before the run.
    ///
    /// `None` lets the runner mint one. The server sets it so the id it returns
    /// in its 202 answer is the id of the written report.
    pub run_id: Option<String>,
    /// Sandbox or live.
    pub env_class: EnvClass,
    /// Sats the operator approved for this run, if any.
    pub approve_payment_sats: Option<u64>,
    /// The live acknowledgement string, if the operator passed one.
    pub live_ack: Option<String>,
    /// Amount asked of the receiver, in millisatoshis.
    pub amount_msat: u64,
    /// Lifetime of the issued invoice, in seconds.
    pub invoice_expiry_seconds: u64,
    /// Session timeouts handed to the wallet adapter.
    pub timeouts: SessionOptions,
    /// Where reports are written.
    pub out_dir: PathBuf,
    /// Where run-guard state files live.
    pub guard_dir: PathBuf,
    /// Whether stale PAYING and AMBIGUOUS guard files may be cleared.
    pub clear_guard: bool,
    /// Label of a single-wallet inspection, already sanitized.
    pub wallet_label: Option<String>,
    /// Methods a single-wallet inspection requires, verbatim.
    pub wallet_required: Vec<String>,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            run_id: None,
            env_class: EnvClass::Sandbox,
            approve_payment_sats: None,
            live_ack: None,
            amount_msat: DEFAULT_AMOUNT_MSAT,
            invoice_expiry_seconds: DEFAULT_INVOICE_EXPIRY_SECONDS,
            timeouts: SessionOptions::default(),
            out_dir: PathBuf::from("reports"),
            guard_dir: PathBuf::from("runs"),
            clear_guard: false,
            wallet_label: None,
            wallet_required: DEFAULT_WALLET_REQUIRED
                .iter()
                .map(|method| String::from(*method))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_AMOUNT_MSAT, DEFAULT_WALLET_REQUIRED, RunConfig};
    use tally_core::report::EnvClass;

    #[test]
    fn defaults_are_sandbox_twenty_one_sats() {
        let config = RunConfig::default();

        assert_eq!(config.env_class, EnvClass::Sandbox);
        assert_eq!(config.amount_msat, DEFAULT_AMOUNT_MSAT);
        assert_eq!(config.amount_msat, 21_000);
        assert_eq!(config.approve_payment_sats, None);
        assert_eq!(config.out_dir.to_str(), Some("reports"));
        assert_eq!(config.guard_dir.to_str(), Some("runs"));
        assert!(!config.clear_guard);
        assert_eq!(config.wallet_label, None);
        assert_eq!(
            config.wallet_required,
            DEFAULT_WALLET_REQUIRED
                .iter()
                .map(|method| String::from(*method))
                .collect::<Vec<String>>()
        );
    }
}
