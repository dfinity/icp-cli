# Canister Discovery

How icp-cli enables canisters to discover each other through automatic ID injection.

## The Discovery Problem

Canister IDs are assigned at deployment time and differ between environments:

| Environment | Backend ID |
|-------------|-----------|
| local | `bkyz2-fmaaa-aaaaa-qaaaq-cai` |
| staging | `rrkah-fqaaa-aaaaa-aaaaq-cai` |
| ic (mainnet) | `xxxxx-xxxxx-xxxxx-xxxxx-cai` |

Hardcoding IDs creates problems:

- Deploying to a new environment requires code changes
- Recreating a canister invalidates hardcoded references
- Sharing code with others fails because IDs don't match

## Automatic Canister ID Injection

icp-cli solves this by automatically injecting canister IDs as environment variables during deployment.

### How It Works

During `icp deploy`, icp-cli automatically:

1. Collects all canister IDs in the current environment
2. Creates a variable for each: `PUBLIC_CANISTER_ID:<canister-name>` → `<principal>`
3. Injects **all** these variables into **every** canister in the environment

This means each canister receives the IDs of all other canisters, enabling any canister to call any other canister without hardcoding IDs.

### Variable Format

For an environment with `backend`, `frontend`, and `worker` canisters:

```
PUBLIC_CANISTER_ID:backend  → bkyz2-fmaaa-aaaaa-qaaaq-cai
PUBLIC_CANISTER_ID:frontend → bd3sg-teaaa-aaaaa-qaaba-cai
PUBLIC_CANISTER_ID:worker   → b77ix-eeaaa-aaaaa-qaada-cai
```

These variables are stored in canister settings, not baked into the WASM. The same WASM can run in different environments with different canister IDs.

### Deployment Order

When deploying multiple canisters:

1. `icp deploy` creates all canisters first (getting their IDs)
2. Then injects `PUBLIC_CANISTER_ID:*` variables into all canisters
3. Then installs WASM code

All canisters can reference each other's IDs regardless of declaration order in `icp.yaml`.

## Frontend to Backend Communication

When your frontend is deployed to an asset canister:

1. The asset canister receives `PUBLIC_CANISTER_ID:*` variables
2. It exposes them via a cookie named `ic_env`, along with the network's root key (`IC_ROOT_KEY`)
3. Your frontend JavaScript reads the cookie to get canister IDs and root key

This mechanism works identically on local networks and mainnet — your frontend code doesn't need to change between environments.

### Working Examples

- **hello-world template** — The template from `icp new` demonstrates this pattern. Look at the frontend source code to see how it reads the backend canister ID.
- **[frontend-environment-variables example](https://github.com/dfinity/icp-cli/tree/main/examples/icp-frontend-environment-variables)** — A detailed example showing dev server configuration with Vite.

### Implementation

Use [@icp-sdk/core](https://www.npmjs.com/package/@icp-sdk/core) to read the cookie:

```typescript
import { getCanisterEnv } from "@icp-sdk/core/agent/canister-env";

interface CanisterEnv {
  "PUBLIC_CANISTER_ID:backend": string;
  IC_ROOT_KEY: Uint8Array;  // Parsed from hex by the library
}

const env = getCanisterEnv<CanisterEnv>();
```

For local development with a dev server, see the [Local Development Guide](../guides/local-development.md#frontend-development).

## Backend to Backend Communication

Since all canisters receive `PUBLIC_CANISTER_ID:*` variables for every canister in the environment, backend canisters can discover each other's IDs at runtime.

### Reading Environment Variables

**Rust** canisters can read the injected canister IDs using [`ic_cdk::api::env_var_value`](https://docs.rs/ic-cdk/latest/ic_cdk/api/fn.env_var_value.html):

```rust
use candid::Principal;

let backend_id = Principal::from_text(
    &ic_cdk::api::env_var_value("PUBLIC_CANISTER_ID:backend")
).unwrap();
```

**Motoko** does not currently have native support for reading canister environment variables. Use init arguments instead — pass canister IDs when initializing the canister:

```motoko
actor class MyCanister(backend_id : Principal) {
    // Use backend_id for inter-canister calls
};
```

### Making Inter-Canister Calls

Once you have the target canister ID, make calls using your language's CDK:

- **Rust**: [`ic_cdk::call`](https://docs.rs/ic-cdk) API
- **Motoko**: [Actor imports](https://docs.internetcomputer.org/motoko/home)

### Alternative Patterns

Beyond environment variables (Rust) or when environment variables aren't available (Motoko):

1. **Init arguments** — Pass canister IDs as initialization parameters
2. **Configuration** — Store IDs in canister state during setup

## Custom Environment Variables

Beyond automatic `PUBLIC_CANISTER_ID:*` variables, you can define custom ones in `icp.yaml`. See the [Environment Variables Reference](../reference/environment-variables.md#custom-variables) for configuration syntax.

## Troubleshooting

### "Canister not found" errors

Ensure the target canister is deployed:

```bash
icp canister list  # Check what's deployed
icp deploy         # Deploy all canisters
```

### Environment variables not available

Environment variables are set automatically during `icp deploy`. If you're using `icp canister install` directly, variables won't be set. Use `icp deploy` instead.

### Wrong canister ID in different environment

Check which environment you're targeting:

```bash
icp canister list -e local       # Local environment
icp canister list -e production  # Production environment
```

## See Also

- [Binding Generation](binding-generation.md) — Type-safe canister interfaces
- [Environment Variables Reference](../reference/environment-variables.md) — Complete variable documentation
- [Canister Settings Reference](../reference/canister-settings.md) — Settings configuration
- [Build, Deploy, Sync](build-deploy-sync.md) — Deployment lifecycle details
- [Local Development](../guides/local-development.md) — Frontend local dev setup

[Browse all documentation →](../index.md)
