# Architecture

Tally is a Rust workspace. One binary, `tally`, drives one run and writes two reports from one report value.

## Crates and dependency direction

```
+---------------------------+
| apps/tally-cli            |  binary "tally": doctor, inspect, test, report
+-------------+-------------+
              |
              v
+---------------------------+
| crates/tally-runner       |  profiles, orchestration, run guard, renderers, summary
+--------+---------+--------+
         |         |
         v         v
+---------------+ +---------------+
| crates/tally- | | crates/tally- |  L402 engine      NWC adapter
| l402          | | nwc           |
+-------+-------+ +-------+-------+
        |                 |
        +--------+--------+
                 v
        +-----------------+
        | crates/tally-   |  status table, outcome table, capabilities,
        | core            |  report model, redaction, time
        +-----------------+
```

The intended direction is `apps -> tally-runner -> tally-l402 and tally-nwc -> tally-core`, and the manifests follow it. `apps/tally-server` depends only on the CLI scaffolding and `web/` is static files; neither has protocol logic yet.

## The run flow

`crates/tally-runner/src/run.rs` executes the selected profile in this order:

1. Read the guard directory. If a `PAYING` or `AMBIGUOUS` file exists, the payment test is BLOCKED with reason `RUN_GUARD_BLOCKED`; with `clear_guard` the blocking files are removed first and the count is reported.
2. Connect both wallets concurrently (one task per wallet) and count the relays each one reached.
3. Fetch each wallet's kind 13194 info event within `DEFAULT_INFO_EVENT_TIMEOUT` (20 seconds) and build the capability view.
4. Call `get_info` on both wallets and compare it with the info event.
5. Call `get_balance` on both wallets, in millisatoshis.
6. Ask the fixture for the L402 challenge (HTTP 402 plus `WWW-Authenticate`).
7. Validate the invoice: amount, expiry, network and payment hash.
8. Check the binding: the macaroon identifier must carry that invoice's payment hash.
9. Pay, through a `PaymentPermit`, with the run guard in state `PAYING`; then `PAID` or `FAILED`, or `AMBIGUOUS` when no wallet error code came back.
10. Verify the preimage: SHA-256 of the preimage must equal the payment hash.
11. Present the credentials to the fixture and require status 200 and the expected body.
12. Run the two negative challenge checks (a tampered proof and a macaroon without a preimage), which must both be refused with 402.
13. Look the invoice up at the wallet and compare the lookup state with the payment outcome.
14. Probe `sign_message`: advertised means NOT TESTED (no API in `nwc` 0.45.2), not advertised means UNSUPPORTED; no request is sent either way.

The guard check is not a test. Each of the other steps records one `TestResult` with a status, a reason code, expected and observed text, and diagnostics. `tally-core`'s outcome table is the only place that maps an observation to a status.

## NWC lifecycle

- Connect: `WalletSession::connect` opens the relay connections from the connection string. The connect wait is `DEFAULT_CONNECT_WAIT` (15 seconds) and each request has `DEFAULT_REQUEST_TIMEOUT` (30 seconds).
- Cipher: the info event's `encryption` tag decides. `nip44_v2` is selected when the tag lists it, otherwise `nip04`, including when there is no tag. The choice is passed to the `nwc` builder explicitly (`force_nip44_v2` or `force_nip04`). The recorded run shows both tokens advertised (`nip44_v2`, `nip04`); the report field `encryption_mode_selected` is null in that Step 7 report because it predates the fill; current runs record the payer session's cipher there, and the Step 10 real-wallet inspections record `nip44_v2`.
- Requests: one request, one response, no retry. A request that carries no wallet error code is BLOCKED, not FAIL.
- Errors: NIP-47 codes are read from the error message by `crates/tally-nwc/src/error.rs` and mapped through the shared table.

## L402 lifecycle

