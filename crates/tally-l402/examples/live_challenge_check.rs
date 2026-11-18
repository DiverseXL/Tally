//! Live, no-spend L402 check: the receiver wallet issues the invoice, the
//! client fetches a challenge, and a wrong proof is refused.
//!
//! Reads `.env.receiver` only. The crate holds no payment code at all, so this
//! example cannot pay, and it never prints the connection string, the invoice,
//! the macaroon, the preimage or the fixture root key.
//!
//! Exit 0 only when the first answer is 402, the macaroon is bound to the
//! invoice's payment hash, and a wrong preimage is also refused with 402.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};

use tally_core::redact::Scrubber;

use tally_l402::client::L402Client;
use tally_l402::fixture::{self, FixtureConfig, NwcInvoiceSource};
use tally_l402::invoice;
use tally_l402::macaroon::identifier_payment_hash;
use tally_nwc::connection::read_secret_line;
use tally_nwc::session::{SessionOptions, WalletSession, select_encryption_mode};

/// Console output that scrubs every line and remembers whether a secret would
/// have been printed.
struct Output {
    scrubber: Scrubber,
    leaked: bool,
}

impl Output {
    fn new(scrubber: Scrubber) -> Self {
        Self {
            scrubber,
            leaked: false,
        }
    }

    fn add_secret(&mut self, secret: SecretString) {
        self.scrubber.add(secret);
    }

    fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if self.scrubber.contains_secret(text) {
            self.leaked = true;
        }
        println!("{}", self.scrubber.scrub(text));
    }
}

fn add_secret_component(scrubber: &mut Scrubber, connection: &SecretString) {
    let text = connection.expose_secret();
    let Some((_, rest)) = text.split_once("secret=") else {
        return;
    };
    let value = rest.split('&').next().unwrap_or(rest);

    if !value.is_empty() {
        scrubber.add(SecretString::from(value.to_owned()));
    }
}

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("runtime_error={}", error.kind());
            return ExitCode::from(2);
        }
    };

    runtime.block_on(run())
}

