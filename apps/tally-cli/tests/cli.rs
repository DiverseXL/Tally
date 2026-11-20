//! Offline CLI tests: no network, no wallet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tally_core::report::{
    EndpointClass, EnvClass, Environment, NwcClientIdentity, RunReport, SCHEMA_VERSION, TestResult,
    WorkflowEvidence,
};
use tally_core::status::Status;

const BIN: &str = env!("CARGO_BIN_EXE_tally");

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tally-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn run(dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command.current_dir(dir).args(args);
    command.env_remove("TALLY_PAYER_NWC_URI");
    command.env_remove("TALLY_RECEIVER_NWC_URI");
    command.env_remove("TALLY_WALLET_NWC_URI");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("binary runs")
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn sample_json() -> String {
    let report = RunReport {
        schema_version: SCHEMA_VERSION,
        run_id: String::from("fedcba9876543210fedcba9876543210"),
        started_at: String::from("2026-11-18T10:00:00Z"),
        finished_at: String::from("2026-11-18T10:00:01Z"),
        duration_ms: 1_000,
        tally_version: String::from("0.1.0"),
        environment: Environment {
            class: EnvClass::Sandbox,
            endpoint_class: EndpointClass::None,
            nwc_client: NwcClientIdentity::new("nwc", "0.45.2"),
            encryption_mode_selected: None,
            network_reported: Some(String::from("signet")),
            wallet_label: None,
        },
        wallets: Vec::new(),
        capabilities: Vec::new(),
        tests: vec![TestResult::new("connect", "connect", Status::Pass, true)],
        workflow_evidence: WorkflowEvidence::empty(),
        limitations: vec![String::from("a limitation")],
    };

    serde_json::to_string_pretty(&report).expect("json")
}

#[test]
fn version_prints_the_package_version() {
    let dir = workdir("version");
    let output = run(&dir, &["--version"], &[]);

    assert!(output.status.success());
    assert!(text(&output).contains("tally 0.1.0"));
}

#[test]
fn doctor_without_configuration_exits_two_and_prints_no_values() {
    let dir = workdir("doctor-empty");
    let output = run(&dir, &["doctor"], &[]);
    let output_text = text(&output);

    assert_eq!(output.status.code(), Some(2));
    assert!(output_text.contains("source=none"));
    assert!(output_text.contains("ready=false"));
    assert!(!output_text.contains("nostr+walletconnect"));
}

#[test]
fn doctor_reports_a_parsing_connection_string_without_printing_it() {
    let dir = workdir("doctor-sentinel");
    let sentinel = "nostr+walletconnect://sentinel-value-never-printed";
    let output = run(&dir, &["doctor"], &[("TALLY_PAYER_NWC_URI", sentinel)]);
    let output_text = text(&output);

    assert!(output_text.contains("payer source=env parses=false"));
    assert!(!output_text.contains("sentinel-value-never-printed"));
}

#[test]
fn a_usage_error_exits_two() {
    let dir = workdir("usage");
    let missing_flag = run(&dir, &["inspect"], &[]);
    assert_eq!(missing_flag.status.code(), Some(2));

    let unknown_profile = run(&dir, &["test", "--profile", "nope"], &[]);
    assert_eq!(unknown_profile.status.code(), Some(2));

    let unknown_command = run(&dir, &["nonsense"], &[]);
    assert_eq!(unknown_command.status.code(), Some(2));
}

#[test]
fn a_missing_connection_string_exits_two_without_echoing_anything() {
    let dir = workdir("missing-config");
    let output = run(&dir, &["test", "--profile", "basic"], &[]);
    let output_text = text(&output);

    assert_eq!(output.status.code(), Some(2));
    assert!(output_text.contains("configuration error"));
    assert!(!output_text.contains("nostr+walletconnect"));
}

#[test]
fn report_round_trips_json_to_html() {
    let dir = workdir("report");
    let input = dir.join("report.json");
    let out = dir.join("report.html");
    fs::write(&input, sample_json()).expect("write json");

    let output = run(
        &dir,
        &[
            "report",
            "--input",
            input.to_str().expect("path"),
            "--format",
            "html",
            "--out",
            out.to_str().expect("path"),
        ],
        &[],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output));

    let html = fs::read_to_string(&out).expect("html");
    assert!(html.contains("fedcba9876543210fedcba9876543210"));
    assert!(html.contains("Status legend"));
    assert!(html.is_ascii());
    assert!(!html.contains("<script"));
}

