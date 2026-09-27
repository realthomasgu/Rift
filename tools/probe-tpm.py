#!/usr/bin/env python3
"""Probe: what a software tpm in the vm can do for persist.

Boots the image with a software tpm, unlocks persist with its passphrase and then asks, from the
shell, everything P2.9 needs to know before anything is built:

  1. the guest sees a tpm at all, and which driver it came up on
  2. systemd-cryptenroll lists the device and takes the passphrase out of the environment
  3. a slot sealed to pcr 7 goes into the header of persist
  4. systemd-cryptsetup opens a second mapping of persist with that slot and no passphrase
  5. the same slot still opens it after a reboot, so the tpm's state and pcr 7 are stable
  6. what pcr 7 reads, before and after the reboot

Nothing here is a test of Rift. It answers questions, prints PROBE lines and exits 0 when it got
through. It works against an image that knows nothing about the tpm, so no image is built for it.
"""

import argparse
import os
import re
import socket
import sys
import time

import pexpect

PROMPT = r"rift(\x1b\[[0-9;]*m)*@(\x1b\[[0-9;]*m)*rift"
PASSPHRASE = r"(?i)passphrase[^\r\n]*:"
COMMAND_START = r"\x1b\]133;C[^\x07\x1b]*(?:\x07|\x1b\\)"
COMMAND_END = r"\x1b\]133;D;(\d+)(?:\x07|\x1b\\)"
ESCAPES = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-9;?>=]*[A-Za-z]|\x1b[=>]")
JOURNAL = re.compile(r"(?:^[ \t]*)?\[\s*\d+\.\d+\] [^\n]*\n{0,2}", re.M)
PERSIST = "/dev/disk/by-partlabel/persist"


def clean(output):
    return JOURNAL.sub("", ESCAPES.sub("", output).replace("\r", "")).strip()


