//! The `tally` command line interface.
//!
//! Connection strings come only from the environment or from the two dotenv
//! files; no flag accepts one, and no command prints a value from them.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use secrecy::SecretString;
use tally_core::report::{SCHEMA_VERSION, exit_code};
use tally_nwc::{SessionOptions, WalletSession};
use tally_runner::{
    DEFAULT_WALLET_REQUIRED, Profile, RunConfig, guard, run, run_wallet, sanitize_label,
    summary_lines, to_html, to_json,
};

/// Exit code of a usage or configuration error before any test runs.
const EXIT_USAGE: u8 = 2;

#[derive(Parser)]
#[command(name = "tally", version, about = "Test a wallet connected through NWC")]
struct Cli {
    /// Increase log detail on stderr: once for info.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the configuration without any network call.
    Doctor(ConnectionArgs),
    /// Run the basic profile and print the compact summary.
    Inspect {
        /// Profile to run: `basic` or `wallet`.
        #[arg(long)]
        profile: String,
        #[command(flatten)]
        connection: ConnectionArgs,
        /// Dotenv file holding one wallet connection string; only with `--profile wallet`.
        #[arg(long)]
        wallet_file: Option<PathBuf>,
        /// Label of a single-wallet run, sanitized to [a-z0-9-] and at most 24 characters.
        #[arg(long)]
        label: Option<String>,
        /// Required methods, comma separated; replaces the default list.
        #[arg(long)]
        require: Option<String>,
        /// Where reports are written.
        #[arg(long)]
        out_dir: Option<PathBuf>,
    },
    /// Run a profile against the two wallets.
    Test {
        /// `basic` or `l402-e2e`.
        #[arg(long)]
        profile: String,
        /// `sandbox` or `live`.
        #[arg(long, default_value = "sandbox")]
        env: String,
        /// Sats approved for this run; without it the payment tests are not run.
        #[arg(long)]
        approve_payment_sats: Option<u64>,
        /// The live acknowledgement string.
        #[arg(long)]
        live_ack: Option<String>,
        /// Non-interactive; no prompt is ever shown anyway.
        #[arg(long)]
        ci: bool,
        /// Allow clearing stale PAYING and AMBIGUOUS guard files.
        #[arg(long)]
        clear_guard: bool,
        /// Where reports are written.
        #[arg(long)]
        out_dir: Option<PathBuf>,
        /// Where run-guard state files live.
        #[arg(long)]
        guard_dir: Option<PathBuf>,
        #[command(flatten)]
        connection: ConnectionArgs,
    },
    /// Re-render a stored JSON report.
    Report {
        /// Path of the stored JSON report.
        #[arg(long)]
        input: PathBuf,
        /// `json` or `html`.
        #[arg(long)]
        format: String,
        /// Path of the output file.
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Args)]
struct ConnectionArgs {
    /// Dotenv file of the paying wallet. The environment wins over it.
    #[arg(long)]
    payer_file: Option<PathBuf>,
    /// Dotenv file of the receiving wallet. The environment wins over it.
    #[arg(long)]
    receiver_file: Option<PathBuf>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let code = match cli.command {
        Command::Doctor(connection) => doctor(&connection),
        Command::Inspect {
            profile,
            connection,
            wallet_file,
            label,
            require,
            out_dir,
        } => {
            let Some(profile) = Profile::parse(&profile) else {
                eprintln!("unknown profile: {}", profile);
                return ExitCode::from(EXIT_USAGE);
            };
            if profile == Profile::Wallet {
                inspect_wallet(wallet_file, label, require, out_dir).await
            } else {
                if wallet_file.is_some() || label.is_some() || require.is_some() {
                    eprintln!(
                        "configuration error: --wallet-file, --label and --require need --profile wallet"
                    );
                    return ExitCode::from(EXIT_USAGE);
                }
                run_and_print(profile, connection, out_dir, None).await
            }
        }
        Command::Test {
            profile,
            env,
            approve_payment_sats,
            live_ack,
            ci,
            clear_guard,
            out_dir,
            guard_dir,
            connection,
        } => {
            let Some(profile) = Profile::parse(&profile) else {
                eprintln!("unknown profile: {}", profile);
                return ExitCode::from(EXIT_USAGE);
            };
            let Some(env_class) = parse_env(&env) else {
                eprintln!("unknown environment class: {}", env);
                return ExitCode::from(EXIT_USAGE);
            };
            let guard_dir = guard_dir.unwrap_or_else(|| PathBuf::from("runs"));
            let mut config = RunConfig {
                env_class,
                approve_payment_sats,
                live_ack,
                out_dir: out_dir.unwrap_or_else(|| PathBuf::from("reports")),
                guard_dir,
                clear_guard,
                ..RunConfig::default()
            };
            let _ = ci;
            config.timeouts = SessionOptions::default();
            run_and_print(profile, connection, None, Some(config)).await
        }
        Command::Report { input, format, out } => report(&input, &format, &out),
    };

