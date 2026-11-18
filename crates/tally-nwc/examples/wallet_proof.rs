//! Live proof of the wallet adapter: two NWC wallets, one invoice, one payment.
//!
//! Reads `.env.payer` and `.env.receiver`, prints only scrubbed output, and pays
//! only when started with `--approve-payment-sats 21` and only through a
//! [`PaymentPermit`]. A payment is never retried.
//!
//! Every printed line goes through the [`Scrubber`], which holds both connection
//! strings, both embedded secret keys and the issued invoice.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use secrecy::{ExposeSecret, SecretString};

use tally_core::capabilities::{
    Advertisement, Capabilities, CapabilitySource, InfoEventCapabilities,
};
use tally_core::outcome::{Observation, classify_wallet_outcome};
use tally_core::redact::Scrubber;
use tally_core::report::{
    EndpointClass, EnvClass, Environment, InvoiceEvidence, NwcClientIdentity, PaymentEvidence,
    PaymentOutcome, RoleCapabilities, RunReport, SCHEMA_VERSION, TestResult, WalletRole,
    WalletSummary, WorkflowEvidence, exit_code,
};
use tally_core::status::Status;
use tally_core::{new_run_id, now_rfc3339};
use tally_nwc::connection::read_secret_line;
use tally_nwc::session::NWC_CLIENT_VERSION;
use tally_nwc::{
    ConnectReport, EncryptionMode, InfoEventRecord, NwcError, PaymentPermit, SessionOptions,
    WalletInfo, WalletSession, select_encryption_mode,
};

/// Amount the proof moves, in millisatoshis.
const PAYMENT_MSAT: u64 = 21_000;
/// Seconds the receiver invoice stays valid.
const INVOICE_EXPIRY_SECS: u64 = 300;
/// The limitation recorded in every report of this run.
const LIMITATION: &str = "Sandbox wallets from the NWC faucet: balances are play money on a single Alby Hub with no Lightning channels; payments use the real NWC protocol and return a real preimage but are not routed across the real Lightning network.";
/// Output file of the report.
const REPORT_PATH: &str = "reports/report.json";

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