def qmp(path, *commands):
    """Send commands to the qemu monitor and return the last reply."""
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(30)
        s.connect(path)
        f = s.makefile("rwb")
        f.readline()
        reply = None
        for command in ({"execute": "qmp_capabilities"},) + commands:
            import json

            f.write((json.dumps(command) + "\n").encode())
            f.flush()
            while True:
                line = f.readline()
                if not line:
                    raise RuntimeError("the monitor closed")
                answer = json.loads(line)
                if "return" in answer or "error" in answer:
                    reply = answer
                    break
        return reply


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vm")
    ap.add_argument("image")
    ap.add_argument("passfile")
    ap.add_argument("--tpm", required=True, help="directory for the software tpm's state")
    ap.add_argument("--log", default="tpm-probe.log")
    ap.add_argument("--timeout", type=int, default=900)
    args = ap.parse_args()

    with open(args.passfile, encoding="utf-8") as f:
        passphrase = f.read().strip()

    qmp_path = os.path.abspath("probe-qmp.sock")
    cmd = [
        os.path.abspath(args.vm),
        "--image", os.path.abspath(args.image),
        "--persist", os.path.abspath(args.passfile),
        "--tpm", os.path.abspath(args.tpm),
        "-smp", "2",
        "-m", "2048",
        "-display", "none",
        "-monitor", "none",
        "-serial", "stdio",
        "-no-reboot",
        "-nic", "none",
        "-qmp", f"unix:{qmp_path},server,nowait",
    ]
    print("probe: " + " ".join(cmd), flush=True)

    start = time.monotonic()
    deadline = start + args.timeout
    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", dimensions=(40, 160))
    log = open(args.log, "w", encoding="utf-8")
    child.logfile_read = log

    def since():
        return f"{time.monotonic() - start:.0f}s"

    def fail(why):
        print(f"\nprobe: FAILED after {since()}: {why}", flush=True)
        log.flush()
        child.terminate(force=True)
        sys.exit(1)

    def expect(patterns, what):
        try:
            return child.expect(patterns, timeout=max(1, deadline - time.monotonic()))
        except pexpect.TIMEOUT:
            fail(f"timed out waiting for {what}")
        except pexpect.EOF:
            fail(f"qemu exited while waiting for {what}")

    def run(command, what):
        child.send(command + "\r")
        expect([COMMAND_START], f"the shell to start {what}")
        expect([COMMAND_END], what)
        return int(child.match.group(1)), clean(child.before)

    def say(question, answer):
        print(f"PROBE {question}: {answer}", flush=True)

    def unlock():
        child.send(passphrase + "\r")
        for attempt in range(3):
            if expect([PROMPT, PASSPHRASE], "the autologin shell") == 0:
                return
            if attempt == 2:
                fail("the passphrase was refused three times")
            child.send(passphrase + "\r")

    # 1. the tpm
    expect([PASSPHRASE], "the luks passphrase prompt")
    print(f"probe: passphrase prompt at {since()}", flush=True)
    unlock()
    print(f"probe: shell at {since()}", flush=True)

    status, output = run("ls /sys/class/tpm/ 2>&1; echo rc=$status", "the tpm in sysfs")
    say("tpm in sysfs", output.replace("\n", " | "))
    _, output = run("cat /sys/class/tpm/tpm0/tpm_version_major 2>&1", "the tpm version")
    say("tpm version major", output)
    _, output = run("lsmod | grep -i tpm", "the tpm modules")
    say("tpm modules", output.replace("\n", " | ") or "none")

    # 2. systemd-cryptenroll sees it
    status, output = run("sudo systemd-cryptenroll --tpm2-device=list 2>&1", "the tpm devices cryptenroll lists")
    say("cryptenroll --tpm2-device=list", f"status={status} {output.replace(chr(10), ' | ')}")

    _, output = run("sudo systemd-analyze --version 2>&1 | head -n1", "the systemd version")
    say("systemd", output)

    # what the header holds before anything is enrolled
    _, output = run(f"sudo cryptsetup luksDump {PERSIST} 2>&1 | grep -iE 'token|keyslot|version' | head -n 20",
                    "the header before")
    say("header before", output.replace("\n", " | "))

    # 3. enroll a slot sealed to pcr 7. the key file is the way vault would do it: the passphrase
    # as its exact bytes, no newline, which is what cryptsetup was given when persist was made
    run(f"printf '%s' '{passphrase}' | sudo tee /run/probe.key >/dev/null && sudo chmod 600 /run/probe.key",
        "the key file")
    status, output = run(
        f"sudo systemd-cryptenroll --unlock-key-file=/run/probe.key --tpm2-device=auto --tpm2-pcrs=7 {PERSIST} 2>&1",
        "the enrollment with a key file")
    say("enroll pcr 7 with --unlock-key-file", f"status={status} {output.replace(chr(10), ' | ')}")
    if status != 0:
        # and the environment, which the man page also documents, in case the key file is not it
        status, output = run(
            f"sudo PASSWORD='{passphrase}' systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 {PERSIST} 2>&1",
            "the enrollment with PASSWORD")
        say("enroll pcr 7 with $PASSWORD", f"status={status} {output.replace(chr(10), ' | ')}")

    status, output = run(
        f"sudo cryptsetup luksDump --dump-json-metadata {PERSIST} 2>&1 | grep -o 'systemd-tpm2' | head -n1",
        "the token in the header")
    say("token in the header", output or "none")
    _, output = run(f"sudo cryptsetup luksDump {PERSIST} 2>&1 | grep -A3 -iE '^Tokens|^  [0-9]+: systemd' | head -n 12",
                    "the tokens")
    say("tokens", output.replace("\n", " | "))

    # 4. the unseal, in userspace: the same token plugin the initrd uses
    status, output = run(
        f"sudo systemd-cryptsetup attach probeunseal {PERSIST} - tpm2-device=auto,headless 2>&1",
        "the unseal")
    say("unseal", f"status={status} {output.replace(chr(10), ' | ')}")
    _, output = run("ls -l /dev/mapper/probeunseal 2>&1", "the mapping the unseal made")
    say("unseal mapping", output.replace("\n", " | "))
    run("sudo systemd-cryptsetup detach probeunseal 2>&1", "the detach")

    # 6. pcr 7 before the reboot
    _, output = run("sudo cat /sys/class/tpm/tpm0/pcr-sha256/7 2>&1", "pcr 7")
    say("pcr 7 before the reboot", output)
    _, output = run("sudo cat /sys/class/tpm/tpm0/pcr-sha256/11 2>&1", "pcr 11")
    say("pcr 11 before the reboot", output)

    # 5. and after a reboot, in the same qemu with the same tpm state
    try:
        qmp(qmp_path, {"execute": "set-action", "arguments": {"reboot": "reset"}})
    except (OSError, RuntimeError) as e:
        fail(f"qmp set-action: {e}")
    child.send("sudo systemctl reboot\r")
    expect([PASSPHRASE], "the passphrase prompt of the second boot")
    print(f"probe: second boot asks for the passphrase at {since()}", flush=True)
    say("second boot", "asks for the passphrase, which is right: this image's crypttab has no tpm2-device")
    unlock()

    _, output = run("sudo cat /sys/class/tpm/tpm0/pcr-sha256/7 2>&1", "pcr 7 again")
    say("pcr 7 after the reboot", output)
    status, output = run(
        f"sudo systemd-cryptsetup attach probeunseal {PERSIST} - tpm2-device=auto,headless 2>&1",
        "the unseal after the reboot")
    say("unseal after the reboot", f"status={status} {output.replace(chr(10), ' | ')}")
    run("sudo systemd-cryptsetup detach probeunseal 2>&1", "the detach")

    # and what a wipe does
    status, output = run(f"sudo systemd-cryptenroll --wipe-slot=tpm2 {PERSIST} 2>&1", "the wipe")
    say("wipe tpm2 slot", f"status={status} {output.replace(chr(10), ' | ')}")
    status, output = run(
        f"sudo cryptsetup luksDump --dump-json-metadata {PERSIST} 2>&1 | grep -c 'systemd-tpm2'",
        "the token after the wipe")
    say("tokens after the wipe", output)

    # the passphrase still opens it, which is the rule that never bends
    status, output = run(f"printf '%s' '{passphrase}' | sudo cryptsetup open --test-passphrase {PERSIST} 2>&1",
                         "the passphrase after all of it")
    say("passphrase still opens persist", f"status={status} {output.replace(chr(10), ' | ')}")

    print(f"\nprobe: through at {since()}", flush=True)
    child.send("sudo systemctl poweroff\r")
    try:
        child.expect(pexpect.EOF, timeout=90)
    except pexpect.TIMEOUT:
        child.terminate(force=True)
    log.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main())
