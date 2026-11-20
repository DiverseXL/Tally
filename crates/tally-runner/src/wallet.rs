//! The single-wallet inspection profile.
//!
//! One wallet is connected and asked for its info event, `get_info`, balance,
//! an invoice that is never paid, and a lookup of that invoice. Then one test
//! per required method reports whether the wallet advertises it. No fixture
//! runs, no payment is attempted, and the wallet is the only wallet in the
//! report.

use std::time::Instant;

use tally_core::capabilities::{
    Advertisement, Capabilities, CapabilitySource, InfoEventCapabilities,
};
use tally_core::outcome::{Observation, classify_wallet_outcome, reason};
use tally_core::report::{
    EndpointClass, Environment, InvoiceEvidence, NwcClientIdentity, RoleCapabilities, RunReport,
    SCHEMA_VERSION, TestResult, WalletRole, WalletSummary, WorkflowEvidence,
};
use tally_core::status::Status;
use tally_core::time::{new_run_id, now_rfc3339};
use tally_nwc::{EncryptionMode, WalletApi, select_encryption_mode};

use crate::config::RunConfig;
use crate::profile::{Profile, TestSpec};
use crate::render;
use crate::run::{
    NoopObserver, RunError, RunOutcome, StepEvent, StepObserver, StepPhase, TALLY_VERSION,
};

/// The limitation every single-wallet run carries.
pub const NO_PAYMENT_LIMITATION: &str =
    "No payment was made. Capability checks marked advertised-only were not exercised.";

/// The diagnostic every capability check carries.
pub const ADVERTISED_ONLY: &str = "advertised only, not exercised";

/// Sanitizes an operator label to `[a-z0-9-]`, at most 24 characters.
///
/// Returns `None` when nothing usable is left, so an empty label is stored as
/// absent rather than as an empty string.
#[must_use]
pub fn sanitize_label(raw: &str) -> Option<String> {
    let mut out = String::new();

    for character in raw.chars() {
        let mapped = character.to_ascii_lowercase();
        let kept = if mapped.is_ascii_lowercase() || mapped.is_ascii_digit() || mapped == '-' {
            mapped
        } else {
            '-'
        };

        // A run of unusable characters becomes one dash, not one each.
        if kept == '-' && out.ends_with('-') {
            continue;
        }

        if out.len() >= 24 {
            break;
        }

        out.push(kept);
    }

    let trimmed = out.trim_matches('-').to_owned();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Runs the wallet profile against one wallet and writes both reports.
///
/// # Errors
///
/// [`RunError::ReportWrite`] when the reports cannot be written.
pub async fn run_wallet<W>(wallet: &mut W, config: &RunConfig) -> Result<RunOutcome, RunError>
where
    W: WalletApi + Send + Sync,
{
    run_wallet_with_observer(wallet, config, &mut NoopObserver).await
}

/// Runs the wallet profile and reports every step boundary to `observer`.
///
/// # Errors
///
/// [`RunError::ReportWrite`] when the reports cannot be written.
pub async fn run_wallet_with_observer<W, O>(
    wallet: &mut W,
    config: &RunConfig,
    observer: &mut O,
) -> Result<RunOutcome, RunError>
where
    W: WalletApi + Send + Sync,
    O: StepObserver + Send,
{
    let started = Instant::now();
    let started_at = now_rfc3339();
    let run_id = config.run_id.clone().unwrap_or_else(new_run_id);

    let mut ctx = WalletCtx::new(config, wallet, observer);
    ctx.run().await;

    let report = ctx.into_report(
        &run_id,
        &started_at,
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    );
    let (json_path, html_path, latest_json, latest_html) =
        render::write_reports(&report, &config.out_dir).map_err(|_| RunError::ReportWrite)?;

    Ok(RunOutcome {
        profile: Profile::Wallet,
        report,
        json_path,
        html_path,
        latest_json,
        latest_html,
        guard_cleared: 0,
    })
}

struct WalletCtx<'a, W, O> {
    config: &'a RunConfig,
    wallet: &'a mut W,
    observer: &'a mut O,
    results: Vec<TestResult>,
    caps: Option<Capabilities>,
    network: Option<String>,
    encryption: Option<EncryptionMode>,
    invoice_hash: Option<String>,
    evidence: WorkflowEvidence,
}

