//! Runnable proof that two NWC wallets can issue, pay and settle one invoice.
//!
//! Reads `.env.payer` and `.env.receiver`, prints only scrubbed output, and pays
//! only when started with `--approve-payment-sats <sats>`.
//!
//! Every printed line goes through the [`Scrubber`], which holds both connection
//! strings and both embedded secret keys.

use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::Duration;

use lightning_invoice::Bolt11Invoice;
use nostr::nips::nip47::{
    ListTransactionsRequest, LookupInvoiceRequest, MakeInvoiceRequest, Nip47Tag,
    NostrWalletConnectUri, PayInvoiceRequest,
};
use nostr::prelude::{Event, Filter, Kind};
use nwc::NostrWalletConnect;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tally_core::redact::Scrubber;
use tally_nwc::connection::read_secret_line;
use tally_nwc::guard::{self, GuardError, MSAT_PER_SAT};
use zeroize::Zeroize;

/// Upper bound for any single network operation.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Upper bound for fetching the wallet info event.
const INFO_EVENT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait for relay connections.
const CONNECT_WAIT: Duration = Duration::from_secs(15);
/// Seconds the receiver invoice stays valid.
const INVOICE_EXPIRY_SECS: u64 = 120;
/// Methods that cannot spend money, used for the unsupported-method probe.
const PROBE_CANDIDATES: [&str; 3] = ["sign_message", "list_transactions", "get_budget"];

/// Console output that scrubs every line and remembers whether a secret slipped
/// through before scrubbing.
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

    fn line(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if self.scrubber.contains_secret(text) {
            self.leaked = true;
        }
        println!("{}", self.scrubber.scrub(text));
    }
}

/// One connected wallet.
struct Wallet {
    label: &'static str,
    client: NostrWalletConnect,
    uri: NostrWalletConnectUri,
}

/// Advertised methods of a wallet and where the list came from.
struct Capabilities {
    source: &'static str,
    methods: Vec<String>,
}

impl Capabilities {
    fn advertises(&self, method: &str) -> bool {
        self.methods.iter().any(|advertised| advertised == method)
    }
}

fn main() -> ExitCode {
    let approved_sats = match approved_sats_from_args() {
        Ok(approved) => approved,
        Err(message) => {
            println!("argument_error={message}");
            return ExitCode::FAILURE;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("runtime_error={}", error.kind());
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(run(approved_sats))
}

fn approved_sats_from_args() -> Result<Option<u64>, &'static str> {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--approve-payment-sats" {
            let value = args
                .next()
                .ok_or("missing value for --approve-payment-sats")?;
            let sats: u64 = value
                .parse()
                .map_err(|_| "value for --approve-payment-sats is not a number")?;
            return Ok(Some(sats));
        }
    }
    Ok(None)
}

