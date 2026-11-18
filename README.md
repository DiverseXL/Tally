# Tally

Know whether your Lightning wallet integration actually works before your users discover that it does not.

Tally is a developer tool that tests a Bitcoin/Lightning wallet connected through Nostr Wallet Connect (NWC, NIP-47) against the workflows an app depends on. Its flagship proof is a real NWC wallet payment that unlocks an L402-protected resource, where the returned preimage is verified against the invoice payment hash.

Status: working MVP. The runner and the CLI are proven by 145 tests and by recorded live runs; the local server and the web dashboard are not implemented yet.

## The problem

A wallet that connects over NWC has not thereby proven that it supports the methods an app calls, that it can issue and settle payments the way the app expects, or that an L402 payment-proof step works end to end. Those failures show up in production, in front of a paying user.

## Who it is for

Developers and small teams integrating Lightning payments: merchant tools, savings products, paid APIs, wallet companions and agent-enabled services. The first audience is African Bitcoin developers building on NWC.

## Impact

Tally does not process consumer payments. It raises the reliability of products that depend on Lightning wallets by finding integration faults before release, which lowers the chance that a customer payment fails. Demand and monetisation are unproven hypotheses; there are no customer interviews.

Tally is not a wallet, not a security audit, not a wallet certification, and not a promise of universal compatibility. The recorded payment is a sandbox payment and the protected resource is a local test fixture.

## What it does

Two profiles run from one binary, `tally`:

- `basic` runs seven wallet-only tests: `connect`, `info_event`, `get_info`, `get_balance`, `make_invoice`, `lookup_invoice`, `sign_message`.
- `l402-e2e` runs fourteen tests that end in an unlocked resource.

The `l402-e2e` flow, with the statuses recorded in `examples/recorded-sandbox-run/approved/report.json`:

| Test | What it shows | Recorded status |
| --- | --- | --- |
| connect | Both wallets reach their relays; the payer connected to 2 relays and the receiver to 2 | PASS |
| info_event | The kind 13194 info event is fetched and its content is space-separated method names | PASS |
| get_info | Both wallets answer `get_info`; each lists 13 methods | PASS |
| get_balance | Both balances are read in millisatoshis before paying | PASS |
| challenge | The fixture answers 402 with a macaroon and an invoice | PASS |
| invoice_validation | The invoice amount is 21000 msat, it is unexpired and its network matches the wallet | PASS |
| challenge_binding | The macaroon identifier carries the invoice payment hash | PASS |
| pay_invoice | One payment through a single-use permit: amount 21000 msat, fee 0, payer -21000, receiver +21000 | PASS |
| preimage_verification | SHA-256 of the returned preimage equals the invoice payment hash | PASS |
| protected_resource | The L402 credentials unlock the resource: status 200, expected body | PASS |
| negative_wrong_proof | A tampered proof is rejected with 402 | PASS |
| negative_no_proof | A macaroon without a preimage is rejected with 402 | PASS |
| lookup_invoice | The wallet reports the invoice as settled | PASS |
| sign_message | The wallet does not advertise the method, so no request is sent | UNSUPPORTED |

`sign_message` is a capability probe and is not required: advertised means NOT TESTED, because the `nwc` crate has no sign_message API; not advertised means UNSUPPORTED.

## The recorded proof

Two real runs are committed, unedited, under `examples/recorded-sandbox-run/`:

- `examples/recorded-sandbox-run/approved/report.html` and `report.json`: run `24d21e12954ec0d716f3766d4f4afb86`, started `2026-11-18T18:37:23Z`, exit code 0, all required tests PASS, one 21-sat payment.
- `examples/recorded-sandbox-run/unapproved/report.html` and `report.json`: run `88dd082561220ed70982b8649a6e951f`, started `2026-11-18T18:37:06Z`, exit code 5, the payment tests NOT TESTED because no approval was given.

Both carry the limitation text stored in the report: the wallets are sandbox wallets on a single Alby Hub with play money and no Lightning channels, and the protected resource is Tally's own loopback fixture. NWC traffic, wallet calls and the payment are real; the money is not. Provenance is in `examples/recorded-sandbox-run/README.md`.

## Quick start (Git Bash)

