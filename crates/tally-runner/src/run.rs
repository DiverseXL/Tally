//! One run of one profile: orchestration, statuses, evidence and the report.
//!
//! Both wallets are generic parameters over [`WalletApi`], never trait objects.
//! The real fixture, the real guard, the real permit and the real preimage check
//! are used; only a test may pass a fake wallet.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use secrecy::{ExposeSecret, SecretString};
use tally_core::capabilities::{
    Advertisement, Capabilities, CapabilitySource, InfoEventCapabilities,
};
use tally_core::outcome::{Observation, Outcome, classify_wallet_outcome, reason};
use tally_core::report::{
    ChallengeEvidence, EndpointClass, EnvClass, Environment, InvoiceEvidence, NwcClientIdentity,
    PaymentEvidence, PaymentOutcome, ProtectedResourceEvidence, RoleCapabilities, RunReport,
    SCHEMA_VERSION, TestResult, WalletRole, WalletSummary, WorkflowEvidence,
};
use tally_core::status::Status;
use tally_core::time::{new_run_id, now_rfc3339};
use tally_l402::challenge::Challenge;
use tally_l402::client::{ClientErrorKind, L402Client};
use tally_l402::fixture::{self, FixtureConfig, FixtureHandle, NwcInvoiceSource};
use tally_l402::invoice::{self, InvoiceFacts};
use tally_l402::macaroon;
use tally_nwc::{
    EncryptionMode, LIVE_ACKNOWLEDGEMENT, NwcError, NwcErrorKind, PaymentPermit, WalletApi,
    select_encryption_mode, verify_preimage_sha256,
};

use crate::config::RunConfig;
use crate::guard;
use crate::profile::{Profile, TestSpec};
use crate::render;

/// Version printed in every report.
pub const TALLY_VERSION: &str = "0.1.0";

/// The limitation every sandbox run carries.
pub const SANDBOX_LIMITATION: &str = "Sandbox wallets from the NWC faucet: balances are play money on a single Alby Hub with no Lightning channels; payments use the real NWC protocol and return a real preimage but are not routed across the real Lightning network.";

/// The limitation every l402-e2e run carries.
pub const FIXTURE_LIMITATION: &str = "The protected resource is served by a local test fixture started by Tally on 127.0.0.1; this proves Tally's flow, not compatibility with third-party L402 services.";

/// Which half of a step an observer is hearing about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPhase {
    /// The step is about to run.
    Started,
    /// The step produced its result.
    Finished,
}

/// One step boundary, as reported to a [`StepObserver`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepEvent {
    /// The test id from the profile.
    pub id: &'static str,
    /// Started or finished.
    pub phase: StepPhase,
    /// The status once the step finished; none while it runs.
    pub status: Option<Status>,
    /// The reason code once the step finished; none while it runs.
    pub reason: Option<String>,
}

/// Receives every step as it starts and finishes.
///
/// The runner stays generic over the observer, never `dyn`, and the default
/// [`NoopObserver`] keeps `run` callers unchanged.
pub trait StepObserver {
    /// Called once when a step starts and once when it finishes.
    fn step(&mut self, event: &StepEvent);
}

impl<F> StepObserver for F
where
    F: FnMut(&StepEvent),
{
    fn step(&mut self, event: &StepEvent) {
        self(event);
    }
}

/// An observer that ignores every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopObserver;

impl StepObserver for NoopObserver {
    fn step(&mut self, _event: &StepEvent) {}
}

/// Why a run could not produce a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    /// A report directory or file could not be written.
    ReportWrite,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReportWrite => f.write_str("report could not be written"),
        }
    }
}

impl std::error::Error for RunError {}

/// Everything one finished run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// The profile that ran.
    pub profile: Profile,
    /// The report, the single value both renderers read.
    pub report: RunReport,
    /// `reports/<run_id>/report.json`.
    pub json_path: PathBuf,
    /// `reports/<run_id>/report.html`.
    pub html_path: PathBuf,
    /// `reports/latest/report.json`.
    pub latest_json: PathBuf,
    /// `reports/latest/report.html`.
    pub latest_html: PathBuf,
    /// How many guard files an explicit clear removed.
    pub guard_cleared: usize,
}

/// Runs one profile against two generic wallets and writes both reports.
///
/// # Errors
///
/// [`RunError::ReportWrite`] when the reports cannot be written.
pub async fn run<P, R>(
    profile: Profile,
    payer: &mut P,
    receiver: Arc<R>,
    config: &RunConfig,
) -> Result<RunOutcome, RunError>
where
    P: WalletApi + Send + Sync,
    R: WalletApi + Send + Sync + 'static,
{
    run_with_observer(profile, payer, receiver, config, &mut NoopObserver).await
}

