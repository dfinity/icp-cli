# Using Containerized Networks

Run ICP test networks in Docker containers for isolated, reproducible development environments.

## Windows Users

On Windows, icp-cli automatically uses Docker for all local networks—no configuration needed. Just ensure [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/) is installed and running, then use `icp network start` as normal.

For advanced WSL2 setups without Docker Desktop, see [Manual dockerd in WSL2](#advanced-manual-dockerd-in-wsl2).

## When to Use This

On macOS and Linux, icp-cli runs the network launcher natively by default. You may want to use containerized networks when you:
- Want network isolation from your host system
- Need to run multiple independent network instances
- Want reproducible environments across your team
- Are deploying in containerized CI/CD pipelines
- Need specific network versions or configurations

## Prerequisites

- **Docker** installed and running ([Install Docker](https://docs.docker.com/get-docker/))
- **icp-cli** installed
- An existing project with `icp.yaml`

Verify Docker is running:
```bash
docker ps
```

## Quick Start

### 1. Configure a Containerized Network

Add this to your `icp.yaml`:

```yaml
networks:
  - name: docker-local
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "8000:4943"  # Maps container port 4943 to host port 8000
```

The `ghcr.io/dfinity/icp-cli-network-launcher` image is the official ICP test network image and includes:
- ICP ledger canister
- Cycles ledger canister
- Cycles minting canister
- Pre-funded anonymous principal for development

**Note:** Network state is ephemeral—deployed canisters and their data are lost when the network stops. Persistence is not yet supported.

### 2. Start the Network

```bash
icp network start docker-local
```

This will:
1. Pull the Docker image (first time only)
2. Start a container with an ICP test network
3. Expose the network on `http://localhost:8000`

You'll see output indicating the network is ready:
```
✓ Network docker-local started
  Gateway: http://localhost:8000
```

### 3. Deploy Your Canisters

Create an environment that uses your containerized network:

```yaml
environments:
  - name: docker
    network: docker-local
    canisters:
      - my-canister
```

Then deploy:
```bash
icp deploy --env docker
```

### 4. Stop the Network

```bash
# Graceful shutdown
icp network stop docker-local

# Or press Ctrl-C in the terminal where it's running
```

## Common Configurations

### Dynamic Port Allocation

Let Docker choose an available port automatically:

```yaml
networks:
  - name: docker-local
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "0:4943"  # Docker assigns a random available host port
```

Find the assigned port:
```bash
icp network status docker-local
# Shows: Port: 54321 (example)
```

### Multiple Networks

Run multiple isolated networks simultaneously:

```yaml
networks:
  - name: docker-dev
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "8000:4943"

  - name: docker-test
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "8001:4943"
```

Start both:
```bash
icp network start docker-dev
icp network start docker-test
```

### Custom Environment Variables

Pass environment variables to the container:

```yaml
networks:
  - name: docker-local
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "8000:4943"
    environment:
      - LOG_LEVEL=debug
      - POCKET_IC_MUTE_SERVER=false
```

### Remove Container on Exit

Automatically delete the container when stopped:

```yaml
networks:
  - name: docker-local
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "8000:4943"
    rm-on-exit: true  # Clean up container on stop
```

Useful for CI/CD or temporary testing.

## Troubleshooting

### "Cannot connect to Docker daemon"

**Problem**: Docker is not running.

**Solution**: Start Docker Desktop or the Docker daemon:
```bash
sudo systemctl start docker  # Linux
# On macOS/Windows, open the Docker Desktop application
```

### "Port already in use"

**Problem**: Another process is using the host port.

**Solutions**:
1. Change the host port in `port-mapping`:
   ```yaml
   port-mapping:
     - "8001:4943"  # Try a different port
   ```

2. Or use dynamic allocation:
   ```yaml
   port-mapping:
     - "0:4943"
   ```

### "Container fails to start"

**Problem**: Container exits immediately or fails to start.

**Solution**: Check container logs:
```bash
# Find container ID
docker ps -a | grep icp-cli-network-launcher

# View logs
docker logs <container-id>
```

Common issues:
- Image pull failed (check internet connection)
- Port conflict inside container (check `port-mapping`)
- Insufficient resources (increase Docker memory/CPU limits)

### "Network unreachable after start"

**Problem**: `icp network start` succeeds but cannot connect.

**Solution**: Check the network status file was written:
```bash
# The container should write status to the status directory
docker exec <container-id> cat /app/status/status.json
```

If the file is missing or incomplete, the container may still be initializing. Wait a few seconds and try again.

## Configuration Reference

All available configuration options for containerized networks:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Unique network identifier |
| `mode` | string | Yes | Must be `managed` |
| `image` | string | Yes | Docker image to use |
| `port-mapping` | string[] | Yes | Port mappings in `host:container` format |
| `rm-on-exit` | bool | No | Delete container when stopped (default: `false`) |
| `volumes` | string[] | No | Docker volumes in `name:container_path[:options]` format |
| `mounts` | string[] | No | Bind mounts in `host_path:container_path[:flags]` format (flags: `ro` or `rw`) |
| `environment` | string[] | No | Environment variables in `VAR=VALUE` format |
| `args` | string[] | No | Additional arguments to container entrypoint (aliases: `cmd`, `command`) |
| `entrypoint` | string[] | No | Override container entrypoint |
| `platform` | string | No | Platform selection (e.g., `linux/amd64`) |
| `user` | string | No | User to run as in `user[:group]` format (group is optional) |
| `shm-size` | number | No | Size of `/dev/shm` in bytes |
| `status-dir` | string | No | Status directory path (default: `/app/status`) |

Example with multiple options:

```yaml
networks:
  - name: docker-local
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "8000:4943"
    volumes:
      - icp-data:/data
    mounts:
      - "./config:/app/config:ro"  # Mount local config as read-only
    environment:
      - LOG_LEVEL=info
      - POCKET_IC_MUTE_SERVER=true
    rm-on-exit: false
    shm-size: 2147483648  # 2GB
```

## Advanced: Custom Images

If the default image doesn't meet your needs, you can create a custom Docker image that implements the icp-cli network launcher interface.

### Interface Version

Your container must support the icp-cli network launcher interface. The environment variable `ICP_CLI_NETWORK_LAUNCHER_INTERFACE_VERSION` is provided by icp-cli.

Current interface version: `1.0.0`

Your container should:
1. Read `ICP_CLI_NETWORK_LAUNCHER_INTERFACE_VERSION`
2. Verify it supports the version
3. Exit early if the version is incompatible

### Status File Requirements

Your container must write a status file to the status directory (default: `/app/status/status.json`) when the network is ready.

**Important**: The CLI automatically mounts the status directory as read-write, so your container can write to it. The file must contain a single line of JSON ending with a newline character.

**Required fields**:
- `v`: string, must be `"1"` (status file format version)
- `gateway_port`: number, the container port where the ICP HTTP gateway listens
- `root_key`: string, hex-encoded root key of the network

**Optional fields** (primarily for PocketIC-based networks):
- `instance_id`: number or null, PocketIC instance ID
- `config_port`: number or null, PocketIC admin port
- `default_effective_canister_id`: string or null, principal for provisional canister calls

**Example**:

```json
{
  "v": "1",
  "gateway_port": 4943,
  "root_key": "308182301d060d2b0601040182dc7c0503010201060c2b0601040182dc7c05030201036100814c0e6ec71fab583b08bd81373c255c3c371b2e84863c98a4f1e08b74235d14fb5d9c0cd546d9685f913a0c0b2cc5341583bf4b4392e467db96d65b9bb4cb717112f8472e0d5a4d14505ffd7484b01291091c5f87b98883463f98091a0baaae",
  "instance_id": null,
  "config_port": null,
  "default_effective_canister_id": null
}
```

### Container Behavior

Your container must:

1. **Start automatically** - Launch the network when the container starts
2. **Write status file only when ready** - Wait until the gateway API is accessible
3. **Handle stop signals** - Gracefully shut down on `SIGTERM` (or `SIGINT` if you set `STOPSIGNAL`)
4. **Exit cleanly** - Exit after shutdown completes

Example Dockerfile:

```dockerfile
FROM ubuntu:22.04

# Install your ICP network implementation
RUN apt-get update && apt-get install -y curl
# ... install network software ...

# Create status directory
RUN mkdir -p /app/status

# Set stop signal if needed
STOPSIGNAL SIGTERM

# Copy startup script
COPY start-network.sh /app/start-network.sh
RUN chmod +x /app/start-network.sh

ENTRYPOINT ["/app/start-network.sh"]
```

### Network Requirements

Your custom network must include:

- **ICP ledger canister** - For ICP token management
- **Cycles ledger canister** - For cycles management
- **Cycles minting canister** - For creating cycles
- **Pre-funded anonymous principal** - Principal `2vxsx-fae` must have initial ICP balance for identity seeding

### Port Binding

The gateway port (the port your ICP HTTP gateway listens on inside the container) must be mapped to a host port:

```yaml
port-mapping:
  - "8000:4943"  # host:container
```

You can use `0` for dynamic host port allocation:

```yaml
port-mapping:
  - "0:4943"
```

### Testing Your Custom Image

1. Build your image:
   ```bash
   docker build -t my-icp-network .
   ```

2. Test it manually:
   ```bash
   docker run -p 8000:4943 my-icp-network

   # In another terminal, check status
   curl http://localhost:8000/api/v2/status
   ```

3. Configure in `icp.yaml`:
   ```yaml
   networks:
     - name: custom-network
       mode: managed
       image: my-icp-network
       port-mapping:
         - "8000:4943"
   ```

4. Start with icp-cli:
   ```bash
   icp network start custom-network
   ```

## Advanced: Manual `dockerd` in WSL2

If you're on Windows and want to use a manually instantiated `dockerd` in a WSL2 instance instead of Docker Desktop, set these environment variables:
- `ICP_CLI_DOCKER_WSL2_DISTRO=<distro>` — the WSL2 distribution name running dockerd
- `DOCKER_HOST=tcp://<ip>:<port>` — the TCP address where dockerd is listening

## Related Documentation

- [Managing Environments](managing-environments.md) — Configure environments that use containerized networks
- [Local Development](local-development.md) — Development workflow with test networks
- [Configuration Reference](../reference/configuration.md) — Full network configuration options

[Browse all documentation →](../index.md)
