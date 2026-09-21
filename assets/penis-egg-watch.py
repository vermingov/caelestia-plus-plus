#!/usr/bin/env python3
"""Desktop easter eggs: typing a trigger word while no window is focused
pops a surprise via the caelestia shell IPC (one target per egg).

Reads keyboard evdev devices directly (needs read access to /dev/input:
`input` group, or a setfacl grant). Only the last few letter keycodes
are held in memory - nothing is logged, stored, or sent anywhere.
"""

import fcntl
import json
import os
import select
import signal
import struct
import subprocess
import sys
import time

EVENT_FMT = "llHHi"  # struct input_event on 64-bit: timeval + type/code/value
EVENT_SIZE = struct.calcsize(EVENT_FMT)
EV_KEY = 1
KEY_DOWN = 1

# qwerty letter rows (KEY_Q..KEY_P, KEY_A..KEY_L, KEY_Z..KEY_M)
LETTER_CODES = set(range(16, 26)) | set(range(30, 39)) | set(range(44, 51))
# trigger word (as keycodes) -> shell IPC target to call `pop` on
SEQUENCES = {
    (25, 18, 49, 23, 31): "easterEgg",          # p-e-n-i-s
    (23, 31, 19, 30, 18, 38): "israelEgg",      # i-s-r-a-e-l
}
LONGEST_SEQUENCE = max(len(seq) for seq in SEQUENCES)

RESCAN_SECONDS = 15
COOLDOWN_SECONDS = 5


def keyboard_event_paths():
    """Devices with a kbd handler; non-keyboards there never emit letters."""
    paths = []
    with open("/proc/bus/input/devices") as f:
        for line in f:
            if line.startswith("H: Handlers=") and "kbd" in line:
                for token in line.split():
                    if token.startswith("event"):
                        paths.append("/dev/input/" + token)
    return paths


def open_keyboards(test=False):
    files = []
    for path in keyboard_event_paths():
        try:
            files.append(open(path, "rb", buffering=0))
            if test:
                print(f"[test] opened {path}", flush=True)
        except OSError as e:
            # not readable (yet) - picked up on a later rescan
            if test:
                print(f"[test] FAILED to open {path}: {e}", flush=True)
    return files


def hyprctl_json(*args):
    out = subprocess.run(["hyprctl", *args, "-j"],
                         capture_output=True, text=True, timeout=2).stdout
    return json.loads(out)


def cursor_workspace_is_empty():
    """Multi-monitor fallback: a window on another screen can hold focus
    while the user types at an empty desktop, so also accept an empty
    workspace on the monitor under the cursor."""
    try:
        cur = hyprctl_json("cursorpos")
        for m in hyprctl_json("monitors"):
            scale = m.get("scale", 1) or 1
            if m["x"] <= cur["x"] < m["x"] + m["width"] / scale \
                    and m["y"] <= cur["y"] < m["y"] + m["height"] / scale:
                ws_id = m.get("activeWorkspace", {}).get("id")
                return any(w["id"] == ws_id and w.get("windows", 1) == 0
                           for w in hyprctl_json("workspaces"))
    except (subprocess.SubprocessError, OSError, json.JSONDecodeError, KeyError):
        pass
    return False


def desktop_is_focused():
    try:
        out = subprocess.run(["hyprctl", "activewindow", "-j"],
                             capture_output=True, text=True, timeout=2).stdout
    except (subprocess.SubprocessError, OSError):
        return False
    try:
        if "address" not in json.loads(out):
            return True
    except json.JSONDecodeError:
        # hyprctl prints "Invalid" (not JSON) when nothing is focused
        if "Invalid" in out:
            return True
    return cursor_workspace_is_empty()


# What each one is called at cae's door, which is asked first: cae draws
# these now where it is installed, and the QML shell where it is not.
DOOR_VERBS = {"easterEgg": "egg", "israelEgg": "cinema"}


def pop_egg(target):
    # Never let a hung or missing shell kill the watcher — nothing restarts it
    asks = [["cae-shell", DOOR_VERBS[target]],
            ["qs", "-c", "caelestia", "ipc", "call", target, "pop"]]
    for ask in asks:
        try:
            if subprocess.run(ask, capture_output=True, timeout=5).returncode == 0:
                return
        except (subprocess.SubprocessError, OSError):
            continue


def matched_target(recent):
    for sequence, target in SEQUENCES.items():
        if tuple(recent[-len(sequence):]) == sequence:
            return target
    return None


