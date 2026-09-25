# icp message send

Submit a message signed on another machine

Takes a file written by `icp canister call --sign-only`, shows what it contains, submits it, and waits for the reply. No identity is used and none is needed: the message was already signed by whoever composed it, so this machine only has to carry it to the network.

It is submitted to the network the file names. If that has to change — the signing machine recorded a URL this one cannot reach, say — edit `network` in the file: the envelope is signed and carries no URL of its own, so where it goes cannot change what executes.

**Usage:** `icp message send [OPTIONS] <FILE>`

###### **Arguments:**

* `<FILE>` — The signed message file. `-` reads stdin

###### **Options:**

* `--dry-run` — Show what the message contains and exit without submitting it
* `-y`, `--yes` — Submit without asking for confirmation
* `--candid <PATH>` — Path to a Candid (`.did`) file describing the canister's interface, overriding the one embedded in the message
* `-o`, `--output <OUTPUT>` — How to interpret and display the response

  Default value: `auto`

  Possible values:
  - `auto`:
    Try Candid, then UTF-8, then fall back to hex
  - `candid`:
    Parse as Candid and pretty-print; error if parsing fails
  - `text`:
    Parse as UTF-8 text; error if invalid
  - `hex`:
    Print raw response as hex

* `--json` — Output command results as JSON




