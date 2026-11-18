//! The local L402 test fixture: one protected route, bound to loopback.
//!
//! The fixture is a local test service. It proves Tally's flow, not
//! compatibility with third-party L402 services.
//!
//! Every failure answers the same way: status 402, a fresh challenge with a
//! new invoice, `Cache-Control: no-store` and the same JSON body, so the
//! server is not an oracle.

use std::fmt;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use axum::routing::get;
use secrecy::{ExposeSecret, SecretString};
use tally_nwc::WalletApi;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::challenge::{self, MAX_HEADER_BYTES};
use crate::invoice;
use crate::macaroon::{self, RootKey};

/// The body of the protected resource.
pub const RESOURCE_BODY: &str = "{\"resource\":\"tally-fixture-resource-v1\"}";
/// The body of a 402 answer.
pub const PAYMENT_REQUIRED_BODY: &str = "{\"error\":\"payment required\"}";
/// Description sent to the invoice source.
pub const INVOICE_DESCRIPTION: &str = "Tally L402 fixture";

/// How the fixture issues invoices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureConfig {
    /// Amount to ask for, in millisatoshis.
    pub amount_msat: u64,
    /// Seconds the invoice should stay valid.
    pub expiry_seconds: u64,
}

impl Default for FixtureConfig {
    fn default() -> Self {
        Self {
            amount_msat: 21_000,
            expiry_seconds: 300,
        }
    }
}

/// An invoice issued by a source, with the payment hash its macaroon binds.
pub struct SourcedInvoice {
    invoice: SecretString,
    payment_hash: [u8; 32],
}

impl SourcedInvoice {
    /// Builds the pair. The hash must be the invoice's own payment hash.
    #[must_use]
    pub fn new(invoice: SecretString, payment_hash: [u8; 32]) -> Self {
        Self {
            invoice,
            payment_hash,
        }
    }

    /// The invoice string, sensitive.
    #[must_use]
    pub fn invoice(&self) -> &SecretString {
        &self.invoice
    }

    /// The payment hash the macaroon binds.
    #[must_use]
    pub const fn payment_hash(&self) -> [u8; 32] {
        self.payment_hash
    }
}

impl fmt::Debug for SourcedInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourcedInvoice")
            .field("invoice", &"[REDACTED]")
            .field("payment_hash", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for SourcedInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sourced invoice (redacted)")
    }
}

/// Why a source could not issue an invoice. Sanitized: a fixed detail only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvoiceSourceError {
    detail: &'static str,
}

impl InvoiceSourceError {
    /// Builds an error with a fixed detail string.
    #[must_use]
    pub const fn new(detail: &'static str) -> Self {
        Self { detail }
    }

    /// The fixed detail, for logs.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        self.detail
    }
}

impl fmt::Display for InvoiceSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.detail)
    }
}

impl std::error::Error for InvoiceSourceError {}

/// Where the fixture gets invoices.
///
/// A generic parameter, not a boxed trait object: the implementation is chosen
/// at compile time and returns a `Send` future.
pub trait InvoiceSource {
    /// Issues an invoice for `amount_msat`, valid for `expiry_seconds`.
    fn make_invoice(
        &self,
        amount_msat: u64,
        expiry_seconds: u64,
    ) -> impl Future<Output = Result<SourcedInvoice, InvoiceSourceError>> + Send;
}

/// A source backed by a wallet, used as the receiver.
///
/// Generic over [`WalletApi`](tally_nwc::WalletApi): the real wallet and the
/// offline test wallet both fit, and the choice is made at compile time.
pub struct NwcInvoiceSource<W: WalletApi> {
    wallet: Arc<W>,
}

impl<W: WalletApi> NwcInvoiceSource<W> {
    /// Wraps a wallet.
    #[must_use]
    pub fn new(wallet: Arc<W>) -> Self {
        Self { wallet }
    }
}

impl<W: WalletApi> Clone for NwcInvoiceSource<W> {
    fn clone(&self) -> Self {
        Self {
            wallet: Arc::clone(&self.wallet),
        }
    }
}

impl<W> InvoiceSource for NwcInvoiceSource<W>
where
    W: WalletApi + Send + Sync + 'static,
{
    fn make_invoice(
        &self,
        amount_msat: u64,
        expiry_seconds: u64,
    ) -> impl Future<Output = Result<SourcedInvoice, InvoiceSourceError>> + Send {
        let wallet = Arc::clone(&self.wallet);

        async move {
            let made = wallet
                .make_invoice(amount_msat, INVOICE_DESCRIPTION, expiry_seconds)
                .await
                .map_err(|error| InvoiceSourceError::new(error.detail()))?;

            let facts = invoice::parse(made.invoice().expose_secret())
                .map_err(|_| InvoiceSourceError::new("invoice_source_unparseable"))?;

            Ok(SourcedInvoice::new(made.into_invoice(), facts.payment_hash))
        }
    }
}

