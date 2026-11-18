//! The two renderers. Both read the same [`RunReport`] value.
//!
//! The HTML is one self-contained ASCII file: no external resource, no
//! JavaScript, every dynamic value escaped and every non-ASCII character written
//! as a numeric character reference.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tally_core::report::{EndpointClass, EnvClass, RunReport, TestResult, exit_code};
use tally_core::status::Status;

use crate::run::TALLY_VERSION;

/// The five statuses, in the order the legend lists them.
pub const STATUSES: [Status; 5] = [
    Status::Pass,
    Status::Fail,
    Status::Unsupported,
    Status::NotTested,
    Status::Blocked,
];

/// What one renderer produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenReports {
    /// `reports/<run_id>/report.json`.
    pub json: PathBuf,
    /// `reports/<run_id>/report.html`.
    pub html: PathBuf,
    /// `reports/latest/report.json`.
    pub latest_json: PathBuf,
    /// `reports/latest/report.html`.
    pub latest_html: PathBuf,
}

/// Writes both report files and copies them into `latest/`.
///
/// # Errors
///
/// Any filesystem error.
pub fn write_reports(
    report: &RunReport,
    out_dir: &Path,
) -> io::Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let written = write_report_files(report, out_dir)?;

    Ok((
        written.json,
        written.html,
        written.latest_json,
        written.latest_html,
    ))
}

/// Writes both report files and copies them into `latest/`.
///
/// # Errors
///
/// Any filesystem error.
pub fn write_report_files(report: &RunReport, out_dir: &Path) -> io::Result<WrittenReports> {
    let run_dir = out_dir.join(&report.run_id);
    fs::create_dir_all(&run_dir)?;

    let json = run_dir.join("report.json");
    let html = run_dir.join("report.html");
    fs::write(&json, to_json(report))?;
    fs::write(&html, to_html(report))?;

    let latest = out_dir.join("latest");
    fs::create_dir_all(&latest)?;
    let latest_json = latest.join("report.json");
    let latest_html = latest.join("report.html");
    fs::copy(&json, &latest_json)?;
    fs::copy(&html, &latest_html)?;

    Ok(WrittenReports {
        json,
        html,
        latest_json,
        latest_html,
    })
}

/// The JSON report, pretty printed.
#[must_use]
pub fn to_json(report: &RunReport) -> String {
    match serde_json::to_string_pretty(report) {
        Ok(text) => format!("{text}\n"),
        // A RunReport always serializes; this keeps the signature total without
        // ever printing a secret.
        Err(_) => String::from("{}\n"),
    }
}

/// The HTML report.
#[must_use]
pub fn to_html(report: &RunReport) -> String {
    let code = exit_code(report);
    let mut html = String::new();

    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    html.push_str("<title>Tally report ");
    html.push_str(&escape(&report.run_id));
    html.push_str("</title>\n<style>\n");
    html.push_str(CSS);
    html.push_str("</style>\n</head>\n<body>\n<main>\n");

    html.push_str("<h1>Tally report</h1>\n");
    html.push_str("<p class=\"meta\">Run ");
    html.push_str(&escape(&report.run_id));
    html.push_str(" started ");
    html.push_str(&escape(&report.started_at));
    html.push_str(", finished ");
    html.push_str(&escape(&report.finished_at));
    html.push_str(", Tally ");
    html.push_str(&escape(TALLY_VERSION));
    html.push_str(".</p>\n");

    verdict_section(&mut html, report, code);
    environment_section(&mut html, report);
    capabilities_section(&mut html, report);
    tests_section(&mut html, report);
    evidence_section(&mut html, report);
    legend_section(&mut html);

    html.push_str("</main>\n</body>\n</html>\n");
    html
}

fn verdict_section(html: &mut String, report: &RunReport, code: i32) {
    html.push_str("<section>\n<h2>Overall verdict</h2>\n");
    html.push_str("<p class=\"verdict\">Exit code ");
    html.push_str(&code.to_string());
    html.push_str(": ");
    html.push_str(&escape(exit_meaning(code)));
    html.push_str(".</p>\n");

    html.push_str("<table>\n<caption>Tests per status</caption>\n");
    html.push_str("<thead><tr><th scope=\"col\">Status</th><th scope=\"col\">Count</th></tr></thead>\n<tbody>\n");
    for status in STATUSES {
        let count = report
            .tests
            .iter()
            .filter(|test| test.status == status)
            .count();
        html.push_str("<tr><th scope=\"row\">");
        html.push_str(&status_label(status));
        html.push_str("</th><td>");
        html.push_str(&count.to_string());
        html.push_str("</td></tr>\n");
    }
    html.push_str("</tbody>\n</table>\n</section>\n");
}

