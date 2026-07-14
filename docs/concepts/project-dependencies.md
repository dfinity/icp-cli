---
title: Project Dependencies
description: How one icp project can depend on another vendored icp project, deploy it as part of a workspace, and share a single set of canister IDs.
---

An `icp` project can build on top of another `icp` project — a **dependency** — whose source is vendored into it, typically as a git submodule. The dependency stays a complete, standalone project: it can be developed and deployed on its own, and it does not need to know it is being consumed.

This supports two related workflows:

- **Reuse** — build on another project's canisters (your canisters call theirs).
- **Monorepo / workspace** — develop several projects together and deploy them onto one network with a single shared set of canister IDs.

## Declaring a dependency

Add a top-level `dependencies:` block to your `icp.yaml`:

```yaml
dependencies:
  - name: openemail            # local alias — namespaces the dependency's canister IDs
    path: ./vendor/openemail   # directory containing the dependency's icp.yaml
    canisters: [backend]       # which of its canisters to expose (omit to expose all)
```

## What gets deployed

`icp deploy` deploys **all** of the dependency's canisters into the same environment, not just the exposed ones. A dependency's canisters may call each other, and icp-cli does not track an internal "requires" graph, so the whole dependency is always deployed — exactly as it would deploy on its own. `canisters:` is an **exposure** filter (which IDs your canisters see), not a deployment filter.

## Canister ID injection

Each canister receives canister IDs from the perspective of the project that owns it. Your canisters see:

- their own canisters by name — `PUBLIC_CANISTER_ID:backend`
- each **exposed** dependency canister under the alias — `PUBLIC_CANISTER_ID:openemail:backend`

The dependency's own canisters keep their standalone view (`PUBLIC_CANISTER_ID:backend`, …), so vendored code behaves identically whether deployed on its own or through your project. See [Canister Discovery](canister-discovery.md) for the injection mechanism.

## Addressing dependency canisters

Because two projects may each define a `backend`, imported canisters are keyed by their path relative to the workspace root, for example `vendor/openemail:backend`. Use that name anywhere a canister name is accepted:

```bash
icp canister status "vendor/openemail:backend"
icp deploy "vendor/openemail:backend"
```

`:` is reserved in canister names as the namespace separator.

## Deploy URLs

`icp deploy` prints a clickable URL for every canister it deploys, including a dependency's. A canister that serves the `http_request` endpoint gets a **frontend URL**; any other canister gets a **Candid UI URL**.

On a local network, a dependency canister's frontend subdomain is namespaced by the **alias** (not the store-key path), so it stays short and readable:

```
Deployed canisters:
  frontend: http://frontend.local.localhost:8000/
  vendor/openemail:frontend: http://frontend.openemail.local.localhost:8000/
  vendor/openemail:backend (Candid UI): http://<candid-ui>.localhost:8000/?id=<id>
```

A transitive dependency uses its full alias chain (`frontend.libfoo.openemail.<env>.localhost`). A [shared dependency](#shared-dependencies) is deployed once but reached through more than one alias chain, so it prints **one URL per chain**, each resolving to the same canister:

```
  umbrella/openemail:frontend: http://frontend.openemail.service-a.local.localhost:8000/
  umbrella/openemail:frontend: http://frontend.openemail.service-b.local.localhost:8000/
```

## Running commands inside a dependency (the workspace)

Vendored dependencies form a **workspace**. When you run an `icp` command from inside a vendored project, icp-cli walks **up** the directory tree to the outermost project that declares the one you are in as a dependency and treats it as the **workspace root**. The network, environments, and the canister-ID store all come from that root, so there is a single source of truth for canister IDs no matter where you run from.

```
app/
  icp.yaml                 # depends on ./vendor/openemail
  vendor/
    openemail/
      icp.yaml             # a standalone project
```

- `cd app && icp deploy` — deploys `app` and `openemail` into app's environment.
- `cd app/vendor/openemail && icp deploy` — resolves up to `app` and deploys **only openemail's** canisters into app's environment and ID store. The IDs are the same ones app's canisters were wired to, so iterating on a vendored dependency in place does not fork a separate deployment.

When a command resolves to a workspace root above the project you are standing in, icp-cli announces the resolved root so the behavior is visible.

Resolution is **bounded**: an ancestor is adopted only if it (transitively) declares your project, so an unrelated `icp.yaml` higher up never captures your project. A dependency cloned on its own has no declaring ancestor and behaves as its own root.

### Deploying part of a workspace

From inside a member, `icp deploy` with no canister names defaults to **that member's own canisters**. Deploy the whole workspace by running from the root, or target canisters explicitly by their namespaced names from anywhere.

### Setting the root explicitly

Force the workspace root with the `--project-root-override` flag or the `ICP_PROJECT_ROOT` environment variable. This uses the given directory as the root with no upward walk — for example, to deploy a vendored project truly on its own.

## Environments across a workspace

The workspace root owns the **network** and the **canister-ID store** for every environment; a dependency's own network definitions are ignored when it is deployed as part of a workspace.

A dependency's own same-named environment still contributes its **per-canister settings and init args**, so a vendored project's canisters get the configuration their author intended. Precedence, highest first:

1. the workspace root's explicit override for that canister (e.g. `settings: { "openemail:backend": … }`)
2. the dependency's own environment configuration
3. the canister's base settings

Because the root decides which environments exist, **every member must declare each environment the workspace targets.** Deploying to an environment a dependency does not declare fails with a clear error. If a dependency has no environment-specific configuration, declaring the environment with no overrides is enough:

```yaml
# in the dependency's icp.yaml
environments:
  - name: staging
```

`local` and `ic` are implicit for every project, so they never need to be declared.

## Shared dependencies

If two projects in a workspace depend on the same directory — for example two services that both vendor `../openemail` — it resolves to **one** instance, built and deployed once and shared by both. Identity is the resolved directory on disk, so two independent copies at different paths stay separate.

## Keeping a dependency self-contained

A vendored project must remain a complete `icp` project: it never references its parent, and you can copy or clone it elsewhere and it still works on its own. Vendoring may require [aligning environment names](#environments-across-a-workspace), but never changes to how the dependency finds its own canisters.

## Limitations

- icp-cli deploys a parent-owned copy of each dependency; binding directly to an already-deployed on-chain canister is not yet supported.
- Candid and binding generation for dependencies are out of scope — each canister generates the bindings it needs itself. See [Binding Generation](binding-generation.md).

## Examples

- [project-dependency](https://github.com/dfinity/icp-cli/tree/main/examples/icp-project-dependency) — a single vendored dependency.
- [project-dependency-shared](https://github.com/dfinity/icp-cli/tree/main/examples/icp-project-dependency-shared) — a shared dependency across sibling services.

## See Also

- [Canister Discovery](canister-discovery.md) — How canister IDs are injected
- [Environments and Networks](environments.md) — Deployment targets and how they relate
- [Project Model](project-model.md) — How icp-cli discovers and consolidates configuration
- [Configuration Reference](../reference/configuration.md) — `icp.yaml` fields

[Browse all documentation →](../index.md)
