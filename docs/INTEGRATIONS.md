# Integrations

What Tally depends on outside this repository, what those dependencies actually provide, and which parts of a run are real.

## NIP-47 facts used

- The wallet's capability event is kind 13194. Its `content` is plain text: space-separated method names. Tally never parses it as JSON (`capabilities::tests::json_like_content_is_never_parsed_as_capabilities`).
- The `encryption` and `extensions` tags carry space-separated tokens.
- The info event's `notifications` content token is compared against `get_info`'s notification list being non-empty, not against its methods (`capabilities::tests::notifications_flag_agrees_with_a_non_empty_notification_list`).
- `get_info` returns alias, color, pubkey, network, block height, block hash, methods and notifications.
- Every amount and fee is in millisatoshis.
- Wallet error codes come from the NIP-47 set, and lookup states are pending, settled, accepted, expired and failed.
- The cipher is chosen from the `encryption` tag: `nip44_v2` when listed, otherwise `nip04`, including when there is no tag (`session::tests::encryption_mode_follows_the_info_event_tag`).

Source: https://nips.nostr.com/47

## Versions and what the crates lack

Versions read from `Cargo.lock`:

| Crate | Version |
| --- | --- |
| nwc | 0.45.2 |
| nostr | 0.45.7 |
| lightning-invoice | 0.34.2 |
| libmacaroon | 0.4.0 |
| reqwest | 0.13.5 |
| sha2 | 0.11.1 |
| axum | 0.8.9 |
| clap | 4.6.7 |

Gaps that shaped the code:

- No info-event API in `nwc` 0.45.2. The crate's api module (outside this repository) covers `get_info`, `get_balance`, `pay_invoice`, `pay_keysend`, `lookup_invoice`, `make_invoice` and `list_transactions`; the kind 13194 event is fetched by the adapter with `nostr` instead.
- No `extensions` field on `GetInfoResponse` in `nostr` 0.45.7 (read in the crate source file nips/nip47.rs, outside this repository). The adapter reports `get_info` extensions as absent (null), never as an empty list; extension ids come only from the info event's `extensions` tag (`capabilities::tests::get_info_extensions_are_absent_not_empty`).
- No `sign_message` API in `nwc` 0.45.2. That is why an advertised `sign_message` is NOT TESTED rather than PASS, and an unadvertised one is UNSUPPORTED (`METHOD_NOT_ADVERTISED`).
- `UNSUPPORTED_ENCRYPTION` exists in the wallet error taxonomy (`crates/tally-core/src/outcome.rs`), but `crates/tally-nwc/src/error.rs` recognises only ten message spellings and not this one, so a wallet that returns it is currently treated as an error with no code, which is BLOCKED. This is a known gap, not a design choice.

## The faucet

`https://faucet.nwc.dev` creates disposable sub-wallets powered by Alby Hub. `POST https://faucet.nwc.dev?balance=10000` returns a fresh `nostr+walletconnect://` connection string; the source is https://github.com/getAlby/nwc-faucet.

- Balances are play money inside the faucet. The hub states this on its own page; the limitation text Tally writes into every such report says the same.
- One Alby Hub serves every faucet wallet. The hub has no Lightning channels, so nothing is routed over the real network. A payment between two faucet wallets settles inside the hub and returns a real preimage.
- The hub reports network `mainnet` and its invoices carry the Bitcoin mainnet prefix, even though it holds no channels and routes nothing. The invoice prefix is therefore not a safety signal on this hub. Tally records the mismatch instead of trusting the prefix: `invoice_validation` compares the invoice network with the wallet's reported network.
- The two service pubkeys in the recorded report identify disposable wallets created for that run. A service pubkey is a public identifier, not a secret.

Source: https://docs.nwc.dev

## L402 facts

- A challenge is HTTP 402 with `WWW-Authenticate: L402 macaroon="<base64>", invoice="<bolt11>"`. The scheme name is compared case-insensitively.
- Credentials are `Authorization: L402 <macaroon base64>:<preimage hex>`. Several macaroons would be base64 individually and comma separated before the colon; Tally supports exactly one.
- A macaroon or preimage containing control characters or a stray colon is an invalid L402. Missing, invalid or insufficient credentials always get 402 with a fresh challenge, never a different status.
- Caveats are `key = value` pairs. Tally mints exactly one: `services = tally_fixture:0`.
- Tally's own macaroon identifier layout is version (2 bytes, big endian), then the 32-byte payment hash, then a 32-byte user id (`macaroon::tests::identifier_layout_is_version_hash_then_user_id`). This is Tally's layout; no byte compatibility with other implementations is claimed.
- Verification: with the root key, verify the signature and every caveat, then check that SHA-256 of the preimage equals the payment hash in the identifier. Unknown caveats fail closed. Valid credentials are reusable, not single-use.

Sources: https://docs.lightning.engineering/the-lightning-network/l402 and https://github.com/lightninglabs/L402

## The macaroon crate

Tally uses `libmacaroon` 0.4.0, which is pure Rust. The older `macaroon` crate pulls in a C crypto library, so it was rejected. Neither crate is independently audited as far as we know. Macaroons are therefore used only inside the local fixture; the proof Tally actually relies on in its flagship check is the SHA-256 preimage check, done with `sha2` (`session::tests::preimage_check_matches_sha256_of_the_preimage`).

## Capabilities actually observed

From `examples/recorded-sandbox-run/approved/report.json`, both wallets (payer and receiver) reported:

- info event found, content kind `space_separated`;
- 14 effective method tokens from the info event: `get_info`, `pay_invoice`, `pay_keysend`, `multi_pay_invoice`, `multi_pay_keysend`, `get_balance`, `make_invoice`, `make_hold_invoice`, `settle_hold_invoice`, `cancel_hold_invoice`, `lookup_invoice`, `list_transactions`, `get_budget`, `notifications`;
- `encryption` tokens `nip44_v2` and `nip04`;
- `extensions` tokens `02`, `03`, `04`, `05`, `06`;
- `get_info` listing 13 methods and three notifications (`payment_received`, `payment_sent`, `hold_invoice_accepted`);
- no discrepancies between the two sources.

`sign_message` is not among the advertised methods, which is why the recorded run reports it UNSUPPORTED without sending anything.

## What is real, sandbox, local or mocked

| Layer | Status | Evidence |
| --- | --- | --- |
| Nostr relay traffic | LIVE | Two relays per wallet per the recorded report (`payer_relays_connected=2`, `receiver_relays_connected=2`) |
| Wallet calls (`get_info`, `get_balance`, `make_invoice`, `lookup_invoice`, `pay_invoice`) | LIVE | Requests went to the Alby Hub behind the faucet wallets |
| The payment itself | LIVE protocol, SANDBOX funds | 21000 msat moved inside one Alby Hub; no channels, no routing |
| L402 resource | LOCAL FIXTURE | `crates/tally-l402/src/fixture.rs` binds 127.0.0.1 port 0 |
| Invoices and preimages | REAL values, play money | Signed BOLT11 invoices from the hub, real preimages, stored nowhere |
| TEST ONLY invoice generator | MOCKED | `crates/tally-l402/src/test_support.rs`, off-by-default `test-support` feature |
| Fake wallet pair | MOCKED | `crates/tally-runner/tests/common/mod.rs`, compiled only into the tally-runner test binary |
