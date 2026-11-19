//! The local Tally server: a loopback-only Axum app that runs Tally's profiles
//! and serves the dashboard.
//!
//! Sandbox only in this version. There is no live mode, no flag accepts a
//! connection string and no host flag exists: [`bind`] binds 127.0.0.1 and
//! checks the address it got.

pub mod api;
pub mod config;
pub mod events;
pub mod security;
pub mod state;

pub use api::app;
pub use config::{
    DEFAULT_PORT, DoctorFacts, PAYMENT_AMOUNT_MSAT, SecretRef, ServerConfig, WalletFacts,
    WalletSet, load_wallets,
};
pub use events::{EventSink, LoggedEvent, RunLog, Runs};
pub use state::{AppState, RealExecutor, RunExecutor, RunRequest};

use std::io;
use std::net::SocketAddr;

use tokio::net::TcpListener;

/// Binds this server to 127.0.0.1 and proves the address is loopback.
///
/// Port 0 asks the operating system for a free port; the caller reads the real
/// one from the returned address.
///
/// # Errors
///
/// The error of [`TcpListener::bind`].
///
/// # Panics
///
/// Panics if the bound address is not a loopback address, which would mean the
/// request for 127.0.0.1 was not honoured.
pub async fn bind(config: &ServerConfig) -> io::Result<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind(("127.0.0.1", config.port)).await?;
    let address = listener.local_addr()?;
    assert!(
        address.ip().is_loopback(),
        "tally-server must bind a loopback address"
    );

    Ok((listener, address))
}
