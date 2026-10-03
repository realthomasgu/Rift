#!/usr/bin/env python3
"""Probe: boot an esp holding one two profile uki three ways and say what the firmware did.

1. nothing typed: which profile systemd-boot picks by itself.
2. space held from the start, then return: whether a hidden menu comes up, what its entries are
   called, which one it starts on, and a picture of it.
3. the number 2 pressed from the start: whether systemd-boot boots the second entry outright.

The answer in each case is the kernel's own "Command line:" line, which names the profile the stub
handed it, and for 2 the picture of the menu.
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


def key(qmp_path, name, code):
    try:
        qmp(qmp_path, {"execute": "send-key", "arguments": {"keys": [{"type": "qcode", "data": code}]}})
        return True
    except Exception as e:  # the socket is not up yet, or qemu has gone
        print(f"{name}: send-key {code}: {e}", flush=True)
        return False


def run(args, name, hold=None, held=0.0, press=None, shot=None, shot_at=0.0, seconds=90):
    """Boot once and return what the serial console said.

    hold is a key sent every 100 ms for the first held seconds, the way a person holds one down.
    press is a list of (seconds from the start, key) sent once each. shot is a ppm of the screen
    taken shot_at seconds in.
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
        "-drive", f"if=none,id=esp,format=raw,file={os.path.abspath(args.esp)}",
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

    def typing():
        while hold and time.monotonic() - start < held:
            key(qmp_path, name, hold)
            time.sleep(0.1)
        for at, code in press or []:
            while time.monotonic() - start < at:
                time.sleep(0.1)
            print(f"{name}: pressing {code} at {time.monotonic() - start:.0f}s", flush=True)
            key(qmp_path, name, code)

    def picture():
        while time.monotonic() - start < shot_at:
            time.sleep(0.1)
        if qmp(qmp_path, {"execute": "screendump", "arguments": {"filename": os.path.abspath(shot)}}):
            print(f"{name}: a picture of the screen at {time.monotonic() - start:.0f}s", flush=True)

    threading.Thread(target=typing, daemon=True).start()
    if shot:
        threading.Thread(target=picture, daemon=True).start()

    said = []
    while time.monotonic() - start < seconds:
        line = child.stdout.readline()
        if not line:
            break
        line = line.decode("utf-8", "replace").rstrip("\r\n")
        said.append(line)
        print(f"{name}| {line}", flush=True)
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
    ap.add_argument("--esp", required=True)
    ap.add_argument("--log", default="ghost-probe.log")
    ap.add_argument("--shot", default="ghost-probe-menu.ppm")
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

    found = {}
    found["nothing typed"] = answer(run(args, "plain", seconds=60))
    # the menu comes up, a picture is taken, and return starts whichever entry it began on
    found["space held, then return"] = answer(
        run(args, "menu", hold="spc", held=8, press=[(20, "ret")], shot=args.shot, shot_at=16, seconds=90)
    )
    found["2 pressed"] = answer(run(args, "second", hold="2", held=8, seconds=60))

    print("\n==== what the firmware did ====")
    for what, line in found.items():
        which = "?" if not line else "ghost" if "rift.probe=ghost" in line else "base" if "rift.probe=base" in line else "neither"
        print(f"{what}: profile {which}")
        print(f"  {line}")
    log.flush()


if __name__ == "__main__":
    main()
