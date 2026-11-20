# Compatibility matrix

Built from the reports in `examples/real-wallets/` by `scripts/build-compatibility.py`. Every value below is read from those JSON files; no wallet was paid.

## Inspections

| Wallet | Capability source | Encryption | Network | Methods advertised | connect | info_event | get_info | get_balance | make_invoice | lookup_invoice | Exit code |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| coinos | info_event | nip44_v2 | mainnet | 9 | PASS | PASS | PASS | PASS | PASS | PASS | 0 |
| faucet-alby-hub | info_event | nip44_v2 | mainnet | 14 | PASS | PASS | PASS | PASS | PASS | PASS | 0 |

## Capability checks

A PASS here means the method was advertised by the effective capability source. No request for the method was sent, so a PASS is not evidence that the call works.

| Wallet | capability_get_info | capability_get_balance | capability_make_invoice | capability_lookup_invoice | capability_pay_invoice |
| --- | --- | --- | --- | --- | --- |
| coinos | PASS | PASS | PASS | PASS | PASS |
| faucet-alby-hub | PASS | PASS | PASS | PASS | PASS |

## Differences found

- Network reported: mainnet.
- Encryption mode selected: nip44_v2.
- Capability source: info_event.
- Methods advertised by the effective source: coinos 9, faucet-alby-hub 14.
- Relays reached during connect: coinos 1, faucet-alby-hub 2.
- Balance reported at inspection time: coinos 0 msat, faucet-alby-hub 10000000 msat.
- coinos: notifications is listed by get_info but not by info_event.
- faucet-alby-hub: the two capability sources agree.
- Every required capability check passed on every wallet.

## Limits

Two connections on one date are a sample, not a survey. Only one of them is a third-party provider; the other is the sandbox Alby Hub that Tally already used. No payment was made, so nothing here says whether a payment succeeds or whether a preimage comes back. Permissions, budgets and method lists belong to each NWC connection and differ between connections to the same provider.

Date: 2026-11-20
