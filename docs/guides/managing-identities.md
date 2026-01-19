# Managing Identities

Identities represent who you are when interacting with the Internet Computer. This guide covers creating, importing, and using identities with icp-cli.

## Understanding Identities

An identity consists of:
- A **private key** — Used to sign messages
- A **principal** — Your public identifier derived from the key

Identities are stored in `~/.config/icp/identity/`.

## Creating an Identity

Create a new identity:

```bash
icp identity new my-identity
```

This generates a new key pair and stores it securely.

### Saving the Seed Phrase

To back up your identity, save the seed phrase:

```bash
icp identity new my-identity --output-seed seed.txt
```

Store `seed.txt` securely — it can restore your identity.

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

## Managing Controllers

Your identity's principal can be a controller of canisters. View canister controllers:

```bash
icp canister settings show <canister-id> -n ic
```

Add a controller:

```bash
icp canister settings update <canister-id> --add-controller <principal> -n ic
```

Remove a controller:

```bash
icp canister settings update <canister-id> --remove-controller <principal> -n ic
```

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
