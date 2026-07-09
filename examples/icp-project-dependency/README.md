# Project Dependency Example

This example shows how an `icp` project can depend on **another** `icp` project
that is vendored into it (typically as a git submodule), so both are developed in
one source tree and deployed together.

## Layout

```
icp-project-dependency/
  icp.yaml                     # the "app" — has a `backend` canister and a dependency
  vendor/
    openemail/                 # a self-contained icp project (would be a git submodule)
      icp.yaml                 # has `backend` and `frontend` canisters
```

The app declares:

```yaml
dependencies:
  - name: openemail
    path: ./vendor/openemail
    canisters: [backend]
```

## What `icp deploy` does

Running `icp deploy` in this directory:

1. Deploys the app's canisters **and all** of `openemail`'s canisters
   (`backend` and `frontend`) into the same environment. The whole dependency is
   always deployed because a dependency's canisters may call each other.
2. Injects the **selected** dependency canister IDs into the app's canisters as
   environment variables. Here the app's `backend` receives
   `PUBLIC_CANISTER_ID:openemail:backend`.

Canister-ID environment variables are set per project scope:

- The app's canisters see their own canisters by name
  (`PUBLIC_CANISTER_ID:backend`) plus the exposed dependency canisters under the
  alias (`PUBLIC_CANISTER_ID:openemail:backend`).
- `openemail`'s canisters see exactly what they would see when deployed
  standalone: `PUBLIC_CANISTER_ID:backend` and `PUBLIC_CANISTER_ID:frontend`,
  each resolving to openemail's own canisters — so vendored code behaves
  identically whether deployed on its own or as a dependency.

## Store keys

Because the app and openemail both define a `backend` canister, imported
dependency canisters are keyed by their path relative to the project root, e.g.
`vendor/openemail:backend`. Use these names to address dependency canisters
directly:

```bash
icp canister status "vendor/openemail:backend"
```
