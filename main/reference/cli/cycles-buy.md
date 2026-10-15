# icp cycles buy

Buy cycles with a card.

Creates an order on a cycles gateway canister as the current identity, prints the page to pay on (and opens it in a browser when run from a terminal, but not over SSH or on CI), then waits for the cycles to land on the identity's own cycles-ledger account. Fund a canister from there with `icp canister top-up`. Say either how much to spend (--amount, in the currency given by --currency) or how many cycles to receive (--cycles); the latter pays the least amount that buys them once the card fee is taken out. The gateway publishes its per-order range and its fees, and the quote shows them. The default gateway is CyclePay's, on mainnet, which accepts USD only.

Exactly one of --amount or --cycles must be given.

**Usage:** `icp cycles buy [OPTIONS]`

###### **Options:**

* `--amount <AMOUNT>` — Amount to spend, as a decimal (e.g. 10 or 12.50), in the currency given by --currency
* `--cycles <CYCLES>` — Cycles to receive; the amount to pay is worked out from the current rate, card fee included. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `--currency <CODE>` — ISO 4217 code of the currency to pay in. CyclePay accepts USD only

  Default value: `USD`
* `--gateway <CANISTER_ID>` — Canister id of the cycles gateway to buy through: any canister serving the cycles gateway interface. Defaults to CyclePay's backend on mainnet, or to ICP_CYCLES_GATEWAY_CANISTER_ID when set
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`. One of `mainnet`, `fetch`, or a 266-character hex-encoded root key
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-y`, `--yes` — Skip confirmation prompts
* `--no-open` — Do not open the payment page in a browser
* `--no-wait` — Print the payment page's link and return without waiting for the order to complete
* `--json` — Output command results as JSON
* `-q`, `--quiet` — Suppress human-readable output; print only the payment page's link, then the final status




