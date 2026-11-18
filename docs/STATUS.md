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
| tally-core capabilities | crates/tally-core/src/capabilities.rs | VERIFIED | cargo test --workspace: capabilities::tests 7 passed (whitespace split, empty-content fallback, no source, discrepancies, JSON-like content, verbatim names, content kinds) |
| tally-core report and exit codes | crates/tally-core/src/report.rs | VERIFIED | cargo test --workspace: report::tests 4 passed (exit 0 only when all required pass, one per required status, priority, JSON snapshot of schema 1) |
| tally-core time helpers | crates/tally-core/src/time.rs | VERIFIED | cargo test --workspace: time::tests 3 passed (known_timestamps, negative_times_are_clamped_to_the_epoch, run_id_is_hex_and_differs_between_calls) |
| tally-core redact | crates/tally-core/src/redact.rs | VERIFIED | cargo test --workspace: redact::tests 4 passed (scrub, detect, ignore secrets under 8 chars, Debug hides secrets) |
| tally-nwc | crates/tally-nwc | IMPLEMENTED | connection, guard, error, permit and session modules; nothing from nwc or nostr leaves the crate |
| tally-nwc connection | crates/tally-nwc/src/connection.rs | VERIFIED | cargo test --workspace: connection::tests 4 passed; also read the real .env.payer and .env.receiver in the Step 4 and Step 5 runs |
| tally-nwc guard | crates/tally-nwc/src/guard.rs | VERIFIED | cargo test --workspace: guard::tests 5 passed (match, wrong amount, sub-satoshi amount, expired invoice, missing approval) |
| tally-nwc error mapping | crates/tally-nwc/src/error.rs | VERIFIED | cargo test --workspace: error::tests 3 passed (all ten code names read from a message, no input echoed, shared table used) |
| tally-nwc PaymentPermit | crates/tally-nwc/src/permit.rs | VERIFIED | cargo test --workspace: permit::tests 7 passed (issue, no approval, amount mismatch, expired, live acknowledgement, unparseable without echo, Debug redaction) |
| tally-nwc WalletSession | crates/tally-nwc/src/session.rs | VERIFIED | cargo test --workspace: session::tests 8 passed (pubkey and hosts only, Debug safe, invalid string refused, sha256 preimage check, invoice and preimage Debug redaction, lookup Debug, host parsing) |
| Wallet proof example | crates/tally-nwc/examples/wallet_proof.rs | VERIFIED | Step 5 runs: read-only run exit_code=5 with pay_invoice NOT_TESTED; approved run exit_code=0, pay_invoice PASS with preimage_present=true and preimage_matches_payment_hash=true, balance delta payer -21000 msat and receiver +21000 msat, lookup_invoice state=settled, output_contains_secret=false |
| NIP-47 get_info extensions | crates/tally-nwc/src/session.rs | IMPLEMENTED | nostr 0.45.7 `GetInfoResponse` has no `extensions` field (read in ~/.cargo/registry/src/.../nostr-0.45.7/src/nips/nip47.rs), so the adapter always reports an empty `get_info` extension list; the info-event `extensions` tag is the only extension source. Reported as a specification contradiction. |
| tally-l402 | crates/tally-l402 | SCAFFOLD | none |
| tally-runner | crates/tally-runner | SCAFFOLD | none |
| tally-cli | apps/tally-cli | SCAFFOLD | none |
| tally-server | apps/tally-server | SCAFFOLD | none |
| web | web | SCAFFOLD | none |
| Toolchain and library build test (scratch, deleted) | deleted scratch crate | VERIFIED | Step 2 report: pinned libraries built together and passed offline probes on x86_64-pc-windows-gnu |
