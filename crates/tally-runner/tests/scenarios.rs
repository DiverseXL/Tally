//! The twelve offline scenarios: real fixture on loopback, fake wallets.
//!
//! Each scenario asserts the statuses, the exit code and the number of payments
//! the fake payer was asked for.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{FakeConfig, FakePair, InvoiceMode, PayMode};
use tally_core::report::{EnvClass, exit_code};
use tally_core::status::Status;
use tally_runner::{Profile, RunConfig, RunOutcome, guard, run};

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tally-scenario-{name}-{}", std::process::id()));
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

async fn run_pair(pair: &FakePair, config: &RunConfig) -> RunOutcome {
    let mut payer = pair.payer.clone();

    run(
        Profile::L402E2e,
        &mut payer,
        Arc::new(pair.receiver.clone()),
        config,
    )
    .await
    .expect("run finishes")
}

fn status(outcome: &RunOutcome, id: &str) -> Status {
    outcome
        .report
        .tests
        .iter()
        .find(|test| test.id == id)
        .unwrap_or_else(|| panic!("no test {id}"))
        .status
}

fn reason(outcome: &RunOutcome, id: &str) -> String {
    let test = outcome
        .report
        .tests
        .iter()
        .find(|test| test.id == id)
        .unwrap_or_else(|| panic!("no test {id}"));

    test.diagnostics
        .iter()
        .find_map(|line| line.strip_prefix("reason="))
        .unwrap_or("UNKNOWN")
        .to_owned()
}

