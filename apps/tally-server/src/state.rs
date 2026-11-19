//! The state every request shares, and the executor that turns a request into a
//! run.
//!
//! The executor is a trait object because the real one drives the wallet
//! sessions while tests drive the TEST ONLY fake wallets; handlers stay
//! non-generic.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use secrecy::SecretString;
use tally_core::redact::Scrubber;
use tally_nwc::WalletSession;
use tally_runner::{Profile, RunConfig, RunOutcome, run_with_observer};

use crate::config::{DoctorFacts, SecretRef, ServerConfig, WalletSet, doctor};
use crate::events::{EventSink, Runs};

/// One run the server is asked to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// The id the report will carry.
    pub run_id: String,
    /// `basic` or `l402-e2e`.
    pub profile: Profile,
    /// Sats approved for this run; `None` never pays.
    pub approve_payment_sats: Option<u64>,
    /// Whether stale guard files may be cleared.
    pub clear_guard: bool,
}

/// Starts one run and reports every step to the given sink.
pub trait RunExecutor: Send + Sync {
    /// Runs the request to completion.
    fn execute(
        &self,
        request: RunRequest,
        sink: EventSink,
    ) -> Pin<Box<dyn Future<Output = Result<RunOutcome, String>> + Send>>;
}

/// The executor that talks to the two configured wallets.
pub struct RealExecutor {
    config: Arc<ServerConfig>,
    wallets: WalletSet,
}

impl RealExecutor {
    /// Builds the executor for one configuration.
    #[must_use]
    pub fn new(config: Arc<ServerConfig>, wallets: WalletSet) -> Self {
        Self { config, wallets }
    }
}

impl RunExecutor for RealExecutor {
    fn execute(
        &self,
        request: RunRequest,
        sink: EventSink,
    ) -> Pin<Box<dyn Future<Output = Result<RunOutcome, String>> + Send>> {
        let config = RunConfig {
            run_id: Some(request.run_id),
            approve_payment_sats: request.approve_payment_sats,
            clear_guard: request.clear_guard,
            out_dir: self.config.out_dir.clone(),
            guard_dir: self.config.guard_dir.clone(),
            ..RunConfig::default()
        };

        let payer: Option<SecretRef> = self.wallets.payer.clone();
        let receiver: Option<SecretRef> = self.wallets.receiver.clone();

        Box::pin(async move {
            let Some(payer) = payer else {
                return Err(String::from("the payer connection string is missing"));
            };
            let Some(receiver) = receiver else {
                return Err(String::from("the receiver connection string is missing"));
            };

            let mut payer = WalletSession::new(payer.secret())
                .map_err(|_| String::from("the payer connection string does not parse"))?;
            let receiver = WalletSession::new(receiver.secret())
                .map_err(|_| String::from("the receiver connection string does not parse"))?;

            let mut sink = sink;
            run_with_observer(
                request.profile,
                &mut payer,
                Arc::new(receiver),
                &config,
                &mut sink,
            )
            .await
            .map_err(|error| error.to_string())
        })
    }
}

/// Everything the routes read.
#[derive(Clone)]
pub struct AppState {
    /// Ports, directories and the payment amount.
    pub config: Arc<ServerConfig>,
    /// The two wallets, when configured.
    pub wallets: WalletSet,
    /// Redacts known secrets from every response body built here.
    pub scrubber: Arc<Scrubber>,
    /// The runs this process started.
    pub runs: Arc<Runs>,
    /// Turns a request into a run.
    pub executor: Arc<dyn RunExecutor>,
}

impl AppState {
    /// Builds the state, filling the scrubber with both connection strings.
    #[must_use]
    pub fn new(config: ServerConfig, wallets: WalletSet, executor: Arc<dyn RunExecutor>) -> Self {
        let mut scrubber = Scrubber::new();
        for wallet in [wallets.payer.as_ref(), wallets.receiver.as_ref()]
            .into_iter()
            .flatten()
        {
            scrubber.add(SecretString::from(wallet.secret().clone()));
        }

        Self {
            config: Arc::new(config),
            wallets,
            scrubber: Arc::new(scrubber),
            runs: Arc::new(Runs::new()),
            executor,
        }
    }

    /// Returns `text` with any known secret replaced.
    #[must_use]
    pub fn scrub(&self, text: &str) -> String {
        self.scrubber.scrub(text)
    }

    /// The facts `tally doctor` prints, collected for this request.
    #[must_use]
    pub fn doctor(&self) -> DoctorFacts {
        doctor(&self.config, &self.wallets)
    }
}
