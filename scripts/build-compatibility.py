#!/usr/bin/env python3
"""Builds docs/COMPATIBILITY.md from the recorded real-wallet reports.

Reads examples/real-wallets/<label>/report.json and writes the matrix, the
per-method support table, the differences that the data shows, the limits
paragraph and the date. It prints nothing but the output path.
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

# The info event carries method names and this flag in the same token list.
# The flag is not a method, so it is excluded from every method count and
# from the method support table.
NOTIFICATIONS_FLAG = "notifications"


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
        effective = capabilities["effective_methods"] if capabilities else []
        methods = [name for name in effective if name != NOTIFICATIONS_FLAG]
        get_info = capabilities["get_info"] if capabilities else None
        rows.append({
            "label": report_path.parent.name,
            "run_id": report["run_id"],
            "source": capabilities["effective_source"] if capabilities else "none",
            "encryption": report["environment"]["encryption_mode_selected"],
            "network": report["environment"]["network_reported"],
            "methods": methods,
            "info_event_methods": len(methods),
            "get_info_methods": len(get_info["methods"]) if get_info else 0,
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


def table(rows):
    header = ("| Wallet | Capability source | Encryption | Network | "
              "Info-event methods | get_info methods | connect | info_event | "
              "get_info | get_balance | make_invoice | lookup_invoice | "
              "Exit code |")
    divider = "| --- " * 13 + "|"
    lines = [header, divider]
    for row in rows:
        lines.append(
            "| {label} | {source} | {encryption} | {network} | {info_count} | "
            "{getinfo_count} | {connect} | {info} | {getinfo} | {balance} | "
            "{make} | {lookup} | {exit_code} |".format(
                label=row["label"], source=row["source"],
                encryption=row["encryption"] or "not recorded",
                network=row["network"] or "not reported",
                info_count=row["info_event_methods"],
                getinfo_count=row["get_info_methods"],
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


def capability_summary(rows):
    failed = [f"{row['label']} {name}={state}"
              for row in rows for name, state in row["capability_checks"]
              if state != "PASS"]
    if failed:
        return "Capability checks that did not pass: " + ", ".join(failed) + "."
    return "Every required capability check passed on every wallet."


def method_support(rows):
    union = sorted({name for row in rows for name in row["methods"]})
    sources = sorted({row["source"] for row in rows})
    lines = [
        "Effective capability source used: " + ", ".join(sources) + " "
        "(the info event governs when it lists at least one method token). "
        "`yes` means the method is in that wallet's effective source; `-` "
        "means it is not. The `notifications` token is a flag, not a method "
        "name, so it is left out of this table.",
        "",
    ]
    header = "| Method | " + " | ".join(row["label"] for row in rows) + " |"
    divider = "| --- " * (len(rows) + 1) + "|"
    lines += [header, divider]
    for name in union:
        cells = ["yes" if name in row["methods"] else "-" for row in rows]
        lines.append("| " + name + " | " + " | ".join(cells) + " |")
    return lines


def differences(rows):
    lines = []
    labels = [row["label"] for row in rows]
    union = sorted({name for row in rows for name in row["methods"]})
    unique_lines = []
    for row in rows:
        others = set()
        for other in rows:
            if other is not row:
                others.update(other["methods"])
        only = [name for name in union
                if name in row["methods"] and name not in others]
        if only:
            unique_lines.append(f"- Methods only in {row['label']}: " +
                                ", ".join(only) + ".")
    if unique_lines:
        lines += unique_lines
    else:
        lines.append("- Every wallet advertises the same method names.")

    for row in rows:
        for item in row["discrepancies"]:
            lines.append(
                f"- Capability-source disagreement on {row['label']}: "
                f"{item['method']} is listed by {item['present_in']} but not "
                f"by {item['absent_in']}.")

    relays = ", ".join(f"{row['label']} {row['relays']}" for row in rows)
    lines.append("- Relays listed in each connection: " + relays + ".")

    identical = []
    for label, key, fallback in (("network", "network", "not reported"),
                                 ("encryption mode", "encryption", "not recorded"),
                                 ("capability source", "source", "none")):
        found = {row[key] or fallback for row in rows}
        if len(found) == 1:
            identical.append(f"{label} {found.pop()}")
    if identical and len(rows) > 1:
        lines.append("- Identical on every wallet: " + "; ".join(identical) + ".")
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
                "## Inspections", "",
                "Method counts are method names only: the `notifications` "
                "token is a flag, not a method, so it is excluded from the "
                "info-event count (an info event with 14 tokens including the "
                "flag shows 13). The `get_info` count is that response's "
                "method list.", ""]
    document += table(rows)
    document += ["", "## Method support", ""]
    document += method_support(rows)
    document += ["", "## Capability checks", "",
                 "A PASS here means the method was advertised by the effective "
                 "capability source. No request for the method was sent, so a "
                 "PASS is not evidence that the call works.", ""]
    document += capability_table(rows)
    document += ["", capability_summary(rows), "", "## Differences found", ""]
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
