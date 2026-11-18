# Tally

Tally is a developer tool that tests whether a Bitcoin/Lightning wallet connected through Nostr Wallet Connect (NWC, NIP-47) can complete the workflows an app depends on. Its flagship proof is a real NWC wallet payment unlocking an L402-protected resource, where the returned preimage is checked against the invoice payment hash.

Status: the runner and the CLI are proven by tests and by recorded live runs; the server and the web dashboard are still scaffolds.

Commands that work today (run from this folder):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo run -q -p tally-cli -- --version
cargo run -q -p tally-server -- --version
```

The CLI needs two connection strings, one for the paying wallet and one for the receiving wallet. It reads them from `TALLY_PAYER_NWC_URI` and `TALLY_RECEIVER_NWC_URI`, or from `.env.payer` and `.env.receiver` in the working folder; the environment wins. No flag accepts a connection string, and no command prints one.

```
cargo run -q -p tally-cli -- doctor
cargo run -q -p tally-cli -- inspect --profile basic
cargo run -q -p tally-cli -- test --profile l402-e2e
cargo run -q -p tally-cli -- test --profile l402-e2e --approve-payment-sats 21
cargo run -q -p tally-cli -- report --input reports/latest/report.json --format html --out reports/latest/rerendered.html
```

Without `--approve-payment-sats` the payment tests stay NOT TESTED and the run exits 5, so the free checks never spend. With an approval the run pays that many sats once, with no retry, and writes a JSON report and an HTML report to `reports/<run id>/`; `reports/latest/` always points at the newest run. A live run additionally needs `--env live --live-ack "I ACKNOWLEDGE LIVE FUNDS"`. Stale `PAYING` or `AMBIGUOUS` guard files in `runs/` block every run until you pass `--clear-guard`.
