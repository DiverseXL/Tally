# Security

Tally is a developer tool, not a wallet and not an audit. This page states what is implemented, where it is tested, and what remains open.

## Assets

1. NWC connection strings. Anyone holding one can spend from the wallet it points at, within that wallet's budget.
2. Invoice payment preimages. A preimage proves a payment and unlocks an L402 resource.
3. Macaroon root keys and minted macaroons. They authorise access to the fixture, and in other systems can authorise spending.
4. The two wallet funds balances the tests read and move.
5. Reports. They are meant to be shareable, so they must not carry any of the above.

## Trust boundaries

| Boundary | What crosses it | Control |
| --- | --- | --- |
| Disk and process | `.env.payer`, `.env.receiver` | gitignored, read into `SecretString`, never printed |
| Process and Nostr relays | NIP-47 requests and responses | no secret in logs, no retry, connection string never serialised |
| Process and Alby Hub | wallet calls including one payment | single-use permit, approval amount, environment class, live acknowledgement, run guard |
| Process and L402 fixture | HTTP on loopback only | client refuses non-loopback and https |
| Process and report files | one report value | model has no field for a secret, and the HTML is escaped |

## Controls implemented

| Control | Where | Test |
| --- | --- | --- |
| Connection strings are held only in `secrecy::SecretString` | `crates/tally-nwc/src/session.rs` | `session::tests::session_debug_has_no_secret` |
| The `nostr` URI type's `Display` prints the secret, so it is never stringified | rule in `AGENTS.md`; enforced by review of `crates/tally-nwc/src/session.rs` | no test: this is a code-review finding, not a runtime property |
| Errors never echo the input that failed to parse | `crates/tally-nwc/src/connection.rs`, `crates/tally-l402/src/challenge.rs`, `crates/tally-l402/src/invoice.rs`, `crates/tally-nwc/src/permit.rs` | `connection::tests::errors_do_not_contain_content_or_value`, `challenge::tests::errors_never_echo_the_input`, `invoice::tests::unparseable_input_is_refused_without_echoing_it`, `permit::tests::refuses_unparseable_input_without_echoing_it` |
| The CLI prints no value from a connection string | `apps/tally-cli/src/main.rs` | `a_connection_string_sentinel_never_reaches_any_output` |
| Invoices, preimages and macaroons redact `Debug` and `Display` | `crates/tally-nwc/src/session.rs`, `crates/tally-l402/src/challenge.rs`, `crates/tally-l402/src/macaroon.rs` | `session::tests::invoice_debug_never_reveals_the_invoice`, `session::tests::paid_invoice_debug_never_reveals_the_preimage`, `challenge::tests::debug_and_display_redact_macaroon_and_invoice`, `macaroon::tests::debug_and_display_redact_the_root_key` |
| A scrubber removes known secrets from any string that is about to be logged | `crates/tally-core/src/redact.rs` | `redact::tests::scrub_removes_sentinel_and_detects_it`, `redact::tests::scrub_redacts_every_occurrence` |
| The report model has no field for a connection string, invoice, preimage or macaroon | `crates/tally-core/src/report.rs` | `a_real_run_keeps_invoice_and_secrets_out_of_both_reports`, `scenario_11_responses_never_contain_the_preimage_or_the_invoice` |
| One payment per permit; the permit binds invoice, amount, payment hash and environment and is consumed | `crates/tally-nwc/src/permit.rs` | `permit::tests::issues_for_a_matching_unexpired_sandbox_invoice`, `permit::tests::refuses_a_different_amount`, `permit::tests::refuses_an_expired_invoice`; the struct derives neither `Clone` nor `Copy` |
| A payment is never retried, and an error with no wallet code is BLOCKED | `crates/tally-nwc/src/session.rs`, `crates/tally-core/src/outcome.rs` | `outcome::tests::every_wallet_error_row_of_the_table`; scenario 10 proves the ambiguous case |
| Live payments need `I ACKNOWLEDGE LIVE FUNDS` | `crates/tally-nwc/src/permit.rs` | `permit::tests::live_needs_the_acknowledgement_string`, `scenario_11_live_without_acknowledgement` |
| A PAYING or AMBIGUOUS run blocks every later run | `crates/tally-runner/src/guard.rs` | `guard::tests::paying_blocks_and_paid_does_not`, `scenario_10_ambiguous_payment_blocks_the_next_run`, `scenario_12_a_paying_state_file_blocks_the_run` |
| Guard files hold a state token, not an invoice or a secret | `crates/tally-runner/src/guard.rs` | `guard::tests::state_files_hold_no_secret_and_no_invoice_text` |
| The L402 client refuses https and non-loopback hosts before opening a socket | `crates/tally-l402/src/client.rs` | `client::tests::refuses_https_and_non_loopback_urls`, `scenario_12_client_refuses_non_loopback_and_https` |
| The fixture binds 127.0.0.1 port 0 and serves one path | `crates/tally-l402/src/fixture.rs` | `scenario_01_no_authorization_gives_a_bound_challenge` |
| Every failure returns the same 402 shape, so the response is not an oracle | `crates/tally-l402/src/fixture.rs` | `scenario_13_every_failure_has_the_same_response_shape` |
| Challenge and credential size caps (8192 bytes per header value, 4096 per macaroon) with control-character rejection | `crates/tally-l402/src/challenge.rs` | `challenge::tests::challenge_rejects_control_characters_and_oversized_values`, `challenge::tests::authorization_rejects_control_characters_and_oversized_values` |
| The HTML report escapes text and loads nothing external | `crates/tally-runner/src/render.rs` | `a_sentinel_in_diagnostics_is_escaped`, `the_html_has_no_script_and_no_external_resource` |
| `reqwest` is built without TLS features, so plain HTTP to loopback is all it can do | `crates/tally-l402/Cargo.toml` | no test: it is a manifest fact |

## Open risks

- `.env.payer` and `.env.receiver` are plaintext files on disk. They are gitignored, but anyone with filesystem access to them can spend from those wallets.
- If a connection string is typed inline in a shell command, it lands in shell history. The CLI provides no inline form, but nothing stops a user from writing one into a file by hand.
- `libmacaroon` is not independently audited as far as we know. Macaroons are used only inside the local fixture, and the flagship check is the SHA-256 preimage comparison.
- The faucet is a third-party service. Tally depends on it being reachable to create test wallets, and on its hub behaving as documented.
- Sandbox invoices carry the Bitcoin mainnet prefix even though the hub has no channels. Anyone reading a report could mistake that for a mainnet payment; the report's limitation text says otherwise, and `invoice_validation` compares the invoice network against the wallet's own answer.
- Live mode is implemented but was never exercised. Its behaviour against a real wallet is unverified.
- Only the Windows gnu toolchain was built and tested. MSVC and non-Windows platforms are unverified.
- No formal audit, penetration test or threat model review has been done.
- The four cargo dependencies that handle cryptography or money (`nwc`, `nostr`, `lightning-invoice`, `libmacaroon`) are trusted as published; Tally does not pin checksums beyond `Cargo.lock`.

## Known advisories

To be filled from the dependency audit run in this step, with the date and the database state.
