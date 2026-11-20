# Compatibility matrix

Built from the reports in `examples/real-wallets/` by `scripts/build-compatibility.py`. Every value below is read from those JSON files; no wallet was paid.

## Inspections

Method counts are method names only: the `notifications` token is a flag, not a method, so it is excluded from the info-event count (an info event with 14 tokens including the flag shows 13). The `get_info` count is that response's method list.

| Wallet | Capability source | Encryption | Network | Info-event methods | get_info methods | connect | info_event | get_info | get_balance | make_invoice | lookup_invoice | Exit code |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| coinos | info_event | nip44_v2 | mainnet | 9 | 9 | PASS | PASS | PASS | PASS | PASS | PASS | 0 |
| faucet-alby-hub | info_event | nip44_v2 | mainnet | 13 | 13 | PASS | PASS | PASS | PASS | PASS | PASS | 0 |

## Method support

Effective capability source used: info_event (the info event governs when it lists at least one method token). `yes` means the method is in that wallet's effective source; `-` means it is not. The `notifications` token is a flag, not a method name, so it is left out of this table.

| Method | coinos | faucet-alby-hub |
| --- | --- | --- |
| cancel_hold_invoice | - | yes |
| get_balance | yes | yes |
| get_budget | - | yes |
| get_info | yes | yes |
| list_transactions | yes | yes |
| lookup_invoice | yes | yes |
| make_hold_invoice | - | yes |
| make_invoice | yes | yes |
| multi_pay_invoice | - | yes |
| multi_pay_keysend | - | yes |
| pay | yes | - |
| pay_invoice | yes | yes |
| pay_keysend | yes | yes |
| receive | yes | - |
| settle_hold_invoice | - | yes |

## Capability checks

A PASS here means the method was advertised by the effective capability source. No request for the method was sent, so a PASS is not evidence that the call works.

| Wallet | capability_get_info | capability_get_balance | capability_make_invoice | capability_lookup_invoice | capability_pay_invoice |
| --- | --- | --- | --- | --- | --- |
| coinos | PASS | PASS | PASS | PASS | PASS |
| faucet-alby-hub | PASS | PASS | PASS | PASS | PASS |

Every required capability check passed on every wallet.

## Differences found

- Methods only in coinos: pay, receive.
- Methods only in faucet-alby-hub: cancel_hold_invoice, get_budget, make_hold_invoice, multi_pay_invoice, multi_pay_keysend, settle_hold_invoice.
- Capability-source disagreement on coinos: notifications is listed by get_info but not by info_event.
- Relays listed in each connection: coinos 1, faucet-alby-hub 2.
- Identical on every wallet: network mainnet; encryption mode nip44_v2; capability source info_event.

## Limits

Two connections on one date are a sample, not a survey. Only one of them is a third-party provider; the other is the sandbox Alby Hub that Tally already used. No payment was made, so nothing here says whether a payment succeeds or whether a preimage comes back. Permissions, budgets and method lists belong to each NWC connection and differ between connections to the same provider.

Date: 2026-11-20