async fn run(approved_sats: Option<u64>) -> ExitCode {
    let mut scrubber = Scrubber::new();

    let mut payer_text = match read_connection(Path::new(".env.payer"), "TALLY_PAYER_NWC_URI") {
        Ok(value) => value,
        Err(kind) => {
            println!("env_payer_error={kind}");
            return ExitCode::FAILURE;
        }
    };
    let mut receiver_text =
        match read_connection(Path::new(".env.receiver"), "TALLY_RECEIVER_NWC_URI") {
            Ok(value) => value,
            Err(kind) => {
                payer_text.zeroize();
                println!("env_receiver_error={kind}");
                return ExitCode::FAILURE;
            }
        };

    scrubber.add(SecretString::from(payer_text.clone()));
    scrubber.add(SecretString::from(receiver_text.clone()));

    let mut out = Output::new(scrubber);
    match approved_sats {
        Some(sats) => out.line(format!("mode=approved approved_sats={sats}")),
        None => out.line("mode=read_only"),
    }

    let payer_uri = match NostrWalletConnectUri::parse(payer_text.as_str()) {
        Ok(uri) => uri,
        Err(error) => {
            payer_text.zeroize();
            receiver_text.zeroize();
            out.line(format!("payer_uri_parse_error kind={}", error.kind()));
            return ExitCode::FAILURE;
        }
    };
    let receiver_uri = match NostrWalletConnectUri::parse(receiver_text.as_str()) {
        Ok(uri) => uri,
        Err(error) => {
            payer_text.zeroize();
            receiver_text.zeroize();
            out.line(format!("receiver_uri_parse_error kind={}", error.kind()));
            return ExitCode::FAILURE;
        }
    };

    out.scrubber
        .add(SecretString::from(payer_uri.secret.to_secret_hex()));
    out.scrubber
        .add(SecretString::from(receiver_uri.secret.to_secret_hex()));

    out.line(format!(
        "wallets payer_pubkey={} receiver_pubkey={} payer_relays={} receiver_relays={}",
        payer_uri.public_key,
        receiver_uri.public_key,
        payer_uri.relays.len(),
        receiver_uri.relays.len()
    ));

    payer_text.zeroize();
    receiver_text.zeroize();

    let payer = Wallet {
        label: "payer",
        client: NostrWalletConnect::builder(payer_uri.clone())
            .timeout(REQUEST_TIMEOUT)
            .build(),
        uri: payer_uri,
    };
    let receiver = Wallet {
        label: "receiver",
        client: NostrWalletConnect::builder(receiver_uri.clone())
            .timeout(REQUEST_TIMEOUT)
            .build(),
        uri: receiver_uri,
    };

    connect(&payer, &mut out).await;
    connect(&receiver, &mut out).await;

    let payer_caps = capabilities(&payer, &mut out).await;
    let receiver_caps = capabilities(&receiver, &mut out).await;

    let payer_before = balance_sats(&payer, &mut out).await;
    let receiver_before = balance_sats(&receiver, &mut out).await;

    // Receiver issues the invoice.
    let request = MakeInvoiceRequest {
        amount: 21 * MSAT_PER_SAT,
        description: Some(String::from("Tally step 4 wallet proof")),
        description_hash: None,
        expiry: Some(INVOICE_EXPIRY_SECS),
    };
    let invoice_response = match receiver
        .client
        .make_invoice(request)
        .timeout(REQUEST_TIMEOUT)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            out.line(format!("make_invoice=failed kind={}", error.kind()));
            out.line(format!("make_invoice_message={error}"));
            finish(&mut out);
            return ExitCode::FAILURE;
        }
    };

    let invoice = match Bolt11Invoice::from_str(&invoice_response.invoice) {
        Ok(invoice) => invoice,
        Err(_) => {
            out.line("invoice_parse=failed");
            finish(&mut out);
            return ExitCode::FAILURE;
        }
    };

    let amount_msat = invoice.amount_milli_satoshis().unwrap_or(0);
    let payment_hash_hex = invoice.payment_hash().to_string();
    let seconds_until_expiry = invoice.duration_until_expiry().as_secs();

    out.line(format!("invoice_amount_msat={amount_msat}"));
    out.line(format!("invoice_network={:?}", invoice.network()));
    out.line(format!("invoice_payment_hash={payment_hash_hex}"));
    out.line(format!(
        "invoice_seconds_until_expiry={seconds_until_expiry}"
    ));
    out.line(format!("invoice_expired={}", invoice.is_expired()));

    match invoice_response.payment_hash.as_deref() {
        Some(wallet_hash) => out.line(format!(
            "make_invoice_payment_hash_matches={}",
            wallet_hash.eq_ignore_ascii_case(&payment_hash_hex)
        )),
        None => out.line("make_invoice_payment_hash=absent"),
    }

    // Guard, then pay at most once.
    match guard::check_payment(approved_sats, amount_msat, invoice.is_expired()) {
        Ok(sats) => out.line(format!("guard=accepted approved_sats={sats}")),
        Err(error) => out.line(format!("guard=refused reason={}", guard_reason(&error))),
    }

    let mut paid = false;
    match approved_sats
        .filter(|_| guard::check_payment(approved_sats, amount_msat, invoice.is_expired()).is_ok())
    {
        None => out.line("pay_invoice=not_called"),
        Some(_) => {
            let request = PayInvoiceRequest::new(invoice_response.invoice.clone());
            match payer
                .client
                .pay_invoice(request)
                .timeout(Duration::from_secs(30))
                .await
            {
                Ok(response) => {
                    paid = true;
                    out.line(format!(
                        "pay_invoice=ok fees_paid_msat={:?}",
                        response.fees_paid
                    ));
                    out.line(format!(
                        "preimage_present={}",
                        !response.preimage.is_empty()
                    ));
                    match (hex_to_32(&response.preimage), hex_to_32(&payment_hash_hex)) {
                        (Some(preimage_bytes), Some(expected_hash)) => {
                            let digest = Sha256::digest(preimage_bytes);
                            let digest_bytes: &[u8] = digest.as_ref();
                            let matches: bool = digest_bytes.ct_eq(expected_hash.as_slice()).into();
                            out.line(format!("preimage_matches_payment_hash={matches}"));
                        }
                        _ => out.line("preimage_matches_payment_hash=false"),
                    }
                }
                Err(error) => {
                    out.line(format!("pay_invoice=failed kind={}", error.kind()));
                    out.line(format!("pay_invoice_message={error}"));
                    out.line("AMBIGUOUS_PAYMENT_NOT_RETRIED");
                }
            }
        }
    }

    // Balances afterwards.
    let payer_after = balance_sats(&payer, &mut out).await;
    let receiver_after = balance_sats(&receiver, &mut out).await;
    out.line(format!(
        "payer_balance_delta_sats={}",
        delta(payer_before, payer_after)
    ));
    out.line(format!(
        "receiver_balance_delta_sats={}",
        delta(receiver_before, receiver_after)
    ));

    // Receiver-side settlement check.
    if receiver_caps.advertises("lookup_invoice") {
        let request = LookupInvoiceRequest {
            payment_hash: Some(payment_hash_hex.clone()),
            invoice: None,
        };
        match receiver
            .client
            .lookup_invoice(request)
            .timeout(REQUEST_TIMEOUT)
            .await
        {
            Ok(response) => {
                out.line(format!(
                    "lookup_invoice[source={}] state={:?} amount_msat={} settled_at_present={}",
                    receiver_caps.source,
                    response.state,
                    response.amount,
                    response.settled_at.is_some()
                ));
            }
            Err(error) => out.line(format!(
                "lookup_invoice=failed kind={} message={error}",
                error.kind()
            )),
        }
    } else {
        out.line(format!(
            "lookup_invoice=UNSUPPORTED [source={}]",
            receiver_caps.source
        ));
    }

    // Probe one method the payer does not advertise.
    probe_unsupported_method(&payer, &payer_caps, &mut out).await;

    if paid {
        finish(&mut out);
        ExitCode::SUCCESS
    } else {
        let read_only = approved_sats.is_none();
        finish(&mut out);
        if read_only {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

fn read_connection(path: &Path, key: &str) -> Result<String, String> {
    read_secret_line(path, key)
        .map(|secret| secret.expose_secret().to_owned())
        .map_err(|error| error.to_string())
}

fn guard_reason(error: &GuardError) -> String {
    match error {
        GuardError::NoExplicitApproval => String::from("NoExplicitApproval"),
        GuardError::AmountMismatch {
            approved_sats,
            invoice_sats,
        } => format!("AmountMismatch(approved={approved_sats},invoice={invoice_sats})"),
        GuardError::InvoiceExpired => String::from("InvoiceExpired"),
    }
}

fn delta(before: Option<u64>, after: Option<u64>) -> String {
    match (before, after) {
        (Some(before), Some(after)) => (after as i64 - before as i64).to_string(),
        _ => String::from("unknown"),
    }
}

fn finish(out: &mut Output) {
    let leaked = out.leaked;
    out.line(format!("output_contains_secret={leaked}"));
}

async fn connect(wallet: &Wallet, out: &mut Output) {
    for relay in wallet.uri.relays.iter() {
        match wallet.client.client().add_relay(relay.clone()).await {
            Ok(added) => out.line(format!("[{}] relay_added={added}", wallet.label)),
            Err(error) => out.line(format!(
                "[{}] relay_add_failed kind={}",
                wallet.label,
                error.kind()
            )),
        }
    }

    wallet
        .client
        .client()
        .connect()
        .and_wait(CONNECT_WAIT)
        .await;

    let relays = wallet.client.client().relays().await;
    out.line(format!(
        "[{}] connect relays={}",
        wallet.label,
        relays.len()
    ));
}

async fn balance_sats(wallet: &Wallet, out: &mut Output) -> Option<u64> {
    match wallet.client.get_balance().timeout(REQUEST_TIMEOUT).await {
        Ok(response) => {
            let sats = response.balance / MSAT_PER_SAT;
            out.line(format!(
                "[{}] get_balance balance_msat={} balance_sats={sats}",
                wallet.label, response.balance
            ));
            Some(sats)
        }
        Err(error) => {
            out.line(format!(
                "[{}] get_balance=failed kind={}",
                wallet.label,
                error.kind()
            ));
            None
        }
    }
}

async fn capabilities(wallet: &Wallet, out: &mut Output) -> Capabilities {
    let filter = Filter::new()
        .kind(Kind::WalletConnectInfo)
        .author(wallet.uri.public_key)
        .limit(1);

    let event = match wallet
        .client
        .client()
        .fetch_events(filter)
        .timeout(INFO_EVENT_TIMEOUT)
        .max_events(1)
        .await
    {
        Ok(events) => events.into_iter().next(),
        Err(error) => {
            out.line(format!(
                "[{}] info_event=fetch_failed kind={} within={}s",
                wallet.label,
                error.kind(),
                INFO_EVENT_TIMEOUT.as_secs()
            ));
            None
        }
    };

    let info_event_caps = event.as_ref().and_then(|event| {
        let content: serde_json::Value =
            serde_json::from_str(&event.content).unwrap_or(serde_json::Value::Null);
        let methods = string_array(content.get("methods"));
        let notifications = string_array(content.get("notifications"));
        let keys = content
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<String>>())
            .unwrap_or_default();
        out.line(format!(
            "[{}] info_event found=true content_keys=[{}] methods_count={} encryption=[{}] [source=info_event]",
            wallet.label,
            keys.join(","),
            methods.len(),
            encryption_tags(event).join(",")
        ));
        if methods.is_empty() {
            out.line(format!(
                "[{}] info_event advertises no methods, falling_back=true",
                wallet.label
            ));
            None
        } else {
            out.line(format!(
                "[{}] capabilities methods=[{}] notifications=[{}] encryption=[{}] [source=info_event]",
                wallet.label,
                methods.join(","),
                notifications.join(","),
                encryption_tags(event).join(",")
            ));
            Some(methods)
        }
    });

    // get_info is always called and always labelled with its own source.
    let info_methods = match wallet.client.get_info().timeout(REQUEST_TIMEOUT).await {
        Ok(info) => {
            let methods: Vec<String> = info
                .methods
                .iter()
                .map(|method| method.as_str().to_owned())
                .collect();
            match info_event_caps {
                Some(_) => out.line(format!(
                    "[{}] get_info alias={:?} network={:?} block_height={:?} methods_count={} [source=get_info]",
                    wallet.label,
                    info.alias,
                    info.network,
                    info.block_height,
                    methods.len()
                )),
                None => out.line(format!(
                    "[{}] capabilities methods=[{}] notifications=[{}] [source=get_info]",
                    wallet.label,
                    methods.join(","),
                    info.notifications.join(",")
                )),
            }
            methods
        }
        Err(error) => {
            out.line(format!(
                "[{}] get_info=failed kind={}",
                wallet.label,
                error.kind()
            ));
            Vec::new()
        }
    };

    match info_event_caps {
        Some(methods) => Capabilities {
            source: "info_event",
            methods,
        },
        None => Capabilities {
            source: "get_info",
            methods: info_methods,
        },
    }
}

async fn probe_unsupported_method(wallet: &Wallet, caps: &Capabilities, out: &mut Output) {
    let absent: Vec<&str> = PROBE_CANDIDATES
        .iter()
        .copied()
        .filter(|method| !caps.advertises(method))
        .collect();

    if absent.is_empty() {
        out.line(format!(
            "probe=NOT_APPLICABLE all_candidates_advertised [source={}]",
            caps.source
        ));
        return;
    }

    if !absent.contains(&"list_transactions") {
        out.line(format!(
            "probe=NOT_APPLICABLE absent_methods=[{}] no_typed_api [source={}]",
            absent.join(","),
            caps.source
        ));
        return;
    }

    match wallet
        .client
        .list_transactions(ListTransactionsRequest::default())
        .timeout(REQUEST_TIMEOUT)
        .await
    {
        Ok(_) => out.line("probe method=list_transactions result=accepted_unexpectedly"),
        Err(error) => {
            out.line(format!(
                "probe method=list_transactions error_kind={} error_message={error}",
                error.kind()
            ));
        }
    }
}

fn encryption_tags(event: &Event) -> Vec<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| match Nip47Tag::try_from(tag) {
            Ok(Nip47Tag::Encryption(ciphers)) => Some(ciphers.to_string()),
            Err(_) => None,
        })
        .collect()
}

fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn hex_to_32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }

    let mut bytes = [0u8; 32];
    for (index, chunk) in bytes.iter_mut().enumerate() {
        *chunk = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}