    ExitCode::from(code)
}

fn init_tracing(verbose: u8) {
    use tracing_subscriber::filter::LevelFilter;

    let level = if verbose > 0 {
        LevelFilter::INFO
    } else {
        LevelFilter::WARN
    };

    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .try_init();
}

fn parse_env(value: &str) -> Option<tally_core::report::EnvClass> {
    match value {
        "sandbox" => Some(tally_core::report::EnvClass::Sandbox),
        "live" => Some(tally_core::report::EnvClass::Live),
        _ => None,
    }
}

/// One resolved connection string and where it came from.
struct Resolved {
    secret: SecretString,
    source: String,
}

fn resolve(variable: &str, file: &Path, key: &str) -> Result<Option<Resolved>, std::io::Error> {
    if let Some(value) = env::var_os(variable) {
        let value = value.to_string_lossy().trim().to_owned();
        if !value.is_empty() {
            return Ok(Some(Resolved {
                secret: SecretString::from(value),
                source: String::from("env"),
            }));
        }
    }

    let Ok(text) = std::fs::read_to_string(file) else {
        return Ok(None);
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        let value = value.trim().to_owned();
        if value.is_empty() {
            return Ok(None);
        }
        return Ok(Some(Resolved {
            secret: SecretString::from(value),
            source: format!("file {}", file.display()),
        }));
    }

    Ok(None)
}

fn resolve_both(connection: &ConnectionArgs) -> (Option<Resolved>, Option<Resolved>) {
    let payer_file = connection
        .payer_file
        .clone()
        .unwrap_or_else(|| PathBuf::from(".env.payer"));
    let receiver_file = connection
        .receiver_file
        .clone()
        .unwrap_or_else(|| PathBuf::from(".env.receiver"));

    let payer = resolve("TALLY_PAYER_NWC_URI", &payer_file, "TALLY_PAYER_NWC_URI").unwrap_or(None);
    let receiver = resolve(
        "TALLY_RECEIVER_NWC_URI",
        &receiver_file,
        "TALLY_RECEIVER_NWC_URI",
    )
    .unwrap_or(None);

    (payer, receiver)
}

/// One connection string taken from the environment or from a one-line file.
fn resolve_wallet(wallet_file: Option<&Path>) -> Result<Option<SecretString>, ()> {
    if let Some(value) = env::var_os("TALLY_WALLET_NWC_URI") {
        let value = value.to_string_lossy().trim().to_owned();
        if !value.is_empty() {
            return Ok(Some(SecretString::from(value)));
        }
    }

    let Some(path) = wallet_file else {
        return Ok(None);
    };

    let Ok(text) = std::fs::read_to_string(path) else {
        return Err(());
    };

    let mut found = None;
    let mut lines = 0;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        lines += 1;
        if lines > 1 {
            return Err(());
        }
        let Some((name, value)) = line.split_once('=') else {
            return Err(());
        };
        let name = name.trim();
        if !matches!(
            name,
            "TALLY_WALLET_NWC_URI" | "TALLY_PAYER_NWC_URI" | "TALLY_RECEIVER_NWC_URI"
        ) {
            return Err(());
        }
        let value = value.trim();
        if value.is_empty() {
            return Err(());
        }
        found = Some(SecretString::from(value.to_owned()));
    }

    if lines == 1 { Ok(found) } else { Err(()) }
}

fn parse_required(list: &str) -> Vec<String> {
    list.split(',')
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn inspect_wallet(
    wallet_file: Option<PathBuf>,
    label: Option<String>,
    require: Option<String>,
    out_dir: Option<PathBuf>,
) -> u8 {
    let wallet = match resolve_wallet(wallet_file.as_deref()) {
        Ok(Some(wallet)) => wallet,
        Ok(None) => {
            eprintln!("configuration error: no wallet connection string is configured");
            return EXIT_USAGE;
        }
        Err(()) => {
            eprintln!("configuration error: the wallet file must hold one connection string line");
            return EXIT_USAGE;
        }
    };

    let Ok(mut session) = WalletSession::new(&wallet) else {
        eprintln!("configuration error: the wallet connection string does not parse");
        return EXIT_USAGE;
    };

    let wallet_label = match label {
        Some(label) => match sanitize_label(&label) {
            Some(label) => Some(label),
            None => {
                eprintln!("configuration error: the label has no usable characters");
                return EXIT_USAGE;
            }
        },
        None => None,
    };

    let wallet_required = match require {
        Some(list) => parse_required(&list),
        None => DEFAULT_WALLET_REQUIRED
            .iter()
            .map(|method| String::from(*method))
            .collect(),
    };

    let config = RunConfig {
        out_dir: out_dir.unwrap_or_else(|| PathBuf::from("reports")),
        wallet_label,
        wallet_required,
        ..RunConfig::default()
    };

    let outcome = match run_wallet(&mut session, &config).await {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("run error: {error}");
            return EXIT_USAGE;
        }
    };

    for line in summary_lines(&outcome) {
        println!("{line}");
    }

    let code = exit_code(&outcome.report);
    u8::try_from(code).unwrap_or(EXIT_USAGE)
}

