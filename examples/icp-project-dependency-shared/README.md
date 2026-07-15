# Shared Dependency ("Umbrella") Example

This example shows a **shared** dependency: two independent sub-projects each
depend on the same third project, and it is deployed **once** and shared.

## Layout

```
icp-project-dependency-shared/
  icp.yaml                     # the "app" — depends on both services
  umbrella/                    # bundle of sibling projects (a git submodule in practice)
    service-a/
      icp.yaml                 # depends on ../openemail
    service-b/
      icp.yaml                 # depends on ../openemail
    openemail/
      icp.yaml                 # the shared service
```

`service-a` and `service-b` are self-contained `icp` projects — each can be
deployed on its own — and each declares:

```yaml
dependencies:
  - name: openemail
    path: ../openemail
    canisters: [backend]
```

The app depends on both services:

```yaml
dependencies:
  - name: service-a
    path: ./umbrella/service-a
    canisters: [service]
  - name: service-b
    path: ./umbrella/service-b
    canisters: [service]
```

## Shared instance

Both `service-a` and `service-b` reach `openemail` through `../openemail`, which
resolves to the **same** directory (`umbrella/openemail`). A dependency's identity
is its resolved source directory, so openemail is imported **once**:

Running `icp deploy` produces these canisters:

- `frontend` (the app)
- `umbrella/service-a:service`
- `umbrella/service-b:service`
- `umbrella/openemail:backend` — a **single** shared instance
- `umbrella/openemail:frontend` — the shared instance's asset canister

Both services' canisters read `PUBLIC_CANISTER_ID:openemail:backend`, and both
resolve to that one shared canister. If the two services instead vendored
separate copies of openemail (different directories), they would get isolated
instances.

## URLs printed after deploy

A canister that serves `http_request` prints a friendly frontend URL; anything
else prints a Candid UI URL. The interesting case here is
`umbrella/openemail:frontend`: it is deployed **once**, but it is reached through
two alias chains (`app → service-a → openemail` and `app → service-b →
openemail`), so `icp deploy` prints **one friendly URL per chain** — both point
at the same canister:

```
Deployed canisters:
  frontend: http://frontend.local.localhost:<port>/
  umbrella/service-a:service (Candid UI): ...
  umbrella/service-b:service (Candid UI): ...
  umbrella/openemail:backend (Candid UI): ...
  umbrella/openemail:frontend: http://frontend.openemail.service-a.local.localhost:<port>/
  umbrella/openemail:frontend: http://frontend.openemail.service-b.local.localhost:<port>/
```

The dependency subdomains are namespaced by the **alias chain**, not the on-disk
`umbrella/` path. The app's own `frontend` needs no alias.

Inspect the result with:

```bash
icp project show
icp canister status "umbrella/openemail:backend"
```
