#!/usr/bin/env python3
"""Boots an image through the flake's vm app and checks that the system comes up. The image job in ci
runs this after it builds the image.

Usage: boot-test.py <rift-vm> <image> <passfile> [--models dir] [--exchange size] [--timeout 600]
       [--log serial.log] [--splash splash.png] [--desktop desktop.png] [--lens] [--updates updates.img]
       [--backup backup.img] [--clone clone.img] [--flatpak dir] [--first-boot] [--offline png]
       [--boot-style]

With --first-boot rift-flash writes the drive without persist, the way it writes one on macOS and
Windows, and the drive makes persist when it first starts. The test answers its questions over serial: a
passphrase that is too short and two that differ are asked for again, then the passphrase from the
passfile goes in twice. The drive goes on to the shell without asking again, and gets the checks below
up to the drive's own: the slots, luks2 with argon2id, the subvolumes, the owner's home and the exchange
partition. Persist has one key slot and the system runs with the machine id in @var. After a reboot the
drive asks systemd-cryptsetup's question, not the first boot's, and opens the same persist with the same
passphrase: the same uuids, machine id, key slots and partitions. The test ends there.

With --offline as well the vm has no network card. The session starts Welcome on the new drive, and it
opens on the page that says there is no network, whose screendump is saved as the png --offline names.
Open later closes it without writing the note that the drive has been welcomed, and after the reboot
Welcome opens on the same page again.

<rift-vm> is the program from `nix build .#vm` (result/bin/rift-vm). It writes the image, .raw or
.raw.zst, onto a drive in a file with rift-flash (through sudo), with the passphrase from the passfile
for persist, then boots the drive as an nvme drive. Everything goes through the serial console: the luks prompt,
the autologin shell, a few commands, the default apps on the path, the a/b slots, the host profile
orbit wrote and what `rift host` and `rift doctor` print. The serial output is printed as it
arrives and kept in the log file.

The first tier of apps: every app, tool and language in the image prints its version. gcc, clang, g++,
clang++, cmake with ninja, make, zig cc, rustc and go each build a program that runs, java runs one from
its source, and python, node and bun run a line. JAVA_HOME is the jdk in the image. libvirtd is not
running after the boot, starts when `virsh -c qemu:///system version` connects as the owner and names the
QEMU it runs guests with, and has UEFI firmware with secure boot for them.

Timeline: the test takes a snapshot of home with `rift snapshot take`, changes one file and
deletes another, finds the snapshot through `rift snapshot` and on the bus, and restores both from
it. The deleted file comes back as the owner's; the changed one stays as it is without --replace and
is the copy from the snapshot with it. The hourly timer's service runs once and adds a snapshot. Then
`vault prune` with one hour, one day and two weeks drops the snapshots named by hand for January that
fall past those limits and keeps the one that does not.

Backup: with --backup the vm gets another drive, an empty ext4 file system labelled backup. The test
mounts it, chooses a folder on it with `sudo vault target`, which prints the password, and unmounts it
again. `rift backup now` backs up home; vault mounts the disk by its uuid by itself. One file is
changed and another deleted, and both come back from the backup through `rift backup restore` the
way they do from a snapshot. Then the test mounts the disk again: rustic refuses the repository with a
wrong password and opens it with the printed one, and grep finds the file's text in none of its files.

Clone: with --clone the vm gets an empty scsi disk that says it is removable. Last of all the test
writes a file to home and runs `sudo rift clone`. Vault refuses the drive the system runs from, the
backup drive, which is not removable, and a serial that is not the disk's, and writes nothing. Then it
clones onto the removable disk with a passphrase of its own. Slot a of the clone holds the running
version under the running slot's uuids and its store matches the usrhash, slot b is empty, the first
drive's passphrase does not open the clone's persist, and the first drive's header over the clone's
data reads as no file system, so the two volume keys differ. After the poweroff qemu starts again with
only the clone. Its luks prompt refuses the first drive's passphrase and takes the clone's, the file is
in home, and the clone boots the version it was made from, from its own esp and slot a, with a machine
id of its own and none of the first drive's snapshots.

With --boot-style the test sets the boot style from the Appearance page instead of the checks below.
The first boot draws the text splash, which is what a drive with no setting draws. Then Settings opens
in the session, `rift-settings --set boot graphical` writes the word onto the esp through Vault, the
test reads it back from /boot, and the vm reboots: the splash of that boot has to be the graphical one.
It sets the style back to text from the page the same way, reboots again, and that splash has to be the
text one. The two screendumps are saved beside the png --splash names. Before each reboot the Date and
time page sets a time zone, Pacific/Auckland and then UTC again, and the boot after it has to be in that
zone, which timedated keeps on persist. The Keyboard page adds English (UK) before the first reboot and
takes it off before the second: localed and horizon have it in the boot after the first, and localed
has English (US) alone after the second, which localed keeps on persist too. The test ends there.

The drive: the vm app writes it from the image into a sparse file with rift-flash, with an exchange
partition when --exchange gives its size. Persist has to be luks2 with argon2id, the settings a person
gets, with every subvolume and the owner's home, and the exchange partition an exfat labelled EXCHANGE
of that size. A clone of the drive gets an exchange partition of the same size.

The slots: systemd-boot started the uki with a boot counter in its name, the boot reached
boot-complete.target and the counter is gone, systemd-sysupdate lists the running version as
installed, /usr runs from slot a, and slot b's two partitions are there and empty.

With --updates the vm gets a second drive, an ext4 file system labelled updates with the update files
of two newer versions: next (nix build .#update) and broken (nix build .#broken-update), a version
after next whose boot check always fails. After the other checks the test mounts next where
systemd-sysupdate reads updates and installs that version: its store and verity partitions in slot b
with the uuids from the file names, its uki on the esp with three tries. The vm reboots and the slots
are checked again for the new version: systemd-boot started its uki, the boot was marked good,
sysupdate lists both versions with the new one current, and /usr runs from slot b.

Then the rollback. sysupdate installs broken over the oldest version, in slot a, and the vm boots it
three times. Each of those boots comes up to the shell, the check fails, nothing marks the boot good,
and systemd-boot has taken one more try off its uki: +2-1, +1-2, +0-3. The fourth boot runs next from
slot b again, and sysupdate still lists broken as installed.

With --models the files in that directory go into the @models subvolume before boot. The test waits
on the system bus until quasard has loaded the model it picked for orbit's tier, checks that it is the
one in the directory, asks the local api for a short completion and asks quasar a question over the bus
and through `rift ai`.
The local api has to refuse the same completion when the request comes with a web page's Origin or
Host header, and the owner must not reach llama-server's socket behind it. The voice in the manifest
says a sentence into a wav, which the test reads back as 22050 Hz mono speech.

With --splash the test also takes a screendump through the qemu monitor while the luks prompt, or the
first boot's first question, is up and checks that Liftoff's boot screen is on it. The text style, the
default, is crates/liftoff-splash on its near black: the logo's ice in the block at the top left where
the logo is drawn, and systemd's green OK in the console under it. With --style graphical the drive
boots with plymouth.splash=liftoff-graphical, which a SMBIOS string hands systemd-stub for the kernel
command line, and the screen has the mark from nix/liftoff/logo in the middle of the same near black. The dump is saved as a png. --splash-only ends the test when the shell is up after
the splash.

With --desktop the test checks that greetd is up and takes a screendump of the running session: horizon
paints its background gray over the whole screen, a console would show black with text. The vm has a
virtio gpu for this, horizon renders on it in software.

Before that, Welcome: the session starts it on a new drive, and it opens in the middle of the screen on
its start page. The test takes a screendump of every page, finds flathub among the system installation's
remotes on its Apps page, which flatpak adds from /etc/flatpak/remotes.d the first time anything uses that
installation, and presses Done, which writes the note and closes it. With --flatpak the test serves its
own signed repository later and adds it as a remote, Welcome opened again lists the test's app with its
size, and the test ticks it and presses Install: the install finishes, flatpak list has the app and its
runtime in the system installation, and the app is in the Applications menu.

With --lens the desktop check expects lens's bar along the top of that screen: horizon reports a
layer surface with its namespace, and the screendump has the bar gray with something drawn at its
left, in its middle and at its right, and the desktop gray below. `lens --state` prints what the bar
shows and the test compares its clock with `date` in the vm and its network with nmcli's. The test
then opens the Applications menu with `lens --menu`: under the field are the apps of the session in
their sections, typing an app's name with `lens --type` filters them, and `lens --enter` starts the
one that is selected, which horizon then lists as a window. The field takes lines from the serial
shell the same way: a nushell pipeline puts three rows under it, a command with arguments it does
not know puts a line under it, the first `lens --escape` clears the field and the second closes
the menu. After the lock screen's own checks a pointer click on the status icons opens the system
menu at the right of the bar: `lens --state` says the cable is connected and that the vm has no
wireless card, Bluetooth adapter, backlight or battery, a click on the volume slider changes what
`wpctl get-volume` reads, a second click on the icons closes the menu, Restart asks first and escape
says no, and Lock starts the lock screen, which the owner's password unlocks. Then notifications:
lens owns org.freedesktop.Notifications on the session bus, a critical one from notify-send stands
under the bar at the right in the screendump and stays until its close button closes it, a click on a
notification's button makes notify-send print the action's key, and one that is not critical goes
after five seconds and stays in the list. A click on the clock opens the clock menu, which lists it,
its Do not disturb switch keeps the next one off the screen, and a second click closes the menu. The
volume key sent over qmp turns the sink up and shows the key popup over the dock. Killing the shell
brings it back, since it is a user unit that restarts, and the apps it started keep their windows
through a restart, since each runs in a scope of its own. Then Firefox and Ghostty, started from the
dock, stand side by side between the bar and the dock, each with the title bar it draws itself and a close
button at its right. KeePassXC, the first Qt app, started from the Applications menu, stands there with
the Adwaita title bar Qt draws for it in dark. The everyday apps follow, one at a time from the same menu:
pictures, documents, video, sound, the calculator, archives, the disks, where the space went and the
characters, each with the title bar it draws itself. polkit refuses every action of the disk utility on a
disk the machine boots from, a file of each kind names the app that owns it, and the image viewer, given a
photograph, draws it in colour between the bars. The owner's theme set to light and the shell started again
make the bar, the dock, the desktop, the Applications menu, the lock screen and both apps light, and dark
again after that.
With --models as
well, a question goes through `lens --do`, which prints quasar's answer, and then into the field,
where the answer shows up as rows under it.
"""

import argparse
import base64
import collections
import functools
import glob
import http.server
import json
import math
import os
import re
import socket
import struct
import sys
import tempfile
import threading
import time
import tomllib
import zlib

import pexpect

# the fish prompt is user@host with colour codes in between
PROMPT = r"rift(\x1b\[[0-9;]*m)*@(\x1b\[[0-9;]*m)*rift"
PASSPHRASE = r"(?i)passphrase[^\r\n]*:"
# what vault-first-boot asks on a drive written without persist
CHOOSE = r"Choose a passphrase"
AGAIN = r"Type the passphrase again"
# fish marks every command line it runs: osc 133;C when it starts and 133;D;<status> when it is
# done. it also repaints the prompt whenever the journal writes to the console, so a prompt is not
# where a command's output ends, these marks are
COMMAND_START = r"\x1b\]133;C[^\x07\x1b]*(?:\x07|\x1b\\)"
COMMAND_END = r"\x1b\]133;D;(\d+)(?:\x07|\x1b\\)"
ESCAPES = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-9;?>=]*[A-Za-z]|\x1b[=>]")
# a line of the journal on the serial console, with the two line ends it comes with. it can land in
# the middle of a line a command prints, between two of its writes
JOURNAL = re.compile(r"(?:^[ \t]*)?\[\s*\d+\.\d+\] [^\n]*\n{0,2}", re.M)


def without_console(output):
    """What a command printed, without the journal's lines that reach the serial console while it
    runs and without blank lines at either end. Where a journal line cut a line of the command's in
    two, the two halves are one line again."""
    return JOURNAL.sub("", output).strip("\n")


def first(paths):
    for pattern in paths:
        found = sorted(glob.glob(pattern))
        if found:
            return found[0]
    return None


def version_key(version):
    """Sorts versions like 0.2.0 and 0.10.0 by their numbers."""
    return tuple(int(part) for part in version.split("."))


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


def pdf_bytes(pages):
    """A small PDF, one page for each list of lines. The streams are not compressed and the xref
    table holds every object, which is all a program that reads PDFs needs. The lines hold no
    brackets or backslashes, which a PDF string would take as its own."""
    objects = [(1, "<< /Type /Catalog /Pages 2 0 R >>")]
    font = 3 + len(pages) * 2
    kids = " ".join(f"{3 + at * 2} 0 R" for at in range(len(pages)))
    objects.append((2, f"<< /Type /Pages /Kids [{kids}] /Count {len(pages)} >>"))
    for at, lines in enumerate(pages):
        page = 3 + at * 2
        drawn = ("BT /F1 12 Tf 72 720 Td 16 TL\n"
                 + "".join(f"({line}) Tj T*\n" for line in lines) + "ET")
        objects.append((page, f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
                              f"/Resources << /Font << /F1 {font} 0 R >> >> "
                              f"/Contents {page + 1} 0 R >>"))
        objects.append((page + 1, f"<< /Length {len(drawn)} >>\nstream\n{drawn}\nendstream"))
    objects.append((font, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"))
    out = bytearray(b"%PDF-1.4\n")
    offsets = {}
    for number, body in objects:
        offsets[number] = len(out)
        out += f"{number} 0 obj\n{body}\nendobj\n".encode("ascii")
    started = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode("ascii")
    for number in range(1, len(objects) + 1):
        out += f"{offsets[number]:010d} 00000 n \n".encode("ascii")
    out += (f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
            f"startxref\n{started}\n%%EOF\n").encode("ascii")
    return bytes(out)


def read_ppm(path):
    """Parse a binary ppm (P6) into (width, height, bytes of rgb triples)."""
    data = open(path, "rb").read()
    fields = []
    pos = 0
    while len(fields) < 4:
        while data[pos : pos + 1].isspace():
            pos += 1
        if data[pos : pos + 1] == b"#":
            pos = data.index(b"\n", pos)
            continue
        end = pos
        while not data[end : end + 1].isspace():
            end += 1
        fields.append(data[pos:end])
        pos = end
    pos += 1
    if fields[0] != b"P6" or fields[3] != b"255":
        raise RuntimeError(f"unexpected ppm header {fields}")
    width, height = int(fields[1]), int(fields[2])
    return width, height, data[pos : pos + width * height * 3]


def write_png(path, width, height, rgb):
    raw = bytearray()
    stride = width * 3
    for y in range(height):
        raw.append(0)
        raw.extend(rgb[y * stride : (y + 1) * stride])

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 6)))
        f.write(chunk(b"IEND", b""))


# black, which a text console and the edges of a screen without a picture are
MOON = (0, 0, 0)
# what the text boot draws, from crates/liftoff-splash: its near black, a cell at scale 1 with the logo
# one cell in from the top left, and the green systemd writes OK in
TEXT_BACKGROUND = (4, 4, 6)
TEXT_CELL = (8, 16)
OK_GREEN = (0, 170, 0)
# the graphical theme's mark, which it scales to a fifth of the screen height in the middle
MARK = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "nix", "liftoff", "logo", "rift-mark.png")
# what horizon paints with no window open, the background from nix/modules/horizon.nix
DESKTOP = (36, 36, 36)
# the default wallpaper, nix/wallpapers, which horizon scales to fill the screen: the mean colour of
# squares of it where the photograph is smooth, from the jpeg the image builds cut and scaled to
# 1280x800 the way horizon does it. the dark of space and the lit side of the earth, at the left of
# the screen and at its right, where a window at the left leaves it showing
WALLPAPER = "dark-side-of-earth"
WALLPAPER_SIZE = (1280, 800)
WALLPAPER_SQUARE = 24
WALLPAPER_TOLERANCE = 8
WALLPAPER_LEFT = [((32, 60), (8, 7, 7)), ((356, 264), (78, 68, 54)), ((320, 348), (57, 47, 38)),
                  ((100, 680), (8, 6, 7)), ((488, 72), (48, 42, 34)), ((560, 500), (17, 16, 16))]
WALLPAPER_RIGHT = [((932, 420), (54, 54, 60)), ((812, 384), (53, 46, 38)), ((776, 612), (63, 65, 73)),
                   ((1200, 100), (7, 6, 6)), ((1180, 680), (8, 7, 8)), ((764, 60), (25, 23, 20))]
# the flat grays rift wallpaper set gives the desktop for the rest of the test, dark and light
DARK_GRAY = "#242424"
LIGHT_GRAY = "#f2f1f0"
# lens's bar and menu, from crates/lens/src/{bar,menu,theme}.rs: the bar's gray and the hairline
# along its bottom, the menu's gray and the field's, and the sizes in logical pixels. the field has
# the bar's own gray, and they never share a row
BAR = (30, 30, 30)
BAR_LINE = (20, 20, 20)
MENU = (46, 46, 46)
FIELD = BAR
BAR_HEIGHT = 32
# the two grays and everything between them, for measuring a bar at an interface text size that is
# not a whole number of pixels: the hairline along its edge falls on half a pixel and that row is
# the two mixed, which is neither gray but is still the bar
BAR_GRAYS = (BAR_LINE[0] - 3, BAR[0] + 3)
MENU_WIDTH = 496
MENU_PAD = 8
MENU_GAP = 4
FIELD_SIZE = (480, 32)
ROW_HEIGHT = 28
OUTPUT_ROWS = 8
APP_ROWS = 20
# lens's dock, from crates/lens/src/dock.rs: its height, the padding at each end, one item and the
# gap between two of them, and the menu a right click on an item opens, all in logical pixels
DOCK_HEIGHT = 44
DOCK_PAD = 6
DOCK_ITEM = 40
DOCK_GAP = 4
DOCK_MENU_WIDTH = 240
DOCK_MENU_PAD = 8
DOCK_MENU_ROW = 28
# a workspace button at the right end, the gap between two of them, and the space between the apps,
# the places and the workspaces
DOCK_SPACE = 24
DOCK_SPACE_GAP = 4
DOCK_BETWEEN = 12
# the apps the dock keeps when the owner has said nothing, in their order, from crates/librift/src/dock.rs
DOCK_KEPT = ["firefox", "dev.rift.Files", "com.mitchellh.ghostty", "dev.zed.Zed", "dev.rift.Settings"]
# how far a dock that does not reach the sides stands off its edge, how tall the dock is with its
# large icons, and how tall the line a hidden dock leaves at the bottom edge is, from
# crates/lens/src/dock.rs and crates/librift/src/dock.rs
DOCK_OFF_EDGE = 8
DOCK_LARGE = 60
DOCK_HIDDEN = 2
# the files the dock keeps its apps and its settings in, and the ones Do not disturb, the apps that
# have sent a notification and the ones whose banners stay off live in, from crates/librift/src
DOCK_FILE = "~/.config/rift/dock"
DOCK_OPTIONS = "~/.config/rift/dock-options"
QUIET_FILE = "~/.config/rift/do-not-disturb"
QUIET_APPS = "~/.config/rift/quiet-apps"
# the name notify-send gives itself, which the Notifications page lists it by
NOTIFY_APP = "notify-send"
# lens's system menu, from crates/lens/src/{bar,system}.rs: the bar's padding at each end and the
# status button's inside it, a status icon, and the menu's width, its margin from the right edge of
# the screen, its padding, a row, the gap in a row and the space at the end of one, in logical pixels
BAR_PAD = 8
STATUS_PAD = 8
STATUS_ICON = 16
SYSTEM_WIDTH = 340
SYSTEM_MARGIN = 8
SYSTEM_PAD = 8
SYSTEM_ROW = 32
SYSTEM_GAP = 8
SYSTEM_INSET = 8
# the rows at the bottom of the system menu, in their order
SESSION_ROWS = ["Lock", "Log out", "Restart", "Shut down"]
# lens's notifications, clock menu and key popup, from crates/lens/src/{notice,banner,datemenu,popup}.rs:
# the gap under the bar and from the right edge, a notification's width, padding, icon, close button and
# action buttons, the clock menu's width, padding and row, and the popup's size and its height over the
# dock, all in logical pixels
NOTIFY_GAP = 8
NOTIFY_WIDTH = 380
NOTIFY_PAD = 12
NOTIFY_ICON = 24
NOTIFY_ICON_GAP = 12
NOTIFY_CLOSE = 24
NOTIFY_BUTTON = 28
CLOCK_WIDTH = 340
CLOCK_PAD = 8
CLOCK_ROW = 32
CLOCK_INSET = 8
SWITCH = 20
POPUP_SIZE = (220, 56)
POPUP_ABOVE = 96
# what the test's notifications say
NOTIFY_SUMMARY = "Rift boot test"
# the app the menu starts, its name in the list and the app id its window has
MENU_APP = "Ghostty"
MENU_APP_ID = "com.mitchellh.ghostty"
# the second app, which is not in the dock until the test pins it. it wants a terminal, so lens
# starts it in one with a class of its own and its window is its, not the terminal's
DOCK_APP = "Helix"
# the name the test flatpak of nix/test-flatpak.nix is listed under once it is installed
FLATPAK_APP = "Rift test app"
# welcome, from crates/welcome: the app id of its window, the note Done writes under home, the size it
# opens at, which horizon floats in the middle of the screen, and the grays of its header bar and its
# page on dark, from crates/rift-ui/src/theme.rs
WELCOME_APP_ID = "dev.rift.Welcome"
WELCOME_NOTE = "~/.local/state/rift/welcomed"
WELCOME_SIZE = (880, 640)
WELCOME_HEADER = (48, 48, 48)
WELCOME_PAGE = (38, 38, 38)
# files, from crates/files: the app id its windows have, in lower case the way the window list is
# searched, the unit the test opens a folder in the way another app would, the folders of home the
# session makes, the file the test copies, renames and throws away, and what the trash's note says
FILES_APP = "Files"
FILES_APP_ID = "dev.rift.files"
FILES_UNIT = "rift-files-test"
FILES_FOLDERS = ["Documents", "Downloads", "Music", "Pictures", "Videos"]
FILES_NOTE = "notes.txt"
FILES_RENAMED = "minutes.txt"
FILES_FOLDER = "Plans"
# the pdf step 4c writes into ~/notes: its pages, the words that find it by meaning with none of
# theirs in it, and the page those words are on, which a row names instead of a line
SEARCH_PDF = "letter.pdf"
SEARCH_PDF_PAGES = [
    ["A letter from Green Lane", "Reference 4471"],
    ["Your appointment with the dentist is on Tuesday at half past nine.",
     "Please bring your insurance card, and tell us a day before if you cannot come."],
]
SEARCH_PDF_WORDS = "teeth checkup booking"
SEARCH_PDF_PAGE = 2
SEARCH_PDF_PHRASE = "insurance card"
# what the app that opens a pdf, from DEFAULT_APPS, has in the app id of its window
SEARCH_PDF_APP = "papers"
# the Timeline and the search field: the file that changes between a snapshot and now, the one that
# is deleted after it, the one under another folder a search by name finds, and the words the search
# by meaning takes, which step 4c wrote ~/notes/bike.txt for
FILES_DIARY = "diary.txt"
FILES_RECEIPTS = "receipts.txt"
FILES_UNDER = "diary of last year.txt"
FILES_MEANING = "bicycle repair"
FILES_FOUND = "notes/bike.txt"
# the disks in Files' sidebar: the drive's own exchange partition and the stick --stick attaches
DRIVE_EXCHANGE = "/exchange"
DRIVE_LABEL = "STICK"
DRIVE_MOUNT = f"/run/media/rift/{DRIVE_LABEL}"
DRIVE_FILE = "handover.txt"
# the locked disk --locked attaches: the luks label it carries, the file system inside it, the
# passphrase the dialog takes and the file that is on it. the sidebar names it by whatever udisks
# says, so the step finds it by being the one that is locked
DRIVE_LOCKED = "LOCKED"
DRIVE_INSIDE = "PRIVATE"
DRIVE_INSIDE_MOUNT = f"/run/media/rift/{DRIVE_INSIDE}"
DRIVE_SECRET = "rift-locked"
DRIVE_WRONG = "not the passphrase"
DRIVE_KEPT = "holiday.txt"
# the pictures step 5q puts in Pictures for the grid, beside the one step 5n opened
PICTURES = ("Harbour at dusk", "Coast road", "Pine ridge")
# one window of `horizon msg --json windows`, whose fields come in the order niri-ipc declares them
WINDOW = re.compile(r'\{"id":(\d+),"title":(?:null|"(?:[^"\\]|\\.)*"),"app_id":(?:null|"([^"]*)"),'
                    r'"pid":(?:null|\d+),"workspace_id":(?:null|\d+),"is_focused":(true|false)')
# the words lens --state prints, and how the bar writes the time
STATE_KEYS = ("clock", "theme", "accent", "text", "apps", "network", "volume", "battery", "menu", "field", "rows", "found", "error", "notice",
              "dock", "workspaces", "item", "brightness", "wired", "wifi", "bluetooth", "system", "dialog",
              "notifications", "banners", "latest", "do-not-disturb", "clock-menu", "popup",
              "recording", "screen-reader", "keyboard", "layout", "dock-position", "dock-extend",
              "dock-icons", "dock-hide", "dock-hidden", "dock-places", "places", "listening", "said",
              "restored", "passed")
DATE_FORMAT = "+%a %-d %b %H:%M"
CLOCK = re.compile(r"^[A-Z][a-z]{2} \d{1,2} [A-Z][a-z]{2} \d\d:\d\d$", re.M)
# what the field and the list ask lens to type, and how many rows the pipeline prints
RESULT_LINE = "echo [rift rift rift]"
RESULT_ROWS = 3
ERROR_LINE = "wifi dance"
# a question with a short answer. the model runs on the cpu, next to horizon's software renderer
QUESTION = "What is the capital of France?"
# the microphone this machine does not have. a pipewire loopback whose capture side is a sink and
# whose playback side is a source: a wav played into the sink is what the machine then hears, which
# is the only way to hand recorded speech to a vm with no sound hardware of its own
MIC_UNIT = "rift-test-mic"
MIC_SINK = "rift-test-sink"
MIC_SOURCE = "rift-test-mic"
# the session journal of P2.8 part 1: where the shell writes down what is open, and the words a
# window's paragraph can hold. the app opened beside the terminal in step 5t is the quickest window
# in the image to open and close, and its app id is matched in lower case the way step 5j matches
SESSION_JOURNAL = "~/.local/state/rift/session"
SESSION_KEYS = ("app", "window", "title", "workspace", "screen", "column", "tile", "floating")
SESSION_APP = "Calculator"
SESSION_APP_ID = "calculator"
# P2.8 part 2, the session coming back: the note in the runtime directory that says this login has
# had its session back, an entry no machine has, and the lines of the journal to compare before and
# after, which are the ones restoring has to put back
SESSION_NOTE = "$XDG_RUNTIME_DIR/lens-restored"
# the second app of the session that comes back, on a workspace of its own. three apps, not three
# windows of one app: an app that keeps one process for all its windows is asked for the second one
# by a message to the process that is already running, and ghostty in this image answers the first
# such message and not a second one, so a session of two terminals comes back as one terminal
SESSION_OTHER = "Characters"
SESSION_OTHER_ID = "characters"
SESSION_STRANGER = "com.example.NotHere"
SESSION_PLACE = ("app", "workspace", "column", "tile")
# the console, from nix/modules/horizon.nix: ghostty's background, the height the window rule gives
# the window in logical pixels, and the app id the bind shows and hides
CONSOLE = (4, 4, 6)
CONSOLE_HEIGHT = 400
CONSOLE_APP_ID = "dev.rift.Console"
# the lock screen, from crates/horizon-lock/src/draw.rs: its gray, the inside of the field, the ring
# around it, the sentence for a refused password, and the field's size in logical pixels
LOCK = (30, 30, 30)
LOCK_FIELD = (46, 46, 46)
ACCENT = (120, 174, 237)
REFUSED = (224, 109, 109)
LOCK_FIELD_SIZE = (280, 32)
LOCK_RING = 2
# the colours of one theme, for the checks that look at the bars, the menu, the desktop and the lock
# screen. dark is the names above; light is crates/lens/src/theme.rs, crates/horizon-lock/src/draw.rs
# and the part of horizon's config crates/librift/src/appearance.rs writes for light
Colors = collections.namedtuple("Colors", "bar line menu field desktop lock lock_field accent refused")
DARK_COLORS = Colors(bar=BAR, line=BAR_LINE, menu=MENU, field=FIELD, desktop=DESKTOP, lock=LOCK,
                     lock_field=LOCK_FIELD, accent=ACCENT, refused=REFUSED)
LIGHT_COLORS = Colors(bar=(235, 235, 235), line=(208, 208, 208), menu=(250, 250, 250), field=(255, 255, 255),
                      desktop=(242, 241, 240), lock=(235, 235, 235), lock_field=(255, 255, 255),
                      accent=(53, 132, 228), refused=(192, 28, 40))
# the photograph the image viewer opens, the one with the most colour in it of the shipped set
PICTURE = "aurora"
# the apps whose title bars the test looks at, the first two in the dock, from left to right on screen
TITLED_APPS = ["firefox", "com.mitchellh.ghostty"]
# the image's first qt app, which the test starts from the Applications menu: its name in the list, and
# what the app id of its window has in it whatever case it is in
QT_APP = "KeePassXC"
QT_APP_ID = "keepassxc"
# the everyday apps, each started from the Applications menu by the name the list shows: the name to
# type, what the app id of its window has in it whatever case it is in, what its screendump is called,
# how much of its title bar one gray covers, and whether it asks a portal for permission before it
# draws. an app id is the application's own name for most of them and the program's name for the disk
# utility, so these are the part they share
BASIC_APPS = [
    ("Image Viewer", "loupe", "loupe", 0.4, False),
    ("Document Viewer", "papers", "papers", 0.4, False),
    ("Video Player", "showtime", "showtime", 0.4, False),
    ("Audio Player", "decibels", "decibels", 0.4, False),
    ("Calculator", "calculator", "calculator", 0.4, False),
    ("File Roller", "roller", "file-roller", 0.4, False),
    # the disk utility is the one of the nine still written for gtk 3, whose title bar is a gradient
    # of grays next to a flat one over the sidebar, so no single gray covers much of it
    ("Disks", "disk", "disks", 0.2, False),
    ("Disk Usage Analyzer", "baobab", "baobab", 0.4, False),
    ("Characters", "characters", "characters", 0.4, False),
    # the camera and the scanner have no hardware to find in a virtual machine, so each draws the
    # page it draws when there is none, with its title bar above it. the camera asks the portal for
    # the camera before it looks for one, and the portal asks the owner in a window of its own
    ("Camera", "snapshot", "camera", 0.4, True),
    ("Document Scanner", "scan", "scanner", 0.4, False),
]
# the settings app: the name typed into the Applications menu, what the app id of its window has in
# it in lower case, how many pages its sidebar lists, and the two accents the Appearance page is set
# to and put back to, in the colours crates/librift/src/appearance.rs gives them on dark
SETTINGS_APP = "Settings"
SETTINGS_APP_ID = "dev.rift.settings"
SETTINGS_PAGES = 23
SETTINGS_ACCENT = ("blue", "#78aeed")
SETTINGS_OTHER = ("teal", "#68b4c1")
# where the boot style lives on the esp, from crates/librift/src/boot.rs. the running system has the
# esp at /boot, root only, so reading it back takes sudo
BOOT_STYLE_FILE = "rift/boot-style"
# what rift-settings --state prints, one word each
SETTINGS_KEYS = ("page", "theme", "accent", "wallpaper", "gaps", "radius", "text", "terminal",
                 "greeting", "boot", "screen", "wifi", "networks", "network", "wired", "address",
                 "bluetooth", "devices", "connected", "volume", "mute", "output", "outputs",
                 "input-volume", "input-mute", "input", "inputs", "battery", "ai", "model", "models",
                 "tier", "search", "search-model", "indexed", "index", "indexing", "snapshots",
                 "snapshot", "backups", "backup", "backup-folder", "taking", "backing", "version",
                 "slot", "slot-a", "slot-b", "tries-a", "tries-b", "updates", "waiting", "timezone",
                 "ntp", "synchronized", "rtc", "time", "date", "locale", "language", "formats",
                 "paper", "keymap", "layout", "printers", "printer", "default-printer", "jobs", "job",
                 "screen-reader", "on-screen-keyboard", "layouts", "console-keymap", "mice", "touchpads",
                 "primary-button", "mouse-speed", "mouse-acceleration", "mouse-natural-scrolling",
                 "touchpad-speed", "tap-to-click", "touchpad-natural-scrolling", "disable-while-typing",
                 "edge-scrolling", "pinned", "dock-position", "dock-extend", "dock-icons", "dock-hide",
                 "do-not-disturb", "notifiers", "owner-user", "owner-name", "owner-password", "problem")
# the time zone the Date and time page sets and puts back, with what date calls it at either time of
# year. a drive where no zone was ever chosen is in UTC
SETTINGS_ZONE = ("Pacific/Auckland", ("NZST", "NZDT"))
# where timedated keeps the link to the zone, on persist, and the locale the image is in
ZONE_LINK = "/var/lib/rift/zone/localtime"
SETTINGS_LOCALE = "en_GB.UTF-8"
# the layout the Keyboard page adds and takes off again, with the name horizon has for it, and the
# file on persist localed keeps the desktop's layouts in
SETTINGS_LAYOUT = ("gb", "English (UK)")
KEYBOARD_FILE = "/var/lib/rift/keyboard/vconsole.conf"
# the border horizon draws around its list of shortcuts: (0.5, 0.8, 1.0), drawn at ninety per cent over
# the dark grays under it. the accent's green is too far from it to count. the border is four pixels
# wide around a box of hundreds, so the list brings thousands of them
SHORTCUTS_BORDER = (122, 188, 234)
SHORTCUTS_BORDER_PIXELS = 1_000
# the part of horizon's config the Mouse and touchpad page writes, and the file it keeps its settings in
POINTER_PART = "~/.local/state/rift/pointer.kdl"
POINTER_FILE = "~/.config/rift/pointer"
# CUPS's own test printer, which stands in for a printer on the network on a machine with none: the
# port of localhost it answers IPP on, the queue the test makes for it and what that queue is
# called, the job the test holds in it, and the message it is stopped with
TEST_PRINTER_PORT = 8631
TEST_PRINTER = ("Rift_test", "Rift test printer")
TEST_JOB = "Rift boot test page"
TEST_STOPPED = "Paused by the boot test"
# the interface text size the Appearance page is set to and put back to, in per cent, with the
# factor dconf holds for the first of them. the shell asks for its surfaces at that much of their
# size, so the bar and the dock on screen are their own heights times it
SETTINGS_TEXT = (150, 100)
SETTINGS_FACTOR = "1.5"
# the terminal colour schemes it is set to and put back to, from crates/librift/src/appearance.rs:
# the word and the colour the terminal is drawn on. Rift's own is the near black of the console
SETTINGS_SCHEME = ("solarized-dark", (0, 43, 54))
SETTINGS_OWN_SCHEME = ("rift", CONSOLE)
# how many pixels of that colour mean the terminal window is drawn on it. the window stands beside
# the Settings window, so it is a quarter to a half of the screen with text over part of it
TERMINAL_PIXELS = 60_000
# a window a portal asks a question in, with the rectangle horizon gave it. the size comes before the
# position in the window list, and both are the logical pixels the pointer moves in
PORTAL_WINDOW = re.compile(r'"app_id":"[^"]*portal[^"]*".{0,400}?"window_size":\[(\d+),(\d+)\],'
                           r'"tile_pos_in_workspace_view":\[([\d.]+),([\d.]+)\]')
# how many light gray pixels on the screen mean a window is drawn in the light theme. the portal's
# question is about 515 by 220, so a light one is over eighty thousand of them and the text of a dark
# one is a few thousand at most
PORTAL_LIGHT = 20000
# the app a file of each kind opens with, as `xdg-mime query default` prints it
DEFAULT_APPS = [
    ("image/jpeg", "org.gnome.Loupe.desktop"),
    ("application/pdf", "org.gnome.Papers.desktop"),
    ("video/mp4", "org.gnome.Showtime.desktop"),
    ("audio/flac", "org.gnome.Decibels.desktop"),
    ("application/zip", "org.gnome.FileRoller.desktop"),
    ("inode/directory", "dev.rift.Files.desktop"),
]
# the kind of file the image has more than one app for, as the Apps page words it: the app the image
# opens it with, the one the page chooses instead, which runs in a terminal, and a second type of the
# kind, which follows the first. then the file handed to that app, and the unit it is started in
SETTINGS_KIND = ("text", "dev.zed.Zed.desktop", "Helix.desktop", "text/x-csrc")
HANDED_FILE = "/home/rift/rift-apps-test.txt"
HANDED_UNIT = "rift-apps-test"
# the permission store on the session bus, where the camera portal keeps each app's answer, and the
# dconf key GTK reads for recent files, as the Privacy and security page reads both
PERMISSION_STORE = ("org.freedesktop.impl.portal.PermissionStore /org/freedesktop/impl/portal/PermissionStore "
                    "org.freedesktop.impl.portal.PermissionStore")
RECENT_FILES_KEY = "/org/gnome/desktop/privacy/remember-recent-files"
# the app rift net turns off in a terminal, for the page to give the network back to
PRIVACY_APP = "rift-privacy-test"
# the apps, tools and languages of the image's first tier, each with the command that prints its
# version and what that has to print. the commands run in fish, as the owner
TOOLS = [
    ("keepassxc-cli --version", r"\b2\.\d+\.\d+"),
    ("nvim --version", r"^NVIM v0\.\d+"),
    ("virt-manager --version", r"^\d+\.\d+\.\d+"),
    ("virsh --version", r"^\d+\.\d+\.\d+"),
    ("gh --version", r"^gh version \d"),
    ("gdb --version", r"^GNU gdb .* \d+\.\d+"),
    ("lldb --version", r"^lldb version \d"),
    ("valgrind --version", r"^valgrind-\d"),
    ("strace -V", r"^strace -- version \d"),
    ("ltrace -V", r"^ltrace 0\.\d+"),
    ("perf --version", r"^perf version \d"),
    ("fzf --version", r"^\d+\.\d+"),
    ("bat --version", r"^bat \d"),
    ("nmap --version", r"^Nmap version \d"),
    ("ssh -V 2>&1", r"^OpenSSH_\d"),
    ("wg --version", r"^wireguard-tools v\d"),
    ("gpg --version", r"^gpg \(GnuPG\) 2\."),
    ("age --version", r"^v?1\.\d+"),
    ("sensors -v", r"^sensors version \d"),
    ("smartctl --version", r"^smartctl \d"),
    ("powertop --version", r"PowerTOP version"),
    ("iotop --version", r"iotop-c 1\.\d+"),
    ("nvtop --version", r"^nvtop version \d"),
    ("rustc --version", r"^rustc 1\.\d+"),
    ("cargo --version", r"^cargo 1\.\d+"),
    ("rustfmt --version", r"^rustfmt \d"),
    ("cargo clippy --version", r"^clippy \d"),
    ("rust-analyzer --version", r"^rust-analyzer "),
    ("gcc --version", r"\(GCC\) 1\d\."),
    ("g++ --version", r"\(GCC\) 1\d\."),
    ("cc --version", r"\(GCC\) 1\d\."),
    ("clang --version", r"^clang version \d"),
    ("clangd --version", r"clangd version \d"),
    ("ld.lld --version", r"^LLD \d"),
    ("llvm-ar --version", r"LLVM version \d"),
    ("cmake --version", r"^cmake version \d"),
    ("ninja --version", r"^1\.\d+"),
    ("make --version", r"^GNU Make \d"),
    ("python3 --version", r"^Python 3\.\d+"),
    ("node --version", r"^v\d+\."),
    ("npm --version", r"^\d+\.\d+"),
    ("bun --version", r"^1\.\d+"),
    ("go version", r"^go version go1\.\d+"),
    ("zig version", r"^0\.\d+"),
    ("java -version 2>&1", r'^openjdk version "25'),
    ("javac -version 2>&1", r"^javac 25"),
]
# a program for each compiler of the first tier: the command line that writes, builds and runs it in the
# folder the test makes, and the line it prints. fish's printf with %s writes each word as a line. zig
# draws its progress on a terminal, so its output goes through cat, which is not one
BUILDS = [
    ("gcc", r'''printf '%s\n' '#include <stdio.h>' 'int main(void) { puts("c runs"); return 0; }' > hello.c; '''
            r'''and gcc -o hello-gcc hello.c; and ./hello-gcc''', "c runs"),
    ("clang", r'''clang -o hello-clang hello.c; and ./hello-clang''', "c runs"),
    ("g++", r'''printf '%s\n' '#include <iostream>' '''
            r''''int main() { std::cout << "c++ runs" << std::endl; }' > hello.cpp; '''
            r'''and g++ -o hello-gxx hello.cpp; and ./hello-gxx''', "c++ runs"),
    ("clang++", r'''clang++ -o hello-clangxx hello.cpp; and ./hello-clangxx''', "c++ runs"),
    ("cmake and ninja", r'''printf '%s\n' 'cmake_minimum_required(VERSION 3.20)' 'project(hello C)' '''
                        r''''add_executable(hello hello.c)' > CMakeLists.txt; '''
                        r'''and cmake -G Ninja -S . -B build; and cmake --build build; and ./build/hello''', "c runs"),
    ("make", r'''printf 'hello-make: hello.c\n\tcc -o hello-make hello.c\n' > Makefile; '''
             r'''and make hello-make; and ./hello-make''', "c runs"),
    ("zig cc", r'''zig cc -o hello-zig hello.c 2>&1 | cat; and ./hello-zig''', "c runs"),
    ("rustc", r'''printf '%s\n' 'fn main() { println!("rust runs"); }' > hello.rs; '''
              r'''and rustc -o hello-rs hello.rs; and ./hello-rs''', "rust runs"),
    ("go", r'''printf '%s\n' 'package main' 'import "fmt"' 'func main() { fmt.Println("go runs") }' > hello.go; '''
           r'''and go run hello.go''', "go runs"),
    ("java", r'''printf '%s\n' 'class Hello { public static void main(String[] args) { '''
             r'''System.out.println("java runs"); } }' > Hello.java; and java Hello.java''', "java runs"),
    ("python", r'''python3 -c 'print("python runs")' ''', "python runs"),
    ("node", r'''node -e 'console.log("node runs")' ''', "node runs"),
    ("bun", r'''bun -e 'console.log("bun runs")' ''', "bun runs"),
]
# the owner's password from nix/profiles/base.nix, and one that is not it
PASSWORD = "rift"
WRONG_PASSWORD = "wrongpassword"
# the owner's account and the name every drive starts with, from crates/librift/src/owner.rs, where
# vault keeps the hash of a password the owner chose, and the name and the password the Owner page
# gives the owner for a while. the name is longer than the image's, so the lock screen draws a wider
# line for it, and the password is letters and digits, which the monitor types as key codes
OWNER_USER = "rift"
OWNER_NAME = "Rift owner"
OWNER_PASSWORD_FILE = "/var/lib/rift/owner/password"
OWNER_NEW_NAME = "Samantha Taylor-Brooks"
OWNER_NEW_PASSWORD = "riverstone42"
# the passphrase the test gives the clone's persist, not the first drive's
CLONE_PASSPHRASE = "clone-test-5213"
# gpt partition types from the discoverable partitions specification: the esp, /usr on x86-64 and
# its verity data. slot a and slot b each have a store and a verity partition
ESP_TYPE = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
USR_TYPE = "8484680c-9521-48c6-9c11-b0720656f69e"
USR_VERITY_TYPE = "77ff5f63-e7b6-4633-acf4-1565b864c0e6"
# where the transfers in nix/image/ab-sysupdate.nix read a new version from, and the tries they give
# its uki
UPDATES = "/var/lib/rift/updates"
TRIES = 3
# where the test mounts the updates drive, and the unit that keeps the broken version from being good
UPDATES_DRIVE = "/run/updates-drive"
NEVER_GOOD = "never-good.service"


def near(pixel, color, tolerance):
    return all(abs(a - b) <= tolerance for a, b in zip(pixel, color))


def png_size(path):
    """A png's width and height, from its header."""
    with open(path, "rb") as f:
        return struct.unpack(">II", f.read(24)[16:24])


def check_splash(width, height, rgb):
    """Find the graphical theme in a screendump: its near black over most of the screen, and the
    mark's ice in its box a fifth of the screen high in the middle and nowhere else. Returns (ok,
    lines to print)."""
    mark_width, mark_height = png_size(MARK)
    size = height // 5
    scaled = mark_width * size / mark_height
    left, top = width / 2 - scaled / 2, height / 2 - size / 2
    background = inside = outside = 0
    for y in range(height):
        row = y * width * 3
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, TEXT_BACKGROUND, 6):
                background += 1
            elif ice(px):
                if left - 2 <= x < left + scaled + 2 and top - 2 <= y < top + size + 2:
                    inside += 1
                else:
                    outside += 1
    total = width * height
    # the mark's lines cover about a sixth of its box
    checks = [
        ("the background covers most of the screen", background >= 0.9 * total, f"{background} of {total}"),
        ("the mark is where the theme puts it", inside >= 0.08 * scaled * size,
         f"{inside} pixels of its ice in a box of {scaled:.0f}x{size}"),
        ("and nothing else is in the logo's ice", outside <= 100, f"{outside} pixels outside it"),
    ]
    lines = [f"splash: {width}x{height}, graphical style"]
    ok = True
    for name, passed, detail in checks:
        lines.append(f"splash: {'ok  ' if passed else 'FAIL'} {name}: {detail}")
        ok = ok and passed
    return ok, lines


def menu_height(rows, line):
    """How tall lens's menu is with this many result rows and with or without the line under them."""
    listed = 0 if rows == 0 else MENU_GAP + rows * ROW_HEIGHT
    under = MENU_GAP + ROW_HEIGHT if line else 0
    return MENU_PAD + FIELD_SIZE[1] + listed + under + MENU_PAD


def bar_gray_rows(width, height, rgb, colors=DARK_COLORS):
    """How much of each row is one of the bar's two grays. The bar is the run of those rows from the
    top of the screen and the dock the run from the bottom; the same gray anywhere else is the
    field's, inside a menu."""
    rows = []
    for y in range(height):
        row = y * width * 3
        found = 0
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, colors.bar, 3) or near(px, colors.line, 3):
                found += 1
        rows.append(found)
    return rows


def bar_and_dock(width, height, rows):
    """(the bar's rows at the top, the dock's rows at the bottom) from the counts of one screendump."""
    bar = 0
    while bar < height and rows[bar] > width / 2:
        bar += 1
    dock = 0
    while bar + dock < height and rows[height - 1 - dock] > width / 2:
        dock += 1
    return bar, dock


def keyboard_rows(width, height, rgb, colors=DARK_COLORS):
    """The top and bottom rows the on-screen keyboard covers, or None when it is not on the screen.
    Its keys are drawn in the gray of a menu, it takes the bottom of the screen, and nothing else
    down there is that gray, so the rows of the bottom half where it covers a third of the width
    are the keyboard's."""
    found = []
    for y in range(height // 2, height):
        row = y * width * 3
        keys = 0
        for x in range(0, width, 4):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, colors.menu, 3):
                keys += 1
        if keys * 4 > width / 3:
            found.append(y)
    return (found[0], found[-1]) if found else None


def ink_in(width, rgb, top, bottom, colors=DARK_COLORS):
    """What is drawn on the bar's gray in these rows, counted by third of the screen's width."""
    found = [0, 0, 0]
    for y in range(top, bottom):
        row = y * width * 3
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if not near(px, colors.bar, 3) and not near(px, colors.line, 3):
                found[min(2, x * 3 // width)] += 1
    return found


def coloured_in(width, rgb, top, bottom):
    """How many of the pixels in these rows have a colour, of the pixels looked at. The bars, the
    windows and their title bars are neutral grays, so a count near zero means nothing on screen
    is drawn from a photograph."""
    found = 0
    looked = 0
    for y in range(top, bottom, 4):
        row = y * width * 3
        for x in range(0, width, 4):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            looked += 1
            if max(px) - min(px) > 20:
                found += 1
    return found, looked


def wallpaper_squares(width, height, rgb, squares):
    """The check that the default wallpaper is on screen: the mean colour of each square against the
    one the photograph has there. One square may be under the pointer."""
    if (width, height) != WALLPAPER_SIZE:
        return ("the wallpaper fills the screen", False,
                f"the squares are for {WALLPAPER_SIZE[0]}x{WALLPAPER_SIZE[1]}, the screen is {width}x{height}")
    matched, found = 0, []
    area = WALLPAPER_SQUARE * WALLPAPER_SQUARE
    for (left, top), wanted in squares:
        total = [0, 0, 0]
        for y in range(top, top + WALLPAPER_SQUARE):
            row = y * width * 3
            for x in range(left, left + WALLPAPER_SQUARE):
                for channel in range(3):
                    total[channel] += rgb[row + x * 3 + channel]
        mean = tuple(round(value / area) for value in total)
        found.append(f"{mean} for {wanted}")
        matched += near(mean, wanted, WALLPAPER_TOLERANCE)
    return (f"the wallpaper {WALLPAPER} fills the screen", matched >= len(squares) - 1,
            f"{matched} of {len(squares)} squares match: " + ", ".join(found))


def check_desktop(width, height, rgb, lens=False, menu=False, rows=0, line=False, system=None,
                  notification=None, clock=None, popup=None, colors=DARK_COLORS, wallpaper=None):
    """Count the desktop gray and the console's black in a screendump, and with lens the bar along
    the top with something drawn at its left, in its middle and at its right, the dock along the
    bottom with the apps in it, and with menu the Applications menu under the bar with the field in
    it. rows is a count, or (fewest, most) when the test cannot know how many rows there are: then
    any count in that range that fits passes. system is the (width, height) lens says the system
    menu has, which then hangs under the bar at the right. notification, clock and popup are the
    sizes lens says a notification, the clock menu and the key popup have: the first stands under
    the bar at the right, the second hangs under the clock in the middle, and the third stands over
    the dock in the middle. colors are the theme's. wallpaper is squares of the default wallpaper,
    which then shows instead of the flat gray. Returns (ok, lines to print)."""
    gray = black = menu_gray = 0
    bar_like = [0] * height
    # the field has the bar's own gray on dark and is white on light
    field_like = [0] * height
    # the rows that are part of the menu, and where its gray starts and ends in each
    menu_rows = []
    for y in range(height):
        row = y * width * 3
        row_bar = row_menu = row_field = 0
        first = last = -1
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, colors.bar, 3) or near(px, colors.line, 3):
                row_bar += 1
            elif near(px, colors.desktop, 3):
                gray += 1
            elif near(px, MOON, 8):
                black += 1
            elif near(px, colors.menu, 3):
                row_menu += 1
                first, last = (x if first < 0 else first), x
            elif near(px, colors.field, 3):
                row_field += 1
        bar_like[y] = row_bar
        field_like[y] = row_field if colors.field != colors.bar else row_bar
        menu_gray += row_menu
        # a row of the menu has at least its padding on either side of whatever is in it; a pixel
        # of the menu's gray anywhere else is the edge of a letter or of the pointer
        if row_menu >= 12:
            menu_rows.append((y, first, last))
    bar_rows, dock_rows = bar_and_dock(width, height, bar_like)
    # the same gray between the two bars is the field's, inside the menu, and only the rows between
    # them can be the menu's: a few pixels of its gray in the bar are the edges of letters
    field = sum(field_like[bar_rows : height - dock_rows])
    menu_rows = [found for found in menu_rows if bar_rows <= found[0] < height - dock_rows]
    total = width * height
    # the photograph is mostly the black of space, so it has its squares instead of the counts
    checks = [wallpaper_squares(width, height, rgb, wallpaper)] if wallpaper else [
        ("no console black", black <= 0.02 * total, f"{black} of {total}"),
    ]
    if not lens:
        if not wallpaper:
            checks.insert(0, ("the desktop background covers the screen", gray >= 0.95 * total,
                              f"{gray} of {total}"))
        lines = [f"desktop: {width}x{height}"]
        return report("desktop", lines, checks)
    # the compositor may scale the bar, so its height on screen gives the scale
    scale = bar_rows / BAR_HEIGHT if bar_rows else 1
    ink = ink_in(width, rgb, 0, bar_rows, colors)
    dock_ink = ink_in(width, rgb, height - dock_rows, height, colors)
    checks += [
        ("the bar is along the top", 0.9 * BAR_HEIGHT <= bar_rows <= 3 * BAR_HEIGHT,
         f"{bar_rows} rows, expected about {BAR_HEIGHT} at scale 1"),
        ("the button, the clock and the icons are in it", all(count >= 30 for count in ink),
         f"{ink[0]} pixels at the left, {ink[1]} in the middle, {ink[2]} at the right"),
        ("the dock is along the bottom", 0.9 * DOCK_HEIGHT <= dock_rows <= 3 * DOCK_HEIGHT,
         f"{dock_rows} rows, expected about {DOCK_HEIGHT} at scale 1"),
        ("the apps are in it and the workspaces at its right", dock_ink[0] >= 300 and dock_ink[2] >= 20,
         f"{dock_ink[0]} pixels at the left, {dock_ink[2]} at the right"),
    ]
    # the run of rows the menu covers, and where its gray starts and ends in the first of them,
    # which is the padding above the field and so the full width of the menu inside its border
    run = []
    for found in menu_rows:
        if not run or found[0] == run[-1][0] + 1:
            run.append(found)
        else:
            break
    top = run[0][0] if run else -1
    left, right = (run[0][1], run[0][2]) if run else (-1, -1)
    box = (right - left + 1, len(run)) if run else (0, 0)
    shown = notification or clock or popup
    if shown:
        # the gray starts inside the one pixel border
        if notification:
            name, where = "the notification", "under the bar at the right"
            wanted = (width - (NOTIFY_GAP + shown[0]) * scale + 1, bar_rows + NOTIFY_GAP * scale + 1)
        elif clock:
            name, where = "the clock menu", "under the clock"
            wanted = ((width - shown[0] * scale) / 2 + 1, bar_rows + 1)
        else:
            name, where = "the key popup", "over the dock in the middle"
            wanted = ((width - shown[0] * scale) / 2 + 1, height - dock_rows - (POPUP_ABOVE + shown[1]) * scale + 1)
        inside = ((shown[0] - 2) * scale, (shown[1] - 2) * scale)
        below = total - (bar_rows + dock_rows) * width - inside[0] * inside[1]
        checks += [
            (f"{name} is {where}", abs(left - wanted[0]) <= 2 * scale and abs(top - wanted[1]) <= 2 * scale,
             f"its gray starts at {left},{top}, expected {wanted[0]:.0f},{wanted[1]:.0f}"),
            (f"{name} is as wide and as tall as lens says",
             abs(box[0] - inside[0]) <= 4 * scale and abs(box[1] - inside[1]) <= 4 * scale,
             f"{box[0]}x{box[1]}, expected about {inside[0]:.0f}x{inside[1]:.0f}"),
            ("the desktop background covers the rest", gray >= 0.9 * below, f"{gray} of {below:.0f}"),
        ]
        return report("desktop", [f"desktop: {width}x{height}"], checks)
    if system:
        # the menu's gray starts inside its one pixel border, and its right edge is its margin from
        # the edge of the screen
        inside = ((system[0] - 2) * scale, (system[1] - 2) * scale)
        edge = width - (SYSTEM_MARGIN + 1) * scale - 1
        below = total - (bar_rows + dock_rows) * width - inside[0] * inside[1]
        checks += [
            ("the system menu hangs under the bar at the right",
             abs(top - bar_rows - 1) <= 2 * scale and abs(right - edge) <= 2 * scale,
             f"its gray runs from {left},{top} to {right}, the bar ends at {bar_rows}, expected its right at {edge:.0f}"),
            ("the system menu is as wide and as tall as lens says",
             abs(box[0] - inside[0]) <= 4 * scale and abs(box[1] - inside[1]) <= 4 * scale,
             f"{box[0]}x{box[1]}, expected about {inside[0]:.0f}x{inside[1]:.0f}"),
            ("the desktop background covers the rest", gray >= 0.9 * below, f"{gray} of {below:.0f}"),
        ]
        return report("desktop", [f"desktop: {width}x{height}"], checks)
    if wallpaper:
        return report("desktop", [f"desktop: {width}x{height}"], checks)
    if not menu:
        below = total - (bar_rows + dock_rows) * width
        checks += [
            ("the desktop background covers the rest", gray >= 0.95 * below, f"{gray} of {below}"),
            # the menu's padding is eight rows before anything else in it
            ("no menu is open", len(run) < 3, f"{len(run)} rows of the menu's gray under the bar"),
        ]
        return report("desktop", [f"desktop: {width}x{height}"], checks)

    def menu_checks(count):
        # the menu's gray starts inside its one pixel border, and the field is a rectangle in it
        wanted = menu_height(count, line)
        inside = ((MENU_WIDTH - 2) * scale, (wanted - 2) * scale)
        field_area = FIELD_SIZE[0] * FIELD_SIZE[1] * scale * scale
        below = total - (bar_rows + dock_rows) * width - inside[0] * inside[1]
        return wanted, [
            ("the menu hangs under the bar at the left", abs(top - bar_rows - 1) <= 2 * scale and abs(left - (MENU_PAD + 1) * scale) <= 2 * scale,
             f"its gray starts at {left},{top}, the bar ends at {bar_rows}"),
            ("the menu is as wide and as tall as its contents", abs(box[0] - inside[0]) <= 4 * scale and abs(box[1] - inside[1]) <= 4 * scale,
             f"{box[0]}x{box[1]}, expected about {inside[0]:.0f}x{inside[1]:.0f} for {count} result rows"),
            ("the field is in it", 0.6 * field_area <= field <= 1.1 * field_area,
             f"{field} pixels of the field's gray, expected about {field_area:.0f}"),
            ("the desktop background covers the rest", gray >= 0.9 * below, f"{gray} of {below:.0f}"),
        ]

    fewest, most = rows if isinstance(rows, tuple) else (rows, rows)
    options = [menu_checks(count) for count in range(fewest, most + 1)]
    # the first count that fits, or when none does, the one closest to the menu on screen
    fits = [found for wanted, found in options if all(passed for _, passed, _ in found)]
    checks += fits[0] if fits else min(options, key=lambda option: abs(option[0] - box[1]))[1]
    return report("desktop", [f"desktop: {width}x{height}"], checks)


def report(what, lines, checks):
    """Print one line per check and say whether they all passed."""
    ok = True
    for name, passed, detail in checks:
        lines.append(f"{what}: {'ok  ' if passed else 'FAIL'} {name}: {detail}")
        ok = ok and passed
    return ok, lines


# the logo in characters
LOGO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "nix", "liftoff", "logo", "rift-logo.txt")
# a text console draws with the kernel's 8 by 16 font and palette: gray text on black
TTY_GRAY = (170, 170, 170)
TTY_CELL = (8, 16)


def logo_lines():
    """The logo's lines the way a terminal shows them, without trailing spaces."""
    with open(LOGO, encoding="ascii") as f:
        return [line.rstrip() for line in f.read().rstrip("\n").split("\n")]


def ice(px):
    """The logo's blues, far more blue than red. The terminal's text and the grays are neither."""
    return px[2] >= 90 and px[2] - px[0] >= 40


def check_text_splash(width, height, rgb):
    """Count the text boot's colours in a screendump: its near black over most of the screen, the
    logo's ice in the block at the top left where the logo is drawn and nowhere else, and systemd's
    green in the console under the logo, an OK line or more. Returns (ok, lines to print)."""
    cell_w, cell_h = TEXT_CELL
    logo = logo_lines()
    left, top = cell_w, cell_h
    right, bottom = left + max(len(line) for line in logo) * cell_w, top + len(logo) * cell_h
    background = ice_inside = ice_outside = green = 0
    for y in range(height):
        row = y * width * 3
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, TEXT_BACKGROUND, 6):
                background += 1
            elif ice(px):
                if left <= x < right and top <= y < bottom:
                    ice_inside += 1
                else:
                    ice_outside += 1
            elif y >= bottom and near(px, OK_GREEN, 24):
                green += 1
    total = width * height
    # an OK in bold at this size is about 32 pixels of the green
    checks = [
        ("the background covers most of the screen", background >= 0.85 * total, f"{background} of {total}"),
        ("the logo's ice is where the logo is drawn", ice_inside >= 3000, f"{ice_inside} pixels"),
        ("and nowhere else", ice_outside <= 100, f"{ice_outside} pixels outside the logo"),
        ("systemd's green OK is in the console under the logo", green >= 64, f"{green} pixels"),
    ]
    lines = [f"splash: {width}x{height}, text style"]
    ok = True
    for name, passed, detail in checks:
        lines.append(f"splash: {'ok  ' if passed else 'FAIL'} {name}: {detail}")
        ok = ok and passed
    return ok, lines


def check_console(width, height, rgb):
    """Find the console in a screendump: lens's bar along the top, under it a run of rows that
    are mostly the console's background, and the desktop under that. The terminal shows fish's
    greeting from its first line down: fastfetch's rows alone, as the console is too short for the
    whole logo beside them. Returns (ok, lines to print)."""
    gray = black = logo = 0
    bar_like = [0] * height
    console_top, console_rows, console_width = -1, 0, 0
    text_rows = []
    for y in range(height):
        row = y * width * 3
        row_bar = row_console = row_text = row_ice = row_black = 0
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, CONSOLE, 1):
                row_console += 1
                continue
            # what is not close to the console's background is text. lines start at the left edge,
            # the pointer sits in the middle of the screen
            if x < width / 4 and not near(px, CONSOLE, 12):
                row_text += 1
            # the focus ring runs along the window's edges
            if 8 <= x < width - 8 and ice(px):
                row_ice += 1
            if near(px, DESKTOP, 3):
                gray += 1
            elif near(px, MOON, 8):
                row_black += 1
            elif near(px, BAR, 3) or near(px, BAR_LINE, 3):
                row_bar += 1
        bar_like[y] = row_bar
        # the first run of rows that are mostly the console's background. a line of text in the
        # terminal covers only some of a row. the edges of text on its near black are near black
        # too, so black counts only outside the console
        if row_console > width / 2 and (console_top < 0 or console_top + console_rows == y):
            if console_top < 0:
                console_top = y
            console_rows += 1
            console_width = max(console_width, row_console)
            logo += row_ice
            if row_text:
                text_rows.append(y)
        else:
            black += row_black
    total = width * height
    bar_rows, dock_rows = bar_and_dock(width, height, bar_like)
    scale = bar_rows / BAR_HEIGHT if bar_rows else 1
    wanted = CONSOLE_HEIGHT * scale
    below = total - (bar_rows + dock_rows + console_rows) * width
    # a line of DejaVu Sans Mono 11 is 17 rows. the greeting starts a few rows under the window's
    # top, and fastfetch's rows run down most of the window
    greeting = bool(text_rows) and text_rows[0] - console_top <= 12 * scale \
        and text_rows[-1] - text_rows[0] >= 18 * 17 * scale
    checks = [
        ("no console black", black <= 0.02 * total, f"{black} of {total}"),
        ("the bar is along the top", 0.9 * BAR_HEIGHT <= bar_rows <= 3 * BAR_HEIGHT, f"{bar_rows} rows"),
        ("the dock is along the bottom", 0.9 * DOCK_HEIGHT <= dock_rows <= 3 * DOCK_HEIGHT, f"{dock_rows} rows"),
        ("the console starts under the bar", console_top >= 0 and bar_rows <= console_top <= bar_rows + 16 * scale,
         f"first row {console_top}, the bar ends at {bar_rows}"),
        ("the console is as tall as the window rule says", 0.95 * wanted <= console_rows <= 1.05 * wanted,
         f"{console_rows} rows, expected about {wanted:.0f}"),
        ("the console is as wide as the screen", console_width >= 0.9 * width, f"{console_width} of {width} in its widest row"),
        ("the desktop background covers the rest", gray >= 0.9 * below, f"{gray} of {below}"),
        ("the terminal shows the greeting", greeting,
         f"text in rows {text_rows[0]} to {text_rows[-1]}, the console starts at {console_top}" if text_rows else "no text"),
        ("the greeting has no logo, the console is too short for it", logo <= 100 * scale * scale,
         f"{logo} pixels in the logo's blues"),
    ]
    lines = [f"console: {width}x{height}"]
    ok = True
    for name, passed, detail in checks:
        lines.append(f"console: {'ok  ' if passed else 'FAIL'} {name}: {detail}")
        ok = ok and passed
    return ok, lines


def check_tty(width, height, rgb):
    """Find /etc/issue on a text console: black over the screen, gray text at the top left, which is
    the name line and the login, and nothing in colour, since the logo is not in /etc/issue.
    Returns (ok, lines to print)."""
    cell_w, cell_h = TTY_CELL
    black = coloured = text = 0
    for y in range(height):
        row = y * width * 3
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, MOON, 8):
                black += 1
            elif near(px, TTY_GRAY, 24):
                if x < 40 * cell_w and y < 8 * cell_h:
                    text += 1
            else:
                coloured += 1
    total = width * height
    checks = [
        ("black covers most of the screen", black >= 0.95 * total, f"{black} of {total}"),
        ("the name and the login are at the top left", text >= 150, f"{text} gray pixels"),
        ("nothing is in colour, the logo is not there", coloured <= 0.001 * total, f"{coloured} coloured pixels"),
    ]
    lines = [f"tty: {width}x{height}"]
    ok = True
    for name, passed, detail in checks:
        lines.append(f"tty: {'ok  ' if passed else 'FAIL'} {name}: {detail}")
        ok = ok and passed
    return ok, lines


def check_lock(width, height, rgb, refused=False, colors=DARK_COLORS):
    """Find the lock screen in a screendump: its gray over the whole screen, the field in the middle
    with the blue ring around it, and none of the desktop, lens's bar or the console. With
    refused, the red sentence is under the field, without it there is none. colors are the theme's.
    Returns (ok, lines to print)."""
    ground = desktop = console = ring = red = field = 0
    left, top, right, bottom = width, height, -1, -1
    for y in range(height):
        row = y * width * 3
        row_field, row_left, row_right = 0, width, -1
        for x in range(width):
            px = rgb[row + x * 3 : row + x * 3 + 3]
            if near(px, colors.lock, 2):
                ground += 1
            elif near(px, colors.lock_field, 2):
                row_field += 1
                row_left, row_right = min(row_left, x), max(row_right, x)
            elif near(px, colors.desktop, 1):
                desktop += 1
            elif near(px, CONSOLE, 1):
                console += 1
            elif near(px, colors.accent, 24):
                ring += 1
            elif near(px, colors.refused, 24):
                red += 1
        # a row of the field has a long run of its gray. the edges of the text above and under it
        # pass through that gray in a few pixels
        if row_field >= 100:
            field += row_field
            left, right = min(left, row_left), max(right, row_right)
            top, bottom = min(top, y), max(bottom, y)
    total = width * height
    # the inside of the field is the field less its ring. lens's field would stretch the box to
    # the top of the screen
    inner = (LOCK_FIELD_SIZE[0] - 2 * LOCK_RING, LOCK_FIELD_SIZE[1] - 2 * LOCK_RING)
    box = (right - left + 1, bottom - top + 1) if right >= 0 else (0, 0)
    scale = max(1, round(box[0] / inner[0]))
    ring_wanted = 2 * (LOCK_FIELD_SIZE[0] + LOCK_FIELD_SIZE[1]) * LOCK_RING * scale * scale
    checks = [
        ("the lock screen's gray covers the screen", ground >= 0.95 * total, f"{ground} of {total}"),
        ("nothing of the desktop", desktop <= 0.002 * total, f"{desktop} desktop gray pixels"),
        ("nothing of the console", console <= 0.002 * total, f"{console} console gray pixels"),
        ("the field is in the middle", right >= 0 and abs((left + right) / 2 - width / 2) <= 4 * scale
         and abs((top + bottom) / 2 - height / 2) <= 4 * scale,
         f"from {left},{top} to {right},{bottom} on {width}x{height}"),
        ("the field is as big as the lock screen draws it", abs(box[0] - inner[0] * scale) <= 4 * scale
         and abs(box[1] - inner[1] * scale) <= 4 * scale and field >= 0.8 * box[0] * box[1],
         f"{box[0]}x{box[1]}, {field} field pixels, expected about {inner[0] * scale}x{inner[1] * scale}"),
        ("the blue ring is around it", 0.6 * ring_wanted <= ring <= 1.5 * ring_wanted,
         f"{ring}, expected about {ring_wanted}"),
    ]
    if refused:
        checks.append(("the sentence under the field says the password was refused", red >= 100, f"{red} red pixels"))
    else:
        checks.append(("no sentence about a refused password", red <= 20, f"{red} red pixels"))
    lines = [f"lock: {width}x{height}"]
    ok = True
    for name, passed, detail in checks:
        lines.append(f"lock: {'ok  ' if passed else 'FAIL'} {name}: {detail}")
        ok = ok and passed
    return ok, lines


def luminance(px):
    return (px[0] + px[1] + px[2]) / 3


def lock_name_width(width, height, rgb, colors=DARK_COLORS):
    """How wide the owner's name is on the lock screen in a screendump, in pixels: the columns with
    text in them in the band above the field where crates/horizon-lock/src/draw.rs draws the name,
    between 32 and 12 of its pixels over the field's ring at scale 1. 0 when there is no field or no
    text there."""
    top = None
    for y in range(height // 4, height * 3 // 4):
        row = y * width * 3
        if sum(1 for x in range(0, width, 2) if near(rgb[row + x * 3:row + x * 3 + 3], colors.lock_field, 2)) >= 50:
            top = y
            break
    if top is None:
        return 0
    left, right = width, -1
    ground = luminance(colors.lock)
    for y in range(max(0, top - LOCK_RING - 32), max(0, top - LOCK_RING - 12)):
        row = y * width * 3
        for x in range(width):
            if abs(luminance(rgb[row + x * 3:row + x * 3 + 3]) - ground) > 40:
                left, right = min(left, x), max(right, x)
    return right - left + 1 if right >= left else 0


def most_common(rgb, width, left, right, top, bottom):
    """The colour that covers most of a rectangle of the screendump, and the share of it it covers."""
    counts = collections.Counter()
    for y in range(top, bottom):
        row = y * width * 3
        for x in range(left, right):
            counts[bytes(rgb[row + x * 3 : row + x * 3 + 3])] += 1
    if not counts:
        return (0, 0, 0), 0.0
    color, found = counts.most_common(1)[0]
    return tuple(color), found / sum(counts.values())


def check_apps(width, height, rgb, apps, colors=DARK_COLORS, share_wanted=0.4):
    """Find windows side by side between the bar and the dock, one for each of apps from left to right,
    each with a title bar it draws itself: a band along its top in one neutral gray of the theme that
    is not the desktop's, over share_wanted of the band, with something drawn in the right end of it,
    where the close button is. The desktop's gray is matched exactly: the dark window gray of GTK and
    Firefox, #222226, is two steps from it. Returns (ok, lines to print)."""
    bar_rows, dock_rows = bar_and_dock(width, height, bar_gray_rows(width, height, rgb, colors))
    scale = bar_rows / BAR_HEIGHT if bar_rows else 1
    ink = ink_in(width, rgb, 0, bar_rows, colors)
    checks = [
        ("the bar is along the top", 0.9 * BAR_HEIGHT <= bar_rows <= 3 * BAR_HEIGHT,
         f"{bar_rows} rows, expected about {BAR_HEIGHT} at scale 1"),
        ("the button, the clock and the icons are in it", all(count >= 30 for count in ink),
         f"{ink[0]} pixels at the left, {ink[1]} in the middle, {ink[2]} at the right"),
        ("the dock is along the bottom", 0.9 * DOCK_HEIGHT <= dock_rows <= 3 * DOCK_HEIGHT,
         f"{dock_rows} rows, expected about {DOCK_HEIGHT} at scale 1"),
    ]
    # a column of the screen is a gap between windows when the desktop shows in it all the way down the
    # middle of the working area; the focus ring around the window in front is the accent
    top_of_area, bottom_of_area = bar_rows, height - dock_rows
    probes = [round(top_of_area + (bottom_of_area - top_of_area) * part / 10) for part in range(2, 9)]
    gap = []
    for x in range(width):
        pixels = [rgb[(y * width + x) * 3 : (y * width + x) * 3 + 3] for y in probes]
        gap.append(all(near(px, colors.desktop, 1) or near(px, colors.accent, 40) for px in pixels))
    windows = []
    x = 0
    while x < width:
        if gap[x]:
            x += 1
            continue
        start = x
        while x < width and not gap[x]:
            x += 1
        if x - start >= 200 * scale:
            windows.append((start, x))
    checks.append((f"{len(apps)} windows stand side by side between the bars", len(windows) == len(apps),
                   f"windows from x {', '.join(f'{a} to {b}' for a, b in windows) or 'nowhere'}"))
    lines = [f"apps: {width}x{height}"]
    dark = colors.bar[0] < 128
    for app, (left, right) in zip(apps, windows):
        middle = (left + right) // 2
        samples = [left + (right - left) * part // 6 for part in range(1, 6)]
        top = -1
        for y in range(top_of_area, min(bottom_of_area, top_of_area + round(80 * scale))):
            drawn = [rgb[(y * width + sx) * 3 : (y * width + sx) * 3 + 3] for sx in samples]
            if sum(1 for px in drawn if not near(px, colors.desktop, 1) and not near(px, colors.accent, 40)) >= 4:
                top = y
                break
        if top < 0:
            checks.append((f"{app}'s window has a top edge", False, f"nothing but the desktop under the bar at x {middle}"))
            continue
        inset = round(12 * scale)
        fill, share = most_common(rgb, width, left + inset, right - inset, top + round(3 * scale), top + round(30 * scale))
        neutral = max(fill) - min(fill) <= 10
        shade = 12 <= luminance(fill) <= 90 if dark else 180 <= luminance(fill) <= 255
        close = 0
        for y in range(top + round(4 * scale), top + round(42 * scale)):
            row = y * width * 3
            for x in range(right - round(56 * scale), right - round(4 * scale)):
                if abs(luminance(rgb[row + x * 3 : row + x * 3 + 3]) - luminance(fill)) >= 80:
                    close += 1
        checks += [
            (f"{app}'s title bar is one gray of the theme along the top of its window",
             neutral and shade and share >= share_wanted and not near(fill, colors.desktop, 1),
             f"from x {left} to {right}, top {top}: #{bytes(fill).hex()} over {share:.0%} of its first rows"),
            (f"{app}'s title bar has its close button at the right", close >= 12 * scale * scale,
             f"{close} pixels drawn in its right end"),
        ]
    return report("apps", lines, checks)


def check_welcome(width, height, rgb):
    """Welcome's start page, which fills its window: horizon floats the window in the middle of the
    screen, so a column a little in from its left edge runs down the gray of its header bar and then
    the page's for most of the window's height."""
    x = (width - WELCOME_SIZE[0]) // 2 + 40
    header = page = 0
    for y in range(height):
        px = rgb[(y * width + x) * 3:(y * width + x) * 3 + 3]
        if near(px, WELCOME_HEADER, 2):
            header += 1
        elif near(px, WELCOME_PAGE, 2):
            page += 1
    checks = [
        ("the header bar's gray is in the column", header >= 30, f"{header} rows"),
        ("and the page's under it", page >= WELCOME_SIZE[1] // 2, f"{page} rows"),
    ]
    return report("welcome", [f"welcome: a column at x={x} of {width}x{height}"], checks)


def screendump(qmp_path, work, name):
    """Take a screendump through the monitor and return (width, height, rgb)."""
    ppm = os.path.join(work, name + ".ppm")
    qmp(qmp_path, {"execute": "screendump", "arguments": {"filename": ppm}})
    return read_ppm(ppm)


def point(qmp_path, size, at):
    """Move the pointer to a point of the screen through the monitor. The tablet's absolute axes run
    over the whole screen, so a point read off a screendump is a point on it. size and at are
    (width, height) and (x, y)."""
    def axis(name, value, whole):
        return {"type": "abs", "data": {"axis": name, "value": round(value * 0x7FFF / whole)}}

    qmp(qmp_path, {"execute": "input-send-event",
                   "arguments": {"events": [axis("x", at[0], size[0]), axis("y", at[1], size[1])]}})


def click(qmp_path, size, at, button="left"):
    """Click at a point of the screen through the monitor."""
    def press(down):
        return {"execute": "input-send-event",
                "arguments": {"events": [{"type": "btn", "data": {"down": down, "button": button}}]}}

    point(qmp_path, size, at)
    # the compositor takes the motion first, then the button, or the click lands where the pointer was
    time.sleep(0.3)
    qmp(qmp_path, press(True), press(False))
    # and the button before whatever moves the pointer next: iced reads the cursor once for the events
    # that reach a surface together, so a leave that comes with the release takes the click away
    time.sleep(0.3)


class Tee:
    def __init__(self, path):
        self.file = open(path, "w", encoding="utf-8", errors="replace")

    def write(self, data):
        self.file.write(data)
        sys.stdout.write(data)

    def flush(self):
        self.file.flush()
        sys.stdout.flush()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("vm", help="the rift-vm program from nix build .#vm")
    ap.add_argument("image", help="the image, .raw or .raw.zst, that rift-flash writes onto the drive the vm boots")
    ap.add_argument("passfile")
    ap.add_argument("--models", help="directory with gguf files for the models subvolume, enables the quasar check")
    ap.add_argument("--exchange", help="give the drive an exchange partition of this size, like 1G, and check it")
    ap.add_argument("--first-boot", action="store_true", help="write the drive without persist, choose the passphrase "
                    "at its first boot, check what it made and boot it again")
    ap.add_argument("--timeout", type=int, default=600, help="seconds for the whole test")
    ap.add_argument("--quasar-timeout", type=int, default=120, help="seconds for quasar to load the model")
    ap.add_argument("--answer-timeout", type=int, default=240, help="seconds for quasar's answer to reach the field")
    ap.add_argument("--log", default="serial.log")
    ap.add_argument("--memory", default="4096")
    ap.add_argument("--qmp", help="unix socket for the qemu monitor")
    ap.add_argument("--splash", help="take a screendump at the luks prompt, check it, save it as this png")
    ap.add_argument("--style", choices=("text", "graphical"), default="text",
                    help="the boot style to check the splash for: text, the default, or graphical, which the drive "
                    "gets through the kernel command line")
    ap.add_argument("--splash-only", action="store_true", help="end once the shell is up after the splash")
    ap.add_argument("--boot-style", action="store_true",
                    help="set the boot style from the Appearance page, reboot, and check the splash the next boot "
                         "draws, both ways. Needs --splash: the two screendumps are saved beside that png")
    ap.add_argument("--desktop", help="take a screendump of the session, check it, save it as this png")
    ap.add_argument("--desktop-timeout", type=int, default=60, help="seconds for horizon to paint its first frame")
    ap.add_argument("--lens", action="store_true", help="expect lens's bar on the desktop")
    ap.add_argument("--updates", help="an ext4 image labelled updates with a newer version's update files, "
                    "install them and reboot into that version")
    ap.add_argument("--backup", help="an empty ext4 image labelled backup, back up home onto it and restore from it")
    ap.add_argument("--clone", help="an empty file of at least 24G, clone the drive onto it as a removable disk "
                    "and boot the clone")
    ap.add_argument("--stick", help=f"an image holding one file system labelled {DRIVE_LABEL}, attach it as a "
                    "removable disk and mount it, copy onto it and eject it from Files")
    ap.add_argument("--locked", help=f"an image holding one luks2 volume labelled {DRIVE_LOCKED} with a file "
                    f"system labelled {DRIVE_INSIDE} in it, attach it as a removable disk and unlock it from Files")
    ap.add_argument("--flatpak", help="the directory nix build .#test-flatpak makes, with a signed repository and its "
                    "key: add it as a remote, install its app from Welcome and run it with the portals")
    ap.add_argument("--offline", help="boot with no network card, check Welcome opens on the page that says so, save "
                    "its screendump as this png, and that it opens again after Open later and a reboot")
    args = ap.parse_args()
    if args.boot_style and not args.splash:
        ap.error("--boot-style needs --splash, which names the png its screendumps are saved beside")
    with open(args.passfile, encoding="utf-8") as f:
        passphrase = f.read()

    work = tempfile.mkdtemp(prefix="rift-boot-")
    if (args.splash or args.desktop or args.updates or args.first_boot or args.boot_style or args.offline) \
            and not args.qmp:
        args.qmp = os.path.join(work, "qmp.sock")

    # the app picks kvm or tcg and the firmware. what follows its options replaces its defaults.
    # the gpu is virtio: the firmware draws the splash on it and horizon opens it as a drm device.
    # with --first-boot rift-flash leaves persist out and the drive asks for the passphrase
    drive = ["--first-boot"] if args.first_boot else ["--persist", os.path.abspath(args.passfile)]
    if args.models:
        drive += ["--models", os.path.abspath(args.models)]
    if args.exchange:
        drive += ["--exchange", args.exchange]
    cmd = [
        os.path.abspath(args.vm),
        "--image", os.path.abspath(args.image),
        *drive,
        "-smp", "2",
        "-m", args.memory,
        "-device", "virtio-vga",
        # an absolute pointer, so a click can be sent to a point of the screendump through the
        # monitor. the emulated ps/2 mouse only moves by so much at a time
        "-device", "virtio-tablet-pci",
        # a sound card that plays nowhere, so pipewire has a sink: the bar's volume icon and the
        # system menu's slider need one, and the vm has no sound hardware otherwise
        "-audiodev", "none,id=quiet",
        "-device", "intel-hda",
        "-device", "hda-output,audiodev=quiet",
        "-display", "none",
        "-monitor", "none",
        "-serial", "stdio",
        "-no-reboot",
        # qemu's user network. the vm reaches the host's loopback at 10.0.2.2, where the network switch
        # step runs a server of its own. with --offline the vm has no network card at all
        "-nic", "none" if args.offline else "user,model=virtio-net-pci",
    ]
    if args.qmp:
        cmd += ["-qmp", f"unix:{args.qmp},server,nowait"]
    if args.style == "graphical":
        # systemd-stub adds this SMBIOS string to the kernel command line of a drive booted without
        # secure boot, and plymouth.splash picks the theme
        cmd += ["-smbios", "type=11,value=io.systemd.stub.kernel-cmdline-extra=plymouth.splash=liftoff-graphical"]
    if args.updates:
        # a second nvme drive. nothing on the system mounts it, the test does
        cmd += ["-drive", f"if=none,id=updates,format=raw,file={os.path.abspath(args.updates)}",
                "-device", "nvme,drive=updates,serial=updates"]
    if args.backup:
        # and one for backups. vault mounts it by the uuid of its file system
        cmd += ["-drive", f"if=none,id=backup,format=raw,file={os.path.abspath(args.backup)}",
                "-device", "nvme,drive=backup,serial=backup"]
    if args.clone or args.stick or args.locked:
        # a scsi controller for the disks that say they are removable, the way a stick in a card
        # reader does
        cmd += ["-device", "virtio-scsi-pci,id=scsi"]
    if args.clone:
        # the disk the clone goes onto, since vault clones onto nothing else. zeros written to it stay
        # holes in the file
        cmd += ["-drive", f"if=none,id=clone,format=raw,discard=unmap,detect-zeroes=unmap,file={os.path.abspath(args.clone)}",
                "-device", "scsi-hd,bus=scsi.0,drive=clone,serial=clone,removable=on"]
    if args.stick:
        # and a memory stick with a file system on it, which Files mounts through udisks
        cmd += ["-drive", f"if=none,id=stick,format=raw,file={os.path.abspath(args.stick)}",
                "-device", "scsi-hd,bus=scsi.0,drive=stick,serial=stick,removable=on"]
    if args.locked:
        # and an encrypted stick, which Files unlocks with the passphrase typed into its dialog
        cmd += ["-drive", f"if=none,id=locked,format=raw,file={os.path.abspath(args.locked)}",
                "-device", "scsi-hd,bus=scsi.0,drive=locked,serial=locked,removable=on"]
    print("boot-test: " + " ".join(cmd), flush=True)

    start = time.monotonic()
    deadline = start + args.timeout
    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", dimensions=(40, 160))
    # the clone boots in a second qemu, whose output goes on in the same log
    tee = Tee(args.log)
    child.logfile_read = tee

    def since():
        return f"{time.monotonic() - start:.0f}s"

    def fail(why):
        print(f"\nboot-test: FAILED after {since()}: {why}", flush=True)
        print(f"boot-test: the serial log is in {args.log}", flush=True)
        child.terminate(force=True)
        sys.exit(1)

    def expect(patterns, what):
        try:
            return child.expect(patterns, timeout=max(1, deadline - time.monotonic()))
        except pexpect.TIMEOUT:
            fail(f"timed out waiting for {what}")
        except pexpect.EOF:
            fail(f"qemu exited while waiting for {what}")

    def ok(what):
        print(f"\nboot-test: {what} at {since()}", flush=True)

    def run(command, what):
        """Run one command line in the serial shell. Returns its exit status and what it printed,
        without escape codes and carriage returns."""
        child.send(command + "\r")
        expect([COMMAND_START], f"the shell to start {what}")
        expect([COMMAND_END], what)
        status = int(child.match.group(1))
        return status, ESCAPES.sub("", child.before).replace("\r", "")

    def unlock():
        """Answer the luks prompt that is up and wait for the autologin shell."""
        child.send(passphrase + "\r")
        for attempt in range(3):
            if expect([PROMPT, PASSPHRASE], "the autologin shell") == 0:
                break
            if attempt == 2:
                fail("the passphrase was refused three times")
            print("\nboot-test: passphrase prompt again, retrying", flush=True)
            child.send(passphrase + "\r")
        ok("shell")

    def reboot_action(action):
        """What qemu does when the guest reboots. -no-reboot ends it, which is right for the last
        boot of a run; a reboot the test goes on after needs a reset."""
        try:
            qmp(args.qmp, {"execute": "set-action", "arguments": {"reboot": action}})
        except (OSError, RuntimeError) as e:
            fail(f"qmp set-action reboot={action}: {e}")

    def power_off():
        """Shut the vm down from the shell and wait for qemu to go."""
        child.send("sudo systemctl poweroff\r")
        try:
            child.expect(pexpect.EOF, timeout=90)
        except pexpect.TIMEOUT:
            print("\nboot-test: poweroff did not end qemu, killing it", flush=True)
            child.terminate(force=True)

    def choose():
        """Answer the first boot's questions for a new passphrase: one too short, two that differ, then
        the passphrase twice. The drive makes persist, opens it and goes on to the autologin shell
        without asking again."""
        # a person takes a while to choose a passphrase. systemd gives up on a device after 90 s, and
        # the persist partition only comes once the passphrase is in
        print("\nboot-test: waiting 100 s before answering, as a person choosing a passphrase would", flush=True)
        time.sleep(100)
        child.send("short77\r")
        expect([rf"at least 8 characters\. {CHOOSE}"], "the question again after a passphrase that is too short")
        child.send(passphrase + "\r")
        expect([AGAIN], "the question to type the passphrase again")
        child.send(passphrase + "-other\r")
        expect([rf"not the same\. {CHOOSE}"], "the question again after two passphrases that differ")
        child.send(passphrase + "\r")
        expect([AGAIN], "the question to type the passphrase again")
        child.send(passphrase + "\r")
        if expect([PROMPT, CHOOSE, PASSPHRASE], "the autologin shell after the first boot made persist") != 0:
            fail("the first boot asked for a passphrase again after it had one")
        ok("shell, after the first boot refused a short passphrase and two that differ and made persist")

    def welcome_said(what):
        """What rift-welcome --state prints, a line each, or None while no Welcome answers."""
        status, output = run("rift-welcome --state", what)
        if status != 0:
            return None
        return [printed.strip() for printed in without_console(output).splitlines() if printed.strip()]

    def welcome_value(lines, key):
        """The rest of the first line that starts with this word."""
        for printed in lines or []:
            word, _, rest = printed.partition(" ")
            if word == key:
                return rest.strip()
        return None

    def welcome_until(seconds, ready, what):
        """Ask rift-welcome --state until ready(lines) is true, and answer those lines."""
        until = time.monotonic() + seconds
        while True:
            lines = welcome_said(what)
            if lines is not None and ready(lines):
                return lines
            if time.monotonic() > until:
                _, output = run("journalctl -b -t welcome -o cat --no-pager | tail -n 20", "Welcome's log")
                fail(f"Welcome did not come to {what} in {seconds} s, its state is {lines!r}"[:1500]
                     + f". It logged: {without_console(output).strip()[-1000:]!r}")
            time.sleep(2)

    def welcome_picture(png, name, check=False):
        """A screendump of Welcome as it stands, saved as png. With check, taken again until the
        start page's window is drawn in the middle of the screen."""
        until = time.monotonic() + 60
        while True:
            try:
                width, height, rgb = screendump(args.qmp, work, name)
            except (OSError, RuntimeError) as e:
                fail(f"screendump: {e}")
            good, lines = check_welcome(width, height, rgb) if check else (True, [])
            if good or time.monotonic() > until:
                break
            time.sleep(2)
        write_png(png, width, height, rgb)
        if lines:
            print("\nboot-test: " + "\nboot-test: ".join(lines), flush=True)
        if not good:
            fail(f"Welcome's window is not drawn in the middle of the screen, see {png}")

    def welcome_gone(what):
        """Wait for Welcome to end: its socket stops answering."""
        until = time.monotonic() + 60
        while welcome_said(f"whether Welcome is still there after {what}") is not None:
            if time.monotonic() > until:
                fail(f"Welcome is still running after {what}")
            time.sleep(2)

    def welcome_offline(what, png):
        """Welcome on the page that says there is no network, as the session opened it."""
        lines = welcome_until(300, lambda lines: welcome_value(lines, "page") == "offline",
                              f"the page with no network on {what}")
        if welcome_value(lines, "network") != "offline" or welcome_value(lines, "welcomed") != "no":
            fail(f"Welcome says network {welcome_value(lines, 'network')!r} and welcomed "
                 f"{welcome_value(lines, 'welcomed')!r} on {what}, with no network card")
        if png:
            welcome_picture(png, "welcome-offline", check=True)

    # 1. the luks prompt, answered over serial. a second prompt means the passphrase was refused. a
    # drive written with --first-boot asks for a new passphrase instead
    if args.first_boot:
        expect([CHOOSE], "the first boot's question for a new passphrase")
        ok("the first boot asks for a new passphrase")
    else:
        expect([PASSPHRASE], "the luks passphrase prompt")
        ok("passphrase prompt")

    # 1a. the splash. whatever asks waits for us, so the screen is stable
    if args.splash:
        time.sleep(3)
        try:
            width, height, rgb = screendump(args.qmp, work, "splash")
        except (OSError, RuntimeError) as e:
            fail(f"screendump: {e}")
        write_png(args.splash, width, height, rgb)
        check = check_splash if args.style == "graphical" else check_text_splash
        good, lines = check(width, height, rgb)
        print("\nboot-test: " + "\nboot-test: ".join(lines), flush=True)
        if not good:
            fail(f"the splash is not on screen, see {args.splash}")
        ok("splash")

    if args.first_boot:
        choose()
    else:
        unlock()

    if args.splash_only:
        power_off()
        print(f"\nboot-test: PASSED in {since()}", flush=True)
        return

    # 1c. the boot style. the Appearance page writes a word onto the esp, liftoff-style reads it there
    # in the initrd before plymouthd starts, and the boot after it draws the style it names. The splash
    # checked above is the text one, which is what a drive whose owner has chosen nothing draws
    if args.boot_style:
        stem, extension = os.path.splitext(args.splash)

        def waited(seconds, ready):
            """Poll until ready() answers something, or give up and answer what it last said."""
            until = time.monotonic() + seconds
            while True:
                found = ready()
                if found or time.monotonic() > until:
                    return found
                time.sleep(2)

        def page_state(what):
            """What rift-settings --state prints, as a dict of the words it knows. The boot style is
            only in it once Vault has answered, since Vault is the one that reads the esp."""
            status, output = run("rift-settings --state", what)
            if status != 0:
                return {}
            state = {}
            for printed in without_console(output).splitlines():
                key, _, value = printed.strip().partition(" ")
                if key in SETTINGS_KEYS:
                    state[key] = value.strip()
            return state

        def open_settings():
            """Start Settings in the session and wait for its socket to answer."""
            if not waited(args.desktop_timeout + 120,
                          lambda: run("lens --state", "the shell")[0] == 0 or None):
                fail("the shell does not answer, so Settings has no screen to open on")
            run("systemd-run --user --quiet --collect rift-settings", "the Settings window")
            if not waited(120, lambda: page_state("the page Settings opens on") or None):
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"rift-settings --state answers nothing: {without_console(output).strip()[-800:]!r}")

        def set_style(style):
            """Set the boot style from the Appearance page, and read the word back off the esp."""
            status, output = run(f"rift-settings --set boot {style}", f"the boot style set to {style}")
            if status != 0:
                fail(f"rift-settings --set boot {style} exited with {status}: "
                     f"{without_console(output).strip()[-300:]!r}")
            if not waited(120, lambda: page_state("the boot style").get("boot") == style):
                said = page_state("the boot style").get("boot")
                fail(f"rift-settings --state says boot {said!r} after the page was set to {style}")
            status, output = run(f"sudo cat /boot/{BOOT_STYLE_FILE}", "the word on the esp")
            written = without_console(output).strip()
            if status != 0 or written != style:
                fail(f"/boot/{BOOT_STYLE_FILE} holds {written!r} after the page was set to {style}")
            ok(f"the Appearance page set the boot style to {style}, and the drive's esp holds the word")

        def choose_zone(zone):
            """Set the time zone from the Date and time page, and read it back from timedated."""
            status, output = run(f"rift-settings --set timezone {zone}", f"the time zone set to {zone}")
            if status != 0:
                fail(f"rift-settings --set timezone {zone} exited with {status}: "
                     f"{without_console(output).strip()[-300:]!r}")
            if not waited(60, lambda: page_state("the time zone").get("timezone") == zone):
                said = page_state("the time zone").get("timezone")
                fail(f"rift-settings --state says timezone {said!r} after the page was set to {zone}")
            _, output = run("timedatectl show -p Timezone --value", "the zone timedated has")
            if zone not in without_console(output).split():
                fail(f"timedatectl says {without_console(output).strip()!r} after the page set {zone}")
            ok(f"the Date and time page set the time zone to {zone}")

        def zone_kept(zone, names):
            """Check the boot that follows is in the zone the page chose before it."""
            _, output = run("timedatectl show -p Timezone --value; date +%Z", "the zone after the reboot")
            said = without_console(output).split()
            if zone not in said or not any(name in said for name in names):
                fail(f"timedatectl and date say {said!r} after the reboot, and the page chose {zone} before it")
            ok(f"the boot after {zone} was chosen is in {zone}, which date calls {said[-1]}")

        def localed_layout(what):
            """The layouts localed has, as it keeps them: us,gb."""
            _, output = run("busctl --system get-property org.freedesktop.locale1 /org/freedesktop/locale1 "
                            "org.freedesktop.locale1 X11Layout | cat", what)
            return "".join(re.findall(r'"([^"]*)"', without_console(output)))

        def change_layouts(asked, wanted):
            """Add or take off a layout on the Keyboard page, and read the layouts back from localed."""
            run("rift-settings --page keyboard", "the Keyboard page")
            if not waited(30, lambda: page_state("the Keyboard page").get("page") == "keyboard"):
                fail("rift-settings --page keyboard did not show that page")
            if not waited(30, lambda: page_state("the layouts on the page").get("layouts")):
                fail("the Keyboard page says nothing about the layouts, and localed is there to ask")
            status, output = run(f"rift-settings --set {asked}", f"the layouts: {asked}")
            if status != 0:
                fail(f"rift-settings --set {asked} exited with {status}: {without_console(output).strip()[-300:]!r}")
            if not waited(60, lambda: localed_layout(f"the layouts after {asked}") == wanted):
                fail(f"localed has layouts {localed_layout('the layouts again')!r} after the page's {asked}, "
                     f"expected {wanted}")
            ok(f"the Keyboard page's {asked} left localed with layouts {wanted}")

        def layouts_kept(wanted, names=None):
            """Check the boot that follows has the layouts the page chose before it: localed reads them
            off persist, and horizon, when the session is up, has them by name."""
            if localed_layout("the layouts after the reboot") != wanted:
                fail(f"localed has layouts {localed_layout('the layouts after the reboot again')!r} after the "
                     f"reboot, and the page chose {wanted} before it")
            if names:
                def in_horizon():
                    _, output = run("set -x NIRI_SOCKET (ls -t /run/user/(id -u)/niri.wayland-1.*.sock | head -n1); "
                                    "horizon msg --json keyboard-layouts", "horizon's layouts after the reboot")
                    found = re.search(r'"names":\[(.*?)\]', without_console(output).replace("\n", ""))
                    return found and re.findall(r'"([^"]*)"', found.group(1)) == names
                if not waited(60, in_horizon):
                    fail(f"horizon does not have the layouts {names} after the reboot")
            ok(f"the boot after the layouts were chosen has {wanted}" + (" in localed and horizon" if names else ""))

        def change_owner(name, current, new):
            """Give the owner a name and a password on the Owner page, and wait for the page to say
            Vault has both."""
            run("rift-settings --page owner", "the Owner page")
            if not waited(30, lambda: page_state("the Owner page").get("page") == "owner"):
                fail("rift-settings --page owner did not show that page")
            if not waited(30, lambda: page_state("the owner on the page").get("owner-user") == OWNER_USER):
                fail(f"the Owner page says owner-user {page_state('the owner again').get('owner-user')!r}")
            run(f"rift-settings --set owner-name {name}", f"the owner's name set to {name}")
            if not waited(60, lambda: page_state("the owner's name").get("owner-name") == name):
                fail(f"the Owner page says {page_state('the owner once more')} after it set the name {name!r}")
            password_word = "image" if new == PASSWORD else "own"
            run(f"rift-settings --set owner-password {current} {new}", "the owner's password")
            if not waited(60, lambda: page_state("the owner's password").get("owner-password") == password_word):
                fail(f"the Owner page says {page_state('the owner once more')} after it set a password, expected "
                     f"owner-password {password_word}")
            ok(f"the Owner page named the owner {name!r} and gave them {'the image' if new == PASSWORD else 'a new'} "
               "password")

        def owner_session():
            """The session greetd opened, which logind locks."""
            _, told = run("for s in (loginctl list-sessions --no-legend | string trim | string split -f1 ' '); "
                          "if test (loginctl show-session $s -p Service --value) = greetd; echo session=$s; end; end",
                          "the session greetd opened")
            found = re.search(r"session=(\S+)", without_console(told))
            if not found:
                fail(f"logind lists no session from greetd: {without_console(told).strip()[-300:]!r}")
            return found.group(1)

        def owner_hint(session_id, wanted, what):
            """Wait up to twenty seconds for logind's LockedHint on the session to say wanted."""
            if not waited(20, lambda: re.search(rf"^{wanted}$", without_console(run(
                    f"loginctl show-session {session_id} -p LockedHint --value", "the locked hint")[1]), re.M)):
                fail(f"logind does not say LockedHint={wanted} for session {session_id} {what}")

        def owner_types(text, what):
            """Type a line on the vm's keyboard through the monitor, one key at a time."""
            keys = [[letter] for letter in text] + [["ret"]]
            try:
                qmp(args.qmp, *({"execute": "send-key", "arguments": {
                    "keys": [{"type": "qcode", "data": code} for code in held]}} for held in keys))
            except (OSError, RuntimeError) as e:
                fail(f"typing {what}: {e}")

        def owner_kept(name, password, other):
            """Check the boot that follows has the name and the password the page chose before it:
            getent gives the name, the lock screen reads it, refuses the other password and opens
            with this one."""
            _, told = run(f"getent passwd {OWNER_USER} | cut -d: -f5", "the owner's name after the reboot")
            said = (without_console(told).strip().splitlines() or [""])[-1].strip()
            if said != name:
                fail(f"getent names the owner {said!r} after the reboot, and the page chose {name!r} before it")
            session_id = owner_session()
            status, told = run(f"loginctl lock-session {session_id}", "the lock screen after the reboot")
            if status != 0:
                fail(f"loginctl lock-session {session_id} exited with {status}: {without_console(told).strip()!r}")
            owner_hint(session_id, "yes", "with the lock screen up after the reboot")
            _, told = run("journalctl -b -t lock -o cat --no-pager | grep 'locking the screen for' | tail -n 1",
                          "the name the lock screen read")
            if f"locking the screen for {name}" not in without_console(told):
                fail(f"the lock screen after the reboot says {without_console(told).strip()[-200:]!r}, expected {name!r}")
            time.sleep(1)
            owner_types(other, "a password that is not the owner's")
            # pam holds a wrong password for about two seconds before the screen says so
            time.sleep(5)
            owner_hint(session_id, "yes", "after a password that is not the owner's")
            owner_types(password, "the owner's password")
            owner_hint(session_id, "no", "after the owner's password")
            ok(f"the boot after the owner was named {name!r} has the name in getent and on the lock screen, which "
               f"refused {other!r} and opened with {password!r}")

        def next_boot(style):
            """Reboot, and check the splash of the boot that follows is the style that was chosen."""
            png = f"{stem}-{style}{extension}"
            reboot_action("reset")
            child.send("sudo systemctl reboot\r")
            expect([PASSPHRASE], f"the passphrase prompt of the boot after {style} was chosen")
            ok(f"passphrase prompt of the boot after {style} was chosen")
            time.sleep(3)
            try:
                width, height, rgb = screendump(args.qmp, work, f"boot-style-{style}")
            except (OSError, RuntimeError) as e:
                fail(f"screendump: {e}")
            write_png(png, width, height, rgb)
            check = check_splash if style == "graphical" else check_text_splash
            good, lines = check(width, height, rgb)
            print("\nboot-test: " + "\nboot-test: ".join(lines), flush=True)
            if not good:
                fail(f"the boot after {style} was chosen does not draw it, see {png}")
            ok(f"the boot after {style} was chosen draws the {style} splash")

        # graphical, which is not the style the image was built with, then text again
        layout_added, layout_added_name = SETTINGS_LAYOUT
        open_settings()
        set_style("graphical")
        choose_zone(SETTINGS_ZONE[0])
        change_layouts(f"add-layout {layout_added}", f"us,{layout_added}")
        change_owner(OWNER_NEW_NAME, PASSWORD, OWNER_NEW_PASSWORD)
        next_boot("graphical")
        unlock()
        zone_kept(*SETTINGS_ZONE)
        open_settings()
        layouts_kept(f"us,{layout_added}", ["English (US)", layout_added_name])
        owner_kept(OWNER_NEW_NAME, OWNER_NEW_PASSWORD, PASSWORD)
        set_style("text")
        choose_zone("UTC")
        change_layouts(f"remove-layout {layout_added}", "us")
        change_owner(OWNER_NAME, OWNER_NEW_PASSWORD, PASSWORD)
        next_boot("text")
        unlock()
        zone_kept("UTC", ("UTC",))
        layouts_kept("us")
        open_settings()
        owner_kept(OWNER_NAME, PASSWORD, OWNER_NEW_PASSWORD)
        reboot_action("shutdown")
        power_off()
        print(f"\nboot-test: PASSED in {since()}", flush=True)
        return

    # 2. the system is ours
    child.send("rift --version\r")
    expect([r"Rift \d+\.\d+\.\d+"], "rift --version output")
    version = child.after
    expect([PROMPT], "the prompt")

    child.send("echo phase=(cat /etc/rift/phase)\r")
    expect([r"phase=(\d+)\s"], "the phase")
    phase = child.match.group(1)
    expect([PROMPT], "the prompt")
    if phase != "0":
        fail(f"phase is {phase}, expected 0")

    child.send("findmnt -no SOURCE,FSTYPE /home\r")
    expect([r"/dev/mapper/persist\S*\s+btrfs"], "/home on persist")
    expect([PROMPT], "the prompt")
    ok(f"{version.strip()}, phase {phase}, home on persist")

    # the status lines the text boot shows name the unit in bold, then describe it. systemd has no
    # property for the format, and plymouth keeps the console it showed in /var/log/boot.log
    status, output = run("sudo grep -a -c -E 'plymouth-start[.]service.* - Show Plymouth Boot Screen' /var/log/boot.log",
                         "plymouth's log of the console")
    found = re.search(r"^\s*(\d+)\s*$", without_console(output), re.M)
    if status != 0 or not found or int(found.group(1)) < 1:
        fail("plymouth's /var/log/boot.log has no status line that names the unit and then describes it: "
             f"{without_console(output).strip()!r}")

    # 2a. the default apps are on the path, firefox has its policies, zed got its settings with
    # telemetry off, and podman runs rootless in the owner's ranges
    apps = ["firefox", "zeditor", "hx", "zellij", "ghostty", "fish", "podman", "docker"]
    _, output = run("for app in " + " ".join(apps) + "; command -q $app; or echo missing=$app; end; echo apps-done",
                    "the default apps on the path")
    missing = re.findall(r"missing=(\S+)", output)
    if missing or "apps-done" not in output:
        fail(f"not on the path: {' '.join(missing) or repr(output.strip())}")
    status, _ = run("grep -q DisableTelemetry /etc/firefox/policies/policies.json", "firefox's policies")
    if status != 0:
        fail("firefox has no policies file that turns telemetry off")
    status, output = run("cat ~/.config/zed/settings.json", "zed's settings")
    if status != 0 or '"metrics":false' not in output:
        fail(f"zed's settings do not turn telemetry off: {output.strip()!r}")
    status, output = run("podman info --format 'rootless={{.Host.Security.Rootless}}'", "podman info")
    if status != 0 or "rootless=true" not in output:
        fail(f"podman does not run rootless for the owner: {output.strip()[-600:]!r}")
    ok(f"{', '.join(apps)} on the path, firefox policies, zed settings, podman rootless")

    # 2b. the slots. systemd-boot started the uki with its boot counter, boot-complete.target was
    # reached and systemd-bless-boot took the counter off the file name, sysupdate finds this version
    # installed, /usr runs from slot a, and slot b's two partitions wait empty behind it
    def image_version():
        status, output = run("grep '^IMAGE_VERSION=' /etc/os-release", "the image version")
        found = re.search(r'^IMAGE_VERSION="?([^"\s]+)"?\s*$', without_console(output), re.M)
        if status != 0 or not found:
            fail(f"/etc/os-release has no IMAGE_VERSION: {without_console(output).strip()!r}")
        return found.group(1)

    def started_by_systemd_boot(uki):
        """Check that systemd-boot started this uki and titles every Rift entry Rift with its
        version, and return the boot loader's product name and version."""
        _, output = run("sudo bootctl status --no-pager", "bootctl status")
        printed = without_console(output)
        print(f"\nboot-test: bootctl status printed:\n{printed}", flush=True)
        loader = re.search(r"Current Boot Loader:\s*\n\s*Product:\s*(systemd-boot \S+)", printed)
        if not loader:
            fail("bootctl says this boot was not started by systemd-boot")
        entry = re.search(r"Current Entry:\s*(\S+)", printed)
        if not entry or entry.group(1) != uki:
            fail(f"systemd-boot started {entry.group(1) if entry else 'no entry'}, expected {uki}")
        # systemd-boot titles a uki from the PRETTY_NAME in it, and bootctl marks the default and
        # the selected entry after the title
        _, output = run("sudo bootctl list --no-pager", "bootctl list")
        printed = without_console(output)
        listed = re.findall(r"^\s*title:\s*(.*?)\s*\n\s*id:\s*rift_(\d+\.\d+\.\d+)[^\n]*$", printed, re.M)
        titles = [(re.sub(r"(?:\s+\([a-z/ ]+\))+$", "", title), version) for title, version in listed]
        if not titles or any(title != f"Rift {version}" for title, version in titles):
            print(f"\nboot-test: bootctl list printed:\n{printed}", flush=True)
            fail(f"systemd-boot's entries are titled {titles}, expected Rift and each one's version")
        return loader.group(1)

    def unit_state(unit):
        """What systemctl says about a unit. `--user <name>` asks the owner's manager."""
        _, output = run(f"systemctl is-active {unit}", f"the state of {unit}")
        found = re.search(r"^(active|inactive|failed|activating|deactivating)\s*$", without_console(output), re.M)
        return found.group(1) if found else without_console(output).strip()

    def assessment():
        """What systemd-bless-boot says about this boot: good, bad, indeterminate, dirty, or clean
        when the uki had no counter."""
        _, output = run("sudo /run/current-system/systemd/lib/systemd/systemd-bless-boot status",
                        "the assessment of this boot")
        found = re.search(r"^(good|bad|indeterminate|clean|dirty)\s*$", without_console(output), re.M)
        return found.group(1) if found else without_console(output).strip()

    def ukis_on_esp(wanted, what):
        _, output = run("sudo ls -1 /boot/EFI/Linux", what)
        ukis = sorted(without_console(output).split())
        if ukis != sorted(wanted):
            fail(f"the esp holds {ukis} {what}, expected {sorted(wanted)}")

    def boot_drive():
        """(name, label, type, size) of each partition on the drive this boot came from, in order."""
        _, output = run("lsblk -brno NAME,PARTLABEL,PARTTYPE,SIZE /dev/(lsblk -no PKNAME /dev/disk/by-designator/esp)",
                        "the partitions of the boot drive")
        printed = without_console(output)
        print(f"\nboot-test: lsblk printed:\n{printed}", flush=True)
        parts = [row.split() for row in printed.splitlines()]
        return [(name, label, kind.lower(), int(size)) for name, label, kind, size in (p for p in parts if len(p) == 4)]

    def usr_from(slot, store):
        _, output = run("sudo veritysetup status usr", "the verity device under /usr")
        data = re.search(r"data device:\s*(\S+)", without_console(output))
        if not data or data.group(1) != f"/dev/{store}":
            fail(f"/usr runs from {data.group(1) if data else without_console(output).strip()!r}, "
                 f"expected slot {slot} on /dev/{store}")

    def check_slots(slot="a", other=None, failed=None, counted=True):
        """Check how this boot came up on the a/b layout and return the running version. slot is the
        slot it should run from, other the version in the other slot, None while that one is empty.
        failed is a version whose uki used up its tries and keeps its counter on the esp. counted
        says whether systemd-boot counted this boot: a uki marked good on an earlier boot has no
        counter left, then nothing marks this boot and the test starts boot-complete.target itself."""
        version = image_version()
        uki = f"rift_{version}.efi"
        installed = sorted((v for v in (version, other) if v), key=version_key)

        _, output = run("ls /dev/disk/by-designator/", "udev's names for the partitions of the boot drive")
        print(f"\nboot-test: /dev/disk/by-designator holds:\n{without_console(output)}", flush=True)
        loader = started_by_systemd_boot(uki)

        if counted:
            # the boot is marked good once orbit and greetd are up, a little after the shell
            until = time.monotonic() + 120
            while True:
                state = unit_state("systemd-bless-boot")
                if state == "active":
                    break
                if state == "failed" or time.monotonic() > until:
                    _, output = run("systemctl status --no-pager systemd-bless-boot boot-complete.target",
                                    "why the boot was not marked good")
                    print(f"\nboot-test: systemctl status printed:\n{without_console(output)}", flush=True)
                    fail(f"systemd-bless-boot is {state} after {since()}, the boot was never marked good")
                time.sleep(3)
            blessed = f"the boot was marked good at {since()} and the counter is gone"
            verdict = assessment()
            if verdict != "good":
                fail(f"systemd-bless-boot says {verdict!r}, expected good")
        else:
            verdict = assessment()
            if verdict != "clean":
                fail(f"systemd-bless-boot says {verdict!r}, expected clean for a uki without a counter")
            state = unit_state("systemd-bless-boot")
            if state != "inactive":
                fail(f"systemd-bless-boot is {state} on a boot that was not counted, expected inactive")
            status, output = run("sudo timeout 120 systemctl start boot-complete.target", "boot-complete.target")
            if status != 0 or unit_state("boot-complete.target") != "active":
                fail(f"boot-complete.target could not be reached: {without_console(output).strip()!r}")
            blessed = f"its uki has no counter and boot-complete.target was reached at {since()}"
        ukis_on_esp([f"rift_{v}+0-{TRIES}.efi" if v == failed else f"rift_{v}.efi" for v in installed],
                    "with the counters of good boots gone")

        # current is the newest version installed, which is not the running one after a rollback
        _, output = run("sudo systemd-sysupdate --offline --json=short list", "systemd-sysupdate list")
        found = re.search(r'^\{"current.*\}\s*$', without_console(output), re.M)
        listing = json.loads(found.group(0)) if found else {}
        if listing.get("current") != installed[-1] or sorted(listing.get("all", []), key=version_key) != installed:
            fail(f"systemd-sysupdate lists {without_console(output).strip()[-600:]!r}, expected {installed[-1]} current "
                 f"and {', '.join(installed)} installed")

        # esp, slot a, slot b in partition order, then the exchange partition when the drive has one,
        # and persist
        parts = boot_drive()
        gib = 1024**3
        wanted = [("esp", ESP_TYPE, gib)]
        for held in ((version, other) if slot == "a" else (other, version)):
            wanted += [
                (f"store-verity_{held}" if held else "_empty", USR_VERITY_TYPE, gib),
                (f"store_{held}" if held else "_empty", USR_TYPE, 8 * gib),
            ]
        tail = (["exchange"] if args.exchange else []) + ["persist"]
        if [p[1:] for p in parts[:5]] != wanted or [p[1] for p in parts[5:]] != tail:
            fail(f"the boot drive's partitions are {[p[1:] for p in parts]}, expected {wanted} and then {', '.join(tail)}")

        store = parts[2 if slot == "a" else 4][0]
        usr_from(slot, store)
        rest = f"slot {'b' if slot == 'a' else 'a'} holds {other}" if other else "slot b is empty"
        ok(f"{loader} started {uki}, {blessed}, sysupdate lists {', '.join(installed)} installed and "
           f"{installed[-1]} current, /usr runs from slot {slot} on {store}, {rest}")
        return version

    def check_failed_boot(version, good, done):
        """Check a boot of a version whose boot check always fails, from slot a, with good in slot b.
        systemd-boot started its uki and has taken done tries off it, the check failed, nothing
        marked the boot good and the uki keeps its counter."""
        running = image_version()
        if running != version:
            fail(f"boot {done} came up running {running}, expected {version}")
        uki = f"rift_{version}.efi"
        loader = started_by_systemd_boot(uki)

        until = time.monotonic() + 120
        while (state := unit_state(NEVER_GOOD)) != "failed":
            if time.monotonic() > until:
                fail(f"{NEVER_GOOD} is {state} after {since()}, expected failed")
            time.sleep(3)
        for unit in ("boot-complete.target", "systemd-bless-boot"):
            state = unit_state(unit)
            if state != "inactive":
                fail(f"{unit} is {state} after {NEVER_GOOD} failed, expected inactive")
        # the file keeps the name systemd-boot gave it before starting it. with no tries left the
        # boot is already as bad as a counter can say
        left = TRIES - done
        verdict = assessment()
        if verdict != ("dirty" if left == 0 else "indeterminate"):
            fail(f"systemd-bless-boot says {verdict!r} on boot {done}, expected {'dirty' if left == 0 else 'indeterminate'}")
        counter = f"rift_{version}+{left}-{done}.efi"
        ukis_on_esp([f"rift_{good}.efi", counter], f"on boot {done} of {version}")

        parts = boot_drive()
        labels = [p[1] for p in parts[1:5]]
        wanted = [f"store-verity_{version}", f"store_{version}", f"store-verity_{good}", f"store_{good}"]
        if labels != wanted:
            fail(f"the slots hold {labels}, expected {wanted}")
        usr_from("a", parts[2][0])
        ok(f"{loader} started {uki} as {counter}, {NEVER_GOOD} failed and the boot was not marked good, "
           f"/usr runs from slot a on {parts[2][0]}")

    running = check_slots()

    # 2c. the drive rift-flash wrote. persist is luks2 with argon2id, the settings a person gets, and
    # its btrfs has every subvolume and the owner's home. with --exchange the exchange partition is an
    # exfat labelled EXCHANGE, as big as asked
    _, output = run("sudo cryptsetup luksDump /dev/disk/by-partlabel/persist", "the luks header of persist")
    dump = without_console(output)
    if not re.search(r"^Version:\s*2\s*$", dump, re.M) or not re.search(r"PBKDF:\s*argon2id\s*$", dump, re.M):
        fail(f"persist is not luks2 with argon2id: {dump.strip()[-600:]!r}")
    _, output = run("sudo btrfs subvolume list /persist", "the subvolumes of persist")
    found = re.findall(r"\spath (@\w+)\s*$", without_console(output), re.M)
    missing = [name for name in ("@home", "@var", "@flatpak", "@models", "@hosts", "@snapshots") if name not in found]
    if missing:
        fail(f"persist has no {', '.join(missing)}: {without_console(output).strip()!r}")
    _, output = run("stat -c home=%U:%G /home/rift", "the owner's home")
    if "home=rift:users" not in output:
        fail(f"/home/rift is not the owner's: {without_console(output).strip()!r}")
    exchange_bytes = None
    if args.exchange:
        unit = {"G": 1024**3, "M": 1024**2}[args.exchange[-1].upper()]
        exchange_bytes = int(args.exchange[:-1]) * unit
        _, output = run("sudo blkid -p -o export /dev/disk/by-partlabel/exchange; and sudo blockdev --getsize64 "
                        "/dev/disk/by-partlabel/exchange", "the exchange partition")
        found = without_console(output)
        if not re.search(r"^TYPE=exfat\s*$", found, re.M) or not re.search(r"^LABEL=EXCHANGE\s*$", found, re.M) \
                or not re.search(rf"^{exchange_bytes}\s*$", found, re.M):
            fail(f"the exchange partition is not an exfat of {exchange_bytes} bytes labelled EXCHANGE: {found.strip()!r}")
    maker = "the first boot" if args.first_boot else "rift-flash"
    ok(f"{maker} made persist luks2 with argon2id, every subvolume and the owner's home"
       + (f", and an exfat exchange partition of {args.exchange}" if args.exchange else ""))

    # 2e. the system says Rift. os-release names it, keeps IMAGE_ID and IMAGE_VERSION the way
    # sysupdate and the clone read them, and says NixOS only in ID_LIKE. hostnamectl and lsb-release
    # say the same, rift --version --logo prints the logo over the name, /etc/issue has the name
    # line without the logo, and fastfetch shows the logo and the name
    logo = logo_lines()
    status, output = run("cat /etc/os-release", "/etc/os-release")
    release = dict(re.findall(r'^([A-Z_]+)="?([^"\n]*)"?\s*$', without_console(output), re.M))
    wanted = {"NAME": "Rift", "ID": "rift", "ID_LIKE": "nixos", "IMAGE_ID": "rift",
              "IMAGE_VERSION": running, "VERSION_ID": running, "PRETTY_NAME": f"Rift {running}",
              "LOGO": "rift-logo", "ANSI_COLOR": "38;2;93;172;217"}
    wrong = {key: release.get(key) for key, value in wanted.items() if release.get(key) != value}
    if status != 0 or wrong:
        fail(f"/etc/os-release has {wrong}, expected {wanted}: {release}")
    said_nixos = [key for key, value in release.items() if key != "ID_LIKE" and "nixos" in value.lower()]
    if said_nixos:
        fail(f"/etc/os-release says NixOS in {', '.join(said_nixos)}: {release}")
    _, output = run("hostnamectl", "hostnamectl")
    printed = without_console(output)
    print(f"\nboot-test: hostnamectl printed:\n{printed}", flush=True)
    for label, value in (("Operating System", f"Rift {running}"), ("OS Image", "rift"),
                         ("OS Image Version", running)):
        if not re.search(rf"^\s*{label}:\s*{re.escape(value)}\s*$", printed, re.M):
            fail(f"hostnamectl does not say {label}: {value}")
    _, output = run("grep '^DISTRIB_DESCRIPTION=' /etc/lsb-release", "lsb-release")
    if f'DISTRIB_DESCRIPTION="Rift {running}"' not in without_console(output):
        fail(f"/etc/lsb-release does not say Rift {running}: {without_console(output).strip()!r}")
    ok(f"os-release, hostnamectl and lsb-release say Rift {running}, IMAGE_ID is {release['IMAGE_ID']} "
       f"and IMAGE_VERSION {release['IMAGE_VERSION']}")

    status, output = run("rift --version --logo | cat", "rift --version --logo")
    printed = "\n".join(line.rstrip() for line in without_console(output).split("\n"))
    if status != 0 or "\n".join(logo) + f"\n\nRift {running}" not in printed:
        fail(f"rift --version --logo printed {printed!r}, expected the logo with Rift {running} under it")
    _, output = run("cat /etc/issue", "/etc/issue")
    issue = without_console(output)
    # the logo is 110 columns and a text console can be 80, so /etc/issue has the name line and no logo
    if "\\S{PRETTY_NAME} \\r (\\l)" not in issue or logo[0].strip() in issue or "\\e[" in issue:
        fail(f"/etc/issue should have the name line and no logo: {issue!r}")
    started = time.monotonic()
    status, output = run("fastfetch --pipe", "fastfetch")
    took = time.monotonic() - started
    # a raw logo file keeps its colours even in a pipe
    printed = ESCAPES.sub("", without_console(output))
    print(f"\nboot-test: fastfetch --pipe printed in {took:.1f}s:\n{printed}", flush=True)
    if status != 0 or not printed.startswith(logo[0]):
        fail(f"fastfetch does not start with the logo's first line {logo[0]!r}")
    if not re.search(rf"\bOS: Rift {re.escape(running)}\b", printed):
        fail(f"fastfetch does not say OS: Rift {running}")
    missing = [key for key in ("Host class", "AI tier", "Last snapshot") if f"{key}: " not in printed]
    if missing:
        fail(f"fastfetch shows no {', '.join(missing)}")
    ok(f"rift --version --logo and fastfetch in {took:.1f}s show the logo and Rift {running}, /etc/issue the name alone")

    # 2d. a drive written with --first-boot. persist has one key slot, the system runs with the machine
    # id in @var, and vault-first-boot said what it made. the next boot asks systemd-cryptsetup's
    # question, not the first boot's, opens the same persist with the same passphrase and makes nothing
    if args.first_boot:
        uuid = r"^\s*([0-9a-fA-F-]{36})\s*$"
        machine_id = r"^\s*([0-9a-f]{32})\s*$"

        def one_line(command, what, pattern):
            status, output = run(command, what)
            found = re.search(pattern, without_console(output), re.M)
            if status != 0 or not found:
                fail(f"{what}: {without_console(output).strip()!r}")
            return found.group(1)

        def made(what):
            """What the first boot made, to compare after the next boot."""
            _, output = run("sudo cryptsetup luksDump /dev/disk/by-partlabel/persist", f"the key slots of persist {what}")
            found = {
                "slots": re.findall(r"^\s+(\d+): luks2\s*$", without_console(output), re.M),
                "luks": one_line("sudo cryptsetup luksUUID /dev/disk/by-partlabel/persist", f"the luks uuid {what}",
                                 uuid).lower(),
                "btrfs": one_line("sudo blkid -s UUID -o value /dev/mapper/persist", f"the btrfs uuid {what}", uuid).lower(),
                "machine": one_line("cat /etc/machine-id", f"the machine id {what}", machine_id),
                "partitions": [(name, label, size) for name, label, _, size in boot_drive()],
            }
            if args.exchange:
                found["exchange"] = one_line("sudo blkid -s UUID -o value /dev/disk/by-partlabel/exchange",
                                             f"the uuid of the exchange partition {what}",
                                             r"^\s*([0-9A-F]{4}-[0-9A-F]{4})\s*$")
            return found

        def said(what):
            _, output = run("sudo journalctl -b -o cat --no-pager -u vault-first-boot", f"what vault-first-boot said {what}")
            printed = without_console(output)
            print(f"\nboot-test: vault-first-boot {what}:\n{printed}", flush=True)
            return printed

        first = made("after the first boot")
        if first["slots"] != ["0"]:
            fail(f"persist has the key slots {first['slots']} after the first boot, expected one")
        in_var = one_line("sudo cat /persist/@var/lib/rift/machine-id", "the machine id in @var", machine_id)
        if in_var != first["machine"]:
            fail(f"the system runs with the machine id {first['machine']}, and @var holds {in_var}")
        printed = said("on the first boot")
        if "Made persist on " not in printed:
            fail("vault-first-boot did not say it made persist")
        if args.exchange and "Formatting the exchange partition." not in printed:
            fail("vault-first-boot did not say it formatted the exchange partition")
        ok(f"persist on {first['partitions'][-1][0]} has one key slot, and the system runs with the machine id in @var")

        # with no network card Welcome, which the session starts on a new drive, opens on the page that
        # says there is no network. Open later closes it without the note, so the next login opens it
        if args.offline:
            welcome_offline("the first boot", args.offline)
            run("rift-welcome --set later now", "Open later on Welcome's page with no network")
            welcome_gone("Open later")
            _, output = run(f"test -e {WELCOME_NOTE}; and echo noted; or echo no-note", "Welcome's note")
            if not re.search(r"^no-note\s*$", without_console(output), re.M):
                fail("Open later wrote the note that says the drive has been welcomed")
            ok(f"Welcome opened on the page that says there is no network, see {args.offline}, and Open later "
               "closed it without the note")

        reboot_action("reset")
        child.send("sudo systemctl reboot\r")
        if expect([CHOOSE, PASSPHRASE], "the passphrase prompt of the second boot") == 0:
            fail("the second boot asked for a new passphrase, it did not find the persist the first boot made")
        ok("passphrase prompt of the second boot")
        unlock()
        second = made("after the second boot")
        if second != first:
            fail(f"the second boot does not find what the first made: {first} before, {second} after")
        printed = said("on the second boot")
        if "Making persist." in printed or "Formatting the exchange partition." in printed:
            fail("vault-first-boot made something again on the second boot")
        ok("the second boot opened the same persist with the same passphrase and made nothing new")
        if args.offline:
            welcome_offline("the second boot", None)
            ok("the next login opened Welcome again on the page that says there is no network")

        power_off()
        print(f"\nboot-test: PASSED in {since()}", flush=True)
        return

    # 2f. the apps, tools and languages of the image's first tier. each prints its version, each compiler
    # builds a program that runs, java runs one from its source, and libvirtd, which no boot starts,
    # starts when virsh connects to the system instance as the owner and names the qemu it runs guests
    # with. every problem is gathered before the step fails, so one run shows all of them
    problems = []
    for command, pattern in TOOLS:
        status, output = run(command, command)
        printed = "\n".join(line.strip() for line in without_console(output).splitlines())
        if status != 0 or not re.search(pattern, printed, re.M):
            problems.append(f"{command} exited with {status} and printed {printed.strip()[-300:]!r}")
    print(f"\nboot-test: {len(TOOLS) - len(problems)} of {len(TOOLS)} version commands printed their versions",
          flush=True)
    _, output = run("echo $JAVA_HOME", "JAVA_HOME")
    if "openjdk-25" not in without_console(output):
        problems.append(f"JAVA_HOME is {without_console(output).strip()!r}, not the jdk in the image")
    run("mkdir -p /tmp/first-tier; and cd /tmp/first-tier", "a folder to build in")
    for name, command, line in BUILDS:
        started = time.monotonic()
        status, output = run(command, f"a program built with {name}")
        printed = "\n".join(printed_line.strip() for printed_line in without_console(output).splitlines())
        # a line can start with what is left of a program's own terminal codes
        if status != 0 or not any(printed_line.endswith(line) for printed_line in printed.splitlines()):
            problems.append(f"the program built with {name} exited with {status} and printed "
                            f"{printed.strip()[-600:]!r}")
        else:
            print(f"\nboot-test: {name} built and ran a program in {time.monotonic() - started:.0f}s", flush=True)
    # what the builds left in home would go into the backups, snapshots and the clone of the steps after
    run("cd ~; and rm -rf /tmp/first-tier ~/.cache/zig ~/.cache/go-build", "home again, without the build caches")
    _, output = run("systemctl is-active libvirtd | cat", "libvirtd before anything connects to it")
    if without_console(output).strip().splitlines()[-1:] != ["inactive"]:
        problems.append(f"libvirtd is {without_console(output).strip()!r} before anything connects, not inactive")
    status, output = run("virsh -c qemu:///system version", "libvirtd's version on the system connection")
    printed = without_console(output)
    if status != 0 or not re.search(r"^\s*Running hypervisor: QEMU \d", printed, re.M):
        problems.append(f"virsh -c qemu:///system version exited with {status}: {printed.strip()[-400:]!r}")
    _, output = run("ls /run/libvirt/nix-ovmf", "the uefi firmware for guests")
    if "edk2-x86_64-secure-code.fd" not in without_console(output):
        problems.append("libvirt has no uefi firmware with secure boot for guests: "
                        f"{without_console(output).strip()!r}")
    if problems:
        fail("the first tier: " + "; ".join(problems))
    ok(f"the {len(TOOLS)} version commands, {len(BUILDS)} programs built and run, JAVA_HOME, and libvirtd "
       "started on the owner's connection")

    # 2g. the hardware a desktop talks to, none of which a virtual machine has: printers, scanners
    # and the firmware of the machine. what this can check is that the services are there and
    # answer, and that the two rules hold: rift asks the network for printers and never announces
    # itself on it, and the firmware of a machine that may be borrowed is read and never written
    problems = []
    status, output = run("lpstat -r", "the print scheduler")
    printed = without_console(output)
    if status != 0 or "scheduler is running" not in printed:
        problems.append(f"lpstat -r exited with {status} and printed {printed.strip()[-200:]!r}")
    # and it reaches a printer the driverless way: the ipp backend, which is what a queue made from
    # what the printer says about itself prints through. asking cups for its backends runs each of
    # them in the mode where it names itself, so the answer comes from the backend that would print
    _, output = run("sudo lpinfo --timeout 10 -v | grep -c -E '^network ipps?'", "the backends cups has")
    found = re.search(r"^\s*(\d+)\s*$", without_console(output), re.M)
    if not found or int(found.group(1)) < 1:
        problems.append(f"cups has no ipp backend: {without_console(output).strip()[-200:]!r}")
    for unit in ("cups", "avahi-daemon", "cups-browsed"):
        _, output = run(f"systemctl is-active {unit} | cat", f"the {unit} unit")
        if without_console(output).strip().splitlines()[-1:] != ["active"]:
            _, journal = run(f"journalctl -b -u {unit} -o cat -n 20 | cat", f"{unit}'s log")
            problems.append(f"{unit} is not active: {without_console(journal).strip()[-400:]!r}")
    # asked to announce a name and an address on the link, avahi says no: publishing is off, and the
    # daemon refuses the entry group itself. the config it was started with is in the store, not in
    # /etc, so this asks the daemon rather than reading a file. avahi-publish-address says what the
    # daemon answered and then exits 0 either way, and it stays up while a name is registered, so
    # the refusal is the message, and a timeout means it published
    status, output = run("timeout 10 avahi-publish-address rift-test.local 192.0.2.1",
                         "avahi asked to announce a name")
    said = without_console(output)
    if status == 124 or "Not permitted" not in said:
        problems.append(f"avahi did not refuse to announce rift-test.local: it exited with {status} "
                        f"and printed {said.strip()[-200:]!r}")
    _, output = run("scanimage -L", "the scanners sane can see")
    printed = without_console(output)
    if "No scanners were identified" not in printed and "device" not in printed:
        problems.append(f"scanimage -L printed {printed.strip()[-300:]!r}")
    # fwupdmgr draws a progress bar over its own output while the daemon reads the devices, and
    # busctl pages, so both go through cat
    status, output = run("fwupdmgr --version | cat", "the firmware service's version")
    printed = without_console(output)
    if status != 0 or not re.search(r"\d+\.\d+\.\d+", printed):
        problems.append(f"fwupdmgr --version exited with {status} and printed {printed.strip()[-300:]!r}")
    # fwupd hangs its interface off the root of its bus name, not off a path of its own
    _, output = run("busctl --system get-property org.freedesktop.fwupd / "
                    "org.freedesktop.fwupd DaemonVersion | cat", "the firmware daemon on the bus")
    if not re.search(r's\s+"\d+\.\d+\.\d+"', without_console(output)):
        problems.append(f"the fwupd daemon did not answer on the bus: {without_console(output).strip()[-300:]!r}")
    # nothing is ever installed, so there is no metadata to download from a vendor once a day
    _, output = run("grep '^Enabled' /etc/fwupd/remotes.d/lvfs.conf", "the vendor remote")
    if "Enabled=false" not in without_console(output):
        problems.append(f"the vendor remote is not disabled: {without_console(output).strip()[-200:]!r}")
    # and every action that would write a firmware is refused outright, with nothing to authenticate
    for action, refused in (("update-internal", True), ("device-unlock", True), ("get-remotes", False)):
        _, output = run(f"pkcheck --action-id org.freedesktop.fwupd.{action} --process $fish_pid",
                        f"whether the owner may {action}")
        said = without_console(output)
        if refused != ("Not authorized." in said):
            problems.append(f"polkit answers {said.strip()[-200:]!r} for {action}, expected "
                            f"{'a refusal with nothing to authenticate' if refused else 'no refusal'}")
    if problems:
        fail("the hardware services: " + "; ".join(problems))
    ok("the print scheduler is running with avahi and cups-browsed beside it, avahi announces nothing, "
       "sane answers, the firmware daemon names its version, and every firmware write is refused")

    # 3. orbit: the profile it wrote into @hosts, and the same answers on the system bus.
    # fish puts a bare \r before a command's output, so these anchor on the whitespace after the
    # value, not before it
    child.send("systemctl is-active orbit\r")
    expect([r"(?<![\w-])(active|inactive|failed|activating)\s"], "the orbit unit state")
    state = child.match.group(1)
    expect([PROMPT], "the prompt")
    if state != "active":
        fail(f"orbit.service is {state}, expected active")

    hosts = "/var/lib/rift/hosts"
    child.send(f"cat {hosts}/current\r")
    expect([r"(?<![0-9a-f])([0-9a-f]{64})\s"], "the fingerprint in hosts/current")
    fingerprint = child.match.group(1)
    expect([PROMPT], "the prompt")

    # the profile is a delta over the defaults, so a virtual machine writes eleven lines with a
    # value on them: the four that say which machine this is, the two settings a qemu box does
    # not share with the defaults, and five for its one output. the class, the chassis, the
    # vendor and the scale are all the defaults, so they are not in the file at all.
    profile = f"{hosts}/{fingerprint}.toml"
    child.send(f"cat {profile}\r")
    expect([rf'fingerprint = "{fingerprint}"'], "the fingerprint in the profile")
    expect([r'host = "([^"]*)"'], "the machine name in the profile")
    machine = child.match.group(1)
    expect([r'gpu_path = "(\w+)"'], "the gpu path in the profile")
    file_gpu_path = child.match.group(1)
    expect([r'ai_tier = "(\w+)"'], "the ai tier in the profile")
    file_ai_tier = child.match.group(1)
    expect([r'connector = "([\w-]+)"'], "the output in the profile")
    file_connector = child.match.group(1)
    # a number with nothing after it matches as soon as its first digits arrive
    expect([r"width = (\d+)\s"], "the output width in the profile")
    file_width = child.match.group(1)
    expect([r"height = (\d+)\s"], "the output height in the profile")
    file_height = child.match.group(1)
    expect([PROMPT], "the prompt")

    def count(what, command):
        child.send(f"echo {what}=({command})\r")
        expect([rf"{what}=(\d+)\s"], f"the {what} count")
        value = int(child.match.group(1))
        expect([PROMPT], "the prompt")
        return value

    keys = count("keys", f"grep -c ' = ' {profile}")
    if keys != 11:
        fail(f"the profile has {keys} lines with a value on them, expected 11, not a delta")
    if count("class", f"grep -c '^class = ' {profile}") != 0:
        fail("the profile writes the class, which is the default and belongs to no machine")
    if file_gpu_path != "none":
        fail(f"the profile says gpu path {file_gpu_path}, expected none for a virtual machine")
    if file_ai_tier != "small":
        fail(f"the profile says ai tier {file_ai_tier}, expected small for a 4 GB machine")
    if (file_width, file_height) != ("1280", "800"):
        fail(f"the profile says the output is {file_width}x{file_height}, expected 1280x800")
    if count("scale", f"grep -c '^scale = ' {profile}") != 0:
        fail("the profile writes a scale, but a 32 by 20 cm 1280x800 panel is about 102 dpi")

    # the bus. the interface is read only, so the owner reads it without sudo
    bus, obj = "dev.rift.Orbit", "/dev/rift/Orbit"
    if count("bus", f"busctl --system list --no-pager --no-legend | grep -c '^{bus}'") != 1:
        fail(f"{bus} is not on the system bus")

    def prop(name, pattern):
        child.send(f"busctl --system get-property {bus} {obj} {bus} {name}\r")
        expect([pattern], f"the {name} property")
        match = child.match
        expect([PROMPT], "the prompt")
        return match

    if prop("Fingerprint", r's "([0-9a-f]{64})"').group(1) != fingerprint:
        fail("the fingerprint on the bus is not the one in hosts/current")
    klass = prop("Class", r's "(\w+)"').group(1)
    if klass != "borrowed":
        fail(f"the bus says class {klass}, expected the default borrowed")
    gpu_path = prop("GpuPath", r's "(\w+)"').group(1)
    if gpu_path != file_gpu_path:
        fail(f"the bus says gpu path {gpu_path}, the profile says {file_gpu_path}")
    ai_tier = prop("AiTier", r's "(\w+)"').group(1)
    if ai_tier != file_ai_tier:
        fail(f"the bus says ai tier {ai_tier}, the profile says {file_ai_tier}")

    # one virtual output: its connector, its mode, its size in centimetres and the size it is
    # drawn at. qemu gives it an edid, so the mode and the size are real; at 32 by 20 centimetres
    # 1280x800 is about 102 dpi, which is under the line, so the scale is 1
    displays = prop("Displays", r"a\(suuuuu\) (\d+)([^\r\n]*)\r*\n")
    if displays.group(1) != "1":
        fail(f"the bus lists {displays.group(1)} outputs, expected 1:{displays.group(2)}")
    output = re.match(r'\s*"([\w-]+)" (\d+) (\d+) (\d+) (\d+) (\d+)', displays.group(2))
    if not output:
        fail(f"the output on the bus does not read as one:{displays.group(2)}")
    if output.group(1) != file_connector:
        fail(f"the bus calls the output {output.group(1)}, the profile calls it {file_connector}")
    if output.group(2, 3, 6) != (file_width, file_height, "1"):
        fail(f"the output on the bus is {output.group(2, 3, 6)}, the profile says "
             f"{file_width}x{file_height} at scale 1")
    if output.group(4, 5) != ("32", "20"):
        fail(f"the bus says the output is {output.group(4)} by {output.group(5)} cm, "
             "qemu's edid says 32 by 20")
    ok(
        f"host profile {fingerprint[:12]}, {machine}, class {klass}, gpu {gpu_path}, "
        f"ai tier {ai_tier}, output {output.group(1)} {output.group(4)}x{output.group(5)} cm "
        f"scale {output.group(6)}, on the bus"
    )

    # 3a. `rift host` reads the same properties off the bus and prints a row for each
    host_rows = {
        "Fingerprint": fingerprint,
        "Class": klass,
        "Display": f"{output.group(1)}, {output.group(2)}x{output.group(3)}, scale {output.group(6)}",
        "GPU path": gpu_path,
        "AI tier": ai_tier,
    }
    status, printed = run("rift host", "rift host")
    printed = without_console(printed)
    print(f"\nboot-test: rift host printed:\n{printed}", flush=True)
    if status != 0:
        fail(f"rift host exited with {status}")
    rows = dict(re.findall(r"^(Fingerprint|Class|Display|GPU path|AI tier):[ \t]+(.*?)[ \t]*$", printed, re.M))
    for label, value in host_rows.items():
        if rows.get(label) != value:
            fail(f"rift host says {label} {rows.get(label)!r}, the bus says {value!r}")
    ok(f"rift host printed fingerprint {fingerprint[:12]} and ai tier {ai_tier}, as the bus did")

    # 4. quasar. quasard reads the tier from orbit, picks a model that is on the drive, runs
    # llama-server as its child and answers on the system bus. the name is there before the model
    # has loaded, so poll the State property
    if args.models:
        _, output = run("systemctl is-active quasar", "the quasar unit state")
        state = re.search(r"(?<![\w-])(active|inactive|failed|activating)\s", output)
        state = state.group(1) if state else output.strip()
        if state not in ("active", "activating"):
            fail(f"quasar.service is {state}, expected active")

        quasar, quasar_path = "dev.rift.Quasar", "/dev/rift/Quasar"

        def quasar_prop(name):
            """A string property of quasar's, or None when the bus gave no answer."""
            status, output = run(f"busctl --system get-property {quasar} {quasar_path} {quasar} {name}",
                                 f"quasar's {name} property")
            value = re.search(r's "([^"\n]*)"', output)
            return value.group(1) if status == 0 and value else None

        quasar_deadline = time.monotonic() + args.quasar_timeout
        while True:
            quasar_state = quasar_prop("State")
            if quasar_state == "ready":
                break
            if quasar_state in ("none", "failed") or time.monotonic() > quasar_deadline:
                why = quasar_prop("Error")
                fail(f"quasar is {quasar_state or 'not on the bus'} after {since()}: {why}")
            time.sleep(5)

        # the only model on the drive is the one in --models, and the manifest says which id it is
        manifest_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "models", "manifest.toml")
        with open(manifest_path, "rb") as f:
            chat = tomllib.load(f)["chat"]
        on_drive = set(os.listdir(args.models))
        wanted_model = [m["id"] for m in chat if m["file"] in on_drive]
        quasar_tier = quasar_prop("Tier")
        quasar_model = quasar_prop("Model")
        ok(f"quasar loaded {quasar_model} for tier {quasar_tier}")
        if quasar_tier != ai_tier:
            fail(f"quasar says the tier is {quasar_tier}, orbit says {ai_tier}")
        if [quasar_model] != wanted_model:
            fail(f"quasar runs {quasar_model}, the models on the drive are {wanted_model}")

        # the local api that other programs use is the same server
        api = "localhost:11434"
        _, output = run(f"curl -s -o /dev/null -w 'health=%{{http_code}}\\n' {api}/health", "the quasar health code")
        code = re.search(r"health=(\d{3})", output)
        if not code or code.group(1) != "200":
            fail(f"the local api says {output.strip()!r} on /health, but quasar says the model is ready")

        body = '{"prompt":"The capital of France is","n_predict":4}'
        _, output = run(f"curl -s {api}/completion -d '{body}'", "a completion")
        content = re.search(r'"content":"([^"]+)"', output)
        if not content:
            fail(f"the local api gave no completion: {output.strip()!r}")
        ok(f"the local api completed {content.group(1)!r}")

        # a web page cannot use it. a browser sends an Origin header with anything a page asks
        # for, and a page that points its own name at 127.0.0.1 sends that name as the Host. the
        # model's socket behind the api is quasar's alone
        def api_code(options, what):
            _, output = run(f"curl -s -o /dev/null -w 'code=%{{http_code}}\\n' {options}", what)
            code = re.search(r"code=(\d{3})", output)
            return code.group(1) if code else output.strip()

        for header, what in [
            ("Origin: https://example.com", "a completion a web page asked for"),
            ("Host: example.com:11434", "a completion for a name that is not the loopback address"),
        ]:
            code = api_code(f"-H '{header}' {api}/completion -d '{body}'", what)
            if code != "403":
                fail(f"the local api answered {what} with {code}, expected 403")
        code = api_code("--unix-socket /run/quasar/llama.sock http://localhost/health", "llama-server's socket")
        if code != "000":
            fail(f"the owner reached llama-server's socket without the local api, it said {code}")
        ok("the local api refuses web pages, and only quasar opens the model's socket")

        # and the question over the bus, as the owner, no sudo. Ask returns a kind and a text, and
        # busctl's json keeps both on one line with their quotes escaped
        _, output = run(f"busctl --system --json=short --timeout=240 call {quasar} {quasar_path} {quasar} Ask s '{QUESTION}'",
                        "quasar's answer on the bus")
        reply = re.search(r'"type":"ss","data":\["(\w+)","((?:[^"\\]|\\.)+)"\]\}', output)
        if not reply:
            fail(f"Ask on the bus gave no answer: {output.strip()!r}")
        kind, answer = reply.group(1), json.loads('"' + reply.group(2) + '"')
        if kind != "answer":
            fail(f"Ask on the bus said {kind} {answer!r} to {QUESTION!r}, expected an answer")
        ok(f"quasar answered {QUESTION!r} on the bus with {answer!r}")

        # 4a. the same question through `rift ai`, which prints the answer, and `rift ai`
        # without one, which prints the properties the bus just gave
        status, printed = run(f'rift ai "{QUESTION}"', "quasar's answer through rift ai")
        printed = without_console(printed)
        print(f'\nboot-test: rift ai "{QUESTION}" printed:\n{printed}', flush=True)
        if status != 0 or "paris" not in printed.lower():
            fail(f"rift ai exited with {status} and did not say Paris")
        ok(f"rift ai answered {printed!r}")

        status, printed = run("rift ai", "quasar's state through rift ai")
        printed = without_console(printed)
        print(f"\nboot-test: rift ai printed:\n{printed}", flush=True)
        rows = dict(re.findall(r"^(State|Model|Tier):[ \t]+(.*?)[ \t]*$", printed, re.M))
        wanted = {"State": "ready", "Model": quasar_model, "Tier": quasar_tier}
        if status != 0 or rows != wanted:
            fail(f"rift ai says {rows}, the bus says {wanted}")
        ok("rift ai printed the state, model and tier the bus gave")

        # 4c. search by meaning. quasard runs the embedding model beside the chat model, and the owner's
        # user manager has a unit that walks home, gets a vector for each part of a file from quasar and
        # keeps them in the owner's cache. quasar never reads home. a search finds a file by what it
        # means, with none of its words
        embedding_deadline = time.monotonic() + args.quasar_timeout
        while True:
            embedding_state = quasar_prop("EmbeddingState")
            if embedding_state == "ready":
                break
            if embedding_state in ("none", "failed") or time.monotonic() > embedding_deadline:
                why = quasar_prop("EmbeddingError")
                fail(f"quasar's embedding model is {embedding_state or 'not on the bus'} after {since()}: {why}")
            time.sleep(5)
        with open(manifest_path, "rb") as f:
            embedding = tomllib.load(f)["embedding"][0]["id"]
        if quasar_prop("EmbeddingModel") != embedding:
            fail(f"quasar runs {quasar_prop('EmbeddingModel')} for search, the manifest's embedding model is {embedding}")
        status, printed = run("rift ai", "the search row of rift ai")
        printed = without_console(printed)
        if status != 0 or not re.search(rf"^Search:[ \t]+ready, {re.escape(embedding)}[ \t]*$", printed, re.M):
            fail(f"rift ai does not say search is ready with {embedding}: {printed!r}")
        ok(f"quasar loaded {embedding} for search by meaning")

        notes = "/home/rift/notes"
        documents = {
            "garden.md": ["Tomatoes want six hours of sun.", "Water the beans early and pull weeds before they seed."],
            "bike.txt": ["Pump the tyres to 80 psi.",
                         "Oil the chain every 300 km and change the brake pads when they squeal."],
            "taxes.md": ["The return is due at the end of April.",
                         "Keep the receipts for the home office deduction and the donations."],
            "soup.txt": ["Chop two onions and a carrot, fry them in butter.", "Add stock and simmer for twenty minutes."],
            "backup.py": ["import shutil", "", "def copy_to_disk(source, target):",
                          "    shutil.copytree(source, target, dirs_exist_ok=True)"],
        }
        searches = {"bicycle repair": "bike.txt", "duplicate folders onto a drive": "backup.py",
                    SEARCH_PDF_WORDS: SEARCH_PDF}
        wording = {name: " ".join(lines) for name, lines in documents.items()}
        wording[SEARCH_PDF] = " ".join(line for page in SEARCH_PDF_PAGES for line in page)
        for words, name in searches.items():
            text = (name + " " + wording[name]).lower()
            shared = [word for word in words.split() if len(word) > 3 and word in text]
            if shared:
                fail(f"the search for {words!r} shares {shared} with {name}, it would not be by meaning")
        run(f"mkdir -p {notes}", "the folder for the files to search")
        for name, lines in documents.items():
            quoted = " ".join(f"'{line}'" for line in lines)
            status, output = run(f"printf '%s\\n' {quoted} > {notes}/{name}", f"{notes}/{name}")
            if status != 0:
                fail(f"{notes}/{name} could not be written: {without_console(output).strip()!r}")

        # and a pdf, which holds its text the way it is drawn on a page. it goes in as base64 in
        # pieces the serial console takes, since the shell cannot type the bytes themselves
        letter = pdf_bytes(SEARCH_PDF_PAGES)
        encoded = base64.b64encode(letter).decode("ascii")
        run(f"rm -f {notes}/letter.base64", "anything left of an earlier pdf")
        for at in range(0, len(encoded), 300):
            status, output = run(f"printf '%s' '{encoded[at:at + 300]}' >> {notes}/letter.base64",
                                 f"a piece of {SEARCH_PDF}")
            if status != 0:
                fail(f"{SEARCH_PDF} could not be written: {without_console(output).strip()!r}")
        status, output = run(f"base64 -d {notes}/letter.base64 > {notes}/{SEARCH_PDF}; "
                             f"and rm {notes}/letter.base64; and stat -c 'pdf=%s' {notes}/{SEARCH_PDF}",
                             f"{notes}/{SEARCH_PDF}")
        if status != 0 or f"pdf={len(letter)}" not in without_console(output):
            fail(f"{notes}/{SEARCH_PDF} is not the {len(letter)} bytes it was written from: "
                 f"{without_console(output).strip()!r}")

        # pdftotext reads it in a sandbox with no network, which is the one place a pdf is parsed.
        # the form feed it puts between pages is what makes a part of the index a page
        status, printed = run(f"airlock text {notes}/{SEARCH_PDF} | tr '\\f' '@'",
                              f"the text of {SEARCH_PDF} out of the sandbox")
        printed = without_console(printed)
        print(f"\nboot-test: airlock text {SEARCH_PDF} printed:\n{printed}", flush=True)
        if status != 0 or SEARCH_PDF_PHRASE not in printed or "@" not in printed:
            fail(f"airlock text exited with {status} and did not write out {SEARCH_PDF_PHRASE!r} "
                 f"with a form feed between the pages: {printed[-300:]!r}")
        ok(f"airlock text wrote out the text of {SEARCH_PDF} in a sandbox, page by page")

        # the timer's unit, started now instead of ten minutes after login. start waits for a oneshot
        status, output = run("systemctl --user start quasar-index.service", "the index of home")
        if status != 0:
            fail(f"quasar-index.service did not start: {without_console(output).strip()!r}")
        _, output = run("systemctl --user show --property=Result,ExecMainStatus,ConditionResult quasar-index.service | cat",
                        "how the index unit ended")
        shown = dict(re.findall(r"^(\w+)=(\S*)\s*$", without_console(output), re.M))
        if shown.get("Result") != "success" or shown.get("ExecMainStatus") != "0" or shown.get("ConditionResult") != "yes":
            fail(f"quasar-index.service ended with {shown}")
        _, output = run("stat -c 'index=%U:%a' ~/.cache/rift ~/.cache/rift/search.index", "the index's owner")
        modes = re.findall(r"index=(\w+:\d+)", output)
        if modes != ["rift:700", "rift:600"]:
            fail(f"the index and its folder are {modes}, expected the owner's alone")

        # nothing changed since, so a second update reads nothing again
        status, printed = run("rift ai index", "a second update of the index")
        printed = without_console(printed)
        print(f"\nboot-test: rift ai index printed:\n{printed}", flush=True)
        counted = re.search(r"(\d+) files? (?:is|are) in the index\. (\d+) (?:was|were) new or changed", printed)
        if status != 0 or not counted:
            fail(f"rift ai index exited with {status}: {printed!r}")
        indexed = len(documents) + 1
        if int(counted.group(1)) < indexed or counted.group(2) != "0":
            fail(f"rift ai index says {counted.group(0)!r}, expected the {indexed} files and none read again")

        best = {}
        for words, name in searches.items():
            status, printed = run(f"rift ai search {words}", f"a search for {words}")
            printed = without_console(printed)
            print(f"\nboot-test: rift ai search {words} printed:\n{printed}", flush=True)
            rows = re.findall(r"^(~/\S+):(page \d+|\d+)[ \t]+(\d{4}-\d{2}-\d{2})[ \t]*$", printed, re.M)
            if status != 0 or not rows:
                fail(f"rift ai search {words} exited with {status} and listed no files")
            if rows[0][0] != f"~/notes/{name}":
                fail(f"rift ai search {words} put {rows[0][0]} first, expected ~/notes/{name}")
            best[name] = rows[0][1]
        ok("rift ai search found " + " and ".join(f"{name} for {words!r}" for words, name in searches.items())
           + ", by meaning")
        # a pdf has pages and no lines, so its row says which page the words are on
        if best[SEARCH_PDF] != f"page {SEARCH_PDF_PAGE}":
            fail(f"the row for {SEARCH_PDF} says {best[SEARCH_PDF]!r}, expected page {SEARCH_PDF_PAGE}")
        ok(f"the row for {SEARCH_PDF} names page {SEARCH_PDF_PAGE}, where the words it was found by are")

        # 4d. words out loud. the voice is a model on the drive like the others, and nothing runs
        # between sentences, so it is ready as soon as its files are there. the sound card of this
        # vm plays nowhere, so the test reads the wav instead of listening to it
        with open(manifest_path, "rb") as f:
            say_voice = tomllib.load(f)["tts"][0]["id"]
        say_deadline = time.monotonic() + args.quasar_timeout
        while True:
            say_state = quasar_prop("VoiceState")
            if say_state == "ready":
                break
            if say_state == "none" or time.monotonic() > say_deadline:
                why = quasar_prop("VoiceError")
                fail(f"quasar's voice is {say_state or 'not on the bus'} after {since()}: {why}")
            time.sleep(2)
        if quasar_prop("Voice") != say_voice:
            fail(f"quasar's voice is {quasar_prop('Voice')}, the manifest's voice is {say_voice}")
        status, printed = run("rift ai", "the voice row of rift ai")
        printed = without_console(printed)
        if status != 0 or not re.search(rf"^Voice:[ \t]+ready, {re.escape(say_voice)}[ \t]*$", printed, re.M):
            fail(f"rift ai does not say the voice is ready with {say_voice}: {printed!r}")
        ok(f"quasar says words out loud with {say_voice}")

        say_words = "Rift runs the model on the drive and says this out loud."
        say_file = "/home/rift/said.wav"
        status, printed = run(f'rift ai say --wav {say_file} "{say_words}"', "a sentence out loud")
        printed = without_console(printed)
        print(f"\nboot-test: rift ai say printed:\n{printed}", flush=True)
        said = re.search(r"Wrote ([\d.]+) seconds to " + re.escape(say_file), printed)
        if status != 0 or not said:
            fail(f"rift ai say exited with {status} and wrote no wav: {printed!r}")

        # the wav itself: a header the test reads byte by byte, since the file cannot leave the vm
        _, output = run(f"printf 'wav='; and od -A n -v -t x1 -N 64 {say_file} | tr -d ' \\n'; and echo",
                        "the head of the wav")
        head = re.search(r"wav=([0-9a-f]{128})", without_console(output))
        if not head:
            fail(f"the head of {say_file} did not come back: {without_console(output).strip()!r}")
        head = bytes.fromhex(head.group(1))
        if head[:4] != b"RIFF" or head[8:12] != b"WAVE" or head[12:16] != b"fmt ":
            fail(f"{say_file} does not start like a wav: {head[:16]!r}")
        say_format, say_channels = int.from_bytes(head[20:22], "little"), int.from_bytes(head[22:24], "little")
        say_rate, say_bits = int.from_bytes(head[24:28], "little"), int.from_bytes(head[34:36], "little")
        at = head.find(b"data")
        if at < 0:
            fail(f"{say_file} has no data chunk in its first 64 bytes: {head!r}")
        say_bytes = int.from_bytes(head[at + 4:at + 8], "little")
        if (say_format, say_channels, say_rate, say_bits) != (1, 1, 22050, 16):
            fail(f"{say_file} is format {say_format}, {say_channels} channels, {say_rate} Hz, {say_bits} bits, "
                 "expected plain 22050 Hz mono of 16 bit samples")
        say_seconds = say_bytes / (say_rate * say_channels * say_bits // 8)
        if not 2.0 <= say_seconds <= 10.0:
            fail(f"{say_file} holds {say_seconds:.1f} seconds of audio for {len(say_words)} characters")
        if abs(say_seconds - float(said.group(1))) > 0.1:
            fail(f"rift ai say said {said.group(1)} seconds, the wav holds {say_seconds:.1f}")

        # and it is speech, not a file of silence: a wav of nothing but zeros is nothing but zeros
        _, output = run(f"printf 'loud='; and tr -d '\\0' < {say_file} | wc -c",
                        "the bytes of the wav that are not zero")
        say_loud = re.search(r"loud=(\d+)", without_console(output))
        if not say_loud or int(say_loud.group(1)) < say_bytes // 5:
            fail(f"{say_file} is {say_loud and say_loud.group(1)} bytes of {say_bytes} that are not zero, "
                 "which is silence, not speech")
        ok(f"rift ai say wrote {say_seconds:.1f} seconds of 22050 Hz mono speech to {say_file}")

        # the same sentence with no file named goes to the speakers, which this vm has and hears
        # nothing through
        status, printed = run('rift ai say "Ready."', "a sentence through the speakers")
        if status != 0:
            fail(f"rift ai say exited with {status}: {without_console(printed).strip()!r}")
        ok("rift ai say played a sentence through the machine's own speakers")

        # 4e. speech into words, the other way round. the vm has no microphone, so the recording is
        # the wav the voice just wrote: the sentence goes out through one model and comes back
        # through the other, and no recorded speech has to live in git
        with open(manifest_path, "rb") as f:
            hear_model = tomllib.load(f)["speech"][0]["id"]
        hear_deadline = time.monotonic() + args.quasar_timeout
        while True:
            hear_state = quasar_prop("SpeechState")
            if hear_state == "ready":
                break
            if hear_state == "none" or time.monotonic() > hear_deadline:
                why = quasar_prop("SpeechError")
                fail(f"quasar's speech model is {hear_state or 'not on the bus'} after {since()}: {why}")
            time.sleep(2)
        if quasar_prop("Speech") != hear_model:
            fail(f"quasar's speech model is {quasar_prop('Speech')}, the manifest's is {hear_model}")
        status, printed = run("rift ai", "the speech row of rift ai")
        printed = without_console(printed)
        print(f"\nboot-test: rift ai printed:\n{printed}", flush=True)
        if status != 0 or not re.search(rf"^Speech:[ \t]+ready, {re.escape(hear_model)}[ \t]*$", printed, re.M):
            fail(f"rift ai does not say speech is ready with {hear_model}: {printed!r}")
        ok(f"quasar turns speech into words with {hear_model}")

        status, printed = run(f"rift ai listen {say_file}", f"the words in {say_file}")
        printed = without_console(printed)
        print(f"\nboot-test: rift ai listen printed:\n{printed}", flush=True)
        hear_line = " ".join(line.strip() for line in printed.splitlines() if line.strip())
        hear_spoken = re.findall(r"[a-z]+", say_words.lower())
        hear_heard = re.findall(r"[a-z]+", hear_line.lower())
        hear_shared = [word for word in hear_spoken if word in hear_heard]
        # a small model gets a word wrong now and then, so most of the sentence is the test, not all
        # of it. the words of the command itself are no help: it names a file, not the sentence
        if status != 0 or len(hear_shared) < 0.7 * len(hear_spoken):
            fail(f"rift ai listen exited with {status} and gave back {hear_line!r} of "
                 f"{say_words!r}: {len(hear_shared)} of its {len(hear_spoken)} words")
        ok(f"rift ai listen read {len(hear_shared)} of the sentence's {len(hear_spoken)} words back "
           f"out of the wav the voice wrote: {hear_line!r}")
        run(f"rm -f {say_file}", "the wav the test wrote")

        # and a file that is not a recording at all is refused before any model runs
        hear_junk = "/home/rift/notes/not-a-recording.wav"
        run(f"printf 'this is not audio' > {hear_junk}", "a file that is not a recording")
        status, printed = run(f"rift ai listen {hear_junk}", "what quasar says about it")
        printed = without_console(printed)
        if status == 0 or "not one" not in printed:
            fail(f"rift ai listen said {printed.strip()!r} about a file that is not a recording")
        run(f"rm -f {hear_junk}", "the file that is not a recording")
        ok("rift ai listen refuses a file that is not a recording")

    # 4b. `rift doctor`: no check fails, and orbit and quasar each have a row. with the model
    # loaded, quasar's row has to pass
    status, printed = run("rift doctor", "rift doctor")
    printed = without_console(printed)
    print(f"\nboot-test: rift doctor printed:\n{printed}", flush=True)
    rows = dict(re.findall(r"^(Orbit|Quasar|Persist|Memory|CPU|IO|System image)[ \t]+(Passed|Warning|Failed)[ \t]",
                           printed, re.M))
    if status != 0:
        fail(f"rift doctor exited with {status}")
    if rows.get("Orbit") != "Passed":
        fail(f"rift doctor says Orbit {rows.get('Orbit')}, expected Passed")
    if "Quasar" not in rows or (args.models and rows["Quasar"] != "Passed"):
        fail(f"rift doctor says Quasar {rows.get('Quasar')}, expected Passed")
    ok("rift doctor: " + ", ".join(f"{name} {verdict}" for name, verdict in rows.items()))

    # 5. the desktop. greetd runs horizon on tty1 as the owner. horizon needs a moment to open the gpu
    # and paint its first frame, so the screendump is retried until it shows the background
    if args.desktop:
        child.send("systemctl is-active greetd\r")
        expect([r"(?<![\w-])(active|inactive|failed|activating)\s"], "the greetd unit state")
        state = child.match.group(1)
        expect([PROMPT], "the prompt")
        if state != "active":
            fail(f"greetd.service is {state}, expected active")

        def look(what, png, seconds, console=False, lock=None, apps=None, colors=DARK_COLORS, journals=(),
                 settle=0, share=0.4, **shape):
            """Screendump until the bar and the menu have the shape we asked for, or the console is
            open, or the lock screen is up (lock says whether it has refused a password), or the apps
            stand side by side with their title bars, or give up and save it. colors are the theme's.
            With settle, it passes only when a second screendump that many seconds later passes too,
            so a window that is still drawing its first frames is not taken for done. On a failure the
            journal of each tag in journals is printed."""
            deadline = time.monotonic() + seconds
            passes = 0
            while True:
                try:
                    width, height, rgb = screendump(args.qmp, work, "desktop")
                except (OSError, RuntimeError) as e:
                    fail(f"screendump: {e}")
                if lock is not None:
                    good, lines = check_lock(width, height, rgb, refused=lock, colors=colors)
                elif console:
                    good, lines = check_console(width, height, rgb)
                elif apps:
                    good, lines = check_apps(width, height, rgb, apps, colors, share)
                else:
                    good, lines = check_desktop(width, height, rgb, lens=args.lens, colors=colors, **shape)
                passes = passes + 1 if good else 0
                if passes >= (2 if settle else 1) or time.monotonic() > deadline:
                    break
                time.sleep(settle if good else 2 if shape or console or apps or lock is not None else 5)
            write_png(png, width, height, rgb)
            print("\nboot-test: " + "\nboot-test: ".join(lines), flush=True)
            if not good:
                for tag in journals:
                    _, output = run(f"journalctl -b -t {tag} --no-pager -n 40 -o cat", f"the {tag} journal")
                    print(f"\nboot-test: journalctl -t {tag} printed:\n{without_console(output)}", flush=True)
                fail(f"{what} is not on screen, see {png}")
            ok(what)

        # 5. first, Welcome. horizon starts rift-welcome --login with the session, and on a drive that has
        # not been welcomed it opens over the desktop, in the middle of the screen, on its start page,
        # since the vm has a network. the test walks its pages with a screendump of each. its Apps page
        # asks flatpak, and flathub is a remote of the system installation from the first time anything
        # uses it. Done writes the note and closes it, and the desktop behind it is what the checks
        # below expect
        stem, extension = os.path.splitext(args.desktop)
        welcome_now = welcome_until(args.desktop_timeout + 180,
                                    lambda lines: welcome_value(lines, "page") == "start", "its start page")
        if welcome_value(welcome_now, "network") != "online" or welcome_value(welcome_now, "welcomed") != "no":
            fail(f"Welcome says network {welcome_value(welcome_now, 'network')!r} and welcomed "
                 f"{welcome_value(welcome_now, 'welcomed')!r} on a new drive with a network")
        _, output = run("set -x NIRI_SOCKET (ls -t /run/user/(id -u)/niri.wayland-1.*.sock | head -n1); "
                        "horizon msg --json windows", "horizon's windows with Welcome up")
        welcome_windows = [found.group(2) for found in WINDOW.finditer(without_console(output).replace("\n", ""))]
        if WELCOME_APP_ID not in welcome_windows:
            fail(f"horizon lists no {WELCOME_APP_ID} window, only {welcome_windows}")
        welcome_picture(f"{stem}-welcome-start{extension}", "welcome-start", check=True)
        run("rift-welcome --set next now", "Next on Welcome's start page")
        welcome_until(30, lambda lines: welcome_value(lines, "page") == "appearance", "its Appearance page")
        time.sleep(1)
        welcome_picture(f"{stem}-welcome-appearance{extension}", "welcome-appearance")
        run("rift-welcome --set next now", "Next on Welcome's Appearance page")
        welcome_now = welcome_until(
            150, lambda lines: welcome_value(lines, "page") == "apps"
            and welcome_value(lines, "remotes") not in (None, "asking", "unknown")
            and any(printed.startswith("sizes flathub ") for printed in lines), "what flathub has")
        welcome_remotes = welcome_value(welcome_now, "remotes") or ""
        if "flathub" not in welcome_remotes.split(","):
            fail(f"Welcome's Apps page says remotes {welcome_remotes!r}, and the system installation has no flathub")
        welcome_sizes = next(printed for printed in welcome_now if printed.startswith("sizes flathub "))
        _, output = run("flatpak remotes --system --columns=name,url | cat", "the system installation's remotes")
        if not re.search(r"^flathub\s+https://dl\.flathub\.org/repo/\s*$", without_console(output), re.M):
            fail(f"flatpak remotes does not list flathub at dl.flathub.org: {without_console(output).strip()!r}")
        time.sleep(1)
        welcome_picture(f"{stem}-welcome-apps{extension}", "welcome-apps")
        run("rift-welcome --page developer", "Welcome's Developer page")
        welcome_until(30, lambda lines: welcome_value(lines, "page") == "developer", "its Developer page")
        time.sleep(1)
        welcome_picture(f"{stem}-welcome-developer{extension}", "welcome-developer")
        run("rift-welcome --set next now", "Next on Welcome's Developer page")
        welcome_until(30, lambda lines: welcome_value(lines, "page") == "done", "its Done page")
        time.sleep(1)
        welcome_picture(f"{stem}-welcome-done{extension}", "welcome-done")
        run("rift-welcome --set done now", "Done")
        welcome_gone("Done")
        _, output = run(f"test -e {WELCOME_NOTE}; and echo noted; or echo no-note", "Welcome's note")
        if not re.search(r"^noted\s*$", without_console(output), re.M):
            fail("Done did not write the note that says the drive has been welcomed")
        _, output = run("horizon msg --json windows", "horizon's windows after Done")
        if WELCOME_APP_ID in without_console(output):
            fail("Welcome's window is still there after Done")
        ok(f"Welcome opened in the middle of the screen on its start page, its Apps page found flathub among the "
           f"system installation's remotes ({welcome_sizes}), and Done wrote the note and closed it")

        look("desktop", args.desktop, args.desktop_timeout, wallpaper=WALLPAPER_LEFT + WALLPAPER_RIGHT)

        # 5a. the compositor knows lens's surface too. the session's ipc socket is in the
        # owner's runtime directory, the serial shell runs as the owner
        if args.lens:
            _, output = run("set -x NIRI_SOCKET (ls -t /run/user/(id -u)/niri.wayland-1.*.sock | head -n1); horizon msg --json layers",
                            "horizon's layer surfaces")
            if not re.search(r'"namespace":\s*"lens"', output):
                fail("horizon lists no layer surface named lens")
            ok("lens's bar")

            stem, extension = os.path.splitext(args.desktop)
            run("set -x XDG_RUNTIME_DIR /run/user/(id -u)", "the runtime directory")

            def bar_state(what):
                """What `lens --state` prints, as a dict of the words it knows. A shell that has just
                been started again has no socket yet, which is nothing to fail over: every caller asks
                again until it answers, and says its own piece when it never does."""
                status, output = run("lens --state", what)
                printed = without_console(output)
                if status != 0 and ("Connection refused" in printed or "No such file" in printed):
                    return {}
                if status != 0:
                    fail(f"lens --state exited with {status}: {printed.strip()[-300:]!r}")
                state = {}
                for printed_line in printed.splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    if key in STATE_KEYS:
                        state[key] = value.strip()
                return state

            def open_windows(what):
                """Horizon's windows as (id, app id, whether it has the focus)."""
                status, output = run("horizon msg --json windows", what)
                printed = without_console(output).replace("\n", "")
                if status != 0:
                    fail(f"horizon msg windows exited with {status}: {printed.strip()[-300:]!r}")
                return [(int(found.group(1)), found.group(2) or "", found.group(3) == "true")
                        for found in WINDOW.finditer(printed)]

            def vm_clock(what):
                """The minute the vm's own clock is in, in the format the bar writes."""
                _, output = run(f"date '{DATE_FORMAT}'", what)
                found = CLOCK.findall(without_console(output))
                if not found:
                    fail(f"date printed no time in the bar's format: {without_console(output).strip()[-200:]!r}")
                return found[-1]

            # 5b. what the bar shows. its unit is up, and `lens --state` agrees with the system: the
            # clock with date, the network icon with nmcli. the minute can turn between the two
            # readings, so either of them is right
            state = unit_state("--user lens.service")
            if state != "active":
                fail(f"lens.service is {state} for the owner, expected active")
            # the unit is not a child of the compositor, so it only finds desktop entries if the
            # session's data directories reached the user manager
            _, output = run("journalctl --user -u lens -b -o cat | cat", "the shell's log")
            found = re.search(r"lens \S+: (\d+) apps, opening the bar", without_console(output))
            if not found or int(found.group(1)) == 0:
                fail(f"the shell found no apps: {without_console(output).strip()[-300:]!r}")
            ok(f"the shell runs as a user unit and found {found.group(1)} apps")
            # the bar turns its clock on the minute and the readings here are a second or so apart,
            # so the minute can turn between them; either of them is right, and a moment later one
            # of them has to be
            until = time.monotonic() + 20
            while True:
                before = vm_clock("the time in the vm")
                bar = bar_state("what the bar shows")
                after = vm_clock("the time in the vm again")
                if bar.get("clock") in (before, after):
                    break
                if time.monotonic() > until:
                    fail(f"the bar's clock says {bar.get('clock')!r}, date in the vm says {before!r}")
                time.sleep(2)
            ok(f"the bar's clock says {bar['clock']}, the minute date in the vm is in")
            # the bar reads the status on the minute, so a connection that came up after its last
            # tick reaches it at the next one
            until = time.monotonic() + 80
            while True:
                _, output = run("nmcli -t -f TYPE,STATE device status", "what nmcli says about the devices")
                wired = "ethernet:connected" in without_console(output)
                bar = bar_state("what the bar shows")
                if wired == (bar.get("network") == "wired"):
                    break
                if time.monotonic() > until:
                    fail(f"the bar says network {bar.get('network')!r} and nmcli says "
                         f"{'a cable is up' if wired else 'no cable is up'}")
                time.sleep(5)
            if bar.get("menu") != "closed":
                fail(f"the bar says the menu is {bar.get('menu')!r} before anything opened it")
            ok(f"the bar's status: network {bar['network']}, volume {bar['volume']}, battery {bar['battery']}")

            # 5b2. the wallpaper. horizon draws the system's photograph under the windows, scaled to
            # fill the screen, and the first screendump above matched it square by square. rift
            # wallpaper list names every photograph the image ships and the flat grays, and each
            # photograph has a text file next to it with its source and its license. a terminal opens
            # over it, and then rift wallpaper set makes the desktop the flat gray the rest of the test
            # looks for, at once and without the shell starting again
            status, output = run("rift wallpaper list", "the wallpapers")
            listed = without_console(output)
            photos = re.findall(r"^([a-z][a-z0-9-]*)  ", listed, re.M)
            if status != 0 or f"The wallpaper is {WALLPAPER}." not in listed:
                fail(f"rift wallpaper list exited with {status} and does not say {WALLPAPER} is up: "
                     f"{listed.strip()[-600:]!r}")
            if not 6 <= len(photos) <= 10 or WALLPAPER not in photos:
                fail(f"rift wallpaper list names {len(photos)} photographs, expected 6 to 10 with {WALLPAPER}")
            if not re.search(rf"^{DARK_GRAY}  +Dark gray$", listed, re.M) or LIGHT_GRAY not in listed:
                fail(f"rift wallpaper list lacks the flat grays: {listed.strip()[-300:]!r}")
            _, output = run("for photo in /run/current-system/sw/share/backgrounds/rift/*.jpg; "
                            "set about (string replace -r '[.]jpg$' .txt $photo); "
                            "grep -q '^Source: https://' $about; and grep -q '^License: ' $about; "
                            "or echo \"no source or license: $photo\"; end; echo checked",
                            "the text file next to each photograph")
            if "no source or license" in without_console(output) or "checked" not in without_console(output):
                fail(f"a photograph has no source or license next to it: {without_console(output).strip()[-300:]!r}")
            ok(f"rift wallpaper list names {len(photos)} photographs, each with its source and license, "
               f"and the flat grays, and says {WALLPAPER} is up")

            width, height, _ = screendump(args.qmp, work, "wallpaper")
            size = (width, height)
            # the pointer to the middle of the dock, where nothing is and no square of the wallpaper
            point(args.qmp, size, (width // 2, height - 20))
            before = {window for window, _, _ in open_windows("the windows before the terminal")}
            run("horizon msg action spawn -- ghostty", "a terminal over the wallpaper")
            until = time.monotonic() + 120
            while not (opened := [window for window, app, _ in open_windows("the terminal's window")
                                  if window not in before and app == MENU_APP_ID]):
                if time.monotonic() > until:
                    fail("ghostty opened no window over the wallpaper")
                time.sleep(2)
            # a column of half the width at the left, so the right half of the wallpaper still shows
            look("the wallpaper with a window", f"{stem}-wallpaper-window{extension}", 60, settle=3,
                 wallpaper=WALLPAPER_RIGHT, journals=("horizon",))
            for window in opened:
                run(f"horizon msg action close-window --id {window}", "closing the terminal")
            until = time.monotonic() + 60
            while set(opened) & {window for window, _, _ in open_windows("the windows after closing it")}:
                if time.monotonic() > until:
                    fail("the terminal over the wallpaper did not close")
                time.sleep(2)
            # horizon started that terminal in the owner's session, so its shell was the session's first
            # and took fish's greeting, which step 5d looks for in the console
            run("rm -f $XDG_RUNTIME_DIR/rift-greeted-*", "the mark of the session's greeting")

            status, output = run(f"rift wallpaper set '{DARK_GRAY}'", "the flat dark gray")
            if status != 0 or f"The wallpaper is {DARK_GRAY}." not in without_console(output):
                fail(f"rift wallpaper set exited with {status}: {without_console(output).strip()[-300:]!r}")
            look("the desktop in flat gray", f"{stem}-gray{extension}", 30, journals=("horizon",))
            part = without_console(run("cat ~/.local/state/rift/horizon.kdl", "horizon's part")[1])
            if "wallpaper null" not in part or f'background-color "{DARK_GRAY}"' not in part:
                fail(f"the part of horizon's config says {part.strip()[-300:]!r} for {DARK_GRAY}")
            ok(f"rift wallpaper set {DARK_GRAY} made the desktop flat gray at once")

            def wait_for(seconds, ready):
                """Poll until ready() answers something, or give up and answer what it last said."""
                until = time.monotonic() + seconds
                while True:
                    found = ready()
                    if found or time.monotonic() > until:
                        return found
                    time.sleep(2)

            def shot(png, name="dock"):
                """A screendump written as it is, with no checks: a picture for a person to look at."""
                width, height, rgb = screendump(args.qmp, work, name)
                write_png(png, width, height, rgb)
                print(f"\nboot-test: wrote {png}", flush=True)

            # 5c. the Applications menu with the app list in it. Mod+Space runs `lens --menu`, and
            # the list under the field is every desktop entry the session has, in its section
            run("lens --menu", "the Applications menu")
            # the shell reads the line on its socket and opens the menu a moment later
            state = wait_for(20, lambda: next(
                (said for said in [bar_state("the state with the menu open")]
                 if said.get("menu") == "open"), None)) or bar_state("the state without the menu")
            if state.get("menu") != "open":
                # the line reached the shell, since the state query is answered on the same socket,
                # so either the shell never read it or the menu opened and was dismissed at once.
                # a picture and the shell's log say which
                shot(f"{stem}-menu-missing{extension}", "menu-missing")
                _, journal = run("journalctl --user -u lens -b -n 40 | cat", "the shell's log")
                print(f"\nboot-test: the menu did not open. the shell said:\n"
                      f"{without_console(journal).strip()[-2000:]}", flush=True)
                run("lens --menu", "the Applications menu, asked for a second time")
                state = bar_state("the state after the second time")
                if state.get("menu") != "open":
                    fail("lens --state does not say the menu is open after lens --menu, twice, see "
                         f"{stem}-menu-missing{extension}")
                print("\nboot-test: the menu opened the second time it was asked for", flush=True)
            known = int(state.get("apps") or 0)
            listed = int(state.get("rows") or 0)
            if known < 3 or listed < 4 or listed > APP_ROWS:
                fail(f"the menu lists {listed} rows for {known} apps")
            look("the Applications menu", f"{stem}-menu{extension}", 20, menu=True, rows=listed,
                 journals=("lens",))
            ok(f"the menu lists {listed} rows of the {known} apps the shell found")

            # the words in the field filter the list at once, and the best match is the one Enter
            # takes. --type does not press it
            run(f'lens --type "{MENU_APP}"', "an app's name typed into the field")
            state = bar_state("the state with the name in the field")
            found = int(state.get("rows") or 0)
            if state.get("field") != MENU_APP or not 1 <= found < listed:
                fail(f"the field says {state.get('field')!r} with {found} rows under it, "
                     f"expected {MENU_APP!r} with fewer than the {listed} of the whole list")
            look("the app search", f"{stem}-menu-search{extension}", 20, menu=True, rows=found)
            ok(f"typing {MENU_APP!r} left {found} of the {listed} rows")

            # Enter starts the app that is selected and closes the menu. the window it opens is
            # horizon's, so the compositor is what says whether the app started
            run("lens --enter", "enter on the app the list selected")
            until = time.monotonic() + 60
            while True:
                _, output = run("horizon msg --json windows", "horizon's windows")
                window = re.search(r'\{"id":(\d+),"title":(?:null|"(?:[^"\\]|\\.)*"),"app_id":"'
                                   + re.escape(MENU_APP_ID) + r'"', without_console(output).replace("\n", ""))
                if window or time.monotonic() > until:
                    break
                time.sleep(2)
            if not window:
                _, output = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                fail(f"horizon lists no {MENU_APP_ID} window after Enter in the menu: "
                     f"{without_console(output).strip()[-400:]!r}")
            if bar_state("the state after the app started").get("menu") != "closed":
                fail("the menu is still open after Enter started the app")
            run(f"horizon msg action close-window --id {window.group(1)}", f"closing {MENU_APP}'s window")
            look("the desktop after the app closed", f"{stem}-menu-started{extension}", 30)
            ok(f"the menu started {MENU_APP}, window {window.group(1)}, and closed itself")

            # the field, with the same four interpreters as before, under the same list
            run(f'lens --enter "{RESULT_LINE}"', "a pipeline typed into the field")
            look("the result list", f"{stem}-lens{extension}", 20, menu=True, rows=RESULT_ROWS,
                 journals=("lens",))
            printed = bar_state("the state with the list open").get("rows")
            if printed != str(RESULT_ROWS):
                fail(f"lens --state says {printed} rows under the field, expected {RESULT_ROWS}")

            run(f'lens --enter "{ERROR_LINE}"', "a wrong command typed into the field")
            look("the line under the field", f"{stem}-lens-error{extension}", 20, menu=True, line=True)
            if "error" not in bar_state("the state with the error line"):
                fail("lens --state prints no error after a command it does not understand")

            run("lens --escape", "escape in the field")
            look("the menu back at the field and the app list", f"{stem}-lens-empty{extension}", 20,
                 menu=True, rows=listed)
            run("lens --escape", "escape again, which closes the menu")
            look("the desktop with the menu closed", f"{stem}-lens-closed{extension}", 20)
            if bar_state("the state with the menu closed").get("menu") != "closed":
                fail("the menu is still open after the second escape")
            ok("the menu opened under the bar, took a line, cleared it and closed")

            # 5c2. the dock along the bottom. it is a surface of its own, made when the shell
            # starts, and it lists the apps that stay in it before anything is running
            _, output = run("horizon msg --json layers", "horizon's layer surfaces again")
            if not re.search(r'"namespace":\s*"lens-dock"', without_console(output)):
                fail("horizon lists no layer surface named lens-dock")

            def dock_items(what):
                """What the dock lists, as a dict of app id to (its windows, whether it is in front)."""
                listed = bar_state(what).get("dock") or ""
                found = {}
                for word in listed.split():
                    key, _, count = word.rpartition(":")
                    found[key] = (int(count.rstrip("*") or 0), count.endswith("*"))
                return found

            def dock_when(what, seconds, fits):
                """The dock's items once they fit, or what they were when the wait ran out."""
                until = time.monotonic() + seconds
                while True:
                    items = dock_items(what)
                    if fits(items) or time.monotonic() > until:
                        return items
                    time.sleep(2)

            items = dock_when("what the dock lists", 20, lambda items: list(items) == DOCK_KEPT
                              and not any(windows for windows, _ in items.values()))
            if list(items) != DOCK_KEPT:
                fail(f"the dock lists {list(items)}, expected the apps it keeps, {DOCK_KEPT}")
            if any(windows for windows, _ in items.values()):
                fail(f"the dock shows a window before anything started: {items}")
            # horizon keeps an empty workspace after the last, so there are two with nothing open;
            # the first of them is the one on screen
            spaces = bar_state("the workspaces in the dock").get("workspaces")
            if not (spaces or "").startswith("1*"):
                fail(f"the dock says the workspaces are {spaces!r}, expected the one that is on screen")
            ok(f"the dock lists {' '.join(items)} and workspace {spaces}")

            # a click on the icon of a pinned app that is not running starts it. the pointer goes to
            # the middle of its item, which is where the dock draws it: the padding at the end of
            # the bar, then one item and its gap for each app before it
            width, height, rgb = screendump(args.qmp, work, "dock")
            size = (width, height)
            _, dock_rows = bar_and_dock(width, height, bar_gray_rows(width, height, rgb))
            if not 0.9 * DOCK_HEIGHT <= dock_rows <= 3 * DOCK_HEIGHT:
                fail(f"the dock is {dock_rows} rows of {height}, expected about {DOCK_HEIGHT}")
            scale = dock_rows / DOCK_HEIGHT

            def dock_point(place):
                """Where the middle of the item at this place in the dock is on screen."""
                x = (DOCK_PAD + place * (DOCK_ITEM + DOCK_GAP) + DOCK_ITEM / 2) * scale
                return round(x), round(height - dock_rows / 2)

            def menu_point(place, row, rows):
                """Where the middle of a row of an item's menu is. The menu stands on the dock with
                its left edge where the item is."""
                left = (DOCK_PAD + place * (DOCK_ITEM + DOCK_GAP)) * scale
                top = height - dock_rows - (2 * DOCK_MENU_PAD + rows * DOCK_MENU_ROW) * scale
                return (round(left + DOCK_MENU_WIDTH * scale / 2),
                        round(top + (DOCK_MENU_PAD + (row + 0.5) * DOCK_MENU_ROW) * scale))

            click(args.qmp, size, dock_point(DOCK_KEPT.index(MENU_APP_ID)))
            started = wait_for(60, lambda: [win for win in open_windows("horizon's windows")
                                            if win[1] == MENU_APP_ID])
            if not started:
                _, output = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                fail(f"a click on {MENU_APP}'s icon opened no window: "
                     f"{without_console(output).strip()[-400:]!r}")
            ghostty = started[0][0]
            ok(f"a click on {MENU_APP}'s icon in the dock started it, window {ghostty}")

            # and a second app from a command. it wants a terminal, so lens starts it in one with a
            # class of its own, and the window is the app's and not the terminal's
            run(f'lens --enter "{DOCK_APP}"', f"{DOCK_APP} started from the field")
            second = wait_for(60, lambda: [win for win in open_windows("horizon's windows")
                                           if win[1] not in (MENU_APP_ID, CONSOLE_APP_ID)])
            if not second:
                _, output = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                fail(f"{DOCK_APP} opened no window of its own: "
                     f"{without_console(output).strip()[-400:]!r}")
            other, other_id = second[0][0], second[0][1]
            ok(f"{DOCK_APP} started from a command, window {other} as {other_id}")

            items = dock_when("the dock with both apps in it", 30,
                              lambda items: len(items) > len(DOCK_KEPT))
            key = next((key for key in items if key not in DOCK_KEPT), None)
            if key is None:
                fail(f"the dock does not list {DOCK_APP}: {items}")
            if items[MENU_APP_ID][0] != 1 or items[key][0] != 1:
                fail(f"the dock shows {items}, expected one window each for {MENU_APP_ID} and {key}")
            if not items[key][1]:
                fail(f"the dock does not mark {key} as the app in front: {items}")
            shot(f"{stem}-dock{extension}")
            ok(f"the dock shows {MENU_APP_ID} and {key} with a window mark each, {key} in front")

            # a click on the other app's icon moves horizon's focus to its window
            click(args.qmp, size, dock_point(DOCK_KEPT.index(MENU_APP_ID)))
            moved = wait_for(30, lambda: [win for win in open_windows("horizon's windows")
                                          if win[1] == MENU_APP_ID and win[2]])
            if not moved:
                fail(f"a click on {MENU_APP}'s icon did not move the focus to window {ghostty}")
            items = dock_when("the dock after the click", 20,
                              lambda items: items.get(MENU_APP_ID, (0, False))[1])
            if not items.get(MENU_APP_ID, (0, False))[1]:
                fail(f"the dock does not mark {MENU_APP_ID} as the app in front: {items}")
            ok(f"a click on {MENU_APP}'s icon moved horizon's focus to window {ghostty}")

            # a right click opens the menu of that app: its window by title, New window, Pin to
            # dock and Close
            place = len(DOCK_KEPT)
            click(args.qmp, size, dock_point(place), button="right")
            item = wait_for(20, lambda: bar_state("the state with the item menu open").get("item"))
            if not item or not item.startswith(f"{key} "):
                fail(f"a right click on {key}'s icon says item {item!r}")
            menu_rows = int(item.split()[-1])
            if menu_rows != 4:
                fail(f"the menu of {key} has {menu_rows} rows, expected its window, "
                     "New window, Pin to dock and Close")
            shot(f"{stem}-dock-menu{extension}", "dock-menu")
            ok(f"a right click on {key}'s icon opened its menu with {menu_rows} rows")

            # Pin to dock is the third row of that menu, and it writes the list back
            click(args.qmp, size, menu_point(place, 2, menu_rows))
            kept = wait_for(20, lambda: [word for word in
                                         without_console(run("cat ~/.config/rift/dock", "the pinned list")[1]).split()
                                         if word == key])
            if not kept:
                fail(f"Pin to dock did not write {key} into ~/.config/rift/dock")
            ok(f"Pin to dock kept {key} in the list")

            # the shell starts every app in a scope of its own, so starting the shell again, which
            # stops whatever is left in its unit, leaves both apps running with their windows
            def window_scope(window, what):
                """The scope the program that owns this window runs in, from its pid in horizon's
                list, or what its cgroup says when that is no scope of the shell's."""
                _, printed = run("horizon msg --json windows", what)
                owner = re.search(r'\{"id":' + str(window) + r',"title":(?:null|"(?:[^"\\]|\\.)*"),'
                                  r'"app_id":(?:null|"[^"]*"),"pid":(\d+)',
                                  without_console(printed).replace("\n", ""))
                if not owner:
                    return None
                _, printed = run(f"cat /proc/{owner.group(1)}/cgroup", f"{what}, its cgroup")
                scoped = re.search(r"app-rift-[^/\s]+\.scope", without_console(printed))
                return scoped.group(0) if scoped else without_console(printed).strip()[-200:]

            scopes = {window: window_scope(window, f"the scope of window {window}") for window in (ghostty, other)}
            if not all(scope and scope.startswith("app-rift-") for scope in scopes.values()):
                fail(f"the apps the shell started are not in scopes of their own: {scopes}")
            run("systemctl --user restart lens.service", "the shell started again with both apps open")
            if not wait_for(30, lambda: run("lens --state", "the state after the restart")[0] == 0):
                fail("lens --state does not answer after the shell was started again")
            kept_windows = [win[0] for win in open_windows("horizon's windows after the restart")]
            if ghostty not in kept_windows or other not in kept_windows:
                fail(f"after the shell started again horizon has windows {kept_windows}, where {ghostty} and "
                     f"{other} were open before it")
            items = dock_when("the dock after the restart", 20,
                              lambda items: items.get(key, (0, False))[0] == 1
                              and items.get(MENU_APP_ID, (0, False))[0] == 1)
            if items.get(key, (0, False))[0] != 1 or items.get(MENU_APP_ID, (0, False))[0] != 1:
                fail(f"the dock lists {items} after the restart, expected a window each for {MENU_APP_ID} and {key}")
            ok(f"the shell started again and both apps kept their windows, each in a scope of its own: "
               f"{', '.join(scopes.values())}")

            # with both apps ended, the app it pinned is still there, with no window marks under it.
            # each app is its scope now, so stopping the scope ends it; closing the window would have
            # Ghostty ask first, since Helix is still running in it
            run("systemctl --user stop " + " ".join(f"'{scope}'" for scope in scopes.values()),
                "both apps stopped with their scopes")
            items = dock_when("the dock with both apps ended", 30, lambda items: items.get(key) == (0, False)
                              and items.get(MENU_APP_ID) == (0, False))
            if items.get(key) != (0, False) or items.get(MENU_APP_ID) != (0, False):
                fail(f"the dock lists {items} with both apps ended, expected {MENU_APP_ID} and {key} in it with "
                     f"no window")
            look("the desktop with the dock", f"{stem}-dock-desktop{extension}", 30, journals=("lens",))
            ok(f"stopping their scopes ended both apps, and {key} is still in the dock: {' '.join(items)}")

            # 5d. the console. Mod+Grave runs toggle-console with the arguments in
            # nix/modules/horizon.nix, and horizon msg runs the same action without the key. the first
            # time it starts ghostty, after that it hides and shows that same window
            status, printed = run("ghostty +validate-config", "ghostty's settings")
            printed = without_console(printed).strip()
            if status != 0 or printed:
                fail(f"ghostty does not take the settings file the image writes: {printed!r}")
            toggle = (f"horizon msg action toggle-console --app-id {CONSOLE_APP_ID} -- "
                      f"systemd-cat -t console ghostty --class={CONSOLE_APP_ID} --window-decoration=none")

            def console_window():
                """(id, pid) of the console's window in horizon's list, or None when it is not there."""
                status, output = run("horizon msg --json windows", "horizon's windows")
                output = without_console(output).replace("\n", "")
                if status != 0 or "[" not in output:
                    fail(f"horizon msg windows exited with {status}: {output.strip()[-300:]!r}")
                window = re.search(r'\{"id":(\d+),"title":(?:null|"(?:[^"\\]|\\.)*"),"app_id":"'
                                   + re.escape(CONSOLE_APP_ID) + r'","pid":(\d+)', output)
                return (int(window.group(1)), int(window.group(2))) if window else None

            def children(pid, what):
                _, output = run(f"pgrep -P {pid}", what)
                return re.findall(r"^\s*(\d+)\s*$", without_console(output), re.M)

            run(toggle, "the show action")
            look("the console", f"{stem}-console{extension}", 60, console=True, journals=("console", "horizon"))
            window = console_window()
            if not window:
                fail("horizon lists no console window after the show action")
            window_id, pid = window
            programs = children(pid, "the program in the console")
            if not programs:
                fail(f"ghostty {pid} runs nothing in the console")
            shell = programs[0]
            ok(f"the console is open under the bar, window {window_id}, ghostty {pid}, shell {shell}")

            run(toggle, "the hide action")
            look("the desktop and the bar with the console hidden", f"{stem}-console-hidden{extension}", 20)
            if console_window():
                fail("horizon still lists the console window after the hide action")
            status, _ = run(f"kill -0 {shell}", "the shell in the hidden console")
            if status != 0:
                fail(f"the console's shell {shell} ended when the console was hidden")
            ok(f"the console is hidden and its shell {shell} still runs")

            run(toggle, "the show action again")
            look("the console again", f"{stem}-console-again{extension}", 20, console=True, journals=("console", "horizon"))
            again = console_window()
            if again != window:
                fail(f"the console came back as {again}, expected window {window_id} of ghostty {pid}")
            if shell not in children(pid, "the program in the console again"):
                fail(f"the console's shell {shell} is gone after showing it again")
            run(toggle, "the hide action again")
            look("the bar back without the console", f"{stem}-console-closed{extension}", 20)
            ok(f"the same console came back with shell {shell} and went away again")

            # 5e. the lock screen. logind signals the session greetd opened when it is asked to lock
            # it, the listener horizon started runs horizon-lock, and the password goes in on the vm's
            # keyboard through the monitor. the console is open while the session is locked and
            # has to come back as it was
            _, output = run("for s in (loginctl list-sessions --no-legend | string trim | string split -f1 ' '); "
                            "if test (loginctl show-session $s -p Service --value) = greetd; "
                            "echo session=$s class=(loginctl show-session $s -p Class --value); end; end",
                            "the session greetd opened")
            found = re.search(r"session=(\S+) class=(\S+)", output)
            if not found:
                fail(f"logind lists no session from greetd: {without_console(output).strip()[-400:]!r}")
            session, session_class = found.group(1), found.group(2)
            if session_class != "user":
                fail(f"greetd's session {session} is a {session_class} session, logind locks only user sessions")
            _, output = run("grep '^N:' /proc/bus/input/devices", "the input devices")
            print(f"\nboot-test: the vm's input devices:\n{without_console(output)}", flush=True)

            def locked_hint(wanted, what):
                """Wait up to ten seconds for logind's LockedHint on the session to say wanted."""
                hint = None
                for _ in range(10):
                    _, output = run(f"loginctl show-session {session} -p LockedHint --value", "the locked hint")
                    found = re.search(r"^(yes|no)$", without_console(output), re.M)
                    hint = found.group(1) if found else without_console(output).strip()
                    if hint == wanted:
                        return
                    time.sleep(1)
                fail(f"logind says LockedHint={hint} for session {session} {what}, expected {wanted}")

            def press(*keys, what):
                """Press keys on the vm's keyboard. Each item is a list of qemu key codes held together."""
                commands = [{"execute": "send-key", "arguments": {"keys": [{"type": "qcode", "data": code} for code in held]}}
                            for held in keys]
                try:
                    qmp(args.qmp, *commands)
                except (OSError, RuntimeError) as e:
                    fail(f"typing {what}: {e}")

            def type_line(text, what):
                press(*([c] for c in text), ["ret"], what=what)

            run(toggle, "the show action before locking")
            look("the console before locking", f"{stem}-lock-console{extension}", 20, console=True, journals=("console", "horizon"))
            status, output = run(f"loginctl lock-session {session}", "loginctl lock-session")
            if status != 0:
                fail(f"loginctl lock-session {session} exited with {status}: {without_console(output).strip()!r}")
            look("the lock screen", f"{stem}-lock{extension}", 30, lock=False, journals=("lock", "horizon"))
            locked_hint("yes", "with the lock screen up")
            ok(f"loginctl locked session {session}, the lock screen covers the console and the bar")

            type_line(WRONG_PASSWORD, "a wrong password")
            look("the lock screen refusing a wrong password", f"{stem}-lock-refused{extension}", 30, lock=True,
                 journals=("lock", "horizon"))
            locked_hint("yes", "after a wrong password")
            ok("a wrong password was refused and the session stayed locked")

            type_line(PASSWORD, "the owner's password")
            look("the console after unlocking", f"{stem}-lock-unlocked{extension}", 30, console=True, journals=("lock", "horizon"))
            locked_hint("no", "after the owner's password")
            if console_window() != window:
                fail(f"the console came back as {console_window()} after unlocking, expected window {window_id} of ghostty {pid}")
            if shell not in children(pid, "the program in the console after unlocking"):
                fail(f"the console's shell {shell} is gone after unlocking")
            run(toggle, "the hide action after unlocking")
            look("the desktop and the bar after unlocking", f"{stem}-lock-desktop{extension}", 20)
            ok(f"the owner's password unlocked it, the console came back with shell {shell}")

            # Mod+L on the same keyboard runs horizon-lock from the bind
            press(["meta_l", "l"], what="Mod+L")
            look("the lock screen from Mod+L", f"{stem}-lock-key{extension}", 30, lock=False, journals=("lock", "horizon"))
            type_line(PASSWORD, "the owner's password")
            look("the desktop and the bar after unlocking again", f"{stem}-lock-desktop-again{extension}", 30,
                 journals=("lock", "horizon"))
            locked_hint("no", "after unlocking the lock from Mod+L")
            ok("Mod+L locked the session and the owner's password unlocked it")

            # 5e2. the system menu. the status icons at the right of the bar are one button, and a
            # click on it opens the menu under them. the vm has a sink and a cable and nothing else,
            # so the menu is the volume slider, the cable, and the session
            state = bar_state("the state before the system menu")
            if state.get("system") != "closed":
                fail(f"lens --state says the system menu is {state.get('system')!r} before anything opened it")
            if state.get("wired") != "connected":
                fail(f"lens --state says wired {state.get('wired')!r}, and the vm's cable is up")
            absent = {key: state.get(key) for key in ("wifi", "bluetooth", "brightness", "battery")}
            if any(value != "none" for value in absent.values()):
                fail(f"lens --state says {absent}, and the vm has no wireless card, adapter, backlight or battery")
            ok(f"lens --state says wired connected and none for {', '.join(absent)}")

            def volume_now():
                """The default sink's volume as wpctl reads it, or None."""
                _, output = run("wpctl get-volume @DEFAULT_AUDIO_SINK@", "the sink's volume")
                found = re.search(r"Volume: (\d+\.\d+)", without_console(output))
                return float(found.group(1)) if found else None

            def system_open(what):
                """The size lens says the system menu has, when it is open."""
                shown = bar_state(what).get("system") or ""
                if not shown.startswith("open "):
                    return None
                return tuple(int(number) for number in shown.split()[1].split("x"))

            width, height, rgb = screendump(args.qmp, work, "system")
            size = (width, height)
            bar_rows, _ = bar_and_dock(width, height, bar_gray_rows(width, height, rgb))
            if not 0.9 * BAR_HEIGHT <= bar_rows <= 3 * BAR_HEIGHT:
                fail(f"the bar is {bar_rows} rows of {height}, expected about {BAR_HEIGHT}")
            scale = bar_rows / BAR_HEIGHT
            # the last status icon is the button's padding and half an icon inside the bar's padding
            status_icons = (round(width - (BAR_PAD + STATUS_PAD + STATUS_ICON / 2) * scale), round(bar_rows / 2))
            menu_left = width - (SYSTEM_MARGIN + SYSTEM_WIDTH) * scale

            def session_row(label, menu_size):
                """Where the middle of a row of the session is: the four rows at the bottom of the menu."""
                bottom = bar_rows + (menu_size[1] - SYSTEM_PAD) * scale
                below = len(SESSION_ROWS) - SESSION_ROWS.index(label) - 0.5
                return round(menu_left + SYSTEM_WIDTH * scale / 2), round(bottom - below * SYSTEM_ROW * scale)

            before = volume_now()
            if before is None:
                fail("wpctl reads no volume for the default sink")
            click(args.qmp, size, status_icons)
            menu_size = wait_for(20, lambda: system_open("the state after a click on the status icons"))
            if not menu_size:
                _, output = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                fail(f"a click on the status icons opened no system menu: {without_console(output).strip()[-400:]!r}")
            # the pointer over the button would cover a corner of the menu in the screendump
            point(args.qmp, size, (round(width / 3), round(height / 2)))
            look("the system menu", f"{stem}-system{extension}", 20, system=menu_size, journals=("lens",))
            ok(f"a click on the status icons opened the system menu, {menu_size[0]}x{menu_size[1]}")

            # the volume slider is the first row: the mute button at its left, the slider from there to
            # the row's end. a click on it sets the sink to where it landed
            wanted = 0.75 if before < 0.5 else 0.25
            # not start and end: start is the test's own clock
            rail_left = menu_left + (SYSTEM_PAD + SYSTEM_ROW + SYSTEM_GAP) * scale
            rail_right = menu_left + (SYSTEM_WIDTH - SYSTEM_PAD - SYSTEM_INSET) * scale
            click(args.qmp, size, (round(rail_left + wanted * (rail_right - rail_left)),
                                   round(bar_rows + (SYSTEM_PAD + SYSTEM_ROW / 2) * scale)))

            def moved():
                level = volume_now()
                return level is not None and abs(level - wanted) <= 0.03 and level > 0

            if not wait_for(20, moved):
                fail(f"a click at {wanted:.0%} of the volume slider left the sink at {volume_now()}, it was {before}")
            after = volume_now()
            shown = wait_for(20, lambda: bar_state("the bar's volume").get("volume") == str(round(after * 100)))
            if not shown:
                fail(f"wpctl says {after} and the bar says volume {bar_state('the bar volume').get('volume')!r}")
            shot(f"{stem}-system-volume{extension}", "system-volume")
            ok(f"a click on the volume slider moved the sink from {before:.2f} to {after:.2f}, and the bar follows")

            # the button at the left of the slider mutes the sink, and the bar's icon and state follow;
            # a second press unmutes it
            mute = (round(menu_left + (SYSTEM_PAD + SYSTEM_ROW / 2) * scale),
                    round(bar_rows + (SYSTEM_PAD + SYSTEM_ROW / 2) * scale))
            for muted in (True, False):
                click(args.qmp, size, mute)
                word = f"{round(after * 100)} muted" if muted else str(round(after * 100))

                def followed():
                    _, output = run("wpctl get-volume @DEFAULT_AUDIO_SINK@", "whether the sink is muted")
                    return ("[MUTED]" in without_console(output)) == muted and \
                        bar_state("the bar's volume after the mute button").get("volume") == word

                if not wait_for(20, followed):
                    fail(f"the mute button did not {'mute' if muted else 'unmute'} the sink: the bar says "
                         f"{bar_state('the bar volume').get('volume')!r}, expected {word!r}")
            ok("the mute button muted the sink and unmuted it, and the bar followed both times")

            # a second click on the status icons closes the menu, and it stays closed
            click(args.qmp, size, status_icons)
            if not wait_for(20, lambda: bar_state("the state after a second click").get("system") == "closed"):
                fail("a second click on the status icons left the system menu open")
            time.sleep(2)
            if bar_state("the state a moment later").get("system") != "closed":
                fail("the system menu opened again after the click that closed it")
            ok("a second click on the status icons closed the system menu")

            # Restart asks first. the shell runs as a user unit, outside the session, and logind has to
            # let it restart the machine without a password, or the row would do nothing
            _, output = run("systemd-run --user --wait --pipe --quiet busctl call org.freedesktop.login1 "
                            "/org/freedesktop/login1 org.freedesktop.login1.Manager CanReboot",
                            "whether logind allows the shell to restart the machine")
            if '"yes"' not in without_console(output):
                fail(f"logind does not let a user unit restart the machine: {without_console(output).strip()!r}")
            click(args.qmp, size, status_icons)
            menu_size = wait_for(20, lambda: system_open("the system menu again"))
            if not menu_size:
                fail("a click on the status icons did not open the system menu again")
            click(args.qmp, size, session_row("Restart", menu_size))
            if not wait_for(20, lambda: bar_state("the state after Restart").get("dialog") == "Restart the computer?"):
                fail(f"Restart asked nothing: dialog {bar_state('the dialog').get('dialog')!r}")
            shot(f"{stem}-system-restart{extension}", "system-restart")
            press(["esc"], what="escape in the dialog")
            if not wait_for(20, lambda: bar_state("the state after escape").get("dialog") == "closed"):
                fail("escape did not close the dialog that asks about restarting")
            ok("Restart in the system menu asked first, logind allows the shell to restart, and escape said no")

            # Lock starts the lock screen, the same one logind's signal and Mod+L start
            click(args.qmp, size, status_icons)
            menu_size = wait_for(20, lambda: system_open("the system menu for Lock"))
            if not menu_size:
                fail("a click on the status icons did not open the system menu for Lock")
            click(args.qmp, size, session_row("Lock", menu_size))
            look("the lock screen from the system menu", f"{stem}-system-lock{extension}", 30, lock=False,
                 journals=("lock", "horizon"))
            locked_hint("yes", "after Lock in the system menu")
            type_line(PASSWORD, "the owner's password")
            look("the desktop after unlocking the lock from the system menu", f"{stem}-system-unlocked{extension}", 30,
                 journals=("lock", "horizon"))
            locked_hint("no", "after unlocking the lock from the system menu")
            if bar_state("the state after the lock").get("system") != "closed":
                fail("the system menu is still open after the lock")
            ok("Lock in the system menu locked the session and the owner's password unlocked it")

            # 5e3. notifications, the clock menu and the key popup. lens serves notifications on the
            # session bus, and notify-send, from libnotify, sends them from the serial shell
            state = bar_state("the state before any notification")
            quiet = {key: state.get(key) for key in ("notifications", "banners", "clock-menu", "popup", "do-not-disturb")}
            if quiet != {"notifications": "0 0", "banners": "none", "clock-menu": "closed", "popup": "closed",
                         "do-not-disturb": "off"}:
                fail(f"lens --state says {quiet} before anything was sent")
            _, output = run("busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications "
                            "org.freedesktop.Notifications GetServerInformation", "who serves notifications")
            if '"Lens"' not in without_console(output):
                fail(f"the session bus has no notification server from lens: {without_console(output).strip()!r}")
            ok("lens serves org.freedesktop.Notifications")

            def notices(what):
                """(on screen, kept) from lens --state."""
                counted = (bar_state(what).get("notifications") or "").split()
                return tuple(int(number) for number in counted) if len(counted) == 2 else None

            def banner_sizes(what):
                """The sizes of the notifications on screen, top to bottom."""
                printed = bar_state(what).get("banners") or "none"
                return [] if printed == "none" else [tuple(int(n) for n in size.split("x")) for size in printed.split()]

            width, height, rgb = screendump(args.qmp, work, "notify")
            size = (width, height)
            bar_rows, dock_rows = bar_and_dock(width, height, bar_gray_rows(width, height, rgb))
            scale = bar_rows / BAR_HEIGHT
            away = (round(width / 3), round(height / 2))
            point(args.qmp, size, away)

            # a critical notification stays until it is closed
            status, output = run(f'notify-send --urgency=critical --icon=dialog-information "{NOTIFY_SUMMARY}" '
                                 '"A critical notification stays on screen until it is closed."',
                                 "a critical notification")
            if status != 0:
                fail(f"notify-send exited with {status}: {without_console(output).strip()!r}")
            if wait_for(20, lambda: notices("the state after notify-send") == (1, 1)) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) after notify-send, expected (1, 1)")
            sizes = banner_sizes("the notification's size")
            if len(sizes) != 1 or sizes[0][0] != NOTIFY_WIDTH:
                fail(f"lens says the notification on screen is {sizes}")
            look("the notification", f"{stem}-notification{extension}", 20, notification=sizes[0], journals=("lens",))
            if bar_state("the newest notification").get("latest") != NOTIFY_SUMMARY:
                fail(f"lens keeps {bar_state('the newest').get('latest')!r}, expected {NOTIFY_SUMMARY!r}")
            time.sleep(7)
            if notices("the critical notification a while later") != (1, 1):
                fail("the critical notification did not stay on screen")
            ok(f"notify-send put a critical notification under the bar at the right, {sizes[0][0]}x{sizes[0][1]}, "
               "and it stayed")

            # its close button closes it, and a notification the owner closed leaves the list too
            banner_top = bar_rows + NOTIFY_GAP * scale
            banner_right = width - NOTIFY_GAP * scale
            click(args.qmp, size, (round(banner_right - (NOTIFY_PAD + NOTIFY_CLOSE / 2) * scale),
                                   round(banner_top + (NOTIFY_PAD + NOTIFY_CLOSE / 2) * scale)))
            if wait_for(20, lambda: notices("the state after the close button") == (0, 0)) is not True:
                fail(f"the close button left {notices('the notifications')} (on screen, kept)")
            # away only once the click did what it does, and off where the next one will stand
            point(args.qmp, size, away)
            ok("the close button closed the critical notification")

            # a button for an action: notify-send waits for it and prints the action's key
            run('notify-send --action=open=Open --icon=dialog-information "Rift boot test" "A notification with a button." '
                '< /dev/null > /tmp/rift-action.txt 2>&1 &; disown', "a notification with a button")
            if wait_for(20, lambda: notices("the state with the button") == (1, 1)) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) after a notification with a button")
            sizes = banner_sizes("the size with a button")
            words_left = width - (NOTIFY_GAP + NOTIFY_WIDTH - NOTIFY_PAD - NOTIFY_ICON - NOTIFY_ICON_GAP) * scale
            click(args.qmp, size, (round(words_left + 40 * scale),
                                   round(banner_top + (sizes[0][1] - NOTIFY_PAD - NOTIFY_BUTTON / 2) * scale)))
            if wait_for(20, lambda: "open" in without_console(run("cat /tmp/rift-action.txt", "what notify-send printed")[1]).split()) is not True:
                fail(f"notify-send printed {without_console(run('cat /tmp/rift-action.txt', 'notify-send')[1]).strip()!r} "
                     "after a click on the button, expected the action's key")
            if wait_for(20, lambda: notices("the state after the button") == (0, 0)) is not True:
                fail(f"the button left {notices('the notifications')} (on screen, kept)")
            # a notification under the pointer keeps its time, so the pointer goes before the next one
            point(args.qmp, size, away)
            time.sleep(1)
            ok("a click on the notification's button told notify-send, and the notification closed")

            # a notification that is not critical goes after five seconds and stays in the list
            run(f'notify-send "{NOTIFY_SUMMARY}" "This one closes by itself."', "a notification that closes by itself")
            if wait_for(10, lambda: notices("the state with the notification") == (1, 1)) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) after notify-send")
            if wait_for(20, lambda: notices("the state a while later") == (0, 1)) is not True:
                fail(f"the notification is still up after its five seconds: {notices('the notifications')}")
            ok("a notification went after five seconds and stayed in the list")

            # a click on the clock opens the clock menu, which lists it
            clock_point = (round(width / 2), round(bar_rows / 2))
            click(args.qmp, size, clock_point)

            def clock_open(what):
                """The size lens says the clock menu has, when it is open."""
                shown = bar_state(what).get("clock-menu") or ""
                return tuple(int(number) for number in shown.split()[1].split("x")) if shown.startswith("open ") else None

            menu_size = wait_for(20, lambda: clock_open("the state after a click on the clock"))
            if not menu_size:
                fail("a click on the clock opened no clock menu")
            point(args.qmp, size, away)
            time.sleep(1)
            look("the clock menu", f"{stem}-clock{extension}", 20, clock=menu_size, journals=("lens",))
            if bar_state("what the clock menu lists").get("latest") != NOTIFY_SUMMARY:
                fail(f"the clock menu lists {bar_state('the list').get('latest')!r}, expected {NOTIFY_SUMMARY!r}")
            ok(f"a click on the clock opened the clock menu, {menu_size[0]}x{menu_size[1]}, with the notification in it")

            # Do not disturb keeps a notification off the screen and in the list. the switch is at the
            # right of the last row, and the menu grows by a row when the list does

            def switch_point(menu_size):
                menu_left = (width - CLOCK_WIDTH * scale) / 2
                return (round(menu_left + (CLOCK_WIDTH - CLOCK_PAD - CLOCK_INSET - SWITCH) * scale),
                        round(bar_rows + (menu_size[1] - CLOCK_PAD - CLOCK_ROW / 2) * scale))

            click(args.qmp, size, switch_point(menu_size))
            if wait_for(20, lambda: bar_state("the state after the switch").get("do-not-disturb") == "on") is not True:
                fail("the Do not disturb switch did not turn it on")
            run(f'notify-send "{NOTIFY_SUMMARY}" "Do not disturb keeps this one quiet."', "a notification while it is on")
            if wait_for(20, lambda: notices("the state with Do not disturb") == (0, 2)) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) with Do not disturb on, expected (0, 2)")
            grown = wait_for(20, lambda: (clock_open("the clock menu with two") or (0, 0))[1] > menu_size[1]
                             and clock_open("the clock menu with two"))
            if not grown:
                fail(f"the clock menu is {clock_open('the clock menu')} with two notifications, it was {menu_size}")
            point(args.qmp, size, away)
            time.sleep(1)
            shot(f"{stem}-clock-quiet{extension}", "clock-quiet")
            click(args.qmp, size, switch_point(grown))
            if wait_for(20, lambda: bar_state("the state after the switch again").get("do-not-disturb") == "off") is not True:
                fail("the Do not disturb switch did not turn it off again")
            ok("with Do not disturb on a notification went into the list without showing")

            # a second click on the clock closes the menu, and it stays closed
            click(args.qmp, size, clock_point)
            if wait_for(20, lambda: bar_state("the state after a second click on the clock").get("clock-menu") == "closed") is not True:
                fail("a second click on the clock left the clock menu open")
            time.sleep(2)
            if bar_state("the clock menu a moment later").get("clock-menu") != "closed":
                fail("the clock menu opened again after the click that closed it")
            point(args.qmp, size, away)
            ok("a second click on the clock closed the clock menu")

            # the volume key shows the popup for a second. a full check of a screendump takes seconds,
            # longer than the popup is up, so the screendumps follow the key as fast as they come and
            # each is only looked at in three points of the popup's padding; the one that has it gets
            # the full check

            def popup_up(width, height, rgb):
                """Whether the popup's gray is in the padding along its top edge."""
                y = round(height - dock_rows - (POPUP_ABOVE + POPUP_SIZE[1] - 4) * scale)
                points = [round(width / 2 + offset * scale) for offset in (-90, 0, 90)]
                return all(near(rgb[(y * width + x) * 3:(y * width + x) * 3 + 3], MENU, 3) for x in points)

            def popup_after(what, send):
                """Send a key or a command, then screendumps for three seconds. Returns the first that has
                the popup, or None, and the last one taken."""
                send()
                until = time.monotonic() + 3
                frame = None
                while time.monotonic() < until:
                    frame = screendump(args.qmp, work, "popup")
                    if popup_up(*frame):
                        return frame, frame
                print(f"\nboot-test: no popup in three seconds after {what}", flush=True)
                return None, frame

            before = volume_now()
            found = None
            for _ in range(3):
                found, last = popup_after("the volume key", lambda: press(["volumeup"], what="the volume key"))
                if found:
                    break
            if not found:
                write_png(f"{stem}-popup{extension}", *last)
                pressed = volume_now()
                # the verb the key runs, from the serial line, tells the key and the popup apart
                by_hand, _ = popup_after("lens --volume up", lambda: run("lens --volume up", "the verb the volume key runs"))
                _, output = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                _, horizon_log = run("journalctl -b -t horizon -o cat -n 20 --no-pager | cat", "horizon's log")
                fail(f"the volume key showed no popup: the sink was {before} and is {pressed} after three presses, "
                     f"and lens --volume up {'did' if by_hand else 'did not'} show it. lens: "
                     f"{without_console(output).strip()[-300:]!r} horizon: {without_console(horizon_log).strip()[-300:]!r}")
            good, lines = check_desktop(*found, lens=True, popup=POPUP_SIZE)
            write_png(f"{stem}-popup{extension}", *found)
            print("\nboot-test: " + "\nboot-test: ".join(lines), flush=True)
            if not good:
                fail(f"the key popup is not where it belongs, see {stem}-popup{extension}")
            after = volume_now()
            if before is None or after is None or after <= before:
                fail(f"the volume key left the sink at {after}, it was {before}")
            shown = wait_for(20, lambda: bar_state("the bar's volume after the key").get("volume") == str(round(after * 100)))
            if not shown:
                fail(f"wpctl says {after} and the bar says volume {bar_state('the bar volume').get('volume')!r}")
            if wait_for(10, lambda: bar_state("the popup a moment later").get("popup") == "closed") is not True:
                fail("the key popup did not go away")
            ok(f"the volume key turned the sink up from {before:.2f} to {after:.2f} and showed the popup")

            # 5f. a text console. ctrl+alt+f2 moves to the second one, where logind starts a getty that
            # shows /etc/issue: the name line without the logo, and a login that asks for a name, since
            # only the serial console logs in by itself. ctrl+alt+f1 goes back to the desktop, which
            # horizon draws again
            press(["ctrl", "alt", "f2"], what="ctrl+alt+f2")
            waited = time.monotonic() + 30
            while (state := unit_state("getty@tty2")) != "active":
                if time.monotonic() > waited:
                    fail(f"no getty runs on tty2 after ctrl+alt+f2, getty@tty2 is {state}")
                time.sleep(2)
            waited = time.monotonic() + 20
            while True:
                try:
                    width, height, rgb = screendump(args.qmp, work, "tty2")
                except (OSError, RuntimeError) as e:
                    fail(f"screendump: {e}")
                good, lines = check_tty(width, height, rgb)
                if good or time.monotonic() > waited:
                    break
                time.sleep(2)
            write_png(f"{stem}-tty2{extension}", width, height, rgb)
            print("\nboot-test: " + "\nboot-test: ".join(lines), flush=True)
            if not good:
                fail(f"/etc/issue is not on tty2, see {stem}-tty2{extension}")
            # the line is wider than the serial console, and systemctl would page it
            _, output = run("systemctl show --no-pager getty@tty2 -p ExecStart --value | cat", "the getty on tty2")
            if "--autologin" in without_console(output):
                fail("the getty on tty2 logs someone in by itself")
            ok("tty2 shows the name and a login from /etc/issue without the logo, and logs no one in by itself")
            press(["ctrl", "alt", "f1"], what="ctrl+alt+f1")
            look("the desktop back from tty2", f"{stem}-tty2-back{extension}", 30, journals=("horizon",))

            # 5g. a question for quasar, from the terminal first, which prints the answer here, and
            # then typed into the field. the answer is as many rows as the model makes it, so the
            # list is only expected to have at least one
            if args.models:
                status, output = run(f'lens --do "{QUESTION}"', "quasar's answer through lens")
                # the journal's lines on the console land in the output too
                said = "\n".join(line for line in output.splitlines()
                                 if line.strip() and not re.match(r"\s*\[\s*\d+\.\d+\] ", line))
                if status != 0 or not said:
                    fail(f"lens --do could not ask quasar: {output.strip()!r}")
                ok(f"lens asked quasar and printed {said!r}")

                run(f'lens --enter "{QUESTION}"', "a question typed into the field")
                look("the answer under the field", f"{stem}-lens-answer{extension}", args.answer_timeout,
                     menu=True, rows=(1, OUTPUT_ROWS), journals=("lens",))
                run("lens --escape", "escape after the answer")
                run("lens --escape", "escape again, which closes the menu")

            # 5h. the shell draws the bar and the menu in one process, so it runs as a user unit
            # that restarts: killing it brings the bar back by itself
            status, _ = run("pkill -x lens", "killing the shell")
            if status != 0:
                fail("pkill found no lens process to kill")
            look("the bar after the shell was killed", f"{stem}-lens-restarted{extension}", 30,
                 journals=("lens",))
            _, output = run("systemctl --user show -p NRestarts --value lens.service | cat",
                            "how often the shell has restarted")
            restarts = re.search(r"^(\d+)\s*$", without_console(output), re.M)
            if not restarts or int(restarts.group(1)) < 1:
                fail(f"systemd did not restart the shell: NRestarts={without_console(output).strip()!r}")
            ok(f"the shell came back after it was killed, restart {restarts.group(1)}")

            # 5i. apps draw their own title bars. what they read is there first: the dark colour scheme
            # in dconf, and the cursor and qt's platform theme in the user manager, which lens starts
            # apps from
            if bar_state("the theme the shell runs with").get("theme") != "dark":
                fail(f"lens --state says theme {bar_state('the theme').get('theme')!r}, expected dark")
            _, output = run("dconf read /org/gnome/desktop/interface/color-scheme", "the colour scheme apps read")
            if "'prefer-dark'" not in without_console(output):
                fail(f"dconf reads the colour scheme as {without_console(output).strip()!r}, expected 'prefer-dark'")
            _, output = run("systemctl --user show-environment | cat", "the user manager's environment")
            missing = [word for word in ("XCURSOR_THEME=Adwaita", "XCURSOR_SIZE=24", "QT_QPA_PLATFORMTHEME=gtk3",
                                         "QT_WAYLAND_DECORATION=adwaita", "/run/current-system/sw/lib/qt-5.")
                       if word not in without_console(output)]
            if missing:
                fail(f"the user manager's environment lacks {', '.join(missing)}")
            ok("dconf has the dark colour scheme, and the user manager the cursor, qt's platform theme, its "
               "decorations and the system's qt plugins")

            width, height, rgb = screendump(args.qmp, work, "apps")
            size = (width, height)
            _, dock_rows = bar_and_dock(width, height, bar_gray_rows(width, height, rgb))
            scale = dock_rows / DOCK_HEIGHT

            def titled_apps(what, png, colors):
                """Start the titled apps from the dock, each at its own place in it, and look for their
                title bars."""
                for app in TITLED_APPS:
                    click(args.qmp, size, dock_point(DOCK_KEPT.index(app)))
                    if not wait_for(120, lambda: [win for win in open_windows(f"{app}'s window")
                                                  if win[1] == app]):
                        _, output = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                        fail(f"a click on {app}'s icon in the dock opened no window: "
                             f"{without_console(output).strip()[-400:]!r}")
                # the pointer goes into the terminal, away from both title bars
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                # a new window's first frame has its title bar before the terminal or the page draws
                look(what, png, 120, apps=TITLED_APPS, colors=colors, journals=("horizon",), settle=3)

            def close_titled_apps():
                for window, app, _ in open_windows("the windows to close"):
                    if app in TITLED_APPS:
                        run(f"horizon msg action close-window --id {window}", f"closing {app}'s window")
                if not wait_for(60, lambda: not [win for win in open_windows("the windows left")
                                                 if win[1] in TITLED_APPS]):
                    fail(f"the windows of {', '.join(TITLED_APPS)} did not close")
                # the last window closing ends firefox, and a click before it has gone would open a
                # window in the process that is ending
                wait_for(30, lambda: run("pgrep -f firefox", "whether firefox has ended")[0] != 0)

            titled_apps("firefox and ghostty with their title bars", f"{stem}-apps{extension}", DARK_COLORS)

            def theme_to(word):
                """Write the owner's theme and the flat gray of that theme as the wallpaper, and start
                the shell again, which hands the theme on to dconf and both to horizon, and wait until
                both have it."""
                scheme = "'default'" if word == "light" else "'prefer-dark'"
                gray = LIGHT_GRAY if word == "light" else DARK_GRAY
                run(f"printf '{word}\\n' > ~/.config/rift/theme; and rift wallpaper set '{gray}'; "
                    f"and systemctl --user restart lens.service", f"the {word} theme")
                if not wait_for(30, lambda: bar_state(f"the shell in {word}").get("theme") == word):
                    fail(f"lens --state does not say theme {word} after the shell started again")
                if not wait_for(30, lambda: scheme in without_console(
                        run("dconf read /org/gnome/desktop/interface/color-scheme", "the colour scheme")[1])):
                    fail(f"the shell did not set the colour scheme to {scheme}")
                part = without_console(run("cat ~/.local/state/rift/horizon.kdl", "horizon's part for the theme")[1])
                if (f": {word}, " not in part or f", {gray}" not in part
                        or f'background-color "{gray}"' not in part
                        or ("#3584e4" in part) != (word == "light")):
                    fail(f"the part of horizon's config says {part.strip()[-300:]!r} for {word}")
                ok(f"the shell handed the {word} theme on to dconf and to horizon")

            # keepassxc, the image's first qt app, from the Applications menu. qt draws its title bar with the
            # adwaita decorations the session names, over the wayland plugin that came with it, dark with the
            # desktop, and not with qt's own decorations and their blue gradient
            def qt_windows(what):
                """Horizon's windows of the qt app."""
                return [win for win in open_windows(what) if QT_APP_ID in win[1].lower()]

            close_titled_apps()
            run("lens --menu", f"the Applications menu for {QT_APP}")
            run(f'lens --type "{QT_APP}"', f"{QT_APP}'s name typed into the field")
            run("lens --enter", f"enter on {QT_APP}")
            if not wait_for(120, lambda: qt_windows(f"{QT_APP}'s window")):
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"the Applications menu opened no {QT_APP_ID} window: {without_console(output).strip()[-800:]!r}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{QT_APP} with its title bar", f"{stem}-keepassxc{extension}", 120, apps=[QT_APP_ID],
                 journals=("horizon", "lens"), settle=3)
            for window, _, _ in qt_windows(f"{QT_APP}'s window to close"):
                run(f"horizon msg action close-window --id {window}", f"closing {QT_APP}'s window")
            if not wait_for(60, lambda: not qt_windows("the windows left")):
                fail(f"{QT_APP}'s window did not close")

            # 5j. the everyday apps, one at a time from the Applications menu by the name the list shows:
            # pictures, documents, video, sound, the calculator, archives, the disks, where the space went
            # and the characters. each opens a window horizon lists, with the title bar gtk draws for it in
            # a gray of the theme, and closes again
            def app_windows(app_id, what):
                """Horizon's windows whose app id has app_id in it."""
                return [win for win in open_windows(what) if app_id in win[1].lower()]

            def open_from_menu(name, app_id):
                """Type an app's name into the Applications menu and press enter, and wait for its window."""
                run("lens --menu", f"the Applications menu for {name}")
                run(f'lens --type "{name}"', f"{name} typed into the field")
                run("lens --enter", f"enter on {name}")
                if not wait_for(180, lambda: app_windows(app_id, f"{name}'s window")):
                    _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                    fail(f"the Applications menu opened no {app_id} window: "
                         f"{without_console(output).strip()[-800:]!r}")

            def close_app(name, app_id):
                for window, _, _ in app_windows(app_id, f"{name}'s window to close"):
                    run(f"horizon msg action close-window --id {window}", f"closing {name}'s window")
                if not wait_for(60, lambda: not app_windows(app_id, "the windows left")):
                    fail(f"{name}'s window did not close")

            def light_pixels(name):
                """How many pixels of the screen are a light neutral gray, which is what a window
                drawn in the light theme is made of. Every window of the session is dark, and the
                desktop behind them is a dark gray or a dark photograph, so a window in the light
                theme is tens of thousands of these and nothing else is more than a few."""
                wide, tall, rgb = screendump(args.qmp, work, name)
                count = 0
                for at in range(0, wide * tall * 3, 3):
                    red, green, blue = rgb[at], rgb[at + 1], rgb[at + 2]
                    if red >= 180 and max(red, green, blue) - min(red, green, blue) <= 12:
                        count += 1
                return count

            def portal_question(what):
                """The rectangle a portal is asking a question in, as (width, height, left, top),
                or None when no portal has a window up."""
                status, output = run("horizon msg --json windows", what)
                printed = without_console(output).replace("\n", "")
                if status != 0:
                    fail(f"horizon msg windows exited with {status}: {printed.strip()[-300:]!r}")
                found = PORTAL_WINDOW.search(printed)
                return found and (int(found.group(1)), int(found.group(2)),
                                  float(found.group(3)), float(found.group(4)))

            def grant_the_camera(name):
                """Answer the portal's question about the camera the way a person would, when there is
                one. Its buttons are along the bottom of its window, deny at the left and grant at the
                right, and a drive whose owner has answered once is not asked again."""
                if not wait_for(60, lambda: portal_question(f"the portal's question for {name}")):
                    print(f"\nboot-test: the portal did not ask before {name} took the camera", flush=True)
                    return
                asked = portal_question("the question's window")
                # and it is drawn in the theme the session set. the portal's dialogs are gtk 3, which
                # has no colour scheme and goes dark by the name of its theme, so the image carries an
                # Adwaita-dark theme for gtk 3 to find. every window of the session is dark, so a
                # light window on the screen is this one drawn in the wrong theme
                light = light_pixels("portal-question")
                if light > PORTAL_LIGHT:
                    shot(f"{stem}-portal-question{extension}", "portal-question")
                    _, env = run(r"cat /proc/(pgrep -f xdg-desktop-portal-gtk | head -1)/environ | "
                                 r"tr '\0' '\n' | grep -E 'GIO_EXTRA_MODULES|XDG_DATA_DIRS|DCONF|GTK'",
                                 "the portal's environment")
                    _, said = run("gsettings get org.gnome.desktop.interface gtk-theme; "
                                  "ls /run/current-system/sw/share/themes", "the theme gsettings gives")
                    print(f"\nboot-test: the portal's environment:\n{without_console(env)}\n"
                          f"the theme gsettings gives and the themes on the drive:\n{without_console(said)}",
                          flush=True)
                    fail(f"the portal's question is drawn in the light theme: {light} pixels of the "
                         f"screen are a light gray, see {stem}-portal-question{extension}")
                ok(f"the portal's question follows the dark theme of the session, {light} light pixels "
                   "on the screen")
                click(args.qmp, size, (asked[2] + asked[0] * 0.75, asked[3] + asked[1] - round(20 * scale)))
                if not wait_for(60, lambda: not portal_question("the question after the answer")):
                    fail(f"the portal kept asking for the camera after {name} was granted it")
                ok(f"the portal asked before {name} took the camera, and took the owner's answer")

            for name, app_id, png, share, asks in BASIC_APPS:
                open_from_menu(name, app_id)
                if asks:
                    grant_the_camera(name)
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                look(f"{name} with its title bar", f"{stem}-{png}{extension}", 120, apps=[app_id],
                     journals=("horizon", "lens"), settle=3, share=share)
                close_app(name, app_id)
            ok(f"the Applications menu opened {len(BASIC_APPS)} everyday apps, each with the title bar it "
               "draws itself, and each closed again")

            # the disk utility reads udisks, and udisks asks polkit before it writes anything. an action
            # on a disk internal to the machine, which is what a host's disk is, is refused outright and
            # has nothing to authenticate; the same action on a removable disk is not refused here. that
            # is the rule that leaves host disks as they are, and it is the reason udisks may run at all
            for action, refused in (("filesystem-mount-system", True), ("filesystem-mount", False)):
                _, output = run(f"pkcheck --action-id org.freedesktop.udisks2.{action} --process $fish_pid",
                                f"whether the owner may {action}")
                said = without_console(output)
                if refused != ("Not authorized." in said):
                    fail(f"polkit answers {said.strip()[-200:]!r} for {action}, expected "
                         f"{'a refusal with nothing to authenticate' if refused else 'no refusal'}")
            ok("polkit refuses every udisks action on a disk the machine boots from, and refuses none on "
               "a removable one")
            # and the whole way through: mounting a disk that belongs to the machine is refused. the
            # backup disk is one of the machine's own, with a file system on it and nothing mounted
            # from it, which is what a host's disk is. the drive's own exchange partition is no longer
            # the example here: the system mounts that one itself, at /exchange, and 5o checks it
            if args.backup:
                status, output = run("udisksctl mount --no-user-interaction "
                                     "-b (realpath /dev/disk/by-label/backup)",
                                     "mounting a disk that belongs to the machine")
                refusal = without_console(output)
                if status == 0 or "Not authorized" not in refusal:
                    fail(f"udisks did not refuse the mount: it exited with {status} and said "
                         f"{refusal.strip()[-300:]!r}")
                ok("udisks refuses to mount a disk that belongs to the machine")

            # a file opens with the app that owns its kind, and the image viewer draws a real photograph:
            # it reads the file in a sandbox of its own, one loader per format, so a picture on screen says
            # that sandbox works
            wrong = []
            for kind, desktop in DEFAULT_APPS:
                _, output = run(f"xdg-mime query default {kind}", f"what opens {kind}")
                if desktop not in without_console(output):
                    wrong.append(f"{kind} opens with {without_console(output).strip()[-60:]!r}, "
                                 f"expected {desktop}")
            if wrong:
                fail("; ".join(wrong))
            ok(f"a file of each of {len(DEFAULT_APPS)} kinds opens with the app that owns it")

            photograph = f"/run/current-system/sw/share/backgrounds/rift/{PICTURE}.jpg"
            run(f"systemd-run --user --quiet --collect loupe {photograph}",
                "the image viewer on a photograph")
            if not wait_for(180, lambda: app_windows("loupe", "the image viewer's window")):
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"loupe {photograph} opened no window: {without_console(output).strip()[-800:]!r}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look("the photograph in the image viewer", f"{stem}-picture{extension}", 120,
                 apps=["loupe"], journals=("horizon", "lens"), settle=3)
            _, _, shown = screendump(args.qmp, work, "picture")
            top_rows, bottom_rows = bar_and_dock(width, height, bar_gray_rows(width, height, shown))
            found, looked = coloured_in(width, shown, top_rows + 8, height - bottom_rows - 8)
            if found < looked / 20:
                fail(f"the image viewer draws no photograph: {found} of {looked} pixels between the bars "
                     f"have a colour, see {stem}-picture{extension}")
            ok(f"the image viewer drew {PICTURE}, {found} of {looked} pixels between the bars in colour")
            close_app("the image viewer", "loupe")
            # the thumbnails and the plugin list the apps wrote are in home, where the backup, the
            # snapshots and the clone after them would carry them
            run("rm -rf ~/.cache/thumbnails ~/.cache/gstreamer-1.0", "what the apps left in the cache")

            # the owner's theme to light: the shell, the desktop behind it, the menus, the lock screen and
            # the apps, which start again so they read it as they would at the start of a session
            theme_to("light")
            point(args.qmp, size, (round(width / 3), round(height / 2)))
            look("the light desktop", f"{stem}-light{extension}", 30, colors=LIGHT_COLORS,
                 journals=("lens", "horizon"))
            run("lens --menu", "the Applications menu in light")
            listed = int(bar_state("the menu in light").get("rows") or 0)
            look("the light Applications menu", f"{stem}-light-menu{extension}", 20, menu=True, rows=listed,
                 colors=LIGHT_COLORS, journals=("lens",))
            run("lens --escape", "escape, which closes the menu")
            if not wait_for(20, lambda: bar_state("the menu closed").get("menu") == "closed"):
                fail("escape did not close the Applications menu in light")
            # the pointer's white arrow would add to the white of the field, so it goes to the top right
            point(args.qmp, size, (width - round(40 * scale), round(height / 8)))
            status, output = run(f"loginctl lock-session {session}", "loginctl lock-session in light")
            if status != 0:
                fail(f"loginctl lock-session {session} exited with {status}: {without_console(output).strip()!r}")
            look("the light lock screen", f"{stem}-light-lock{extension}", 30, lock=False, colors=LIGHT_COLORS,
                 journals=("lock",))
            locked_hint("yes", "with the light lock screen up")
            type_line(PASSWORD, "the owner's password")
            locked_hint("no", "after unlocking the light lock screen")
            ok("the light lock screen locked the session and the owner's password unlocked it")
            titled_apps("firefox and ghostty in light", f"{stem}-light-apps{extension}", LIGHT_COLORS)

            # and dark again, which the rest of the test and the next boots of this drive have
            close_titled_apps()
            theme_to("dark")
            point(args.qmp, size, (round(width / 3), round(height / 2)))
            look("the desktop back in dark", f"{stem}-dark-again{extension}", 30, journals=("lens", "horizon"))

            # 5k. the last of the everyday basics, each one a surface a person sees: the screen
            # recorder on its key, the screen reader, the on-screen keyboard, and the guide in the
            # browser. the vm has no camera, no sound and no printer, but a recording of the desktop
            # needs none of those, so this is the part of P1.19 that runs end to end
            def access_state(key, what):
                """What `lens --state` says about the recorder, the screen reader or the keyboard."""
                return bar_state(what).get(key, "")

            def recording_file():
                """The file the recorder is writing, while it is running."""
                said = access_state("recording", "what the recorder is writing")
                return said if said not in ("", "off") else None

            press(["ctrl", "alt", "shift", "r"], what="the screen recording key")
            recording = wait_for(30, recording_file)
            if not recording:
                # horizon starts what a key runs with its output thrown away, so the same command
                # from the terminal is the only way to see what it had to say
                _, output = run("lens --record", "the recorder from the terminal, to read its error")
                said = without_console(output).strip()[-300:]
                _, output = run("journalctl --user -b -o cat -n 20 | cat", "the user manager's log")
                fail(f"the screen recording key started nothing: {said!r}, "
                     f"{without_console(output).strip()[-500:]!r}")
            # something happens on the screen while it records, and the recording is as long as the
            # time it is taken over, since the recorder asks for every frame and not only the ones
            # that change something
            run("sleep 3", "a moment for the recorder to take its first frames")
            run("lens --menu", "the Applications menu while the screen is recorded")
            run('lens --type "Image"', "words in the field while the screen is recorded")
            run("lens --escape", "escape, which clears the field")
            run("lens --escape", "escape again, which closes the menu")
            look("the bar while the screen is recorded", f"{stem}-recording{extension}", 30,
                 journals=("lens", "horizon"))
            press(["ctrl", "alt", "shift", "r"], what="the screen recording key again")
            if not wait_for(60, lambda: access_state("recording", "the recorder after the second key") == "off"):
                fail(f"the screen recording key did not stop the recorder, still writing {recording}")
            _, output = run(f'stat -c %s "{recording}"', "the size of the recording")
            written = re.search(r"^(\d+)\s*$", without_console(output), re.M)
            if not written or int(written.group(1)) < 1000:
                fail(f"the recording {recording} is {without_console(output).strip()[-100:]!r} bytes")
            status, output = run(f'ffprobe -v error -show_entries format=duration -of csv=p=0 "{recording}" | cat',
                                 "the length of the recording")
            length = re.search(r"(\d+\.\d+)", without_console(output))
            if status != 0 or not length or float(length.group(1)) < 1:
                fail(f"ffprobe reads no length for {recording}: {without_console(output).strip()[-200:]!r}")
            said = bar_state("the notification about the recording").get("latest", "")
            if said != "Screen recording saved":
                fail(f"the newest notification is {said!r}, expected the one that names the recording")
            ok(f"the key recorded the screen for {length.group(1)}s into {int(written.group(1))} bytes, "
               "the bar carried the mark while it ran, and a notification named the file")
            # home goes into the backup, the snapshots and the clone, and the backup disk is 256M
            run(f'rm -f "{recording}"', "the recording, which home does not keep")

            # the screen reader. it draws nothing on the screen: what says it is running is its name
            # on the session bus, and what says it can speak is the voice writing a file
            def orca_on_the_bus(what):
                _, output = run("busctl --user --no-pager list | grep org.gnome.Orca | cat", what)
                return "org.gnome.Orca.Service" in without_console(output)

            press(["meta_l", "alt", "s"], what="the screen reader key")
            if not wait_for(30, lambda: access_state("screen-reader", "the screen reader after the key") == "on"):
                _, output = run("lens --screen-reader", "the screen reader from the terminal")
                said = without_console(output).strip()[-300:]
                _, output = run("journalctl --user -b -o cat -n 20 | cat", "the user manager's log")
                fail(f"the screen reader key started nothing: {said!r}, "
                     f"{without_console(output).strip()[-500:]!r}")
            if not wait_for(180, lambda: orca_on_the_bus("the screen reader on the session bus")):
                # again with its output kept, so a failure to start says why
                run("systemd-run --user --quiet --collect --unit=orca-probe orca --replace",
                    "the screen reader in a unit of its own")
                run("sleep 30", "a moment for it to say what is wrong")
                _, output = run("journalctl --user -u orca-probe -b -o cat -n 40 | cat", "what it said")
                said = without_console(output).strip()[-800:]
                fail(f"the screen reader did not take org.gnome.Orca.Service on the session bus: {said!r}")
            wav = "/tmp/rift-speech.wav"
            _, output = run(f'espeak-ng -w {wav} "The screen reader is on"; and stat -c %s {wav}',
                            "the voice writing a file")
            spoken = re.search(r"^(\d+)\s*$", without_console(output), re.M)
            if not spoken or int(spoken.group(1)) < 1000:
                fail(f"the voice wrote {without_console(output).strip()[-200:]!r}, expected a wav of some size")
            run(f"rm -f {wav}", "the wav the voice wrote")
            press(["meta_l", "alt", "s"], what="the screen reader key again")
            if not wait_for(30, lambda: access_state("screen-reader", "the screen reader after the second key") == "off"):
                fail("the screen reader key did not stop it")
            if not wait_for(60, lambda: not orca_on_the_bus("the screen reader after it was stopped")):
                fail("the screen reader kept its name on the session bus after it was stopped")
            ok(f"the key started and stopped the screen reader, and its voice wrote {int(spoken.group(1))} "
               "bytes of speech with no sound hardware in the machine")

            # the on-screen keyboard. it is a layer surface along the bottom, in the shell's own
            # colours, and a key pressed on it with the pointer types into whatever has the keyboard
            def keyboard_band():
                """Where the on-screen keyboard is drawn, or None when it is not on the screen."""
                wide, tall, shown = screendump(args.qmp, work, "keyboard")
                return keyboard_rows(wide, tall, shown)

            press(["meta_l", "alt", "k"], what="the on-screen keyboard key")
            if not wait_for(30, lambda: access_state("keyboard", "the keyboard after the key") == "on"):
                _, output = run("lens --keyboard", "the on-screen keyboard from the terminal")
                said = without_console(output).strip()[-300:]
                _, output = run("journalctl --user -b -o cat -n 20 | cat", "the user manager's log")
                fail(f"the on-screen keyboard key started nothing: {said!r}, "
                     f"{without_console(output).strip()[-500:]!r}")
            band = wait_for(30, keyboard_band)
            if not band:
                _, output = run("pgrep -a wvkbd | cat", "whether the keyboard is running at all")
                shot(f"{stem}-keyboard{extension}", "keyboard")
                fail("the on-screen keyboard drew nothing along the bottom of the screen: "
                     f"{without_console(output).strip()[-300:]!r}")
            shot(f"{stem}-keyboard{extension}", "keyboard")
            run("lens --menu", "the Applications menu for the keyboard to type into")
            if not wait_for(20, lambda: bar_state("the menu for the keyboard").get("menu") == "open"):
                fail("the Applications menu did not open for the on-screen keyboard")
            # the second row of the keyboard is letters the whole way across, and a third of the way
            # in is one of them. a press on the keyboard leaves the menu its keyboard focus, since
            # the keyboard asked for none of its own.
            # the pointer goes into the keyboard first and moves inside it before the press: wvkbd
            # reads where the pointer is from the motion events alone and takes nothing from the
            # enter, so a press after a jump onto a key lands nowhere
            top, bottom = band
            point(args.qmp, size, (round(width * 0.35), round(top + (bottom - top) * 0.6)))
            time.sleep(0.5)
            click(args.qmp, size, (round(width * 0.35), round(top + (bottom - top) * 0.375)))

            def typed_letter():
                said = bar_state("the field after a key on the on-screen keyboard").get("field", "")
                return said if re.fullmatch(r"[a-z]", said) else None

            typed = wait_for(30, typed_letter)
            if not typed:
                shot(f"{stem}-keyboard-typing{extension}", "keyboard")
                fail("a key pressed on the on-screen keyboard typed nothing into the field: "
                     f"{bar_state('the field').get('field', '')!r}, see {stem}-keyboard-typing{extension}")
            run("lens --escape", "escape, which clears the field")
            run("lens --escape", "escape again, which closes the menu")
            press(["meta_l", "alt", "k"], what="the on-screen keyboard key again")
            if not wait_for(30, lambda: access_state("keyboard", "the keyboard after the second key") == "off"):
                fail("the on-screen keyboard key did not hide it")
            if not wait_for(30, lambda: keyboard_band() is None):
                fail("the on-screen keyboard is still drawn after the key that hides it")
            ok(f"the on-screen keyboard drew itself over rows {top} to {bottom} and typed {typed!r} "
               "into the field of the shell")

            # the guide, which is on the drive and opens in the browser with no network at all
            status, output = run("rift guide list", "the pages of the guide")
            printed = without_console(output)
            listed = [line.split()[0] for line in printed.splitlines() if line.strip()]
            if status != 0 or "the-desktop" not in listed:
                fail(f"rift guide list exited with {status} and printed {printed.strip()[-300:]!r}")
            run("systemd-run --user --quiet --collect rift guide", "the guide in the browser")
            if not wait_for(180, lambda: app_windows("firefox", "the browser with the guide")):
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"rift guide opened no window: {without_console(output).strip()[-800:]!r}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look("the guide in the browser", f"{stem}-guide{extension}", 180, apps=["firefox"],
                 journals=("horizon", "lens"), settle=3)
            ok(f"the guide has {len(listed)} pages on the drive and opens in the browser")
            close_app("the guide", "firefox")

            # 5m. Settings, the app for how the system looks and what it does. The Applications menu
            # opens it, its window stands between the bars with the title bar it draws itself, and
            # the Appearance page changes the accent: the ring horizon draws around the window in
            # front and every mark the shell draws in the accent take the new colour while
            # everything is running. The desktop is still the flat gray here, which is what the
            # window check counts on; the photograph comes back in the step after this one
            def settings_state(what):
                """What `rift-settings --state` prints, as a dict of the words it knows. Empty while
                the window is still opening and nothing answers on its socket yet."""
                status, output = run("rift-settings --state", what)
                if status != 0:
                    return {}
                state = {}
                for printed_line in without_console(output).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    if key in SETTINGS_KEYS:
                        state[key] = value.strip()
                return state

            def accents_on_screen(name):
                """How many pixels of the screen are each of the two accents, within a step or two
                of the colour. One walk of the screendump counts both."""
                wide, tall, rgb = screendump(args.qmp, work, name)
                wanted = [tuple(int(colour[at:at + 2], 16) for at in (1, 3, 5))
                          for _, colour in (SETTINGS_ACCENT, SETTINGS_OTHER)]
                counts = [0, 0]
                for at in range(0, wide * tall * 3, 3):
                    pixel = rgb[at:at + 3]
                    for which, want in enumerate(wanted):
                        if near(pixel, want, 6):
                            counts[which] += 1
                return counts

            open_from_menu(SETTINGS_APP, SETTINGS_APP_ID)
            state = wait_for(60, lambda: settings_state("the page Settings opens on") or None)
            if not state:
                fail("rift-settings --state answers nothing after the Settings window opened")
            if state.get("page") != "appearance" or state.get("accent") != SETTINGS_ACCENT[0]:
                fail(f"Settings opened with {state}, expected the Appearance page and the "
                     f"{SETTINGS_ACCENT[0]} accent")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} with its title bar", f"{stem}-settings{extension}", 120,
                 apps=[SETTINGS_APP], journals=("horizon", "lens"), settle=3)
            ok(f"the Applications menu opened Settings on the {state['page']} page, with the title bar "
               "it draws itself")

            # the Appearance page changes the accent, and the shell and the compositor follow it
            # without either of them starting again
            blue, teal = accents_on_screen("accent-before")
            if blue < 3000 or teal > 2000:
                fail(f"the screen has {blue} pixels of {SETTINGS_ACCENT[0]} and {teal} of "
                     f"{SETTINGS_OTHER[0]} before the accent was changed")
            status, output = run(f"rift-settings --set accent {SETTINGS_OTHER[0]}",
                                 "the accent on the Appearance page")
            if status != 0:
                fail(f"rift-settings --set accent exited with {status}: "
                     f"{without_console(output).strip()[-300:]!r}")
            if not wait_for(30, lambda: bar_state("the shell's accent").get("accent") == SETTINGS_OTHER[0]):
                fail(f"lens --state does not say accent {SETTINGS_OTHER[0]} after the page changed it")
            written = without_console(run("cat ~/.config/rift/accent; cat ~/.local/state/rift/horizon.kdl",
                                          "the accent the page wrote")[1])
            if SETTINGS_OTHER[0] not in written or f'active-color "{SETTINGS_OTHER[1]}"' not in written:
                fail(f"the accent the page wrote says {written.strip()[-300:]!r}")
            # the shell repaints as soon as it is told, and the compositor when it has read its
            # config again, so the colour arrives in two parts and the count is given a moment
            counted = wait_for(60, lambda: next(
                (found for found in [accents_on_screen("accent-after")]
                 if found[1] >= 3000 and found[0] <= 2000), None))
            shot(f"{stem}-settings-accent{extension}", "accent-after")
            if not counted:
                blue, teal = accents_on_screen("accent-after")
                fail(f"the screen has {teal} pixels of {SETTINGS_OTHER[0]} and {blue} of "
                     f"{SETTINGS_ACCENT[0]} after the accent was changed, see "
                     f"{stem}-settings-accent{extension}")
            blue, teal = counted
            ok(f"the Appearance page set the accent to {SETTINGS_OTHER[0]}: the shell says so and "
               f"{teal} pixels of the screen are it, where {blue} are left of {SETTINGS_ACCENT[0]}")

            # and back to the blue the rest of the test and the next boots of this drive have
            run(f"rift-settings --set accent {SETTINGS_ACCENT[0]}", "the accent back to blue")
            if not wait_for(30, lambda: bar_state("the shell's accent").get("accent") == SETTINGS_ACCENT[0]):
                fail(f"lens --state does not say accent {SETTINGS_ACCENT[0]} after it was put back")

            # the interface text size. dconf carries it to the apps, and the shell follows it by
            # asking for every surface at that much of its size and drawing it at the same scale,
            # so the bar and the dock on screen are their own heights times the factor
            def bars(name):
                """The rows the bar and the dock cover on screen, counting the row where the
                hairline falls on half a pixel as the bar, which it is."""
                wide, tall, pixels = screendump(args.qmp, work, name)
                rows = []
                for y in range(tall):
                    row = y * wide * 3
                    found = 0
                    for x in range(wide):
                        px = pixels[row + x * 3:row + x * 3 + 3]
                        if (BAR_GRAYS[0] <= px[0] <= BAR_GRAYS[1]
                                and BAR_GRAYS[0] <= px[1] <= BAR_GRAYS[1]
                                and BAR_GRAYS[0] <= px[2] <= BAR_GRAYS[1]):
                            found += 1
                    rows.append(found)
                return bar_and_dock(wide, tall, rows)

            grown = (round(BAR_HEIGHT * SETTINGS_TEXT[0] / 100),
                     round(DOCK_HEIGHT * SETTINGS_TEXT[0] / 100))
            status, output = run(f"rift-settings --set text {SETTINGS_TEXT[0]}",
                                 "the interface text size on the Appearance page")
            if status != 0:
                fail(f"rift-settings --set text exited with {status}: "
                     f"{without_console(output).strip()[-300:]!r}")
            if not wait_for(30, lambda: bar_state("the shell's text size").get("text") == str(SETTINGS_TEXT[0])):
                fail(f"lens --state does not say text {SETTINGS_TEXT[0]} after the page set it")
            _, output = run("dconf read /org/gnome/desktop/interface/text-scaling-factor",
                            "the text size apps read")
            if SETTINGS_FACTOR not in without_console(output):
                fail(f"dconf reads the text scaling factor as {without_console(output).strip()!r}, "
                     f"expected {SETTINGS_FACTOR}")
            bigger = wait_for(60, lambda: next(
                (found for found in [bars("text-size")] if found == grown), None))
            shot(f"{stem}-settings-text{extension}", "text-size")
            if not bigger:
                fail(f"the bar and the dock are {bars('text-size')} rows at {SETTINGS_TEXT[0]} per cent, "
                     f"expected {grown}, see {stem}-settings-text{extension}")
            ok(f"the Appearance page set the interface text size to {SETTINGS_TEXT[0]} per cent: dconf "
               f"reads {SETTINGS_FACTOR}, the shell says so and its bar and dock are {grown[0]} and "
               f"{grown[1]} rows")

            # and back, so the rest of the test and the next boots see the sizes they know
            run(f"rift-settings --set text {SETTINGS_TEXT[1]}", "the text size back")
            if not wait_for(30, lambda: bar_state("the shell's text size").get("text") == str(SETTINGS_TEXT[1])):
                fail(f"lens --state does not say text {SETTINGS_TEXT[1]} after it was put back")
            if not wait_for(60, lambda: next(
                    (found for found in [bars("text-size-back")]
                     if found == (BAR_HEIGHT, DOCK_HEIGHT)), None)):
                fail(f"the bar and the dock are {bars('text-size-back')} rows again, expected "
                     f"{(BAR_HEIGHT, DOCK_HEIGHT)}")

            # the terminal colour scheme. the page writes a file the image's ghostty config reads
            # after itself, and signals the terminals that are open, so a window that is already
            # up changes colour
            def terminal_pixels(name, colour):
                """How many pixels of the screen are the colour a terminal is drawn on."""
                wide, tall, pixels = screendump(args.qmp, work, name)
                return sum(1 for at in range(0, wide * tall * 3, 3)
                           if near(pixels[at:at + 3], colour, 3))

            open_from_menu(MENU_APP, MENU_APP_ID)
            own = wait_for(60, lambda: terminal_pixels("terminal-own", SETTINGS_OWN_SCHEME[1]) > TERMINAL_PIXELS)
            if not own:
                fail(f"a terminal window is not drawn on {SETTINGS_OWN_SCHEME[1]}: "
                     f"{terminal_pixels('terminal-own', SETTINGS_OWN_SCHEME[1])} pixels of it")
            status, output = run(f"rift-settings --set terminal {SETTINGS_SCHEME[0]}",
                                 "the terminal colours on the Appearance page")
            if status != 0:
                fail(f"rift-settings --set terminal exited with {status}: "
                     f"{without_console(output).strip()[-300:]!r}")
            written = without_console(run("cat ~/.config/rift/terminal; cat ~/.config/rift/terminal.ghostty",
                                          "the colours the page wrote")[1])
            if SETTINGS_SCHEME[0] not in written or "background = #002b36" not in written:
                fail(f"the terminal colours the page wrote say {written.strip()[-300:]!r}")
            changed = wait_for(60, lambda: terminal_pixels("terminal-scheme", SETTINGS_SCHEME[1]) > TERMINAL_PIXELS)
            shot(f"{stem}-settings-terminal{extension}", "terminal-scheme")
            if not changed:
                # the page signals the terminals by the name the kernel keeps for them, so a window
                # that did not change colour is answered by what the terminals are called here
                _, names = run("ps -eo comm | sort -u | grep -i ghost", "what a terminal is called")
                fail(f"the terminal window is not drawn on {SETTINGS_SCHEME[1]} after the page set "
                     f"{SETTINGS_SCHEME[0]}: {terminal_pixels('terminal-scheme', SETTINGS_SCHEME[1])} "
                     f"pixels of it, running terminals are {without_console(names).strip()[-200:]!r}, "
                     f"see {stem}-settings-terminal{extension}")
            ok(f"the Appearance page set the terminal colours to {SETTINGS_SCHEME[0]} and the window "
               f"that was open took them")
            run(f"rift-settings --set terminal {SETTINGS_OWN_SCHEME[0]}", "the terminal colours back")
            if not wait_for(60, lambda: terminal_pixels("terminal-back", SETTINGS_OWN_SCHEME[1]) > TERMINAL_PIXELS):
                fail(f"the terminal window is not drawn on {SETTINGS_OWN_SCHEME[1]} again")
            close_app(MENU_APP, MENU_APP_ID)

            # the Displays page. the size a screen is drawn at is in the host profile, which is
            # root's, so the page asks Orbit over the system bus; the session writes the part of
            # the compositor's config that draws the screens, so the change lands where it stands.
            # the whole screen is drawn twice as big for a moment, so it is put back before the
            # rest of the test looks at anything
            def screen_state(what, scale):
                """Whether the page says the one screen of this machine is drawn at that size."""
                said = settings_state(what).get("screen")
                return said == f"{file_connector} {file_width}x{file_height} scale {scale}"

            def scale_on_the_bus(what):
                """The size Orbit says the screen is drawn at, off the system bus."""
                child.send(f"busctl --system get-property {bus} {obj} {bus} Displays\r")
                expect([r"a\(suuuuu\) \d+ [^\r\n]*\r*\n"], what)
                said = re.search(r'"[\w-]+" \d+ \d+ \d+ \d+ (\d+)', child.match.group(0))
                expect([PROMPT], "the prompt")
                return said.group(1) if said else None

            def scale_in_horizon(what):
                """The size the compositor is drawing that screen at, which it reads from the part
                of its config the session writes."""
                _, printed = run("horizon msg --json outputs", what)
                found = re.search(r'"scale":\s*([\d.]+)', without_console(printed))
                return found.group(1) if found else None

            run("rift-settings --page displays", "the Displays page")
            if not wait_for(30, lambda: settings_state("the Displays page").get("page") == "displays"):
                fail("rift-settings --page displays did not show that page")
            if not wait_for(30, lambda: screen_state("the screen on the Displays page", 1)):
                said = settings_state("the screen on the Displays page").get("screen")
                fail(f"the Displays page says screen {said!r}, expected {file_connector} "
                     f"{file_width}x{file_height} scale 1")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Displays page", f"{stem}-settings-displays{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)

            status, output_said = run(f"rift-settings --set scale {file_connector} 2",
                                      "the size of the screen on the Displays page")
            if status != 0:
                fail(f"rift-settings --set scale exited with {status}: "
                     f"{without_console(output_said).strip()[-300:]!r}")
            if not wait_for(60, lambda: screen_state("the screen's new size", 2)):
                said = settings_state("the screen's new size").get("screen")
                # orbit prints a line for every setting it writes, so its journal says whether the
                # page reached it at all
                _, printed = run("journalctl -b -u orbit --no-pager -n 20 -o cat | cat",
                                 "orbit's journal")
                print(f"\nboot-test: orbit's journal:\n{without_console(printed)}", flush=True)
                fail(f"the Displays page says screen {said!r} after it was set to scale 2")
            # orbit wrote it into the [set] layer of the profile, and says so on the bus
            _, written = run(f"cat {profile}", "the profile after the page set the size")
            written = without_console(written)
            for wanted in ("[[set.display]]", f'connector = "{file_connector}"', "scale = 2"):
                if wanted not in written:
                    fail(f"the profile has no {wanted!r} after the page set the size: "
                         f"{written.strip()[-400:]!r}")
            on_the_bus = scale_on_the_bus("the size on the bus")
            if on_the_bus != "2":
                fail(f"the bus says the screen is drawn at scale {on_the_bus}, the page set 2")
            _, printed = run("rift host", "rift host after the page set the size")
            row = re.search(r"^Display:[ \t]+(.*?)[ \t]*$", without_console(printed), re.M)
            if not row or row.group(1) != f"{file_connector}, {file_width}x{file_height}, scale 2":
                fail(f"rift host says Display {row and row.group(1)!r}, expected scale 2")
            # and the compositor draws it that way, so the bar and the dock cover twice the rows
            if not wait_for(60, lambda: scale_in_horizon("the size the compositor draws") == "2.0"):
                _, part = run("cat ~/.local/state/rift/displays.kdl", "the part written for the screens")
                fail(f"horizon draws the screen at scale {scale_in_horizon('the size the compositor draws')}, "
                     f"expected 2.0 after the page set it; the part says "
                     f"{without_console(part).strip()[-300:]!r}")
            twice = (2 * BAR_HEIGHT, 2 * DOCK_HEIGHT)
            bigger = wait_for(60, lambda: next((found for found in [bars("screen-scale")] if found == twice), None))
            shot(f"{stem}-settings-scale{extension}", "screen-scale")
            if not bigger:
                fail(f"the bar and the dock are {bars('screen-scale')} rows with the screen drawn twice as "
                     f"big, expected {twice}, see {stem}-settings-scale{extension}")
            ok(f"the Displays page set {file_connector} to scale 2: the profile, the bus and rift host "
               f"all say so, and horizon draws the bar and the dock {twice[0]} and {twice[1]} rows tall")

            # and back, so the rest of the test and the next boots see the screen they know
            run(f"rift-settings --set scale {file_connector} 1", "the size of the screen back")
            if not wait_for(60, lambda: screen_state("the screen's size again", 1)):
                fail("the Displays page does not say scale 1 after it was put back")
            if not wait_for(60, lambda: next(
                    (found for found in [bars("screen-scale-back")]
                     if found == (BAR_HEIGHT, DOCK_HEIGHT)), None)):
                fail(f"the bar and the dock are {bars('screen-scale-back')} rows again, expected "
                     f"{(BAR_HEIGHT, DOCK_HEIGHT)}")

            # the Wi-Fi, Network and Bluetooth pages, over the three services the shell's system
            # menu already talks to. this machine has no wireless card and no adapter, so two of
            # them say so in their own words and neither writes anything; the cable is real, and
            # what the Network page says about it is checked against nmcli on this same boot
            def printed_word(printed):
                """The last word a command printed, or nothing when it printed nothing."""
                lines = [line.strip() for line in without_console(printed).splitlines() if line.strip()]
                return lines[-1] if lines else ""

            def radio_word(what):
                """Whether NetworkManager has Wi-Fi switched on, whatever cards there are."""
                _, printed = run("nmcli radio wifi", what)
                return printed_word(printed)

            def nmcli_wired():
                """What nmcli says the cable is called and what address it has, off this boot. The
                two whole groups are asked for rather than named fields, since terse output names
                an address IP4.ADDRESS[1] and a group is one word."""
                _, printed = run("nmcli -t -f GENERAL,IP4 device show", "the devices nmcli knows")
                said = without_console(printed)
                kind, name, found = None, None, {}
                for printed_line in said.splitlines():
                    key, _, value = printed_line.strip().partition(":")
                    if key == "GENERAL.TYPE":
                        kind = value.strip()
                    elif key == "GENERAL.DEVICE":
                        name = value.strip()
                    elif key.startswith("IP4.ADDRESS") and kind == "ethernet":
                        found = {"name": name, "address": value.strip()}
                found["said"] = said.strip()[-400:]
                return found

            run("rift-settings --page wifi", "the Wi-Fi page")
            if not wait_for(30, lambda: settings_state("the Wi-Fi page").get("page") == "wifi"):
                fail("rift-settings --page wifi did not show that page")
            if not wait_for(60, lambda: settings_state("the Wi-Fi page").get("wifi") == "none"):
                said = settings_state("the Wi-Fi page").get("wifi")
                fail(f"the Wi-Fi page says wifi {said!r}, and this machine has no wireless card")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Wi-Fi page", f"{stem}-settings-wifi{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            # with no card there is no switch to press, so there is none from a terminal either
            before = radio_word("whether Wi-Fi is on before the page was told to turn it off")
            run("rift-settings --set wifi off", "the Wi-Fi switch on a machine with no card")
            if radio_word("whether Wi-Fi is on after") != before:
                fail(f"rift-settings --set wifi off changed nmcli radio wifi from {before!r}, and "
                     "this machine has no wireless card to switch")
            ok(f"the Wi-Fi page says this machine has no wireless card and writes nothing, with "
               f"nmcli radio wifi still {before}")

            run("rift-settings --page network", "the Network page")
            if not wait_for(30, lambda: settings_state("the Network page").get("page") == "network"):
                fail("rift-settings --page network did not show that page")
            cable = wait_for(60, lambda: next(
                (found for found in [settings_state("the cable on the Network page")]
                 if found.get("wired") == "connected" and found.get("address", "none") != "none"),
                None))
            if not cable:
                said = settings_state("the cable on the Network page")
                fail(f"the Network page says wired {said.get('wired')!r} with address "
                     f"{said.get('address')!r}, and the vm's cable is up")
            said_by_nmcli = nmcli_wired()
            if said_by_nmcli.get("address") != cable["address"]:
                fail(f"the Network page says the cable has {cable['address']!r} and nmcli says "
                     f"{said_by_nmcli.get('address')!r}, from {said_by_nmcli['said']!r}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Network page", f"{stem}-settings-network{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            ok(f"the Network page says the cable is connected with address {cable['address']}, "
               f"which is what nmcli says about {said_by_nmcli.get('name')}")

            run("rift-settings --page bluetooth", "the Bluetooth page")
            if not wait_for(30, lambda: settings_state("the Bluetooth page").get("page") == "bluetooth"):
                fail("rift-settings --page bluetooth did not show that page")
            if not wait_for(60, lambda: settings_state("the Bluetooth page").get("bluetooth") == "none"):
                said = settings_state("the Bluetooth page").get("bluetooth")
                fail(f"the Bluetooth page says bluetooth {said!r}, and this machine has no adapter")
            run("rift-settings --set bluetooth on", "the Bluetooth switch on a machine with no adapter")
            if not wait_for(20, lambda: settings_state("the page after the switch").get("bluetooth") == "none"):
                fail("the Bluetooth page stopped saying none after the switch was told to turn on")
            # asking a machine with no adapter must not start BlueZ, which would fail every time
            _, output = run("systemctl is-active bluetooth", "whether BlueZ is running")
            # a name of its own: main has no block scope, and `running` is the version of the
            # system the update part installs the next one over
            bluez = printed_word(output)
            if bluez not in ("inactive", "unknown"):
                fail(f"systemctl is-active bluetooth says {bluez!r}, and the page must not start "
                     "BlueZ on a machine with no adapter")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Bluetooth page", f"{stem}-settings-bluetooth{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            ok(f"the Bluetooth page says this machine has no adapter, and BlueZ is {bluez}")

            # the Sound and Power pages, over the two things the shell's system menu has a
            # slider and a battery row for. the vm has one sink, through the emulated card, and no
            # battery: the page names that sink, sets its volume and mutes it, and wpctl says the
            # same on this same boot
            run("rift-settings --page sound", "the Sound page")
            if not wait_for(30, lambda: settings_state("the Sound page").get("page") == "sound"):
                fail("rift-settings --page sound did not show that page")
            sink_name = wait_for(60, lambda: next(
                (found.get("output") for found in [settings_state("the sink on the Sound page")]
                 if found.get("output", "none") not in ("none", "") and found.get("outputs") != "0"),
                None))
            if not sink_name:
                said = settings_state("the sink on the Sound page")
                fail(f"the Sound page says output {said.get('output')!r} of {said.get('outputs')!r}, "
                     "and the vm has one sink")
            _, output = run("wpctl inspect @DEFAULT_AUDIO_SINK@ | cat", "what PipeWire calls the sink")
            inspected = without_console(output)
            if sink_name not in inspected:
                fail(f"the Sound page calls the sink {sink_name!r}, and wpctl inspect says "
                     f"{inspected.strip()[-400:]!r}")

            sound_before = volume_now()
            if sound_before is None:
                fail("wpctl reads no volume for the default sink while the Sound page is up")
            sound_wanted = 25 if sound_before >= 0.5 else 75
            status, output = run(f"rift-settings --set volume {sound_wanted}",
                                 "the volume on the Sound page")
            if status != 0:
                fail(f"rift-settings --set volume exited with {status}: "
                     f"{without_console(output).strip()[-300:]!r}")
            if not wait_for(30, lambda: next(
                    (True for level in [volume_now()]
                     if level is not None and abs(level - sound_wanted / 100) <= 0.01), None)):
                fail(f"the Sound page set the volume to {sound_wanted} and wpctl says "
                     f"{volume_now()}, it was {sound_before}")
            if not wait_for(30, lambda: settings_state("the level the page says").get("volume")
                            == str(sound_wanted)):
                fail(f"the Sound page says volume "
                     f"{settings_state('the level again').get('volume')!r} after it was set to "
                     f"{sound_wanted}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Sound page", f"{stem}-settings-sound{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)

            run("rift-settings --set mute on", "the mute switch on the Sound page")
            if not wait_for(30, lambda: settings_state("the mute switch").get("mute") == "on"):
                fail(f"the Sound page says mute {settings_state('the switch again').get('mute')!r} "
                     "after it was turned on")
            _, output = run("wpctl get-volume @DEFAULT_AUDIO_SINK@", "whether the sink is muted")
            if "MUTED" not in without_console(output):
                fail(f"the Sound page muted the sink and wpctl says "
                     f"{without_console(output).strip()!r}")
            run("rift-settings --set mute off", "the mute switch off again")
            if not wait_for(30, lambda: settings_state("the mute switch off").get("mute") == "off"):
                fail("the Sound page still says mute on after the switch was turned off")
            # and the level back where the rest of the test left it
            run(f"rift-settings --set volume {round(sound_before * 100)}", "the volume back")
            ok(f"the Sound page names the sink {sink_name}, set it to {sound_wanted} per cent and "
               f"muted it, and wpctl says so on the same boot")

            run("rift-settings --page power", "the Power page")
            if not wait_for(30, lambda: settings_state("the Power page").get("page") == "power"):
                fail("rift-settings --page power did not show that page")
            if not wait_for(60, lambda: settings_state("the Power page").get("battery") == "none"):
                said = settings_state("the Power page").get("battery")
                # the page says nothing about a battery until UPower has answered, and UPower is
                # started by the first call that asks it something
                _, output = run("systemctl is-active upower", "whether UPower is running")
                fail(f"the Power page says battery {said!r} and this machine has no battery, with "
                     f"upower {printed_word(output)}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Power page", f"{stem}-settings-power{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            ok("the Power page says this machine has no battery, which is what UPower answers")

            # the AI and Search pages, the two over Quasar. step 4 read the model and the tier off
            # the bus and step 4c the embedding model and the index of home; each page says the same
            # about the same boot, and the size of model this machine runs is a setting the page
            # writes into the host profile through Orbit
            if args.models:

                def orbit_tier(what):
                    """The size of model the host profile says, off Orbit's own interface."""
                    _, told = run(f"busctl --system get-property {bus} {obj} {bus} AiTier", what)
                    found = re.search(r's "(\w+)"', without_console(told))
                    return found.group(1) if found else without_console(told).strip()[-100:]

                def index_written(what):
                    """When the search index was last written, in seconds since 1970."""
                    _, told = run("stat -c written=%Y ~/.cache/rift/search.index", what)
                    found = re.search(r"written=(\d+)", without_console(told))
                    return found.group(1) if found else None

                run("rift-settings --page ai", "the AI page")
                if not wait_for(30, lambda: settings_state("the AI page").get("page") == "ai"):
                    fail("rift-settings --page ai did not show that page")
                ai_page = wait_for(60, lambda: next(
                    (found for found in [settings_state("the model on the AI page")]
                     if found.get("ai") == "ready" and found.get("tier")), None))
                if not ai_page:
                    said = settings_state("the model on the AI page")
                    fail(f"the AI page says ai {said.get('ai')!r} with model {said.get('model')!r} for "
                         f"tier {said.get('tier')!r}, and the bus says quasar is ready with "
                         f"{quasar_model} for {quasar_tier}")
                if ai_page.get("model") != quasar_model or ai_page.get("tier") != quasar_tier:
                    fail(f"the AI page says model {ai_page.get('model')!r} for tier "
                         f"{ai_page.get('tier')!r}, the bus says {quasar_model} for {quasar_tier}")
                # the only model on the drive is the one the test put in @models, and the page counts it
                if ai_page.get("models") != "1":
                    fail(f"the AI page says {ai_page.get('models')!r} models are on the drive, and the "
                         f"test put one there")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                look(f"{SETTINGS_APP} on the AI page", f"{stem}-settings-ai{extension}", 60,
                     apps=[SETTINGS_APP], journals=("horizon",), settle=3)
                ok(f"the AI page says quasar runs {quasar_model} for the {quasar_tier} tier, which is "
                   f"what the bus says on this boot, with one model on the drive")

                # the size is a setting: the page writes it through Orbit, which puts it in the host
                # profile and says so on the bus. quasar picked its model when it started and keeps
                # it, which is what the page says under the rows
                other_tier = "medium" if quasar_tier != "medium" else "large"
                run(f"rift-settings --set tier {other_tier}", f"the model size set to {other_tier}")
                if not wait_for(60, lambda: settings_state("the size on the AI page").get("tier") == other_tier):
                    said = settings_state("the size again").get("tier")
                    _, printed = run("journalctl -b -u orbit --no-pager -n 20 -o cat | cat", "orbit's journal")
                    print(f"\nboot-test: orbit's journal:\n{without_console(printed)}", flush=True)
                    fail(f"the AI page says tier {said!r} after it was set to {other_tier}")
                tier_on_the_bus = orbit_tier("the size of model on the bus")
                if tier_on_the_bus != other_tier:
                    fail(f"Orbit says the size of model is {tier_on_the_bus!r}, the page set {other_tier}")
                if quasar_prop("Model") != quasar_model:
                    fail(f"quasar runs {quasar_prop('Model')} after the size was set to {other_tier}, and "
                         f"it picks a model when it starts, so it keeps {quasar_model}")
                run(f"rift-settings --set tier {ai_tier}", "the model size back")
                if not wait_for(60, lambda: settings_state("the size put back").get("tier") == ai_tier):
                    fail(f"the AI page says tier {settings_state('the size once more').get('tier')!r} "
                         f"after it was put back to {ai_tier}")
                if orbit_tier("the size of model on the bus again") != ai_tier:
                    fail(f"Orbit says the size of model is {orbit_tier('the size once more')!r} after it "
                         f"was put back to {ai_tier}")
                ok(f"the AI page set the model size to {other_tier} in the host profile and back to "
                   f"{ai_tier}, and quasar keeps {quasar_model} until it starts again")

                run("rift-settings --page search", "the Search page")
                if not wait_for(30, lambda: settings_state("the Search page").get("page") == "search"):
                    fail("rift-settings --page search did not show that page")
                search_page = wait_for(60, lambda: next(
                    (found for found in [settings_state("the index on the Search page")]
                     if found.get("search") == "ready" and found.get("indexed", "none") != "none"),
                    None))
                if not search_page:
                    said = settings_state("the index on the Search page")
                    fail(f"the Search page says search {said.get('search')!r} with {said.get('indexed')!r} "
                         f"files indexed, and step 4c indexed {len(documents)} files with {embedding}")
                if search_page.get("search-model") != embedding:
                    fail(f"the Search page says the model is {search_page.get('search-model')!r}, the bus "
                         f"says {embedding}")
                if int(search_page.get("indexed")) < len(documents):
                    fail(f"the Search page says {search_page.get('indexed')} files are indexed, and step "
                         f"4c wrote {len(documents)} of them into home")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                look(f"{SETTINGS_APP} on the Search page", f"{stem}-settings-search{extension}", 60,
                     apps=[SETTINGS_APP], journals=("horizon",), settle=3)

                # the button runs the command the timer runs, which writes the index file again
                index_before = index_written("when the index was last written")
                if not index_before:
                    fail("stat says nothing about ~/.cache/rift/search.index, and step 4c wrote it")
                run("rift-settings --set index now", "the index brought up to date from the page")
                if not wait_for(240, lambda: index_written("when the index was written") != index_before):
                    _, printed = run("journalctl --user -b -o cat -n 20 | cat", "the user manager's log")
                    fail(f"the index was last written at {index_before} before the page was pressed and "
                         f"{index_written('when the index was written')} after: "
                         f"{without_console(printed).strip()[-400:]!r}")
                indexed_again = wait_for(120, lambda: next(
                    (found for found in [settings_state("the index after the update")]
                     if found.get("indexing") == "off" and found.get("index") == "just now"), None))
                if not indexed_again:
                    said = settings_state("the index after the update")
                    fail(f"the Search page says index {said.get('index')!r} with indexing "
                         f"{said.get('indexing')!r} after it brought the index up to date")
                if indexed_again.get("indexed") != search_page.get("indexed"):
                    fail(f"the Search page says {indexed_again.get('indexed')!r} files are indexed after "
                         f"the update, and it said {search_page.get('indexed')!r} before it")
                ok(f"the Search page says {embedding} reads {search_page.get('indexed')} files of home, "
                   f"and the button wrote the index again on this boot")

            # the Backups page, over Vault. this runs before the timeline of step 6, so the
            # snapshots it counts are the ones it takes itself, and the folder backups go to is
            # chosen in step 6b, after this, so the page says there is none yet
            run("rift-settings --page backups", "the Backups page")
            if not wait_for(30, lambda: settings_state("the Backups page").get("page") == "backups"):
                fail("rift-settings --page backups did not show that page")
            backups_page = wait_for(120, lambda: next(
                (found for found in [settings_state("the snapshots on the Backups page")]
                 if found.get("snapshots") and found.get("backup-folder")), None))
            if not backups_page:
                said = settings_state("the Backups page once more")
                _, printed = run("journalctl -b -u vault --no-pager -n 20 -o cat | cat", "vault's journal")
                fail(f"the Backups page says snapshots {said.get('snapshots')!r} and backup-folder "
                     f"{said.get('backup-folder')!r}: {without_console(printed).strip()[-400:]!r}")
            if backups_page.get("backup-folder") != "none" or backups_page.get("backups") != "none":
                fail(f"the Backups page says backups go to {backups_page.get('backup-folder')!r} with "
                     f"{backups_page.get('backups')!r} of them, and no folder has been chosen yet")
            snapshots_before = int(backups_page.get("snapshots"))
            run("rift-settings --set snapshot now", "a snapshot taken from the Backups page")
            page_took = wait_for(180, lambda: next(
                (found for found in [settings_state("the snapshot the page took")]
                 if found.get("taking") == "off"
                 and int(found.get("snapshots", -1)) > snapshots_before), None))
            if not page_took:
                said = settings_state("the snapshots after the page took one")
                fail(f"the Backups page says snapshots {said.get('snapshots')!r} with taking "
                     f"{said.get('taking')!r} after it took one, and it said {snapshots_before} before")
            if page_took.get("snapshot") != "just now":
                fail(f"the Backups page says the newest snapshot was taken {page_took.get('snapshot')!r} "
                     f"after it took one")
            status, output = run("rift snapshot", "the snapshots vault lists")
            in_vault = re.findall(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ)\s*$", without_console(output), re.M)
            if status != 0 or len(in_vault) < int(page_took.get("snapshots")):
                fail(f"rift snapshot lists {len(in_vault)} snapshots and the page says "
                     f"{page_took.get('snapshots')!r}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Backups page", f"{stem}-settings-backups{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon", "vault"), settle=3)
            ok(f"the Backups page took a snapshot, {snapshots_before} to {page_took.get('snapshots')} of "
               "them, which is what rift snapshot lists, and no backup disk is chosen yet")

            # the Date and time page, over timedated. no zone was ever chosen on this drive, so it is
            # in UTC, and the page says what timedatectl says on the same boot. whether a time server
            # answers the vm's user network is not the test's to decide, so that part is compared with
            # timedated rather than assumed. choosing a zone on the page writes the link timedated
            # keeps on persist, and the clock of the whole desktop follows it, the bar's too; then it
            # goes back to UTC for the rest of the test
            def timedated_says(what):
                """What timedatectl show prints, as a dict of its properties."""
                _, told = run("timedatectl show | cat", what)
                said = {}
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition("=")
                    if key and value:
                        said[key] = value
                return said

            def clock_agrees(page, said):
                """Whether the page says what timedated says about the zone, the time server and the
                hardware clock."""
                ntp = "none" if said.get("CanNTP") != "yes" else "on" if said.get("NTP") == "yes" else "off"
                synchronized = "yes" if ntp == "on" and said.get("NTPSynchronized") == "yes" else "no"
                return (page.get("timezone") == said.get("Timezone") and page.get("ntp") == ntp
                        and page.get("synchronized") == synchronized
                        and page.get("rtc") == ("local" if said.get("LocalRTC") == "yes" else "utc"))

            def clock_in_vm(what):
                """The minute and the day date in the vm is in, the way the page writes them."""
                _, told = run("date '+%H:%M %F'", what)
                found = re.search(r"(\d\d:\d\d) (\d{4}-\d\d-\d\d)", without_console(told))
                return found.groups() if found else None

            def page_clock_now(what):
                """Whether the time and the day the page shows are the ones date says, before or after
                the page is asked, since the minute can turn between the readings."""
                clock_before = clock_in_vm(what)
                shown = settings_state(what)
                clock_after = clock_in_vm(what)
                return (shown.get("time"), shown.get("date")) in (clock_before, clock_after)

            def bar_follows(what):
                """Whether the bar's clock says the minute date says, in whatever zone is set now."""
                bar_before = vm_clock(what)
                shown = bar_state(what).get("clock")
                bar_after = vm_clock(what)
                return shown in (bar_before, bar_after)

            run("rift-settings --page datetime", "the Date and time page")
            if not wait_for(30, lambda: settings_state("the Date and time page").get("page") == "datetime"):
                fail("rift-settings --page datetime did not show that page")
            # the page reads timedated again as every minute turns, and a time server can answer in
            # between, so the two are given a minute to agree
            clock_page = wait_for(70, lambda: next(
                (found for found in [settings_state("the clock on the Date and time page")]
                 if clock_agrees(found, timedated_says("what timedated says"))), None))
            if not clock_page:
                said = settings_state("the clock on the Date and time page again")
                fail(f"the Date and time page says timezone {said.get('timezone')!r}, ntp {said.get('ntp')!r}, "
                     f"synchronized {said.get('synchronized')!r} and rtc {said.get('rtc')!r}, and timedatectl "
                     f"says {timedated_says('what timedated says again')}")
            if clock_page.get("timezone") != "UTC":
                fail(f"the Date and time page says timezone {clock_page.get('timezone')!r} on a drive where no "
                     "zone was ever chosen")
            if not wait_for(20, lambda: page_clock_now("the time on the Date and time page")):
                said = settings_state("the time on the page again")
                fail(f"the Date and time page says {said.get('time')!r} on {said.get('date')!r}, and date in "
                     f"the vm says {clock_in_vm('the time in the vm')}")
            ok(f"the Date and time page says the zone is UTC with ntp {clock_page.get('ntp')} and "
               f"synchronized {clock_page.get('synchronized')}, which is what timedatectl says, and the time "
               "date says")

            chosen_zone, zone_names = SETTINGS_ZONE
            status, output = run(f"rift-settings --set timezone {chosen_zone}", "the time zone on the Date and time page")
            if status != 0:
                fail(f"rift-settings --set timezone exited with {status}: {without_console(output).strip()[-300:]!r}")
            if not wait_for(60, lambda: settings_state("the zone the page set").get("timezone") == chosen_zone):
                said = settings_state("the zone the page set again")
                _, printed = run("journalctl -b -u systemd-timedated --no-pager -n 20 -o cat | cat",
                                 "timedated's journal")
                fail(f"the Date and time page says timezone {said.get('timezone')!r} after it was set to "
                     f"{chosen_zone}: {without_console(printed).strip()[-400:]!r}")
            # timedated wrote its link onto persist, /etc/localtime points at that link, and date reads
            # the zone through the two of them
            _, output = run(f"timedatectl show -p Timezone --value; readlink {ZONE_LINK} /etc/localtime; "
                            f"date +%Z; findmnt -n -o SOURCE -T {os.path.dirname(ZONE_LINK)}",
                            "where the zone is kept")
            zone_said = without_console(output).split()
            zone_wanted = [chosen_zone, f"/etc/zoneinfo/{chosen_zone}", ZONE_LINK]
            # findmnt is asked about the folder, since it follows a link: the folder is on persist, in
            # the subvolume /var is. timedated writes nothing else outside /etc
            on_persist = any("persist" in word and "@var" in word for word in zone_said)
            if (any(word not in zone_said for word in zone_wanted) or not on_persist
                    or not any(name in zone_said for name in zone_names)):
                fail(f"after the page set {chosen_zone}, timedatectl, the two links, date and findmnt say "
                     f"{zone_said!r}")
            if not wait_for(20, lambda: page_clock_now("the time on the page in the new zone")):
                said = settings_state("the time on the page in the new zone again")
                fail(f"the Date and time page says {said.get('time')!r} in {chosen_zone}, and date says "
                     f"{clock_in_vm('the time in the new zone')}")
            # the bar asks date on the minute, so it follows within one
            if not wait_for(80, lambda: bar_follows("the bar's clock in the new zone")):
                fail(f"the bar's clock says {bar_state('the bar in the new zone').get('clock')!r}, and date "
                     f"says {vm_clock('the time in the new zone')!r} in {chosen_zone}")
            # the picture is taken while timedated is up: stopping it cancels a start the page's
            # minute tick may just have asked for, and the page then says timedated is not answering
            # until its next tick
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Date and time page", f"{stem}-settings-datetime{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            # and a timedated started afresh reads the zone off the drive, not out of its memory
            run("sudo systemctl stop systemd-timedated", "timedated stopped")
            _, output = run("timedatectl show -p Timezone --value", "the zone a new timedated reads")
            if chosen_zone not in without_console(output).split():
                fail(f"a timedated started again says {without_console(output).strip()!r}, and the page set "
                     f"{chosen_zone}")
            ok(f"the Date and time page set the zone to {chosen_zone}: timedated keeps it in {ZONE_LINK} on "
               f"persist, date calls it {next((name for name in zone_said if name in zone_names), zone_said)}, "
               "and the page and the bar say the time in it")

            status, output = run("rift-settings --set timezone UTC", "the time zone back to UTC")
            if not wait_for(60, lambda: settings_state("the zone put back").get("timezone") == "UTC"):
                fail(f"the Date and time page says timezone "
                     f"{settings_state('the zone put back again').get('timezone')!r} after it was set to UTC")
            _, output = run(f"timedatectl show -p Timezone --value; readlink {ZONE_LINK}; date +%Z",
                            "the zone put back")
            zone_said = without_console(output).split()
            if zone_said.count("UTC") < 2 or "/etc/zoneinfo/UTC" not in zone_said:
                fail(f"after the page set UTC, timedatectl, the link and date say {zone_said!r}")
            # the bar reads the clock again when timedated says the zone changed, rather than when the
            # minute turns, so with the minute some way off it already says the time in UTC
            _, output = run("date +%S", "the second the zone went back in")
            zone_second = re.search(r"^\s*(\d+)\s*$", without_console(output), re.M)
            if (zone_second and int(zone_second.group(1)) < 45
                    and not wait_for(10, lambda: bar_follows("the bar's clock back in UTC"))):
                fail(f"the bar's clock says {bar_state('the bar back in UTC').get('clock')!r} after the zone went "
                     f"back to UTC, and date says {vm_clock('the time back in UTC')!r}: the bar waited for the minute")
            ok("the Date and time page put the zone back to UTC, timedated's link says so, and the bar followed")

            # the Region and language page, over localed. the image is in British English, with the
            # keymap and the layout every keyboard starts in, and the page says what localed says on
            # the same boot, in the words the C library has for the locale
            def localed_says(what, name):
                """One property of localed, as the quoted strings busctl prints for it."""
                _, told = run(f"busctl --system get-property org.freedesktop.locale1 /org/freedesktop/locale1 "
                              f"org.freedesktop.locale1 {name} | cat", what)
                return re.findall(r'"([^"]*)"', without_console(told))

            run("rift-settings --page region", "the Region and language page")
            if not wait_for(30, lambda: settings_state("the Region and language page").get("page") == "region"):
                fail("rift-settings --page region did not show that page")
            region_page = wait_for(60, lambda: next(
                (found for found in [settings_state("the language on the Region and language page")]
                 if found.get("locale")), None))
            if not region_page:
                fail("the Region and language page says nothing about the locale, and localed is there to ask")
            region_locale = localed_says("the locale localed has", "Locale")
            region_keymap = localed_says("the console's keymap", "VConsoleKeymap")
            region_layout = localed_says("the desktop's layout", "X11Layout")
            said_by_localed = {
                "locale": " ".join(region_locale) or "none",
                "keymap": "".join(region_keymap) or "none",
                "layout": "".join(region_layout) or "none",
            }
            for localed_key, localed_value in said_by_localed.items():
                if region_page.get(localed_key) != localed_value:
                    fail(f"the Region and language page says {localed_key} {region_page.get(localed_key)!r}, "
                         f"and localed says {localed_value!r}")
            if region_page.get("locale") != f"LANG={SETTINGS_LOCALE}":
                fail(f"the system's locale is {region_page.get('locale')!r}, and the image is in {SETTINGS_LOCALE}")
            # the language in words comes from the C library's own data about the locale
            _, output = run(f"env LC_ALL={SETTINGS_LOCALE} locale -k lang_name country_name",
                            "what the C library calls the locale")
            locale_names = re.findall(r'_name="([^"]*)"', without_console(output))
            if len(locale_names) != 2 or region_page.get("language") != f"{locale_names[0]} ({locale_names[1]})":
                fail(f"the Region and language page says language {region_page.get('language')!r}, and the C "
                     f"library says {without_console(output).strip()[-200:]!r}")
            if region_page.get("paper") != "A4" or region_page.get("formats") != SETTINGS_LOCALE:
                fail(f"the Region and language page says formats {region_page.get('formats')!r} on "
                     f"{region_page.get('paper')!r} paper, and {SETTINGS_LOCALE} is A4")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Region and language page", f"{stem}-settings-region{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            ok(f"the Region and language page says {region_page.get('language')} with "
               f"{region_page.get('locale')}, keymap {region_page.get('keymap')} and layout "
               f"{region_page.get('layout')}, which is what localed says on the same boot")

            # the Printers page, over CUPS. the vm has no printer, so CUPS's own test printer stands
            # in for one: ippeveprinter answers IPP Everywhere on a port of localhost, and the queue
            # the owner makes for it is the one cups-browsed makes for a driverless printer it hears
            # on the network. at every step the page says what lpstat says on the same boot: the
            # queue, the default the page sets in the owner's lpoptions, a job held in the queue, the
            # queue stopped with a message and resumed from the page, and the job cancelled from it
            queue_name, queue_title = TEST_PRINTER

            def printers_page(what):
                """What the page says about the printers: the counts and the default, and every
                printer and every job line in order."""
                status, told = run("rift-settings --state", what)
                said = {"printer": [], "job": []}
                if status != 0:
                    return said
                for printed_line in without_console(told).splitlines():
                    printed_key, _, printed_value = printed_line.strip().partition(" ")
                    if printed_key in ("printer", "job"):
                        said[printed_key].append(printed_value.strip())
                    elif printed_key in ("printers", "default-printer", "jobs"):
                        said[printed_key] = printed_value.strip()
                return said

            def lpstat_says(what):
                """What lpstat says on the same boot, in the page's words: a line per printer with what
                it is doing and the message it has, the default, and the numbers of the test queue's
                jobs. lpstat writes a printer's message on the line after it, indented."""
                _, told = run("lpstat -p -d -o | cat", what)
                printed_lines = without_console(told).splitlines()
                lpstat_printers = []
                for at, printed_line in enumerate(printed_lines):
                    found = re.match(r"printer (\S+) (is idle|now printing|disabled)", printed_line.strip())
                    if not found:
                        continue
                    doing = {"is idle": "idle", "now printing": "printing", "disabled": "stopped"}[found.group(2)]
                    after = printed_lines[at + 1] if at + 1 < len(printed_lines) else ""
                    message = after.strip() if re.match(r"\s+\S", after) else ""
                    lpstat_printers.append(f"{found.group(1)} {doing} {message}".strip())
                default = re.search(r"system default destination: (\S+)", "\n".join(printed_lines))
                return {"printers": lpstat_printers, "default": default.group(1) if default else "none",
                        "jobs": re.findall(rf"^{re.escape(queue_name)}-(\d+)\s", "\n".join(printed_lines), re.M)}

            def printers_agree(page, told):
                """Whether the page and lpstat name the same printers doing the same, the same default
                and the same jobs of the test queue."""
                page_jobs = [job_line.split()[0] for job_line in page["job"] if job_line.split()[1:2] == [queue_name]]
                return (page["printer"] == told["printers"] and page.get("default-printer") == told["default"]
                        and page_jobs == told["jobs"])

            def printers_when(what, wanted):
                """The page's printers once they are what wanted asks and lpstat agrees, or None."""
                return wait_for(30, lambda: next(
                    (found for found in [printers_page(what)]
                     if wanted(found) and printers_agree(found, lpstat_says(f"lpstat for {what}"))), None))

            def printers_differ(what):
                """Say what the page and lpstat said, when they did not say what they should have."""
                fail(f"{what}: the Printers page says {printers_page('the printers on the page again')}, and "
                     f"lpstat says {lpstat_says('what lpstat says again')}")

            run(f"systemd-run --user --quiet --collect --unit=rift-test-printer ippeveprinter "
                f"-p {TEST_PRINTER_PORT} -r off -n localhost '{queue_title}'", "CUPS's test printer")
            # lpadmin asks the printer what it takes before it makes the queue, so it is tried until the
            # printer answers. the owner is in wheel, which cupsd.conf's SystemGroup names, so the owner
            # makes the queue over CUPS's own socket with no password
            queue_said = []

            def queue_made():
                status, told = run(f"lpadmin -p {queue_name} -D '{queue_title}' -E "
                                   f"-v ipp://localhost:{TEST_PRINTER_PORT}/ipp/print -m everywhere",
                                   "a queue for the test printer")
                queue_said[:] = [without_console(told).strip()[-300:]]
                return status == 0

            if not wait_for(60, queue_made):
                _, journal = run("journalctl --user -b -u rift-test-printer -o cat -n 20 | cat",
                                 "the test printer's log")
                fail(f"lpadmin made no queue for the test printer: {queue_said!r}, "
                     f"{without_console(journal).strip()[-400:]!r}")
            run("rift-settings --page printers", "the Printers page")
            if not wait_for(30, lambda: settings_state("the Printers page").get("page") == "printers"):
                fail("rift-settings --page printers did not show that page")
            printers_found = printers_when("the printers on the page", lambda found: found.get("printers") == "1")
            if not printers_found:
                printers_differ("the queue the owner made")
            if not printers_found["printer"][0].startswith(f"{queue_name} idle") or \
                    printers_found.get("default-printer") != "none":
                fail(f"the Printers page says {printers_found} for a new idle queue on a drive with no default")
            ok(f"the Printers page lists {queue_name} idle with no default printer, which is what lpstat says")

            # the default is the owner's own: the page writes it into ~/.cups/lpoptions, which the CUPS
            # library reads before it asks the scheduler, and lpstat -d then names it
            run(f"rift-settings --set printer {queue_name}", "the printer made the default on the page")
            if not printers_when("the default on the page", lambda found: found.get("default-printer") == queue_name):
                printers_differ(f"the page made {queue_name} the default")
            _, told = run("cat ~/.cups/lpoptions", "the owner's lpoptions")
            if f"Default {queue_name}" not in without_console(told):
                fail(f"the owner's lpoptions says {without_console(told).strip()[-200:]!r} after the page "
                     f"made {queue_name} the default")
            # a job held in the queue waits there until it is released or cancelled
            status, told = run(f"lp -d {queue_name} -H hold -o raw -t '{TEST_JOB}' /etc/os-release",
                               "a job held in the test queue")
            held_job = re.search(rf"request id is {re.escape(queue_name)}-(\d+)", without_console(told))
            if status != 0 or not held_job:
                fail(f"lp exited with {status} and printed {without_console(told).strip()[-300:]!r}")
            job_number = held_job.group(1)
            if not printers_when("the held job on the page",
                                 lambda found: found["job"] == [f"{job_number} {queue_name} held {TEST_JOB}"]):
                printers_differ(f"job {job_number} held in {queue_name}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Printers page", f"{stem}-settings-printers{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            ok(f"the Printers page made {queue_name} the default in the owner's lpoptions and lists job "
               f"{job_number} held in it, which is what lpstat says")

            # stopped, with the message CUPS keeps for it, then resumed from the page. cupsenable is
            # CUPS's administrator's to run, and the owner is one through wheel
            status, told = run(f"cupsdisable -r '{TEST_STOPPED}' {queue_name}", "the test queue stopped")
            if status != 0:
                fail(f"cupsdisable exited with {status}: {without_console(told).strip()[-300:]!r}")
            if not printers_when("the stopped queue on the page",
                                 lambda found: found["printer"] == [f"{queue_name} stopped {TEST_STOPPED}"]):
                printers_differ(f"{queue_name} stopped")
            run(f"rift-settings --set resume {queue_name}", "Resume on the Printers page")
            if not printers_when("the queue resumed on the page",
                                 lambda found: found["printer"][:1] == [f"{queue_name} idle"]):
                _, told = run(f"cupsenable {queue_name}", "cupsenable from the terminal, to read what it says")
                printers_differ(f"Resume on the page, where cupsenable in a terminal says "
                                f"{without_console(told).strip()[-200:]!r}")
            run(f"rift-settings --set cancel {job_number}", "Cancel on the Printers page")
            if not printers_when("the jobs after the cancel", lambda found: found.get("jobs") == "0"):
                _, told = run(f"cancel {job_number}", "cancel from the terminal, to read what it says")
                printers_differ(f"Cancel on job {job_number}, where cancel in a terminal says "
                                f"{without_console(told).strip()[-200:]!r}")
            ok(f"the Printers page showed {queue_name} stopped with its message, resumed it, and cancelled job "
               f"{job_number}, and lpstat agreed each time")

            # the test printer goes, and so does the default it had, since home goes into the backups
            run(f"lpadmin -x {queue_name}; systemctl --user stop rift-test-printer; rm -f ~/.cups/lpoptions",
                "the test printer taken away")
            if not printers_when("the printers after the queue went",
                                 lambda found: found.get("printers") == "0" and found.get("default-printer") == "none"):
                printers_differ("the test queue taken away")

            # the Accessibility page. each switch runs what its key runs and says whether its program is
            # running now, which is what `lens --state` and pgrep say on the same boot: on and off from
            # the page, and a key pressed while the page is up moves the switch with it
            def access_page(what):
                """What the page says about the screen reader and the on-screen keyboard."""
                found = settings_state(what)
                return found.get("screen-reader"), found.get("on-screen-keyboard")

            def running_now(pattern, what):
                """The processes whose command line has the pattern in it."""
                _, told = run(f"pgrep -af {pattern} | cat", what)
                return [found for found in without_console(told).splitlines() if found.strip()]

            def access_agrees(reader, keyboard, what):
                """Whether the page, the shell and pgrep all say the screen reader and the keyboard
                are what is wanted."""
                shell = bar_state(f"the shell for {what}")
                return (access_page(f"the page for {what}") == (reader, keyboard)
                        and (shell.get("screen-reader"), shell.get("keyboard")) == (reader, keyboard)
                        and bool(running_now("orca", f"orca for {what}")) == (reader == "on")
                        and bool(running_now("wvkbd", f"wvkbd for {what}")) == (keyboard == "on"))

            def access_when(reader, keyboard, what):
                """Wait for the page, the shell and pgrep to agree, or say what each said."""
                if wait_for(60, lambda: access_agrees(reader, keyboard, what)):
                    return
                shell = bar_state(f"the shell for {what} again")
                # the notes in the runtime directory are where the page and the shell read it from,
                # so they say whether the program went or the note did
                _, notes = run("cat $XDG_RUNTIME_DIR/lens-reader $XDG_RUNTIME_DIR/lens-keyboard 2>/dev/null; "
                               "or true", "the notes lens leaves")
                fail(f"{what}: the Accessibility page says {access_page(f'the page for {what} again')}, lens "
                     f"--state says {(shell.get('screen-reader'), shell.get('keyboard'))}, pgrep finds "
                     f"{running_now('orca', 'orca again')} and {running_now('wvkbd', 'wvkbd again')}, and the "
                     f"notes say {without_console(notes).strip()[-200:]!r}, where "
                     f"the screen reader should be {reader} and the keyboard {keyboard}")

            run("rift-settings --page accessibility", "the Accessibility page")
            if not wait_for(30, lambda: settings_state("the Accessibility page").get("page") == "accessibility"):
                fail("rift-settings --page accessibility did not show that page")
            access_when("off", "off", "the page as it comes up")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Accessibility page", f"{stem}-settings-accessibility{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            run("rift-settings --set screen-reader on", "the screen reader's switch on")
            access_when("on", "off", "the screen reader turned on from the page")
            # the screen reader the page started is the whole of it, and takes its name on the bus
            if not wait_for(180, lambda: orca_on_the_bus("the screen reader the page started")):
                fail("the screen reader the page started did not take org.gnome.Orca.Service on the session bus")
            run("rift-settings --set screen-reader off", "the screen reader's switch off")
            access_when("off", "off", "the screen reader turned off from the page")
            run("rift-settings --set on-screen-keyboard on", "the keyboard's switch on")
            access_when("off", "on", "the keyboard turned on from the page")
            if not wait_for(30, keyboard_band):
                shot(f"{stem}-settings-keyboard{extension}", "keyboard")
                fail("the on-screen keyboard the page started drew nothing along the bottom of the screen")
            shot(f"{stem}-settings-keyboard{extension}", "keyboard")
            # the key while the page is up: the switch follows what the key did
            press(["meta_l", "alt", "k"], what="the on-screen keyboard key with the page up")
            access_when("off", "off", "the keyboard hidden by its key with the page up")
            press(["meta_l", "alt", "k"], what="the on-screen keyboard key again")
            access_when("off", "on", "the keyboard shown by its key with the page up")
            run("rift-settings --set on-screen-keyboard off", "the keyboard's switch off")
            access_when("off", "off", "the keyboard turned off from the page")
            if not wait_for(30, lambda: keyboard_band() is None):
                fail("the on-screen keyboard is still drawn after the page turned it off")
            ok("the Accessibility page turned the screen reader and the on-screen keyboard on and off, its "
               "switch followed the keyboard's key, and lens --state and pgrep agreed each time")

            # the Keyboard page, over localed, which keeps the desktop's layouts in a file on persist
            # and which horizon follows. at each step the page says what localed and horizon msg
            # keyboard-layouts say on the same boot: the one layout every keyboard starts in, then gb
            # added from the page, which Mod+Shift+Space switches to, then gb put first, then taken off
            # again. the text console keeps the image's keymap whatever the layouts are
            layout_word, layout_name = SETTINGS_LAYOUT

            def layouts_page(what):
                """What the Keyboard page says: its layouts in order as (word, name), and the console's
                keymap. Nothing while localed has not answered on the page."""
                status, told = run("rift-settings --state", what)
                if status != 0:
                    return None
                listed, keymap = [], None
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    if key == "keyboard-layout":
                        word, _, name = value.strip().partition(" ")
                        listed.append((word, name.strip()))
                    elif key == "console-keymap":
                        keymap = value.strip()
                return (listed, keymap) if keymap is not None else None

            def localed_layouts(what):
                """The layouts localed has, in the words the page prints: us, gb, us(dvorak). None
                chosen is the layout every keyboard starts in, which is what the page lists then."""
                codes = "".join(localed_says(f"{what}, the layouts", "X11Layout")).split(",")
                variants = "".join(localed_says(f"{what}, the variants", "X11Variant")).split(",")
                variants += [""] * (len(codes) - len(variants))
                words = [code if not variant else f"{code}({variant})"
                         for code, variant in zip(codes, variants) if code]
                return words or ["us"]

            def horizon_layouts(what):
                """The layouts horizon has by name, and the one in use."""
                status, told = run("horizon msg --json keyboard-layouts", what)
                found = re.search(r'"names":\[(.*?)\],"current_idx":(\d+)',
                                  without_console(told).replace("\n", ""))
                if status != 0 or not found:
                    return None
                return re.findall(r'"([^"]*)"', found.group(1)), int(found.group(2))

            def layouts_agree(words, what):
                """Whether the page and localed list these layouts in this order, and horizon has them
                by the names the page gives them."""
                page_layouts = layouts_page(f"the page for {what}")
                in_horizon = horizon_layouts(f"horizon for {what}")
                return (page_layouts is not None and [word for word, _ in page_layouts[0]] == words
                        and localed_layouts(f"localed for {what}") == words
                        and in_horizon is not None and in_horizon[0] == [name for _, name in page_layouts[0]])

            def layouts_when(words, what):
                """Wait for the page, localed and horizon to agree on these layouts, or say what each said."""
                if wait_for(30, lambda: layouts_agree(words, what)):
                    return
                _, printed = run("journalctl -b -u systemd-localed --no-pager -n 20 -o cat | cat", "localed's journal")
                fail(f"{what}: the Keyboard page says {layouts_page(f'the page for {what} again')}, localed says "
                     f"{localed_layouts(f'localed for {what} again')} and horizon says "
                     f"{horizon_layouts(f'horizon for {what} again')}, where the layouts should be {words}; "
                     f"localed's journal: {without_console(printed).strip()[-400:]!r}")

            def layout_in_use(what):
                """Which of horizon's layouts is in use, by its place in the list."""
                return (horizon_layouts(what) or ([], None))[1]

            def bar_layout_when(wanted, what):
                """Wait for the bar to name this layout, or none, or say what it names."""
                if wait_for(20, lambda: bar_state(what).get("layout") == wanted):
                    return
                fail(f"{what}: the bar names layout {bar_state(f'{what} again').get('layout')!r}, expected {wanted}")

            run("rift-settings --page keyboard", "the Keyboard page")
            if not wait_for(30, lambda: settings_state("the Keyboard page").get("page") == "keyboard"):
                fail("rift-settings --page keyboard did not show that page")
            layouts_when(["us"], "the page as it comes up")
            page_keymap = layouts_page("the console's keymap on the page")[1]
            localed_keymap = "".join(localed_says("the console's keymap", "VConsoleKeymap")) or "none"
            if page_keymap != localed_keymap:
                fail(f"the Keyboard page says the console's keymap is {page_keymap!r}, and localed says "
                     f"{localed_keymap!r}")
            run(f"rift-settings --set add-layout {layout_word}", f"{layout_name} added on the Keyboard page")
            layouts_when(["us", layout_word], f"{layout_name} added from the page")
            # localed wrote the layouts into its own file on persist, in the subvolume /var is. the
            # folder is asked about, since findmnt follows the path it is given
            _, output = run(f"cat {KEYBOARD_FILE}; findmnt -n -o SOURCE -T {os.path.dirname(KEYBOARD_FILE)}",
                            "where the layouts are kept")
            kept = without_console(output)
            if (not re.search(rf'^XKBLAYOUT="?us,{layout_word}"?\s*$', kept, re.M)
                    or not any("persist" in word and "@var" in word for word in kept.split())):
                fail(f"after the page added {layout_word}, {KEYBOARD_FILE} and findmnt say {kept.strip()[-300:]!r}")
            # with two layouts the bar names the one in use, the way GNOME shows its input source
            bar_layout_when("US", "the bar with two layouts")
            # the key switches to the next layout and back to the first, and the page's list is the
            # order horizon keeps them in
            press(["meta_l", "shift", "spc"], what="the key that switches the layout")
            if not wait_for(20, lambda: layout_in_use("the layout in use after the key") == 1):
                fail(f"after Mod+Shift+Space horizon has layout {layout_in_use('the layout in use again')} in use, "
                     f"expected {layout_name}, the second")
            bar_layout_when(layout_word.upper(), "the bar after the key")
            shot(f"{stem}-bar-layout{extension}", "bar-layout")
            press(["meta_l", "shift", "spc"], what="the key again")
            if not wait_for(20, lambda: layout_in_use("the layout in use after the key again") == 0):
                fail(f"after Mod+Shift+Space again horizon has layout {layout_in_use('the layout in use')} in use, "
                     "expected the first")
            bar_layout_when("US", "the bar after the key again")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Keyboard page", f"{stem}-settings-layouts{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            run(f"rift-settings --set first-layout {layout_word}", f"{layout_name} put first on the page")
            layouts_when([layout_word, "us"], f"{layout_name} put first from the page")
            if layout_in_use(f"the layout in use with {layout_word} first") != 0:
                fail(f"horizon does not start in {layout_name} with it first")
            bar_layout_when(layout_word.upper(), f"the bar with {layout_word} first")
            run(f"rift-settings --set remove-layout {layout_word}", f"{layout_name} taken off on the page")
            layouts_when(["us"], f"{layout_name} taken off from the page")
            bar_layout_when("none", "the bar with one layout")
            _, output = run(f"cat {KEYBOARD_FILE}", "the layouts kept after the page took one off")
            if not re.search(r'^XKBLAYOUT="?us"?\s*$', without_console(output), re.M):
                fail(f"after the page took {layout_word} off, {KEYBOARD_FILE} says "
                     f"{without_console(output).strip()[-200:]!r}")
            # the Show button brings up the list Mod+Shift+Slash shows, in the middle of the screen with
            # a light blue border nothing else on it is drawn in, and any key takes it away
            def shortcuts_border(name):
                """A screendump, and how many of its pixels are the border of horizon's list of shortcuts."""
                wide, tall, rgb = screendump(args.qmp, work, name)
                return wide, tall, rgb, sum(1 for at in range(0, wide * tall * 3, 3)
                                            if near(rgb[at:at + 3], SHORTCUTS_BORDER, 10))

            border_before = shortcuts_border("before-shortcuts")[3]
            status, output = run("rift-settings --set shortcuts show", "the Show button of the shortcuts")
            time.sleep(2)
            shown_wide, shown_tall, shown, border_shown = shortcuts_border("shortcuts")
            write_png(f"{stem}-settings-shortcuts{extension}", shown_wide, shown_tall, shown)
            said_problem = settings_state("the page after the Show button").get("problem")
            press(["esc"], what="a key that takes the shortcuts away")
            if status != 0 or said_problem or border_shown - border_before < SHORTCUTS_BORDER_PIXELS:
                fail(f"after the Show button the screen has {border_shown} pixels of the shortcuts' border where "
                     f"it had {border_before}, and the page says {said_problem!r}, see "
                     f"{stem}-settings-shortcuts{extension}")
            ok(f"the Keyboard page added {layout_name}, put it first and took it off, and localed, horizon and "
               f"the file on persist agreed each time; Mod+Shift+Space switched to it and back, with the bar "
               f"naming the layout in use until only one was left; the console keeps keymap {page_keymap}; the "
               f"Show button brought up the shortcuts")

            # the Mouse and touchpad page. the page says what the machine has, which is what udev tags
            # each input device as on the same boot: the vm has qemu's ps/2 mouse and its virtio tablet,
            # which udev tags a mouse and libinput takes as one, and no touchpad. a setting the page
            # changes is written into the part of horizon's config it keeps, horizon reads its config
            # again with no error, and a touchpad's setting on a machine with none writes nothing
            def pointer_page(what):
                """What the Mouse and touchpad page says: the mice and the touchpads by name, and every
                setting. Nothing until the page has looked."""
                status, told = run("rift-settings --state", what)
                if status != 0:
                    return None
                found_devices, found_settings = [], {}
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    if key in ("mouse", "touchpad"):
                        found_devices.append((key, value.strip()))
                    elif key in SETTINGS_KEYS:
                        found_settings[key] = value.strip()
                if "mice" not in found_settings:
                    return None
                return sorted(found_devices), found_settings

            def udev_pointers(what):
                """The mice and the touchpads udev tags, by name."""
                _, told = run("for e in /sys/class/input/event*; set -l tags (udevadm info -q property -p $e); "
                              "set -l named (cat $e/device/name); if contains ID_INPUT_TOUCHPAD=1 $tags; "
                              "echo \"tagged touchpad $named\"; else if contains ID_INPUT_MOUSE=1 $tags; "
                              "or contains ID_INPUT_POINTINGSTICK=1 $tags; echo \"tagged mouse $named\"; end; end",
                              what)
                tagged = []
                for printed_line in without_console(told).splitlines():
                    kind_and_name = re.match(r"^tagged (mouse|touchpad) (.+?)\s*$", printed_line.strip())
                    if kind_and_name:
                        tagged.append((kind_and_name.group(1), kind_and_name.group(2)))
                return sorted(tagged)

            def config_loads(what):
                """How many times horizon has read its config this boot, and how many of them failed."""
                _, told = run("journalctl -b -t horizon -o cat --no-pager | grep -c -e 'loaded config from' "
                              "-e 'error loading config'; journalctl -b -t horizon -o cat --no-pager | "
                              "grep -c 'error loading config'", what)
                counts = [int(number) for number in re.findall(r"^\s*(\d+)\s*$", without_console(told), re.M)]
                return tuple(counts) if len(counts) == 2 else (0, 0)

            def pointer_written(wanted, what):
                """Whether the part horizon includes and the page's own file both say what was set."""
                _, told = run(f"cat {POINTER_PART} {POINTER_FILE}", what)
                return all(re.search(pattern, without_console(told), re.S) for pattern in wanted)

            run("rift-settings --page pointer", "the Mouse and touchpad page")
            if not wait_for(30, lambda: settings_state("the Mouse and touchpad page").get("page") == "pointer"):
                fail("rift-settings --page pointer did not show that page")
            pointer_said = wait_for(30, lambda: pointer_page("the Mouse and touchpad page"))
            if not pointer_said:
                fail("the Mouse and touchpad page says nothing about the mice and the touchpads")
            tagged_said = udev_pointers("the mice and touchpads udev tags")
            if pointer_said[0] != tagged_said:
                fail(f"the Mouse and touchpad page lists {pointer_said[0]}, and udev tags {tagged_said}")
            if pointer_said[1].get("touchpads") != "0" or pointer_said[1].get("mice") in (None, "0"):
                fail(f"the Mouse and touchpad page says mice {pointer_said[1].get('mice')!r} and touchpads "
                     f"{pointer_said[1].get('touchpads')!r}, and the vm has mice and no touchpad")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Mouse and touchpad page", f"{stem}-settings-pointer{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            loads_before = config_loads("horizon's config before the page wrote its part")
            run("rift-settings --set mouse-natural-scrolling on", "natural scrolling for the mouse")
            run("rift-settings --set mouse-speed 5", "the mouse's speed")
            if not wait_for(20, lambda: pointer_written(
                    [r"mouse \{[^}]*accel-speed 0\.5[^}]*natural-scroll",
                     r"mouse-natural-scrolling on", r"mouse-speed 5"], "the part the page wrote")):
                _, told = run(f"cat {POINTER_PART} {POINTER_FILE}", "the part the page wrote again")
                fail(f"the page set natural scrolling and a speed for the mouse, and the part and its file say "
                     f"{without_console(told).strip()[-500:]!r}")
            if not wait_for(20, lambda: config_loads("horizon's config after the page wrote")[0] > loads_before[0]):
                fail(f"horizon did not read its config again after the page wrote its part: {loads_before}")
            loads_after = config_loads("horizon's config errors after the page wrote")
            if loads_after[1] != loads_before[1]:
                _, told = run("journalctl -b -t horizon -o cat --no-pager -n 30 | cat", "horizon's log")
                fail(f"horizon could not read its config with the part the page wrote: "
                     f"{without_console(told).strip()[-800:]!r}")
            pointer_after = pointer_page("the Mouse and touchpad page after the change")
            if not pointer_after or (pointer_after[1].get("mouse-natural-scrolling"),
                                     pointer_after[1].get("mouse-speed")) != ("on", "5"):
                fail(f"the Mouse and touchpad page says {pointer_after and pointer_after[1]} after it set natural "
                     "scrolling and speed 5")
            # the vm has no touchpad, so the page has no row for tapping and writes nothing for it
            run("rift-settings --set tap-to-click off", "tap to click on a machine with no touchpad")
            if not pointer_written([r"touchpad \{[^}]*\btap\b", r"tap-to-click on"], "the part after tap to click"):
                fail("the page wrote tap to click off on a machine with no touchpad")
            run("rift-settings --set mouse-natural-scrolling off", "natural scrolling off again")
            run("rift-settings --set mouse-speed 0", "the mouse's speed again")
            if not wait_for(20, lambda: pointer_written([r"mouse \{\s*accel-speed 0\.0\s*\}"],
                                                        "the part put back")):
                fail("the part does not say the mouse's own speed and scrolling after the page put them back")
            ok(f"the Mouse and touchpad page lists {', '.join(name for _, name in pointer_said[0])} as udev "
               f"does, wrote natural scrolling and a speed for the mouse into horizon's part and horizon read "
               f"it with no error, and wrote nothing for a touchpad the vm has not got")

            # the Dock page, over the file the shell keeps the dock's apps in and the file of its
            # settings. the page lists what the dock keeps in the order the file and the dock have it,
            # moves one and takes one off with the file and the dock following, and each of its
            # settings moves the dock's surface where horizon msg layers says and the screendump has
            # it, then goes back
            def dock_page(what):
                """What the Dock page says: the apps the dock keeps in order as (id, name), and its
                settings. Nothing until the page has read its files."""
                status, told = run("rift-settings --state", what)
                if status != 0:
                    return None
                kept_apps, dock_settings = [], {}
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    if key == "pinned-app":
                        word, _, name = value.strip().partition(" ")
                        kept_apps.append((word, name.strip()))
                    elif key in ("pinned", "dock-position", "dock-extend", "dock-icons", "dock-hide"):
                        dock_settings[key] = value.strip()
                return (kept_apps, dock_settings) if "pinned" in dock_settings else None

            def dock_file(what):
                """The apps the shell's file lists, in its order."""
                return without_console(run(f"cat {DOCK_FILE}", what)[1]).split()

            def dock_agrees(wanted, what):
                """Whether the page, the file and the dock all have these apps first, in this order."""
                said = dock_page(f"the page for {what}")
                return (said is not None and [word for word, _ in said[0]] == wanted
                        and dock_file(f"the file for {what}") == wanted
                        and list(dock_items(f"the dock for {what}"))[:len(wanted)] == wanted)

            def kept_when(wanted, what):
                """Wait for the page, the file and the dock to agree on these apps, or say what each said."""
                if wait_for(30, lambda: dock_agrees(wanted, what)):
                    return
                fail(f"{what}: the Dock page says {dock_page(f'the page for {what} again')}, the file "
                     f"{dock_file(f'the file for {what} again')} and the dock "
                     f"{list(dock_items(f'the dock for {what} again'))}, where the apps should be {wanted}")

            def dock_layer(what, namespace="lens-dock"):
                """Where horizon put the dock, or another of the shell's surfaces, as x, y, width,
                height and what it keeps of the screen."""
                _, told = run("horizon msg --json layers", what)
                placed = re.search(r'"namespace":"' + namespace + r'"[^}]*?"geometry":\{"x":(-?\d+),"y":(-?\d+),'
                                   r'"width":(\d+),"height":(\d+)\},"exclusive_zone":(-?\d+)',
                                   without_console(told).replace("\n", ""))
                return tuple(int(number) for number in placed.groups()) if placed else None

            def dock_drawn(placed, name):
                """A screendump, and whether the dock is drawn where horizon says it put it: the bar's
                gray over most of that rectangle, and next to none in the two rows past the edge of it
                that faces the windows, which is the desktop's gray in the gap before them."""
                wide, tall, pixels = screendump(args.qmp, work, name)
                left, top, across, down = placed[:4]

                def gray_share(rows):
                    counted = total = 0
                    for y in rows:
                        if not 0 <= y < tall:
                            continue
                        for x in range(max(left, 0), min(left + across, wide)):
                            total += 1
                            counted += near(pixels[(y * wide + x) * 3:(y * wide + x) * 3 + 3], BAR, 3)
                    return counted / total if total else 0.0

                facing = range(top + down + 1, top + down + 3) if top < tall / 2 else range(top - 3, top - 1)
                shares = (round(gray_share(range(top, top + down)), 2), round(gray_share(facing), 2))
                return wide, tall, pixels, shares[0] > 0.4 and shares[1] < 0.2, shares

            def dock_set(setting, value, fits, png):
                """Set one of the dock's settings from the page, wait for horizon to have the dock where
                fits says and for the shell and the page to say the setting, and check the screendump
                has the dock there. What horizon says, for the next setting to compare with."""
                what = f"{setting} {value} from the Dock page"
                run(f"rift-settings --set {setting} {value}", what)
                placed = wait_for(30, lambda: next((found for found in [dock_layer(what)]
                                                    if found and fits(found)), None))
                if not placed:
                    fail(f"after {what} horizon has the dock at {dock_layer(f'{what} again')}")
                if bar_state(f"the shell after {what}").get(setting) != value:
                    fail(f"after {what} lens --state says {setting} "
                         f"{bar_state(f'the shell after {what} again').get(setting)!r}")
                # the page follows the same file the shell does, a moment behind it
                page_said = wait_for(20, lambda: next(
                    (said for said in [(dock_page(f"the page after {what}") or ([], {}))[1]]
                     if said.get(setting) == value), None))
                if not page_said:
                    said_again = (dock_page(f"the page after {what} again") or ([], {}))[1]
                    fail(f"after {what} the Dock page says {said_again}")
                written = without_console(run(f"cat {DOCK_OPTIONS}", f"the dock's settings after {what}")[1])
                if f"{setting} {value}" not in written.splitlines():
                    fail(f"after {what} {DOCK_OPTIONS} says {written.strip()[-200:]!r}")
                drawn = wait_for(20, lambda: next((found for found in [dock_drawn(placed, "dock-setting")]
                                                   if found[3]), None))
                if not drawn:
                    shown = dock_drawn(placed, "dock-setting")
                    write_png(png, *shown[:3])
                    fail(f"after {what} the screendump does not have the dock at {placed}: its gray covers "
                         f"{shown[4][0]} of it and {shown[4][1]} of the rows past it, see {png}")
                write_png(png, *drawn[:3])
                return placed

            run("rift-settings --page dock", "the Dock page")
            if not wait_for(30, lambda: settings_state("the Dock page").get("page") == "dock"):
                fail("rift-settings --page dock did not show that page")
            kept_before = dock_file("the apps the dock keeps")
            if len(kept_before) < 3:
                fail(f"the dock keeps {kept_before}, expected the apps the image pins and {DOCK_APP}")
            kept_when(kept_before, "the page as it comes up")
            kept_names = dict((dock_page("the names on the Dock page") or ([], {}))[0])
            if kept_names.get(MENU_APP_ID) != MENU_APP:
                fail(f"the Dock page names the apps {kept_names}, expected {MENU_APP} for {MENU_APP_ID}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Dock page", f"{stem}-settings-dock{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            second_kept, last_kept = kept_before[1], kept_before[-1]
            run(f"rift-settings --set move-up {second_kept}", f"{second_kept} moved up on the Dock page")
            kept_when([second_kept, kept_before[0]] + kept_before[2:], f"{second_kept} moved up")
            run(f"rift-settings --set move-down {second_kept}", f"{second_kept} moved down again")
            kept_when(kept_before, f"{second_kept} moved down again")
            run(f"rift-settings --set unpin {last_kept}", f"{last_kept} taken off the dock on the page")
            kept_when(kept_before[:-1], f"{last_kept} taken off")
            if last_kept in dock_items(f"the dock without {last_kept}"):
                fail(f"the dock still lists {last_kept}, which is not running, after the page took it off")
            run(f"rift-settings --set pin {last_kept}", f"{last_kept} pinned again on the page")
            kept_when(kept_before, f"{last_kept} pinned again")

            full_dock = dock_layer("the dock before its settings changed")
            if not full_dock or full_dock[0] != 0 or full_dock[3] != DOCK_HEIGHT or full_dock[4] != DOCK_HEIGHT:
                fail(f"horizon has the dock at {full_dock}, expected it from side to side along the bottom, "
                     f"{DOCK_HEIGHT} tall and keeping as much")
            screen_across, screen_down = full_dock[2], full_dock[1] + full_dock[3]
            placed_top = dock_set("dock-position", "top",
                                  lambda found: found[:4] == (0, BAR_HEIGHT, screen_across, DOCK_HEIGHT)
                                  and found[4] == DOCK_HEIGHT, f"{stem}-dock-top{extension}")
            # a menu of the bar still hangs from the bar with the dock along the top, over the dock:
            # it stays in the working area, which starts under the dock, and goes up by the dock's height
            run("lens --menu", "the Applications menu with the dock along the top")
            menu_placed = wait_for(20, lambda: dock_layer("the menu with the dock along the top", "lens-menu"))
            run("lens --escape", "escape, which closes the menu over the dock")
            if not menu_placed or menu_placed[1] != BAR_HEIGHT:
                fail(f"with the dock along the top horizon has the Applications menu at {menu_placed}, expected "
                     f"it at y {BAR_HEIGHT}, under the bar")
            if not wait_for(20, lambda: bar_state("the menu closed over the dock").get("menu") == "closed"):
                fail("escape did not close the Applications menu with the dock along the top")
            dock_set("dock-position", "bottom", lambda found: found == full_dock, f"{stem}-dock-bottom{extension}")
            placed_middle = dock_set("dock-extend", "off",
                                     lambda found: found[2] < screen_across / 2 and found[0] > 0
                                     and abs(2 * found[0] + found[2] - screen_across) <= 2
                                     and found[1] == screen_down - DOCK_OFF_EDGE - DOCK_HEIGHT
                                     and found[3] == DOCK_HEIGHT and found[4] == DOCK_HEIGHT,
                                     f"{stem}-dock-middle{extension}")
            dock_set("dock-extend", "on", lambda found: found == full_dock, f"{stem}-dock-extended{extension}")
            placed_large = dock_set("dock-icons", "large",
                                    lambda found: found[:4] == (0, screen_down - DOCK_LARGE, screen_across, DOCK_LARGE)
                                    and found[4] == DOCK_LARGE, f"{stem}-dock-large{extension}")
            dock_set("dock-icons", "small", lambda found: found == full_dock, f"{stem}-dock-small{extension}")
            ok(f"the Dock page lists {', '.join(kept_before)} as the file and the dock do, moved one up and "
               f"down and took one off and on with both following, and horizon put the dock at {placed_top} "
               f"along the top with the Applications menu at {menu_placed[:4]} over it, {placed_middle} in the "
               f"middle off the edge and {placed_large} with large icons, each where the screendump has it, "
               f"then back at {full_dock}")

            # hiding. the dock keeps nothing of the screen then and is a line along the bottom edge; the
            # pointer pushed against that edge brings it out over the windows, it goes again a moment
            # after the pointer has left it, and with the setting off it is back to stay
            placed_hidden = dock_set("dock-hide", "on",
                                     lambda found: found[:4] == (0, screen_down - DOCK_HIDDEN, screen_across, DOCK_HIDDEN)
                                     and found[4] == 0, f"{stem}-dock-hidden{extension}")
            if bar_state("the shell with the dock hidden").get("dock-hidden") != "yes":
                fail(f"lens --state says dock-hidden {bar_state('the shell again').get('dock-hidden')!r} with the "
                     f"dock at {placed_hidden}")
            placed_out_wanted = (0, screen_down - DOCK_HEIGHT, screen_across, DOCK_HEIGHT, 0)
            point(args.qmp, size, (width // 2, height))
            placed_out = wait_for(20, lambda: next((found for found in [dock_layer("the dock with the pointer at the edge")]
                                                    if found == placed_out_wanted), None))
            if not placed_out:
                fail(f"with the pointer at the bottom edge horizon has the dock at "
                     f"{dock_layer('the dock at the edge again')}, expected {placed_out_wanted}")
            if bar_state("the shell with the dock out").get("dock-hidden") != "no":
                fail(f"lens --state says dock-hidden {bar_state('the shell again').get('dock-hidden')!r} with the "
                     "pointer at the bottom edge")
            # the windows have the room the dock stood in, so the rows over it are a window's; only the
            # dock's own rectangle is looked at
            out_drawn = wait_for(20, lambda: next((found for found in [dock_drawn(placed_out, "dock-out")]
                                                   if found[4][0] > 0.4), None))
            if not out_drawn:
                shown = dock_drawn(placed_out, "dock-out")
                write_png(f"{stem}-dock-out{extension}", *shown[:3])
                fail(f"with the pointer at the bottom edge the screendump does not have the dock at {placed_out}: its "
                     f"gray covers {shown[4][0]} of it, see {stem}-dock-out{extension}")
            write_png(f"{stem}-dock-out{extension}", *out_drawn[:3])
            point(args.qmp, size, away)
            if not wait_for(20, lambda: dock_layer("the dock after the pointer left it") == placed_hidden):
                fail(f"a moment after the pointer left the dock horizon has it at "
                     f"{dock_layer('the dock after the pointer left it again')}, expected {placed_hidden}")
            dock_set("dock-hide", "off", lambda found: found == full_dock, f"{stem}-dock-stays{extension}")
            ok(f"the Dock page's hiding put the dock at {placed_hidden} keeping nothing of the screen, the pointer "
               f"at the bottom edge brought it out at {placed_out[:4]} over the windows and it went again when the "
               f"pointer left, and with hiding off it is back at {full_dock}")

            # the Notifications page. Do not disturb is one line of the owner's that the page and the
            # clock menu's switch both write: from the page it keeps a notification off the screen and
            # in the clock menu's list, and the menu's switch moves the page's. the app that sent them
            # is listed, and with its banners off from the page the next one it sends stays off the
            # screen too
            def notices_page(what):
                """What the Notifications page says: Do not disturb, and each app with whether its
                banners show. Nothing until the page has read its files."""
                status, told = run("rift-settings --state", what)
                if status != 0:
                    return None
                quiet_said, apps_said = None, {}
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    if key == "do-not-disturb":
                        quiet_said = value.strip()
                    elif key == "app-banners":
                        shown, _, app_name = value.strip().partition(" ")
                        apps_said[app_name.strip()] = shown
                return (quiet_said, apps_said) if quiet_said is not None else None

            def quiet_agrees(wanted, what):
                """Whether the page, the shell and the file all say Do not disturb is this."""
                said = notices_page(f"the page for {what}")
                kept_word = without_console(run(f"cat {QUIET_FILE}", f"the file for {what}")[1]).strip()
                return (said is not None and said[0] == wanted and kept_word == wanted
                        and bar_state(f"the shell for {what}").get("do-not-disturb") == wanted)

            def quiet_when(wanted, what):
                """Wait for the page, the shell and the file to agree on Do not disturb, or say what each said."""
                if wait_for(30, lambda: quiet_agrees(wanted, what)):
                    return
                fail(f"{what}: the Notifications page says {notices_page(f'the page for {what} again')}, the shell "
                     f"says {bar_state(f'the shell for {what} again').get('do-not-disturb')!r} and the file "
                     f"{without_console(run(f'cat {QUIET_FILE}', 'the file again')[1]).strip()!r}, expected {wanted}")

            def banners_when(wanted, what):
                """Wait for the page to say this about notify-send's banners and for the file of apps
                kept quiet to have it in it only when they are off."""
                def agreed():
                    shown = (notices_page(f"the page for {what}") or (None, {}))[1].get(NOTIFY_APP)
                    listed = [line.strip() for line in without_console(
                        run(f"cat {QUIET_APPS}", f"the apps kept quiet for {what}")[1]).splitlines()]
                    return shown == wanted and (NOTIFY_APP in listed) == (wanted == "off")
                if not wait_for(20, agreed):
                    fail(f"{what}: the Notifications page says {notices_page(f'the page for {what} again')}, "
                         f"expected {NOTIFY_APP}'s banners {wanted}")

            run("rift-settings --page notifications", "the Notifications page")
            if not wait_for(30, lambda: settings_state("the Notifications page").get("page") == "notifications"):
                fail("rift-settings --page notifications did not show that page")
            quiet_when("off", "the page as it comes up")
            banners_when("on", "the page as it comes up")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Notifications page", f"{stem}-settings-notifications{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            run("rift-settings --set do-not-disturb on", "Do not disturb on from the page")
            quiet_when("on", "Do not disturb on from the page")
            counted_before = notices("the notifications before one with Do not disturb from the page")
            run(f'notify-send "{NOTIFY_SUMMARY}" "Do not disturb from the page keeps this one quiet."',
                "a notification with Do not disturb on from the page")
            if wait_for(20, lambda: notices("the state with Do not disturb from the page")
                        == (0, counted_before[1] + 1)) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) with Do not disturb on from "
                     f"the page, where it kept {counted_before[1]} before")
            if bar_state("the banners with Do not disturb from the page").get("banners") != "none":
                fail("a notification is on screen with Do not disturb on from the page")
            # the clock menu's switch turns it off again, and the page follows
            click(args.qmp, size, clock_point)
            quiet_menu = wait_for(20, lambda: clock_open("the clock menu over the Notifications page"))
            if not quiet_menu:
                fail("a click on the clock opened no clock menu over the Notifications page")
            click(args.qmp, size, switch_point(quiet_menu))
            quiet_when("off", "the clock menu's switch")
            run("lens --escape", "the clock menu closed")
            if wait_for(20, lambda: bar_state("the clock menu after escape").get("clock-menu") == "closed") is not True:
                fail("escape left the clock menu open")
            point(args.qmp, size, away)
            # notify-send's banners off from the page keep its next one off the screen, and on again
            # let the one after that show
            run(f"rift-settings --set app-banners {NOTIFY_APP} off", f"{NOTIFY_APP}'s banners off from the page")
            banners_when("off", f"{NOTIFY_APP}'s banners off")
            counted_before = notices("the notifications before one with its banners off")
            run(f'notify-send "{NOTIFY_SUMMARY}" "With its banners off this one goes into the list."',
                f"a notification from {NOTIFY_APP} with its banners off")
            if wait_for(20, lambda: notices("the state with its banners off")
                        == (0, counted_before[1] + 1)) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) with {NOTIFY_APP}'s banners off, "
                     f"where it kept {counted_before[1]} before")
            run(f"rift-settings --set app-banners {NOTIFY_APP} on", f"{NOTIFY_APP}'s banners on again")
            banners_when("on", f"{NOTIFY_APP}'s banners on again")
            run(f'notify-send "{NOTIFY_SUMMARY}" "With its banners on again this one shows."',
                f"a notification from {NOTIFY_APP} with its banners on")
            if wait_for(20, lambda: (notices("the state with its banners on") or (0, 0))[0] == 1) is not True:
                fail(f"lens shows {notices('the notifications')} (on screen, kept) with {NOTIFY_APP}'s banners on again")
            if wait_for(20, lambda: (notices("the banner going") or (1, 0))[0] == 0) is not True:
                fail("the notification from the page's last check is still on screen after its five seconds")
            ok(f"the Notifications page's Do not disturb kept a notification off the screen and in the clock menu's "
               f"list, the clock menu's switch turned the page's off, and {NOTIFY_APP}'s banners off from the page "
               f"kept its next one off the screen too")

            # the Apps page. each kind of file opens with the app xdg-mime names on the same boot. the
            # one kind the image has more than one app for, text, is given another from the page, and
            # xdg-mime follows for two of its types while the owner's own list holds them all; a file
            # handed to that app the way glib hands a file to an app with Terminal=true opens in a
            # terminal window with the app in it. then the kind goes back to the app the image has
            def defaults_page(what):
                """What the Apps page says: for each kind, the type it opens with and the desktop id
                of its app or none, and the ids of the apps that open it. Nothing until the page has
                read the lists."""
                status, told = run("rift-settings --state", what)
                if status != 0:
                    return None
                said_kinds, said_apps = {}, {}
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    words = value.split()
                    if key == "default-app" and len(words) == 3:
                        said_kinds[words[0]] = (words[1], words[2])
                    elif key == "can-open" and words:
                        said_apps[words[0]] = words[1:]
                return (said_kinds, said_apps) if said_kinds else None

            def xdg_default(mime, what):
                """The desktop id xdg-mime says opens a type, or none when it names nothing."""
                _, told = run(f"xdg-mime query default {mime}", what)
                named = [said.strip() for said in without_console(told).splitlines() if said.strip()]
                return named[-1] if named else "none"

            def defaults_differ(what):
                """The first kind the page and xdg-mime disagree about, in a sentence, or None."""
                said = defaults_page(f"the Apps page for {what}")
                if said is None:
                    return "the page says nothing"
                for kind, (mime, desktop) in said[0].items():
                    named = xdg_default(mime, f"what opens {mime} for {what}")
                    if named != desktop:
                        return f"the page opens {kind} ({mime}) with {desktop} and xdg-mime with {named}"
                return None

            def kind_opens(wanted, what):
                """Whether the page, xdg-mime for two types of the kind and the owner's own list all
                say this app opens it."""
                said = defaults_page(f"the Apps page for {what}")
                if said is None or said[0].get(apps_kind, ("", ""))[1] != wanted:
                    return False
                for mime in (apps_mime, SETTINGS_KIND[3]):
                    if xdg_default(mime, f"what opens {mime} for {what}") != wanted:
                        return False
                listed = without_console(run("cat ~/.config/mimeapps.list", f"the owner's list for {what}")[1])
                return all(f"{mime}={wanted}" in listed.splitlines() for mime in (apps_mime, SETTINGS_KIND[3]))

            def handed(what):
                """The names of the processes whose command line has the handed file in it."""
                _, told = run(f"for p in (pgrep -f {HANDED_FILE}); cat /proc/$p/comm 2>/dev/null; end", what)
                return [name.strip() for name in without_console(told).splitlines() if name.strip()]

            run("rift-settings --page apps", "the Apps page")
            if not wait_for(30, lambda: settings_state("the Apps page").get("page") == "apps"):
                fail("rift-settings --page apps did not show that page")
            apps_before = wait_for(30, lambda: defaults_page("the Apps page as it comes up"))
            if not apps_before:
                fail("the Apps page says nothing about which app opens each kind of file")
            apps_differ = defaults_differ("the page as it comes up")
            if apps_differ:
                fail(f"as the Apps page comes up {apps_differ}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Apps page", f"{stem}-settings-apps{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            apps_several = [kind for kind, ids in apps_before[1].items() if len(ids) > 1]
            if apps_several != [SETTINGS_KIND[0]]:
                fail(f"the Apps page has more than one app for {apps_several}, expected {SETTINGS_KIND[0]} alone: "
                     f"{apps_before[1]}")
            apps_kind = apps_several[0]
            apps_mime, apps_first = apps_before[0][apps_kind]
            apps_other = next(app_id for app_id in apps_before[1][apps_kind] if f"{app_id}.desktop" != apps_first)
            if (apps_first, f"{apps_other}.desktop") != SETTINGS_KIND[1:3]:
                fail(f"the image opens {apps_kind} with {apps_first} and the page offers {apps_other} next, expected "
                     f"{SETTINGS_KIND[1]} and {SETTINGS_KIND[2]}")
            run(f"rift-settings --set default-app {apps_kind} {apps_other}", f"{apps_other} for {apps_kind} on the Apps page")
            if not wait_for(20, lambda: kind_opens(f"{apps_other}.desktop", f"{apps_other} chosen")):
                fail(f"after the Apps page chose {apps_other} for {apps_kind}, the page, xdg-mime and the owner's list say "
                     f"{defaults_page('the page again')}, {xdg_default(apps_mime, 'xdg-mime again')} and "
                     f"{without_console(run('cat ~/.config/mimeapps.list', 'the list again')[1]).strip()[-300:]!r}")
            # glib hands a file to an app with Terminal=true by running xdg-terminal-exec with the app's
            # command, and xdg-terminal-exec starts the terminal xdg-terminals.list names with it
            run(f"echo 'Rift boot test' > {HANDED_FILE}", "a text file to hand on")
            run(f"systemd-run --user --quiet --collect --unit={HANDED_UNIT} -- xdg-terminal-exec hx {HANDED_FILE}",
                f"the text file handed to {apps_other}")
            apps_running = wait_for(30, lambda: next(
                (names for names in [handed(f"the terminal with {apps_other} in it")]
                 if any("ghostty" in name for name in names) and any("hx" in name for name in names)), None))
            if not apps_running:
                _, output = run(f"journalctl --user -b -o cat -u {HANDED_UNIT} -n 20 | cat", "the handed file's unit")
                fail(f"the text file handed to {apps_other} is open in {handed('the processes again')}, expected a "
                     f"terminal with hx in it: {without_console(output).strip()[-500:]!r}")
            run(f"systemctl --user stop {HANDED_UNIT}", "the terminal with the handed file closed")
            if not wait_for(30, lambda: not handed("the handed file closed")):
                fail(f"stopping {HANDED_UNIT} left {handed('the processes left')} with the handed file open")
            run(f"rift-settings --set default-app {apps_kind} {apps_first}", f"{apps_first} for {apps_kind} again")
            if not wait_for(20, lambda: kind_opens(apps_first, f"{apps_first} put back")):
                fail(f"after the Apps page put {apps_first} back for {apps_kind}, the page says {defaults_page('the page')} "
                     f"and xdg-mime {xdg_default(apps_mime, 'xdg-mime once more')}")
            ok(f"the Apps page opens every kind with the app xdg-mime names ({', '.join(f'{kind} {app}' for kind, (_, app) in apps_before[0].items())}), "
               f"chose {apps_other} for {apps_kind} with xdg-mime following for {apps_mime} and {SETTINGS_KIND[3]}, which "
               f"opened a handed file in a terminal ({', '.join(apps_running)}), and put {apps_first} back")

            # the Privacy and security page. it says what the permission store says about the camera
            # after step 5's Camera app asked, and turns that answer round and back with the store
            # following; recent files follow dconf; the firewall is the unit that loads it; an app rift
            # net turned off in a terminal is on the page, which gives it the network back; and the
            # security level is the id fwupd gives on the bus
            def store_answers(what):
                """The camera's answers in the permission store as {app: [words]}, or None when it
                has no entry for the camera, which the portal makes the first time an app asks."""
                status, told = run(f"busctl --user --json=short --no-pager call {PERMISSION_STORE} Lookup ss devices camera",
                                   what)
                printed = without_console(told)
                found = re.search(r"\{.*\}", printed, re.S)
                if status != 0 or not found:
                    return None
                try:
                    return json.loads(found.group(0))["data"][0]
                except (ValueError, KeyError, IndexError, TypeError):
                    fail(f"busctl's answer about the camera is not the json it prints: {printed.strip()[-300:]!r}")
                return None

            def privacy_page(what):
                """What the Privacy page says: its lines of one value, the camera's answers as
                {app: word} and the sandboxes as {app: word}. Nothing until the page has read."""
                status, told = run("rift-settings --state", what)
                if status != 0:
                    return None
                said_lines, said_camera, said_sandboxes = {}, {}, {}
                for printed_line in without_console(told).splitlines():
                    key, _, value = printed_line.strip().partition(" ")
                    word, _, named = value.strip().partition(" ")
                    if key == "camera-app":
                        said_camera["" if named.strip() == "-" else named.strip()] = word
                    elif key == "app-network":
                        said_sandboxes[named.strip()] = word
                    elif key in ("camera-apps", "recent-files", "firewall", "sandboxed", "device-security"):
                        said_lines[key] = value.strip()
                return (said_lines, said_camera, said_sandboxes) if "camera-apps" in said_lines else None

            def camera_says(wanted, what):
                """Whether the page and the permission store both give the camera app this answer."""
                said = privacy_page(f"the page for {what}")
                kept = store_answers(f"the store for {what}") or {}
                return said is not None and said[1].get(camera_app) == wanted and kept.get(camera_app) == [wanted]

            def recent_says(wanted, key_wanted, what):
                """Whether the page says this about recent files and dconf holds one of these for the key."""
                said = privacy_page(f"the page for {what}")
                key_now = without_console(run(f"dconf read {RECENT_FILES_KEY}", f"the key for {what}")[1]).strip()
                return said is not None and said[0].get("recent-files") == wanted and key_now in key_wanted

            def every_app_online(what):
                """Whether rift net and the page both say no app is off and none runs in a sandbox."""
                _, told = run("rift net", what)
                said = privacy_page(f"the page for {what}")
                return ("Every app has the network" in " ".join(without_console(told).split())
                        and said is not None and said[0].get("sandboxed") == "0")

            run("rift-settings --page privacy", "the Privacy and security page")
            if not wait_for(30, lambda: settings_state("the Privacy page").get("page") == "privacy"):
                fail("rift-settings --page privacy did not show that page")
            privacy_before = wait_for(30, lambda: privacy_page("the Privacy page as it comes up"))
            if not privacy_before:
                fail("the Privacy and security page says nothing")
            # fwupd starts for the page's question and looks at the machine before it answers
            privacy_id = wait_for(90, lambda: (privacy_page("fwupd's answer on the page") or ({},))[0].get("device-security"))
            _, output = run("busctl --system get-property org.freedesktop.fwupd / org.freedesktop.fwupd HostSecurityId | cat",
                            "the security id fwupd gives")
            fwupd_id = re.search(r's\s+"(.*)"', without_console(output))
            if not privacy_id or not fwupd_id or fwupd_id.group(1) != privacy_id:
                fail(f"the Privacy page gives the security level as {privacy_id!r}, and fwupd on the bus says "
                     f"{without_console(output).strip()[-200:]!r}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Privacy and security page", f"{stem}-settings-privacy{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            store_before = store_answers("the camera's answers in the permission store")
            store_words = {app: (words[0] if len(words) == 1 and words[0] in ("yes", "no") else "ask")
                           for app, words in (store_before or {}).items()}
            privacy_now = privacy_page("the camera's answers on the page")
            if privacy_now is None or privacy_now[1] != store_words:
                fail(f"the Privacy page says the camera's answers are {privacy_now and privacy_now[1]}, and the "
                     f"permission store says {store_before}")
            camera_turned = "none, since no app has asked for it yet"
            camera_app = next((app for app in sorted(store_words) if store_words[app] in ("yes", "no")), None)
            if camera_app is None:
                print("\nboot-test: the permission store has no answer about the camera, so the Privacy page has none "
                      "to change", flush=True)
            else:
                camera_was = store_words[camera_app]
                camera_flip, camera_back = ("off", "on") if camera_was == "yes" else ("on", "off")
                camera_named = camera_app or "-"
                run(f"rift-settings --set camera-app {camera_named} {camera_flip}", f"the camera {camera_flip} for {camera_named}")
                if not wait_for(20, lambda: camera_says("no" if camera_flip == "off" else "yes", "the answer turned round")):
                    fail(f"after the page turned the camera {camera_flip} for {camera_named}, it says "
                         f"{privacy_page('the page again')} and the store {store_answers('the store again')}")
                run(f"rift-settings --set camera-app {camera_named} {camera_back}", f"the camera {camera_back} again")
                if not wait_for(20, lambda: camera_says(camera_was, "the answer turned back")):
                    fail(f"after the page turned the camera {camera_back} again for {camera_named}, the store says "
                         f"{store_answers('the store once more')}")
                camera_turned = f"{camera_named}'s {camera_was} turned {camera_flip} and back with the store following"
            if not recent_says("on", ("", "true"), "recent files as the page comes up"):
                fail(f"the Privacy page says recent files are {privacy_before[0].get('recent-files')!r} with the key unset")
            run("rift-settings --set recent-files off", "recent files off on the page")
            if not wait_for(20, lambda: recent_says("off", ("false",), "recent files off")):
                fail("after the page turned recent files off, the page and dconf do not both say so")
            run("rift-settings --set recent-files on", "recent files on again")
            if not wait_for(20, lambda: recent_says("on", ("true",), "recent files on again")):
                fail("after the page turned recent files on again, the page and dconf do not both say so")
            _, output = run("systemctl is-active nftables.service | cat", "the unit that loads the firewall")
            firewall_state = (without_console(output).strip().splitlines() or [""])[-1].strip()
            if firewall_state != "active" or privacy_before[0].get("firewall") != "on":
                fail(f"nftables.service is {firewall_state!r} and the Privacy page says the firewall is "
                     f"{privacy_before[0].get('firewall')!r}")
            status, output = run(f"rift net off {PRIVACY_APP}", "an app's network off in a terminal")
            if status != 0:
                fail(f"rift net off {PRIVACY_APP} exited with {status}: {without_console(output).strip()[-200:]!r}")
            if not wait_for(20, lambda: (privacy_page("the page after rift net off") or ({}, {}, {}))[2].get(PRIVACY_APP) == "off"):
                fail(f"the Privacy page does not list {PRIVACY_APP} with its network off after rift net off: "
                     f"{privacy_page('the page once more')}")
            run(f"rift-settings --set app-network {PRIVACY_APP} on", f"{PRIVACY_APP}'s network on from the page")
            if not wait_for(20, lambda: every_app_online(f"{PRIVACY_APP}'s network given back")):
                fail(f"after the page gave {PRIVACY_APP} the network back, rift net and the page do not say every app has it")
            ok(f"the Privacy page names the camera's answers the permission store holds ({store_words}), {camera_turned}; "
               f"turned recent files off and on with dconf following, says the firewall is on as nftables.service is "
               f"active, gave {PRIVACY_APP} back the network rift net took, and gives fwupd's {privacy_id}")

            # the Owner page. Vault keeps the owner's own name and password on persist and puts them into
            # the password files at once and at every boot, so the page names who getent has, a new name
            # from it is in getent and on the lock screen, and a new password from it unlocks the lock
            # screen where the image's no longer does. then both go back to the image's
            def owner_page(what):
                """What the Owner page says once Vault has answered, and the problem it shows."""
                said = settings_state(what)
                return {key: said[key] for key in ("owner-user", "owner-name", "owner-password", "problem")
                        if key in said}

            def owner_getent(what):
                """The full name getent gives the owner's account."""
                _, told = run(f"getent passwd {OWNER_USER} | cut -d: -f5", what)
                return (without_console(told).strip().splitlines() or [""])[-1].strip()

            def owner_locked_for(what):
                """The name the lock screen last said it locked the screen for."""
                _, told = run("journalctl -b -t lock -o cat --no-pager | grep 'locking the screen for' | tail -n 1",
                              what)
                found = re.search(r"locking the screen for (.+?)\s*$", without_console(told), re.M)
                return found.group(1) if found else None

            def owner_lock(name, png, what):
                """Lock the session, wait for the lock screen to be up and to have read this name, and
                say how wide it draws the name."""
                point(args.qmp, size, (round(width / 3), round(height * 3 / 4)))
                status, told = run(f"loginctl lock-session {session}", f"loginctl lock-session {what}")
                if status != 0:
                    fail(f"loginctl lock-session {session} exited with {status}: {without_console(told).strip()!r}")
                look(f"the lock screen {what}", png, 30, lock=False, journals=("lock", "horizon"))
                locked_hint("yes", f"with the lock screen up {what}")
                if not wait_for(10, lambda: owner_locked_for(f"the name the lock screen read {what}") == name):
                    fail(f"the lock screen {what} says it locked the screen for "
                         f"{owner_locked_for('the name the lock screen read again')!r}, expected {name!r}")
                wide, tall, pixels = screendump(args.qmp, work, "owner-lock")
                return lock_name_width(wide, tall, pixels)

            def owner_is(name, password_word, what):
                """Whether the page and getent both have this name, and the page says the password
                is the image's or the owner's own."""
                said = owner_page(f"the page for {what}")
                return (said.get("owner-name") == name and said.get("owner-password") == password_word
                        and owner_getent(f"getent for {what}") == name)

            run("rift-settings --page owner", "the Owner page")
            if not wait_for(30, lambda: settings_state("the Owner page").get("page") == "owner"):
                fail("rift-settings --page owner did not show that page")
            owner_before = wait_for(30, lambda: next((said for said in [owner_page("the Owner page as it comes up")]
                                                      if said.get("owner-user")), None))
            if not owner_before:
                fail("the Owner page says nothing about the owner, and Vault is there to ask")
            getent_before = owner_getent("the owner's name in getent")
            if (owner_before.get("owner-user"), owner_before.get("owner-name"), owner_before.get("owner-password")) \
                    != (OWNER_USER, getent_before, "image") or getent_before != OWNER_NAME:
                fail(f"the Owner page says {owner_before}, and getent names the owner {getent_before!r}, expected "
                     f"{OWNER_USER} called {OWNER_NAME} with the image's password")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the Owner page", f"{stem}-settings-owner{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            owner_width_before = owner_lock(OWNER_NAME, f"{stem}-owner-lock{extension}", "with the image's name")
            type_line(PASSWORD, "the image's password")
            locked_hint("no", "after the image's password on the lock screen with the image's name")
            # a current password that is not the owner's changes nothing
            run(f"rift-settings --set owner-password {WRONG_PASSWORD} {OWNER_NEW_PASSWORD}",
                "a new password with a wrong current one")
            if not wait_for(20, lambda: owner_page("the page after a wrong current password").get("problem")
                            == "The current password is incorrect."):
                fail(f"after a wrong current password the Owner page says {owner_page('the page again')}")
            if owner_page("the password after a wrong current one").get("owner-password") != "image":
                fail("a wrong current password changed the owner's password")
            run(f"rift-settings --set owner-name {OWNER_NEW_NAME}", "a new name on the Owner page")
            if not wait_for(30, lambda: owner_is(OWNER_NEW_NAME, "image", "the new name")):
                fail(f"after the page set the name {OWNER_NEW_NAME!r} it says {owner_page('the page again')} and getent "
                     f"{owner_getent('getent again')!r}")
            run(f"rift-settings --set owner-password {PASSWORD} {OWNER_NEW_PASSWORD}", "a new password on the Owner page")
            if not wait_for(30, lambda: owner_is(OWNER_NEW_NAME, "own", "the new password")):
                fail(f"after the page set a new password it says {owner_page('the page again')}")
            status, told = run(f"sudo cat {OWNER_PASSWORD_FILE}; sudo grep '^{OWNER_USER}:' /etc/shadow | cut -d: -f2",
                               "the hash kept on persist and the one in the shadow file")
            owner_hashes = without_console(told).split()
            if status != 0 or len(owner_hashes) != 2 or owner_hashes[0] != owner_hashes[1] \
                    or not owner_hashes[0].startswith("$y$"):
                fail(f"{OWNER_PASSWORD_FILE} and the shadow file hold {owner_hashes}, expected one yescrypt hash in both")
            owner_width_after = owner_lock(OWNER_NEW_NAME, f"{stem}-owner-lock-new{extension}", "with the new name")
            if owner_width_after <= owner_width_before + 20:
                fail(f"the lock screen draws {OWNER_NEW_NAME!r} {owner_width_after} pixels wide and {OWNER_NAME!r} "
                     f"{owner_width_before}, see {stem}-owner-lock-new{extension}")
            type_line(PASSWORD, "the image's password, which is no longer the owner's")
            look("the lock screen refusing the image's password", f"{stem}-owner-lock-refused{extension}", 30,
                 lock=True, journals=("lock", "horizon"))
            locked_hint("yes", "after the image's password")
            type_line(OWNER_NEW_PASSWORD, "the owner's new password")
            locked_hint("no", "after the owner's new password")
            # and both back to the image's, which the rest of the test and the drive's next boots have
            run(f"rift-settings --set owner-name {OWNER_NAME}", "the image's name again")
            if not wait_for(30, lambda: owner_is(OWNER_NAME, "own", "the image's name again")):
                fail(f"after the page set the name back it says {owner_page('the page again')}")
            run(f"rift-settings --set owner-password {OWNER_NEW_PASSWORD} {PASSWORD}", "the image's password again")
            if not wait_for(30, lambda: owner_is(OWNER_NAME, "image", "the image's password again")):
                fail(f"after the page set the password back it says {owner_page('the page again')}")
            ok(f"the Owner page names {OWNER_USER} {OWNER_NAME!r} as getent does, refused a wrong current password, "
               f"set {OWNER_NEW_NAME!r} with getent following and the lock screen drawing it {owner_width_after} "
               f"pixels wide where the image's name was {owner_width_before}, and a new password that unlocked the "
               f"lock screen where the image's was refused, the same yescrypt hash on persist and in the shadow file; "
               f"then both back to the image's")

            # the About page, which reads os-release and asks Orbit about this machine
            run("rift-settings --page about", "the About page")
            if not wait_for(30, lambda: settings_state("the About page").get("page") == "about"):
                fail("rift-settings --page about did not show the About page")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{SETTINGS_APP} on the About page", f"{stem}-settings-about{extension}", 60,
                 apps=[SETTINGS_APP], journals=("horizon",), settle=3)
            run("rift-settings --page appearance", "the Appearance page again")
            status, output = run("rift-settings --page nowhere", "a page that is not one")
            if status == 0 or "there is no page called nowhere" not in without_console(output):
                fail(f"rift-settings --page nowhere exited with {status}: "
                     f"{without_console(output).strip()[-200:]!r}")
            ok(f"Settings shows the About page and every one of its {SETTINGS_PAGES} pages by name")
            close_app(SETTINGS_APP, SETTINGS_APP_ID)

            # 5n. Files, the file manager. the session makes the folders of home and the file that
            # names them for every app, and a folder opens in Files: xdg-open asks for the app of
            # inode/directory, which is Files now and was the disk usage analyzer before. the test
            # drives the window over its socket the way a person would with the pointer: a second
            # window from the command line, a folder from its dialog, a file copied and pasted into
            # it, a rename, the trash with its note and Undo, the trash's own view and emptying it, a
            # menu, and a photograph opened with the app that opens its kind, in a scope of its own
            def files_state(what):
                """What rift-files --state prints, a line each, or None while nothing answers."""
                status, output = run("rift-files --state", what)
                if status != 0:
                    return None
                return [line.strip() for line in without_console(output).splitlines() if line.strip()]

            def files_value(lines, key):
                """The rest of the first line that starts with key, or None."""
                for printed in lines or []:
                    if printed.startswith(key + " "):
                        return printed[len(key) + 1:]
                return None

            def files_until(seconds, ready, what):
                """Ask rift-files --state until ready(lines) is true, and answer those lines."""
                deadline = time.monotonic() + seconds
                while True:
                    lines = files_state(what)
                    if lines and ready(lines):
                        return lines
                    if time.monotonic() > deadline:
                        _, output = run("journalctl --user -b -o cat -n 40 | cat", "the user manager's log")
                        fail(f"Files did not come to {what} in {seconds} s, its state is {lines!r}; the user "
                             f"manager's log said {without_console(output).strip()[-800:]!r}"[:2400])
                    time.sleep(2)

            def files_set(name, value, what):
                """Press something in the window in front, over the socket."""
                status, output = run(f'rift-files --set {name} "{value}"', what)
                if status != 0:
                    fail(f"rift-files --set {name} exited with {status}: "
                         f"{without_console(output).strip()[-300:]!r}")

            def files_select(name):
                """Select one row by its name, pressing again while the folder has not been read with it
                in yet: a press on a row that is not there does nothing."""
                deadline = time.monotonic() + 30
                while True:
                    files_set("select", name, f"{name} selected")
                    time.sleep(1)
                    lines = files_state(f"{name} selected")
                    if lines and f"selected {name}" in lines:
                        return
                    if time.monotonic() > deadline:
                        fail(f"Files did not select {name}, its state is {lines!r}"[:2000])

            files_home = f"/home/{OWNER_USER}"
            files_documents = f"{files_home}/Documents"
            # one a line: on the serial console ls writes a terminal, and lays a list out in columns
            _, files_listed = run(" ".join(["ls", "-1d"] + [f"~/{name}" for name in FILES_FOLDERS])
                                  + "; cat ~/.config/user-dirs.dirs", "the folders of home")
            files_listed = without_console(files_listed)
            files_lines = [printed.strip() for printed in files_listed.splitlines()]
            files_missing = [name for name in FILES_FOLDERS if f"{files_home}/{name}" not in files_lines]
            if files_missing or 'XDG_DOWNLOAD_DIR="$HOME/Downloads"' not in files_listed:
                fail(f"home is missing {files_missing}, or the file that names its folders says "
                     f"{files_listed.strip()[-400:]!r}")
            ok(f"the session made {', '.join(FILES_FOLDERS)} in home, and the file that names them for every app")

            run(f"printf 'Minutes of the meeting\\n' > ~/Documents/{FILES_NOTE}", "a file for Files to copy")
            run(f"systemd-run --user --quiet --collect --unit={FILES_UNIT} -- xdg-open {files_documents}",
                "a folder opened the way another app opens one")
            files_until(120, lambda lines: files_value(lines, "location") == files_documents
                        and files_value(lines, "ready") == "yes" and f"row file {FILES_NOTE}" in lines,
                        "its window on Documents")
            if not wait_for(60, lambda: app_windows(FILES_APP_ID, "Files' window")):
                fail(f"rift-files answers on its socket and horizon lists no {FILES_APP_ID} window")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{FILES_APP} on Documents with its title bar", f"{stem}-files{extension}", 120,
                 apps=[FILES_APP], journals=("horizon",), settle=3)
            ok(f"xdg-open opened {files_documents} in Files, whose list has {FILES_NOTE} in it")

            run(f"rift-files {files_home}/Downloads", "a second window, from the command line")
            files_until(30, lambda lines: files_value(lines, "windows") == "2"
                        and files_value(lines, "location") == f"{files_home}/Downloads", "a second window on Downloads")
            files_set("close", "now", "closing the second window")
            files_until(30, lambda lines: files_value(lines, "windows") == "1"
                        and files_value(lines, "location") == files_documents, "one window again")
            ok("rift-files with Files running opened a second window in the same app, and it closed again")

            files_set("new-folder", FILES_FOLDER, "a new folder, named in its dialog")
            files_until(30, lambda lines: files_value(lines, "dialog") == "new-folder"
                        and files_value(lines, "dialog-name") == FILES_FOLDER, "the dialog for a new folder")
            shot(f"{stem}-files-new-folder{extension}", "files-new-folder")
            files_set("confirm", "now", "Create")
            files_until(30, lambda lines: f"row folder {FILES_FOLDER}" in lines and f"selected {FILES_FOLDER}" in lines
                        and files_value(lines, "dialog") == "none", "the new folder, selected")
            _, files_said = run(f"test -d ~/Documents/{FILES_FOLDER}; and echo made; or echo missing", "the new folder")
            if "made" not in without_console(files_said):
                fail(f"Files lists {FILES_FOLDER} and there is no such folder in Documents")

            files_select(FILES_NOTE)
            files_set("copy", "now", "Copy")
            files_until(20, lambda lines: files_value(lines, "clipboard") == "copy 1", "a file on the clipboard")
            files_set("activate", FILES_FOLDER, f"{FILES_FOLDER} opened")
            files_until(30, lambda lines: files_value(lines, "location") == f"{files_documents}/{FILES_FOLDER}"
                        and files_value(lines, "ready") == "yes", f"the window on {FILES_FOLDER}")
            files_set("paste", "now", "Paste")
            files_until(60, lambda lines: f"row file {FILES_NOTE}" in lines, f"{FILES_NOTE} pasted into {FILES_FOLDER}")
            _, files_said = run(f"cmp ~/Documents/{FILES_NOTE} ~/Documents/{FILES_FOLDER}/{FILES_NOTE}; "
                                "and echo same; or echo differ", "the copy against the original")
            if "same" not in without_console(files_said):
                fail(f"the copy of {FILES_NOTE} is not the same as the original: {without_console(files_said).strip()!r}")

            files_select(FILES_NOTE)
            files_set("rename", FILES_RENAMED, "a new name, typed in its dialog")
            files_until(30, lambda lines: files_value(lines, "dialog") == "rename"
                        and files_value(lines, "dialog-name") == FILES_RENAMED, "the dialog for a new name")
            files_set("confirm", "now", "Rename")
            files_until(30, lambda lines: f"row file {FILES_RENAMED}" in lines and f"row file {FILES_NOTE}" not in lines,
                        f"{FILES_NOTE} called {FILES_RENAMED}")
            ok(f"a folder made in its dialog, {FILES_NOTE} copied and pasted into it the same as the original, "
               f"and the copy renamed {FILES_RENAMED}")

            files_select(FILES_RENAMED)
            files_set("trash", "now", "Move to trash")
            files_until(30, lambda lines: f"row file {FILES_RENAMED}" not in lines and files_value(lines, "trash") == "full",
                        f"{FILES_RENAMED} in the trash")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            shot(f"{stem}-files-trashed{extension}", "files-trashed")
            _, files_note = run(f"cat ~/.local/share/Trash/info/{FILES_RENAMED}.trashinfo; "
                                f"ls ~/.local/share/Trash/files", "the trash's note")
            files_note = without_console(files_note)
            if (f"Path={files_documents}/{FILES_FOLDER}/{FILES_RENAMED}" not in files_note
                    or "DeletionDate=" not in files_note):
                fail(f"the trash's note for {FILES_RENAMED} says {files_note.strip()[-400:]!r}")
            files_set("undo", "now", "Undo")
            files_until(30, lambda lines: f"row file {FILES_RENAMED}" in lines and files_value(lines, "trash") == "empty",
                        f"{FILES_RENAMED} back from the trash")
            ok(f"{FILES_RENAMED} went into the trash with the note every GTK app writes, and Undo put it back")

            files_select(FILES_RENAMED)
            files_set("trash", "now", "Move to trash again")
            files_until(30, lambda lines: files_value(lines, "trash") == "full", f"{FILES_RENAMED} in the trash again")
            files_set("place", "trash", "the trash in the sidebar")
            files_until(30, lambda lines: files_value(lines, "location") == "trash" and
                        f"row file {FILES_RENAMED} from {files_documents}/{FILES_FOLDER}/{FILES_RENAMED}" in lines,
                        "the trash's own view")
            shot(f"{stem}-files-trash{extension}", "files-trash")
            files_set("empty", "now", "Empty trash")
            files_until(30, lambda lines: files_value(lines, "dialog") == "empty", "the question before the trash is emptied")
            shot(f"{stem}-files-empty{extension}", "files-empty")
            files_set("confirm", "now", "Empty trash")
            files_until(60, lambda lines: files_value(lines, "rows") == "0" and files_value(lines, "trash") == "empty",
                        "an empty trash")
            _, files_left = run("find ~/.local/share/Trash -mindepth 2 | wc -l", "what is left in the trash")
            if without_console(files_left).strip().splitlines()[-1:] != ["0"]:
                fail(f"the trash still holds {without_console(files_left).strip()!r} things after it was emptied")
            ok(f"the trash listed {FILES_RENAMED} with where it was, and emptying it left nothing behind")

            files_set("place", "documents", "Documents in the sidebar")
            files_until(30, lambda lines: files_value(lines, "location") == files_documents
                        and files_value(lines, "ready") == "yes", "Documents again")
            files_select(FILES_NOTE)
            files_set("menu", "selection", "the menu of the selection")
            files_until(30, lambda lines: files_value(lines, "menu") == "selection", "the menu of the selection")
            shot(f"{stem}-files-menu{extension}", "files-menu")
            files_set("escape", "now", "Escape")
            files_until(20, lambda lines: files_value(lines, "menu") == "none", "the menu closed")

            run(f"cp /run/current-system/sw/share/backgrounds/rift/{PICTURE}.jpg ~/Pictures/", "a photograph in Pictures")
            files_set("place", "pictures", "Pictures in the sidebar")
            files_until(30, lambda lines: files_value(lines, "location") == f"{files_home}/Pictures"
                        and f"row file {PICTURE}.jpg" in lines, "the photograph in Pictures")
            files_set("activate", f"{PICTURE}.jpg", "the photograph, pressed twice")
            if not wait_for(180, lambda: app_windows("loupe", "the image viewer's window")):
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"Files opened no image viewer for {PICTURE}.jpg: {without_console(output).strip()[-800:]!r}")
            _, files_scopes = run("systemctl --user list-units --type=scope --no-legend --plain | cat", "the apps' scopes")
            if "app-rift-org.gnome.Loupe-" not in without_console(files_scopes):
                fail(f"the image viewer Files opened is in no scope of its own: {without_console(files_scopes).strip()[-600:]!r}")
            close_app("the image viewer", "loupe")
            ok(f"{PICTURE}.jpg opened in the image viewer, the app for its kind, in a scope of its own")

            # 5o. the disks in the sidebar. the drive's own exchange partition is mounted by the
            # system, because every partition of the drive belongs to the drive; a memory stick is
            # mounted by udisks when the owner presses it, and never by itself. nothing of the
            # machine's own can be mounted at all, which 5j shows polkit refusing. then a file is
            # copied onto the stick, one on the stick goes to the stick's own trash, a second copy
            # asks before it replaces a name, and the stick is unmounted and ejected
            def drive_lines(lines):
                """The disks --state printed, by name."""
                found = {}
                for printed in lines or []:
                    if printed.startswith("drive "):
                        name, state, mount = printed[len("drive "):].rsplit(" ", 2)
                        found[name] = (state, mount)
                return found

            if args.exchange:
                _, drive_said = run(f"findmnt --noheadings --output SOURCE,FSTYPE {DRIVE_EXCHANGE}; "
                                    f"printf 'for another computer\\n' > {DRIVE_EXCHANGE}/{DRIVE_FILE}; "
                                    f"stat -c owner=%U {DRIVE_EXCHANGE}/{DRIVE_FILE}",
                                    "the exchange partition, and a file written on it")
                drive_said = without_console(drive_said)
                if "exfat" not in drive_said or f"owner={OWNER_USER}" not in drive_said:
                    fail(f"{DRIVE_EXCHANGE} is not the drive's exfat partition the owner can write to: "
                         f"{drive_said.strip()[-400:]!r}")
                files_set("place", "exchange", "the exchange partition in the sidebar")
                files_until(30, lambda lines: files_value(lines, "location") == DRIVE_EXCHANGE
                            and f"row file {DRIVE_FILE}" in lines, "the exchange partition's own folder")
                ok(f"the system mounted the drive's exchange partition at {DRIVE_EXCHANGE}, the owner wrote "
                   f"{DRIVE_FILE} on it, and Files lists it in the sidebar")

            if args.stick:
                drive_found = files_until(180, lambda lines: DRIVE_LABEL in drive_lines(lines),
                                          f"the {DRIVE_LABEL} disk in the sidebar")
                drive_listed = drive_lines(drive_found)
                if drive_listed[DRIVE_LABEL] != ("there", "none"):
                    fail(f"Files says {DRIVE_LABEL} is {drive_listed[DRIVE_LABEL]}, and nothing is mounted "
                         "by itself")
                drive_wanted = 2 if args.locked else 1
                if len(drive_listed) != drive_wanted:
                    fail(f"Files lists the disks {sorted(drive_listed)}, expected {drive_wanted} of them: only a "
                         "disk a person plugged in belongs there, and the backup disk of this machine is not one")
                files_set("drive", DRIVE_LABEL, f"{DRIVE_LABEL} in the sidebar")
                drive_found = files_until(120, lambda lines: drive_lines(lines).get(DRIVE_LABEL)
                                          == ("mounted", DRIVE_MOUNT)
                                          and files_value(lines, "location") == DRIVE_MOUNT
                                          and files_value(lines, "ready") == "yes",
                                          f"{DRIVE_LABEL} mounted and open")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                look(f"{FILES_APP} on the disk, with it in the sidebar", f"{stem}-files-drive{extension}", 120,
                     apps=[FILES_APP], journals=("horizon",), settle=3)
                ok(f"Files listed {DRIVE_LABEL} without mounting it, and mounted it at {DRIVE_MOUNT} when it "
                   "was pressed")

                files_set("place", "documents", "Documents in the sidebar")
                files_until(30, lambda lines: files_value(lines, "location") == files_documents
                            and f"row file {FILES_NOTE}" in lines, "Documents again")
                files_select(FILES_NOTE)
                files_set("copy", "now", "Copy")
                files_until(20, lambda lines: files_value(lines, "clipboard") == "copy 1", "a file on the clipboard")
                files_set("drive", DRIVE_LABEL, f"{DRIVE_LABEL} again")
                files_until(60, lambda lines: files_value(lines, "location") == DRIVE_MOUNT
                            and files_value(lines, "ready") == "yes", f"{DRIVE_LABEL} open again")
                files_set("paste", "now", "Paste onto the disk")
                files_until(120, lambda lines: f"row file {FILES_NOTE}" in lines,
                            f"{FILES_NOTE} on {DRIVE_LABEL}")
                _, drive_said = run(f"cmp ~/Documents/{FILES_NOTE} {DRIVE_MOUNT}/{FILES_NOTE}; "
                                    "and echo same; or echo differ", "the copy on the disk")
                if "same" not in without_console(drive_said):
                    fail(f"the copy of {FILES_NOTE} on {DRIVE_LABEL} differs: {without_console(drive_said).strip()!r}")
                ok(f"{FILES_NOTE} was copied from home onto {DRIVE_LABEL}")

                files_select(FILES_NOTE)
                files_set("trash", "now", "Move to trash on the disk")
                files_until(60, lambda lines: f"row file {FILES_NOTE}" not in lines
                            and files_value(lines, "trash") == "full", f"{FILES_NOTE} in the disk's trash")
                _, drive_said = run(f"ls -1 {DRIVE_MOUNT}/.Trash-1000/files; "
                                    f"cat {DRIVE_MOUNT}/.Trash-1000/info/{FILES_NOTE}.trashinfo",
                                    "the disk's own trash")
                drive_said = without_console(drive_said)
                # the note says where it was under the top of the disk, not where the disk is mounted
                if FILES_NOTE not in drive_said or f"Path={FILES_NOTE}" not in drive_said:
                    fail(f"the disk's own trash holds {drive_said.strip()[-400:]!r}")
                files_set("place", "trash", "the trash in the sidebar")
                files_until(30, lambda lines: files_value(lines, "location") == "trash"
                            and f"row file {FILES_NOTE} from {DRIVE_MOUNT}/{FILES_NOTE}" in lines,
                            "the disk's trash in the trash's own view")
                files_set("undo", "now", "Undo")
                files_until(60, lambda lines: files_value(lines, "rows") == "0", "the trash empty again")
                ok(f"{FILES_NOTE} went into {DRIVE_LABEL}'s own trash, listed there with where it was on the "
                   "disk, and Undo put it back")

                files_set("drive", DRIVE_LABEL, f"{DRIVE_LABEL} once more")
                files_until(60, lambda lines: files_value(lines, "location") == DRIVE_MOUNT
                            and f"row file {FILES_NOTE}" in lines, f"{FILES_NOTE} back on {DRIVE_LABEL}")
                files_set("paste", "now", "Paste the same name again")
                files_until(60, lambda lines: files_value(lines, "dialog") == "replace",
                            "the question before a name is replaced")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                shot(f"{stem}-files-replace{extension}", "files-replace")
                files_set("replace", "now", "Replace")
                files_until(120, lambda lines: files_value(lines, "toast") is not None
                            and "in the trash" in files_value(lines, "toast"),
                            "what the copy that replaced a name says")
                _, drive_said = run(f"ls -1 {DRIVE_MOUNT}/.Trash-1000/files", "the disk's trash after the replace")
                if FILES_NOTE not in without_console(drive_said):
                    fail(f"the name that was replaced is not in {DRIVE_LABEL}'s trash: "
                         f"{without_console(drive_said).strip()[-300:]!r}")
                ok("a copy onto a name that is taken asked first, and the one that was there went to the "
                   "disk's own trash")

                files_set("eject", DRIVE_LABEL, f"eject {DRIVE_LABEL}")
                files_until(120, lambda lines: drive_lines(lines).get(DRIVE_LABEL, ("gone", ""))[0] != "mounted",
                            f"{DRIVE_LABEL} unmounted")
                _, drive_said = run(f"findmnt --noheadings --output TARGET {DRIVE_MOUNT} | cat",
                                    "what is mounted where the disk was")
                if DRIVE_MOUNT in without_console(drive_said):
                    fail(f"{DRIVE_MOUNT} is still mounted after {DRIVE_LABEL} was ejected")
                ok(f"{DRIVE_LABEL} was unmounted and ejected from its row in the sidebar")

            # 5p. the Timeline and the search field. vault takes a snapshot of home every hour, and a
            # folder in the Timeline is that folder as it was at one of them: a file changed since is
            # brought back by copying it out of the snapshot, so what is there now goes to the trash
            # first and nothing is written over. then the field in the header bar: by name over the
            # folder and what is under it as it is typed, and by meaning through the index of step 4c
            def files_contents(path):
                """What is in a file, or nothing when it cannot be read."""
                status, said = run(f"cat {path}", f"what is in {path}")
                return without_console(said) if status == 0 else ""

            files_set("place", "documents", "Documents in the sidebar")
            files_until(30, lambda lines: files_value(lines, "location") == files_documents
                        and files_value(lines, "ready") == "yes", "Documents for the Timeline")
            status, _ = run(f"printf 'First draft\\n' > ~/Documents/{FILES_DIARY}; "
                            f"and printf 'Two coffees\\n' > ~/Documents/{FILES_RECEIPTS}",
                            "the files the Timeline brings back")
            if status != 0:
                fail("the files for the Timeline could not be written")
            status, files_took = run("rift snapshot take", "a snapshot of home for the Timeline")
            files_taken = re.search(r"^Took snapshot (\S+Z)\.\s*$", without_console(files_took), re.M)
            if status != 0 or not files_taken:
                fail(f"rift snapshot take exited with {status}: {without_console(files_took).strip()[-300:]!r}")
            files_moment = files_taken.group(1)
            status, _ = run(f"printf 'Second draft\\n' > ~/Documents/{FILES_DIARY}; "
                            f"and rm ~/Documents/{FILES_RECEIPTS}",
                            "one file changed and one deleted since the snapshot")
            if status != 0:
                fail("the files could not be changed after the snapshot")
            files_until(30, lambda lines: f"row file {FILES_RECEIPTS}" not in lines,
                        f"{FILES_RECEIPTS} gone from the list")
            # the snapshot keeps home's permissions, so the owner reads a folder in it as they do in home
            files_snapshot = f"/persist/@snapshots/home/{files_moment}/{OWNER_USER}/Documents"
            _, files_listed = run(f"ls -1 {files_snapshot} | cat", "the folder inside the snapshot")
            if FILES_RECEIPTS not in without_console(files_listed):
                fail(f"the owner cannot list {files_snapshot}: "
                     f"{without_console(files_listed).strip()[-300:]!r}")

            files_set("timeline", "now", "the Timeline")
            files_until(60, lambda lines: files_value(lines, "moment") == files_moment
                        and files_value(lines, "ready") == "yes"
                        and f"row file {FILES_DIARY}" in lines and f"row file {FILES_RECEIPTS}" in lines,
                        f"Documents as it was at {files_moment}")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            shot(f"{stem}-files-timeline{extension}", "files-timeline")
            ok(f"Files shows Documents as it was at {files_moment}, with {FILES_RECEIPTS} still in it")

            # the deleted file is nowhere now, so it comes back without a question
            files_select(FILES_RECEIPTS)
            files_set("bring", "now", "Restore, for the file that was deleted")
            if not wait_for(60, lambda: "Two coffees"
                            in files_contents(f"{files_home}/Documents/{FILES_RECEIPTS}")):
                fail(f"{FILES_RECEIPTS} did not come back from {files_moment}")
            # the changed one asks first, and Replace puts what is there in the trash
            files_select(FILES_DIARY)
            files_set("bring", "now", "Restore, for the file that changed")
            files_until(30, lambda lines: files_value(lines, "dialog") == "replace",
                        "the question before the file that changed is replaced")
            shot(f"{stem}-files-restore{extension}", "files-restore")
            files_set("replace", "now", "Replace")
            if not wait_for(60, lambda: "First draft"
                            in files_contents(f"{files_home}/Documents/{FILES_DIARY}")):
                fail(f"{FILES_DIARY} was not brought back from {files_moment}")
            _, files_trashed = run(f"cat ~/.local/share/Trash/files/{FILES_DIARY}", "the copy in the trash")
            if "Second draft" not in without_console(files_trashed):
                fail(f"the {FILES_DIARY} that was there is not in the trash: "
                     f"{without_console(files_trashed).strip()[-200:]!r}")
            files_set("now", "now", "Back to now")
            files_until(30, lambda lines: files_value(lines, "location") == files_documents
                        and files_value(lines, "moment") is None, "Documents as it is now")
            ok(f"{FILES_RECEIPTS} came back from the snapshot, {FILES_DIARY} after the question, and the "
               "copy that was there went to the trash")

            run(f"printf 'Older\\n' > ~/Documents/{FILES_FOLDER}/'{FILES_UNDER}'",
                "a file under another folder for the search")
            files_set("search", FILES_DIARY.removesuffix(".txt"), "the search field with a word in it")
            files_until(60, lambda lines: files_value(lines, "search-kind") == "name"
                        and f"row file {FILES_DIARY} in {FILES_DIARY}" in lines
                        and f"row file {FILES_UNDER} in {FILES_FOLDER}/{FILES_UNDER}" in lines,
                        "what the names found in Documents and under it")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            shot(f"{stem}-files-search{extension}", "files-search")
            ok(f"the search field found {FILES_DIARY} and {FILES_UNDER} by name, in Documents and under it")

            if args.models:
                # by meaning, through quasar's index of home from step 4c. it is an index of home, so
                # the search runs from home, and the file it finds shares no word with the question
                files_set("escape", "now", "Escape, which closes the field")
                files_set("place", "home", "Home in the sidebar")
                files_until(30, lambda lines: files_value(lines, "location") == files_home
                            and files_value(lines, "ready") == "yes", "Home for the search by meaning")
                files_set("meaning", FILES_MEANING, f"a search for {FILES_MEANING}")
                files_meaning = files_until(180, lambda lines: files_value(lines, "search-kind") == "meaning"
                                            and (files_value(lines, "search-problem") is not None
                                                 or any(line.startswith("row file ") for line in lines)),
                                            f"what the index found for {FILES_MEANING}")
                files_why = files_value(files_meaning, "search-problem")
                if files_why:
                    fail(f"the search by meaning said {files_why!r}, and step 4c indexed home")
                files_rows = [line for line in files_meaning if line.startswith("row ")]
                if not files_rows or not files_rows[0].endswith(f" in {FILES_FOUND}"):
                    fail(f"the search for {FILES_MEANING!r} put {files_rows[:1]} first, expected {FILES_FOUND}")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                shot(f"{stem}-files-meaning{extension}", "files-meaning")
                ok(f"the search for {FILES_MEANING!r} put {FILES_FOUND} first, by meaning, from the index "
                   "Quasar's model made of home")

                # the pdf of step 4c the same way: its text came out of the sandbox pdftotext ran
                # in, so the index holds its pages and the field finds it like any other file. no
                # name in home holds these words, so the list is empty by name and every row that
                # comes after is the answer to this search and not the one before it
                files_set("escape", "now", "Escape, which closes the field")
                files_until(30, lambda lines: files_value(lines, "search-kind") is None,
                            "the field closed before the next search")
                files_set("search", SEARCH_PDF_WORDS, f"the field with {SEARCH_PDF_WORDS} in it")
                files_until(60, lambda lines: files_value(lines, "search-kind") == "name"
                            and not any(line.startswith("row ") for line in lines),
                            "no name in home with those words in it")
                files_set("meaning", SEARCH_PDF_WORDS, f"a search for {SEARCH_PDF_WORDS}")
                files_pdf = files_until(180, lambda lines: files_value(lines, "search-kind") == "meaning"
                                        and (files_value(lines, "search-problem") is not None
                                             or any(line.startswith("row ") for line in lines)),
                                        f"what the index found for {SEARCH_PDF_WORDS}")
                files_pdf_why = files_value(files_pdf, "search-problem")
                if files_pdf_why:
                    fail(f"the search by meaning said {files_pdf_why!r}, and step 4c wrote {SEARCH_PDF}")
                files_pdf_rows = [line for line in files_pdf if line.startswith("row ")]
                if not files_pdf_rows or not files_pdf_rows[0].endswith(f" in notes/{SEARCH_PDF}"):
                    fail(f"the search for {SEARCH_PDF_WORDS!r} put {files_pdf_rows[:1]} first, "
                         f"expected notes/{SEARCH_PDF}")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                shot(f"{stem}-files-pdf{extension}", "files-pdf")
                ok(f"the search field put {SEARCH_PDF} first for {SEARCH_PDF_WORDS!r}, out of the pages "
                   "of the pdf")
                files_set("escape", "now", "Escape, which closes the field")

            # 5q. pictures. the grid is the other way to show a folder: a tile for each thing with
            # the small picture of a file that has one, made by the freedesktop thumbnailers the
            # image has and kept in the owner's own cache. then what is known about one file, in the
            # Properties dialog, and last the encrypted disk, which is unlocked with the passphrase
            # typed into its dialog and mounted and opened
            files_set("escape", "now", "Escape, in case a field is open")
            for picture_name in PICTURES:
                run(f"cp /run/current-system/sw/share/backgrounds/rift/{PICTURE}.jpg "
                    f"'{files_home}/Pictures/{picture_name}.jpg'", f"{picture_name}.jpg in Pictures")
            files_set("place", "pictures", "Pictures in the sidebar")
            files_until(60, lambda lines: files_value(lines, "location") == f"{files_home}/Pictures"
                        and all(f"row file {name}.jpg" in lines for name in PICTURES),
                        "the pictures in Pictures")
            files_set("view", "grid", "the grid")
            files_until(180, lambda lines: files_value(lines, "view") == "grid"
                        and int(files_value(lines, "thumbnails") or 0) >= len(PICTURES),
                        "a small picture for each of the pictures")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            look(f"{FILES_APP} showing Pictures as a grid", f"{stem}-files-grid{extension}", 120,
                 apps=[FILES_APP], journals=("horizon",), settle=3)
            # the cache is the freedesktop one: a png of the file's address in md5, readable by the
            # owner alone, with the address and the time of the file written into it
            _, picture_said = run("ls -1 ~/.cache/thumbnails/normal | wc -l; "
                                  "stat -c %a ~/.cache/thumbnails/normal/*.png | sort -u | head -2; "
                                  "grep -al 'Thumb::URI' ~/.cache/thumbnails/normal/*.png | wc -l",
                                  "the pictures in the cache")
            picture_said = [line.strip() for line in without_console(picture_said).splitlines() if line.strip()]
            picture_made = [line for line in picture_said if line.isdigit()]
            if len(picture_made) < 2 or int(picture_made[0]) < len(PICTURES) \
                    or int(picture_made[-1]) < len(PICTURES) or "600" not in picture_said:
                fail(f"~/.cache/thumbnails/normal holds {picture_said!r}, expected at least {len(PICTURES)} "
                     "pngs with Thumb::URI in them, readable by the owner alone")
            ok(f"the grid drew a small picture of each of {len(PICTURES) + 1} photographs, made by the image's own "
               f"thumbnailers and kept as {picture_made[0]} pngs in ~/.cache/thumbnails/normal")

            files_select(f"{PICTURES[0]}.jpg")
            files_set("properties", "now", "Properties")
            picture_lines = files_until(60, lambda lines: files_value(lines, "dialog") == "properties"
                                        and files_value(lines, "property pixels") is not None,
                                        f"what is known about {PICTURES[0]}.jpg")
            point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
            shot(f"{stem}-files-properties{extension}", "files-properties")
            picture_facts = {key: files_value(picture_lines, f"property {key}")
                             for key in ("name", "kind", "size", "where", "changed", "pixels", "permissions")}
            picture_wide = re.match(r"^(\d+) by (\d+)$", picture_facts["pixels"] or "")
            if picture_facts["name"] != f"{PICTURES[0]}.jpg" or picture_facts["kind"] != "image/jpeg" \
                    or picture_facts["where"] != "Pictures" or not picture_wide \
                    or int(picture_wide.group(1)) < 100 or not picture_facts["permissions"] \
                    or not picture_facts["changed"] or not picture_facts["size"]:
                fail(f"Properties says {picture_facts!r} about {PICTURES[0]}.jpg")
            files_set("escape", "now", "Close")
            files_until(30, lambda lines: files_value(lines, "dialog") == "none", "the dialog closed")
            files_set("view", "list", "the list again")
            files_until(30, lambda lines: files_value(lines, "view") == "list", "the list again")
            ok(f"Properties says {PICTURES[0]}.jpg is a {picture_facts['kind']} of {picture_facts['size']}, "
               f"{picture_facts['pixels']} pixels, in {picture_facts['where']}, changed {picture_facts['changed']}, "
               f"{picture_facts['permissions']}")

            if args.locked:
                def locked_disks(lines):
                    """The disks Files says are locked, by name."""
                    return [name for name, (state, _) in drive_lines(lines).items() if state == "locked"]

                locked_found = files_until(180, lambda lines: len(locked_disks(lines)) == 1,
                                           "the encrypted disk in the sidebar, locked")
                locked_name = locked_disks(locked_found)[0]
                if drive_lines(locked_found)[locked_name][1] != "none":
                    fail(f"Files says the encrypted disk is {drive_lines(locked_found)[locked_name]}, expected "
                         "locked and mounted nowhere")
                files_set("unlock", locked_name, f"a press on {locked_name}")
                files_until(30, lambda lines: files_value(lines, "dialog") == "unlock",
                            "the dialog that asks for the passphrase")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                shot(f"{stem}-files-unlock{extension}", "files-unlock")
                files_set("type", DRIVE_WRONG, "a passphrase that does not open it")
                files_set("confirm", "now", "Unlock")
                files_until(120, lambda lines: files_value(lines, "dialog") == "unlock"
                            and files_value(lines, "dialog-problem") is not None,
                            "what a wrong passphrase says")
                files_set("type", DRIVE_SECRET, "the passphrase")
                files_set("confirm", "now", "Unlock again")
                files_until(180, lambda lines: files_value(lines, "location") == DRIVE_INSIDE_MOUNT
                            and files_value(lines, "ready") == "yes"
                            and files_value(lines, "dialog") == "none",
                            f"{DRIVE_INSIDE} unlocked, mounted and open")
                point(args.qmp, size, (width - round(60 * scale), height - dock_rows - round(60 * scale)))
                look(f"{FILES_APP} on the disk it unlocked", f"{stem}-files-unlocked{extension}", 120,
                     apps=[FILES_APP], journals=("horizon",), settle=3)
                _, locked_said = run(f"findmnt --noheadings --output SOURCE,FSTYPE {DRIVE_INSIDE_MOUNT}; "
                                     f"cat '{DRIVE_INSIDE_MOUNT}/{DRIVE_KEPT}'", "what came out of the locked disk")
                locked_said = without_console(locked_said)
                if "/dev/mapper/" not in locked_said or "the beach house" not in locked_said:
                    fail(f"{DRIVE_INSIDE_MOUNT} is not the file system out of the locked disk: "
                         f"{locked_said.strip()[-400:]!r}")
                ok(f"{locked_name} was listed locked and mounted nowhere, refused a wrong passphrase, and with the "
                   f"right one udisks unlocked it and mounted {DRIVE_INSIDE} at {DRIVE_INSIDE_MOUNT}, which Files "
                   f"opened with {DRIVE_KEPT} in it")

            files_set("close", "now", "Files' close button")
            if not wait_for(60, lambda: not app_windows(FILES_APP_ID, "Files' window after it closed")):
                fail("Files' window did not close")
            _, files_unit = run(f"systemctl --user is-active {FILES_UNIT} | cat", "the unit xdg-open ran in")
            if without_console(files_unit).strip().splitlines()[-1:] == ["active"]:
                fail("xdg-open's unit is still active after the last window of Files closed")
            ok("the last window of Files closed, and with it the app and the xdg-open that started it")

            # 5r. the shell. Files is one of the apps the dock keeps, so it is there from the first
            # boot; the trash and a mounted disk stand at the right end of the dock while there is
            # something to open, a press opens either in Files and a disk's own menu ejects it; the
            # places are in a section of their own over the apps in the Applications menu, and
            # pressing one opens it in Files; and the name every file manager answers to on the
            # session bus, org.freedesktop.FileManager1, shows a file in the folder it is in, which
            # is what the portal's OpenDirectory and a browser's Show in folder ask for. Files is
            # not running here, so the first call has to start it
            def shell_state(what):
                """What lens says, with the places at the right end of the dock as a list of keys."""
                said = bar_state(what)
                said["dock-place-keys"] = [word.split(":", 1)[0]
                                           for word in (said.get("dock-places") or "").split()]
                return said

            def shell_until(seconds, ready, what):
                """The shell's state once it fits, or what it was when the wait ran out."""
                until = time.monotonic() + seconds
                while True:
                    said = shell_state(what)
                    if ready(said) or time.monotonic() > until:
                        return said
                    time.sleep(2)

            def shell_place_left(key, said):
                """Where the left edge of a place at the right end of the dock is, in the shell's
                own pixels: the workspaces stand at the very right, the places before them."""
                keys = said["dock-place-keys"]
                spaces = len((said.get("workspaces") or "").split())
                wide = spaces * DOCK_SPACE + max(spaces - 1, 0) * DOCK_SPACE_GAP
                places = len(keys) * DOCK_ITEM + (len(keys) - 1) * DOCK_GAP
                first = shell_logical - DOCK_PAD - wide - DOCK_BETWEEN - places
                return first + keys.index(key) * (DOCK_ITEM + DOCK_GAP)

            def shell_place_point(key, said):
                """Where the middle of that place is on screen."""
                left = shell_place_left(key, said) + DOCK_ITEM / 2
                return round(left * shell_scale), round(shell_height - shell_dock / 2)

            def shell_place_menu(key, said, row, rows):
                """Where the middle of a row of the menu a right click on a place opens is. The menu
                stands on the dock with its left edge where the place is, moved left when it would
                run off the screen."""
                left = min(shell_place_left(key, said), shell_logical - DOCK_MENU_WIDTH)
                top = shell_height - shell_dock - (2 * DOCK_MENU_PAD + rows * DOCK_MENU_ROW) * shell_scale
                return (round((left + DOCK_MENU_WIDTH / 2) * shell_scale),
                        round(top + (DOCK_MENU_PAD + (row + 0.5) * DOCK_MENU_ROW) * shell_scale))

            def shell_menu_row(row):
                """Where the middle of a row of the Applications menu is on screen. The menu hangs
                under the bar with its left edge a margin in, the field at the top of it."""
                middle = (MENU_PAD + MENU_WIDTH / 2) * shell_scale
                down = (MENU_PAD + FIELD_SIZE[1] + MENU_GAP + (row + 0.5) * ROW_HEIGHT) * shell_scale
                return round(middle), round(shell_bar + down)

            shell_width, shell_height, shell_rgb = screendump(args.qmp, work, "shell")
            shell_size = (shell_width, shell_height)
            shell_bar, shell_dock = bar_and_dock(shell_width, shell_height,
                                                 bar_gray_rows(shell_width, shell_height, shell_rgb))
            shell_scale = shell_dock / DOCK_HEIGHT if shell_dock else 1
            shell_logical = shell_width / shell_scale

            # the bus starts Files for the call, since nothing of ours is running: ShowItems opens
            # the folder the file is in with the file selected, which is what a browser's Show in
            # folder does
            shell_document = f"{files_documents}/{FILES_NOTE}"
            shell_call = ("busctl --user call org.freedesktop.FileManager1 /org/freedesktop/FileManager1 "
                          "org.freedesktop.FileManager1")
            run(f"printf 'Minutes of the meeting\\n' > {shell_document}", "the file the bus call names")

            def shell_show(method, what):
                """One call to FileManager1 on the session bus. A call that starts Files is tried
                again: a cold start in a virtual machine can take longer than the bus waits for the
                app to take the name, and the app is running by the second try."""
                for attempt in (1, 2):
                    status, output = run(f"{shell_call} {method} ass 1 file://{shell_document} ''",
                                         f"{what}, try {attempt}")
                    if status == 0:
                        return
                    print(f"\nboot-test: the {method} call exited with {status}: "
                          f"{without_console(output).strip()[-300:]!r}", flush=True)
                    time.sleep(20)
                _, journal = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"the {method} call on the session bus did not go through; the user manager's "
                     f"log said {without_console(journal).strip()[-800:]!r}"[:2400])

            shell_show("ShowItems", "ShowItems on the session bus, with Files not running")
            files_until(120, lambda lines: files_value(lines, "location") == files_documents
                        and f"selected {FILES_NOTE}" in lines, f"{FILES_NOTE} shown in its folder")
            point(args.qmp, shell_size, (shell_width - round(60 * shell_scale),
                                         shell_height - shell_dock - round(60 * shell_scale)))
            look(f"{FILES_APP} on the file the bus asked for", f"{stem}-files-shown{extension}", 120,
                 apps=[FILES_APP], journals=("horizon",), settle=3)
            ok(f"a ShowItems call on the session bus started Files and showed {FILES_NOTE} in "
               f"{files_documents}, selected")

            # and ShowItemProperties opens a window with what is known about the file over it
            shell_show("ShowItemProperties", "ShowItemProperties on the session bus")
            shell_facts = files_until(120, lambda lines: files_value(lines, "dialog") == "properties"
                                      and files_value(lines, "property name") == FILES_NOTE,
                                      f"what is known about {FILES_NOTE}")
            if files_value(shell_facts, "property where") != "Documents":
                fail(f"Properties says {FILES_NOTE} is in "
                     f"{files_value(shell_facts, 'property where')!r}, expected Documents")
            files_set("escape", "now", "Close")
            files_until(30, lambda lines: files_value(lines, "dialog") == "none", "the dialog closed")
            files_set("close", "now", "the window ShowItemProperties opened")
            files_until(30, lambda lines: files_value(lines, "windows") == "1", "one window again")
            ok("ShowItemProperties opened what is known about the same file")

            # the dock keeps Files, so it is in it with the window that is open
            shell_said = shell_until(30, lambda said: "dev.rift.Files:1" in (said.get("dock") or ""),
                                     "Files in the dock with its window")
            if "dev.rift.Files:1" not in (shell_said.get("dock") or ""):
                fail(f"the dock says {shell_said.get('dock')!r}, expected Files with one window")
            ok(f"the dock lists {shell_said.get('dock')}")

            # the trash stands at the right end while there is something in it
            files_select(FILES_NOTE)
            files_set("trash", "now", "Move to trash")
            files_until(30, lambda lines: files_value(lines, "trash") == "full", f"{FILES_NOTE} in the trash")
            shell_said = shell_until(30, lambda said: "trash" in said["dock-place-keys"],
                                     "the trash at the right end of the dock")
            if "trash" not in shell_said["dock-place-keys"]:
                fail(f"the dock's places are {shell_said.get('dock-places')!r}, expected the trash in them")
            if args.locked and DRIVE_INSIDE_MOUNT not in shell_said["dock-place-keys"]:
                fail(f"the dock's places are {shell_said.get('dock-places')!r}, expected the disk "
                     f"that was unlocked, mounted at {DRIVE_INSIDE_MOUNT}")
            shot(f"{stem}-dock-places{extension}", "dock-places")
            # and a press on it opens the trash in Files
            click(args.qmp, shell_size, shell_place_point("trash", shell_said))
            # a row of the trash says where the file was, the way 5n reads them
            files_until(60, lambda lines: files_value(lines, "location") == "trash"
                        and f"row file {FILES_NOTE} from {shell_document}" in lines,
                        "the trash open in Files")
            ok(f"the trash stood at the right end of the dock with {FILES_NOTE} in it, and a press "
               "opened it in Files")
            # emptying it takes the place away again, since there is nothing left to open
            files_set("empty", "now", "Empty trash")
            files_until(30, lambda lines: files_value(lines, "dialog") == "empty", "the question")
            files_set("confirm", "now", "Empty trash")
            files_until(60, lambda lines: files_value(lines, "trash") == "empty", "an empty trash")
            shell_said = shell_until(30, lambda said: "trash" not in said["dock-place-keys"],
                                     "the dock without the trash")
            if "trash" in shell_said["dock-place-keys"]:
                fail(f"the dock still has the trash in it after it was emptied: {shell_said.get('dock-places')!r}")
            files_set("close", "now", "the window on the trash")
            files_until(30, lambda lines: files_value(lines, "windows") == "1", "one window again")
            ok("emptying the trash took its place in the dock away")

            if args.locked:
                # the disk that was unlocked in 5q is mounted, so it is a place too: a press opens
                # it, and Eject in its own menu unmounts it, closes it and switches it off
                shell_said = shell_state("the dock with the disk in it")
                click(args.qmp, shell_size, shell_place_point(DRIVE_INSIDE_MOUNT, shell_said))
                files_until(60, lambda lines: files_value(lines, "location") == DRIVE_INSIDE_MOUNT
                            and files_value(lines, "ready") == "yes", f"{DRIVE_INSIDE} open in Files")
                files_set("close", "now", "the window on the disk")
                files_until(30, lambda lines: files_value(lines, "windows") == "1", "one window again")
                click(args.qmp, shell_size, shell_place_point(DRIVE_INSIDE_MOUNT, shell_said), button="right")
                shell_item = wait_for(20, lambda: bar_state("the state with the place's menu open").get("item"))
                if not shell_item or not shell_item.startswith(f"{DRIVE_INSIDE_MOUNT} "):
                    fail(f"a right click on the disk in the dock says item {shell_item!r}")
                shell_rows = int(shell_item.split()[-1])
                if shell_rows != 2:
                    fail(f"the menu of the disk has {shell_rows} rows, expected Open and Eject")
                shot(f"{stem}-dock-place-menu{extension}", "dock-place-menu")
                click(args.qmp, shell_size, shell_place_menu(DRIVE_INSIDE_MOUNT, shell_said, 1, shell_rows))
                shell_said = shell_until(180, lambda said: DRIVE_INSIDE_MOUNT not in said["dock-place-keys"],
                                         "the dock after the disk was ejected")
                if DRIVE_INSIDE_MOUNT in shell_said["dock-place-keys"]:
                    fail(f"the disk is still in the dock after Eject: {shell_said.get('dock-places')!r}")
                _, shell_mounted = run(f"findmnt --noheadings --output TARGET {DRIVE_INSIDE_MOUNT}; "
                                       "lsblk --noheadings --output NAME,TYPE | grep crypt; true",
                                       "what is left of the disk that was ejected")
                if DRIVE_INSIDE_MOUNT in without_console(shell_mounted):
                    fail(f"{DRIVE_INSIDE_MOUNT} is still mounted after Eject in the dock")
                ok("the disk that was unlocked stood in the dock, a press opened it in Files, and "
                   "Eject in its menu unmounted it and shut it")

            # the places in the Applications menu, over the apps, and a press opens one in Files.
            # the last window closes first, so the desktop behind the menu is the desktop, and the
            # press starts Files again the way the call on the bus did
            files_set("close", "now", "the last window before the menu")
            if not wait_for(60, lambda: not app_windows(FILES_APP_ID, "Files' windows before the menu")):
                fail("Files' windows did not close")
            run("lens --menu", "the Applications menu with the places in it")
            shell_said = shell_until(20, lambda said: said.get("menu") == "open"
                                     and "home" in (said.get("places") or "").split(), "the menu open")
            shell_places = (shell_said.get("places") or "").split()
            if "home" not in shell_places or "documents" not in shell_places:
                fail(f"the Applications menu lists the places {shell_places}, expected home and the "
                     "folders of it")
            # the drive of this test has an exchange partition, which 5o found mounted
            if "exchange" not in shell_places:
                fail(f"the Applications menu lists the places {shell_places}, expected the drive's "
                     f"own exchange partition, mounted at {DRIVE_EXCHANGE}, with them")
            look("the Applications menu with the places over the apps", f"{stem}-menu-places{extension}",
                 20, menu=True, rows=int(shell_said.get("rows") or 0), journals=("lens",))
            # the first row is the name of the section and the second is home
            click(args.qmp, shell_size, shell_menu_row(1))
            files_until(180, lambda lines: files_value(lines, "windows") == "1"
                        and files_value(lines, "location") == files_home
                        and files_value(lines, "ready") == "yes", "home open in Files")
            if bar_state("the state after the place was pressed").get("menu") != "closed":
                fail("the Applications menu is still open after a place was pressed")
            ok(f"the Applications menu lists {len(shell_places)} places over the apps, and a press "
               "on home opened it in Files")

            files_set("close", "now", "the window on home")
            if not wait_for(60, lambda: not app_windows(FILES_APP_ID, "Files' windows at the end")):
                fail("Files' window did not close")
            ok("the shell opens every place there is in Files, and Files closed again")

            # and the field looks through home by meaning. plain words that are no app, no command
            # and no question bring up the files of the index step 4c made, a moment after the
            # typing stops. the pdf is the one that proves the whole way through: its text came out
            # of the sandbox, and none of the words typed are in its name
            # the pointer is still on the row the place was pressed on, and a row under it is drawn
            # in its own gray, so it goes onto the desktop beside the menu before the menu opens
            point(args.qmp, shell_size, (round(shell_width * 0.8), round(shell_height * 0.5)))
            run("lens --menu", "the Applications menu for a search of home")
            run(f'lens --type "{SEARCH_PDF_WORDS}"', f"{SEARCH_PDF_WORDS!r} typed into the field")
            found_said = shell_until(90, lambda said: said.get("found") not in (None, "", "none"),
                                     "the files the field found")
            found_files = (found_said.get("found") or "").split()
            if found_files[:1] != [f"notes/{SEARCH_PDF}"]:
                _, journal = run("journalctl --user -u lens -b -o cat -n 20 | cat", "the shell's log")
                fail(f"the field found {found_files} for {SEARCH_PDF_WORDS!r}, expected "
                     f"notes/{SEARCH_PDF} first: {without_console(journal).strip()[-400:]!r}")
            if found_said.get("field") != SEARCH_PDF_WORDS:
                fail(f"the field says {found_said.get('field')!r} with the files under it")
            look("the files the field found by meaning", f"{stem}-menu-found{extension}", 20,
                 menu=True, rows=int(found_said.get("rows") or 0), journals=("lens",))
            ok(f"the field found {found_files} in home for {SEARCH_PDF_WORDS!r}, closest first")

            # the first row is the name of the section and the second is the closest file, which
            # opens with the app its kind opens with, in a scope of its own, the way a place does
            click(args.qmp, shell_size, shell_menu_row(1))
            if not wait_for(180, lambda: app_windows(SEARCH_PDF_APP, f"the window for {SEARCH_PDF}")):
                _, journal = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"a press on {SEARCH_PDF} opened no {SEARCH_PDF_APP} window: "
                     f"{without_console(journal).strip()[-800:]!r}")
            if bar_state("the state after the file was pressed").get("menu") != "closed":
                fail("the Applications menu is still open after a file was pressed")
            # a picture with a window in it, so the desktop is no longer what covers the screen and
            # there is nothing to check but the window horizon already listed. it gets a moment to
            # draw its first page first
            time.sleep(2)
            shot(f"{stem}-menu-opened{extension}", "menu-opened")
            close_app(SEARCH_PDF, SEARCH_PDF_APP)
            ok(f"a press on {SEARCH_PDF} opened it with the app for its kind and closed the menu")

            # 5s. push to talk, which is P2.6 part 3. a key starts a recording, the model on the
            # drive writes down what was said, the words go in the field and Enter follows them,
            # and the answer to a question asked out loud is read back out loud. this machine has
            # no microphone, so one is made out of pipewire: a loopback whose capture side is a
            # sink and whose playback side is a source, the source made the default, and the wav
            # the voice writes played into the sink while the shell is listening
            if args.models:
                run(f"systemd-run --user --quiet --collect --unit={MIC_UNIT} pw-loopback "
                    f"--capture-props='media.class=Audio/Sink node.name={MIC_SINK} node.description={MIC_SINK}' "
                    f"--playback-props='media.class=Audio/Source node.name={MIC_SOURCE} node.description={MIC_SOURCE}'",
                    "a microphone made out of a pipewire loopback")

                def mic_id():
                    """The id wireplumber gives the loopback's source, once it is there."""
                    _, listed = run("wpctl status | cat", "what wireplumber lists")
                    found = re.search(rf"(\d+)\.\s+{re.escape(MIC_SOURCE)}\b", without_console(listed))
                    return found.group(1) if found else None

                talk_id = wait_for(60, mic_id)
                if not talk_id:
                    _, listed = run("wpctl status | cat", "what wireplumber lists in the end")
                    _, journal = run(f"journalctl --user -u {MIC_UNIT} -b -o cat -n 20 | cat", "what it said")
                    fail(f"the loopback made no source called {MIC_SOURCE}: "
                         f"{without_console(listed).strip()[-600:]!r} {without_console(journal).strip()[-300:]!r}")
                status, _ = run(f"wpctl set-default {talk_id}", "the loopback as the machine's microphone")
                if status != 0:
                    fail(f"wpctl would not make {talk_id} the default source")
                ok(f"a pipewire loopback stands in for a microphone: {MIC_SOURCE} is source {talk_id}")

                # the pointer goes onto the desktop first: the menu opens under it otherwise and a
                # row under the pointer is drawn in its own gray
                point(args.qmp, shell_size, (round(shell_width * 0.8), round(shell_height * 0.5)))

                # a room with nobody in it is not a sentence. the shell records digital silence and
                # says so, and nothing is asked of quasar and nothing is typed in the field
                run("lens --listen", "the key for push to talk, with nothing to hear")
                talk_said = shell_until(20, lambda said: said.get("listening") == "on",
                                        "the shell listening")
                if talk_said.get("listening") != "on" or talk_said.get("menu") != "open":
                    fail(f"the key left the shell listening {talk_said.get('listening')!r} with the "
                         f"menu {talk_said.get('menu')!r}")
                run("lens --listen", "the key again, which stops it")
                talk_said = shell_until(90, lambda said: said.get("listening") == "off"
                                        and said.get("notice", "").startswith("Nothing"),
                                        "what the shell heard in a quiet room")
                if talk_said.get("listening") != "off" or not talk_said.get("notice", "").startswith("Nothing"):
                    fail(f"the shell says listening {talk_said.get('listening')!r} and "
                         f"{talk_said.get('notice')!r} about a recording with nothing in it")
                if talk_said.get("field"):
                    fail(f"the field holds {talk_said.get('field')!r} after a recording with nothing in it")
                ok("the key started and stopped the shell listening, and a recording with nothing in "
                   f"it left the field empty: {talk_said.get('notice')!r}")

                # nothing typed has ever been read out loud: 5g asked this same question in the
                # field and the answer only ever went on the screen. a question asked out loud is
                # what the voice answers, which is the whole rule
                if talk_said.get("said") != "none":
                    fail(f"the shell has read {talk_said.get('said')!r} out loud without being "
                         "asked out loud")

                # and now the question, said out loud by the voice and played into the microphone
                # while the shell listens. the words come back into the field, Enter follows them,
                # and quasar's answer goes to the speakers as well as under the field
                talk_wav = "/home/rift/asked.wav"
                status, printed = run(f'rift ai say --wav {talk_wav} "{QUESTION}"', "the question out loud")
                if status != 0 or "Wrote" not in without_console(printed):
                    fail(f"rift ai say wrote no wav to ask with: {without_console(printed).strip()[-300:]!r}")
                run("lens --listen", "the key for push to talk")
                talk_said = shell_until(20, lambda said: said.get("listening") == "on", "the shell listening")
                if talk_said.get("listening") != "on":
                    fail("the key did not start the shell listening for the question")
                status, printed = run(f"pw-play --target {MIC_SINK} {talk_wav}", "the question into the microphone")
                if status != 0:
                    fail(f"the question would not play into {MIC_SINK}: "
                         f"{without_console(printed).strip()[-300:]!r}")
                run("lens --listen", "the key again, which stops it")
                talk_said = shell_until(args.answer_timeout,
                                        lambda said: said.get("said") not in (None, "", "none"),
                                        "the answer the shell read out loud")
                talk_heard = talk_said.get("field") or ""
                talk_spoken = re.findall(r"[a-z]+", QUESTION.lower())
                talk_shared = [word for word in talk_spoken if word in re.findall(r"[a-z]+", talk_heard.lower())]
                print(f"\nboot-test: the field holds {talk_heard!r} and the shell said "
                      f"{talk_said.get('said')!r}", flush=True)
                if len(talk_shared) < 0.7 * len(talk_spoken):
                    _, journal = run("journalctl --user -u lens -b -o cat -n 30 | cat", "the shell's log")
                    fail(f"the field holds {talk_heard!r} of {QUESTION!r}: {len(talk_shared)} of its "
                         f"{len(talk_spoken)} words: {without_console(journal).strip()[-600:]!r}")
                if int(talk_said.get("rows") or 0) < 1:
                    fail(f"the shell heard {talk_heard!r} and put no answer under the field")
                if talk_said.get("said") in (None, "", "none"):
                    _, journal = run("journalctl --user -u lens -b -o cat -n 30 | cat", "the shell's log")
                    fail(f"the shell did not read the answer out loud: "
                         f"{without_console(journal).strip()[-600:]!r}")
                look("the words the shell heard, with the answer under them",
                     f"{stem}-menu-heard{extension}", 20, menu=True,
                     rows=int(talk_said.get("rows") or 0), journals=("lens",))
                ok(f"the shell heard {len(talk_shared)} of the question's {len(talk_spoken)} words "
                   f"({talk_heard!r}), answered it in {talk_said.get('rows')} rows and read the "
                   f"answer out loud: {talk_said.get('said')!r}")

                run("lens --escape", "escape after the answer")
                run("lens --escape", "escape again, which closes the menu")
                run(f"rm -f {talk_wav}", "the wav the question was said into")
                run(f"systemctl --user stop {MIC_UNIT}", "the loopback that stood in for a microphone")

            # 5t. the session journal, which is P2.8 part 1. the shell writes down what is open as
            # windows open, move and close: for each window the desktop entry that opened it, the
            # workspace and the screen it is on, and where it stands in the scrolling layout. it is
            # the half of teleport that remembers; opening the apps again is part 2. the entry is
            # the one thing the compositor cannot say, so the step opens a window each of the two
            # ways an app is started here: from the Applications menu, which gives it a scope of the
            # shell's named after its entry, and by the compositor itself, which gives it no scope
            # of a session's at all and leaves the app id its window carries to say what it is
            def session_windows(what):
                """The windows the session journal names, each as a dict of its lines. The lines of
                a window go in a fixed order, so a word that comes round again starts the next
                window, which is what says where one ends when a journal line lands in the blank
                line between two."""
                status, output = run(f"cat {SESSION_JOURNAL}", what)
                if status != 0:
                    return []
                windows, win = [], {}
                for printed_line in without_console(output).splitlines():
                    printed_line = printed_line.strip()
                    if printed_line.startswith("#"):
                        continue
                    if not printed_line:
                        if win:
                            windows.append(win)
                        win = {}
                        continue
                    key, _, value = printed_line.partition(" ")
                    if key not in SESSION_KEYS:
                        continue
                    if key in win:
                        windows.append(win)
                        win = {}
                    win[key] = value.strip()
                if win:
                    windows.append(win)
                return windows

            def session_of(app_id, windows):
                """The journal's paragraph for the window whose app id has this in it."""
                return next((win for win in windows
                             if app_id in (win.get("window") or "").lower()), None)

            def session_until(seconds, ready, what):
                """The journal once it fits, or what it said when the wait ran out."""
                until = time.monotonic() + seconds
                while True:
                    windows = session_windows(what)
                    if ready(windows) or time.monotonic() > until:
                        return windows
                    time.sleep(2)

            def session_entry(scope):
                """The desktop entry the name of a scope of the shell's names, with the escapes a
                unit name needs taken back out, the way crates/librift/src/session.rs reads it."""
                found = re.fullmatch(r"app-rift-(.+)-\d+\.scope", scope or "")
                if not found:
                    return None
                return re.sub(r"\\x([0-9a-f]{2})",
                              lambda hexed: chr(int(hexed.group(1), 16)), found.group(1))

            # a terminal the compositor spawned itself, which is what Super and T does. the
            # compositor only puts what it spawns in a scope of its own when it is a notify unit
            # itself, and greetd starts it without a notify socket, so this one is in no scope of a
            # session's and the entry has to be found from the app id its window carries
            session_already = {win[0] for win in app_windows(MENU_APP_ID, "the terminals open now")}
            run("horizon msg action spawn -- ghostty", "a terminal started the way a key starts one")
            session_terminal = wait_for(120, lambda: next(
                (win[0] for win in app_windows(MENU_APP_ID, "the terminal the compositor started")
                 if win[0] not in session_already), None))
            if not session_terminal:
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"the compositor started no {MENU_APP_ID} window: "
                     f"{without_console(output).strip()[-800:]!r}")
            # and an app from the Applications menu, which the shell starts in a scope of its own
            # named after the entry it came from
            open_from_menu(SESSION_APP, SESSION_APP_ID)

            # the column each one stands in comes with the window, so waiting for it is waiting for
            # the whole paragraph rather than for the first line of it
            session_found = session_until(
                60,
                lambda windows: all((session_of(app, windows) or {}).get("column")
                                    for app in (MENU_APP_ID, SESSION_APP_ID)),
                "the journal with both windows in it, each in a column of its own")
            session_spawned = session_of(MENU_APP_ID, session_found)
            session_started = session_of(SESSION_APP_ID, session_found)
            if not session_spawned or not session_started:
                _, output = run("journalctl --user -u lens -b -o cat -n 30 | cat", "the shell's log")
                fail(f"the journal names {[win.get('window') for win in session_found]}, expected "
                     f"{MENU_APP_ID} and an app id with {SESSION_APP_ID} in it: "
                     f"{without_console(output).strip()[-600:]!r}")

            # the entry each window came from. the app came from the menu, so it runs in a scope of
            # the shell's named after its entry and the journal has to say the same; the terminal is
            # in no such scope, so its entry was found from the app id its window carries, which is
            # the way the dock has always found an app
            session_open = app_windows(SESSION_APP_ID, "the app's window")
            if not session_open:
                fail(f"{SESSION_APP} is in the journal with no window of its own open")
            session_scopes = {name: window_scope(window, f"the scope of {name}")
                              for name, window in ((MENU_APP_ID, session_terminal),
                                                   (SESSION_APP_ID, session_open[0][0]))}
            if session_entry(session_scopes[MENU_APP_ID]):
                fail(f"the terminal the compositor started runs in {session_scopes[MENU_APP_ID]!r}, "
                     "which is a scope of the shell's, so this proves nothing about the app id")
            if session_spawned.get("app") != MENU_APP_ID:
                fail(f"the journal says the terminal came from {session_spawned.get('app')!r}, expected "
                     f"{MENU_APP_ID} found from the app id of a window in no scope of the shell's")
            session_named = session_entry(session_scopes[SESSION_APP_ID])
            if not session_named:
                fail(f"{SESSION_APP} runs in {session_scopes[SESSION_APP_ID]!r}, expected a scope of "
                     "the shell's own named after its entry")
            if session_started.get("app") != session_named:
                fail(f"the journal says {SESSION_APP} came from {session_started.get('app')!r}, where "
                     f"its scope says {session_named}")

            # and where each one stood. a new window opens as a column of its own, so the two are on
            # the same workspace in columns of their own, each the only window in its column
            for name, win in ((MENU_APP_ID, session_spawned), (SESSION_APP_ID, session_started)):
                for key in ("workspace", "screen", "column", "tile"):
                    if not win.get(key):
                        fail(f"the journal says {win} about {name}, with no {key} in it")
                if not win["workspace"].isdigit() or not win["column"].isdigit():
                    fail(f"the journal says workspace {win['workspace']!r} and column "
                         f"{win['column']!r} about {name}, expected numbers")
            if session_spawned["workspace"] != session_started["workspace"]:
                fail(f"the two windows are on workspaces {session_spawned['workspace']} and "
                     f"{session_started['workspace']}, where both opened on the one in front of the owner")
            if session_spawned["column"] == session_started["column"]:
                fail(f"the journal puts both windows in column {session_spawned['column']}, where a new "
                     "window opens as a column of its own")
            ok(f"the journal names {SESSION_APP} from the entry its scope names "
               f"({session_started['app']} from {session_scopes[SESSION_APP_ID]}, workspace "
               f"{session_started['workspace']}, column {session_started['column']}) and the terminal "
               f"the compositor started from its app id ({session_spawned['app']}, column "
               f"{session_spawned['column']})")

            # what a person reads: one row a window, the app named the way the Applications menu
            # names it, then where it stood, then what it was showing
            status, output = run("rift session", "what rift session prints")
            session_printed = without_console(output)
            if status != 0:
                fail(f"rift session exited with {status}: {session_printed.strip()[-300:]!r}")
            print(f"\nboot-test: rift session prints\n{session_printed}", flush=True)
            session_rows = [row for row in session_printed.splitlines() if row.strip()]
            if len(session_rows) < 2:
                fail(f"rift session printed {len(session_rows)} rows for the {len(session_found)} "
                     f"windows the journal names: {session_printed.strip()[-400:]!r}")
            # each app is named the way the Applications menu names it, from the entry the journal
            # keeps, not by the app id its window carries
            for named, column in ((SESSION_APP, session_started["column"]),
                                  (MENU_APP, session_spawned["column"])):
                if not any(row.startswith(f"{named} ") and f"column {column}" in row
                           for row in session_rows):
                    fail(f"rift session printed no row naming {named} in column {column}: "
                         f"{session_printed.strip()[-400:]!r}")
            ok(f"rift session prints {len(session_rows)} rows, each the name of the app from its "
               f"entry, where it stood and what it was showing: {session_rows[0].strip()!r}")

            # and a window that closes leaves the journal, which is what says the journal is what is
            # open now rather than everything that was ever opened
            close_app(SESSION_APP, SESSION_APP_ID)
            session_left = session_until(60, lambda windows: not session_of(SESSION_APP_ID, windows),
                                         "the journal after the app closed")
            if session_of(SESSION_APP_ID, session_left):
                fail(f"{SESSION_APP} is still in the journal after its window closed: "
                     f"{[win.get('window') for win in session_left]}")
            if not session_of(MENU_APP_ID, session_left):
                fail("the terminal left the journal with the app that closed: "
                     f"{[win.get('window') for win in session_left]}")
            ok(f"{SESSION_APP} left the journal when its window closed, and the terminal stayed in it")
            run(f"horizon msg action close-window --id {session_terminal}", "the terminal at the end")
            if not wait_for(60, lambda: not [win for win in app_windows(MENU_APP_ID, "the terminals left")
                                             if win[0] == session_terminal]):
                fail("the terminal the compositor started did not close")

            # 5u. the session coming back, which is P2.8 part 2 and the other half of teleport. the
            # journal of what is open is read at the first login after a boot and those apps are
            # started again, each on the workspace it was on and in the column it stood in. the
            # compositor has no action that opens an app in a column, and needs none: a new window
            # opens as a column of its own beside the one with the keyboard, so starting the apps one
            # at a time, workspace by workspace and left to right, puts the columns back. three
            # windows of three apps say it: a terminal and an app side by side on the first workspace,
            # and another app of its own on the second
            session_ours = []
            # a terminal the compositor spawned, which is in no scope of a session's, so the entry
            # the journal names it by comes from the app id its window carries
            session_already = {win[0] for win in app_windows(MENU_APP_ID, "the terminals open now")}
            run("horizon msg action spawn -- ghostty", "a terminal for the session to come back")
            session_terminal = wait_for(180, lambda: next(
                (win[0] for win in app_windows(MENU_APP_ID, "the terminal of the session")
                 if win[0] not in session_already), None))
            if not session_terminal:
                _, output = run("journalctl --user -b -o cat -n 30 | cat", "the user manager's log")
                fail(f"the compositor started no terminal for the session: "
                     f"{without_console(output).strip()[-800:]!r}")
            session_ours.append(session_terminal)
            # and two apps from the Applications menu, each in a scope of the shell's named after the
            # entry it came from
            for session_name, session_id in ((SESSION_APP, SESSION_APP_ID),
                                             (SESSION_OTHER, SESSION_OTHER_ID)):
                open_from_menu(session_name, session_id)
                session_window = app_windows(session_id, f"{session_name}'s window")
                if not session_window:
                    fail(f"{session_name} opened no window of its own")
                session_ours.append(session_window[0][0])
            # the last of them onto the second workspace, which is the empty one the compositor keeps
            # at the end of the row
            run(f"horizon msg action move-window-to-workspace --window-id {session_ours[-1]} 2",
                f"{SESSION_OTHER} onto the second workspace")

            def session_ours_now(what):
                """The windows of the terminal and the two apps, however they were opened."""
                return [win for win in open_windows(what)
                        if any(name in win[1].lower()
                               for name in (MENU_APP_ID, SESSION_APP_ID, SESSION_OTHER_ID))]

            def session_restored(said):
                """How many windows the shell says came back, or None while it has said no number."""
                number = said.get("restored") or ""
                return int(number) if number.isdigit() else None

            def session_places(windows):
                """What the journal says about where each window stood, which is what coming back
                has to put back: the entry, the workspace, the column and the place in it."""
                return sorted(tuple(win.get(key, "") for key in SESSION_PLACE) for win in windows)

            session_before = session_until(
                120,
                lambda windows: len(windows) == 3 and all(win.get("column") for win in windows)
                and {win.get("workspace") for win in windows} == {"1", "2"},
                "the journal with the three windows of the session in it")
            if len(session_before) != 3:
                fail(f"the journal names {[win.get('window') for win in session_before]}, expected the "
                     "terminal and the two apps")
            session_wanted = session_places(session_before)
            if len({place[1] for place in session_wanted}) != 2:
                fail(f"the journal puts all three windows on one workspace: {session_wanted}")
            ok(f"the journal of the session to come back: {session_wanted}")

            # the shell is stopped before the windows are closed, so the journal keeps them: a shell
            # that is not running writes nothing, which is what keeps the session through a shutdown
            run("systemctl --user stop lens", "the shell, stopped before the windows are closed")
            for window in session_ours:
                run(f"horizon msg action close-window --id {window}", f"closing window {window}")
            if not wait_for(120, lambda: not [win for win in open_windows("the windows left")
                                             if win[0] in session_ours]):
                fail("the three windows of the session did not close")
            session_kept = session_windows("the journal after the windows closed with the shell stopped")
            if session_places(session_kept) != session_wanted:
                fail(f"the journal says {session_places(session_kept)} after the windows closed with the "
                     f"shell stopped, expected the {session_wanted} it said before")
            # and one window of an app no machine has, to be passed over and said so
            run(f"printf '\\napp {SESSION_STRANGER}\\nworkspace 1\\ncolumn 4\\ntile 1\\n' >> {SESSION_JOURNAL}",
                "a window of an app this machine does not have, added to the journal")
            # the note a login leaves once its session is back, which a boot clears
            run(f"rm -f {SESSION_NOTE}", "the note that says this login has had its session back")
            run("systemctl --user start lens", "the shell again, which is the login coming back")

            session_state = wait_for(420, lambda: next(
                (said for said in [bar_state("what the shell says about the session coming back")]
                 if (session_restored(said) or 0) >= 3), None))
            if not session_state or session_restored(session_state) != 3:
                said = bar_state("what the shell says about the session once more")
                _, log = run("journalctl --user -u lens -b -o cat -n 40 | cat", "the shell's log")
                fail(f"lens --state says restored {said.get('restored')!r} and passed "
                     f"{said.get('passed')!r}, expected three windows back: "
                     f"{without_console(log).strip()[-1200:]!r}")
            if session_state.get("passed") != "1":
                fail(f"lens --state says passed {session_state.get('passed')!r}, expected the one window "
                     f"of {SESSION_STRANGER}, which nothing here opens")

            # and the journal of the session that came back says what the one it came from said
            session_after = session_until(
                120,
                lambda windows: len(windows) == 3 and all(win.get("column") for win in windows),
                "the journal of the session that came back")
            if session_places(session_after) != session_wanted:
                _, log = run("journalctl --user -u lens -b -o cat -n 40 | cat", "the shell's log")
                fail(f"the session came back as {session_places(session_after)}, where it left "
                     f"{session_wanted}: {without_console(log).strip()[-1200:]!r}")
            session_back = session_ours_now("the windows of the session that came back")
            if len(session_back) != 3:
                fail(f"the compositor has {len(session_back)} windows after the session came back: "
                     f"{session_back}")
            ok(f"the session came back: {session_places(session_after)}, with the window of "
               f"{SESSION_STRANGER} passed over")
            shot(f"{stem}-session-back{extension}", "session-back")

            # what a person reads about it, under the rows of `rift session`
            status, output = run("rift session", "what rift session says about the next login")
            session_said = without_console(output)
            if status != 0 or "These come back at the next login." not in session_said:
                fail(f"rift session exited with {status} and does not say the windows come back: "
                     f"{session_said.strip()[-400:]!r}")
            ok("rift session says these windows come back at the next login")

            # a shell that starts again inside the same login brings nothing back a second time: the
            # note is what says the session has already come back, and a crash is not a login
            run("systemctl --user restart lens", "the shell started again inside the same login")
            session_again = wait_for(120, lambda: next(
                (said for said in [bar_state("what the shell says after starting again")]
                 if said.get("restored")), None)) or {}
            if session_again.get("restored") != "none":
                fail(f"lens --state says restored {session_again.get('restored')!r} after the shell "
                     "started again inside the same login, expected none")
            session_twice = session_ours_now("the windows after the shell started again")
            if len(session_twice) != 3:
                fail(f"the shell brought the session back a second time: {len(session_twice)} windows "
                     f"where three were open")
            ok("a shell that starts again inside the same login brings nothing back")
            for window, _, _ in session_twice:
                run(f"horizon msg action close-window --id {window}", f"closing window {window} at the end")
            if not wait_for(120, lambda: not session_ours_now("the windows left")):
                fail("the windows of the session that came back did not close")

            # 5l. the photograph again, by its name, which the next boots of this drive keep. horizon
            # reads it while the gray stays up, then draws it without the shell starting again
            status, output = run(f"rift wallpaper set {WALLPAPER}", "the default wallpaper by its name")
            if status != 0 or f"The wallpaper is {WALLPAPER}." not in without_console(output):
                fail(f"rift wallpaper set {WALLPAPER} exited with {status}: {without_console(output).strip()[-300:]!r}")
            look("the default wallpaper again", f"{stem}-wallpaper-again{extension}", 30,
                 wallpaper=WALLPAPER_LEFT + WALLPAPER_RIGHT, journals=("horizon",))


    # 6. timeline. vault answers on the bus and a timer takes a snapshot of home every hour. take one,
    # change a file and delete another, find the snapshot through rift snapshot and on the bus,
    # and restore both from it
    _, output = run("systemctl is-active vault vault-timeline.timer", "the vault units")
    states = re.findall(r"^(active|inactive|failed|activating)\s*$", without_console(output), re.M)
    if states != ["active", "active"]:
        fail(f"vault and its timer are {states or without_console(output).strip()!r}, expected both active")
    _, output = run("systemctl show -p TimersCalendar --value vault-timeline.timer", "the timer's schedule")
    if "OnCalendar=*-*-* *:00:00" not in without_console(output):
        fail(f"vault-timeline.timer does not run every hour: {without_console(output).strip()!r}")

    snapshots = "/persist/@snapshots/home"
    notes, todo = "/home/rift/timeline/notes.txt", "/home/rift/timeline/todo.txt"

    def snapshot_list(what):
        """The names `rift snapshot` prints, oldest first."""
        status, output = run("rift snapshot", f"rift snapshot {what}")
        printed = without_console(output)
        print(f"\nboot-test: rift snapshot {what} printed:\n{printed}", flush=True)
        if status != 0:
            fail(f"rift snapshot exited with {status} {what}")
        return re.findall(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ)\s*$", printed, re.M)

    def contents(path):
        status, output = run(f"cat {path}", f"what is in {path}")
        return without_console(output) if status == 0 else f"nothing, cat exited with {status}"

    def restore(options, what):
        status, output = run(f"rift snapshot restore {options}", what)
        printed = without_console(output)
        print(f"\nboot-test: rift snapshot restore {options} printed:\n{printed}", flush=True)
        return status, printed

    status, output = run(f"mkdir -p (dirname {notes}); and printf 'First draft\\n' > {notes}; "
                         f"and printf 'Buy milk\\n' > {todo}", "the files for the snapshot")
    if status != 0:
        fail(f"the files for the snapshot could not be written: {without_console(output).strip()!r}")
    status, output = run("rift snapshot take", "rift snapshot take")
    taken = re.search(r"^Took snapshot (\S+Z)\.\s*$", without_console(output), re.M)
    if status != 0 or not taken:
        fail(f"rift snapshot take exited with {status}: {without_console(output).strip()!r}")
    snapshot = taken.group(1)
    _, output = run(f"sudo btrfs property get -ts {snapshots}/{snapshot} ro", "whether the snapshot is read only")
    if "ro=true" not in output:
        fail(f"{snapshots}/{snapshot} is not a read-only snapshot: {without_console(output).strip()!r}")
    ok(f"took snapshot {snapshot}, read only under {snapshots}")

    status, _ = run(f"printf 'Second draft\\n' > {notes}; and rm {todo}", "changing one file and deleting the other")
    if status != 0:
        fail("the files could not be changed")
    names = snapshot_list("after the changes")
    if snapshot not in names:
        fail(f"rift snapshot lists {names}, without {snapshot}")
    _, output = run("busctl --system --json=short call dev.rift.Vault /dev/rift/Vault dev.rift.Vault List",
                    "the snapshots on the bus")
    found = re.search(r'\{"type":"as","data":\[(\[[^\]]*\])\]\}', output)
    on_bus = json.loads(found.group(1)) if found else without_console(output).strip()
    if on_bus != names:
        fail(f"the bus lists {on_bus!r}, rift snapshot lists {names}")
    # the snapshot keeps home's permissions, so the owner reads their own files in it
    if "Buy milk" not in contents(f"{snapshots}/{snapshot}/rift/timeline/todo.txt"):
        fail("the owner cannot read the deleted file in the snapshot")
    ok(f"rift snapshot and the bus list {len(names)} snapshots with {snapshot}")

    status, printed = restore(f"{snapshot} {todo}", "restoring the deleted file")
    if status != 0 or f"Restored {todo} from {snapshot}." not in printed:
        fail(f"restoring the deleted file exited with {status}")
    if "Buy milk" not in contents(todo):
        fail(f"{todo} did not come back as it was")
    _, output = run(f"stat -c owner=%U:%a {todo}", "the owner of the restored file")
    if "owner=rift:644" not in output:
        fail(f"the restored file is not the owner's own: {without_console(output).strip()!r}")
    # without a terminal to ask on, a file that changed stays as it is
    status, printed = restore(f"{snapshot} {notes} </dev/null", "restoring the changed file without --replace")
    if status != 1 or f"{notes} has changed since this snapshot." not in printed or "--replace" not in printed:
        fail(f"restoring the changed file without --replace exited with {status}, expected 1 and a sentence")
    if "Second draft" not in contents(notes):
        fail(f"{notes} was overwritten without --replace")
    status, printed = restore(f"--replace {snapshot} {notes}", "restoring the changed file with --replace")
    if status != 0 or f"Replaced {notes} with the copy from {snapshot}." not in printed:
        fail(f"restoring the changed file with --replace exited with {status}")
    if "First draft" not in contents(notes):
        fail(f"{notes} is not the copy from the snapshot after --replace")
    status, printed = restore(f"{snapshot} {notes}", "restoring a file that is the same")
    if status != 0 or "Nothing was restored." not in printed:
        fail(f"restoring a file that is the same as in the snapshot exited with {status}")
    ok(f"restored {todo}, and {notes} only with --replace")

    # 6a. the schedule and the rules. the timer's service takes a snapshot the way the hour does. then
    # snapshots named by hand for January: 2026-01-05 and 2026-01-12 are Mondays. keeping one hour, one
    # day and two weeks keeps this week's first and the first of the week of the 12th, and drops the
    # rest of January
    status, output = run("sudo systemctl start vault-timeline.service", "the timer's snapshot")
    if status != 0:
        _, log = run("journalctl -u vault-timeline --no-pager -n 20", "the timer's log")
        fail(f"vault-timeline.service failed: {without_console(log).strip()[-800:]!r}")
    timed = [name for name in snapshot_list("after the timer's snapshot") if name not in names]
    if len(timed) != 1 or timed[0] <= snapshot:
        fail(f"vault-timeline.service added {timed}, expected one snapshot after {snapshot}")
    ok(f"vault-timeline.service took {timed[0]}")

    by_hand = ["2026-01-05T09:00:00Z", "2026-01-12T09:00:00Z", "2026-01-13T09:00:00Z", "2026-01-13T10:00:00Z"]
    status, output = run("; and ".join(f"sudo btrfs subvolume snapshot -r /persist/@home {snapshots}/{name}"
                                       for name in by_hand), "snapshots named by hand")
    before = snapshot_list("with the snapshots named by hand")
    if status != 0 or not set(by_hand) <= set(before):
        fail(f"the snapshots named by hand are not all there: {before}")
    status, output = run("sudo vault prune --hourly 1 --daily 1 --weekly 2", "the retention rules")
    printed = without_console(output)
    print(f"\nboot-test: vault prune printed:\n{printed}", flush=True)
    dropped = re.findall(r"^Dropped snapshot (\S+Z)\.\s*$", printed, re.M)
    past = {"2026-01-05T09:00:00Z", "2026-01-13T09:00:00Z", "2026-01-13T10:00:00Z"}
    if status != 0 or not past <= set(dropped) or "2026-01-12T09:00:00Z" in dropped or before[-1] in dropped:
        fail(f"vault prune exited with {status} and dropped {dropped}, expected {sorted(past)} and not "
             f"2026-01-12T09:00:00Z or the newest")
    left = snapshot_list("after the retention rules")
    if left != sorted(set(before) - set(dropped)):
        fail(f"rift snapshot lists {left} after the rules dropped {dropped} out of {before}")
    ok(f"the retention rules dropped {len(dropped)} of {len(before)} snapshots and kept {', '.join(left)}")

    # 6b. backup. the drive labelled backup is an empty ext4 disk. the test mounts it the way a desktop
    # would, chooses a folder on it and unmounts it: from then on vault finds the disk by uuid and
    # mounts it itself. back up home, change a file and delete another, restore both from the backup,
    # then look at the repository on the disk
    if args.backup:
        disk = "/run/backup-disk"
        folder = f"{disk}/Rift"
        letter, plan = "/home/rift/backup/letter.txt", "/home/rift/backup/plan.txt"
        words = "Kept in the backup 4127"

        def backup_cli(options, what):
            status, output = run(f"rift backup {options}", what)
            printed = without_console(output)
            print(f"\nboot-test: rift backup {options} printed:\n{printed}", flush=True)
            return status, printed

        def with_disk(what):
            status, output = run(f"sudo mkdir -p {disk}; and sudo mount /dev/disk/by-label/backup {disk}", what)
            if status != 0:
                fail(f"the backup disk could not be mounted for {what}: {without_console(output).strip()!r}")

        status, output = run(f"mkdir -p (dirname {letter}); and printf '{words}\\n' > {letter}; "
                             f"and printf 'Plan A\\n' > {plan}", "the files for the backup")
        if status != 0:
            fail(f"the files for the backup could not be written: {without_console(output).strip()!r}")
        with_disk("choosing the backup folder")
        status, output = run(f"sudo vault target {folder}", "sudo vault target")
        printed = without_console(output)
        print(f"\nboot-test: vault target printed:\n{printed}", flush=True)
        found = re.search(r"The password of these backups is ([0-9a-z]{5}(?:-[0-9a-z]{5}){4})\.", printed)
        if status != 0 or f"Backups of home go to {folder} now." not in printed or not found:
            fail(f"sudo vault target exited with {status} without the folder and a password")
        password = found.group(1)
        _, output = run("sudo stat -c key=%a:%U /var/lib/rift/vault/backup.key", "who can read the password")
        if "key=600:root" not in output:
            fail(f"the backup password is not only root's: {without_console(output).strip()!r}")
        status, _ = run(f"sudo umount {disk}", "unmounting the backup disk")
        if status != 0:
            fail("the backup disk could not be unmounted")
        ok(f"backups go to {folder}, with a password only root reads")

        status, printed = backup_cli("now", "backing up home")
        made = re.search(r"^Backed up home as ([0-9a-f]{8}) at (\S+Z)\.\s*$", printed, re.M)
        if status != 0 or not made:
            _, log = run("journalctl -u vault --no-pager -n 20", "vault's log")
            fail(f"rift backup now exited with {status}: {without_console(log).strip()[-800:]!r}")
        backup = made.group(1)
        _, output = run("echo left=(count (sudo ls -A /persist/@snapshots/backup))", "the snapshot the backup read")
        if "left=0" not in output:
            fail(f"the snapshot the backup read is still there: {without_console(output).strip()!r}")
        status, printed = backup_cli("list", "the backups")
        if status != 0 or not re.search(rf"^{backup}  {made.group(2)}\s*$", printed, re.M):
            fail(f"rift backup list exited with {status} without {backup} at {made.group(2)}")
        _, output = run("busctl --system --json=short call dev.rift.Vault /dev/rift/Vault dev.rift.Vault Backups",
                        "the backups on the bus")
        if f'"{backup}' not in output:
            fail(f"the bus does not list backup {backup}: {without_console(output).strip()!r}")
        ok(f"backed up home as {backup} at {made.group(2)}, and the snapshot it read is gone")

        status, _ = run(f"printf 'Plan B\\n' > {plan}; and rm {letter}", "changing one file and deleting the other")
        if status != 0:
            fail("the files could not be changed")
        status, printed = backup_cli(f"restore {backup} {letter}", "restoring the deleted file from the backup")
        if status != 0 or f"Restored {letter} from backup {backup}." not in printed:
            fail(f"restoring the deleted file from the backup exited with {status}")
        if words not in contents(letter):
            fail(f"{letter} did not come back from the backup as it was")
        _, output = run(f"stat -c owner=%U:%a {letter}", "the owner of the file from the backup")
        if "owner=rift:644" not in output:
            fail(f"the file from the backup is not the owner's own: {without_console(output).strip()!r}")
        status, printed = backup_cli(f"restore {backup} {plan} </dev/null", "restoring the changed file without --replace")
        if status != 1 or f"{plan} has changed since this backup." not in printed or "--replace" not in printed:
            fail(f"restoring the changed file from the backup without --replace exited with {status}, expected 1")
        if "Plan B" not in contents(plan):
            fail(f"{plan} was overwritten from the backup without --replace")
        status, printed = backup_cli(f"restore --replace {backup} {plan}", "restoring the changed file with --replace")
        if status != 0 or f"Replaced {plan} with the copy from backup {backup}." not in printed:
            fail(f"restoring the changed file from the backup with --replace exited with {status}")
        if "Plan A" not in contents(plan):
            fail(f"{plan} is not the copy from the backup after --replace")
        ok(f"restored {letter} from backup {backup}, and {plan} only with --replace")

        # the repository is rustic's, encrypted: a wrong password opens nothing, the printed one opens
        # it, and the text of the file is in none of its files
        with_disk("looking at the repository")
        _, output = run(f"sudo ls {folder}", "the repository's files")
        if not all(part in output for part in ("config", "data", "index", "keys", "snapshots")):
            fail(f"{folder} does not hold a rustic repository: {without_console(output).strip()!r}")
        status, output = run(f"sudo rustic -r {folder} --password not-the-password --no-cache --no-progress snapshots",
                             "the repository with a wrong password")
        if status == 0 or "incorrect" not in without_console(output):
            fail(f"rustic opened the repository with a wrong password, status {status}")
        status, output = run(f"sudo rustic -r {folder} --password {password} --no-cache --no-progress snapshots --json",
                             "the repository with the printed password")
        if status != 0 or f'"id": "{backup}' not in output:
            fail(f"the printed password does not open the repository, status {status}")
        status, output = run(f"sudo grep -r -l -F '{words}' {folder}", "the file's text in the repository")
        if status != 1:
            fail(f"grep exited with {status} looking for the file's text in the repository: {without_console(output).strip()!r}")
        status, _ = run(f"sudo umount {disk}", "unmounting the backup disk again")
        if status != 0:
            fail("the backup disk could not be unmounted again")
        ok("rustic refuses the repository with a wrong password and opens it with the printed one, "
           "and the file's text is in none of its files")

        # 6d. the Backups page over the same disk. step 5m saw the half of it with no folder chosen;
        # here there is one with a backup in it, and the button makes another. the window was closed
        # at the end of step 5m, so this opens it again on the page
        if args.desktop:
            on_disk = folder[len(disk):]
            run("systemd-run --user --quiet --collect -- rift-settings --page backups",
                "Settings on the Backups page again")
            backup_page = wait_for(300, lambda: next(
                (found for found in [settings_state("the Backups page with a disk behind it")]
                 if found.get("page") == "backups"
                 and found.get("backup-folder", "none") != "none"), None))
            if not backup_page:
                said = settings_state("the Backups page once more")
                _, log = run("journalctl -b -u vault --no-pager -n 20 -o cat | cat", "vault's journal")
                fail(f"the Backups page says backup-folder {said.get('backup-folder')!r} on the "
                     f"{said.get('page')!r} page, and backups go to {on_disk} on the backup disk: "
                     f"{without_console(log).strip()[-400:]!r}")
            if backup_page.get("backup-folder") != on_disk:
                fail(f"the Backups page says backups go to {backup_page.get('backup-folder')!r}, and "
                     f"sudo vault target chose {on_disk} on the disk")
            if backup_page.get("backups") != "1":
                fail(f"the Backups page says {backup_page.get('backups')!r} backups are on the disk, "
                     f"and rift backup now made one on this boot")
            shot(f"{stem}-settings-backups-disk{extension}", "backups-page")
            run("rift-settings --set backup now", "a backup made from the Backups page")
            page_backed_up = wait_for(600, lambda: next(
                (found for found in [settings_state("the backup the page made")]
                 if found.get("backing") == "off" and found.get("backups") == "2"), None))
            if not page_backed_up:
                said = settings_state("the backups after the page made one")
                _, log = run("journalctl -b -u vault --no-pager -n 20 -o cat | cat", "vault's journal")
                fail(f"the Backups page says {said.get('backups')!r} backups with backing "
                     f"{said.get('backing')!r} after it made one: {without_console(log).strip()[-400:]!r}")
            status, printed = backup_cli("list", "the backups after the page made one")
            if status != 0 or len(re.findall(r"^[0-9a-f]{8}  \S+Z\s*$", printed, re.M)) != 2:
                fail(f"rift backup list exited with {status} without the two backups")
            close_app(SETTINGS_APP, SETTINGS_APP_ID)
            ok(f"the Backups page says backups go to {on_disk} on the backup disk, and made the "
               f"second one of this boot from the page")

    # 6c. airlock. `rift run --sandbox` runs a command in bwrap, under landlock rules and a seccomp
    # filter. it gets the folder it runs in and the system's programs, nothing else of the owner's: not
    # the rest of home, not /persist, not a disk of the vm. what it writes outside its folder is gone
    # when it ends, and home as a whole goes in only read only
    home, sandbox = "/home/rift", "/home/rift/sandbox"
    secret, secret_words = "/home/rift/private.txt", "Kept out of the sandbox 5813"

    def sandboxed(command, what):
        status, output = run(command, what)
        printed = without_console(output)
        print(f"\nboot-test: {command} printed:\n{printed}", flush=True)
        return status, printed

    def said(printed, word):
        return re.search(rf"^{re.escape(word)}\s*$", printed, re.M) is not None

    status, output = run(f"mkdir -p {sandbox}; and printf '{secret_words}\\n' > {secret}", "the files for the sandbox")
    if status != 0:
        fail(f"the files for the sandbox could not be written: {without_console(output).strip()!r}")
    _, output = run("lsblk --nodeps --noheadings --output NAME", "the disks of the vm")
    disks = re.findall(r"^\s*((?:nvme|sd|vd)\w+)\s*$", without_console(output), re.M)
    if not disks:
        fail(f"lsblk lists no disks in the vm: {without_console(output).strip()!r}")

    # the folder it runs in is the one it gets
    status, printed = sandboxed(f"cd {sandbox}; and rift run --sandbox sh -c 'echo made > made.txt; "
                                f"grep -E \"^(NoNewPrivs|Seccomp):\" /proc/self/status; echo dev:; ls -A /dev; "
                                f"echo home:; ls -A {home}'", "a command in a sandbox")
    run("cd ~", "going home again")
    if status != 0:
        fail(f"rift run --sandbox exited with {status}")
    if not re.search(r"^NoNewPrivs:\s+1\s*$", printed, re.M) or not re.search(r"^Seccomp:\s+2\s*$", printed, re.M):
        fail("the sandboxed command does not run with no new privileges and a seccomp filter")
    listed = re.search(r"^dev:\s*$(.*)^home:\s*$(.*)", printed, re.M | re.S)
    if not listed:
        fail("the sandboxed command did not list /dev and home")
    devices = listed.group(1).split()
    seen = [name for name in devices if name.startswith(tuple(disks)) or name in ("disk", "mapper", "block")
            or name.startswith(("dm-", "loop"))]
    if "null" not in devices or seen:
        fail(f"/dev in the sandbox has {seen or devices}, expected no disks and a null device")
    if listed.group(2).split() != ["sandbox"]:
        fail(f"home in the sandbox holds {listed.group(2).split()}, expected only the folder it runs in")
    _, output = run(f"stat -c owner=%U:%a {sandbox}/made.txt; and cat {sandbox}/made.txt", "the file the sandbox made")
    if "owner=rift:644" not in output or not said(without_console(output), "made"):
        fail(f"the sandbox did not make {sandbox}/made.txt as the owner: {without_console(output).strip()!r}")
    ok(f"rift run --sandbox ran in {sandbox} with a seccomp filter, no disk in /dev and nothing else of home")

    # what it cannot reach. home and /tmp in the sandbox are empty and its own, the rest is not there
    status, printed = sandboxed(
        f"rift run --sandbox --folder {sandbox} sh -c 'test -e /persist && echo persist-there; "
        f"test -e /sys/block && echo sys-there; test -e /var/lib/rift && echo var-there; "
        f"cat {secret} && echo secret-read; cat /dev/{disks[0]} > /dev/null && echo disk-read; "
        f"echo out > {home}/outside.txt && echo home-written; echo out > /tmp/outside.txt && echo tmp-written; "
        f"echo renamed > /proc/self/comm && echo proc-written; unshare --user true; echo finished'",
        "what a sandbox cannot reach")
    if status != 0 or not said(printed, "finished"):
        fail(f"the sandboxed command exited with {status} before it finished")
    reached = [word for word in ("persist-there", "sys-there", "var-there", "secret-read", "disk-read", "proc-written")
               if said(printed, word)]
    if reached or secret_words in printed:
        fail(f"the sandbox reached what it must not: {reached or 'the words of ' + secret}")
    if not said(printed, "home-written") or not said(printed, "tmp-written"):
        fail("the sandbox could not write into its own empty home and /tmp")
    if "Operation not permitted" not in printed:
        fail("unshare in the sandbox was not refused by the seccomp filter")
    status, _ = run(f"test -e {home}/outside.txt -o -e /tmp/outside.txt", "whether what the sandbox wrote outside is there")
    if status == 0:
        fail("what the sandbox wrote outside its folder is still there after it ended")
    ok(f"the sandbox found no /persist, /sys or /var, could not read {secret} or /dev/{disks[0]} or write to /proc, "
       f"was refused a user namespace, and what it wrote outside {sandbox} was gone")

    # home as a whole, read only
    status, printed = sandboxed(f"rift run --sandbox --folder {sandbox} --read {home} sh -c 'cat {secret}; "
                                f"echo changed > {secret} && echo secret-written; echo new > {sandbox}/new.txt "
                                f"&& echo folder-written'", "a sandbox with home read only")
    if secret_words not in printed or said(printed, "secret-written") or not said(printed, "folder-written"):
        fail(f"with --read {home} the sandbox did not read {secret}, or wrote to it, or could not write to its folder")
    if secret_words not in contents(secret):
        fail(f"{secret} changed after a sandbox had it read only")
    ok(f"with --read {home} the sandbox read {secret} and could not change it")

    # what rift run refuses before anything runs
    for command, words in ((f"rift run --sandbox --folder {sandbox} --read /persist true",
                            "/persist cannot go into a sandbox."),
                           (f"rift run --sandbox --folder {sandbox} --read /dev/{disks[0]} true",
                            f"/dev/{disks[0]} cannot go into a sandbox."),
                           (f"rift run --sandbox --folder {home} true", f"{home} is all of your home folder."),
                           ("cd ~; and rift run --sandbox true", "that is all of your home folder."),
                           (f"sudo rift run --sandbox --folder {sandbox} true", "not as root."),
                           ("rift run true", "--sandbox is needed")):
        status, printed = sandboxed(command, f"what {command} refuses")
        if status not in (1, 2) or words not in " ".join(printed.split()):
            fail(f"{command} exited with {status} without saying {words!r}")
    ok("rift run refused /persist, a disk, all of home, root and a command without --sandbox")

    # 6d. the network switch. airlock keeps one for each app that runs in a sandbox, named after its
    # command or by --name. off cuts the network of the app's sandboxes that run now and of every one it
    # starts later, loopback included, and on gives it back. what is off stays off when airlock starts
    # again. the vm reaches a server this test runs on the host through qemu's user network, at 10.0.2.2
    served = tempfile.mkdtemp(prefix="rift-net-")
    net_words = "Reached the test server 2718"
    with open(os.path.join(served, "net.txt"), "w", encoding="utf-8") as f:
        f.write(net_words + "\n")

    class Quiet(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=served))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://10.0.2.2:{server.server_address[1]}/net.txt"
    fetch = f"curl -s -m 4 {url}"
    fetcher = "/home/rift/fetcher"

    def spaced(printed):
        return " ".join(printed.split())

    for _ in range(20):
        status, output = run(fetch, "the test server from the vm")
        if status == 0 and net_words in output:
            break
        time.sleep(3)
    else:
        _, output = run("ip -brief address; nmcli device | cat", "the network of the vm")
        fail(f"the vm does not reach the test server at {url}, curl exited with {status}: "
             f"{without_console(output).strip()!r}")
    status, output = run("systemctl is-active airlock", "whether airlock runs")
    if status != 0:
        fail(f"airlock is not running: {without_console(output).strip()!r}")
    status, printed = sandboxed("rift net", "the apps before any is off")
    if status != 0 or "Every app has the network" not in spaced(printed):
        fail(f"rift net exited with {status} before any app was off, or did not say every app has the network")
    run(f"mkdir -p {fetcher}", "the folder for the fetching sandboxes")
    status, printed = sandboxed(f"rift run --sandbox --folder {fetcher} --name fetcher {fetch}",
                                "a sandbox that reaches the test server")
    if status != 0 or net_words not in printed:
        fail(f"a sandbox did not reach the test server at {url}, it exited with {status}")
    ok(f"airlock runs, no app is off, and a sandbox reaches the test server at {url}")

    # a sandbox that goes on running. each time the test tells it to, it fetches and writes down what it got
    steps = ("for step in 1 2 3; do while ! test -e go-$step; do sleep 0.2; done; "
             f"curl -s -m 4 {url} > got-$step; echo $? > status-$step; done")
    status, output = run(f"rift run --sandbox --folder {fetcher} --name fetcher sh -c '{steps}' "
                         f"< /dev/null > {fetcher}/fetcher.log 2>&1 &; disown", "a sandbox that goes on running")
    if status != 0:
        fail(f"the sandbox that goes on running could not be started: {without_console(output).strip()!r}")

    def fetched(step):
        """Tells the running sandbox to fetch once more. Returns curl's exit status and what it got."""
        run(f"touch {fetcher}/go-{step}", f"telling the sandbox to fetch for step {step}")
        until = time.monotonic() + 40
        while time.monotonic() < until:
            _, output = run(f"cat {fetcher}/status-{step}", f"whether the sandbox fetched for step {step}")
            done = re.search(r"^(\d+)\s*$", without_console(output), re.M)
            if done:
                _, got = run(f"cat {fetcher}/got-{step}", f"what the sandbox got in step {step}")
                return int(done.group(1)), without_console(got)
            time.sleep(1)
        _, output = run(f"cat {fetcher}/fetcher.log", "what the running sandbox printed")
        fail(f"the running sandbox did not fetch for step {step}: {without_console(output).strip()!r}")

    code, got = fetched(1)
    if code != 0 or net_words not in got:
        fail(f"the running sandbox did not reach the test server before its network was off, curl exited with {code}")
    # into a pipe systemctl neither pages nor cuts the unit's name to the console's width
    _, output = run("systemctl --user list-units --full --plain --no-legend 'app-airlock-fetcher-*' | cat",
                    "the running sandbox's scope")
    units = re.findall(r"app-airlock-fetcher-\d+\.scope", without_console(output))
    print(f"\nboot-test: the user manager lists {units}", flush=True)
    if len(units) != 1:
        fail(f"the user manager lists {units} for fetcher, expected the one scope of the running sandbox")
    status, printed = sandboxed("rift net off fetcher", "turning fetcher's network off while it runs")
    if status != 0 or "The network is off for fetcher, also in the sandbox it runs in now." not in spaced(printed):
        fail(f"rift net off fetcher exited with {status} without saying it cut the running sandbox")
    status, printed = sandboxed("rift net", "the apps with fetcher off")
    if status != 0 or not re.search(r"^fetcher\s+Off\s+1\s*$", printed, re.M):
        fail(f"rift net does not list fetcher off with one sandbox running: {printed.strip()!r}")
    _, output = run("sudo nft list table inet airlock", "airlock's table")
    table = without_console(output)
    print(f"\nboot-test: sudo nft list table inet airlock printed:\n{table}", flush=True)
    if units[0] not in table:
        fail(f"airlock's table does not hold {units[0]}")
    cut, got = fetched(2)
    if cut == 0 or net_words in got:
        fail("the running sandbox reached the test server after its network was turned off")
    status, printed = sandboxed("rift net on fetcher", "turning fetcher's network on while it runs")
    if status != 0 or "The network is on for fetcher, also in the sandbox it runs in now." not in spaced(printed):
        fail(f"rift net on fetcher exited with {status} without saying it gave the running sandbox the network back")
    code, got = fetched(3)
    if code != 0 or net_words not in got:
        fail(f"the running sandbox did not reach the test server after its network was on again, curl exited with {code}")
    ok(f"rift net off cut the network of {units[0]} while it ran (curl exited with {cut}), and rift net on gave it back")

    # the next sandbox of an app that is off starts without the network. other apps keep theirs
    status, printed = sandboxed("rift net off fetcher", "turning fetcher's network off")
    if status != 0 or "The network is off for fetcher" not in spaced(printed):
        fail(f"rift net off fetcher exited with {status}")
    status, printed = sandboxed(f"rift run --sandbox --folder {fetcher} --name fetcher {fetch}",
                                "a new sandbox of fetcher while its network is off")
    if status == 0 or net_words in printed or "The network is off for fetcher." not in spaced(printed):
        fail(f"a new sandbox of fetcher exited with {status} while its network was off, or did not say it was off")
    status, printed = sandboxed(f"rift run --sandbox --folder {fetcher} {fetch}", "a sandbox of curl")
    if status != 0 or net_words not in printed:
        fail(f"a sandbox of curl did not reach the test server while fetcher's network was off, it exited with {status}")
    if args.models:
        # quasar's local api on 127.0.0.1. a sandbox without the network has no loopback either
        loopback = "curl -s -o /dev/null -m 4 -w 'code=%{http_code}' http://127.0.0.1:11434/v1/models"
        codes = []
        for app in ("fetcher", "curl"):
            _, printed = sandboxed(f"rift run --sandbox --folder {fetcher} --name {app} {loopback}",
                                   f"quasar's local api from a sandbox of {app}")
            found = re.search(r"code=(\d{3})", printed)
            codes.append(found.group(1) if found else None)
        if codes[0] != "000" or codes[1] in (None, "000"):
            fail(f"quasar's local api answered a sandbox of fetcher with {codes[0]} and one of curl with {codes[1]}, "
                 "expected no answer and an answer")
    ok("a new sandbox of fetcher started without the network while one of curl reached the test server"
       + (", and only curl's reached quasar's local api" if args.models else ""))

    # quasar is not an app of the switch. its unit keeps it off the network, which holds for anything in its cgroup
    status, _ = run("systemctl is-active quasar", "whether quasar runs")
    if status == 0:
        status, printed = sandboxed(f"sudo sh -c 'echo $$ > /sys/fs/cgroup/system.slice/quasar.service/cgroup.procs; "
                                    f"exec {fetch}'", "the test server from quasar's cgroup")
        if status == 0 or net_words in printed:
            fail("a process in quasar's cgroup reached the test server")
        ok(f"a process in quasar's cgroup does not reach the test server, curl exited with {status}")

    # what is off stays off when airlock starts again
    status, output = run("sudo systemctl restart airlock; and systemctl is-active airlock", "restarting airlock")
    if status != 0:
        fail(f"airlock did not start again: {without_console(output).strip()!r}")
    _, output = run("sudo cat /var/lib/rift/airlock/network-off", "the apps airlock keeps off")
    if not said(without_console(output), "fetcher"):
        fail(f"airlock's file does not hold fetcher: {without_console(output).strip()!r}")
    status, printed = sandboxed("rift net", "the apps after airlock started again")
    if status != 0 or not re.search(r"^fetcher\s+Off\s+\d+\s*$", printed, re.M):
        fail(f"rift net does not list fetcher off after airlock started again: {printed.strip()!r}")
    status, printed = sandboxed(f"rift run --sandbox --folder {fetcher} --name fetcher {fetch}",
                                "a sandbox of fetcher after airlock started again")
    if status == 0 or net_words in printed:
        fail(f"a sandbox of fetcher reached the test server after airlock started again, it exited with {status}")
    ok("fetcher's network stayed off when airlock started again")

    # what the switch refuses
    for command, words, codes in (("rift net off 'no/such'", "cannot be the name of an app.", (2,)),
                                  (f"rift run --sandbox --folder {fetcher} --name 'a b' true",
                                   "cannot be the name of an app.", (2,)),
                                  ("airlock start -- true", "is not in one. Nothing was run.", (126,))):
        status, printed = sandboxed(command, f"what {command} refuses")
        if status not in codes or words not in spaced(printed):
            fail(f"{command} exited with {status} without saying {words!r}")
    status, printed = sandboxed("sudo -u nobody busctl call dev.rift.Airlock /dev/rift/Airlock "
                                "dev.rift.Airlock SetNetwork sb fetcher true", "the switch turned by nobody")
    _, listed = sandboxed("rift net", "the apps after nobody tried the switch")
    if status == 0 or not re.search(r"^fetcher\s+Off\s+\d+\s*$", listed, re.M):
        fail(f"nobody turned fetcher's network on, busctl exited with {status}")
    status, printed = sandboxed("rift net on fetcher", "turning fetcher's network on")
    on_status, printed = sandboxed(f"rift run --sandbox --folder {fetcher} --name fetcher {fetch}",
                                   "a sandbox of fetcher with its network on")
    if status != 0 or on_status != 0 or net_words not in printed:
        fail(f"fetcher did not reach the test server after rift net on, which exited with {status}")
    server.shutdown()
    ok("rift net refused a name that is not an app's, airlock refused a start outside a sandbox's scope, "
       "nobody could not turn the switch, and fetcher's network came back")

    # 6e. Welcome installs an app, and flatpak runs it with the portals. the test's own runtime and app
    # are in a signed repository the host serves, which the test adds to the system installation as a
    # remote with its key, the way the image has flathub. Welcome, opened again as the Applications menu
    # opens it, lists the app once that remote is there, with the size the remote gives, and the test
    # ticks it and presses Install: flatpak's system helper installs the runtime and the app, which
    # polkit allows the owner, and a row says how far it has got. then the app runs in flatpak's
    # sandbox with the network and nothing of home: it reads a file of home only after the document
    # portal exported it for that app, it asks the desktop portal about the network over the session
    # bus, and the bus proxy keeps the rest of that bus from it. the serial shell has no graphical
    # session, so the portals come up by bus activation
    if args.flatpak:
        app_id = "dev.rift.TestApp"
        flatpak_stem, flatpak_extension = os.path.splitext(args.desktop or "desktop.png")
        flatpak_repo = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(Quiet, directory=os.path.abspath(args.flatpak)))
        threading.Thread(target=flatpak_repo.serve_forever, daemon=True).start()
        flatpak_base = f"http://10.0.2.2:{flatpak_repo.server_address[1]}"
        status, output = run(f"curl -sf -o ~/rift-test.gpg {flatpak_base}/key.gpg", "the test repository's key")
        if status != 0:
            fail(f"the vm did not get the test repository's key from {flatpak_base}: {without_console(output).strip()!r}")
        status, printed = sandboxed(f"sudo flatpak remote-add --system --gpg-import=$HOME/rift-test.gpg rift-test "
                                    f"{flatpak_base}/repo/", "the test repository as a remote of the system installation")
        if status != 0:
            fail(f"flatpak remote-add exited with {status}")
        # a unit of its own, so the window is not the serial shell's and the test can end it
        run("systemd-run --user --quiet --collect --unit=rift-welcome-apps rift-welcome --page apps",
            "Welcome from the Applications menu, on its Apps page")
        flatpak_listed = re.compile(rf"^app {re.escape(app_id)} (?!unknown)")
        welcome_now = welcome_until(
            180, lambda lines: welcome_value(lines, "page") == "apps"
            and any(flatpak_listed.match(printed) for printed in lines), f"{app_id} listed with its size")
        flatpak_size = next(printed for printed in welcome_now if flatpak_listed.match(printed)).split(" ", 2)[2]
        run(f"rift-welcome --set tick {app_id}", f"ticking {app_id}")
        welcome_until(30, lambda lines: app_id in (welcome_value(lines, "ticked") or "").split(","), f"{app_id} ticked")
        time.sleep(1)
        welcome_picture(f"{flatpak_stem}-welcome-ticked{flatpak_extension}", "welcome-ticked")
        run("rift-welcome --set install now", "Install")
        flatpak_seen = []
        flatpak_until = time.monotonic() + 300
        while True:
            welcome_now = welcome_said("how the install is going")
            flatpak_doing = next((printed.split(" ", 2)[2] for printed in welcome_now or []
                                  if printed.startswith(f"install {app_id} ")), None)
            if flatpak_doing and flatpak_doing not in flatpak_seen:
                flatpak_seen.append(flatpak_doing)
                if flatpak_doing.startswith("running") \
                        and not any(seen.startswith("running") for seen in flatpak_seen[:-1]):
                    welcome_picture(f"{flatpak_stem}-welcome-installing{flatpak_extension}", "welcome-installing")
            if flatpak_doing == "installed" or (flatpak_doing or "").startswith("failed"):
                break
            if time.monotonic() > flatpak_until:
                fail(f"Welcome's install of {app_id} did not finish in 300 s: {flatpak_seen}")
            time.sleep(1)
        if flatpak_doing != "installed":
            _, output = run("journalctl -b -u flatpak-system-helper -o cat --no-pager | tail -n 30",
                            "the system helper's log")
            fail(f"Welcome could not install {app_id}: {flatpak_doing}. The system helper said: "
                 f"{without_console(output).strip()[-1200:]!r}")
        welcome_picture(f"{flatpak_stem}-welcome-installed{flatpak_extension}", "welcome-installed")
        _, printed = sandboxed("flatpak list --system --columns=application,branch | cat",
                               "the system installation's flatpaks")
        for ref in ("dev.rift.TestPlatform", app_id):
            if not re.search(rf"^{re.escape(ref)}\s+test\s*$", printed, re.M):
                fail(f"flatpak list does not show {ref} on its test branch in the system installation")
        if f"app {app_id} installed" not in (welcome_said("the Apps page after the install") or []):
            fail(f"Welcome's Apps page does not say {app_id} is installed")
        ok(f"Welcome listed {app_id} at {flatpak_size}, installed it with its runtime into the system "
           f"installation ({', '.join(flatpak_seen)}), and flatpak list has both")

        # the Appearance page writes what Settings' does, and the shell draws in its accent
        if args.lens:
            for flatpak_accent in (SETTINGS_OTHER[0], SETTINGS_ACCENT[0]):
                run(f"rift-welcome --set accent {flatpak_accent}", f"the accent {flatpak_accent} from Welcome")
                flatpak_until = time.monotonic() + 30
                while bar_state("the shell's accent").get("accent") != flatpak_accent:
                    if time.monotonic() > flatpak_until:
                        fail(f"lens --state does not say accent {flatpak_accent} after Welcome set it")
                    time.sleep(2)
            ok(f"Welcome's accent {SETTINGS_OTHER[0]} reached the shell, and {SETTINGS_ACCENT[0]} again")
        # nothing is installing, so the close button ends it
        run("rift-welcome --set close now", "Welcome's close button")
        welcome_gone("the close button")
        flatpak_repo.shutdown()

        # and an installed flatpak is in the applications menu: it exports a desktop entry into the
        # system installation's folder, and the shell reads the entries again every time the menu opens
        if args.lens:
            run("lens --menu", "the applications menu after the install")
            run(f'lens --type "{FLATPAK_APP}"', "the flatpak's name typed into the field")
            listed = bar_state("the state with the flatpak's name in the field").get("rows")
            if listed != "1":
                fail(f"the menu lists {listed} rows for {FLATPAK_APP!r}, expected the flatpak alone")
            run("lens --escape", "escape in the field")
            run("lens --escape", "escape again, which closes the menu")
            ok(f"the applications menu lists the installed flatpak as {FLATPAK_APP!r}")

        # a file of home, exported for the app by the document portal. the app finds it under
        # /run/flatpak/doc and not where it is
        status, output = run(f"flatpak document-export --app={app_id} {secret}", "exporting a file of home for the app")
        exported = re.search(r"^/run/user/\d+/doc/(\w+)/private\.txt\s*$", without_console(output), re.M)
        if status != 0 or not exported:
            fail(f"flatpak document-export exited with {status}: {without_console(output).strip()!r}")
        document = f"/run/flatpak/doc/{exported.group(1)}/private.txt"
        # xdg-desktop-portal starts only in a graphical session. horizon's on tty1 is the owner's too
        status, output = run("systemctl --user is-active graphical-session.target", "whether horizon's session is up")
        if status != 0:
            fail("graphical-session.target is not active in the owner's user manager, so the desktop portal cannot "
                 f"start: {without_console(output).strip()!r}")

        def flatpak_app(options, what):
            # into a file first. the portals log to the console as they start, right while the app prints
            status, _ = run(f"flatpak run {options}{app_id} {document} {secret} > ~/flatpak-app.txt 2>&1", what)
            _, printed = sandboxed("cat ~/flatpak-app.txt", f"what {what} printed")
            if not said(printed, "finished"):
                fail(f"the flatpak app exited with {status} before it finished")
            return printed

        printed = flatpak_app("", "the flatpak app")
        if not said(printed, "document-read") or secret_words not in printed:
            fail(f"the flatpak app could not read {document}, which the document portal exported for it")
        if said(printed, "direct-read"):
            fail(f"the flatpak app read {secret} where it is in home")
        if not re.search(r"^\(true,\)\s*$", printed, re.M) or not said(printed, "portal-answered"):
            _, output = run("systemctl --user status xdg-desktop-portal | cat", "the desktop portal's unit")
            fail("the desktop portal did not tell the flatpak app that the network is there: "
                 f"{without_console(output).strip()[-1500:]!r}")
        if said(printed, "manager-answered"):
            fail("the bus proxy let the flatpak app reach the user manager")
        scope = re.search(rf"app-flatpak-{re.escape(app_id)}-\d+\.scope", printed)
        if not scope:
            fail("the flatpak app did not run in a scope of its own")
        _, output = run("systemctl --user list-units --full --plain --no-legend 'xdg-*portal*' | cat", "the portals")
        print(f"\nboot-test: the user manager runs {re.findall(r'xdg-[a-z-]+portal\.service', without_console(output))}",
              flush=True)
        ok(f"the flatpak app in {scope.group(0)} read {document} and not {secret}, the desktop portal said the "
           "network is there, and the bus proxy kept the user manager from it")

        # the portal asks what the app may do. without the network the network monitor does not answer it
        printed = flatpak_app("--unshare=network ", "the flatpak app without the network")
        if said(printed, "portal-answered") or "not available inside the sandbox" not in printed:
            fail("the desktop portal told the flatpak app about the network while it had none")
        # and the file is the app's only while it is exported
        status, _ = run(f"flatpak document-unexport {secret}", "taking the file back from the app")
        if status != 0:
            fail(f"flatpak document-unexport exited with {status}")
        printed = flatpak_app("", "the flatpak app after the file was taken back")
        if said(printed, "document-read") or secret_words in printed:
            fail(f"the flatpak app still read {document} after the document portal took it back")
        ok("the network monitor refused the app without the network, and the app lost the file when it was unexported")

    # 7. the update. the second drive holds two newer versions' files. systemd-sysupdate checks them
    # against SHA256SUMS, writes the store and its verity partition into the free slot under the
    # uuids in their names and puts the uki on the esp with three tries. then the vm reboots into it
    if args.updates:

        def mount_updates(directory, what):
            """Put the files of one version of the updates drive where sysupdate and the Updates
            page look for them, and return their names."""
            status, output = run(f"sudo mkdir -p {UPDATES_DRIVE} {UPDATES}; "
                                 f"and sudo mount -o ro /dev/disk/by-label/updates {UPDATES_DRIVE}; "
                                 f"and sudo mount --bind -o ro {UPDATES_DRIVE}/{directory} {UPDATES}; and ls -1 {UPDATES}",
                                 what)
            names = without_console(output).split()
            if status != 0:
                fail(f"{directory} on the updates drive could not be mounted on {UPDATES}: {without_console(output).strip()!r}")
            print(f"\nboot-test: {UPDATES} holds:\n" + "\n".join(names), flush=True)
            return names

        def umount_updates(what):
            status, output = run(f"sudo umount {UPDATES} {UPDATES_DRIVE}", what)
            if status != 0:
                print(f"\nboot-test: {what} exited with {status}: "
                      f"{without_console(output).strip()[-200:]!r}", flush=True)

        def install(directory, running, slot):
            """Install the version in this directory of the updates drive while running runs. Its
            partitions have to land in slot under the uuids in the file names, running stays in the
            other slot, and the uki is on the esp with all its tries. Returns the new version."""
            names = mount_updates(directory, f"the update files in {directory}")
            new = next((found.group(1) for found in (re.fullmatch(r"rift_([^_]+)\.efi", name) for name in names)
                        if found), None)
            if not new or version_key(new) <= version_key(running):
                fail(f"{directory} on the updates drive has no uki of a version after {running}: {names}")

            def uuid_in_name(kind):
                for name in names:
                    found = re.fullmatch(rf"rift_{re.escape(new)}_([0-9a-fA-F-]{{36}})\.{kind}(?:\.zst)?", name)
                    if found:
                        return found.group(1).lower()
                fail(f"{directory} on the updates drive has no {kind} file for {new}: {names}")

            verity_uuid, store_uuid = uuid_in_name("verity"), uuid_in_name("store")

            # the store is about 6G, written from the zstd file on the other drive
            started = time.monotonic()
            _, output = run("sudo systemd-sysupdate --verify=no update 2>&1 | tail -n 40; echo update-status=$pipestatus[1]",
                            f"systemd-sysupdate update to {new}")
            took = time.monotonic() - started
            printed = without_console(output)
            print(f"\nboot-test: systemd-sysupdate update printed:\n{printed}", flush=True)
            found = re.search(r"update-status=(\d+)", printed)
            if not found or found.group(1) != "0":
                fail(f"systemd-sysupdate update exited with {found.group(1) if found else 'no status'}")

            # sysupdate's current is the newest version installed, not the one running. a version
            # older than running was removed to make room
            _, output = run("sudo systemd-sysupdate --offline --json=short list", "systemd-sysupdate list after the update")
            found = re.search(r'^\{"current.*\}\s*$', without_console(output), re.M)
            listing = json.loads(found.group(0)) if found else {}
            if listing.get("current") != new or sorted(listing.get("all", [])) != sorted([running, new]):
                fail(f"systemd-sysupdate lists {without_console(output).strip()[-600:]!r} after the update, expected "
                     f"{new} current and {running} installed next to it")

            # all tries left and none done. systemd-boot takes one off each time it starts the file
            fresh = f"rift_{new}+{TRIES}-0.efi"
            ukis_on_esp([f"rift_{running}.efi", fresh], "after the update")

            # the table on the drive itself, udev may not have read the new labels yet
            _, output = run("sudo sfdisk --dump /dev/(lsblk -no PKNAME /dev/disk/by-designator/esp)",
                            "the partition table after the update")
            table = [(name, uuid.lower()) for uuid, name in
                     re.findall(r'uuid=([0-9A-Fa-f-]{36}), name="([^"]*)"', without_console(output))]
            wanted = [(f"store-verity_{new}", verity_uuid), (f"store_{new}", store_uuid)]
            written, kept = (table[1:3], table[3:5]) if slot == "a" else (table[3:5], table[1:3])
            if written != wanted or [name for name, _ in kept] != [f"store-verity_{running}", f"store_{running}"]:
                fail(f"the partitions after the update are {table}, expected {wanted} in slot {slot} and {running} "
                     f"in the other")
            umount_updates("unmounting the updates drive")
            ok(f"systemd-sysupdate installed {new} in {took:.0f}s: verity {verity_uuid} and store {store_uuid} in "
               f"slot {slot}, {fresh} on the esp")
            return new

        def reboot(what):
            child.send("sudo systemctl reboot\r")
            expect([PASSPHRASE], f"the luks passphrase prompt {what}")
            ok(f"passphrase prompt {what}")
            unlock()

        # 7b. the Updates page, over the same two slots. step 5m closed the window, so this opens
        # it again on the page. Vault reads the esp and the drive's partition table for it, which
        # only root can do, and the page reads all of it when it comes up, so a page that has to
        # say something new is left and opened again
        def updates_state(what, ready):
            """Wait for the Updates page to say something, leaving it and showing it again so it
            reads the drive afresh: it reads when it comes up and not after that. The window has to
            be open already, since rift-settings with no window opens one and does not come back."""
            run("rift-settings --page appearance", f"another page before {what}")
            run("rift-settings --page updates", f"the Updates page for {what}")
            return wait_for(180, lambda: next(
                (found for found in [settings_state(what)]
                 if found.get("page") == "updates" and ready(found)), None))

        if args.desktop:
            run("systemd-run --user --quiet --collect -- rift-settings --page updates",
                "Settings on the Updates page")
            updates_page = wait_for(300, lambda: next(
                (found for found in [settings_state("the slots on the Updates page")]
                 if found.get("page") == "updates" and found.get("version")), None))
            if not updates_page:
                said = settings_state("the Updates page once more")
                _, log = run("journalctl -b -u vault --no-pager -n 20 -o cat | cat", "vault's journal")
                fail(f"the Updates page says version {said.get('version')!r} on the {said.get('page')!r} "
                     f"page, and this drive runs {running}: {without_console(log).strip()[-400:]!r}")
            wanted_page = {"version": running, "slot": "a", "slot-a": running, "tries-a": "none",
                           "slot-b": "none", "tries-b": "none", "updates": UPDATES, "waiting": "none"}
            updates_wrong = {key: updates_page.get(key) for key, value in wanted_page.items()
                             if updates_page.get(key) != value}
            if updates_wrong:
                fail(f"the Updates page says {updates_wrong}, expected {wanted_page}")
            shot(f"{stem}-settings-updates{extension}", "updates-page")

            # and with the files of the next version in that folder it says which one is waiting.
            # the install mounts them again for itself
            updates_names = mount_updates("next", "the update files for the Updates page")
            updates_waiting = updates_state("the version waiting on the Updates page",
                                            lambda found: found.get("waiting", "none") != "none")
            umount_updates("unmounting the updates drive again")
            if not updates_waiting or version_key(updates_waiting["waiting"]) <= version_key(running):
                said = settings_state("the Updates page once more").get("waiting")
                fail(f"the Updates page says waiting {said!r} with {len(updates_names)} update files "
                     f"in {UPDATES}, expected a version after {running}")
            ok(f"the Updates page says {running} runs from slot a with slot b empty, updates come from "
               f"{UPDATES}, and {updates_waiting['waiting']} is waiting there")

        new = install("next", running, "b")

        # the same page after the install: the new version is in slot b with all its tries, and the
        # running one has not moved
        if args.desktop:
            updates_after = updates_state("the slots after the install",
                                          lambda found: found.get("slot-b", "none") != "none")
            wanted_page = {"version": running, "slot": "a", "slot-a": running, "tries-a": "none",
                           "slot-b": new, "tries-b": str(TRIES)}
            updates_wrong = {key: (updates_after or {}).get(key) for key, value in wanted_page.items()
                             if (updates_after or {}).get(key) != value}
            shot(f"{stem}-settings-updates-installed{extension}", "updates-page-installed")
            if updates_wrong:
                fail(f"the Updates page says {updates_wrong} after the install, expected {wanted_page}, "
                     f"see {stem}-settings-updates-installed{extension}")
            if updates_waiting["waiting"] != new:
                fail(f"the Updates page said {updates_waiting['waiting']} was waiting and sysupdate "
                     f"installed {new}")
            close_app(SETTINGS_APP, SETTINGS_APP_ID)
            ok(f"the Updates page says {new} is in slot b with {TRIES} tries and {running} still runs "
               f"from slot a")

        reboot_action("reset")
        reboot("after the update")
        reboot_action("shutdown")
        after = check_slots(slot="b", other=running)
        if after != new:
            fail(f"the vm came back running {after}, expected {new}")
        ok(f"rebooted into {new} from slot b, {running} stays in slot a")

        # 7a. the rollback. broken's boot check always fails. sysupdate writes it over running, the
        # oldest version, in slot a. none of its boots is marked good, so each start takes a try off
        # its uki, and once it has none left systemd-boot starts new from slot b again
        broken = install("broken", new, "a")
        reboot_action("reset")
        for done in range(1, TRIES + 1):
            reboot(f"for boot {done} of {broken}")
            check_failed_boot(broken, new, done)
        reboot(f"after {broken} used up its tries")
        reboot_action("shutdown")
        after = check_slots(slot="b", other=broken, failed=broken, counted=False)
        if after != new:
            fail(f"the vm came back running {after} after {TRIES} failed boots, expected {new}")
        ok(f"{broken} failed {TRIES} boots and {new} started again from slot b, sysupdate still lists {broken}")

    # 8. the clone. the vm has an empty scsi disk that says it is removable, the way a card reader or
    # a usb bridge does. rift clone refuses the drive this system runs from, a disk that is not
    # removable and a serial that is not the disk's, then writes the running drive onto the removable
    # disk with a passphrase of its own. before the vm goes down the test reads what it wrote: the
    # partition table, the store against its verity tree and the luks header. step 10 boots it
    if args.clone:
        clone_letter = "/home/rift/clone/letter.txt"
        clone_words = "Written before the clone 7051"
        status, output = run(f"mkdir -p (dirname {clone_letter}); and printf '{clone_words}\\n' > {clone_letter}",
                             "the file for the clone")
        if status != 0:
            fail(f"the file for the clone could not be written: {without_console(output).strip()!r}")
        cloned = image_version()

        def one_line(command, what, pattern):
            """The first match of pattern in what a command printed."""
            status, output = run(command, what)
            found = re.search(pattern, without_console(output), re.M)
            if status != 0 or not found:
                fail(f"{what}: {command} exited with {status}: {without_console(output).strip()[-400:]!r}")
            return found.group(1)

        uuid = r"^\s*([0-9a-fA-F-]{8,36})\s*$"
        boot = one_line("lsblk --noheadings --output PKNAME /dev/disk/by-designator/esp", "the boot drive", r"^\s*(\S+)\s*$")
        esp_uuid = one_line("lsblk --noheadings --output UUID /dev/disk/by-designator/esp", "the esp's uuid", uuid)
        machine = one_line("cat /etc/machine-id", "the machine id", r"^\s*([0-9a-f]{32})\s*$")
        usrhash = one_line("cat /proc/cmdline", "the usrhash", r"usrhash=([0-9a-f]{64})")
        # by-designator/usr is the verity device, not a partition. veritysetup names the two partitions
        running_store = one_line("sudo veritysetup status usr", "the store /usr runs from", r"data device:\s*(\S+)")
        running_verity = one_line("sudo veritysetup status usr", "the verity partition /usr runs from",
                                  r"hash device:\s*(\S+)")
        store_uuid = one_line(f"lsblk --noheadings --output PARTUUID {running_store}",
                              "the store's partition uuid", uuid).lower()
        verity_uuid = one_line(f"lsblk --noheadings --output PARTUUID {running_verity}",
                               "the verity partition's uuid", uuid).lower()
        first_persist = one_line(f"lsblk --list --noheadings --output PATH,PARTLABEL /dev/{boot}",
                                 "the persist partition of the boot drive", r"^\s*(\S+)\s+persist\s*$")
        first_luks = one_line(f"sudo cryptsetup luksUUID {first_persist}", "the uuid of persist", uuid).lower()
        first_snapshots = snapshot_list("before the clone")
        key_hash = None
        if args.backup:
            key_hash = one_line("sudo sha256sum /var/lib/rift/vault/backup.key", "the hash of the backup password",
                                r"^([0-9a-f]{64})\s")

        # the disks by serial. the clone's is the removable one qemu gave that serial: with --stick
        # the memory stick Files mounted is removable too, and nothing of this drive goes onto it
        _, output = run("lsblk --nodeps --bytes --pairs --output PATH,NAME,SERIAL,RM,TRAN,SIZE", "the disks of the vm")
        printed = without_console(output)
        print(f"\nboot-test: lsblk printed:\n{printed}", flush=True)
        disks = [fields for fields in (dict(re.findall(r'(\w+)="([^"]*)"', line)) for line in printed.splitlines())
                 if "PATH" in fields]
        removable = [disk for disk in disks if disk.get("RM") == "1"]
        wanted = [disk for disk in removable if disk.get("SERIAL") == "clone"]
        if len(wanted) != 1:
            fail(f"the vm has {len(removable)} removable disks and {len(wanted)} with the serial clone, "
                 "expected the one for the clone")
        target = wanted[0]["PATH"]
        serial = wanted[0].get("SERIAL") or wanted[0]["NAME"]
        by_id = one_line(f"for link in /dev/disk/by-id/*; if test (realpath $link) = {target}; echo link=$link; end; end",
                         "the clone's disk in /dev/disk/by-id", r"^link=(\S+)\s*$")

        def clone_cli(disk, typed, what):
            status, output = run(f"printf '%s\\n' '{CLONE_PASSPHRASE}' | sudo rift clone --serial '{typed}' {disk}", what)
            printed = without_console(output)
            print(f"\nboot-test: sudo rift clone --serial {typed} {disk} printed:\n{printed}", flush=True)
            return status, printed

        status, printed = clone_cli(f"/dev/{boot}", "rift", "a clone onto the drive this system runs from")
        if status != 1 or "is the drive this system runs from." not in printed:
            fail(f"rift clone onto the running drive exited with {status}, expected 1 and a refusal")
        if args.backup:
            backup_disk = next((disk["PATH"] for disk in disks if disk.get("SERIAL") == "backup"), None)
            if not backup_disk:
                fail("lsblk lists no disk with the serial backup")
            status, printed = clone_cli(backup_disk, "backup", "a clone onto a disk that is not removable")
            if status != 1 or "is neither removable nor on USB." not in printed:
                fail(f"rift clone onto the backup disk exited with {status}, expected 1 and a refusal")
        status, printed = clone_cli(by_id, f"not-{serial}", "a clone with a serial that is not the disk's")
        if status != 1 or f"is not the serial of {target}. Nothing was written." not in printed:
            fail(f"rift clone with a wrong serial exited with {status}, expected 1 and a refusal")
        _, output = run(f"lsblk --noheadings --list --output NAME {target}", "the clone's disk after the refusals")
        if len(without_console(output).split()) != 1:
            fail(f"{target} has partitions after rift clone refused it: {without_console(output).strip()!r}")
        ok("rift clone refused the running drive, a disk that is not removable and a wrong serial, and wrote nothing")

        started = time.monotonic()
        status, printed = clone_cli(by_id, serial, "rift clone")
        took = time.monotonic() - started
        if status != 0 or f"is a second drive now, with version {cloned} " not in printed:
            fail(f"rift clone exited with {status}")
        _, output = run("sudo ls -A /persist/@snapshots/clone", "the snapshots the clone sent")
        if without_console(output).strip():
            fail(f"the clone left snapshots behind: {without_console(output).strip()!r}")
        ok(f"rift clone wrote {cloned} onto {target} ({by_id}) in {took:.0f}s")

        # slot a holds the running version under the uuids its uki looks for, slot b is empty
        _, output = run(f"sudo sfdisk --dump {target}", "the clone's partition table")
        table = re.findall(r'^(\S+) : start=\s*\d+, size=\s*(\d+), type=([0-9A-Fa-f-]{36}), uuid=([0-9A-Fa-f-]{36}), '
                           r'name="([^"]*)"', without_console(output), re.M)
        print(f"\nboot-test: the clone's partitions: {table}", flush=True)
        sectors = 1024**3 // 512
        wanted = [("esp", ESP_TYPE, sectors), (f"store-verity_{cloned}", USR_VERITY_TYPE, sectors),
                  (f"store_{cloned}", USR_TYPE, 8 * sectors), ("_empty", USR_VERITY_TYPE, sectors),
                  ("_empty", USR_TYPE, 8 * sectors)]
        tail = (["exchange"] if args.exchange else []) + ["persist"]
        if [(name, kind.lower(), int(size)) for _, size, kind, _, name in table[:5]] != wanted \
                or [row[4] for row in table[5:]] != tail:
            fail(f"the clone's partitions are {table}, expected {wanted} and then {', '.join(tail)}")
        if (table[1][3].lower(), table[2][3].lower()) != (verity_uuid, store_uuid):
            fail(f"the clone's slot a has the uuids {table[1][3]} and {table[2][3]}, the running slot "
                 f"{verity_uuid} and {store_uuid}")
        clone_verity, clone_store, clone_persist = table[1][0], table[2][0], table[-1][0]
        if args.exchange:
            # as big as the first drive's, and an empty exfat of its own
            _, output = run(f"sudo blkid -p -o export {table[5][0]}", "the clone's exchange partition")
            found = without_console(output)
            if int(table[5][1]) * 512 != exchange_bytes or not re.search(r"^TYPE=exfat\s*$", found, re.M) \
                    or not re.search(r"^LABEL=EXCHANGE\s*$", found, re.M):
                fail(f"the clone's exchange partition is not an exfat of {exchange_bytes} bytes: {found.strip()!r}")
        status, output = run(f"sudo veritysetup verify {clone_store} {clone_verity} {usrhash}",
                             "the clone's store against its verity tree")
        if status != 0:
            fail(f"the clone's store does not match the usrhash: {without_console(output).strip()[-400:]!r}")
        ok(f"the clone's slot a holds {cloned} under the running uuids, its store matches the usrhash, slot b is empty")

        # persist. the first drive's passphrase does not open the clone's header and the clone's does.
        # the first drive's header opens with its own passphrase over the clone's data, and what that
        # reads is not a file system: the volume keys differ
        header = "/run/first-persist.header"
        status, _ = run(f"sudo rm -f {header}; and sudo cryptsetup luksHeaderBackup {first_persist} --header-backup-file {header}",
                        "the first drive's luks header")
        if status != 0:
            fail("the first drive's luks header could not be saved")

        def opens(options, secret, what, name=""):
            status, _ = run(f"printf '%s' '{secret}' | sudo cryptsetup open {options} --key-file - {clone_persist} {name}", what)
            return status == 0

        def signature(name):
            _, output = run(f"sudo blkid -p -o export /dev/mapper/{name}; sudo cryptsetup close {name}",
                            f"what {name} reads as")
            return without_console(output)

        if opens("--test-passphrase", passphrase, "the clone's header with the first drive's passphrase"):
            fail("the first drive's passphrase opens the clone's persist")
        if not opens("--test-passphrase", CLONE_PASSPHRASE, "the clone's header with its own passphrase"):
            fail("the passphrase the clone was made with does not open its persist")
        if not opens(f"--readonly --header {header}", passphrase, "the clone's data under the first drive's header",
                     "first-key"):
            fail("the first drive's saved header does not open with its passphrase")
        found = signature("first-key")
        if re.search(r"^TYPE=", found, re.M):
            fail(f"the clone's persist reads as {found!r} with the first drive's volume key")
        if not opens("--readonly", CLONE_PASSPHRASE, "the clone's data under its own header", "clone-key"):
            fail("the clone's persist does not open read only with its passphrase")
        found = signature("clone-key")
        if not re.search(r"^TYPE=btrfs\s*$", found, re.M) or not re.search(r"^LABEL=persist\s*$", found, re.M):
            fail(f"the clone's persist is not the btrfs labelled persist: {found!r}")
        clone_luks = one_line(f"sudo cryptsetup luksUUID {clone_persist}", "the uuid of the clone's persist", uuid).lower()
        if clone_luks == first_luks:
            fail(f"the clone's persist has the first drive's luks uuid {first_luks}")
        ok(f"the clone's persist {clone_luks} opens only with its own passphrase and has a volume key of its own")

    # 9. down
    power_off()

    # 10. the clone by itself. qemu starts again with only the clone's disk as its drive. the first
    # drive's passphrase is refused and the clone's opens it, the file from home is there, and the clone
    # runs the version that ran when it was made, from its own esp and slot a
    if args.clone:
        child.close()
        cmd = [
            os.path.abspath(args.vm),
            "--image", os.path.abspath(args.clone),
            "-smp", "2",
            "-m", args.memory,
            "-device", "virtio-vga",
            "-display", "none",
            "-monitor", "none",
            "-serial", "stdio",
            "-no-reboot",
        ]
        print("\nboot-test: " + " ".join(cmd), flush=True)
        child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace", dimensions=(40, 160))
        child.logfile_read = tee
        expect([PASSPHRASE], "the luks passphrase prompt of the clone")
        ok("passphrase prompt of the clone")
        child.send(passphrase + "\r")
        if expect([PROMPT, PASSPHRASE], "the clone to answer the first drive's passphrase") == 0:
            fail("the first drive's passphrase opened the clone")
        ok("the clone refused the first drive's passphrase")
        child.send(CLONE_PASSPHRASE + "\r")
        if expect([PROMPT, PASSPHRASE], "the autologin shell on the clone") == 1:
            fail("the clone refused the passphrase it was made with")
        ok("shell on the clone")

        if clone_words not in contents(clone_letter):
            fail(f"{clone_letter} is not on the clone")
        _, output = run(f"stat -c owner=%U:%a {clone_letter}", "the owner of the file on the clone")
        if "owner=rift:644" not in output:
            fail(f"the file on the clone is not the owner's own: {without_console(output).strip()!r}")
        ok(f"{clone_letter} is on the clone, the owner's own")

        after = check_slots(slot="a", counted=False)
        if after != cloned:
            fail(f"the clone runs {after}, expected {cloned}, the version that ran when it was made")
        if one_line("lsblk --noheadings --output UUID /dev/disk/by-designator/esp", "the clone's esp uuid", uuid) == esp_uuid:
            fail(f"the clone booted from an esp with the first drive's uuid {esp_uuid}")
        if one_line("cat /etc/machine-id", "the clone's machine id", r"^\s*([0-9a-f]{32})\s*$") == machine:
            fail(f"the clone has the first drive's machine id {machine}")
        if one_line("sudo cryptsetup luksUUID /dev/disk/by-partlabel/persist", "the uuid of persist on the clone",
                    uuid).lower() != clone_luks:
            fail(f"the clone unlocked a persist that is not {clone_luks}")
        _, output = run("sudo find /persist/@snapshots -mindepth 1 -maxdepth 2", "the snapshots on the clone")
        carried = [name for name in first_snapshots if name in output]
        if carried:
            fail(f"the clone has the first drive's snapshots {carried}")
        if key_hash and one_line("sudo sha256sum /var/lib/rift/vault/backup.key", "the backup password on the clone",
                                 r"^([0-9a-f]{64})\s") != key_hash:
            fail("the clone does not have the backup password of the first drive")
        ok(f"the clone booted {cloned} from its own esp, with a machine id of its own, none of the first drive's "
           f"snapshots{' and its backup password' if key_hash else ''}")
        power_off()

    print(f"\nboot-test: PASSED in {since()}", flush=True)


if __name__ == "__main__":
    main()
