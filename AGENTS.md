# Tally - rules for coding agents (read fully at the start of every session)

## What Tally is

Tally is an open-source developer tool (Dakar Bitcoin Hack 2026 entry) that tests whether a Bitcoin/Lightning wallet connected through Nostr Wallet Connect (NWC, NIP-47) can complete the workflows an app depends on. Flagship proof: a real NWC wallet payment unlocks an L402-protected resource, and the returned preimage is verified (SHA-256 of the preimage equals the invoice payment hash). The product name is Tally. RelayBench is an obsolete earlier name.

## Fixed decisions

- Rust-first Cargo workspace. Crates: tally-core, tally-nwc, tally-l402, tally-runner. Apps: tally-cli (binary `tally`) and tally-server (local Axum server). web/ is the dashboard in plain HTML, CSS and TypeScript: presentation only, no protocol logic. Never build the backend in Node.js.
- Intended dependency direction once code exists: apps -> tally-runner -> tally-l402 and tally-nwc -> tally-core. Add a dependency only when code needs it.
- Test results use exactly five statuses: PASS, FAIL, UNSUPPORTED, NOT TESTED, BLOCKED. One result model feeds both the JSON and the HTML report.
- The paying wallet and the receiving (invoice-issuing) wallet must be two different wallets.
- Macaroons: use the `libmacaroon` crate (pure Rust). Do not use the older `macaroon` crate (it pulls a C crypto library). Neither is independently audited as far as we know, so macaroons are used ONLY by the local test fixture. The real proof check is the preimage-hash check, done with sha2.
- reqwest: add it with default features off (plain HTTP to the local fixture). Do not enable TLS unless asked.
- Toolchain on this machine: Rust stable, host triple x86_64-pc-windows-gnu, with WinLibs MinGW gcc. rustup reports its default host as msvc, so a rust-toolchain.toml with a bare version would pick the msvc toolchain, which does not link here. Do not add a rust-toolchain.toml, a rustup override, or change toolchains without asking.
- Library versions proven to build and run together on this machine (Step 2 spike): nwc 0.45.2, nostr 0.45.7, lightning-invoice 0.34.2, libmacaroon 0.4.0, axum 0.8.9, clap 4.6.7, reqwest 0.13.5, tokio 1.54.1, serde 1.0.229, serde_json 1.0.154, sha2 0.11.1, subtle 2.6.1, secrecy 0.10.3, zeroize 1.9.1, tracing 0.1.44, tracing-subscriber 0.3.23.
- NIP-47 facts as implemented: the kind 13194 info event content is PLAIN TEXT, space-separated method names and is never parsed as JSON; the `encryption` and `extensions` tags carry space-separated tokens; `get_info` returns alias, color, pubkey, network, block height, block hash, methods and notifications; every amount and fee is in millisatoshis; wallet error codes are the NIP-47 set (PAYMENT_FAILED and NOT_FOUND among them) and lookup states are pending, settled, accepted, expired and failed. nostr 0.45.7 has no `extensions` field on `GetInfoResponse`, so the adapter reports `get_info` extensions as absent (None), never as an empty list; the info-event `extensions` tag is the only extension source for now. The info event's `notifications` content token is compared with `get_info`'s notifications list being non-empty, not with its methods.
- Capability policy: the info-event content is kept raw and split on whitespace; a token counts as a method name only if it is lowercase ASCII letters, digits and underscores; the info event governs when it has at least one such token, otherwise the run falls back to `get_info` and says so; both sources and every method that only one of them lists are reported.
- Status mapping and exit codes: one table in tally-core maps every observation to PASS, FAIL, UNSUPPORTED, NOT TESTED or BLOCKED, and nothing else decides a status. Exit codes: 0 all required tests passed, 1 a required test failed, 2 usage or configuration error before any test ran, 3 a required test was blocked, 4 a required test was unsupported, 5 a required test was not tested.
- Payment safety: `pay_invoice` accepts only a `PaymentPermit`, which binds one invoice, its amount, its payment hash and the environment, and is consumed by the payment; a payment is never retried; a `pay_invoice` error that carries no wallet error code is BLOCKED, not FAIL; a live payment additionally needs the acknowledgement string `I ACKNOWLEDGE LIVE FUNDS`.
- Runner: `crates/tally-runner` orchestrates one run over `WalletApi` and returns a single `RunReport`; both renderers (`to_json`, `to_html`) take that one value, so the two files cannot disagree. `write_reports` also refreshes `reports/latest/`. The run guard in `crates/tally-runner/src/guard.rs` writes `runs/<payment-hash>.state` and holds PAYING, PAID, FAILED or AMBIGUOUS; a PAYING or AMBIGUOUS file blocks every later run until `--clear-guard`, so an ambiguous payment is never retried. `run.rs` never retries a payment.
- Exit code priority when several required tests are not PASS: FAIL (1), then UNSUPPORTED (4), then BLOCKED (3), then NOT TESTED (5). An unsupported required test must not be masked by downstream blocked tests. Code in `crates/tally-core/src/report.rs`, its test `report::tests::exit_code_priority_between_mixed_required_results`, docs/ARCHITECTURE.md, docs/TESTING.md, docs/EVALUATION.md and CLI output all state this one order.
- `sign_message` is a capability probe, never a required test. Advertised means NOT TESTED because nwc 0.45.2 has no sign_message API. Not advertised means UNSUPPORTED and no request is sent.
- The name `pay_invoice` legitimately appears in tally-core as NIP-47 method data (capability test data in `crates/tally-core/src/capabilities.rs` and one doc example in `crates/tally-core/src/report.rs`). Payment code lives only in tally-nwc, tally-runner and tally-cli.
- CLI facts: `tally doctor` (no network; prints sources as names only, never a value), `tally inspect --profile basic`, `tally test --profile basic|l402-e2e [--env sandbox|live] [--approve-payment-sats N] [--live-ack TEXT] [--clear-guard]`, `tally report --input FILE --format json|html --out FILE`. Connection strings come only from `TALLY_PAYER_NWC_URI`/`TALLY_RECEIVER_NWC_URI` (environment wins) or `.env.payer`/`.env.receiver` (`--payer-file`/`--receiver-file` change the paths); no flag accepts a connection string. `--ci` only documents intent: the CLI never prompts. A missing or unparseable connection string is exit 2 before any test runs, and the message never echoes the input. Reports default to `reports/` and guard files to `runs/`. `tally report` refuses an unknown `schema_version` with exit 2.
- Environments: every run is `sandbox` or `live`, and every endpoint is `local_fixture` or `external_sandbox`. The two faucet wallets are sandbox wallets on an external hub, and every report of such a run carries the limitation text saying so.
- Encryption negotiation: the cipher is `nip44_v2` when the info event's `encryption` tag lists `nip44_v2`, otherwise `nip04` (including when there is no `encryption` tag). The choice is passed to the nwc builder explicitly with `force_nip44_v2` or `force_nip04`, and the session tests prove the rule. The report field `encryption_mode_selected` exists, but the runner currently writes `null` into it (crates/tally-runner/src/run.rs); the recorded reports show `null` and no document may claim otherwise until the runner fills it in.
- L402 facts as implemented. (1) A challenge is HTTP 402 with `WWW-Authenticate: L402 macaroon="<base64>", invoice="<bolt11>"`; the scheme name is case-insensitive. (2) Credentials are `Authorization: L402 <macaroon base64>:<preimage hex>`; several macaroons would be base64 individually and comma separated before the colon, and Tally supports exactly one. (3) A macaroon or preimage containing control characters or a stray colon is an invalid L402; missing, invalid or insufficient credentials always get a 402 with a fresh challenge. (4) An L402 macaroon carries a version (0), a 32-byte user id and the invoice's payment hash; Tally puts the hash in the identifier and defines its own byte layout: version (2 bytes, big endian), then payment hash (32 bytes), then user id (32 bytes). This is Tally's own layout and is not a claim of byte compatibility with other implementations. (5) Caveats are `key = value` pairs; Tally mints exactly one, `services = tally_fixture:0`. (6) Verification: with the root key, verify the macaroon signature and every caveat, then check that SHA-256 of the preimage equals the payment hash in the identifier. Valid credentials stay valid for repeated use, not single-use, and verification fails closed on any unknown caveat. The fixture is a local test service. It proves Tally's flow, not compatibility with third-party L402 services.
- TEST ONLY exception: "A TEST ONLY invoice generator that signs real BOLT11 invoices offline (as the permit tests already do with the bitcoin crate) and simulates a payer by returning the stored preimage IS APPROVED, provided it is compiled only in tests (cfg(test), tests/common, or an off-by-default cargo feature), every item has a doc comment starting with "TEST ONLY", and no runtime path (CLI, server, examples) can use it."

