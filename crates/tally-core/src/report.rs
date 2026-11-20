//! The run report shared by the JSON output and the dashboard.
//!
//! No field of this model can hold an invoice string, a preimage or a
//! connection string; evidence is limited to hashes, amounts and booleans.

use serde::{Deserialize, Serialize};

use crate::capabilities::{
    Capabilities, CapabilitySource, Discrepancy, GetInfoCapabilities, InfoEventCapabilities,
};
use crate::status::Status;

/// Version of the report layout. Bump on any breaking change.
pub const SCHEMA_VERSION: u32 = 1;

/// Exit code when every required test passed.
pub const EXIT_ALL_REQUIRED_PASSED: i32 = 0;
/// Exit code when a required test failed.
pub const EXIT_REQUIRED_FAILED: i32 = 1;
/// Exit code reserved for usage or configuration errors before any test runs.
pub const EXIT_USAGE_OR_CONFIG: i32 = 2;
/// Exit code when a required test was blocked.
pub const EXIT_REQUIRED_BLOCKED: i32 = 3;
/// Exit code when a required test was unsupported.
pub const EXIT_REQUIRED_UNSUPPORTED: i32 = 4;
/// Exit code when a required test was not tested.
pub const EXIT_REQUIRED_NOT_TESTED: i32 = 5;

/// Which kind of money the wallets hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvClass {
    /// Faucet wallets with play money; no real funds.
    #[serde(rename = "sandbox")]
    Sandbox,
    /// Real funds; needs an explicit acknowledgement to pay.
    #[serde(rename = "live")]
    Live,
}

impl EnvClass {
    /// The field value used in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sandbox => "sandbox",
            Self::Live => "live",
        }
    }
}

/// Where the tested endpoint lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointClass {
    /// Another process on this machine.
    #[serde(rename = "local_fixture")]
    LocalFixture,
    /// A third-party sandbox service.
    #[serde(rename = "external_sandbox")]
    ExternalSandbox,
    /// The run tested no protected endpoint at all.
    #[serde(rename = "none")]
    None,
}

impl EndpointClass {
    /// The field value used in reports and console output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalFixture => "local_fixture",
            Self::ExternalSandbox => "external_sandbox",
            Self::None => "none",
        }
    }
}

/// The NWC client library in use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NwcClientIdentity {
    /// Crate name.
    pub name: String,
    /// Crate version.
    pub version: String,
}

impl NwcClientIdentity {
    /// Names the client crate and the version it was built against.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// A wallet that took part in the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletRole {
    /// The wallet that pays.
    #[serde(rename = "payer")]
    Payer,
    /// The wallet that issues the invoice.
    #[serde(rename = "receiver")]
    Receiver,
    /// The only wallet of a single-wallet inspection.
    #[serde(rename = "wallet")]
    Wallet,
}

impl WalletRole {
    /// The field value used in reports and on the command line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Payer => "payer",
            Self::Receiver => "receiver",
            Self::Wallet => "wallet",
        }
    }
}

/// One wallet, identified by role and service public key only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletSummary {
    /// Payer or receiver.
    pub role: WalletRole,
    /// The wallet service public key, hex.
    pub service_pubkey: String,
}

/// Capability data for one wallet, with the effective source and disagreements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleCapabilities {
    /// Payer or receiver.
    pub role: WalletRole,
    /// The raw info event data.
    pub info_event: InfoEventCapabilities,
    /// The `get_info` data, when obtained.
    pub get_info: Option<GetInfoCapabilities>,
    /// Where the governing method list came from.
    pub effective_source: CapabilitySource,
    /// The governing method list.
    pub effective_methods: Vec<String>,
    /// Methods only one source lists.
    pub discrepancies: Vec<Discrepancy>,
}

impl RoleCapabilities {
    /// Builds the report section from the collected capabilities.
    #[must_use]
    pub fn from_capabilities(role: WalletRole, capabilities: &Capabilities) -> Self {
        let effective = capabilities.effective();

        Self {
            role,
            info_event: capabilities.info_event.clone(),
            get_info: capabilities.get_info.clone(),
            effective_source: effective.source,
            effective_methods: effective.methods,
            discrepancies: capabilities.discrepancies(),
        }
    }
}

