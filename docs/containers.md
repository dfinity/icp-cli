# Containerized networks

`icp-cli` contains native support for managed networks launched through Docker containers.

```yml
# icp.yaml
networks:
  - name: my-network
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - 0:4943
```

This will automatically spin up a container on `icp network start`, and stop the container on Ctrl-C or `icp network stop`.

The `ghcr.io/dfinity/icp-cli-network-launcher` image is a reference image; it is suitable for most use cases, but you can also bring your own as long as it conforms to the image requirements.

## Requirements for custom images

### Interface version

In addition to any environment variables specified in the `environment` field, `icp-cli` will provide `ICP_CLI_NETWORK_LAUNCHER_INTERFACE_VERSION=<version>`. Even if a container has no backcompat behavior, it should ideally check for this version and early-exit if it is an incompatible version.

The latest version is `1.0.0` at the time of writing, and a v1.x interface version is expected to write a *status file* to the *status directory.*

### Status file

A mount point is described in the network config under the key `status-dir`. If not specified it defaults to `/app/status`. The container is expected to write a file named `status.json` to this directory, containing a JSON object with the following fields:

- `v`: string, always `"1"`.
- `gateway_port`: uint, container-side port of the ICP HTTP gateway.
- `root_key`: string, hex-encoded root key of the network.

It may optionally also provide the following PocketIC-specific fields, if pocket-ic is used in the image:

- `config_port`: uint, the pocket-ic server's own port (also called the admin port).
- `instance_id`: uint, the ID of the pocket-ic instance.
- `default_effective_canister_id`: string, the principal that provisional management canister methods should be called under

### Container

The container should start its network automatically, and write the status file only when the gateway API is ready to be connected to. When the container receives the stop signal (which is `SIGTERM` by default, be sure to specify `STOPSIGNAL` if you need it to be `SIGINT` instead), it should gracefully shut down the network, then exit.

### ICP network

The network is expected to have the ICP ledger, the cycles ledger, and the cycles minting canister set up. The anonymous principal `2vxsx-fae` requires a significant initial ICP balance for seeding identities with ICP/cycles.

## Configuration

The gateway port of the network must be bound to a host port (permitted to be 0). Containerized network configurations can have the following fields:

- `port-bindings`: []string, mandatory if an image is specified, in `host:container` format. There must be an entry for the gateway port.
- `rm-on-exit`: bool, default false, deletes the container when the network is stopped
- `args`: []string, appended to the container's entrypoint
- `entrypoint`: []string, entrypoint executable for the container
- `environment`: []string, environment variables for the container, in `VAR=VALUE` format, or just `VAR` to inherit from icp-cli's environment.
- `volumes`: []string, Docker volumes to mount into the container, in `name:path` format
- `mounts`: []string, bind mounts for the container, in `host-path:container-path[:flags]` format. `host-path` is permitted to be relative, and `flags` can be `rw` or `ro`.
- `platform`: string, explicit platform selection for Docker installations that support multi-platform hosts.
- `user`: string, the user to run the container as, in `user:group` format
- `shm-size`: uint, size of `/dev/shm` in bytes.
- `status-dir`: string, default `/app/status`, the status directory mentioned above.
