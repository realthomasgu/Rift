#!/usr/bin/env python3
"""Probe: what a ghost boot of the image main already built says about itself today.

The drive is written once and booted ordinarily, so persist exists and the first boot's questions
are answered, then booted again as a ghost boot. The ghost words go in through systemd-stub's
SMBIOS string, so nothing is rebuilt, and they are exactly what nix/image/ghost.nix puts on the
ghost profile's command line.

What it asks the ghost session is below in QUESTIONS: which units failed, whether Orbit, Quasar and
Vault are on the bus at all, what the rift command says, what the shell and Settings print about
themselves, and whether the drive's own exchange partition shows up as a disk a person could mount.
Every answer is printed with its question, so one run is a page of what part 2 has to change.
"""

import argparse
import os
import re
import sys
import time

import pexpect

PROMPT = r"rift(\x1b\[[0-9;]*m)*@(\x1b\[[0-9;]*m)*rift"
PASSPHRASE = r"(?i)passphrase[^\r\n]*:"
EMERGENCY = r"Reached target emergency\.target|Emergency Mode"
# the markers fish writes around a command, so an answer can be read without the prompt
COMMAND_START = r"\x1b\]133;C[^\x07\x1b]*(?:\x07|\x1b\\)"
COMMAND_END = r"\x1b\]133;D;(\d+)(?:\x07|\x1b\\)"
ESCAPES = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-9;?>=]*[A-Za-z]|\x1b[=>]")
# the line the firmware prints a fraction of a second before it starts the loader on the esp, and
# how a vm that never started is told from one that did
HANDOFF = r"BdsDxe: starting Boot"
BOOTING = r"rift-vm: booting \S+ as an nvme drive"

# the words that make a boot a ghost one, from nix/image/ghost.nix
PERSIST = [
    "persist",
    "home",
    "var",
    "var-lib-flatpak",
    "var-lib-rift-models",
    "var-lib-rift-hosts",
]
GHOST = "rift.ghost rd.luks=0 " + " ".join(f"systemd.mask={name}.mount" for name in PERSIST)

# what to ask the ghost session, in order. each is a command line for the serial shell
QUESTIONS = [
    "cat /proc/cmdline",
    "systemctl --failed --no-pager --no-legend | cat",
    "systemctl is-active orbit.service quasar.service vault.service vault-owner.service | cat",
    "systemctl show -p Result -p ActiveState -p ConditionResult orbit.service quasar.service vault.service | cat",
    "journalctl -b --no-pager -o cat -u orbit.service -u quasar.service -u vault.service -n 40 | cat",
    "busctl list --no-pager --acquired | grep -i rift | cat",
    "rift doctor; echo status=$status",
    "rift host; echo status=$status",
    "rift ai --help | head -20; echo status=$status",
    "rift ai ask 'hello'; echo status=$status",
    "rift snapshot list; echo status=$status",
    "rift backup list; echo status=$status",
    "rift update; echo status=$status",
    "rift session; echo status=$status",
    "rift --version",
    "ls -a /home/rift | cat",
    "lsblk -o NAME,LABEL,PARTLABEL,SIZE,MOUNTPOINT | cat",
    "ps -eo comm | sort -u | grep -iE 'welcome|lens|horizon|quasar|orbit|vault' | cat",
    "lens --state; echo status=$status",
    "systemd-run --user --quiet --collect rift-settings; sleep 25; rift-settings --state; echo status=$status",
    "rift-settings --page owner; sleep 5; rift-settings --state | head -40; echo status=$status",
    "systemd-run --user --quiet --collect rift-files; sleep 20; rift-files --state; echo status=$status",
    "rift-welcome --state; echo status=$status",
    "free -m | cat",
    "findmnt --real -o TARGET,SOURCE,FSTYPE | cat",
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vm")
    ap.add_argument("image")
    ap.add_argument("passfile")
    ap.add_argument("--log", default="ghost-probe.log")
    ap.add_argument("--seconds", type=int, default=420, help="how long a boot gets to reach a shell")
    ap.add_argument("--write-seconds", type=int, default=1500,
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
    passphrase = open(args.passfile).read().strip()
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
    first = [base[0], "--image", os.path.abspath(args.image), "--persist",
             os.path.abspath(args.passfile), "--drive", kept, "--exchange", "1G"] + base[1:]

    def spawn(cmd, what, seconds):
        """Start the vm and wait for the firmware to hand the loader the machine. A boot that says
        nothing at all is the hang this vm has about once in five boots, so it is started again."""
        for attempt in range(1, 4):
            print(f"\n==== {what}, try {attempt}\n==== {' '.join(cmd)}", flush=True)
            child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace",
                                  dimensions=(40, 200))
            child.logfile_read = Tee()
            try:
                child.expect([BOOTING], timeout=seconds)
                child.expect([HANDOFF], timeout=240)
                return child
            except (pexpect.TIMEOUT, pexpect.EOF):
                print(f"\n==== {what}: nothing from the firmware, starting it again", flush=True)
                child.terminate(force=True)
                time.sleep(2)
        print(f"\n==== {what}: never started", flush=True)
        sys.exit(1)

    def shell(child, what, ghost):
        """Wait for the autologin shell, answering the passphrase prompt when there is one."""
        which = child.expect([PROMPT, PASSPHRASE, EMERGENCY], timeout=args.seconds)
        if which == 2:
            print(f"\n==== {what}: emergency mode", flush=True)
            sys.exit(1)
        if which == 1:
            if ghost:
                print(f"\n==== {what}: asked for a passphrase, which a ghost boot must not",
                      flush=True)
                sys.exit(1)
            child.send(passphrase + "\r")
            child.expect([PROMPT], timeout=args.seconds)
        print(f"\n==== {what}: a shell", flush=True)

    def ask(child, command):
        """Run one command line in the serial shell and print what it said."""
        print(f"\n######## {command}", flush=True)
        child.send(command + "\r")
        try:
            child.expect([COMMAND_START], timeout=60)
            child.expect([COMMAND_END], timeout=240)
        except (pexpect.TIMEOUT, pexpect.EOF) as why:
            print(f"\n######## {command}: nothing came back ({type(why).__name__})", flush=True)
            return
        said = ESCAPES.sub("", child.before).replace("\r", "")
        print(f"\n######## {command} said:\n{said.strip()}", flush=True)

    def off(child):
        child.send("sudo systemctl poweroff\r")
        try:
            child.expect(pexpect.EOF, timeout=120)
        except pexpect.TIMEOUT:
            child.terminate(force=True)
        time.sleep(2)

    # the ordinary boot, which writes the drive and makes persist
    child = spawn(first, "the ordinary boot", args.write_seconds)
    shell(child, "the ordinary boot", ghost=False)
    ask(child, "findmnt -no SOURCE,FSTYPE /home")
    # let the desktop settle, so the ordinary session is the one the ghost one is compared with
    time.sleep(60)
    for command in ("rift doctor; echo status=$status", "lens --state; echo status=$status"):
        ask(child, command)
    off(child)

    # and the same drive as a ghost boot
    child = spawn([base[0], "--image", kept] + base[1:]
                  + ["-smbios", f"type=11,value=io.systemd.stub.kernel-cmdline-extra={GHOST}"],
                  "the ghost boot", args.seconds)
    shell(child, "the ghost boot", ghost=True)
    # the desktop takes a moment, and Settings and Files are started from the shell below
    time.sleep(90)
    for command in QUESTIONS:
        ask(child, command)
    off(child)
    print("\n==== done", flush=True)


if __name__ == "__main__":
    main()