#[test]
fn report_rejects_an_unknown_schema_version() {
    let dir = workdir("report-schema");
    let input = dir.join("report.json");
    let out = dir.join("out.json");
    fs::write(
        &input,
        sample_json().replace("\"schema_version\": 1", "\"schema_version\": 99"),
    )
    .expect("write json");

    let output = run(
        &dir,
        &[
            "report",
            "--input",
            input.to_str().expect("path"),
            "--format",
            "json",
            "--out",
            out.to_str().expect("path"),
        ],
        &[],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output).contains("unsupported schema_version"));
    assert!(!out.exists());
}

#[test]
fn a_connection_string_sentinel_never_reaches_any_output() {
    let dir = workdir("sentinel");
    let sentinel = "nostr+walletconnect://sentinel-connection-string-value";
    let receiver = "not-a-valid-connection-string-either";

    let doctor = run(
        &dir,
        &["doctor"],
        &[
            ("TALLY_PAYER_NWC_URI", sentinel),
            ("TALLY_RECEIVER_NWC_URI", receiver),
        ],
    );
    assert!(!text(&doctor).contains("sentinel-connection-string-value"));

    let test = run(
        &dir,
        &["test", "--profile", "basic"],
        &[
            ("TALLY_PAYER_NWC_URI", sentinel),
            ("TALLY_RECEIVER_NWC_URI", receiver),
        ],
    );
    assert_eq!(test.status.code(), Some(2));
    assert!(!text(&test).contains("sentinel-connection-string-value"));
    assert!(!text(&test).contains(receiver));
}

#[test]
fn inspect_wallet_without_a_wallet_exits_two_and_prints_nothing() {
    let dir = workdir("wallet-empty");
    let output = run(&dir, &["inspect", "--profile", "wallet"], &[]);
    let output_text = text(&output);

    assert_eq!(output.status.code(), Some(2));
    assert!(output_text.contains("no wallet connection string is configured"));
    assert!(!output_text.contains("nostr+walletconnect"));
    assert!(!dir.join("reports").exists());
}

#[test]
fn inspect_wallet_never_prints_the_connection_string_it_refuses() {
    let dir = workdir("wallet-sentinel");
    fs::write(
        dir.join(".env.wallet"),
        "TALLY_WALLET_NWC_URI=nostr+walletconnect://wallet-file-sentinel-value\n",
    )
    .expect("write wallet file");

    let output = run(
        &dir,
        &[
            "inspect",
            "--profile",
            "wallet",
            "--wallet-file",
            ".env.wallet",
            "--label",
            "Test Wallet",
            "--require",
            "get_info,pay_invoice",
        ],
        &[],
    );
    let output_text = text(&output);

    assert_eq!(output.status.code(), Some(2));
    assert!(output_text.contains("does not parse"));
    assert!(!output_text.contains("wallet-file-sentinel-value"));
    assert!(!output_text.contains("nostr+walletconnect"));
    assert!(!dir.join("reports").exists());
}

#[test]
fn a_wallet_file_must_hold_exactly_one_wallet_line() {
    let dir = workdir("wallet-shape");
    fs::write(
        dir.join("two-lines.env"),
        "TALLY_WALLET_NWC_URI=nostr+walletconnect://sentinel-a\nTALLY_WALLET_NWC_URI=nostr+walletconnect://sentinel-b\n",
    )
    .expect("write two-line file");
    fs::write(
        dir.join("other-key.env"),
        "SOMETHING_ELSE=nostr+walletconnect://sentinel-c\n",
    )
    .expect("write other-key file");

    for file in ["two-lines.env", "other-key.env"] {
        let output = run(
            &dir,
            &["inspect", "--profile", "wallet", "--wallet-file", file],
            &[],
        );
        let output_text = text(&output);

        assert_eq!(output.status.code(), Some(2), "{file}");
        assert!(
            output_text.contains("one connection string line"),
            "{file}: {output_text}"
        );
        assert!(!output_text.contains("sentinel-"), "{file}");
    }
}

#[test]
fn single_wallet_flags_are_refused_by_the_two_wallet_profiles() {
    let dir = workdir("wallet-flags");
    let output = run(
        &dir,
        &[
            "inspect",
            "--profile",
            "basic",
            "--wallet-file",
            "missing.env",
        ],
        &[],
    );

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output).contains("need --profile wallet"));
}

#[test]
fn dotenv_files_are_read_when_the_environment_is_empty() {
    let dir = workdir("dotenv");
    fs::write(
        dir.join(".env.payer"),
        "TALLY_PAYER_NWC_URI=nostr+walletconnect://file-sentinel-value\n",
    )
    .expect("write payer file");

    let output = run(&dir, &["doctor"], &[]);
    let output_text = text(&output);

    assert!(output_text.contains("payer source=file"));
    assert!(!output_text.contains("file-sentinel-value"));
}