fn doctor(connection: &ConnectionArgs) -> u8 {
    let (payer, receiver) = resolve_both(connection);

    let payer_parses = payer
        .as_ref()
        .is_some_and(|resolved| WalletSession::new(&resolved.secret).is_ok());
    let receiver_parses = receiver
        .as_ref()
        .is_some_and(|resolved| WalletSession::new(&resolved.secret).is_ok());

    println!(
        "payer source={} parses={}",
        payer
            .as_ref()
            .map_or("none", |resolved| resolved.source.as_str()),
        payer_parses
    );
    println!(
        "receiver source={} parses={}",
        receiver
            .as_ref()
            .map_or("none", |resolved| resolved.source.as_str()),
        receiver_parses
    );
    println!("environment sandbox");

    let out_dir = PathBuf::from("reports");
    let guard_dir = PathBuf::from("runs");
    let out_writable = dir_writable(&out_dir);
    let guard_writable = dir_writable(&guard_dir);
    println!("out_dir {} writable={out_writable}", out_dir.display());
    println!(
        "guard_dir {} writable={guard_writable}",
        guard_dir.display()
    );

    let blocking = guard::blocking_files(&guard_dir).unwrap_or_default();
    if blocking.is_empty() {
        println!("guard_files blocking=0");
    } else {
        println!("guard_files blocking={}", blocking.len());
        for file in &blocking {
            println!("guard_state {}", file.state.as_str());
        }
    }

    let ready = payer_parses && receiver_parses && out_writable && guard_writable;
    println!("ready={ready}");

    if ready { 0 } else { EXIT_USAGE }
}

fn dir_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }

    let probe = dir.join(".tally-write-probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

async fn run_and_print(
    profile: Profile,
    connection: ConnectionArgs,
    out_dir: Option<PathBuf>,
    config: Option<RunConfig>,
) -> u8 {
    let (payer, receiver) = resolve_both(&connection);

    let Some(payer) = payer else {
        eprintln!("configuration error: the payer connection string is missing");
        return EXIT_USAGE;
    };
    let Some(receiver) = receiver else {
        eprintln!("configuration error: the receiver connection string is missing");
        return EXIT_USAGE;
    };

    let Ok(mut payer_session) = WalletSession::new(&payer.secret) else {
        eprintln!("configuration error: the payer connection string does not parse");
        return EXIT_USAGE;
    };
    let Ok(receiver_session) = WalletSession::new(&receiver.secret) else {
        eprintln!("configuration error: the receiver connection string does not parse");
        return EXIT_USAGE;
    };

    let mut config = config.unwrap_or_default();
    if let Some(dir) = out_dir {
        config.out_dir = dir;
    }

    let outcome = match run(
        profile,
        &mut payer_session,
        Arc::new(receiver_session),
        &config,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("run error: {error}");
            return EXIT_USAGE;
        }
    };

    for line in summary_lines(&outcome) {
        println!("{line}");
    }

    let code = exit_code(&outcome.report);
    u8::try_from(code).unwrap_or(EXIT_USAGE)
}

fn report(input: &Path, format: &str, out: &Path) -> u8 {
    let Ok(text) = std::fs::read_to_string(input) else {
        eprintln!("cannot read the report");
        return EXIT_USAGE;
    };

    let Ok(report) = serde_json::from_str::<tally_core::report::RunReport>(&text) else {
        eprintln!("the input is not a Tally report");
        return EXIT_USAGE;
    };

    if report.schema_version != SCHEMA_VERSION {
        eprintln!(
            "unsupported schema_version {}: this build reads {}",
            report.schema_version, SCHEMA_VERSION
        );
        return EXIT_USAGE;
    }

    let rendered = match format {
        "json" => to_json(&report),
        "html" => to_html(&report),
        _ => {
            eprintln!("unknown format: {}", format);
            return EXIT_USAGE;
        }
    };

    if std::fs::write(out, rendered).is_err() {
        eprintln!("cannot write the output file");
        return EXIT_USAGE;
    }

    0
}
