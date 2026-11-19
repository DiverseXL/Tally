# Testing

Every number here comes from `cargo test --workspace -- --list` and `cargo test --workspace` on this machine, not from memory.

## Layers and commands

| Layer | Command | Needs network | Needs wallets |
| --- | --- | --- | --- |
| Unit and integration tests | `cargo test --workspace` | no | no |
| Recorded live sandbox run | `cargo run -q -p tally-cli -- test --profile l402-e2e --approve-payment-sats 21` | yes | yes, and it spends 21 sandbox sats |
| CI | `.github/workflows/ci.yml` | no | no |

## Counts

`cargo test --workspace` runs 166 tests:

| Binary | Tests |
| --- | --- |
| tally-core (lib) | 27 |
| tally-l402 (lib) | 34 |
| tally-l402 (`crates/tally-l402/tests/fixture.rs`) | 13 |
| tally-nwc (lib) | 31 |
| tally-runner (lib) | 10 |
| tally-runner (`crates/tally-runner/tests/observer.rs`) | 2 |
| tally-runner (`crates/tally-runner/tests/renderer.rs`) | 9 |
| tally-runner (`crates/tally-runner/tests/scenarios.rs`) | 15 |
| tally-cli (`apps/tally-cli/tests/cli.rs`) | 9 |
| tally-server (`apps/tally-server/tests/api.rs`) | 16 |

Per module inside the libraries: `status::tests` 2, `outcome::tests` 3, `capabilities::tests` 11, `report::tests` 4, `time::tests` 3, `redact::tests` 4, `connection::tests` 4, `guard::tests` 5, `error::tests` 3, `permit::tests` 7, `session::tests` 9, `invoice::tests` 7, `challenge::tests` 13, `macaroon::tests` 12, `client::tests` 2, runner `guard::tests` 6, `profile::tests` 3, `config::tests` 1.

## The fifteen runner scenarios

`crates/tally-runner/tests/scenarios.rs` drives the real loopback fixture through the real runner, with the wallet pair faked.

```
# test names
scenario_01_happy_path - all 14 tests, exit 0, one payment of 21000 msat
scenario_02_no_approval_flag - payment tests NOT TESTED with PAYMENT_NOT_APPROVED, exit 5, no payment
scenario_03_receiver_does_not_advertise_make_invoice - challenge UNSUPPORTED, pay BLOCKED, exit 4
scenario_04_connect_transport_timeout - connect BLOCKED, exit 3
scenario_05_expired_or_wrong_amount_invoice - invoice_validation FAIL, exit 1, no payment
scenario_06_invoice_network_inconsistent_with_wallet - invoice_validation FAIL, exit 1
scenario_07_payer_has_insufficient_balance - pay_invoice FAIL, protected_resource BLOCKED, exit 1
scenario_08_wallet_returns_no_preimage - preimage_verification FAIL with NO_PREIMAGE, resource never unlocked
scenario_09_wallet_returns_a_wrong_preimage - preimage_verification FAIL with PREIMAGE_MISMATCH, wrong proof gets 402
scenario_10_ambiguous_payment_blocks_the_next_run - AMBIGUOUS guard file written, next run RUN_GUARD_BLOCKED, --clear-guard re-enables paying
scenario_11_live_without_acknowledgement - live pay BLOCKED with LIVE_NOT_ACKNOWLEDGED, exit 3
scenario_12_a_paying_state_file_blocks_the_run - stale PAYING file blocks, no payment attempted
scenario_13_the_report_records_the_negotiated_encryption_mode - nip44_v2 selected, applied and recorded in the report
scenario_14_nip04_only_wallet_records_nip04 - a payer that only advertises nip04 runs and records nip04
scenario_15_payer_and_receiver_keep_their_own_modes - payer nip44_v2 and receiver nip04 are each selected from their own info event
```

## The thirteen L402 scenarios

`crates/tally-l402/tests/fixture.rs` runs the fixture over real loopback sockets.

```
# test names
scenario_01_no_authorization_gives_a_bound_challenge - 402 with macaroon and invoice, identifier carries the payment hash
scenario_02_correct_credentials_give_the_resource - 200 and the expected body
scenario_03_wrong_preimage_gives_402 - tampered proof refused
scenario_04_preimage_from_another_challenge_gives_402 - crossed credentials refused
scenario_05_macaroon_only_gives_402 - no preimage, no access
scenario_06_tampered_macaroon_gives_402 - signature check fails
scenario_07_extra_caveat_gives_402 - unknown caveat fails closed
scenario_08_macaroon_from_another_fixture_gives_402 - another root key refused
scenario_09_malformed_authorization_gives_402_never_500 - malformed headers are 402, never 500
scenario_10_the_same_credentials_work_twice - credentials are reusable, not single-use
scenario_11_responses_never_contain_the_preimage_or_the_invoice - no sensitive value in any response
scenario_12_client_refuses_non_loopback_and_https - the client refuses before opening a socket
scenario_13_every_failure_has_the_same_response_shape - no oracle in the failure bodies
```

## The renderer tests

`crates/tally-runner/tests/renderer.rs` covers both renderers over one report value:

```
# test names
a_sentinel_in_diagnostics_is_escaped - a script tag in diagnostics is HTML-escaped
per_status_counts_in_html_equal_the_json - the HTML counts match the JSON
all_five_statuses_and_the_legend_are_present - the legend and all five statuses appear
the_html_has_no_script_and_no_external_resource - no script, iframe, or external URL
the_html_is_ascii_only - non-ASCII text is escaped to entities
json_and_html_come_from_the_same_value - both files render the same struct
the_table_has_a_caption_and_column_headers - the table is accessible
a_real_run_keeps_invoice_and_secrets_out_of_both_reports - a real run writes no invoice, preimage or connection string
evidence_sections_render_every_part - every evidence field is rendered
```