/// Environment block of the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    /// Sandbox or live.
    pub class: EnvClass,
    /// Where the endpoint lives.
    pub endpoint_class: EndpointClass,
    /// The NWC client crate.
    pub nwc_client: NwcClientIdentity,
    /// The encryption mode the client used, when known.
    pub encryption_mode_selected: Option<String>,
    /// The network the wallet reported, when known.
    pub network_reported: Option<String>,
    /// The operator-supplied label of a single-wallet run, when one was given.
    #[serde(default)]
    pub wallet_label: Option<String>,
}

/// The issued invoice, as evidence only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceEvidence {
    /// The network prefix of the invoice.
    pub network: Option<String>,
    /// Amount in millisatoshis.
    pub amount_msat: u64,
    /// Payment hash, hex.
    pub payment_hash: Option<String>,
    /// Seconds until expiry at the time of the run.
    pub seconds_until_expiry: u64,
}

/// How a payment ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentOutcome {
    /// The wallet returned a preimage.
    #[serde(rename = "paid")]
    Paid,
    /// The wallet reported a failure.
    #[serde(rename = "failed")]
    Failed,
    /// The payment may have happened; it was not retried.
    #[serde(rename = "ambiguous")]
    Ambiguous,
    /// No payment was attempted in this run.
    #[serde(rename = "not_attempted")]
    NotAttempted,
}

/// The payment, as evidence only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentEvidence {
    /// How the payment ended.
    pub outcome: PaymentOutcome,
    /// Fees the wallet reported, in millisatoshis.
    pub fees_paid_msat: Option<u64>,
    /// Whether a preimage was returned at all.
    pub preimage_present: bool,
    /// Whether SHA-256 of the preimage equalled the invoice payment hash.
    pub preimage_matches_payment_hash: Option<bool>,
}

/// The L402 challenge, as evidence only.
///
/// No field here holds a macaroon, an invoice, a preimage or a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeEvidence {
    /// The HTTP status the challenge arrived with.
    pub http_status: u16,
    /// The authentication scheme name, for example `L402`.
    pub scheme: String,
    /// Whether a macaroon was present in the challenge.
    pub macaroon_present: bool,
    /// Whether an invoice was present in the challenge.
    pub invoice_present: bool,
    /// Whether the macaroon was checked against the invoice.
    pub binding_checked: bool,
    /// Whether the macaroon's payment hash equalled the invoice's; null when
    /// the check could not run.
    pub binding_matches: Option<bool>,
}

/// The protected resource, as evidence only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedResourceEvidence {
    /// The status of a request without credentials.
    pub unauthenticated_status: u16,
    /// The status of a request with a wrong proof; null when not tried.
    pub wrong_proof_status: Option<u16>,
    /// The status of a request with valid credentials; null when not tried.
    pub valid_proof_status: Option<u16>,
    /// Whether the unlocked body was the expected one; null when not unlocked.
    pub body_matches_expected: Option<bool>,
}

/// Workflow evidence gathered during the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowEvidence {
    /// The issued invoice.
    pub invoice: Option<InvoiceEvidence>,
    /// The payment.
    pub payment: Option<PaymentEvidence>,
    /// The L402 challenge.
    pub challenge: Option<ChallengeEvidence>,
    /// The protected resource.
    pub protected_resource: Option<ProtectedResourceEvidence>,
}

impl WorkflowEvidence {
    /// Evidence for a run that collected none of it.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            invoice: None,
            payment: None,
            challenge: None,
            protected_resource: None,
        }
    }
}

/// One test and what it observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestResult {
    /// Stable id, for example `pay_invoice`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// The status.
    pub status: Status,
    /// Whether this test decides the exit code.
    pub required: bool,
    /// The capability this test needs, when it needs one.
    pub required_capability: Option<String>,
    /// What the test expected.
    pub expected: String,
    /// What actually happened, sanitized.
    pub actual: String,
    /// Extra sanitized notes, one per entry.
    pub diagnostics: Vec<String>,
}

