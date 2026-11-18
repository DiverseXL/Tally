# Recorded sandbox runs

These are recorded real runs of the `tally` CLI on this Windows machine, not synthetic files. Nothing in this folder was edited after the run: the JSON and the HTML are the exact bytes written by `write_reports`.

## approved/

- Run id: `24d21e12954ec0d716f3766d4f4afb86`
- started_at `2026-11-18T18:37:23Z`, finished_at `2026-11-18T18:37:28Z`, duration_ms 4995
- Environment: `sandbox`, endpoint class `local_fixture`
- Exit code: 0 (all required tests passed)
- Command that produced it, run from the repository root in Git Bash:

```
cargo run -q -p tally-cli -- test --profile l402-e2e --approve-payment-sats 21
```

That command paid one invoice of 21000 msat (21 sats) exactly once. It was not retried. The report shows the payer balance delta -21000 msat and the receiver balance delta +21000 msat, `pay_invoice` PASS, `preimage_verification` PASS and `protected_resource` PASS with `valid_proof_status` 200 and `body_matches_expected` true.

## unapproved/

- Run id: `88dd082561220ed70982b8649a6e951f`
- started_at `2026-11-18T18:37:06Z`, finished_at `2026-11-18T18:37:10Z`, duration_ms 3838
- Environment: `sandbox`, endpoint class `local_fixture`
- Exit code: 5 (a required test was not tested: the payment tests)
- Command that produced it, run from the repository root in Git Bash:

```
cargo run -q -p tally-cli -- test --profile l402-e2e
```

No payment was approved, so `pay_invoice`, `preimage_verification` and `protected_resource` are NOT TESTED with reason `PAYMENT_NOT_APPROVED`. The free checks (`connect`, `info_event`, `get_info`, `get_balance`, `challenge`, `invoice_validation`, `challenge_binding`, the two negative challenge tests and `lookup_invoice`) still ran and passed against the real wallets.

## What these runs prove, and what they do not

Both runs carry these two limitation sentences, stored in the report itself:

- "Sandbox wallets from the NWC faucet: balances are play money on a single Alby Hub with no Lightning channels; payments use the real NWC protocol and return a real preimage but are not routed across the real Lightning network."
- "The protected resource is served by a local test fixture started by Tally on 127.0.0.1; this proves Tally's flow, not compatibility with third-party L402 services."

The NWC traffic, the wallet calls and the payment are real: two different wallets on the `faucet.nwc.dev` hub, reached over the Nostr relays named in their connection strings, with a real NIP-47 `pay_invoice` request and a real preimage returned. The invoice, the payment hash check and the HTTP 402 exchange are real. The amounts are play money and the L402 server is Tally's own loopback fixture.

The service pubkeys in the JSON belong to disposable faucet wallets. A service pubkey is a public identifier, not a secret, but it identifies nothing beyond those throwaway wallets.

The hub reports `network_reported` as `mainnet`. The faucet invoices are labelled as Bitcoin mainnet invoices even though the hub holds no channels and routes nothing, so an invoice prefix is not a safety signal on this hub.

## What is deliberately not stored

No connection string, no BOLT11 invoice, no preimage and no macaroon appears in either file. The report model in `crates/tally-core/src/report.rs` has no field for any of them: the payment evidence records only whether a preimage was present and whether its SHA-256 matched the invoice payment hash. The secret scan described in `docs/SECURITY.md` checks these files too.

## Reproducing

The quick start in the top-level `README.md` creates the two wallet files, then runs `tally doctor`, `tally inspect --profile basic` and `tally test --profile l402-e2e` with and without an approval. Run ids and timestamps will differ; the statuses and the exit codes should not.
