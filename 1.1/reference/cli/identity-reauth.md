# icp identity reauth

Re-authenticate an Internet Identity delegation or create a PEM session delegation

**Usage:** `icp identity reauth [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Name of the identity to re-authenticate

###### **Options:**

* `--duration <DURATION>` — Session delegation duration (e.g. "30m", "8h", "1d"). Note that 2m extra is added when creating the delegation to account for clock drift. Required for PEM identities when session caching is disabled in settings. Not applicable for web-auth identities




