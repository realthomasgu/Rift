#!/usr/bin/env python3
"""Probe: boot one drive as a ghost boot several times, each with different words on the kernel
command line, and say how far each one got.

The drive is written once and kept, then booted again and again with --image, the way boot-test's
tpm run does. The words go in through systemd-stub's SMBIOS string, so nothing is rebuilt.

Each boot is read for three things: the shell prompt (it worked), the passphrase prompt (persist
was opened, which a ghost boot must not do) and emergency mode (what happens today).
"""

import argparse
import os
import re
import subprocess
import sys
import time

import pexpect

PROMPT = r"rift(\x1b\[[0-9;]*m)*@(\x1b\[[0-9;]*m)*rift"
PASSPHRASE = r"(?i)passphrase[^\r\n]*:"
EMERGENCY = r"Reached target emergency\.target|Emergency Mode"
TIMEDOUT = r"Timed out waiting for device"

# the two words that make a boot a ghost one, from nix/image/ghost.nix
GHOST = "rift.ghost rd.luks=0"
# the mounts that come off persist, under /sysroot in the initrd and at their own names after it
PERSIST = [
    "persist",
    "home",
    "var",
    "var-lib-flatpak",
    "var-lib-rift-models",
    "var-lib-rift-hosts",
]


def masks(prefix, names):
    return " ".join(f"{prefix}systemd.mask={name}" for name in names)


# what to try, in order. the first is the control: it is what main does today
TRIES = [
    ("as it is today", GHOST),
    (
        "masking initrd-fs.target",
        f"{GHOST} rd.systemd.mask=initrd-fs.target",
    ),
    (
        "masking each mount under sysroot",
        f"{GHOST} " + masks("rd.", [f"sysroot-{name}.mount" for name in PERSIST]),
    ),
    (
        "masking initrd-fs.target and each mount on the other side of the switch",
        f"{GHOST} rd.systemd.mask=initrd-fs.target "
        + masks("", [f"{name}.mount" for name in PERSIST]),
    ),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vm")
    ap.add_argument("image")
    ap.add_argument("passfile")
    ap.add_argument("--log", default="ghost-probe.log")
    ap.add_argument("--seconds", type=int, default=240, help="how long each boot gets")
    args = ap.parse_args()

    log = open(args.log, "w", encoding="utf-8")

    class Tee:
        def write(self, text):
            sys.__stdout__.write(text)
            log.write(text)
            log.flush()

        def flush(self):
            sys.__stdout__.flush()
            log.flush()

    sys.stdout = Tee()

    kept = os.path.abspath("ghost-drive.img")
    base = [
        os.path.abspath(args.vm),
        "-smp", "2",
        "-m", "4096",
        "-device", "virtio-vga",
        "-display", "none",
        "-monitor", "none",
        "-serial", "stdio",
        "-no-reboot",
        "-nic", "none",
    ]

    # the drive is written once, with a persist of its own, and every boot after that is the same
    # drive. the first boot is an ordinary one, to prove the drive is good
    first = [base[0], "--image", os.path.abspath(args.image), "--persist",
             os.path.abspath(args.passfile), "--drive", kept, "--exchange", "1G"] + base[1:]
    found = {}
    for name, words in [("an ordinary boot, to prove the drive", None)] + TRIES:
        if words is None:
            cmd = first
        else:
            cmd = [base[0], "--image", kept] + base[1:] + [
                "-smbios", f"type=11,value=io.systemd.stub.kernel-cmdline-extra={words}"]
        print(f"\n==== {name}\n==== {' '.join(cmd)}", flush=True)
        child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace",
                              dimensions=(40, 160))
        child.logfile_read = Tee()
        start = time.monotonic()
        said = "nothing"
        try:
            which = child.expect([PROMPT, PASSPHRASE, EMERGENCY, TIMEDOUT],
                                 timeout=args.seconds)
            if which == 1 and words is None:
                # the ordinary boot asks for the passphrase, which is right
                child.send(open(args.passfile).read() + "\r")
                child.expect([PROMPT], timeout=args.seconds)
                said = "a shell, after the passphrase"
            else:
                said = ["a shell", "the passphrase prompt", "emergency mode",
                        "a device that timed out"][which]
        except pexpect.TIMEOUT:
            said = f"nothing in {args.seconds} s"
        except pexpect.EOF:
            said = "qemu ended"
        print(f"\n==== {name}: {said} after {time.monotonic() - start:.0f}s", flush=True)
        found[name] = said
        # a boot that got to a shell is shut down tidily so the drive is clean for the next one
        if said.startswith("a shell"):
            child.send("sudo systemctl poweroff\r")
            try:
                child.expect(pexpect.EOF, timeout=120)
            except pexpect.TIMEOUT:
                pass
        child.terminate(force=True)
        time.sleep(2)

    print("\n==== how far each one got ====")
    for name, said in found.items():
        print(f"{name}: {said}")


if __name__ == "__main__":
    main()
