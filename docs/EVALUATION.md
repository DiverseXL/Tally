# Evaluation guide

## What Tally is

A developer tool that tests whether a Lightning wallet connected through NWC can complete the workflow an app depends on, ending in a real L402 payment proof.

## Primary user

A developer or small team wiring Lightning payments into a product, starting with African Bitcoin developers building on NWC.

## The problem

A working NWC connection does not mean the wallet supports the methods the app calls or that an L402 proof step works. Those faults surface in production, in front of a paying user.

## The distinctive workflow

`tally test --profile l402-e2e` connects two different wallets, checks what each one advertises, fetches an L402 challenge from a local fixture, validates the invoice and its binding, pays once through a single-use permit, verifies SHA-256 of the preimage against the payment hash, unlocks the resource, tries two tampered proofs that must fail, looks the invoice up, and exits with a CI code.

## Where the evidence is

- Recorded approved run, exit 0: `examples/recorded-sandbox-run/approved/report.html` and `examples/recorded-sandbox-run/approved/report.json`.
- Recorded unapproved run, exit 5: `examples/recorded-sandbox-run/unapproved/report.html` and `examples/recorded-sandbox-run/unapproved/report.json`.
- Provenance and exact commands: `examples/recorded-sandbox-run/README.md`.
- Demo video: TO BE ADDED BEFORE SUBMISSION.

## How to run the safe profile

`cargo test --workspace` needs no wallets and no network. `tally doctor` and `tally inspect --profile basic` need the wallet files but spend nothing. `tally test --profile l402-e2e` without an approval leaves the payment tests NOT TESTED and exits 5.

## How to reproduce the live sandbox flow

Follow the quick start in `README.md` to create two faucet wallets, then run `cargo run -q -p tally-cli -- test --profile l402-e2e --approve-payment-sats 21`. It spends 21 sandbox sats once and exits 0.

## Where the code is

- NWC adapter: `crates/tally-nwc`, with the wallet trait in `crates/tally-nwc/src/wallet_api.rs`.
- L402 engine: `crates/tally-l402`, with the fixture in `crates/tally-l402/src/fixture.rs`.
- Orchestration and reports: `crates/tally-runner`.
- CLI: `apps/tally-cli/src/main.rs`.
- Tests: `crates/tally-runner/tests/scenarios.rs`, `crates/tally-l402/tests/fixture.rs`, `crates/tally-runner/tests/renderer.rs`, `apps/tally-cli/tests/cli.rs`.

## What is real, sandbox, local and mocked

Relay traffic and wallet calls are live; the payment is live protocol on sandbox funds inside one Alby Hub with no channels; the L402 resource is a local fixture; the invoice generator and the wallet pair are TEST ONLY and exist only in tests. See `docs/INTEGRATIONS.md`.

## What is not implemented

- The web dashboard (`web/`) and the local server (`apps/tally-server`) are scaffolds.
- Live mode (`--env live`) is implemented but never exercised.
- No third-party L402 service was tested against.
- CI is defined but was never run on GitHub.
- No wallet certification, no compatibility guarantee, no security audit.

## Criteria map

| Criterion | Implementation | Verification command | Evidence |
| --- | --- | --- | --- |
| Usefulness and Local Impact | Profiles for the NWC methods apps call plus the L402 proof step; one command gives a CI signal before release | `cargo run -q -p tally-cli -- test --profile l402-e2e` | `examples/recorded-sandbox-run/unapproved/report.html` (exit 5, no spend), `examples/recorded-sandbox-run/approved/report.html` (exit 0) |
| Security by Design | `SecretString` only for connection strings, single-use `PaymentPermit`, no-retry payments, live acknowledgement, run guard, loopback-only fixture, redacting types | `cargo test --workspace` then read `docs/SECURITY.md` | `crates/tally-nwc/src/permit.rs`, `crates/tally-runner/src/guard.rs`, `crates/tally-core/src/redact.rs`, tests named in `docs/SECURITY.md` |
| UX/UI Quality | Plain CLI summary in run order, HTML report with status counts and a legend, five-status taxonomy so a partial result reads correctly | `cargo run -q -p tally-cli -- report --input reports/latest/report.json --format html --out reports/latest/rerendered.html` | `examples/recorded-sandbox-run/approved/report.html`, `crates/tally-runner/src/render.rs` |
| Working MVP | 165 tests, one recorded 21-sat payment that unlocked the resource, a loopback-only local server with a dashboard, exit codes 0 to 5 all exercised by tests | `cargo test --workspace` | `examples/recorded-sandbox-run/README.md`, `docs/TESTING.md` |
| Clear Pitch and Docs | One-sentence proposition, honest limits, six documents, a docs checker, a recorded run | `bash scripts/check-docs.sh` | `README.md`, `docs/`, `examples/recorded-sandbox-run/` |

## Five-minute path

```
git clone <repository-url> Tally
cd Tally
cargo test --workspace
cargo run -q -p tally-cli -- --version
open examples/recorded-sandbox-run/approved/report.html
bash scripts/check-docs.sh
```

Then, if you want the live sandbox flow, follow the faucet quick start in `README.md`. Nothing hidden: every behaviour described here is what the code does, and the recorded reports are the executor's own output.
