# Status

Legend:

- SCAFFOLD: structure only, no behaviour.
- IMPLEMENTED: code exists, not yet proven.
- VERIFIED: proven by a test or recorded run, with the evidence named.

| Component | Path | Status | Evidence |
| --- | --- | --- | --- |
| tally-core | crates/tally-core | IMPLEMENTED | redact module only; result model, status taxonomy and exit codes not written |
| tally-core redact | crates/tally-core/src/redact.rs | VERIFIED | cargo test --workspace: redact::tests 4 passed (scrub, detect, ignore secrets under 8 chars, Debug hides secrets) |
| tally-nwc | crates/tally-nwc | IMPLEMENTED | connection and guard modules; NWC client wrapper not written |
| tally-nwc connection | crates/tally-nwc/src/connection.rs | VERIFIED | cargo test --workspace: connection::tests 4 passed; also read the real .env.payer and .env.receiver in the Step 4 runs |
| tally-nwc guard | crates/tally-nwc/src/guard.rs | VERIFIED | cargo test --workspace: guard::tests 5 passed (match, wrong amount, sub-satoshi amount, expired invoice, missing approval) |
| Wallet proof example | crates/tally-nwc/examples/wallet_proof.rs | VERIFIED | Step 4 approved run: preimage_matches_payment_hash=true, payer_balance_delta_sats=-21, receiver_balance_delta_sats=21, lookup_invoice state=Settled, output_contains_secret=false |
| tally-l402 | crates/tally-l402 | SCAFFOLD | none |
| tally-runner | crates/tally-runner | SCAFFOLD | none |
| tally-cli | apps/tally-cli | SCAFFOLD | none |
| tally-server | apps/tally-server | SCAFFOLD | none |
| web | web | SCAFFOLD | none |
| Toolchain and library build test (scratch, deleted) | deleted scratch crate | VERIFIED | Step 2 report: pinned libraries built together and passed offline probes on x86_64-pc-windows-gnu |