/// Runs one profile and reports every step boundary to `observer`.
///
/// # Errors
///
/// [`RunError::ReportWrite`] when the reports cannot be written.
pub async fn run_with_observer<P, R, O>(
    profile: Profile,
    payer: &mut P,
    receiver: Arc<R>,
    config: &RunConfig,
    observer: &mut O,
) -> Result<RunOutcome, RunError>
where
    P: WalletApi + Send + Sync,
    R: WalletApi + Send + Sync + 'static,
    O: StepObserver + Send,
{
    let started = Instant::now();
    let started_at = now_rfc3339();
    let run_id = config.run_id.clone().unwrap_or_else(new_run_id);

    let mut ctx = Ctx::new(profile, config, payer, receiver, observer);
    let mut fixture = None;

    if profile == Profile::L402E2e {
        ctx.run_l402(&mut fixture).await;
    } else {
        ctx.run_basic().await;
    }

    // The fixture is shut down on every path, including a failed one.
    if let Some(handle) = fixture {
        handle.shutdown().await;
    }

    let guard_cleared = ctx.guard_cleared;
    let report = ctx.into_report(
        &run_id,
        &started_at,
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
    let (json_path, html_path, latest_json, latest_html) =
        render::write_reports(&report, &config.out_dir).map_err(|_| RunError::ReportWrite)?;

    Ok(RunOutcome {
        profile,
        report,
        json_path,
        html_path,
        latest_json,
        latest_html,
        guard_cleared,
    })
}

struct Ctx<'a, P, R, O> {
    profile: Profile,
    config: &'a RunConfig,
    payer: &'a mut P,
    receiver: Arc<R>,
    observer: &'a mut O,
    results: Vec<TestResult>,
    payer_caps: Option<Capabilities>,
    receiver_caps: Option<Capabilities>,
    payer_network: Option<String>,
    payer_encryption_mode: Option<EncryptionMode>,
    receiver_encryption_mode: Option<EncryptionMode>,
    challenge: Option<Challenge>,
    facts: Option<InvoiceFacts>,
    paid_preimage: Option<SecretString>,
    fixture_base: Option<String>,
    balances_before: (Option<u64>, Option<u64>),
    evidence: WorkflowEvidence,
    guard_cleared: usize,
}

