# Managing Identities

This is a detailed reference for identity management. If you're deploying to mainnet for the first time, start with [Deploying to Mainnet](deploying-to-mainnet.md) instead.

This guide covers:
- Understanding identity storage and security
- Creating and importing identities
- Exporting identities for backup or migration
- Renaming and deleting identities
- Using multiple identities
- Account identifiers for exchange compatibility
- Advanced identity management

## Understanding Identities

An identity consists of:
- A **private key** — Used to sign messages
- A **principal** — Your public identifier derived from the key

## Default Identity

When you first install icp-cli, an **anonymous** identity is used by default. This identity:
- Has the principal `2vxsx-fae` (anonymous principal)
- Is suitable for local development and testing (automatically funded on local networks)
- **Cannot be used for mainnet deployments** (no ICP or cycles, shared by all users)

For mainnet deployments, you must create a dedicated identity that you control and can fund with ICP and cycles.

## Creating an Identity

Create a new identity:

```bash
icp identity new my-identity
```

This generates a new key pair and displays a seed phrase. **Save the seed phrase** — it's only shown once and is required to restore your identity later.

## Listing Identities

View all available identities:

```bash
icp identity list
```

## Setting the Default Identity

Set which identity to use by default:

```bash
icp identity default my-identity
```

Check the current default:

```bash
icp identity default
```

## Viewing Your Principal

Display the principal for the current identity:

```bash
icp identity principal
```

For a specific identity:

```bash
icp identity principal --identity other-identity
```

## Account Identifiers

An **account identifier** is an address format used by the ICP ledger. The ICP ledger supports both principals and account identifiers for transfers.

Principals can be deterministically converted to account identifiers, but account identifiers cannot be converted back to principals (the conversion is one-way).

### Getting Your Account ID

Display your account identifier:

```bash
icp identity account-id
```

For a specific identity:

```bash
icp identity account-id --identity other-identity
```

### Converting Any Principal to Account ID

Convert any principal to its corresponding account identifier:

```bash
icp identity account-id --of-principal aaaaa-aa
```

**Note:** The `--of-principal` flag cannot be used with `--identity` since you're converting a specific principal, not using an identity's principal.

### When to Use Account Identifiers

You may need account identifiers when:
- **Receiving ICP from exchanges** — Many exchanges use account identifier format for ICP withdrawals
- **Interacting with certain wallets** — Some ICP wallets prefer account identifiers
- **Backwards compatibility** — Older integrations may expect account identifiers

For most modern use cases, you can use principals directly since the ICP ledger supports both formats.

## Importing Identities

### From a PEM File

```bash
icp identity import my-identity --from-pem ./key.pem
```

### From a Seed Phrase

```bash
icp identity import my-identity --from-seed-file ./seed.txt
```

Or enter interactively:

```bash
icp identity import my-identity --read-seed-phrase
```

## Exporting Identities

Export an identity as a plaintext PEM file for backup or migration purposes:

```bash
icp identity export my-identity > backup.pem
```

This works with all storage types (plaintext, password-protected, keyring). The exported PEM file can be imported on another machine or used as a backup.

### Exporting Password-Protected Identities

For password-protected identities, you can provide the password via file to avoid interactive prompts:

```bash
icp identity export my-identity --password-file ./password.txt > backup.pem
```

If you don't provide a password file, you'll be prompted to enter the password interactively.

**Security Note:** The exported PEM file contains your private key in plaintext. Store it securely and delete it after importing if no longer needed.

## Renaming and Deleting Identities

### Renaming an Identity

Change the name of an existing identity:

```bash
icp identity rename old-name new-name
```

This updates the identity's name while preserving all its keys and configuration.

### Deleting an Identity

Remove an identity you no longer need:

```bash
icp identity delete my-old-identity
```

**Warning:** This permanently deletes the identity. Make sure you have a backup (using `icp identity export`) if you might need to restore it later.

## Storage Options

When creating or importing, choose how to store the key:

### Keyring (Default, Recommended)

Uses your system's secure keyring:

```bash
icp identity new my-identity --storage keyring
```

### Password-Protected

Encrypts the key with a password:

```bash
icp identity new my-identity --storage password
```

You'll be prompted for the password when using this identity.

### Plaintext (Not Recommended)

Stores the key unencrypted:

```bash
icp identity new my-identity --storage plaintext
```

Only use for testing or non-sensitive deployments.

### Storage Locations

By default, identity data is stored in platform-specific directories:

- **macOS:** `~/Library/Application Support/org.dfinity.icp-cli/identity/`
- **Linux:** `~/.local/share/icp-cli/identity/`
- **Windows:** `%APPDATA%\icp-cli\data\identity\`

You can override the base directory by setting the [`ICP_HOME`](../reference/environment-variables.md) environment variable. When set, identities will be stored in `$ICP_HOME/identity/` instead.

The identity directory contains:
- `identity_list.json` — List of all identities and their metadata
- `identity_defaults.json` — Current default identity selection
- `keys/<name>.pem` — Private keys (only for password-protected or plaintext storage)

When using keyring storage (default), private keys are stored securely in your system's keyring instead of as PEM files.

## Using Identities per Command

Override the default identity for a single command:

```bash
icp deploy --identity production-deployer -e ic
```

## Using Password Files

For automation, provide passwords via file:

```bash
icp deploy --identity my-identity --identity-password-file ./password.txt
```

## Identity Best Practices

**Development:**
- Use a dedicated development identity
- Plaintext storage is acceptable for local testing

**Production:**
- Use keyring or password-protected storage
- Keep seed phrases in secure, offline storage
- Use separate identities for different environments
- Limit who has access to production identities

**CI/CD:**
- Store keys as secrets in your CI system
- Use password files for automated deployments
- Consider separate identities with limited permissions

## Troubleshooting

**"Not a controller"**

Your identity isn't authorized to manage this canister. You need to be added as a controller by an existing controller.

**"Password required"**

The identity uses password-protected storage. Either enter the password when prompted or use `--identity-password-file`.

**"Identity not found"**

Check available identities:

```bash
icp identity list
```

## Next Steps

- [Deploying to IC Mainnet](deploying-to-mainnet.md) — Use your identity to deploy

[Browse all documentation →](../index.md)
