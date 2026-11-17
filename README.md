# Tally

Tally is a developer tool that tests whether a Bitcoin/Lightning wallet connected through Nostr Wallet Connect (NWC, NIP-47) can complete the workflows an app depends on. Its flagship proof is a real NWC wallet payment unlocking an L402-protected resource, where the returned preimage is checked against the invoice payment hash.

Status: early scaffold

Commands that work today (run from this folder):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo run -q -p tally-cli -- --version
cargo run -q -p tally-server -- --version
```
