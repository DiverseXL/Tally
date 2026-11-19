//! The `tally-server` command: start the loopback-only dashboard server.
//!
//! Nothing here prints or stores a connection string; the line printed at start
//! is the URL only.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use tally_server::{AppState, RealExecutor, ServerConfig, app, bind, load_wallets};

/// Exit code of a configuration error before the server starts.
const EXIT_CONFIG: u8 = 2;

#[derive(Parser)]
#[command(
    name = "tally-server",
    version,
    about = "Run Tally from a loopback-only local dashboard",
    long_about = "Run Tally from a loopback-only local dashboard.\n\nSandbox wallets only: this version has no live mode and no flag accepts a connection string. The server binds 127.0.0.1 and nothing else."
)]
struct Cli {
    /// Port on 127.0.0.1; 0 lets the operating system pick one.
    #[arg(long, default_value_t = tally_server::DEFAULT_PORT)]
    port: u16,

    /// Dotenv file of the paying wallet. TALLY_PAYER_NWC_URI wins over it.
    #[arg(long)]
    payer_file: Option<PathBuf>,

    /// Dotenv file of the receiving wallet. TALLY_RECEIVER_NWC_URI wins over it.
    #[arg(long)]
    receiver_file: Option<PathBuf>,

    /// Where reports are written.
    #[arg(long)]
    out_dir: Option<PathBuf>,

    /// Where run-guard state files live.
    #[arg(long)]
    guard_dir: Option<PathBuf>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let mut config = ServerConfig {
        port: cli.port,
        ..ServerConfig::default()
    };
    if let Some(file) = cli.payer_file {
        config.payer_file = file;
    }
    if let Some(file) = cli.receiver_file {
        config.receiver_file = file;
    }
    if let Some(dir) = cli.out_dir {
        config.out_dir = dir;
    }
    if let Some(dir) = cli.guard_dir {
        config.guard_dir = dir;
    }

    let (listener, address) = match bind(&config).await {
        Ok(bound) => bound,
        Err(error) => {
            eprintln!("configuration error: cannot bind 127.0.0.1: {error}");
            return ExitCode::from(EXIT_CONFIG);
        }
    };

    // The Host check needs the port the socket really got, not the one asked for.
    config.port = address.port();

    let wallets = load_wallets(&config);
    let config = Arc::new(config);
    let executor = Arc::new(RealExecutor::new(Arc::clone(&config), wallets.clone()));
    let state = AppState::new(
        Arc::try_unwrap(config).unwrap_or_else(|shared| (*shared).clone()),
        wallets,
        executor,
    );

    println!(
        "tally-server listening on http://127.0.0.1:{}",
        address.port()
    );

    if let Err(error) = axum::serve(listener, app(state)).await {
        eprintln!("server error: {error}");
        return ExitCode::from(EXIT_CONFIG);
    }

    ExitCode::SUCCESS
}
