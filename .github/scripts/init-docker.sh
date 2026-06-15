#!/usr/bin/env bash
# This sets up dockerd in a WSL2 container manually. You could probably do this locally if you wanted,
# but the primary purpose is that GitHub Actions' WSL2 runners aren't set up for hypervisor support,
# so Docker Desktop's automatic WSL2 integration doesn't work.
set -ex
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io
# Installing docker.io enables a systemd docker service. The Ubuntu rootfs that
# setup-wsl v7 provisions has systemd enabled, so that service auto-starts and
# claims /var/run/docker.pid, which blocks the tcp-only daemon below (the one
# the Windows-side tests connect to). Stop it first. On a non-systemd image
# (e.g. setup-wsl v6) systemctl is absent, so this is a harmless no-op.
systemctl stop docker.socket docker.service 2>/dev/null || true
rm -f /var/run/docker.pid
nohup dockerd -H tcp://127.0.0.1:2375 >/var/log/dockerd.log 2>&1 &
for i in $(seq 1 30); do
    if docker -H tcp://127.0.0.1:2375 info >/dev/null 2>&1; then
        echo Docker ready
        {
            echo DOCKER_HOST=tcp://127.0.0.1:2375
            echo ICP_CLI_DOCKER_WSL2_DISTRO="$WSL_DISTRO_NAME"
        } >> $GITHUB_ENV
        exit 0
    fi
    sleep 1
done
cat /var/log/dockerd.log
exit 1
