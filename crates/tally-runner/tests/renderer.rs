//! Renderer tests: escaping, counts, legend, ASCII and secrecy.

mod common;

use std::sync::Arc;

use common::{FakeConfig, FakePair};
use tally_core::report::{
    ChallengeEvidence, EndpointClass, EnvClass, Environment, InvoiceEvidence, NwcClientIdentity,
    PaymentEvidence, PaymentOutcome, ProtectedResourceEvidence, RunReport, SCHEMA_VERSION,
    TestResult, WalletRole, WalletSummary, WorkflowEvidence,
};
use tally_core::status::Status;
use tally_runner::{Profile, RunConfig, run, to_html, to_json};

const SENTINEL: &str = "<script>alert(1)</script>";

fn sample_report(statuses: &[(Status, bool)]) -> RunReport {
    RunReport {
        schema_version: SCHEMA_VERSION,
        run_id: String::from("0123456789abcdef0123456789abcdef"),
        started_at: String::from("2026-11-18T10:00:00Z"),
        finished_at: String::from("2026-11-18T10:00:05Z"),
        duration_ms: 5_000,
        tally_version: String::from("0.1.0"),
        environment: Environment {
            class: EnvClass::Sandbox,
            endpoint_class: EndpointClass::LocalFixture,
            nwc_client: NwcClientIdentity::new("nwc", "0.45.2"),
            encryption_mode_selected: None,
            network_reported: Some(String::from("signet")),
        },
        wallets: vec![WalletSummary {
            role: WalletRole::Payer,
            service_pubkey: String::from("aa"),
        }],
        capabilities: Vec::new(),
        tests: statuses
            .iter()
            .enumerate()
            .map(|(index, (status, required))| {
                let mut result = TestResult::new(
                    format!("test_{index}"),
                    format!("test {index}"),
                    *status,
                    *required,
                )
                .with_detail("expected", "observed");
                if index == 0 {
                    result = result.with_diagnostic(format!("reason={SENTINEL}"));
                }
                result
            })
            .collect(),
        workflow_evidence: WorkflowEvidence::empty(),
        limitations: vec![String::from("a limitation")],
    }
}

#[test]
fn a_sentinel_in_diagnostics_is_escaped() {
    let report = sample_report(&[(Status::Pass, true)]);
    let html = to_html(&report);

    assert!(!html.contains(SENTINEL));
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
}

#[test]
fn per_status_counts_in_html_equal_the_json() {
    let report = sample_report(&[
        (Status::Pass, true),
        (Status::Pass, true),
        (Status::Fail, true),
        (Status::Unsupported, false),
        (Status::NotTested, true),
        (Status::Blocked, true),
    ]);
    let html = to_html(&report);
    let json = to_json(&report);
    let value: serde_json::Value = serde_json::from_str(&json).expect("json");
    let tests = value["tests"].as_array().expect("tests array");

    for status in tally_runner::STATUSES {
        let count = tests
            .iter()
            .filter(|test| test["status"].as_str() == Some(status.as_str()))
            .count();
        let row = format!("<tr><th scope=\"row\">{}</th><td>{count}</td></tr>", status);
        assert!(html.contains(&row), "missing count row for {status}: {row}");
    }
}

#[test]
fn all_five_statuses_and_the_legend_are_present() {
    let report = sample_report(&[(Status::Pass, true)]);
    let html = to_html(&report);

    assert!(html.contains("Status legend"));
    for status in tally_runner::STATUSES {
        assert!(html.contains(status.as_str()) || html.contains(&status.to_string()));
    }
    assert!(html.contains("NOT TESTED"));
}

#[test]
fn the_html_has_no_script_and_no_external_resource() {
    let report = sample_report(&[(Status::Pass, true)]);
    let html = to_html(&report);

    assert!(!html.contains("<script"));
    assert!(!html.contains("http://"));
    assert!(!html.contains("https://"));
    assert!(!html.contains("<iframe"));
    assert!(html.contains("lang=\"en\""));
    assert!(html.contains("name=\"viewport\""));
    assert!(html.contains("prefers-color-scheme"));
}

#[test]
fn the_html_is_ascii_only() {
    let mut report = sample_report(&[(Status::Pass, true)]);
    report.limitations = vec![String::from("caf\u{e9} and \u{20ac} and \u{1f600}")];

    let html = to_html(&report);

    assert!(html.is_ascii(), "html must stay ASCII");
    assert!(html.contains("caf&#233;"));
}

#[test]
fn json_and_html_come_from_the_same_value() {
    let report = sample_report(&[(Status::Pass, true), (Status::Fail, true)]);
    let json = to_json(&report);
    let html = to_html(&report);

    let from_json: serde_json::Value = serde_json::from_str(&json).expect("json");
    assert_eq!(from_json, serde_json::to_value(&report).expect("value"));
    assert!(html.contains(&report.run_id));
    for test in &report.tests {
        assert!(html.contains(&test.id));
    }
    assert!(from_json["run_id"].as_str() == Some(report.run_id.as_str()));
}

#[test]
fn the_table_has_a_caption_and_column_headers() {
    let report = sample_report(&[(Status::Pass, true)]);
    let html = to_html(&report);

    assert!(html.contains("<caption>Every test in run order</caption>"));
    assert!(html.contains("<th scope=\"col\">Status</th>"));
    assert!(html.contains("<th scope=\"row\">test_0</th>"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_run_keeps_invoice_and_secrets_out_of_both_reports() {
    let dir = std::env::temp_dir().join(format!("tally-renderer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let pair = FakePair::new(FakeConfig::default());
    let config = RunConfig {
        approve_payment_sats: Some(21),
        out_dir: dir.join("reports"),
        guard_dir: dir.join("guards"),
        ..RunConfig::default()
    };

    let mut payer = pair.payer.clone();
    let outcome = run(
        Profile::L402E2e,
        &mut payer,
        Arc::new(pair.receiver.clone()),
        &config,
    )
    .await
    .expect("run finishes");

    let html = std::fs::read_to_string(&outcome.html_path).expect("html file");
    let json = std::fs::read_to_string(&outcome.json_path).expect("json file");

    for text in [&html, &json] {
        assert!(!text.contains("lnbc"));
        assert!(!text.contains("nostr+walletconnect"));
        assert!(!text.contains("secret="));
        assert!(!text.contains(SENTINEL));
    }
    assert!(html.is_ascii());

    // The same evidence reaches both files.
    let value: serde_json::Value = serde_json::from_str(&json).expect("json");
    assert_eq!(
        value["workflow_evidence"]["protected_resource"]["valid_proof_status"].as_u64(),
        Some(200)
    );
    assert!(html.contains("Workflow evidence"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn evidence_sections_render_every_part() {
    let mut report = sample_report(&[(Status::Pass, true)]);
    report.workflow_evidence = WorkflowEvidence {
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
    };

    let html = to_html(&report);

    for needle in [
        "Invoice network",
        "Payment outcome",
        "Challenge scheme",
        "Valid proof status",
        "L402",
        "200",
    ] {
        assert!(html.contains(needle), "missing {needle}");
    }
}