/// Why the fixture could not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureError {
    detail: &'static str,
}

impl FixtureError {
    /// The fixed detail, for logs.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        self.detail
    }
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.detail)
    }
}

impl std::error::Error for FixtureError {}

#[derive(Clone)]
struct FixtureState<S> {
    root: Arc<RootKey>,
    config: FixtureConfig,
    source: S,
}

/// A running fixture.
pub struct FixtureHandle {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
}

impl FixtureHandle {
    /// The address the fixture listens on.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The base URL, for example `http://127.0.0.1:5000`.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The URL of the protected route.
    #[must_use]
    pub fn protected_url(&self) -> String {
        format!("{}/protected", self.base_url())
    }

    /// Stops the fixture and waits for the server task to finish.
    pub async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        let _ = self.join.await;
    }
}

impl fmt::Debug for FixtureHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixtureHandle")
            .field("addr", &self.addr)
            .finish()
    }
}

/// Starts the fixture on `127.0.0.1` with a port the operating system picks.
///
/// There is no way to bind anywhere else.
///
/// # Errors
///
/// [`FixtureError`] when the root key or the listener cannot be set up, or when
/// the bound address is somehow not loopback.
pub async fn spawn<S>(config: FixtureConfig, source: S) -> Result<FixtureHandle, FixtureError>
where
    S: InvoiceSource + Clone + Send + Sync + 'static,
{
    let root = RootKey::generate().map_err(|_| FixtureError {
        detail: "fixture_random_unavailable",
    })?;

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| FixtureError {
            detail: "fixture_bind_failed",
        })?;

    let addr = listener.local_addr().map_err(|_| FixtureError {
        detail: "fixture_address_unavailable",
    })?;

    if !addr.ip().is_loopback() {
        return Err(FixtureError {
            detail: "fixture_not_loopback",
        });
    }

    let state = FixtureState {
        root: Arc::new(root),
        config,
        source,
    };

    let app = Router::new()
        .route("/protected", get(protected::<S>))
        .with_state(state);

    let (sender, receiver) = oneshot::channel::<()>();

    let join = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = receiver.await;
        });
        let _ = server.await;
    });

    Ok(FixtureHandle {
        addr,
        shutdown: Some(sender),
        join,
    })
}

async fn protected<S>(State(state): State<FixtureState<S>>, headers: HeaderMap) -> Response<Body>
where
    S: InvoiceSource + Send + Sync + 'static,
{
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        let presentable = value.as_bytes().len() <= MAX_HEADER_BYTES;
        let accepted = presentable
            && value
                .to_str()
                .ok()
                .and_then(|text| challenge::parse_authorization(text).ok())
                .is_some_and(|credentials| {
                    macaroon::verify(
                        &state.root,
                        credentials.macaroon_b64(),
                        credentials.preimage_hex(),
                    )
                    .is_ok()
                });

        if accepted {
            return resource_response();
        }
    }

    new_challenge(&state).await
}

fn resource_response() -> Response<Body> {
    let mut response = Response::new(Body::from(RESOURCE_BODY));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn new_challenge<S>(state: &FixtureState<S>) -> Response<Body>
where
    S: InvoiceSource,
{
    let Ok(sourced) = state
        .source
        .make_invoice(state.config.amount_msat, state.config.expiry_seconds)
        .await
    else {
        return internal_error_response();
    };

    let Ok(user_id) = macaroon::new_user_id() else {
        return internal_error_response();
    };

    let Ok(token) = macaroon::mint(&state.root, sourced.payment_hash(), user_id) else {
        return internal_error_response();
    };

    let Ok(header_value) =
        challenge::format_www_authenticate(&token, sourced.invoice().expose_secret())
    else {
        return internal_error_response();
    };

    let Ok(header_value) = HeaderValue::from_str(&header_value) else {
        return internal_error_response();
    };

    let mut response = Response::new(Body::from(PAYMENT_REQUIRED_BODY));
    *response.status_mut() = StatusCode::PAYMENT_REQUIRED;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, header_value);
    response
}

fn internal_error_response() -> Response<Body> {
    let mut response = Response::new(Body::from("{\"error\":\"fixture failed\"}"));
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response
}