fn environment_section(html: &mut String, report: &RunReport) {
    html.push_str("<section>\n<h2>Environment and limitations</h2>\n");
    html.push_str("<table>\n<caption>Environment</caption>\n<tbody>\n");
    row(html, "Environment class", env_label(report));
    row(html, "Endpoint class", endpoint_label(report));
    row(
        html,
        "NWC client",
        &format!(
            "{} {}",
            report.environment.nwc_client.name, report.environment.nwc_client.version
        ),
    );
    row(
        html,
        "Encryption mode",
        report
            .environment
            .encryption_mode_selected
            .as_deref()
            .unwrap_or("not recorded"),
    );
    row(
        html,
        "Wallet network",
        report
            .environment
            .network_reported
            .as_deref()
            .unwrap_or("not reported"),
    );
    html.push_str("</tbody>\n</table>\n");

    html.push_str("<h3>Limitations</h3>\n<ul class=\"limitations\">\n");
    if report.limitations.is_empty() {
        html.push_str("<li>None recorded.</li>\n");
    }
    for limitation in &report.limitations {
        html.push_str("<li>");
        html.push_str(&escape(limitation));
        html.push_str("</li>\n");
    }
    html.push_str("</ul>\n</section>\n");
}

fn capabilities_section(html: &mut String, report: &RunReport) {
    html.push_str("<section>\n<h2>Capabilities per wallet</h2>\n");

    if report.capabilities.is_empty() {
        html.push_str("<p>No capability data was collected.</p>\n");
        html.push_str("</section>\n");
        return;
    }

    html.push_str("<table>\n<caption>Advertised methods and disagreements</caption>\n");
    html.push_str("<thead><tr><th scope=\"col\">Wallet</th><th scope=\"col\">Governing source</th><th scope=\"col\">Methods</th><th scope=\"col\">Discrepancies</th></tr></thead>\n<tbody>\n");

    for role in &report.capabilities {
        html.push_str("<tr><th scope=\"row\">");
        html.push_str(role.role.as_str());
        html.push_str("</th><td>");
        html.push_str(&escape(role.effective_source.as_str()));
        html.push_str("</td><td>");
        html.push_str(&escape(&role.effective_methods.join(" ")));
        html.push_str("</td><td>");
        if role.discrepancies.is_empty() {
            html.push_str("none");
        } else {
            let items = role
                .discrepancies
                .iter()
                .map(|item| {
                    format!(
                        "{} listed by {} only",
                        item.method,
                        item.present_in.as_str()
                    )
                })
                .collect::<Vec<String>>();
            html.push_str(&escape(&items.join("; ")));
        }
        html.push_str("</td></tr>\n");
    }

    html.push_str("</tbody>\n</table>\n");
    html.push_str("<p class=\"meta\">Info events: ");
    html.push_str(
        &report
            .capabilities
            .iter()
            .map(|role| format!("{} found={}", role.role.as_str(), role.info_event.found))
            .collect::<Vec<String>>()
            .join("; "),
    );
    html.push_str("</p>\n</section>\n");
}

fn tests_section(html: &mut String, report: &RunReport) {
    html.push_str("<section>\n<h2>Tests</h2>\n");
    html.push_str("<table>\n<caption>Every test in run order</caption>\n");
    html.push_str("<thead><tr><th scope=\"col\">Test</th><th scope=\"col\">Status</th><th scope=\"col\">Required</th><th scope=\"col\">Expected</th><th scope=\"col\">Observed</th><th scope=\"col\">Details</th></tr></thead>\n<tbody>\n");

    for test in &report.tests {
        test_row(html, test);
    }

    html.push_str("</tbody>\n</table>\n</section>\n");
}

fn test_row(html: &mut String, test: &TestResult) {
    html.push_str("<tr><th scope=\"row\">");
    html.push_str(&escape(&test.id));
    html.push_str("</th><td class=\"status ");
    html.push_str(status_class(test.status));
    html.push_str("\">");
    html.push_str(&status_label(test.status));
    html.push_str("</td><td>");
    html.push_str(if test.required { "yes" } else { "no" });
    html.push_str("</td><td>");
    html.push_str(&escape(&test.expected));
    html.push_str("</td><td>");
    html.push_str(&escape(&test.actual));
    html.push_str("</td><td>");
    html.push_str(&escape(&test.diagnostics.join("; ")));
    html.push_str("</td></tr>\n");
}