fn approved(dir: &Path) -> RunConfig {
    RunConfig {
        approve_payment_sats: Some(21),
        ..config(dir)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_01_happy_path() {
    let dir = workdir("01");
    let pair = FakePair::new(FakeConfig::default());

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(exit_code(&outcome.report), 0);
    assert_eq!(pair.pay_calls(), 1);
    assert_eq!(status(&outcome, "pay_invoice"), Status::Pass);
    assert_eq!(status(&outcome, "preimage_verification"), Status::Pass);
    assert_eq!(status(&outcome, "protected_resource"), Status::Pass);
    assert_eq!(status(&outcome, "lookup_invoice"), Status::Pass);
    assert_eq!(
        outcome
            .report
            .workflow_evidence
            .protected_resource
            .as_ref()
            .and_then(|resource| resource.valid_proof_status),
        Some(200)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_02_no_approval_flag() {
    let dir = workdir("02");
    let pair = FakePair::new(FakeConfig::default());

    let outcome = run_pair(&pair, &config(&dir)).await;

    assert_eq!(status(&outcome, "pay_invoice"), Status::NotTested);
    assert_eq!(reason(&outcome, "pay_invoice"), "PAYMENT_NOT_APPROVED");
    assert_eq!(status(&outcome, "preimage_verification"), Status::NotTested);
    assert_eq!(status(&outcome, "protected_resource"), Status::NotTested);
    assert_eq!(status(&outcome, "negative_wrong_proof"), Status::Pass);
    assert_eq!(status(&outcome, "negative_no_proof"), Status::Pass);
    assert_eq!(exit_code(&outcome.report), 5);
    assert_eq!(pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_03_receiver_does_not_advertise_make_invoice() {
    let dir = workdir("03");
    let pair = FakePair::new(FakeConfig {
        receiver_info: Some("get_info get_balance pay_invoice lookup_invoice"),
        receiver_get_info: "get_info get_balance pay_invoice lookup_invoice",
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(status(&outcome, "challenge"), Status::Unsupported);
    assert_eq!(status(&outcome, "pay_invoice"), Status::Blocked);
    assert_eq!(reason(&outcome, "pay_invoice"), "PREREQUISITE_FAILED");
    assert_eq!(exit_code(&outcome.report), 4);
    assert_eq!(pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_04_connect_transport_timeout() {
    let dir = workdir("04");
    let pair = FakePair::new(FakeConfig {
        connect_fails: true,
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(status(&outcome, "connect"), Status::Blocked);
    assert_eq!(reason(&outcome, "connect"), "TRANSPORT_PROBLEM");
    assert_eq!(status(&outcome, "challenge"), Status::Blocked);
    assert_eq!(exit_code(&outcome.report), 3);
    assert_eq!(pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_05_expired_or_wrong_amount_invoice() {
    let dir = workdir("05");

    for mode in [InvoiceMode::Expired, InvoiceMode::WrongAmount] {
        let pair = FakePair::new(FakeConfig {
            invoice: mode,
            ..FakeConfig::default()
        });
        let outcome = run_pair(&pair, &approved(&dir)).await;

        assert_eq!(status(&outcome, "invoice_validation"), Status::Fail);
        assert_eq!(exit_code(&outcome.report), 1);
        assert_eq!(pair.pay_calls(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_06_invoice_network_inconsistent_with_wallet() {
    let dir = workdir("06");
    let pair = FakePair::new(FakeConfig {
        network: "mainnet",
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(status(&outcome, "invoice_validation"), Status::Fail);
    assert_eq!(exit_code(&outcome.report), 1);
    assert_eq!(pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_07_payer_has_insufficient_balance() {
    let dir = workdir("07");
    let pair = FakePair::new(FakeConfig {
        pay: PayMode::InsufficientBalance,
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(status(&outcome, "pay_invoice"), Status::Fail);
    assert_eq!(
        reason(&outcome, "pay_invoice"),
        "WALLET_INSUFFICIENT_BALANCE"
    );
    assert_eq!(status(&outcome, "protected_resource"), Status::Blocked);
    assert_eq!(exit_code(&outcome.report), 1);
    assert_eq!(pair.pay_calls(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_08_wallet_returns_no_preimage() {
    let dir = workdir("08");
    let pair = FakePair::new(FakeConfig {
        pay: PayMode::NoPreimage,
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(status(&outcome, "pay_invoice"), Status::Pass);
    assert_eq!(status(&outcome, "preimage_verification"), Status::Fail);
    assert_eq!(reason(&outcome, "preimage_verification"), "NO_PREIMAGE");
    assert_ne!(status(&outcome, "protected_resource"), Status::Pass);
    assert_eq!(exit_code(&outcome.report), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_09_wallet_returns_a_wrong_preimage() {
    let dir = workdir("09");
    let pair = FakePair::new(FakeConfig {
        pay: PayMode::WrongPreimage,
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &approved(&dir)).await;

    assert_eq!(status(&outcome, "preimage_verification"), Status::Fail);
    assert_eq!(
        reason(&outcome, "preimage_verification"),
        "PREIMAGE_MISMATCH"
    );
    assert_eq!(status(&outcome, "protected_resource"), Status::Fail);
    assert_eq!(
        outcome
            .report
            .workflow_evidence
            .protected_resource
            .as_ref()
            .and_then(|resource| resource.valid_proof_status),
        Some(402)
    );
    assert_eq!(exit_code(&outcome.report), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_10_ambiguous_payment_blocks_the_next_run() {
    let dir = workdir("10");
    let pair = FakePair::new(FakeConfig {
        pay: PayMode::Ambiguous,
        ..FakeConfig::default()
    });
    let config = approved(&dir);

    let first = run_pair(&pair, &config).await;
    assert_eq!(status(&first, "pay_invoice"), Status::Blocked);
    assert_eq!(
        reason(&first, "pay_invoice"),
        "AMBIGUOUS_PAYMENT_NOT_RETRIED"
    );
    assert_eq!(pair.pay_calls(), 1);

    let state_files = fs::read_dir(&config.guard_dir)
        .expect("guard dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("state"))
        .count();
    assert_eq!(state_files, 1);
    let text = fs::read_to_string(
        fs::read_dir(&config.guard_dir)
            .expect("guard dir")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().and_then(|value| value.to_str()) == Some("state"))
            .expect("state file"),
    )
    .expect("state text");
    assert!(text.contains("AMBIGUOUS"));

    // A second run refuses to pay.
    let second = run_pair(&pair, &config).await;
    assert_eq!(status(&second, "pay_invoice"), Status::Blocked);
    assert_eq!(reason(&second, "pay_invoice"), "RUN_GUARD_BLOCKED");
    assert_eq!(pair.pay_calls(), 1);

    // Clearing the guard re-enables paying.
    let cleared_config = RunConfig {
        clear_guard: true,
        ..approved(&dir)
    };
    let third = run_pair(&pair, &cleared_config).await;
    assert_eq!(third.guard_cleared, 1);
    assert_eq!(pair.pay_calls(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_11_live_without_acknowledgement() {
    let dir = workdir("11");
    let pair = FakePair::new(FakeConfig::default());
    let config = RunConfig {
        env_class: EnvClass::Live,
        approve_payment_sats: Some(21),
        ..config(&dir)
    };

    let outcome = run_pair(&pair, &config).await;

    assert_eq!(status(&outcome, "pay_invoice"), Status::Blocked);
    assert_eq!(reason(&outcome, "pay_invoice"), "LIVE_NOT_ACKNOWLEDGED");
    assert_eq!(exit_code(&outcome.report), 3);
    assert_eq!(pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_12_a_paying_state_file_blocks_the_run() {
    let dir = workdir("12");
    let pair = FakePair::new(FakeConfig::default());
    let config = approved(&dir);

    guard::begin(&config.guard_dir, &"ab".repeat(32)).expect("guard file");

    let outcome = run_pair(&pair, &config).await;

    assert_eq!(status(&outcome, "pay_invoice"), Status::Blocked);
    assert_eq!(reason(&outcome, "pay_invoice"), "RUN_GUARD_BLOCKED");
    assert_eq!(pair.pay_calls(), 0);
    assert_eq!(exit_code(&outcome.report), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_13_the_report_records_the_negotiated_encryption_mode() {
    let dir = workdir("13");
    let pair = FakePair::new(FakeConfig::default());

    let outcome = run_pair(&pair, &config(&dir)).await;

    // The fake info event lists nip44_v2, so that is the cipher selected and
    // recorded.
    assert_eq!(
        outcome
            .report
            .environment
            .encryption_mode_selected
            .as_deref(),
        Some("nip44_v2")
    );

    let info_event = outcome
        .report
        .tests
        .iter()
        .find(|test| test.id == "info_event")
        .expect("info_event result");
    assert!(
        info_event
            .diagnostics
            .iter()
            .any(|line| line == "payer_encryption_mode=nip44_v2")
    );
    assert!(
        info_event
            .diagnostics
            .iter()
            .any(|line| line == "receiver_encryption_mode=nip44_v2")
    );

    // The stored JSON carries the same value, not only the in-memory report.
    let json = fs::read_to_string(&outcome.json_path).expect("json file");
    assert!(json.contains("\"encryption_mode_selected\": \"nip44_v2\""));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_14_nip04_only_wallet_records_nip04() {
    let dir = workdir("14");
    let pair = FakePair::new(FakeConfig {
        encryption_tokens: "nip04",
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &config(&dir)).await;

    assert_eq!(
        outcome
            .report
            .environment
            .encryption_mode_selected
            .as_deref(),
        Some("nip04")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_15_payer_and_receiver_keep_their_own_modes() {
    let dir = workdir("15");
    let pair = FakePair::new(FakeConfig {
        encryption_tokens: "nip44_v2 nip04",
        receiver_encryption_tokens: Some("nip04"),
        ..FakeConfig::default()
    });

    let outcome = run_pair(&pair, &config(&dir)).await;

    // Each wallet picks its own cipher from its own info event: the receiver
    // must not inherit the payer's mode.
    assert_eq!(
        outcome
            .report
            .environment
            .encryption_mode_selected
            .as_deref(),
        Some("nip44_v2")
    );

    let info_event = outcome
        .report
        .tests
        .iter()
        .find(|test| test.id == "info_event")
        .expect("info_event result");
    assert!(
        info_event
            .diagnostics
            .iter()
            .any(|line| line == "payer_encryption_mode=nip44_v2")
    );
    assert!(
        info_event
            .diagnostics
            .iter()
            .any(|line| line == "receiver_encryption_mode=nip04")
    );
}
