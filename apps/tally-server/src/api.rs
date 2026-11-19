//! The HTTP surface: dashboard, status, runs, the event stream and the two
//! stored report files.
//!
//! Everything here is loopback-only and sandbox-only. No handler accepts a
//! connection string, and every response body built here is scrubbed before it
//! leaves the process.

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router, middleware};
use serde::{Deserialize, Serialize};
use tally_core::report::{RunReport, exit_code};
use tally_core::time::new_run_id;
use tally_runner::Profile;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::events::{EventSink, LoggedEvent};
use crate::security::{self, error_response, guard};
use crate::state::{AppState, RunRequest};

/// Largest request body the server accepts.
pub const MAX_BODY_BYTES: usize = 4 * 1024;

/// How many runs the history lists.
pub const MAX_RUNS: usize = 20;

/// The dashboard page, embedded at compile time.
const DASHBOARD_HTML: &str = include_str!("../../../web/index.html");
/// The dashboard script, embedded at compile time.
const DASHBOARD_JS: &str = include_str!("../../../web/dist/app.js");
/// The dashboard stylesheet, embedded at compile time.
const DASHBOARD_CSS: &str = include_str!("../../../web/styles.css");

/// Builds the whole server.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/assets/app.js", get(script))
        .route("/assets/styles.css", get(stylesheet))
        .route("/api/status", get(status))
        .route("/api/runs", get(list_runs).post(start_run))
        .route("/api/runs/{id}/events", get(run_events))
        .route("/api/runs/{id}/report.json", get(report_json))
        .route("/api/runs/{id}/report.html", get(report_html))
        .fallback(security::not_found)
        .method_not_allowed_fallback(security::method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

fn text_response(
    state: &AppState,
    status: StatusCode,
    content_type: &'static str,
    text: &str,
) -> Response {
    let mut response = Response::new(Body::from(state.scrub(text)));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn json_response(state: &AppState, status: StatusCode, value: &serde_json::Value) -> Response {
    let text = serde_json::to_string(value).unwrap_or_else(|_| String::from("{}"));
    text_response(state, status, "application/json", &text)
}

async fn dashboard(State(state): State<AppState>) -> Response {
    text_response(
        &state,
        StatusCode::OK,
        "text/html; charset=utf-8",
        DASHBOARD_HTML,
    )
}

async fn script(State(state): State<AppState>) -> Response {
    text_response(
        &state,
        StatusCode::OK,
        "text/javascript; charset=utf-8",
        DASHBOARD_JS,
    )
}

async fn stylesheet(State(state): State<AppState>) -> Response {
    text_response(
        &state,
        StatusCode::OK,
        "text/css; charset=utf-8",
        DASHBOARD_CSS,
    )
}

async fn status(State(state): State<AppState>) -> Response {
    let doctor = state.doctor();
    let ready = doctor.ready();
    let amount_msat = state.config.amount_msat;

    let value = serde_json::json!({
        "version": tally_runner::TALLY_VERSION,
        "environment": "sandbox",
        "payment_amount_sats": amount_msat / 1_000,
        "doctor": doctor,
        "ready": ready,
        "current_run": state.runs.current(),
        "latest_run": latest_run(&state.config.out_dir),
    });

    json_response(&state, StatusCode::OK, &value)
}

/// One line of the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunEntry {
    /// The run id, which is the report directory name.
    pub run_id: String,
    /// The profile, derived from the test ids the report carries.
    pub profile: &'static str,
    /// The exit code the run produced.
    pub exit_code: i32,
    /// When the run started, as the report stored it.
    pub started_at: String,
}

fn is_run_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

fn read_report(dir: &Path) -> Option<RunReport> {
    let text = std::fs::read_to_string(dir.join("report.json")).ok()?;
    serde_json::from_str::<RunReport>(&text).ok()
}

fn runs_in(out_dir: &Path) -> Vec<RunEntry> {
    let Ok(entries) = std::fs::read_dir(out_dir) else {
        return Vec::new();
    };

    let mut runs = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_run_id(&name) {
            continue;
        }
        let Some(report) = read_report(&entry.path()) else {
            continue;
        };
        // The report has no profile field; a challenge test exists only in the
        // L402 profile.
        let profile = if report.tests.iter().any(|test| test.id == "challenge") {
            "l402-e2e"
        } else {
            "basic"
        };
        runs.push(RunEntry {
            run_id: name,
            profile,
            exit_code: exit_code(&report),
            started_at: report.started_at,
        });
    }

    runs.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    runs.truncate(MAX_RUNS);
    runs
}

fn latest_run(out_dir: &Path) -> Option<String> {
    runs_in(out_dir).first().map(|run| run.run_id.clone())
}

