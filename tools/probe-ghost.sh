#!/usr/bin/env bash
# Probe: what a ghost boot of the image main already built says about itself today.
#
# The words on the ghost profile's command line are the whole of what makes a boot a ghost one, and
# systemd-stub takes extra words off a SMBIOS string, so the image main already built can be booted
# as a ghost boot without building anything. One run is a page of what part 2 has to change.
set -euo pipefail

run=${1:?usage: probe-ghost.sh <run id whose image artifact to boot>}

echo "== the vm =="
nix build .#vm -o vm -L

echo "== the image of run $run =="
gh run download "$run" --name "rift-image-$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run" --jq .head_sha)" --dir .
ls -la rift.raw.zst
df -h .

echo "== boot =="
sudo apt-get update >/dev/null
sudo apt-get install -y --no-install-recommends python3-pexpect >/dev/null
if [ -e /dev/kvm ]; then sudo chmod 666 /dev/kvm; ls -l /dev/kvm; else echo "no /dev/kvm, tcg"; fi
printf 'rift-test' > pass.txt
python3 tools/probe-ghost.py vm/bin/rift-vm rift.raw.zst pass.txt --log ghost-probe.log
