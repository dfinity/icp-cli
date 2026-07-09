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

Both services' canisters read `PUBLIC_CANISTER_ID:openemail:backend`, and both
resolve to that one shared canister. If the two services instead vendored
separate copies of openemail (different directories), they would get isolated
instances.

Inspect the result with:

```bash
icp project show
icp canister status "umbrella/openemail:backend"
```
