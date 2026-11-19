//! The server tests: in-process Axum on 127.0.0.1:0, TEST ONLY fake wallets.
//!
//! Every test here starts a real router and real sockets. Only the two wallets
//! are fake; the fixture, the guard, the permit, the preimage check and the
//! reports are the real code.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use nostr::key::Keys;
use serde_json::Value;
use tally_runner::test_support::{FakeConfig, FakePair, PayMode};
use tally_runner::{Profile, RunConfig, RunOutcome, run_with_observer};
use tally_server::{
    AppState, EventSink, RunExecutor, RunRequest, SecretRef, ServerConfig, WalletSet, app, bind,
};

/// A syntactically valid connection string built from a fresh throwaway key,
/// returned with the secret it carries. Parsing it opens no socket, and no test
/// here sends it anywhere.
fn dummy_connection() -> (String, String) {
    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let secret = keys.secret_key().to_secret_hex();
    let uri = format!(
        "nostr+walletconnect://{pubkey}?secret={secret}&relay=wss%3A%2F%2Frelay.example.com"
    );

    (uri, secret)
}

struct FakeExecutor {
    pair: Arc<FakePair>,
    out_dir: PathBuf,
    guard_dir: PathBuf,
}

impl RunExecutor for FakeExecutor {
    fn execute(
        &self,
        request: RunRequest,
        sink: EventSink,
    ) -> Pin<Box<dyn Future<Output = Result<RunOutcome, String>> + Send>> {
        let config = RunConfig {
            run_id: Some(request.run_id),
            approve_payment_sats: request.approve_payment_sats,
            clear_guard: request.clear_guard,
            out_dir: self.out_dir.clone(),
            guard_dir: self.guard_dir.clone(),
            ..RunConfig::default()
        };
        let pair = Arc::clone(&self.pair);

        Box::pin(async move {
            let mut payer = pair.payer.clone();
            let mut sink = sink;

            run_with_observer(
                request.profile,
                &mut payer,
                Arc::new(pair.receiver.clone()),
                &config,
                &mut sink,
            )
            .await
            .map_err(|error| error.to_string())
        })
    }
}

struct TestServer {
    base: String,
    client: reqwest::Client,
    dir: PathBuf,
    pair: Arc<FakePair>,
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tally-server-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn ready_wallets() -> WalletSet {
    let (uri, _) = dummy_connection();

    WalletSet::new(
        Some(SecretRef::new(uri.clone(), "env")),
        Some(SecretRef::new(uri, "env")),
    )
}

async fn start(name: &str, pair: FakePair, wallets: WalletSet) -> TestServer {
    let dir = temp_dir(name);
    let config = ServerConfig {
        port: 0,
        out_dir: dir.join("reports"),
        guard_dir: dir.join("guards"),
        ..ServerConfig::default()
    };

    let (listener, address) = bind(&config).await.expect("bind loopback");
    let config = ServerConfig {
        port: address.port(),
        ..config
    };

    let pair = Arc::new(pair);
    let executor = Arc::new(FakeExecutor {
        pair: Arc::clone(&pair),
        out_dir: config.out_dir.clone(),
        guard_dir: config.guard_dir.clone(),
    });
    let state = AppState::new(config, wallets, executor);
    let router = app(state);

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    TestServer {
        base: format!("http://127.0.0.1:{}", address.port()),
        client: reqwest::Client::new(),
        dir,
        pair,
    }
}

impl TestServer {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn out_dir(&self) -> PathBuf {
        self.dir.join("reports")
    }

    async fn status(&self) -> Value {
        let response = self
            .client
            .get(self.url("/api/status"))
            .send()
            .await
            .expect("status");

        json_body(response).await
    }

    async fn post(&self, body: Value) -> reqwest::Response {
        self.client
            .post(self.url("/api/runs"))
            .header("content-type", "application/json")
            .header("x-tally-request", "1")
            .body(body.to_string())
            .send()
            .await
            .expect("post")
    }

    async fn start_run(&self, profile: &str, sats: Option<u64>, clear_guard: bool) -> String {
        let body = match sats {
            Some(sats) => serde_json::json!({
                "profile": profile,
                "approve_payment_sats": sats,
                "confirm": format!("PAY {sats} SATS"),
                "clear_guard": clear_guard,
            }),
            None => serde_json::json!({
                "profile": profile,
                "approve_payment_sats": null,
                "confirm": null,
                "clear_guard": clear_guard,
            }),
        };

        let response = self.post(body).await;
        assert_eq!(response.status(), 202, "run accepted");
        let value = json_body(response).await;
        value["run_id"].as_str().expect("run id string").to_owned()
    }

