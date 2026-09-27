#!/usr/bin/env bash
# Probe: a software tpm in the vm, against the image the last run of main built. It downloads that
# image rather than building one, so the whole answer comes back in minutes.
set -euo pipefail

run=${1:?usage: probe-tpm.sh <run id of a green image job on main>}

echo "== qemu and swtpm =="
nix build .#vm -o vm -L
nix shell --inputs-from . nixpkgs#swtpm -c swtpm --version

echo "== what qemu can do =="
qemu=$(nix shell --inputs-from . nixpkgs#qemu_kvm -c sh -c 'command -v qemu-system-x86_64')
"$qemu" -device help 2>&1 | grep -i tpm || { echo "no tpm device in qemu"; exit 1; }
"$qemu" -tpmdev help 2>&1 | grep -i emulator || { echo "no emulator tpmdev in qemu"; exit 1; }

echo "== the image of run $run =="
gh run download "$run" --name "rift-image-$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run" --jq .head_sha)" --dir .
ls -la rift.raw.zst
df -h .

echo "== boot =="
sudo apt-get update >/dev/null
sudo apt-get install -y --no-install-recommends python3-pexpect >/dev/null
if [ -e /dev/kvm ]; then sudo chmod 666 /dev/kvm; fi
printf 'rift-test' > pass.txt
mkdir -p tpmstate
python3 tools/probe-tpm.py vm/bin/rift-vm rift.raw.zst pass.txt --tpm tpmstate --log tpm-probe.log
echo "== swtpm state =="
ls -la tpmstate
