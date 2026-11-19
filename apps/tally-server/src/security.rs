//! The controls every request and every response passes through.
//!
//! The server binds only to 127.0.0.1, and on top of that it refuses any request
//! whose `Host` header is not this server (DNS rebinding), requires a custom
//! header on POST, checks `Origin` when the browser sends one, and marks every
//! response `no-store` and `nosniff`.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

/// The policy of the dashboard page and its two assets.
pub const DASHBOARD_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// The policy of a stored HTML report, which inlines its own styles.
pub const REPORT_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Builds the one JSON error shape every failure uses.
#[must_use]
pub fn error_response(state: &AppState, status: StatusCode, code: &str, message: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "code": code,
            "message": state.scrub(message),
        }
    });
    let text = serde_json::to_string(&body).unwrap_or_else(|_| String::from("{}"));

    let mut response = Response::new(Body::from(text));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn host_allowed(state: &AppState, host: &str) -> bool {
    let port = state.config.port;
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

fn check_request(state: &AppState, method: &Method, headers: &HeaderMap) -> Option<Response> {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return Some(error_response(
            state,
            StatusCode::FORBIDDEN,
            "forbidden",
            "the Host header is missing",
        ));
    };

    if !host_allowed(state, host) {
        return Some(error_response(
            state,
            StatusCode::FORBIDDEN,
            "forbidden",
            "the Host header does not name this server",
        ));
    }

    if *method != Method::POST {
        return None;
    }

    if headers
        .get("x-tally-request")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Some(error_response(
            state,
            StatusCode::FORBIDDEN,
            "forbidden",
            "the X-Tally-Request header is missing",
        ));
    }

    if let Some(origin) = headers.get(header::ORIGIN) {
        let expected = format!("http://{host}");
        if origin.to_str().ok() != Some(expected.as_str()) {
            return Some(error_response(
                state,
                StatusCode::FORBIDDEN,
                "forbidden",
                "the Origin header does not match this server",
            ));
        }
    }

    None
}

fn apply_headers(mut response: Response, path: &str) -> Response {
    let policy = if path == "/" || path.starts_with("/assets/") {
        Some(DASHBOARD_CSP)
    } else if path.ends_with("/report.html") {
        Some(REPORT_CSP)
    } else {
        None
    };

    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Some(policy) = policy {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(policy),
        );
    }

    response
}

/// Refuses a request before it reaches a handler, then marks the answer.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();

    if let Some(response) = check_request(&state, &method, request.headers()) {
        return apply_headers(response, &path);
    }

    apply_headers(next.run(request).await, &path)
}

/// A route that does not exist, in the shared error shape.
pub async fn not_found(State(state): State<AppState>) -> Response {
    error_response(&state, StatusCode::NOT_FOUND, "not_found", "no such route")
}

/// A route called with a method it does not serve.
pub async fn method_not_allowed(State(state): State<AppState>) -> Response {
    error_response(
        &state,
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "this route does not accept that method",
    )
}
