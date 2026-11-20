#!/usr/bin/env python3
"""Builds docs/COMPATIBILITY.md from the recorded real-wallet reports.

Reads examples/real-wallets/<label>/report.json and writes the matrix, the
differences that the data shows, the limits paragraph and the date. It prints
nothing but the output path.
"""

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
EVIDENCE = ROOT / "examples" / "real-wallets"
OUT = ROOT / "docs" / "COMPATIBILITY.md"
DATE = "2026-11-20"

PROFILE_TESTS = ["connect", "info_event", "get_info", "get_balance",
                 "make_invoice", "lookup_invoice"]


def load():
    rows = []
    for report_path in sorted(EVIDENCE.glob("*/report.json")):
        report = json.loads(report_path.read_text(encoding="utf-8"))
        tests = {test["id"]: test for test in report["tests"]}
        capabilities = report["capabilities"][0] if report["capabilities"] else None
        required = [test for test in report["tests"] if test["required"]]
        statuses = [test["status"] for test in required]
        if all(status == "PASS" for status in statuses):
            exit_code = 0
        else:
            priority = ["FAIL", "UNSUPPORTED", "BLOCKED", "NOT_TESTED"]
            exit_code = [1, 4, 3, 5][next(i for i, name in enumerate(priority)
                                          if name in statuses)]
        rows.append({
            "label": report_path.parent.name,
            "run_id": report["run_id"],
            "source": capabilities["effective_source"] if capabilities else "none",
            "encryption": report["environment"]["encryption_mode_selected"],
            "network": report["environment"]["network_reported"],
            "advertised": len(capabilities["effective_methods"]) if capabilities else 0,
            "discrepancies": capabilities["discrepancies"] if capabilities else [],
            "tests": tests,
            "capability_checks": [
                (test["id"], test["status"])
                for test in report["tests"]
                if test["id"].startswith("capability_")
            ],
            "balance_msat": next(
                (line.split("=", 1)[1] for line in
                 tests["get_balance"]["diagnostics"]
                 if line.startswith("balance_msat=")), "unknown"),
            "relays": next(
                (line.split("=", 1)[1] for line in
                 tests["connect"]["diagnostics"]
                 if line.startswith("relays_connected=")), "unknown"),
            "invoice_amount": (report["workflow_evidence"]["invoice"] or {}).get("amount_msat"),
            "exit_code": exit_code,
        })
    return rows


def status(rows, test_id):
    return "|".join(row["tests"].get(test_id, {}).get("status", "NOT RUN")
                    for row in rows)


def table(rows):
    header = ("| Wallet | Capability source | Encryption | Network | Methods "
              "advertised | connect | info_event | get_info | get_balance | "
              "make_invoice | lookup_invoice | Exit code |")
    divider = "| --- " * 12 + "|"
    lines = [header, divider]
    for row in rows:
        lines.append(
            "| {label} | {source} | {encryption} | {network} | {advertised} | "
            "{connect} | {info} | {getinfo} | {balance} | {make} | {lookup} | "
            "{exit_code} |".format(
                label=row["label"], source=row["source"],
                encryption=row["encryption"] or "not recorded",
                network=row["network"] or "not reported",
                advertised=row["advertised"],
                connect=row["tests"].get("connect", {}).get("status", "NOT RUN"),
                info=row["tests"].get("info_event", {}).get("status", "NOT RUN"),
                getinfo=row["tests"].get("get_info", {}).get("status", "NOT RUN"),
                balance=row["tests"].get("get_balance", {}).get("status", "NOT RUN"),
                make=row["tests"].get("make_invoice", {}).get("status", "NOT RUN"),
                lookup=row["tests"].get("lookup_invoice", {}).get("status", "NOT RUN"),
                exit_code=row["exit_code"]))
    return lines


def capability_table(rows):
    methods = []
    for row in rows:
        for name, _ in row["capability_checks"]:
            if name not in methods:
                methods.append(name)
    header = "| Wallet | " + " | ".join(methods) + " |"
    divider = "| --- " * (len(methods) + 1) + "|"
    lines = [header, divider]
    for row in rows:
        checks = dict(row["capability_checks"])
        cells = [checks.get(name, "not required") for name in methods]
        lines.append("| " + row["label"] + " | " + " | ".join(cells) + " |")
    return lines


def differences(rows):
    lines = []
    networks = sorted({row["network"] or "not reported" for row in rows})
    lines.append("- Network reported: " + ", ".join(networks) + ".")
    ciphers = sorted({row["encryption"] or "not recorded" for row in rows})
    lines.append("- Encryption mode selected: " + ", ".join(ciphers) + ".")
    sources = sorted({row["source"] for row in rows})
    lines.append("- Capability source: " + ", ".join(sources) + ".")
    counts = ", ".join(f"{row['label']} {row['advertised']}" for row in rows)
    lines.append("- Methods advertised by the effective source: " + counts + ".")
    relays = ", ".join(f"{row['label']} {row['relays']}" for row in rows)
    lines.append("- Relays reached during connect: " + relays + ".")
    balances = ", ".join(f"{row['label']} {row['balance_msat']} msat"
                         for row in rows)
    lines.append("- Balance reported at inspection time: " + balances + ".")
    for row in rows:
        if row["discrepancies"]:
            for item in row["discrepancies"]:
                lines.append(
                    f"- {row['label']}: {item['method']} is listed by "
                    f"{item['present_in']} but not by {item['absent_in']}.")
        else:
            lines.append(f"- {row['label']}: the two capability sources agree.")
    failed = [f"{row['label']} {name}={state}"
              for row in rows for name, state in row["capability_checks"]
              if state != "PASS"]
    if failed:
        lines.append("- Capability checks that did not pass: " + ", ".join(failed) + ".")
    else:
        lines.append("- Every required capability check passed on every wallet.")
    return lines


def main():
    rows = load()
    if not rows:
        print("no reports found", file=sys.stderr)
        return 1

    document = ["# Compatibility matrix", "",
                "Built from the reports in `examples/real-wallets/` by "
                "`scripts/build-compatibility.py`. Every value below is read "
                "from those JSON files; no wallet was paid.", "",
                "## Inspections", ""]
    document += table(rows)
    document += ["", "## Capability checks", "",
                 "A PASS here means the method was advertised by the effective "
                 "capability source. No request for the method was sent, so a "
                 "PASS is not evidence that the call works.", ""]
    document += capability_table(rows)
    document += ["", "## Differences found", ""]
    document += differences(rows)
    document += ["", "## Limits", "",
                 "Two connections on one date are a sample, not a survey. Only "
                 "one of them is a third-party provider; the other is the "
                 "sandbox Alby Hub that Tally already used. No payment was "
                 "made, so nothing here says whether a payment succeeds or "
                 "whether a preimage comes back. Permissions, budgets and "
                 "method lists belong to each NWC connection and differ "
                 "between connections to the same provider.", "",
                 "Date: " + DATE, ""]

    OUT.write_text("\n".join(document), encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