async fn run() -> ExitCode {
    let mut scrubber = Scrubber::new();

    let receiver_secret =
        match read_secret_line(Path::new(".env.receiver"), "TALLY_RECEIVER_NWC_URI") {
            Ok(secret) => secret,
            Err(error) => {
                println!("env_receiver_error={error}");
                return ExitCode::from(2);
            }
        };
    scrubber.add(receiver_secret.clone());
    add_secret_component(&mut scrubber, &receiver_secret);

    let mut out = Output::new(scrubber);

    let session = match WalletSession::with_options(&receiver_secret, SessionOptions::default()) {
        Ok(session) => session,
        Err(error) => {
            out.line(format!(
                "connection_error kind={} detail={}",
                error.kind(),
                error.detail()
            ));
            return ExitCode::from(2);
        }
    };
    drop(receiver_secret);

    let mut session = session;
    if let Err(error) = session.connect().await {
        out.line(format!(
            "connect_error kind={} detail={}",
            error.kind(),
            error.detail()
        ));
        return ExitCode::from(2);
    }
    out.line(format!(
        "receiver_pubkey={} relay_count={}",
        session.service_pubkey_hex(),
        session.relay_hosts().len()
    ));

    // The info event's encryption tag picks the cipher; the choice is passed to
    // the client builder explicitly.
    let tokens = match session.fetch_info_event().await {
        Ok(Some(record)) => record.encryption_tokens,
        Ok(None) => Vec::new(),
        Err(error) => {
            out.line(format!(
                "info_event_error kind={} detail={}",
                error.kind(),
                error.detail()
            ));
            Vec::new()
        }
    };
    let mode = select_encryption_mode(&tokens);
    if let Err(error) = session.set_encryption_mode(mode).await {
        out.line(format!(
            "encryption_error kind={} detail={}",
            error.kind(),
            error.detail()
        ));
        return ExitCode::from(2);
    }
    out.line(format!("encryption_mode={}", mode.as_str()));

    let wallet_network = match session.get_info().await {
        Ok(info) => info.network,
        Err(error) => {
            out.line(format!(
                "get_info_error kind={} detail={}",
                error.kind(),
                error.detail()
            ));
            None
        }
    };
    out.line(format!(
        "wallet_network={}",
        wallet_network.as_deref().unwrap_or("unknown")
    ));

    let source = NwcInvoiceSource::new(Arc::new(session));
    let handle = match fixture::spawn(FixtureConfig::default(), source).await {
        Ok(handle) => handle,
        Err(error) => {
            out.line(format!("fixture_error detail={}", error.detail()));
            return ExitCode::from(2);
        }
    };
    out.line(format!("fixture_addr={}", handle.addr()));
    let url = handle.protected_url();

    let client = match L402Client::new() {
        Ok(client) => client,
        Err(error) => {
            out.line(format!("client_error kind={}", error.kind().as_str()));
            handle.shutdown().await;
            return ExitCode::from(2);
        }
    };

    let challenge = match client.fetch_challenge(&url).await {
        Ok(challenge) => challenge,
        Err(error) => {
            out.line(format!(
                "challenge_error kind={} status={:?}",
                error.kind().as_str(),
                error.status()
            ));
            handle.shutdown().await;
            return ExitCode::from(1);
        }
    };

    out.add_secret(SecretString::from(challenge.macaroon_b64().to_owned()));
    out.add_secret(SecretString::from(challenge.invoice().to_owned()));

    let facts = match invoice::parse(challenge.invoice()) {
        Ok(facts) => facts,
        Err(error) => {
            out.line(format!("invoice_error kind={}", error.kind().as_str()));
            handle.shutdown().await;
            return ExitCode::from(1);
        }
    };

    let identifier_hash = identifier_payment_hash(challenge.macaroon_b64());
    let binding_matches = identifier_hash
        .as_ref()
        .is_ok_and(|hash| *hash == facts.payment_hash);
    let network_consistent = wallet_network
        .as_deref()
        .is_some_and(|network| facts.check_network(network).is_ok());

    out.line(format!("http_status={}", challenge.http_status()));
    out.line(format!("scheme={}", challenge.scheme()));
    out.line(format!(
        "macaroon_present={}",
        !challenge.macaroon_b64().is_empty()
    ));
    out.line(format!("invoice_network={}", facts.network));
    out.line(format!(
        "amount_msat={}",
        facts
            .amount_msat
            .map_or_else(|| String::from("none"), |amount| amount.to_string())
    ));
    out.line(format!(
        "seconds_until_expiry={}",
        facts.seconds_until_expiry
    ));
    out.line(format!(
        "payment_hash={}",
        facts
            .payment_hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    out.line(format!(
        "identifier_payment_hash_matches_invoice={binding_matches}"
    ));
    out.line(format!(
        "invoice_network_consistent_with_wallet={network_consistent}"
    ));

    // A wrong preimage must be refused. The fixture mints a fresh challenge; no
    // invoice is paid, so no funds move.
    let wrong_preimage = "aa".repeat(32);
    let wrong = client
        .fetch_with_credentials(&url, challenge.macaroon_b64(), &wrong_preimage)
        .await;

    let wrong_status = match wrong {
        Ok(response) => Some(response.status),
        Err(error) => {
            out.line(format!("wrong_proof_error kind={}", error.kind().as_str()));
            None
        }
    };
    out.line(format!(
        "wrong_proof_status={}",
        wrong_status.map_or_else(|| String::from("none"), |status| status.to_string())
    ));

    handle.shutdown().await;

    out.line(format!("output_contains_secret={}", out.leaked));

    if challenge.http_status() == 402 && binding_matches && wrong_status == Some(402) && !out.leaked
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
