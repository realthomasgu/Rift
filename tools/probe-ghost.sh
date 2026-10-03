#!/usr/bin/env bash
# Probe: a second boot entry for one kernel, the way Ghost mode needs one. It builds nothing of
# Rift. The questions are all about ukify, systemd-boot and the stub, so the probe makes a two
# profile uki out of a plain nixpkgs kernel, puts it on an esp of its own with systemd-boot, and
# boots that in qemu. It also runs systemd-cryptsetup-generator by hand to see what rd.luks=0 does
# to a crypttab entry.
set -euo pipefail

echo "== the pieces, all from the binary cache =="
nix build --inputs-from . nixpkgs#systemd -o probe-systemd -L
nix build --inputs-from . nixpkgs#systemdUkify -o probe-ukify -L
# the fd output is the one with the firmware volumes in it, and nix names its link after it
nix build --inputs-from . nixpkgs#OVMF.fd -o probe-ovmf -L
ovmf=$(nix build --inputs-from . nixpkgs#OVMF.fd --no-link --print-out-paths | grep -- '-fd$' | head -1)
ls -R "$ovmf" | head -20
nix build --inputs-from . nixpkgs#qemu -o probe-qemu -L
nix build --inputs-from . nixpkgs#linuxPackages.kernel -o probe-kernel -L
nix build --inputs-from . nixpkgs#dosfstools -o probe-dosfstools -L
nix build --inputs-from . nixpkgs#mtools -o probe-mtools -L

ukify=probe-ukify/lib/systemd/ukify
bzimage=probe-kernel/bzImage
systemd_boot=probe-systemd/lib/systemd/boot/efi/systemd-bootx64.efi
ls -l "$bzimage" "$systemd_boot"

echo "== what ukify knows about profiles =="
$ukify --version
$ukify build --help 2>&1 | grep -iE -- "--profile|--join-profile|--sign-profile" || true

echo "== the base and the ghost profile =="
# a profile section is an env file: ID is what the entry id gets, TITLE what the menu shows
printf 'ID=main\nTITLE=Rift\n' > base.profile
printf 'ID=ghost\nTITLE=Ghost mode\n' > ghost.profile
# the sections of a profile override the base's. a profile binary has no kernel of its own, only
# the sections that differ, and the .profile section has to be first in it
$ukify build \
  --profile=@ghost.profile \
  --cmdline="console=ttyS0,115200 rift.probe=ghost rift.ghost rd.luks=0" \
  --output=ghost-profile.efi
ls -l ghost-profile.efi

# os-release gives the entry its name in the menu, the way the image's does
printf 'ID=rift\nNAME=Rift\nPRETTY_NAME=Rift 0.1.0\nVERSION=0.1.0\nVERSION_ID=0.1.0\n' > probe-os-release
# an initrd that is a cpio of nothing: the kernel prints its command line before it looks for an init
: > empty && printf '' | cpio -o --format=newc > initrd.cpio 2>/dev/null || true
ls -l initrd.cpio

$ukify build \
  --linux="$bzimage" \
  --initrd=initrd.cpio \
  --cmdline="console=ttyS0,115200 rift.probe=base" \
  --os-release=@probe-os-release \
  --uname=probe \
  --profile=@base.profile \
  --join-profile=ghost-profile.efi \
  --output=rift_0.1.0+3.efi
ls -l rift_0.1.0+3.efi

echo "== what the two profile uki holds =="
# --json comes before the verb, the way the nixos module calls it
$ukify --json=pretty inspect rift_0.1.0+3.efi | head -200 || true
echo "-- what the verity check reads, which has to be the base profile's command line --"
cat > probe-profiles.py <<'PYEOF'
import json
import sys

uki = json.load(sys.stdin)
print("base .cmdline:", uki.get(".cmdline", {}).get("text"))
for n, profile in enumerate(uki.get("_profiles", [])):
    said = profile.get(".profile", {}).get("text")
    line = profile.get(".cmdline", {}).get("text")
    print(f"profile {n}: .profile={said!r} .cmdline={line!r}")
PYEOF
$ukify --json=short inspect rift_0.1.0+3.efi | python3 probe-profiles.py || true
echo "-- section order --"
$ukify inspect rift_0.1.0+3.efi 2>&1 | grep -E "^[a-z.]+:|name:" | head -60 || true

echo "== the esp =="
rm -f esp.img
truncate -s 256M esp.img
probe-dosfstools/bin/mkfs.vfat -F 32 -n ESP esp.img >/dev/null
export MTOOLS_SKIP_CHECK=1
mcopy=probe-mtools/bin/mcopy
mmd=probe-mtools/bin/mmd
$mmd -i esp.img ::/EFI ::/EFI/BOOT ::/EFI/Linux ::/loader
$mcopy -i esp.img "$systemd_boot" ::/EFI/BOOT/BOOTX64.EFI
$mcopy -i esp.img rift_0.1.0+3.efi ::/EFI/Linux/rift_0.1.0+3.efi
# the image's own loader.conf: no menu unless a key is held, and no editing the command line
printf 'timeout 0\neditor no\n' > loader.conf
$mcopy -i esp.img loader.conf ::/loader/loader.conf
probe-mtools/bin/mdir -i esp.img -/ ::/ || true

echo "== what rd.luks=0 does to a crypttab entry =="
# the generator reads /etc/crypttab and the kernel command line. a mount namespace of its own gets
# a crypttab like the image's, and SYSTEMD_PROC_CMDLINE is what systemd's own tests use
printf 'persist /dev/disk/by-partlabel/persist - x-systemd.device-timeout=infinity\n' > probe-crypttab
gen=probe-systemd/lib/systemd/system-generators/systemd-cryptsetup-generator
for line in "" "rd.luks=0"; do
  sudo rm -rf genout && mkdir -p genout/normal genout/early genout/late
  sudo unshare --mount sh -c "
    mount --bind $PWD/probe-crypttab /etc/crypttab
    SYSTEMD_IN_INITRD=1 SYSTEMD_PROC_CMDLINE='$line' \
      $PWD/$gen $PWD/genout/normal $PWD/genout/early $PWD/genout/late
  " || echo "the generator exited $?"
  echo "-- command line ${line:-(nothing)} --"
  find genout -mindepth 1 | sort
done

echo "== boot =="
sudo apt-get update >/dev/null
sudo apt-get install -y --no-install-recommends python3-pexpect >/dev/null
if [ -e /dev/kvm ]; then sudo chmod 666 /dev/kvm; ls -l /dev/kvm; else echo "no /dev/kvm, tcg"; fi
python3 tools/probe-ghost.py \
  --qemu probe-qemu/bin/qemu-system-x86_64 \
  --firmware "$ovmf/FV" \
  --esp esp.img \
  --log ghost-probe.log \
  --shot ghost-probe-menu.ppm