fn main() -> ExitCode {
    let approved_sats = match approved_sats_from_args() {
        Ok(approved) => approved,
        Err(message) => {
            println!("argument_error={message}");
            return ExitCode::from(2);
        }
    };

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

/// Everything collected from one wallet.
struct WalletState {
    label: &'static str,
    session: WalletSession,
    connect_error: Option<NwcError>,
    relays_connected: usize,
    info: Option<InfoEventRecord>,
    details: Option<WalletInfo>,
    capabilities: Capabilities,
    balance_before_msat: Option<u64>,
    balance_after_msat: Option<u64>,
}

impl WalletState {
    fn effective_source(&self) -> CapabilitySource {
        self.capabilities.effective().source
    }
}

/// The outcome of the payment step.
struct PaymentStep {
    status: Status,
    actual: String,
    diagnostics: Vec<String>,
    evidence: PaymentEvidence,
}

impl PaymentStep {
    fn not_attempted(status: Status, actual: String, reason: &str) -> Self {
        Self {
            status,
            actual,
            diagnostics: vec![format!("reason={reason}")],
            evidence: PaymentEvidence {
                outcome: PaymentOutcome::NotAttempted,
                fees_paid_msat: None,
                preimage_present: false,
                preimage_matches_payment_hash: None,
            },
        }
    }
}

async fn run(approved_sats: Option<u64>) -> ExitCode {
    let started_at = now_rfc3339();
    let started = Instant::now();

    let mut scrubber = Scrubber::new();

    let payer_secret = match read_secret_line(Path::new(".env.payer"), "TALLY_PAYER_NWC_URI") {
        Ok(secret) => secret,
        Err(error) => {
            println!("env_payer_error={error}");
            return ExitCode::from(2);
        }
    };
    scrubber.add(payer_secret.clone());
    add_secret_component(&mut scrubber, &payer_secret);

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

    let payer_session = WalletSession::with_options(&payer_secret, SessionOptions::default());
    let receiver_session = WalletSession::with_options(&receiver_secret, SessionOptions::default());
    drop(payer_secret);
    drop(receiver_secret);

    let (payer_session, receiver_session) = match (payer_session, receiver_session) {
        (Ok(payer), Ok(receiver)) => (payer, receiver),
        (Err(error), _) | (_, Err(error)) => {
            out.line(format!(
                "connection_error kind={} detail={}",
                error.kind(),
                error.detail()
            ));
            return ExitCode::from(2);
        }
    };

    match approved_sats {
        Some(sats) => out.line(format!("mode=approved approved_sats={sats}")),
        None => out.line("mode=read_only"),
    }

    let mut payer = WalletState {
        label: "payer",
        session: payer_session,
        connect_error: None,
        relays_connected: 0,
        info: None,
        details: None,
        capabilities: empty_capabilities(),
        balance_before_msat: None,
        balance_after_msat: None,
    };
    let mut receiver = WalletState {
        label: "receiver",
        session: receiver_session,
        connect_error: None,
        relays_connected: 0,
        info: None,
        details: None,
        capabilities: empty_capabilities(),
        balance_before_msat: None,
        balance_after_msat: None,
    };

    out.line(format!(
        "wallets payer_pubkey={} receiver_pubkey={} payer_relay_hosts=[{}] receiver_relay_hosts=[{}]",
        payer.session.service_pubkey_hex(),
        receiver.session.service_pubkey_hex(),
        payer.session.relay_hosts().join(","),
        receiver.session.relay_hosts().join(",")
    ));

    // Connect both wallets concurrently.
    let (payer_connect, receiver_connect) =
        tokio::join!(payer.session.connect(), receiver.session.connect());
    apply_connect(&mut payer, payer_connect, &mut out);
    apply_connect(&mut receiver, receiver_connect, &mut out);

    // Raw info events, concurrently.
    let (payer_info, receiver_info) = tokio::join!(
        payer.session.fetch_info_event(),
        receiver.session.fetch_info_event()
    );
    apply_info(&mut payer, payer_info, &mut out);
    apply_info(&mut receiver, receiver_info, &mut out);

    // Cipher choice: the info event's encryption tag decides it, and the
    // choice is passed to the client builder explicitly.
    apply_encryption(&mut payer, &mut out).await;
    apply_encryption(&mut receiver, &mut out).await;

    // get_info, concurrently.
    let (payer_details, receiver_details) =
        tokio::join!(payer.session.get_info(), receiver.session.get_info());
    apply_details(&mut payer, payer_details, &mut out);
    apply_details(&mut receiver, receiver_details, &mut out);

    // Balances before, concurrently.
    let (payer_before, receiver_before) =
        tokio::join!(payer.session.get_balance(), receiver.session.get_balance());
    payer.balance_before_msat = apply_balance(&mut payer, payer_before, &mut out);
    receiver.balance_before_msat = apply_balance(&mut receiver, receiver_before, &mut out);

    // The receiver issues the invoice.
    let make_invoice = receiver
        .session
        .make_invoice(
            PAYMENT_MSAT,
            "Tally step 5 wallet proof",
            INVOICE_EXPIRY_SECS,
        )
        .await;

    let mut invoice_evidence: Option<InvoiceEvidence> = None;
    let mut invoice_secret: Option<SecretString> = None;
    let mut payment_hash_hex: Option<String> = None;
    let mut make_invoice_diagnostics: Vec<String> = Vec::new();
    let mut make_invoice_ok = false;

    match make_invoice {
        Ok(made) => {
            let network = made.network();
            let amount_msat = made.amount_msat();
            let seconds_until_expiry = made.seconds_until_expiry();
            let hash = made.payment_hash_hex().map(str::to_owned);
            let wallet_hash_matches = made.wallet_hash_matches();

            out.line(format!(
                "[receiver] make_invoice amount_msat={amount_msat} network={network} seconds_until_expiry={seconds_until_expiry} invoice_expiry_requested_s={INVOICE_EXPIRY_SECS}"
            ));
            match &hash {
                Some(hash) => out.line(format!("[receiver] make_invoice payment_hash={hash}")),
                None => out.line("[receiver] make_invoice payment_hash=absent"),
            }
            match wallet_hash_matches {
                Some(matches) => out.line(format!(
                    "[receiver] make_invoice_payment_hash_matches={matches}"
                )),
                None => out.line("[receiver] make_invoice_payment_hash=absent"),
            }

            let invoice = made.into_invoice();
            // The invoice is a secret from here on: scrub it everywhere.
            out.add_secret(invoice.clone());

            make_invoice_ok = amount_msat == PAYMENT_MSAT && hash.is_some();
            make_invoice_diagnostics.push(format!("amount_msat={amount_msat}"));
            make_invoice_diagnostics.push(format!("network={network}"));
            if !make_invoice_ok {
                make_invoice_diagnostics.push(String::from("amount_or_hash_unexpected"));
            }

            invoice_evidence = Some(InvoiceEvidence {
                network: Some(network.to_owned()),
                amount_msat,
                payment_hash: hash.clone(),
                seconds_until_expiry,
            });
            payment_hash_hex = hash;
            invoice_secret = Some(invoice);
        }
        Err(error) => {
            out.line(format!(
                "[receiver] make_invoice=failed kind={} detail={} status={}",
                error.kind(),
                error.detail(),
                error.classify().status()
            ));
            make_invoice_diagnostics.push(format!("error_kind={}", error.kind()));
        }
    }

    let payment = run_payment(
        &mut payer,
        &mut receiver,
        approved_sats,
        invoice_secret.as_ref(),
        &mut out,
    )
    .await;

    // Receiver-side settlement check.
    let (lookup_status, lookup_actual, lookup_diagnostics) = lookup(
        &receiver,
        payment_hash_hex.as_deref(),
        approved_sats.is_some(),
        &mut out,
    )
    .await;

    let sign_message = sign_message_test(&payer, &mut out);

    let tests = vec![
        connect_test(&payer, &receiver),
        info_event_test(&payer, &receiver),
        get_info_test(&payer, &receiver),
        get_balance_test(&payer, &receiver),
        make_invoice_test(make_invoice_ok, make_invoice_diagnostics),
        TestResult::new("pay_invoice", "pay_invoice", payment.status, true)
            .with_required_capability("pay_invoice")
            .with_detail("one payment with a matching preimage", payment.actual)
            .with_diagnostics(payment.diagnostics),
        TestResult::new("lookup_invoice", "lookup_invoice", lookup_status, true)
            .with_required_capability("lookup_invoice")
            .with_detail(
                if approved_sats.is_some() {
                    "invoice found and settled"
                } else {
                    "invoice found as pending"
                },
                lookup_actual,
            )
            .with_diagnostics(lookup_diagnostics),
        sign_message,
    ];

    let report = RunReport {
        schema_version: SCHEMA_VERSION,
        run_id: new_run_id(),
        started_at,
        finished_at: now_rfc3339(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        tally_version: String::from(env!("CARGO_PKG_VERSION")),
        environment: Environment {
            class: EnvClass::Sandbox,
            endpoint_class: EndpointClass::ExternalSandbox,
            nwc_client: NwcClientIdentity::new("nwc", NWC_CLIENT_VERSION),
            encryption_mode_selected: payer
                .session
                .encryption_mode()
                .map(EncryptionMode::as_str)
                .map(str::to_owned),
            network_reported: receiver
                .details
                .as_ref()
                .and_then(|details| details.network.clone()),
        },
        wallets: vec![
            WalletSummary {
                role: WalletRole::Payer,
                service_pubkey: payer.session.service_pubkey_hex(),
            },
            WalletSummary {
                role: WalletRole::Receiver,
                service_pubkey: receiver.session.service_pubkey_hex(),
            },
        ],
        capabilities: vec![
            RoleCapabilities::from_capabilities(WalletRole::Payer, &payer.capabilities),
            RoleCapabilities::from_capabilities(WalletRole::Receiver, &receiver.capabilities),
        ],
        tests,
        workflow_evidence: WorkflowEvidence {
            invoice: invoice_evidence,
            payment: Some(payment.evidence),
            challenge: None,
            protected_resource: None,
        },
        limitations: vec![String::from(LIMITATION)],
    };

    out.line(format!("tests_total={}", report.tests.len()));

    emit(&report, &mut out)
}

/// Runs the payment step according to the capability policy and the approval.
async fn run_payment(
    payer: &mut WalletState,
    receiver: &mut WalletState,
    approved_sats: Option<u64>,
    invoice: Option<&SecretString>,
    out: &mut Output,
) -> PaymentStep {
    let Some(sats) = approved_sats else {
        out.line("[payer] pay_invoice=not_called read_only=true");
        let classified = classify_wallet_outcome(Observation::NotSelected);
        return PaymentStep::not_attempted(
            classified.status(),
            String::from("no payment attempted: read-only run"),
            classified.reason_code(),
        );
    };

    let advertised = payer.capabilities.advertises("pay_invoice");
    if !advertised.is_yes() {
        let classified = classify_wallet_outcome(Observation::MethodNotAdvertised);
        out.line(format!(
            "[payer] pay_invoice=UNSUPPORTED advertised=false no_request_sent=true source={}",
            advertised.source().map_or("none", CapabilitySource::as_str)
        ));
        return PaymentStep::not_attempted(
            classified.status(),
            String::from("pay_invoice not advertised, so no request was sent"),
            classified.reason_code(),
        );
    }

    let Some(invoice) = invoice else {
        out.line("[payer] pay_invoice=not_called no_invoice=true");
        return PaymentStep::not_attempted(
            Status::Fail,
            String::from("no invoice to pay"),
            "NO_INVOICE",
        );
    };

    let permit = match PaymentPermit::issue(invoice, Some(sats), EnvClass::Sandbox, None) {
        Ok(permit) => permit,
        Err(error) => {
            let classified = error.classify();
            out.line(format!(
                "[payer] permit=refused kind={} detail={} status={} reason={}",
                error.kind(),
                error.detail(),
                classified.status(),
                classified.reason_code()
            ));
            return PaymentStep::not_attempted(
                classified.status(),
                format!("permit refused: {}", error.detail()),
                classified.reason_code(),
            );
        }
    };

    out.line(format!(
        "[payer] permit=issued amount_msat={} env=sandbox",
        permit.amount_msat()
    ));
    let permit_hash = permit.payment_hash_hex().to_owned();

    // Exactly one attempt; there is deliberately no retry path.
    let step = match payer.session.pay_invoice(permit).await {
        Ok(paid) => {
            let fees = paid.fees_paid_msat();
            let preimage_present = paid.preimage_present();
            let matches = paid.preimage_matches(&permit_hash);

            out.line(format!(
                "[payer] pay_invoice=ok fees_paid_msat={fees:?} preimage_present={preimage_present} preimage_matches_payment_hash={matches}"
            ));

            let classified = classify_wallet_outcome(if preimage_present && matches {
                Observation::ExpectedBehaviourObserved
            } else {
                Observation::ExpectedBehaviourNotObserved
            });

            PaymentStep {
                status: classified.status(),
                actual: format!(
                    "preimage_present={preimage_present} preimage_matches_payment_hash={matches}"
                ),
                diagnostics: vec![
                    format!("fees_paid_msat={fees:?}"),
                    format!("reason={}", classified.reason_code()),
                ],
                evidence: PaymentEvidence {
                    outcome: PaymentOutcome::Paid,
                    fees_paid_msat: fees,
                    preimage_present,
                    preimage_matches_payment_hash: Some(matches),
                },
            }
        }
        Err(error) => {
            let classified = error.classify();
            out.line(format!(
                "[payer] pay_invoice=failed kind={} detail={} status={} reason={}",
                error.kind(),
                error.detail(),
                classified.status(),
                classified.reason_code()
            ));
            if classified.reason_code() == "AMBIGUOUS_PAYMENT_NOT_RETRIED" {
                out.line("[payer] AMBIGUOUS_PAYMENT_NOT_RETRIED");
            }

            PaymentStep {
                status: classified.status(),
                actual: format!("pay_invoice failed: {}", error.detail()),
                diagnostics: vec![
                    format!("error_kind={}", error.kind()),
                    format!("reason={}", classified.reason_code()),
                ],
                evidence: PaymentEvidence {
                    outcome: if classified.reason_code() == "AMBIGUOUS_PAYMENT_NOT_RETRIED" {
                        PaymentOutcome::Ambiguous
                    } else {
                        PaymentOutcome::Failed
                    },
                    fees_paid_msat: None,
                    preimage_present: false,
                    preimage_matches_payment_hash: None,
                },
            }
        }
    };

    let (payer_after, receiver_after) =
        tokio::join!(payer.session.get_balance(), receiver.session.get_balance());
    payer.balance_after_msat = apply_balance(payer, payer_after, out);
    receiver.balance_after_msat = apply_balance(receiver, receiver_after, out);

    match (
        payer.balance_before_msat,
        payer.balance_after_msat,
        receiver.balance_before_msat,
        receiver.balance_after_msat,
    ) {
        (Some(payer_before), Some(payer_after), Some(receiver_before), Some(receiver_after)) => {
            out.line(format!(
                "balance_delta_msat payer={} receiver={}",
                delta(payer_before, payer_after),
                delta(receiver_before, receiver_after)
            ));
        }
        _ => out.line("balance_delta_msat=unknown"),
    }

    step
}

/// Emits the report, the exit code and the leak line, then returns the code.
fn emit(report: &RunReport, out: &mut Output) -> ExitCode {
    let pretty = match serde_json::to_string_pretty(report) {
        Ok(pretty) => pretty,
        Err(error) => {
            out.line(format!("report_serialize_error={error}"));
            return ExitCode::from(2);
        }
    };

    out.line("report_json_begin");
    out.line(&pretty);
    out.line("report_json_end");

    match write_report(&pretty) {
        Ok(path) => out.line(format!("report_written={path}")),
        Err(kind) => out.line(format!("report_write_failed kind={kind}")),
    }

    let code = exit_code(report);
    out.line(format!("exit_code={code}"));

    let leaked = out.leaked;
    out.line(format!("output_contains_secret={leaked}"));

    ExitCode::from(u8::try_from(code).unwrap_or(2))
}

fn write_report(pretty: &str) -> std::io::Result<String> {
    let path = PathBuf::from(REPORT_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, pretty)?;
    Ok(path.display().to_string())
}

fn empty_capabilities() -> Capabilities {
    Capabilities::new(InfoEventCapabilities::missing(), None)
}

fn apply_connect(
    wallet: &mut WalletState,
    result: Result<ConnectReport, NwcError>,
    out: &mut Output,
) {
    match result {
        Ok(report) => {
            wallet.relays_connected = report.relays_connected;
            out.line(format!(
                "[{}] connect relays_requested={} relays_connected={}",
                wallet.label, report.relays_requested, report.relays_connected
            ));
        }
        Err(error) => {
            wallet.connect_error = Some(error);
            out.line(format!(
                "[{}] connect=failed kind={} detail={} status={}",
                wallet.label,
                error.kind(),
                error.detail(),
                error.classify().status()
            ));
        }
    }
}

fn apply_info(
    wallet: &mut WalletState,
    result: Result<Option<InfoEventRecord>, NwcError>,
    out: &mut Output,
) {
    match result {
        Ok(Some(info)) => {
            out.line(format!(
                "[{}] info_event found=true id={} created_at={} content_len={} content={}",
                wallet.label,
                info.id,
                info.created_at,
                info.raw_content.len(),
                info.raw_content
            ));
            for (index, tag) in info.tags.iter().enumerate() {
                let name = tag.first().map_or("", String::as_str);
                let values = tag
                    .iter()
                    .skip(1)
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(",");
                out.line(format!(
                    "[{}] info_event_tag[{index}] name={name} values=[{values}]",
                    wallet.label
                ));
            }
            out.line(format!(
                "[{}] info_event_content_kind={}",
                wallet.label,
                info.content_kind.as_str()
            ));
            out.line(format!(
                "[{}] info_event methods=[{}] encryption=[{}] extensions=[{}]",
                wallet.label,
                info.methods.join(","),
                info.encryption_tokens.join(","),
                info.extensions.join(",")
            ));

            wallet.capabilities = Capabilities::new(info.capabilities(), None);
            wallet.info = Some(info);
        }
        Ok(None) => {
            out.line(format!(
                "[{}] info_event found=false (no kind 13194 event from this wallet)",
                wallet.label
            ));
        }
        Err(error) => {
            out.line(format!(
                "[{}] info_event=fetch_failed kind={} detail={} status={}",
                wallet.label,
                error.kind(),
                error.detail(),
                error.classify().status()
            ));
        }
    }
}

/// Picks the cipher from the info event and rebuilds the connection with it.
async fn apply_encryption(wallet: &mut WalletState, out: &mut Output) {
    let mode = select_encryption_mode(&wallet.capabilities.info_event.encryption_tokens);

    match wallet.session.set_encryption_mode(mode).await {
        Ok(report) => out.line(format!(
            "[{}] encryption_mode={} relays_connected={}",
            wallet.label,
            mode.as_str(),
            report.relays_connected
        )),
        Err(error) => {
            out.line(format!(
                "[{}] encryption_mode={} reconnect=failed kind={} detail={}",
                wallet.label,
                mode.as_str(),
                error.kind(),
                error.detail()
            ));
            wallet.connect_error = Some(error);
        }
    }
}

fn apply_details(wallet: &mut WalletState, result: Result<WalletInfo, NwcError>, out: &mut Output) {
    match result {
        Ok(details) => {
            out.line(format!(
                "[{}] get_info alias={:?} network={:?} methods_count={} notifications_count={} get_info_extensions={:?} block_height={:?}",
                wallet.label,
                details.alias,
                details.network,
                details.methods.len(),
                details.notifications.len(),
                details.extensions.as_ref(),
                details.block_height
            ));
            out.line(format!(
                "[{}] get_info methods=[{}]",
                wallet.label,
                details.methods.join(",")
            ));

            let get_info_caps = details.capabilities();
            wallet.capabilities =
                Capabilities::new(wallet.capabilities.info_event.clone(), Some(get_info_caps));
            wallet.details = Some(details);
        }
        Err(error) => {
            out.line(format!(
                "[{}] get_info=failed kind={} detail={} status={}",
                wallet.label,
                error.kind(),
                error.detail(),
                error.classify().status()
            ));
        }
    }

    let effective = wallet.capabilities.effective();
    out.line(format!(
        "[{}] capabilities effective_source={} effective_methods=[{}]",
        wallet.label,
        effective.source.as_str(),
        effective.methods.join(",")
    ));

    let discrepancies = wallet.capabilities.discrepancies();
    if discrepancies.is_empty() {
        out.line(format!("[{}] capability_discrepancies=none", wallet.label));
    } else {
        let text = discrepancies
            .iter()
            .map(|item| {
                format!(
                    "{}:present_in={}:absent_in={}",
                    item.method,
                    item.present_in.as_str(),
                    item.absent_in.as_str()
                )
            })
            .collect::<Vec<String>>()
            .join(",");
        out.line(format!(
            "[{}] capability_discrepancies=[{text}]",
            wallet.label
        ));
    }

    if wallet.capabilities.info_event.found && wallet.capabilities.info_event.methods.is_empty() {
        out.line(format!(
            "[{}] info_event lists no method token, fell_back_to_get_info={}",
            wallet.label,
            wallet.details.is_some()
        ));
    }
}

fn apply_balance(
    wallet: &mut WalletState,
    result: Result<u64, NwcError>,
    out: &mut Output,
) -> Option<u64> {
    match result {
        Ok(balance_msat) => {
            out.line(format!(
                "[{}] get_balance balance_msat={balance_msat} balance_sats={}",
                wallet.label,
                balance_msat / 1_000
            ));
            Some(balance_msat)
        }
        Err(error) => {
            out.line(format!(
                "[{}] get_balance=failed kind={} detail={} status={}",
                wallet.label,
                error.kind(),
                error.detail(),
                error.classify().status()
            ));
            None
        }
    }
}

fn delta(before: u64, after: u64) -> i64 {
    i64::try_from(after).unwrap_or(i64::MAX) - i64::try_from(before).unwrap_or(i64::MAX)
}

fn status_of(pass: bool) -> Status {
    classify_wallet_outcome(if pass {
        Observation::ExpectedBehaviourObserved
    } else {
        Observation::ExpectedBehaviourNotObserved
    })
    .status()
}

fn connect_test(payer: &WalletState, receiver: &WalletState) -> TestResult {
    let pass = payer.connect_error.is_none()
        && receiver.connect_error.is_none()
        && payer.relays_connected > 0
        && receiver.relays_connected > 0;

    TestResult::new("connect", "connect", status_of(pass), true).with_detail(
        "both wallets connect to their relay",
        format!(
            "payer relays_connected={} receiver relays_connected={}",
            payer.relays_connected, receiver.relays_connected
        ),
    )
}

fn info_event_test(payer: &WalletState, receiver: &WalletState) -> TestResult {
    let pass = payer.info.is_some() && receiver.info.is_some();

    TestResult::new("info_event", "info_event", status_of(pass), true)
        .with_required_capability("info_event")
        .with_detail(
            "kind 13194 info event from both wallets",
            format!(
                "payer_found={} receiver_found={} payer_source={} receiver_source={}",
                payer.info.is_some(),
                receiver.info.is_some(),
                payer.effective_source().as_str(),
                receiver.effective_source().as_str()
            ),
        )
}

fn get_info_test(payer: &WalletState, receiver: &WalletState) -> TestResult {
    let pass = payer.details.is_some() && receiver.details.is_some();

    TestResult::new("get_info", "get_info", status_of(pass), true)
        .with_required_capability("get_info")
        .with_detail(
            "get_info answers with alias and network",
            format!(
                "payer_alias_present={} receiver_alias_present={} receiver_network={:?}",
                payer.details.as_ref().is_some_and(|d| d.alias.is_some()),
                receiver.details.as_ref().is_some_and(|d| d.alias.is_some()),
                receiver.details.as_ref().and_then(|d| d.network.clone())
            ),
        )
}

fn get_balance_test(payer: &WalletState, receiver: &WalletState) -> TestResult {
    let pass = payer.balance_before_msat.is_some() && receiver.balance_before_msat.is_some();

    TestResult::new("get_balance", "get_balance", status_of(pass), true)
        .with_required_capability("get_balance")
        .with_detail(
            "both wallets report a balance",
            format!(
                "payer_reported={} receiver_reported={}",
                payer.balance_before_msat.is_some(),
                receiver.balance_before_msat.is_some()
            ),
        )
}

fn make_invoice_test(pass: bool, diagnostics: Vec<String>) -> TestResult {
    TestResult::new("make_invoice", "make_invoice", status_of(pass), true)
        .with_required_capability("make_invoice")
        .with_detail(
            "receiver issues a 21000 msat invoice with a payment hash",
            if pass {
                String::from("invoice issued and parsed")
            } else {
                String::from("invoice missing or unexpected")
            },
        )
        .with_diagnostics(diagnostics)
}

async fn lookup(
    receiver: &WalletState,
    payment_hash_hex: Option<&str>,
    approved: bool,
    out: &mut Output,
) -> (Status, String, Vec<String>) {
    let Some(payment_hash_hex) = payment_hash_hex else {
        out.line("[receiver] lookup_invoice=skipped no_payment_hash");
        return (
            Status::Fail,
            String::from("no payment hash"),
            vec![String::from("reason=NO_PAYMENT_HASH")],
        );
    };

    let advertised = receiver.capabilities.advertises("lookup_invoice");
    if !advertised.is_yes() {
        let classified = classify_wallet_outcome(Observation::MethodNotAdvertised);
        out.line(format!(
            "[receiver] lookup_invoice=UNSUPPORTED advertised=false no_request_sent=true source={}",
            advertised.source().map_or("none", CapabilitySource::as_str)
        ));
        return (
            classified.status(),
            String::from("lookup_invoice not advertised, so no request was sent"),
            vec![format!("reason={}", classified.reason_code())],
        );
    }

    match receiver.session.lookup_invoice(payment_hash_hex).await {
        Ok(found) => {
            let state = found
                .state
                .clone()
                .unwrap_or_else(|| String::from("unknown"));
            out.line(format!(
                "[receiver] lookup_invoice state={state} amount_msat={} fees_paid_msat={} settled_at_present={}",
                found.amount_msat,
                found.fees_paid_msat,
                found.settled_at.is_some()
            ));

            let expected = if approved { "settled" } else { "pending" };
            let pass = state == expected;

            (
                status_of(pass),
                format!("state={state}"),
                vec![
                    format!("expected_state={expected}"),
                    format!(
                        "source={}",
                        advertised.source().map_or("none", CapabilitySource::as_str)
                    ),
                ],
            )
        }
        Err(error) => {
            let classified = error.classify();
            out.line(format!(
                "[receiver] lookup_invoice=failed kind={} detail={} status={} reason={}",
                error.kind(),
                error.detail(),
                classified.status(),
                classified.reason_code()
            ));
            (
                classified.status(),
                format!("lookup failed: {}", error.detail()),
                vec![
                    format!("error_kind={}", error.kind()),
                    format!("reason={}", classified.reason_code()),
                ],
            )
        }
    }
}

fn sign_message_test(payer: &WalletState, out: &mut Output) -> TestResult {
    let advertised: Advertisement = payer.capabilities.advertises("sign_message");
    let classified = classify_wallet_outcome(Observation::MethodNotAdvertised);
    let source = advertised.source().map_or("none", CapabilitySource::as_str);

    let actual = if advertised.is_yes() {
        // The adapter has no sign_message request, so nothing is sent even when
        // a wallet claims to support the method.
        out.line(
            "[payer] sign_message=UNSUPPORTED advertised=true no_request_sent=true adapter_support=none",
        );
        String::from("advertised, but the adapter sends no sign_message request")
    } else {
        out.line(format!(
            "[payer] sign_message=UNSUPPORTED advertised=false no_request_sent=true source={source}"
        ));
        String::from("not advertised, so no request was sent")
    };

    TestResult::new("sign_message", "sign_message", classified.status(), false)
        .with_required_capability("sign_message")
        .with_detail("sign_message available or not advertised", actual)
        .with_diagnostic(format!("reason={}", classified.reason_code()))
        .with_diagnostic(format!("source={source}"))
}

/// Adds the value of the `secret=` parameter of a connection string, as a
/// secret, so it is scrubbed as well as the whole string.
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