Prerequisites: Rust stable (tested on `stable-x86_64-pc-windows-gnu` with MinGW-w64; the MSVC toolchain was not verified), `git`, and `curl`.

```
git clone <repository-url> Tally
cd Tally
cargo test --workspace
```

Create two disposable faucet wallets. Each command writes a connection string into a file; no command prints one.

```
printf 'TALLY_PAYER_NWC_URI=%s\n' "$(curl -s -X POST 'https://faucet.nwc.dev?balance=10000')" > .env.payer
printf 'TALLY_RECEIVER_NWC_URI=%s\n' "$(curl -s -X POST 'https://faucet.nwc.dev?balance=10000')" > .env.receiver
```

```
cargo run -q -p tally-cli -- doctor
cargo run -q -p tally-cli -- inspect --profile basic
cargo run -q -p tally-cli -- test --profile l402-e2e
cargo run -q -p tally-cli -- test --profile l402-e2e --approve-payment-sats 21
cargo run -q -p tally-cli -- report --input reports/latest/report.json --format html --out reports/latest/rerendered.html
```

The first `test` spends nothing and exits 5 (a required test was not tested). The second pays 21 sats once, without retry, and exits 0. Reports land in `reports/<run id>/`; `reports/latest/` always holds the newest run.

## Quick start (Windows PowerShell 5.1)

PowerShell 5.1 has no `&&`; run one command per line, from the repository root.

```
cargo test --workspace

$payer = Invoke-RestMethod -Method Post -Uri 'https://faucet.nwc.dev?balance=10000'
$receiver = Invoke-RestMethod -Method Post -Uri 'https://faucet.nwc.dev?balance=10000'
"TALLY_PAYER_NWC_URI=$payer" | Set-Content -Encoding ascii .env.payer
"TALLY_RECEIVER_NWC_URI=$receiver" | Set-Content -Encoding ascii .env.receiver

cargo run -q -p tally-cli -- doctor
cargo run -q -p tally-cli -- inspect --profile basic
cargo run -q -p tally-cli -- test --profile l402-e2e
cargo run -q -p tally-cli -- test --profile l402-e2e --approve-payment-sats 21
```

## Safety

- No flag accepts a connection string. The CLI reads `TALLY_PAYER_NWC_URI` and `TALLY_RECEIVER_NWC_URI` from the environment, or from `.env.payer` and `.env.receiver`; the environment wins. `--payer-file` and `--receiver-file` point at other files.
- Errors never echo the input that failed to parse, and no command prints a secret.
- A payment needs both `--approve-payment-sats <n>` and a permit bound to that invoice, that amount, that payment hash and that environment. A permit cannot be reused and a payment is never retried.
- A live payment additionally needs `--env live --live-ack "I ACKNOWLEDGE LIVE FUNDS"`.
- Reports have no field for a connection string, an invoice or a preimage, and an ambiguous payment leaves a guard file in `runs/` that blocks later runs until `--clear-guard`.
- The faucet wallets can only move play money inside the faucet hub.

## Known limits

- The recorded run is a sandbox run: play money on one Alby Hub, no Lightning channels, no routing over the real network.
- The L402 server is a local fixture on 127.0.0.1 that Tally starts itself; compatibility with third-party L402 services is not tested.
- Live mode is implemented (`--env live` plus the acknowledgement) but was never exercised; no real funds were ever spent.
- The web dashboard and the local server are scaffolds, not features.
- CI is defined in `.github/workflows/ci.yml` but has not been run on GitHub yet.
- Windows with the gnu toolchain is the only verified platform.

## Documentation

- `docs/ARCHITECTURE.md`: crates, run flow, NWC and L402 lifecycles, trust boundaries.
- `docs/INTEGRATIONS.md`: the NIP-47, L402, faucet and macaroon facts Tally relies on.
- `docs/TESTING.md`: test layers, counts, scenarios and what a green run does not prove.
- `docs/SECURITY.md`: implemented controls, open risks, known advisories.
- `docs/EVALUATION.md`: judge-facing guide, including the criteria map.
- `docs/STATUS.md`: what is proven and what is still a scaffold.

## License

MIT. See `LICENSE`.

Demo video: TO BE ADDED BEFORE SUBMISSION