impl TestResult {
    /// Builds a result.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        status: Status,
        required: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            status,
            required,
            required_capability: None,
            expected: String::new(),
            actual: String::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Sets the required capability.
    #[must_use]
    pub fn with_required_capability(mut self, capability: impl Into<String>) -> Self {
        self.required_capability = Some(capability.into());
        self
    }

    /// Sets the expected and actual text.
    #[must_use]
    pub fn with_detail(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected = expected.into();
        self.actual = actual.into();
        self
    }

    /// Appends one diagnostic line.
    #[must_use]
    pub fn with_diagnostic(mut self, diagnostic: impl Into<String>) -> Self {
        self.diagnostics.push(diagnostic.into());
        self
    }

    /// Appends several diagnostic lines.
    #[must_use]
    pub fn with_diagnostics<I, S>(mut self, diagnostics: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.diagnostics
            .extend(diagnostics.into_iter().map(Into::into));
        self
    }
}

/// The complete report of one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    /// Layout version, always [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Id of this run.
    pub run_id: String,
    /// Start time, RFC 3339 UTC.
    pub started_at: String,
    /// Finish time, RFC 3339 UTC.
    pub finished_at: String,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Version of the Tally workspace.
    pub tally_version: String,
    /// Environment block.
    pub environment: Environment,
    /// Wallets that took part.
    pub wallets: Vec<WalletSummary>,
    /// Capability data, one entry per wallet.
    pub capabilities: Vec<RoleCapabilities>,
    /// Tests in run order.
    pub tests: Vec<TestResult>,
    /// Evidence collected during the run.
    pub workflow_evidence: WorkflowEvidence,
    /// Known limitations of this run.
    pub limitations: Vec<String>,
}