## The CLI tests

`apps/tally-cli/tests/cli.rs` runs the built binary offline; it never opens a relay connection.

```
# test names
version_prints_the_package_version - "tally 0.1.0"
doctor_without_configuration_exits_two_and_prints_no_values - exit 2, no value printed
doctor_reports_a_parsing_connection_string_without_printing_it - parses=false, sentinel absent from output
a_usage_error_exits_two - missing flag, unknown profile and unknown command all exit 2
a_missing_connection_string_exits_two_without_echoing_anything - exit 2 with a generic message
report_round_trips_json_to_html - a stored report re-renders to HTML
report_rejects_an_unknown_schema_version - exit 2 and no output file
a_connection_string_sentinel_never_reaches_any_output - no output contains the sentinel
dotenv_files_are_read_when_the_environment_is_empty - .env.payer is read when the environment is empty
```

## The server tests

`apps/tally-server/tests/api.rs` starts a real Axum router on `127.0.0.1:0` and talks to it over real sockets. Only the two wallets are fake; the fixture, the guard, the permit and the reports are the real code.

```
# test names
test_01_status_has_no_secret_and_reports_readiness - ready with two parsing wallets, not ready without them, no secret in the body
test_02_wrong_host_is_forbidden - a foreign Host header gets 403
test_03_post_without_the_custom_header_is_forbidden - 403 without X-Tally-Request
test_04_mismatched_origin_is_forbidden - 403 for a foreign Origin, 202 for the matching one
test_05_wrong_content_type_is_rejected - a non-JSON body gets a 4xx in the shared error shape
test_06_oversize_body_is_rejected - an 8 KiB body is refused and nothing is paid
test_07_wrong_confirmation_starts_nothing - a mismatched PAY line gets 400 and writes no run
test_08_unpaid_run_streams_ordered_events_and_serves_both_reports - 28 steps then done, exit 5, both reports served
test_09_paid_run_with_the_exact_confirmation_pays_once - one payment, exit 0
test_10_a_second_post_while_running_is_a_conflict - 409 while a run is in progress
test_11_bad_report_ids_never_read_a_file - unknown, non-hex and traversal ids all 404
test_12_unknown_route_and_method_share_the_error_shape - 404 and 405 in the shared JSON error
test_13_a_late_subscriber_gets_the_replay - the events route replays a finished run
test_14_an_ambiguous_guard_file_blocks_until_cleared - blocked, blocked again, then cleared
test_15_security_headers_on_dashboard_and_report - no-store, nosniff and both CSPs
test_16_the_bound_address_is_loopback - bind gives 127.0.0.1 and a real port
```

## Where the mocks are

Two things are mocked, and only in tests:

- The TEST ONLY invoice generator in `crates/tally-l402/src/test_support.rs`, behind the off-by-default `test-support` feature. It signs real BOLT11 invoices offline and is used by the 13 L402 scenarios and by the 12 runner scenarios through a dev-dependency. Every item's doc comment starts with TEST ONLY.
- The fake wallet pair in `crates/tally-runner/src/test_support.rs`, implementing `WalletApi` with configurable failure modes behind the off-by-default `test-support` feature. tally-runner's own tests reach it through a dev-dependency, and tally-server's tests enable the same feature; no runtime binary can. Only the wallets are faked: the fixture, the run guard, the permit, the preimage check and the renderers are the real code.

Nothing else is simulated. The unit tests use no network at all, and the CLI tests launch the real binary.

## Running the live profile

Live mode is implemented but was never exercised. It needs a real wallet with real funds:

```
cargo run -q -p tally-cli -- test --profile l402-e2e --env live --approve-payment-sats <n> --live-ack "I ACKNOWLEDGE LIVE FUNDS"
```

Without the acknowledgement string the payment is BLOCKED with `LIVE_NOT_ACKNOWLEDGED` before any request is sent (scenario 11 proves this offline). Do not run it without a wallet you are willing to spend from.

## The run guard

`crates/tally-runner/src/guard.rs` writes `runs/<payment hash>.state` with one of PAYING, PAID, FAILED or AMBIGUOUS. A PAYING or AMBIGUOUS file blocks every later run with `RUN_GUARD_BLOCKED` until you pass `--clear-guard`, which removes blocking files only (`guard::tests::clearing_removes_only_blocking_files`). An ambiguous payment is never retried automatically.

## Exit codes and priority

0 all required tests passed; 1 a required test failed; 2 a usage or configuration error before any test ran; 3 a required test was blocked; 4 a required test was unsupported; 5 a required test was not tested. With several non-PASS required tests the priority is FAIL, then UNSUPPORTED, then BLOCKED, then NOT TESTED (`report::tests::exit_code_priority_between_mixed_required_results`). Scenario 3 proves the combination: an UNSUPPORTED challenge with a BLOCKED payment exits 4.

## What a green run does not prove

- It does not prove any third-party L402 service works with Tally; the fixture is Tally's own.
- It does not prove a real Lightning payment routes; the faucet hub has no channels.
- It does not prove live mode works; no live run was made.
- It does not prove the MSVC toolchain links; only the gnu toolchain was built.
- It does not prove the GitHub workflow runs; it was never pushed.
- Unit tests do not cover the `nwc` or `nostr` crates themselves.