## Security rules (non-negotiable)

- An NWC connection string (nostr+walletconnect://...) can spend money. Never print, log, serialize, commit or paste one. Hold the raw string only in secrecy::SecretString.
- VERIFIED: nostr's NostrWalletConnectUri implements Display, and Display PRINTS the secret. Never call to_string() or format it with {}, and never put it inside a type that implements Display or Serialize. Its Debug output is masked (verified), but still never log it.
- Errors must never echo the input that failed to parse.
- Treat invoices, preimages and macaroons as sensitive: no raw values in logs or reports.
- No real payment without my explicit approval for that specific run.

## Working rules

1. One step at a time. Do only what the current step says, then stop and report. Never start the next step.
2. Never fabricate. Report only what you ran or read in this session; write NOT VERIFIED for the rest. Do not guess versions, feature names, API names, field names or file contents: read them from the library's own source or docs and name the file you read.
3. No stubs, fakes or placeholder integrations. If you think one is needed, stop, flag it and wait for my confirmation. Scaffolding is allowed only when its doc comment starts with "Scaffold:" and docs/STATUS.md lists it as SCAFFOLD.
4. No emojis and no non-ASCII characters in code, comments, logs, console output or docs.
5. Keep docs/STATUS.md current at the end of every step.
6. Reports: paste command output verbatim, list NOT VERIFIED items, and end with the exact line the step asks for.

## Environment notes (this machine)

- Windows. The agent shell is Git Bash (bash syntax). Windows PowerShell here is 5.1: no && or ||. Call it as powershell.exe -NoProfile -Command '...' (single quotes).
- The IDE shell PATH may lack cargo. If cargo is not found, begin each bash call with:
  export PATH="/c/Users/user/.cargo/bin:/c/Users/user/AppData/Local/Microsoft/WinGet/Packages/BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe/mingw64/bin:$PATH"
- Long builds can time out your tool: run them in the background (nohup ... > log 2>&1 &) and poll the log.

## Session protocol

- Resume protocol: at the start of every step, run `git status --short` and `git log --oneline -n 8`. If the tree is not clean, or commits for the step already exist, report that first, verify the existing work against the step's requirements, and continue from it. Do not restart a step whose work is already on disk.
- Context hygiene: never cat large files or print long logs. Redirect long output to a file under /tmp and print only counts, grep lines or `tail -n 40`. For gates print the exit lines plus `grep "test result"`. Keep reports compact; paste CLI summaries in full because they are compact by design. Never describe output you did not obtain. If something will not fit in a report, say so and stop with the step marked BLOCKED rather than summarising silently.
- Commit at the end of every task of a step, with the message the step gives, so a context reset never loses work.

## TEST ONLY approvals

- TEST ONLY invoice generator, reuse: the offline invoice generator that signs real BOLT11 invoices is moved from `crates/tally-l402/tests/common` into `crates/tally-l402/src/test_support.rs`, behind the off-by-default cargo feature `test-support` of tally-l402, so tally-runner's tests can reuse it through a dev-dependency. Every item keeps a doc comment starting with "TEST ONLY", and no runtime path (CLI, server, examples) can reach it, because only a dev-dependency enables the feature.
- TEST ONLY fake wallets: a fake wallet pair in `crates/tally-runner/tests/common` implements the `WalletApi` trait with configurable failure modes. Only the wallets are faked: the fixture, guard, permit, preimage verification and reports are the real code. The fake pair is compiled only into the tally-runner test binary.

## Commands that must stay green

  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