1. The resource answers 402 with `WWW-Authenticate: L402 macaroon="...", invoice="..."`. Parsing is in `crates/tally-l402/src/challenge.rs` and enforces `MAX_HEADER_BYTES` (8192) and `MAX_MACAROON_BYTES` (4096).
2. Tally checks the invoice and the macaroon identifier, then pays through a `PaymentPermit` (`crates/tally-nwc/src/permit.rs`), which binds the invoice text, the approved amount, the payment hash and the environment and is consumed by the payment.
3. Tally verifies the preimage locally: SHA-256 of the preimage hex must equal the payment hash (`crates/tally-nwc/src/session.rs`).
4. Tally presents `Authorization: L402 <macaroon>:<preimage>` to the fixture and requires 200 plus the expected body.
5. Tamper controls: a tampered macaroon, a wrong preimage, a macaroon without a preimage, an extra caveat and a macaroon from another fixture must all be refused with 402. The fixture returns an identical 402 shape for every failure (`crates/tally-l402/src/fixture.rs`).

The fixture is local. It proves Tally's flow, not compatibility with third-party L402 services.

## Data flow

Connection strings live only in `secrecy::SecretString` inside `tally-nwc`. Invoices, preimages and macaroons live in types that redact their `Debug` and `Display` output. A run produces one `RunReport` (`crates/tally-core/src/report.rs`) holding the environment, wallet summaries, capability views, test results, workflow evidence and limitations. `to_json` and `to_html` in `crates/tally-runner/src/render.rs` both take that value, so the two files cannot disagree; `write_reports` also copies the newest pair into `reports/latest/`.

The report model has no field for a connection string, a BOLT11 invoice, a preimage or a macaroon. Payment evidence records only whether a preimage was present and whether its hash matched.

## Trust boundaries

| Boundary | What crosses it | What protects it |
| --- | --- | --- |
| Process and disk | Connection strings in `.env.payer` and `.env.receiver` | Files are gitignored; values are read into `SecretString` and never printed |
| Process and relays | NIP-47 requests and responses | No secret is logged; the connection string never appears in output |
| Process and hub | Wallet calls, including one payment | Single-use permit, approval amount, environment class, run guard |
| Process and fixture | HTTP over loopback only | `L402Client` refuses https and non-loopback hosts (`client::tests::refuses_https_and_non_loopback_urls`) |

## Failure handling

- Status taxonomy: PASS, FAIL, UNSUPPORTED, NOT TESTED, BLOCKED. One table in `crates/tally-core/src/outcome.rs` decides.
- Reason codes: each result carries a stable token such as `EXPECTED_BEHAVIOUR_OBSERVED`, `PAYMENT_NOT_APPROVED`, `RUN_GUARD_BLOCKED`, `PREREQUISITE_FAILED`, `NO_PREIMAGE`, `PREIMAGE_MISMATCH`, `METHOD_NOT_ADVERTISED`, `TRANSPORT_PROBLEM`.
- Exit codes: 0 all required tests passed, 1 a required test failed, 2 usage or configuration error before any test ran, 3 a required test was blocked, 4 a required test was unsupported, 5 a required test was not tested. When several required tests are not PASS the priority is FAIL, then UNSUPPORTED, then BLOCKED, then NOT TESTED, so an unsupported required test is not masked by downstream blocked tests (`report::tests::exit_code_priority_between_mixed_required_results`).
- Run guard states: `PAYING`, `PAID`, `FAILED`, `AMBIGUOUS`, stored as `runs/<payment hash>.state` by `crates/tally-runner/src/guard.rs`. `PAYING` and `AMBIGUOUS` block later runs; `--clear-guard` removes them and nothing else.

## Report model

One JSON document per run, `schema_version` 1, containing `run_id`, `started_at`, `finished_at`, `duration_ms`, `tally_version`, `environment`, `wallets`, `capabilities`, `tests`, `workflow_evidence` and `limitations`. The HTML report renders the same fields with a status legend and per-status counts. `tally report` re-renders a stored JSON file and refuses an unknown `schema_version` with exit 2 (`report_rejects_an_unknown_schema_version`).

## Extension points

- `WalletApi` (`crates/tally-nwc/src/wallet_api.rs`): the runner talks to this trait, not to the `nwc` crate. A new wallet library or a fake wallet implements the trait; the runner does not change.
- `InvoiceSource` (`crates/tally-l402/src/fixture.rs`): the fixture asks an invoice source for the invoice it puts in a challenge. `NwcInvoiceSource<W: WalletApi>` is the real one; the TEST ONLY generator plugs in behind the off-by-default `test-support` feature.