fn evidence_section(html: &mut String, report: &RunReport) {
    html.push_str("<section>\n<h2>Workflow evidence</h2>\n");
    let evidence = &report.workflow_evidence;

    html.push_str("<table>\n<caption>Invoice, payment and protected resource</caption>\n<tbody>\n");

    match &evidence.invoice {
        Some(invoice) => {
            row(
                html,
                "Invoice network",
                invoice.network.as_deref().unwrap_or("unknown"),
            );
            row(
                html,
                "Invoice amount (msat)",
                &invoice.amount_msat.to_string(),
            );
            row(
                html,
                "Payment hash",
                invoice.payment_hash.as_deref().unwrap_or("redacted"),
            );
            row(
                html,
                "Seconds until expiry",
                &invoice.seconds_until_expiry.to_string(),
            );
        }
        None => row(html, "Invoice", "not collected"),
    }

    match &evidence.payment {
        Some(payment) => {
            row(
                html,
                "Payment outcome",
                payment_outcome_label(payment.outcome),
            );
            row(
                html,
                "Fees paid (msat)",
                &payment
                    .fees_paid_msat
                    .map_or_else(|| String::from("unknown"), |fees| fees.to_string()),
            );
            row(
                html,
                "Preimage present",
                &payment.preimage_present.to_string(),
            );
            row(
                html,
                "Preimage matches payment hash",
                &payment
                    .preimage_matches_payment_hash
                    .map_or_else(|| String::from("not checked"), |value| value.to_string()),
            );
        }
        None => row(html, "Payment", "not collected"),
    }

    match &evidence.challenge {
        Some(challenge) => {
            row(html, "Challenge status", &challenge.http_status.to_string());
            row(html, "Challenge scheme", &challenge.scheme);
            row(
                html,
                "Macaroon present",
                &challenge.macaroon_present.to_string(),
            );
            row(
                html,
                "Invoice present",
                &challenge.invoice_present.to_string(),
            );
            row(
                html,
                "Binding checked",
                &challenge.binding_checked.to_string(),
            );
            row(
                html,
                "Binding matches",
                &challenge
                    .binding_matches
                    .map_or_else(|| String::from("not checked"), |value| value.to_string()),
            );
        }
        None => row(html, "Challenge", "not collected"),
    }

    match &evidence.protected_resource {
        Some(resource) => {
            row(
                html,
                "Unauthenticated status",
                &resource.unauthenticated_status.to_string(),
            );
            row(
                html,
                "Wrong proof status",
                &resource
                    .wrong_proof_status
                    .map_or_else(|| String::from("not tried"), |status| status.to_string()),
            );
            row(
                html,
                "Valid proof status",
                &resource
                    .valid_proof_status
                    .map_or_else(|| String::from("not tried"), |status| status.to_string()),
            );
            row(
                html,
                "Body matches expected",
                &resource
                    .body_matches_expected
                    .map_or_else(|| String::from("not unlocked"), |value| value.to_string()),
            );
        }
        None => row(html, "Protected resource", "not collected"),
    }

    html.push_str("</tbody>\n</table>\n</section>\n");
}

fn legend_section(html: &mut String) {
    html.push_str("<section>\n<h2>Status legend</h2>\n");
    html.push_str("<dl class=\"legend\">\n");
    for status in STATUSES {
        html.push_str("<dt class=\"status ");
        html.push_str(status_class(status));
        html.push_str("\">");
        html.push_str(&status_label(status));
        html.push_str("</dt><dd>");
        html.push_str(&escape(status_meaning(status)));
        html.push_str("</dd>\n");
    }
    html.push_str("</dl>\n</section>\n");
}

fn row(html: &mut String, label: &str, value: &str) {
    html.push_str("<tr><th scope=\"row\">");
    html.push_str(&escape(label));
    html.push_str("</th><td>");
    html.push_str(&escape(value));
    html.push_str("</td></tr>\n");
}

/// The text label of a status. Colour is only ever an addition to this.
#[must_use]
pub fn status_label(status: Status) -> String {
    status.to_string()
}

