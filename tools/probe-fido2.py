#!/usr/bin/env python3
"""Probe: what systemd-cryptenroll does about a security key when there is no security key.

No FIDO2 token exists in a virtual machine and none can be made, so P2.9 part 2 has to be built
against the refusals rather than against an enrollment. This boots the image the last run of main
built, unlocks persist with its passphrase and asks, from the shell:

  1. what --fido2-device=list prints and exits with when nothing is plugged in
  2. whether it needs root to say it
  3. what --fido2-device=auto does, and whether it touches the header before it gives up
  4. what --wipe-slot= says about a slot that is not there
  5. whether SYSTEMD_EMOJI=0 takes the lock emoji off a systemd prompt
  6. that the passphrase still opens persist after all of it

Nothing here is a test of Rift. It answers questions, prints PROBE lines and exits 0 when it got
through.
"""

import argparse
import os
import re
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vm")
    ap.add_argument("image")
    ap.add_argument("passfile")
    ap.add_argument("--log", default="fido2-probe.log")
    ap.add_argument("--timeout", type=int, default=900)
    args = ap.parse_args()

    with open(args.passfile, encoding="utf-8") as f:
        passphrase = f.read().strip()

    cmd = [
        os.path.abspath(args.vm),
        "--image", os.path.abspath(args.image),
        "--persist", os.path.abspath(args.passfile),
        "-smp", "2",
        "-m", "2048",
        "-display", "none",
        "-monitor", "none",
        "-serial", "stdio",
        "-no-reboot",
        "-nic", "none",
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

    def one_line(text):
        return text.replace("\n", " | ")

    def unlock():
        child.send(passphrase + "\r")
        for attempt in range(3):
            if expect([PROMPT, PASSPHRASE], "the autologin shell") == 0:
                return
            if attempt == 2:
                fail("the passphrase was refused three times")
            child.send(passphrase + "\r")

    expect([PASSPHRASE], "the luks passphrase prompt")
    print(f"probe: passphrase prompt at {since()}", flush=True)
    unlock()
    print(f"probe: shell at {since()}", flush=True)

    _, output = run("sudo systemd-analyze --version 2>&1 | head -n1", "the systemd version")
    say("systemd", output)

    # 1 and 2. what list says, as root and as the owner
    status, output = run("sudo systemd-cryptenroll --fido2-device=list 2>&1", "the fido2 devices as root")
    say("list as root", f"status={status} {one_line(output)!r}")
    status, output = run("systemd-cryptenroll --fido2-device=list 2>&1", "the fido2 devices as the owner")
    say("list as the owner", f"status={status} {one_line(output)!r}")
    status, output = run("sudo systemd-cryptenroll --fido2-device=list 2>/dev/null", "list on stdout alone")
    say("list, stdout only", f"status={status} {one_line(output)!r}")
    status, output = run("sudo systemd-cryptenroll --fido2-device=list 2>&1 >/dev/null", "list on stderr alone")
    say("list, stderr only", f"status={status} {one_line(output)!r}")

    # the hidraw devices the machine has, which is what a token would come up as
    _, output = run("ls /dev/hidraw* 2>&1", "the hidraw devices")
    say("hidraw devices", one_line(output))

    # the token plugin that would unseal it in the initrd
    _, output = run("ls /run/current-system/sw/lib/cryptsetup 2>/dev/null; "
                    "ls (dirname (readlink -f (which systemd-cryptenroll)))/../lib/cryptsetup 2>&1",
                    "the cryptsetup token plugins")
    say("token plugins", one_line(output))
    _, output = run("echo path=$CRYPTSETUP_TOKEN_PATH", "the token path in the environment")
    say("CRYPTSETUP_TOKEN_PATH in a shell", one_line(output))

    # 3. the enrollment, with a key file so the order of what it does is visible
    _, output = run(f"sudo cryptsetup luksDump {PERSIST} 2>&1 | "
                    "grep -iE '^Tokens|^Keyslots|^  [0-9]+:' | head -n 20", "the header before")
    say("header before", one_line(output))

    run(f"printf '%s' '{passphrase}' | sudo tee /run/probe.key >/dev/null && sudo chmod 600 /run/probe.key",
        "the key file")
    status, output = run(
        f"sudo systemd-cryptenroll --unlock-key-file=/run/probe.key --fido2-device=auto {PERSIST} 2>&1",
        "the enrollment with no key plugged in")
    say("enroll --fido2-device=auto", f"status={status} {one_line(output)!r}")
    status, output = run(
        f"sudo systemd-cryptenroll --unlock-key-file=/run/probe.key --fido2-device=auto "
        f"--fido2-with-client-pin=yes --fido2-with-user-presence=yes {PERSIST} 2>&1",
        "the enrollment with the pin and presence asked for")
    say("enroll with pin and presence", f"status={status} {one_line(output)!r}")
    status, output = run(f"sudo systemd-cryptenroll --unlock-key-file=/run/probe.key "
                         f"--fido2-device=/dev/hidraw9 {PERSIST} 2>&1",
                         "the enrollment naming a device that is not there")
    say("enroll a named device that is not there", f"status={status} {one_line(output)!r}")

    _, output = run(f"echo fido2=(sudo cryptsetup luksDump --dump-json-metadata {PERSIST} | "
                    "grep -o systemd-fido2 | count) tpm2=(sudo cryptsetup luksDump "
                    f"--dump-json-metadata {PERSIST} | grep -o systemd-tpm2 | count)",
                    "the tokens after the refusals")
    say("tokens after the refusals", one_line(output))
    _, output = run(f"sudo cryptsetup luksDump {PERSIST} 2>&1 | "
                    "grep -iE '^Tokens|^Keyslots|^  [0-9]+:' | head -n 20", "the header after the refusals")
    say("header after the refusals", one_line(output))

    # 4. a wipe of a slot that is not there, and of a token type that is not there
    status, output = run(f"sudo systemd-cryptenroll --wipe-slot=7 {PERSIST} 2>&1", "the wipe of an empty slot")
    say("wipe-slot=7 with nothing in it", f"status={status} {one_line(output)!r}")
    status, output = run(f"sudo systemd-cryptenroll --wipe-slot=fido2 {PERSIST} 2>&1",
                         "the wipe of a fido2 slot that is not there")
    say("wipe-slot=fido2 with none enrolled", f"status={status} {one_line(output)!r}")

    # 5. the emoji in a systemd prompt. --timeout ends it by itself, so nothing waits for a person
    status, output = run("systemd-ask-password --timeout=3 'Probe with the emoji:' 2>&1 | cat -v", "a prompt")
    say("systemd-ask-password", f"status={status} {one_line(output)!r}")
    status, output = run("SYSTEMD_EMOJI=0 systemd-ask-password --timeout=3 'Probe with no emoji:' 2>&1 | cat -v",
                         "a prompt with the emoji off")
    say("systemd-ask-password, SYSTEMD_EMOJI=0", f"status={status} {one_line(output)!r}")

    # 6. the rule that never bends
    status, output = run(f"printf '%s' '{passphrase}' | sudo cryptsetup open --test-passphrase "
                         f"--key-file - {PERSIST} 2>&1; echo rc=$status",
                         "the passphrase after all of it")
    say("passphrase still opens persist", f"status={status} {one_line(output)!r}")

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
