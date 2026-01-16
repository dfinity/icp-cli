#Requires -Version 7.3
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

wsl --install -d Ubuntu-22.04 --no-launch
wsl --set-default-version 2

ubuntu2204.exe install --root

wsl -d Ubuntu-22.04 -u root -- bash -c @'
set -ex
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io
nohup dockerd -H tcp://127.0.0.1:2375 >/var/log/dockerd.log 2>&1 &
for i in $(seq 1 30); do
    if docker -H tcp://127.0.0.1:2375 info >/dev/null 2>&1; then
        echo Docker ready
        exit 0
    fi
    sleep 1
done
cat /var/log/dockerd.log
exit 1
'@

'DOCKER_HOST=tcp://localhost:2375' >> $env:GITHUB_ENV
