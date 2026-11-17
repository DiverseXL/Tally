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

## Commands that must stay green

  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
