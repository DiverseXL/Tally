//! Offline tests of the single-wallet inspection profile.
//!
//! Every test uses the TEST ONLY fake wallet, and every one asserts that the
//! fake was never asked to pay.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{FakeConfig, FakePair};
use tally_core::outcome::reason;
use tally_core::report::{RunReport, WalletRole, exit_code};
use tally_core::status::Status;
use tally_runner::{
    ADVERTISED_ONLY, NO_PAYMENT_LIMITATION, Profile, RunConfig, run_wallet, summary_lines,
};

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tally-wallet-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn config(dir: &Path) -> RunConfig {
    RunConfig {
        out_dir: dir.join("reports"),
        guard_dir: dir.join("guards"),
        ..RunConfig::default()
    }
}

fn status(report: &RunReport, id: &str) -> Option<Status> {
    report
        .tests
        .iter()
        .find(|test| test.id == id)
        .map(|test| test.status)
}

fn diagnostic(report: &RunReport, id: &str, line: &str) -> bool {
    report
        .tests
        .iter()
        .find(|test| test.id == id)
        .is_some_and(|test| test.diagnostics.iter().any(|value| value == line))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wallet_profile_passes_when_every_required_method_is_advertised() {
    let dir = workdir("defaults");
    let pair = FakePair::new(FakeConfig::default());
    let mut wallet = pair.receiver.clone();

    let outcome = run_wallet(&mut wallet, &config(&dir))
        .await
        .expect("run finishes");
    let report = &outcome.report;

    assert_eq!(outcome.profile, Profile::Wallet);
    assert_eq!(exit_code(report), 0);
    assert_eq!(pair.pay_calls(), 0);

    assert_eq!(status(report, "connect"), Some(Status::Pass));
    assert_eq!(status(report, "info_event"), Some(Status::Pass));
    assert_eq!(status(report, "get_info"), Some(Status::Pass));
    assert_eq!(status(report, "get_balance"), Some(Status::Pass));
    assert_eq!(status(report, "make_invoice"), Some(Status::Pass));
    assert_eq!(status(report, "lookup_invoice"), Some(Status::Pass));

    for method in ["get_info", "get_balance", "make_invoice", "lookup_invoice"] {
        let id = format!("capability_{method}");
        assert_eq!(status(report, &id), Some(Status::Pass), "{id}");
        assert!(
            diagnostic(report, &id, ADVERTISED_ONLY),
            "the {id} check says it was not exercised"
        );
    }
    assert_eq!(status(report, "capability_pay_invoice"), Some(Status::Pass));
    assert!(diagnostic(
        report,
        "capability_pay_invoice",
        ADVERTISED_ONLY
    ));

    assert_eq!(report.wallets.len(), 1);
    assert_eq!(report.wallets[0].role, WalletRole::Wallet);
    assert!(report.tests.iter().all(|test| test.required));
    assert!(
        report
            .limitations
            .iter()
            .any(|line| line == NO_PAYMENT_LIMITATION)
    );

    let evidence = report
        .workflow_evidence
        .invoice
        .as_ref()
        .expect("invoice evidence");
    assert_eq!(evidence.amount_msat, 21_000);
    assert!(evidence.payment_hash.is_some());
    assert!(report.workflow_evidence.payment.is_none());

    let printed = summary_lines(&outcome).join("\n");
    assert!(printed.contains("profile wallet"));
    assert!(printed.contains("exit_code 0"));

    let json = fs::read_to_string(&outcome.json_path).expect("report json");
    assert!(!printed.to_lowercase().contains("lnbc"));
    assert!(json.contains("\"wallet_label\": null"));

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unadvertised_required_method_is_unsupported_and_nothing_is_paid() {
    let dir = workdir("unadvertised");
    let advertised = "get_info get_balance make_invoice lookup_invoice";
    let pair = FakePair::new(FakeConfig {
        receiver_info: Some(advertised),
        receiver_get_info: advertised,
        ..FakeConfig::default()
    });
    let mut wallet = pair.receiver.clone();

    let outcome = run_wallet(&mut wallet, &config(&dir))
        .await
        .expect("run finishes");
    let report = &outcome.report;

    assert_eq!(
        status(report, "capability_pay_invoice"),
        Some(Status::Unsupported)
    );
    assert!(diagnostic(
        report,
        "capability_pay_invoice",
        "capability_source=info_event"
    ));
    assert_eq!(exit_code(report), 4);
    assert_eq!(pair.pay_calls(), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_required_method_the_wallet_lacks_is_unsupported() {
    let dir = workdir("sign-message");
    let pair = FakePair::new(FakeConfig::default());
    let mut wallet = pair.receiver.clone();
    let mut config = config(&dir);
    config.wallet_required = vec![String::from("sign_message")];

    let outcome = run_wallet(&mut wallet, &config)
        .await
        .expect("run finishes");
    let report = &outcome.report;

    assert_eq!(
        status(report, "capability_sign_message"),
        Some(Status::Unsupported)
    );
    assert_eq!(exit_code(report), 4);
    assert_eq!(pair.pay_calls(), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unknown_method_name_is_allowed_verbatim() {
    let dir = workdir("unknown");
    let pair = FakePair::new(FakeConfig::default());
    let mut wallet = pair.receiver.clone();
    let mut config = config(&dir);
    config.wallet_required = vec![String::from("no_such_method")];

    let outcome = run_wallet(&mut wallet, &config)
        .await
        .expect("run finishes");
    let report = &outcome.report;

    assert_eq!(
        status(report, "capability_no_such_method"),
        Some(Status::Unsupported)
    );
    assert_eq!(exit_code(report), 4);
    assert_eq!(pair.pay_calls(), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_required_list_replaces_the_default_list() {
    let dir = workdir("replace");
    let pair = FakePair::new(FakeConfig::default());
    let mut wallet = pair.receiver.clone();
    let mut config = config(&dir);
    config.wallet_required = vec![String::from("get_balance"), String::from("make_invoice")];

    let outcome = run_wallet(&mut wallet, &config)
        .await
        .expect("run finishes");
    let report = &outcome.report;

    let capability_tests = report
        .tests
        .iter()
        .filter(|test| test.id.starts_with("capability_"))
        .map(|test| test.id.as_str())
        .collect::<Vec<&str>>();
    assert_eq!(
        capability_tests,
        vec!["capability_get_balance", "capability_make_invoice"]
    );
    assert_eq!(exit_code(report), 0);
    assert_eq!(pair.pay_calls(), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_info_event_does_not_block_the_later_tests() {
    let dir = workdir("no-info-event");
    let pair = FakePair::new(FakeConfig {
        receiver_info: None,
        ..FakeConfig::default()
    });
    let mut wallet = pair.receiver.clone();

    let outcome = run_wallet(&mut wallet, &config(&dir))
        .await
        .expect("run finishes");
    let report = &outcome.report;

    assert_eq!(status(report, "info_event"), Some(Status::Fail));
    assert!(diagnostic(
        report,
        "info_event",
        &format!("reason={}", reason::INFO_EVENT_NOT_FOUND)
    ));

    // Every later test still runs, and the capability list comes from get_info.
    assert_eq!(status(report, "get_info"), Some(Status::Pass));
    assert_eq!(status(report, "get_balance"), Some(Status::Pass));
    assert_eq!(status(report, "make_invoice"), Some(Status::Pass));
    assert_eq!(status(report, "capability_pay_invoice"), Some(Status::Pass));
    assert!(diagnostic(
        report,
        "capability_pay_invoice",
        "capability_source=get_info"
    ));
    assert!(
        report
            .limitations
            .iter()
            .any(|line| line.contains("comes from get_info"))
    );
    assert_eq!(exit_code(report), 1);
    assert_eq!(pair.pay_calls(), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_label_is_stored_in_the_environment_block() {
    let dir = workdir("label");
    let pair = FakePair::new(FakeConfig::default());
    let mut wallet = pair.receiver.clone();
    let mut config = config(&dir);
    config.wallet_label = Some(String::from("alby-hub"));

    let outcome = run_wallet(&mut wallet, &config)
        .await
        .expect("run finishes");

    assert_eq!(
        outcome.report.environment.wallet_label.as_deref(),
        Some("alby-hub")
    );
    assert_eq!(outcome.report.environment.endpoint_class.as_str(), "none");
    assert_eq!(pair.pay_calls(), 0);

    let _ = fs::remove_dir_all(&dir);
}
