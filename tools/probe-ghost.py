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


# what to try, in order. the first is the control: it is what main does today.
#
# The last probe found that masking initrd-fs.target, or the mounts under it, both end in emergency
# mode within seconds: systemd-sysroot-fstab-check asks for initrd-fs.target by name, and it is run
# by initrd-parse-etc.service, which carries OnFailure=emergency.target. Nothing requires that
# service, so masking it as well should leave the initrd with nothing to say about persist at all.
# rd.systemd.wants= stands in for the one change this cannot test from the command line: in the real
# thing ghost-mode.service hangs off initrd.target instead of the target that is now masked
INITRD = (
    "rd.systemd.mask=initrd-fs.target rd.systemd.mask=initrd-parse-etc.service "
    "rd.systemd.wants=ghost-mode.service"
)
TRIES = [
    ("as it is today", GHOST),
    ("the initrd told to leave persist alone", f"{GHOST} {INITRD}"),
    (
        "and each mount masked on the other side of the switch",
        f"{GHOST} {INITRD} " + masks("", [f"{name}.mount" for name in PERSIST]),
    ),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vm")
    ap.add_argument("image")
    ap.add_argument("passfile")
    ap.add_argument("--log", default="ghost-probe.log")
    ap.add_argument("--seconds", type=int, default=300, help="how long each boot gets")
    ap.add_argument("--write-seconds", type=int, default=1200,
                    help="how long the first one gets, since it writes the drive first")
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
        # a boot that says nothing at all after the firmware hands over is the hang this vm has
        # about once in five boots, and it is not an answer to anything. It is tried again, and the
        # drive is written only the first time
        for attempt in range(1, 4):
            written = words is None and attempt == 1
            if written:
                cmd = first
            else:
                cmd = [base[0], "--image", kept] + base[1:]
                if words:
                    cmd += ["-smbios",
                            f"type=11,value=io.systemd.stub.kernel-cmdline-extra={words}"]
            print(f"\n==== {name}, try {attempt}\n==== {' '.join(cmd)}", flush=True)
            child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace",
                                  dimensions=(40, 160))
            child.logfile_read = Tee()
            start = time.monotonic()
            said = "nothing"
            seconds = args.write_seconds if written else args.seconds
            try:
                which = child.expect([PROMPT, PASSPHRASE, EMERGENCY, TIMEDOUT], timeout=seconds)
                if which == 1 and words is None:
                    child.send(open(args.passfile).read() + "\r")
                    child.expect([PROMPT], timeout=args.seconds)
                    said = "a shell, after the passphrase"
                else:
                    said = ["a shell", "the passphrase prompt", "emergency mode",
                            "a device that timed out"][which]
            except pexpect.TIMEOUT:
                said = f"nothing in {seconds} s"
            except pexpect.EOF:
                said = "qemu ended"
            print(f"\n==== {name}: {said} after {time.monotonic() - start:.0f}s", flush=True)
            if said.startswith("a shell"):
                child.send("sudo systemctl poweroff\r")
                try:
                    child.expect(pexpect.EOF, timeout=120)
                except pexpect.TIMEOUT:
                    pass
            child.terminate(force=True)
            time.sleep(2)
            if not said.startswith("nothing"):
                break
        found[name] = said

    print("\n==== how far each one got ====")
    for name, said in found.items():
        print(f"{name}: {said}")


if __name__ == "__main__":
    main()
