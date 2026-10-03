#!/usr/bin/env python3
"""Probe: how a person, and a test, pick the second entry of a two profile uki.

Holding a key down from the start of a boot does not work: the firmware's boot stage reads the
keyboard too, and by the time systemd-boot polls for one the buffer is empty. What the firmware
does print on the serial console is `BdsDxe: starting Boot...` a fraction of a second before it
starts the loader, so that line is the anchor: the key goes in the moment it appears.

Two esps, one with `timeout 0` (the menu hidden, which is what the image ships) and one with
`timeout 3` (a menu at every boot). Each is booted with the keys that should pick the second entry.
The answer is the kernel's own "Command line:" line, which names the profile the stub handed it,
and a picture of the screen while the menu is up.
"""

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time

# the kernel prints this before it looks for an init, so it is the one line every run gets
CMDLINE = re.compile(r"Command line: (.*)")
# and the firmware prints this a fraction of a second before it starts the loader on the esp
HANDOFF = re.compile(r"BdsDxe: starting Boot")


def qmp(path, *commands):
    """Run monitor commands over the qmp socket and return their replies."""
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(30)
    sock.connect(path)
    buf = b""
    replies = []

    def read_reply():
        nonlocal buf
        while True:
            while b"\n" in buf:
                line, buf = buf.split(b"\n", 1)
                if not line.strip():
                    continue
                msg = json.loads(line)
                if "return" in msg or "error" in msg or "QMP" in msg:
                    return msg
            chunk = sock.recv(65536)
            if not chunk:
                raise RuntimeError("qmp socket closed")
            buf += chunk

    read_reply()  # the greeting
    for command in ({"execute": "qmp_capabilities"},) + commands:
        sock.sendall(json.dumps(command).encode() + b"\n")
        reply = read_reply()
        if "error" in reply:
            raise RuntimeError(f"qmp {command['execute']}: {reply['error']}")
        replies.append(reply["return"])
    sock.close()
    return replies[1:]


def key(qmp_path, name, code, times=1):
    for _ in range(times):
        try:
            qmp(qmp_path, {"execute": "send-key", "arguments": {"keys": [{"type": "qcode", "data": code}]}})
        except Exception as e:
            print(f"{name}: send-key {code}: {e}", flush=True)
            return False
    return True


def run(args, name, esp, bursts, shot=None, shot_after=2.0, seconds=90):
    """Boot an esp once and return what the serial console said.

    bursts is a list of (seconds after the firmware hands the loader the machine, key, how many
    times) sent in order.
    """
    work = os.path.abspath(f"probe-{name}")
    os.makedirs(work, exist_ok=True)
    qmp_path = os.path.join(work, "qmp.sock")
    vars_fd = os.path.join(work, "vars.fd")
    for path in (qmp_path, vars_fd):
        if os.path.exists(path):
            os.remove(path)
    subprocess.run(["cp", os.path.join(args.firmware, "OVMF_VARS.fd"), vars_fd], check=True)
    os.chmod(vars_fd, 0o644)
    cmd = [
        args.qemu,
        "-machine", "q35",
        "-smp", "2",
        "-m", "2048",
        "-drive", f"if=pflash,format=raw,readonly=on,file={os.path.join(args.firmware, 'OVMF_CODE.fd')}",
        "-drive", f"if=pflash,format=raw,file={vars_fd}",
        "-drive", f"if=none,id=esp,format=raw,file={os.path.abspath(esp)}",
        "-device", "nvme,drive=esp,serial=esp",
        "-device", "virtio-vga",
        "-display", "none",
        "-monitor", "none",
        "-serial", "stdio",
        "-no-reboot",
        "-nic", "none",
        "-qmp", f"unix:{qmp_path},server,nowait",
    ]
    cmd += ["-accel", "kvm", "-cpu", "host"] if os.access("/dev/kvm", os.W_OK) else ["-accel", "tcg", "-cpu", "max"]
    print(f"\n==== {name}: {' '.join(cmd)}", flush=True)
    child = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    start = time.monotonic()
    handoff = threading.Event()
    said = []

    def reading():
        while True:
            line = child.stdout.readline()
            if not line:
                return
            said.append(line.decode("utf-8", "replace").rstrip("\r\n"))
            print(f"{name}| {said[-1]}", flush=True)
            if HANDOFF.search(said[-1]):
                handoff.set()

    threading.Thread(target=reading, daemon=True).start()

    def typing():
        if not handoff.wait(60):
            print(f"{name}: the firmware never said it was starting the loader", flush=True)
            return
        began = time.monotonic()
        print(f"{name}: the firmware started the loader at {began - start:.1f}s", flush=True)
        for at, code, times in bursts:
            while time.monotonic() - began < at:
                time.sleep(0.02)
            if key(qmp_path, name, code, times):
                print(f"{name}: {code} x{times} at {time.monotonic() - began:.2f}s after the handoff", flush=True)
        if shot:
            while time.monotonic() - began < shot_after:
                time.sleep(0.05)
            try:
                qmp(qmp_path, {"execute": "screendump", "arguments": {"filename": os.path.abspath(shot)}})
                print(f"{name}: {shot} at {time.monotonic() - began:.1f}s after the handoff", flush=True)
            except Exception as e:
                print(f"{name}: screendump {shot}: {e}", flush=True)

    threading.Thread(target=typing, daemon=True).start()

    while time.monotonic() - start < seconds:
        if child.poll() is not None:
            break
        if any(CMDLINE.search(line) for line in list(said)):
            time.sleep(3)
            break
        time.sleep(0.5)
    child.kill()
    child.wait()
    print(f"{name}: ended after {time.monotonic() - start:.0f}s", flush=True)
    return said


def answer(said):
    for line in said:
        found = CMDLINE.search(line)
        if found:
            return found.group(1)
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--qemu", required=True)
    ap.add_argument("--firmware", required=True, help="the FV directory with OVMF_CODE.fd and OVMF_VARS.fd")
    ap.add_argument("--esp", required=True, help="an esp whose loader.conf says timeout 0")
    ap.add_argument("--esp-timeout", required=True, help="the same esp whose loader.conf says timeout 3")
    ap.add_argument("--log", default="ghost-probe.log")
    args = ap.parse_args()

    log = open(args.log, "w", encoding="utf-8")

    class Tee:
        def write(self, text):
            sys.__stdout__.write(text)
            log.write(text)

        def flush(self):
            sys.__stdout__.flush()
            log.flush()

    sys.stdout = Tee()

    # the space bar as soon as the loader has the machine, then a step down and return. the picture
    # is taken while the menu is still waiting for the return
    keys = [(0.0, "spc", 20), (1.5, "down", 1), (3.0, "ret", 1)]
    found = {}
    found["timeout 0, space on the handoff"] = answer(
        run(args, "hidden", args.esp, keys, shot="ghost-probe-hidden.ppm", shot_after=2.2)
    )
    found["timeout 3, space on the handoff"] = answer(
        run(args, "timeout", args.esp_timeout, keys, shot="ghost-probe-menu.ppm", shot_after=2.2)
    )
    # and the same esp with a timeout, left alone: the menu has to come up and then boot the first
    found["timeout 3, nothing typed"] = answer(run(args, "alone", args.esp_timeout, [], seconds=60))

    print("\n==== what the firmware did ====")
    for what, line in found.items():
        which = "?" if not line else "ghost" if "rift.probe=ghost" in line else "base" if "rift.probe=base" in line else "neither"
        print(f"{what}: profile {which}")
        print(f"  {line}")
    log.flush()


if __name__ == "__main__":
    main()