/// The CSS class that adds colour to a status label.
#[must_use]
pub const fn status_class(status: Status) -> &'static str {
    match status {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::Unsupported => "unsupported",
        Status::NotTested => "not-tested",
        Status::Blocked => "blocked",
    }
}

fn status_meaning(status: Status) -> &'static str {
    match status {
        Status::Pass => "The expected behaviour was observed.",
        Status::Fail => "The test ran and the expected behaviour was not observed.",
        Status::Unsupported => {
            "The wallet does not offer what the test needs; no request was sent."
        }
        Status::NotTested => {
            "The test was not run, for example because the run was not approved to pay."
        }
        Status::Blocked => {
            "The test could not run because something outside the wallet blocked it."
        }
    }
}

fn exit_meaning(code: i32) -> &'static str {
    match code {
        0 => "every required test passed",
        1 => "a required test failed",
        2 => "usage or configuration error before any test ran",
        3 => "a required test was blocked",
        4 => "a required test was unsupported",
        5 => "a required test was not tested",
        _ => "unknown exit code",
    }
}

fn env_label(report: &RunReport) -> &'static str {
    match report.environment.class {
        EnvClass::Sandbox => "sandbox",
        EnvClass::Live => "live",
    }
}

fn endpoint_label(report: &RunReport) -> &'static str {
    match report.environment.endpoint_class {
        EndpointClass::LocalFixture => "local_fixture",
        EndpointClass::ExternalSandbox => "external_sandbox",
        EndpointClass::None => "none",
    }
}

fn payment_outcome_label(outcome: tally_core::report::PaymentOutcome) -> &'static str {
    use tally_core::report::PaymentOutcome;
    match outcome {
        PaymentOutcome::Paid => "paid",
        PaymentOutcome::Failed => "failed",
        PaymentOutcome::Ambiguous => "ambiguous",
        PaymentOutcome::NotAttempted => "not_attempted",
    }
}

/// Escapes a dynamic value for HTML and rewrites every non-ASCII character as a
/// numeric character reference, so the file stays ASCII.
#[must_use]
pub fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character if character.is_ascii() => escaped.push(character),
            character => {
                let mut buffer = [0u16; 2];
                for unit in character.encode_utf16(&mut buffer) {
                    escaped.push_str(&format!("&#{unit};"));
                }
            }
        }
    }

    escaped
}

const CSS: &str = r#"
:root { color-scheme: light dark; --bg: #ffffff; --fg: #1a1a1a; --muted: #555555; --line: #d0d0d0; --pass: #146c2e; --fail: #a11b1b; --unsupported: #6a4b00; --not-tested: #4a4a4a; --blocked: #7a2a00; }
@media (prefers-color-scheme: dark) { :root { --bg: #14161a; --fg: #eceff3; --muted: #b3b9c2; --line: #343a44; --pass: #6fd18a; --fail: #ff8a80; --unsupported: #f0c05a; --not-tested: #c2c8d2; --blocked: #ffab73; } }
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg); font-family: system-ui, -apple-system, "Segoe UI", Arial, sans-serif; line-height: 1.5; }
main { max-width: 60rem; margin: 0 auto; padding: 1rem; }
h1 { font-size: 1.6rem; margin: 0.5rem 0; }
h2 { font-size: 1.25rem; margin-top: 2rem; border-bottom: 1px solid var(--line); padding-bottom: 0.25rem; }
h3 { font-size: 1.05rem; margin-top: 1.25rem; }
p, li, dd, dt { font-size: 0.95rem; }
.meta { color: var(--muted); }
table { width: 100%; border-collapse: collapse; margin: 0.75rem 0; display: block; overflow-x: auto; }
caption { text-align: left; font-weight: 600; padding: 0.25rem 0; }
th, td { border: 1px solid var(--line); padding: 0.35rem 0.5rem; text-align: left; vertical-align: top; }
thead th { background: rgba(127, 127, 127, 0.12); }
.status { font-weight: 700; white-space: nowrap; }
.status.pass { color: var(--pass); }
.status.fail { color: var(--fail); }
.status.unsupported { color: var(--unsupported); }
.status.not-tested { color: var(--not-tested); }
.status.blocked { color: var(--blocked); }
.verdict { font-weight: 600; }
.legend { display: grid; grid-template-columns: max-content 1fr; gap: 0.25rem 1rem; }
.legend dt { margin: 0; }
.legend dd { margin: 0; }
@media (max-width: 40rem) { .legend { grid-template-columns: 1fr; } }
"#;
