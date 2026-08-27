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

`icp deploy` deploys **all** of the dependency's canisters into the same environment, not just the exposed ones. A dependency's canisters may call each other, and icp-cli does not track an internal "requires" graph, so the whole dependency is always deployed — exactly as it would deploy on its own. `canisters:` here is an **exposure** filter (which IDs your canisters see), not a deployment filter.

"Exactly as it would deploy on its own" includes its own environments: if the dependency's environment [names a subset of its canisters](#which-canisters-an-environment-holds), only those are deployed to that environment.

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

Canister names and dependency aliases must contain only ASCII letters, digits, `_`, and `-`. This keeps them safe to reuse as store-key segments, `PUBLIC_CANISTER_ID` env-var names, URL subdomains, and archive paths; `:` in particular is reserved as the namespace separator.

## Deploy URLs

`icp deploy` prints a clickable URL for every canister it deploys, including a dependency's. A canister that serves the `http_request` endpoint gets a **frontend URL**; any other canister gets a **Candid UI URL**.

On a local network, a dependency canister's frontend subdomain is namespaced by the **alias** (not the store-key path), so it stays short and readable:

```
Deployed canisters:

Frontends (serving http_request):
  frontend: http://frontend.local.localhost:8000/
  vendor/openemail:frontend: http://frontend.openemail.local.localhost:8000/

Backends (Candid UI):
  vendor/openemail:backend: http://<candid-ui>.localhost:8000/?id=<id>
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

Because a member-scoped deploy does not redeploy the member's dependencies, it fails with a clear error if any dependency canister it is wired to has not been deployed in the workspace yet — deploy from the workspace root first so those IDs exist.

### Setting the root explicitly

Force the workspace root with the `--project-root-override` flag or the `ICP_PROJECT_ROOT` environment variable. This uses the given directory as the root with no upward walk — for example, to deploy a vendored project truly on its own.

## Environments across a workspace

The workspace root owns the **network** and the **canister-ID store** for every environment; a dependency's own network definitions are ignored when it is deployed as part of a workspace.

A dependency's own same-named environment still contributes its **per-canister settings** and **init args**, so a vendored project's canisters get the configuration their author intended. Precedence, highest first:

1. the workspace root's explicit override for that canister, keyed by the same path-based name used to address it (e.g. `settings: { "vendor/openemail:backend": … }`)
2. the dependency's own environment configuration
3. the canister's base settings

### Which canisters an environment holds

`canisters:` is not an override, so it does not follow that precedence. Each project's environment block decides **its own** canisters, and a project that writes no list contributes all of them:

```yaml
# app/icp.yaml — the workspace root, whose canisters are `app` and `worker`
environments:
  - name: staging
    canisters: [app]
```

```yaml
# app/vendor/openemail/icp.yaml — canisters `backend` and `frontend`
environments:
  - name: staging
    canisters: [backend]
```

`staging` holds `app` and `vendor/openemail:backend`. The root's list left out its own `worker`; openemail's left out its own `frontend`. Neither list reaches into the other project: this is what the dependency would deploy on its own, which is the point — vendoring does not change it.

`canisters: []` keeps the project that writes it out of the environment entirely, and only that project: from the root, it deploys none of the root's own canisters while every dependency still deploys all of its own.

A project may name **only** its own canisters — never `"vendor/openemail:frontend"` from the root, nor `"libfoo:util"` from inside openemail, however those names are spelled elsewhere in the manifest. Naming a canister the writing project does not declare is rejected when the project is read, whether it belongs to another project in the workspace or to nobody at all. So the answer to "is this canister in `staging`?" always comes from one manifest — the one that declares the canister — and it is the same answer vendored as standalone. Keeping a dependency's canister out of an environment means editing that dependency, which is what deploying it on its own would already have required.

Lists in different projects therefore never interact, and neither do lists for different environments. Two parents of a [shared dependency](#shared-dependencies) have nothing to disagree about: the shared instance decides for itself, once.

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

## Bundling a workspace

`icp project bundle` packages a workspace by mirroring it: the root project's `icp.yaml` sits at the archive root, each dependency instance gets its own `icp.yaml` at the directory it occupies in the workspace, and the `dependencies:` declarations are preserved, each pointing at the directory its dependency occupies in the archive. For a plainly vendored layout that is the path the manifest already used; a path that does not describe the dependency's location relative to the workspace root — an absolute path, or one that traverses a symlink — is rewritten so the extracted bundle stays self-contained. Canister names stay as each project wrote them, a shared dependency remains a single instance, and canister discovery works from the extracted bundle exactly as it did in the source workspace. Extracting the archive gives you the same workspace with every build step replaced by its built wasm.

Every dependency must resolve to a directory **inside** the workspace root, so that the archive can contain it. A dependency that resolves outside the root — `../elsewhere` declared by the root project, or a directory that is a symlink pointing out of the workspace — is rejected. For the same reason a vendored member that depends on a sibling cannot be bundled on its own (e.g. with `ICP_PROJECT_ROOT` pointing at the member): the sibling would fall outside the bundle. Bundle the workspace root instead.

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
