#!/usr/bin/env bash
# Probe: what systemd-cryptenroll says about a security key when there is none, against the image
# the last run of main built. It downloads that image rather than building one.
set -euo pipefail

run=${1:?usage: probe-fido2.sh <run id of a green image job on main>}

echo "== the vm =="
nix build .#vm -o vm -L

echo "== the image of run $run =="
gh run download "$run" --name "rift-image-$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run" --jq .head_sha)" --dir .
ls -la rift.raw.zst
df -h .

echo "== boot =="
sudo apt-get update >/dev/null
sudo apt-get install -y --no-install-recommends python3-pexpect >/dev/null
if [ -e /dev/kvm ]; then sudo chmod 666 /dev/kvm; fi
printf 'rift-test' > pass.txt
python3 tools/probe-fido2.py vm/bin/rift-vm rift.raw.zst pass.txt --log fido2-probe.log
