#!/usr/bin/env bash
# Probe: what stops the initrd waiting for /dev/mapper/persist in a ghost boot.
#
# A ghost boot leaves persist locked, so /dev/mapper/persist never appears, and the mounts that come
# off it are Requires= of initrd-fs.target: the device job times out after 90 s and the boot ends in
# emergency mode. A ConditionKernelCommandLine on the mounts does not help, because conditions are
# checked after a unit's dependencies are satisfied and the device job is enqueued either way.
#
# The words rift.ghost and rd.luks=0 are the whole of what makes a boot a ghost one, and
# systemd-stub takes extra words off a SMBIOS string, so the image main already built can be booted
# as a ghost boot without building anything. That makes every command line answer cheap to try.
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