impl<'a, W, O> WalletCtx<'a, W, O>
where
    W: WalletApi + Send + Sync,
    O: StepObserver,
{
    fn new(config: &'a RunConfig, wallet: &'a mut W, observer: &'a mut O) -> Self {
        Self {
            config,
            wallet,
            observer,
            results: Vec::new(),
            caps: None,
            network: None,
            encryption: None,
            invoice_hash: None,
            evidence: WorkflowEvidence::empty(),
        }
    }

    async fn run(&mut self) {
        self.step_connect().await;
        self.step_info_event().await;
        self.step_get_info().await;
        self.step_get_balance().await;
        self.step_make_invoice().await;
        self.step_lookup_invoice().await;
        self.step_capabilities();
    }

    fn spec(&self, id: &str) -> &'static TestSpec {
        Profile::Wallet
            .spec(id)
            .unwrap_or_else(|| panic!("the wallet profile has no test {id}"))
    }

    fn step_started(&mut self, id: &str) {
        tracing::info!(event = "step_started", id, "step started");

        self.observer.step(&StepEvent {
            id: id.to_owned(),
            phase: StepPhase::Started,
            status: None,
            reason: None,
        });
    }

    fn push_result(
        &mut self,
        id: &str,
        name: &str,
        outcome: tally_core::outcome::Outcome,
        expected: String,
        actual: String,
        required_capability: Option<String>,
    ) {
        tracing::info!(
            event = "step_finished",
            id,
            status = outcome.status().as_str(),
            "step finished"
        );

        self.observer.step(&StepEvent {
            id: id.to_owned(),
            phase: StepPhase::Finished,
            status: Some(outcome.status()),
            reason: Some(String::from(outcome.reason_code())),
        });

        let mut result = TestResult::new(id, name, outcome.status(), true)
            .with_detail(expected, actual)
            .with_diagnostic(format!("reason={}", outcome.reason_code()));

        if let Some(capability) = required_capability {
            result = result.with_required_capability(capability);
        }

        self.results.push(result);
    }

    fn push_diagnostics(&mut self, id: &str, diagnostics: &[String]) {
        if let Some(result) = self.results.iter_mut().find(|result| result.id == id) {
            result.diagnostics.extend(diagnostics.iter().cloned());
        }
    }

    async fn step_connect(&mut self) {
        self.step_started("connect");

        let expected = String::from("the wallet reaches at least one relay");

        match self.wallet.connect().await {
            Ok(report) if report.relays_connected >= 1 => {
                let relays = report.relays_connected;
                self.push_result(
                    "connect",
                    self.spec("connect").name,
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("the wallet connected"),
                    None,
                );
                self.push_diagnostics("connect", &[format!("relays_connected={relays}")]);
            }
            Ok(_) => {
                self.push_result(
                    "connect",
                    self.spec("connect").name,
                    classify_wallet_outcome(Observation::TransportProblem),
                    expected,
                    String::from("the wallet reached no relay"),
                    None,
                );
            }
            Err(error) => {
                self.push_result(
                    "connect",
                    self.spec("connect").name,
                    error.classify(),
                    expected,
                    format!("transport problem: {}", error.kind().as_str()),
                    None,
                );
            }
        }
    }

    async fn step_info_event(&mut self) {
        self.step_started("info_event");

        let expected = String::from("the wallet publishes an info event");

        match self.wallet.fetch_info_event().await {
            Err(error) => {
                self.caps = Some(Capabilities::new(InfoEventCapabilities::missing(), None));
                self.push_result(
                    "info_event",
                    self.spec("info_event").name,
                    error.classify(),
                    expected,
                    format!("fetch failed: {}", error.kind().as_str()),
                    None,
                );
            }
            Ok(None) => {
                self.caps = Some(Capabilities::new(InfoEventCapabilities::missing(), None));
                self.push_result(
                    "info_event",
                    self.spec("info_event").name,
                    classify_wallet_outcome(Observation::InfoEventMissing),
                    expected,
                    String::from("no info event arrived in time"),
                    None,
                );
            }
            Ok(Some(record)) => {
                let info = record.capabilities();
                let mode = select_encryption_mode(&info.encryption_tokens);
                let applied = self.wallet.set_encryption_mode(mode).await;
                if applied.is_ok() {
                    self.encryption = Some(mode);
                }

                let mut diagnostics = vec![
                    format!("content_kind={}", info.content_kind().as_str()),
                    format!(
                        "encryption_mode={}",
                        self.encryption
                            .map_or("not_applied", EncryptionMode::as_str)
                    ),
                ];
                if let Err(error) = applied {
                    diagnostics.push(format!("encryption_apply_error={}", error.kind().as_str()));
                }

                self.caps = Some(Capabilities::new(info, None));
                self.push_result(
                    "info_event",
                    self.spec("info_event").name,
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("info event received"),
                    None,
                );
                self.push_diagnostics("info_event", &diagnostics);
            }
        }
    }

    async fn step_get_info(&mut self) {
        self.step_started("get_info");

        let expected = String::from("the wallet answers get_info");

        match self.wallet.get_info().await {
            Ok(info) => {
                let methods = info.methods.len();
                self.network = info.network.clone();
                let get_info = Some(info.capabilities());

                if let Some(caps) = self.caps.take() {
                    self.caps = Some(Capabilities::new(caps.info_event, get_info));
                }

                self.push_result(
                    "get_info",
                    self.spec("get_info").name,
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("the wallet answered"),
                    self.spec("get_info").capability.map(str::to_owned),
                );
                self.push_diagnostics(
                    "get_info",
                    &[
                        format!("network={}", self.network.as_deref().unwrap_or("unknown")),
                        format!("methods={methods}"),
                    ],
                );
            }
            Err(error) => {
                self.push_result(
                    "get_info",
                    self.spec("get_info").name,
                    error.classify(),
                    expected,
                    format!("wallet answered: {}", error.kind().as_str()),
                    self.spec("get_info").capability.map(str::to_owned),
                );
            }
        }
    }

    async fn step_get_balance(&mut self) {
        self.step_started("get_balance");

        let expected = String::from("the wallet reports a balance");

        match self.wallet.get_balance().await {
            Ok(balance) => {
                self.push_result(
                    "get_balance",
                    self.spec("get_balance").name,
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("the balance was read"),
                    self.spec("get_balance").capability.map(str::to_owned),
                );
                self.push_diagnostics("get_balance", &[format!("balance_msat={balance}")]);
            }
            Err(error) => {
                self.push_result(
                    "get_balance",
                    self.spec("get_balance").name,
                    error.classify(),
                    expected,
                    format!("wallet answered: {}", error.kind().as_str()),
                    self.spec("get_balance").capability.map(str::to_owned),
                );
            }
        }
    }

    async fn step_make_invoice(&mut self) {
        self.step_started("make_invoice");

        let expected = String::from("the wallet issues an invoice without paying it");

        match self
            .wallet
            .make_invoice(
                self.config.amount_msat,
                "Tally wallet inspection: this invoice is never paid",
                self.config.invoice_expiry_seconds,
            )
            .await
        {
            Ok(made) => {
                let payment_hash = made.payment_hash_hex().map(str::to_owned);
                let wallet_hash_matches = made.wallet_hash_matches();
                self.invoice_hash = payment_hash.clone();

                self.evidence.invoice = Some(InvoiceEvidence {
                    network: Some(made.network().to_owned()),
                    amount_msat: made.amount_msat(),
                    payment_hash,
                    seconds_until_expiry: made.seconds_until_expiry(),
                });

                self.push_result(
                    "make_invoice",
                    self.spec("make_invoice").name,
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                    expected,
                    String::from("invoice issued"),
                    self.spec("make_invoice").capability.map(str::to_owned),
                );
                self.push_diagnostics(
                    "make_invoice",
                    &[
                        format!("amount_msat={}", made.amount_msat()),
                        format!("expiry_seconds={}", self.config.invoice_expiry_seconds),
                        format!("payment_hash_from_invoice={}", self.invoice_hash.is_some()),
                        format!("wallet_hash_matches={wallet_hash_matches:?}"),
                    ],
                );
            }
            Err(error) => {
                self.push_result(
                    "make_invoice",
                    self.spec("make_invoice").name,
                    error.classify(),
                    expected,
                    format!("wallet answered: {}", error.kind().as_str()),
                    self.spec("make_invoice").capability.map(str::to_owned),
                );
            }
        }
    }

    async fn step_lookup_invoice(&mut self) {
        self.step_started("lookup_invoice");

        let expected = String::from("the new invoice is found as pending");

        let Some(payment_hash) = self.invoice_hash.clone() else {
            self.push_result(
                "lookup_invoice",
                self.spec("lookup_invoice").name,
                classify_wallet_outcome(Observation::PrerequisiteFailed),
                expected,
                String::from("not run: no invoice was issued"),
                self.spec("lookup_invoice").capability.map(str::to_owned),
            );
            return;
        };

        match self.wallet.lookup_invoice(&payment_hash).await {
            Ok(lookup) => {
                let state = lookup.state.as_deref().unwrap_or("none");
                let outcome = if state == "pending" {
                    classify_wallet_outcome(Observation::ExpectedBehaviourObserved)
                } else {
                    classify_wallet_outcome(Observation::ExpectedBehaviourNotObserved)
                };
                self.push_result(
                    "lookup_invoice",
                    self.spec("lookup_invoice").name,
                    outcome,
                    expected,
                    format!("wallet reported state {state}"),
                    self.spec("lookup_invoice").capability.map(str::to_owned),
                );
            }
            Err(error) => {
                self.push_result(
                    "lookup_invoice",
                    self.spec("lookup_invoice").name,
                    error.classify(),
                    expected,
                    format!("wallet answered: {}", error.kind().as_str()),
                    self.spec("lookup_invoice").capability.map(str::to_owned),
                );
            }
        }
    }

    fn step_capabilities(&mut self) {
        let methods = self.config.wallet_required.clone();

        for method in methods {
            let id = format!("capability_{method}");
            self.step_started(&id);

            let expected = format!("the wallet advertises {method}");
            let name = format!("Advertises {method}");

            let advertisement = self
                .caps
                .as_ref()
                .map_or(Advertisement::Unknown, |caps| caps.advertises(&method));

            match advertisement {
                Advertisement::Yes(source) => {
                    self.push_result(
                        &id,
                        &name,
                        classify_wallet_outcome(Observation::ExpectedBehaviourObserved),
                        expected,
                        String::from(ADVERTISED_ONLY),
                        Some(method.clone()),
                    );
                    self.push_diagnostics(
                        &id,
                        &[
                            String::from(ADVERTISED_ONLY),
                            format!("capability_source={}", source.as_str()),
                        ],
                    );
                }
                Advertisement::No(source) => {
                    self.push_result(
                        &id,
                        &name,
                        classify_wallet_outcome(Observation::MethodNotAdvertised),
                        expected,
                        format!("not advertised by {}", source.as_str()),
                        Some(method.clone()),
                    );
                    self.push_diagnostics(&id, &[format!("capability_source={}", source.as_str())]);
                }
                Advertisement::Unknown => {
                    self.push_result(
                        &id,
                        &name,
                        classify_wallet_outcome(Observation::PrerequisiteFailed),
                        expected,
                        String::from("not checked: neither capability source named a method"),
                        Some(method.clone()),
                    );
                }
            }
        }
    }

    fn into_report(self, run_id: &str, started_at: &str, duration_ms: u64) -> RunReport {
        let capabilities = self
            .caps
            .as_ref()
            .map(|caps| {
                vec![RoleCapabilities::from_capabilities(
                    WalletRole::Wallet,
                    caps,
                )]
            })
            .unwrap_or_default();

        let source = self.caps.as_ref().map(|caps| caps.effective().source);
        let mut limitations = vec![String::from(NO_PAYMENT_LIMITATION)];
        if source == Some(CapabilitySource::GetInfo) {
            limitations.push(String::from(
                "The effective capability list comes from get_info because the wallet's info event was missing or empty.",
            ));
        }

        let mut results = self.results;
        for spec in Profile::Wallet.tests() {
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
                endpoint_class: EndpointClass::None,
                nwc_client: NwcClientIdentity::new("nwc", tally_nwc::NWC_CLIENT_VERSION),
                encryption_mode_selected: self.encryption.map(|mode| String::from(mode.as_str())),
                network_reported: self.network,
                wallet_label: self.config.wallet_label.clone(),
            },
            wallets: vec![WalletSummary {
                role: WalletRole::Wallet,
                service_pubkey: self.wallet.service_pubkey_hex(),
            }],
            capabilities,
            tests: results,
            workflow_evidence: self.evidence,
            limitations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_label;

    #[test]
    fn labels_keep_only_lowercase_letters_digits_and_dashes() {
        assert_eq!(
            sanitize_label("Alby Hub / Faucet_1"),
            Some(String::from("alby-hub-faucet-1"))
        );
        assert_eq!(sanitize_label("--ok--"), Some(String::from("ok")));
        assert_eq!(sanitize_label("!!!"), None);
        assert_eq!(sanitize_label(""), None);
    }

    #[test]
    fn labels_are_at_most_twenty_four_characters() {
        let label = sanitize_label("abcdefghijklmnopqrstuvwxyz0123456789").expect("label");

        assert_eq!(label.len(), 24);
        assert_eq!(label, "abcdefghijklmnopqrstuvwx");
    }
}
