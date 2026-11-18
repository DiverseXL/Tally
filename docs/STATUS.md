# Status

Legend:

- SCAFFOLD: structure only, no behaviour.
- IMPLEMENTED: code exists, not yet proven.
- VERIFIED: proven by a test or recorded run, with the evidence named.

| Component | Path | Status | Evidence |
| --- | --- | --- | --- |
| tally-core | crates/tally-core | IMPLEMENTED | status, outcome, capabilities, report, time and redact modules written; no network and no wallet SDK types |
| tally-core status taxonomy | crates/tally-core/src/status.rs | VERIFIED | cargo test --workspace: status::tests 2 passed (serializes_with_exact_names, display_uses_space_for_not_tested) |
| tally-core outcome table | crates/tally-core/src/outcome.rs | VERIFIED | cargo test --workspace: outcome::tests 3 passed (every wallet error row, every remaining row, wire names) |
| tally-core capabilities | crates/tally-core/src/capabilities.rs | VERIFIED | cargo test --workspace: capabilities::tests 11 passed (whitespace split, empty-content fallback, no source, discrepancies, JSON-like content, verbatim names, content kinds, notifications flag yes/yes, yes/no, no/yes, absent get_info extensions) |
| tally-core report and exit codes | crates/tally-core/src/report.rs | VERIFIED | cargo test --workspace: report::tests 4 passed (exit 0 only when all required pass, one per required status, priority, JSON snapshot of schema 1 with the challenge and protected-resource evidence) |
| tally-core time helpers | crates/tally-core/src/time.rs | VERIFIED | cargo test --workspace: time::tests 3 passed (known_timestamps, negative_times_are_clamped_to_the_epoch, run_id_is_hex_and_differs_between_calls) |
| tally-core redact | crates/tally-core/src/redact.rs | VERIFIED | cargo test --workspace: redact::tests 4 passed (scrub, detect, ignore secrets under 8 chars, Debug hides secrets) |
| tally-nwc | crates/tally-nwc | IMPLEMENTED | connection, guard, error, permit and session modules; nothing from nwc or nostr leaves the crate |
| tally-nwc connection | crates/tally-nwc/src/connection.rs | VERIFIED | cargo test --workspace: connection::tests 4 passed; also read the real .env.payer and .env.receiver in the Step 4, Step 5 and Step 6 runs |
| tally-nwc guard | crates/tally-nwc/src/guard.rs | VERIFIED | cargo test --workspace: guard::tests 5 passed (match, wrong amount, sub-satoshi amount, expired invoice, missing approval) |
| tally-nwc error mapping | crates/tally-nwc/src/error.rs | VERIFIED | cargo test --workspace: error::tests 3 passed (all ten code names read from a message, no input echoed, shared table used) |
| tally-nwc PaymentPermit | crates/tally-nwc/src/permit.rs | VERIFIED | cargo test --workspace: permit::tests 7 passed (issue, no approval, amount mismatch, expired, live acknowledgement, unparseable without echo, Debug redaction); the struct does not derive Clone or Copy |
| tally-nwc WalletSession | crates/tally-nwc/src/session.rs | VERIFIED | cargo test --workspace: session::tests 9 passed (pubkey and hosts only, Debug safe, invalid string refused, sha256 preimage check, invoice and preimage Debug redaction, lookup Debug, host parsing, encryption mode from the info event) |
| Wallet proof example | crates/tally-nwc/examples/wallet_proof.rs | VERIFIED | Step 5 runs: read-only run exit_code=5 with pay_invoice NOT_TESTED; approved run exit_code=0, pay_invoice PASS with preimage_present=true and preimage_matches_payment_hash=true, balance delta payer -21000 msat and receiver +21000 msat, lookup_invoice state=settled, output_contains_secret=false. Not re-run in Step 6. |
| NIP-47 get_info extensions | crates/tally-nwc/src/session.rs | VERIFIED | nostr 0.45.7 `GetInfoResponse` has no `extensions` field (read in ~/.cargo/registry/src/.../nostr-0.45.7/src/nips/nip47.rs), so the adapter reports `get_info` extensions as `None` (not exposed), never as an empty list; extension ids come from the info-event `extensions` tag only. Reported as a specification contradiction. |
| tally-l402 | crates/tally-l402 | IMPLEMENTED | invoice, challenge, macaroon, fixture and client modules written; the crate holds no payment code |
| tally-l402 invoice | crates/tally-l402/src/invoice.rs | VERIFIED | cargo test --workspace: invoice::tests 7 passed (network, amount and hash facts, amount absent, wallet network names, exact amount, expiry, unparseable without echo, combined check) |
| tally-l402 challenge | crates/tally-l402/src/challenge.rs | VERIFIED | cargo test --workspace: challenge::tests 13 passed (specification examples, case-insensitive scheme, round trip, missing/duplicate/unquoted parameters, control characters, oversized value, colon count, bad preimage, bad macaroon, no input echoed, redacted Debug and Display) |
| tally-l402 macaroon | crates/tally-l402/src/macaroon.rs | VERIFIED | cargo test --workspace: macaroon::tests 12 passed (mint, Tally identifier layout, matching and wrong preimage, other root key, tampered byte, unknown and absent caveats, bad encoding, bad identifier, redaction, random user ids); API read in libmacaroon-0.4.0/src/lib.rs, caveat.rs, verifier.rs and error.rs |
| tally-l402 fixture | crates/tally-l402/src/fixture.rs | VERIFIED | tests/fixture.rs 13 scenarios pass over real loopback: binds 127.0.0.1 port 0 only, GET /protected only, identical 402 shape for every failure |
| tally-l402 client | crates/tally-l402/src/client.rs | VERIFIED | cargo test --workspace: client::tests 2 passed (refuses https and non-loopback, builds with timeouts); integration scenario 12 proves the refusals before any socket is opened |
| tally-l402 integration tests | crates/tally-l402/tests/fixture.rs | VERIFIED | cargo test --workspace: 13 passed (no credentials, correct credentials, wrong preimage, crossed challenge, macaroon only, tampered macaroon, extra caveat, other fixture, malformed headers, reuse, no secret in responses, URL refusal, identical failure shape) |
| TEST ONLY invoice source | crates/tally-l402/tests/common/mod.rs | VERIFIED | compiled only for the integration test binary; every item's doc comment starts with TEST ONLY; 13 scenarios use it; no CLI, server or example can reach it |
| L402 live no-spend check | crates/tally-l402/examples/live_challenge_check.rs | VERIFIED | Step 6 run exit 0: receiver from .env.receiver only, http_status=402, invoice_network=bitcoin, amount_msat=21000, identifier_payment_hash_matches_invoice=true, invoice_network_consistent_with_wallet=true, wrong_proof_status=402, output_contains_secret=false; no payment made |
| tally-runner | crates/tally-runner | SCAFFOLD | none |
| tally-cli | apps/tally-cli | SCAFFOLD | none |
| tally-server | apps/tally-server | SCAFFOLD | none |
| web | web | SCAFFOLD | none |
| Toolchain and library build test (scratch, deleted) | deleted scratch crate | VERIFIED | Step 2 report: pinned libraries built together and passed offline probes on x86_64-pc-windows-gnu |
