//! The compact ASCII summary printed by the CLI.
//!
//! It is deliberately short: one line per test plus a few context lines. It
//! never prints an invoice, a preimage or a connection string, because the
//! report it reads holds none of them.

use tally_core::capabilities::CapabilitySource;
use tally_core::report::{RunReport, exit_code};

use crate::profile::Profile;
use crate::run::RunOutcome;

/// Builds the summary lines of one finished run.
#[must_use]
pub fn summary_lines(outcome: &RunOutcome) -> Vec<String> {
    let report = &outcome.report;
    let mut lines = Vec::new();

    lines.push(format!(
        "run {} profile {} env {} endpoint {}",
        report.run_id,
        outcome.profile,
        report.environment.class.as_str(),
        endpoint_class(report)
    ));

    for role in &report.capabilities {
        lines.push(format!(
            "capability_source {} {}",
            role.role.as_str(),
            match role.effective_source {
                CapabilitySource::InfoEvent => "info_event",
                CapabilitySource::GetInfo => "get_info",
                CapabilitySource::None => "none",
            }
        ));
    }

    for test in &report.tests {
        lines.push(format!(
            "test {} {} reason={}",
            test.id,
            test.status.as_str(),
            reason_of(&test.diagnostics)
        ));
    }

    if outcome.guard_cleared > 0 {
        lines.push(format!("guard_cleared {}", outcome.guard_cleared));
    }

    lines.push(String::from("limitations:"));
    for limitation in &report.limitations {
        lines.push(format!("- {limitation}"));
    }

    lines.push(format!("report json {}", display(&outcome.json_path)));
    lines.push(format!("report html {}", display(&outcome.html_path)));
    lines.push(format!("latest json {}", display(&outcome.latest_json)));
    lines.push(format!("exit_code {}", exit_code(report)));

    lines
}

/// The reason code of one test, as stored in its diagnostics.
#[must_use]
pub fn reason_of(diagnostics: &[String]) -> &str {
    diagnostics
        .iter()
        .find_map(|line| line.strip_prefix("reason="))
        .unwrap_or("UNKNOWN")
}

fn endpoint_class(report: &RunReport) -> &'static str {
    use tally_core::report::EndpointClass;
    match report.environment.endpoint_class {
        EndpointClass::LocalFixture => "local_fixture",
        EndpointClass::ExternalSandbox => "external_sandbox",
        EndpointClass::None => "none",
    }
}

fn display(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// The profile name of a titled run, for headers.
#[must_use]
pub fn profile_header(profile: Profile) -> String {
    format!("tally {} profile", profile.as_str())
}
