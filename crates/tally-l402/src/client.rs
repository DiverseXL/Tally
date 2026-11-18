//! The L402 client: fetch a challenge, then fetch again with credentials.
//!
//! Plain HTTP only, no redirects, and only loopback hosts in this step. The
//! macaroon is treated as an opaque string; only the fixture's verifier knows
//! its contents.

use std::fmt;
use std::time::Duration;

use reqwest::Url;
use reqwest::header::{AUTHORIZATION, HeaderValue, WWW_AUTHENTICATE};
use reqwest::redirect::Policy;

use crate::challenge::{self, Challenge};

/// Connect timeout of the client.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Total timeout of one request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Why a client call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientErrorKind {
    /// The URL is not `http://` with a loopback host.
    UrlNotAllowed,
    /// The HTTP client could not be built.
    ClientBuild,
    /// The request failed below HTTP.
    Transport,
    /// The answer was not the 402 a challenge is expected from.
    UnexpectedStatus,
    /// The 402 carried no usable `WWW-Authenticate` header.
    MissingChallenge,
    /// The headers could not be turned into a request.
    BadRequest,
}

impl ClientErrorKind {
    /// A stable short name, for logs and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UrlNotAllowed => "url_not_allowed",
            Self::ClientBuild => "client_build_failed",
            Self::Transport => "transport_failed",
            Self::UnexpectedStatus => "unexpected_status",
            Self::MissingChallenge => "missing_challenge",
            Self::BadRequest => "bad_request",
        }
    }
}

/// A sanitized client error. It never holds a URL, a header value or a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientError {
    kind: ClientErrorKind,
    status: Option<u16>,
}

impl ClientError {
    const fn new(kind: ClientErrorKind) -> Self {
        Self { kind, status: None }
    }

    const fn with_status(kind: ClientErrorKind, status: u16) -> Self {
        Self {
            kind,
            status: Some(status),
        }
    }

    /// What went wrong, as a kind.
    #[must_use]
    pub const fn kind(&self) -> ClientErrorKind {
        self.kind
    }

    /// The HTTP status that caused the failure, when there was one.
    #[must_use]
    pub const fn status(&self) -> Option<u16> {
        self.status
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.as_str())
    }
}

impl std::error::Error for ClientError {}

/// What a request returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientResponse {
    /// The HTTP status.
    pub status: u16,
    /// The body, as a string.
    pub body: String,
}

/// An HTTP client limited to loopback plain-HTTP URLs.
#[derive(Debug, Clone)]
pub struct L402Client {
    http: reqwest::Client,
}

impl L402Client {
    /// Builds the client: no redirects, a 5 s connect timeout and a 15 s total
    /// timeout.
    ///
    /// # Errors
    ///
    /// [`ClientErrorKind::ClientBuild`] when reqwest refuses the settings.
    pub fn new() -> Result<Self, ClientError> {
        let http = reqwest::Client::builder()
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| ClientError::new(ClientErrorKind::ClientBuild))?;

        Ok(Self { http })
    }

    /// Requests the URL without credentials and reads the challenge.
    ///
    /// # Errors
    ///
    /// [`ClientErrorKind::UrlNotAllowed`] for a non-loopback or non-http URL,
    /// [`ClientErrorKind::UnexpectedStatus`] when the answer is not 402,
    /// [`ClientErrorKind::MissingChallenge`] when the 402 carries no usable
    /// `WWW-Authenticate` value.
    pub async fn fetch_challenge(&self, url: &str) -> Result<Challenge, ClientError> {
        let url = guard_url(url)?;

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| ClientError::new(ClientErrorKind::Transport))?;

        let status = response.status().as_u16();

        if status != 402 {
            return Err(ClientError::with_status(
                ClientErrorKind::UnexpectedStatus,
                status,
            ));
        }

        let header = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .ok_or(ClientError::new(ClientErrorKind::MissingChallenge))?;

        let parsed = challenge::parse_www_authenticate(header)
            .map_err(|_| ClientError::new(ClientErrorKind::MissingChallenge))?;

        Ok(Challenge::new(
            status,
            parsed.scheme().to_owned(),
            parsed.macaroon_b64().to_owned(),
            parsed.invoice().to_owned(),
        ))
    }

    /// Requests the URL with one macaroon and one preimage.
    ///
    /// # Errors
    ///
    /// [`ClientErrorKind::UrlNotAllowed`] for a non-loopback or non-http URL,
    /// [`ClientErrorKind::BadRequest`] when the credentials cannot be sent.
    pub async fn fetch_with_credentials(
        &self,
        url: &str,
        macaroon_b64: &str,
        preimage_hex: &str,
    ) -> Result<ClientResponse, ClientError> {
        let url = guard_url(url)?;

        let header = challenge::format_authorization(macaroon_b64, preimage_hex)
            .map_err(|_| ClientError::new(ClientErrorKind::BadRequest))?;

        let header = HeaderValue::from_str(&header)
            .map_err(|_| ClientError::new(ClientErrorKind::BadRequest))?;

        let response = self
            .http
            .get(url)
            .header(AUTHORIZATION, header)
            .send()
            .await
            .map_err(|_| ClientError::new(ClientErrorKind::Transport))?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|_| ClientError::new(ClientErrorKind::Transport))?;

        Ok(ClientResponse { status, body })
    }
}

/// Whether the URL is `http://` on `127.0.0.1` or `localhost`.
#[must_use]
pub fn is_allowed_url(url: &str) -> bool {
    guard_url(url).is_ok()
}

/// Accepts only `http://127.0.0.1` or `http://localhost` URLs.
fn guard_url(url: &str) -> Result<Url, ClientError> {
    let parsed = Url::parse(url).map_err(|_| ClientError::new(ClientErrorKind::UrlNotAllowed))?;

    if parsed.scheme() != "http" {
        return Err(ClientError::new(ClientErrorKind::UrlNotAllowed));
    }

    match parsed.host_str() {
        Some("127.0.0.1" | "localhost") => Ok(parsed),
        _ => Err(ClientError::new(ClientErrorKind::UrlNotAllowed)),
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientErrorKind, L402Client, is_allowed_url};

    #[test]
    fn refuses_https_and_non_loopback_urls() {
        assert!(!is_allowed_url("https://127.0.0.1:1234/protected"));
        assert!(!is_allowed_url("http://example.com/protected"));
        assert!(!is_allowed_url("http://192.168.0.2/protected"));
        assert!(!is_allowed_url("not a url"));
        assert!(is_allowed_url("http://127.0.0.1:1234/protected"));
        assert!(is_allowed_url("http://localhost:1234/protected"));
    }

    #[test]
    fn client_builds_with_timeouts() {
        assert!(L402Client::new().is_ok());
        assert_eq!(ClientErrorKind::UrlNotAllowed.as_str(), "url_not_allowed");
    }
}