def lock_holder_pid(lock_path):
    """PID holding the flock, read straight from /proc/locks by the lock
    file's inode - identifies the holder exactly, whatever its cmdline.
    Device numbers are deliberately NOT compared: on btrfs, stat() gives
    the subvolume's anonymous device while /proc/locks shows the
    superblock's, so they legitimately differ."""
    try:
        ino = os.stat(lock_path).st_ino
        with open("/proc/locks") as f:
            for line in f:
                parts = line.split()
                # e.g.: 12: FLOCK ADVISORY WRITE 4242 08:03:1234567 0 EOF
                if "FLOCK" not in parts:
                    continue
                if int(parts[-3].split(":")[2]) == ino:
                    return int(parts[-4])
    except (OSError, ValueError, IndexError):
        pass
    return None


def looks_like_watcher(pid):
    """Sanity guard before eviction: inode matching is filesystem-local,
    so double-check the holder is actually some copy of this watcher."""
    try:
        comm = open(f"/proc/{pid}/comm").read().strip()
        if comm.startswith("python") or comm.startswith("penis-egg"):
            return True
        return "egg-watch" in open(f"/proc/{pid}/cmdline", "rb").read().decode()
    except OSError:
        return False


def is_same_script(pid):
    """True when PID runs from this very file (a plain duplicate spawn)."""
    me = os.path.realpath(__file__)
    try:
        args = open(f"/proc/{pid}/cmdline", "rb").read().decode().split("\0")
    except OSError:
        return False
    for arg in args:
        try:
            if arg and os.path.realpath(arg) == me:
                return True
        except OSError:
            continue
    return False


def acquire_single_instance_lock():
    """The shell spawns this at startup and users may also autostart a
    copy. A same-path duplicate exits like before, but a holder running
    from a DIFFERENT copy gets evicted and the lock taken over - the
    shell's post-update spawn always carries the newest sequences, and
    old copies predate this code so they can never evict back."""
    lock_dir = os.path.expanduser("~/.local/state/caelestia")
    os.makedirs(lock_dir, exist_ok=True)
    lock_path = os.path.join(lock_dir, "egg-watch.lock")
    lock = open(lock_path, "w")
    for _ in range(12):
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return lock
        except OSError:
            pass
        holder = lock_holder_pid(lock_path)
        if holder is None:
            time.sleep(0.25)
            continue
        if is_same_script(holder):
            raise SystemExit(0)
        if not looks_like_watcher(holder):
            raise SystemExit(0)
        try:
            os.kill(holder, signal.SIGTERM)
        except OSError:
            # not ours to kill (e.g. another user's) - yield to it
            raise SystemExit(0)
        time.sleep(0.25)
    raise SystemExit(0)


def main(test=False):
    # --test: no lock (runs alongside the live watcher), no pop — prints what
    # the watcher sees so "types but nothing happens" can be diagnosed
    lock = None if test else acquire_single_instance_lock()
    keyboards = open_keyboards(test)
    if test:
        print(f"[test] {len(keyboards)} device(s) open; every keypress prints below. Ctrl-C to quit", flush=True)
    recent = []
    last_scan = time.monotonic()
    last_pop = 0.0

    while True:
        if time.monotonic() - last_scan > RESCAN_SECONDS:
            # Reopen only when the device set actually changed (hotplug)
            if set(keyboard_event_paths()) != {f.name for f in keyboards}:
                for f in keyboards:
                    f.close()
                keyboards = open_keyboards()
            last_scan = time.monotonic()

        if not keyboards:
            time.sleep(RESCAN_SECONDS)
            last_scan = 0
            continue

        readable, _, _ = select.select(keyboards, [], [], RESCAN_SECONDS)
        for f in readable:
            try:
                data = f.read(EVENT_SIZE * 64)
            except OSError:
                keyboards.remove(f)
                f.close()
                continue
            if not data:
                continue

            for i in range(0, len(data) - EVENT_SIZE + 1, EVENT_SIZE):
                _, _, etype, code, value = struct.unpack_from(EVENT_FMT, data, i)
                if etype != EV_KEY or value != KEY_DOWN:
                    continue
                if test:
                    print(f"[test] keydown code={code} {'letter' if code in LETTER_CODES else 'other'} from {f.name}", flush=True)
                if code not in LETTER_CODES:
                    recent.clear()
                    continue
                recent.append(code)
                del recent[:-LONGEST_SEQUENCE]
                target = matched_target(recent)
                if target is None:
                    continue
                if test:
                    print(f"[test] SEQUENCE DETECTED ({target}); gate desktop_is_focused() = {desktop_is_focused()}", flush=True)
                    recent.clear()
                    continue
                if time.monotonic() - last_pop > COOLDOWN_SECONDS:
                    recent.clear()
                    if desktop_is_focused():
                        last_pop = time.monotonic()
                        pop_egg(target)


if __name__ == "__main__":
    main(test="--test" in sys.argv)