    async fn wait_for_report(&self, run_id: &str) -> PathBuf {
        let path = self.out_dir().join(run_id).join("report.json");
        for _ in 0..400 {
            if path.exists() {
                // The done event is written just after the report.
                tokio::time::sleep(Duration::from_millis(25)).await;
                return path;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("report for {run_id} never appeared");
    }

    async fn events(&self, run_id: &str) -> String {
        for _ in 0..200 {
            let text = self
                .client
                .get(self.url(&format!("/api/runs/{run_id}/events")))
                .send()
                .await
                .expect("events")
                .text()
                .await
                .expect("events text");

            if text.contains("event: done") {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("no done event for {run_id}");
    }

    async fn run_to_end(&self, profile: &str, sats: Option<u64>) -> (String, Value) {
        let run_id = self.start_run(profile, sats, false).await;
        let path = self.wait_for_report(&run_id).await;
        let text = std::fs::read_to_string(path).expect("report text");
        let report: Value = serde_json::from_str(&text).expect("report json");
        (run_id, report)
    }
}

fn sse_events(text: &str) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    let mut name = String::new();

    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            name = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("data: ") {
            let data: Value = serde_json::from_str(value).expect("event json");
            events.push((name.clone(), data));
        }
    }

    events
}

fn status_of(report: &Value, id: &str) -> String {
    report["tests"]
        .as_array()
        .expect("tests array")
        .iter()
        .find(|test| test["id"] == id)
        .unwrap_or_else(|| panic!("no test {id}"))["status"]
        .as_str()
        .expect("status string")
        .to_owned()
}

fn reason_of(report: &Value, id: &str) -> String {
    let test = report["tests"]
        .as_array()
        .expect("tests array")
        .iter()
        .find(|test| test["id"] == id)
        .unwrap_or_else(|| panic!("no test {id}"));

    test["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter_map(|line| line.as_str())
        .find_map(|line| line.strip_prefix("reason="))
        .unwrap_or("UNKNOWN")
        .to_owned()
}

/// Reads a response body as JSON. The `json` feature of reqwest stays off.
async fn json_body(response: reqwest::Response) -> Value {
    let text = response.text().await.expect("body text");
    serde_json::from_str(&text).expect("json body")
}

fn run_dirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.len() == 32 && name.chars().all(|c| c.is_ascii_hexdigit()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_01_status_has_no_secret_and_reports_readiness() {
    let (uri, secret) = dummy_connection();
    let wallets = WalletSet::new(
        Some(SecretRef::new(uri.clone(), "env")),
        Some(SecretRef::new(uri, "env")),
    );
    let server = start("status", FakePair::new(FakeConfig::default()), wallets).await;

    let response = server
        .client
        .get(server.url("/api/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(!text.contains(&secret));
    assert!(!text.contains("nostr+walletconnect"));

    let value: Value = serde_json::from_str(&text).expect("status json");
    assert_eq!(value["ready"], true);
    assert_eq!(value["environment"], "sandbox");
    assert_eq!(value["payment_amount_sats"], 21);
    assert_eq!(value["doctor"]["payer"]["parses"], true);

    // The same route on a server with no wallets at all.
    let bare = start(
        "status-bare",
        FakePair::new(FakeConfig::default()),
        WalletSet::default(),
    )
    .await;
    let value = bare.status().await;
    assert_eq!(value["ready"], false);
    assert_eq!(value["doctor"]["payer"]["source"], "none");
    assert_eq!(value["doctor"]["receiver"]["parses"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_02_wrong_host_is_forbidden() {
    let server = start(
        "host",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let response = server
        .client
        .get(server.url("/api/status"))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
    assert_eq!(response.headers()["cache-control"], "no-store");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_03_post_without_the_custom_header_is_forbidden() {
    let server = start(
        "noheader",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let response = server
        .client
        .post(server.url("/api/runs"))
        .header("content-type", "application/json")
        .body(r#"{"profile":"basic"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 403);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_04_mismatched_origin_is_forbidden() {
    let server = start(
        "origin",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let response = server
        .client
        .post(server.url("/api/runs"))
        .header("content-type", "application/json")
        .header("x-tally-request", "1")
        .header("origin", "http://evil.example")
        .body(r#"{"profile":"basic"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 403);

    // The matching origin is accepted past the control.
    let response = server
        .client
        .post(server.url("/api/runs"))
        .header("content-type", "application/json")
        .header("x-tally-request", "1")
        .header("origin", server.base.clone())
        .body(r#"{"profile":"basic"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_05_wrong_content_type_is_rejected() {
    let server = start(
        "ctype",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let response = server
        .client
        .post(server.url("/api/runs"))
        .header("content-type", "text/plain")
        .header("x-tally-request", "1")
        .body(r#"{"profile":"basic"}"#)
        .send()
        .await
        .unwrap();

    assert!(response.status().is_client_error());
    let value = json_body(response).await;
    assert!(value["error"]["code"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_06_oversize_body_is_rejected() {
    let server = start(
        "oversize",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let padding = "a".repeat(8 * 1024);
    let body = serde_json::json!({ "profile": "basic", "padding": padding }).to_string();

    let response = server
        .client
        .post(server.url("/api/runs"))
        .header("content-type", "application/json")
        .header("x-tally-request", "1")
        .body(body)
        .send()
        .await
        .unwrap();

    assert!(response.status().is_client_error());
    assert_eq!(server.pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_07_wrong_confirmation_starts_nothing() {
    let server = start(
        "confirm",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let response = server
        .post(serde_json::json!({
            "profile": "l402-e2e",
            "approve_payment_sats": 21,
            "confirm": "PAY 22 SATS",
            "clear_guard": false,
        }))
        .await;

    assert_eq!(response.status(), 400);
    let value = json_body(response).await;
    assert_eq!(value["error"]["code"], "payment_not_confirmed");
    assert!(run_dirs(&server.out_dir()).is_empty());
    assert_eq!(server.pair.pay_calls(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_08_unpaid_run_streams_ordered_events_and_serves_both_reports() {
    let server = start(
        "unpaid",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let run_id = server.start_run("l402-e2e", None, false).await;
    server.wait_for_report(&run_id).await;

    let events = sse_events(&server.events(&run_id).await);
    let expected = Profile::L402E2e
        .tests()
        .iter()
        .map(|spec| spec.id)
        .collect::<Vec<&str>>();

    let steps = events
        .iter()
        .filter(|(name, _)| name == "step")
        .collect::<Vec<_>>();
    assert_eq!(steps.len(), expected.len() * 2);

    let started = steps
        .iter()
        .filter(|(_, data)| data["phase"] == "started")
        .map(|(_, data)| data["id"].as_str().unwrap())
        .collect::<Vec<&str>>();
    assert_eq!(started, expected);

    let (last_name, last) = events.last().expect("events");
    assert_eq!(last_name, "done");
    assert_eq!(last["run_id"], run_id.as_str());
    assert_eq!(last["exit_code"], 5);
    assert_eq!(server.pair.pay_calls(), 0);

    let json = server
        .client
        .get(server.url(&format!("/api/runs/{run_id}/report.json")))
        .send()
        .await
        .unwrap();
    assert_eq!(json.status(), 200);
    let report = json_body(json).await;
    assert_eq!(status_of(&report, "pay_invoice"), "NOT_TESTED");
    assert_eq!(reason_of(&report, "pay_invoice"), "PAYMENT_NOT_APPROVED");
    assert_eq!(
        report["environment"]["encryption_mode_selected"],
        "nip44_v2"
    );

    let html = server
        .client
        .get(server.url(&format!("/api/runs/{run_id}/report.html")))
        .send()
        .await
        .unwrap();
    assert_eq!(html.status(), 200);
    let body = html.text().await.unwrap();
    assert!(body.contains("<!DOCTYPE html>"));
    assert!(body.contains("pay_invoice"));
    assert!(body.contains("Overall verdict"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_09_paid_run_with_the_exact_confirmation_pays_once() {
    let server = start(
        "paid",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let (run_id, report) = server.run_to_end("l402-e2e", Some(21)).await;

    assert_eq!(server.pair.pay_calls(), 1);
    assert_eq!(status_of(&report, "pay_invoice"), "PASS");
    assert_eq!(status_of(&report, "preimage_verification"), "PASS");
    assert_eq!(status_of(&report, "protected_resource"), "PASS");

    let events = sse_events(&server.events(&run_id).await);
    let (_, last) = events.last().expect("events");
    assert_eq!(last["exit_code"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_10_a_second_post_while_running_is_a_conflict() {
    let server = start(
        "conflict",
        FakePair::new(FakeConfig {
            delay_ms: 400,
            ..FakeConfig::default()
        }),
        ready_wallets(),
    )
    .await;

    let first = server.post(serde_json::json!({ "profile": "basic" })).await;
    assert_eq!(first.status(), 202);

    let second = server.post(serde_json::json!({ "profile": "basic" })).await;
    assert_eq!(second.status(), 409);
    let value = json_body(second).await;
    assert_eq!(value["error"]["code"], "run_in_progress");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_11_bad_report_ids_never_read_a_file() {
    let server = start("ids", FakePair::new(FakeConfig::default()), ready_wallets()).await;
    let (run_id, _) = server.run_to_end("basic", None).await;

    for path in [
        "/api/runs/zzz/report.json".to_owned(),
        format!("/api/runs/{}/report.json", "z".repeat(32)),
        "/api/runs/..%2f..%2fCargo.toml/report.json".to_owned(),
        "/api/runs/0123456789abcdef0123456789abcdef/report.json".to_owned(),
        format!("/api/runs/{run_id}/report.json.evil"),
    ] {
        let response = server.client.get(server.url(&path)).send().await.unwrap();
        assert!(
            response.status() == 404 || response.status() == 400,
            "{path} gave {}",
            response.status()
        );
        let text = response.text().await.unwrap();
        assert!(!text.contains("[workspace]"), "{path} leaked a file");
        assert!(!text.contains("root:"), "{path} leaked a file");
    }

    // The real report is still served.
    let response = server
        .client
        .get(server.url(&format!("/api/runs/{run_id}/report.json")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_12_unknown_route_and_method_share_the_error_shape() {
    let server = start(
        "fallback",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let missing = server.client.get(server.url("/nope")).send().await.unwrap();
    assert_eq!(missing.status(), 404);
    let value = json_body(missing).await;
    assert_eq!(value["error"]["code"], "not_found");

    let wrong_method = server
        .client
        .delete(server.url("/api/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_method.status(), 405);
    let value = json_body(wrong_method).await;
    assert_eq!(value["error"]["code"], "method_not_allowed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_13_a_late_subscriber_gets_the_replay() {
    let server = start(
        "replay",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let (run_id, _) = server.run_to_end("basic", None).await;

    let events = sse_events(&server.events(&run_id).await);
    let steps = events.iter().filter(|(name, _)| name == "step").count();
    assert_eq!(steps, Profile::Basic.tests().len() * 2);
    assert_eq!(events.last().expect("events").0, "done");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_14_an_ambiguous_guard_file_blocks_until_cleared() {
    let server = start(
        "guard",
        FakePair::new(FakeConfig {
            pay: PayMode::Ambiguous,
            ..FakeConfig::default()
        }),
        ready_wallets(),
    )
    .await;

    // First paid run: the payment outcome is unknown, so the guard keeps it.
    let run_id = server.start_run("l402-e2e", Some(21), false).await;
    let path = server.wait_for_report(&run_id).await;
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).expect("report");
    assert_eq!(status_of(&report, "pay_invoice"), "BLOCKED");
    assert_eq!(
        reason_of(&report, "pay_invoice"),
        "AMBIGUOUS_PAYMENT_NOT_RETRIED"
    );

    // Second run without clearing: the guard blocks before any payment.
    let run_id = server.start_run("l402-e2e", Some(21), false).await;
    let path = server.wait_for_report(&run_id).await;
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).expect("report");
    assert_eq!(status_of(&report, "pay_invoice"), "BLOCKED");
    assert_eq!(reason_of(&report, "pay_invoice"), "RUN_GUARD_BLOCKED");
    assert_eq!(server.pair.pay_calls(), 1);

    // Third run with clear_guard: the guard is cleared and the wallet is asked
    // once more, still with an unknown outcome.
    let run_id = server.start_run("l402-e2e", Some(21), true).await;
    let path = server.wait_for_report(&run_id).await;
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).expect("report");
    assert_eq!(
        reason_of(&report, "pay_invoice"),
        "AMBIGUOUS_PAYMENT_NOT_RETRIED"
    );
    assert_eq!(server.pair.pay_calls(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_15_security_headers_on_dashboard_and_report() {
    let server = start(
        "headers",
        FakePair::new(FakeConfig::default()),
        ready_wallets(),
    )
    .await;

    let response = server.client.get(server.url("/")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    let csp = headers["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(!csp.contains("unsafe-inline"));

    let (run_id, _) = server.run_to_end("basic", None).await;
    let response = server
        .client
        .get(server.url(&format!("/api/runs/{run_id}/report.html")))
        .send()
        .await
        .unwrap();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(
        headers["content-security-policy"],
        "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_16_the_bound_address_is_loopback() {
    let config = ServerConfig {
        port: 0,
        ..ServerConfig::default()
    };

    let (_listener, address) = bind(&config).await.expect("bind");

    assert!(address.ip().is_loopback());
    assert_eq!(address.ip().to_string(), "127.0.0.1");
    assert_ne!(address.port(), 0);
}