impl<'a, P, R, O> Ctx<'a, P, R, O>
where
    P: WalletApi + Send + Sync,
    R: WalletApi + Send + Sync + 'static,
    O: StepObserver,
{
    fn new(
        profile: Profile,
        config: &'a RunConfig,
        payer: &'a mut P,
        receiver: Arc<R>,
        observer: &'a mut O,
    ) -> Self {
        Self {
            profile,
            config,
            payer,
            receiver,
            observer,
            results: Vec::new(),
            payer_caps: None,
            receiver_caps: None,
            payer_network: None,
            payer_encryption_mode: None,
            receiver_encryption_mode: None,
            challenge: None,
            facts: None,
            paid_preimage: None,
            fixture_base: None,
            balances_before: (None, None),
            evidence: WorkflowEvidence::empty(),
            guard_cleared: 0,
        }
    }

    fn spec(&self, id: &str) -> &'static TestSpec {
        self.profile
            .spec(id)
            .unwrap_or_else(|| panic!("profile {} has no test {id}", self.profile))
    }

    fn step_started(&mut self, id: &'static str) {
        tracing::info!(event = "step_started", id, "step started");

        self.observer.step(&StepEvent {
            id,
            phase: StepPhase::Started,
            status: None,
            reason: None,
        });
    }

    fn push(&mut self, id: &str, outcome: Outcome, expected: &str, actual: String) {
        tracing::info!(
            event = "step_finished",
            id,
            status = outcome.status().as_str(),
            "step finished"
        );

        let spec = self.spec(id);

        self.observer.step(&StepEvent {
            id: spec.id,
            phase: StepPhase::Finished,
            status: Some(outcome.status()),
            reason: Some(String::from(outcome.reason_code())),
        });
        let mut result = TestResult::new(spec.id, spec.name, outcome.status(), spec.required)
            .with_detail(expected, actual)
            .with_diagnostic(format!("reason={}", outcome.reason_code()));

        if let Some(capability) = spec.capability {
            result = result.with_required_capability(capability);
        }

        self.results.push(result);
    }

    fn push_diagnostics(&mut self, id: &str, diagnostics: &[String]) {
        if let Some(result) = self.results.iter_mut().find(|result| result.id == id) {
            result.diagnostics.extend(diagnostics.iter().cloned());
        }
    }

    fn status_of(&self, id: &str) -> Option<Status> {
        self.results
            .iter()
            .find(|result| result.id == id)
            .map(|result| result.status)
    }

    fn passed(&self, id: &str) -> bool {
        self.status_of(id) == Some(Status::Pass)
    }

    /// False when any required test before `id` did not pass.
    fn prerequisites_ok(&self, id: &str) -> bool {
        let Some(position) = self.profile.tests().iter().position(|spec| spec.id == id) else {
            return false;
        };

        self.profile.tests()[..position]
            .iter()
            .filter(|spec| spec.required)
            .all(|spec| self.passed(spec.id))
    }

    fn blocked_on_prerequisite(&mut self, id: &str, expected: &str) {
        let outcome = classify_wallet_outcome(Observation::PrerequisiteFailed);
        self.push(
            id,
            outcome,
            expected,
            String::from("not run: an earlier required test did not pass"),
        );
    }

    async fn run_basic(&mut self) {
        self.step_connect().await;
        self.step_info_event().await;
        self.step_get_info().await;
        self.step_get_balance().await;
        self.step_make_invoice().await;
        self.step_lookup_basic().await;
        self.step_sign_message();
    }

    async fn run_l402(&mut self, fixture: &mut Option<FixtureHandle>) {
        self.step_connect().await;
        self.step_info_event().await;
        self.step_get_info().await;
        self.step_get_balance().await;
        self.step_challenge(fixture).await;
        self.step_invoice_validation();
        self.step_challenge_binding();
        self.step_pay_invoice().await;
        self.step_preimage();
        self.step_protected_resource().await;
        self.step_negative_wrong_proof().await;
        self.step_negative_no_proof().await;
        self.step_lookup_l402().await;
        self.step_sign_message();
    }

    async fn step_connect(&mut self) {
        self.step_started("connect");

        let (payer, receiver) = tokio::join!(self.payer.connect(), self.receiver.connect());

        let payer_relays = payer.as_ref().map_or(0, |report| report.relays_connected);
        let receiver_relays = receiver
            .as_ref()
            .map_or(0, |report| report.relays_connected);

        if payer_relays >= 1 && receiver_relays >= 1 {
            self.push(
                "connect",
                classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                "both wallets reach at least one relay",
                String::from("both wallets connected"),
            );
            let diagnostics = vec![
                format!("payer_relays_connected={payer_relays}"),
                format!("receiver_relays_connected={receiver_relays}"),
            ];
            self.push_diagnostics("connect", &diagnostics);
            return;
        }

        let actual = match (&payer, &receiver) {
            (Err(error), _) | (_, Err(error)) => {
                format!("transport problem: {}", error.kind().as_str())
            }
            _ => String::from("a wallet reached no relay"),
        };
        self.push(
            "connect",
            classify_wallet_outcome(Observation::TransportProblem),
            "both wallets reach at least one relay",
            actual,
        );
    }

    async fn step_info_event(&mut self) {
        self.step_started("info_event");

        if !self.prerequisites_ok("info_event") {
            self.blocked_on_prerequisite("info_event", "the payer publishes an info event");
            self.payer_caps = Some(Capabilities::new(InfoEventCapabilities::missing(), None));
            self.receiver_caps = Some(Capabilities::new(InfoEventCapabilities::missing(), None));
            return;
        }

        let (payer, receiver) = tokio::join!(
            self.payer.fetch_info_event(),
            self.receiver.fetch_info_event()
        );

        if let Err(error) = &payer {
            self.push(
                "info_event",
                error.classify(),
                "the payer publishes an info event",
                format!("fetch failed: {}", error.kind().as_str()),
            );
            return;
        }

        let payer_record = payer.ok().flatten();
        let receiver_record = receiver.ok().flatten();

        let payer_info = payer_record
            .as_ref()
            .map_or_else(InfoEventCapabilities::missing, |record| {
                record.capabilities()
            });
        let receiver_info = receiver_record
            .as_ref()
            .map_or_else(InfoEventCapabilities::missing, |record| {
                record.capabilities()
            });

        self.payer_caps = Some(Capabilities::new(payer_info.clone(), None));
        self.receiver_caps = Some(Capabilities::new(receiver_info.clone(), None));

        if payer_record.is_none() {
            self.push(
                "info_event",
                classify_wallet_outcome(Observation::InfoEventMissing),
                "the payer publishes an info event",
                String::from("no info event arrived in time"),
            );
            return;
        }

        self.push(
            "info_event",
            classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
            "the payer publishes an info event",
            String::from("info event received"),
        );

        // The cipher is picked from the payer's info event and passed to the
        // wallet explicitly, so the report can name the one actually used.
        let payer_mode = select_encryption_mode(&payer_info.encryption_tokens);
        let applied = self.payer.set_encryption_mode(payer_mode).await;
        if applied.is_ok() {
            self.payer_encryption_mode = Some(payer_mode);
        }

        // The same rule picks the receiver's cipher, and it is applied the same
        // way. This runs before the fixture clones the receiver, so the handle
        // is unique here; if it ever is not, the selected mode is still
        // recorded and the failure to apply it is named in the diagnostics.
        let receiver_mode = select_encryption_mode(&receiver_info.encryption_tokens);
        let receiver_applied = match Arc::get_mut(&mut self.receiver) {
            Some(session) => session.set_encryption_mode(receiver_mode).await,
            None => Err(NwcError::new(
                NwcErrorKind::Transport,
                "receiver_session_shared",
            )),
        };
        self.receiver_encryption_mode = Some(receiver_mode);

        let mut diagnostics = vec![
            format!("content_kind={}", payer_info.content_kind().as_str()),
            format!("receiver_info_event_found={}", receiver_info.found),
            format!(
                "payer_encryption_mode={}",
                self.payer_encryption_mode
                    .map_or("not_applied", EncryptionMode::as_str)
            ),
            format!(
                "receiver_encryption_mode={}",
                self.receiver_encryption_mode
                    .map_or("not_applied", EncryptionMode::as_str)
            ),
        ];
        if let Err(error) = applied {
            diagnostics.push(format!("encryption_apply_error={}", error.kind().as_str()));
        }
        if let Err(error) = receiver_applied {
            diagnostics.push(format!(
                "receiver_encryption_apply_error={}",
                error.kind().as_str()
            ));
        }
        self.push_diagnostics("info_event", &diagnostics);
    }

    async fn step_get_info(&mut self) {
        self.step_started("get_info");

        if !self.prerequisites_ok("get_info") {
            self.blocked_on_prerequisite("get_info", "both wallets answer get_info");
            return;
        }

        let (payer, receiver) = tokio::join!(self.payer.get_info(), self.receiver.get_info());

        match (&payer, &receiver) {
            (Ok(payer_info), Ok(receiver_info)) => {
                self.payer_network = payer_info.network.clone();

                if let Some(caps) = self.payer_caps.take() {
                    self.payer_caps = Some(Capabilities::new(
                        caps.info_event,
                        Some(payer_info.capabilities()),
                    ));
                }
                if let Some(caps) = self.receiver_caps.take() {
                    self.receiver_caps = Some(Capabilities::new(
                        caps.info_event,
                        Some(receiver_info.capabilities()),
                    ));
                }

                self.push(
                    "get_info",
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    "both wallets answer get_info",
                    String::from("both wallets answered"),
                );
                let diagnostics = vec![
                    format!(
                        "payer_network={}",
                        payer_info.network.as_deref().unwrap_or("unknown")
                    ),
                    format!("payer_methods={}", payer_info.methods.len()),
                    format!("receiver_methods={}", receiver_info.methods.len()),
                ];
                self.push_diagnostics("get_info", &diagnostics);
            }
            (Err(error), _) | (_, Err(error)) => {
                self.push(
                    "get_info",
                    error.classify(),
                    "both wallets answer get_info",
                    format!("wallet answered: {}", error.kind().as_str()),
                );
            }
        }
    }

    async fn step_get_balance(&mut self) {
        self.step_started("get_balance");

        if !self.prerequisites_ok("get_balance") {
            self.blocked_on_prerequisite("get_balance", "both wallets report a balance");
            return;
        }

        let (payer, receiver) = tokio::join!(self.payer.get_balance(), self.receiver.get_balance());

        match (&payer, &receiver) {
            (Ok(payer_balance), Ok(receiver_balance)) => {
                self.balances_before = (Some(*payer_balance), Some(*receiver_balance));
                self.push(
                    "get_balance",
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    "both wallets report a balance",
                    String::from("both balances read"),
                );
                let diagnostics = vec![
                    format!("payer_balance_msat={payer_balance}"),
                    format!("receiver_balance_msat={receiver_balance}"),
                ];
                self.push_diagnostics("get_balance", &diagnostics);
            }
            (Err(error), _) | (_, Err(error)) => {
                self.push(
                    "get_balance",
                    error.classify(),
                    "both wallets report a balance",
                    format!("wallet answered: {}", error.kind().as_str()),
                );
            }
        }
    }

    async fn step_make_invoice(&mut self) {
        self.step_started("make_invoice");

        if !self.prerequisites_ok("make_invoice") {
            self.blocked_on_prerequisite("make_invoice", "the receiver issues an invoice");
            return;
        }

        match self
            .receiver
            .make_invoice(
                self.config.amount_msat,
                fixture::INVOICE_DESCRIPTION,
                self.config.invoice_expiry_seconds,
            )
            .await
        {
            Ok(made) => {
                self.evidence.invoice = Some(InvoiceEvidence {
                    network: Some(made.network().to_owned()),
                    amount_msat: made.amount_msat(),
                    payment_hash: made.payment_hash_hex().map(str::to_owned),
                    seconds_until_expiry: made.seconds_until_expiry(),
                });
                self.push(
                    "make_invoice",
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    "the receiver issues an invoice",
                    String::from("invoice issued"),
                );
                let diagnostics = vec![
                    format!("amount_msat={}", made.amount_msat()),
                    format!("wallet_hash_matches={:?}", made.wallet_hash_matches()),
                ];
                self.push_diagnostics("make_invoice", &diagnostics);
            }
            Err(error) => {
                self.push(
                    "make_invoice",
                    error.classify(),
                    "the receiver issues an invoice",
                    format!("wallet answered: {}", error.kind().as_str()),
                );
            }
        }
    }

    async fn step_lookup_basic(&mut self) {
        self.step_started("lookup_invoice");

        if !self.prerequisites_ok("lookup_invoice") {
            self.blocked_on_prerequisite("lookup_invoice", "the invoice is found as pending");
            return;
        }

        let Some(payment_hash) = self.invoice_payment_hash() else {
            self.blocked_on_prerequisite("lookup_invoice", "the invoice is found as pending");
            return;
        };

        self.lookup("lookup_invoice", &payment_hash, "pending")
            .await;
    }

    async fn lookup(&mut self, id: &str, payment_hash: &str, expected_state: &str) {
        let expected = format!("the invoice is found as {expected_state}");

        match self.receiver.lookup_invoice(payment_hash).await {
            Ok(lookup) => {
                let state = lookup.state.as_deref().unwrap_or("none");
                let outcome = if state == expected_state {
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved)
                } else {
                    classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved)
                };
                self.push(
                    id,
                    outcome,
                    &expected,
                    format!("wallet reported state {state}"),
                );
            }
            Err(error) => {
                self.push(
                    id,
                    error.classify(),
                    &expected,
                    format!("wallet answered: {}", error.kind().as_str()),
                );
            }
        }
    }

    async fn step_challenge(&mut self, slot: &mut Option<FixtureHandle>) {
        self.step_started("challenge");

        if !self.prerequisites_ok("challenge") {
            self.blocked_on_prerequisite("challenge", "402 with a valid L402 challenge");
            return;
        }

        if let Some(caps) = &self.receiver_caps {
            let advertisement = caps.advertises("make_invoice");
            if caps.effective().source != CapabilitySource::None && !advertisement.is_yes() {
                self.push(
                    "challenge",
                    classify_wallet_outcome(Observation::MethodNotAdvertised),
                    "402 with a valid L402 challenge",
                    String::from("the receiver does not advertise make_invoice"),
                );
                return;
            }
        }

        let source = NwcInvoiceSource::new(Arc::clone(&self.receiver));
        let config = FixtureConfig {
            amount_msat: self.config.amount_msat,
            expiry_seconds: self.config.invoice_expiry_seconds,
        };

        let handle = match fixture::spawn(config, source).await {
            Ok(handle) => handle,
            Err(error) => {
                self.push(
                    "challenge",
                    classify_wallet_outcome(Observation::TransportProblem),
                    "402 with a valid L402 challenge",
                    format!("fixture could not start: {}", error.detail()),
                );
                return;
            }
        };

        let url = handle.protected_url();
        self.fixture_base = Some(url.clone());
        *slot = Some(handle);

        let client = match L402Client::new() {
            Ok(client) => client,
            Err(_) => {
                self.push(
                    "challenge",
                    classify_wallet_outcome(Observation::TransportProblem),
                    "402 with a valid L402 challenge",
                    String::from("http client could not be built"),
                );
                return;
            }
        };

        match client.fetch_challenge(&url).await {
            Ok(challenge) => {
                let scheme = challenge.scheme().to_owned();
                let macaroon_present = !challenge.macaroon_b64().is_empty();
                let invoice_present = !challenge.invoice().is_empty();
                self.evidence.challenge = Some(ChallengeEvidence {
                    http_status: challenge.http_status(),
                    scheme: scheme.clone(),
                    macaroon_present,
                    invoice_present,
                    binding_checked: false,
                    binding_matches: None,
                });
                self.evidence.protected_resource = Some(ProtectedResourceEvidence {
                    unauthenticated_status: challenge.http_status(),
                    wrong_proof_status: None,
                    valid_proof_status: None,
                    body_matches_expected: None,
                });
                self.challenge = Some(challenge);

                self.push(
                    "challenge",
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    "402 with a valid L402 challenge",
                    String::from("402 with a parsed challenge"),
                );
                let diagnostics = vec![
                    format!("scheme={scheme}"),
                    format!("macaroon_present={macaroon_present}"),
                    format!("invoice_present={invoice_present}"),
                ];
                self.push_diagnostics("challenge", &diagnostics);
            }
            Err(error) => {
                let outcome = match error.kind() {
                    ClientErrorKind::Transport => Observation::TransportProblem,
                    _ => Observation::ExpectedBehaviourNotObserved,
                };
                self.push(
                    "challenge",
                    classify_wallet_outcome(outcome),
                    "402 with a valid L402 challenge",
                    format!("client error: {}", error.kind().as_str()),
                );
            }
        }
    }

    fn step_invoice_validation(&mut self) {
        self.step_started("invoice_validation");

        let expected = "the invoice parses, matches the network and amount, and is not expired";

        if !self.prerequisites_ok("invoice_validation") {
            self.blocked_on_prerequisite("invoice_validation", expected);
            return;
        }

        let Some(challenge) = &self.challenge else {
            self.blocked_on_prerequisite("invoice_validation", expected);
            return;
        };

        let facts = match invoice::parse(challenge.invoice()) {
            Ok(facts) => facts,
            Err(error) => {
                self.push(
                    "invoice_validation",
                    classify_wallet_outcome(Observation::InputRefused),
                    expected,
                    format!("invoice refused: {}", error.kind().as_str()),
                );
                return;
            }
        };

        let network = self.payer_network.clone().unwrap_or_default();
        let check = if network.is_empty() {
            Err(invoice::InvoiceErrorKind::UnknownWalletNetwork)
        } else {
            facts
                .check(&network, self.config.amount_msat)
                .map_err(|error| error.kind())
        };

        let seconds_until_expiry = facts.seconds_until_expiry;
        let payment_hash = hex(&facts.payment_hash);
        let network_name = facts.network.to_owned();
        let amount_msat = facts.amount_msat.unwrap_or(0);
        self.facts = Some(facts);

        match check {
            Ok(()) => {
                self.evidence.invoice = Some(InvoiceEvidence {
                    network: Some(network_name),
                    amount_msat,
                    payment_hash: Some(payment_hash),
                    seconds_until_expiry,
                });
                self.push(
                    "invoice_validation",
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("invoice valid"),
                );
                let diagnostics = vec![
                    format!("amount_msat={amount_msat}"),
                    format!("seconds_until_expiry={seconds_until_expiry}"),
                ];
                self.push_diagnostics("invoice_validation", &diagnostics);
            }
            Err(kind) => {
                self.push(
                    "invoice_validation",
                    classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved),
                    expected,
                    format!("invoice refused: {}", kind.as_str()),
                );
            }
        }
    }

    fn step_challenge_binding(&mut self) {
        self.step_started("challenge_binding");

        let expected = "the macaroon binds the invoice payment hash";

        if !self.prerequisites_ok("challenge_binding") {
            self.blocked_on_prerequisite("challenge_binding", expected);
            return;
        }

        let (Some(challenge), Some(facts)) = (&self.challenge, &self.facts) else {
            self.blocked_on_prerequisite("challenge_binding", expected);
            return;
        };

        let macaroon_b64 = challenge.macaroon_b64().to_owned();
        let expected_hash = facts.payment_hash;
        let matches = macaroon::identifier_payment_hash(&macaroon_b64)
            .is_ok_and(|hash| hash == expected_hash);

        if let Some(evidence) = &mut self.evidence.challenge {
            evidence.binding_checked = true;
            evidence.binding_matches = Some(matches);
        }

        let outcome = if matches {
            classify_wallet_outcome(Observation::ExpectedBehaviourObserved)
        } else {
            classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved)
        };

        self.push(
            "challenge_binding",
            outcome,
            expected,
            format!("binding matches: {matches}"),
        );
    }

    async fn step_pay_invoice(&mut self) {
        self.step_started("pay_invoice");

        let expected = "the invoice is paid once";

        if !self.prerequisites_ok("pay_invoice") {
            self.blocked_on_prerequisite("pay_invoice", expected);
            self.record_payment(PaymentOutcome::NotAttempted, None, false);
            return;
        }

        if self.config.approve_payment_sats.is_none() {
            self.push(
                "pay_invoice",
                classify_wallet_outcome(Observation::PaymentNotApproved),
                expected,
                String::from("no payment approval was given for this run"),
            );
            self.record_payment(PaymentOutcome::NotAttempted, None, false);
            return;
        }

        if self.config.env_class == EnvClass::Live
            && self.config.live_ack.as_deref() != Some(LIVE_ACKNOWLEDGEMENT)
        {
            self.push(
                "pay_invoice",
                classify_wallet_outcome(Observation::LiveNotAcknowledged),
                expected,
                String::from("the live run carries no acknowledgement string"),
            );
            self.record_payment(PaymentOutcome::NotAttempted, None, false);
            return;
        }

        let (Some(challenge), Some(facts)) = (&self.challenge, &self.facts) else {
            self.blocked_on_prerequisite("pay_invoice", expected);
            return;
        };
        let invoice_text = SecretString::from(challenge.invoice().to_owned());
        let payment_hash = hex(&facts.payment_hash);

        let guard_path = match self.prepare_guard(&payment_hash) {
            Ok(path) => path,
            Err(outcome) => {
                self.push(
                    "pay_invoice",
                    outcome,
                    expected,
                    String::from("refused before any payment"),
                );
                self.record_payment(PaymentOutcome::NotAttempted, None, false);
                return;
            }
        };

        self.pay(&invoice_text, guard_path).await;
    }

    fn record_payment(
        &mut self,
        outcome: PaymentOutcome,
        fees_paid_msat: Option<u64>,
        preimage_present: bool,
    ) {
        self.evidence.payment = Some(PaymentEvidence {
            outcome,
            fees_paid_msat,
            preimage_present,
            preimage_matches_payment_hash: None,
        });
    }

    fn prepare_guard(&mut self, payment_hash: &str) -> Result<PathBuf, Outcome> {
        if self.config.clear_guard
            && let Ok(removed) = guard::clear_blocking(&self.config.guard_dir)
        {
            self.guard_cleared = removed;
        }

        if !guard::blocking_files(&self.config.guard_dir)
            .unwrap_or_default()
            .is_empty()
        {
            return Err(classify_wallet_outcome(Observation::RunGuardBlocked));
        }

        guard::begin(&self.config.guard_dir, payment_hash)
            .map_err(|_| classify_wallet_outcome(Observation::RunGuardBlocked))
    }

    async fn pay(&mut self, invoice_text: &SecretString, guard_path: PathBuf) {
        let expected = "the invoice is paid once";

        let permit = match PaymentPermit::issue(
            invoice_text,
            self.config.approve_payment_sats,
            self.config.env_class,
            self.config.live_ack.as_deref(),
        ) {
            Ok(permit) => permit,
            Err(error) => {
                let _ = guard::finish(&guard_path, guard::GuardState::Failed);
                self.push(
                    "pay_invoice",
                    error.classify(),
                    expected,
                    format!("permit refused: {}", error.detail()),
                );
                self.record_payment(PaymentOutcome::Failed, None, false);
                return;
            }
        };

        let amount_msat = permit.amount_msat();
        let result = self.payer.pay_invoice(permit).await;
        let deltas = self.measure_deltas().await;

        match result {
            Ok(paid) => {
                let _ = guard::finish(&guard_path, guard::GuardState::Paid);
                let preimage_present = paid.preimage_present();
                let fees_paid_msat = paid.fees_paid_msat();
                self.paid_preimage = Some(paid.preimage().clone());

                self.push(
                    "pay_invoice",
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("wallet reported the payment as paid"),
                );
                let mut diagnostics = vec![
                    format!("amount_msat={amount_msat}"),
                    format!("fees_paid_msat={fees_paid_msat:?}"),
                    format!("preimage_present={preimage_present}"),
                ];
                diagnostics.extend(deltas);
                self.push_diagnostics("pay_invoice", &diagnostics);

                self.record_payment(PaymentOutcome::Paid, fees_paid_msat, preimage_present);
            }
            Err(error) => {
                let outcome = error.classify();
                let ambiguous = outcome.status() == Status::Blocked;
                let state = if ambiguous {
                    guard::GuardState::Ambiguous
                } else {
                    guard::GuardState::Failed
                };
                let _ = guard::finish(&guard_path, state);

                self.push(
                    "pay_invoice",
                    outcome,
                    expected,
                    format!("wallet answered: {}", error.kind().as_str()),
                );
                self.push_diagnostics("pay_invoice", &deltas);

                self.record_payment(
                    if ambiguous {
                        PaymentOutcome::Ambiguous
                    } else {
                        PaymentOutcome::Failed
                    },
                    None,
                    false,
                );
            }
        }
    }

    async fn measure_deltas(&self) -> Vec<String> {
        let (payer, receiver) = tokio::join!(self.payer.get_balance(), self.receiver.get_balance());

        let payer_delta = match (self.balances_before.0, payer) {
            (Some(before), Ok(after)) => Some(i128::from(after) - i128::from(before)),
            _ => None,
        };
        let receiver_delta = match (self.balances_before.1, receiver) {
            (Some(before), Ok(after)) => Some(i128::from(after) - i128::from(before)),
            _ => None,
        };

        vec![
            format!(
                "payer_balance_delta_msat={}",
                payer_delta.map_or_else(|| String::from("unknown"), |delta| delta.to_string())
            ),
            format!(
                "receiver_balance_delta_msat={}",
                receiver_delta.map_or_else(|| String::from("unknown"), |delta| delta.to_string())
            ),
        ]
    }

    fn step_preimage(&mut self) {
        self.step_started("preimage_verification");

        let expected = "SHA-256 of the preimage equals the invoice payment hash";

        match self.status_of("pay_invoice") {
            Some(Status::Pass) => {}
            Some(Status::NotTested) => {
                self.push(
                    "preimage_verification",
                    classify_wallet_outcome(Observation::PaymentNotApproved),
                    expected,
                    String::from("no payment was approved for this run"),
                );
                return;
            }
            _ => {
                self.blocked_on_prerequisite("preimage_verification", expected);
                return;
            }
        }

        let Some(preimage) = &self.paid_preimage else {
            self.push(
                "preimage_verification",
                classify_wallet_outcome(Observation::PreimageMissing),
                expected,
                String::from("the wallet returned no preimage"),
            );
            return;
        };

        let preimage_value = preimage.expose_secret().to_owned();
        let present = !preimage_value.is_empty();
        let expected_hash = self
            .facts
            .as_ref()
            .map(|facts| hex(&facts.payment_hash))
            .unwrap_or_default();
        let matches = present && verify_preimage_sha256(&preimage_value, &expected_hash);

        if let Some(payment) = &mut self.evidence.payment {
            payment.preimage_present = present;
            payment.preimage_matches_payment_hash = Some(matches);
        }

        if !present {
            self.push(
                "preimage_verification",
                classify_wallet_outcome(Observation::PreimageMissing),
                expected,
                String::from("the wallet returned no preimage"),
            );
        } else if matches {
            self.push(
                "preimage_verification",
                classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                expected,
                String::from("the preimage hashes to the payment hash"),
            );
        } else {
            self.push(
                "preimage_verification",
                classify_wallet_outcome(Observation::PreimageMismatch),
                expected,
                String::from("the preimage does not hash to the payment hash"),
            );
        }
    }

    async fn step_protected_resource(&mut self) {
        self.step_started("protected_resource");

        let expected = "the unlocked body is the expected one";

        match self.status_of("preimage_verification") {
            Some(Status::NotTested) => {
                self.push(
                    "protected_resource",
                    classify_wallet_outcome(Observation::PaymentNotApproved),
                    expected,
                    String::from("no payment was approved for this run"),
                );
                return;
            }
            Some(Status::Blocked) => {
                self.blocked_on_prerequisite("protected_resource", expected);
                return;
            }
            _ => {}
        }

        let (Some(challenge), Some(base), Some(preimage)) = (
            self.challenge.as_ref(),
            self.fixture_base.clone(),
            self.paid_preimage.as_ref(),
        ) else {
            self.blocked_on_prerequisite("protected_resource", expected);
            return;
        };

        let macaroon_b64 = challenge.macaroon_b64().to_owned();
        let preimage_value = preimage.expose_secret().to_owned();
        let preimage_failed = self.status_of("preimage_verification") != Some(Status::Pass);

        // An empty preimage cannot form a credential at all, so there is nothing
        // to send; the resource stays locked and the test keeps its FAIL.
        if preimage_value.is_empty() {
            self.push(
                "protected_resource",
                classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved),
                expected,
                String::from("no preimage to present, the resource stays locked"),
            );
            return;
        }

        let client = match L402Client::new() {
            Ok(client) => client,
            Err(_) => {
                self.push(
                    "protected_resource",
                    classify_wallet_outcome(Observation::TransportProblem),
                    expected,
                    String::from("http client could not be built"),
                );
                return;
            }
        };

        // Sent exactly once. When the preimage check failed this records the
        // server's verdict (a 402) without moving any money, and the test keeps
        // its FAIL.
        match client
            .fetch_with_credentials(&base, &macaroon_b64, &preimage_value)
            .await
        {
            Ok(response) => {
                let body_matches =
                    response.status == 200 && response.body == fixture::RESOURCE_BODY;
                if let Some(evidence) = &mut self.evidence.protected_resource {
                    evidence.valid_proof_status = Some(response.status);
                    evidence.body_matches_expected = Some(body_matches);
                }

                let outcome = if !preimage_failed && body_matches {
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved)
                } else {
                    classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved)
                };

                self.push(
                    "protected_resource",
                    outcome,
                    expected,
                    format!(
                        "status {} body matches {body_matches} preimage_check_passed {}",
                        response.status, !preimage_failed
                    ),
                );
            }
            Err(error) => {
                self.push(
                    "protected_resource",
                    classify_wallet_outcome(Observation::TransportProblem),
                    expected,
                    format!("client error: {}", error.kind().as_str()),
                );
            }
        }
    }

    async fn step_negative_wrong_proof(&mut self) {
        self.step_started("negative_wrong_proof");

        let expected = "a wrong preimage is refused with 402";

        // The negative controls only need a challenge: they run in unapproved
        // runs too.
        if !self.passed("challenge") {
            self.blocked_on_prerequisite("negative_wrong_proof", expected);
            return;
        }

        let (Some(challenge), Some(base)) = (self.challenge.as_ref(), self.fixture_base.clone())
        else {
            self.blocked_on_prerequisite("negative_wrong_proof", expected);
            return;
        };

        let macaroon_b64 = challenge.macaroon_b64().to_owned();
        let wrong_preimage = "bb".repeat(32);

        match L402Client::new() {
            Ok(client) => {
                match client
                    .fetch_with_credentials(&base, &macaroon_b64, &wrong_preimage)
                    .await
                {
                    Ok(response) => {
                        if let Some(evidence) = &mut self.evidence.protected_resource {
                            evidence.wrong_proof_status = Some(response.status);
                        }
                        let outcome = if response.status == 402 {
                            classify_wallet_outcome(Observation::ExpectedBehaviourObserved)
                        } else {
                            classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved)
                        };
                        self.push(
                            "negative_wrong_proof",
                            outcome,
                            expected,
                            format!("status {}", response.status),
                        );
                    }
                    Err(error) => {
                        self.push(
                            "negative_wrong_proof",
                            classify_wallet_outcome(Observation::TransportProblem),
                            expected,
                            format!("client error: {}", error.kind().as_str()),
                        );
                    }
                }
            }
            Err(_) => self.push(
                "negative_wrong_proof",
                classify_wallet_outcome(Observation::TransportProblem),
                expected,
                String::from("http client could not be built"),
            ),
        }
    }

    async fn step_negative_no_proof(&mut self) {
        self.step_started("negative_no_proof");

        let expected = "a macaroon without a preimage is refused with 402";

        if !self.passed("challenge") {
            self.blocked_on_prerequisite("negative_no_proof", expected);
            return;
        }

        let (Some(challenge), Some(base)) = (self.challenge.as_ref(), self.fixture_base.clone())
        else {
            self.blocked_on_prerequisite("negative_no_proof", expected);
            return;
        };

        let macaroon_b64 = challenge.macaroon_b64().to_owned();

        match L402Client::new() {
            Ok(client) => match client.fetch_with_macaroon_only(&base, &macaroon_b64).await {
                Ok(response) => {
                    let outcome = if response.status == 402 {
                        classify_wallet_outcome(Observation::ExpectedBehaviourObserved)
                    } else {
                        classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved)
                    };
                    self.push(
                        "negative_no_proof",
                        outcome,
                        expected,
                        format!("status {}", response.status),
                    );
                }
                Err(error) => {
                    self.push(
                        "negative_no_proof",
                        classify_wallet_outcome(Observation::TransportProblem),
                        expected,
                        format!("client error: {}", error.kind().as_str()),
                    );
                }
            },
            Err(_) => self.push(
                "negative_no_proof",
                classify_wallet_outcome(Observation::TransportProblem),
                expected,
                String::from("http client could not be built"),
            ),
        }
    }

    async fn step_lookup_l402(&mut self) {
        self.step_started("lookup_invoice");

        let expected = "the invoice is found by the receiver";

        if !self.passed("challenge") {
            self.blocked_on_prerequisite("lookup_invoice", expected);
            return;
        }

        let Some(payment_hash) = self.invoice_payment_hash() else {
            self.blocked_on_prerequisite("lookup_invoice", expected);
            return;
        };

        let expected_state = if self.passed("pay_invoice") {
            "settled"
        } else {
            "pending"
        };

        self.lookup("lookup_invoice", &payment_hash, expected_state)
            .await;
    }

    fn invoice_payment_hash(&self) -> Option<String> {
        self.evidence
            .invoice
            .as_ref()
            .and_then(|invoice| invoice.payment_hash.clone())
    }

    fn step_sign_message(&mut self) {
        self.step_started("sign_message");

        let advertisement = self
            .payer_caps
            .as_ref()
            .map_or(Advertisement::Unknown, |caps| {
                caps.advertises("sign_message")
            });

        let (outcome, actual) = if advertisement.is_yes() {
            (
                classify_wallet_outcome(Observation::NotSelected),
                String::from("advertised, but the wallet adapter has no sign_message call yet"),
            )
        } else {
            (
                classify_wallet_outcome(Observation::MethodNotAdvertised),
                String::from("the payer does not advertise sign_message"),
            )
        };

        self.push(
            "sign_message",
            outcome,
            "the payer advertises and answers sign_message",
            actual,
        );
    }

    fn into_report(self, run_id: &str, started_at: &str, duration_ms: u64) -> RunReport {
        let mut capabilities = Vec::new();
        if let Some(caps) = &self.payer_caps {
            capabilities.push(RoleCapabilities::from_capabilities(WalletRole::Payer, caps));
        }
        if let Some(caps) = &self.receiver_caps {
            capabilities.push(RoleCapabilities::from_capabilities(
                WalletRole::Receiver,
                caps,
            ));
        }

        let limitations = limitations(
            self.config,
            self.profile,
            self.payer_caps.as_ref(),
            self.receiver_caps.as_ref(),
        );

        let mut results = self.results;

        // Any profile test that never got a result is BLOCKED, never silently
        // dropped.
        for spec in self.profile.tests() {
            if !results.iter().any(|result| result.id == spec.id) {
                results.push(
                    TestResult::new(spec.id, spec.name, Status::Blocked, spec.required)
                        .with_detail("the test runs", "not run")
                        .with_diagnostic(format!("reason={}", reason::PREREQUISITE_FAILED)),
                );
            }
        }

        RunReport {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.to_owned(),
            started_at: started_at.to_owned(),
            finished_at: now_rfc3339(),
            duration_ms,
            tally_version: String::from(TALLY_VERSION),
            environment: Environment {
                class: self.config.env_class,
                endpoint_class: if self.profile == Profile::L402E2e {
                    EndpointClass::LocalFixture
                } else {
                    EndpointClass::None
                },
                nwc_client: NwcClientIdentity::new("nwc", tally_nwc::NWC_CLIENT_VERSION),
                encryption_mode_selected: self
                    .payer_encryption_mode
                    .map(|mode| String::from(mode.as_str())),
                network_reported: self.payer_network,
            },
            wallets: vec![
                WalletSummary {
                    role: WalletRole::Payer,
                    service_pubkey: self.payer.service_pubkey_hex(),
                },
                WalletSummary {
                    role: WalletRole::Receiver,
                    service_pubkey: self.receiver.service_pubkey_hex(),
                },
            ],
            capabilities,
            tests: results,
            workflow_evidence: self.evidence,
            limitations,
        }
    }
}

fn limitations(
    config: &RunConfig,
    profile: Profile,
    payer: Option<&Capabilities>,
    receiver: Option<&Capabilities>,
) -> Vec<String> {
    let mut limitations = Vec::new();

    if config.env_class == EnvClass::Sandbox {
        limitations.push(String::from(SANDBOX_LIMITATION));
    }

    if profile == Profile::L402E2e {
        limitations.push(String::from(FIXTURE_LIMITATION));
    }

    for (role, caps) in [("payer", payer), ("receiver", receiver)] {
        if let Some(caps) = caps
            && caps.effective().source == CapabilitySource::GetInfo
        {
            limitations.push(format!(
                "The effective capability list for the {role} comes from get_info because the wallet's info event was missing or empty."
            ));
        }
    }

    limitations
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}
