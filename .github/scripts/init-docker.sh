#!/usr/bin/env bash
set -ex
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io
nohup dockerd -H tcp://127.0.0.1:2375 >/var/log/dockerd.log 2>&1 &
for i in $(seq 1 30); do
    if docker -H tcp://127.0.0.1:2375 info >/dev/null 2>&1; then
        echo Docker ready
        echo DOCKER_HOST=tcp://127.0.0.1:2375 >> $GITHUB_ENV
        exit 0
    fi
    sleep 1
done
cat /var/log/dockerd.log
exit 1