async fn list_runs(State(state): State<AppState>) -> Response {
    let runs = runs_in(&state.config.out_dir);
    let value = serde_json::to_value(&runs).unwrap_or_else(|_| serde_json::json!([]));

    json_response(&state, StatusCode::OK, &value)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRunBody {
    profile: String,
    #[serde(default)]
    approve_payment_sats: Option<u64>,
    #[serde(default)]
    confirm: Option<String>,
    #[serde(default)]
    clear_guard: bool,
}

async fn start_run(
    State(state): State<AppState>,
    body: Result<Json<StartRunBody>, JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            let (status, message) = match rejection.status() {
                StatusCode::PAYLOAD_TOO_LARGE => (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "the request body is too large",
                ),
                StatusCode::UNSUPPORTED_MEDIA_TYPE => (
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "the request body must be application/json",
                ),
                _ => (
                    StatusCode::BAD_REQUEST,
                    "the request body must be a JSON object with the documented fields",
                ),
            };
            return error_response(&state, status, "invalid_request", message);
        }
    };

    let Some(profile) = Profile::parse(&body.profile) else {
        return error_response(
            &state,
            StatusCode::BAD_REQUEST,
            "unknown_profile",
            "the profile must be basic or l402-e2e",
        );
    };

    if let Some(sats) = body.approve_payment_sats {
        let expected = format!("PAY {sats} SATS");
        if body.confirm.as_deref() != Some(expected.as_str()) {
            return error_response(
                &state,
                StatusCode::BAD_REQUEST,
                "payment_not_confirmed",
                "the confirmation text does not match the payment amount",
            );
        }
    }

    let doctor = state.doctor();
    if !doctor.ready() {
        return error_response(
            &state,
            StatusCode::SERVICE_UNAVAILABLE,
            "not_ready",
            "the wallets or the directories are not configured",
        );
    }

    let run_id = new_run_id();
    let Some(log) = state.runs.begin(&run_id) else {
        return error_response(
            &state,
            StatusCode::CONFLICT,
            "run_in_progress",
            "a run is already in progress",
        );
    };

    let request = RunRequest {
        run_id: run_id.clone(),
        profile,
        approve_payment_sats: body.approve_payment_sats,
        clear_guard: body.clear_guard,
    };

    let sink = EventSink::new(log, state.scrubber.clone());
    let executor = state.executor.clone();
    let runs = state.runs.clone();
    let id = run_id.clone();

    tokio::spawn(async move {
        let code = match executor.execute(request, sink).await {
            Ok(outcome) => exit_code(&outcome.report),
            Err(error) => {
                tracing::warn!(event = "run_failed", detail = %error, "run failed");
                2
            }
        };
        if let Some(log) = runs.get(&id) {
            log.finish(&id, code);
        }
        runs.finish(&id);
    });

    json_response(
        &state,
        StatusCode::ACCEPTED,
        &serde_json::json!({ "run_id": run_id }),
    )
}

fn sse_event(event: &LoggedEvent) -> Event {
    Event::default().event(event.name()).data(event.data())
}

async fn run_events(State(state): State<AppState>, UrlPath(id): UrlPath<String>) -> Response {
    let Some(log) = state.runs.get(&id) else {
        return error_response(&state, StatusCode::NOT_FOUND, "not_found", "no such run");
    };

    let (replay, mut receiver, done) = log.snapshot_and_subscribe();
    let (tx, out) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        for event in replay {
            if tx.send(Ok(sse_event(&event))).await.is_err() {
                return;
            }
        }
        if done {
            return;
        }

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let finished = matches!(event, LoggedEvent::Done(_));
                    if tx.send(Ok(sse_event(&event))).await.is_err() {
                        return;
                    }
                    if finished {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            }
        }
    });

    Sse::new(ReceiverStream::new(out))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

fn report_path(out_dir: &Path, id: &str, file: &str) -> Option<PathBuf> {
    if !is_run_id(id) {
        return None;
    }

    Some(out_dir.join(id).join(file))
}

async fn report_json(State(state): State<AppState>, UrlPath(id): UrlPath<String>) -> Response {
    let Some(path) = report_path(&state.config.out_dir, &id, "report.json") else {
        return error_response(&state, StatusCode::NOT_FOUND, "not_found", "no such report");
    };

    match std::fs::read(&path) {
        Ok(bytes) => text_response(
            &state,
            StatusCode::OK,
            "application/json",
            &String::from_utf8_lossy(&bytes),
        ),
        Err(_) => error_response(&state, StatusCode::NOT_FOUND, "not_found", "no such report"),
    }
}

async fn report_html(State(state): State<AppState>, UrlPath(id): UrlPath<String>) -> Response {
    let Some(path) = report_path(&state.config.out_dir, &id, "report.html") else {
        return error_response(&state, StatusCode::NOT_FOUND, "not_found", "no such report");
    };

    match std::fs::read(&path) {
        Ok(bytes) => text_response(
            &state,
            StatusCode::OK,
            "text/html; charset=utf-8",
            &String::from_utf8_lossy(&bytes),
        ),
        Err(_) => error_response(&state, StatusCode::NOT_FOUND, "not_found", "no such report"),
    }
}
