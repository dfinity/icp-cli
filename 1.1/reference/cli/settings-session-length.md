# icp settings session-length

Set the session length for password-protected PEM identities

**Usage:** `icp settings session-length [VALUE]`

###### **Arguments:**

* `<VALUE>` — Duration (e.g. `5m`, `1h`, `2d`) or `disabled`. If omitted, prints the current value.

   Note that due to clock drift, 2 minutes are added to the given value, so `5m` produces a 7-minute-expiry delegation. `disabled` turns off session caching entirely.