/// The exit code for a finished report.
///
/// `0` when every required test passed; otherwise `1` for FAIL, `4` for
/// UNSUPPORTED, `3` for BLOCKED and `5` for NOT_TESTED, in that order. A
/// definitive "the wallet does not offer this" therefore outranks a test that
/// could not run because of it. Tests that are not required never change the
/// code. `2` is never returned here; it is reserved for usage and configuration
/// errors before any test runs.
#[must_use]
pub fn exit_code(report: &RunReport) -> i32 {
    let statuses = report
        .tests
        .iter()
        .filter(|test| test.required)
        .map(|test| test.status)
        .collect::<Vec<Status>>();

    if statuses.iter().all(|status| status.is_pass()) {
        return EXIT_ALL_REQUIRED_PASSED;
    }

    if statuses.contains(&Status::Fail) {
        EXIT_REQUIRED_FAILED
    } else if statuses.contains(&Status::Unsupported) {
        EXIT_REQUIRED_UNSUPPORTED
    } else if statuses.contains(&Status::Blocked) {
        EXIT_REQUIRED_BLOCKED
    } else if statuses.contains(&Status::NotTested) {
        EXIT_REQUIRED_NOT_TESTED
    } else {
        EXIT_ALL_REQUIRED_PASSED
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChallengeEvidence, EXIT_ALL_REQUIRED_PASSED, EXIT_REQUIRED_BLOCKED, EXIT_REQUIRED_FAILED,
        EXIT_REQUIRED_NOT_TESTED, EXIT_REQUIRED_UNSUPPORTED, EndpointClass, EnvClass, Environment,
        InvoiceEvidence, NwcClientIdentity, PaymentEvidence, PaymentOutcome,
        ProtectedResourceEvidence, RunReport, SCHEMA_VERSION, TestResult, WalletRole,
        WalletSummary, WorkflowEvidence, exit_code,
    };
    use crate::status::Status;

    fn report(statuses: &[(Status, bool)]) -> RunReport {
        RunReport {
            schema_version: SCHEMA_VERSION,
            run_id: String::from("run"),
            started_at: String::from("2026-01-01T00:00:00Z"),
            finished_at: String::from("2026-01-01T00:00:01Z"),
            duration_ms: 1,
            tally_version: String::from("0.1.0"),
            environment: Environment {
                class: EnvClass::Sandbox,
                endpoint_class: EndpointClass::ExternalSandbox,
                nwc_client: NwcClientIdentity {
                    name: String::from("nwc"),
                    version: String::from("0.45.2"),
                },
                encryption_mode_selected: None,
                network_reported: None,
                wallet_label: None,
            },
            wallets: Vec::new(),
            capabilities: Vec::new(),
            tests: statuses
                .iter()
                .enumerate()
                .map(|(index, (status, required))| {
                    TestResult::new(
                        format!("test_{index}"),
                        format!("test {index}"),
                        *status,
                        *required,
                    )
                })
                .collect(),
            workflow_evidence: WorkflowEvidence::empty(),
            limitations: Vec::new(),
        }
    }

    #[test]
    fn exit_code_zero_only_when_all_required_pass() {
        assert_eq!(
            exit_code(&report(&[(Status::Pass, true), (Status::Pass, true)])),
            EXIT_ALL_REQUIRED_PASSED
        );
        assert_eq!(
            exit_code(&report(&[
                (Status::Fail, false),
                (Status::Blocked, false),
                (Status::Pass, true)
            ])),
            EXIT_ALL_REQUIRED_PASSED
        );
    }

    #[test]
    fn exit_code_for_each_required_status() {
        assert_eq!(
            exit_code(&report(&[(Status::Fail, true)])),
            EXIT_REQUIRED_FAILED
        );
        assert_eq!(
            exit_code(&report(&[(Status::Blocked, true)])),
            EXIT_REQUIRED_BLOCKED
        );
        assert_eq!(
            exit_code(&report(&[(Status::Unsupported, true)])),
            EXIT_REQUIRED_UNSUPPORTED
        );
        assert_eq!(
            exit_code(&report(&[(Status::NotTested, true)])),
            EXIT_REQUIRED_NOT_TESTED
        );
    }

    #[test]
    fn exit_code_priority_between_mixed_required_results() {
        let mixed = [
            (Status::Unsupported, true),
            (Status::NotTested, true),
            (Status::Blocked, true),
            (Status::Fail, true),
        ];
        assert_eq!(exit_code(&report(&mixed)), EXIT_REQUIRED_FAILED);

        // A definitive UNSUPPORTED outranks a BLOCKED test that could not run
        // because of it, so an unsupported required method exits 4.
        let without_fail = [
            (Status::Unsupported, true),
            (Status::NotTested, true),
            (Status::Blocked, true),
        ];
        assert_eq!(exit_code(&report(&without_fail)), EXIT_REQUIRED_UNSUPPORTED);

        let blocked_only = [(Status::Blocked, true), (Status::NotTested, true)];
        assert_eq!(exit_code(&report(&blocked_only)), EXIT_REQUIRED_BLOCKED);
    }

    #[test]
    fn snapshot_of_a_sample_report() {
        let sample = RunReport {
            schema_version: SCHEMA_VERSION,
            run_id: String::from("0123456789abcdef0123456789abcdef"),
            started_at: String::from("2026-11-18T10:00:00Z"),
            finished_at: String::from("2026-11-18T10:00:05Z"),
            duration_ms: 5_000,
            tally_version: String::from("0.1.0"),
            environment: Environment {
                class: EnvClass::Sandbox,
                endpoint_class: EndpointClass::ExternalSandbox,
                nwc_client: NwcClientIdentity {
                    name: String::from("nwc"),
                    version: String::from("0.45.2"),
                },
                encryption_mode_selected: Some(String::from("nip04")),
                network_reported: Some(String::from("signet")),
                wallet_label: None,
            },
            wallets: vec![
                WalletSummary {
                    role: WalletRole::Payer,
                    service_pubkey: String::from("aa"),
                },
                WalletSummary {
                    role: WalletRole::Receiver,
                    service_pubkey: String::from("bb"),
                },
            ],
            capabilities: Vec::new(),
            tests: vec![
                TestResult::new("connect", "connect", Status::Pass, true)
                    .with_detail("both wallets connect", "connected"),
                TestResult::new("sign_message", "sign_message", Status::NotTested, false)
                    .with_required_capability("sign_message"),
            ],
            workflow_evidence: WorkflowEvidence {
                invoice: Some(InvoiceEvidence {
                    network: Some(String::from("signet")),
                    amount_msat: 21_000,
                    payment_hash: Some(String::from("cc")),
                    seconds_until_expiry: 298,
                }),
                payment: Some(PaymentEvidence {
                    outcome: PaymentOutcome::Paid,
                    fees_paid_msat: Some(0),
                    preimage_present: true,
                    preimage_matches_payment_hash: Some(true),
                }),
                challenge: Some(ChallengeEvidence {
                    http_status: 402,
                    scheme: String::from("L402"),
                    macaroon_present: true,
                    invoice_present: true,
                    binding_checked: true,
                    binding_matches: Some(true),
                }),
                protected_resource: Some(ProtectedResourceEvidence {
                    unauthenticated_status: 402,
                    wrong_proof_status: Some(402),
                    valid_proof_status: Some(200),
                    body_matches_expected: Some(true),
                }),
            },
            limitations: vec![String::from("example limitation")],
        };

        let expected = concat!(
            "{\"schema_version\":1,",
            "\"run_id\":\"0123456789abcdef0123456789abcdef\",",
            "\"started_at\":\"2026-11-18T10:00:00Z\",",
            "\"finished_at\":\"2026-11-18T10:00:05Z\",",
            "\"duration_ms\":5000,",
            "\"tally_version\":\"0.1.0\",",
            "\"environment\":{",
            "\"class\":\"sandbox\",",
            "\"endpoint_class\":\"external_sandbox\",",
            "\"nwc_client\":{\"name\":\"nwc\",\"version\":\"0.45.2\"},",
            "\"encryption_mode_selected\":\"nip04\",",
            "\"network_reported\":\"signet\",",
            "\"wallet_label\":null},",
            "\"wallets\":[",
            "{\"role\":\"payer\",\"service_pubkey\":\"aa\"},",
            "{\"role\":\"receiver\",\"service_pubkey\":\"bb\"}],",
            "\"capabilities\":[],",
            "\"tests\":[",
            "{\"id\":\"connect\",\"name\":\"connect\",\"status\":\"PASS\",\"required\":true,",
            "\"required_capability\":null,\"expected\":\"both wallets connect\",",
            "\"actual\":\"connected\",\"diagnostics\":[]},",
            "{\"id\":\"sign_message\",\"name\":\"sign_message\",\"status\":\"NOT_TESTED\",",
            "\"required\":false,\"required_capability\":\"sign_message\",",
            "\"expected\":\"\",\"actual\":\"\",\"diagnostics\":[]}],",
            "\"workflow_evidence\":{",
            "\"invoice\":{\"network\":\"signet\",\"amount_msat\":21000,",
            "\"payment_hash\":\"cc\",\"seconds_until_expiry\":298},",
            "\"payment\":{\"outcome\":\"paid\",\"fees_paid_msat\":0,",
            "\"preimage_present\":true,\"preimage_matches_payment_hash\":true},",
            "\"challenge\":{\"http_status\":402,\"scheme\":\"L402\",",
            "\"macaroon_present\":true,\"invoice_present\":true,",
            "\"binding_checked\":true,\"binding_matches\":true},",
            "\"protected_resource\":{\"unauthenticated_status\":402,",
            "\"wrong_proof_status\":402,\"valid_proof_status\":200,",
            "\"body_matches_expected\":true}},",
            "\"limitations\":[\"example limitation\"]}"
        );

        assert_eq!(serde_json::to_string(&sample).unwrap(), expected);
    }
}
