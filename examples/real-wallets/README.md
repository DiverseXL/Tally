# Real-wallet runs

Recorded real runs against real providers; no payment was made.

Date: 2026-11-20.

Two single-wallet inspections are stored here, each with the report.json and
report.html that the run wrote, unedited:

| Label | Command | Exit code |
| --- | --- | --- |
| coinos | `tally inspect --profile wallet --wallet-file .env.coinos --label coinos` | 0 |
| faucet-alby-hub | `tally inspect --profile wallet --wallet-file .env.payer --label faucet-alby-hub` | 0 |

The wallet files hold NWC connection strings and are git-ignored; they are not
part of this repository and no value from them appears in these reports. Each
run connected to the relays named inside its own connection string, issued one
invoice of 21 sats that was never paid, and looked it up once. Neither run was
given a payment approval, no profile that pays was run, and no payment was
made.

These results describe those wallets and those connections on that date only.
Permissions, budgets and method lists belong to each NWC connection and can
change without notice. The matrix built from these files is in
`docs/COMPATIBILITY.md`.
